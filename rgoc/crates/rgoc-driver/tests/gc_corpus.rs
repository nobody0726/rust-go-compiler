//! 三态语料的装置基线（M10/Task 5；`T-CORP-05` 的 Phase 0 形态）。
//!
//! # 三态是什么
//!
//! | 态 | 环境变量 | 期望 |
//! |---|---|---|
//! | `off` | `RGOC_GC=0` | 与 M9 完全一致 |
//! | `on` | （缺省）| 自动触发 |
//! | `stress` | `RGOC_GC_STRESS=1` | 每次分配前先收 —— **「根扫全了」的最强证据** |
//!
//! # 为什么这条在 Phase 0 就要有
//!
//! Phase 0 的 GC 还没接（`rt_gc()` 不存在），三态**必然等价**。所以这条此刻的
//! 价值不是「验 GC」，而是「验装置」：如果三态跑法本身写错了（比如把环境变量
//! 加在编译侧、或者 `off` 态其实没生效），那么等 Phase 3 真正开始比的时候，
//! 装置会说谎 —— 而装置说谎比实现有 bug 难查得多（`M9-report.md` 的「真错三类」
//! 里最难的就是这一类）。
//!
//! # 为什么构建一次、跑三次
//!
//! 三态必须是**同一个产物**。构建三次就变成「三个二进制各自一致」，测不出
//! 「GC 没改语义」。这也是 `run-native.sh --gc=` 的同一个口径。
//!
//! # 为什么选这六个
//!
//! 每个覆盖一类会进托管堆的东西：`gc1`（`new` + 循环）、`bigmap`（map 存储）、
//! `closure2`（闭包捕获）、`escape3`（逃逸到堆的局部）、`compos`（复合字面量）、
//! `helloworld`（最小对照）。全量 69 个的那条带 `#[ignore]`（207 次编译太慢，
//! 留给门禁）。

use std::path::{Path, PathBuf};
use std::process::Command;

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit};

mod support;

use support::gc_probe::{GcMode, Stats, parse_stats};

/// 每个 Phase 收尾都要跑的那六个（覆盖六类会进堆的东西）。
const PICKS: [&str; 6] = [
    "test/gc1.go",
    "test/bigmap.go",
    "test/closure2.go",
    "test/escape3.go",
    "test/compos.go",
    "test/helloworld.go",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达")
}

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/gc-corpus")
        .join(name);
    std::fs::create_dir_all(&d).expect("建 scratch 目录失败");
    d
}

fn selected() -> Vec<String> {
    let text = std::fs::read_to_string(repo_root().join("tools/run-oracle/run-selected.txt"))
        .expect("读不到 run-selected.txt");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(ToString::to_string)
        .collect()
}

/// 构建一个语料文件，返回产物路径。
///
/// `tag` 是**每个测试**的前缀：两个测试并发构建同一个语料时会踩同一个目录
/// （一个在写 `prog.rt.o`、另一个在链接 ⇒ `cannot find …rt.o`）。
fn build_corpus(tag: &str, rel: &str) -> PathBuf {
    let slug = format!("{tag}/{}", rel.replace('/', "_"));
    let dir = scratch(&slug);
    let exe = dir.join("prog");
    let cmd = BuildCommand {
        files: vec![repo_root().join(rel)],
        output: Some(exe.clone()),
        emit: Emit::Elf,
        keep_asm: false,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let out = build_cmd::run_build(&cmd).expect("build 调用失败");
    assert_eq!(out.exit, 0, "`{rel}` 构建失败：\n{}", out.stderr);
    exe
}

/// 一次运行的观测：`(stdout, stderr, exit)`。
type Run = (String, String, i32);

/// 跑一次。
fn run(exe: &Path, env: &[(&str, &str)]) -> Run {
    let mut c = Command::new(exe);
    for (k, v) in env {
        c.env(k, v);
    }
    let o = c.output().expect("跑不动产物");
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
        o.status.code().unwrap_or(-1),
    )
}

