//! M11 并发语料的拒绝面（`GO-7` 的 `T-GCORP-02/03`）。
//!
//! # 这一组语料的两半
//!
//! `tools/run-oracle/chan-input.txt` 是 `test/chan/*.go` 全部 19 个。
//! 其中 12 个在**检查面**就被拒（`chan-rejected.txt`：11 个 `import` stdlib +
//! 1 个 errorcheck 用例），剩下 7 个是 `N'`。
//!
//! `N'` 的清单**算出来**（`chan-input − chan-rejected`），不抄第二份 ——
//! 抄一份就会有第三份、然后三份互相不一致（`T-GDEV-01`）。
//!
//! # 为什么这条判据的靶子是 `N'`，不是 `M'`
//!
//! 原来的写法（`M11-plan.md` Task 1 的初稿）是「12 个 `M'` 的退出码没有 101」。
//! **实测发现靶子打错了**：12 个 `M'` 全部 `rc=1`（它们走 `import` / `check`
//! 的拒绝路径，**根本到不了** `effect.rs` 的 `unreachable!`）；真正 `rc=101`
//! 的是 `N'` 里的 **6 个**：
//!
//! `select.go` / `select4.go` / `select6.go` / `select8.go` / `sendstmt.go` /
//! `sieve1.go` —— 它们含 `go` / `select` / `ch <- v`，`unsupported()` 本该拒掉，
//! 但 `build.rs:60` 的 `effect::summarize` 跑在 `unsupported()`（647 行）**之前**，
//! 于是先撞上 `effect.rs:211` 的 `unreachable!`。
//!
//! 所以这条判据要两半都查：`M'` 的 12 个必须干净拒绝（守住既有面），`N'` 的
//! 7 个**绝不允许 101**（这是 `F3` 的回归判据）。
//!
//! # 为什么用 `catch_unwind`
//!
//! `build_cmd::run_build` 不捕获 panic。不包一层的话，第一个 panic 的文件会
//! 把整个测试进程打死，只能看到一个文件名 —— 而这条判据的价值恰恰是
//! 「**6 个**全都不 panic」。所以逐个 `catch_unwind`，把 panic 折成 `rc=101`。

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达")
}

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/go-corpus")
        .join(name);
    std::fs::create_dir_all(&d).expect("建 scratch 目录失败");
    d
}

/// 读一份清单的第一列（跳过注释与空行）。
fn read_list(name: &str) -> Vec<String> {
    let text = std::fs::read_to_string(repo_root().join("tools/run-oracle").join(name))
        .unwrap_or_else(|e| panic!("读不到 {name}：{e}"));
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split('\t').next().unwrap_or(l).trim().to_string())
        .collect()
}

/// `N'` = `chan-input` − `chan-rejected`（算出来，不抄）。
fn chan_ok() -> Vec<String> {
    let all = read_list("chan-input.txt");
    let rej = read_list("chan-rejected.txt");
    let mut out: Vec<String> = all.into_iter().filter(|p| !rej.contains(p)).collect();
    out.sort();
    out
}

