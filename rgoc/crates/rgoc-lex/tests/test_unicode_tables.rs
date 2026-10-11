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
//!
//! **后果是静默的**，但**不在词元边界上**（F29 实测更正）：官方 `atIdentChar` 对
//! 「不是字母、也不是 Nd」的非 ASCII 字符**报错但仍然吸收** ⇒ `x²` 在 Go 里是
//! **一个** `name` **外加一条** `LEX-BAD-IDENT-CHAR`；而用 `is_numeric` 时 `²` 被当成
//! 数字 ⇒ **不报错** ⇒ **少一条诊断**。
//!
//! **少一条诊断比「词元切错」更难发现** —— 切错了至少词元数会变。
//!
//! 所以本文件里那两条"**拒绝 Rust std 会接受的那批**"是核心断言，
//! 它们同时**反证** Rust std 确实会接受 —— 否则那两条断言可能毫无信息量（恒真）。

use rgoc_lex::unicode_tables::{DIGIT_RANGES, LETTER_RANGES, is_digit, is_letter};

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
fn 两张表都是有序且互不重叠的() {
    // `lookup` 用 binary_search —— 表一旦乱序或有重叠，查找会**静默**给出错误答案
    // （不 panic、不报错，只是偶尔答错）。所以这条断言是查找正确性的前提，不是洁癖。
    for (name, table) in [("LETTER", LETTER_RANGES), ("DIGIT", DIGIT_RANGES)] {
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
fn 两张表的区间数与实测值一致() {
    // 与 `M1-design.md` §6 / §10.3 的记录对齐。
    // 这条同时防"生成器中途截断"—— 截断后表仍有序、仍能查出部分码点，只是**少了尾巴**，
    // 不数一遍是看不出来的。
    assert_eq!(LETTER_RANGES.len(), 684, "字母区间数（unicode.IsLetter）");
    assert_eq!(
        DIGIT_RANGES.len(),
        72,
        "数字区间数（unicode.IsDigit，即 Nd）"
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
    for (name, table) in [("LETTER", LETTER_RANGES), ("DIGIT", DIGIT_RANGES)] {
        for &(lo, hi) in table {
            assert!(
                hi < 0xD800 || lo > 0xDFFF,
                "{name} 里含代理区区间：({lo:#x}, {hi:#x})"
            );
        }
    }
}

#[test]
fn 两张表都止于_unicode_最大码点之内() {
    for (name, table) in [("LETTER", LETTER_RANGES), ("DIGIT", DIGIT_RANGES)] {
        let last = table.last().expect("表不该为空");
        assert!(
            last.1 <= 0x10FFFF,
            "{name} 超出 Unicode 上限：{:#x}",
            last.1
        );
    }
}
