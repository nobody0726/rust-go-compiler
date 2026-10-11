//! 诊断 —— `C1` 契约的另一半（诊断表示与排序）。
//!
//! 来源：`docs/contracts/C1-source-map.md` §3 / §4。
//!
//! # 为什么 `DiagCode` 是契约的一部分而不是注释
//!
//! `code` 看着只是个标识，但它**参与排序**（`C1` §4 的第 4 个排序键）。
//! 所以它必须：
//!
//! - **稳定** —— 改了代号就改变诊断顺序 ⇒ 会改变与 golden 的比对结果 ⇒ 回归。
//! - **可排序** —— 统一 `大写-前缀-小写词`（`LEX-...`），无大小写混排陷阱。
//! - **可分组** —— 前缀标识来源子系统（`LEX-`），「某一类问题」能被一次性筛出来。
//!
//! 反面做法是把 `message` 当排序键 —— 文案一改就重排全部诊断。
//!
//! # 消息文本不复刻官方
//!
//! `03` §3.4 规定：诊断先对齐**拒绝/接受、位置和预定义类别**，**文本另列指标**。
//! 所以 golden 只存 `行:列 + DiagCode`（`M1-design.md` D-M1-9），
//! `message` 是给人看的，不参与任何比对。

use crate::pos::Pos;

/// 词法诊断代号。
///
/// **变体的声明顺序 == `as_str()` 的字典序**。这条由
/// `diag_code_的声明顺序与_as_str_字典序一致` 钉住 —— 不一致时，
/// 「同位置按 code 排序」在 Rust 侧（按 `Ord` = 声明顺序）与 golden 侧
/// （按字符串）会给出**不同的顺序**，而那种不一致只在两条诊断同位置时才暴露。
///
/// 加新代号时：**插入到它字典序该在的位置**，然后让那条断言告诉你有没有放对。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum DiagCode {
    /// 不认识的字符
    BadChar,
    /// 指数缺数字 / 进制与尾数不匹配
    BadExponent,
    /// 标识符里混入非法字符
    BadIdentChar,
    /// `_` 没有分隔连续的数码
    BadSep,
    /// 文件中部出现 BOM
    BomMidFile,
    /// 块注释未终止
    CommentUnterminated,
    /// 八进制转义值 > 255
    EscapeOctalOver,
    /// 转义值超范围或落在代理区
    EscapeRange,
    /// 未知转义
    EscapeUnknown,
    /// 超出单文件字节上限（**官方无对应行为**，是本仓的实现限制）
    ExceedsFileLimit,
    /// 标识符以数字开头
    IdLeadingDigit,
    /// NUL 字符
    InvalidNul,
    /// 非法的 UTF-8 编码
    InvalidUtf8,
    /// 进制前缀之后没有数字
    LitNoDigits,
    /// rune 字面量为空
    RuneEmpty,
    /// rune 字面量多于一个字符
    RuneMulti,
    /// rune 字面量里出现换行
    RuneNewline,
    /// rune 字面量未终止
    RuneUnterminated,
    /// 字符串未终止
    StringUnterminated,
}

impl DiagCode {
    /// 全部代号，**按声明顺序**（即字典序）。
    ///
    /// 供遍历式断言使用 —— 让「漏了一个代号」这件事有地方可查，
    /// 而不是靠人工维护一份可能过期的清单。
    pub const ALL: [DiagCode; 19] = [
        DiagCode::BadChar,
        DiagCode::BadExponent,
        DiagCode::BadIdentChar,
        DiagCode::BadSep,
        DiagCode::BomMidFile,
        DiagCode::CommentUnterminated,
        DiagCode::EscapeOctalOver,
        DiagCode::EscapeRange,
        DiagCode::EscapeUnknown,
        DiagCode::ExceedsFileLimit,
        DiagCode::IdLeadingDigit,
        DiagCode::InvalidNul,
        DiagCode::InvalidUtf8,
        DiagCode::LitNoDigits,
        DiagCode::RuneEmpty,
        DiagCode::RuneMulti,
        DiagCode::RuneNewline,
        DiagCode::RuneUnterminated,
        DiagCode::StringUnterminated,
    ];

