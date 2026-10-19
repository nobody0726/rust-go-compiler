//! `T-CK-61`：版本门控**特性表**（M4/Task 17；设计 `F1`）。
//!
//! # 判据
//!
//! 官方 `go/types` 的门控是 `verifyVersionf` / `allowVersion` 两件套
//! （`version.go:56-72`）：`allowVersion` 是纯谓词，`verifyVersionf` 在拒时
//! 追加一条 `UnsupportedFeature`（消息 `"%s requires %s or later"`）。
//! 本文件的向量按**特性 × 低/高两档**打（`M4-tests.md` §2.1 第 6 组）：
//!
//! * 低档（`since - 1`）⇒ `allow == false`；报错特性 ⇒ 恰一条
//!   `TypFeatureRequiresGoVersion`，类别恒 `version`；
//! * 高档（`since`）⇒ `allow == true`，**零**诊断；
//! * `None`（不门控）与「文件不在表里」⇒ 恒允许（官方 `!version.isValid()`）。
//!
//! **唯一的非 version 错**：`comparable` 嵌入（官方 1.14 的阈值在
//! `typeset.go:243`，但报的是**重复方法**、类别 `decl`，不是 `UnsupportedFeature`）。
//! 表里保留阈值（`allow` 面），`emits_version_error()` 把它标出来 ——
//! 调用面（方法集检查）走 `allow` 自报，不走 `verify_version`。
//!
//! # 为什么冻结全表
//!
//! 阈值是**对撞判据**的一部分（`CH-3`）：表里少一行 = 少一条门禁，多一行 =
//! 把合法程序判死。所以除了逐特性低/高档，还有一条**逐项钉死**的全表快照
//! （`FROZEN`）—— 改阈值必须同时改这里，否则红。

use std::collections::HashSet;

use rgoc_check::diag::{Category, category_of};
use rgoc_check::version::{Feat, allow, verify_version};
use rgoc_lex::{DiagCode, FileId, Pos};
use rgoc_types::version::{GO_CURRENT, GoVersion, Versions};

// ── 夹具 ────────────────────────────────────────────────────────────

fn file() -> FileId {
    FileId::from_index(0)
}

fn at(f: FileId) -> Pos {
    Pos {
        base: None,
        file: f,
        line: 3,
        col: 7,
    }
}

/// 一个文件的生效版本；`None` = 不门控（`-lang` 缺省 / 非法）。
fn versions(f: FileId, minor: Option<u32>) -> Versions {
    Versions::from_entries([(f, minor.map(GoVersion::new))])
}

/// 设计 `F1` 的**全表**（特性 → 引入版本），逐项冻结。
///
/// 出处（`go_types` 行号来自语料快照 `go1.27.1`）：
/// `decl.go:524/521/561/508`｜`expr.go:664`｜`literals.go:23-44`｜
/// `typeset.go:243/276/285/299`｜`builtins.go:234/587/672/737/869/891/908/928`｜
/// `conversions.go:206/219`｜`call.go:44/301/617`｜`signature.go:307`｜
/// `typexpr.go:34/277`｜`range.go:227/242`｜`resolver.go:427/448`。
const FROZEN: &[(Feat, u32)] = &[
    (Feat::AliasDecl, 9),
    (Feat::SignedShiftCount, 13),
    (Feat::UnderscoreInNumericLiteral, 13),
    (Feat::BinaryLiteral, 13),
    (Feat::Octal0oLiteral, 13),
    (Feat::HexFloatLiteral, 13),
    (Feat::ComparableEmbedding, 14),
    (Feat::UnsafeAdd, 17),
    (Feat::UnsafeSlice, 17),
    (Feat::SliceToArrayPointer, 17),
    (Feat::TypeParameter, 18),
    (Feat::FunctionInstantiation, 18),
    (Feat::TypeInstantiation, 18),
    (Feat::ImportedConstraint, 18),
    (Feat::EmbeddingConstraintInterface, 18),
    (Feat::EmbeddingInterfaceElement, 18),
    (Feat::EmbeddingNonInterfaceType, 18),
    (Feat::PredeclaredAnyComparable, 18),
    (Feat::SliceToArray, 20),
    (Feat::UnsafeSliceData, 20),
    (Feat::UnsafeString, 20),
    (Feat::UnsafeStringData, 20),
    (Feat::Clear, 21),
    (Feat::BuiltinMinMax, 21),
    (Feat::ImplicitInstantiationAsArgument, 21),
    (Feat::RangeOverInt, 22),
    (Feat::RangeOverFunc, 23),
    (Feat::GenericTypeAlias, 23),
    (Feat::NewExpr, 26),
    (Feat::PromotedFieldKey, 27),
    (Feat::GenericMethod, 27),
];

