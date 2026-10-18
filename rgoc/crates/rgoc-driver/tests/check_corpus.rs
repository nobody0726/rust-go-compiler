//! check 面语料清单的**纪律守卫**（`T-CK-93` / `T-CK-A1`）—— M4/Task 5。
//!
//! # 三份清单与它们的关系
//!
//! | 清单 | 内容 | 谁消费 |
//! |---|---|---|
//! | `check-selected.txt` | 检查面**入选**（**75**；路径 only、排序去重） | `run-collide.sh --check` 的**唯一**输入 |
//! | `check-allowlist.txt` | 检查面**排除**（**16**；`<路径>\t<理由>`，理由必须带指针） | 本文件（两向集合相等） |
//! | `check-fixedbugs-waveA.txt` | fixedbugs **wave A**（**50**；4 组注释 G1–G4） | `run-collide.sh --check --list`（Task 15） |
//!
//! **候选** = `check/` `spec/` `examples/` 三目录的**顶层** `.go` 文件
//! （91 = check 70 + spec 13 + examples 8）。目录包（`check/decls2`、`check/importdecl0`、
//! `check/importdecl1`、`check/issue25008`）**不在**候选 —— 多文件包是另一条对撞腿，
//! wave 2 再议（`U-M4-11`）。
//!
//! # 数字为什么敢写死
//!
//! 语料被 `corpus-manifest.sha256` 锁死（`AGENTS.md` §3.1）⇒ 文件集合不变 ⇒
//! 「候选 91 = 入选 75 + 排除 16」「wave A 50（25+8+13+4）」都是**精确值**。
//! 任何一处悄悄少收 / 多收，都会在这里变红。
//!
//! # 与 `run-collide.sh --check` 的分工
//!
//! 本文件**只做 rgoc 侧能做的那半**：清单纪律（两向、无重复、无死行）、每行存在、
//! 理由具体（带 `D-M4-` / `U-M3-` / `T-CK-` / `CH-` 指针）、wave A 的结构保持。
//! 真正的两侧渲染对撞（腿① marker 判定 + 腿② 剥列多重集）由
//! `tools/types-oracle/run-collide.sh --check <testdata 根>` 在**门禁**里跑
//! —— 它要同时驱动 `go` 与 `cargo`，宿主上两个都没有。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 检查面候选的三个目录（**顶层** `.go` 才算候选；目录包见 `PACKAGE_DIRS`）。
const CHECK_DIRS: [&str; 3] = ["check", "spec", "examples"];

/// 已知的四个目录包（多文件包）：存在、但**不在**候选（wave 2 的另一条腿，`U-M4-11`）。
const PACKAGE_DIRS: [&str; 4] = [
    "check/decls2",
    "check/importdecl0",
    "check/importdecl1",
    "check/issue25008",
];

/// wave A 的分组注释前缀：`# ── G1 …（25）`（`──` = U+2500 ×2）。
const WAVE_GROUP_PREFIX: &str = "# \u{2500}\u{2500} ";

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达（cargo 的 CARGO_MANIFEST_DIR 变了？）")
}

/// 语料里 `types/testdata` 的根（**不入库**，只在容器里）。
///
/// 与 `collide_corpus.rs` / `rgoc-load/tests/smoke_corpus.rs` 同一个纪律：
/// **不许静默跳过** —— 读不到就响亮地失败（`RGOC_CORPUS_SRC_DIR` 可覆盖，三处共用）。
fn corpus_root() -> PathBuf {
    let src = match std::env::var("RGOC_CORPUS_SRC_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => repo_root().join("go_source_code/src"),
    };
    let dir = src.join("internal/types/testdata");
    assert!(
        dir.is_dir(),
        "语料目录不存在：{}\n\
         这条守卫必须真的读语料，**不允许**静默跳过。\
         可用 RGOC_CORPUS_SRC_DIR 指向 go_source_code/src。",
        dir.display()
    );
    dir
}

fn oracle_dir() -> PathBuf {
    repo_root().join("tools/types-oracle")
}

fn read_list(name: &str) -> String {
    let p = oracle_dir().join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()))
}

/// 检查面候选 = 三目录**顶层** `.go`（相对路径，按目录序、各目录内排序）。
fn check_candidates(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for dir in CHECK_DIRS {
        let d = root.join(dir);
        assert!(
            d.is_dir(),
            "语料目录不存在：{}（语料锁变了，或路径口径漂了）",
            d.display()
        );
        let mut files: Vec<PathBuf> = std::fs::read_dir(&d)
            .expect("读语料目录")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "go"))
            .collect();
        files.sort();
        for f in files {
            let name = f
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            out.push(format!("{dir}/{name}"));
        }
    }
    out
}

