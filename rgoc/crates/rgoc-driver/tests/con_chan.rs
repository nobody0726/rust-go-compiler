//! 通道与调度（`M8-plan.md` Task 19–24；`T-CON-01..12`）。
//!
//! # 判据的形状
//!
//! 这一层的每条断言都是「**输出行的顺序**」—— 交错是调度器唯一可观测的
//! 产物。所以 fixture 里每个动作都先 `println` 再做通信：打印是「这个 G 跑到
//! 这里了」的探针，而通道是「谁先谁后」的闸门。
//!
//! # 确定性从哪来
//!
//! `C11` §2：M:1、FIFO、只在阻塞点让出、`go` 不切走。三条合起来让**单就绪**
//! 的交错完全确定 —— 所以这里的期望值可以写死成字符串，而不是「随便哪种顺序」。
//! 多就绪（select）才需要 seed（Task 37）。

use std::path::PathBuf;

use rgoc_driver::cli::{RunCommand, Sched, Via};
use rgoc_driver::run_cmd::{RunOutcome, run_run};

fn run_src(name: &str, src: &str) -> RunOutcome {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/con-chan");
    std::fs::create_dir_all(&dir).expect("建不了 target/tmp/con-chan");
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

// ───────────────────────── Task 24：创建与退出 ─────────────────────────

#[test]
fn t_con_01_创建顺序是_fifo() {
    let s = out(
        "t_con_01",
        r#"
package main

func main() {
	c := make(chan int)
	go func() { c <- 1 }()
	go func() { c <- 2 }()
	println(<-c, <-c)
}
"#,
    );
    assert_eq!(s, "1 2\n", "先创建的 goroutine 先跑（`C11` §2.1 规则 3）");
}

#[test]
fn t_con_02_实参在旧_goroutine_求值() {
    // `g()` 必须在 **main** 里跑完，`f` 才在新 goroutine 开始 ——
    // 所以 "g" 的打印早于 "main"（`STM-14`）。
    let s = out(
        "t_con_02",
        r#"
package main

func g() int { println("g"); return 1 }
func f(n int) { println("f", n) }

func main() {
	go f(g())
	println("main")
}
"#,
    );
    assert_eq!(s, "g\nmain\nf 1\n");
}

#[test]
fn t_con_02_go_不切走当前_goroutine() {
    // `go f()` 之后 main 接着跑：`main-1` 必须早于 `f`。
    let s = out(
        "t_con_02b",
        r#"
package main

func f() { println("f") }

func main() {
	go f()
	println("main-1")
	println("main-2")
}
"#,
    );
    assert_eq!(s, "main-1\nmain-2\nf\n", "go 只入队尾，不切走");
}

// ───────────────────────── Task 19/20：阻塞与唤醒 ─────────────────────────

#[test]
fn t_con_03_阻塞的_goroutine_被唤醒后继续() {
    let s = out(
        "t_con_03",
        r#"
package main

func main() {
	done := make(chan bool)
	go func() {
		println("B")
		done <- true
	}()
	println("A")
	<-done
	println("C")
}
"#,
    );
    assert_eq!(s, "A\nB\nC\n");
}

#[test]
fn t_con_11_无缓冲_handoff_接收方先打印() {
    // 无缓冲通道上 `c <- 42` 必须等接收方就位；接收方拿到值才打印。
    // 这条是 `C11` §3.2 send ② 的端到端判据。
    let s = out(
        "t_con_11",
        r#"
package main

func main() {
	c := make(chan int)
	go func() {
		println("recv")
		println(<-c)
	}()
	println("send")
	c <- 42
	println("done")
}
"#,
    );
    assert_eq!(s, "send\nrecv\n42\ndone\n");
}

#[test]
fn t_con_12_发送方阻塞时值不丢() {
    // 先发的那个 G 阻塞在 sendq 上，接收方后到 —— 值必须还在（`Waiter.slot`）。
    let s = out(
        "t_con_12",
        r#"
package main

func main() {
	c := make(chan int)
	go func() { c <- 7 }()
	println(<-c)
}
"#,
    );
    assert_eq!(s, "7\n");
}
