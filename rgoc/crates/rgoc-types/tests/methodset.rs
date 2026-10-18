//! `T-CK-14` 底座：方法集（M4/Task 7）。
//!
//! # 期望值从哪来
//!
//! 语义从 `go/types/methodset.go::NewMethodSet`（go1.27）逐条读出，并用容器内
//! `types.NewMethodSet` 真机探针复核过边界形态（值/指针接收者的过滤与**占名
//! 遮蔽**、同级歧义、多倍数排除、字段遮蔽、具名指针类型、递归类型、`*I` 无
//! 方法；记录见 `.superpowers/sdd/M4-plan/progress.md` 的 Task 7 行）。
//! **不是**抄 oracle 输出 —— oracle 对撞（`types --cmp methodset`）由 Task 15 接线。
//!
//! # 表怎么搭
//!
//! 全部手工建表（`table.rs` 的构造协议）。方法挂载走
//! [`TypeTable::add_named_method`]（`(名字, 签名)`；指针性由签名 `recv` 表达）。
//! `rgoc-load` 侧的挂载（真源码 → 表）由 `rgoc-load/tests/method_attach.rs` 管。

use rgoc_types::table::Field;
use rgoc_types::{NamedId, TypeId, TypeTable, methodset};

/// `type Name Underlying` —— 先占号再填底层。
fn named(tt: &mut TypeTable, name: &str, under: TypeId) -> NamedId {
    let n = tt.add_named(name);
    tt.set_named_underlying(n, under);
    n
}

/// `type Name struct{...}` —— 返回 `(NamedId, 类型条目)`。
fn struct_named(tt: &mut TypeTable, name: &str, fields: Vec<Field>) -> (NamedId, TypeId) {
    let st_id = tt.add_struct(fields);
    let st = tt.struct_type(st_id);
    let n = named(tt, name, st);
    (n, tt.named_type(n))
}

/// 无参无结果签名 `func()`（可带接收者）。
fn msig(tt: &mut TypeTable, recv: Option<TypeId>) -> TypeId {
    let t0 = tt.add_tuple(vec![]);
    let s = tt.add_signature(recv, t0, t0, false);
    tt.signature_type(s)
}

/// 嵌入字段。
fn embed(typ: TypeId) -> Field {
    Field {
        name: None,
        typ,
        tag: None,
        embedded: true,
    }
}

/// 普通字段。
fn field(name: &str, typ: TypeId) -> Field {
    Field {
        name: Some(name.to_string()),
        typ,
        tag: None,
        embedded: false,
    }
}

/// 方法集（`(名字, 签名)` 序列，按名字升序）。
fn entries(tt: &TypeTable, t: TypeId) -> Vec<(String, TypeId)> {
    methodset(tt, t)
        .expect("Task 7 的 methodset 在窄面内不该拒绝")
        .into_iter()
        .map(|e| (e.name, e.sig))
        .collect()
}

/// 方法名序列。
fn names(tt: &TypeTable, t: TypeId) -> Vec<String> {
    entries(tt, t).into_iter().map(|(n, _)| n).collect()
}

#[test]
fn t_ck_14_值接收者与指针接收者() {
    let mut tt = TypeTable::new();
    let (p, p_ty) = struct_named(&mut tt, "P", vec![]);
    let sig_val = msig(&mut tt, Some(p_ty));
    let p_ptr = tt.add_pointer(p_ty);
    let sig_ptr = msig(&mut tt, Some(p_ptr));
    tt.add_named_method(p, "Val", sig_val);
    tt.add_named_method(p, "Ptr", sig_ptr);
    let p_ptr2 = tt.add_pointer(p_ty);

    // T：只有值接收者方法
    assert_eq!(entries(&tt, p_ty), vec![("Val".to_string(), sig_val)]);
    // *T：值 + 指针接收者方法（按名字升序：Ptr < Val）
    assert_eq!(
        entries(&tt, p_ptr2),
        vec![("Ptr".to_string(), sig_ptr), ("Val".to_string(), sig_val)]
    );
}

