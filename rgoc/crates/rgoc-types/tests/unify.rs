//! `T-CK-24` 底座：统一（`unify`）三模式与 `Unifier` 引擎（M4/Task 13）。
//!
//! # 判据来源
//!
//! 官方 `go/types/unify.go` 的 `(*unifier).nify`（含 `join` / 句柄机制）
//! 与 `under.go::commonUnder`（核心类型）。三个模式对应官方调用点的三个取值：
//!
//! * `Loose` ≡ 官方 `unifyMode(0)`（`infer.go:262`）——具名可对本体的**中间**
//!   形态、通道方向忽略、接口推断开；
//! * `Exact` ≡ 官方 `exact`（`infer.go:305`）——逐点同一性语义；
//! * `Assign` ≡ 官方 `assign`（`infer.go:180`）——顶层宽容、**元素层收紧为 exact**
//!   （`emode = mode | exact`）。
//!
//! # 三档断言
//!
//! 真 / 假 / 绑定表（`Unifier::at` / `inferred` / 自由函数 `infer` 回填）。
//! `unify`/`infer` **不设** cmp 形式（`D-M4-15`），所以这里全部是表级向量。
//!
//! # v1 边界（在模块文档里声明、测试不覆盖）
//!
//! * 递归深度护栏 = 官方 `unificationDepthLimit`（50）；官方越限 panic，
//!   本层**安静返回假**（发行库不 panic）。
//! * `union` 直达统一 / 泛型方法满足接口两处官方是断言不可达 / 明确拒绝 ——
//!   见实现处注释。

use rgoc_const::BasicId;
use rgoc_types::{
    ChanDir, Field, IfaceTSet, NamedId, TypeId, TypeParamId, TypeTable, Unifier,
    UnifyMode::{Assign, Exact, Loose},
    instantiate, unify,
};

// ── 构造助手 ────────────────────────────────────────────────────────

fn int(tt: &mut TypeTable) -> TypeId {
    tt.add_basic(BasicId::Int)
}

fn string(tt: &mut TypeTable) -> TypeId {
    tt.add_basic(BasicId::String)
}

/// `interface{}`（空接口，`tset = All`）—— `any` 的表示。
fn any(tt: &mut TypeTable) -> TypeId {
    let i = tt.add_interface(Vec::new(), None, IfaceTSet::All);
    tt.interface_type(i)
}

/// 单 `~T` 项接口。
fn tilde(tt: &mut TypeTable, t: TypeId) -> TypeId {
    let u = tt.add_union(vec![(true, t)]);
    let ut = tt.union_type(u);
    let i = tt.add_interface(Vec::new(), Some(ut), IfaceTSet::Terms(vec![(true, t)]));
    tt.interface_type(i)
}

/// 项表接口（`tset = Terms`；语法项集也建 `union`）。
fn terms_iface(tt: &mut TypeTable, terms: &[(bool, TypeId)]) -> TypeId {
    let u = tt.add_union(terms.to_vec());
    let ut = tt.union_type(u);
    let i = tt.add_interface(Vec::new(), Some(ut), IfaceTSet::Terms(terms.to_vec()));
    tt.interface_type(i)
}

/// 记录一个形参条目：`T(idx)`，约束为 `c`。
fn param(tt: &mut TypeTable, idx: u32, c: TypeId) -> TypeParamId {
    tt.add_type_param(idx, c)
}

/// `func(参数) 结果` 的签名条目。
fn sig(
    tt: &mut TypeTable,
    params: Option<TypeId>,
    results: Option<TypeId>,
    variadic: bool,
) -> TypeId {
    let p = tt.add_tuple(params.map(|t| vec![(None, t)]).unwrap_or_default());
    let r = tt.add_tuple(results.map(|t| vec![(None, t)]).unwrap_or_default());
    let s = tt.add_signature(None, p, r, variadic);
    tt.signature_type(s)
}

/// 带方法集与类型集的接口。
fn iface_of(tt: &mut TypeTable, methods: Vec<(&str, TypeId)>, tset: IfaceTSet) -> TypeId {
    let i = tt.add_interface(
        methods
            .into_iter()
            .map(|(n, s)| (n.to_string(), s))
            .collect(),
        None,
        tset,
    );
    tt.interface_type(i)
}

/// 裸具名：`type Name Under`。
fn plain(tt: &mut TypeTable, name: &str, under: TypeId) -> NamedId {
    let n = tt.add_named(name);
    tt.set_named_underlying(n, under);
    n
}

