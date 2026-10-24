//! 取址槽零值初始化的判据（`T-NAT-03`）。
//!
//! # 只清取址槽
//!
//! 值槽不清：`V2`（定义支配使用）保证每一次读前面都写过，清它是多余指令。
//! 取址槽要清：`var a [3]int` 源码里没写过，`&a` 之后必须读到零值。
//!
//! 所以这里的判据有两半：**该清的清**（取址槽）、**不该清的不清**（值槽）。

use std::collections::BTreeMap;

use rgoc_arm64::emit::Asm;
use rgoc_arm64::frame::layout;
use rgoc_hir::{FuncId, SlotId};
use rgoc_ssa::ir::{Block, BlockId, SsaFunc, Term};
use rgoc_types::table::TypeTable;
use rgoc_types::{BasicId, TypeId};

fn func(value_types: Vec<TypeId>, slot_types: Vec<(SlotId, TypeId)>) -> SsaFunc {
    SsaFunc {
        src: FuncId(0),
        name: "f".to_string(),
        params: Vec::new(),
        blocks: vec![Block {
            params: Vec::new(),
            instrs: Vec::new(),
            term: Term::Return(Vec::new()),
        }],
        entry: BlockId(0),
        roots: Vec::new(),
        slot_root: BTreeMap::new(),
        slot_types: slot_types.into_iter().collect(),
        value_types,
        call_sigs: BTreeMap::new(),
    }
}

fn lines_of(f: &SsaFunc, t: &TypeTable) -> Vec<String> {
    let fr = layout(f, t).unwrap();
    let mut asm = Asm::new();
    fr.zero_init(&mut asm);
    asm.text()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

#[test]
fn 八字节取址槽清一次() {
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int);
    let f = func(Vec::new(), vec![(SlotId(0), i)]);
    let lines = lines_of(&f, &t);
    assert_eq!(lines.len(), 2, "一条 add + 一条 str，实际 {lines:?}");
    assert!(lines[0].starts_with("add x9, x29, #-"), "{}", lines[0]);
    assert_eq!(lines[1], "str xzr, [x9, #0]");
}

#[test]
fn 大槽按八字节步长清() {
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int64);
    let three = t
        .consts_mut()
        .add(rgoc_const::value::Const::Int(rgoc_const::BigInt::from_i64(
            3,
        )));
    let a3 = t.add_array(Some(three), i); // [3]int64 = 24 字节
    let f = func(Vec::new(), vec![(SlotId(0), a3)]);
    let lines = lines_of(&f, &t);
    assert_eq!(lines.len(), 4, "一条 add + 三条 str，实际 {lines:?}");
    assert_eq!(
        lines[1..],
        [
            "str xzr, [x9, #0]",
            "str xzr, [x9, #8]",
            "str xzr, [x9, #16]"
        ]
    );
}

#[test]
fn 零大小槽一条指令都不出() {
    let mut t = TypeTable::new();
    let empty = t.add_struct(Vec::new());
    let st = t.struct_type(empty);
    let f = func(Vec::new(), vec![(SlotId(0), st)]);
    assert!(
        lines_of(&f, &t).is_empty(),
        "零大小槽没有字节要清（地址由分配器给）"
    );
}

#[test]
fn 值槽不清() {
    // 只有值槽、没有取址槽 ⇒ 一条指令都不该有。
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int);
    let f = func(vec![i, i, i], Vec::new());
    assert!(
        lines_of(&f, &t).is_empty(),
        "值槽由 V2 保证先写后读，清它是多余指令"
    );
}

#[test]
fn 多个取址槽按槽号升序() {
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int);
    let f = func(Vec::new(), vec![(SlotId(2), i), (SlotId(0), i)]);
    let lines = lines_of(&f, &t);
    // 槽 0 的偏移比槽 2 浅（先分配）⇒ 先出现 -24，后出现 -32。
    assert!(lines[0].contains("#-24"), "槽 0 先清：{}", lines[0]);
    assert!(lines[2].contains("#-32"), "槽 2 后清：{}", lines[2]);
}
