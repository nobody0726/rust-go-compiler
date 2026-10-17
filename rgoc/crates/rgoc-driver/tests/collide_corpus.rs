//! 语料精选扩大的对撞（`T-TY-94`）—— M3/Task 35。
//!
//! # 腿乙的第三块拼图
//!
//! 前两块：`collide_dump.rs` / `collide_cmp.rs` 把**自有 fixture** 与冻结的 `.want`
//! 对上（传递性 ⇒ 「rgoc == oracle」）；本文件把战场扩到**官方语料**
//! （`types/testdata/`，365 个 `.go`）里能对上的一批。
//!
//! 语料**不入库**（`AGENTS.md` §3.1：只读 + 由 `corpus-manifest.sha256` 锁死），
//! 所以这里**不 vendor 副本、不提交 golden**：本文件只做**rgoc 侧**能做的三件事，
//! 真正的两侧逐字节对撞由 `tools/types-oracle/run-collide.sh --corpus <根>` 在
//! **门禁**里跑（它要同时驱动 `go` 与 `cargo`）。
//!
//! | 谁 | 断言 |
//! |---|---|
//! | **本文件** | ① 精选**纪律**：候选集机械重算、两向集合相等、理由非空且带指针；② 入选文件 rgoc 侧自足（退出码 0 / stderr 空 / 正文非空） |
//! | `run-collide.sh --corpus` | 入选文件**两侧现跑逐字节一致** + stderr 空 + 重复运行确定 |
//!
//! # 精选口径（唯一权威是 `corpus-allowlist.txt` 的头部，这里只是它的执行副本）
//!
//! * **候选** = 机械筛：有 `^package p$`、无 `^import`、无 `ERROR` 子串
//!   （`D-M3-14` 的单包前提 + 官方负例标注会被 oracle 自己报错 ⇒ dump 不可比）；
//! * **入选** = 候选里两侧 dump 逐字节一致且非空的（`corpus-selected.txt`
//!   —— 那是 `run-collide.sh --corpus` 的输入，**唯一**清单，脚本不自己重算，
//!   免得口径在 bash 与 Rust 两边漂移）；
//! * **排除** = 候选里没入选的**每一个**，在 `corpus-allowlist.txt` 里一行理由。
//!
//! # 数字为什么敢写死
//!
//! 语料被 `corpus-manifest.sha256` 锁死（`AGENTS.md` §3.1）⇒ 文件内容与分类都不变
//! ⇒ 这里的「候选 57 / 入选 31 / 排除 26」是**精确值**。任何一处悄悄少收 / 多收，
//! 都会在这里变红。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达（cargo 的 CARGO_MANIFEST_DIR 变了？）")
}

/// 语料里 `types/testdata` 的根（**不入库**，只在容器里）。
///
/// 与 `rgoc-load/tests/smoke_corpus.rs` 同一个纪律：**不许静默跳过** ——
/// 读不到就响亮地失败（`RGOC_CORPUS_SRC_DIR` 可覆盖，与那条冒烟共用）。
fn corpus_root() -> PathBuf {
    let src = match std::env::var("RGOC_CORPUS_SRC_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => repo_root().join("go_source_code/src"),
    };
    let dir = src.join("internal/types/testdata");
    assert!(
        dir.is_dir(),
        "语料目录不存在：{}\n\
         这条对撞必须真的读语料，**不允许**静默跳过。\
         可用 RGOC_CORPUS_SRC_DIR 指向 go_source_code/src。",
        dir.display()
    );
    dir
}

fn oracle_dir() -> PathBuf {
    repo_root().join("tools/types-oracle")
}

/// 机械筛（三条，与 `corpus-allowlist.txt` 头部逐字对应）。
fn is_candidate(src: &str) -> bool {
    let has_pkg = src.lines().any(|l| l == "package p");
    let has_import = src.lines().any(|l| l.starts_with("import"));
    has_pkg && !has_import && !src.contains("ERROR")
}

/// 候选集 = `<根>/*/*.go` 里过机械筛的（相对路径，**排序**）。
///
/// 只走一层子目录：语料那几个族（`check/` `examples/` `fixedbugs/` `spec/`）
/// 都是平的 —— 走深一层就会与普查脚本的 `"$ROOT"/*/*.go` 不一致。
fn candidates(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut subs: Vec<PathBuf> = std::fs::read_dir(root)
        .expect("读语料根")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    subs.sort();
    for sub in subs {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&sub)
            .expect("读语料子目录")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "go"))
            .collect();
        files.sort();
        let sub_name = sub
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        for f in files {
            let name = f
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let rel = format!("{sub_name}/{name}");
            let src = std::fs::read_to_string(&f).expect("读语料文件");
            if is_candidate(&src) {
                out.push(rel);
            }
        }
    }
    out
}