/// 空结构体的底层（当「随便一个本体」用）。
fn empty_struct(tt: &mut TypeTable) -> TypeId {
    let s = tt.add_struct(Vec::new());
    tt.struct_type(s)
}

/// `Pair[K, V]` 泛型定义（本体 `struct{}`）。
fn pair_decl(tt: &mut TypeTable) -> (NamedId, TypeParamId, TypeParamId) {
    let a = any(tt);
    let k = tt.add_type_param(0, a);
    let v = tt.add_type_param(1, a);
    let st = empty_struct(tt);
    let n = tt.add_named("Pair");
    tt.set_named_underlying(n, st);
    tt.set_named_type_params(n, vec![k, v]);
    (n, k, v)
}

fn field(name: Option<&str>, typ: TypeId, tag: Option<&str>, embedded: bool) -> Field {
    Field {
        name: name.map(str::to_string),
        typ,
        tag: tag.map(str::to_string),
        embedded,
    }
}

// ── 三模式差异 ──────────────────────────────────────────────────────

#[test]
fn t_ck_24_三模式_具名对本体() {
    let mut tt = TypeTable::new();
    let i = int(&mut tt);
    let my = plain(&mut tt, "MyInt", i);
    let myt = tt.named_type(my);
    // Loose / Assign：顶层剥本体 ⇒ 真；Exact：不下钻 ⇒ 假
    assert!(unify(&tt, Loose, myt, i, &mut vec![]).unwrap());
    assert!(unify(&tt, Assign, myt, i, &mut vec![]).unwrap());
    assert!(!unify(&tt, Exact, myt, i, &mut vec![]).unwrap());
    // 统一是对称的
    assert!(unify(&tt, Loose, i, myt, &mut vec![]).unwrap());
}

#[test]
fn t_ck_24_三模式_元素层_assign收紧() {
    let mut tt = TypeTable::new();
    let i = int(&mut tt);
    let my = plain(&mut tt, "MyInt", i);
    let myt = tt.named_type(my);
    let s_my = tt.add_slice(myt);
    let s_i = tt.add_slice(i);
    // 顶层都是 slice（无具名可剥）⇒ Loose 的元素也宽容、Assign/Exact 的元素 Exact
    assert!(unify(&tt, Loose, s_my, s_i, &mut vec![]).unwrap());
    assert!(!unify(&tt, Assign, s_my, s_i, &mut vec![]).unwrap());
    assert!(!unify(&tt, Exact, s_my, s_i, &mut vec![]).unwrap());
}

#[test]
fn t_ck_24_三模式_通道方向() {
    let mut tt = TypeTable::new();
    let i = int(&mut tt);
    let both = tt.add_chan(ChanDir::SendRecv, i);
    let send = tt.add_chan(ChanDir::SendOnly, i);
    // 方向只在 Exact 参与；Assign 的「收紧」只针对元素
    assert!(unify(&tt, Loose, both, send, &mut vec![]).unwrap());
    assert!(unify(&tt, Assign, both, send, &mut vec![]).unwrap());
    assert!(!unify(&tt, Exact, both, send, &mut vec![]).unwrap());
}

// ── 绑定与句柄 ──────────────────────────────────────────────────────

#[test]
fn t_ck_24_记录形参_直接绑定并回填() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let t = param(&mut tt, 0, a);
    let tp = tt.type_param_type(t);
    let i = int(&mut tt);
    let s = string(&mut tt);

    let mut infer = Vec::new();
    assert!(unify(&tt, Loose, tp, i, &mut infer).unwrap());
    assert_eq!(infer, vec![(t, i)]);

    // 失败 ⇒ 不回填（自由函数把两侧出现的形参全记录；这里没有形参）
    let mut infer = Vec::new();
    assert!(!unify(&tt, Loose, i, s, &mut infer).unwrap());
    assert!(infer.is_empty());
}

#[test]
fn t_ck_24_已绑定形参_须与前置相符() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let t = param(&mut tt, 0, a);
    let tp = tt.type_param_type(t);
    let i = int(&mut tt);
    let s = string(&mut tt);

    let mut u = Unifier::new(&tt);
    u.record([t]);
    assert!(u.unify(Loose, tp, i).unwrap());
    assert_eq!(u.at(t), Some(i));
    assert!(!u.unify(Loose, tp, s).unwrap());
    assert_eq!(u.at(t), Some(i), "失败的统一不改绑定");
}

