//! SSA 求值器在**入选语料**上的闭环（`T-SSA-32/34/35/36/37` 的 rgoc 侧半边）。
//!
//! # 与 `ssa_run.rs` 的分工
//!
//! `ssa_run.rs` 是**手写 fixture**：每条只钉一个语义点（递归、字段、切片共享…），
//! 失败时能立刻指到哪一处。这一条是**语料**：68 个真实 Go 文件（`N` 侧）跑下来
//! 与官方 `.out` 逐字节一致 —— 它回答的是另一个问题：「这些语义点组合起来还对不对」。
//!
//! 手写 fixture 全绿而语料全红是**完全可能**的（`M4` 的教训：弱绿只证明「不乱报」）。
//!
//! # 判据
//!
//! | 断言 | 为什么 |
//! |---|---|
//! | 退出码 0 | `M5` 的 `T-EX-42` 同口径；非 0 说明 `panic`/超预算 |
//! | 合并流 == `<同名>.out`（无 `.out` ⇒ 期望为空） | 官方语义（`testdir_test.go:1178`） |
//! | `compared == N` | 分母写出来：空集合上的全称命题不是证据 |
//!
//! # `M` 侧为什么直接跳过
//!
//! `build()` 拒绝的文件（23 个，全带白名单码）没有 SSA 可跑 —— 它们由
//! `ssa_reject.rs` 与 `ssa_slice.rs` 负责。这里**只数分母**，不静默当成通过。

use std::path::{Path, PathBuf};

use rgoc_check::CheckOptions;
use rgoc_load::load_with_imports;
use rgoc_ssa::build::build;
use rgoc_types::TypeTable;

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

/// 跑一个文件的 SSA 路径。`None` ⇒ 被构建期拒绝（`M` 侧）。
fn run_one(src: &Path) -> Option<(String, i32, Option<String>)> {
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, &[src.to_path_buf()], &[], None);
    let opts = CheckOptions { test_hooks: false };
    let (checked, lowered) =
        rgoc_check::check::check_package_with(&pkg, &mut tt, bag, &opts, |s| {
            rgoc_hir::lower(&pkg, s)
        });
    assert_eq!(
        checked.rendered_count(),
        0,
        "{} 不该有静态诊断（入选集都是 M5 绿的）",
        src.display()
    );
    let program = lowered.unwrap_or_else(|_| panic!("{} 被 lowering 拒绝", src.display()));
    let out = build(&program, &mut tt);
    if !out.rejected.is_empty() {
        return None;
    }
    let o = rgoc_interp::run_ssa(&out.module, &tt, rgoc_interp::Options::default());
    Some((o.output.replace("\r\n", "\n"), o.exit, o.diag))
}

#[test]
fn t_ssa_32_入选集上_ssa_与官方期望逐字节一致() {
    let root = repo_root();
    let rows = selected();
    let mut bad: Vec<String> = Vec::new();
    let mut compared = 0usize;
    let mut rejected = 0usize;

    for rel in &rows {
        let src = root.join(rel);
        let Some((got, exit, diag)) = run_one(&src) else {
            rejected += 1;
            continue;
        };
        compared += 1;
        if exit != 0 {
            bad.push(format!(
                "{rel}: 退出码 {exit}（判据只接受 0）\n  诊断={diag:?}\n  输出={got:?}"
            ));
            continue;
        }
        let want = match std::fs::read_to_string(src.with_extension("out")) {
            Ok(s) => s.replace("\r\n", "\n"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                bad.push(format!("{rel}: 读不到 .out：{e}"));
                continue;
            }
        };
        if got != want {
            bad.push(format!(
                "{rel}: 合并流与期望不同（期望 {} 字节，实际 {} 字节）\n  期望={want:?}\n  实际={got:?}",
                want.len(),
                got.len()
            ));
        }
    }

    assert!(
        bad.is_empty(),
        "入选集有 {} 条与官方期望不符：\n{}",
        bad.len(),
        bad.join("\n\n")
    );
    // 分母：69 个可建图 + 0 个被拒 == 69（`R-6-1` 冻结；M9/Phase 5c 重冻结 `R-9-26`）
    assert_eq!(
        compared + rejected,
        rows.len(),
        "每个入选文件都要么跑过、要么被拒（不许静默跳过）"
    );
    assert_eq!(
        compared, 69,
        "可建图的入选文件应恰为 69 个（`M6-tests.md` §6 的 `N`，M9/Phase 5c 重冻结 `R-9-26`），实际 {compared}"
    );
    assert_eq!(
        rejected, 0,
        "被拒的入选文件应恰为 1 个（`M6-tests.md` §6 的 `M`，M9/Phase 5c 重冻结 `R-9-26`），实际 {rejected}"
    );
}
