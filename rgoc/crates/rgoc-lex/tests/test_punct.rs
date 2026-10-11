//! `scan_punct` 的验收测试（T-L09；LEX-04 最长匹配 / LEX-06 `..` 回退 / F3 的 48 条）。
//!
//! # 这一层要盯住的三件事
//!
//! 1. **最长匹配**：`>>=` 是**一个**词元不是三个；`..` 是**两个** `.` 不是 `...`。
//! 2. **`..` 是全阶段唯一的回退点** —— 且回退**只影响本段**，不得让列号漂移（RF1）。
//! 3. **`+=` 一族收敛到同一段逻辑**（官方 `assignop` 汇合点）——
//!    11 个复合赋值运算符共用它，逐个分支手写必然漏一两个。
//!
//! # 期望值来自 oracle
//!
//! 每个非平凡用例都跑过 `tools/token-oracle`（go1.27.1）。其中一条是**只靠 oracle
//! 才发现**的：`..` 后面紧跟换行时，官方会把**行号多加一次**（见
//! `两点后紧跟换行_官方会多加一次行号`）。

use rgoc_lex::{Delim, FileId, Lexer, OpForm, Operator, Pos, PunctScan, SemiOrigin, TokenKind};

fn p(line: u32, col: u32) -> Pos {
    Pos {
        file: FileId::from_index(0),
        line,
        col,
    }
}

/// 在当前游标处扫一个标点，返回（**扫描前**的游标位置 = 词元起点，结果）。
fn scan(src: &[u8]) -> (Pos, Option<PunctScan>) {
    let mut lx = Lexer::new(src.to_vec(), "t.go");
    let at = lx.pos();
    (at, lx.scan_punct())
}

/// 只关心"是一个词元"的情形 —— 其余三种结果（没扫到 / 数字交接 / 注释）在这里算失败。
fn tok(src: &[u8]) -> Option<TokenKind> {
    match scan(src).1 {
        None => None,
        Some(PunctScan::Token(k)) => Some(k),
        Some(other) => panic!("{src:?} 扫出了非词元结果 {other:?}"),
    }
}

fn op(form: OpForm, operator: Operator) -> Option<TokenKind> {
    Some(TokenKind::Op {
        form,
        op: operator,
        prec: operator.prec(),
    })
}

fn plain(operator: Operator) -> Option<TokenKind> {
    op(OpForm::Plain, operator)
}

fn assign(operator: Operator) -> Option<TokenKind> {
    op(OpForm::Assign, operator)
}

// ── ① 48 条运算符与标点，逐条 ────────────────────────────────────────

/// spec `Operators and punctuation` 的 **48 条**（9+9+9+9+9+3），逐行照抄。
///
/// 顺序与 spec 的表格一致，便于与 `doc/go_spec.html` 对照。
fn spellings() -> Vec<(&'static str, TokenKind)> {
    use Delim::*;
    use OpForm::*;
    use Operator as O;
    let o = |form, op: O| TokenKind::Op {
        form,
        op,
        prec: op.prec(),
    };
    let d = |x| TokenKind::Delim(x);
    vec![
        // 行 1
        ("+", o(Plain, O::Add)),
        ("&", o(Plain, O::And)),
        ("+=", o(Assign, O::Add)),
        ("&=", o(Assign, O::And)),
        ("&&", o(Plain, O::AndAnd)),
        ("==", o(Plain, O::Eql)),
        ("!=", o(Plain, O::Neq)),
        ("(", d(LParen)),
        (")", d(RParen)),
        // 行 2
        ("-", o(Plain, O::Sub)),
        ("|", o(Plain, O::Or)),
        ("-=", o(Assign, O::Sub)),
        ("|=", o(Assign, O::Or)),
        ("||", o(Plain, O::OrOr)),
        ("<", o(Plain, O::Lss)),
        ("<=", o(Plain, O::Leq)),
        ("[", d(LBrack)),
        ("]", d(RBrack)),
        // 行 3
        ("*", o(Plain, O::Mul)),
        ("^", o(Plain, O::Xor)),
        ("*=", o(Assign, O::Mul)),
        ("^=", o(Assign, O::Xor)),
        ("<-", o(Plain, O::Arrow)),
        (">", o(Plain, O::Gtr)),
        (">=", o(Plain, O::Geq)),
        ("{", d(LBrace)),
        ("}", d(RBrace)),
        // 行 4
        ("/", o(Plain, O::Div)),
        ("<<", o(Plain, O::Shl)),
        ("/=", o(Assign, O::Div)),
        ("<<=", o(Assign, O::Shl)),
        ("++", o(IncDec, O::Add)),
        ("=", o(Plain, O::Assign)),
        (":=", o(Plain, O::Define)),
        (",", d(Comma)),
        (";", TokenKind::Semi(SemiOrigin::Explicit)),
        // 行 5
        ("%", o(Plain, O::Rem)),
        (">>", o(Plain, O::Shr)),
        ("%=", o(Assign, O::Rem)),
        (">>=", o(Assign, O::Shr)),
        ("--", o(IncDec, O::Sub)),
        ("!", o(Plain, O::Not)),
        ("...", d(Ellipsis)),
        (".", d(Dot)),
        (":", d(Colon)),
        // 行 6
        ("&^", o(Plain, O::AndNot)),
        ("&^=", o(Assign, O::AndNot)),
        ("~", o(Plain, O::Tilde)),
    ]
}

