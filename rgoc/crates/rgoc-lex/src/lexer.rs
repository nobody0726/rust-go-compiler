//! 词元层：分派 + 各类词元扫描（`M1-design.md` §5.2）。
//!
//! T-L08 只落 **`ident`（标识符 / 关键字）** 与支撑它的最小 `Lexer` 壳。
//! 分号状态机（T-L14）与 `next()` 分派（T-L15）后续加入。
//!
//! # 这一层最容易被"顺手统一"掉的三处
//!
//! 1. **入口判定的短路** —— 官方是 `isLetter(ch) || ch >= RuneSelf && atIdentChar(true)`，
//!    `&&` 比 `||` 紧 ⇒ **ASCII 数字走不到 `atIdentChar`** ⇒ `1str` 不报错。
//! 2. **一般循环不能加 `>= 0x80` 守卫** —— `atIdentChar(false)` 对 **ASCII 字母/数字**
//!    也是 `true`，所以 `٠abc` 里 `٠` 之后的 `abc` 必须一并吃掉。
//! 3. **第三条分支是"报错但吸收"** —— `atIdentChar` 对非字母非 Nd 的非 ASCII 字符
//!    先 `errorf` 再 `return true`，所以 `x²` 是**一个**标识符（外加一条诊断），
//!    不是"切成两个词元"（F29）。

use crate::diag::{Diag, DiagBag, DiagCode};
use crate::pos::{FileId, FileTable};
use crate::source::SourceReader;
use crate::token::{Keyword, TokenKind};
use crate::unicode_tables::{is_digit, is_letter};

/// 词法器。
///
/// 持有三样东西：字符层（[`SourceReader`]）、文件表、诊断包。
/// **不持有**分号状态（那是 `T-L14` 的 `SemicolonPolicy`）。
#[derive(Debug)]
pub struct Lexer {
    src: SourceReader,
    /// 文件表归 `Lexer` 持有（预检裁决 **P2**）：`SourceReader` 只要 `FileId`，
    /// 表放在这里才让 `Lexer` 能对外解释"这个 `FileId` 是哪个文件"。
    table: FileTable,
    file: FileId,
    diags: DiagBag,
    /// 是否已经读过第一个字符。
    ///
    /// 官方 `scanner.init()` **不**调 `nextch()` —— 第一次推进发生在首次 `next()` 里。
    /// 这里保持同样的**懒**语义：`new()` 不碰游标，第一次真正需要字符时才推进。
    started: bool,
}

impl Lexer {
    /// 建一个词法器。`name` 会登记进文件表（`Pos.file` 只是一张表的索引）。
    ///
    /// **不预读第一个字符** —— 见 [`Lexer::started`] 的说明。
    pub fn new(src: Vec<u8>, name: &str) -> Self {
        let mut table = FileTable::new();
        let file = table.intern(name);
        Self {
            src: SourceReader::new(src, file),
            table,
            file,
            diags: DiagBag::new(),
            started: false,
        }
    }

    /// 文件表 —— 用来把 `Pos.file` 还原成文件名（`Pos` 自己不存名字）。
    #[must_use]
    pub fn table(&self) -> &FileTable {
        &self.table
    }

    /// 本次扫描的文件标识。
    #[must_use]
    pub fn file(&self) -> FileId {
        self.file
    }

    /// 当前**尚未归属**的字符（`None` = EOF）。
    ///
    /// 与 [`SourceReader::ch`] 同一语义：它是"下一个待判定的字符"，
    /// **不属于**已经切出的词元。
    #[must_use]
    pub fn peek(&self) -> Option<char> {
        self.src.ch()
    }

    /// 取走诊断包（取走之后本对象里就是空的）。
    ///
    /// 用"收集 + take"而不是官方那种回调（`M1-design.md` §3.3）：
    /// 回调 + 可变借用会和 `&mut self` 打架。
    pub fn take_diags(&mut self) -> DiagBag {
        core::mem::take(&mut self.diags)
    }

    /// 在当前游标处扫一个**标识符或关键字**。
    ///
    /// - 成功：`Some(kind)`，游标停在标识符之后的第一个字符上
    /// - 失败：`None`，**游标不动**（一个字符都没消耗、也不产诊断）
    ///
    /// # 入口判定：短路顺序不能改
    ///
    /// 官方是 `isLetter(s.ch) || s.ch >= utf8.RuneSelf && atIdentChar(true)`。
    /// `&&` 比 `||` 紧，所以 **ASCII 数字/标点根本走不到 `atIdentChar`**。
    /// 若"顺手统一"成 `is_letter(ch) || is_digit(ch) || at_ident_char(true)`，
    /// `1str` 会凭空多一条 `LEX-ID-LEADING-DIGIT` —— 实测官方**无任何诊断**。
    ///
    /// # 第二个循环为什么**不能**加 `>= 0x80` 守卫
    ///
    /// 官方的一般循环是 `for s.atIdentChar(false) { nextch() }`，而
    /// `atIdentChar(false)` 对 **ASCII 字母/数字同样是 `true`**（分支 1 / 2）。
    /// `٠abc`（U+0660，Nd 起头）就是活例：吸收 `٠` 之后，后面的 ASCII `abc`
    /// **必须一并吃掉**，否则词元会在 `٠` 处被截断。
    ///
    /// 官方那行 `if s.ch >= utf8.RuneSelf` 只是"跳过进入循环"的**优化**：
    /// 当前字符是 ASCII 非字母时 `atIdentChar(false)` 本来就返回 `false`（且不报错）
    /// ⇒ 加不加守卫行为等价。这里**不加**，让循环条件即语义。
    ///
    /// # 分号状态不在这里
    ///
    /// 官方在 `ident()` 末尾顺手设了 `s.nlsemi`。本仓把它提出去做成独立的
    /// `SemicolonPolicy`（`T-L14`），由 `T-L15` 的 `next()` 在拿到 `kind` 之后设置 ——
    /// 这样那套规则可以一张表逐行断言，而不必靠端到端 golden 反推。
    pub fn scan_ident(&mut self) -> Option<TokenKind> {
        self.ensure_started();

        let ch = self.src.ch()?;
        let starts =
            is_ascii_letter_or_underscore(ch) || (ch >= '\u{80}' && self.at_ident_char(true));
        if !starts {
            return None;
        }

        self.src.start_segment();
        self.bump(); // 官方在 `next()` 里先 `nextch()` 再调 `ident()`

        // ① ASCII 快路径（官方 `for isLetter(s.ch) || isDecimal(s.ch)`）
        while let Some(c) = self.src.ch() {
            if is_ascii_letter_or_underscore(c) || is_ascii_digit(c) {
                self.bump();
            } else {
                break;
            }
        }

        // ② 一般路径（见上：**不加** `>= 0x80` 守卫）
        while self.at_ident_char(false) {
            self.bump();
        }

        let lit = self.src.segment().to_vec();
        self.src.stop_segment();
        Some(keyword_or_ident(lit))
    }

