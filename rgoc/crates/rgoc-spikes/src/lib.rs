//! # `rgoc-spikes` —— **SPIKE-ONLY**，三个架构 spike 的隔离载体
//!
//! 决策 **D-M0-14**：「三个 spike 放在独立 crate `rgoc-spikes`（三个 bin + 共享固定 HIR fixture）」。
//! 依据是 `03` §4 的纪律「**不盲目演进临时代码**」—— 隔离成一个 crate 后，
//! 后续用测试驱动的正式实现替换时**可以整块删掉**，不会与正式代码纠缠在同一模块里。
//!
//! # 三个 bin 与它们要验证的门禁
//!
//! | bin | 测试 ID | 门禁 | 验证什么 |
//! |---|---|---|---|
//! | `s1_interp` | `T-S1-01/02/03` | E6 | 固定 HIR → 宿主求值 → stderr 精确 `3\n` |
//! | `s2_ssa`   | `T-S2-01/02/03` | E6 | 同一份 HIR → Block/Value → 求值**必须与 S1 逐字节相同** |
//! | `s3_native`| `T-S3-01…05` | E6 / E7 | 固定函数 → arm64 汇编 → `clang` → 可运行 ELF |
//!
//! # 「输入写死」是硬要求（`M0-plan.md` T41）
//!
//! 三个 spike **一律不接受命令行参数**。理由：E6 判定的是「输入、结果、环境全部可复现」——
//! 如果输入能由命令行改变，那「重复三次一致」验证的是用户有没有传同样的参数，
//! 而不是编译器可复现与否。所以本 crate 的 `main` 不读 `std::env::args()`。
//! 有专门的反向测试（`tests/` 里）检查源码里**没有** `env::args`。

pub mod fixtures;
pub mod interp;
pub mod native;
pub mod native_records;
pub mod ssa;
pub mod ssa_needs;
