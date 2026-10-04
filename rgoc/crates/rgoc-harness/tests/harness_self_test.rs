//! T36 的验收测试：harness 六类自测（**E3 门禁**）。
//!
//! 六类各自对应 `M0-tests.md` 的 T-H-01 ~ T-H-06：
//! 成功 / 失败 / 未知指令 / 平台过滤 / 超时 / 版本不符。
//!
//! **每一类都带反例**（`M0-plan` §8 的执行纪律）—— 只证明「能判失败」是不够的，
//! 还要证明「不会把正常的东西判成失败」。计划里点名的那条：
//! 「T-H-04 除『被过滤』外，还要证明『合法用例不会被误过滤』」。
//!
//! 这些测试会**真的调用容器内的 go1.27.1**（超时与版本守门对 mock 没有意义）。
//!
//! TDD 位置：**RED**。T36 实现前本文件编译不过（`rgoc_harness::runner` 尚不存在）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use rgoc_harness::corpus::CorpusConfig;
use rgoc_harness::ir::{Layer, Limits, Verdict};
use rgoc_harness::oracle::OracleConfig;
use rgoc_harness::runner::{CaseSpec, run_case, run_layer};

// ══ 工具 ═══════════════════════════════════════════════════════════════════

/// 一个必然通过的小程序：用内建 `println`（写 **stderr**，R2b 的关键样本）。
const 小程序: &str = r#"package main

func main() { println("hello") }
"#;

/// oracle 配置：`work_dir` 用临时目录（官方默认是 GOROOT/test）。
fn oracle_cfg(dir: &Path, per_case: Duration) -> OracleConfig {
    let mut c = OracleConfig::from(CorpusConfig::m0());
    c.work_dir = dir.to_path_buf();
    c.limits = Limits::for_layer(Layer::Corpus).with_per_case_override(per_case);
    c
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("m0-t36-{tag}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&d);
    d
}

/// 造一个 `run` 层用例的 spec。
fn run_spec(
    dir: &std::path::Path,
    name: &'static str,
    directive: &str,
    body: &str,
    expected: Option<&str>,
) -> CaseSpec<'static> {
    let mut spec = 落盘(dir, name, leak(&format!("{directive}\n\n{body}")));
    spec.expected_out = expected.map(str::to_string);
    spec
}

/// 把字符串泄漏成 `&'static str`（测试夹具用；进程短命，无妨）。
fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// 泄漏一个 Path（测试夹具用）。
fn leak_path(dir: &std::path::Path, name: &str) -> &'static std::path::Path {
    Box::leak(dir.join(name).into_boxed_path())
}

/// 把源码写进 `dir` 并造一个 spec。
///
/// **必须落盘**：执行器只读磁盘上的文件（真实语料在只读的 GOROOT/test 里），
/// 它从不写工作目录 —— 所以合成用例得由调用方准备好。
fn 落盘(dir: &std::path::Path, name: &'static str, src: &'static str) -> CaseSpec<'static> {
    std::fs::write(dir.join(name), src).expect("写 fixture");
    CaseSpec {
        name,
        path: leak_path(dir, name),
        src,
        expected_out: None,
        layer: Layer::Corpus,
    }
}

fn m0() -> CorpusConfig {
    CorpusConfig::m0()
}

// ══ T-H-01 成功 ════════════════════════════════════════════════════════════

#[test]
fn th01_成功_输出与_out_一致() {
    let dir = temp_dir("th01");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_secs(60)))
        .expect("oracle");
    let s = run_spec(&dir, "x.go", "// run", 小程序, Some("hello\n"));
    let out = run_case(&s, &m0(), &oracle);
    assert_eq!(out.verdict, Verdict::Pass, "应当通过：{out:?}");
    assert!(out.in_denominator, "成功用例当然在分母内");
}

#[test]
fn th01_成功_缺_out_且无输出() {
    // R2：缺 .out 即期望为**空** —— 无输出就是通过
    let dir = temp_dir("th01b");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_secs(60)))
        .expect("oracle");
    let quiet = "package main\n\nfunc main() {}\n";
    let s = run_spec(&dir, "y.go", "// run", quiet, None);
    let out = run_case(&s, &m0(), &oracle);
    assert_eq!(out.verdict, Verdict::Pass, "{out:?}");
}

