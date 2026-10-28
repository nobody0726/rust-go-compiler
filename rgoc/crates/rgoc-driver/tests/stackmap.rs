//! `STACK-MAP v1` 的两方对账（`T-ROOT-01/02/03/08`；门禁 `ROOT-1`）。
//!
//! # 这一条要回答什么
//!
//! `M9-design.md` §3.11 承诺了两份**同源但不同形**的栈图：
//!
//! | 形态 | 谁消费 | 形状 |
//! |---|---|---|
//! | 并列产物 `<out>.stackmap` | 人 / 调试 / 文档 | 逐行的 `STACK-MAP v1` 文本 |
//! | `.rodata` 的 `RtFuncInfo` 表 | M10 的栈扫描器 | `{frame_size, n_sp, sp_off*, sp_bits*}` |
//!
//! 两份都由 `module::stack_map_of` 算出来，所以「它们一致」在**当前实现里**
//! 是构造性的 —— 但这句话不是判据：发射那一层（`.long` / `.quad` 的排布、
//! 字序、`off` 的单位）完全可能各自出错。所以这里**解析 `.s` 的 `.rodata`**，
//! 再与文本逐字节比。
//!
//! # 为什么读 `.s` 而不是读 ELF
//!
//! 与 `desc_emit.rs` 同一个理由：`.s` 是那些字节的唯一来源，错了要能一眼看出
//! 是哪一行错。汇编器有没有忠实搬运由 `T-NAT-*` 与 M10 的扫描器负责。
//!
//! # 两条被这一条钉住的协议细节
//!
//! * **`off=<n>` 的单位**（`R-9-33`）：第 `n` 个 safepoint 的位图在 `sp_bits`
//!   里的**起始字下标**（不是字节、不是位）。两侧同一个单位，文本上就能直接对。
//! * **位点写 `b<块>:<下标>`**（`R-9-34`）：只写块内下标时，不同块的同一下标
//!   无法区分 —— 而这条测试必须能定位到**那一条指令**。
//!
//! # 分母
//!
//! 69 个入选语料（`tools/run-oracle/run-selected.txt`）。函数数与 safepoint
//! 总数都写出来：空集合上的全称命题不是证据（`M4` 的教训）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rgoc_check::CheckOptions;
use rgoc_load::load_with_imports;
use rgoc_ssa::build::build;
use rgoc_ssa::ir::Module;
use rgoc_types::TypeTable;

// ── 装置 ────────────────────────────────────────────────────────────

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达")
}

/// 入选清单的非注释非空行。
fn selected() -> Vec<String> {
    let path = repo_root().join("tools/run-oracle/run-selected.txt");
    let text = std::fs::read_to_string(&path).expect("读不到 run-selected.txt");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(ToString::to_string)
        .collect()
}

/// 把一个 Go 文件跑到 SSA 模块（`None` ⇒ 构建期拒绝；入选集里不该有）。
fn module_of(src: &Path) -> Option<(Module, TypeTable)> {
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, &[src.to_path_buf()], &[], None);
    let (checked, lowered) = rgoc_check::check::check_package_with(
        &pkg,
        &mut tt,
        bag,
        &CheckOptions { test_hooks: false },
        |s| rgoc_hir::lower(&pkg, s),
    );
    assert_eq!(
        checked.rendered_count(),
        0,
        "{} 不该有静态诊断",
        src.display()
    );
    let program = lowered.unwrap_or_else(|_| panic!("{} 被 lowering 拒绝", src.display()));
    let out = build(&program, &mut tt);
    if !out.rejected.is_empty() {
        return None;
    }
    Some((out.module, tt))
}

/// 同 [`module_of`]，但源码是字符串（写进 `target/stackmap` 下的临时文件）。
fn module_of_text(name: &str, src: &str) -> (Module, TypeTable) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/stackmap");
    std::fs::create_dir_all(&dir).expect("建目录失败");
    let go = dir.join(format!("{name}.go"));
    std::fs::write(&go, src).expect("写 fixture 失败");
    module_of(&go).unwrap_or_else(|| panic!("{name} 被构建期拒绝"))
}

/// 一个 safepoint 的记录（文本侧的形态）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct SpRec {
    /// 位点所在块。
    block: u32,
    /// 块内指令下标（`== instrs.len()` ⇒ 终止符）。
    idx: usize,
    /// 位图在 `sp_bits` 里的起始字下标。
    off: usize,
    /// 活值个数。
    live: u32,
    /// 位图（字）。
    words: Vec<u64>,
}

