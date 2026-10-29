//! `M12-manifest.json` 的对账（M12-plan.md Task 26）。
//!
//! # 为什么清单也要有判据
//!
//! M9 的教训（沿用 M10/M11 的同一口径）：`manifest.measured` 是**自报**，
//! 不是证据 —— 清单一旦能说谎，后面所有「已完成」都不可信。这里做三件事：
//!
//! 1. **结构对账**：门禁 G12-1..13 / 未支持面 U-M12-01..12 / 门禁面 116 =
//!    run 71 + compile 40 + errorcheck 5 / 拒绝面 76，且语料条数与**分类装置
//!    现算**的清单一致（`.superpowers/m12-lists` 在 gitignore 里，所以
//!    这里自己跑 `m12-classify.sh` 重算 —— 不抄第二份）；
//! 2. **数字复现**（M12 特有）：`measured.gen_conc_three_states` 的 4 语料
//!    × 三态必须能**从一次真实跑里原样读出来**（逐字节相同）；
//! 3. **合成反例都必须红**：改坏清单一处，对账必须**指名道姓**地报错。
//!
//! # M12 新增的一条：`.sh` 证据也要在盘上
//!
//! M12a 有两族（rejected / errorcheck）的逐文件判据**只在装置脚本里**
//! （`run-native-typeparam.sh`）—— evidence 里点名了它，就必须查它存在，
//! 否则「判据在脚本里」会变成「判据可以不存在」。

mod support;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// 现算七份清单（判据自己跑分类装置，不信任盘上缓存）。
fn classify(out: &Path) {
    let st = Command::new("bash")
        .arg(repo_root().join("tools/run-oracle/m12-classify.sh"))
        .arg("--out")
        .arg(out)
        .status()
        .expect("跑不了 m12-classify.sh（bash 不在 PATH？）");
    assert!(st.success(), "分类装置自己失败了（划分不自洽？）");
}

