//! 顺序语料闭环的**清单纪律 + rgoc 侧自足**（`EX-6` / Task 46；`M5-tests.md` §3.2）。
//!
//! # 这条测试盖住哪几个 ID
//!
//! | ID | 判据 | 本文件怎么钉 |
//! |---|---|---|
//! | `T-EX-41` | 候选恰 80 行；`入选 ∪ 排除 == 候选`（**两向**）；无死行、无重复 | [`lists_are_disjoint_and_complete`] |
//! | `T-EX-42` | 每个入选文件 rgoc 侧自足：退出码 0、stderr 空、无 panic | [`selected_files_are_self_sufficient`] |
//! | `T-EX-43` | 入选集上「合并流 == `<同名>.out`（无 `.out` ⇒ 期望为空）」**逐字节** | [`three_way_agreement_against_dot_out`] |
//! | `T-EX-44` | `ken/` 子集单独一组（32 个候选，全部入选） | [`ken_subset_is_complete`] |
//! | `T-EX-45` | 5 个 `runoutput` 全部在排除清单，理由是「动态编译」 | [`runoutput_family_is_excluded`] |
//! | `T-EX-46` | 不确定性守卫：含地址输出的 fixture 若混进入选集**必红** | [`address_output_would_turn_the_list_red`] |
//! | `T-EX-47` | 排除清单逐行四栏俱全、类别合法、理由不含 TODO | [`excluded_lines_carry_reason_and_pointer`] |
//! | `T-EX-48` | 同一批语料连跑两次，合并流逐字节相同 | [`repeated_runs_are_byte_identical`] |
//! | `T-EX-56` | 跑完整个批次后 `go_source_code/` 无文件被写（哈希校验） | [`corpus_files_are_untouched`] |
//!
//! # 为什么**不**在这里比字节
//!
//! 判据的**主**半边（合并流 vs 官方 `go run` 逐字节 + 退出码）需要 `go`，
//! 那是门禁脚本 `tools/run-oracle/run-collide.sh` 的活（`EX-6` 的 `O` 腿）。
//!
//! 但**官方判据本身**（`M5-tests.md` §4.1）不需要 `go`：
//!
//! ```text
//! rgoc 合并流 == <同名>.out 的内容（无 .out ⇒ 期望为空）
//! ```
//!
//! 所以 `T-EX-43` 的这一半在这里就能钉死 —— 而它恰好是最容易被装置偷懒偷掉的一半
//! （把「没有 `.out`」当成「不用比」）。`go run` 那一路负责回答另一个问题：
//! 「`.out` 本身有没有跟官方实现脱节」（`--three-way`）。两条腿合起来才是完整的
//! `T-EX-43`；少一条都只是半句话。
//!
//! # 数字为什么敢写死
//!
//! 语料被 `corpus-manifest.sha256` 锁死（`AGENTS.md` §3.1：只读 + 哈希校验），
//! 三个清单文件也随仓库冻结 ⇒ 「候选 80 / 入选 69 / 排除 11」是**精确值**。
//! 任何一处悄悄多收一个文件、把某个文件从排除挪回入选而不改数字，都会在这里变红。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rgoc_driver::cli::RunCommand;
use rgoc_driver::run_cmd::run_run;

/// 候选总数（`run-candidates.txt` 的非注释行数）。
const CANDIDATES: usize = 80;
/// 入选总数（`run-selected.txt` 的非注释行数）。
const SELECTED: usize = 69;
/// 排除总数（`run-excluded.txt` 的非注释行数）。
const EXCLUDED: usize = 11;
/// `ken/` 子集在候选里的条数（`M5-tests.md` §6.1「`ken/` 32」）。
const KEN: usize = 32;
/// 允许的排除类别（清单头部声明的五类，多一个都算口径漂移）。
const CATEGORIES: [&str; 5] = ["runoutput", "stdlib", "nondet", "unsupported", "rule"];

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达（cargo 的 CARGO_MANIFEST_DIR 变了？）")
}

fn list_path(name: &str) -> PathBuf {
    repo_root().join("tools/run-oracle").join(name)
}

/// 读清单的**非注释非空**行（保留列，供排除清单切四栏）。
fn read_rows(name: &str) -> Vec<String> {
    let path = list_path(name);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到 {}：{e}", path.display()));
    text.lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#')
        })
        .map(ToString::to_string)
        .collect()
}

