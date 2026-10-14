//! `rgoc-ast` —— Go 的抽象语法树（AST）类型定义（M2 阶段）。
//!
//! 只承载**结构 + 位置**：节点 = 语法构造，边 = 包含关系，每节点带起点 `Pos`。
//! **不含任何 parser 逻辑**（`M2-design.md` §4 / G16）—— parser 在 `rgoc-parse`。
//!
//! 设计依据：`docs/milestones/M2-design.md` §3（数据模型）；
//! `docs/01-feature-set.md` 的 `AST-01..49` 映射列。
//!
//! # 三族和类型
//!
//! | 族 | 对应官方 | 说明 |
//! |---|---|---|
//! | [`Decl`] | `Decl` 接口 | 五种声明 |
//! | [`Expr`] | `Expr` 接口 | **含类型族**（G18：parser 阶段类型与表达式同族） |
//! | [`Stmt`] | `Stmt` 接口 | 语句；错误处返回 `Option`（官方无 `BadStmt`，G19） |
//!
//! [`SimpleStmt`] 独立成 enum（`if`/`for`/`switch` 的 init 位专用）。

pub mod aux;
pub mod decl;
pub mod dump;
pub mod expr;
pub mod stmt;
pub mod walk;

pub use aux::*;
pub use decl::*;
pub use dump::*;
pub use expr::*;
pub use stmt::*;
pub use walk::*;
