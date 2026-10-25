//! `select` 的三阶段（`M8-plan.md` Task 29–38；`T-CON-25..34`）。
//!
//! # 判据的形状
//!
//! select 的正确性全在**顺序**里：求值顺序（`T-CON-33`）、多就绪取哪个
//! （`T-CON-28/29`）、撤销干不干净（`T-CON-31`）。所以 fixture 里每个分支
//! 都打印自己是谁，断言就是「哪些行、什么顺序」。
//!
//! # `T-CON-33` 为什么用「有副作用的通道表达式」
//!
//! 规范：**选中 `default` 时，其他 case 的通道表达式也已经求过值**。
//! 用 `mk(1)` / `mk(2)` 这种会打印的函数当通道表达式，才能把「求过没有」
//! 变成可观测的字节 —— 用普通变量判不出来（取值没有副作用）。

use std::path::PathBuf;

use rgoc_driver::cli::{RunCommand, Sched, Via};
use rgoc_driver::run_cmd::{RunOutcome, run_run};

fn run_with(name: &str, src: &str, sched: Sched) -> RunOutcome {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/con-select");
    std::fs::create_dir_all(&dir).expect("建不了 target/tmp/con-select");
    let path = dir.join(format!("{name}.go"));
    std::fs::write(&path, src).expect("写不了 fixture");
    run_run(&RunCommand {
        files: vec![path],
        dump_hir: false,
        max_steps: Some(2_000_000),
        lang: None,
        via: Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
        sched,
    })
    .expect("内部错（`run_run` 返回 Err）")
}

#[track_caller]
fn out(name: &str, src: &str) -> String {
    let o = run_with(name, src, Sched::default());
    assert_eq!(o.exit, 0, "{name}: 退出码 {}，stderr={}", o.exit, o.stderr);
    assert!(o.stderr.is_empty(), "{name}: stderr 非空：{}", o.stderr);
    o.stdout.replace("\r\n", "\n")
}

// ───────────────────────── Task 29/30：求值与试探 ─────────────────────────

#[test]
fn t_con_33_走_default_时所有_case_的通道表达式都求值过() {
    let s = out(
        "t_con_33",
        r#"
package main

func mk(n int) chan int {
	println("eval", n)
	return make(chan int)
}

func main() {
	a := mk(1)
	b := mk(2)
	select {
	case <-a:
		println("a")
	case <-b:
		println("b")
	default:
		println("default")
	}
}
"#,
    );
    assert_eq!(s, "eval 1\neval 2\ndefault\n");
}

#[test]
fn t_con_25_select_没有就绪就走_default() {
    let s = out(
        "t_con_25",
        r#"
package main

func main() {
	c := make(chan int)
	select {
	case <-c:
		println("recv")
	default:
		println("default")
	}
}
"#,
    );
    assert_eq!(s, "default\n");
}

#[test]
fn t_con_26_select_recv_就绪() {
    let s = out(
        "t_con_26",
        r#"
package main

func main() {
	c := make(chan int, 1)
	c <- 7
	select {
	case v := <-c:
		println("got", v)
	default:
		println("default")
	}
}
"#,
    );
    assert_eq!(s, "got 7\n");
}

#[test]
fn t_con_36_select_send_就绪() {
    let s = out(
        "t_con_36",
        r#"
package main

func main() {
	c := make(chan int, 1)
	select {
	case c <- 1:
		println("sent")
	default:
		println("default")
	}
	println(<-c)
}
"#,
    );
    assert_eq!(s, "sent\n1\n");
}

#[test]
fn t_con_36b_select_send_无接收方且无_default_则阻塞() {
    // 无缓冲 + 没有接收方 ⇒ 这一支必须阻塞到有人来收。
    let s = out(
        "t_con_36b",
        r#"
package main

func main() {
	c := make(chan int)
	go func() {
		println(<-c)
	}()
	select {
	case c <- 42:
		println("sent")
	}
}
"#,
    );
    assert_eq!(s, "42\nsent\n");
}

// ───────────────────────── Task 31：阻塞与多路登记 ─────────────────────────