/// 排除清单每行的第一栏（文件路径）。
fn excluded_paths() -> Vec<String> {
    read_rows("run-excluded.txt")
        .iter()
        .map(|l| l.split('\t').next().unwrap_or("").trim().to_string())
        .collect()
}

// ───────────────────────── T-EX-41 ─────────────────────────

/// `T-EX-41`：候选恰 80；`入选 ∪ 排除 == 候选`（两向）；无重复、无死行。
///
/// 「两向」是刻意的：只断言 `入选 ⊆ 候选` 会漏掉「排除清单里塞了一个
/// 不在候选里的幽灵文件」（数字凑得上，实际多排了一个）；只断言
/// `候选 ⊆ 入选 ∪ 排除` 会漏掉「同一个文件既入选又排除」（`run-collide`
/// 会跑它，`run-excluded` 又说它不算 —— 两边各说各话）。
#[test]
fn lists_are_disjoint_and_complete() {
    let cands: Vec<String> = read_rows("run-candidates.txt");
    let sel: Vec<String> = read_rows("run-selected.txt");
    let exc = excluded_paths();

    assert_eq!(
        cands.len(),
        CANDIDATES,
        "候选清单非注释行数变了（期望 {CANDIDATES}，实际 {}）—— 语料被哈希锁死，\
         行数变化说明清单被改过",
        cands.len()
    );
    assert_eq!(sel.len(), SELECTED, "入选清单行数变了（期望 {SELECTED}）");
    assert_eq!(exc.len(), EXCLUDED, "排除清单行数变了（期望 {EXCLUDED}）");

    let cset: BTreeSet<&String> = cands.iter().collect();
    assert_eq!(cset.len(), cands.len(), "候选清单里有重复行");

    let sset: BTreeSet<&String> = sel.iter().collect();
    assert_eq!(sset.len(), sel.len(), "入选清单里有重复行");

    let eset: BTreeSet<&String> = exc.iter().collect();
    assert_eq!(eset.len(), exc.len(), "排除清单里有重复行");

    // 方向 ①：两边都在候选里（没有幽灵行）
    for f in sset.iter().chain(eset.iter()) {
        assert!(
            cset.contains(f),
            "{f} 不在候选清单里 —— 入选/排除只能来自候选（§6.3 判据 1）"
        );
    }
    // 方向 ②：不相交
    let both: Vec<&&String> = sset.intersection(&eset).collect();
    assert!(both.is_empty(), "既入选又排除：{both:?}");

    // 方向 ③：并集 == 候选（没有死行）
    let union: BTreeSet<&String> = sset.union(&eset).copied().collect();
    assert_eq!(
        union,
        cset,
        "入选 ∪ 排除 != 候选 —— 差额：{:?}",
        cset.difference(&union).collect::<Vec<_>>()
    );

    // 算术面：候选 == 入选 + 排除
    assert_eq!(
        CANDIDATES,
        SELECTED + EXCLUDED,
        "80 != 69 + 11：三个常量之间已经不自洽"
    );
}

// ───────────────────────── T-EX-42 ─────────────────────────

/// `T-EX-42`（rgoc 侧半句）：每个入选文件退出码 0、stderr 空、stdout 不含 `panic:`。
///
/// 为什么把「stdout 不含 `panic:`」也钉上：`// run` 的官方语义是「跑通」，
/// 一个以未捕获 panic 结束的程序退出码是 2（`§4.3`），但如果哪天退出码表被
/// 改坏成「panic 也算 0」，这一条会立刻红 —— 它盯的是**程序跑完了**，
/// 不是**进程退出了**。
#[test]
fn selected_files_are_self_sufficient() {
    let root = repo_root();
    let mut bad: Vec<String> = Vec::new();
    for rel in read_rows("run-selected.txt") {
        let cmd = RunCommand {
            files: vec![root.join(&rel)],
            dump_hir: false,
            max_steps: None,
            lang: None,
            via: rgoc_driver::cli::Via::Hir,
            opt: rgoc_ssa::opt::Config::all(),
        };
        match run_run(&cmd) {
            Ok(o) => {
                if o.exit != 0 {
                    bad.push(format!("{rel}: 退出码 {}（期望 0）", o.exit));
                }
                if !o.stderr.is_empty() {
                    bad.push(format!("{rel}: stderr 非空：{}", o.stderr.trim()));
                }
                if o.stdout.contains("panic:") {
                    bad.push(format!("{rel}: 输出里有未捕获 panic"));
                }
            }
            Err(e) => bad.push(format!("{rel}: 内部错 {e}")),
        }
    }
    assert!(
        bad.is_empty(),
        "入选集有 {} 条不达标：\n{}",
        bad.len(),
        bad.join("\n")
    );
}

