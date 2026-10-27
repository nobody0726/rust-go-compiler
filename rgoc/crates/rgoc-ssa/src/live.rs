//! 值级活跃性 + `SSA-LIVE v1` 协议（`D-M6-10`；门禁 `SSA-8`）。
//!
//! # 这里到底算什么
//!
//! 三样东西：
//!
//! 1. **块级活跃集** `live_in` / `live_out`（含回边，迭代到不动点）；
//! 2. **值级区间** `[def, last_use]`（按「块号 + 指令下标」排序的位点）；
//! 3. **safepoint 的存活集合** —— 逐函数一段确定性文本（`SSA-LIVE v1`）。
//!
//! # 为什么 phi（块参数）要特殊处理
//!
//! 块参数 `Block.params` 是 phi 的容身处（`D-M6-3`）。它在**目标块入口**定义，
//! 实参在**前驱块的终止符**读。所以：
//!
//! ```text
//! live_out[p] = ⋃ over succ s: (live_in[s] − params(s)) ∪ edge_args(p→s)
//! ```
//!
//! 关键是**两块**：块参数算作「块入口已定义」（所以 `use` 里不含它、`live_in`
//! 也不含它），而前驱的出口要额外带上**边实参**（它们在跳转时被读走）。
//!
//! ⚠️ 上式里的 `− params(s)` 在当前 use/def 约定下是**冗余**的：块参数既然算
//! 「入口已定义」，`live_in[s]` 里就永远不会有它。这一项留着是为了让公式本身
//! 自洽（而不是靠另一处的约定兜住），但它**不宣称有判据覆盖** —— 变异打不掉
//! 它。真正有判据的是「边实参进 `live_out`」与「块参数不进 `live_in`」。
//!
//! # safepoint 的定义
//!
//! **能调出去或能分配**的指令：`Call`（任何被调方 —— 用户函数、方法、内建）
//! 与 `Make`（`make` / `new`）。理由：这两类是指令流里唯一「别的代码可能跑起来」
//! 的点，也就是 GC 唯一可能发生的地方。`Load`/`Store` 不分配也不调用，不算。
//!
//! safepoint 的 `live` 是「**跨过这条指令还活着**的值」∪「这条指令自己读的值」
//! （传进调用的指针在调用期间当然还活着）。
//!
//! # 保守策略：派生指针 ⇒ 根也活（`D-M6-10`）
//!
//! M6 **不做对象级**分析。`q := &s.n` 是从根 `&s` 派生的指针：即使 `q` 之后
//! 只被 `Load` 用过、`&s` 本身看起来「没人用了」，只要 `q` 活着，`&s` 指向的
//! 那块存储就必须活着 —— 否则 `q` 会指向已经回收的存储。所以 safepoint 的
//! `live` 会**闭包**到根（`root_of`），并在文档里如实说明它是保守的：
//! 它可能多留一个值，但不会少留。
//!
//! 反过来说：`ptrs` 只收**指针类型**（`*T`）。切片与 `map` 的底层存储属于
//! M9 的对象模型，M6 没有它的表示 —— 把它们塞进 `ptrs` 是假承诺。

use std::collections::{BTreeMap, BTreeSet};

use rgoc_types::{TypeId, TypeTable};

use crate::cfg::Cfg;
use crate::ir::{BlockId, Instr, SsaFunc, ValueId};

/// 协议版本行（改动等于契约变更；`C10` 的 `SSA v1` 是另一份契约）。
pub const LIVE_VERSION: &str = "# SSA-LIVE v1";

/// 一个位点：`block` 块的第 `idx` 条指令之前。
///
/// `idx == -1` 表示**块入口**（块参数在这里定义）。用 `i32` 而不是 `usize`
/// 就是为了这个 -1 —— 造一个「比任何指令都小」的哨兵值，比给 `Pos` 加一个
/// `enum { Entry, Instr(usize) }` 更省事，且排序天然正确。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Pos {
    /// 块号。
    pub block: u32,
    /// 指令下标（`-1` = 块入口）。
    pub idx: i32,
}

impl Pos {
    /// 块入口。
    #[must_use]
    pub const fn entry(block: u32) -> Pos {
        Pos { block, idx: -1 }
    }

    /// 第 `idx` 条指令之前。
    #[must_use]
    pub const fn instr(block: u32, idx: usize) -> Pos {
        Pos {
            block,
            idx: idx as i32,
        }
    }
}

