//! `rgoc-check` —— M4 的静态检查主体。
//!
//! # 职责
//!
//! 输入 = `rgoc-load` 装载出的 [`rgoc_load::Package`]；输出 = [`CheckedPackage`]
//! （**原始诊断流** + `C8` 渲染入口）。有诊断 ⇒ 调用方退出码 1。
//!
//! M4 的检查面：操作数模式与表达式/语句、内建函数与转换、选择器与方法集、
//! 初始化序与环、main/init 约束、版本门控、`unsafe` 白名单、import 可见性缝合。
//! 逐项以 `M4-tests.md` 的 `T-CK-*` 为准；**不做**消息复刻 —— `C8` 的
//! 对撞面只比对「位置 + 类别」（诊断按 [`diag`] 的纪律投影）。
//!
//! # 模块地图
//!
//! | 模块 | 面 |
//! |---|---|
//! | [`check`] | 版本源（Task 18）+ **检查会话**与 [`check_package`]（Task 20 起真身） |
//! | [`operand`] | 操作数：带类型值（`operand.go` 的 11 模式；值层接线） |
//! | [`expr`] | 表达式运算符面（Task 21 起；Task 23 索引 / 切片、Task 24 选择器） |
//! | [`lookup`] | 字段 / 方法查找（Task 24；官方 `lookup.go` 逐条对齐） |
//! | [`selection`] | 选择结果与签名构造（Task 24；方法值 / 方法表达式） |
//! | [`render`] | `--diag-dump` 的 `C8` 渲染（过滤 / 排序 / 字节格式） |
//! | [`diag`] | 原始诊断位（soft）+ 26 类类别投影（全量穷尽） |
//! | [`version`] | 版本门控判定（Task 19） |
//!
//! # 与骨架期的区别（Task 2 → Task 20）
//!
//! Task 2 的占位（恒报 `CHK-UNIMPLEMENTED`、`Result<_, DiagBag>` 返回型）
//! 已随真身落地退役：会话（[`Session`]）接管诊断流的组装与上报，那个
//! **没有触发者**的占位码按五件套纪律从 `DiagCode` 移除（`C8` §10 的
//! R-4-2；`T-CK-16` 快照同步）。

pub mod check;
pub mod diag;
pub(crate) mod env;
pub mod expr;
pub mod lookup;
pub mod operand;
pub mod render;
pub mod selection;
pub mod stmt;
pub mod version;

pub use check::{CheckOptions, CheckedPackage, Session, check_package};
