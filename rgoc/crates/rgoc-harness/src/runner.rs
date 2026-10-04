//! 用例执行器：把 T31–T35 的零件串成「**一条用例端到端**」。
//!
//! 顺序（**不可颠倒**，每一步都对应一条已冻结的规则）：
//!
//! ```text
//! 1) R1    解析 action            → 失败 ⇒ harness-failure
//! 2) R1b   平台过滤（**先于**指令判定）⇒ target-filtered（仍计入分母，且**不执行**）
//! 3) switch 指令是否已知            → 未知 ⇒ harness-failure（**不是** compiler-failure）
//! 4) 跑 oracle                     → 超时 / 超 RSS / 拉起失败
//! 5) R2/R3/R4 比对                  → 通过或语义失败
//! ```
//!
//! # 为什么第 2 步在第 3 步之前
//!
//! T29 冻结时从真实语料 `linkmain.go` 上查出来的坑：那个文件按 R1 解析出的 action 是
//! `Copyright 2015 …`（一个**非法指令**），但官方因为**先**做平台过滤而 `t.Skip`，
//! 从不 `Fatalf`。顺序颠倒的 harness 会在真实语料上误报「未知指令硬失败」。
//! 本模块的 `dispatch(ins, platform_ok)` 也把这个顺序写进了函数签名（T32）。
//!
//! # 「harness 能力不足」与「真的不匹配」必须分开
//!
//! T35 的正则子集不认识 `{n,m}` / `[...]` / `+` 等构造时**明确报错**（`UNSUPPORTED-REGEX:`）。
//! 那种情况判 [`Verdict::HarnessFailure`] —— **不是** `compiler-failure`。
//! 顶层 `test/` 的 5435 条 ERROR 模式大多在子集外，若把「不认识」当成「不匹配」，
//! 它们会被误判成「编译器有 bug」—— 比「跑不动」严重得多的偏差。

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::compare::{check_expected_output, error_check, replace_prefix, wanted_errors};
use crate::corpus::{CorpusConfig, header_of, should_test};
use crate::instruction::{Dispatch, DispatchError, dispatch, parse_action};
use crate::ir::{Layer, Mode, Verdict};
use crate::oracle::{Oracle, OracleConfig};

/// 一条待跑的用例。
///
/// 之所以把 `src` / `expected_out` 打包进来，而不是让 runner 自己去磁盘找：
/// **R2 的期望文件在 goroot 的 test 目录**（不是测试文件所在目录），而六类自测要用
/// 合成用例 —— 路径解析是调用方（`corpus.rs` / 未来的 driver）的事，本模块只管跑与判。
#[derive(Debug, Clone)]
pub struct CaseSpec<'a> {
    /// 短文件名（如 `helloworld.go`），诊断前缀就是它
    pub name: &'a str,
    /// **磁盘上的文件路径**。真实语料指向只读的 `GOROOT/test`，自测用例指向临时目录 ——
    /// 执行器只读它、从不写它。
    pub path: &'a std::path::Path,
    /// 源码全文（含首行的 execution recipe）—— 只用于解析 ERROR 注释
    pub src: &'a str,
    /// `.out` 的**内容**；`None` 表示该文件不存在 ⇒ 期望为**空**（R2）
    pub expected_out: Option<String>,
    /// 用哪一层的时间/资源预算
    pub layer: Layer,
}

/// 一条用例跑完的结果。
#[derive(Debug, Clone)]
pub struct CaseOutcome {
    /// 用例名（短文件名）
    pub name: String,
    /// 判定结果
    pub verdict: Verdict,
    /// 人可读的原因（失败时说明「到底哪儿对不上」）
    pub detail: String,
    /// 实际耗时（**被平台过滤 / 未执行时为 0** —— 这本身是个信号）
    pub duration: Duration,
    /// 是否计入分母（`03` §3.3：过滤项也计入分母）
    pub in_denominator: bool,
    /// 最后一步子进程的 pid（超时那类要靠它验证进程被回收）
    pub child_pid: Option<u32>,
}

impl CaseOutcome {
    fn quick(name: &str, verdict: Verdict, detail: impl Into<String>) -> Self {
        Self {
            name: name.to_string(),
            verdict,
            detail: detail.into(),
            duration: Duration::ZERO,
            in_denominator: true,
            child_pid: None,
        }
    }
}

