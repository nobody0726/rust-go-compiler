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
pub fn build_frontend(
    files: &[PathBuf],
    roots: &[PathBuf],
    opt: rgoc_ssa::opt::Config,
    lang_opt: Option<&str>,
) -> Result<Frontend, FrontendError> {
    for f in files {
        std::fs::metadata(f)
            .map_err(|e| FrontendError::Internal(format!("读 {} 失败：{e}", f.display())))?;
    }

    let lang = match files.first() {
        Some(f) => {
            let src = std::fs::read(f)
                .map_err(|e| FrontendError::Internal(format!("读 {} 失败：{e}", f.display())))?;
            version_source(lang_opt, &src)
                .map_err(|e| FrontendError::Internal(format!("{}: {e}", f.display())))?
        }
        None => lang_opt.map(str::to_string),
    };

    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, files, roots, lang.as_deref());

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
        return Err(FrontendError::Rejected(stderr));
    };
    if errors > 0 {
        return Err(FrontendError::Rejected(stderr));
    }

    // ④ SSA
    let built = rgoc_ssa::build::build(&program, &mut tt);
    if !built.rejected.is_empty() {
        use std::fmt::Write as _;
        for r in &built.rejected {
            let _ = writeln!(stderr, "{}", r.render());
        }
        return Err(FrontendError::Rejected(stderr));
    }
    let mut module = built.module;
    if opt.any() {
        rgoc_ssa::opt::run(&mut module, &tt, opt)
            .map_err(|e| FrontendError::Internal(format!("pass 之后 verifier 失败：{e:?}")))?;
    }
    Ok(Frontend {
        module,
        types: tt,
        stderr,
    })
}

/// 前半段（装载 → 检查 → lowering → SSA → opt）。
///
/// # 为什么要单独一个函数
///
/// M9 的元数据对撞（`desc_layout`）与栈图（`stackmap`）都要**拿到** `Module`
/// 与 `TypeTable` 才能独立算一遍布局/ptrmask 再与产物比。让它们各自抄一遍
/// 前半段就等于有两个「什么算合法程序」的答案 —— 而那条分叉会在「检查器
/// 放行、抄的那份不放行」时静默地让判据失效。
#[derive(Debug)]
pub struct Frontend {
    /// SSA 图（`opt` 之后）。
    pub module: rgoc_ssa::ir::Module,
    /// 类型表（与 `module` 里的 `TypeId` 同一张）。
    pub types: TypeTable,
    /// 非致命诊断（成功时也可能非空）。
    pub stderr: String,
}

/// 前半段的失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontendError {
    /// 静态拒绝（诊断 / HIR / SSA）：退出码 **1**。
    Rejected(String),
    /// 内部失败（读盘、opt 后图非法）：退出码 **2**。
    Internal(String),
}

pub fn run_build(cmd: &BuildCommand) -> Result<BuildOutcome, String> {
    let Frontend {
        module,
        types: tt,
        mut stderr,
    } = match build_frontend(&cmd.files, &cmd.roots, cmd.opt, cmd.lang.as_deref()) {
        Ok(f) => f,
        Err(FrontendError::Rejected(stderr)) => {
            return Ok(BuildOutcome {
                stderr,
                exit: 1,
                ..BuildOutcome::default()
            });
        }
        Err(FrontendError::Internal(e)) => return Err(e),
    };

    // ⑤ 汇编
    //
    // ⚠️ `CgCode` 是**静态拒绝**（不支持面），不是内部失败 —— 退出码必须是 1。
    // 走 `?` 会变成 `Err(String)` ⇒ 退出码 2，于是「M7 不支持切片」被记成
    // 「编译器自己炸了」，语料分类器（`run-collide --native`）也会把它算成坏。
    let asm = match rgoc_arm64::module::compile(&module, &tt) {
        Ok(a) => a,
        // 静态拒绝：不支持面 ⇒ 退出码 1，码本身是给人看的诊断。
        Err(rgoc_arm64::module::CompileError::Cg(code)) => {
            use std::fmt::Write as _;
            let _ = writeln!(stderr, "{}", code.as_str());
            return Ok(BuildOutcome {
                stderr,
                exit: 1,
                ..BuildOutcome::default()
            });
        }
        // 没有运行期布局的类型走到后端 = 上游漏了收敛，是**内部**失败 ⇒ 2。
        Err(e @ rgoc_arm64::module::CompileError::Frame(..)) => {
            return Err(format!("帧布局失败：{e}"));
        }
    };

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

    // `STACK-MAP v1` 是**交付物**（`M9-design.md` §3.11）：M10 的栈扫描器消费它，
    // 所以无条件落盘，不受 `--keep-asm` 影响。同一份内容也编在 `.rodata` 的
    // `RtFuncInfo` 表里 —— 两侧逐字节对账是 `T-ROOT-01`。
    //
    // ⚠️ 它**不能**塞进 `.s`：行首的 `#` 在 AArch64 的 as 里是立即数前缀。
    {
        let map = rgoc_arm64::module::stack_maps(&module, &tt).map_err(|e| e.to_string())?;
        let map_path = out.with_extension("stackmap");
        std::fs::write(&map_path, map)
            .map_err(|e| format!("写 {} 失败：{e}", map_path.display()))?;
    }

    // `FRAME-MAP v1` 与 `.s` 并列（`C5` §8）：前缀 `#` 在 AArch64 的 as 里是立即数前缀，
    // 塞进 `.s` 就汇编不过。只在 `--keep-asm` 时留 —— 它与 `.s` 同属「中间件」。
    if cmd.keep_asm {
        let map = rgoc_arm64::module::frame_maps(&module, &tt).map_err(|e| e.to_string())?;
        let map_path = out.with_extension("framemap");
        std::fs::write(&map_path, map)
            .map_err(|e| format!("写 {} 失败：{e}", map_path.display()))?;
    }

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
    //
    // ⚠️ **`-no-pie` 也是必需的**（M11/Task 9 加）。clang 在 Linux 上默认
    // 走 PIE，而 PIE 下全局符号不能用 `adrp x9, sym; add x9, x9, :lo12:sym`
    // 直接寻址（必须走 GOT）—— 实测报
    // `relocation R_AARCH64_ADR_PREL_PG_HI21 … can not be used when making
    // a shared object`。`rt.c` 的顶层汇编块（`rt_switch` 自测）要直接引用
    // 全局，所以产物必须是**非 PIE 的静态 ELF**。
    //
    // 这与「freestanding、无 libc、`-nostartfiles`」是同一个方向：产物本来
    // 就没有动态段，PIE 只是链接器的默认值，不是需求。
    let link = Command::new("clang")
        .arg("-nostartfiles")
        .arg("-no-pie")
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
