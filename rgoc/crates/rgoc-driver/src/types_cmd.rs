//! `types` 子命令 —— 对一组同包 `.go` 文件跑**类型检查入口**（M3）。
//!
//! # 状态：**占位**（M3/Task 2）
//!
//! 类型检查本体在 `rgoc-const` / `rgoc-types` / `rgoc-load` 三个 crate 里，
//! 从 `M3-plan.md` Task 28 起接进本命令。在那之前本命令**显式报错**。
//!
//! ## 为什么宁可报错也不静默返回「无诊断」
//!
//! `D-M3-11`：用户可见的「未实现」必须是**稳定信号**。若这里静默成功，
//! 门禁与用户都会把它读成「这个包类型检查通过了」—— 那是最坏的一种失败
//! （`AGENTS.md` 反复记的「静默失败」形状）。
//!
//! 这也是 `rgoc-load::load_from_explicit_files` 的桩**刻意不被调用**的原因：
//! 那个桩现在返回空诊断，等于「一切正常」。

use std::path::PathBuf;

/// 跑 `types`。`Ok` 是要打印的正文；`Err` 是给 `main_entry` 的错误文本
/// （统一走 stderr + 退出码 2）。
pub fn run_types(files: &[PathBuf]) -> Result<String, String> {
    Err(未实现文案(files))
}

/// 未实现的报错文案。**稳定**：`tests/test_driver.rs` 按它断言。
///
/// 带上文件个数（而不是文件名列表）是为了让「参数确实传进来了」可见 ——
/// 参数被吞掉与参数正确但在下游失败，是两种完全不同的故障。
fn 未实现文案(files: &[PathBuf]) -> String {
    format!(
        "rgoc types：类型检查尚未实现（M3 进行中）（已收到 {} 个文件）",
        files.len()
    )
}
