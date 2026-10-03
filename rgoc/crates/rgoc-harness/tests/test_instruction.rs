//! T32 的验收测试：指令行解析（R1）与分派顺序（R1b）。
//!
//! 规则来源全部标注了 `go_source_code/src/cmd/internal/testdir/testdir_test.go` 的行号，
//! 以及 `go1.27.1` 的 `go/build/constraint`（构建约束判定）。
//!
//! **本文件最重要的断言是 `分派顺序_平台过滤先于未知指令判定`** —— 它是 T29 冻结时
//! 从真实语料 `linkmain.go` 上查出来的坑：那个文件按 R1 解析出的 action 是一个
//! **非法指令**，但官方因为先做平台过滤而 `t.Skip`，从不 `Fatalf`。若 harness 顺序颠倒，
//! 就会在真实语料上误报 T-H-03（未知指令硬失败）。
//!
//! TDD 位置：**RED**。T32 实现前本文件编译不过（`rgoc_harness::instruction` 尚不存在）。
//!
//! **文件命名**：Rust 里 `tests/` 下每个**顶层** `.rs` 都自动成为集成测试 target，
//! 文件名不参与判断（没有 `test_` 前缀规则）—— target 名就是文件名词干，
//! 所以 `cargo test --test test_instruction`。本仓库统一加 `test_` 前缀 purely 为了可读性。

use rgoc_harness::instruction::{
    Dispatch, DispatchError, KNOWN_COMMANDS, ParseError, dispatch, is_go_build_line,
    is_plus_build_line, parse_action, split_quoted,
};
use rgoc_harness::ir::Mode;

// ══ R1：解析 action ═══════════════════════════════════════════════════════

#[test]
fn r1_指令在首行() {
    let i = parse_action("// run\npackage main\n").unwrap();
    assert_eq!(i.action, "run");
    assert!(i.args.is_empty(), "裸指令没有参数");
}

#[test]
fn r1_指令可以不在第一行() {
    // 推论（写在 M0-tests §1.3 R1 下）：**不能假定指令在第 1 行**。
    // 但要注意「跳过的只有**构建约束行**」—— 一条**普通注释**就是 action 的候选。
    // 真实语料 `linkmain.go` 正是如此：`//go:build ignore` 被跳过后，
    // 下一条版权注释成了 action（T29 实测记录的现象）。
    let i = parse_action("//go:build linux\n\n\n// run\npackage main\n").unwrap();
    assert_eq!(i.action, "run", "构建约束行与空行都应被跳过");

    // 版权注释在指令之前 ⇒ 它就是 action（官方会一路走到 switch 才 Fatalf）
    let i = parse_action("// Copyright 2015 The Go Authors.\n// run\n").unwrap();
    assert_eq!(
        i.action, "Copyright",
        "普通注释不是构建约束，会被当成 action —— 这正是 linkmain.go 的成因"
    );
    assert!(matches!(
        dispatch(&i, true),
        Err(DispatchError::UnknownAction(_))
    ));

    // 注意 fixture 不能以换行开头：那是 LeadingNewline（:497），另一条测试覆盖。
}

#[test]
fn r1_跳过_go_build_约束行() {
    // constraint.IsGoBuild：`//go:build` 前缀且其后须有空白（或整行只有它）
    let i = parse_action("//go:build linux\n// run\n").unwrap();
    assert_eq!(i.action, "run", "//go:build 行是构建约束，不当作 action");
    // 整行只有 `//go:build`（后面没东西）也算约束
    let i = parse_action("//go:build\n// run\n").unwrap();
    assert_eq!(i.action, "run");
}

#[test]
fn r1_跳过加号_build_约束行() {
    // constraint.IsPlusBuild：`//` + 可选空格 + `+build`；`//+build` 也认
    let i = parse_action("// +build linux\n// run\n").unwrap();
    assert_eq!(i.action, "run");
    let i = parse_action("//+build linux\n// run\n").unwrap();
    assert_eq!(i.action, "run", "加号与 // 之间允许没有空格");
}

#[test]
fn r1_空的注释行继续往下扫() {
    // 官方循环条件是 `action == ""`，所以一个光秃秃的 `//` 行解析出空串后
    // **继续扫描**，而不是停下报错。这条容易被实现成「取到空串就返回」。
    let i = parse_action("//\n// run\n").unwrap();
    assert_eq!(i.action, "run");
}

