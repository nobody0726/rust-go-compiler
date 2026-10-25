//! 单 goroutine 下的 `go` / `chan` 语义（`M8-plan.md` Task 15）。
//!
//! # 这一层为什么存在
//!
//! `con_lower.rs` 只证明「节点产出了」，`chan_obj.rs` 只证明「通道对象本身对」。
//! 两者之间还有一段：**解释器有没有把它们接上**。这一层就是那段 ——
//! 跑真程序、比 stdout。
//!
//! # 为什么这里的 `go` 是同步的
//!
//! M8 的 `go` 要「新 goroutine」，那是 Task 24 的事。在那之前同步执行是
//! 唯一不撒谎的形态：输出顺序与官方不同（官方不保证），但它确定、可回放，
//! 而且 `T-CON-02` 判的另一半（「实参在当前 G 求值」）在这里已经成立。
//! 所以本文件的断言都是「同步语义」的 —— Task 24 会把它们改成端到端交错判据。
//!
//! # 不能写什么
//!
//! 任何会**阻塞**的 fixture：`make(chan int)`（无缓冲）上先 send 或先 recv
//! 都会走到 Task 20 的 `todo!()`。这里只写「缓冲区够用」的程序。

use std::path::PathBuf;

use rgoc_driver::cli::{RunCommand, Sched, Via};
use rgoc_driver::run_cmd::{RunOutcome, run_run};

fn run_src(name: &str, src: &str) -> RunOutcome {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/con-single");
    std::fs::create_dir_all(&dir).expect("建不了 target/tmp/con-single");
    let path = dir.join(format!("{name}.go"));
    std::fs::write(&path, src).expect("写不了 fixture");
    run_run(&RunCommand {
        files: vec![path],
        dump_hir: false,
        max_steps: None,
        lang: None,
        via: Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
        sched: Sched::default(),
    })
    .expect("内部错（`run_run` 返回 Err）")
}

/// 跑一个应当正常结束的 fixture，返回合并流。
#[track_caller]
fn out(name: &str, src: &str) -> String {
    let o = run_src(name, src);
    assert_eq!(o.exit, 0, "{name}: 退出码 {}，stderr={}", o.exit, o.stderr);
    assert!(o.stderr.is_empty(), "{name}: stderr 非空：{}", o.stderr);
    o.stdout.replace("\r\n", "\n")
}

// ───────────────────────── go ─────────────────────────

#[test]
fn t15_go_同步执行() {
    // 同步语义：`f` 的打印必须出现在下一句之前。
    let s = out(
        "t15_go_sync",
        r#"
package main

func f(n int) { println("f", n) }

func main() {
	go f(1)
	println("main")
}
"#,
    );
    assert_eq!(s, "f 1\nmain\n");
}

#[test]
fn t15_go_函数字面量_捕获外层变量() {
    let s = out(
        "t15_go_lit",
        r#"
package main

func main() {
	x := 7
	go func() { println(x) }()
	println("after")
}
"#,
    );
    assert_eq!(s, "7\nafter\n");
}

#[test]
fn t15_go_不留下多余的值() {
    // `go f()` 之后值栈必须干净：`f` 里的局部值不许漏到调用方。
    let s = out(
        "t15_go_discard",
        r#"
package main

func f() {
	x := 9
	println(x)
}

func main() {
	go f()
	println("ok")
}
"#,
    );
    assert_eq!(s, "9\nok\n");
}

// ───────────────────────── chan ─────────────────────────

#[test]
fn t15_缓冲通道_发两个收两个() {
    let s = out(
        "t15_chan_fifo",
        r#"
package main

func main() {
	c := make(chan int, 2)
	c <- 1
	c <- 2
	a := <-c
	b := <-c
	println(a, b)
}
"#,
    );
    assert_eq!(s, "1 2\n");
}

