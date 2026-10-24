//! 死代码删除（`T-SSA-61`；`M6-plan.md` Task 42）。
//!
//! # 判据的两半
//!
//! * **删得掉**：没人用且没副作用的值；
//! * **删不掉**：`Store`（写是唯一可观察副作用）、有 effect 的 `Call`（可能 panic / 写）、
//!   间接调用（被调方未知，effect 只是保守估计）。
//!
//! 第二半是这条 pass 的**安全边界**：只测「删得掉」的话，一个「什么都删」的实现
//! 也能全绿 —— 而它会把 `*p = 1` 删掉。

use rgoc_hir::BuiltinKind;
use rgoc_hir::types::ConstVal;
use rgoc_ssa::ir::{CalleeRef, Effect, Instr, SsaFunc, Term};
use rgoc_ssa::opt::dce;
use rgoc_ssa::testsupport::FnBuilder;
use rgoc_ssa::verify::verify;
use rgoc_types::TypeTable;

fn check(f: &mut SsaFunc, tt: &TypeTable) {
    dce::run(f);
    verify(f, tt).expect("dce 之后必须仍然合法");
}

fn instrs(f: &SsaFunc) -> Vec<&Instr> {
    f.blocks.iter().flat_map(|b| &b.instrs).collect()
}

#[test]
fn t_ssa_61_没用的常量被删() {
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let e = fb.entry();
    let dead = fb.value(t);
    let live = fb.value(t);
    fb.instr(
        e,
        Instr::Const {
            dst: dead,
            c: ConstVal::Int(1),
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Const {
            dst: live,
            c: ConstVal::Int(2),
            typ: t,
        },
    );
    fb.term(e, Term::Return(vec![live]));
    let (mut f, tt) = fb.into_parts();
    check(&mut f, &tt);
    assert_eq!(instrs(&f).len(), 1, "死常量该被删");
    assert_eq!(f.value_types.len(), 1, "值也要压实（V1 要求每个值有定义）");
}

#[test]
fn t_ssa_61_没用的_store_必须留着() {
    // `*p = 1` 的 mem 令牌没人用是常态 —— 删了就把写变没了
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let pt = fb.basic("int");
    let e = fb.entry();
    let root = fb.root(rgoc_ssa::ir::Root::Heap);
    let addr = fb.value(pt);
    let val = fb.value(t);
    let mem = fb.value(pt);
    let st = fb.value(pt);
    fb.instr(
        e,
        Instr::Const {
            dst: addr,
            c: ConstVal::Int(0),
            typ: pt,
        },
    );
    fb.instr(
        e,
        Instr::Const {
            dst: val,
            c: ConstVal::Int(1),
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Const {
            dst: mem,
            c: ConstVal::Int(0),
            typ: pt,
        },
    );
    fb.instr(
        e,
        Instr::Store {
            dst: st,
            mem,
            root,
            addr,
            val,
        },
    );
    fb.term(e, Term::Return(Vec::new()));
    let (mut f, tt) = fb.into_parts();
    check(&mut f, &tt);
    assert!(
        instrs(&f).iter().any(|i| matches!(i, Instr::Store { .. })),
        "Store 必须留着（它的 mem 没人用不代表写没发生）"
    );
}

#[test]
fn t_ssa_61_纯调用没人用就删() {
    // `len(x)` 的 effect 全空 ⇒ 结果没人用就可以删
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let e = fb.entry();
    let x = fb.value(t);
    let r = fb.value(t);
    fb.instr(
        e,
        Instr::Const {
            dst: x,
            c: ConstVal::Int(1),
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Call {
            dsts: vec![r],
            callee: CalleeRef::Builtin(BuiltinKind::Len),
            args: vec![x],
            dots: false,
            effect: Effect::default(),
        },
    );
    fb.term(e, Term::Return(Vec::new()));
    let (mut f, tt) = fb.into_parts();
    check(&mut f, &tt);
    assert!(
        !instrs(&f).iter().any(|i| matches!(i, Instr::Call { .. })),
        "纯调用没人用 ⇒ 删"
    );
}

#[test]
fn t_ssa_61_有副作用的调用必须留着() {
    // `writes_all` ⇒ 可能写任何根；哪怕结果没人用也不能删
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let e = fb.entry();
    let x = fb.value(t);
    let r = fb.value(t);
    fb.instr(
        e,
        Instr::Const {
            dst: x,
            c: ConstVal::Int(1),
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Call {
            dsts: vec![r],
            callee: CalleeRef::Builtin(BuiltinKind::Len),
            args: vec![x],
            dots: false,
            effect: Effect {
                writes_all: true,
                ..Effect::default()
            },
        },
    );
    fb.term(e, Term::Return(Vec::new()));
    let (mut f, tt) = fb.into_parts();
    check(&mut f, &tt);
    assert!(
        instrs(&f).iter().any(|i| matches!(i, Instr::Call { .. })),
        "有效果的调用必须留着"
    );
}

#[test]
fn t_ssa_61_可能_panic_的调用必须留着() {
    // panic 也是可观察的（输出 + 退出码 2）
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let e = fb.entry();
    let x = fb.value(t);
    let r = fb.value(t);
    fb.instr(
        e,
        Instr::Const {
            dst: x,
            c: ConstVal::Int(1),
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Call {
            dsts: vec![r],
            callee: CalleeRef::Builtin(BuiltinKind::Len),
            args: vec![x],
            dots: false,
            effect: Effect {
                may_panic: true,
                ..Effect::default()
            },
        },
    );
    fb.term(e, Term::Return(Vec::new()));
    let (mut f, tt) = fb.into_parts();
    check(&mut f, &tt);
    assert!(
        instrs(&f).iter().any(|i| matches!(i, Instr::Call { .. })),
        "可能 panic 的调用必须留着"
    );
}

#[test]
fn t_ssa_61_间接调用必须留着() {
    // 被调方未知 ⇒ effect 只是保守估计，删它等于赌「它没副作用」
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let e = fb.entry();
    let fv = fb.value(t);
    let r = fb.value(t);
    fb.instr(
        e,
        Instr::Const {
            dst: fv,
            c: ConstVal::Int(0),
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Call {
            dsts: vec![r],
            callee: CalleeRef::Indirect(fv),
            args: Vec::new(),
            dots: false,
            effect: Effect::default(),
        },
    );
    fb.term(e, Term::Return(Vec::new()));
    let (mut f, tt) = fb.into_parts();
    check(&mut f, &tt);
    assert!(
        instrs(&f).iter().any(|i| matches!(i, Instr::Call { .. })),
        "间接调用必须留着"
    );
}

#[test]
fn t_ssa_61_不可达块被清掉() {
    // `fold` 把 `if true` 改成 `goto` 之后就会出现这种图（`V8` 会红），dce 收尾清它
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let b0 = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    fb.term(
        b0,
        Term::Goto {
            to: b1,
            args: Vec::new(),
        },
    );
    let v1 = fb.value(t);
    fb.instr(
        b1,
        Instr::Const {
            dst: v1,
            c: ConstVal::Int(1),
            typ: t,
        },
    );
    fb.term(b1, Term::Return(vec![v1]));
    let v2 = fb.value(t);
    fb.instr(
        b2,
        Instr::Const {
            dst: v2,
            c: ConstVal::Int(2),
            typ: t,
        },
    );
    fb.term(b2, Term::Return(vec![v2]));
    let (mut f, tt) = fb.into_parts();
    check(&mut f, &tt);
    assert_eq!(f.blocks.len(), 2, "b2 不可达 ⇒ 该被清掉");
}

#[test]
fn t_ssa_61_删链式死代码要到不动点() {
    // `a = 1; b = a + 1;`（b 没人用）⇒ 删 b 之后 a 也死了，要再来一轮
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let e = fb.entry();
    let a = fb.value(t);
    let b = fb.value(t);
    fb.instr(
        e,
        Instr::Const {
            dst: a,
            c: ConstVal::Int(1),
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Bin {
            dst: b,
            op: rgoc_hir::BinOp::Add,
            lhs: a,
            rhs: a,
            typ: t,
        },
    );
    fb.term(e, Term::Return(Vec::new()));
    let (mut f, tt) = fb.into_parts();
    check(&mut f, &tt);
    assert!(
        instrs(&f).is_empty(),
        "两条都该死：{}",
        rgoc_ssa::dump::dump_func(&f, &tt)
    );
    assert!(f.value_types.is_empty(), "值也要压实到空");
}
