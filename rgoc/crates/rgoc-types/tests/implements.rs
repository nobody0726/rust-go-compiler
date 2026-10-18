//! `T-CK-12/13` 底座：接口实现（`implements`，非约束）与约束满足
//! （`satisfies`，`constraint=true`）（M4/Task 8）。
//!
//! # 期望值从哪来
//!
//! 语义从官方 `go/types/instantiate.go::implements`（守卫序、空接口 / 空类型集
//! 边界、`checkComparability` 两态）与 `lookup.go::hasAllMethods/missingMethod`
//! （方法名 + 签名 `Identical`）逐条读出；`go/types/predicates.go::comparableType`
//! 与 `typeset.go::IsComparable` 是可比性面的权威。oracle 对撞
//! （`types --cmp implements|satisfies`）由 Task 15 接线。
//!
//! # 三档断言
//!
//! 判定面只有 `Ok(true)` / `Ok(false)` / `Err`（能力边界码）三档；
//! 本文件把「判得出」的两个方向如实断言（`yes` / `no`）。

use rgoc_const::BasicId;
use rgoc_types::table::Field;
use rgoc_types::typeset::IfaceTSet;
use rgoc_types::{NamedId, Type, TypeId, TypeTable, implements};

/// 判得出且为真。
fn yes(tt: &TypeTable, v: TypeId, t: TypeId, constraint: bool) {
    assert_eq!(implements(tt, v, t, constraint), Ok(true), "应判「满足」");
}

/// 判得出且为假（**明确回答**：不是「不判」）。
fn no(tt: &TypeTable, v: TypeId, t: TypeId, constraint: bool) {
    assert_eq!(
        implements(tt, v, t, constraint),
        Ok(false),
        "应判「不满足」"
    );
}

/// `type Name Underlying` —— 先占号再填底层（`table.rs` 的构造协议）。
fn named(tt: &mut TypeTable, name: &str, under: TypeId) -> TypeId {
    let n = tt.add_named(name);
    tt.set_named_underlying(n, under);
    tt.named_type(n)
}

/// 无参无结果签名 `func()`。
fn sig0(tt: &mut TypeTable) -> TypeId {
    let t0 = tt.add_tuple(vec![]);
    let s = tt.add_signature(None, t0, t0, false);
    tt.signature_type(s)
}

/// 方法签名 `func (recv) ()`（`recv` 存**接收者类型**本身）。
fn msig0(tt: &mut TypeTable, recv: TypeId) -> TypeId {
    let t0 = tt.add_tuple(vec![]);
    let s = tt.add_signature(Some(recv), t0, t0, false);
    tt.signature_type(s)
}

/// 普通接口（`tset = All`）。
fn iface(tt: &mut TypeTable, methods: Vec<(&str, TypeId)>) -> TypeId {
    iface_with(tt, methods, IfaceTSet::All)
}

/// 带指定语义类型集的接口。
fn iface_with(tt: &mut TypeTable, methods: Vec<(&str, TypeId)>, tset: IfaceTSet) -> TypeId {
    let iid = tt.add_interface(
        methods
            .into_iter()
            .map(|(n, s)| (n.to_string(), s))
            .collect(),
        None,
        tset,
    );
    tt.interface_type(iid)
}

/// `type Name struct { fields... }`。
fn struct_named(tt: &mut TypeTable, name: &str, fields: Vec<(Option<&str>, TypeId)>) -> TypeId {
    let sid = tt.add_struct(
        fields
            .into_iter()
            .map(|(n, t)| Field {
                name: n.map(str::to_string),
                typ: t,
                tag: None,
                embedded: false,
            })
            .collect(),
    );
    named(tt, name, tt.struct_type(sid))
}

/// 从 `TypeId` 拆 `NamedId`（表手工建好，unwrap 安全）。
fn tt_named(tt: &TypeTable, t: TypeId) -> NamedId {
    match tt.type_of(t) {
        Type::Named(n) => *n,
        other => panic!("期望具名类型，得 {other:?}"),
    }
}

#[test]
fn t_ck_12_空接口恒真_与_指向接口的指针() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let s0 = sig0(&mut tt);
    let empty = iface(&mut tt, vec![]);
    let m_iface = iface(&mut tt, vec![("M", s0)]);

    // 空接口：任意 V 恒真（含指向接口的指针、非接口类型）
    yes(&tt, int_, empty, false);
    let p_m = tt.add_pointer(m_iface);
    yes(&tt, p_m, empty, false);

    // 非空接口：V 必须实现
    no(&tt, int_, m_iface, false);

    // T 不是接口 ⇒ 假（含指向接口的指针作 T —— 官方 `Ti == nil` 面）
    no(&tt, int_, int_, false);
    no(&tt, int_, p_m, false);
}

