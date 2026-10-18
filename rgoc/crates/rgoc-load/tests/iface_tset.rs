//! 接口的**语法拍平**（C7 / oracle `synthUnion` 口径）与**语义类型集**
//! （官方 `computeInterfaceTypeSet` 口径）（M4/Task 8 的两条腿）。
//!
//! # 为什么要分两面
//!
//! oracle 的 dump 是**语法拍平**（`tools/types-oracle/main.go::synthUnion`：
//! 单元素是 `Union` 才 splice，否则渲染 **as-written 的 exact 项**），
//! 而判定（`implements` / `satisfies` / 后续的实例化）吃的是**语义类型集**
//! （交集 / `comparable` 过滤 / ∅ 可达 —— 与语法面貌**不必**同形）。
//! 两件事都放在 `InterfaceDef` 上：`terms`（语法，dump 渲染）+
//! `tset`（语义，判定消费）。
//!
//! # 期望值从哪来
//!
//! 官方源码（`typeset.go::computeInterfaceTypeSet / intersectTermLists`、
//! `union.go::computeUnionTypeSet`）逐条读出；语法面由容器探针与 oracle
//! dump **逐字节对撞**实测（`tsprobe*.go`，记录见
//! `.superpowers/sdd/M4-plan/progress.md` 的 Task 8 行）。

use rgoc_const::BasicId;
use rgoc_lex::Diag;
use rgoc_load::{Package, load_sources};
use rgoc_types::typeset::IfaceTSet;
use rgoc_types::{InterfaceId, NamedId, Type, TypeId, TypeTable, identical, unalias};

fn load(files: &[(&str, &str)]) -> (TypeTable, Package, Vec<Diag>) {
    let srcs: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(n, s)| ((*n).to_string(), s.as_bytes().to_vec()))
        .collect();
    let borrowed: Vec<(&str, &[u8])> = srcs
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_sources(&mut tt, &borrowed, None);
    (tt, pkg, bag.into_sorted())
}

/// 包级具名类型 → `(NamedId, 类型条目)`。
fn named_of(tt: &TypeTable, pkg: &Package, name: &str) -> (NamedId, TypeId) {
    let o = pkg
        .lookup(name)
        .unwrap_or_else(|| panic!("包级找不到 {name}"));
    let t = unalias(tt, o.typ.expect("对象没类型"));
    let Type::Named(n) = tt.type_of(t) else {
        panic!("{name} 不是具名类型：{:?}", tt.type_of(t));
    };
    (*n, t)
}

/// 包级类型的**as-written** 条目（不 `unalias` —— 别名要留住别名本身）。
fn as_written(pkg: &Package, name: &str) -> TypeId {
    pkg.lookup(name)
        .unwrap_or_else(|| panic!("包级找不到 {name}"))
        .typ
        .expect("对象没类型")
}

/// 具名类型的本体（下钻到 `InterfaceId`）。
fn iface_of(tt: &TypeTable, pkg: &Package, name: &str) -> InterfaceId {
    let (nid, _) = named_of(tt, pkg, name);
    let under = tt.named_underlying(nid);
    match tt.type_of(under) {
        Type::Interface(i) => *i,
        other => panic!("{name} 的本体不是接口：{other:?}"),
    }
}

/// 语义类型集。
fn tset_of(tt: &TypeTable, pkg: &Package, name: &str) -> IfaceTSet {
    tt.interface_def(iface_of(tt, pkg, name)).tset.clone()
}

/// 语法项的 `(tilde, TypeId)` 列表（`None` ⇒ 无项线）。
fn terms_of(tt: &TypeTable, pkg: &Package, name: &str) -> Option<Vec<(bool, TypeId)>> {
    let id = iface_of(tt, pkg, name);
    let terms = tt.interface_def(id).terms?;
    let Type::Union(u) = tt.type_of(terms) else {
        panic!("{name} 的项集不是 union");
    };
    Some(tt.union_def(*u).terms.clone())
}

/// 接口的方法名序列（构造期已排序）。
fn methods_of(tt: &TypeTable, pkg: &Package, name: &str) -> Vec<String> {
    tt.interface_def(iface_of(tt, pkg, name))
        .methods
        .iter()
        .map(|(n, _)| n.clone())
        .collect()
}

