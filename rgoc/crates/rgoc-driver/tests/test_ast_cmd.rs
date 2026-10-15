//! T-AP19 的验收测试：`rgoc ast` 子命令（CLI 契约 + dump 输出 + 错误路径）。
//!
//! 三组断言，对齐 M2-plan T-AP19 的 Step 1：
//!
//! 1. **CLI 契约** —— `ast` 是顶层子命令，手写解析（对齐 `lex` 模式，不引 clap）；
//!    `-- ast --help` 的报错里列出用法（USAGE 含 ast 行）。
//! 2. **dump 输出** —— `run_ast` 的输出 = `# golden:`/`# source: rgoc` 两行头 +
//!    中立 dump 正文，含 `File` / `PkgName` 行（来源标注纪律 §5.2/§5.3）。
//! 3. **错误路径** —— 语法错误文件 ⇒ `Err`，错误文本含诊断（不静默、不 panic）。
//!
//! TDD 位置：**RED**。实现前 `Command::Ast` / `ast_cmd` 不存在，本文件编译不过。

use std::path::{Path, PathBuf};

use rgoc_driver::ast_cmd;
use rgoc_driver::cli::{CliError, Command, parse_args};

// ── 夹具 ─────────────────────────────────────────────────────────────────────

/// 往临时目录写一个唯一的夹具文件（进程号隔离，测试后删除）。
fn fixture(name: &str, src: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("rgoc-tap19-{}-{name}", std::process::id()));
    std::fs::write(&p, src).expect("写夹具失败");
    p
}

fn cleanup(p: &Path) {
    let _ = std::fs::remove_file(p);
}

const OK_SRC: &str = "package p\n\nimport \"fmt\"\n\nfunc F() {\n\tfmt.Println(1)\n}\n";
const BAD_SRC: &str = "package p\n\nfunc f() {\n\tbreak\n}\n";

// ── 1. CLI 契约 ──────────────────────────────────────────────────────────────

#[test]
fn 解析_ast_子命令() {
    let got = parse_args(&["ast", "/tmp/x.go"]).expect("应可解析");
    assert_eq!(
        got,
        Command::Ast(rgoc_driver::cli::AstCommand {
            file: PathBuf::from("/tmp/x.go")
        })
    );
}

#[test]
fn ast_缺文件参数报错() {
    let e = parse_args(&["ast"]).unwrap_err();
    assert!(matches!(e, CliError::AstNeedsFile), "实际 {e:?}");
}

#[test]
fn ast_多余参数报错() {
    let e = parse_args(&["ast", "a.go", "b.go"]).unwrap_err();
    assert!(matches!(e, CliError::ExtraArg(_)), "实际 {e:?}");
}

#[test]
fn ast_未知选项报错且用法含_ast_行() {
    let e = parse_args(&["ast", "a.go", "--bogus"]).unwrap_err();
    let msg = e.to_string();
    assert!(msg.contains("--bogus"), "错误要指明是哪个选项：{msg}");
    // 「-- ast --help 列出用法」的验收口径：用法说明里必须出现 ast 子命令行。
    assert!(
        msg.contains("rgoc-driver ast"),
        "用法说明里要列出 ast 子命令：{msg}"
    );
}

// ── 2. dump 输出 ─────────────────────────────────────────────────────────────

#[test]
fn run_ast_输出头两行与_dump_正文() {
    let f = fixture("ok.go", OK_SRC);
    let out = ast_cmd::run_ast(&f).expect("合法文件应 Ok");
    cleanup(&f);

    let mut lines = out.lines();
    assert_eq!(
        lines.next().unwrap(),
        format!("# golden: {}", f.display()),
        "第一行回显传入路径"
    );
    assert_eq!(lines.next().unwrap(), "# source: rgoc", "来源必须如实标注");
    let body = lines.collect::<Vec<_>>().join("\n");
    assert!(body.contains("File"), "正文要有 File 行：\n{body}");
    assert!(body.contains("PkgName"), "正文要有 PkgName 行：\n{body}");
    // 中立 dump 的基本形状：FuncDecl 也在
    assert!(body.contains("FuncDecl"), "正文要有 FuncDecl 行：\n{body}");
}

#[test]
fn run_ast_正文与_rgoc_ast_dump_一致() {
    // 命令层不许「再渲染一遍」：正文必须逐字节等于 `rgoc_ast::dump` 的输出。
    let f = fixture("ok2.go", OK_SRC);
    let out = ast_cmd::run_ast(&f).expect("合法文件应 Ok");
    cleanup(&f);

    // 头两行按字节剥掉（各带 `\n`），正文与 `dump()` 全文逐字节比
    // （`lines().join` 会丢末尾换行，那种比法恒差一行 —— 别用）。
    let header_len = format!("# golden: {}\n# source: rgoc\n", f.display()).len();
    let body = &out[header_len..];
    let res = rgoc_parse::parse_file(OK_SRC.as_bytes(), &f.display().to_string());
    let want = rgoc_ast::dump(&res.file.expect("夹具应可解析"));
    assert_eq!(body, want, "命令输出正文 == dump 直接输出");
}

// ── 3. 错误路径 ──────────────────────────────────────────────────────────────

#[test]
fn run_ast_语法错误返回_err_且含诊断() {
    let f = fixture("bad.go", BAD_SRC);
    let err = ast_cmd::run_ast(&f).expect_err("语法错误应 Err");
    cleanup(&f);
    assert!(err.contains("SYN-"), "错误文本要带诊断代号：{err}");
}

#[test]
fn run_ast_文件不存在返回_err() {
    let p = std::env::temp_dir().join(format!("rgoc-tap19-{}-不存在.go", std::process::id()));
    let err = ast_cmd::run_ast(&p).expect_err("io 失败应 Err");
    assert!(err.contains("读"), "io 失败的错误要说明在读哪个文件：{err}");
}
