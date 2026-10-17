//! `types` 子命令 —— 对一组同包 `.go` 文件跑**类型检查入口**（M3）。
//!
//! # 这一层做什么
//!
//! 只做**编排**（与 `lex_cmd`/`ast_cmd` 同一条纪律）：
//!
//! ```text
//! 探文件可读性  →  rgoc_load::load_with_imports  →  ├─ 诊断（stderr + 退出码）
//!                                                   └─ dump_types（stdout，--dump）
//! ```
//!
//! 判定逻辑一条都不在这里：装载、作用域、常量求值、类型同一性全在
//! `rgoc-load` / `rgoc-types` / `rgoc-const` 里。本命令**不解释**诊断、
//! 不过滤诊断、不改写诊断 —— 它只排序、渲染、翻译成退出码。
//!
//! # 退出码（`T-TY-81..85` 的判据之一）
//!
//! | 情形 | 码 | 谁判 |
//! |---|---|---|
//! | 零诊断 | 0 | 本命令 |
//! | 有诊断 | 1 | 本命令（**没有警告级**：M3 的诊断都是 error） |
//! | 参数错 / io 错 / 内部错 | 2 | `main_entry` |
//!
//! ⚠️ 「文件读不到」走 **2** 而不是 1：它不是语言层面的错，官方 `go/types`
//! 也没有对应代号（`Package::skipped` 的注释），所以**不**伪装成诊断。
//!
//! # `--dump` 的来源标注（`M3-tests.md` §5.3）
//!
//! stdout 前几行是注释（`# source: rgoc` / `# files: …`）：C7 §2 规则 1 说
//! 两侧都忽略 `#` 行，所以它们**不进对撞**，但失败时能一眼看出是哪个文件、
//! 哪一侧出的 dump。dump 正文**只允许有一处实现**（`rgoc_types::dump_types`），
//! 本命令不二次渲染（与 `ast_cmd` 的 R30 同口径）。

use std::fmt::Write as _;

use rgoc_lex::Diag;
use rgoc_load::{Package, load_with_imports};
use rgoc_types::{TypeTable, try_dump_types};

use crate::cli::TypesCommand;

/// `types` 的运行结果。三个字段对应三条通道，**不合并**：
/// 合并过一次，就会出现「dump 里混进诊断」这种再也分不开的输出。
#[derive(Debug)]
pub struct TypesOutcome {
    /// stdout（`--dump` 时是 C7 正文；否则是摘要）
    pub stdout: String,
    /// stderr（渲染后的诊断，逐行 `<file>:<line>:<col>: <msg> [<CODE>]`）
    pub stderr: String,
    /// 诊断条数（>0 ⇒ 退出码 1）
    pub errors: usize,
}

/// 跑 `types`：装载 + （可选）dump。`Err` ⇒ 退出码 2（参数/io/内部错）。
pub fn run_types(cmd: &TypesCommand) -> Result<TypesOutcome, String> {
    // ① 可读性先探：`rgoc_load` **不读文件**（官方 `go/types` 也不读 —— 调用方读），
    //    所以「读不到」是**调用方**的责任（`Package::skipped` 的注释就是这么说的）。
    //    静默跳过是最坏的形态（本仓反复记的「静默失败」），所以这里直接报错。
    for f in &cmd.files {
        std::fs::metadata(f).map_err(|e| format!("读 {} 失败：{e}", f.display()))?;
    }

    // ② 装载（含 import 递归；`-I` 给根，没有根就一切 import 报「不支持的 import 形式」）
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, &cmd.files, &cmd.roots, cmd.lang.as_deref());
    let diags = bag.into_sorted();

    // ③ 三条通道各自成形
    let mut stdout = String::new();
    if cmd.dump {
        // 注释行（C7 §2 规则 1：两侧忽略 `#`）—— 只为人读，不进对撞
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
        let _ = writeln!(stdout, "# package: {}", pkg.name);
        // ⚠️ 环（`C7` §5.1）⇒ **可判定的错误**（退出码 2）：`types/testdata` 里有
        //    三例（`issue43056/48695/50426`）会让后序走访不终止；从前是
        //    `fatal runtime error: stack overflow`（SIGABRT），现在是响亮的一句。
        let body = try_dump_types(&tt, &pkg.type_roots).map_err(|e| e.to_string())?;
        stdout.push_str(&body);
    } else {
        // 摘要：**人读**用（门禁读的是退出码，对撞读的是 `--dump`）。
        // 数字都摆出来是为了让「装载悄悄少收了文件」可见（静默失败的老形状）。
        let _ = writeln!(stdout, "package {}", pkg.name);
        let _ = writeln!(stdout, "files {}", pkg.files.len());
        let _ = writeln!(stdout, "roots {}", pkg.type_roots.len());
        let _ = writeln!(stdout, "diagnostics {}", diags.len());
    }

    let errors = diags.len();
    Ok(TypesOutcome {
        stdout,
        stderr: render_diags(&pkg, &diags),
        errors,
    })
}

/// 渲染诊断。位置用**装载器自己的** `FileTable`（`C1` §2.2 的 `{file}:{line}:{col}`）——
/// 不拿命令行入参回显：多文件时「被接受的文件」可能少于入参（包名不一致的会被丢掉），
/// 而诊断的位置是装载器给的。
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
