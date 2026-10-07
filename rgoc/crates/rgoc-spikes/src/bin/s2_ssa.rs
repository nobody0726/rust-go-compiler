//! S2 —— SSA spike 的**进程边界**（bin）。实现 `M0-plan.md` T43。
//!
//! 三件事，各自对应一条测试 ID：
//!
//! 1. **T-S2-01**：把 `1 + 2` lowering 成 Block/Value 图再求值，输出必须与 S1 **逐字节相同**；
//! 2. **T-S2-02**：把 memory / tuple / 调用边界（另附 phi、verifier）的需求清单落盘；
//! 3. **T-S2-03**：把与 M6 完整 verifier 的差距落盘。
//!
//! 与 S1 同样的三个约束交汇处：**stderr 精确 `3\n`**、**stdout 精确为空**、记录要落盘。
//! 所以两份记录走**独立文件**，可观测输出只有 stderr 那三行。
//!
//! **拒绝任何命令行参数**（E6「输入写死」的兜底，与 S1 同）。

use std::io::Write;
use std::process::ExitCode;

use rgoc_hir::Stream;
use rgoc_spikes::fixtures::sum_expr;
use rgoc_spikes::interp::check_t_s1_01;
use rgoc_spikes::ssa::{eval, lower_expr_program};
use rgoc_spikes::ssa_needs::{render_gap, render_needs};

/// 退出码：spike 失败（`EX_SOFTWARE`）。
const EXIT_SPIKE_FAILED: u8 = 70;
/// 退出码：收到了不应存在的命令行参数（`EX_USAGE`）。
const EXIT_USAGE: u8 = 64;

/// 附带产物的输出目录。落 `target/` 而非 CWD —— 记录是**构建产物**，
/// 写进源码树会污染 `git status`（第一版就踩过：三个 `.txt` 散在 `rgoc/` 下）。
const OUT_DIR: &str = "target";

/// **T-S2-01**：图求值结果与 S1 逐字节相同。
pub fn check_t_s2_01() -> Result<String, String> {
    // S2 的路径：从**表达式** lowering（不是从 HIR 里已求值的 Val），
    // 这样图里真的有一条 `bin` 指令，S2 才验证了「图求值」而不是「搬运求值结果」。
    let f = lower_expr_program("main", &[sum_expr()], true, Stream::Stderr)
        .map_err(|e| e.to_string())?;

    // 图里必须有 bin 指令 —— 否则这条路径与 S1 等价，交叉验证失去意义。
    // （`From<LowerError> for String` 已实现，`?` 自动转换，不需要 map_err）
    let has_bin = f
        .entry_block()?
        .instrs
        .iter()
        .any(|(_, i)| matches!(i, rgoc_spikes::ssa::Instr::Bin { .. }));
    if !has_bin {
        return Err(
            "T-S2-01 失败：图里没有 bin 指令，S2 退化成「搬运 HIR 的求值结果」".to_string(),
        );
    }

    let s2 = eval(&f)?;
    let s1 = check_t_s1_01()?;

    if s2.stderr != s1.stderr {
        return Err(format!(
            "T-S2-01 失败：S2 与 S1 输出不同\n  S1 {:?}\n  S2 {:?}",
            s1.stderr, s2.stderr
        ));
    }
    if s2.stdout != s1.stdout {
        return Err(format!(
            "T-S2-01 失败：stdout 不同\n  S1 {:?}\n  S2 {:?}",
            s1.stdout, s2.stdout
        ));
    }
    Ok(s2.stderr)
}

fn main() -> ExitCode {
    if std::env::args().len() > 1 {
        eprintln!("s2_ssa: 本 spike 的输入是写死的，不接受任何参数");
        return ExitCode::from(EXIT_USAGE);
    }

    let stderr_bytes = match check_t_s2_01() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s2_ssa: {e}");
            return ExitCode::from(EXIT_SPIKE_FAILED);
        }
    };

    // T-S2-02 / T-S2-03 的记录落盘（stdout 须为空、stderr 须精确 3\n，故走独立文件）。
    // 落 `target/` 而非 CWD —— 记录是**构建产物**，写进源码树会污染 `git status`。
    let out = std::path::Path::new(OUT_DIR);
    let _ = std::fs::create_dir_all(out);
    let _ = std::fs::write(out.join("s2-ssa-needs.txt"), render_needs());
    let _ = std::fs::write(out.join("s2-verifier-gap.txt"), render_gap());

    // 顺便把 SSA 图本身也落盘，供人工复核结构（T45 汇总进 benchmarks §12）
    if let Ok(f) = lower_expr_program("main", &[sum_expr()], true, Stream::Stderr) {
        let _ = std::fs::write(out.join("s2-ssa-graph.txt"), f.render());
    }

    let mut err = std::io::stderr();
    let _ = err.write_all(stderr_bytes.as_bytes());
    let _ = err.flush();

    ExitCode::SUCCESS
}
