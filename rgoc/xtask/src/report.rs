//! xtask 的第 2 职责：**报告生成**。
//!
//! 转发 `rgoc_driver` 的执行与渲染，把 JSON 落到磁盘。**不自己判语义** ——
//! 判定全在 `rgoc_harness::runner::run_layer`，driver 与 xtask 都只是调用方。
//!
//! # 为什么报告渲染只有一处
//!
//! 人读文本由 `rgoc_driver::report::render_text` 渲染，本模块**不再抄一份**。
//! 早先这里写了个从 JSON 反推文本的 `human_text`，看着等价，实际有两个坏处：
//! ① 两份渲染器迟早漂（改了 driver 的那份、忘了改 xtask 的那份，报告就自相矛盾）；
//! ② 从 JSON 反推会丢类型（判定从枚举变成字符串再比字符串）。
//! **唯一例外**是 `write_report` 返回人读文本时用到的 `RunSummary` —— 那个直接
//! 复用 driver 的 `summarize`，不重跑。

use std::path::{Path, PathBuf};

use rgoc_driver::report::{RunSummary, render_text, summarize, to_json};
use rgoc_driver::samples::{CORPUS_M0_SAMPLES, LoadedSamples};
use rgoc_harness::corpus::CorpusConfig;
use rgoc_harness::ir::Layer;
use rgoc_harness::oracle::OracleConfig;
use rgoc_harness::runner::run_layer;

/// 语料目录（`go_source_code/test`）。可用 `RGOC_CORPUS_TEST_DIR` 覆盖。
pub fn corpus_test_dir() -> PathBuf {
    if let Ok(d) = std::env::var("RGOC_CORPUS_TEST_DIR") {
        return PathBuf::from(d);
    }
    // CARGO_MANIFEST_DIR = <repo>/rgoc/xtask
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../go_source_code/test")
        .to_path_buf()
}

/// 跑 20 个基线样本，返回汇总（**唯一执行点**）。
pub fn build_summary() -> Result<RunSummary, String> {
    let dir = corpus_test_dir();
    let loaded = LoadedSamples::load(&dir, &CORPUS_M0_SAMPLES).map_err(|e| e.to_string())?;
    if loaded.is_empty() {
        return Err("没有可跑的样本 —— 静默跑 0 条会被误读成「全部通过」".into());
    }
    let oc = OracleConfig {
        work_dir: dir.clone(),
        ..OracleConfig::from(CorpusConfig::m0())
    };
    let specs = loaded.specs(Layer::Corpus);
    let rep = run_layer(&specs, &oc, &CorpusConfig::m0());
    Ok(summarize(&rep))
}

/// 跑 20 个样本，把 JSON 报告写到 `out`，返回人读文本。
///
/// ⚠️ 只跑**一次**：`summarize` 出的 `RunSummary` 同时喂给 JSON 与文本渲染。
/// 早先版本为了拿文本又跑了一遍，耗时翻倍且两次结果可能不同（oracle 侧状态在变）。
pub fn write_report(out: &Path) -> Result<String, String> {
    let s = build_summary()?;
    let v = to_json(&s);
    let text = serde_json::to_string_pretty(&v).map_err(|e| format!("JSON 序列化失败：{e}"))?;
    std::fs::write(out, format!("{text}\n"))
        .map_err(|e| format!("写 {} 失败：{e}", out.display()))?;
    Ok(render_text(&s))
}
