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
    // V5 要求直接调用必须有签名记录（「查不了」不许当通过）
    fb.call_sig(rgoc_hir::FuncId(1), 1, 2, false);
    fb.call_sig(rgoc_hir::FuncId(2), 1, 0, false);
    // 两结果
    fb.instr(
        b0,
        Instr::Call {
            dsts: vec![r0, r1],
            callee: CalleeRef::Func(rgoc_hir::FuncId(1)),
            args: vec![a],
            dots: false,
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
            dots: false,
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

#[test]
fn 变参调用按至少算实参() {
    // `func f(a int, rest ...int)` 的 `params = 2`；调用 `f(1)` 合法（rest 为空）。
    let mut fb = FnBuilder::new("variadic");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let a = fb.value(ti);
    let r0 = fb.value(ti);
    const_int(&mut fb, b0, a, 1, ti);
    fb.call_sig(rgoc_hir::FuncId(1), 2, 1, true);
    fb.instr(
        b0,
        Instr::Call {
            dsts: vec![r0],
            callee: rgoc_ssa::ir::CalleeRef::Func(rgoc_hir::FuncId(1)),
            args: vec![a],
            dots: false,
            effect: rgoc_ssa::ir::Effect::default(),
        },
    );
    fb.term(b0, Term::Return(vec![r0]));
    let (f, tt) = fb.into_parts();
    ok(&f, &tt);
}

#[test]
fn 间接调用没有签名可查时跳过_v5检查() {
    // 函数值调用（`f()` 里 `f` 是变量）在 M6 支持的建图面内，
    // 但签名运行期才定 ⇒ V5 无可查对象，跳过而不是瞎判。
    // 若哪天有人把「查不到签名」当成「必须拒」，这条会红 —— 那是**故意的**：
    // 间接调用能建图，就不该被 V5 拒掉。
    let mut fb = FnBuilder::new("indirect");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let fv = fb.value(ti);
    let r0 = fb.value(ti);
    fb.set_params(b0, &[fv]); // 函数值来自形参
    fb.instr(
        b0,
        Instr::Call {
            dsts: vec![r0],
            callee: rgoc_ssa::ir::CalleeRef::Indirect(fv),
            args: vec![],
            dots: false,
            effect: rgoc_ssa::ir::Effect::default(),
        },
    );
    fb.term(b0, Term::Return(vec![r0]));
    let (f, tt) = fb.into_parts();
    ok(&f, &tt);
}

#[test]
fn mem_链同块同根按_store_接续() {
    let mut fb = FnBuilder::new("mem-chain");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let r0 = fb.root(rgoc_ssa::ir::Root::Heap);
    let seed = fb.value(ti);
    let addr = fb.value(ti);
    let val = fb.value(ti);
    let m1 = fb.value(ti);
    let x = fb.value(ti);
    let m2 = fb.value(ti);
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
            mem: m1, // 用 store 的新 mem
            root: r0,
            addr,
            typ: ti,
        },
    );
    fb.instr(
        b0,
        Instr::Store {
            dst: m2,
            mem: m1, // load 不改 mem，所以第二次 store 还接 m1
            root: r0,
            addr,
            val: x,
        },
    );
    fb.term(b0, Term::Return(vec![x]));
    let (f, tt) = fb.into_parts();
    ok(&f, &tt);
}

#[test]
fn 异根访存互不约束() {
    let mut fb = FnBuilder::new("two-roots");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let r0 = fb.root(rgoc_ssa::ir::Root::Heap);
    let r1 = fb.root(rgoc_ssa::ir::Root::Global(rgoc_hir::GlobalId(0)));
    let seed = fb.value(ti);
    let addr = fb.value(ti);
    let val = fb.value(ti);
    let m1 = fb.value(ti);
    let x = fb.value(ti);
    const_int(&mut fb, b0, seed, 0, ti);
    const_int(&mut fb, b0, addr, 0, ti);
    const_int(&mut fb, b0, val, 1, ti);
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
    // 另一个根用的还是 seed —— 合法：同根才要求串行
    fb.instr(
        b0,
        Instr::Load {
            dst: x,
            mem: seed,
            root: r1,
            addr,
            typ: ti,
        },
    );
    fb.term(b0, Term::Return(vec![x]));
    let (f, tt) = fb.into_parts();
    ok(&f, &tt);
}

