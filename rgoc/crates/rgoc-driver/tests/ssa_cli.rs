//! `ssa` 子命令的**参数与通道**契约（M6/Task 2 的骨架版，`T-SSA-38` 的前半）。
//!
//! # 这一版测什么、不测什么
//!
//! Task 2 只落「参数解析 + 三条通道」这层皮：IR 管线要到 Task 7 起才有东西可 dump。
//! 所以这里**不测 dump 内容**，只测两件在骨架期就能钉死的事：
//!
//! 1. **参数面**：`ssa` 必须有文件；`--dump` / `--verify` 至少给一个。
//! 2. **通道面**：任何错误都不许写 stdout（否则 `run-collide` 分不清
//!    「程序的字节」与「编译器的抱怨」）。
//!
//! # 为什么骨架期就要有「不假装支持」的判据
//!
//! `T37` 的纪律：**未实现的子命令必须报错，不许悄悄接受**。`ssa` 在 Task 2 时
//! 确实还没有 IR —— 那就必须**响亮地**说「尚未接入」，而不是打一个空 dump
//! 让人以为跑通了。这条断言在 Task 16 会被换成「dump 有内容」，但**在此之前**
//! 它是防止「假绿」的唯一一道闸。

use std::path::{Path, PathBuf};

use rgoc_driver::cli::{CliError, Command, SsaCommand, parse_args};
use rgoc_driver::ssa_cmd;

/// 仓库根的语料文件（`CARGO_MANIFEST_DIR` 是 `rgoc/crates/rgoc-driver`）。
fn corpus_file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../go_source_code/test/helloworld.go")
}

fn ssa(files: &[&str], dump: bool, verify: bool) -> SsaCommand {
    SsaCommand {
        files: files.iter().map(PathBuf::from).collect(),
        dump,
        verify,
        opt: rgoc_ssa::opt::Config::all(),
    }
}

#[test]
fn t_ssa_38_解析_ssa_dump_与_verify() {
    assert_eq!(
        parse_args(&["ssa", "--dump", "a.go"]).unwrap(),
        Command::Ssa(ssa(&["a.go"], true, false)),
        "--dump 只置 dump"
    );
    assert_eq!(
        parse_args(&["ssa", "--verify", "a.go", "b.go"]).unwrap(),
        Command::Ssa(ssa(&["a.go", "b.go"], false, true)),
        "--verify 收多文件"
    );
    assert_eq!(
        parse_args(&["ssa", "--dump", "--verify", "a.go"]).unwrap(),
        Command::Ssa(ssa(&["a.go"], true, true)),
        "两个旗标可同时给（Task 38 要求两者都输出）"
    );
}

#[test]
fn t_ssa_38_解析_ssa_缺文件或缺模式必须报错() {
    assert_eq!(
        parse_args(&["ssa"]).unwrap_err(),
        CliError::SsaNeedsFile,
        "没有文件 ⇒ 报错（不静默跑 0 个）"
    );
    assert_eq!(
        parse_args(&["ssa", "a.go"]).unwrap_err(),
        CliError::SsaNeedsMode,
        "既没 --dump 也没 --verify ⇒ 报错（不猜用户要哪个）"
    );
}

#[test]
fn t_ssa_38_骨架_文件不可读时走_err() {
    // `Err` ⇒ `main_entry` 打 stderr + 退出码 2，**绝不写 stdout**
    let err = ssa_cmd::run_ssa(&ssa(&["/nonexistent/rgoc-m6.go"], true, false)).unwrap_err();
    assert!(
        err.contains("读"),
        "错误消息要说明是「读文件」失败，实际：{err:?}"
    );
}

#[test]
fn t_ssa_31_接上管线之后_dump_有内容且通道分开() {
    // Task 31 把 `ssa` 从骨架接到真管线：dump 走 stdout、错误走 stderr、退出码 0。
    let file = corpus_file();
    let out = ssa_cmd::run_ssa(&ssa(&[file.to_str().unwrap()], true, true)).unwrap();
    assert_eq!(out.exit, 0, "helloworld 该建图成功并过 verify：{out:?}");
    assert!(
        out.stdout.contains("# stage: ssa"),
        "dump 必须在 stdout 且带版本头，实际：{:?}",
        out.stdout
    );
    assert!(
        out.stdout.contains("# func main"),
        "dump 里要有 main，实际：{:?}",
        out.stdout
    );
    assert!(
        out.stderr.is_empty(),
        "没有错误时 stderr 必须为空，实际：{:?}",
        out.stderr
    );
}

// ── Task 37：`run --via={hir,ssa}`（`T-SSA-39`）─────────────────────

use rgoc_driver::cli::{RunCommand, Via};
use rgoc_driver::run_cmd::{RunOutcome, run_run};

