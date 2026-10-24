//! 浮点与复数打印（`T-NAT-34` / `T-NAT-35`）。
//!
//! # 判据有两层
//!
//! 1. **六个冻结点**（`C9` §4.4）—— 外部锚点，逐字节。这六个点覆盖了 `%g` 的
//!    指数/小数切换边界（`exp < -4 || exp >= 6`）、指数至少两位、`f32` 舍入、复数格式。
//! 2. **与解释器对撞** —— 20 个刁钻值（最小次正规、最大有限、`2^53+1`、`0.1+0.2`…）。
//!    解释器的 `format_float` 借 Rust 的 `{:e}` 拿最短往返，是独立实现。
//!
//! 只做第 1 层会漏「边界之外全错」；只做第 2 层会漏「两边一起错」。两层都要。

mod support;

use support::native_harness;

const FLOAT6: &str = include_str!("../fixtures/native/float6.go");
const FLOAT_PROP: &str = include_str!("../fixtures/native/float_prop.go");

#[test]
fn t_nat_34_c9_的六个冻结点() {
    let out = native_harness::build_and_run("float6", FLOAT6).unwrap();
    assert_eq!(
        out.stdout, "1e+06\n0.0001\n1e-05\n1e-07\n0.33333334\n(-1-2i)\n",
        "C9 §4.4 的六个点必须逐字节一致"
    );
    assert_eq!(out.exit, 0);
}

#[test]
fn t_nat_34_与解释器逐字节对撞() {
    // 20 个值：次正规 / 最大有限 / 2^53+1 / 0.1+0.2 / 各种指数边界。
    let native = native_harness::build_and_run("float_prop", FLOAT_PROP).unwrap();
    let hir = native_harness::hir_output("float_prop", FLOAT_PROP).unwrap();
    assert_eq!(
        native.stdout, hir,
        "native 与解释器的浮点格式化必须逐字节相同"
    );
    assert_eq!(native.exit, 0);
}
