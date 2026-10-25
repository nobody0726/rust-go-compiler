//! native 装置的第一批 fixture（`M7-plan.md` Task 4；`T-NAT-42/43` 的最早形态）。
//!
//! # 三条 fixture 的判据
//!
//! Task 50 接线后 `build` 有真产物，三条从 `#[ignore]` 转为必跑。
//! `骨架期_build_未实现时装置必须报错` 随之改名 —— 它现在钉的是**另一半**：
//! 装置不许把「编译失败」当成「跑出了空输出」（用一条故意写错的 Go 源去撞）。

mod support;

use support::native_harness;

const HELLO: &str = include_str!("../fixtures/native/hello.go");
const RET42: &str = include_str!("../fixtures/native/ret42.go");
const EMPTY: &str = include_str!("../fixtures/native/empty.go");

#[test]
fn 编译失败时装置必须报错而不是返回空输出() {
    // 装置不许把「编译失败」当成「跑出了空输出」—— 那会让每条 fixture 都「通过」。
    let bad = "package main\nfunc main() { x := }\n";
    let err = native_harness::build_and_run("bad-syntax", bad).unwrap_err();
    assert!(
        err.contains("build 失败"),
        "应带出 build 的失败信息，实际：{err}"
    );
}

#[test]
fn t_nat_42_hello_的字节() {
    let out = native_harness::build_and_run("hello", HELLO).unwrap();
    assert_eq!(out.stdout, "hello, world\n");
    assert_eq!(out.stderr, "");
    assert_eq!(out.exit, 0);
}

#[test]
fn t_nat_16_乘法走指令选择() {
    let out = native_harness::build_and_run("ret42", RET42).unwrap();
    assert_eq!(out.stdout, "42\n");
    assert_eq!(out.exit, 0);
}

#[test]
fn t_nat_43_空程序退出码为零() {
    let out = native_harness::build_and_run("empty", EMPTY).unwrap();
    assert_eq!(out.stdout, "");
    assert_eq!(out.stderr, "");
    assert_eq!(out.exit, 0);
}

/// `println` 打 `bool` 必须是 `true`/`false`，**不是** `1`/`0`。
///
/// # 这条是写 `mem_alloc` 时顺手发现的既有 bug（M9/Task 6）
///
/// `1 == 1` 的类型是 **`untyped bool`**，而 isel 的打印分派只认
/// `BasicId::Bool`（不带 `untyped`）⇒ 落到 `rt_print_i64` ⇒ 打 `1 0`。
/// 官方与解释器都是 `true false`。
///
/// # 为什么 M7 的语料没抓到
///
/// `t_nat_42_43`（native vs 解释器，46 个语料逐字节）全绿 —— 但那 46 个文件里
/// **没有一条** `println` 一个布尔值。这就是「语料全绿」与「语义对」之间的差。
#[test]
fn t_m9_06_布尔值的打印是_true_false() {
    let out = native_harness::build_and_run(
        "printbool",
        r#"
package main

func main() {
	println(1 == 1, 1 == 2)
	print(true, false)
}
"#,
    )
    .unwrap();
    assert_eq!(out.stdout, "true false\ntruefalse");
    assert_eq!(out.exit, 0);
}
