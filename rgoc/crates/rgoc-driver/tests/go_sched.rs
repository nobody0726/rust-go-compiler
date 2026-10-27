//! `GO-2` 调度三规则（M11b）—— 判据骨架。
//!
//! `C11` §2.1 三条：**M:1**（一个 OS 线程）/ **不抢占**（只在阻塞点让出）/
//! **就绪队列 FIFO**。这里先放 FIFO 那条（最容易写错成「队头插入」），
//! M:1 与不抢占的判据在 Task 14 补。
//!
//! # 为什么用「各自发信号」而不是 `for {}` + 超时
//!
//! FIFO 的判据要**确定的输出**。用 `for {}` 只能证明「挂死」，证明不了顺序。
//! 而 5 个 G 按创建顺序入队、各自跑完发信号，输出必须恰好是 `01234` ——
//! 倒序（队头插入）或乱序（真并发）都会红。

mod support;

use support::go_probe;

const FIFO: &str = r#"
package main

func main() {
	done := make(chan bool)
	for i := 0; i < 5; i++ {
		go func(n int) {
			print(n)
			done <- true
		}(i)
	}
	for i := 0; i < 5; i++ {
		<-done
	}
	println()
}
"#;

#[test]
#[ignore = "M11b 实现后启用（Task 31 收口）：现在还没有 go 语句"]
fn t_go_03_就绪队列_fifo() {
    let t =
        go_probe::probe("go03", FIFO, &[("RGOC_GC", "0")]).expect("应能编译并跑起来（M11b 之后）");
    t.assert_l1("go03");
    assert_eq!(
        t.native.stdout, "01234\n",
        "就绪队列必须是 FIFO（队尾入队）：实际 {:?}",
        t.native.stdout
    );
    assert_eq!(t.native.exit, 0, "stderr={:?}", t.native.stderr);
}

/// 空 `main`：只用来把调度自测跑起来。
const MIN: &str = "\npackage main\n\nfunc main() {\n}\n";

/// `T-GO-01`（Phase 1 形态）：**调度骨架能切两个 G，且顺序是 FIFO**。
///
/// # 为什么这条在 `go` 语句之前就要有
///
/// `rt_switch` 在 Task 9 已经单独验过（`t_stk_01`：寄存器对称）。但「切换对了」
/// 不等于「调度对了」—— 就绪队列、`rt_goexit`、`main` 也入队这三件事，任一处
/// 写错都会让 G 跑不起来或顺序错乱，而那时**没有任何判据会红**（只是输出不对）。
///
/// 自测装置在 `rt.c` 的 `rt_schedtest`：建两个 G（A 打印 `A`、B 打印 `B`），
/// `main` 也入队，`RGOC_G11_SCHEDTEST=1` 触发。
///
/// # 为什么断言的是**顺序**而不只是「都跑了」
///
/// 队头插入会让输出变成 `B\nA\n`，而「都跑了」照样成立 —— 那正是变异
/// `M-M11-06` 要抓的形态。Task 12 有 `go` 语句之后，这条会被 `t_go_03`
/// 用 Go 源码写的同样形状取代。
#[test]
fn t_go_01_调度骨架_fifo() {
    let out =
        support::native_harness::build_and_run_env("go01", MIN, &[("RGOC_G11_SCHEDTEST", "1")])
            .expect("调度自测应能跑起来");
    assert_eq!(
        out.stdout, "A\nB\n",
        "就绪队列必须是 FIFO（A 先入队 ⇒ 先跑）：实际 {:?}",
        out.stdout
    );
    assert_eq!(out.exit, 0, "stderr={:?}", out.stderr);
}

