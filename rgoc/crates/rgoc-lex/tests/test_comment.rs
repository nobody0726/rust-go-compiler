//! `scan_comment` 的验收测试（T-L13；LEX-03 行注释/块注释 + D-M1-1 两种 mode）。
//!
//! # 期望值全部来自 oracle
//!
//! 每条断言的输入都与探针 `tools/token-oracle/fixtures/comment/pXX_*.txt`
//! **逐字节相同**（go1.27.1）；探针文件名写在断言旁的注释里。
//!
//! # 三处只有实测才说得清的地方
//!
//! 1. **上报位置与 `Semi` 位置都是「注释起点」**（p24：`Semi@1:2`，而注释体
//!    从 1:2 一直铺到 2:4）—— 官方 `case '*'` 里 `s.line/s.col` 是 token 起点、
//!    检查跨行时全程未被更新（`scanner.go:240-246`）。
//! 2. **`directives` 模式不做行首检查**（p08/p22/p23：行中间的 `//line` 照样上报）。
//!    「`//line` 必须在行首」是 **parser** `updateBase()` 的规则（F11），
//!    scanner 层没有这条 —— 计划简报里「校验 `//line` 必须在行首」是跨层混淆，
//!    以实测为准（执行期裁决 R25）。
//! 3. **注释文本保留原始字节**：`\r` 留在文本里（p10，官方 TestComments 的
//!    `"// want this one\r"` 同款）、非法 UTF-8 也留着（p14）——
//!    ⇒ `Message::text` 必须是 `Vec<u8>`，`String` 装不下非法字节。
//!
//! # 未终止块注释的两种行为（p29/p30/p06）
//!
//! 报 `LEX-COMMENT-UNTERMINATED`（报在**注释起点**）且**不上报 message**；
//! 但「跨行 + `nlsemi`」的 `Semi` 判定在 `fullComment()` **返回之后**，
//! 与它成没成功无关 ⇒ 未终止的跨行块注释**仍产 `Semi`**（p06 实测）。
//!
//! # 判据是「参数响应」，不是重建 `nlsemi`
//!
//! `scan_comment` 的 `nlsemi` 是**显式参数**（由 `T-L14`/`T-L15` 提供）。
//! 测试直接以参数形式驱动「nlsemi 真 / 假」两个分支；`nlsemi` 从哪些词元来
//! 属于 `SemicolonPolicy` 的规则表（T-L14 逐行断言）。

use rgoc_lex::{
    CommentScan, CommentStyle, FileId, Lexer, Message, MessageKind, Mode, Pos, PunctScan, TokenKind,
};

fn p(line: u32, col: u32) -> Pos {
    Pos {
        file: FileId::from_index(0),
        line,
        col,
    }
}

/// 扫一条**从文件头开始**的注释。走真实入口链：`scan_punct` 消耗 `//`/`/*`
/// （带回 style，游标停在注释体首字符），再交给 `scan_comment`。
///
/// `start`（注释起点）由**调用方在 `scan_punct` 之前**记下 —— 与将来 `next()`
/// 在重扫循环开头记 token 起点的方式一致（官方是 `s.line/s.col` 字段）。
fn scan_comment_at(src: &[u8], mode: Mode, nlsemi: bool) -> (Pos, CommentScan, Lexer) {
    let mut lx = Lexer::new(src.to_vec(), "t.go");
    lx.set_mode(mode);
    let start = lx.pos();
    let style = match lx.scan_punct() {
        Some(PunctScan::Comment(s)) => s,
        other => panic!("{src:?} 开头必须是注释，实得 {other:?}"),
    };
    let r = lx.scan_comment(start, style, nlsemi);
    (start, r, lx)
}

fn msgs(lx: &mut Lexer) -> Vec<Message> {
    lx.take_messages()
}

fn diags(lx: &mut Lexer) -> Vec<(u32, u32, &'static str)> {
    lx.take_diags()
        .into_sorted()
        .iter()
        .map(|d| (d.pos.line, d.pos.col, d.code.as_str()))
        .collect()
}

fn msg(line: u32, col: u32, kind: MessageKind, text: &[u8]) -> Message {
    Message {
        pos: p(line, col),
        kind,
        text: text.to_vec(),
    }
}

// ── ① 三种模式的上报范围 ────────────────────────────────────────────

#[test]
fn 默认模式不上报任何注释() {
    // p25/p26 在**默认模式**下运行：输出里没有 `== messages ==` 节。
    let (_, _, mut lx) = scan_comment_at(b"// c\n", Mode::NONE, false);
    assert!(msgs(&mut lx).is_empty(), "行注释");
    let (_, _, mut lx) = scan_comment_at(b"/* c */ y\n", Mode::NONE, false);
    assert!(msgs(&mut lx).is_empty(), "块注释");
}

