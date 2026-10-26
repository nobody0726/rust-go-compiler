//! 托管堆的形状判据（M10/Task 6–8、14；`C12` §1 / §2）。
//!
//! # 为什么「块头 16 字节」要用**两个对象之间的距离**来钉
//!
//! 直接读块头需要内建（会动 `C8`/`C9` 两套契约），而距离是**纯 Go 可观测**的：
//! `new(int)` 的 data 是 8 字节，块 = `align16(16 + 8) = 32` ⇒ 两个对象至少差
//! **32**。块头还是 8 字节时差 16 —— 一条断言就把两代块头分开。
//!
//! # 为什么还要 `% 16 == 0`
//!
//! 「差 32」不能排除「整体偏移 8」（比如 base+8 起）。而 `data` 的 16 对齐是
//! `uintptr` 往返不乱的前提（`C12` §1.3）。两条合起来才是「块头 16 字节 +
//! 块 16 对齐」。
//!
//! # 为什么 `chunks` 与 `heap` 要自洽
//!
//! `heap` 是 `Σ(chunk 的 data 容量 + 64 头 + 288 KiB 元数据)`，而 `chunks` 是
//! chunk 数。若 chunk 没按 16 MiB 取整（比如按 `need` 精确 mmap），两个数就对
//! 不上 —— 而「按 need 精确 mmap」会让 `p >> 24` 的判定失效（一个 chunk 可能
//! 只占 4 KiB，下一个 chunk 与它同段）。

mod support;

use support::gc_probe::{parse_stats, run_with_env};

const CHUNK: u64 = 16 * 1024 * 1024;

/// 每个 chunk 的**额外开销**上界：64 字节头 + 256 KiB 起始位图 + 512 B 页摘要
/// + 对齐余量。用它给 `heap` 一个上界，见 [`assert_chunk_accounting`]。
const OVERHEAD_MAX: u64 = 512 * 1024;

/// `chunks` 与 `heap` 自洽（`C12` §1.4 不变量 5）。
///
/// # 为什么不是 `chunks == ceil(heap / 16 MiB)`
///
/// 那个式子**不成立**：`heap` 含每个 chunk 的 64 字节头 + 288 KiB 元数据，
/// 所以 `heap` 恰好比 `chunks × 16 MiB` 大一点，`ceil` 会多算一个 chunk
/// （实测 `chunks=1 heap=16777240 ceil=2`）。写成上下界才对：
///
/// * 下界 `heap >= chunks × 16 MiB`：每个 chunk 的 data 容量至少 16 MiB。
///   「按 need 精确 mmap」会让这条红（chunk 只占 4 KiB 时 `heap < 16 MiB`）。
/// * 上界 `heap <= chunks × (16 MiB + 512 KiB)`：开销有界，不会偷偷多 mmap。
fn assert_chunk_accounting(s: &support::gc_probe::Stats) {
    assert!(s.chunks >= 1, "至少一个 chunk，实际 {}", s.chunks);
    let lo = s.chunks * CHUNK;
    let hi = s.chunks * (CHUNK + OVERHEAD_MAX);
    assert!(
        s.heap >= lo && s.heap <= hi,
        "chunks 与 heap 不自洽（chunk 没按 16 MiB 取整？）：chunks={} heap={} 允许区间 [{lo}, {hi}]",
        s.chunks,
        s.heap
    );
}

#[test]
fn t_hdr_02_块头十六字节与十六对齐() {
    // 两个 `new(int)`：块 = align16(16 + 8) = 32 ⇒ 距离 >= 32，且都 16 对齐。
    let src = r#"package main

import "unsafe"

func main() {
	p := new(int)
	q := new(int)
	println(uintptr(unsafe.Pointer(q))-uintptr(unsafe.Pointer(p)) >= 32)
	println(uintptr(unsafe.Pointer(p))%16, uintptr(unsafe.Pointer(q))%16)
}
"#;
    let (out, _) = run_with_env("gc_heap_hdr", src, &[]).expect("编译 + 运行不该失败");
    assert_eq!(
        out.stdout, "true\n0 0\n",
        "块头必须是 16 字节且 data 16 对齐（C12 §2.1）；实际输出：{}",
        out.stdout
    );
}

