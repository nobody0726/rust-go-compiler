//! lowering 的**拒绝面**（`EX-1` 的判据：未支持构造必须显式拒绝）。
//!
//! # 为什么不是 `todo!()`
//!
//! `M5-design.md` §3.2 的末段把这条写成反向验证：把某个拒绝分支改成
//! `todo!()`，对撞必红 —— 因为进程 abort 的退出码不是 1，而且**没有任何
//! 用户可见的说明**。所以每个「M5 不做」的构造都有一个稳定码：
//!
//! ```text
//! HIR-UNSUPPORTED-RANGE-FUNC   range over func（M5 的窄面；M8/Task 45 起换成 RANGE-FUNC-RETURN）
//! HIR-UNSUPPORTED-GOTO         函数体顶层块以外的 goto（窄面）
//! HIR-UNSUPPORTED-GENERIC      超出单态化面的泛型
//! HIR-MISSING-TYPE             拿不到静态类型（`D-M5-2` 的失效条件）
//! HIR-INCONSISTENT-CHECK       lowering 期会话冒出诊断（检查面不一致）
//! ```
//!
//! # M8 下线了四条
//!
//! `HIR-UNSUPPORTED-{GO,CHAN,SELECT,CLOSE}` 在 M8 全部消失：`go` / `chan` /
//! `select` / `close` 现在真的被 lowering（`M8-plan.md` Task 7–12）。
//! **不许**把它们当「保留字」留着 —— 一个永远不会被产出的码只会让
//! 「哪个构造还不支持」这个问题有四种错误答案。守卫判据：
//! `rgoc-driver/tests/con_lower.rs` 的 `t_task14_四条码不许再出现`。
//!
//! 每个码都对应 `M5-manifest.json` 的一条 `unsupported` 或一条 `open_gap`。

use rgoc_lex::Pos;

/// 拒绝码。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum RejectKind {
    /// `range` over func（`U-M5-06`）。
    RangeFunc,
    /// 顶层块以外的 `goto`（窄面）。
    Goto,
    /// 超出单态化面的泛型（`U-M5-10`）。
    Generic,
    /// 拿不到静态类型（`D-M5-2` 失效条件）。
    MissingType,
    /// 检查面不一致（lowering 期会话冒诊断）。
    InconsistentCheck,
}

impl RejectKind {
    /// 稳定码文本（进 `HIR-*` 命名空间；改动等于契约变更）。
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            RejectKind::RangeFunc => "HIR-UNSUPPORTED-RANGE-FUNC",
            RejectKind::Goto => "HIR-UNSUPPORTED-GOTO",
            RejectKind::Generic => "HIR-UNSUPPORTED-GENERIC",
            RejectKind::MissingType => "HIR-MISSING-TYPE",
            RejectKind::InconsistentCheck => "HIR-INCONSISTENT-CHECK",
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
