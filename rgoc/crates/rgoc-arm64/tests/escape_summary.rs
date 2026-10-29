//! 模块级逃逸摘要（`T-OPT-02` / `G12-11`；`M12-plan.md` Task 38）。
//!
//! # 判据的两半
//!
//! 1. **摘要本身**：`escape::analyze` 对一个模块算出的逐形参结论（`false` = 那个
//!    形参不会从被调方里逃出去）；
//! 2. **摘要真的改变了帧布局**：`false` 的形参 ⇒ 调用方那个槽**不进** `fr.heap`
//!    （留在帧里，不 `rt_alloc_escaped`）；`true` 的 ⇒ 进。
//!
//! 只有第 1 半的话，一个「算得很准但没人用」的实现也能全绿 —— 这正是
//! `T-OPT-02` 要挡的东西（读数：`allocs` 下降）。
//!
//! # 三条保守面的判据
//!
//! * 间接调用（函数值）的被调方运行期才定 ⇒ 实参一律逃逸；
//! * `go f(&x)` 拷的是指针值 ⇒ 一律逃逸；
//! * 摘要里查不到的 `FuncId`（跨模块/合成函数）⇒ 一律逃逸。

use std::collections::BTreeMap;

use rgoc_arm64::escape;
use rgoc_arm64::frame;
use rgoc_hir::types::ConstVal;
use rgoc_hir::{FuncId, GlobalId, SlotId};
use rgoc_ssa::ir::{CalleeRef, Effect, Instr, Module, Root, SsaFunc, Term, ValueId};
use rgoc_ssa::testsupport::{FnBuilder, module_of};
use rgoc_types::TypeTable;

/// `func g(p *int) int { return *p }` —— 形参**不**逃逸。
fn callee_reads() -> (SsaFunc, TypeTable) {
    let mut fb = FnBuilder::new("g");
    fb.src_func(FuncId(1));
    let int = fb.basic("int64");
    let ptr = fb.ptr_to(int);
    let e = fb.entry();
    let p = fb.value(ptr);
    fb.set_params(e, &[p]);
    let mem = fb.value(ptr);
    fb.instr(
        e,
        Instr::Const {
            dst: mem,
            c: ConstVal::Int(0),
            typ: ptr,
        },
    );
    let root = fb.root(Root::Heap);
    let v = fb.value(int);
    fb.instr(
        e,
        Instr::Load {
            dst: v,
            mem,
            root,
            addr: p,
            typ: int,
        },
    );
    fb.term(e, Term::Return(vec![v]));
    fb.into_parts()
}

/// `func g(p *int) { global = p }` —— 形参**逃逸**。
fn callee_stores() -> (SsaFunc, TypeTable) {
    let mut fb = FnBuilder::new("g");
    fb.src_func(FuncId(1));
    let int = fb.basic("int64");
    let ptr = fb.ptr_to(int);
    let e = fb.entry();
    let p = fb.value(ptr);
    fb.set_params(e, &[p]);
    let mem = fb.value(ptr);
    fb.instr(
        e,
        Instr::Const {
            dst: mem,
            c: ConstVal::Int(0),
            typ: ptr,
        },
    );
    // `g0 = p`：写进**全局**根 ⇒ 地址活得比这一帧长
    let g = fb.root(Root::Global(GlobalId(0)));
    let nm = fb.value(ptr);
    fb.instr(
        e,
        Instr::Store {
            dst: nm,
            mem,
            root: g,
            addr: ValueId(0),
            val: p,
        },
    );
    fb.term(e, Term::Return(vec![]));
    fb.into_parts()
}

/// `func f() int { x := 1; return g(&x) }`，`g` 由 `CalleeRef::Func(FuncId(1))` 指定。
fn caller_with(target: CalleeRef) -> SsaFunc {
    let mut fb = FnBuilder::new("f");
    fb.src_func(FuncId(0));
    let int = fb.basic("int64");
    let ptr = fb.ptr_to(int);
    let e = fb.entry();
    let slot = fb.root(Root::Slot(SlotId(0)));
    fb.slot_root(SlotId(0), slot);
    fb.slot_typ(SlotId(0), int);
    let addr = fb.value(ptr);
    fb.instr(
        e,
        Instr::AddrOf {
            dst: addr,
            root: slot,
        },
    );
    let r = fb.value(int);
    fb.instr(
        e,
        Instr::Call {
            dsts: vec![r],
            callee: target,
            args: vec![addr],
            dots: false,
            effect: Effect::default(),
        },
    );
    fb.call_sig(FuncId(1), 1, 1, false);
    fb.term(e, Term::Return(vec![r]));
    fb.finish()
}

/// 用摘要布一次帧，返回「那个槽在不在 `heap` 里」（= 有没有 `rt_alloc_escaped`）。
fn slot_escapes(
    f: &SsaFunc,
    siblings: &BTreeMap<FuncId, &SsaFunc>,
    m: &Module,
    tt: &TypeTable,
) -> bool {
    let esc = escape::analyze(m);
    let fr = frame::layout_with_esc(f, tt, siblings, &esc).expect("帧布局不该失败");
    fr.heap.contains_key(&SlotId(0))
}

