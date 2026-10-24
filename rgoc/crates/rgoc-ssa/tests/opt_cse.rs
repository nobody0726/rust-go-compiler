//! 块内公共子表达式消除（`T-SSA-63`；`M6-plan.md` Task 44）。
//!
//! # 判据的两半
//!
//! * **合得掉**：同一个块里 `(op, 操作数, 类型)` 相同的纯指令只留一条；
//! * **不许跨块**：两个块各算一遍同样的表达式，**不**合 —— 跨块要可用表达式分析，
//!   那是 M7 的事。这条反例是「只做块内」这个边界的唯一证据。

use rgoc_hir::BinOp;
use rgoc_hir::types::ConstVal;
use rgoc_ssa::ir::{Instr, SsaFunc, Term};
use rgoc_ssa::opt::cse;
use rgoc_ssa::testsupport::FnBuilder;
use rgoc_ssa::verify::verify;
use rgoc_types::TypeTable;

fn check(f: &mut SsaFunc, tt: &TypeTable) {
    cse::run(f);
    verify(f, tt).expect("cse 之后必须仍然合法");
}

fn bins(f: &SsaFunc) -> usize {
    f.blocks
        .iter()
        .flat_map(|b| &b.instrs)
        .filter(|i| matches!(i, Instr::Bin { .. }))
        .count()
}

#[test]
fn t_ssa_63_块内相同表达式合一() {
    // v0=1, v1=2, v2=v0+v1, v3=v0+v1, return v2+v3
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let e = fb.entry();
    let v0 = fb.value(t);
    let v1 = fb.value(t);
    let v2 = fb.value(t);
    let v3 = fb.value(t);
    let r = fb.value(t);
    fb.instr(
        e,
        Instr::Const {
            dst: v0,
            c: ConstVal::Int(1),
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Const {
            dst: v1,
            c: ConstVal::Int(2),
            typ: t,
        },
    );
    for d in [v2, v3] {
        fb.instr(
            e,
            Instr::Bin {
                dst: d,
                op: BinOp::Add,
                lhs: v0,
                rhs: v1,
                typ: t,
            },
        );
    }
    fb.instr(
        e,
        Instr::Bin {
            dst: r,
            op: BinOp::Add,
            lhs: v2,
            rhs: v3,
            typ: t,
        },
    );
    fb.term(e, Term::Return(vec![r]));
    let (mut f, tt) = fb.into_parts();
    check(&mut f, &tt);
    assert_eq!(bins(&f), 2, "两条 `v0+v1` 该合成一条（剩下的是 r 那条）");
    // 第三条的两个操作数都该指向同一条结果
    let last = f.blocks[0].instrs.last().expect("有指令");
    let Instr::Bin { lhs, rhs, .. } = last else {
        panic!("最后一条该是 Bin：{last:?}");
    };
    assert_eq!(
        lhs, rhs,
        "合完之后 `v2+v3` 的两个操作数是同一个值：{last:?}"
    );
}

#[test]
fn t_ssa_63_跨块不合() {
    // b0: v2 = v0+v1 ; goto b1
    // b1: v3 = v0+v1 ; return v3
    // 两条都在**不同块**里 ⇒ 都留着（块内 CSE 不做跨块）
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let b0 = fb.entry();
    let b1 = fb.block();
    let v0 = fb.value(t);
    let v1 = fb.value(t);
    let v2 = fb.value(t);
    let v3 = fb.value(t);
    fb.instr(
        b0,
        Instr::Const {
            dst: v0,
            c: ConstVal::Int(1),
            typ: t,
        },
    );
    fb.instr(
        b0,
        Instr::Const {
            dst: v1,
            c: ConstVal::Int(2),
            typ: t,
        },
    );
    fb.instr(
        b0,
        Instr::Bin {
            dst: v2,
            op: BinOp::Add,
            lhs: v0,
            rhs: v1,
            typ: t,
        },
    );
    fb.term(
        b0,
        Term::Goto {
            to: b1,
            args: Vec::new(),
        },
    );
    fb.instr(
        b1,
        Instr::Bin {
            dst: v3,
            op: BinOp::Add,
            lhs: v0,
            rhs: v1,
            typ: t,
        },
    );
    fb.term(b1, Term::Return(vec![v3]));
    let (mut f, tt) = fb.into_parts();
    check(&mut f, &tt);
    assert_eq!(bins(&f), 2, "跨块不许合（那是可用表达式分析，属 M7）");
}

#[test]
fn t_ssa_63_操作数不同就不合() {
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let e = fb.entry();
    let v0 = fb.value(t);
    let v1 = fb.value(t);
    let a = fb.value(t);
    let b = fb.value(t);
    let r = fb.value(t);
    fb.instr(
        e,
        Instr::Const {
            dst: v0,
            c: ConstVal::Int(1),
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Const {
            dst: v1,
            c: ConstVal::Int(2),
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Bin {
            dst: a,
            op: BinOp::Add,
            lhs: v0,
            rhs: v1,
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Bin {
            dst: b,
            op: BinOp::Add,
            lhs: v1,
            rhs: v0,
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Bin {
            dst: r,
            op: BinOp::Add,
            lhs: a,
            rhs: b,
            typ: t,
        },
    );
    fb.term(e, Term::Return(vec![r]));
    let (mut f, tt) = fb.into_parts();
    check(&mut f, &tt);
    assert_eq!(
        bins(&f),
        3,
        "`v0+v1` 与 `v1+v0` 不合并（不做交换律：浮点位级不保证）"
    );
}

#[test]
fn t_ssa_63_同操作数不同目标类型的转换不合() {
    // `Convert` 的 `typ` 是**语义的一部分**：`int32(v)` 与 `int64(v)` 不是同一个表达式。
    // （`Bin` 不需要这条判据 —— `V3` 要求它的结果类型等于操作数类型，`typ` 冗余。）
    let mut fb = FnBuilder::new("f");
    let i32t = fb.basic("int32");
    let i64t = fb.basic("int64");
    let e = fb.entry();
    let v = fb.value(i64t);
    let a = fb.value(i32t);
    let b = fb.value(i32t);
    let r = fb.value(i32t);
    fb.instr(
        e,
        Instr::Const {
            dst: v,
            c: ConstVal::Int(7),
            typ: i64t,
        },
    );
    fb.instr(
        e,
        Instr::Convert {
            dst: a,
            x: v,
            typ: i32t,
        },
    );
    fb.instr(
        e,
        Instr::Convert {
            dst: b,
            x: v,
            typ: i32t,
        },
    );
    fb.instr(
        e,
        Instr::Bin {
            dst: r,
            op: BinOp::Add,
            lhs: a,
            rhs: b,
            typ: i32t,
        },
    );
    fb.term(e, Term::Return(vec![r]));
    let (mut f, tt) = fb.into_parts();
    check(&mut f, &tt);
    let converts = f.blocks[0]
        .instrs
        .iter()
        .filter(|i| matches!(i, Instr::Convert { .. }))
        .count();
    assert_eq!(converts, 1, "同一个转换该合一");
}