#[test]
fn r1_构建约束判定_照_go_build_constraint_的边界() {
    // `//go:buildsomethingelse` **不是**构建约束：前缀之后没有空白。
    // （constraint/expr.go 的 splitGoBuild：TrimSpace 后长度不变且非空 → 返回 false）
    assert!(is_go_build_line("//go:build linux"));
    assert!(is_go_build_line("//go:build"));
    assert!(!is_go_build_line("//go:buildsomethingelse"));
    assert!(
        !is_go_build_line("// go:build linux"),
        "前缀后必须有紧贴的 //go:build"
    );
    assert!(!is_go_build_line("// run"), "普通注释不是构建约束");
    // 行首缩进的 //go:build 也不认（HasPrefix 判的是整行开头）
    assert!(!is_go_build_line("  //go:build linux"));

    assert!(is_plus_build_line("// +build linux"));
    assert!(is_plus_build_line("//+build linux"));
    assert!(!is_plus_build_line("// +buildsomethingelse"));
    assert!(!is_plus_build_line("//build linux"));
    assert!(!is_plus_build_line("// run"));
}

#[test]
fn r1_首行不是注释时_只取第一段() {
    // 官方不在这儿报错：`action = TrimSpace(TrimPrefix(line, "//"))` 对
    // `package main` 这类行照样成立，错误要到 switch 阶段才暴露。
    // 而 `splitQuoted` 会把它切成 ["package", "main"]，官方只取 f[0] 当 action ——
    // 所以 Fatalf 打印的是 `unknown pattern: "package"`（**不含 main**）。
    let i = parse_action("package main\n").unwrap();
    assert_eq!(i.action, "package");
    assert_eq!(i.args, vec!["main"]);
    assert!(matches!(
        dispatch(&i, true),
        Err(DispatchError::UnknownAction(a)) if a == "package"
    ));
}

#[test]
fn r1_文件以换行开头是硬失败() {
    // testdir_test.go:497 —— 官方 t.Fatal，不降级
    assert_eq!(parse_action("\n// run\n"), Err(ParseError::LeadingNewline));
    assert_eq!(parse_action("\n"), Err(ParseError::LeadingNewline));
}

#[test]
fn r1_没有_instruction_是硬失败() {
    // testdir_test.go:514 —— execution recipe not found。
    // ⚠️ 触发条件是「扫完所有行，action 一直是**空串**」，**不是**「没找到合法指令」。
    assert_eq!(
        parse_action("//\n//\n"),
        Err(ParseError::NoRecipe),
        "全是空注释行"
    );
    assert_eq!(parse_action(""), Err(ParseError::NoRecipe), "空文件");
    assert_eq!(
        parse_action("//\n\n//   \n"),
        Err(ParseError::NoRecipe),
        "空行与空白注释"
    );

    // 反过来：一条**非空**的普通注释就是合法的 action —— 哪怕它不是 16 个指令之一。
    // 官方会一路走到 switch 才 Fatalf，所以这里返回 Ok，然后在 dispatch 被拒。
    let i = parse_action("// 只有注释\n// 还是注释\n").unwrap();
    assert_eq!(i.action, "只有注释", "首个非空非约束行就是 action");
    assert!(matches!(
        dispatch(&i, true),
        Err(DispatchError::UnknownAction(_))
    ));
}

#[test]
fn r1_参数按_引号感知_分词() {
    // testdir_test.go:2009 splitQuoted：引号内的空格不分词；单双引号都认
    let i = parse_action("// errorcheck -d=ssa/check/on\n").unwrap();
    assert_eq!(i.action, "errorcheck");
    assert_eq!(i.args, vec!["-d=ssa/check/on"]);

    let i = parse_action("// runoutput \"foo bar\"\n").unwrap();
    assert_eq!(i.action, "runoutput");
    assert_eq!(i.args, vec!["foo bar"], "引号内的空格不该被当成参数分隔");

    let i = parse_action("// run a 'b c' d\n").unwrap();
    assert_eq!(i.args, vec!["a", "b c", "d"]);

    assert_eq!(
        split_quoted("a  b").unwrap(),
        vec!["a", "b"],
        "连续空白不产生空参数"
    );
    assert_eq!(
        split_quoted("").unwrap(),
        Vec::<String>::new(),
        "空串产出零个参数"
    );
}

#[test]
fn r1_未闭合引号是硬失败() {
    // splitQuoted 返回 unclosed quote → 官方 t.Fatal("invalid test recipe")
    assert_eq!(
        parse_action("// runoutput \"foo\n"),
        Err(ParseError::UnclosedQuote)
    );
    assert_eq!(
        parse_action("// runoutput 'foo\n"),
        Err(ParseError::UnclosedQuote)
    );
}

