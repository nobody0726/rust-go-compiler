//! `T-CK-21` 底座：显式实例化（`instantiate`）与类型参数替换（`substitute`）
//! （M4/Task 12；`U-M3-04` 翻转的第一件）。
//!
//! # 期望值从哪来
//!
//! 官方 `go/types/instantiate.go`：
//!
//! * `validateTArgLen` —— 数量错的稳定拒绝（`WrongTypeArgCount`）；
//! * `verify` —— 约束满足校验 = `implements(targ, bound, constraint=true)`，
//!   其中 `bound` 先按**实参**替换（`makeSubstMap(tparams, targs)`）；
//! * `instance` / `Context.lookup` —— **按 `Identical` 实参比对复用实例**
//!   （同一实例化 ⇒ 同一类型；等价实参的不同写法也命中）。
//!
//! `subst.go`：替换只重建**受影响**的节点，其余子树**复用原条目**。
//! `named.go::expandMethod`：方法签名按**接收者类型参数**（位置对齐实参）
//! 替换（`makeSubstMap(rtpars, rtargs)`）。
//!
//! # 三档断言
//!
//! `Ok(实例)` / 稳定拒绝码（`TypWrongTypeArgCount` / `TypInvalidTypeArg`）。
//! 拒绝码的**分类投影**（`type-arg` / `implements`）由 `rgoc-check` 的
//! `T-CK-16` 断言，不在这里重复。

use rgoc_const::BasicId;
use rgoc_lex::DiagCode;
use rgoc_types::{
    Field, IfaceTSet, NamedId, SubstMap, Type, TypeId, TypeParamId, TypeTable, identical,
    instantiate, substitute,
};

// ── 构造助手 ────────────────────────────────────────────────────────

/// `interface{}`（空接口，`tset = All`）—— `any` 的表示。
fn any(tt: &mut TypeTable) -> TypeId {
    let i = tt.add_interface(Vec::new(), None, IfaceTSet::All);
    tt.interface_type(i)
}

/// `interface{ ~T }`：单 `~T` 项约束（语法项集 + 语义项集都记）。
fn tilde(tt: &mut TypeTable, t: TypeId) -> TypeId {
    let u = tt.add_union(vec![(true, t)]);
    let ut = tt.union_type(u);
    let i = tt.add_interface(Vec::new(), Some(ut), IfaceTSet::Terms(vec![(true, t)]));
    tt.interface_type(i)
}

/// `type Box[T any] struct { elem T }` ⇒ `(定义, 形参)`。
fn box_decl(tt: &mut TypeTable) -> (NamedId, TypeParamId) {
    let a = any(tt);
    let tp = tt.add_type_param(0, a);
    let t = tt.type_param_type(tp);
    let st = tt.add_struct(vec![Field {
        name: Some("elem".into()),
        typ: t,
        tag: None,
        embedded: false,
    }]);
    let st = tt.struct_type(st);
    let n = tt.add_named("Box");
    tt.set_named_underlying(n, st);
    tt.set_named_type_params(n, vec![tp]);
    (n, tp)
}

/// 裸具名（无类型参数）：`type Name int` 形态。
fn plain(tt: &mut TypeTable, name: &str, under: TypeId) -> NamedId {
    let n = tt.add_named(name);
    tt.set_named_underlying(n, under);
    n
}

// ── 实例身份（memo）───────────────────────────────────────────────

#[test]
fn t_ck_21_同实参两次实例化_同_id() {
    let mut tt = TypeTable::new();
    let (n, _) = box_decl(&mut tt);
    let int = tt.add_basic(BasicId::Int);

    let i1 = instantiate(&mut tt, n, &[int]).expect("实例化应成功");
    let len = tt.len();
    let i2 = instantiate(&mut tt, n, &[int]).expect("实例化应成功");

    assert_eq!(i1, i2, "同一 (origin, args) ⇒ 同一实例");
    assert_eq!(tt.len(), len, "memo 命中不该再长表");
}

#[test]
fn t_ck_21_等价实参不同写法_同_id() {
    let mut tt = TypeTable::new();
    let (n, _) = box_decl(&mut tt);
    let int = tt.add_basic(BasicId::Int);
    // `[]int` 写两次是**两个**表条目（结构型不去重，C7 §3）——
    // memo 必须按同一性比对，而不是按条目身份。
    let s1 = tt.add_slice(int);
    let s2 = tt.add_slice(int);
    assert_ne!(s1, s2, "前提：结构型不去重");

    let i1 = instantiate(&mut tt, n, &[s1]).expect("实例化应成功");
    let i2 = instantiate(&mut tt, n, &[s2]).expect("实例化应成功");
    assert_eq!(i1, i2, "等价（identical）实参 ⇒ 同一实例");
}