/// 一个函数的记录（文本侧的形态）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct FuncRec {
    /// `fn<FuncId>`。
    sym: String,
    /// 帧字节数。
    frame: u32,
    /// 头行自称的 safepoint 数。
    n_sp: usize,
    /// 逐 safepoint。
    sps: Vec<SpRec>,
}

/// 把 `ptrs=<hex>` 解成字。
///
/// 文本是「按字节下标升序直排」的十六进制（每字节两位），所以每 16 个字符
/// 是一个 `u64` 的 **LE** 字节序 —— 与 `hex_words` 互为逆运算。
fn decode_hex(s: &str) -> Vec<u64> {
    assert!(
        s.len().is_multiple_of(16),
        "位图十六进制长度 {} 不是 16 的倍数：{s:?}",
        s.len()
    );
    let bytes: Vec<u8> = (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("十六进制"))
        .collect();
    bytes
        .chunks(8)
        .map(|c| {
            let mut w = [0u8; 8];
            w.copy_from_slice(c);
            u64::from_le_bytes(w)
        })
        .collect()
}

/// 解析 `STACK-MAP v1` 文本。
fn parse_map(text: &str) -> BTreeMap<usize, FuncRec> {
    let mut out: BTreeMap<usize, FuncRec> = BTreeMap::new();
    let mut cur: Option<usize> = None;
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some(rgoc_arm64::module::STACKMAP_VERSION),
        "版本行必须是 `STACK-MAP v1`（改动等于契约变更）"
    );
    for line in lines {
        if let Some(rest) = line.strip_prefix("# func ") {
            // `<id> name=<sym> frame=<n> safepoints=<n>`
            let mut it = rest.split(' ');
            let idx: usize = it.next().expect("函数号").parse().expect("函数号是数字");
            let (mut sym, mut frame, mut nsp) = (None, None, None);
            for f in it {
                if let Some(v) = f.strip_prefix("name=") {
                    sym = Some(v.to_string());
                } else if let Some(v) = f.strip_prefix("frame=") {
                    frame = Some(v.parse::<u32>().expect("frame 是数字"));
                } else if let Some(v) = f.strip_prefix("safepoints=") {
                    nsp = Some(v.parse::<usize>().expect("safepoints 是数字"));
                }
            }
            out.insert(
                idx,
                FuncRec {
                    sym: sym.expect("缺 name="),
                    frame: frame.expect("缺 frame="),
                    n_sp: nsp.expect("缺 safepoints="),
                    sps: Vec::new(),
                },
            );
            cur = Some(idx);
            continue;
        }
        if let Some(rest) = line.strip_prefix("sp ") {
            let idx = cur.expect("`sp` 行必须在 `# func` 之后");
            let mut it = rest.split(' ');
            // `b<块>:<下标>`
            let pos = it.next().expect("位点");
            let (blk, ins) = pos.split_once(':').expect("位点要写 `b<块>:<下标>`");
            let block: u32 = blk
                .strip_prefix('b')
                .expect("块号以 b 开头")
                .parse()
                .expect("块号");
            let sp_idx: usize = ins.parse().expect("下标是数字");
            let (mut off, mut live, mut words) = (None, None, None);
            for f in it {
                if let Some(v) = f.strip_prefix("off=") {
                    off = Some(v.parse::<usize>().expect("off 是数字"));
                } else if let Some(v) = f.strip_prefix("live=") {
                    live = Some(v.parse::<u32>().expect("live 是数字"));
                } else if let Some(v) = f.strip_prefix("ptrs=") {
                    words = Some(decode_hex(v));
                }
            }
            out.get_mut(&idx).expect("函数记录存在").sps.push(SpRec {
                block,
                idx: sp_idx,
                off: off.expect("缺 off="),
                live: live.expect("缺 live="),
                words: words.expect("缺 ptrs="),
            });
            continue;
        }
        assert!(line.trim().is_empty(), "不认识的协议行：{line:?}");
    }
    for (idx, rec) in &out {
        assert_eq!(
            rec.n_sp,
            rec.sps.len(),
            "fn{idx}: 头行写 safepoints={} 但实到 {} 行",
            rec.n_sp,
            rec.sps.len()
        );
    }
    out
}

