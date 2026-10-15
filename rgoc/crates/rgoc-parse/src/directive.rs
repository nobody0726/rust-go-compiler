//! `//line` 位置重映射 + `//go:` 指令解析（`T-AP15`，对齐 `M2-design.md` §5.5）。
//!
//! 对齐官方 `parser.go` 的 `init` 里那个 scanner 回调（`:51-88`）、`updateBase`
//! （`:124-175`）与 `commentText`（`:177-189`）。
//!
//! # 为什么这两件事住在一起
//!
//! 官方把「`//line` 改基准」与「`//go:` 挂 pragma」塞在**同一个回调**里，
//! 靠「注释文本以什么开头」分派。拆开就要在词元流上做两次判定，而那两处的
//! **`col` 偏移量不同**（`:73` 的 `col+2+5` vs `:85` 的 `col+2`）——
//! 放一起时偏移量是**算一次、写两处**，分开后每个都要重抄一遍 `+2`。
//!
//! # `text` 里已经有什么（官方 `commentText`，`:177-189`）
//!
//! ```text
//! "//go:build"  →  "go:build"      （去掉 `//`）
//! "/*line foo*/" →  "line foo "    （去掉 `/*` 与 `*/`；`text[5:]` 后是 " foo"）
//! ```
//!
//! ⚠️ **行注释的 `\r` 也被剥掉**（`:184-187`）—— 那是官方对 Windows 文件的处理，
//! 我们照搬：`text` 若以 `\r` 结尾就**去尾**。
//!
//! # 官方那句注释（`:57-59`）是本模块存在的理由
//!
//! > *"Because the (line, col) positions passed to the handler is always at or
//! > after the current reading position, it is safe to use the most recent
//! > position base to compute the corresponding Pos value."*
//!
//! 官方把 scanner 的回调设计成「**同步**调用」，所以 parser 能在回调里直接改
//! `p.base`。我们这里 lexer 是**拉取式**的（`next()` 才推进），所以改成
//! 「**取到消息时立刻处理**」—— 时机上等价（都在「读到这条注释的下一个词元之前」），
//! 而 `p.base` 只由 [`Parser::next`] 那一处换算 ⇒ 不存在「base 改了但已造好的
//! `Pos` 没跟上」的窗口期。

use rgoc_ast::Pragma;
use rgoc_lex::diag::DiagCode;
use rgoc_lex::message::{Message, MessageKind};
use rgoc_lex::pos::Pos;

use crate::parser::Parser;

/// 官方 `colbase`（`pos.go:154` 附近的常量）—— 行的第 1 列。
const COLBASE: u32 = 1;

/// 官方 `PosMax`（`pos.go:13`）：`1 << 30`。行号/列号超过它会被 `sat32` 截断。
const POS_MAX: u32 = 1 << 30;

/// 一条指令分派的结果（`Parser::handle_directive` 的返回值）。
pub(crate) enum Classified {
    /// 不是指令（官方回调的第一条 `if msg[0] != '/'` 之外的情形）。
    NotDirective,
    /// `//go:` 指令（`text` 已去 `//` 前缀）。
    Go(String),
    /// `//line` 指令 —— `text` 是**去掉 `line ` 之后**的部分（官方 `:73` 传 `text[5:]`）。
    Line(String),
}

/// 把一条 lexer 上报的注释/指令分成三类（官方 `init` 回调的 `:52-79` 那三步）。
///
/// ⚠️ 官方对 `//line` 的判据是 **两个条件的合取**：
/// `(col == colbase || msg[1] == '*') && strings.HasPrefix(text, "line ")`。
/// 即 `//line` **必须落在行首**（`col == 1`），而 `/*line */` 可以落在行内任意处。
/// 漏掉 `col == colbase` 那一半 ⇒ 行内的 `//line` 会**错误地**改基准；
/// 漏掉 `msg[1] == '*'` 那一半 ⇒ 行内的 `/*line */` 不生效。
pub(crate) fn classify(m: &Message) -> Classified {
    if m.kind != MessageKind::Directive {
        return Classified::NotDirective;
    }
    let text = comment_text(&m.text);
    // `//line` 要在行首；`/*line */` 任意处都可以（官方 `:59` 的注释）。
    let is_block = m.text.starts_with(b"/*");
    let at_line_start = m.pos.col == COLBASE;
    if (at_line_start || is_block) && text.starts_with("line ") {
        return Classified::Line(text[5..].to_string());
    }
    if text.starts_with("go:") {
        return Classified::Go(text);
    }
    Classified::NotDirective
}

