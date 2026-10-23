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
//!
//! # 类型口径（Task 12 起）
//!
//! `V3` 要解析真实类型，所以 fixture 必须用**真的**基本类型（`fb.basic("int")`），
//! 不能拿 `TypeId::from_index(0)` 糊过去 —— 那会解析成 `bool` 或 `invalid`，
//! 反例就变成在测「类型表里的第 0 项是什么」。

use rgoc_hir::types::ConstVal;
use rgoc_ssa::ir::{BlockId, Instr, SsaFunc, Term, ValueId};
use rgoc_ssa::testsupport::FnBuilder;
use rgoc_ssa::verify::{INVARIANTS, verify};
use rgoc_types::{TypeId, TypeTable};

fn b(i: u32) -> BlockId {
    BlockId(i)
}

/// 断言被拒，且拒在 `want` 这条不变式上；返回渲染后的错误文本供进一步断言。
fn rejected(f: &SsaFunc, tt: &TypeTable, want: &str) -> String {
    match verify(f, tt) {
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
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let v0 = fb.value(ti);
    const_int(&mut fb, b0, v0, 1, ti);
    const_int(&mut fb, b0, v0, 2, ti); // 第二次定义
    fb.term(b0, Term::Return(vec![v0]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V1");
    assert!(msg.contains("v0"), "错误必须点名是哪个值：{msg}");
    assert!(
        msg.contains("b0[1]"),
        "错误必须指向第二次定义的位置（b0 的第 1 个位置）：{msg}"
    );
}

#[test]
fn t_ssa_02_v1_块参数与指令抢同一个值() {
    let mut fb = FnBuilder::new("param-clash");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let b1 = fb.block();
    let v0 = fb.value(ti);
    let p1 = fb.value(ti);
    const_int(&mut fb, b0, v0, 7, ti);
    fb.set_params(b1, &[v0]); // v0 又被当成块参数定义一次
    fb.term(
        b0,
        Term::Goto {
            to: b1,
            args: vec![v0],
        },
    );
    fb.term(b1, Term::Return(vec![p1]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V1");
    assert!(msg.contains("块参数"), "要说明第二次定义来自块参数：{msg}");
}

#[test]
fn t_ssa_02_v1_值从未被定义() {
    let mut fb = FnBuilder::new("undef");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let v0 = fb.value(ti);
    let v1 = fb.value(ti);
    const_int(&mut fb, b0, v0, 1, ti);
    fb.term(b0, Term::Return(vec![v1])); // v1 没有任何定义点
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V1");
    assert!(msg.contains("v1"), "错误必须点名未定义的值：{msg}");
}

#[test]
fn t_ssa_02_v1_目标值越界() {
    let mut fb = FnBuilder::new("oob");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let v0 = fb.value(ti);
    // 只分配了 1 个值（v0），却让一条指令定义 v9
    fb.instr(
        b0,
        Instr::Const {
            dst: ValueId(9),
            c: ConstVal::Int(1),
            typ: ti,
        },
    );
    fb.term(b0, Term::Return(vec![v0]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V1");
    assert!(msg.contains('9'), "要报出越界的那个编号：{msg}");
}

// ── V2：定义必须支配使用 ───────────────────────────────────────────

#[test]
fn t_ssa_03_v2_一支定义汇合点直接用() {
    let mut fb = FnBuilder::new("no-dom");
    let ti = fb.basic("int");
    let tb = fb.basic("bool");
    let b0 = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let b3 = fb.block();
    let cond = fb.value(tb);
    let only_in_b1 = fb.value(ti);
    const_bool(&mut fb, b0, cond, true, tb);
    fb.term(
        b0,
        Term::If {
            cond,
            then_: (b1, vec![]),
            else_: (b2, vec![]),
        },
    );
    const_int(&mut fb, b1, only_in_b1, 2, ti);
    fb.term(
        b1,
        Term::Goto {
            to: b3,
            args: vec![],
        },
    );
    fb.term(
        b2,
        Term::Goto {
            to: b3,
            args: vec![],
        },
    );
    fb.term(b3, Term::Return(vec![only_in_b1])); // b1 不支配 b3
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V2");
    assert!(
        msg.contains("b3") && msg.contains("b1"),
        "要同时报出定义块与使用块：{msg}"
    );
}

#[test]
fn t_ssa_03_v2_同块内先用后定义() {
    let mut fb = FnBuilder::new("use-before-def");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let early = fb.value(ti);
    let late = fb.value(ti);
    // 先用 late，再定义 late（块内顺序反了）
    fb.instr(
        b0,
        Instr::Copy {
            dst: early,
            src: late,
        },
    );
    const_int(&mut fb, b0, late, 5, ti);
    fb.term(b0, Term::Return(vec![early]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V2");
    assert!(
        msg.contains("b0[1]") && msg.contains("[0]"),
        "要同时报出定义位置 b0[1] 与使用位置 [0]：{msg}"
    );
}

#[test]
fn t_ssa_03_v2_用的是兄弟分支的定义() {
    let mut fb = FnBuilder::new("sibling");
    let ti = fb.basic("int");
    let tb = fb.basic("bool");
    let b0 = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let cond = fb.value(tb);
    let in_b1 = fb.value(ti);
    const_bool(&mut fb, b0, cond, true, tb);
    fb.term(
        b0,
        Term::If {
            cond,
            then_: (b1, vec![]),
            else_: (b2, vec![]),
        },
    );
    const_int(&mut fb, b1, in_b1, 3, ti);
    fb.term(b1, Term::Return(vec![in_b1]));
    fb.term(b2, Term::Return(vec![in_b1])); // b2 里用 b1 的定义
    let (f, tt) = fb.into_parts();

    assert_eq!(verify(&f, &tt).expect_err("必须被拒").invariant, "V2");
}

#[test]
fn t_ssa_03_v2_死块里的使用不报_v2_让_v8_报() {
    // 死块里的支配关系没有定义（不可达块 `dominates` 恒假）。
    // 若 V2 不跳过死块，这条会报 V2 —— 断言锁死「必须是 V8」。
    let mut fb = FnBuilder::new("dead-use");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let b1 = fb.block();
    let v0 = fb.value(ti);
    const_int(&mut fb, b0, v0, 1, ti);
    fb.term(b0, Term::Return(vec![v0]));
    fb.term(b1, Term::Return(vec![v0])); // 死块里用 b0 的定义
    let (f, tt) = fb.into_parts();

    assert_eq!(
        verify(&f, &tt).expect_err("必须被拒").invariant,
        "V8",
        "死块由 V8 报；V2 不该对不可达块下判断"
    );
}

// ── V3：类型自洽 ───────────────────────────────────────────────────

#[test]
fn t_ssa_04_v3_二元运算两侧类型不同() {
    let mut fb = FnBuilder::new("i64-plus-string");
    let ti = fb.basic("int");
    let ts = fb.basic("string");
    let b0 = fb.entry();
    let x = fb.value(ti);
    let y = fb.value(ts);
    let z = fb.value(ti);
    const_int(&mut fb, b0, x, 1, ti);
    fb.instr(
        b0,
        Instr::Const {
            dst: y,
            c: ConstVal::Str("s".to_string()),
            typ: ts,
        },
    );
    fb.instr(
        b0,
        Instr::Bin {
            dst: z,
            op: rgoc_hir::BinOp::Add,
            lhs: x,
            rhs: y, // int + string
            typ: ti,
        },
    );
    fb.term(b0, Term::Return(vec![z]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V3");
    assert!(
        msg.contains("类型不同") || msg.contains("操作数"),
        "要说清是操作数类型不同：{msg}"
    );
}

#[test]
fn t_ssa_04_v3_convert_的目标与记录类型不符() {
    let mut fb = FnBuilder::new("bad-convert");
    let ti = fb.basic("int");
    let tf = fb.basic("float64");
    let b0 = fb.entry();
    let x = fb.value(ti);
    let y = fb.value(tf); // 记成 float64
    const_int(&mut fb, b0, x, 1, ti);
    fb.instr(
        b0,
        Instr::Convert {
            dst: y,
            x,
            typ: ti, // 但指令声称转到 int
        },
    );
    fb.term(b0, Term::Return(vec![y]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V3");
    assert!(msg.contains("convert"), "要点名是指令 convert：{msg}");
}

#[test]
fn t_ssa_04_v3_if_的条件不是_bool() {
    let mut fb = FnBuilder::new("int-cond");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let b1 = fb.block();
    let cond = fb.value(ti); // int 当条件
    const_int(&mut fb, b0, cond, 1, ti);
    fb.term(
        b0,
        Term::If {
            cond,
            then_: (b1, vec![]),
            else_: (b1, vec![]),
        },
    );
    fb.term(b1, Term::Return(vec![cond]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V3");
    assert!(msg.contains("bool"), "要说清条件必须是 bool：{msg}");
}

#[test]
fn t_ssa_04_v3_块参数交接类型不符() {
    let mut fb = FnBuilder::new("param-type");
    let ti = fb.basic("int");
    let ts = fb.basic("string");
    let b0 = fb.entry();
    let b1 = fb.block();
    let s = fb.value(ts);
    let p1 = fb.value(ti); // 目标参数是 int
    fb.instr(
        b0,
        Instr::Const {
            dst: s,
            c: ConstVal::Str("s".to_string()),
            typ: ts,
        },
    );
    fb.set_params(b1, &[p1]);
    fb.term(
        b0,
        Term::Goto {
            to: b1,
            args: vec![s],
        },
    ); // 拿 string 喂 int 参数
    fb.term(b1, Term::Return(vec![p1]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V3");
    assert!(msg.contains("实参"), "要点名是边实参的问题：{msg}");
}

// ── V4：前驱/后继互逆、入口无前驱 ──────────────────────────────────

#[test]
fn t_ssa_05_v4_入口块有前驱() {
    let mut fb = FnBuilder::new("entry-pred");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let b1 = fb.block();
    let v0 = fb.value(ti);
    const_int(&mut fb, b0, v0, 1, ti);
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
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V4");
    assert!(msg.contains("入口"), "要说明是入口块的问题：{msg}");
}

#[test]
fn t_ssa_05_v4_边指向不存在的块() {
    let mut fb = FnBuilder::new("dangling");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let v0 = fb.value(ti);
    const_int(&mut fb, b0, v0, 1, ti);
    fb.term(
        b0,
        Term::Goto {
            to: b(7),
            args: vec![],
        },
    );
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V4");
    assert!(
        msg.contains("不存在的 b7"),
        "要报成「目标块不存在」，而不是落到互逆检查上（两者都得留，但诊断要准）：{msg}"
    );
}

#[test]
fn t_ssa_05_v4_同一目标两条边实参不同() {
    let mut fb = FnBuilder::new("dup-edge");
    let ti = fb.basic("int");
    let tb = fb.basic("bool");
    let b0 = fb.entry();
    let b1 = fb.block();
    let cond = fb.value(tb);
    let v0 = fb.value(ti);
    let v1 = fb.value(ti);
    const_bool(&mut fb, b0, cond, true, tb);
    const_int(&mut fb, b0, v0, 1, ti);
    const_int(&mut fb, b0, v1, 2, ti);
    // then 与 else 指向**同一块**但给不同实参：IR 里无法表达「走的是哪条边」
    fb.term(
        b0,
        Term::If {
            cond,
            then_: (b1, vec![v0]),
            else_: (b1, vec![v1]),
        },
    );
    fb.term(b1, Term::Return(vec![v0]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V4");
    assert!(msg.contains("实参"), "要说明冲突在实参上：{msg}");
}

// ── V8：可达性 ─────────────────────────────────────────────────────

#[test]
fn t_ssa_09_v8_悬挂死块() {
    let mut fb = FnBuilder::new("dead");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let v0 = fb.value(ti);
    let v1 = fb.value(ti);
    const_int(&mut fb, b0, v0, 1, ti);
    fb.term(b0, Term::Return(vec![v0]));
    let b1 = fb.block(); // 没有任何前驱
    const_int(&mut fb, b1, v1, 2, ti);
    fb.term(b1, Term::Return(vec![v1]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V8");
    assert!(msg.contains("b1"), "要报出死块编号：{msg}");
}

#[test]
fn t_ssa_09_v8_自环孤岛也是死块() {
    let mut fb = FnBuilder::new("island");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let v0 = fb.value(ti);
    const_int(&mut fb, b0, v0, 1, ti);
    fb.term(b0, Term::Return(vec![v0]));
    let b1 = fb.block();
    fb.term(
        b1,
        Term::Goto {
            to: b1,
            args: vec![],
        },
    ); // 自环：有前驱但不可达
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V8");
    assert!(msg.contains("b1"), "要报出孤岛编号：{msg}");
}

// ── 定位与优先级 ───────────────────────────────────────────────────

#[test]
fn 错误带得动位置() {
    let mut fb = FnBuilder::new("pos");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let b1 = fb.block();
    let v0 = fb.value(ti);
    const_int(&mut fb, b0, v0, 1, ti);
    const_int(&mut fb, b0, v0, 2, ti);
    fb.term(
        b0,
        Term::Goto {
            to: b1,
            args: vec![],
        },
    );
    fb.term(b1, Term::Return(vec![v0]));
    let (f, tt) = fb.into_parts();

    let e = verify(&f, &tt).expect_err("必须被拒");
    assert_eq!(e.block, b0, "第二次定义在 b0");
    assert_eq!(e.idx, 1, "块内位置 0 是第一条 const，1 是第二条");
}

#[test]
fn 先报_v1_再报_v4_顺序稳定() {
    // 同时违反 V1（重复定义）与 V4（边指向 b9）—— 必须只报 V1，
    // 否则同一张图在不同实现下报不同码，断言无法锚定。
    let mut fb = FnBuilder::new("both");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let v0 = fb.value(ti);
    const_int(&mut fb, b0, v0, 1, ti);
    const_int(&mut fb, b0, v0, 2, ti);
    fb.term(
        b0,
        Term::Goto {
            to: b(9),
            args: vec![],
        },
    );
    let (f, tt) = fb.into_parts();

    assert_eq!(verify(&f, &tt).expect_err("必须被拒").invariant, "V1");
}

// ── V5：调用与签名一致 ─────────────────────────────────────────────

#[test]
fn t_ssa_06_v5_结果个数与签名不符() {
    let mut fb = FnBuilder::new("arity");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let a = fb.value(ti);
    let r0 = fb.value(ti);
    let r1 = fb.value(ti);
    const_int(&mut fb, b0, a, 1, ti);
    // 被调方签名只返回 1 个值，却接了两个
    fb.call_sig(rgoc_hir::FuncId(1), 1, 1, false);
    fb.instr(
        b0,
        Instr::Call {
            dsts: vec![r0, r1],
            callee: rgoc_ssa::ir::CalleeRef::Func(rgoc_hir::FuncId(1)),
            args: vec![a],
            effect: rgoc_ssa::ir::Effect::default(),
        },
    );
    fb.term(b0, Term::Return(vec![r0, r1]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V5");
    assert!(
        msg.contains("结果"),
        "要说清是结果个数不符（不是实参个数）：{msg}"
    );
}

#[test]
fn t_ssa_06_v5_实参个数与签名不符() {
    let mut fb = FnBuilder::new("argc");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let a = fb.value(ti);
    let c = fb.value(ti);
    let r0 = fb.value(ti);
    const_int(&mut fb, b0, a, 1, ti);
    const_int(&mut fb, b0, c, 2, ti);
    fb.call_sig(rgoc_hir::FuncId(1), 1, 1, false); // 只要 1 个实参
    fb.instr(
        b0,
        Instr::Call {
            dsts: vec![r0],
            callee: rgoc_ssa::ir::CalleeRef::Func(rgoc_hir::FuncId(1)),
            args: vec![a, c], // 给了 2 个
            effect: rgoc_ssa::ir::Effect::default(),
        },
    );
    fb.term(b0, Term::Return(vec![r0]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V5");
    assert!(msg.contains("实参"), "要说清是实参个数不符：{msg}");
}

#[test]
fn t_ssa_06_v5_被调方没有签名记录() {
    // 「查不了」不许当「通过」：直接调用一个没登记签名的函数是可验证性缺口。
    let mut fb = FnBuilder::new("no-sig");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let r0 = fb.value(ti);
    fb.instr(
        b0,
        Instr::Call {
            dsts: vec![r0],
            callee: rgoc_ssa::ir::CalleeRef::Func(rgoc_hir::FuncId(7)),
            args: vec![],
            effect: rgoc_ssa::ir::Effect::default(),
        },
    );
    fb.term(b0, Term::Return(vec![r0]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V5");
    assert!(msg.contains("签名"), "要说明缺的是签名记录：{msg}");
}

// ── V7：边实参个数 == 目标块参数个数 ───────────────────────────────

#[test]
fn t_ssa_08_v7_边实参比块参数少() {
    let mut fb = FnBuilder::new("few-args");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let b1 = fb.block();
    let v = fb.value(ti);
    let p1 = fb.value(ti);
    let p2 = fb.value(ti);
    const_int(&mut fb, b0, v, 1, ti);
    fb.set_params(b1, &[p1, p2]); // 两个参数
    fb.term(
        b0,
        Term::Goto {
            to: b1,
            args: vec![v],
        },
    ); // 只给一个
    fb.term(b1, Term::Return(vec![p1, p2]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V7");
    assert!(
        msg.contains("2") && msg.contains("1"),
        "要报出两边的个数：{msg}"
    );
}

#[test]
fn t_ssa_08_v7_边实参比块参数多() {
    let mut fb = FnBuilder::new("many-args");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let b1 = fb.block();
    let v = fb.value(ti);
    let p1 = fb.value(ti);
    const_int(&mut fb, b0, v, 1, ti);
    fb.set_params(b1, &[p1]); // 一个参数
    fb.term(
        b0,
        Term::Goto {
            to: b1,
            args: vec![v, v, v],
        },
    ); // 给三个
    fb.term(b1, Term::Return(vec![p1]));
    let (f, tt) = fb.into_parts();

    assert_eq!(verify(&f, &tt).expect_err("必须被拒").invariant, "V7");
}

// ── V6：mem 链 ─────────────────────────────────────────────────────

#[test]
fn t_ssa_07_v6_mem_操作数不被支配() {
    // b1 里 store 产出 m1，b2 里直接用它 —— b1 不支配 b2
    let mut fb = FnBuilder::new("mem-no-dom");
    let ti = fb.basic("int");
    let tb = fb.basic("bool");
    let b0 = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let r0 = fb.root(rgoc_ssa::ir::Root::Heap);
    let cond = fb.value(tb);
    let seed = fb.value(ti);
    let addr = fb.value(ti);
    let val = fb.value(ti);
    let m1 = fb.value(ti);
    let x = fb.value(ti);
    const_bool(&mut fb, b0, cond, true, tb);
    const_int(&mut fb, b0, seed, 0, ti);
    const_int(&mut fb, b0, addr, 0, ti);
    const_int(&mut fb, b0, val, 42, ti);
    fb.term(
        b0,
        Term::If {
            cond,
            then_: (b1, vec![]),
            else_: (b2, vec![]),
        },
    );
    fb.instr(
        b1,
        Instr::Store {
            dst: m1,
            mem: seed,
            root: r0,
            addr,
            val,
        },
    );
    fb.term(
        b1,
        Term::Goto {
            to: b2,
            args: vec![],
        },
    );
    fb.instr(
        b2,
        Instr::Load {
            dst: x,
            mem: m1, // ← 来自兄弟分支
            root: r0,
            addr,
            typ: ti,
        },
    );
    fb.term(b2, Term::Return(vec![x]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V6");
    assert!(msg.contains("mem"), "要点名是 mem 操作数：{msg}");
}

#[test]
fn t_ssa_07_v6_同块同根跳过了_store() {
    // store 产出 m1，同一块里随后的 load 却还用 seed
    let mut fb = FnBuilder::new("mem-skip");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let r0 = fb.root(rgoc_ssa::ir::Root::Heap);
    let seed = fb.value(ti);
    let addr = fb.value(ti);
    let val = fb.value(ti);
    let m1 = fb.value(ti);
    let x = fb.value(ti);
    const_int(&mut fb, b0, seed, 0, ti);
    const_int(&mut fb, b0, addr, 0, ti);
    const_int(&mut fb, b0, val, 42, ti);
    fb.instr(
        b0,
        Instr::Store {
            dst: m1,
            mem: seed,
            root: r0,
            addr,
            val,
        },
    );
    fb.instr(
        b0,
        Instr::Load {
            dst: x,
            mem: seed, // ← 应该用 m1（store 的新 mem）
            root: r0,
            addr,
            typ: ti,
        },
    );
    fb.term(b0, Term::Return(vec![x]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V6");
    assert!(
        msg.contains("跳过") || msg.contains("m1") || msg.contains("v3"),
        "要说清是漏了 store 产出新 mem：{msg}"
    );
}

// ── V9：取址即建根 ─────────────────────────────────────────────────

#[test]
fn t_ssa_10_v9_slot_根没有登记在_slot_root() {
    let mut fb = FnBuilder::new("no-slot-root");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let v0 = fb.value(ti);
    // roots[0] = Slot(s0)，但 slot_root 是空的
    fb.root(rgoc_ssa::ir::Root::Slot(rgoc_hir::SlotId(0)));
    const_int(&mut fb, b0, v0, 1, ti);
    fb.term(b0, Term::Return(vec![v0]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V9");
    assert!(msg.contains("slot_root"), "要点名 slot_root：{msg}");
}

#[test]
fn t_ssa_10_v9_slot_root_指向的不是_slot_根() {
    let mut fb = FnBuilder::new("wrong-root-kind");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let v0 = fb.value(ti);
    let r0 = fb.root(rgoc_ssa::ir::Root::Heap); // 指向堆根
    fb.slot_root(rgoc_hir::SlotId(0), r0);
    const_int(&mut fb, b0, v0, 1, ti);
    fb.term(b0, Term::Return(vec![v0]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V9");
    assert!(msg.contains("slot"), "要说明期望的是 slot 根：{msg}");
}

#[test]
fn t_ssa_10_v9_取址的根越界() {
    let mut fb = FnBuilder::new("addr-oob");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let p = fb.value(ti); // 假装是指针
    // 一个根都没建，却让 AddrOf 指向 RootId(3)
    fb.instr(
        b0,
        Instr::AddrOf {
            dst: p,
            root: rgoc_ssa::ir::RootId(3),
        },
    );
    fb.term(b0, Term::Return(vec![p]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V9");
    assert!(msg.contains('3'), "要报出越界的根编号：{msg}");
}

#[test]
fn t_ssa_10_v9_两个_slot_根被交叉登记() {
    // roots = [Slot(s0), Slot(s1)]，slot_root 却写成 s0→R1 / s1→R0。
    // 两个方向都错，但 ①（roots→slot_root 回指）先跑 ⇒ 必须报「两边不一致」。
    // 这条**专门**为 ① 的 `Some(back) if back.index() == ri` 分支存在：
    // 只写「没登记」那一种，`Some(..)` 分支就永远测不到（变异实测过）。
    let mut fb = FnBuilder::new("crossed");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let v0 = fb.value(ti);
    let r0 = fb.root(rgoc_ssa::ir::Root::Slot(rgoc_hir::SlotId(0)));
    let r1 = fb.root(rgoc_ssa::ir::Root::Slot(rgoc_hir::SlotId(1)));
    fb.slot_root(rgoc_hir::SlotId(0), r1);
    fb.slot_root(rgoc_hir::SlotId(1), r0);
    const_int(&mut fb, b0, v0, 1, ti);
    fb.term(b0, Term::Return(vec![v0]));
    let (f, tt) = fb.into_parts();

    let msg = rejected(&f, &tt, "V9");
    assert!(msg.contains("不一致"), "要报成两边不一致：{msg}");
}

#[test]
fn 位置序号把块参数算在前面() {
    // b1 有 1 个块参数，它的**第一条指令**在块内位置应为 1（不是 0）。
    // 这条钉住 `VerifyError::idx` 的口径 —— 若哪天改成「指令下标」，
    // 报出来的位置会静默偏移一格，而没人会注意到。
    let mut fb = FnBuilder::new("idx-base");
    let ti = fb.basic("int");
    let tf = fb.basic("float64");
    let b0 = fb.entry();
    let b1 = fb.block();
    let p1 = fb.value(ti);
    let one = fb.value(ti);
    let bad = fb.value(tf);
    const_int(&mut fb, b0, one, 1, ti);
    fb.set_params(b1, &[p1]);
    fb.term(
        b0,
        Term::Goto {
            to: b1,
            args: vec![one],
        },
    );
    fb.instr(
        b1,
        Instr::Convert {
            dst: bad,
            x: p1,
            typ: ti, // 与 bad 的 float64 不符
        },
    );
    fb.term(b1, Term::Return(vec![bad]));
    let (f, tt) = fb.into_parts();

    let e = verify(&f, &tt).expect_err("必须被拒");
    assert_eq!(e.invariant, "V3");
    assert_eq!(e.block, b1);
    assert_eq!(e.idx, 1, "块参数占位置 0，第一条指令是位置 1");
}
