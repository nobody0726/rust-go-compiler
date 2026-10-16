//! 词元层：分派 + 各类词元扫描（`M1-design.md` §5.2）。
//!
//! 已落地：`ident`（T-L08）、`punct`（T-L09）、`number`（T-L10）、`rune`/`escape`（T-L11）、
//! `string`（T-L12）、注释与两种 mode 的上报通道（T-L13）。
//! 后续加入：分号状态机（T-L14）与 `next()` 分派（T-L15）。
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
use crate::message::{Message, MessageKind, Mode};
use crate::pos::{FileId, FileTable, Pos};
use crate::semicolon::SemicolonPolicy;
use crate::source::SourceReader;
use crate::token::{Delim, Keyword, LitKind, OpForm, Operator, SemiOrigin, Token, TokenKind};
use crate::unicode_tables::{is_digit, is_letter};

use std::fs;
use std::io;
use std::path::Path;

/// 单文件字节上限：**64 MiB**（`D-M1-4` / `M1-design.md` §7.3）。
///
/// 这是**本仓的实现限制**，不是 Go 的语义 —— 官方 scanner 没有任何长度上限
/// 常量（F4），所以超限行为（`T-L-63`，判据 `H`）没有 oracle 可对，
/// golden 是**手写**的（`# source: hand-written`，全仓唯一一条 `H`）。
///
/// # 为什么「只设一个」
///
/// 词元是文件的**连续子串** ⇒ 词元长度不可能超过文件大小 ⇒ 限住文件，
/// 词元自动有界。这省掉了「单个词元超限后游标往哪走」那套没有标准答案的
/// 恢复逻辑（少一个上限 = 少一套恢复逻辑 = 少一组测试）。
///
/// 取 64 MiB 的依据：语料最大文件 3.77 MB（F14）的 **17 倍**余量。
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// 词法器。
///
/// 持有五样东西：字符层（[`SourceReader`]）、文件表、诊断包、注释上报（按 mode）、
/// 分号状态（[`SemicolonPolicy`]）。
#[derive(Debug)]
pub struct Lexer {
    src: SourceReader,
    /// 文件表归 `Lexer` 持有（预检裁决 **P2**）：`SourceReader` 只要 `FileId`，
    /// 表放在这里才让 `Lexer` 能对外解释"这个 `FileId` 是哪个文件"。
    table: FileTable,
    file: FileId,
    diags: DiagBag,
    /// 注释/指令的上报模式（默认 [`Mode::NONE`]）。官方在 `init()` 时设定、
    /// 之后不变；这里后设 —— 不改 [`Lexer::new`] 的既有签名（`T-L15` 组装
    /// `next()` 时再定最终构造形态）。
    mode: Mode,
    /// 上报的注释与指令。**与 `diags` 独立**（D-M1-8）—— 注释不是诊断，
    /// `C1` §4 的排序规则不该施加到它上面。
    messages: Vec<Message>,
    /// 分号插入状态（LEX-07 的跨调用字段 —— 官方 `scanner.nlsemi`）。
    semi: SemicolonPolicy,
    /// 是否已经读过第一个字符。
    ///
    /// 官方 `scanner.init()` **不**调 `nextch()` —— 第一次推进发生在首次 `next()` 里。
    /// 这里保持同样的**懒**语义：`new()` 不碰游标，第一次真正需要字符时才推进。
    started: bool,
}

impl Lexer {
    /// 建一个词法器。`name` 会登记进**本词法器自己的**文件表
    /// （`Pos.file` 只是一张表的索引）。
    ///
    /// **不预读第一个字符** —— 见 [`Lexer::started`] 的说明。
    ///
    /// ⚠️ **多文件必须用 [`Lexer::new_in`]**：本入口每次新建一张表 ⇒ 每个文件的
    /// `FileId` 都是 `0`，跨文件的诊断排序与定位都会失准（Task 23 Ruling 23-4）。
    pub fn new(src: Vec<u8>, name: &str) -> Self {
        let mut table = FileTable::new();
        let lx = Self::new_in(&mut table, src, name);
        // `new_in` 里的快照就是这张只有本文件的表 —— 两者一致，不必再拷一次
        lx
    }

    /// 用**调用方持有的**文件表登记本文件 —— **多文件装载的唯一正确形态**。
    ///
    /// 与 [`Lexer::new`] 的唯一区别是 `FileId` 的来源：这里登记进共享表，
    /// 所以两个文件各自得到不同的 `FileId` —— `C1` §4 的诊断排序第一键
    /// （`pos.file`）才有意义。
    ///
    /// 本词法器内部保存该表的一份**快照**（含本次之前登记的名字），
    /// 供 [`Lexer::table`] 解释本文件及更早文件的位置：
    /// `Lexer` 的几个错误路径要用它渲染位置，拿不到表就只能 panic。
    /// 快照**不含**此后登记的文件 —— 那时位置也不该由本词法器解释。
    pub fn new_in(table: &mut FileTable, src: Vec<u8>, name: &str) -> Self {
        let file = table.intern(name);
        let snapshot = table.clone();
        Self {
            src: SourceReader::new(src, file),
            table: snapshot,
            file,
            diags: DiagBag::new(),
            mode: Mode::NONE,
            messages: Vec::new(),
            semi: SemicolonPolicy::new(),
            started: false,
        }
    }

    /// 从文件构造：**先 `fs::metadata` 检查大小，再读入**（`T-L17`）。
    ///
    /// # 检查为什么必须在读之前（§7.3）
    ///
    /// `fs::read` 一个 10 GB 的文件**在那一步就 OOM** ——「读完再检查」等于没检查。
    /// `metadata` 不读内容、也不看读权限，是唯一正确的探路手段。
    ///
    /// # 超限时返回 `Ok`（不是 `Err`）
    ///
    /// 超限是**词法层的拒绝**：产出一条 `LEX-EXCEEDS-FILE-LIMIT` 诊断 +
    /// 只含 `Eof` 的词元流（`T-L-63`，判据 `H`）—— 与「报错但继续」的全局恢复
    /// 纪律一致（**不 panic、不静默截断**）。诊断位置取**文件起点**（1:1）：
    /// 超限判定只看大小、不看内容，起点是唯一有意义的坐标。
    ///
    /// 而 io 失败（不存在 / 不可读）**如实返回 `Err`** —— 把它映射成超限诊断
    /// 会让「文件读不到」与「文件太大」不可区分。
    pub fn from_path(path: impl AsRef<Path>, mode: Mode) -> io::Result<Self> {
        let path = path.as_ref();
        let name = path.to_string_lossy();

        let len = fs::metadata(path)?.len();
        if len > MAX_FILE_BYTES {
            // 「已耗尽」实例：空源 ⇒ 只产 Eof@1:1（收尾也不补分号：nlsemi 初始为假）。
            let mut lx = Self::new(Vec::new(), &name);
            lx.set_mode(mode);
            let pos = Pos {
                base: None, // 词法层不挂 `//line` 基准（见 `source.rs::pos`）
                file: lx.file,
                line: 1,
                col: 1,
            };
            lx.push_diag(Diag {
                pos,
                code: DiagCode::ExceedsFileLimit,
                message: format!("文件超过单文件上限：{len} 字节 > {MAX_FILE_BYTES} 字节"),
            });
            return Ok(lx);
        }

        // 大小在限内 ⇒ 此刻读入是安全的（最坏驻留 ≈ 64 MiB）。
        let mut lx = Self::new(fs::read(path)?, &name);
        lx.set_mode(mode);
        Ok(lx)
    }