/// 官方 `commentText`（`parser.go:177-189`）：去掉 `//` 或 `/* */` 定界符，
/// 行注释再去掉结尾的 `\r`。
///
/// ⚠️ **有损转换**：`Message::text` 是 `Vec<u8>`，非法 UTF-8 走 `String::from_utf8_lossy`
/// ⇒ 那种输入会被替换成 U+FFFD。官方直接对 `string` 操作，不存在这个问题。
/// 与 `lit_text()` 属**同一个**已知缺口，归 `T-AP17`/`T-AP18` 的 C6 归一
/// （**不登记就会在 `T-AP19` 全量对撞时以「某行 diff」暴露**）。
fn comment_text(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    if let Some(rest) = s.strip_prefix("/*") {
        // 官方 `s[2 : len(s)-2]`（去尾 `*/`）。`strip_prefix` 之后是 `…*/`。
        return rest.strip_suffix("*/").unwrap_or(rest).to_string();
    }
    // 行注释（不含结尾换行）：剥 `//`，再剥结尾的 `\r`（官方 `:184-187`）。
    let rest = s.strip_prefix("//").unwrap_or(&s);
    rest.strip_suffix('\r').unwrap_or(rest).to_string()
}

impl Parser {
    /// 造一个「报在 `(tline, tcol)`」的诊断位置（官方 `p.posAt(tline, tcol+…)`）。
    ///
    /// 官方 `posAt` 用的是**当前** `p.base`（`parser.go:234-236`）—— 那条注释
    /// （`:47-50`）明确说「回调传进来的 `(line, col)` 总在当前读取位置**之后**，
    /// 所以用**最近的** base 是安全的」。这里照抄：base 取**当前**那条。
    fn diag_pos(&self, pos: Pos, tline: u32, tcol: u32) -> Pos {
        Pos {
            base: self.bases.current(),
            file: pos.file,
            line: tline,
            col: tcol,
        }
    }

    /// 处理一条 lexer 上报的注释/指令（官方 `init` 那个回调，`parser.go:51-88`）。
    ///
    /// # `//go:build` 的 `goVersion` **只在 `top`**（`F7`）
    ///
    /// 官方 `:79`：`if p.top && strings.HasPrefix(msg, "//go:build")`。
    /// ⚠️ 这个 `top` 判据是**端到端可观测**的：把 `//go:build` 放到 package
    /// 子句**之后**，`go_version` 必须是 `None`。
    pub(crate) fn handle_directive(&mut self, m: &Message) {
        match classify(m) {
            Classified::NotDirective => {}
            Classified::Go(text) => {
                // 官方 `:79-83`：仅 `top` 时、`constraint.Parse` 成功才取。
                // let-chain 直译官方那句 `&&` 合取（照抄嵌套 `if` 会被 clippy 打爆门禁）。
                if self.top
                    && m.text.starts_with(b"//go:build")
                    && let Some(v) = go_version_from_build(&text)
                {
                    self.go_version = Some(v);
                }
                // 官方 `:84-86`：`if pragh != nil { p.pragma = pragh(pos, blank, text, p.pragma) }`
                // —— M2 的 `pragh` 恒非 nil（我们总是挂载），见 [`Self::pragma_handler`]。
                self.pragma = self.pragma_handler(&text);
            }
            Classified::Line(rest) => {
                // 官方 `:61-74`。`pos` = 「紧跟注释之后」的位置：
                // - 行注释 ⇒ **下一行**的开头（换行属于行注释）
                // - 块注释 ⇒ 本行的 `col + len(msg)`
                let pos = if m.text.starts_with(b"//") {
                    Pos {
                        base: self.bases.current(),
                        file: m.pos.file,
                        line: m.pos.line + 1,
                        col: COLBASE,
                    }
                } else {
                    Pos {
                        base: self.bases.current(),
                        file: m.pos.file,
                        line: m.pos.line,
                        col: m.pos.col + m.text.len() as u32,
                    }
                };
                // 官方 `:73`：`p.updateBase(pos, line, col+2+5, text[5:])`
                // —— `+2` 跳 `//` 或 `/*`，`+5` 跳 `"line "`。
                let tcol = m.pos.col + 2 + 5;
                self.update_base(pos, m.pos.line, tcol, &rest);
            }
        }
    }