/// 跑一条用例，得出判定结果。
pub fn run_case(spec: &CaseSpec, cfg: &CorpusConfig, oracle: &Oracle) -> CaseOutcome {
    // ① R1：解析 action。官方对 `:497` / `:514` 是硬失败 ⇒ harness 自己的问题
    let ins = match parse_action(spec.src) {
        Ok(i) => i,
        Err(why) => {
            return CaseOutcome::quick(
                spec.name,
                Verdict::HarnessFailure,
                format!("execution recipe 解析失败：{why}"),
            );
        }
    };

    // ② R1b：平台过滤**先于**指令判定（见模块头）
    if !should_test(header_of(spec.src, &ins.action), cfg) {
        return CaseOutcome::quick(
            spec.name,
            Verdict::TargetFiltered,
            "构建约束不满足当前目标 ⇒ 不执行（仍计入分母）",
        );
    }

    // ③ switch：指令是否已知
    let mode = match dispatch(&ins, true) {
        Ok(Dispatch::Proceed(m)) => m,
        Ok(Dispatch::SkippedByDesign) => {
            return CaseOutcome::quick(
                spec.name,
                Verdict::HarnessFailure,
                "该用例是 skip（上游设计即跳过，U13），不应送进执行器",
            );
        }
        // 平台过滤已在 ② 判过，走到这里说明调用方与执行器对过滤结论有分歧
        Ok(Dispatch::TargetFiltered) => {
            return CaseOutcome::quick(
                spec.name,
                Verdict::HarnessFailure,
                "dispatch 判被过滤，但 should_test 说通过 —— 两处结论不一致",
            );
        }
        Err(DispatchError::UnknownAction(a)) => {
            return CaseOutcome::quick(
                spec.name,
                Verdict::HarnessFailure,
                // ⚠️ 不是 compiler-failure：harness 连指令都没读懂，
                // 与「被测件编译失败」是两件完全不同的事
                format!("unknown pattern: {a:?}（harness 读不懂这条指令）"),
            );
        }
    };

    // ③b 非 v0 模式不该走到这里：它们**不在分母内**（corpus.rs 会先排除），
    // 送进执行器属于调用方的编排错误 —— 同样判 harness-failure（基建类）
    if !mode.is_v0_supported() {
        return CaseOutcome::quick(
            spec.name,
            Verdict::HarnessFailure,
            format!("模式 {} 不在 v0 支持集，不应送进执行器", mode.name()),
        );
    }

    // ④ 跑 oracle
    let start = Instant::now();
    let out = match oracle.run_mode_path(mode, spec.path) {
        Ok(o) => o,
        Err(e) => {
            return CaseOutcome::quick(
                spec.name,
                Verdict::HarnessFailure,
                format!("oracle 调用失败：{e}"),
            );
        }
    };
    let dur = start.elapsed();

    // ⑤ 判定
    let (verdict, detail) = judge(mode, spec, &out, oracle);
    CaseOutcome {
        name: spec.name.to_string(),
        verdict,
        detail,
        duration: dur,
        in_denominator: true,
        child_pid: out.pid,
    }
}

