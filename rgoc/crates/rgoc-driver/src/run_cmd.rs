//! `run` 子命令 —— 执行面（M5/Task 27；`M5-design.md` §4.1）。
//!
//! # 这一层做什么
//!
//! 只做**编排**（与 `check_cmd` 同一条纪律）：
//!
//! ```text
//! 探文件可读性 → load_with_imports → check_package_with（会话交给 lowering）
//!              → rgoc_hir::lower → rgoc_interp::run
//! ```
//!
//! 判定逻辑一条都不在这里：「HIR 拒绝」由 `rgoc-hir` 判，「跑出什么」由
//! `rgoc-interp` 判。本命令只把三条通道翻译成字节与退出码。
//!
//! # 三条通道（`D-M5-6`）
//!
//! | 通道 | 内容 | 何时 |
//! |---|---|---|
//! | stdout | 程序的合并输出（`print` / `println` + 未捕获 panic 的 `panic: …`） | 跑起来了 |
//! | stdout | HIR dump | 给了 `--dump-hir`（此时**不跑**） |
//! | stderr | 静态诊断（`C8` 行）与 `HIR-*` 拒绝 | 诊断/拒绝非空 |
//! | stderr | 资源超限原因 | 撞上限 |
//!
//! # 退出码
//!
//! | 情形 | 码 |
//! |---|---|
//! | 跑完且正常返回 | 0 |
//! | 静态诊断 / HIR 拒绝（**没跑**） | 1 |
//! | 未捕获 panic / 资源超限 / 参数与 io 错 | 2 |
//!
//! 注意 1 与 2 的分界：**静态诊断 ⇒ 不执行**（官方 `go run` 在编译失败时
//! 也不会跑），所以「有诊断」时输出里绝不会有半截程序输出 —— 这条是
//! `EX-2`「诊断优先」的判据。

use std::fmt::Write as _;

use rgoc_check::CheckOptions;
use rgoc_load::load_with_imports;
use rgoc_types::TypeTable;

use crate::cli::{RunCommand, Via};

/// `run` 的运行结果（三条通道不合并）。
#[derive(Debug)]
pub struct RunOutcome {
    /// stdout（程序输出，或 HIR dump）。
    pub stdout: String,
    /// stderr（静态诊断 / HIR 拒绝 / 资源超限原因）。
    pub stderr: String,
    /// 退出码（0 / 1 / 2）。
    pub exit: u8,
}

/// 跑 `run`。`Err` ⇒ 退出码 2（参数/io/内部错）。
pub fn run_run(cmd: &RunCommand) -> Result<RunOutcome, String> {
    // ① 可读性先探：`rgoc_load` 不读文件，静默跳过是最坏的形态。
    for f in &cmd.files {
        std::fs::metadata(f).map_err(|e| format!("读 {} 失败：{e}", f.display()))?;
    }

    // ①.5 首行 `// -lang=` 旗标（与 `check` 同源）。
    let lang = match cmd.files.first() {
        Some(f) => {
            let src = std::fs::read(f).map_err(|e| format!("读 {} 失败：{e}", f.display()))?;
            rgoc_check::check::version_source(cmd.lang.as_deref(), &src)
                .map_err(|e| format!("{}: {e}", f.display()))?
        }
        None => cmd.lang.clone(),
    };

    // ② 装载
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, &cmd.files, &[], lang.as_deref());

    // ③ 检查 + lowering（**同一个会话**：`D-M5-2`）
    let opts = CheckOptions { test_hooks: false };
    let (checked, lowered) =
        rgoc_check::check::check_package_with(&pkg, &mut tt, bag, &opts, |s| {
            rgoc_hir::lower(&pkg, s)
        });

    let mut stderr = String::new();

    // ④ 静态诊断优先：有诊断就**不跑**（`EX-2`）
    let errors = checked.rendered_count();
    if errors > 0 {
        // 去重：`check_package_with` 把会话同时交给 lowering（`D-M5-2`），而 lowering
        // 会反复向同一个会话查询同一个表达式的静态类型 ⇒ 同一条诊断被**重复推入**
        // 多次（实测 `F[int](1)` 推了 8 次）。`check` 子命令看不到这个现象（它不
        // lowering），所以这是 `run` 独有的失真：用户看到 8 行一样的错。
        //
        // 按 (位置, 码, 消息) 去重 —— 这是**同一句话**的三要素，任一不同都必须
        // 各自成行（否则 `i.(T)` 与 `i.(V)` 这种同码不同消息会被吞掉一条）。
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

    // ④′ 静态诊断已经把程序判死了 ⇒ **不打印** HIR 拒绝。
    //
    // 理由：`HIR-*` 拒绝描述的是「这个程序要执行的形态不在 M5 面」。可程序连
    // 静态检查都没过，压根到不了执行面；此时每一条 `HIR-*` 都是**下游噪声**，
    // 而且最刺眼的那条（`HIR-MISSING-TYPE`）的文案是「D-M5-2 失效条件」——
    // 那是**内部不变式报警**的措辞，摆在一个普通用户错误（`undefined()`、
    // 泛型实例化）后面，等于把「你的程序写错了」说成「编译器坏了」。
    //
    // 反过来「check 过了、lower 拒了」必须**照打** —— 那才是 `T-EX-02` 的
    // `HIR-UNSUPPORTED-*` 通道，此时 `errors == 0`，走下面那一支。
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
        return Ok(RunOutcome {
            stdout: String::new(),
            stderr,
            exit: 1,
        });
    };
    if errors > 0 {
        return Ok(RunOutcome {
            stdout: String::new(),
            stderr,
            exit: 1,
        });
    }

    // ⑤ 只 dump 不跑（`--dump-hir`，见 `RunCommand` 的文档）
    if cmd.dump_hir {
        return Ok(RunOutcome {
            stdout: rgoc_hir::dump_program(&program),
            stderr,
            exit: 0,
        });
    }

    // ⑥ 执行
    let interp_opts = rgoc_interp::Options {
        max_steps: cmd
            .max_steps
            .unwrap_or(rgoc_interp::Options::default().max_steps),
        // `--sched=seed:N`（M8/Task 5）：只喂 `select` 的伪随机源，调度顺序不受影响。
        seed: cmd.sched.seed(),
        ..rgoc_interp::Options::default()
    };
    if cmd.via == Via::Ssa {
        return Ok(run_via_ssa(&program, &mut tt, cmd.opt, interp_opts, stderr));
    }
    let out = rgoc_interp::run(&program, &tt, interp_opts);
    if let Some(d) = out.diag {
        let _ = writeln!(stderr, "{d}");
    }
    Ok(RunOutcome {
        stdout: out.output,
        stderr,
        exit: u8::try_from(out.exit).unwrap_or(2),
    })
}

