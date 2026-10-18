//! 方法挂载：真源码 → 表（M4/Task 7 的**生产者**面）。
//!
//! `methodset` 的判定面在 `rgoc-types/tests/methodset.rs`（手工建表）；
//! 本文件管「`rgoc-load` 把 `func (r T) M()` 挂到 `named T` 上」这条路：
//! 值 / 指针接收者、空名、跨文件、嵌入、泛型接收者、**别名接收者**
//! （`type A = T; func (a A) M()` —— 合法 Go 且属于 T，探针实测）、
//! 非本地接收者（畸形，不 panic、不挂）。
//!
//! 期望值来源：官方 `go/types` 容器探针（记录见
//! `.superpowers/sdd/M4-plan/progress.md` 的 Task 7 行），oracle 对撞
//! （`types --cmp methodset`）由 Task 15 接线。

use rgoc_lex::Diag;
use rgoc_load::{Package, load_sources};
use rgoc_types::{BasicId, NamedId, Type, TypeId, TypeTable, instantiate, unalias};

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

/// 挂在 `named` 上的方法名（声明序）。
fn methods_of(tt: &TypeTable, nid: NamedId) -> Vec<String> {
    tt.named_def(nid)
        .methods
        .iter()
        .map(|(n, _)| n.clone())
        .collect()
}

/// 方法集的名字序列（`rgoc-types::methodset` 的端到端面）。
fn ms_names(tt: &TypeTable, t: TypeId) -> Vec<String> {
    rgoc_types::methodset(tt, t)
        .expect("methodset 不该拒绝")
        .into_iter()
        .map(|e| e.name)
        .collect()
}

#[test]
fn t_ck_14_加载挂载_值与指针接收者_空名跳过() {
    let (mut tt, pkg, diags) = load(&[(
        "a.go",
        r#"package p

type T struct{}

func (t T) M() {}
func (t *T) P() {}
func (t T) _() {}
"#,
    )]);
    assert!(diags.is_empty(), "不该有诊断：{diags:?}");
    let (nid, t_ty) = named_of(&tt, &pkg, "T");
    // 声明序；空名 `_` 不挂
    assert_eq!(methods_of(&tt, nid), vec!["M", "P"]);
    // 方法名**不进**包级作用域（M3 口径），函数名进
    assert!(pkg.lookup("M").is_none());
    // 端到端：T = {M}、*T = {M, P}
    assert_eq!(ms_names(&tt, t_ty), vec!["M"]);
    let t_ptr = tt.add_pointer(t_ty);
    assert_eq!(ms_names(&tt, t_ptr), vec!["M", "P"]);
}

#[test]
fn t_ck_14_加载挂载_跨文件与嵌入() {
    let (tt, pkg, diags) = load(&[
        ("a.go", "package p\n\ntype A struct{}\n\nfunc (A) AM() {}\n"),
        (
            "b.go",
            "package p\n\ntype B struct{ A }\n\nfunc (b *B) BM() {}\n",
        ),
    ]);
    assert!(diags.is_empty(), "不该有诊断：{diags:?}");
    let (a_nid, a_ty) = named_of(&tt, &pkg, "A");
    let (b_nid, b_ty) = named_of(&tt, &pkg, "B");
    assert_eq!(methods_of(&tt, a_nid), vec!["AM"]);
    assert_eq!(methods_of(&tt, b_nid), vec!["BM"]);
    // 嵌入提升端到端：B = {AM}（值集）；*B = {AM, BM}（指针集）
    assert_eq!(ms_names(&tt, b_ty), vec!["AM"]);
    assert_eq!(ms_names(&tt, a_ty), vec!["AM"]);
}

#[test]
fn t_ck_14_加载挂载_泛型接收者() {
    let (tt, pkg, diags) = load(&[(
        "a.go",
        r#"package p

type Box[T any] struct{ v T }

func (b Box[T]) V() T { return b.v }
func (b *Box[T]) W() {}
"#,
    )]);
    assert!(diags.is_empty(), "不该有诊断：{diags:?}");
    // M3 的表示：`Box[T]` 解析成的就是 `named Box`（探针实测 dump `signature #0 …`）
    let (nid, _) = named_of(&tt, &pkg, "Box");
    assert_eq!(methods_of(&tt, nid), vec!["V", "W"]);
}

