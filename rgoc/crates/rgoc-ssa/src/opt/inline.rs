//! 内联（`T-OPT-01` / `G12-10`；`M12-plan.md` Task 37）。
//!
//! # 它做什么
//!
//! 把「直接调用一个**小而规矩**的函数」换成那个函数体：调用点所在块在 `Call`
//! 处**切开**，被调方的块整段搬进调用方，`Return` 改成跳到**续块**，续块守着
//! 原来的 `dsts`（它们从「`Call` 的结果」变成「续块的块参数」）。
//!
//! ```text
//! 之前：  b0: … v5 = call f#7(v3, v4) …      ; 后面还有 v6 = bin v5, …
//!         b0: return v6
//!
//! 之后：  b0: … goto b2(v3, v4)              ; b0 在 Call 处被切开
//!         b2: … (f 的入口块，形参就是 v3/v4)
//!         b3: … (f 的其它块)
//!         b4(v5): v6 = bin v5, …            ; 续块：参数 = 原来的 dsts
//!                 return v6
//! ```
//!
//! 这是**唯一**一个跨函数的 pass（其余四个都是单函数），所以：
//! * 它是 `Module` 级的，必须在逐函数循环之前跑；
//! * 它要重映射**四张表**（值 / 块 / 根 / 槽），漏一张就是静默错。
//!
//! # 为什么「切开+续块」而不是「把调用点原地展开」
//!
//! 原地展开要把 `Call` 之后的所有指令重编号（`dsts` 的定义位置从 `Call` 挪到
//! 被调方 `Return` 之前），而那段代码可能跨块 —— 那是「把整个函数重排一遍」。
//! 切开只需在调用点造一条边和一个块，**原来的块与指令一个字都不动**（除了
//! `Call` 本身被删、终止符换成 `Goto`）。
//!
//! 顺带解决了 `V6`：`mem` 令牌是按块查「同块上一次同根访存」的（`verify` 的
//! `V6` ②）。被调方的 `Store` 落在**新块**里，续块在 `V6` 眼里是一张干净的
//! 表 —— 否则「调用点之前是旧 mem、内联体把新 mem 推进了」会让同块后续的
//! `Load` 报「跳过了新 mem」。
//!
//! # mem 令牌为什么可以各走各的
//!
//! 被调方的每条 mem 链从它自己的种子（`const 0 : *u8`）起，内联之后原样搬过来；
//! 调用方的链**不动**。这在语义上是空的：`mem` 是**抽象令牌**，两个执行器
//! （`ssa_exec` 与 native）都按**程序顺序**访问同一份存储，令牌只用于
//! `V6` 的结构检查与 pass 的排序推理。这一点写在 `C10` 的 `mem` 一节与
//! `D-M12-8` 里；换成「按令牌版本化内存」的实现之前必须先改这两处。
//!
//! # 收尸
//!
//! 内联之后被调方可能**一次都不再被引用**（只有一个调用点、且它已经被搬走）。
//! 这时把它从 `Module::funcs` 里删掉，内联才真的**省体积** —— 否则调用方变长、
//! 被调方照旧发射，总量只增不减。
//!
//! 收尸的范围**刻意收窄**：只删「内联前有人引用、内联后一次都没有」的函数。
//! 本来就没人引用的函数（死代码）不归这个 pass 管 —— 那是另一个决定
//! （要不要保留未使用的函数，与内联无关），把两件事混在一个 pass 里，
//! 判据就没法分清了。
//!
//! # 哪些不内联
//!
//! 见 [`eligible`]：只认 `CalleeRef::Func`、只认**单层**（不自递归）、被调方
//! 不带 `ctx`/捕获/`defer`/`go`、块数与指令数在预算内。这些限制不是保守，
//! 是**正确性**：`defer` 登记在**当前帧**上（内联会把「被调方的 defer」变成
//! 「调用方的 defer」，退出时机变了）；`go` 的实参要深拷贝到新 G 的栈
//! （内联之后实参来自调用方帧，拷贝点没了）；`ctx` 是隐式第 0 实参
//! （内联之后没有 funcval 可传）。
//!
//! 还有两条同样是**正确性**，容易被当成「保守」：
//!
//! * **实参与返回值的类型必须逐个相同**（[`types_line_up`]）：`Call` 的实参
//!   类型不要求与形参相同（`V3` 不查），但内联之后它们变成**块的边**，而边上
//!   必须类型相同。`T.Len(t)`（具名切片型 vs `[]int`）就是这个坑。
//! * **把帧槽地址当返回值交出去的函数不内联**（[`returns_local_addr`]）：
//!   帧槽逃逸由后端的 `frame.rs::escaping_slots` 按**本函数的 `Return`** 判，
//!   内联之后那个 `Return` 没了 ⇒ 槽退回帧 ⇒ 多次调用共用一格存储。

