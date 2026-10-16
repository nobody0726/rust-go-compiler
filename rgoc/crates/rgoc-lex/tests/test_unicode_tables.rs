//! `unicode_tables` 的验收测试（T-L05，D-M1-6）。
//!
//! # 这张表为什么不能"用 Rust std 凑"
//!
//! 官方 scanner 的字符分类是（`scanner.go` 的 `atIdentChar`）：
//!
//! ```go
//! case unicode.IsLetter(s.ch) || s.ch == '_':
//! case unicode.IsDigit(s.ch):
//! ```
//!
//! 而 Rust std 的近似谓词**都更宽**，实测差异（本轮量化）：
//!
//! | 谓词 | Go（oracle） | Rust std | 差异 |
//! |---|---|---|---|
//! | 字母 `IsLetter`(L*) | 145,672 码点 | `is_alphabetic` 147,421 | **多 1,749**（全是 Mn 组合符） |
//! | 数字 `IsDigit`(Nd) | 770 码点 | `is_numeric` 1,924 | **多 1,154**（全是 No） |
//! | 大写 `IsUpper`(Lu) | 1,886 码点 | `is_uppercase` 2,006 | **多 120**（5 段 Other_Uppercase，M3/Task 26 量化） |
//!
//! **后果是静默的**，但**不在词元边界上**（F29 实测更正）：官方 `atIdentChar` 对
//! 「不是字母、也不是 Nd」的非 ASCII 字符**报错但仍然吸收** ⇒ `x²` 在 Go 里是
//! **一个** `name` **外加一条** `LEX-BAD-IDENT-CHAR`；而用 `is_numeric` 时 `²` 被当成
//! 数字 ⇒ **不报错** ⇒ **少一条诊断**。
//!
//! **少一条诊断比「词元切错」更难发现** —— 切错了至少词元数会变。
//!
//! 所以本文件里那几条"**拒绝 Rust std 会接受的那批**"是核心断言，
//! 它们同时**反证** Rust std 确实会接受 —— 否则那几条断言可能毫无信息量（恒真）。

use rgoc_lex::unicode_tables::{
    DIGIT_RANGES, LETTER_RANGES, UPPER_RANGES, is_digit, is_letter, is_upper,
};

// ── ① 覆盖：官方样本里的非 ASCII 标识符 ──────────────────────────────

#[test]
fn 字母表覆盖_官方样本里的非_ascii_标识符() {
    // 依据：M1-design.md §8.3 提到的 sampleTokens 表明确含这几个
    for c in ['ŝ', '日', '本', '語', '中'] {
        assert!(is_letter(c), "{c} 应当是字母");
    }
    // 反证：普通 ASCII 也仍然要能过（表把 a-zA-Z 一并覆盖了，没被生成器弄丢）
    for c in ['a', 'z', 'A', 'Z'] {
        assert!(is_letter(c), "{c} 应当是字母");
    }
}

#[test]
fn 数字表覆盖_官方样本里的非_ascii_数字() {
    // sampleTokens 里的 `a۰۱۸` / `foo६४` / `bar９８７６` 就是靠这几个码点
    for c in ['۰', '۱', '۸'] {
        assert!(is_digit(c), "Arabic-Indic {c} 应当是数字");
    }
    for c in ['६', '४'] {
        assert!(is_digit(c), "Devanagari {c} 应当是数字");
    }
    for c in ['0', '9'] {
        assert!(is_digit(c), "{c} 应当是数字");
    }
}

// ── ② 拒真：Rust std 会接受、而 Go 不接受的码点 ──────────────────────

#[test]
fn 字母表_拒绝_rust_std_会接受的那批() {
    // U+0345 COMBINING GREEK YPOGEGRAMMENI：类别 Mn，Rust 的 Alphabetic 含它，Go 的 L* 不含。
    // 若这里用了 `char::is_alphabetic`：`x\u{0345}` 仍会是**一个**标识符（边界不变），
    // 但**少一条** `LEX-BAD-IDENT-CHAR` —— 诊断层面的静默错误，比边界错更难发现。
    let c = '\u{0345}';
    assert!(!is_letter(c), "U+0345 不该是字母（Go 的 IsLetter 不含 Mn）");
    assert!(
        c.is_alphabetic(),
        "反证失败：Rust std 竟然也不认为它是字母，本断言失去意义"
    );
}

