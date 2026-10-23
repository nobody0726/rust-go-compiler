//! `ssa` 子命令 —— **参数与通道**这一层（M6/Task 2 骨架）。
//!
//! # 骨架期为什么必须「响亮拒绝」
//!
//! IR 形状要到 Task 7 才落地、构建要到 Task 17、dump 内容要到 Task 16。
//! 在这之前，`ssa --dump` 唯一诚实的回答是「尚未接入」——
//! 打一个空 dump（或退出码 0）会让人以为「SSA 能用」，而那正是
//! `T37` 纪律（未实现的能力不许假装支持）要防的事。
//!
//! 所以本模块当前**只做三件不涉及 IR 的事**：
//!
//! 1. 探文件可读性（`rgoc_load` 不读文件，静默跳过是最坏的形态）；
//! 2. 把「读不到」变成 `Err`（`main_entry` 打 stderr + 退出码 2，**绝不写 stdout**）；
//! 3. 可读但管线未接 ⇒ 退出码 2 + stderr 一行，stdout 保持空。
//!
//! Task 16 会把第 3 条换成真的 `rgoc_ssa::dump::dump_module`，
//! 届时 `tests/ssa_cli.rs` 的「未接入」断言也要一起改（台账有记）。

use crate::cli::SsaCommand;

/// `ssa` 的运行结果（三条通道不合并，与 `run_cmd::RunOutcome` 同形）。
#[derive(Debug)]
pub struct SsaOutcome {
    /// stdout：`C10` 中立 dump（骨架期为空）。
    pub stdout: String,
    /// stderr：verifier 错误 / 「尚未接入」的说明。
    pub stderr: String,
    /// 退出码（0 正常 / 1 静态拒绝 / 2 参数与 io 错、内部未完成）。
    pub exit: u8,
}

/// 跑 `ssa`。`Err` ⇒ 退出码 2（参数/io/内部错）。
pub fn run_ssa(cmd: &SsaCommand) -> Result<SsaOutcome, String> {
    // ① 可读性先探：`rgoc_load` 不读文件，静默跳过是最坏的形态。
    for f in &cmd.files {
        std::fs::metadata(f).map_err(|e| format!("读 {} 失败：{e}", f.display()))?;
    }

    // ② IR 管线未接入 —— 响亮拒绝，不吐空 dump。
    Ok(SsaOutcome {
        stdout: String::new(),
        stderr: "ssa: IR 管线尚未接入（M6/Task 7 起；dump 内容见 Task 16）。\
                 当前只做参数与通道骨架。"
            .to_string()
            + "\n",
        exit: 2,
    })
}
