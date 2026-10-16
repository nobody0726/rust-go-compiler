//! `C7` 中立类型 dump 渲染器（M3/Task 16）。
//!
//! # 这个文件是**契约的可执行副本**
//!
//! 逐字节对撞的权威在 `tools/types-oracle/main.go`（标准库 `go/types` 侧）。
//! 本文件是 **rgoc 侧**：同一份 `TypeTable` 走同一套规则，输出到 `#` 注释行
//! 与空行之外必须**逐字节相同**（`C7` §2 规则 1 规定这两类两侧都忽略）。
//!
//! # 编号（`C7` §3）—— 本文件最容易写错的一处
//!
//! **不是**「表内插入序」，而是**从根出发的走访序**：
//!
//! * 根 = 顶层声明的类型，**源码序**（由调用方给出，Task 30/31 的 driver 负责）
//! * 每个条目：**后序**（先子后父）占号；同一个「键」只占一个号
//! * **定义型（`named`/`alias`）例外：先占号、再下钻本体** —— 这是递归类型
//!   （`type N struct{ p *N }`）能终止的唯一手段
//!
//! 去重的「键」（实测 oracle 的 `idx map[types.Type]int` 的对应物）：
//!
//! | 键 | 谁共用 | 实测依据 |
//! |---|---|---|
//! | `Basic(BasicId)` | 同一个 `BasicId` 的所有条目 | `go/types` 的 `*types.Basic` 是**单例**（`Typ[Int]`） |
//! | `Invalid` | 所有 `invalid` | 实测：三个未定义类型只占一个 `0 invalid` |
//! | `EmptyTuple` | **所有空元组** | 实测：`var x func(); var y func(); var z func() ()` ⇒ 只有一条 `0 tuple 0`，三个签名全引用它 |
//! | `Named`/`Alias`(定义 id) | 同一个定义的每一次引用 | 实测：`var e S` / `var f S` 只占一个号 |
//! | `Occ(TypeId)` | 其余（结构型）**一次出现一个号** | 实测：两个 `[]byte` ⇒ `1`/`2`；`struct{}` 两次 ⇒ 两个号；`interface{}` 两次 ⇒ 两个号 |
//!
//! ⚠️ 空元组单独成为一类是**实测**的推断，不是设计选择：`go/types` 里空元组是
//! 被复用的实例（`func(a int)` 的**结果**元组与 `func()` 的**参数**元组是同一个），
//! 而**非空**元组各自独立（`A() (int,string)` 与 `Z() int` 的结果元组是两个号）。
//! 若某个 fixture 上这条不成立，对撞会立刻红 —— 那时再按实测收窄。
//!
//! ⚠️ `Basic` 按 `BasicId` 去重这一条**不是**可有可无的优化：检查器会把
//! **universe 的类型**（`error` 的方法签名里的 `string` 等）与 fixture 自己的
//! 类型放进**同一张表**（实测 oracle 在 `func g() (int, error)` 上把 `error`
//! 拉了进来），若不按 `BasicId` 去重，`int` 会出现两次而 oracle 只会有一次。
//!
//! # 环守卫：本文件**不需要**它
//!
//! oracle 侧有环守卫（`interface{ comparable; M() }` 这类**非法**样本会让后序
//! 走访成环 ⇒ 曾经的 `fatal error: stack overflow`，见 `C7` §5.1）。本文件
//! **结构上不可能**成环：`Type` 里唯一能指向「尚未填好的条目」的是
//! `Named`/`Alias`（它们「先占号、后填本体」），而这两类在成环时**已经占过号**
//! ⇒ 递归在 `assigned` 那一行就返回了。结构型（`struct`/`tuple`/`interface`/…）
//! 的子节点必须先于父节点存在 ⇒ 匿名类型无法自引用。

use std::collections::HashMap;
use std::fmt::Write as _;

use rgoc_const::{BasicId, Const, ConstId};

use crate::table::TypeTable;
use crate::typ::{AliasId, NamedId, Type, TypeId};

/// 渲染 `C7` 格式的类型 dump（`roots` 是根类型，**源码序**）。
///
/// 输出以 `# source: rgoc` 开头（`C7` §2 规则 1：`#` 行是注释，两侧都忽略；
/// oracle 侧写 `# source: oracle` —— **这是刻意保留的唯一一处不同**，
/// 用来标记「这份 dump 谁产的」）。
///
/// 末尾**恰一个**换行（`C7` §2）。
#[must_use]
pub fn dump_types(tt: &TypeTable, roots: &[TypeId]) -> String {
    let mut d = Dumper {
        tt,
        order: Vec::new(),
        assigned: HashMap::new(),
    };
    for &r in roots {
        d.number(r);
    }
    d.render()
}

