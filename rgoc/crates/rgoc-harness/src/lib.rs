//! rgoc 的测试基础设施：Test IR、官方语料驱动、结果比较器。
//!
//! **当前状态**：Phase 1 骨架 —— 只有一个用于验证调试链路的最小函数。
//! 完整实现（Test IR 字段、指令解析、`.out` 比较器）在 Phase 2 按
//! `docs/milestones/M0-tests.md` §1.3 的规则逐条 RED→GREEN 实现。

/// 把两个数相加，再把结果翻倍。
///
/// 这是 Phase 1 的**调试目标**：T28 会在这个函数内下断点，
/// 验证 VSCode + CodeLLDB 的调试链路真的可用（E5）。
/// Phase 2 引入真实功能后可删除。
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
