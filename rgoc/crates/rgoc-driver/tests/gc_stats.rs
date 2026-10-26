//! GC 统计行与三个环境变量（M10/Task 3，`T-GC-02`；契约 `C12` §5）。
//!
//! # 为什么这一条要在「堆还没造出来」的时候就立起来
//!
//! 统计行是后面**所有** GC 判据的观测面（`T-GC-01/04/05/09/12` 全靠解析它）。
//! 如果它到 Phase 3 才接，前面每个 Task 都只能靠「跑完不崩」当判据 —— 而
//! 「不崩」对「GC 根本没跑」也是成立的。所以先把**行**立起来（字段全 0），
//! 再逐 Task 把字段填实。
//!
//! # 三条判据，缺一条就会假绿
//!
//! 1. **stdout 一个字节不多**：统计行进 stderr 是 `D-M10-15` 的全部内容。
//!    只断言「stderr 有一行」的话，把它打到 stdout 也能过 —— 而那会改
//!    `C9` §4.1 的合并流。
//! 2. **不设 `RGOC_GC_STATS` 时 stderr 为空**：没有这条，「无条件打一行」
//!    也能过第 1 条。
//! 3. **字段齐全且顺序固定**：`C12` §5.2 冻结了 11 个键的顺序。只断言前缀
//!    `gc: collections=` 的话，后面 Phase 加字段时会悄悄改序，而解析方
//!    （`gc_probe.rs` / `gc_corpus.rs`）是按下标取的。

mod support;

use support::native_harness::build_and_run_env;

const HI: &str = "package main\n\nfunc main() { println(\"hi\") }\n";

/// `C12` §5.2 冻结的键序（`kinds` 是复合值、`mode` 是枚举，单列）。
const KEYS: [&str; 12] = [
    "collections",
    "allocs",
    "frees",
    "live",
    "heap",
    "chunks",
    "rss",
    "kinds",
    "bad_sp",
    "bad_ptr",
    "bad_desc",
    "mode",
];

/// 把 `gc: k=v k=v …` 拆成**保序**的 `(key, value)` 表。
fn parse(line: &str) -> Vec<(String, String)> {
    let body = line
        .strip_prefix("gc: ")
        .unwrap_or_else(|| panic!("统计行必须以 `gc: ` 开头，实际：{line}"));
    body.split(' ')
        .map(|kv| {
            let (k, v) = kv
                .split_once('=')
                .unwrap_or_else(|| panic!("字段不是 k=v 形态：{kv}（整行：{line}）"));
            (k.to_string(), v.to_string())
        })
        .collect()
}

#[test]
fn t_gc_02_统计行只进_stderr_且字段齐全() {
    let out = build_and_run_env("gc_stats_hi", HI, &[("RGOC_GC_STATS", "1")])
        .expect("编译 + 运行不该失败");

    assert_eq!(out.exit, 0, "退出码必须是 0，stderr={}", out.stderr);
    // ① stdout 一个字节不多
    assert_eq!(out.stdout, "hi\n", "统计行不许污染 stdout（C9 §4.1）");

    // ② stderr 恰好一行
    let lines: Vec<&str> = out.stderr.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "stderr 必须恰好一行，实际 {} 行：{:?}",
        lines.len(),
        out.stderr
    );

    // ③ 字段齐全且顺序固定
    let fields = parse(lines[0]);
    let got: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(got, KEYS, "字段顺序是 `C12` §5.2 冻结的契约");

    // 值域：`kinds` 是五个十进制、`mode` 是三选一、其余是十进制
    for (k, v) in &fields {
        if k == "kinds" {
            let parts: Vec<&str> = v.split(',').collect();
            assert_eq!(parts.len(), 5, "kinds 必须是五个计数，实际：{v}");
            for p in parts {
                assert!(
                    p.parse::<u64>().is_ok(),
                    "kinds 的每一项都要是十进制，实际：{p}"
                );
            }
        } else if k == "mode" {
            assert!(
                matches!(v.as_str(), "off" | "on" | "stress"),
                "mode 只认 off|on|stress，实际：{v}"
            );
        } else {
            assert!(v.parse::<u64>().is_ok(), "`{k}` 的值要是十进制，实际：{v}");
        }
    }
}

#[test]
fn t_gc_02b_不设_stats_时_stderr_为空() {
    let out = build_and_run_env("gc_stats_off", HI, &[]).expect("编译 + 运行不该失败");
    assert_eq!(out.exit, 0);
    assert_eq!(out.stdout, "hi\n");
    assert_eq!(
        out.stderr, "",
        "没设 RGOC_GC_STATS 就一个字节都不许打（C12 §5.2）"
    );
}
