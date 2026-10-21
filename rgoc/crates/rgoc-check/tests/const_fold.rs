//! `T-CK-43` 常量折叠接线（M4/Task 30）：`U-M3-08` 的**检查面**解除。
//!
//! # 判据
//!
//! 装载面（`rgoc-load::decls`）的常量求值器没有类型信息，判不了
//! `len(s)` / `f()` / `H + 1`，统一报 `TypInvalidConstInit`（`D-M3-19` /
//! `T-TY-83`）。检查面（`rgoc-check`）**有**类型信息 —— `builtins` 的
//! `len` / `cap` 会给出常量结果。所以：
//!
//! * 检查面**能**折成常量的 `const` 初始化式 ⇒ 不再转述装载面的拒绝
//!   （`T-CK-43` 的原文：「`const x = len("abc")` 合法」）；
//! * 检查面**也判不了**的形态（`len(s)` 的 `s` 是变量、`H + 1` 的 `H` 是
//!   变量）⇒ 装载诊断照旧入流（`T-TY-83` 正向不回归）。
//!
//! # 夹具与 oracle 证据
//!
//! 夹具与 `.superpowers/sdd/M4-plan/probe/task30-constfold.go` **逐字节
//! 一致**；oracle `--check` 实测 **2 行**：`15:14 decl` / `16:14 decl`。
//! 链式夹具与 `probe/task30-chain.go` 逐字节一致（oracle 零诊断）。
//!
//! # 窄面（`Ruling 30-1`，台账登记）
//!
//! * `unsafe.Sizeof/Alignof/Offsetof` 的常量结果：Task 31 的 `unsafe_pkg`
//!   落地后自动接上（`constfold` 不需要改）；
//! * 声明了类型的常量（`const x int = len("abc")`）只判「是否常量」，不在
//!   这一层做 `representable` —— 那是装载面的既有职责。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_lex::DiagCode;
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 折叠 / 拒绝混合夹具（与 `probe/task30-constfold.go` 逐字节一致）。
const SRC: &str = r#"// T-CK-43 常量折叠接线 —— 探针（Task 30）。
package p

var s []int
var arr [4]int
var H int

const n1 = len("abc")
const n2 = len(arr)
const n3 = cap(arr)
const n4 = n1 + n2 + n3
var x [n4]int
var _ = x

const bad1 = len(s)
const bad2 = H + 1
"#;

/// 链式夹具（与 `probe/task30-chain.go` 逐字节一致；oracle 零诊断）。
const CHAIN_SRC: &str = r#"package p

const a = len("abc")
const b = a
const c = b + 1
var x [c]int
var _ = x
"#;

fn load(src: &str) -> (TypeTable, Package, rgoc_lex::DiagBag) {
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_sources(&mut tt, &[("t.go", src.as_bytes())], None);
    (tt, pkg, bag)
}

fn rows(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// 折叠面与拒绝面**同一条夹具**：可折叠的 4 条零诊断，判不了的 2 条保留。
#[test]
fn t_ck_43_折叠面解除_拒绝面保留() {
    let (mut tt, pkg, bag) = load(SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(
        rows(&checked.render("t.go")),
        vec!["15:14\tdecl", "16:14\tdecl"],
        "渲染行 vs oracle（`probe/task30-constfold.go` 实测）"
    );
    // 两条都必须是装载面的 `TypInvalidConstInit`，不是检查面新造的。
    assert!(
        checked
            .diags
            .iter()
            .all(|r| r.diag.code == DiagCode::TypInvalidConstInit),
        "只应保留装载面的 `TypInvalidConstInit`，got {:?}",
        checked
            .diags
            .iter()
            .map(|r| r.diag.code)
            .collect::<Vec<_>>()
    );
}

/// 链式：`const a = len("abc")` 折叠后，`b = a` 与 `c = b + 1` 跟着通
/// （检查面按声明序递推，装载面的三条拒绝全部被过滤）。
#[test]
fn t_ck_43_链式递推_零诊断() {
    let (mut tt, pkg, bag) = load(CHAIN_SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert!(
        checked.diags.is_empty(),
        "链式折叠必须零诊断，got {:?}",
        checked
            .diags
            .iter()
            .map(|r| (r.diag.pos.line, r.diag.pos.col, r.diag.code))
            .collect::<Vec<_>>()
    );
    assert_eq!(checked.rendered_count(), 0);
}

/// `T-CK-43` 的原文形态：`const x = len("abc")` 单独一条也合法
/// （数组长度能用它 ⇒ 值确实是常量、不是「不报但也没有值」）。
#[test]
fn t_ck_43_len_字符串常量_可作数组长度() {
    const SRC2: &str = "package p\n\nconst n = len(\"abc\")\nvar a [n]int\nvar _ = a\n";
    let (mut tt, pkg, bag) = load(SRC2);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert!(
        checked.diags.is_empty(),
        "`const n = len(\"abc\")` + `[n]int` 必须零诊断，got {:?}",
        checked
            .diags
            .iter()
            .map(|r| (r.diag.pos.line, r.diag.pos.col, r.diag.code))
            .collect::<Vec<_>>()
    );
}

/// 装载面**不改**：同一个源走 `load_sources` 仍然拿到
/// `TypInvalidConstInit`（`T-TY-83` 的正向在装载面成立）。
///
/// 这条是 `Ruling 30-2` 的钉子 —— 「解除 `U-M3-08`」只在检查面成立；
/// 若哪天有人在装载面也放开了，这条会红，提醒去改 `T-TY-83` 的注记
/// （Task 40）。
#[test]
fn t_ck_43_装载面行为不变() {
    let (_, _, bag) = load("package p\n\nconst n = len(\"abc\")\n");
    assert_eq!(
        bag.into_sorted().iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagCode::TypInvalidConstInit],
        "装载面（`rgoc types`）必须保持原行为"
    );
}