#[test]
fn th01_成功_编译层_编译通过即通过() {
    let dir = temp_dir("th01c");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_secs(60)))
        .expect("oracle");
    let s = 落盘(
        &dir,
        "z.go",
        leak("// compile\n\npackage main\n\nfunc main() {}\n"),
    );
    let out = run_case(&s, &m0(), &oracle);
    assert_eq!(out.verdict, Verdict::Pass, "{out:?}");
}

// ══ T-H-02 失败 ════════════════════════════════════════════════════════════

#[test]
fn th02_失败_输出与_out_不符() {
    let dir = temp_dir("th02");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_secs(60)))
        .expect("oracle");
    let s = run_spec(&dir, "x.go", "// run", 小程序, Some("goodbye\n"));
    let out = run_case(&s, &m0(), &oracle);
    assert_eq!(
        out.verdict,
        Verdict::RuntimeFailure,
        "run 层输出不符 = 运行结果不对：{out:?}"
    );
    assert!(out.detail.contains("不匹配"), "detail 要说清原因：{out:?}");
}

#[test]
fn th02_失败_缺_out_却有输出() {
    // R2 的另一半：无 .out 却有输出 ⇒ 同样失败，且措辞是「本应为空」
    let dir = temp_dir("th02b");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_secs(60)))
        .expect("oracle");
    let s = run_spec(&dir, "x.go", "// run", 小程序, None);
    let out = run_case(&s, &m0(), &oracle);
    assert_eq!(out.verdict, Verdict::RuntimeFailure, "{out:?}");
    assert!(
        out.detail.contains("本应为空"),
        "缺 .out 时的 detail 措辞要对：{out:?}"
    );
}

#[test]
fn th02_失败_编译层_编译不过() {
    let dir = temp_dir("th02c");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_secs(60)))
        .expect("oracle");
    let s = 落盘(
        &dir,
        "b.go",
        leak("// compile\n\npackage main\n\nfunc main() { this is not go }\n"),
    );
    let out = run_case(&s, &m0(), &oracle);
    assert_eq!(
        out.verdict,
        Verdict::CompilerFailure,
        "编译层编译失败 = compiler-failure：{out:?}"
    );
}

#[test]
fn th02_反例_同样的输入_期望对上就通过() {
    // 证明判定不是「一律失败」：只把 .out 换回正确的那份
    let dir = temp_dir("th02d");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_secs(60)))
        .expect("oracle");
    let s = run_spec(&dir, "x.go", "// run", 小程序, Some("hello\n"));
    assert_eq!(run_case(&s, &m0(), &oracle).verdict, Verdict::Pass);
}

// ══ T-H-03 未知指令 ═════════════════════════════════════════════════════════

#[test]
fn th03_未知指令_判_harness_failure_而不是_compiler_failure() {
    // 这是本任务最容易判错的一类：**harness 读不懂指令**是判卷机自己的问题，
    // 绝不能记成「被测编译器编译失败」。
    let dir = temp_dir("th03");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_secs(60)))
        .expect("oracle");
    let s = run_spec(&dir, "x.go", "// notacommand", 小程序, Some("hello\n"));
    let out = run_case(&s, &m0(), &oracle);
    assert_eq!(
        out.verdict,
        Verdict::HarnessFailure,
        "未知指令必须是 harness-failure：{out:?}"
    );
    assert!(
        out.detail.contains("unknown pattern"),
        "detail 要指出是未知指令：{out:?}"
    );
}

#[test]
fn th03_未知指令_不属于_compiler_failure() {
    // 显式反向断言：`compiler-failure` 意味着「被测件编译失败」，
    // 而这里连指令都没读懂，两者不可混同
    let dir = temp_dir("th03b");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_secs(60)))
        .expect("oracle");
    let s = run_spec(&dir, "x.go", "// notacommand", 小程序, Some("hello\n"));
    let out = run_case(&s, &m0(), &oracle);
    assert_ne!(out.verdict, Verdict::CompilerFailure);
    assert_ne!(out.verdict, Verdict::RuntimeFailure);
    assert!(out.verdict.is_infrastructure(), "应当属基建类：{out:?}");
}

#[test]
fn th03_反例_合法指令不被误判() {
    let dir = temp_dir("th03c");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_secs(60)))
        .expect("oracle");
    for d in ["// run", "// compile", "// errorcheck"] {
        let body = if d == "// errorcheck" {
            "package main\n\nfunc main() { undefinedThing() }\n"
        } else {
            小程序
        };
        let s = run_spec(&dir, "x.go", d, body, None);
        let out = run_case(&s, &m0(), &oracle);
        assert_ne!(
            out.verdict,
            Verdict::HarnessFailure,
            "{d} 是合法指令，不该判 harness-failure：{out:?}"
        );
    }
}

