//! `build` 子命令（M7）：SSA → `.s` → `clang` → ELF。
//!
//! # 管线与 `run` 完全共用前半段
//!
//! 装载 → 检查 → lowering → SSA → opt。差别只在最后一步：`run` 进解释器，
//! `build` 出机器码。前半段抄一遍就等于有两个「什么算合法程序」的答案。
//!
//! # 三条通道（`C9` §5 的分界）
//!
//! | 情形 | 码 | 谁写 |
//! |---|---|---|
//! | 静态诊断 / HIR 拒绝 / SSA 拒绝 / codegen 拒绝 | **1** | stderr（**没跑**） |
//! | 工具链失败（`clang` 起不来或报错） | **2** | stderr（**跑了但没成**） |
//! | 成功 | **0** | 静默 |
//!
//! # `clang` 的 stderr **原样带出**
//!
//! M0 的教训（`C5` §2.5）：`-nostartfiles` 缺失时的报错是
//! `multiple definition of '_start'` —— 如果被改写成「内部错误」，就再也定位不到。

use std::path::{Path, PathBuf};
use std::process::Command;

use rgoc_check::check::{CheckOptions, check_package_with, version_source};
use rgoc_load::load_with_imports;
use rgoc_types::TypeTable;

use crate::cli::{BuildCommand, Emit};

/// `build` 的三通道结果（与 `ssa_cmd::SsaOutcome` 同构）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildOutcome {
    /// 只放**给对撞看**的东西（`--emit=asm` 时是汇编文本）。
    pub stdout: String,
    /// 诊断与工具链错误。
    pub stderr: String,
    /// 退出码：`0` 成功 / `1` 静态拒绝（不支持面、检查诊断）/ `2` 内部或工具链失败。
    pub exit: u8,
    /// 产物路径（成功时才有）。
    ///
    /// **为什么要返回它**：`--emit=asm` 的判据是「两次逐字节相同」，而比对两份文件
    /// 之前得先知道比哪两份。让测试自己按「`<stem>`」的规则去猜，等于把**命名规则**
    /// 复制到每个测试里 —— 规则一改，测试就会去比一对不存在的文件（然后「通过」）。
    pub artifact: Option<PathBuf>,
}

/// runtime 源码（相对本 crate 的 `CARGO_MANIFEST_DIR`）。
fn rt_source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../runtime/native/aarch64-unknown-linux-gnu/rt.c")
}