#[test]
fn t_ck_24_记录与未记录_两条道() {
    let mut tt = TypeTable::new();
    let i = int(&mut tt);
    let myn = plain(&mut tt, "MyS", i);
    let mys = tt.named_type(myn);
    let s_mys = tt.add_slice(mys);
    let si = tt.add_slice(i);
    let c = tilde(&mut tt, si); // `~[]int`
    let p = param(&mut tt, 0, c);
    let pt = tt.type_param_type(p);

    // 已记录（= 推断变量）⇒ 直接绑定：`[]MyS` 也成立
    let mut u = Unifier::new(&tt);
    u.record([p]);
    assert!(u.unify(Loose, pt, s_mys).unwrap());
    assert_eq!(u.at(p), Some(s_mys));

    // 未记录 ⇒ 核心类型路：`[]int` 命中
    let mut u2 = Unifier::new(&tt);
    assert!(u2.unify(Loose, pt, si).unwrap());
    // 核心类型（`[]int`）与 `[]MyS`：元素层以 Assign ⇒ Exact ⇒ 假
    let mut u3 = Unifier::new(&tt);
    assert!(!u3.unify(Loose, pt, s_mys).unwrap());
}

#[test]
fn t_ck_24_核心类型_多项合并与无核心() {
    let mut tt = TypeTable::new();
    let i = int(&mut tt);
    let s = string(&mut tt);
    // `~[]int | ~MySlice`：两项底层同（`type MySlice []int`）⇒ 核心 `[]int`
    let si = tt.add_slice(i);
    let my_slice = plain(&mut tt, "MySlice", si);
    let my_slice_t = tt.named_type(my_slice);
    let c_merge = terms_iface(&mut tt, &[(true, si), (true, my_slice_t)]);
    let p1 = param(&mut tt, 0, c_merge);
    let p1t = tt.type_param_type(p1);
    let mut u = Unifier::new(&tt);
    assert!(u.unify(Loose, p1t, si).unwrap());

    // `~int | ~string`：无共同底层 ⇒ 无核心 ⇒ 假（哪怕 `int` 在集里）
    let c_disjoint = terms_iface(&mut tt, &[(true, i), (true, s)]);
    let p2 = param(&mut tt, 1, c_disjoint);
    let p2t = tt.type_param_type(p2);
    let mut u2 = Unifier::new(&tt);
    assert!(!u2.unify(Loose, p2t, i).unwrap());
}

#[test]
fn t_ck_24_两形参_join与unknowns() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let t1 = param(&mut tt, 0, a);
    let t2 = param(&mut tt, 1, a);
    let (t1p, t2p) = (tt.type_param_type(t1), tt.type_param_type(t2));
    let i = int(&mut tt);
    let s = string(&mut tt);

    let mut u = Unifier::new(&tt);
    u.record([t1, t2]);
    assert_eq!(u.unknowns(), 2);
    assert!(u.unify(Loose, t1p, t2p).unwrap());
    assert!(u.unify(Loose, t1p, i).unwrap());
    assert_eq!(u.at(t2), Some(i), "join 之后共享绑定");
    assert_eq!(u.unknowns(), 0);
    assert_eq!(u.inferred(&[t1, t2]), vec![Some(i), Some(i)]);

    // join 失败路：两者各有不同绑定 ⇒ 比较绑定值
    let mut u2 = Unifier::new(&tt);
    u2.record([t1, t2]);
    assert!(u2.unify(Loose, t1p, i).unwrap());
    assert!(u2.unify(Loose, t2p, s).unwrap());
    assert!(!u2.unify(Loose, t1p, t2p).unwrap());

    // preset：显式实参预置（官方 `newUnifier` 的 `targs` 初值）
    let mut u3 = Unifier::new(&tt);
    u3.preset(t1, i);
    assert_eq!(u3.at(t1), Some(i));
    assert!(u3.unify(Loose, t1p, i).unwrap());
    assert!(!u3.unify(Loose, t1p, s).unwrap());
}

// ── 接口推断 ────────────────────────────────────────────────────────

