//! `GO-6` 多 G 根扫描（M11c，Task 33–36）—— 判据。
//!
//! M10 的 `g_frames` 是**单链**，多 G 时只描述当前那个。M11c 把它变成
//! 「当前 G 的链头」+ 扫**所有** gdesc（`D-M11-10/11`）。
//!
//! # 为什么这条最难糊弄
//!
//! 挂起 G 的栈上有活值（它正等着被唤醒，值必须活到那时）。漏扫不会立刻崩 ——
//! 要等 GC 把对象收掉、再被别的分配覆盖、醒来读到垃圾。所以判据必须
//! **先阻塞、再 churn、再唤醒、最后读值**，四步缺一不可。
//!
//! # 为什么每条都要 `RGOC_GC_STRESS`
//!
//! 自动触发看的是「自上次 GC 以来的分配量 ≥ max(阈值, 存活×2)」——语料规模的
//! 程序可能一次都不触发，于是「GC 没跑」和「GC 跑对了」在输出上完全一样。
//! `STRESS=64` 把「每 64 次分配收一次」钉死，判据才真的在验根扫描。
//!
//! # 一个实测到的**真 bug**（Task 33 才暴露）
//!
//! `go f(p)` 的实参被拷进**新 G 的栈顶**，而那块区域不在任何帧记录里 ⇒
//! 闭包块（`fv` 在 arg0）与按值传的指针都没有根。症状不是「读到垃圾」，而是
//! trampoline 调 `[fv+8]` 时撞上 **nil 检查 panic**（块被回收并清零）。
//! `t_groot_02` 钉的就是这条；修法见 `rt.c` 的 `rt_go` / `mark_all_g_stacks`。

mod support;

use support::go_probe;
use support::native_harness::{Outcome, build_and_run_env};

/// 从统计行里取一个字段（`C12` §5.2 的键序冻结，但这里按名取 —— 加字段不破）。
fn gc_field(out: &Outcome, key: &str) -> String {
    let line = out
        .stderr
        .lines()
        .find(|l| l.starts_with("gc: "))
        .unwrap_or_else(|| panic!("没有统计行（stderr 为空？）：{:?}", out.stderr));
    let pat = format!("{key}=");
    line["gc: ".len()..]
        .split(' ')
        .find_map(|kv| kv.strip_prefix(&pat).map(str::to_string))
        .unwrap_or_else(|| panic!("统计行里没有 `{key}`：{line}"))
}

/// 跑一次并断言「真的收过垃圾」——没有这句，`bad_ptr == 0` 可能是「没跑」。
fn assert_collected(out: &Outcome, what: &str) {
    let n: u64 = gc_field(out, "collections")
        .parse()
        .expect("collections 是数字");
    assert!(
        n > 0,
        "[{what}] 一次 GC 都没跑 ⇒ 这条判据什么也没验（stress 没生效？）"
    );
}

// ════════════════════════════════════════════════════════════════════
// T-GROOT-01 挂起 G 的栈是根
// ════════════════════════════════════════════════════════════════════

const SUSPENDED: &str = r#"
package main

func churn() {
	for i := 0; i < 20000; i++ {
		_ = new([64]byte)
	}
}

func main() {
	c := make(chan int)
	go func() {
		p := new([1024]byte)
		p[0] = 42
		<-c
		println(p[0])
	}()
	churn()
	c <- 1
}
"#;

#[test]
fn t_groot_01_挂起_g_的栈是根() {
    let t = go_probe::probe("groot01", SUSPENDED, &[("RGOC_GC_STRESS", "64")])
        .expect("应能编译并跑起来（M11c 之后）");
    t.assert_l1("groot01");
    assert_eq!(
        t.native.stdout, "42\n",
        "挂起 G 的栈上对象被回收了（`mark_frames` 没走 blocked G）：实际 {:?}",
        t.native.stdout
    );
}

// ════════════════════════════════════════════════════════════════════
// T-GROOT-02 就绪 G 的实参是根
// ════════════════════════════════════════════════════════════════════

/// `p` 在 `go` 之后被置 nil ⇒ 实参区是它**唯一**的根。
///
/// 这是 Task 33 实测到的那个真 bug 的判据：实参区不在帧记录里，漏标就是
/// 「闭包块被回收 ⇒ trampoline 调 `[fv+8]` ⇒ nil 检查 panic」。
const READY_ARGS: &str = r#"
package main

func churn() {
	for i := 0; i < 20000; i++ {
		_ = new([64]byte)
	}
}

func worker(p *[1024]byte, done chan int) {
	println(p[0])
	done <- 1
}

func main() {
	done := make(chan int)
	p := new([1024]byte)
	p[0] = 7
	go worker(p, done)
	p = nil
	churn()
	<-done
	println("ok")
}
"#;

