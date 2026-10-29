//! **模块级**逃逸摘要：每个函数的第 i 个形参会不会从它里面逃出去
//! （`T-OPT-02` / `G12-11`；`M12-plan.md` Task 38）。
//!
//! # 为什么需要跨函数的摘要
//!
//! 「`&x` 能不能留在帧里」这个判定的关键一票来自**被调方**：`f(&x)` 里
//! `x` 逃不逃，取决于 `f` 有没有把那个指针存到全局、返回出去、交给别人。
//! 只看调用点只能保守地一律「逃逸」—— 那就是「所有取址槽都上堆」，
//! 也是 `T-OPT-02` 要改掉的那一档。
//!
//! 摘要是一个**单调不动点**：所有形参先算「不逃逸」，发现某处会把某个形参
//! 交出去就把它标成逃逸，再重算 —— 只涨不落，所以一定收敛（最坏全 true）。
//! 递归/互递归因此不需要特殊处理：环里的函数在下一轮看见自己上一轮的结论。
//!
//! # 两处订正（这次一起做）
//!
//! 1. **更精确**：`Func` / `Method` 的实参按被调方的**逐形参**摘要判
//!    （以前是一律逃逸）。
//! 2. **补漏判**：`Indirect`（函数值调用）与 `Go` 的实参**一律**逃逸 ——
//!    以前两处都没算（见 `M12-manifest.json` 的 `open_gaps`）：
//!    * `Indirect` 的被调方运行期才定，可能把指针存下来；
//!    * `go f(&x)` 深拷贝的是**指针值**，指向的仍是**调用方帧** ——
//!      父 G 先返回，子 G 就在读已死的栈。
//!
//! 第 2 条会让一部分语料**多**分配几个字节（保守方向），这正是
//! 「宁可多分配，不可悬空」的代价。
//!
//! # 为什么不放进 `rgoc-ssa`
//!
//! 它服务的是**帧布局**（`frame.rs::escaping_slots`），而对「slot 留在帧里还是
//! 上 arena」做决定的是后端（`C12` §4.5）。放进 SSA 会让 `--dump=ssa` 的输出
//! 依赖一个它不用的分析，也让 `rgoc-ssa` 多一个只有后端关心的入口。

use std::collections::{BTreeMap, BTreeSet};

use rgoc_hir::FuncId;
use rgoc_ssa::ir::{CalleeRef, Instr, Module, Root, SsaFunc, Term, ValueId};

/// 一个函数的逐形参逃逸结论。
#[derive(Clone, Debug, Default)]
pub struct EscInfo {
    per: BTreeMap<FuncId, Vec<bool>>,
}

impl EscInfo {
    /// 第 `param` 个形参逃逸吗。`None` = 这个函数/形参**不在**摘要里（保守地当逃逸）。
    #[must_use]
    pub fn escapes(&self, fid: FuncId, param: usize) -> Option<bool> {
        self.per.get(&fid).and_then(|v| v.get(param).copied())
    }

    /// 某个函数的整条结论（诊断用）。
    #[must_use]
    pub fn of(&self, fid: FuncId) -> Option<&[bool]> {
        self.per.get(&fid).map(Vec::as_slice)
    }

    /// 空摘要：任何查询都是 `None`（⇒ 调用点一律当逃逸）。
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }
}

