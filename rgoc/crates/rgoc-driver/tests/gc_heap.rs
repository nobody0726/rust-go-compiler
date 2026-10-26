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

/// 五种 kind 都必须出现（`T-HDR-03`，`C12` §2.2）。
///
/// # 为什么用 `kinds` 计数而不是「跑得对」
///
/// 「跑得对」只证明**分配发生了**，不证明「走的是带 kind 的入口」。如果
/// `make([]int,3)` 仍然走 `rt_arena_alloc`（无块头），程序照样跑对，
/// 而 GC 扫不到它 —— 那正是 Phase 2 会踩的雷。
///
/// # 五种都要出现（Task 25 起 `mapslots` 也进托管堆）
///
/// 早先这里断言 `m == 0`（map 槽块还在 arena）。Task 25 把槽块托管化之后翻成
/// `m >= 1` —— 这一位就是「map 的槽有没有被 GC 看见」的开关。
#[test]
fn t_hdr_03_五种_kind_都出现() {
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
	mm := map[int]int{0: 5}
	println(*p + s[0] + f() + len(g) + mm[0])
}
"#;
    let (out, s) = run_with_env("gc_heap_kinds", src, &[]).expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "13\n", "程序本身要跑对");
    let [o, a, c, r, m] = s.kinds;
    assert!(o >= 1, "object（new）没出现：{:?}", s.kinds);
    assert!(a >= 1, "array（make）没出现：{:?}", s.kinds);
    assert!(c >= 1, "closure 没出现：{:?}", s.kinds);
    assert!(r >= 1, "raw（字符串拼接）没出现：{:?}", s.kinds);
    assert!(m >= 1, "mapslots（map 槽块）没出现：{:?}", s.kinds);
    s.assert_clean().expect("自检计数器");
}

/// 内部指针回块首（`T-HEAP-05`，`C12` §3.3–§3.4）。
///
/// # 这条钉的是什么
///
/// `P` 是**全局** `*byte`，指向一个 1 MiB 数组的**最后一个字节** —— 一个典型的
/// 内部指针。GC 扫全局根时拿到的是块中部地址，必须靠起始位图 + 页摘要**回扫**
/// 到块首才能标记整个数组。
///
/// # 为什么必须是「跨页」的尾指针
///
/// 1 MiB 数组跨 256 个页，起点在**页 0**。尾字节落在页 255，而页 255 的摘要位
/// 是 0（数组内部没有别的块起点）⇒ 这条走的正是 `resolve_block` 的**退化路径**：
/// 本页没起点 ⇒ 往前找最近有起点的页（页 0）⇒ 从页 0 顶部回扫到块首。
///
/// 漏了退化，`resolve_block` 返回 null ⇒ `bad_ptr++` ⇒ 数组被回收 ⇒
/// `s[0]` 读到垃圾（`gc_alloc` 复用后是 0）。
///
/// ⚠️ 用「页内」的内部指针（比如 `&s[3]`）**测不出**这条：本页有起点，
/// 第 2 步就够了。所以必须取尾字节。
#[test]
fn t_heap_05_内部指针回块首() {
    let src = r#"package main

var P *byte

func churn() {
	for i := 0; i < 20000; i++ {
		_ = new([1000]byte)
	}
}

func main() {
	s := make([]byte, 1<<20)
	s[0] = 1
	s[(1<<20)-1] = 2
	P = &s[(1<<20)-1]
	churn()
	println(*P, s[0])
}
"#;
    let (out, s) = run_with_env("gc_heap_inner", src, &[("RGOC_GC_STRESS", "16")])
        .expect("编译 + 运行不该失败");
    assert_eq!(
        out.stdout, "2 1\n",
        "内部指针没回块首 ⇒ 1 MiB 数组被回收（stderr: {}）",
        out.stderr
    );
    s.assert_clean().expect("自检计数器（bad_ptr 必须 0）");
}