    /// 首次调用时读入第一个字符（幂等）。
    fn ensure_started(&mut self) {
        if !self.started {
            self.started = true;
            self.bump();
        }
    }

    /// 消费当前字符并前进；把字符层报出的诊断收进自己的包。
    fn bump(&mut self) {
        for d in self.src.advance() {
            self.diags.push(d);
        }
    }

    /// 在**当前字符**的位置报一条诊断（对应官方 `s.error` 报在 `s.pos()`）。
    fn report(&mut self, code: DiagCode, message: String) {
        self.diags.push(Diag {
            pos: self.src.pos(),
            code,
            message,
        });
    }

    /// 官方 `scanner.atIdentChar` 的逐条翻译。**分支顺序不能动。**
    ///
    /// | # | 条件 | 报错 | 返回 |
    /// |---|---|---|---|
    /// | 1 | 字母（`L*`）或 `_` | — | `true` |
    /// | 2 | `Nd` 数字 | 仅当 `first` ⇒ `LEX-ID-LEADING-DIGIT` | `true` |
    /// | 3 | **其他** `>= 0x80` 的字符 | 总是 ⇒ `LEX-BAD-IDENT-CHAR` | **`true`** ← **吸收** |
    /// | 4 | 其余（ASCII 非字母非数字） | — | `false` |
    ///
    /// **第 3 条的"吸收"是 F29 的机制**：`x²` / `x€` / `x\u{0345}` 都被吃进同一个
    /// 标识符，只是附带一条诊断。这与"词元边界会变"的直觉相反 —— 实测才看得出来。
    fn at_ident_char(&mut self, first: bool) -> bool {
        let Some(ch) = self.src.ch() else {
            return false;
        };
        if is_letter(ch) {
            return true;
        }
        if is_digit(ch) {
            if first {
                self.report(
                    DiagCode::IdLeadingDigit,
                    format!("标识符不能以数字开头 {ch:?}"),
                );
            }
            return true;
        }
        if ch >= '\u{80}' {
            self.report(
                DiagCode::BadIdentChar,
                format!("标识符里出现非法字符 {ch:?}"),
            );
            return true;
        }
        false
    }
}

/// 官方 `isLetter`：**只认 ASCII**。
///
/// 官方把它写成 `'a' <= ('a'-'A')|ch && ('a'-'A')|ch <= 'z' || ch == '_'`；
/// `0x20 | ch` 在 `ch >= 0x80` 时必然仍 `>= 0x80` ⇒ 落不进 `a..=z`
/// ⇒ 这个谓词对非 ASCII 恒为假（非 ASCII 一律走 [`Lexer::at_ident_char`]）。
///
/// 用 `is_ascii_alphabetic` 而不是手写 `matches!(c, 'a'..='z' | 'A'..='Z')`：
/// 后者会触发 `clippy::manual_is_ascii_check`（`-D warnings` 下打爆门禁），
/// 而且 ASCII 版本**没有歧义** —— 与 D-M1-6 禁掉的是 std 的 **Unicode** 谓词
/// （`char::is_alphabetic` 等）不冲突。
///
/// ⚠️ 因此 §12 **R5** 那条收尾检查（"全仓 grep `is_alphabetic` 确认零使用"）
/// 必须用**精确**模式 `.is_alphabetic(` —— 朴素的子串 `is_alphabetic` 会命中
/// `is_ascii_alphabetic`，那是假阳性。
fn is_ascii_letter(c: char) -> bool {
    c.is_ascii_alphabetic()
}

fn is_ascii_digit(c: char) -> bool {
    c.is_ascii_digit()
}

fn is_ascii_letter_or_underscore(c: char) -> bool {
    is_ascii_letter(c) || c == '_'
}

/// **整串切完**再查关键字（LEX-05）；查不到就是普通标识符。
///
/// 边扫边匹配会让 `forx` 变成 `for` + `x` —— `forx` / `format` 两条测试专门盯它。
///
/// 关键字全是 ASCII ⇒ 含非法 UTF-8 的标识符在 `from_utf8` 那一步就失败 ⇒ 直接归
/// `Ident`。**不用 `from_utf8_lossy`**（D-M1-12：那会把非法字节静默换成 U+FFFD）。
fn keyword_or_ident(lit: Vec<u8>) -> TokenKind {
    if let Some(k) = core::str::from_utf8(&lit).ok().and_then(Keyword::lookup) {
        return TokenKind::Keyword(k);
    }
    TokenKind::Ident(lit)
}
