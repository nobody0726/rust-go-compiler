//! SSA 的**拒绝面**（`M6-tests.md` §5 的白名单；`SSA-7` 的判据）。
//!
//! # 为什么这些码**不进** `DiagCode`（`D-M6-14`）
//!
//! `rgoc-lex::DiagCode` 的每个码都必须被 `rgoc-check/tests/category.rs` 投影到
//! `C8` 冻结的 **26 类**之一，而其中 `other` 的值域被 `T-CK-16` 钉死为「恰好 3 个
//! 实现限制码」。把 7 个 SSA 拒绝码硬塞进去只有两条路：让 `other` 漂（破 `C8` +
//! 破 `T-CK-16`），或者给它们编一个假类别（用类别答错）。**两条都不行。**
//!
//! 所以走 `rgoc_hir::RejectKind` 的同一条路：crate 自己一个 `SsaCode`，
//! 码文本进 `SSA-*` 命名空间，`DiagCode` 保持 103 不动。
//!
//! # 为什么 `Reject` 没有位置字段
//!
//! HIR 的 `Expr` / `Stmt` **不带位置**（位置只在 lowering 期的 AST 上有）。
//! 计划稿写的是 `pos: Option<rgoc_lex::Pos>`，但 SSA 层拿不到它 —— 与其留一个
//! **永远是 `None`** 的字段（读的人会以为「有时有」），不如换成 `detail`：
//! 说清是哪个构造、有几个捕获变量之类。修订行 `R-17-1`。

use rgoc_hir::FuncId;

/// SSA 构建期的拒绝码（**7 条**）。
///
/// # 下线的码（不是「忘了写」）
///
/// | 码 | 何时下线 | 为什么 |
/// |---|---|---|
/// | `SSA-UNSUPPORTED-MAP` | M9/Phase 2（`R-9-8`） | `rt_map_*` + `CalleeRef::Runtime` 实现了 |
/// | `SSA-UNSUPPORTED-IFACE-DYN` | M9/Phase 3（`R-9-14`） | 方法表 + 静态 ITab + `Instr::TypeAssert` 实现了派发与 type switch |
/// | `SSA-UNSUPPORTED-RANGE-FUNC` | M8/Task 45 | HIR 把 range-over-func 下降成合成函数，拒绝改走 `FUNC-VALUE` |
///
/// 纪律是「**不留永远不产出的码**」：一条没人能触发的拒绝码，读的人会以为
/// 「有这个限制」，而实际上它只是一段死分支。
///
/// 变体按**字典序**声明（沿 `DiagCode` 的纪律：顺序进断言，不许重排）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum SsaCode {
    /// `chan` / `go` / `select`。
    Chan,
    /// 闭包（捕获变量写外层根）。
    Closure,
    /// `defer` / `recover`。
    Defer,
    /// 函数值（`func(){}` / 方法值 / 间接调用）—— `R-31-1`。
    FuncValue,
    /// 泛型实例化后的形态。
    Generic,
    /// `map` 的读写（键相等是宿主语义）。
    Map,
    /// `range` over string（要按 rune 解码，指令集里没有）—— `R-31-1`。
    RangeStr,
}

impl SsaCode {
    /// 全部码（顺序 == 声明顺序 == 字典序）。
    pub const ALL: [SsaCode; 7] = [
        SsaCode::Chan,
        SsaCode::Closure,
        SsaCode::Defer,
        SsaCode::FuncValue,
        SsaCode::Generic,
        SsaCode::Map,
        SsaCode::RangeStr,
    ];

    /// 稳定码文本（进 `SSA-*` 命名空间；改动等于契约变更）。
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            SsaCode::Chan => "SSA-UNSUPPORTED-CHAN",
            SsaCode::Closure => "SSA-UNSUPPORTED-CLOSURE",
            SsaCode::Defer => "SSA-UNSUPPORTED-DEFER",
            SsaCode::FuncValue => "SSA-UNSUPPORTED-FUNC-VALUE",
            SsaCode::Generic => "SSA-UNSUPPORTED-GENERIC",
            SsaCode::Map => "SSA-UNSUPPORTED-MAP",
            SsaCode::RangeStr => "SSA-UNSUPPORTED-RANGE-STR",
        }
    }
}

/// 一次拒绝（**构建期**发生，不是运行期降级）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reject {
    /// 出问题的函数（HIR 编号）。
    pub func: FuncId,
    /// 拒绝码。
    pub code: SsaCode,
    /// 人读说明（必须带具体构造，否则用户只知道「不支持」不知道「哪一句」）。
    pub detail: String,
}

impl Reject {
    /// 渲染成**一行** stderr（`SSA-7`：退出码 1 + 稳定码 + 进程不 abort）。
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "{}: func#{} — {}",
            self.code.code(),
            self.func.0,
            self.detail
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Reject, SsaCode};
    use rgoc_hir::FuncId;

    #[test]
    fn 码总数与字典序自洽() {
        assert_eq!(
            SsaCode::ALL.len(),
            7,
            "§5 的 7 条 + R-31-1 补的 2 条 − 下线的三条（RANGE-FUNC / MAP / IFACE-DYN）"
        );
        let mut sorted = SsaCode::ALL;
        sorted.sort_unstable();
        assert_eq!(sorted, SsaCode::ALL, "ALL 必须按字典序（顺序进断言）");
        let texts: Vec<&str> = SsaCode::ALL.iter().map(|c| c.code()).collect();
        let mut uniq = texts.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), texts.len(), "码文本不许重复");
        assert!(
            texts.iter().all(|t| t.starts_with("SSA-UNSUPPORTED-")),
            "全部进 SSA-UNSUPPORTED- 命名空间"
        );
    }

    #[test]
    fn 拒绝渲染成一行且带函数号() {
        let r = Reject {
            func: FuncId(3),
            code: SsaCode::Closure,
            detail: "捕获 2 个变量".to_string(),
        };
        let line = r.render();
        assert_eq!(line, "SSA-UNSUPPORTED-CLOSURE: func#3 — 捕获 2 个变量");
        assert!(!line.contains('\n'), "stderr 一行（T-SSA-33 的判据）");
    }
}
