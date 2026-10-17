//! identity / representable 对撞（`T-TY-92` / `T-TY-93`）—— `testdata/cmp-*.go`
//! 的判定行必须与 oracle golden 一致，**除了** `D-M3-13` 划出的那一小块。
//!
//! # 与 `collide_dump.rs` 同一套传递
//!
//! | 谁 | 断言 |
//! |---|---|
//! | `tools/types-oracle/cmp_test.go`（oracle 侧） | oracle **现跑** == `.want` |
//! | **本文件**（rgoc 侧） | rgoc **现跑** == 同一个 `.want`（除声明的排除） |
//!
//! 「两侧现跑」的形态在 `tools/types-oracle/run-collide.sh --cmp`（门禁里跑）。
//!
//! # 排除集不是「跑出来什么样就是什么样」
//!
//! M3 对**一部分** identity 请求明确不判（`D-M3-13` / `U-M3-05`：两侧都是
//! **匿名接口**、且至少一侧含项集或 `comparable` —— 那才非做类型集推理不可）。
//! 这种条目 rgoc 输出 `result = reject` + `# reject <行> <代号>`。
//!
//! 难点在于**怎么断言这个集合**：如果只是「rgoc 拒了就跳过」，那实现
//! 悄悄多拒几条也没人管 —— 排除集会变成一块**可以随意生长的遮羞布**。
//! 所以把排除集写成**样本旁边的一句承诺**：fixture 里每条预期被拒的指令
//! **前面一行**写
//!
//! ```go
//! // rgoc:expect-reject U-M3-05
//! //rgoc:cmp identical interface{ ~int } => interface{ ~int }
//! ```
//!
//! 判据是**两个方向的集合相等**：
//!
//! - 标记了却没 reject（说明实现突然判得出了，或标记写错了）⇒ 失败；
//! - reject 了却没标记（说明排除集**长出了**没申报的一条）⇒ 失败；
//! - 代号必须正是 `TYP-UNSUPPORTED-CONSTRAINT-IDENTITY`（`U-M3-05` 的实现侧）。
//!
//! 另外钉一条**不变量**：`representable` **永远**不出 `reject`。非基本类型的目标
//! 是**样本违规 ⇒ 响亮报错**（退出码 2），不是「拒绝」——`rgoc types --cmp`
//! 的模块头与 `Ruling 27-1` 都是这条纪律。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const K_ID: &str = "identical";
const K_REP: &str = "representable";
const R_REJECT: &str = "reject";

/// 一条判定行：`<行号>\t<kind>\t<result>`。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    line: u32,
    kind: String,
    result: String,
}

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达（cargo 的 CARGO_MANIFEST_DIR 变了？）")
}

fn testdata() -> PathBuf {
    repo_root().join("tools/types-oracle/testdata")
}

/// 读判定行（`#` 开头的行与空行忽略，`M3-tests.md` §5.2）。
fn parse_entries(text: &str) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let l = raw.trim_end_matches('\r');
        if l.starts_with('#') || l.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() != 3 {
            return Err(format!(
                "第 {} 行不是 `<行号>\\t<kind>\\t<result>`：{l:?}",
                i + 1
            ));
        }
        let line: u32 = f[0]
            .parse()
            .map_err(|_| format!("第 {} 行的行号不是数字：{:?}", i + 1, f[0]))?;
        if f[1] != K_ID && f[1] != K_REP {
            return Err(format!("第 {} 行的 kind 不认识：{:?}", i + 1, f[1]));
        }
        if !matches!(f[2], "true" | "false" | R_REJECT) {
            return Err(format!("第 {} 行的 result 不认识：{:?}", i + 1, f[2]));
        }
        out.push(Entry {
            line,
            kind: f[1].to_string(),
            result: f[2].to_string(),
        });
    }
    Ok(out)
}

/// 读 `# reject <行> <代号>` 注释（排除集的**实际**成员与理由）。
fn parse_rejects(text: &str) -> Result<BTreeMap<u32, String>, String> {
    let mut m = BTreeMap::new();
    for raw in text.lines() {
        let Some(rest) = raw.strip_prefix("# reject ") else {
            continue;
        };
        let f: Vec<&str> = rest.split(' ').collect();
        if f.len() != 2 {
            return Err(format!(
                "`# reject` 行形状不对（要 `<行> <代号>`）：{raw:?}"
            ));
        }
        let line: u32 = f[0]
            .parse()
            .map_err(|_| format!("`# reject` 的行号不是数字：{raw:?}"))?;
        if m.insert(line, f[1].to_string()).is_some() {
            return Err(format!("同一行被 reject 了两次：{line}"));
        }
    }
    Ok(m)
}

