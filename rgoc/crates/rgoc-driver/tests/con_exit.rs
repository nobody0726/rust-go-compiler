//! 死锁与退出（`M8-plan.md` Task 39–44；`T-CON-35..42`）。
//!
//! # 这一层的判据为什么都在「边界」上
//!
//! 正常路径已经被 `con_chan` / `con_select` 覆盖。这里只判**异常出口**：
//! 死锁怎么报、`main` 返回时不等谁、panic 从哪个 G 发出来、被丢弃的 G 跑不跑
//! defer。这些是「程序shape 正确但结局错了」的那一类 —— 输出看着对，退出码错。
//!
//! # 退出码分界（`C9` §5）
//!
//! | 情形 | 退出码 | 输出去向 |
//! |---|---|---|
//! | 正常结束 | 0 | stdout |
//! | 未捕获 panic | 2 | `panic: …` 进**合并流** |
//! | 死锁 | 2 | `fatal error: …` 进 **stderr** |
//! | 预算 / 帧深超限 | 2 | 诊断进 stderr |
//! | 静态拒绝（诊断非空） | 1 | stderr |

use std::path::PathBuf;

use rgoc_driver::cli::{RunCommand, Sched, Via};
use rgoc_driver::run_cmd::{RunOutcome, run_run};

fn run_src(name: &str, src: &str) -> RunOutcome {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/con-exit");
    std::fs::create_dir_all(&dir).expect("建不了 target/tmp/con-exit");
    let path = dir.join(format!("{name}.go"));
    std::fs::write(&path, src).expect("写不了 fixture");
    run_run(&RunCommand {
        files: vec![path],
        dump_hir: false,
        max_steps: Some(200_000),
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

// ───────────────────────── Task 39/43：死锁 ─────────────────────────

#[test]
fn t_con_35_真死锁的字节面() {
    let o = run_src(
        "t_con_35",
        r#"
package main

func main() {
	c := make(chan int)
	<-c
}
"#,
    );
    assert_eq!(o.exit, 2);
    assert_eq!(
        o.stderr, "fatal error: all goroutines are asleep - deadlock!\n",
        "死锁文本走 stderr，且**不带**文件位置前缀"
    );
    assert!(
        !o.stdout.contains("deadlock"),
        "死锁文本不进合并流：{}",
        o.stdout
    );
}

#[test]
fn t_con_35b_两个_goroutine_互等也是死锁() {
    let o = run_src(
        "t_con_35b",
        r#"
package main

func main() {
	a := make(chan int)
	b := make(chan int)
	go func() { <-a }()
	go func() { <-b }()
	<-a
}
"#,
    );
    assert_eq!(o.exit, 2);
    assert!(o.stderr.contains("deadlock"), "实际：{}", o.stderr);
}

// ───────────────────────── Task 40/42：main 返回即退出 ─────────────────────────

#[test]
fn t_con_36_main_返回时丢弃还在跑的_goroutine() {
    // 这个 G 会无限打印。若实现「等所有 G 结束」，程序永远不会退出；
    // 若实现「按步数抢占」，它会被切走然后 main 才返回。正确行为：
    // main 一返回，整个进程立刻结束 ⇒ 输出只有 main 那一行。
    let o = run_src(
        "t_con_36",
        r#"
package main

func main() {
	go func() {
		for {
			println("spin")
		}
	}()
	println("main")
}
"#,
    );
    assert_eq!(o.exit, 0, "main 正常返回 ⇒ 退出 0");
    assert_eq!(
        o.stdout.replace("\r\n", "\n"),
        "main\n",
        "spin 一行都不该出现"
    );
}

#[test]
fn t_con_40_被丢弃的_goroutine_不跑_defer() {
    let s = out(
        "t_con_40",
        r#"
package main

func main() {
	go func() {
		defer println("never")
		for {
		}
	}()
	println("main")
}
"#,
    );
    assert_eq!(s, "main\n", "main 返回时不做任何 teardown");
}

// ───────────────────────── Task 41：goroutine 内 panic ─────────────────────────

#[test]
fn t_con_37_goroutine_内未捕获_panic_终止全程序() {
    let o = run_src(
        "t_con_37",
        r#"
package main

func boom() { panic("bang") }

func main() {
	// main 必须**阻塞**才能把 CPU 让给 boom —— M:1 不抢占，
	// `for {}` 会把预算烧光也轮不到它（那由 `t_con_07` 单独判）。
	done := make(chan bool)
	go boom()
	<-done
}
"#,
    );
    assert_eq!(o.exit, 2, "panic 退出码 2");
    assert!(
        o.stdout.contains("panic: bang"),
        "panic 值不包装，进合并流：{}",
        o.stdout
    );
}

#[test]
fn t_con_39_panic_之后不再有输出() {
    let o = run_src(
        "t_con_39",
        r#"
package main

func main() {
	done := make(chan bool)
	go func() { panic(42) }()
	<-done
}
"#,
    );
    assert_eq!(o.exit, 2);
    assert!(o.stdout.contains("panic: 42"), "实际：{}", o.stdout);
    assert!(
        !o.stdout.contains("after"),
        "panic 之后 main 不该被唤醒继续跑：{}",
        o.stdout
    );
}

// ───────────────────────── Task 44：反例集 ─────────────────────────

#[test]
fn t_con_44_不该判死锁的两种形状() {
    // ① G 阻塞但 main 还能跑完 ⇒ 正常退出 0。
    let s = out(
        "t_con_44a",
        r#"
package main

func main() {
	c := make(chan int)
	go func() { <-c }()
	println("main")
}
"#,
    );
    assert_eq!(s, "main\n");

    // ② main 已经返回 ⇒ 不等任何人。
    let s2 = out(
        "t_con_44b",
        r#"
package main

func main() {
	c := make(chan int)
	go func() { c <- 1 }()
	println("main")
}
"#,
    );
    assert_eq!(s2, "main\n");
}

#[test]
fn t_con_41_退出码_1_与_2_的分界() {
    // 静态拒绝 ⇒ 1；运行期 panic ⇒ 2。两者都是「非 0」，但含义不同。
    let o = run_src(
        "t_con_41",
        "package main\n\nfunc main() {\n\tvar c chan int\n\tclose(c)\n}\n",
    );
    assert_eq!(o.exit, 2, "close(nil) 是运行期 panic");

    let bad = run_src(
        "t_con_41b",
        "package main\n\nfunc main() {\n\tx := 1\n\ty := x +\n}\n",
    );
    assert_eq!(bad.exit, 1, "语法错 ⇒ 静态面退出 1");
}

#[test]
fn t_con_42_交错的顺序是确定的() {
    // 同一 fixture 跑两次，输出必须逐字节相同（`CON-7` 的可回放性）。
    let src = r#"
package main

func main() {
	done := make(chan bool)
	go func() {
		for i := 0; i < 3; i++ {
			println("g", i)
		}
		done <- true
	}()
	for i := 0; i < 3; i++ {
		println("m", i)
	}
	<-done
}
"#;
    let a = out("t_con_42_a", src);
    let b = out("t_con_42_b", src);
    assert_eq!(a, b, "同 seed 两次必须逐字节相同");
    assert_eq!(a, "m 0\nm 1\nm 2\ng 0\ng 1\ng 2\n");
}
