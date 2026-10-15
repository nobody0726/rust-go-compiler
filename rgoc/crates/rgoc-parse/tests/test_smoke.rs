//! T-AP20 的验收测试：标准库冒烟 + 递归深度上限 + 一致性。
//!
//! 四条判据（M2-tests §4.3）：
//!
//! 1. **`T-A-94`**（O）：`go_source_code/src` 全部 `.go`（官方 `walkDirs` 口径：
//!    跳过任何名为 `testdata` 的目录与名为 `test` 的目录）解析**零意外错误**。
//! 2. **`T-A-96`**（**H**）：>1000 层嵌套报 `SYN-NESTING-TOO-DEEP` 且
//!    **不 panic / 不栈溢出**。官方没有这个行为 —— 这是本仓唯一的 `H` 判据
//!    （Rust 主线程栈是有限资源，官方编译器靠 Go 的可增长栈）。
//! 3. **`T-A-98`**（S）：同输入 3 次，AST dump + 诊断流逐字节相同。
//! 4. **`T-A-99`**（S）：解析结束后 `File.eof` 是一个**已设置**的 Pos
//!    （官方 `p.tok.pos` 的 EOF 位置：末行 +1、col 1）。

use std::path::{Path, PathBuf};

use rgoc_ast::dump;
use rgoc_lex::DiagCode;
use rgoc_parse::parse_file;

// ── 公共 ─────────────────────────────────────────────────────────────────────

/// 语料 `src/` 目录（与 `rgoc-driver::corpus_test_dir` 同形的推算 + 环境覆盖）。
fn corpus_src_dir() -> PathBuf {
    if let Ok(d) = std::env::var("RGOC_CORPUS_SRC_DIR") {
        return PathBuf::from(d);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../go_source_code/src")
        .to_path_buf()
}

/// 官方 `walkDirs`（`parser_test.go:126`）的目录跳过规则：`testdata` 与 `test`。
fn is_skipped_dir(name: &str) -> bool {
    name == "testdata" || name == "test"
}

/// 递归收集 `.go` 文件（官方 walkDirs 的目录序无所谓，用确定序即可）。
fn collect_go_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        if path.is_dir() {
            if !is_skipped_dir(&e.file_name().to_string_lossy()) {
                collect_go_files(&path, out);
            }
        } else if path.extension().is_some_and(|x| x == "go") {
            out.push(path);
        }
    }
}

/// 深度测试在**显式大栈线程**里跑：守卫在 1000 层触发，触发前的 1000 层
/// 递归本身要有足够的栈余量（libtest 线程的默认栈不承诺够用）。
fn with_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(f)
        .expect("起大栈线程失败")
        .join()
        .expect("大栈线程 panic")
}

const SHALLOW: &str = "package p\n\nfunc F() {\n\t_ = ((((1))))\n}\n";

// ── T-A-94：标准库全量冒烟 ────────────────────────────────────────────────────

