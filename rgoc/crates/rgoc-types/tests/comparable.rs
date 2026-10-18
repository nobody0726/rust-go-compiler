//! `T-CK-32` 底座：可比性判定的验收（M4/Task 9）。
//!
//! # 期望值从哪来
//!
//! 语义从 `go/types/predicates.go::comparableType` / `types.Comparable`
//! （go1.27）逐条读出；边界形态（`untyped nil` / 接口动态面 / 类型参数约束 /
//! 嵌入字段）在 Task 8 的容器探针里复核过（台账 Task 8 行）。
//! **不是**抄 oracle 输出 —— oracle 对撞（`types --cmp`）由 Task 15 接线。
//!
//! # 两个面
//!
//! * [`comparable`] = 官方 `types.Comparable`（`comparableType(T, dynamic=true)`）
//!   —— 接口**动态值**面：普通接口恒可比（运行期才可能炸）；
//! * [`strictly_comparable`] = `dynamic = false` —— `implements` 的
//!   `checkComparability` 与类型集过滤（`Comparable ∩ 有限项`）的那一面。

use rgoc_const::BasicId;
use rgoc_types::table::Field;
use rgoc_types::{IfaceTSet, TypeId, TypeTable, comparable, strictly_comparable};

fn basic(tt: &mut TypeTable, id: BasicId) -> TypeId {
    tt.add_basic(id)
}

/// `type Name Underlying` —— 先占号再填底层（`table.rs` 的构造协议）。
fn named(tt: &mut TypeTable, name: &str, under: TypeId) -> TypeId {
    let n = tt.add_named(name);
    tt.set_named_underlying(n, under);
    tt.named_type(n)
}

/// 无方法的接口（只有类型集面）。
fn iface_tset(tt: &mut TypeTable, s: IfaceTSet) -> TypeId {
    let i = tt.add_interface(vec![], None, s);
    tt.interface_type(i)
}

/// 类型参数（约束 = 给定接口条目）。
fn tp(tt: &mut TypeTable, index: u32, constraint: TypeId) -> TypeId {
    let t = tt.add_type_param(index, constraint);
    tt.type_param_type(t)
}

fn field(name: &str, typ: TypeId) -> Field {
    Field {
        name: Some(name.to_string()),
        typ,
        tag: None,
        embedded: false,
    }
}

/// `dynamic = true` 面判「可比」。
fn cmp_yes(tt: &TypeTable, t: TypeId) {
    assert_eq!(comparable(tt, t), Ok(true), "应判「可比」");
}

/// `dynamic = true` 面判「不可比」。
fn cmp_no(tt: &TypeTable, t: TypeId) {
    assert_eq!(comparable(tt, t), Ok(false), "应判「不可比」");
}

/// 严格面判「可比」。
fn strict_yes(tt: &TypeTable, t: TypeId) {
    assert_eq!(strictly_comparable(tt, t), Ok(true), "严格面应判「可比」");
}

/// 严格面判「不可比」。
fn strict_no(tt: &TypeTable, t: TypeId) {
    assert_eq!(
        strictly_comparable(tt, t),
        Ok(false),
        "严格面应判「不可比」"
    );
}

#[test]
fn t_ck_32_基本型_指针_通道() {
    let mut tt = TypeTable::new();
    let int_ = basic(&mut tt, BasicId::Int);
    let untyped_int = basic(&mut tt, BasicId::UntypedInt);
    let untyped_nil = basic(&mut tt, BasicId::UntypedNil);
    let uintptr = basic(&mut tt, BasicId::Uintptr);
    let unsafe_p = basic(&mut tt, BasicId::UnsafePointer);

    cmp_yes(&tt, int_);
    strict_yes(&tt, int_);
    cmp_yes(&tt, untyped_int);
    // 官方注释：动态类型为 nil 的接口值不可比较 ⇒ `untyped nil` 是**唯一**
    // 不可比的基本型
    cmp_no(&tt, untyped_nil);
    strict_no(&tt, untyped_nil);
    // `unsafe.Pointer` 是 Basic（kind 是 `UnsafePointer`），照可比
    cmp_yes(&tt, uintptr);
    cmp_yes(&tt, unsafe_p);

    // 指针/通道**不展开元素**
    let p = tt.add_pointer(int_);
    cmp_yes(&tt, p);
    let slice = tt.add_slice(int_);
    let ch_both = tt.add_chan(rgoc_types::ChanDir::SendRecv, int_);
    let ch_recv = tt.add_chan(rgoc_types::ChanDir::RecvOnly, int_);
    let ch_send = tt.add_chan(rgoc_types::ChanDir::SendOnly, int_);
    cmp_yes(&tt, ch_both);
    cmp_yes(&tt, ch_recv);
    cmp_yes(&tt, ch_send);
    // `chan []int` 照可比（元素是 slice 也一样）
    let ch_slice = tt.add_chan(rgoc_types::ChanDir::SendRecv, slice);
    cmp_yes(&tt, ch_slice);
    // `*[]int` 照可比
    let p_slice = tt.add_pointer(slice);
    cmp_yes(&tt, p_slice);
}