// ── ① 全表冻结 ─────────────────────────────────────────────────────

#[test]
fn t_ck_61_全表冻结_逐项阈值() {
    assert_eq!(
        Feat::ALL.len(),
        FROZEN.len(),
        "特性表条数变了 —— 改表必须同步改 FROZEN（见模块头）"
    );
    for (feat, minor) in FROZEN {
        assert_eq!(
            feat.since(),
            GoVersion::new(*minor),
            "{feat:?} 的阈值与冻结表不符"
        );
        assert!(
            feat.since().minor() >= 9 && feat.since() <= GO_CURRENT,
            "{feat:?} 的阈值越出 go1.9..={GO_CURRENT} 的合理范围"
        );
    }
    // 顺序也冻结：ALL 与 FROZEN 必须逐项对应，防止「漏一项但条数相等」。
    for (i, (feat, _)) in FROZEN.iter().enumerate() {
        assert_eq!(Feat::ALL[i], *feat, "ALL[{i}] 与冻结表不同序");
    }
}

#[test]
fn t_ck_61_特性名唯一() {
    let mut seen = HashSet::new();
    for feat in Feat::ALL {
        assert!(seen.insert(feat.name()), "特性名重复：{}", feat.name());
        assert!(!feat.name().is_empty(), "特性名不得为空");
    }
}

// ── ② 每特性低/高两档（本任务的 ≥12 特性面）────────────────────────

#[test]
fn t_ck_61_每特性低档拒_高档接() {
    let f = file();
    for feat in Feat::ALL {
        let since = feat.since();
        let low = versions(f, Some(since.minor() - 1));
        assert!(
            !allow(&low, f, feat),
            "{feat:?} 在 go1.{} 应被拒（阈值 {since}）",
            since.minor() - 1
        );
        let high = versions(f, Some(since.minor()));
        assert!(
            allow(&high, f, feat),
            "{feat:?} 在 {since} 应被接受（阈值即本体）"
        );
    }
}

#[test]
fn t_ck_61_低档报_version_类诊断_高档零诊断() {
    let f = file();
    for feat in Feat::ALL
        .iter()
        .copied()
        .filter(|x| x.emits_version_error())
    {
        let since = feat.since();

        let low = versions(f, Some(since.minor() - 1));
        let mut bag = rgoc_lex::DiagBag::new();
        assert!(
            !verify_version(&low, f, at(f), feat, "X", &mut bag),
            "{feat:?} 低档必须返回 false"
        );
        let diags = bag.into_sorted();
        assert_eq!(diags.len(), 1, "{feat:?} 低档应恰报一条");
        let d = &diags[0];
        assert_eq!(d.code, DiagCode::TypFeatureRequiresGoVersion, "{feat:?}");
        assert_eq!(d.pos, at(f), "{feat:?} 位置必须原样落在触发点");
        assert_eq!(
            category_of(d.code),
            Category::Version,
            "{feat:?} 类别必须是 version"
        );
        assert!(
            d.message.ends_with(&format!("requires {since} or later")),
            "{feat:?} 消息尾部应为官方形态：{}",
            d.message
        );

        let high = versions(f, Some(since.minor()));
        let mut bag = rgoc_lex::DiagBag::new();
        assert!(
            verify_version(&high, f, at(f), feat, "X", &mut bag),
            "{feat:?} 高档必须返回 true"
        );
        assert!(bag.is_empty(), "{feat:?} 高档不得报诊断");
    }
}

// ── ③ 不门控的两种来源 ─────────────────────────────────────────────

#[test]
fn t_ck_61_不门控_恒允许且零诊断() {
    let f = file();
    let unset = versions(f, None);
    let empty = Versions::from_entries([]);
    for feat in Feat::ALL {
        assert!(allow(&unset, f, feat), "{feat:?} 不门控应允许");
        assert!(
            allow(&empty, f, feat),
            "{feat:?} 文件不在表里应按不门控兜底"
        );
        let mut bag = rgoc_lex::DiagBag::new();
        assert!(verify_version(&unset, f, at(f), feat, "X", &mut bag));
        assert!(bag.is_empty());
    }
}

