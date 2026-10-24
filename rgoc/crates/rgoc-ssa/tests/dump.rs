//! dump 的判据面（`T-SSA-13` 确定性 / `T-SSA-14` 字段与排序 / `T-SSA-46` 与 `C10` 一致）。
//!
//! # 为什么「逐字节对示例」比「断言几个字段」强
//!
//! 断言 `contains("block b0")` 这种写法，在格式漂了之后仍然绿 —— 它只证明
//! 「有这串字符」，不证明**整段文本**没变。所以这里的主判据是
//! `C10` §3.1 的示例**原样抄进来**做 golden：实现与契约之间任何一处
//! 空格、顺序、字段的差异都会红。
//!
//! # 为什么要 `FnBuilder::bare`
//!
//! `C10` 的示例写的是 `# type t0 = bool` / `t1 = int`。`FnBuilder::new` 走
//! `universe()`（表里已有 22 个基本类型），`bool` 会是 `t22`。golden 必须用
//! 空表构建器，否则对不上 —— 这不是「为了测试改实现」，而是**编号是格式的一部分**。

use rgoc_hir::types::ConstVal;
use rgoc_ssa::dump::{DUMP_VERSION, dump_func, dump_module};
use rgoc_ssa::ir::{BlockId, Instr, Term, ValueId};
use rgoc_ssa::testsupport::{FnBuilder, module_of};

fn const_bool(fb: &mut FnBuilder, blk: BlockId, v: ValueId, x: bool, tb: rgoc_types::TypeId) {
    fb.instr(
        blk,
        Instr::Const {
            dst: v,
            c: ConstVal::Bool(x),
            typ: tb,
        },
    );
}

fn const_int(fb: &mut FnBuilder, blk: BlockId, v: ValueId, n: i128, ti: rgoc_types::TypeId) {
    fb.instr(
        blk,
        Instr::Const {
            dst: v,
            c: ConstVal::Int(n),
            typ: ti,
        },
    );
}

/// `C10` §3.1 的示例函数：`func f() int { if true { return 1 }; return 0 }`。
fn c10_example() -> (rgoc_ssa::ir::SsaFunc, rgoc_types::TypeTable) {
    let mut fb = FnBuilder::bare("f");
    let tb = fb.basic("bool"); // t0
    let ti = fb.basic("int"); // t1
    let b0 = fb.entry();
    let b1 = fb.block();
    let b2 = fb.block();
    let v0 = fb.value(tb);
    let v1 = fb.value(ti);
    let v2 = fb.value(ti);
    let v3 = fb.value(ti);
    let v4 = fb.value(ti);
    const_bool(&mut fb, b0, v0, true, tb);
    const_int(&mut fb, b0, v1, 0, ti);
    fb.term(
        b0,
        Term::If {
            cond: v0,
            then_: (b1, vec![]),
            else_: (b2, vec![v1]),
        },
    );
    const_int(&mut fb, b1, v2, 1, ti);
    fb.term(
        b1,
        Term::Goto {
            to: b2,
            args: vec![v2],
        },
    );
    fb.set_params(b2, &[v3]);
    fb.instr(
        b2,
        Instr::Bin {
            dst: v4,
            op: rgoc_hir::BinOp::Add,
            lhs: v3,
            rhs: v3,
            typ: ti,
        },
    );
    fb.term(b2, Term::Return(vec![v4]));
    fb.into_parts()
}

#[test]
fn t_ssa_46_与_c10_示例逐字节一致() {
    let (f, tt) = c10_example();
    let want = "\
# SSA v1
# stage: ssa
# func f blocks=3 values=5
block b0 params=[] preds=[] succs=[b1,b2]
  v0 = const true : t0
  v1 = const 0 : t1
  term if v0 -> b1 () | b2 (v1)
block b1 params=[] preds=[b0] succs=[b2]
  v2 = const 1 : t1
  term goto b2 (v2)
block b2 params=[v3] preds=[b0,b1] succs=[]
  v4 = bin add v3 v3 : t1
  term return (v4)
# type t0 = bool
# type t1 = int
";
    let m = module_of(vec![f]);
    assert_eq!(
        dump_module(&m, &tt),
        want,
        "dump 与 C10 §3.1 的示例必须逐字节一致（格式漂了就是契约漂了）"
    );
}

