//! 并发节点的 **lowering 面**（`M8-plan.md` Task 7–13）。
//!
//! # 为什么这一层能单独判
//!
//! `--dump-hir` 在**执行之前**返回（`RunCommand::dump_hir`），所以这些判据
//! 不依赖解释器有没有实现 `go`/`chan` —— Task 7–13 只做「让 lowering 放行、
//! 且产出的节点形状对」，执行是 Task 15 起的事。
//!
//! # 判据为什么落在 dump 文本上
//!
//! 断言「没报 `HIR-UNSUPPORTED-*`」只证明**没拒**，不证明**产出了什么**。
//! `Send` 与 `Go` 都只有一个 `call`/`value` 子表达式，拒掉与放行都能让
//! 「程序不报错」；只有 dump 里的那行 `Send` / `Go` / `recv comma_ok=true`
//! 能证明它真的被 lowering 成了那个节点。
//!
//! # fixture 怎么放
//!
//! 内联字符串写到 `target/tmp/con-lower`（与 `run_semantics.rs` 同一套做法，
//! 不进只读语料目录）。

use std::path::PathBuf;

use rgoc_driver::cli::{RunCommand, Sched, Via};
use rgoc_driver::run_cmd::{RunOutcome, run_run};

/// 把源码写到临时目录再跑 `--dump-hir`。
fn dump_raw(name: &str, src: &str) -> RunOutcome {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/con-lower");
    std::fs::create_dir_all(&dir).expect("建不了 target/tmp/con-lower");
    let path = dir.join(format!("{name}.go"));
    std::fs::write(&path, src).expect("写不了 fixture");
    run_run(&RunCommand {
        files: vec![path],
        dump_hir: true,
        max_steps: None,
        lang: None,
        via: Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
        sched: Sched::default(),
    })
    .expect("内部错（`run_run` 返回 Err）")
}

/// 跑 `--dump-hir` 并断言「没有拒绝、退出码 0」，返回 dump 文本。
#[track_caller]
fn dump(name: &str, src: &str) -> String {
    let o = dump_raw(name, src);
    assert_eq!(o.exit, 0, "{name}: 退出码 {}（stderr={}）", o.exit, o.stderr);
    assert!(
        !o.stderr.contains("HIR-UNSUPPORTED"),
        "{name}: 不该再被拒：{}",
        o.stderr
    );
    o.stdout
}

/// 断言 dump 里有一行包含 `needle`。
#[track_caller]
fn has(text: &str, needle: &str) {
    assert!(
        text.lines().any(|l| l.contains(needle)),
        "dump 里找不到 {needle:?}：\n{text}"
    );
}

// ───────────────────────── Task 7：make(chan) ─────────────────────────

#[test]
fn t_task7_make_chan_三种形态都放行() {
    let src = r#"
package main

func main() {
	a := make(chan int)
	b := make(chan int, 3)
	n := 2
	c := make(chan int, n)
	_, _, _ = a, b, c
}
"#;
    let d = dump("t7_make", src);
    has(&d, "make chan args=0"); // 无缓冲
    has(&d, "make chan args=1"); // 带 cap（常量与变量各一处）
    // 两处 args=1（`3` 与 `n`），加一处 args=0。
    assert_eq!(
        d.matches("make chan args=0").count(),
        1,
        "无缓冲只该有一处：\n{d}"
    );
    assert_eq!(
        d.matches("make chan args=1").count(),
        2,
        "带 cap 的有两处（常量 / 变量）：\n{d}"
    );
}

// ───────────────────────── Task 8：close / len / cap ─────────────────────────

#[test]
fn t_task8_close_放行() {
    let d = dump(
        "t8_close",
        r#"
package main

func main() {
	c := make(chan int)
	close(c)
}
"#,
    );
    has(&d, "builtin close args=1");
}

#[test]
fn t_task8_len_cap_对_chan_放行() {
    let d = dump(
        "t8_lencap",
        r#"
package main

func main() {
	c := make(chan int, 4)
	println(len(c), cap(c))
}
"#,
    );
    has(&d, "builtin len args=1");
    has(&d, "builtin cap args=1");
}

// ───────────────────────── Task 9：ch <- v ─────────────────────────

#[test]
fn t_task9_send_语句产出_Send_节点() {
    let d = dump(
        "t9_send",
        r#"
package main

func main() {
	c := make(chan int, 1)
	c <- 1
}
"#,
    );
    has(&d, "Send");
}

