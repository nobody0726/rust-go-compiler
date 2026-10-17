//! `rgoc types` 端到端（`T-TY-81..85`）—— M3/Task 36。
//!
//! # 这一层测什么
//!
//! 前面各 Task 的测试都在 **crate 内**（`rgoc-load` / `rgoc-types` 直接调库）。
//! 这里换到**进程边界**上再问一遍同一批问题 —— 因为「库判对了」不等于「命令行
//! 把结论交出来了」：退出码、两条通道（stdout/stderr）、诊断代号都可能在这一层丢。
//!
//! # 每条都是**成对**的（`M3-tests.md` §3.8）
//!
//! 正向（该拒的确实拒、拒的理由是**稳定**的）+ 反向（不该拒的不拒）。
//! **缺一边不算过** —— M2 已经抓过 6 条以上「只测一边就恒真」的断言。
//!
//! | ID | 正向 | 反向 |
//! |---|---|---|
//! | `T-TY-81` | 约束接口（项集 / `comparable`）的同一性 ⇒ `reject` | 纯方法集接口、**具名**约束接口 ⇒ 按名字判，不 `reject` |
//! | `T-TY-82` | `TYP-13/18/19/23..26` 的判定请求 ⇒ 稳定拒绝码 | 相邻的 M3 支持项（`identical` / `representable`）⇒ 有答 |
//! | `T-TY-83` | 需类型信息的 `const` 初始化式 ⇒ 稳定拒绝码 | 常量子集内 ⇒ 不报 |
//! | `T-TY-84` | 未知 `//go:` 指令 ⇒ 前端**不**报错（`U-M3-09`） | `//go:build` 的版本**被读取**（高版本 ⇒ `TYP-FILE-REQUIRES-NEWER-GO`） |
//! | `T-TY-85` | 模块路径 / 二进制 export data ⇒ 稳定拒绝码 | 源码路径 import（有 roots）⇒ 不报 |
//!
//! # 退出码语义的一条**空条款**（记账，不是漏测）
//!
//! `M3-plan.md` Task 36 Step 1 写「仅警告 ⇒ 0；有 error ⇒ 非 0」。M3 的 [`rgoc_lex::Diag`]
//! **没有严重级别**（`pos` / `code` / `message` 三个字段：一个诊断要么有要么没有）
//! ⇒ 「仅警告」这个前提在 M3 上**不存在**，那一半是空条款。测试改为钉住**真实的**
//! 那条：`exit == 0 ⟺ diagnostics == 0`（外加 `reject` 不是 error —— 它退出码仍是 0）。
//! 想让它有内容，得等真出现**非致命**诊断（那时才有「警告」这一类）。
//!
//! # `T-TY-82` 的码为什么是驱动层的
//!
//! `TYP-CMP-KIND-UNSUPPORTED` **不是** [`rgoc_lex::DiagCode`]：它说的是「这个
//! **请求**我们不判」，没有官方对应代号、也不来自某个源码位置 —— 与 `T-TY-81` 的
//! `TYP-UNSUPPORTED-CONSTRAINT-IDENTITY`（那条**是**语言层的能力边界）同理不同层。
//!
//! `//rgoc:cmp` 的 kind 集合**与 oracle 侧共用且封闭**（都只认 `identical` /
//! `representable`）：fixture 由两侧读**同一份**文件，输入语言分叉比「少支持一个
//! kind」坏得多。「拼错的 kind」与「M3 不判的判定」在语法上不可分 ⇒ 同一条码。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 每个用例一个**新**临时目录（同名先删）—— 用例之间不许共享状态。
fn tmp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rgoc-m3-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

/// 在 `dir` 下写一个文件（自动建父目录），返回它的路径。
fn write(dir: &Path, rel: &str, src: &str) -> PathBuf {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).expect("建父目录");
    }
    std::fs::write(&p, src).expect("写临时 .go");
    p
}

/// 跑一次 `rgoc-driver <args…>` ⇒ `(退出码, stdout, stderr)`。
fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rgoc-driver"))
        .args(args)
        .output()
        .expect("起 rgoc-driver 进程");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// `types <file>`（无 flag ⇒ 出摘要 + 诊断）。
fn run_types(file: &Path) -> (i32, String, String) {
    run(&["types", &file.to_string_lossy()])
}

/// 取摘要里的 `diagnostics N`。
fn diag_count(stdout: &str) -> u32 {
    stdout
        .lines()
        .find_map(|l| l.strip_prefix("diagnostics "))
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or_else(|| panic!("摘要里没有 `diagnostics N`：{stdout}"))
}

/// 取第一行含 `needle` 的行。**按行断言**，不整段比 —— 行序是规格的一部分。
fn line_with<'a>(s: &'a str, needle: &str) -> &'a str {
    s.lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("找不到含 {needle:?} 的行：{s}"))
}

