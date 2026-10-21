//! `M4-manifest.json` ↔ `M4-tests.md` 的**对账断言** —— M4/Task 42。
//!
//! # 为什么要有这一条
//!
//! manifest 是「机器可读的事实」，`M4-tests.md` 是「冻结的约定」。两者**同源但不自动同步**
//! —— 没有断言的话，改一处忘了另一处只会留下两份互相矛盾的文件，谁也不知道哪份算数。
//!
//! 沿 `manifest_m3.rs` 的范式，但对账口径**因 M4 的诚实状态而不同**：M3 收口时门禁全 `pass`，
//! 所以那条断言可以要求 `status == "pass"`；M4 收口时 `CH-2`/`CH-8` 实测**未兑现**、
//! `CH-3` **部分兑现**。若照抄「全 pass」的断言，只剩两条路：要么把 manifest 写成假的，
//! 要么把断言删了。两条都不行。所以这里把判据改成**可证伪的一致性**：
//!
//! | 对账项 | 判据 |
//! |---|---|
//! | `test_ids` | 与 `M4-tests.md` §3 五节表**第一列**声明的 ID **集合相等**（63 条）；`verdict ∈ {pass, partial}`；**非 pass 者必须在 `open_gaps` 里被点名**（显式降级 ≠ 静默失败） |
//! | `gate` | 键**恰好**是 `CH-1..CH-9`；`status ∈ {pass, partial, fail}`；**非 pass 者必须在 `open_gaps.gate` 里被点名**；**标 `pass` 而证据里自认「未兑现」= 红**（自相矛盾即谎）；**标 `pass` 时其名下每条 `test_id` 必须 pass，或该条已被 `open_gaps` 点名**（`CH-9` 对 `T-CK-A2` 的显式降级正是这一条） |
//! | `unsupported` | 与 §7.1+§7.2 白名单的 `U-M4-xx`/`U-M3-xx` **集合相等**（18 条）；四栏非空 + 证据带指针；`U-M3-11/12/13` 的**斜杠简写**必须展开（否则集合会静默变小） |
//! | `corpus_lists` | 三个计数（75/16/50）**同时**与磁盘上清单文件的非注释行数相等；且 `75 + 16 == 91`（候选 = 入选 ∪ 排除的算术面） |
//! | `budget` | §9.4 的冻结值（深度 1000 / import 100 / 单文件 10 s / smoke 300 / milestone 1800 / full 7200）；`effort_calendar` 三栏齐；`measured` 与 `phase_summary.workspace_tests` **两处一致**且不小于 Task 36 的基线 1536（缩水 = 有人改小了数字而没重跑） |
//! | `coverage` | 计数器 == 实际条数；`matrix_rows` == §2.1 表**实际行数**；`derived_gates` 的每条都必须在 §3 声明集里（不许借派生名目塞野 ID） |
//! | `phase_summary` | `tasks == 44`；四条 cargo 命令 + 五条自检脚本齐；`status == "done"` **当且仅当**九条门禁全 pass（否则「done」就是自欺） |
//! | `m3_sync` | 五条齐全，每条都指 `T-CK-E5`（`CH-6` 的兑现链不许断） |
//! | `open_gaps` | 每条四栏俱全、ID 不重复；`gate` 栏是「这条缺口卡住谁」的指针 |
//!
//! # 断言自己也要能被证伪
//!
//! 只有「真文件 ⇒ Ok」的断言是**恒真**的近亲：只要把检查写成空函数就永远绿。
//! 所以下面还有一条**合成反例**测试：对真 manifest 逐项**改坏**（24 种），
//! 每种都必须报出**指名道姓**的错误。这才是「这条断言会失败吗」的答案。
//!
//! # 依赖
//!
//! `serde_json` 是 `rgoc-driver` **已有**的依赖（`xtask` 生成 manifest 时用的就是它），
//! 不是为这条测试新引的（`AGENTS.md` §4.3：不默认新增第三方依赖）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达（cargo 的 CARGO_MANIFEST_DIR 变了？）")
}

fn manifest_path() -> PathBuf {
    repo_root().join("docs/milestones/M4-manifest.json")
}

fn tests_doc_path() -> PathBuf {
    repo_root().join("docs/milestones/M4-tests.md")
}

