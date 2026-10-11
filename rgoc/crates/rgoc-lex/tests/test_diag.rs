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
fn 代号表恰好二十三个且无重复() {
    assert_eq!(DiagCode::ALL.len(), 23);
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

/// 跨语言对撞：Rust 的码表必须与 Go 侧 `allCodes` **逐字一致**。
///
/// # 为什么要跨语言去读一个 Go 文件
///
/// 这张表曾经只有 19 个代号 —— 是**凭印象**从官方报错消息里挑的，而把
/// `scanner.go` / `source.go` 里全部 `errorf`/`errorAtf`/`error` 调用点枚举出来，
/// 实际有 **25 条消息模板**。漏掉的 4 条（`LEX-BAD-DIGIT` / `LEX-BAD-RADIX-POINT` /
/// `LEX-STRING-NEWLINE` / `LEX-ESCAPE-BAD-DIGIT`）在当时**不会引发任何报错** ——
/// 只会在那 4 条消息真的出现时行为未定义。
///
/// 「两侧各维护一份清单」= 必然漂移（这正是 `AGENTS.md` 记的
/// 「写死的键列表会变成盲区」，已三次同形）。所以这里用一条断言把它们钉在一起：
/// 任何一侧改了而另一侧没改，这条测试立刻红。
///
/// 用 `CARGO_MANIFEST_DIR` 拼**绝对路径**，所以不受测试工作目录影响。
#[test]
fn diag_code_的码表与_go_侧映射表逐字一致() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tools/token-oracle/diagmap.go");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("读不到 Go 侧映射表 {}：{e}", path.display()));

    const HEAD: &str = "var allCodes = []string{";
    let start = src
        .find(HEAD)
        .unwrap_or_else(|| panic!("{} 里找不到 `{HEAD}` 声明", path.display()));
    let rest = &src[start..];
    let end = rest
        .find('}')
        .unwrap_or_else(|| panic!("{} 的 allCodes 声明未闭合", path.display()));
    let body = &rest[HEAD.len()..end];

    let mut go_codes: Vec<&str> = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let lit = line.strip_suffix(',').unwrap_or(line).trim();
        let lit = lit.trim_matches('"');
        if !lit.is_empty() {
            go_codes.push(lit);
        }
    }
    assert!(
        !go_codes.is_empty(),
        "解析 {} 的 allCodes 得到 0 条 —— 解析逻辑坏了（不是数据坏了）",
        path.display()
    );
    go_codes.sort_unstable();

    let mut rs_codes: Vec<&str> = DiagCode::ALL.iter().map(|c| c.as_str()).collect();
    rs_codes.sort_unstable();

    assert_eq!(
        rs_codes, go_codes,
        "Rust 的 DiagCode 与 Go 侧 allCodes 不一致 —— 两侧必须同步（见 `M1-design.md` §7.2 的 D-M1-11）"
    );
}
