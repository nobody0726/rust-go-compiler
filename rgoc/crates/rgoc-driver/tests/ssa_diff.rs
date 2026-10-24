//! 两条执行通路的**三方差分**（`T-SSA-32/34/35/36/37` 的驱动侧半边）。
//!
//! # 与 `ssa_corpus_run.rs` 的分工
//!
//! | | 走哪条路 | 比什么 |
//! |---|---|---|
//! | `ssa_corpus_run.rs` | 库（`load → check → lower → build → run_ssa`） | SSA vs 官方 `.out` |
//! | 本文件 | **CLI**（`run --via=hir` / `--via=ssa`） | HIR vs SSA（外加 `.out` 作锚） |
//!
//! 两条都要有：前者证明「SSA 算得对」，后者证明「`--via` 的接线没有把两条路走岔」
//! （拒绝渲染、退出码、`verify` 的介入都在驱动层，库测试看不见）。
//!
//! # `M` 侧为什么是 SKIP 而不是红
//!
//! 23 个文件在构建期就被拒（白名单 9 条码）。它们的 `--via=ssa` 退出码是 1，
//! 而 `--via=hir` 是 0 —— 那不是「不一致」，是**能力边界**。判据必须是
//! 「有 `SSA-UNSUPPORTED-*` 码」而不是「退出码 1」：只看退出码的话，
//! 任何 SSA 侧的红都能被一句「它被拒了」解释掉。

use std::path::{Path, PathBuf};

use rgoc_driver::cli::{RunCommand, Via};
use rgoc_driver::run_cmd::run_run;

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达")
}

/// 入选清单的非注释非空行。
fn selected() -> Vec<String> {
    let path = repo_root().join("tools/run-oracle/run-selected.txt");
    let text = std::fs::read_to_string(&path).expect("读不到 run-selected.txt");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(ToString::to_string)
        .collect()
}

fn run_via(src: &Path, via: Via) -> (String, u8, String) {
    let o = run_run(&RunCommand {
        files: vec![src.to_path_buf()],
        dump_hir: false,
        max_steps: None,
        lang: None,
        via,
        opt: rgoc_ssa::opt::Config::all(),
    })
    .unwrap_or_else(|e| panic!("{} 跑 {} 失败（内部错）：{e}", src.display(), via.as_str()));
    (o.stdout.replace("\r\n", "\n"), o.exit, o.stderr)
}

#[test]
fn t_ssa_32_两条通路在入选集上逐字节一致() {
    let root = repo_root();
    let rows = selected();
    let mut bad: Vec<String> = Vec::new();
    let mut compared = 0usize;
    let mut skipped = 0usize;

    for rel in &rows {
        let src = root.join(rel);
        let (hir_out, hir_exit, hir_err) = run_via(&src, Via::Hir);
        let (ssa_out, ssa_exit, ssa_err) = run_via(&src, Via::Ssa);

        // M 侧：构建期拒绝（白名单码 + 退出码 1）
        if ssa_exit == 1 && ssa_err.contains("SSA-UNSUPPORTED-") {
            skipped += 1;
            // 拒绝是**静态**结论：HIR 那边必须能正常跑（否则这条语料本身有问题）
            assert_eq!(
                hir_exit, 0,
                "{rel}: SSA 拒了但 HIR 也跑不动（hir_exit={hir_exit}）—— 语料或装置坏了"
            );
            continue;
        }
        compared += 1;
        if ssa_exit != hir_exit {
            bad.push(format!(
                "{rel}: 退出码不同 hir={hir_exit} ssa={ssa_exit}\n  ssa stderr={ssa_err:?}\n  hir stderr={hir_err:?}"
            ));
            continue;
        }
        if ssa_out != hir_out {
            bad.push(format!(
                "{rel}: 合并流不同\n  hir={hir_out:?}\n  ssa={ssa_out:?}\n  ssa stderr={ssa_err:?}"
            ));
        }
    }

    assert!(
        bad.is_empty(),
        "有 {} 条两条通路不一致：\n{}",
        bad.len(),
        bad.join("\n\n")
    );
    // 分母：46 跑通 + 23 被拒 == 69（`R-6-1` 的冻结值）
    assert_eq!(compared + skipped, rows.len(), "不许静默跳过");
    assert_eq!(compared, 46, "可建图的入选文件应恰为 46 个");
    assert_eq!(skipped, 23, "被拒的入选文件应恰为 23 个");
}
