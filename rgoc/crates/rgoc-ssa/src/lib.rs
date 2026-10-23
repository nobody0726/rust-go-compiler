//! `rgoc-ssa` —— SSA 中间表示（M6）。
//!
//! 设计见 `docs/milestones/M6-design.md` §3；判据见 `docs/milestones/M6-tests.md`。
//!
//! # 本 crate 的边界
//!
//! **纯数据，不执行**（`D-M6-7`）：IR、构建、CFG、支配、verifier、优化、dump、活跃性都在这里；
//! 「跑 SSA」在 `rgoc-interp::ssa_exec` —— 值模型只有一个 crate，第三套 runtime 在依赖图上
//! 不可能出现。
//!
//! # 当前进度（Task 2 骨架）
//!
//! 只有 ID 与 `dump` 的头行。完整 IR 形状在 Task 7 落地，构建在 Task 17 起，
//! verifier 在 Task 10 起。**骨架期不许假装支持**：`rgoc-driver ssa` 会明确拒绝
//! （见 `ssa_cmd::run_ssa`）。
#![deny(missing_docs)]

pub mod cfg;
pub mod dom;
pub mod dump;
pub mod ir;
pub mod testsupport;
pub mod verify;
