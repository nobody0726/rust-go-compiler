//! M7 白名单 4 条的正向证据（`U-M7-01..04`，`T-NAT-52`）。
//!
//! # 「支持面」与「拒绝面」必须都能自证
//!
//! 只证明「39 个能跑」是不够的 —— 那可能只是「恰好这 39 个不碰不支持的面」。
//! 每一条白名单都要有**一个 fixture + 一个码 + 一个退出码**，否则「不支持」
//! 就成了一句没有判据的话。
//!
//! | 条目 | 能不能在这里正面测 | 为什么 |
//! |---|---|---|
//! | `U-M7-01` `[]T` | ✅ | 后端主动拒，码就在 `CgCode` 里 |
//! | `U-M7-02` 间接调用 | ⚠️ **守卫** | M6 已拒函数值（`SSA-UNSUPPORTED-FUNC-VALUE`），后端**够不着** |
//! | `U-M7-03` 方法值 | ⚠️ **守卫** | 同上（`SSA-UNSUPPORTED-IFACE-DYN`） |
//! | `U-M7-04` arena 耗尽 | ✅ | 运行期，跑出来就是 |
//!
//! 守卫测试的形态是**反向**的：断言「这个码**不出现**，拒绝来自 M6」。M6 一旦
//! 放行函数值，这条守卫就红 —— 那时后端的 `CG-UNSUPPORTED-INDIRECT-CALL` 才有
//! 意义，而它现在只是一条**到不了**的分支。

use std::path::{Path, PathBuf};

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达")
}

/// 把一段源码写到 `target/native-unsupported/<name>.go`，返回路径。
fn write_src(name: &str, src: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/native-unsupported");
    std::fs::create_dir_all(&dir).expect("建目录失败");
    let p = dir.join(format!("{name}.go"));
    std::fs::write(&p, src).expect("写 fixture 失败");
    p
}

fn build(name: &str, src: &str) -> (u8, String, PathBuf) {
    let go = write_src(name, src);
    let exe = go.with_extension("");
    let _ = std::fs::remove_file(&exe);
    let cmd = BuildCommand {
        files: vec![go],
        output: Some(exe.clone()),
        emit: Emit::Elf,
        keep_asm: false,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    match build_cmd::run_build(&cmd) {
        Ok(o) => (o.exit, o.stderr, exe),
        Err(e) => (2, e, exe),
    }
}

#[test]
fn u_m7_01_切片面已经放行() {
    // # 这条判据在 M9/Phase 1 换过面（`R-9-4`）
    //
    // M7 时 `U-M7-01` 说「`[]T` 全集不支持」，判据是「报 `CG-UNSUPPORTED-SLICE`
    // 并退出 1」。M9 把切片真的实现了（`make` / `Index` / `Slice` / `append` /
    // 字面量 / 越界检查），所以现在它必须**构建成功且跑对**。
    //
    // 反过来的守卫（「这条码不许再出现」）在 `native_slice.rs` 的
    // `t_m9_15_slice_码不许再出现`。两条一起才完整：一条说「现在能跑」，
    // 一条说「旧码不许复活」。
    let src = "package main\n\nfunc main() {\n\ts := []int{1, 2, 3}\n\ts[0] = 9\n\tprintln(s[0], len(s))\n}\n";
    let (exit, stderr, exe) = build("slice_probe", src);
    assert_eq!(exit, 0, "切片现在必须构建成功，stderr={stderr}");
    let run = std::process::Command::new(&exe)
        .output()
        .expect("应能启动产物");
    assert_eq!(run.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "9 3\n");
}

#[test]
fn u_m7_02_间接调用够不着_守卫() {
    // 函数值调用：M6 的拒绝面先拦住，后端到不了 `CalleeRef::Indirect`。
    let src =
        "package main\n\nfunc f() int { return 1 }\n\nfunc main() {\n\tg := f\n\tprintln(g())\n}\n";
    let (exit, stderr, _) = build("indirect_probe", src);
    assert_eq!(exit, 1, "应被静态拒绝，stderr={stderr}");
    assert!(
        !stderr.contains("CG-UNSUPPORTED-INDIRECT-CALL"),
        "M6 应该先拒（函数值）。若这条红了，说明 M6 放行了 —— \
         那时才需要给后端补间接调用的正向证据。实际：{stderr}"
    );
    assert!(
        stderr.contains("SSA-UNSUPPORTED-"),
        "拒绝必须来自 M6 的码，实际：{stderr}"
    );
}

#[test]
fn u_m7_03_方法值够不着_守卫() {
    let src = "package main\n\ntype T struct{ n int }\n\nfunc (t T) Get() int { return t.n }\n\nfunc main() {\n\tv := T{7}\n\tm := v.Get\n\tprintln(m())\n}\n";
    let (exit, stderr, _) = build("method_value_probe", src);
    assert_eq!(exit, 1, "应被静态拒绝，stderr={stderr}");
    assert!(
        !stderr.contains("CG-UNSUPPORTED-METHOD-VALUE"),
        "M6 应该先拒。若红了，说明 M6 放行了方法值。实际：{stderr}"
    );
    assert!(
        stderr.contains("SSA-UNSUPPORTED-"),
        "拒绝必须来自 M6 的码，实际：{stderr}"
    );
}

#[test]
fn u_m7_04_堆耗尽打一行并退出2() {
    // # 这条判据在 M9/Task 7 换过面（`R-9-3`）
    //
    // M7 时 arena 是**静态 8 MiB**，所以「2000 次 `new([1024]int)` 必然耗尽」
    // 就能撞出 `panic: out of arena`。M9 起堆是**可增长**的（mmap chunk 链），
    // 同一个程序会正常跑完 —— 判据的前提没了。
    //
    // 但它要钉的**意图**没变：内存耗尽时打一行到 stderr、退出 2、不继续执行。
    // 现在这件事由 `RGOC_HEAP_MAX` 触发（上限是唯一能「确定耗尽」的手段），
    // 文本也换成 `C9` §6 的那一行。
    //
    // 循环里写全局，防止被折叠掉。
    let src = "package main\n\nvar sink *[1024]int\n\nfunc main() {\n\tfor i := 0; i < 2000; i = i + 1 {\n\t\tp := new([1024]int)\n\t\tp[0] = i\n\t\tsink = p\n\t}\n\tprintln(\"should not reach\")\n}\n";
    let (exit, stderr, exe) = build("arena_probe", src);
    assert_eq!(exit, 0, "构建应成功，stderr={stderr}");
    let run = std::process::Command::new(&exe)
        .env("RGOC_HEAP_MAX", "1048576")
        .output()
        .expect("应能启动产物");
    assert_eq!(run.status.code(), Some(2), "堆耗尽 ⇒ 退出码 2");
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(
        err.contains("runtime error: out of memory (heap limit 1048576 bytes)"),
        "必须打一行到 stderr（带上限数值），实际：{err:?}"
    );
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(
        !out.contains("should not reach"),
        "耗尽后不该继续执行，stdout={out:?}"
    );
    let _ = repo_root();
}