/// `T-GO-02`（Phase 1 形态）：**`rt_go` 能启动一个新 G 并跑完**。
///
/// # 为什么这条与 `t_go_01` 分开
///
/// `t_go_01` 验的是「调度」（队列 FIFO、`rt_goexit`）；这条验的是「**启动**」——
/// 新 G 的栈布局（实参区 + fn + 160 字节保存区）、`rt_go_trampoline` 的
/// `ldp`/`blr`/`rt_goexit`。任一处偏移算错都会让 G 跑飞，而 `t_go_01` 用的是
/// 手工构造的栈（`sc_mkg`），**绕过了 `rt_go` 那段布局代码**。
///
/// # 为什么现在还不能用 Go 源码写
///
/// `go` 语句在 Task 13 才放开（`unsup_stmt` 仍拒 `Stmt::Go`）。所以自测在
/// `rt.c` 的 `rt_gotest`：手工调 `rt_go(sc_g_a, 0, 0)`，期望输出 `A`。
/// Task 13 之后这条会被 `t_go_03`（真 Go 源码）取代。
#[test]
fn t_go_02_rt_go_能启动新_g() {
    let out = support::native_harness::build_and_run_env("go02", MIN, &[("RGOC_G11_GOTEST", "1")])
        .expect("`rt_go` 自测应能跑起来");
    assert_eq!(
        out.stdout, "A\n",
        "`rt_go` 应启动一个新 G 并跑完（新栈布局或 trampoline 算错了）：实际 {:?}",
        out.stdout
    );
    assert_eq!(out.exit, 0, "stderr={:?}", out.stderr);
}

/// `T-GO-01`（`GO-2` 的第一条）：**M:1 —— 只有一个 OS 线程**。
///
/// # 为什么扫 `rt.c` 而不是扫产物
///
/// 产物是 freestanding ELF，没有 libc，`pthread_create` 根本不可能出现 ——
/// 扫产物是一条**永远绿**的断言。真正会破坏 M:1 的地方是 runtime 自己：
/// 只要有人在 `rt.c` 里加一次 `SYS_clone`（或 `clone3`），就会变成 M:N，
/// 而那时**没有任何判据会红**（程序照样跑对，只是不再确定）。
///
/// 这条扫的是**源码**：`rt.c` 里的系统调用白名单只有 `mmap` / `write` / `exit`。
#[test]
fn t_go_04_m1_只有一个线程() {
    let rt = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../runtime/native/aarch64-unknown-linux-gnu/rt.c"),
    )
    .expect("读不到 rt.c");
    for bad in ["SYS_clone", "SYS_clone3", "pthread_create", "SYS_fork"] {
        assert!(
            !rt.contains(bad),
            "`rt.c` 里出现了 `{bad}` —— M11a 是 **M:1**（`C11` §2.1），\
             多一个线程就会让「同 seed 逐字节可回放」失效"
        );
    }
}

/// `T-GO-05`（`GO-2` 的第三条）：**同一程序跑两次，输出逐字节相同**。
///
/// # 为什么确定性是 M:1 的判据而不是「运气好」
///
/// M:1 + 不抢占 + 就绪队列 FIFO ⇒ 执行顺序**只由源码决定**。所以「跑两次
/// 一样」不是巧合，而是这三条规则的推论。反过来，任何一条被破坏（真的并发了、
/// 或者队列变成 LIFO），这条都会红 —— 而且它比 `t_go_01` 更狠：`t_go_01`
/// 只跑一次，靠期望字符串；这条不写期望，纯靠**自比**。
#[test]
fn t_go_05_同一程序两次输出一致() {
    let a = support::native_harness::build_and_run_env("go05a", MIN, &[("RGOC_G11_GOTEST", "1")])
        .expect("第一次应跑起来");
    let b = support::native_harness::build_and_run_env("go05b", MIN, &[("RGOC_G11_GOTEST", "1")])
        .expect("第二次应跑起来");
    assert_eq!(
        a.stdout, b.stdout,
        "同一程序两次输出不同 ⇒ 执行顺序不再只由源码决定（M:1 / 不抢占 / FIFO 有一条破了）"
    );
    assert_eq!(a.exit, b.exit, "退出码也要一致");
}
