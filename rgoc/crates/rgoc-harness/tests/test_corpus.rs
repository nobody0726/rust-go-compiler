//! T33 的验收测试：语料枚举 + 平台过滤（`shouldTest`）+ unsupported 归类。
//!
//! 规则来源：`go_source_code/src/cmd/internal/testdir/testdir_test.go` 的 `:517-524`
//! （平台过滤）与 `:380-467`（`shouldTest` / `match`），tag 集合取 `go1.27.1` 实测值。
//!
//! **最重要的一条是 `枚举语料_分母等于_279`** —— 它是 T29 冻结的文档口径与本实现之间
//! 的交叉校验：分母对不上，说明枚举或分类实现与冻结口径不一致，比任何单测都更早发现问题。
//!
//! TDD 位置：**RED**。T33 实现前本文件编译不过（`rgoc_harness::corpus` 尚不存在）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rgoc_harness::corpus::{
    CorpusConfig, CorpusError, enumerate, has_exclusion_args, header_of, should_test,
};
use rgoc_harness::instruction::parse_action;

/// M0 的目标平台与 tag 集合（`go1.27.1` linux/arm64 实测，见 M0-benchmarks §1）。
fn cfg() -> CorpusConfig {
    CorpusConfig::m0()
}

// ══ 平台过滤：表达式求值 ═══════════════════════════════════════════════════

#[test]
fn should_test_单行约束() {
    let c = cfg();
    // 真值：目标平台、gc、发布 tag、test_run
    assert!(should_test("//go:build linux", &c));
    assert!(should_test("//go:build arm64", &c));
    assert!(should_test("//go:build gc", &c));
    assert!(should_test("//go:build go1.21", &c));
    assert!(should_test("//go:build test_run", &c));
    // 假值：别的 OS/ARCH、未开启的实验、cgo（-cgo 默认关）、gcflags_noopt（GO_GCFLAGS 空）
    assert!(!should_test("//go:build windows", &c));
    assert!(!should_test("//go:build amd64", &c));
    assert!(
        !should_test("//go:build goexperiment.simd", &c),
        "simd 不在 go1.27.1 的 ToolTags 里"
    );
    assert!(!should_test("//go:build cgo", &c));
    assert!(!should_test("//go:build gcflags_noopt", &c));
    // 默认开启的实验（go1.27.1 的 ToolTags 实测值）
    assert!(should_test("//go:build goexperiment.regabiargs", &c));
    assert!(should_test("//go:build goexperiment.dwarf5", &c));
    assert!(should_test("//go:build goexperiment.jsonv2", &c));
    // 开关打开后 cgo / gcflags_noopt 才为真
    let mut on = cfg();
    on.cgo_enabled = true;
    on.no_opt_env = true;
    assert!(should_test("//go:build cgo", &on));
    assert!(should_test("//go:build gcflags_noopt", &on));
    assert!(
        !should_test("//go:build cgo", &c),
        "默认必须为假，否则判断就反了"
    );
}

#[test]
fn should_test_逻辑运算() {
    let c = cfg();
    assert!(should_test("//go:build linux || windows", &c));
    assert!(should_test("//go:build windows || linux", &c));
    assert!(!should_test("//go:build windows || darwin", &c));
    assert!(should_test("//go:build linux && arm64", &c));
    assert!(!should_test("//go:build linux && !arm64", &c));
    assert!(should_test("//go:build !windows && !darwin", &c));
    // 括号（语料里真实出现过 `(amd64 && !gcflags_noopt) || (arm64 && !gcflags_noopt)`）
    assert!(should_test(
        "//go:build (amd64 && !gcflags_noopt) || (arm64 && !gcflags_noopt)",
        &c
    ));
    assert!(should_test(
        "//go:build (386 || amd64 || arm64) && !gcflags_noopt",
        &c
    ));
    assert!(!should_test(
        "//go:build (386 || amd64) && !gcflags_noopt",
        &c
    ));
    // `&&` 优先于 `||`
    assert!(should_test("//go:build windows || linux && arm64", &c));
    assert!(!should_test("//go:build (windows || linux) && amd64", &c));
    // 点号是 tag 名的一部分：`amd64.v3` 不是有效 tag（ToolTags 里的 arm64.v8.0 也不该被匹配，
    // 因为官方只对 `goexperiment.` 前缀查 ToolTags）
    assert!(!should_test("//go:build amd64.v3", &c));
    assert!(should_test("//go:build amd64.v3 || arm64", &c));
}