#[test]
fn 数字表_拒绝_rust_std_会接受的那批() {
    // ²³¹ 是 No（上标数字），¼½¾ 是 No（分数）—— Rust 的 is_numeric 是 N*，全都放行。
    for c in ['²', '³', '¹', '¼', '½', '¾'] {
        assert!(!is_digit(c), "{c} 不是 Nd，不该是数字");
        assert!(
            c.is_numeric(),
            "反证失败：Rust std 不认为 {c} 是数字，本断言失去意义"
        );
    }
}

// ── ③ 表本身的不变式（binary_search 的前提）──────────────────────────

#[test]
fn 三张表都是有序且互不重叠的() {
    // `lookup` 用 binary_search —— 表一旦乱序或有重叠，查找会**静默**给出错误答案
    // （不 panic、不报错，只是偶尔答错）。所以这条断言是查找正确性的前提，不是洁癖。
    for (name, table) in [
        ("LETTER", LETTER_RANGES),
        ("DIGIT", DIGIT_RANGES),
        ("UPPER", UPPER_RANGES),
    ] {
        for i in 0..table.len() {
            let (lo, hi) = table[i];
            assert!(lo <= hi, "{name}[{i}] 区间反了：({lo:#x}, {hi:#x})");
            if i > 0 {
                let (_, prev_hi) = table[i - 1];
                assert!(
                    prev_hi < lo,
                    "{name} 第 {i} 项与前一项重叠或乱序：{prev_hi:#x} >= {lo:#x}"
                );
            }
        }
    }
}

#[test]
fn 三张表的区间数与实测值一致() {
    // 与 `M1-design.md` §6 / §10.3 的记录对齐（UPPER 是 M3/Task 26 加的）。
    // 这条同时防"生成器中途截断"—— 截断后表仍有序、仍能查出部分码点，只是**少了尾巴**，
    // 不数一遍是看不出来的。
    assert_eq!(LETTER_RANGES.len(), 684, "字母区间数（unicode.IsLetter）");
    assert_eq!(
        DIGIT_RANGES.len(),
        72,
        "数字区间数（unicode.IsDigit，即 Nd）"
    );
    assert_eq!(
        UPPER_RANGES.len(),
        655,
        "大写区间数（unicode.IsUpper，即 Lu）"
    );
}

#[test]
fn 区间总数与码点数也对得上() {
    // 区间数对得上、但边界写错（把 `hi` 写成 `lo`）时，上面那条计数断言抓不到。
    // 所以再钉一次**码点总数**（本轮由 oracle 实测）。
    let count = |t: &[(u32, u32)]| t.iter().map(|&(lo, hi)| (hi - lo + 1) as u64).sum::<u64>();
    assert_eq!(
        count(LETTER_RANGES),
        145_672,
        "字母码点数（Go unicode.IsLetter）"
    );
    assert_eq!(count(DIGIT_RANGES), 770, "数字码点数（Go unicode.IsDigit）");
    assert_eq!(
        count(UPPER_RANGES),
        1_886,
        "大写码点数（Go unicode.IsUpper）"
    );
}

// ── ④ `_` 的特殊性（Go 规范，不是 Unicode 属性）──────────────────────

#[test]
fn 下划线算字母_但表里没有它() {
    // Go 规范：`letter = unicode_letter | "_"` —— `_` 是**语言层面**算字母的，
    // 而 Unicode 没把它归入 L*。所以它由访问函数处理，**不并进生成表** ——
    // 这样表能逐字对着 oracle 校验（"== unicode.IsLetter 的区间，一条不多一条不少"）。
    assert!(is_letter('_'), "`_` 必须算字母（官方 isLetter 含它）");
    assert!(
        !LETTER_RANGES
            .iter()
            .any(|&(lo, hi)| lo <= 0x5F && 0x5F <= hi),
        "`_`(U+005F) 不该出现在生成表里"
    );
    assert!(!'_'.is_alphabetic(), "反证：Unicode 不认为 `_` 是字母");
}

