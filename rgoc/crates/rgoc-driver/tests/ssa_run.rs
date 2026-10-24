//! SSA 求值器的**端到端**判据（`M6-plan.md` Task 33 起；`T-SSA-32/34/35/36/37` 的雏形）。
//!
//! # 为什么在 `rgoc-driver` 而不是 `rgoc-interp`
//!
//! `rgoc-interp` 的 `tests/ssa_exec.rs` 手搓 IR，测的是**执行器**（块参数、边实参、
//! 内存根这些形态）。这一层测的是**整条链**：Go 源码 → 检查 → lowering → 建图 → 跑，
//! 判据是「输出与 `go run` 一致」的雏形 —— 只有 `rgoc-driver` 能同时看到前后两端。
//!
//! # 判据口径
//!
//! 与 M5 的 `run_semantics.rs` 同源：合并流**逐字节**比较（`print`/`println` 都进
//! 同一股），退出码一起比。唯一容差 `\r\n → \n`。

use std::path::PathBuf;

use rgoc_check::CheckOptions;
use rgoc_load::load_with_imports;
use rgoc_ssa::build::build;
use rgoc_types::TypeTable;

/// 跑一段源码的 SSA 路径，返回 `(输出, 退出码, 诊断)`。
fn run_ssa_src(name: &str, src: &str) -> (String, i32, Option<String>) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/ssa-run");
    std::fs::create_dir_all(&dir).expect("建不了 target/tmp/ssa-run");
    let path = dir.join(format!("{name}.go"));
    std::fs::write(&path, src).expect("写不了 fixture");

    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, &[path], &[], None);
    let opts = CheckOptions { test_hooks: false };
    let (checked, lowered) =
        rgoc_check::check::check_package_with(&pkg, &mut tt, bag, &opts, |s| {
            rgoc_hir::lower(&pkg, s)
        });
    assert_eq!(
        checked.rendered_count(),
        0,
        "fixture 不该有静态诊断（有诊断说明 fixture 写错了）"
    );
    let program = lowered.expect("fixture 不该被 lowering 拒绝");
    let out = build(&program, &mut tt);
    assert!(
        out.rejected.is_empty(),
        "fixture 不该被 SSA 拒绝：{:?}",
        out.rejected
    );
    let o = rgoc_interp::run_ssa(&out.module, &tt, rgoc_interp::Options::default());
    (o.output, o.exit, o.diag)
}

/// 断言「输出 == 期望」且退出码 0。
fn expect_out(name: &str, src: &str, want: &str) {
    let (out, exit, diag) = run_ssa_src(name, src);
    assert_eq!(out, want, "输出不符（{name}）；diag = {diag:?}");
    assert_eq!(exit, 0, "退出码该是 0（{name}）；diag = {diag:?}");
}

// ── Task 33：纯值 ───────────────────────────────────────────────────

#[test]
fn t_ssa_33_算术与优先级() {
    // `1 + x*3`：`x` 是变量 ⇒ 不会被检查器折叠成常量，真走 `Bin`
    expect_out(
        "arith",
        "package main\n\nfunc main() {\n\tx := 2\n\tprintln(1 + x*3)\n}\n",
        "7\n",
    );
}

#[test]
fn t_ssa_33_int8_溢出回绕() {
    // Go 的整数运算是按类型宽度回绕的：`int8(127) + 1 == -128`
    expect_out(
        "wrap",
        "package main\n\nfunc main() {\n\tvar x int8 = 127\n\tprintln(x + 1)\n}\n",
        "-128\n",
    );
}

#[test]
fn t_ssa_33_字符串拼接() {
    expect_out(
        "strcat",
        "package main\n\nfunc main() {\n\ts := \"a\"\n\tprintln(s + \"b\")\n}\n",
        "ab\n",
    );
}

#[test]
fn t_ssa_33_复合字面量() {
    // `[2]int{1,2}` 走 `Composite`。**注意**：Go 的内建 `println` 对聚合值打印的是
    // **地址**（不可复现），`C9` 的契约用占位 `0xc000000000` 表示它
    // （`print::PTR_TEXT`）。所以这条只断言「Composite 跑得通、输出是占位地址」；
    // 元素级的判据要等 `Index`/`Field`（Task 35）能读之后。
    expect_out(
        "composite",
        "package main\n\nfunc main() {\n\tprintln([2]int{1, 2})\n}\n",
        "0xc000000000\n",
    );
}

#[test]
fn t_ssa_33_短路的值() {
    // 值层面：`true && false` = false，`false || true` = true
    // （「右侧不被求值」是 Task 34 之后的运行期判据）
    expect_out(
        "short-value",
        "package main\n\nfunc main() {\n\tprintln(true && false, false || true)\n}\n",
        "false true\n",
    );
}

#[test]
fn t_ssa_33_转换与浮点() {
    expect_out(
        "convert",
        "package main\n\nfunc main() {\n\tx := 1\n\tprintln(float64(x) / 2)\n}\n",
        "0.5\n",
    );
}

#[test]
fn t_ssa_33_一元运算() {
    expect_out(
        "unary",
        "package main\n\nfunc main() {\n\tx := 5\n\tprintln(-x, ^x, !(x == 5))\n}\n",
        "-5 -6 false\n",
    );
}
