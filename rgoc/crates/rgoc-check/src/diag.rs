//! `rgoc-check` 的诊断：原始诊断位 + 类别投影。
//!
//! # 两面
//!
//! - **原始诊断**（Task 2 起步、Task 20 定型）：[`RawDiag`] 是 `rgoc_lex::Diag`
//!   加官方 `types.Error.Soft` 位；检查阶段自己算位置（`Pos` 从 `rgoc-lex` 来）。
//!   将来新增的 `CHK-` 码落位在 `rgoc-lex::diag::DiagCode`，与
//!   `LEX-`/`SYN-`/`TYP-` 同表。
//! - **类别投影**（Task 11）：[`DiagCode`] → `C8` §3 的 26 类枚举，
//!   **全量穷尽**（编译期 `match` 无 `_` 通配 + 计数断言，见
//!   `tests/category.rs` 的 `T-CK-16`）。进 `--diag-dump`（对撞面）的每条
//!   诊断都必须能投影 —— 投影不进去是**设计事故**，而不是「无类别」。
//!
//! # 类别（`C8` §3 冻结 v1，26 类）
//!
//! 枚举是**封闭集合**；[`Category::Other`] 是**逃逸口**：按 `T-CK-94` 的
//! 纪律，入选语料上出现 `other` 即红。值域定案（**Ruling 11-1**，经
//! **Ruling 20-1** 收窄）= 恰好 3 个「无官方语义」码 ——
//! [`DiagCode::ExceedsFileLimit`] / [`DiagCode::SynNestingTooDeep`] /
//! [`DiagCode::TypNestingTooDeep`]（本仓实现限制；官方无对应行为）。（原第 4
//! 码 `ChkUnimplemented` 随 Task 20 的检查会话真身落地退出 —— 五件套纪律
//! 「没有触发者的码不许进枚举」。）其余全部映射到真实语义族 —— 包括能力边界
//! **`U-` 码**（`TypUnsupported*`：官方有答案、我们阶段没做到，投影到各自
//! 语义族，不用 `other` 顶包）。
//!
//! ⚠️ 新增 `CHK-` 码时：先按字典序落进 `rgoc-lex::diag::DiagCode`（五件套），
//! 再在 [`category_of`] 里归类 —— 忘了归类**编译不过**（无通配的穷尽
//! `match`），这就是本文件存在的意义。

use rgoc_lex::{Diag, DiagCode};

/// `C8` §3 的类别枚举（26 类，冻结 v1）。
///
/// 声明序**无关**（token 才是契约）；[`Category::as_str`] 与 `ALL_CATEGORIES`
/// 是渲染与断言的唯一入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    /// 赋值 / 可赋值性面（`cannot use x as T` 族）。
    Assign,
    /// 内建函数使用面（`len`/`cap`/`append`/`make`/`new`/…）。
    Builtin,
    /// 逗号-ok 语境（多重赋值 / map 取双值 / 类型断言双值）。
    CommaOk,
    /// 转换面（`cannot convert` / `truncated to`）。
    Convert,
    /// 声明 / 初始化**环**（类型环、声明环、初始化环）。
    Cycle,
    /// 声明面（重复声明、非法声明、`iota`/初始化式数量等）。
    Decl,
    /// 接口实现 / 约束满足面（`does not satisfy` 族）。
    Implements,
    /// import 面（`could not import`、环、非法形态）。
    Import,
    /// 索引面（越界、非法索引类型）。
    Index,
    /// 「不是类型 / 不是表达式」面（`used as value` 族）。
    NotType,
    /// 非法操作面（`invalid operation:` 通用前缀族）。
    OpInvalid,
    /// **逃逸口**：无官方语义的能力边界诊断（见模块头 Ruling 11-1 / 20-1）。
    Other,
    /// 常量溢出 / 表示范围面。
    Overflow,
    /// 解析（词法 / 语法）面诊断的统一类别。
    Parse,
    /// `range` 静态面（不可 range、迭代变量个数等）。
    Range,
    /// 返回值面（`return` 值与结果不匹配）。
    Ret,
    /// 选择器 / 字段提升 / 方法值面。
    Selector,
    /// 移位面（计数与操作数）。
    Shift,
    /// 切片面（切片表达式与索引约束）。
    Slice,
    /// 语句静态规则面（break/continue/goto/fallthrough/switch/select）。
    Stmt,
    /// 终止语句面（`missing return`）。
    Terminate,
    /// 类型实参 / 实例化面（推断失败、实参不符、缺实例化）。
    TypeArg,
    /// 未声明名字面（`undefined` / `not declared`）。
    Undef,
    /// 未使用面（变量 / 标签 / import `not used`）。
    Unused,
    /// `unsafe` 白名单与 `Pointer` 使用面。
    Unsafe,
    /// 版本门控面（`requires go1.N or later` 族）。
    Version,
}