#[test]
fn t_ssa_13_同一输入三次_dump_逐字节相同() {
    let (f, tt) = c10_example();
    let m = module_of(vec![f]);
    let a = dump_module(&m, &tt);
    let b = dump_module(&m, &tt);
    let c = dump_module(&m, &tt);
    assert_eq!(a, b, "第二次就变了 ⇒ 有哈希序或时间戳");
    assert_eq!(b, c);
    assert!(a.starts_with(DUMP_VERSION), "版本行必须是第一行");
}

#[test]
fn t_ssa_13_不含地址与哈希序痕迹() {
    let (f, tt) = c10_example();
    let out = dump_func(&f, &tt);
    assert!(!out.contains("0x"), "地址不可复现：{out}");
    assert!(!out.contains("Hash"), "哈希序不可复现：{out}");
    assert!(!out.contains("/Volumes"), "绝对路径不可复现：{out}");
}

#[test]
fn t_ssa_14_块段三个字段必须都在() {
    let (f, tt) = c10_example();
    let out = dump_func(&f, &tt);
    for line in out.lines().filter(|l| l.starts_with("block ")) {
        for field in ["params=", "preds=", "succs="] {
            assert!(
                line.contains(field),
                "块段缺字段 {field}（空也要写 `[]`）：{line}"
            );
        }
    }
    // 空列表写 []，不是省略
    assert!(out.contains("params=[]"), "空参数要写 []");
}

#[test]
fn t_ssa_14_块按编号升序且指令保持程序顺序() {
    let (f, tt) = c10_example();
    let out = dump_func(&f, &tt);
    let blocks: Vec<u32> = out
        .lines()
        .filter_map(|l| l.strip_prefix("block b"))
        .filter_map(|r| r.split(' ').next())
        .filter_map(|n| n.parse::<u32>().ok())
        .collect();
    assert_eq!(blocks, vec![0, 1, 2], "块必须按 BlockId 升序");

    // b0 的两条指令必须是 IR 里的顺序（const true 在 const 0 之前）
    let i_true = out.find("v0 = const true").expect("缺 v0");
    let i_zero = out.find("v1 = const 0").expect("缺 v1");
    assert!(i_true < i_zero, "块内指令必须按程序顺序");
}

#[test]
fn t_ssa_14_终止符带边实参且空写括号() {
    let (f, tt) = c10_example();
    let out = dump_func(&f, &tt);
    assert!(out.contains("term if v0 -> b1 () | b2 (v1)"), "{out}");
    assert!(out.contains("term goto b2 (v2)"), "{out}");
    assert!(out.contains("term return (v4)"), "{out}");
}

#[test]
fn t_ssa_14_每种指令都在_dump_里有稳定形状() {
    // 一张把 14 条指令都放进去的图：断言每一行都能被「v<k> = <op> ...」匹配上，
    // 且 op 名字在 kebab 白名单里 —— 加了变体却忘了写 dump 会在这里红。
    let mut fb = FnBuilder::bare("kitchen");
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
    let (f, tt) = fb.into_parts();

    let out = dump_func(&f, &tt);
    let allowed = [
        "const",
        "copy",
        "bin",
        "un",
        "convert",
        "addr-of",
        "field",
        "index",
        "slice",
        "composite",
        "make",
        "load",
        "store",
        "call",
    ];
    let mut ops: Vec<&str> = Vec::new();
    for line in out.lines().filter(|l| l.starts_with("  v")) {
        // `  v3 = <op> ...`
        let after_eq = line.split(" = ").nth(1).unwrap_or("");
        let op = after_eq.split(' ').next().unwrap_or("");
        assert!(
            allowed.contains(&op),
            "dump 里出现了没登记的 op `{op}`：{line}"
        );
        if !ops.contains(&op) {
            ops.push(op);
        }
    }
    ops.sort_unstable();
    let mut want = allowed.to_vec();
    want.sort_unstable();
    assert_eq!(ops, want, "14 条指令都要在 dump 里出现一次");
}
