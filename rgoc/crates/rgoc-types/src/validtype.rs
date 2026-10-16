//! 类型图合法性：**非法递归**与**嵌套深度上限**（M3/Task 22）。
//!
//! # 判据来源
//!
//! `go/types/validtype.go`（`F9`）。核心一句话：
//!
//! > 一个类型是合法的，当且仅当它不会「无限展开」—— 即在类型图里，
//! > **按值包含**关系不构成环。
//!
//! 「按值包含」的边只有四条（官方的 `switch` 就这四个 case + `Named`）：
//!
//! | 构造 | 为什么它「包含」子类型 |
//! |---|---|
//! | `array` | 元素在内存里**紧接着**排（数组是值语义） |
//! | `struct` | 字段在内存里排开 |
//! | `union` | 项集的每个项都是这个约束的一部分 |
//! | `interface` | 嵌入者/项集在类型集里「包含」 |
//!
//! 于是 `type A [1]A`（环经 array）非法，而 `type A *A`（环经 pointer）**合法**
//! —— 指针不包含被指对象，它只是**指向**。切片/映射/通道/函数同理。
//!
//! 具名类型（`*Named`）是唯一会「下钻」的节点：走到它就把**它自己**压进
//! `nest`（内存嵌套栈）与 `path`（诊断路径），再检查它的**本体**。
//! 再遇见 `nest` 里已有的具名类型 ⇒ 内存里装下了自己 ⇒ 报环。
//!
//! # `nest` 与 `path` 为什么要两个
//!
//! * `nest` = 「当前**正在**展开的具名类型」—— 只在这里查环（所以同一个类型
//!   的两个**兄弟**字段 `struct{ A; A }` 不算环：展开完第一个出栈了）；
//! * `path` = 「一路走来**经过过**的具名类型」—— 报环时取它的**后缀**
//!   （官方 `cycleError(path[start:])`），好让消息是 `A refers to B refers to A`
//!   而不是从根开始的一大串。
//!
//! 官方注释里还有一条更细的理由：`path` 会让「起点」比 `nest` 里更靠前。
//!
//! ⚠️ **M3 实测：这两个栈恒等**。官方让它们分家的**唯一**原因是 `*TypeParam`
//! 那支的剪枝（`nest[:d]`），而 M3 没有实例化 ⇒ 没有剪枝 ⇒ 两者元素始终相同。
//! 实测证据：把查环从 `nest` 换成 `path`，`tests/type_cycle.rs` 三条**全绿**
//! （记在 `.superpowers/sdd/M3-plan/progress.md` 的 Task 22 条目）。
//! 保留两个栈是为了 M4 加实例化时**不必重写这一段**，且「查环用哪一个是**语义**」
//! 这件事本身要被写下来（用 `path` 查环在 M4 会**多报**合法的实例化环）。
//!
//! # 官方有、我们**没有**的两块
//!
//! 1. **实例化**（`*TypeParam` 那一整段）：M3 没有实例化（`D-M3-2`）⇒
//!    类型参数一律**忽略**。官方文档也说了：不属于任何实例化的类型参数
//!    「must be from an enclosing function and can be ignored」。
//! 2. **别名环**：官方注释明说 `type A = [10]A` 由**声明级**的循环检测提前抓走
//!    （`objDecl` 的机制），不走 `validType`。我们照抄这个分工：别名在
//!    [`check`](State::check) 里直接返回 `true`（诊断归声明收集，Task 24）。
//!
//! # 嵌套深度上限：**`H` 判据**（官方没有）
//!
//! 官方靠 Go 运行时的**可增长栈**，多深都不怕。我们落在固定的宿主栈上，
//! 所以自设上限 [`MAX_TYPE_NEST_DEPTH`]（1000，与 parser 的 `MAX_NEST_DEPTH` 同值）。
//!
//! ⚠️ **它和「非法递归」必须分开报**：1001 层的 `[1][1]…[1]int` 在 Go 里
//! **是合法类型**。把深度超限报成 `TypInvalidRecursiveType` 等于告诉用户
//! 「你写错了」——那是**假诊断**。所以深度走 [`DiagCode::TypNestingTooDeep`]，
//! 与 `SynNestingTooDeep` 同一族（都带 `H` 判据的烙印）。

