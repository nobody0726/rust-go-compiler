//! 指令选择的**逐变体覆盖**（`NAT-2` / `G-M7-03`）。
//!
//! # 为什么要有这张表
//!
//! 「14 个 `Instr` + 5 个 `Term` 都有实现分支」是**编译期**性质（`match` 没有通配，
//! 加变体必编译失败）。但「都有**判据**」不是 —— 一支从没被跑到的分支与不存在
//! 没有区别。这张表把「哪一支由哪条 fixture 覆盖」写死。
//!
//! # 19 个变体里只有 17 个能被覆盖，另外 2 个**没有生产者**
//!
//! 这是实测结论，不是省略（`R-13-7`）：
//!
//! | 变体 | 为什么没有 fixture |
//! |---|---|
//! | `Instr::Copy` | `build.rs` **从不构造**它。46 个语料的 SSA 里出现 0 次；赋值走 `Load`/`Store`，`opt/copy.rs` 只消费不生产。它是「pass 的插入点」而不是「构建器的产物」 |
//! | `Term::Unreachable` | 同样只有匹配点（`build.rs:3804`），没有构造点。Go 源码里的 `panic` 走 `Term::Panic` |
//!
//! 这两条写在这里而不是留在 `M7-manifest.json` 的 `open_gaps` 里：它们不是「没做」，
//! 是「做不了」—— 而「做不了」必须写清楚**为什么**，否则下一个人会以为漏了。
//!
//! # 判据
//!
//! 每条 `native == 解释器 == 手算`。手算那一边不是为了炫技：两边一起错时对撞仍然绿。

use std::path::{Path, PathBuf};

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit, RunCommand, Via};

fn scratch() -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/native-isel");
    std::fs::create_dir_all(&d).expect("建目录失败");
    d
}