#[test]
fn t_groot_02_就绪_g_的实参是根() {
    let t = go_probe::probe("groot02", READY_ARGS, &[("RGOC_GC_STRESS", "64")])
        .expect("应能编译并跑起来");
    t.assert_l1("groot02");
    assert_eq!(
        t.native.stdout, "7\nok\n",
        "`go` 的实参区没有根（新 G 还没跑，`p` 已经被 `p = nil` 丢掉）：实际 {:?}",
        t.native.stdout
    );
}

// ════════════════════════════════════════════════════════════════════
// T-GROOT-03 `select` 阻塞期间 GC 不坏（`gdesc->wait` 那条路）
// ════════════════════════════════════════════════════════════════════

/// ⚠️ **这条判据的射程要说清楚**：Go 源码里 `select` 的通道一定也在栈槽里
/// （`rt_select_add` 的实参就是从槽里读的），所以「`WaitSet` 单独保活」这件事
/// **在源码层面观察不到** —— 真正被验的是「GC 在 `select` 阻塞期间跑」这条路
/// 本身：`gdesc->wait` 的标记、`WaitSet` 的 `K_RAW` 内容、醒来后的撤销都不崩、
/// `bad_ptr == 0`、输出不变。
///
/// 先让 G 真的阻塞在 `select` 上（`ready` 握手），再 churn —— 顺序反了就只是
/// 「G 还没跑」的另一种情形。
const SELECT_BLOCKED: &str = r#"
package main

func churn() {
	for i := 0; i < 20000; i++ {
		_ = new([64]byte)
	}
}

func waiter(a chan int, b chan int, ready chan int, done chan int) {
	ready <- 1
	select {
	case v := <-a:
		println(v)
	case v := <-b:
		println(v)
	}
	done <- 1
}

func main() {
	a := make(chan int)
	b := make(chan int)
	ready := make(chan int)
	done := make(chan int)
	go waiter(a, b, ready, done)
	<-ready
	churn()
	b <- 5
	<-done
	println("ok")
}
"#;

#[test]
fn t_groot_03_select_阻塞期间_gc_不坏() {
    let out = build_and_run_env(
        "groot03",
        SELECT_BLOCKED,
        &[("RGOC_GC_STRESS", "64"), ("RGOC_GC_STATS", "1")],
    )
    .expect("应能编译并跑起来");
    assert_eq!(
        out.exit, 0,
        "退出码：{}（stderr: {}）",
        out.exit, out.stderr
    );
    assert_eq!(
        out.stdout, "5\nok\n",
        "`select` 阻塞期间被 GC 搞坏了：实际 {:?}",
        out.stdout
    );
    assert_collected(&out, "groot03");
    assert_eq!(gc_field(&out, "bad_ptr"), "0", "扫到了非块首的指针");
    assert_eq!(gc_field(&out, "bad_desc"), "0", "扫到了非法块头");
}

// ════════════════════════════════════════════════════════════════════
// T-GROOT-05 翻开关：并发语料在 stress 下收垃圾且输出不变
// ════════════════════════════════════════════════════════════════════

/// `T-GROOT-04`（a/b 阶段强制 `mode=off`）在 c 阶段**退役**：`rt_gc_force_off`
/// 从来没有接线（a/b 靠装置侧的 `RGOC_GC=0` 对照，见 `M11-plan.md` Task 34）。
/// 所以这里只留「翻过来」这一半 —— 而且它比 04 更强：不只是「开关生效」，
/// 还要「收过垃圾 + 输出与 `off` 态逐字节相同」。
const FLIP: &str = r#"
package main

func churn() {
	for i := 0; i < 4000; i++ {
		_ = new([64]byte)
	}
}

func main() {
	c := make(chan int)
	go func() {
		p := new([256]byte)
		p[0] = 9
		<-c
		println(p[0])
	}()
	churn()
	c <- 1
	println("done")
}
"#;

#[test]
fn t_groot_05_翻开关后收过垃圾且输出不变() {
    let off = build_and_run_env("groot05_off", FLIP, &[("RGOC_GC", "0")]).expect("off 态应能跑");
    let stress = build_and_run_env(
        "groot05_stress",
        FLIP,
        &[
            ("RGOC_GC_STRESS", "8"),
            ("RGOC_GC_STATS", "1"),
            ("RGOC_GC_THRESHOLD", "0"),
        ],
    )
    .expect("stress 态应能跑");

    assert_eq!(off.exit, 0, "off 态退出码 {}", off.exit);
    assert_eq!(stress.exit, 0, "stress 态退出码 {}", stress.exit);
    assert_eq!(gc_field(&stress, "mode"), "stress", "开关没翻过来");
    assert_collected(&stress, "groot05");
    assert_eq!(
        stress.stdout, off.stdout,
        "stress 态的输出与 off 态不同 ⇒ 要么漏标、要么误收：\n-- off --\n{}\n-- stress --\n{}",
        off.stdout, stress.stdout
    );
    assert_eq!(gc_field(&stress, "bad_ptr"), "0", "扫到了非块首的指针");
}

