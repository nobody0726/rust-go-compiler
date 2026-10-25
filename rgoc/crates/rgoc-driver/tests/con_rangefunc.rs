//! range-over-func（`M8-plan.md` Task 45–48；`T-CON-43..48`）。
//!
//! # 判据的形状
//!
//! 这一层的每条断言都是「**打印出的值集合 + 顺序**」。range-over-func 没有
//! 调度交错（单 goroutine 就完全确定），所以期望值可以写死成字符串。
//!
//! # 实现口径（`M8-plan.md` Task 45）
//!
//! `for v := range seq { body }` 下降成一次调用 `seq(__range_body_<n>)`：
//! 合成函数的形参 = yield 的形参、结果 = `[bool]`，body 里的 `break` 变成
//! `return false`、`continue` 变成 `return true`（正常结束也 `return true`）。
//! 所以「迭代器看到 `false` 就停」与「body 里 break 就停」是**同一条**通路。
//!
//! # 窄面（`U-M8-03`）
//!
//! body 里的 `return` / `goto` **静态拒绝**（`HIR-UNSUPPORTED-RANGEFUNC-RETURN`）。
//! 理由：body 被下沉成一个**新函数**，而 HIR 的 `return`/`goto` 都是函数作用域
//! 的 —— `return` 会从合成函数返回（语义错），`goto` 的目标标签在另一个函数里
//! （无法表示）。官方 go 接受这两种写法，所以这是**有记录的窄化**，不是漏判。

use std::path::PathBuf;

use rgoc_driver::cli::{RunCommand, Sched, Via};
use rgoc_driver::run_cmd::{RunOutcome, run_run};

fn run_src(name: &str, src: &str) -> RunOutcome {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/con-rangefunc");
    std::fs::create_dir_all(&dir).expect("建不了 target/tmp/con-rangefunc");
    let path = dir.join(format!("{name}.go"));
    std::fs::write(&path, src).expect("写不了 fixture");
    run_run(&RunCommand {
        files: vec![path],
        dump_hir: false,
        max_steps: Some(2_000_000),
        lang: None,
        via: Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
        sched: Sched::default(),
    })
    .expect("内部错（`run_run` 返回 Err）")
}

#[track_caller]
fn out(name: &str, src: &str) -> String {
    let o = run_src(name, src);
    assert_eq!(o.exit, 0, "{name}: 退出码 {}，stderr={}", o.exit, o.stderr);
    assert!(o.stderr.is_empty(), "{name}: stderr 非空：{}", o.stderr);
    o.stdout.replace("\r\n", "\n")
}

#[track_caller]
fn rejected(name: &str, src: &str, code: &str) {
    let o = run_src(name, src);
    assert_eq!(o.exit, 1, "{name}: 应当退出 1，实际 {}：{o:?}", o.exit);
    assert!(
        o.stderr.contains(code),
        "{name}: 应当报 {code}，实际：{:?}",
        o.stderr
    );
}

// ───────────────────────── Task 45：基本形态 ─────────────────────────

#[test]
fn t_con_43_基本形态() {
    let s = out(
        "t_con_43",
        r#"
package main

func seq(yield func(int) bool) {
	for i := 0; i < 3; i++ {
		if !yield(i) {
			return
		}
	}
}

func main() {
	for v := range seq {
		println(v)
	}
	println("end")
}
"#,
    );
    assert_eq!(s, "0\n1\n2\nend\n");
}

#[test]
fn t_con_43b_合成函数能读外层变量() {
    // body 里引用的 `n` 在 main 的帧上 —— 合成函数必须**捕获**它。
    // 这一条同时是捕获机制的判据：少了捕获，`v+n` 解析不到名字。
    let s = out(
        "t_con_43b",
        r#"
package main

func seq(yield func(int) bool) {
	for i := 0; i < 3; i++ {
		if !yield(i) {
			return
		}
	}
}

func main() {
	n := 10
	for v := range seq {
		println(v + n)
	}
}
"#,
    );
    assert_eq!(s, "10\n11\n12\n");
}

// ───────────────────────── Task 46：break / continue ─────────────────────────

