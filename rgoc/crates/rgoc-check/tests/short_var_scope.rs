//! `T-EX-61` D 族之二：短变量声明的**作用域**（M5/Task 5）。
//!
//! # 规范原文（`go_spec.html` §Short variable declarations）
//!
//! > Unlike regular variable declarations, a short variable declaration may
//! > redeclare variables provided they were originally declared earlier in the
//! > **same block** (or the parameter lists if the block is the function body)
//! > with the same type, and at least one of the non-blank variables is new.
//!
//! 关键在「**same block**」：内层块里 `p := …` 声明的是一个**新** `p`，
//! 外层同名变量的类型**不参与**这次赋值的可赋值性判定。
//!
//! # 我们原先错在哪
//!
//! `stmt.rs::short_var_decl` 的分类（`fresh`，判「新变量」）用的是
//! `Env::has_local`（只看**栈顶**一层，正确），但随后取目标类型用的是
//! `self.local(…)` = `Env::lookup`（**沿栈查**，错误）——
//! 于是新变量也会拿外层同名变量的类型去判可赋值性：
//!
//! ```text
//! var p *[]byte
//! if p := (*[4]int)(nil); p != nil { … }   // 误报 cannot use *[4]int as *[]byte
//! ```
//!
//! 官方 `initVars` 的分叉是 `if obj != nil { assignVar } else { declare }`，
//! `obj` 由**同一块**的查找决定 —— 新变量根本不进 `assignVar`。
//!
//! # 语料证据（一处修复清掉两个文件）
//!
//! | 文件 | 形态 | 外层 | 内层 |
//! |---|---|---|---|
//! | `convert4.go:73` | `if p := (*Int4)(ii); …` | `var p *[]byte`（61 行） | `p *Int4` |
//! | `bigmap.go:42` | 块内 `m := make(map[T]V)` | `m := make(map[int][1000]byte)`（30 行） | `map[T]V` |
//!
//! 两个文件**都不是**「转换规则缺失」——`(*Int4)(ii)` 的 slice→array-pointer
//! 转换本来就通，假阳性全来自这一处作用域取值。
//!
//! # oracle 证据
//!
//! 两个夹具与 `.superpowers/m5-probe/d3-{accept,reject}.go` 逐字节一致，
//! oracle（`tools/types-oracle --check`）实测：接受夹具 0 行；拒绝夹具 2 行
//! —— `5:10 assign`、`11:10 assign`（同一块内**真**重声明仍要报）。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 接受向量：三种内层块（裸块 / `if` 初始化 / `for` 初始化）各自遮蔽外层。
const ACCEPT: &str = r#"package p

func f() {
	var p *[]byte
	{
		if p := (*[4]int)(nil); p != nil {
			_ = p
		}
		_ = p
	}
}

func g() {
	m := make(map[int][2]byte)
	{
		m := make(map[string]int)
		m["a"] = 1
	}
	_ = m
}

func h() {
	var x int
	for x := 0; x < 3; x++ {
		_ = x
	}
	_ = x
}

func i() {
	var s string
	s, y := "a", 2
	_, _ = s, y
}
"#;

/// 拒绝向量：**同一块**内重声明 —— 非新变量仍走 `assignVar`。
const REJECT: &str = r#"package p

func bad() {
	var s string
	s, y := 1, 2
	_, _ = s, y
}

func bad2() {
	var m map[int]int
	m, n := make(map[string]int), 1
	_, _ = m, n
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
fn t_ex_61_内层块短声明是_新_变量_接受面零诊断() {
    let (mut tt, pkg, bag) = load(ACCEPT);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(rows(&checked.render("t.go")), Vec::<String>::new());
}

#[test]
fn t_ex_61_同块重声明仍判可赋值性() {
    let (mut tt, pkg, bag) = load(REJECT);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    let got = rows(&checked.render("t.go"));
    assert_eq!(
        got,
        vec!["5:10\tassign".to_owned(), "11:10\tassign".to_owned(),],
        "与 oracle 的 (行,列,类别) 逐项一致"
    );
}
