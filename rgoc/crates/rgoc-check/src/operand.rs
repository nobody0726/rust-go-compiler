//! 操作数：检查器内部的**带类型值**（M4/Task 20）。
//!
//! 模式枚举照 `go/types/operand.go:24-37`（11 项；官方第 12 项 `cgofunc`
//! **出** —— cgo 是 `-fakeImportC` 面，装置响亮失败，`U-M4-05`）。模式文本
//! 逐字照 `operandModeString`（`operand.go:38-52`）—— 它进入官方诊断措辞
//! （`(value of type %s)` 一族），措辞即语义。
//!
//! # 为什么这层存在（值层接线）
//!
//! `M3-manifest.json` `open_gaps`：「**值层没有类型接线**」—— M3 的常量求值
//! 只留了值（`Object.val`），没有一处「名字 → 带类型的操作数」的映射，于是
//! `-uint32(5)` / `uint32(0)-1` 一族的溢出判定没有输入。本模块把这条线接上：
//! [`Operand::from_object`] 把装载出的对象翻成操作数时**类型随值一起走**
//! （`typ` + `val`），Task 21 起的表达式检查从操作数读「这个名字是什么
//! 类型」，同一性 / 可赋值性 / 溢出判定才有落点。
//!
//! # 对象 → 操作数的映射（官方 `check.ident` 的落点）
//!
//! | `ObjKind` | 模式 | 官方依据 |
//! |---|---|---|
//! | `Const` | `Constant` | `*Const ⇒ constant`（带 `typ` + `val`） |
//! | `Var` | `Variable` | `*Var ⇒ variable` |
//! | `TypeName` | `TypeExpr` | `*TypeName ⇒ typexpr` |
//! | `Builtin` | `Builtin` | `*Builtin ⇒ builtin` |
//! | `Nil` | `NilValue` | `*Nil ⇒ nilvalue`（仅 types2；GNU 侧同义） |
//! | `Func` | `Value` | `*Func ⇒ value` |
//! | `PkgName` / `Label` | —（`None`） | 语境消费（选择器底座 / `goto` 目标），**不产生**操作数 |
//!
//! 内建的**签名**（`len` 的调用面）等 M3 没有建的对象数据不在这里补 ——
//! 类型为 `None` 就如实为 `None`（`M3-manifest.json` `open_gaps` 已登记；
//! Task 29 的内建面再接线）。

use rgoc_const::Const;
use rgoc_lex::Pos;
use rgoc_types::{ObjKind, Object, TypeId};

/// 操作数模式（官方 `operandMode` 的 11 项子集）。
///
/// 声明序 = 官方声明序（`operand.go:24-37`）；[`Mode::ALL`] 供遍历断言。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Mode {
    /// 操作数无效（表达式检查失败后的传播态）。
    Invalid,
    /// 无值（无返回值函数调用的结果；`novalue`）。
    NoValue,
    /// 内建函数（`len` / `make` …；**不能**当值传递）。
    Builtin,
    /// 类型表达式（`typexpr`）。
    TypeExpr,
    /// 常量（`typ` 是它的类型 —— 无类型常量 ⇒ 无类型基本类型）。
    Constant,
    /// 可寻址变量。
    Variable,
    /// map 索引表达式（LHS 当变量、RHS 逗号-ok 双值）。
    MapIndex,
    /// 计算出的值。
    Value,
    /// 预声明的 `nil`。
    NilValue,
    /// 逗号-ok：可用于 `x, ok = m[k]` / `v, ok := i.(T)`。
    CommaOk,
    /// 逗号-err：第二个值是 `error`（channel 接收等）。
    CommaErr,
}

impl Mode {
    /// 官方 `operandModeString` 文本（`operand.go:38-52`，逐字）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Mode::Invalid => "invalid operand",
            Mode::NoValue => "no value",
            Mode::Builtin => "built-in",
            Mode::TypeExpr => "type",
            Mode::Constant => "constant",
            Mode::Variable => "variable",
            Mode::MapIndex => "map index expression",
            Mode::Value => "value",
            Mode::NilValue => "nil",
            Mode::CommaOk => "comma, ok expression",
            Mode::CommaErr => "comma, error expression",
        }
    }

    /// 全部 11 个模式（**声明序**，= 官方序）。
    pub const ALL: [Mode; 11] = [
        Mode::Invalid,
        Mode::NoValue,
        Mode::Builtin,
        Mode::TypeExpr,
        Mode::Constant,
        Mode::Variable,
        Mode::MapIndex,
        Mode::Value,
        Mode::NilValue,
        Mode::CommaOk,
        Mode::CommaErr,
    ];
}