/// 一个值的活跃区间 `[def, last]`。
///
/// # 它不是「线性扫描寄存器分配能用」的区间
///
/// 位点是**位置序**（块号 + 指令下标），不是执行序。循环里「块 5 定义、块 3 用」
/// （回边）的区间会看起来反的 —— 这里用 `max` 兜住，不假装它是执行序。
/// 真正的线性扫描需要 M7 的循环嵌套信息。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Interval {
    /// 定义点。
    pub def: Pos,
    /// 最后一次使用（没有使用 ⇒ 等于 `def`）。
    pub last: Pos,
}

/// 一个 safepoint（能调出去 / 能分配的指令）。
#[derive(Clone, Debug)]
pub struct Safepoint {
    /// 所在块。
    pub block: BlockId,
    /// 指令下标（块内）。
    pub idx: usize,
    /// 存活的值（**已闭包到根**，见模块头）。
    pub live: Vec<ValueId>,
    /// `live` 里**至少有一个字是根**的那些（`ptrs ⊆ live`）。
    ///
    /// ⚠️ M9/Phase 6 起它的含义从「值是指针类型」放宽成「值含根字」：
    /// `[]T` / `string` / `interface` / 含指针的聚合都进来了。判据是
    /// `ptr_mask` 至少一位为真，与栈图（`STACK-MAP`）**同一个来源**。
    pub ptrs: Vec<ValueId>,
    /// 每个活值里**哪几个字**是根（`(值, 字下标)`，按值号、再按字下标升序）。
    ///
    /// 这是 `RM-2` 的修正（`M9-design.md` §3.11）：`ptrs` 只到「值」粒度，
    /// 而栈扫描器需要「字」粒度 —— `[]int` 的三个字里只有一个是指向堆的根，
    /// 另外两个是 `len` / `cap`（按根去追会把整数当地址，按值类型判又会漏根）。
    ///
    /// 来源是 `rgoc_types::layout::ptr_mask`（与 `TypeDesc.gcdata` 同一个函数），
    /// **不另写一套**「什么类型算含指针」的判据。
    pub ptr_words: Vec<(ValueId, u8)>,
}

/// 一个函数的活跃性结果。
#[derive(Clone, Debug, Default)]
pub struct Liveness {
    /// 每块的 `live_in`（`Vec` 下标即 `BlockId`）。
    pub live_in: Vec<BTreeSet<ValueId>>,
    /// 每块的 `live_out`。
    pub live_out: Vec<BTreeSet<ValueId>>,
    /// 值 → 区间（只含**有定义点**的值）。
    pub intervals: BTreeMap<ValueId, Interval>,
    /// safepoint（按 `(块号, 指令下标)` 升序）。
    pub safepoints: Vec<Safepoint>,
}