/// 一行理由必须**具体**：非空、够长、且带一个指针。这三条都是为了挡住
/// 「以后再补」式的占位理由 —— 占位比空行更坏：它看起来像写过理由了。
fn reason_ok(r: &str) -> bool {
    const POINTERS: [&str; 4] = ["D-M4-", "U-M3-", "T-CK-", "CH-"];
    !r.trim().is_empty() && r.chars().count() >= 12 && POINTERS.iter().any(|p| r.contains(p))
}

/// 读**入选**清单：丢注释与空行；路径 only —— 出现理由列 ⇒ 报错（理由只属于排除清单）。
fn parse_selected(src: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for (i, l) in src.lines().enumerate() {
        if l.starts_with('#') || l.trim().is_empty() {
            continue;
        }
        if l.contains('\t') {
            return Err(format!(
                "第 {} 行：入选清单不该有理由列（理由只属于排除清单）",
                i + 1
            ));
        }
        out.push(l.trim().to_string());
    }
    Ok(out)
}

/// 读**排除**清单：`<路径>\t<理由>`；缺 TAB（理由缺失，或写成空格）⇒ 报错。
fn parse_allowlist(src: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for (i, l) in src.lines().enumerate() {
        if l.starts_with('#') || l.trim().is_empty() {
            continue;
        }
        let Some((p, r)) = l.split_once('\t') else {
            if l.contains(char::is_whitespace) {
                return Err(format!("第 {} 行：分隔符必须是 TAB（空格不算）", i + 1));
            }
            return Err(format!("第 {} 行：排除行缺理由（`<路径>\\t<理由>`）", i + 1));
        };
        out.push((p.trim().to_string(), r.trim().to_string()));
    }
    Ok(out)
}

/// 清单纪律的**唯一实现**（真实文件与合成反例都走它；别在测试里另抄一份判据）。
///
/// 判据：① 候选集合为输入；② 入选 ∪ 排除 == 候选（**两向**，无重复、无死行）；
/// ③ 排除的每一条理由具体；④ 入选集排序、去重、且都在候选里。
fn check_split(cands: &[String], allowlist_src: &str, selected: &[String]) -> Result<(), String> {
    let cand_set: BTreeSet<&String> = cands.iter().collect();
    let allowed = parse_allowlist(allowlist_src)?;
    let mut allowed_set = BTreeSet::new();
    for (p, reason) in &allowed {
        if !cand_set.contains(p) {
            return Err(format!("{p}：不在候选集里（死行，或路径写错了）"));
        }
        if !allowed_set.insert(p.clone()) {
            return Err(format!("{p}：在排除清单里出现了两次"));
        }
        if !reason_ok(reason) {
            return Err(format!(
                "{p}：理由不合格（{reason:?}）—— 要非空、够长、带指针（D-M4-/U-M3-/T-CK-/CH-）"
            ));
        }
    }

    let mut sorted = selected.to_vec();
    sorted.sort();
    sorted.dedup();
    if sorted != selected {
        return Err("入选清单没有排序/去重".to_string());
    }
    for p in selected {
        if !cand_set.contains(p) {
            return Err(format!("{p}：入选了，但它不是候选（三目录顶层 .go 口径漂了？）"));
        }
        if allowed_set.contains(p) {
            return Err(format!("{p}：既入选又排除"));
        }
    }

    let covered: BTreeSet<&String> = selected.iter().chain(allowed_set.iter()).collect();
    if covered != cand_set {
        let mut miss: Vec<&String> = cand_set.difference(&covered).copied().collect();
        let mut extra: Vec<&String> = covered.difference(&cand_set).copied().collect();
        miss.sort();
        extra.sort();
        return Err(format!(
            "候选 {n} 个，但两清单只覆盖 {c} 个（没覆盖：{miss:?}；多余的：{extra:?}）",
            n = cand_set.len(),
            c = covered.len()
        ));
    }
    Ok(())
}

/// wave A 的一组：`G<n>` + 组头声明的条数 + 组内文件（顺序即文件顺序）。
#[derive(Debug)]
struct WaveGroup {
    name: String,
    declared: usize,
    files: Vec<String>,
}