#[test]
fn 四十八条运算符与标点_逐条是单个词元() {
    let table = spellings();
    // 反向断言：条数必须与 spec 一致（漏一条会让"全部通过"变得毫无意义）
    assert_eq!(table.len(), 48, "spec 的运算符与标点表是 48 条（F3）");

    for (spelling, want) in table {
        let mut lx = Lexer::new(spelling.as_bytes().to_vec(), "t.go");
        let at = lx.pos();
        let got = lx.scan_punct();
        assert_eq!(got, Some(PunctScan::Token(want)), "{spelling} 的词元类别");
        assert_eq!(at, p(1, 1), "{spelling} 的起点");
        assert_eq!(lx.peek(), None, "{spelling} 必须**整个**被消耗（最长匹配）");
        assert!(lx.take_diags().is_empty(), "{spelling} 不该产生诊断");
    }
}

// ── ② 最长匹配：三处最容易写成"逐个 case"的地方 ────────────────────

#[test]
fn 加号三种写法给出三个不同的_kind() {
    // D-M1-10 的端到端版：官方靠**不同的 tok** 区分，我们靠 OpForm
    assert_eq!(tok(b"+"), plain(Operator::Add));
    assert_eq!(tok(b"+="), assign(Operator::Add));
    assert_eq!(tok(b"++"), op(OpForm::IncDec, Operator::Add));
    // `-` 同构
    assert_eq!(tok(b"-"), plain(Operator::Sub));
    assert_eq!(tok(b"-="), assign(Operator::Sub));
    assert_eq!(tok(b"--"), op(OpForm::IncDec, Operator::Sub));
}

#[test]
fn 复合赋值十一个都收敛到同一段逻辑() {
    // 官方用 `goto assignop` 汇合；逐个分支手写必然漏一两个
    for (s, o) in [
        (&b"+="[..], Operator::Add),
        (b"-=", Operator::Sub),
        (b"*=", Operator::Mul),
        (b"/=", Operator::Div),
        (b"%=", Operator::Rem),
        (b"&=", Operator::And),
        (b"|=", Operator::Or),
        (b"^=", Operator::Xor),
        (b"<<=", Operator::Shl),
        (b">>=", Operator::Shr),
        (b"&^=", Operator::AndNot),
    ] {
        assert_eq!(tok(s), assign(o), "{s:?}");
    }
    // 反向：`op=` 的形式**只**属于这 11 个 —— `==`/`<=`/`!=`/`>=` 是 `Plain`
    for s in [&b"=="[..], b"<=", b"!=", b">="] {
        assert!(
            matches!(
                tok(s),
                Some(TokenKind::Op {
                    form: OpForm::Plain,
                    ..
                })
            ),
            "{s:?} 应当是 Plain"
        );
    }
}

