//! `rgoc-hir` —— **正式的 HIR**（M5 起替代 spike 版）。
//!
//! # 位置
//!
//! ```text
//! rgoc-load ─► rgoc-check（诊断 + 类型）─► rgoc-hir（本 crate）─► rgoc-interp
//! ```
//!
//! 本 crate 吃「**已通过检查**的包 + 同一个检查会话」，产出可执行的
//! [`Program`]。它**不**推导类型（`D-M5-2`）：每个节点的 `typ` 都来自
//! `rgoc_check::Session` 的操作数。
//!
//! # 形状
//!
//! | 模块 | 内容 |
//! |---|---|
//! | [`types`] | `Program` / `Func` / `Stmt` / `Expr` / `Place`（`M5-design.md` §3.2） |
//! | [`diag`] | 稳定拒绝码 `HIR-UNSUPPORTED-*` 与 `HirReject` |
//! | [`lower`] | AST + `Session` → `Program` |
//! | [`dump`] | 确定性文本 dump（`--dump-hir` / `T-EX-04`） |
//!
//! # 三条与设计文档的**偏离**（都在 [`types`] 的模块文档里逐条说明）
//!
//! 1. `Expr` 是 `{ kind, typ }` 结构体而不是裸 enum —— 每个表达式都带静态类型；
//! 2. `Program::init_order` 是 `Vec<InitGroup>`（一个 `var a, b = f()` 是一组）；
//! 3. `Stmt::Empty` 与 `Expr::Nil` 是为了「显式拒绝 + 显式零值」而加的两个变体。

#![deny(missing_docs)]

pub mod diag;
pub mod dump;
pub mod lower;
pub mod types;

pub use diag::{HirReject, RejectKind};
pub use dump::dump_program;
pub use lower::lower;
pub use types::{
    BinOp, Block, BranchKind, BuiltinKind, Callee, Capture, Case, Composite, CompositeItem, Expr,
    ExprKind, Func, FuncId, Global, GlobalId, InitGroup, MakeKind, MethodRef, Place, Program,
    RangeKind, SlotId, Stmt, TypeCase, UnOp,
};
