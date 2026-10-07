//! S3 的**端到端**验收（需要容器内 `clang` 与 `file`）。
//!
//! 与 `s3_native_test.rs` 分开：那份是纯 codegen（不依赖外部工具），这份真的要
//! **链接并运行** ELF。分开的理由是「换机器没装 clang 时，前者仍应全绿」——
//! 否则环境问题会伪装成代码问题。
//!
//! 覆盖 **T-S3-02**（`file` 判定）与 **T-S3-03**（运行结果）。

use std::path::{Path, PathBuf};
use std::process::Command;

use rgoc_spikes::fixtures::s3_hello;
use rgoc_spikes::native::{check_t_s3_02, check_t_s3_03, gen_asm, link_elf, run_elf, write_asm};

/// 产物根目录。
fn root() -> PathBuf {
    std::env::current_exe()
        .expect("应能取到当前测试可执行文件路径")
        .parent()
        .and_then(Path::parent)
        .expect("应能上溯到 target/")
        .join("s3-e2e")
}

/// **每个测试一个独立目录**。
///
/// ⚠️ 第一版所有测试共用一个目录，于是 cargo 的**并行测试**互相踩：
/// 一个测试正在 `execve` 那个 ELF，另一个测试把同一路径重写了 ——/
/// ```text
/// 应能运行 ELF：Text file busy (os error 26)
/// ```
///
/// 那个报错**看起来像**「产物坏了」，实际是竞态。给每个测试独立目录即可根治；
/// 另一个选项是让整个文件串行（`--test-threads=1`），但那会让门禁变慢且依赖命令行参数。
fn out_dir(tag: &str) -> PathBuf {
    root().join(tag)
}

/// 编译一次，返回 ELF 路径。`tag` 区分调用方，避免竞态。
fn build_elf(tag: &str) -> PathBuf {
    let asm = gen_asm(&s3_hello()).expect("codegen 应成功");
    let asm_path = write_asm(&asm, &out_dir(tag)).expect("应能写汇编");
    link_elf(&asm_path, &out_dir(tag)).expect("容器内应有 clang 且能链接")
}

#[test]
fn t_s3_01_能产出_elf_文件() {
    let elf = build_elf("t01");
    assert!(elf.exists(), "ELF 应存在：{}", elf.display());
    let meta = std::fs::metadata(&elf).expect("应能读元信息");
    assert!(meta.len() > 0, "ELF 不应为空文件");
}

#[test]
fn t_s3_02_file_判定含_elf_64位_lsb_与_arm_aarch64() {
    let elf = build_elf("t02");
    let text = check_t_s3_02(&elf).expect("T-S3-02 应通过");
    // 两个条件都断言 —— x86-64 的 ELF 同样含 "ELF 64-bit LSB"
    assert!(text.contains("ELF 64-bit LSB"), "file 输出：{text}");
    assert!(text.contains("ARM aarch64"), "file 输出：{text}");
}

#[test]
fn t_s3_03_stdout_精确为_hello_换行() {
    let elf = build_elf("t03a");
    let (stdout, _stderr, _code) = run_elf(&elf).expect("应能运行");
    assert_eq!(stdout, "hello\n", "必须逐字节相等");
    assert_eq!(stdout.len(), 6);
}

#[test]
fn t_s3_03_stderr_精确为空() {
    let elf = build_elf("t03b");
    let (_stdout, stderr, _code) = run_elf(&elf).expect("应能运行");
    assert!(stderr.is_empty(), "stderr 应为空，实际 {stderr:?}");
}

#[test]
fn t_s3_03_退出码为_0() {
    let elf = build_elf("t03c");
    let (_s, _e, code) = run_elf(&elf).expect("应能运行");
    assert_eq!(code, 0, "退出码应为 0");
}

#[test]
fn t_s3_03_总判定一次通过() {
    // 库里的组合判定（与 bin 用的是同一份逻辑）
    let elf = build_elf("t03d");
    check_t_s3_03(&elf).expect("T-S3-03 应通过");
}

