//! verifier 的正例面：**合法图必须过**（M6/Task 10 起）。
//!
//! # 为什么正例和反例一样重要
//!
//! 一个「见谁都拒」的 verifier 在反例测试里是 100% 绿的。只有正例能把它拉回来。
//! 这里的图全部**走得通**（每条路径都终止、每个值都有定义、每条边都有实参，
//! 且类型逐个自洽），而且刻意包含后续不变式要用的形状：块参数、回边、多结果调用。
//!
//! # 与 `cfg.rs` 的 fixture 不共用
//!
//! `cfg.rs` 的 `loop_cfg()` 里 `v0` 没有定义点（它只测 CFG 形状）。
//! 一旦共用，正例集里就藏着一张违反 `V1` 的图 —— 那时 `verify` 会红，
//! 而看的人会以为是 verifier 错了。**正例必须自己造，且自己就是合法的。**

use rgoc_hir::types::ConstVal;
use rgoc_ssa::ir::{BlockId, CalleeRef, Effect, Instr, SsaFunc, Term, ValueId};
use rgoc_ssa::testsupport::FnBuilder;
use rgoc_ssa::verify::verify as verify_with;
use rgoc_types::{TypeId, TypeTable};

fn ok(f: &SsaFunc, tt: &TypeTable) {
    if let Err(e) = verify_with(f, tt) {
        panic!("合法图被拒：{e}\n{f:#?}");
    }
}

fn const_int(fb: &mut FnBuilder, blk: BlockId, v: ValueId, n: i128, ti: TypeId) {
    fb.instr(
        blk,
        Instr::Const {
            dst: v,
            c: ConstVal::Int(n),
            typ: ti,
        },
    );
}

fn const_bool(fb: &mut FnBuilder, blk: BlockId, v: ValueId, x: bool, tb: TypeId) {
    fb.instr(
        blk,
        Instr::Const {
            dst: v,
            c: ConstVal::Bool(x),
            typ: tb,
        },
    );
}

#[test]
fn 直线函数() {
    let mut fb = FnBuilder::new("line");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let v0 = fb.value(ti);
    let v1 = fb.value(ti);
    const_int(&mut fb, b0, v0, 1, ti);
    fb.instr(
        b0,
        Instr::Bin {
            dst: v1,
            op: rgoc_hir::BinOp::Add,
            lhs: v0,
            rhs: v0,
            typ: ti,
        },
    );
    fb.term(b0, Term::Return(vec![v1]));
    let (f, tt) = fb.into_parts();
    ok(&f, &tt);
}

#[test]
fn 菱形带块参数() {
    let mut fb = FnBuilder::new("diamond");
    let ti = fb.basic("int");
    let tb = fb.basic("bool");
    let b0 = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let b3 = fb.block();
    let cond = fb.value(tb);
    let one = fb.value(ti);
    let p3 = fb.value(ti);
    const_bool(&mut fb, b0, cond, true, tb);
    const_int(&mut fb, b0, one, 1, ti);
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
            args: vec![one],
        },
    );
    fb.term(
        b2,
        Term::Goto {
            to: b3,
            args: vec![one],
        },
    );
    fb.term(b3, Term::Return(vec![p3]));
    let (f, tt) = fb.into_parts();
    ok(&f, &tt);
}

#[test]
fn 循环带回边() {
    let mut fb = FnBuilder::new("loop");
    let ti = fb.basic("int");
    let tb = fb.basic("bool");
    let b0 = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let b3 = fb.block();
    let zero = fb.value(ti);
    let tr = fb.value(tb);
    let p1 = fb.value(ti);
    let p2 = fb.value(ti);
    const_int(&mut fb, b0, zero, 0, ti);
    const_bool(&mut fb, b0, tr, true, tb);
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
            cond: tr,
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
    let (f, tt) = fb.into_parts();
    ok(&f, &tt);
}