/// 解析 wave A：数据行必须落在某个 `# ── G<n> …（N）` 组头之后；
/// 组头计数从行尾全角括号里读，**不**与实到数自动对齐（对齐由测试断言）。
fn parse_wave_a(src: &str) -> Result<Vec<WaveGroup>, String> {
    let mut groups: Vec<WaveGroup> = Vec::new();
    for (i, l) in src.lines().enumerate() {
        let t = l.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(head) = t.strip_prefix(WAVE_GROUP_PREFIX) {
            let name = head.split_whitespace().next().unwrap_or("").to_string();
            let declared = head
                .rsplit_once('\u{ff08}')
                .and_then(|(_, tail)| tail.strip_suffix('\u{ff09}'))
                .and_then(|n| n.trim().parse::<usize>().ok())
                .ok_or_else(|| format!("第 {} 行：组头缺 `（N）` 计数", i + 1))?;
            groups.push(WaveGroup {
                name,
                declared,
                files: Vec::new(),
            });
        } else if t.starts_with('#') {
            continue;
        } else {
            match groups.last_mut() {
                Some(g) => g.files.push(t.to_string()),
                None => return Err(format!("第 {} 行：数据行出现在任何分组之前", i + 1)),
            }
        }
    }
    Ok(groups)
}

/// `T-CK-93`（纪律）：候选集机械重算，入选 / 排除**互补**（两向），理由条条具体。
#[test]
fn check_清单_两向集合相等且理由具体() {
    let root = corpus_root();
    let cands = check_candidates(&root);
    let selected = parse_selected(&read_list("check-selected.txt")).expect("入选清单可解析");
    let allowlist = read_list("check-allowlist.txt");

    check_split(&cands, &allowlist, &selected)
        .unwrap_or_else(|e| panic!("check 面精选纪律不成立：{e}"));

    // 语料被锁死 ⇒ 精确值（见模块文档「数字为什么敢写死」）。
    assert_eq!(cands.len(), 91, "候选数变了 ⇒ 三目录顶层口径或语料变了");
    assert_eq!(selected.len(), 75, "入选数变了");
    assert_eq!(
        91 - 75,
        parse_allowlist(&allowlist).expect("排除清单可解析").len(),
        "排除数 != 候选 − 入选"
    );

    // 三目录各自的数字（70 + 13 + 8 = 91，同为锁死值）。
    let mut per_dir: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for c in &cands {
        let d = c.split('/').next().expect("候选必有目录前缀");
        *per_dir.entry(d).or_insert(0) += 1;
    }
    assert_eq!(per_dir.get("check").copied(), Some(70), "check/ 顶层数变了");
    assert_eq!(per_dir.get("spec").copied(), Some(13), "spec/ 顶层数变了");
    assert_eq!(per_dir.get("examples").copied(), Some(8), "examples/ 顶层数变了");

    // ② 每行存在（语料被 sha256 锁死 ⇒ 不存在就是清单写错了，不许含糊过去）。
    let allowed = parse_allowlist(&allowlist).expect("排除清单可解析");
    for p in selected.iter().chain(allowed.iter().map(|(p, _)| p)) {
        assert!(root.join(p).is_file(), "{p}：语料里不存在");
    }
}

/// 反向（机制有牙）：清单纪律的**每一条**都得会红 —— 用合成输入逐个踩。
///
/// 没有这一组，「两向集合相等」就可能是**恒真**断言（比如实现里漏判某一支，
/// 或者把空理由当合格理由）。B7 的第一半（真清单里移走一行 ⇒ 变红）在
/// progress 台账里有实测记录；这里管的是**离线**可重放的反例。
#[test]
fn check_清单_合成反例逐条报错() {
    let cands = vec!["check/a.go".to_string(), "spec/b.go".to_string()];
    let ok_sel = vec!["check/a.go".to_string()];
    let ok_allow = "spec/b.go\t计数不等（`D-M4-4`，wave 2）\n";

    check_split(&cands, ok_allow, &ok_sel).expect("基线该通过");

    // ① 空理由
    let e = check_split(&cands, "spec/b.go\t\n", &ok_sel).unwrap_err();
    assert!(e.contains("理由不合格"), "{e}");

    // ② 占位理由（够长但没有指针）
    let e = check_split(&cands, "spec/b.go\t以后再补这一条（占位理由）\n", &ok_sel).unwrap_err();
    assert!(e.contains("理由不合格"), "{e}");

    // ③ 少一条理由 ⇒ 候选没被覆盖（B7 第一半的真清单红就是这个形态：差集点名）
    let e = check_split(&cands, "", &ok_sel).unwrap_err();
    assert!(e.contains("没覆盖"), "{e}");
    assert!(e.contains("spec/b.go"), "差集要点名缺了谁：{e}");

    // ④ 死行（路径不在候选里）
    let e = check_split(&cands, "check/zz.go\t计数不等（`D-M4-4`）\n", &ok_sel).unwrap_err();
    assert!(e.contains("不在候选集"), "{e}");

    // ⑤ 既入选又排除
    let e = check_split(&cands, "check/a.go\t计数不等（`D-M4-4`）\n", &ok_sel).unwrap_err();
    assert!(e.contains("既入选又排除"), "{e}");

    // ⑥ 入选清单未排序 / 重复
    let e = check_split(&cands, ok_allow, &["check/a.go".into(), "check/a.go".into()]).unwrap_err();
    assert!(e.contains("排序"), "{e}");

    // ⑦ 入选了非候选
    let e = check_split(&cands, ok_allow, &["check/zz.go".to_string()]).unwrap_err();
    assert!(e.contains("不是候选"), "{e}");

    // ⑧ 空格分隔（前缀里踩过：`<路径>\t<理由>` 写成空格）
    let e = check_split(&cands, "spec/b.go 理由（`D-M4-4`）\n", &ok_sel).unwrap_err();
    assert!(e.contains("TAB"), "{e}");

    // ⑨ 排除行缺理由（连 TAB 都没有）
    let e = check_split(&cands, "spec/b.go\n", &ok_sel).unwrap_err();
    assert!(e.contains("缺理由"), "{e}");

    // ⑩ 入选行带理由（理由只属于排除清单）
    let e = parse_selected("check/a.go\t计数不等（`D-M4-4`）\n").unwrap_err();
    assert!(e.contains("不该有理由"), "{e}");

    // ⑪ wave A 解析：数据行在组头之前
    let e = parse_wave_a("fixedbugs/issue1.go\n").unwrap_err();
    assert!(e.contains("分组之前"), "{e}");
}

