//! 栈根（M10/Task 22–23；`C12` §4.2–§4.4）。
//!
//! # Phase 1 与 Phase 2 的分界
//!
//! Phase 1 的 GC **只扫全局根** —— 局部变量持有的指针会被收掉。所以 Phase 1
//! 的 fixture 全都写成「活对象挂在全局变量上」。
//!
//! Phase 2 接上**帧记录链**之后，这一条不再是限制：下面三个程序里的 `p`
//! 都是**局部变量**，而 `churn()` 期间的每一次 GC 都必须把它们标记成活的。
//!
//! # 三条判据各钉什么
//!
//! | 判据 | 钉的 |
//! |---|---|
//! | `t_root_09` | 帧记录链**通**了：`main` 的 `p` 活过 2 万次分配 |
//! | `t_root_10` | **逐位点**（不是函数级并集）：像地址的整数 `u` 不能被当根 |
//! | `t_root_11` | 三层调用：每层的 epilogue 都摘链（漏一层就扫到已返回的帧）|

mod support;

use support::gc_probe::run_with_env;

/// 栈上的局部指针活过 churn（`T-ROOT-09`）。
///
/// # 这条为什么是 Phase 2 的里程碑
///
/// `p` 只在 `main` 的帧里。GC 在 `churn()` 的分配点触发，此刻栈上有两帧
/// （`main` + `churn`）。要保住 `*p`，必须：
///
/// 1. `main` 的序言把 `prev/fi/sp_idx` 写进帧、`g_frames = fp`；
/// 2. `churn` 的序言接着链上去；
/// 3. GC 从 `g_frames` 走到 `main` 的帧，用**当前位点**的位图找到 `p` 的槽；
/// 4. `mark_ptr` 标记 `p` 指向的块。
///
/// 任何一环漏了，`*p` 读到的是被回收后复用成 0 的内存。
#[test]
fn t_root_09_局部指针活过_churn() {
    let src = r#"package main

func churn() {
	for i := 0; i < 20000; i++ {
		_ = new([64]byte)
	}
}

func main() {
	p := new(int)
	*p = 7
	churn()
	println(*p)
}
"#;
    let (out, s) = run_with_env("gc_stack_local", src, &[("RGOC_GC_STRESS", "16")])
        .expect("编译 + 运行不该失败");
    assert_eq!(
        out.stdout, "7\n",
        "栈根没扫到 ⇒ `*p` 读到被复用的内存（stderr: {}）",
        out.stderr
    );
    s.assert_clean().expect("自检计数器（bad_sp 必须 0）");
    assert!(s.collections >= 1, "stress 下必须真的收过：{s:?}");
}

/// **逐位点**而不是函数级并集（`T-ROOT-10`，`C12` §4.4）。
///
/// # 这条钉的是什么
///
/// `u` 是 `uintptr(p) + 8` —— 一个**像地址但不是块首**的整数。它在
/// `churn()` 的分配点上是活的（后面还要 `u != 0`），所以会出现在**函数级
/// 并集**位图里（它的槽类型是 `uintptr`，不是指针 —— 但并集位图是「整个
/// 函数所有位点的并」，只要有任何一处它是根就会带上）。
///
/// 用并集位图的后果：`mark_ptr(u)` ⇒ `u` 在 chunk 范围内但不在块首 ⇒
/// `bad_ptr++`。所以 `bad_ptr == 0` 就是「真的逐位点」的证据。
///
/// ⚠️ `u` 必须**活到 GC 之后**（`println(..., u != 0)`）—— 否则 DCE 会把它删掉，
/// 判据退化成空话。
#[test]
fn t_root_10_逐位点不是并集() {
    let src = r#"package main

import "unsafe"

func churn() {
	for i := 0; i < 20000; i++ {
		_ = new([64]byte)
	}
}

func main() {
	p := new(int)
	*p = 42
	u := uintptr(unsafe.Pointer(p)) + 8
	churn()
	println(*p, u != 0)
}
"#;
    let (out, s) = run_with_env("gc_stack_precise", src, &[("RGOC_GC_STRESS", "16")])
        .expect("编译 + 运行不该失败");
    assert_eq!(
        out.stdout, "42 true\n",
        "`*p` 被回收或 `u` 被当根（stderr: {}）",
        out.stderr
    );
    s.assert_clean()
        .expect("`uintptr` 被当成根 ⇒ bad_ptr 非 0（那是保守扫描，不是精确扫描）");
}

/// 三层调用：每层的 epilogue 都要摘链（`T-ROOT-11`）。
///
/// # 漏摘链的症状
///
/// `epilogue` 是唯一出口（`F14`），但如果某个函数的 `has_rec` 判定错了
/// （比如「有 safepoint 却没发记录」），它的 `g_frames` 就不会被设成自己的
/// `fp` —— 链会**跳过**它（这没问题）。反过来，如果**发了记录却没摘**
/// （epilogue 里漏了 `g_frames = prev`），链会指向一个**已经返回的帧**：
/// 那块栈随后被 `churn()` 的帧覆盖，GC 就会拿垃圾当 `fi`/`sp_idx` 去扫。
///
/// 三层是为了让「漏摘」在第 2、3 层返回之后**一定**发生（单层可能碰巧没事）。
#[test]
fn t_root_11_三层帧的链() {
    let src = r#"package main

func churn() {
	for i := 0; i < 20000; i++ {
		_ = new([64]byte)
	}
}

func lvl3(p *int) {
	churn()
	*p = *p + 1
}

func lvl2(p *int) {
	lvl3(p)
}

func lvl1(p *int) {
	lvl2(p)
}

func main() {
	p := new(int)
	*p = 5
	lvl1(p)
	println(*p)
}
"#;
    let (out, s) = run_with_env("gc_stack_three", src, &[("RGOC_GC_STRESS", "16")])
        .expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "6\n", "stderr: {}", out.stderr);
    s.assert_clean().expect("自检计数器");
}
