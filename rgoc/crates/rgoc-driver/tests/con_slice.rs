//! 并发语料切片的**冻结**断言（`M8-plan.md` Task 56；`T-CON-54`）。
//!
//! # 为什么要有这一层
//!
//! `chan-input.txt` / `chan-rejected.txt` 是手写的清单，而清单会和实现一起漂：
//! 哪天我们多支持了一个 stdlib import，`M'` 就该少一行 —— 但没人会记得改清单。
//! 这一层把「清单 ↔ 目录实况」钉住：**目录里有而清单没有 ⇒ 红**。
//!
//! # 三条不变量
//!
//! 1. `N' + M' == 目录里的 .go 数`（19）；
//! 2. `M'` 与 `chan-rejected.txt` **集合相同**；
//! 3. `M'` 每条类别 ∈ {`errorcheck`, `stdlib`}，且 `perm` 在 `M'` 里、理由含
//!    `errorcheck`（它是 `<-chan int` 赋给 `chan int` 的类型错用例，该拒）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 仓库根的语料目录。
fn chan_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../go_source_code/test/chan")
}

fn oracle_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tools/run-oracle")
}

/// 目录里实有的 `.go` 文件（相对仓库根的路径）。
fn dir_files() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let rd = std::fs::read_dir(chan_dir()).expect("读不了 test/chan 目录");
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "go") {
            out.insert(format!(
                "go_source_code/test/chan/{}",
                p.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    out
}

/// 清单里的条目（剥注释与空行）。
fn list_files(name: &str) -> BTreeSet<String> {
    let text = std::fs::read_to_string(oracle_dir().join(name)).expect("读不了清单");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// 拒绝清单里的**路径**（第一栏）。
fn rejected_paths() -> BTreeSet<String> {
    rejected_rows().into_iter().map(|(p, _, _)| p).collect()
}

/// `chan-rejected.txt` 的 `(路径, 类别, 理由)`。
fn rejected_rows() -> Vec<(String, String, String)> {
    let text =
        std::fs::read_to_string(oracle_dir().join("chan-rejected.txt")).expect("读不了拒绝清单");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut it = l.split('\t');
            (
                it.next().unwrap_or_default().to_string(),
                it.next().unwrap_or_default().to_string(),
                it.next().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

#[test]
fn t_con_54_清单就是目录_拒绝是它的子集() {
    let all = dir_files();
    let input = list_files("chan-input.txt");
    let rejected = rejected_paths();

    assert_eq!(all.len(), 19, "test/chan 下应当有 19 个 .go：{all:?}");
    // `chan-input.txt` 是**整张**语料（19 行），不是 N' —— N' 是跑出来的
    // （19 - 12 = 7）。所以第一条不变量是「清单 == 目录」。
    assert_eq!(
        input,
        all,
        "chan-input.txt 必须逐项等于目录（差异：{:?}）",
        all.symmetric_difference(&input).collect::<Vec<_>>()
    );
    assert!(
        rejected.is_subset(&input),
        "被拒的文件必须在清单里（{:?}）",
        rejected.difference(&input).collect::<Vec<_>>()
    );
    assert_eq!(
        input.len() - rejected.len(),
        7,
        "N' = 清单 - M' 必须等于冻结值 7"
    );
}

#[test]
fn t_con_54b_每条拒绝都有类别与理由() {
    for (path, cat, why) in rejected_rows() {
        assert!(
            matches!(cat.as_str(), "errorcheck" | "stdlib"),
            "{path} 的类别 {cat:?} 不在 {{errorcheck, stdlib}} 里"
        );
        assert!(!why.is_empty(), "{path} 没有理由（三栏的第二栏不许空）");
    }
}

#[test]
fn t_con_54c_perm_是_errorcheck_用例() {
    // `perm.go` 拒它的理由是**类型错**（`<-chan int` 赋给 `chan int`）——
    // 它本来就是 errorcheck 用例，拒它是对的；换成「我们跑不动」就成了假红。
    let row = rejected_rows()
        .into_iter()
        .find(|(p, _, _)| p.ends_with("perm.go"))
        .expect("perm.go 必须在 M' 里");
    assert_eq!(
        row.1, "errorcheck",
        "perm.go 的类别应当是 errorcheck：{row:?}"
    );
    assert!(
        row.2.contains("errorcheck") || row.2.contains("CHK-INVALID-ASSIGN"),
        "理由要点明它是类型错用例：{row:?}"
    );
}

#[test]
fn t_con_54d_拒绝条数是冻结的() {
    assert_eq!(
        rejected_rows().len(),
        12,
        "M' 条数是冻结值 12（11 stdlib + 1 errorcheck）—— 变了说明支持面动了"
    );
}
