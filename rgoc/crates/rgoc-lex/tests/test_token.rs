//! `token.rs` 的数据模型测试（T-L03）。
//!
//! 这一组**不做词法分析** —— 只验证"词元的数据模型"本身：
//! 枚举基数、往返一致性、优先级分组、以及 D-M1-10 的无损性。
//!
//! # 断言一律遍历 `ALL` 常量，不手写清单
//!
//! 本仓的教训是「**写死的键列表会变成盲区**」（T38 → T39 → T47 三次同形）：
//! 手写清单漏一项时**不会报错**，只会让那一条断言恒真。
//! 所以这里用 `Keyword::ALL` / `Operator::ALL` / `Delim::ALL` 遍历，
//! 再配一条"基数等于文档声称的数字"的反向断言 ——
//! 两者合起来才既覆盖全部、又能发现"新增了变体但常量没跟上"。

use rgoc_lex::{Delim, FileId, FileTable, Keyword, LitKind, OpForm, Operator, Pos, SemiOrigin};
use rgoc_lex::{Token, TokenKind};

/// 造一个位置，只为让 `Token` 可构造。
fn at(line: u32, col: u32) -> Pos {
    let mut t = FileTable::new();
    let f: FileId = t.intern("t.go");
    Pos {
        base: None, // 无 `//line` 基准
        file: f,
        line,
        col,
    }
}

// ── 关键字（LEX-05）──────────────────────────────────────────────────

#[test]
fn keyword_all_基数为_25_且无重复() {
    assert_eq!(Keyword::ALL.len(), 25, "关键字个数必须与文档声称的 25 一致");
    let mut seen = BTreeSet::new();
    for k in Keyword::ALL {
        assert!(seen.insert(k.as_str()), "{} 在 ALL 里重复出现", k.as_str());
    }
    assert_eq!(seen.len(), 25);
}

#[test]
fn keyword_lookup_与_as_str_对_all_往返一致() {
    for k in Keyword::ALL {
        assert_eq!(
            Keyword::lookup(k.as_str()),
            Some(k),
            "{} 的 lookup 没有往返回同一个变体",
            k.as_str()
        );
    }
}

#[test]
fn keyword_lookup_对预声明标识符返回_none() {
    // 实测事实 F7：这些在官方也是 _Name（普通标识符），不是关键字。
    // 这条是「M1 不解释语义」的具体体现 —— 把它们提升为关键字是**静默的错**。
    for s in [
        "true", "false", "nil", "iota", "_", "println", "append", "int", "error", "any",
    ] {
        assert_eq!(Keyword::lookup(s), None, "{s} 不是关键字");
    }
}

#[test]
fn keyword_lookup_对空串与相邻拼写返回_none() {
    for s in ["", " ", "forx", "iff", "selects", "typee", "GO", "For"] {
        assert_eq!(Keyword::lookup(s), None, "{s:?} 不是关键字");
    }
}

// ── 运算符：form 无损（D-M1-10 的核心）────────────────────────────────

#[test]
fn op_form_无损_加号三种写法互不相等() {
    let plus = (OpForm::Plain, Operator::Add);
    let plus_assign = (OpForm::Assign, Operator::Add);
    let plus_plus = (OpForm::IncDec, Operator::Add);
    assert_ne!(plus, plus_assign);
    assert_ne!(plus, plus_plus);
    assert_ne!(plus_assign, plus_plus);
}

#[test]
fn token_kind_层面_加号三种写法互不相等() {
    let mk = |form| TokenKind::Op {
        form,
        op: Operator::Add,
        prec: Operator::Add.prec(),
    };
    let a = mk(OpForm::Plain);
    let b = mk(OpForm::Assign);
    let c = mk(OpForm::IncDec);
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_ne!(b, c);
}