#[test]
fn 尖括号与竖线的最长匹配() {
    // `<<` / `<=` / `<-` / `<` 四条都在同一个分支里，最容易写错
    assert_eq!(tok(b"<"), plain(Operator::Lss));
    assert_eq!(tok(b"<="), plain(Operator::Leq));
    assert_eq!(tok(b"<<"), plain(Operator::Shl));
    assert_eq!(tok(b"<<="), assign(Operator::Shl));
    assert_eq!(tok(b"<-"), plain(Operator::Arrow));
    assert_eq!(tok(b">"), plain(Operator::Gtr));
    assert_eq!(tok(b">>"), plain(Operator::Shr));
    assert_eq!(tok(b">>="), assign(Operator::Shr));
    // `&` 一族：`&` / `&&` / `&^` / `&^=` / `&=` 五条
    assert_eq!(tok(b"&"), plain(Operator::And));
    assert_eq!(tok(b"&&"), plain(Operator::AndAnd));
    assert_eq!(tok(b"&^"), plain(Operator::AndNot));
    assert_eq!(tok(b"&^="), assign(Operator::AndNot));
    assert_eq!(tok(b"&="), assign(Operator::And));
    // `|` 一族
    assert_eq!(tok(b"|"), plain(Operator::Or));
    assert_eq!(tok(b"||"), plain(Operator::OrOr));
    assert_eq!(tok(b"|="), assign(Operator::Or));
}

#[test]
fn 冒号与等号族必须可区分() {
    // F21：`=` / `:=` / `<-` 官方**不设 op**，我们给语义值；三者与 `==` 必须能分开
    assert_eq!(tok(b"="), plain(Operator::Assign));
    assert_eq!(tok(b"=="), plain(Operator::Eql));
    assert_eq!(tok(b":="), plain(Operator::Define));
    assert_eq!(tok(b":"), Some(TokenKind::Delim(Delim::Colon)));
    assert_eq!(tok(b"!"), plain(Operator::Not));
    assert_eq!(tok(b"!="), plain(Operator::Neq));
    assert_eq!(tok(b"~"), plain(Operator::Tilde));
    // 这三个的优先级是 0（不参与二元）
    for k in [tok(b"="), tok(b":="), tok(b"<-"), tok(b"!"), tok(b"~")] {
        match k {
            Some(TokenKind::Op { prec, .. }) => assert_eq!(prec, 0),
            other => panic!("{other:?}"),
        }
    }
}

// ── ③ `.` 的三种长度 + 两条交接 ──────────────────────────────────────

#[test]
fn 点号的三种长度() {
    assert_eq!(tok(b"."), Some(TokenKind::Delim(Delim::Dot)));
    assert_eq!(tok(b"..."), Some(TokenKind::Delim(Delim::Ellipsis)));

    // `..` **不是** `...`：扫出**一个** `.`，第二个留给下一次
    // （oracle 实测 `..x` ⇒ `.`@1:1、`.`@1:2、`x`@1:3）
    let mut lx = Lexer::new(b"..x".to_vec(), "t.go");
    assert_eq!(lx.pos(), p(1, 1));
    assert_eq!(
        lx.scan_punct(),
        Some(PunctScan::Token(TokenKind::Delim(Delim::Dot)))
    );
    assert_eq!(lx.peek(), Some('.'), "第二个 `.` 必须留着");
    assert_eq!(lx.pos(), p(1, 2));
    assert_eq!(
        lx.scan_punct(),
        Some(PunctScan::Token(TokenKind::Delim(Delim::Dot)))
    );
    assert_eq!(lx.peek(), Some('x'));
    assert_eq!(lx.scan_ident(), Some(TokenKind::Ident(b"x".to_vec())));
}

