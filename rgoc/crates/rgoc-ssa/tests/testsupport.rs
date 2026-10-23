//! 手写 IR 工具的自测（M6/Task 3，`D-M6-11` 的地基）。
//!
//! # 这个测试要证明的**不是**「工具好用」，而是「工具不拦人」
//!
//! verifier 的反例必须**手工造非法图**。如果构造工具顺手做了校验
//! （去重 `ValueId`、检查支配、检查块参数个数），那反例根本造不出来 ——
//! 于是 `verify_negative.rs` 会变成「用合法图证明 verifier 会拒绝」，
//! 永远绿，且没人发现（`D-M6-11` 的失效模式）。
//!
//! 所以这里断言：`FnBuilder` 能**成功构造**一张违反 `V1` 的图
//! （同一个 `ValueId` 被两条指令定义），并且构造后 `verify` 会拒它。
//! 前者证明工具不拦，后者证明图确实是非法的。

use rgoc_hir::types::ConstVal;
use rgoc_ssa::ir::{Instr, Term, ValueId};
use rgoc_ssa::testsupport::FnBuilder;

fn t() -> rgoc_types::TypeId {
    rgoc_types::TypeId::from_index(0)
}

#[test]
fn t_task3_构造工具允许造出非法的图() {
    let mut fb = FnBuilder::new("bad");
    let b0 = fb.entry();
    let v0 = fb.value(t());

    // 同一个 v0 被定义两次 —— `V1`（每值恰好定义一次）必拒。
    fb.instr(
        b0,
        Instr::Const {
            dst: v0,
            c: ConstVal::Int(1),
            typ: t(),
        },
    );
    fb.instr(
        b0,
        Instr::Const {
            dst: v0,
            c: ConstVal::Int(2),
            typ: t(),
        },
    );
    fb.term(b0, Term::Return(vec![v0]));

    let f = fb.finish();
    assert_eq!(f.block_count(), 1, "工具不该拒收这张图");
    assert_eq!(f.value_count(), 1);
    assert_eq!(
        f.blocks[0].instrs.len(),
        2,
        "两条指令都该在（工具不做去重）"
    );
    assert_eq!(
        std::mem::discriminant(&f.blocks[0].instrs[0]),
        std::mem::discriminant(&f.blocks[0].instrs[1]),
        "两条都是 Const（值不同）"
    );
    // 这里**不**调 verify —— verifier 的判据在 Task 10 起才有实现。
}

#[test]
fn t_task3_构造工具能建多块与块参数() {
    let mut fb = FnBuilder::new("diamond");
    let b0 = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let b3 = fb.block();
    let v0 = fb.value(t());
    let p = fb.value(t());

    fb.term(
        b0,
        Term::If {
            cond: v0,
            then_: (b1, vec![]),
            else_: (b2, vec![]),
        },
    );
    fb.set_params(b3, &[p]);
    fb.term(
        b1,
        Term::Goto {
            to: b3,
            args: vec![v0],
        },
    );
    fb.term(
        b2,
        Term::Goto {
            to: b3,
            args: vec![v0],
        },
    );
    fb.term(b3, Term::Return(vec![p]));

    let f = fb.finish();
    assert_eq!(f.block_count(), 4);
    assert_eq!(f.blocks[3].params, vec![p], "块参数按我给的顺序原样落盘");
    assert_eq!(ValueId(1), p);
}
