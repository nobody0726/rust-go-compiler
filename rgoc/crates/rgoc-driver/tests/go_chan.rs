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

/// `T-CHAN-03`：**阻塞 + 唤醒**（`C11` §3.2 的「登记 / 阻塞 / 唤醒」三行）。
///
/// # 这条验的是什么
///
/// 无缓冲通道、sender G 发 42、main 收。两边各阻塞一次：
///
/// 1. main 先 `rt_recv`（此刻 sender 只入队还没跑）⇒ `recvq` 空 ⇒ 登记 Sudog
///    + `g_cur->state = blocked` + `rt_schedule()`；
/// 2. 调度到 sender ⇒ `rt_send` 看到 `recvq` 里有 main ⇒ 拷值 + `rt_wake`；
/// 3. sender `rt_goexit` ⇒ 切回 main ⇒ `rt_recv` 从 `rt_schedule` 正确返回。
///
/// 任一处错都会**挂死**或拿到错值 —— 而「挂死」正是最难在 CI 里发现的形态，
/// 所以自测用 `timeout` 兜着（`run-native-chan.sh` 里有）。
///
/// # 三个退出码（与 `t_chan_02` 同一套）
///
/// **0** 通过 / **8** 值不对 / **9** 阻塞路径未实现（现在应该不会出现）。
#[test]
fn t_chan_03_阻塞与唤醒() {
    let out =
        support::native_harness::build_and_run_env("chan03", MIN, &[("RGOC_G11_CHAN3TEST", "1")])
            .unwrap_or_else(|e| panic!("chan 阻塞自测失败（8 = 值不对）：\n{e}"));
    assert_eq!(out.exit, 0, "阻塞 + 唤醒应干净退出");
    assert!(out.stderr.is_empty(), "自测不该有输出：{:?}", out.stderr);
}

/// `T-CHAN-04`：**`v, ok := <-c` 的 comma-ok**（`C11` §3.2 最后一格）。
///
/// # 为什么这条必须单独存在
///
/// `rt_recv` 返回 `u64`（1 = 真的收到值，0 = 已关闭且空），而**单值形态会
/// 忽略它**。如果 isel 把返回值接错了槽（或者 `zero_slot` 没做），症状是
/// 「`ok` 恒为 true」—— 而所有单值用例都照样绿。
///
/// 判据用两段：`ok == true`（有值）与 `ok == false`（关闭且空）。只查前者
/// 会被「恒 true」骗过。
const COMMA_OK: &str = r#"
package main

func main() {
	c := make(chan int, 1)
	c <- 7
	v, ok := <-c
	println(v, ok)
}
"#;

#[test]
fn t_chan_04_comma_ok() {
    let t = go_probe::probe("chan04", COMMA_OK, &[("RGOC_GC", "0")]).expect("应能编译并跑起来");
    t.assert_l1("chan04");
    assert_eq!(
        t.native.stdout, "7 true\n",
        "`v, ok := <-c`（有值）应为 `7 true`：实际 {:?}",
        t.native.stdout
    );
    assert_eq!(t.native.exit, 0, "stderr={:?}", t.native.stderr);
}

/// `T-CHAN-05`：**`close` 之后「先排空 buf，再拿零值」**（`C11` §3.2 的 close 那行）。
///
/// # 为什么这条要一次查五件事
///
/// `close` 的语义是「不再有新值，但**已有的值还能读**」。所以 `close` 之后：
/// * `len(c)` 仍是缓冲里剩下的个数（**不是** 0）；
/// * `cap(c)` 不变；
/// * 前两次 `<-c` 拿到 `1 true` / `2 true`（真值 + ok）；
/// * 第三次拿到 `0 false`（零值 + ok 为假）。
///
/// 少查任何一条都会漏掉一整类错：只查「零值」的话，把 buf 直接丢弃也能过。
const CLOSE_DRAIN: &str = r#"
package main

func main() {
	c := make(chan int, 2)
	c <- 1
	c <- 2
	close(c)
	println(len(c), cap(c))
	for i := 0; i < 3; i++ {
		v, ok := <-c
		println(v, ok)
	}
}
"#;

#[test]
fn t_chan_05_close_后先排空再零值() {
    let t = go_probe::probe("chan05", CLOSE_DRAIN, &[("RGOC_GC", "0")]).expect("应能编译并跑起来");
    t.assert_l1("chan05");
    assert_eq!(
        t.native.stdout, "2 2\n1 true\n2 true\n0 false\n",
        "`close` 后必须先排空 buf 再拿零值：实际 {:?}",
        t.native.stdout
    );
    assert_eq!(t.native.exit, 0, "stderr={:?}", t.native.stderr);
}

