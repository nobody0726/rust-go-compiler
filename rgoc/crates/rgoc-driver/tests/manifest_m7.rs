//! `M7-manifest.json` ↔ 磁盘事实的**对账断言** —— M7/Task 59。
//!
//! # 为什么要有这一条
//!
//! manifest 是「机器可读的事实」，而事实必须是**可验证**的，不是自报。M5 的教训
//! （`M5-report.md` §5）：`manifest.measured` 是自报，收口实跑才发现 `cargo fmt`
//! 本来是红的 —— `cargo test` 全绿**不覆盖** fmt。
//!
//! M6 的教训（`R-50-1`）：`done: true` 但报告文件不存在，也没人发现。
//!
//! 所以这条测试做三件事：
//!
//! | 谎 | 这里怎么拦 |
//! |---|---|
//! | 标 `pass` 却自认未兑现 | `evidence` 里出现「未实现 / 未兑现 / TODO」⇒ 红 |
//! | 非 `pass` 无缺口 | 每条非 `pass` 的门禁必须在 `open_gaps` 里被点名（或自己有 `gap` 文本） |
//! | 数字自报 | `native_*` 的数字必须能从 `native-input.txt` / `native-rejected.txt` **算出**来 |
//!
//! # 断言自己也要能被证伪
//!
//! 末尾有一条**合成反例**测试：对真 manifest 逐项改坏，每种都必须报出指名道姓的
//! 错误。只有「真文件 ⇒ Ok」的断言是恒真的近亲。

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

/// 对账。返回**全部**问题（不是第一个）—— 一次看到所有不一致才好改。
fn reconcile(m: &Value) -> Vec<String> {
    let mut errs = Vec::new();
    let obj = m.as_object().expect("manifest 顶层应是对象");

    // ① 门禁：9 条，id 唯一且是 NAT-1..9
    let gate = m["gate"].as_array().expect("gate 应是数组");
    if gate.len() != 9 {
        errs.push(format!("gate 应有 9 条，实际 {}", gate.len()));
    }
    let mut ids = BTreeSet::new();
    for g in gate {
        let id = g["id"].as_str().unwrap_or("<无 id>").to_string();
        if !ids.insert(id.clone()) {
            errs.push(format!("门禁 id 重复：{id}"));
        }
        let status = g["status"].as_str().unwrap_or("<无 status>");
        if !matches!(status, "pass" | "partial" | "fail") {
            errs.push(format!(
                "{id} 的 status 只能是 pass/partial/fail，实际 {status:?}"
            ));
        }
        let ev = g["evidence"].as_str().unwrap_or("");
        if ev.is_empty() {
            errs.push(format!("{id} 没有 evidence —— 「做完了」不是证据"));
        }
        // 第一类谎：标 pass 却在证据里自认没做。
        if status == "pass" {
            for bad in ["未实现", "未兑现", "TODO", "没有实现"] {
                if ev.contains(bad) {
                    errs.push(format!("{id} 标 pass 但证据里有「{bad}」"));
                }
            }
        }
    }
    let want: BTreeSet<String> = (1..=9).map(|i| format!("NAT-{i}")).collect();
    if ids != want {
        errs.push(format!("门禁 id 集合不是 NAT-1..9：{ids:?}"));
    }

    // ② 第二类谎：非 pass 的门禁必须在 open_gaps 里被点名（或自己有 gap 文本）
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

    // ③ 数字必须能从清单算出来
    let measured = &m["measured"];
    let rejected = line_list("tools/run-oracle/native-rejected.txt");
    let input = line_list("tools/run-oracle/native-input.txt");
    let m_rejected: BTreeSet<String> = rejected
        .iter()
        .filter_map(|l| l.split('\t').next())
        .map(ToString::to_string)
        .collect();

    let check = |key: &str, actual: usize, errs: &mut Vec<String>| {
        let claimed_n = measured[key].as_u64().unwrap_or(u64::MAX);
        if claimed_n != actual as u64 {
            errs.push(format!(
                "measured.{key} 自报 {claimed_n}，清单算出 {actual}"
            ));
        }
    };
    check("native_input", input.len(), &mut errs);
    check("native_rejected", m_rejected.len(), &mut errs);
    check(
        "native_supported",
        input.len() - m_rejected.len(),
        &mut errs,
    );

    // ④ corpus_lists 与 measured 一致
    if m["corpus_lists"]["supported"]["count"].as_u64()
        != Some(input.len() as u64 - m_rejected.len() as u64)
    {
        errs.push("corpus_lists.supported.count 与 native-input − native-rejected 不一致".into());
    }
    if m["corpus_lists"]["rejected"]["count"].as_u64() != Some(m_rejected.len() as u64) {
        errs.push("corpus_lists.rejected.count 与 native-rejected.txt 不一致".into());
    }

    // ⑤ 白名单 4 条，id 唯一，码要么是 CgCode 要么是运行期 panic 文案
    let unsup = m["unsupported"].as_array().expect("unsupported 应是数组");
    if unsup.len() != 4 {
        errs.push(format!("unsupported 应有 4 条，实际 {}", unsup.len()));
    }
    let cg: BTreeSet<&str> = rgoc_arm64::diag::CgCode::ALL
        .iter()
        .map(|c| c.as_str())
        .collect();
    for u in unsup {
        let code = u["code"].as_str().unwrap_or("");
        if code.starts_with("CG-") && !cg.contains(code) {
            errs.push(format!("码 {code} 不在 CgCode::ALL 里"));
        }
        if u["test_id"].as_str().unwrap_or("").is_empty() {
            errs.push(format!("{} 没有 test_id", u["id"].as_str().unwrap_or("?")));
        }
    }

    // ⑥ `done` 当且仅当九条门禁全 pass
    let done = m["phase_summary"]["done"].as_bool().unwrap_or(false);
    let all_pass = gate.iter().all(|g| g["status"].as_str() == Some("pass"));
    if done != all_pass {
        errs.push(format!(
            "phase_summary.done={done} 但九条门禁{}全 pass",
            if all_pass { "已经" } else { "没有" }
        ));
    }
    // ③ 的报告没有 `done` 也一样：`done` 为真却缺报告 ⇒ 红（`R-50-1`）
    if done {
        let report = repo_root().join("docs/milestones/M7-report.md");
        if !report.exists() {
            errs.push("done=true 但 docs/milestones/M7-report.md 不存在（R-50-1）".into());
        }
    }

    // ⑦ 顶层必备字段
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
    errs
}