// ════════════════════════════════════════════════════════════════════
// T-GROOT-06 多 G 下的超集扫描不坏（`G-M10-06` 的回归面）
// ════════════════════════════════════════════════════════════════════

/// 三个 G 轮流阻塞/唤醒，每个都持有栈上指针，每个都 churn。
///
/// 判据不是「打印了什么」（顺序由 M:1 FIFO 决定，`C11` 已冻结，但这里不靠它
/// 当证据），而是：**stress 态与 off 态逐字节相同** + `bad_ptr == 0`。这正是
/// 「位图是 live-out 的保守超集」（`R-10-21`）在多 G 下的落点 —— 多扫几个
/// G 只会多标，不会标错；一旦标错就会在输出上露。
const MULTI_G: &str = r#"
package main

func churn() {
	for i := 0; i < 2000; i++ {
		_ = new([64]byte)
	}
}

func stage(p *[256]byte, in chan int, out chan int) {
	<-in
	println(p[0])
	churn()
	out <- 1
}

func main() {
	c1 := make(chan int)
	c2 := make(chan int)
	c3 := make(chan int)
	c4 := make(chan int)
	a := new([256]byte)
	a[0] = 1
	b := new([256]byte)
	b[0] = 2
	c := new([256]byte)
	c[0] = 3
	go stage(a, c1, c2)
	go stage(b, c2, c3)
	go stage(c, c3, c4)
	c1 <- 1
	<-c4
	println("end")
}
"#;

#[test]
fn t_groot_06_多_g_超集扫描不坏() {
    let off = build_and_run_env("groot06_off", MULTI_G, &[("RGOC_GC", "0")]).expect("off 态应能跑");
    let stress = build_and_run_env(
        "groot06_stress",
        MULTI_G,
        &[("RGOC_GC_STRESS", "64"), ("RGOC_GC_STATS", "1")],
    )
    .expect("stress 态应能跑");
    let interp = support::native_harness::hir_output("groot06", MULTI_G).expect("解释器应能跑");

    assert_eq!(off.exit, 0, "off 态退出码 {}", off.exit);
    assert_eq!(stress.exit, 0, "stress 态退出码 {}", stress.exit);
    assert_eq!(
        stress.stdout, off.stdout,
        "多 G 超集扫描改变了输出：\n-- off --\n{}\n-- stress --\n{}",
        off.stdout, stress.stdout
    );
    assert_eq!(
        stress.stdout, interp,
        "与解释器不一致（L1）：\n-- native --\n{}\n-- interp --\n{}",
        stress.stdout, interp
    );
    assert_collected(&stress, "groot06");
    assert_eq!(gc_field(&stress, "bad_ptr"), "0", "扫到了非块首的指针");
    assert_eq!(gc_field(&stress, "bad_sp"), "0", "帧记录的 sp_idx 越界");
}

// ════════════════════════════════════════════════════════════════════
// T-GROOT-07 goroutine 栈不泄漏（Task 36）
// ════════════════════════════════════════════════════════════════════

/// 10 万个 G **依次**创建并退出（不是同时）——`<-done` 让每个 G 跑完再起下一个。
///
/// # 为什么这条必须有
///
/// `G_MAX = 1024` 是「同时存活」的上限，不是「一生总数」的上限。如果
/// `rt_gdesc_new` 只增不减，第 1025 个 G 就会撞 `too many goroutines` ——
/// 而那个错误对「用 goroutine 处理 10 万条任务」这种**正常**写法是致命的。
///
/// 同理，每个 G 的 64 KiB 栈必须在它退出后 `munmap`：10 万个 × 64 KiB =
/// 6.4 GiB 虚拟地址，RSS 也会被踩过的页撑起来。
const MANY_G: &str = r#"
package main

func work(n int, done chan int) {
	if n%20000 == 0 {
		println(n)
	}
	done <- 1
}

func main() {
	done := make(chan int)
	for i := 0; i < 100000; i++ {
		go work(i, done)
		<-done
	}
	println("ok")
}
"#;

#[test]
fn t_groot_07_goroutine_栈不泄漏() {
    let out = build_and_run_env(
        "groot07",
        MANY_G,
        &[("RGOC_GC_STATS", "1"), ("RGOC_GC_STRESS", "1000")],
    )
    .expect("应能编译并跑起来");
    assert_eq!(out.exit, 0, "退出码 {}（stderr: {}）", out.exit, out.stderr);
    let heap: u64 = gc_field(&out, "heap").parse().expect("heap 是数字");
    let rss: u64 = gc_field(&out, "rss").parse().expect("rss 是数字");
    assert!(
        heap <= 64 * 1024 * 1024,
        "10 万个 G 之后堆涨到 {heap} 字节 ⇒ 栈或 gdesc 没回收"
    );
    assert!(
        rss <= 64 * 1024,
        "10 万个 G 之后 RSS 是 {rss} KB ⇒ 栈的物理页没还回去"
    );
}