#[test]
fn t_task9_send_求值顺序是_chan_先于_value() {
    // 规范：操作数从左到右 ⇒ `chan` 先求值。用两个**可区分的调用**做探针：
    // dump 的子节点顺序就是求值顺序（`Stmt::Send` 分支先 dump `chan`）。
    // 用 `c <- 2` 判不了 —— 那时 chan 只是一个 `slot#0`，两种顺序长得一样。
    let d = dump(
        "t9_order",
        r#"
package main

func getch() chan int { return make(chan int, 1) }
func gen() int { return 2 }

func main() {
	getch() <- gen()
}
"#,
    );
    let lines: Vec<&str> = d.lines().collect();
    let send = lines
        .iter()
        .position(|l| l.trim_end().ends_with("Send"))
        .unwrap_or_else(|| panic!("找不到 Send 行：\n{d}"));
    let chan_at = lines[send..]
        .iter()
        .position(|l| l.contains("call func#0"))
        .map(|i| send + i);
    let value_at = lines[send..]
        .iter()
        .position(|l| l.contains("call func#1"))
        .map(|i| send + i);
    let (Some(c), Some(v)) = (chan_at, value_at) else {
        panic!("Send 下面应当同时有 getch(func#0) 与 gen(func#1) 两个调用：\n{d}");
    };
    assert!(c < v, "通道表达式必须先于右值求值（getch@{c} gen@{v}）：\n{d}");
}

// ───────────────────────── Task 10：<-ch ─────────────────────────

#[test]
fn t_task10_recv_单值形态() {
    let d = dump(
        "t10_recv1",
        r#"
package main

func main() {
	c := make(chan int, 1)
	v := <-c
	println(v)
}
"#,
    );
    has(&d, "recv comma_ok=false");
}

#[test]
fn t_task10_recv_comma_ok_形态() {
    let d = dump(
        "t10_recv2",
        r#"
package main

func main() {
	c := make(chan int, 1)
	v, ok := <-c
	println(v, ok)
}
"#,
    );
    has(&d, "recv comma_ok=true");
}

// ───────────────────────── Task 11：go f() ─────────────────────────

#[test]
fn t_task11_go_调用产出_Go_节点() {
    let d = dump(
        "t11_go",
        r#"
package main

func f(n int) { println(n) }

func main() {
	go f(1)
}
"#,
    );
    has(&d, "Go");
    has(&d, "call f");
}

#[test]
fn t_task11_go_函数字面量() {
    let d = dump(
        "t11_go_lit",
        r#"
package main

func main() {
	go func() { println(1) }()
}
"#,
    );
    has(&d, "Go");
}

// ───────────────────────── Task 12：select ─────────────────────────

#[test]
fn t_task12_select_五种形态() {
    let d = dump(
        "t12_select",
        r#"
package main

func main() {
	a := make(chan int, 1)
	b := make(chan int, 1)
	select {
	case <-a:
	case v := <-b:
		println(v)
	}
	select {
	case v, ok := <-a:
		println(v, ok)
	case b <- 1:
	default:
	}
}
"#,
    );
    has(&d, "Select\tclauses=2");
    has(&d, "Select\tclauses=3");
    has(&d, "#0 recv targets=0");
    has(&d, "#1 recv targets=1");
    has(&d, "#0 recv targets=2");
    has(&d, "#1 send targets=0 send=true");
    has(&d, "#2 default targets=0 send=false");
}

#[test]
fn t_task12_select_子句顺序与源码一致() {
    // `D-M8-3`：多就绪时取源码顺序的第一个 ⇒ clauses 的顺序**就是**判据。
    // 这里故意把 send 写在 recv 前面，若实现按种类重排，下面的断言会红。
    let d = dump(
        "t12_order",
        r#"
package main

func main() {
	a := make(chan int, 1)
	select {
	case a <- 1:
	case <-a:
	}
}
"#,
    );
    has(&d, "#0 send targets=0 send=true");
    has(&d, "#1 recv targets=0 send=false");
}

// ───────────────────────── Task 13：range over chan ─────────────────────────

#[test]
fn t_task13_range_over_chan() {
    let d = dump(
        "t13_range",
        r#"
package main

func main() {
	c := make(chan int, 1)
	for v := range c {
		println(v)
	}
}
"#,
    );
    has(&d, "kind=Chan key=0 value=1");
}
