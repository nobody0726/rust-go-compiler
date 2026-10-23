//! `M5-manifest.json` ↔ `M5-tests.md` 的**对账断言** —— M5/Task 50（`T-EX-55`）。
//!
//! # 为什么要有这一条
//!
//! manifest 是「机器可读的事实」，`M5-tests.md` 是「冻结的约定」。两者**同源但不自动同步**
//! —— 没有断言的话，改一处忘了另一处只会留下两份互相矛盾的文件，谁也不知道哪份算数。
//!
//! # 与 `manifest_m4.rs` 的关系
//!
//! 沿它的范式，但 M5 的诚实状态要求**更严**：M4 收口时只有两条门禁未兑现，M5 有四条
//! （`EX-1` / `EX-2` / `EX-4` / `EX-8`）。所以这里不能照抄「全 pass」的断言，而是钉
//! **可证伪的一致性**：
//!
//! | 对账项 | 判据 |
//! |---|---|
//! | `test_ids` | 与 `M5-tests.md` §3 五节表第一列**集合相等**（70 条）；`verdict ∈ {pass, partial}`；**非 pass 者必须在 `open_gaps` 里被点名** |
//! | `gate` | 键**恰好** `EX-1..EX-9`；`status ∈ {pass, partial, fail}`；**非 pass 者必须在 `open_gaps` 里被点名**；**标 `pass` 而证据自认「未兑现」= 红**；**标 `pass` 时名下每条 `test_id` 必须 pass，或该条已被一条缺口与门禁同时点名** |
//! | `unsupported` | 与 §7 + §7.1 的 `U-M5-xx` **集合相等**（14 条）；四栏非空 + 证据带指针 |
//! | `corpus_lists` | 四个计数（80/11/69/32）**同时**与磁盘清单的非注释行数相等；且 `80 == 69 + 11` |
//! | `budget` | 四个资源上限冻结值；`measured` 与 `phase_summary.workspace_tests` **两处一致**且不小于 M4 基线 1541 |
//! | `coverage` | 计数器 == 实际条数；`matrix_rows` == §2.1 实有行数；`derived_gates` 的每条都在 §3 声明集里 |
//! | `phase_summary` | `tasks == 52`；四条 cargo + 五条自检；`status == "done"` **当且仅当**九条门禁全 pass |
//! | `open_gaps` | 四栏俱全、ID 不重复 |
//!
//! # 断言自己也要能被证伪
//!
//! 只有「真文件 ⇒ Ok」的断言是**恒真**的近亲：把检查写成空函数就永远绿。所以下面还有
//! 一条**合成反例**测试：对真 manifest 逐项改坏（20 种），每种都必须报出**指名道姓**的错误。
//! 其中最后两条改的是**文档**而不是 manifest —— 证明解析器真的在读 `M5-tests.md`，
//! 而不是把 70/14 写死在代码里。
//!
//! # 依赖
//!
//! `serde_json` 是 `rgoc-driver` **已有**的依赖，不是为这条测试新引的（`AGENTS.md` §4.3）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达（cargo 的 CARGO_MANIFEST_DIR 变了？）")
}

fn manifest_path() -> PathBuf {
    repo_root().join("docs/milestones/M5-manifest.json")
}

fn tests_doc_path() -> PathBuf {
    repo_root().join("docs/milestones/M5-tests.md")
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

/// §3 五节表第一列声明的 ID（`| \`T-EX-11\` | …`）。
///
/// 只认行首形态：ID 出现在别的列（例如判据里引用 `T-EX-09/18`）算**引用**，不算声明。
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
        if id.starts_with("T-EX-") {
            out.insert(id.to_string());
        }
    }
    out
}

/// §7（冻结 12 条）+ §7.1（`R-7-1` 追加 2 条）的 `U-M5-xx`。
///
/// 整个 `## 7` … `## 8` 区间都扫：§7.1 是 §7 的子节，两张表都要算进白名单 ——
/// 只扫第一张表会让声明集从 14 掉到 12，而 manifest 若也只写 12，两边就
/// 「一致地错」了（那是对账测试最该防的失效模式）。
fn declared_unsupported(doc: &str) -> BTreeSet<String> {
    let sec = section(doc, "\n## 7", "\n## 8");
    let mut out = BTreeSet::new();
    let mut i = 0;
    while let Some(p) = sec[i..].find("U-M5-") {
        let start = i + p;
        let mut j = start + "U-M5-".len();
        while sec.as_bytes().get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
        if j > start + "U-M5-".len() {
            out.insert(sec[start..j].to_string());
        }
        i = j.max(start + 1);
    }
    out
}