/// 算一个函数的活跃性。
#[must_use]
pub fn analyze(f: &SsaFunc, cfg: &Cfg, tt: &TypeTable) -> Liveness {
    let n = f.blocks.len();
    let mut use_b: Vec<BTreeSet<ValueId>> = vec![BTreeSet::new(); n];
    let mut def_b: Vec<BTreeSet<ValueId>> = vec![BTreeSet::new(); n];

    // ── ① 每块的 use / def
    //
    // `use` 的判据是「**读到之前**没被本块定义过」：块参数先算作已定义，
    // 然后按程序顺序走指令。这样 `v = v + 1`（不合法 SSA，但手写 IR 造得出）
    // 也不会把自己算成 use。
    for (bi, blk) in f.blocks.iter().enumerate() {
        let mut defined: BTreeSet<ValueId> = BTreeSet::new();
        for p in &blk.params {
            defined.insert(*p);
            def_b[bi].insert(*p);
        }
        for ins in &blk.instrs {
            for u in ins.uses() {
                if !defined.contains(&u) {
                    use_b[bi].insert(u);
                }
            }
            for d in ins.dsts() {
                defined.insert(d);
                def_b[bi].insert(d);
            }
        }
        // 终止符的读（含边实参）：同样按「本块已定义」过滤
        for u in blk.term.uses() {
            if !defined.contains(&u) {
                use_b[bi].insert(u);
            }
        }
    }

    // ── ② 不动点
    let mut live_in: Vec<BTreeSet<ValueId>> = vec![BTreeSet::new(); n];
    let mut live_out: Vec<BTreeSet<ValueId>> = vec![BTreeSet::new(); n];
    // 逆序起步：回边（→ 低块号）在前几轮就能把信息带上去，收敛快一些。
    // 这只是性能，不影响结果（不动点唯一）。
    let mut changed = true;
    while changed {
        changed = false;
        for bi in (0..n).rev() {
            let b = BlockId(bi as u32);
            let mut out: BTreeSet<ValueId> = BTreeSet::new();
            for s in cfg.succs(b) {
                // 目标块的块参数在**目标**定义，不算「从这里活着出去」
                let params: BTreeSet<ValueId> = f
                    .blocks
                    .get(s.index())
                    .map(|x| x.params.iter().copied().collect())
                    .unwrap_or_default();
                out.extend(live_in[s.index()].iter().filter(|v| !params.contains(v)));
                // 边实参在前驱的终止符里读走 ⇒ 它们也是「出去时活着的」
                out.extend(cfg.edge_args(b, *s).iter().copied());
            }
            let mut inn = use_b[bi].clone();
            inn.extend(out.iter().filter(|v| !def_b[bi].contains(v)).copied());
            if out != live_out[bi] || inn != live_in[bi] {
                live_out[bi] = out;
                live_in[bi] = inn;
                changed = true;
            }
        }
    }

    // ── ③ 区间
    let mut intervals: BTreeMap<ValueId, Interval> = BTreeMap::new();
    for (bi, blk) in f.blocks.iter().enumerate() {
        let b = bi as u32;
        for p in &blk.params {
            intervals.entry(*p).or_insert(Interval {
                def: Pos::entry(b),
                last: Pos::entry(b),
            });
        }
        for (ii, ins) in blk.instrs.iter().enumerate() {
            let pos = Pos::instr(b, ii);
            for d in ins.dsts() {
                // `entry`：重复定义（不合法 SSA）时保留**第一次**的定义点
                intervals.entry(d).or_insert(Interval {
                    def: pos,
                    last: pos,
                });
            }
            for u in ins.uses() {
                touch(&mut intervals, u, pos);
            }
        }
        let end = Pos::instr(b, blk.instrs.len());
        for u in blk.term.uses() {
            touch(&mut intervals, u, end);
        }
    }
    // 兜底：区间必须 `last >= def`（回边上「定义在后面的块、用在前面」）
    for iv in intervals.values_mut() {
        if iv.last < iv.def {
            iv.last = iv.def;
        }
    }

    // ── ④ safepoint
    let root_of = root_values(f);
    let mem_values = mem_tokens(f);
    let mut safepoints: Vec<Safepoint> = Vec::new();
    // 指令侧与终止符侧**共用**同一个构造函数：字粒度、根闭包、mem 令牌的排除
    // 都只有一处口径（`R-9-32` 的教训 —— 两处各写一遍就会有一处忘掉 mem）。
    let mk = |block: BlockId, idx: usize, set: BTreeSet<ValueId>| -> Safepoint {
        // 闭包到根（保守，见模块头）
        let closure: BTreeSet<ValueId> = set.iter().map(|v| root_of_of(&root_of, *v)).collect();
        let mut set = set;
        set.extend(closure);
        let live: Vec<ValueId> = set.into_iter().collect();
        // 字粒度（`R-9-31`）：逐值查 `ptr_mask`，把为真的字下标摊平。
        // `live` 已按值号升序 ⇒ `ptr_words` 天然是「值号、再字下标」升序，
        // 排序键不需要再排一遍（也就没有第二个排序口径）。
        let mut ptr_words: Vec<(ValueId, u8)> = Vec::new();
        for v in &live {
            // mem 令牌是「时间点」而不是对象地址：它的类型确实是 `*T`
            // （构建器用 `const 0 : *T` 起头，见 `Fb::mem_of`），
            // 但它**不是**一个可以回收的指针。不排掉它，位图里会
            // 混进一堆常量 0。
            if mem_values.contains(v) {
                continue;
            }
            let Some(t) = f.value_types.get(v.index()).copied() else {
                continue;
            };
            for w in root_words(tt, t) {
                ptr_words.push((*v, w));
            }
        }
        // `ptrs` 是它的「值级投影」：出现过的值去重，顺序仍按值号。
        let mut ptrs: Vec<ValueId> = Vec::new();
        for (v, _) in &ptr_words {
            if ptrs.last() != Some(v) {
                ptrs.push(*v);
            }
        }
        Safepoint {
            block,
            idx,
            live,
            ptrs,
            ptr_words,
        }
    };
    // 回边判据用**已有的**支配关系（`F16` / `dom.rs`）：边 `b → h` 是回边
    // ⟺ `h` 支配 `b`。不另写「谁是循环头」的识别 —— 两套判据迟早会分叉。
    let dom = crate::dom::DomTree::build(f, cfg);
    for (bi, blk) in f.blocks.iter().enumerate() {
        let b = BlockId(bi as u32);
        if !cfg.reachable(b) {
            continue;
        }
        // 反向扫：`after[i]` = 执行完第 i 条之后还活着的值
        let mut after: Vec<BTreeSet<ValueId>> = vec![BTreeSet::new(); blk.instrs.len()];
        // ⚠️ 种子必须是 `live_out ∪ 终止符的读`（`R-11-1`）。
        //
        // 终止符不是指令，`live_out` 里也没有它的操作数（`live_out` 只由**后继**的
        // `live_in` 与边实参构成）。所以只从 `live_out` 起步，会漏掉「只在本块终止符里
        // 被读」的值 —— 它们从定义点一直活到块尾，却在 `after[*]` 里整个消失。
        //
        // 后果不是「少一个数」：safepoint 的 `live` 会漏根，M10 拿它扫栈就会把
        // 还活着的对象当成垃圾回收掉。M6 的 5 条 liveness 测试没抓到它，
        // 因为那些用例的返回值/边实参**同时也被别的指令用过**。
        let mut cur = live_out[bi].clone();
        cur.extend(blk.term.uses());
        // 终止符位点的活集 = 这个种子本身（`live_out ∪ 终止符的读`）。
        // 反向扫会把它改成 `live_in`，所以要**先留一份**。
        let term_set = cur.clone();
        for ii in (0..blk.instrs.len()).rev() {
            after[ii] = cur.clone();
            for d in blk.instrs[ii].dsts() {
                cur.remove(&d);
            }
            cur.extend(blk.instrs[ii].uses());
        }
        for (ii, ins) in blk.instrs.iter().enumerate() {
            if !is_safepoint(ins, f, tt) {
                continue;
            }
            // 这条指令自己读的也算（传进调用的指针在调用期间活着）
            let mut set = after[ii].clone();
            set.extend(ins.uses());
            // ⚠️ **位图是 live-out 的保守超集，不是精确的 live-in**（`R-10-21`）。
            //
            // `after[ii]` 是 live-**out**，而 GC 是在指令**执行之前**跑的，所以
            // 本指令的 def 槽此刻还是旧值 —— 标它等于多标一个可能已死的块。
            //
            // # 为什么保留超集（`G-M10-06` 已查清，2026-10-27）
            //
            // 去掉 def（真 live-in）曾让 `defernil` / `print` / `nilptr2` /
            // `method5` 全部坏掉。**根因不在位图**，在 `rt_defer_push`：它用
            // `rt_arena_alloc` 分配 `DeferRec`，而那条路径走 `gc_alloc`，后者的
            // stress 钩子在**分配之前**跑 GC —— 此刻新记录还没挂上 `g_defer_top`，
            // `mark_defer` 看不到这个闭包；而 `Defer` 不是 safepoint，`sp_idx`
            // 还停在本位点。于是 `MakeClosure` 的 dst 就是闭包块的**唯一**根。
            // 超集恰好含它，所以 M10 一直没露 —— 那是巧合，不是保证。
            //
            // 修法：`rt_defer_push` 加 `gc_pause`（`R-10-24`，与 `R-10-23`
            // 同源）。修完之后真 live-in 也能跑绿（`scripts/g-m10-06-probe.md`
            // 有四个语料 × 四种组合的读数）。**但位图仍保留超集** —— 这是策略
            // 选择，不是给那个缺口打的补丁：多标 = 泄漏一轮，漏标 = 活对象被
            // 回收，两者代价不对等，保守优先（`C12` §4.4）。
            //
            // 代价是「扫到已死指针」会变成常态，所以探针分成两个：
            // `bad_ptr`（解析不到块首 / 块头非法 —— 真错）与
            // `bad_stale`（指向已回收的块 —— 保守位图的预期产物）。
            safepoints.push(mk(b, ii, set));
        }
        // 终止符也能是 safepoint：**回边**（`T-ROOT-04`）。
        //
        // 循环体里没有调用也没有分配时，`Call`/`Make` 一条都命中不了，于是
        // 整个循环里**没有任何位点**可以扫栈。可循环的存活时间只受迭代次数
        // 限制，别的线程（M11）随时可能在这条回边上把程序停下来 —— 那一瞬间
        // 栈上活着的对象必须能被描述出来，否则就被当垃圾回收掉。
        //
        // 位点写 `idx = instrs.len()`：与区间里的「块尾」同一个哨兵
        // （`Pos::instr(b, len)`），消费方不需要第二套约定。
        if cfg.succs(b).iter().any(|t| dom.dominates(*t, b)) {
            safepoints.push(mk(b, blk.instrs.len(), term_set));
        }
    }
    // 排序键 = `(块号, 指令下标)`：终止符位点的 `idx` 是 `instrs.len()`，
    // 天然排在本块所有指令位点之后；跨块要重排（上面是块内追加、再按块循环）。
    safepoints.sort_by_key(|s| (s.block.0, s.idx));

    Liveness {
        live_in,
        live_out,
        intervals,
        safepoints,
    }
}

