//! `T-CK-33` 底座：可转换性判定的验收（M4/Task 9）。
//!
//! # 期望值从哪来
//!
//! 语义从 `go/types/conversions.go::(*operand).convertibleTo`（go1.27）逐条
//! 读出，**全部边界形态**用容器内 gc 探针复核过（合法/非法逐条两轮探针，
//! 记录见台账 Task 9 行）—— 探针抓到过一处读码误判：官方 slice→array 转换
//! **只比元素、不比长度**（`[]int → *[4]int` 合法）。
//! **不是**抄 oracle 输出 —— oracle 对撞（`types --cmp convertible`）由 Task 15 接线。
//!
//! # v1 口径（两处刻意的边界）
//!
//! 1. **版本门恒允许**：slice→array（go1.20）与 slice→*array（go1.17）在
//!    官方 `check == nil`（`convertibleTo` 直调）口径下恒允许；检查器层
//!    （Task 22）按文件版本另行门控（与 `Ruling 8-3` 的 comparable 门同款）；
//! 2. **第 1 条（可赋值）的 `Err` 吞掉继续**：官方此步只是快路径；类型参数
//!    形态的正确性由第 13 条**逐项展开**独立给出（官方 `Vp.is`/`Tp.is`），
//!    所以「答不出可赋值」不等于「答不出可转换」。**M4/Task 10 后**：
//!    `identical` 完整作答（`U-M3-05` 解除）⇒ 第 3/4/11 条不再有可透传的界；
//!    本函数剩下的 `Err` 来源只有第 1 条**被吞掉**的 `assignable` 边界
//!    （`TypUnsupportedAssignability`，不外泄）。`Result` 形态保留（调用面稳定）。

use rgoc_const::BasicId;
use rgoc_types::table::Field;
use rgoc_types::{
    ChanDir, IfaceTSet, TypeId, TypeTable, convertible, identical, identical_ignore_tags,
};

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

fn field(name: &str, typ: TypeId, tag: Option<&str>) -> Field {
    Field {
        name: Some(name.to_string()),
        typ,
        tag: tag.map(str::to_string),
        embedded: false,
    }
}

/// 判得出且为真。
fn yes(tt: &TypeTable, v: TypeId, t: TypeId) {
    assert_eq!(convertible(tt, v, t), Ok(true), "应判「可转换」");
}

/// 判得出且为假（**明确回答**）。
fn no(tt: &TypeTable, v: TypeId, t: TypeId) {
    assert_eq!(convertible(tt, v, t), Ok(false), "应判「不可转换」");
}

#[test]
fn t_ck_33_规则1_可赋值与底层同一() {
    let mut tt = TypeTable::new();
    let int_ = basic(&mut tt, BasicId::Int);
    let bool_ = basic(&mut tt, BasicId::Bool);

    yes(&tt, int_, int_);
    // 具名 ↔ 底层（规则 3：IgnoreTags 同一 + 至少一侧未命名 —— 基础型一侧
    // 永远满足；官方规则 5 的 `hasName` 只约束「两侧都有名字」的形态）
    let n = named(&mut tt, "N", int_);
    yes(&tt, n, int_);
    yes(&tt, int_, n);

    // 通道方向靠规则 1（可赋值面）
    let ch = tt.add_chan(ChanDir::SendRecv, int_);
    let rc = tt.add_chan(ChanDir::RecvOnly, int_);
    let sc = tt.add_chan(ChanDir::SendOnly, int_);
    yes(&tt, ch, rc);
    yes(&tt, ch, sc);

    // 基础型之间没有底层同一面
    no(&tt, int_, bool_);
}

