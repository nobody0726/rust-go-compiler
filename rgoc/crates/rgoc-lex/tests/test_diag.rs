//! T-L02 的验收测试 —— 诊断与排序（C1 §3 / §4）。
//!
//! 判据来源：`M1-design.md` §7.2 的诊断代号表、`C1` §4 的排序规则。
//! 四条断言分别钉住：排序键顺序、同位置唯一性、代号前缀、**声明顺序与字典序一致**。

use rgoc_lex::{Diag, DiagBag, DiagCode, FileId, Pos};

/// 测试用的位置构造器（`FileId` 的合法来源只有 `intern`，
/// 这里用 `from_index` 是为了不引入文件表）。
fn p(file: u32, line: u32, col: u32) -> Pos {
    Pos {
        file: FileId::from_index(file),
        line,
        col,
    }
}

fn d(file: u32, line: u32, col: u32, code: DiagCode) -> Diag {
    Diag {
        pos: p(file, line, col),
        code,
        message: format!("{} 的说明", code.as_str()),
    }
}

#[test]
fn 代号表恰好十九个且无重复() {
    assert_eq!(DiagCode::ALL.len(), 19);
    let mut s = DiagCode::ALL;
    s.sort();
    for w in s.windows(2) {
        assert_ne!(w[0], w[1], "代号不得重复：{}", w[0].as_str());
    }
}

#[test]
fn diag_bag_into_sorted_按_文件_行_列_code() {
    let mut bag = DiagBag::new();
    // 故意乱序插入
    bag.push(d(1, 1, 1, DiagCode::BadChar));
    bag.push(d(0, 2, 1, DiagCode::InvalidNul));
    bag.push(d(0, 1, 9, DiagCode::BadSep));
    bag.push(d(0, 1, 2, DiagCode::LitNoDigits));

    let sorted = bag.into_sorted();

    // C1 §4 的排序键：file → line → col → code
    assert_eq!(sorted[0].pos, p(0, 1, 2));
    assert_eq!(sorted[1].pos, p(0, 1, 9));
    assert_eq!(sorted[2].pos, p(0, 2, 1));
    assert_eq!(sorted[3].pos, p(1, 1, 1));
}

#[test]
fn 同位置的诊断按_code_排序_保证输出唯一() {
    // 同一位置两条诊断 —— 顺序若取决于插入顺序，输出就不确定（C1 §4.2）
    let mut a = DiagBag::new();
    a.push(d(0, 1, 1, DiagCode::LitNoDigits));
    a.push(d(0, 1, 1, DiagCode::BadChar));

    let mut b = DiagBag::new();
    b.push(d(0, 1, 1, DiagCode::BadChar));
    b.push(d(0, 1, 1, DiagCode::LitNoDigits));

    let ca: Vec<DiagCode> = a.into_sorted().into_iter().map(|x| x.code).collect();
    let cb: Vec<DiagCode> = b.into_sorted().into_iter().map(|x| x.code).collect();

    assert_eq!(ca, cb, "插入顺序不得影响输出顺序");
    assert_eq!(ca, [DiagCode::BadChar, DiagCode::LitNoDigits]);
}

#[test]
fn diag_code_的_as_str_前缀均为_lex_() {
    for c in DiagCode::ALL {
        let s = c.as_str();
        assert!(s.starts_with("LEX-"), "前缀必须是 LEX-：{s}");
        assert_eq!(
            s,
            s.to_uppercase(),
            "代号必须全大写（可排序、无大小写混排陷阱）：{s}"
        );
    }
}

#[test]
fn diag_code_的声明顺序与_as_str_字典序一致() {
    // `Ord` 是声明顺序，而 golden 与 C1 §4.2 按【字符串】排。
    // 两者不一致时，「同位置按 code 排序」在 Rust 侧与 golden 侧会给出不同顺序 ——
    // 那种不一致只在两条诊断同位置时才暴露，极难定位。
    let mut by_ord = DiagCode::ALL;
    by_ord.sort();

    let mut by_str = DiagCode::ALL;
    by_str.sort_by_key(|c| c.as_str());

    let ord_names: Vec<&str> = by_ord.iter().map(|c| c.as_str()).collect();
    let str_names: Vec<&str> = by_str.iter().map(|c| c.as_str()).collect();
    assert_eq!(ord_names, str_names);
}

#[test]
fn diag_bag_的_len_与_is_empty_跟随_push() {
    let mut bag = DiagBag::new();
    assert!(bag.is_empty());
    assert_eq!(bag.len(), 0);
    bag.push(d(0, 1, 1, DiagCode::BadChar));
    assert!(!bag.is_empty());
    assert_eq!(bag.len(), 1);
}
