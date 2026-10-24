//! 值分类判据（`T-NAT-01` 的前半：槽宽 / 对齐 / 传参类别）。
//!
//! 表里的每个数都来自 `C5` §7.2（槽宽）与 §6.1（寄存器）——
//! 而 §7.2 的数又是 `go1.27.1/arm64` 的 `unsafe.Sizeof` 实测（`rgoc-types::layout` 的文档表）。
//!
//! 这里**不测**「相邻槽不互相覆盖」——那要有真的帧，归 Task 9/16。

use rgoc_arm64::abi::{ArgKind, SlotClass, arg_kind, reg_count, slot_of};
use rgoc_const::BigInt;
use rgoc_const::value::Const;
use rgoc_types::table::{Field, TypeTable};
use rgoc_types::{BasicId, TypeId};

fn slot(tt: &TypeTable, t: TypeId) -> (SlotClass, u32) {
    let s = slot_of(tt, t).expect("应可分类");
    (s.class, s.size)
}

#[test]
fn 标量一律一个八字节槽() {
    let mut t = TypeTable::new();
    for b in [
        BasicId::Bool,
        BasicId::Int8,
        BasicId::Int16,
        BasicId::Int32,
        BasicId::Int64,
        BasicId::Uint8,
        BasicId::Uintptr,
        BasicId::UnsafePointer,
    ] {
        let id = t.add_basic(b);
        assert_eq!(slot(&t, id), (SlotClass::Int, 8), "{b:?}");
    }
}

#[test]
fn 浮点走浮点类别() {
    let mut t = TypeTable::new();
    let f32_ = t.add_basic(BasicId::Float32);
    let f64_ = t.add_basic(BasicId::Float64);
    assert_eq!(slot(&t, f32_).0, SlotClass::Float);
    assert_eq!(slot(&t, f64_).0, SlotClass::Float);
}

#[test]
fn 复数的两种宽度() {
    let mut t = TypeTable::new();
    let c64_ = t.add_basic(BasicId::Complex64);
    let c128 = t.add_basic(BasicId::Complex128);
    assert_eq!(
        slot(&t, c64_),
        (SlotClass::Complex64, 8),
        "complex64 一个槽"
    );
    assert_eq!(
        slot(&t, c128),
        (SlotClass::Complex128, 16),
        "complex128 十六字节"
    );
}

#[test]
fn 字符串占两个寄存器() {
    let mut t = TypeTable::new();
    let s = t.add_basic(BasicId::String);
    assert_eq!(
        slot(&t, s),
        (SlotClass::Int, 16),
        "string 的槽是 16 字节（ptr + len 两个字）"
    );
    // 但传参要两个 x：`(ptr, len)`。
    assert_eq!(reg_count(&t, s), Some(2), "string 传参占两个 x");
}

#[test]
fn 聚合的槽宽向上取八() {
    let mut t = TypeTable::new();
    let i8_ = t.add_basic(BasicId::Int8);
    let i64_ = t.add_basic(BasicId::Int64);
    let three = t.consts_mut().add(Const::Int(BigInt::from_i64(3)));
    let a3i8 = t.add_array(Some(three), i8_);
    assert_eq!(
        slot(&t, a3i8),
        (SlotClass::Agg, 8),
        "[3]int8 ⇒ 3 字节向上取 8"
    );

    let sixteen = t.consts_mut().add(Const::Int(BigInt::from_i64(16)));
    let a16i64 = t.add_array(Some(sixteen), i64_);
    assert_eq!(
        slot(&t, a16i64),
        (SlotClass::Agg, 128),
        "[16]int64 ⇒ 128 字节"
    );
}

#[test]
fn 零大小类型的槽宽是零() {
    let mut t = TypeTable::new();
    let empty = t.add_struct(vec![]);
    let st = t.struct_type(empty);
    assert_eq!(slot(&t, st), (SlotClass::Agg, 0), "struct{{}} 槽宽 0");
    // 但它仍然**占一个寄存器**（传的是地址）—— 槽宽 0 不等于「不传」。
    assert_eq!(reg_count(&t, st), Some(1));
    assert_eq!(arg_kind(SlotClass::Agg), ArgKind::Ptr);
}

#[test]
fn 结构体字段组合() {
    let mut t = TypeTable::new();
    let i8_ = t.add_basic(BasicId::Int8);
    let i64_ = t.add_basic(BasicId::Int64);
    let f = |name: &str, typ| Field {
        name: Some(name.to_string()),
        typ,
        tag: None,
        embedded: false,
    };
    // S1{a int8; b int64} ⇒ 16 字节（实测），向上取 8 仍是 16。
    let s = t.add_struct(vec![f("a", i8_), f("b", i64_)]);
    assert_eq!(slot(&t, t.struct_type(s)), (SlotClass::Agg, 16));
}

#[test]
fn 寄存器族() {
    assert_eq!(arg_kind(SlotClass::Int), ArgKind::X);
    assert_eq!(arg_kind(SlotClass::Float), ArgKind::V);
    assert_eq!(arg_kind(SlotClass::Complex64), ArgKind::V);
    assert_eq!(arg_kind(SlotClass::Complex128), ArgKind::V);
    assert_eq!(
        arg_kind(SlotClass::Agg),
        ArgKind::Ptr,
        "聚合按指针（与 AAPCS64 故意不同）"
    );
}

#[test]
fn 复数的寄存器个数() {
    let mut t = TypeTable::new();
    let c64_ = t.add_basic(BasicId::Complex64);
    let c128 = t.add_basic(BasicId::Complex128);
    let i = t.add_basic(BasicId::Int);
    // `R-13-2`：`complex64` 是**两个 `f32` 的 HFA** ⇒ AAPCS64 按元素分配寄存器
    // （两个 `s`），不是按大小打包进一个 `d`。
    assert_eq!(reg_count(&t, c64_), Some(2), "complex64 是两个 s 寄存器");
    assert_eq!(reg_count(&t, c128), Some(2), "complex128 实部虚部各一个 v");
    assert_eq!(reg_count(&t, i), Some(1));
}

#[test]
fn 命名类型按本体分类() {
    // `type MyFloat float64` 必须走**浮点**寄存器 —— 按名字分类会把它送进 x0。
    let mut t = TypeTable::new();
    let f64_ = t.add_basic(BasicId::Float64);
    let n = t.add_named("MyFloat");
    t.set_named_underlying(n, f64_);
    assert_eq!(slot(&t, t.named_type(n)), (SlotClass::Float, 8));
}

#[test]
fn 不可分类的类型返回_none() {
    let mut t = TypeTable::new();
    let inv = t.invalid();
    assert_eq!(slot_of(&t, inv), None);
    assert_eq!(reg_count(&t, inv), None);

    // 无类型常量按**默认类型**分类（`R-13-1`）：`print("hi")` 的实参就是
    // `untyped string`，后端不认它，整类程序都编不出来。
    let untyped_int = t.add_basic(BasicId::UntypedInt);
    assert_eq!(
        slot(&t, untyped_int),
        (SlotClass::Int, 8),
        "untyped int ⇒ int"
    );
    let untyped_str = t.add_basic(BasicId::UntypedString);
    assert_eq!(
        slot(&t, untyped_str),
        (SlotClass::Int, 16),
        "untyped string ⇒ string"
    );
    // 只有 `nil` 没有默认类型。
    let untyped_nil = t.add_basic(BasicId::UntypedNil);
    assert_eq!(slot_of(&t, untyped_nil), None, "nil 必须由上下文给类型");
}
