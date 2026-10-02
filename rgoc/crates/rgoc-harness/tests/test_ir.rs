//! T31 的验收测试：Test IR 的必录字段、八种判定结果、冻结的超时/资源上限。
//!
//! **这份文件是 C2 契约（Test IR / 构建条件 / 比较器 / 结果格式）的可执行副本**
//! —— 契约写错时这里会红。字段清单的来源是 `docs/milestones/M0-tests.md`
//! 与 `docs/03-roadmap.md` §3.1，超时/资源上限的数值来源是 `M0-tests.md` §7.5。
//!
//! TDD 位置：**RED**。T31 实现前本文件编译不过（`rgoc_harness::ir` 尚不存在）。

use std::time::Duration;

use rgoc_harness::ir::{
    Comparator, Expected, ExpectedDiag, Layer, Limits, Mode, Target, TestCase, Unsupported, Verdict,
};

// ── 八种判定结果 ────────────────────────────────────────────────────────────

#[test]
fn verdict_恰好八种且可枚举() {
    // 「至少八种且不得合并」：合并就等于把基建失败算成语义失败（`03` §3.3）
    assert_eq!(
        Verdict::ALL.len(),
        8,
        "判定分类必须正好 8 种：{:?}",
        Verdict::ALL.map(|v| v.as_str())
    );
    let names: Vec<&str> = Verdict::ALL.iter().map(|v| v.as_str()).collect();
    for n in [
        "pass",
        "compiler-failure",
        "runtime-failure",
        "harness-failure",
        "target-filtered",
        "timeout",
        "resource-failure",
        "reference-toolchain-failure",
    ] {
        assert!(names.contains(&n), "缺少判定分类 {n}；现有：{names:?}");
    }
    // 标识必须唯一 —— 报告里靠它聚合
    let mut uniq = names.clone();
    uniq.sort_unstable();
    uniq.dedup();
    assert_eq!(uniq.len(), names.len(), "判定标识必须唯一：{names:?}");
}

#[test]
fn verdict_只有_pass_计入分子() {
    // `03` §3.3：过滤项不计入分子、仍计入分母；基建失败绝不记 pass
    for v in Verdict::ALL {
        assert_eq!(
            v.counts_toward_numerator(),
            v == Verdict::Pass,
            "{} 的计入分子判定不对",
            v.as_str()
        );
    }
    assert!(Verdict::Pass.counts_toward_numerator());
    for v in [
        Verdict::CompilerFailure,
        Verdict::RuntimeFailure,
        Verdict::HarnessFailure,
        Verdict::TargetFiltered,
        Verdict::Timeout,
        Verdict::ResourceFailure,
        Verdict::ReferenceToolchainFailure,
    ] {
        assert!(!v.counts_toward_numerator(), "{} 不该计入分子", v.as_str());
    }
}

#[test]
fn verdict_能区分基建失败与语义失败() {
    // harness-failure / reference-toolchain-failure 是「判卷机或 oracle 的问题」，
    // 必须能与「被测件真的错了」区分开，否则基建故障会被当成编译器的 bug。
    let infra = [
        Verdict::HarnessFailure,
        Verdict::ReferenceToolchainFailure,
        Verdict::ResourceFailure,
    ];
    let semantic = [
        Verdict::CompilerFailure,
        Verdict::RuntimeFailure,
        Verdict::Pass,
    ];
    for v in infra {
        assert!(v.is_infrastructure(), "{} 应属基建类", v.as_str());
        for s in semantic {
            assert!(!s.is_infrastructure(), "{} 不该属基建类", s.as_str());
        }
    }
}

// ── 冻结的超时与资源上限（M0-tests §7.5，唯一来源）─────────────────────────

#[test]
fn limits_与冻结值一致() {
    let cases = [
        (Layer::HarnessSelfTest, 30, 180), // T-H-*：30s / 3min
        (Layer::Corpus, 60, 300),          // T-C-*：60s / 5min
        (Layer::SpikeFast, 30, 60),        // T-S1/S2：30s / 1min
        (Layer::SpikeNative, 120, 300),    // T-S3：120s / 5min
        (Layer::Overall, 900, 900),        // 整体 M0 门禁：15min
    ];
    for (layer, per, total) in cases {
        let l = Limits::for_layer(layer);
        assert_eq!(
            l.per_case,
            Duration::from_secs(per),
            "{} 单项超时不对",
            layer.name()
        );
        assert_eq!(
            l.layer_total,
            Duration::from_secs(total),
            "{} 整层超时不对",
            layer.name()
        );
    }
}