/// 拿到输出与期望之后怎么判。
fn judge(
    mode: Mode,
    spec: &CaseSpec,
    out: &crate::oracle::OracleOutput,
    oracle: &Oracle,
) -> (Verdict, String) {
    if out.timed_out {
        return (Verdict::Timeout, "超过单项 wall time 上限".into());
    }
    if out.resource_exceeded {
        return (
            Verdict::ResourceFailure,
            format!(
                "峰值 RSS {} 超过上限 {}",
                out.peak_rss_bytes,
                oracle.limits().max_rss_bytes
            ),
        );
    }
    match mode {
        Mode::Run => {
            // fast path 的 compile / link 步失败就是「编译不过」
            if out.exit_code != Some(0) {
                return (
                    Verdict::CompilerFailure,
                    format!("编译或链接失败，exit = {:?}", out.exit_code),
                );
            }
            match check_expected_output(&out.merged, spec.expected_out.as_deref()) {
                Ok(()) => (Verdict::Pass, String::new()),
                Err(e) => (Verdict::RuntimeFailure, e.to_string()),
            }
        }
        Mode::Compile => {
            // 官方 compileFile 只看 err，不看输出（警告不算失败）
            if out.exit_code == Some(0) {
                (Verdict::Pass, String::new())
            } else {
                (
                    Verdict::CompilerFailure,
                    format!(
                        "编译失败，exit = {:?}：{}",
                        out.exit_code,
                        first_line(&out.merged)
                    ),
                )
            }
        }
        Mode::ErrorCheck => {
            // 官方 wantError：编译「成功」反而是失败（`:792-798`）
            if out.exit_code == Some(0) {
                return (
                    Verdict::CompilerFailure,
                    "errorcheck 用例本应编译失败，却成功了".into(),
                );
            }
            let want = match wanted_errors(spec.src, spec.name) {
                Ok(w) => w,
                Err(e) => {
                    return (Verdict::HarnessFailure, format!("解析 ERROR 注释失败：{e}"));
                }
            };
            // 诊断里的**全路径**换成短名（R6 细节 3；续行里的路径也要换）
            let full = spec.path.to_string_lossy().into_owned();
            let diag = replace_prefix(&out.merged, &full, spec.name);
            match error_check(&diag, &want) {
                Ok(()) => (Verdict::Pass, String::new()),
                Err(errs) => {
                    // 「harness 能力不足」与「真的不匹配」必须分开
                    if errs.iter().any(|e| e.starts_with("UNSUPPORTED-REGEX")) {
                        (Verdict::HarnessFailure, errs.join(" ;; "))
                    } else {
                        (Verdict::CompilerFailure, errs.join(" ;; "))
                    }
                }
            }
        }
        // ③b 已挡住非 v0 模式；这里只是让 match 穷尽
        other => (
            Verdict::HarnessFailure,
            format!("未预期的模式 {}（应当在 ③b 就被拦下）", other.name()),
        ),
    }
}

fn first_line(s: &str) -> &str {
    s.lines().find(|l| !l.trim().is_empty()).unwrap_or("")
}

/// 一整层的执行报告。
#[derive(Debug, Clone, Default)]
pub struct LayerReport {
    /// 逐用例结果
    pub outcomes: Vec<CaseOutcome>,
    /// 层级级错误（目前只有 oracle 版本/平台不符）
    pub oracle_error: Option<String>,
}

impl LayerReport {
    /// **分母**：跑进来的用例数。被平台过滤的**也计入**（`03` §3.3）。
    pub fn denominator(&self) -> usize {
        self.outcomes.len()
    }

    /// **分子**：只有 `pass` 计入。
    pub fn passed(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| o.verdict.counts_toward_numerator())
            .count()
    }

    /// 某一种判定的计数。
    pub fn count(&self, v: Verdict) -> usize {
        self.outcomes.iter().filter(|o| o.verdict == v).count()
    }

    /// 按判定分类聚合（报告用；`Verdict::ALL` 的顺序即报告顺序）。
    pub fn by_verdict(&self) -> BTreeMap<Verdict, usize> {
        let mut m = BTreeMap::new();
        for o in &self.outcomes {
            *m.entry(o.verdict).or_insert(0) += 1;
        }
        m
    }
}

/// 跑一整层。
///
/// **版本守门在这里生效**（T-H-06）：oracle 建不起来（版本或 GOOS/GOARCH 不符）时，
/// 整层用例一律判 `reference-toolchain-failure`、**一个都不跑** ——
/// 「版本不符就降级跑别的 Go」是最危险的一种偏差。
pub fn run_layer(specs: &[CaseSpec], oc: &OracleConfig, cfg: &CorpusConfig) -> LayerReport {
    let oracle = match Oracle::new(oc.clone()) {
        Ok(o) => o,
        Err(e) => {
            return LayerReport {
                outcomes: specs
                    .iter()
                    .map(|s| {
                        CaseOutcome::quick(
                            s.name,
                            Verdict::ReferenceToolchainFailure,
                            e.to_string(),
                        )
                    })
                    .collect(),
                oracle_error: Some(e.to_string()),
            };
        }
    };
    LayerReport {
        outcomes: specs.iter().map(|s| run_case(s, cfg, &oracle)).collect(),
        oracle_error: None,
    }
}