#[test]
fn t_heap_03_chunk_按十六_mib_取整() {
    // 分配一个 1 MiB 的数组 ⇒ 至少要一个 chunk。
    let src = r#"package main

func main() {
	s := make([]byte, 1<<20)
	s[0] = 1
	s[1<<20-1] = 2
	println(s[0] + s[1<<20-1])
}
"#;
    let (out, s) = run_with_env("gc_heap_chunk", src, &[]).expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "3\n");
    assert_chunk_accounting(&s);
}

#[test]
fn t_heap_03b_多_chunk_也自洽() {
    // 分配超过一个 chunk 的量（18 MiB）⇒ 必须出现第 2 个 chunk。
    let src = r#"package main

func main() {
	a := make([]byte, 9<<20)
	b := make([]byte, 9<<20)
	a[0] = 1
	b[9<<20-1] = 2
	println(a[0] + b[9<<20-1])
}
"#;
    let (out, s) = run_with_env("gc_heap_chunk2", src, &[]).expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "3\n");
    assert!(
        s.chunks >= 2,
        "18 MiB 装不进一个 16 MiB chunk，实际 {} 个",
        s.chunks
    );
    assert_chunk_accounting(&s);
}

#[test]
fn t_heap_03c_空程序不占堆() {
    // 惰性 mmap 的回归（`rt.c` 的 `rt_grow` 只在第一次分配时跑）。
    let src = "package main\n\nfunc main() { println(\"hi\") }\n";
    let (out, s) = run_with_env("gc_heap_empty", src, &[]).expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "hi\n");
    assert_eq!(s.chunks, 0);
    assert_eq!(s.heap, 0);
}

#[test]
fn t_heap_04_统计行可解析且自检干净() {
    // 兜底：任何一条堆判据跑完之后 `bad_*` 都必须是 0。
    let src = "package main\n\nfunc main() {\n\tp := new(int)\n\t*p = 7\n\tprintln(*p)\n}\n";
    let (out, _s) = run_with_env("gc_heap_clean", src, &[]).expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "7\n");
    let s = parse_stats(&out.stderr)
        .expect("统计行格式")
        .expect("必须有统计行");
    s.assert_clean().expect("自检计数器");
}

/// 四种 kind 都必须出现（`T-HDR-03`，`C12` §2.2）。
///
/// # 为什么用 `kinds` 计数而不是「跑得对」
///
/// 「跑得对」只证明**分配发生了**，不证明「走的是带 kind 的入口」。如果
/// `make([]int,3)` 仍然走 `rt_arena_alloc`（无块头），程序照样跑对，
/// 而 GC 扫不到它 —— 那正是 Phase 2 会踩的雷。
///
/// # 为什么现在只断言四种
///
/// `mapslots`（kind 4）的切换在 Task 25（map 托管化）。这里断言
/// `o/a/c/r` 各 >= 1、`m == 0` —— **`m == 0` 是有信息的**：它钉住「现在还没
/// 切 map」，Task 25 会把这一位翻成 >= 1。
#[test]
fn t_hdr_03_四种_kind_都出现() {
    // ⚠️ `g := a + b` 里的两个字面量会被**编译期折叠**成 "abcd"（不调
    // `rt_strcat`），所以拼接必须放进函数 —— 那里的操作数是形参，折不了。
    let src = r#"package main

func cat(x, y string) string { return x + y }

func main() {
	p := new(int)
	*p = 1
	s := make([]int, 3)
	s[0] = 2
	f := func() int { return *p }
	g := cat("ab", "cd")
	println(*p + s[0] + f() + len(g))
}
"#;
    let (out, s) = run_with_env("gc_heap_kinds", src, &[]).expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "8\n", "程序本身要跑对");
    let [o, a, c, r, m] = s.kinds;
    assert!(o >= 1, "object（new）没出现：{:?}", s.kinds);
    assert!(a >= 1, "array（make）没出现：{:?}", s.kinds);
    assert!(c >= 1, "closure 没出现：{:?}", s.kinds);
    assert!(r >= 1, "raw（字符串拼接）没出现：{:?}", s.kinds);
    assert_eq!(m, 0, "mapslots 还没切（Task 25 才切）：{:?}", s.kinds);
    s.assert_clean().expect("自检计数器");
}
