//! `M11-manifest.json` 的对账（M11-plan.md Task 42）。
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
//!    未支持面逐条有证据 / 实测数字之间自洽（`depth_overflow == depth_ok + 1`）。
//! 2. **数字复现**（M11 特有）：`measured.corpus_chan_three_states` 必须能
//!    **从一次真实跑里原样读出来** —— 7 个 `N'` × 三态，逐字节相同。
//!    这条比 M10 的「长跑读数可复现」更硬：它同时钉住「三态等价」这件事本身。
//!
//! # 判据本身要可证伪
//!
//! `reconcile` 的清单由调用方传入，`合成反例都必须红` 喂**改坏的**清单进去。
//! 否则「对账通过」只说明对账函数什么都没查。

mod support;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use support::native_harness::build_and_run_env;

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

/// 清单文件的**第一列**（跳过空行与 `#` 注释）。
fn line_list(rel: &str) -> Vec<String> {
    let p = repo_root().join(rel);
    let text =
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读 {} 失败：{e}", p.display()));
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split_whitespace().next().unwrap_or(l).to_string())
        .collect()
}

/// `N'` = `chan-input` − `chan-rejected`（**算出来**，不抄第二份，`T-GDEV-01`）。
fn chan_ok() -> Vec<String> {
    let rej: BTreeSet<String> = line_list("tools/run-oracle/chan-rejected.txt")
        .into_iter()
        .collect();
    line_list("tools/run-oracle/chan-input.txt")
        .into_iter()
        .filter(|p| !rej.contains(p))
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

/// `evidence` 里提到的 `<名字>.rs`。
fn mentioned_rs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_alphanumeric() || chars[i] == '_' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
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
fn reconcile(m: &Value, selected: &[String], chan_ok_list: &[String]) -> Vec<String> {
    let mut errs = Vec::new();

    if m["stage"].as_str() != Some("M11") {
        errs.push("stage 必须是 M11".into());
    }

    // ── ① 门禁：7 条 GO-1..GO-7 ──
    let want: BTreeSet<String> = (1..=7).map(|i| format!("GO-{i}")).collect();
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
            for f in mentioned_rs(ev) {
                if !test_files_on_disk().contains(&f) {
                    errs.push(format!("{id} 的证据提到 {f}，但盘上没有这个测试文件"));
                }
            }
        }
        // `gap` 必须出现（哪怕是 `—`）：没有这一栏就没法区分「没想到」与「没有」。
        if g["gap"].is_null() {
            errs.push(format!("{id} 没有 gap 栏（没有就写 `—`）"));
        }
    }
    if ids != want {
        errs.push(format!("门禁 id 集合不对：{ids:?}"));
    }

    // ── ② 未支持面：10 条，每条有 evidence ──
    let uns = m["unsupported"].as_array().expect("unsupported 应是数组");
    if uns.len() != 10 {
        errs.push(format!("unsupported 应有 10 条，实际 {}", uns.len()));
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
    let want_u: BTreeSet<String> = (1..=10).map(|i| format!("U-M11-{i:02}")).collect();
    if uids != want_u {
        errs.push(format!("unsupported id 集合不对：{uids:?}"));
    }

    // ── ③ 缺口：pass 的门禁不许出现在 open_gaps ──
    let opens = m["open_gaps"].as_array().expect("open_gaps 应是数组");
    for g in gate {
        let id = g["id"].as_str().unwrap_or("");
        let status = g["status"].as_str().unwrap_or("");
        let listed = opens.iter().any(|x| x["gate"].as_str() == Some(id));
        if status == "pass" && listed {
            errs.push(format!("{id} 是 pass，却出现在 open_gaps 里"));
        }
    }
    for o in opens {
        if o["what"].as_str().is_none_or(str::is_empty) {
            errs.push("open_gaps 里有一条没写 what".into());
        }
        // M11 的 open_gaps 用 `detail` 而不是 `gate` 关联门禁 —— 但细节必须写。
        if o["detail"].as_str().is_none_or(str::is_empty) {
            errs.push("open_gaps 里有一条没写 detail（只写「有个缺口」等于没说）".into());
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

    // ── ⑤ 语料：count 与路径列一致；三态形状与分母 ──
    let n = m["corpus_lists"]["native"]["count"]
        .as_u64()
        .expect("native.count 应是数字");
    if n as usize != selected.len() {
        errs.push(format!(
            "native.count = {n}，但 run-selected.txt 有 {} 条",
            selected.len()
        ));
    }
    for st in ["off", "on", "stress"] {
        let Some(s) = m["corpus_lists"]["native"]["counts"][st].as_str() else {
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

    let chan = &m["corpus_lists"]["chan"];
    let ci = chan["input"]["count"].as_u64().expect("chan input.count");
    let cr = chan["rejected"]["count"]
        .as_u64()
        .expect("chan rejected.count");
    if ci as usize != line_list("tools/run-oracle/chan-input.txt").len() {
        errs.push("chan.input.count 与 chan-input.txt 的行数不一致".into());
    }
    if cr as usize != line_list("tools/run-oracle/chan-rejected.txt").len() {
        errs.push("chan.rejected.count 与 chan-rejected.txt 的行数不一致".into());
    }
    if ci - cr != chan_ok_list.len() as u64 {
        errs.push(format!(
            "chan 的 input − rejected = {}，但算出来 N' 有 {} 条",
            ci - cr,
            chan_ok_list.len()
        ));
    }
    if chan["native_accepted"].as_u64() != Some(chan_ok_list.len() as u64) {
        errs.push("chan.native_accepted 与算出来的 N' 条数不一致".into());
    }
    for st in ["off", "on", "stress"] {
        let Some(s) = chan["counts"][st].as_str() else {
            errs.push(format!("chan.counts 缺 {st}"));
            continue;
        };
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() != 3 || parts[1] != "0" || parts[2] != "0" {
            errs.push(format!("chan.counts.{st} = {s:?} 应是 a/0/0"));
            continue;
        }
        if parts[0].parse::<u64>().ok() != Some(chan_ok_list.len() as u64) {
            errs.push(format!("chan.counts.{st} = {s} 的 ok 数不等于 N' 的条数"));
        }
    }

    // ── ⑥ measured：必填项 + 自洽 ──
    let measured = &m["measured"];
    for key in [
        "workspace",
        "corpus_native_three_states",
        "corpus_chan_three_states",
        "corpus_interp_chan",
        "mutations",
        "three_layer",
        "stack_overflow_depth",
        "many_g",
        "select_waitset",
    ] {
        if measured[key].is_null() {
            errs.push(format!("measured 缺 {key}"));
        }
    }
    let d = &measured["stack_overflow_depth"];
    let (Some(ok), Some(over)) = (d["depth_ok"].as_u64(), d["depth_overflow"].as_u64()) else {
        errs.push("measured.stack_overflow_depth 缺 depth_ok / depth_overflow（数字）".into());
        return errs;
    };
    if over != ok + 1 {
        errs.push(format!(
            "measured.stack_overflow_depth：depth_overflow({over}) 必须等于 depth_ok({ok}) + 1"
        ));
    }
    if d["stack_bytes"].as_u64() != Some(65536) {
        errs.push("measured.stack_overflow_depth.stack_bytes 必须是 65536（D-M11-2）".into());
    }
    if d["slack_bytes"].as_u64() != Some(4096) {
        errs.push("measured.stack_overflow_depth.slack_bytes 必须是 4096（G_STACK_SLACK）".into());
    }
    if d["reproducible"].as_bool() != Some(true) {
        errs.push("measured.stack_overflow_depth.reproducible 必须是 true".into());
    }
    if measured["many_g"]["goroutines"].as_u64() != Some(100_000) {
        errs.push("measured.many_g.goroutines 必须是 100000".into());
    }
    if measured["many_g"]["gdesc_slots"].as_u64() != Some(1024) {
        errs.push("measured.many_g.gdesc_slots 必须是 1024（G_MAX）".into());
    }

    errs
}

#[test]
fn t_manifest_11_真清单必须自洽() {
    let m = read_json("docs/milestones/M11-manifest.json");
    let selected = line_list("tools/run-oracle/run-selected.txt");
    let ok = chan_ok();
    let errs = reconcile(&m, &selected, &ok);
    assert!(
        errs.is_empty(),
        "manifest 对账失败 {} 条：\n{}",
        errs.len(),
        errs.join("\n")
    );
}

/// `measured.corpus_chan_three_states` 必须能**跑出来**（M11 特有）。
///
/// # 这条钉的是什么
///
/// 清单写「off 7/0/0 · on 7/0/0 · stress 7/0/0」。手填的字符串最容易漂：
/// 改了 hchan、改了调度、改了 GC 的触发，清单还是老样子。
///
/// 这里把 7 个 `N'` × 三态**真跑一遍**：
/// * ok 的数等于清单里写的（从清单读，不硬编码 7）；
/// * 三态的 `stdout` / `stderr` / 退出码**逐字节相同**（`T-CORP-05` 的形态）。
///
/// # 为什么三态都不设 `RGOC_GC_STATS`
///
/// 统计行进 stderr，而 `mode=` 三态必然不同 —— 那是**预期**差异，设了这条
/// 判据就只能比较 stdout。清单里的三态读数本来也不含统计行。
#[test]
fn t_manifest_11_并发语料三态必须能复现() {
    let m = read_json("docs/milestones/M11-manifest.json");
    let want: u64 = m["corpus_lists"]["chan"]["counts"]["off"]
        .as_str()
        .and_then(|s| s.split('/').next())
        .and_then(|s| s.parse().ok())
        .expect("chan.counts.off 的 ok 数");
    let ok_list = chan_ok();
    assert_eq!(
        ok_list.len() as u64,
        want,
        "N' 的条数与清单不一致（清单 {want}，算出来 {}）",
        ok_list.len()
    );

    let mut bad: Vec<String> = Vec::new();
    for rel in &ok_list {
        let src = std::fs::read_to_string(repo_root().join(rel))
            .unwrap_or_else(|e| panic!("读不到 `{rel}`：{e}"));
        let slug = rel.rsplit('/').next().unwrap_or(rel).replace(".go", "");
        let mut seen: Option<(String, String, i32)> = None;
        for (tag, env) in [
            ("off", vec![("RGOC_GC", "0")]),
            ("on", vec![]),
            ("stress", vec![("RGOC_GC_STRESS", "8")]),
        ] {
            let out =
                build_and_run_env(&format!("m11m{slug}{tag}"), &src, &env).unwrap_or_else(|e| {
                    panic!("`{rel}` 的 {tag} 态跑不起来（清单说三态都是 ok）：\n{e}")
                });
            if out.exit != 0 {
                bad.push(format!("{rel} [{tag}]：退出码 {}", out.exit));
            }
            match &seen {
                None => seen = Some((out.stdout, out.stderr, out.exit)),
                Some((o, e, x)) => {
                    if out.stdout != *o {
                        bad.push(format!("{rel} [{tag}]：stdout 与 off 态不同"));
                    }
                    if out.stderr != *e {
                        bad.push(format!("{rel} [{tag}]：stderr 与 off 态不同"));
                    }
                    if out.exit != *x {
                        bad.push(format!("{rel} [{tag}]：退出码 {} ≠ off 态 {x}", out.exit));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "三态不可复现：\n{}", bad.join("\n"));
}

/// 把清单改坏一处，断言**有指名道姓的错误**。
fn expect_red(what: &str, mutate: impl FnOnce(&mut Value), needle: &str) {
    let mut m = read_json("docs/milestones/M11-manifest.json");
    mutate(&mut m);
    let selected = line_list("tools/run-oracle/run-selected.txt");
    let ok = chan_ok();
    let errs = reconcile(&m, &selected, &ok);
    assert!(!errs.is_empty(), "改坏「{what}」之后居然还是绿的");
    assert!(
        errs.iter().any(|e| e.contains(needle)),
        "改坏「{what}」后应报出含 {needle:?} 的错误，实际：{errs:?}"
    );
}

#[test]
fn t_manifest_11_合成反例都必须红() {
    expect_red(
        "门禁数不对",
        |m| {
            m["gate"].as_array_mut().unwrap().pop();
        },
        "gate 应有 7 条",
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
        "证据里点名的测试文件不存在",
        |m| {
            m["gate"][0]["evidence"] = Value::String("go_nonexistent.rs 里那几条".into());
        },
        "但盘上没有这个测试文件",
    );

    expect_red(
        "语料计数与路径列不一致",
        |m| {
            m["corpus_lists"]["native"]["count"] = Value::Number(68.into());
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
        "并发语料的 ok 数与算出来的不一致",
        |m| {
            m["corpus_lists"]["chan"]["native_accepted"] = Value::Number(8.into());
        },
        "与算出来的 N' 条数不一致",
    );

    expect_red(
        "溢出深度与 +1 不自洽",
        |m| {
            m["measured"]["stack_overflow_depth"]["depth_overflow"] = Value::Number(500.into());
        },
        "必须等于 depth_ok",
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
        "unsupported 应有 10 条",
    );

    expect_red(
        "measured 缺 many_g 的 G 数",
        |m| {
            m["measured"]["many_g"]["goroutines"] = Value::Null;
        },
        "measured.many_g.goroutines 必须是 100000",
    );

    expect_red(
        "open_gaps 只写了「有个缺口」",
        |m| {
            m["open_gaps"][0]["detail"] = Value::String(String::new());
        },
        "没写 detail",
    );
}
