//! M12 检查面判据（`T-GEN-*` 的检查面部分，`M12-tests.md` §3）。
//!
//! 这一族判据只管**前端**：`check` 退出 0 就说「这段代码在静态面上是对的」。
//! 它们全部拿命令行真身（`CARGO_BIN_EXE_rgoc-driver`）跑，不调用库函数 ——
//! 因为要判的是「用户看到的行为」，而 M12 已知的第一个真错（`T-GEN-08a`）
//! 恰恰是**两个子命令对同一份源码给出不同结论**，那种错只在命令行边界上可见。
//!
//! # 为什么门禁文件清单是**算**出来的
//!
//! `M12-baseline.md` §1 记过三次「谓词打错靶」的翻车（`^// run$` 漏掉带尾随
//! 参数的文件、`grep '^import'` 被块注释骗、`'^import|"'` 把带字符串字面量的
//! 文件全误排除）。所以这里不抄一份文件名清单，而是让 `m12-classify.sh` 现算
//! 一份到临时目录再读 —— 语料一变，这里跟着变。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达（cargo 的 CARGO_MANIFEST_DIR 变了？）")
}

fn corpus() -> PathBuf {
    repo_root().join("go_source_code/test/typeparam")
}

/// 现算一份清单到 `out`（`m12-classify.sh --out out`）。
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
    let s = fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不了 {}：{e}", p.display()));
    s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// 从诊断渲染里取**检查面**的 `[...]` 码（去重、保序）。
///
/// 只认全大写的码（`CHK-*` / `TYP-*` / `HIR-*` / `SYN-*`）—— clang 的输出里有
/// 大量方括号（`[-Werror]` 之类），混进来会把「下游失败」误判成「检查面失败」。
///
/// **排除 `SSA-*` / `CG-*`**：那是后端能力面（`M11` 的
/// `SSA-UNSUPPORTED-CHAN`、`M12` 的 `CG-UNSUPPORTED-GO-ARGS`）。它们出现说明
/// 「前端过了、后端不支持」，是**另一个面**的账 —— 混进来会让这条判据去催
/// 后端，而不是催检查面。
fn codes_of(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] != '[' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut j = start;
        while j < b.len() && b[j] != ']' {
            j += 1;
        }
        if j < b.len() {
            let c: String = b[start..j].iter().collect();
            let ok = c.len() >= 5
                && c.chars().next().is_some_and(|x| x.is_ascii_uppercase())
                && c.chars()
                    .all(|x| x.is_ascii_uppercase() || x.is_ascii_digit() || x == '-')
                && c.contains('-')
                && !c.starts_with("SSA-")
                && !c.starts_with("CG-");
            if ok && !out.contains(&c) {
                out.push(c);
            }
        }
        i = j + 1;
    }
    out
}

/// `check` 的结论：`(退出码, 码集合)`。
fn check_verdict(file: &Path) -> (i32, Vec<String>) {
    let out = Command::new(env!("CARGO_BIN_EXE_rgoc-driver"))
        .arg("check")
        .arg(file)
        .output()
        .expect("起不了 rgoc-driver check");
    let mut all = String::from_utf8_lossy(&out.stdout).into_owned();
    all.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), codes_of(&all))
}

/// `build` 的结论：`(退出码, 码集合)`。
///
/// `bin_dir` 只用来放产物 —— 判据不看产物，看的是「build 的前置 check 说了什么」。
fn build_verdict(file: &Path, bin_dir: &Path) -> (i32, Vec<String>) {
    let stem = file.file_stem().unwrap_or_default().to_string_lossy();
    let out = Command::new(env!("CARGO_BIN_EXE_rgoc-driver"))
        .args(["build", "--emit=elf", "-o"])
        .arg(bin_dir.join(stem.as_ref()))
        .arg(file)
        .output()
        .expect("起不了 rgoc-driver build");
    let mut all = String::from_utf8_lossy(&out.stdout).into_owned();
    all.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), codes_of(&all))
}

// ════════════════════════════════════════════════════════════════════
// T-GEN-08a `check` 与 `build` 的口径一致
// ════════════════════════════════════════════════════════════════════

