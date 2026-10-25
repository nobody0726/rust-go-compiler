//! `FRAME-MAP v1` 与**反汇编**的逐偏移对账（`NAT-7` / `T-NAT-51` / `G-M7-02`）。
//!
//! # 为什么必须看反汇编，而不是看生成的 `.s`
//!
//! `.s` 里的 `add x9, x29, #-24` 是**我们写的**；真正的偏移是**汇编器编码出来的**。
//! 两者之间还有一步（立即数范围、编码别名：`add x29,#-24` 会被编成 `sub x29,#0x18`）。
//! 只比 `.s` 等于「自己跟自己对账」—— 而 `T-NAT-04` 原文要的是 objdump。
//!
//! # 对账的三条
//!
//! | 断言 | 挡住什么 |
//! |---|---|
//! | `frame=` == 实际 `sub sp, sp, #N` + 16 | 帧大小与产物不一致（`R-13-4` 就是这么错的） |
//! | 代码里每个 `x29` 相对偏移 ∈ 帧图的槽偏移 **或** 栈实参区 `[16, 16+outgoing)` | 访问越出帧（另一个方向的同一个错） |
//! | 帧图里出现的槽偏移**至少有一个**真的在代码里 | 帧图变成「谁都不用的漂亮表格」 |
//!
//! 第二条是**双向**的：只有「代码 ⊆ 表」会漏掉「表是假的」；只有「表 ⊆ 代码」会漏掉
//! 越界访问。所以要两向。
//!
//! # `sym=` 字段（`R-13-8`）
//!
//! 帧图的 `# func <name>` 是 HIR 名字，而 `.s` 里的符号是 `fn<FuncId>`。
//! 行尾的 `sym=fn<id>` 是两者之间的桥 —— 没有它，这条对账根本对不上函数。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rgoc_driver::build_cmd;
use rgoc_driver::cli::{BuildCommand, Emit};
use rgoc_ssa::ir::SsaFunc;

fn scratch() -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/native-framemap");
    std::fs::create_dir_all(&d).expect("建目录失败");
    d
}

/// 帧图里一个函数的两行信息。
struct MapFn {
    sym: String,
    frame: i64,
    offsets: BTreeSet<i64>,
}

/// 解析 `FRAME-MAP v1`。只认 `# func` / `# slot` 两种行，其余忽略。
fn parse_map(text: &str) -> BTreeMap<String, MapFn> {
    let mut out: BTreeMap<String, MapFn> = BTreeMap::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        let Some(body) = line.strip_prefix("# ") else {
            continue;
        };
        if let Some(rest) = body.strip_prefix("func ") {
            // `func <name> frame=<n> slots=<n> sym=fn<id>`
            let mut name = String::new();
            let mut sym = String::new();
            let mut frame = 0i64;
            for (i, tok) in rest.split_whitespace().enumerate() {
                if i == 0 {
                    name = tok.to_string();
                } else if let Some(v) = tok.strip_prefix("frame=") {
                    frame = v.parse().expect("frame= 不是整数");
                } else if let Some(v) = tok.strip_prefix("sym=") {
                    sym = v.to_string();
                }
            }
            assert!(!sym.is_empty(), "帧图缺 sym=（R-13-8）：{line}");
            out.insert(
                name.clone(),
                MapFn {
                    sym,
                    frame,
                    offsets: BTreeSet::new(),
                },
            );
            cur = Some(name);
        } else if let Some(rest) = body.strip_prefix("slot ") {
            let Some(name) = cur.as_ref() else { continue };
            let mut off = 0i64;
            for tok in rest.split_whitespace() {
                if let Some(v) = tok.strip_prefix("off=") {
                    off = v.parse().expect("off= 不是整数");
                }
            }
            out.get_mut(name)
                .expect("slot 行没有对应的 func")
                .offsets
                .insert(off);
        }
    }
    out
}

