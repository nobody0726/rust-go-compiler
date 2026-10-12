//! `rune` 与 `escape` 的验收测试（T-L11；LEX-12 转义 / rune 字面量）。
//!
//! # 期望值全部来自 oracle
//!
//! 每条的**位置**都跑过 `tools/token-oracle`（go1.27.1）—— 转义这类"错在哪一位"的问题，
//! 靠读代码推很容易差一格（诊断分 `errorf`（报在**当前字符**）与 `errorAtf(0)`
//! （报在**词元起点**）两种，不看实测根本分不清哪条是哪条）。
//!
//! # 五处只有 oracle 才说得清的地方
//!
//! 1. **引号种类是转义白名单的一部分**：`escape(quote)` 只把**匹配的那个**引号算进
//!    简单转义 ⇒ `\'` 在 rune 里合法，而 `\"` 是 **UNKNOWN**（T-L12 的字符串里正好相反）。
//! 2. **`'\8'` 是 UNKNOWN、`'\18'` 才是 BAD-DIGIT** —— `8`/`9` 根本不在八进制的
//!    `case '0'..'7'` 列表里，直接落到 `default`（"未知转义"），而不是"非法数码"。
//! 3. **八进制必须写满 3 位**：`'\0'` / `'\07'` 都报 BAD-DIGIT ——
//!    官方按 `(n, base, max) = (3, 8, 255)` **定长**读，缺位时报在**闭引号**上。
//! 4. **两种 "一个字面量里字符数不对" 的报错位置不同**：`''` 的 EMPTY 报在**闭引号**
//!    （`errorf`）；`'ab'` 的 MULTI 报在**开引号**（`errorAtf(0)`）。
//! 5. **转义算"一个字符"**：`'\n\n'` / `'\\n'` 都是两个字符 ⇒ MULTI。

use rgoc_lex::{Lexer, LitKind, TokenKind};

