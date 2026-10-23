//! verifier 的反例面（**手写 IR**，M6/Task 10 起；`D-M6-11`）。
//!
//! # 为什么每条反例都必须手写
//!
//! 如果反例由构建器产出，就变成「用被检对象的输出喂给检查器」：
//! 构建器一旦有 bug（比如它自己就不再产出重复定义），反例根本造不出来，
//! verifier 于是**永远绿**。手写 IR 是唯一能让 `V1..V9` 各自有牙的方式。
//!
//! # 断言纪律
//!
//! 每条反例除了「必须被拒」，还要断言**拒在哪条不变式**上。
//! 只断言 `is_err()` 会让「V4 顺手把 V1 的图也拒了」这种串味混过去。
//!
//! # 位置口径
//!
//! `VerifyError::idx` 是**块内位置序号**：先块参数（`0..params.len()`），
//! 再指令（`params.len()..`）。这样「参数 vs 指令」不需要一个哨兵值。

use rgoc_hir::types::ConstVal;
use rgoc_ssa::ir::{BlockId, Instr, SsaFunc, Term, ValueId};
use rgoc_ssa::testsupport::FnBuilder;
use rgoc_ssa::verify::{INVARIANTS, verify};

fn t() -> rgoc_types::TypeId {
    rgoc_types::TypeId::from_index(0)
}

fn b(i: u32) -> BlockId {
    BlockId(i)
}