#[test]
fn t_ck_24_接口推断_两接口_子集与位() {
    let mut tt = TypeTable::new();
    let sig0 = sig(&mut tt, None, None, false);
    let i1 = iface_of(&mut tt, vec![("M", sig0)], IfaceTSet::All);
    let i2 = iface_of(&mut tt, vec![("M", sig0), ("N", sig0)], IfaceTSet::All);
    // Loose：小方法集是子集 ⇒ 真；Exact：方法数须相等 ⇒ 假
    assert!(unify(&tt, Loose, i1, i2, &mut vec![]).unwrap());
    assert!(!unify(&tt, Exact, i1, i2, &mut vec![]).unwrap());
    // 关掉接口推断 ⇒ Loose 也走结构路
    let mut u = Unifier::new(&tt);
    u.set_interface_inference(false);
    assert!(!u.unify(Loose, i1, i2).unwrap());
    // comparable 位不同 ⇒ 先挡（两种模式都假）
    let i3 = iface_of(&mut tt, vec![("M", sig0)], IfaceTSet::Comparable);
    assert!(!unify(&tt, Loose, i1, i3, &mut vec![]).unwrap());
    assert!(!unify(&tt, Exact, i1, i3, &mut vec![]).unwrap());
}

#[test]
fn t_ck_24_接口推断_一接口_方法集() {
    let mut tt = TypeTable::new();
    let sig0 = sig(&mut tt, None, None, false);
    let ii = iface_of(&mut tt, vec![("M", sig0)], IfaceTSet::All);
    let under = empty_struct(&mut tt);
    let n = plain(&mut tt, "N", under);
    tt.add_named_method(n, "M", sig0);
    let nt = tt.named_type(n);
    // 有 `M` ⇒ 真（两个方向）；Exact ⇒ 假（接口对具名：结构不同）
    assert!(unify(&tt, Loose, ii, nt, &mut vec![]).unwrap());
    assert!(unify(&tt, Loose, nt, ii, &mut vec![]).unwrap());
    assert!(!unify(&tt, Exact, ii, nt, &mut vec![]).unwrap());
    // 缺方法的具名 ⇒ 假
    let under2 = empty_struct(&mut tt);
    let n2n = plain(&mut tt, "N2", under2);
    let n2 = tt.named_type(n2n);
    assert!(!unify(&tt, Loose, ii, n2, &mut vec![]).unwrap());
}

#[test]
fn t_ck_24_接口_方法签名统一_绑定形参() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let t = param(&mut tt, 0, a);
    let tp = tt.type_param_type(t);
    let i = int(&mut tt);
    let sig_t = sig(&mut tt, Some(tp), None, false); // `func(T)`
    let sig_i = sig(&mut tt, Some(i), None, false); // `func(int)`
    let i1 = iface_of(&mut tt, vec![("M", sig_t)], IfaceTSet::All);
    let i2 = iface_of(&mut tt, vec![("M", sig_i)], IfaceTSet::All);
    let mut infer = Vec::new();
    assert!(unify(&tt, Loose, i1, i2, &mut infer).unwrap());
    assert_eq!(infer, vec![(t, i)], "方法签名在 Exact 下统一仍可绑定形参");
}

// ── 签名 / 结构递归 ────────────────────────────────────────────────

#[test]
fn t_ck_24_签名_维数变参与形参绑定() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let t = param(&mut tt, 0, a);
    let tp = tt.type_param_type(t);
    let i = int(&mut tt);
    let s = string(&mut tt);

    let sig_t = sig(&mut tt, Some(tp), None, false);
    let sig_i = sig(&mut tt, Some(i), None, false);
    let mut infer = Vec::new();
    assert!(unify(&tt, Loose, sig_t, sig_i, &mut infer).unwrap());
    assert_eq!(infer, vec![(t, i)]);

    // 变参不符 ⇒ 假
    let sig_vi = sig(&mut tt, Some(i), None, true);
    assert!(!unify(&tt, Loose, sig_vi, sig_i, &mut vec![]).unwrap());

    // 形数不符 ⇒ 假（结构化：参数元组长度不同）
    let sig_2 = {
        let p = tt.add_tuple(vec![(None, i), (None, s)]);
        let r = tt.add_tuple(Vec::new());
        let x = tt.add_signature(None, p, r, false);
        tt.signature_type(x)
    };
    assert!(!unify(&tt, Loose, sig_2, sig_i, &mut vec![]).unwrap());
}

