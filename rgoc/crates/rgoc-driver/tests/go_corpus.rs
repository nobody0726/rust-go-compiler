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

mod support;

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
        roots: Vec::new(),
        files: vec![repo_root().join(rel)],
        output: Some(dir.join("prog")),
        emit: Emit::Elf,
        // ⚠️ **必须保留 **： 时链接器读的是临时路径，
        // 而 （第一个真编译成功的并发文件）在这个模式下报
        //  —— 那是装置层面的路径问题。
        // 判据要检查的是**构建结果**，多留一个  无害。
        keep_asm: true,
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
        // ⚠️ **不要求「全被拒」**：`make(chan)` 在 M11b/Task 16 放行后，
        // `zerosize.go` 已经能编译（exit=0）。这条判据只钉「拒绝原因里没有
        // `go 语句`」—— 某个文件是 0 还是 1 是支持面的事，不是这条的事。
        assert!(
            exit == 0 || exit == 1,
            "`{rel}` 应是编译成功或干净拒绝，实际 {exit}"
        );
        if err.contains("go 语句") {
            still_go.push(format!("{rel} ⇒ {}", err.trim()));
        }
    }
    assert!(
        still_go.is_empty(),
        "这些仍因 `go 语句` 被拒（Task 13 的目标是把它从拒绝面拿掉）：\n  {}",
        still_go.join("\n  ")
    );

    // ── ③ 原来这里断言「`zerosize.go` 干净拒绝且带码」——**删掉了** ──────
    //
    // M11b/Task 16 的 `make(chan)` 放行后，这个文件**能编译了**
    // （`make(chan [0]byte)` / `make(chan struct{}` 都是合法分配），
    // 「必须 exit=1」的前提没了。
    //
    // 为什么不再换个断言：`build_once` 用 `Emit::Elf`，而这个文件的 `.s` 在
    // 那个模式下没落到 `prog.s`（同目录直接 `build` 是 rc=0）—— 那是**装置**
    // 的路径问题。与其为它改装置，不如让 `run-native-chan.sh` 覆盖：它数的是
    // **行为**（构建一次、跑三次、比字节），比退出码强。
    //
    // 这条判据的主旨（`N'` 不 panic）由 ① ② 两段保证，没丢。
}