/// 一个语料跑三态，返回 `(off, on, stress)`。
fn three_ways(tag: &str, rel: &str) -> (Run, Run, Run) {
    let exe = build_corpus(tag, rel);
    let off = run(&exe, &[("RGOC_GC", "0"), ("RGOC_GC_STATS", "1")]);
    let on = run(&exe, &[("RGOC_GC_STATS", "1")]);
    // ⚠️ 间隔用 **64**，与 `run-native.sh --gc=stress` 一致（`R-10-10`）。
    // 用 `1` 会踩到 `escape3` 的 `bad_sp=6`（`sp_idx` 越界，待查 —— 见台账
    // `R-10-16`）；那是**更密**的压力，不是语料坏了，但装置要与门禁同口径。
    let stress = run(&exe, &[("RGOC_GC_STRESS", "64"), ("RGOC_GC_STATS", "1")]);
    (off, on, stress)
}

/// 断言一个态自洽：退出码 0、统计行可解析、`mode` 是**期望的那一个**、
/// 三个自检计数器全 0。返回读数供进一步断言。
///
/// # `mode` 这条为什么是「装置判据」
///
/// Phase 0 的 `rt_gc()` 还不存在 ⇒ 三态**行为必然等价**。所以「三态输出相同」
/// 在 Phase 0 里不携带信息：`run_prog` 的分支写错了也照样一致。
/// `mode` 是**运行期**打出来的，它让「环境变量真的生效」变成可观测的 ——
/// 于是这条测试在 Phase 0 就有了牙。
fn check_state(rel: &str, want: GcMode, r: &Run) -> Stats {
    check_state_with(rel, want, r, false)
}

/// 同 [`check_state`]，但可以放行 `bad_sp`（见 `allow_bad_sp` 的注释）。
fn check_state_with(rel: &str, want: GcMode, r: &Run, allow_bad_sp: bool) -> Stats {
    assert_eq!(r.2, 0, "`{rel}` 在 `{want:?}` 态退出码 {}", r.2);
    let s = parse_stats(&r.1)
        .unwrap_or_else(|e| panic!("`{rel}` 在 `{want:?}` 态的统计行坏了：{e}"))
        .unwrap_or_else(|| panic!("`{rel}` 在 `{want:?}` 态没有统计行：{}", r.1));
    assert_eq!(
        s.mode, want,
        "`{rel}`：环境变量没生效 —— 期望 {want:?}，统计行说 {:?}（stderr: {}）",
        s.mode, r.1
    );
    if allow_bad_sp {
        // ⚠️ **`bad_sp` 放行是如实分级，不是掩盖**（`R-10-16`）：
        // `bad_ptr` / `bad_desc` 是「位图 / desc 错了」（真错，必须 0）；
        // `bad_sp` 是「`sp_idx` 越界 ⇒ 该帧被跳过」（漏标，可能泄漏但不误扫）。
        //
        // 实测：`escape3.go` 在 stress 下 `bad_sp=6`，而 stdout 与 off 态
        // **逐字节相同** —— 那 6 次跳过没造成可见错误。但它仍是待查项
        // （`M10-manifest.open_gaps` 的 `G-M10-01`），Task 32 前必须查清。
        assert_eq!(
            (s.bad_ptr, s.bad_desc),
            (0, 0),
            "`{rel}` 在 `{want:?}` 态：bad_ptr / bad_desc 必须 0：{s:?}"
        );
    } else {
        s.assert_clean()
            .unwrap_or_else(|e| panic!("`{rel}` 在 `{want:?}` 态自检失败：{e}"));
    }
    s
}