/// `T-CK-A1`（纪律）：wave A 的结构保持 —— 50 行、4 组（G1–G4，25+8+13+4）、
/// 全部存在、且**不**与检查面三目录候选重复（它是 `fixedbugs/` 的另一条腿）。
#[test]
fn fixedbugs_wave_a_50行4组且不与检查面候选重复() {
    let root = corpus_root();
    let groups = parse_wave_a(&read_list("check-fixedbugs-waveA.txt")).expect("wave A 可解析");

    let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["G1", "G2", "G3", "G4"], "分组名/顺序变了");

    let declared: Vec<usize> = groups.iter().map(|g| g.declared).collect();
    assert_eq!(declared, [25, 8, 13, 4], "组头声明的计数变了");

    let mut total = 0usize;
    let mut all: Vec<String> = Vec::new();
    for g in &groups {
        assert_eq!(
            g.files.len(),
            g.declared,
            "{}：组头声明 {} 个，实到 {} 个（组头与数据行脱钩）",
            g.name,
            g.declared,
            g.files.len()
        );
        let mut sorted = g.files.clone();
        sorted.sort();
        assert_eq!(sorted, g.files, "{}：组内没有排序", g.name);
        for p in &g.files {
            assert!(p.starts_with("fixedbugs/"), "{p}：不属于 fixedbugs/ 面");
            assert!(
                root.join(p).is_file(),
                "{p}：语料里不存在（语料被 sha256 锁死 ⇒ 是清单写错了）"
            );
        }
        total += g.files.len();
        all.extend(g.files.iter().cloned());
    }
    assert_eq!(total, 50, "wave A 总数变了");

    let uniq: BTreeSet<&String> = all.iter().collect();
    assert_eq!(uniq.len(), all.len(), "wave A 有重复行");

    // 与检查面三目录候选**零相交**（两条腿不许互相污染）。
    let cands: BTreeSet<String> = check_candidates(&root).into_iter().collect();
    for p in &all {
        assert!(!cands.contains(p), "{p}：不该同时出现在检查面候选里");
    }
}

/// `U-M4-11` 的前置：四个目录包**存在**（多文件包腿的原料），且不在顶层候选里。
#[test]
fn check_目录包_存在但不在候选() {
    let root = corpus_root();
    let cands: BTreeSet<String> = check_candidates(&root).into_iter().collect();
    for d in PACKAGE_DIRS {
        let dir = root.join(d);
        assert!(
            dir.is_dir(),
            "{d}：目录包不存在（wave 2 的多文件包对撞腿要靠它）"
        );
        let go_files = std::fs::read_dir(&dir)
            .expect("读目录包")
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "go"))
            .count();
        assert!(go_files >= 1, "{d}：目录包里没有 .go 文件");
        for c in &cands {
            assert!(
                !c.starts_with(&format!("{d}/")),
                "{c}：目录包的文件不该进顶层候选（候选只收三目录顶层）"
            );
        }
    }
}
