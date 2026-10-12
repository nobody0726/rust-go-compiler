//! `lex` 子命令 —— 对单个文件跑词法器，输出与 oracle adapter **同构**的三节文本
//! （`T-L18`）。
//!
//! # 与 oracle 的对齐口径（腿乙 `T-L19` 的前提）
//!
//! 输出 = 头两行 + [`render::golden_body_with_messages`] 的正文，与
//! `tools/token-oracle` 的输出**逐字节同构**，**只有一处刻意不同**：
//!
//! | 行 | oracle | 本命令 | 说明 |
//! |---|---|---|---|
//! | `# golden: <path>` | 传入路径**原样** | 同左 | 两边约定传同一个路径字符串 |
//! | `# source: …` | `oracle` | **`rgoc`** | 来源必须**如实**标注（§5.2 的纪律） |
//!
//! 腿乙 diff 时归一 `# source:` 这一行（或剥头两行比正文）—— 其余必须逐字节相同。
//!
//! # 渲染不在本模块
//!
//! 正文渲染是 `rgoc-lex::render`（R30：「与 adapter 可 diff 的格式在 Rust 侧
//! 只能有一处实现」）。本模块只做三件事：读入（经 `Lexer::from_path` 的
//! 资源上限检查）、驱动全流、拼头两行。
//!
//! # 退出码语义
//!
//! - 成功（含**超限**）⇒ `Ok` ⇒ 0。超限是**词法层的拒绝**（`LEX-EXCEEDS-FILE-LIMIT`
//!   + 只含 `Eof` 的流，`T-L17` 验收过）——它是**正常输出**，不是错误；
//! - io 失败（不存在 / 不可读）⇒ `Err` 上浮 ⇒ `main_entry` 统一 stderr + 2。

use std::fmt::Write as _;
use std::path::Path;

use rgoc_lex::{Lexer, Mode, Token, TokenKind, render};

use crate::cli::LexMode;

/// 跑 `lex`：读文件 → 全流 → 三节文本（含头两行）。返回值即要打印的内容。
pub fn run_lex(file: &Path, mode: LexMode) -> Result<String, String> {
    let lex_mode = match mode {
        LexMode::None => Mode::NONE,
        LexMode::Comments => Mode::COMMENTS,
        LexMode::Directives => Mode::DIRECTIVES,
    };

    let mut lx =
        Lexer::from_path(file, lex_mode).map_err(|e| format!("读 {} 失败：{e}", file.display()))?;

    // 全流。**不设人为上限**：词元流的有限性是 `Lexer` 的契约（每个 `next()`
    // 前进或返回 `Eof`），由 `rgoc-lex` 的测试（含 10_000 步上限的守护）保证。
    let mut tokens: Vec<Token> = Vec::new();
    loop {
        let t = lx.next();
        let last = matches!(t.kind, TokenKind::Eof);
        tokens.push(t);
        if last {
            break;
        }
    }
    let diags = lx.take_diags().into_sorted();
    let messages = lx.take_messages();

    // 是否输出 messages 节**看 mode**（不看 messages 是否为空）—— 与 oracle 的
    // `if scanMode != 0` 同构：空节也照出。
    let msgs = if matches!(mode, LexMode::None) {
        None
    } else {
        Some(messages.as_slice())
    };

    let mut out = String::new();
    let _ = writeln!(out, "# golden: {}", file.display());
    let _ = writeln!(out, "# source: rgoc");
    out.push_str(&render::golden_body_with_messages(&tokens, &diags, msgs));
    Ok(out)
}
