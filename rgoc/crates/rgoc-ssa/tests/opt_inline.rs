//! 内联（`T-OPT-01` / `G12-10`；`M12-plan.md` Task 37）。
//!
//! # 判据的四半
//!
//! 1. **搬到了**：调用点没了、被调方的体进了调用方、`verify` 全绿；
//! 2. **不多搬**：带 `defer` / `go` / 自递归 / 超预算的被调方一律不动
//!    （这些不是保守，是正确性 —— 见 `opt::inline` 的模块注释）；
//! 3. **重映射四张表**：值号 / 块号 / 根下标 / **槽号**。最后一张最容易漏，
//!    漏了的症状是「被调方的局部变量与调用方的局部变量共用一格存储」
//!    —— 静默算错，不是崩溃；
//! 4. **收尸有边界**：只收「内联前有人引用、内联后一次都没有」的函数；
//!    本来就没引用的死代码不动。
//!
//! # 关于两张类型表
//!
//! `FnBuilder` 每人一张 `TypeTable`，但表头 22 项是 `universe()` 的**确定性**
//! 产物，所以两个构建器里 `basic("int64")` 拿到的 `TypeId` 是同一个下标 ——
//! 只要两边只加同一个基本类型，`verify` 用其中一张表就够。

use rgoc_hir::types::ConstVal;
use rgoc_hir::{BinOp, FuncId, SlotId};
use rgoc_ssa::ir::{CalleeRef, Effect, Instr, Module, Root, Sig, SsaFunc, Term};
use rgoc_ssa::opt::{self, Config, inline};
use rgoc_ssa::testsupport::{FnBuilder, module_of};
use rgoc_ssa::verify::verify;
use rgoc_types::TypeTable;

fn calls(f: &SsaFunc) -> usize {
    f.blocks
        .iter()
        .flat_map(|b| &b.instrs)
        .filter(|i| matches!(i, Instr::Call { .. }))
        .count()
}

fn module(main: FuncId, funcs: Vec<SsaFunc>) -> Module {
    Module {
        main: Some(main),
        ..module_of(funcs)
    }
}

/// `func g(a, b int) int { return a + b }`
fn callee_add() -> (SsaFunc, TypeTable) {
    let mut fb = FnBuilder::new("g");
    fb.src_func(FuncId(1));
    let int = fb.basic("int64");
    let e = fb.entry();
    let a = fb.value(int);
    let b = fb.value(int);
    let s = fb.value(int);
    fb.set_params(e, &[a, b]);
    fb.instr(
        e,
        Instr::Bin {
            dst: s,
            op: BinOp::Add,
            lhs: a,
            rhs: b,
            typ: int,
        },
    );
    fb.term(e, Term::Return(vec![s]));
    fb.into_parts()
}

/// `func f() int { return g(1, 2) }`（`sites` 个调用点）
fn caller(sites: usize) -> SsaFunc {
    let mut fb = FnBuilder::new("f");
    fb.src_func(FuncId(0));
    let int = fb.basic("int64");
    let e = fb.entry();
    let c1 = fb.value(int);
    let c2 = fb.value(int);
    fb.instr(
        e,
        Instr::Const {
            dst: c1,
            c: ConstVal::Int(1),
            typ: int,
        },
    );
    fb.instr(
        e,
        Instr::Const {
            dst: c2,
            c: ConstVal::Int(2),
            typ: int,
        },
    );
    let mut last = c1;
    for _ in 0..sites {
        let r = fb.value(int);
        fb.instr(
            e,
            Instr::Call {
                dsts: vec![r],
                callee: CalleeRef::Func(FuncId(1)),
                args: vec![c1, c2],
                dots: false,
                effect: Effect::default(),
            },
        );
        last = r;
    }
    fb.call_sig(FuncId(1), 2, 1, false);
    fb.term(e, Term::Return(vec![last]));
    fb.finish()
}

#[test]
fn t_opt_01_单调用点被搬进调用方且被调方被收尸() {
    let (g, tt) = callee_add();
    let f = caller(1);
    let mut m = module(FuncId(0), vec![f, g]);

    let st = inline::run(&mut m, &tt).expect("内联之后必须仍然合法");
    assert_eq!(st.sites, 1, "应搬一个调用点");
    assert_eq!(st.dropped, 1, "只有一个调用点 ⇒ 搬完就没人引用，应收尸");
    assert_eq!(m.funcs.len(), 1, "被调方已入土");
    assert_eq!(calls(&m.funcs[0]), 0, "调用点应消失");
    verify(&m.funcs[0], &tt).expect("收口再验一次");
    // 三段结构：切开后的入口块 / 被调方的体 / 续块
    assert_eq!(m.funcs[0].blocks.len(), 3, "切开 + 体 + 续块");
    let cont = m.funcs[0].blocks.last().expect("续块");
    assert_eq!(cont.params.len(), 1, "续块的参数 = 原来的 dsts");
    assert!(matches!(cont.term, Term::Return(ref v) if v.len() == 1));
}