/// 读 fixture 里的 `// rgoc:expect-reject <单元>` 标记（排除集的**声明**）。
///
/// 标记的**下一非空行**必须是一条 `//rgoc:cmp` 指令 —— 这条形状约束是故意的：
/// 标记写错位置（例如后面跟着普通代码）会当场报错，而不是静静地什么也没标记。
fn declared_rejects(text: &str) -> Result<BTreeMap<u32, String>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut m = BTreeMap::new();
    for (i, raw) in lines.iter().enumerate() {
        let Some(unit) = raw.trim().strip_prefix("// rgoc:expect-reject ") else {
            continue;
        };
        if unit.trim().is_empty() {
            return Err(format!("第 {} 行的标记没写单元名（如 `U-M3-05`）", i + 1));
        }
        let mut j = i + 1;
        while j < lines.len() && lines[j].trim().is_empty() {
            j += 1;
        }
        let Some(next) = lines.get(j) else {
            return Err(format!("第 {} 行的标记后面没有指令", i + 1));
        };
        if !next.trim_start().starts_with("//rgoc:cmp ") {
            return Err(format!(
                "第 {} 行的标记后面不是 `//rgoc:cmp` 指令（第 {} 行是 {:?}）",
                i + 1,
                j + 1,
                next
            ));
        }
        m.insert((j + 1) as u32, unit.trim().to_string());
    }
    Ok(m)
}

/// 对撞一份 fixture（**纯函数** —— 便于用合成输入钉住排除集机制本身）。
///
/// `Ok(摘要)` = 逐条对齐（排除集两个方向相等且代号正确）；`Err` = 指名道姓的理由。
fn check(fixture_text: &str, want: &str, rgoc_out: &str) -> Result<String, String> {
    let golden = parse_entries(want)?;
    let got = parse_entries(rgoc_out)?;
    if golden.is_empty() || got.is_empty() {
        return Err("两侧都必须非空 —— 「两边都空」是最阴的一种通过".into());
    }
    let rejects = parse_rejects(rgoc_out)?;

    let by_line: BTreeMap<u32, &Entry> = got.iter().map(|e| (e.line, e)).collect();
    if got.len() != by_line.len() {
        return Err(format!(
            "rgoc 侧有重复行号（{} 条 vs {} 个行号）",
            got.len(),
            by_line.len()
        ));
    }
    if golden.len() < got.len() {
        return Err(format!(
            "oracle 只有 {} 条、rgoc 有 {} 条 —— rgoc 多出来的条目（行 {:?}）说明两侧读到的指令集不同",
            golden.len(),
            got.len(),
            by_line
                .keys()
                .filter(|l| !golden.iter().any(|g| g.line == **l))
                .collect::<Vec<_>>()
        ));
    }

    let mut excluded: Vec<u32> = Vec::new();
    for g in &golden {
        let Some(r) = by_line.get(&g.line) else {
            return Err(format!("第 {} 行：oracle 有、rgoc 没有产出", g.line));
        };
        if r.kind != g.kind {
            return Err(format!(
                "第 {} 行：kind 不同（oracle {} vs rgoc {}）",
                g.line, g.kind, r.kind
            ));
        }
        if r.result == g.result {
            continue;
        }
        if r.result == R_REJECT {
            excluded.push(g.line);
            continue;
        }
        return Err(format!(
            "第 {} 行（{}）：oracle {} vs rgoc {} —— 不一致，且**不是**声明的排除",
            g.line, g.kind, g.result, r.result
        ));
    }

    // 不变量：representable 永不 reject（非基本类型的目标是**响亮报错**）
    if let Some(e) = got.iter().find(|e| e.kind == K_REP && e.result == R_REJECT) {
        return Err(format!(
            "第 {} 行：representable 出了 reject —— M3 的边界只在 identity（非基本类型的目标应当**响亮报错**，退出码 2）",
            e.line
        ));
    }

    // 排除集：**两个方向的集合相等**（见模块头）
    let declared = declared_rejects(fixture_text)?;
    let actual: BTreeMap<u32, String> = excluded
        .iter()
        .map(|l| (*l, rejects.get(l).cloned().unwrap_or_default()))
        .collect();
    if actual.keys().ne(declared.keys()) {
        return Err(format!(
            "排除集与声明的集合不同：\n  声明的（fixture 里的 `// rgoc:expect-reject` 标记）：{:?}\n  实际 reject 的：{:?}",
            declared.keys().collect::<Vec<_>>(),
            actual.keys().collect::<Vec<_>>()
        ));
    }
    let want_code = rgoc_lex::DiagCode::TypUnsupportedConstraintIdentity.as_str();
    for (line, unit) in &declared {
        let code = &actual[line];
        if code != want_code {
            return Err(format!(
                "第 {line} 行：reject 的理由是 {code:?}，但 `{unit}` 对应的应当是 {want_code:?}"
            ));
        }
    }

    Ok(format!(
        "{} 条（排除 {} 条，全部 {want_code}）",
        golden.len(),
        excluded.len()
    ))
}

fn run_cmp(fixture: &Path) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rgoc-driver"))
        .args(["types", "--cmp"])
        .arg(fixture)
        .output()
        .expect("起 rgoc-driver 进程");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// `T-TY-92/93`（rgoc 侧）：`testdata/cmp-*.go` 的判定行与 oracle golden 逐条一致