use std::collections::{BTreeMap, BTreeSet};

use rgoc_hir::{FuncId, SlotId};
use rgoc_types::{TypeId, TypeTable};

use crate::ir::{Block, BlockId, CalleeRef, Instr, Module, Root, RootId, SsaFunc, Term, ValueId};
use crate::verify::{VerifyError, verify};

use super::{dsts_of_mut, instr_uses, term_uses};

/// 被调方最多几条指令（含全部块）。
pub const MAX_CALLEE_INSTRS: usize = 16;

/// 被调方最多几个块。
pub const MAX_CALLEE_BLOCKS: usize = 4;

/// 一个调用方最多吃进多少条内联指令（防止「互相调用链 + 多个调用点」指数膨胀）。
pub const MAX_INLINED_PER_FUNC: usize = 256;

/// 内联的读数（进 manifest 的 `measured`，也是判据的一半）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// 搬了几个调用点。
    pub sites: usize,
    /// 收了几具尸（内联之后没人引用的函数）。
    pub dropped: usize,
    /// 搬进调用方的指令总数。
    pub instrs: usize,
}

/// 跑内联。返回读数。
///
/// # Errors
///
/// 每个改动过的调用方**立刻** `verify`：内联是重映射四张表的操作，
/// 「图还合法」是它能被相信的唯一前提（`SSA-5`）。
pub fn run(m: &mut Module, tt: &TypeTable) -> Result<Stats, VerifyError> {
    let before = ref_counts(m);
    let mut st = Stats::default();

    for ci in 0..m.funcs.len() {
        let mut budget = MAX_INLINED_PER_FUNC;
        while let Some((bi, ii, cj)) = find_site(m, tt, ci) {
            let cost = instr_count(&m.funcs[cj]);
            if cost > budget {
                break;
            }
            budget -= cost;
            inline_one(m, ci, bi, ii, cj);
            st.sites += 1;
            st.instrs += cost;
            verify(&m.funcs[ci], tt)?;
        }
    }

    let after = ref_counts(m);
    st.dropped = drop_emptied(m, &before, &after);
    Ok(st)
}

/// 被调方的指令数（`Term` 也算一条：它会被换成 `Goto`）。
fn instr_count(f: &SsaFunc) -> usize {
    f.blocks.iter().map(|b| b.instrs.len() + 1).sum()
}

/// 在 `m.funcs[ci]` 里找**第一个**可内联的调用点，返回 `(块, 块内下标, 被调方下标)`。
fn find_site(m: &Module, tt: &TypeTable, ci: usize) -> Option<(usize, usize, usize)> {
    let caller = &m.funcs[ci];
    for (bi, b) in caller.blocks.iter().enumerate() {
        for (ii, ins) in b.instrs.iter().enumerate() {
            let Instr::Call {
                dsts,
                callee: CalleeRef::Func(fid),
                args,
                dots,
                ..
            } = ins
            else {
                continue;
            };
            if *dots {
                continue;
            }
            let Some(cj) = m.funcs.iter().position(|f| f.src == *fid) else {
                continue;
            };
            if cj == ci {
                continue;
            }
            let callee = &m.funcs[cj];
            if args.len() != callee.params.len() || !eligible(callee) {
                continue;
            }
            if !types_line_up(tt, caller, callee, args, dsts) {
                continue;
            }
            return Some((bi, ii, cj));
        }
    }
    None
}

