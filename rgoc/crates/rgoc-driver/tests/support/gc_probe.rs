#![allow(dead_code)]
//! GC 判据的公共装置（M10/Task 4，契约 `C12` §5.2 / §5.4）。
//!
//! # 为什么所有 GC 判据都从**统计行**读，而不新增内建
//!
//! 加一个 `gc.stats()` 内建意味着：`C8` 的诊断面要动、`C9` 的值面要动、
//! `rgoc-interp` 也要有一份（否则 `--via=ssa` 拒绝）。而统计行**已经是**
//! 契约的一部分（`C12` §5.2）—— 测试读它，等于同时验证了「行还在」。
//!
//! # 为什么要 `Stats` 结构而不是裸 `HashMap`
//!
//! `C12` §5.2 冻结了 11 个键的**顺序**。用 `HashMap` 就丢掉顺序，而顺序是
//! 契约的一部分（`gc_stats::t_gc_02` 钉它）。这里按顺序解析、缺键即失败 ——
//! 少一个键就是契约漂了。

use std::collections::BTreeMap;

use super::native_harness::{Outcome, build_and_run_env};

/// `C12` §5.2 的 11 个字段，按冻结顺序。
pub const KEYS: [&str; 11] = [
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
];

/// 一次 GC 统计的读数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    pub collections: u64,
    pub allocs: u64,
    pub frees: u64,
    pub live: u64,
    pub heap: u64,
    pub chunks: u64,
    pub rss: u64,
    /// `object / array / closure / raw / mapslots` 五个累计分配计数。
    pub kinds: [u64; 5],
    pub bad_sp: u64,
    pub bad_ptr: u64,
    pub bad_desc: u64,
}

impl Stats {
    /// 三个自检计数器全 0 —— 「位图 / 位点 / desc 都没错」的**唯一**判据。
    ///
    /// # Errors
    ///
    /// 任一计数器非 0。
    pub fn assert_clean(&self) -> Result<(), String> {
        if self.bad_sp == 0 && self.bad_ptr == 0 && self.bad_desc == 0 {
            return Ok(());
        }
        Err(format!(
            "自检计数器必须全 0（C12 §5.4）：bad_sp={} bad_ptr={} bad_desc={}",
            self.bad_sp, self.bad_ptr, self.bad_desc
        ))
    }
}

/// 解析 stderr 里的 `gc: …` 行。没有该行 ⇒ `None`。
///
/// # Errors
///
/// 行存在但**格式不对**（前缀错 / 键序错 / 少键 / 值不是十进制）—— 这类必须
/// 报错而不是返回 `None`：返回 `None` 会让「行被改坏了」看起来像「没设 STATS」。
pub fn parse_stats(stderr: &str) -> Result<Option<Stats>, String> {
    let mut found: Option<&str> = None;
    for line in stderr.lines() {
        if line.starts_with("gc: ") {
            if found.is_some() {
                return Err(format!("统计行出现了不止一次：\n{stderr}"));
            }
            found = Some(line);
        }
    }
    let Some(line) = found else {
        return Ok(None);
    };

    let body = line.strip_prefix("gc: ").unwrap_or_default();
    let mut pairs: Vec<(&str, &str)> = Vec::new();
    for kv in body.split(' ') {
        let (k, v) = kv
            .split_once('=')
            .ok_or_else(|| format!("字段不是 k=v 形态：`{kv}`（整行：{line}）"))?;
        pairs.push((k, v));
    }

    let got: Vec<&str> = pairs.iter().map(|(k, _)| *k).collect();
    if got != KEYS {
        return Err(format!(
            "字段序不对（C12 §5.2 冻结）：\n  期望 {KEYS:?}\n  实际 {got:?}"
        ));
    }

    let mut map: BTreeMap<&str, &str> = BTreeMap::new();
    for (k, v) in pairs {
        map.insert(k, v);
    }
    let num = |k: &str| -> Result<u64, String> {
        let v = map.get(k).copied().unwrap_or_default();
        v.parse::<u64>()
            .map_err(|_| format!("`{k}` 的值不是十进制：`{v}`"))
    };

    let kinds_str = map.get("kinds").copied().unwrap_or_default();
    let parts: Vec<&str> = kinds_str.split(',').collect();
    if parts.len() != 5 {
        return Err(format!("kinds 必须是五个计数，实际：`{kinds_str}`"));
    }
    let mut kinds = [0u64; 5];
    for (i, p) in parts.iter().enumerate() {
        kinds[i] = p
            .parse::<u64>()
            .map_err(|_| format!("kinds 的第 {i} 项不是十进制：`{p}`"))?;
    }

    Ok(Some(Stats {
        collections: num("collections")?,
        allocs: num("allocs")?,
        frees: num("frees")?,
        live: num("live")?,
        heap: num("heap")?,
        chunks: num("chunks")?,
        rss: num("rss")?,
        kinds,
        bad_sp: num("bad_sp")?,
        bad_ptr: num("bad_ptr")?,
        bad_desc: num("bad_desc")?,
    }))
}

/// 编译 + 运行一次，并**要求**有统计行（`RGOC_GC_STATS=1` 自动加上）。
///
/// # 为什么强制要统计行
///
/// 后面每一条 GC 判据都要读它。如果某次装置调用拿不到统计行，「读数全 0」
/// 与「统计行没了」在断言里长得一模一样 —— 强制 `Err` 把这两种情况分开。
///
/// # Errors
///
/// 编译/运行失败，或 stderr 里没有统计行，或统计行格式不对。
pub fn run_with_env(
    name: &str,
    src: &str,
    env: &[(&str, &str)],
) -> Result<(Outcome, Stats), String> {
    let mut all: Vec<(&str, &str)> = vec![("RGOC_GC_STATS", "1")];
    all.extend_from_slice(env);
    let out = build_and_run_env(name, src, &all)?;
    let stats = parse_stats(&out.stderr)?.ok_or_else(|| {
        format!(
            "stderr 里没有 `gc: ` 行（RGOC_GC_STATS=1 时必须有）：\n{}",
            out.stderr
        )
    })?;
    Ok((out, stats))
}
