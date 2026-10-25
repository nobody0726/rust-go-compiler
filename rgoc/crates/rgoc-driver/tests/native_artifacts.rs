//! 产物形态与值语义：可复现（`T-NAT-48`）+ 聚合拷贝（`T-NAT-25` / `G-M7-04` / `G-M7-05`）。
//!
//! # 为什么「可复现」是一条判据而不是洁癖
//!
//! `C5` §9 冻结：同一份 SSA 生成的 `.s` 必须逐字节相同。理由不是好看 ——
//! 是**判据的成本**：`.s` 一旦随环境变（时间戳、绝对路径、哈希序），
//! 「这次与上次一样」就不再可测，而所有「拿两份产物比」的验证都失去意义。
//!
//! 这里连 ELF 一起测：`-Wl,-s` 去掉符号表之后，剩下的不确定性只有 build-id
//! 与 `.comment`。真出现差异时，这条测试会直接指出是 `.s` 还是 ELF 那一步。
//!
//! # 为什么「聚合赋值是拷贝」值得单独一条
//!
//! Go 的赋值语义是**值复制**。SSA 里 `Instr::Copy` 对 >8 字节走 `rt_memcpy`，
//! 而 `< 8` 字节走单字 load/store —— 两条路都要对，而且**槽共享**是这套设计里
//! 最自然的错法（把聚合「优化」成指针，一切照跑，只是改一个动两个）。

use std::path::{Path, PathBuf};

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit};

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/native-artifacts")
        .join(name);
    std::fs::create_dir_all(&d).expect("建目录失败");
    d
}

fn build_to(dir: &Path, src_name: &str, src: &str, emit: Emit, keep_asm: bool) -> PathBuf {
    let go = dir.join(format!("{src_name}.go"));
    std::fs::write(&go, src).expect("写源失败");
    let exe = dir.join(src_name);
    let cmd = BuildCommand {
        files: vec![go],
        output: Some(exe.clone()),
        emit,
        keep_asm,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let out = build_cmd::run_build(&cmd).expect("build 不该失败");
    assert_eq!(out.exit, 0, "构建失败：{}", out.stderr);
    if emit == Emit::Asm {
        exe.with_extension("s")
    } else {
        exe
    }
}

#[test]
fn t_nat_48_同一份源码两次构建的汇编与_elf_都逐字节相同() {
    let src = "package main\n\nvar g int\n\nfunc add(a, b int) int { return a + b }\n\nfunc main() {\n\tg = add(1, 2)\n\tprintln(g, 1.5, \"hi\", complex(1.0, 2.0))\n}\n";
    let a = scratch("repro_a");
    let b = scratch("repro_b");

    let s1 = build_to(&a, "prog", src, Emit::Asm, true);
    let s2 = build_to(&b, "prog", src, Emit::Asm, true);
    let t1 = std::fs::read(&s1).expect("读 .s 失败");
    let t2 = std::fs::read(&s2).expect("读 .s 失败");
    assert_eq!(
        t1,
        t2,
        "同一份源码两次生成的 .s 不同（{} vs {} 字节）",
        t1.len(),
        t2.len()
    );

    let e1 = build_to(&a, "elf", src, Emit::Elf, false);
    let e2 = build_to(&b, "elf", src, Emit::Elf, false);
    let x1 = std::fs::read(&e1).expect("读 ELF 失败");
    let x2 = std::fs::read(&e2).expect("读 ELF 失败");
    assert_eq!(
        x1.len(),
        x2.len(),
        "两次链接的 ELF 大小不同（{} vs {}）",
        x1.len(),
        x2.len()
    );
    if x1 != x2 {
        // 指出差异位置，省掉手工 `cmp -l` 的一轮。
        let at = x1.iter().zip(&x2).position(|(p, q)| p != q).unwrap_or(0);
        panic!(
            "两次链接的 ELF 不同，首个差异在字节 {at}（共 {} 字节）",
            x1.len()
        );
    }
}

#[test]
fn t_nat_25_结构体赋值是拷贝_改一个不动另一个() {
    let src = "package main\n\ntype S struct {\n\tA int\n\tB int\n}\n\nfunc main() {\n\tx := S{1, 2}\n\ty := x\n\ty.A = 99\n\tprintln(x.A, x.B, y.A, y.B)\n}\n";
    let dir = scratch("copy_struct");
    let exe = build_to(&dir, "prog", src, Emit::Elf, false);
    let out = std::process::Command::new(&exe).output().expect("启动失败");
    assert!(out.status.success(), "退出码 {:?}", out.status.code());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1 2 99 2\n");
}

#[test]
fn t_nat_25_大数组赋值是拷贝_改一个不动另一个() {
    // 24 字节 → 走 `rt_memcpy`（`Copy` 的 >8 字节分支）。
    let src = "package main\n\nfunc main() {\n\tvar a [3]int\n\ta[0] = 1\n\ta[1] = 2\n\ta[2] = 3\n\tb := a\n\tb[1] = 99\n\tprintln(a[0], a[1], a[2], b[0], b[1], b[2])\n}\n";
    let dir = scratch("copy_array");
    let exe = build_to(&dir, "prog", src, Emit::Elf, false);
    let out = std::process::Command::new(&exe).output().expect("启动失败");
    assert!(out.status.success(), "退出码 {:?}", out.status.code());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1 2 3 1 99 3\n");
}
