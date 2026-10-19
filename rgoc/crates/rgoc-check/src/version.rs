//! Go 语言版本门控的**特性表**（M4/Task 17；设计 `F1`）。
//!
//! # 判据来源
//!
//! 官方是两件套（`go/types/version.go:56-72`）：
//!
//! * `allowVersion(want)`：纯谓词 —— 生效版本为空（不门控）或 `>= want`；
//! * `verifyVersionf(at, want, format, …)`：谓词为假时追加一条
//!   `UnsupportedFeature`，消息 `"%s requires %s or later"`（`errors.go:253-258`）。
//!
//! 本模块把**站点**冻结成 [`Feat`] 表：每个变体 = 官方一处 `go1.N` 阈值。
//! 调用面（Task 20+ 的 `check.rs` / `expr.rs` / …）只报「哪个特性、在哪」，
//! 阈值与文案都由表给 —— 阈值散落在检查逻辑里正是 `F1` 想避免的漂移。
//!
//! # 与官方的一处**有意分叉**
//!
//! [`Feat::ComparableEmbedding`]（1.14，`typeset.go:243`）的官方报错是
//! **重复方法**（类别 `decl`），**不是** `UnsupportedFeature`；阈值只决定
//! 「签名相同的重叠嵌入算不算重复」。所以它在表里保留阈值，
//! [`Feat::emits_version_error`] 返回 `false`，调用面用 [`allow`] 自报；
//! [`verify_version`] 对它只返回判定、不推诊断。
//!
//! # 消息只是**形态**，不是对撞判据
//!
//! `C8` 的对撞面只比对「位置 + 类别」（`M4-design.md` §5.1），所以
//! [`Feat::what`] 的拼法照官方 `format` 的静态/动态结构，但不承诺逐字
//! （例如 range 的官方消息由 range 检查拼 `cannot range over …:` 前缀）。
//! 需要逐字时在调用面拼，表只提供**特性主体**。
//!
//! # `F1` 的全表去哪了
//!
//! [`Feat::ALL`] 的 31 项覆盖 `M4-design.md` §2.3 `F1` 的**全部阈值**
//! （含同一特性的多个内层站点：`call.go:90/396/560`、`instantiate.go:316`、
//! `resolver.go:427`、`range.go:69` —— 它们共用表里的同一变体）。
//! 冻结快照在 `tests/version_table.rs` 的 `FROZEN`。

use rgoc_ast::expr::CallExpr;
use rgoc_ast::stmt::RangeClause;
use rgoc_lex::{Diag, DiagBag, DiagCode, FileId, Pos};
use rgoc_types::version::{GoVersion, Versions};

use crate::diag::RawDiag;