    /// 设置注释/指令的上报模式（D-M1-1）。默认 [`Mode::NONE`]（一律不上报）。
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
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

    /// 当前游标的位置（即 [`Lexer::peek`] 那个字符的 `Pos`）。
    ///
    /// 在扫描**之前**取它，得到的就是下一个词元的起点 —— `next()`（T-L15）
    /// 正是这么用它来填 [`crate::Token::pos`] 的。
    #[must_use]
    pub fn pos(&self) -> Pos {
        self.src.pos()
    }

    /// 取走诊断包（取走之后本对象里就是空的）。
    ///
    /// 用"收集 + take"而不是官方那种回调（`M1-design.md` §3.3）：
    /// 回调 + 可变借用会和 `&mut self` 打架。
    pub fn take_diags(&mut self) -> DiagBag {
        core::mem::take(&mut self.diags)
    }

    /// 取走上报的注释/指令，**按扫描顺序**（取走之后本对象里就是空的）。
    ///
    /// 与 [`Lexer::take_diags`] 同一手法（收集 + take）。不排序：
    /// 扫描顺序本身就是确定的，而渲染时的排序（`line → col → text`）
    /// 是 T-L18/T-L19 渲染层的事。
    pub fn take_messages(&mut self) -> Vec<Message> {
        core::mem::take(&mut self.messages)
    }

    /// 前进到下一个词元（`M1-design.md` §5.2 的骨架；官方 `scanner.next()` 的逐条翻译）。
    ///
    /// # 生命周期（与其它部分的分工）
    ///
    /// ```text
    /// ① take()           取走上一次的分号状态并复位（T-L14）
    /// ② loop             「重扫」回边 —— 注释、非法字符走这里（官方 `goto redo`）
    ///     ③ skip_whitespace(nlsemi)   '\n' 只在 !nlsemi 时被吞（LEX-07 第一处机关）
    ///     ④ 记录词元起点 start
    ///     ⑤ EOF 判定（nlsemi 为真时补 Semi("EOF")）
    ///     ⑥ 首字符分派：ident → 数字 → 字符串三型 → 标点（含注释/非法字符）
    /// ⑦ set_after(&kind) 按类别决定"下一个换行是否译成分号"（T-L14）
    /// ```
    ///
    /// # 三处位置语义（全部由 oracle 钉住，见 `tests/test_next.rs`）
    ///
    /// - `Semi("newline")`（换行来源）：位置 = **换行符**（下一轮循环的起点）；
    /// - `Semi("newline")`（跨行块注释来源）：位置 = **注释起点**（`scan_comment`
    ///   的返回值语义 —— 官方 `case '*'` 读的 `s.line/s.col` 未被更新）；
    /// - `Semi("EOF")` 与 `Eof`：同位于 **EOF 处**（不凭空造列号，RF5）。
    ///
    /// # 为什么保留 `next` 这个名字（而不实现 `Iterator`）
    ///
    /// 官方接口就叫 `scanner.next()`，这是 M1 最核心的一处对应关系。clippy 的
    /// `should_implement_trait` 会问「为什么不实现 `Iterator`」——**因为本流不满足
    /// `Iterator` 的终止契约**：
    ///
    /// - 流里**没有** `None`：`Eof` 是一个**词元**（实测：流里恰好一个、在最后），
    ///   而且 `Eof` 之后继续调用仍返回 `Eof`（不 panic、不终止）；
    /// - 若硬实现 `Iterator`，要么改变流的形状（把 `Eof` 词元吞掉换成 `None`），
    ///   要么违约（`for` 循环死循环）——两者都比"名字撞车"更糟。
    ///
    /// 与 R3 的 `from_str` 裁决**刻意不同**：那里的撞名会导致**运行时**误导
    /// （`parse()` 静默不可用）；这里的撞名只导致**编译期**错误（Iterator 方法用不了）
    /// —— 显性失败，不值得拿"官方名"去换。
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Token {
        // 懒读首字符（官方 `init()` 不碰游标，第一次 `next()` 才推进 —— 见 `started`）
        self.ensure_started();
        let nlsemi = self.semi.take();

        let (start, kind) = loop {
            self.skip_whitespace(nlsemi);

            let start = self.pos();
            let Some(ch) = self.src.ch() else {
                // EOF：nlsemi 为真时补分号（官方 `case -1`）
                let kind = if nlsemi {
                    TokenKind::Semi(SemiOrigin::Eof)
                } else {
                    TokenKind::Eof
                };
                break (start, kind);
            };

            // ① 标识符 / 关键字。入口判定与扫描在 `scan_ident` 内部**一次完成** ——
            //    若在这里先探一次、函数里再判一次，`x²` 那类输入会报**两条**
            //    LEX-BAD-IDENT-CHAR（`at_ident_char` 有副作用）。
            if let Some(kind) = self.scan_ident() {
                break (start, kind);
            }

            // ② 其余首字符分派（顺序照官方 `next()` 的 switch）
            match ch {
                '\n' => {
                    // 只有 nlsemi 为真时 `\n` 才活到这儿（否则被 skip_whitespace 吞掉）
                    self.bump();
                    break (start, TokenKind::Semi(SemiOrigin::Newline));
                }
                '0'..='9' => break (start, self.scan_number(false)),
                '"' => break (start, self.scan_std_string()),
                '`' => break (start, self.scan_raw_string()),
                '\'' => break (start, self.scan_rune()),
                _ => match self.scan_punct() {
                    Some(PunctScan::Token(kind)) => break (start, kind),
                    Some(PunctScan::LeadingDotNumber) => break (start, self.scan_number(true)),
                    Some(PunctScan::Comment(style)) => {
                        match self.scan_comment(start, style, nlsemi) {
                            // 注释处理完 ⇒ 重扫（官方 `goto redo`）
                            CommentScan::Redo => continue,
                            // 跨行块注释 + nlsemi ⇒ 等价于换行（位置 = 注释起点）
                            CommentScan::Semi => {
                                break (start, TokenKind::Semi(SemiOrigin::Newline));
                            }
                        }
                    }
                    None => {
                        // 官方 `default`：报"invalid character"、消耗、重扫。
                        // （非 ASCII 到不了这里 —— 它们全部被 ① 吸收）
                        self.report(DiagCode::BadChar, format!("不认识的字符 {ch:?}"));
                        self.bump();
                        continue;
                    }
                },
            }
        };

        self.semi.set_after(&kind);
        Token { pos: start, kind }
    }

