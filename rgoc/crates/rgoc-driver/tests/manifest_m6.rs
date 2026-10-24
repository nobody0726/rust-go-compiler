//! `M6-manifest.json` ↔ `M6-tests.md` 的**对账断言** —— M6/Task 50（`T-SSA-44`）。
//!
//! # 为什么要有这一条
//!
//! manifest 是「机器可读的事实」，`M6-tests.md` 是「冻结的约定」，磁盘上的清单是
//! 「第三方证据」。三者**同源但不自动同步** —— 没有断言的话，改一处忘了另一处只会
//! 留下几份互相矛盾的文件，谁也不知道哪份算数。
//!
//! # 三类谎必须被拦（`T-SSA-44` 的原话）
//!
//! | 谎 | 长什么样 | 这里怎么拦 |
//! |---|---|---|
//! | 标 `pass` 却自认未兑现 | `status: "pass"` + `evidence: "未实现…"` | 证据文本里出现「未实现 / 未兑现 / TODO」⇒ 红 |
//! | 非 `pass` 无缺口 | `status: "partial"` 但 `open_gaps` 里没人认领 | 非 `pass` 的门禁 / 测试必须在 `open_gaps` 里被**同时**点名 |
//! | 未全绿自称 `done` | `workspace_failed > 0` 而 `phase_summary.status == "done"` | `done` **当且仅当**九条门禁全 `pass` 且 `failed == 0` |
//!
//! # 断言自己也要能被证伪
//!
//! 只有「真文件 ⇒ Ok」的断言是恒真的近亲（把检查写成空函数就永远绿）。所以下面
//! 还有一条**合成反例**测试：对真 manifest 逐项改坏（8 种），每种都必须报出
//! **指名道姓**的错误。其中两种改的是**文档**而不是 manifest —— 证明解析器真的
//! 在读 `M6-tests.md`，而不是把 64 / 9 写死在代码里。
//!
//! # 依赖
//!
//! `serde_json` 是 `rgoc-driver` **已有**的依赖（M5 就为 `manifest_m5.rs` 引的），
//! 不是为这条测试新加的（`AGENTS.md` §4.3）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达（cargo 的 CARGO_MANIFEST_DIR 变了？）")
}

fn manifest_path() -> PathBuf {
    repo_root().join("docs/milestones/M6-manifest.json")
}

fn tests_doc_path() -> PathBuf {
    repo_root().join("docs/milestones/M6-tests.md")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("读不到 {}：{e}", path.display()))
}

/// 取 `from` 到 `to` 之间的正文（两端标题都必须存在，缺一个就响亮失败）。
fn section<'a>(doc: &'a str, from: &str, to: &str) -> &'a str {
    let s = doc
        .find(from)
        .unwrap_or_else(|| panic!("测试文档里找不到章节 {from:?}"));
    let rest = &doc[s..];
    let e = rest
        .find(to)
        .unwrap_or_else(|| panic!("测试文档里找不到 {to:?}（{from:?} 的下一节）"));
    &rest[..e]
}

/// §3 各节表第一列声明的 ID（`| \`T-SSA-11\` | …`）。
///
/// 只认行首形态：ID 出现在别的列（例如判据里引用 `T-SSA-09/18`）算**引用**，不算声明。
fn declared_ids(doc: &str) -> BTreeSet<String> {
    let sec = section(doc, "\n## 3", "\n## 4");
    let mut out = BTreeSet::new();
    for line in sec.lines() {
        let Some(rest) = line.strip_prefix("| `") else {
            continue;
        };
        let Some(id) = rest.split('`').next() else {
            continue;
        };
        if id.starts_with("T-SSA-") {
            out.insert(id.to_string());
        }
    }
    out
}

/// §5 白名单的 `U-M6-xx`（整节扫：子表也算）。
fn declared_unsupported(doc: &str) -> BTreeSet<String> {
    let sec = section(doc, "\n## 5", "\n## 6");
    let mut out = BTreeSet::new();
    let mut i = 0;
    while let Some(p) = sec[i..].find("U-M6-") {
        let start = i + p;
        let mut j = start + "U-M6-".len();
        while sec.as_bytes().get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
        if j > start + "U-M6-".len() {
            out.insert(sec[start..j].to_string());
        }
        i = j.max(start + 1);
    }
    out
}

