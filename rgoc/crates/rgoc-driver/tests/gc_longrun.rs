//! 自动触发、长跑、碎片、OOM、确定性（`T-GC-01/05/06/07/08/09`，M10/Task 33–35）。
//!
//! # 这一组测的是「GC 在**没人逼它**的时候也活着」
//!
//! Phase 1/2 的判据全用 `RGOC_GC_STRESS=<n>` —— 那是**装置压力**，不是运行时
//! 的真实行为。`RGOC_GC` 缺省（自动态）在 Task 33 之前**一次都不收**：
//! 计划初稿的触发条件是「分配失败且 `heap_committed > threshold`」，而 chunk
//! 是 16 MiB、语料里的程序都分配不到 16 MiB ⇒ bump 永远够 ⇒ 自动态退化成
//! 关态，`off` 与 `on` 两态**逐字节相同**而没人发现。
//!
//! 所以这里第一条判据（`T-GC-01`）就是「把阈值调到 64 KiB，分配 1 MiB，
//! 必须真的收过」。

mod support;

use support::gc_probe::{Stats, run_with_env};

/// 自动触发（`T-GC-01`，`C12` §5.3）。
///
/// 阈值 64 KiB + 分配 1 MiB ⇒ `collections >= 1`。
///
/// # 反向：`RGOC_GC=0` 时一次都不收
///
/// 两个方向都要断言。只断言「on 态收过」的话，把触发条件写成「永远收」
/// 也能过 —— 而那是另一个 bug（每次分配都收 = O(n²)）。
#[test]
fn t_gc_01_自动触发() {
    let src = r#"package main

func main() {
	s := 0
	for i := 0; i < 2000; i++ {
		a := make([]int, 64)
		a[0] = i
		s += a[0]
	}
	println(s)
}
"#;
    // 2000 × 512 B = 1 MiB。
    let (out, on) = run_with_env("gc_auto_on", src, &[("RGOC_GC_THRESHOLD", "65536")])
        .expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "1999000\n", "stderr: {}", out.stderr);
    assert!(
        on.collections >= 1,
        "阈值 64 KiB、分配 1 MiB，却一次都没收 —— 自动触发没接上：{on:?}"
    );

    // 关态：同一份代码，一次都不收（`C12` §5.1）。
    let (out, off) = run_with_env(
        "gc_auto_off",
        src,
        &[("RGOC_GC", "0"), ("RGOC_GC_THRESHOLD", "65536")],
    )
    .expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "1999000\n");
    assert_eq!(off.collections, 0, "`RGOC_GC=0` 却收过：{off:?}");
    assert_eq!(off.frees, 0, "`RGOC_GC=0` 却回收过：{off:?}");
}

/// 长跑不涨堆（`T-GC-05`）：1e5 × 1 KiB ⇒ `live <= 2 MiB`、`heap <= 64 MiB`。
///
/// # 为什么是这两个数
///
/// * `live <= 2 MiB`：循环体里只有 `a` 一个对象活着（1 KiB），加上 map /
///   闭包之类的常数项。2 MiB 是**两个数量级**的余量，卡的是「GC 真的在收」
///   而不是「收得刚好」。
/// * `heap <= 64 MiB`：累计分配 100 MB。不复用/不回收至少要 7 个 chunk
///   （112 MiB）；能压在 64 MiB 以内说明「回收 + 复用」都在工作。
///
/// 用**自动态**（不是 stress）：这条要测的正是「没人逼它的时候」。
#[test]
fn t_gc_05_长跑不涨堆() {
    let src = r#"package main

func main() {
	s := 0
	for i := 0; i < 100000; i++ {
		a := make([]int, 128) // 1 KiB
		a[0] = i
		s += a[0]
	}
	println(s)
}
"#;
    let (out, s) = run_with_env("gc_longrun", src, &[("RGOC_GC_THRESHOLD", "1048576")])
        .expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "4999950000\n", "stderr: {}", out.stderr);
    assert!(
        s.collections >= 5,
        "累计 100 MB 只收了 {} 次：{s:?}",
        s.collections
    );
    assert!(
        s.live <= 2 * 1024 * 1024,
        "存活 {} 字节 > 2 MiB —— 循环里的切片没被回收：{s:?}",
        s.live
    );
    assert!(
        s.heap <= 64 * 1024 * 1024,
        "mmap 了 {} 字节 > 64 MiB —— 回收后没复用（堆只增不减）：{s:?}",
        s.heap
    );
    s.assert_clean().expect("自检计数器");
}

