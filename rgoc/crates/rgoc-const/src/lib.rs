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
//! **M3/Task 2 只建骨架。** 本文件里的类型都是**不可构造的占位**
//! （空 enum / 空 struct）：它们让骨架能编译，但**造不出值**，
//! 所以「还没实现」在类型层面就成立。真正的表示与算法从 Task 4
//! （`BigInt`/`BigRat`）与 Task 5（`Const`）开始填。

pub mod bigint;
pub mod bigrat;
pub mod value;

/// 基本类型的标识（26 个）。
///
/// 全集与拼写写在 `docs/contracts/C7-type-dump.md` §4.1（`T-TY-12` 的
/// 计数断言与之对账）。M3/Task 14 填充变体。
#[derive(Debug)]
pub enum BasicId {}

/// 目标平台的整数/指针字长规格。
///
/// `int` / `uint` / `uintptr` / 指针宽度是**平台相关**的，而常量可表示性判定
/// （`Representable`）必须知道它们 —— 这是官方 `types2/const.go:76`
/// 的 `representableConst(x, check, typ, rounded)` 里 `check` 那一半的作用。
///
/// M3/Task 8（首个消费者）填充，宿主值 `HOST_ARM64` 亦在那里给出。
#[derive(Debug)]
pub struct Sizes;

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
