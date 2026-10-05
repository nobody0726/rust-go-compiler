//! `xtask` —— 构建期工具（**不是**编译器本体，也不是测试运行器）。
//!
//! # 三个职责（T37 冻结）
//!
//! 1. [`corpus`] —— 语料枚举（全量 279，产出基线报告的数据）
//! 2. [`report`] —— 报告生成（跑 20 个基线样本，产出 E4 的报告）
//! 3. [`manifest`] —— 环境 manifest 生成（把 `environment` 节从手填改为可重放）
//!
//! # 与 `rgoc-driver` 的分工
//!
//! | | `rgoc-driver` | `xtask` |
//! |---|---|---|
//! | 面向 | 人 / 门禁脚本 | 构建期 / 开发期 |
//! | 入口 | `rgoc-driver harness …` | `cargo run -p xtask -- …` |
//! | 判定 | 调 `harness::run_layer` | **不判定**，转发 driver |
//! | 用途 | 跑样本、出报告（门禁路径）| 枚举语料、刷新 manifest |
//!
//! **判定逻辑只有一处**（`rgoc_harness::runner::run_layer`）。driver 与 xtask 都只是
//! 调用方 —— 两处各写一份判定，E4 门禁就会变成「两份报告说过了」而不是「harness 判过了」。
//!
//! 用法：
//!
//! ```sh
//! cargo run -p xtask -- corpus                    # 枚举语料（分母应为 279）
//! cargo run -p xtask -- report --out <路径>        # 跑 20 样本，JSON 落盘
//! cargo run -p xtask -- manifest [--check]         # 生成 environment 节
//! ```

pub mod corpus;
pub mod manifest;
pub mod report;

use std::path::PathBuf;
use std::process::ExitCode;

/// `xtask <子命令>`。
const SUBCOMMANDS: [&str; 3] = ["corpus", "report", "manifest"];

/// 二进制入口。退出码：0 成功，1 用法错，2 执行失败。
pub fn main_entry() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(sub) = argv.first().map(String::as_str) else {
        eprintln!("{USAGE}");
        return ExitCode::from(1);
    };
    if !SUBCOMMANDS.contains(&sub) {
        eprintln!("xtask: 未知子命令 {sub:?}\n{USAGE}");
        return ExitCode::from(1);
    }
    let rest = &argv[1..];
    let r = match sub {
        "corpus" => cmd_corpus(rest),
        "report" => cmd_report(rest),
        "manifest" => cmd_manifest(rest),
        _ => unreachable!("上面的 contains 已挡住"),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask {sub}: {e}");
            ExitCode::from(2)
        }
    }
}

const USAGE: &str = "用法：\n  \
     cargo run -p xtask -- corpus\n  \
     cargo run -p xtask -- report --out <路径>\n  \
     cargo run -p xtask -- manifest [--check]";

fn cmd_corpus(_rest: &[String]) -> Result<(), String> {
    let dir = report::corpus_test_dir();
    let v = corpus::enumerate_json(&dir).map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn cmd_report(rest: &[String]) -> Result<(), String> {
    let Some(i) = rest.iter().position(|a| a == "--out") else {
        return Err("report 需要 --out <路径>（报告必须落盘才可重放比对）".into());
    };
    let Some(p) = rest.get(i + 1) else {
        return Err("--out 缺值".into());
    };
    let out = PathBuf::from(p);
    let text = report::write_report(&out)?;
    print!("{text}");
    eprintln!("JSON 报告已写入 {}", out.display());
    Ok(())
}

fn cmd_manifest(rest: &[String]) -> Result<(), String> {
    let v = manifest::environment_json();
    if rest.iter().any(|a| a == "--check") {
        // --check：只打印，不写盘。用来在容器里核对「现在测到的是什么」，
        // 而不改动入库文件（改不改动是人的决定）。
        println!(
            "{}",
            serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    let m = manifest::repo_manifest_path();
    manifest::update_manifest(&m)?;
    eprintln!("已更新 {} 的 environment 节", m.display());
    Ok(())
}
