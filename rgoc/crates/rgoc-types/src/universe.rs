//! universe 作用域：Go 的**预声明对象**（M3/Task 15，`T-TY-51`，`F8`）。
//!
//! 清单来源：Go 规范 §Declarations and scope 的 **Universe block** 一节
//! （权威、可核对），三种「特殊身份」按 `go/types/universe.go` 并**经 oracle 实测**：
//!
//! | 名字 | 官方身份 | 实测证据（`tools/types-oracle`） |
//! |---|---|---|
//! | `byte` | `uint8` 的**别名**，但 dump 出 `basic uint8` | `var a byte` ⇒ `basic uint8` |
//! | `rune` | `int32` 的**别名**，dump 出 `basic int32` | `var b rune` ⇒ `basic int32` |
//! | `error` | **`named`**（本体是 `interface{ Error() string }`） | `var e error` ⇒ `named` + `interface 1 0` |
//! | `any` | **`alias`**（右侧是 `interface{}`） | `var y any` ⇒ `alias` + `interface 0 0` |
//! | `comparable` | **`named`**（本体是带 `comparable` 标记的接口） | 只能作约束；`named` + `interface 0 0` 与 `interface{}` 的**本体**同形 |
//!
//! ⚠️ `byte`/`rune` 折到 `Uint8`/`Int32` 是**与 `BasicId` 的 25 项口径一致**的
//! （`C7` §4.1 的实测订正：`go/types` 的 `BasicKind` 有 26 个，但 `byte`/`rune`
//! dump 出的是本体名）—— 见 `rgoc_const::BasicId`。
//!
//! # 内建函数**没有签名**
//!
//! 18 个内建（`len`/`cap`/`make`/…）在这里只登记**名字与种类**（`typ: None`）：
//! M3 不做调用检查，编造 18 个签名只会造出「看起来能查、其实没查」的假象
//! （官方对这些内建的签名也是 `Typ[Invalid]` 的样子货）。
//! 调用检查与内建签名归后续里程碑 ⇒ `open_gaps`。
//!
//! # 为什么 `declare_universe` 与 `universe()` 是两个函数
//!
//! 检查器要把 universe **声明进它自己那张表**（fixture 引用 `error` 时，
//! dump 必须把 `error` 拉进来编号 —— 实测 oracle 正是如此），所以主入口收
//! `&mut TypeTable`（Task 23 起还收 `&mut ScopeTable`）。[`universe()`] 只是
//! 「自带两张表」的便利包装（测试与独立使用）。

use rgoc_const::{BasicId, Const};

use crate::object::{ObjKind, Object};
use crate::scope::{ScopeId, ScopeKind, ScopeTable};
use crate::table::TypeTable;
use crate::typeset::IfaceTSet;

/// 预声明**类型名**（22 个：18 个基本名 + `byte`/`rune` 两个别名拼写
/// + `error`/`any`/`comparable`）。
///
/// 顺序照 Go 规范的口径（基本类型按名字，再三个特殊身份），与
/// `BasicId::ALL` 的 25 项**不是**同一份清单：25 是 `BasicId` 的变体数
/// （含 6 个无类型），22 是**源码里能写的名字**数。
pub const TYPE_NAMES: [&str; 22] = [
    "bool",
    "byte",
    "comparable",
    "complex64",
    "complex128",
    "error",
    "float32",
    "float64",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "rune",
    "string",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
    "any",
];

/// 预声明**常量**（3 个）。
pub const CONST_NAMES: [&str; 3] = ["true", "false", "iota"];

/// 预声明**内建函数**（18 个，照规范 Universe block 一节）。
pub const BUILTIN_NAMES: [&str; 18] = [
    "append", "cap", "clear", "close", "complex", "copy", "delete", "imag", "len", "make", "max",
    "min", "new", "panic", "print", "println", "real", "recover",
];

/// universe 里的对象总数：22 类型名 + 3 常量 + 1 个 `nil` + 18 内建。
pub const UNIVERSE_SIZE: usize = 22 + 3 + 1 + 18;

/// universe 的全部名字，**按字典序**（`UNIVERSE_SIZE` 个）。
///
/// 它是 [`declare_universe`] 会登记的名字集，**与登记的先后无关** ——
/// 供测试与报告做「清单对账」（[`crate::scope::Scope::names`] 的序来自表实现，
/// 这里是清单自身的规范序）。
#[must_use]
pub fn declared_names() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = TYPE_NAMES
        .iter()
        .chain(CONST_NAMES.iter())
        .chain(BUILTIN_NAMES.iter())
        .copied()
        .collect();
    v.push("nil");
    v.sort_unstable();
    v
}

