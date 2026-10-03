//! T34 的验收测试：oracle 调用与版本守门。
//!
//! 规则来源：`testdir_test.go` 的 `:642-662`（`runcmd` 合并 stdout+stderr、固定环境变量、
//! cwd 默认 GOROOT/test）、`:218-220`（stdlibImportcfg）、`:188`（`compileFile`）、
//! `:787-790`（`errorcheck` 的命令形态）、`:613-625`（R5 自动加 `-d=ssa/check/on`）。
//! 版本守门见 `M0-tests.md` T-H-06 与 `04` §7。
//!
//! **本文件会真的调用容器内的 `go1.27.1`**（不是 mock）—— 版本守门与超时回收
//! 这两件事只有对真实进程做才有意义。
//!
//! TDD 位置：**RED**。T34 实现前本文件编译不过（`rgoc_harness::oracle` 尚不存在）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use rgoc_harness::corpus::CorpusConfig;
use rgoc_harness::ir::Limits;
use rgoc_harness::oracle::{Oracle, OracleConfig, OracleError, OracleOutput};

// ══ 工具 ═══════════════════════════════════════════════════════════════════

/// 建一个临时目录并写入给定的 Go 源码，返回 (dir, 主文件名)。
fn fixture(name: &str, src: &str) -> (PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("m0-t34-{}-{}", name, std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let file = format!("{name}.go");
    std::fs::write(dir.join(&file), src).expect("写 fixture");
    (dir, file)
}

fn cfg_for(dir: &Path) -> OracleConfig {
    OracleConfig {
        // 官方 runcmd 的 cwd：默认 GOROOT/test（`:654`）。测试用临时目录等价。
        work_dir: dir.to_path_buf(),
        // T-C 层预算：单项 60 s（这里多数用例几毫秒就跑完）
        limits: Limits::for_layer(rgoc_harness::ir::Layer::Corpus),
        ..OracleConfig::from(CorpusConfig::m0())
    }
}

/// 一个只写 stderr 的程序（内建 `println` 写 stderr，R2b 的关键样本）。
const 打印到标准错误的程序: &str = r#"// run

package main

func main() { println("hi") }
"#;

/// 一个死等（不烧 CPU）的程序，用来触发超时。
const 死等程序: &str = r#"// run

package main

import "time"

func main() { time.Sleep(30 * time.Second) }
"#;

/// 一个分配大内存的程序，用来触发 RSS 上限。
///
/// ⚠️ 必须**读回**一个值：只写不读的 `b[i] = 1` 会被 Go 当成 dead store 优化掉，
/// 页面从未被触碰，RSS 也就上不去（本轮实测只观测到 13 MiB）。
const 大内存程序: &str = r#"// run

package main

import "time"

func main() {
	b := make([]byte, 256<<20)
	for i := 0; i < len(b); i += 4096 {
		b[i] = 1
	}
	// 读回：否则写入是 dead store，页面不被触碰，RSS 上不去
	println(int(b[12345]), len(b))
	// 停留一小会儿：让轮询采得到峰值。真实负载都在百毫秒以上，
	// 只有这种极短命进程才可能错过采样（oracle.rs 的 read_peak_rss 注释里记了这个限制）
	time.Sleep(300 * time.Millisecond)
}
"#;

// ══ 版本守门（T-H-06）═══════════════════════════════════════════════════════

#[test]
fn 版本_匹配时_正常建oracle() {
    let (dir, _f) = fixture("版本ok", 打印到标准错误的程序);
    let o = Oracle::new(cfg_for(&dir)).expect("容器内的 go1.27.1 应当通过版本守门");
    assert_eq!(o.version(), "go1.27.1", "应当解析出确切的版本号");
}

#[test]
fn 版本_不符时_拒绝建oracle() {
    // T-H-06 / `04` §7：版本不符**即拒绝作基线**，不得降级跑别的版本
    let (dir, _f) = fixture("版本不符", 打印到标准错误的程序);
    let mut c = cfg_for(&dir);
    c.expect_version = "go1.24.5".into(); // 宿主那个版本
    let err = Oracle::new(c).unwrap_err();
    match err {
        OracleError::VersionMismatch { expected, got } => {
            // expected 记的是完整的「版本 GOOS/GOARCH」
            assert!(
                expected.starts_with("go1.24.5"),
                "expected 应含期望版本，实际：{expected:?}"
            );
            assert!(
                got.contains("go1.27.1"),
                "报错里应带上实际版本，实际：{got:?}"
            );
        }
        other => panic!("应当是 VersionMismatch，实际 {other:?}"),
    }
    // 关键：**没有**返回任何「可降级」的 oracle —— 拒绝就是拒绝
    assert!(
        Oracle::new({
            let mut c = cfg_for(&dir);
            c.expect_version = "go1.24.5".into();
            c
        })
        .is_err(),
        "版本不符时不得给出可用实例"
    );
}

#[test]
fn 版本_goos_goarch_也要对上() {
    // oracle 的 `go version` 输出带 GOOS/GOARCH；目标平台不匹配同样不能作基线
    let (dir, _f) = fixture("平台不符", 打印到标准错误的程序);
    let mut c = cfg_for(&dir);
    c.expect_goos = "windows".into();
    assert!(
        matches!(Oracle::new(c), Err(OracleError::VersionMismatch { .. })),
        "GOOS 不符也应当拒绝"
    );
}

// ══ 正常调用（R6 的命令形态）═══════════════════════════════════════════════

#[test]
fn run_层_拿到_合并流与退出码() {
    let (dir, f) = fixture("runok", 打印到标准错误的程序);
    let o = Oracle::new(cfg_for(&dir)).expect("oracle");
    let out: OracleOutput = o
        .run_mode(rgoc_harness::ir::Mode::Run, &f)
        .expect("run 应成功");
    // 内建 println 写 **stderr**；R2b 要求合并流，所以这里必须能看到 hi
    assert!(
        out.merged.contains("hi"),
        "合并流里应含 stderr 的输出，实际：{:?}",
        out.merged
    );
    assert_eq!(out.exit_code, Some(0));
    assert!(!out.timed_out && !out.resource_exceeded);
}

#[test]
fn run_层_固定环境变量_照官方() {
    // :647 / :656：GOENV=off、GOFLAGS=（清空）、PWD=<dir>
    let src = r#"// run

package main

import (
	"fmt"
	"os"
)

func main() {
	for _, k := range []string{"GOENV", "GOFLAGS", "PWD"} {
		v, ok := os.LookupEnv(k)
		fmt.Printf("%s=%v/%q\n", k, ok, v)
	}
}
"#;
    let (dir, f) = fixture("env", src);
    let o = Oracle::new(cfg_for(&dir)).expect("oracle");
    let out = o.run_mode(rgoc_harness::ir::Mode::Run, &f).expect("run");
    assert!(
        out.merged.contains("GOENV=true/\"off\""),
        "GOENV 应被设为 off：{:?}",
        out.merged
    );
    assert!(
        out.merged.contains("GOFLAGS=true/\"\""),
        "GOFLAGS 应被设为空串（清空）：{:?}",
        out.merged
    );
    assert!(
        out.merged.contains("PWD=true/"),
        "PWD 应指向工作目录：{:?}",
        out.merged
    );
}

#[test]
fn compile_层_成功时无诊断() {
    let (dir, f) = fixture("compileok", 打印到标准错误的程序);
    let o = Oracle::new(cfg_for(&dir)).expect("oracle");
    let out = o
        .run_mode(rgoc_harness::ir::Mode::Compile, &f)
        .expect("compile");
    assert_eq!(out.exit_code, Some(0));
    assert!(
        out.merged.is_empty(),
        "编译成功不该有输出：{:?}",
        out.merged
    );
}

#[test]
fn errorcheck_层_拿到诊断_且命令带_ssa_check() {
    // R5：errorcheck 即使裸写也会被自动加 `-d=ssa/check/on`（:613-625）
    let bad = r#"// errorcheck

package main

func main() {
	undefinedFunction()
}
"#;
    let (dir, f) = fixture("errorcheck", bad);
    let o = Oracle::new(cfg_for(&dir)).expect("oracle");
    let out = o
        .run_mode(rgoc_harness::ir::Mode::ErrorCheck, &f)
        .expect("errorcheck 应跑通（它本来就该失败）");
    assert_ne!(
        out.exit_code,
        Some(0),
        "errorcheck 样本必须编译失败，否则期望落空"
    );
    assert!(
        out.merged.contains("undefinedFunction") || out.merged.contains("undefined"),
        "诊断里应能看到未定义的函数：{:?}",
        out.merged
    );
    // 命令形态必须是 go tool compile（官方 :787-790），不是 go build
    let steps = o.steps(rgoc_harness::ir::Mode::ErrorCheck, &f);
    assert_eq!(steps.len(), 1, "errorcheck 层只要一步：{steps:?}");
    let argv = &steps[0].args;
    assert_eq!(
        argv.first().map(String::as_str),
        Some("tool"),
        "argv[1] 应是 tool：{argv:?}"
    );
    assert_eq!(
        argv.get(1).map(String::as_str),
        Some("compile"),
        "argv[2] 应是 compile：{argv:?}"
    );
    assert!(
        argv.iter().any(|a| a == "-C"),
        "errorcheck 必须带 -C（关掉首行列号）：{argv:?}"
    );
    assert!(
        argv.iter().any(|a| a == "-d=ssa/check/on"),
        "R5：必须带 -d=ssa/check/on：{argv:?}"
    );
    assert!(
        argv.iter().any(|a| a.starts_with("-importcfg=")),
        "必须带 stdlib 的 importcfg：{argv:?}"
    );
}

// ══ 超时（T-H-05：子进程必须被真正回收）════════════════════════════════════

#[test]
fn 超时_判_timeout_且子进程被回收() {
    let (dir, f) = fixture("超时", 死等程序);
    let mut c = cfg_for(&dir);
    c.limits = Limits::for_layer(rgoc_harness::ir::Layer::Corpus)
        .with_per_case_override(Duration::from_millis(600));
    let o = Oracle::new(c).expect("oracle");
    let out = o
        .run_mode(rgoc_harness::ir::Mode::Run, &f)
        .expect("超时也应返回结构化结果，不是 Err");
    assert!(out.timed_out, "应判超时，实际 exit={:?}", out.exit_code);
    assert!(
        out.duration < Duration::from_secs(5),
        "应在超时上限附近返回，实际耗时 {:?}",
        out.duration
    );
    // 关键：**子进程真的被回收了** —— 官方 runcmd 会 kill 整个进程组（:666-675 的思路），
    // 只 kill 父进程的话，30 秒的 sleep 会变成孤儿进程继续占资源。
    if let Some(pid) = out.pid {
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "超时后子进程 {pid} 仍在 —— 说明只杀了父进程、没杀进程组"
        );
    }
}