#[test]
fn t_con_44_break_让迭代器看到_false() {
    let s = out(
        "t_con_44",
        r#"
package main

func seq(yield func(int) bool) {
	for i := 0; i < 5; i++ {
		if !yield(i) {
			println("iter-stop")
			return
		}
	}
	println("iter-done")
}

func main() {
	for v := range seq {
		if v == 2 {
			break
		}
		println(v)
	}
	println("end")
}
"#,
    );
    // body 的 break ⇒ 合成函数返回 false ⇒ 迭代器立刻 return（不走完自己的循环）。
    assert_eq!(s, "0\n1\niter-stop\nend\n");
}

#[test]
fn t_con_45_continue_只跳本轮后半() {
    let s = out(
        "t_con_45",
        r#"
package main

func seq(yield func(int) bool) {
	for i := 0; i < 4; i++ {
		if !yield(i) {
			return
		}
	}
}

func main() {
	for v := range seq {
		if v == 1 {
			continue
		}
		println(v)
	}
	println("end")
}
"#,
    );
    // continue ⇒ 合成函数返回 true ⇒ 迭代器继续产出下一个值。
    assert_eq!(s, "0\n2\n3\nend\n");
}

// ───────────────────────── Task 47：return / goto 静态拒 ─────────────────────────

#[test]
fn t_con_48a_body_里的_return_被拒() {
    rejected(
        "t_con_48a",
        r#"
package main

func seq(yield func(int) bool) {
	for i := 0; i < 3; i++ {
		if !yield(i) {
			return
		}
	}
}

func main() {
	for v := range seq {
		println(v)
		return
	}
}
"#,
        "HIR-UNSUPPORTED-RANGEFUNC-RETURN",
    );
}

#[test]
fn t_con_48b_body_里的_goto_被拒() {
    rejected(
        "t_con_48b",
        r#"
package main

func seq(yield func(int) bool) {
	for i := 0; i < 3; i++ {
		if !yield(i) {
			return
		}
	}
}

func main() {
	for v := range seq {
		println(v)
		goto out
	}
out:
	println("done")
}
"#,
        "HIR-UNSUPPORTED-RANGEFUNC-RETURN",
    );
}

#[test]
fn t_con_48c_嵌套函数里的_return_不误伤() {
    // `return` 在**嵌套函数字面量**里是合法的（它属于那个字面量，不是合成函数）。
    // 下沉前扫 body 时必须跳过嵌套函数的体，否则会误拒。
    let s = out(
        "t_con_48c",
        r#"
package main

func seq(yield func(int) bool) {
	for i := 0; i < 3; i++ {
		if !yield(i) {
			return
		}
	}
}

func main() {
	for v := range seq {
		f := func() int { return v }
		println(f())
	}
}
"#,
    );
    assert_eq!(s, "0\n1\n2\n");
}

#[test]
fn t_con_48d_嵌套循环里的_break_不误伤() {
    // `break` 在**嵌套循环**里针对的是那个循环，不是 range。
    // 只有「直接落在合成函数体这一层」的 break/continue 才翻译成 return。
    let s = out(
        "t_con_48d",
        r#"
package main

func seq(yield func(int) bool) {
	for i := 0; i < 3; i++ {
		if !yield(i) {
			return
		}
	}
}

func main() {
	for v := range seq {
		for j := 0; j < 3; j++ {
			if j == 1 {
				break
			}
			println(v, j)
		}
	}
	println("end")
}
"#,
    );
    // 每个 v 打 2 行（j=0 一行；j=1 时 break 掉内层），三个 v ⇒ 6 行。
    assert_eq!(s, "0 0\n1 0\n2 0\nend\n");
}

// ───────────────────────── Task 48：seq2 与嵌套 ─────────────────────────

#[test]
fn t_con_47_seq2_两个迭代变量() {
    let s = out(
        "t_con_47",
        r#"
package main

func seq2(yield func(int, string) bool) {
	for i := 0; i < 3; i++ {
		if !yield(i, "x") {
			return
		}
	}
}

func main() {
	for i, s := range seq2 {
		println(i, s)
	}
}
"#,
    );
    assert_eq!(s, "0 x\n1 x\n2 x\n");
}

#[test]
fn t_con_46_嵌套两层不互相干扰() {
    let s = out(
        "t_con_46",
        r#"
package main

func seq(n int) func(func(int) bool) {
	return func(yield func(int) bool) {
		for i := 0; i < n; i++ {
			if !yield(i) {
				return
			}
		}
	}
}

func main() {
	// 外层 3 次、内层 2 次 ⇒ 总共 6 行；用计数把「游标互不干扰」钉住。
	for a := range seq(3) {
		for b := range seq(2) {
			println(a, b)
		}
	}
	println("end")
}
"#,
    );
    assert_eq!(s, "0 0\n0 1\n1 0\n1 1\n2 0\n2 1\nend\n");
}