#[test]
fn t_ck_12_方法名与签名全含() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let s0 = sig0(&mut tt);
    let t = iface(&mut tt, vec![("M", s0)]);

    // V1：**另一条**结构相同的签名条目 —— 签名比较走 identical，不是指针相等
    let v1 = named(&mut tt, "V1", int_);
    let s0b = sig0(&mut tt);
    tt.add_named_method(tt_named(&tt, v1), "M", s0b);
    yes(&tt, v1, t, false);

    // V2：有 M 但签名不同（带一个 int 参数）⇒ 假
    let v2 = named(&mut tt, "V2", int_);
    let m_int = {
        let p = tt.add_tuple(vec![(None, int_)]);
        let r = tt.add_tuple(vec![]);
        let s = tt.add_signature(None, p, r, false);
        tt.signature_type(s)
    };
    tt.add_named_method(tt_named(&tt, v2), "M", m_int);
    no(&tt, v2, t, false);

    // V3：没有 M（另有 N）⇒ 假
    let v3 = named(&mut tt, "V3", int_);
    let s0c = sig0(&mut tt);
    tt.add_named_method(tt_named(&tt, v3), "N", s0c);
    no(&tt, v3, t, false);

    // V4：M + 多余方法 P ⇒ 真（只要求全含）
    let v4 = named(&mut tt, "V4", int_);
    let s0d = sig0(&mut tt);
    let s0e = sig0(&mut tt);
    tt.add_named_method(tt_named(&tt, v4), "M", s0d);
    tt.add_named_method(tt_named(&tt, v4), "P", s0e);
    yes(&tt, v4, t, false);
}

#[test]
fn t_ck_12_指针接收者_值不实现_指针实现() {
    let mut tt = TypeTable::new();
    let s0 = sig0(&mut tt);
    let t = iface(&mut tt, vec![("M", s0)]);

    // P 的 M 是指针接收者：值 P 不实现；*P 实现
    //（签名 `recv` 存**指针类型**条目时才是指针接收者 —— 裸 `P` 是值接收者）
    let int_ = tt.add_basic(BasicId::Int);
    let p = named(&mut tt, "P", int_);
    let p_ptr = tt.add_pointer(p);
    let m_sig = msig0(&mut tt, p_ptr);
    tt.add_named_method(tt_named(&tt, p), "M", m_sig);
    no(&tt, p, t, false);
    yes(&tt, p_ptr, t, false);
}

#[test]
fn t_ck_12_空类型集一方恒满足() {
    let mut tt = TypeTable::new();
    let s0 = sig0(&mut tt);
    let int_ = tt.add_basic(BasicId::Int);

    // V 的类型集为空 ⇒ 满足任何接口（含空集 T —— V 空集先行）
    let v_empty = iface_with(&mut tt, vec![], IfaceTSet::Empty);
    let m_iface = iface(&mut tt, vec![("M", s0)]);
    let t_empty = iface_with(&mut tt, vec![], IfaceTSet::Empty);
    yes(&tt, v_empty, m_iface, false);
    yes(&tt, v_empty, t_empty, false);

    // T 空集、V 非空集 ⇒ 无一满足（非接口 V 与接口 V 两路）
    no(&tt, int_, t_empty, false);
    let v_iface = iface(&mut tt, vec![("M", s0)]);
    no(&tt, v_iface, t_empty, false);
}

#[test]
fn t_ck_13_约束_项集与并集() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let uint_ = tt.add_basic(BasicId::Uint);
    let string = tt.add_basic(BasicId::String);
    let bool_ = tt.add_basic(BasicId::Bool);

    // `~int`：底层是 int 的都满足
    let c_tilde_int = iface_with(&mut tt, vec![], IfaceTSet::Terms(vec![(true, int_)]));
    let my_int = named(&mut tt, "MyInt", int_);
    yes(&tt, my_int, c_tilde_int, true);
    yes(&tt, int_, c_tilde_int, true);
    no(&tt, uint_, c_tilde_int, true);
    no(&tt, string, c_tilde_int, true);

    // `~int | string`：并集
    let c_union = iface_with(
        &mut tt,
        vec![],
        IfaceTSet::Terms(vec![(true, int_), (false, string)]),
    );
    yes(&tt, int_, c_union, true);
    yes(&tt, string, c_union, true);
    no(&tt, bool_, c_union, true);

    // ⚠️ 非约束模式下 implements 仍做**成员判**（官方不为 `constraint=false`
    // 关掉项集面；该形态从 assignable 不可达 —— 普通接口不能带项集）——
    // 这里如实断言官方语义，不做「可达性收窄」。
    yes(&tt, int_, c_tilde_int, false);
    yes(&tt, my_int, c_union, false);
}

