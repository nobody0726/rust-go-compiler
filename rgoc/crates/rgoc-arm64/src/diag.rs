//! 生成期拒绝码（`M7-design.md` `D-M7-13`；`M7-tests.md` §5 白名单）。
//!
//! # 为什么是 crate-local，不进 `DiagCode`
//!
//! 与 `rgoc-hir::RejectKind` / `rgoc_ssa::diag::SsaCode` 同一条路：`DiagCode` 的每个码
//! 都必须被 `rgoc-check/tests/category.rs` 投影到 `C8` 冻结的 26 类之一，而 `other` 的值域
//! 被 `T-CK-16` 钉死为「恰好 3 个实现限制码」。把 `CG-UNSUPPORTED-*` 塞进去只有两条路：
//! 让 `other` 的值域漂，或给它们编一个假类别 —— **两条都不行**。
//! 所以码文本进 `CG-*` 命名空间，`DiagCode` 保持 **103**。
//!
//! # 为什么 M9 起只剩两条
//!
//! `CG-UNSUPPORTED-SLICE` 在 M9/Phase 1 下线：切片真的被选择（`make` /
//! `Index` / `Slice` / `append` / 字面量）。守卫判据是 `native_slice.rs` 的
//! 「源码里扫不到这条码」—— 留着一条永远不产出的码，会让「哪个面还不支持」
//! 有一个错误答案。
//!
//! # 为什么原本只有三条
//!
//! 白名单里的另外一条 `U-M7-04`（arena 耗尽）是**运行期**事件：它的出口是
//! `panic: out of arena` + 退出码 2（`C9` §5），不是生成期的一张诊断。把运行期事件
//! 编成「码」会让 `CgCode::ALL` 里混进一个永远不会在编译期出现的成员 ——
//! 那正是「拒绝必须有码」这条纪律要防的反面（`M7-tests.md` §4.7）。

use std::fmt;

/// 生成期的拒绝码。
///
/// 三条都对应 `M7-tests.md` §5 的白名单项；其中 `IndirectCall` / `MethodValue`
/// 在本前端**够不着**（M6 的 `U-M6-03/08` 先拒），保留码是为了契约稳定 ——
/// 它们的正向证据是**守卫测试**（断言该码不出现、拒绝来自 M6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CgCode {
    /// 间接调用：`CalleeRef::Indirect`（`U-M7-02`）。
    ///
    /// 函数值在 M6 就被拒（`SSA-UNSUPPORTED-FUNC-VALUE`），native 见不到。
    IndirectCall,
    /// 方法值 / 接口动态派发（`U-M7-03`）。
    ///
    /// 同 `U-M6-03`，上游先拒。
    MethodValue,
}

impl CgCode {
    /// 全部码（**顺序即字典序**，`T-NAT-52` 用它枚举）。
    ///
    /// 新增一个变体而不更新这里 ⇒ `tests/diag.rs` 的计数测试立刻红。
    pub const ALL: [Self; 2] = [Self::IndirectCall, Self::MethodValue];

    /// 稳定的码文本（进 stderr 的就是这个）。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IndirectCall => "CG-UNSUPPORTED-INDIRECT-CALL",
            Self::MethodValue => "CG-UNSUPPORTED-METHOD-VALUE",
        }
    }
}

impl fmt::Display for CgCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
