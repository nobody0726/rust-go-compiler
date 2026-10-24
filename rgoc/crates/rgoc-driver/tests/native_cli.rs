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

use std::path::{Path, PathBuf};

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, CliError, Command, Emit, parse_args};

fn build(files: &[&str]) -> BuildCommand {
    BuildCommand {
        files: files.iter().map(PathBuf::from).collect(),
        output: None,
        emit: Emit::Elf,
        keep_asm: false,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
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
        lang: None,
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

/// 把一段 Go 源写到 `target/native-cli/<name>.go`。
fn write_src(name: &str, src: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/native-cli");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{name}.go"));
    std::fs::write(&p, src).unwrap();
    p
}

#[test]
fn t_nat_47_emit_asm_只出汇编() {
    let go = write_src("asm_only", "package main\nfunc main() { println(1) }\n");
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/native-cli/asm_only.s");
    let cmd = BuildCommand {
        files: vec![go],
        output: Some(out.clone()),
        emit: Emit::Asm,
        keep_asm: false,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let r = build_cmd::run_build(&cmd).unwrap();
    assert_eq!(r.exit, 0, "stderr={}", r.stderr);
    assert_eq!(r.artifact.as_deref(), Some(out.as_path()));
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains(".text"), "应是汇编文本");
    assert!(text.contains("bl rt_print_i64"), "println(1) 应调 runtime");
    assert!(!out.with_extension("").exists(), "--emit=asm 不该出 ELF");
}

#[test]
fn t_nat_47_默认出_elf_且不保留汇编() {
    let go = write_src("elf_default", "package main\nfunc main() { println(7) }\n");
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/native-cli/elf_default");
    let cmd = BuildCommand {
        files: vec![go],
        output: Some(out.clone()),
        emit: Emit::Elf,
        keep_asm: false,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let r = build_cmd::run_build(&cmd).unwrap();
    assert_eq!(r.exit, 0, "stderr={}", r.stderr);
    assert!(out.exists(), "应出 ELF");
    assert!(!out.with_extension("s").exists(), "默认不保留 .s");

    // 真跑一次：`println(7)` ⇒ `7\n`
    let run = std::process::Command::new(&out).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n");
    assert_eq!(run.status.code(), Some(0));
}

#[test]
fn t_nat_47_keep_asm_保留中间件() {
    let go = write_src("keep_asm", "package main\nfunc main() {}\n");
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/native-cli/keep_asm");
    let cmd = BuildCommand {
        files: vec![go],
        output: Some(out.clone()),
        emit: Emit::Elf,
        keep_asm: true,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let r = build_cmd::run_build(&cmd).unwrap();
    assert_eq!(r.exit, 0, "stderr={}", r.stderr);
    assert!(out.with_extension("s").exists(), "--keep-asm 应留下 .s");
}
