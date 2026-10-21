//! `T-CK-16` 底座：`DiagCode` → `C8` 类别投影的**穷尽性**守卫。
//!
//! # 判据（`M4-tests.md` §4 / `C8` §3–§4）
//!
//! * 类别枚举 = **26 类、封闭集合、字典序**；`ALL_CATEGORIES` 与 `C8` §3 的
//!   文档枚举块**逐字一致**（文档漂了就红 —— 同 oracle 侧
//!   `TestC8CategoryVocabulary` 的纪律）。
//! * `category_of` 对 `DiagCode::ALL` 的**每个**码都给类别，且落在枚举内
//!   （穷尽性由编译期 `match` 无通配强制；本测试断言运行期值域）。
//! * **Ruling 11-1 + Ruling 20-1**（后者纪要：Task 20 移除骨架占位码
//!   `ChkUnimplemented` —— 真身会话接管）：`other` 是**逃逸口**，其值域 = 恰好
//!   **3 个「无官方语义」码** —— `ExceedsFileLimit` / `SynNestingTooDeep` /
//!   `TypNestingTooDeep`（本仓实现限制，官方无对应行为）。把边界码硬塞进真实
//!   类别 = 用类别「答错」（本仓纪律：「不判」与「答错」是两件事）；`T-CK-94`
//!   的「入选语料出现 `other` 即红」对它们**恰好正确** —— 边界码出现在入选
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
    // 总数守恒：101 个码各有恰好一个类别（M4/Task 34 后 101）
    assert_eq!(DiagCode::ALL.len(), 101, "码表总数（M4/Task 34 后 101）");
}

#[test]
fn t_ck_16_other_值域恰好是三个实现限制码() {
    use DiagCode::*;
    // Ruling 11-1 原文 4 码，经 Ruling 20-1 收窄为 3：`ChkUnimplemented` 随
    // Task 20 的检查会话真身落地退出（没有触发者的码不许进枚举）。
    let boundary = [ExceedsFileLimit, SynNestingTooDeep, TypNestingTooDeep];
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
    let vectors: [(DiagCode, Category); 38] = [
        (ChkDuplicateLitField, Category::Decl),
        (ChkDuplicateLitIndex, Category::Decl),
        (ChkInvalidLit, Category::Decl),
        (ChkInvalidLitField, Category::Selector),
        (ChkInvalidLitIndex, Category::Index),
        (ChkStructLitCount, Category::Assign),
        (ChkInvalidBuiltinArg, Category::Builtin),
        (ChkUnsupportedUnsafe, Category::Unsafe),
        (TypInvalidInitDecl, Category::Decl),
        (TypInvalidMainDecl, Category::Decl),
        (TypInvalidInitSig, Category::Ret),
        (TypInvalidMainSig, Category::Ret),
        (ChkInvalidIndex, Category::Index),
        (ChkInvalidOperation, Category::OpInvalid),
        (ChkInvalidSelector, Category::Selector),
        (ChkInvalidSlice, Category::Slice),
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
        (TypInvalidInstanceCycle, Category::Cycle),
        (TypUnsupportedPendingInstanceArg, Category::TypeArg),
        (TypInvalidBlank, Category::NotType),
        (TypInvalidTypeArg, Category::Implements),
        (TypWrongTypeArgCount, Category::TypeArg),
        (TypCannotInferTypeArgs, Category::TypeArg),
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
    // 定案快照（Ruling 11-1 / Ruling 20-1；Task 12 补 implements/type-arg 首码；
    // M4/Task 14 给 type-arg 加推断失败码；M4/Task 20 other 4→3；
    // M4/Task 21 新增 `ChkInvalidOperation` ⇒ op-invalid 1→2、总数 71→72；
    // M4/Task 23 新增 `ChkInvalidIndex` / `ChkInvalidSlice` ⇒ index 0→1、
    // slice 0→1、总数 72→74；M4/Task 24 新增 `ChkInvalidSelector` ⇒
    // selector 0→1、总数 74→75；M4/Task 25 新增 11 个语句面码 ⇒
    // assign 1→2、comma-ok 0→1、decl 8→9、not-type 1→2、ret 0→1、
    // stmt 7→11、terminate 0→1、unused 2→3、总数 75→86；
    // M4/Task 26 新增 `ChkInvalidIterVar` / `ChkInvalidRangeExpr` ⇒
    // range 0→2、总数 86→88；
    // M4/Task 27 新增 6 个复合字面量面码 ⇒
    // assign 2→3（`ChkStructLitCount`）、decl 9→12（`ChkInvalidLit` +
    // `ChkDuplicateLitField` + `ChkDuplicateLitIndex`）、index 1→2
    // （`ChkInvalidLitIndex`）、selector 1→2（`ChkInvalidLitField`）、
    // 总数 88→94）；
    // M4/Task 28 新增 `ChkImpossibleAssert` ⇒ comma-ok 1→2、总数 94→95）；
    // M4/Task 29 新增 `ChkInvalidBuiltinArg` ⇒ builtin 0→1、总数 95→96；
    // M4/Task 31 新增 `ChkUnsupportedUnsafe` ⇒ unsafe 0→1、总数 96→97）：
    // 改动 `category_of` 或新增码 ⇒ 这里逐项对账。
    let want: BTreeMap<&'static str, usize> = BTreeMap::from([
        ("assign", 3),
        ("builtin", 1),
        ("comma-ok", 2),
        ("convert", 2),
        ("cycle", 4),
        ("decl", 14),
        ("implements", 1),
        ("import", 3),
        ("index", 2),
        ("not-type", 2),
        ("op-invalid", 2),
        ("other", 3),
        ("overflow", 2),
        ("parse", 25),
        ("range", 2),
        ("ret", 3),
        ("selector", 2),
        ("shift", 2),
        ("slice", 1),
        ("stmt", 11),
        ("terminate", 1),
        ("type-arg", 3),
        ("undef", 3),
        ("unused", 3),
        ("unsafe", 1),
        ("version", 3),
    ]);
    assert_eq!(counts, want, "每类计数快照");
    let total: usize = counts.values().sum();
    assert_eq!(total, DiagCode::ALL.len(), "并集覆盖全部码");
}
