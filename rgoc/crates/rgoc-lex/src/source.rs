//! 字符层 —— 字节缓冲 + UTF-8 解码 + 位置推进 + 段切片（`M1-design.md` §5.1）。
//!
//! 这一层**不认识任何词元**。它只回答三个问题：现在读到哪个字符、它在哪一行哪一列、
//! 以及"从段起点到现在"的原始字节是什么。
//!
//! # 三条真语义
//!
//! 1. **`(line, col)` 是「当前 `ch` 的位置」**（1 起）—— 不是"下一个待读的位置"。
//!    官方 `source.pos()` 就是 `(linebase+line, colbase+col)`。
//! 2. **`col` 的单位是字节**。`变量` 占 6 列、`😀` 占 4 列。
//!    这不是 bug，是 C1 §2.1 的硬规定 —— 位置要能与 oracle 逐字节对齐。
//! 3. **[`segment`](SourceReader::segment) 不含当前 `ch`**（`buf[b : r-chw]`）⇒
//!    "切出来的词元天然不含终止符"，这是 LEX-02 那一节里"游标停在未归属字符上"的落地。
//!
//! # 相对官方的三处简化（D-M1-7「照抄语义、不照抄实现」）
//!
//! | 官方机制 | 这里 | 理由 |
//! |---|---|---|
//! | `io.Reader` 增量填充 + `e` 索引 + `fill()` | **去掉**（一次性读入） | M1 的输入是文件不是管道；省掉增长逻辑与对应测试 |
//! | `sentinel` 哨兵字节 | **去掉** | 它只是"ASCII 快路径少一次比较"的微优化（**先正确，后优化**） |
//! | `b`（段起点）/ `r`（已读位置）/ `rewind()` | **保留** | 段切片与 `..` 的回退都要它 |
//!
//! 但**语义一处不改**，包括两个最容易被"优化掉"的地方：
//!
//! - **`redo` 回边在位置推进之前**：官方 `nextch` 的 `redo:` 标签在 `col += chw` 的**上方**，
//!   所以每一个被跳过的字节（NUL / 非法 UTF-8 / BOM）**各自推进一次列号**。
//!   把位置推进提到循环外会让 `x\0\0y` 的列号少算 2。
//! - **`rewind` 的前置条件**：活动段内**不得含换行**（否则位置信息就是错的）——
//!   官方文档明写，这里靠"位置直接算"的方式让该前置条件更不容易被违反（见下）。
//!
//! # 诊断怎么出去（P1 裁决 + 实测修正）
//!
//! 字符层**不持有**诊断集合（预检裁决 P1）：[`advance`](SourceReader::advance) 把诊断
//! **返回**给调用方，由 `Lexer` 收进自己的 `DiagBag`。
//!
//! ⚠️ 但返回类型是 `Vec<Diag>` 而**不是** `Option<Diag>` —— 见 [`advance`](SourceReader::advance)
//! 的文档：一次推进**可以**产出多条诊断（`x\0\0y` 实测两条）。

use crate::diag::{Diag, DiagCode};
use crate::pos::{FileId, Pos};

/// 段起点索引：`None` = 没有活动段。
type SegStart = Option<usize>;

/// 字符层读取器。
///
/// 字段语义（对应官方 `source` 的 `b/r/e` 与 `line/col/ch/chw`）：
///
/// - `b`：当前段起点在 `buf` 中的下标（`None` = 无活动段）
/// - `r`：**下一个待解码字节**的下标
/// - `ch`：**尚未归属**的字符；`None` = EOF
/// - `chw`：`ch` 的字节宽度 ⇒ `r - chw` 就是 `ch` 的起始字节
/// - `line` / `col`：**内部 0 起**；对外经 [`pos`](SourceReader::pos) 返回 1 起
#[derive(Debug)]
pub struct SourceReader {
    buf: Vec<u8>,
    file: FileId,
    b: SegStart,
    r: usize,
    ch: Option<char>,
    chw: u8,
    line: u32,
    col: u32,
}

impl SourceReader {
    /// 建一个读取器。**只收 `FileId`**（预检裁决 P2）——
    /// 造 `Pos` 只需要它；`FileTable` 归 `Lexer` 持有，交给这里会让 `Lexer` 拿不到所有权。
    ///
    /// 初始状态照官方：位置 (0,0)（对外即 1:1）、`chw = 0`、`ch` 尚无。
    /// 所以**第一次 [`advance`](Self::advance) 读到的是文件第一个字符，且位置正好是 1:1**。
    pub fn new(buf: Vec<u8>, file: FileId) -> Self {
        Self {
            buf,
            file,
            b: None,
            r: 0,
            ch: None,
            chw: 0,
            line: 0,
            col: 0,
        }
    }

    /// 当前**尚未归属**的字符；`None` = 文件结束。
    #[must_use]
    pub fn ch(&self) -> Option<char> {
        self.ch
    }

    /// `ch` 的字节宽度（1..=4；EOF 时为 0）。
    #[must_use]
    pub fn chw(&self) -> u8 {
        self.chw
    }

