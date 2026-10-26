//! 精确性与「没有寄存器根」（`T-PREC-01` / `T-GC-10`，M10/Task 28–29）。
//!
//! # 这两条钉的是同一件事的两面
//!
//! `M10-design.md` §3 的两条硬约束：
//!
//! 1. **没有寄存器根** —— 每个值一槽，函数调用前后寄存器里的东西都能在帧里
//!    找到（`C5` §7）。所以扫描器只看帧 + 全局 + defer 链，**不看寄存器**。
//! 2. **位图是精确的**（逐位点）—— 只有真正是指针的字才被当根。
//!
//! 第 1 条一旦破了（有人加了寄存器分配、把值留在 `x19..x28` 里），扫描器就
//! 会漏根，而漏根的症状是**随机时刻**的活对象被回收 —— 极难查。所以用一条
//! **静态**判据把它钉住：函数体里不许出现 callee-saved 寄存器的保存。
//!
//! 第 2 条破了（退化成整帧保守扫描）不会漏根，但会把「像地址的整数」当根 ——
//! 于是死对象被钉住不回收。判据用**存活字节**：一个 `uintptr` 里存着某个
//! 大对象的地址，那个对象必须能被回收。

mod support;

use support::gc_probe::run_with_env;
use support::native_harness::build_and_run_env;

/// 像地址的整数**不保活**（`T-PREC-01`，`C12` §4.3）。
///
/// # 这条钉的是什么
///
/// `u := uintptr(unsafe.Pointer(p))` 之后 `p = nil`：那块 8 KiB 就是垃圾了。
/// `u` 是 **`uintptr`**（不是指针），它的槽在 `ptr_mask` 里是 0 —— 所以精确
/// 位图**不标它**，块会被回收。
///
/// 如果位图退化成「整帧所有字都当根」，`u` 会被当指针扫 ⇒ 那块 8 KiB 永远
/// 活着 ⇒ `live >= 8208`。
///
/// # ⚠️ 为什么判据是 `live` 而不是 `bad_ptr`
///
/// `bad_ptr` 抓的是「解析不到块首」。而 `resolve_block` 对「块之后的地址」
/// 会往前找到那个块的起点（起始位图里有位），所以**任何**看起来像堆地址的
/// 整数都能"解析成功" —— `bad_ptr` 恒 0，这条判据就废了。
///
/// 换句话说：保守扫描的代价不是「扫错」而是「钉住死对象」，判据必须量**泄漏**。
#[test]
fn t_prec_01_像地址的整数不保活() {
    let src = r#"package main

import "unsafe"

func churn() {
	for i := 0; i < 3000; i++ {
		_ = new([64]byte)
	}
}

func main() {
	p := new([8192]byte)
	u := uintptr(unsafe.Pointer(p))
	p = nil
	churn()
	println(u != 0)
}
"#;
    let (out, st) = run_with_env("gc_prec_uintptr", src, &[("RGOC_GC_STRESS", "8")])
        .expect("编译 + 运行不该失败");
    assert_eq!(
        out.stdout, "true\n",
        "程序本身要跑对：stderr {}",
        out.stderr
    );
    // `[8192]byte` 的块是 `align16(16 + 8192) = 8208` 字节。
    assert!(
        st.live < 8192,
        "存活字节 {} ≥ 8192 —— 那块 `[8192]byte` 被一个 `uintptr` 钉住了：{st:?}",
        st.live
    );
    st.assert_clean().expect("自检计数器");
}

/// 函数体里**没有** callee-saved 寄存器的保存（`T-GC-10`，`C5` §7）。
///
/// # 为什么这条能当「没有寄存器根」的判据
///
/// 扫描器不看寄存器，前提是「任何时刻活着的值都在帧里」。如果代码把值留在
/// `x19..x28`（callee-saved）跨过一次调用，那个值在 GC 眼里就**不存在** ——
/// 而它正是最容易被回收的（跨调用的值通常就是长命的）。
///
/// 本项目**没有寄存器分配器**：每个 `ValueId` 一个槽，用到时 `ldr` 出来、
/// 用完就丢。所以「不出现 callee-saved 的保存」是这条设计的**结构性证据**：
/// 出现了就说明有人开始做寄存器分配，那么 `C12` §4 的整个根协议都要重审。
///
/// 顺带钉住 `x29`（`fp`）只在序言/尾声出现 —— 记录链靠它。
#[test]
fn t_gc_10_没有寄存器根() {
    // 挑一个「有调用、有循环、有指针」的函数：最容易诱使人把值留在寄存器里。
    let src = r#"package main

type T struct {
	a, b int
	p    *int
}

func sum(xs []int) int {
	s := 0
	for i := 0; i < len(xs); i++ {
		s += xs[i]
	}
	return s
}

func main() {
	t := new(T)
	t.p = new(int)
	*t.p = 7
	xs := make([]int, 8)
	for i := 0; i < 8; i++ {
		xs[i] = i
	}
	println(t.a + t.b + *t.p + sum(xs))
}
"#;
    let out = build_and_run_env("gc_reg_roots", src, &[("RGOC_GC_STRESS", "8")])
        .expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "35\n", "程序本身要跑对：stderr {}", out.stderr);

    // callee-saved：x19..x28（x29 = fp、x30 = lr 由 `stp x29, x30` 保存）。
    let mut hits: Vec<String> = Vec::new();
    for (i, line) in out.asm.lines().enumerate() {
        let t = line.trim();
        if t.starts_with("//") || t.starts_with('#') || t.starts_with('.') {
            continue;
        }
        for r in [
            "x19", "x20", "x21", "x22", "x23", "x24", "x25", "x26", "x27", "x28",
        ] {
            // 只要出现「存到 callee-saved」或「从 callee-saved 取」就算。
            if t.starts_with(&format!("str {r},"))
                || t.starts_with(&format!("stp {r},"))
                || t.contains(&format!("[{r}]"))
                || t.starts_with(&format!("ldr {r},"))
                || t.starts_with(&format!("ldp {r},"))
                || t.starts_with(&format!("mov {r},"))
            {
                hits.push(format!("{}:{}: {t}", i + 1, r));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "函数体里出现了 callee-saved 寄存器（{} 处）。本项目**没有寄存器分配**，\n\
         值一律落帧槽（`C5` §7）；一旦有值跨调用留在 `x19..x28`，扫描器就看不见它\n\
         ⇒ 随机时刻的活对象被回收。要么改回用槽，要么重审 `C12` §4 的根协议：\n  {}",
        hits.len(),
        hits.join("\n  ")
    );
}