/// `.rodata` 里的 `RtFuncInfo`。
#[derive(Debug, Clone, PartialEq, Eq)]
struct InfoRec {
    frame: u32,
    n_sp: usize,
    offs: Vec<u32>,
    bits: Vec<u64>,
}

fn parse_rodata(asm: &str) -> BTreeMap<usize, InfoRec> {
    let lines: Vec<&str> = asm.lines().map(str::trim).collect();
    // 先收两张平行表：`.Lspoff<i>` / `.Lspbits<i>`
    let mut offs: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
    let mut bits: BTreeMap<usize, Vec<u64>> = BTreeMap::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(lbl) = line.strip_suffix(':') else {
            continue;
        };
        if let Some(k) = lbl.strip_prefix(".Lspoff") {
            let k: usize = k.parse().expect("偏移表下标");
            let mut v = Vec::new();
            let mut j = i + 1;
            while let Some(x) = lines.get(j).and_then(|l| l.strip_prefix(".long ")) {
                v.push(x.parse().expect(".long 是数字"));
                j += 1;
            }
            offs.insert(k, v);
        } else if let Some(k) = lbl.strip_prefix(".Lspbits") {
            let k: usize = k.parse().expect("位图流下标");
            let mut v = Vec::new();
            let mut j = i + 1;
            while let Some(x) = lines.get(j).and_then(|l| l.strip_prefix(".quad ")) {
                v.push(x.parse().expect(".quad 是数字"));
                j += 1;
            }
            bits.insert(k, v);
        }
    }
    // 再收 `__funcinfo_<i>`
    let mut out = BTreeMap::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(lbl) = line.strip_suffix(':') else {
            continue;
        };
        let Some(k) = lbl.strip_prefix("__funcinfo_") else {
            continue;
        };
        let k: usize = k.parse().expect("函数索引");
        let frame: u32 = lines[i + 1]
            .strip_prefix(".long ")
            .expect("frame_size 是 .long")
            .parse()
            .expect("frame_size 是数字");
        let n_sp: usize = lines[i + 2]
            .strip_prefix(".long ")
            .expect("n_sp 是 .long")
            .parse()
            .expect("n_sp 是数字");
        let (o, b) = if n_sp == 0 {
            assert_eq!(
                lines[i + 3],
                ".quad 0",
                "n_sp == 0 ⇒ sp_off 必须是 null（与 gcdata 同一条纪律）"
            );
            assert_eq!(lines[i + 4], ".quad 0", "n_sp == 0 ⇒ sp_bits 必须是 null");
            (Vec::new(), Vec::new())
        } else {
            assert_eq!(
                lines[i + 3],
                format!(".quad .Lspoff{k}"),
                "__funcinfo_{k} 的 sp_off 指向错表"
            );
            assert_eq!(
                lines[i + 4],
                format!(".quad .Lspbits{k}"),
                "__funcinfo_{k} 的 sp_bits 指向错表"
            );
            (
                offs.remove(&k).unwrap_or_else(|| panic!("缺 .Lspoff{k}")),
                bits.remove(&k).unwrap_or_else(|| panic!("缺 .Lspbits{k}")),
            )
        };
        out.insert(
            k,
            InfoRec {
                frame,
                n_sp,
                offs: o,
                bits: b,
            },
        );
    }
    // 「表有剩余」= 有一张位图没被任何 `__funcinfo_` 引用（死数据或下标算错）
    assert!(offs.is_empty(), "有 .Lspoff 表没被引用：{:?}", offs.keys());
    assert!(bits.is_empty(), "有 .Lspbits 表没被引用：{:?}", bits.keys());
    out
}

/// 一个文件的两侧 + 模块。
struct Built {
    map: BTreeMap<usize, FuncRec>,
    info: BTreeMap<usize, InfoRec>,
    module: Module,
    /// 每个函数的 `Call` 指令数（`T-ROOT-03` 的分母）。
    calls: Vec<usize>,
}

/// 编译一个文件并解析两侧。`None` ⇒ 构建期拒绝。
fn built(src: &Path) -> Option<Built> {
    let (module, tt) = module_of(src)?;
    let text = rgoc_arm64::module::stack_maps(&module, &tt).expect("栈图渲染");
    let asm = rgoc_arm64::module::compile(&module, &tt).expect("发射");
    let calls = module
        .funcs
        .iter()
        .map(|f| {
            f.blocks
                .iter()
                .flat_map(|b| b.instrs.iter())
                .filter(|i| matches!(i, rgoc_ssa::ir::Instr::Call { .. }))
                .count()
        })
        .collect();
    Some(Built {
        map: parse_map(&text),
        info: parse_rodata(&asm),
        module,
        calls,
    })
}

