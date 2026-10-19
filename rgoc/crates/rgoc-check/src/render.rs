//! `--diag-dump` 的渲染层（`C8` §2 的字节格式；M4/Task 20）。
//!
//! # 位置与分工
//!
//! ```text
//! 会话（check.rs）──诊断流──► render.rs ──String──► CLI 的 stdout
//! ```
//!
//! 这里是 `C8` 对撞面的**唯一出口**：头两行 + 排序后的数据行。过滤发生在
//! **这里**而不是上报层 —— 诊断流内部保留原始次级链（人读；`M4-plan.md`
//! Review Focus #3），而对撞与退出码只认渲染得出来的行。official
//! `check_test.go:169` 的「回调剔除」与本层「渲染剔除」在**行集合**上等价。
//!
//! # 次级消息判据为什么长这样
//!
//! 官方过滤的是**格式化字符串**（`<pos>: <msg>`）含 `": \t"` 的消息。go/types
//! 把带合法位置的子错误**单发**成 `Msg = "\t"+msg`（`errors.go:186-188`），
//! 格式化后 = `"<pos>: \t<msg>"` —— 命中 `": \t"`。我方消息不带位置前缀，
//! 等价判据 = 「以 `\t` 开头」**或**「自身含 `": \t"`」：前缀与消息头能拼出
//! `": \t"` 的**唯一**方式是消息头是 `\t`；消息体自含的照样命中。两条都挡，
//! 少滤 ⇒ 多行、误滤 ⇒ 丢行（本仓两类失效都会让对撞假红/假绿）。
//!
//! # 排序与确定性
//!
//! 按（行号, 列, 类别 token）**稳定排序**（`C8` §2 规则 5；oracle 侧同款：
//! `tools/types-oracle/check.go` 的 `renderCheckDiags`）。排序键覆盖数据行的
//! 全部字段 ⇒ 任意输入次序产出同一串字节（`T-CK-92` 的确定性由此保证；
//! `sort_by_key` 是稳定排序）。**重复行保留**（多重集语义，规则 6）：不去重。
//!
//! 单文件前提（`C8` §2 装置前提，`U-M4-11`）：跨文件不归并；`file` 参数
//! 原样进 `# file:` 行（oracle 侧同款 —— 调用方给什么写什么）。

use crate::diag::{RawDiag, category_of};

/// 渲染 `C8` 的字节面：`# source: rgoc` / `# file: <file>` 两行头 + 数据行
/// `<行号>:<列>\t<类别>`。零诊断时只输出头行（规则 8 —— 头行是「跑过了」的
/// 证据，缺了它「两侧都空」看不出是渲染还是没跑）。
#[must_use]
pub fn render(diags: &[RawDiag], file: &str) -> String {
    let mut rows: Vec<&RawDiag> = diags.iter().filter(|d| is_rendered(d)).collect();
    rows.sort_by_key(|d| {
        (
            d.diag.pos.line,
            d.diag.pos.col,
            category_of(d.diag.code).as_str(),
        )
    });
    let mut out = format!("# source: rgoc\n# file: {file}\n");
    for d in rows {
        out.push_str(&d.render_row());
        out.push('\n');
    }
    out
}

/// 次级消息判据（见模块头）：官方 `": \t"` 过滤的 rgoc 侧等价物。
#[must_use]
pub fn is_secondary(msg: &str) -> bool {
    msg.starts_with('\t') || msg.contains(": \t")
}

/// 这条诊断会产出渲染数据行吗。
///
/// 退出码判据也用它 —— 与 oracle 的「过滤后 `len(diags) > 0`」同语义
/// （`check.go` 的 `runCheckMode`）。
#[must_use]
pub fn is_rendered(raw: &RawDiag) -> bool {
    !is_secondary(&raw.diag.message)
}