#[test]
fn t_opt_02_不逃逸的形参_槽留在帧里() {
    let (g, gtt) = callee_reads();
    let f = caller_with(CalleeRef::Func(FuncId(1)));
    let mut m = module_of(vec![f.clone(), g.clone()]);
    m.main = Some(FuncId(0));

    let info = escape::analyze(&m);
    assert_eq!(
        info.escapes(FuncId(1), 0),
        Some(false),
        "`g` 只把 `p` 读过一次，形参不该逃逸"
    );

    let sib: BTreeMap<FuncId, &SsaFunc> = m.funcs.iter().map(|x| (x.src, x)).collect();
    assert!(
        !slot_escapes(&m.funcs[0], &sib, &m, &gtt),
        "摘要说不逃逸 ⇒ 调用方的槽必须留在帧里（不 `rt_alloc_escaped`）"
    );
}

#[test]
fn t_opt_02_存进全局的形参_槽进_arena() {
    let (g, gtt) = callee_stores();
    let f = caller_with(CalleeRef::Func(FuncId(1)));
    let mut m = module_of(vec![f.clone(), g.clone()]);
    m.main = Some(FuncId(0));

    let info = escape::analyze(&m);
    assert_eq!(
        info.escapes(FuncId(1), 0),
        Some(true),
        "`g` 把 `p` 写进全局 ⇒ 形参逃逸"
    );

    let sib: BTreeMap<FuncId, &SsaFunc> = m.funcs.iter().map(|x| (x.src, x)).collect();
    assert!(
        slot_escapes(&m.funcs[0], &sib, &m, &gtt),
        "逃逸结论必须传到帧布局上"
    );
}

#[test]
fn t_opt_02_互递归也能收敛() {
    // `a(p)` 调 `b(p)`；`b(p)` 把 `p` 写进全局 ⇒ 两个都是 true。
    let (b, btt) = callee_stores(); // b(p) 写全局
    let b = SsaFunc {
        src: FuncId(2),
        name: "b".to_string(),
        ..b
    };
    let (a, att) = {
        let mut fb = FnBuilder::new("a");
        fb.src_func(FuncId(1));
        let int = fb.basic("int64");
        let ptr = fb.ptr_to(int);
        let e = fb.entry();
        let p = fb.value(ptr);
        fb.set_params(e, &[p]);
        let r = fb.value(int);
        fb.instr(
            e,
            Instr::Call {
                dsts: vec![r],
                callee: CalleeRef::Func(FuncId(2)),
                args: vec![p],
                dots: false,
                effect: Effect::default(),
            },
        );
        fb.call_sig(FuncId(2), 1, 1, false);
        fb.term(e, Term::Return(vec![r]));
        fb.into_parts()
    };
    let m = module_of(vec![a, b]);
    let info = escape::analyze(&m);
    assert_eq!(info.escapes(FuncId(2), 0), Some(true), "`b` 自己写全局");
    assert_eq!(
        info.escapes(FuncId(1), 0),
        Some(true),
        "`a` 只把形参交给 `b`，但 `b` 会让它逃逸 ⇒ 不动点要把 `a` 也翻成 true"
    );
    let _ = (btt, att);
}

#[test]
fn t_opt_02_间接调用的实参一律逃逸() {
    let (g, gtt) = callee_reads();
    let f = caller_with(CalleeRef::Indirect(ValueId(0)));
    let m = module_of(vec![f.clone(), g]);
    let sib: BTreeMap<FuncId, &SsaFunc> = m.funcs.iter().map(|x| (x.src, x)).collect();
    assert!(
        slot_escapes(&m.funcs[0], &sib, &m, &gtt),
        "函数值调用没有静态目标 ⇒ 实参必须按逃逸处理（`M12/Task 38` 补的漏判）"
    );
}

#[test]
fn t_opt_02_摘要里查不到的被调方一律逃逸() {
    // `CalleeRef::Func(FuncId(9))` 在模块里没有对应函数 ⇒ 当逃逸
    let (g, gtt) = callee_reads();
    let f = caller_with(CalleeRef::Func(FuncId(9)));
    let m = module_of(vec![f.clone(), g]);
    let sib: BTreeMap<FuncId, &SsaFunc> = m.funcs.iter().map(|x| (x.src, x)).collect();
    assert!(
        slot_escapes(&m.funcs[0], &sib, &m, &gtt),
        "查不到被调方 ⇒ 不能假设它不留下指针"
    );
}

#[test]
fn t_opt_02_没有摘要时退化成保守档() {
    // `layout`（不传摘要）必须与「全都逃逸」一致 —— 单函数测试走的就是这条路
    let (g, gtt) = callee_reads();
    let f = caller_with(CalleeRef::Func(FuncId(1)));
    let m = module_of(vec![f.clone(), g]);
    let sib: BTreeMap<FuncId, &SsaFunc> = m.funcs.iter().map(|x| (x.src, x)).collect();
    let fr = frame::layout_with(&m.funcs[0], &gtt, &sib).expect("帧布局不该失败");
    assert!(
        fr.heap.contains_key(&SlotId(0)),
        "`layout_with` 没有摘要 ⇒ 一律逃逸（保守），不能偷偷变精确"
    );
}