// ─────────────────────────── T-TY-81 ───────────────────────────

/// `T-TY-81`：约束接口的同一性 —— 正向 `reject`，反向（方法集 / 具名）**不**拒。
#[test]
fn ty81_约束接口同一性_正向reject_反向不拒() {
    let d = tmp_root("ty81");

    // 正向：两侧都是匿名接口 + 至少一侧含项集 ⇒ 判不了 ⇒ `reject`（带稳定码）。
    let fwd = write(
        &d,
        "fwd.go",
        "package p\n\n//rgoc:cmp identical interface{ ~int } => interface{ ~int | ~string }\n",
    );
    let (code, out, err) = run(&["types", "--cmp", &fwd.to_string_lossy()]);
    assert_eq!(
        code, 0,
        "reject **不是**错误（D-M3-13）⇒ 退出码仍 0；stderr={err}"
    );
    assert!(err.is_empty(), "reject 走 stdout，stderr 该空：{err}");
    assert!(
        line_with(&out, "identical").ends_with("\treject"),
        "正向该 reject：{out}"
    );
    assert!(
        out.contains("# reject 3 TYP-UNSUPPORTED-CONSTRAINT-IDENTITY"),
        "reject 要带**稳定码**（物理行号 3）：{out}"
    );

    // 反向甲：纯方法集接口 ⇒ 判得了，**不**出现 reject。
    let rev_a = write(
        &d,
        "rev_a.go",
        "package p\n\n\
         //rgoc:cmp identical interface{ String() string } => interface{ String() string }\n\
         //rgoc:cmp identical interface{ String() string } => interface{ Error() string }\n",
    );
    let (code, out, err) = run(&["types", "--cmp", &rev_a.to_string_lossy()]);
    assert_eq!(code, 0, "反向该零诊断；stderr={err}");
    assert_eq!(
        line_with(&out, "3\tidentical"),
        "3\tidentical\ttrue",
        "{out}"
    );
    assert_eq!(
        line_with(&out, "4\tidentical"),
        "4\tidentical\tfalse",
        "{out}"
    );
    assert!(
        !out.contains("reject"),
        "方法集接口**不该**被拒（拒面窄于 U-M3-05 的措辞，R-32-1）：{out}"
    );

    // 反向乙：**具名**约束接口 ⇒ 按名字 / 同一份声明判，也不拒。
    let rev_b = write(
        &d,
        "rev_b.go",
        "package p\n\n\
         type Num interface{ ~int | ~float64 }\n\
         type Num2 interface{ ~int | ~float64 }\n\n\
         //rgoc:cmp identical Num => Num\n\
         //rgoc:cmp identical Num => Num2\n",
    );
    let (code, out, err) = run(&["types", "--cmp", &rev_b.to_string_lossy()]);
    assert_eq!(code, 0, "具名约束接口该零诊断；stderr={err}");
    assert_eq!(
        line_with(&out, "6\tidentical"),
        "6\tidentical\ttrue",
        "{out}"
    );
    assert_eq!(
        line_with(&out, "7\tidentical"),
        "7\tidentical\tfalse",
        "{out}"
    );
    assert!(!out.contains("reject"), "具名约束接口不该被拒：{out}");
}

// ─────────────────────────── T-TY-82 ───────────────────────────

/// `T-TY-82`：`TYP-13/18/19/23..26` 的判定请求 ⇒ 稳定拒绝码；相邻支持项 ⇒ 有答。
#[test]
fn ty82_m3不判的判定请求_正向稳定码_反向支持项有答() {
    let d = tmp_root("ty82");

    // 正向：`assignable`（TYP-18）/ `implements`（TYP-13）/ `insance`（TYP-23..26）
    // 都是「M3 不判」的判定名 ⇒ 同一个稳定码、退出码 2（请求本身不合格）。
    for (kind, typ) in [
        ("assignable", "TYP-18（U-M3-02）"),
        ("implements", "TYP-13（U-M3-01）"),
        ("instance", "TYP-23..26（U-M3-04）"),
    ] {
        let f = write(
            &d,
            &format!("fwd_{kind}.go"),
            &format!("package p\n\n//rgoc:cmp {kind} int => string\n"),
        );
        let (code, out, err) = run(&["types", "--cmp", &f.to_string_lossy()]);
        assert_eq!(code, 2, "{kind}（{typ}）该被拒；stdout={out} stderr={err}");
        assert!(out.is_empty(), "拒绝时不该有 stdout：{out}");
        assert!(
            err.contains("[TYP-CMP-KIND-UNSUPPORTED]"),
            "{kind} 缺稳定码：{err}"
        );
        assert!(
            err.contains("identical") && err.contains("representable"),
            "拒绝理由要说出**支持**哪些 kind：{err}"
        );
        assert!(
            err.contains(&format!("{kind:?}")),
            "拒绝理由要点名**是哪个** kind：{err}"
        );
    }

    // 反向：两个支持项照常有答（这正是「相邻」的含义：kind 名差一点，行为天壤之别）。
    let rev = write(
        &d,
        "rev.go",
        "package p\n\n//rgoc:cmp identical int => int\n//rgoc:cmp representable 1 => int8\n",
    );
    let (code, out, err) = run(&["types", "--cmp", &rev.to_string_lossy()]);
    assert_eq!(code, 0, "支持项该零诊断；stderr={err}");
    assert!(err.is_empty(), "{err}");
    assert_eq!(
        line_with(&out, "3\tidentical"),
        "3\tidentical\ttrue",
        "{out}"
    );
    assert_eq!(
        line_with(&out, "4\trepresentable"),
        "4\trepresentable\ttrue",
        "{out}"
    );
    assert!(
        !out.contains("TYP-CMP-KIND-UNSUPPORTED"),
        "支持项的判定**不该**被拒：{out}"
    );
}

