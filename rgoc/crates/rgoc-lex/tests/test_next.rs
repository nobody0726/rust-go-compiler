//! `Lexer::next` 的验收测试（T-L15；LEX-07 端到端 + RF4/RF5）。
//!
//! # 第一次「接线」
//!
//! 本测试把 T-L08–T-L14 的全部零件组装起来跑**整条流**：判据从「单函数行为」
//! 升级为「词元流 + 诊断流渲染后与 oracle golden **逐字节**相同」。
//!
//! # golden 的来源（全部 oracle 直出）
//!
//! `tests/golden/*.golden` 均为 `tools/token-oracle` 的输出**去掉头两行**
//! （`# golden:` / `# source:`）的正文。生成命令：
//!
//! ```sh
//! cd /work/tools/token-oracle
//! go run . fixtures/hello.txt | tail -n +3 > .../golden/hello.golden
//! for i in 1..7: go run . ../../go_source_code/test/syntax/semi$i.go | tail -n +3
//! for n in n01..n08: go run . fixtures/next/$n.txt | tail -n +3
//! ```
//!
//! 输入侧：`fixtures/next/n01..n08` 是 T-L15 的探针（与测试内联输入逐字节相同）；
//! `semi1..7` 是官方语料（`include_bytes!` 编译期嵌入 —— 语料只读、且变更会触发重编译）。
//!
//! # 四条只有端到端才验收得到的点
//!
//! - **RF5**：EOF 补出的 `Semi("EOF")` 与 `Eof` **同位于文件末尾**（不凭空造列号）；
//! - **RF4**：词元中间撞 EOF 的收尾顺序 —— `Literal(bad)` → `Semi("EOF")` → `Eof`；
//! - **LEX-07 第一处机关**：`'\n'` 只在 `nlsemi` 为假时被吞 —— `x\n\n\ny` 只有**第一个**
//!   换行产分号；
//! - **`default` 分支**：非法字符（`@`）报 `LEX-BAD-CHAR`、消耗、继续（且不搅乱 `nlsemi`）。

use rgoc_lex::{Diag, DiagCode, Lexer, SemiOrigin, Token, TokenKind, render};

/// 跑完整条流（含 `Eof`），返回（词元流，排序后的诊断）。
///
/// ⚠️ 循环上限 10_000 防的是「**每次都在前进、但产出过多**」的路径。
/// 若单次 `next()` **内部**不返回（接线缺陷，如非法字符分支忘了消耗），
/// 这个上限防不住 —— 那会表现为进程挂死或崩溃（反验 R32 记录过一次 SIGKILL 实例）。
/// 对后者的防线在**变异脚本**（限时执行）与**分派分支的逐一覆盖**（本文件的 n 系列）。
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

// ── ① hello：完整程序的全流 ─────────────────────────────────────────

#[test]
fn hello_程序的完整词元流() {
    // 输入与 fixtures/next/n09_hello.txt、tools/token-oracle/fixtures/hello.txt 同内容
    assert_golden!(
        b"package main\n\nfunc main() {\n\tprintln(\"hello\")\n}\n",
        "hello"
    );
}

// ── ② 分号的三个来源（T-L-40）───────────────────────────────────────

#[test]
fn 分号的三个来源() {
    assert_golden!(b"x;y\nz", "n01");

    let (toks, diags) = lex_all(b"x;y\nz");
    assert!(diags.is_empty());
    let semis: Vec<&Token> = toks
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Semi(_)))
        .collect();
    assert_eq!(semis.len(), 3, "显式 / 换行 / EOF 各一条");
    assert_eq!(semis[0].kind, TokenKind::Semi(SemiOrigin::Explicit));
    assert_eq!(semis[1].kind, TokenKind::Semi(SemiOrigin::Newline));
    assert_eq!(semis[2].kind, TokenKind::Semi(SemiOrigin::Eof));
    // 显式的在 `;` 处（1:2）；EOF 补的在文件末尾（2:2）
    assert_eq!((semis[0].pos.line, semis[0].pos.col), (1, 2));
    assert_eq!((semis[2].pos.line, semis[2].pos.col), (2, 2));
}

// ── ③ `)` 后补、`(` 后不补（T-L-41 / T-L-42）────────────────────────

#[test]
fn 右括号后补分号左括号后不补() {
    assert_golden!(b"f()\ng()", "n02");
    assert_golden!(b"f(\ng()", "n03");

    // 对照断言（失败信息比 golden diff 更直白）
    let (t2, _) = lex_all(b"f()\ng()");
    assert!(
        t2.iter()
            .any(|t| (t.pos.line, t.pos.col) == (1, 4)
                && t.kind == TokenKind::Semi(SemiOrigin::Newline)),
        "`)` 后换行必须补分号（1:4）"
    );
    let (t3, _) = lex_all(b"f(\ng()");
    assert!(
        !t3.iter()
            .any(|t| t.pos.line == 1 && matches!(t.kind, TokenKind::Semi(_))),
        "`(` 后换行不许补分号"
    );
}

