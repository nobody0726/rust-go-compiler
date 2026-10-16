//! 对象（`Object`）与种类（M3/Task 15）。
//!
//! 「对象」是**有名字的实体**：变量、常量、类型名、函数、内建、包名、标签、`nil`。
//! 字段对齐 `go/types` 的 `Object` 家族（`Var` / `Const` / `TypeName` / `Func` /
//! `Builtin` / `PkgName` / `Label` / `Nil`）—— 官方的实现是**一个接口 + 八种实现**，
//! 这里合成一个 struct + 一个 `ObjKind` 判别式。
//!
//! # 为什么合成一个 struct
//!
//! ① 八种对象共享的字段超过它们的差异（名字、类型、位置）；
//! ② 官方的**变体特定**字段只有两个真正被 M3 用到 —— 常量的**值**（`val`）
//!    与内建函数的**编号**（不需要：M3 不做调用检查）。所以两个额外字段就够，
//!    而不是八个子类型。
//! ③ 「八种」本身是要对账的东西（`T-TY-51` 的「预声明对象」按种类数），
//!    判别式让这个对账是**穷尽 match**，编译器帮我们守着。

use rgoc_const::Const;
use rgoc_lex::Pos;

use crate::typ::TypeId;

/// 对象的种类（八种，与 `go/types` 的 `Object` 家族一一对应）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ObjKind {
    /// 变量（`var`、参数、结果、`for` 的变量…）。
    Var,
    /// 常量（`const`、`true`、`false`、`iota`）。
    Const,
    /// 类型名（`type T ...`、预声明类型名）。
    TypeName,
    /// 函数 / 方法（有体或只是声明）。
    Func,
    /// 内建函数（`len`、`make`…）—— 与 `Func` **分开**：它们不能被当作值传递。
    Builtin,
    /// 包名（`import p "path"` 里的 `p`）。
    PkgName,
    /// 标签（`goto` 的目标）。
    Label,
    /// 预声明的 `nil`。
    Nil,
}

impl ObjKind {
    /// 诊断文本里用的拼写（照官方 `Object.String()` 的措辞：
    /// `x (variable of type int)`）。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ObjKind::Var => "variable",
            ObjKind::Const => "constant",
            ObjKind::TypeName => "type",
            ObjKind::Func => "func",
            ObjKind::Builtin => "builtin",
            ObjKind::PkgName => "package",
            ObjKind::Label => "label",
            ObjKind::Nil => "nil",
        }
    }
}

/// 一个对象。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Object {
    /// 种类。
    pub kind: ObjKind,
    /// 名字（**唯一标识符**：`_` 不进作用域，见 `T-TY-58`）。
    pub name: String,
    /// 类型；`None` ⇒ 「不知道 / 不适用」。
    ///
    /// * 内建函数：`None`（M3 不做调用检查，内建的签名不在本阶段范围 ⇒ `open_gaps`）
    /// * `iota`：`Some(untyped int)` 但**没有值**（它的值随组内位置变）
    /// * 类型名：`Some(该类型的 TypeId)`
    pub typ: Option<TypeId>,
    /// 常量的值；非 `Const` / 值待定（`iota`）⇒ `None`。
    pub val: Option<Const>,
    /// 位置；universe 里的对象**没有位置**（官方 `nopos`）。
    pub pos: Option<Pos>,
}

impl Object {
    /// 造一个「只有名字与种类」的对象（内建、`nil`、标签等）。
    #[must_use]
    pub fn new(kind: ObjKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            typ: None,
            val: None,
            pos: None,
        }
    }

    /// 同 `new`，再挂上类型。
    #[must_use]
    pub fn with_type(kind: ObjKind, name: impl Into<String>, typ: TypeId) -> Self {
        Self {
            typ: Some(typ),
            ..Self::new(kind, name)
        }
    }

    /// 常量对象（带值）。
    #[must_use]
    pub fn const_with_val(name: impl Into<String>, typ: TypeId, val: Const) -> Self {
        Self {
            val: Some(val),
            ..Self::with_type(ObjKind::Const, name, typ)
        }
    }

    /// 有没有确定的值（`Const` 且 `val.is_some()`）。
    #[must_use]
    pub fn has_value(&self) -> bool {
        self.kind == ObjKind::Const && self.val.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TypeTable;
    use rgoc_const::BasicId;
    use rgoc_const::BigInt;

    #[test]
    fn 八种种类的拼写() {
        let all = [
            (ObjKind::Var, "variable"),
            (ObjKind::Const, "constant"),
            (ObjKind::TypeName, "type"),
            (ObjKind::Func, "func"),
            (ObjKind::Builtin, "builtin"),
            (ObjKind::PkgName, "package"),
            (ObjKind::Label, "label"),
            (ObjKind::Nil, "nil"),
        ];
        for (k, s) in all {
            assert_eq!(k.as_str(), s);
        }
        // 八种，无重名拼写
        let mut names: Vec<&str> = all.iter().map(|(_, s)| *s).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn 只有常量对象有值() {
        let mut tt = TypeTable::new();
        let int = tt.add_basic(BasicId::Int);
        let c = Object::const_with_val("C", int, Const::Int(BigInt::from_i64(3)));
        assert!(c.has_value());
        assert_eq!(c.val, Some(Const::Int(BigInt::from_i64(3))));
        // 不是常量 ⇒ 即使挂了个值也不算
        let v = Object {
            val: Some(Const::Int(BigInt::from_i64(3))),
            ..Object::with_type(ObjKind::Var, "x", int)
        };
        assert!(!v.has_value());
        // 是常量但值待定（`iota`）⇒ 也没有值
        let i = Object::with_type(ObjKind::Const, "iota", int);
        assert!(!i.has_value());
    }

    #[test]
    fn 无类型的对象() {
        let b = Object::new(ObjKind::Builtin, "len");
        assert_eq!(b.typ, None);
        assert_eq!(b.val, None);
        assert_eq!(b.pos, None, "universe 对象没有位置");
    }
}
