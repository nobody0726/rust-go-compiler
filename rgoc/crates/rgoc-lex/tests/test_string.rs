//! `std_string` 与 `raw_string` 的验收测试（T-L12；LEX-13）。
//!
//! # 期望值全部来自 oracle
//!
//! 每条的**位置**都跑过 `tools/token-oracle`（go1.27.1）。
//!
//! # 三处只有实测才说得清的地方
//!
//! 1. **`stdString` 的换行/EOF 分支没有 `if ok` 守卫，而 `rune()` 有** ——
//!    所以 `"\q` + 换行报 **两条**诊断（UNKNOWN + STRING-NEWLINE），
//!    而 `'\q` + 换行只报一条。**"顺手统一"成同一个形状会少一条诊断**
//!    （少一条诊断比词元错更难发现 —— F29 那次的同形）。
//! 2. **原始串的 `\r` 留在文本里**（F39）—— 官方注释原文：「We leave CRs in the
//!    string since they are part of the literal (even though they are not part of
//!    the literal value)」。⇒ "丢弃"是**值**层面的事（M3 的 unquote），M1 只管文本。
//!    ⚠️ 计划简报里写的「`\r` 被丢弃（不进入载荷）」**是错的**。
//! 3. **`\'` 在解释型字符串里是 UNKNOWN**（F36 的另一半：转义白名单与引号种类绑定）。

use rgoc_lex::{Lexer, LitKind, TokenKind};

fn diags(lx: &mut Lexer) -> Vec<(u32, u32, &'static str)> {
    lx.take_diags()
        .into_sorted()
        .iter()
        .map(|d| (d.pos.line, d.pos.col, d.code.as_str()))
        .collect()
}