/// 参数与返回值的类型**逐个相同**吗。
///
/// # 为什么这条是必需条件，不是优化
///
/// `Call` 的实参类型**不要求**与被调方形参相同：`V3` 不查它（`V5` 只查个数），
/// 因为表示层的差异由 ABI 落位管（`T.Len(t)` 这种「具名切片型 vs `[]int`」的
/// 实参就是如此）。但内联把调用点变成**块的边**，而边上的实参与目标块参数
/// **必须**类型相同（`V3` 的第三条子判据）。
///
/// 也就是说：类型不同的调用点一旦内联，图当场非法。这不是「可以事后补
/// `Convert`」的事 —— `Convert` 是**值转换**，而这些差异里有一部分只是表示层的
/// 重解释（补了反而改语义）。所以判据是「一拍两散」：类型对不上就不内联。
fn types_line_up(
    tt: &TypeTable,
    caller: &SsaFunc,
    callee: &SsaFunc,
    args: &[ValueId],
    dsts: &[ValueId],
) -> bool {
    let same = |a: TypeId, b: TypeId| crate::typing::same_type(tt, a, b);
    for (a, p) in args.iter().zip(&callee.params) {
        let (Some(ta), Some(tp)) = (
            caller.value_types.get(a.index()),
            callee.value_types.get(p.index()),
        ) else {
            return false;
        };
        if !same(*ta, *tp) {
            return false;
        }
    }
    for b in &callee.blocks {
        if let Term::Return(vs) = &b.term {
            if vs.len() != dsts.len() {
                return false;
            }
            for (v, d) in vs.iter().zip(dsts) {
                let (Some(tv), Some(td)) = (
                    callee.value_types.get(v.index()),
                    caller.value_types.get(d.index()),
                ) else {
                    return false;
                };
                if !same(*tv, *td) {
                    return false;
                }
            }
        }
    }
    true
}

/// 这个函数能当内联的被调方吗。
fn eligible(c: &SsaFunc) -> bool {
    if c.ctx_abi || !c.captured.is_empty() {
        return false;
    }
    if c.blocks.len() > MAX_CALLEE_BLOCKS || instr_count(c) > MAX_CALLEE_INSTRS {
        return false;
    }
    // 自递归：内联它 = 把调用点换成含它自己的体，`find_site` 会一直命中
    if c.call_sigs.contains_key(&c.src) {
        return false;
    }
    // 入口块的形参必须与 `params` 一致（`C10` 规则 2），否则边上的实参个数对不上
    if c.blocks.get(c.entry.index()).map(|b| &b.params) != Some(&c.params) {
        return false;
    }
    let mut returns = 0usize;
    for b in &c.blocks {
        for i in &b.instrs {
            match i {
                // 帧语义差别：`defer` 登记在当前帧、`go` 要深拷贝实参
                Instr::Defer { .. } | Instr::Go { .. } | Instr::CapturePtr { .. } => return false,
                _ => {}
            }
        }
        match &b.term {
            Term::Return(_) => returns += 1,
            Term::Goto { .. } | Term::If { .. } | Term::Panic(_) => {}
            // `Unreachable` 的块没有出边；若全函数都没有 `Return`，续块会变成死块（`V8`）
            Term::Unreachable => return false,
        }
    }
    if returns == 0 {
        return false;
    }
    // 把局部变量的地址当返回值交出去 ⇒ 不内联（见 `returns_local_addr`）
    !returns_local_addr(c)
}