/// 拿某个方法项（签名 TypeId）。
fn method_sig(tt: &TypeTable, pkg: &Package, iface_name: &str, m: &str) -> TypeId {
    tt.interface_def(iface_of(tt, pkg, iface_name))
        .methods
        .iter()
        .find(|(n, _)| n == m)
        .unwrap_or_else(|| panic!("{iface_name} 没有方法 {m}"))
        .1
}

/// 语法项断言：`identical` 逐项比（基本型条目**不去重** ⇒ id 不可直接比）。
fn assert_terms(
    tt: &TypeTable,
    got: Option<&Vec<(bool, TypeId)>>,
    want: &[(bool, TypeId)],
    label: &str,
) {
    let got = got.unwrap_or_else(|| panic!("{label}: 期望有项线，得 None"));
    assert_eq!(got.len(), want.len(), "{label}: 项数");
    for ((gt, gt_t), (wt, wt_t)) in got.iter().zip(want) {
        assert_eq!(gt, wt, "{label}: tilde 位");
        assert_eq!(
            identical(tt, *gt_t, *wt_t),
            Ok(true),
            "{label}: 项 {gt_t:?} 应与期望同一"
        );
    }
}

/// 语义项集断言。
fn assert_tset_terms(tt: &TypeTable, got: &IfaceTSet, want: &[(bool, TypeId)], label: &str) {
    match got {
        IfaceTSet::Terms(ts) => {
            assert_eq!(ts.len(), want.len(), "{label}: 项数");
            for ((gt, gt_t), (wt, wt_t)) in ts.iter().zip(want) {
                assert_eq!(gt, wt, "{label}: tilde 位");
                assert_eq!(
                    identical(tt, *gt_t, *wt_t),
                    Ok(true),
                    "{label}: 项 {gt_t:?} 应与期望同一"
                );
            }
        }
        other => panic!("{label}: 期望 Terms，得 {other:?}"),
    }
}

#[test]
fn 嵌入具名接口_exact项_方法拍平() {
    let (tt, pkg, diags) = load(&[(
        "a.go",
        r#"package p

type J interface{ M() }
type D interface{ J }
"#,
    )]);
    assert!(diags.is_empty(), "不该有诊断：{diags:?}");

    // 语义：D 与 J 同集（无项 ⇒ 𝓤）
    assert_eq!(tset_of(&tt, &pkg, "J"), IfaceTSet::All);
    assert_eq!(tset_of(&tt, &pkg, "D"), IfaceTSet::All);

    // 方法拍平进 D
    assert_eq!(methods_of(&tt, &pkg, "D"), vec!["M"]);

    // 语法：**exact 项指向 as-written 的具名 J**（不是 splice）
    let (_, j_ty) = named_of(&tt, &pkg, "J");
    assert_terms(
        &tt,
        terms_of(&tt, &pkg, "D").as_ref(),
        &[(false, j_ty)],
        "D",
    );
}

#[test]
fn 嵌入具名约束_语义取实项_语法exact项() {
    let (mut tt, pkg, diags) = load(&[(
        "a.go",
        r#"package p

type C interface{ ~int | ~string }
type E interface{ C }
"#,
    )]);
    assert!(diags.is_empty(), "不该有诊断：{diags:?}");

    let int_ = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);

    // C 与 E 的语义集相同（E = 𝓤 ∩ C.tset —— 两侧都是 loader 内部条目，直接相等）
    assert_eq!(tset_of(&tt, &pkg, "E"), tset_of(&tt, &pkg, "C"));
    // 且就是「~int 与 ~string 两条」（基本型条目**不去重** ⇒ 用 identical 比）
    assert_tset_terms(
        &tt,
        &tset_of(&tt, &pkg, "C"),
        &[(true, int_), (true, string)],
        "C",
    );

    // 语法：E 的项是 **exact 项指向具名 C**（oracle 探针实测；不是 splice）
    let (_, c_ty) = named_of(&tt, &pkg, "C");
    assert_terms(
        &tt,
        terms_of(&tt, &pkg, "E").as_ref(),
        &[(false, c_ty)],
        "E",
    );
    // 而 C 自己的语法项是拼接的两条
    assert_terms(
        &tt,
        terms_of(&tt, &pkg, "C").as_ref(),
        &[(true, int_), (true, string)],
        "C",
    );
}