// ───────────────────────── T-EX-44 ─────────────────────────

/// `T-EX-44`：`ken/` 子集 32 个，**全部**入选。
///
/// `ken/` 是最老的语义测试（for / range / 接口 / 指针 / 移位），
/// 它们能全绿才算「顺序语义基线」站得住。所以这里不接受「ken 少一两个也行」：
/// 32 个候选必须 32 个入选。
#[test]
fn ken_subset_is_complete() {
    let cands: Vec<String> = read_rows("run-candidates.txt");
    let ken_c: Vec<&String> = cands.iter().filter(|f| f.contains("/ken/")).collect();
    assert_eq!(ken_c.len(), KEN, "候选里的 ken/ 条数变了（期望 {KEN}）");

    let sel: BTreeSet<String> = read_rows("run-selected.txt").into_iter().collect();
    let missing: Vec<&&String> = ken_c.iter().filter(|f| !sel.contains(**f)).collect();
    assert!(
        missing.is_empty(),
        "ken/ 有 {} 个没入选：{missing:?}",
        missing.len()
    );
}

// ───────────────────────── T-EX-45 ─────────────────────────

/// `T-EX-45`：5 个 `runoutput` 文件全在排除清单，且理由写的是「动态编译」。
///
/// 不是「理由里出现了 runoutput 这个词」就算 —— 要求理由里同时出现
/// 「动态编译」和「M5 不做」两段，否则一个「TODO」式的占位行也能过。
#[test]
fn runoutput_family_is_excluded() {
    const RUNOUTPUT: [&str; 5] = [
        "go_source_code/test/index0.go",
        "go_source_code/test/rotate0.go",
        "go_source_code/test/rotate1.go",
        "go_source_code/test/rotate2.go",
        "go_source_code/test/rotate3.go",
    ];
    let rows = read_rows("run-excluded.txt");
    for f in RUNOUTPUT {
        let row = rows
            .iter()
            .find(|l| l.split('\t').next().map(str::trim) == Some(f))
            .unwrap_or_else(|| panic!("{f} 不在排除清单里（runoutput 必须排除）"));
        let cols: Vec<&str> = row.split('\t').collect();
        assert_eq!(cols[1].trim(), "runoutput", "{f} 的类别应为 runoutput");
        assert!(
            cols[2].contains("动态编译"),
            "{f} 的理由没写清「动态编译」：{}",
            cols[2]
        );
        assert!(cols[2].contains("M5 不做"), "{f} 的理由没写「M5 不做」");
    }
}

// ───────────────────────── T-EX-47 ─────────────────────────

/// `T-EX-47`：排除清单逐行**四栏**俱全、类别合法、理由非空且不含 TODO。
///
/// 四栏 = `<路径>\t<类别>\t<理由>\t<指针>`。少一栏就是「理由缺失」，
/// 而「理由缺失」正是 §11.2 要防的那个坏形态（为了让数字好看移出，
/// 又不留下为什么）。
#[test]
fn excluded_lines_carry_reason_and_pointer() {
    let rows = read_rows("run-excluded.txt");
    assert_eq!(rows.len(), EXCLUDED);
    for (i, row) in rows.iter().enumerate() {
        let cols: Vec<&str> = row.split('\t').collect();
        assert_eq!(
            cols.len(),
            4,
            "第 {} 行不是四栏（实际 {} 栏）：{row}",
            i + 1,
            cols.len()
        );
        let (path, cat, reason, pointer) = (
            cols[0].trim(),
            cols[1].trim(),
            cols[2].trim(),
            cols[3].trim(),
        );
        assert!(
            path.starts_with("go_source_code/"),
            "第 {} 行路径不像语料：{path}",
            i + 1
        );
        assert!(
            CATEGORIES.contains(&cat),
            "第 {} 行类别 `{cat}` 不在 {CATEGORIES:?} 里",
            i + 1
        );
        assert!(
            reason.len() >= 8,
            "第 {} 行理由太短（像是占位）：`{reason}`",
            i + 1
        );
        let upper = reason.to_uppercase();
        assert!(
            !upper.contains("TODO") && !upper.contains("FIXME") && !upper.contains("待补"),
            "第 {} 行理由里留着 TODO：`{reason}`",
            i + 1
        );
        // 指针必须是**可复核的**：章节号 / 设计锚 / 测试 ID 三者之一。
        assert!(
            pointer.contains('§') || pointer.starts_with('U') || pointer.contains('F'),
            "第 {} 行的指针不可复核：`{pointer}`",
            i + 1
        );
    }
}

