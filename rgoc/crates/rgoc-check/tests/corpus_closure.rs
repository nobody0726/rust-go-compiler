//! Task 37 收口期的两条**能力补齐**回归（M4/CH-2 现场驱动）。
//!
//! 这两条不是新测试项，而是收口时被语料逼出来的**真缺口**，各留一条自有断言
//! 防回退：
//!
//! | 缺口 | 现场 | 修法 |
//! |---|---|---|
//! | 函数体内**具名**类型不建 named | `check/conversions1.go`（oracle 47 / rgoc 0）—— 局部 `type S struct{}` 只得到结构本体 ⇒ `S` 与 `T` 被当成同一类型，赋值不报 | `stmt.rs::decl_stmt` 的 `Decl::Type` 建 `add_named` + 本体（别名不建） |
//! | `--test-hooks` 未注入 `assert` / `trace` | `check/literals.go`（oracle 0 / rgoc 68 条 `undefined: assert`） | `check.rs::seed_filtered` 在旗标下过滤这两条装载诊断；`expr.rs::ident` 兜底返回内建 |
//!
//! # 期望值来源
//!
//! 两条都由 oracle `--check` 实测（`.superpowers/sdd/M4-plan/probe/t37/locals*.go`）：
//! `8:6 assign` / `6:6 assign`，与 `literals.go` 的 0 行。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_lex::DiagCode;
use rgoc_load::load_sources;
use rgoc_types::TypeTable;

fn run(src: &str, hooks: bool) -> (Vec<String>, Vec<DiagCode>) {
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_sources(&mut tt, &[("t.go", src.as_bytes())], None);
    let opts = CheckOptions { test_hooks: hooks };
    let c = check_package(&pkg, &mut tt, bag, &opts);
    let rows = c
        .render("t.go")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(str::to_owned)
        .collect();
    (rows, c.diags.iter().map(|d| d.diag.code).collect())
}

/// 局部 `type S struct{a int}` 与 `type T struct{a int}` 是**两个具名类型**：
/// `s = t` 必须报 `assign`（官方 `definedType` 的同一性）。
#[test]
fn t_ck_91_局部具名类型参与同一性() {
    const SRC: &str = r#"package p

func f() {
	type S struct{ a int }
	type T struct{ a int }
	var s S
	var t T
	s = t
	_ = s
}
"#;
    let (rows, codes) = run(SRC, false);
    assert_eq!(rows, vec!["8:6\tassign"], "oracle 实测 8:6 assign");
    assert_eq!(codes, vec![DiagCode::ChkInvalidAssign]);
}

/// 别名（`type A = B`）**不**建 named ⇒ 与本体可赋值。
#[test]
fn t_ck_91_局部别名不建具名类型() {
    const SRC: &str = r#"package p

func f() {
	type S struct{ a int }
	type A = S
	var s S
	var a A
	s = a
	_ = s
}
"#;
    let (rows, codes) = run(SRC, false);
    assert!(rows.is_empty(), "别名与本体同型，不该报，got {rows:?}");
    assert!(codes.is_empty(), "got {codes:?}");
}

/// `--test-hooks`：`assert` / `trace` 是官方 `DefPredeclaredTestFuncs` 注入的
/// **内建**。开 ⇒ 零诊断；关 ⇒ 两条 `undefined`（真诊断，不许静默）。
#[test]
fn t_ck_91_测试内建注入() {
    const SRC: &str = r#"package p

func f() {
	assert(true)
	trace("x")
}
"#;
    let (rows, codes) = run(SRC, true);
    assert!(rows.is_empty(), "开 hooks 时零诊断，got {rows:?}");
    assert!(codes.is_empty(), "got {codes:?}");

    let (rows, codes) = run(SRC, false);
    assert_eq!(
        rows,
        vec!["4:2\tundef", "5:2\tundef"],
        "关 hooks 时两条 undef"
    );
    assert!(
        codes.iter().all(|c| *c == DiagCode::TypUndeclaredName),
        "got {codes:?}"
    );
}
