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
//!
//! # 这张码表是**枚举**出来的，不是想出来的
//!
//! 第一版只有 19 个代号，是**凭印象**从官方 scanner 的报错消息里挑的 —— 结果漏了 4 个
//! （`LEX-BAD-DIGIT` / `LEX-BAD-RADIX-POINT` / `LEX-STRING-NEWLINE` / `LEX-ESCAPE-BAD-DIGIT`）。
//! 漏掉时**不会报错**：那 4 条消息出现时行为未定义 —— 与 `AGENTS.md` 记的
//! 「写死的键列表会变成盲区」同形。
//!
//! 现在是**枚举** `scanner.go` / `source.go` 里全部 `errorf`/`errorAtf`/`error` 调用点
//! （26 处 ⇒ 去重 25 条消息模板）得到 22 个官方代号，再加 1 个本仓自己的限制，共 **23**（M1 冻结）。
//! M2 增补 3 个 `SYN-` 前缀语法代号（见 [`DiagCode`] 末尾），**当前共 26**。
//!
//! 两侧（Go 的 `tools/token-oracle/diagmap.go` 与这里）必须同步 ——
//! 由 `lex_代号表与_go_侧映射表逐字一致` 跨语言对撞，而不是靠人记得同步。
//!
//! ⚠️ 但那只对撞 **`LEX-` 子表**：token oracle 的映射表是词法专用的，`SYN-` 代号在那张表里
//! 永远不可能被产出（它的自检 ② 要求每个代号都有官方消息能产出）。`SYN-` 的半边由
//! `tools/ast-oracle` 的镜像表覆盖（T-AP16）。前缀与镜像表的对应关系见
//! `diag_code_的_as_str_前缀落在阶段白名单内`。

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
    /// 进制字面量里出现了不属于该进制的数位（如 `0b12` 里的 `2`）
    BadDigit,
    /// 指数缺数字 / 进制与尾数不匹配
    BadExponent,
    /// 标识符里混入非法字符
    BadIdentChar,
    /// 进制字面量里出现小数点（如 `0b1.0`）—— 只有十进制与十六进制允许
    BadRadixPoint,
    /// `_` 没有分隔连续的数码
    BadSep,
    /// 文件中部出现 BOM
    BomMidFile,
    /// 块注释未终止
    CommentUnterminated,
    /// 转义序列里的数位非法（如 `\xZZ`）
    EscapeBadDigit,
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
    /// 解释型字符串里出现裸换行
    StringNewline,
    /// 字符串未终止
    StringUnterminated,

    // ── M2 语法分析（`SYN-` 前缀）──
    /// 期望 `;`
    SynExpectedSemicolon,
    /// `//line` 指令格式非法
    SynInvalidLineDirective,
    /// 意外的词元
    SynUnexpectedToken,
}

impl DiagCode {
    /// 全部代号，**按声明顺序**（即字典序）。
    ///
    /// 供遍历式断言使用 —— 让「漏了一个代号」这件事有地方可查，
    /// 而不是靠人工维护一份可能过期的清单。
    pub const ALL: [DiagCode; 26] = [
        DiagCode::BadChar,
        DiagCode::BadDigit,
        DiagCode::BadExponent,
        DiagCode::BadIdentChar,
        DiagCode::BadRadixPoint,
        DiagCode::BadSep,
        DiagCode::BomMidFile,
        DiagCode::CommentUnterminated,
        DiagCode::EscapeBadDigit,
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
        DiagCode::StringNewline,
        DiagCode::StringUnterminated,
        DiagCode::SynExpectedSemicolon,
        DiagCode::SynInvalidLineDirective,
        DiagCode::SynUnexpectedToken,
    ];

    /// 机器可读代号文本。**它是契约的一部分**，改动等于契约变更。
    pub fn as_str(self) -> &'static str {
        match self {
            DiagCode::BadChar => "LEX-BAD-CHAR",
            DiagCode::BadDigit => "LEX-BAD-DIGIT",
            DiagCode::BadExponent => "LEX-BAD-EXPONENT",
            DiagCode::BadIdentChar => "LEX-BAD-IDENT-CHAR",
            DiagCode::BadRadixPoint => "LEX-BAD-RADIX-POINT",
            DiagCode::BadSep => "LEX-BAD-SEP",
            DiagCode::BomMidFile => "LEX-BOM-MIDFILE",
            DiagCode::CommentUnterminated => "LEX-COMMENT-UNTERMINATED",
            DiagCode::EscapeBadDigit => "LEX-ESCAPE-BAD-DIGIT",
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
            DiagCode::StringNewline => "LEX-STRING-NEWLINE",
            DiagCode::StringUnterminated => "LEX-STRING-UNTERMINATED",
            DiagCode::SynExpectedSemicolon => "SYN-EXPECTED-SEMICOLON",
            DiagCode::SynInvalidLineDirective => "SYN-INVALID-LINE-DIRECTIVE",
            DiagCode::SynUnexpectedToken => "SYN-UNEXPECTED-TOKEN",
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