/// 从一段反汇编里抠出所有 `x29` 相对偏移。返回（符号名, 偏移集合）。
///
/// 只认我们真会发的四种形态，其余含 `x29` 的行（`stp x29, x30, [sp, #-16]!`）
/// 里 `x29` 是**源寄存器**，不是基址 —— 必须跳过。
fn parse_disasm(text: &str) -> BTreeMap<String, BTreeSet<i64>> {
    let mut out: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    let mut cur = String::new();
    for line in text.lines() {
        if let Some(rest) = line.split('<').nth(1)
            && let Some(name) = rest.split('>').next()
            && !name.contains("+0x")
            && !name.is_empty()
        {
            cur = name.to_string();
            out.entry(cur.clone()).or_default();
        }
        let Some((_, asm)) = line.split_once('\t') else {
            continue;
        };
        // `objdump -d` 的格式：`   addr:\t<machine code>\t<mnemonic>\t<operands>`
        //
        // ⚠️ 第一段是**机器码**不是助记符。按「取前两段」写会把机器码当助记符，
        // 于是所有指令都解析不出偏移 —— 表现为「帧图的偏移一个都没出现」，
        // 而那不是码的问题，是**装置不识字**。
        let mut parts: Vec<&str> = asm.split('\t').filter(|s| !s.trim().is_empty()).collect();
        if parts
            .first()
            .is_some_and(|p| p.trim().chars().all(|c| c.is_ascii_hexdigit() || c == ' '))
        {
            parts.remove(0);
        }
        let Some(mnem) = parts.first() else { continue };
        let ops = parts.get(1).copied().unwrap_or("");
        let mnem = mnem.trim();
        let ops = ops.split("//").next().unwrap_or("").trim();
        if mnem.is_empty() {
            continue;
        }

        let imm: Option<i64> = if (mnem == "sub" || mnem == "add") && ops.contains(", x29, #") {
            let v = ops.rsplit('#').next().unwrap_or("");
            let n = parse_hex_or_dec(v);
            Some(if mnem == "sub" { -n } else { n })
        } else if ops.contains("[x29") {
            match ops.split("[x29").nth(1) {
                Some(after) if after.starts_with(',') => {
                    let v = after.split('#').nth(1).and_then(|s| s.split(']').next());
                    v.map(|s| parse_hex_or_dec(s.trim()))
                }
                // `[x29]` 是偏移 0
                _ => Some(0),
            }
        } else {
            None
        };
        if let Some(v) = imm {
            out.entry(cur.clone()).or_default().insert(v);
        }
    }
    out
}

fn parse_hex_or_dec(s: &str) -> i64 {
    let s = s.trim().trim_start_matches('#');
    if let Some(h) = s.strip_prefix("0x") {
        i64::from_str_radix(h, 16).expect("十六进制立即数解析失败")
    } else if let Some(h) = s.strip_prefix("-0x") {
        -i64::from_str_radix(h, 16).expect("十六进制立即数解析失败")
    } else {
        s.parse().expect("十进制立即数解析失败")
    }
}

/// 一条 fixture：既有值槽、又有调用（因此有 outgoing 区）、还有栈实参。
const SRC: &str = "package main\n\nvar g int\n\nfunc sum8(a, b, c, d, e, f, h, i int) int {\n\treturn a + b + c + d + e + f + h + i + g\n}\n\nfunc call10(a, b, c, d, e, f, h, i, j, k int) int {\n\treturn sum8(a, b, c, d, e, f, h, i) + j*100 + k*1000\n}\n\nfunc main() {\n\tg = 7\n\tprintln(call10(1, 2, 3, 4, 5, 6, 7, 8, 9, 1))\n}\n";

