//! native 后端在**入选语料**上的闭环（`T-NAT-42/43`）。
//!
//! # 与 `native_smoke.rs` / `native_float.rs` 的分工
//!
//! 那两个是**手写 fixture**：每条只钉一个语义点，失败时能立刻指到哪一处。
//! 这一条是**语料**：`N` 侧的每个真实 Go 文件都 编译 → 链接 → 执行，
//! 与**解释器**逐字节对撞。它回答的是另一个问题：
//! 「这些语义点组合起来，native 与解释器还一致吗」。
//!
//! # 判据必须分**两栏**
//!
//! `N` 侧 46 个文件里绝大多数**输出为空**（官方 `.out` 是 0 字节）。在空输出上
//! 「两边都是空」是**没有信息量**的通过 —— 它只证明「没崩」，不证明「算对了」。
//! 所以这条测试：
//!
//! | 断言 | 为什么 |
//! |---|---|
//! | 每个 `N` 侧文件都能构建 + 退出码 0 | 非 0 说明 `panic`/段错误 |
//! | native 的 stdout == 解释器的 stdout | 两条独立通路的强判据 |
//! | **有输出的文件数 ≥ 3** | 分母写出来：全是空输出时这条测试退化成空话 |
//!
//! 第三条是这条测试的**牙齿**。没有它，「全绿」可能只是「全空」。
//!
//! # `M` 侧为什么直接跳过
//!
//! 被 `rgoc-arm64` 拒绝的文件（切片面等）没有产物可跑。这里**只数分母**，
//! 不静默当成通过。

use std::path::{Path, PathBuf};

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit, RunCommand, Via};

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达")
}

/// 入选清单的路径（仓库相对路径）。
fn selected() -> Vec<String> {
    let text = std::fs::read_to_string(repo_root().join("tools/run-oracle/run-selected.txt"))
        .expect("读不到 run-selected.txt");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(ToString::to_string)
        .collect()
}

/// 被 M6 拒掉的清单（这些连 SSA 都没有，不在本测试的分母里）。
fn ssa_rejected() -> Vec<String> {
    let path = repo_root().join("tools/run-oracle/ssa-rejected.txt");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split('\t').next())
        .map(ToString::to_string)
        .collect()
}

/// 每次运行都落在一个独立目录里（并行测试不互相踩）。
fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/native-corpus")
        .join(name);
    std::fs::create_dir_all(&d).expect("建 scratch 目录失败");
    d
}

#[test]
fn t_nat_42_43_语料上的_native_与解释器逐字节一致() {
    let rejected = ssa_rejected();
    let n: Vec<String> = selected()
        .into_iter()
        .filter(|p| !rejected.contains(p))
        .collect();
    assert!(!n.is_empty(), "N 侧为空 —— 清单读错了");

    let mut compared = 0usize;
    let mut with_output = 0usize;
    let mut rejected_by_cg: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    for rel in &n {
        let src = repo_root().join(rel);
        let tag = src
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("x")
            .to_string();
        let dir = scratch(&tag);
        let exe = dir.join("prog");

        // ① native：编译 + 链接 + 跑
        let cmd = BuildCommand {
            roots: Vec::new(),
            files: vec![src.clone()],
            output: Some(exe.clone()),
            emit: Emit::Elf,
            keep_asm: false,
            opt: rgoc_ssa::opt::Config::all(),
            lang: None,
        };
        match build_cmd::run_build(&cmd) {
            Ok(out) if out.exit == 0 => {}
            // 静态拒绝（`CgCode`）⇒ 归 `M'`，只数分母。
            Ok(out) if out.exit == 1 => {
                rejected_by_cg.push(format!("{rel} [{}]", out.stderr.trim()));
                continue;
            }
            Ok(out) => {
                failures.push(format!(
                    "{rel}: build exit={} {}",
                    out.exit,
                    out.stderr.trim()
                ));
                continue;
            }
            Err(e) => {
                failures.push(format!("{rel}: build 失败 {e}"));
                continue;
            }
        }
        let run = match std::process::Command::new(&exe).output() {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("{rel}: 跑不起来 {e}"));
                continue;
            }
        };
        if !run.status.success() {
            failures.push(format!(
                "{rel}: 退出码 {:?}，stderr={}",
                run.status.code(),
                String::from_utf8_lossy(&run.stderr).trim()
            ));
            continue;
        }
        let native = String::from_utf8_lossy(&run.stdout).into_owned();

        // ② 解释器：同一份源码
        let rcmd = RunCommand {
            roots: Vec::new(),
            files: vec![src.clone()],
            dump_hir: false,
            max_steps: None,
            lang: None,
            via: Via::Hir,
            opt: rgoc_ssa::opt::Config::all(),
            sched: rgoc_driver::cli::Sched::default(),
        };
        let hir = match rgoc_driver::run_cmd::run_run(&rcmd) {
            Ok(o) if o.exit == 0 => o.stdout,
            Ok(o) => {
                failures.push(format!("{rel}: 解释器 exit={} {}", o.exit, o.stderr.trim()));
                continue;
            }
            Err(e) => {
                failures.push(format!("{rel}: 解释器失败 {e}"));
                continue;
            }
        };

        if native != hir {
            failures.push(format!(
                "{rel}: 输出不一致\n  native={native:?}\n  hir   ={hir:?}"
            ));
            continue;
        }
        if !native.is_empty() {
            with_output += 1;
        }
        compared += 1;
    }

    assert!(
        failures.is_empty(),
        "{} 条不一致：\n{}",
        failures.len(),
        failures.join("\n")
    );
    // 分母写出来：空集合上的全称命题不是证据。
    assert_eq!(
        compared + rejected_by_cg.len(),
        n.len(),
        "分母对不上：compared={compared} cg拒={} n={}",
        rejected_by_cg.len(),
        n.len()
    );
    // 牙齿：全是空输出时「两边都是空」不构成证据（`M7-tests.md` 的分栏判据）。
    assert!(
        with_output >= 3,
        "有输出的语料只剩 {with_output} 个（应 ≥ 3）—— 判据已经退化成「全空全绿」。\
         拒绝清单进了 {} 个：{:?}",
        rejected_by_cg.len(),
        rejected_by_cg
    );
}