/// §7 门禁表的 `SSA-x`。
fn declared_gates(doc: &str) -> BTreeSet<String> {
    let sec = section(doc, "\n## 7", "\n## 8");
    let mut out = BTreeSet::new();
    for line in sec.lines() {
        let Some(rest) = line.strip_prefix("| `SSA-") else {
            continue;
        };
        let Some(n) = rest.split('`').next() else {
            continue;
        };
        out.insert(format!("SSA-{n}"));
    }
    out
}

fn strs(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|x| x.as_str().unwrap_or("").to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn nonempty(v: &Value, key: &str) -> bool {
    v.get(key)
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty())
}

/// 清单文件的非注释非空行数。
fn list_lines(rel: &str) -> usize {
    read(&repo_root().join(rel))
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .count()
}

/// 全部对账。返回第一条错误（带「哪一项」前缀，便于定位）。
fn check(v: &Value, doc: &str) -> Result<(), String> {
    // ── ① 顶层结构
    for k in [
        "stage",
        "name",
        "generated_at",
        "filled_at",
        "status",
        "scope",
        "docs",
        "crates",
        "corpus_lists",
        "gate",
        "test_ids",
        "unsupported",
        "open_gaps",
        "closed_gaps",
        "measured",
        "phase_summary",
    ] {
        if v.get(k).is_none() {
            return Err(format!("structure: 顶层缺 `{k}`"));
        }
    }
    if v["stage"].as_str() != Some("M6") {
        return Err(format!("stage: 应为 M6，实际 {:?}", v["stage"]));
    }
    if v["filled_at"].is_null() {
        return Err("structure: filled_at 仍是 null（骨架没填）".into());
    }
    for k in ["design", "tests", "plan", "report"] {
        if !nonempty(&v["docs"], k) {
            return Err(format!("structure: docs.{k} 空"));
        }
    }
    let crates = strs(v, "crates");
    if crates.len() < 5 || crates.iter().any(|c| c.trim().is_empty()) {
        return Err("structure: crates 至少 5 条且都不为空".into());
    }

    // ── ② 语料计数：与磁盘**同时**相等，且 N + M == 69
    let cl = &v["corpus_lists"];
    let cand = cl["candidate"]["count"].as_u64();
    if cand != Some(69) || list_lines("tools/run-oracle/run-selected.txt") != 69 {
        return Err(format!(
            "corpus_lists: candidate 应为 69（清单 {} 行，manifest {:?}）",
            list_lines("tools/run-oracle/run-selected.txt"),
            cand
        ));
    }
    let nb = cl["buildable"]["count"].as_u64();
    let nr = cl["rejected"]["count"].as_u64();
    if nb != Some(46) {
        return Err(format!("corpus_lists: buildable 应为 46，实际 {nb:?}"));
    }
    if nr != Some(23) {
        return Err(format!("corpus_lists: rejected 应为 23，实际 {nr:?}"));
    }
    let rej_file = cl["rejected"]["file"].as_str().unwrap_or("");
    if rej_file != "tools/run-oracle/ssa-rejected.txt" {
        return Err(format!(
            "corpus_lists: rejected.file 应为 ssa-rejected.txt，实际 {rej_file:?}"
        ));
    }
    if list_lines(rej_file) != 23 {
        return Err(format!(
            "corpus_lists: 拒绝清单实际 {} 行（manifest 说 23）",
            list_lines(rej_file)
        ));
    }
    if nb.unwrap_or(0) + nr.unwrap_or(0) != 69 {
        return Err("corpus_lists: N + M 必须等于 69".into());
    }

    // ── ③ 门禁：键恰好是 §7 声明的九条
    let declared_gates = declared_gates(doc);
    let gate_obj = v["gate"]
        .as_object()
        .ok_or_else(|| "gate: 不是对象".to_string())?;
    let have_gates: BTreeSet<String> = gate_obj.keys().cloned().collect();
    if have_gates != declared_gates {
        return Err(format!(
            "gate: 键集与 §7 不一致（manifest {have_gates:?} / 文档 {declared_gates:?}）"
        ));
    }

    // ── ④ 测试 ID：集合与 §3 声明相等
    let declared = declared_ids(doc);
    let rows = v["test_ids"]
        .as_array()
        .ok_or_else(|| "test_ids: 不是数组".to_string())?;
    let mut have: BTreeSet<String> = BTreeSet::new();
    for r in rows {
        let id = r["id"].as_str().unwrap_or("").to_string();
        if !have.insert(id.clone()) {
            return Err(format!("test_ids: {id} 出现了两次"));
        }
        let verdict = r["verdict"].as_str().unwrap_or("");
        if !matches!(verdict, "pass" | "partial" | "missing") {
            return Err(format!("test_ids: {id} 的 verdict {verdict:?} 不合法"));
        }
        if !nonempty(r, "gate") {
            return Err(format!("test_ids: {id} 缺 gate"));
        }
    }
    if have != declared {
        let missing: Vec<&String> = declared.difference(&have).collect();
        let extra: Vec<&String> = have.difference(&declared).collect();
        return Err(format!(
            "test_ids: 与 §3 声明不一致（缺 {missing:?} / 多 {extra:?}）"
        ));
    }

    // ── ⑤ 缺口：非 pass 的门禁与测试必须被**同时**点名
    let gaps = v["open_gaps"]
        .as_array()
        .ok_or_else(|| "open_gaps: 不是数组".to_string())?;
    let mut gap_ids: BTreeSet<String> = BTreeSet::new();
    let mut gap_gates: BTreeSet<String> = BTreeSet::new();
    let mut gap_tests: BTreeSet<String> = BTreeSet::new();
    for g in gaps {
        for k in ["id", "gate", "test_id", "what", "why", "owner"] {
            if !nonempty(g, k) {
                return Err(format!("open_gaps: 有一条缺 `{k}`（{g:?}）"));
            }
        }
        let gid = g["id"].as_str().unwrap_or("").to_string();
        if !gap_ids.insert(gid.clone()) {
            return Err(format!("open_gaps: {gid} 出现了两次"));
        }
        let gate = g["gate"].as_str().unwrap_or("").to_string();
        if !declared_gates.contains(&gate) {
            return Err(format!("open_gaps: {gid} 的 gate {gate:?} 不是九条之一"));
        }
        gap_gates.insert(gate);
        let tid = g["test_id"].as_str().unwrap_or("").to_string();
        if !declared.contains(&tid) {
            return Err(format!("open_gaps: {gid} 的 test_id {tid:?} 不在 §3 里"));
        }
        gap_tests.insert(tid);
    }

    // ── ⑥ 门禁状态：pass 的证据不许自认未兑现；非 pass 必须有缺口
    for (gid, g) in gate_obj {
        let status = g["status"].as_str().unwrap_or("");
        if !matches!(status, "pass" | "partial" | "fail" | "missing") {
            return Err(format!("gate.{gid}: status {status:?} 不合法"));
        }
        let evidence = g["evidence"].as_str().unwrap_or("");
        if status == "pass" {
            if evidence.trim().is_empty() {
                return Err(format!("gate.{gid}: 标 pass 却没有 evidence"));
            }
            for lie in ["未实现", "未兑现", "TODO", "待补"] {
                if evidence.contains(lie) {
                    return Err(format!(
                        "gate.{gid}: 标 pass 但证据自认「{lie}」（这是第一类谎）"
                    ));
                }
            }
        } else if !gap_gates.contains(gid) {
            return Err(format!(
                "gate.{gid}: status={status} 但 open_gaps 里没人认领（这是第二类谎）"
            ));
        }
        // 门禁名下的每条测试：pass 或「被一条缺口与门禁同时点名」
        let tids = strs(g, "test_ids");
        if tids.is_empty() {
            return Err(format!("gate.{gid}: test_ids 为空"));
        }
        for t in &tids {
            if !declared.contains(t) {
                return Err(format!("gate.{gid}: 名下的 {t} 不在 §3 里"));
            }
            let verdict = rows
                .iter()
                .find(|r| r["id"].as_str() == Some(t.as_str()))
                .and_then(|r| r["verdict"].as_str())
                .unwrap_or("");
            if verdict != "pass" && !(gap_tests.contains(t) && gap_gates.contains(gid)) {
                return Err(format!(
                    "gate.{gid}: 名下 {t} 是 {verdict}，但没有缺口同时点名它和这个门禁（第二类谎）"
                ));
            }
        }
    }

    // ── ⑦ 每条测试：非 pass 必须在缺口里被点名
    for r in rows {
        let id = r["id"].as_str().unwrap_or("");
        if r["verdict"].as_str() != Some("pass") && !gap_tests.contains(id) {
            return Err(format!(
                "test_ids: {id} 的 verdict 不是 pass，但 open_gaps 里没人点名它（第二类谎）"
            ));
        }
    }

    // ── ⑧ 白名单：与 §5 集合相等，五栏俱全
    let declared_u = declared_unsupported(doc);
    let us = v["unsupported"]
        .as_array()
        .ok_or_else(|| "unsupported: 不是数组".to_string())?;
    let mut have_u: BTreeSet<String> = BTreeSet::new();
    for u in us {
        let id = u["id"].as_str().unwrap_or("").to_string();
        if !have_u.insert(id.clone()) {
            return Err(format!("unsupported: {id} 出现了两次"));
        }
        for k in ["item", "why", "positive_evidence", "phase"] {
            if !nonempty(u, k) {
                return Err(format!("unsupported: {id} 缺 `{k}`"));
            }
        }
    }
    if have_u != declared_u {
        let missing: Vec<&String> = declared_u.difference(&have_u).collect();
        let extra: Vec<&String> = have_u.difference(&declared_u).collect();
        return Err(format!(
            "unsupported: 与 §5 不一致（缺 {missing:?} / 多 {extra:?}）"
        ));
    }

    // ── ⑨ 实测数字：不许低于 M5 基线，且 `done` 当且仅当全绿
    let me = &v["measured"];
    let tests = me["workspace_tests"].as_u64().unwrap_or(0);
    let base = me["m5_baseline_tests"].as_u64().unwrap_or(u64::MAX);
    let failed = me["workspace_failed"].as_u64().unwrap_or(u64::MAX);
    if tests < base {
        return Err(format!(
            "measured: workspace_tests {tests} < M5 基线 {base}"
        ));
    }
    if me["diag_codes"].as_u64() != Some(103) {
        return Err(format!(
            "measured: DiagCode 必须仍是 103（C8 冻结），实际 {:?}",
            me["diag_codes"]
        ));
    }
    let all_pass = gate_obj
        .values()
        .all(|g| g["status"].as_str() == Some("pass"));
    let status = v["phase_summary"]["status"].as_str().unwrap_or("");
    if !matches!(status, "in_progress" | "done") {
        return Err(format!("phase_summary: status {status:?} 不合法"));
    }
    if status == "done" {
        if !(all_pass && failed == 0) {
            return Err(format!(
                "phase_summary: 自称 done 但门禁全 pass={all_pass}、failed={failed}（这是第三类谎）"
            ));
        }
        // 报告是「交付 B」的实物：自称 done 而文件不存在 = 又一类谎
        let report = v["docs"]["report"].as_str().unwrap_or("");
        let path = report.split('（').next().unwrap_or("").trim();
        if !repo_root().join(path).exists() {
            return Err(format!(
                "phase_summary: 自称 done 但报告 {path:?} 不存在（交付物没落地）"
            ));
        }
    }

    // ── ⑩ 已关闭的缺口也要有出处
    for g in v["closed_gaps"]
        .as_array()
        .ok_or_else(|| "closed_gaps: 不是数组".to_string())?
    {
        for k in ["id", "what", "closed_by"] {
            if !nonempty(g, k) {
                return Err(format!("closed_gaps: 有一条缺 `{k}`"));
            }
        }
    }
    Ok(())
}

