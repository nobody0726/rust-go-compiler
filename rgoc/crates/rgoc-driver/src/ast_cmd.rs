//! `ast` 子命令 —— 对单个文件跑 parser，输出中立 dump（`T-AP19`）。
//!
//! # 与 oracle 的对齐口径
//!
//! 输出 = 头两行 + [`rgoc_ast::dump`] 的正文。正文**不允许**命令层再渲染一遍
//! （R30 的口径：「可 diff 的格式只能有一处实现」），diff 的另一半是
//! `tools/ast-oracle` 的 `dump.go`（T-AP17/T-AP18 已冻结）。
//!
//! | 行 | oracle | 本命令 | 说明 |
//! |---|---|---|---|
//! | `# golden: <path>` | 传入路径**原样** | 同左 | 两边约定传同一个路径字符串 |
//! | `# source: …` | `oracle` | **`rgoc`** | 来源必须**如实**标注（§5.2 的纪律） |
//!
//! # 退出码语义
//!
//! - 解析**零错误** ⇒ `Ok` ⇒ 0；
//! - **有诊断**（含 `file == None`）⇒ `Err`，错误文本 = 排序后的诊断（代号 +
//!   位置 + 消息）⇒ `main_entry` 统一 stderr + 2。与 oracle「dump 照出 + exit 1」
//!   的形状刻意不同：oracle 是开发对照工具（要看半棵树），本命令是**门禁入口**
//!   —— 静默出半棵树会被误读成「文件没问题」。T-AP18 的对撞测试直接调
//!   `parse_file` + `dump`，不经过这里，不受此契约影响。
//! - io 失败（不存在 / 不可读）⇒ `Err` 上浮 ⇒ 同上。

use std::fmt::Write as _;
use std::path::Path;

use rgoc_ast::dump;
use rgoc_lex::Diag;
use rgoc_parse::parse_file;

/// 跑 `ast`：读文件 → `parse_file` → `dump`。返回值即要打印的内容。
pub fn run_ast(file: &Path) -> Result<String, String> {
    let src = std::fs::read(file).map_err(|e| format!("读 {} 失败：{e}", file.display()))?;

    // 路径回显用**传入的原样字符串**（`# golden:` 行的契约），文件名给 parser
    // 也用同一个 —— 诊断里的短名与 golden 行能对上。
    let name = file.to_string_lossy();
    let res = parse_file(&src, &name);

    let diags = res.diags.into_sorted();
    if res.file.is_none() || !diags.is_empty() {
        return Err(render_diags(&name, &diags, res.first.as_ref(), res.errors));
    }

    let f = res.file.expect("零诊断 ⇒ file 必为 Some（上面已挡 None）");
    let mut out = String::new();
    let _ = writeln!(out, "# golden: {name}");
    let _ = writeln!(out, "# source: rgoc");
    out.push_str(&dump(&f));
    Ok(out)
}

/// 把诊断渲染成错误文本。`into_sorted` 排序在前，这里按序输出。
///
/// 位置格式 = `C1` §2.2 的 `{file}:{line}:{col}`（`FileTable::format_pos` 的
/// 同形手拼）：`parse_file` 只 intern 一个文件（传入的 `name`），直接用入参
/// 回显与表内名字一致；`pos.line`/`pos.col` 就是该格式的两根（绝对坐标）。
fn render_diags(name: &str, diags: &[Diag], first: Option<&Diag>, errors: usize) -> String {
    let mut s = format!("解析失败（{errors} 个错误）：\n");
    for d in diags {
        let _ = writeln!(
            s,
            "{}:{}:{}: {} [{}]",
            name,
            d.pos.line,
            d.pos.col,
            d.message,
            d.code.as_str()
        );
    }
    // first 与 diags 首条是同一条（F19）；独立兜底只为防御性（不应触达）。
    if diags.is_empty()
        && let Some(d) = first
    {
        let _ = writeln!(
            s,
            "{}:{}:{}: {} [{}]",
            name,
            d.pos.line,
            d.pos.col,
            d.message,
            d.code.as_str()
        );
    }
    s
}