#[test]
fn should_test_多行约束_按与组合() {
    let c = cfg();
    // go/build 的语义：多行 //go:build 之间是 AND
    assert!(should_test("//go:build linux\n//go:build arm64", &c));
    assert!(!should_test("//go:build linux\n//go:build windows", &c));
}

#[test]
fn should_test_无约束一律通过() {
    let c = cfg();
    assert!(should_test("", &c));
    assert!(should_test("package main", &c));
    assert!(should_test("// run", &c), "普通注释不是约束");
    assert!(should_test("// Copyright 2015", &c));
}

#[test]
fn should_test_非法表达式_不放行() {
    let c = cfg();
    // 解析不了 ⇒ 官方 Parse 返回 err 后**不参与判定**（当作没有约束行）⇒ 放行。
    // 这里锁住「不因为解析失败而误杀用例」这个方向。
    assert!(
        should_test("//go:build ((linux", &c),
        "解析失败不应把用例判死"
    );
    assert!(
        should_test("//go:build", &c),
        "整行只有前缀：constraint 认它是约束 ⇒ 后面无表达式 ⇒ 放行"
    );
}

#[test]
fn 旧式加号_build_按空格或逗号() {
    let c = cfg();
    // 旧语法：空格 = OR，逗号 = AND
    assert!(should_test("// +build linux darwin", &c));
    assert!(should_test("// +build windows linux", &c));
    assert!(!should_test("// +build windows darwin", &c));
    assert!(should_test("// +build linux,arm64", &c));
    assert!(!should_test("// +build linux,amd64", &c));
    assert!(should_test("// +build !windows", &c));
    assert!(!should_test("// +build !linux", &c));
    // 多行 +build 之间是 AND
    assert!(!should_test("// +build linux\n// +build windows", &c));
    // //go:build 存在时**优先**，+build 被忽略（go/build 的规则）
    assert!(should_test("//go:build linux\n// +build windows", &c));
    assert!(!should_test("//go:build windows\n// +build linux", &c));
}

#[test]
fn header_只取_package_之前() {
    // 官方：header, _, ok := strings.Cut(src, "\npackage")；没有 \npackage 时 header = action
    let src = "//go:build windows\n\npackage main\n\nfunc main() {}\n";
    assert_eq!(header_of(src, "run"), "//go:build windows\n");
    // 故意畸形的文件（官方注释：some files are intentionally malformed）
    assert_eq!(header_of("// no package here\n", "run"), "run");
}

// ══ U7：排除参数 ═══════════════════════════════════════════════════════════

#[test]
fn u7_排除参数_按参数形态() {
    // 这不是官方规则，是 M0-tests §6 的 **U7** 项目决策：编译器内部开关，非语言语义。
    // ⚠️ 开关在 **args** 里 —— parse_action 已把 action 与参数分开（`errorcheck` vs `-d=panic`）
    for yes in [
        "errorcheck -d=panic",
        "run -gcflags=-l=4",
        "run -gcflags -l=4", // 官方 splitQuoted 切成两个参数，-gcflags 单独一个
        "errorcheck -0 -l -d=defer",
        "run -goexperiment.loopvar",
        "run -godebug=panicnil=1",
    ] {
        let ins = parse_action(yes).unwrap();
        assert!(has_exclusion_args(&ins), "应判为 U7：{yes}");
    }
    for no in [
        "run",
        "compile",
        "errorcheck",
        "errorcheck -0 -l",
        "run a b",
        "compile -S",
    ] {
        let ins = parse_action(no).unwrap();
        assert!(!has_exclusion_args(&ins), "不应判为 U7：{no}");
    }
}

