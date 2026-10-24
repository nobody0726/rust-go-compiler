//! native 装置的第一批 fixture（`M7-plan.md` Task 4；`T-NAT-42/43` 的最早形态）。
//!
//! # 为什么三条 fixture 现在带 `#[ignore]`
//!
//! `build_cmd::run_build` 还没有产物（Task 50 接线）。三条测试若**现在**跑，必然全红 ——
//! 于是有两条路：
//!
//! | 做法 | 后果 |
//! |---|---|
//! | 让它们静默通过（`if !ok { return; }`） | 假绿 —— 本仓最反复禁止的一件事 |
//! | `#[ignore = "…"]` + 一条**绿**的状态断言 | 跳过是**显式**的；Task 50 去掉 ignore 时，红就是红 |
//!
//! 所以这里同时留了 `骨架期_build_未实现时装置必须报错`：它断言**当前**的诚实行为
//! （`Err` 且信息里有「尚未实现」）。这条绿测试不是「等 Task 50 就删」的占位 ——
//! 它证明装置**不会**把「编译失败」当成「跑出了空输出」。

mod support;

use support::native_harness;

const HELLO: &str = include_str!("../fixtures/native/hello.go");
const RET42: &str = include_str!("../fixtures/native/ret42.go");
const EMPTY: &str = include_str!("../fixtures/native/empty.go");

#[test]
fn 骨架期_build_未实现时装置必须报错() {
    // Task 4 的判据：装置不许把「没产物」当成「跑出空输出」。
    let err = native_harness::build_and_run("skeleton-probe", EMPTY).unwrap_err();
    assert!(
        err.contains("尚未实现"),
        "装置应把 build 的 Err 原样带出，实际：{err}"
    );
}

#[test]
#[ignore = "build 无产物（Task 50 接线后去掉这条 ignore）"]
fn t_nat_42_hello_的字节() {
    let out = native_harness::build_and_run("hello", HELLO).unwrap();
    assert_eq!(out.stdout, "hello, world\n");
    assert_eq!(out.stderr, "");
    assert_eq!(out.exit, 0);
}

#[test]
#[ignore = "build 无产物（Task 50 接线后去掉这条 ignore）"]
fn t_nat_16_乘法走指令选择() {
    let out = native_harness::build_and_run("ret42", RET42).unwrap();
    assert_eq!(out.stdout, "42\n");
    assert_eq!(out.exit, 0);
}

#[test]
#[ignore = "build 无产物（Task 50 接线后去掉这条 ignore）"]
fn t_nat_43_空程序退出码为零() {
    let out = native_harness::build_and_run("empty", EMPTY).unwrap();
    assert_eq!(out.stdout, "");
    assert_eq!(out.stderr, "");
    assert_eq!(out.exit, 0);
}
