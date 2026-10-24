//! `build` 子命令（M7）：SSA → `.s` → `clang` → ELF。
//!
//! # 当前状态（Task 2）：**只有参数解析，没有产物**
//!
//! 本模块先把**通道形状**固定下来（`stdout` / `stderr` / `exit` 三件，与 `run_cmd`/`ssa_cmd` 同构），
//! 真身从 Task 50 起接。在那之前 `run_build` 一律返回 `Err` ⇒ `main_entry` 打一行到 stderr
//! 并给退出码 **2**。
//!
//! 为什么先返回 `Err` 而不是「静默成功」：一个「命令存在但什么都没做、退出码 0」的入口
//! 会让 `T-NAT-47` 与 `run-collide.sh --native` 都变成**恒绿的空话**
//! —— 这是本仓 T37 起就有的纪律（`cli.rs` 模块文档：「宁可直接报错也不假装支持」）。

use std::path::PathBuf;

use crate::cli::BuildCommand;

/// `build` 的三通道结果（与 `ssa_cmd::SsaOutcome` 同构）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildOutcome {
    /// 只放**给对撞看**的东西（`--emit=asm` 时是汇编文本）。
    pub stdout: String,
    /// 诊断与工具链错误。
    pub stderr: String,
    /// 退出码：`0` 成功 / `1` 静态拒绝（不支持面、检查诊断）/ `2` 内部或工具链失败。
    pub exit: u8,
    /// 产物路径（成功时才有）。
    ///
    /// **为什么要返回它**：`--emit=asm` 的判据是「两次逐字节相同」，而比对两份文件
    /// 之前得先知道比哪两份。让测试自己按「`<stem>`」的规则去猜，等于把**命名规则**
    /// 复制到每个测试里 —— 规则一改，测试就会去比一对不存在的文件（然后「通过」）。
    pub artifact: Option<PathBuf>,
}

/// 跑一次 `build`。
///
/// # 错误
///
/// 返回 `Err` 表示「连跑都没跑起来」（参数组合非法、IO 失败）—— 由 `main_entry` 统一
/// 打成退出码 2。**静态拒绝**（`CgCode`）不走这里，走 `Ok(BuildOutcome { exit: 1, .. })`：
/// 「没跑」与「跑了但产物不对」必须能区分（`C9` §5 的 1/2 分界）。
pub fn run_build(cmd: &BuildCommand) -> Result<BuildOutcome, String> {
    Err(format!(
        "build 尚未实现（M7/Task 50 接线；现在只有参数解析，收到 {} 个文件、emit={:?}）",
        cmd.files.len(),
        cmd.emit
    ))
}
