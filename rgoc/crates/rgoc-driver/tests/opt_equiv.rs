//! passes 的**语义保持**（`T-SSA-64` 的语料腿；`SSA-5`）。
//!
//! # 判据
//!
//! 同一个程序，`--opt=all` 与 `--opt=none` 的输出必须**逐字节一致**、退出码一致。
//!
//! 这是 pass 唯一真正的判据。单测里的「合掉了 / 删掉了」只证明变换**发生了**，
//! 不证明它**没改语义** —— 而优化 pass 的错误几乎都长这样：图仍然合法
//! （verifier 全绿）、程序照跑、结果错一个数。
//!
//! # 为什么 `M` 侧直接跳过
//!
//! 23 个文件在建图期就被拒（pass 根本没跑）。两边都退出 1 + 同一条码，
//! 比较没有信息量 —— 但仍要**数出来**，否则「46 个全绿」会在分母上骗人。

use std::path::{Path, PathBuf};

use rgoc_driver::cli::{RunCommand, Via};
use rgoc_driver::run_cmd::run_run;
use rgoc_ssa::opt::Config;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达")
}

fn selected() -> Vec<String> {
    let path = repo_root().join("tools/run-oracle/run-selected.txt");
    let text = std::fs::read_to_string(&path).expect("读不到 run-selected.txt");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(ToString::to_string)
        .collect()
}

fn run_opt(src: &Path, opt: Config) -> (String, u8, String) {
    let o = run_run(&RunCommand {
        files: vec![src.to_path_buf()],
        dump_hir: false,
        max_steps: None,
        lang: None,
        via: Via::Ssa,
        opt,
    })
    .expect("内部错（不该发生）");
    (o.stdout.replace("\r\n", "\n"), o.exit, o.stderr)
}

#[test]
fn t_ssa_64_全开与全关在入选集上输出一致() {
    let root = repo_root();
    let rows = selected();
    let mut bad: Vec<String> = Vec::new();
    let mut compared = 0usize;
    let mut skipped = 0usize;

    for rel in &rows {
        let src = root.join(rel);
        let (on_out, on_exit, on_err) = run_opt(&src, Config::all());
        let (off_out, off_exit, off_err) = run_opt(&src, Config::none());

        // 建图期拒绝（pass 没跑）：两边都退出 1 + 同一条码
        if on_exit == 1 && on_err.contains("SSA-UNSUPPORTED-") {
            skipped += 1;
            assert_eq!(
                off_exit, 1,
                "{rel}: `--opt=none` 与 `--opt=all` 的拒绝面不一致"
            );
            continue;
        }
        compared += 1;
        if on_exit != off_exit {
            bad.push(format!(
                "{rel}: 退出码不同 all={on_exit} none={off_exit}\n  all stderr={on_err:?}\n  none stderr={off_err:?}"
            ));
            continue;
        }
        if on_out != off_out {
            bad.push(format!(
                "{rel}: 输出不同\n  all ={on_out:?}\n  none={off_out:?}"
            ));
        }
    }

    assert!(
        bad.is_empty(),
        "有 {} 条 pass 改了语义：\n{}",
        bad.len(),
        bad.join("\n\n")
    );
    assert_eq!(compared + skipped, rows.len(), "不许静默跳过");
    assert_eq!(compared, 46, "可建图的入选文件应恰为 46 个");
    assert_eq!(skipped, 23, "被拒的入选文件应恰为 23 个");
}