// ───────────────────────── 清单与磁盘一致 ─────────────────────────

/// 清单里的每个路径都**真的存在**（语料目录只读，路径写错不会有人替你发现）。
#[test]
fn every_listed_path_exists() {
    let root = repo_root();
    let mut missing: Vec<String> = Vec::new();
    for rel in read_rows("run-candidates.txt") {
        if !root.join(&rel).is_file() {
            missing.push(rel);
        }
    }
    assert!(missing.is_empty(), "清单里有不存在的文件：{missing:?}");
}

// ───────────────────────── 共用的跑法 ─────────────────────────

/// 跑一个源文件，返回 `(合并流, 退出码)`。
///
/// 合并流 = `RunOutcome::stdout`（`print`/`println` 与未捕获 panic 都在里面，
/// `D-M5-6` 的单 buffer）。唯一容差 `\r\n → \n` 在这里就归一掉 —— 与官方
/// `testdir_test.go:1184` 及 `run-collide.sh` 的 `perl -pi` 同款。
fn run_file(path: &Path) -> (String, u8) {
    let o = run_run(&RunCommand {
        files: vec![path.to_path_buf()],
        dump_hir: false,
        max_steps: None,
        lang: None,
        via: rgoc_driver::cli::Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
    })
    .unwrap_or_else(|e| panic!("跑 {} 失败（内部错）：{e}", path.display()));
    (o.stdout.replace("\r\n", "\n"), o.exit)
}

/// `<同名>.out`：**把 `.go` 换成 `.out`**（官方 `testdir_test.go:1176`）。
///
/// ⚠️ 不是「在 `.go` 后面接 `.out`」。这个差别曾经让 `run-collide.sh --three-way`
/// 静默退化成两方比对（见该脚本 `one()` 里的修复注记）—— 一个拼错的路径不会报错，
/// 只会让「有期望值的文件」凭空变成「没有期望值」。
fn dot_out_for(go_path: &Path) -> PathBuf {
    go_path.with_extension("out")
}

// ───────────────────────── T-EX-43 ─────────────────────────