#[test]
fn 回退之后紧跟多字节字符_位置不漂移() {
    // ★ RF1：`..中` —— 回退只影响本段，不能让列号漂移
    // oracle 实测：`.`@1:1、`.`@1:2、`中`@1:3（列按**字节**，`中` 占 3 列）
    let mut lx = Lexer::new("..中".as_bytes().to_vec(), "t.go");
    assert_eq!(lx.pos(), p(1, 1));
    assert_eq!(
        lx.scan_punct(),
        Some(PunctScan::Token(TokenKind::Delim(Delim::Dot)))
    );
    assert_eq!(lx.pos(), p(1, 2), "回退后第二个 `.` 仍在 1:2");
    assert_eq!(
        lx.scan_punct(),
        Some(PunctScan::Token(TokenKind::Delim(Delim::Dot)))
    );
    assert_eq!(lx.peek(), Some('中'), "多字节字符必须原封不动地留在游标上");
    assert_eq!(lx.pos(), p(1, 3), "`中` 的起点是 1:3");
    assert_eq!(
        lx.scan_ident(),
        Some(TokenKind::Ident("中".as_bytes().to_vec()))
    );
    assert!(lx.take_diags().is_empty());
}

#[test]
fn 两点后紧跟换行_官方会多加一次行号() {
    // ★ 这条是**只有靠 oracle 才看得出来**的官方行为：`rewind()` 内部调的是
    //   **完整**的 `nextch()`，而 `nextch` 会按**前一个字符**判断换行 ——
    //   于是 `..` 后面紧跟 `\n` 时，这次回退会把行号**多加一次**，
    //   且**后续所有位置都跟着偏移**。
    //
    // oracle 实测 `..\n`       ⇒ `.`@1:1、`.`@**2:2**、EOF@3:1
    // oracle 实测 `x\n..\ny\n` ⇒ `.`@2:1、`.`@**3:2**、`y`@**4:1**
    //
    // 这是**可精确写下来的规则**（不像 R11 那种依赖缓冲填充时机的产物），
    // 所以**照抄**：否则文件中 `..` 之后的每个位置都会与 oracle 差一行。
    let mut lx = Lexer::new(b"..\n".to_vec(), "t.go");
    assert_eq!(lx.pos(), p(1, 1));
    assert_eq!(
        lx.scan_punct(),
        Some(PunctScan::Token(TokenKind::Delim(Delim::Dot)))
    );
    assert_eq!(lx.pos(), p(2, 2), "行号被多加一次、列号停在第 2 列");
}

#[test]
fn 点后紧跟数字_交给数字扫描器且不消耗任何字符() {
    // 官方在 `.` 分支里：`if isDecimal(s.ch) { s.number(true); break }`。
    // `.5` 是**一个** Float 字面量（oracle 实测 `lit=".5" kind=1`）。
    //
    // 数字扫描器在 T-L10 —— 这里只断言**交接的状态**：游标退回 `.` 上、什么都没消耗。
    let mut lx = Lexer::new(b".5".to_vec(), "t.go");
    let at = lx.pos();
    assert_eq!(lx.scan_punct(), Some(PunctScan::LeadingDotNumber));
    assert_eq!(lx.pos(), at, "交接后位置必须回到 `.`");
    assert_eq!(
        lx.peek(),
        Some('.'),
        "`.` 必须没被消耗 —— 数字扫描器从它开始"
    );
    assert!(lx.take_diags().is_empty());
}

#[test]
fn 非数字非点号时_点就是点() {
    // `.x` ⇒ `.` 然后 x（不是数字，也不是 `...`）
    let mut lx = Lexer::new(b".x".to_vec(), "t.go");
    assert_eq!(
        lx.scan_punct(),
        Some(PunctScan::Token(TokenKind::Delim(Delim::Dot)))
    );
    assert_eq!(lx.peek(), Some('x'));
    // `..` 之后紧跟 `.` 的情况由 `...` 覆盖；`.!` 这类也给 `.`
    let mut lx = Lexer::new(b".!".to_vec(), "t.go");
    assert_eq!(
        lx.scan_punct(),
        Some(PunctScan::Token(TokenKind::Delim(Delim::Dot)))
    );
}

// ── ④ 斜杠的三种去向 ────────────────────────────────────────────────