#[test]
fn comments_模式上报全部注释() {
    // p25：`// c\n`
    let (_, _, mut lx) = scan_comment_at(b"// c\n", Mode::COMMENTS, false);
    assert_eq!(
        msgs(&mut lx),
        vec![msg(1, 1, MessageKind::Comment, b"// c")]
    );

    // p26：`/* c */ y\n` —— 块注释也是「起点 + 全文（含定界符）」
    let (_, _, mut lx) = scan_comment_at(b"/* c */ y\n", Mode::COMMENTS, false);
    assert_eq!(
        msgs(&mut lx),
        vec![msg(1, 1, MessageKind::Comment, b"/* c */")]
    );

    // p16：`// hi`（无换行直抵 EOF）—— 文本完整、不含换行
    let (_, _, mut lx) = scan_comment_at(b"// hi", Mode::COMMENTS, false);
    assert_eq!(
        msgs(&mut lx),
        vec![msg(1, 1, MessageKind::Comment, b"// hi")]
    );

    // p27：空注释 `//\n` —— 文本就是两个斜杠（官方 TestComments `{"//", "//"}` 同款）
    let (_, _, mut lx) = scan_comment_at(b"//\n", Mode::COMMENTS, false);
    assert_eq!(msgs(&mut lx), vec![msg(1, 1, MessageKind::Comment, b"//")]);
}

#[test]
fn directives_模式只上报_go_与_line() {
    // p35…p42/p45 的独立版矩阵（`directives` 模式）。
    for (src, want) in [
        (&b"//line\n"[..], None),
        (b"//line foo\n", Some(&b"//line foo"[..])),
        (b"//linea foo\n", None),
        (b"//gofx\n", None),
        (b"//go:foo\n", Some(&b"//go:foo"[..])),
        (b"// go:\n", None),
        (b"/*line a.go:1 */\n", Some(&b"/*line a.go:1 */"[..])),
        (b"/* line */\n", None),
        // ★ p45：块注释**不认** `go:`（官方 `fullComment` 只匹配 `"line "`）——
        // 行注释认 `go:`、块注释不认，这个不对称只有逐字读源码/实测才分得清
        (b"/*go:build */\n", None),
    ] {
        let (_, _, mut lx) = scan_comment_at(src, Mode::DIRECTIVES, false);
        let got = msgs(&mut lx);
        match want {
            Some(text) => assert_eq!(
                got,
                vec![msg(1, 1, MessageKind::Directive, text)],
                "输入 {src:?} 应当上报"
            ),
            None => assert!(got.is_empty(), "输入 {src:?} 不应上报"),
        }
    }
}

#[test]
fn comments_位优先于_directives_位() {
    // 官方两个函数都是 `if s.mode&comments != 0` 先判（scanner.go:743/793）——
    // 两个位同时设时等价于 comments（判据 S：源码语义，不依赖 oracle）。
    let both = Mode::COMMENTS.union(Mode::DIRECTIVES);
    let (_, _, mut lx) = scan_comment_at(b"// hi\n", both, false);
    assert_eq!(
        msgs(&mut lx),
        vec![msg(1, 1, MessageKind::Comment, b"// hi")],
        "非指令文本在组合模式下也要上报（comments 行为）"
    );
}

// ── ② 行注释不吃换行（分号逻辑的输入）──────────────────────────────

#[test]
fn 行注释不吃换行() {
    // p02：`x// c\ny` —— oracle 在 1:6（换行符）产出 `Semi("newline")`，
    // 前提就是行注释**停在 `\n` 之前**、把换行留给了分号层。
    let mut lx = Lexer::new(b"x// c\ny".to_vec(), "t.go");
    lx.set_mode(Mode::COMMENTS);
    assert_eq!(lx.scan_ident(), Some(TokenKind::Ident(b"x".to_vec())));
    let start = lx.pos();
    assert_eq!(start, p(1, 2));
    assert_eq!(
        lx.scan_punct(),
        Some(PunctScan::Comment(CommentStyle::Line))
    );
    assert_eq!(
        lx.scan_comment(start, CommentStyle::Line, false),
        CommentScan::Redo
    );
    assert_eq!(lx.peek(), Some('\n'), "行注释必须停在换行前（留给分号层）");
    assert_eq!(
        msgs(&mut lx),
        vec![msg(1, 2, MessageKind::Comment, b"// c")]
    );

    // 行注释在 nlsemi 两个值下行为一致：都不产 Semi（它是「行尾」语义）
    for nlsemi in [false, true] {
        let (_, r, _) = scan_comment_at(b"// c\n", Mode::NONE, nlsemi);
        assert_eq!(r, CommentScan::Redo, "nlsemi={nlsemi}");
    }
}

