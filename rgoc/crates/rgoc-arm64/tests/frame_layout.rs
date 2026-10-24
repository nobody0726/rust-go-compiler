//! 帧布局判据（`T-NAT-01/04` 的前半）。
//!
//! # 这一版测什么
//!
//! 只测**纯算术**：偏移、槽宽、帧大小、临时区。真正的「写 A 不覆盖 B」要到 Task 16
//! 有了可运行产物才验得了 —— 这里的「不互相覆盖」是**几何断言**（区间不相交），
//! 不是运行期断言。两者都要有：几何错了运行期必错，几何对了运行期仍可能错（写错偏移）。

use std::collections::BTreeMap;

use rgoc_arm64::frame::{FrameError, layout};
use rgoc_hir::{FuncId, SlotId};
use rgoc_ssa::ir::{Block, BlockId, SsaFunc, Term, ValueId};
use rgoc_types::table::TypeTable;
use rgoc_types::{BasicId, TypeId};

fn one_block(term: Term) -> Vec<Block> {
    vec![Block {
        params: Vec::new(),
        instrs: Vec::new(),
        term,
    }]
}

fn func(value_types: Vec<TypeId>, blocks: Vec<Block>) -> SsaFunc {
    SsaFunc {
        src: FuncId(0),
        name: "f".to_string(),
        params: Vec::new(),
        blocks,
        entry: BlockId(0),
        roots: Vec::new(),
        slot_root: BTreeMap::new(),
        slot_types: BTreeMap::new(),
        value_types,
        call_sigs: BTreeMap::new(),
    }
}

#[test]
fn 空函数的帧只有保存区() {
    let mut t = TypeTable::new();
    let f = func(Vec::new(), one_block(Term::Return(Vec::new())));
    let fr = layout(&f, &t).unwrap();
    assert_eq!(fr.frame_size, 16, "只有 saved fp/lr");
    assert_eq!(fr.sub_sp_imm(), 0, "stp 已经动了 16，sub 就是 0");
    assert_eq!(fr.tmp_base, None);
    let _ = t.add_basic(BasicId::Int); // 类型表与布局无关，这里只确认借用无副作用
}

#[test]
fn 相邻槽的区间不相交() {
    let mut t = TypeTable::new();
    let i8_ = t.add_basic(BasicId::Int8);
    let i64_ = t.add_basic(BasicId::Int64);
    let f = func(vec![i8_, i64_], one_block(Term::Return(Vec::new())));
    let fr = layout(&f, &t).unwrap();

    let a = fr.values[&ValueId(0)];
    let b = fr.values[&ValueId(1)];
    // 第一个槽占 [fp-24, fp-16)（fp+0/+8 是 saved fp/lr）。
    assert_eq!((a.off, a.size), (-24, 8), "v0 的槽");
    assert_eq!((b.off, b.size), (-32, 8), "v1 的槽");
    // 几何断言：两个区间不相交（[off, off+size) 为左闭右开）。
    assert!(
        a.off + a.size as i32 <= b.off || b.off + b.size as i32 <= a.off,
        "槽区间相交：{a:?} 与 {b:?}"
    );
    assert_eq!(fr.frame_size, 32, "16 保存 + 16 两个槽");
}

#[test]
fn 零大小值不推进游标() {
    let mut t = TypeTable::new();
    let empty = t.add_struct(Vec::new());
    let st = t.struct_type(empty);
    let i64_ = t.add_basic(BasicId::Int64);
    let f = func(vec![st, i64_], one_block(Term::Return(Vec::new())));
    let fr = layout(&f, &t).unwrap();
    assert_eq!(
        (fr.values[&ValueId(0)].off, fr.values[&ValueId(0)].size),
        (-16, 0)
    );
    assert_eq!(
        (fr.values[&ValueId(1)].off, fr.values[&ValueId(1)].size),
        (-24, 8)
    );
}