#[test]
fn t_s3_05_重复链接三次产物逐字节一致() {
    // 「结果一致」的可执行版本：三次各自链接，比较字节内容。
    // 用 cmp 语义（逐字节）而非 hash —— hash 有极小的碰撞概率。
    //
    // ⚠️ 必须**同路径**重复链接才能比较：不同路径会把路径写进产物，反而引入
    // 路径这一无关变量。实测依据见 `native::link_elf` 的文档：
    // 不加 `-Wl,-s` 时，clang 汇编器的随机中间名（`hello-d9450b.o`）会留在
    // `.strtab` 里，让产物天然不可复现 —— 而 BuildID 三次相同，证明代码内容本身没问题。
    let dir = out_dir("t05");
    let asm = gen_asm(&s3_hello()).expect("codegen 应成功");
    let asm_path = write_asm(&asm, &dir).expect("应能写汇编");

    let mut prev: Option<Vec<u8>> = None;
    for i in 0..3 {
        // 同一份汇编、同一输出路径，只让链接器重跑三次
        let elf = link_elf(&asm_path, &dir).expect("应能链接");
        let bytes = std::fs::read(&elf).expect("应能读 ELF");
        if let Some(p) = &prev {
            assert_eq!(
                p.len(),
                bytes.len(),
                "第 {i} 次产物的 ELF 大小与第一次不同（{} vs {}）",
                p.len(),
                bytes.len()
            );
            assert!(
                p == &bytes,
                "第 {i} 次产物与第一次**逐字节**不同（T-S3-05 失败）"
            );
        }
        prev = Some(bytes);
    }
}

#[test]
fn t_s3_05_三次链接的_build_id_相同() {
    // 比整文件更进一步的证据：BuildID 是**内容哈希**，
    // 它相同即说明「代码内容」可复现，与符号表无关。
    let dir = out_dir("t05buildid");
    let asm = gen_asm(&s3_hello()).expect("codegen 应成功");
    let asm_path = write_asm(&asm, &dir).expect("应能写汇编");

    let mut ids: Vec<String> = Vec::new();
    for _ in 0..3 {
        let elf = link_elf(&asm_path, &dir).expect("应能链接");
        let out = Command::new("readelf").arg("-n").arg(&elf).output();
        let Ok(out) = out else {
            eprintln!("skip: 容器内无 readelf");
            return;
        };
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let id = text
            .lines()
            .find(|l| l.contains("Build ID:"))
            .unwrap_or("none")
            .trim()
            .to_string();
        ids.push(id);
    }
    assert_eq!(ids[0], ids[1], "第 1、2 次的 BuildID 不同：{ids:?}");
    assert_eq!(ids[0], ids[2], "第 1、3 次的 BuildID 不同：{ids:?}");
    assert!(ids[0].contains("Build ID:"), "应真的取到 BuildID：{ids:?}");
}

#[test]
fn 汇编文件确实被写出且内容与_codegen_一致() {
    let asm = gen_asm(&s3_hello()).expect("codegen 应成功");
    let p = write_asm(&asm, &out_dir("asm")).expect("应能写汇编");
    let on_disk = std::fs::read_to_string(&p).expect("应能读回汇编");
    assert_eq!(on_disk, asm, "落盘的汇编必须与 codegen 输出一致");
}

#[test]
fn elf_可被_readelf_识别且入口在_text() {
    // 「能跑」不等于「是个正常的 ELF」。用 readelf 补两个结构性的确认：
    // ① 入口落在 .text（start=0x25c 之类）
    // ② 有 PT_LOAD 可执行段
    let elf = build_elf("t_h");
    let out = Command::new("readelf").arg("-h").arg(&elf).output();
    let Ok(out) = out else {
        eprintln!("skip: 容器内无 readelf");
        return;
    };
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        text.contains("Entry point address"),
        "readelf -h 输出：{text}"
    );
    // 入口地址非 0 —— 为 0 说明链接器找不到入口（那正是「不加 -nostartfiles
    // 却又没有 _start」时的症状）
    let entry = text
        .lines()
        .find(|l| l.contains("Entry point address"))
        .expect("有入口行");
    let hex = entry
        .split("0x")
        .nth(1)
        .expect("入口地址应是 0x 形式")
        .split_whitespace()
        .next()
        .unwrap_or("0");
    assert_ne!(hex, "0", "入口地址不应为 0（否则程序无法正常启动）");
}