#[test]
fn t_ck_32_结构体_数组_递归() {
    let mut tt = TypeTable::new();
    let int_ = basic(&mut tt, BasicId::Int);
    let slice_int = tt.add_slice(int_);

    let s_ok = tt.add_struct(vec![field("x", int_)]);
    cmp_yes(&tt, tt.struct_type(s_ok));
    strict_yes(&tt, tt.struct_type(s_ok));

    let s_bad = tt.add_struct(vec![field("v", slice_int)]);
    cmp_no(&tt, tt.struct_type(s_bad));
    strict_no(&tt, tt.struct_type(s_bad));

    // 混合：第二个字段不可比 ⇒ 整体不可比
    let s_mixed = tt.add_struct(vec![field("x", int_), field("v", slice_int)]);
    cmp_no(&tt, tt.struct_type(s_mixed));

    // 自引用经**指针**：`type N struct{ next *N }` —— 指针不展开，环不进递归
    let n = tt.add_named("N");
    let self_ty = tt.named_type(n);
    let ptr_n = tt.add_pointer(self_ty);
    let s_n = tt.add_struct(vec![field("next", ptr_n)]);
    tt.set_named_underlying(n, tt.struct_type(s_n));
    cmp_yes(&tt, self_ty);
    strict_yes(&tt, self_ty);

    // 自引用里挂着不可比字段：`type M struct{ v []int; next *M }`
    let m = tt.add_named("M");
    let self_m = tt.named_type(m);
    let ptr_m = tt.add_pointer(self_m);
    let s_m = tt.add_struct(vec![field("v", slice_int), field("next", ptr_m)]);
    tt.set_named_underlying(m, tt.struct_type(s_m));
    cmp_no(&tt, self_m);
    strict_no(&tt, self_m);

    // 数组看元素
    let three = tt
        .consts_mut()
        .add(rgoc_const::Const::Int(rgoc_const::BigInt::from_i64(3)));
    let a_ok = tt.add_array(Some(three), int_);
    cmp_yes(&tt, a_ok);
    let three2 = tt
        .consts_mut()
        .add(rgoc_const::Const::Int(rgoc_const::BigInt::from_i64(3)));
    let a_bad = tt.add_array(Some(three2), slice_int);
    cmp_no(&tt, a_bad);
    // 长度未定（`[...]T`）不影响可比面
    let a_none = tt.add_array(None, int_);
    cmp_yes(&tt, a_none);
}

#[test]
fn t_ck_32_接口面_动态与严格() {
    let mut tt = TypeTable::new();
    let int_ = basic(&mut tt, BasicId::Int);
    let string_ = basic(&mut tt, BasicId::String);
    let slice_int = tt.add_slice(int_);

    // 普通接口（类型集 𝓤）：动态面恒真（官方：「a non-type-parameter
    // interface is always comparable」）；严格面看类型集
    let any_if = iface_tset(&mut tt, IfaceTSet::All);
    cmp_yes(&tt, any_if);
    strict_no(&tt, any_if);
    // 具名接口同理（`isTypeParam` 为假 ⇒ 走动态面）
    let named_if = named(&mut tt, "I", any_if);
    cmp_yes(&tt, named_if);
    strict_no(&tt, named_if);

    // `comparable` 的接口形态：两个面都真
    let cmp_if = iface_tset(&mut tt, IfaceTSet::Comparable);
    cmp_yes(&tt, cmp_if);
    strict_yes(&tt, cmp_if);

    // 有限项全严格可比：两个面都真
    let terms_ok = iface_tset(
        &mut tt,
        IfaceTSet::Terms(vec![(false, int_), (true, string_)]),
    );
    cmp_yes(&tt, terms_ok);
    strict_yes(&tt, terms_ok);

    // 有限项含 slice：严格面假、动态面真（动态值还可能带接口）
    let terms_slice = iface_tset(&mut tt, IfaceTSet::Terms(vec![(true, slice_int)]));
    cmp_yes(&tt, terms_slice);
    strict_no(&tt, terms_slice);

    // ∅：动态面**仍真**（官方只拒类型参数那一支）；严格面假
    let empty_if = iface_tset(&mut tt, IfaceTSet::Empty);
    cmp_yes(&tt, empty_if);
    strict_no(&tt, empty_if);
}