/// 被调方**把自己的帧槽地址当返回值**交出去了吗。
///
/// # 为什么这一条是正确性，不是风格
///
/// 「返回 `&x` ⇒ `x` 必须逃逸到 arena」这条规则由**后端**的
/// `rgoc-arm64/src/frame.rs::escaping_slots` 判，而它的证据之一是**本函数的
/// `Term::Return`**。内联之后那个 `Return` 没了（变成了续块的块参数），槽于是
/// 退回帧里 —— 而帧槽是**一个函数一份**的：
///
/// ```text
/// for i := 0; i < 2; i++ { xs = append(xs, f()) }   // f() 返回 &T{}
/// ```
///
/// 两次 `f()` 会拿到**同一格**存储 ⇒ 两个指针相等、后一次覆盖前一次。
/// `compos.go` 就是拿 `x == y` 判这件事的（`heap=0` 是它的读数）。
///
/// 修法有两条：让「逃逸」这件事跟着内联走（要给 IR 加标记），或者干脆不内联
/// 这类函数。选后者 —— 判据简单、错误方向安全（少优化，不是算错）。
fn returns_local_addr(c: &SsaFunc) -> bool {
    let mut origin: BTreeSet<ValueId> = BTreeSet::new();
    for b in &c.blocks {
        for i in &b.instrs {
            if let Instr::AddrOf { dst, root } = i
                && matches!(c.roots[root.index()], Root::Slot(_))
            {
                origin.insert(*dst);
            }
        }
    }
    // 指针的别名（`copy` / `convert` / 取字段 / 取下标）继承同一个来源，两遍足够
    for _ in 0..2 {
        for b in &c.blocks {
            for i in &b.instrs {
                let (dst, src) = match i {
                    Instr::Copy { dst, src } => (*dst, *src),
                    Instr::Convert { dst, x, .. } => (*dst, *x),
                    Instr::Field { dst, base, .. } => (*dst, *base),
                    Instr::Index { dst, base, .. } => (*dst, *base),
                    _ => continue,
                };
                if origin.contains(&src) {
                    origin.insert(dst);
                }
            }
        }
    }
    // 装进复合字面量再整体返回，也等于把地址交出去（`return S{p: &x}`）
    let packed: BTreeSet<ValueId> = c
        .blocks
        .iter()
        .flat_map(|b| &b.instrs)
        .filter_map(|i| match i {
            Instr::Composite { dst, items, .. } if items.iter().any(|v| origin.contains(v)) => {
                Some(*dst)
            }
            _ => None,
        })
        .collect();
    // 写进**本帧另一个槽**再读回来返回：证据（`Store`）还在，但逃逸判定不认槽根
    // （`frame.rs` 只认全局/堆）—— 所以这里保守地当成「交出去了」。
    let stashed = c.blocks.iter().flat_map(|b| &b.instrs).any(|i| {
        matches!(i, Instr::Store { val, root, .. }
            if origin.contains(val) && matches!(c.roots[root.index()], Root::Slot(_)))
    });
    if stashed {
        return true;
    }
    c.blocks.iter().any(|b| match &b.term {
        Term::Return(vs) => vs.iter().any(|v| origin.contains(v) || packed.contains(v)),
        _ => false,
    })
}

/// 把一个调用点内联掉（`m.funcs[ci].blocks[bi].instrs[ii]` 是那条 `Call`）。
fn inline_one(m: &mut Module, ci: usize, bi: usize, ii: usize, cj: usize) {
    let callee = m.funcs[cj].clone();
    let caller = &mut m.funcs[ci];

    let bbase = caller.blocks.len();
    let vbase = caller.value_types.len();
    let (call_dsts, call_args) = match &caller.blocks[bi].instrs[ii] {
        Instr::Call { dsts, args, .. } => (dsts.clone(), args.clone()),
        _ => unreachable!("inline_one 的入参必须是 find_site 给出的调用点"),
    };

    let root_map = merge_roots(caller, &callee);
    // 值：类型表整段接在后面（编号 = 旧编号 + vbase）
    caller.value_types.extend_from_slice(&callee.value_types);

    // 块：整段接在后面，值/块/根三张表同时重映射
    let vadd = u32::try_from(vbase).expect("值号超出 u32");
    let badd = u32::try_from(bbase).expect("块号超出 u32");
    let mut moved: Vec<Block> = callee.blocks.clone();
    for b in &mut moved {
        remap_block(b, vadd, badd, &root_map);
    }

    // 调用点切开：`b0` 只留 `Call` 之前的指令，终止符换成往被调方入口的跳转
    let post = caller.blocks[bi].instrs.split_off(ii + 1);
    caller.blocks[bi].instrs.pop();
    let post_term = std::mem::take(&mut caller.blocks[bi].term);
    let callee_entry = BlockId((bbase + callee.entry.index()) as u32);
    caller.blocks[bi].term = Term::Goto {
        to: callee_entry,
        args: call_args,
    };
    caller.blocks.extend(moved);

    // 续块：参数就是原来的 `dsts`（`Call` 没了，它们的定义点搬到这里）
    let cont = BlockId(caller.blocks.len() as u32);
    for b in &mut caller.blocks[bbase..] {
        if let Term::Return(vs) = &b.term {
            let vs = vs.clone();
            b.term = Term::Goto { to: cont, args: vs };
        }
    }
    caller.blocks.push(Block {
        params: call_dsts,
        instrs: post,
        term: post_term,
    });

    // 被调方调到的签名要并进来：`V5` 查的是**本函数**的 `call_sigs`
    for (k, v) in callee.call_sigs {
        caller.call_sigs.entry(k).or_insert(v);
    }
}

