//! S1 —— 解释 spike 的**进程边界**（bin）。
//!
//! 这里只做三件事，且每件都有其存在的理由：
//!
//! 1. **把 stderr / stdout 分开写**。S1 的验收是「stderr 精确 `3\n` 且 stdout 精确为空」，
//!    两条流必须由两次独立的写操作产生 —— 若混流（比如都走 `println!`），
//!    「stdout 精确为空」这条断言就永远不会被真正验证。
//! 2. **拒绝任何命令行参数**（E6「输入写死」的运行时兜底；源码层的守卫在
//!    `tests/no_cli_input.rs`）。收到参数 → 退出码 64（`EX_USAGE`）。
//! 3. **把 T-S1-03 的边界记录写到单独文件**。因为「stderr 精确 `3\n`」意味着
//!    bin 的可观测输出**只有那三行**，记录不能混进去；写 stdout 又会破坏「stdout 精确为空」
//!    —— 这是三个约束交汇处唯一不冲突的出口。
//!
//! 判定逻辑全部在 [`rgoc_spikes::interp`] 里，**此处不含任何语义判定** ——
//! 与 harness 侧「driver 与 xtask 都只调 `run_layer`」同一条纪律。

use std::io::Write;
use std::process::ExitCode;

use rgoc_spikes::interp::{check_t_s1_01, check_t_s1_02, check_t_s1_03, render_records};

/// 退出码：fixture 或求值失败（`EX_SOFTWARE`）。与「语义不匹配」可分。
const EXIT_SPIKE_FAILED: u8 = 70;
/// 退出码：收到了不应存在的命令行参数（`EX_USAGE`）。
const EXIT_USAGE: u8 = 64;

/// 附带产物的输出目录。落 `target/` 而非 CWD —— 记录是**构建产物**，
/// 写进源码树会污染 `git status`（第一版就踩过：四个 `.txt` 散在 `rgoc/` 下）。
const OUT_DIR: &str = "target";

fn main() -> ExitCode {
    if std::env::args().len() > 1 {
        eprintln!("s1_interp: 本 spike 的输入是写死的，不接受任何参数");
        return ExitCode::from(EXIT_USAGE);
    }

    let out = match check_t_s1_01() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("s1_interp: {e}");
            return ExitCode::from(EXIT_SPIKE_FAILED);
        }
    };
    if let Err(e) = check_t_s1_02() {
        eprintln!("s1_interp: {e}");
        return ExitCode::from(EXIT_SPIKE_FAILED);
    }
    let edges = match check_t_s1_03() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("s1_interp: {e}");
            return ExitCode::from(EXIT_SPIKE_FAILED);
        }
    };

    // 记录走独立文件（三个约束交汇处的唯一出口，见模块文档第 3 条）。
    // 落在 `target/` 而不是 CWD —— 它是**构建产物**，写进源码树会污染 `git status`。
    // 写失败不判 spike 失败（它是附带产物，不是验收项），但用 `let _ =` 明确表示「故意忽略」。
    let _ = std::fs::create_dir_all(OUT_DIR);
    let _ = std::fs::write(
        std::path::Path::new(OUT_DIR).join("s1-const-edges.txt"),
        render_records(&edges),
    );

    // 唯一的可观测输出：stderr 精确 "3\n"。stdout 一个字节都不写。
    let mut err = std::io::stderr();
    let _ = err.write_all(out.stderr.as_bytes());
    let _ = err.flush();

    ExitCode::SUCCESS
}
