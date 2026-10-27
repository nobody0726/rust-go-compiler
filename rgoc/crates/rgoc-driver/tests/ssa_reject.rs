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
//! # 为什么只剩一条「够不着」的码
//!
//! `SSA-UNSUPPORTED-GENERIC` 在**这个前端**里没有入口：泛型被检查器挡住
//! （`G-M5-03`）。它有一个「守卫测试」：断言它**在检查器层就被拒**
//! （`C8` 诊断）—— 一旦哪天上游开始放行，守卫就会红，提醒我们回来实现
//! SSA 侧的码，而不是留一个永远走不到的 `todo!`。
//!
//! 曾经还有一条 `-RANGE-FUNC`，M8/Task 45 把它下线了：HIR 放行 range over
//! func 之后下降成「调用 + 合成函数字面量」，SSA 的拒绝入口换成
//! `-FUNC-VALUE`（见 `t_ssa_33_range_over_func_改走_func_value`）。

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
        opt: rgoc_ssa::opt::Config::all(),
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
    // M9/Phase 4b 之前：闭包（`Func.captured` 非空）必须被拒
    // （`SSA-UNSUPPORTED-CLOSURE`）。现在捕获按**引用**实现（funcval 存指向
    // 变量存储的指针，`R-9-15`）⇒ 判据反转成「能建图 + 退出码 0」，
    // 那个码**整条下线**（`R-9-16`）。
    let p = fixture(
        "closure",
        "package main\n\nfunc main() {\n\tx := 1\n\tf := func() int { x++; return x }\n\tprintln(f(), x)\n}\n",
    );
    let out = run(&p);
    assert_eq!(
        out.exit, 0,
        "闭包已在 M9/Phase 4b 支持，该退出码 0；stderr = {:?}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("SSA-UNSUPPORTED-CLOSURE"),
        "`SSA-UNSUPPORTED-CLOSURE` 已在 R-9-16 下线：{:?}",
        out.stderr
    );
}

#[test]
fn t_ssa_33_defer() {
    // M9/Phase 5b 之前：`defer` 一律被拒（`SSA-UNSUPPORTED-DEFER`）。现在实参
    // 打包进合成的包装函数（`synth_defer_wrapper`）⇒ 普通形态能建图，判据
    // **反转**成「能建图 + 那个词不出现」。
    //
    // ⚠️ 码**没有**整条下线：`defer delete/recover` 仍是拒绝面（见下一条）。
    // 「没下线」和「没实现」是两件事，判据必须分开写 —— 合成一条就等于给
    // 留着的死分支盖章。
    let p = fixture(
        "defer",
        "package main\n\nfunc main() {\n\tdefer println(1)\n\tprintln(2)\n}\n",
    );
    let out = run(&p);
    assert_eq!(
        out.exit, 0,
        "普通 defer 已在 M9/Phase 5b 支持，该退出码 0；stderr = {:?}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("SSA-UNSUPPORTED-DEFER"),
        "普通 defer 不该再被拒：{:?}",
        out.stderr
    );
}

#[test]
fn t_ssa_33_defer_内建仍拒() {
    // `SsaCode::Defer` 今天的**唯一**生产者：`defer` 的两个内建形态。
    // `delete` 走的是 runtime 的 map 助手（延迟调用要自己造一个 funcval，
    // 而 `rt_map_del` 的签名不是 `func()`）；`defer recover()` 只会把
    // `recover()` 当场求值再延迟一个空操作 —— 没有实现的必要。
    assert_rejected(
        "defer-delete",
        "package main\n\nfunc main() {\n\tm := map[string]int{}\n\tdefer delete(m, \"a\")\n}\n",
        SsaCode::Defer,
    );
}

#[test]
fn t_ssa_33_recover() {
    // M9/Phase 5a 之前 `recover()` 被拒（与 defer 共用那个码）。recover 实现了
    // （`CalleeRef::Runtime(Recover)` + 帧上的 defer 链）⇒ 判据反转成「能建图」。
    let p = fixture(
        "recover",
        "package main\n\nfunc f() any {\n\treturn recover()\n}\n\nfunc main() {}\n",
    );
    let out = run(&p);
    assert_eq!(
        out.exit, 0,
        "recover 已在 M9/Phase 5a 支持，该退出码 0；stderr = {:?}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("SSA-UNSUPPORTED-DEFER"),
        "recover 不该再被拒：{:?}",
        out.stderr
    );
}

