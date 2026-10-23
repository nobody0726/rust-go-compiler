//! lowering 的**拒绝面**（`EX-1` 的判据：未支持构造必须显式拒绝）。
//!
//! # 为什么不是 `todo!()`
//!
//! `M5-design.md` §3.2 的末段把这条写成反向验证：把某个拒绝分支改成
//! `todo!()`，对撞必红 —— 因为进程 abort 的退出码不是 1，而且**没有任何
//! 用户可见的说明**。所以每个「M5 不做」的构造都有一个稳定码：
//!
//! ```text
//! HIR-UNSUPPORTED-GO           go 语句
//! HIR-UNSUPPORTED-CHAN         chan 类型 / 通道操作
//! HIR-UNSUPPORTED-SELECT       select 语句
//! HIR-UNSUPPORTED-RANGE-FUNC   range over func（PIPE-16b）
//! HIR-UNSUPPORTED-CLOSE        close 内建
//! HIR-UNSUPPORTED-GOTO         函数体顶层块以外的 goto（窄面）
//! HIR-UNSUPPORTED-GENERIC      超出单态化面的泛型
//! HIR-MISSING-TYPE             拿不到静态类型（`D-M5-2` 的失效条件）
//! HIR-INCONSISTENT-CHECK       lowering 期会话冒出诊断（检查面不一致）
//! ```
//!
//! 每个码都对应 `M5-manifest.json` 的一条 `unsupported` 或一条 `open_gap`。

use rgoc_lex::Pos;

/// 拒绝码。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum RejectKind {
    /// `go` 语句（`U-M5-05`）。
    Go,
    /// 通道类型 / 通道操作（`U-M5-05`）。
    Chan,
    /// `select` 语句（`U-M5-05`）。
    Select,
    /// `range` over func（`U-M5-06`）。
    RangeFunc,
    /// `close` 内建（`U-M5-12`）。
    Close,
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
            RejectKind::Go => "HIR-UNSUPPORTED-GO",
            RejectKind::Chan => "HIR-UNSUPPORTED-CHAN",
            RejectKind::Select => "HIR-UNSUPPORTED-SELECT",
            RejectKind::RangeFunc => "HIR-UNSUPPORTED-RANGE-FUNC",
            RejectKind::Close => "HIR-UNSUPPORTED-CLOSE",
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