/// 扫一个 rune 字面量，返回（种类，(行,列,代号) 列表）。
///
/// 诊断走 `take_diags()` + `into_sorted()`（C1 §4 的排序键是 `file→line→col→code`）。
fn scan(src: &[u8]) -> (TokenKind, Vec<(u32, u32, &'static str)>) {
    let mut lx = Lexer::new(src.to_vec(), "t.go");
    let kind = lx.scan_rune();
    let codes = lx
        .take_diags()
        .into_sorted()
        .iter()
        .map(|d| (d.pos.line, d.pos.col, d.code.as_str()))
        .collect();
    (kind, codes)
}

fn scan1(src: &str) -> (TokenKind, Vec<(u32, u32, &'static str)>) {
    scan(src.as_bytes())
}

/// `Literal { text, lit_kind, bad }` —— 载荷是**字节**（D-M1-12）。
fn lit(text: &str, k: LitKind, bad: bool) -> TokenKind {
    TokenKind::Literal {
        text: text.as_bytes().to_vec(),
        lit_kind: k,
        bad,
    }
}

fn ok_rune(s: &str) -> TokenKind {
    lit(s, LitKind::Rune, false)
}
fn bad_rune(s: &str) -> TokenKind {
    lit(s, LitKind::Rune, true)
}

// ── ① 简单转义：白名单与引号绑定 ─────────────────────────────────────

#[test]
fn 简单转义九个拼写() {
    // 七个字母转义 + `\\` + **匹配的引号**（这里是 `'`）
    for s in [
        r"'\a'", r"'\b'", r"'\f'", r"'\n'", r"'\r'", r"'\t'", r"'\v'", r"'\\'", r"'\''",
    ] {
        assert_eq!(scan1(s), (ok_rune(s), vec![]), "输入 {s:?}");
    }
}

#[test]
fn 引号种类是转义白名单的一部分() {
    // `\"` 在 **rune** 里不是合法转义（白名单里是 `'`）——
    // 官方 `escape(quote)` 的第一个 case 就是 `quote` 本身。
    assert_eq!(
        scan1(r#"'\"'"#),
        (bad_rune(r#"'\"'"#), vec![(1, 3, "LEX-ESCAPE-UNKNOWN")])
    );
}

// ── ② 数值转义：四种长度 ─────────────────────────────────────────────

#[test]
fn 数值转义的四种长度() {
    for s in [r"'\101'", r"'\x41'", r"'\u00e9'", r"'\U0001F600'"] {
        assert_eq!(scan1(s), (ok_rune(s), vec![]), "输入 {s:?}");
    }
}

#[test]
fn 数值转义位数不足报非法数字() {
    // 报在**第一个不是该进制数码的字符**上 —— 缺位时就是闭引号。
    let cases: &[(&str, u32)] = &[
        (r"'\x'", 4),   // `x` 后紧跟闭引号
        (r"'\x4g'", 5), // `g` 不是十六进制数码
        (r"'\u12'", 6), // 缺两位
        (r"'\18'", 4),  // 八进制里 `8` 越界
        (r"'\0'", 4),   // ★ 八进制**必须写满 3 位**（不是"能写多少写多少"）
        (r"'\07'", 5),
    ];
    for &(s, col) in cases {
        assert_eq!(
            scan1(s),
            (bad_rune(s), vec![(1, col, "LEX-ESCAPE-BAD-DIGIT")]),
            "输入 {s:?}"
        );
    }
}

#[test]
fn 八进制转义超过_255() {
    // 0o400 = 256、0o777 = 511：都超过 `max = 255`，报在**闭引号**上（循环读完之后）
    for s in [r"'\400'", r"'\777'"] {
        assert_eq!(
            scan1(s),
            (bad_rune(s), vec![(1, 6, "LEX-ESCAPE-OCTAL-OVER")]),
            "输入 {s:?}"
        );
    }
}

#[test]
fn 代理区与超出_max_rune() {
    let cases: &[(&str, u32)] = &[
        (r"'\ud800'", 8),      // 代理区下界
        (r"'\U0000DFFF'", 12), // 代理区上界（大 U 路径）
        (r"'\U00110000'", 12), // > MaxRune
    ];
    for &(s, col) in cases {
        assert_eq!(
            scan1(s),
            (bad_rune(s), vec![(1, col, "LEX-ESCAPE-RANGE")]),
            "输入 {s:?}"
        );
    }
}

#[test]
fn 未知转义() {
    let cases: &[(&str, u32)] = &[
        (r"'\q'", 3),
        // ★ `8` 不在八进制的 `case '0'..'7'` 里 ⇒ 落到 default ⇒ **UNKNOWN**，
        //   而不是"非法数码"（对照上面 `'\18'` 那条）
        (r"'\8'", 3),
    ];
    for &(s, col) in cases {
        assert_eq!(
            scan1(s),
            (bad_rune(s), vec![(1, col, "LEX-ESCAPE-UNKNOWN")]),
            "输入 {s:?}"
        );
    }
}

// ── ③ rune 的结构 ────────────────────────────────────────────────────

#[test]
fn rune_的结构错误() {
    // 空的报在**闭引号**（`errorf` = 当前字符）……
    assert_eq!(
        scan1("''"),
        (bad_rune("''"), vec![(1, 2, "LEX-RUNE-EMPTY")])
    );
    // ……多字符的报在**开引号**（`errorAtf(0)` = 词元起点）
    assert_eq!(
        scan1("'ab'"),
        (bad_rune("'ab'"), vec![(1, 1, "LEX-RUNE-MULTI")])
    );
    // 未终止（EOF）同样报在**开引号**；`'` 与 `'a` 走同一条出口（`T-L-53` 的两种例子）
    assert_eq!(
        scan1("'"),
        (bad_rune("'"), vec![(1, 1, "LEX-RUNE-UNTERMINATED")])
    );
    assert_eq!(
        scan1("'a"),
        (bad_rune("'a"), vec![(1, 1, "LEX-RUNE-UNTERMINATED")])
    );
    // 对照组：正常的一个字符
    assert_eq!(scan1("'a'"), (ok_rune("'a'"), vec![]));
}

#[test]
fn 转义算一个字符() {
    // 官方 `rune()` 里 `continue` 会走 `n++` ⇒ 转义序列**只算一个字符**。
    // 三条都是"两个字符" ⇒ MULTI。
    for s in [r"'\n\n'", r"'\1012'", r"'\\n'"] {
        assert_eq!(
            scan1(s),
            (bad_rune(s), vec![(1, 1, "LEX-RUNE-MULTI")]),
            "输入 {s:?}"
        );
    }
}

#[test]
fn 非_ascii_的_rune() {
    // 一个多字节字符算**一个**（不是按字节数）
    for s in ["'中'", "'😀'"] {
        assert_eq!(scan1(s), (ok_rune(s), vec![]), "输入 {s:?}");
    }
    assert_eq!(
        scan1("'中a'"),
        (bad_rune("'中a'"), vec![(1, 1, "LEX-RUNE-MULTI")])
    );
}

#[test]
fn 真实换行中断_rune_且不消耗换行() {
    // `'` + 真换行：报 RUNE-NEWLINE（在**换行自己**的位置），且**不消耗**它 ——
    // 换行留给分号层处理（官方 `break` 前没有 `nextch`）。
    let mut lx = Lexer::new(b"'\n".to_vec(), "t.go");
    assert_eq!(lx.scan_rune(), bad_rune("'"));
    let codes: Vec<_> = lx
        .take_diags()
        .into_sorted()
        .iter()
        .map(|d| (d.pos.line, d.pos.col, d.code.as_str()))
        .collect();
    assert_eq!(codes, vec![(1, 2, "LEX-RUNE-NEWLINE")]);
    assert_eq!(lx.peek(), Some('\n'), "换行必须原样留下");
}

#[test]
fn eof_在转义里只报未终止() {
    // `'\` + EOF：`escape` 撞到 EOF 时**不报错**（官方注释「complain in caller about EOF」），
    // 由 `rune()` 报"未终止" —— 所以**只有一条**诊断，且没有那条 J 型的尾巴。
    assert_eq!(
        scan1("'\\"),
        (bad_rune("'\\"), vec![(1, 1, "LEX-RUNE-UNTERMINATED")])
    );
}