    /// 官方 `next()` 开头的空白循环（`scanner.go:96-98`）：
    ///
    /// ```text
    /// for s.ch == ' ' || s.ch == '\t' || s.ch == '\n' && !nlsemi || s.ch == '\r'
    /// ```
    ///
    /// `&&` 比 `||` 紧 ⇒ **`'\n'` 只在 `nlsemi` 为假时被吞**（LEX-07 的第一处机关）。
    /// `\r` 与空格/制表符一样无条件跳过。
    fn skip_whitespace(&mut self, nlsemi: bool) {
        while let Some(c) = self.src.ch() {
            let skip = c == ' ' || c == '\t' || c == '\r' || (c == '\n' && !nlsemi);
            if !skip {
                break;
            }
            self.bump();
        }
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

    /// 在当前游标处扫一个**数字字面量**（LEX-09 / LEX-10 / LEX-11）。
    ///
    /// **入口前置条件**：游标在字面量的第一个字符上 ——
    /// - `seen_point == false`：那个字符是十进制数字（`0`-`9`）
    /// - `seen_point == true`：那个字符是 `.`，且**后面跟数字**
    ///   （由 [`PunctScan::LeadingDotNumber`] 交接，见 [`Lexer::scan_punct`]）
    ///
    /// 官方在调 `number(true)` **之前**已经把 `.` 消耗掉了；本仓把交接点定在
    /// "`.` 未被消耗"，所以这里补上 [`Lexer::bump`]。两种入口的**字面量文本都从
    /// 第一个字符起算** —— 实测 `.5` 的 `lit` 就是 `".5"`。
    ///
    /// # 三处必须照抄官方的细节
    ///
    /// 1. **`digsep` 位集**：bit0 = 吃到过数码、bit1 = 吃到过 `_`。扫描时**只记位集**，
    ///    `_` 的位置是否非法交给切完之后的 [`invalid_sep`] 再扫一遍 —— 边扫边判会
    ///    写出另一套规则，两套在 `1__2` / `0b__1000` 这类输入上**不等价**。
    /// 2. **指数分支里 `digsep` 是 `=` 不是 `|=`**（官方 `digsep = s.digits(10, nil) | digsep&2`）：
    ///    尾数的"有数字"位被**丢弃**，只保留"有 `_`"位。写成 `|=` 会让 `1.5e_`
    ///    **少报**一条 `LEX-BAD-EXPONENT`、**多报**一条 `LEX-BAD-SEP`。
    /// 3. **`invalid`（进制里第一个越界数字的偏移）只在 `base <= 10` 记**：
    ///    `base > 10` 时 [`Lexer::digits`] 根本不看"数字是否越界"（十六进制里没有
    ///    越界数字，非十六进制字符直接停下）⇒ `0x1g` 是 "`0x1` + 标识符 `g`"、
    ///    **零诊断**（实测）。
    ///
    /// 畸形输入**照常产出**词元（`bad = true`）—— 恢复纪律见 `M1-design.md` §7.1。
    pub fn scan_number(&mut self, seen_point: bool) -> TokenKind {
        self.ensure_started();
        let start = self.pos();
        self.src.start_segment();

        let mut ok = true;
        let mut kind = LitKind::Int;
        let mut base: u8 = 10;
        let mut prefix = NumPrefix::None;
        let mut digsep: u8 = 0;
        let mut invalid: Option<u32> = None;
        let mut seen_point = seen_point;

        if seen_point {
            self.bump(); // 交接时未消耗的那个 `.`（官方在调用前已消耗）
        }

        // ── 整数部分
        if !seen_point {
            if self.src.ch() == Some('0') {
                self.bump();
                match self.src.ch() {
                    Some('x' | 'X') => {
                        self.bump();
                        base = 16;
                        prefix = NumPrefix::X;
                    }
                    Some('o' | 'O') => {
                        self.bump();
                        base = 8;
                        prefix = NumPrefix::O;
                    }
                    Some('b' | 'B') => {
                        self.bump();
                        base = 2;
                        prefix = NumPrefix::B;
                    }
                    // 前导 `0` **自己算一个数码**（所以 `0_1` 合法，而 `0x` 仍报缺数字）
                    _ => {
                        base = 8;
                        prefix = NumPrefix::Zero;
                        digsep = 1;
                    }
                }
            }
            digsep |= self.digits(base, &mut invalid, start);
            if self.src.ch() == Some('.') {
                if matches!(prefix, NumPrefix::O | NumPrefix::B) {
                    self.report(
                        DiagCode::BadRadixPoint,
                        format!("{}字面量里不能有小数点", base_name(base)),
                    );
                    ok = false;
                }
                self.bump();
                seen_point = true;
            }
        }

        // ── 小数部分
        if seen_point {
            kind = LitKind::Float;
            digsep |= self.digits(base, &mut invalid, start);
        }

        if digsep & 1 == 0 && ok {
            self.report(
                DiagCode::LitNoDigits,
                format!("{}字面量里没有数字", base_name(base)),
            );
            ok = false;
        }

        // ── 指数
        let is_p = matches!(self.src.ch(), Some('p' | 'P'));
        if matches!(self.src.ch(), Some('e' | 'E' | 'p' | 'P')) {
            if ok {
                if !is_p && !matches!(prefix, NumPrefix::None | NumPrefix::Zero) {
                    self.report(
                        DiagCode::BadExponent,
                        "'e'/'E' 指数要求十进制尾数".to_string(),
                    );
                    ok = false;
                } else if is_p && prefix != NumPrefix::X {
                    self.report(
                        DiagCode::BadExponent,
                        "'p'/'P' 指数要求十六进制尾数".to_string(),
                    );
                    ok = false;
                }
            }
            self.bump();
            kind = LitKind::Float;
            if matches!(self.src.ch(), Some('+' | '-')) {
                self.bump();
            }
            // 官方给这里传 `nil`（不记 invalid）。传一个 scratch 与之等价：
            // `base == 10` 时 `ch >= max` 恒假（`'9' < ':'`），永远写不进去。
            let mut scratch = None;
            digsep = self.digits(10, &mut scratch, start) | digsep & 2;
            if digsep & 1 == 0 && ok {
                self.report(DiagCode::BadExponent, "指数里没有数字".to_string());
                ok = false;
            }
        } else if prefix == NumPrefix::X && kind == LitKind::Float && ok {
            self.report(
                DiagCode::BadExponent,
                "十六进制尾数要求 'p' 指数".to_string(),
            );
            ok = false;
        }

        // ── 虚数后缀
        if self.src.ch() == Some('i') {
            kind = LitKind::Imag;
            self.bump();
        }

        let lit = self.src.segment().to_vec();
        self.src.stop_segment();

        // ── 事后校验（官方在 `setLit` 之后做的两段，都靠 `ok` 守卫）
        if kind == LitKind::Int
            && ok
            && let Some(off) = invalid
        {
            let c = char::from(lit[off as usize]);
            self.report_at(
                start,
                off,
                DiagCode::BadDigit,
                format!("{}字面量里的非法数字 {c:?}", base_name(base)),
            );
            ok = false;
        }
        if digsep & 2 != 0
            && ok
            && let Some(i) = invalid_sep(&lit)
        {
            self.report_at(
                start,
                i as u32,
                DiagCode::BadSep,
                "'_' 必须分隔连续的数码".to_string(),
            );
            ok = false;
        }

        TokenKind::Literal {
            text: lit,
            lit_kind: kind,
            bad: !ok,
        }
    }

    /// 官方 `scanner.digits(base, invalid)` 的逐条翻译：吃掉 `{ 数码 | '_' }`。
    ///
    /// 返回 **`digsep` 位集**：bit0 = 吃到过数码、bit1 = 吃到过 `_`。
    /// `base <= 10` 时**还**记录第一个越界数字相对**词元起点**的字节偏移；
    /// `base > 10` 时不记 —— 十六进制里没有"越界数字"这回事，
    /// 不是十六进制字符就直接停下（所以 `0x1g` 的 `g` 原样留给下一个词元）。
    fn digits(&mut self, base: u8, invalid: &mut Option<u32>, start: Pos) -> u8 {
        let mut digsep = 0u8;
        if base <= 10 {
            let max = b'0' + base;
            while let Some(ch) = self.src.ch() {
                let ds = if ch == '_' {
                    2
                } else if ch.is_ascii_digit() {
                    if ch as u8 >= max && invalid.is_none() {
                        // 相对词元起点的**字节**偏移（官方 `col - s.col`）。
                        // 这条循环只吃单字节 ASCII ⇒ 列差 == 字节差。
                        *invalid = Some(self.pos().col - start.col);
                    }
                    1
                } else {
                    break;
                };
                digsep |= ds;
                self.bump();
            }
        } else {
            while let Some(ch) = self.src.ch() {
                let ds = if ch == '_' {
                    2
                } else if ch.is_ascii_hexdigit() {
                    1
                } else {
                    break;
                };
                digsep |= ds;
                self.bump();
            }
        }
        digsep
    }

    /// 在当前游标处扫一个 **rune 字面量**（LEX-12）。
    ///
    /// **入口前置条件**：游标在**开引号**上 —— 官方 `next()` 的 `case '\''` **直接**调
    /// `rune()`（不带 `nextch`），函数自己吃掉它。所以 lit 从开引号起算。
    ///
    /// # 字符计数 `n` 的三条判据（官方顺序）
    ///
    /// - `n == 0` ⇒ `LEX-RUNE-EMPTY`（`errorf` ⇒ 报在**闭引号**）
    /// - `n != 1` ⇒ `LEX-RUNE-MULTI`（`errorAtf(0)` ⇒ 报在**开引号**！）
    /// - **转义算一个字符**（官方 `continue` 会走 `n++` ⇒ `'\n\n'` 是两个字符）
    ///
    /// 换行与 EOF 都是 `break` **不消耗**当前字符：换行留给分号层（LEX-07），
    /// EOF 之后还有"补分号"一步。
    pub fn scan_rune(&mut self) -> TokenKind {
        self.ensure_started();
        let start = self.pos();
        self.src.start_segment();
        let mut ok = true;
        self.bump(); // 吃掉开引号

        let mut n = 0usize;
        loop {
            match self.src.ch() {
                Some('\'') => {
                    if ok {
                        if n == 0 {
                            self.report(
                                DiagCode::RuneEmpty,
                                "空的 rune 字面量（或未转义的 '）".to_string(),
                            );
                            ok = false;
                        } else if n != 1 {
                            self.report_at(
                                start,
                                0,
                                DiagCode::RuneMulti,
                                "rune 字面量里多于一个字符".to_string(),
                            );
                            ok = false;
                        }
                    }
                    self.bump();
                    break;
                }
                Some('\\') => {
                    self.bump();
                    if !self.escape('\'') {
                        ok = false;
                    }
                    n += 1; // 转义算**一个**字符
                }
                Some('\n') => {
                    if ok {
                        self.report(DiagCode::RuneNewline, "rune 字面量里出现换行".to_string());
                        ok = false;
                    }
                    break;
                }
                None => {
                    if ok {
                        self.report_at(
                            start,
                            0,
                            DiagCode::RuneUnterminated,
                            "rune 字面量未终止".to_string(),
                        );
                        ok = false;
                    }
                    break;
                }
                Some(_) => {
                    self.bump();
                    n += 1;
                }
            }
        }

        let lit = self.src.segment().to_vec();
        self.src.stop_segment();
        TokenKind::Literal {
            text: lit,
            lit_kind: LitKind::Rune,
            bad: !ok,
        }
    }

    /// 官方 `scanner.escape(quote)` 的逐条翻译：吃掉**一个**（`\` 之后的）转义序列。
    ///
    /// **入口前置条件**：游标在 `\` **之后的那个字符**上（官方调用前已经 `nextch()` 掉 `\`）。
    /// 返回 `false` = 已报错（由调用方把 `ok` 置假）；返回 `true` 也可能是**撞上 EOF** ——
    /// 那种情况**故意不在这里报错**，由调用方报"未终止"（官方注释
    /// 「complain in caller about EOF」）。
    ///
    /// # 表驱动
    ///
    /// | 形式 | `(n, base, max)` | 备注 |
    /// |---|---|---|
    /// | `quote` / `a b f n r t v` / `\\` | —— | 直接 `bump` 返回；**`quote` 是白名单的一部分** |
    /// | `\ooo` | `(3, 8, 255)` | **不先 `bump`**：进循环时当前字符就是第一个八进制数码 |
    /// | `\x` / `\u` / `\U` | `(2,16,255)` / `(4,16,MAX_RUNE)` / `(8,16,MAX_RUNE)` | 先 `bump` 掉 `x`/`u`/`U` |
    ///
    /// # 三条诊断各有各的时机
    ///
    /// ① `LEX-ESCAPE-UNKNOWN`：`\` 后面那个字符不在任何一类里；
    /// ② `LEX-ESCAPE-BAD-DIGIT`：定长循环里遇到非该进制的数码（报在**那个字符**上，
    ///    缺位时报在闭引号上）；③ `LEX-ESCAPE-OCTAL-OVER` / `LEX-ESCAPE-RANGE`：
    /// 循环**之后**统一查 —— 八进制先查 `> 255`（它的 `max` 只有 255），再统一查
    /// `> max || 代理区`。
    fn escape(&mut self, quote: char) -> bool {
        let Some(ch) = self.src.ch() else {
            return true; // EOF：由调用方报"未终止"
        };

        if ch == quote || matches!(ch, 'a' | 'b' | 'f' | 'n' | 'r' | 't' | 'v' | '\\') {
            self.bump();
            return true;
        }

        let (n, base, max): (u32, u32, u32) = match ch {
            '0'..='7' => (3, 8, 255),
            'x' => {
                self.bump();
                (2, 16, 255)
            }
            'u' => {
                self.bump();
                (4, 16, MAX_RUNE)
            }
            'U' => {
                self.bump();
                (8, 16, MAX_RUNE)
            }
            _ => {
                self.report(
                    DiagCode::EscapeUnknown,
                    "反斜杠后面是不认识的转义".to_string(),
                );
                return false;
            }
        };

        let mut x: u32 = 0;
        for _ in 0..n {
            let Some(c) = self.src.ch() else {
                return true; // EOF：由调用方报"未终止"
            };
            let lower = c.to_ascii_lowercase();
            // 哨兵值 `base` 恰好保证 `d >= base` 成立（官方 `d := base` 的同一手法）
            let d = if c.is_ascii_digit() {
                c as u32 - u32::from(b'0')
            } else if matches!(lower, 'a'..='f') {
                lower as u32 - u32::from(b'a') + 10
            } else {
                base
            };
            if d >= base {
                self.report(
                    DiagCode::EscapeBadDigit,
                    format!("{}转义里出现非法字符 {c:?}", base_name(base as u8)),
                );
                return false;
            }
            x = x * base + d;
            self.bump();
        }

        if x > max && base == 8 {
            self.report(DiagCode::EscapeOctalOver, format!("八进制转义值 {x} > 255"));
            return false;
        }
        if x > max || (0xD800..0xE000).contains(&x) {
            self.report(
                DiagCode::EscapeRange,
                format!("转义不是合法的 Unicode 码点 U+{x:04X}"),
            );
            return false;
        }
        true
    }

    /// 在当前游标处扫一个**解释型字符串字面量**（LEX-13 的双引号形式）。
    ///
    /// **入口前置条件**：游标在**开引号** `"` 上（官方 `case '"'` **直接**调
    /// `stdString()`，函数自己 `nextch()` 掉它）。
    ///
    /// # 与 [`Lexer::scan_rune`] 的一处**刻意的不一致**（别"顺手统一"）
    ///
    /// 官方 `stdString()` 的换行/EOF 分支**没有 `if ok` 守卫**，而 `rune()` **有** ——
    /// 所以 `"\q` + 换行会报**两条**诊断（`LEX-ESCAPE-UNKNOWN` + `LEX-STRING-NEWLINE`），
    /// 而 `'\q` + 换行只报一条。这是官方的**实际行为**（实测），照抄。
    /// **"少一条诊断"比"词元切错"更难发现** —— 词元数会变，诊断数不会。
    pub fn scan_std_string(&mut self) -> TokenKind {
        self.ensure_started();
        let start = self.pos();
        self.src.start_segment();
        let mut ok = true;
        self.bump(); // 吃掉开引号

        loop {
            match self.src.ch() {
                Some('"') => {
                    self.bump();
                    break;
                }
                Some('\\') => {
                    self.bump();
                    if !self.escape('"') {
                        ok = false;
                    }
                }
                Some('\n') => {
                    // ⚠️ 没有 `if ok` 守卫（与 `scan_rune` 不同 —— 见上）
                    self.report(DiagCode::StringNewline, "字符串里出现换行".to_string());
                    ok = false;
                    break;
                }
                None => {
                    self.report_at(
                        start,
                        0,
                        DiagCode::StringUnterminated,
                        "字符串未终止".to_string(),
                    );
                    ok = false;
                    break;
                }
                Some(_) => self.bump(),
            }
        }

        let lit = self.src.segment().to_vec();
        self.src.stop_segment();
        TokenKind::Literal {
            text: lit,
            lit_kind: LitKind::String,
            bad: !ok,
        }
    }

    /// 在当前游标处扫一个**原始字符串字面量**（LEX-13 的反引号形式）。
    ///
    /// **入口前置条件**：游标在**开反引号**上（官方 `next()` 的反引号分支**直接**调
    /// `rawString()`，函数自己 `nextch()` 掉它）。
    ///
    /// # 三条与解释型的差别
    ///
    /// 1. **不处理转义** —— `\` 只是普通字符；
    /// 2. **可跨行** —— 换行不是错误（也**没有**换行诊断）；行号照常推进；
    /// 3. **`\r` 留在文本里**（F39）—— 官方注释原文：
    ///    「We leave CRs in the string since they are part of the literal (even though
    ///    they are not part of the literal value)」。⇒ "丢弃"是**值**层面的事（M3 的
    ///    unquote），M1 只管文本，一个字节都不动。
    pub fn scan_raw_string(&mut self) -> TokenKind {
        self.ensure_started();
        let start = self.pos();
        self.src.start_segment();
        let mut ok = true;
        self.bump(); // 吃掉开反引号

        loop {
            match self.src.ch() {
                Some('`') => {
                    self.bump();
                    break;
                }
                None => {
                    self.report_at(
                        start,
                        0,
                        DiagCode::StringUnterminated,
                        "字符串未终止".to_string(),
                    );
                    ok = false;
                    break;
                }
                Some(_) => self.bump(),
            }
        }

        let lit = self.src.segment().to_vec();
        self.src.stop_segment();
        TokenKind::Literal {
            text: lit,
            lit_kind: LitKind::String,
            bad: !ok,
        }
    }

    /// 在当前游标处扫一个**运算符或标点**（LEX-04 最长匹配）。
    ///
    /// 返回值有三种去向，见 [`PunctScan`]。当前字符不是标点的起始字符时返回 `None`
    /// 且**一个字符都不消耗**。
    ///
    /// # 三条要从官方照抄的地方
    ///
    /// 1. **最长匹配用手写前瞻**，不是"扫到最长再回退"。
    ///    唯一需要回退的是 `.` 分支（`..` 与 `.`+数字两种情形），
    ///    全阶段**只有这一处**会调 [`SourceReader::rewind`]。
    /// 2. **`+=` 一族收敛到同一段逻辑**（[`Lexer::assign_op`]，官方 `assignop` 标签）。
    ///    11 个复合赋值运算符共用它 —— 逐个分支手写必然漏一两个。
    /// 3. **`*` 不需要单独照搬官方的 `_Star`**：`(Plain, Mul)` 已经唯一（D-M1-10）。
    ///
    /// # 官方在这里顺手设的 `nlsemi` 不在这里
    ///
    /// `)` `]` `}` `++` `--` 会让"下一个换行译成 `;`"。那属于 `SemicolonPolicy`
    /// （`T-L14`），由 `T-L15` 的 `next()` 在拿到 `kind` 之后设置 —— 与本模块
    /// 对 `ident` 的处理一致。
    pub fn scan_punct(&mut self) -> Option<PunctScan> {
        self.ensure_started();
        let ch = self.src.ch()?;
        if !is_punct_start(ch) {
            return None;
        }
        Some(match ch {
            '(' => self.simple(Delim::LParen),
            '[' => self.simple(Delim::LBrack),
            '{' => self.simple(Delim::LBrace),
            ',' => self.simple(Delim::Comma),
            ')' => self.simple(Delim::RParen),
            ']' => self.simple(Delim::RBrack),
            '}' => self.simple(Delim::RBrace),
            ';' => {
                self.bump();
                PunctScan::Token(TokenKind::Semi(SemiOrigin::Explicit))
            }
            ':' => {
                self.bump();
                if self.src.ch() == Some('=') {
                    self.bump();
                    op_expr(OpForm::Plain, Operator::Define)
                } else {
                    PunctScan::Token(TokenKind::Delim(Delim::Colon))
                }
            }
            '.' => {
                // 只有 `.` 分支可能需要回退（`..` / `.`+数字），所以只有它要活动段。
                self.src.start_segment();
                let r = self.scan_dot();
                self.src.stop_segment();
                r
            }
            '+' => {
                self.bump();
                if self.src.ch() == Some('+') {
                    self.bump();
                    op_expr(OpForm::IncDec, Operator::Add)
                } else {
                    self.assign_op(Operator::Add)
                }
            }
            '-' => {
                self.bump();
                if self.src.ch() == Some('-') {
                    self.bump();
                    op_expr(OpForm::IncDec, Operator::Sub)
                } else {
                    self.assign_op(Operator::Sub)
                }
            }
            '*' => {
                // 官方特意注释「don't goto assignop - want _Star token」——
                // 因为 `a * b` 与 `*p` 在语法上要分开。本仓用 `(Plain, Mul)` 表达前者，
                // 不需要额外类别（D-M1-10 的无损性核对表）。
                self.bump();
                if self.src.ch() == Some('=') {
                    self.bump();
                    op_expr(OpForm::Assign, Operator::Mul)
                } else {
                    op_expr(OpForm::Plain, Operator::Mul)
                }
            }
            '/' => {
                // 注释的**段**从第一个 `/` 起算（官方在 redo 循环开头已 `s.start()`，
                // token 起点就是 `/`）。所以在消耗任何字符**之前**先开段；
                // 若不是注释（除法家族），立刻放弃这个段。
                // ⚠️ 注释分支**故意不关段** —— 文本由 `scan_comment` 取走（跨调用契约）。
                self.src.start_segment();
                self.bump();
                if matches!(self.src.ch(), Some('/' | '*')) {
                    let style = if self.src.ch() == Some('/') {
                        CommentStyle::Line
                    } else {
                        CommentStyle::Block
                    };
                    self.bump();
                    PunctScan::Comment(style)
                } else {
                    self.src.stop_segment();
                    self.assign_op(Operator::Div)
                }
            }
            '%' => {
                self.bump();
                self.assign_op(Operator::Rem)
            }
            '&' => {
                self.bump();
                if self.src.ch() == Some('&') {
                    self.bump();
                    op_expr(OpForm::Plain, Operator::AndAnd)
                } else {
                    // `&^` 与 `&` 共用 `assignop`，只有 op 不同
                    let mut operator = Operator::And;
                    if self.src.ch() == Some('^') {
                        self.bump();
                        operator = Operator::AndNot;
                    }
                    self.assign_op(operator)
                }
            }
            '|' => {
                self.bump();
                if self.src.ch() == Some('|') {
                    self.bump();
                    op_expr(OpForm::Plain, Operator::OrOr)
                } else {
                    self.assign_op(Operator::Or)
                }
            }
            '^' => {
                self.bump();
                self.assign_op(Operator::Xor)
            }
            '<' => {
                self.bump();
                match self.src.ch() {
                    Some('=') => {
                        self.bump();
                        op_expr(OpForm::Plain, Operator::Leq)
                    }
                    Some('<') => {
                        self.bump();
                        self.assign_op(Operator::Shl)
                    }
                    // `<-` 是 `_Arrow`：官方**不设 op**（F21），本仓给语义值但不给优先级
                    Some('-') => {
                        self.bump();
                        op_expr(OpForm::Plain, Operator::Arrow)
                    }
                    _ => op_expr(OpForm::Plain, Operator::Lss),
                }
            }
            '>' => {
                self.bump();
                match self.src.ch() {
                    Some('=') => {
                        self.bump();
                        op_expr(OpForm::Plain, Operator::Geq)
                    }
                    Some('>') => {
                        self.bump();
                        self.assign_op(Operator::Shr)
                    }
                    _ => op_expr(OpForm::Plain, Operator::Gtr),
                }
            }
            '=' => {
                self.bump();
                if self.src.ch() == Some('=') {
                    self.bump();
                    op_expr(OpForm::Plain, Operator::Eql)
                } else {
                    // 官方 `_Assign`，**不设 op**（F21）；本仓给 `Operator::Assign`、prec 0
                    op_expr(OpForm::Plain, Operator::Assign)
                }
            }
            '!' => {
                self.bump();
                if self.src.ch() == Some('=') {
                    self.bump();
                    op_expr(OpForm::Plain, Operator::Neq)
                } else {
                    op_expr(OpForm::Plain, Operator::Not)
                }
            }
            '~' => {
                self.bump();
                op_expr(OpForm::Plain, Operator::Tilde)
            }
            other => unreachable!("is_punct_start 已排除 {other:?}"),
        })
    }

    /// 扫一个注释；返回它对「下一个词元」的影响（见 [`CommentScan`]）。
    ///
    /// **入口前置条件**：由 [`Lexer::scan_punct`] 的 [`PunctScan::Comment`] 交接而来 —
    /// 开头两字符（`//` 或 `/*`）已消耗、游标在注释体第一个字符上（EOF 也算），
    /// 且从第一个 `/` 起算的活动**段**已经打开（段由本函数负责关闭）。
    ///
    /// 两处「官方字段 → 显式参数」的镜像（与 `nlsemi` 同一手法，`M1-design.md`
    /// 「照抄语义、不照抄实现」）：
    ///
    /// - `start` = 官方的 `s.line/s.col`（token 起点，redo 循环开头记录）。
    ///   注释上报与未终止诊断都报这个位置，跨行块注释的 `Semi` 也用它。
    ///   注意**不能**在本函数里用 `self.pos()` 推——注释体里若有被跳过的
    ///   违规字节（`//\0`），游标位置与「起点 + 常量」不再等价。
    /// - `nlsemi` = 官方的 `s.nlsemi`（`SemicolonPolicy::take()` 的值，T-L14）。
    ///
    /// 本函数把官方两处逻辑收进一个入口：
    ///
    /// 1. `lineComment()` / `fullComment()`（`scanner.go:740/790`）—— 扫描与上报；
    /// 2. `case '*'` 里 fullComment 返回**之后**的产分号判定（`scanner.go:240-246`）——
    ///    它比较 `s.pos().line > s.line`，所以 `Semi` 的位置就是**注释起点**。
    ///    判定在**所有分支之后**执行（与闭合成功与否无关，p30/p06）。
    pub fn scan_comment(&mut self, start: Pos, style: CommentStyle, nlsemi: bool) -> CommentScan {
        let r = match style {
            CommentStyle::Line => {
                self.scan_line_comment(start);
                // 行注释永远不给分号（它本身停在 `\n` 之前，由分号层处理那个换行）
                CommentScan::Redo
            }
            CommentStyle::Block => self.scan_block_comment(start, nlsemi),
        };
        self.src.stop_segment();
        r
    }

    /// 行注释体（官方 `lineComment()`，`scanner.go:740` 逐条翻译）。
    ///
    /// **不消费 `\n`**（官方 `skipLine()` 的注释原文：「don't consume '\n' - needed
    /// for nlsemi logic」）；`\r` 是普通字符、**留在文本里**（p44，官方
    /// TestComments 的 `"// want this one\r"` 同款）。
    ///
    /// 三条去向（顺序照官方）：
    ///
    /// 1. `comments` 模式：全部上报（连 `//go:` 也当普通注释）；
    /// 2. 无 `directives` 位，或首字符不是 `g`/`l`：静默跳过
    ///    （⚠️ `NONE` 走的就是这条 —— 官方 `s.mode&directives == 0` 短路）；
    /// 3. `directives` 模式：逐字符匹配 `go:` / `line ` 前缀，**成不成功都跳过**，
    ///    只有成功才上报。
    ///
    /// 官方匹配失败路径里的 `s.stop()`（放弃段）在这里的等价物是「不取段」——
    /// 段由 [`Lexer::scan_comment`] 统一关闭。
    fn scan_line_comment(&mut self, start: Pos) {
        if self.mode.contains(Mode::COMMENTS) {
            self.skip_line();
            self.push_message(start, MessageKind::Comment);
            return;
        }

        // 官方：`if s.mode&directives == 0 || (s.ch != 'g' && s.ch != 'l')`
        let ch = self.src.ch();
        if !self.mode.contains(Mode::DIRECTIVES) || !matches!(ch, Some('g' | 'l')) {
            self.skip_line();
            return;
        }

        // 逐字符匹配前缀 —— 注意 `line ` **带尾随空格**（官方 prefix 常量原样），
        // 所以 `//line`、`//linea` 都不算指令，而 `//line foo` 算。
        let prefix: &str = if ch == Some('l') { "line " } else { "go:" };
        for m in prefix.chars() {
            if self.src.ch() != Some(m) {
                self.skip_line();
                return;
            }
            self.bump();
        }
        self.skip_line();
        self.push_message(start, MessageKind::Directive);
    }

    /// 块注释体（官方 `fullComment()` 与 `case '*'` 收尾判定，`scanner.go:790/240`）。
    ///
    /// 三条路径（顺序照官方）：
    ///
    /// 1. `comments` 模式：尝试闭合；**闭合成功才上报**（未终止只报诊断，p29）；
    /// 2. `directives` 模式且首字符是 `l`：先匹配 `"line "` 前缀、**再**闭合，
    ///    两者都成功才上报 —— 注意块注释**不认** `go:`（官方 fullComment 只匹配
    ///    "line "，`/*go:build*/` 永远不上报）；
    /// 3. 其余（`NONE`、首字符不是 `l`、前缀不匹配）：静默跳过。
    ///
    /// 最后统一做「跨行 + `nlsemi` ⇒ `Semi`」判定 —— 与闭合与否**无关**
    /// （p06/p30：未终止的跨行块注释照样产 `Semi`）。
    fn scan_block_comment(&mut self, start: Pos, nlsemi: bool) -> CommentScan {
        if self.mode.contains(Mode::COMMENTS) {
            if self.skip_comment(start) {
                self.push_message(start, MessageKind::Comment);
            }
        } else if self.mode.contains(Mode::DIRECTIVES) && self.src.ch() == Some('l') {
            let mut matched = true;
            for m in "line ".chars() {
                if self.src.ch() != Some(m) {
                    matched = false;
                    break;
                }
                self.bump();
            }
            let closed = self.skip_comment(start);
            if matched && closed {
                self.push_message(start, MessageKind::Directive);
            }
        } else {
            // 返回值（闭合与否）在这条路径上没有用途：不上报任何东西
            self.skip_comment(start);
        }

        // 官方：`if line, _ := s.pos(); line > s.line && nlsemi`
        // —— `s.line` 是注释起点行、`s.pos()` 是注释**之后**当前字符的行。
        if self.src.pos().line > start.line && nlsemi {
            CommentScan::Semi
        } else {
            CommentScan::Redo
        }
    }

    /// 官方 `skipLine()`（`scanner.go:733`）：跳到行尾，**不消费 `\n`**。
    ///
    /// 那句 `\n` 要留给分号逻辑（`\n` 只有在 `nlsemi` 置位时才译成 `;`）——
    /// 「吃掉它再补回去」是做不到的（位置已过）。
    fn skip_line(&mut self) {
        while let Some(c) = self.src.ch() {
            if c == '\n' {
                break;
            }
            self.bump();
        }
    }

    /// 官方 `skipComment()`（`scanner.go:775`）：扫到 `*/` 之后（含）。
    ///
    /// 撞 EOF 时报 `LEX-COMMENT-UNTERMINATED`（报在**注释起点** —— 官方
    /// `errorAtf(0, …)` 用的是 token 起点）并返回 `false`。
    ///
    /// 「报错但不上报文本」的守卫在调用方（[`Lexer::scan_block_comment`]）——
    /// 官方也是这么分工的：`if s.skipComment() { s.comment(…) }`。
    fn skip_comment(&mut self, start: Pos) -> bool {
        while self.src.ch().is_some() {
            while self.src.ch() == Some('*') {
                self.bump();
                if self.src.ch() == Some('/') {
                    self.bump();
                    return true;
                }
            }
            self.bump();
        }
        self.report_at(
            start,
            0,
            DiagCode::CommentUnterminated,
            "块注释未终止".to_string(),
        );
        false
    }

    /// 把当前活动段（从注释起点到游标之前的全部字节）收成一条上报。
    ///
    /// 段是 [`Lexer::scan_punct`] 的 `case '/'` 开的（见那里的跨调用契约）——
    /// 行注释停在 `\n` / EOF 之前，块注释停在 `*/` 之后，所以段的右端天然
    /// 就是「不含结尾换行、含全部定界符」的官方文本语义。
    fn push_message(&mut self, start: Pos, kind: MessageKind) {
        self.messages.push(Message {
            pos: start,
            kind,
            text: self.src.segment().to_vec(),
        });
    }

    /// 单字符分隔符。
    fn simple(&mut self, d: Delim) -> PunctScan {
        self.bump();
        PunctScan::Token(TokenKind::Delim(d))
    }

    /// 官方 `assignop` **汇合点**：`+=` 一族与它们的裸形式共用这段。
    ///
    /// 每个分支只需把 `operator` 摆好 —— 剩下的"后面是不是 `=`"在这里统一判。
    /// 11 个复合赋值运算符（F3）全靠这一段。
    fn assign_op(&mut self, operator: Operator) -> PunctScan {
        let form = if self.src.ch() == Some('=') {
            self.bump();
            OpForm::Assign
        } else {
            OpForm::Plain
        };
        op_expr(form, operator)
    }

    /// `.` 分支 —— 官方 `case '.'` 的逐条翻译，**全阶段唯一的回退点**。
    ///
    /// 官方原文（`scanner.go` 的 `case '.'`）：
    ///
    /// ```text
    /// s.nextch()                     // 吃掉第一个 '.'
    /// if isDecimal(s.ch) { s.number(true); break }
    /// if s.ch == '.' {
    ///     s.nextch()                 // 吃掉第二个 '.'
    ///     if s.ch == '.' { s.nextch(); s.tok = _DotDotDot; break }
    ///     s.rewind()                 // now s.ch holds 1st '.'
    ///     s.nextch()                 // consume 1st '.' again
    /// }
    /// s.tok = _Dot
    /// ```
    ///
    /// 注意回退之后**还有一次 `nextch()`** —— 少了它 `..` 会原地打转。
    fn scan_dot(&mut self) -> PunctScan {
        self.bump(); // 吃掉第一个 `.`

        if self.src.ch().is_some_and(is_ascii_digit) {
            // 官方在这里调 `number(true)`：`.5` 是**一个** Float 字面量。
            // 数字扫描器在 T-L10 —— 把游标**退回 `.` 上**再交接，
            // 这样它的前置条件保持统一（"总是从字面量的第一个字符开始"），
            // 不必区分"数字起头"与"点号起头"两种入口状态。
            self.rewind_to_segment_start();
            return PunctScan::LeadingDotNumber;
        }

        if self.src.ch() == Some('.') {
            self.bump(); // 吃掉第二个 `.`
            if self.src.ch() == Some('.') {
                self.bump(); // 吃掉第三个 `.`
                return PunctScan::Token(TokenKind::Delim(Delim::Ellipsis));
            }
            // 不是三个点 ⇒ 回到段首（第一个 `.`），再吃掉它
            self.rewind_to_segment_start();
            self.bump();
        }

        PunctScan::Token(TokenKind::Delim(Delim::Dot))
    }

    /// **诊断的唯一入口** —— lexer 侧的全部报错路径都经这里进包。
    ///
    /// 为什么要「只有一个」：诊断包是**多来源的汇合处**（字符层的 `advance`/`rewind`
    /// 返回值、扫描层的 `report`/`report_at`）。若各自直写 `self.diags`，将来加约束
    /// （条数上限、去重、debug 期校验）时**必然漏掉某一路** —— 而漏掉不会报错，
    /// 只会让约束"部分生效"（"漏了不会报错"已在本仓三次同形）。
    ///
    /// 字符层（`source.rs`）内部另有 3 处对**局部 `Vec<Diag>`** 的写入 —— 那不是
    /// 漏洞：它们是 `advance`/`rewind` 的返回值，[`Lexer::bump`] 与
    /// [`Lexer::rewind_to_segment_start`] **全量收编**，结构上必进本函数。
    ///
    /// 它同时是反验的锚点：对 `push_diag` 做变异（如按类别丢弃）⇒ **全部报错路径**
    /// 同时受影响，任何一条绕过它的路径都会在变异下"漏网"而暴露。
    fn push_diag(&mut self, d: Diag) {
        self.diags.push(d);
    }

    /// 回退到活动段起点。
    ///
    /// `SourceReader::rewind` 返回诊断（它内部会重读段首字符），这里照 [`Lexer::bump`]
    /// 的做法收进自己的包。
    fn rewind_to_segment_start(&mut self) {
        for d in self.src.rewind() {
            self.push_diag(d);
        }
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
            self.push_diag(d);
        }
    }