/// 扫一个解释型字符串（游标从开引号开始），返回（种类，诊断列表）。
fn std(src: &[u8]) -> (TokenKind, Vec<(u32, u32, &'static str)>) {
    let mut lx = Lexer::new(src.to_vec(), "t.go");
    let kind = lx.scan_std_string();
    (kind, diags(&mut lx))
}

/// 扫一个原始字符串（游标从开反引号开始）。
fn raw(src: &[u8]) -> (TokenKind, Vec<(u32, u32, &'static str)>) {
    let mut lx = Lexer::new(src.to_vec(), "t.go");
    let kind = lx.scan_raw_string();
    (kind, diags(&mut lx))
}

/// 字符串词元的期望值 —— 载荷是**字节**、含两端定界符、**转义未解码**（D-M1-12）。
fn s(text: &str, bad: bool) -> TokenKind {
    TokenKind::Literal {
        text: text.as_bytes().to_vec(),
        lit_kind: LitKind::String,
        bad,
    }
}

// ── ① 解释型：转义 ──────────────────────────────────────────────────

#[test]
fn 解释型支持全部转义() {
    for src in [
        r#""a\tb""#,
        r#""a\"b""#,
        r#""a\\b""#,
        r#""\n""#,
        r#""\x41""#,
        r#""\101""#,
        r#""""#,
    ] {
        assert_eq!(std(src.as_bytes()), (s(src, false), vec![]), "输入 {src:?}");
    }
}

#[test]
fn 单引号在解释型字符串里不是合法转义() {
    // F36 的另一半：白名单与引号种类绑定 ⇒ 字符串里合法的是 `\"`、不是 `\'`
    let src = r#""\'""#;
    assert_eq!(
        std(src.as_bytes()),
        (s(src, true), vec![(1, 3, "LEX-ESCAPE-UNKNOWN")])
    );
}

// ── ② 解释型：换行 / EOF / 报告纪律 ─────────────────────────────────

#[test]
fn 解释型遇换行报错且不消耗换行() {
    let mut lx = Lexer::new(b"\"abc\n".to_vec(), "t.go");
    assert_eq!(lx.scan_std_string(), s("\"abc", true));
    assert_eq!(diags(&mut lx), vec![(1, 5, "LEX-STRING-NEWLINE")]);
    assert_eq!(lx.peek(), Some('\n'), "换行必须原样留下（留给分号层）");
}

#[test]
fn 未知转义之后遇换行要报两条诊断() {
    // ★ 官方 `stdString()` 的换行分支**没有 `if ok` 守卫**（`rune()` 有）——
    // 一个词元上两条诊断。若"顺手统一"成 rune 那样带守卫，这里会**少一条**。
    let mut lx = Lexer::new(b"\"\\q\ndef\"\n".to_vec(), "t.go");
    assert_eq!(lx.scan_std_string(), s("\"\\q", true));
    assert_eq!(
        diags(&mut lx),
        vec![(1, 3, "LEX-ESCAPE-UNKNOWN"), (1, 4, "LEX-STRING-NEWLINE"),]
    );
}

#[test]
fn 解释型未终止的三个入口() {
    // ① 直接撞 EOF；② 只有一个开引号；③ 转义（吃掉了 `"`）之后撞 EOF
    for src in ["\"abc", "\"", "\"a\\\""] {
        assert_eq!(
            std(src.as_bytes()),
            (s(src, true), vec![(1, 1, "LEX-STRING-UNTERMINATED")]),
            "输入 {src:?}"
        );
    }
}

#[test]
fn 转义里撞_eof_只报未终止() {
    // `"\` + EOF：`escape` 撞 EOF 时**不报错**（complain in caller），
    // 由 `stdString` 报"未终止" —— 所以**只有一条**诊断。
    assert_eq!(
        std(b"\"\\"),
        (s("\"\\", true), vec![(1, 1, "LEX-STRING-UNTERMINATED")])
    );
}

// ── ③ 原始型 ────────────────────────────────────────────────────────

#[test]
fn 原始字符串不转义() {
    // 载荷里是**反斜杠 + t 两个字面字符**（不是制表符）
    let src = "`a\\tb`";
    let (k, d) = raw(src.as_bytes());
    assert_eq!(d, vec![]);
    assert_eq!(k, s(src, false));
    if let TokenKind::Literal { text, .. } = &k {
        assert_eq!(text, b"`a\\tb`", "反斜杠与 t 必须原样保留");
    }
}

#[test]
fn 原始字符串可跨行() {
    // 换行不是错误，且**在文本里**；行号必须跟着涨
    let src = "`a\nb`";
    let mut lx = Lexer::new(src.as_bytes().to_vec(), "t.go");
    assert_eq!(lx.scan_raw_string(), s(src, false));
    assert!(diags(&mut lx).is_empty());
    assert_eq!(
        (lx.pos().line, lx.pos().col),
        (2, 3),
        "多行原始串之后的行列必须与 oracle 一致"
    );
}

#[test]
fn 原始字符串保留_cr() {
    // ★ F39：官方注释「We leave CRs in the string since they are part of the literal
    //   (even though they are not part of the literal value)」⇒ **文本保留 `\r`**；
    //   "丢弃"是**值**层面的事（M3），M1 只管文本。
    let src = "`a\rb`";
    let (k, d) = raw(src.as_bytes());
    assert_eq!(d, vec![], "CR 不是错误");
    assert_eq!(k, s(src, false));
    if let TokenKind::Literal { text, .. } = &k {
        assert!(text.contains(&b'\r'), "载荷必须含 0x0D 字节：{text:?}");
    }
    // 尾随 CR（紧贴闭反引号）同样保留
    let src2 = "`a\r`";
    let (k2, _) = raw(src2.as_bytes());
    assert_eq!(k2, s(src2, false));
}

#[test]
fn 原始字符串_crlf() {
    // CRLF：`\r` 与 `\n` **都**在文本里；`\n` 照常推进行号
    let src = "`a\r\nb`";
    let mut lx = Lexer::new(src.as_bytes().to_vec(), "t.go");
    assert_eq!(lx.scan_raw_string(), s(src, false));
    assert!(diags(&mut lx).is_empty());
    assert_eq!((lx.pos().line, lx.pos().col), (2, 3));
}

#[test]
fn 原始字符串未终止() {
    assert_eq!(
        raw(b"`abc"),
        (s("`abc", true), vec![(1, 1, "LEX-STRING-UNTERMINATED")])
    );
}

#[test]
fn 空串与含引号的原始串() {
    // 空原始串、含 `"` 与 `'` 的原始串：都是合法载荷
    for src in ["``", "`a\"b'c`"] {
        assert_eq!(raw(src.as_bytes()), (s(src, false), vec![]), "输入 {src:?}");
    }
}