#[test]
fn t_ck_24_映射与元组_结构递归() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let t = param(&mut tt, 0, a);
    let tp = tt.type_param_type(t);
    let i = int(&mut tt);
    let s = string(&mut tt);

    // map[int]T ≡ map[int]string ⇒ T→string
    let m_t = tt.add_map(i, tp);
    let m_s = tt.add_map(i, s);
    let mut infer = Vec::new();
    assert!(unify(&tt, Loose, m_t, m_s, &mut infer).unwrap());
    assert_eq!(infer, vec![(t, s)]);

    // 元组名字不参与：`func(a T)` ≡ `func(b int)`
    let p1 = tt.add_tuple(vec![(Some("a".to_string()), tp)]);
    let r1 = tt.add_tuple(Vec::new());
    let sig1 = tt.add_signature(None, p1, r1, false);
    let x1 = tt.signature_type(sig1);
    let p2 = tt.add_tuple(vec![(Some("b".to_string()), i)]);
    let r2 = tt.add_tuple(Vec::new());
    let sig2 = tt.add_signature(None, p2, r2, false);
    let x2 = tt.signature_type(sig2);
    let mut infer = Vec::new();
    assert!(unify(&tt, Loose, x1, x2, &mut infer).unwrap());
    assert_eq!(infer, vec![(t, i)]);
}

#[test]
fn t_ck_24_结构体_字段三判() {
    let mut tt = TypeTable::new();
    let i = int(&mut tt);
    let st = |tt: &mut TypeTable, fields: Vec<Field>| {
        let s = tt.add_struct(fields);
        tt.struct_type(s)
    };
    let same_a = st(&mut tt, vec![field(Some("a"), i, None, false)]);
    let same_b = st(&mut tt, vec![field(Some("a"), i, None, false)]);
    assert!(unify(&tt, Loose, same_a, same_b, &mut vec![]).unwrap());
    assert!(unify(&tt, Exact, same_a, same_b, &mut vec![]).unwrap());

    // tag 不同 ⇒ 假（统一里 tag **恒**参与，与 `identical` 的严格面一致）
    let tag_x = st(&mut tt, vec![field(Some("a"), i, Some("x"), false)]);
    let tag_y = st(&mut tt, vec![field(Some("a"), i, Some("y"), false)]);
    assert!(!unify(&tt, Loose, tag_x, tag_y, &mut vec![]).unwrap());

    // 名字不同 ⇒ 假
    let name_b = st(&mut tt, vec![field(Some("b"), i, None, false)]);
    assert!(!unify(&tt, Loose, same_a, name_b, &mut vec![]).unwrap());

    // 嵌入标记不同 ⇒ 假
    let emb = st(&mut tt, vec![field(None, i, None, true)]);
    assert!(!unify(&tt, Loose, same_a, emb, &mut vec![]).unwrap());
}

#[test]
fn t_ck_24_invalid与别名_宽容面() {
    let mut tt = TypeTable::new();
    let inv1 = tt.invalid();
    let inv2 = tt.invalid();
    let i = int(&mut tt);
    assert!(unify(&tt, Loose, inv1, inv2, &mut vec![]).unwrap());
    assert!(!unify(&tt, Loose, inv1, i, &mut vec![]).unwrap());

    // 别名在统一里透明（首个 `Unalias` 快径）
    let al = tt.add_alias("A");
    tt.set_alias_rhs(al, i);
    let alt = tt.alias_type(al);
    assert!(unify(&tt, Exact, alt, i, &mut vec![]).unwrap());
}

// ── 实例实参 ────────────────────────────────────────────────────────

#[test]
fn t_ck_24_嵌套实例_实参统一() {
    let mut tt = TypeTable::new();
    let (pair, k, _v) = pair_decl(&mut tt);
    let kt = tt.type_param_type(k);
    let i = int(&mut tt);
    let s = string(&mut tt);
    let inst_a = instantiate(&mut tt, pair, &[kt, s]).unwrap();
    let inst_b = instantiate(&mut tt, pair, &[i, s]).unwrap();
    let mut infer = Vec::new();
    assert!(unify(&tt, Loose, inst_a, inst_b, &mut infer).unwrap());
    assert_eq!(infer, vec![(k, i)]);

    // 同形但不同原点 ⇒ 假
    let (pair2, _k2, _v2) = pair_decl(&mut tt);
    let inst_c = instantiate(&mut tt, pair2, &[i, s]).unwrap();
    assert!(!unify(&tt, Loose, inst_b, inst_c, &mut vec![]).unwrap());
}
