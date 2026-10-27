#![allow(dead_code)]
//! M11 并发判据的**三层差分**装置（`M11-design.md` §4.1）。
//!
//! | 层 | 比什么 | 覆盖面 |
//! |---|---|---|
//! | **L1** native vs **解释器** | stdout + stderr + 退出码 | 全部新 fixture（**主判据**）|
//! | **L2** native vs **官方 `go run`** | 同上 | `N'` 的 7 个 |
//! | **L3** 解释器 vs 官方 | 同上 | M8 已绿（7/7），M11 只**复跑**不改 |
//!
//! # 为什么 L1 是主判据
//!
//! 官方的并发行为有**未定义面**（`select` 的随机选择、goroutine 的实际交错），
//! 而 `C11` 已经把 `rgoc` 的并发语义**冻结**成「M:1 协作式 + 源码顺序」。所以
//! 「与解释器逐字节一致」才是 M11 真正要交的东西；官方只用来证明「解释器那一侧
//! 没有偏离 Go」（L3，M8 已经做完）。
//!
//! 这条分工不是省事：如果拿官方当主判据，遇到 `select` 多就绪的用例就必然
//! 偶发红，而那种红**无法归因**（`M11-design.md` `U-M11-07`）。
//!
//! # 为什么 stdout 与 stderr **分开**比
//!
//! `C9` §5 的死锁文本只进 stderr。合并了就分不清「谁打的」—— 而「stdout 为空 +
//! stderr 恰好一行」正是 `T-EXIT-01` 的判据形态。
//!
//! # 为什么 `oracle` 是 `Option`
//!
//! 新增 fixture（`FIX` 组）**没有**官方期望值 —— 它们是给 `rgoc` 自己的语义写的
//! 边界用例（比如「`select` 里重复 channel 只有一个 winner」）。这类用例只走 L1。
//! 把它写成必填会逼着人去伪造一份 `.exp`，那是把判据变成装饰。

use super::native_harness::{Outcome, build_and_run_env, hir_output};

/// 一次三层观测。
pub struct Triple {
    /// native 侧：编译产物跑出来的 `(stdout, stderr, exit)`。
    pub native: Outcome,
    /// 解释器侧的 stdout（`rgoc-driver run --via=hir`）。
    pub interp: String,
    /// 官方 `go run` 的 stdout；新增 fixture 为 `None`。
    pub oracle: Option<String>,
}

/// 编译 + 跑 native，再跑一遍解释器。
///
/// # Errors
///
/// native 构建失败（含静态拒绝 `SSA-UNSUPPORTED-*`）或解释器退出码非 0 ——
/// 两者都是「没跑起来」，不是「跑出来的结果」。
pub fn probe(name: &str, src: &str, env: &[(&str, &str)]) -> Result<Triple, String> {
    let native = build_and_run_env(name, src, env)?;
    let interp = hir_output(name, src)?;
    Ok(Triple {
        native,
        interp,
        oracle: None,
    })
}

impl Triple {
    /// 挂上官方期望值（`N'` 的 7 个才有）。
    #[must_use]
    pub fn with_oracle(mut self, oracle: impl Into<String>) -> Self {
        self.oracle = Some(oracle.into());
        self
    }

    /// L1：native 的 stdout 必须与解释器**逐字节**相同。
    ///
    /// # Panics
    ///
    /// 不一致时把两边的完整字节打出来 —— 截断会让「差在哪」变成猜。
    pub fn assert_l1(&self, what: &str) {
        assert_eq!(
            self.native.stdout, self.interp,
            "[{what}] L1 失败：native 与解释器的 stdout 不一致\n\
             --- native ---\n{}\n--- interp ---\n{}",
            self.native.stdout, self.interp
        );
    }

    /// L2：若挂了官方期望值，native 也必须与它相同。
    ///
    /// # Panics
    ///
    /// 同 [`Self::assert_l1`]。
    pub fn assert_l2(&self, what: &str) {
        if let Some(o) = &self.oracle {
            assert_eq!(
                self.native.stdout, *o,
                "[{what}] L2 失败：native 与官方 `go run` 的 stdout 不一致\n\
                 --- native ---\n{}\n--- oracle ---\n{o}",
                self.native.stdout
            );
        }
    }

    /// 一次跑完 L1 + L2。
    ///
    /// # Panics
    ///
    /// 任一层不一致。
    pub fn assert_same(&self, what: &str) {
        self.assert_l1(what);
        self.assert_l2(what);
    }
}
