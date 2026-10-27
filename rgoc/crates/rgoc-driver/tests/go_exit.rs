//! `GO-5` 死锁与退出（M11b）—— 判据骨架。
//!
//! `C9` §5 冻结：死锁 = `fatal error: all goroutines are asleep - deadlock!`
//! 进 **stderr**、退出 **2**、**不进合并流**（stdout 必须为空）。
//!
//! 这里先放死锁文本那条（逐字节）。三条条件各自必要 / `main` 返回即退出 /
//! G 内 panic 在 Task 27–28 补。

mod support;

use support::go_probe;

const DEADLOCK: &str = r#"
package main

func main() {
	var c chan int
	<-c
}
"#;

#[test]
#[ignore = "M11b 实现后启用（Task 31 收口）：现在 <-ch 还静态拒绝"]
fn t_exit_01_死锁文本逐字节() {
    let t = go_probe::probe("exit01", DEADLOCK, &[("RGOC_GC", "0")])
        .expect("应能编译并跑起来（M11b 之后）");
    assert_eq!(t.native.exit, 2, "死锁必须是退出码 2");
    assert_eq!(
        t.native.stderr, "fatal error: all goroutines are asleep - deadlock!\n",
        "死锁文本必须逐字节相同（`C9` §5）：实际 {:?}",
        t.native.stderr
    );
    assert!(
        t.native.stdout.is_empty(),
        "死锁文本**只**进 stderr（`C9` §4.1 不合并）：stdout={:?}",
        t.native.stdout
    );
}