/// 一条**指令**是不是 safepoint（能调出去 / 能分配）。
///
/// 终止符不由它判：回边是「图上的性质」（目标支配本块），见 `analyze` 的 ④ 末段。
/// 这条指令**会不会走到分配器**（`C12` §5.1 / `R-10-13`）。
///
/// # 为什么必须是**穷尽**匹配（不留 `_ => false`）
///
/// GC 只在分配点触发。所以「哪些指令是位点」必须与「哪些指令会分配」
/// **一一对应** —— 漏一条，那个点上的 GC 就没有位点，`sp_idx` 停在上一个
/// 位点上，扫到的是**那一刻**的活跃集，而不是**这一刻**的。
///
/// 这个错在 M10 里犯了三次，每次都是「漏了一条指令」，每次症状都不同：
///
/// | 漏的 | 症状 | 怎么发现的 |
/// |---|---|---|
/// | `Instr::Make`（DCE 层）| `heap=0`，stress 空跑 | 统计行 |
/// | `MakeClosure` / `Convert` / `Bin` | `interbasic` 的 `ia == ib` 成立 | stress 语料 |
/// | `Composite`（切片字面量）| 三个 slice 头指向同一个数组 | stress 语料 |
///
/// 所以这里**不留兜底**：新增一个 `Instr` 变体会编译失败，逼人写下判断。
/// 判据是 `live::t_root_07_每条指令都要被分类`（穷尽性由编译器保证，
/// 那条测的是「分类结果与 isel 的实际行为一致」）。
fn is_safepoint(i: &Instr, f: &SsaFunc, tt: &TypeTable) -> bool {
    match i {
        // ── 会分配 ⇒ 必须是位点 ──────────────────────────────────
        //
        // `Call`：用户函数、方法、内建。`append` 会 `rt_append_one` /
        // `rt_append_dots`（扩容时分配），`println` 会调出去。
        Instr::Call { .. } => true,
        // `make` / `new`：`rt_makeslice` / `rt_alloc`。
        Instr::Make { .. } => true,
        // 闭包：`rt_alloc_closure`。
        Instr::MakeClosure { .. } => true,
        // 复合字面量：**切片**的那种要 `rt_makeslice` 建底层数组
        // （数组/结构体的元素就长在值里，不分配）。
        Instr::Composite { typ, .. } => is_slice(tt, *typ),
        // 装箱：接口 ⇒ `rt_box`；`string` ⇒ `rt_i64_to_str` /
        // `rt_bytes2str` / `rt_runes2utf8`。四个都分配。
        Instr::Convert { typ, .. } => is_boxing_target(tt, *typ),
        // 字符串拼接 ⇒ `rt_strcat`（分配 raw 块）。
        Instr::Bin { op, lhs, .. } => {
            matches!(op, rgoc_hir::BinOp::Add) && is_string(tt, f.value_types[lhs.index()])
        }

        // ── 不分配 ⇒ 不是位点 ────────────────────────────────────
        //
        // `TypeAssert` 走 `rt_itab` / `rt_panic_assert`：可能 panic，但**不分配**
        // ⇒ 不会触发 GC ⇒ 不需要位点。
        //
        // `Bin` 的接口 `==`/`!=` 走 `rt_iface_eq`：同上，调出去但不分配。
        // 给它位点的代价是每个二元运算前多两条指令（`turing.go` 那种程序会
        // 多出几百条），而收益是零。
        Instr::Const { .. }
        | Instr::Copy { .. }
        | Instr::Un { .. }
        | Instr::AddrOf { .. }
        | Instr::Field { .. }
        | Instr::Index { .. }
        | Instr::Slice { .. }
        | Instr::Load { .. }
        | Instr::Store { .. }
        | Instr::FuncAddr { .. }
        | Instr::CapturePtr { .. }
        | Instr::Defer { .. }
        | Instr::TypeAssert { .. }
        | Instr::NilCheck { .. } => false,
    }
}

