//! `T-EX-59` C 族：复合字面量取址（M5/Task 4）。
//!
//! # 规范原文（`go_spec.html` §Address operators）
//!
//! > The operand must be *addressable*, that is, either a variable, pointer
//! > indirection, or slice indexing operation; or a field selector of an
//! > addressable struct operand; or an array indexing operation of an
//! > addressable array. **As an exception to the addressability requirement,
//! > `x` may also be a (possibly parenthesized) composite literal.**
//!
//! 我们原先只放行 `Mode::Variable`，于是 `&T{1}` 被报成
//! `invalid operation: cannot take address of T` —— `compos.go:16`、
//! `ddd.go:189/222`、`method7.go:62` 三处语料的形态。
//!
//! # 为什么这条会被 A 族掩盖
//!
//! `method7.go` 的 `h(&Outer{&Inner{"hello"}})` 里 `h` 是函数值变量；Task 2 之前
//! 调用面整体返回 `invalid`，**实参从未求值** ⇒ 取址规则没被触发，文件是「绿」的。
//! A 族修好后它才露出来 —— 这是修 A 的正当连带，不是回归。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 接受向量：复合字面量的各种取址形态。
const ACCEPT: &str = r#"package p

type T struct{ A int }
type Inner struct{ S string }
type Outer struct{ *Inner }

func mk() *T { return &T{1} }
func nested() *Outer { return &Outer{&Inner{"x"}} }

var p1 = &T{}
var p2 = &[]int{1, 2}
var p3 = &struct{ X int }{1}
var p4 = &[2]int{1, 2}
var p5 = &map[string]int{"a": 1}
var p6 = (&T{2})
"#;

/// 拒绝向量：不是复合字面量、也不可寻址的操作数。
const REJECT: &str = r#"package p

type T struct{ A int }

func mk() T { return T{} }

func bad() {
	_ = &mk()
	_ = &1
	m := map[string]T{}
	_ = &m["a"]
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
fn t_ex_59_复合字面量取址合法() {
    let (mut tt, pkg, bag) = load(ACCEPT);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(rows(&checked.render("t.go")), Vec::<String>::new());
}

#[test]
fn t_ex_59_非复合字面量仍须可寻址() {
    let (mut tt, pkg, bag) = load(REJECT);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    let got = rows(&checked.render("t.go"));
    // 三条：`&mk()`、`&1`、`&m["a"]`（map 元素不可寻址）。
    assert_eq!(got.len(), 3, "实际渲染行：{got:?}");
    for r in &got {
        assert!(
            r.ends_with("\top-invalid"),
            "类别应为 op-invalid（取址失败），实际：{r}"
        );
    }
}