/// 一行理由必须**具体**：非空、够长、且带一个指针（指向 `U-M3-0x` / `D-M3-x` /
/// `C7` / `T-TY-xx`）。这三条都是为了挡住「以后再补」式的占位理由 ——
/// 占位比空行更坏：它看起来像写过理由了。
fn reason_ok(r: &str) -> bool {
    const POINTERS: [&str; 4] = ["U-M3-", "D-M3-", "C7", "T-TY-"];
    !r.trim().is_empty() && r.chars().count() >= 12 && POINTERS.iter().any(|p| r.contains(p))
}

/// 读一个清单：丢 `#` 注释与空行，`路径<TAB>理由`（理由可缺 ⇒ 由调用方判）。
fn parse_list(src: &str) -> Result<Vec<(String, Option<String>)>, String> {
    let mut out = Vec::new();
    for (i, l) in src.lines().enumerate() {
        if l.starts_with('#') || l.trim().is_empty() {
            continue;
        }
        match l.split_once('\t') {
            Some((p, r)) => out.push((p.trim().to_string(), Some(r.trim().to_string()))),
            None => {
                if l.contains(char::is_whitespace) {
                    return Err(format!("第 {} 行：分隔符必须是 TAB（空格不算）", i + 1));
                }
                out.push((l.trim().to_string(), None));
            }
        }
    }
    Ok(out)
}

/// 精选纪律的**唯一实现**（真实文件与合成反例都走它；别在测试里另抄一份判据）。
///
/// 判据：① 候选集合为输入；② 入选集与排除集**互补**（每个候选恰好出现一次）；
/// ③ 排除的每一条理由具体；④ 入选集排序、去重、且都在候选里。
fn check_selection(
    cands: &[String],
    allowlist_src: &str,
    selected: &[String],
) -> Result<(), String> {
    let cand_set: std::collections::BTreeSet<&String> = cands.iter().collect();
    let allowed = parse_list(allowlist_src)?;
    let mut allowed_set = std::collections::BTreeSet::new();
    for (p, reason) in &allowed {
        if !cand_set.contains(p) {
            return Err(format!("{p}：不在候选集里（死行，或路径写错了）"));
        }
        if !allowed_set.insert(p.clone()) {
            return Err(format!("{p}：在排除清单里出现了两次"));
        }
        let r = reason.clone().unwrap_or_default();
        if !reason_ok(&r) {
            return Err(format!("{p}：理由不合格（{r:?}）—— 要非空、够长、带指针"));
        }
    }

    let mut sorted = selected.to_vec();
    sorted.sort();
    sorted.dedup();
    if sorted != selected {
        return Err("入选清单没有排序/去重".to_string());
    }
    let sel_set: std::collections::BTreeSet<&String> = selected.iter().collect();
    for p in selected {
        if !cand_set.contains(p) {
            return Err(format!("{p}：入选了，但它不是候选（机械筛口径漂了？）"));
        }
        if allowed_set.contains(p) {
            return Err(format!("{p}：既入选又排除"));
        }
    }

    let covered: std::collections::BTreeSet<&String> =
        selected.iter().chain(allowed_set.iter()).collect();
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
    let _ = sel_set;
    Ok(())
}

fn dump_of(fixture: &Path) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rgoc-driver"))
        .args(["types", "--dump"])
        .arg(fixture)
        .output()
        .expect("起 rgoc-driver 进程");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// `C7` §2 规则 1：丢 `#` 注释行与空行（两侧都忽略）。
fn body(s: &str) -> String {
    s.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn read_list(name: &str) -> String {
    let p = oracle_dir().join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()))
}

