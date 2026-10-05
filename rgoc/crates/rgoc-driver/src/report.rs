//! 报告：把 [`LayerReport`] 汇总成**门禁能直接读**的数字与人读文本。
//!
//! # 报告的三个硬要求（`M0-tests.md` §8）
//!
//! 1. **分子/分母必须都在** —— 分母是 `outcomes.len()`，**不因被平台过滤而变小**。
//!    `03` §3.3：过滤项不计入分子、仍计入分母。把分母写成「pass + 失败」以外的东西
//!    就等于缩小分母。
//! 2. **八类判定必须齐全**（含计数为 0 的）—— 门禁要能一眼看出「是哪一类在挡」。
//!    只列出出现过的类别，读报告的人得自己猜「没列的是没有还是漏了」。
//! 3. **退出码由「有没有非 pass」决定** —— 这是 E4 门禁的机器可判形式。
//!    注意 `target-filtered` 也算失败：20 个样本一个都不能被平台过滤掉
//!    （`M0-tests.md` §4 的样本全在分母内且不带构建约束）。

use std::collections::BTreeMap;
use std::fmt::Write as _;

use rgoc_harness::ir::Verdict;
use rgoc_harness::runner::LayerReport;
use serde_json::{Value, json};

/// 一次执行的汇总结果 —— 本模块的公开形状。
///
/// `xtask` 与将来的 `M0-report.md` 生成都读它的字段，所以字段名是**契约**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    /// 分母（= 跑进来的用例数，被平台过滤的**也**计入）
    pub denominator: usize,
    /// 分子（只有 `pass` 计入）
    pub numerator: usize,
    /// 八类判定的计数，**八类齐全**（`Verdict::ALL` 顺序）
    pub by_verdict: BTreeMap<&'static str, usize>,
    /// 逐用例明细
    pub cases: Vec<CaseLine>,
    /// 整层错误（oracle 版本/平台不符等）
    pub oracle_error: Option<String>,
    /// 整层的**最大**峰值 RSS（字节）—— 与 `M0-tests.md` §7.5 的单用例上限对照
    pub peak_rss_bytes: u64,
    /// 整层 wall time（毫秒）
    pub total_duration_ms: u64,
}

/// 一条用例的明细行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseLine {
    /// 用例名
    pub name: String,
    /// 判定
    pub verdict: Verdict,
    /// 耗时（毫秒）
    pub duration_ms: u64,
    /// 观测到的峰值 RSS（字节）—— T38 报告要求含它
    pub peak_rss_bytes: u64,
    /// 失败原因 / 备注
    pub detail: String,
}

impl RunSummary {
    /// 某类判定的计数。
    pub fn count(&self, v: Verdict) -> usize {
        self.by_verdict.get(v.as_str()).copied().unwrap_or(0)
    }

    /// 是否「成功」—— 即**每一条都是 `pass`**。
    ///
    /// 这是退出码的依据。空集算**失败**：跑 0 条不等于跑过了。
    pub fn is_success(&self) -> bool {
        self.denominator > 0 && self.numerator == self.denominator && self.oracle_error.is_none()
    }
}