#[test]
fn 多结果调用与零结果调用() {
    let mut fb = FnBuilder::new("calls");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let a = fb.value(ti);
    let r0 = fb.value(ti);
    let r1 = fb.value(ti);
    const_int(&mut fb, b0, a, 3, ti);
    // 两结果
    fb.instr(
        b0,
        Instr::Call {
            dsts: vec![r0, r1],
            callee: CalleeRef::Func(rgoc_hir::FuncId(1)),
            args: vec![a],
            effect: Effect::default(),
        },
    );
    // 零结果（void 调用）
    fb.instr(
        b0,
        Instr::Call {
            dsts: vec![],
            callee: CalleeRef::Func(rgoc_hir::FuncId(2)),
            args: vec![r0],
            effect: Effect::default(),
        },
    );
    fb.term(b0, Term::Return(vec![r1]));
    let (f, tt) = fb.into_parts();
    ok(&f, &tt);
}

#[test]
fn 函数形参就是入口块参数() {
    let mut fb = FnBuilder::new("with-params");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let p0 = fb.value(ti);
    let p1 = fb.value(ti);
    fb.set_params(b0, &[p0, p1]);
    fb.term(b0, Term::Return(vec![p0, p1]));
    let (f, tt) = fb.into_parts();
    ok(&f, &tt);
}

#[test]
fn 字符串相加是合法的() {
    // V3 只查自洽：`string + string` 两侧同型 ⇒ 合法。
    // 「`%` 不能用于浮点」这类**合法性**归检查器，不在这里重复判。
    let mut fb = FnBuilder::new("strcat");
    let ts = fb.basic("string");
    let b0 = fb.entry();
    let a = fb.value(ts);
    let c = fb.value(ts);
    let d = fb.value(ts);
    fb.instr(
        b0,
        Instr::Const {
            dst: a,
            c: ConstVal::Str("a".to_string()),
            typ: ts,
        },
    );
    fb.instr(
        b0,
        Instr::Const {
            dst: c,
            c: ConstVal::Str("c".to_string()),
            typ: ts,
        },
    );
    fb.instr(
        b0,
        Instr::Bin {
            dst: d,
            op: rgoc_hir::BinOp::Add,
            lhs: a,
            rhs: c,
            typ: ts,
        },
    );
    fb.term(b0, Term::Return(vec![d]));
    let (f, tt) = fb.into_parts();
    ok(&f, &tt);
}

#[test]
fn 移位计数可以是另一种整数型() {
    // `int64 << uint`：官方只要求计数是整数（`T-EX-60` 踩过这条）。
    let mut fb = FnBuilder::new("shift");
    let ti64 = fb.basic("int64");
    let tu = fb.basic("uint");
    let b0 = fb.entry();
    let x = fb.value(ti64);
    let n = fb.value(tu);
    let z = fb.value(ti64);
    const_int(&mut fb, b0, x, 1, ti64);
    const_int(&mut fb, b0, n, 3, tu);
    fb.instr(
        b0,
        Instr::Bin {
            dst: z,
            op: rgoc_hir::BinOp::Shl,
            lhs: x,
            rhs: n,
            typ: ti64,
        },
    );
    fb.term(b0, Term::Return(vec![z]));
    let (f, tt) = fb.into_parts();
    ok(&f, &tt);
}

#[test]
fn 比较的结果是_bool() {
    let mut fb = FnBuilder::new("cmp");
    let ti = fb.basic("int");
    let tb = fb.basic("bool");
    let b0 = fb.entry();
    let x = fb.value(ti);
    let y = fb.value(ti);
    let z = fb.value(tb);
    const_int(&mut fb, b0, x, 1, ti);
    const_int(&mut fb, b0, y, 2, ti);
    fb.instr(
        b0,
        Instr::Bin {
            dst: z,
            op: rgoc_hir::BinOp::Lt,
            lhs: x,
            rhs: y,
            typ: tb,
        },
    );
    fb.term(b0, Term::Return(vec![z]));
    let (f, tt) = fb.into_parts();
    ok(&f, &tt);
}
