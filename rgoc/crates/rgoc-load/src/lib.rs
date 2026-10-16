//! `rgoc-load` —— 包模型与装载（M3）。
//!
//! # 位置与依赖方向
//!
//! ```text
//! rgoc-lex ◄── rgoc-ast ◄── rgoc-const ◄── rgoc-types ◄── rgoc-load
//!                                               ▲
//!                              rgoc-lex ◄── rgoc-parse
//! ```
//!
//! 本 crate 在链条末端：它把 `rgoc-parse` 的语法树按**包**组织起来，
//! 建作用域、收声明、解析源码 import，并给出导出摘要。
//!
//! # 包模型的边界（`D-M3-5`，**中等档**）
//!
//! | 做 | **不**做（归谁） |
//! |---|---|
//! | 单包多文件装载 | 构建约束筛选、`_test.go`/cgo/汇编（`U-M3-06`，M4+） |
//! | 源码 import + import 环诊断 | 二进制 export data / 模块下载 / `go.mod`（`U-M3-07`） |
//! | 导出摘要（内存 API） | 导出摘要的**持久化**（`D-M3-18`：只做内存 API） |
//!
//! # 状态
//!
//! M3 逐任务落地：**Task 23 = [`package`]**（单包多文件 + `universe ⊃ package ⊃ file`
//! 三层作用域 + 包名一致性 + 顶层声明名占位 + 文件级 import 名）；
//! **Task 24 = [`decls`] + [`typeexpr`]**（类型/常量求值、前向引用、声明环、
//! 版本表接线、dump 根）。块作用域归 Task 25，源码 import 归 Task 27，
//! 显式文件列表的 CLI 侧归 Task 28，导出摘要归 Task 29。

pub mod decls;
pub mod package;
pub mod scope;
pub mod typeexpr;

pub use decls::collect_decls;
pub use package::{Package, Source, load_from_explicit_files, load_sources};
