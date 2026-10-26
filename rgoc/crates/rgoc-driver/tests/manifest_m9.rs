//! `M9-manifest.json` ↔ 磁盘事实的**对账断言** —— M9/Task 69。
//!
//! # 为什么要有这一条
//!
//! 沿 `manifest_m8.rs` 的五类谎，再加 M9 特有的一类：
//!
//! | 谎 | 这里怎么拦 |
//! |---|---|
//! | 标 `pass` 却自认未兑现 | `evidence` 里出现「未实现 / 未兑现 / TODO」⇒ 红 |
//! | 非 `pass` 无缺口 | 每条非 `pass` 的门禁必须进 `open_gaps` 或自己有 `gap` 文本 |
//! | 数字自报 | 语料分母、码表条数、`hchan` 尺寸都必须能从**磁盘/常量**算出来 |
//! | 码不在集合 | `unsupported` 里的 `HIR-*` 码必须 ∈ `RejectKind::ALL`（唯一来源） |
//! | `done` 与全 pass 不同步 | `phase_summary.done` 当且仅当十条门禁全 `pass` |
//! | **证据指向不存在的测试** | `evidence` 里出现的每个 `<名字>.rs` 必须在某个 `tests/` 目录下真实存在 |
//!
//! 最后一类是 M9 特有的：这一轮的报告与 manifest 里，「证据」大量写成
//! 「`xxx.rs` 的 N 条」。写错文件名不会有任何编译错，而读者会以为有据可查。
//! M9 收口时实测：`obj_iface.rs` / `obj_funcval.rs` **不存在**（那两面的证据
//! 其实是语料），靠这一条才没把假文件名留在 manifest 里。
//!
//! # 断言自己也要能被证伪
//!
//! 末尾十四条**合成反例**：对真 manifest 逐项改坏，每种都必须报出指名道姓的错误。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达")
}

fn read_json(rel: &str) -> Value {
    let p = repo_root().join(rel);
    let text =
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} 不是合法 JSON：{e}", p.display()))
}

/// 清单文件的**路径列**（跳过空行与 `#` 注释）。
fn line_list(rel: &str) -> Vec<String> {
    let p = repo_root().join(rel);
    let text =
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()));
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(ToString::to_string)
        .collect()
}

/// `C11` 里被标记包住的那张表的总大小行。
fn c11_total(tag: &str) -> usize {
    let path = repo_root().join("docs/contracts/C11-concurrency.md");
    let text = std::fs::read_to_string(&path).expect("读不到 C11");
    let body = text
        .split(&format!("<!-- {tag}:begin -->"))
        .nth(1)
        .unwrap_or_else(|| panic!("C11 里找不到 {tag}"))
        .split(&format!("<!-- {tag}:end -->"))
        .next()
        .expect("有 begin 就该有 end");
    for line in body.lines() {
        let cells: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.len() >= 3 && cells[2].starts_with("_（对象总大小）_") {
            return cells[0].parse().expect("总大小是数字");
        }
    }
    panic!("C11 的 {tag} 表里没有「对象总大小」行");
}

/// `evidence` / `what` 里提到的 `<名字>.rs` 是否真实存在（在任一 `tests/` 目录下）。
fn mentioned_test_files(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i + 3 < bytes.len() {
        // 找 `.rs`，往前回溯到标识符边界
        if &bytes[i..i + 3] == b".rs" && bytes[i + 3] != b'a'
        // 排除 `.rs` 后面接字母（如 `.rsbuild`）
        {
            let mut j = i;
            while j > 0 && (bytes[j - 1].is_ascii_alphanumeric() || bytes[j - 1] == b'_') {
                j -= 1;
            }
            let name = &text[j..i + 3];
            if name.len() > 3 {
                out.push(name.to_string());
            }
            i += 3;
            continue;
        }
        i += 1;
    }
    out
}

/// 所有 `tests/` 目录下的 `.rs` 文件名集合。
fn test_files_on_disk() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![repo_root().join("rgoc/crates")];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        // 当前目录本身叫不叫 `tests` —— 决定它的**直接**子 `.rs` 算不算测试文件
        let in_tests = dir.file_name().is_some_and(|n| n == "tests");
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if in_tests
                && let Some(name) = p.file_name().and_then(|n| n.to_str())
                && name.ends_with(".rs")
            {
                out.insert(name.to_string());
            }
        }
    }
    out
}

