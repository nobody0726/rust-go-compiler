//! 切片（M9/Phase 1，`T-SLICE-01..08`）。
//!
//! # 期望值从哪来
//!
//! 全部是官方 `go run` 的输出（容器内实测），不是从实现推的。
//!
//! # 为什么切片值得一整个 Phase
//!
//! `[]T` 是**第一个三字值**（`{ptr,len,cap}`）：M7 的 isel 只处理过单字与
//! `string` 的两字。三字带来的新东西是「结果槽有三个字要写」与「`s[i]` 的
//! 底座不是地址而是**头里的 ptr**」—— 两处都容易写成「看起来对」。

use std::path::{Path, PathBuf};

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit};

fn scratch() -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/obj-slice");
    std::fs::create_dir_all(&d).expect("建目录失败");
    d
}

fn build_and_run(name: &str, src: &str) -> (String, i32, String) {
    let dir = scratch();
    let go = dir.join(format!("{name}.go"));
    std::fs::write(&go, src).expect("写 fixture 失败");
    let exe = dir.join(name);
    let cmd = BuildCommand {
        roots: Vec::new(),
        files: vec![go],
        output: Some(exe.clone()),
        emit: Emit::Elf,
        keep_asm: true,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let out = build_cmd::run_build(&cmd).expect("build 不该失败");
    if out.exit != 0 {
        return (String::new(), i32::from(out.exit), out.stderr);
    }
    let asm = std::fs::read_to_string(exe.with_extension("s")).expect("读 .s 失败");
    let run = std::process::Command::new(&exe)
        .output()
        .expect("跑不动产物");
    (
        String::from_utf8_lossy(&run.stdout).into_owned(),
        run.status.code().unwrap_or(-1),
        asm,
    )
}

fn out(name: &str, src: &str) -> String {
    let (stdout, code, asm) = build_and_run(name, src);
    assert_eq!(code, 0, "应跑通（exit={code}）：\n{asm}");
    stdout
}

// ───────────────── Task 11：make([]T, …) ─────────────────

#[test]
fn t_slice_01_make_的三种形态() {
    let s = out(
        "t_slice_01",
        r#"
package main

func main() {
	a := make([]int, 3)
	b := make([]int, 1, 5)
	var c []int
	println(len(a), cap(a), len(b), cap(b), len(c), cap(c))
	d := make([]struct{}, 3)
	println(len(d), cap(d))
}
"#,
    );
    assert_eq!(s, "3 3 1 5 0 0\n3 3\n");
}

#[test]
fn t_slice_02_元素可读写() {
    let s = out(
        "t_slice_02",
        r#"
package main

func main() {
	a := make([]int, 3)
	a[0] = 7
	a[1] = 8
	a[2] = a[0] + a[1]
	println(a[0], a[1], a[2])
}
"#,
    );
    assert_eq!(s, "7 8 15\n");
}

#[test]
fn t_slice_02b_底层数组在堆上() {
    // spec §3.4 第 3 条「新对象一律 arena」的可观测形式：
    // `make([]int, 3)` 必须走 `rt_makeslice`（它内部 `rt_arena_alloc`），
    // **不是**在帧里开一块。判据是产物里出现 `bl rt_makeslice`。
    //
    // 只看输出的话，「在栈上开一块」也能打印对 —— 而这个差别在
    // 「slice 逃出函数」时才现形（那时栈那块已经没了）。
    let (stdout, code, asm) = build_and_run(
        "t_slice_02b",
        r#"
package main

func main() {
	a := make([]int, 3)
	a[0] = 1
	println(a[0])
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
    assert!(
        asm.contains("bl rt_makeslice"),
        "make([]T, n) 必须走 rt_makeslice（否则底层数组没进堆）"
    );
}

// ───────────────── Task 12：append ─────────────────

#[test]
fn t_slice_05_append_不扩容时复用底层数组() {
    let s = out(
        "t_slice_05",
        r#"
package main

func main() {
	s := make([]int, 2, 4)
	old := s
	s = append(s, 7)
	println(s[2], old[0], len(s), cap(s))
	t := make([]int, 2, 2)
	u := t
	t = append(t, 9)
	println(t[2], len(u), len(t), cap(t))
}
"#,
    );
    // 第二轮：cap=2 已满 ⇒ 扩容，`u` 仍是 len 2（不共享新数组）
    assert_eq!(s, "7 0 3 4\n9 2 3 4\n");
}

#[test]
fn t_slice_06_append_多个与展开() {
    let s = out(
        "t_slice_06",
        r#"
package main

func main() {
	var s []int
	s = append(s, 1, 2, 3)
	println(len(s), s[0], s[2])
	t := []int{4, 5}
	s = append(s, t...)
	println(len(s), s[3], s[4])
}
"#,
    );
    assert_eq!(s, "3 1 3\n5 4 5\n");
}

// ───────────────── Task 13：切片表达式与字面量 ─────────────────

#[test]
fn t_slice_07_字面量与切片表达式() {
    let s = out(
        "t_slice_07",
        r#"
package main

func main() {
	s := []int{1, 2, 3, 4, 5}
	t := s[1:3]
	println(len(t), cap(t), t[0])
	u := s[1:3:4]
	println(cap(u))
	t[0] = 9
	println(s[1])
	w := s[:2]
	println(len(w), w[0])
	v := s[3:]
	println(len(v), v[0])
}
"#,
    );
    assert_eq!(s, "2 4 2\n3\n9\n2 1\n2 4\n");
}

#[test]
fn t_slice_08_字符串切片() {
    // 字符串切片是**两字**结果（不是三字）：`string` 没有 cap。
    let s = out(
        "t_slice_08",
        r#"
package main

func main() {
	s := "hello"
	t := s[1:3]
	println(t, len(t))
	u := s[:2]
	println(u)
}
"#,
    );
    assert_eq!(s, "el 2\nhe\n");
}

// ───────────────── Task 14：越界检查 ─────────────────

#[test]
fn t_slice_09_越界打官方文本并退出2() {
    let (stdout, code, asm) = build_and_run(
        "t_slice_09",
        r#"
package main

func main() {
	s := []int{1, 2, 3}
	i := 3
	println(s[i])
}
"#,
    );
    assert_eq!(code, 2, "越界必须退出 2（stdout={stdout:?}）");
    assert_eq!(
        stdout, "",
        "越界前不该有输出（`println(s[i])` 的求值先于打印）\n{asm}"
    );
}

#[test]
fn t_slice_09b_越界的文本逐字节() {
    // 文本必须与解释器一致（`C9` §4.5）：`panic: runtime error: index out of
    // range [3] with length 3`。所以这条要单独看 stderr。
    let dir = scratch();
    let go = dir.join("t_slice_09b.go");
    std::fs::write(
        &go,
        "package main\n\nfunc main() {\n\ts := []int{1, 2, 3}\n\ti := 3\n\tprintln(s[i])\n}\n",
    )
    .unwrap();
    let exe = dir.join("t_slice_09b");
    let cmd = BuildCommand {
        roots: Vec::new(),
        files: vec![go],
        output: Some(exe.clone()),
        emit: Emit::Elf,
        keep_asm: false,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let out = build_cmd::run_build(&cmd).unwrap();
    assert_eq!(out.exit, 0, "构建失败：{}", out.stderr);
    let run = std::process::Command::new(&exe).output().unwrap();
    let err = String::from_utf8_lossy(&run.stderr);
    assert_eq!(
        err, "panic: runtime error: index out of range [3] with length 3\n",
        "文本必须与解释器逐字节一致"
    );
}