/// 编号的「键」：同一个键只占一个号（见模块文档的表）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Key {
    /// 同一个 `BasicId`（`go/types` 的 `*types.Basic` 是单例）。
    Basic(BasicId),
    /// 所有 `invalid`（`go/types` 的 `Typ[Invalid]` 也是那个单例）。
    Invalid,
    /// **空元组**（`tuple 0`）—— 实测是**单例**，见模块文档。
    EmptyTuple,
    /// 同一个 `named` 定义（**定义身份**，不是出现位置）。
    Named(NamedId),
    /// 同一个 `alias` 定义。
    Alias(AliasId),
    /// 其余：**出现位置**（结构型不去重）。
    Occ(TypeId),
}

/// 取一个类型条目的编号键。
fn key_of(tt: &TypeTable, id: TypeId) -> Key {
    match tt.type_of(id) {
        Type::Basic(b) => Key::Basic(*b),
        Type::Invalid => Key::Invalid,
        Type::Tuple(t) if tt.tuple_def(*t).vars.is_empty() => Key::EmptyTuple,
        Type::Named(n) => Key::Named(*n),
        Type::Alias(a) => Key::Alias(*a),
        _ => Key::Occ(id),
    }
}

struct Dumper<'a> {
    tt: &'a TypeTable,
    /// 占号顺序：第 `i` 个条目（即键第一次出现的那个 `TypeId`）拿到号 `i`。
    order: Vec<TypeId>,
    /// 键 → 号。
    assigned: HashMap<Key, usize>,
}

impl Dumper<'_> {
    /// 走访并占号（`C7` §3）。
    fn number(&mut self, id: TypeId) {
        let tt = self.tt;
        let key = key_of(tt, id);
        if self.assigned.contains_key(&key) {
            return;
        }
        // 定义型：**先占号，再下钻本体**（顺序与「后序」相反，这是刻意的）
        match tt.type_of(id) {
            Type::Named(n) => {
                let n = *n;
                self.occupy(id, key);
                let under = tt.named_underlying(n);
                self.number(under);
                return;
            }
            Type::Alias(a) => {
                let a = *a;
                self.occupy(id, key);
                let rhs = tt.alias_rhs(a);
                self.number(rhs);
                return;
            }
            _ => {}
        }
        // 其余：先子后父
        for child in children(tt, id) {
            self.number(child);
        }
        self.occupy(id, key);
    }

    fn occupy(&mut self, id: TypeId, key: Key) {
        let n = self.order.len();
        self.order.push(id);
        self.assigned.insert(key, n);
    }

    fn render(&self) -> String {
        let mut out = String::from("# source: rgoc\n");
        for (i, id) in self.order.iter().enumerate() {
            let _ = writeln!(out, "{i}\t{}", self.line(*id));
        }
        out
    }

    /// 一次类型引用（`C7` §2.2）。
    fn ref_of(&self, id: TypeId) -> String {
        let n = self
            .assigned
            .get(&key_of(self.tt, id))
            .expect("C7 §3 违反：类型未编号就被引用（本文件的下钻路径漏了一条）");
        match self.tt.type_of(id) {
            Type::Named(_) | Type::Alias(_) => format!("#{n}"),
            _ => n.to_string(),
        }
    }

    /// 一个条目的首行（嵌套项另起行、2 空格缩进）。
    fn line(&self, id: TypeId) -> String {
        match self.tt.type_of(id) {
            Type::Invalid => "invalid".to_string(),
            Type::Basic(b) => format!("basic {}", b.as_str()),
            Type::Named(n) => format!("named {}", self.ref_of(self.tt.named_underlying(*n))),
            Type::Alias(a) => format!("alias {}", self.ref_of(self.tt.alias_rhs(*a))),
            Type::Pointer(e) => format!("pointer {}", self.ref_of(*e)),
            Type::Slice(e) => format!("slice {}", self.ref_of(*e)),
            Type::Array { len, elem } => {
                format!("array {} {}", len_text(self.tt, *len), self.ref_of(*elem))
            }
            Type::Map { key, elem } => format!("map {} {}", self.ref_of(*key), self.ref_of(*elem)),
            Type::Chan { dir, elem } => {
                format!("chan {} {}", dir.as_str(), self.ref_of(*elem))
            }
            Type::Struct(sid) => {
                let def = self.tt.struct_def(*sid);
                let mut b = format!("struct {}", def.fields.len());
                for f in &def.fields {
                    // 嵌入字段的名字位恒 `-`（`C7` §2.3）；名字是源码里的字符串 ⇒ 加引号
                    let name = if f.embedded {
                        "-".to_string()
                    } else {
                        rust_debug_quote(f.name.as_deref().unwrap_or(""))
                    };
                    let tag = match &f.tag {
                        Some(t) if !t.is_empty() => rust_debug_quote(t),
                        _ => "-".to_string(),
                    };
                    let emb = if f.embedded { "true" } else { "false" };
                    let _ = write!(b, "\n  {name} {} {tag} {emb}", self.ref_of(f.typ));
                }
                b
            }
            Type::Tuple(tid) => {
                let def = self.tt.tuple_def(*tid);
                let mut b = format!("tuple {}", def.vars.len());
                for (name, ty) in &def.vars {
                    let _ = write!(
                        b,
                        "\n  {} {}",
                        name_or_dash(name.as_deref().unwrap_or("")),
                        self.ref_of(*ty)
                    );
                }
                b
            }
            Type::Signature(sid) => {
                let def = self.tt.signature_def(*sid);
                let recv = match def.recv {
                    Some(r) => self.ref_of(r),
                    None => "-".to_string(),
                };
                format!(
                    "signature {recv} {} {} {}",
                    self.ref_of(def.params),
                    self.ref_of(def.results),
                    def.variadic
                )
            }
            Type::Interface(iid) => {
                let def = self.tt.interface_def(*iid);
                // 方法**按名字升序**（`C7` §4.2）：保证两侧输出确定
                let mut ms: Vec<&(String, TypeId)> = def.methods.iter().collect();
                ms.sort_by(|a, b| a.0.cmp(&b.0));
                let mut b = format!(
                    "interface {} {}",
                    ms.len(),
                    if def.terms.is_some() { "1" } else { "0" }
                );
                for (name, sig) in ms {
                    let _ = write!(
                        b,
                        "\n  method {} {}",
                        rust_debug_quote(name),
                        self.ref_of(*sig)
                    );
                }
                if let Some(u) = def.terms {
                    let _ = write!(b, "\n  term {}", self.ref_of(u));
                }
                b
            }
            Type::TypeParam(tpid) => {
                let def = self.tt.type_param_def(*tpid);
                format!("typeparam {} {}", def.index, self.ref_of(def.constraint))
            }
            Type::Union(uid) => {
                let def = self.tt.union_def(*uid);
                let mut b = format!("union {}", def.terms.len());
                for (tilde, ty) in &def.terms {
                    let kind = if *tilde { "tilde" } else { "exact" };
                    let _ = write!(b, "\n  term {kind} {}", self.ref_of(*ty));
                }
                b
            }
        }
    }
}

