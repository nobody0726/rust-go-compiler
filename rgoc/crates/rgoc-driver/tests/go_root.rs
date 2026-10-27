//! `GO-6` 多 G 根扫描（M11c）—— 判据骨架。
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
//! Phase 0 的 a/b 阶段强制 `mode=off`（`D-M11-12`），所以这条要 `RGOC_GC_STRESS=64`
//! 才能真的验到东西 —— 它是 `GO-6` 里**唯一**依赖 GC 重开的那条。

mod support;

use support::go_probe;

const SUSPENDED: &str = r#"
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
#[ignore = "M11c 实现后启用（Task 39 收口）：现在多 G 扫根还没有"]
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