    /// 在**当前字符**的位置报一条诊断（对应官方 `s.error` 报在 `s.pos()`）。
    fn report(&mut self, code: DiagCode, message: String) {
        self.push_diag(Diag {
            pos: self.src.pos(),
            code,
            message,
        });
    }

    /// 在**相对词元起点 `offset` 字节**处报一条诊断（对应官方 `errorAtf`）。
    ///
    /// 官方 `errorAtf(offset, …)` 报在 `Pos { 词元起点行, 词元起点列 + offset }`。
    /// 数字字面量不可能跨行（[`Lexer::digits`] 只吃单字节 ASCII），
    /// 所以行号直接沿用词元起点行。
    fn report_at(&mut self, start: Pos, offset: u32, code: DiagCode, message: String) {
        self.push_diag(Diag {
            pos: Pos {
                // 诊断由词法层产 ⇒ 不挂基准（官方 `scanner` 的 `errorAtf` 走
                // `NoPos`/`p.pos()`，基准由 parser 侧补；见 `source.rs::pos`）。
                base: None,
                file: start.file,
                line: start.line,
                col: start.col + offset,
            },
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

/// 全部**能起头**一个运算符或标点的字符（23 个）。
///
/// 与官方 `next()` 那个 `switch` 的 `case` 列表**逐条对应** ——
/// 多认一个字符（比如把 `@` 也收进来）会让"非法字符"诊断永远不触发，
/// 少认一个会让合法程序切出 `None`。两边都是静默的，
/// 所以 `tests/test_punct.rs` 里有一条**ASCII 全域穷举**盯着这个集合。
fn is_punct_start(c: char) -> bool {
    matches!(
        c,
        '(' | '['
            | '{'
            | ','
            | ';'
            | ')'
            | ']'
            | '}'
            | ':'
            | '.'
            | '+'
            | '-'
            | '*'
            | '/'
            | '%'
            | '&'
            | '|'
            | '^'
            | '<'
            | '>'
            | '='
            | '!'
            | '~'
    )
}

/// 造一个 `Op` 词元，优先级直接取自 [`Operator::prec`]（与官方 `precXxx` 常量同编号）。
fn op_expr(form: OpForm, operator: Operator) -> PunctScan {
    PunctScan::Token(TokenKind::Op {
        form,
        op: operator,
        prec: operator.prec(),
    })
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

/// [`Lexer::scan_punct`] 的结果。
///
/// 三种结果各自对应官方 `next()` 里的一条**非词元**出口 —— 把它们显式建模成类型，
/// 而不是让调用方从 `None` 里猜，是因为"没扫到标点"与"扫到一半要交接给别人"
/// 在后续处理上**完全不同**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PunctScan {
    /// 一个完整的运算符 / 标点词元。
    Token(TokenKind),
    /// `.` 后面紧跟数字 ⇒ 这是**数字字面量**的开头（官方 `case '.'` 里的 `number(true)`）。
    ///
    /// `.5` 是**一个** Float 词元。交接时游标已由 `rewind` 退回那个 `.` 上、
    /// **一个字符都没消耗** —— 数字扫描器（T-L10）从字面量第一个字符开始。
    LeadingDotNumber,
    /// `//` 或 `/*`：注释开头。
    ///
    /// **两个字符都已被消耗**，游标在注释体的第一个字符上（EOF 时是 `None`）——
    /// 与官方调 `lineComment()` / `fullComment()` 时的状态一致。
    ///
    /// 同时：一个从第一个 `/` 起算的活动**段**已经打开，等
    /// [`Lexer::scan_comment`] 取走文本（它是段的所有者，负责关闭）。
    Comment(CommentStyle),
}

/// 注释的种类（`//` 与 `/* */`）。
///
/// 官方不用类型表达 —— `case '/'` 里两个就地 `if` 分流。这里显式成类型并**随
/// [`PunctScan::Comment`] 带出来**，是因为 Rust 侧的 `scan_punct` 在交接时
/// 已经消耗了这两个字符：调用方（`scan_comment`）无从再从游标判别种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommentStyle {
    /// `//` —— 到行尾（**不消费换行**）
    Line,
    /// `/* */` —— 可跨行
    Block,
}

/// [`Lexer::scan_comment`] 的结果 —— 注释对「下一个词元」的影响。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommentScan {
    /// 注释已处理完，调用方**重扫**（官方 `goto redo`）：
    /// 下一个词元从注释之后开始。
    Redo,
    /// **跨行块注释 + `nlsemi`**：调用方应当产出 `Semi(Newline)`，
    /// 位置取**注释起点**（官方 `case '*'` 里 `s.line/s.col` 是 token 起点、
    /// 全程未被更新）。
    Semi,
}

/// 数字字面量的进制前缀（官方 `number()` 里那个 `prefix rune`）。
///
/// `None` = 十进制（没有前导零）；`Zero` = **前导 `0`**（退化成八进制）——
/// 官方用 `0` 与 `'0'` 两个不同常量区分这两者，Rust 里用两个变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NumPrefix {
    None,
    Zero,
    X,
    O,
    B,
}

/// Unicode 码点上限（官方 `unicode.MaxRune`）。转义的 `max` 用得上。
const MAX_RUNE: u32 = 0x10FFFF;

/// 诊断消息里的进制名（官方 `baseName`）。消息文本不参与比对，只为人读。
fn base_name(base: u8) -> &'static str {
    match base {
        2 => "二进制",
        8 => "八进制",
        10 => "十进制",
        16 => "十六进制",
        _ => "未知进制",
    }
}

