//! xtask 的第 1 职责：**语料枚举**。
//!
//! 转发 `rgoc_harness::corpus::enumerate`，并把结果整理成 JSON。
//!
//! **为什么要有这一层而不用 driver**：E4 的分母是 20 个样本，但 `M0-tests.md` §8
//! 还有一行「基线报告（**非门禁**）：顶层 `test/` 全量枚举，分母 **279**」。
//! 那份全量报告要在 T38 随交付报告一起产出，而它的数据源与 `harness list`
//! （20 个基线样本）**不是同一个集合** —— 混用会把 279 悄悄缩成 20。

use std::path::Path;

use rgoc_harness::corpus::{CorpusConfig, CorpusError, enumerate};
use serde_json::{Value, json};

/// 枚举失败。
#[derive(Debug)]
pub enum EnumerateError {
    /// 语料目录读不了
    Corpus(CorpusError),
    /// JSON 序列化失败
    Json(String),
}

impl std::fmt::Display for EnumerateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Corpus(e) => write!(f, "语料枚举失败：{e}"),
            Self::Json(e) => write!(f, "JSON 序列化失败：{e}"),
        }
    }
}

impl std::error::Error for EnumerateError {}

/// 枚举语料顶层 `*.go`，返回 harness 的报告。
pub fn enumerate_report(dir: &Path) -> Result<rgoc_harness::corpus::CorpusReport, EnumerateError> {
    enumerate(dir, &CorpusConfig::m0()).map_err(EnumerateError::Corpus)
}

/// 枚举结果的 JSON 形态。
pub fn report_to_json(rep: &rgoc_harness::corpus::CorpusReport) -> Result<Value, EnumerateError> {
    let by_mode: serde_json::Map<String, Value> = rep
        .by_mode
        .iter()
        .map(|(m, n)| (m.name().to_string(), json!(n)))
        .collect();
    let by_u: serde_json::Map<String, Value> = rep
        .excluded_by_u
        .iter()
        .map(|(u, n)| (u.clone(), json!(n)))
        .collect();
    Ok(json!({
        "total": rep.total,
        "denominator": rep.denominator,
        "excluded": rep.excluded,
        "target_filtered": rep.target_filtered,
        "executed": rep.denominator - rep.target_filtered,
        "by_mode": Value::Object(by_mode),
        "excluded_by_u": Value::Object(by_u),
    }))
}

/// 枚举并直接拿到 JSON。
pub fn enumerate_json(dir: &Path) -> Result<Value, EnumerateError> {
    let rep = enumerate_report(dir)?;
    report_to_json(&rep)
}