#[test]
fn d_m1_10_无损性核对表的八行逐行成立() {
    // 每行断言"两个不同拼写不会被建模成同一个值"。
    // 只断言"相等的东西相等"是白写 —— 这里断言的都是**不相等**。
    let eq = TokenKind::Op {
        form: OpForm::Plain,
        op: Operator::Assign,
        prec: 0,
    };
    let eqeq = TokenKind::Op {
        form: OpForm::Plain,
        op: Operator::Eql,
        prec: Operator::Eql.prec(),
    };
    assert_ne!(eq, eqeq, "`=` 与 `==` 必须可区分");

    let define = TokenKind::Op {
        form: OpForm::Plain,
        op: Operator::Define,
        prec: 0,
    };
    assert_ne!(
        define,
        TokenKind::Delim(Delim::Colon),
        "`:=` 与 `:` 必须可区分"
    );

    let arrow = TokenKind::Op {
        form: OpForm::Plain,
        op: Operator::Arrow,
        prec: 0,
    };
    let lss = TokenKind::Op {
        form: OpForm::Plain,
        op: Operator::Lss,
        prec: Operator::Lss.prec(),
    };
    assert_ne!(arrow, lss, "`<-` 与 `<` 必须可区分");

    let star = TokenKind::Op {
        form: OpForm::Plain,
        op: Operator::Mul,
        prec: Operator::Mul.prec(),
    };
    let star_assign = TokenKind::Op {
        form: OpForm::Assign,
        op: Operator::Mul,
        prec: Operator::Mul.prec(),
    };
    assert_ne!(
        star, star_assign,
        "`*` 与 `*=` 必须可区分（故不需要单独的 _Star）"
    );
}

// ── 运算符：优先级（spec:L268-282）───────────────────────────────────

#[test]
fn operator_prec_与_spec_一致() {
    // 官方 tokens.go 的优先级常量：precOrOr=1 … precMul=5
    assert_eq!(Operator::OrOr.prec(), 1);
    assert_eq!(Operator::AndAnd.prec(), 2);
    assert_eq!(Operator::Eql.prec(), 3);
    assert_eq!(Operator::Add.prec(), 4);
    assert_eq!(Operator::Shr.prec(), 5);
}

#[test]
fn operator_prec_的分组与官方一致_逐组核对() {
    let group = |p: u8| -> Vec<&str> {
        let mut v: Vec<&str> = Operator::ALL
            .iter()
            .filter(|o| o.prec() == p)
            .map(|o| name_of(*o))
            .collect();
        v.sort_unstable();
        v
    };
    assert_eq!(
        group(0).len(),
        5,
        "无优先级组应恰为 5 个（Not/Tilde/Assign/Define/Arrow）"
    );
    assert_eq!(group(1), vec!["OrOr"], "prec 1 只应有 || （spec:L268-282）");
    assert_eq!(group(2), vec!["AndAnd"], "prec 2 只应有 &&");
    assert_eq!(
        group(3),
        vec!["Eql", "Geq", "Gtr", "Leq", "Lss", "Neq"],
        "prec 3 = 六个比较运算符"
    );
    assert_eq!(
        group(4),
        vec!["Add", "Or", "Sub", "Xor"],
        "prec 4 = + - | ^"
    );
    assert_eq!(
        group(5),
        vec!["And", "AndNot", "Div", "Mul", "Rem", "Shl", "Shr"],
        "prec 5 = * / % & &^ << >>"
    );
}

#[test]
fn operator_all_每个变体的_prec_都不超过_5() {
    for o in Operator::ALL {
        assert!(o.prec() <= 5, "{o:?} 的优先级越界");
    }
    assert_eq!(Operator::ALL.len(), 24, "Operator 变体数");
}

// ── 分隔符与分号来源 ────────────────────────────────────────────────

#[test]
fn delim_all_基数为_10_且无重复() {
    // `( ) [ ] { } , : . ...` —— `;` 归 Semi，不在这里
    assert_eq!(Delim::ALL.len(), 10);
    let mut seen = BTreeSet::new();
    for d in Delim::ALL {
        assert!(seen.insert(format!("{d:?}")), "{d:?} 在 ALL 里重复");
    }
    assert_eq!(seen.len(), 10);
    assert!(
        Delim::ALL.contains(&Delim::Colon),
        "Colon 必须在 Delim 里（`:` 是分隔符，`:=` 才是运算符）"
    );
}