#[test]
fn t_ck_14_嵌入提升_一层与两层() {
    let mut tt = TypeTable::new();
    // P2：值 M + 指针 PM
    let (p2, p2_ty) = struct_named(&mut tt, "P2", vec![]);
    let sig_m = msig(&mut tt, Some(p2_ty));
    let p2_ptr = tt.add_pointer(p2_ty);
    let sig_pm = msig(&mut tt, Some(p2_ptr));
    tt.add_named_method(p2, "M", sig_m);
    tt.add_named_method(p2, "PM", sig_pm);
    // E1 struct{ P2 }；E2 struct{ E1 }
    let (e1, e1_ty) = struct_named(&mut tt, "E1", vec![embed(p2_ty)]);
    let _ = e1;
    let (e2, e2_ty) = struct_named(&mut tt, "E2", vec![embed(e1_ty)]);
    let _ = e2;

    // 一层提升
    assert_eq!(entries(&tt, e1_ty), vec![("M".to_string(), sig_m)]);
    let e1_ptr = tt.add_pointer(e1_ty);
    assert_eq!(
        entries(&tt, e1_ptr),
        vec![("M".to_string(), sig_m), ("PM".to_string(), sig_pm)]
    );
    // 两层提升（*E2 的链上有指针 ⇒ PM 也在）
    assert_eq!(entries(&tt, e2_ty), vec![("M".to_string(), sig_m)]);
    let e2_ptr = tt.add_pointer(e2_ty);
    assert_eq!(
        entries(&tt, e2_ptr),
        vec![("M".to_string(), sig_m), ("PM".to_string(), sig_pm)]
    );
}

#[test]
fn t_ck_14_指针接收者在值方法集中占名遮蔽() {
    // 官方 `addOne` 的语义（探针复核）：值方法集里被 `!indirect && ptrRecv`
    // 过滤掉的方法**仍然占名**（碰撞位）—— 把更深处嵌出来的同名方法一并遮蔽。
    let mut tt = TypeTable::new();
    let (inner, inner_ty) = struct_named(&mut tt, "Inner", vec![]);
    let m_inner = msig(&mut tt, Some(inner_ty));
    tt.add_named_method(inner, "M", m_inner);

    let (t, t_ty) = struct_named(&mut tt, "T", vec![embed(inner_ty)]);
    let t_recv_ptr = tt.add_pointer(t_ty);
    let m_ptr = msig(&mut tt, Some(t_recv_ptr));
    tt.add_named_method(t, "M", m_ptr);

    // T 的方法集为空：深度 0 的指针方法 M 占了名，深度 1 的 Inner.M 出不来
    assert_eq!(names(&tt, t_ty), Vec::<String>::new());
    // *T 只有深度 0 的那个（指针接收者）—— 浅者胜
    let t_ptr = tt.add_pointer(t_ty);
    assert_eq!(entries(&tt, t_ptr), vec![("M".to_string(), m_ptr)]);
}

#[test]
fn t_ck_14_同级歧义与多倍数() {
    let mut tt = TypeTable::new();
    // A、B 各自有 M ⇒ E{ A; B } 歧义 ⇒ 排除
    let (a, a_ty) = struct_named(&mut tt, "A", vec![]);
    let m_a = msig(&mut tt, Some(a_ty));
    tt.add_named_method(a, "M", m_a);
    let (b, b_ty) = struct_named(&mut tt, "B", vec![]);
    let m_b = msig(&mut tt, Some(b_ty));
    tt.add_named_method(b, "M", m_b);
    let (_e, e_ty) = struct_named(&mut tt, "E", vec![embed(a_ty), embed(b_ty)]);
    assert_eq!(names(&tt, e_ty), Vec::<String>::new());
    let e_ptr = tt.add_pointer(e_ty);
    assert_eq!(names(&tt, e_ptr), Vec::<String>::new());

    // D{ L; R }，L、R 同嵌 C ⇒ C 在同一深度出现**多次** ⇒ 多倍数 ⇒ 排除
    let (c, c_ty) = struct_named(&mut tt, "C", vec![]);
    let m_c = msig(&mut tt, Some(c_ty));
    tt.add_named_method(c, "M", m_c);
    let (_l, l_ty) = struct_named(&mut tt, "L", vec![embed(c_ty)]);
    let (_r, r_ty) = struct_named(&mut tt, "R", vec![embed(c_ty)]);
    let (_d, d_ty) = struct_named(&mut tt, "D", vec![embed(l_ty), embed(r_ty)]);
    assert_eq!(names(&tt, d_ty), Vec::<String>::new());
    let d_ptr = tt.add_pointer(d_ty);
    assert_eq!(names(&tt, d_ptr), Vec::<String>::new());
}