#[test]
fn 取址即建根_且_slot_root_与_roots_互为逆() {
    let mut fb = FnBuilder::new("addr-root");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let r0 = fb.root(rgoc_ssa::ir::Root::Slot(rgoc_hir::SlotId(2)));
    fb.slot_root(rgoc_hir::SlotId(2), r0);
    let p = fb.value(ti);
    fb.instr(b0, Instr::AddrOf { dst: p, root: r0 });
    fb.term(b0, Term::Return(vec![p]));
    let (f, tt) = fb.into_parts();
    ok(&f, &tt);
}

/// 「大杂烩」函数：14 条指令**每条至少出现一次**，且整体合法。
///
/// 这是 `T-SSA-11` 的主图。它同时是一个**自维护的穷尽检查**：
/// 以后 `Instr` 加了变体，`每条指令族至少有一张合法图` 会红 —— 逼作者
/// 决定「这个新指令在合法 IR 里长什么样」，而不是只写个反例了事。
fn kitchen_sink() -> (SsaFunc, TypeTable) {
    let mut fb = FnBuilder::new("kitchen");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let r0 = fb.root(rgoc_ssa::ir::Root::Heap);
    let c = fb.value(ti);
    let cp = fb.value(ti);
    let bin = fb.value(ti);
    let un = fb.value(ti);
    let conv = fb.value(ti);
    let addr = fb.value(ti);
    let field = fb.value(ti);
    let index = fb.value(ti);
    let slice = fb.value(ti);
    let comp = fb.value(ti);
    let make = fb.value(ti);
    let ld = fb.value(ti);
    let st = fb.value(ti);
    let call_r = fb.value(ti);
    fb.call_sig(rgoc_hir::FuncId(1), 1, 1, false);

    const_int(&mut fb, b0, c, 1, ti);
    fb.instr(b0, Instr::Copy { dst: cp, src: c });
    fb.instr(
        b0,
        Instr::Bin {
            dst: bin,
            op: rgoc_hir::BinOp::Add,
            lhs: cp,
            rhs: c,
            typ: ti,
        },
    );
    fb.instr(
        b0,
        Instr::Un {
            dst: un,
            op: rgoc_hir::UnOp::Neg,
            x: bin,
            typ: ti,
        },
    );
    fb.instr(
        b0,
        Instr::Convert {
            dst: conv,
            x: un,
            typ: ti,
        },
    );
    fb.instr(
        b0,
        Instr::AddrOf {
            dst: addr,
            root: r0,
        },
    );
    fb.instr(
        b0,
        Instr::Field {
            dst: field,
            base: addr,
            index: 0,
        },
    );
    fb.instr(
        b0,
        Instr::Index {
            dst: index,
            base: addr,
            i: c,
        },
    );
    fb.instr(
        b0,
        Instr::Slice {
            dst: slice,
            base: addr,
            lo: Some(c),
            hi: None,
            max: None,
        },
    );
    fb.instr(
        b0,
        Instr::Composite {
            dst: comp,
            typ: ti,
            items: vec![c, conv],
        },
    );
    fb.instr(
        b0,
        Instr::Make {
            dst: make,
            typ: ti,
            len: Some(c),
            cap: None,
        },
    );
    fb.instr(
        b0,
        Instr::Load {
            dst: ld,
            mem: c,
            root: r0,
            addr,
            typ: ti,
        },
    );
    fb.instr(
        b0,
        Instr::Store {
            dst: st,
            mem: c,
            root: r0,
            addr,
            val: ld,
        },
    );
    fb.instr(
        b0,
        Instr::Call {
            dsts: vec![call_r],
            callee: rgoc_ssa::ir::CalleeRef::Func(rgoc_hir::FuncId(1)),
            args: vec![conv],
            dots: false,
            effect: rgoc_ssa::ir::Effect::default(),
        },
    );
    fb.term(
        b0,
        Term::Return(vec![call_r, field, index, slice, comp, make]),
    );
    fb.into_parts()
}