/// §2.1 覆盖矩阵的**实际行数**（行首 `| <数字> |`）。
fn matrix_rows(doc: &str) -> usize {
    section(doc, "\n### 2.1", "\n### 2.2")
        .lines()
        .filter(|l| {
            let b = l.as_bytes();
            b.len() > 3 && b[0] == b'|' && b[1] == b' ' && b[2].is_ascii_digit()
        })
        .count()
}

fn strs(v: &Value, key: &str) -> Vec<String> {
    v[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} 不是数组"))
        .iter()
        .map(|x| x.as_str().unwrap_or_default().to_string())
        .collect()
}

fn nonempty(v: &Value, key: &str) -> bool {
    v[key].as_str().is_some_and(|s| !s.trim().is_empty())
}

/// 把文本切成「词」：`-` 与字母数字之外一律当分隔符。
fn tokens(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter(|t| !t.is_empty())
}

fn is_test_id(t: &str) -> bool {
    t.strip_prefix("T-EX-")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric()))
}

fn is_gate_id(t: &str) -> bool {
    t.strip_prefix("EX-")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

/// 证据/理由必须**指得到落点**：含 `/`（文件路径）或点名一条 `T-EX-*`。
///
/// 按完整词比，不用 `contains("T-EX-")`：子串匹配会让 `T-EX-6` 也命中 `T-EX-61`，
/// 「必须指向某条测试」这条判据就被一个不相干的 ID 满足了。
fn points_at_something(s: &str) -> bool {
    s.contains('/') || tokens(s).any(is_test_id)
}

/// 清单一类的文件：**非空且非 `#` 注释**的行数。
fn list_lines(rel: &str) -> usize {
    let p = repo_root().join(rel);
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()));
    text.lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#')
        })
        .count()
}

fn gaps_raw(v: &Value) -> &[Value] {
    v["open_gaps"]["items"].as_array().map_or(&[], Vec::as_slice)
}

/// 所有 `open_gaps.items[].gate`/`.note` 里出现的**完整词**（`EX-2`、`T-EX-40` …）。
///
/// 这是「缺口登记」与「门禁/测试状态」之间的桥：非 pass 的门禁/测试项必须在这里被点名，
/// 否则「显式降级」与「悄悄算了」在文件里长得一模一样。
fn gap_tokens(v: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for g in gaps_raw(v) {
        for f in ["gate", "note"] {
            if let Some(s) = g[f].as_str() {
                for t in tokens(s) {
                    if is_test_id(t) || is_gate_id(t) {
                        out.insert(t.to_string());
                    }
                }
            }
        }
    }
    out
}

/// 证据里**自认未兑现**的措辞 —— 与 `status == "pass"` 同时出现即谎。
const NOT_DONE_MARKERS: &[&str] = &["未兑现", "未达成", "尚未绿", "仍红"];