// ── ④ comparable 嵌入：阈值在表里，但不是 version 错 ───────────────

#[test]
fn t_ck_61_comparable_嵌入_阈值面与报错面分开() {
    let f = file();
    let feat = Feat::ComparableEmbedding;
    assert!(
        !feat.emits_version_error(),
        "官方报重复方法（decl），不是 version"
    );
    assert!(!allow(&versions(f, Some(13)), f, feat));
    assert!(allow(&versions(f, Some(14)), f, feat));

    // `verify_version` 仍返回**判定**，但不越俎代庖报 version 错。
    let mut bag = rgoc_lex::DiagBag::new();
    assert!(!verify_version(
        &versions(f, Some(13)),
        f,
        at(f),
        feat,
        "",
        &mut bag
    ));
    assert!(bag.is_empty(), "阈值面不得冒充 version 错");
}

// ── ⑤ 文案形态（官方 format 的静态/动态拼法）──────────────────────

#[test]
fn t_ck_61_特性名的官方文案形态() {
    use Feat::*;
    assert_eq!(AliasDecl.what(""), "type alias");
    assert_eq!(SignedShiftCount.what("x"), "signed shift count x");
    assert_eq!(
        UnderscoreInNumericLiteral.what(""),
        "underscore in numeric literal"
    );
    assert_eq!(BinaryLiteral.what(""), "binary literal");
    assert_eq!(Octal0oLiteral.what(""), "0o/0O-style octal literal");
    assert_eq!(
        HexFloatLiteral.what(""),
        "hexadecimal floating-point literal"
    );
    assert_eq!(UnsafeAdd.what(""), "unsafe.Add");
    assert_eq!(UnsafeSlice.what(""), "unsafe.Slice");
    assert_eq!(
        SliceToArrayPointer.what(""),
        "conversion of slice to array pointer"
    );
    assert_eq!(TypeParameter.what(""), "type parameter");
    assert_eq!(FunctionInstantiation.what(""), "function instantiation");
    assert_eq!(TypeInstantiation.what(""), "type instantiation");
    assert_eq!(ImportedConstraint.what("C"), "using type constraint C");
    assert_eq!(
        EmbeddingConstraintInterface.what("C"),
        "embedding constraint interface C"
    );
    assert_eq!(
        EmbeddingInterfaceElement.what("~int"),
        "embedding interface element ~int"
    );
    assert_eq!(
        EmbeddingNonInterfaceType.what("int"),
        "embedding non-interface type int"
    );
    assert_eq!(PredeclaredAnyComparable.what("any"), "predeclared any");
    assert_eq!(SliceToArray.what(""), "conversion of slice to array");
    assert_eq!(UnsafeSliceData.what(""), "unsafe.SliceData");
    assert_eq!(UnsafeString.what(""), "unsafe.String");
    assert_eq!(UnsafeStringData.what(""), "unsafe.StringData");
    assert_eq!(Clear.what(""), "clear");
    assert_eq!(BuiltinMinMax.what("min"), "built-in min");
    assert_eq!(BuiltinMinMax.what("max"), "built-in max");
    assert_eq!(
        ImplicitInstantiationAsArgument.what(""),
        "implicitly instantiated function as argument"
    );
    assert_eq!(RangeOverInt.what(""), "range over int");
    assert_eq!(RangeOverFunc.what(""), "range over func");
    assert_eq!(GenericTypeAlias.what(""), "generic type alias");
    assert_eq!(NewExpr.what("x"), "new(x)");
    assert_eq!(
        PromotedFieldKey.what("X in struct literal of type T"),
        "use of promoted field X in struct literal of type T"
    );
    assert_eq!(GenericMethod.what(""), "generic method");

    // 拼出来的整条消息与官方 `"%s requires %s or later"` 同形。
    let f = file();
    let mut bag = rgoc_lex::DiagBag::new();
    verify_version(
        &versions(f, Some(20)),
        f,
        at(f),
        BuiltinMinMax,
        "min",
        &mut bag,
    );
    let d = bag.into_sorted().remove(0);
    assert_eq!(d.message, "built-in min requires go1.21 or later");
}
