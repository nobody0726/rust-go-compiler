//! 堆增长与上限（M9/Task 7，`T-MEM-06/07`）。
//!
//! # 判据的两侧
//!
//! 1. **能长大**：默认上限下跑一个吃 ~19 MiB 的程序（远超 M7 的 8 MiB 静态
//!    arena）。这条同时是「M9 的分配器真的换掉了静态 arena」的证据 ——
//!    M7 在这里会打 `panic: out of arena`。
//! 2. **会报错**：`RGOC_HEAP_MAX=1048576` 时退出 **2** + 一行
//!    `runtime error: out of memory (heap limit 1048576 bytes)`。
//!
//! 只测第 1 条 ⇒ 「上限」是空话；只测第 2 条 ⇒ 「上限」是唯一功能。
//!
//! # 环境变量怎么读进 freestanding 程序
//!
//! 没有 libc 的 `getenv`。`_start` 入口的 `sp` 指向 `argc`，`argv` 紧随其后，
//! `envp` 在 `argv + argc + 1` —— 所以 `_start` 把 `sp` 交给 `rt_init`，
//! 由它扫一遍 `envp`。这条路径**没有别的办法**（`/proc/self/environ` 要
//! `open`/`read` 两个新系统调用，而它们对 M9 没用）。

use std::path::{Path, PathBuf};

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit};

/// 一个吃 ~40 MiB 的程序：10 轮 × 64 个 `new([8192]int)`（每个 64 KiB）。
///
/// # 两个刻意的选择
///
/// * 用固定数组 + 多轮，而不是 `[300]*[8192]int` 或 `append`：
///   `append` 要到 Phase 1 才支持，而本判据不该等它；而 300 个槽会让
///   `ps[i]` 的**索引偏移**超过 4095，撞上 isel 的立即数上限
///   （`add x9, x9, #65536` 编不出来）—— 那是另一个待办，不是本 Task 的事。
/// * 每轮重新 `new` 覆盖 `keep`：M9 没有 GC，旧对象留在 arena 里，
///   所以「吃掉」的总量是 640 × 64 KiB ≈ 40 MiB，而不是最后那 4 MiB。
const HOG: &str = r#"
package main

func main() {
	var keep [64]*[8192]int
	total := 0
	for round := 0; round < 10; round++ {
		for i := range keep {
			keep[i] = new([8192]int)
			keep[i][0] = i
			total++
		}
	}
	println(total, keep[63][0])
}
"#;

fn scratch() -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/mem-limit");
    std::fs::create_dir_all(&d).expect("建目录失败");
    d
}

/// 编出产物，返回它的路径。
fn build(name: &str, src: &str) -> PathBuf {
    let dir = scratch();
    let go = dir.join(format!("{name}.go"));
    std::fs::write(&go, src).expect("写 fixture 失败");
    let exe = dir.join(name);
    let cmd = BuildCommand {
        files: vec![go],
        output: Some(exe.clone()),
        emit: Emit::Elf,
        keep_asm: false,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let out = build_cmd::run_build(&cmd).expect("build 不该失败");
    assert_eq!(out.exit, 0, "构建失败：{}", out.stderr);
    exe
}

#[test]
fn 默认上限下能吃_40_mib() {
    let exe = build("t_mem_06", HOG);
    let run = std::process::Command::new(&exe)
        .output()
        .expect("跑不动产物");
    let out = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        run.status.code(),
        Some(0),
        "默认上限（256 MiB）下 40 MiB 必须跑得通。stdout={out:?} stderr={:?}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(out, "640 63\n");
}

#[test]
fn 超上限时退出_2_并给出上限数值() {
    let exe = build("t_mem_07", HOG);
    let run = std::process::Command::new(&exe)
        .env("RGOC_HEAP_MAX", "1048576")
        .output()
        .expect("跑不动产物");
    let err = String::from_utf8_lossy(&run.stderr);
    assert_eq!(run.status.code(), Some(2), "超限必须退出 2，stderr={err:?}");
    assert!(
        err.contains("runtime error: out of memory (heap limit 1048576 bytes)"),
        "超限文本必须带上限数值（`C9` §6 的形状），实际：{err:?}"
    );
}

#[test]
fn 上限可以被环境变量放大() {
    // 同一个程序、同一份产物：只把上限调小就失败、调大就成功 ——
    // 这条钉住「上限真的来自环境变量」而不是某个编译期常量。
    let exe = build("t_mem_07b", HOG);
    let small = std::process::Command::new(&exe)
        .env("RGOC_HEAP_MAX", "1048576")
        .output()
        .expect("跑不动产物");
    assert_eq!(small.status.code(), Some(2));
    let big = std::process::Command::new(&exe)
        .env("RGOC_HEAP_MAX", "134217728") // 128 MiB
        .output()
        .expect("跑不动产物");
    assert_eq!(big.status.code(), Some(0), "128 MiB 上限下该成功");
}