#[test]
fn t_ssa_33_接口动态派发() {
    // M9/Phase 3/4 之前：接口派发、类型断言、type switch 都要被拒
    // （`SSA-UNSUPPORTED-IFACE-DYN`）。派发（Task 32）、断言（Task 33）与
    // type switch（Task 34）都实现了 ⇒ 判据**反转**成「能建图 + 退出码 0」，
    // 而那个码**整条下线**（`R-9-14`：不留永远不产出的码）。
    //
    // 这里守两件事：① 四种形态都能过 verifier；② `SSA-UNSUPPORTED-IFACE-DYN`
    // 这个词**不再出现**（下线的码不许偷偷回来当「兜底」）。
    let cases = [
        (
            "iface-dispatch",
            "package main\n\ntype I interface{ M() int }\n\ntype T struct{}\n\nfunc (T) M() int { return 1 }\n\nfunc main() {\n\tvar i I = T{}\n\tprintln(i.M())\n}\n",
        ),
        (
            "iface-assert",
            "package main\n\nfunc main() {\n\tvar a any = 42\n\tprintln(a.(int))\n}\n",
        ),
        (
            "iface-assert-ok",
            "package main\n\nfunc main() {\n\tvar a any = 42\n\tv, ok := a.(int)\n\tprintln(v, ok)\n}\n",
        ),
        (
            "iface-typeswitch",
            "package main\n\nfunc main() {\n\tvar x any = 1\n\tswitch x.(type) {\n\tcase int:\n\t\tprintln(1)\n\t}\n}\n",
        ),
    ];
    for (name, src) in cases {
        let p = fixture(name, src);
        let out = run(&p);
        assert_eq!(
            out.exit, 0,
            "{name}: 接口面已在 M9/Phase 3 支持，该退出码 0；stderr = {:?}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("SSA-UNSUPPORTED-IFACE-DYN"),
            "{name}: `SSA-UNSUPPORTED-IFACE-DYN` 已在 R-9-14 下线，不该再出现：{:?}",
            out.stderr
        );
    }
}

#[test]
fn t_ssa_33_map() {
    // M9/Phase 2 之前这里是「map 必须被拒」（`SsaCode::Map`）。map 实现了
    // （`rt_map_*` + `CalleeRef::Runtime`，`D-M9-5`），所以这条判据**反过来**：
    // 断言它今天**不再**被拒，并且 `SSA-UNSUPPORTED-MAP` 这个词不再出现。
    //
    // 为什么留一条反向判据而不是删掉：删掉之后没人守「map 真的在面内」——
    // 哪天 `face_kind` 又被谁加回一条 map 分支，这条会红。
    let p = fixture(
        "map",
        "package main\n\nfunc main() {\n\tm := map[string]int{\"a\": 1}\n\tprintln(m[\"a\"])\n}\n",
    );
    let out = run(&p);
    assert_eq!(
        out.exit, 0,
        "map 已在 M9/Phase 2 支持，该退出码 0；stderr = {:?}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("SSA-UNSUPPORTED-MAP"),
        "`SSA-UNSUPPORTED-MAP` 已无入口，不该再出现：{:?}",
        out.stderr
    );
    // 结构断言：建图结果里**没有** Map 拒绝
    let (program, mut tt) = front_end(&p);
    let built = rgoc_ssa::build::build(&program, &mut tt);
    assert!(
        !built.rejected.iter().any(|r| r.code == SsaCode::Map),
        "rejected 里不该有 Map：{:?}",
        built.rejected
    );
}

