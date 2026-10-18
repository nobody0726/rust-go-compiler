//! `T-CK-16` 底座：`DiagCode` → `C8` 类别投影的**穷尽性**守卫。
//!
//! # 判据（`M4-tests.md` §4 / `C8` §3–§4）
//!
//! * 类别枚举 = **26 类、封闭集合、字典序**；`ALL_CATEGORIES` 与 `C8` §3 的
//!   文档枚举块**逐字一致**（文档漂了就红 —— 同 oracle 侧
//!   `TestC8CategoryVocabulary` 的纪律）。
//! * `category_of` 对 `DiagCode::ALL` 的**每个**码都给类别，且落在枚举内
//!   （穷尽性由编译期 `match` 无通配强制；本测试断言运行期值域）。
//! * **Ruling 11-1**（对 `M4-plan.md` Task 11 的「`Other` 无码映射」条目的
//!   修订，走 `C8` §3 的「除修订」口）：`other` 是**逃逸口**，其值域 = 恰好
//!   **4 个「无官方语义」码** —— `ChkUnimplemented`（骨架占位）+
//!   `ExceedsFileLimit` / `SynNestingTooDeep` / `TypNestingTooDeep`
//!   （本仓实现限制，官方无对应行为）。把边界码硬塞进真实类别 = 用类别
//!   「答错」（本仓纪律：「不判」与「答错」是两件事）；`T-CK-94` 的
//!   「入选语料出现 `other` 即红」对它们**恰好正确** —— 边界码出现在入选
//!   语料上本来就无法与 oracle 对齐。每类计数快照钉住这次定案；后续新码
//!   落位时必须同步 `category_of` 与快照（编译开路，快照收口）。

use std::collections::BTreeMap;

use rgoc_check::diag::{ALL_CATEGORIES, Category, category_of};
use rgoc_lex::DiagCode;

/// 26 类的字典序 token（`C8` §3 冻结；本测试内的逐字副本）。
const TOKENS: [&str; 26] = [
    "assign",
    "builtin",
    "comma-ok",
    "convert",
    "cycle",
    "decl",
    "implements",
    "import",
    "index",
    "not-type",
    "op-invalid",
    "other",
    "overflow",
    "parse",
    "range",
    "ret",
    "selector",
    "shift",
    "slice",
    "stmt",
    "terminate",
    "type-arg",
    "undef",
    "unused",
    "unsafe",
    "version",
];

#[test]
fn t_ck_16_类别枚举_26_类且逐字典序() {
    assert_eq!(ALL_CATEGORIES.len(), 26, "类别枚举冻结为 26 类");
    let got: Vec<&str> = ALL_CATEGORIES.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        got,
        TOKENS.to_vec(),
        "ALL_CATEGORIES 必须按字典序与冻结表一致"
    );
    // 顺带：token 不许重复
    let uniq: std::collections::BTreeSet<&str> = got.iter().copied().collect();
    assert_eq!(uniq.len(), 26, "token 不得重复");
}

#[test]
fn t_ck_16_c8_文档枚举块与_all_categories_逐字一致() {
    let doc = std::fs::read_to_string("../../../docs/contracts/C8-check-diags.md")
        .expect("读 C8 契约（相对 crate 目录）");
    let start = doc.find("## 3. 类别枚举").expect("C8 缺 §3");
    let rest = &doc[start..];
    let end = rest.find("## 4.").expect("C8 缺 §4");
    let sec = &rest[..end];
    let fence = sec.find("```text\n").expect("C8 §3 缺 ```text 块");
    let after = &sec[fence + "```text\n".len()..];
    let close = after.find("```").expect("C8 §3 缺 ``` 收口");
    let mut doc_tokens: Vec<&str> = after[..close].split_whitespace().collect();
    doc_tokens.sort_unstable();
    let mut ours: Vec<&str> = TOKENS.to_vec();
    ours.sort_unstable();
    assert_eq!(
        doc_tokens, ours,
        "C8 §3 文档枚举 == 冻结副本 == ALL_CATEGORIES"
    );
}

#[test]
fn t_ck_16_投影穷尽_全部码落进枚举() {
    for code in DiagCode::ALL {
        let cat = category_of(code);
        assert!(
            ALL_CATEGORIES.contains(&cat),
            "{code:?} 投影到 {cat:?}，不在 26 类枚举内"
        );
    }
    // 总数守恒：69 个码各有恰好一个类别（Task 12 后 69）
    assert_eq!(DiagCode::ALL.len(), 69, "码表总数（Task 12 后 69）");
}

#[test]
fn t_ck_16_other_值域恰好是四个无官方语义码() {
    use DiagCode::*;
    let boundary = [
        ChkUnimplemented,
        ExceedsFileLimit,
        SynNestingTooDeep,
        TypNestingTooDeep,
    ];
    for code in boundary {
        assert_eq!(category_of(code), Category::Other, "{code:?} 是能力边界码");
    }
    for code in DiagCode::ALL {
        let expected_other = boundary.contains(&code);
        assert_eq!(
            category_of(code) == Category::Other,
            expected_other,
            "{code:?} 的 other 归属与定案不符（Ruling 11-1）"
        );
    }
}

#[test]
fn t_ck_16_抽样映射向量() {
    use DiagCode::*;
    let vectors: [(DiagCode, Category); 19] = [
        (BadChar, Category::Parse),
        (SynExpectedSemicolon, Category::Parse),
        (SynBranchBreak, Category::Stmt),
        (SynGotoJumpsOverDecl, Category::Stmt),
        (SynLabelUnused, Category::Unused),
        (SynLabelUndefined, Category::Undef),
        (SynLabelDuplicate, Category::Decl),
        (TypUndeclaredName, Category::Undef),
        (TypDuplicateDecl, Category::Decl),
        (TypDivByZero, Category::OpInvalid),
        (TypInvalidShiftCount, Category::Shift),
        (TypInvalidConversion, Category::Convert),
        (TypNumericOverflow, Category::Overflow),
        (TypFeatureRequiresGoVersion, Category::Version),
        (TypBrokenImport, Category::Import),
        (TypInvalidRecursiveType, Category::Cycle),
        (TypInvalidBlank, Category::NotType),
        (TypInvalidTypeArg, Category::Implements),
        (TypWrongTypeArgCount, Category::TypeArg),
    ];
    for (code, want) in vectors {
        assert_eq!(category_of(code), want, "{code:?}");
    }
}

#[test]
fn t_ck_16_每类计数快照() {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for code in DiagCode::ALL {
        *counts.entry(category_of(code).as_str()).or_default() += 1;
    }
    // 定案快照（Ruling 11-1；Task 12 补 implements/type-arg 首码）：
    // 改动 `category_of` 或新增码 ⇒ 这里逐项对账。
    let want: BTreeMap<&'static str, usize> = BTreeMap::from([
        ("assign", 1),
        ("convert", 2),
        ("cycle", 3),
        ("decl", 8),
        ("implements", 1),
        ("import", 3),
        ("not-type", 1),
        ("op-invalid", 1),
        ("other", 4),
        ("overflow", 2),
        ("parse", 25),
        ("shift", 2),
        ("stmt", 7),
        ("type-arg", 1),
        ("undef", 3),
        ("unused", 2),
        ("version", 3),
    ]);
    assert_eq!(counts, want, "每类计数快照");
    let total: usize = counts.values().sum();
    assert_eq!(total, DiagCode::ALL.len(), "并集覆盖全部码");
}