use rgoc_lex::diag::{Diag, DiagBag, DiagCode};
use rgoc_lex::{FileId, Pos};

use crate::table::TypeTable;
use crate::typ::{NamedId, Type, TypeId};

/// 类型**嵌套**深度上限（**`H` 判据**：官方没有这个限制）。
///
/// 与 `rgoc-parse` 的 `MAX_NEST_DEPTH` **同值**（1000）—— 两条链路面对的是
/// 同一个宿主栈，用两个不同的数字会让「多大算深」变成两个答案。
///
/// 约定与 parser 一致：**`> 上限` 才算超限**（恰 1000 层是合法的，
/// 1001 层才报）—— 有测试从**两侧**钉住这条边界。
pub const MAX_TYPE_NEST_DEPTH: usize = 1000;

/// 判定一个类型是否合法（不会无限展开）。
///
/// 诊断**追加**到 `bag`（不消费它，便于调用方对多个声明累计）：
///
/// * [`DiagCode::TypInvalidRecursiveType`] —— 非法的按值自包含（官方同名代号）；
/// * [`DiagCode::TypNestingTooDeep`] —— 嵌套超 [`MAX_TYPE_NEST_DEPTH`]
///   （**`H` 判据**，我方能力边界）。
///
/// 返回 `false` = 至少报了一条。两条诊断各自**最多报一次**（一次调用
/// = 一棵类型图），与 parser 的「每文件只报一次」是同一种克制。
///
/// # 位置从哪来
///
/// 官方报在**环路径起点的类型名对象**上。本层的对应物是
/// [`crate::table::NamedDef::pos`]（由检查器在建表时填）。
/// 纯匿名类型嵌套超限时没有具名类型可指 ⇒ 退回「位置未知」
/// （`Pos::is_known() == false`）；这条限制记在 `open_gaps`。
pub fn valid_type(tt: &TypeTable, t: TypeId, bag: &mut DiagBag) -> bool {
    let mut st = State {
        bag,
        depth_reported: false,
    };
    let mut nest: Vec<NamedId> = Vec::new();
    let mut path: Vec<NamedId> = Vec::new();
    st.check(tt, t, 0, &mut nest, &mut path)
}

/// 判定过程中的可变状态（诊断袋 + 「深度只报一次」的闸）。
struct State<'a> {
    bag: &'a mut DiagBag,
    depth_reported: bool,
}

