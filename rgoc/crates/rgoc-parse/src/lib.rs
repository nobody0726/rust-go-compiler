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
//! | [`decl`] | 声明族（`appendGroup` + import/const/type/var）—— 它们共用「分组括号」与「说明到此为止」两套约定 |
//! | [`expr`] | 表达式族（`expr`/`binaryExpr`/`unaryExpr`/`operand`/`pexpr` + 两个列表产生式）—— 优先级爬升与后缀链**只有一处** |
//! | [`type_`] | 类型族（`type_`/`typeOrNil`）—— 与表达式族同族（`G18`），但**分派入口**独立 |
//! | [`stmt`] | 语句族产生式（`stmtOrNil` 分派 / `stmtList` / `blockStmt` / `funcBody`）—— 它们共用「语句边界」这一套约定（`funcBody` 还决定 stopset **何时**生效），拆开会让恢复行为散落 |
//! | [`text`] | 官方 `String()` 的**最小组件子集** —— 只为 `header` 的错误消息服务（`D-M2-7` 排除完整 printer，见该模块文档的边界说明） |
//! | [`directive`] | `//line` 位置重映射 + `//go:` 指令（`updateBase` / `PragmaHandler` / `goVersion`）—— 官方把两件事塞在**同一个** scanner 回调里，拆开就要把 `col` 偏移量抄两遍 |

pub mod branches;
pub mod decl;
pub mod directive;
pub mod expr;
pub mod parser;
pub mod recovery;
pub mod stmt;
pub mod text;
pub mod type_;

pub use parser::*;
pub use recovery::STOPSET;
