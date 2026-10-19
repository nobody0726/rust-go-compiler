//! `T-CK-25`：实例环与拒绝面（M4/Task 16）。
//!
//! # 判据从哪来
//!
//! 官方 `go/types` 对**嵌套实例**的裁决（`go1.27.1` 实测，探针
//! `.superpowers/sdd/M4-plan/probe/cycprobe.go`）：
//!
//! | 形态 | 官方 |
//! |---|---|
//! | `type A[T any] struct{ next *A[T] }` | 接受（自引用同参，链在 memo 处闭合） |
//! | `type A[T any] struct{ next *A[A[T]] }` | `instantiation cycle`（无界增长） |
//! | `type A[T any] struct{ x B[A[T]] }; type B[T any] struct{ y *A[T] }` | `instantiation cycle` |
//! | `var _ A[A[int]]` | 接受（有限嵌套） |
//!
//! 官方的环检测在 `mono.go`（**检查器**层、按实例化图报 `InvalidInstanceCycle`）；
//! 本层（类型表）做的是**同一条判据的下界**：链在 `(origin, args)` 上闭合 ⇒ 复用
//! memo；不闭合 ⇒ 深度越界时给稳定拒绝码，**不 panic、不静默给半成品**。
//!
//! # 表示层的前提（`T-TY-29` 的边界）
//!
//! 装载器把源码里的 `X[A]` **退化**为 `X`（D-M4-11 冻结），所以本文件的实例
//! 全部由测试**直接**用 [`TypeTable::add_named_instance`] 构造 —— 这是检查器
//! 后续会走的那条路（类型实参可以是实例）。
//!
//! # 三条纪律
//!
//! 1. **可以闭合的链必须成功**，且自引用指向**实例自身**（memo 先登记再展开）。
//! 2. **无界链必须稳定拒绝**（`TypInvalidInstanceCycle`），且**不留半成品**
//!    —— 失败后同一 `(origin, args)` 再问一次仍是拒绝，不是「命中残缺实例」。
//! 3. **替换没动的实例条目复用**（不重建、不增长表）—— 与 `subst.rs` 的
//!    「只重建受影响节点」同一条判据。

use rgoc_const::BasicId;
use rgoc_lex::DiagCode;
use rgoc_types::{Field, IfaceTSet, NamedId, Type, TypeId, TypeParamId, TypeTable, instantiate};

// ── 构造助手 ────────────────────────────────────────────────────────

fn any(tt: &mut TypeTable) -> TypeId {
    let i = tt.add_interface(Vec::new(), None, IfaceTSet::All);
    tt.interface_type(i)
}

/// 声明 `type <name>[T any] …`（本体由调用方通过 [`set_body_one`] 填）。
fn generic(tt: &mut TypeTable, name: &str) -> (NamedId, TypeParamId) {
    let a = any(tt);
    generic_in(tt, name, a)
}

/// 同上，但形参约束由调用方给（`T <constraint>`）。
fn generic_in(tt: &mut TypeTable, name: &str, constraint: TypeId) -> (NamedId, TypeParamId) {
    let tp = tt.add_type_param(0, constraint);
    let n = tt.add_named(name);
    tt.set_named_type_params(n, vec![tp]);
    (n, tp)
}

/// `interface{ ~T }`：单 `~T` 项约束（语法项集 + 语义项集都记）。
fn tilde(tt: &mut TypeTable, t: TypeId) -> TypeId {
    let u = tt.add_union(vec![(true, t)]);
    let ut = tt.union_type(u);
    let i = tt.add_interface(Vec::new(), Some(ut), IfaceTSet::Terms(vec![(true, t)]));
    tt.interface_type(i)
}

/// 单字段结构体本体：`struct { <fname> <ftyp> }`。
fn set_body_one(tt: &mut TypeTable, n: NamedId, fname: &str, ftyp: TypeId) {
    let st = tt.add_struct(vec![Field {
        name: Some(fname.into()),
        typ: ftyp,
        tag: None,
        embedded: false,
    }]);
    let st = tt.struct_type(st);
    tt.set_named_underlying(n, st);
}

/// `A[args]` 实例条目（不登记 memo —— 它是**类型表达式**，不是实例化调用）。
fn instance_of(tt: &mut TypeTable, origin: NamedId, args: Vec<TypeId>) -> TypeId {
    let id = tt.add_named_instance(origin, args);
    tt.named_type(id)
}

