//! `M8-manifest.json` ↔ 磁盘事实的**对账断言** —— M8/Task 58（`T-CON-60`）。
//!
//! # 为什么要有这一条
//!
//! 沿 `manifest_m7.rs` 的三类谎，再加 M8 特有的两类：
//!
//! | 谎 | 这里怎么拦 |
//! |---|---|
//! | 标 `pass` 却自认未兑现 | `evidence` 里出现「未实现 / 未兑现 / TODO」⇒ 红 |
//! | 非 `pass` 无缺口 | 每条非 `pass` 的门禁必须进 `open_gaps` 或自己有 `gap` 文本 |
//! | 数字自报 | `chan_*` 的数字必须能从 `chan-input.txt` / `chan-rejected.txt` **算出**来 |
//! | 码不在集合 | `unsupported` 里每个 `HIR-*` 码必须 ∈ `RejectKind::ALL`（唯一来源，不抄副本） |
//! | `done` 与全 pass 不同步 | `phase_summary.done` 当且仅当九条门禁全 `pass` |
//!
//! # 骨架期的特殊状态
//!
//! 骨架（Task 57）里 `gate` 九条全是 `pending`、`done: false`。所以对账要接受
//! `pending` 这个状态，并要求**它必须被 open_gaps 认领**（否则「挂着不写理由」
//! 就能一直绿）。收口时（Task 61–66）把它们换成 `pass` 并填 `evidence`。
//!
//! # 断言自己也要能被证伪
//!
//! 末尾十一条**合成反例**：对真 manifest 逐项改坏，每种都必须报出指名道姓的错误。

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

/// `chan-rejected.txt` 的三栏行：`(路径, 类别, 理由)`。
fn rejected_rows() -> Vec<(String, String, String)> {
    line_list("tools/run-oracle/chan-rejected.txt")
        .into_iter()
        .map(|l| {
            let mut it = l.split('\t');
            (
                it.next().unwrap_or("").to_string(),
                it.next().unwrap_or("").to_string(),
                it.next().unwrap_or("").to_string(),
            )
        })
        .collect()
}

