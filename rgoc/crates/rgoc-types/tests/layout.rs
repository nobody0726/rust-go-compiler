//! 布局判据（M7/Task 8 的前置，`R-8-1`）。
//!
//! **每一个期望值都是实测的**：`go1.27.1 / linux-arm64` 的 `unsafe.Sizeof` /
//! `Alignof` / `Offsetof`（2026-10-24 容器内 `go run` 跑出来的数，见 `layout.rs` 模块文档的表）。
//!
//! 这里刻意**不**写成「按规范推一遍再断言」：布局是那种「推错一格，
//! 只有特定字段组合才崩」的东西，所以判据必须是外部锚点。

use rgoc_const::BigInt;
use rgoc_const::value::Const;
use rgoc_types::layout::Layout;
use rgoc_types::table::{Field, TypeTable};
use rgoc_types::{BasicId, field_offset, field_offsets, layout};

fn l(size: u32, align: u32) -> Layout {
    Layout { size, align }
}

fn basic(b: BasicId) -> TypeTable {
    let mut t = TypeTable::new();
    let _ = t.add_basic(b);
    t
}

fn assert_layout(tt: &TypeTable, t: rgoc_types::TypeId, size: u32, align: u32, what: &str) {
    assert_eq!(layout(tt, t), Some(l(size, align)), "{what}");
}

#[test]
fn 标量尺寸与对齐() {
    // (BasicId, size, align) —— 全部来自 unsafe.Sizeof/Alignof 实测。
    for (b, size, align) in [
        (BasicId::Bool, 1, 1),
        (BasicId::Int8, 1, 1),
        (BasicId::Uint8, 1, 1),
        (BasicId::Int16, 2, 2),
        (BasicId::Uint16, 2, 2),
        (BasicId::Int32, 4, 4),
        (BasicId::Uint32, 4, 4),
        (BasicId::Float32, 4, 4),
        (BasicId::Int64, 8, 8),
        (BasicId::Uint64, 8, 8),
        (BasicId::Float64, 8, 8),
        (BasicId::Int, 8, 8),
        (BasicId::Uint, 8, 8),
        (BasicId::Uintptr, 8, 8),
        // ★ 最容易写错的一格：complex64 的 align 是 **4**，不是 8。
        (BasicId::Complex64, 8, 4),
        (BasicId::Complex128, 16, 8),
        (BasicId::String, 16, 8),
        (BasicId::UnsafePointer, 8, 8),
    ] {
        let t = basic(b);
        let id = rgoc_types::TypeId::from_index(0);
        assert_layout(&t, id, size, align, &format!("{b:?}"));
    }
}

#[test]
fn 无类型常量按默认类型取布局() {
    // `R-13-1`：`untyped X` 的默认类型是 `X`（`untyped rune` ⇒ `int32`）。
    for (b, size, align) in [
        (BasicId::UntypedBool, 1, 1),
        (BasicId::UntypedInt, 8, 8),
        (BasicId::UntypedRune, 4, 4),
        (BasicId::UntypedFloat, 8, 8),
        (BasicId::UntypedComplex, 16, 8),
        (BasicId::UntypedString, 16, 8),
    ] {
        let t = basic(b);
        assert_layout(&t, rgoc_types::TypeId::from_index(0), size, align, &format!("{b:?}"));
    }
}

#[test]
fn untyped_nil_没有布局() {
    // `nil` 没有默认类型 —— 必须由上下文给。
    let t = basic(BasicId::UntypedNil);
    assert_eq!(layout(&t, rgoc_types::TypeId::from_index(0)), None);
}

#[test]
fn 引用型与接口() {
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int);
    let p = t.add_pointer(i);
    assert_layout(&t, p, 8, 8, "*int");

    let s = t.add_slice(i);
    assert_layout(&t, s, 24, 8, "[]int");

    let m = t.add_map(i, i);
    assert_layout(&t, m, 8, 8, "map[int]int");

    let c = t.add_chan(rgoc_types::ChanDir::SendRecv, i);
    assert_layout(&t, c, 8, 8, "chan int");

    let iface = t.add_interface(Vec::new(), None, rgoc_types::IfaceTSet::All);
    assert_layout(&t, t.interface_type(iface), 16, 8, "interface{}");
}