/// 跑一次 `build`。
///
/// # Errors
///
/// 返回 `Err` 表示「连跑都没跑起来」（文件读不了、写盘失败、`opt` 后图非法）。
/// **静态拒绝**走 `Ok(exit: 1)`，「跑了但没成」走 `Ok(exit: 2)` —— 三者必须能区分。
pub fn run_build(cmd: &BuildCommand) -> Result<BuildOutcome, String> {
    for f in &cmd.files {
        std::fs::metadata(f).map_err(|e| format!("读 {} 失败：{e}", f.display()))?;
    }

    let lang = match cmd.files.first() {
        Some(f) => {
            let src = std::fs::read(f).map_err(|e| format!("读 {} 失败：{e}", f.display()))?;
            version_source(cmd.lang.as_deref(), &src)
                .map_err(|e| format!("{}: {e}", f.display()))?
        }
        None => cmd.lang.clone(),
    };

    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, &cmd.files, &[], lang.as_deref());

    let opts = CheckOptions { test_hooks: false };
    let (checked, lowered) =
        check_package_with(&pkg, &mut tt, bag, &opts, |s| rgoc_hir::lower(&pkg, s));

    let mut stderr = String::new();
    let errors = checked.rendered_count();
    if errors > 0 {
        use std::fmt::Write as _;
        let mut seen = std::collections::BTreeSet::new();
        for d in checked
            .diags
            .iter()
            .filter(|d| rgoc_check::render::is_rendered(d))
        {
            let pos = pkg.file_table.format_pos(d.diag.pos);
            let key = (pos.clone(), d.diag.code.as_str(), d.diag.message.clone());
            if seen.insert(key) {
                let _ = writeln!(
                    stderr,
                    "{pos}: {} [{}]",
                    d.diag.message,
                    d.diag.code.as_str()
                );
            }
        }
    }

    let program = match lowered {
        Ok(p) => Some(p),
        Err(rejects) => {
            if errors == 0 {
                use std::fmt::Write as _;
                for r in &rejects {
                    let _ = writeln!(stderr, "{}", r.render(&pkg.file_table));
                }
            }
            None
        }
    };
    let Some(program) = program else {
        return Ok(BuildOutcome {
            stderr,
            exit: 1,
            ..BuildOutcome::default()
        });
    };
    if errors > 0 {
        return Ok(BuildOutcome {
            stderr,
            exit: 1,
            ..BuildOutcome::default()
        });
    }

    // ④ SSA
    let built = rgoc_ssa::build::build(&program, &mut tt);
    if !built.rejected.is_empty() {
        use std::fmt::Write as _;
        for r in &built.rejected {
            let _ = writeln!(stderr, "{}", r.render());
        }
        return Ok(BuildOutcome {
            stderr,
            exit: 1,
            ..BuildOutcome::default()
        });
    }
    let mut module = built.module;
    if cmd.opt.any() {
        rgoc_ssa::opt::run(&mut module, &tt, cmd.opt)
            .map_err(|e| format!("pass 之后 verifier 失败：{e:?}"))?;
    }

    // ⑤ 汇编
    let asm = rgoc_arm64::module::compile(&module, &tt).map_err(|e| e.to_string())?;

    // ⑥ 输出路径
    let stem = cmd
        .files
        .first()
        .map(|f| f.with_extension(""))
        .unwrap_or_else(|| PathBuf::from("a.out"));
    let out = cmd.output.clone().unwrap_or(stem);
    let asm_path = out.with_extension("s");

    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("建 {} 失败：{e}", parent.display()))?;
    }
    std::fs::write(&asm_path, &asm).map_err(|e| format!("写 {} 失败：{e}", asm_path.display()))?;

    if cmd.emit == Emit::Asm {
        return Ok(BuildOutcome {
            stdout: asm,
            stderr,
            exit: 0,
            artifact: Some(asm_path),
        });
    }

    // ⑦ rt.o（与产物同目录：不做跨次缓存，避免「改了 rt.c 但用的是旧 .o」）
    let rt_obj = out.with_extension("rt.o");
    let cc = Command::new("clang")
        .args(["-ffreestanding", "-fno-builtin", "-nostdlib", "-O2", "-c"])
        .arg(rt_source())
        .arg("-o")
        .arg(&rt_obj)
        .output()
        .map_err(|e| format!("应能启动 clang：{e}"))?;
    if !cc.status.success() {
        return Ok(BuildOutcome {
            stderr: format!(
                "{}clang 编译 rt.c 失败：\n{}",
                stderr,
                String::from_utf8_lossy(&cc.stderr)
            ),
            exit: 2,
            ..BuildOutcome::default()
        });
    }

    // ⑧ 链接（`-nostartfiles` 与 `-Wl,-s` 都是**必需**，`C5` §2.5 / §3）
    let link = Command::new("clang")
        .arg("-nostartfiles")
        .arg("-Wl,-s")
        .arg("-o")
        .arg(&out)
        .arg(&asm_path)
        .arg(&rt_obj)
        .output()
        .map_err(|e| format!("应能启动 clang：{e}"))?;
    if !link.status.success() {
        return Ok(BuildOutcome {
            stderr: format!(
                "{}clang 链接失败：\n{}{}",
                stderr,
                String::from_utf8_lossy(&link.stderr),
                String::from_utf8_lossy(&link.stdout)
            ),
            exit: 2,
            ..BuildOutcome::default()
        });
    }

    if !cmd.keep_asm {
        let _ = std::fs::remove_file(&asm_path);
        let _ = std::fs::remove_file(&rt_obj);
    }

    Ok(BuildOutcome {
        stdout: String::new(),
        stderr,
        exit: 0,
        artifact: Some(out),
    })
}