    /// 当前 `ch` 的位置，**行列都从 1 起、列按字节**。
    #[must_use]
    pub fn pos(&self) -> Pos {
        Pos {
            file: self.file,
            // 内部 0 起 → 对外 1 起（官方的 linebase / colbase）
            line: self.line + 1,
            col: self.col + 1,
        }
    }

    /// 往后读一个字符。
    ///
    /// # 为什么返回 `Vec<Diag>` 而不是 `Option<Diag>`
    ///
    /// 预检裁决 P1 设想的是 `Option<Diag>`，并注明"若一次推进产出多条诊断（**目前不会**）"。
    /// **实测推翻了那个前提**：官方 `nextch` 的 `redo:` 标签在位置推进**之前**，
    /// 所以 NUL / 非法 UTF-8 / BOM 的"跳过并重读"是**在同一个 `nextch` 里循环**的。
    ///
    /// ```text
    /// 输入 x\0\0y  ⇒ oracle 输出两条诊断：1:2 LEX-INVALID-NUL、1:3 LEX-INVALID-NUL
    /// 输入 x\xff\xffy ⇒ 两条 LEX-INVALID-UTF8
    /// ```
    ///
    /// `Option<Diag>` 只能带回一条 ⇒ **第二条被静默丢弃**。这正是本仓反复吃亏的形状。
    ///
    /// 代价为零：正常路径返回 `Vec::new()`，**不分配**。
    #[must_use]
    pub fn advance(&mut self) -> Vec<Diag> {
        let mut diags = Vec::new();
        loop {
            // ★ `redo` 回边必须在这里 —— 官方是：
            //     redo:
            //         s.col += uint(s.chw)
            //         ...
            //         goto redo
            //   位置推进在循环体**内**，所以每个被跳过的字节各自推进一次列号。
            //   把这两行提到循环外，会让 `x\0\0y` 的列号少算 2，而**不会报任何错**。
            self.col += u32::from(self.chw);
            if self.ch == Some('\n') {
                self.line += 1;
                self.col = 0;
            }

            if !self.read_step(&mut diags) {
                return diags;
            }
            // read_step 返回 true ⇒ 这个字节要被跳过（NUL / 非法 UTF-8 / BOM），重来一次
        }
    }

    /// 开始一个活动段。**段包含当前 `ch`**；`ch` 自身不计入 [`segment`](Self::segment)。
    pub fn start_segment(&mut self) {
        // 官方的 `s.b = s.r - s.chw` —— 用 `saturating_sub` 是为了让"刚开始还没读过字符"
        // （`r = chw = 0`）也算 0 而不是下溢。
        self.b = Some(self.r.saturating_sub(usize::from(self.chw)));
    }

    /// 结束活动段。
    pub fn stop_segment(&mut self) {
        self.b = None;
    }

    /// 活动段的原始字节：`buf[b : r-chw]` —— **不含当前 `ch`**。
    ///
    /// # Panics
    ///
    /// 没有活动段时 panic。**刻意不返回空切片**：静默返回 `&[]` 会让"空标识符"这种
    /// 不可能的东西看起来正常，进而让调用方在错误的假设上继续跑。
    #[must_use]
    pub fn segment(&self) -> &[u8] {
        let b = self
            .b
            .expect("segment() 要求先调用 start_segment()（活动段）");
        &self.buf[b..self.r - usize::from(self.chw)]
    }

    /// 回退到活动段的起点，并重新读出段首字符。
    ///
    /// **只为 `..` 服务**（LEX-06 的最长匹配）：`.` 分支要前瞻一个字符才能知道
    /// 是 `.` 还是 `..`，判断完必须把游标退回去。这是全阶段**唯一**的回退点。
    ///
    /// # 前置条件
    ///
    /// 活动段内**不得含换行**（官方 `source.go:97-102` 明写）——
    /// 否则位置信息就是错的。
    ///
    /// # Panics
    ///
    /// 没有活动段时 panic。
    #[must_use]
    pub fn rewind(&mut self) -> Vec<Diag> {
        let b = self.b.expect("rewind() 要求有活动段（且段内不得含换行）");

        // 位置直接算出终值：段首字符的列 = b - 行首字节。
        //
        // 官方写的是 `col -= (r - b)` 然后紧跟一次 `nextch()` 的 `col += chw` ——
        // 两步的**中间值在 `b - chw < 行首` 时为负**，靠 Go 的 `uint` 回绕兜住。
        // Rust 的 `u32` 减法在 debug 下会 panic，所以这里把两步合并成一次减法：
        //     col_old - ((r - chw) - b) == b - 行首
        // 与官方终值相同，但**不经过一个负的中间值**。
        self.col -= ((self.r - usize::from(self.chw)) - b) as u32;
        self.r = b;

        // ★ 官方 `rewind` 内部调的是**完整**的 `nextch()`，而 `nextch` 会按
        //   **前一个字符**判断换行。于是 `..` 后面紧跟 `\n` 时，这次回退会把
        //   行号**多加一次**，而且**后续所有位置都跟着偏移**。
        //
        //   实测（oracle，go1.27.1）：
        //     `..\n`       ⇒ `.`@1:1、`.`@**2:2**、EOF@3:1
        //     `x\n..\ny\n` ⇒ `.`@2:1、`.`@**3:2**、`y`@**4:1**
        //
        //   这个规则**可以精确写下来**（不像首字节 NUL 那种依赖缓冲填充时机的产物），
        //   所以照抄 —— 否则文件中 `..` 之后的每个位置都会与 oracle 差一行。
        //   顺序也要照官方：先加 `chw`（已折进上面那次减法），**再**判换行。
        if self.ch == Some('\n') {
            self.line += 1;
            self.col = 0;
        }

        let mut diags = Vec::new();
        // 只重读字符，**不再动 (line, col)** —— 位置已经置成段首。
        let again = self.read_step(&mut diags);
        debug_assert!(
            !again,
            "段首是需要跳过的字节（NUL / 非法 UTF-8 / BOM）：这会超出 rewind 的前置条件。\
             rewind 只为 `..` 服务，段首必是 '.'。"
        );
        diags
    }