// ─────────────────────────── T-TY-83 ───────────────────────────

/// `T-TY-83`：需类型信息的 `const` 初始化式 ⇒ 稳定拒绝码；常量子集内 ⇒ 不报。
#[test]
fn ty83_需类型信息的const_正向稳定码_反向常量子集不报() {
    let d = tmp_root("ty83");

    // 正向：`len("abc")` 要类型信息（`D-M3-19`、`U-M3-08`）。
    let fwd = write(&d, "fwd.go", "package p\n\nconst x = len(\"abc\")\n");
    let (code, out, err) = run_types(&fwd);
    assert_eq!(code, 1, "有 error ⇒ 退出码 1；stdout={out} stderr={err}");
    assert_eq!(diag_count(&out), 1, "摘要该说 1 条：{out}");
    assert!(
        err.contains("[TYP-INVALID-CONST-INIT]"),
        "缺稳定拒绝码：{err}"
    );
    assert!(err.contains(":3:"), "位置该落在第 3 行：{err}");

    // 反向：常量子集内（含跨声明的 `x * 2` 与类型化常量）⇒ 零诊断。
    // `var z = x * 2` 也在里面：M3 **不**判表达式（`D-M3-2`）⇒ 它不该响。
    let rev = write(
        &d,
        "rev.go",
        "package p\n\nconst x = 1 + 2\nconst y int8 = 3\nvar z = x * 2\n",
    );
    let (code, out, err) = run_types(&rev);
    assert_eq!(code, 0, "常量子集该零诊断；stderr={err}");
    assert!(err.is_empty(), "{err}");
    assert_eq!(diag_count(&out), 0, "{out}");
}

// ─────────────────────────── T-TY-84 ───────────────────────────

/// `T-TY-84`：未知 `//go:` 指令**不**报错；`//go:build` 的版本**被读取**。
#[test]
fn ty84_未知go指令不报错_而版本门控确实读build行() {
    let d = tmp_root("ty84");

    // 正向：未知指令 ⇒ 前端不报错（`D-M3-6`：裁定归 noder 等价阶段，`U-M3-09`）。
    let fwd = write(
        &d,
        "fwd.go",
        "//go:totally-made-up\n//go:another-silly-one x y\npackage p\n\nvar x int\n",
    );
    let (code, out, err) = run_types(&fwd);
    assert_eq!(code, 0, "未知 //go: 指令不该响；stdout={out} stderr={err}");
    assert!(err.is_empty(), "{err}");
    assert_eq!(diag_count(&out), 0, "{out}");

    // 反向甲：`//go:build` 的版本**真的**被读了 —— 高版本 ⇒ 文件级 TooNew。
    // 这条就是「不报错」与「没读」的分界线：同样的位置，只是版本换成读不了的。
    let too_new = write(
        &d,
        "too_new.go",
        "//go:build go1.99\n\npackage p\n\nvar x int\n",
    );
    let (code, out, err) = run_types(&too_new);
    assert_eq!(code, 1, "高版本该报；stdout={out} stderr={err}");
    assert_eq!(diag_count(&out), 1, "{out}");
    assert!(
        err.contains("[TYP-FILE-REQUIRES-NEWER-GO]"),
        "版本门控的代号：{err}"
    );

    // 反向乙：可满足的版本（`go1.21`）+ `//line`（另一条**已知**指令）⇒ 零诊断。
    let ok = write(
        &d,
        "ok.go",
        "//go:build go1.21\n\npackage p\n\n//line fake.go:10\n\nvar x int\n",
    );
    let (code, out, err) = run_types(&ok);
    assert_eq!(code, 0, "已知指令 + 可满足版本该零诊断；stderr={err}");
    assert!(err.is_empty(), "{err}");
    assert_eq!(diag_count(&out), 0, "{out}");
}