/// 对账。返回**全部**问题（不是第一个）—— 一次看到所有不一致才好改。
///
/// 清单由调用方传进来：这样合成反例可以喂**合成的**清单，判据才真的可证伪。
fn reconcile(m: &Value, selected: &[String], ssa_rejected: &[String]) -> Vec<String> {
    let mut errs = Vec::new();
    let obj = m.as_object().expect("manifest 顶层应是对象");

    // ① 门禁：10 条，id 唯一且是那十个
    let want: BTreeSet<String> = [
        "MEM-1", "MEM-2", "OBJ-1", "OBJ-2", "OBJ-3", "OBJ-4", "CORP-1", "EXC-1", "ROOT-1", "SCH-1",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
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
        }
    }
    if ids != want {
        errs.push(format!("门禁 id 集合不对：{ids:?}"));
    }

    // ② 非 pass 的门禁必须被 open_gaps 认领（或自己有 gap 文本）
    let gaps = m["open_gaps"].as_array().expect("open_gaps 应是数组");
    let claimed: BTreeSet<String> = gaps
        .iter()
        .filter_map(|g| g["gate"].as_str().map(ToString::to_string))
        .collect();
    for g in gate {
        let status = g["status"].as_str().unwrap_or("");
        if status == "pass" {
            continue;
        }
        let id = g["id"].as_str().unwrap_or("");
        let has_gap_text = g["gap"].as_str().is_some_and(|s| !s.is_empty() && s != "—");
        if !claimed.contains(id) && !has_gap_text {
            errs.push(format!("{id} 是 {status} 但既没进 open_gaps 也没写 gap"));
        }
    }

    // ③ 数字必须能从磁盘/常量算出来
    let measured = &m["measured"];
    let check = |key: &str, actual: usize, errs: &mut Vec<String>| {
        let claimed_n = measured[key].as_u64().unwrap_or(u64::MAX);
        if claimed_n != actual as u64 {
            errs.push(format!(
                "measured.{key} 自报 {claimed_n}，磁盘算出 {actual}"
            ));
        }
    };
    check("corpus_n", selected.len() - ssa_rejected.len(), &mut errs);
    check("corpus_m", ssa_rejected.len(), &mut errs);
    check("ssa_codes", rgoc_ssa::diag::SsaCode::ALL.len(), &mut errs);
    check(
        "hir_reject_codes",
        rgoc_hir::diag::RejectKind::ALL.len(),
        &mut errs,
    );
    check(
        "hchan_size",
        core::mem::size_of::<rgoc_arm64::hchan::Hchan>(),
        &mut errs,
    );
    check(
        "sudog_size",
        core::mem::size_of::<rgoc_arm64::hchan::Sudog>(),
        &mut errs,
    );
    // 与 `C11` 的两张表也要一致（「尺寸」只有一个来源）
    if measured["hchan_size"].as_u64() != Some(c11_total("hchan-layout") as u64) {
        errs.push(format!(
            "measured.hchan_size 与 C11 §7 的总大小 {} 不一致",
            c11_total("hchan-layout")
        ));
    }
    if measured["sudog_size"].as_u64() != Some(c11_total("sudog-layout") as u64) {
        errs.push(format!(
            "measured.sudog_size 与 C11 §7 的总大小 {} 不一致",
            c11_total("sudog-layout")
        ));
    }
    // 失败数必须是 0（收口之后）
    if measured["workspace_failed"].as_u64() != Some(0) {
        errs.push("measured.workspace_failed 不是 0".into());
    }
    // 四套语料的三栏必须自洽
    for (ok, skip, bad) in [
        ("native_ok", "native_skip", "native_bad"),
        ("collide_run_green", "collide_run_skip", "collide_run_red"),
        ("collide_ssa_green", "collide_ssa_skip", "collide_ssa_red"),
    ] {
        let sum = measured[ok].as_u64().unwrap_or(0)
            + measured[skip].as_u64().unwrap_or(0)
            + measured[bad].as_u64().unwrap_or(0);
        if sum != selected.len() as u64 {
            errs.push(format!(
                "{ok}/{skip}/{bad} 三栏之和 {sum} ≠ 入选语料 {}",
                selected.len()
            ));
        }
        if measured[bad].as_u64() != Some(0) {
            errs.push(format!("{bad} 不是 0 —— 收口后不该有坏的"));
        }
    }

    // ④ corpus_lists 与清单一致
    let cl = &m["corpus_lists"];
    if cl["input"]["count"].as_u64() != Some(selected.len() as u64) {
        errs.push("corpus_lists.input.count 与 run-selected.txt 行数不一致".into());
    }
    if cl["buildable"]["count"].as_u64() != Some((selected.len() - ssa_rejected.len()) as u64) {
        errs.push("corpus_lists.buildable.count ≠ 入选 − 被拒".into());
    }
    if cl["rejected"]["count"].as_u64() != Some(ssa_rejected.len() as u64) {
        errs.push("corpus_lists.rejected.count 与 ssa-rejected.txt 行数不一致".into());
    }
    if selected.len() != selected.iter().collect::<BTreeSet<_>>().len() {
        errs.push("run-selected.txt 有重复行".into());
    }
    let sel_set: BTreeSet<&String> = selected.iter().collect();
    for p in ssa_rejected {
        if !sel_set.contains(p) {
            errs.push(format!("被拒的文件 {p} 不在 run-selected.txt 里"));
        }
    }

    // ⑤ 白名单 10 条；`HIR-*` 码必须 ∈ RejectKind::ALL（唯一来源）
    let unsup = m["unsupported"].as_array().expect("unsupported 应是数组");
    if unsup.len() != 10 {
        errs.push(format!("unsupported 应有 10 条，实际 {}", unsup.len()));
    }
    let hir: BTreeSet<&str> = rgoc_hir::diag::RejectKind::ALL
        .iter()
        .map(|c| c.code())
        .collect();
    let mut uids = BTreeSet::new();
    for u in unsup {
        let id = u["id"].as_str().unwrap_or("<无 id>").to_string();
        if !uids.insert(id.clone()) {
            errs.push(format!("unsupported id 重复：{id}"));
        }
        match u["code"].as_str() {
            Some(c) if c.starts_with("HIR-") => {
                if !hir.contains(c) {
                    errs.push(format!("{id} 的码 {c} 不在 RejectKind::ALL 里"));
                }
                if u["exit"].as_u64() != Some(1) {
                    errs.push(format!("{id} 有 HIR 码但 exit 不是 1"));
                }
            }
            Some(_) => {}
            None => {
                if u["exit"].as_u64().is_some() {
                    errs.push(format!("{id} 没有码却写了 exit"));
                }
            }
        }
        if u["what"].as_str().unwrap_or("").is_empty() {
            errs.push(format!("{id} 没有 what"));
        }
        if u["evidence"].as_str().unwrap_or("").is_empty() {
            errs.push(format!("{id} 没有 evidence —— 白名单项也要有据"));
        }
    }
    let want_u: BTreeSet<String> = (1..=10).map(|i| format!("U-M9-{i:02}")).collect();
    if uids != want_u {
        errs.push(format!("unsupported id 集合不是 U-M9-01..10：{uids:?}"));
    }

    // ⑥ **证据里提到的测试文件必须存在**（M9 特有的一类谎）
    let disk = test_files_on_disk();
    let mut mentioned = BTreeSet::new();
    for g in gate {
        if let Some(ev) = g["evidence"].as_str() {
            for f in mentioned_test_files(ev) {
                mentioned.insert(f);
            }
        }
    }
    for u in unsup {
        if let Some(ev) = u["evidence"].as_str() {
            for f in mentioned_test_files(ev) {
                mentioned.insert(f);
            }
        }
    }
    for f in &mentioned {
        if !disk.contains(f) {
            errs.push(format!(
                "evidence 里提到的 `{f}` 在磁盘上不存在（编造的证据）"
            ));
        }
    }
    if mentioned.len() < 8 {
        errs.push(format!(
            "evidence 只提到 {} 个测试文件 —— 太少了，说明证据写得不够具体",
            mentioned.len()
        ));
    }

    // ⑦ `done` 当且仅当十条门禁全 pass；且与报告的存在性同步（`R-50-1`）
    let done = m["phase_summary"]["done"].as_bool().unwrap_or(false);
    let all_pass = gate.iter().all(|g| g["status"].as_str() == Some("pass"));
    if done != all_pass {
        errs.push(format!(
            "phase_summary.done={done} 但十条门禁{}全 pass",
            if all_pass { "已经" } else { "没有" }
        ));
    }
    let report = repo_root().join("docs/milestones/M9-report.md");
    if done && !report.exists() {
        errs.push("done=true 但 docs/milestones/M9-report.md 不存在（R-50-1）".into());
    }
    if !done && report.exists() {
        errs.push("M9-report.md 已存在但 phase_summary.done=false".into());
    }

    // ⑧ 顶层必备字段
    for key in [
        "stage",
        "name",
        "fill_task",
        "docs",
        "crates",
        "corpus_lists",
        "gate",
        "unsupported",
        "measured",
        "open_gaps",
        "phase_summary",
    ] {
        if obj.get(key).is_none() {
            errs.push(format!("manifest 缺顶层字段 {key}"));
        }
    }
    if m["stage"].as_str() != Some("M9") {
        errs.push("stage 必须是 M9".into());
    }
    errs
}