/// 下钻顺序 —— **与 oracle 的 `number` 逐条对齐**（改这里必须同步 `main.go`）。
fn children(tt: &TypeTable, id: TypeId) -> Vec<TypeId> {
    let mut out = Vec::new();
    match tt.type_of(id) {
        Type::Invalid | Type::Basic(_) => {}
        // `named`/`alias` 由 `number` 自己处理（先占号、后下钻）
        Type::Named(_) | Type::Alias(_) => {}
        Type::Pointer(e) | Type::Slice(e) | Type::Chan { elem: e, .. } => out.push(*e),
        Type::Array { elem, .. } => out.push(*elem),
        Type::Map { key, elem } => {
            out.push(*key);
            out.push(*elem);
        }
        Type::Struct(sid) => out.extend(tt.struct_def(*sid).fields.iter().map(|f| f.typ)),
        Type::Tuple(tid) => out.extend(tt.tuple_def(*tid).vars.iter().map(|(_, t)| *t)),
        Type::Signature(sid) => {
            let def = tt.signature_def(*sid);
            // 顺序：recv（若有）→ params → results（与 go/types 的 Signature 一致）
            if let Some(r) = def.recv {
                out.push(r);
            }
            out.push(def.params);
            out.push(def.results);
        }
        Type::Interface(iid) => {
            let def = tt.interface_def(*iid);
            // ⚠️ 这里**也**按名字升序（不是「存进来的顺序」）：`go/types` 的
            // `Interface.NumMethods()` 对**已完成**的接口是名字序，oracle 照它下钻。
            let mut ms: Vec<&(String, TypeId)> = def.methods.iter().collect();
            ms.sort_by(|a, b| a.0.cmp(&b.0));
            out.extend(ms.iter().map(|(_, sig)| *sig));
            if let Some(u) = def.terms {
                out.push(u);
            }
        }
        Type::TypeParam(tpid) => out.push(tt.type_param_def(*tpid).constraint),
        Type::Union(uid) => out.extend(tt.union_def(*uid).terms.iter().map(|(_, t)| *t)),
    }
    out
}