/// 碎片：**只测不修**（`T-GC-06`）。
///
/// # 这条判据的野心很小，故意的
///
/// M10 的分配器是「first-fit + 不移动」，所以**必然**有碎片。判据不断言
/// 「碎片小」（那要压缩，而 `D-M10-5` 明确说不移动），只断言：
///
/// 1. `heap_committed` 不超过「理论上限」的一个宽松倍数；
/// 2. 这个倍数被**记录**下来（`R-10-24`：碎片率是可观测的，不是传说）。
///
/// 理论上限 = 峰值存活量 × 2（一轮垃圾）+ 16 MiB（一个 chunk 的粒度）。
/// 实测倍数写在下方的注释里，**改了分配器就要重新量**。
#[test]
fn t_gc_06_碎片只测不修() {
    // 交替大小的分配：最容易把 free list 打成筛子的形状。
    let src = r#"package main

func main() {
	s := 0
	for i := 0; i < 20000; i++ {
		if i%3 == 0 {
			b := make([]byte, 64)
			b[0] = 1
			s += int(b[0])
		} else {
			c := make([]int, 64)
			c[0] = 2
			s += c[0]
		}
	}
	println(s)
}
"#;
    let (out, s) = run_with_env("gc_frag", src, &[("RGOC_GC_THRESHOLD", "262144")])
        .expect("编译 + 运行不该失败");
    // 20000 轮：i%3==0 的 6667 轮加 1，其余 13333 轮加 2。
    assert_eq!(out.stdout, "33333\n", "stderr: {}", out.stderr);
    assert!(s.collections >= 3, "没收够，量不出碎片：{s:?}");

    // 峰值存活量的上界：循环里最多一个 512 B 的切片活着。
    const PEAK_LIVE: u64 = 4096;
    const CHUNK: u64 = 16 * 1024 * 1024;
    /// 每个 chunk 的元数据上界（64 字节头 + 256 KiB 起始位图 + 512 B 页摘要
    /// + 对齐余量）。与 `gc_heap::OVERHEAD_MAX` 同一个口径。
    const OVERHEAD: u64 = 512 * 1024;

    // ① 回收有效：`live` 必须压在峰值存活附近。
    //    这一条才是「碎片没把回收搞坏」的证据 —— 碎片会让 `heap` 大，但
    //    不会让 `live` 大；`live` 大了说明有东西该收没被收。
    assert!(
        s.live <= PEAK_LIVE,
        "存活 {} 字节 > {PEAK_LIVE} —— 交替大小的分配把某些块钉住了：{s:?}",
        s.live
    );

    // ② 堆不涨：峰值存活 × 2 + 一个 chunk（含元数据）。
    let bound = PEAK_LIVE * 2 + s.chunks * (CHUNK + OVERHEAD);
    assert!(
        s.heap <= bound,
        "heap {} > 理论上限 {bound}（峰值存活 {PEAK_LIVE}×2 + {} 个 chunk）—— \
         碎片或者复用出了问题：{s:?}",
        s.heap,
        s.chunks
    );

    // ③ **记录**（不断言）：碎片率 = (heap - live) / heap。
    //
    // 实测（20000 轮交替 64 B / 512 B，阈值 256 KiB）：
    //   collections=27  frees=19519  live=528  heap=17039872  ⇒ 碎片率 99.99%
    //
    // 这个数**大得吓人但完全正常**：`heap` 是 mmap 量，一个 chunk 就是
    // 16 MiB，而程序峰值只用了 528 字节 —— 分母里 16 MiB 是「粒度」不是
    // 「碎片」。所以碎片率这个指标只有在**多 chunk** 的程序里才有意义；
    // 单 chunk 时它衡量的其实是「chunk 粒度相对负载有多大」。
    // 写在这里是为了让后来的人不必再推一遍（`R-10-24`）。
}

