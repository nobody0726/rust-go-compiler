//! T-L16 的验收测试 —— 错误恢复（N1–N11）与同位置诊断排序（T-L-54）。
//!
//! # 双向断言
//!
//! `M1-tests.md` §4 要求每条负例**同时**断言两件事：
//!
//! 1. **诊断** —— 位置 + 代号逐条明确（失败信息比 golden diff 直白）；
//! 2. **报错之后的词元流** —— 与 oracle golden **逐字节**相等
//!    （`assert_golden!`，与 `test_next.rs` 同一装置）。
//!
//! 「报错但继续」的恢复纪律在这里获得**端到端**验收：畸形输入不只是"报个错完事"，
//! 而是照常产出词元（畸形字面量 `bad=true`、含非法字节的标识符原文不变），
//! 且**后续词元不受污染**。
//!
//! # 三条只有实测挡得住的「反常」
//!
//! - **N1**：`1\xff2` 是**一个** `Literal`（含非法字节），且 `bad` **仍是 false**（F31）
//!   —— 非法字节由字符层报出、不经数字语法，所以 `bad` 不变真；
//! - **N2/N3/N4**：NUL / BOM / 私有区字符被**吸收进标识符原文**（段切片是字节区间，
//!   违规字节留在段里）—— 「相同前缀的两种行为」猜不出来，只能问 oracle；
//! - **T-L-54**：`0x\` 的两条诊断**同位置、不同代号**，且**输出顺序与发生时序相反**
//!   （时序 L→B，C1 §4 按 code 排序后 B→L）—— 这是「排序键真的被应用」的证据；
//!   若不排序，golden diff 会直接显形。
//!
//! # golden 的来源
//!
//! `tests/golden/n*.golden` / `t54_*.golden` 均为 `tools/token-oracle` 输出
//! **去掉头两行**的正文，对应探针 `fixtures/recovery/*.txt`（逐字节相同）。生成命令：
//!
//! ```sh
//! cd /work/tools/token-oracle
//! go run . fixtures/recovery/n1b_four_lines.txt | tail -n +3 > .../golden/n1_utf8.golden
//! （其余同为 go run . fixtures/recovery/<探针>.txt | tail -n +3）
//! ```

use rgoc_lex::{Diag, DiagCode, Lexer, LitKind, SemiOrigin, Token, TokenKind, render};

/// 跑完整条流（含 `Eof`），返回（词元流，排序后的诊断）。
fn lex_all(src: &[u8]) -> (Vec<Token>, Vec<Diag>) {
    let mut lx = Lexer::new(src.to_vec(), "t.go");
    let mut toks = Vec::new();
    for _ in 0..10_000 {
        let t = lx.next();
        let last = matches!(t.kind, TokenKind::Eof);
        toks.push(t);
        if last {
            return (toks, lx.take_diags().into_sorted());
        }
    }
    panic!("next() 在 10000 步内没有产出 Eof —— 疑似不前进的循环");
}

/// 渲染成 golden 正文（与 oracle 输出去掉头两行后逐字节可比）。
fn golden(src: &[u8]) -> String {
    let (toks, diags) = lex_all(src);
    render::golden_body(&toks, &diags)
}

/// golden 文件断言（`include_str!`：编译期嵌入 + golden 变更触发重编译）。
macro_rules! assert_golden {
    ($src:expr, $name:literal) => {
        assert_eq!(
            golden($src),
            include_str!(concat!("golden/", $name, ".golden")),
            "{} 与 oracle golden 不一致",
            $name
        )
    };
}

/// 诊断代号序列。
fn codes(ds: &[Diag]) -> Vec<DiagCode> {
    ds.iter().map(|d| d.code).collect()
}

/// 诊断位置序列。
fn locs(ds: &[Diag]) -> Vec<(u32, u32)> {
    ds.iter().map(|d| (d.pos.line, d.pos.col)).collect()
}

/// 取出 `(line, col)` 处的字面量并展开三元组 `(原文, 子类, bad)`。
fn literal_at(toks: &[Token], line: u32, col: u32) -> (&[u8], LitKind, bool) {
    let t = toks
        .iter()
        .find(|t| t.pos.line == line && t.pos.col == col)
        .unwrap_or_else(|| panic!("({line},{col}) 处没有词元"));
    match &t.kind {
        TokenKind::Literal {
            text,
            lit_kind,
            bad,
        } => (text, *lit_kind, *bad),
        other => panic!("({line},{col}) 应是 Literal，实得 {other:?}"),
    }
}