#[test]
fn semi_origin_三个来源齐备() {
    assert_eq!(SemiOrigin::ALL.len(), 3);
    assert_eq!(
        SemiOrigin::ALL,
        [SemiOrigin::Explicit, SemiOrigin::Newline, SemiOrigin::Eof]
    );
}

// ── 字面量子类（LEX-09..13）────────────────────────────────────────

#[test]
fn lit_kind_五种齐备() {
    assert_eq!(LitKind::ALL.len(), 5);
    let mut seen = BTreeSet::new();
    for k in LitKind::ALL {
        assert!(seen.insert(k.as_official_code()), "{k:?} 的官方编码重复");
    }
    assert_eq!(seen.len(), 5);
}

#[test]
fn lit_kind_的官方编码与实测值一致() {
    // 实测事实 F6：0=Int 1=Float 2=Imag 3=Rune 4=String
    assert_eq!(LitKind::Int.as_official_code(), 0);
    assert_eq!(LitKind::Float.as_official_code(), 1);
    assert_eq!(LitKind::Imag.as_official_code(), 2);
    assert_eq!(LitKind::Rune.as_official_code(), 3);
    assert_eq!(LitKind::String.as_official_code(), 4);
}

// ── Token / TokenKind 的形状 ──────────────────────────────────────

#[test]
fn token_是_位置_加_类别() {
    let t = Token {
        pos: at(3, 7),
        kind: TokenKind::Ident("变量".as_bytes().to_vec()),
    };
    assert_eq!(t.pos.line, 3);
    assert_eq!(t.pos.col, 7);
    assert_eq!(t.kind, TokenKind::Ident("变量".as_bytes().to_vec()));
    assert_ne!(t.kind, TokenKind::Eof);
}

#[test]
fn 七种_token_kind_分支都能构造且两两不等() {
    let all = [
        TokenKind::Ident(b"x".to_vec()),
        TokenKind::Literal {
            text: b"1".to_vec(),
            lit_kind: LitKind::Int,
            bad: false,
        },
        TokenKind::Keyword(Keyword::Func),
        TokenKind::Op {
            form: OpForm::Plain,
            op: Operator::Add,
            prec: 4,
        },
        TokenKind::Delim(Delim::LParen),
        TokenKind::Semi(SemiOrigin::Newline),
        TokenKind::Eof,
    ];
    for (i, a) in all.iter().enumerate() {
        for (j, b) in all.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "分支 {i} 与 {j} 撞了");
            }
        }
    }
}

#[test]
fn literal_的_bad_是载荷的一部分_不能丢() {
    // `0x` 这类畸形字面量**照常产出词元**（实测事实：bad=true + 一条诊断）。
    // 若 bad 不参与相等性，"畸形"与"正常"就会在比对时被当成同一个词元。
    let ok = TokenKind::Literal {
        text: b"0".to_vec(),
        lit_kind: LitKind::Int,
        bad: false,
    };
    let bad = TokenKind::Literal {
        text: b"0x".to_vec(),
        lit_kind: LitKind::Int,
        bad: true,
    };
    assert_ne!(ok, bad);
}

// ── 测试辅助 ───────────────────────────────────────────────────────

use std::collections::BTreeSet;

