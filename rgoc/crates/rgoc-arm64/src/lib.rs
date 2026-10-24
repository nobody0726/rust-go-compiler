//! arm64 代码生成（M7）：SSA → 文本汇编。
//!
//! # 这个 crate 是**纯函数**
//!
//! 输入是 `rgoc_ssa::Module` 与 `rgoc_types::TypeTable`，输出是一段 `.s` 文本。
//! 它**不碰磁盘、不读环境变量、不看时间、不用哈希序** —— 否则
//! `T-NAT-48`（同一份 SSA 两次生成逐字节相同）就不成立，而那条判据是
//! 「产物可复现」的唯一证据（`M7-design.md` `D-M7-14`）。
//!
//! 真正的「写文件 + 调 clang」在 `rgoc-driver` 的 `build_cmd` 里 —— 那是**副作用**，
//! 与本 crate 的职责刻意分开。
//!
//! # 模块按任务逐个建（不留空占位文件）
//!
//! | 模块 | 内容 | 建在 |
//! |---|---|---|
//! | `diag` | `CgCode`（`CG-UNSUPPORTED-*`） | Task 2 |
//! | `emit` | 确定性文本汇编 | Task 3 |
//! | `abi` | 值分类 / 槽宽 / 传参类别 | Task 8 |
//! | `frame` | 帧布局 + `FRAME-MAP v1` | Task 9–11 |
//! | `parallel_copy` | 块参数交接 | Task 12 |
//! | `isel` | 指令选择 | Task 13 起 |
//!
//! 先建空文件再逐个填的做法在本仓是**禁止**的（「不用 `None` 冒充未实现」的同一条纪律）：
//! 一个只有 `//!` 的模块看起来像「已实现」，实际什么都没有。

pub mod abi;
pub mod diag;
pub mod emit;
pub mod frame;
pub mod parallel_copy;