/// 取 `from` 到 `to` 之间的正文（两端的标题都必须在，缺一个就响亮失败）。
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

/// §3 五节表**第一列**声明的 ID（`| \`T-CK-11\` | …`）。
///
/// 只认行首形态：ID 出现在**别的列**里（例如备注列引用 `T-CK-62/63`）不算声明
/// —— 那些是**引用**，声明集合比它小且必须精确。
///
/// ⚠️ 这个解析器是**行首形态**的：若有人把冻结表折行／改成别的写法，`declared` 会**变小**。
/// 那不会静默削弱断言 —— 调用方有 `test_ids` 条数的显式断言（63），集合不等会立刻报出
/// 「只在 manifest」的那一批，等于把「解析器瞎了」与「manifest 漏了」一起顶到台面上。
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
        if id.starts_with("T-CK-") {
            out.insert(id.to_string());
        }
    }
    out
}

/// §7.1 + §7.2 白名单里的 `U-M4-xx` / `U-M3-xx`。
///
/// **不含 §7.3**：那张表是 `D-M4-11` 的**翻转表**（`U-M3-01..05`、`U-M3-08` 被 M4 解除），
/// 把它们算进白名单等于宣称「解除了却仍不支持」——正好反了。
///
/// `U-M3-11/12/13` 的斜杠简写要**展开**：只取首号会让声明集从 18 掉到 16，
/// 而 manifest 若也只写 16，两边就「一致地错」了 —— 那是对账测试最该防的失效模式。
fn declared_unsupported(doc: &str) -> BTreeSet<String> {
    let sec = section(doc, "\n### 7.1", "\n### 7.3");
    let mut out = BTreeSet::new();
    for prefix in ["U-M4-", "U-M3-"] {
        let mut i = 0;
        while let Some(p) = sec[i..].find(prefix) {
            let start = i + p;
            let mut j = start + prefix.len();
            while sec.as_bytes().get(j).is_some_and(u8::is_ascii_digit) {
                j += 1;
            }
            if j == start + prefix.len() {
                i = j;
                continue;
            }
            out.insert(sec[start..j].to_string());
            // `-11/12/13` 简写：把后续 `/<数字>` 逐个展开成同前缀的完整 ID。
            loop {
                let mut k = j;
                if sec.as_bytes().get(k) != Some(&b'/') {
                    break;
                }
                k += 1;
                let d0 = k;
                while sec.as_bytes().get(k).is_some_and(u8::is_ascii_digit) {
                    k += 1;
                }
                if k == d0 {
                    break;
                }
                out.insert(format!(
                    "{}{}",
                    &sec[start..start + prefix.len()],
                    &sec[d0..k]
                ));
                j = k;
            }
            i = j;
        }
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

/// 是否是**完整词**的 `T-CK-*`（`T-CK-A1`/`T-CK-B1`/`T-CK-E1` 也认）。
fn is_test_id(t: &str) -> bool {
    t.strip_prefix("T-CK-")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// 证据/理由必须**指得到落点**：要么含 `/`（文件路径），要么点名一条 `T-CK-*`。
///
/// **按完整词比，不用 `contains("T-CK-")`**：子串匹配会让 `T-CK-9` 也命中 `T-CK-91..95`，
/// 于是「必须指向某条测试」这条判据会被一个不相干的 ID 满足（测试自己骗自己）。
fn points_at_something(s: &str) -> bool {
    s.contains('/') || tokens(s).any(is_test_id)
}

/// 清单一类的文件：**非空且非 `#` 注释**的行数。
fn list_lines(rel: &str) -> usize {
    let p = repo_root().join(rel);
    let text =
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()));
    text.lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#')
        })
        .count()
}