#[test]
fn 斜杠分出除法_复合赋值_注释三条路() {
    assert_eq!(tok(b"/"), plain(Operator::Div));
    assert_eq!(tok(b"/="), assign(Operator::Div));

    // `//` 与 `/*` ⇒ 注释：官方把**两个字符都吃掉**再调 lineComment/fullComment，
    // 所以交接后游标在注释体的第一个字符上。（注释本体由 T-L13 处理）
    let mut lx = Lexer::new(b"//x".to_vec(), "t.go");
    assert_eq!(lx.scan_punct(), Some(PunctScan::Comment));
    assert_eq!(lx.peek(), Some('x'), "`//` 两个字符都已被消耗");

    let mut lx = Lexer::new(b"/*x*/".to_vec(), "t.go");
    assert_eq!(lx.scan_punct(), Some(PunctScan::Comment));
    assert_eq!(lx.peek(), Some('x'), "`/*` 两个字符都已被消耗");
}

// ── ⑤ 与标识符的邻接（分界必须干净）────────────────────────────────

#[test]
fn 关键字与标点紧邻也能分界() {
    for (src, kw, delim, col) in [
        (&b"for("[..], "for", Delim::LParen, 4),
        (b"if{", "if", Delim::LBrace, 3),
        (b"return(", "return", Delim::LParen, 7),
    ] {
        let mut lx = Lexer::new(src.to_vec(), "t.go");
        let got = lx.scan_ident();
        assert_eq!(
            got,
            Some(TokenKind::Keyword(
                rgoc_lex::Keyword::lookup(kw).expect("是关键字")
            )),
            "{src:?}"
        );
        assert_eq!(lx.pos(), p(1, col), "{src:?} 的标点起点");
        assert_eq!(
            lx.scan_punct(),
            Some(PunctScan::Token(TokenKind::Delim(delim))),
            "{src:?}"
        );
    }
}

// ── ⑥ 不是标点起始字符 ─────────────────────────────────────────────

#[test]
fn 不是标点起始字符时返回_none_且不消耗() {
    for src in [
        &b""[..],
        b"x",
        b"1",
        b"\"s\"",
        "中".as_bytes(),
        b"@",
        b"#",
        b"$",
        b"\\",
    ] {
        let mut lx = Lexer::new(src.to_vec(), "t.go");
        let at = lx.pos();
        assert_eq!(lx.scan_punct(), None, "输入 {src:?} 不该被当作标点");
        assert_eq!(lx.pos(), at, "输入 {src:?} 不该消耗任何字符");
        // 游标必须还是第一个**字符**（注意 `中` 是多字节，不能按字节取）
        let first = std::str::from_utf8(src).ok().and_then(|s| s.chars().next());
        assert_eq!(lx.peek(), first, "输入 {src:?} 的游标必须不动");
    }
}

#[test]
fn 空白与换行不是标点() {
    // 跳空白是 `next()`（T-L15）的事，不是标点扫描的
    for src in [&b" "[..], b"\t", b"\n", b"\r"] {
        assert_eq!(tok(src), None, "{src:?}");
    }
}

#[test]
fn ascii_全域穷举_能起头标点的集合恰好是那二十三个() {
    // ★ 反向断言：上面那条"48 条都能扫出来"只能证明"没少认"，
    //   而**多认一个字符**会让非法字符的诊断永远不触发 —— 同样是静默的。
    //   所以把 ASCII 0x00..0x7F 全扫一遍，逐个对集合。
    const STANDS: &str = "([{,;)]}:+-*/%&|^<>=!~.";
    assert_eq!(
        STANDS.chars().count(),
        23,
        "官方 switch 的 case 字符共 23 个"
    );

    for c in 0u8..=0x7f {
        let ch = c as char;
        let mut lx = Lexer::new(vec![c], "t.go");
        let got = lx.scan_punct().is_some();
        assert_eq!(got, STANDS.contains(ch), "字符 {ch:?}（0x{c:02x}）");
    }
}

#[test]
fn 非_ascii_字符一律不是标点() {
    for ch in ['中', '😀', '²', 'µ', '€', '\u{feff}', '\u{e000}'] {
        let mut lx = Lexer::new(ch.to_string().into_bytes(), "t.go");
        assert_eq!(lx.scan_punct(), None, "{ch:?} 不该被当作标点");
    }
}