#[test]
fn 嵌入comparable_exact项_语义_comparable() {
    let (tt, pkg, diags) = load(&[(
        "a.go",
        r#"package p

type F interface{ comparable }
"#,
    )]);
    assert!(diags.is_empty(), "不该有诊断：{diags:?}");

    assert_eq!(tset_of(&tt, &pkg, "F"), IfaceTSet::Comparable);
    assert_eq!(methods_of(&tt, &pkg, "F"), Vec::<String>::new());

    // 语法：exact 项指向预声明的具名 `comparable`
    let terms = terms_of(&tt, &pkg, "F").expect("有项线");
    assert_eq!(terms.len(), 1);
    assert!(!terms[0].0, "exact 项");
    let Type::Named(nid) = tt.type_of(terms[0].1) else {
        panic!("项不是具名：{:?}", tt.type_of(terms[0].1));
    };
    assert_eq!(tt.named_def(*nid).name, "comparable");
}

#[test]
fn 嵌入别名_语法给别名条目_语义穿透() {
    let (tt, pkg, diags) = load(&[(
        "a.go",
        r#"package p

type C interface{ ~int }
type A = C
type E interface{ A }
"#,
    )]);
    assert!(diags.is_empty(), "不该有诊断：{diags:?}");

    // 语义：穿透别名到 C 的 tset（E = 𝓤 ∩ C.tset）
    assert_eq!(tset_of(&tt, &pkg, "E"), tset_of(&tt, &pkg, "C"));

    // 语法：exact 项 as-written = **别名条目本身**（oracle 实测：
    // alias 给 alias 条目，而不是它的目标）
    let a_ty = as_written(&pkg, "A");
    let got = terms_of(&tt, &pkg, "E").expect("有项线");
    assert_eq!(got.len(), 1);
    assert!(!got[0].0, "exact 项");
    assert!(
        matches!(tt.type_of(got[0].1), Type::Alias(_)),
        "项应是别名条目：{:?}",
        tt.type_of(got[0].1)
    );
    assert_eq!(got[0].1, a_ty, "项应指向 as-written 的别名条目");
}

#[test]
fn 交叠嵌入_语义积出交集_语法保持拼接() {
    let (mut tt, pkg, diags) = load(&[(
        "a.go",
        r#"package p

type A interface{ ~int; ~string }
type B interface{ comparable; ~[]byte }
type C interface{ ~int | ~string; ~string | ~float64 }
"#,
    )]);
    assert!(diags.is_empty(), "不该有诊断：{diags:?}");

    let int_ = tt.add_basic(BasicId::Int);
    let uint8_ = tt.add_basic(BasicId::Uint8);
    let string = tt.add_basic(BasicId::String);
    let float64_ = tt.add_basic(BasicId::Float64);
    let slice_byte = tt.add_slice(uint8_);

    // A：~int ∩ ~string = ∅；语法两条都在
    assert_eq!(tset_of(&tt, &pkg, "A"), IfaceTSet::Empty);
    assert_terms(
        &tt,
        terms_of(&tt, &pkg, "A").as_ref(),
        &[(true, int_), (true, string)],
        "A",
    );

    // B：comparable ∩ ~[]byte = ∅（[]byte 不严格可比）；语法 [comparable, ~[]byte]
    assert_eq!(tset_of(&tt, &pkg, "B"), IfaceTSet::Empty);
    let b_terms = terms_of(&tt, &pkg, "B").expect("B 有项线");
    assert_eq!(b_terms.len(), 2, "B: 项数");
    assert!(!b_terms[0].0, "B: 第一条是 exact 的 comparable");
    let Type::Named(nid) = tt.type_of(b_terms[0].1) else {
        panic!("B: 第一条不是具名：{:?}", tt.type_of(b_terms[0].1));
    };
    assert_eq!(
        tt.named_def(*nid).name,
        "comparable",
        "B: 第一条应为 comparable"
    );
    assert!(b_terms[1].0, "B: 第二条是 ~ 项");
    assert_eq!(
        identical(&tt, b_terms[1].1, slice_byte),
        Ok(true),
        "B: 第二条应为 ~[]byte"
    );

    // C：(~int|~string) ∩ (~string|~float64) = ~string（官方探针实测的形态）
    assert_tset_terms(&tt, &tset_of(&tt, &pkg, "C"), &[(true, string)], "C");
    assert_terms(
        &tt,
        terms_of(&tt, &pkg, "C").as_ref(),
        &[
            (true, int_),
            (true, string),
            (true, string),
            (true, float64_),
        ],
        "C",
    );
}

