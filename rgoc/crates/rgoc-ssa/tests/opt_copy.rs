//! 复制传播（`T-SSA-62`；`M6-plan.md` Task 43）。
//!
//! # 判据的两半
//!
//! * **消得掉**：`Copy` 链要一路解析到终点（`v2=copy v0; v3=copy v2` ⇒ 都用 `v0`）；
//! * **不许破并行**：块参数的边实参是**同时**赋值的，`a,b=b,a` 不能变成「两个都是 b」。
//!
//! 第二半靠「先建全表、再同时替换」保证：映射是一次性快照，替换顺序无关。

use rgoc_hir::types::ConstVal;
use rgoc_ssa::ir::{Instr, SsaFunc, Term, ValueId};
use rgoc_ssa::opt::copy;
use rgoc_ssa::testsupport::FnBuilder;
use rgoc_ssa::verify::verify;
use rgoc_types::TypeTable;

fn check(f: &mut SsaFunc, tt: &TypeTable) {
    copy::run(f);
    verify(f, tt).expect("copy 之后必须仍然合法");
}

fn copies(f: &SsaFunc) -> usize {
    f.blocks
        .iter()
        .flat_map(|b| &b.instrs)
        .filter(|i| matches!(i, Instr::Copy { .. }))
        .count()
}

#[test]
fn t_ssa_62_链式_copy_一路消到终点() {
    // v0 = 5; v1 = copy v0; v2 = copy v1; v3 = v2 + v0; return v3
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let e = fb.entry();
    let v0 = fb.value(t);
    let v1 = fb.value(t);
    let v2 = fb.value(t);
    let v3 = fb.value(t);
    fb.instr(
        e,
        Instr::Const {
            dst: v0,
            c: ConstVal::Int(5),
            typ: t,
        },
    );
    fb.instr(e, Instr::Copy { dst: v1, src: v0 });
    fb.instr(e, Instr::Copy { dst: v2, src: v1 });
    fb.instr(
        e,
        Instr::Bin {
            dst: v3,
            op: rgoc_hir::BinOp::Add,
            lhs: v2,
            rhs: v0,
            typ: t,
        },
    );
    fb.term(e, Term::Return(vec![v3]));
    let (mut f, tt) = fb.into_parts();
    check(&mut f, &tt);
    assert_eq!(copies(&f), 0, "Copy 全消");
    // 压实之后：v0 = 5（Const），v1 = add v0 v0
    let ins = &f.blocks[0].instrs;
    assert!(
        matches!(
            &ins[1],
            Instr::Bin { lhs, rhs, .. } if *lhs == ValueId(0) && *rhs == ValueId(0)
        ),
        "两个操作数都该指向源 v0：{:?}",
        ins[1]
    );
}

#[test]
fn t_ssa_62_边实参的并行语义不许破() {
    // `x, y = y, x`：b0 里 v2=copy v0、v3=copy v1，goto b1 (v3, v2) —— 即 (2, 1)。
    // 消解之后实参必须还是 (v1, v0)，不能变成 (v1, v1) 或 (v0, v0)。
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let b0 = fb.entry();
    let b1 = fb.block();
    let v0 = fb.value(t);
    let v1 = fb.value(t);
    let v2 = fb.value(t);
    let v3 = fb.value(t);
    let p0 = fb.value(t);
    let p1 = fb.value(t);
    fb.set_params(b1, &[p0, p1]);
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
    fb.instr(b0, Instr::Copy { dst: v2, src: v0 });
    fb.instr(b0, Instr::Copy { dst: v3, src: v1 });
    fb.term(
        b0,
        Term::Goto {
            to: b1,
            args: vec![v3, v2],
        },
    );
    fb.term(b1, Term::Return(vec![p0, p1]));
    let (mut f, tt) = fb.into_parts();
    check(&mut f, &tt);
    let Term::Goto { args, .. } = &f.blocks[0].term else {
        panic!("b0 该是 goto：{:?}", f.blocks[0].term);
    };
    // 压实之后：v0=1、v1=2 仍是前两个值
    assert_eq!(
        args,
        &vec![ValueId(1), ValueId(0)],
        "实参该是 (2, 1)（v1, v0）—— 交换语义不许被消解破坏：{:?}",
        args
    );
}

#[test]
fn t_ssa_62_没有_copy_时是空操作() {
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let e = fb.entry();
    let v = fb.value(t);
    fb.instr(
        e,
        Instr::Const {
            dst: v,
            c: ConstVal::Int(1),
            typ: t,
        },
    );
    fb.term(e, Term::Return(vec![v]));
    let (mut f, tt) = fb.into_parts();
    let before = rgoc_ssa::dump::dump_func(&f, &tt);
    check(&mut f, &tt);
    assert_eq!(
        rgoc_ssa::dump::dump_func(&f, &tt),
        before,
        "没 Copy 就不该变"
    );
}

#[test]
fn t_ssa_62_自拷贝也消掉() {
    // `v1 = copy v1` 这种形态构建期不会产，但 pass 不能因此崩
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let e = fb.entry();
    let v = fb.value(t);
    let p = fb.value(t);
    fb.set_params(e, &[p]);
    fb.instr(
        e,
        Instr::Const {
            dst: v,
            c: ConstVal::Int(1),
            typ: t,
        },
    );
    fb.instr(e, Instr::Copy { dst: p, src: p });
    fb.term(e, Term::Return(vec![v]));
    let (mut f, _tt) = fb.into_parts();
    // 输入本身违反 V1（p 被重复定义），所以**不**先 verify —— 只看 pass 不崩
    copy::run(&mut f);
    assert_eq!(copies(&f), 0);
}