/// `T-EX-43`：官方判据的**字面形态** —— 合并流 == `<同名>.out`，无 `.out` ⇒ 期望为空。
///
/// 这条与 [`selected_files_are_self_sufficient`] 的分工：
///
/// | | 断言 |
/// |---|---|
/// | `T-EX-42`（rgoc 侧） | 退出码 0、stderr 空、没有未捕获 panic |
/// | `T-EX-43`（本测试） | 输出**内容**等于官方期望（`.out` 或空） |
///
/// 「无 `.out` ⇒ 期望为空」是官方语义（`testdir_test.go:1178` 的
/// `// File is allowed to be missing, in which case output should be empty.`）。
/// 少了这半句，一个「什么都没打印」的文件与一个「打印了 3 行」的文件在装置里
/// 长得一模一样 —— 那正是 `T-EX-70` 那条变异要打的东西。
#[test]
fn three_way_agreement_against_dot_out() {
    let root = repo_root();
    let mut bad: Vec<String> = Vec::new();
    let mut with_out = 0usize;
    // 分母之二：**真的被比过**的文件数。`with_out` 只数「有期望值」的那几个，
    // 数不到「无 `.out` ⇒ 期望为空」这一半 —— 而后者才是入选集的主体（64/69）。
    // 少了这个计数，「无 `.out` 当成不比」的变异（`T-EX-70`）会**整段消失而不报错**：
    // 剩下 5 个文件全绿，测试照样 ok（实测过，见 `scripts/mutation-m5-notes.md`）。
    let mut compared = 0usize;
    for rel in read_rows("run-selected.txt") {
        let src = root.join(&rel);
        let (got, exit) = run_file(&src);
        if exit != 0 {
            bad.push(format!("{rel}: 退出码 {exit}（判据只接受 0）"));
            continue;
        }
        let expect_path = dot_out_for(&src);
        let want = match std::fs::read_to_string(&expect_path) {
            Ok(s) => {
                with_out += 1;
                s.replace("\r\n", "\n")
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                bad.push(format!("{rel}: 读不到 {}：{e}", expect_path.display()));
                continue;
            }
        };
        compared += 1;
        if got != want {
            bad.push(format!(
                "{rel}: 合并流与期望不同（期望 {} 字节，实际 {} 字节）\n  期望={want:?}\n  实际={got:?}",
                want.len(),
                got.len()
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "入选集有 {} 条与官方期望不符：\n{}",
        bad.len(),
        bad.join("\n")
    );
    // 分母写出来：一个 `.out` 都没有时这条测试仍然「绿」（空集合上的全称命题），
    // 而那不是证据。实测 5 个，写死以防有人把 `.out` 全删了还以为在比。
    assert_eq!(
        with_out, 5,
        "有 `.out` 的入选文件应恰为 5 个（helloworld / print / printbig / ken/cplx0 / ken/string）"
    );
    // 另一半分母：69 个入选文件**每一个**都必须进过比较（含 64 个「期望为空」的）。
    // 只断言 `with_out` 会漏掉「无 `.out` 直接跳过」这一类变异 —— 那正是 `T-EX-70`。
    assert_eq!(
        compared, 69,
        "入选集应逐条比较（含「无 `.out` ⇒ 期望为空」的 64 条），实际只比了 {compared} 条"
    );
}

// ───────────────────────── T-EX-46 ─────────────────────────

/// `T-EX-46`：不确定性守卫的**探针** —— 含地址输出的 fixture 混进入选集必红。
///
/// `U-M5-02`（地址数值不可复现）不是「我们不想比」而是「没法比」：官方每次跑
/// 打印的地址都不同（实测 `0x2ba966b54e90` 这类，ASLR + 堆布局）。本仓的
/// `Value::Ptr` 打印的是**稳定的**假地址（`0xc000000000`），所以这个文件在本仓
/// 内部两次跑是一样的 —— **恰恰因此更危险**：它会被误当成「确定」的语料收进来。
///
/// 探针要证明的只有一件事：**判据是逐字节的**，所以这类文件混进入选集（期望为空）
/// 一定红。三条断言分别对应三个失效点：
///
/// 1. 输出**非空** —— 否则「期望为空」与它相等，什么都抓不到；
/// 2. 输出里有 `0x` —— 确认它真的是地址，不是别的输出；
/// 3. 输出 != 空串 —— 即「塞进入选集 ⇒ 必红」，这是 `T-EX-46` 的正向证据。
#[test]
fn address_output_would_turn_the_list_red() {
    let dir = repo_root().join("rgoc/target/tmp/run-corpus");
    std::fs::create_dir_all(&dir).expect("建不了 tmp 目录");
    let src = dir.join("t_ex_46_addr.go");
    std::fs::write(
        &src,
        "package main\n\nfunc main() {\n\tx := 1\n\tprintln(&x)\n}\n",
    )
    .expect("写不了 fixture");

    let (got, exit) = run_file(&src);
    assert_eq!(exit, 0, "探针本身必须能跑通（否则它证明不了判据的事）");
    assert!(
        !got.trim().is_empty(),
        "探针没打印任何东西 —— 那它证明不了「非空输出会被判据抓住」"
    );
    assert!(
        got.contains("0x"),
        "探针的输出不像地址（期望含 `0x`）：{got:?}"
    );
    // 入选集里没有 `.out` 的文件，期望就是空串；这条把它判红。
    assert_ne!(
        got, "",
        "含地址输出的文件若混进入选集，判据必须判红（否则 T-EX-70 的守卫是摆设）"
    );

    // 另一半：本仓内部**两次跑相同**（稳定假地址）。这是「不确定」的伪装 ——
    // 记下来，免得以后有人拿「我们自己跑两次一样」当「可以入集」的理由。
    let (again, _) = run_file(&src);
    assert_eq!(
        got, again,
        "本仓的指针打印是稳定的假地址（这是 U-M5-02 的成因，不是判据的漏洞）"
    );
}

// ───────────────────────── T-EX-48 ─────────────────────────

/// `T-EX-48`：同一批语料连跑两次，合并流**逐字节**相同。
///
/// 不确定性有两个来源，都要挡：
/// * **容器内**：`HashMap` 迭代序、`Rc` 地址、时间戳混进输出 —— 同一进程里两次
///   跑就可能不同，所以这条在同一进程里跑两遍就能抓。
/// * **跨进程**：ASLR、分配器布局 —— 那要两次**容器调用**才抓得到，由门禁脚本
///   连跑两次 + `sha256` 比对（`M5-report.md` §7 有实测记录）。
///
/// 这里做前者。68 个文件 × 2 次，秒级。
#[test]
fn repeated_runs_are_byte_identical() {
    let root = repo_root();
    let mut bad: Vec<String> = Vec::new();
    for rel in read_rows("run-selected.txt") {
        let src = root.join(&rel);
        let (a, ea) = run_file(&src);
        let (b, eb) = run_file(&src);
        if a != b || ea != eb {
            bad.push(format!(
                "{rel}: 两次运行不同（退出码 {ea}/{eb}；字节 {} / {}）",
                a.len(),
                b.len()
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "有 {} 个文件不是确定性的：\n{}",
        bad.len(),
        bad.join("\n")
    );
}

// ───────────────────────── T-EX-56 ─────────────────────────

/// `T-EX-56`：跑完整个批次后，`go_source_code/` **没有任何文件被写**。
///
/// 语料是**只读输入**（`AGENTS.md` §3.1，`corpus-manifest.sha256` 锁定）。
/// 「跑测试顺手把 `.out` 写回语料目录」是这类项目最常见的自伤：本地看着一切正常，
/// 而下一台机器上「期望值」已经被上一次运行改过了 —— 判据从此自我实现。
///
/// 校验范围是**本批次真的碰过的文件**（80 个候选 + 它们的 `.out`），不是全部
/// 15618 行：那 80 个才是「跑完批次」这句话的主语，而全量校验是另一条命令
/// （`sha256sum -c corpus-manifest.sha256`，`M5-report.md` §7 有记录）。
#[test]
fn corpus_files_are_untouched() {
    let root = repo_root();
    let manifest = std::fs::read_to_string(root.join("corpus-manifest.sha256"))
        .expect("读不到 corpus-manifest.sha256");

    // 要校验的路径集合：候选 + 它们的 `.out`，但**只取磁盘上真有的**
    // （`.out` 是可选的，绝大多数文件没有 —— 没有的文件不该出现在哈希清单里）。
    let mut want: BTreeSet<String> = BTreeSet::new();
    for rel in read_rows("run-candidates.txt") {
        for p in [
            rel.clone(),
            Path::new(&rel)
                .with_extension("out")
                .to_string_lossy()
                .into_owned(),
        ] {
            if root.join(&p).is_file() {
                want.insert(p);
            }
        }
    }

    let mut lines: Vec<String> = Vec::new();
    let mut covered: BTreeSet<String> = BTreeSet::new();
    for line in manifest.lines() {
        // 格式：`<64 hex>  <path>`（两个空格）
        let Some((hash, path)) = line.split_once("  ") else {
            continue;
        };
        if want.contains(path) {
            lines.push(format!("{hash}  {path}"));
            covered.insert(path.to_string());
        }
    }

    // 「清单里查不到」= 语料文件被加进来了却没进哈希清单 —— 那也是被写过的一种。
    let missing: Vec<&String> = want.difference(&covered).collect();
    assert!(
        missing.is_empty(),
        "这些文件不在 corpus-manifest.sha256 里（语料被动过？）：{missing:?}"
    );

    let input = dir_of_tmp().join("t_ex_56_subset.sha256");
    std::fs::write(&input, lines.join("\n") + "\n").expect("写不了临时清单");

    // 容器里有 `sha256sum`（coreutils）；宿主 macOS 上是 `shasum -a 256`。
    // 两个都没有就**响亮失败**，不静默跳过 —— 跳过等于这条测试不存在。
    let attempts: [(&str, &[&str]); 2] = [
        ("sha256sum", &["-c", "--quiet"]),
        ("shasum", &["-a", "256", "-c"]),
    ];
    let mut last_err = String::new();
    for (bin, args) in attempts {
        let out = std::process::Command::new(bin)
            .args(args)
            .arg(&input)
            .current_dir(&root)
            .output();
        match out {
            Ok(o) if o.status.success() => return,
            Ok(o) => panic!(
                "语料被写过（{bin} -c 失败）：\n{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            ),
            Err(e) => last_err = format!("{bin}: {e}"),
        }
    }
    panic!("这台机器上没有 sha256 校验工具（{last_err}），T-EX-56 无法验证");
}

/// 测试用的临时目录（与 `run_semantics.rs` 同款位置，进 `target/` 不进仓库）。
fn dir_of_tmp() -> PathBuf {
    let d = repo_root().join("rgoc/target/tmp/run-corpus");
    std::fs::create_dir_all(&d).expect("建不了 tmp 目录");
    d
}