/// 一处版本门控（官方 `verifyVersionf` / `allowVersion` 的站点）。
///
/// 每项的文档给：**官方阈值出处**（语料快照 `go1.27.1` 行号）与
/// **官方消息主体**（`%s` 处由调用方给 `subject`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feat {
    /// `type alias` = 1.9 —— `decl.go:524`（`atPos(tdecl.Assign)`）。
    AliasDecl,
    /// `signed shift count %s` = 1.13 —— `expr.go:664`（消息带 `invalid operation: ` 前缀）。
    SignedShiftCount,
    /// `underscore in numeric literal` = 1.13 —— `literals.go:28`。
    UnderscoreInNumericLiteral,
    /// `binary literal` = 1.13 —— `literals.go:36`。
    BinaryLiteral,
    /// `0o/0O-style octal literal` = 1.13 —— `literals.go:40`。
    Octal0oLiteral,
    /// `hexadecimal floating-point literal` = 1.13 —— `literals.go:44`。
    HexFloatLiteral,
    /// 1.14 —— `typeset.go:243`。**阈值面**：官方在此报的是重复方法
    /// （`DuplicateDecl`），不是 version 错（见模块头）。
    ComparableEmbedding,
    /// `unsafe.Add` = 1.17 —— `builtins.go:737`。
    UnsafeAdd,
    /// `unsafe.Slice` = 1.17 —— `builtins.go:869`。
    UnsafeSlice,
    /// `conversion of slice to array pointer` = 1.17 —— `conversions.go:219`。
    SliceToArrayPointer,
    /// `type parameter` = 1.18 —— `decl.go:561` / `resolver.go:427`。
    TypeParameter,
    /// `function instantiation` = 1.18 —— `call.go:44/301`。
    FunctionInstantiation,
    /// `type instantiation` = 1.18 —— `signature.go:307` / `typexpr.go:277`。
    TypeInstantiation,
    /// `using type constraint %s` = 1.18 —— `decl.go:508`。
    ImportedConstraint,
    /// `embedding constraint interface %s` = 1.18 —— `typeset.go:276`。
    EmbeddingConstraintInterface,
    /// `embedding interface element %s` = 1.18 —— `typeset.go:285`。
    EmbeddingInterfaceElement,
    /// `embedding non-interface type %s` = 1.18 —— `typeset.go:299`。
    EmbeddingNonInterfaceType,
    /// `predeclared %s` = 1.18 —— `typexpr.go:34`（`any` / `comparable`）。
    PredeclaredAnyComparable,
    /// `conversion of slice to array` = 1.20 —— `conversions.go:206`。
    SliceToArray,
    /// `unsafe.SliceData` = 1.20 —— `builtins.go:891`。
    UnsafeSliceData,
    /// `unsafe.String` = 1.20 —— `builtins.go:908`。
    UnsafeString,
    /// `unsafe.StringData` = 1.20 —— `builtins.go:928`。
    UnsafeStringData,
    /// `clear` = 1.21 —— `builtins.go:234`。
    Clear,
    /// `built-in %s` = 1.21 —— `builtins.go:587`（`min` / `max`）。
    BuiltinMinMax,
    /// `implicitly instantiated function as argument` = 1.21 —— `call.go:617`。
    ImplicitInstantiationAsArgument,
    /// `range over int` = 1.22 —— `range.go:227`（官方消息由 range 检查拼前缀）。
    RangeOverInt,
    /// `range over func` = 1.23 —— `range.go:242`（同上）。
    RangeOverFunc,
    /// `generic type alias` = 1.23 —— `decl.go:521`。
    GenericTypeAlias,
    /// `new(%s)` = 1.26 —— `builtins.go:672`。
    NewExpr,
    /// `use of promoted field %s in struct literal of type %s` = 1.27 ——
    /// `literals.go:194`。`subject` 给两个 `%s` 拼好的整段。
    PromotedFieldKey,
    /// `generic method` = 1.27 —— `resolver.go:448`。
    GenericMethod,
}

impl Feat {
    /// 全表（顺序与 `tests/version_table.rs` 的 `FROZEN` 一致）。
    pub const ALL: [Feat; 31] = [
        Feat::AliasDecl,
        Feat::SignedShiftCount,
        Feat::UnderscoreInNumericLiteral,
        Feat::BinaryLiteral,
        Feat::Octal0oLiteral,
        Feat::HexFloatLiteral,
        Feat::ComparableEmbedding,
        Feat::UnsafeAdd,
        Feat::UnsafeSlice,
        Feat::SliceToArrayPointer,
        Feat::TypeParameter,
        Feat::FunctionInstantiation,
        Feat::TypeInstantiation,
        Feat::ImportedConstraint,
        Feat::EmbeddingConstraintInterface,
        Feat::EmbeddingInterfaceElement,
        Feat::EmbeddingNonInterfaceType,
        Feat::PredeclaredAnyComparable,
        Feat::SliceToArray,
        Feat::UnsafeSliceData,
        Feat::UnsafeString,
        Feat::UnsafeStringData,
        Feat::Clear,
        Feat::BuiltinMinMax,
        Feat::ImplicitInstantiationAsArgument,
        Feat::RangeOverInt,
        Feat::RangeOverFunc,
        Feat::GenericTypeAlias,
        Feat::NewExpr,
        Feat::PromotedFieldKey,
        Feat::GenericMethod,
    ];

