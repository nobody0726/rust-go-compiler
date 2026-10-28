//! 全局 roots 表（M9/Task 8，`T-ROOT-07`）。
//!
//! # 这张表给谁用
//!
//! M10 的扫描器在 STW 时要扫**所有**根：栈（`STACK-MAP`）、全局（本表）、
//! 寄存器（M9 不需要，见设计 §2.2 的「红利」）。全局漏了 ⇒ 只有全局引用的
//! 对象被回收 ⇒ 症状是「跑一会儿数据自己变了」。
//!
//! # 判据怎么读
//!
//! 表是 `.data` 里的两列 `(desc, addr)`，加一个计数符号。这里从 `.s` 里读它，
//! 并与同一份 `.s` 里的 desc 块**互相印证**（desc 的 size/kind 是另一处独立
//! 算出来的）—— 只看「有 3 条」会漏掉「desc 指错了」。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit};

fn scratch() -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/global-roots");
    std::fs::create_dir_all(&d).expect("建目录失败");
    d
}

fn build_asm(name: &str, src: &str) -> String {
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
    std::fs::read_to_string(exe.with_extension("s")).expect("读 .s 失败")
}

/// desc 符号 → `(size, kind)`。
fn descs(asm: &str) -> BTreeMap<String, (u32, u8)> {
    let lines: Vec<&str> = asm.lines().map(str::trim).collect();
    let mut out = BTreeMap::new();
    for i in 0..lines.len() {
        let Some(sym) = lines[i].strip_suffix(':') else {
            continue;
        };
        if !sym.starts_with("__desc_") {
            continue;
        }
        let body = lines[i + 1];
        let Some(rest) = body.strip_prefix(".byte ") else {
            continue;
        };
        let b: Vec<u8> = rest
            .split(',')
            .map(|s| s.trim().parse().expect("字节"))
            .collect();
        let size = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        out.insert(sym.to_string(), (size, b[12]));
    }
    out
}

/// `rt_globals` 的 `(desc 符号, 地址符号)` 列表 + `rt_nglobals` 的值。
fn roots(asm: &str) -> (Vec<(String, String)>, u64) {
    let lines: Vec<&str> = asm.lines().map(str::trim).collect();
    let mut pairs = Vec::new();
    let mut n = None;
    for i in 0..lines.len() {
        if lines[i] == "rt_globals:" {
            let mut j = i + 1;
            while j + 1 < lines.len() && lines[j].starts_with(".quad ") {
                let desc = lines[j].trim_start_matches(".quad ").to_string();
                let addr = lines[j + 1].trim_start_matches(".quad ").to_string();
                assert!(
                    lines[j + 1].starts_with(".quad "),
                    "rt_globals 必须是 (desc, addr) 成对，第 {j} 行是：{}",
                    lines[j + 1]
                );
                pairs.push((desc, addr));
                j += 2;
            }
        }
        if lines[i] == "rt_nglobals:" {
            let v = lines[i + 1].trim_start_matches(".quad ");
            n = Some(v.parse::<u64>().expect("rt_nglobals 不是整数"));
        }
    }
    (pairs, n.expect("没有 rt_nglobals 符号"))
}

#[test]
fn 三个包级变量进表且_desc_对得上() {
    let asm = build_asm(
        "t_global_roots",
        r#"
package main

var a int
var b string
var c *int

func main() {
	c = &a
	b = "hi"
	println(a, b, *c)
}
"#,
    );
    let d = descs(&asm);
    let (pairs, n) = roots(&asm);
    assert_eq!(n, 3, "三个包级变量都要进表：{pairs:?}");
    assert_eq!(pairs.len(), 3, "条目数与 rt_nglobals 必须一致");

    // 第 1 条：int（size 8 / kind 2）
    let (sym, addr) = &pairs[0];
    assert_eq!(
        d.get(sym).copied(),
        Some((8, 2)),
        "a 的 desc 该是 int（8 字节 / Kind::Int），实际 {sym} ⇒ {:?}",
        d.get(sym)
    );
    assert!(addr.starts_with('g'), "addr 该是全局的标签，实际 {addr}");

    // 第 2 条：string（size 16 / kind 24 —— 官方 27 档，M12）
    let (sym, _) = &pairs[1];
    assert_eq!(
        d.get(sym).copied(),
        Some((16, 24)),
        "b 的 desc 该是 string（16 字节 / Kind::String），实际 {sym} ⇒ {:?}",
        d.get(sym)
    );

    // 第 3 条：*int（size 8 / kind 22 —— 官方 27 档，M12）
    let (sym, _) = &pairs[2];
    assert_eq!(
        d.get(sym).copied(),
        Some((8, 22)),
        "c 的 desc 该是指针（8 字节 / Kind::Pointer），实际 {sym} ⇒ {:?}",
        d.get(sym)
    );
}

#[test]
fn 零大小全局不进表() {
    // 零大小全局别名到 `rt_zerobase`，**没有可扫描的字** —— 进表只会让 M10
    // 每次 STW 都去扫一块 0 字节的东西。
    let asm = build_asm(
        "t_global_roots_zero",
        r#"
package main

type Empty struct{}

var e Empty
var a int

func main() {
	a = 1
	println(a)
}
"#,
    );
    let (pairs, n) = roots(&asm);
    assert_eq!(n, 1, "只有 int 那个该进表：{pairs:?}");
    assert_eq!(pairs.len(), 1);
}

#[test]
fn 没有全局变量时符号仍在() {
    // `rt_globals` / `rt_nglobals` 必须**恒存在**：M10 的扫描器无条件读它们，
    // 少一个符号就是链接期 `undefined reference`，而空程序是最容易被漏的那类。
    let asm = build_asm(
        "t_global_roots_none",
        r#"
package main

func main() {
	println(1)
}
"#,
    );
    let (pairs, n) = roots(&asm);
    assert_eq!(n, 0);
    assert!(pairs.is_empty());
}
