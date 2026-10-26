//! GC 装置自身的判据（M10/Task 4）。
//!
//! # 为什么装置也要判据
//!
//! `support/gc_probe.rs` 是后面每一条 GC 判据的观测面。它一旦「宽容」，
//! 所有下游判据都会跟着变软 —— 而症状是「全绿」。这里钉三件事：
//!
//! 1. **编译失败必须报错**：否则「语法错的程序」会变成「空 stdout + 空 stderr」，
//!    而空 stdout 恰好等于很多期望值（`M10-plan.md` Task 4 Step 1 的原话：
//!    沿 `native_smoke.rs` 的先例）。
//! 2. **解析器对坏行必须报错而不是返回 `None`**：`None` 的语义是「没设 STATS」，
//!    把「行被改坏了」混进 `None` 会让契约漂移不可见。
//! 3. **键序变了要报错**：`C12` §5.2 冻结了 11 个键的顺序，而后续 Phase 会往
//!    行里加字段 —— 加错位置必须红。

mod support;

use support::gc_probe::{KEYS, parse_stats, run_with_env};
use support::native_harness::build_and_run_env;

/// 一条合法统计行（与 `rt_gc_stats_line` 的输出逐字节同形）。
const GOOD: &str = "gc: collections=3 allocs=10 frees=7 live=256 heap=16777344 chunks=1 rss=0 kinds=2,3,1,4,0 bad_sp=0 bad_ptr=0 bad_desc=0";

#[test]
fn t_gc_probe_01_编译失败必须报错() {
    // `func` 后面缺名字 —— 前端必然拒绝。
    let bad = "package main\n\nfunc () {}\n";
    let r = build_and_run_env("gc_probe_bad", bad, &[("RGOC_GC_STATS", "1")]);
    let err = r.expect_err("语法错的程序必须返回 Err，不能变成「空输出 = 通过」");
    assert!(
        err.contains("build 失败"),
        "错误里要看得出是构建失败，实际：{err}"
    );
}

#[test]
fn t_gc_probe_02_解析器接受合法行() {
    let s = parse_stats(GOOD)
        .expect("合法行不该报错")
        .expect("必须有值");
    assert_eq!(s.collections, 3);
    assert_eq!(s.allocs, 10);
    assert_eq!(s.frees, 7);
    assert_eq!(s.live, 256);
    assert_eq!(s.heap, 16_777_344);
    assert_eq!(s.chunks, 1);
    assert_eq!(s.rss, 0);
    assert_eq!(s.kinds, [2, 3, 1, 4, 0]);
    s.assert_clean().expect("三个 bad_* 都是 0");
}

#[test]
fn t_gc_probe_03_没有统计行返回_none() {
    assert_eq!(
        parse_stats("panic: boom\n").expect("没 gc 行不是格式错"),
        None
    );
    assert_eq!(parse_stats("").expect("空 stderr 不是格式错"), None);
}

#[test]
fn t_gc_probe_04_坏行必须报错() {
    // 少一个键
    let missing = GOOD.replace(" bad_desc=0", "");
    let e = parse_stats(&missing).expect_err("少键必须报错");
    assert!(e.contains("字段序不对"), "实际：{e}");

    // 键序被换
    let swapped = GOOD.replace("allocs=10 frees=7", "frees=7 allocs=10");
    let e = parse_stats(&swapped).expect_err("换序必须报错");
    assert!(e.contains("字段序不对"), "实际：{e}");

    // 值不是十进制
    let nonnum = GOOD.replace("collections=3", "collections=x");
    let e = parse_stats(&nonnum).expect_err("非数字必须报错");
    assert!(e.contains("不是十进制"), "实际：{e}");

    // kinds 不是五个
    let four = GOOD.replace("kinds=2,3,1,4,0", "kinds=2,3,1,4");
    let e = parse_stats(&four).expect_err("kinds 少一项必须报错");
    assert!(e.contains("五个"), "实际：{e}");

    // 两行统计
    let two = format!("{GOOD}\n{GOOD}\n");
    let e = parse_stats(&two).expect_err("两行统计必须报错");
    assert!(e.contains("不止一次"), "实际：{e}");
}

#[test]
fn t_gc_probe_05_空程序不分配也不占堆() {
    // Phase 0 还没有 GC 消费点：所有计数必须是 0。
    //
    // `chunks == 0` 是**对的**，不是漏了：arena 是惰性的（`rt.c` 的 `rt_grow`
    // 只在第一次分配时 mmap）。这条顺带钉住「惰性」本身 —— 如果哪天有人在
    // `rt_init` 里预分配一个 chunk，这条会红。
    let src = "package main\n\nfunc main() { println(\"hi\") }\n";
    let (out, s) = run_with_env("gc_probe_zero", src, &[]).expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "hi\n");
    assert_eq!(s.collections, 0);
    assert_eq!(s.allocs, 0);
    assert_eq!(s.frees, 0);
    assert_eq!(s.live, 0);
    assert_eq!(s.heap, 0);
    assert_eq!(s.chunks, 0, "空程序不分配 ⇒ 一个 chunk 都不该 mmap");
    assert_eq!(s.kinds, [0, 0, 0, 0, 0]);
    s.assert_clean().expect("三个 bad_* 都是 0");
}

#[test]
fn t_gc_probe_07_分配过就有_chunk() {
    // 与上一条互补：分配之后 `chunks` 与 `heap` 必须自洽（`C12` §1.4 不变量 5）。
    let src = "package main\n\nfunc main() {\n\tp := new(int)\n\t*p = 7\n\tprintln(*p)\n}\n";
    let (out, s) = run_with_env("gc_probe_chunk", src, &[]).expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "7\n");
    assert!(s.chunks >= 1, "分配过就至少一个 chunk，实际 {}", s.chunks);
    assert!(
        s.heap >= 16 * 1024 * 1024,
        "heap 必须 >= 16 MiB（一个 chunk 的 data 容量），实际 {}",
        s.heap
    );
}

#[test]
fn t_gc_probe_06_keys_与契约一致() {
    // 把 `C12` §5.2 的键序抄一份在这里**是刻意的**：解析器用的是同一份常量，
    // 所以这条只证明「常量没被悄悄改」。真正的对账在 `gc_stats::t_gc_02`
    // （它拿运行期的行去比 `KEYS`）。
    assert_eq!(KEYS.len(), 11);
    assert_eq!(KEYS[0], "collections");
    assert_eq!(KEYS[10], "bad_desc");
}