/// 仅测试用：把 `Operator` 映射成官方 `operator_string.go` 的名字。
///
/// **这不是生产代码** —— 生产侧的渲染口径属于 T-L18（driver `lex` 输出格式），
/// 那里才会面对「`=`/`:=`/`<-` 在官方没有 op 值」这个问题。
/// 这里只是为了能写出"分组恰好等于哪几个"的可读断言。
fn name_of(o: Operator) -> &'static str {
    match o {
        Operator::Not => "Not",
        Operator::Tilde => "Tilde",
        Operator::Assign => "Assign",
        Operator::Define => "Define",
        Operator::Arrow => "Arrow",
        Operator::OrOr => "OrOr",
        Operator::AndAnd => "AndAnd",
        Operator::Eql => "Eql",
        Operator::Neq => "Neq",
        Operator::Lss => "Lss",
        Operator::Leq => "Leq",
        Operator::Gtr => "Gtr",
        Operator::Geq => "Geq",
        Operator::Add => "Add",
        Operator::Sub => "Sub",
        Operator::Or => "Or",
        Operator::Xor => "Xor",
        Operator::Mul => "Mul",
        Operator::Div => "Div",
        Operator::Rem => "Rem",
        Operator::And => "And",
        Operator::AndNot => "AndNot",
        Operator::Shl => "Shl",
        Operator::Shr => "Shr",
    }
}

/// R7 的回归断言：词元载荷**必须能装下非法 UTF-8**。
///
/// 实测（oracle）：`x\xff y` 里标识符的原文是 `x\xff` —— 那个 `0xFF` 不是合法 UTF-8。
/// 若载荷是 `String`，这条断言**根本无法构造**（或者被迫做有损转换 ⇒ 静默丢信息，
/// 而 golden 比对会因为"少了一个字节"而红，红得让人找不到原因）。
#[test]
fn 标识符与字面量的载荷能装下非法_utf8() {
    let ident = TokenKind::Ident(vec![b'x', 0xff]);
    let lit = TokenKind::Literal {
        text: vec![b'1', 0xff, b'2'],
        lit_kind: LitKind::Int,
        bad: false,
    };
    // 关键：字节必须**原样保留**，一个都不能少、不能被替换成 U+FFFD
    match &ident {
        TokenKind::Ident(b) => assert_eq!(b, &vec![b'x', 0xff], "非法字节被改写了"),
        other => panic!("期望 Ident，得到 {other:?}"),
    }
    match &lit {
        TokenKind::Literal { text, .. } => {
            assert_eq!(text, &vec![b'1', 0xff, b'2'], "非法字节被改写了")
        }
        other => panic!("期望 Literal，得到 {other:?}"),
    }
}

/// 与上一条配对：合法 UTF-8 的载荷仍应能当 `&str` 用（关键字查询走这条路）。
#[test]
fn 合法_utf8_的标识符可以转成_str() {
    let k = TokenKind::Ident("变量".as_bytes().to_vec());
    match &k {
        TokenKind::Ident(b) => {
            assert_eq!(std::str::from_utf8(b).unwrap(), "变量");
            // 关键字全是 ASCII ⇒ 这个标识符必然不是关键字
            assert_eq!(Keyword::lookup(std::str::from_utf8(b).unwrap()), None);
        }
        other => panic!("期望 Ident，得到 {other:?}"),
    }
}

/// **非法 UTF-8 的标识符必然不是关键字** —— 这是"载荷用字节"之后要保住的性质。
///
/// 推导：关键字表全是 ASCII 拼写，而非法的字节序列连 `&str` 都构造不出来，
/// 所以查询必然返回 `None`。它值得单独一条断言，因为**另一条路是错的**：
/// 若用 `from_utf8_lossy`（把非法字节换成 U+FFFD）再查，
/// 会得到一个"看起来正常但内容已被改写"的字符串 —— 静默丢信息。
#[test]
fn 非法_utf8_的标识符查不到关键字() {
    // 经过一层函数参数，避免把常量非法字面量直接喂给 from_utf8
    fn lookup_bytes(b: &[u8]) -> Option<Keyword> {
        std::str::from_utf8(b).ok().and_then(Keyword::lookup)
    }

    let invalid: Vec<u8> = vec![b'x', 0xff];
    assert_eq!(lookup_bytes(&invalid), None, "非法 UTF-8 不该查出关键字");
    // 反证：同样是字节，合法拼写能查到 —— 说明上面那个 None 是"因为非法"，不是"因为坏了"
    assert_eq!(lookup_bytes(b"for"), Some(Keyword::For));
}