impl Category {
    /// 渲染 token（`C8` §5.2 的 `<类别>` 段；`M4-tests.md` §5.2 冻结）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Category::Assign => "assign",
            Category::Builtin => "builtin",
            Category::CommaOk => "comma-ok",
            Category::Convert => "convert",
            Category::Cycle => "cycle",
            Category::Decl => "decl",
            Category::Implements => "implements",
            Category::Import => "import",
            Category::Index => "index",
            Category::NotType => "not-type",
            Category::OpInvalid => "op-invalid",
            Category::Other => "other",
            Category::Overflow => "overflow",
            Category::Parse => "parse",
            Category::Range => "range",
            Category::Ret => "ret",
            Category::Selector => "selector",
            Category::Shift => "shift",
            Category::Slice => "slice",
            Category::Stmt => "stmt",
            Category::Terminate => "terminate",
            Category::TypeArg => "type-arg",
            Category::Undef => "undef",
            Category::Unused => "unused",
            Category::Unsafe => "unsafe",
            Category::Version => "version",
        }
    }
}

/// 检查器的**原始诊断**：`rgoc_lex::Diag` + 官方 `types.Error.Soft`（`api.go:55`）。
///
/// # 为什么 soft 要留到渲染层
///
/// 官方 range 门控走 `softErrorf`（`range.go:72`），产出的错误带 `Soft: true`；
/// 但 `Soft` **不参与** check 面判据（`check_test.go` 的 `Error` 回调只看
/// `Pos/Msg`），渲染（`C8`）也只比「位置 + 类别」。所以我们**保留**这个位
/// 而**不**让它改变任何输出 —— 丢掉它则等于丢掉官方的公开字段，将来任何
/// 需要区分软硬的面（工具、IDE）都拿不回来。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawDiag {
    /// 诊断本体。
    pub diag: Diag,
    /// 官方 `types.Error.Soft`。
    pub soft: bool,
}

impl RawDiag {
    /// 硬错误（默认；`soft = false`）。
    #[must_use]
    pub fn hard(diag: Diag) -> Self {
        Self { diag, soft: false }
    }

    /// 软错误（官方 `softErrorf` 面）。
    #[must_use]
    pub fn soft(diag: Diag) -> Self {
        Self { diag, soft: true }
    }

    /// `C8` 数据行：`<行号>:<列>\t<类别>`（`C8` §2 规则 4）。
    ///
    /// soft 与 hard **同形** —— 软硬不进入渲染，也不进入比较。
    #[must_use]
    pub fn render_row(&self) -> String {
        format!(
            "{}:{}\t{}",
            self.diag.pos.line,
            self.diag.pos.col,
            category_of(self.diag.code).as_str()
        )
    }
}

/// 26 类全部成员，**按 token 字典序**（`C8` §3 的文档序；`T-CK-16` 断言
/// 与文档枚举块逐字一致）。
pub const ALL_CATEGORIES: [Category; 26] = [
    Category::Assign,
    Category::Builtin,
    Category::CommaOk,
    Category::Convert,
    Category::Cycle,
    Category::Decl,
    Category::Implements,
    Category::Import,
    Category::Index,
    Category::NotType,
    Category::OpInvalid,
    Category::Other,
    Category::Overflow,
    Category::Parse,
    Category::Range,
    Category::Ret,
    Category::Selector,
    Category::Shift,
    Category::Slice,
    Category::Stmt,
    Category::Terminate,
    Category::TypeArg,
    Category::Undef,
    Category::Unused,
    Category::Unsafe,
    Category::Version,
];

