//! `rgoc-driver` —— rgoc 的**统一 CLI 入口**。
//!
//! # 职责边界（T37）
//!
//! 本 crate 只做**编排**：把命令行参数翻译成对 [`rgoc_harness`] 的调用，再把结果
//! 渲染成人读文本与机器读 JSON。它**不含任何判定逻辑** ——
//! 「这个用例算不算通过」全部由 harness 的 `runner` / `compare` 决定。
//!
//! 这条边界很重要：driver 一旦自己开始判语义，E4 门禁就变成「driver 说过了」，
//! 而不是「harness 判过了」，两层混在一起再也分不开。
//!
//! # M0 的形状
//!
//! 顶层**只有** `harness` 一个子命令，三个动作：
//!
//! ```text
//! rgoc-driver harness list   [--all]              列出样本
//! rgoc-driver harness run    --all | <T-C-nn>...  跑样本
//! rgoc-driver harness report [--out <路径>]       跑完出报告
//! ```
//!
//! **不预留**未实现的子命令（见 [`cli`] 模块头的说明）。三个架构 spike 属
//! M0 Phase 3（T40–T47），到时再加。

pub mod cli;
pub mod report;
pub mod samples;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rgoc_harness::corpus::CorpusConfig;
use rgoc_harness::ir::Layer;
use rgoc_harness::oracle::OracleConfig;
use rgoc_harness::runner::run_layer;

use cli::{Command, HarnessCommand};
use report::{RunSummary, render_text, summarize, to_json};
use samples::{CORPUS_M0_SAMPLES, LoadedSamples, SampleEntry, index_of};

/// 语料目录（`go_source_code/test`）。按 crate 位置推算，可用 `RGOC_CORPUS_TEST_DIR` 覆盖。
pub fn corpus_test_dir() -> PathBuf {
    if let Ok(d) = std::env::var("RGOC_CORPUS_TEST_DIR") {
        return PathBuf::from(d);
    }
    // CARGO_MANIFEST_DIR = <repo>/rgoc/crates/rgoc-driver
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../go_source_code/test")
        .to_path_buf()
}

/// M0 的 oracle 配置：`work_dir` 指向**只读**语料，预算取冻结值。
///
/// ⚠️ `work_dir` 必须是语料目录：`replace_prefix`（R6 细节 3）靠它把诊断里的
/// 全路径换回短文件名，而 `compare` 层的期望值也按短名组织。
fn oracle_config(corpus: &Path) -> OracleConfig {
    OracleConfig {
        work_dir: corpus.to_path_buf(),
        ..OracleConfig::from(CorpusConfig::m0())
    }
}

/// 跑一组样本并出汇总。**真跑 oracle**（容器内 go1.27.1）。
fn execute(entries: &[SampleEntry]) -> Result<RunSummary, String> {
    let dir = corpus_test_dir();
    let loaded = LoadedSamples::load(&dir, entries).map_err(|e| e.to_string())?;
    if loaded.is_empty() {
        return Err("没有可跑的样本 —— 静默跑 0 条会被误读成「全部通过」".into());
    }
    let specs = loaded.specs(Layer::Corpus);
    let rep = run_layer(&specs, &oracle_config(&dir), &CorpusConfig::m0());
    Ok(summarize(&rep))
}

/// `harness list` 的文本输出。
fn render_list(all: bool) -> String {
    let mut w = String::new();
    let _ = writeln!(w, "M0 官方语料基线样本（E4 门禁，分母 = 20）");
    let _ = writeln!(w, "──────────────────────────────────────────────");
    // ⚠️ 表头里 **不许把「有 .out」当格式串的一部分**（写成 `{:<12} 有 .out` 会让
    // clippy::write_literal 报错：`有 .out` 被当成没有占位符的「多余参数」）。
    // 它必须作为一个**参数**传进去。
    let _ = writeln!(w, "{:<10} {:<14} {:<12} .out", "ID", "文件", "模式");
    for s in CORPUS_M0_SAMPLES.iter() {
        // `.out` 只看语料在不在，不读内容 —— 装载时的校验由 LoadedSamples 负责
        let has_out = corpus_test_dir()
            .join(s.path)
            .with_extension("out")
            .is_file();
        let _ = writeln!(
            w,
            "{:<10} {:<14} {:<12} {}",
            s.id,
            s.path,
            s.mode.name(),
            if has_out { "是" } else { "—" }
        );
    }
    let _ = writeln!(w, "──────────────────────────────────────────────");
    if all {
        let _ = writeln!(
            w,
            "（--all 已给出；M0 的分母就是这 20 条，分母外的用例属 U1–U14 排除项）"
        );
    }
    w
}

/// 二进制入口（`main.rs` 只调它）。返回值即进程退出码。
pub fn main_entry() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
    let cmd = match cli::parse_args(&argv) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rgoc-driver: {e}");
            return ExitCode::from(2);
        }
    };
    match run(cmd) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("rgoc-driver: {e}");
            ExitCode::from(2)
        }
    }
}

fn run(cmd: Command) -> Result<ExitCode, String> {
    let Command::Harness(h) = cmd;
    match h {
        HarnessCommand::List { all } => {
            print!("{}", render_list(all));
            Ok(ExitCode::SUCCESS)
        }
        HarnessCommand::Run { ids, all } => {
            let entries = pick(&ids, all)?;
            let s = execute(&entries)?;
            print!("{}", render_text(&s));
            Ok(finish(&s))
        }
        HarnessCommand::Report { out } => {
            let s = execute(&CORPUS_M0_SAMPLES)?;
            let text = serde_json::to_string_pretty(&to_json(&s))
                .map_err(|e| format!("JSON 序列化失败：{e}"))?;
            match out {
                Some(p) => {
                    std::fs::write(&p, format!("{text}\n"))
                        .map_err(|e| format!("写 {} 失败：{e}", p.display()))?;
                    print!("{}", render_text(&s));
                    eprintln!("\nJSON 报告已写入 {}", p.display());
                }
                None => println!("{text}"),
            }
            Ok(finish(&s))
        }
    }
}

/// 把命令行给的 ID 变成样本条目。**认不出的 ID 直接报错**，不静默忽略。
fn pick(ids: &[String], all: bool) -> Result<Vec<SampleEntry>, String> {
    if all {
        return Ok(CORPUS_M0_SAMPLES.to_vec());
    }
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let i = index_of(id).ok_or_else(|| {
            format!("未知样本 ID {id:?}：合法范围 T-C-01..T-C-20（或用 harness list 看）")
        })?;
        out.push(CORPUS_M0_SAMPLES[i]);
    }
    Ok(out)
}

/// 退出码：全部 pass ⇒ 0，否则 1。
fn finish(s: &RunSummary) -> ExitCode {
    if s.is_success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
