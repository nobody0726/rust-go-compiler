//! `GO-3` `hchan` 四操作（M11b）—— 判据骨架。
//!
//! `C11` §3.2 的四张表是**状态机**：发送方阻塞、接收方唤醒、`closed` 检查排第一。
//! 这里先放最基础的一条（无缓冲同步），其余（缓冲 FIFO / closed / len-cap /
//! nil 通道 / Sudog 不泄漏）在 Task 30 补。
//!
//! # 为什么「无缓冲同步」是第一条
//!
//! 它同时钉住三件事：① 发送方在无缓冲时必须**阻塞**；② 阻塞要**换 G**（不是
//! 忙等）；③ 唤醒后**值是对的**。任何一条错都会让它红，而它只有 5 行。

mod support;

use support::go_probe;

const SYNC: &str = r#"
package main

func main() {
	c := make(chan int)
	go func() {
		println(<-c)
	}()
	c <- 42
}
"#;

#[test]
#[ignore = "阻塞还没实现（Sudog 是 Task 18）：无缓冲 send 会走 rt_panic_blocked"]
fn t_chan_01_无缓冲同步() {
    let t = go_probe::probe("chan01", SYNC, &[("RGOC_GC", "0")])
        .expect("应能编译并跑起来（M11b 之后）");
    t.assert_l1("chan01");
    assert_eq!(t.native.stdout, "42\n", "实际 {:?}", t.native.stdout);
    assert_eq!(t.native.exit, 0, "stderr={:?}", t.native.stderr);
}

/// 空 `main`：只用来把 chan 自测跑起来。
const MIN: &str = "\npackage main\n\nfunc main() {\n}\n";

/// `T-CHAN-02`（Phase 2 形态）：**有缓冲通道的 send/recv 直通 + FIFO**。
///
/// # 为什么这条现在就能有
///
/// 它只验「缓冲未满的 send」与「缓冲非空的 recv」—— **不需要阻塞**
/// （`C11` §3.2 四张表里前两行）。阻塞（`recvq` / `sendq` / Sudog）是 Task 18。
///
/// 自测在 `rt.c` 的 `rt_chan2test`：`cap = 3` 发 11/22/33 再收三次，断言
/// FIFO 顺序；然后置 `closed`，断言 recv 拿到**零值**（`C11` §3.2 的
/// `closed && qcount == 0` 那一格）。
///
/// # 三个退出码分开
///
/// * **0** = 自测通过
/// * **8** = 自测发现值不对（FIFO 或零值）
/// * **9** = 走到了还没实现的阻塞路径（Task 18）
///
/// 分开是刻意的：红了就能直接分辨「实现有 bug」与「这一块还没做」。
/// 判据只认 0 —— 8 与 9 都是红，但错误信息不同。
#[test]
fn t_chan_02_缓冲_fifo() {
    let out =
        support::native_harness::build_and_run_env("chan02", MIN, &[("RGOC_G11_CHAN2TEST", "1")])
            .unwrap_or_else(|e| panic!("chan 自测失败（8 = 值不对，9 = 阻塞路径未实现）：\n{e}"));
    assert_eq!(out.exit, 0, "chan 自测应干净退出");
    assert!(out.stderr.is_empty(), "自测不该有输出：{:?}", out.stderr);
}