/// `T-GCORP-05`（M11c/Task 35）：**7 个 `N'` 的三态逐字节相同**。
///
/// # 为什么三态要在这条判据里，而不是只靠 `run-native-chan.sh`
///
/// 装置脚本证明的是「一次构建、跑三次」这件事能跑通；这条证明的是**每一个**
/// `N'` 在 `off` / `on` / `stress` 下的 `stdout` / `stderr` / 退出码三样
/// **完全一样**。两者不重复：脚本的汇总行只数「有输出 / 空跑」，看不出
/// 「`stress` 态少打了一个字节」。
///
/// # 为什么 `stderr` 也要比
///
/// `C9` §5 的 panic / 死锁文本只进 stderr。只比 stdout 的话，「`stress` 态
/// 打了一行 panic 但 stdout 碰巧一样」会漏过去。
///
/// ⚠️ 三态都**不设** `RGOC_GC_STATS` —— 统计行进 stderr，设了就三态必然不同
/// （`mode=off` / `on` / `stress`），那是**预期**的差异，不是判据要抓的东西。
#[test]
fn t_gcorp_05_并发语料三态() {
    let ok = chan_ok();
    assert_eq!(ok.len(), 7, "`N'` 应为 7");
    let mut bad: Vec<String> = Vec::new();
    for rel in &ok {
        let src = std::fs::read_to_string(repo_root().join(rel))
            .unwrap_or_else(|e| panic!("读不到 `{rel}`：{e}"));
        let slug = rel.rsplit('/').next().unwrap_or(rel).replace(".go", "");
        let mut seen: Option<(String, String, i32)> = None;
        for (tag, env) in [
            ("off", vec![("RGOC_GC", "0")]),
            ("on", vec![]),
            ("stress", vec![("RGOC_GC_STRESS", "8")]),
        ] {
            let out =
                support::native_harness::build_and_run_env(&format!("g5{slug}{tag}"), &src, &env)
                    .unwrap_or_else(|e| panic!("`{rel}` 的 {tag} 态跑不起来：\n{e}"));
            if out.exit != 0 {
                bad.push(format!(
                    "{rel} [{tag}]: 退出码 {}（stderr {:?}）",
                    out.exit, out.stderr
                ));
            }
            match &seen {
                None => seen = Some((out.stdout, out.stderr, out.exit)),
                Some((o, e, x)) => {
                    if out.stdout != *o {
                        bad.push(format!(
                            "{rel} [{tag}]: stdout {:?} ≠ off 态 {:?}",
                            out.stdout, o
                        ));
                    }
                    if out.stderr != *e {
                        bad.push(format!(
                            "{rel} [{tag}]: stderr {:?} ≠ off 态 {:?}",
                            out.stderr, e
                        ));
                    }
                    if out.exit != *x {
                        bad.push(format!("{rel} [{tag}]: 退出码 {} ≠ off 态 {}", out.exit, x));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "并发语料三态不一致：\n{}", bad.join("\n"));
}

/// `T-GCORP-01`：**7 个 `N'` 三层一致**（`M11-tests.md` 的主判据）。
///
/// # 三层
///
/// | 层 | 从哪来 |
/// |---|---|
/// | native | `rgoc build` 出来的 aarch64 ELF |
/// | 解释器 | `--via=hir`（M8 那份实现，与 native **完全独立**）|
/// | 官方 | 这 7 个文件的 `// run` 语义是「**只在失败时打印**」⇒ 期望 = 空输出 + 退出 0 |
///
/// # 为什么这是主判据而不是「跑通就行」
///
/// 「跑通」有两种可能：**语义对**，或者**两边都错得一样**。三层里 native 与
/// 解释器是两套独立实现（一份 arm64 汇编生成、一份树遍历求值），它们同时
/// 错成同一个输出的概率远低于各自对 —— 而官方那层（空输出 + 0）钉住了
/// 「这两个文件本来就不该打印任何东西」。
///
/// ⚠️ 官方层**不跑 `go run`**：语料目录是只读的，而且 `go run` 在容器里要
/// `-buildvcs=false`（见记忆）。这 7 个文件的判据形态是「静默成功」，
/// 所以「空输出 + 退出 0」就是官方语义的**完整**表达 —— 不存在需要比对的
/// 期望字符串。
#[test]
fn t_gcorp_01_七个_n_三层一致() {
    let ok = chan_ok();
    assert_eq!(ok.len(), 7, "`N'` 应为 7");
    let mut bad: Vec<String> = Vec::new();
    for rel in &ok {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../..")
                .join(rel),
        )
        .unwrap_or_else(|e| panic!("读不到 `{rel}`：{e}"));
        let slug = rel.rsplit('/').next().unwrap_or(rel).replace(".go", "");

        // native
        let nat = support::native_harness::build_and_run_env(&format!("g1n{slug}"), &src, &[])
            .unwrap_or_else(|e| panic!("`{rel}` native 侧失败：\n{e}"));
        // 解释器
        let interp = support::native_harness::hir_output(&format!("g1h{slug}"), &src)
            .unwrap_or_else(|e| panic!("`{rel}` 解释器侧失败：\n{e}"));

        // ① native vs 解释器（stdout 逐字节）
        if nat.stdout != interp {
            bad.push(format!(
                "{rel}: native stdout {:?} ≠ 解释器 {:?}",
                nat.stdout, interp
            ));
        }
        // ② native vs 官方（空输出 + 退出 0）
        if !nat.stdout.is_empty() || nat.exit != 0 {
            bad.push(format!(
                "{rel}: 官方语义是静默成功，实际 exit={} stdout={:?}",
                nat.exit, nat.stdout
            ));
        }
        // ③ 解释器 vs 官方
        if !interp.is_empty() {
            bad.push(format!("{rel}: 解释器不该有输出，实际 {interp:?}"));
        }
    }
    assert!(bad.is_empty(), "三层不一致：\n{}", bad.join("\n"));
}
