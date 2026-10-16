//! `rgoc-const` —— 常量表示、折叠与可表示性（M3）。
//!
//! # 位置与依赖方向（`D-M3-8` / `D-M3-16`）
//!
//! ```text
//! rgoc-lex ◄── rgoc-ast ◄── rgoc-const ◄── rgoc-types ◄── rgoc-load
//! ```
//!
//! 本 crate 依赖 `rgoc-lex`（诊断与位置）与 `rgoc-ast`（字面量节点，
//! `const_from_literal(&BasicLit)`），且 **不得依赖 `rgoc-types`**。
//! `rgoc-ast` 这条边**无环**：AST 层不看常量。
//!
//! 这条方向是 `D-M3-16` 说的「解环」逼出来的：`convert_const()` 与 `representable()`
//! 住在**本 crate**，而它们要吃 `BasicId` + `Sizes`。若把这两个类型放在
//! `rgoc-types`，本 crate 就得反向依赖它，而 `rgoc-types` 又依赖本 crate
//! ⇒ **成环**（Cargo 直接拒绝构建）。
//!
//! ⇒ **`BasicId` / `Sizes` 的定义落在这里**，`rgoc-types` 只做转导出
//! （`pub use rgoc_const::{BasicId, Sizes}`）。`M3-plan.md` Task 14 的
//! 「`BasicId` 在 `rgoc-types`」按此口径执行 —— 它是**转导出**，定义仍在本地。
//!
//! # 状态
//!
//! M3 的常量核心已经落地：`BigInt`/`BigRat`（Task 4）、`Const` 与字面量
//! （Task 5）、折叠（Task 6–7）、`BasicId`/`Sizes`/转换（Task 8）。

pub mod basic;
pub mod bigint;
pub mod bigrat;
pub mod convert;
pub mod eval;
pub mod fold;
pub mod iota;
pub mod representable;
pub mod sizes;
pub mod value;

/// 基本类型标识（25 个）—— 定义在 [`basic`]，`rgoc-types` 转导出。
pub use basic::BasicId;

/// 平台尺寸（`int`/`uint`/`uintptr` 的字节宽）—— 定义在 [`sizes`]。
pub use sizes::Sizes;

/// 无类型常量的默认类型与常量转换（定义在 [`convert`]）。
pub use convert::{convert_const, default_type_of, untyped_default_type};

/// 任意精度整数（常量与数组长度的底座）。M3/Task 4 填充。
///
/// 定义在 [`bigint`]，这里转导出。
pub use bigint::BigInt;

/// **精确**有理数 —— 浮点常量的表示（`D-M3-4`：不引入实现限制）。
///
/// 只在**转换**到具体浮点类型时才做「向偶数舍入」，折叠过程本身是精确的。
/// 定义在 [`bigrat`]，这里转导出。
pub use bigrat::{BigRat, RoundOutcome, RoundResult};

/// 一个常量值（定义在 [`value`]）。
pub use value::{Const, const_from_literal};

/// 常量折叠（定义在 [`fold`]）。
pub use fold::{fold_binary, fold_compare, fold_unary};

/// 常量表达式求值（定义在 [`eval`]）—— 只看值、不看类型。
pub use eval::{ConstEnv, EvalOutcome, eval_const_expr};

/// `iota` 与常量声明组展开（定义在 [`iota`]）。
pub use iota::expand_const_group;

/// 可表示性判定（定义在 [`representable`]）。
pub use representable::{Represented, representable};

/// 常量在 [`ConstTable`] 里的下标。
///
/// **为什么要有下标这一层**：`BigInt` 不是 `Copy`，而 `rgoc_types::Type`
/// 必须保持廉价可拷贝（类型表要能整体复制/比较）⇒ 类型表里凡引用常量的地方
/// （如 `array` 的长度）只存下标，常量本体集中在 [`ConstTable`]。
/// M3/Task 5 填充。
#[derive(Debug)]
pub struct ConstId;

/// 常量表。M3/Task 5 填充。
#[derive(Debug)]
pub struct ConstTable;