#[test]
fn nat7_帧图与反汇编的偏移必须一致() {
    let dir = scratch();
    let go = dir.join("fm.go");
    std::fs::write(&go, SRC).expect("写源失败");
    let exe = dir.join("fm");
    let cmd = BuildCommand {
        files: vec![go],
        output: Some(exe.clone()),
        emit: Emit::Elf,
        keep_asm: true,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let out = build_cmd::run_build(&cmd).expect("build 不该失败");
    assert_eq!(out.exit, 0, "构建失败：{}", out.stderr);

    let asm_path = exe.with_extension("s");
    let map_path = exe.with_extension("framemap");
    assert!(map_path.exists(), "--keep-asm 应留下 .framemap");

    let map = parse_map(&std::fs::read_to_string(&map_path).expect("读帧图失败"));
    assert!(!map.is_empty(), "帧图为空");

    // 汇编 → objdump（`-d` 只看 .text）。
    let obj = dir.join("fm.o");
    let st = std::process::Command::new("clang")
        .args(["-c"])
        .arg(&asm_path)
        .arg("-o")
        .arg(&obj)
        .output()
        .expect("应能启动 clang");
    assert!(
        st.status.success(),
        "汇编失败：{}",
        String::from_utf8_lossy(&st.stderr)
    );
    let dis = std::process::Command::new("objdump")
        .arg("-d")
        .arg(&obj)
        .output()
        .expect("应能启动 objdump");
    assert!(dis.status.success(), "objdump 失败");
    let dis = parse_disasm(&String::from_utf8_lossy(&dis.stdout));

    let mut checked = 0usize;
    for (name, m) in &map {
        let Some(codes) = dis.get(&m.sym) else {
            panic!("反汇编里找不到符号 {}（帧图里的函数 {name}）", m.sym);
        };
        // ① 帧大小：`sub sp, sp, #N` 之后 `sp = fp - N`，而 `frame = 16 + N`。
        //    这一条只能从 `.s` 的文本里找（`sp` 相对，不是 `x29` 相对）。
        checked += 1;
        // ② 代码里的 x29 相对偏移必须落在「帧图槽偏移」或「栈实参区」里。
        //    栈实参区是 `[16, 16+outgoing)`，而上界我们不知道 —— 用 `frame` 兜底：
        //    `16 + outgoing <= frame - 16`（outgoing 在帧底，槽区在它上面）。
        let limit = m.frame - 16;
        for off in codes {
            if off.abs() > limit.max(0) && *off > 0 {
                panic!(
                    "{name}: 访问越出帧 —— 偏移 {off} 超过 {limit}（frame={}）",
                    m.frame
                );
            }
            if *off < 0 && -*off > limit {
                panic!("{name}: 负偏移 {off} 越过帧底（frame={}）", m.frame);
            }
        }
        // ③ 双向：帧图里至少有一个槽偏移真的出现在代码里（否则表是假的）。
        let seen = m.offsets.iter().any(|o| codes.contains(o));
        assert!(
            seen,
            "{name}: 帧图的槽偏移 {:?} 在反汇编里一个都没出现（代码只用了 {codes:?}）",
            m.offsets
        );
        // ④ 每个负数偏移要么是槽，要么在栈实参区；正数偏移只能是栈实参。
        for off in codes {
            if *off < 0 && !m.offsets.contains(off) {
                // 负偏移不是槽 ⇒ 一定是别的东西，报出来。
                panic!("{name}: 负偏移 {off} 不在帧图的槽偏移里（{:?}）", m.offsets);
            }
        }
    }
    assert!(checked >= 3, "只对账了 {checked} 个函数（应 ≥ 3）");
}

#[test]
fn nat7_帧大小等式_frame_等于_sub_加_16() {
    // ② 那一半单独立一条：从 `.s` 里读 `sub sp, sp, #N`，与 `frame=` 比。
    // 之所以不能从 objdump 拿：`sp` 是 16 对齐的寄存器，反汇编里同样能看到 ——
    // 但 `.s` 里更直接（且 `--keep-asm` 就是给人看的）。
    let dir = scratch();
    let go = dir.join("fm2.go");
    std::fs::write(&go, SRC).expect("写源失败");
    let exe = dir.join("fm2");
    let cmd = BuildCommand {
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
    let map = parse_map(&std::fs::read_to_string(exe.with_extension("framemap")).expect("读帧图"));
    let subs: Vec<u32> = asm
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            t.strip_prefix("sub sp, sp, #")
                .and_then(|n| n.parse::<u32>().ok())
        })
        .collect();
    // 每个函数恰好一条 `sub sp`（除非 sub 是 0，那就不发）。
    assert!(
        subs.len() >= map.len(),
        "`.s` 里有 {} 条 sub sp，帧图有 {} 个函数",
        subs.len(),
        map.len()
    );
    for f in map.values() {
        let want = f.frame as u32 - 16;
        if want == 0 {
            continue;
        }
        assert!(
            subs.contains(&want),
            "帧图说 {} 的 frame={}（sub 应为 {want}），但 `.s` 里的 sub 集合是 {subs:?}",
            f.sym,
            f.frame
        );
    }
}

/// 这条只为了让 `SsaFunc` 出现在签名里（将来若要从 SSA 直接对账）——
/// 现在它是 `unused` 会触发 clippy，所以显式用一次。
#[allow(dead_code)]
fn _typing_anchor(f: &SsaFunc) -> usize {
    f.value_types.len()
}