/// 核心检查：`Err(检查名: 说明)`。第一个不通过就返回（顺序即优先级）。
fn check(v: &Value, tests_md: &str) -> Result<(), String> {
    // ① 顶层结构
    for k in [
        "stage", "name", "generated_at", "filled_at", "status", "scope", "docs", "crates",
        "corpus_lists", "gate", "coverage", "test_ids", "unsupported", "budget", "open_gaps",
        "phase_summary",
    ] {
        if v.get(k).is_none() {
            return Err(format!("structure: 顶层缺 `{k}`"));
        }
    }
    if v["stage"].as_str() != Some("M5") {
        return Err(format!("stage: 应为 M5，实际 {:?}", v["stage"]));
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

    // ② corpus_lists：四个计数与磁盘**同时**相等，且候选 == 入选 ∪ 排除
    let cl = &v["corpus_lists"];
    for (key, want, file) in [
        ("candidate", 80u64, "tools/run-oracle/run-candidates.txt"),
        ("excluded", 11, "tools/run-oracle/run-excluded.txt"),
        ("selected", 69, "tools/run-oracle/run-selected.txt"),
        ("ken", 32, "tools/run-oracle/run-ken.txt"),
    ] {
        if cl[key]["count"].as_u64() != Some(want) {
            return Err(format!(
                "corpus_lists: {key}.count 应为 {want}，实际 {:?}",
                cl[key]["count"]
            ));
        }
        if cl[key]["file"].as_str() != Some(file) {
            return Err(format!("corpus_lists: {key}.file 应为 {file}"));
        }
        let actual = list_lines(file);
        if actual as u64 != want {
            return Err(format!(
                "corpus_lists: {file} 实有 {actual} 行（非注释），与声明的 {want} 不符"
            ));
        }
    }
    let cand = cl["candidate"]["count"].as_u64().unwrap_or(0);
    let sel = cl["selected"]["count"].as_u64().unwrap_or(0);
    let exc = cl["excluded"]["count"].as_u64().unwrap_or(0);
    if cand != 80 || sel + exc != cand {
        return Err(format!(
            "corpus_lists: 候选 {cand} != 入选 {sel} + 排除 {exc}（应为 80 = 69 + 11）"
        ));
    }
    if cl["excluded"]["frozen_base"].as_u64() != Some(6) {
        return Err("corpus_lists: excluded.frozen_base 应记 6（R-6-2 的基线）".into());
    }
    if !nonempty(cl, "complement_assert") {
        return Err("corpus_lists: complement_assert 空".into());
    }

    // ③ gate：键恰好 EX-1..9；状态合法；非 pass 必须被点名；pass 不许自认未兑现
    let want_gates: BTreeSet<String> = (1..=9).map(|i| format!("EX-{i}")).collect();
    let gate = v["gate"].as_object().ok_or("gate: 不是对象")?;
    let got_gates: BTreeSet<String> = gate.keys().cloned().collect();
    if got_gates != want_gates {
        return Err(format!("gate: 键集应为 {want_gates:?}，实际 {got_gates:?}"));
    }
    let pointers = gap_tokens(v);
    for (k, g) in gate {
        let status = g["status"].as_str().unwrap_or_default();
        if !matches!(status, "pass" | "partial" | "fail") {
            return Err(format!("gate: {k} 的 status `{status}` 不在 pass/partial/fail"));
        }
        for f in ["name", "definition", "evidence"] {
            if !nonempty(g, f) {
                return Err(format!("gate: {k} 的 `{f}` 空"));
            }
        }
        let ev = g["evidence"].as_str().unwrap_or_default();
        if ev.len() < 16 || !points_at_something(ev) {
            return Err(format!("gate: {k} 的 evidence 太短或无指针：{ev:?}"));
        }
        if status == "pass" && NOT_DONE_MARKERS.iter().any(|m| ev.contains(m)) {
            return Err(format!(
                "gate: {k} 标了 pass，但 evidence 自认未兑现（自相矛盾）：{ev:?}"
            ));
        }
        if status != "pass" && !pointers.contains(k.as_str()) {
            return Err(format!(
                "gate: {k} 标了 {status} 却没有 open_gaps 登记（显式降级必须留痕）"
            ));
        }
        let ids = strs(g, "test_ids");
        if ids.is_empty() {
            return Err(format!("gate: {k} 的 test_ids 空"));
        }
        for id in &ids {
            if !is_test_id(id) {
                return Err(format!("gate: {k} 的 test_ids 里有非 T-EX 项：{id}"));
            }
        }
    }

    // ④ test_ids：集合与 §3 声明**相等**；非 pass 者必须被点名
    let ids: Vec<&Value> = v["test_ids"]
        .as_array()
        .ok_or("test_ids: 不是数组")?
        .iter()
        .collect();
    let got: BTreeSet<String> = ids
        .iter()
        .map(|t| t["id"].as_str().unwrap_or_default().to_string())
        .collect();
    let declared = declared_ids(tests_md);
    if got != declared {
        let only_doc: Vec<&String> = declared.difference(&got).collect();
        let only_manifest: Vec<&String> = got.difference(&declared).collect();
        return Err(format!(
            "test_ids: 与 §3 集合不等 —— 只在文档 {only_doc:?} / 只在 manifest {only_manifest:?}"
        ));
    }
    for t in &ids {
        let id = t["id"].as_str().unwrap_or_default();
        let verdict = t["verdict"].as_str().unwrap_or_default();
        if !matches!(verdict, "pass" | "partial") {
            return Err(format!("test_ids: {id} 的 verdict `{verdict}` 不在 pass/partial"));
        }
        let g = t["gate"].as_str().unwrap_or_default();
        if !want_gates.contains(g) {
            return Err(format!("test_ids: {id} 的 gate `{g}` 不在 EX-1..9"));
        }
        let leg = t["leg"].as_str().unwrap_or_default();
        if !matches!(leg, "甲" | "乙" | "丙" | "丁" | "反") {
            return Err(format!("test_ids: {id} 的 leg `{leg}` 不在 甲乙丙丁反"));
        }
        let ev = t["evidence"].as_str().unwrap_or_default();
        if ev.len() < 12 || !points_at_something(ev) {
            return Err(format!("test_ids: {id} 的 evidence 太短或无指针：{ev:?}"));
        }
        if verdict != "pass" && !pointers.contains(id) {
            return Err(format!(
                "test_ids: {id} 标了 {verdict} 却没有 open_gaps 登记（显式降级必须留痕）"
            ));
        }
    }

    // ④′ pass 的门禁不许带非 pass 的测试项 —— 除非有一条缺口把「该门禁」与
    //    「该测试项」**同时**点名。这是 ③ 与 ④ 的交叉：只靠 ④ 的话，一条缺口
    //    可以一个字都不提它卡住的是哪个门禁，于是「某条边角测试没做完」与
    //    「整条门禁没兑现」在文件里长得一模一样。
    let mut tied: Vec<BTreeSet<String>> = Vec::new();
    for g in gaps_raw(v) {
        let mut toks = BTreeSet::new();
        for f in ["gate", "note"] {
            if let Some(s) = g[f].as_str() {
                for t in tokens(s) {
                    if is_test_id(t) || is_gate_id(t) {
                        toks.insert(t.to_string());
                    }
                }
            }
        }
        tied.push(toks);
    }
    for (k, g) in gate {
        if g["status"].as_str() != Some("pass") {
            continue;
        }
        for t in &ids {
            let id = t["id"].as_str().unwrap_or_default();
            if t["gate"].as_str() != Some(k.as_str()) || t["verdict"].as_str() == Some("pass") {
                continue;
            }
            let paired = tied
                .iter()
                .any(|toks| toks.contains(k.as_str()) && toks.contains(id));
            if !paired {
                return Err(format!(
                    "gate: {k} 标了 pass，但名下 {id} 未 pass 且没有一条缺口把两者同时点名"
                ));
            }
        }
    }

    // ⑤ unsupported：集合与 §7+§7.1 相等；四栏非空 + 证据带指针
    let us: Vec<&Value> = v["unsupported"]
        .as_array()
        .ok_or("unsupported: 不是数组")?
        .iter()
        .collect();
    let got_u: BTreeSet<String> = us
        .iter()
        .map(|u| u["id"].as_str().unwrap_or_default().to_string())
        .collect();
    let declared_u = declared_unsupported(tests_md);
    if got_u != declared_u {
        let only_doc: Vec<&String> = declared_u.difference(&got_u).collect();
        let only_manifest: Vec<&String> = got_u.difference(&declared_u).collect();
        return Err(format!(
            "unsupported: 与 §7/§7.1 集合不等 —— 只在文档 {only_doc:?} / 只在 manifest {only_manifest:?}"
        ));
    }
    for u in &us {
        let id = u["id"].as_str().unwrap_or_default();
        for f in ["item", "why", "planned_phase"] {
            if !nonempty(u, f) {
                return Err(format!("unsupported: {id} 的 `{f}` 空"));
            }
        }
        let pe = u["positive_evidence"].as_str().unwrap_or_default();
        if !points_at_something(pe) {
            return Err(format!("unsupported: {id} 缺证据指针（T-EX-* 或路径）：{pe:?}"));
        }
    }

    // ⑥ coverage：计数器自洽（手抄的数字不许和实际条数打架）
    let cov = &v["coverage"];
    if cov["test_ids_total"].as_u64() != Some(ids.len() as u64) {
        return Err(format!(
            "coverage: test_ids_total={:?} 与实际 {} 条不符",
            cov["test_ids_total"],
            ids.len()
        ));
    }
    if cov["unsupported_total"].as_u64() != Some(us.len() as u64) {
        return Err(format!(
            "coverage: unsupported_total={:?} 与实际 {} 条不符",
            cov["unsupported_total"],
            us.len()
        ));
    }
    let rows = matrix_rows(tests_md);
    if cov["matrix_rows"].as_u64() != Some(rows as u64) {
        return Err(format!(
            "coverage: matrix_rows={:?} 与 §2.1 实有 {rows} 行不符",
            cov["matrix_rows"]
        ));
    }
    // leg 分布也要与实际相符（§0 的「甲 40 / 乙 8 / 丙 8 / 丁 6 / 反 8」）
    for (leg, want) in [("甲", 40u64), ("乙", 8), ("丙", 8), ("丁", 6), ("反", 8)] {
        let n = ids.iter().filter(|t| t["leg"] == json!(leg)).count() as u64;
        if cov["legs"][leg].as_u64() != Some(n) || n != want {
            return Err(format!(
                "coverage: legs.{leg} 应为 {want}，manifest 记 {:?}，实际 {n}",
                cov["legs"][leg]
            ));
        }
    }
    let derived: Vec<String> = cov["derived_gates"]["items"]
        .as_array()
        .ok_or("coverage: derived_gates.items 不是数组")?
        .iter()
        .map(|x| x.as_str().unwrap_or_default().to_string())
        .collect();
    if derived.is_empty() {
        return Err("coverage: derived_gates.items 空".into());
    }
    for d in &derived {
        if !declared.contains(d) {
            return Err(format!(
                "coverage: derived_gates 里的 {d} 不在 §3 声明集（野 ID 不许借派生名目混入）"
            ));
        }
        // 派生项不许同时出现在 §8 的显式列表里（否则「派生」是假话）
        let listed = gate
            .values()
            .any(|g| strs(g, "test_ids").iter().any(|x| x == d));
        if listed {
            return Err(format!(
                "coverage: {d} 已在某条门禁的显式 test_ids 里，不该再算派生项"
            ));
        }
    }

    // ⑦ budget：资源上限冻结值 + 实测两处一致且不低于 M4 基线
    let b = &v["budget"];
    for (k, want) in [
        ("frame_depth", 1000u64),
        ("eval_stack_depth", 10000),
        ("steps", 100000000),
        ("wall_clock_sec", 10),
    ] {
        if b["resource_limits"][k].as_u64() != Some(want) {
            return Err(format!(
                "budget: resource_limits.{k} 应为 {want}，实际 {:?}",
                b["resource_limits"][k]
            ));
        }
    }
    if b["measured"]["workspace_tests_failed"].as_u64() != Some(0) {
        return Err("budget: measured.workspace_tests_failed 应为 0".into());
    }
    // 实测**测试总数**是收口时刻的一次称重，不是活的不变量。钉两件能钉的事：
    //   ① 两处记录必须一致（budget.measured 与 phase_summary.workspace_tests 同源）；
    //   ② 不得小于 M4 收口的基线 1541（缩水 = 有人改小了数字而没重跑）。
    const WORKSPACE_TESTS_AT_M4_BASELINE: u64 = 1541;
    let measured = b["measured"]["workspace_tests_passed"].as_u64().unwrap_or(0);
    let summary = v["phase_summary"]["workspace_tests"]["passed"]
        .as_u64()
        .unwrap_or(0);
    if measured != summary {
        return Err(format!(
            "budget: 两处实测数不一致（measured={measured} / phase_summary={summary}）"
        ));
    }
    if measured < WORKSPACE_TESTS_AT_M4_BASELINE {
        return Err(format!(
            "budget: 实测测试数 {measured} 小于 M4 收口的基线 {WORKSPACE_TESTS_AT_M4_BASELINE}"
        ));
    }

    // ⑧ open_gaps：四栏俱全、ID 不重复
    let gaps = v["open_gaps"]["items"]
        .as_array()
        .ok_or("open_gaps: items 不是数组")?;
    if gaps.is_empty() {
        return Err("open_gaps: items 空 —— M5 的缺口如实登记是硬要求".into());
    }
    let mut seen = BTreeSet::new();
    for g in gaps {
        for f in ["id", "item", "note", "gate"] {
            if !nonempty(g, f) {
                return Err(format!("open_gaps: 有条目缺 `{f}`"));
            }
        }
        let id = g["id"].as_str().unwrap_or_default();
        if !seen.insert(id.to_string()) {
            return Err(format!("open_gaps: ID `{id}` 重复"));
        }
    }

    // ⑨ phase_summary：52 任务、四条命令、≥5 条脚本；done 当且仅当九条门禁全 pass
    let ps = &v["phase_summary"];
    if ps["tasks"].as_u64() != Some(52) {
        return Err(format!("phase_summary: tasks 应为 52，实际 {:?}", ps["tasks"]));
    }
    if !nonempty(ps, "status") || !nonempty(ps, "completed_tasks") {
        return Err("phase_summary: status / completed_tasks 空".into());
    }
    let cmds = strs(ps, "gate_commands").join(" ");
    for c in ["cargo fmt", "cargo check", "cargo clippy", "cargo test"] {
        if !cmds.contains(c) {
            return Err(format!("phase_summary: gate_commands 缺 `{c}`"));
        }
    }
    if strs(ps, "gate_scripts").len() < 5 {
        return Err("phase_summary: gate_scripts 少于五条（§9.2 五形态）".into());
    }
    let all_pass = gate.values().all(|g| g["status"].as_str() == Some("pass"));
    let says_done = ps["status"].as_str() == Some("done");
    if all_pass != says_done {
        return Err(format!(
            "phase_summary: status={:?} 与九条门禁（全 pass={all_pass}）不一致 —— done 不许自封",
            ps["status"]
        ));
    }
    let npass = gate
        .values()
        .filter(|g| g["status"].as_str() == Some("pass"))
        .count();
    if ps["gates_pass"].as_u64() != Some(npass as u64) {
        return Err(format!(
            "phase_summary: gates_pass={:?} 与实际 {npass} 条 pass 不符",
            ps["gates_pass"]
        ));
    }
    if ps["gates_total"].as_u64() != Some(9) {
        return Err("phase_summary: gates_total 应为 9".into());
    }

    Ok(())
}

fn load() -> (Value, String) {
    let mp = manifest_path();
    let tp = tests_doc_path();
    let text = std::fs::read_to_string(&mp).unwrap_or_else(|e| panic!("读不到 {}：{e}", mp.display()));
    let v: Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} 不是合法 JSON：{e}", mp.display()));
    let doc = std::fs::read_to_string(&tp).unwrap_or_else(|e| panic!("读不到 {}：{e}", tp.display()));
    (v, doc)
}

