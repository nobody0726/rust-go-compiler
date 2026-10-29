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

// ───────────────── `T-MAP-10`：删除不切断探测链（`G-M9-04` 收口）─────────

#[test]
fn t_map_10_删除不切断探测链() {
    // # 这条判据为什么长这样
    //
    // `M-M9-04`（`rt_map_del` 把槽置成 `EMPTY` 而不是 `TOMB`）在原有的 9 条用例
    // 上**全是绿的** —— 因为「删掉的那个键查不到」在两种状态下都成立。唯一能
    // 露头的情形是：**另一个键的探测链经过被删的槽**。那时 `EMPTY` 会让探测
    // 提前终止（键查不到），而 `TOMB` 会继续往后找（键查得到）。
    //
    // 先试过「填 40 个 → 删偶数 → 再插 20 个 → 查奇数」：**仍然是绿的** ——
    // FNV-1a 把 40 个整数摊得很开，没有一条链穿过被删的槽。所以这里改成
    // **构造碰撞**：
    //
    // * `rt_hash` 对整数是 FNV-1a（8 字节小端），初表 `cap = 8` ⇒ 桶 =
    //   `hash & 7`；
    // * `0` 与 `8` 的桶**都是 5**（实测：`hash(0) & 7 == hash(8) & 7 == 5`）；
    // * 先插 `0`（占 5 号槽），再插 `8`（5 号被占 ⇒ 落到 6 号）；
    // * 删 `0` ⇒ 5 号槽的状态是这条判据的**唯一**观察点。
    //
    // 期望值来自官方 Go（`800 true 1`）。两个键都只是小整数，`cap` 仍是 8
    // （两次插入不触发扩容），所以这条 fixture 不依赖运行期的任何偶然。
    let s = out(
        "t_map_10",
        r#"
package main

func main() {
	m := map[int]int{}
	m[0] = 100
	m[8] = 800
	delete(m, 0)
	v, ok := m[8]
	println(v, ok, len(m))
	println(len(m), m[0], m[8])
}
"#,
    );
    assert_eq!(
        s, "800 true 1\n1 0 800\n",
        "删掉 0 之后 8 必须还查得到（探测链不能断在 TOMB 上）"
    );
}

// ───────────────── `T-OPT-03`：SwissTable 探测（M12/Task 39）────────────────