// ══ R1b：分派顺序 ═════════════════════════════════════════════════════════

#[test]
fn r1b_平台过滤先于未知指令判定() {
    // 真实语料 go_source_code/test/linkmain.go 的头几行（原样照抄）
    let src = "//go:build ignore\n\n// Copyright 2015 The Go Authors. All rights reserved.\n// Use of this source code is governed by a BSD-style\n// license that can be found in the LICENSE file.\n\npackage main\n";
    let ins = parse_action(src).unwrap();

    // 按 R1 跳过 `//go:build ignore` 后，下一条注释被当成 action —— 一个**非法指令**
    assert!(
        ins.action.starts_with("Copyright"),
        "R1 的解析结果应是被截断的版权行，实际：{:?}",
        ins.action
    );
    assert!(!KNOWN_COMMANDS.contains(&ins.action.as_str()));

    // 官方顺序（:517-524 先于 :541-558）：平台过滤通过与否，决定它走哪条路
    assert_eq!(
        dispatch(&ins, true),
        Err(DispatchError::UnknownAction(ins.action.clone())),
        "平台过滤放行 + 未知指令 = 硬失败（t.Fatalf）"
    );
    assert_eq!(
        dispatch(&ins, false),
        Ok(Dispatch::TargetFiltered),
        "平台过滤拦下时判 target-filtered，**不得**报未知指令 —— 这就是 T29 查出的坑"
    );
}

#[test]
fn r1b_十六个指令全部被识别() {
    // M0-tests §1.2 的完整清单（源码 :542 / :550 / :544 / :546 / :552）
    let expect: [(&str, Mode); 16] = [
        ("compile", Mode::Compile),
        ("compiledir", Mode::CompileDir),
        ("build", Mode::Build),
        ("builddir", Mode::BuildDir),
        ("buildrundir", Mode::BuildRunDir),
        ("run", Mode::Run),
        ("buildrun", Mode::BuildRun),
        ("runoutput", Mode::RunOutput),
        ("rundir", Mode::RunDir),
        ("runindir", Mode::RunIndir),
        ("asmcheck", Mode::AsmCheck),
        ("errorcheck", Mode::ErrorCheck),
        ("errorcheckdir", Mode::ErrorCheckDir),
        ("errorcheckoutput", Mode::ErrorCheckOutput),
        ("errorcheckandrundir", Mode::ErrorCheckAndRunDir),
        ("errorcheckwithauto", Mode::ErrorCheckWithAuto),
    ];
    assert_eq!(
        KNOWN_COMMANDS.len(),
        expect.len(),
        "KNOWN_COMMANDS 必须正好 16 个"
    );
    for (name, mode) in expect {
        assert!(
            KNOWN_COMMANDS.contains(&name),
            "KNOWN_COMMANDS 漏了 {name}（M0-tests §1.2 的 16 个）"
        );
        let ins = parse_action(&format!("// {name}\n")).unwrap();
        assert_eq!(
            dispatch(&ins, true),
            Ok(Dispatch::Proceed(mode)),
            "{name} 应当映射到 {mode:?}"
        );
    }
    // `skip` 单独处理：它不是模式，而是「上游设计即跳过」（官方 :552 直接 t.Skip）
    assert!(
        !KNOWN_COMMANDS.contains(&"skip"),
        "skip 不在 16 个模式里，另行处理"
    );
    let ins = parse_action("// skip\n").unwrap();
    assert_eq!(
        dispatch(&ins, true),
        Ok(Dispatch::SkippedByDesign),
        "// skip 不是模式，是「上游设计即跳过」（官方 :552 直接 t.Skip）"
    );
}

#[test]
fn r1b_平台过滤对_合法_指令同样适用() {
    // 平台过滤在 switch 之前，所以它对**合法**指令也一样会拦下。
    // 语料里有 5 个这样的文件（T29 实测：inline_math_bits_rotate / nilptr_aix /
    // simd_inline / wasmexport / wasmexport2），它们仍在分母里、但判 target-filtered。
    let ins = parse_action("// run\n").unwrap();
    assert_eq!(dispatch(&ins, false), Ok(Dispatch::TargetFiltered));
    let ins = parse_action("// compile\n").unwrap();
    assert_eq!(dispatch(&ins, false), Ok(Dispatch::TargetFiltered));
}
