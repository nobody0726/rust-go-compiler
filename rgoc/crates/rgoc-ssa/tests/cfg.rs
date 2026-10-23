//! `T-SSA-05/09` 的底座：CFG 的边、可达性与边上实参（M6/Task 8）。
//!
//! # 三条判据
//!
//! 1. **前驱/后继互逆**：`b ∈ succs(a)` ⟺ `a ∈ preds(b)`（`V4` 的原料）；
//! 2. **可达性**：从入口 BFS；不可达标 `is_dead`（`V8` 的原料）；
//! 3. **边上实参**：`edge_args(from, to)` 就是终止符里那串 `Vec<ValueId>`
//!    （`V7` 的比对面 —— 数它，别数「谁看起来像 phi」）。
//!
//! # 为什么用菱形 + 循环两张图
//!
//! 菱形测「汇合」（两条边进同一块），循环测「回边」（一条边指回祖先）。
//! 只测其中一种会漏掉另一类的排序/去重 bug —— M3 就吃过「只测前向 goto」的亏。

use rgoc_hir::types::ConstVal;
use rgoc_ssa::cfg::Cfg;
use rgoc_ssa::ir::{BlockId, Instr, Term, ValueId};
use rgoc_ssa::testsupport::FnBuilder;

fn t() -> rgoc_types::TypeId {
    rgoc_types::TypeId::from_index(0)
}

fn b(i: u32) -> BlockId {
    BlockId(i)
}

/// 菱形：`b0 if → b1 | b2`，两支都 goto `b3`，`b3 return`。
fn diamond() -> rgoc_ssa::ir::SsaFunc {
    let mut fb = FnBuilder::new("diamond");
    let b0 = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let b3 = fb.block();
    let v0 = fb.value(t());
    let p3 = fb.value(t());
    fb.instr(
        b0,
        Instr::Const {
            dst: v0,
            c: ConstVal::Bool(true),
            typ: t(),
        },
    );
    fb.set_params(b3, &[p3]);
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
    fb.finish()
}

/// 循环：`b0 → b1`；`b1 if → b2 | b3`；`b2 goto b1`（**回边**）；`b3 return`。
fn loop_cfg() -> rgoc_ssa::ir::SsaFunc {
    let mut fb = FnBuilder::new("loop");
    let b0 = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let b3 = fb.block();
    let v0 = fb.value(t());
    let p1 = fb.value(t());
    fb.set_params(b1, &[p1]);
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
            then_: (b2, vec![]),
            else_: (b3, vec![]),
        },
    );
    fb.term(
        b2,
        Term::Goto {
            to: b1,
            args: vec![v0],
        },
    );
    fb.term(b3, Term::Return(vec![v0]));
    fb.finish()
}

#[test]
fn t_ssa_05_前驱后继互逆() {
    let f = diamond();
    let cfg = Cfg::build(&f);

    assert_eq!(cfg.succs(b(0)), &[b(1), b(2)][..], "if 的两个后继按源码序");
    assert!(cfg.preds(b(0)).is_empty(), "入口无前驱");
    assert_eq!(cfg.preds(b(3)), &[b(1), b(2)][..], "汇合块两条前驱");
    assert!(cfg.succs(b(3)).is_empty(), "return 无后继");

    // 互逆性：对每条边双向查
    for from in 0..4u32 {
        for to in cfg.succs(b(from)).iter().copied() {
            assert!(
                cfg.preds(to).contains(&b(from)),
                "b{from} 的后继里有 b{}，但 b{} 的前驱里没有 b{from}",
                to.0,
                to.0
            );
        }
    }
}

#[test]
fn t_ssa_05_回边被看见() {
    let f = loop_cfg();
    let cfg = Cfg::build(&f);
    assert_eq!(
        cfg.preds(b(1)),
        &[b(0), b(2)][..],
        "b1 有两条前驱（入口 + 回边）"
    );
    assert!(cfg.succs(b(2)).contains(&b(1)), "b2 的边指回 b1");
}

#[test]
fn t_ssa_09_不可达块被标出来() {
    let mut f = diamond();
    // 追加一个没有任何前驱的块（悬挂死块）
    f.blocks.push(Default::default());

    let cfg = Cfg::build(&f);
    assert!(cfg.reachable(b(3)), "汇合块可达");
    assert!(!cfg.reachable(b(4)), "悬挂块不可达");
    assert!(cfg.is_dead(b(4)), "is_dead = 不可达");
    assert!(!cfg.is_dead(b(0)), "入口不是死的");
}

#[test]
fn t_ssa_05_边上实参就是终止符里那一串() {
    let f = diamond();
    let cfg = Cfg::build(&f);
    let v0 = ValueId(0);
    assert_eq!(cfg.edge_args(b(1), b(3)), &[v0][..]);
    assert_eq!(cfg.edge_args(b(2), b(3)), &[v0][..]);
    assert_eq!(cfg.edge_args(b(0), b(1)), &[][..], "if 的假分支没带参");
    assert_eq!(cfg.edge_args(b(3), b(0)), &[][..], "不存在的边返回空");
}
