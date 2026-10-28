//! 单态化的**实例命名**与**待实例化队列**（M12/Task 13）。
//!
//! # 这一层只做两件事
//!
//! 1. 给 `(origin, args)` 起一个**稳定、可读、不撞名**的实例名；
//! 2. 记住谁要实例化、按入队序吐出来（[`Mono::request`] 的返回值就是
//!    「要不要新造一份」）。
//!
//! **不做**代码生成、不做替换、不碰 `TypeTable` 的实例 memo —— 那是 Task 14/15。
//!
//! # 为什么名字是 `String` 而不是 id
//!
//! 实例是**新造的函数**，它要出现在 HIR 的 `Func` 表、SSA 的符号、最终的 ELF
//! 里；那些地方只有名字。用 id 会在跨越「类型表 → HIR → 后端」时断掉，
//! 而名字在任何一层都能被读出来。代价是名字必须自己保证唯一 —— 见下。
//!
//! # 命名的分隔符（一处必须说清的设计）
//!
//! | 分隔符 | 用在哪 | 为什么不能混用 |
//! |---|---|---|
//! | `·`（U+00B7） | **实参之间**、`origin` 与第一个实参之间 | 它是 Go 标识符**不允许**的字符 ⇒ 用户符号不可能冒充它 |
//! | `.` | **复合类型内部**（`ptr.int` / `slice.int` / `struct.a.int`） | 同上 |
//!
//! 两种分隔符都必要。只用 `·` 时，`F[[]int]`（1 个实参，`F·slice·int`）与
//! `F[slice, int]`（2 个实参，`F·slice·int`）会**撞名** —— 而 `slice` 是个
//! 完全合法的用户类型名。把复合内部换成 `.` 之后，前者是 `F·slice.int`、
//! 后者是 `F·slice·int`，实参个数一眼可数。
//!
//! # 名字里为什么不能有 `_` / `$`
//!
//! `_` 与 Go 的「空标识符」约定混淆（诊断里 `_` 有确定含义），`$` 是汇编器
//! 保留字符。⚠️ 这条**不是**恒真的：`BasicId::as_str()` 对 `unsafe.Pointer`
//! 返回 `unsafe_pointer`、对 `untyped_*` 返回 `untyped_int` … —— 唯一的
//! 强制点就是本文件的 [`push_ident`]，漏了它下划线就一路漏到符号表。
//!
//! # 哪些类型是**结构性**命名，哪些不是（如实记）
//!
//! 结构性 = 「`identical` 的两个类型必同名」。它是 Task 14 那个不动点能收敛的
//! 前提：`add_struct` **不去重**，每个调用点重新解析一次 `struct{}` 就得到新的
//! `StructId`；名字若跟着 `StructId` 走，`request` 的 memo 虽然按 `identical`
//! 命中，**名字却不同** ⇒ 每轮都在造新实例。
//!
//! | 类型 | 命名 | 结构性的代价 |
//! |---|---|---|
//! | `basic` / `named` / `pointer` / `slice` / `array` / `map` / `chan` / `struct` | **结构性** | — |
//! | `interface`（匿名字面量） | 条目下标（`iface3`） | 结构相同的两个匿名接口会各造一份实例 —— **多造**，不是错误合并 |
//! | `signature` / `tuple` / `union` / `typeparam` | 条目下标 | 同上；这四类在 Go 里**不能**作值类型的实参，实际走不到 |
//!
//! 「多造」与「错误合并」的代价不对称，是本仓一贯的取法：合并会让两个不同的
//! 函数悄悄共享一份代码，多造只是浪费。
//!
//! 另一处残留：**结构体标签**只把字母数字拼进名字，纯标点差异（`` `a:"x"` ``
//! 与 `` `a:'x'` ``）会撞。标签出现在泛型实参上的概率极低，且撞了也是
//! 「多造 / 合并各一份」—— 与上面同一档，不另设机制。

use std::collections::VecDeque;

use rgoc_const::Const;
use rgoc_types::{ChanDir, Type, TypeId, TypeTable, identical, unalias};

/// 实参之间的分隔符（U+00B7）。Go 标识符不允许这个字符 ⇒ 不可冒充。
const ARG_SEP: char = '\u{00b7}';

/// 复合类型内部的分隔符。`.` 同样不可能出现在标识符里。
const INNER_SEP: char = '.';

/// 标签渲染的深度上限。
///
/// 递归不会成环（`Named`/`Alias` 只有名字、不展开本体；匿名结构体的自引用
/// 在 Go 里写不出来），但**深度**可以很大 —— `valid_type` 的上限是 1000。
/// 64 层足够覆盖一切真实程序，超了就退化成「按下标命名」：代价是那一档不再
/// 结构性（多造实例），不会崩栈。
const MAX_LABEL_DEPTH: usize = 64;

/// 泛型函数的身份：`rgoc-load` 里 `FuncDecl` 的下标。
///
/// **不是** `TypeId` —— 函数没有类型条目，只有签名（签名进表，函数不进）。
pub type FuncKey = usize;