/// `--via ssa`：建图 → verifier → 求值（`M6-plan.md` Task 37）。
///
/// # 为什么中间要跑 verifier
///
/// 求值器假定 IR 满足 `V1..V9`（值必有定义、块参数与边实参等长、同根保序…）。
/// 图不合法时求值器的行为是「未定义」——它可能算错、可能读到 `None` 报内部错，
/// 也可能碰巧对。**跑一遍 verifier 只要几毫秒**，换来的是「要么图合法、要么
/// 响亮报错」这条硬边界。所以这里不是可选的调试开关。
///
/// # 退出码与 `hir` 同口径
///
/// 拒绝（`SSA-UNSUPPORTED-*`）= 1（静态面），verifier 失败 = 2（内部错），
/// 程序自己的 panic / 预算耗尽 = 2（`run_ssa` 给）。
fn run_via_ssa(
    program: &rgoc_hir::Program,
    tt: &mut TypeTable,
    opt: rgoc_ssa::opt::Config,
    opts: rgoc_interp::Options,
    mut stderr: String,
) -> RunOutcome {
    let mut built = rgoc_ssa::build::build(program, tt);
    if !built.rejected.is_empty() {
        for r in &built.rejected {
            let _ = writeln!(stderr, "{}", r.render());
        }
        return RunOutcome {
            stdout: String::new(),
            stderr,
            exit: 1,
        };
    }
    // 先验后优化：pass 假定 `V1..V9` 成立，坏图上做变换只会把错报错地方。
    for f in &built.module.funcs {
        if let Err(e) = rgoc_ssa::verify::verify(f, tt) {
            let _ = writeln!(stderr, "SSA 内部错误（构建出的图不合法）：{e}");
            return RunOutcome {
                stdout: String::new(),
                stderr,
                exit: 2,
            };
        }
    }
    // passes：每个 pass 之后 `opt::run` 自己 verify（`SSA-5`）；这里只负责
    // 把「pass 破坏了图」翻译成内部错（退出码 2），不报成用户程序的错。
    if let Err(e) = rgoc_ssa::opt::run(&mut built.module, tt, opt) {
        let _ = writeln!(stderr, "SSA 内部错误（pass 之后 verifier 不过）：{e}");
        return RunOutcome {
            stdout: String::new(),
            stderr,
            exit: 2,
        };
    }
    for f in &built.module.funcs {
        if let Err(e) = rgoc_ssa::verify::verify(f, tt) {
            let _ = writeln!(stderr, "SSA 内部错误（verifier）：{e}");
            return RunOutcome {
                stdout: String::new(),
                stderr,
                exit: 2,
            };
        }
    }
    let out = rgoc_interp::run_ssa(&built.module, tt, opts);
    if let Some(d) = out.diag {
        let _ = writeln!(stderr, "{d}");
    }
    RunOutcome {
        stdout: out.output,
        stderr,
        exit: u8::try_from(out.exit).unwrap_or(2),
    }
}