#[test]
fn limits_默认_rss_上限是_512mib() {
    // §7.5：单用例峰值 RSS ≤ 512 MiB；T-C-03（gc1.go 分配测试）放宽到 768 MiB
    assert_eq!(
        Limits::for_layer(Layer::Corpus).max_rss_bytes,
        512 * 1024 * 1024
    );
    // 放宽不是改全局，而是「单用例覆盖 + 记明原因」
    let relaxed = Limits::for_layer(Layer::Corpus).with_rss_override(768 * 1024 * 1024);
    assert_eq!(relaxed.max_rss_bytes, 768 * 1024 * 1024);
    assert_eq!(
        relaxed.per_case,
        Duration::from_secs(60),
        "覆盖 RSS 不应动超时"
    );
}

// ── Test IR 的必录字段 ──────────────────────────────────────────────────────

fn 完整用例() -> TestCase {
    TestCase {
        id: "T-C-01".into(),
        rel_path: "helloworld.go".into(),
        input_files: vec!["helloworld.go".into()],
        mode: Mode::Run,
        instruction_args: vec![],
        build_tags: vec![],
        target: Target {
            goos: "linux".into(),
            goarch: "arm64".into(),
            go_version: "go1.27.1".into(),
        },
        imports: vec![],
        feature_deps: vec!["FMT-01".into(), "PRINT-01".into()],
        comparator: Comparator::MergedStreamStrictEq,
        expected: Expected {
            exit_code: 0,
            stdout: "hello, world\n".into(),
            has_out_file: true,
            diagnostics: vec![],
        },
        limits: Limits::for_layer(Layer::Corpus),
        seed: 0,
        milestone: "M0".into(),
        unsupported: None,
    }
}

#[test]
fn test_ir_必录字段齐全且可读回() {
    let c = 完整用例();
    assert_eq!(c.id.as_str(), "T-C-01");
    assert_eq!(c.rel_path.to_string_lossy(), "helloworld.go");
    assert_eq!(c.input_files.len(), 1);
    assert_eq!(c.mode, Mode::Run);
    assert_eq!(c.target.goos, "linux");
    assert_eq!(c.target.goarch, "arm64");
    assert_eq!(c.target.go_version, "go1.27.1");
    assert_eq!(c.comparator, Comparator::MergedStreamStrictEq);
    assert_eq!(c.expected.stdout, "hello, world\n");
    assert!(c.expected.has_out_file);
    assert_eq!(c.limits, Limits::for_layer(Layer::Corpus));
    assert_eq!(c.milestone, "M0");
    assert!(c.unsupported.is_none());
    // 字段齐全的用例必须通过校验
    assert!(
        c.validate().is_ok(),
        "完整用例应当通过 validate：{:?}",
        c.validate()
    );
}

#[test]
fn validate_会挡住缺字段的用例() {
    // 必录字段不是「建议填」—— 缺了就必须报错，而不是静默填默认值
    let base = 完整用例();

    let mut 无id = base.clone();
    无id.id = "".into();
    assert!(无id.validate().is_err(), "空用例 ID 应当被拒");

    let mut 无路径 = base.clone();
    无路径.rel_path = "".into();
    assert!(无路径.validate().is_err(), "空路径应当被拒");

    let mut 比较器错配 = base.clone();
    比较器错配.comparator = Comparator::ExitCodeOnly;
    assert!(
        比较器错配.validate().is_err(),
        "run 模式却用 ExitCodeOnly 比较器，应当被拒"
    );

    let mut errorcheck_却期望成功 = base.clone();
    errorcheck_却期望成功.mode = Mode::ErrorCheck;
    errorcheck_却期望成功.comparator = Comparator::ErrorRegexPerDiag;
    errorcheck_却期望成功.expected.exit_code = 0;
    assert!(
        errorcheck_却期望成功.validate().is_err(),
        "errorcheck 模式期望退出码 0（官方 wantError：编译成功反而是失败）"
    );
}