/// 跑一段源码的 `run`（`via` 由调用者定）。
fn run_via(name: &str, src: &str, via: Via) -> RunOutcome {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/ssa-via");
    std::fs::create_dir_all(&dir).expect("建不了 target/tmp/ssa-via");
    let path = dir.join(format!("{name}.go"));
    std::fs::write(&path, src).expect("写不了 fixture");
    run_run(&RunCommand {
        files: vec![path],
        dump_hir: false,
        max_steps: None,
        lang: None,
        via,
        opt: rgoc_ssa::opt::Config::all(),
        sched: rgoc_driver::cli::Sched::default(),
    })
    .expect("内部错（不该发生）")
}

#[test]
fn t_ssa_39_解析_run_的_via() {
    let base = RunCommand {
        files: vec![PathBuf::from("a.go")],
        dump_hir: false,
        max_steps: None,
        lang: None,
        via: Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
        sched: rgoc_driver::cli::Sched::default(),
    };
    assert_eq!(
        parse_args(&["run", "a.go"]).unwrap(),
        Command::Run(base.clone()),
        "不给 --via ⇒ 默认 hir（语料 69/69 是在它上面冻结的）"
    );
    assert_eq!(
        parse_args(&["run", "--via", "hir", "a.go"]).unwrap(),
        Command::Run(base.clone()),
        "--via hir 显式给也一样"
    );
    assert_eq!(
        parse_args(&["run", "--via", "ssa", "a.go"]).unwrap(),
        Command::Run(RunCommand {
            via: Via::Ssa,
            ..base.clone()
        }),
        "--via ssa 选中 SSA 通路"
    );
    // `--via=X` 的等号写法也必须认（`run-collide.sh --ssa` 用的就是它）
    assert_eq!(
        parse_args(&["run", "--via=ssa", "a.go"]).unwrap(),
        Command::Run(RunCommand {
            via: Via::Ssa,
            ..base.clone()
        }),
        "--via=ssa 与 --via ssa 等价"
    );
    // 非法值必须报错，不猜（猜错的代价是「跑了另一条通路还以为是这条」）
    assert_eq!(
        parse_args(&["run", "--via", "hir2", "a.go"]).unwrap_err(),
        CliError::BadVia("hir2".to_string())
    );
    assert_eq!(
        parse_args(&["run", "--via=hir2", "a.go"]).unwrap_err(),
        CliError::BadVia("hir2".to_string())
    );
    assert_eq!(
        parse_args(&["run", "--via"]).unwrap_err(),
        CliError::MissingValue("--via")
    );
}

#[test]
fn t_ssa_39_两条通路在正常程序上输出一致() {
    let src = "package main\n\ntype P struct{ x int }\n\nfunc (p *P) add(d int) {\n\tp.x += d\n}\n\nfunc fib(n int) int {\n\tif n < 2 {\n\t\treturn n\n\t}\n\treturn fib(n-1) + fib(n-2)\n}\n\nfunc main() {\n\tp := P{1}\n\tp.add(4)\n\ts := []int{1, 2}\n\ts = append(s, 3)\n\tprintln(p.x, fib(7), len(s), s[2])\n}\n";
    let hir = run_via("via-eq", src, Via::Hir);
    let ssa = run_via("via-eq", src, Via::Ssa);
    assert_eq!(hir.exit, 0, "hir 通路该成功：{}", hir.stderr);
    assert_eq!(ssa.exit, 0, "ssa 通路该成功：{}", ssa.stderr);
    assert_eq!(ssa.stdout, hir.stdout, "两条通路必须逐字节一致");
    assert_eq!(hir.stdout, "5 13 3 3\n");
}

#[test]
fn t_ssa_39_via_ssa_的拒绝面退出码是_1() {
    // 拒绝是**静态面**的结论（`SSA-UNSUPPORTED-*`），所以退出码 1，不是 2
    let o = run_via(
        "via-reject",
        "package main\n\nfunc main() {\n\tm := map[int]int{}\n\t_ = m\n}\n",
        Via::Ssa,
    );
    assert_eq!(o.exit, 1, "拒绝 ⇒ 退出码 1：{:?}", o.stderr);
    assert!(
        o.stderr.contains("SSA-UNSUPPORTED-MAP"),
        "stderr 要带稳定码：{:?}",
        o.stderr
    );
    assert!(o.stdout.is_empty(), "拒绝时 stdout 必须为空");
}

#[test]
fn t_ssa_39_via_ssa_的_panic_退出码是_2() {
    let o = run_via(
        "via-panic",
        "package main\n\nfunc main() {\n\tprintln(\"before\")\n\tpanic(\"boom\")\n}\n",
        Via::Ssa,
    );
    assert_eq!(o.exit, 2, "未捕获 panic ⇒ 退出码 2");
    assert_eq!(o.stdout, "before\npanic: boom\n");
    assert!(o.stderr.is_empty(), "panic 不是诊断（与 hir 同口径）");
}
