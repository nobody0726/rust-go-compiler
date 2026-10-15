//! 渲染器**三节**的验收测试（`T-L18` 立）。
//!
//! # 与 golden 家族的分工
//!
//! 前两节（`== tokens ==` / `== diags ==`）由 38 个 golden 间接覆盖
//! （`test_next` / `test_recovery` / `test_limits` 全部经 [`render::golden_body`] 对撞）。
//! 本文件补的是**第三节 `== messages ==`** —— 它是 `lex --mode` 与 oracle
//! `--mode` 对齐的部分（腿乙 `T-L19` 要 diff 它），此前没有任何直接验收。
//!
//! # 期望值来源（oracle）
//!
//! 与探针 `tools/token-oracle/fixtures/driver/d01_comments.txt` **逐字节相同**的输入，
//! `go run . --mode comments` 的输出（剥头两行）内联在下。
//!
//! # 一条只有这里能验的性质
//!
//! **排序由渲染器负责**：`take_messages()` 给的是发生顺序，而 oracle 在渲染前
//! 按 `(line, col, text)`（字节序）排序 —— 这是格式的一部分。本文件用**乱序的
//! 手工 messages** 直接钉住渲染器的排序行为（词法测试拿不到乱序数据）。

use rgoc_lex::{FileId, Lexer, Message, MessageKind, Mode, Pos, Token, TokenKind, render};

/// 跑全流（含 `Eof`）。
fn lex_all(src: &[u8], mode: Mode) -> (Vec<Token>, Vec<rgoc_lex::Diag>, Vec<Message>) {
    let mut lx = Lexer::new(src.to_vec(), "t.go");
    lx.set_mode(mode);
    let mut toks = Vec::new();
    for _ in 0..10_000 {
        let t = lx.next();
        let last = matches!(t.kind, TokenKind::Eof);
        toks.push(t);
        if last {
            break;
        }
    }
    let diags = lx.take_diags().into_sorted();
    let msgs = lx.take_messages();
    (toks, diags, msgs)
}

fn p(file: u32, line: u32, col: u32) -> Pos {
    Pos {
        base: None, // 无 `//line` 基准
        file: FileId::from_index(file),
        line,
        col,
    }
}

fn msg(line: u32, col: u32, text: &[u8]) -> Message {
    Message {
        pos: p(0, line, col),
        kind: MessageKind::Comment,
        text: text.to_vec(),
    }
}

#[test]
fn comments_模式渲染三节_与_oracle_一致() {
    // 输入 = fixtures/driver/d01_comments.txt（`// c\nx\n`），期望正文取自
    // `go run . --mode comments fixtures/driver/d01_comments.txt`（剥头两行）。
    let (toks, diags, msgs) = lex_all(b"// c\nx\n", Mode::COMMENTS);
    assert_eq!(msgs.len(), 1, "注释必须上报");
    assert_eq!(msgs[0].text, b"// c".to_vec());

    let got = render::golden_body_with_messages(&toks, &diags, Some(&msgs));
    let want = concat!(
        "== tokens ==\n",
        "  2:1    name     lit=\"x\"\n",
        "  2:2    ;        lit=\"newline\"\n",
        "  3:1    EOF      lit=-\n",
        "== diags ==\n",
        "== messages ==\n",
        "  1:1    \"// c\"\n",
    );
    assert_eq!(got, want);
}

#[test]
fn directives_模式渲染指令文本() {
    // 输入 = fixtures/driver/d02_directives.txt，期望同其 oracle 输出（剥头两行）。
    let (toks, diags, msgs) = lex_all(b"//go:build linux\n//line a.go:1\nx\n", Mode::DIRECTIVES);
    assert_eq!(msgs.len(), 2, "两条指令都上报");

    let got = render::golden_body_with_messages(&toks, &diags, Some(&msgs));
    let want = concat!(
        "== tokens ==\n",
        "  3:1    name     lit=\"x\"\n",
        "  3:2    ;        lit=\"newline\"\n",
        "  4:1    EOF      lit=-\n",
        "== diags ==\n",
        "== messages ==\n",
        "  1:1    \"//go:build linux\"\n",
        "  2:1    \"//line a.go:1\"\n",
    );
    assert_eq!(got, want);
}

#[test]
fn messages_为_none_时与两节渲染逐字节相同() {
    // `None` 分支必须**恰好**退化成 golden_body —— 否则默认模式的 38 个 golden
    // 会在有人「顺手统一」两条路径时无声漂移。
    let (toks, diags, _) = lex_all(b"x\n", Mode::NONE);
    assert_eq!(
        render::golden_body_with_messages(&toks, &diags, None),
        render::golden_body(&toks, &diags)
    );
}

#[test]
fn messages_节按_line_col_text_排序_由渲染器负责() {
    // 乱序输入 ⇒ 输出有序（`(line, col, text)`，text 走**字节序**——与 Go 的
    // 字符串比较一致）。`sort_by` 是稳定排序：同位置同文本的相对次序保持。
    let (toks, diags, _) = lex_all(b"", Mode::NONE);
    let scrambled = [
        msg(2, 1, b"// c"),
        msg(1, 9, b"// b"),
        msg(1, 1, b"// z"),
        msg(1, 1, b"// a"), // 同位置：按 text 字节序，`// a` < `// z`
    ];

    let got = render::golden_body_with_messages(&toks, &diags, Some(&scrambled));
    let want = concat!(
        "== tokens ==\n",
        "  1:1    EOF      lit=-\n",
        "== diags ==\n",
        "== messages ==\n",
        "  1:1    \"// a\"\n",
        "  1:1    \"// z\"\n",
        "  1:9    \"// b\"\n",
        "  2:1    \"// c\"\n",
    );
    assert_eq!(got, want);
}

#[test]
fn messages_转义走_quote_规则() {
    // 注释文本可含任意字节（F42：`\r`、非法 UTF-8 都在）——渲染必须走 quote
    // 的纯字节级转义（`\r` → `\\r`、`0xFF` → `\\xff`），而不是 Rust 的 `{:?}`。
    let (toks, diags, _) = lex_all(b"", Mode::NONE);
    let m = [msg(1, 1, b"// a\r\xffb")];

    let got = render::golden_body_with_messages(&toks, &diags, Some(&m));
    assert!(
        got.contains("1:1    \"// a\\r\\xffb\""),
        "必须按 quote 规则转义：{got}"
    );
}
