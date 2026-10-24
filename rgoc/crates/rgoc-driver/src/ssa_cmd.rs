//! `ssa` 子命令 —— 编排层（M6/Task 2 起；Task 31 接上拒绝面与 dump/verify）。
//!
//! # 这一层做什么
//!
//! 与 `run_cmd` / `check_cmd` 同一条纪律：**只编排**。
//!
//! ```text
//! 探文件可读性 → load_with_imports → check_package_with（会话交给 lowering）
//!              → rgoc_hir::lower → rgoc_ssa::build →（dump / verify）
//! ```
//!
//! 判定逻辑一条都不在这里：`SSA-*` 拒绝由 `rgoc-ssa` 判，dump 由 `dump` 排版，
//! 不变式由 `verify` 判。本命令只把三条通道翻译成字节与退出码。
//!
//! # 三条通道（`T-SSA-38`）
//!
//! | 通道 | 内容 | 何时 |
//! |---|---|---|
//! | stdout | `C10` 中立 dump | 给了 `--dump` 且没有拒绝 |
//! | stderr | 静态诊断（`C8` 行） | 检查没过（此时**不建图**） |
//! | stderr | `SSA-UNSUPPORTED-*`（一行一条） | 构建期拒绝 |
//! | stderr | verifier 的 `V1..V9` 错误 | `--verify` 且图非法 |
//!
//! **dump 先于 verify**：两者同时给时，dump 照出（它是诊断材料），
//! verify 的错误追加在 stderr —— 顺序固定，免得「先报错就没 dump」。
//!
//! # 退出码
//!
//! | 情形 | 码 |
//! |---|---|
//! | 建图成功（`--verify` 也过） | 0 |
//! | 静态诊断 / HIR 拒绝 / SSA 拒绝 / verify 失败 | 1 |
//! | 参数与 io 错、管线内部错 | 2 |

use std::fmt::Write as _;

use rgoc_check::CheckOptions;
use rgoc_load::load_with_imports;
use rgoc_types::TypeTable;

use crate::cli::SsaCommand;

/// `ssa` 的运行结果（三条通道不合并，与 `run_cmd::RunOutcome` 同形）。
#[derive(Debug)]
pub struct SsaOutcome {
    /// stdout：`C10` 中立 dump（没给 `--dump` 时为空）。
    pub stdout: String,
    /// stderr：诊断 / 拒绝 / verifier 错误。
    pub stderr: String,
    /// 退出码（0 正常 / 1 静态拒绝 / 2 参数与 io 错、内部未完成）。
    pub exit: u8,
}

/// 跑 `ssa`。`Err` ⇒ 退出码 2（参数/io/内部错）。
pub fn run_ssa(cmd: &SsaCommand) -> Result<SsaOutcome, String> {
    // ① 可读性先探：`rgoc_load` 不读文件，静默跳过是最坏的形态。
    for f in &cmd.files {
        std::fs::metadata(f).map_err(|e| format!("读 {} 失败：{e}", f.display()))?;
    }

    // ② 装载
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, &cmd.files, &[], None);

    // ③ 检查 + lowering（同一个会话，`D-M5-2`）
    let opts = CheckOptions { test_hooks: false };
    let (checked, lowered) =
        rgoc_check::check::check_package_with(&pkg, &mut tt, bag, &opts, |s| {
            rgoc_hir::lower(&pkg, s)
        });

    let mut stderr = String::new();

    // ④ 静态诊断优先：连 HIR 都没过就不该谈 SSA（与 `run` 的 `EX-2` 同口径）
    let errors = checked.rendered_count();
    if errors > 0 {
        let mut seen = std::collections::BTreeSet::new();
        for d in checked
            .diags
            .iter()
            .filter(|d| rgoc_check::render::is_rendered(d))
        {
            let pos = pkg.file_table.format_pos(d.diag.pos);
            let key = (
                pos.clone(),
                d.diag.code.as_str().to_string(),
                d.diag.message.clone(),
            );
            if !seen.insert(key) {
                continue;
            }
            let _ = writeln!(
                stderr,
                "{pos}: {} [{}]",
                d.diag.message,
                d.diag.code.as_str()
            );
        }
    }
    let program = match lowered {
        Ok(p) => Some(p),
        Err(rejects) => {
            if errors == 0 {
                for r in &rejects {
                    let _ = writeln!(stderr, "{}", r.render(&pkg.file_table));
                }
            }
            None
        }
    };
    let Some(program) = program else {
        return Ok(SsaOutcome {
            stdout: String::new(),
            stderr,
            exit: 1,
        });
    };
    if errors > 0 {
        return Ok(SsaOutcome {
            stdout: String::new(),
            stderr,
            exit: 1,
        });
    }

    // ⑤ 建图
    let out = rgoc_ssa::build::build(&program, &mut tt);

    // ⑥ 拒绝面：**有码的拒绝**（`T-SSA-33` 的 S 腿：码 + 退出码 1）
    if !out.rejected.is_empty() {
        for r in &out.rejected {
            let _ = writeln!(stderr, "{}", r.render());
        }
        return Ok(SsaOutcome {
            stdout: String::new(),
            stderr,
            exit: 1,
        });
    }

    // ⑥.5 passes（`--opt=all` 是默认）：**先优化再 dump/verify**，
    // 这样 `--dump` 看到的就是「要拿去执行的那份图」。
    // pass 之后 verifier 不过 ⇒ 那是 pass 的 bug（内部错，退出码 2），
    // 不能报成「你的程序有问题」（1）。
    let mut out = out;
    if cmd.opt.any() {
        // **先验后优化**：pass 假定 `V1..V9` 成立。在坏图上做变换，出来的错会
        // 被报成「pass 的 bug」—— 而真正的错在构建器（`T-SSA-33` 的泛型守卫
        // 就是这么暴露的：图本来就 `V3` 不合法）。
        let mut bad = false;
        for f in &out.module.funcs {
            if let Err(e) = rgoc_ssa::verify::verify(f, &tt) {
                let _ = writeln!(stderr, "{}: {e}", f.name);
                bad = true;
            }
        }
        if bad {
            return Ok(SsaOutcome {
                stdout: String::new(),
                stderr,
                exit: 1,
            });
        }
        if let Err(e) = rgoc_ssa::opt::run(&mut out.module, &tt, cmd.opt) {
            let _ = writeln!(stderr, "SSA 内部错误（pass 之后 verifier 不过）：{e}");
            return Ok(SsaOutcome {
                stdout: String::new(),
                stderr,
                exit: 2,
            });
        }
    }

    // ⑦ dump（stdout，先于 verify）
    let mut stdout = String::new();
    if cmd.dump {
        stdout = rgoc_ssa::dump::dump_module(&out.module, &tt);
    }

    // ⑧ verify（stderr）
    let mut bad = false;
    if cmd.verify {
        for f in &out.module.funcs {
            if let Err(e) = rgoc_ssa::verify::verify(f, &tt) {
                let _ = writeln!(stderr, "{}: {e}", f.name);
                bad = true;
            }
        }
    }

    Ok(SsaOutcome {
        stdout,
        stderr,
        exit: u8::from(bad),
    })
}
