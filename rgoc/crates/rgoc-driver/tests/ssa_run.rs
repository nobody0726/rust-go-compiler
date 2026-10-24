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

// ── Task 34：控制流、块参数与 Panic ─────────────────────────────────

#[test]
fn t_ssa_34_if_else_分支() {
    expect_out(
        "if-else",
        "package main\n\nfunc main() {\n\tx := 3\n\tif x > 2 {\n\t\tprintln(\"big\")\n\t} else {\n\t\tprintln(\"small\")\n\t}\n}\n",
        "big\n",
    );
}

#[test]
fn t_ssa_34_循环累加() {
    // 回边 + 块参数：`sum` 每轮携带新值
    expect_out(
        "loop",
        "package main\n\nfunc main() {\n\tsum := 0\n\tfor i := 1; i <= 5; i++ {\n\t\tsum += i\n\t}\n\tprintln(sum)\n}\n",
        "15\n",
    );
}

#[test]
fn t_ssa_34_并行赋值交换() {
    // `a, b = b, a`：边实参必须**先全算再全写**（`T-SSA-28` 的求值面）
    expect_out(
        "swap",
        "package main\n\nfunc main() {\n\ta, b := 1, 2\n\ta, b = b, a\n\tprintln(a, b)\n}\n",
        "2 1\n",
    );
}

#[test]
fn t_ssa_34_switch_与_fallthrough() {
    expect_out(
        "switch",
        "package main\n\nfunc main() {\n\tx := 2\n\tswitch x {\n\tcase 1:\n\t\tprintln(\"one\")\n\tcase 2:\n\t\tprintln(\"two\")\n\t\tfallthrough\n\tcase 3:\n\t\tprintln(\"three\")\n\tdefault:\n\t\tprintln(\"other\")\n\t}\n}\n",
        "two\nthree\n",
    );
}

#[test]
fn t_ssa_34_break_与_continue() {
    expect_out(
        "break-continue",
        "package main\n\nfunc main() {\n\tsum := 0\n\tfor i := 0; i < 10; i++ {\n\t\tif i == 5 {\n\t\t\tbreak\n\t\t}\n\t\tif i%2 == 0 {\n\t\t\tcontinue\n\t\t}\n\t\tsum += i\n\t}\n\tprintln(sum)\n}\n",
        // 1 + 3 = 4（0/2/4 被 continue 跳过，5 处 break）
        "4\n",
    );
}

#[test]
fn t_ssa_34_labeled_break() {
    expect_out(
        "labeled-break",
        "package main\n\nfunc main() {\n\tn := 0\nouter:\n\tfor i := 0; i < 3; i++ {\n\t\tfor j := 0; j < 3; j++ {\n\t\t\tif j == 1 {\n\t\t\t\tbreak outer\n\t\t\t}\n\t\t\tn++\n\t\t}\n\t}\n\tprintln(n)\n}\n",
        // 只有 (0,0) 会 +1
        "1\n",
    );
}

// `goto` 的运行期判据在 Task 35：`goto_mode` 会把**所有槽**建成根
// （`R-25-1`），于是 `i++` 走 `AddrOf`/`Load`/`Store` —— 那是 Task 35 的面。

#[test]
fn t_ssa_34_panic_走合并流且退出码_2() {
    let (out, exit, diag) = run_ssa_src(
        "panic",
        "package main\n\nfunc main() {\n\tprintln(\"before\")\n\tpanic(\"boom\")\n}\n",
    );
    assert_eq!(out, "before\npanic: boom\n", "panic 文本进合并流");
    assert_eq!(exit, 2, "未捕获 panic ⇒ 退出码 2");
    assert!(diag.is_none(), "panic 不是诊断（与 M5 同口径）");
}

#[test]
fn t_ssa_34_短路真的不执行右侧() {
    // `f() && g()`：`g` 不能被调用。这里用「调用有输出」的可见副作用验证：
    // `side()` 打印一次，`false && side()` 的右支不该跑。
    let (out, exit, _) = run_ssa_src(
        "short-side-effect",
        "package main\n\nfunc side() bool {\n\tprintln(\"side\")\n\treturn true\n}\n\nfunc main() {\n\tif false && side() {\n\t\tprintln(\"never\")\n\t}\n\tprintln(\"done\")\n}\n",
    );
    assert_eq!(exit, 0, "正常退出");
    assert_eq!(
        out, "done\n",
        "`false && side()` 的右支不该被求值（短路必须是控制流）"
    );
}
