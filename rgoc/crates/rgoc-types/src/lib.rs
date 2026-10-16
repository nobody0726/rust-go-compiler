//! `rgoc-types` —— 类型表示、同一性与按文件的版本门控（M3）。
//!
//! # 位置与依赖方向
//!
//! ```text
//! rgoc-lex ◄── rgoc-const ◄── rgoc-types ◄── rgoc-load
//! ```
//!
//! 单向，无环。本 crate 吃 [`rgoc_const`] 的常量与 [`rgoc_lex`] 的诊断/位置。
//!
//! # `BasicId` / `Sizes` 为什么是**转导出**
//!
//! 它们的**定义**在 [`rgoc_const`]（那里是 `convert_const` / `representable`
//! 的家乡，解环见该 crate 的模块文档）。这里转导出是为了让下游（`rgoc-load`、
//! `rgoc-driver`）能用 `rgoc_types::BasicId` 这个更顺手的路径 ——
//! **不是**第二份定义。
//!
//! # 状态
//!
//! **M3/Task 2 只建骨架**：[`Type`] 是空 enum（造不出值），[`TypeTable`] 是空 struct。
//! 表示（15 个变体，与 `docs/contracts/C7-type-dump.md` §2.1 的标签一一对应）
//! 从 Task 13 开始填。

pub use rgoc_const::{BasicId, ConstId, ConstTable, Sizes};

/// 类型表的一个条目。
///
/// **15 个变体**：`basic` `named` `alias` `pointer` `slice` `array` `map` `chan`
/// `struct` `signature` `tuple` `interface` `typeparam` `union` `invalid`
/// —— 与 `docs/contracts/C7-type-dump.md` §2.1 的标签集**一一对应**，
/// `T-TY-11` 的穷尽性断言同时钉住这个数字。M3/Task 13 填充。
///
/// ⚠️ **表示做宽、判定做窄**（`D-M3-2`）：`typeparam`/`union` 在这个 enum 里
/// **有位置**（要能 dump），但 `Identical` **不判**它们（`U-M3-05`，M4 才解除）。
#[derive(Debug)]
pub enum Type {}

/// 类型表：类型去重、编号，`Type` 之间用下标互相引用（`C7` §2.2 的引用语法）。
///
/// M3/Task 13 填充。
#[derive(Debug)]
pub struct TypeTable;
