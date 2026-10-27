//! `GO-4` `select` 三阶段（M11b）—— 判据骨架。
//!
//! `C11` §4 三阶段：① 所有 case 的 chan 表达式 + send 右值**各求值一次**；
//! ② 源码顺序试探；③ 都没就绪且无 `default` ⇒ 登记 + 换 G。
//!
//! 这里先放阶段①（最容易漏：只求值第一个 case、或者求值两次）。
//! 撤销 / 重复 channel / 空 select 在 Task 24–25 补。

mod support;

use support::go_probe;

const EVAL_ONCE: &str = r#"
package main

var n = 0

func f(c chan int) chan int {
	n++
	return c
}

func main() {
	a := make(chan int, 1)
	b := make(chan int, 1)
	select {
	case a <- f(a):
	case b <- f(b):
	}
	println(n)
}
"#;

#[test]
#[ignore = "M11b 实现后启用（Task 31 收口）：现在 select 还静态拒绝"]
fn t_sel_01_求值一次() {
    let t = go_probe::probe("sel01", EVAL_ONCE, &[("RGOC_GC", "0")])
        .expect("应能编译并跑起来（M11b 之后）");
    t.assert_l1("sel01");
    assert_eq!(
        t.native.stdout, "2\n",
        "阶段① 必须给**每个** case 求值一次（不是 0 次、不是 4 次）：实际 {:?}",
        t.native.stdout
    );
}
