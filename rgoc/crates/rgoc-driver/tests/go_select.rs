//! `GO-4` `select` 三阶段（M11b）—— 判据骨架。
//!
//! `C11` §4 三阶段：① 所有 case 的 chan 表达式 + send 右值**各求值一次**；
//! ② 源码顺序试探；③ 都没就绪且无 `default` ⇒ 登记 + 换 G。
//!
//! 这里先放阶段①（最容易漏：只求值第一个 case、或者求值两次）。
//! 撤销 / 重复 channel / 空 select 在 Task 24–25 补。

mod support;

use support::go_probe;

/// ⚠️ **Phase 0 的初版 fixture 是非法 Go**（`f` 返回 `chan int`，却往
/// `chan int` 里发）—— 那时 `select` 还被静态拒绝，判据是 `#[ignore]`，
/// 谁也没编译过它。启用时才发现。教训：**骨架的 fixture 也要能过 check**，
/// 否则它证明的只是「装置会报错」。
const EVAL_ONCE: &str = r#"
package main

var n = 0

func f() int {
	n++
	return 1
}

func main() {
	a := make(chan int, 1)
	b := make(chan int, 1)
	select {
	case a <- f():
	case b <- f():
	}
	println(n)
}
"#;

#[test]
fn t_sel_01_求值一次() {
    let t = go_probe::probe("sel01", EVAL_ONCE, &[("RGOC_GC", "0")])
        .expect("应能编译并跑起来（M11b 之后）");
    t.assert_l1("sel01");
    assert_eq!(
        t.native.stdout, "2\n",
        "阶段① 必须给**每个** case 求值一次（不是 0 次、不是 4 次）：实际 {:?}",
        t.native.stdout
    );
}

/// `T-SEL-02`：**走 `default` 也要把每个 case 求值一次**（阶段① 与阶段② 无关）。
///
/// # 为什么这条与 `t_sel_01` 分开
///
/// `t_sel_01` 里两个 case 都就绪（`cap=1` 的缓冲通道），所以它证明的是
/// 「选中的那条路上求值了」。这条**没有任何 case 就绪**（全部无缓冲且无对端），
/// 一定走 `default` —— 如果实现把阶段① 挪到「试探某个 case 时才求值」，
/// 这里 `n` 会是 0。
///
/// Go 规范：`select` 的**所有** case 的通道表达式与 send 右值都在选择开始前
/// 求值，与最后选中哪一条无关。
const EVAL_ON_DEFAULT: &str = r#"
package main

var n = 0

func f() int {
	n++
	return 1
}

func main() {
	a := make(chan int)
	b := make(chan int)
	select {
	case a <- f():
	case b <- f():
	default:
	}
	println(n)
}
"#;

#[test]
fn t_sel_02_走_default_也求值() {
    let t =
        go_probe::probe("sel02", EVAL_ON_DEFAULT, &[("RGOC_GC", "0")]).expect("应能编译并跑起来");
    t.assert_l1("sel02");
    assert_eq!(
        t.native.stdout, "2\n",
        "`default` 分支也要先把两个 case 都求值一次：实际 {:?}",
        t.native.stdout
    );
}

/// `T-SEL-03`：**多就绪时取源码顺序的第一个**（`C11` §4 / `D-M8-3`）。
///
/// # 为什么这条能区分「源码顺序」与「随便挑一个」
///
/// 两个通道都预置了值（都就绪）。取第一个 ⇒ `1`；取最后一个 ⇒ `2`。
/// M11b 是**确定性**的（M:1 + 不抢占 + 源码顺序），所以这条必须是稳定输出 ——
/// 偶发红就说明调度不再是确定的。
///
/// ⚠️ 这也是「不拿官方 `select` 当主判据」的原因：官方在多就绪时**随机**
/// 挑（`U-M11-07`），同一段源码两次可能不同。这里的语义由 `C11` §4 冻结，
/// 是**我们自己**的确定性契约。
const FIRST_READY: &str = r#"
package main