/// 把一层报告汇总成 [`RunSummary`]。
pub fn summarize(rep: &LayerReport) -> RunSummary {
    // ★ 分母 = outcomes.len()，不过滤。被平台过滤的用例它的 duration 恒为 0，
    // 但**仍占一个位置** —— 这正是「不计入分子、仍计入分母」的实现方式。
    let denominator = rep.denominator();
    let numerator = rep.passed();
    // 八类齐全：先全部填 0，再累加。漏掉哪类，测试会红。
    let mut by_verdict: BTreeMap<&'static str, usize> =
        Verdict::ALL.iter().map(|v| (v.as_str(), 0)).collect();
    for o in &rep.outcomes {
        *by_verdict.entry(o.verdict.as_str()).or_insert(0) += 1;
    }
    RunSummary {
        denominator,
        numerator,
        by_verdict,
        cases: rep
            .outcomes
            .iter()
            .map(|o| CaseLine {
                name: o.name.clone(),
                verdict: o.verdict,
                duration_ms: o.duration.as_millis() as u64,
                peak_rss_bytes: o.peak_rss_bytes,
                detail: o.detail.clone(),
            })
            .collect(),
        oracle_error: rep.oracle_error.clone(),
        // 整层峰值 = 各用例的最大值（不是求和 —— 逐条串行执行，峰值不叠加）
        peak_rss_bytes: rep
            .outcomes
            .iter()
            .map(|o| o.peak_rss_bytes)
            .max()
            .unwrap_or(0),
        total_duration_ms: rep
            .outcomes
            .iter()
            .map(|o| o.duration.as_millis() as u64)
            .sum(),
    }
}

/// 人读文本。
pub fn render_text(s: &RunSummary) -> String {
    let mut w = String::new();
    let _ = writeln!(w, "═══════════════════════════════════════════");
    let _ = writeln!(w, " rgoc harness 执行报告");
    let _ = writeln!(w, "═══════════════════════════════════════════");
    if let Some(e) = &s.oracle_error {
        let _ = writeln!(w, "层错误：{e}");
    }
    let _ = writeln!(w, "分母 = {}　分子 = {}", s.denominator, s.numerator);
    let _ = writeln!(w, "判定分布（八类，缺失的按 0 计）：");
    for v in Verdict::ALL {
        let _ = writeln!(w, "  {:<28} {}", v.as_str(), s.count(v));
    }
    let _ = writeln!(w, "逐用例（{} 条）：", s.cases.len());
    let _ = writeln!(
        w,
        "整层 wall time = {:.1}s　峰值 RSS = {} MiB",
        s.total_duration_ms as f64 / 1000.0,
        s.peak_rss_bytes / 1024 / 1024
    );
    for c in &s.cases {
        // 耗时用秒带三位小数：报告是给人读的，ms 精度太低、ns 太噪
        let _ = write!(w, "  {:<24} {:<28}", c.name, c.verdict.as_str());
        let _ = write!(w, "{:>8.3}s", c.duration_ms as f64 / 1000.0);
        // 峰值 RSS 以 MiB 列出（字节数在报告里没人读得下去）
        let _ = writeln!(w, " {:>6} MiB", c.peak_rss_bytes / 1024 / 1024);
        if !c.detail.is_empty() {
            let _ = writeln!(w, "      {}", c.detail);
        }
    }
    let _ = writeln!(w, "───────────────────────────────────────────");
    let _ = writeln!(
        w,
        "结论：{}",
        if s.is_success() {
            "全部通过"
        } else {
            "未全部通过（详见上面的分布）"
        }
    );
    w
}

/// 机器读 JSON。`by_verdict` 与 `cases` 的键名用 [`Verdict::as_str`]，
/// 与 manifest 的 `test_ids` 共用同一套标识。
pub fn to_json(s: &RunSummary) -> Value {
    let by_verdict: serde_json::Map<String, Value> = s
        .by_verdict
        .iter()
        .map(|(k, v)| ((*k).to_string(), json!(v)))
        .collect();
    let cases: Vec<Value> = s
        .cases
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "verdict": c.verdict.as_str(),
                "duration_ms": c.duration_ms,
                "peak_rss_bytes": c.peak_rss_bytes,
                "detail": c.detail,
            })
        })
        .collect();
    json!({
        "denominator": s.denominator,
        "numerator": s.numerator,
        "success": s.is_success(),
        "peak_rss_bytes": s.peak_rss_bytes,
        "total_duration_ms": s.total_duration_ms,
        "by_verdict": Value::Object(by_verdict),
        "oracle_error": s.oracle_error,
        "cases": Value::Array(cases),
    })
}
