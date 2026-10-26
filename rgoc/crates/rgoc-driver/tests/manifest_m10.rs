//! `M10-manifest.json` 的对账（M10-plan.md Task 40）。
//!
//! # 为什么清单也要有判据
//!
//! M9 的教训：`manifest.measured` 是**自报**，不是证据 —— 有一轮 `fmt` 红了
//! 而 `test` 全绿，清单里照样写着「四条门禁全绿」。清单一旦能说谎，后面所有
//! 「已完成」都不可信。
//!
//! 所以这里做两件事：
//!
//! 1. **结构对账**：门禁 id 集合 / 状态与证据的搭配 / 语料计数与路径列一致 /
//!    未支持面逐条有证据 / `open_gaps` 与 `partial` 一一对应。
//! 2. **数字复现**（M10 特有，`R-10-26`）：`measured.longrun` 里的
//!    `collections` / `live_bytes` / `heap_committed` / `chunks` 必须能**从一次
//!    真实长跑里原样读出来**。跑一次 1e5 次分配的 fixture 大约 0.3 秒 ——
//!    比「相信清单」便宜得多。
//!
//! # 判据本身要可证伪
//!
//! `reconcile` 的清单由调用方传入，`合成反例都必须红` 喂**改坏的**清单进去。
//! 否则「对账通过」只说明对账函数什么都没查。

mod support;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use support::gc_probe::run_with_env;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("仓库根")
        .to_path_buf()
}

fn read_json(rel: &str) -> Value {
    let p = repo_root().join(rel);
    let text =
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读 {} 失败：{e}", p.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} 不是合法 JSON：{e}", p.display()))
}

/// 清单文件的**路径列**（跳过空行与 `#` 注释）。
fn line_list(rel: &str) -> Vec<String> {
    let p = repo_root().join(rel);
    let text =
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读 {} 失败：{e}", p.display()));
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(ToString::to_string)
        .collect()
}

/// 所有 `tests/` 目录下的 `.rs` 文件名集合。
fn test_files_on_disk() -> BTreeSet<String> {
    fn collect(dir: &Path, out: &mut BTreeSet<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs")
                && let Some(name) = p.file_name().and_then(|s| s.to_str())
            {
                out.insert(name.to_string());
            }
        }
    }
    let mut out = BTreeSet::new();
    collect(&repo_root().join("rgoc/crates"), &mut out);
    out
}