#[test]
fn t_ck_33_底层同一_标签不参与() {
    let mut tt = TypeTable::new();
    let int_ = basic(&mut tt, BasicId::Int);
    let sid1 = tt.add_struct(vec![field("X", int_, Some("a"))]);
    let sid2 = tt.add_struct(vec![field("X", int_, Some("b"))]);
    let sid3 = tt.add_struct(vec![field("X", int_, Some("a"))]);
    let s1t = tt.struct_type(sid1);
    let s2t = tt.struct_type(sid2);
    let s3t = tt.struct_type(sid3);

    // 严格面：标签参与 ⇒ 不同
    assert_eq!(identical(&tt, s1t, s2t), Ok(false), "严格 identical");
    // 忽略标签面：同
    assert_eq!(identical_ignore_tags(&tt, s1t, s2t), Ok(true));
    assert_eq!(identical_ignore_tags(&tt, s1t, s3t), Ok(true), "同标签照同");

    // 转换面：`S2(s1)` / `S1(s2)` 合法（探针；规则 3 用的是 IgnoreTags）
    let n1 = named(&mut tt, "S1", s1t);
    let n2 = named(&mut tt, "S2", s2t);
    yes(&tt, n1, n2);
    yes(&tt, n2, n1);

    // 非标签字段差异不豁免：字段名不同 ⇒ 底层**不**同一 ⇒ 不可转换
    let sid4 = tt.add_struct(vec![field("Y", int_, Some("a"))]);
    let s4t = tt.struct_type(sid4);
    let n4 = named(&mut tt, "S4", s4t);
    no(&tt, n1, n4);
}

#[test]
fn t_ck_33_字符串_字节_rune() {
    let mut tt = TypeTable::new();
    let string_ = basic(&mut tt, BasicId::String);
    let int_ = basic(&mut tt, BasicId::Int);
    let uintptr = basic(&mut tt, BasicId::Uintptr);
    let float64 = basic(&mut tt, BasicId::Float64);
    let complex128 = basic(&mut tt, BasicId::Complex128);
    let uint8 = basic(&mut tt, BasicId::Uint8);
    let int32 = basic(&mut tt, BasicId::Int32);
    let slice_byte = tt.add_slice(uint8);
    let slice_rune = tt.add_slice(int32);
    let slice_int = tt.add_slice(int_);

    // 官方 `isBytesOrRunes`：`Byte = Uint8`、`Rune = Int32`（kind 常量相等）
    yes(&tt, slice_byte, string_);
    yes(&tt, string_, slice_byte);
    yes(&tt, slice_rune, string_);
    yes(&tt, string_, slice_rune);

    // 整数 → 字符串（rune 解释）；**浮点/复数不行**
    yes(&tt, int_, string_);
    yes(&tt, uintptr, string_);
    no(&tt, float64, string_);
    no(&tt, complex128, string_);
    no(&tt, string_, int_);

    // 元素不是 byte/rune 的 slice 不行；两 slice 之间也不行
    no(&tt, slice_int, string_);
    no(&tt, slice_byte, slice_rune);
    no(&tt, slice_rune, slice_byte);

    // 官方 `isBytesOrRunes` 用的是**元素的底层**（`s.elem.Underlying()`）
    // ⇒ `type MyByte uint8; []MyByte → string` 也算（探针复核）
    let my_byte = named(&mut tt, "MyByte", uint8);
    let slice_mybyte = tt.add_slice(my_byte);
    yes(&tt, slice_mybyte, string_);
}

#[test]
fn t_ck_33_数值与复数() {
    let mut tt = TypeTable::new();
    let int_ = basic(&mut tt, BasicId::Int);
    let int8 = basic(&mut tt, BasicId::Int8);
    let uint64 = basic(&mut tt, BasicId::Uint64);
    let float64 = basic(&mut tt, BasicId::Float64);
    let complex64 = basic(&mut tt, BasicId::Complex64);
    let complex128 = basic(&mut tt, BasicId::Complex128);
    let untyped_int = basic(&mut tt, BasicId::UntypedInt);

    yes(&tt, int_, float64);
    yes(&tt, float64, int_);
    yes(&tt, int8, uint64);
    yes(&tt, untyped_int, float64);
    yes(&tt, complex64, complex128);
    yes(&tt, complex128, complex64);

    // 整数/浮点 × 复数**互不相通**（官方规则 5/6；探针：`complex128(i)` 非法）
    no(&tt, int_, complex128);
    no(&tt, float64, complex128);
    no(&tt, complex128, int_);
    no(&tt, complex128, float64);
}