#[test]
fn 超时_上限来自_limits_而不是硬编码() {
    // §7.5 的预算只定义一次（ir.rs 的 Limits::for_layer），oracle 只准读它
    let (dir, f) = fixture("超时预算", 死等程序);
    let mut c = cfg_for(&dir);
    c.limits = Limits::for_layer(rgoc_harness::ir::Layer::Corpus)
        .with_per_case_override(Duration::from_millis(400));
    let o = Oracle::new(c).expect("oracle");
    // ⚠️ 只量 run_mode：`Oracle::new` 里的 `go list -export std` 可能很慢
    // （高并发下实测能到 45 s，那是 std 导出缓存的问题，与本用例的超时预算无关）
    let start = std::time::Instant::now();
    let out = o.run_mode(rgoc_harness::ir::Mode::Run, &f).expect("超时");
    let spent = start.elapsed();
    assert!(out.timed_out, "应判超时");
    assert!(
        spent < Duration::from_secs(10),
        "400 ms 的上限却跑了 {spent:?} —— 说明超时不是从 limits 取的"
    );
}

// ══ RSS 上限（§7.5：≤ 512 MiB，T-C-03 放宽到 768 MiB）══════════════════════

#[test]
fn rss_超上限时判_resource_failure() {
    let (dir, f) = fixture("大内存", 大内存程序);
    let mut c = cfg_for(&dir);
    // 把上限压到 64 MiB —— 样本会分配 256 MiB，必然超
    c.limits =
        Limits::for_layer(rgoc_harness::ir::Layer::Corpus).with_rss_override(64 * 1024 * 1024);
    let o = Oracle::new(c).expect("oracle");
    let out = o
        .run_mode(rgoc_harness::ir::Mode::Run, &f)
        .expect("超 RSS 应返回结构化结果");
    assert!(
        out.resource_exceeded,
        "应判 resource-exceeded（峰值 RSS 观测值 {:?}）",
        out.peak_rss_bytes
    );
    assert!(
        out.peak_rss_bytes > 64 * 1024 * 1024,
        "峰值应确实超过上限：{:?}",
        out.peak_rss_bytes
    );
}