/// 这个类型是切片吗（含具名与别名）？
fn is_slice(tt: &TypeTable, t: TypeId) -> bool {
    if t.index() >= tt.len() {
        return false;
    }
    matches!(
        tt.type_of(rgoc_types::underlying(tt, t)),
        rgoc_types::typ::Type::Slice(_)
    )
}

/// 这个转换目标会不会**分配**（`R-10-13`）？
///
/// 只有两种：
/// * 接口 ⇒ `rt_box`（分配一块放被装箱的值）；
/// * `string` ⇒ `rt_i64_to_str` / `rt_bytes2str` / `rt_runes2utf8`（分配 raw 块）。
///
/// ⚠️ `Bin`（接口 `==` → `rt_iface_eq`）**不需要**：它调出去但不分配，
/// 而 GC 只在分配点触发 ⇒ 没有位点也不会漏根。少一个位点就少两条指令。
fn is_boxing_target(tt: &TypeTable, t: TypeId) -> bool {
    if t.index() >= tt.len() {
        return false;
    }
    matches!(
        tt.type_of(rgoc_types::underlying(tt, t)),
        rgoc_types::typ::Type::Interface(_)
            | rgoc_types::typ::Type::Basic(rgoc_types::BasicId::String)
    )
}

/// 这个类型是 `string` 吗（含具名与别名）？
fn is_string(tt: &TypeTable, t: TypeId) -> bool {
    if t.index() >= tt.len() {
        return false;
    }
    matches!(
        tt.type_of(rgoc_types::underlying(tt, t)),
        rgoc_types::typ::Type::Basic(rgoc_types::BasicId::String)
    )
}

