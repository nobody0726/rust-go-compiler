//! `T-CK-91/95` 会话面：诊断流组装 + Cheap-trick 上报层（M4/Task 20）。
//!
//! # 三段纪律
//!
//! 1. **装载诊断先入流**：`rgoc-load` 的 `DiagBag` 以**硬错误**进会话流，
//!    顺序在检查诊断之前（官方 go/types 里装载与检查同属一个 checker 的
//!    错误流，`firstErr` 语义也跨两者）。
//! 2. **Cheap-trick**（`F2`⑧；`go/types/errors.go:111-123`）：**已有首错**
//!    且**首个子错误**含 `"invalid operand"` / `"invalid type"`（出现位置
//!    **> 0**）⇒ 丢弃，不进流。首错自己永不被吞；软错也计入「已有首错」
//!    （官方 `report` 不区分软硬）。
//! 3. **次级消息**（`\t` 开头）留流、渲染剔除（见 `render.rs` —— 过滤点是
//!    渲染出口）。
//!
//! # 为什么 min fixture 走真实装载
//!
//! `check-diags-min.go` 的判据（`C8` §8）就是「位置 + 类别」逐字节，合成
//! 诊断只能证明渲染器自洽；只有装载器真出的三条 `undefined` 才能证明
//! **值层没接线的地方已经接上**（`M3-manifest.json` `open_gaps` 的
//! 「值层没有类型接线」在 Task 20 的落点 = 会话 + 操作数层，见 `operand.rs`）。

use rgoc_check::check::{CheckOptions, Session, check_package};
use rgoc_check::diag::RawDiag;
use rgoc_lex::{Diag, DiagBag, DiagCode, FileTable, Pos};
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 最小对撞目标（与 `tools/types-oracle/testdata/check-diags-min.go` 同源）。
const MIN_SRC: &str = "package p\n\nvar _ = x\nvar _ = y; var _ = y\n";

/// 装载一个内存单文件包（版本源缺省 = 不门控）。
fn load(src: &str) -> (TypeTable, Package, DiagBag) {
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_sources(&mut tt, &[("t.go", src.as_bytes())], None);
    (tt, pkg, bag)
}

/// 合成一条检查侧诊断。
fn diag(line: u32, col: u32, code: DiagCode, msg: &str) -> Diag {
    Diag {
        pos: Pos {
            base: None,
            file: FileTable::new().intern("t.go"),
            line,
            col,
        },
        code,
        message: msg.to_owned(),
    }
}

/// 数据行（剥 `#` 注释行）。
fn rows(text: &str) -> Vec<&str> {
    text.lines().filter(|l| !l.starts_with('#')).collect()
}

/// `check-diags-min` 端到端：三条 `undefined` 原样进流，渲染与契约 want
/// 的**非 `#` 行**逐字节一致（`C8` §8）。
#[test]
fn t_ck_95_min_fixture_三条_undef_与_want_逐字节一致() {
    let (_tt, pkg, bag) = load(MIN_SRC);
    let checked = check_package(&pkg, bag, &CheckOptions::default());

    assert_eq!(checked.diags.len(), 3, "每处使用各一条，不多不少");
    let positions: Vec<(u32, u32)> = checked
        .diags
        .iter()
        .map(|d| (d.diag.pos.line, d.diag.pos.col))
        .collect();
    assert_eq!(positions, vec![(3, 9), (4, 9), (4, 20)], "位置 = 标识符处");
    for d in &checked.diags {
        assert_eq!(d.diag.code, DiagCode::TypUndeclaredName);
        assert!(!d.soft, "装载侧诊断是硬错误");
        assert!(
            d.diag.message.starts_with("undefined: "),
            "{}",
            d.diag.message
        );
    }

    let got = checked.render("testdata/check-diags-min.go");
    let want = include_str!("../../../../docs/contracts/testdata/check-diags-min.want");
    assert_eq!(
        rows(&got),
        rows(want),
        "数据行（位置+类别）必须与 docs/contracts/testdata/check-diags-min.want 一致"
    );
    assert!(got.starts_with("# source: rgoc\n# file: "), "{got:?}");
}

/// 装载诊断以**硬错误**入流，且排在检查诊断**之前**。
#[test]
fn t_ck_91_装载诊断_先入流且为硬错误() {
    let (_tt, _pkg, bag) = load(MIN_SRC);
    assert!(
        !bag.into_sorted().is_empty(),
        "min fixture 至少有一条装载诊断"
    );
    let (_tt2, pkg2, bag2) = load(MIN_SRC);
    let opts = CheckOptions::default();
    let mut s = Session::new(&pkg2, &opts);
    s.seed(bag2);
    s.report_hard(diag(9, 1, DiagCode::TypUndeclaredLabel, "label L not used"));
    let diags = s.into_diags();
    assert_eq!(diags.len(), 4, "3 条装载 + 1 条检查");
    assert_eq!(
        (diags[0].diag.pos.line, diags[3].diag.pos.line),
        (3, 9),
        "装载在前、检查在后"
    );
    assert!(diags.iter().all(|d| !d.soft), "本向量里全是硬错误");
}

