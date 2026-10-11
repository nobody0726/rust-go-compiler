//! `ident` 的验收测试（T-L08；LEX-02 字符分类 / LEX-05 关键字 / LEX-08 标识符）。
//!
//! # 这一层的两条纪律
//!
//! 1. **谓词必须来自 `unicode_tables`，不能用 Rust std 的近似方法**（D-M1-6）。
//!    std 的 `char::is_alphabetic`/`is_numeric` 都比 Go 宽 2,903 个码点 ——
//!    差异是**诊断层面**的静默错误（见 F29），不是词元边界。
//! 2. **切完再查关键字**（LEX-05）—— 边扫边匹配会让 `forx` 被误判成 `for` + `x`。
//!    `forx` / `format` 这两条就是专门盯它的。
//!
//! # 期望值全部来自 oracle
//!
//! 本文件里每个非 ASCII 用例的位置/诊断都跑过 `tools/token-oracle`（go1.27.1）。
//! 其中 F29 那组（`x²` / `²abc` / `x\u{0345}` / `٠abc` / `xµ` / `x１`）是**先测后写**：
//! 我原先在设计文档里断言「`x²` 在 Go 里是 `x` + 非法字符」，实测**证伪** ——
//! 官方是「**吸收 + 报 `LEX-BAD-IDENT-CHAR`**」。

use rgoc_lex::{DiagCode, Keyword, Lexer, TokenKind};

/// 扫一个标识符，返回（结果，诊断代号）。
///
/// 诊断走 `take_diags()` + `into_sorted()`（C1 §4 的排序键是 `file→line→col→code`）。
fn scan(src: &[u8]) -> (Option<TokenKind>, Vec<&'static str>) {
    let mut lx = Lexer::new(src.to_vec(), "t.go");
    let kind = lx.scan_ident();
    let codes = lx
        .take_diags()
        .into_sorted()
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    (kind, codes)
}

fn kind_of(src: &[u8]) -> Option<TokenKind> {
    scan(src).0
}

/// `Some(Ident(字节))` —— 载荷是**字节**不是 `String`（D-M1-12 / F22）。
fn ident(bytes: &[u8]) -> Option<TokenKind> {
    Some(TokenKind::Ident(bytes.to_vec()))
}

fn kw(k: Keyword) -> Option<TokenKind> {
    Some(TokenKind::Keyword(k))
}

// ── ① 续接谓词：能续接的是一类 ───────────────────────────────────────

#[test]
fn 能续接的字符构成一个完整的标识符() {
    // 数字、`_`、Unicode 字母、Nd 数字都是合法**续接**字符
    for s in [
        "str1",
        "str1x",
        "str_",
        "_x",
        "日本語",
        "ab中cd",
        "a۰۱۸",
        "foo६४",
        "bar９８７６",
        "ŝ",
    ] {
        assert_eq!(kind_of(s.as_bytes()), ident(s.as_bytes()), "输入 {s}");
    }
}

#[test]
fn 不能续接的字符把标识符就地截断() {
    // 成对边界：上面那批"能续接"，这批"不能续接" —— 只测一侧等于没测
    for (src, want) in [
        (&b"str x"[..], &b"str"[..]),
        (&b"str+x"[..], &b"str"[..]),
        (&b"a-b"[..], &b"a"[..]),
        (&b"\xe4\xb8\xad."[..], &b"\xe4\xb8\xad"[..]), // 中.
        (&b"x(y)"[..], &b"x"[..]),
    ] {
        // 刻意不用 `from_utf8_lossy` 做提示（D-M1-12 禁止它；这里直接打字节）
        assert_eq!(kind_of(src), ident(want), "输入 {src:?}");
    }
}

#[test]
fn 终止符原封不动地留在游标上() {
    // "完整"不是检查出来的，是**走不动了自然停下来的位置** ——
    // 所以词元是 `[起点, 游标)`，终止符天然不含在内，且**没有被消耗**。
    let mut lx = Lexer::new(b"str+x".to_vec(), "t.go");
    assert_eq!(lx.scan_ident(), ident(b"str"));
    assert_eq!(lx.peek(), Some('+'), "终止符必须留在游标上，供下一个词元用");
}

// ── ② 关键字：切完再查（LEX-05）───────────────────────────────────────

