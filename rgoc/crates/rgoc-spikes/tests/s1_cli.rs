//! S1 bin 的**进程边界**测试：真跑子进程，捕获两条流与退出码。
//!
//! 为什么必须用子进程：`rgoc_spikes::interp` 返回的是一个 `RunOutput { stderr, stdout }`，
//! 它**证明不了** bin 真的把两条流写到了正确的 fd 上。一个「两条流都写 stderr」
//! 或「都写 stdout」的实现，`RunOutput.stdout == ""` 照样成立。
//! 库测试抓不到这类错，只有真进程 + 真 fd 能抓。

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin_path(name: &str) -> PathBuf {
    // 集成测试的可执行文件在 target/<profile>/deps/ 下，bin 在其父目录。
    let mut p = std::env::current_exe().expect("应能取到当前测试可执行文件路径");
    p.pop(); // deps/
    if p.ends_with("deps") {
        p.pop();
    }
    p.join(name)
}

/// workspace 根（`rgoc/`）。
///
/// 测试可执行文件在 `rgoc/target/<profile>/deps/`，故上溯三层即得 `rgoc/`。
fn workspace_root() -> PathBuf {
    std::env::current_exe()
        .expect("应能取到当前测试可执行文件路径")
        .parent() // deps/
        .and_then(Path::parent) // <profile>/
        .and_then(Path::parent) // target/
        .and_then(Path::parent) // rgoc/
        .expect("应能上溯到 workspace 根（rgoc/）")
        .to_path_buf()
}

struct Captured {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run(name: &str, args: &[&str]) -> Captured {
    // ⚠️ **必须显式设 `current_dir`**：`cargo test` 把 CWD 设成 **package 根**
    // （`rgoc/crates/rgoc-spikes/`），而子进程默认继承它。三个 bin 都按
    // **相对路径** `target/` 写附带产物，于是产物落进了
    // `rgoc/crates/rgoc-spikes/target/` —— **污染源码树**（第一版就这么留下的，
    // `git status` 里冒出 4 个 `.txt` 与一整个 `target/`）。
    // 设成 workspace 根后，产物稳定落在 `rgoc/target/`，且被 .gitignore 覆盖。
    let out = Command::new(bin_path(name))
        .args(args)
        .current_dir(workspace_root())
        .output()
        .unwrap_or_else(|e| panic!("应能启动 {name}：{e}"));
    Captured {
        code: out.status.code().expect("应是正常退出而非被信号杀死"),
        stdout: out.stdout,
        stderr: out.stderr,
    }
}

#[test]
fn s1_bin_退出码为_0() {
    let r = run("s1_interp", &[]);
    assert_eq!(
        r.code,
        0,
        "S1 成功时退出码必须是 0（stdout={:?}）",
        String::from_utf8_lossy(&r.stdout)
    );
}

#[test]
fn s1_bin_stderr_精确为_3_换行() {
    let r = run("s1_interp", &[]);
    assert_eq!(
        r.stderr,
        b"3\n".to_vec(),
        "必须是逐字节的 \"3\\n\"，实际 {:?}",
        String::from_utf8_lossy(&r.stderr)
    );
}

#[test]
fn s1_bin_stdout_精确为空() {
    // 这条是「println 走 stderr」的唯一端到端证据。库里的 stdout 字段为空
    // 说明不了问题 —— 只有真进程的 fd 1 能说明。
    let r = run("s1_interp", &[]);
    assert!(
        r.stdout.is_empty(),
        "bin 的 stdout 必须一个字节都没有，实际 {:?}",
        String::from_utf8_lossy(&r.stdout)
    );
}

#[test]
fn s1_bin_拒绝任何参数_退出码为_64() {
    // E6「输入写死」的运行时兜底。给了参数就必须拒绝，不能默默忽略 ——
    // 默默忽略会让「我传的参数怎么没效果」变成一个查不出的问题。
    for args in [vec!["1"], vec!["--verbose"], vec!["x", "y"]] {
        let r = run("s1_interp", &args);
        assert_eq!(
            r.code, 64,
            "传入 {args:?} 应以 EX_USAGE(64) 拒绝，实际 {}",
            r.code
        );
        assert!(
            !r.stderr.is_empty(),
            "拒绝时必须有 stderr 说明，不能静默退出"
        );
        assert!(r.stdout.is_empty(), "拒绝路径也不该写 stdout");
    }
}

#[test]
fn 三个_spike_的_bin_都存在且可启动() {
    // 这张表是「哪个 spike 已实现」的**唯一登记处**。
    //
    // ⚠️ 它必须随实现进度更新，否则会出现两种难查的失败：
    //   ① spike 已实现但表里还写 false → 「退出码应非 0」失败（本次就踩了：T43 完成后
    //      s2_ssa 已能跑通，但表里仍是 RED）；
    //   ② spike 尚未实现但表里写了 true → 「退出码应为 0」失败，且会让人误以为是实现坏了。
    // 改这张表时，对应任务的验收命令也该跑一遍（T-S1-01 → T-S2-01 → T-S3-03）。
    const TABLE: &[(&str, bool, &str)] = &[
        ("s1_interp", true, "T-S1-01"),
        ("s2_ssa", true, "T-S2-01"),
        ("s3_native", true, "T-S3-01/02/03"),
    ];
    for (name, implemented, id) in TABLE {
        let p = bin_path(name);
        assert!(p.exists(), "{name} 应存在于 {}", p.display());
        let r = run(name, &[]);
        if *implemented {
            assert_eq!(r.code, 0, "{name}（{id}）已实现，退出码应为 0");
        } else {
            assert_ne!(r.code, 0, "{name}（{id}）仍是 RED 占位，退出码应非 0");
        }
    }
}

#[test]
fn s2_bin_的输出与_s1_逐字节相同() {
    // 端到端的交叉验证：两个**独立进程**的 stderr 必须逐字节相同。
    // 库内的比较（s2_ssa_test.rs）证明的是「同一份输入两条路径结果一致」；
    // 这一条证明的是「两个 bin 的进程边界都没把流写错」。
    let s1 = run("s1_interp", &[]);
    let s2 = run("s2_ssa", &[]);
    assert_eq!(s1.code, 0, "S1 应成功退出");
    assert_eq!(s2.code, 0, "S2 应成功退出");
    assert_eq!(
        s2.stderr, s1.stderr,
        "T-S2-01：S2 与 S1 的 stderr 必须逐字节相同"
    );
    assert!(s2.stdout.is_empty(), "S2 的 stdout 也必须精确为空");
}

#[test]
fn s2_bin_拒绝任何参数_退出码为_64() {
    let r = run("s2_ssa", &["--x"]);
    assert_eq!(r.code, 64, "S2 同样必须拒绝参数");
    assert!(!r.stderr.is_empty());
    assert!(r.stdout.is_empty());
}