/// 把预声明对象声明进 `tt` 与作用域表 `st`，返回 **universe 作用域**（它拥有那些对象）。
///
/// 返回的层没有父 —— 它是作用域链的底（[`ScopeKind::Universe`]）。
///
/// # 为什么要收 `&mut ScopeTable`（`Task 23` Ruling 23-1）
///
/// 作用域自 Task 23 起是**竞技场**（[`crate::scope`] 的模块文档有完整理由）：
/// 层只能由 [`ScopeTable`] 产出。universe 是唯一合法的**根**，所以它必须从表里建。
pub fn declare_universe(tt: &mut TypeTable, st: &mut ScopeTable) -> ScopeId {
    let u = st.root(ScopeKind::Universe);

    // ① 基本类型名（含 byte/rune 的别名拼写）
    for name in TYPE_NAMES {
        if let Some(b) = basic_name_to_id(name) {
            let ty = tt.add_basic(b);
            st.insert(u, Object::with_type(ObjKind::TypeName, name, ty));
        }
    }

    // ② `error`：**named**，本体 `interface{ Error() string }`
    //
    // ⚠️ 方法签名的 `recv` 是 **`None`**：接口方法**没有接收者**（C7 §4.2：
    // 「`recv` 为 `-` 表示非方法（普通函数 / **接口方法**）」）。go/types 在
    // 内部给接口方法的签名也设了接收者（实测 `Signature.Recv()` = 接口本身），
    // 但那是它的实现细节 —— C7 与 oracle 的**渲染口径**都是 `-`
    // （M3/Task 33 对撞实测：写成 `Some(error_self)` 会让两侧 dump 不一致）。
    let error_ty = tt.add_named("error");
    let error_self = tt.named_type(error_ty);
    let no_params = tt.add_tuple(Vec::new());
    let str_ty = tt.add_basic(BasicId::String);
    let results = tt.add_tuple(vec![(None, str_ty)]);
    let sig = tt.add_signature(None, no_params, results, false);
    let sig_ty = tt.signature_type(sig);
    let err_iface = tt.add_interface(vec![("Error".to_string(), sig_ty)], None, IfaceTSet::All);
    let err_iface_ty = tt.interface_type(err_iface);
    tt.set_named_underlying(error_ty, err_iface_ty);
    st.insert(u, Object::with_type(ObjKind::TypeName, "error", error_self));

    // ③ `any`：**alias**，右侧 `interface{}`
    let any_ty = tt.add_alias("any");
    let any_self = tt.alias_type(any_ty);
    let empty_iface = tt.add_interface(Vec::new(), None, IfaceTSet::All);
    let empty_iface_ty = tt.interface_type(empty_iface);
    tt.set_alias_rhs(any_ty, empty_iface_ty);
    st.insert(u, Object::with_type(ObjKind::TypeName, "any", any_self));

    // ④ `comparable`：预声明**特殊约束接口**（`F8`：不是普通类型名），
    //    但它在官方表示里是 **`named`**（go1.27.1 实测：
    //    `types.Universe.Lookup("comparable").Type()` 是 `*types.Named`，
    //    本体是那个带 `comparable` 标记的接口）⇒ 这里也必须有这层 Named 壳，
    //    否则 dump 少一层（对撞实测：`[T comparable]` 的约束少 `named` 行）。
    //    ⚠️ 「有 `comparable` 标记」这件事**不进 dump**（C7 §4.2）：`interface 0 0`。
    let cmp_ty = tt.add_named("comparable");
    let cmp_self = tt.named_type(cmp_ty);
    let cmp_iface = tt.add_interface(Vec::new(), None, IfaceTSet::Comparable);
    let cmp_iface_ty = tt.interface_type(cmp_iface);
    tt.set_named_underlying(cmp_ty, cmp_iface_ty);
    st.insert(
        u,
        Object::with_type(ObjKind::TypeName, "comparable", cmp_self),
    );

    // ⑤ 常量：`true`/`false` 有值，`iota` **没有**（值随组内位置变）
    let untyped_bool = tt.add_basic(BasicId::UntypedBool);
    st.insert(
        u,
        Object::const_with_val("true", untyped_bool, Const::Bool(true)),
    );
    st.insert(
        u,
        Object::const_with_val("false", untyped_bool, Const::Bool(false)),
    );
    let untyped_int = tt.add_basic(BasicId::UntypedInt);
    st.insert(u, Object::with_type(ObjKind::Const, "iota", untyped_int));

    // ⑥ `nil`
    let untyped_nil = tt.add_basic(BasicId::UntypedNil);
    st.insert(u, Object::with_type(ObjKind::Nil, "nil", untyped_nil));

    // ⑦ 内建函数：只有名字与种类（签名不在 M3 范围 ⇒ `open_gaps`）
    for name in BUILTIN_NAMES {
        st.insert(u, Object::new(ObjKind::Builtin, name));
    }

    u
}