#[test]
fn 关键字在整串切完之后才查() {
    assert_eq!(kind_of(b"for"), kw(Keyword::For));
    // ★ 反证：`for` 不是边扫边匹配的 —— 否则 `forx` 会变成 `for` + `x`
    assert_eq!(kind_of(b"forx"), ident(b"forx"));
    assert_eq!(kind_of(b"format"), ident(b"format"));
    assert_eq!(kind_of(b"iff"), ident(b"iff"));
    assert_eq!(kind_of(b"return_"), ident(b"return_"));
}

#[test]
fn 二十五个关键字全部命中且大小写敏感() {
    // 遍历 `Keyword::ALL` 而不是手写清单 —— 本仓三次踩过「写死的键列表变成盲区」
    for k in Keyword::ALL {
        assert_eq!(kind_of(k.as_str().as_bytes()), kw(k), "{}", k.as_str());
    }
    // 关键字是大小写敏感的：`For` 是普通标识符
    assert_eq!(kind_of(b"For"), ident(b"For"));
    assert_eq!(kind_of(b"TRUE"), ident(b"TRUE"));
}

#[test]
fn 预声明标识符是普通标识符不是关键字() {
    // 实测 F7：`true`/`nil`/`iota`/`println`/`int` 全是 `_Name`
    for s in [
        "true", "false", "nil", "iota", "println", "append", "int", "error", "any", "len",
    ] {
        assert_eq!(
            kind_of(s.as_bytes()),
            ident(s.as_bytes()),
            "{s} 不该是关键字"
        );
    }
}

// ── ③ 入口判定的短路（照抄官方，别"顺手统一"）───────────────────────

#[test]
fn ascii_数字开头不报错_因为入口短路了() {
    // 官方入口是 `isLetter(s.ch) || s.ch >= utf8.RuneSelf && atIdentChar(true)`，
    // `&&` 比 `||` 紧 ⇒ **ASCII 数字根本不会走到 atIdentChar** ⇒ 不会误报
    // 「identifier cannot begin with digit」。
    //
    // ★ 这条断言盯的是"把入口写成 `is_letter || is_digit || at_ident_char(true)`"这类
    //    **顺手统一** —— 那样 `1str` 会凭空多一条 LEX-ID-LEADING-DIGIT。
    let (k, d) = scan(b"1str");
    assert_eq!(k, None, "ASCII 数字不能起头标识符（那归数字字面量扫描）");
    assert!(d.is_empty(), "不得产生任何诊断，实际：{d:?}");
}

#[test]
fn 非_ascii_的_nd_开头才报不能以数字开头() {
    // 实测（oracle）：`٠abc`（U+0660，Nd）⇒ 一个 name + `LEX-ID-LEADING-DIGIT` @1:1
    // 注意它**仍然是一个标识符** —— 官方的 atIdentChar 是「报错但返回 true」
    assert_eq!(
        scan("٠abc".as_bytes()),
        (ident("٠abc".as_bytes()), vec!["LEX-ID-LEADING-DIGIT"])
    );
}

#[test]
fn 下划线属于字母类() {
    // 官方 `isLetter` 把 `_` 算作字母（`unicode.IsLetter(ch) || ch == '_'`）
    assert_eq!(kind_of(b"_"), ident(b"_"));
    assert_eq!(kind_of(b"_x"), ident(b"_x"));
    assert_eq!(kind_of(b"x_1"), ident(b"x_1"));
}

// ── ④ F29：非字母非 Nd 的非 ASCII 字符 —— 吸收 + 报错 ────────────────

#[test]
fn 非字母非_nd_的非_ascii_字符被吸收但仍然报错() {
    // ★ 这条是**实测证伪了我在设计文档里写错的断言**之后立起来的。
    //   我原写「`x²` 在 Go 里是 `x` + 非法字符」—— 错。
    //   官方 `atIdentChar` 的第三条分支是 `case s.ch >= utf8.RuneSelf:`
    //   ⇒ **先 errorf 再 return true** ⇒ 该字符**被吸收**，只是附带一条诊断。
    //   ⇒ 差异在**诊断层面**，不在词元边界（F29）。
    assert_eq!(
        scan("x²".as_bytes()),
        (ident("x²".as_bytes()), vec!["LEX-BAD-IDENT-CHAR"])
    );
    // 起始位置也是同一个代号（**不是** LEX-ID-LEADING-DIGIT）——
    // 这是"用 std 近似谓词"与真实现分道扬镳的地方
    assert_eq!(
        scan("²abc".as_bytes()),
        (ident("²abc".as_bytes()), vec!["LEX-BAD-IDENT-CHAR"])
    );
    // Mn（组合符）：Rust 的 is_alphabetic 认它，Go 的 L* 不认
    assert_eq!(
        scan("x\u{345}".as_bytes()),
        (ident("x\u{345}".as_bytes()), vec!["LEX-BAD-IDENT-CHAR"])
    );
    // 吸收之后**继续**续接：`²` 后面的数字仍然是合法的续接字符
    assert_eq!(
        scan("x²1".as_bytes()),
        (ident("x²1".as_bytes()), vec!["LEX-BAD-IDENT-CHAR"])
    );
}

