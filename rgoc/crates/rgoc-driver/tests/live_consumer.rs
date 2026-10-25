//! `SSA-LIVE v1` 的**消费方桩**（`T-SSA-57`；门禁 `SSA-8`）。
//!
//! # 这一条到底在证明什么
//!
//! 「协议存在」不等于「协议可消费」。这一条扮演**下游消费者**（将来的寄存器分配 /
//! 栈图 / GC 根表）：只拿到一段文本，就要能构造出「每个 safepoint 的存活指针集合」。
//!
//! 所以本文件的解析器**只吃 `&str`** —— 签名里没有 `SsaFunc`、没有 `Cfg`、
//! 没有 `TypeTable`。这不是「顺手这么写」，而是判据的一部分：一旦解析器需要
//! IR 内部结构，协议就没资格叫「自描述」，`D-M6-10` 的「M6e 只到协议级」也就
//! 没有兑现。
//!
//! # 为什么跑真实语料
//!
//! 手写 fixture 只能证明「我想到的形态能解析」。真实语料（57 个 `N` 侧文件）
//! 会带来构建器实际产出的各种组合：多块、回边、方法调用、`append`、`make`、
//! 结构体字段…… 分母写出来（跑了多少个函数 / 多少个 safepoint），
//! 否则「全部解析成功」可能只是「一个都没跑」。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rgoc_check::CheckOptions;
use rgoc_load::load_with_imports;
use rgoc_ssa::build::build;
use rgoc_ssa::live;
use rgoc_types::TypeTable;

// ── 消费方（只认文本）────────────────────────────────────────────────

/// 一个值的区间（协议里的 `def` / `last`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Span {
    def: (u32, i32),
    last: (u32, i32),
}

/// 一个 safepoint 的存活集合。
#[derive(Clone, Debug)]
struct Safepoint {
    block: u32,
    idx: usize,
    live: BTreeSet<u32>,
    ptrs: BTreeSet<u32>,
}

/// 一个函数的协议内容。
#[derive(Clone, Debug)]
struct FuncLive {
    name: String,
    blocks: usize,
    values: usize,
    safepoints: Vec<Safepoint>,
    intervals: BTreeMap<u32, Span>,
}

/// 解析整段协议文本。
///
/// 失败一律 panic 并带上原文行 —— 消费方对格式的容忍度应该是**零**：
/// 悄悄跳过一行不认识的协议，等于把「协议变了但没人知道」当成通过。
fn consume(text: &str) -> Vec<FuncLive> {
    let mut out: Vec<FuncLive> = Vec::new();
    let mut cur: Option<FuncLive> = None;
    for line in text.lines() {
        if line == "# SSA-LIVE v1" {
            if let Some(f) = cur.take() {
                out.push(f);
            }
            cur = Some(FuncLive {
                name: String::new(),
                blocks: 0,
                values: 0,
                safepoints: Vec::new(),
                intervals: BTreeMap::new(),
            });
            continue;
        }
        let Some(f) = cur.as_mut() else {
            panic!("协议正文出现在版本行之前：{line:?}");
        };
        if let Some(rest) = line.strip_prefix("# func ") {
            // `<name> blocks=<n> values=<n>`（名字里可能有空格？我们的名字没有，
            // 但 `T.M` 这种带点 —— 按空格切最后一个不行，用 rsplit 取两个字段）
            let mut parts: Vec<&str> = rest.split(' ').collect();
            let values = parts
                .pop()
                .and_then(|x| x.strip_prefix("values="))
                .and_then(|x| x.parse::<usize>().ok())
                .unwrap_or_else(|| panic!("头行缺 values：{line:?}"));
            let blocks = parts
                .pop()
                .and_then(|x| x.strip_prefix("blocks="))
                .and_then(|x| x.parse::<usize>().ok())
                .unwrap_or_else(|| panic!("头行缺 blocks：{line:?}"));
            f.name = parts.join(" ");
            f.blocks = blocks;
            f.values = values;
        } else if let Some(rest) = line.strip_prefix("# safepoint b") {
            let mut it = rest.split(' ');
            let block = it.next().unwrap().parse::<u32>().unwrap();
            let idx = it.next().unwrap().parse::<usize>().unwrap();
            let live = list(it.next().unwrap(), "live=");
            let ptrs = list(it.next().unwrap(), "ptrs=");
            f.safepoints.push(Safepoint {
                block,
                idx,
                live,
                ptrs,
            });
        } else if let Some(rest) = line.strip_prefix("# interval v") {
            let mut it = rest.split(' ');
            let v = it.next().unwrap().parse::<u32>().unwrap();
            let def = pos(it.next().unwrap());
            let last = pos(it.next().unwrap());
            f.intervals.insert(v, Span { def, last });
        } else {
            panic!("消费方不认识这一行：{line:?}");
        }
    }
    if let Some(f) = cur.take() {
        out.push(f);
    }
    out
}

fn list(s: &str, field: &str) -> BTreeSet<u32> {
    let body = s
        .strip_prefix(field)
        .and_then(|x| x.strip_prefix('['))
        .and_then(|x| x.strip_suffix(']'))
        .unwrap_or_else(|| panic!("{field} 不是列表：{s:?}"));
    if body.is_empty() {
        return BTreeSet::new();
    }
    body.split(',')
        .map(|x| {
            x.strip_prefix('v')
                .and_then(|n| n.parse::<u32>().ok())
                .unwrap_or_else(|| panic!("不是 v<k>：{x:?}"))
        })
        .collect()
}

