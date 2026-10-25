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
fn t_con_14_缓冲满时接收会补位() {
    // 关键帧：`<-c` 从满缓冲取走 1 的同时，把**阻塞发送方**的 3 搬进缓冲
    // （`C11` §3.2 recv ①）。少了补位，`len` 会掉到 1，且发送方永远醒不来。
    let s = out(
        "t_con_14",
        r#"
package main

func main() {
	c := make(chan int, 2)
	gate := make(chan bool)
	c <- 1
	c <- 2
	go func() {
		gate <- true
		println("g1 send")
		c <- 3
	}()
	<-gate
	v := <-c
	println(v, len(c), cap(c))
	println(<-c)
	println(<-c)
}
"#,
    );
    assert_eq!(
        s, "g1 send\n1 2 2\n2\n3\n",
        "收一个立刻补一个，len 回到 cap"
    );
}

#[test]
fn t_con_15_缓冲是_fifo() {
    let s = out(
        "t_con_15",
        r#"
package main

func main() {
	c := make(chan int, 3)
	for i := 0; i < 3; i++ {
		c <- i * 10
	}
	for i := 0; i < 3; i++ {
		println(<-c)
	}
}
"#,
    );
    assert_eq!(s, "0\n10\n20\n");
}

// ───────────────────────── Task 23：nil 通道 ─────────────────────────

#[test]
fn t_con_22_nil_通道接收是死锁不是_panic() {
    let o = run_src(
        "t_con_22",
        r#"
package main

func main() {
	var c chan int
	<-c
}
"#,
    );
    assert_eq!(o.exit, 2, "死锁退出码是 2（`C11` §4）");
    assert!(
        o.stderr.contains("all goroutines are asleep - deadlock!"),
        "死锁文本要与官方一致，实际：{}",
        o.stderr
    );
    assert!(
        !o.stdout.contains("panic:"),
        "nil 通道是**永久阻塞**，不是 panic：{}",
        o.stdout
    );
}

#[test]
fn t_con_22b_nil_通道发送是死锁() {
    let o = run_src(
        "t_con_22b",
        r#"
package main

func main() {
	var c chan int
	c <- 1
}
"#,
    );
    assert_eq!(o.exit, 2);
    assert!(
        o.stderr.contains("all goroutines are asleep - deadlock!"),
        "实际：{}",
        o.stderr
    );
}

// ───────────────────────── Task 25/26：深拷贝 ─────────────────────────

#[test]
fn t_con_13_发送与接收各拷一次() {
    // 发送后改源：接收方必须看到旧值（发送时的拷贝）。
    // 接收后改副本：通道里那一份已经取走，改它不该影响任何东西 —— 这条
    // 由「再发一个、收到的仍是原值」间接判。
    let s = out(
        "t_con_13",
        r#"
package main

func main() {
	c := make(chan [3]int, 2)
	a := [3]int{1, 2, 3}
	c <- a
	a[0] = 9
	got := <-c
	println(got[0], got[1], got[2])
	got[1] = 8
	c <- got
	back := <-c
	println(back[0], back[1], back[2])
}
"#,
    );
    assert_eq!(s, "1 2 3\n1 8 3\n");
}

#[test]
fn t_con_24_聚合元素的每个字都要拷() {
    // M7 踩过「只拷第一个字」：`[3]int` 的 2、3 号元素若共享 `Cell`，
    // 发送后改源会把通道里的值一起改掉。
    let s = out(
        "t_con_24",
        r#"
package main

func main() {
	c := make(chan [3]int, 1)
	a := [3]int{0, 0, 0}
	c <- a
	a[2] = 7
	got := <-c
	println(got[0], got[1], got[2])
}
"#,
    );
    assert_eq!(s, "0 0 0\n", "第 3 个元素也必须独立");
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

// ───────────────────────── Task 28：不抢占（`U-M8-01` 的守卫） ─────────────────────────

#[test]
fn t_con_07_死循环的_goroutine_不会被切走() {
    // 若实现里有一丁点「按步数抢占」，main 就有机会拿到 CPU 跑完并退出 0。
    // 实际必须是：那个 `for {}` 烧完预算 ⇒ 整个程序退出 2。
    let o = run_src(
        "t_con_07",
        r#"
package main

func main() {
	done := make(chan bool)
	go func() {
		for {
		}
	}()
	<-done
}
"#,
    );
    assert_eq!(o.exit, 2, "预算超限退出 2（`C9` §6）");
    assert!(
        o.stderr.contains("step budget exceeded"),
        "诊断要说清是预算：{}",
        o.stderr
    );
}

// ───────────────────────── Task 22：range over chan ─────────────────────────

#[test]
fn t_con_17_range_在空通道上阻塞到有值() {
    // `range` 会阻塞：`close` 之前它必须一直等，而不是「看到空就结束」。
    let s = out(
        "t_con_17",
        r#"
package main

func main() {
	c := make(chan int)
	go func() {
		println("g1")
		c <- 1
		close(c)
	}()
	for v := range c {
		println(v)
	}
	println("end")
}
"#,
    );
    assert_eq!(s, "g1\n1\nend\n");
}

#[test]
fn t_con_18_close_之后_range_先排空再结束() {
    let s = out(
        "t_con_18",
        r#"
package main

func main() {
	c := make(chan int, 3)
	c <- 1
	c <- 2
	close(c)
	for v := range c {
		println(v)
	}
	println("end")
}
"#,
    );
    assert_eq!(s, "1\n2\nend\n", "close 不排空缓冲，range 要把剩下的吐完");
}

#[test]
fn t_con_18b_range_期间_close_立刻结束() {
    let s = out(
        "t_con_18b",
        r#"
package main

func main() {
	c := make(chan int)
	go func() {
		c <- 1
		close(c)
	}()
	n := 0
	for range c {
		n++
	}
	println(n)
}
"#,
    );
    assert_eq!(s, "1\n");
}

#[test]
fn t_con_18c_range_nil_通道是死锁() {
    let o = run_src(
        "t_con_18c",
        r#"
package main

func main() {
	var c chan int
	for range c {
	}
}
"#,
    );
    assert_eq!(o.exit, 2, "nil 通道的 range 永久阻塞 ⇒ 死锁");
    assert!(
        o.stderr.contains("all goroutines are asleep - deadlock!"),
        "实际：{}",
        o.stderr
    );
}