#[test]
fn t_ck_21_加载挂载_形参表与接收者条目回写() {
    let (mut tt, pkg, diags) = load(&[(
        "a.go",
        r#"package p

type Pair[K comparable, V any] struct {
	k K
	v V
}

func (p Pair[K, V]) Get() V { return p.v }
"#,
    )]);
    assert!(diags.is_empty(), "不该有诊断：{diags:?}");
    let (nid, _) = named_of(&tt, &pkg, "Pair");

    // ① 定义拿到形参表（与声明同序：K, V）。
    let tps = tt.named_def(nid).type_params.clone();
    assert_eq!(tps.len(), 2, "形参表挂到定义上");
    assert_eq!(tt.type_param_def(tps[0]).index, 0);
    assert_eq!(tt.type_param_def(tps[1]).index, 1);

    // ② 方法签名拿到**接收者**类型参数 —— 与形参表**不同批**条目（官方两套）。
    let (_, sig) = tt.named_def(nid).methods[0].clone();
    let Type::Signature(sid) = *tt.type_of(sig) else {
        panic!("方法应是签名");
    };
    let rtps = tt.signature_def(sid).recv_tparams.clone();
    assert_eq!(rtps.len(), 2, "接收者类型参数挂到签名上");
    for rt in &rtps {
        assert!(!tps.contains(rt), "接收者条目与形参表不是同一批");
    }

    // ③ 端到端：实例化 ⇒ 本体 K/V 落位 + 方法结果按**位置**替换（V → string）。
    let int = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);
    let inst = instantiate(&mut tt, nid, &[int, string]).expect("实例化应成功");
    let Type::Named(inid) = *tt.type_of(inst) else {
        panic!("实例应是 named");
    };
    let d = tt.named_def(inid);
    assert_eq!(d.targs, vec![int, string]);
    let Type::Struct(bsid) = *tt.type_of(d.underlying.expect("实例本体已填")) else {
        panic!("Pair 本体应是 struct");
    };
    assert_eq!(tt.struct_def(bsid).fields[0].typ, int, "K → int");
    assert_eq!(tt.struct_def(bsid).fields[1].typ, string, "V → string");
    let (_, msig) = tt.named_def(inid).methods[0].clone();
    let Type::Signature(msid) = *tt.type_of(msig) else {
        panic!("方法应是签名");
    };
    let Type::Tuple(tid) = *tt.type_of(tt.signature_def(msid).results) else {
        panic!("结果应是元组");
    };
    assert_eq!(tt.tuple_def(tid).vars[0].1, string, "结果 V → string");
}

#[test]
fn t_ck_14_加载挂载_别名接收者透明() {
    // `type A = T; func (a A) M()`：合法 Go，M 属于 T（官方探针实测）
    let (tt, pkg, diags) = load(&[(
        "a.go",
        r#"package p

type T struct{}
type A = T

func (a A) M() {}
"#,
    )]);
    assert!(diags.is_empty(), "不该有诊断：{diags:?}");
    let (nid, _) = named_of(&tt, &pkg, "T");
    assert_eq!(methods_of(&tt, nid), vec!["M"]);
}

#[test]
fn t_ck_14_加载挂载_非本地接收者不挂不慌() {
    // `func (x int) M()` 是检查器要报的错（非法接收者），加载层**不 panic**、
    // 不挂到任何具名类型上；正常方法不受影响。
    let (tt, pkg, _diags) = load(&[(
        "a.go",
        r#"package p

type S struct{}
func (s S) M() {}

func (x int) Ghost() {}
"#,
    )]);
    let (nid, _) = named_of(&tt, &pkg, "S");
    assert_eq!(methods_of(&tt, nid), vec!["M"]);
}