/// 一个待实例化的请求。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InstRequest {
    /// 被实例化的泛型函数。
    pub origin: FuncKey,
    /// 实参（已按 [`unalias`] 规范化 ⇒ 别名与底层是同一次请求）。
    pub args: Vec<TypeId>,
    /// 实例名（[`mangled`] 的产物）。
    pub name: String,
    /// **实例化链的深度**：从包级函数体（深度 0）里直接请求的是 1，
    /// 从深度 1 的实例体里请求的是 2，依此类推。
    ///
    /// 为什么需要它（而不是只数总条数）：`test/typeparam/issue48711.go` 的
    /// ```go
    /// func f[T interface{ ~[]P }, P any](t T) { f[[]T, T]([]T{t}) }
    /// func main() { f[[]int](nil) }
    /// ```
    /// 每一层实参都不同（`[]int` → `[][]int` → …）⇒ **memo 永远不命中**。
    /// 只按条数封顶时，一万条实例每条都要再检查一遍体，容器直接被 OOM 杀掉
    /// （实测 exit 137）。按**深度**封顶才是官方的口径（实例化**嵌套**太深 =
    /// 环），代价与宽度无关。
    pub depth: usize,
}

/// 待实例化队列 + 已请求过的 memo。
#[derive(Default, Debug)]
pub struct Mono {
    /// `FuncKey` → 泛型函数的名字（登记序即 `FuncKey`）。
    origins: Vec<String>,
    /// 待实例化（FIFO）。
    queue: VecDeque<InstRequest>,
    /// 已请求过的 `(origin, args)`；`args` 已规范化。
    seen: Vec<(FuncKey, Vec<TypeId>)>,
    /// 当前正在检查的实例深度（0 = 包级函数体）。由调用方在每轮
    /// [`Self::take`] 之后用 [`Self::set_depth`] 摆好。
    cur_depth: usize,
}

impl Mono {
    /// 空队列。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个泛型函数（**必须按 `FuncDecl` 的声明序调用**），返回它的 [`FuncKey`]。
    pub fn declare(&mut self, name: impl Into<String>) -> FuncKey {
        let key = self.origins.len();
        self.origins.push(name.into());
        key
    }

    /// `FuncKey` 对应的函数名（诊断与命名用）。
    #[must_use]
    pub fn origin_name(&self, key: FuncKey) -> Option<&str> {
        self.origins.get(key).map(String::as_str)
    }

    /// 队列里还有多少条待办。
    #[must_use]
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// 请求实例化 `(origin, args)`。**返回 `true` 表示是新请求**（已入队）；
    /// `false` 表示 memo 命中（之前已经请求过，调用方不要重复造）。
    ///
    /// 实参先过 [`unalias`]：官方语义里别名**不是**独立类型，`type MyInt = int`
    /// 与 `int` 必须共用同一份实例，否则语义一样、代码两份。
    ///
    /// # Panics
    ///
    /// `origin` 不是 [`Self::declare`] 产出的下标时 panic —— 那是调用方的
    /// 缺陷（拿错下标），不是运行时输入。静默返回 `false` 会让实例**永远不产**，
    /// 而症状要到链接期才出现。
    pub fn request(&mut self, tt: &mut TypeTable, origin: FuncKey, args: &[TypeId]) -> bool {
        let name = self
            .origins
            .get(origin)
            .unwrap_or_else(|| panic!("Mono::request 收到未登记的 FuncKey {origin}"))
            .clone();
        let norm: Vec<TypeId> = args.iter().map(|&a| unalias(&*tt, a)).collect();
        if self
            .seen
            .iter()
            .any(|(k, a)| *k == origin && same_args(tt, a, &norm))
        {
            return false;
        }
        let name = mangled(tt, &name, &norm);
        self.seen.push((origin, norm.clone()));
        self.queue.push_back(InstRequest {
            origin,
            args: norm,
            name,
            depth: self.cur_depth + 1,
        });
        true
    }

    /// 摆好「当前正在检查的实例深度」（见 [`InstRequest::depth`]）。
    pub fn set_depth(&mut self, d: usize) {
        self.cur_depth = d;
    }

    /// 当前深度（诊断与测试用）。
    #[must_use]
    pub fn depth(&self) -> usize {
        self.cur_depth
    }

    /// 取下一个待实例化请求（FIFO）。
    pub fn take(&mut self) -> Option<InstRequest> {
        self.queue.pop_front()
    }
}

/// 两组实参是否同一（按 [`identical`]；比较失败时按**不同**处理）。
///
/// 判失败（`Err`）保守地当成「不同」：宁可多造一份实例，也不能把两个不同的
/// 类型合并成一个函数。
fn same_args(tt: &TypeTable, a: &[TypeId], b: &[TypeId]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x == y || matches!(identical(tt, *x, *y), Ok(true)))
}

/// 实例名：`Sum·int`、`Box·string`、`Pair·int·string`。
///
/// `args` 为空时**原样**返回 `origin_name`（非泛型函数不该被 `request`，
/// 但命名函数自己不能崩）。
#[must_use]
pub fn mangled(tt: &TypeTable, origin_name: &str, args: &[TypeId]) -> String {
    let mut out = String::from(origin_name);
    for &a in args {
        out.push(ARG_SEP);
        label_into(tt, a, &mut out, 0);
    }
    out
}