/// （排除集按 §模块头 的两向断言）。
#[test]
fn cmp_对撞_全部_cmp_fixture() {
    let dir = testdata();
    assert!(dir.is_dir(), "找不到 {}", dir.display());

    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("读 testdata")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            let n = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            n.starts_with("cmp-") && n.ends_with(".go")
        })
        .collect();
    fixtures.sort();
    assert!(
        !fixtures.is_empty(),
        "一个 cmp-*.go 都没有 —— 这条对撞什么都没测"
    );

    for f in &fixtures {
        let name = f
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let text = std::fs::read_to_string(f).expect("读 fixture");
        let want_path = f.with_extension("want");
        let want = std::fs::read_to_string(&want_path).unwrap_or_else(|e| {
            panic!(
                "读不到 {}（新 fixture 要在 tools/types-oracle 里跑一次 `go test ./... -update`）：{e}",
                want_path.display()
            )
        });

        let (code, stdout, stderr) = run_cmp(f);
        assert_eq!(
            code, 0,
            "{name}：reject **不是**错误（退出码 0）；stderr:\n{stderr}"
        );
        assert!(stderr.is_empty(), "{name}：stderr 必须为空\n{stderr}");

        let summary = check(&text, &want, &stdout).unwrap_or_else(|e| panic!("{name}：{e}"));
        println!("  ✓ {name}：{summary}");
    }
}

/// 排除集机制本身的四种情形（**合成输入**，不用 fixture —— 这样它永远不会
/// 因为样本变动而失效，也不会因为样本恰好没触发某条而静默失去覆盖）。
#[test]
fn 排除集机制_四种情形() {
    let code = rgoc_lex::DiagCode::TypUnsupportedConstraintIdentity.as_str();
    let marked = "package p\n\n// rgoc:expect-reject U-M3-05\n\
                  //rgoc:cmp identical interface{ ~int } => interface{ ~int }\n";
    let unmarked = "package p\n\n\
                    //rgoc:cmp identical interface{ ~int } => interface{ ~int }\n";
    // 合成 fixture 的行号：1=package、2=空行、3=标记、4=指令 ⇒ 判定的是第 4 行。
    let want = "# source: oracle\n4\tidentical\ttrue\n";

    // ① 标记 + reject + 代号对 ⇒ 过
    let ok = format!("4\tidentical\treject\n# reject 4 {code}\n");
    let s = check(marked, want, &ok).expect("标记与实现一致时必须通过");
    assert!(s.contains("排除 1 条"), "{s}");

    // ② reject 了但**没标记** ⇒ 排除集不能偷偷生长
    let e = check(unmarked, want, &ok).expect_err("没标记的 reject 必须报错");
    assert!(e.contains("排除集与声明的集合不同"), "{e}");

    // ③ 标记了但**没 reject**（rgoc 给出了 true）⇒ 标记成了空头支票
    let e = check(marked, want, "4\tidentical\ttrue\n").expect_err("标记却没 reject 必须报错");
    assert!(e.contains("排除集与声明的集合不同"), "{e}");

    // ④ 一致但**代号不对** ⇒ 说不出「拒绝的是哪一条」
    let e = check(
        marked,
        want,
        "4\tidentical\treject\n# reject 4 SOME-OTHER-CODE\n",
    )
    .expect_err("代号不对必须报错");
    assert!(e.contains("对应的应当是"), "{e}");

    // ⑤ 普通的**不一致**（既不是 reject 也不是相等）⇒ 报「不是声明的排除」
    let e = check(marked, want, "4\tidentical\tfalse\n").expect_err("不一致必须报错");
    assert!(e.contains("**不是**声明的排除"), "{e}");

    // ⑥ `representable` 出 reject ⇒ 违反「边界只在 identity」
    let fx_rep = "package p\n\n//rgoc:cmp representable 1 => int8\n";
    let e = check(
        fx_rep,
        "1\trepresentable\ttrue\n",
        "1\trepresentable\treject\n",
    )
    .expect_err("representable 不许 reject");
    assert!(e.contains("只在 identity"), "{e}");
}

/// 标记写错位置要**当场报错**（不然后面就是「静静地什么也没标记」）。
#[test]
fn 标记位置错_当场报错() {
    let fx = "package p\n\n// rgoc:expect-reject U-M3-05\nvar x int\n";
    let e = declared_rejects(fx).expect_err("标记后面不是指令 ⇒ 必须报错");
    assert!(e.contains("不是 `//rgoc:cmp` 指令"), "{e}");
}

/// `cmp-*.go` 的 golden 都在且非空（与 `collide_dump.rs` 的同名钉子对称）。
#[test]
fn cmp_golden_都在且非空() {
    let dir = testdata();
    let mut n = 0;
    for e in std::fs::read_dir(&dir).expect("读 testdata") {
        let p = e.expect("目录项").path();
        let name = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if !(name.starts_with("cmp-") && name.ends_with(".go")) {
            continue;
        }
        n += 1;
        let want = p.with_extension("want");
        let text = std::fs::read_to_string(&want)
            .unwrap_or_else(|e| panic!("{} 缺 golden：{e}", want.display()));
        assert!(
            !parse_entries(&text).expect("golden 必须可解析").is_empty(),
            "{} 一条判定行都没有 —— 「两侧都空」是最阴的一种通过",
            want.display()
        );
    }
    assert!(n > 0, "一个 fixture 都没有");
}
