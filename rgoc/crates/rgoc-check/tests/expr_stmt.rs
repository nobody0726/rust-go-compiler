//! `T-EX-58` B 族：调用语句的结果被丢弃是合法的（M5/Task 3）。
//!
//! # 修的是什么
//!
//! 官方 `stmt.go:439-461` 用 `kind := check.rawExpr(...)` 的 `exprKind` 判定：
//! 函数/方法调用返回 `statement`，于是 `switch x.mode()` 的 `default` 分支
//! 直接 `return` —— **不看结果个数**。我们原先只放行 `Mode::NoValue`
//! （无结果调用），有结果的调用当语句会被报成 `value of type T is not used`：
//! `divmod.go:126/127/143`、`escape3.go:16/17/18`、`ken/rob1.go:64` 共 7 行。
//!
//! # 关键区分：**转换不是调用**
//!
//! `int(x)` 也是 `Expr::Call`，官方对它返回 `conversion` 而不是 `statement`
//! ⇒ 仍报 `is not used`。所以不能用语法形状判，必须看**被调方是不是类型表达式**。
//! 本文件的拒绝向量就是钉这一条的。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 接受向量：四种「调用当语句」，结果全部丢弃。
const ACCEPT: &str = r#"package p

type T struct{}

func (T) M() int { return 1 }

func ret() int { return 1 }
func ret2() (int, int) { return 1, 2 }

func f() {
	ret()
	ret2()
	T{}.M()
	var g func() int
	g()
	(ret)()
}
"#;

/// 拒绝向量：非调用表达式语句（含**转换**，它语法上也是 `Call`）。
const REJECT: &str = r#"package p

func f() int { return 1 }

func g() {
	f() + 1
	int(1)
	"x"
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
fn t_ex_58_调用当语句_结果丢弃合法() {
    let (mut tt, pkg, bag) = load(ACCEPT);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(rows(&checked.render("t.go")), Vec::<String>::new());
}

#[test]
fn t_ex_58_非调用表达式语句仍报_转换也算() {
    let (mut tt, pkg, bag) = load(REJECT);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    let got = rows(&checked.render("t.go"));
    // 三行：`f() + 1`、`int(1)`、`"x"`。类别都是 `unused`。
    assert_eq!(got.len(), 3, "实际渲染行：{got:?}");
    for r in &got {
        assert!(r.ends_with("\tunused"), "类别应为 unused，实际：{r}");
    }
}
