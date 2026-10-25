//! M7 native 的**冻结切片**与两向断言（`T-NAT-40`，`R-6-1` 同款）。
//!
//! # 为什么要冻结，而不是「每次跑出来是多少就是多少」
//!
//! `N'` / `M'` 是「M7 支持哪些程序」的**承诺**。如果它随每次运行浮动，那么
//! 「今天 39 绿」与「昨天 39 绿」可能根本不是同一批文件 —— 于是「绿」不构成证据。
//! 冻结把它们变成可对账的事实：新增或消失都必须**改这个文件**，改不动就是回归。
//!
//! # 三向断言
//!
//! | 断言 | 挡住什么 |
//! |---|---|
//! | `|N'| + |M'| == N` | 分母。少了/多了都说明清单被改过 |
//! | `M'` 的路径集合 == `native-rejected.txt` | 悄悄放行（本该拒的编过了）或悄悄收缩（原本能跑的现在拒了） |
//! | 每条码 ∈ `CgCode::ALL` | 拒绝原因必须是**码表里的码**，不是随手一句话 |
//!
//! # 4 个「有输出」的语料里为什么只有 3 个在 `N'`
//!
//! `M7-design.md` `Q2` 的理由是「保住全部 4 个有输出的语料」，实际只能保 3 个：
//! `ken/string.go` 用字符串切片（`c[0:3]`）与 `string(z1[0:])`，那是 `[]T` 面。
//! 所以它在 `M'` 里，且 `M'` 的**唯一**成员原因也确实是切片 —— 这条写在这里，
//! 不让它变成「设计稿说了 4 个，实现只做到 3 个而没人提」。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rgoc_arm64::diag::CgCode;

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达")
}

fn line_list(rel: &str) -> Vec<String> {
    let path = repo_root().join(rel);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到 {}：{e}", path.display()));
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(ToString::to_string)
        .collect()
}

#[test]
fn t_nat_40_冻结的分母对得上() {
    // `N` = 入选语料 − M6 拒绝面。
    let m6: BTreeSet<String> = line_list("tools/run-oracle/ssa-rejected.txt")
        .iter()
        .filter_map(|l| l.split('\t').next())
        .map(ToString::to_string)
        .collect();
    let n: BTreeSet<String> = line_list("tools/run-oracle/run-selected.txt")
        .into_iter()
        .filter(|p| !m6.contains(p))
        .collect();
    let input: BTreeSet<String> = line_list("tools/run-oracle/native-input.txt")
        .into_iter()
        .collect();

    // `native-input.txt` 就是 `N` 的快照（`run-native.sh` 的输入）。
    assert_eq!(input, n, "native-input.txt 与「入选 − M6 拒绝」不一致");
    assert_eq!(
        n.len(),
        60,
        "N 侧应为 60（46 → map +4 → iface +6 → typeswitch +1 → funcval +3）"
    );

    // `M'` 的路径集合。
    let m: BTreeSet<String> = line_list("tools/run-oracle/native-rejected.txt")
        .iter()
        .filter_map(|l| l.split('\t').next())
        .map(ToString::to_string)
        .collect();
    assert_eq!(m.len(), 0, "M' 在 M9/Phase 1 冻结为 0（切片已实现）");
    assert!(
        m.is_subset(&n),
        "M' 里出现了不在 N 里的路径：{:?}",
        m.difference(&n).collect::<Vec<_>>()
    );
    assert_eq!(
        n.len() - m.len(),
        60,
        "N' 冻结为 60（M7 的 39 + 切片 7 + map 4 + iface 6 + typeswitch 1 + funcval 3）"
    );
}

#[test]
fn t_nat_40_拒绝原因必须是码表里的码() {
    let all: BTreeSet<&str> = CgCode::ALL.iter().map(|c| c.as_str()).collect();
    let rows = line_list("tools/run-oracle/native-rejected.txt");
    for row in &rows {
        let cols: Vec<&str> = row.split('\t').collect();
        assert!(cols.len() >= 3, "行格式应为 <路径>\\t<码>\\t<原因>：{row}");
        assert!(
            all.contains(cols[1]),
            "码 {:?} 不在 CgCode::ALL 里（{}）",
            cols[1],
            CgCode::ALL.len()
        );
    }
}

/// `CG-UNSUPPORTED-SLICE` 已下线（M9/Task 15）：源码里扫不到它。
///
/// 沿 M8 的「五条码不许再出现」做法：**留一条永远不产出的码**比删掉更糟 ——
/// 它会让人以为「切片还是不支持」，而实际是「这条码没人发了」。
/// 扫的是 `crates/`（**不含** `tests/`：负向断言本身要能写这条字符串）。
#[test]
fn t_m9_15_slice_码不许再出现() {
    let root = repo_root().join("rgoc/crates");
    let mut hits = Vec::new();
    fn scan(dir: &Path, hits: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().and_then(|n| n.to_str()) == Some("tests") {
                    continue;
                }
                scan(&p, hits);
            } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                // 只看**代码行**：注释里提到这条码是合法的（比如 diag.rs 的
                // 「它为什么下线了」）。判据要抓的是「还有人会产出它」——
                // 也就是非注释行里的字符串。
                let in_code = text.lines().any(|l| {
                    !l.trim_start().starts_with("//") && l.contains("CG-UNSUPPORTED-SLICE")
                });
                if in_code {
                    hits.push(p.clone());
                }
            }
        }
    }
    scan(&root, &mut hits);
    assert!(
        hits.is_empty(),
        "`CG-UNSUPPORTED-SLICE` 已下线（M9/Task 15），但仍在这些文件里出现：{hits:?}"
    );
    assert_eq!(CgCode::ALL.len(), 2, "CgCode::ALL 应为 2 条");
}

#[test]
fn t_nat_40_有输出的语料三个在_n_prime_一个在_m_prime() {
    // 4 个非空输出：helloworld / printbig / cplx0 在 `N'`，`ken/string` 在 `M'`。
    let ok = [
        "go_source_code/test/helloworld.go",
        "go_source_code/test/printbig.go",
        "go_source_code/test/ken/cplx0.go",
    ];
    let rejected: BTreeSet<String> = line_list("tools/run-oracle/native-rejected.txt")
        .iter()
        .filter_map(|l| l.split('\t').next())
        .map(ToString::to_string)
        .collect();
    for p in ok {
        assert!(!rejected.contains(p), "{p} 有输出，必须在 N' 里");
    }
    // M9/Phase 1 起切片支持了 ⇒ `ken/string` 也在 `N'`（设计稿当年想保的 4 个，
    // 到 M9 才真的保住）。
    assert!(
        !rejected.contains("go_source_code/test/ken/string.go"),
        "ken/string 的字符串切片已支持，必须在 N' 里"
    );
}