// ─────────────────────────── T-TY-85 ───────────────────────────

/// `T-TY-85`：模块路径 / 二进制 export data ⇒ 稳定拒绝码；源码路径 import ⇒ 不报。
#[test]
fn ty85_import形式_正向稳定码_反向源码路径不报() {
    let d = tmp_root("ty85");
    let root = d.join("root");
    write(&root, "a/b/b.go", "package b\n\nvar B int\n");

    let code5 = "[TYP-UNSUPPORTED-IMPORT-FORM]";
    let src = "package p\n\nimport \"fmt\"\n\nvar _ = fmt.Println\n";
    let fwd = write(&d, "fwd.go", src);

    // 正向甲：没有 roots ⇒ `fmt` 找不到源码目录（标准库只有归档 ⇒ 二进制 export data）。
    let (code, out, err) = run_types(&fwd);
    assert_eq!(code, 1, "该报；stdout={out} stderr={err}");
    assert_eq!(diag_count(&out), 1, "{out}");
    assert!(err.contains(code5), "缺稳定拒绝码：{err}");

    // 正向乙：**给了 roots 也一样拒** —— 这条才是「模块路径不属于源码 import」的证明：
    // 拒的理由不是「找不到」，而是「`fmt` 这种形式我们要 `go.mod` / 下载（`U-M3-07`）」。
    let (code, _out, err) = run(&[
        "types",
        "-I",
        &root.to_string_lossy(),
        &fwd.to_string_lossy(),
    ]);
    assert_eq!(code, 1, "给了 roots 也该拒：{err}");
    assert!(err.contains(code5), "{err}");
    assert!(err.contains("fmt"), "拒绝理由要点名 import 路径：{err}");

    // 反向：源码路径 import + 正确的 root ⇒ 零诊断（T-TY-62 的支持面）。
    let rev = write(&d, "rev.go", "package p\n\nimport \"a/b\"\n\nvar _ b.B\n");
    let (code, out, err) = run(&[
        "types",
        "-I",
        &root.to_string_lossy(),
        &rev.to_string_lossy(),
    ]);
    assert_eq!(code, 0, "源码路径 import 该零诊断；stderr={err}");
    assert!(err.is_empty(), "{err}");
    assert_eq!(diag_count(&out), 0, "{out}");

    // 反向的**对照**：同一个文件、root 指错 ⇒ 同一条拒绝码。缺了这一半，
    // 上面那条「不报错」就可能只是「哪里漏了」而不是「真的解析到了」。
    let (code, _out, err) = run(&[
        "types",
        "-I",
        &d.join("nowhere").to_string_lossy(),
        &rev.to_string_lossy(),
    ]);
    assert_eq!(code, 1, "root 指错该拒：{err}");
    assert!(err.contains(code5), "{err}");
}

// ─────────────────────── 退出码语义（Task 36 Step 1）───────────────────────

/// 退出码语义：`exit == 0 ⟺ diagnostics == 0`；且 `reject` **不是** error。
///
/// 「仅警告 ⇒ 0」那一半见模块文档：M3 没有严重级别 ⇒ 空条款。
#[test]
fn ty8x_退出码语义_零诊断与零一一对应且reject不是error() {
    let d = tmp_root("exit");

    let clean = write(&d, "clean.go", "package p\n\nvar x int\n");
    let (code, out, err) = run_types(&clean);
    assert_eq!((code, diag_count(&out)), (0, 0), "无诊断 ⇒ 0；stderr={err}");

    let bad = write(&d, "bad.go", "package p\n\nconst x = len(\"abc\")\n");
    let (code, out, _err) = run_types(&bad);
    assert_eq!((code, diag_count(&out)), (1, 1), "有诊断 ⇒ 非 0");

    // 三条诊断 ⇒ 退出码仍是 1（不是「诊断条数」），摘要如实报 3。
    let three = write(
        &d,
        "three.go",
        "package p\n\nconst a = len(\"a\")\nconst b = len(\"bb\")\nconst c = len(\"ccc\")\n",
    );
    let (code, out, _err) = run_types(&three);
    assert_eq!((code, diag_count(&out)), (1, 3), "{out}");

    // `reject` 走 stdout 且退出码 0 —— 「不判」不是「错了」。
    let rej = write(
        &d,
        "reject.go",
        "package p\n\n//rgoc:cmp identical interface{ ~int } => interface{ ~string }\n",
    );
    let (code, out, err) = run(&["types", "--cmp", &rej.to_string_lossy()]);
    assert_eq!(code, 0, "reject 不该让退出码非 0；stderr={err}");
    assert!(out.contains("reject"), "{out}");
}