/// 读一份清单（一行一个文件名）。
fn read_list(dir: &Path, name: &str) -> Vec<String> {
    let p = dir.join(format!("typeparam-{name}.txt"));
    let s = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不了 {}：{e}", p.display()));
    s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// `evidence` 里提到的 `<名字>.rs`（M10 起的同款扫描）。
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

/// `evidence` 里提到的 `tools/run-oracle/<名字>.sh`（M12 新增：装置脚本也要在盘上）。
fn mentioned_sh(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter(|w| w.ends_with(".sh") && w.contains('/'))
        .map(|w| {
            w.trim_matches(|c: char| !c.is_ascii_graphic() || c == ')' || c == ',')
                .to_string()
        })
        .filter(|w| w.ends_with(".sh"))
        .collect()
}

/// 所有 crate 下的 `.rs` 测试文件名集合（M11 的同款）。
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

/// 对账。返回**全部**问题（不是第一个）。
fn reconcile(m: &Value, lists: &Path) -> Vec<String> {
    let mut errs = Vec::new();

    if m["stage"].as_str() != Some("M12") {
        errs.push("stage 必须是 M12".into());
    }

    // ── ① 门禁：13 条 G12-1..G12-13（1..6 = a 线，7..9 = b 线，10..13 = c 线）──
    let want: BTreeSet<String> = (1..=13).map(|i| format!("G12-{i}")).collect();
    let gate = m["gate"].as_array().expect("gate 应是数组");
    if gate.len() != want.len() {
        errs.push(format!("gate 应有 {} 条，实际 {}", want.len(), gate.len()));
    }
    let disk = test_files_on_disk();
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
                if !disk.contains(&f) {
                    errs.push(format!("{id} 的证据提到 {f}，但盘上没有这个测试文件"));
                }
            }
            for f in mentioned_sh(ev) {
                if !repo_root().join(&f).is_file() {
                    errs.push(format!("{id} 的证据提到 {f}，但盘上没有这个脚本"));
                }
            }
        }
        // `gap` 必须出现（哪怕是 `—`）。
        if g["gap"].is_null() {
            errs.push(format!("{id} 没有 gap 栏（没有就写 `—`）"));
        }
    }
    if ids != want {
        errs.push(format!("门禁 id 集合不对：{ids:?}"));
    }

    // ── ② 未支持面：10 条 U-M12-01..10 ──
    let uns = m["unsupported"].as_array().expect("unsupported 应是数组");
    if uns.len() != 12 {
        errs.push(format!("unsupported 应有 12 条，实际 {}", uns.len()));
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
    let want_u: BTreeSet<String> = (1..=12).map(|i| format!("U-M12-{i:02}")).collect();
    if uids != want_u {
        errs.push(format!("unsupported id 集合不对：{uids:?}"));
    }

    // ── ③ 缺口：pass 的门禁不许挂在 open_gaps；open_gaps 必须写 detail ──
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

    // ── ⑤ 语料：门禁面 116 = 71+40+5，与**现算**的分类一致；拒绝面 76 ──
    let gf = &m["corpus_lists"]["gate_face"];
    let mut sum = 0u64;
    for fam in ["run", "compile", "errorcheck"] {
        let Some(n) = gf[fam]["count"].as_u64() else {
            errs.push(format!("gate_face.{fam}.count 缺失"));
            continue;
        };
        let actual = read_list(lists, fam).len() as u64;
        if n != actual {
            errs.push(format!(
                "gate_face.{fam}.count = {n}，但 m12-classify.sh 现算是 {actual}"
            ));
        }
        sum += n;
    }
    let total = gf["total"].as_u64().unwrap_or(0);
    if total != 116 {
        errs.push(format!(
            "门禁面 total 必须是 116（run 71 + compile 40 + errorcheck 5），实际 {total}"
        ));
    }
    if sum != total {
        errs.push(format!("gate_face 三族之和 {sum} ≠ total {total}"));
    }
    let rej = m["corpus_lists"]["rejected"]["count"].as_u64().unwrap_or(0);
    let actual_rej = read_list(lists, "rejected").len() as u64;
    if rej != 76 {
        errs.push(format!("拒绝面 rejected.count 必须是 76，实际 {rej}"));
    }
    if rej != actual_rej {
        errs.push(format!(
            "拒绝面 rejected.count = {rej}，但 m12-classify.sh 现算是 {actual_rej}"
        ));
    }

    // ── ⑥ measured：必填项 + 自洽 ──
    let measured = &m["measured"];
    for key in [
        "workspace",
        "gate_face_counts",
        "typeparam_family",
        "run_native_interp",
        "min_checked",
        "gen_conc_three_states",
        "m11_regression",
        "instance_determinism",
        "no_typeparam_residue",
        "mutations",
    ] {
        if measured[key].is_null() {
            errs.push(format!("measured 缺 {key}"));
        }
    }
    let ws = measured["workspace"].as_str().unwrap_or("");
    let parts: Vec<&str> = ws.split_whitespace().collect();
    // 形状：「246 suite / 2324 passed / 0 failed / 2 ignored」= 11 段
    let shape_ok = parts.len() == 11
        && parts[1] == "suite"
        && parts[2] == "/"
        && parts[4] == "passed"
        && parts[5] == "/"
        && parts[6] == "0"
        && parts[7] == "failed"
        && parts[8] == "/"
        && parts[10] == "ignored"
        && parts[0].parse::<u64>().is_ok()
        && parts[3].parse::<u64>().is_ok()
        && parts[9].parse::<u64>().is_ok();
    if !shape_ok {
        errs.push(format!(
            "measured.workspace = {ws:?} 不是「N suite / M passed / 0 failed / K ignored」的形状"
        ));
    }
    if measured["min_checked"]
        .as_str()
        .is_none_or(|s| !s.contains("108"))
    {
        errs.push("measured.min_checked 必须含 108（t_gen_05 的账）".into());
    }
    if measured["mutations"]
        .as_str()
        .is_none_or(|s| !s.contains("3/3"))
    {
        errs.push("measured.mutations 必须含 3/3".into());
    }

    // ── ⑥b b 线读数：键必须齐，形态要能核 ──
    //
    // 数字**复现**留在 `t_ref_06` 自己（它断言 `collections > 0` 且
    // `bad_ptr == 0`），这里只钉「清单里写了这个数、而且形状对」——
    // 抄第二份 fixture 进对账测试就是 `R-51-1` 说的「文档与副本各一份」，
    // 迟早会漂。
    for key in [
        "reflect_face",
        "ref_metadata",
        "ref_call",
        "ref_write",
        "ref_gc",
        "mutations_b",
    ] {
        if measured[key].is_null() {
            errs.push(format!("measured 缺 {key}（b 线）"));
        }
    }
    let gc = measured["ref_gc"].as_str().unwrap_or("");
    let collections = gc
        .split_once("collections=")
        .and_then(|(_, rest)| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|d| d.parse::<u64>().ok());
    if collections.is_none_or(|v| v == 0) {
        errs.push(
            "measured.ref_gc 必须写出 `collections=<正整数>`（stress 态一次都没收 ⇒ 判据恒真）"
                .into(),
        );
    }
    if !gc.contains("bad_ptr=0") {
        errs.push("measured.ref_gc 必须含 `bad_ptr=0`".into());
    }
    if measured["mutations_b"]
        .as_str()
        .is_none_or(|s| !s.contains("3/3"))
    {
        errs.push("measured.mutations_b 必须含 3/3".into());
    }
    if measured["reflect_face"]
        .as_str()
        .is_none_or(|s| !s.contains("12"))
    {
        errs.push("measured.reflect_face 必须写出顶层导出符号数（12 = 6 类型 + 6 函数）".into());
    }

    // ── ⑥c c 线读数：三条优化各自要有**可复跑的读数**，不是「做了」──
    //
    // 形态钉法同 ⑥b：数字的复现留在各自的测试里（`t_opt_01` / `escape_summary`
    // 的六条 / `t_map_11`+`t_map_12`），这里只钉「清单里写了这个数」。
    for key in [
        "opt_inline",
        "opt_escape",
        "opt_swiss",
        "opt_dict",
        "mutations_c",
    ] {
        if measured[key].is_null() {
            errs.push(format!("measured 缺 {key}（c 线）"));
        }
    }
    if measured["mutations_c"]
        .as_str()
        .is_none_or(|s| !s.contains("3/3"))
    {
        errs.push("measured.mutations_c 必须含 3/3".into());
    }
    // 四条判据的读数都必须在，且必须带**数字**（只写「下降了」等于没写）。
    for (key, must) in [
        ("opt_inline", "951616"),
        ("opt_escape", "45"),
        ("opt_swiss", "1.109"),
        ("opt_dict", "1408"),
    ] {
        if measured[key].as_str().is_none_or(|s| !s.contains(must)) {
            errs.push(format!("measured.{key} 必须含读数 {must}"));
        }
    }
    // `opt_dict` 是**切断**决策，清单里必须写明白，否则下一个人会当它「还没做」。
    if measured["opt_dict"]
        .as_str()
        .is_none_or(|s| !s.contains("切断"))
    {
        errs.push("measured.opt_dict 必须写明这是**切断**决策（不是没做）".into());
    }

    errs
}

