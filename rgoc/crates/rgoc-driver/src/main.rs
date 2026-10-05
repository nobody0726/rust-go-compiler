//! `rgoc-driver` 的二进制入口。逻辑全在 lib 里，本文件只做「调库 → 转退出码」。

use std::process::ExitCode;

fn main() -> ExitCode {
    rgoc_driver::main_entry()
}