/// 同一份源码，`check` 与 `build` 的**前置检查结论**必须相同。
///
/// # 判据形态
///
/// * `check` 过（rc=0）⇒ `build` 的 stderr 里**没有**任何检查面诊断码。
///   （build 可以因为下游原因失败 —— `clang` 链接失败、`SSA` 拒、verifier
///   拒 —— 但那些不带 `CHK-*` / `TYP-*` 的码。）
/// * `check` 不过 ⇒ `build` 也报出那些码（检查面结论不会被 `build` 放宽）。
///
/// # 为什么它算 M12 的判据而不是「早就该有的卫生检查」
///
/// 这两条路径走的是**同一个** `check_package_with`（`build_cmd.rs:84` /
/// `check_cmd.rs`），唯一区别是 `build` 传了 lowering 闭包。所以「结论不同」
/// 只可能来自 lowering 期新报的诊断 —— 而那些诊断在 `M12a` 里会**成片增加**
/// （实例化后 body 才真正被检查）。没有这条判据，`M12a` 的进展就会被
/// 「check 绿了、build 红着」这种分裂吃掉，且**看不出来是谁的错**。
///
/// # 今天的状态（`F14` 的**完整**口径）
///
/// `M12-baseline.md` 记的是「7 个不一致」，那是按 `build_kind = other` 数的
/// （`TYP-INVALID-CONVERSION` 1 / `CHK-INVALID-OPERATION` 5 / clang 链接失败 1）。
/// 本判据的口径更宽，于是**当天实测 29 个**：
///
/// * `HIR-MISSING-TYPE` 6 —— lowering 拿不到类型（类型参数没实例化）；
/// * `CHK-INVALID-SELECTOR` 7 —— 实例上的字段/方法选择器在 lowering 期才报
///   （就是记忆里那条「类型表达式两份实现」的坑，Task 15）；
/// * `CHK-INVALID-OPERATION` 4 / `TYP-INVALID-CONVERSION` 1 —— 类型参数上的
///   运算与转换（Task 8）；
/// * 其余是同一批文件的不同码。
///
/// 宽口径是**故意**的：这条判据要当 `M12a` 的**总账**用 —— 它必须收敛到 0，
/// 而不是「`build_kind` 的分类里没有 other」。窄口径会让 22 个文件在
/// 「check 绿 / build 红」的分裂里躺着，而没人看见。
///
/// 第 7 个（`issue52124.go`）是 **clang 链接失败**，不带检查面码 ⇒ 按本判据
/// **算一致**。这条差别写在断言消息里，免得下一个人以为是漏了。
#[test]
fn t_gen_08a_check_与_build_口径一致() {
    let lists = repo_root().join("target/m12-checkface-lists");
    let bins = repo_root().join("target/m12-checkface-bins");
    let _ = fs::remove_dir_all(&lists);
    let _ = fs::remove_dir_all(&bins);
    fs::create_dir_all(&bins).expect("建不了产物目录");

    classify(&lists);
    let mut files: Vec<String> = read_list(&lists, "run");
    files.extend(read_list(&lists, "compile"));
    assert_eq!(files.len(), 111, "门禁文件数变了（run + compile 应当 111）");

    let mut split: Vec<String> = Vec::new();
    for b in &files {
        let f = corpus().join(b);
        let (crc, ccodes) = check_verdict(&f);
        let (brc, bcodes) = build_verdict(&f, &bins);
        if crc == 0 && !bcodes.is_empty() {
            split.push(format!(
                "{b}: check 过（rc=0），但 build 报 {:?}（build rc={brc}）",
                bcodes
            ));
        } else if crc != 0 {
            let missing: Vec<&String> = ccodes.iter().filter(|c| !bcodes.contains(c)).collect();
            if !missing.is_empty() {
                split.push(format!(
                    "{b}: check 报 {:?}（rc={crc}），build 少了 {:?}（build rc={brc}）",
                    ccodes, missing
                ));
            }
        }
    }

    assert!(
        split.is_empty(),
        "{} 个文件的 check / build 口径不一致。\n这两条路径共用同一个 \
         `check_package_with`，差别只有 lowering —— 所以差异来自 lowering 期\
         新报的诊断。\n（注：clang 链接失败**不算**不一致：它不带检查面诊断码。）\n\n{}",
        split.len(),
        split.join("\n")
    );
}

/// 门禁清单与语料对得上：每条都是真存在的文件，且 run / compile 不相交。
///
/// 这条是给上面那条**兜底**的：若 `classify` 因为环境变量指错目录而返回一份
/// 空清单，上面那条会「0 个不一致」地通过 —— 一个不跑的判据。
#[test]
fn t_gen_08a_门禁清单非空且文件存在() {
    let lists = repo_root().join("target/m12-checkface-lists2");
    let _ = fs::remove_dir_all(&lists);
    classify(&lists);
    let run = read_list(&lists, "run");
    let comp = read_list(&lists, "compile");
    assert!(run.len() >= 60, "run 清单只有 {} 条（应当 ~71）", run.len());
    assert!(comp.len() >= 30, "compile 清单只有 {} 条", comp.len());
    let a: BTreeSet<&String> = run.iter().collect();
    let b: BTreeSet<&String> = comp.iter().collect();
    assert!(a.is_disjoint(&b), "run 与 compile 清单有交集");
    for f in run.iter().chain(comp.iter()) {
        assert!(corpus().join(f).is_file(), "清单里的 {f} 不存在");
    }
}