#[test]
fn t_opt_01_还被引用就不收尸() {
    // 两个调用点都搬走之后，剩下的引用来自**方法表**（`g` 是某个类型的 M）：
    // 收尸的边界是「一次都没人引用」，不是「没有调用点」。
    let (g, tt) = callee_add();
    let f = caller(2);
    let mut m = module(FuncId(0), vec![f, g]);
    let ty = m.funcs[0].value_types[0];
    m.methods.push(rgoc_ssa::ir::MethodSet {
        ty,
        methods: vec![rgoc_ssa::ir::MethodImpl {
            name: "M".to_string(),
            params: vec![],
            results: vec![ty],
            func: FuncId(1),
            hops: vec![],
        }],
    });
    let st = inline::run(&mut m, &tt).expect("内联之后必须仍然合法");
    assert_eq!(st.sites, 2, "两个调用点都该搬");
    assert_eq!(st.dropped, 0, "方法表还指着它 ⇒ 不能收尸");
    assert_eq!(m.funcs.len(), 2, "被调方留着");
    assert_eq!(calls(&m.funcs[0]), 0);
    verify(&m.funcs[0], &tt).expect("两次内联之后也要合法");
}

#[test]
fn t_opt_01_带_defer_的被调方不内联() {
    let (mut g, tt) = callee_add();
    // 塞一条 `Defer`（`fv` 用现成的形参顶替；本测试只关心 `eligible` 说不）
    let fv = g.blocks[0].params[0];
    g.blocks[0].instrs.push(Instr::Defer { fv });
    let f = caller(1);
    let mut m = module(FuncId(0), vec![f, g]);
    let st = inline::run(&mut m, &tt).expect("不内联也要合法");
    assert_eq!(st.sites, 0, "`defer` 登记在当前帧上，不能内联");
    assert_eq!(calls(&m.funcs[0]), 1, "调用点原样留着");
    assert_eq!(m.funcs.len(), 2, "没人动被调方");
}

#[test]
fn t_opt_01_自递归的被调方不内联() {
    let (mut g, tt) = callee_add();
    // 自己调自己（`call_sigs` 里有自己的 `src`）
    g.call_sigs.insert(
        FuncId(1),
        Sig {
            params: 2,
            results: 1,
            variadic: false,
        },
    );
    let f = caller(1);
    let mut m = module(FuncId(0), vec![f, g]);
    let st = inline::run(&mut m, &tt).expect("不内联也要合法");
    assert_eq!(st.sites, 0, "自递归不内联（否则 find_site 会一直命中）");
}

#[test]
fn t_opt_01_槽号不会撞车() {
    // 被调方与调用方**都有** s0 的取址槽：内联之后必须是两个不同的槽，
    // 否则两个变量共用一格存储（静默算错）。
    let mut fb = FnBuilder::new("g");
    fb.src_func(FuncId(1));
    let int = fb.basic("int64");
    let e0 = fb.entry();
    let p = fb.value(int);
    fb.set_params(e0, &[p]);
    let rg = fb.root(Root::Slot(SlotId(0)));
    fb.slot_root(SlotId(0), rg);
    fb.slot_typ(SlotId(0), int);
    let addr = fb.value(int);
    fb.instr(
        e0,
        Instr::AddrOf {
            dst: addr,
            root: rg,
        },
    );
    let one = fb.value(int);
    fb.instr(
        e0,
        Instr::Const {
            dst: one,
            c: ConstVal::Int(1),
            typ: int,
        },
    );
    fb.term(e0, Term::Return(vec![one]));
    let (g, tt) = fb.into_parts();

    let mut fb = FnBuilder::new("f");
    fb.src_func(FuncId(0));
    let int = fb.basic("int64");
    let e = fb.entry();
    let rc = fb.root(Root::Slot(SlotId(0)));
    fb.slot_root(SlotId(0), rc);
    fb.slot_typ(SlotId(0), int);
    let ca = fb.value(int);
    fb.instr(e, Instr::AddrOf { dst: ca, root: rc });
    let a0 = fb.value(int);
    fb.instr(
        e,
        Instr::Const {
            dst: a0,
            c: ConstVal::Int(7),
            typ: int,
        },
    );
    let r = fb.value(int);
    fb.instr(
        e,
        Instr::Call {
            dsts: vec![r],
            callee: CalleeRef::Func(FuncId(1)),
            args: vec![a0],
            dots: false,
            effect: Effect::default(),
        },
    );
    fb.call_sig(FuncId(1), 1, 1, false);
    fb.term(e, Term::Return(vec![r]));
    let f = fb.finish();

    let mut m = module(FuncId(0), vec![f, g]);
    let st = inline::run(&mut m, &tt).expect("内联之后必须仍然合法");
    assert_eq!(st.sites, 1);
    let f = &m.funcs[0];
    assert_eq!(
        f.slot_root.len(),
        2,
        "两个 s0 必须被拆成两个不同的槽：{:?}",
        f.slot_root
    );
    assert_eq!(f.roots.len(), 2, "两个 Slot 根");
    assert!(
        f.roots.iter().all(|r| matches!(r, Root::Slot(_))),
        "不该混进别种根"
    );
    verify(f, &tt).expect("槽号撞车的图不可能过 V9");
}

