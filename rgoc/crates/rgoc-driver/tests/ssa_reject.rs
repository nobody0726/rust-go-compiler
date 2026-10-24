//! SSA 拒绝面的判据（`M6-plan.md` Task 31；`T-SSA-33` 的 `S` 腿）。
//!
//! # 判据的形状：**码 + 退出码**，两件都要
//!
//! 「拒了」不算数：静默跳过、空图、panic 都是「拒了」，只有**带码**的拒绝能让人
//! 知道发生了什么（`M6-tests.md` §4.6）。所以每条 fixture 断言三件事：
//!
//! 1. 退出码是 **1**（静态拒绝）—— 不是 0（假装成功）也不是 2（内部错）；
//! 2. stderr 里有那条 **`SSA-UNSUPPORTED-*` 文本**；
//! 3. 建图结果里被拒的函数**带**那个 `SsaCode`（结构断言，不靠文本匹配）。
//!
//! # 为什么有两条「够不着」的码
//!
//! `SSA-UNSUPPORTED-RANGE-FUNC` / `-GENERIC` 在**这个前端**里没有入口：
//! `RangeKind` 没有 `Func` 变体（`range` over func 在 lowering 就拒了），
//! 泛型也被检查器挡住（`G-M5-03`）。两条各有一个「守卫测试」：断言它们
//! **在 HIR 层就被拒**（`HIR-*` / `C8` 诊断）—— 一旦哪天上游开始放行，
//! 守卫就会红，提醒我们回来实现 SSA 侧的码，而不是留一个永远走不到的 `todo!`。

use std::path::{Path, PathBuf};

use rgoc_driver::cli::SsaCommand;
use rgoc_driver::ssa_cmd;
use rgoc_ssa::diag::SsaCode;

/// 写 fixture 并返回路径（`target/tmp/ssa-reject`，**不往只读语料里写**）。
fn fixture(name: &str, src: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/ssa-reject");
    std::fs::create_dir_all(&dir).expect("建不了 target/tmp/ssa-reject");
    let path = dir.join(format!("{name}.go"));
    std::fs::write(&path, src).expect("写不了 fixture");
    path
}

/// 跑 `ssa --verify`（最严格的一档：连 verifier 都要求过）。
fn run(path: &Path) -> ssa_cmd::SsaOutcome {
    ssa_cmd::run_ssa(&SsaCommand {
        files: vec![path.to_path_buf()],
        dump: false,
        verify: true,
    })
    .expect("run_ssa 不该返回 Err（Err 只代表参数/io 内部错）")
}

/// 断言：带 `code` 拒绝 + 退出码 1。
fn assert_rejected(name: &str, src: &str, code: SsaCode) {
    let p = fixture(name, src);
    let out = run(&p);
    assert_eq!(
        out.exit, 1,
        "{name}: 必须退出码 1（静态拒绝），实际 {}；stderr = {:?}",
        out.exit, out.stderr
    );
    assert!(
        out.stdout.is_empty(),
        "{name}: 拒绝时 stdout 必须为空（不许吐半截 dump），实际 {:?}",
        out.stdout
    );
    assert!(
        out.stderr.contains(code.code()),
        "{name}: stderr 必须含 `{}`，实际：{:?}",
        code.code(),
        out.stderr
    );
}

#[test]
fn t_ssa_33_闭包() {
    // 闭包会**提升成独立函数**：子函数因捕获被拒（CLOSURE），
    // 父函数因「函数字面量」被拒（FUNC-VALUE）—— 两条都在理，都要求带码。
    let p = fixture(
        "closure",
        "package main\n\nfunc main() {\n\tx := 1\n\tf := func() int { return x }\n\tprintln(f())\n}\n",
    );
    let out = run(&p);
    assert_eq!(out.exit, 1, "闭包必须被拒：{:?}", out.stderr);
    assert!(
        out.stderr.contains("SSA-UNSUPPORTED-CLOSURE"),
        "被提升的闭包函数要报 CLOSURE（捕获跨帧写）：{:?}",
        out.stderr
    );
    assert!(
        out.stderr.contains("SSA-UNSUPPORTED-FUNC-VALUE"),
        "父函数里的函数字面量要报 FUNC-VALUE：{:?}",
        out.stderr
    );
    // 结构断言：至少一个 Reject 带 Closure
    let (program, mut tt) = front_end(&p);
    let built = rgoc_ssa::build::build(&program, &mut tt);
    assert!(
        built.rejected.iter().any(|r| r.code == SsaCode::Closure),
        "rejected 里必须有 Closure：{:?}",
        built.rejected
    );
}

#[test]
fn t_ssa_33_defer() {
    assert_rejected(
        "defer",
        "package main\n\nfunc main() {\n\tdefer println(1)\n\tprintln(2)\n}\n",
        SsaCode::Defer,
    );
}

#[test]
fn t_ssa_33_recover() {
    assert_rejected(
        "recover",
        "package main\n\nfunc f() any {\n\treturn recover()\n}\n\nfunc main() {}\n",
        SsaCode::Defer,
    );
}

#[test]
fn t_ssa_33_接口动态派发() {
    assert_rejected(
        "iface",
        "package main\n\ntype I interface{ M() int }\n\ntype T struct{}\n\nfunc (T) M() int { return 1 }\n\nfunc main() {\n\tvar i I = T{}\n\tprintln(i.M())\n}\n",
        SsaCode::IfaceDyn,
    );
}