/// `evidence` 里提到的 `<名字>.rs` 是否真实存在。
fn mentioned_rs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_alphanumeric() || bytes[i] == '_' {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_alphanumeric() || bytes[i] == '_' || bytes[i] == '.')
            {
                i += 1;
            }
            let word: String = bytes[start..i].iter().collect();
            if word.ends_with(".rs") && word.len() > 3 {
                out.push(word);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// 对账。返回**全部**问题（不是第一个）。
fn reconcile(m: &Value, selected: &[String]) -> Vec<String> {
    let mut errs = Vec::new();

    if m["stage"].as_str() != Some("M10") {
        errs.push("stage 必须是 M10".into());
    }

    // ── ① 门禁：9 条 GC-1..GC-9 ──
    let want: BTreeSet<String> = (1..=9).map(|i| format!("GC-{i}")).collect();
    let gate = m["gate"].as_array().expect("gate 应是数组");
    if gate.len() != want.len() {
        errs.push(format!("gate 应有 {} 条，实际 {}", want.len(), gate.len()));
    }
    let mut ids = BTreeSet::new();
    for g in gate {
        let id = g["id"].as_str().unwrap_or("<无 id>").to_string();
        if !ids.insert(id.clone()) {
            errs.push(format!("门禁 id 重复：{id}"));
        }
        let status = g["status"].as_str().unwrap_or("<无 status>");
        if !matches!(status, "pass" | "partial" | "fail" | "pending") {
            errs.push(format!(
                "{id} 的 status 只能是 pass/partial/fail/pending，实际 {status:?}"
            ));
        }
        if status == "pending" && g["evidence"].as_str().is_some_and(|s| !s.is_empty()) {
            errs.push(format!("{id} 还是 pending 却已经写了 evidence"));
        }
        if status == "pass" {
            let ev = g["evidence"].as_str().unwrap_or("");
            if ev.is_empty() {
                errs.push(format!("{id} 没有 evidence —— 「做完了」不是证据"));
            }
            for bad in ["未实现", "未兑现", "TODO", "没有实现", "待填"] {
                if ev.contains(bad) {
                    errs.push(format!("{id} 标 pass 但证据里有「{bad}」"));
                }
            }
            // 证据里点名的 `.rs` 必须真实存在。
            for f in mentioned_rs(ev) {
                if !test_files_on_disk().contains(&f) {
                    errs.push(format!("{id} 的证据提到 {f}，但盘上没有这个测试文件"));
                }
            }
        }
    }
    if ids != want {
        errs.push(format!("门禁 id 集合不对：{ids:?}"));
    }

    // ── ② 未支持面：8 条，每条有 evidence ──
    let uns = m["unsupported"].as_array().expect("unsupported 应是数组");
    if uns.len() != 8 {
        errs.push(format!("unsupported 应有 8 条，实际 {}", uns.len()));
    }
    let mut uids = BTreeSet::new();
    for u in uns {
        let id = u["id"].as_str().unwrap_or("<无 id>").to_string();
        if !uids.insert(id.clone()) {
            errs.push(format!("unsupported id 重复：{id}"));
        }
        if u["what"].as_str().is_none_or(str::is_empty) {
            errs.push(format!("{id} 没有 what"));
        }
        if u["evidence"].as_str().is_none_or(str::is_empty) {
            errs.push(format!("{id} 没有 evidence —— 「不做」也要写清为什么"));
        }
    }
    let want_u: BTreeSet<String> = (1..=8).map(|i| format!("U-M10-{i:02}")).collect();
    if uids != want_u {
        errs.push(format!("unsupported id 集合不对：{uids:?}"));
    }

    // ── ③ 缺口：pass 的门禁不许出现在 open_gaps；partial/fail 必须出现 ──
    let opens = m["open_gaps"].as_array().expect("open_gaps 应是数组");
    for g in gate {
        let id = g["id"].as_str().unwrap_or("");
        let status = g["status"].as_str().unwrap_or("");
        let listed = opens.iter().any(|x| x["gate"].as_str() == Some(id));
        if status == "pass" && listed {
            errs.push(format!("{id} 是 pass，却出现在 open_gaps 里"));
        }
        if matches!(status, "partial" | "fail") && !listed {
            errs.push(format!("{id} 是 {status}，却没进 open_gaps"));
        }
    }
    for o in opens {
        if o["what"].as_str().is_none_or(str::is_empty) {
            errs.push("open_gaps 里有一条没写 what".into());
        }
    }

    // ── ④ closed_gaps：每条有 resolution ──
    let closed = m["closed_gaps"].as_array().expect("closed_gaps 应是数组");
    let mut cids = BTreeSet::new();
    for c in closed {
        let id = c["id"].as_str().unwrap_or("<无 id>").to_string();
        if !cids.insert(id.clone()) {
            errs.push(format!("closed_gaps id 重复：{id}"));
        }
        if c["resolution"].as_str().is_none_or(str::is_empty) {
            errs.push(format!("{id} 关了却没写 resolution —— 「关了」不是证据"));
        }
    }

    // ── ⑤ 语料：count 与路径列一致；三态都写 69/0/0 且分母等于 count ──
    let n = m["corpus_lists"]["input"]["count"]
        .as_u64()
        .expect("input.count 应是数字");
    if n as usize != selected.len() {
        errs.push(format!(
            "input.count = {n}，但 run-selected.txt 有 {} 条",
            selected.len()
        ));
    }
    let counts = &m["corpus_lists"]["native"]["counts"];
    for st in ["off", "on", "stress"] {
        let Some(s) = counts[st].as_str() else {
            errs.push(format!("native.counts 缺 {st}"));
            continue;
        };
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() != 3 {
            errs.push(format!("native.counts.{st} = {s:?} 不是 a/b/c 形状"));
            continue;
        }
        if parts[0].parse::<u64>().ok() != Some(n) {
            errs.push(format!(
                "native.counts.{st} = {s} 的 ok 数不等于 {n}（语料总数）"
            ));
        }
        if parts[1] != "0" || parts[2] != "0" {
            errs.push(format!("native.counts.{st} = {s}：skip 与 bad 必须都是 0"));
        }
    }

    // ── ⑥ measured：必填项 ──
    let measured = &m["measured"];
    for key in [
        "workspace",
        "corpus_native_three_states",
        "mutations",
        "longrun",
    ] {
        if measured[key].is_null() {
            errs.push(format!("measured 缺 {key}"));
        }
    }
    for key in ["collections", "live_bytes", "heap_committed", "chunks"] {
        if measured["longrun"][key].as_u64().is_none() {
            errs.push(format!("measured.longrun 缺 {key}（数字）"));
        }
    }

    // ── ⑦ 偏差：每条四要素齐全 ──
    for d in m["measurement_deviations"]
        .as_array()
        .expect("measurement_deviations 应是数组")
    {
        for k in ["item", "trigger", "resolution"] {
            if d[k].as_str().is_none_or(str::is_empty) {
                errs.push(format!("measurement_deviations 有一条缺 {k}"));
            }
        }
        if d["planned"].is_null() || d["actual"].is_null() {
            errs.push("measurement_deviations 有一条缺 planned/actual".into());
        }
    }

    errs
}

#[test]
fn t_manifest_10_真清单必须自洽() {
    let m = read_json("docs/milestones/M10-manifest.json");
    let selected = line_list("tools/run-oracle/run-selected.txt");
    let errs = reconcile(&m, &selected);
    assert!(
        errs.is_empty(),
        "manifest 对账失败 {} 条：\n{}",
        errs.len(),
        errs.join("\n")
    );
}

/// `measured.longrun` 的四个数必须能**跑出来**（`R-10-26`）。
///
/// # 这条是 M10 特有的「自报不算证据」
///
/// 清单里的数字是手填的。手填的数字最容易漂：改了分配器、改了阈值、
/// 改了 fixture，清单还是老数字，而**没人会发现**。
///
/// 所以这里把同一个 fixture、同一个阈值再跑一遍，逐个数字比。
/// `collections` 也一起比 —— 它由分配序列唯一决定（`T-GC-08` 已证明确定性），
/// 所以「跑出来一样」是有意义的；如果哪天它变得不确定，这条会先红。
#[test]
fn t_manifest_10_长跑读数必须能复现() {
    let m = read_json("docs/milestones/M10-manifest.json");
    let lr = &m["measured"]["longrun"];
    let src = r#"package main

func main() {
	s := 0
	for i := 0; i < 100000; i++ {
		a := make([]int, 128)
		a[0] = i
		s += a[0]
	}
	println(s)
}
"#;
    let (out, s) = run_with_env("manifest_longrun", src, &[("RGOC_GC_THRESHOLD", "1048576")])
        .expect("编译 + 运行不该失败");
    assert_eq!(
        out.stdout, "4999950000\n",
        "fixture 的输出变了：stderr {}",
        out.stderr
    );

    let want = |k: &str| lr[k].as_u64().unwrap_or(u64::MAX);
    assert_eq!(
        s.collections,
        want("collections"),
        "manifest 写 collections={}，实测 {}（改了触发策略却没更新清单？）",
        want("collections"),
        s.collections
    );
    assert_eq!(
        s.live,
        want("live_bytes"),
        "manifest 写 live_bytes={}，实测 {}",
        want("live_bytes"),
        s.live
    );
    assert_eq!(
        s.heap,
        want("heap_committed"),
        "manifest 写 heap_committed={}，实测 {}",
        want("heap_committed"),
        s.heap
    );
    assert_eq!(
        s.chunks,
        want("chunks"),
        "manifest 写 chunks={}，实测 {}",
        want("chunks"),
        s.chunks
    );
}

/// 把清单改坏一处，断言**有指名道姓的错误**。
fn expect_red(what: &str, mutate: impl FnOnce(&mut Value), needle: &str) {
    let mut m = read_json("docs/milestones/M10-manifest.json");
    mutate(&mut m);
    let selected = line_list("tools/run-oracle/run-selected.txt");
    let errs = reconcile(&m, &selected);
    assert!(!errs.is_empty(), "改坏「{what}」之后居然还是绿的");
    assert!(
        errs.iter().any(|e| e.contains(needle)),
        "改坏「{what}」后应报出含 {needle:?} 的错误，实际：{errs:?}"
    );
}

#[test]
fn t_manifest_10_合成反例都必须红() {
    expect_red(
        "门禁数不对",
        |m| {
            m["gate"].as_array_mut().unwrap().pop();
        },
        "gate 应有 9 条",
    );

    expect_red(
        "标 pass 却自认没做",
        |m| {
            m["gate"][0]["evidence"] = Value::String("未实现".into());
        },
        "标 pass 但证据里有",
    );

    expect_red(
        "pass 却没有 evidence",
        |m| {
            m["gate"][0]["evidence"] = Value::String(String::new());
        },
        "没有 evidence",
    );

    expect_red(
        "pass 却进了 open_gaps",
        |m| {
            let opens = m["open_gaps"].as_array_mut().unwrap();
            opens.push(serde_json::json!({"gate": "GC-1", "what": "瞎写的"}));
        },
        "却出现在 open_gaps",
    );

    expect_red(
        "语料计数与路径列不一致",
        |m| {
            m["corpus_lists"]["input"]["count"] = Value::Number(68.into());
        },
        "run-selected.txt 有",
    );

    expect_red(
        "三态里有一态没收全",
        |m| {
            m["corpus_lists"]["native"]["counts"]["stress"] = Value::String("67/0/2".into());
        },
        "skip 与 bad 必须都是 0",
    );

    expect_red(
        "证据里点名的测试文件不存在",
        |m| {
            m["gate"][0]["evidence"] = Value::String("gc_nonexistent.rs 里那几条".into());
        },
        "但盘上没有这个测试文件",
    );

    expect_red(
        "关了缺口却没写 resolution",
        |m| {
            m["closed_gaps"][0]["resolution"] = Value::String(String::new());
        },
        "关了却没写 resolution",
    );

    expect_red(
        "未支持面少一条",
        |m| {
            m["unsupported"].as_array_mut().unwrap().pop();
        },
        "unsupported 应有 8 条",
    );

    expect_red(
        "measured 缺 longrun 的数字",
        |m| {
            m["measured"]["longrun"]["live_bytes"] = Value::Null;
        },
        "measured.longrun 缺 live_bytes",
    );
}
