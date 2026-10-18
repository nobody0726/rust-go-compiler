//! `rgoc-check` 的诊断：构造 + （Task 20 起的）类别投影。
//!
//! # 两面
//!
//! - **构造**（本任务）：`CHK-` 族代号的 [`Diag`] 构造 —— 位置一律由检查阶段
//!   自己算（`Pos` 从 `rgoc-lex` 来；`CHK-` 码的枚举落位在
//!   `rgoc-lex::diag::DiagCode`，与 `LEX-`/`SYN-`/`TYP-` 同表）。
//! - **类别投影**（Task 20 落）：[`DiagCode`] → `C8` §3 的 26 类枚举，
//!   **全量穷尽**（编译期 `match` 无 `_` 通配 + 计数断言），投影表不含 `Other`。
//!   进 `--diag-dump`（对撞面）的每条诊断都必须能投影 —— 投影不进去是
//!   **设计事故**，而不是「无类别」（守卫见 `M4-tests.md` §4.2 / `T-CK-16`）。

use rgoc_lex::{Diag, DiagCode, FileId, Pos};

/// 占位诊断：`CHK-UNIMPLEMENTED` —— 「检查器尚未实现」。
///
/// 挂在 `anchor` 的 `1:1`。这条代号在骨架期由
/// `rgoc-driver` 的 `check_未实现期_稳定失败且可判读` 触发
/// （五件套纪律：没有触发者的码不许进枚举）。
pub(crate) fn unimplemented(anchor: FileId) -> Diag {
    Diag {
        pos: Pos {
            base: None,
            file: anchor,
            line: 1,
            col: 1,
        },
        code: DiagCode::ChkUnimplemented,
        message: "检查器尚未实现（M4 骨架期）：`rgoc check` 当前只提供命令形态与稳定失败".into(),
    }
}