// ── T-ROOT-01：两侧逐字节对账 ────────────────────────────────────────

#[test]
fn t_root_01_stackmap_与_rodata_逐字节一致() {
    let root = repo_root();
    let mut bad: Vec<String> = Vec::new();
    let mut compared = 0usize;
    let mut nsp_total = 0usize;
    let mut funcs_total = 0usize;

    for rel in selected() {
        let src = root.join(&rel);
        let Some(b) = built(&src) else { continue };
        compared += 1;
        funcs_total += b.module.funcs.len();

        // ① 两侧的函数索引集合必须相同
        let a: Vec<usize> = b.map.keys().copied().collect();
        let c: Vec<usize> = b.info.keys().copied().collect();
        if a != c {
            bad.push(format!(
                "{rel}: 两侧函数索引不同\n  文本={a:?}\n  rodata={c:?}"
            ));
            continue;
        }
        for (idx, rec) in &b.map {
            let info = &b.info[idx];
            nsp_total += rec.sps.len();
            if rec.frame != info.frame {
                bad.push(format!(
                    "{rel} fn{idx}: frame 不同 文本={} rodata={}",
                    rec.frame, info.frame
                ));
                continue;
            }
            if rec.sps.len() != info.n_sp {
                bad.push(format!(
                    "{rel} fn{idx}: safepoint 数不同 文本={} rodata={}",
                    rec.sps.len(),
                    info.n_sp
                ));
                continue;
            }
            if rec.sym != format!("fn{}", b.module.funcs[*idx].src.0) {
                bad.push(format!(
                    "{rel} fn{idx}: name 与模块不符 文本={} 期望=fn{}",
                    rec.sym, b.module.funcs[*idx].src.0
                ));
            }
            // ② 并行表：`off` 与位图流
            let mut want_bits: Vec<u64> = Vec::new();
            for (k, sp) in rec.sps.iter().enumerate() {
                let want_off = u32::try_from(sp.off).expect("off 超 u32");
                if info.offs.get(k) != Some(&want_off) {
                    bad.push(format!(
                        "{rel} fn{idx} sp{k}: off 不同 文本={} rodata={:?}",
                        sp.off,
                        info.offs.get(k)
                    ));
                }
                want_bits.extend_from_slice(&sp.words);
            }
            if info.bits != want_bits {
                bad.push(format!(
                    "{rel} fn{idx}: 位图流不同\n  文本={want_bits:?}\n  rodata={:?}",
                    info.bits
                ));
            }
            // ③ `off` 的步长 = 一个位图几个字；宽度 = `(frame/8)` 位、每 64 位一个字
            let want = (rec.frame as usize / 8).div_ceil(64);
            for (k, sp) in rec.sps.iter().enumerate() {
                if sp.off != k * want {
                    bad.push(format!(
                        "{rel} fn{idx} sp{k}: off={} 应为 {}（frame={}）",
                        sp.off,
                        k * want,
                        rec.frame
                    ));
                }
                if sp.words.len() != want {
                    bad.push(format!(
                        "{rel} fn{idx} sp{k}: 位图 {} 个字，frame={} 应 {} 个",
                        sp.words.len(),
                        rec.frame,
                        want
                    ));
                }
            }
        }
    }

    assert!(bad.is_empty(), "两侧不一致：\n{}", bad.join("\n"));
    assert_eq!(compared, 69, "入选集应恰有 69 个文件能建图");
    assert!(funcs_total > 69, "函数数应多于文件数：{funcs_total}");
    assert!(
        nsp_total > funcs_total,
        "safepoint 总数应多于函数数（每个非空函数至少一条 `bl`）：{nsp_total} vs {funcs_total}"
    );
}

// ── T-ROOT-02 / 03：覆盖与下界 ──────────────────────────────────────