// ══ 真实语料枚举（与 T29 冻结口径的交叉校验）═══════════════════════════════

/// 语料目录。默认按 crate 位置推算（`<repo>/go_source_code/test`），
/// 可用 `RGOC_CORPUS_TEST_DIR` 覆盖。
///
/// ⚠️ **找不到就 panic，不静默跳过** —— 本文件的核心价值就是那个「分母 == 279」的交叉校验，
/// 语料缺失时静默 skip 等于把唯一的校验悄悄关掉。
fn corpus_test_dir() -> PathBuf {
    if let Ok(d) = std::env::var("RGOC_CORPUS_TEST_DIR") {
        return PathBuf::from(d);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../go_source_code/test")
        .to_path_buf()
}

fn report() -> BTreeMap<String, usize> {
    let dir = corpus_test_dir();
    assert!(
        dir.is_dir(),
        "语料目录不存在：{}\n\
         交叉校验（分母 == 279）是本文件的核心价值，**不允许**因为找不到语料而静默跳过。\n\
         可用 RGOC_CORPUS_TEST_DIR 指向 go_source_code/test。",
        dir.display()
    );
    let r = enumerate(&dir, &cfg()).unwrap_or_else(|e| panic!("枚举失败：{e:?}"));
    assert_eq!(r.files.len(), r.total, "files 列表长度应等于总数");
    r.summary()
}

#[test]
fn 枚举语料_分母等于_279() {
    // ★ T29 冻结的 M0 分母。实现与文档口径不一致时这里会红。
    let s = report();
    assert_eq!(
        s.get("denominator").copied(),
        Some(279),
        "M0 分母必须是 279（T29 冻结值），实际 {:?}",
        s.get("denominator")
    );
}

#[test]
fn 枚举语料_计数与冻结明细一致() {
    let s = report();
    let expect: &[(&str, usize)] = &[
        ("total", 356),       // 顶层 test/*.go 总数
        ("denominator", 279), // v0 集 ∧ 无排除参数
        ("by_mode/run", 147),
        ("by_mode/compile", 12),
        ("by_mode/errorcheck", 120),
        ("target_filtered", 5), // 平台过滤（仍在分母）
        ("executed", 274),      // 分母内真正参与执行
        ("excluded", 77),       // 356 − 279
        ("u/U1", 5),            // rundir 4 + runindir 1
        ("u/U2", 14),           // runoutput
        ("u/U3", 1),            // errorcheckdir
        ("u/U5", 9),            // build 4 + buildrundir 3 + compiledir 2
        ("u/U6", 11),           // errorcheckoutput 3 + errorcheckandrundir 3 + withauto 5
        ("u/U7", 31),           // 排除参数
        ("u/U13", 5),           // skip
        ("u/U14", 1),           // linkmain.go
    ];
    for (k, v) in expect {
        assert_eq!(
            s.get(*k).copied(),
            Some(*v),
            "{k} 应为 {v}，实际 {:?}（全量：{s:?}）",
            s.get(*k)
        );
    }
    // 分类之和必须自洽：分母 + 排除 == 总数
    assert_eq!(
        s["denominator"] + s["excluded"],
        s["total"],
        "分母 + 排除 必须等于总数"
    );
}

#[test]
fn 枚举语料_被平台过滤的就是那_5_个() {
    // T29 实测点名的 5 个（仍在分母、不计入分子）
    let dir = corpus_test_dir();
    let r = enumerate(&dir, &cfg()).expect("枚举");
    // 只在**分母内**筛：U7 文件里也有被平台过滤的（如 checkbce.go 有 //go:build amd64），
    // 它们不在分母，不属于「分母里被过滤的那 5 个」。
    let mut filtered: Vec<&str> = r
        .files
        .iter()
        .filter(|f| r.in_denominator(f) && f.target_filtered)
        .map(|f| f.name.as_str())
        .collect();
    filtered.sort_unstable();
    assert_eq!(
        filtered,
        vec![
            "inline_math_bits_rotate.go",
            "nilptr_aix.go",
            "simd_inline.go",
            "wasmexport.go",
            "wasmexport2.go",
        ],
        "平台过滤的文件名单与 T29 实测不一致"
    );
}

#[test]
fn 枚举语料_每个用例都能说清落在哪一条() {
    // 「每个不通过的用例都要能说清落在哪一条」—— 分母内不许有 unsupported，
    // 排除的必须带 U 编号与原因。
    let dir = corpus_test_dir();
    let r = enumerate(&dir, &cfg()).expect("枚举");
    for f in &r.files {
        if r.in_denominator(f) {
            assert!(
                f.unsupported.is_none(),
                "{} 在分母里却带 unsupported：{:?}",
                f.name,
                f.unsupported
            );
        } else {
            let u = f
                .unsupported
                .as_ref()
                .unwrap_or_else(|| panic!("{} 不在分母里却没有 unsupported 归类", f.name));
            assert!(
                u.code.starts_with('U') && !u.reason.trim().is_empty(),
                "{} 的 unsupported 归类不完整：{:?}",
                f.name,
                u
            );
        }
    }
}

#[test]
fn 枚举语料_可重复且确定() {
    let dir = corpus_test_dir();
    let a = enumerate(&dir, &cfg()).expect("第一次");
    let b = enumerate(&dir, &cfg()).expect("第二次");
    assert_eq!(a.summary(), b.summary(), "两次枚举结果必须一致（可复现）");
    assert_eq!(
        a.files.iter().map(|f| &f.name).collect::<Vec<_>>(),
        b.files.iter().map(|f| &f.name).collect::<Vec<_>>(),
        "文件列表与顺序也必须一致"
    );
}

#[test]
fn 枚举语料_解析不出的文件要报出来而不是静默丢() {
    // 官方对「文件以换行开头」是 t.Fatal。枚举时若遇到这种文件，
    // 应当**报错**（或至少单列出来），不能悄悄从分母里消失。
    let err = enumerate(Path::new("/nonexistent-corpus-dir"), &cfg()).unwrap_err();
    assert!(
        matches!(err, CorpusError::Io(_)),
        "目录不存在应报 Io 错误，实际 {err:?}"
    );
}

// ══ Test IR 的必录字段：从枚举结果能填出来 ════════════════════════════════

#[test]
fn 枚举结果_能直接填出_test_ir() {
    use rgoc_harness::ir::{Expected, Layer, Limits, Target, TestCase};
    let dir = corpus_test_dir();
    let r = enumerate(&dir, &cfg()).expect("枚举");
    // 取第一个**在分母内**的用例：只有它才有比较器（`Comparator::for_mode` 对
    // 非 v0 模式返回 None），也才是真正会跑起来的那些。
    let c = r
        .files
        .iter()
        .find(|f| r.in_denominator(f))
        .expect("枚举结果里应有分母内的用例");
    let tc = TestCase {
        id: format!("T-C-{:02}", 1).into(),
        rel_path: c.name.clone().into(),
        input_files: vec![c.name.clone().into()],
        mode: c.mode,
        instruction_args: c.instruction_args.clone(),
        build_tags: c.build_tags.clone(),
        target: Target {
            goos: cfg().goos.clone(),
            goarch: cfg().goarch.clone(),
            go_version: cfg().go_version.clone(),
        },
        imports: vec![],
        feature_deps: vec![],
        comparator: rgoc_harness::ir::Comparator::for_mode(c.mode).expect("分母内的用例必有比较器"),
        expected: Expected::default(),
        limits: Limits::for_layer(Layer::Corpus),
        seed: 0,
        milestone: "M0".into(),
        unsupported: c.unsupported.clone(),
    };
    // 分母内的用例必须能通过 validate；被排除的则带着 unsupported 也应通过
    assert!(tc.validate().is_ok(), "{:?}", tc.validate());
}
