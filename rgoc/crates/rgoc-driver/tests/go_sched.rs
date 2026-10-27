//! `GO-2` 调度三规则（M11b）—— 判据骨架。
//!
//! `C11` §2.1 三条：**M:1**（一个 OS 线程）/ **不抢占**（只在阻塞点让出）/
//! **就绪队列 FIFO**。这里先放 FIFO 那条（最容易写错成「队头插入」），
//! M:1 与不抢占的判据在 Task 14 补。
//!
//! # 为什么用「各自发信号」而不是 `for {}` + 超时
//!
//! FIFO 的判据要**确定的输出**。用 `for {}` 只能证明「挂死」，证明不了顺序。
//! 而 5 个 G 按创建顺序入队、各自跑完发信号，输出必须恰好是 `01234` ——
//! 倒序（队头插入）或乱序（真并发）都会红。

mod support;

use support::go_probe;

const FIFO: &str = r#"
func main() {
	done := make(chan bool)
	for i := 0; i < 5; i++ {
		go func(n int) {
			print(n)
			done <- true
		}(i)
	}
	for i := 0; i < 5; i++ {
		<-done
	}
	println()
}
"#;

#[test]
#[ignore = "M11b 实现后启用（Task 31 收口）：现在还没有 go 语句"]
fn t_go_03_就绪队列_fifo() {
    let t =
        go_probe::probe("go03", FIFO, &[("RGOC_GC", "0")]).expect("应能编译并跑起来（M11b 之后）");
    t.assert_l1("go03");
    assert_eq!(
        t.native.stdout, "01234\n",
        "就绪队列必须是 FIFO（队尾入队）：实际 {:?}",
        t.native.stdout
    );
    assert_eq!(t.native.exit, 0, "stderr={:?}", t.native.stderr);
}