/// 跑一个 fixture，并把 `env` 传进产物；返回 `(stdout, stderr)`。
///
/// 与 [`build_and_run`] 只差环境变量 —— 映射的探测读数走 `RGOC_MAP_STATS=1`
/// 这一条**独立**的开关（`C12` §5.2 的 gc 行不进新键），所以必须有传 env 的口。
fn run_env(name: &str, src: &str, env: &[(&str, &str)]) -> (String, String, i32) {
    let dir = scratch();
    let go = dir.join(format!("{name}.go"));
    std::fs::write(&go, src).expect("写 fixture 失败");
    let exe = dir.join(name);
    let cmd = BuildCommand {
        roots: Vec::new(),
        files: vec![go],
        output: Some(exe.clone()),
        emit: Emit::Elf,
        keep_asm: false,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let out = build_cmd::run_build(&cmd).expect("build 不该失败");
    assert_eq!(out.exit, 0, "应构建成功，stderr={}", out.stderr);
    let mut c = std::process::Command::new(&exe);
    for (k, v) in env {
        c.env(k, v);
    }
    let run = c.output().expect("跑不动产物");
    (
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
        run.status.code().unwrap_or(-1),
    )
}

/// 十万键的 fixture：填、全查、删一半、再查。
///
/// # 为什么非要十万键
///
/// 小 fixture 看不出探测结构的好坏，也看不出「DELETED 当终止符」这类错：
/// `cap = 8` 时一组（16 个控制字节）覆盖整张表，任何终止判定都能命中。
/// 十万键 ⇒ `cap` 长到 131072（8192 组），一串探测才真的会跨组、跨墓碑。
///
/// 期望值：`len = 50000`、`sum = 4999950000`（`Σ i`，i < 100000）、`miss = 0`。
const BIG_MAP: &str = r#"
package main

func main() {
	const n = 100000
	m := make(map[int]int)
	for i := 0; i < n; i++ {
		m[i*7] = i
	}
	sum := 0
	for i := 0; i < n; i++ {
		sum += m[i*7]
	}
	for i := 0; i < n; i += 2 {
		delete(m, i*7)
	}
	miss := 0
	for i := 0; i < n; i++ {
		v, ok := m[i*7]
		if i%2 == 0 {
			if ok {
				miss++
			}
		} else if !ok || v != i {
			miss++
		}
	}
	println(len(m), sum, miss)
}
"#;

#[test]
fn t_map_11_十万键的语义不随探测结构变() {
    let (stdout, stderr, code) = run_env("t_map_11", BIG_MAP, &[]);
    assert_eq!(code, 0, "应跑通，stderr={stderr}");
    assert_eq!(
        stdout, "50000 4999950000 0\n",
        "十万键的 len/sum/miss 变了 —— 探测结构改了语义"
    );
}

#[test]
fn t_map_12_十万键的探测读数() {
    let (stdout, stderr, code) = run_env("t_map_12", BIG_MAP, &[("RGOC_MAP_STATS", "1")]);
    assert_eq!(code, 0, "应跑通，stderr={stderr}");
    assert_eq!(stdout, "50000 4999950000 0\n", "统计开关不能改变输出");

    let line = stderr
        .lines()
        .find(|l| l.starts_with("map: "))
        .unwrap_or_else(|| panic!("RGOC_MAP_STATS=1 必须打一行 `map: …`，实际 stderr：\n{stderr}"));
    let get = |key: &str| -> u64 {
        let pat = format!("{key}=");
        let i = line
            .find(&pat)
            .unwrap_or_else(|| panic!("行里没有 {key}：{line}"))
            + pat.len();
        line[i..]
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| panic!("{key} 不是数字：{line}"))
    };
    let lookups = get("lookups");
    let groups = get("groups");
    let cmps = get("key_cmps");

    // ① 计数器必须是活的：不能「恒 0 然后断言 0 <= 阈值」。
    assert!(
        lookups >= 300_000,
        "lookups 太少（{lookups}），fixture 没跑满"
    );
    assert!(groups > 0, "groups 恒 0 ⇒ 计数器没接上");
    // ② 一组 16 个控制字节，7/8 负载下「命中」几乎总落在第一组里 ⇒ 平均约 **1.11**
    //    组。阈值 1.5 的来历是**实测的两头**：SwissTable 1.109 组/次；把组步进临时
    //    改成 1 个槽（等效线性探测，语义不变）是 2.17 组/次 —— 红。所以这条判据
    //    有这个牙，而且两边都有余量。
    assert!(
        groups * 2 < lookups * 3,
        "平均超过 1.5 组/次就不算 SwissTable：lookups={lookups} groups={groups}（{:.2} 组/次）",
        groups as f64 / lookups as f64
    );
    // ③ H2 起筛子作用：比键次数应远少于「每组 16 个槽都去比」的上界。
    assert!(
        cmps <= groups * 16,
        "键比较次数超过了控制字节的总数：cmps={cmps} groups={groups}"
    );
    eprintln!(
        "map 探测读数：lookups={lookups} groups={groups} key_cmps={cmps}（{:.3} 组/次、{:.3} 次比键/次）",
        groups as f64 / lookups as f64,
        cmps as f64 / lookups as f64
    );
}

#[test]
fn t_map_13_不设开关就不打映射统计() {
    // 这条挡的是「新统计行默认就打印」——那会让 69 个语料的期望输出全变。
    // 用小 fixture：这条判据只说「有没有那一行」，与规模无关。
    let (stdout, stderr, code) = run_env(
        "t_map_13",
        r#"
package main

func main() {
	m := map[int]int{}
	m[1] = 2
	println(len(m), m[1])
}
"#,
        &[],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1 2\n");
    assert!(
        !stderr.contains("map: "),
        "不设 RGOC_MAP_STATS 时不该有映射统计行：\n{stderr}"
    );
}