impl State<'_> {
    /// 走一步。`depth` = 当前**递归层数**（入口为 0）。
    fn check(
        &mut self,
        tt: &TypeTable,
        t: TypeId,
        depth: usize,
        nest: &mut Vec<NamedId>,
        path: &mut Vec<NamedId>,
    ) -> bool {
        // 深度上限先于一切：再往下走就要吃宿主栈了。
        if depth > MAX_TYPE_NEST_DEPTH {
            if !self.depth_reported {
                self.depth_reported = true;
                let pos = path.first().map_or_else(unknown_pos, |n| named_pos(tt, *n));
                self.bag.push(Diag {
                    pos,
                    code: DiagCode::TypNestingTooDeep,
                    message: format!(
                        "type nesting depth exceeds {MAX_TYPE_NEST_DEPTH} (implementation limit)"
                    ),
                });
            }
            return false;
        }

        match tt.type_of(t) {
            // ── 「按值包含」的四条边：要下钻 ──────────────────────────
            Type::Array { elem, .. } => self.check(tt, *elem, depth + 1, nest, path),
            Type::Struct(sid) => {
                for f in &tt.struct_def(*sid).fields {
                    if !self.check(tt, f.typ, depth + 1, nest, path) {
                        return false;
                    }
                }
                true
            }
            Type::Union(uid) => {
                for (_, term) in &tt.union_def(*uid).terms {
                    if !self.check(tt, *term, depth + 1, nest, path) {
                        return false;
                    }
                }
                true
            }
            // ⚠️ 我们的 `interface` 把**嵌入者折叠成了 methods + terms**
            // （官方保留 `embeddeds`）⇒ 这里只能走项集那条边。
            // 接口**自引用**的写法（`type A interface{ A }`）在 Go 里本来就非法，
            // 且折叠期的构造方就会先卡住/报错，落不到这里。
            Type::Interface(iid) => match tt.interface_def(*iid).terms {
                Some(u) => self.check(tt, u, depth + 1, nest, path),
                None => true,
            },
            // ── 唯一的「下钻本体」节点 ───────────────────────────────
            Type::Named(nid) => {
                // 官方这里是 `Identical(e, t)`。对两个具名类型来说
                // 「同一」= **同一个定义**（Task 17 的 `identical` 就是这么判的）
                // ⇒ 比 `NamedId` 等价且不必处理「判不了」的 `Err`。
                if nest.contains(nid) {
                    self.report_cycle(tt, *nid, path);
                    return false;
                }
                // 本体。⚠️ 这里**假设表已建完**（`named_underlying` 在本体未填时
                // panic，那是表自己的构造协议：`add_named` 之后立即 set）。
                // 对**环**不需要这个假设（`nest` 命中就返回了），但两个互相引用
                // 的具名类型要求**两边**都填好 —— 检查器（Task 24）负责：
                // 构建失败（如引用了不存在的类型）要降级成 `invalid` 条目，
                // **不能**留空本体。
                let rhs = tt.named_underlying(*nid);
                nest.push(*nid);
                path.push(*nid);
                let ok = self.check(tt, rhs, depth + 1, nest, path);
                nest.pop();
                path.pop();
                ok
            }
            // ── 不下钻：这些「边」不构成按值包含 ──────────────────────
            //
            // 指针/切片/映射/通道/签名/元组都只是**引用**或**可增长/间接**的，
            // `type A *A` 这类合法的自引用正好从这里走出去（官方亦然）。
            // `basic` / `invalid` 是叶子。别名环归声明收集（见模块文档）。
            Type::TypeParam(_)
            | Type::Alias(_)
            | Type::Pointer(_)
            | Type::Slice(_)
            | Type::Map { .. }
            | Type::Chan { .. }
            | Type::Signature(_)
            | Type::Tuple(_)
            | Type::Basic(_)
            | Type::Invalid => true,
        }
    }

    /// 报非法递归：取 `path` 里**从环起点开始的后缀**（官方 `path[start:]`）。
    fn report_cycle(&mut self, tt: &TypeTable, nid: NamedId, path: &[NamedId]) {
        let start = path.iter().position(|p| *p == nid).unwrap_or(0);
        let names: Vec<&str> = path[start..]
            .iter()
            .map(|p| tt.named_def(*p).name.as_str())
            .collect();
        // 官方文案：环长 1 ⇒ `invalid recursive type A`；否则
        // `invalid recursive type: A refers to B refers to A`
        // （消息文本**不承诺逐字一致**，`D-M1-9`；这里只保证形状可读）。
        let message = match names.as_slice() {
            [] => "invalid recursive type".to_string(),
            [one] => format!("invalid recursive type {one}"),
            rest => format!(
                "invalid recursive type: {} refers to {}",
                rest.join(" refers to "),
                rest[0]
            ),
        };
        self.bag.push(Diag {
            pos: named_pos(tt, nid),
            code: DiagCode::TypInvalidRecursiveType,
            message,
        });
    }
}

/// 具名类型的声明位置（没填 ⇒ 位置未知）。
fn named_pos(tt: &TypeTable, nid: NamedId) -> Pos {
    tt.named_def(nid).pos.unwrap_or_else(unknown_pos)
}

/// 「位置未知」（`Pos::is_known()` 为假：行号为 0）。
///
/// 只在两种情形出现：① 手工建的表（测试）没给位置；② 超限的是**匿名**类型的
/// 嵌套（没有具名类型可指）。两者都不该在正常编译路径上出现 ——
/// 检查器（Task 24）建表时必须 `set_named_pos`。
fn unknown_pos() -> Pos {
    Pos {
        base: None,
        file: FileId::from_index(0),
        line: 0,
        col: 0,
    }
}