#[test]
fn t_ssa_33_chan_现在由_ssa_层拒() {
    // 这条判据的**前身**是「chan 在 HIR 层就被拒，所以 `SSA-UNSUPPORTED-CHAN`
    // 没有入口」—— 它当时是一条守卫：上游一旦放行就会红。
    // M8/Task 14 正是那次放行（`HIR-UNSUPPORTED-CHAN` 已下线），所以这里改成
    // 判「入口打开了、SSA 侧的码真的接住了」：SSA 不实现并发（`M8-design.md` §1.3）。
    // ⚠️ fixture 换过：`c := make(chan int); _ = c` 在 M11b/Task 16/19 之后
    // **能编译了**（`make(chan)` 分配、chan 值是普通指针）。换成 `close(c)`
    // —— 它仍然被拒（`BuiltinKind::Close`，Task 20 才做）。
    let p = fixture(
        "chan",
        "package main\n\nfunc main() {\n\tc := make(chan int)\n\tclose(c)\n}\n",
    );
    let out = run(&p);
    assert_eq!(out.exit, 1, "chan 在 SSA 面必须被拒：{out:?}");
    assert!(
        out.stderr.contains("SSA-UNSUPPORTED-CHAN"),
        "拒绝该来自 SSA 侧（HIR 已放行），实际：{:?}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("HIR-UNSUPPORTED-CHAN"),
        "`HIR-UNSUPPORTED-CHAN` 已下线，不该再出现：{:?}",
        out.stderr
    );
}

#[test]
fn t_ssa_33_立即调用的函数字面量() {
    // `func(){...}()`：被调方**本身**就是函数字面量。初版只扫实参表，
    // 漏掉被调方 ⇒ 落进 `expr(FuncLit)` 的 `todo!()`（panic）。实测 `func6.go`。
    //
    // M9/Phase 4a 起无捕获的函数字面量**能建图**（`Instr::FuncAddr` +
    // 间接调用），所以这条判据**反转**：断言它能过 verifier、退出码 0。
    // 「被调方也要扫」这条纪律仍然有效 —— 它现在由 `t_ssa_33_闭包`（捕获闭包
    // 仍被拒）守着。
    let p = fixture(
        "funclit-call",
        "package main\n\nfunc main() {\n\tfunc() {\n\t\tprintln(1)\n\t}()\n}\n",
    );
    let out = run(&p);
    assert_eq!(
        out.exit, 0,
        "无捕获的函数字面量已在 M9/Phase 4a 支持，该退出码 0；stderr = {:?}",
        out.stderr
    );
}

#[test]
fn t_ssa_33_函数值() {
    // 无捕获的函数值（具名函数当值 / 空字面量）在 M9/Phase 4a 放行；
    // **有捕获**的仍拒（`t_ssa_33_闭包` 守它）。这里守放行那一半。
    let p = fixture(
        "funcvalue",
        "package main\n\nfunc main() {\n\tf := func() {}\n\tf()\n}\n",
    );
    let out = run(&p);
    assert_eq!(
        out.exit, 0,
        "无捕获的函数值已支持，该退出码 0；stderr = {:?}",
        out.stderr
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
fn t_ssa_33_range_over_func_现在能跑了() {
    // M8/Task 45 起 HIR 真的放行 range over func：它下降成
    // `seq(__range_body_0)` —— 一次调用 + 一个合成函数字面量。
    // 所以 SSA 看到的**不是** range，而是 `FUNC-VALUE`（IR 里没有函数引用值）。
    //
    // 这条守卫的前身是「range over func 在 HIR 层就被拒」。上游放行后它如期
    // 变红，提醒我们回来接 SSA 侧 —— 接完的结论是：SSA 层拒绝它的**入口**
    // 从 `RANGE-FUNC` 换成了 `FUNC-VALUE`，那个码本来就在 `ALL` 里。
    // 而且 M9/Phase 4a 之后它**真的能跑**了：HIR 把它下降成
    // `seq(__range_body_0)` —— 一次调用 + 一个**无捕获**的函数字面量，
    // 正好落在新放行的面上。这条判据因此从「拒绝」翻成「能建图」。
    //
    // ⚠️ 体里**捕获外层变量**的 range-over-func 仍被拒（合成函数带捕获 ⇒
    // `SSA-UNSUPPORTED-FUNC-VALUE`），那是 Task 38 的面。
    let src = "package main\n\nfunc seq(yield func(int) bool) {\n\t_ = yield(1)\n}\n\nfunc main() {\n\tfor i := range seq {\n\t\tprintln(i)\n\t}\n}\n";
    let p = fixture("range-func", src);
    let out = run(&p);
    assert_eq!(
        out.exit, 0,
        "range over func 现在能建图（无捕获的合成函数）：{out:?}"
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