fn pos(s: &str) -> (u32, i32) {
    let body = s
        .split_once('=')
        .unwrap_or_else(|| panic!("不是 k=v：{s:?}"))
        .1;
    let (b, i) = body
        .split_once(':')
        .unwrap_or_else(|| panic!("不是 b<i>:<j>：{body:?}"));
    (
        b.strip_prefix('b')
            .and_then(|x| x.parse::<u32>().ok())
            .unwrap_or_else(|| panic!("块号不是 b<i>：{b:?}")),
        i.parse::<i32>()
            .unwrap_or_else(|_| panic!("指令下标不是整数：{i:?}")),
    )
}

// ── 语料侧 ─────────────────────────────────────────────────────────

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达")
}

fn selected() -> Vec<String> {
    let path = repo_root().join("tools/run-oracle/run-selected.txt");
    let text = std::fs::read_to_string(&path).expect("读不到 run-selected.txt");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(ToString::to_string)
        .collect()
}

/// 跑一个文件，返回协议文本（`None` ⇒ 构建期拒绝）。
fn protocol_of(src: &Path) -> Option<String> {
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, &[src.to_path_buf()], &[], None);
    let opts = CheckOptions { test_hooks: false };
    let (checked, lowered) =
        rgoc_check::check::check_package_with(&pkg, &mut tt, bag, &opts, |s| {
            rgoc_hir::lower(&pkg, s)
        });
    assert_eq!(
        checked.rendered_count(),
        0,
        "{} 不该有静态诊断（入选集都是 M5 绿的）",
        src.display()
    );
    let program = lowered.unwrap_or_else(|_| panic!("{} 被 lowering 拒绝", src.display()));
    let out = build(&program, &mut tt);
    if !out.rejected.is_empty() {
        return None;
    }
    Some(live::render_module(&out.module, &tt))
}

#[test]
fn t_ssa_57_消费方桩能读全部入选集的协议() {
    let root = repo_root();
    let rows = selected();
    let mut funcs = 0usize;
    let mut safepoints = 0usize;
    let mut with_ptrs = 0usize;
    let mut live_vals = 0usize;
    let mut files = 0usize;
    let mut bad: Vec<String> = Vec::new();

    for rel in &rows {
        let src = root.join(rel);
        let Some(text) = protocol_of(&src) else {
            continue; // `M` 侧（23 个），由 `ssa_reject.rs` 负责
        };
        files += 1;
        let parsed = consume(&text);
        if parsed.is_empty() {
            bad.push(format!("{rel}: 协议里一个函数都没有"));
            continue;
        }
        for f in &parsed {
            funcs += 1;
            // ① 头行自洽：块数 ≥ 1、值数 == 区间条数（每个值都该有区间）
            if f.blocks == 0 {
                bad.push(format!("{rel}/{}: blocks=0", f.name));
            }
            if f.values != f.intervals.len() {
                bad.push(format!(
                    "{rel}/{}: values={} 但区间只有 {} 条",
                    f.name,
                    f.values,
                    f.intervals.len()
                ));
            }
            // ② 每个 safepoint：`ptrs ⊆ live`、块号在范围内、`live` 里的值都有区间
            for sp in &f.safepoints {
                safepoints += 1;
                live_vals += sp.live.len();
                if !sp.ptrs.is_subset(&sp.live) {
                    bad.push(format!(
                        "{rel}/{}: safepoint b{} {} 的 ptrs 不是 live 的子集",
                        f.name, sp.block, sp.idx
                    ));
                }
                if sp.block as usize >= f.blocks {
                    bad.push(format!(
                        "{rel}/{}: safepoint 块号 b{} 越界（blocks={}）",
                        f.name, sp.block, f.blocks
                    ));
                }
                for v in &sp.live {
                    if !f.intervals.contains_key(v) {
                        bad.push(format!(
                            "{rel}/{}: safepoint b{} {} 的 v{v} 没有区间",
                            f.name, sp.block, sp.idx
                        ));
                    }
                }
                if !sp.ptrs.is_empty() {
                    with_ptrs += 1;
                }
            }
            // ③ 区间自洽：`def ≤ last`，且两个位点都在块范围内
            for (v, s) in &f.intervals {
                if s.def > s.last {
                    bad.push(format!("{rel}/{}: v{v} 的区间反了", f.name));
                }
                if s.def.0 as usize >= f.blocks || s.last.0 as usize >= f.blocks {
                    bad.push(format!("{rel}/{}: v{v} 的区间块号越界", f.name));
                }
            }
        }
    }

    assert!(
        bad.is_empty(),
        "协议消费失败 {} 处（前 10 条）：\n{}",
        bad.len(),
        bad.iter().take(10).cloned().collect::<Vec<_>>().join("\n")
    );
    // ④ 分母写出来：空集合上的全称命题不是证据
    assert_eq!(files, 57, "入选集该有 57 个能建图的文件（N 侧冻结值）");
    assert!(funcs > 57, "函数数该多于文件数（每个文件至少一个函数）");
    assert!(
        safepoints > 0,
        "一个 safepoint 都没有 ⇒ 这条测试什么也没证明"
    );
    assert!(
        with_ptrs > 0,
        "一个「有存活指针」的 safepoint 都没有 ⇒ `ptrs` 这一列没被验过"
    );
    // 打印分母（`cargo test -- --nocapture` 时可见；数字变了这里不改也能过，
    // 但会立刻在输出里看到 —— 这些数字是「协议真的覆盖到真实代码」的规模感）
    println!(
        "SSA-LIVE 消费方：files={files} funcs={funcs} safepoints={safepoints} \
         有指针的 safepoint={with_ptrs} live 值总数={live_vals}"
    );
}
