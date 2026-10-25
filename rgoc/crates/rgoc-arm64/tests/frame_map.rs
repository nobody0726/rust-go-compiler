//! `FRAME-MAP v1` 的判据（`T-NAT-51` 的格式与自洽面）。
//!
//! 这里钉四件事：头两行字段、`slot` 按 `ValueId` 升序、`safepoint` 的 `offs` 与活值一致、
//! 两次渲染逐字节相同。**与 `objdump` 逐偏移对账**要到 Task 17（要有真产物）。

use std::collections::BTreeMap;

use rgoc_arm64::frame::{layout, render_frame_map};
use rgoc_hir::types::ConstVal;
use rgoc_hir::{BuiltinKind, FuncId};
use rgoc_ssa::cfg::Cfg;
use rgoc_ssa::ir::{Block, BlockId, CalleeRef, Effect, Instr, SsaFunc, Term, ValueId};
use rgoc_ssa::live;
use rgoc_types::table::TypeTable;
use rgoc_types::{BasicId, TypeId};

/// 造一个 `main`：`v0 = const; v1 = const; print(v0); return v1`。
///
/// 两个值都**有定义点**（`V1` 要求）：v0 被调用用掉，v1 只在 `Return` 里用 ⇒
/// 跨过调用仍然活跃，正好当 safepoint 的判据。
fn func_with_call(value_types: Vec<TypeId>) -> SsaFunc {
    let t0 = value_types[0];
    let t1 = value_types[1];
    SsaFunc {
        captured: Vec::new(),
        ctx_abi: false,
        src: FuncId(0),
        name: "main".to_string(),
        params: Vec::new(),
        blocks: vec![Block {
            params: Vec::new(),
            instrs: vec![
                Instr::Const {
                    dst: ValueId(0),
                    c: ConstVal::Int(1),
                    typ: t0,
                },
                Instr::Const {
                    dst: ValueId(1),
                    c: ConstVal::Int(0),
                    typ: t1,
                },
                Instr::Call {
                    dsts: Vec::new(),
                    callee: CalleeRef::Builtin(BuiltinKind::Print),
                    args: vec![ValueId(0)],
                    dots: false,
                    effect: Effect::default(),
                },
            ],
            term: Term::Return(vec![ValueId(1)]),
        }],
        entry: BlockId(0),
        roots: Vec::new(),
        slot_root: BTreeMap::new(),
        slot_types: BTreeMap::new(),
        value_types,
        call_sigs: BTreeMap::new(),
    }
}

fn render(f: &SsaFunc, t: &TypeTable) -> String {
    let fr = layout(f, t).unwrap();
    let cfg = Cfg::build(f);
    let lv = live::analyze(f, &cfg, t);
    render_frame_map(f, &fr, &lv)
}

#[test]
fn 头两行字段齐() {
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int);
    let p = t.add_pointer(i);
    let text = render(&func_with_call(vec![i, p]), &t);
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("# FRAME-MAP v1"));
    let head = lines.next().unwrap();
    assert!(head.starts_with("# func main frame="), "{head}");
    assert!(head.contains("slots=2"), "slots 是值槽个数：{head}");
    assert_eq!(head.split_whitespace().nth(4), Some("slots=2"));
}

#[test]
fn slot_行按值号升序且字段齐() {
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int);
    let c128 = t.add_basic(BasicId::Complex128);
    let text = render(&func_with_call(vec![i, c128]), &t);
    let slots: Vec<&str> = text.lines().filter(|l| l.contains("# slot ")).collect();
    assert_eq!(slots.len(), 2);
    assert!(slots[0].contains("v0 "), "{}", slots[0]);
    assert!(slots[1].contains("v1 "), "{}", slots[1]);
    for l in &slots {
        for k in ["off=", "size=", "type="] {
            assert!(l.contains(k), "缺 {k}：{l}");
        }
    }
}

#[test]
fn safepoint_的偏移与活值一致() {
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int);
    let p = t.add_pointer(i);
    let f = func_with_call(vec![i, p]);
    let fr = layout(&f, &t).unwrap();
    let text = render(&f, &t);
    let sp_line = text
        .lines()
        .find(|l| l.contains("# safepoint "))
        .expect("print 是 safepoint");
    // v1 跨调用活跃 ⇒ 它的偏移必须出现在 offs 里。
    let off1 = fr.values[&ValueId(1)].off;
    assert!(
        sp_line.contains(&format!("[{off1}")) || sp_line.contains(&format!(",{off1}]")),
        "offs 里应有 v1 的偏移 {off1}：{sp_line}"
    );
    // 且 v1 是指针 ⇒ ptrs 里也要有它。
    assert!(
        sp_line.contains(&format!("ptrs=[{off1}]")),
        "ptrs 应恰好是 v1 的偏移：{sp_line}"
    );
}

#[test]
fn 偏移列表是数值升序() {
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int);
    let p = t.add_pointer(i);
    let text = render(&func_with_call(vec![i, p]), &t);
    let sp = text.lines().find(|l| l.contains("# safepoint ")).unwrap();
    let offs = sp
        .split("offs=[")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .unwrap();
    let nums: Vec<i32> = offs
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().unwrap())
        .collect();
    let mut sorted = nums.clone();
    sorted.sort_unstable();
    assert_eq!(nums, sorted, "offs 必须数值升序（-32 在 -24 之前）");
}

#[test]
fn 没有_safepoint_时只有头与_slot() {
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int);
    let f = SsaFunc {
        captured: Vec::new(),
        ctx_abi: false,
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
        slot_types: BTreeMap::new(),
        value_types: vec![i],
        call_sigs: BTreeMap::new(),
    };
    let text = render(&f, &t);
    assert!(
        !text.contains("# safepoint "),
        "没有 Call/Make 就没有 safepoint"
    );
    assert_eq!(text.lines().count(), 3, "头两行 + 一个 slot");
}

#[test]
fn 两次渲染逐字节相同() {
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int);
    let p = t.add_pointer(i);
    let f = func_with_call(vec![i, p, i]);
    assert_eq!(render(&f, &t), render(&f, &t));
}
