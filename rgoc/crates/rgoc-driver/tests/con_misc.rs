//! 三块补齐的判据（`M8-plan.md` Task 45–51；`T-CON-43..51`）。
//!
//! # 三块分别是什么
//!
//! 1. **map 迭代快照**（`D-M8-12`）：进入 `range` 时记下 key 列表，取的
//!    时候再看它还在不在 —— 迭代中 `delete` 掉的 key 不再产出，迭代中新增的
//!    key 不会被访问。
//! 2. **defer 边界**：命名返回值 / 嵌套倒序 / panic 展开 / recover 同帧。
//!    这四条 M5 就实现了，这里是**判据**（Task 50：实现不动，除非判据揭出真错）。
//! 3. **comma-ok 接收**（`rgoc-check` 侧）：`v, ok := <-c` 算两个值。

use std::path::PathBuf;

use rgoc_driver::cli::{RunCommand, Sched, Via};
use rgoc_driver::run_cmd::{RunOutcome, run_run};

fn run_src(name: &str, src: &str) -> RunOutcome {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/con-misc");
    std::fs::create_dir_all(&dir).expect("建不了 target/tmp/con-misc");
    let path = dir.join(format!("{name}.go"));
    std::fs::write(&path, src).expect("写不了 fixture");
    run_run(&RunCommand {
        files: vec![path],
        dump_hir: false,
        max_steps: Some(200_000),
        lang: None,
        via: Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
        sched: Sched::default(),
    })
    .expect("内部错（`run_run` 返回 Err）")
}

#[track_caller]
fn out(name: &str, src: &str) -> String {
    let o = run_src(name, src);
    assert_eq!(o.exit, 0, "{name}: 退出码 {}，stderr={}", o.exit, o.stderr);
    assert!(o.stderr.is_empty(), "{name}: stderr 非空：{}", o.stderr);
    o.stdout.replace("\r\n", "\n")
}

// ───────────────────────── Task 49：map 迭代快照 ─────────────────────────

#[test]
fn t_con_49_迭代中删除的_key_不再产出() {
    let s = out(
        "t_con_49",
        r#"
package main

func main() {
	m := map[string]int{"a": 1, "b": 2, "c": 3}
	for k := range m {
		if k == "b" {
			delete(m, "c")
		}
		println(k)
	}
}
"#,
    );
    // 快照是 key 列表；`c` 在还没轮到它时就被删掉 ⇒ 不产出。
    // 顺序是插入序（我们的 map 是可复现的插入序，官方是随机的 —— 这里是
    // 自家 fixture，判的是「删了的不出现」，不是顺序）。
    assert_eq!(s, "a\nb\n");
}

#[test]
fn t_con_50_迭代中新增的_key_不被访问() {
    let s = out(
        "t_con_50",
        r#"
package main

func main() {
	m := map[string]int{"a": 1}
	for k := range m {
		println(k)
		m["z"] = 9
	}
}
"#,
    );
    assert_eq!(s, "a\n", "迭代中新增的 key 不该被访问");
}

#[test]
fn t_con_50b_迭代中改的值是现读的() {
    let s = out(
        "t_con_50b",
        r#"
package main

func main() {
	m := map[string]int{"a": 1, "b": 2}
	for k, v := range m {
		println(k, v)
		m[k] = v * 10
	}
}
"#,
    );
    assert_eq!(s, "a 1\nb 2\n", "每个 key 只产出一次，读到的是当时的旧值");
}

// ───────────────────────── Task 50：defer 边界 ─────────────────────────

#[test]
fn t_con_51_defer_能改命名返回值() {
    let s = out(
        "t_con_51a",
        r#"
package main

func f() (n int) {
	defer func() { n = 99 }()
	return 1
}

func main() {
	println(f())
}
"#,
    );
    assert_eq!(s, "99\n", "defer 在 return 之后、函数真正返回之前跑");
}

#[test]
fn t_con_51_defer_嵌套倒序() {
    let s = out(
        "t_con_51b",
        r#"
package main

func main() {
	defer println("1")
	defer println("2")
	defer println("3")
}
"#,
    );
    assert_eq!(s, "3\n2\n1\n");
}

#[test]
fn t_con_51_panic_展开时_defer_照跑() {
    let o = run_src(
        "t_con_51c",
        r#"
package main

func f() {
	defer println("deferred")
	println("before")
	panic("x")
}

func main() {
	f()
}
"#,
    );
    // panic 没人接 ⇒ 退出 2，但 **defer 照跑**（这才是这条判据的重点）。
    assert_eq!(o.exit, 2);
    assert_eq!(
        o.stdout.replace("\r\n", "\n"),
        "before\ndeferred\npanic: x\n"
    );
}

#[test]
fn t_con_51_recover_必须在同一个帧() {
    // `recover` 只拦得住**当前帧**的 panic；在 defer 里调用它，拦的就是
    // 正在展开的那个 panic。放在下一帧就晚了。
    let s = out(
        "t_con_51d",
        r#"
package main

func f() (ok bool) {
	defer func() {
		if recover() != nil {
			ok = true
		}
	}()
	panic("boom")
}

func main() {
	println(f())
}
"#,
    );
    assert_eq!(s, "true\n");
}
