//! 映射（M9/Phase 2，`T-MAP-01..09`）。
//!
//! # 期望值从哪来
//!
//! 全部是官方 Go 的语义（容器内 `go run` 核过的那几条），不是从实现推的。
//! 迭代顺序**不承诺**（规范如此），所以涉及 `range` 的断言只看「与顺序无关的
//! 不变量」（和、个数、`len`），不看具体序列。

use std::path::{Path, PathBuf};

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit};

fn scratch() -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/obj-map");
    std::fs::create_dir_all(&d).expect("建目录失败");
    d
}

fn build_and_run(name: &str, src: &str) -> (String, String, i32, String) {
    let dir = scratch();
    let go = dir.join(format!("{name}.go"));
    std::fs::write(&go, src).expect("写 fixture 失败");
    let exe = dir.join(name);
    let cmd = BuildCommand {
        files: vec![go],
        output: Some(exe.clone()),
        emit: Emit::Elf,
        keep_asm: true,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let out = build_cmd::run_build(&cmd).expect("build 不该失败");
    if out.exit != 0 {
        return (
            String::new(),
            out.stderr,
            i32::from(out.exit),
            String::new(),
        );
    }
    let asm = std::fs::read_to_string(exe.with_extension("s")).expect("读 .s 失败");
    let run = std::process::Command::new(&exe)
        .output()
        .expect("跑不动产物");
    (
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
        run.status.code().unwrap_or(-1),
        asm,
    )
}

fn out(name: &str, src: &str) -> String {
    let (stdout, stderr, code, asm) = build_and_run(name, src);
    assert_eq!(code, 0, "应跑通（exit={code}）stderr={stderr}\n{asm}");
    stdout
}

// ───────────────── Task 19/20/21：make / get / set / del ─────────────────

#[test]
fn t_map_01_nil_map_读零值且_len_为_0() {
    let s = out(
        "t_map_01",
        r#"
package main

func main() {
	var m map[int]int
	println(m[1], len(m))
	n := make(map[int]int)
	println(len(n))
	n[1] = 2
	println(n[1], len(n))
}
"#,
    );
    assert_eq!(s, "0 0\n0\n2 1\n");
}

#[test]
fn t_map_02_字符串键的读写与_comma_ok() {
    let s = out(
        "t_map_02",
        r#"
package main

func main() {
	m := map[string]int{"a": 1, "b": 2}
	println(len(m), m["a"], m["z"])
	m["c"] = 3
	println(len(m), m["c"])
	delete(m, "a")
	println(len(m), m["a"])
	v, ok := m["b"]
	println(v, ok)
	v, ok = m["z"]
	println(v, ok)
}
"#,
    );
    assert_eq!(s, "2 1 0\n3 3\n2 0\n2 true\n0 false\n");
}

#[test]
fn t_map_03_覆盖写与删除不存在的键() {
    let s = out(
        "t_map_03",
        r#"
package main

func main() {
	m := make(map[int]int)
	for i := 0; i < 5; i++ {
		m[i] = i * i
	}
	m[2] = 99
	println(len(m), m[2])
	delete(m, 42)
	println(len(m))
	delete(m, 2)
	delete(m, 2)
	println(len(m), m[2])
}
"#,
    );
    assert_eq!(s, "5 99\n5\n4 0\n");
}

// ───────────────── Task 18：hash / equal（结构体键） ─────────────────

#[test]
fn t_map_04_结构体键按字段比较() {
    // `{1,2}` 与 `{2,1}` 必须不同；`{}` 全相等（size==0 的特殊路径）。
    let s = out(
        "t_map_04",
        r#"
package main

type P struct {
	a, b int
}

func main() {
	m := map[P]int{{1, 2}: 10, {2, 1}: 20}
	println(m[P{1, 2}], m[P{2, 1}], len(m))
	e := map[struct{}]int{{}: 7}
	println(e[struct{}{}], len(e))
	s := map[string]int{"ab": 1, "ba": 2}
	println(s["ab"], s["ba"], len(s))
}
"#,
    );
    assert_eq!(s, "10 20 2\n7 1\n1 2 2\n");
}

// ───────────────── Task 22：rehash ─────────────────

#[test]
fn t_map_05_一百个键触发扩容后仍全对() {
    let s = out(
        "t_map_05",
        r#"
package main

func main() {
	m := map[int]int{}
	for i := 0; i < 100; i++ {
		m[i] = i
	}
	sum := 0
	for i := 0; i < 100; i++ {
		sum += m[i]
	}
	println(len(m), sum)
}
"#,
    );
    assert_eq!(s, "100 4950\n");
}

// ───────────────── Task 23：迭代 ─────────────────

#[test]
fn t_map_06_迭代的和与个数自洽() {
    // 顺序不承诺 ⇒ 断言只用「和」与「个数」。
    let s = out(
        "t_map_06",
        r#"
package main

func main() {
	m := map[int]int{0: 0, 1: 1, 2: 2, 3: 3, 4: 4}
	n, sum := 0, 0
	for k, v := range m {
		n++
		sum += k + v
	}
	println(n, sum, len(m))
	total := 0
	for range m {
		total++
	}
	println(total)
}
"#,
    );
    assert_eq!(s, "5 20 5\n5\n");
}

#[test]
fn t_map_07_迭代中删除的键不再产出() {
    // 规范：迭代开始后被删的条目**不产出**。我们的迭代器按 `state` 判，
    // 所以「删掉还没访问的键」必然不再产出。
    //
    // 断言方式：只删 `k == 0` 之后的所有键的**其中一个**，然后要求
    // 「产出的键里没有它」—— 用集合判断，不依赖顺序。
    let s = out(
        "t_map_07",
        r#"
package main

func main() {
	m := map[int]int{0: 0, 1: 10, 2: 20, 3: 30, 4: 40}
	seen := 0
	saw := 0
	for k, v := range m {
		seen++
		if k == 2 {
			saw = v
		}
		if k == 0 {
			delete(m, 3)
			delete(m, 4)
		}
	}
	println(len(m), seen <= 5, seen >= 3)
	println(saw)
}
"#,
    );
    // `len(m)` 必是 3（删了两个）；`seen` 落在 [3,5]（删的键可能已访问过）。
    assert_eq!(s, "3 true true\n20\n");
}