#[test]
fn 大槽按自身宽度占位() {
    let mut t = TypeTable::new();
    let c128 = t.add_basic(BasicId::Complex128);
    let i = t.add_basic(BasicId::Int);
    let f = func(vec![c128, i], one_block(Term::Return(Vec::new())));
    let fr = layout(&f, &t).unwrap();
    assert_eq!(
        (fr.values[&ValueId(0)].off, fr.values[&ValueId(0)].size),
        (-32, 16)
    );
    assert_eq!(
        (fr.values[&ValueId(1)].off, fr.values[&ValueId(1)].size),
        (-40, 8)
    );
    // 16 + 16 + 8 = 40 → 向上取 16 = 48
    assert_eq!(fr.frame_size, 48);
    assert_eq!(fr.sub_sp_imm(), 32);
}

#[test]
fn 临时区按最大边实参() {
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int);
    // 一个块，`goto b1(v0, v1, v2)` —— 边上有 3 个实参。
    let term = Term::Goto {
        to: BlockId(1),
        args: vec![ValueId(0), ValueId(1), ValueId(2)],
    };
    let f = func(vec![i, i, i], one_block(term));
    let fr = layout(&f, &t).unwrap();
    assert_eq!(fr.tmp_slots, 3);
    // 16 + 3*8 = 40（槽） + 3*8 = 64（临时区）
    assert_eq!(fr.frame_size, 64);
    assert_eq!(fr.tmp_base, Some(-64), "临时区起点 = 帧底");
}

#[test]
fn 条件跳转的两条边取较大者() {
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int);
    let term = Term::If {
        cond: ValueId(0),
        then_: (BlockId(1), vec![ValueId(1)]),
        else_: (BlockId(2), vec![ValueId(1), ValueId(2)]),
    };
    let f = func(vec![i, i, i], one_block(term));
    let fr = layout(&f, &t).unwrap();
    assert_eq!(fr.tmp_slots, 2, "取两条边里较大的那条");
}

#[test]
fn 帧大小恒为十六的倍数() {
    let mut t = TypeTable::new();
    let i8_ = t.add_basic(BasicId::Int8);
    // 1..=5 个值各算一次
    for n in 1..=5usize {
        let f = func(vec![i8_; n], one_block(Term::Return(Vec::new())));
        let fr = layout(&f, &t).unwrap();
        assert_eq!(fr.frame_size % 16, 0, "n={n} 的帧大小 {}", fr.frame_size);
        assert!(fr.frame_size >= 16);
    }
}

#[test]
fn 取址槽进独立区() {
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int);
    let mut f = func(vec![i], one_block(Term::Return(Vec::new())));
    f.slot_types.insert(SlotId(0), i);
    let fr = layout(&f, &t).unwrap();
    assert_eq!(fr.values.len(), 1);
    assert_eq!(fr.addr_slots.len(), 1);
    // 值槽占 [fp-24,-16)，取址槽在它下面 [fp-32,-24)
    assert_eq!(fr.addr_slots[&SlotId(0)].off, -32);
}

#[test]
fn 同一输入两次布局相同() {
    // 确定性（`T-NAT-48` 的一半）：帧是 `.s` 的一部分，帧不定则 `.s` 不定。
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int);
    let c128 = t.add_basic(BasicId::Complex128);
    let f = func(vec![i, c128, i], one_block(Term::Return(Vec::new())));
    assert_eq!(layout(&f, &t).unwrap(), layout(&f, &t).unwrap());
}

#[test]
fn 无布局的类型报错并带值号() {
    let mut t = TypeTable::new();
    let inv = t.invalid();
    let f = func(vec![inv], one_block(Term::Return(Vec::new())));
    assert_eq!(
        layout(&f, &t).unwrap_err(),
        FrameError::Value {
            value: ValueId(0),
            typ: inv
        }
    );
}

#[test]
fn 取址槽无布局也报错() {
    let mut t = TypeTable::new();
    let inv = t.invalid();
    let mut f = func(Vec::new(), one_block(Term::Return(Vec::new())));
    f.slot_types.insert(SlotId(7), inv);
    assert_eq!(
        layout(&f, &t).unwrap_err(),
        FrameError::AddrSlot {
            slot: SlotId(7),
            typ: inv
        }
    );
}