// ══ T-H-04 平台过滤 ════════════════════════════════════════════════════════

#[test]
fn th04_平台过滤_判_target_filtered() {
    let dir = temp_dir("th04");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_secs(60)))
        .expect("oracle");
    // 目标是 linux/arm64；这条只满足 windows ⇒ 被过滤。
    // ⚠️ 构建约束**之后还得有指令行**（`// run`）—— 只写约束的话，
    // R1 跳过后第一条非约束行就成了 `package main`，action 变成 "package"（未知指令）。
    // 被过滤的用例会短路、**根本走不到指令判定**，所以这个错只有下面那条「反例」才暴露。
    let body = "package main\n\nfunc main() { println(\"hi\") }\n";
    let mut s = 落盘(
        &dir,
        "w.go",
        leak(&format!("//go:build windows\n// run\n\n{body}")),
    );
    s.expected_out = Some("hi\n".to_string());
    let out = run_case(&s, &m0(), &oracle);
    assert_eq!(out.verdict, Verdict::TargetFiltered, "{out:?}");
}

#[test]
fn th04_平台过滤_仍计入分母但不计入分子() {
    // `03` §3.3：过滤项**不计入分子、仍计入分母** —— 分子分母口径不同，必须分开记
    let dir = temp_dir("th04b");
    let oc = oracle_cfg(&dir, Duration::from_secs(60));
    let body = "package main\n\nfunc main() { println(\"hi\") }\n";
    let specs = vec![
        {
            let mut s = 落盘(
                &dir,
                "win.go",
                leak(&format!("//go:build windows\n// run\n\n{body}")),
            );
            s.expected_out = Some("hi\n".to_string());
            s
        },
        {
            let mut s = 落盘(
                &dir,
                "lin.go",
                leak(&format!("//go:build linux\n// run\n\n{body}")),
            );
            s.expected_out = Some("hi\n".to_string());
            s
        },
    ];
    let rep = run_layer(&specs, &oc, &m0());
    assert_eq!(rep.denominator(), 2, "两个都在分母里");
    assert_eq!(rep.passed(), 1, "只有一个计入分子（被过滤的那个不算）");
    assert_eq!(rep.count(Verdict::TargetFiltered), 1);
    assert!(
        rep.by_verdict().contains_key(&Verdict::TargetFiltered),
        "按判定分类可聚合"
    );
}

#[test]
fn th04_反例_合法用例不被误过滤() {
    let dir = temp_dir("th04c");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_secs(60)))
        .expect("oracle");
    let body = "package main\n\nfunc main() { println(\"hi\") }\n";
    let mut s = 落盘(
        &dir,
        "lin.go",
        leak(&format!("//go:build linux\n// run\n\n{body}")),
    );
    s.expected_out = Some("hi\n".to_string());
    let out = run_case(&s, &m0(), &oracle);
    assert_eq!(out.verdict, Verdict::Pass, "linux 用例不该被过滤：{out:?}");
}

// ══ T-H-05 超时 ════════════════════════════════════════════════════════════

#[test]
fn th05_超时_判_timeout_且进程被回收() {
    let dir = temp_dir("th05");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_millis(500)))
        .expect("oracle");
    let sleeper =
        "package main\n\nimport \"time\"\n\nfunc main() { time.Sleep(30 * time.Second) }\n";
    let s = 落盘(&dir, "slow.go", leak(&format!("// run\n\n{sleeper}")));
    let out = run_case(&s, &m0(), &oracle);
    assert_eq!(out.verdict, Verdict::Timeout, "{out:?}");
    // 关键：**子进程真的被回收了**（T-H-05 的硬要求）
    assert!(
        out.child_pid.is_some(),
        "应当带回子进程 pid 以便验证回收：{out:?}"
    );
    if let Some(pid) = out.child_pid {
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "超时后子进程 {pid} 仍在 —— 只杀了父进程、没杀干净"
        );
    }
}

#[test]
fn th05_反例_快程序在同样预算下通过() {
    let dir = temp_dir("th05b");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_millis(500)))
        .expect("oracle");
    let s = run_spec(&dir, "fast.go", "// run", 小程序, Some("hello\n"));
    let out = run_case(&s, &m0(), &oracle);
    assert_eq!(out.verdict, Verdict::Pass, "不该误判超时：{out:?}");
}