/// 真文件必须通过 —— 这是「manifest 与冻结文档一致」的可执行判据。
#[test]
fn m5清单_与测试文档对得上且字段齐备() {
    let (v, doc) = load();
    assert_eq!(
        v["test_ids"].as_array().map(Vec::len),
        Some(70),
        "test_ids 应为 70 条（§3 的声明数：甲 40 + 乙 8 + 丙 8 + 丁 6 + 反 8）"
    );
    assert_eq!(
        v["unsupported"].as_array().map(Vec::len),
        Some(14),
        "unsupported 应为 14 条（U-M5-01..12 冻结 + R-7-1 追加 13/14）"
    );
    assert_eq!(
        v["gate"].as_object().map(serde_json::Map::len),
        Some(9),
        "gate 应为 9 条（EX-1..EX-9）"
    );
    check(&v, &doc).unwrap_or_else(|e| panic!("manifest 与冻结文档不一致：{e}"));
}

/// 合成反例：把真 manifest **逐项改坏**，每种都必须报错（否则断言是摆设）。
///
/// 最后两条改的是**文档**而不是 manifest（证明 `declared_ids` / `declared_unsupported`
/// 真的在读 `M5-tests.md`，而不是把 70/14 写死在代码里）。
#[test]
fn m5清单_合成反例逐条报错() {
    let (base, doc) = load();
    let cases: Vec<(&str, Value, &str)> = vec![
        (
            "删一条 test_id",
            {
                let mut v = base.clone();
                v["test_ids"]
                    .as_array_mut()
                    .expect("array")
                    .retain(|t| t["id"] != json!("T-EX-31"));
                v
            },
            "test_ids",
        ),
        (
            "编一条不存在的 test_id",
            {
                let mut v = base.clone();
                v["test_ids"].as_array_mut().expect("array").push(json!({
                    "id":"T-EX-99","leg":"甲","name":"编的","gate":"EX-1",
                    "verdict":"pass","evidence":"rgoc/x/y.rs 的某测试"
                }));
                v
            },
            "test_ids",
        ),
        (
            "test_id 的 verdict 改成 fail",
            {
                let mut v = base.clone();
                v["test_ids"][0]["verdict"] = json!("fail");
                v
            },
            "test_ids",
        ),
        (
            "test_id 的 evidence 无指针",
            {
                let mut v = base.clone();
                v["test_ids"][3]["evidence"] = json!("已经覆盖");
                v
            },
            "test_ids",
        ),
        (
            "test_id 的 gate 越界",
            {
                let mut v = base.clone();
                v["test_ids"][0]["gate"] = json!("EX-99");
                v
            },
            "test_ids",
        ),
        (
            "test_id 的 leg 编造",
            {
                let mut v = base.clone();
                v["test_ids"][0]["leg"] = json!("戊");
                v
            },
            "test_ids",
        ),
        (
            "非 pass 的 test_id 没有缺口登记",
            {
                let mut v = base.clone();
                // 抹掉 G-M5-02 对 T-EX-40 的点名（只留门禁）
                for g in v["open_gaps"]["items"].as_array_mut().expect("array") {
                    if g["id"] == json!("G-M5-02") {
                        g["gate"] = json!("EX-2");
                    }
                }
                v
            },
            "test_ids",
        ),
        (
            "把 gate 的键换成 EX-10",
            {
                let mut v = base.clone();
                let g = v["gate"].as_object_mut().expect("obj");
                let ex9 = g.remove("EX-9").expect("EX-9");
                g.insert("EX-10".into(), ex9);
                v
            },
            "gate",
        ),
        (
            "某个 gate 标 fail 却没缺口登记",
            {
                let mut v = base.clone();
                v["gate"]["EX-5"]["status"] = json!("fail");
                v
            },
            "gate",
        ),
        (
            "gate 标 pass 但证据自认未兑现",
            {
                let mut v = base.clone();
                v["gate"]["EX-1"]["status"] = json!("pass");
                v
            },
            "gate",
        ),
        (
            "gate 的 evidence 换成空话",
            {
                let mut v = base.clone();
                v["gate"]["EX-6"]["evidence"] = json!("好了");
                v
            },
            "gate",
        ),
        (
            "gate 标 pass 但名下测试项未 pass 且无成对登记",
            {
                let mut v = base.clone();
                // EX-4 名下有 T-EX-32/33 两条 partial；把 EX-4 改成 pass 并把两条缺口的
                // 门禁指针抹掉（只留测试项）⇒ ④ 仍满足，但 ③′ 的「成对」判据必须炸。
                v["gate"]["EX-4"]["status"] = json!("pass");
                for g in v["open_gaps"]["items"].as_array_mut().expect("array") {
                    if g["id"] == json!("G-M5-03") || g["id"] == json!("G-M5-04") {
                        let t = g["gate"].as_str().unwrap_or_default().to_string();
                        let only_test = t.split('/').next_back().unwrap_or("").trim().to_string();
                        g["gate"] = json!(only_test);
                    }
                }
                v
            },
            "gate",
        ),
        (
            "删一条 unsupported",
            {
                let mut v = base.clone();
                v["unsupported"]
                    .as_array_mut()
                    .expect("array")
                    .retain(|u| u["id"] != json!("U-M5-05"));
                v
            },
            "unsupported",
        ),
        (
            "编一条文档没有的 unsupported",
            {
                let mut v = base.clone();
                v["unsupported"].as_array_mut().expect("array").push(json!({
                    "id":"U-M5-99","item":"x","why":"y","planned_phase":"z",
                    "positive_evidence":"rgoc/a/b.rs"
                }));
                v
            },
            "unsupported",
        ),
        (
            "unsupported 的 planned_phase 空",
            {
                let mut v = base.clone();
                let a = v["unsupported"].as_array_mut().expect("array");
                a[0]["planned_phase"] = json!("  ");
                v
            },
            "unsupported",
        ),
        (
            "语料计数与磁盘不符",
            {
                let mut v = base.clone();
                v["corpus_lists"]["selected"]["count"] = json!(68);
                v
            },
            "corpus_lists",
        ),
        (
            "候选 != 入选 + 排除",
            {
                let mut v = base.clone();
                v["corpus_lists"]["candidate"]["count"] = json!(81);
                v
            },
            "corpus_lists",
        ),
        (
            "coverage 的 test_ids_total 对不上",
            {
                let mut v = base.clone();
                v["coverage"]["test_ids_total"] = json!(69);
                v
            },
            "coverage",
        ),
        (
            "coverage 的 leg 分布对不上",
            {
                let mut v = base.clone();
                v["coverage"]["legs"]["甲"] = json!(39);
                v
            },
            "coverage",
        ),
        (
            "derived_gates 里塞一条野 ID",
            {
                let mut v = base.clone();
                v["coverage"]["derived_gates"]["items"]
                    .as_array_mut()
                    .expect("array")
                    .push(json!("T-EX-98"));
                v
            },
            "coverage",
        ),
        (
            "把一条已显式列出的 ID 也算成派生项",
            {
                let mut v = base.clone();
                v["coverage"]["derived_gates"]["items"]
                    .as_array_mut()
                    .expect("array")
                    .push(json!("T-EX-01"));
                v
            },
            "coverage",
        ),
        (
            "resource_limits 被放宽",
            {
                let mut v = base.clone();
                v["budget"]["resource_limits"]["frame_depth"] = json!(100000);
                v
            },
            "budget",
        ),
        (
            "实测测试数两处不一致",
            {
                let mut v = base.clone();
                v["phase_summary"]["workspace_tests"]["passed"] = json!(1605);
                v
            },
            "budget",
        ),
        (
            "实测测试数低于 M4 基线",
            {
                let mut v = base.clone();
                v["budget"]["measured"]["workspace_tests_passed"] = json!(1500);
                v["phase_summary"]["workspace_tests"]["passed"] = json!(1500);
                v
            },
            "budget",
        ),
        (
            "open_gaps 的 ID 重复",
            {
                let mut v = base.clone();
                let a = v["open_gaps"]["items"].as_array_mut().expect("array");
                let first = a[0].clone();
                a.push(first);
                v
            },
            "open_gaps",
        ),
        (
            "phase_summary 自称 done 而门禁没全绿",
            {
                let mut v = base.clone();
                v["phase_summary"]["status"] = json!("done");
                v
            },
            "phase_summary",
        ),
        (
            "gates_pass 数对不上",
            {
                let mut v = base.clone();
                v["phase_summary"]["gates_pass"] = json!(9);
                v
            },
            "phase_summary",
        ),
        (
            "tasks 数被改小",
            {
                let mut v = base.clone();
                v["phase_summary"]["tasks"] = json!(50);
                v
            },
            "phase_summary",
        ),
        (
            "gate_commands 少一条",
            {
                let mut v = base.clone();
                let a = v["phase_summary"]["gate_commands"]
                    .as_array_mut()
                    .expect("array");
                a.retain(|c| !c.as_str().unwrap_or_default().contains("clippy"));
                v
            },
            "phase_summary",
        ),
        (
            "filled_at 退回 null",
            {
                let mut v = base.clone();
                v["filled_at"] = Value::Null;
                v
            },
            "structure",
        ),
    ];

    for (why, v, want) in &cases {
        match check(v, &doc) {
            Ok(()) => panic!("合成反例「{why}」没有报错 —— 这条断言是摆设"),
            Err(e) => assert!(
                e.starts_with(want),
                "合成反例「{why}」报的是 `{e}`，期望以 `{want}` 开头"
            ),
        }
    }

    // 文档侧的两条：证明解析器真在读 M5-tests.md
    let mut doc_no_id = doc.clone();
    doc_no_id = doc_no_id.replacen("| `T-EX-24` |", "| `T-EX-XX` |", 1);
    match check(&base, &doc_no_id) {
        Ok(()) => panic!("删掉 §3 里的一条声明后仍然通过 —— 解析器没在读文档"),
        Err(e) => assert!(e.starts_with("test_ids"), "文档反例报的是 `{e}`"),
    }
    let doc_no_u = doc.replacen("| `U-M5-14` |", "| `U-M5-XX` |", 1);
    match check(&base, &doc_no_u) {
        Ok(()) => panic!("删掉 §7.1 里的一条白名单后仍然通过 —— 解析器没在读文档"),
        Err(e) => assert!(e.starts_with("unsupported"), "文档反例报的是 `{e}`"),
    }
}