/// `T-CHAN-06`：**三条 chan panic 的文本逐字节**（`C11` §5）。
///
/// # 为什么文本要逐字节
///
/// `C9` §4.5 冻结了 panic 的输出形状：`panic: <文本>\n` 进 stderr、退出 2。
/// 三条 chan panic 的 `<文本>` 分别是 `send on closed channel` /
/// `close of closed channel` / `close of nil channel`。
///
/// ⚠️ 实现里**不能**把 `panic: ` 写进文本 —— `rt_print_panic` 自己加那 7 个
/// 字节。带上就会输出 `panic: panic: close of closed channel`（实测踩过）。
///
/// 这条用 `assert_eq!` 整串比而不是 `contains`：多一个前缀、少一个换行都要红。
#[test]
fn t_chan_06_三条_panic_文本() {
    // ① 往已关闭的通道发送（缓冲未满 —— 检查必须排第一）
    let send_closed = r#"
package main

func main() {
	c := make(chan int, 2)
	close(c)
	c <- 1
}
"#;
    // ② 重复关闭
    let close_closed = r#"
package main

func main() {
	c := make(chan int)
	close(c)
	close(c)
}
"#;
    // ③ 关闭 nil 通道
    let close_nil = r#"
package main

func main() {
	var c chan int
	close(c)
}
"#;
    for (name, src, want) in [
        ("chan06a", send_closed, "panic: send on closed channel\n"),
        ("chan06b", close_closed, "panic: close of closed channel\n"),
        ("chan06c", close_nil, "panic: close of nil channel\n"),
    ] {
        let out = support::native_harness::build_and_run_env(name, src, &[])
            .unwrap_or_else(|e| panic!("`{name}` 应能编译并跑起来：\n{e}"));
        assert_eq!(out.exit, 2, "`{name}` 的 panic 应退出 2：{:?}", out.stderr);
        assert_eq!(
            out.stderr, want,
            "`{name}` 的 panic 文本必须逐字节相同（`C11` §5）：实际 {:?}",
            out.stderr
        );
        assert!(
            out.stdout.is_empty(),
            "`{name}` panic 前不该有输出：{:?}",
            out.stdout
        );
    }
}

/// `T-CHAN-07`：**阻塞中的发送方被 `close` 唤醒 ⇒ 在它自己的 G 里 panic**。
///
/// # 为什么这条与 `T-CHAN-06` 的 ① 不同
///
/// ① 是「已经关闭，然后再发」—— `rt_send` 的 `closed` 检查直接命中。
/// 这条是「**先阻塞**，再被 `close` 唤醒」—— 走的是「醒来发现 `done == 0` ⇒
/// 循环回去重试 ⇒ 这次才看到 `closed`」那条路。
///
/// 两条路都必要：只测 ① 的话，`rt_send` 的阻塞分支写成「醒来就当成发成功了」
/// 也照样绿（变异 `M-M11-08` 把 `close` 的 `sendq` 唤醒删掉，症状就是这条挂死）。
///
/// # 为什么需要 `ready` 这个会合握手
///
/// ⚠️ **第一版没有它，变异 `M-M11-08` 跑绿了**：子 G 还没被调度过就
/// `close(c)` 了，`sendq` 是**空的** —— 于是 `close` 唤不唤醒 `sendq`
/// 根本不影响结果，子 G 后面自己撞上 `closed` 检查。
///
/// `ready <- 1` 保证子 G 真的走到了 `c <- 1` 并**阻塞**：
/// M:1 不抢占 ⇒ 子 G 发完 `ready` 会继续往下跑（主 G 只是被**入队**，
/// 还没被切换过去），直到 `c <- 1` 阻塞才让出。主 G 这时才 `close`，
/// `sendq` 里躺着那个子 G。
const SEND_BLOCKED_CLOSED: &str = r#"
package main

func main() {
	c := make(chan int)
	ready := make(chan int)
	done := make(chan int)
	go func() {
		ready <- 1
		c <- 1
		println("BAD")
		done <- 1
	}()
	<-ready
	close(c)
	<-done
}
"#;

#[test]
fn t_chan_07_阻塞发送被_close_唤醒后_panic() {
    let out = support::native_harness::build_and_run_env("chan07", SEND_BLOCKED_CLOSED, &[])
        .expect("应能编译并跑起来");
    assert_eq!(
        out.exit, 2,
        "阻塞中的发送方被 close 唤醒后必须 panic（退出 2）：实际 exit={} stderr={:?}",
        out.exit, out.stderr
    );
    assert!(
        out.stderr.contains("panic: send on closed channel"),
        "panic 文本不对：{:?}",
        out.stderr
    );
    assert!(
        out.stdout.is_empty(),
        "panic 前不该有输出（`done <- 1` 不该跑到）：{:?}",
        out.stdout
    );
}
