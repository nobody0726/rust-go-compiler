//! `--sched=seed:N` 的参数面（`T-CON-57`，`M8-plan.md` Task 5）。
//!
//! # 这一版测什么、不测什么
//!
//! Task 5 只落**参数面**：`--sched` 能解析、非法值必须报错、默认值固定。
//! 「seed 真的改变了 select 的选择」要到 Task 37 才有东西可观测（现在
//! `select` 连执行都没有），那条端到端判据是 `T-CON-28/29`。
//!
//! 所以这里刻意**不**假装能测「seed 生效」—— 用 `--sched=seed:7` 跑一个
//! 没有 select 的程序，输出必然与 seed=0 一样，那样的断言恒真。
//!
//! # 为什么非法值必须报错而不是回落到默认
//!
//! `--sched=random` 看起来像「随便跑」，但 M8 只有确定性调度（`C11` §5）。
//! 静默回落等于**承诺了一个不存在的模式**，而用户拿到的输出仍然叫
//! 「random 跑出来的」—— 这正是 `T37` 那条「不许假装支持」的 CLI 纪律。

use std::path::PathBuf;

use rgoc_driver::cli::{CliError, Command, RunCommand, Sched, Via, parse_args};

/// 解析 `run` 的参数，断言它确实是 `Command::Run`，返回里面的 `RunCommand`。
fn run_of(argv: &[&str]) -> RunCommand {
    match parse_args(argv).expect("参数应当合法") {
        Command::Run(c) => c,
        other => panic!("应当是 Run，实际是 {other:?}"),
    }
}

#[test]
fn t_con_57_默认_seed_是_0() {
    // 默认必须是「源码顺序」模式（`D-M8-3`）：语料的期望值是在它上面冻结的，
    // 换默认等于悄悄换判据。
    assert_eq!(
        run_of(&["run", "a.go"]).sched,
        Sched::Deterministic { seed: 0 },
        "不给 --sched ⇒ seed=0"
    );
    assert_eq!(
        Sched::default(),
        Sched::Deterministic { seed: 0 },
        "Default 也必须是 seed=0"
    );
}

#[test]
fn t_con_57_sched_两种写法都收() {
    // `--sched=seed:7` 与 `--sched seed:7` 都收：脚本用前者、人敲后者，
    // 只收一种就等于让另一个入口静默失灵（与 `--via` 同一条理由）。
    assert_eq!(
        run_of(&["run", "--sched=seed:7", "a.go"]).sched,
        Sched::Deterministic { seed: 7 },
        "--sched=seed:7"
    );
    assert_eq!(
        run_of(&["run", "--sched", "seed:7", "a.go"]).sched,
        Sched::Deterministic { seed: 7 },
        "--sched seed:7"
    );
    // 上限：u64::MAX 是合法 seed（不许因为「看着太大」就拒）。
    assert_eq!(
        run_of(&["run", "--sched=seed:18446744073709551615", "a.go"])
            .sched
            .seed(),
        u64::MAX,
        "u64::MAX 必须被接受"
    );
    // 其它参数不受影响。
    let c = run_of(&["run", "--sched=seed:3", "--max-steps", "5", "a.go"]);
    assert_eq!(c.sched, Sched::Deterministic { seed: 3 });
    assert_eq!(c.max_steps, Some(5));
    assert_eq!(c.via, Via::Hir);
    assert_eq!(c.files, vec![PathBuf::from("a.go")]);
}

#[test]
fn t_con_57_sched_非法值必须报错() {
    // 逐条都要报 `BadSched`，且**不许**回落到 seed=0。
    for bad in [
        "seed:x",  // 不是数字
        "seed:",   // 空
        "seed",    // 缺冒号与数字
        "seed:7x", // 尾巴带脏字符
        "seed:-1", // 负数
        "seed:+7", // 显式正号：不是「纯数字」，拒绝比接受更容易解释
        "seed: 7", // 带空格
        "random",  // 未实现的模式
        "",        // 空值
        "Seed:7",  // 大小写敏感
        "7",       // 只给数字
    ] {
        let argv = ["run", &format!("--sched={bad}"), "a.go"];
        match parse_args(&argv) {
            Err(CliError::BadSched(v)) => assert_eq!(v, bad, "报错要回显原值"),
            other => panic!("--sched={bad:?} 必须报 BadSched，实际 {other:?}"),
        }
    }
    // 空格写法同样要报。
    assert_eq!(
        parse_args(&["run", "--sched", "random", "a.go"]).unwrap_err(),
        CliError::BadSched("random".to_string())
    );
    // 缺值：给的是文件还是漏了值？—— 这里 `a.go` 会被当成值并判为非法，
    // 所以真正的「缺值」只在末尾出现。
    assert_eq!(
        parse_args(&["run", "--sched"]).unwrap_err(),
        CliError::MissingValue("--sched")
    );
}

#[test]
fn t_con_57_错误文本带用法() {
    let msg = CliError::BadSched("random".to_string()).to_string();
    assert!(msg.contains("--sched"), "错误文本必须点名是哪个旗标：{msg}");
    assert!(msg.contains("random"), "必须回显收到的值：{msg}");
    assert!(
        msg.contains("--sched=seed:N"),
        "USAGE 必须列出正确写法（否则用户不知道 `seed:` 还是 `seed=`）：{msg}"
    );
    // USAGE 是硬编码常量，`run` 那一行必须带上新旗标。
    let usage = CliError::RunNeedsFile.to_string();
    assert!(
        usage.contains("[--sched=seed:N]"),
        "run 的用法行要登记 --sched：{usage}"
    );
}

#[test]
fn t_con_57_sched_不污染其它子命令() {
    // `--sched` 只属于 `run`；别的子命令收到它必须报未知选项，不许静默吞掉。
    assert_eq!(
        parse_args(&["ssa", "--dump", "--sched=seed:7", "a.go"]).unwrap_err(),
        CliError::UnknownFlag("--sched=seed:7".to_string())
    );
    assert_eq!(
        parse_args(&["check", "--sched=seed:7", "a.go"]).unwrap_err(),
        CliError::UnknownFlag("--sched=seed:7".to_string())
    );
}
