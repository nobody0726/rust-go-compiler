//! S3 —— native spike 的**进程边界**（bin）。实现 `M0-plan.md` T44。
//!
//! 流程：**固定 HIR → 生成 arm64 汇编 → `clang` 链接 → `file` 判定 → 运行**。
//!
//! | 测试 ID | 本文件里对应什么 |
//! |---|---|
//! | `T-S3-01` | `gen_asm` + `link_elf` 产出 ELF |
//! | `T-S3-02` | `check_t_s3_02`：`file` 判定含 `ELF 64-bit LSB` + `ARM aarch64` |
//! | `T-S3-03` | `check_t_s3_03`：stdout 精确 `hello\n`、stderr 空、退出码 0 |
//! | `T-S3-04` | 六项记录落盘（`native_records`） |
//! | `T-S3-05` | 重复构建 3 次结果一致（由 T45 编排，本文件只保证 codegen 是纯函数）|
//!
//! **拒绝任何命令行参数**（E6「输入写死」的兜底，与 S1/S2 同）。

use std::process::ExitCode;

use rgoc_spikes::fixtures::s3_hello;
use rgoc_spikes::native::{check_t_s3_02, check_t_s3_03, gen_asm, link_elf, write_asm};
use rgoc_spikes::native_records::render_records;

/// 产物目录。
///
/// **就是 `target/`**（不另设子目录），因为 `M0-plan.md` T44 的验收命令写死了
/// `/work/rgoc/target/s3-hello` —— 门禁命令必须与文档逐字一致，
/// 否则文档就变成「没人真正跑过的那份」。
const OUT_DIR: &str = "target";

const EXIT_SPIKE_FAILED: u8 = 70;
const EXIT_USAGE: u8 = 64;

fn main() -> ExitCode {
    if std::env::args().len() > 1 {
        eprintln!("s3_native: 本 spike 的输入是写死的，不接受任何参数");
        return ExitCode::from(EXIT_USAGE);
    }

    let dir = std::path::Path::new(OUT_DIR);

    // ── T-S3-01：生成 + 链接 ────────────────────────────────────────────────
    let asm = match gen_asm(&s3_hello()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("s3_native: codegen 失败：{e}");
            return ExitCode::from(EXIT_SPIKE_FAILED);
        }
    };
    let asm_path = match write_asm(&asm, dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("s3_native: 写汇编失败：{e}");
            return ExitCode::from(EXIT_SPIKE_FAILED);
        }
    };
    let elf = match link_elf(&asm_path, dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("s3_native: {e}");
            return ExitCode::from(EXIT_SPIKE_FAILED);
        }
    };

    // ── T-S3-02：file 判定 ─────────────────────────────────────────────────
    let file_out = match check_t_s3_02(&elf) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("s3_native: {e}");
            return ExitCode::from(EXIT_SPIKE_FAILED);
        }
    };

    // ── T-S3-03：运行 ──────────────────────────────────────────────────────
    if let Err(e) = check_t_s3_03(&elf) {
        eprintln!("s3_native: {e}");
        return ExitCode::from(EXIT_SPIKE_FAILED);
    }

    // ── 记录落盘（T-S3-04 + T45 要用的产物清单）────────────────────────────
    let _ = std::fs::write(dir.join("s3-hello.s"), &asm);
    let _ = std::fs::write(dir.join("s3-six-records.txt"), render_records());
    let _ = std::fs::write(
        dir.join("s3-summary.txt"),
        format!(
            "T-S3-01 汇编: {}\nT-S3-01 ELF: {}\nT-S3-02 file: {}\nT-S3-03: stdout 精确 hello\\n、stderr 空、退出码 0\n",
            asm_path.display(),
            elf.display(),
            file_out.trim()
        ),
    );

    // S3 的可观测输出：**stdout 精确 `hello\n`** —— 但注意这是**被测 ELF** 的输出，
    // 不是本 bin 的输出。本 bin 自己只往 stderr 写一行摘要（stdout 必须保持干净，
    // 否则「ELF 的 stdout 是 hello\n」这条会被本 bin 的输出污染）。
    eprintln!("s3_native: T-S3-01/02/03 通过；ELF = {}", elf.display());

    ExitCode::SUCCESS
}