#[test]
fn t_opt_01_超预算的被调方不内联() {
    let mut fb = FnBuilder::new("g");
    fb.src_func(FuncId(1));
    let int = fb.basic("int64");
    let e = fb.entry();
    let p = fb.value(int);
    fb.set_params(e, &[p]);
    let mut last = p;
    for k in 0..(inline::MAX_CALLEE_INSTRS + 4) {
        let v = fb.value(int);
        fb.instr(
            e,
            Instr::Const {
                dst: v,
                c: ConstVal::Int(k as i128),
                typ: int,
            },
        );
        last = v;
    }
    fb.term(e, Term::Return(vec![last]));
    let (g, tt) = fb.into_parts();

    let f = caller(1);
    let mut m = module(FuncId(0), vec![f, g]);
    let st = inline::run(&mut m, &tt).expect("不内联也要合法");
    assert_eq!(st.sites, 0, "超预算不内联");
    assert_eq!(m.funcs.len(), 2, "没人动被调方");
}

#[test]
fn t_opt_01_返回局部地址的被调方不内联() {
    // `func g() *int { x := 1; return &x }`：内联之后 `Return` 没了，
    // 后端的逃逸分析（`frame.rs`）会把 `x` 留在帧里 ⇒ 循环里两次调用同址。
    let mut fb = FnBuilder::new("g");
    fb.src_func(FuncId(1));
    let int = fb.basic("int64");
    let ptr = fb.ptr_to(int);
    let e = fb.entry();
    let r = fb.root(Root::Slot(SlotId(0)));
    fb.slot_root(SlotId(0), r);
    fb.slot_typ(SlotId(0), int);
    let addr = fb.value(ptr);
    fb.instr(e, Instr::AddrOf { dst: addr, root: r });
    fb.term(e, Term::Return(vec![addr]));
    let (g, tt) = fb.into_parts();

    let mut fb = FnBuilder::new("f");
    fb.src_func(FuncId(0));
    let int = fb.basic("int64");
    let ptr = fb.ptr_to(int);
    let e = fb.entry();
    let d = fb.value(ptr);
    fb.instr(
        e,
        Instr::Call {
            dsts: vec![d],
            callee: CalleeRef::Func(FuncId(1)),
            args: vec![],
            dots: false,
            effect: Effect::default(),
        },
    );
    fb.call_sig(FuncId(1), 0, 1, false);
    fb.term(e, Term::Return(vec![d]));
    let f = fb.finish();

    let mut m = module(FuncId(0), vec![f, g]);
    let st = inline::run(&mut m, &tt).expect("不内联也要合法");
    assert_eq!(st.sites, 0, "返回局部地址 ⇒ 不能内联（帧槽会退回帧里）");
    assert_eq!(m.funcs.len(), 2, "被调方留着");
}

#[test]
fn t_opt_01_全开之后仍然合法() {
    // `opt::run` 的入口也要走一遍：inline 之后的四个逐函数 pass 不能把它弄坏
    let (g, tt) = callee_add();
    let f = caller(1);
    let mut m = module(FuncId(0), vec![f, g]);
    opt::run(&mut m, &tt, Config::all()).expect("全开之后必须仍然合法");
    for f in &m.funcs {
        verify(f, &tt).expect("逐个再验一次");
    }
    assert_eq!(calls(&m.funcs[0]), 0, "内联 + dce 之后不该剩调用点");
}