/// 断言被拒，且拒在 `want` 这条不变式上；返回渲染后的错误文本供进一步断言。
fn rejected(f: &SsaFunc, want: &str) -> String {
    match verify(f) {
        Ok(()) => panic!("期望 Err({want})，实际 Ok —— 这条反例没有牙"),
        Err(e) => {
            assert_eq!(e.invariant, want, "拒错了不变式：{e}");
            assert!(
                !e.detail.is_empty(),
                "detail 不能为空（否则断言只能看到编号）"
            );
            e.to_string()
        }
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
fn 九条不变式的编号与顺序固定() {
    assert_eq!(
        INVARIANTS,
        ["V1", "V2", "V3", "V4", "V5", "V6", "V7", "V8", "V9"],
        "编号顺序进了 dump 与断言，不许重排"
    );
}

// ── V1：每个值恰好定义一次 ──────────────────────────────────────────

#[test]
fn t_ssa_02_v1_同一值被两条指令定义() {
    let mut fb = FnBuilder::new("dup");
    let b0 = fb.entry();
    let v0 = fb.value(t());
    const_int(&mut fb, b0, v0, 1);
    const_int(&mut fb, b0, v0, 2); // 第二次定义
    fb.term(b0, Term::Return(vec![v0]));
    let f = fb.finish();

    let msg = rejected(&f, "V1");
    assert!(msg.contains("v0"), "错误必须点名是哪个值：{msg}");
    assert!(
        msg.contains("b0[1]"),
        "错误必须指向第二次定义的位置（b0 的第 1 个位置）：{msg}"
    );
}

#[test]
fn t_ssa_02_v1_块参数与指令抢同一个值() {
    let mut fb = FnBuilder::new("param-clash");
    let b0 = fb.entry();
    let b1 = fb.block();
    let v0 = fb.value(t());
    let p1 = fb.value(t());
    const_int(&mut fb, b0, v0, 7);
    fb.set_params(b1, &[v0]); // v0 又被当成块参数定义一次
    fb.term(
        b0,
        Term::Goto {
            to: b1,
            args: vec![v0],
        },
    );
    fb.term(b1, Term::Return(vec![p1]));
    let f = fb.finish();

    let msg = rejected(&f, "V1");
    assert!(msg.contains("块参数"), "要说明第二次定义来自块参数：{msg}");
}

#[test]
fn t_ssa_02_v1_值从未被定义() {
    let mut fb = FnBuilder::new("undef");
    let b0 = fb.entry();
    let v0 = fb.value(t());
    let v1 = fb.value(t());
    const_int(&mut fb, b0, v0, 1);
    fb.term(b0, Term::Return(vec![v1])); // v1 没有任何定义点
    let f = fb.finish();

    let msg = rejected(&f, "V1");
    assert!(msg.contains("v1"), "错误必须点名未定义的值：{msg}");
}

#[test]
fn t_ssa_02_v1_目标值越界() {
    let mut fb = FnBuilder::new("oob");
    let b0 = fb.entry();
    let v0 = fb.value(t());
    // 只分配了 1 个值（v0），却让一条指令定义 v9
    fb.instr(
        b0,
        Instr::Const {
            dst: ValueId(9),
            c: ConstVal::Int(1),
            typ: t(),
        },
    );
    fb.term(b0, Term::Return(vec![v0]));
    let f = fb.finish();

    let msg = rejected(&f, "V1");
    assert!(msg.contains('9'), "要报出越界的那个编号：{msg}");
}

// ── V4：前驱/后继互逆、入口无前驱 ──────────────────────────────────

#[test]
fn t_ssa_05_v4_入口块有前驱() {
    let mut fb = FnBuilder::new("entry-pred");
    let b0 = fb.entry();
    let b1 = fb.block();
    let v0 = fb.value(t());
    const_int(&mut fb, b0, v0, 1);
    fb.term(
        b0,
        Term::Goto {
            to: b1,
            args: vec![],
        },
    );
    fb.term(
        b1,
        Term::Goto {
            to: b0,
            args: vec![],
        },
    ); // 回到入口 ⇒ 入口有了前驱
    let f = fb.finish();

    let msg = rejected(&f, "V4");
    assert!(msg.contains("入口"), "要说明是入口块的问题：{msg}");
}

#[test]
fn t_ssa_05_v4_边指向不存在的块() {
    let mut fb = FnBuilder::new("dangling");
    let b0 = fb.entry();
    let v0 = fb.value(t());
    const_int(&mut fb, b0, v0, 1);
    fb.term(
        b0,
        Term::Goto {
            to: b(7),
            args: vec![],
        },
    );
    let f = fb.finish();

    let msg = rejected(&f, "V4");
    assert!(
        msg.contains("不存在的 b7"),
        "要报成「目标块不存在」，而不是落到互逆检查上（两者都得留，但诊断要准）：{msg}"
    );
}

#[test]
fn t_ssa_05_v4_同一目标两条边实参不同() {
    let mut fb = FnBuilder::new("dup-edge");
    let b0 = fb.entry();
    let b1 = fb.block();
    let v0 = fb.value(t());
    let v1 = fb.value(t());
    const_int(&mut fb, b0, v0, 1);
    const_int(&mut fb, b0, v1, 2);
    // then 与 else 指向**同一块**但给不同实参：IR 里无法表达「走的是哪条边」
    fb.term(
        b0,
        Term::If {
            cond: v0,
            then_: (b1, vec![v0]),
            else_: (b1, vec![v1]),
        },
    );
    fb.term(b1, Term::Return(vec![v0]));
    let f = fb.finish();

    let msg = rejected(&f, "V4");
    assert!(msg.contains("实参"), "要说明冲突在实参上：{msg}");
}

// ── V8：可达性 ─────────────────────────────────────────────────────

#[test]
fn t_ssa_09_v8_悬挂死块() {
    let mut fb = FnBuilder::new("dead");
    let b0 = fb.entry();
    let v0 = fb.value(t());
    const_int(&mut fb, b0, v0, 1);
    fb.term(b0, Term::Return(vec![v0]));
    let b1 = fb.block(); // 没有任何前驱
    fb.term(b1, Term::Return(vec![v0]));
    let f = fb.finish();

    let msg = rejected(&f, "V8");
    assert!(msg.contains("b1"), "要报出死块编号：{msg}");
}

#[test]
fn t_ssa_09_v8_自环孤岛也是死块() {
    let mut fb = FnBuilder::new("island");
    let b0 = fb.entry();
    let v0 = fb.value(t());
    const_int(&mut fb, b0, v0, 1);
    fb.term(b0, Term::Return(vec![v0]));
    let b1 = fb.block();
    fb.term(
        b1,
        Term::Goto {
            to: b1,
            args: vec![],
        },
    ); // 自环：有前驱但不可达
    let f = fb.finish();

    let msg = rejected(&f, "V8");
    assert!(msg.contains("b1"), "要报出孤岛编号：{msg}");
}

// ── 定位与优先级 ───────────────────────────────────────────────────

#[test]
fn 错误带得动位置() {
    let mut fb = FnBuilder::new("pos");
    let b0 = fb.entry();
    let b1 = fb.block();
    let v0 = fb.value(t());
    const_int(&mut fb, b0, v0, 1);
    const_int(&mut fb, b0, v0, 2);
    fb.term(
        b0,
        Term::Goto {
            to: b1,
            args: vec![],
        },
    );
    fb.term(b1, Term::Return(vec![v0]));
    let f = fb.finish();

    let e = verify(&f).expect_err("必须被拒");
    assert_eq!(e.block, b0, "第二次定义在 b0");
    assert_eq!(e.idx, 1, "块内位置 0 是第一条 const，1 是第二条");
}

#[test]
fn 先报_v1_再报_v4_顺序稳定() {
    // 同时违反 V1（重复定义）与 V4（边指向 b9）—— 必须只报 V1，
    // 否则同一张图在不同实现下报不同码，断言无法锚定。
    let mut fb = FnBuilder::new("both");
    let b0 = fb.entry();
    let v0 = fb.value(t());
    const_int(&mut fb, b0, v0, 1);
    const_int(&mut fb, b0, v0, 2);
    fb.term(
        b0,
        Term::Goto {
            to: b(9),
            args: vec![],
        },
    );
    let f = fb.finish();

    assert_eq!(verify(&f).expect_err("必须被拒").invariant, "V1");
}
