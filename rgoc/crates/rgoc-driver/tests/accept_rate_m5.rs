//! `T-EX-62` P0 门禁：`check` 面对 run 语料的**接受率两向断言**（M5/Task 6）。
//!
//! # 断言什么
//!
//! | 量 | 值 | 出处 |
//! |---|---|---|
//! | 候选 | **80** | `tools/run-oracle/run-candidates.txt`（机械筛，`F1`） |
//! | 接受 | **79** | 本文件逐个跑 `check` 实测 |
//! | 拒绝 | **1** —— `test/reflectmethod2.go` | 同上 |
//!
//! 两向：**拒绝集合 == {`reflectmethod2.go`}**（不多不少），且
//! `80 == 79 + 1`。只断言「≥ 79」会漏掉「顺手多接受了不该接受的」，
//! 只断言「拒绝 ⊆ 名单」会漏掉「悄悄多拒了一个」—— 两个方向都要钉。
//!
//! # 为什么这条拒绝是**合法**的
//!
//! `reflectmethod2.go` 首行就 `import "reflect"`。M5 的支持面是**无 import 的
//! 单文件程序**（`M5-design.md` §1.3 / `F17`）：标准库 import 要模块解析或
//! 二进制导出数据，两条都超出 M5 的范围（`U-M5-01`）。所以它在
//! `run-excluded.txt` 里以 `stdlib` 类别登记 —— 这条断言同时钉住
//! 「**唯一的**拒绝必须出现在排除清单里」，防止以后新增的假阳性被悄悄塞进清单。
//!
//! # 为什么用 `run_check` 而不是直接 `check_package`
//!
//! `run_check` 就是 CLI 那条路径（可读性 → 版本源 → `load_with_imports` →
//! `check_package` → `rendered_count`）。用它与 `tools/run-oracle/check-all.sh`
//! 的口径**逐字节同源**：那边调的就是 `rgoc-driver check <file>` 的退出码。
//! 直接调 `check_package` 会绕开版本源与装载器，接受率就会与门禁数字漂开。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rgoc_driver::check_cmd::run_check;
use rgoc_driver::cli::CheckCommand;

/// 候选清单里唯一允许被拒绝的文件（`stdlib` 类别，`U-M5-01`）。
const ONLY_REJECT: &str = "go_source_code/test/reflectmethod2.go";

/// 候选总数（语料被 `corpus-manifest.sha256` 锁死 ⇒ 精确值）。
const CANDIDATES: usize = 80;

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达（cargo 的 CARGO_MANIFEST_DIR 变了？）")
}

fn oracle_dir() -> PathBuf {
    repo_root().join("tools/run-oracle")
}

/// 读清单：丢注释与空行，取第一列（TAB 分隔）。
fn read_list(name: &str) -> Vec<String> {
    let p = oracle_dir().join(name);
    let src = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()));
    src.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| l.split('\t').next().unwrap_or("").trim().to_string())
        .collect()
}

/// 读排除清单的完整四列（`<路径>\t<类别>\t<理由>\t<指针>`）。
fn read_excluded() -> Vec<(String, String, String, String)> {
    let p = oracle_dir().join("run-excluded.txt");
    let src = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()));
    src.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            assert_eq!(
                f.len(),
                4,
                "排除行必须是四列 `<路径>\\t<类别>\\t<理由>\\t<指针>`，实际：{l:?}"
            );
            (
                f[0].trim().to_string(),
                f[1].trim().to_string(),
                f[2].trim().to_string(),
                f[3].trim().to_string(),
            )
        })
        .collect()
}

/// 跑一个文件（CLI 同一条路径）。返回 `true` = 接受（零诊断）。
fn accepts(rel: &str) -> bool {
    let path = repo_root().join(rel);
    assert!(
        path.is_file(),
        "{rel}：语料里不存在（语料被 sha256 锁死 ⇒ 是清单写错了）"
    );
    let cmd = CheckCommand {
        files: vec![path],
        diag_dump: false,
        lang: None,
        test_hooks: false,
    };
    let out = run_check(&cmd).unwrap_or_else(|e| panic!("{rel}：check 内部错：{e}"));
    out.errors == 0
}

/// `T-EX-62`（两向）：80 候选里**恰好**一个被拒，且它必须在排除清单里。
#[test]
fn t_ex_62_接受率_79_80_且拒绝集合唯一() {
    let cands = read_list("run-candidates.txt");
    assert_eq!(
        cands.len(),
        CANDIDATES,
        "候选数变了（机械筛口径或语料锁变了）"
    );

    let mut rejected: BTreeSet<String> = BTreeSet::new();
    for rel in &cands {
        if !accepts(rel) {
            rejected.insert(rel.clone());
        }
    }
    let accepted = cands.len() - rejected.len();

    // 方向 ①：拒绝集合**恰好**是那一个（多一个 = 假阳性，少一个 = 假阴性）。
    let want: BTreeSet<String> = [ONLY_REJECT.to_string()].into_iter().collect();
    assert_eq!(
        rejected,
        want,
        "拒绝集合漂了（接受 {accepted}/{}）：多拒/少拒都算红",
        cands.len()
    );

    // 方向 ②：算术闭合。
    assert_eq!(accepted, 79, "接受数应为 79");
    assert_eq!(accepted + rejected.len(), CANDIDATES, "80 != 79 + 1");

    // 方向 ③：那一个拒绝必须在排除清单里，类别必须是 `stdlib`。
    let excluded = read_excluded();
    let hit = excluded
        .iter()
        .find(|(p, _, _, _)| p == ONLY_REJECT)
        .unwrap_or_else(|| panic!("{ONLY_REJECT}：被拒了但不在 run-excluded.txt 里"));
    assert_eq!(
        hit.1, "stdlib",
        "类别应为 stdlib（标准库 import 不在 M5 面）"
    );
    assert!(
        hit.3.contains("U-M5-01") || hit.3.contains("F17"),
        "指针要指向 U-M5-01 / F17，实际：{:?}",
        hit.3
    );
}

/// `T-EX-62`（互补）：排除清单是候选的**子集**，理由条条具体。
///
/// 「候选 == 入选 ∪ 排除」的完整两向断言在 Task 46 落（`T-EX-41`）；这里先钉住
/// 与 P0 门禁直接相关的那一半：排除的每一条**都在** 80 个候选里，且四列齐全、
/// 理由非空。否则「拒绝集合唯一」可以被一句 `echo >> run-excluded.txt` 洗白。
#[test]
fn t_ex_62_排除清单是候选子集且四列齐全() {
    let cands: BTreeSet<String> = read_list("run-candidates.txt").into_iter().collect();
    let excluded = read_excluded();
    assert!(!excluded.is_empty(), "排除清单不该是空的");
    for (p, cat, reason, pointer) in &excluded {
        assert!(cands.contains(p), "{p}：不在候选清单里（死行）");
        assert!(
            ["runoutput", "stdlib", "nondet", "unsupported", "rule"].contains(&cat.as_str()),
            "{p}：类别 {cat:?} 不在 M5-tests.md §11.2 的五个类别里"
        );
        assert!(reason.chars().count() >= 8, "{p}：理由太短（{reason:?}）");
        assert!(!pointer.is_empty(), "{p}：指针为空");
    }
}