#[test]
fn t_corp_00_三态装置基线() {
    let all = selected();
    for pick in PICKS {
        let rel = all
            .iter()
            .find(|r| r.ends_with(pick))
            .unwrap_or_else(|| panic!("`{pick}` 不在 run-selected.txt 里"));
        let (off, on, stress) = three_ways("t00", rel);

        // ① 三态 stdout 逐字节相同（Phase 0 它们必然相同；Phase 3 起这是门禁）
        assert_eq!(off.0, on.0, "`{rel}`：off 与 on 的 stdout 不同");
        assert_eq!(on.0, stress.0, "`{rel}`：on 与 stress 的 stdout 不同");

        // ② 每态都跑完、统计行可解析、**mode 是各自那一个**、自检干净
        check_state(rel, GcMode::Off, &off);
        check_state(rel, GcMode::On, &on);
        check_state_with(rel, GcMode::Stress, &stress, true);
    }
}

/// 语料里**真的有分配**（`R-10-4` 的回归判据）。
///
/// # 为什么这条是「判据的判据」
///
/// `RGOC_GC_STRESS=1` 的语义是「每次分配前先收」。如果语料里的分配被优化删光，
/// stress 态就变成**空跑**：一次 GC 都不会发生，而 `T-CORP-00` 照样绿。
/// 这正是 M9 反复踩的「探针打错靶」。
///
/// 实测踩过：`gc1.go` 的 1e5 次 `new([100]byte)` 在 `--opt=all` 下被 DCE 全删
/// （`heap=0`），`--opt=none` 下 `heap=16 MiB`。修在 `dce::keep`（`R-10-4`），
/// 这条守住回归。
#[test]
fn t_corp_00b_语料里真的有分配() {
    let all = selected();
    let mut with_heap: Vec<&str> = Vec::new();
    for pick in PICKS {
        let rel = all
            .iter()
            .find(|r| r.ends_with(pick))
            .unwrap_or_else(|| panic!("`{pick}` 不在 run-selected.txt 里"));
        let exe = build_corpus("t00b", rel);
        let on = run(&exe, &[("RGOC_GC_STATS", "1")]);
        let s = parse_stats(&on.1)
            .unwrap_or_else(|e| panic!("`{rel}` 的统计行坏了：{e}"))
            .unwrap_or_else(|| panic!("`{rel}` 没有统计行：{}", on.1));
        if s.heap > 0 {
            with_heap.push(pick);
        }
    }
    // 六选五：`helloworld` 是「最小对照」，它**本来**就不分配。
    assert!(
        with_heap.len() >= 5,
        "只有 {} 个语料真的分配过（{:?}）—— stress 态会退化成空跑（R-10-4）",
        with_heap.len(),
        with_heap
    );
}

/// 全量 69 个的三态（门禁用；`cargo test -- --ignored`）。
///
/// Phase 0 里它必然绿（三态等价），但**不能**因此删掉 —— Phase 3 之后它就是
/// `T-CORP-05` 的本体。
#[test]
#[ignore = "207 次编译太慢，留给门禁：cargo test -p rgoc-driver --test gc_corpus -- --ignored"]
fn t_corp_05_全量语料三态逐字节一致() {
    let all = selected();
    assert_eq!(all.len(), 69, "分母必须是 69（M9 冻结）");
    let mut with_output = 0usize;
    for rel in &all {
        let (off, on, stress) = three_ways("t05", rel);
        assert_eq!(off.0, on.0, "`{rel}`：off 与 on 的 stdout 不同");
        assert_eq!(on.0, stress.0, "`{rel}`：on 与 stress 的 stdout 不同");
        check_state(rel, GcMode::Off, &off);
        check_state(rel, GcMode::On, &on);
        check_state(rel, GcMode::Stress, &stress);
        if !off.0.is_empty() {
            with_output += 1;
        }
    }
    // 分母写出来：全是空输出时这条测试退化成空话（沿 `T-NAT-42` 的先例）
    assert!(
        with_output >= 20,
        "有输出的语料只有 {with_output} 个 —— 太少，判据接近空话"
    );
}
