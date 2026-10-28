//! `rt_alloc`（M9/Task 6，`T-MEM-01/02/05`）。
//!
//! # 三条判据，缺一条就会假绿
//!
//! 1. **跑得对**：100 次 `new(int)` 之后读写正常（`7 100`）。
//! 2. **真的走了新分配器**：`.s` 里必须出现 `bl rt_alloc`，且**不再**出现
//!    `bl rt_arena_alloc`。只测第 1 条的话，`new` 仍走老的 `rt_arena_alloc`
//!    时测试照样过 —— 而「新分配器从没被调用」是最容易漏的一类假绿
//!    （`R-9-1` 就是为了这条才把 desc 发射提到 Task 6 之前）。
//! 3. **`need_zero` 的实参来自 `needs_zero`**：纯整数类型传 0、含指针的类型传 1。
//!    这条把「编译期的判定」与「运行期的调用」接上 —— 只靠 mmap 恰好给零页是
//!    **不可观测**的（M9 的 arena 只增不复用），所以必须在 `.s` 上看实参。

use std::path::{Path, PathBuf};

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit};

fn scratch() -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/mem-alloc");
    std::fs::create_dir_all(&d).expect("建目录失败");
    d
}

/// 编 + 跑，返回 `(stdout, exit, asm)`。
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
    assert_eq!(out.exit, 0, "构建失败：{}", out.stderr);
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

/// `bl rt_alloc` 之前那条给 `x1`（`need_zero`）的 `movz` 立即数。
fn need_zero_arg(asm: &str) -> Vec<u8> {
    let lines: Vec<&str> = asm.lines().map(str::trim).collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if *l != "bl rt_alloc" {
            continue;
        }
        // 往前找最近的 `movz x1, #N`（中间可能夹着 `adrp/add x0`）。
        for back in (0..i).rev() {
            let prev = lines[back];
            if prev.starts_with("movz x1,") {
                let n: u8 = prev
                    .rsplit('#')
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(255);
                out.push(n);
                break;
            }
            if prev.starts_with("bl ") {
                break; // 跨过了上一个调用 ⇒ 这个 x1 不是给本调用的
            }
        }
    }
    out
}

#[test]
fn 一百次_new_都对且走新分配器() {
    let (out, code, asm) = build_and_run(
        "t_mem_01",
        r#"
package main

func main() {
	var ps [100]*int
	for i := range ps {
		ps[i] = new(int)
		*ps[i] = i * 2
	}
	sum := 0
	for i := range ps {
		sum += *ps[i]
	}
	println(*ps[0], len(ps), sum)
}
"#,
    );
    assert_eq!(code, 0);
    // sum = 2*(0+1+…+99) = 9900
    assert_eq!(out, "0 100 9900\n");

    assert!(
        asm.contains("bl rt_alloc"),
        "`new(T)` 必须走 `rt_alloc`（否则新分配器一次都没被调用）"
    );
    assert!(
        !asm.contains("bl rt_arena_alloc"),
        "`new(T)` 不该再走 `rt_arena_alloc`"
    );
    // 纯 int：不需要清零
    assert_eq!(
        need_zero_arg(&asm),
        vec![0u8; asm.matches("bl rt_alloc").count()],
        "`new(int)` 的 need_zero 必须是 0"
    );
}

#[test]
fn 含指针的类型要清零() {
    let (out, code, asm) = build_and_run(
        "t_mem_04",
        r#"
package main

type P struct {
	s string
	n int
}

func main() {
	p := new(P)
	p.n = 7
	p.s = "ok"
	println(p.n, p.s)
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(out, "7 ok\n");
    let args = need_zero_arg(&asm);
    assert!(!args.is_empty(), "应至少有一次 rt_alloc 调用");
    assert!(
        args.contains(&1),
        "`new(P)`（含 string ⇒ 含指针）的 need_zero 必须是 1，实际：{args:?}"
    );
}

#[test]
fn new_出来的对象读字段_0_是零() {
    // 判据 `T-HDR-01`（M10 前置，`R-10-1`）。
    //
    // # 这条判据为什么必须单独存在
    //
    // `rt_alloc` 的块形状是 `[+0 *TypeDesc][+8 data...]`（`C5` §11.1 的 `D-M9-1`）。
    // 它**必须返回 `data`**（块首 + 8）—— 返回块首的话，Go 侧的指针就指到了对象头，
    // 于是字段偏移 0 与 desc 槽重叠：
    //
    // * `p.a` 读出来是 **desc 指针**（实测 `187650506776800`）；
    // * `need_zero` 的 memset 从 `块首+8` 起算，正好**漏掉**字段 0；
    // * 对象头在第一次写字段时就被覆盖 ⇒ M10 的 GC 读不到 desc。
    //
    // 这条一直没被抓到，是因为 `含指针的类型要清零` 那条用例**先把每个字段都写了一遍**
    // 再读（`p.n = 7; p.s = "ok"`）—— 它验的是「need_zero 这个参数传对了」，
    // 不是「新对象读出来是零」。**判据只覆盖了正确实现那一面。**
    let (out, code, _asm) = build_and_run(
        "t_hdr_01",
        r#"
package main

type S struct {
	a int
	b int
}

func main() {
	p := new(S)
	println(p.a, p.b)
	q := new([2]int)
	println(q[0], q[1])
	var r *S
	r = new(S)
	println(r.a)
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(
        out, "0 0\n0 0\n0\n",
        "`new(T)` 读字段 0 必须是零值（不是对象头里的 desc 指针）"
    );
}

#[test]
fn 零大小类型的_new_不给独立地址() {
    // `new([0]int)` 与 `new(struct{})` 都该拿到 `rt_zerobase`（`zerosize.go` 的要求），
    // 而不是各分配一块。判据：跑通 + 两次 new 的指针相等（Go 允许比较 `*[0]int`）。
    let (out, code, _asm) = build_and_run(
        "t_mem_05",
        r#"
package main

func main() {
	a := new([0]int)
	b := new([0]int)
	println(a == b)
}
"#,
    );
    assert_eq!(code, 0);
    assert_eq!(out, "true\n", "零大小对象必须共享同一个哨兵地址");
}