    /// 官方 `updateBase`（`parser.go:124-175`）：解析 `//line filename:line[:col]` 并改基准。
    ///
    /// ⚠️ **`trailing_digits` 从**右边**找 `:`**（官方 `:192` 的注释：
    /// *"look from right (Windows filenames may contain ':')"*）——
    /// 所以 `//line C:foo:123` 的文件名是 `C:foo` 而不是 `foo`。
    /// 用 `find(':')`（从左）会把 `C:foo` 拆成 `C` + `foo:123` ⇒ 报错。
    pub(crate) fn update_base(&mut self, pos: Pos, tline: u32, tcol: u32, mut text: &str) {
        // 官方 `:125-135`
        let Some((mut i, n, ok)) = trailing_digits(text) else {
            return; // 官方 `:126-128`：没有 `:` ⇒ 不是 line 指令，**静默忽略**
        };
        if !ok {
            // 官方 `:131-135`：有 `:xxx` 但 xxx 不是数字。
            let p = self.diag_pos(pos, tline, tcol + i);
            self.error_at(
                p,
                DiagCode::SynInvalidLineDirective,
                format!("invalid line number: {}", &text[i as usize..]),
            );
            return;
        }

        // 官方 `:137-151`。⚠️ `i, i2 = i2, i` 那个**交换必须照做** ——
        // 交换后 `i` 指向**前**一个 `:` 的后一位、`i2` 指向**后**一个 `:` 的后一位，
        // 后面三处错误消息的**位置与文本切片**全依赖它。跳过交换得到的
        // 「文件名正确、错误消息串到别处」症状很难定位。
        let i2;
        let mut line = n;
        let mut col = 0u32;
        let mut ok2 = false;
        // `text[:i-1]`（去掉最后一个 `:`）
        //
        // ⚠️ 官方是三层嵌套 `if`；这里用 **let-chain**（edition 2024）串成一个合取，
        // 条件与顺序**逐个照抄**。拆成三个独立 `match` 会丢掉「`i > 1` 是前置守卫」
        // 这层语义（`i == 1` 时 `&text[..0]` 会 panic）。
        if i > 1
            && let Some((i2_, n2, ok2_)) = trailing_digits(&text[..(i - 1) as usize])
            && ok2_
        {
            // 官方 `:142`：`line, col = n2, n` —— **行取前一个、列取后一个**。
            line = n2;
            col = n;
            i2 = i; // 官方 `:141` 的 `i, i2 = i2, i` 之后的 `i2`
            i = i2_; // 交换后的 `i`
            ok2 = true;

            // 官方 `:143-146`：**列**必须是 `1..=PosMax`。
            // ⚠️ 检查的是 `col`（= 后一个数字），不是 `n2`（= 行）。
            // 检查错对象 ⇒ `/*line :123:0*/` 不报错（官方 `parser_test.go:320` 期望报错）。
            if col == 0 || col > POS_MAX {
                let p = self.diag_pos(pos, tline, tcol + i2);
                self.error_at(
                    p,
                    DiagCode::SynInvalidLineDirective,
                    format!("invalid column number: {}", &text[i2 as usize..]),
                );
                return;
            }
            // 官方 `:147`：`text = text[:i2-1]`（切掉 `":col"`）。
            text = &text[..(i2 - 1) as usize];
        }

        // 官方 `:153-156`
        if line == 0 || line > POS_MAX {
            let p = self.diag_pos(pos, tline, tcol + i);
            self.error_at(
                p,
                DiagCode::SynInvalidLineDirective,
                format!("invalid line number: {}", &text[i as usize..]),
            );
            return;
        }

        // 官方 `:158-172`：空文件名在 `ok2`（有 `:col`）时沿用上一个基准的文件名。
        //
        // ⚠️ `if filename == "" && ok2` 里的 **`&& ok2`** 是**唯一**能区分
        // 官方 `parser_test.go:341`（留空）与 `:342`（沿用）两条期望的条件。
        // 无条件沿用的实现会让 `:341` 得到 `dir/directives.go` ⇒ 对不上。
        let mut filename = text[..(i - 1) as usize].to_string();
        let mut trimmed = false;
        if filename.is_empty() && ok2 {
            filename = self
                .bases
                .current()
                .and_then(|b| self.bases.get(b))
                .map(|b| b.filename.clone())
                .unwrap_or_default();
            trimmed = self
                .bases
                .current()
                .and_then(|b| self.bases.get(b))
                .is_some_and(|b| b.trimmed);
        } else if !filename.is_empty() {
            // 官方 `:165-171`：`filepath.Clean` + 相对路径拼到源文件目录。
            //
            // ⚠️ **本轮刻意不做**，登记在 `progress.md` 的 T-AP15 残留表：
            // ① `filepath` 是 **OS 相关**的（`\` vs `/`），而首发平台只 Linux；
            // ② 相对路径拼接要用「源文件的**目录**」，而我们的 `Parser` 目前
            //    只拿到**文件名**（`parse_file(src, name)`）—— 那是 M3 的
            //    SourceMap 才有的信息。
            // 官方 `parser_test.go:346-352` 那 6 条「effect of cleaning filenames」
            // 因此暂不可测。名字**先原样存**，形状与官方一致。
        }

        self.bases.line_base(pos, &filename, trimmed, line, col);
    }

