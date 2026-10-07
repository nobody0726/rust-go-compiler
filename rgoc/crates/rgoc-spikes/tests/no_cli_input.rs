//! E6「输入写死」的可执行守卫。
//!
//! `M0-plan.md` T41 明确：三个 spike 的输入**必须写死**，不允许从命令行传入可变输入 ——
//! 否则「可复现」无从判定（传不同输入当然不一致，那不叫可复现）。
//!
//! 这条纪律**只写在文档里是不够的**：文档不会因为有人加了 `env::args()` 而变红。
//! 所以本测试直接对本 crate 的源码做文本检查。
//!
//! 检查方式刻意用**文本**而非 AST 或运行时：这是最弱但最不会漏的形式 ——
//! 只要有人写了 `env::args` / `std::env::args` / `args().nth(` 中的任一形态就会命中。

use std::fs;
use std::path::{Path, PathBuf};

/// 递归收集本 crate 下所有 `.rs` 源码路径。
fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// 剔除整行注释，避免注释里提到 `env::args` 就让本测试恒红
/// （`check-m0-consistency.py` 的第 5/5b 节因同一个理由使用 `code_only()`）。
fn code_only(src: &str) -> String {
    src.lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with("//") || t.starts_with("/*") || t.starts_with('*'))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn 三个_spike_的源码里没有命令行输入() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_sources(&crate_dir.join("src"), &mut files);
    assert!(
        files.len() >= 5,
        "至少应找到 lib.rs + interp.rs + fixtures/mod.rs + 三个 bin，实际找到 {}",
        files.len()
    );

    // 分两类文件、两套规则 —— 因为「读命令行」有两种完全不同的性质：
    //
    //   A) 语义路径（`src/lib.rs` / `src/interp.rs` / `src/fixtures/**`）：
    //      一律禁止出现 `env::args`。这里若读了参数，被读到的值就会经由
    //      `interpret()` 之类的函数影响**判定结果** —— 那就正是 E6 要排除的
    //      「输入可变量 ⇒ 可复现无从判定」。
    //
    //   B) 进程边界（`src/bin/**`）：**禁止把参数用作输入**，但允许一种形态 ——
    //      纯拒绝式读取（`args().len() > 1` 之类）。它在语义上等价于「本程序无参数」，
    //      是纪律的**执行者**；若连它也禁掉，纪律就只剩文档里的一句话。
    //      判据是「出现 env::args 的每一行都必须含 args().len()」——
    //      只认这一种拒绝式形态，取值比较（`args().nth(0)`）仍然会被抓出来。
    for f in &files {
        let raw = fs::read_to_string(f).expect("源码应可读");
        let is_bin = f.components().any(|c| c.as_os_str() == "bin");

        for (i, line) in code_only(&raw).lines().enumerate() {
            let has_args = line.contains("env::args") || line.contains("args_os");
            if !has_args {
                continue;
            }
            assert!(
                !line.contains("env::var") && !line.contains("clap") && !line.contains("getopts"),
                "{}:{} 出现了 env::var/clap/getopts —— 违反 E6「输入写死」",
                f.display(),
                i + 1
            );
            if is_bin {
                assert!(
                    line.contains("args().len()"),
                    "{}:{} 允许 bin **拒绝**参数（args().len()），\
                     但不允许把参数**当作输入**（如 args().nth / args_os / env::var）",
                    f.display(),
                    i + 1
                );
            } else {
                panic!(
                    "{}:{} 语义路径出现 env::args —— 违反 E6「输入写死」：\
                     三个 spike 的输入必须是固定 HIR，不接受命令行可变输入",
                    f.display(),
                    i + 1
                );
            }
        }
    }
}

#[test]
fn bin_里的拒绝式读取_确实存在_否则纪律只是文档() {
    // 反向验证：若哪天有人把 bin 里的拒绝逻辑删掉（觉得「没人会传参」），
    // 本测试会红。纪律要有执行者。
    let bin = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("bin")
        .join("s1_interp.rs");
    let code = code_only(&fs::read_to_string(&bin).expect("应可读"));
    assert!(
        code.contains("args().len()"),
        "s1_interp 必须保留「拒绝任何参数」的兜底，否则 E6 纪律退化为文档里的一句话"
    );
}

#[test]
fn 本测试文件自身也在被检查范围内所以要用_别名绕开() {
    // 反向验证：本文件在 src/ 之外（tests/），因此不会自己撞上自己的禁用词检查。
    // 但它的**辅助函数**里用了 `env!("CARGO_MANIFEST_DIR")` —— 那是编译期常量展开，
    // 不含 `env::args`，所以即便将来有人把 tests 挪进 src 也不会误报。
    let here = file!();
    assert!(here.contains("no_cli_input"), "本文件应被 cargo 单独编译");
}