#[test]
fn t_root_02_每个函数都有帧记录() {
    let root = repo_root();
    let mut bad: Vec<String> = Vec::new();
    let mut files = 0usize;
    for rel in selected() {
        let Some(b) = built(&root.join(&rel)) else {
            continue;
        };
        files += 1;
        for (what, n) in [("文本", b.map.len()), ("rodata", b.info.len())] {
            if n != b.module.funcs.len() {
                bad.push(format!(
                    "{rel}: {what} {n} 条帧记录，模块 {} 个函数",
                    b.module.funcs.len()
                ));
            }
        }
        // 索引必须是 `0..n` 且连续（M10 按索引取表）
        let want: Vec<usize> = (0..b.module.funcs.len()).collect();
        if b.map.keys().copied().collect::<Vec<_>>() != want {
            bad.push(format!("{rel}: 文本的函数索引不是 0..n 连续"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    assert_eq!(files, 69, "分母应是 69 个文件");
}

#[test]
fn t_root_03_safepoint_数不少于调用点数() {
    let root = repo_root();
    let mut bad: Vec<String> = Vec::new();
    let mut files = 0usize;
    let mut calls_total = 0usize;
    for rel in selected() {
        let Some(b) = built(&root.join(&rel)) else {
            continue;
        };
        files += 1;
        for (idx, rec) in &b.map {
            calls_total += b.calls[*idx];
            if rec.sps.len() < b.calls[*idx] {
                bad.push(format!(
                    "{rel} fn{idx}: n_sp={} < Call 数={}",
                    rec.sps.len(),
                    b.calls[*idx]
                ));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    assert_eq!(files, 69);
    assert!(calls_total > 500, "语料里的调用总数应上百：{calls_total}");
}

// ── T-ROOT-08：帧内区间约定 ─────────────────────────────────────────

/// `FRAME-MAP v1` 里每个函数的 `(frame, 逐 safepoint 的 ptrs 偏移)`。
fn parse_frame_map_ptrs(text: &str) -> Vec<(u32, Vec<Vec<i32>>)> {
    let mut out: Vec<(u32, Vec<Vec<i32>>)> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# func ") {
            let frame = rest
                .split(' ')
                .find_map(|f| f.strip_prefix("frame="))
                .and_then(|v| v.parse::<u32>().ok())
                .expect("FRAME-MAP 头行要有 frame=");
            out.push((frame, Vec::new()));
            continue;
        }
        if let Some(rest) = line.strip_prefix("# safepoint ") {
            let ptrs = rest
                .split("ptrs=[")
                .nth(1)
                .and_then(|s| s.strip_suffix(']'))
                .expect("safepoint 行要有 ptrs=[...]");
            let offs: Vec<i32> = if ptrs.trim().is_empty() {
                Vec::new()
            } else {
                ptrs.split(',')
                    .map(|x| x.trim().parse().expect("偏移是整数"))
                    .collect()
            };
            out.last_mut().expect("先有 # func").1.push(offs);
        }
    }
    out
}

/// 第 `bit` 位是不是 1。
fn bit_of(words: &[u64], bit: usize) -> bool {
    (words[bit / 64] >> (bit % 64)) & 1 == 1
}

#[test]
fn t_root_08_帧内区间约定() {
    // 一个「有指针活值 + 有栈实参」的 fixture：`take` 的第 10 个实参（`string`）
    // 装不进寄存器 ⇒ 上栈。`p` 是指针活值（会被 `println` 读到 ⇒ 跨调用活着）。
    let src = r#"package main

func take(a, b, c, d, e, f, g, h, i int, s string) int { return a + i + len(s) }

func main() {
	p := new(int)
	*p = 1
	println(take(1, 2, 3, 4, 5, 6, 7, 8, 9, "xy"), *p)
}
"#;
    let (module, tt) = module_of_text("root_08", src);
    let map_text = rgoc_arm64::module::stack_maps(&module, &tt).expect("栈图");
    let asm = rgoc_arm64::module::compile(&module, &tt).expect("发射");
    let fmap = rgoc_arm64::module::frame_maps(&module, &tt).expect("帧图");
    let map = parse_map(&map_text);
    let info = parse_rodata(&asm);
    let fr = parse_frame_map_ptrs(&fmap);

    assert!(!map.is_empty(), "至少有一个函数");
    assert_eq!(map.len(), fr.len(), "帧图与栈图的函数数必须一致");

    for (idx, rec) in &map {
        let (frame, ptr_offss) = &fr[*idx];
        assert_eq!(*frame, rec.frame, "fn{idx}: 帧图 frame 与栈图不同");
        // 位宽：`frame` 是 16 的倍数 ⇒ 位图恰 `frame/64` 个字，位数 `frame/8`
        let total_bits = *frame as usize / 8;
        let want_words = total_bits.div_ceil(64);
        for (k, sp) in rec.sps.iter().enumerate() {
            assert_eq!(sp.words.len(), want_words, "fn{idx} sp{k}: 位图宽度");
            assert_eq!(sp.off, k * want_words, "fn{idx} sp{k}: off 的步长");
            assert_eq!(
                info[idx].n_sp,
                rec.sps.len(),
                "fn{idx}: 两侧 safepoint 数（rodata 那一侧）"
            );
            // ① 帧内低位那 16 字节（`[sp-16, sp)`）永远不是根：
            //    `frame_size = 16 + sub` 而 `sp = fp - sub` ⇒ 位 0..1 是死区。
            //    这是**位序**的牙：把 bit 0 当成 `fp-16`（而不是 `fp-frame_size`）
            //    就会在这里红。
            assert_eq!(
                sp.words[0] & 0b11,
                0,
                "fn{idx} sp{k}: 位 0/1 在 sp 之下，不该被标（frame={frame}）"
            );
            // ② saved fp/lr 在 `[fp-16, fp)` ⇒ 最高的两位永远是 0
            assert!(
                !bit_of(&sp.words, total_bits - 2),
                "fn{idx} sp{k}: saved fp 不是根"
            );
            assert!(
                !bit_of(&sp.words, total_bits - 1),
                "fn{idx} sp{k}: saved lr 不是根"
            );
            // ③ 帧图的 `ptrs`（值级根偏移）必须被栈图覆盖：位 `(frame+off)/8` 为 1。
            //    这是两份产物的**交叉**判据 —— 各自内部自洽还不够。
            for off in &ptr_offss[k] {
                let bit = ((*frame as i64 + i64::from(*off)) / 8) as usize;
                assert!(
                    bit < total_bits,
                    "fn{idx} sp{k}: ptrs 里的偏移 {off} 落到帧外（frame={frame}）"
                );
                assert!(
                    bit_of(&sp.words, bit),
                    "fn{idx} sp{k}: 帧图的 ptrs 偏移 {off}（位 {bit}）在栈图里没标上"
                );
            }
        }
    }

    // ④ outgoing 区：用 `call_stack_args`（与发射同一份）求期望位，再逐字对位图。
    //
    //    `take(int×9, string)`：第 9 个 `int` 与 `string` 都上栈 ——
    //    `[sp,#0)` = 第 9 个 int（**不是**根）、`[sp,#8)` = string 的 ptr（根）、
    //    `[sp,#16)` = string 的 len（**不是**根）。判据对每个栈实参的**每个字**
    //    都表态（标了该标的、也没标不该标的），所以「多标」同样会红。
    let siblings: BTreeMap<rgoc_hir::FuncId, &rgoc_ssa::ir::SsaFunc> =
        module.funcs.iter().map(|f| (f.src, f)).collect();
    let main_idx = module
        .main
        .and_then(|fid| module.funcs.iter().position(|f| f.src == fid))
        .expect("有 main");
    let rec = &map[&main_idx];
    let mut args_seen = 0usize;
    let mut roots_seen = 0usize;
    for sp in &rec.sps {
        let Some(ins) = module.funcs[main_idx]
            .blocks
            .get(sp.block as usize)
            .and_then(|b| b.instrs.get(sp.idx))
        else {
            continue;
        };
        let rgoc_ssa::ir::Instr::Call { callee, args, .. } = ins else {
            continue;
        };
        let (stack_args, _) = rgoc_arm64::frame::call_stack_args(
            &module.funcs[main_idx],
            &tt,
            &siblings,
            callee,
            args,
        );
        for (off, ty) in &stack_args {
            args_seen += 1;
            // 聚合按指针传 ⇒ 那一格是「指向调用方槽的指针」，不是它自己的位图。
            let is_agg = rgoc_arm64::abi::slot_of(&tt, *ty)
                .is_some_and(|s| s.class == rgoc_arm64::abi::SlotClass::Agg);
            let words: Vec<bool> = if is_agg {
                vec![true]
            } else {
                rgoc_types::layout::ptr_mask(&tt, *ty).unwrap_or_default()
            };
            for (w, want) in words.iter().enumerate() {
                // 位下标 = `(16 + off) / 8 + 字下标`（`sp = fp - (frame_size - 16)`）
                let bit = (16 + *off as usize) / 8 + w;
                let got = bit_of(&sp.words, bit);
                assert_eq!(
                    got, *want,
                    "fn{main_idx} b{}:{}：栈实参 off={off}（{ty:?}）的字 {w} \
                     （位 {bit}）标的对不对：{got} vs 应为 {want}\n{map_text}",
                    sp.block, sp.idx
                );
                if *want {
                    roots_seen += 1;
                }
            }
        }
    }
    assert!(
        args_seen > 0,
        "这个 fixture 必须真的产生栈实参（否则 ④ 是空断言）\n{map_text}"
    );
    assert!(
        roots_seen > 0,
        "栈实参里必须真的有根字（否则「标了」这一侧没有证据）\n{map_text}"
    );
}

// ── 产物落盘（`Task 59`）：`build` 之后两份都在，且行数对得上 ────────────

#[test]
fn t_root_01b_产物落盘与行数() {
    use rgoc_driver::cli::{BuildCommand, Emit};

    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/stackmap");
    std::fs::create_dir_all(&dir).expect("建目录失败");
    let src = r#"package main

func add(a, b int) int { return a + b }

func main() {
	p := new(int)
	for i := 0; i < 3; i++ {
		*p = add(*p, i)
	}
	println(*p)
}
"#;
    let go = dir.join("artifact.go");
    std::fs::write(&go, src).expect("写 fixture 失败");
    let out = dir.join("artifact");
    let cmd = BuildCommand {
        roots: Vec::new(),
        files: vec![go],
        output: Some(out.clone()),
        emit: Emit::Asm,
        keep_asm: true,
        opt: rgoc_ssa::opt::Config::none(),
        lang: None,
    };
    let outcome = rgoc_driver::build_cmd::run_build(&cmd).expect("build 失败");
    assert_eq!(outcome.exit, 0, "构建应成功：{:?}", outcome.stderr);

    let map_path = out.with_extension("stackmap");
    let asm_path = out.with_extension("s");
    assert!(map_path.exists(), "`{}` 必须落盘", map_path.display());
    assert!(asm_path.exists(), "`{}` 必须落盘", asm_path.display());
    // `FRAME-MAP` 仍然只在 `--keep-asm` 时留（它是中间件，不是交付物）
    assert!(
        out.with_extension("framemap").exists(),
        "--keep-asm 时应留帧图"
    );

    let text = std::fs::read_to_string(&map_path).expect("读栈图失败");
    let map = parse_map(&text);
    let asm = std::fs::read_to_string(&asm_path).expect("读 .s 失败");
    let info = parse_rodata(&asm);
    assert_eq!(map.len(), info.len(), "两侧函数数");

    // 行数 == 1（版本行）+ Σ(1 + n_sp)
    let want: usize = 1 + map.values().map(|r| 1 + r.sps.len()).sum::<usize>();
    assert_eq!(
        text.lines().count(),
        want,
        "`.stackmap` 的行数应恰为 1 + Σ(1 + n_sp)：\n{text}"
    );
    // 这个 fixture 必须真的同时有「调用」与「回边」两种 safepoint，
    // 否则上面那条行数判据只是数了个空集合。
    //
    // 「回边位点」的判据：`idx == 该块 instrs.len()`（终止符），且该块有目标
    // 支配自己。用模块自己算 —— 光看文本分不出「终止符位点」与「恰好等于
    // 指令数的指令位点」（那本来就是同一个数）。
    let (module, _tt) = module_of_text("artifact", src);
    let by_id: BTreeMap<u32, &rgoc_ssa::ir::SsaFunc> =
        module.funcs.iter().map(|f| (f.src.0, f)).collect();
    let mut has_loop_sp = false;
    for (idx, rec) in &map {
        assert_eq!(rec.sym, format!("fn{}", module.funcs[*idx].src.0));
        let f = by_id[&module.funcs[*idx].src.0];
        for sp in &rec.sps {
            if sp.idx == f.blocks[sp.block as usize].instrs.len() {
                has_loop_sp = true;
            }
        }
    }
    assert!(
        has_loop_sp,
        "fixture 应产生回边 safepoint（终止符位点）：\n{text}"
    );
}
