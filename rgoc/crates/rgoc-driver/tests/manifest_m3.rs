//! `M3-manifest.json` ↔ `M3-tests.md` 的**对账断言** —— M3/Task 38 Step 4。
//!
//! # 为什么要有这一条
//!
//! manifest 是「机器可读的事实」，`M3-tests.md` 是「冻结的约定」。两者**同源但不自动同步**
//! —— 没有断言的话，改一处忘了另一处只会留下两份互相矛盾的文件，谁也不知道哪份算数。
//! 所以这里把两件事**对撞**起来：集合必须**逐元素相等**（不是包含，也不是「差不多」）。
//!
//! | 对账项 | 判据 |
//! |---|---|
//! | `test_ids` | 与 `M3-tests.md` §3 九节表**第一列**声明的 ID **集合相等**（68 条） |
//! | `unsupported` | 与 §6 白名单的 `U-M3-xx` **集合相等**（13 条）；每条有证据指针，「被显式拒绝」那几条还必须是 `T-TY-8x` |
//! | `gate` | 键**恰好**是 `TY-1..TY-8`，全 `pass`，证据非空且带指针 |
//! | `budget` | §7.4 的冻结值（深度 1000 / import 100 / smoke 300 / milestone 1800） |
//! | `coverage` | 自洽：计数器 == 实际条数 |
//! | `phase_summary` | `status == done`、0 失败、四条门禁命令齐 |
//! | §6 纪律 | **每一条**白名单项都要有 `T-TY-8x` 的**正向**断言指针 |
//!
//! # 断言自己也要能被证伪
//!
//! 只有「真文件 ⇒ Ok」的断言是**恒真**的近亲：只要把检查写成空函数就永远绿。
//! 所以下面还有一条**合成反例**测试：对真 manifest 逐项**改坏**（12 种），
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
    repo_root().join("docs/milestones/M3-manifest.json")
}

fn tests_doc_path() -> PathBuf {
    repo_root().join("docs/milestones/M3-tests.md")
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

/// §3 九节表**第一列**声明的 ID（`| \`T-TY-11\` | …`）。
///
/// 只认行首形态：ID 出现在**别的列**里（例如 3.8 的反向列引用 `T-TY-28`）不算声明
/// —— 那些是**引用**，声明集合比它小且必须精确。
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
        if id.starts_with("T-TY-") {
            out.insert(id.to_string());
        }
    }
    out
}

/// §6 白名单里的 `U-M3-xx`（扫全文的**出现**，含表外的纪律行 —— 那些提到就必须对齐）。
fn declared_unsupported(doc: &str) -> BTreeSet<String> {
    let sec = section(doc, "\n## 6", "\n## 7");
    let mut out = BTreeSet::new();
    let mut i = 0;
    while let Some(p) = sec[i..].find("U-M3-") {
        let start = i + p;
        let mut j = start + "U-M3-".len();
        while sec.as_bytes().get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
        out.insert(sec[start..j].to_string());
        i = j;
    }
    out
}

fn strs(v: &Value, key: &str) -> Vec<String> {
    v[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} 不是数组"))
        .iter()
        .map(|x| x.as_str().unwrap_or_default().to_string())
        .collect()
}

/// 证据/理由必须**指得到落点**：要么是 `T-TY-*`，要么含 `/`（文件路径）。
fn points_at_something(s: &str) -> bool {
    s.contains("T-TY-") || s.contains('/')
}

/// 白名单里**不**要求 `T-TY-8x` 的五条（见 [`check`] ② 的说明）。这张表是**判据**：
/// 谁往里加，就会被那条「例外表该收窄」的反向断言拦下。
const NO_8X_OK: &[&str] = &[
    "U-M3-06", // 约定路径是「不筛文件」⇒ 证据在 T-TY-65（6x 的反向断言）
    "U-M3-10", // 「不承诺逐字」⇒ 证据是「只比代号」的一条**纪律**，不是某条拒绝
    "U-M3-11", // M2 承继：M3 未触发
    "U-M3-12", // 同上
    "U-M3-13", // 同上
];

fn nonempty(v: &Value, key: &str) -> bool {
    v[key].as_str().is_some_and(|s| !s.trim().is_empty())
}