#[test]
fn t_ck_14_字段遮蔽与深度最小者() {
    let mut tt = TypeTable::new();
    // Q{ M int; H }，H 有 M ⇒ 字段 M 在同深度碰撞 ⇒ 排除
    let int_ = tt.add_basic(rgoc_const::BasicId::Int);
    let (h, h_ty) = struct_named(&mut tt, "H", vec![]);
    let m_h = msig(&mut tt, Some(h_ty));
    tt.add_named_method(h, "M", m_h);
    let (_q, q_ty) = struct_named(&mut tt, "Q", vec![field("M", int_), embed(h_ty)]);
    assert_eq!(names(&tt, q_ty), Vec::<String>::new());
    let q_ptr = tt.add_pointer(q_ty);
    assert_eq!(names(&tt, q_ptr), Vec::<String>::new());

    // 深度最小者：Outer 自己的 M（深度 0）压过 Base.M（深度 1）
    let (base, base_ty) = struct_named(&mut tt, "Base", vec![]);
    let m_base = msig(&mut tt, Some(base_ty));
    tt.add_named_method(base, "M", m_base);
    let (outer, outer_ty) = struct_named(&mut tt, "Outer", vec![embed(base_ty)]);
    let m_outer = msig(&mut tt, Some(outer_ty));
    tt.add_named_method(outer, "M", m_outer);
    assert_eq!(entries(&tt, outer_ty), vec![("M".to_string(), m_outer)]);
}

#[test]
fn t_ck_14_接口方法集与指针接口() {
    let mut tt = TypeTable::new();
    let m = msig(&mut tt, None);
    let iid = tt.add_interface(vec![("M".to_string(), m)], None, false);
    let j_ty = tt.interface_type(iid);
    // 接口字面量本身
    assert_eq!(entries(&tt, j_ty), vec![("M".to_string(), m)]);
    // 具名接口（定义型 + 底层是接口）
    let j2 = named(&mut tt, "J", j_ty);
    let j2_ty = tt.named_type(j2);
    assert_eq!(entries(&tt, j2_ty), vec![("M".to_string(), m)]);
    // *I：无方法（官方 `isPtr && IsInterface` 早退）
    let j2_ptr = tt.add_pointer(j2_ty);
    assert_eq!(names(&tt, j2_ptr), Vec::<String>::new());
}

#[test]
fn t_ck_14_嵌入接口提升() {
    let mut tt = TypeTable::new();
    let m = msig(&mut tt, None);
    let iid = tt.add_interface(vec![("M".to_string(), m)], None, false);
    let j_ty = tt.interface_type(iid);
    // struct{ J }：接口方法在**下一层**加入（`indirect=true`）
    let (_e, e_ty) = struct_named(&mut tt, "E", vec![embed(j_ty)]);
    assert_eq!(entries(&tt, e_ty), vec![("M".to_string(), m)]);
    let e_ptr = tt.add_pointer(e_ty);
    assert_eq!(entries(&tt, e_ptr), vec![("M".to_string(), m)]);
}

#[test]
fn t_ck_14_具名指针类型为空() {
    // `type IP *P`：方法集**恒空**（官方开头 `asNamed && isPointer` 守卫），
    // `*IP` 也空（underlying 是 `*P`，走不进任何 offering 分支）。
    let mut tt = TypeTable::new();
    let (_p, p_ty) = struct_named(&mut tt, "P", vec![]);
    let p_ptr = tt.add_pointer(p_ty);
    let ip = named(&mut tt, "IP", p_ptr);
    let ip_ty = tt.named_type(ip);
    assert_eq!(names(&tt, ip_ty), Vec::<String>::new());
    let ip_ptr = tt.add_pointer(ip_ty);
    assert_eq!(names(&tt, ip_ptr), Vec::<String>::new());
}

#[test]
fn t_ck_14_递归与平凡输入() {
    // `type S struct{ *S }` + 值方法 SM：`seen` 防环，两个方向都是 {SM}
    let mut tt = TypeTable::new();
    let s = tt.add_named("S");
    let s_ty = tt.named_type(s);
    let s_self_ptr = tt.add_pointer(s_ty);
    let st_id = tt.add_struct(vec![embed(s_self_ptr)]);
    let st = tt.struct_type(st_id);
    tt.set_named_underlying(s, st);
    let sm = msig(&mut tt, Some(s_ty));
    tt.add_named_method(s, "SM", sm);
    assert_eq!(entries(&tt, s_ty), vec![("SM".to_string(), sm)]);
    let s_ptr = tt.add_pointer(s_ty);
    assert_eq!(entries(&tt, s_ptr), vec![("SM".to_string(), sm)]);

    // 平凡：基本型 / 通道 / invalid —— 全空，且**不** Err
    let int_ = tt.add_basic(rgoc_const::BasicId::Int);
    assert_eq!(names(&tt, int_), Vec::<String>::new());
    let ch = tt.add_chan(rgoc_types::ChanDir::SendRecv, int_);
    assert_eq!(names(&tt, ch), Vec::<String>::new());
    let inv = tt.invalid();
    assert_eq!(names(&tt, inv), Vec::<String>::new());
}
