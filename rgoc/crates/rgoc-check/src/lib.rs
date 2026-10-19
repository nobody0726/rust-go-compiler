//! `rgoc-check` —— M4 的静态检查主体。
//!
//! # 职责
//!
//! 输入 = `rgoc-load` 装载出的 [`Package`]；输出 = [`CheckedPackage`]（诊断流 +
//! 渲染入口）或 `DiagBag`（有诊断 ⇒ 调用方退出码 1）。
//!
//! M4 的检查面：操作数模式与表达式/语句、内建函数与转换、选择器与方法集、
//! 初始化序与环、main/init 约束、版本门控、`unsafe` 白名单、import 可见性缝合。
//! 逐项以 `M4-tests.md` 的 `T-CK-*` 为准；**不做**消息复刻 —— `C8` 的
//! 对撞面只比对「位置 + 类别」（诊断按 [`diag`] 的纪律投影）。
//!
//! # 这一版（Task 2）是**占位**
//!
//! [`check_package`] 恒返回一条稳定诊断 `CHK-UNIMPLEMENTED`（指 `anchor` 的
//! 1:1）。作用是让 `rgoc check` **响亮失败**而不是静默通过 —— 静默失败是本仓
//! 反复记录的最坏形状；Task 20 起换真身（`check.rs` / `operand.rs` / …）。

pub mod check;
pub mod diag;
pub mod version;

use rgoc_lex::{DiagBag, FileId};
use rgoc_load::Package;

/// 检查结果（**Task 2 占位形态**；Task 20 的真身在此携带检查产物与渲染入口）。
#[derive(Debug, Default)]
pub struct CheckedPackage {
    /// 被检查的文件数（占位期唯一可观察值）。
    pub files_checked: usize,
}

/// 检查一个**已装载**的包。
///
/// `anchor` = 占位诊断挂靠的文件（由调用方选第一个入口文件；即使装载把入口
/// 文件丢掉，`FileTable` 里也有一个可指的名字 —— 诊断位置不许无主）。
///
/// **Task 2 占位**：恒返回 `Err`（一条 `CHK-UNIMPLEMENTED`，见 [`diag`]）。
/// Task 20 起为真身。
pub fn check_package(_pkg: &Package, anchor: FileId) -> Result<CheckedPackage, DiagBag> {
    let mut bag = DiagBag::new();
    bag.push(diag::unimplemented(anchor));
    Err(bag)
}
