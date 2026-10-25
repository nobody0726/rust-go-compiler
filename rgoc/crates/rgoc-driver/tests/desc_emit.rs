//! `.rodata` 里的 `TypeDesc` 表（M9/Task 5，`T-META-02` 的一半）。
//!
//! # 期望值从哪来
//!
//! 全部来自 `M9-design.md` §3.2 的**冻结字段集**与 `ptr_mask` 的形状，
//! 不是从实现里抄的：
//!
//! | 类型 | size | ptrdata | kind | flags |
//! |---|---|---|---|---|
//! | `int` | 8 | 0 | 2 (`Int`) | 1 (`COMPARABLE`) |
//! | `string` | 16 | 8 | 6 (`String`) | 7（可比较 + 含指针 + 要清零）|
//! | `*int` | 8 | 8 | 7 (`Pointer`) | 3（可比较 + 含指针）|
//! | `struct{a int; b string}` | 24 | 16 | 9 (`Struct`) | 7 |
//!
//! `ptrdata` 的口径：**最后一个为根的字之后**的字节数（`string` 是 8，
//! 因为字 0 是 ptr、字 1 是 len）。这与官方 `stringType.PtrBytes == 8` 一致。
//!
//! # 为什么读 `.s` 而不是读 ELF
//!
//! 这一条要验的是「发射出来的字节对不对」，而 `.s` 的 `.rodata` 就是那些字节的
//! 唯一来源。读 ELF 多一层 `readelf` 解析，收益是把「汇编器有没有忠实搬运」
//! 也纳入 —— 那件事由 `T-META-03`（Task 9 的 69 语料对撞）与 M10 的扫描器负责。
//! 这里刻意选窄的判据：**错了要能一眼看出是哪一字段错**。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit};

fn scratch() -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/desc-emit");
    std::fs::create_dir_all(&d).expect("建目录失败");
    d
}

/// 一个发射出来的 `TypeDesc`。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Desc {
    /// 符号名（`__desc_<hash>`）。
    sym: String,
    size: u32,
    ptrdata: u32,
    hash: u32,
    kind: u8,
    flags: u8,
    /// `gcdata` 指向的位图字节（`None` ⇒ `.quad 0`）。
    gcdata: Option<Vec<u8>>,
    /// `elem` 指向的符号（`None` ⇒ `.quad 0`）。
    elem: Option<String>,
}

/// 编一个程序，返回 `.s` 的全文。
fn build_asm(name: &str, src: &str) -> String {
    let dir = scratch();
    let go = dir.join(format!("{name}.go"));
    std::fs::write(&go, src).expect("写 fixture 失败");
    let exe = dir.join(name);
    let cmd = BuildCommand {
        files: vec![go],
        output: Some(exe.clone()),
        emit: Emit::Elf,
        keep_asm: true,
        opt: rgoc_ssa::opt::Config::none(),
        lang: None,
    };
    let out = build_cmd::run_build(&cmd).expect("build 不该失败");
    assert_eq!(out.exit, 0, "构建失败：{}", out.stderr);
    std::fs::read_to_string(exe.with_extension("s")).expect("读 .s 失败")
}

/// 解析 `.rodata` 里的 desc 块与 `gcdata` 位图。
///
/// 只认这一段的三种行：`.Lgc<N>:` / `__desc_<hash>:` / `.byte` / `.quad`。
/// 其余（`.p2align`）跳过。
fn parse_descs(asm: &str) -> Vec<Desc> {
    // 先收位图：任何 `.L…:` 标签后紧跟一行 `.byte a,b,c`（`__desc_` 除外）。
    let mut bitmaps: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let lines: Vec<&str> = asm.lines().collect();
    for i in 0..lines.len().saturating_sub(1) {
        let l = lines[i].trim();
        let Some(label) = l.strip_suffix(':') else {
            continue;
        };
        if !label.starts_with(".L") {
            continue;
        }
        let body = lines[i + 1].trim();
        if body.starts_with(".byte ") {
            bitmaps.insert(label.to_string(), parse_byte_list(body));
        }
    }

    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i].trim();
        if l.starts_with("__desc_") && l.ends_with(':') {
            let sym = l.trim_end_matches(':').to_string();
            let header = parse_byte_list(lines[i + 1].trim());
            assert_eq!(header.len(), 16, "{sym} 的头必须是 16 字节：{header:?}");
            let u32_at = |k: usize| {
                u32::from_le_bytes([header[k], header[k + 1], header[k + 2], header[k + 3]])
            };
            let quads: Vec<String> = (2..5).map(|k| lines[i + k].trim().to_string()).collect();
            let operand = |line: &str| -> Option<String> {
                let v = line.strip_prefix(".quad ")?.trim();
                if v == "0" { None } else { Some(v.to_string()) }
            };
            let gcdata = operand(&quads[0]).map(|l| {
                bitmaps
                    .get(&l)
                    .unwrap_or_else(|| panic!("{sym} 的 gcdata 指向不存在的位图 {l}"))
                    .clone()
            });
            // 符号名就是 `__desc_<hash>`，所以「头里的 hash 字段」与「符号名」
            // 必须自洽 —— 这条一行断言能抓住「hash 字段发成 0/截断/字节序错」
            // 一整类（`desc_layout` 的 177 个类型对撞也会抓，但那是语料级的；
            // 这里让单条 fixture 自己也能抓）。
            let got_hash = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
            assert_eq!(
                sym,
                format!("__desc_{got_hash:08x}"),
                "desc 头里的 hash 与符号名不一致"
            );
            out.push(Desc {
                sym,
                size: u32_at(0),
                ptrdata: u32_at(4),
                hash: u32_at(8),
                kind: header[12],
                flags: header[13],
                gcdata,
                elem: operand(&quads[1]),
            });
        }
        i += 1;
    }
    out
}