/// 码 → 类别：**全量穷尽**（无 `_` 通配；新增码漏判 = 编译错误）。
///
/// 归类的判据 = 「该码官方对应消息经 oracle 适配表得到的类别」
/// （`tools/types-oracle/check.go` 的 `c8Rules` 即 26 类的**语义字典**）；
/// 官方无对应行为的实现限制码（`ExceedsFileLimit` / `*NestingTooDeep`）→
/// [`Category::Other`]（Ruling 11-1 / 20-1）。
///
/// 少数**跨语境**码（`TypTruncatedFloat` / `TypInvalidConversion` /
/// `TypNumericOverflow` / `TypInvalidConstVal`）官方在不同语法语境下措辞不同、
/// 类别随之不同（`(truncated)` 后缀归 `assign`；`truncated to` 归 `convert`；
/// 等）；本表取**主导语境**的近似值，具体行级错配由 `T-CK-94` / Task 37–39
/// 的语料收口校准（必要时拆码）。
pub fn category_of(code: DiagCode) -> Category {
    use DiagCode::*;
    match code {
        // ── parse：词法面（LEX-）─────────────────────────────────────────
        BadChar | BadDigit | BadExponent | BadIdentChar | BadRadixPoint | BadSep | BomMidFile
        | CommentUnterminated | EscapeBadDigit | EscapeOctalOver | EscapeRange | EscapeUnknown
        | IdLeadingDigit | InvalidNul | InvalidUtf8 | LitNoDigits | RuneEmpty | RuneMulti
        | RuneNewline | RuneUnterminated | StringNewline | StringUnterminated => Category::Parse,

        // ── parse：语法面（SYN- 的纯解析三类）───────────────────────────
        SynExpectedSemicolon | SynInvalidLineDirective | SynUnexpectedToken => Category::Parse,

        // ── stmt：跳转 / 分支面（SYN- 的 branches.go 族 + TYP- 的 labels.go 族）──
        SynBranchBreak | SynBranchContinue | SynBranchFallthrough | SynGotoIntoBlock
        | SynGotoJumpsOverDecl | TypJumpIntoBlock | TypMisplacedLabel => Category::Stmt,

        // ── decl：声明面 ────────────────────────────────────────────────
        SynLabelDuplicate | TypBlankPkgName | TypDuplicateDecl | TypDuplicateLabel
        | TypInvalidConstInit | TypMismatchedPkgName | TypRepeatedDecl | TypWrongAssignCount => {
            Category::Decl
        }

        // ── undef / unused ─────────────────────────────────────────────
        SynLabelUndefined | TypUndeclaredLabel | TypUndeclaredName => Category::Undef,
        SynLabelUnused | TypUnusedLabel => Category::Unused,

        // ── cycle / import / version / shift ───────────────────────────
        TypInvalidDeclCycle
        | TypInvalidInitCycle
        | TypInvalidInstanceCycle
        | TypInvalidRecursiveType => Category::Cycle,
        TypBrokenImport | TypImportCycle | TypUnsupportedImportForm => Category::Import,
        TypFeatureRequiresGoVersion | TypFileRequiresNewerGo | TypPackageRequiresNewerGo => {
            Category::Version
        }
        TypInvalidShiftCount | TypInvalidShiftOperand => Category::Shift,
        TypUnsupportedPendingInstanceArg => Category::TypeArg,

        // ── 单码类别 ────────────────────────────────────────────────────
        TypDivByZero => Category::OpInvalid,
        TypInvalidBlank => Category::NotType,
        TypInvalidTypeArg => Category::Implements,
        TypWrongTypeArgCount => Category::TypeArg,
        TypCannotInferTypeArgs => Category::TypeArg,

        // ── 跨语境近似（见函数文档；T-CK-94 收口校准）────────────────────
        TypInvalidConversion | TypTruncatedFloat => Category::Convert,
        TypInvalidConstVal | TypNumericOverflow => Category::Overflow,
        TypUnsupportedAssignability => Category::Assign,

        // ── other：无官方语义（Ruling 11-1 / 20-1；T-CK-94 出现即红）──────
        ExceedsFileLimit | SynNestingTooDeep | TypNestingTooDeep => Category::Other,
    }
}