// ───────────────────────── 边界：`=` 形态与 0 参 yield ─────────────────────────

#[test]
fn t_con_49_等号形态写到已存在的变量() {
    // `=` 形态：左值不在合成函数里声明，体首补一条 `x = v0`。
    // 左值是外层帧的槽 ⇒ 走捕获链（这条同时是捕获**写**路径的判据）。
    let s = out(
        "t_con_49",
        r#"
package main

func seq(yield func(int) bool) {
	for i := 0; i < 3; i++ {
		if !yield(i) {
			return
		}
	}
}

func main() {
	var x int
	for x = range seq {
		println(x)
	}
	println("after", x)
}
"#,
    );
    assert_eq!(s, "0\n1\n2\nafter 2\n");
}

#[test]
fn t_con_49b_零参_yield() {
    // 规范表格的第一行：`func(yield func() bool)` ⇒ 0 个迭代变量。
    // `for range seq` 一个左值都不写，body 照样每轮跑一次。
    let s = out(
        "t_con_49b",
        r#"
package main

func seq(yield func() bool) {
	for i := 0; i < 3; i++ {
		if !yield() {
			return
		}
	}
}

func main() {
	n := 0
	for range seq {
		n++
		println(n)
	}
}
"#,
    );
    assert_eq!(s, "1\n2\n3\n");
}

#[test]
fn t_con_49c_switch_里的_break_不误伤() {
    // `switch` 是可 break 的构造但不是循环：body 里 switch 中的 `break`
    // 针对 switch ⇒ 不翻译成 `return false`（迭代必须继续）。
    let s = out(
        "t_con_49c",
        r#"
package main

func seq(yield func(int) bool) {
	for i := 0; i < 3; i++ {
		if !yield(i) {
			return
		}
	}
}

func main() {
	for v := range seq {
		switch v {
		case 1:
			println("one")
			break
		default:
			println("other", v)
		}
	}
	println("end")
}
"#,
    );
    assert_eq!(s, "other 0\none\nother 2\nend\n");
}

#[test]
fn t_con_49d_switch_里的_continue_不误伤() {
    // `switch` 可 break 但**不是循环**：它只把 `break` 的深度 +1，不动
    // `continue` 的。所以 switch 里的 `continue` 仍然针对 range ⇒ 必须
    // 翻译成 `return true`（迭代继续，只是跳过本轮后半）。
    //
    // 这条判据抓的是一个**维度看错**的实现：若 `continue` 也去看
    // `breakable`，switch 里的 continue 就不会被翻译 ⇒ 解释器的
    // `Branch{Continue}` 找不到循环就把 body 栈弹空 ⇒ 整个迭代**提前结束**。
    let s = out(
        "t_con_49d",
        r#"
package main

func seq(yield func(int) bool) {
	for i := 0; i < 4; i++ {
		if !yield(i) {
			return
		}
	}
}

func main() {
	for v := range seq {
		switch v {
		case 1:
			println("skip", v)
			continue
		default:
			println("keep", v)
		}
	}
	println("end")
}
"#,
    );
    assert_eq!(s, "keep 0\nskip 1\nkeep 2\nkeep 3\nend\n");
}

#[test]
fn t_con_49e_select_里的_break_不误伤() {
    // `select` 与 `switch` 一样只是「可 break 的构造」：body 里 select
    // 子句中的 `break` 针对 select ⇒ 不翻译成 `return false`。
    //
    // 这条判据抓的是「select 不提深度」的实现：漏了它，break 会被翻译 ⇒
    // 整个迭代在第一轮就结束（只打印一个 `got`）。
    let s = out(
        "t_con_49e",
        r#"
package main

func seq(yield func(int) bool) {
	for i := 0; i < 3; i++ {
		if !yield(i) {
			return
		}
	}
}

func main() {
	done := make(chan bool)
	close(done)
	for v := range seq {
		select {
		case <-done:
			println("got", v)
			break
		}
	}
	println("end")
}
"#,
    );
    assert_eq!(s, "got 0\ngot 1\ngot 2\nend\n");
}
