//! verifier 的正例面：**合法图必须过**（M6/Task 10 起）。
//!
//! # 为什么正例和反例一样重要
//!
//! 一个「见谁都拒」的 verifier 在反例测试里是 100% 绿的。只有正例能把它拉回来。
//! 这里的图全部**走得通**（每条路径都终止、每个值都有定义、每条边都有实参），
//! 而且刻意包含后续不变式要用的形状：块参数、回边、多结果调用。
//!
//! # 与 `cfg.rs` 的 fixture 不共用
//!
//! `cfg.rs` 的 `loop_cfg()` 里 `v0` 没有定义点（它只测 CFG 形状）。
//! 一旦共用，正例集里就藏着一张违反 `V1` 的图 —— 那时 `verify` 会红，
//! 而看的人会以为是 verifier 错了。**正例必须自己造，且自己就是合法的。**

use rgoc_hir::FuncId;
use rgoc_hir::types::ConstVal;
use rgoc_ssa::ir::{BlockId, CalleeRef, Effect, Instr, SsaFunc, Term, ValueId};
use rgoc_ssa::testsupport::FnBuilder;
use rgoc_ssa::verify::verify;

fn t() -> rgoc_types::TypeId {
    rgoc_types::TypeId::from_index(0)
}

fn ok(f: &SsaFunc) {
    if let Err(e) = verify(f) {
        panic!("合法图被拒：{e}\n{f:#?}");
    }
}

fn const_int(fb: &mut FnBuilder, blk: BlockId, v: ValueId, n: i128) {
    fb.instr(
        blk,
        Instr::Const {
            dst: v,
            c: ConstVal::Int(n),
            typ: t(),
        },
    );
}

#[test]
fn 直线函数() {
    let mut fb = FnBuilder::new("line");
    let b0 = fb.entry();
    let v0 = fb.value(t());
    let v1 = fb.value(t());
    const_int(&mut fb, b0, v0, 1);
    fb.instr(
        b0,
        Instr::Bin {
            dst: v1,
            op: rgoc_hir::BinOp::Add,
            lhs: v0,
            rhs: v0,
            typ: t(),
        },
    );
    fb.term(b0, Term::Return(vec![v1]));
    ok(&fb.finish());
}

#[test]
fn 菱形带块参数() {
    let mut fb = FnBuilder::new("diamond");
    let b0 = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let b3 = fb.block();
    let cond = fb.value(t());
    let p3 = fb.value(t());
    const_int(&mut fb, b0, cond, 1);
    fb.set_params(b3, &[p3]);
    fb.term(
        b0,
        Term::If {
            cond,
            then_: (b1, vec![]),
            else_: (b2, vec![]),
        },
    );
    fb.term(
        b1,
        Term::Goto {
            to: b3,
            args: vec![cond],
        },
    );
    fb.term(
        b2,
        Term::Goto {
            to: b3,
            args: vec![cond],
        },
    );
    fb.term(b3, Term::Return(vec![p3]));
    ok(&fb.finish());
}

#[test]
fn 循环带回边() {
    let mut fb = FnBuilder::new("loop");
    let b0 = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let b3 = fb.block();
    let zero = fb.value(t());
    let p1 = fb.value(t());
    let p2 = fb.value(t());
    const_int(&mut fb, b0, zero, 0);
    fb.set_params(b1, &[p1]);
    fb.set_params(b2, &[p2]);
    fb.term(
        b0,
        Term::Goto {
            to: b1,
            args: vec![zero],
        },
    );
    fb.term(
        b1,
        Term::If {
            cond: p1,
            then_: (b2, vec![p1]),
            else_: (b3, vec![]),
        },
    );
    fb.term(
        b2,
        Term::Goto {
            to: b1,
            args: vec![p2],
        },
    ); // 回边
    fb.term(b3, Term::Return(vec![p1]));
    ok(&fb.finish());
}

#[test]
fn 多结果调用与零结果调用() {
    let mut fb = FnBuilder::new("calls");
    let b0 = fb.entry();
    let a = fb.value(t());
    let r0 = fb.value(t());
    let r1 = fb.value(t());
    const_int(&mut fb, b0, a, 3);
    // 两结果
    fb.instr(
        b0,
        Instr::Call {
            dsts: vec![r0, r1],
            callee: CalleeRef::Func(FuncId(1)),
            args: vec![a],
            effect: Effect::default(),
        },
    );
    // 零结果（void 调用）
    fb.instr(
        b0,
        Instr::Call {
            dsts: vec![],
            callee: CalleeRef::Func(FuncId(2)),
            args: vec![r0],
            effect: Effect::default(),
        },
    );
    fb.term(b0, Term::Return(vec![r1]));
    ok(&fb.finish());
}

#[test]
fn 函数形参就是入口块参数() {
    let mut fb = FnBuilder::new("with-params");
    let b0 = fb.entry();
    let p0 = fb.value(t());
    let p1 = fb.value(t());
    fb.set_params(b0, &[p0, p1]);
    fb.term(b0, Term::Return(vec![p0, p1]));
    ok(&fb.finish());
}