#[test]
fn 真_manifest_必须自洽() {
    let m = read_json("docs/milestones/M7-manifest.json");
    assert_eq!(m["stage"].as_str(), Some("M7"));
    let errs = reconcile(&m);
    assert!(
        errs.is_empty(),
        "manifest 对账失败 {} 条：\n{}",
        errs.len(),
        errs.join("\n")
    );
}

/// 把 manifest 改坏一处，断言**有指名道姓的错误**。
fn expect_red(what: &str, mutate: impl FnOnce(&mut Value), needle: &str) {
    let mut m = read_json("docs/milestones/M7-manifest.json");
    mutate(&mut m);
    let errs = reconcile(&m);
    assert!(!errs.is_empty(), "改坏「{what}」之后居然还是绿的");
    assert!(
        errs.iter().any(|e| e.contains(needle)),
        "改坏「{what}」后应报出含 {needle:?} 的错误，实际：{errs:?}"
    );
}

#[test]
fn 合成反例八种都必须红() {
    expect_red(
        "门禁数不对",
        |m| {
            m["gate"].as_array_mut().unwrap().pop();
        },
        "gate 应有 9 条",
    );

    expect_red(
        "标 pass 却自认未做",
        |m| {
            m["gate"][0]["status"] = Value::String("pass".into());
            m["gate"][0]["evidence"] = Value::String("未实现".into());
        },
        "标 pass 但证据里有",
    );

    expect_red(
        "partial 没人认领",
        |m| {
            m["gate"][0]["status"] = Value::String("partial".into());
            m["gate"][0].as_object_mut().unwrap().remove("gap");
            // 同时把 open_gaps 里指向 NAT-1 的都删掉
            let g = m["open_gaps"].as_array_mut().unwrap();
            g.retain(|x| x["gate"].as_str() != Some("NAT-1"));
        },
        "既没进 open_gaps 也没写 gap",
    );

    expect_red(
        "门禁 id 重复",
        |m| {
            m["gate"][1]["id"] = Value::String("NAT-1".into());
        },
        "门禁 id 重复",
    );

    expect_red(
        "measured 自报偏大",
        |m| {
            m["measured"]["native_supported"] = Value::from(99u64);
        },
        "清单算出",
    );

    expect_red(
        "unsupported 少一条",
        |m| {
            m["unsupported"].as_array_mut().unwrap().pop();
        },
        "unsupported 应有 4 条",
    );

    expect_red(
        "码不在码表里",
        |m| {
            m["unsupported"][0]["code"] = Value::String("CG-NOT-A-CODE".into());
        },
        "不在 CgCode::ALL",
    );

    expect_red(
        "done 但门禁没全过",
        |m| {
            m["phase_summary"]["done"] = Value::Bool(true);
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
