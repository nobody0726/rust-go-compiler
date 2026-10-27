//! `GO-1` 栈与切换（M11a）—— 判据骨架。
//!
//! # 为什么 Phase 0 只放骨架
//!
//! native 侧现在**没有** goroutine 栈：`gdesc` / `rt_switch` 在 Task 8–9 才出现，
//! 序言栈检查在 Task 10。所以这里先写判据的**形状**（fixture + 期望文本），
//! `#[ignore]` 到 Task 15（Phase 1 收口）再去掉。
//!
//! 先写形状不是形式主义：判据说「递归到超过 64 KiB ⇒ stderr 恰好
//! `runtime error: goroutine stack exceeded 65536 bytes` + 退出 2」，才知道
//! `rt_panic_stack_overflow` 要打**哪个数**（是配置值还是实测值 —— 见 `T-STK-06`）。

mod support;

use support::go_probe;

/// 深递归：64 KiB 栈必然溢出（每帧 ≥ 48 字节 ⇒ 不到 1400 层）。
const DEEP: &str = r#"
package main

func rec(n int) int {
	if n == 0 {
		return 0
	}
	return rec(n-1) + 1
}

func main() {
	println(rec(100000))
}
"#;

#[test]
#[ignore = "M11a 实现后启用（Task 15 收口）：现在还没有 goroutine 栈"]
fn t_stk_03_溢出报错() {
    let t =
        go_probe::probe("stk03", DEEP, &[("RGOC_GC", "0")]).expect("应能编译并跑起来（M11a 之后）");
    assert_eq!(
        t.native.exit, 2,
        "栈溢出必须是退出码 2，实际 {}（stdout={:?} stderr={:?}）",
        t.native.exit, t.native.stdout, t.native.stderr
    );
    assert!(
        t.native
            .stderr
            .contains("runtime error: goroutine stack exceeded 65536 bytes"),
        "溢出文本不对（`C9` §5）：\n{}",
        t.native.stderr
    );
    assert!(
        t.native.stdout.is_empty(),
        "溢出前不应有半截输出：{:?}",
        t.native.stdout
    );
}

/// 最小程序：只为把 `rt_switch` 自测跑起来（程序本身什么都不做）。
const MIN: &str = r#"
package main

func main() {
}
"#;

/// `T-STK-01`：切换后 `x19..x28` 与 `d8..d15` 逐个不变（`M11-design.md` §3.3）。
///
/// # 为什么这条判据是「跑产物 + 看退出码」而不是直接调 `rt_switch`
///
/// 填寄存器、比对寄存器**只能是汇编** —— C 语言看不到寄存器分配，编译器可以
/// 随时复用它们。所以自测写在 `rt.c` 的顶层 `__asm__` 块里（`rt_sc_run`），
/// 由 `rt_init` 在 `RGOC_G11_SELFTEST=1` 时调用，失败 **退出 9**。
///
/// 9 这个码是刻意的：`2` 是运行期错误（`C9` §5），而自测失败只可能来自切换 ——
/// 用不同的码，判据红了就能直接分辨「切换坏了」与「程序自己坏了」。
///
/// # 自测自己也要守 ABI（`R-11-1`，实测踩过）
///
/// `rt_sc_run` 往 18 个 callee-saved 寄存器里填值，**必须先保存 C 的那份**。
/// 第一版没存 ⇒ 返回后 C 的局部变量全成了 `0x1111` ⇒ `rc=139`，而现象
/// **看起来像 `rt_switch` 切坏了**。查了半天才发现是自测函数自己违约。
#[test]
fn t_stk_01_callee_saved_全保住() {
    let out =
        support::native_harness::build_and_run_env("stk01", MIN, &[("RGOC_G11_SELFTEST", "1")])
            .unwrap_or_else(|e| {
                panic!(
                    "`rt_switch` 自测失败（rc=9 表示切换漏存了某个 callee-saved）：\n{e}\n\
                 期望：18 个寄存器（x19..x28 / d8..d15）切一圈回来逐个不变。"
                )
            });
    assert_eq!(out.exit, 0, "自测应干净退出");
    assert!(out.stderr.is_empty(), "自测不该有输出：{:?}", out.stderr);
}