#[test]
fn 数组的尺寸与对齐() {
    let mut t = TypeTable::new();
    let i8_ = t.add_basic(BasicId::Int8);
    let i16 = t.add_basic(BasicId::Int16);
    let i64_ = t.add_basic(BasicId::Int64);
    let int = t.add_basic(BasicId::Int);

    let three = t.consts_mut().add(Const::Int(BigInt::from_i64(3)));
    let a3i8 = t.add_array(Some(three), i8_);
    assert_layout(&t, a3i8, 3, 1, "[3]int8");

    let a3i16 = t.add_array(Some(three), i16);
    assert_layout(&t, a3i16, 6, 2, "[3]int16");

    let a3i64 = t.add_array(Some(three), i64_);
    assert_layout(&t, a3i64, 24, 8, "[3]int64");

    let zero = t.consts_mut().add(Const::Int(BigInt::from_i64(0)));
    let a0 = t.add_array(Some(zero), int);
    // ★ 零大小但**对齐仍是元素的对齐**（实测 arr0: size 0 / align 8）。
    assert_layout(&t, a0, 0, 8, "[0]int");
}

#[test]
fn 长度未定的数组没有布局() {
    let mut t = TypeTable::new();
    let i = t.add_basic(BasicId::Int);
    let a = t.add_array(None, i);
    assert_eq!(layout(&t, a), None, "[...]T 没有布局可言");
}

fn field(name: &str, typ: rgoc_types::TypeId) -> Field {
    Field {
        name: Some(name.to_string()),
        typ,
        tag: None,
        embedded: false,
    }
}

#[test]
fn 结构体_字段间填充() {
    // S1{a int8; b int64} → size 16 / align 8 / Offsetof(b)=8
    let mut t = TypeTable::new();
    let i8_ = t.add_basic(BasicId::Int8);
    let i64_ = t.add_basic(BasicId::Int64);
    let s = t.add_struct(vec![field("a", i8_), field("b", i64_)]);
    let st = t.struct_type(s);
    assert_layout(&t, st, 16, 8, "S1");
    assert_eq!(field_offset(&t, s, 1), Some(8), "S1.b");
}

#[test]
fn 结构体_末尾零大小字段多占一字节() {
    // S2{a int64; b struct{}} → size **16**（不是 8）/ align 8 / Offsetof(b)=8
    let mut t = TypeTable::new();
    let i64_ = t.add_basic(BasicId::Int64);
    let empty = t.add_struct(vec![]);
    let s = t.add_struct(vec![field("a", i64_), field("b", t.struct_type(empty))]);
    let st = t.struct_type(s);
    assert_layout(&t, st, 16, 8, "S2");
    assert_eq!(field_offset(&t, s, 1), Some(8), "S2.b");
}

#[test]
fn 空结构体是零大小() {
    // S3{b struct{}} → size **0** / align 1（末尾零大小 +1 的规则**不适用**）
    let mut t = TypeTable::new();
    let empty = t.add_struct(vec![]);
    let st = t.struct_type(empty);
    assert_layout(&t, st, 0, 1, "struct{}");

    let s = t.add_struct(vec![field("b", st)]);
    let st2 = t.struct_type(s);
    assert_layout(&t, st2, 0, 1, "S3");
}

#[test]
fn 结构体_前导零大小字段不占位() {
    // S7{a struct{}; b int8} → size 1 / align 1 / Offsetof(b)=0
    let mut t = TypeTable::new();
    let empty = t.add_struct(vec![]);
    let i8_ = t.add_basic(BasicId::Int8);
    let s = t.add_struct(vec![field("a", t.struct_type(empty)), field("b", i8_)]);
    let st = t.struct_type(s);
    assert_layout(&t, st, 1, 1, "S7");
    assert_eq!(field_offset(&t, s, 1), Some(0), "S7.b");
}