#[test]
fn t_ck_21_别名实参与本体实参同_id() {
    let mut tt = TypeTable::new();
    let (n, _) = box_decl(&mut tt);
    let int = tt.add_basic(BasicId::Int);
    // `type MyInt = int`：`Identical` 里别名透明 ⇒ 命中同一实例。
    let a = tt.add_alias("MyInt");
    tt.set_alias_rhs(a, int);
    let at = tt.alias_type(a);

    let i1 = instantiate(&mut tt, n, &[at]).expect("实例化应成功");
    let i2 = instantiate(&mut tt, n, &[int]).expect("实例化应成功");
    assert_eq!(i1, i2, "别名实参按穿透后的类型比对");
}

#[test]
fn t_ck_21_不同实参_不同_id() {
    let mut tt = TypeTable::new();
    let (n, _) = box_decl(&mut tt);
    let int = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);

    let i1 = instantiate(&mut tt, n, &[int]).expect("实例化应成功");
    let i2 = instantiate(&mut tt, n, &[string]).expect("实例化应成功");
    assert_ne!(i1, i2, "不同实参 ⇒ 不同实例");
}

// ── 替换 ───────────────────────────────────────────────────────────

#[test]
fn t_ck_21_实例本体按实参替换() {
    let mut tt = TypeTable::new();
    let (n, _) = box_decl(&mut tt);
    let int = tt.add_basic(BasicId::Int);

    let i = instantiate(&mut tt, n, &[int]).expect("实例化应成功");
    let Type::Named(inid) = *tt.type_of(i) else {
        panic!("实例应是 named");
    };
    let d = tt.named_def(inid);
    assert_eq!(d.origin, Some(n), "实例要记录来源");
    assert_eq!(d.targs, vec![int], "实例要记录实参");
    assert!(d.type_params.is_empty(), "实例不再是泛型");

    let Type::Struct(sid) = *tt.type_of(d.underlying.expect("实例本体已填")) else {
        panic!("Box 本体应是 struct");
    };
    assert_eq!(tt.struct_def(sid).fields[0].typ, int, "字段 T 应替换为 int");
    assert_ne!(i, tt.named_type(n), "实例 ≠ 原点");
}

#[test]
fn t_ck_21_替换_未命中子树复用() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let tp = tt.add_type_param(0, a);
    let t = tt.type_param_type(tp);
    let int = tt.add_basic(BasicId::Int);
    // `map[T]string`：键受替换影响，值不受。
    let string = tt.add_basic(BasicId::String);
    let m = tt.add_map(t, string);

    let mut subst = SubstMap::new();
    let int_slice = tt.add_slice(int);
    subst.insert(tp, int_slice);
    let out = substitute(&mut tt, m, &subst);
    let Type::Map { key, elem } = *tt.type_of(out) else {
        panic!("替换结果应是 map");
    };
    assert_eq!(key, int_slice, "键被替换");
    assert_eq!(elem, string, "未受影响的子树复用原条目");

    // 空映射 ⇒ 恒等。
    assert_eq!(substitute(&mut tt, m, &SubstMap::new()), m);
}