#[test]
fn validate_要求_unsupported_必须带编号与原因() {
    // 「每个不通过的用例都要能说清落在哪一条」—— U 编号是 M0-tests §6 的冻结清单
    let mut c = 完整用例();
    c.mode = Mode::RunOutput; // 不在 v0 支持集
    c.comparator = Comparator::MergedStreamStrictEq;
    assert!(c.validate().is_err(), "非 v0 模式必须给 unsupported 说明");

    c.unsupported = Some(Unsupported {
        code: "U2".into(),
        reason: "runoutput 需两阶段（先跑生成器再编译运行）".into(),
    });
    assert!(c.validate().is_ok(), "带 U 编号与原因后应当通过");

    c.unsupported = Some(Unsupported {
        code: String::new(),
        reason: "有原因但没编号".into(),
    });
    assert!(c.validate().is_err(), "unsupported 必须带 U 编号");
}

#[test]
fn errorcheck_用例_能记多条诊断期望() {
    // T-C-20 mainsig.go：同行两条 ERROR，全文件 5 条（R4 的多引号/多行要点）
    let mut c = 完整用例();
    c.id = "T-C-20".into();
    c.rel_path = "mainsig.go".into();
    c.input_files = vec!["mainsig.go".into()];
    c.mode = Mode::ErrorCheck;
    c.comparator = Comparator::ErrorRegexPerDiag;
    c.expected = Expected {
        exit_code: 1,
        stdout: String::new(),
        has_out_file: false,
        diagnostics: vec![
            ExpectedDiag::at_line(9, r"func main must have no arguments and no return values"),
            ExpectedDiag::at_line(10, r"func main must have no arguments and no return values"),
            ExpectedDiag::at_line(10, r"main redeclared in this block"),
            ExpectedDiag::at_line(12, r"func init must have no arguments and no return values"),
            ExpectedDiag::at_line(13, r"func init must have no arguments and no return values"),
        ],
    };
    assert_eq!(
        c.expected.diagnostics.len(),
        5,
        "T-C-20 的 ERROR 期望是 5 条"
    );
    assert_eq!(
        c.expected
            .diagnostics
            .iter()
            .filter(|d| d.line == 10)
            .count(),
        2,
        "第 10 行有两条独立期望（同一行多 ERROR）"
    );
    assert!(c.validate().is_ok());
}

#[test]
fn mode_覆盖_v0_支持集与已冻结的不支持集() {
    // v0 支持集（M0-tests §6.1 的口径）：run / compile / errorcheck
    for m in [Mode::Run, Mode::Compile, Mode::ErrorCheck] {
        assert!(m.is_v0_supported(), "{} 应当属于 v0 支持集", m.name());
    }
    // U1–U12 里出现过的其余模式都【不是】v0 支持集
    for m in [
        Mode::RunDir,
        Mode::RunIndir,
        Mode::RunOutput,
        Mode::ErrorCheckDir,
        Mode::AsmCheck,
        Mode::Build,
        Mode::BuildDir,
        Mode::BuildRun,
        Mode::BuildRunDir,
        Mode::CompileDir,
        Mode::ErrorCheckOutput,
        Mode::ErrorCheckAndRunDir,
        Mode::ErrorCheckWithAuto,
        Mode::Skip,
    ] {
        assert!(!m.is_v0_supported(), "{} 不该属于 v0 支持集", m.name());
    }
}

// ── E5 的复验锚点（D-M0-15：double_sum 不删，改当 T-H-01 的进程内正例）────

#[test]
fn double_sum_仍在_且是_t_h_01_的进程内正例() {
    // D-M0-15：它不再是「遗留调试目标」，而是承担 T-H-01（成功类）的进程内正例。
    // **不能删** —— E5 是人工门禁，删掉锚点就无法复验（M0-plan §8 执行纪律）。
    assert_eq!(rgoc_harness::double_sum(1, 2), 6);
    // 且它的形状仍要满足 §8 的 DWARF 前提：中间值被第二次读取
    let src = include_str!("../src/lib.rs");
    assert!(
        src.contains("let sum = a + b;") && src.contains("sum * 2"),
        "double_sum 的形状被改了 —— 会破坏 E5 第 4 项（单步后读 sum），见 M0-benchmarks.md §8"
    );
}
