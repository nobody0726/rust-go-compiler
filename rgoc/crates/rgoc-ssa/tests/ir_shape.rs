//! `T-SSA-01`：IR 变体的**穷尽性**（M6/Task 7）。
//!
//! # 这个测试为什么这样写
//!
//! 「变体穷尽」有两种坏写法：
//!
//! 1. 只写 `match` 不带 `_` —— 编译期能挡「漏处理」，但挡不住「加了变体却没人写测试」
//!    这一侧的漂移：新变体被处理了，但 dump / 构建 / 求值器都没跟上。
//! 2. 数一个常量 —— 常量与枚举不同源，改枚举时没人会想起它。
//!
//! 这里的做法：**每个变体各造一个实例**（`ALL_INSTR`），收集它们的 `kind()`，
//! 断言「名字去重后个数 == 实例个数」且「名字集合 == 冻结清单」。
//! 于是加一个变体，编译会逼你补 `kind()`；`ALL_INSTR` 与冻结清单都会红，
//! **强迫你回来决定**新变体在 dump / 构建 / 求值器里的位置。
//!
//! `Term` / `Root` / `CalleeRef` 同理。
//!
//! # 残留的洞（如实记）
//!
//! 如果有人加了变体、补了 `kind()`，但**既没加进 `ALL_INSTR` 也没改冻结清单**，
//! 这个测试仍然绿 —— 因为它数的是「我列出来的」，不是「枚举实际有几个」。
//! 编译期只有 `kind()` 的 `match` 能挡一半。彻底闭合需要反射/`strum`，而本仓
//! 不引第三方依赖（`D-M6-15`）。所以这里靠**两个手动清单互相校验**：
//! 加变体的人必须动其中一处，动错一处就红。

use std::collections::BTreeSet;

use rgoc_hir::{BinOp, BuiltinKind, FuncId, GlobalId, SlotId, UnOp};
use rgoc_ssa::ir::{
    BlockId, CalleeRef, Effect, Instr, Root, RootId, Term, ValueId, kind_of_callee, kind_of_instr,
    kind_of_root, kind_of_term,
};

/// 一个假后缀 id（只为构造实例，不参与语义）。
fn v(i: u32) -> ValueId {
    ValueId(i)
}
fn b(i: u32) -> BlockId {
    BlockId(i)
}
fn r(i: u32) -> RootId {
    RootId(i)
}
/// 一个假类型 id（只为构造实例）。
fn t() -> rgoc_types::TypeId {
    rgoc_types::TypeId::from_index(0)
}

/// 每个 `Instr` 变体各一个实例。
fn all_instrs() -> Vec<Instr> {
    vec![
        Instr::Const {
            dst: v(0),
            c: rgoc_hir::types::ConstVal::Int(1),
            typ: t(),
        },
        Instr::Copy {
            dst: v(1),
            src: v(0),
        },
        Instr::Bin {
            dst: v(2),
            op: BinOp::Add,
            lhs: v(0),
            rhs: v(1),
            typ: t(),
        },
        Instr::Un {
            dst: v(3),
            op: UnOp::Neg,
            x: v(0),
            typ: t(),
        },
        Instr::Convert {
            dst: v(4),
            x: v(0),
            typ: t(),
        },
        Instr::AddrOf {
            dst: v(5),
            root: r(0),
        },
        Instr::Field {
            dst: v(6),
            base: v(5),
            index: 0,
        },
        Instr::Index {
            dst: v(7),
            base: v(5),
            i: v(0),
        },
        Instr::Slice {
            dst: v(8),
            base: v(5),
            lo: None,
            hi: Some(v(0)),
            max: None,
        },
        Instr::Composite {
            dst: v(9),
            typ: t(),
            items: vec![v(0)],
        },
        Instr::Make {
            dst: v(10),
            typ: t(),
            len: Some(v(0)),
            cap: None,
        },
        Instr::Load {
            dst: v(11),
            mem: v(0),
            root: r(0),
            addr: v(5),
            typ: t(),
        },
        Instr::Store {
            dst: v(12),
            mem: v(11),
            root: r(0),
            addr: v(5),
            val: v(0),
        },
        Instr::Call {
            dsts: vec![v(13)],
            callee: CalleeRef::Func(FuncId(0)),
            args: vec![v(0)],
            effect: Effect::default(),
        },
    ]
}

#[test]
fn t_ssa_01_指令变体穷尽且名字唯一() {
    let instrs = all_instrs();
    let kinds: Vec<&'static str> = instrs.iter().map(kind_of_instr).collect();

    let uniq: BTreeSet<&str> = kinds.iter().copied().collect();
    assert_eq!(
        uniq.len(),
        instrs.len(),
        "有两个变体共用一个 kind 名：{kinds:?}"
    );

    // 冻结清单：加变体必须回来改这里（以及 dump / 构建 / 求值器）。
    const FROZEN: [&str; 14] = [
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
    let mut want: Vec<&str> = FROZEN.to_vec();
    want.sort_unstable();
    let mut got: Vec<&str> = uniq.into_iter().collect();
    got.sort_unstable();
    assert_eq!(got, want, "Instr 变体集合变了：补 dump 格式与穷尽表");
}

#[test]
fn t_ssa_01_终止符与根与调用者变体穷尽() {
    let terms = [
        Term::Goto {
            to: b(1),
            args: vec![v(0)],
        },
        Term::If {
            cond: v(0),
            then_: (b(1), vec![]),
            else_: (b(2), vec![]),
        },
        Term::Return(vec![v(0)]),
        Term::Panic(v(0)),
        Term::Unreachable,
    ];
    let kinds: BTreeSet<&str> = terms.iter().map(kind_of_term).collect();
    assert_eq!(kinds.len(), terms.len(), "Term 变体名重复");
    assert_eq!(
        kinds.into_iter().collect::<Vec<_>>(),
        vec!["goto", "if", "panic", "return", "unreachable"],
        "Term 变体集合变了"
    );

    let roots = [Root::Global(GlobalId(0)), Root::Slot(SlotId(0)), Root::Heap];
    let rk: BTreeSet<&str> = roots.iter().map(kind_of_root).collect();
    assert_eq!(rk.len(), 3, "Root 必须恰好三类（D-M6-5）");

    let callees = [
        CalleeRef::Func(FuncId(0)),
        CalleeRef::Method {
            recv: v(0),
            func: FuncId(0),
        },
        CalleeRef::Builtin(BuiltinKind::Len),
        CalleeRef::Indirect(v(0)),
    ];
    let ck: BTreeSet<&str> = callees.iter().map(kind_of_callee).collect();
    assert_eq!(ck.len(), callees.len(), "CalleeRef 变体名重复");
    assert_eq!(ck.len(), 4, "CalleeRef 必须四类");
}