#[test]
fn 方法显式先行_同名去重首见() {
    let (tt, pkg, diags) = load(&[(
        "a.go",
        r#"package p

type J interface{ M() }
type D interface {
	J
	M()
}
"#,
    )]);
    assert!(diags.is_empty(), "不该有诊断：{diags:?}");

    // 去重：只剩一条 M
    assert_eq!(methods_of(&tt, &pkg, "D"), vec!["M"]);
    // 首见 = **显式**方法（官方先收 `ityp.methods` 再收嵌入）——
    // 若按源码序（J 在前），留下来的会是 J 的那条签名。
    let j_m = method_sig(&tt, &pkg, "J", "M");
    let d_m = method_sig(&tt, &pkg, "D", "M");
    assert_ne!(d_m, j_m, "D.M 应取显式声明的签名条目");
}

#[test]
fn 多嵌入_同名取先见_声明序保持() {
    let (tt, pkg, diags) = load(&[(
        "a.go",
        r#"package p

type J interface{ M() }
type D2 interface {
	M()
	N()
}
type G interface {
	J
	D2
}
"#,
    )]);
    assert!(diags.is_empty(), "不该有诊断：{diags:?}");

    // 去重：M 取 J 的（先见），N 取 D2 的
    assert_eq!(methods_of(&tt, &pkg, "G"), vec!["M", "N"]);
    assert_eq!(
        method_sig(&tt, &pkg, "G", "M"),
        method_sig(&tt, &pkg, "J", "M")
    );
    assert_eq!(
        method_sig(&tt, &pkg, "G", "N"),
        method_sig(&tt, &pkg, "D2", "N")
    );

    // 语法项按嵌入声明序：J 在前、D2 在后
    let (_, j_ty) = named_of(&tt, &pkg, "J");
    let (_, d2_ty) = named_of(&tt, &pkg, "D2");
    assert_terms(
        &tt,
        terms_of(&tt, &pkg, "G").as_ref(),
        &[(false, j_ty), (false, d2_ty)],
        "G",
    );
}

#[test]
fn 自引用接口_不慌_退化项() {
    let (tt, pkg, _diags) = load(&[(
        "a.go",
        r#"package p

type S interface{ S }
"#,
    )]);

    // 非法码面：不 panic；语义集退化为 𝓤（官方 partial 的 topTypeSet 同款口径），
    // 语法项 = exact 项指向自己（官方 dump 面未对撞过该形态）。
    assert_eq!(tset_of(&tt, &pkg, "S"), IfaceTSet::All);
    let (_, s_ty) = named_of(&tt, &pkg, "S");
    assert_terms(
        &tt,
        terms_of(&tt, &pkg, "S").as_ref(),
        &[(false, s_ty)],
        "S",
    );
}

#[test]
fn 波浪具名_语义漂移空_语法留下() {
    let (tt, pkg, _diags) = load(&[(
        "a.go",
        r#"package p

type MyInt int
type W interface{ ~MyInt }
"#,
    )]);

    // `~MyInt` 非法码（官方在检查器报 "invalid use of ~"），但类型集面
    // 官方 `computeUnionTypeSet` 会把该项漂移成 ∅ ⇒ 集合为空；语法项还在。
    assert_eq!(tset_of(&tt, &pkg, "W"), IfaceTSet::Empty);
    let (_, my_int) = named_of(&tt, &pkg, "MyInt");
    assert_terms(
        &tt,
        terms_of(&tt, &pkg, "W").as_ref(),
        &[(true, my_int)],
        "W",
    );
}
