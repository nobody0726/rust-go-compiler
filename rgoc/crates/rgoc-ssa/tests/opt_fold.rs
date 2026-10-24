//! 常量折叠（`T-SSA-60`；`M6-plan.md` Task 41）。
//!
//! # 判据的两半
//!
//! * **折对了**：折出来的常量就是 Go 语义算出的值（回绕、浮点、字符串拼接）；
//! * **没折错**：除零 / 负移位 / 浮点→整数这些「不能折」的情形**必须留着**。
//!
//! 第二半比第一半重要：折叠是语义保持变换，折错一个值程序就在没有任何报错的
//! 情况下算错。所以每条「不折」的分支都要有一条测试钉住。

use rgoc_hir::types::ConstVal;
use rgoc_hir::{BinOp, UnOp};
use rgoc_ssa::ir::{BlockId, Instr, SsaFunc, Term};
use rgoc_ssa::opt::fold;
use rgoc_ssa::testsupport::FnBuilder;
use rgoc_ssa::verify::verify;
use rgoc_types::TypeTable;

/// `func f() T { return <二元运算>(a, b) }`，类型由 `ty` 指定。
fn bin_of(ty: &str, op: BinOp, a: ConstVal, b: ConstVal) -> (SsaFunc, TypeTable) {
    let mut fb = FnBuilder::new("f");
    let t = fb.basic(ty);
    let e = fb.entry();
    let va = fb.value(t);
    let vb = fb.value(t);
    let r = fb.value(t);
    fb.instr(
        e,
        Instr::Const {
            dst: va,
            c: a,
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Const {
            dst: vb,
            c: b,
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Bin {
            dst: r,
            op,
            lhs: va,
            rhs: vb,
            typ: t,
        },
    );
    fb.term(e, Term::Return(vec![r]));
    fb.into_parts()
}

fn run(f: &mut SsaFunc, tt: &TypeTable) {
    fold::run(f, tt);
    verify(f, tt).expect("折叠之后必须仍然合法");
}

/// 全部 `Const` 的值（按出现顺序）。
fn consts(f: &SsaFunc) -> Vec<ConstVal> {
    f.blocks
        .iter()
        .flat_map(|b| &b.instrs)
        .filter_map(|i| match i {
            Instr::Const { c, .. } => Some(c.clone()),
            _ => None,
        })
        .collect()
}

fn has_bin(f: &SsaFunc) -> bool {
    f.blocks
        .iter()
        .flat_map(|b| &b.instrs)
        .any(|i| matches!(i, Instr::Bin { .. }))
}

#[test]
fn t_ssa_60_整数二元运算折成常量() {
    let (mut f, tt) = bin_of("int", BinOp::Add, ConstVal::Int(1), ConstVal::Int(2));
    run(&mut f, &tt);
    assert!(!has_bin(&f), "`1+2` 该折掉");
    assert!(
        consts(&f).contains(&ConstVal::Int(3)),
        "折出来必须是 3，实际：{:?}",
        consts(&f)
    );
}

#[test]
fn t_ssa_60_溢出按宽度回绕() {
    // `var x int8 = 127; x + 1` ⇒ `-128`（Go 定义成按宽度回绕，不是饱和也不是 UB）
    let (mut f, tt) = bin_of("int8", BinOp::Add, ConstVal::Int(127), ConstVal::Int(1));
    run(&mut f, &tt);
    assert!(
        consts(&f).contains(&ConstVal::Int(-128)),
        "int8 的 127+1 必须是 -128，实际：{:?}",
        consts(&f)
    );
}

#[test]
fn t_ssa_60_除零不折() {
    // 除零是**运行期 panic**：折掉就把 panic 变没了
    let (mut f, tt) = bin_of("int", BinOp::Div, ConstVal::Int(1), ConstVal::Int(0));
    run(&mut f, &tt);
    assert!(has_bin(&f), "除零必须留着（运行期要 panic）");
}

#[test]
fn t_ssa_60_负移位计数不折() {
    // Go 规范：移位计数为负 ⇒ 运行期 panic
    let (mut f, tt) = bin_of("int", BinOp::Shl, ConstVal::Int(1), ConstVal::Int(-1));
    run(&mut f, &tt);
    assert!(has_bin(&f), "负移位计数必须留着");
}

#[test]
fn t_ssa_60_浮点与字符串也折() {
    let (mut f, tt) = bin_of(
        "float64",
        BinOp::Add,
        ConstVal::Float(1.5),
        ConstVal::Float(2.5),
    );
    run(&mut f, &tt);
    assert!(
        consts(&f).contains(&ConstVal::Float(4.0)),
        "1.5+2.5 该是 4.0，实际：{:?}",
        consts(&f)
    );

    let (mut f, tt) = bin_of(
        "string",
        BinOp::Add,
        ConstVal::Str("a".to_string()),
        ConstVal::Str("b".to_string()),
    );
    run(&mut f, &tt);
    assert!(
        consts(&f).contains(&ConstVal::Str("ab".to_string())),
        "\"a\"+\"b\" 该是 \"ab\"，实际：{:?}",
        consts(&f)
    );
}

#[test]
fn t_ssa_60_比较折成_bool() {
    // 比较的**操作数**是 int，**结果**是 bool —— 两边的类型不同，`bin_of` 只造
    // 「三者同型」的形态，所以这里单独搭
    let mut fb = FnBuilder::new("f");
    let int = fb.basic("int");
    let bt = fb.basic("bool");
    let e = fb.entry();
    let a = fb.value(int);
    let b = fb.value(int);
    let r = fb.value(bt);
    fb.instr(
        e,
        Instr::Const {
            dst: a,
            c: ConstVal::Int(1),
            typ: int,
        },
    );
    fb.instr(
        e,
        Instr::Const {
            dst: b,
            c: ConstVal::Int(2),
            typ: int,
        },
    );
    fb.instr(
        e,
        Instr::Bin {
            dst: r,
            op: BinOp::Lt,
            lhs: a,
            rhs: b,
            typ: bt,
        },
    );
    fb.term(e, Term::Return(vec![r]));
    let (mut f, tt) = fb.into_parts();
    run(&mut f, &tt);
    assert!(!has_bin(&f), "`1 < 2` 该折掉");
    assert!(
        consts(&f).contains(&ConstVal::Bool(true)),
        "该折出 true，实际：{:?}",
        consts(&f)
    );
}

#[test]
fn t_ssa_60_整数转换截断到宽度() {
    // `int8(300)` ⇒ 44（Go 的转换就是截断，不是饱和）
    let mut fb = FnBuilder::new("f");
    let int = fb.basic("int");
    let i8t = fb.basic("int8");
    let e = fb.entry();
    let v = fb.value(int);
    let r = fb.value(i8t);
    fb.instr(
        e,
        Instr::Const {
            dst: v,
            c: ConstVal::Int(300),
            typ: int,
        },
    );
    fb.instr(
        e,
        Instr::Convert {
            dst: r,
            x: v,
            typ: i8t,
        },
    );
    fb.term(e, Term::Return(vec![r]));
    let (mut f, tt) = fb.into_parts();
    run(&mut f, &tt);
    assert!(
        consts(&f).contains(&ConstVal::Int(44)),
        "int8(300) 该是 44，实际：{:?}",
        consts(&f)
    );
}

#[test]
fn t_ssa_60_浮点转整数不折() {
    // Go 说「浮点→整数的越界转换是实现定义的」⇒ 替实现做决定就是错
    let mut fb = FnBuilder::new("f");
    let ft = fb.basic("float64");
    let int = fb.basic("int");
    let e = fb.entry();
    let v = fb.value(ft);
    let r = fb.value(int);
    fb.instr(
        e,
        Instr::Const {
            dst: v,
            c: ConstVal::Float(1.5),
            typ: ft,
        },
    );
    fb.instr(
        e,
        Instr::Convert {
            dst: r,
            x: v,
            typ: int,
        },
    );
    fb.term(e, Term::Return(vec![r]));
    let (mut f, tt) = fb.into_parts();
    run(&mut f, &tt);
    assert!(
        f.blocks[0]
            .instrs
            .iter()
            .any(|i| matches!(i, Instr::Convert { .. })),
        "浮点→整数必须留着（实现定义）"
    );
}

#[test]
fn t_ssa_60_一元负号与取反() {
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let e = fb.entry();
    let v = fb.value(t);
    let r = fb.value(t);
    fb.instr(
        e,
        Instr::Const {
            dst: v,
            c: ConstVal::Int(5),
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Un {
            dst: r,
            op: UnOp::Neg,
            x: v,
            typ: t,
        },
    );
    fb.term(e, Term::Return(vec![r]));
    let (mut f, tt) = fb.into_parts();
    run(&mut f, &tt);
    assert!(
        consts(&f).contains(&ConstVal::Int(-5)),
        "`-5` 该折出来，实际：{:?}",
        consts(&f)
    );
}

#[test]
fn t_ssa_60_if_常量条件简化成_goto_并清掉死块() {
    // b0: if true -> b1 | b2 ; b1: return 1 ; b2: return 2
    let mut fb = FnBuilder::new("f");
    let bt = fb.basic("bool");
    let int = fb.basic("int");
    let b0 = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let c = fb.value(bt);
    fb.instr(
        b0,
        Instr::Const {
            dst: c,
            c: ConstVal::Bool(true),
            typ: bt,
        },
    );
    fb.term(
        b0,
        Term::If {
            cond: c,
            then_: (b1, Vec::new()),
            else_: (b2, Vec::new()),
        },
    );
    let v1 = fb.value(int);
    fb.instr(
        b1,
        Instr::Const {
            dst: v1,
            c: ConstVal::Int(1),
            typ: int,
        },
    );
    fb.term(b1, Term::Return(vec![v1]));
    let v2 = fb.value(int);
    fb.instr(
        b2,
        Instr::Const {
            dst: v2,
            c: ConstVal::Int(2),
            typ: int,
        },
    );
    fb.term(b2, Term::Return(vec![v2]));
    let (mut f, tt) = fb.into_parts();
    verify(&f, &tt).expect("输入必须合法（两条边都可达）");

    fold::run(&mut f, &tt);
    verify(&f, &tt).expect("折叠之后必须仍然合法（死块要被清掉，否则 V8 红）");
    assert_eq!(f.blocks.len(), 2, "b2 不可达 ⇒ 该被清掉");
    // prune 之后原 b1 重编号成 b1（下标 1）
    assert!(
        matches!(f.blocks[0].term, Term::Goto { to, .. } if to == BlockId(1)),
        "b0 该变成 goto b1，实际：{:?}",
        f.blocks[0].term
    );
}

#[test]
fn t_ssa_60_链式常量折到不动点() {
    // `(1+2)+3`：一轮只折得出 `1+2`，要迭代
    let mut fb = FnBuilder::new("f");
    let t = fb.basic("int");
    let e = fb.entry();
    let a = fb.value(t);
    let b = fb.value(t);
    let c = fb.value(t);
    let ab = fb.value(t);
    let r = fb.value(t);
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
        Instr::Const {
            dst: b,
            c: ConstVal::Int(2),
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Const {
            dst: c,
            c: ConstVal::Int(3),
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Bin {
            dst: ab,
            op: BinOp::Add,
            lhs: a,
            rhs: b,
            typ: t,
        },
    );
    fb.instr(
        e,
        Instr::Bin {
            dst: r,
            op: BinOp::Add,
            lhs: ab,
            rhs: c,
            typ: t,
        },
    );
    fb.term(e, Term::Return(vec![r]));
    let (mut f, tt) = fb.into_parts();
    run(&mut f, &tt);
    assert!(
        !has_bin(&f),
        "两条加法都该折掉：{}",
        rgoc_ssa::dump::dump_func(&f, &tt)
    );
    assert!(
        consts(&f).contains(&ConstVal::Int(6)),
        "`(1+2)+3` 该折出 6，实际：{:?}",
        consts(&f)
    );
}
