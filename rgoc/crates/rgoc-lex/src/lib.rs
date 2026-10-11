//! `rgoc-lex` —— Go 源码的词法分析（M1 阶段）。
//!
//! 把 `.go` 文件的字节流切成**带位置的词元流**，并产出一份**诊断流**。
//! 它不建语法树（M2）、不算字面量的值（M3）、不做任何语义判断（M4）。
//!
//! # 设计依据
//!
//! 本 crate 的形状由 `docs/milestones/M1-design.md` 冻结，关键约束：
//!
//! 1. **不依赖任何其他 `rgoc-*` crate** —— 尤其不得引用 AST/HIR 类型（限界上下文边界）。
//!    反向依赖（`rgoc-driver → rgoc-lex`）是允许且必需的。
//! 2. **分派与判定顺序照官方 `gc/syntax/scanner.go` 的 `next()` 写** ——
//!    但**只照抄语义，不照抄实现**（去掉 sentinel 哨兵、去掉流式增量填充）。
//! 3. **`unsafe_code = "forbid"`**（workspace 级），本 crate 不引入任何 `unsafe`。
//! 4. **不引入第三方依赖** —— Unicode 分类表由 oracle 生成（见 `unicode_tables`）。
//!
//! # 模块
//!
//! | 模块 | 职责 |
//! |---|---|
//! | [`pos`] | 位置表示：`Pos` / `FileId` / `FileTable`（C1 契约） |
//! | [`diag`] | 诊断：`Diag` / `DiagBag` / `DiagCode`（C1 契约） |
//! | [`token`] | 词元数据模型：`Token` / `TokenKind` / `Keyword` / `Operator` / `Delim` … |
//! | [`unicode_tables`] | **生成物**：字符分类区间表（字母 684 / 数字 72），来自 oracle |
//! | [`source`] | 字符层：UTF-8 解码 + 位置推进 + 段切片（**不认识词元**） |
//! | [`lexer`] | 词元层：分派 + 各类词元扫描（T-L08 先落 `ident`） |
//!
//! 后续任务将加入 `semicolon`（分号状态机）、`lexer::next`（分派骨架）。

pub mod diag;
pub mod lexer;
pub mod pos;
pub mod source;
pub mod token;
pub mod unicode_tables;

pub use diag::{Diag, DiagBag, DiagCode};
pub use lexer::{Lexer, PunctScan};
pub use pos::{FileId, FileTable, Pos};
pub use source::SourceReader;
pub use token::{Delim, Keyword, LitKind, OpForm, Operator, SemiOrigin, Token, TokenKind};