#[test]
fn t_ck_13_约束_comparable() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let c = iface_with(&mut tt, vec![], IfaceTSet::Comparable);

    // 基本型 / 数组 / 空 struct：满足
    yes(&tt, int_, c, true);
    let arr = tt.add_array(None, int_);
    yes(&tt, arr, c, true);
    let empty_struct = struct_named(&mut tt, "S0", vec![]);
    yes(&tt, empty_struct, c, true);

    // slice / 含 slice 的 struct：严格与规范都不可比较 ⇒ 不满足
    let slice = tt.add_slice(int_);
    no(&tt, slice, c, true);
    let with_slice = struct_named(&mut tt, "WS", vec![(Some("f"), slice)]);
    no(&tt, with_slice, c, true);

    // 指向含 slice 的 struct 的指针：指针恒可比较
    let ws_ptr = tt.add_pointer(with_slice);
    yes(&tt, ws_ptr, c, true);

    // 普通接口（非类型参数）：规范比较面恒真（`dynamic && !isTypeParam(T)`）
    let s0 = sig0(&mut tt);
    let plain_iface = iface(&mut tt, vec![("M", s0)]);
    yes(&tt, plain_iface, c, true);
    let empty = iface(&mut tt, vec![]);
    yes(&tt, empty, c, true);
}

#[test]
fn t_ck_13_约束_类型参数面() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);

    // `[T comparable]`：满足 comparable；`[T any]`：不满足
    let comparable_iface = iface_with(&mut tt, vec![], IfaceTSet::Comparable);
    let any_iface = iface(&mut tt, vec![]);
    let p_ok = tt.add_type_param(0, comparable_iface);
    let tp_ok = tt.type_param_type(p_ok);
    let p_any = tt.add_type_param(1, any_iface);
    let tp_any = tt.type_param_type(p_any);
    yes(&tt, tp_ok, comparable_iface, true);
    no(&tt, tp_any, comparable_iface, true);

    // `[T ~int]`：T ∈ `~int`（V 是接口 ⇒ 走「约束集 ⊆ 目标集」支）
    let tilde_int = iface_with(&mut tt, vec![], IfaceTSet::Terms(vec![(true, int_)]));
    let p_int = tt.add_type_param(2, tilde_int);
    let tp_int = tt.type_param_type(p_int);
    yes(&tt, tp_int, tilde_int, true);

    // `[T any]` 满足空接口（空接口恒真先行）
    yes(&tt, tp_any, any_iface, true);
}

#[test]
fn t_ck_13_约束_接口_v_子集() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);

    let v = iface_with(&mut tt, vec![], IfaceTSet::Terms(vec![(true, int_)]));
    let t_big = iface_with(
        &mut tt,
        vec![],
        IfaceTSet::Terms(vec![(true, int_), (false, string)]),
    );
    let t_small = iface_with(&mut tt, vec![], IfaceTSet::Terms(vec![(false, string)]));
    // V 的每个可能类型都在 T 里 ⇒ 满足；反过来不
    yes(&tt, v, t_big, true);
    no(&tt, t_big, v, true);
    // 不相交
    no(&tt, v, t_small, true);

    // V 是普通接口（集合 = 𝓤）：𝓤 ⊄ 有限项集
    let any_v = iface(&mut tt, vec![]);
    no(&tt, any_v, t_big, true);
}

#[test]
fn t_ck_12_invalid_守卫面() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let empty = iface(&mut tt, vec![]);
    let invalid = tt.invalid();

    // 两侧任一无效 ⇒ 真（避免连锁错误）
    yes(&tt, invalid, empty, false);
    yes(&tt, int_, invalid, false);

    // 指向无效基的指针 ⇒ 真（go.dev/issue/49541）
    let p_invalid = tt.add_pointer(invalid);
    yes(&tt, p_invalid, empty, false);
}