#[test]
fn t_ck_33_指针_unsafe() {
    let mut tt = TypeTable::new();
    let int_ = basic(&mut tt, BasicId::Int);
    let int8 = basic(&mut tt, BasicId::Int8);
    let uintptr = basic(&mut tt, BasicId::Uintptr);
    let unsafe_p = basic(&mut tt, BasicId::UnsafePointer);

    // 指针 ↔ unsafe.Pointer、uintptr ↔ unsafe.Pointer（规则 9/10）
    let p_int = tt.add_pointer(int_);
    yes(&tt, p_int, unsafe_p);
    yes(&tt, unsafe_p, p_int);
    yes(&tt, uintptr, unsafe_p);
    yes(&tt, unsafe_p, uintptr);

    // 指针 ↛ uintptr：必须经 unsafe.Pointer 中转（探针）
    no(&tt, p_int, uintptr);
    no(&tt, uintptr, p_int);

    // 规则 4：两侧指针，**base 的底层**（IgnoreTags）同一 ⇒ 可转换
    let n = named(&mut tt, "N", int_);
    let m = named(&mut tt, "M", int_);
    let pn = tt.add_pointer(n);
    let pm = tt.add_pointer(m);
    yes(&tt, pn, pm);

    // 元素底层不同 ⇒ 不可
    let p_int8 = tt.add_pointer(int8);
    no(&tt, p_int, p_int8);
}

#[test]
fn t_ck_33_通道方向() {
    let mut tt = TypeTable::new();
    let int_ = basic(&mut tt, BasicId::Int);
    let ch = tt.add_chan(ChanDir::SendRecv, int_);
    let rc = tt.add_chan(ChanDir::RecvOnly, int_);
    let sc = tt.add_chan(ChanDir::SendOnly, int_);

    // 双向 → 定向可赋值（规则 1）；反向与定向之间都不可
    yes(&tt, ch, rc);
    yes(&tt, ch, sc);
    no(&tt, rc, ch);
    no(&tt, sc, ch);
    no(&tt, rc, sc);
}

#[test]
fn t_ck_33_切片与数组() {
    let mut tt = TypeTable::new();
    let int_ = basic(&mut tt, BasicId::Int);
    let string_ = basic(&mut tt, BasicId::String);
    let uint8 = basic(&mut tt, BasicId::Uint8);
    let c3 = tt
        .consts_mut()
        .add(rgoc_const::Const::Int(rgoc_const::BigInt::from_i64(3)));
    let c4 = tt
        .consts_mut()
        .add(rgoc_const::Const::Int(rgoc_const::BigInt::from_i64(4)));
    let c3b = tt
        .consts_mut()
        .add(rgoc_const::Const::Int(rgoc_const::BigInt::from_i64(3)));
    let x = tt.add_slice(int_);
    let b = tt.add_slice(uint8);

    // v1 口径：go1.20 / go1.17 门**恒允许**（见模块文档）
    let a3 = tt.add_array(Some(c3), int_);
    let pa3 = tt.add_pointer(a3);
    let a4 = tt.add_array(Some(c4), int_);
    let pa4 = tt.add_pointer(a4);
    let a3s = tt.add_array(Some(c3), string_);
    let a3b = tt.add_array(Some(c3b), uint8);
    yes(&tt, x, a3);
    yes(&tt, x, pa3);
    // **只比元素不比长度**（官方；探针复核）
    yes(&tt, x, pa4);
    // 元素不同 ⇒ 不可
    no(&tt, x, a3s);
    // `[]byte → [3]byte` 同款
    yes(&tt, b, a3b);

    // 数组之间：长度不同 ⇒ 不「同一」⇒ 不可转换（也非数值面）
    no(&tt, a3, a4);
}