#[test]
fn t_ck_32_类型参数面() {
    let mut tt = TypeTable::new();
    let int_ = basic(&mut tt, BasicId::Int);
    let string_ = basic(&mut tt, BasicId::String);
    let slice_int = tt.add_slice(int_);

    let any_if = iface_tset(&mut tt, IfaceTSet::All);
    let cmp_if = iface_tset(&mut tt, IfaceTSet::Comparable);
    let terms_ok = iface_tset(
        &mut tt,
        IfaceTSet::Terms(vec![(true, int_), (true, string_)]),
    );
    let terms_slice = iface_tset(&mut tt, IfaceTSet::Terms(vec![(true, slice_int)]));
    let empty_if = iface_tset(&mut tt, IfaceTSet::Empty);

    // `[T any]`：底层剥到约束接口，但 `isTypeParam(t)` 为真 ⇒ **不**走
    // 动态面，严格/动态都看类型集（𝓤 不可比）⇒ 两个面都假
    let tp_any = tp(&mut tt, 0, any_if);
    cmp_no(&tt, tp_any);
    strict_no(&tt, tp_any);

    // `[T comparable]`：两个面都真
    let tp_cmp = tp(&mut tt, 1, cmp_if);
    cmp_yes(&tt, tp_cmp);
    strict_yes(&tt, tp_cmp);

    // `[T ~int | ~string]`：项全严格可比 ⇒ 两个面都真
    let tp_terms = tp(&mut tt, 2, terms_ok);
    cmp_yes(&tt, tp_terms);
    strict_yes(&tt, tp_terms);

    // `[T ~[]int]`：两个面都假
    let tp_slice = tp(&mut tt, 3, terms_slice);
    cmp_no(&tt, tp_slice);
    strict_no(&tt, tp_slice);

    // `[T interface{ ~int; ~[]int }]`（∅ 可达）：两个面都假
    let tp_empty = tp(&mut tt, 4, empty_if);
    cmp_no(&tt, tp_empty);
    strict_no(&tt, tp_empty);
}

#[test]
fn t_ck_32_结构体含接口字段_动态面穿透() {
    let mut tt = TypeTable::new();
    let int_ = basic(&mut tt, BasicId::Int);
    let any_if = iface_tset(&mut tt, IfaceTSet::All);
    let cmp_if = iface_tset(&mut tt, IfaceTSet::Comparable);

    // 动态面：接口字段恒过 ⇒ 结构体可比；严格面：𝓤 不过 ⇒ 不可比
    let s_any = tt.add_struct(vec![field("x", int_), field("i", any_if)]);
    cmp_yes(&tt, tt.struct_type(s_any));
    strict_no(&tt, tt.struct_type(s_any));

    // 严格面：`comparable` 约束的接口字段严格可比 ⇒ 过
    let s_cmp = tt.add_struct(vec![field("i", cmp_if)]);
    cmp_yes(&tt, tt.struct_type(s_cmp));
    strict_yes(&tt, tt.struct_type(s_cmp));

    // 类型参数字段**不**走动态面（`isTypeParam` 为真）⇒ `[T any]` 字段
    // 在**两个面**里都不过
    let tp_any = tp(&mut tt, 0, any_if);
    let s_tp = tt.add_struct(vec![field("t", tp_any)]);
    cmp_no(&tt, tt.struct_type(s_tp));
    strict_no(&tt, tt.struct_type(s_tp));
}