fn parse_byte_list(line: &str) -> Vec<u8> {
    let Some(rest) = line.strip_prefix(".byte ") else {
        panic!("期望 `.byte ...`，实际：{line}");
    };
    rest.split(',')
        .map(|s| s.trim().parse::<u8>().expect("字节不是 0-255"))
        .collect()
}

/// 按 `kind` 找 desc —— **不按符号名**：符号名含 hash，而 hash 的编码方式
/// 不属于本判据（它是 `T-META-05` 的事）。
fn by_kind(ds: &[Desc], kind: u8) -> Vec<&Desc> {
    ds.iter().filter(|d| d.kind == kind).collect()
}

#[test]
fn 结构体的_desc_与_layout_一致() {
    let asm = build_asm(
        "t_desc_struct",
        r#"
package main

type S struct {
	a int
	b string
}

var g S

func main() {
	g.b = "hi"
	println(g.a, g.b)
}
"#,
    );
    let ds = parse_descs(&asm);
    let st = by_kind(&ds, 9); // Kind::Struct
    assert_eq!(st.len(), 1, "应只有一个结构体 desc：{ds:#?}");
    let s = st[0];
    assert_eq!(s.size, 24, "S = int(8) + string(16)");
    assert_eq!(
        s.ptrdata, 16,
        "字 1 是 string 的 ptr ⇒ ptrdata 覆盖前 16 字节"
    );
    assert_eq!(
        s.flags, 7,
        "可比较 + 含指针 + 要清零（string 含指针且需清零）"
    );
    assert_eq!(
        s.gcdata.as_deref(),
        Some([0b10u8].as_slice()),
        "位图：字 0（int）不是根，字 1（string.ptr）是根"
    );
    assert!(s.elem.is_none(), "结构体没有 elem");
}

#[test]
fn 标量与指针的_desc() {
    let asm = build_asm(
        "t_desc_ptr",
        r#"
package main

var p *int

func main() {
	p = new(int)
	*p = 3
	println(*p)
}
"#,
    );
    let ds = parse_descs(&asm);

    let ints = by_kind(&ds, 2); // Kind::Int
    assert_eq!(ints.len(), 1, "int 只该有一块 desc（去重）：{ds:#?}");
    assert_eq!(ints[0].size, 8);
    assert_eq!(ints[0].ptrdata, 0, "int 没有根");
    assert_eq!(ints[0].flags, 1, "int 只有 COMPARABLE");
    assert!(ints[0].gcdata.is_none(), "ptrdata == 0 ⇒ gcdata 为 null");

    // 按 `elem` 找 `*int` —— **不按个数**：程序里还可能有别的指针类型
    // （`*bool` 之类由 `println` 的展开带出来），而本判据只关心 `*int`。
    let pint = by_kind(&ds, 7) // Kind::Pointer
        .into_iter()
        .find(|d| d.elem.as_deref() == Some(ints[0].sym.as_str()))
        .unwrap_or_else(|| panic!("找不到 elem 指向 int 的指针 desc：{ds:#?}"));
    assert_eq!(pint.size, 8);
    assert_eq!(pint.ptrdata, 8);
    assert_eq!(
        pint.flags, 7,
        "可比较 + 含指针 + 要清零（nil 是全零，但要明确）"
    );
    assert_eq!(
        pint.gcdata.as_deref(),
        Some([0b1u8].as_slice()),
        "指针的字 0 是根"
    );
}

#[test]
fn 同一类型只有一块_desc() {
    // `a int` / `b int` / `[3]int` 的元素 / `*int` 的 pointee 都是 `int`：
    // 四处在类型闭包里出现，只该发射**一块** desc。重复发射不会崩，
    // 但会让 `.rodata` 随「同一个类型被用几次」膨胀，且两块的地址不同 ⇒
    // 接口断言 `x.(int)` 在两条路径上会拿到不同的 desc 指针。
    let asm = build_asm(
        "t_desc_dedup",
        r#"
package main

type T struct {
	a int
	b int
	c [3]int
	d *int
}

var g T

func main() {
	g.d = new(int)
	println(g.a, g.b, g.c[0], *g.d)
}
"#,
    );
    let ds = parse_descs(&asm);
    let ints = by_kind(&ds, 2);
    assert_eq!(ints.len(), 1, "int 只该一块：{ds:#?}");
    // 每个 desc 的符号名必须唯一（hash 撞了就会重名，链接期重复定义）
    let mut syms: Vec<&str> = ds.iter().map(|d| d.sym.as_str()).collect();
    syms.sort_unstable();
    let mut uniq = syms.clone();
    uniq.dedup();
    assert_eq!(uniq.len(), syms.len(), "desc 符号名不许重复：{syms:?}");
}

#[test]
fn 发射是确定的() {
    let src = r#"
package main

type P struct {
	x int
	y string
}

var g P

func main() {
	g.y = "a"
	println(g.x, g.y)
}
"#;
    let a = build_asm("t_desc_det1", src);
    let b = build_asm("t_desc_det2", src);
    // 先确认真的有东西 —— 否则「两个空产物相同」也是绿的（空对空）。
    assert!(
        !parse_descs(&a).is_empty(),
        "这个 fixture 至少要发射一块 desc"
    );
    assert_eq!(a, b, "同一份源码两次构建的 .s 必须逐字节一致");
}
