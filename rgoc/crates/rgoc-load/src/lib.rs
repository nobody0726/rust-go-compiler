//! `rgoc-load` —— 包模型与装载（M3）。
//!
//! # 位置与依赖方向
//!
//! ```text
//! rgoc-lex ◄── rgoc-const ◄── rgoc-types ◄── rgoc-load
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
//! **M3/Task 2 只建骨架**：[`Package`] 是空 struct，[`load_from_explicit_files`]
//! 是**未被接线**的桩。详见该函数的文档。

use std::path::PathBuf;

use rgoc_lex::DiagBag;

/// 一个已装载的包。
///
/// M3/Task 23 填充（包名、文件表、包作用域、导出摘要……）。
#[derive(Debug)]
pub struct Package;

/// 从一个**显式的文件列表**装载一个包 —— M3 唯一的装载入口。
///
/// # ⚠️ 本桩**刻意不被任何东西调用**
///
/// 它现在返回「空包 + 空诊断」，也就是**「一切正常」**。那个语义在
/// `M3-plan.md` Task 28 之前**没有资格被相信**（装载、作用域、类型检查都还没写）。
///
/// 所以这里不把它接给 `rgoc types`：CLI 此刻**直接显式报错**
/// （见 `rgoc-driver` 的 `types_cmd`）。一个「调了就得到成功」的桩比没有更危险
/// —— 它会让「还没实现」伪装成「检查通过」。Task 28 起它才是真入口。
///
/// 参数 `lang` 是 `-lang go1.N` 的覆盖值；`None` ⇒ 按每个文件自己的
/// `//go:build` 版本（`D-M3-15`：版本门控**按文件**）。
pub fn load_from_explicit_files(files: &[PathBuf], lang: Option<&str>) -> (Package, DiagBag) {
    // 占位实现：明天（Task 28）这里会真的读文件、解析、建作用域。
    let _ = (files, lang);
    (Package, DiagBag::new())
}
