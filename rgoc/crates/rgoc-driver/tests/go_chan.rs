//! `GO-3` `hchan` 四操作（M11b）—— 判据骨架。
//!
//! `C11` §3.2 的四张表是**状态机**：发送方阻塞、接收方唤醒、`closed` 检查排第一。
//! 这里先放最基础的一条（无缓冲同步），其余（缓冲 FIFO / closed / len-cap /
//! nil 通道 / Sudog 不泄漏）在 Task 30 补。
//!
//! # 为什么「无缓冲同步」是第一条
//!
//! 它同时钉住三件事：① 发送方在无缓冲时必须**阻塞**；② 阻塞要**换 G**（不是
//! 忙等）；③ 唤醒后**值是对的**。任何一条错都会让它红，而它只有 5 行。

mod support;

use support::go_probe;

const SYNC: &str = r#"
package main

func main() {
	c := make(chan int)
	go func() {
		println(<-c)
	}()
	c <- 42
}
"#;

#[test]
#[ignore = "M11b 实现后启用（Task 31 收口）：现在 make(chan) 还静态拒绝"]
fn t_chan_01_无缓冲同步() {
    let t = go_probe::probe("chan01", SYNC, &[("RGOC_GC", "0")])
        .expect("应能编译并跑起来（M11b 之后）");
    t.assert_l1("chan01");
    assert_eq!(t.native.stdout, "42\n", "实际 {:?}", t.native.stdout);
    assert_eq!(t.native.exit, 0, "stderr={:?}", t.native.stderr);
}
