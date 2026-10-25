//! `ptr_mask` 判据（M9/Task 2，`T-META-04`）。
//!
//! # 期望值从哪来
//!
//! 位图的**形状**由 `M9-design.md` §3.4 的表定，而那张表的依据是官方
//! `internal/abi/type.go` 的 `PtrBytes` + `gcdata` 的语义（每 8 字节 1 位、
//! 只覆盖 `[0, ptrdata)`）。这里断言的是**形状**，不是「照抄某个数」——
//! 所以每条都写成「哪个字是根」，而不是「位图等于某串十六进制」。
//!
//! # 为什么这一层值得单独判
//!
//! 位图错了不会有任何编译错：`RM-2` 的症状是「M9 全绿，M10 一上就随机崩」。
//! 所以它必须在**造它的地方**（本 crate）就有牙。

use rgoc_const::BigInt;
use rgoc_const::value::Const;
use rgoc_types::table::{Field, TypeTable};
use rgoc_types::{BasicId, IfaceTSet, TypeId, ptr_mask};

fn field(name: &str, typ: TypeId) -> Field {
    Field {
        name: Some(name.to_string()),
        typ,
        tag: None,
        embedded: false,
    }
}

fn array_of(tt: &mut TypeTable, n: i64, elem: TypeId) -> TypeId {
    let c = tt.consts_mut().add(Const::Int(BigInt::from_i64(n)));
    tt.add_array(Some(c), elem)
}

/// 断言位图，并在失败时报出「哪个字错了」—— 只报 `left/right` 两个数组
/// 在 6 个字的位图上很难看出差在哪。
fn assert_mask(tt: &TypeTable, t: TypeId, want: &[bool], what: &str) {
    let got = ptr_mask(tt, t).unwrap_or_else(|| panic!("{what}：ptr_mask 返回 None"));
    assert_eq!(
        got.len(),
        want.len(),
        "{what}：字数不对（got {got:?}, want {want:?}）"
    );
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert_eq!(g, w, "{what}：第 {i} 个字（字节 {}) 判错", i * 8);
    }
}

#[test]
fn 五个特例的位图与_design_一致() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let string_ = tt.add_basic(BasicId::String);
    let p_int = tt.add_pointer(int_);
    let s_int = tt.add_slice(int_);
    let iid = tt.add_interface(vec![], None, IfaceTSet::All);
    let any_ = tt.interface_type(iid);
    let sid = tt.add_struct(vec![field("a", int_), field("b", p_int)]);
    let st = tt.struct_type(sid);

    assert_mask(&tt, p_int, &[true], "*int 是单字指针");
    assert_mask(&tt, string_, &[true, false], "string = ptr + len");
    assert_mask(&tt, s_int, &[true, false, false], "slice = ptr + len + cap");
    assert_mask(&tt, any_, &[true, true], "iface = 元数据指针 + data");
    assert_mask(
        &tt,
        st,
        &[false, true],
        "struct{a int; b *int} 只有字 1 是根",
    );
}

#[test]
fn 标量与空类型没有根() {
    let mut tt = TypeTable::new();
    for b in [
        BasicId::Int,
        BasicId::Float64,
        BasicId::Bool,
        BasicId::Uint8,
    ] {
        let t = tt.add_basic(b);
        let m = ptr_mask(&tt, t).expect("标量有布局");
        assert!(m.iter().all(|x| !x), "{b:?} 不该有根：{m:?}");
    }
    let sid = tt.add_struct(vec![]);
    let empty = tt.struct_type(sid);
    assert_eq!(
        ptr_mask(&tt, empty),
        Some(vec![]),
        "空结构体 0 字节 ⇒ 0 个字（不是 1 个 false）"
    );
}

#[test]
fn 数组按元素重复() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let p_int = tt.add_pointer(int_);
    let a_n = tt.add_array(None, p_int); // `[...]*int` 长度未定 ⇒ 无布局
    assert_eq!(ptr_mask(&tt, a_n), None, "[...]T 没有长度就没有位图");

    // [3]*int：三个字全是根。
    let a3 = array_of(&mut tt, 3, p_int);
    assert_mask(&tt, a3, &[true, true, true], "[3]*int");

    // [2]string：每个元素 2 字 ⇒ 4 字，奇数位是根。
    let string_ = tt.add_basic(BasicId::String);
    let a2 = array_of(&mut tt, 2, string_);
    assert_mask(&tt, a2, &[true, false, true, false], "[2]string");

    // [0]*int：零元素 ⇒ 零个字（与 `[0]int` 的 size 0 一致）。
    let a0 = array_of(&mut tt, 0, p_int);
    assert_eq!(ptr_mask(&tt, a0), Some(vec![]), "[0]*int 没有字");
}

#[test]
fn 嵌套结构体与多字字段() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let string_ = tt.add_basic(BasicId::String);
    let p_int = tt.add_pointer(int_);

    // struct{ s string; a int; p *int } ⇒ size 32 ⇒ 4 字
    //   s @0  ⇒ 字 0,1 = true,false
    //   a @16 ⇒ 字 2 = false
    //   p @24 ⇒ 字 3 = true
    let sid = tt.add_struct(vec![
        field("s", string_),
        field("a", int_),
        field("p", p_int),
    ]);
    let st = tt.struct_type(sid);
    assert_mask(
        &tt,
        st,
        &[true, false, false, true],
        "struct{s string; a int; p *int}",
    );

    // 内层结构体作字段：struct{ x [2]int; in struct{p *int} }
    //   x  @0  ⇒ 字 0,1 = false,false
    //   in @16 ⇒ 字 2 = true
    let inner_sid = tt.add_struct(vec![field("p", p_int)]);
    let inner = tt.struct_type(inner_sid);
    let a2_int = array_of(&mut tt, 2, int_);
    let outer_sid = tt.add_struct(vec![field("x", a2_int), field("in", inner)]);
    let outer = tt.struct_type(outer_sid);
    assert_mask(
        &tt,
        outer,
        &[false, false, true],
        "嵌套结构体的根要递归上来",
    );
}

#[test]
fn 单字引用类型都是根() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let p_int = tt.add_pointer(int_);
    let up = tt.add_basic(BasicId::UnsafePointer);
    let m = tt.add_map(int_, int_);
    let c = tt.add_chan(rgoc_types::typ::ChanDir::SendRecv, int_);
    let params = tt.add_tuple(vec![]);
    let results = tt.add_tuple(vec![]);
    let sig = tt.add_signature(None, params, results, false);
    let sig_ty = tt.signature_type(sig);

    for (t, what) in [
        (up, "unsafe.Pointer"),
        (m, "map[int]int"),
        (c, "chan int"),
        (sig_ty, "func()"),
    ] {
        assert_mask(&tt, t, &[true], what);
    }
    // 指针的指针也是单字（指向一个槽，槽里是另一个指针）
    let pp = tt.add_pointer(p_int);
    assert_mask(&tt, pp, &[true], "**int");
}

#[test]
fn 命名类型看本体() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let p_int = tt.add_pointer(int_);
    // type P *int —— 具名类型的位图必须等于它的本体（别给 Named 单开一支）
    let named = tt.add_named("P");
    tt.set_named_underlying(named, p_int);
    let named_ty = tt.named_type(named);
    assert_mask(&tt, named_ty, &[true], "type P *int");

    // 别名同理：type Q = []int
    let s_int = tt.add_slice(int_);
    let alias = tt.add_alias("Q");
    tt.set_alias_rhs(alias, s_int);
    let alias_ty = tt.alias_type(alias);
    assert_mask(&tt, alias_ty, &[true, false, false], "type Q = []int");
}
