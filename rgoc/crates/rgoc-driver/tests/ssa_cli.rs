//! `ssa` 子命令的**参数与通道**契约（M6/Task 2 的骨架版，`T-SSA-38` 的前半）。
//!
//! # 这一版测什么、不测什么
//!
//! Task 2 只落「参数解析 + 三条通道」这层皮：IR 管线要到 Task 7 起才有东西可 dump。
//! 所以这里**不测 dump 内容**，只测两件在骨架期就能钉死的事：
//!
//! 1. **参数面**：`ssa` 必须有文件；`--dump` / `--verify` 至少给一个。
//! 2. **通道面**：任何错误都不许写 stdout（否则 `run-collide` 分不清
//!    「程序的字节」与「编译器的抱怨」）。
//!
//! # 为什么骨架期就要有「不假装支持」的判据
//!
//! `T37` 的纪律：**未实现的子命令必须报错，不许悄悄接受**。`ssa` 在 Task 2 时
//! 确实还没有 IR —— 那就必须**响亮地**说「尚未接入」，而不是打一个空 dump
//! 让人以为跑通了。这条断言在 Task 16 会被换成「dump 有内容」，但**在此之前**
//! 它是防止「假绿」的唯一一道闸。

use std::path::{Path, PathBuf};

use rgoc_driver::cli::{CliError, Command, SsaCommand, parse_args};
use rgoc_driver::ssa_cmd;

/// 仓库根的语料文件（`CARGO_MANIFEST_DIR` 是 `rgoc/crates/rgoc-driver`）。
fn corpus_file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../go_source_code/test/helloworld.go")
}

fn ssa(files: &[&str], dump: bool, verify: bool) -> SsaCommand {
    SsaCommand {
        files: files.iter().map(PathBuf::from).collect(),
        dump,
        verify,
    }
}

#[test]
fn t_ssa_38_解析_ssa_dump_与_verify() {
    assert_eq!(
        parse_args(&["ssa", "--dump", "a.go"]).unwrap(),
        Command::Ssa(ssa(&["a.go"], true, false)),
        "--dump 只置 dump"
    );
    assert_eq!(
        parse_args(&["ssa", "--verify", "a.go", "b.go"]).unwrap(),
        Command::Ssa(ssa(&["a.go", "b.go"], false, true)),
        "--verify 收多文件"
    );
    assert_eq!(
        parse_args(&["ssa", "--dump", "--verify", "a.go"]).unwrap(),
        Command::Ssa(ssa(&["a.go"], true, true)),
        "两个旗标可同时给（Task 38 要求两者都输出）"
    );
}

#[test]
fn t_ssa_38_解析_ssa_缺文件或缺模式必须报错() {
    assert_eq!(
        parse_args(&["ssa"]).unwrap_err(),
        CliError::SsaNeedsFile,
        "没有文件 ⇒ 报错（不静默跑 0 个）"
    );
    assert_eq!(
        parse_args(&["ssa", "a.go"]).unwrap_err(),
        CliError::SsaNeedsMode,
        "既没 --dump 也没 --verify ⇒ 报错（不猜用户要哪个）"
    );
}

#[test]
fn t_ssa_38_骨架_文件不可读时走_err() {
    // `Err` ⇒ `main_entry` 打 stderr + 退出码 2，**绝不写 stdout**
    let err = ssa_cmd::run_ssa(&ssa(&["/nonexistent/rgoc-m6.go"], true, false)).unwrap_err();
    assert!(
        err.contains("读"),
        "错误消息要说明是「读文件」失败，实际：{err:?}"
    );
}

#[test]
fn t_ssa_31_接上管线之后_dump_有内容且通道分开() {
    // Task 31 把 `ssa` 从骨架接到真管线：dump 走 stdout、错误走 stderr、退出码 0。
    let file = corpus_file();
    let out = ssa_cmd::run_ssa(&ssa(&[file.to_str().unwrap()], true, true)).unwrap();
    assert_eq!(out.exit, 0, "helloworld 该建图成功并过 verify：{out:?}");
    assert!(
        out.stdout.contains("# stage: ssa"),
        "dump 必须在 stdout 且带版本头，实际：{:?}",
        out.stdout
    );
    assert!(
        out.stdout.contains("# func main"),
        "dump 里要有 main，实际：{:?}",
        out.stdout
    );
    assert!(
        out.stderr.is_empty(),
        "没有错误时 stderr 必须为空，实际：{:?}",
        out.stderr
    );
}