#[test]
fn 下划线不是数字() {
    assert!(!is_digit('_'));
}

// ── ⑤ 与 Rust std 的"边界一致"处也要成立 ────────────────────────────

#[test]
fn 表里没有代理区码点() {
    // `char` 本身排除了代理区，但生成器是**按 u32 遍历**的 —— 若忘了跳过 0xD800..=0xDFFF，
    // 表里会出现永远查不到的区间（`char` 造不出来），也就是**死数据**。
    for (name, table) in [
        ("LETTER", LETTER_RANGES),
        ("DIGIT", DIGIT_RANGES),
        ("UPPER", UPPER_RANGES),
    ] {
        for &(lo, hi) in table {
            assert!(
                hi < 0xD800 || lo > 0xDFFF,
                "{name} 里含代理区区间：({lo:#x}, {hi:#x})"
            );
        }
    }
}

#[test]
fn 三张表都止于_unicode_最大码点之内() {
    for (name, table) in [
        ("LETTER", LETTER_RANGES),
        ("DIGIT", DIGIT_RANGES),
        ("UPPER", UPPER_RANGES),
    ] {
        let last = table.last().expect("表不该为空");
        assert!(
            last.1 <= 0x10FFFF,
            "{name} 超出 Unicode 上限：{:#x}",
            last.1
        );
    }
}

// ── ⑥ IsUpper（M3/Task 26：导出标识符判定）─────────────────────────────

/// 大写表要覆盖官方样本里真会出现的非 ASCII 大写字母。
#[test]
fn 大写表覆盖_非_ascii_大写字母() {
    // 官方 `go/token.IsExported` 的样本：É（LATIN CAPITAL E WITH ACUTE）、
    // Ł（LATIN CAPITAL L WITH STROKE）、Ω（GREEK CAPITAL OMEGA）、
    // 日（CJK 没有大小写之分 ⇒ **不算**大写，`IsExported("日x")` = false）。
    for c in ['É', 'Ł', 'Ω', 'Á', 'Ξ'] {
        assert!(is_upper(c), "{c} 应当是大写（Lu）");
    }
    for c in ['a', 'z', 'é', 'ω', '中', '日'] {
        assert!(!is_upper(c), "{c} 不该是大写（Lu）");
    }
    // ASCII 也必须一并覆盖（生成器没把 a-z / A-Z 弄丢）
    for c in ['A', 'Z'] {
        assert!(is_upper(c), "{c} 应当是大写");
    }
}

/// **拒绝 Rust std 的 `char::is_uppercase` 会多收的那 120 个码点**。
///
/// 这是本节的核型断言：std 的 `Uppercase` 是 derived 属性（Lu ∪ Other_Uppercase），
/// 多出 5 段共 120 个码点。若这里用了 std 的谓词，导出面就会把 `Ⅰx` 这种
/// **不导出**的名字判成导出（Go `unicode.IsUpper('Ⅰ')` = false）。
#[test]
fn 大写表_拒绝_rust_std_会接受的那批() {
    // 五段的第一码点各取一个（由 `m3-26` 探针实测：Rs 2006 / Go 1886 / 多 120）
    const EXTRAS: [(char, &str); 5] = [
        ('\u{2160}', "罗马数字 Ⅰ（Nl）"),
        ('\u{24B6}', "带圈 Ⓐ（So）"),
        ('\u{1F130}', "带框 🄰（So）"),
        ('\u{1F150}', "反白 🅐（So）"),
        ('\u{1F170}', "方块 🅰（So）"),
    ];
    for (c, what) in EXTRAS {
        assert!(!is_upper(c), "{what} 不是 Lu，不该判成大写");
        assert!(
            c.is_uppercase(),
            "反证失败：Rust std 也不认为 {what} 是大写，本断言失去意义"
        );
    }
}

#[test]
fn 下划线不是大写() {
    // Go：`unicode.IsUpper('_')` = false ⇒ `IsExported("_")` = false（与「`_` 不绑定」两回事）
    assert!(!is_upper('_'));
    assert!(!'_'.is_uppercase(), "反证：std 也不认为 `_` 是大写");
}
