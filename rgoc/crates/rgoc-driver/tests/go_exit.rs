//! `GO-5` 死锁与退出（M11b）—— 判据骨架。
//!
//! `C9` §5 冻结：死锁 = `fatal error: all goroutines are asleep - deadlock!`
//! 进 **stderr**、退出 **2**、**不进合并流**（stdout 必须为空）。
//!
//! 这里先放死锁文本那条（逐字节）。三条条件各自必要 / `main` 返回即退出 /
//! G 内 panic 在 Task 27–28 补。

mod support;

const DEADLOCK: &str = r#"
package main

func main() {
	var c chan int
	<-c
}
"#;

/// 编译 + 跑，**允许非 0 退出码**。
///
/// `go_probe::probe` 会顺带跑 HIR 解释器（`hir_output`），而它把「非零退出」
/// 当错误 —— 死锁 / panic **就是**非零。这几条判据只关心 native 的三通道，
/// 所以直接走 `build_and_run_env`。
fn run(name: &str, src: &str) -> support::native_harness::Outcome {
    support::native_harness::build_and_run_env(name, src, &[("RGOC_GC", "0")])
        .unwrap_or_else(|e| panic!("`{name}` 应能编译并跑起来：\n{e}"))
}

#[test]
fn t_exit_01_死锁文本逐字节() {
    let t = run("exit01", DEADLOCK);
    assert_eq!(t.exit, 2, "死锁必须是退出码 2");
    assert_eq!(
        t.stderr, "fatal error: all goroutines are asleep - deadlock!\n",
        "死锁文本必须逐字节相同（`C9` §5）：实际 {:?}",
        t.stderr
    );
    assert!(
        t.stdout.is_empty(),
        "死锁文本**只**进 stderr（`C9` §4.1 不合并）：stdout={:?}",
        t.stdout
    );
}

/// `T-EXIT-02`：**`main` 返回即退出**（`C11` §5.1），不等其它 goroutine。
///
/// # 为什么这条是「不打印」而不是「打印了」
///
/// `go f()` 只把 f **入队**，M:1 不抢占 ⇒ main 继续跑到 `return`。此时
/// 进程直接退出（`_start` 的收尾），f 永远不会被调度。
///
/// 反过来的实现（「等所有 G 跑完再退出」）在这里会打印 `X` —— 而
/// `sieve1.go` 那种**无限**的 goroutine 链就永远退不出。这条判据钉的是
/// 「退出**不**取决于 goroutine」。
const MAIN_RETURNS: &str = r#"
package main

func main() {
	go func() {
		println("X")
	}()
}
"#;

#[test]
fn t_exit_02_main_返回即退出() {
    let out = run("exit02", MAIN_RETURNS);
    assert_eq!(out.exit, 0, "`main` 返回 ⇒ 退出 0：{:?}", out.stderr);
    assert!(
        out.stdout.is_empty(),
        "`main` 返回后不该再调度其它 G（`C11` §5.1）：stdout={:?}",
        out.stdout
    );
    assert!(out.stderr.is_empty(), "stderr={:?}", out.stderr);
}

/// `T-EXIT-03`：**任一 G 里未捕获的 panic ⇒ 整个程序退出 2**（`C9` §5）。
///
/// # 为什么这条不能靠 `t_chan_06`
///
/// `t_chan_06` 的 panic 发生在 **main** 的栈上 —— 走的是函数尾的 panic 出口
/// 与 `_start` 的收尾。这条发生在**子 G** 的栈上：包装函数返回后由
/// `rt_gorets` 检查 `rt_panicking()`。
///
/// 漏了 `rt_gorets` 的症状（实测过）：子 G 照常 `rt_goexit`，主 G 还在
/// `<-done` 上等 ⇒ 报 `all goroutines are asleep - deadlock!`，与**真死锁**
/// 无法区分。文本必须逐字节是 `panic: boom`，不是死锁那行。
const GOROUTINE_PANIC: &str = r#"
package main

func main() {
	done := make(chan int)
	go func() {
		panic("boom")
	}()
	<-done
}
"#;

#[test]
fn t_exit_03_g_内_panic_退出_2() {
    let out = run("exit03", GOROUTINE_PANIC);
    assert_eq!(out.exit, 2, "G 内未捕获 panic ⇒ 退出 2：{:?}", out.stderr);
    assert_eq!(
        out.stderr, "panic: boom\n",
        "文本不包装（`C9` §5）：实际 {:?}",
        out.stderr
    );
    assert!(out.stdout.is_empty(), "stdout={:?}", out.stdout);
}

/// `T-EXIT-04`：**goroutine 数硬上限 1024**（`M11-design.md` §4.4）。
///
/// 文本里的 `1024` 是 `G_MAX` 的字面值 —— 实现里**刻意不拼宏**，这样
/// 「文本与配置一致」这件事能被这条判据发现（拼宏会让两边永远一起变）。
#[test]
fn t_exit_04_goroutine_数上限() {
    let src = r#"
package main

func f() {}

func main() {
	for i := 0; i < 2000; i++ {
		go f()
	}
}
"#;
    let out = run("exit04", src);
    assert_eq!(out.exit, 2, "超限 ⇒ 退出 2：{:?}", out.stderr);
    assert_eq!(
        out.stderr, "runtime error: too many goroutines (limit 1024)\n",
        "超限文本必须逐字节相同：实际 {:?}",
        out.stderr
    );
}