/// 复用块必须清零（`T-HEAP-01`，`C12` §1.3）。
///
/// `G = nil` 之后那块 `int` 变垃圾，被回收进 free list。`q := new(int)` 会
/// **复用**它 —— 而里面躺着 99。`gc_alloc` 恒 memset，所以 `*q` 必须是 0。
///
/// 不清零的后果不是「读到脏数据」这么轻：里面躺着的可能是**上一条命的指针**，
/// 那会让 GC 去标记一个已经死了的对象（或者更糟，指向空闲块 ⇒ `bad_ptr`）。
#[test]
fn t_heap_01_复用块必须清零() {
    let src = r#"package main

var G *int

func main() {
	G = new(int)
	*G = 99
	G = nil
	for i := 0; i < 20000; i++ {
		_ = new([64]byte)
	}
	q := new(int)
	println(*q)
}
"#;
    let (out, s) = run_with_env("gc_heap_zero", src, &[("RGOC_GC_STRESS", "16")])
        .expect("编译 + 运行不该失败");
    assert_eq!(
        out.stdout, "0\n",
        "复用的块没清零 ⇒ 读到上一条命的 99（stderr: {}）",
        out.stderr
    );
    s.assert_clean().expect("自检计数器");
}

/// 切片复合字面量的元素**各不相同**（`T-HEAP-07`，`R-10-22`）。
///
/// # 这条钉的是什么
///
/// `Instr::Composite` 在类型是**切片**时要 `rt_makeslice` 建底层数组 ——
/// 那是**分配**，所以它必须是 safepoint。漏了它，`SC{[]int{1,2,3}, ...}` 的
/// 三个底层数组会在构造过程中互相顶掉：第一个被回收、第二个复用同一块、
/// 第三个再复用 ⇒ 三个 slice 头指向**同一个数组**。
///
/// 实测（`ken/complit.go`）：`sc.a[i]` / `sc.b[i]` / `sc.c[i]` 全读到
/// `sc.c` 的值。最小复现只要三行。
#[test]
fn t_heap_07_切片字面量元素各不相同() {
    let src = r#"package main

type SC struct{ a, b, c []int }

var sc = SC{[]int{1, 2, 3}, []int{4, 5, 6}, []int{7, 8, 9}}

func main() {
	println(sc.a[0], sc.b[0], sc.c[0])
}
"#;
    let (out, s) = run_with_env("gc_heap_slicelit", src, &[("RGOC_GC_STRESS", "1")])
        .expect("编译 + 运行不该失败");
    assert_eq!(
        out.stdout, "1 4 7\n",
        "三个 slice 头指向了同一个数组（Composite 不是 safepoint？）"
    );
    s.assert_clean().expect("自检计数器");
}

/// `map` 的**值**里的指针要保活（`T-MAP-11`，`R-10-17`）。
///
/// # 这条钉的是什么
///
/// map 的槽块（`mapslots`）必须进托管堆并**被扫描**：`map[int][]int` 的每个
/// 值是一个 slice 头，它的 ptr 指向另一块。槽块如果不扫（M9 的 arena 语义），
/// 那些数组会被回收、被后来的分配复用 ⇒ 所有键读到同一份数据。
///
/// 实测（`ken/complit.go`）：`mc[i][j]` 全部读到最后一组。
#[test]
fn t_map_11_map_值里的指针要保活() {
    let src = r#"package main

var mc = map[int][]int{0: []int{1, 2, 3}, 1: []int{4, 5, 6}, 2: []int{7, 8, 9}}

func main() {
	for i := 0; i < 2000; i++ {
		_ = new([64]byte)
	}
	println(mc[0][0], mc[1][0], mc[2][0], len(mc))
}
"#;
    let (out, s) =
        run_with_env("gc_map_ptr", src, &[("RGOC_GC_STRESS", "8")]).expect("编译 + 运行不该失败");
    assert_eq!(
        out.stdout, "1 4 7 3\n",
        "map 值里的指针没被扫描（槽块进了托管堆但没扫？）stderr: {}",
        out.stderr
    );
    s.assert_clean().expect("自检计数器");
}