/// 算整个模块的摘要。
#[must_use]
pub fn analyze(m: &Module) -> EscInfo {
    let mut info = EscInfo::default();
    for f in &m.funcs {
        info.per.insert(f.src, vec![false; f.params.len()]);
    }
    // 单调不动点。轮数上限取「形参总数 + 1」—— 每一轮至少要把一个 `false`
    // 翻成 `true` 才会继续，所以这是收敛的硬上界（不是拍脑袋的次数）。
    let bound: usize = m.funcs.iter().map(|f| f.params.len() + 1).sum();
    for _ in 0..bound.max(1) {
        let mut changed = false;
        for f in &m.funcs {
            let s = summary(f, &info);
            if let Some(slot) = info.per.get_mut(&f.src) {
                for (i, v) in s.iter().enumerate() {
                    if *v && !slot[i] {
                        slot[i] = true;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    info
}

/// 在**当前摘要**下重算一个函数的逃逸集。
fn summary(f: &SsaFunc, info: &EscInfo) -> Vec<bool> {
    let n = f.params.len();
    let mut esc = vec![false; n];
    if n == 0 {
        return esc;
    }

    // 值 → 由哪些形参派生（指针的别名：`copy` / `convert` / 取字段 / 取下标 /
    // 打包进复合字面量）。收敛到不变为止：值号未必按定义顺序跨块排列。
    let mut from: BTreeMap<ValueId, BTreeSet<usize>> = BTreeMap::new();
    for (i, p) in f.params.iter().enumerate() {
        from.entry(*p).or_default().insert(i);
    }
    loop {
        let mut grew = false;
        for b in &f.blocks {
            for i in &b.instrs {
                let (dst, srcs): (ValueId, Vec<ValueId>) = match i {
                    Instr::Copy { dst, src } => (*dst, vec![*src]),
                    Instr::Convert { dst, x, .. } => (*dst, vec![*x]),
                    Instr::Field { dst, base, .. } => (*dst, vec![*base]),
                    Instr::Index { dst, base, .. } => (*dst, vec![*base]),
                    Instr::Composite { dst, items, .. } => (*dst, items.clone()),
                    _ => continue,
                };
                let mut add: BTreeSet<usize> = BTreeSet::new();
                for s in srcs {
                    if let Some(set) = from.get(&s) {
                        add.extend(set.iter().copied());
                    }
                }
                if add.is_empty() {
                    continue;
                }
                let slot = from.entry(dst).or_default();
                let before = slot.len();
                slot.extend(add);
                if slot.len() != before {
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }

    let mark = |v: &ValueId, esc: &mut Vec<bool>| {
        if let Some(set) = from.get(v) {
            for i in set {
                esc[*i] = true;
            }
        }
    };

    for b in &f.blocks {
        for i in &b.instrs {
            match i {
                // 静态目标：按**被调方的逐形参结论**判（`None` ⇒ 不知道 ⇒ 逃逸）
                Instr::Call {
                    callee: CalleeRef::Func(fid),
                    args,
                    ..
                } => {
                    for (k, a) in args.iter().enumerate() {
                        if info.escapes(*fid, k).unwrap_or(true) {
                            mark(a, &mut esc);
                        }
                    }
                }
                Instr::Call {
                    callee: CalleeRef::Method { func, recv },
                    args,
                    ..
                } => {
                    // `call_sigs` 里的形参**含接收者**，所以 recv 是第 0 个
                    if info.escapes(*func, 0).unwrap_or(true) {
                        mark(recv, &mut esc);
                    }
                    for (k, a) in args.iter().enumerate() {
                        if info.escapes(*func, k + 1).unwrap_or(true) {
                            mark(a, &mut esc);
                        }
                    }
                }
                // 运行期助手与内建**拷值**（`rt_map_set` 把 `*val` 的字节抄进槽），
                // 地址本身不外流；接口派发的 `data` 是箱子（`box_iface` 已拷贝）。
                Instr::Call {
                    callee: CalleeRef::Runtime(_) | CalleeRef::Builtin(_) | CalleeRef::Iface { .. },
                    ..
                } => {}
                // 函数值调用：被调方运行期才定，可能把指针存下来 ⇒ 一律逃逸
                Instr::Call {
                    callee: CalleeRef::Indirect(_),
                    args,
                    ..
                } => {
                    for a in args {
                        mark(a, &mut esc);
                    }
                }
                // `go f(&x)`：深拷贝的是**指针值**，指向的仍是本帧
                Instr::Go { callee, args, .. } => {
                    match callee {
                        CalleeRef::Method { recv, .. } | CalleeRef::Iface { recv, .. } => {
                            mark(recv, &mut esc);
                        }
                        CalleeRef::Indirect(v) => mark(v, &mut esc),
                        CalleeRef::Func(_) | CalleeRef::Builtin(_) | CalleeRef::Runtime(_) => {}
                    }
                    for a in args {
                        mark(a, &mut esc);
                    }
                }
                // 闭包捕获的变量活得比本帧长（funcval 在堆上）
                Instr::MakeClosure { captures, .. } => {
                    for c in captures {
                        mark(c, &mut esc);
                    }
                }
                // 写进全局 / 堆 ⇒ 地址活得比这一帧长
                Instr::Store {
                    root, addr, val, ..
                } if !matches!(f.roots[root.index()], Root::Slot(_)) => {
                    mark(addr, &mut esc);
                    mark(val, &mut esc);
                }
                _ => {}
            }
        }
        if let Term::Return(vs) = &b.term {
            for v in vs {
                mark(v, &mut esc);
            }
        }
    }
    esc
}
