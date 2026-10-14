//! `rgoc-parse` —— Go 的语法分析（parser，M2 阶段）。
//!
//! 把 M1 吐出的**线性词元流**重建为 **AST 树**（`rgoc-ast`），并接住 M1 推迟的
//! 位置重映射、错误恢复、指令解析。
//!
//! # 设计依据
//!
//! `docs/milestones/M2-design.md`：手写递归下降，骨架对齐官方
//! `cmd/compile/internal/syntax/parser.go`（单前瞻 `tok`、状态 `top`/`fnest`/`xnest`、
//! 错误恢复 `advance`+`stopset`）。
//!
//! # 模块
//!
//! | 模块 | 职责 |
//! |---|---|
//! | [`parser`] | 状态机 + 各产生式的递归下降 |
//! | [`recovery`] | 错误恢复机制（`advance`/`followset`/`stopset`/`errh` 双档）—— 四个函数合起来是**一个机制**，不拆散 |
//! | [`stmt`] | 语句族产生式（`stmtOrNil` 分派 / `stmtList` / `blockStmt`）—— 它们共用「语句边界」这一套约定，拆开会让恢复行为散落 |

pub mod parser;
pub mod recovery;
pub mod stmt;

pub use parser::*;
pub use recovery::STOPSET;