// ── ④ EOF 补分号的位置（RF5 / T-L-48）───────────────────────────────

#[test]
fn eof_补分号的位置与_oracle_相同() {
    assert_golden!(b"x := 1", "n04");

    let (toks, _) = lex_all(b"x := 1");
    let n = toks.len();
    assert_eq!(toks[n - 2].kind, TokenKind::Semi(SemiOrigin::Eof));
    assert_eq!(toks[n - 1].kind, TokenKind::Eof);
    // ★ RF5：两个词元同位于 EOF 处（1:7 —— 最后一个字符之后），不凭空造列号
    assert_eq!((toks[n - 2].pos.line, toks[n - 2].pos.col), (1, 7));
    assert_eq!((toks[n - 1].pos.line, toks[n - 1].pos.col), (1, 7));
}

// ── ⑤ 词元中间撞 EOF 的收尾顺序（RF4）──────────────────────────────

#[test]
fn 词元中间遇到_eof_的收尾顺序() {
    assert_golden!(b"x := \"abc", "n05");

    let (toks, diags) = lex_all(b"x := \"abc");
    let kinds: Vec<&TokenKind> = toks.iter().map(|t| &t.kind).collect();
    assert_eq!(kinds.len(), 5, "x := <bad literal> ;EOF EOF");
    assert!(
        matches!(kinds[2], TokenKind::Literal { bad: true, .. }),
        "未终止的字符串照常产出 Literal（bad=true）"
    );
    assert_eq!(*kinds[3], TokenKind::Semi(SemiOrigin::Eof));
    assert_eq!(*kinds[4], TokenKind::Eof);
    // 诊断（先于收尾进 bag）；报在字符串起点
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, DiagCode::StringUnterminated);
    assert_eq!((diags[0].pos.line, diags[0].pos.col), (1, 6));
}

// ── ⑥ 语料 semi1..7 逐词元与 oracle 相等（T-L-47）───────────────────

#[test]
fn 语料_semi1_至_semi7_逐词元与_oracle_相等() {
    assert_golden!(
        include_bytes!("../../../../go_source_code/test/syntax/semi1.go"),
        "semi1"
    );
    assert_golden!(
        include_bytes!("../../../../go_source_code/test/syntax/semi2.go"),
        "semi2"
    );
    assert_golden!(
        include_bytes!("../../../../go_source_code/test/syntax/semi3.go"),
        "semi3"
    );
    assert_golden!(
        include_bytes!("../../../../go_source_code/test/syntax/semi4.go"),
        "semi4"
    );
    assert_golden!(
        include_bytes!("../../../../go_source_code/test/syntax/semi5.go"),
        "semi5"
    );
    assert_golden!(
        include_bytes!("../../../../go_source_code/test/syntax/semi6.go"),
        "semi6"
    );
    assert_golden!(
        include_bytes!("../../../../go_source_code/test/syntax/semi7.go"),
        "semi7"
    );
}

// ── ⑦ LEX-07 第一处机关：空白只吞「nlsemi 为假」时的换行 ────────────

#[test]
fn 空白跳过只吞_nlsemi_为假时的换行() {
    assert_golden!(b"x\n\n\ny\n", "n06");

    let (toks, _) = lex_all(b"x\n\n\ny\n");
    let semis: Vec<&Token> = toks
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Semi(_)))
        .collect();
    // 只有第 1 个换行产分号（1:2）与 y 后的换行（4:2）；末尾 `\n` 之后 EOF 不补
    assert_eq!(semis.len(), 2);
    assert_eq!((semis[0].pos.line, semis[0].pos.col), (1, 2));
    assert_eq!((semis[1].pos.line, semis[1].pos.col), (4, 2));
    // y 在第 4 行 —— 中间两个换行**被吞**（不产分号、也不推进成别的词元）
    assert!(
        toks.iter()
            .any(|t| t.kind == TokenKind::Ident(b"y".to_vec()) && t.pos.line == 4),
        "被吞的换行不改变后续位置"
    );
}

// ── ⑧ 非法字符：报错、消耗、继续（`default` 分支）───────────────────

#[test]
fn 非法字符报错并继续() {
    assert_golden!(b"x @ y", "n07");

    let (toks, diags) = lex_all(b"x @ y");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, DiagCode::BadChar);
    assert_eq!((diags[0].pos.line, diags[0].pos.col), (1, 3));
    // `@` 被跳过；`y` 照常成为词元；`x` 的 nlsemi 置位没有被搅乱 ⇒ EOF 补分号
    assert!(
        toks.iter()
            .any(|t| t.kind == TokenKind::Ident(b"y".to_vec()) && t.pos.col == 5)
    );
    assert_eq!(
        toks[toks.len() - 2].kind,
        TokenKind::Semi(SemiOrigin::Eof),
        "EOF 处仍补分号"
    );
}