/// 一个类型的**可读名字**（`Sum·int` 里 `int` 那一段）。
#[must_use]
pub fn label(tt: &TypeTable, t: TypeId) -> String {
    let mut out = String::new();
    label_into(tt, t, &mut out, 0);
    out
}

/// 只拼标识符安全的字符（`_` 在这里被消掉）。
fn push_ident(out: &mut String, s: &str) {
    for c in s.chars() {
        if c != '_' {
            out.push(c);
        }
    }
}

/// 复合类型的内部段（`ptr` / `slice` / `map` …）。
///
/// ⚠️ **只写段名，不写分隔符**：`·`（实参之间）由 [`mangled`] 发，`.`（复合
/// 内部）由**父节点**在段之间发。让子节点自己「看情况补一个」会多吐一个 ——
/// `F[struct{}]` 一度渲染成 `F·.struct`。
fn push_tag(out: &mut String, tag: &str) {
    push_ident(out, tag);
}

fn label_into(tt: &TypeTable, t: TypeId, out: &mut String, depth: usize) {
    if depth >= MAX_LABEL_DEPTH {
        // 不再结构性：按下标命名（唯一，只是同结构不同名 ⇒ 多造实例）
        push_tag(out, "nest");
        out.push_str(&t.index().to_string());
        return;
    }
    let d = depth + 1;
    match tt.type_of(t) {
        Type::Basic(b) => push_ident(out, b.as_str()),
        Type::Named(n) => {
            let def = tt.named_def(*n);
            // 实例的 `name` 与原点同名 ⇒ `Box` + `.int` = `Box.int`
            push_ident(out, &def.name);
            for &a in &def.targs {
                out.push(INNER_SEP);
                label_into(tt, a, out, d);
            }
        }
        Type::Alias(_) => {
            // 别名不是独立类型：跟着右侧走（`request` 也先 `unalias` 过）。
            label_into(tt, unalias(tt, t), out, d);
        }
        Type::Pointer(e) => {
            push_tag(out, "ptr");
            out.push(INNER_SEP);
            label_into(tt, *e, out, d);
        }
        Type::Slice(e) => {
            push_tag(out, "slice");
            out.push(INNER_SEP);
            label_into(tt, *e, out, d);
        }
        Type::Array { len, elem } => {
            push_tag(out, "arr");
            match len.map(|c| tt.consts().get(c)) {
                Some(Const::Int(v)) if !v.is_neg() => out.push_str(&v.to_dec()),
                // `[...]T` 或非整型长度：写 `X`（长度未定，与 `C7` 的 `-` 同义）
                _ => out.push('X'),
            }
            out.push(INNER_SEP);
            label_into(tt, *elem, out, d);
        }
        Type::Map { key, elem } => {
            push_tag(out, "map");
            out.push(INNER_SEP);
            label_into(tt, *key, out, d);
            out.push(INNER_SEP);
            label_into(tt, *elem, out, d);
        }
        Type::Chan { dir, elem } => {
            push_tag(
                out,
                match dir {
                    ChanDir::SendRecv => "chan",
                    ChanDir::SendOnly => "sendchan",
                    ChanDir::RecvOnly => "recvchan",
                },
            );
            out.push(INNER_SEP);
            label_into(tt, *elem, out, d);
        }
        Type::Struct(sid) => {
            // ⚠️ **结构性**：两个独立的 `struct{}` 条目必须同名（`add_struct`
            // 不去重），否则不动点每轮造一个新实例。
            push_tag(out, "struct");
            for f in &tt.struct_def(*sid).fields {
                out.push(INNER_SEP);
                match &f.name {
                    Some(n) if !f.embedded => push_ident(out, n),
                    _ => out.push('e'), // 嵌入字段：名字就是它的类型，下一段给出
                }
                if let Some(tag) = &f.tag
                    && !tag.is_empty()
                {
                    out.push(INNER_SEP);
                    push_ident(out, tag);
                }
                out.push(INNER_SEP);
                label_into(tt, f.typ, out, d);
            }
        }
        // 下面五档不作值类型的实参（`signature`/`tuple`/`union`/`typeparam`
        // 在 Go 里都不是类型字面量能写进实参位置的），匿名 `interface{}` 则是
        // 可能的 —— 都按下标命名，代价见模块文档的「多造 vs 合并」。
        Type::Interface(iid) => {
            push_tag(out, "iface");
            out.push_str(&iid.index().to_string());
        }
        Type::Signature(sid) => {
            push_tag(out, "func");
            out.push_str(&sid.index().to_string());
        }
        Type::Tuple(tid) => {
            push_tag(out, "tuple");
            out.push_str(&tid.index().to_string());
        }
        Type::TypeParam(tpid) => {
            push_tag(out, "tp");
            out.push_str(&tpid.index().to_string());
        }
        Type::Union(uid) => {
            push_tag(out, "union");
            out.push_str(&uid.index().to_string());
        }
        Type::Invalid => push_tag(out, "invalid"),
    }
}