/// `delete` 之后的 `TOMB` 槽不能当成活槽扫（`T-MAP-12`）。
///
/// # 这条钉的是什么
///
/// 槽块的 `n` 是 **cap**，扫描按 cap 走。`delete` 只把状态改成 `TOMB`，
/// **不清 key/val** —— 所以 TOMB 槽里躺着已删条目的旧指针。按状态过滤掉它们，
/// 那些对象才会被回收；不过滤就是把死对象标记活（永久泄漏）。
///
/// # ⚠️ 判据必须是「删完之后还要再分配」
///
/// 第一版在 `delete` 循环后直接 `println(len(m))` —— 那之后**一次分配都没有**，
/// 于是 GC 根本不会再跑，200 个块一个都回收不掉（`frees=6`），
/// 「不过滤」的变异实测**是绿的**。
///
/// 所以后面接一段 `new` 循环把 GC 逼出来。
///
/// # 判据为什么是 `live` 而不是 `frees`
///
/// `frees` 太软：实测「过滤」2205、「不过滤」2005 —— 都远大于任何合理的
/// 阈值，`frees >= 190` 这种写法两次都绿（第一版就是这样，变异没抓到）。
///
/// 换成 `live` 就硬了：那 200 个被删的块每个是 `align16(16+64) = 80` 字节，
/// 合计 **16000** 字节。不过滤时它们全被 TOMB 槽钉住 ⇒ `live >= 16000`；
/// 过滤时只剩最后那批 churn 块 ⇒ 实测 12384。所以阈值 `200 * 80` 是**从数据
/// 推出来的**，不是凑出来的。
#[test]
fn t_map_12_删掉的槽不参与扫描() {
    let src = r#"package main

func main() {
	m := map[int]*[64]byte{}
	for i := 0; i < 200; i++ {
		m[i] = new([64]byte)
	}
	for i := 0; i < 200; i++ {
		delete(m, i)
	}
	for i := 0; i < 2000; i++ {
		_ = new([64]byte)
	}
	println(len(m))
}
"#;
    let (out, s) =
        run_with_env("gc_map_tomb", src, &[("RGOC_GC_STRESS", "8")]).expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "0\n", "stderr: {}", out.stderr);
    assert!(
        s.live < 200 * 80,
        "存活字节 {} ≥ 16000 —— 删掉的 200 个 80 字节块被 TOMB 槽钉住了：{s:?}",
        s.live
    );
    s.assert_clean().expect("自检计数器");
}

/// 字符串累加循环（`T-HEAP-08`，`R-10-20`）。
///
/// # 这条钉的是什么
///
/// 每一轮都 `rt_strcat` 出一个新块、上一轮的块变成垃圾。几轮之后堆里就有
/// **空闲区**，而空闲区的起始位图必须置位（`free_push` 里 `bm_set`）。
///
/// 漏了它，`resolve_block` 对「指向空闲区的指针」会往前找到**残留的旧位**
/// （某个更早被回收的块），把空闲区里的旧字节当成块头 —— 旧的 desc 指针
/// bit0 是 0，于是被当成**活块**标记、进 worklist、`scan_block` 拿旧 desc
/// 扫垃圾指针 ⇒ 段错误。
///
/// 实测：`ken/simpswitch.go` 就是这样崩的（`stress` 任意间隔都崩）。
/// 这个用例是它的最小化：`switch` 只是为了制造多条产出路径，真正起作用的是
/// 循环里的两次 `+`。
#[test]
fn t_heap_08_字符串累加循环() {
    let src = r#"package main

func main() {
	r := ""
	a := 3
	for i := 0; i < 10; i = i + 1 {
		switch i {
		case 5:
			r += "five"
		case a, 7:
			r += "a"
		default:
			r += string(i + 48)
		}
		r += "out" + string(i+48)
	}
	println(r)
}
"#;
    let want = "0out01out12out2aout34out4fiveout56out6aout78out89out9\n";
    let (out, s) = run_with_env("gc_heap_strloop", src, &[("RGOC_GC_STRESS", "1")])
        .expect("编译 + 运行不该失败");
    assert_eq!(
        out.stdout, want,
        "字符串累加循环错了（空闲区的起始位图没维护？）stderr: {}",
        out.stderr
    );
    s.assert_clean().expect("自检计数器");
}
