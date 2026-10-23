//! # `rgoc-hir-spike` —— **SPIKE-ONLY**，已被 M5 替换（现名 `rgoc-hir-spike`）
//!
//! **⚠️ SPIKE-ONLY：本 crate 不是 rgoc 的正式 HIR。** 正式 HIR 是 `rgoc-hir`
//! （M5 新建：`program/stmt/expr/place/lower/dump/diag`）。
//!
//! 本 crate 原名就叫 `rgoc-hir`。**M5 的 `D-M5-4` 把它整体改名为 `rgoc-hir-spike`**，
//! 腾出 `rgoc-hir` 这个名字给正式实现 —— 正是 `D-M0-11` 预留的退场路径
//! （「M5 替换时可整块删」）。保留它而不是删掉，是因为三个 spike（`T-S1/S2/S3-*`）
//! 仍以它为输入，删了等于把 M0 的三条证据链一起删了。
//!
//! 依据 `docs/milestones/M0-design.md` 的决策 **D-M0-11**：「引入，但只含三个 spike 共享的
//! 最小 HIR」。理由是三个 spike（S1 解释 / S2 SSA / S3 native）必须**共用同一份 HIR 形状** ——
//! 否则「固定 HIR」会漂移成三份互不相干的临时数据结构，跨路线的交叉验证
//! （`T-S2-01` 必须与 `T-S1-01` 结果一致）就失去意义。
//!
//! ## 本 crate **不做**什么（这些是最容易被误读的地方）
//!
//! 1. **不做真实 Go 源码的 lex / parse**。这是 **U12**（`M0-tests.md` §6 的 unsupported 清单），
//!    lexer 与 parser 分别是 **M1 / M2** 的活。这里的 HIR 全部由**手写构造器**产生。
//! 2. **不实现类型系统、不做常量折叠的完整规则、不做逃逸分析**。本文件里出现的
//!    `Const::eval` 只覆盖三个 fixture 需要的子集，遇到子集外的形状**显式返回 `None`**
//!    —— 而不是猜一个结果。
//! 3. **C1 契约在*本 crate* 只是 spike 级副本，不假装已实现**。位置表示（[`Pos`]）与
//!    诊断排序规则（[`Diag`]）在这里定下来，是为了让三个 spike 能把「错误从哪儿来」
//!    记清楚；**这不代表本 crate 有源码位置跟踪** —— C1 已于 M1 升为**完整**级，
//!    正式实现（`FileId` / `FileTable` / `DiagCode`）在 `rgoc-lex`；
//!    本 crate 的副本随 M5 替换而退场。
//!
//! ## 三个 spike 各自从中取什么
//!
//! | spike | 用到本 crate 的什么 |
//! |---|---|
//! | S1 解释（`T-S1-*`） | [`Const`] 的任意精度求值 + [`Stmt::Print`] 的内建调用边界 |
//! | S2 SSA（`T-S2-*`） | 同一份 [`Program`]，走 Block/Value 构造（**求值结果必须与 S1 逐字节相同**）|
//! | S3 native（`T-S3-*`） | [`FuncDecl`] 的固定函数体 → arm64 汇编 |
//!
//! ## 为什么值表示在这里就要做对（T-S1-03 的实测依据）
//!
//! 容器内 go1.27.1 实测（证据见 `M0-tests.md` §5.1 的修订 R1）：
//!
//! ```text
//! ./p.go:7:10: cannot use big (untyped int constant 1267650600228229401496703205376)
//!            as int value in argument to built-in println (overflows)
//! ```
//!
//! 含义是 **untyped constant 在被赋给具体类型时���按该类型收敛**。
//! 于是 `1 << 100` 这个常量在 HIR 里必须**先以任意精度存在**（[`Const::Big`]），
//! 直到内建调用的目标类型确定下来才截断。若本 crate 一开始就用 `i64` 存常量，
//! 那么「常量折叠」这一步就会**静默丢位**，`1 << 100` 直接变成一个错的数 ——
//! 而且测试很可能照样通过（因为 `1 + 2` 这种小值是对的）。
//! 这就是 T-S1-03 说的「值表示不能只在 64 位上凑巧成立」。
//!
//! 对照组是 **T-C-04**（`go_source_code/test/printbig.go`）的冻结期望
//! `-9223372036854775808\n9223372036854775807\n` —— 边界是 `int64` 极值。

#![deny(missing_docs)]

pub mod diag;
pub mod hir;
pub mod value;

pub use diag::{Diag, DiagBag, Pos};
pub use hir::{BinOp, Const, Expr, FuncDecl, Program, Stmt, Stream};
pub use value::{BigInt, FormatError, Val};