/// `T-TY-94`（纪律）：候选集机械重算，入选 / 排除**互补**，理由条条具体。
#[test]
fn corpus_精选纪律_两向集合相等且理由具体() {
    let root = corpus_root();
    let cands = candidates(&root);
    let allowlist = read_list("corpus-allowlist.txt");
    let selected: Vec<String> = parse_list(&read_list("corpus-selected.txt"))
        .expect("入选清单可解析")
        .into_iter()
        .map(|(p, r)| {
            assert!(r.is_none(), "{p}：入选清单里不该有理由列");
            p
        })
        .collect();

    check_selection(&cands, &allowlist, &selected)
        .unwrap_or_else(|e| panic!("精选纪律不成立：{e}"));

    // 语料被锁死 ⇒ 这三个数可以写死（见模块文档「数字为什么敢写死」）。
    assert_eq!(cands.len(), 57, "候选数变了 ⇒ 机械筛口径或语料变了");
    assert_eq!(selected.len(), 31, "入选数变了");
    assert_eq!(
        57 - 31,
        parse_list(&allowlist).expect("排除清单可解析").len(),
        "排除数 != 候选 - 入选"
    );
}

/// 反向（机制有牙）：精选纪律的**每一条**都得会红 —— 用合成输入逐个踩。
///
/// 没有这一组，「两向集合相等」就可能是**恒真**断言（比如实现里漏判某一支，
/// 或者把空理由当合格理由）。
#[test]
fn corpus_精选纪律_合成反例逐条报错() {
    let cands = vec!["a/x.go".to_string(), "b/y.go".to_string()];
    let ok_sel = vec!["b/y.go".to_string()];
    let ok_allow = "a/x.go\t实例化未建模（`U-M3-04`）\n";

    check_selection(&cands, ok_allow, &ok_sel).expect("基线该通过");

    // ① 空理由
    let e = check_selection(&cands, "a/x.go\t\n", &ok_sel).unwrap_err();
    assert!(e.contains("理由不合格"), "{e}");

    // ② 占位理由（够长但没有指针）
    let e = check_selection(&cands, "a/x.go\t以后再补这一条（占位）\n", &ok_sel).unwrap_err();
    assert!(e.contains("理由不合格"), "{e}");

    // ③ 少一条理由 ⇒ 候选没被覆盖
    let e = check_selection(&cands, "", &ok_sel).unwrap_err();
    assert!(e.contains("没覆盖"), "{e}");

    // ④ 死行（路径不在候选里）
    let e = check_selection(&cands, "z/z.go\t实例化未建模（`U-M3-04`）\n", &ok_sel).unwrap_err();
    assert!(e.contains("不在候选集"), "{e}");

    // ⑤ 既入选又排除
    let e = check_selection(&cands, "b/y.go\t实例化未建模（`U-M3-04`）\n", &ok_sel).unwrap_err();
    assert!(e.contains("既入选又排除"), "{e}");

    // ⑥ 入选清单未排序
    let e = check_selection(
        &cands,
        ok_allow,
        &["b/y.go".to_string(), "b/y.go".to_string()],
    )
    .unwrap_err();
    assert!(e.contains("排序"), "{e}");

    // ⑦ 入选了非候选
    let e = check_selection(&cands, ok_allow, &["z/z.go".to_string()]).unwrap_err();
    assert!(e.contains("不是候选"), "{e}");

    // ⑧ 空格分隔（前缀里踩过：`<路径>\t<理由>` 写成空格）
    let e = check_selection(&cands, "a/x.go 理由（`U-M3-04`）\n", &ok_sel).unwrap_err();
    assert!(e.contains("TAB"), "{e}");
}

/// `T-TY-94`（rgoc 侧）：每个入选文件都要能**自足**产出 —— 退出码 0、stderr 空、
/// 正文非空（空正文对撞不起来；`run-collide.sh --corpus` 那一侧会再查一遍非空）。
#[test]
fn corpus_入选文件_rgoc_侧自足() {
    let root = corpus_root();
    let selected: Vec<String> = parse_list(&read_list("corpus-selected.txt"))
        .expect("入选清单可解析")
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    assert!(!selected.is_empty(), "入选清单是空的 —— 这条什么都没测");

    for rel in &selected {
        let f = root.join(rel);
        assert!(
            f.is_file(),
            "入选清单里的 {rel} 不在语料里：{}",
            f.display()
        );
        let (code, stdout, stderr) = dump_of(&f);
        assert_eq!(code, 0, "{rel}：入选文件必须是合法程序（退出码 {code}）");
        assert!(stderr.is_empty(), "{rel}：stderr 必须空：{stderr}");
        let b = body(&stdout);
        assert!(!b.is_empty(), "{rel}：dump 正文为空 —— 无可对撞内容");
    }
}
