//! SSA 的**中立 dump**（格式由 `C10-ssa-dump.md` 冻结）。
//!
//! # 当前进度（Task 2 骨架）
//!
//! 只输出头三行与逐函数的 `# func` 行；块段/指令段在 Task 16 落地（那时 `Instr` 才存在）。
//! **这不算「dump 已实现」**：`C10` §4 的三条硬要求（确定性/自描述/文档与实现一致）
//! 要到 Task 16 才有判据可查（`T-SSA-13/14`）。
//!
//! # 为什么头行就要按契约写
//!
//! `C10` 规则 1 说头三行**顺序不可换、字段不可省**。如果骨架期随便写，
//! Task 16 的对撞就会变成「改格式」而不是「填内容」—— 而格式改动会同时
//! 波及 `--dump` 的消费方（`T-SSA-38`）。先按契约写头，后面只加块段。

use crate::ir::Module;

/// dump 的版本行（`C10` 规则 1；改动等于契约变更）。
pub const DUMP_VERSION: &str = "# SSA v1";

/// 把一个模块渲染成中立 dump。
///
/// 结构：
///
/// ```text
/// # SSA v1
/// # stage: ssa
/// # func <name> blocks=<n> values=<n>
/// ...（Task 16 起：块段与指令段）
/// ```
#[must_use]
pub fn dump_module(m: &Module) -> String {
    let mut out = String::new();
    out.push_str(DUMP_VERSION);
    out.push('\n');
    out.push_str("# stage: ssa\n");
    for f in &m.funcs {
        out.push_str(&format!(
            "# func {} blocks={} values={}\n",
            f.name, f.blocks, f.values
        ));
    }
    out
}
