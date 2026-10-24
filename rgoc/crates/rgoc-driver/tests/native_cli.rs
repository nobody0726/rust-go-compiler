//! `build` 子命令的**参数面**与「不假装支持」判据（M7/Task 2 的骨架版，`T-NAT-47` 前半）。
//!
//! # 这一版测什么、不测什么
//!
//! Task 2 只落「参数解析 + 三通道形状」这层皮；产物要到 Task 50 才接。
//! 所以这里**不测产物**，只测三件在骨架期就能钉死的事：
//!
//! 1. **参数面**：`build` 必须有文件；`-o` / `--emit` / `--keep-asm` / `--opt` 都能解析，
//!    且 `--emit=x` 与 `--emit x` 两种写法等价。
//! 2. **拒绝面**：`--emit` 的非法值必须**报错**而不是静默回落到默认值
//!    （静默回落 = 用户以为出了 `.s`，实际出的是 ELF）。
//! 3. **不假装支持**：Task 2 时 `build` 还没有产物 ⇒ `run_build` 必须返回 `Err`。
//!    这条断言在 Task 50 会被换成「产物真的存在」，**在此之前**它是防「假绿」的唯一闸。

use std::path::PathBuf;

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, CliError, Command, Emit, parse_args};

fn build(files: &[&str]) -> BuildCommand {
    BuildCommand {
        files: files.iter().map(PathBuf::from).collect(),
        output: None,
        emit: Emit::Elf,
        keep_asm: false,
        opt: rgoc_ssa::opt::Config::all(),
    }
}

#[test]
fn 解析_build_的默认形态() {
    assert_eq!(
        parse_args(&["build", "a.go"]).unwrap(),
        Command::Build(build(&["a.go"])),
        "缺省 emit=elf、不保留 .s、无 -o"
    );
}

#[test]
fn 解析_全部旗标() {
    let got = parse_args(&[
        "build",
        "-o",
        "out/prog",
        "--emit",
        "asm",
        "--keep-asm",
        "--opt=none",
        "a.go",
        "b.go",
    ])
    .unwrap();
    let want = BuildCommand {
        files: vec![PathBuf::from("a.go"), PathBuf::from("b.go")],
        output: Some(PathBuf::from("out/prog")),
        emit: Emit::Asm,
        keep_asm: true,
        opt: rgoc_ssa::opt::Config::none(),
    };
    assert_eq!(got, Command::Build(want));
}

#[test]
fn emit_两种写法等价() {
    let a = parse_args(&["build", "--emit=asm", "a.go"]).unwrap();
    let b = parse_args(&["build", "--emit", "asm", "a.go"]).unwrap();
    assert_eq!(a, b, "`--emit=asm` 与 `--emit asm` 必须同义");
}

#[test]
fn 缺文件参数报_build_needs_file() {
    assert_eq!(
        parse_args(&["build"]).unwrap_err(),
        CliError::BuildNeedsFile,
        "一个文件都不给必须报错（静默跑 0 个文件会被误读成「成功」）"
    );
}

#[test]
fn 非法_emit_值必须报错() {
    assert_eq!(
        parse_args(&["build", "--emit=llvm", "a.go"]).unwrap_err(),
        CliError::BadEmit("llvm".to_string()),
        "不认识的 emit 不许静默回落到 elf"
    );
    assert_eq!(
        parse_args(&["build", "--emit", "obj", "a.go"]).unwrap_err(),
        CliError::BadEmit("obj".to_string())
    );
}

#[test]
fn 未知选项报错() {
    assert_eq!(
        parse_args(&["build", "--frobnicate", "a.go"]).unwrap_err(),
        CliError::UnknownFlag("--frobnicate".to_string())
    );
}

#[test]
fn t_nat_47_骨架期_build_必须响亮地说未实现() {
    // Task 2 的判据：**命令存在但没有产物** 时，必须 Err（→ 退出码 2），
    // 不许返回 `Ok(exit: 0)` 让 `run-collide --native` 变成恒绿的空话。
    let err = build_cmd::run_build(&build(&["a.go"])).unwrap_err();
    assert!(
        err.contains("尚未实现"),
        "错误信息要说清是「还没接」，实际：{err}"
    );
}