#[test]
fn t_ck_33_类型参数面_单项展开() {
    let mut tt = TypeTable::new();
    let int_ = basic(&mut tt, BasicId::Int);
    let string_ = basic(&mut tt, BasicId::String);
    let uint8 = basic(&mut tt, BasicId::Uint8);
    let slice_int = tt.add_slice(int_);
    let slice_byte = tt.add_slice(uint8);
    let slice_string = tt.add_slice(string_);
    let any_if = iface_tset(&mut tt, IfaceTSet::All);
    let cmp_if = iface_tset(&mut tt, IfaceTSet::Comparable);
    let terms_int = iface_tset(&mut tt, IfaceTSet::Terms(vec![(true, int_)]));
    let terms_str = iface_tset(&mut tt, IfaceTSet::Terms(vec![(true, string_)]));
    let terms_sbi = iface_tset(&mut tt, IfaceTSet::Terms(vec![(true, slice_byte)]));
    let terms_sii = iface_tset(&mut tt, IfaceTSet::Terms(vec![(true, slice_int)]));
    let terms_sss = iface_tset(&mut tt, IfaceTSet::Terms(vec![(true, slice_string)]));

    // `[T ~int]`：`T→int`、`int→T`、具名 `MyInt` 两向都过（探针）
    let tp_int = tp(&mut tt, 0, terms_int);
    let my = named(&mut tt, "MyInt", int_);
    yes(&tt, tp_int, int_);
    yes(&tt, int_, tp_int);
    yes(&tt, tp_int, my);
    yes(&tt, my, tp_int);

    // `[T ~[]byte]`：与 string 两向
    let tp_bytes = tp(&mut tt, 1, terms_sbi);
    yes(&tt, tp_bytes, string_);
    yes(&tt, string_, tp_bytes);

    // `[T ~[]int]`：string 面不过、int 面也不过（探针）
    let tp_intslice = tp(&mut tt, 2, terms_sii);
    no(&tt, tp_intslice, string_);
    no(&tt, tp_intslice, int_);

    // `[T any]` / `[T comparable]`：**没有具体项** ⇒ 两向都不过
    let tp_any = tp(&mut tt, 3, any_if);
    let tp_cmp = tp(&mut tt, 4, cmp_if);
    no(&tt, tp_any, int_);
    no(&tt, int_, tp_any);
    no(&tt, tp_cmp, int_);
    no(&tt, int_, tp_cmp);

    // 自身转换恒过（探针 `T any → T`）
    yes(&tt, tp_any, tp_any);

    // `T → any` 恒过（可赋值面短路；步骤 1 纵有 Err，单项展开也独立成立）
    yes(&tt, tp_int, any_if);

    // 双类型参数：逐对展开
    // `[T ~int] → [U ~string]`：int→string 是**合法**转换（整数→字符串，规则 7）
    let tp_str = tp(&mut tt, 5, terms_str);
    yes(&tt, tp_int, tp_str);
    // `[T ~[]int] → [U ~[]string]`：逐对都不行
    let tp_strslice = tp(&mut tt, 6, terms_sss);
    no(&tt, tp_intslice, tp_strslice);
    // `[T ~[]byte] → [U ~string]`：逐对 `[]byte→string` ⇒ 过
    let tp_str2 = tp(&mut tt, 7, terms_str);
    yes(&tt, tp_bytes, tp_str2);
}

#[test]
fn t_ck_33_接口类型集面_按集合判对() {
    // M4/Task 10（`identical` 升级）前：v1 对「两侧任一类型集 != 𝓤」的接口拒判
    //（`U-M3-05`）⇒ 可转换性第 3 条透传 `Err`。升级后：同集合 ⇒
    // `identical_ignore_tags` 真（第 3 条）⇒ 可转换；异集合 ⇒ 判 false。
    let mut tt = TypeTable::new();
    let int_ = basic(&mut tt, BasicId::Int);
    let i1 = iface_tset(&mut tt, IfaceTSet::Terms(vec![(true, int_)]));
    let i2 = iface_tset(&mut tt, IfaceTSet::Terms(vec![(true, int_)]));
    yes(&tt, i1, i2);
    let float_ = basic(&mut tt, BasicId::Float64);
    let i3 = iface_tset(&mut tt, IfaceTSet::Terms(vec![(true, float_)]));
    no(&tt, i1, i3);
}