    /// 引入版本（官方 `go1_N`）。
    #[must_use]
    pub const fn since(self) -> GoVersion {
        let minor = match self {
            Self::AliasDecl => 9,
            Self::SignedShiftCount
            | Self::UnderscoreInNumericLiteral
            | Self::BinaryLiteral
            | Self::Octal0oLiteral
            | Self::HexFloatLiteral => 13,
            Self::ComparableEmbedding => 14,
            Self::UnsafeAdd | Self::UnsafeSlice | Self::SliceToArrayPointer => 17,
            Self::TypeParameter
            | Self::FunctionInstantiation
            | Self::TypeInstantiation
            | Self::ImportedConstraint
            | Self::EmbeddingConstraintInterface
            | Self::EmbeddingInterfaceElement
            | Self::EmbeddingNonInterfaceType
            | Self::PredeclaredAnyComparable => 18,
            Self::SliceToArray
            | Self::UnsafeSliceData
            | Self::UnsafeString
            | Self::UnsafeStringData => 20,
            Self::Clear | Self::BuiltinMinMax | Self::ImplicitInstantiationAsArgument => 21,
            Self::RangeOverInt => 22,
            Self::RangeOverFunc | Self::GenericTypeAlias => 23,
            Self::NewExpr => 26,
            Self::PromotedFieldKey | Self::GenericMethod => 27,
        };
        GoVersion::new(minor)
    }

    /// 稳定标识（台账 / 测试 / 未来 `--diag-dump` 的调试栏）。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AliasDecl => "alias-decl",
            Self::SignedShiftCount => "signed-shift-count",
            Self::UnderscoreInNumericLiteral => "underscore-in-numeric-literal",
            Self::BinaryLiteral => "binary-literal",
            Self::Octal0oLiteral => "octal-0o-literal",
            Self::HexFloatLiteral => "hex-float-literal",
            Self::ComparableEmbedding => "comparable-embedding",
            Self::UnsafeAdd => "unsafe-add",
            Self::UnsafeSlice => "unsafe-slice",
            Self::SliceToArrayPointer => "slice-to-array-pointer",
            Self::TypeParameter => "type-parameter",
            Self::FunctionInstantiation => "function-instantiation",
            Self::TypeInstantiation => "type-instantiation",
            Self::ImportedConstraint => "imported-constraint",
            Self::EmbeddingConstraintInterface => "embedding-constraint-interface",
            Self::EmbeddingInterfaceElement => "embedding-interface-element",
            Self::EmbeddingNonInterfaceType => "embedding-non-interface-type",
            Self::PredeclaredAnyComparable => "predeclared-any-comparable",
            Self::SliceToArray => "slice-to-array",
            Self::UnsafeSliceData => "unsafe-slice-data",
            Self::UnsafeString => "unsafe-string",
            Self::UnsafeStringData => "unsafe-string-data",
            Self::Clear => "clear",
            Self::BuiltinMinMax => "builtin-min-max",
            Self::ImplicitInstantiationAsArgument => "implicit-instantiation-as-argument",
            Self::RangeOverInt => "range-over-int",
            Self::RangeOverFunc => "range-over-func",
            Self::GenericTypeAlias => "generic-type-alias",
            Self::NewExpr => "new-expr",
            Self::PromotedFieldKey => "promoted-field-key",
            Self::GenericMethod => "generic-method",
        }
    }

    /// 官方 `verifyVersionf` 是否真的报 version 错。
    ///
    /// 只有 [`Self::ComparableEmbedding`] 返回 `false`（官方报重复方法）。
    #[must_use]
    pub const fn emits_version_error(self) -> bool {
        !matches!(self, Self::ComparableEmbedding)
    }

    /// 官方 `format` 的**消息主体**（`%s` 由 `subject` 填）。
    ///
    /// 无动态部分的特性忽略 `subject`。
    #[must_use]
    pub fn what(self, subject: &str) -> String {
        match self {
            Self::AliasDecl => "type alias".to_string(),
            Self::SignedShiftCount => format!("signed shift count {subject}"),
            Self::UnderscoreInNumericLiteral => "underscore in numeric literal".to_string(),
            Self::BinaryLiteral => "binary literal".to_string(),
            Self::Octal0oLiteral => "0o/0O-style octal literal".to_string(),
            Self::HexFloatLiteral => "hexadecimal floating-point literal".to_string(),
            Self::ComparableEmbedding => "comparable (a constraint)".to_string(),
            Self::UnsafeAdd => "unsafe.Add".to_string(),
            Self::UnsafeSlice => "unsafe.Slice".to_string(),
            Self::SliceToArrayPointer => "conversion of slice to array pointer".to_string(),
            Self::TypeParameter => "type parameter".to_string(),
            Self::FunctionInstantiation => "function instantiation".to_string(),
            Self::TypeInstantiation => "type instantiation".to_string(),
            Self::ImportedConstraint => format!("using type constraint {subject}"),
            Self::EmbeddingConstraintInterface => {
                format!("embedding constraint interface {subject}")
            }
            Self::EmbeddingInterfaceElement => format!("embedding interface element {subject}"),
            Self::EmbeddingNonInterfaceType => format!("embedding non-interface type {subject}"),
            Self::PredeclaredAnyComparable => format!("predeclared {subject}"),
            Self::SliceToArray => "conversion of slice to array".to_string(),
            Self::UnsafeSliceData => "unsafe.SliceData".to_string(),
            Self::UnsafeString => "unsafe.String".to_string(),
            Self::UnsafeStringData => "unsafe.StringData".to_string(),
            Self::Clear => "clear".to_string(),
            Self::BuiltinMinMax => format!("built-in {subject}"),
            Self::ImplicitInstantiationAsArgument => {
                "implicitly instantiated function as argument".to_string()
            }
            Self::RangeOverInt => "range over int".to_string(),
            Self::RangeOverFunc => "range over func".to_string(),
            Self::GenericTypeAlias => "generic type alias".to_string(),
            Self::NewExpr => format!("new({subject})"),
            Self::PromotedFieldKey => format!("use of promoted field {subject}"),
            Self::GenericMethod => "generic method".to_string(),
        }
    }
}