// ══ T-H-06 版本不符 ════════════════════════════════════════════════════════

#[test]
fn th06_版本不符_整层判_reference_toolchain_failure_且不产出比较结果() {
    let dir = temp_dir("th06");
    let mut oc = oracle_cfg(&dir, Duration::from_secs(60));
    oc.expect_version = "go1.24.5".into(); // 宿主那个版本
    let specs = vec![run_spec_owned(
        &dir,
        "x.go",
        "// run",
        小程序,
        Some("hello\n"),
    )];
    let rep = run_layer(&specs, &oc, &m0());
    assert_eq!(rep.denominator(), 1, "用例仍计入分母");
    assert_eq!(rep.passed(), 0, "**一个都不该通过**");
    assert_eq!(rep.count(Verdict::ReferenceToolchainFailure), 1);
    assert!(
        rep.oracle_error.as_deref().unwrap_or("").contains("拒绝"),
        "要能看出是「拒绝作基线」：{:?}",
        rep.oracle_error
    );
    // 最关键：没有真的去跑任何比较
    assert!(
        rep.outcomes.iter().all(|o| o.duration == Duration::ZERO),
        "版本不符时不该执行任何用例：{:?}",
        rep.outcomes
    );
}

#[test]
fn th06_反例_版本相符时正常产出结果() {
    let dir = temp_dir("th06b");
    let oc = oracle_cfg(&dir, Duration::from_secs(60));
    let specs = vec![run_spec_owned(
        &dir,
        "x.go",
        "// run",
        小程序,
        Some("hello\n"),
    )];
    let rep = run_layer(&specs, &oc, &m0());
    assert!(rep.oracle_error.is_none(), "版本相符不该有层级错误");
    assert_eq!(rep.passed(), 1, "应当正常通过");
    assert!(rep.outcomes[0].duration > Duration::ZERO, "应当真的跑了");
}

/// 持有一份 `src` 的 spec（`run_layer` 收 `&[CaseSpec]`）。
fn run_spec_owned(
    dir: &std::path::Path,
    name: &'static str,
    directive: &str,
    body: &str,
    expected: Option<&str>,
) -> CaseSpec<'static> {
    let mut spec = 落盘(dir, name, leak(&format!("{directive}\n\n{body}")));
    spec.expected_out = expected.map(str::to_string);
    spec
}

// ══ 跨类：判定分类与计数 ════════════════════════════════════════════════════

#[test]
fn 跨类_只有_pass_计入分子() {
    // §8 判定纪律：过滤项、失败、基建问题都**不得**记 pass
    let dir = temp_dir("跨类");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_secs(60)))
        .expect("oracle");
    let quiet = "package main\n\nfunc main() {}\n";
    let cases: Vec<(&str, Verdict)> = vec![
        ("// run", Verdict::Pass), // 缺 .out + 无输出 = 通过
        ("// notacommand", Verdict::HarnessFailure),
    ];
    for (d, want) in cases {
        let s = run_spec(&dir, "x.go", d, quiet, None);
        let out = run_case(&s, &m0(), &oracle);
        assert_eq!(out.verdict, want, "{d}: {out:?}");
        assert_eq!(
            out.verdict.counts_toward_numerator(),
            out.verdict == Verdict::Pass,
            "{} 的计入分子判定不自洽",
            out.verdict
        );
    }
}

#[test]
fn 跨类_被过滤的用例不执行_oracle() {
    // 被平台过滤的用例**根本不该跑** —— 判卷机省下的不只是时间，
    // 更是「不让不该跑的语料影响结果」。
    let dir = temp_dir("跨类b");
    let oracle = rgoc_harness::oracle::Oracle::new(oracle_cfg(&dir, Duration::from_millis(300)))
        .expect("oracle");
    // 一个「如果真跑就会超时」的程序 + 不满足的构建约束
    let sleeper =
        "package main\n\nimport \"time\"\n\nfunc main() { time.Sleep(30 * time.Second) }\n";
    let s = 落盘(
        &dir,
        "w.go",
        leak(&format!("//go:build windows\n// run\n\n{sleeper}")),
    );
    let out = run_case(&s, &m0(), &oracle);
    assert_eq!(out.verdict, Verdict::TargetFiltered, "{out:?}");
    assert_eq!(
        out.duration,
        Duration::ZERO,
        "被过滤的用例不该花时间：{out:?}"
    );
}