/// 一次 build 的形态：`(exit, stderr)`。panic 折成 `(101, "<panic>")`。
fn build_once(tag: &str, rel: &str) -> (i32, String) {
    let dir = scratch(&format!("{tag}/{}", rel.replace('/', "_")));
    let cmd = BuildCommand {
        files: vec![repo_root().join(rel)],
        output: Some(dir.join("prog")),
        emit: Emit::Elf,
        keep_asm: false,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    match catch_unwind(AssertUnwindSafe(|| build_cmd::run_build(&cmd))) {
        Ok(Ok(out)) => (i32::from(out.exit), out.stderr),
        Ok(Err(e)) => (2, e),
        Err(_) => (101, "<panicked>".to_string()),
    }
}

#[test]
fn t_gcorp_03_并发语料不_panic() {
    // ── ① `M'`：12 个必须干净拒绝（守住既有面）──────────────────────────
    let rejected = read_list("chan-rejected.txt");
    assert_eq!(
        rejected.len(),
        12,
        "`chan-rejected.txt` 的行数变了（冻结在 12，`T-GDEV-03`）"
    );
    let mut bad_m: Vec<String> = Vec::new();
    for rel in &rejected {
        let (exit, err) = build_once("m", rel);
        if exit != 1 {
            bad_m.push(format!("{rel} ⇒ exit={exit}（要 1）stderr={}", err.trim()));
        } else if err.trim().is_empty() {
            bad_m.push(format!("{rel} ⇒ exit=1 但 stderr 空（拒绝必须有理由）"));
        }
    }
    assert!(
        bad_m.is_empty(),
        "`M'` 里这些不是「干净拒绝」：\n  {}",
        bad_m.join("\n  ")
    );

    // ── ② `N'`：7 个**绝不允许 101**（`F3` 的回归判据）──────────────────
    let ok = chan_ok();
    assert_eq!(
        ok.len(),
        7,
        "`N'` 应为 7（19 − 12），实际 {}：{ok:?}",
        ok.len()
    );
    let mut panicked: Vec<String> = Vec::new();
    let mut other: Vec<String> = Vec::new();
    for rel in &ok {
        let (exit, err) = build_once("n", rel);
        match exit {
            // 0 = M11 已实现；1 = 干净拒绝（M11a 之前是常态）。两者都合法。
            0 | 1 => {}
            101 => panicked.push(rel.clone()),
            n => other.push(format!("{rel} ⇒ exit={n} stderr={}", err.trim())),
        }
    }
    assert!(
        panicked.is_empty(),
        "`N'` 里这些 build 时 **panic**（`F3`：`effect::summarize` 跑在 \
         `unsupported()` 之前，撞上 `effect.rs:211` 的 `unreachable!`）：\n  {}\n\
         期望：exit=1 + `SSA-UNSUPPORTED-CHAN`（或 M11 实现后 exit=0）。",
        panicked.join("\n  ")
    );
    assert!(
        other.is_empty(),
        "`N'` 里这些是内部失败（exit 2/其他），不是拒绝也不是成功：\n  {}",
        other.join("\n  ")
    );

    // ── ③ 已经能走到 SSA 面的那个，必须给出可读的诊断 ─────────────────
    let (exit, err) = build_once("n", "go_source_code/test/chan/zerosize.go");
    assert_eq!(exit, 1, "`zerosize.go` 应干净拒绝");
    assert!(
        err.contains("SSA-UNSUPPORTED-CHAN"),
        "`zerosize.go` 的诊断里应出现 `SSA-UNSUPPORTED-CHAN`，实际：\n{err}"
    );
}

// ── 装置判据（`T-GDEV-01/03`）────────────────────────────────────────────
//
// `T-GDEV-02`（三栏相加 == 分母）由 `run-native-chan.sh` **自己**断言 ——
// 在 Rust 里重算一遍只会得到第二套口径。

/// `T-GDEV-01`：`N'` 与 `M'` 是全集的一个**划分**（不重不漏）。
///
/// 这条验的是**清单之间**的一致性，不是脚本。脚本自己算差集；这里验的是
/// 「两份事实源能不能构成划分」—— 抄错一个路径、把某个文件同时写进两边、
/// 或者全集里漏掉一行，都会在这里红。
#[test]
fn t_gdev_01_并发清单是全集的一个划分() {
    let all = read_list("chan-input.txt");
    let rej = read_list("chan-rejected.txt");
    let ok = chan_ok();

    assert_eq!(
        all.len(),
        19,
        "`chan-input.txt` 应为 19（test/chan/*.go 全部）"
    );
    assert_eq!(
        rej.len(),
        12,
        "`chan-rejected.txt` 应为 12（冻结，`T-GDEV-03`）"
    );
    assert_eq!(ok.len(), 7, "`N'` 应为 19 − 12 = 7");

    // 不重：`N'` 与 `M'` 无交集。
    for p in &ok {
        assert!(!rej.contains(p), "`{p}` 同时出现在 N' 与 M' 里");
    }
    // 不漏：两半加起来就是全集。
    let mut union: Vec<String> = ok.iter().chain(rej.iter()).cloned().collect();
    union.sort();
    let mut sorted_all = all.clone();
    sorted_all.sort();
    assert_eq!(union, sorted_all, "N' ∪ M' 应等于全集（不多不少）");
}

/// `T-GDEV-03`：`chan-rejected.txt` 的行数**冻结**。
///
/// 变了说明语料性质变了 —— 要么我们多支持了一个 import（那要改的是
/// `M11-design.md` §1.3 的范围），要么官方换了用例（那要重审 19 个的分母）。
/// 两种情况都必须**人看一眼**，不能静默通过。
#[test]
fn t_gdev_03_拒面行数冻结() {
    let rej = read_list("chan-rejected.txt");
    assert_eq!(rej.len(), 12, "拒面行数变了：{rej:?}");
}

/// `T-GCORP-04`（Task 13 收尾）：**`go` 不再是拒绝原因**。
///
/// # 为什么这条要单独存在
///
/// `t_gcorp_03` 只要求「不 panic」—— 它允许 `N'` 全被拒（Phase 0 就是这样）。
/// 而 Task 13 的目标是「`go` 语句进得来」：如果只靠 `t_gcorp_03`，**把 `go`
/// 又加回拒绝面也照样绿**。
///
/// 所以这条钉的是**拒绝原因的内容**：7 个 `N'` 仍全被拒（它们要 `hchan`），
/// 但没有一个是因为 `go 语句`。
#[test]
fn t_gcorp_04_go_不再是拒绝原因() {
    let ok = chan_ok();
    let mut still_go: Vec<String> = Vec::new();
    for rel in &ok {
        let (exit, err) = build_once("n", rel);
        // 现在仍然全拒（`make(chan)` / `select` / `send` 都是 M11b 的事）。
        assert_eq!(exit, 1, "`{rel}` 现在应仍被拒（channel 还没做）");
        if err.contains("go 语句") {
            still_go.push(format!("{rel} ⇒ {}", err.trim()));
        }
    }
    assert!(
        still_go.is_empty(),
        "这些仍因 `go 语句` 被拒（Task 13 的目标是把它从拒绝面拿掉）：\n  {}",
        still_go.join("\n  ")
    );
}
