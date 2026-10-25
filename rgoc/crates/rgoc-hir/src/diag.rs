//! lowering 的**拒绝面**（`EX-1` 的判据：未支持构造必须显式拒绝）。
//!
//! # 为什么不是 `todo!()`
//!
//! `M5-design.md` §3.2 的末段把这条写成反向验证：把某个拒绝分支改成
//! `todo!()`，对撞必红 —— 因为进程 abort 的退出码不是 1，而且**没有任何
//! 用户可见的说明**。所以每个「M5 不做」的构造都有一个稳定码：
//!
//! ```text
//! HIR-UNSUPPORTED-RANGEFUNC-RETURN  range over func 的 body 里 return/goto
//!                                    （`U-M8-03`；body 被下沉成合成函数，
//!                                    而 HIR 的 return/goto 是函数作用域的）
//! HIR-UNSUPPORTED-GOTO         函数体顶层块以外的 goto（窄面）
//! HIR-UNSUPPORTED-GENERIC      超出单态化面的泛型
//! HIR-MISSING-TYPE             拿不到静态类型（`D-M5-2` 的失效条件）
//! HIR-INCONSISTENT-CHECK       lowering 期会话冒出诊断（检查面不一致）
//! ```
//!
//! # M8 下线了五条
//!
//! `HIR-UNSUPPORTED-{GO,CHAN,SELECT,CLOSE,RANGE-FUNC}` 在 M8 全部消失：
//! `go` / `chan` / `select` / `close` / `range over func` 现在真的被 lowering
//! （`M8-plan.md` Task 7–12 / Task 45–48）。**不许**把它们当「保留字」留着 ——
//! 一个永远不会被产出的码只会让「哪个构造还不支持」这个问题有错误答案。
//! 守卫判据：`rgoc-driver/tests/con_lower.rs` 的 `t_task14_四条码不许再出现`
//! 与 `con_rangefunc.rs` 的 `t_con_49_第五条码不许再出现`。
//!
//! 每个码都对应 `M5-manifest.json` 的一条 `unsupported` 或一条 `open_gap`。

use rgoc_lex::Pos;

/// 拒绝码。
///
/// 声明顺序 == 变体名字典序（与 `DiagCode::ALL_CATEGORIES` 同一条惯例）——
/// `ALL` 的顺序断言靠它。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum RejectKind {
    /// 超出单态化面的泛型（`U-M5-10`）。
    Generic,
    /// 顶层块以外的 `goto`（窄面）。
    Goto,
    /// 检查面不一致（lowering 期会话冒诊断）。
    InconsistentCheck,
    /// 拿不到静态类型（`D-M5-2` 失效条件）。
    MissingType,
    /// range over func 的 body 里 `return` / `goto`（`U-M8-03`）。
    RangeFuncReturn,
}

impl RejectKind {
    /// 全部码（顺序 == 声明顺序 == 字典序）。
    ///
    /// 为什么必须有：`manifest_m8.rs` 的对账要判「manifest 里写的每个码都真的
    /// 产得出来」。在测试里抄一份清单只能证明「实现与那份副本一致」
    /// （`R-51-1`），所以唯一来源是这里。
    pub const ALL: [RejectKind; 5] = [
        RejectKind::Generic,
        RejectKind::Goto,
        RejectKind::InconsistentCheck,
        RejectKind::MissingType,
        RejectKind::RangeFuncReturn,
    ];

    /// 稳定码文本（进 `HIR-*` 命名空间；改动等于契约变更）。
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            RejectKind::Generic => "HIR-UNSUPPORTED-GENERIC",
            RejectKind::Goto => "HIR-UNSUPPORTED-GOTO",
            RejectKind::InconsistentCheck => "HIR-INCONSISTENT-CHECK",
            RejectKind::MissingType => "HIR-MISSING-TYPE",
            RejectKind::RangeFuncReturn => "HIR-UNSUPPORTED-RANGEFUNC-RETURN",
        }
    }
}

/// 一条拒绝。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HirReject {
    /// 报点。
    pub pos: Pos,
    /// 码。
    pub kind: RejectKind,
    /// 人读说明（**不含**码 —— 渲染时拼）。
    pub message: String,
}

impl HirReject {
    /// 造一条。
    #[must_use]
    pub fn new(pos: Pos, kind: RejectKind, message: impl Into<String>) -> Self {
        Self {
            pos,
            kind,
            message: message.into(),
        }
    }

    /// 渲染成 `{file}:{line}:{col}: <msg> [<CODE>]`。
    #[must_use]
    pub fn render(&self, table: &rgoc_lex::FileTable) -> String {
        format!(
            "{}: {} [{}]",
            table.format_pos(self.pos),
            self.message,
            self.kind.code()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::RejectKind;

    /// 「变体按字典序 + ALL 全收 + 码不重复」三件事一条判据（`DiagCode` 同款）。
    ///
    /// 漏一条 ⇒ `manifest_m8.rs` 的对账会把合法码判成非法；多一条没人用 ⇒
    /// 计数测试红（`t_task14_五条码不许再出现` 从另一侧守同一件事）。
    #[test]
    fn all_与字典序自洽() {
        let mut sorted = RejectKind::ALL;
        sorted.sort_unstable();
        assert_eq!(sorted, RejectKind::ALL, "ALL 必须按字典序（顺序进断言）");
        let texts: Vec<&str> = RejectKind::ALL.iter().map(|c| c.code()).collect();
        let mut uniq = texts.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), texts.len(), "码文本不许重复");
        assert!(
            texts
                .iter()
                .all(|t| t.starts_with("HIR-UNSUPPORTED-") || t.starts_with("HIR-")),
            "全部进 HIR- 命名空间：{texts:?}"
        );
        assert_eq!(RejectKind::ALL.len(), 5, "改数量要同步 M8-tests §5 的白名单");
    }
}
