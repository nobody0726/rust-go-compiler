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