    /// 官方 `PragmaHandler`（`parser.go:85` 那一处调用，`F8`）。
    ///
    /// 官方的签名是 `func(pos Pos, blank bool, text string, old Pragma) Pragma`。
    /// M2 的处理：**收集**到 [`Parser::pragma`] 的 `directives` 里（`rgoc_ast::Pragma`），
    /// 由各声明产生式用 `take_pragma()` 取走并挂到节点上。
    ///
    /// # `directives` 里存的是**去掉 `go:` 前缀**之后的文本
    ///
    /// 官方 `go/pragmas.go` 的 `pragmaHandler` 收到的 `text` 已经**含** `go:` 前缀
    /// （`parser.go:85` 传的是 `text` 本身），由它自己 `strings.Cut(text, ":")` 拆开。
    /// 我们在这里拆 —— 因为**存进 AST 的应当是「指令内容」而不是「带前缀的原文」**：
    /// `//go:nosplit` 存 `nosplit`，`//go:linkname f g` 存 `linkname f g`。
    /// ⚠️ 若连 `go:` 一起存，读出来是 `go:nosplit` ⇒ 消费方每次都要再剥一遍前缀，
    /// 而剥的位置一多就会漏（M1 的 lexer 已经剥过一次 `//`，这里再剥 `go:`）。
    ///
    /// # 「已知指令清单」与「未知指令」
    ///
    /// `M2-tests.md` §4.2 的清单 + 「拒绝未知**关键**指令」是 `03-roadmap.md` §M2 的要求。
    /// ⚠️ **本轮刻意不实现「拒绝」**（官方是**全部忽略**）—— 两边行为冲突，
    /// 已作为 `§9 待裁决` 登记在 `progress.md`，归 `T-AP16`。这里**一律收下**，
    /// 未知的不报错（= 官方），「拒绝」留给裁决后。
    pub(crate) fn pragma_handler(&mut self, text: &str) -> Pragma {
        let body = text.strip_prefix("go:").unwrap_or(text).to_string();
        let mut p = std::mem::take(&mut self.pragma);
        p.directives.push(body);
        p
    }
}

/// 官方 `trailingDigits`（`parser.go:191-199`）：
///
/// ```text
/// 返回 (i, n, ok)：i = 最后一个 ':' 的下标 + 1；n = i 之后的数字；ok = 能否解析
/// 没有 ':' ⇒ (0, 0, false)
/// ```
pub(crate) fn trailing_digits(text: &str) -> Option<(u32, u32, bool)> {
    let i = text.rfind(':')?;
    match text[i + 1..].parse::<u32>() {
        Ok(n) => Some((i as u32 + 1, n, true)),
        // 官方 `strconv.ParseUint(text[i+1:], 10, 0)` 出错时返回 `ok = false`，
        // 而 `i` 仍**是** `LastIndexByte(text, ':') + 1` —— 调用方据此报
        // `invalid line number: <text[i:]>`（官方 `:133`）。
        //
        // ⚠️ 必须返回 `Some(…, false)` 而不是 `None`：`None` 会让上层走
        // 「没有 `:` ⇒ 静默忽略」那条分支 ⇒ `/*line foo:x*/` **不报错**，
        // 而官方 `parser_test.go:305` 的期望是报 `invalid line number: x`。
        Err(_) => Some((i as u32 + 1, 0, false)),
    }
}

/// 官方 `parser.go:80-82`：`constraint.Parse(msg)` + `constraint.GoVersion(x)`。
///
/// ⚠️ `go/build/constraint` 的**完整**表达式语法（`&&` / `||` / `!` / 括号 /
/// `unix` 之类标签）在 M2 **不实现** —— 那是一个独立模块。我们只取
/// 「**首行首项**形如 `go1.NN` 的那个版本号」，即 `//go:build go1.21` 与
/// `//go:build go1.21 && !go1.19` 都取到 `go1.21`。
///
/// # 为什么不实现完整表达式（登记在 `progress.md` 的 T-AP15 残留表）
///
/// `constraint.GoVersion` 的语义是「表达式里提到的**最高**版本」，而
/// `&& go1.20 && go1.21` 的最高版本在**中间**、`|| go1.19 || go1.22` 的在末尾
/// ⇒ 要取对就得真的解析整棵表达式树。取「首个 `goX.Y`」在常见的
/// `//go:build go1.NN` 与 `//go:build go1.NN && …` 上**结果相同**，
/// 只在 `//go:build !go1.19`（期望「无版本」⇒ 不设 goVersion）与
/// `//go:build unix`（无版本）这类**没有裸版本号**的表达式上不同 ——
/// 而那些情况下 M2 的版本门控本来也不该生效。
pub(crate) fn go_version_from_build(text: &str) -> Option<String> {
    // `text` 形如 `go:build go1.21 && !go1.19` ⇒ 去掉 `go:build ` 前缀。
    let rest = text.strip_prefix("go:build")?;
    for tok in rest.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.')) {
        if tok.len() > 2
            && tok.starts_with("go1")
            && tok[2..].chars().all(|c| c.is_ascii_digit() || c == '.')
        {
            return Some(tok.to_string());
        }
    }
    None
}