#[test]
fn t_con_27_select_阻塞到第三个_goroutine_发() {
    let s = out(
        "t_con_27",
        r#"
package main

func main() {
	a := make(chan int)
	b := make(chan int)
	go func() { a <- 1 }()
	select {
	case v := <-a:
		println("a", v)
	case v := <-b:
		println("b", v)
	}
}
"#,
    );
    assert_eq!(s, "a 1\n");
}

#[test]
fn t_con_31_重复_channel_两个_case_各跑一次() {
    // 两个 case 盯着同一条通道：唤醒时只有一个 `done`，另一个必须靠 token
    // 撤掉 —— 否则它会留在 `recvq` 里，把下一笔发送提前吃掉。
    let s = out(
        "t_con_31",
        r#"
package main

func main() {
	c := make(chan int)
	go func() {
		c <- 1
		c <- 2
	}()
	select {
	case v := <-c:
		println("first", v)
	case v := <-c:
		println("second", v)
	}
	select {
	case v := <-c:
		println("third", v)
	}
}
"#,
    );
    assert_eq!(s, "first 1\nthird 2\n");
}

// ───────────────────────── Task 34/35：空 select 与 comma-ok ─────────────────────────

#[test]
fn t_con_32_空_select_是死锁() {
    let o = run_with(
        "t_con_32",
        r#"
package main

func main() {
	select {}
}
"#,
        Sched::default(),
    );
    assert_eq!(o.exit, 2, "空 select 永远阻塞 ⇒ 死锁");
    assert!(
        o.stderr.contains("all goroutines are asleep - deadlock!"),
        "实际：{}",
        o.stderr
    );
}

#[test]
fn t_con_34_select_recv_的_comma_ok() {
    let s = out(
        "t_con_34",
        r#"
package main

func main() {
	c := make(chan int, 1)
	c <- 5
	close(c)
	select {
	case v, ok := <-c:
		println(v, ok)
	}
	select {
	case v, ok := <-c:
		println(v, ok)
	}
}
"#,
    );
    assert_eq!(s, "5 true\n0 false\n", "排空后 ok 变 false，值是零值");
}

// ───────────────────────── Task 37：seed 选择 ─────────────────────────

#[test]
fn t_con_28_多就绪默认取源码顺序第一个() {
    // 同一 fixture 跑 8 次，必须 8 次都是第一个 case（`U-M8-02` 的守卫）。
    let src = r#"
package main

func main() {
	a := make(chan int, 1)
	b := make(chan int, 1)
	a <- 1
	b <- 2
	for i := 0; i < 8; i++ {
		select {
		case v := <-a:
			println("a", v)
			a <- 1
		case v := <-b:
			println("b", v)
			b <- 2
		}
	}
}
"#;
    let first = out("t_con_28", src);
    for k in 0..8 {
        let again = out(&format!("t_con_28_{k}"), src);
        assert_eq!(again, first, "第 {k} 次跑出了不同结果");
    }
    assert_eq!(first, "a 1\n".repeat(8));
}

#[test]
fn t_con_29_多_seed_至少出现两种结果() {
    // `seed` 必须真的改变选择（不需要均匀，只需要「seed 起作用」）。
    let src = r#"
package main

func main() {
	a := make(chan int, 1)
	b := make(chan int, 1)
	a <- 1
	b <- 2
	select {
	case v := <-a:
		println("a", v)
	case v := <-b:
		println("b", v)
	}
}
"#;
    let mut seen = std::collections::BTreeSet::new();
    for seed in 1..=8u64 {
        let o = run_with(
            &format!("t_con_29_{seed}"),
            src,
            Sched::Deterministic { seed },
        );
        assert_eq!(o.exit, 0, "seed={seed} 退出码 {}", o.exit);
        seen.insert(o.stdout.replace("\r\n", "\n"));
    }
    assert!(
        seen.len() >= 2,
        "8 个 seed 只给出 {} 种结果：{seen:?} —— seed 没起作用",
        seen.len()
    );
}
