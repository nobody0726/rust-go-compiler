//! `rgoc-interp` —— 顺序解释器（M5）。
//!
//! 设计见 `docs/milestones/M5-design.md` §3.4–§3.8。核心约束（`D-M5-1`）：
//! `step_*` 一律**不递归**，中间状态全部落在显式帧栈 [`interp::Frame`]、
//! 显式求值栈 [`interp::G::eval`] 与值栈 [`interp::G::values`] 上，
//! 以便 M8 的 goroutine 直接保存/恢复。
#![deny(missing_docs)]

pub mod chan;
pub mod interp;
pub mod ops;
pub mod print;
pub mod ssa_exec;
pub mod value;

pub use interp::{Options, Outcome, run};
pub use ssa_exec::run_ssa;
pub use value::{Cell, Value, cell};