// ── ③ 跨行块注释：即「跨行」又看 nlsemi ─────────────────────────────

#[test]
fn 跨行块注释在_nlsemi_为真时等价于换行() {
    // p24：`x/* a\nb */ y\n` —— 注释跨行 + nlsemi（来自 `x`）
    // ⇒ oracle 在 **1:2**（注释起点，不是注释之后）产出 `Semi("newline")`。
    let mut lx = Lexer::new(b"x/* a\nb */ y\n".to_vec(), "t.go");
    lx.set_mode(Mode::COMMENTS);
    assert_eq!(lx.scan_ident(), Some(TokenKind::Ident(b"x".to_vec())));
    let start = lx.pos();
    assert_eq!(
        lx.scan_punct(),
        Some(PunctScan::Comment(CommentStyle::Block))
    );
    assert_eq!(
        lx.scan_comment(start, CommentStyle::Block, true),
        CommentScan::Semi
    );
    assert_eq!(
        msgs(&mut lx),
        vec![msg(1, 2, MessageKind::Comment, b"/* a\nb */")]
    );
    assert_eq!(lx.pos(), p(2, 5), "注释之后游标应停在 `y` 前的空格上");

    // 对照（p26）：**单行**块注释即使 nlsemi=true 也不产 Semi ——
    // 判定是「跨行 && nlsemi」两个条件的合取。
    let (_, r, _) = scan_comment_at(b"/* c */ y\n", Mode::COMMENTS, true);
    assert_eq!(r, CommentScan::Redo);
}

#[test]
fn 跨行块注释在_nlsemi_为假时不产出分号() {
    // ★ RF2：p11 `f(/* a\nb */x)\n` —— `(` 之后 nlsemi 为假，
    // 跨行块注释**不得**补分号（oracle 的 tokens 里 f ( x ) 紧挨着）。
    let mut lx = Lexer::new(b"f(/* a\nb */x)\n".to_vec(), "t.go");
    lx.set_mode(Mode::COMMENTS);
    assert!(lx.scan_ident().is_some(), "f");
    assert!(matches!(lx.scan_punct(), Some(PunctScan::Token(_))), "(");
    let start = lx.pos();
    assert_eq!(
        lx.scan_punct(),
        Some(PunctScan::Comment(CommentStyle::Block))
    );
    assert_eq!(
        lx.scan_comment(start, CommentStyle::Block, false),
        CommentScan::Redo
    );
    assert_eq!(
        msgs(&mut lx),
        vec![msg(1, 3, MessageKind::Comment, b"/* a\nb */")]
    );

    // 对照：**同一个形状**只把 nlsemi 换成 true ⇒ Semi 出现 ——
    // 证明差别只在参数（p03/p24 的规则），不是输入。
    let (_, r, _) = scan_comment_at(b"/* a\nb */x\n", Mode::COMMENTS, true);
    assert_eq!(r, CommentScan::Semi);
}

// ── ④ 指令：不做行首检查（订正简报的跨层混淆）──────────────────────

#[test]
fn 行首之外的_line_仍是指令() {
    // ★ p22：`x//line a.go:1\n` —— 注释在**第 2 列**，directives 模式照样上报。
    // 官方 scanner 的 `lineComment()` 只匹配前缀、**不看列号**；
    // 「行首」规则属于 parser 的 `updateBase()`（F11），M1 不实现。
    let mut lx = Lexer::new(b"x//line a.go:1\n".to_vec(), "t.go");
    lx.set_mode(Mode::DIRECTIVES);
    assert!(lx.scan_ident().is_some(), "x");
    let start = lx.pos();
    assert_eq!(
        lx.scan_punct(),
        Some(PunctScan::Comment(CommentStyle::Line))
    );
    assert_eq!(
        lx.scan_comment(start, CommentStyle::Line, false),
        CommentScan::Redo
    );
    assert_eq!(
        msgs(&mut lx),
        vec![msg(1, 2, MessageKind::Directive, b"//line a.go:1")]
    );

    // ★ p23：块注释形式同理（`/*line a.go:1 */` 在第 2 列）
    let mut lx = Lexer::new(b"x/*line a.go:1 */ y\n".to_vec(), "t.go");
    lx.set_mode(Mode::DIRECTIVES);
    assert!(lx.scan_ident().is_some(), "x");
    let start = lx.pos();
    assert_eq!(
        lx.scan_punct(),
        Some(PunctScan::Comment(CommentStyle::Block))
    );
    assert_eq!(
        lx.scan_comment(start, CommentStyle::Block, false),
        CommentScan::Redo
    );
    assert_eq!(
        msgs(&mut lx),
        vec![msg(1, 2, MessageKind::Directive, b"/*line a.go:1 */")]
    );
}

