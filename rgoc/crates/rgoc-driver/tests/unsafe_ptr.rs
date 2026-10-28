//! `unsafe.Pointer` 作为**类型名**的端到端判据（`G-M9-01` 收口；`R-9-41`）。
//!
//! # 这条缺口是怎么来的
//!
//! M9 收口时发现 `func idp(p unsafe.Pointer) unsafe.Pointer` 构不过去，当时
//! 记成「帧布局失败」—— 复现后其实是**更早**的一步：`bin Ne 的两个操作数类型
//! 不同：invalid vs unsafe.Pointer`。真因是「类型表达式 → `TypeId`」在本仓有
//! **两份**实现，只有装载面那份认 `unsafe.Pointer`：
//!
//! | 位置 | 谁用它 | 认不认 |
//! |---|---|---|
//! | `rgoc-load::typeexpr::resolve_type` | 填 `Decl` 的类型（包级 `var` / 类型声明）| ✅ |
//! | `rgoc-check::expr::type_expr` | HIR lowering 填 `param_types` / `result_types` / 局部 | ❌（`_ => None`）|
//!
//! 于是同一个 `unsafe.Pointer` 在**声明位置**有真类型、在**函数签名里**是
//! `invalid`。这条判据钉的是「两处口径一致」：签名里的 `unsafe.Pointer` 必须
//! 是一个 8 字节、可比较、可打印的指针值。
//!
//! # 判据口径
//!
//! `native == 解释器（--via=ssa） == 手算`。手算那一边不能用「两边一致」代替 ——
//! 两边一起把 `unsafe.Pointer` 当 `void*` 传丢时对撞仍然绿。

use std::path::{Path, PathBuf};

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit, RunCommand, Sched, Via};

fn scratch() -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/unsafe-ptr");
    std::fs::create_dir_all(&d).expect("建目录失败");
    d
}

/// 编译成 ELF 再跑，返回 `(stdout, stderr, exit)`。
fn run_native(name: &str, src: &str) -> (String, String, Option<i32>) {
    let dir = scratch();
    let go = dir.join(format!("{name}.go"));
    std::fs::write(&go, src).expect("写 fixture 失败");
    let exe = dir.join(name);
    let b = BuildCommand {
        roots: Vec::new(),
        files: vec![go],
        output: Some(exe.clone()),
        emit: Emit::Elf,
        keep_asm: false,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let out = build_cmd::run_build(&b).expect("build 不该失败");
    assert_eq!(out.exit, 0, "{name} 构建失败：{}", out.stderr);
    let r = std::process::Command::new(&exe)
        .output()
        .expect("应能启动产物");
    (
        String::from_utf8_lossy(&r.stdout).into_owned(),
        String::from_utf8_lossy(&r.stderr).into_owned(),
        r.status.code(),
    )
}

/// 走 `--via=ssa`（求值器），返回 `(stdout, stderr, exit)`。
fn run_ssa(name: &str, src: &str) -> (String, String, Option<i32>) {
    let go = scratch().join(format!("{name}.ssa.go"));
    std::fs::write(&go, src).expect("写 fixture 失败");
    let cmd = RunCommand {
        roots: Vec::new(),
        files: vec![go],
        dump_hir: false,
        max_steps: None,
        lang: None,
        via: Via::Ssa,
        opt: rgoc_ssa::opt::Config::all(),
        sched: Sched::default(),
    };
    let o = rgoc_driver::run_cmd::run_run(&cmd).expect("解释器不该失败");
    (o.stdout, o.stderr, Some(i32::from(o.exit)))
}

/// `native == --via=ssa == want`，三条一起比。
fn expect_all(name: &str, src: &str, want: &str) {
    let (nout, nerr, ncode) = run_native(name, src);
    let (sout, serr, scode) = run_ssa(name, src);
    assert_eq!(
        ncode,
        Some(0),
        "{name}: native 退出码 {ncode:?}，stderr={nerr}"
    );
    assert_eq!(
        scode,
        Some(0),
        "{name}: 求值器退出码 {scode:?}，stderr={serr}"
    );
    assert_eq!(nout, want, "{name}: native 输出不符；stderr={nerr}");
    assert_eq!(sout, want, "{name}: 求值器输出不符；stderr={serr}");
}

// ── `G-M9-01`：签名里的 `unsafe.Pointer` ─────────────────────────────

#[test]
fn t_g_m9_01_unsafe_pointer_在形参与结果位置() {
    // 病灶形状：`unsafe.Pointer` **同时**出现在形参与结果上，且调用结果参与
    // 比较。修之前这一步报 `V3: bin Ne 的两个操作数类型不同：invalid vs
    // unsafe.Pointer`（结果是 `invalid`，因为签名 lowering 认不出它）。
    //
    // 判据里必须有「结果参与比较」这一条：只 `_ = idp(p)` 的话，`invalid`
    // 类型的 dst 没人读，`V3` 也不会开口。
    expect_all(
        "sig",
        r#"package main

import "unsafe"

func idp(p unsafe.Pointer) unsafe.Pointer { return p }

func main() {
	x := 7
	p := unsafe.Pointer(&x)
	q := idp(p)
	println(q != nil, q == p, p != nil)
}
"#,
        "true true true\n",
    );
}

#[test]
fn t_g_m9_01_unsafe_pointer_过内存仍是_8_字节指针() {
    // 签名只是第一关。第二条钉「它作为一个 8 字节指针值**经过内存**」：
    // 结构体字段（`Field` 按偏移读写）、`var` 的零值（必须清成 nil）、
    // 以及 `== nil`。如果后端把它当成 0 宽或当成长度 16 的 `string`，
    // 这里会读到邻居槽。
    //
    // ⚠️ **不打印地址本身**：那是运行期值，`go run` 与 `rgoc` 不可能一致；
    // 判据只比「是不是 nil」与「是不是同一个指针」。
    expect_all(
        "mem",
        r#"package main

import "unsafe"

type holder struct {
	n int
	p unsafe.Pointer
}

func main() {
	var h holder
	println(h.p == nil, h.n)
	x := 42
	h.p = unsafe.Pointer(&x)
	println(h.p != nil, h.n)
	var z unsafe.Pointer
	println(z == nil)
	z = h.p
	println(z == h.p)
}
"#,
        "true 0\ntrue 0\ntrue\ntrue\n",
    );
}