fn touch(iv: &mut BTreeMap<ValueId, Interval>, v: ValueId, pos: Pos) {
    // 用了一个**没有定义点**的值（不合法 SSA）：不编造区间。
    // 这种图会被 `V1` 拒；活跃性只需要不 panic。
    if let Some(x) = iv.get_mut(&v)
        && pos > x.last
    {
        x.last = pos;
    }
}

/// `ValueId → 它的根值`（`AddrOf` 的结果）。
///
/// 只认**构造性**的派生链：`AddrOf` / `Field` / `Index` / `Copy` / `Convert`。
/// 与构建期的 `Fb::ptr_root` 同一个纪律（`D-M6-5`）：合并值、形参、调用结果
/// 一律「根就是它自己」—— 保守，但不猜。
fn root_values(f: &SsaFunc) -> BTreeMap<ValueId, ValueId> {
    let mut map: BTreeMap<ValueId, ValueId> = BTreeMap::new();
    for blk in &f.blocks {
        for ins in &blk.instrs {
            if let Instr::AddrOf { dst, .. } = ins {
                map.insert(*dst, *dst);
            }
        }
    }
    // 不动点：`Field`/`Index` 的底座可能在后面的块里（回边）或者链在 `Copy` 后面
    let mut changed = true;
    while changed {
        changed = false;
        for blk in &f.blocks {
            for ins in &blk.instrs {
                let (dst, base) = match ins {
                    Instr::Field { dst, base, .. } | Instr::Index { dst, base, .. } => {
                        (*dst, *base)
                    }
                    Instr::Copy { dst, src } => (*dst, *src),
                    Instr::Convert { dst, x, .. } => (*dst, *x),
                    _ => continue,
                };
                if let Some(r) = map.get(&base).copied()
                    && map.get(&dst).copied() != Some(r)
                {
                    map.insert(dst, r);
                    changed = true;
                }
            }
        }
    }
    map
}

