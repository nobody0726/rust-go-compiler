//! 支配树（M6/Task 9；`V2` 的唯一前提）。
//!
//! # 为什么这个模块值得单独测
//!
//! 支配树写错的症状**不是「算不出来」**，而是「**算得太宽或太窄**」：
//!
//! * 太窄（漏边）⇒ `V2` 误拒**合法**图，看起来像构建器坏了；
//! * 太宽（多认）⇒ `V2` 放过非法图，verifier 变成摆设。
//!
//! 两种都不会崩，只会让结论错。所以这里的判据是**逐对要答案**：
//! 菱形（汇合）+ 交叉边（`b3 → b2`，一个兄弟块的边指向另一个兄弟的后继）——
//! 后者是 CHK 迭代最容易算错的一类。
//!
//! # 算法
//!
//! Cooper–Harvey–Kennedy：逆后序（RPO）编号 → `idom` 初值 `None`（入口为自己）
//! → 迭代到不动点（`intersect` 沿 RPO 向上走）。**不用** Lengauer-Tarjan（这里
//! 图很小，不动点更短且好读）。

use rgoc_hir::types::ConstVal;
use rgoc_ssa::cfg::Cfg;
use rgoc_ssa::dom::DomTree;
use rgoc_ssa::ir::{BlockId, Instr, Term};
use rgoc_ssa::testsupport::FnBuilder;

fn t() -> rgoc_types::TypeId {
    rgoc_types::TypeId::from_index(0)
}
fn b(i: u32) -> BlockId {
    BlockId(i)
}

/// 菱形：`b0 if → b1|b2`，两支 `goto b3`。
fn diamond() -> (rgoc_ssa::ir::SsaFunc, Cfg) {
    let mut fb = FnBuilder::new("diamond");
    let b0 = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let b3 = fb.block();
    let v0 = fb.value(t());
    let p = fb.value(t());
    fb.instr(
        b0,
        Instr::Const {
            dst: v0,
            c: ConstVal::Bool(true),
            typ: t(),
        },
    );
    fb.set_params(b3, &[p]);
    fb.term(
        b0,
        Term::If {
            cond: v0,
            then_: (b1, vec![]),
            else_: (b2, vec![]),
        },
    );
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
    fb.term(b3, Term::Return(vec![v0]));
    let f = fb.finish();
    let cfg = Cfg::build(&f);
    (f, cfg)
}

/// 交叉边：`b0 → b1`；`b1 if → b2 | b3`；`b2 goto b4`；`b3 goto b2`；`b4 return`。
///
/// `b3 → b2` 是「兄弟块指向另一个兄弟的后继」—— 支配关系最容易算错的那类边。
fn cross_edge() -> (rgoc_ssa::ir::SsaFunc, Cfg) {
    let mut fb = FnBuilder::new("cross");
    let b0 = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let b3 = fb.block();
    let b4 = fb.block();
    let v0 = fb.value(t());
    let p2 = fb.value(t());
    let p3 = fb.value(t());
    let p4 = fb.value(t());
    fb.instr(
        b0,
        Instr::Const {
            dst: v0,
            c: ConstVal::Bool(true),
            typ: t(),
        },
    );
    fb.set_params(b2, &[p2]);
    fb.set_params(b3, &[p3]);
    fb.set_params(b4, &[p4]);
    fb.term(
        b0,
        Term::Goto {
            to: b1,
            args: vec![v0],
        },
    );
    fb.term(
        b1,
        Term::If {
            cond: v0,
            then_: (b2, vec![v0]),
            else_: (b3, vec![v0]),
        },
    );
    fb.term(
        b2,
        Term::Goto {
            to: b4,
            args: vec![v0],
        },
    );
    fb.term(
        b3,
        Term::Goto {
            to: b2,
            args: vec![v0],
        },
    );
    fb.term(b4, Term::Return(vec![v0]));
    let f = fb.finish();
    let cfg = Cfg::build(&f);
    (f, cfg)
}

#[test]
fn t_ssa_03_菱形图的_直接支配() {
    let (f, cfg) = diamond();
    let dom = DomTree::build(&f, &cfg);

    assert_eq!(dom.idom(b(1)), Some(b(0)));
    assert_eq!(dom.idom(b(2)), Some(b(0)));
    assert_eq!(dom.idom(b(3)), Some(b(0)), "汇合块的 idom 是分叉点");
    assert_eq!(dom.idom(b(0)), None, "入口没有 idom");

    assert!(dom.dominates(b(0), b(3)), "入口支配全部");
    assert!(!dom.dominates(b(1), b(2)), "兄弟互不支配");
    assert!(dom.dominates(b(3), b(3)), "自己支配自己（按定义）");
}

#[test]
fn t_ssa_03_交叉边图的_直接支配() {
    let (f, cfg) = cross_edge();
    let dom = DomTree::build(&f, &cfg);

    assert_eq!(dom.idom(b(1)), Some(b(0)));
    assert_eq!(
        dom.idom(b(2)),
        Some(b(1)),
        "b2 的两条前驱 b1/b3 里，b1 更近"
    );
    assert_eq!(dom.idom(b(3)), Some(b(1)));
    assert_eq!(dom.idom(b(4)), Some(b(2)), "b4 只从 b2 来");

    assert!(dom.dominates(b(2), b(4)));
    assert!(!dom.dominates(b(3), b(4)), "b3 走不到 b4 却不支配它");
    assert!(dom.dominates(b(1), b(2)));
    assert!(
        !dom.dominates(b(2), b(3)),
        "b2 的边指向 b3? 不是 —— 是 b3 指向 b2"
    );
}