/// 官方 `invalidSep(x)` 的逐条翻译：**事后**定位 `_` 的非法位置。
///
/// 一次正扫，`prev` 记住"上一个字符是哪一类"：
/// - `b'0'` = 数码（`0x` / `0o` / `0b` 前缀**也算一个数码** ⇒ `0x_1f` 合法）
/// - `b'_'` = 下划线
/// - `b'.'` = 其他任何东西（小数点、`e`/`p`、正负号、`i`）
///
/// 返回非法 `_` 相对字面量起点的**字节下标**；全部合法时 `None`。
/// "收尾的 `_`"要单独处理 —— 循环里发现不了"后面什么都没有"。
///
/// 字面量不可能含非 ASCII（扫描阶段只吃 ASCII 字符），所以按字节扫是安全的。
fn invalid_sep(x: &[u8]) -> Option<usize> {
    let mut prefix = b' '; // 官方 `x1`：只关心是不是 'x'（决定 `a`-`f` 算不算数码）
    let mut prev = b'.';
    let mut i = 0;

    // 前缀 `0x` / `0o` / `0b` 算一个数码
    if x.len() >= 2 && x[0] == b'0' {
        prefix = x[1].to_ascii_lowercase();
        if matches!(prefix, b'x' | b'o' | b'b') {
            prev = b'0';
            i = 2;
        }
    }

    while i < x.len() {
        let p = prev; // 上一个字符的类别（官方 `p := d`）
        let c = x[i];
        if c == b'_' {
            if p != b'0' {
                return Some(i);
            }
            prev = b'_';
        } else if c.is_ascii_digit() || (prefix == b'x' && c.is_ascii_hexdigit()) {
            prev = b'0';
        } else {
            if p == b'_' {
                // `i >= 1` 必然成立：`p == b'_'` 只可能由上轮写进，而上轮至少是 i-1
                return Some(i - 1);
            }
            prev = b'.';
        }
        i += 1;
    }

    if prev == b'_' {
        return Some(x.len() - 1);
    }
    None
}