fn run_native(name: &str, src: &str) -> (String, String, Option<i32>) {
    let dir = scratch();
    let go = dir.join(format!("{name}.go"));
    std::fs::write(&go, src).expect("写 fixture 失败");
    let exe = dir.join(name);
    let b = BuildCommand {
        files: vec![go.clone()],
        output: Some(exe.clone()),
        emit: Emit::Elf,
        keep_asm: false,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let out = build_cmd::run_build(&b).expect("build 不该失败");
    assert_eq!(out.exit, 0, "{name} 构建失败：{}", out.stderr);
    let r = std::process::Command::new(&exe)
        .output()
        .expect("应能启动产物");
    (
        String::from_utf8_lossy(&r.stdout).into_owned(),
        String::from_utf8_lossy(&r.stderr).into_owned(),
        r.status.code(),
    )
}

fn run_hir(name: &str, src: &str) -> (String, String, Option<i32>) {
    // 自己写盘，不依赖 `run_native` 先跑过 —— 否则两条测试的执行顺序一旦变了，
    // 这条会读到上一个 fixture 的源文件，而**它仍然会给出一个「合理」的输出**。
    let go = scratch().join(format!("{name}.go"));
    std::fs::write(&go, src).expect("写 fixture 失败");
    let cmd = RunCommand {
        files: vec![go],
        dump_hir: false,
        max_steps: None,
        lang: None,
        via: Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
    };
    let o = rgoc_driver::run_cmd::run_run(&cmd).expect("解释器不该失败");
    (o.stdout, o.stderr, Some(i32::from(o.exit)))
}

/// 表格一行：变体名、源码、期望 stdout。
struct Row {
    variant: &'static str,
    src: &'static str,
    want: &'static str,
}

/// 17 个**可产生**的变体，每个一条。
const ROWS: &[Row] = &[
    Row {
        variant: "Const",
        src: "package main\n\nfunc id(a int) int { return a }\n\nfunc main() {\n\tprintln(id(7))\n}\n",
        want: "7\n",
    },
    Row {
        variant: "Bin",
        src: "package main\n\nfunc f(a, b int) int { return a*b + 1 }\n\nfunc main() {\n\tprintln(f(3, 4))\n}\n",
        want: "13\n",
    },
    Row {
        variant: "Un",
        src: "package main\n\nfunc f(a int) int { return -a }\n\nfunc g(b bool) bool { return !b }\n\nfunc main() {\n\tprintln(f(5), g(true))\n}\n",
        want: "-5 false\n",
    },
    Row {
        variant: "Convert",
        src: "package main\n\nfunc f(a int) int32 { return int32(a) }\n\nfunc main() {\n\tprintln(f(300), f(-1))\n}\n",
        want: "300 -1\n",
    },
    // `AddrOf` / `Load` / `Store` 三条一起：全局的读写必然同时用到它们。
    Row {
        variant: "AddrOf+Load+Store",
        src: "package main\n\nvar g int\n\nfunc set(v int) { g = v }\n\nfunc main() {\n\tset(5)\n\tprintln(g)\n}\n",
        want: "5\n",
    },
    Row {
        variant: "Field",
        src: "package main\n\ntype S struct {\n\tA int\n\tB int\n}\n\nfunc get(s *S) int { return s.B }\n\nfunc main() {\n\ts := &S{1, 2}\n\tprintln(get(s))\n}\n",
        want: "2\n",
    },
    Row {
        variant: "Index",
        src: "package main\n\nfunc get(a *[3]int, i int) int { return a[i] }\n\nfunc main() {\n\tvar a [3]int\n\ta[0] = 4\n\ta[1] = 5\n\tprintln(get(&a, 1))\n}\n",
        want: "5\n",
    },
    Row {
        variant: "Composite",
        src: "package main\n\ntype S struct {\n\tA int\n\tB int\n}\n\nfunc f() int {\n\ts := S{7, 8}\n\treturn s.A + s.B\n}\n\nfunc main() {\n\tprintln(f())\n}\n",
        want: "15\n",
    },
    Row {
        variant: "Make(new)",
        src: "package main\n\nfunc f() int {\n\tp := new(int)\n\t*p = 9\n\treturn *p\n}\n\nfunc main() {\n\tprintln(f())\n}\n",
        want: "9\n",
    },
    Row {
        variant: "Call",
        src: "package main\n\nfunc fac(n int) int {\n\tif n < 2 {\n\t\treturn 1\n\t}\n\treturn n * fac(n-1)\n}\n\nfunc main() {\n\tprintln(fac(5))\n}\n",
        want: "120\n",
    },
    Row {
        variant: "Goto",
        src: "package main\n\nfunc f(n int) int {\n\ti := 0\nloop:\n\tif i < n {\n\t\ti = i + 1\n\t\tgoto loop\n\t}\n\treturn i\n}\n\nfunc main() {\n\tprintln(f(4))\n}\n",
        want: "4\n",
    },
    Row {
        variant: "If",
        src: "package main\n\nfunc f(a int) int {\n\tif a > 0 {\n\t\treturn 1\n\t}\n\treturn 0\n}\n\nfunc main() {\n\tprintln(f(1), f(-1))\n}\n",
        want: "1 0\n",
    },
    Row {
        variant: "Return",
        src: "package main\n\nfunc f() (int, int) { return 3, 4 }\n\nfunc main() {\n\ta, b := f()\n\tprintln(a, b)\n}\n",
        want: "3 4\n",
    },
];

#[test]
fn nat2_可产生的变体各有至少一条_fixture() {
    for r in ROWS {
        let (out, err, code) = run_native(r.variant, r.src);
        assert_eq!(
            code,
            Some(0),
            "{}: 退出码 {code:?}，stderr={err}",
            r.variant
        );
        let (hout, herr, hcode) = run_hir(r.variant, r.src);
        assert_eq!(
            hcode,
            Some(0),
            "{}: 解释器退出码 {hcode:?}，stderr={herr}",
            r.variant
        );
        assert_eq!(out, hout, "{}: native 与解释器不一致", r.variant);
        assert_eq!(out, r.want, "{}: 与手算期望值不一致", r.variant);
    }
    // 分母写出来：空表上的全称命题不是证据。
    assert_eq!(
        ROWS.len(),
        13,
        "表里应该覆盖 13 条（17 个变体的 13 组 fixture）"
    );
}

#[test]
fn nat2_panic_终止符() {
    let src = "package main\n\nfunc f(n int) {\n\tif n > 0 {\n\t\tpanic(n)\n\t}\n}\n\nfunc main() {\n\tf(3)\n\tprintln(\"unreachable\")\n}\n";
    let (out, err, code) = run_native("panic_term", src);
    assert_eq!(code, Some(2), "panic ⇒ 退出码 2");
    assert!(
        err.contains("panic: 3"),
        "stderr 应有一行 panic，实际：{err:?}"
    );
    assert!(out.is_empty(), "panic 之后不该再有 stdout：{out:?}");
}

#[test]
fn nat2_切片走拒绝面而不是_变体分支() {
    // 14 个 `Instr` 里只有 `Slice` 是「有变体、有分支、但永远到不了」——
    // 它在 `func_has_slice` 那一层就被拒了。所以它的判据是**码 + 退出码**，
    // 不是「指令选择对不对」。
    let src = "package main\n\nfunc main() {\n\ts := []int{1}\n\tprintln(s[0])\n}\n";
    let dir = scratch();
    let go = dir.join("slice_variant.go");
    std::fs::write(&go, src).expect("写 fixture 失败");
    let b = BuildCommand {
        files: vec![go],
        output: Some(dir.join("slice_variant")),
        emit: Emit::Elf,
        keep_asm: false,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let out = build_cmd::run_build(&b).expect("build 不该返回 Err");
    assert_eq!(out.exit, 1);
    assert!(
        out.stderr.contains("CG-UNSUPPORTED-SLICE"),
        "{}",
        out.stderr
    );
}