// ── ⑤ 未终止与控制细节 ──────────────────────────────────────────────

#[test]
fn 未终止的块注释报错但不中止() {
    // p29：`/* c`（EOF）—— 诊断报在**注释起点**、不上报 message
    let (_, r, mut lx) = scan_comment_at(b"/* c", Mode::COMMENTS, false);
    assert_eq!(r, CommentScan::Redo);
    assert_eq!(diags(&mut lx), vec![(1, 1, "LEX-COMMENT-UNTERMINATED")]);
    assert!(msgs(&mut lx).is_empty(), "未终止 ⇒ 不交出不完整的文本");
    assert_eq!(lx.peek(), None);

    // p30：`/* c\n` —— 诊断照报；跨行 ⇒ nlsemi=true 时**仍然**产 Semi
    // （判定在 fullComment() 返回之后，与成功与否无关；p06 实测 Semi@注释起点）
    let (_, r, mut lx) = scan_comment_at(b"/* c\n", Mode::COMMENTS, true);
    assert_eq!(r, CommentScan::Semi);
    assert_eq!(diags(&mut lx), vec![(1, 1, "LEX-COMMENT-UNTERMINATED")]);
    assert!(msgs(&mut lx).is_empty());

    // p33：`/*/` —— 未终止（官方 TestScanErrors 同款）
    let (_, _, mut lx) = scan_comment_at(b"/*/", Mode::COMMENTS, false);
    assert_eq!(diags(&mut lx), vec![(1, 1, "LEX-COMMENT-UNTERMINATED")]);

    // p32：`/**/` —— 立即闭合、文本是完整的 `/**/`
    let (_, _, mut lx) = scan_comment_at(b"/**/", Mode::COMMENTS, false);
    assert_eq!(
        msgs(&mut lx),
        vec![msg(1, 1, MessageKind::Comment, b"/**/")]
    );
    assert!(diags(&mut lx).is_empty());
}

#[test]
fn 块注释里的星号斜杠配对() {
    // p31：`/* * / */ x\n` —— `* /`（隔空格）不是结束；`*/` 才是。
    let (_, r, mut lx) = scan_comment_at(b"/* * / */ x\n", Mode::COMMENTS, false);
    assert_eq!(r, CommentScan::Redo);
    assert_eq!(
        msgs(&mut lx),
        vec![msg(1, 1, MessageKind::Comment, b"/* * / */")]
    );
    assert!(diags(&mut lx).is_empty());

    // p43：连续星号 `/* **/`（`**` 里的第一个 `*` 不是结束标记）
    let (_, _, mut lx) = scan_comment_at(b"/* **/", Mode::COMMENTS, false);
    assert_eq!(
        msgs(&mut lx),
        vec![msg(1, 1, MessageKind::Comment, b"/* **/")]
    );
    assert!(diags(&mut lx).is_empty());
}

// ── ⑥ 文本是原始字节（两个「装不下」的证据）─────────────────────────

#[test]
fn 注释文本保留_cr() {
    // p10 行 1：`// a\r\n` —— `\r` 留在文本里（官方 TestComments
    // 「`// want this one\r`」同款）；行注释只在 `\n` 前停。
    let (_, _, mut lx) = scan_comment_at(b"// a\r\n", Mode::COMMENTS, false);
    assert_eq!(
        msgs(&mut lx),
        vec![msg(1, 1, MessageKind::Comment, b"// a\r")]
    );

    // p44：`// b\r`（EOF 前是 CR）—— 同样保留
    let (_, _, mut lx) = scan_comment_at(b"// b\r", Mode::COMMENTS, false);
    assert_eq!(
        msgs(&mut lx),
        vec![msg(1, 1, MessageKind::Comment, b"// b\r")]
    );

    // directives 模式下含 CR 的普通注释不上报
    let (_, _, mut lx) = scan_comment_at(b"// a\r\n", Mode::DIRECTIVES, false);
    assert!(msgs(&mut lx).is_empty());
}

#[test]
fn 注释文本保留非法字节() {
    // p14：`// a\xffb\n` —— 非法字节**留在注释文本里**（`Vec<u8>` 的硬证据），
    // 同时字符层照常在它自己的位置报 LEX-INVALID-UTF8。
    let (_, _, mut lx) = scan_comment_at(b"// a\xffb\n", Mode::COMMENTS, false);
    assert_eq!(
        msgs(&mut lx),
        vec![msg(1, 1, MessageKind::Comment, b"// a\xffb")]
    );
    assert_eq!(diags(&mut lx), vec![(1, 5, "LEX-INVALID-UTF8")]);
}