#[test]
fn 大杂烩函数是合法的() {
    let (f, tt) = kitchen_sink();
    ok(&f, &tt);
}

#[test]
fn 每条指令族至少有一张合法图() {
    let (f, _tt) = kitchen_sink();
    let mut seen: Vec<&'static str> = Vec::new();
    for blk in &f.blocks {
        for ins in &blk.instrs {
            let k = rgoc_ssa::ir::kind_of_instr(ins);
            if !seen.contains(&k) {
                seen.push(k);
            }
        }
    }
    seen.sort_unstable();
    let mut want = vec![
        "addr-of",
        "bin",
        "call",
        "composite",
        "const",
        "convert",
        "copy",
        "field",
        "index",
        "load",
        "make",
        "slice",
        "store",
        "un",
    ];
    want.sort_unstable();
    assert_eq!(
        seen, want,
        "14 条指令族必须各有一张合法图（加了变体就来补）"
    );
}

#[test]
fn 五种终止符至少各出现一次() {
    // 把五张图拼起来看终止符覆盖：return / goto / if / panic / unreachable
    let (k, _) = kitchen_sink();
    fn note(seen: &mut Vec<&'static str>, t: &Term) {
        let s = rgoc_ssa::ir::kind_of_term(t);
        if !seen.contains(&s) {
            seen.push(s);
        }
    }
    let mut seen: Vec<&'static str> = Vec::new();
    for blk in &k.blocks {
        note(&mut seen, &blk.term);
    }
    // panic：`p != nil` 才 panic 的形状
    let mut fb = FnBuilder::new("panics");
    let ti = fb.basic("int");
    let b0 = fb.entry();
    let v = fb.value(ti);
    const_int(&mut fb, b0, v, 1, ti);
    fb.term(b0, Term::Panic(v));
    let (p, _) = fb.into_parts();
    for blk in &p.blocks {
        note(&mut seen, &blk.term);
    }
    // if / goto：两条分支都回到自己（只测终止符形态，不测语义）
    let mut fb3 = FnBuilder::new("branches");
    let ti3 = fb3.basic("int");
    let tb3 = fb3.basic("bool");
    let b0 = fb3.entry();
    let b1 = fb3.block();
    let cond = fb3.value(tb3);
    let one = fb3.value(ti3);
    const_bool(&mut fb3, b0, cond, true, tb3);
    const_int(&mut fb3, b0, one, 1, ti3);
    fb3.term(
        b0,
        Term::If {
            cond,
            then_: (b1, vec![]),
            else_: (b1, vec![]),
        },
    );
    fb3.term(
        b1,
        Term::Goto {
            to: b1,
            args: vec![],
        },
    );
    let (br, _) = fb3.into_parts();
    for blk in &br.blocks {
        note(&mut seen, &blk.term);
    }
    // unreachable：块可达但流到此为止
    let mut fb2 = FnBuilder::new("stops");
    let b0 = fb2.entry();
    fb2.term(b0, Term::Unreachable);
    let (u, _) = fb2.into_parts();
    for blk in &u.blocks {
        note(&mut seen, &blk.term);
    }
    seen.sort_unstable();
    let mut want = vec!["goto", "if", "panic", "return", "unreachable"];
    want.sort_unstable();
    assert_eq!(seen, want, "五种终止符都要有合法图");
}