fn gaps_raw(v: &Value) -> &[Value] {
    v["open_gaps"]["items"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// 所有 `open_gaps.items[].gate` 里出现的**完整词**（`CH-2`、`T-CK-A2`、`T-CK-31` …）。
///
/// 这是「缺口登记」与「门禁/测试状态」之间的桥：非 pass 的门禁/测试项必须在**这里**被点名，
/// 否则「显式降级」与「悄悄算了」在文件里长得一模一样。
fn gap_pointers(v: &Value) -> BTreeSet<String> {
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

fn is_gate_id(t: &str) -> bool {
    t.strip_prefix("CH-")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

/// 证据里**自认未兑现**的措辞 —— 与 `status == "pass"` 同时出现即谎。
const NOT_DONE_MARKERS: &[&str] = &["未兑现", "未达成", "尚未绿", "仍红"];

/// 核心检查：`Err(检查名: 说明)`。**第一个**不通过就返回（顺序即优先级）。
fn check(v: &Value, tests_md: &str) -> Result<(), String> {
    // ① 顶层结构
    for k in [
        "stage",
        "name",
        "generated_at",
        "filled_at",
        "scope",
        "docs",
        "crates",
        "corpus_lists",
        "gate",
        "coverage",
        "test_ids",
        "unsupported",
        "m3_sync",
        "budget",
        "open_gaps",
        "phase_summary",
    ] {
        if v.get(k).is_none() {
            return Err(format!("structure: 顶层缺 `{k}`"));
        }
    }
    if v["stage"].as_str() != Some("M4") {
        return Err(format!("stage: 应为 M4，实际 {:?}", v["stage"]));
    }
    for k in ["design", "tests", "plan"] {
        if !nonempty(&v["docs"], k) {
            return Err(format!("structure: docs.{k} 空"));
        }
    }
    let crates = strs(v, "crates");
    if crates.len() < 5 || crates.iter().any(|c| c.trim().is_empty()) {
        return Err("structure: crates 至少 5 条且都不为空".into());
    }

    // ② corpus_lists：三个计数与磁盘文件**同时**相等，且候选 = 入选 ∪ 排除
    let cl = &v["corpus_lists"];
    for (key, want, file) in [
        ("selected", 75u64, "tools/types-oracle/check-selected.txt"),
        ("allowlist", 16, "tools/types-oracle/check-allowlist.txt"),
        ("wave_a", 50, "tools/types-oracle/check-fixedbugs-waveA.txt"),
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
    let allow = cl["allowlist"]["count"].as_u64().unwrap_or(0);
    if cand != 91 || sel + allow != cand {
        return Err(format!(
            "corpus_lists: 候选 {cand} != 入选 {sel} + 排除 {allow}（应为 91 = 75 + 16）"
        ));
    }
    if !nonempty(cl, "complement_assert") {
        return Err("corpus_lists: complement_assert 空".into());
    }

    // ③ gate：键恰好 CH-1..9；状态合法；非 pass 必须在 open_gaps 点名；pass 不许自认未兑现
    let want_gates: BTreeSet<String> = (1..=9).map(|i| format!("CH-{i}")).collect();
    let gate = v["gate"].as_object().ok_or("gate: 不是对象")?;
    let got_gates: BTreeSet<String> = gate.keys().cloned().collect();
    if got_gates != want_gates {
        return Err(format!("gate: 键集应为 {want_gates:?}，实际 {got_gates:?}"));
    }
    let pointers = gap_pointers(v);
    for (k, g) in gate {
        let status = g["status"].as_str().unwrap_or_default();
        if !matches!(status, "pass" | "partial" | "fail") {
            return Err(format!(
                "gate: {k} 的 status `{status}` 不在 pass/partial/fail"
            ));
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
    }

    // ④ test_ids：集合与 §3 声明**相等**；非 pass 者必须被 open_gaps 点名
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
            "test_ids: 与 M4-tests.md §3 集合不等 —— 只在文档 {only_doc:?} / 只在 manifest {only_manifest:?}"
        ));
    }
    for t in &ids {
        let id = t["id"].as_str().unwrap_or_default();
        let verdict = t["verdict"].as_str().unwrap_or_default();
        if !matches!(verdict, "pass" | "partial") {
            return Err(format!(
                "test_ids: {id} 的 verdict `{verdict}` 不在 pass/partial"
            ));
        }
        let g = t["gate"].as_str().unwrap_or_default();
        if !want_gates.contains(g) {
            return Err(format!("test_ids: {id} 的 gate `{g}` 不在 CH-1..9"));
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

    // ③′ pass 的门禁不许带非 pass 的测试项 —— **除非**有一条缺口条目把「该门禁」与
    //    「该测试项」**同时**点名。这是 ③ 与 ④ 的交叉，也是 `CH-9` 能在 `T-CK-A2` 为
    //    `partial` 时仍标 `pass` 的**唯一**凭据（`G-M4-38` 的 gate 栏 = `CH-9 / T-CK-A2`）。
    //
    //    为什么不能只靠 ④：④ 只要求「非 pass 的测试项被某条缺口点名」，那条缺口可以
    //    一个字都不提它卡住的是哪个门禁 —— 于是「某条边角测试没做完」与「整条门禁没兑现」
    //    在文件里长得一模一样。这条把两者焊在一起。
    let mut tied: Vec<(BTreeSet<String>, String)> = Vec::new();
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
        tied.push((toks, g["id"].as_str().unwrap_or_default().to_string()));
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
                .any(|(toks, _)| toks.contains(k.as_str()) && toks.contains(id));
            if !paired {
                return Err(format!(
                    "gate: {k} 标了 pass，但名下 {id} 未 pass 且没有一条缺口把两者同时点名"
                ));
            }
        }
    }

    // ⑤ unsupported：集合与 §7.1+§7.2 相等；四栏非空 + 证据带指针
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
            "unsupported: 与 §7 集合不等 —— 只在文档 {only_doc:?} / 只在 manifest {only_manifest:?}"
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
            return Err(format!(
                "unsupported: {id} 缺证据指针（T-CK-* 或路径）：{pe:?}"
            ));
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
    }

    // ⑦ budget：§9.4 的冻结值 + effort_calendar + 实测两处一致
    let b = &v["budget"];
    for (k, want) in [
        ("recursion_depth_max", 1000u64),
        ("import_depth_max", 100),
        ("per_file_check_timeout_s", 10),
        ("smoke_wall_s_max", 300),
        ("milestone_wall_s_max", 1800),
        ("full_wall_s_max", 7200),
    ] {
        if b[k].as_u64() != Some(want) {
            return Err(format!("budget: {k} 应为 {want}，实际 {:?}", b[k]));
        }
    }
    for f in ["start", "effort_note"] {
        if !nonempty(&b["effort_calendar"], f) {
            return Err(format!("budget: effort_calendar.{f} 空"));
        }
    }
    if b["effort_calendar"]["max_weeks"].as_u64().unwrap_or(0) < 1 {
        return Err(format!(
            "budget: effort_calendar.max_weeks 应为正整数，实际 {:?}",
            b["effort_calendar"]["max_weeks"]
        ));
    }
    if b["measured"]["workspace_tests_failed"].as_u64() != Some(0) {
        return Err("budget: measured.workspace_tests_failed 应为 0".into());
    }
    // 实测**测试总数**是「M4 收口时刻的一次称重」，不是活的不变量（M5 会继续加测试）。
    // 所以这里钉两件能钉的事：
    //   ① 两处记录**必须一致**（`budget.measured` 与 `phase_summary.workspace_tests` 同源）；
    //   ② 不得**小于** Task 36 完成时的基线 **1536**（缩水 = 有人改小了数字而没重跑）。
    // 不能钉的（如实写在测试头）：它不会自己跑去数一遍 —— 谁把两处**同时**改小，这条抓不到。
    const WORKSPACE_TESTS_AT_M4_BASELINE: u64 = 1536;
    let measured = b["measured"]["workspace_tests_passed"]
        .as_u64()
        .unwrap_or(0);
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
            "budget: 实测测试数 {measured} 小于 Task 36 的基线 {WORKSPACE_TESTS_AT_M4_BASELINE}"
        ));
    }

    // ⑧ open_gaps：四栏俱全、ID 不重复
    let gaps = v["open_gaps"]["items"]
        .as_array()
        .ok_or("open_gaps: items 不是数组")?;
    if gaps.is_empty() {
        return Err("open_gaps: items 空 —— M4 的缺口如实登记是硬要求".into());
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

    // ⑨ phase_summary：44 任务、四条命令、五条脚本；`done` 当且仅当九条门禁全 pass
    let ps = &v["phase_summary"];
    if ps["tasks"].as_u64() != Some(44) {
        return Err(format!(
            "phase_summary: tasks 应为 44，实际 {:?}",
            ps["tasks"]
        ));
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

    // ⑩ m3_sync：五条齐全，每条都指 T-CK-E5（CH-6 的兑现链不许断）
    let sync = v["m3_sync"].as_array().ok_or("m3_sync: 不是数组")?;
    if sync.len() < 5 {
        return Err(format!("m3_sync: 只有 {} 条（§7.3 需五条）", sync.len()));
    }
    for s in sync {
        for f in ["item", "flip", "action", "gate"] {
            if !nonempty(s, f) {
                return Err(format!("m3_sync: 有条目缺 `{f}`"));
            }
        }
        if !s["gate"].as_str().unwrap_or_default().contains("T-CK-E5") {
            return Err(format!(
                "m3_sync: 条目 {:?} 没指 T-CK-E5",
                s["item"].as_str().unwrap_or_default()
            ));
        }
    }
    Ok(())
}

fn load() -> (Value, String) {
    let mp = manifest_path();
    let tp = tests_doc_path();
    let text =
        std::fs::read_to_string(&mp).unwrap_or_else(|e| panic!("读不到 {}：{e}", mp.display()));
    let v: Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} 不是合法 JSON：{e}", mp.display()));
    let doc =
        std::fs::read_to_string(&tp).unwrap_or_else(|e| panic!("读不到 {}：{e}", tp.display()));
    (v, doc)
}