// ── ⑨ 注释与分号的端到端（nlsemi 跨注释保持）───────────────────────

#[test]
fn 行注释后的换行仍产分号() {
    assert_golden!(b"x// c\ny", "n08");

    let (toks, _) = lex_all(b"x// c\ny");
    let semi = toks
        .iter()
        .find(|t| matches!(t.kind, TokenKind::Semi(SemiOrigin::Newline)))
        .expect("注释后的换行必须产分号");
    assert_eq!(
        (semi.pos.line, semi.pos.col),
        (1, 6),
        "Semi 的位置是换行符（注释整体已被跳过）"
    );
    // y 在下一行
    assert!(
        toks.iter()
            .any(|t| t.kind == TokenKind::Ident(b"y".to_vec()) && t.pos.line == 2)
    );
}

// ── ⑩ 补盲区（渲染与接线的形态覆盖，n10..n17）──────────────────────
//
// 这一组是**为反向验证补的**：没有它们，以下几个变异「红不了」——
// 跨行块注释的 Semi 位置、`.5` 的交接、`opop`/`*` 的渲染拼写、
// `\r` 的跳过、`\xNN` 转义、rune/raw 的分派。

#[test]
fn 跨行块注释在流里的分号位置是注释起点() {
    // n10：`x /* a\nb */ y\n` —— Semi("newline") 在 **1:3**（注释起点），
    // 不是换行处、也不是注释结束处（F41 的端到端形态）
    assert_golden!(b"x /* a\nb */ y\n", "n10");

    let (toks, _) = lex_all(b"x /* a\nb */ y\n");
    let semi = toks
        .iter()
        .find(|t| matches!(t.kind, TokenKind::Semi(SemiOrigin::Newline)))
        .expect("跨行块注释 + nlsemi 必须产分号");
    assert_eq!((semi.pos.line, semi.pos.col), (1, 3));
}

#[test]
fn 点号起头的数字与普通点号() {
    // n11：`.5` 是**一个** Float 字面量（LeadingDotNumber 交接的端到端）
    assert_golden!(b"x := .5", "n11");
    let (t, _) = lex_all(b"x := .5");
    assert!(matches!(
        &t[2].kind,
        TokenKind::Literal {
            lit_kind: rgoc_lex::LitKind::Float,
            bad: false,
            ..
        }
    ));

    // n17：`x.y` 的 `.` 是普通 Delim（不是数字交接）
    assert_golden!(b"x.y", "n17");
    let (t2, _) = lex_all(b"x.y");
    assert!(
        t2.iter()
            .any(|x| x.kind == TokenKind::Delim(rgoc_lex::Delim::Dot))
    );
}

#[test]
fn 自增自减的拼写与置位() {
    // n12：`i++` / `j--` —— name 列是官方 `_IncOp` 的 "opop"，`op=` 区分 +/−；
    // 且 `++`/`--` 置位 nlsemi ⇒ 换行产出分号
    assert_golden!(b"i++\nj--", "n12");

    let (t, _) = lex_all(b"i++\nj--");
    assert!(
        t.iter()
            .any(|x| x.kind == TokenKind::Semi(SemiOrigin::Newline) && x.pos.line == 1),
        "`++` 后的换行必须产分号"
    );
}

#[test]
fn 星号渲染为专用拼写() {
    // n13：`a * b` —— `*` 在官方是 `_Star`（name 列 "*"，不是 "op"）。
    // 这条 golden 专门盯住 `case '*'` 的 `don't goto assignop`。
    assert_golden!(b"a * b", "n13");
}

#[test]
fn 空白跳过里的_cr_与非法字节载荷() {
    // n14：`x\r\ny` —— `\r` 无条件当空白跳过（Semi 落在 1:3 = `\n` 处）
    assert_golden!(b"x\r\ny", "n14");
    let (t, _) = lex_all(b"x\r\ny");
    assert!(
        t.iter()
            .any(|x| x.kind == TokenKind::Semi(SemiOrigin::Newline) && x.pos.col == 3)
    );

    // n15：`x\xff y` —— 非法字节留在词元原文里（渲染成 `\xNN`）、诊断在它自己的列
    assert_golden!(b"x\xff y", "n15");
    let (t2, d2) = lex_all(b"x\xff y");
    assert_eq!(d2[0].code, DiagCode::InvalidUtf8);
    assert!(
        t2.iter()
            .any(|x| x.kind == TokenKind::Ident(b"x\xff".to_vec()))
    );
}

#[test]
fn rune_与原始串的分派() {
    // n16：`'x'`（kind=3 Rune）与 `` `y` ``（kind=4 String，原始型）
    assert_golden!(b"a := 'x'\nb := `y`\n", "n16");
}