func main() {
	a := make(chan int, 1)
	b := make(chan int, 1)
	a <- 1
	b <- 2
	select {
	case v := <-a:
		println(v)
	case v := <-b:
		println(v)
	}
}
"#;

#[test]
fn t_sel_03_多就绪取第一个() {
    let t = go_probe::probe("sel03", FIRST_READY, &[("RGOC_GC", "0")]).expect("应能编译并跑起来");
    t.assert_l1("sel03");
    assert_eq!(
        t.native.stdout, "1\n",
        "多就绪时必须取**源码顺序第一个**（`C11` §4）：实际 {:?}",
        t.native.stdout
    );
}

/// `T-SEL-04`：**阻塞后被唤醒 ⇒ 撤销其余登记**（`C11` §4 的撤销）。
///
/// # 为什么这条必须两个通道 + 两个 goroutine
///
/// 单 case 的 `select` 与普通收发没区别，测不到撤销。这里：
///
/// 1. main 进入 `select`，两个 case 都没就绪 ⇒ 在 `a` 和 `b` 上各登记一个
///    sudog，然后阻塞；
/// 2. 第一个 goroutine 往 `b` 发 ⇒ main 被 `b` 唤醒，选中第 2 个 case；
/// 3. main 必须把 `a` 上那个登记**撤掉** —— 否则第二个 goroutine 往 `a` 发时
///    会「唤醒一个已经不在 select 里的 G」，那个值就丢了（`a` 的发送方
///    会以为交付成功）。
///
/// 判据查 `a` 上的第二次发送是否**真的交付给了 main 的后续接收**。
const UNDO: &str = r#"
package main

func main() {
	a := make(chan int)
	b := make(chan int)
	ready := make(chan int)
	go func() {
		ready <- 1
		b <- 2
	}()
	<-ready
	select {
	case v := <-a:
		println("a", v)
	case v := <-b:
		println("b", v)
	}
	// 此刻 `a` 上那次登记必须已经撤销。主 G 再收一次 `a`：
	// 如果撤销漏了，发送方会「交付」给一个不存在的等待者。
	go func() { a <- 7 }()
	println("again", <-a)
}
"#;

#[test]
fn t_sel_04_撤销其余登记() {
    let t = go_probe::probe("sel04", UNDO, &[("RGOC_GC", "0")]).expect("应能编译并跑起来");
    t.assert_l1("sel04");
    assert_eq!(
        t.native.stdout, "b 2\nagain 7\n",
        "被 `b` 唤醒后必须撤销 `a` 上的登记：实际 {:?}",
        t.native.stdout
    );
}

/// `T-SEL-05`：**空 `select {}` ⇒ 直接死锁**（`C11` §5.2），不是超时。
///
/// # 为什么这条能区分「死锁」与「挂住」
///
/// 空 `select` 没有任何 case 可登记 ⇒ `rt_select_wait` 的登记循环一次都不跑
/// ⇒ `rt_block_forever()` ⇒ 就绪队列空 ⇒ `rt_fatal_deadlock`。
///
/// 如果实现里「没有登记就 `continue` 重试」，这条会**死循环**（CPU 100%
/// 但永不退出）—— 判据靠 `timeout` 才抓得到，所以这里断言的是**退出码 2 +
/// 确切文本**，比「没超时」强得多。
const EMPTY_SELECT: &str = r#"
package main

func main() {
	select {}
}
"#;

#[test]
fn t_sel_05_空_select_直接死锁() {
    let out =
        support::native_harness::build_and_run_env("sel05", EMPTY_SELECT, &[("RGOC_GC", "0")])
            .expect("应能编译并跑起来");
    assert_eq!(out.exit, 2, "空 select ⇒ 死锁退出 2：{:?}", out.stderr);
    assert_eq!(
        out.stderr, "fatal error: all goroutines are asleep - deadlock!\n",
        "文本必须逐字节相同（`C9` §5）：实际 {:?}",
        out.stderr
    );
    assert!(out.stdout.is_empty(), "stdout={:?}", out.stdout);
}