#[test]
fn t_ssa_44_manifest_与文档磁盘三方一致() {
    let v: Value = serde_json::from_str(&read(&manifest_path())).expect("manifest 不是合法 JSON");
    let doc = read(&tests_doc_path());
    if let Err(e) = check(&v, &doc) {
        panic!("对账失败：{e}");
    }
    // 分母写出来：空集合上的全称命题不是证据
    let ids = declared_ids(&doc);
    assert_eq!(ids.len(), 64, "§3 该声明 64 个 T-SSA-*");
    assert_eq!(declared_gates(&doc).len(), 9, "§7 该声明 9 条门禁");
    assert_eq!(declared_unsupported(&doc).len(), 9, "§5 白名单该是 9 条");
}

#[test]
fn t_ssa_44_四类谎与五种改坏都必须被拦() {
    let real: Value =
        serde_json::from_str(&read(&manifest_path())).expect("manifest 不是合法 JSON");
    let doc = read(&tests_doc_path());
    assert!(check(&real, &doc).is_ok(), "真文件必须先过");

    /// 改一份副本，跑 `check`，断言错误文本里有 `want`。
    fn must_fail(v: &Value, doc: &str, want: &str, what: &str) {
        let err = check(v, doc).expect_err(&format!("{what}：改坏了却仍然通过"));
        assert!(
            err.contains(want),
            "{what}：错误信息该提到 {want:?}，实际 {err:?}"
        );
    }

    // ── 三类谎
    // 第一类：标 pass 却自认未兑现
    let mut a = real.clone();
    a["gate"]["SSA-4"]["evidence"] = Value::String("未兑现：三方差分还没跑".into());
    must_fail(&a, &doc, "第一类谎", "标 pass 但证据自认未兑现");

    // 第二类：非 pass 无缺口
    let mut b = real.clone();
    b["gate"]["SSA-6"]["status"] = Value::String("partial".into());
    must_fail(&b, &doc, "第二类谎", "门禁标 partial 但没人认领");

    let mut c = real.clone();
    for r in c["test_ids"].as_array_mut().unwrap() {
        if r["id"] == "T-SSA-13" {
            r["verdict"] = Value::String("partial".into());
        }
    }
    must_fail(&c, &doc, "第二类谎", "测试非 pass 但没人认领");

    // 第三类：未全绿自称 done
    let mut d = real.clone();
    d["phase_summary"]["status"] = Value::String("done".into());
    d["measured"]["workspace_failed"] = Value::from(3);
    must_fail(&d, &doc, "第三类谎", "有失败却自称 done");

    // 第四类（`R-50-1` 新加）：自称 done 但报告文件不存在 —— 「交付 B」没落地
    let mut d2 = real.clone();
    d2["phase_summary"]["status"] = Value::String("done".into());
    d2["docs"]["report"] = Value::String("docs/milestones/M6-report-不存在.md".into());
    must_fail(&d2, &doc, "不存在", "自称 done 但报告没落地");

    // ── 另外五种改坏
    let mut e = real.clone();
    e["corpus_lists"]["buildable"]["count"] = Value::from(47);
    must_fail(&e, &doc, "buildable", "N 改了");

    let mut f = real.clone();
    f["test_ids"].as_array_mut().unwrap().pop();
    must_fail(&f, &doc, "test_ids: 与 §3 声明不一致", "少一条测试");

    let mut g = real.clone();
    let dup = g["open_gaps"][0].clone();
    g["open_gaps"].as_array_mut().unwrap().push(dup);
    must_fail(&g, &doc, "出现了两次", "缺口 ID 重复");

    // ⑧ 这一条改的是**文档**：证明解析器真的在读 §5，而不是把 9 写死
    let doc_less = doc.replace("| `U-M6-09` |", "| `U-M6-99` |");
    assert_ne!(doc_less, doc, "文档里该有 `U-M6-09` 这一行");
    must_fail(
        &real,
        &doc_less,
        "unsupported: 与 §5 不一致",
        "文档改了白名单",
    );

    // ⑨ 这一条也改文档：§3 多一条声明 ⇒ manifest 缺它
    let doc_more = doc.replace(
        "| `T-SSA-64` | A/B 与 verifier |",
        "| `T-SSA-65` | 不存在的判据 | 占位 |\n| `T-SSA-64` | A/B 与 verifier |",
    );
    assert_ne!(doc_more, doc, "文档里该有 `T-SSA-64` 这一行");
    must_fail(
        &real,
        &doc_more,
        "test_ids: 与 §3 声明不一致",
        "文档多一条测试",
    );
}