/// 断言诊断流已按 C1 §4 的 `(line, col, code)` 非降序 —— 「输出唯一」的独立重算。
fn assert_sorted(src: &[u8], ds: &[Diag]) {
    for w in ds.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let ka = (a.pos.line, a.pos.col, a.code.as_str());
        let kb = (b.pos.line, b.pos.col, b.code.as_str());
        assert!(
            ka <= kb,
            "{src:?}：诊断未按 (line,col,code) 排序：{ka:?} 后跟 {kb:?}"
        );
    }
}

// ── N1：非法 UTF-8（`T-L-50`/`T-L-51` 的主样本）────────────────────────

#[test]
fn n1_非法_utf8_诊断与后续词元() {
    // 输入 = 探针 fixtures/recovery/n1b_four_lines.txt（文档 §4 的 @4:8 出自此四行版）
    let src = b"package main\n\nfunc f() {\n\tx := 1\xff2\n}\n";
    let (toks, diags) = lex_all(src);

    // ① 诊断
    assert_eq!(codes(&diags), [DiagCode::InvalidUtf8]);
    assert_eq!(locs(&diags), [(4, 8)]);

    // ② 后续词元与 oracle 相等
    assert_golden!(src, "n1_utf8");

    // ③ 反常点显式钉住：非法字节留在字面量原文里，而 `bad` 仍是 false
    //    （非法 UTF-8 由字符层报出、不经数字语法 —— F31；「顺手置 bad」会破坏它）
    let (text, kind, bad) = literal_at(&toks, 4, 7);
    assert_eq!(text, b"1\xff2");
    assert_eq!(kind, LitKind::Int);
    assert!(!bad, "字符层的错误不使字面量语法变坏 ⇒ bad 必须保持 false");

    // 报错之后 `}` 与换行分号照常产出（恢复不破坏后续）
    assert!(
        toks.iter()
            .any(|t| t.kind == TokenKind::Delim(rgoc_lex::Delim::RBrace) && t.pos.line == 5),
        "报错之后的 `}}` 必须照常产出"
    );
}

// ── N2–N4：被**吸收进标识符**的违规字符 ────────────────────────────────

#[test]
fn n2_nul_被吸收进标识符() {
    let src = b"x\x00y\n";
    let (toks, diags) = lex_all(src);

    assert_eq!(codes(&diags), [DiagCode::InvalidNul]);
    assert_eq!(locs(&diags), [(1, 2)]);
    assert_golden!(src, "n2_nul");

    // 违规字节留在段里 ⇒ 整个 `x\0y` 是一个标识符（不是切两个）
    assert_eq!(toks[0].kind, TokenKind::Ident(b"x\x00y".to_vec()));
    assert_eq!((toks[0].pos.line, toks[0].pos.col), (1, 1));
}

#[test]
fn n3_文件中部_bom() {
    let src = b"x\xef\xbb\xbfy\n";
    let (toks, diags) = lex_all(src);

    assert_eq!(codes(&diags), [DiagCode::BomMidFile]);
    assert_eq!(locs(&diags), [(1, 2)]);
    assert_golden!(src, "n3_bom");

    // BOM 报错但「不算字符」（goto redo），字节留在段里 ⇒ 原文含三字节 BOM
    assert_eq!(toks[0].kind, TokenKind::Ident(b"x\xef\xbb\xbfy".to_vec()));
}

#[test]
fn n4_私有区字符() {
    let src = b"x\xee\x80\x80y\n";
    let (toks, diags) = lex_all(src);

    assert_eq!(codes(&diags), [DiagCode::BadIdentChar]);
    assert_eq!(locs(&diags), [(1, 2)]);
    assert_golden!(src, "n4_pua");

    // 官方 atIdentChar 第三分支「报错但吸收」⇒ 也是一个标识符（F29 的同形）
    assert_eq!(toks[0].kind, TokenKind::Ident(b"x\xee\x80\x80y".to_vec()));
}