#[test]
fn t_manifest_12_真清单必须自洽() {
    let m = read_json("docs/milestones/M12-manifest.json");
    let lists = std::env::temp_dir().join("rgoc-m12-manifest-lists");
    classify(&lists);
    let errs = reconcile(&m, &lists);
    assert!(
        errs.is_empty(),
        "manifest 对账失败 {} 条：\n{}",
        errs.len(),
        errs.join("\n")
    );
}

/// `measured.gen_conc_three_states` 必须能**跑出来**（M12 特有）。
///
/// 4 语料 × 三态（off / on / stress），逐字节相同 + 退出码 0 —— 与
/// `t_gen_11` 同一形态；这里的分母（4）从清单字符串里**读出来**，
/// 清单改成别的数时这条判据要么跟着改、要么红。
///
/// stress 档用 `RGOC_GC_STRESS=1`（不是装置默认的 64）：gen_conc 只分配
/// 6 次，间隔 8 时一次 GC 都不触发（Task 23 实测，判据恒真的坑）。
#[test]
fn t_manifest_12_泛型并发三态必须能复现() {
    let m = read_json("docs/milestones/M12-manifest.json");
    let s = m["measured"]["gen_conc_three_states"]
        .as_str()
        .expect("measured.gen_conc_three_states");
    let n: usize = s
        .split(|c: char| !c.is_ascii_digit())
        .find(|p| !p.is_empty())
        .and_then(|p| p.parse().ok())
        .expect("gen_conc_three_states 的第一个数字 = 语料条数");
    assert_eq!(n, 4, "清单说 4 语料；改了语料数必须连这条判据一起改");

    const GEN_CONC: &str = include_str!("fixtures/m12/gen_conc.go");
    const GEN_QUEUE: &str = include_str!("fixtures/m12/gen_queue.go");
    let cases: Vec<(&str, String)> = vec![
        (
            "append.go",
            std::fs::read_to_string(repo_root().join("go_source_code/test/typeparam/append.go"))
                .expect("读 append.go"),
        ),
        (
            "issue47901.go",
            std::fs::read_to_string(
                repo_root().join("go_source_code/test/typeparam/issue47901.go"),
            )
            .expect("读 issue47901.go"),
        ),
        ("gen_conc.go", GEN_CONC.to_string()),
        ("gen_queue.go", GEN_QUEUE.to_string()),
    ];
    assert_eq!(cases.len(), n);

    let mut bad: Vec<String> = Vec::new();
    for (name, src) in &cases {
        let mut seen: Option<(String, String, i32)> = None;
        for (tag, env) in [
            ("off", vec![("RGOC_GC", "0")]),
            ("on", vec![]),
            ("stress", vec![("RGOC_GC_STRESS", "1")]),
        ] {
            let out =
                build_and_run_env(&format!("m12m{name}{tag}"), src, &env).unwrap_or_else(|e| {
                    panic!("`{name}` 的 {tag} 态跑不起来（清单说三态都 ok）：\n{e}")
                });
            if out.exit != 0 {
                bad.push(format!("{name} [{tag}]：退出码 {}", out.exit));
            }
            match &seen {
                None => seen = Some((out.stdout, out.stderr, out.exit)),
                Some((o, e, x)) => {
                    if out.stdout != *o {
                        bad.push(format!("{name} [{tag}]：stdout 与 off 态不同"));
                    }
                    if out.stderr != *e {
                        bad.push(format!("{name} [{tag}]：stderr 与 off 态不同"));
                    }
                    if out.exit != *x {
                        bad.push(format!("{name} [{tag}]：退出码 {} ≠ off 态 {x}", out.exit));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "三态不可复现：\n{}", bad.join("\n"));
}

/// 把清单改坏一处，断言**有指名道姓的错误**。
fn expect_red(what: &str, mutate: impl FnOnce(&mut Value), needle: &str) {
    let mut m = read_json("docs/milestones/M12-manifest.json");
    mutate(&mut m);
    let lists = std::env::temp_dir().join("rgoc-m12-manifest-lists");
    let errs = reconcile(&m, &lists);
    assert!(!errs.is_empty(), "改坏「{what}」之后居然还是绿的");
    assert!(
        errs.iter().any(|e| e.contains(needle)),
        "改坏「{what}」后应报出含 {needle:?} 的错误，实际：{errs:?}"
    );
}

#[test]
fn t_manifest_12_合成反例都必须红() {
    // classify 只在「真清单必须自洽」里跑一次；这里复用同目录（不存在时先建）。
    let lists = std::env::temp_dir().join("rgoc-m12-manifest-lists");
    if !lists.join("typeparam-run.txt").exists() {
        classify(&lists);
    }

    expect_red(
        "门禁数不对",
        |m| {
            m["gate"].as_array_mut().unwrap().pop();
        },
        "gate 应有 13 条",
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
            m["gate"][0]["evidence"] = Value::String("m12_nonexistent.rs 里那几条".into());
        },
        "但盘上没有这个测试文件",
    );

    expect_red(
        "证据里点名的装置脚本不存在",
        |m| {
            m["gate"][0]["evidence"] =
                Value::String("tools/run-oracle/run-nonexistent.sh 跑出来的".into());
        },
        "但盘上没有这个脚本",
    );

    expect_red(
        "门禁面 total 改错",
        |m| {
            m["corpus_lists"]["gate_face"]["total"] = Value::Number(115.into());
        },
        "必须是 116",
    );

    expect_red(
        "语料计数与现算不一致",
        |m| {
            m["corpus_lists"]["gate_face"]["run"]["count"] = Value::Number(70.into());
        },
        "现算是 71",
    );

    expect_red(
        "拒绝面改错",
        |m| {
            m["corpus_lists"]["rejected"]["count"] = Value::Number(75.into());
        },
        "必须是 76",
    );

    expect_red(
        "未支持面少一条",
        |m| {
            m["unsupported"].as_array_mut().unwrap().pop();
        },
        "unsupported 应有 12 条",
    );

    expect_red(
        "关了缺口却没写 resolution",
        |m| {
            m["closed_gaps"][0]["resolution"] = Value::String(String::new());
        },
        "关了却没写 resolution",
    );

    expect_red(
        "open_gaps 只写了「有个缺口」",
        |m| {
            m["open_gaps"][0]["detail"] = Value::String(String::new());
        },
        "没写 detail",
    );

    expect_red(
        "workspace 读数形状坏了",
        |m| {
            m["measured"]["workspace"] = Value::String("全绿".into());
        },
        "不是「N suite / M passed",
    );

    expect_red(
        "min_checked 的账改丢",
        |m| {
            m["measured"]["min_checked"] = Value::String("109（随手一填）".into());
        },
        "必须含 108",
    );

    expect_red(
        "c 线读数只写了「下降了」（没有数字）",
        |m| {
            m["measured"]["opt_inline"] = Value::String("内联之后体积下降了".into());
        },
        "必须含读数 951616",
    );

    expect_red(
        "把 c 线的「切断」决策写成别的",
        |m| {
            m["measured"]["opt_dict"] = Value::String("1408B/22 个，3/3 全红".into());
        },
        "必须写明这是**切断**决策",
    );
}
