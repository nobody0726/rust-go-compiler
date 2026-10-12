//! 分号插入的状态机（`M1-design.md` §5.4；LEX-07）。
//!
//! # 一句话
//!
//! 官方在 `next()` 开头取走并复位（`nlsemi := s.nlsemi; s.nlsemi = false`），
//! 在产出词元的各处按类别设置。这个「按类别设置」的规则表就是本模块的全部内容。
//!
//! # 为什么独立成对象
//!
//! LEX-07 是本阶段**唯一跨调用保留状态**的地方。埋在 `next()` 里就只能靠端到端
//! golden 反推；独立出来后规则表可以逐行断言（`T-L-49`，判据 `S`）。
//!
//! # 官方依据：六处设置点（一条不多、一条不少）
//!
//! | # | 官方位置 | 产出什么 | 置位 |
//! |---|---|---|---|
//! | 1 | `setLit()`（`scanner.go:64`） | `_Literal` 一族（**不论 `ok` 与否**） | ✅ |
//! | 2 | `ident()` 尾部（`scanner.go:391`） | `_Name` | ✅ |
//! | 3 | `ident()` 关键字分支（`scanner.go:385`） | 仅 `break`/`continue`/`fallthrough`/`return` | ✅×4 |
//! | 4 | `case ')'` `']'` `'}'`（`:159/164/169`） | 三个**右**定界符 | ✅ |
//! | 5 | `case '+'` `'-'` 的第二字符分支（`:206/216`） | `++` / `--`（`_IncOp`） | ✅ |
//! | 6 | 其余一切 | 运算符 / 左定界符 / `;` / EOF | ❌ |
//!
//! 第 3 条官方写的是 bitmask：`s.nlsemi = contains(1<<_Break|1<<_Continue|
//! 1<<_Fallthrough|1<<_Return, tok)` —— **只有这四个**关键字影响分号。
//!
//! # 官方 `nlsemi` 的三种读法（都发生在 `next()` 里，由 `T-L15` 接线）
//!
//! 1. **空白跳过**：`'\n'` 只在 `!nlsemi` 时被吞（`skipWhitespace` 的循环条件）；
//! 2. **`'\n'` 词元**：`case '\n'` 把它译成 `Semi("newline")`；
//! 3. **EOF**：`nlsemi` 为真时补 `Semi("EOF")`。

use crate::token::{Delim, Keyword, OpForm, TokenKind};

/// 分号插入策略 —— 官方 `scanner.nlsemi` 这个跨调用字段的独立化。
///
/// 生命周期（`T-L15` 的 `next()` 负责接线）：
///
/// ```text
/// next():
///   ① nlsemi_old = take()        // 取走上一次的值并复位
///   …（'\n' 与 EOF 的判定用 nlsemi_old）
///   ⑦ set_after(&kind)           // 产出词元后，按类别决定"下一个换行是否译成分号"
/// ```
#[derive(Debug, Default)]
pub struct SemicolonPolicy {
    nlsemi: bool,
}

impl SemicolonPolicy {
    /// 建一个策略（`nlsemi = false` —— 官方 `init()` 的初值）。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 取走当前值并**复位**（官方 `nlsemi := s.nlsemi; s.nlsemi = false`）。
    ///
    /// 复位与取走**必须同一次调用完成** —— 分开成两个方法的话，调用方可能
    /// 忘了复位，而那样下一个换行会在错误的状态下判定（静默多补/少补分号）。
    pub fn take(&mut self) -> bool {
        core::mem::take(&mut self.nlsemi)
    }

    /// 产出词元后按类别设置（官方六处设置点的汇合）。
    ///
    /// # 逐类别穷举，**不许 `_` 通配**
    ///
    /// `TokenKind` 的 7 个变体、`Keyword` 的 25 个、`Delim` 的 10 个、`OpForm`
    /// 的 3 个**全部显式列出**。这不是啰嗦：本仓反复踩过「写死的键列表会变成
    /// 盲区」—— 用 `_ => false` 兜住的话，新增一个枚举成员时**不会编译报错**，
    /// 而"没进规则表"的默认行为（不置位）看起来又完全正常。
    pub fn set_after(&mut self, k: &TokenKind) {
        self.nlsemi = match k {
            TokenKind::Ident(_) => true,
            TokenKind::Literal { .. } => true,
            TokenKind::Keyword(kw) => match kw {
                // 官方 bitmask 的四个成员
                Keyword::Break | Keyword::Continue | Keyword::Fallthrough | Keyword::Return => true,
                // 其余 21 个（显式列出 —— 新增关键字时这里编译报错）
                Keyword::Case
                | Keyword::Chan
                | Keyword::Const
                | Keyword::Default
                | Keyword::Defer
                | Keyword::Else
                | Keyword::For
                | Keyword::Func
                | Keyword::Go
                | Keyword::Goto
                | Keyword::If
                | Keyword::Import
                | Keyword::Interface
                | Keyword::Map
                | Keyword::Package
                | Keyword::Range
                | Keyword::Select
                | Keyword::Struct
                | Keyword::Switch
                | Keyword::Type
                | Keyword::Var => false,
            },
            TokenKind::Op { form, .. } => match form {
                // `++` / `--`（官方 `_IncOp`）
                OpForm::IncDec => true,
                // `+` `+=` `*` `=` `:=` `<-` …（官方 `_Operator`/`_Star`/`_AssignOp`/`_Assign`/`_Define`/`_Arrow`）
                OpForm::Plain | OpForm::Assign => false,
            },
            TokenKind::Delim(d) => match d {
                Delim::RParen | Delim::RBrack | Delim::RBrace => true,
                Delim::LParen
                | Delim::LBrack
                | Delim::LBrace
                | Delim::Comma
                | Delim::Colon
                | Delim::Dot
                | Delim::Ellipsis => false,
            },
            TokenKind::Semi(_) => false,
            TokenKind::Eof => false,
        };
    }
}
