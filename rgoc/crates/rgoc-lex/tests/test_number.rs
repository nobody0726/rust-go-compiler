//! `number` 的验收测试（T-L10；LEX-09 整数 / LEX-10 浮点 / LEX-11 虚数）。
//!
//! # 期望值全部来自 oracle
//!
//! 本文件每一条**都不许自行发明规则** —— 包括"下划线在指数里算不算合法"这种
//! 看起来可以推理的问题。全部跑过 `tools/token-oracle`（go1.27.1）。
//!
//! # 这一层最容易被"顺手简化"掉的两处（都靠 oracle 才发现）
//!
//! 1. **`digsep` 在指数分支是 `=` 不是 `|=`** —— 尾数的"有数字"位被**丢弃**，
//!    只保留"有 `_`"位。所以 `1.5e_` 报的是「指数缺数字」，
//!    而 `_` 的位置非法（`invalidSep`）**根本不会被检查**（`ok` 已假）。
//!    写成 `|=` 会让 `digsep&1` 被尾数置位 ⇒ **少报一条 `LEX-BAD-EXPONENT`**。
//! 2. **非法分隔符是"事后"定位的** —— 扫描时只记 `digsep` 位集，
//!    错误下标由 `invalidSep()` 对**切完的字面量**再扫一遍得出。
//!    边扫边判会写出另一套规则，而两套规则在 `1__2` / `0b__1000` 这类输入上**不等价**。

use rgoc_lex::{Lexer, LitKind, PunctScan, TokenKind};