#[test]
fn rss_在上限内时不算失败() {
    let (dir, f) = fixture("小内存", 打印到标准错误的程序);
    let o = Oracle::new(cfg_for(&dir)).expect("oracle");
    let out = o.run_mode(rgoc_harness::ir::Mode::Run, &f).expect("run");
    assert!(
        !out.resource_exceeded,
        "小程序不该超限：{:?}",
        out.peak_rss_bytes
    );
    // 观测值至少要有 —— 观测不到就无法判「超限」，那等于形同虚设
    assert!(
        out.peak_rss_bytes > 0,
        "应当能观测到峰值 RSS（读 /proc/<pid>/status 的 VmHWM）"
    );
}

#[test]
fn run_层_走_fast_path_而不是_go_run() {
    // 官方 :1069-1077：无 flags 时 compile + link + 直跑 exe。
    // 这不只是「更贴近官方」——它让超时时 `child.kill()` 就够（父子关系是直的），
    // 不需要 unsafe 的 kill(-pgid)，而 workspace 是 `unsafe_code = "forbid"`。
    let (dir, f) = fixture("fastpath", 打印到标准错误的程序);
    let o = Oracle::new(cfg_for(&dir)).expect("oracle");
    let steps = o.steps(rgoc_harness::ir::Mode::Run, &f);
    assert_eq!(
        steps.len(),
        3,
        "run 层应是 compile -> link -> 直跑 exe：{steps:?}"
    );
    assert_eq!(steps[0].args[0], "tool");
    assert_eq!(steps[0].args[1], "compile");
    assert_eq!(steps[1].args[1], "link");
    assert!(
        steps
            .iter()
            .all(|s| s.args.first().map(String::as_str) != Some("run")),
        "fast path 里不该出现 go run：{steps:?}"
    );
    assert!(
        steps[2].args.is_empty(),
        "最后一步只有 program、没有参数：{:?}",
        steps[2]
    );
}