/// 把被调方的根表并进调用方，返回「旧根下标 → 新根下标」。
///
/// 三种根的处置不一样：
/// * `Global(g)` / `Heap` 是**跨函数同一个东西** ⇒ 按值去重（同一个全局在
///   调用方原来就有根时复用那一条）；
/// * `Slot(s)` 是**帧内**的 ⇒ 必须换号。被调方的槽号是它自己 HIR 里的编号，
///   与调用方的槽号**必然撞车**（两边都从 0 起），撞了就是「两个变量共用一格
///   存储」—— 症状是静默算错，不是崩溃。
fn merge_roots(caller: &mut SsaFunc, callee: &SsaFunc) -> Vec<RootId> {
    // 槽号**不能**按「最大值 + 1」发：`build` 的临时槽是从 `u32::MAX - 1`
    // **往下**发的（`temp_root`），最大值再加一就溢出，撞上去就是两个槽共用一格存储。
    // 所以改成「未用的最小号」：调用方的槽号集合里挑空位，两边都不可能撞。
    let mut used: BTreeSet<u32> = caller
        .slot_root
        .keys()
        .chain(caller.slot_types.keys())
        .map(|s| s.0)
        .collect();
    let mut next = 0u32;
    // 先给被调方**所有**槽（不只是有根的）分配新号：`slot_types` 是求值器建零值
    // 存储的依据，只搬有根的那些会让「有类型没根」的槽在求值器里退化成 nil。
    let mut slot_map: BTreeMap<SlotId, SlotId> = BTreeMap::new();
    let all_slots: BTreeSet<SlotId> = callee
        .slot_root
        .keys()
        .chain(callee.slot_types.keys())
        .copied()
        .collect();
    for s in all_slots {
        // `_` 的槽（`u32::MAX`）是哨兵，不占存储，原样保留
        if s.0 == u32::MAX {
            slot_map.insert(s, s);
            continue;
        }
        while used.contains(&next) {
            next += 1;
        }
        slot_map.insert(s, SlotId(next));
        used.insert(next);
        next += 1;
    }
    for (old, new) in &slot_map {
        if let Some(t) = callee.slot_types.get(old) {
            caller.slot_types.insert(*new, *t);
        }
    }

    let mut map: Vec<RootId> = Vec::with_capacity(callee.roots.len());
    for r in &callee.roots {
        let new = match *r {
            Root::Global(g) => dedup_root(&mut caller.roots, Root::Global(g)),
            Root::Heap => dedup_root(&mut caller.roots, Root::Heap),
            Root::Slot(s) => {
                let ns = slot_map[&s];
                let rid = dedup_root(&mut caller.roots, Root::Slot(ns));
                caller.slot_root.insert(ns, rid);
                rid
            }
        };
        map.push(new);
    }
    map
}

/// 按**值**复用一条根，没有就追加。
fn dedup_root(roots: &mut Vec<Root>, r: Root) -> RootId {
    if let Some(i) = roots.iter().position(|x| *x == r) {
        return RootId(i as u32);
    }
    roots.push(r);
    RootId((roots.len() - 1) as u32)
}

