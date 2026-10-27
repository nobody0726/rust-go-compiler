//! 集成测试的共享装置（只在 `tests/` 下用）。
//!
//! 子目录不会被 cargo 当成独立测试二进制 —— 所以这里放**被 `mod support;` 引入**的代码，
//! 而不是 `tests/support.rs`（那会变成第 N 个测试二进制，跑一遍空测试）。

pub mod gc_probe;
pub mod go_probe;
pub mod native_harness;