#[test]
fn 对照组_两侧分类一致时不报错() {
    // `xµ`：U+00B5 是 Ll ⇒ Go 与 std 都认它是字母 ⇒ 无诊断
    assert_eq!(scan("xµ".as_bytes()), (ident("xµ".as_bytes()), vec![]));
    // `x１`：U+FF11 是 Nd ⇒ Go 与 std 都认它是数字 ⇒ 无诊断
    // （区别在于 std 还把 No 也当数字，见上面那条）
    assert_eq!(scan("x１".as_bytes()), (ident("x１".as_bytes()), vec![]));
}

// ── ⑤ 载荷是字节（F22 / F8）─────────────────────────────────────────

#[test]
fn 标识符原文里的非法字节被原样保留() {
    // 实测 F22：`x\xff y` ⇒ `name lit="x\xff"` —— 0xFF 不是合法 UTF-8，
    // 但它是**原文切片**的一部分，必须原样留下（这也是 D-M1-12 改 Vec<u8> 的理由）
    assert_eq!(
        scan(b"x\xff y"),
        (ident(b"x\xff"), vec!["LEX-INVALID-UTF8"])
    );
    // F8：NUL 同理 —— 字符层报错并跳过它，但字节仍在段里
    assert_eq!(scan(b"x\x00y"), (ident(b"x\x00y"), vec!["LEX-INVALID-NUL"]));
}

// ── ⑥ 不能起头的情形 ───────────────────────────────────────────────

#[test]
fn 不是标识符起始字符时返回_none_且不报错() {
    // 全部是 ASCII 非字母：官方入口的第一项 `isLetter` 为假，第二项要求 `>= 0x80`
    // 也为假 ⇒ 连 `atIdentChar` 都不会调用 ⇒ **一条诊断都不该有**。
    for src in [
        &b""[..],
        b" ",
        b"\t\n",
        b"+",
        b"\"s\"",
        b"//x",
        b";",
        b"1",
        b"0x",
    ] {
        let (k, d) = scan(src);
        assert_eq!(k, None, "输入 {src:?} 不该被当作标识符");
        assert!(d.is_empty(), "输入 {src:?} 不该产生诊断：{d:?}");
    }
}

#[test]
fn emoji_起始会被吸收并报非法字符() {
    // 与 F29 同一机制：非 ASCII、非字母、非 Nd ⇒ 官方走了 `>= RuneSelf` 那条分支
    assert_eq!(
        scan("😀".as_bytes()),
        (ident("😀".as_bytes()), vec!["LEX-BAD-IDENT-CHAR"])
    );
}

// ── ⑦ Lexer 的最小面 ───────────────────────────────────────────────

#[test]
fn lexer_把文件登记进自己的表() {
    let lx = Lexer::new(b"x".to_vec(), "test/hello.go");
    assert_eq!(lx.table().name(lx.file()), "test/hello.go");
}

#[test]
fn take_diags_会把诊断取走而不是复制() {
    let mut lx = Lexer::new("²a".as_bytes().to_vec(), "t.go");
    assert_eq!(lx.scan_ident(), ident("²a".as_bytes()));
    assert_eq!(lx.take_diags().len(), 1);
    assert!(lx.take_diags().is_empty(), "取走之后再取应当是空的");
}

#[test]
fn 诊断的位置落在被吸收的那个字符上() {
    // 位置也来自 oracle：`x²` ⇒ 诊断在 1:2（`²` 自己的位置）
    let mut lx = Lexer::new("x²".as_bytes().to_vec(), "t.go");
    assert_eq!(lx.scan_ident(), ident("x²".as_bytes()));
    let ds = lx.take_diags().into_sorted();
    assert_eq!((ds.len(), ds[0].pos.line, ds[0].pos.col), (1, 1, 2));
    assert_eq!(ds[0].code, DiagCode::BadIdentChar);
}