/// 全部 **mem 令牌**（`Load`/`Store` 的 `mem` 操作数 + `Store` 的 `dst`）。
///
/// mem 值的静态类型是指针（`Fb::mem_of` 用 `const 0 : *T` 起头），但它的语义是
/// 「这块存储在第几个时间点」，不是「一个指向对象的指针」。`ptrs` 要排除它。
fn mem_tokens(f: &SsaFunc) -> BTreeSet<ValueId> {
    let mut out = BTreeSet::new();
    for blk in &f.blocks {
        for ins in &blk.instrs {
            match ins {
                Instr::Load { mem, .. } => {
                    out.insert(*mem);
                }
                Instr::Store { mem, dst, .. } => {
                    out.insert(*mem);
                    out.insert(*dst);
                }
                _ => {}
            }
        }
    }
    out
}

/// 根闭包用：没有记录的 ⇒ 自己是自己的根。
fn root_of_of(map: &BTreeMap<ValueId, ValueId>, v: ValueId) -> ValueId {
    map.get(&v).copied().unwrap_or(v)
}

/// 类型 `t` 的哪些字是根（`T-ROOT-05`）。
///
/// # 为什么不再用「值是指针类型」
///
/// 那个判据在 `[]T` 上**漏根**（`RM-2`）：切片的静态类型不是 `*T`，但字 0
/// 指向堆；在 `struct{p *int; n int}` 上又会把 `n` 当根（多标一位，不至于崩，
/// 但会让位图与 `gcdata` 对不上）。`ptr_mask` 是 `TypeDesc.gcdata` 的**同一个**
/// 函数（`T-META-04`），两侧因此不可能各按各的理解。
fn root_words(tt: &TypeTable, t: TypeId) -> Vec<u8> {
    if t.index() >= tt.len() {
        return Vec::new();
    }
    let Some(mask) = rgoc_types::layout::ptr_mask(tt, t) else {
        return Vec::new();
    };
    mask.iter()
        .enumerate()
        .filter(|(_, b)| **b)
        .map(|(i, _)| u8::try_from(i).unwrap_or(u8::MAX))
        .collect()
}

/// 渲染 `SSA-LIVE v1`（逐函数一段；排序键 = 块号 + 指令下标）。
///
/// ```text
/// # SSA-LIVE v1
/// # func f blocks=4 values=9
/// # safepoint b1 0 live=[v3,v5] ptrs=[v5]
/// # interval v3 def=b0:2 last=b1:0
/// ```
///
/// 空集合写 `[]`（与 `C10` 的 `params=[]` 同口径），不省略字段 —— 消费方不必
/// 猜「缺了是空还是没有」。块入口的定义点写 `def=b<k>:-1`。
#[must_use]
pub fn render(f: &SsaFunc, lv: &Liveness) -> String {
    let mut out = String::new();
    out.push_str(LIVE_VERSION);
    out.push('\n');
    out.push_str(&format!(
        "# func {} blocks={} values={}\n",
        f.name,
        f.block_count(),
        f.value_count()
    ));
    for sp in &lv.safepoints {
        out.push_str(&format!(
            "# safepoint b{} {} live={} ptrs={}\n",
            sp.block.0,
            sp.idx,
            vlist(&sp.live),
            vlist(&sp.ptrs)
        ));
    }
    // `BTreeMap` 的键序就是 `ValueId` 升序
    for (v, iv) in &lv.intervals {
        out.push_str(&format!(
            "# interval v{} def=b{}:{} last=b{}:{}\n",
            v.0, iv.def.block, iv.def.idx, iv.last.block, iv.last.idx
        ));
    }
    out
}

/// 渲染整个模块（每个函数一段，中间空行分隔）。
#[must_use]
pub fn render_module(m: &crate::ir::Module, tt: &TypeTable) -> String {
    let mut out = String::new();
    for f in &m.funcs {
        let cfg = Cfg::build(f);
        let lv = analyze(f, &cfg, tt);
        out.push_str(&render(f, &lv));
    }
    out
}

fn vlist(xs: &[ValueId]) -> String {
    let body: Vec<String> = xs.iter().map(|v| format!("v{}", v.0)).collect();
    format!("[{}]", body.join(","))
}