#[test]
fn t_a_94_标准库全量零意外错误() {
    let src = corpus_src_dir();
    let mut files = Vec::new();
    collect_go_files(&src, &mut files);
    // 分母守卫：语料在（数量随 go 版本固定，go1.27.1 实测 6670）。
    assert!(
        files.len() > 6000,
        "语料分母异常：只找到 {} 个 .go（src = {}）",
        files.len(),
        src.display()
    );

    let mut failures: Vec<String> = Vec::new();
    for f in &files {
        let rel = f
            .strip_prefix(&src)
            .unwrap_or(f)
            .to_string_lossy()
            .into_owned();
        let Ok(bytes) = std::fs::read(f) else {
            failures.push(format!("{rel}: 读失败"));
            continue;
        };
        let res = parse_file(&bytes, &rel);
        if res.errors != 0 {
            let first = res
                .diags
                .into_sorted()
                .first()
                .map(|d| format!("{}:{} {}", d.pos.line, d.pos.col, d.message))
                .unwrap_or_else(|| "<无诊断但 errcnt>0>".into());
            failures.push(format!("{rel}: {first}"));
            if failures.len() >= 20 {
                break;
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} 个文件有意外解析错误（列前 20）：\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ── T-A-96：递归深度上限（H） ────────────────────────────────────────────────

/// 造 `n` 层括号嵌套的表达式。
fn deep_parens(n: usize) -> String {
    format!("package p\n\nvar x = {}1{}\n", "(".repeat(n), ")".repeat(n))
}

/// 造 `n` 层块嵌套。
fn deep_blocks(n: usize) -> String {
    format!("package p\n\nfunc f() {}{}\n", "{".repeat(n), "}".repeat(n))
}

/// 造 `n` 层切片类型嵌套。
fn deep_types(n: usize) -> String {
    format!("package p\n\nvar x {}int\n", "[]".repeat(n))
}

fn has_nesting_diag(src: &str) -> bool {
    let res = parse_file(src.as_bytes(), "deep.go");
    res.diags
        .into_sorted()
        .iter()
        .any(|d| d.code == DiagCode::SynNestingTooDeep)
}

/// 深度诊断的**条数**（「每文件只报一次」的判据：超限后每个祖先层都会看到
/// 超限，不去重就是上千条重复诊断淹没真错误）。
fn nesting_diag_count(src: &str) -> usize {
    parse_file(src.as_bytes(), "deep.go")
        .diags
        .into_sorted()
        .iter()
        .filter(|d| d.code == DiagCode::SynNestingTooDeep)
        .count()
}

#[test]
fn t_a_96_超千层嵌套报诊断不_panic() {
    with_big_stack(|| {
        for (label, src) in [
            ("括号", deep_parens(1200)),
            ("块", deep_blocks(1200)),
            ("类型", deep_types(1200)),
        ] {
            // 不 panic 即通过的第一半（栈溢出会直接杀死进程）。
            let src = src.clone();
            std::thread::Builder::new()
                .stack_size(256 << 20)
                .spawn(move || has_nesting_diag(&src))
                .expect("起线程失败")
                .join()
                .expect("嵌套解析 panic 了");
            // 第二半：确实报了 SYN-NESTING-TOO-DEEP。
            let src2 = match label {
                "括号" => deep_parens(1200),
                "块" => deep_blocks(1200),
                _ => deep_types(1200),
            };
            assert!(
                has_nesting_diag(&src2),
                "{label} 嵌套 1200 层应报 SYN-NESTING-TOO-DEEP"
            );
            // 每文件只报一次（超限后每个祖先层都看得到，不去重会淹没真错误）。
            assert_eq!(
                nesting_diag_count(&src2),
                1,
                "{label} 嵌套 1200 层应恰好报 1 条"
            );
        }
    });
}

#[test]
fn t_a_96_浅层嵌套不触发() {
    // 反向守卫：100 层（正常语言特性的合理范围）不得误报。
    for (label, src) in [
        ("括号", deep_parens(100)),
        ("块", deep_blocks(100)),
        ("类型", deep_types(100)),
    ] {
        let res = parse_file(src.as_bytes(), "shallow.go");
        assert_eq!(
            res.errors,
            0,
            "{label} 100 层不该报错：{:?}",
            res.diags.into_sorted()
        );
    }
}

// ── T-A-98：三次一致性 ───────────────────────────────────────────────────────

#[test]
fn t_a_98_同输入三次结果逐字节相同() {
    let src = "package p\n\nimport \"fmt\"\n\ntype T[X any] struct {\n\tA int `json:\"a\"`\n\t*p\n}\n\nfunc F(a, b int) (int, error) {\n\tif x := a + b; x > 0 {\n\t\treturn x, nil\n\t}\n\tfor i := range 3 {\n\t\tfmt.Println(i)\n\t}\n\treturn 0, nil\n}\n";

    // 合法输入：AST 逐字节同。
    let d1 = {
        let r = parse_file(src.as_bytes(), "det.go");
        dump(&r.file.expect("夹具应可解析"))
    };
    for _ in 0..2 {
        let r = parse_file(src.as_bytes(), "det.go");
        assert_eq!(dump(&r.file.expect("夹具应可解析")), d1, "AST dump 应确定");
    }

    // 错误输入：诊断流（pos/code/message 全键）也逐字节同。
    let bad = "package p\n\nfunc f() {\n\tbreak\n\tgoto L\n\tx := 1\nL:\n\tbreak\n}\n";
    let w1: Vec<_> = parse_file(bad.as_bytes(), "det2.go")
        .diags
        .into_sorted()
        .into_iter()
        .map(|d| (d.pos.line, d.pos.col, d.code, d.message))
        .collect();
    for _ in 0..2 {
        let w: Vec<_> = parse_file(bad.as_bytes(), "det2.go")
            .diags
            .into_sorted()
            .into_iter()
            .map(|d| (d.pos.line, d.pos.col, d.code, d.message))
            .collect();
        assert_eq!(w, w1, "诊断流应确定");
    }
}

// ── T-A-99：Eof 唯一 ─────────────────────────────────────────────────────────

#[test]
fn t_a_99_eof_是已设置的终止位置() {
    // 官方 `File.eof = p.tok.pos`（收尾时的 EOF 词元位置）：
    // 「package p\n」⇒ EOF 在第 2 行第 1 列。
    let r = parse_file(SHALLOW.as_bytes(), "eof.go");
    let f = r.file.expect("夹具应可解析");
    assert_eq!(
        f.eof.line, 6,
        "SHALLOW 共 5 行且以 \\n 结尾 ⇒ EOF 在第 6 行"
    );
    assert_eq!(f.eof.col, 1, "EOF 的 col 恒为 1");
}