    /// 机器可读代号文本。**它是契约的一部分**，改动等于契约变更。
    pub fn as_str(self) -> &'static str {
        match self {
            DiagCode::BadChar => "LEX-BAD-CHAR",
            DiagCode::BadExponent => "LEX-BAD-EXPONENT",
            DiagCode::BadIdentChar => "LEX-BAD-IDENT-CHAR",
            DiagCode::BadSep => "LEX-BAD-SEP",
            DiagCode::BomMidFile => "LEX-BOM-MIDFILE",
            DiagCode::CommentUnterminated => "LEX-COMMENT-UNTERMINATED",
            DiagCode::EscapeOctalOver => "LEX-ESCAPE-OCTAL-OVER",
            DiagCode::EscapeRange => "LEX-ESCAPE-RANGE",
            DiagCode::EscapeUnknown => "LEX-ESCAPE-UNKNOWN",
            DiagCode::ExceedsFileLimit => "LEX-EXCEEDS-FILE-LIMIT",
            DiagCode::IdLeadingDigit => "LEX-ID-LEADING-DIGIT",
            DiagCode::InvalidNul => "LEX-INVALID-NUL",
            DiagCode::InvalidUtf8 => "LEX-INVALID-UTF8",
            DiagCode::LitNoDigits => "LEX-LIT-NO-DIGITS",
            DiagCode::RuneEmpty => "LEX-RUNE-EMPTY",
            DiagCode::RuneMulti => "LEX-RUNE-MULTI",
            DiagCode::RuneNewline => "LEX-RUNE-NEWLINE",
            DiagCode::RuneUnterminated => "LEX-RUNE-UNTERMINATED",
            DiagCode::StringUnterminated => "LEX-STRING-UNTERMINATED",
        }
    }
}

/// 一条诊断。
///
/// `message` **不参与排序、不参与比对** —— 只给人看（见模块文档）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diag {
    /// 出错位置
    pub pos: Pos,
    /// 机器可读代号（参与排序）
    pub code: DiagCode,
    /// 人类可读描述（不参与排序）
    pub message: String,
}

/// 诊断集合。
///
/// 取用**只有一条路**：[`DiagBag::into_sorted`]，且它**消费 `self`**。
///
/// 之所以消费而不是返回 `&[Diag]`：若返回引用，调用方可能拿它去渲染而**忘记先排序** ——
/// 那就绕过了这套类型存在的唯一理由（`C1` §4.3）。消费 `self` 让排序成为
/// **类型上不可绕过**的一步。
#[derive(Default, Debug)]
pub struct DiagBag {
    items: Vec<Diag>,
}

impl DiagBag {
    /// 建一个空集合。
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一条诊断（无序）。
    pub fn push(&mut self, d: Diag) {
        self.items.push(d);
    }

    /// 已收条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 消费并返回**排好序**的诊断。
    ///
    /// 排序键（`C1` §4，按优先级）：`pos.file` → `pos.line` → `pos.col` → `code`。
    /// 前三个由 [`Pos`] 的 `Ord` 提供；第 4 个在这里补上。
    ///
    /// 第 4 个键不是可有可无的洁癖：**同一位置的多条诊断**若顺序取决于插入/哈希顺序，
    /// 输出就不确定，而 `E6` 要求重复执行结果一致 —— 同位置两条诊断的顺序
    /// 正是最容易泄漏随机性的地方。
    pub fn into_sorted(self) -> Vec<Diag> {
        let mut v = self.items;
        v.sort_by(|a, b| a.pos.cmp(&b.pos).then_with(|| a.code.cmp(&b.code)));
        v
    }
}
