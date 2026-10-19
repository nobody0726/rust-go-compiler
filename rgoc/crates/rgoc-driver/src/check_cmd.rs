//! `check` 子命令 —— 静态检查面（M4）。
//!
//! # 这一层做什么
//!
//! 只做**编排**（与 `types_cmd`/`lex_cmd` 同一条纪律）：
//!
//! ```text
//! 探文件可读性  →  rgoc_load::load_with_imports  →  rgoc_check::check_package
//!                →  ├─ 诊断（stderr + 退出码）            ← 非 dump 形态
//!                   └─ dump（stdout，--diag-dump；C8 渲染） ← 对撞入口
//! ```
//!
//! 判定逻辑一条都不在这里：检查在 `rgoc-check` 里。本命令**不解释**诊断、
//! 不过滤诊断、不改写诊断 —— 它只排序、渲染、翻译成退出码。
//!
//! # 退出码（与 `types` 同表；`T-CK-E1`）
//!
//! | 情形 | 码 | 谁判 |
//! |---|---|---|
//! | 零诊断 | 0 | 本命令 |
//! | 有诊断 | 1 | 本命令 |
//! | 参数错 / io 错 / 内部错 | 2 | `main_entry` |
//!
//! ⚠️ 「文件读不到」走 **2** 而不是 1：它不是语言层面的错（与 `types_cmd`
//! 同一条 Ruling），**不**伪装成诊断。
//!
//! # 两条通道的形态（`C8` §2 / §6）
//!
//! - `--diag-dump`：stdout = **C8 渲染**（`# source: rgoc` / `# file:` +
//!   `<行号>:<列>\t<类别>`），**stderr 必须空**（§6 规则 1：对撞只读 stdout，
//!   stderr 里有任何字节都说明通道混了）。
//! - 不给时：stdout = 摘要（人读；数字摆出来是为了让「装载悄悄少收了文件」
//!   可见），stderr = `{file}:{line}:{col}: <msg> [<CODE>]` 逐行诊断。
//!
//! # 「有诊断」指什么
//!
//! = **渲染得出来的行数 > 0**（`CheckedPackage::rendered_count`）—— 与
//! oracle 的「过滤后 `len(diags) > 0`」同语义：只含次级消息（`\t` 开头，
//! `C8` §2 规则 5）的文件两侧都算「无诊断」。
//!
//! `--test-hooks` 已可解析、随 `CheckOptions` 进会话（注入在 Task 29 生效；
//! `F13` 的装置级开关）。

use std::fmt::Write as _;

use rgoc_load::{Package, load_with_imports};
use rgoc_types::TypeTable;

use crate::cli::CheckCommand;

/// `check` 的运行结果。三条通道**不合并**（与 `types` 同纪律：合并过一次就
/// 再也分不开「dump 里混进诊断」的问题）。
#[derive(Debug)]
pub struct CheckOutcome {
    /// stdout（`--diag-dump` 时是 C8 渲染；否则是摘要）
    pub stdout: String,
    /// stderr（非 dump 形态渲染后的诊断；`--diag-dump` 下恒空）
    pub stderr: String,
    /// 渲染得出来的诊断条数（>0 ⇒ 退出码 1）
    pub errors: usize,
}

/// 跑 `check`：装载 + 检查。`Err` ⇒ 退出码 2（参数/io/内部错）。
pub fn run_check(cmd: &CheckCommand) -> Result<CheckOutcome, String> {
    // ① 可读性先探：`rgoc_load` **不读文件**（官方 `go/types` 也不读 —— 调用方读），
    //    静默跳过是最坏的形态，所以这里直接报错（与 `types_cmd` 同一条）。
    for f in &cmd.files {
        std::fs::metadata(f).map_err(|e| format!("读 {} 失败：{e}", f.display()))?;
    }

    // ①.5 首行 harness 旗标（`F2`①）：`// -lang=go1.N` 与 `//go:build`（装载器读）
    //      **同源进入**；`-goexperiment` / `-fakeImportC` 在装置层响亮失败。
    //      显式 `--lang` 覆盖注释（`Ruling 18-1`，`D-M3-15` 的覆盖路径）。
    let lang = match cmd.files.first() {
        Some(f) => {
            let src = std::fs::read(f).map_err(|e| format!("读 {} 失败：{e}", f.display()))?;
            rgoc_check::check::version_source(cmd.lang.as_deref(), &src)
                .map_err(|e| format!("{}: {e}", f.display()))?
        }
        None => cmd.lang.clone(),
    };

    // ② 装载（`check` 暂不收 `-I`：首轮对撞逐文件单跑，import 面按需再扩）。
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, &cmd.files, &[], lang.as_deref());

    // ③ 检查（Task 20 起真身：装载诊断入流 + 会话骨架；Task 21 起含表达式
    //    运算符面；语句/内建/门控点由后续任务逐个挂上）。
    let opts = rgoc_check::CheckOptions {
        test_hooks: cmd.test_hooks,
    };
    let checked = rgoc_check::check_package(&pkg, &mut tt, bag, &opts);

    // ④ 两条通道各自成形
    let display = cmd
        .files
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let mut stdout = String::new();
    let mut stderr = String::new();
    if cmd.diag_dump {
        // C8 渲染（含两行 `#` 头；`#` 行不进对撞，只为人读）。
        stdout = checked.render(&display);
    } else {
        // 摘要：人读用（数字摆出来是为了让「装载悄悄少收了文件」可见）。
        let _ = writeln!(stdout, "package {}", pkg.name);
        let _ = writeln!(stdout, "files {}", pkg.files.len());
        let _ = writeln!(stdout, "diagnostics {}", checked.rendered_count());
        stderr = render_diags(&pkg, &checked.diags);
    }

    Ok(CheckOutcome {
        stdout,
        stderr,
        errors: checked.rendered_count(),
    })
}

/// 渲染诊断（非 dump 形态；跳过渲染层过滤掉的次级消息 —— 与退出码同语义）。
///
/// 位置用**装载器自己的** `FileTable`（`C1` §2.2 的 `{file}:{line}:{col}`）
/// —— 不拿命令行入参回显（与 `types_cmd` 同一条）。
fn render_diags(pkg: &Package, diags: &[rgoc_check::diag::RawDiag]) -> String {
    let mut s = String::new();
    for d in diags.iter().filter(|d| rgoc_check::render::is_rendered(d)) {
        let _ = writeln!(
            s,
            "{}: {} [{}]",
            pkg.file_table.format_pos(d.diag.pos),
            d.diag.message,
            d.diag.code.as_str()
        );
    }
    s
}