#[test]
fn t_ssa_33_type_switch() {
    assert_rejected(
        "typeswitch",
        "package main\n\nfunc main() {\n\tvar x any = 1\n\tswitch x.(type) {\n\tcase int:\n\t\tprintln(1)\n\t}\n}\n",
        SsaCode::IfaceDyn,
    );
}

#[test]
fn t_ssa_33_map() {
    assert_rejected(
        "map",
        "package main\n\nfunc main() {\n\tm := map[string]int{\"a\": 1}\n\tprintln(m[\"a\"])\n}\n",
        SsaCode::Map,
    );
}

#[test]
fn t_ssa_33_chan_在_hir_层就被拒() {
    // 实测：`make(chan int)` 在 lowering 就拒（`U-M5-05`，`HIR-UNSUPPORTED-CHAN`）
    // ⇒ `SSA-UNSUPPORTED-CHAN` 在本前端**没有入口**。守卫同上：
    // 上游一旦放行（比如 M8 开了 chan），这里会红，提醒回来接 SSA 侧的码。
    let p = fixture(
        "chan",
        "package main\n\nfunc main() {\n\tc := make(chan int)\n\t_ = c\n}\n",
    );
    let out = run(&p);
    assert_eq!(out.exit, 1, "chan 必须被拒：{out:?}");
    assert!(
        !out.stderr.contains("SSA-UNSUPPORTED-CHAN"),
        "这条码**不该**出现（HIR 更早拒），实际：{:?}",
        out.stderr
    );
    assert!(
        out.stderr.contains("HIR-UNSUPPORTED-CHAN"),
        "拒绝该来自 HIR 的 U-M5-05，实际：{:?}",
        out.stderr
    );
}

#[test]
fn t_ssa_33_立即调用的函数字面量() {
    // `func(){...}()`：被调方**本身**就是函数字面量。初版只扫实参表，
    // 漏掉被调方 ⇒ 落进 `expr(FuncLit)` 的 `todo!()`（panic）。
    // 实测 `closure1.go` / `ddd.go` / `func6.go`。
    assert_rejected(
        "funclit-call",
        "package main\n\nfunc main() {\n\tfunc() {\n\t\tprintln(1)\n\t}()\n}\n",
        SsaCode::FuncValue,
    );
}

#[test]
fn t_ssa_33_函数值() {
    assert_rejected(
        "funcvalue",
        "package main\n\nfunc main() {\n\tf := func() {}\n\tf()\n}\n",
        SsaCode::FuncValue,
    );
}

#[test]
fn t_ssa_33_range_over_string() {
    assert_rejected(
        "range-str",
        "package main\n\nfunc main() {\n\ts := \"ab\"\n\tfor _, r := range s {\n\t\tprintln(r)\n\t}\n}\n",
        SsaCode::RangeStr,
    );
}

// ── 两条「够不着」的码：守卫它们的上游闸门 ────────────────────────

/// 前端五步（装载 → 检查 → lowering），只给守卫测试用。
fn front_end(path: &Path) -> (rgoc_hir::Program, rgoc_types::TypeTable) {
    let mut tt = rgoc_types::TypeTable::new();
    let (pkg, bag) = rgoc_load::load_with_imports(&mut tt, &[path.to_path_buf()], &[], None);
    let opts = rgoc_check::CheckOptions { test_hooks: false };
    let (checked, lowered) =
        rgoc_check::check::check_package_with(&pkg, &mut tt, bag, &opts, |s| {
            rgoc_hir::lower(&pkg, s)
        });
    assert_eq!(
        checked.rendered_count(),
        0,
        "守卫 fixture 本身该过静态检查（它要走到 lowering / build 才被拒）"
    );
    (
        lowered.expect("守卫 fixture 该过 lowering（否则 HIR 那条闸门就够不着了）"),
        tt,
    )
}

#[test]
fn t_ssa_33_range_over_func_在_hir_层就被拒() {
    // `range` over func：HIR 的 `RangeKind` 没有 `Func` 变体 ⇒ lowering 就拒
    // （`U-M5-06`）。SSA 的 `-RANGE-FUNC` 因此**无输入**。
    // 这条守卫的意义：上游一旦放行，这里会红 —— 那时才轮到 SSA 侧的码。
    let src = "package main\n\nfunc yield(y func(int) bool) {\n\t_ = y(1)\n}\n\nfunc main() {\n\tfor i := range yield {\n\t\tprintln(i)\n\t}\n}\n";
    let p = fixture("range-func", src);
    let out = run(&p);
    assert_eq!(out.exit, 1, "range over func 必须被拒（不分层）：{out:?}");
    assert!(
        !out.stderr.contains("SSA-UNSUPPORTED-RANGE-FUNC"),
        "这条码**不该**出现：SSA 层看不到 range-over-func，实际：{:?}",
        out.stderr
    );
    assert!(
        out.stderr.contains("HIR-") || out.stderr.contains("[C8"),
        "拒绝该来自 HIR 或检查器，实际：{:?}",
        out.stderr
    );
}

#[test]
fn t_ssa_33_泛型在检查器就被拒() {
    // 泛型（`G-M5-03`）：检查器不放行 ⇒ SSA 的 `-GENERIC` 无输入。
    let src = "package main\n\nfunc F[T any](x T) T {\n\treturn x\n}\n\nfunc main() {\n\tprintln(F(1))\n}\n";
    let p = fixture("generic", src);
    let out = run(&p);
    assert_eq!(out.exit, 1, "泛型必须被拒：{out:?}");
    assert!(
        !out.stderr.contains("SSA-UNSUPPORTED-GENERIC"),
        "这条码**不该**出现（检查器更早拒）：{:?}",
        out.stderr
    );
}