/// 真文件必须通过 —— 这是「manifest 与冻结文档一致」的可执行判据。
#[test]
fn m4清单_与测试文档对得上且字段齐备() {
    let (v, doc) = load();
    assert_eq!(
        v["test_ids"].as_array().map(Vec::len),
        Some(63),
        "test_ids 应为 63 条（M4-tests §3 的声明数：甲 37 + 乙 5 + 丙 8 + 丁 5 + 反 8）"
    );
    assert_eq!(
        v["unsupported"].as_array().map(Vec::len),
        Some(18),
        "unsupported 应为 18 条（U-M4-01..11 + M3 承接 7 项）"
    );
    assert_eq!(
        v["gate"].as_object().map(serde_json::Map::len),
        Some(9),
        "gate 应为 9 条（CH-1..CH-9）"
    );
    check(&v, &doc).unwrap_or_else(|e| panic!("manifest 与冻结文档不一致：{e}"));
}

/// 合成反例：把真 manifest **逐项改坏**，每种都必须报错（否则断言是摆设）。
///
/// 最后两条改的是**文档**而不是 manifest（证明 `declared_ids`/`declared_unsupported`
/// 真的在读 `M4-tests.md`，而不是把 63/18 写死在代码里）。
#[test]
fn m4清单_合成反例逐条报错() {
    let (base, doc) = load();
    let cases: Vec<(&str, Value, &str)> = vec![
        (
            "删一条 test_id",
            {
                let mut v = base.clone();
                let a = v["test_ids"].as_array_mut().expect("array");
                a.retain(|t| t["id"] != json!("T-CK-31"));
                v
            },
            "test_ids",
        ),
        (
            "编一条不存在的 test_id",
            {
                let mut v = base.clone();
                v["test_ids"]
                    .as_array_mut()
                    .expect("array")
                    .push(json!({"id":"T-CK-99","gate":"CH-2","verdict":"pass","evidence":"rgoc/x/y.rs 的某测试"}));
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
                v["test_ids"][0]["gate"] = json!("CH-99");
                v
            },
            "test_ids",
        ),
        (
            "非 pass 的 test_id 没有缺口登记",
            {
                let mut v = base.clone();
                // 抹掉 G-M4-38 对 T-CK-A2 的点名（只留门禁），模拟「降级了但没留痕」
                let gaps = v["open_gaps"]["items"].as_array_mut().expect("array");
                for g in gaps {
                    if g["id"] == json!("G-M4-38") {
                        g["gate"] = json!("CH-9");
                    }
                }
                v
            },
            "test_ids",
        ),
        (
            "把 gate 的键换成 CH-10",
            {
                let mut v = base.clone();
                let g = v["gate"].as_object_mut().expect("obj");
                let ch9 = g.remove("CH-9").expect("CH-9");
                g.insert("CH-10".into(), ch9);
                v
            },
            "gate",
        ),
        (
            "某个 gate 标 fail 却没缺口登记",
            {
                let mut v = base.clone();
                v["gate"]["CH-5"]["status"] = json!("fail");
                v
            },
            "gate",
        ),
        (
            "gate 标 pass 但证据自认未兑现",
            {
                let mut v = base.clone();
                v["gate"]["CH-2"]["status"] = json!("pass");
                v
            },
            "gate",
        ),
        (
            "gate 的 evidence 换成空话",
            {
                let mut v = base.clone();
                v["gate"]["CH-6"]["evidence"] = json!("好了");
                v
            },
            "gate",
        ),
        (
            "gate 标 pass 但名下测试项未 pass 且无成对登记",
            {
                let mut v = base.clone();
                // CH-9 名下 T-CK-A2 本就 partial；把 G-M4-38 的门禁指针抹掉（只留测试项）
                // ⇒ ④ 仍满足（T-CK-A2 被点名），但 ③′ 的「成对」判据必须炸。
                let gaps = v["open_gaps"]["items"].as_array_mut().expect("array");
                for g in gaps {
                    if g["id"] == json!("G-M4-38") {
                        g["gate"] = json!("T-CK-A2");
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
                let a = v["unsupported"].as_array_mut().expect("array");
                a.retain(|u| u["id"] != json!("U-M4-05"));
                v
            },
            "unsupported",
        ),
        (
            "编一条文档没有的 unsupported",
            {
                let mut v = base.clone();
                v["unsupported"]
                    .as_array_mut()
                    .expect("array")
                    .push(json!({"id":"U-M4-99","item":"x","why":"y","planned_phase":"z","positive_evidence":"rgoc/a/b.rs"}));
                v
            },
            "unsupported",
        ),
        (
            "unsupported 的 planned_phase 空",
            {
                let mut v = base.clone();
                let a = v["unsupported"].as_array_mut().expect("array");
                let it = a
                    .iter_mut()
                    .find(|u| u["id"] == json!("U-M4-06"))
                    .expect("U-M4-06");
                it["planned_phase"] = json!("  ");
                v
            },
            "unsupported",
        ),
        (
            "unsupported 的 evidence 无指针",
            {
                let mut v = base.clone();
                let a = v["unsupported"].as_array_mut().expect("array");
                let it = a
                    .iter_mut()
                    .find(|u| u["id"] == json!("U-M4-01"))
                    .expect("U-M4-01");
                it["positive_evidence"] = json!("不报错就行");
                v
            },
            "unsupported",
        ),
        (
            "入选清单计数改大 1",
            {
                let mut v = base.clone();
                v["corpus_lists"]["selected"]["count"] = json!(76);
                v
            },
            "corpus_lists",
        ),
        (
            "候选与两清单不自洽",
            {
                let mut v = base.clone();
                v["corpus_lists"]["candidate"]["count"] = json!(90);
                v
            },
            "corpus_lists",
        ),
        (
            "清单文件名写错",
            {
                let mut v = base.clone();
                v["corpus_lists"]["wave_a"]["file"] = json!("tools/types-oracle/check-waveA.txt");
                v
            },
            "corpus_lists",
        ),
        (
            "改递归深度上限",
            {
                let mut v = base.clone();
                v["budget"]["recursion_depth_max"] = json!(999);
                v
            },
            "budget",
        ),
        (
            "改每文件超时",
            {
                let mut v = base.clone();
                v["budget"]["per_file_check_timeout_s"] = json!(30);
                v
            },
            "budget",
        ),
        (
            "effort_calendar 缺 effort_note",
            {
                let mut v = base.clone();
                v["budget"]["effort_calendar"]
                    .as_object_mut()
                    .expect("obj")
                    .remove("effort_note");
                v
            },
            "budget",
        ),
        (
            "实测测试数两处不一致",
            {
                let mut v = base.clone();
                v["phase_summary"]["workspace_tests"]["passed"] = json!(1600);
                v
            },
            "budget",
        ),
        (
            "实测测试数缩水到 1000",
            {
                let mut v = base.clone();
                v["budget"]["measured"]["workspace_tests_passed"] = json!(1000);
                v["phase_summary"]["workspace_tests"]["passed"] = json!(1000);
                v
            },
            "budget",
        ),
        (
            "coverage 计数与实际条数打架",
            {
                let mut v = base.clone();
                v["coverage"]["test_ids_total"] = json!(1);
                v
            },
            "coverage",
        ),
        (
            "coverage 的矩阵行数写错",
            {
                let mut v = base.clone();
                v["coverage"]["matrix_rows"] = json!(8);
                v
            },
            "coverage",
        ),
        (
            "derived_gates 里塞野 ID",
            {
                let mut v = base.clone();
                v["coverage"]["derived_gates"]["items"]
                    .as_array_mut()
                    .expect("array")
                    .push(json!("T-CK-99"));
                v
            },
            "coverage",
        ),
        (
            "open_gaps 有条目缺 gate",
            {
                let mut v = base.clone();
                v["open_gaps"]["items"][0]
                    .as_object_mut()
                    .expect("obj")
                    .remove("gate");
                v
            },
            "open_gaps",
        ),
        (
            "open_gaps 的 ID 重复",
            {
                let mut v = base.clone();
                let a = v["open_gaps"]["items"].as_array_mut().expect("array");
                let dup = a[0].clone();
                a.push(dup);
                v
            },
            "open_gaps",
        ),
        (
            "顶层少了 m3_sync 一节",
            {
                let mut v = base.clone();
                v.as_object_mut().expect("obj").remove("m3_sync");
                v
            },
            "structure",
        ),
        (
            "stage 写成 M3",
            {
                let mut v = base.clone();
                v["stage"] = json!("M3");
                v
            },
            "stage",
        ),
        (
            "m3_sync 有条目缺 action",
            {
                let mut v = base.clone();
                v["m3_sync"][0]
                    .as_object_mut()
                    .expect("obj")
                    .remove("action");
                v
            },
            "m3_sync",
        ),
        (
            "m3_sync 有条目不指 T-CK-E5",
            {
                let mut v = base.clone();
                v["m3_sync"][1]["gate"] = json!("CH-1");
                v
            },
            "m3_sync",
        ),
        (
            "phase_summary 任务数写错",
            {
                let mut v = base.clone();
                v["phase_summary"]["tasks"] = json!(43);
                v
            },
            "phase_summary",
        ),
        (
            "有门禁未 pass 却自称 done",
            {
                let mut v = base.clone();
                v["phase_summary"]["status"] = json!("done");
                v
            },
            "phase_summary",
        ),
        (
            "gate_scripts 少于五条",
            {
                let mut v = base.clone();
                v["phase_summary"]["gate_scripts"] = json!(["bash a.sh", "bash b.sh"]);
                v
            },
            "phase_summary",
        ),
    ];

    assert!(
        cases.len() >= 10,
        "合成反例只有 {} 条（T-CK-E3 要求 ≥10）",
        cases.len()
    );

    for (name, v, want) in &cases {
        let err = match check(v, &doc) {
            Ok(()) => panic!("「{name}」竟然通过了 —— 这条断言没牙"),
            Err(e) => e,
        };
        assert!(
            err.starts_with(want),
            "「{name}」报错前缀该是 {want:?}，实际 {err:?}"
        );
    }

    // ── 文档侧的两条：改 `M4-tests.md` 也必须被抓到 ──
    let drop_line = doc
        .lines()
        .filter(|l| !l.starts_with("| `T-CK-61`"))
        .collect::<Vec<_>>()
        .join("\n");
    let err = match check(&base, &drop_line) {
        Ok(()) => panic!("文档里删掉 T-CK-61 竟然通过了"),
        Err(e) => e,
    };
    assert!(err.contains("T-CK-61"), "{err}");

    // 把 `U-M3-11/12/13` 的斜杠简写改坏：声明集应当**变小**，于是与 manifest 不等。
    let drop_u = doc.replacen("`U-M3-11/12/13`", "`U-M3-11`", 1);
    let err = match check(&base, &drop_u) {
        Ok(()) => panic!("文档里抹掉 U-M3-12/13 竟然通过了"),
        Err(e) => e,
    };
    assert!(err.contains("unsupported"), "{err}");
}