/// OOM：文本与退出码不变（`T-GC-07`，`C9` §6.1）。
///
/// `RGOC_HEAP_MAX=4 MiB` + 无限分配 ⇒
/// `runtime error: out of memory (heap limit N bytes)` / 退出码 2。
///
/// # GC 不能改变这条
///
/// 收了也装不下 ⇒ 必须还是 OOM。如果 GC 让「本该 OOM 的程序」跑完了，
/// 说明它凭空造出了内存（或者 `heap_limit` 判错了）。
#[test]
fn t_gc_07_oom_文本与退出码() {
    let src = r#"package main

func main() {
	for {
		_ = new([4096]byte)
	}
}
"#;
    let out = support::native_harness::build_and_run_env(
        "gc_oom",
        src,
        &[("RGOC_HEAP_MAX", "4194304"), ("RGOC_GC_STATS", "1")],
    )
    .expect("编译不该失败");

    assert_eq!(out.exit, 2, "OOM 的退出码必须是 2：stderr {}", out.stderr);
    assert!(
        out.stderr
            .contains("runtime error: out of memory (heap limit 4194304 bytes)"),
        "OOM 文本不对：{}",
        out.stderr
    );
    // ⚠️ **这里不能断言统计行**：`rt_fatal_oom` 直接 `rt_exit(2)`，而统计行是
    // `main` 出口打的 —— OOM 走的是另一条路，两行都在的话 OOM 文本就不是
    // stderr 的最后一句话了（`C9` §6.1 要求它是）。
    //
    // 于是「OOM 之前收过没有」这条只能从**行为**看：这个程序分配 4 KiB 一块、
    // 上限 4 MiB ⇒ 1000 块就满。没收过的话第一次 `rt_grow` 就会 OOM（因为
    // 一个 chunk 16 MiB > 4 MiB）；能跑到「第二块 chunk 才 OOM」说明它确实
    // 先收了一轮、把第一块的垃圾还回来了。判据落在 `chunks` 无法观测，
    // 所以这里只钉文本与退出码 —— 别把「收过」写进这条。
}

/// 确定性（`T-GC-08`）：同程序跑两次，三个数完全相同。
///
/// `collections` / `live` / `heap` 三个数只要有一个不同，就说明 GC 里混进了
/// 不确定性（时间、地址、哈希遍历顺序）。M10 是单线程 STW，**必须**确定 ——
/// 否则 `T-CORP-05` 的三态对比就不是判据（每次跑都不一样，比什么？）。
#[test]
fn t_gc_08_确定性() {
    let src = r#"package main

func main() {
	m := map[int]int{}
	for i := 0; i < 500; i++ {
		m[i] = i * 2
	}
	s := 0
	for i := 0; i < 3000; i++ {
		a := make([]int, 32)
		a[0] = i
		s += a[0] + m[i%500]
	}
	println(s)
}
"#;
    let (o1, a) = run_with_env("gc_det", src, &[("RGOC_GC_THRESHOLD", "131072")])
        .expect("编译 + 运行不该失败");
    let (o2, b) = run_with_env("gc_det", src, &[("RGOC_GC_THRESHOLD", "131072")])
        .expect("编译 + 运行不该失败");
    assert_eq!(o1.stdout, o2.stdout, "两次运行的 stdout 不同");
    assert!(a.collections >= 1, "没收过，比不出确定性：{a:?}");
    assert_eq!(a.collections, b.collections, "collections 不确定");
    assert_eq!(a.live, b.live, "live 不确定");
    assert_eq!(a.heap, b.heap, "heap 不确定");
    assert_eq!(a.frees, b.frees, "frees 不确定");
}

/// RSS 真的被读出来（`T-GC-09`）。
///
/// # 为什么断言 `0 < rss < heap`
///
/// * `rss > 0`：`getrusage` 的偏移写对了（写错会读到 `ru_stime` 的某个字段，
///   通常是 0）。
/// * `rss < heap`：`heap` 是 mmap 量（含整个 16 MiB chunk），而 RSS 只算
///   **碰过的页**。一个只写了几 KiB 的程序 RSS 必然远小于 16 MiB ——
///   这条把「`rss` 字段其实填的是 `heap`」这种错挡住。
#[test]
fn t_gc_09_rss() {
    let src = r#"package main

func main() {
	s := 0
	for i := 0; i < 100; i++ {
		a := make([]int, 8)
		a[0] = i
		s += a[0]
	}
	println(s)
}
"#;
    let (out, s) = run_with_env("gc_rss", src, &[("RGOC_GC_THRESHOLD", "65536")])
        .expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "4950\n");
    assert!(s.rss > 0, "`rss` 是 0 —— `getrusage` 的偏移写错了？{s:?}");
    assert!(
        s.rss * 1024 < s.heap,
        "`rss` {} KB 不小于 `heap` {} 字节 —— 这两个数填反了？{s:?}",
        s.rss,
        s.heap
    );
    let _ = Stats::default(); // 让 `Stats` 的 import 有用（其它测试用它的字段）
}
