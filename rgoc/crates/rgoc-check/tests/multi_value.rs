//! `T-EX-57` A 族：多值调用的三个形态（M5/Task 2）。
//!
//! # 修的是什么
//!
//! 80 个候选语料里 `rgoc check` 拒了 11 个，其中 2 个属于 A 族：
//!
//! | 文件 | 形态 | 原诊断 |
//! |---|---|---|
//! | `method5.go:103` | `b, x := f(1000, 99)`，`f` 是**函数类型的形参** | `assignment mismatch: 2 variables but f returns 1 value` |
//! | `simassign.go:75` | `a, b = swap(swap(a, b))`，内层多值当**唯一实参** | `not enough arguments in call to swap` |
//!
//! 两处根因不同：
//!
//! 1. **函数类型的变量不是 `Mode::Value`**。`Operand::from_object` 把 `ObjKind::Var`
//!    映射成 `Mode::Variable`，而 `call` 的分派只认 `Mode::Value` ⇒ 函数值调用
//!    整体掉进 `_ => invalid()`。后果分两档：单结果调用**静默通过**（假阴性），
//!    多结果调用被报成「只返回 1 个值」（假阳性）。官方 `call` 对**任何**非类型
//!    表达式的操作数都按其类型走 `arguments`，不区分「声明函数」与「函数值」。
//! 2. **单实参的多值调用没有展开**。官方 `arguments`（`call.go`）：`f(g())`
//!    当 `g` 返回 n 个值时，各返回值**按位置**成为 `f` 的实参。
//!
//! # 反向断言（不许把规则删掉换绿）
//!
//! 接受面修好之后，两条拒绝形态**必须仍然报**：
//! `a, b := one()`（`one` 只返回 1 个值）与 `f(g())`（元数不匹配）。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 接受向量：4 个多值形态，**零诊断**。
///
/// 与 `.superpowers/sdd/M5-plan/probe/mv.go` 同形（探针不入库）。
const ACCEPT: &str = r#"package p

func two() (int, int) { return 1, 2 }

// ① 函数类型的形参
func viaParam(f func(int) (int, int)) (int, int) {
	a, b := f(0)
	return a, b
}

// ② 函数类型的局部变量
func viaLocal() (int, int) {
	var f func() (int, int)
	a, b := f()
	return a, b
}

// ③ 多值调用当唯一实参（`swap(swap(a, b))` 的形态）
func swap(a, b int) (int, int) { return b, a }

func nested() (int, int) {
	a, b := swap(swap(1, 2))
	return a, b
}

// ④ 声明函数的多值调用（本来就通过 —— 回归位）
func direct() (int, int) {
	a, b := two()
	return a, b
}
"#;

/// 拒绝向量：元数不匹配的两条形态。
const REJECT: &str = r#"package p

func one() int { return 1 }
func two() (int, int) { return 1, 2 }
func three(a, b, c int) {}

func bad() {
	a, b := one()          // 1 个值给 2 个变量
	_, _ = a, b
	three(two())           // 2 个值给 3 个形参
}
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

#[test]
fn t_ex_57_接受面_四形态零诊断() {
    let (mut tt, pkg, bag) = load(ACCEPT);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(rows(&checked.render("t.go")), Vec::<String>::new());
}

#[test]
fn t_ex_57_拒绝面_元数不匹配仍报() {
    let (mut tt, pkg, bag) = load(REJECT);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    let got = rows(&checked.render("t.go"));
    // 两条：`a, b := one()` 与 `three(two())`。类别都是 `assign`
    // （官方把元数不匹配归 `assignment mismatch` / `(not enough|too many)
    // arguments in call`，`C8` 适配表把两者都投影到 `assign`）。
    assert_eq!(got.len(), 2, "实际渲染行：{got:?}");
    for r in &got {
        assert!(
            r.ends_with("\tassign"),
            "类别应为 assign（元数不匹配），实际：{r}"
        );
    }
}