#[test]
fn t15_通道是引用语义() {
    // `d := c` 拷的是**句柄**：往 `d` 发的值必须能从 `c` 收到。
    // 若 `deep_copy` 把通道当值拷了，这里会收不到（而且不会报错）。
    let s = out(
        "t15_chan_ref",
        r#"
package main

func main() {
	c := make(chan int, 1)
	d := c
	d <- 42
	println(<-c)
	println(c == d)
}
"#,
    );
    assert_eq!(s, "42\ntrue\n");
}

#[test]
fn t15_接收的_comma_ok_在未关闭时是_true() {
    let s = out(
        "t15_chan_comma_ok",
        r#"
package main

func main() {
	c := make(chan int, 1)
	c <- 5
	v, ok := <-c
	println(v, ok)
}
"#,
    );
    assert_eq!(s, "5 true\n");
}

// ───────────────────────── len / cap / close / print ─────────────────────────

#[test]
fn t16_len_cap_四个组合() {
    // 无缓冲：0/0；带 cap：0/3；发一个：1/3；收一个：0/3。
    let s = out(
        "t16_lencap",
        r#"
package main

func main() {
	a := make(chan int)
	println(len(a), cap(a))
	b := make(chan int, 3)
	println(len(b), cap(b))
	b <- 7
	println(len(b), cap(b))
	<-b
	println(len(b), cap(b))
}
"#,
    );
    assert_eq!(s, "0 0\n0 3\n1 3\n0 3\n");
}

#[test]
fn t16_close_之后_len_cap_不变() {
    let s = out(
        "t16_close_lencap",
        r#"
package main

func main() {
	c := make(chan int, 2)
	c <- 1
	close(c)
	println(len(c), cap(c))
}
"#,
    );
    assert_eq!(s, "1 2\n", "close 不排空缓冲（先拿完值才是零值）");
}

#[test]
fn t16_close_之后先排空再拿零值() {
    let s = out(
        "t16_close_drain",
        r#"
package main

func main() {
	c := make(chan int, 2)
	c <- 1
	c <- 2
	close(c)
	for i := 0; i < 3; i++ {
		v, ok := <-c
		println(v, ok)
	}
}
"#,
    );
    assert_eq!(s, "1 true\n2 true\n0 false\n");
}

#[test]
fn t16_close_两次必须_panic() {
    let o = run_src(
        "t16_double_close",
        r#"
package main

func main() {
	c := make(chan int, 1)
	close(c)
	close(c)
	println("不该到这里")
}
"#,
    );
    assert_eq!(o.exit, 2, "double close 必须退出 2：{:?}", o.stderr);
    assert!(
        o.stdout.contains("panic: close of closed channel"),
        "panic 文本要与官方一致（不带 runtime error: 前缀）：{}",
        o.stdout
    );
    assert!(
        !o.stdout.contains("不该到这里"),
        "panic 之后不许再执行：{}",
        o.stdout
    );
}

#[test]
fn t16_close_nil_必须_panic() {
    let o = run_src(
        "t16_close_nil",
        r#"
package main

func main() {
	var c chan int
	close(c)
}
"#,
    );
    assert_eq!(o.exit, 2);
    assert!(
        o.stdout.contains("panic: close of nil channel"),
        "实际：{}",
        o.stdout
    );
}

#[test]
fn t16_print_通道按指针口径() {
    let s = out(
        "t16_print",
        r#"
package main

func main() {
	c := make(chan int)
	println(c)
	var n chan int
	println(n)
}
"#,
    );
    assert_eq!(s, "0xc000000000\n0x0\n");
}

#[test]
fn t15_元素是聚合值也要深拷贝() {
    // `c <- a` 之后改 `a[0]`，接收方必须还看到 1（`C11` §3.3 的第一次拷贝）。
    let s = out(
        "t15_chan_copy",
        r#"
package main

func main() {
	c := make(chan [2]int, 1)
	a := [2]int{1, 2}
	c <- a
	a[0] = 9
	got := <-c
	println(got[0], got[1])
}
"#,
    );
    assert_eq!(s, "1 2\n");
}