#[test]
fn 真_manifest_必须自洽() {
    let m = read_json("docs/milestones/M9-manifest.json");
    assert_eq!(m["stage"].as_str(), Some("M9"));
    let selected = line_list("tools/run-oracle/run-selected.txt");
    let rejected = line_list("tools/run-oracle/ssa-rejected.txt");
    let errs = reconcile(&m, &selected, &rejected);
    assert!(
        errs.is_empty(),
        "manifest 对账失败 {} 条：\n{}",
        errs.len(),
        errs.join("\n")
    );
}

/// 把 manifest 改坏一处，断言**有指名道姓的错误**。
fn expect_red(what: &str, mutate: impl FnOnce(&mut Value), needle: &str) {
    let mut m = read_json("docs/milestones/M9-manifest.json");
    mutate(&mut m);
    let selected = line_list("tools/run-oracle/run-selected.txt");
    let rejected = line_list("tools/run-oracle/ssa-rejected.txt");
    let errs = reconcile(&m, &selected, &rejected);
    assert!(!errs.is_empty(), "改坏「{what}」之后居然还是绿的");
    assert!(
        errs.iter().any(|e| e.contains(needle)),
        "改坏「{what}」后应报出含 {needle:?} 的错误，实际：{errs:?}"
    );
}

#[test]
fn 合成反例都必须红() {
    expect_red(
        "门禁数不对",
        |m| {
            m["gate"].as_array_mut().unwrap().pop();
        },
        "gate 应有 10 条",
    );

    expect_red(
        "标 pass 却自认没做",
        |m| {
            m["gate"][0]["status"] = Value::String("pass".into());
            m["gate"][0]["evidence"] = Value::String("未实现".into());
        },
        "标 pass 但证据里有",
    );

    expect_red(
        "partial 既没人认领也没写 gap",
        |m| {
            m["gate"][0]["status"] = Value::String("partial".into());
            m["gate"][0]["gap"] = Value::String(String::new());
            let g = m["open_gaps"].as_array_mut().unwrap();
            g.retain(|x| x["gate"].as_str() != Some("MEM-1"));
        },
        "既没进 open_gaps 也没写 gap",
    );

    expect_red(
        "pass 却没有 evidence",
        |m| {
            m["gate"][0]["evidence"] = Value::String(String::new());
        },
        "没有 evidence",
    );

    expect_red(
        "门禁 id 重复",
        |m| {
            m["gate"][1]["id"] = Value::String("MEM-1".into());
        },
        "门禁 id 重复",
    );

    // 数字自报：语料分母改大（磁盘算不出这个数）
    expect_red(
        "measured 自报偏大",
        |m| {
            m["measured"]["corpus_n"] = Value::from(99u64);
        },
        "磁盘算出",
    );

    // 三栏之和必须等于入选语料数
    expect_red(
        "三栏之和不对",
        |m| {
            m["measured"]["native_ok"] = Value::from(99u64);
        },
        "三栏之和",
    );

    expect_red(
        "有坏的不报",
        |m| {
            m["measured"]["native_bad"] = Value::from(1u64);
            m["measured"]["native_ok"] = Value::from(68u64);
        },
        "不该有坏的",
    );

    expect_red(
        "corpus_lists 与清单不一致",
        |m| {
            m["corpus_lists"]["input"]["count"] = Value::from(70u64);
        },
        "corpus_lists.input.count",
    );

    expect_red(
        "码表条数自报",
        |m| {
            m["measured"]["ssa_codes"] = Value::from(9u64);
        },
        "measured.ssa_codes",
    );

    // `hchan` 尺寸与 C11 的表不一致
    expect_red(
        "hchan 尺寸与 C11 不符",
        |m| {
            m["measured"]["hchan_size"] = Value::from(88u64);
        },
        "C11 §7 的总大小",
    );

    expect_red(
        "unsupported 少一条",
        |m| {
            m["unsupported"].as_array_mut().unwrap().pop();
        },
        "unsupported 应有 10 条",
    );

    expect_red(
        "码不在码表里",
        |m| {
            m["unsupported"][4]["code"] = Value::String("HIR-NOT-A-CODE".into());
        },
        "不在 RejectKind::ALL",
    );

    // M9 特有的那类谎：证据指向不存在的测试文件
    expect_red(
        "证据指向不存在的测试",
        |m| {
            m["gate"][4]["evidence"] =
                Value::String("obj_iface.rs 的 5 条（这个文件从来不存在）".into());
        },
        "在磁盘上不存在",
    );

    expect_red(
        "门禁没全过却自称 done",
        |m| {
            m["gate"][0]["status"] = Value::String("partial".into());
            m["gate"][0]["gap"] = Value::String("合成：故意拉下来一条".into());
        },
        "phase_summary.done=true",
    );

    expect_red(
        "顶层缺字段",
        |m| {
            m.as_object_mut().unwrap().remove("measured");
        },
        "缺顶层字段 measured",
    );
}