/// 被搬过来的块：值号 `+vadd`、块号 `+badd`、根过 `root_map`。
fn remap_block(b: &mut Block, vadd: u32, badd: u32, root_map: &[RootId]) {
    for p in &mut b.params {
        p.0 += vadd;
    }
    for i in &mut b.instrs {
        instr_uses(i, &mut |v| v.0 += vadd);
        for d in dsts_of_mut(i) {
            d.0 += vadd;
        }
        match i {
            Instr::AddrOf { root, .. } | Instr::Load { root, .. } | Instr::Store { root, .. } => {
                *root = root_map[root.index()];
            }
            _ => {}
        }
    }
    term_uses(&mut b.term, &mut |v| v.0 += vadd);
    match &mut b.term {
        Term::Goto { to, .. } => to.0 += badd,
        Term::If { then_, else_, .. } => {
            then_.0.0 += badd;
            else_.0.0 += badd;
        }
        Term::Return(_) | Term::Panic(_) | Term::Unreachable => {}
    }
}

/// 每个函数被**引用**了几次（`Call` / `Go` / `FuncAddr` / 闭包 / 方法表 / itab）。
///
/// # 为什么只数这些
///
/// `FuncId` 的每一个出现位置都必须数到：漏一处，收尸就会把一个还在用的函数删掉，
/// 症状是链接期 `undefined reference`（比静默错好，但仍然是 pass 的 bug）。
/// `call_sigs` 的键**不算**引用 —— 那只是签名缓存。
fn ref_counts(m: &Module) -> BTreeMap<FuncId, usize> {
    let mut c: BTreeMap<FuncId, usize> = BTreeMap::new();
    let mut add = |f: FuncId| *c.entry(f).or_insert(0) += 1;
    if let Some(main) = m.main {
        add(main);
    }
    if let Some(i) = m.init
        && let Some(f) = m.funcs.get(i)
    {
        add(f.src);
    }
    for f in &m.funcs {
        for b in &f.blocks {
            for i in &b.instrs {
                match i {
                    Instr::Call { callee, .. } | Instr::Go { callee, .. } => match callee {
                        CalleeRef::Func(x) => add(*x),
                        CalleeRef::Method { func, .. } => add(*func),
                        CalleeRef::Builtin(_)
                        | CalleeRef::Runtime(_)
                        | CalleeRef::Indirect(_)
                        | CalleeRef::Iface { .. } => {}
                    },
                    Instr::FuncAddr { func, .. } | Instr::MakeClosure { func, .. } => add(*func),
                    _ => {}
                }
            }
        }
    }
    for ms in &m.methods {
        for mi in &ms.methods {
            add(mi.func);
        }
    }
    for it in &m.itabs {
        for e in &it.fns {
            add(e.func);
        }
    }
    c
}

/// 收掉「内联前有人引用、内联后一次都没有」的函数。
///
/// 返回删掉的个数。`Module::init` 是**下标**（不是 `FuncId`），删完要重编号。
fn drop_emptied(
    m: &mut Module,
    before: &BTreeMap<FuncId, usize>,
    after: &BTreeMap<FuncId, usize>,
) -> usize {
    let main = m.main;
    let init_src = m.init.and_then(|i| m.funcs.get(i)).map(|f| f.src);
    let dead: BTreeSet<FuncId> = m
        .funcs
        .iter()
        .map(|f| f.src)
        .filter(|src| {
            Some(*src) != main
                && Some(*src) != init_src
                && before.get(src).copied().unwrap_or(0) > 0
                && after.get(src).copied().unwrap_or(0) == 0
        })
        .collect();
    if dead.is_empty() {
        return 0;
    }
    let init_idx = m.init;
    let mut next_init = init_idx;
    let mut kept: Vec<SsaFunc> = Vec::with_capacity(m.funcs.len());
    for (i, f) in std::mem::take(&mut m.funcs).into_iter().enumerate() {
        if dead.contains(&f.src) {
            if let Some(ni) = next_init
                && i < ni
            {
                next_init = Some(ni - 1);
            }
            continue;
        }
        kept.push(f);
    }
    let n = dead.len();
    m.funcs = kept;
    m.init = next_init.map(|i| i.min(m.funcs.len().saturating_sub(1)));
    n
}