/// 自带一张表的便利包装（`declare_universe` 的主用法是往检查器的表里声明）。
///
/// 返回 `(作用域表, universe 层, 类型表)`：作用域表**拥有**那些层，
/// 类型表**拥有**那些类型条目（universe 的对象引用它们的 [`crate::TypeId`]）。
#[must_use]
pub fn universe() -> (ScopeTable, ScopeId, TypeTable) {
    let mut tt = TypeTable::new();
    let mut st = ScopeTable::new();
    let u = declare_universe(&mut tt, &mut st);
    (st, u, tt)
}

/// 预声明类型名 → `BasicId`（**只有基本类型名**；`byte`/`rune` 折到本体）。
///
/// `error`/`any`/`comparable` 走 `None` —— 它们**不是**基本类型，各自有单独的
/// 构造路径（见 [`declare_universe`]）。这个分区是刻意的：把「能当转换目标的名字」
/// 与「具名/别名/约束」分开，[`rgoc_const::eval`] 的转换表也只看基本名。
#[must_use]
pub fn basic_name_to_id(name: &str) -> Option<BasicId> {
    Some(match name {
        "bool" => BasicId::Bool,
        "int" => BasicId::Int,
        "int8" => BasicId::Int8,
        "int16" => BasicId::Int16,
        "int32" => BasicId::Int32,
        "int64" => BasicId::Int64,
        "uint" => BasicId::Uint,
        "uint8" => BasicId::Uint8,
        "uint16" => BasicId::Uint16,
        "uint32" => BasicId::Uint32,
        "uint64" => BasicId::Uint64,
        "uintptr" => BasicId::Uintptr,
        "float32" => BasicId::Float32,
        "float64" => BasicId::Float64,
        "complex64" => BasicId::Complex64,
        "complex128" => BasicId::Complex128,
        "string" => BasicId::String,
        // 别名 ⇒ 折到本体（实测 dump 出的是本体名）
        "byte" => BasicId::Uint8,
        "rune" => BasicId::Int32,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 清单长度与总数自洽() {
        assert_eq!(TYPE_NAMES.len(), 22);
        assert_eq!(CONST_NAMES.len(), 3);
        assert_eq!(BUILTIN_NAMES.len(), 18);
        assert_eq!(UNIVERSE_SIZE, 44);
        assert_eq!(
            TYPE_NAMES.len() + CONST_NAMES.len() + 1 + BUILTIN_NAMES.len(),
            44
        );
        // 无重复（重复会让 `insert` 静默顶替，总数对不上而看不出来）
        let mut all: Vec<&str> = TYPE_NAMES
            .iter()
            .chain(CONST_NAMES.iter())
            .chain(BUILTIN_NAMES.iter())
            .copied()
            .collect();
        all.push("nil");
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), UNIVERSE_SIZE);
    }

    #[test]
    fn 基本名表不认特殊身份() {
        assert_eq!(basic_name_to_id("byte"), Some(BasicId::Uint8));
        assert_eq!(basic_name_to_id("rune"), Some(BasicId::Int32));
        assert_eq!(basic_name_to_id("int"), Some(BasicId::Int));
        assert_eq!(basic_name_to_id("error"), None);
        assert_eq!(basic_name_to_id("any"), None);
        assert_eq!(basic_name_to_id("comparable"), None);
        assert_eq!(basic_name_to_id("Byte"), None, "大小写敏感");
        // 无类型名**不是**源码里的名字
        assert_eq!(basic_name_to_id("untyped int"), None);
    }

    #[test]
    fn 声明的对象数等于_universe_size() {
        let (st, u, _tt) = universe();
        let s = st.scope(u);
        assert_eq!(s.len(), UNIVERSE_SIZE);
        assert_eq!(s.depth(), 1, "universe 是最外层，没有父");
        assert_eq!(s.kind(), ScopeKind::Universe);
        assert_eq!(st.len(), 1, "只有 universe 一层");
    }
}