/// 操作数：模式 + 类型 +（可选）常量值 +（可选）来源位置。
///
/// `typ` / `val` 用 `Option` 而不是「模式蕴含必然有」的强类型，是因为官方
/// 的 `operand.typ` 就是 `Type` 接口 + `nil` 判别 —— 映射阶段（`from_object`）
/// 之后才由各检查面逐步收窄；这里如实保留「不知道」。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Operand {
    /// 模式。
    pub mode: Mode,
    /// 类型；`None` ⇒ 不知道 / 不适用。
    pub typ: Option<TypeId>,
    /// 常量值；仅 [`Mode::Constant`] 会带（`iota` 的值待定 ⇒ `None`）。
    pub val: Option<Const>,
    /// 来源位置（`Object.pos`；universe 对象没有位置）。
    pub pos: Option<Pos>,
}

impl Operand {
    /// 只有模式的构造器（其余字段空）。
    fn bare(mode: Mode) -> Self {
        Self {
            mode,
            typ: None,
            val: None,
            pos: None,
        }
    }

    /// 无效操作数（错误传播态）。
    #[must_use]
    pub fn invalid() -> Self {
        Self::bare(Mode::Invalid)
    }

    /// 无值（无返回函数调用的结果）。
    #[must_use]
    pub fn no_value() -> Self {
        Self::bare(Mode::NoValue)
    }

    /// 内建函数。
    #[must_use]
    pub fn builtin() -> Self {
        Self::bare(Mode::Builtin)
    }

    /// 类型表达式。
    #[must_use]
    pub fn type_expr(typ: TypeId) -> Self {
        Self {
            typ: Some(typ),
            ..Self::bare(Mode::TypeExpr)
        }
    }

    /// 常量（类型 + 值）。
    #[must_use]
    pub fn constant(typ: TypeId, val: Const) -> Self {
        Self {
            typ: Some(typ),
            val: Some(val),
            ..Self::bare(Mode::Constant)
        }
    }

    /// 变量。
    #[must_use]
    pub fn variable(typ: Option<TypeId>) -> Self {
        Self {
            typ,
            ..Self::bare(Mode::Variable)
        }
    }

    /// map 索引表达式。
    #[must_use]
    pub fn map_index(typ: TypeId) -> Self {
        Self {
            typ: Some(typ),
            ..Self::bare(Mode::MapIndex)
        }
    }

    /// 计算值。
    #[must_use]
    pub fn value(typ: Option<TypeId>) -> Self {
        Self {
            typ,
            ..Self::bare(Mode::Value)
        }
    }

    /// `nil` 值（`typ` = 预声明的无类型 nil）。
    #[must_use]
    pub fn nil_value(typ: TypeId) -> Self {
        Self {
            typ: Some(typ),
            ..Self::bare(Mode::NilValue)
        }
    }

    /// 逗号-ok 操作数（`typ` = 主值类型）。
    #[must_use]
    pub fn comma_ok(typ: TypeId) -> Self {
        Self {
            typ: Some(typ),
            ..Self::bare(Mode::CommaOk)
        }
    }

    /// 逗号-err 操作数（第二个值是 `error`）。
    #[must_use]
    pub fn comma_err(typ: TypeId) -> Self {
        Self {
            typ: Some(typ),
            ..Self::bare(Mode::CommaErr)
        }
    }

    /// 从装载出的对象翻出一个操作数（映射表见模块头）。
    ///
    /// `PkgName` / `Label` ⇒ `None`：它们由各自的语境消费（选择器底座 /
    /// `goto` 目标），不是操作数。
    #[must_use]
    pub fn from_object(obj: &Object) -> Option<Self> {
        let mode = match obj.kind {
            ObjKind::Const => Mode::Constant,
            ObjKind::Var => Mode::Variable,
            ObjKind::TypeName => Mode::TypeExpr,
            ObjKind::Builtin => Mode::Builtin,
            ObjKind::Nil => Mode::NilValue,
            ObjKind::Func => Mode::Value,
            ObjKind::PkgName | ObjKind::Label => return None,
        };
        Some(Self {
            mode,
            typ: obj.typ,
            val: obj.val.clone(),
            pos: obj.pos,
        })
    }
}
