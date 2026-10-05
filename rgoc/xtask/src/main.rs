//! `xtask` 的二进制入口。逻辑全在 lib 里。

use std::process::ExitCode;

fn main() -> ExitCode {
    xtask::main_entry()
}