/// 次级消息**留流**、渲染剔除（过滤点是渲染出口，不是上报层）。
#[test]
fn t_ck_91_次级消息_留流_渲染剔除() {
    let (_tt, pkg, _bag) = load(MIN_SRC);
    let opts = CheckOptions::default();
    let mut s = Session::new(&pkg, &opts);
    s.report_hard(diag(
        9,
        1,
        DiagCode::TypDuplicateDecl,
        "x redeclared in this block",
    ));
    s.report_hard(diag(
        9,
        3,
        DiagCode::TypDuplicateDecl,
        "\tother declaration of x",
    ));
    let checked = s.into_checked();
    assert_eq!(checked.diags.len(), 2, "流内两份都在");
    assert_eq!(rows(&checked.render("t.go")), vec!["9:1\tdecl"]);
    assert_eq!(checked.rendered_count(), 1, "退出码判据看渲染得出来的行数");
}

/// Cheap-trick 的边界一：**首错永不被吞**（`firstErr == nil` 时不检查）。
#[test]
fn cheap_trick_首错永不移除() {
    let (_tt, pkg, _bag) = load(MIN_SRC);
    let opts = CheckOptions::default();
    let mut s = Session::new(&pkg, &opts);
    // 这条消息**在首错位置也带电**——只要实现漏了「已有首错」条件，它就会丢。
    s.report_hard(diag(
        1,
        1,
        DiagCode::TypInvalidConstVal,
        "cannot use x (invalid type) as T",
    ));
    assert_eq!(s.into_diags().len(), 1);
}

/// Cheap-trick 的边界二：已有首错时，「子串出现位置 > 0」才丢；位置 0 保留。
#[test]
fn cheap_trick_已有首错_且子串不在位置0_才丢() {
    let (_tt, pkg, _bag) = load(MIN_SRC);
    let opts = CheckOptions::default();
    let mut s = Session::new(&pkg, &opts);
    s.report_hard(diag(1, 1, DiagCode::TypUndeclaredName, "undefined: a")); // 首错
    s.report_hard(diag(
        2,
        1,
        DiagCode::TypInvalidConstVal,
        "cannot use b (invalid type) as T",
    )); // 丢
    s.report_hard(diag(
        3,
        1,
        DiagCode::TypInvalidConstVal,
        "cannot use c (invalid operand of type int)",
    )); // 丢
    s.report_hard(diag(
        4,
        1,
        DiagCode::TypInvalidConstVal,
        "invalid type: int",
    )); // 留（位置 0）
    let kept: Vec<(u32, u32)> = s
        .into_diags()
        .iter()
        .map(|d| (d.diag.pos.line, d.diag.pos.col))
        .collect();
    assert_eq!(
        kept,
        vec![(1, 1), (4, 1)],
        "只有位置 > 0 的两个子串命中被丢"
    );
}

/// Cheap-trick 的边界三：只看**首个子错误**（官方 `err.desc[0].msg`）。
///
/// 澄清行（`"\n\t"` 之后）里出现关键词**不算** —— 只看主句。
#[test]
fn cheap_trick_只看首个子错误() {
    let (_tt, pkg, _bag) = load(MIN_SRC);
    let opts = CheckOptions::default();
    let mut s = Session::new(&pkg, &opts);
    s.report_hard(diag(1, 1, DiagCode::TypUndeclaredName, "undefined: a")); // 首错
    s.report_hard(diag(
        2,
        1,
        DiagCode::TypInvalidInitCycle,
        "initialization cycle\n\tcannot use z (invalid type) as T",
    ));
    assert_eq!(s.into_diags().len(), 2, "主句无匹配 ⇒ 保留");
}

/// Cheap-trick 的边界四：**软错也计入首错**（官方 `report` 不区分软硬）。
#[test]
fn cheap_trick_软错也计入首错() {
    let (_tt, pkg, _bag) = load(MIN_SRC);
    let opts = CheckOptions::default();
    let mut s = Session::new(&pkg, &opts);
    s.report_soft(diag(
        1,
        1,
        DiagCode::TypFeatureRequiresGoVersion,
        "range over int requires go1.22 or later",
    ));
    assert_eq!(s.diags().len(), 1, "软错自己不被吞");
    assert!(s.diags()[0].soft);
    s.report_hard(diag(
        2,
        1,
        DiagCode::TypInvalidConstVal,
        "cannot use d (invalid type) as T",
    ));
    let kept = s.into_diags();
    assert_eq!(
        kept.len(),
        1,
        "软错已把「已有首错」立起来 ⇒ 后续关键词错误被吞"
    );
}

/// `seed` 之后 `reported_any` 已立：装载诊断算「已有首错」。
#[test]
fn cheap_trick_装载诊断计入首错() {
    let (_tt, pkg, bag) = load(MIN_SRC);
    let opts = CheckOptions::default();
    let mut s = Session::new(&pkg, &opts);
    s.seed(bag);
    let before = s.diags().len();
    s.report_hard(diag(
        99,
        1,
        DiagCode::TypInvalidConstVal,
        "cannot use e (invalid operand) as T",
    ));
    assert_eq!(
        s.into_diags().len(),
        before,
        "装载诊断已在流 ⇒ 关键词错误被吞"
    );
}

/// `RawDiag` 的 soft 位由 `report_soft` 保住（`api.go:55` 的字段不许丢）。
#[test]
fn t_ck_91_report_soft_保留_soft_位() {
    let (_tt, pkg, _bag) = load(MIN_SRC);
    let opts = CheckOptions::default();
    let mut s = Session::new(&pkg, &opts);
    let d = diag(
        5,
        6,
        DiagCode::TypFeatureRequiresGoVersion,
        "x requires go1.22 or later",
    );
    s.report_soft(d);
    let raw: Vec<RawDiag> = s.into_diags();
    assert!(raw[0].soft, "soft 位必须保留");
    assert_eq!(raw[0].render_row(), "5:6\tversion", "渲染不看软硬");
}