    /// 读 `r` 处的一个字符，更新 `ch` / `chw` / `r` —— **不碰 `(line, col)`**。
    ///
    /// 返回 `true` 表示"这个字节需要被跳过"（NUL / 非法 UTF-8 / 文件中部 BOM），
    /// 调用方要重新走一次位置推进再读 —— 对应官方 `nextch` 里的 `goto redo`。
    fn read_step(&mut self, diags: &mut Vec<Diag>) -> bool {
        if self.r >= self.buf.len() {
            // EOF：`chw = 0` 让位置不再前进（官方 `s.ch = -1; s.chw = 0`）
            self.ch = None;
            self.chw = 0;
            return false;
        }

        let first = self.buf[self.r];

        // ASCII 快路径（照官方：先看 `ch < sentinel`）
        if first < 0x80 {
            self.r += 1;
            self.chw = 1;
            self.ch = Some(char::from(first));
            if first == 0 {
                diags.push(self.diag(DiagCode::InvalidNul, "源码里出现 NUL 字符"));
                return true;
            }
            return false;
        }

        match decode_at(&self.buf, self.r) {
            Ok((c, w)) => {
                self.r += usize::from(w);
                self.chw = w;
                self.ch = Some(c);
                if c == '\u{FEFF}' {
                    // BOM 只允许出现在文件首字符
                    if self.line > 0 || self.col > 0 {
                        diags.push(self.diag(DiagCode::BomMidFile, "文件中部出现 BOM"));
                    }
                    // 无论报不报错，**BOM 一律不算字符**（官方 `goto redo`）——
                    // 它的字节留在段里，所以 `x\ufeffy` 的标识符原文含 BOM（实测 F8）
                    return true;
                }
                false
            }
            Err(()) => {
                // 非法编码：官方 `utf8.DecodeRune` 返回 `(RuneError, 1)` ⇒ **只消费 1 字节**
                self.r += 1;
                self.chw = 1;
                self.ch = Some('\u{FFFD}');
                diags.push(self.diag(DiagCode::InvalidUtf8, "非法的 UTF-8 编码"));
                true
            }
        }
    }

    /// 在当前 `ch` 的位置造一条诊断 —— 对应官方 `s.error(msg)`（报在 `s.pos()`）。
    ///
    /// 位置为什么正好是"被污染的那个字节"：`read_step` 读出的违规字节**就是**当前 `ch`，
    /// 而 `(line, col)` 已在本次循环的顶部按它推进过。
    fn diag(&self, code: DiagCode, message: &str) -> Diag {
        Diag {
            pos: self.pos(),
            code,
            message: message.to_string(),
        }
    }
}

/// 解出 `buf[r]` 处的一个字符，返回 `(字符, 字节宽度)`。
///
/// `Err(())` = **非法编码或截断**，语义与 Go 的 `utf8.DecodeRune` 一致：
/// 此时**只消费 1 个字节**（调用方负责），并报一条诊断。
///
/// 用"首字节定长度 + 对那一小段调 `str::from_utf8`"实现，
/// **不**对整个剩余缓冲调用 `from_utf8`（那是 O(n) 每个字符）。
fn decode_at(buf: &[u8], r: usize) -> Result<(char, u8), ()> {
    let b0 = buf[r];
    let len: usize = match b0 {
        // 0xC0 / 0xC1 是过长编码的开头，0x80..=0xBF 是孤立的续接字节，
        // 0xF5..=0xFF 超出 U+10FFFF —— 都非法
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => return Err(()),
    };
    if r + len > buf.len() {
        // 文件在序列中间就结束了 ⇒ 截断（Go 同样返回 RuneError, 1）
        return Err(());
    }
    match core::str::from_utf8(&buf[r..r + len]) {
        Ok(s) => {
            let mut it = s.chars();
            match it.next() {
                Some(c) if it.next().is_none() => Ok((c, len as u8)),
                _ => Err(()),
            }
        }
        // 过长编码 / 代理区 / 续接字节不对 —— `from_utf8` 自己会拒
        Err(_) => Err(()),
    }
}