/// 取实例本体的唯一字段类型（断言失败时给出可读原因）。
fn only_field(tt: &TypeTable, inst_ty: TypeId) -> TypeId {
    let Type::Named(inst) = *tt.type_of(inst_ty) else {
        panic!("实例应是 named 条目，实际 {:?}", tt.type_of(inst_ty));
    };
    let under = tt.named_underlying(inst);
    let Type::Struct(sid) = *tt.type_of(under) else {
        panic!("实例本体应是 struct，实际 {:?}", tt.type_of(under));
    };
    let def = tt.struct_def(sid);
    assert_eq!(def.fields.len(), 1, "夹具形状：唯一字段");
    def.fields[0].typ
}

// ── ① 自引用同参：链在 memo 闭合 ──────────────────────────────────

#[test]
fn t_ck_25_自引用同参_替换到实例自身() {
    let mut tt = TypeTable::new();
    let (a, tp) = generic(&mut tt, "A");
    let t = tt.type_param_type(tp);
    // 本体：`struct{ next *A[T] }`（`A[T]` 是**实例**条目，不是起源）。
    let a_t = instance_of(&mut tt, a, vec![t]);
    let next = tt.add_pointer(a_t);
    set_body_one(&mut tt, a, "next", next);

    let int = tt.add_basic(BasicId::Int);
    let first = instantiate(&mut tt, a, &[int]).expect("自引用同参必须成功");
    let second = instantiate(&mut tt, a, &[int]).expect("memo 命中");
    assert_eq!(first, second, "同一 (origin, args) ⇒ 同一实例");

    let Type::Named(inst) = *tt.type_of(first) else {
        panic!("实例应是 named");
    };
    let field = only_field(&tt, first);
    let Type::Pointer(elem) = *tt.type_of(field) else {
        panic!("字段应是 *A[int]，实际 {:?}", tt.type_of(field));
    };
    assert_eq!(
        *tt.type_of(elem),
        Type::Named(inst),
        "自引用必须指向**实例自身**（memo 先登记再展开），不能是 `*A[T]`"
    );
}

// ── ② 自移位：无界增长 ⇒ 稳定拒绝 + 无半成品 ──────────────────────

#[test]
fn t_ck_25_自移位_稳定拒绝且不留半成品() {
    let mut tt = TypeTable::new();
    let (a, tp) = generic(&mut tt, "A");
    let t = tt.type_param_type(tp);
    // 本体：`struct{ next *A[A[T]] }`（官方 `instantiation cycle` 的最小形状）。
    let a_t = instance_of(&mut tt, a, vec![t]);
    let a_a_t = instance_of(&mut tt, a, vec![a_t]);
    let next = tt.add_pointer(a_a_t);
    set_body_one(&mut tt, a, "next", next);

    let int = tt.add_basic(BasicId::Int);
    assert_eq!(
        instantiate(&mut tt, a, &[int]),
        Err(DiagCode::TypInvalidInstanceCycle),
        "无界实例链 ⇒ 稳定拒绝（不是栈溢出 / panic）"
    );
    assert_eq!(
        instantiate(&mut tt, a, &[int]),
        Err(DiagCode::TypInvalidInstanceCycle),
        "失败必须回滚 memo：第二次不能命中残缺实例给 Ok"
    );
}

// ── ③ 互指链：A → B → A，实参每轮增长 ────────────────────────────

#[test]
fn t_ck_25_互指链_稳定拒绝() {
    let mut tt = TypeTable::new();
    let (a, tp) = generic(&mut tt, "A");
    let (b, up) = generic(&mut tt, "B");
    let t = tt.type_param_type(tp);
    let u = tt.type_param_type(up);
    // A[T] struct{ x B[A[T]] }
    let a_t = instance_of(&mut tt, a, vec![t]);
    let b_a_t = instance_of(&mut tt, b, vec![a_t]);
    set_body_one(&mut tt, a, "x", b_a_t);
    // B[T] struct{ y *A[T] }
    let a_u = instance_of(&mut tt, a, vec![u]);
    let py = tt.add_pointer(a_u);
    set_body_one(&mut tt, b, "y", py);

    let int = tt.add_basic(BasicId::Int);
    assert_eq!(
        instantiate(&mut tt, a, &[int]),
        Err(DiagCode::TypInvalidInstanceCycle),
        "A → B → A 的实参每轮增长 ⇒ 同一条拒绝码"
    );
}

// ── ④ 实例作实参：有限嵌套必须成功 ────────────────────────────────