/// 官方 `allowVersion` 的对应物：该文件是否允许 `feat`。
#[must_use]
pub fn allow(versions: &Versions, file: FileId, feat: Feat) -> bool {
    versions.allow(file, feat.since())
}

/// 官方 `verifyVersionf` 的对应物。
///
/// 返回是否允许；拒绝时，若 [`Feat::emits_version_error`] 为真，追加**恰一条**
/// [`DiagCode::TypFeatureRequiresGoVersion`]（位置 = `at`）。
///
/// `subject` 是官方 `format` 的动态部分（无动态部分的特性忽略它）。
pub fn verify_version(
    versions: &Versions,
    file: FileId,
    at: Pos,
    feat: Feat,
    subject: &str,
    bag: &mut DiagBag,
) -> bool {
    if allow(versions, file, feat) {
        return true;
    }
    if feat.emits_version_error() {
        bag.push(version_error(at, feat, subject, false).diag);
    }
    false
}

/// 官方 `softErrorf` 的对应物（**soft** 版本门控）。
///
/// 与 [`verify_version`] 的判定完全相同，只是把诊断标成
/// [`RawDiag::soft`]（官方 `types.Error.Soft`）。返回 `None` = 允许，
/// 或该特性是「阈值面」（[`Feat::emits_version_error`] 为假）——
/// 两种情形都不该产生 version 诊断。
///
/// 调用面（Task 26 的 range 门控）把它放进检查器的**原始诊断流**；
/// 渲染层对 soft / hard 一视同仁（`C8`）。
#[must_use]
pub fn verify_version_soft(
    versions: &Versions,
    file: FileId,
    at: Pos,
    feat: Feat,
    subject: &str,
) -> Option<RawDiag> {
    if allow(versions, file, feat) || !feat.emits_version_error() {
        return None;
    }
    Some(version_error(at, feat, subject, true))
}

/// 造一条门控诊断（**不看**版本；调用方负责判定）。
fn version_error(at: Pos, feat: Feat, subject: &str, soft: bool) -> RawDiag {
    RawDiag {
        diag: Diag {
            pos: at,
            code: DiagCode::TypFeatureRequiresGoVersion,
            message: format!("{} requires {} or later", feat.what(subject), feat.since()),
        },
        soft,
    }
}

/// `new(expr)` 的门控位置 = **`new` 标识符**。
///
/// 官方 `verifyVersionf(call.Fun, go1_26, "new(%s)", arg)`（`builtins.go:672`）；
/// 实测 `go1_25.go` 四条全在 `:6`（`new` 的起始列），**不是** `(` 或实参。
#[must_use]
pub fn pos_of_new_call(call: &CallExpr) -> Pos {
    call.fun.pos()
}

/// `range` 门控的位置 = **被 range 的表达式**。
///
/// 官方 `softErrorf(&x, InvalidRangeExpr, "cannot range over %s: %s", …)`
/// （`range.go:72`）；实测 `go1_22_21.go` 在 `14:16`（操作数 `10`），
/// **不是** `range` 关键字（后者是 `14:10`）。
#[must_use]
pub fn pos_of_range_operand(r: &RangeClause) -> Pos {
    r.x.pos()
}