// ── N5–N7：畸形字面量照常产出（`bad=true`）────────────────────────────

#[test]
fn n5_前缀后无数字() {
    let src = b"0x\n";
    let (toks, diags) = lex_all(src);

    assert_eq!(codes(&diags), [DiagCode::LitNoDigits]);
    assert_eq!(locs(&diags), [(1, 3)]);
    assert_golden!(src, "n5_nodigits");

    assert_eq!(literal_at(&toks, 1, 1), (&b"0x"[..], LitKind::Int, true));
    // 后续：换行补分号
    assert!(
        toks.iter()
            .any(|t| matches!(t.kind, TokenKind::Semi(SemiOrigin::Newline)))
    );
}

#[test]
fn n6_下划线未分隔() {
    let src = b"1__2\n";
    let (toks, diags) = lex_all(src);

    assert_eq!(codes(&diags), [DiagCode::BadSep]);
    assert_eq!(locs(&diags), [(1, 3)]);
    assert_golden!(src, "n6_badsep");

    assert_eq!(literal_at(&toks, 1, 1), (&b"1__2"[..], LitKind::Int, true));
}

#[test]
fn n7_rune_多字符() {
    let src = b"'ab'\n";
    let (toks, diags) = lex_all(src);

    assert_eq!(codes(&diags), [DiagCode::RuneMulti]);
    assert_eq!(locs(&diags), [(1, 1)]);
    assert_golden!(src, "n7_runemulti");

    assert_eq!(literal_at(&toks, 1, 1), (&b"'ab'"[..], LitKind::Rune, true));
}

// ── N8/N8b：未终止（`T-L-53`）+ 收尾顺序（RF4/RF5）───────────────────

#[test]
fn n8_字符串未终止到_eof() {
    let src = b"\"abc";
    let (toks, diags) = lex_all(src);

    assert_eq!(codes(&diags), [DiagCode::StringUnterminated]);
    assert_eq!(locs(&diags), [(1, 1)]);
    assert_golden!(src, "n8_strunterm");

    assert_eq!(
        literal_at(&toks, 1, 1),
        (&b"\"abc"[..], LitKind::String, true)
    );
    // RF4/RF5：收尾顺序 = 分号（EOF 处）→ Eof，且**同位置**（不凭空造列号）
    let semi = toks
        .iter()
        .find(|t| matches!(t.kind, TokenKind::Semi(SemiOrigin::Eof)))
        .expect("词元中间撞 EOF 必须补分号");
    assert_eq!((semi.pos.line, semi.pos.col), (1, 5));
    let eof = toks.last().unwrap();
    assert_eq!(eof.kind, TokenKind::Eof);
    assert_eq!(
        (eof.pos.line, eof.pos.col),
        (semi.pos.line, semi.pos.col),
        "RF5：分号与 Eof 同位于 EOF 处（不凭空造列号）"
    );
}

#[test]
fn n8b_rune_未终止到_eof() {
    let src = b"'a";
    let (toks, diags) = lex_all(src);

    assert_eq!(codes(&diags), [DiagCode::RuneUnterminated]);
    assert_eq!(locs(&diags), [(1, 1)]);
    assert_golden!(src, "n8b_runeunterm");

    assert_eq!(literal_at(&toks, 1, 1), (&b"'a"[..], LitKind::Rune, true));
    assert!(
        toks.iter()
            .any(|t| matches!(t.kind, TokenKind::Semi(SemiOrigin::Eof)))
    );
}

// ── N9：块注释未终止（`T-L-52`）──────────────────────────────────────

#[test]
fn n9_块注释未终止() {
    let src = b"/*";
    let (toks, diags) = lex_all(src);

    assert_eq!(codes(&diags), [DiagCode::CommentUnterminated]);
    assert_eq!(locs(&diags), [(1, 1)]);
    assert_golden!(src, "n9_commentunterm");

    // 「不中止」+「流中只有 Eof」：注释被整体跳过，**连分号都没有**
    // （首词元之前 nlsemi=false，未终止也不产 Semi）
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].kind, TokenKind::Eof);
    assert_eq!((toks[0].pos.line, toks[0].pos.col), (1, 3));
}

// ── N10/N11：数字与转义的深层错误 ────────────────────────────────────

