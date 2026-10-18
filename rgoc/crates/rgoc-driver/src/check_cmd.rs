//! `check` 子命令 —— 静态检查面（M4）。
//!
//! # 这一层做什么
//!
//! 只做**编排**（与 `types_cmd`/`lex_cmd` 同一条纪律）：
//!
//! ```text
//! 探文件可读性  →  rgoc_load::load_with_imports  →  rgoc_check::check_package
//!                →  ├─ 诊断（stderr + 退出码）
//!                   └─ dump（stdout，--diag-dump；C8 类别渲染自 Task 20 起）
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
//! # 骨架期（Task 2）的两处临时形态
//!
//! - `check_package` 恒报 `CHK-UNIMPLEMENTED` ⇒ 本命令现在**稳定失败**（1）；
//! - `--diag-dump` 的正文是**临时形态**（C1 的 `位置: 消息 [代号]` 行 + 两行
//!   `#` 头）；Task 20 起换成 C8 的 `<行号>:<列>\t<类别>`（类别投影在
//!   `rgoc_check::diag` 落地后）。
//!
//! `--test-hooks` 已可解析、Task 20 起注入（`F13` 的装置级开关）。

use std::fmt::Write as _;

use rgoc_lex::Diag;
use rgoc_load::{Package, load_with_imports};
use rgoc_types::TypeTable;

use crate::cli::CheckCommand;

/// `check` 的运行结果。三条通道**不合并**（与 `types` 同纪律：合并过一次就
/// 再也分不开「dump 里混进诊断」的问题）。
#[derive(Debug)]
pub struct CheckOutcome {
    /// stdout（`--diag-dump` 时是 dump；否则是摘要）
    pub stdout: String,
    /// stderr（渲染后的诊断，逐行 `<file>:<line>:<col>: <msg> [<CODE>]`）
    pub stderr: String,
    /// 诊断条数（>0 ⇒ 退出码 1）
    pub errors: usize,
}

/// 跑 `check`：装载 + 检查。`Err` ⇒ 退出码 2（参数/io/内部错）。
pub fn run_check(cmd: &CheckCommand) -> Result<CheckOutcome, String> {
    // ① 可读性先探：`rgoc_load` **不读文件**（官方 `go/types` 也不读 —— 调用方读），
    //    静默跳过是最坏的形态，所以这里直接报错（与 `types_cmd` 同一条）。
    for f in &cmd.files {
        std::fs::metadata(f).map_err(|e| format!("读 {} 失败：{e}", f.display()))?;
    }

    // ② 装载（`check` 暂不收 `-I`：首轮对撞逐文件单跑，import 面按需再扩）。
    let mut tt = TypeTable::new();
    let (mut pkg, mut bag) = load_with_imports(&mut tt, &cmd.files, &[], cmd.lang.as_deref());

    // ③ 检查（Task 2 占位：恒报 `CHK-UNIMPLEMENTED`；Task 20 起真身）。
    //    anchor 保证诊断有可指位置：优先第一个**被接受**的文件；装载把入口也
    //    丢掉（极端）时，把第一个入参名字补进装载器的文件表 —— 诊断位置不许无主。
    let first = pkg.files.first().copied();
    let anchor = match first {
        Some(fid) => fid,
        None => pkg.file_table.intern(&cmd.files[0].display().to_string()),
    };
    match rgoc_check::check_package(&pkg, anchor) {
        Ok(_checked) => {}
        Err(check_bag) => {
            for d in check_bag.into_sorted() {
                bag.push(d);
            }
        }
    }
    let diags = bag.into_sorted();

    // ④ 两条通道各自成形
    let mut stdout = String::new();
    if cmd.diag_dump {
        // 注释行（C8 §2 规则 3：`#` 行不进对撞）—— 只为人读。
        let _ = writeln!(stdout, "# source: rgoc");
        let _ = writeln!(
            stdout,
            "# files: {}",
            cmd.files
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(" ")
        );
        stdout.push_str(&render_diags(&pkg, &diags));
    } else {
        // 摘要：人读用（数字摆出来是为了让「装载悄悄少收了文件」可见）。
        let _ = writeln!(stdout, "package {}", pkg.name);
        let _ = writeln!(stdout, "files {}", pkg.files.len());
        let _ = writeln!(stdout, "diagnostics {}", diags.len());
    }

    let errors = diags.len();
    Ok(CheckOutcome {
        stdout,
        stderr: render_diags(&pkg, &diags),
        errors,
    })
}

/// 渲染诊断。位置用**装载器自己的** `FileTable`（`C1` §2.2 的 `{file}:{line}:{col}`）
/// —— 不拿命令行入参回显（与 `types_cmd` 同一条）。
fn render_diags(pkg: &Package, diags: &[Diag]) -> String {
    let mut s = String::new();
    for d in diags {
        let _ = writeln!(
            s,
            "{}: {} [{}]",
            pkg.file_table.format_pos(d.pos),
            d.message,
            d.code.as_str()
        );
    }
    s
}