/// 扫一个字面量（数字起头），返回（种类，(行,列,代号) 列表）。
///
/// 诊断走 `take_diags()` + `into_sorted()`（C1 §4 的排序键是 `file→line→col→code`）。
fn scan(src: &[u8]) -> (TokenKind, Vec<(u32, u32, &'static str)>) {
    let mut lx = Lexer::new(src.to_vec(), "t.go");
    let kind = lx.scan_number(false);
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

/// `Literal { text, lit_kind, bad }` —— 载荷是**字节**不是 `String`（D-M1-12）。
fn lit(text: &str, k: LitKind, bad: bool) -> TokenKind {
    TokenKind::Literal {
        text: text.as_bytes().to_vec(),
        lit_kind: k,
        bad,
    }
}

fn ok_int(s: &str) -> TokenKind {
    lit(s, LitKind::Int, false)
}
fn ok_float(s: &str) -> TokenKind {
    lit(s, LitKind::Float, false)
}
fn ok_imag(s: &str) -> TokenKind {
    lit(s, LitKind::Imag, false)
}
fn bad(s: &str, k: LitKind) -> TokenKind {
    lit(s, k, true)
}

// ── ① 进制前缀（LEX-09）──────────────────────────────────────────────

#[test]
fn 四种进制与裸零() {
    // `0x/0o/0b` 前缀 + 裸 `0`（八进制）+ 前导零。**大小写都认**（官方 `lower(ch)`）。
    // `0177` / `07` 专门盯**边界数码**：八进制的最大合法数码是 `7`，
    // 越界判断必须是 `>= max`（`max = '0' + base`）而不是 `> max` ——
    // 写成 `>` 会让 `8` 漏报；把 `max` 写成 `'0' + base - 1` 会让 `7` 误报。
    for s in [
        "0b101", "0B101", "0o17", "0O17", "0x1f", "0X1f", "0123", "0", "00", "0X_1F", "0177", "07",
    ] {
        assert_eq!(scan1(s), (ok_int(s), vec![]), "输入 {s:?}");
    }
}

#[test]
fn 十六进制在非十六进制字母处停下不报错() {
    // `0x1g` ⇒ literal `0x1` + ident `g`（oracle 实测**无诊断**）。
    // `digits(16)` 只是"能收就收"，收不动就停 —— 相邻合法性是 parser 的事。
    let mut lx = Lexer::new(b"0x1g".to_vec(), "t.go");
    assert_eq!(lx.scan_number(false), ok_int("0x1"));
    assert!(lx.take_diags().into_sorted().is_empty());
    assert_eq!(lx.peek(), Some('g'), "非十六进制字符必须原样留下");
    assert_eq!(lx.scan_ident(), Some(TokenKind::Ident(b"g".to_vec())));
}

// ── ② 下划线分隔符（LEX-09 的 `_`）───────────────────────────────────

#[test]
fn 下划线分隔符的合法位置() {
    for s in ["1_000", "1_000_000", "0x_1f", "0_1", "0b1_0"] {
        assert_eq!(scan1(s), (ok_int(s), vec![]), "输入 {s:?}");
    }
}

#[test]
fn 下划线分隔符的非法位置() {
    // 期望值全部来自 oracle；列号 = 词元起点列（1）+ 字面量内字节偏移。
    //
    //   `1__2`      → 3（第二个 `_`：前面不是数字）
    //   `1_`        → 2（收尾的 `_`）
    //   `0b__1000`  → 4（前缀 `0b` 算一个数字位，第一个 `_` 合法、第二个非法）
    //   `1_0_`      → 4（收尾）
    //   `0_`        → 2（**前导零算一个数字位**，所以 `0_` 的 `_` 是"收尾"）
    //   `0x1_`      → 4（收尾）
    //   `1_e1`      → 2（`_` 在尾数末尾、紧跟指数 ⇒ 前一个字符不是数字）
    let cases: &[(&str, LitKind, u32)] = &[
        ("1__2", LitKind::Int, 3),
        ("1_", LitKind::Int, 2),
        ("0b__1000", LitKind::Int, 4),
        ("1_0_", LitKind::Int, 4),
        ("0_", LitKind::Int, 2),
        ("0x1_", LitKind::Int, 4),
        ("1_e1", LitKind::Float, 2),
    ];
    for &(s, k, col) in cases {
        assert_eq!(
            scan1(s),
            (bad(s, k), vec![(1, col, "LEX-BAD-SEP")]),
            "输入 {s:?}"
        );
    }
}

// ── ③ 浮点（LEX-10）─────────────────────────────────────────────────

#[test]
fn 十进制与十六进制浮点() {
    for s in [
        "1e9", "1E9", "1.5E-3", "0x1p-2", "0x.8p1", "1.e9", "0x1.p1", "1.", "0x1P2",
    ] {
        assert_eq!(scan1(s), (ok_float(s), vec![]), "输入 {s:?}");
    }
}

#[test]
fn 十六进制浮点缺_p_指数要报错() {
    // 报错位置是**扫描停下时那个字符**的位置（不是 `.`、不是词元起点）：
    // `0x1.8` 停下时游标在换行上（1:6）。
    assert_eq!(
        scan1("0x1.8"),
        (
            bad("0x1.8", LitKind::Float),
            vec![(1, 6, "LEX-BAD-EXPONENT")]
        )
    );
    // 二进制/八进制**不能有小点**（RADIX-POINT 报在**小点自己**的位置 1:4）。
    assert_eq!(
        scan1("0b1.0"),
        (
            bad("0b1.0", LitKind::Float),
            vec![(1, 4, "LEX-BAD-RADIX-POINT")]
        )
    );
}

// ── ④ 虚数后缀（LEX-11）─────────────────────────────────────────────

#[test]
fn 虚数后缀() {
    for s in ["3i", "1.5i", "0x1p-2i"] {
        assert_eq!(scan1(s), (ok_imag(s), vec![]), "输入 {s:?}");
    }
}

// ── ⑤ RF3：下划线出现在**指数部分** ─────────────────────────────────

#[test]
fn 下划线出现在指数部分() {
    // ★ RF3 —— 期望值**全部来自 oracle**（计划明令"不得自行发明规则"）：
    // 下划线只要在指数里"分隔连续数码"就是**合法**的；
    // `1e_1` / `1e1_` 非法，报在 `_` 自己的位置上。
    for s in ["1e1_0", "0x1p1_0", "1_0e1_0"] {
        assert_eq!(scan1(s), (ok_float(s), vec![]), "输入 {s:?}（合法）");
    }
    assert_eq!(
        scan1("1e_1"),
        (bad("1e_1", LitKind::Float), vec![(1, 3, "LEX-BAD-SEP")])
    );
    assert_eq!(
        scan1("1e1_"),
        (bad("1e1_", LitKind::Float), vec![(1, 4, "LEX-BAD-SEP")])
    );
}

#[test]
fn 指数里只有下划线时报指数缺数字而不是分隔符() {
    // `digsep` 在指数分支是 **`=`** 不是 `|=`（官方原文）：
    // 尾数的"有数字"位被**丢弃**，只保留"有 `_`"位 ⇒ `digsep&1 == 0` ⇒ 报「指数缺数字」；
    // 而 `ok` 已假 ⇒ `invalidSep` 那一步**根本不会被检查** ⇒ **没有** BAD-SEP。
    //
    // 若把 `=` 写成 `|=`：`digsep&1` 会被尾数置位 ⇒ **少报一条 LEX-BAD-EXPONENT**，
    // 且会**多报**一条 BAD-SEP —— 两个方向都错这条测试都红。
    let cases: &[(&str, u32)] = &[("1e_", 4), ("1.5e_", 6), ("1_5e_", 6)];
    for &(s, col) in cases {
        assert_eq!(
            scan1(s),
            (bad(s, LitKind::Float), vec![(1, col, "LEX-BAD-EXPONENT")]),
            "输入 {s:?}"
        );
    }
    // 对照：指数里**有数字**、只有收尾的 `_` 非法 ⇒ 才轮到 BAD-SEP。
    assert_eq!(
        scan1("1_5e1_"),
        (bad("1_5e1_", LitKind::Float), vec![(1, 6, "LEX-BAD-SEP")])
    );
}

// ── ⑥ 畸形字面量：报错但**照常产出** ────────────────────────────────

#[test]
fn 畸形字面量仍产出词元且_bad_为真() {
    // 恢复纪律：从不"失败退出"，总是"给你一个可能畸形的词元 + 一条诊断"。
    // 每条都断言**两件事**：词元（含 `bad`）+ 诊断位置 —— 只断言"报错了"是假通过。
    let cases: &[(&str, LitKind, u32, &str)] = &[
        // 进制前缀后没数字：报在**扫完时那个字符**上
        ("0x", LitKind::Int, 3, "LEX-LIT-NO-DIGITS"),
        ("0b", LitKind::Int, 3, "LEX-LIT-NO-DIGITS"),
        ("0x_", LitKind::Int, 4, "LEX-LIT-NO-DIGITS"),
        ("0x.p1", LitKind::Float, 4, "LEX-LIT-NO-DIGITS"),
        // 进制里的非法数字：报在**那个数字**上（偏移由 `digits` 记、事后定位）
        ("089", LitKind::Int, 2, "LEX-BAD-DIGIT"),
        ("0b12", LitKind::Int, 4, "LEX-BAD-DIGIT"),
        // 指数问题
        ("1e", LitKind::Float, 3, "LEX-BAD-EXPONENT"),
        ("1e+", LitKind::Float, 4, "LEX-BAD-EXPONENT"),
        ("0x1p", LitKind::Float, 5, "LEX-BAD-EXPONENT"),
        ("1p3", LitKind::Float, 2, "LEX-BAD-EXPONENT"),
        ("0b1e5", LitKind::Float, 4, "LEX-BAD-EXPONENT"),
    ];
    for &(s, k, col, code) in cases {
        assert_eq!(scan1(s), (bad(s, k), vec![(1, col, code)]), "输入 {s:?}");
    }
}

#[test]
fn 非法数字的报错位置相对词元起点() {
    // `089` 单独出现时报 1:2（词元起点 1 + 偏移 1）。
    // 官方算的是 `col(非法字符) - col(词元起点)` —— 把同一个字面量推到第 4 列，
    // 报错必须跟着**平移**到 1:5。写死"从 1 开始数"的实现这条会红。
    let mut lx = Lexer::new(b"x:=089".to_vec(), "t.go");
    assert!(matches!(lx.scan_ident(), Some(TokenKind::Ident(_))));
    assert!(matches!(lx.scan_punct(), Some(PunctScan::Token(_))));
    let kind = lx.scan_number(false);
    let codes: Vec<_> = lx
        .take_diags()
        .into_sorted()
        .iter()
        .map(|d| (d.pos.line, d.pos.col, d.code.as_str()))
        .collect();
    assert_eq!(
        (kind, codes),
        (bad("089", LitKind::Int), vec![(1, 5, "LEX-BAD-DIGIT")])
    );
}

// ── ⑦ 点号开头的数字：T-L09 交接 ────────────────────────────────────

#[test]
fn 点号开头的数字由交接进入() {
    // `.5` 是**一个** Float 字面量：`scan_punct` 认出"`. 后面跟数字"，
    // 把游标**退回 `.` 上**交给本模块（约定：数字扫描总是从字面量第一个字符开始）。
    let mut lx = Lexer::new(b"x:=.5".to_vec(), "t.go");
    let _ = lx.scan_ident(); // 吃掉 `x`
    let _ = lx.scan_punct(); // 吃掉 `:=`
    let dot_at = lx.pos();
    assert!(matches!(lx.scan_punct(), Some(PunctScan::LeadingDotNumber)));
    assert_eq!(lx.peek(), Some('.'), "交接后游标必须还在 `.` 上（未消耗）");
    assert_eq!(lx.pos(), dot_at, "位置必须退回 `.` 处");
    let kind = lx.scan_number(true);
    assert!(lx.take_diags().into_sorted().is_empty());
    assert_eq!(kind, ok_float(".5"), "lit 必须**含**那个 `.`");
    assert_eq!(lx.pos().col, 6, "扫完后游标停在换行上");
}

#[test]
fn 连续的点号数字逐个切出() {
    // `1.5.6` ⇒ `1.5` + `.6`（两个 Float）：第一个字的尾数不会把第二个点吃掉。
    let mut lx = Lexer::new(b"1.5.6".to_vec(), "t.go");
    assert_eq!(lx.scan_number(false), ok_float("1.5"));
    assert!(matches!(lx.scan_punct(), Some(PunctScan::LeadingDotNumber)));
    assert_eq!(lx.scan_number(true), ok_float(".6"));
    assert!(lx.take_diags().into_sorted().is_empty());
}
