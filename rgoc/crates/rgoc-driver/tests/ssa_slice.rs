//! 语料切片的**冻结值**与两向自洽（`T-SSA-31`；`R-6-1` 的落地）。
//!
//! # 这条测试要防的三类谎
//!
//! | 谎 | 长什么样 | 这里怎么拦 |
//! |---|---|---|
//! | 「大部分绿」 | 只说「63 个能建图」，不说另外 6 个去哪了 | `N + M == 69` 两向断言 + 两个数都写死 |
//! | 「拒绝清单是装饰」 | 清单里塞了幽灵文件 / 漏了真被拒的 | 逐文件**重跑一遍**构建器，实测集合 == 清单集合（两向） |
//! | 「拒绝没码」 | 拒了但只说「不支持」 | 每行三栏、码必须落在 `SsaCode::ALL` 里 |
//!
//! # 为什么要在测试里重跑一遍分类
//!
//! 清单是**冻结**的：它一旦与代码脱节，后续所有「63/6」的引用都成了历史陈述。
//! 重跑一遍的成本是 1~2 秒（69 个文件），换来的是「清单与今天的构建器一致」——
//! 少了这一条，把某个 `N` 文件改坏、或者把某个 `M` 文件的拒绝修好，都不会有人发现。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rgoc_check::CheckOptions;
use rgoc_load::load_with_imports;
use rgoc_ssa::build::build;
use rgoc_ssa::diag::SsaCode;
use rgoc_types::TypeTable;

/// 入选总数（`run-selected.txt` 的非注释行数，M5 冻结）。
const SELECTED: usize = 69;
/// 能建图（`N`）—— 本文件冻的就是这两个数（`M6-tests.md` §6）。
const BUILDABLE: usize = 68;
/// 被拒（`M`）。
const REJECTED: usize = 1;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达")
}

fn rows(name: &str) -> Vec<String> {
    let path = repo_root().join("tools/run-oracle").join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到 {name}：{e}"));
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(ToString::to_string)
        .collect()
}

/// 白名单码的文本集合（**唯一来源**是 `SsaCode::ALL`，不在这里抄一份）。
fn whitelist() -> BTreeSet<&'static str> {
    SsaCode::ALL.iter().map(|c| c.code()).collect()
}

/// 拒绝清单解析成 `路径 → 码`。
fn rejected_map() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in rows("ssa-rejected.txt") {
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(
            cols.len(),
            3,
            "拒绝清单每行必须三栏（文件/码/原因），实际 {} 栏：{line:?}",
            cols.len()
        );
        let (path, code, why) = (cols[0].trim(), cols[1].trim(), cols[2].trim());
        assert!(
            !why.is_empty(),
            "{path}: 原因栏为空（空栏与「漏填」无法区分）"
        );
        assert!(
            whitelist().contains(code),
            "{path}: 码 {code} 不在白名单 {:?} 里",
            whitelist()
        );
        assert!(
            out.insert(path.to_string(), code.to_string()).is_none(),
            "{path}: 在拒绝清单里出现了两次"
        );
    }
    out
}

#[test]
fn t_ssa_31_切片冻结值两向自洽() {
    let sel: BTreeSet<String> = rows("run-selected.txt").into_iter().collect();
    assert_eq!(sel.len(), SELECTED, "入选清单行数变了（M5 冻结的 69）");
    let rej = rejected_map();
    assert_eq!(rej.len(), REJECTED, "拒绝清单行数变了（冻结 1）");
    assert_eq!(
        BUILDABLE + REJECTED,
        SELECTED,
        "N + M 必须等于 69（少一个文件 = 有人静默跳过了它）"
    );
    // 两向：拒绝集 ⊆ 入选集（没有幽灵），且差集恰好是能建图的那批
    for path in rej.keys() {
        assert!(
            sel.contains(path),
            "{path}: 在拒绝清单里但不在入选清单里（幽灵行）"
        );
    }
    assert_eq!(sel.len() - rej.len(), BUILDABLE, "入选 − 拒绝 必须等于 68");
}

#[test]
fn t_ssa_31_清单与实测一致() {
    let root = repo_root();
    let listed = rejected_map();
    let mut actual: BTreeMap<String, String> = BTreeMap::new();

    for rel in rows("run-selected.txt") {
        let src = root.join(&rel);
        let mut tt = TypeTable::new();
        let (pkg, bag) = load_with_imports(&mut tt, std::slice::from_ref(&src), &[], None);
        let opts = CheckOptions { test_hooks: false };
        let (checked, lowered) =
            rgoc_check::check::check_package_with(&pkg, &mut tt, bag, &opts, |s| {
                rgoc_hir::lower(&pkg, s)
            });
        assert_eq!(
            checked.rendered_count(),
            0,
            "{rel}: 入选集不该有静态诊断（M5 全绿）"
        );
        let program = lowered.unwrap_or_else(|_| panic!("{rel}: 被 lowering 拒绝（M5 能跑）"));
        let out = build(&program, &mut tt);
        if let Some(r) = out.rejected.first() {
            actual.insert(rel.clone(), r.code.code().to_string());
        }
    }

    // 两向：实测被拒的 == 清单写的（顺序无关，按路径比）
    let listed_paths: BTreeSet<&String> = listed.keys().collect();
    let actual_paths: BTreeSet<&String> = actual.keys().collect();
    let stale: Vec<_> = listed_paths.difference(&actual_paths).collect();
    let missing: Vec<_> = actual_paths.difference(&listed_paths).collect();
    assert!(
        stale.is_empty(),
        "清单里这些文件**今天已经不被拒了**（清单过期）：{stale:?}"
    );
    assert!(
        missing.is_empty(),
        "这些文件今天**被拒了但清单里没有**（漏登记）：{missing:?}"
    );
    // 码也要一致：同一个文件换了拒绝理由，清单同样过期
    for (path, code) in &actual {
        assert_eq!(
            listed.get(path),
            Some(code),
            "{path}: 拒绝码变了（清单 {listed:?}，实测 {code}）",
            listed = listed.get(path)
        );
    }
    assert_eq!(actual.len(), REJECTED, "实测被拒数应为 1");
}