#[test]
fn t_ck_25_实例作实参_有限嵌套() {
    let mut tt = TypeTable::new();
    let (a, tp) = generic(&mut tt, "A");
    let t = tt.type_param_type(tp);
    let a_t = instance_of(&mut tt, a, vec![t]);
    let next = tt.add_pointer(a_t);
    set_body_one(&mut tt, a, "next", next);

    let int = tt.add_basic(BasicId::Int);
    let inner = instantiate(&mut tt, a, &[int]).expect("内层");
    let outer = instantiate(&mut tt, a, &[inner]).expect("A[A[int]] 是有限嵌套，必须接受");

    let Type::Named(outer_id) = *tt.type_of(outer) else {
        panic!("实例应是 named");
    };
    let field = only_field(&tt, outer);
    let Type::Pointer(elem) = *tt.type_of(field) else {
        panic!("字段应是 *A[A[int]]");
    };
    assert_eq!(
        *tt.type_of(elem),
        Type::Named(outer_id),
        "外层实例的自引用同样落在 memo 上（不是内层，也不是 `*A[T]`）"
    );
}

// ── ⑤ 未受影响的实例：复用原条目 ──────────────────────────────────

#[test]
fn t_ck_25_未受影响的实例_复用原条目() {
    let mut tt = TypeTable::new();
    let (a, tp) = generic(&mut tt, "A");
    let (b, _) = generic(&mut tt, "B");
    let t = tt.type_param_type(tp);
    let int = tt.add_basic(BasicId::Int);

    // 字段一：*B[int]（实参不含 A 的形参 ⇒ 替换不该动它）。
    let b_int = instance_of(&mut tt, b, vec![int]);
    let keep = tt.add_pointer(b_int);
    // 字段二：T（受影响）。
    let st = tt.add_struct(vec![
        Field {
            name: Some("keep".into()),
            typ: keep,
            tag: None,
            embedded: false,
        },
        Field {
            name: Some("elem".into()),
            typ: t,
            tag: None,
            embedded: false,
        },
    ]);
    let st = tt.struct_type(st);
    tt.set_named_underlying(a, st);

    let inst = instantiate(&mut tt, a, &[int]).expect("实例化应成功");
    let Type::Named(id) = *tt.type_of(inst) else {
        panic!("实例应是 named");
    };
    let under = tt.named_underlying(id);
    let Type::Struct(sid) = *tt.type_of(under) else {
        panic!("本体应是 struct");
    };
    let fields = &tt.struct_def(sid).fields;
    assert_eq!(fields[0].typ, keep, "未受影响的 *B[int] 必须复用原条目");
    assert_eq!(
        *tt.type_of(fields[1].typ),
        Type::Basic(BasicId::Int),
        "受影响的 T 换成 int"
    );
}

// ── ⑥ 进行中的实例 + 非空接口约束：显式拒绝（我方边界） ────────────

#[test]
fn t_ck_25_进行中实例作受约束实参_显式拒绝() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let c = tilde(&mut tt, int);
    // A[T ~int] struct{ x T }
    let (a, tpa) = generic_in(&mut tt, "A", c);
    let ta = tt.type_param_type(tpa);
    set_body_one(&mut tt, a, "x", ta);
    // B[T any] struct{ y A[B[T]] }
    let (b, tpb) = generic(&mut tt, "B");
    let tb = tt.type_param_type(tpb);
    let b_t = instance_of(&mut tt, b, vec![tb]);
    let a_b_t = instance_of(&mut tt, a, vec![b_t]);
    set_body_one(&mut tt, b, "y", a_b_t);

    assert_eq!(
        instantiate(&mut tt, b, &[int]),
        Err(DiagCode::TypUnsupportedPendingInstanceArg),
        "急切展开看不到进行中实例的本体 ⇒ 稳定拒绝（不 panic、不静默答真）"
    );
}

#[test]
fn t_ck_25_进行中实例藏在指针下_同样显式拒绝() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let c = tilde(&mut tt, int);
    // A[T ~int] struct{ x T }
    let (a, tpa) = generic_in(&mut tt, "A", c);
    let ta = tt.type_param_type(tpa);
    set_body_one(&mut tt, a, "x", ta);
    // B[T any] struct{ y A[*B[T]] }
    let (b, tpb) = generic(&mut tt, "B");
    let tb = tt.type_param_type(tpb);
    let b_t = instance_of(&mut tt, b, vec![tb]);
    let pb_t = tt.add_pointer(b_t);
    let a_pb_t = instance_of(&mut tt, a, vec![pb_t]);
    set_body_one(&mut tt, b, "y", a_pb_t);

    assert_eq!(
        instantiate(&mut tt, b, &[int]),
        Err(DiagCode::TypUnsupportedPendingInstanceArg),
        "守卫必须递归进匿名构造 —— 否则方法集/项集会走到 `underlying` 而 panic"
    );
}