#[test]
fn 结构体_尾部填充到对齐() {
    // S4{a int32; b int8} → size 8 / align 4 / Offsetof(b)=4
    let mut t = TypeTable::new();
    let i32_ = t.add_basic(BasicId::Int32);
    let i8_ = t.add_basic(BasicId::Int8);
    let s = t.add_struct(vec![field("a", i32_), field("b", i8_)]);
    let st = t.struct_type(s);
    assert_layout(&t, st, 8, 4, "S4");
    assert_eq!(field_offset(&t, s, 1), Some(4), "S4.b");

    // S5{a bool; b int16} → size 4 / align 2 / Offsetof(b)=2
    let b_ = t.add_basic(BasicId::Bool);
    let i16 = t.add_basic(BasicId::Int16);
    let s5 = t.add_struct(vec![field("a", b_), field("b", i16)]);
    assert_layout(&t, t.struct_type(s5), 4, 2, "S5");
    assert_eq!(field_offset(&t, s5, 1), Some(2), "S5.b");
}

#[test]
fn 结构体_数组字段的步长() {
    // S6{a int8; b [3]int16} → size 8 / align 2 / Offsetof(b)=2
    let mut t = TypeTable::new();
    let i8_ = t.add_basic(BasicId::Int8);
    let i16 = t.add_basic(BasicId::Int16);
    let three = t.consts_mut().add(Const::Int(BigInt::from_i64(3)));
    let a3 = t.add_array(Some(three), i16);
    let s = t.add_struct(vec![field("a", i8_), field("b", a3)]);
    assert_layout(&t, t.struct_type(s), 8, 2, "S6");
    assert_eq!(field_offset(&t, s, 1), Some(2), "S6.b");
}

#[test]
fn 全部字段偏移一次性给出() {
    let mut t = TypeTable::new();
    let i8_ = t.add_basic(BasicId::Int8);
    let i64_ = t.add_basic(BasicId::Int64);
    let i32_ = t.add_basic(BasicId::Int32);
    let s = t.add_struct(vec![field("a", i8_), field("b", i64_), field("c", i32_)]);
    assert_eq!(field_offsets(&t, s), Some(vec![0, 8, 16]));
    // size = 16 + 4 = 20 → 向上取整到 8 = 24
    assert_eq!(layout(&t, t.struct_type(s)), Some(l(24, 8)));
    // 越界字段 ⇒ None（不是 panic）
    assert_eq!(field_offset(&t, s, 3), None);
}

#[test]
fn 命名类型与别名穿透() {
    let mut t = TypeTable::new();
    let i64_ = t.add_basic(BasicId::Int64);
    let named = t.add_named("MyInt");
    t.set_named_underlying(named, i64_);
    let nt = t.named_type(named);
    assert_layout(&t, nt, 8, 8, "type MyInt int64");

    let alias = t.add_alias("Alias");
    t.set_alias_rhs(alias, nt);
    let at = t.alias_type(alias);
    assert_layout(&t, at, 8, 8, "type Alias = MyInt");
}

#[test]
fn 别名环不会栈溢出() {
    // `type A = A` 检查器会先拒，但 `layout` 是 pub 的 —— 手搓一个环必须**返回 None**
    // 而不是把进程打爆（深度守卫的存在理由）。
    let mut t = TypeTable::new();
    let a = t.add_alias("A");
    let at = t.alias_type(a);
    t.set_alias_rhs(a, at);
    assert_eq!(layout(&t, at), None);
}

#[test]
fn 不可算的类型返回_none() {
    let mut t = TypeTable::new();
    let invalid = t.invalid();
    assert_eq!(layout(&t, invalid), None, "Invalid 不许被编出一个尺寸");

    let tp = t.add_type_param(0, invalid);
    assert_eq!(layout(&t, t.type_param_type(tp)), None, "TypeParam");

    let tuple = t.add_tuple(vec![(None, invalid)]);
    assert_eq!(
        layout(&t, t.tuple_type(tuple)),
        None,
        "Tuple 不是运行期类型"
    );
}