#[test]
fn n10_十六进制浮点缺指数() {
    let src = b"0x1.8\n";
    let (toks, diags) = lex_all(src);

    assert_eq!(codes(&diags), [DiagCode::BadExponent]);
    assert_eq!(locs(&diags), [(1, 6)]);
    assert_golden!(src, "n10_badexp");

    assert_eq!(
        literal_at(&toks, 1, 1),
        (&b"0x1.8"[..], LitKind::Float, true)
    );
}

#[test]
fn n11_未知转义() {
    let src = b"\"\\q\"\n";
    let (toks, diags) = lex_all(src);

    assert_eq!(codes(&diags), [DiagCode::EscapeUnknown]);
    assert_eq!(locs(&diags), [(1, 3)]);
    assert_golden!(src, "n11_escape");

    assert_eq!(
        literal_at(&toks, 1, 1),
        (&b"\"\\q\""[..], LitKind::String, true)
    );
    // 后续：换行补分号 + Eof
    assert!(
        toks.iter()
            .any(|t| matches!(t.kind, TokenKind::Semi(SemiOrigin::Newline)))
    );
}

// ── T-L-54：同位置多条诊断的排序（输出唯一）──────────────────────────

#[test]
fn t54_同位置双诊断按_code_排序_与时序相反() {
    // `0x\` + 换行：
    //   发生时序（errh 回调）—— `LIT-NO-DIGITS`（数字扫描收尾）先，`BAD-CHAR`（回到主循环）后；
    //   C1 §4 排序（line→col→code 字符串）—— `LEX-BAD-CHAR` **在前**（B < L）。
    // ⇒ 排序后的输出顺序与发生时序**相反**。若实现不排序（或渲染不排序），
    //   本测试的 golden diff 会直接显形 —— 这是排序键真的被应用的最强证据。
    let src = b"0x\\\n";
    let (toks, diags) = lex_all(src);

    assert_eq!(codes(&diags), [DiagCode::BadChar, DiagCode::LitNoDigits]);
    assert_eq!(locs(&diags), [(1, 3), (1, 3)], "两条诊断必须同位置");
    assert_golden!(src, "t54_reversed");

    assert_eq!(literal_at(&toks, 1, 1), (&b"0x"[..], LitKind::Int, true));
}

#[test]
fn t54_同位置双诊断序不变仍排序() {
    // `"\` + 换行：`ESCAPE-UNKNOWN` 与 `STRING-NEWLINE` 同在 1:3。
    // 这一对的字典序与发生时序**相同**（E < S）—— 作为对照，
    // 防「为了让上一条过而把排序方向搞反」。
    let src = b"\"\\\n";
    let (toks, diags) = lex_all(src);

    assert_eq!(
        codes(&diags),
        [DiagCode::EscapeUnknown, DiagCode::StringNewline]
    );
    assert_eq!(locs(&diags), [(1, 3), (1, 3)]);
    assert_golden!(src, "t54_same");

    assert_eq!(
        literal_at(&toks, 1, 1),
        (&b"\"\\"[..], LitKind::String, true)
    );
}

#[test]
fn 全部负例的诊断流按_line_col_code_有序() {
    // 「输出唯一」的独立重算：不依赖 golden 的字节比对，
    // 直接对排序键做非降序检查（C1 §4：file→line→col→code）。
    const NEG_CASES: &[&[u8]] = &[
        b"package main\n\nfunc f() {\n\tx := 1\xff2\n}\n", // n1
        b"x\x00y\n",                                       // n2
        b"x\xef\xbb\xbfy\n",                               // n3
        b"x\xee\x80\x80y\n",                               // n4
        b"0x\n",                                           // n5
        b"1__2\n",                                         // n6
        b"'ab'\n",                                         // n7
        b"\"abc",                                          // n8
        b"'a",                                             // n8b
        b"/*",                                             // n9
        b"0x1.8\n",                                        // n10
        b"\"\\q\"\n",                                      // n11
        b"0x\\\n",                                         // t54 reversed
        b"\"\\\n",                                         // t54 same
    ];
    for src in NEG_CASES {
        let (_, diags) = lex_all(src);
        assert_sorted(src, &diags);
    }
}