/// 对账。返回**全部**问题（不是第一个）—— 一次看到所有不一致才好改。
///
/// 清单由调用方传进来（不是函数内部读磁盘）：这样合成反例可以喂**合成的**清单，
/// 「M' ⊆ 输入清单」「类别合法」这些判据才真的可证伪 —— 否则它们只能对真磁盘
/// 文件恒真。
fn reconcile(m: &Value, input: &[String], rejected: &[(String, String, String)]) -> Vec<String> {
    let mut errs = Vec::new();
    let obj = m.as_object().expect("manifest 顶层应是对象");

    // ① 门禁：9 条，id 唯一且是 CON-1..9；状态三选一
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
        if !matches!(status, "pass" | "partial" | "fail" | "pending") {
            errs.push(format!(
                "{id} 的 status 只能是 pass/partial/fail/pending，实际 {status:?}"
            ));
        }
        // `pending` 是骨架期的占位：不许带 evidence（那会让「挂名」看起来像有据），
        // 也不许不带 gap 之外的理由——见 ②。
        if status == "pending" && g["evidence"].as_str().is_some_and(|s| !s.is_empty()) {
            errs.push(format!("{id} 还是 pending 却已经写了 evidence"));
        }
        // 第一类谎：标 pass 却在证据里自认没做。
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
    let want: BTreeSet<String> = (1..=9).map(|i| format!("CON-{i}")).collect();
    if ids != want {
        errs.push(format!("门禁 id 集合不是 CON-1..9：{ids:?}"));
    }

    // ② 第二类谎：非 pass 的门禁必须被 open_gaps 认领（或自己有 gap 文本）
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
    let rows: Vec<(String, String, String)> = rejected.to_vec();
    let m_rejected: BTreeSet<String> = rows.iter().map(|(p, _, _)| p.clone()).collect();

    let check = |key: &str, actual: usize, errs: &mut Vec<String>| {
        let claimed_n = measured[key].as_u64().unwrap_or(u64::MAX);
        if claimed_n != actual as u64 {
            errs.push(format!(
                "measured.{key} 自报 {claimed_n}，清单算出 {actual}"
            ));
        }
    };
    check("chan_input", input.len(), &mut errs);
    check("chan_rejected", m_rejected.len(), &mut errs);
    check("chan_supported", input.len() - m_rejected.len(), &mut errs);

    // ④ corpus_lists 与 measured / 清单一致
    if m["corpus_lists"]["input"]["count"].as_u64() != Some(input.len() as u64) {
        errs.push("corpus_lists.input.count 与 chan-input.txt 行数不一致".into());
    }
    if m["corpus_lists"]["supported"]["count"].as_u64()
        != Some(input.len() as u64 - m_rejected.len() as u64)
    {
        errs.push("corpus_lists.supported.count 与 chan-input − chan-rejected 不一致".into());
    }
    if m["corpus_lists"]["rejected"]["count"].as_u64() != Some(m_rejected.len() as u64) {
        errs.push("corpus_lists.rejected.count 与 chan-rejected.txt 不一致".into());
    }
    // 三栏：输入 == 清单，M' ⊆ 清单
    let input_set: BTreeSet<String> = input.iter().cloned().collect();
    if input_set.len() != input.len() {
        errs.push("chan-input.txt 有重复行".into());
    }
    for p in &m_rejected {
        if !input_set.contains(p) {
            errs.push(format!("被拒的文件 {p} 不在 chan-input.txt 里"));
        }
    }
    // 每条 M' 必须有类别与理由（`T-CON-54` 的形状）
    for (p, cat, why) in &rows {
        if cat.is_empty() || why.is_empty() {
            errs.push(format!("M' 的 {p} 缺类别或理由"));
        }
        if !matches!(cat.as_str(), "stdlib" | "errorcheck") {
            errs.push(format!(
                "M' 的 {p} 类别 {cat:?} 不在 {{stdlib, errorcheck}} 里"
            ));
        }
    }

    // ⑤ 白名单 8 条，id 唯一；`HIR-*` 码必须 ∈ RejectKind::ALL（**唯一来源**）
    let unsup = m["unsupported"].as_array().expect("unsupported 应是数组");
    if unsup.len() != 8 {
        errs.push(format!("unsupported 应有 8 条，实际 {}", unsup.len()));
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
        let code = u["code"].as_str();
        match code {
            Some(c) if c.starts_with("HIR-") => {
                if !hir.contains(c) {
                    errs.push(format!("{id} 的码 {c} 不在 RejectKind::ALL 里"));
                }
                if u["exit"].as_u64() != Some(1) {
                    errs.push(format!("{id} 有 HIR 码但 exit 不是 1"));
                }
            }
            Some(_) => {
                // 检查面的码（TYP-* / C8）不归 RejectKind 管，但必须写成非空
                if code.is_some_and(str::is_empty) {
                    errs.push(format!("{id} 的 code 是空字符串"));
                }
            }
            None => {
                // 「不存在入口 / 不存在面」：不许有退出码
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
    let want_u: BTreeSet<String> = (1..=8).map(|i| format!("U-M8-{i:02}")).collect();
    if uids != want_u {
        errs.push(format!("unsupported id 集合不是 U-M8-01..08：{uids:?}"));
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
    // `done` 为真却缺报告 ⇒ 红（`R-50-1`）
    if done {
        let report = repo_root().join("docs/milestones/M8-report.md");
        if !report.exists() {
            errs.push("done=true 但 docs/milestones/M8-report.md 不存在（R-50-1）".into());
        }
    }
    // 反向：报告存在但 done=false 也算没写完（骨架期允许，收口时这条会红）
    if !done {
        let report = repo_root().join("docs/milestones/M8-report.md");
        if report.exists() {
            errs.push(
                "M8-report.md 已存在但 phase_summary.done=false —— 要么填完要么删报告".into(),
            );
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
    if m["stage"].as_str() != Some("M8") {
        errs.push("stage 必须是 M8".into());
    }
    errs
}

#[test]
fn 真_manifest_必须自洽() {
    let m = read_json("docs/milestones/M8-manifest.json");
    assert_eq!(m["stage"].as_str(), Some("M8"));
    let input = line_list("tools/run-oracle/chan-input.txt");
    let rejected = rejected_rows();
    let errs = reconcile(&m, &input, &rejected);
    assert!(
        errs.is_empty(),
        "manifest 对账失败 {} 条：\n{}",
        errs.len(),
        errs.join("\n")
    );
}

/// 把 manifest 改坏一处，断言**有指名道姓的错误**。
fn expect_red(what: &str, mutate: impl FnOnce(&mut Value), needle: &str) {
    let mut m = read_json("docs/milestones/M8-manifest.json");
    mutate(&mut m);
    let input = line_list("tools/run-oracle/chan-input.txt");
    let rejected = rejected_rows();
    let errs = reconcile(&m, &input, &rejected);
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
        "gate 应有 9 条",
    );

    expect_red(
        "标 pass 却自认没做",
        |m| {
            m["gate"][0]["status"] = Value::String("pass".into());
            m["gate"][0]["evidence"] = Value::String("未实现".into());
        },
        "标 pass 但证据里有",
    );

    // 非 pass 没人认领 ⇒ 挂着不写理由。
    //
    // ⚠️ 反例要跟着真 manifest 的形状走：收口后九条全是 pass，所以要把某条
    // **改成 partial 再把它的 gap 清空**才构得成矛盾（清 gap 本身不够 ——
    // pass 的门禁不走这条检查）。
    expect_red(
        "partial 既没人认领也没写 gap",
        |m| {
            m["gate"][0]["status"] = Value::String("partial".into());
            m["gate"][0]["gap"] = Value::String(String::new());
            let g = m["open_gaps"].as_array_mut().unwrap();
            g.retain(|x| x["gate"].as_str() != Some("CON-1"));
        },
        "既没进 open_gaps 也没写 gap",
    );

    // pass 却没有 evidence ⇒ 「做完了」不是证据
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
            m["gate"][1]["id"] = Value::String("CON-1".into());
        },
        "门禁 id 重复",
    );

    // 数字自报：把 supported 改大（清单算不出这个数）
    expect_red(
        "measured 自报偏大",
        |m| {
            m["measured"]["chan_supported"] = Value::from(99u64);
        },
        "清单算出",
    );

    // `corpus_lists` 与清单脱节
    expect_red(
        "corpus_lists 与清单不一致",
        |m| {
            m["corpus_lists"]["input"]["count"] = Value::from(20u64);
        },
        "corpus_lists.input.count",
    );

    // 「M' ⊆ 输入清单」与「类别合法」这两条**只能靠合成清单**证伪 ——
    // 真磁盘文件是对的，对它跑多少遍都恒真。所以这里单独喂一份坏清单。
    {
        let m = read_json("docs/milestones/M8-manifest.json");
        let input = line_list("tools/run-oracle/chan-input.txt");
        let bad = vec![(
            "go_source_code/test/chan/不存在.go".to_string(),
            "stdlib".to_string(),
            "合成：这个文件不在 input 里".to_string(),
        )];
        let errs = reconcile(&m, &input, &bad);
        assert!(
            errs.iter().any(|e| e.contains("不在 chan-input.txt 里")),
            "喂一个不在清单里的被拒文件应当报错，实际：{errs:?}"
        );
        // 类别非法
        let bad2 = vec![(input[0].clone(), "鬼类别".to_string(), "合成".to_string())];
        let errs2 = reconcile(&m, &input, &bad2);
        assert!(
            errs2
                .iter()
                .any(|e| e.contains("类别") && e.contains("不在")),
            "喂一个非法类别应当报错，实际：{errs2:?}"
        );
        // 缺理由
        let bad3 = vec![(input[0].clone(), "stdlib".to_string(), String::new())];
        let errs3 = reconcile(&m, &input, &bad3);
        assert!(
            errs3.iter().any(|e| e.contains("缺类别或理由")),
            "喂一个缺理由的行应当报错，实际：{errs3:?}"
        );
    }

    expect_red(
        "unsupported 少一条",
        |m| {
            m["unsupported"].as_array_mut().unwrap().pop();
        },
        "unsupported 应有 8 条",
    );

    expect_red(
        "码不在码表里",
        |m| {
            m["unsupported"][2]["code"] = Value::String("HIR-NOT-A-CODE".into());
        },
        "不在 RejectKind::ALL",
    );

    // `done` 与「九条门禁全 pass」必须**同真同假**。
    //
    // ⚠️ 反例要跟着真 manifest 的形状走：收口后九条全 pass 且 done 已经是
    // true，所以「把 done 设成 true」是恒真的改坏。真正的矛盾是「把一条门禁
    // 拉下来却还留着 done=true」—— 那才是这条断言要抓的。
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