#[test]
fn t_ck_21_方法签名按位置替换() {
    let mut tt = TypeTable::new();
    let (n, _) = box_decl(&mut tt);
    let int = tt.add_basic(BasicId::Int);

    // `func (b Box[T]) Get() T`：接收者**自己声明** `T`（rtp），
    // 签名里引用的就是它（官方 `recv.TypeParams()`，与形参表不同条目）。
    let a = any(&mut tt);
    let rtp = tt.add_type_param(0, a);
    let rt = tt.type_param_type(rtp);
    let t0 = tt.add_tuple(Vec::new());
    let tr = tt.add_tuple(vec![(None, rt)]);
    let recv = {
        let bt = tt.named_type(n);
        tt.add_pointer(bt)
    };
    let sid = tt.add_signature(Some(recv), t0, tr, false);
    let sig = tt.signature_type(sid);
    tt.set_signature_recv_tparams(sid, vec![rtp]);
    tt.add_named_method(n, "Get", sig);

    let i = instantiate(&mut tt, n, &[int]).expect("实例化应成功");
    let Type::Named(inid) = *tt.type_of(i) else {
        panic!("实例应是 named");
    };
    let (name, msig) = &tt.named_def(inid).methods[0];
    assert_eq!(name.as_str(), "Get");
    let Type::Signature(msid) = *tt.type_of(*msig) else {
        panic!("方法应是签名");
    };
    let Type::Tuple(tid) = *tt.type_of(tt.signature_def(msid).results) else {
        panic!("结果应是元组");
    };
    assert_eq!(tt.tuple_def(tid).vars[0].1, int, "rtp 按位置替换为实参");

    // 原点的签名**没被动过**（替换产出新条目，而非就地改写）。
    let Type::Signature(osid) = *tt.type_of(sig) else {
        panic!()
    };
    let Type::Tuple(otid) = *tt.type_of(tt.signature_def(osid).results) else {
        panic!()
    };
    assert_eq!(tt.tuple_def(otid).vars[0].1, rt, "原点结果仍是 T");
}

// ── 稳定拒绝 ───────────────────────────────────────────────────────

#[test]
fn t_ck_21_数量错_稳定拒绝() {
    let mut tt = TypeTable::new();
    let (n, _) = box_decl(&mut tt);
    let int = tt.add_basic(BasicId::Int);

    assert_eq!(
        instantiate(&mut tt, n, &[]),
        Err(DiagCode::TypWrongTypeArgCount),
        "太少 ⇒ 稳定拒绝"
    );
    assert_eq!(
        instantiate(&mut tt, n, &[int, int]),
        Err(DiagCode::TypWrongTypeArgCount),
        "太多 ⇒ 稳定拒绝"
    );
    // 稳定：重复调用给同一个码。
    assert_eq!(
        instantiate(&mut tt, n, &[]),
        Err(DiagCode::TypWrongTypeArgCount)
    );
}

#[test]
fn t_ck_21_约束不满足_稳定拒绝() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);
    // `type Num[T ~int] struct { elem T }`
    let c = tilde(&mut tt, int);
    let tp = tt.add_type_param(0, c);
    let t = tt.type_param_type(tp);
    let st = tt.add_struct(vec![Field {
        name: Some("elem".into()),
        typ: t,
        tag: None,
        embedded: false,
    }]);
    let st = tt.struct_type(st);
    let n = tt.add_named("Num");
    tt.set_named_underlying(n, st);
    tt.set_named_type_params(n, vec![tp]);

    assert_eq!(
        instantiate(&mut tt, n, &[string]),
        Err(DiagCode::TypInvalidTypeArg),
        "`string does not satisfy ~int` ⇒ 稳定拒绝"
    );
    assert_eq!(
        instantiate(&mut tt, n, &[string]),
        Err(DiagCode::TypInvalidTypeArg)
    );
    assert!(instantiate(&mut tt, n, &[int]).is_ok(), "`int` 满足 `~int`");
}

#[test]
fn t_ck_21_非泛型_空实参恒等_非空拒绝() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let p = plain(&mut tt, "Plain", int);

    assert_eq!(
        instantiate(&mut tt, p, &[]),
        Ok(tt.named_type(p)),
        "非泛型 + 空实参 ⇒ 恒等（不造重复条目）"
    );
    assert_eq!(
        instantiate(&mut tt, p, &[int]),
        Err(DiagCode::TypWrongTypeArgCount)
    );
}

// ── 同一性接线 ─────────────────────────────────────────────────────

#[test]
fn t_ck_21_实例与原点不同一_同实参实例同一() {
    let mut tt = TypeTable::new();
    let (n, _) = box_decl(&mut tt);
    let int = tt.add_basic(BasicId::Int);
    let orig = tt.named_type(n);

    let i = instantiate(&mut tt, n, &[int]).expect("实例化应成功");
    let j = instantiate(&mut tt, n, &[int]).expect("实例化应成功");

    assert_eq!(identical(&tt, i, i), Ok(true));
    assert_eq!(identical(&tt, i, j), Ok(true), "同实参实例同一");
    assert_eq!(identical(&tt, i, orig), Ok(false), "实例 ≠ 原点");
    assert_eq!(identical(&tt, orig, i), Ok(false));
}
