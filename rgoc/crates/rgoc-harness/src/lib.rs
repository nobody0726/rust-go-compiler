//! rgoc 的测试基础设施：Test IR、官方语料驱动、结果比较器。
//!
//! **当前状态**：Phase 2 起步 —— `ir`（Test IR / 判定结果 / 冻结预算，T31）与
//! `instruction`（指令行解析 R1 + 分派顺序 R1b，T32）与
//! `corpus`（平台过滤 shouldTest + 语料枚举 + unsupported 归类，T33）与
//! `oracle`（版本守门 + R6 命令形态 + 超时回收 + RSS 观测，T34）已落地，
//! 指令解析（T32）、语料枚举（T33）、oracle 调用（T34）、比较器（T35）待补。
//! 每一步都按 `docs/milestones/M0-tests.md` §1.3 的规则逐条 RED→GREEN 实现，
//! 规则本身也在那次复核中补齐了两条（**R2b** 合并流、**R6** 命令形态）。

pub mod corpus;
pub mod instruction;
pub mod ir;
pub mod oracle;

/// 把两个数相加，再把结果翻倍。
///
/// **它不是遗留代码**（决策 **D-M0-15**）：它是 **T-H-01（成功类）的进程内 fixture**。
/// 原计划写「Phase 2 引入真实功能后删除」，但 **E5 是人工门禁** ——
/// 删掉调试锚点后，换机器 / 重装 / 升级 VSCode 都**无法复验 E5**。
/// 现在它同时承担两个角色：E5 的复验锚点 + harness 自测的进程内正例。
///
/// 原始任务见 `M0-plan.md` T21；保留理由见 `M0-design.md` §2.1 的 D-M0-15。
//
// 【为什么不是 `let sum = a + b; sum`】
//   实测（证据见 docs/milestones/M0-benchmarks.md §8）：
//   rustc 1.98.1 对「直接在尾位置返回的 `let` 绑定」**不生成 DWARF 变量条目** ——
//   `let sum = a + b; sum` 反汇编出的 DWARF 里，`add` 的子节点只有形参 a、b，
//   没有 `DW_TAG_lexical_block` / `DW_TAG_variable sum`，于是 **任何**调试器
//   （lldb CLI、CodeLLDB、gdb）都看不到 `sum`，「单步后观察中间值」这项检查
//   根本无法完成。只要 `sum` 在 `let` 之后被【第二次读取】（这里是被 `sum * 2`
//   读取），rustc 就会为它生成完整条目，调试器即可读到。
//   因此本函数的形状是 E5 可验证性的前提，不是随手写的示例代码。
//
// 【关于 clippy】
//   尾表达式是 `sum * 2` 而非 `sum`，所以不会触发 `clippy::let_and_return`，
//   不需要任何 allow。T22 的 clippy 门禁在无 allow 状态下通过。
pub fn double_sum(a: i64, b: i64) -> i64 {
    let sum = a + b;
    sum * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_sum_works() {
        assert_eq!(double_sum(1, 2), 6);
    }
}
