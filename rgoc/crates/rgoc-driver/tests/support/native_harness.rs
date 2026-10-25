#![allow(dead_code)]
//! 注：`mod support;` 被多个测试二进制各自编译一份，不是每个都用得到全部辅助函数。
//! 对共享装置开 `dead_code` 是**刻意**的 —— 否则每加一个辅助函数，用不到它的
//! 测试二进制就会因为 `-D warnings` 编译失败。

//! native 装置的公共部分：**Go 源 → 产物 → 三通道**（`M7-plan.md` Task 4）。
//!
//! # 三条纪律
//!
//! 1. **不写 `go_source_code/`**：fixture 一律落到 `target/native-fixtures/<name>/`。
//!    语料目录是只读的（`T-NAT-56` 会校验 hash），装置写进去就等于污染判据。
//! 2. **不在装置里重实现编译器**：这里只调 `build_cmd::run_build`（与 CLI 同一条路）。
//!    装置自己拼一条流水线的话，「测试通过」就不再证明「`rgoc build` 能跑」。
//! 3. **失败要带上下文**：返回的错误里必须能看出是「哪一步」失败（写文件 / 编译 / 运行）。
//!
//! # 当前状态（Task 4）
//!
//! `build_cmd::run_build` 还没有产物（Task 50 接线）⇒ `build_and_run` 会返回
//! `Err("… 尚未实现 …")`。依赖它的 smoke 测试因此都带 `#[ignore]`，
//! 并在 Task 50 去掉 —— 用「静默通过」代替「明确跳过」是本仓反复禁止的假绿。

use std::path::{Path, PathBuf};
use std::process::Command;

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit};

/// 一次「编译 + 运行」的观测结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// 程序的 stdout（不是编译器的）。
    pub stdout: String,
    /// 程序的 stderr。
    pub stderr: String,
    /// 程序的退出码（被信号杀掉时为 `-1`）。
    pub exit: i32,
    /// 生成物里的 `.s` 文本（`--keep-asm` 打开时总有）。
    pub asm: String,
}

/// fixture 的落地根目录（`target/` 下，随 `cargo clean` 一起消失）。
#[must_use]
pub fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("target/native-fixtures")
}

/// 把 `src` 写成 `<name>/main.go`，编译成 `<name>/prog`，跑一次。
///
/// # Errors
///
/// - 写文件失败
/// - `run_build` 返回 `Err`（参数非法 / IO 失败 / 尚未实现）
/// - 编译成功但退出码非 0（静态拒绝或工具链失败）—— 这时返回 `Err` 带上 stderr，
///   因为「没跑起来」不是「跑出来的结果」
/// - 产物路径缺失
pub fn build_and_run(name: &str, src: &str) -> Result<Outcome, String> {
    let dir = fixture_root().join(name);
    std::fs::create_dir_all(&dir).map_err(|e| format!("建 {} 失败：{e}", dir.display()))?;
    let go = dir.join("main.go");
    std::fs::write(&go, src).map_err(|e| format!("写 {} 失败：{e}", go.display()))?;

    let prog = dir.join("prog");
    let cmd = BuildCommand {
        files: vec![go.clone()],
        output: Some(prog.clone()),
        emit: Emit::Elf,
        keep_asm: true,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let out = build_cmd::run_build(&cmd)?;
    if out.exit != 0 {
        return Err(format!(
            "build 失败（exit={}）：\nstderr: {}\nstdout: {}",
            out.exit, out.stderr, out.stdout
        ));
    }
    let artifact = out
        .artifact
        .ok_or_else(|| "build 成功但没有产物路径".to_string())?;

    let asm_path = prog.with_extension("s");
    let asm = std::fs::read_to_string(&asm_path).map_err(|e| {
        format!(
            "读 {} 失败（--keep-asm 应当留下它）：{e}",
            asm_path.display()
        )
    })?;

    let run = Command::new(&artifact)
        .output()
        .map_err(|e| format!("运行 {} 失败：{e}", artifact.display()))?;

    Ok(Outcome {
        stdout: String::from_utf8_lossy(&run.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&run.stderr).into_owned(),
        exit: run.status.code().unwrap_or(-1),
        asm,
    })
}

/// 用**解释器**跑同一段源码，返回 stdout（`--via=hir` 的等价物）。
///
/// # 为什么走 `run_cmd` 而不是再实现一遍
///
/// 解释器就是「另一条通路」的基准。自己拼装载/检查/求值等于再造一个基准 ——
/// 那样两边一起错时，对撞仍然绿。
///
/// # Errors
///
/// 写文件失败，或解释器返回 `Err` / 非零退出码。
pub fn hir_output(name: &str, src: &str) -> Result<String, String> {
    let dir = fixture_root().join(format!("{name}-hir"));
    std::fs::create_dir_all(&dir).map_err(|e| format!("建 {} 失败：{e}", dir.display()))?;
    let go = dir.join("main.go");
    std::fs::write(&go, src).map_err(|e| format!("写 {} 失败：{e}", go.display()))?;
    let cmd = rgoc_driver::cli::RunCommand {
        files: vec![go],
        dump_hir: false,
        max_steps: None,
        lang: None,
        via: rgoc_driver::cli::Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
        sched: rgoc_driver::cli::Sched::default(),
    };
    let out = rgoc_driver::run_cmd::run_run(&cmd)?;
    if out.exit != 0 {
        return Err(format!("解释器退出码 {}：{}", out.exit, out.stderr));
    }
    Ok(out.stdout)
}