/// 数组长度的文本（`C7` §4.3）：十进制、无前导 0；未定 / 非整数 / 负 ⇒ `-`。
///
/// oracle 侧的长度来自 `go/types` 的 `int64`（未定写 `-1`）；本层来自常量表，
/// 所以「不是非负整数常量」一律退化成 `-`，与 `-1` 的渲染**同形**。
fn len_text(tt: &TypeTable, len: Option<ConstId>) -> String {
    match len.map(|c| tt.consts().get(c)) {
        Some(Const::Int(v)) if !v.is_neg() => v.to_dec(),
        _ => "-".to_string(),
    }
}

/// 无名 ⇒ `-`；有名 ⇒ 加引号（`C7` §2.3 / §4.2 的 `tuple` 行）。
fn name_or_dash(s: &str) -> String {
    if s.is_empty() {
        "-".to_string()
    } else {
        rust_debug_quote(s)
    }
}

/// 按 **Rust `{:?}`（`str` 的 Debug）语义**给字符串加引号（`C7` §2.3）。
///
/// 手写而不用 `format!("{s:?}")` 的两个理由：
///
/// 1. Rust 的 `escape_debug` 对「组合字符 / 变体选择符」会额外转义成 `\u{...}`
///    （`escape_grapheme_extended`），而 `C7` §2.3 只规定「`<0x20` 用 `\u{XX}`，
///    其余原样」—— 手写才与 oracle 的 `rustDebugQuote` 逐字节同形；
/// 2. 这条规则是**契约**，写在这里的 `match` 就是契约本身，可被测试钉住。
fn rust_debug_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_quote_与_c7_2_3_逐条对齐() {
        assert_eq!(rust_debug_quote(""), r#""""#);
        assert_eq!(rust_debug_quote("x"), r#""x""#);
        assert_eq!(rust_debug_quote(r#"a"b"#), r#""a\"b""#);
        assert_eq!(rust_debug_quote("a\\b"), r#""a\\b""#);
        assert_eq!(rust_debug_quote("a\nb\tc\rd"), r#""a\nb\tc\rd""#);
        assert_eq!(rust_debug_quote("\0"), r#""\0""#);
        // `<0x20` 与 `0x7f` 走 `\u{..}`（**小写十六进制**）
        assert_eq!(rust_debug_quote("\u{1}"), r#""\u{1}""#);
        assert_eq!(rust_debug_quote("\u{7f}"), r#""\u{7f}""#);
        assert_eq!(rust_debug_quote("\u{1f}"), r#""\u{1f}""#);
        // 单引号**不**转义（Rust 的 `str` Debug 语义）
        assert_eq!(rust_debug_quote("it's"), r#""it's""#);
        // 非 ASCII 原样（含 CJK 与组合字符 —— 不学 Rust 的 grapheme 额外转义）
        assert_eq!(rust_debug_quote("结构体"), r#""结构体""#);
        assert_eq!(rust_debug_quote("e\u{301}"), "\"e\u{301}\"");
        // 边界：`0x20`（空格）**不**转义，`0x7f` 转义
        assert_eq!(rust_debug_quote(" "), r#"" ""#);
        assert_eq!(rust_debug_quote("~"), r#""~""#);
    }

    #[test]
    fn 无名写短横() {
        assert_eq!(name_or_dash(""), "-");
        assert_eq!(name_or_dash("a"), r#""a""#);
    }

    #[test]
    fn 数组长度文本() {
        let mut tt = TypeTable::new();
        let e = tt.add_basic(BasicId::Int);
        // 未定（`[...]T`）⇒ `-`
        assert_eq!(len_text(&tt, None), "-");
        // 0 与正整数走十进制
        let zero = tt.consts_mut().add(Const::Int(rgoc_const::BigInt::zero()));
        assert_eq!(len_text(&tt, Some(zero)), "0");
        let n = tt
            .consts_mut()
            .add(Const::Int(rgoc_const::BigInt::from_i64(1).shl(40)));
        assert_eq!(len_text(&tt, Some(n)), "1099511627776");
        // 负数 / 非整数 / 未知 ⇒ `-`（与 oracle 的 `-1` 同形）
        let neg = tt
            .consts_mut()
            .add(Const::Int(rgoc_const::BigInt::from_i64(-1)));
        assert_eq!(len_text(&tt, Some(neg)), "-");
        let b = tt.consts_mut().add(Const::Bool(true));
        assert_eq!(len_text(&tt, Some(b)), "-");
        let u = tt.consts_mut().add(Const::Unknown);
        assert_eq!(len_text(&tt, Some(u)), "-");
        let _ = e;
    }
}