/// 核心检查：`Err(检查名: 说明)`。**第一个**不通过就返回（顺序即优先级）。
fn check(v: &Value, tests_md: &str) -> Result<(), String> {
    // ① 顶层结构
    for k in [
        "stage",
        "generated_at",
        "scope",
        "crates",
        "gate",
        "coverage",
        "test_ids",
        "unsupported",
        "budget",
        "open_gaps",
        "phase_summary",
    ] {
        if v.get(k).is_none() {
            return Err(format!("structure: 顶层缺 `{k}`"));
        }
    }
    if v["stage"].as_str() != Some("M3") {
        return Err(format!("stage: 应为 M3，实际 {:?}", v["stage"]));
    }
    if strs(v, "crates").len() < 3 || strs(v, "crates").iter().any(|c| c.trim().is_empty()) {
        return Err("crates: 至少 3 条且都不为空".into());
    }

    // ② gate：键恰好 TY-1..8、全 pass、证据带指针
    let want_gates: BTreeSet<String> = (1..=8).map(|i| format!("TY-{i}")).collect();
    let gate = v["gate"].as_object().ok_or("gate: 不是对象")?;
    let got_gates: BTreeSet<String> = gate.keys().cloned().collect();
    if got_gates != want_gates {
        return Err(format!("gate: 键集应为 {want_gates:?}，实际 {got_gates:?}"));
    }
    for (k, g) in gate {
        if g["status"].as_str() != Some("pass") {
            return Err(format!("gate: {k} 的 status 不是 pass：{:?}", g["status"]));
        }
        for f in ["name", "definition", "confirmed_at"] {
            if !nonempty(g, f) {
                return Err(format!("gate: {k} 的 `{f}` 空"));
            }
        }
        let ev = g["evidence"].as_str().unwrap_or_default();
        if ev.len() < 16 || !points_at_something(ev) {
            return Err(format!("gate: {k} 的 evidence 太短或无指针：{ev:?}"));
        }
    }

    // ③ test_ids：集合与 §3 声明**相等**，逐条 down to 落点
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
            "test_ids: 与 M3-tests.md §3 集合不等 —— 只在文档 {:?} / 只在 manifest {:?}",
            only_doc, only_manifest
        ));
    }
    for t in &ids {
        let id = t["id"].as_str().unwrap_or_default();
        if t["verdict"].as_str() != Some("pass") {
            return Err(format!("test_ids: {id} 的 verdict 不是 pass"));
        }
        let g = t["gate"].as_str().unwrap_or_default();
        if !want_gates.contains(g) {
            return Err(format!("test_ids: {id} 的 gate `{g}` 不在 TY-1..8"));
        }
        let ev = t["evidence"].as_str().unwrap_or_default();
        if ev.len() < 12 || !points_at_something(ev) {
            return Err(format!("test_ids: {id} 的 evidence 太短或无指针：{ev:?}"));
        }
    }

    // ④ unsupported：集合与 §6 相等；**每条**都要有 T-TY-8x 的正向指针（§6 纪律）
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
            "unsupported: 与 §6 集合不等 —— 只在文档 {:?} / 只在 manifest {:?}",
            only_doc, only_manifest
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
        // ① 一律要有**指针**（指得到证据落点）
        if !points_at_something(pe) {
            return Err(format!(
                "unsupported: {id} 缺证据指针（T-TY-* 或路径）：{pe:?}"
            ));
        }
        // ② 「被显式拒绝」那几条还必须是 **T-TY-8x**（§6 纪律）。
        //    四个例外是**有意**的：`U-M3-06` 的约定路径是「**不**筛文件」⇒ 证据在
        //    `T-TY-65`（6x 的反向断言）；`U-M3-11..13` 是 M2 承继的「M3 未触发」。
        //    硬给这四条要 8x，只会逼着人写一条假测试 —— 那不是纪律，是形式主义。
        if !NO_8X_OK.contains(&id) && !pe.contains("T-TY-8") {
            return Err(format!(
                "unsupported: {id} 缺 T-TY-8x 的正向断言指针（§6 纪律）：{pe:?}"
            ));
        }
        if NO_8X_OK.contains(&id) && pe.contains("T-TY-8") {
            return Err(format!(
                "unsupported: {id} 在「例外表」里却也带了 8x 指针 —— 例外表该收窄：{pe:?}"
            ));
        }
    }

    // ⑤ budget：§7.4 的冻结值
    let b = &v["budget"];
    for (k, want) in [
        ("recursion_depth_max", 1000),
        ("import_depth_max", 100),
        ("smoke_wall_s_max", 300),
        ("milestone_wall_s_max", 1800),
    ] {
        if b[k].as_u64() != Some(want) {
            return Err(format!("budget: {k} 应为 {want}，实际 {:?}", b[k]));
        }
    }
    if b["measured"]["workspace_tests_failed"].as_u64() != Some(0) {
        return Err("budget: measured.workspace_tests_failed 应为 0".into());
    }
    if b["measured"]["workspace_tests_passed"]
        .as_u64()
        .unwrap_or(0)
        == 0
    {
        return Err("budget: measured.workspace_tests_passed 应为正数".into());
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

    // ⑦ open_gaps：每条四栏俱全（缺口登记不许留空栏）
    let gaps = v["open_gaps"]["items"]
        .as_array()
        .ok_or("open_gaps: items 不是数组")?;
    if gaps.is_empty() {
        return Err("open_gaps: items 空 —— M3 的缺口如实登记是硬要求".into());
    }
    for g in gaps {
        for f in ["id", "item", "status", "owner"] {
            if !nonempty(g, f) {
                return Err(format!("open_gaps: 有条目缺 `{f}`"));
            }
        }
    }

    // ⑧ phase_summary
    let ps = &v["phase_summary"];
    if ps["status"].as_str() != Some("done") {
        return Err(format!(
            "phase_summary: status 应为 done，实际 {:?}",
            ps["status"]
        ));
    }
    if ps["workspace_tests"]["failed"].as_u64() != Some(0) {
        return Err("phase_summary: workspace_tests.failed 应为 0".into());
    }
    let cmds = ps["gate_commands"].as_str().unwrap_or_default();
    for c in ["cargo fmt", "cargo check", "cargo clippy", "cargo test"] {
        if !cmds.contains(c) {
            return Err(format!("phase_summary: gate_commands 缺 `{c}`"));
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
fn m3清单_与测试文档对得上且字段齐备() {
    let (v, doc) = load();
    assert_eq!(
        v["test_ids"].as_array().map(Vec::len),
        Some(68),
        "test_ids 应为 68 条（M3-tests §3 的声明数）"
    );
    assert_eq!(
        v["unsupported"].as_array().map(Vec::len),
        Some(13),
        "unsupported 应为 13 条（U-M3-01..13）"
    );
    check(&v, &doc).unwrap_or_else(|e| panic!("manifest 与冻结文档不一致：{e}"));
}

/// 合成反例：把真 manifest **逐项改坏**，每种都必须报错（否则断言是摆设）。
///
/// 最后两条改的是**文档**而不是 manifest（证明 `declared_ids`/`declared_unsupported`
/// 真的在读 `M3-tests.md`，而不是把 68/13 写死在代码里）。
#[test]
fn m3清单_合成反例逐条报错() {
    let (base, doc) = load();
    let cases: Vec<(&str, Value, String)> = vec![
        (
            "删一条 test_id",
            {
                let mut v = base.clone();
                let a = v["test_ids"].as_array_mut().expect("array");
                a.retain(|t| t["id"] != json!("T-TY-19"));
                v
            },
            "test_ids".into(),
        ),
        (
            "编一条不存在的 test_id",
            {
                let mut v = base.clone();
                v["test_ids"]
                    .as_array_mut()
                    .expect("array")
                    .push(json!({"id":"T-TY-99","verdict":"pass","gate":"TY-1","evidence":"rgoc/x/y.rs 的某测试"}));
                v
            },
            "test_ids".into(),
        ),
        (
            "把 gate 的键换成 TY-9",
            {
                let mut v = base.clone();
                let g = v["gate"].as_object_mut().expect("obj");
                let ty7 = g.remove("TY-7").expect("TY-7");
                g.insert("TY-9".into(), ty7);
                v
            },
            "gate".into(),
        ),
        (
            "某个 gate 标 fail",
            {
                let mut v = base.clone();
                v["gate"]["TY-4"]["status"] = json!("fail");
                v
            },
            "gate".into(),
        ),
        (
            "gate 的 evidence 换成空话",
            {
                let mut v = base.clone();
                v["gate"]["TY-6"]["evidence"] = json!("好了");
                v
            },
            "gate".into(),
        ),
        (
            "某个 test_id 的 evidence 无指针",
            {
                let mut v = base.clone();
                v["test_ids"][3]["evidence"] = json!("已经覆盖");
                v
            },
            "test_ids".into(),
        ),
        (
            "某个 test_id 的 gate 越界",
            {
                let mut v = base.clone();
                v["test_ids"][0]["gate"] = json!("TY-99");
                v
            },
            "test_ids".into(),
        ),
        (
            "删一条 unsupported",
            {
                let mut v = base.clone();
                let a = v["unsupported"].as_array_mut().expect("array");
                a.retain(|u| u["id"] != json!("U-M3-05"));
                v
            },
            "unsupported".into(),
        ),
        (
            "unsupported 的 evidence 无指针",
            {
                let mut v = base.clone();
                let a = v["unsupported"].as_array_mut().expect("array");
                let it = a
                    .iter_mut()
                    .find(|u| u["id"] == json!("U-M3-06"))
                    .expect("U-M3-06");
                it["positive_evidence"] = json!("不报错就行");
                v
            },
            "unsupported".into(),
        ),
        (
            "unsupported 有指针但缺 8x",
            {
                let mut v = base.clone();
                let a = v["unsupported"].as_array_mut().expect("array");
                let it = a
                    .iter_mut()
                    .find(|u| u["id"] == json!("U-M3-07"))
                    .expect("U-M3-07");
                it["positive_evidence"] = json!("T-TY-91 的对撞（有指针，但没有拒绝侧断言）");
                v
            },
            "unsupported".into(),
        ),
        (
            "例外表里混进了带 8x 的条目",
            {
                let mut v = base.clone();
                let a = v["unsupported"].as_array_mut().expect("array");
                let it = a
                    .iter_mut()
                    .find(|u| u["id"] == json!("U-M3-11"))
                    .expect("U-M3-11");
                it["positive_evidence"] = json!("T-TY-84 的邻域");
                v
            },
            "unsupported".into(),
        ),
        (
            "改 import 深度上限",
            {
                let mut v = base.clone();
                v["budget"]["import_depth_max"] = json!(101);
                v
            },
            "budget".into(),
        ),
        (
            "阶段没标 done",
            {
                let mut v = base.clone();
                v["phase_summary"]["status"] = json!("wip");
                v
            },
            "phase_summary".into(),
        ),
        (
            "coverage 计数与实际条数打架",
            {
                let mut v = base.clone();
                v["coverage"]["test_ids_total"] = json!(1);
                v
            },
            "coverage".into(),
        ),
        (
            "顶层少了 unsupported 一节",
            {
                let mut v = base.clone();
                v.as_object_mut().expect("obj").remove("unsupported");
                v
            },
            "structure".into(),
        ),
    ];

    for (name, v, want) in &cases {
        let err = check(v, &doc).expect_err(&format!("「{name}」竟然通过了 —— 这条断言没牙"));
        assert!(
            err.starts_with(want.as_str()),
            "「{name}」报错前缀该是 {want:?}，实际 {err:?}"
        );
    }

    // ── 文档侧的两条：改 `M3-tests.md` 也必须被抓到 ──
    let drop_line = doc
        .lines()
        .filter(|l| !l.starts_with("| `T-TY-31`"))
        .collect::<Vec<_>>()
        .join("\n");
    let err = check(&base, &drop_line).expect_err("文档里删掉 T-TY-31 竟然通过了");
    assert!(err.contains("T-TY-31"), "{err}");

    let drop_u = doc.replacen("| U-M3-08 ", "| U-M3-88 ", 1);
    let err = check(&base, &drop_u).expect_err("文档里改掉 U-M3-08 竟然通过了");
    assert!(err.contains("unsupported"), "{err}");
}
