//! `rgoc-meta` —— 类型元数据（M9）。
//!
//! # 位置
//!
//! ```text
//! rgoc-types ◄── rgoc-meta ◄── rgoc-arm64
//! ```
//!
//! 本 crate 是 `TypeDesc` / `MethodTable` / ptrmask 的**唯一**实现：
//! 后端发射 `.rodata` 读它，M10 的扫描器按同一份布局读内存。
//!
//! # 为什么单独一个 crate
//!
//! `TypeDesc` 是给 M10 消费的**协议**，而扫描器不依赖指令集。
//! 放进 `rgoc-arm64` 会把协议绑在 aarch64 上。
//!
//! # 状态
//!
//! M9 逐任务落地：Task 1 = [`desc`]（`TypeDesc`），Task 3 = `needs_zero`，
//! Task 4 = `type_hash`，Task 27 = `MethodTable`。

pub mod desc;
