//! 语义**类型集**：官方 `_TypeSet` 的四态投影（M4/Task 8）。
//!
//! # 为什么不是逐字段照抄 `_TypeSet`
//!
//! 官方 `_TypeSet = { methods, terms (termlist), comparable }`，不变量
//! `!comparable || terms.isAll()`。方法集另有承载（`InterfaceDef::methods`），
//! 所以这里只投影**项面 + `comparable` 位**，压成四态：
//!
//! | 本层 | 官方对应 | 语义 |
//! |---|---|---|
//! | [`IfaceTSet::All`] | `terms.isAll() && !comparable` | 𝓤（全部类型） |
//! | [`IfaceTSet::Comparable`] | `terms.isAll() && comparable` | 全部**可比较**类型 |
//! | [`IfaceTSet::Empty`] | `terms.isEmpty()` | ∅（空集） |
//! | [`IfaceTSet::Terms`] | 有限项表 | 各项类型集的**并集** |
//!
//! `Terms` **不作归一**（官方 `norm()` 会把 `int | ~int` 合并成 `~int`、
//! 去掉重复项）：消费面（[`subset_of`] / [`includes`] / [`is_comparable`]）
//! 一律按**集合语义**判，不看列表形状；列表形状只出现在官方内部表示里，
//! 不进任何对撞面。记录为已知偏离（台账 Task 8 行）。
//!
//! # 判据来源
//!
//! `go/types/typeset.go`：`_TypeSet.IsEmpty/IsAll/IsComparable/hasTerms/
//! subsetOf`、`intersectTermLists`（含 `comparable` 过滤与 comp 不变量）；
//! `go/types/termlist.go`：`isEmpty/isAll/intersect/includes/subsetOf/supersetOf`；
//! `go/types/typeterm.go`：`term.intersect/disjoint/subsetOf/includes`。

use rgoc_lex::diag::DiagCode;

use crate::comparable::comparable_type;
use crate::identical::{identical, unalias, underlying};
use crate::table::TypeTable;
use crate::typ::TypeId;

/// 语义类型集（四态，见模块文档）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum IfaceTSet {
    /// 𝓤：全部类型。
    All,
    /// 全部**可比较**类型（`comparable` 位；不变量下项面必为 𝓤）。
    Comparable,
    /// ∅。
    Empty,
    /// 有限项表的并集：`(tilde, 类型)`，`true` ⇒ `~T`。
    Terms(Vec<(bool, TypeId)>),
}

impl IfaceTSet {
    /// 官方 `IsAll` 的项面部分（不含方法集，方法集在 `InterfaceDef` 上判）。
    #[must_use]
    pub fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    /// 官方 `IsEmpty`。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    /// 官方 `hasTerms()`：`!isEmpty && !isAll` —— 只有有限项表为真
    /// （`Comparable` 的项面是 𝓤，故**不**算 hasTerms）。
    #[must_use]
    pub fn has_terms(&self) -> bool {
        matches!(self, Self::Terms(_))
    }
}

/// 官方 `intersectTermLists`（`typeset.go:328`）。
///
/// `Comparable ∩ 有限项` 按**严格**可比过滤（`comparableType(t, false, nil)`）；
/// 结果非 𝓤 ⇒ comp 位归假（即：结果要么保持 `Comparable` 不变量下的
/// 「𝓤 + 位」，要么落成有限项表 / ∅）。
///
/// # Errors
///
/// 过滤器与项间 `identical` 透传能力边界码。
pub fn intersect(tt: &TypeTable, x: &IfaceTSet, y: &IfaceTSet) -> Result<IfaceTSet, DiagCode> {
    use IfaceTSet::{All, Comparable, Empty, Terms};
    match (x, y) {
        (Empty, _) | (_, Empty) => Ok(Empty),
        (All, other) | (other, All) => Ok(other.clone()),
        (Comparable, Comparable) => Ok(Comparable),
        (Comparable, Terms(ts)) | (Terms(ts), Comparable) => {
            let mut kept: Vec<(bool, TypeId)> = Vec::new();
            for (tilde, t) in ts {
                if comparable_type(tt, *t, false, &mut Vec::new())? {
                    kept.push((*tilde, *t));
                }
            }
            if kept.is_empty() {
                Ok(Empty)
            } else {
                Ok(Terms(kept))
            }
        }
        (Terms(xs), Terms(ys)) => {
            let mut out: Vec<(bool, TypeId)> = Vec::new();
            for x in xs {
                for y in ys {
                    if let Some(r) = term_intersect(tt, x, y)? {
                        out.push(r);
                    }
                }
            }
            if out.is_empty() {
                Ok(Empty)
            } else {
                Ok(Terms(out))
            }
        }
    }
}

/// 官方 `_TypeSet.subsetOf`（`typeset.go:109`）：`s1.terms.subsetOf(s2.terms)`。
///
/// ⚠️ **只看项面，不看 `comparable` 位**（官方原注释：
/// "ignore comparable bits — shouldn't matter for our use case"）。
/// 项面视角：`All` 与 `Comparable` 都是 allTermlist。
///
/// # Errors
///
/// 项间 `identical` 透传能力边界码。
pub fn subset_of(tt: &TypeTable, x: &IfaceTSet, y: &IfaceTSet) -> Result<bool, DiagCode> {
    use IfaceTSet::{All, Comparable, Empty, Terms};
    match (x, y) {
        // allT ⊆ y：y 的项面也必须是 allT（All 或 Comparable）
        (All | Comparable, All | Comparable) => Ok(true),
        (All | Comparable, _) => Ok(false),
        // x 非 allT，y 是 allT ⇒ 恒真
        (_, All | Comparable) => Ok(true),
        // yl.isEmpty() ⇒ xl.isEmpty()
        (Empty, Empty) => Ok(true),
        (Empty, Terms(_)) => Ok(true),
        (Terms(_), Empty) => Ok(false),
        (Terms(xs), Terms(ys)) => terms_subset(tt, xs, ys),
    }
}

/// 官方 `termlist.subsetOf`（`termlist.go:152`）的**裸项表**形态：
/// `yl.isEmpty() ⇒ xl.isEmpty()`，否则 `xl` 每项都要被 `yl` 里某项包含
///（两个空表分支的语义与空循环的自然结果一致，故不再特判）。
fn terms_subset(
    tt: &TypeTable,
    x: &[(bool, TypeId)],
    y: &[(bool, TypeId)],
) -> Result<bool, DiagCode> {
    for x in x {
        let mut ok = false;
        for y in y {
            if term_subset(tt, x, y)? {
                ok = true;
                break;
            }
        }
        if !ok {
            return Ok(false);
        }
    }
    Ok(true)
}

/// 官方 `termlist.equal`（`termlist.go:126`）：双向 `subsetOf`。
///
/// 消费点：`identical` 的 `union` 面（官方
/// `computeUnionTypeSet(...).terms.equal(...)`，`predicates.go:383`）。
///
/// # Errors
///
/// 项间 `identical` 透传能力边界码（M4/Task 10 后不再可达）。
pub(crate) fn terms_equal(
    tt: &TypeTable,
    x: &[(bool, TypeId)],
    y: &[(bool, TypeId)],
) -> Result<bool, DiagCode> {
    Ok(terms_subset(tt, x, y)? && terms_subset(tt, y, x)?)
}

/// 官方 `termlist.includes`（`termlist.go:100`）：`t ∈ x`。
///
/// 全语义面：`All`/`Comparable`（项面 𝓤）恒真、`Empty` 恒假、有限项表取 ∃。
/// `implements` 的调用点带 `has_terms()` 卫兵（∃ 项包含 V 面），
/// 本函数把未卫兵的形态也答全。
///
/// # Errors
///
/// 项与 V 的 `identical` 透传能力边界码。
pub fn includes(tt: &TypeTable, v: TypeId, s: &IfaceTSet) -> Result<bool, DiagCode> {
    match s {
        IfaceTSet::All | IfaceTSet::Comparable => Ok(true),
        IfaceTSet::Empty => Ok(false),
        IfaceTSet::Terms(ts) => {
            for t in ts {
                if term_includes(tt, t, v)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

/// 官方 `_TypeSet.IsComparable(nil)`（本 crate 的公开形态，`seen` 每次新开）。
///
/// # Errors
///
/// 项的严格可比判定透传能力边界码。
pub fn is_comparable(tt: &TypeTable, s: &IfaceTSet) -> Result<bool, DiagCode> {
    is_comparable_seen(tt, s, &mut Vec::new())
}

/// 官方 `_TypeSet.IsComparable(seen)`：`terms.isAll()` ⇒ `comparable` 位；
/// 否则**每项**严格可比（`is` 对空表返回 `f(nil)`，即 ∅ ⇒ false）。
pub(crate) fn is_comparable_seen(
    tt: &TypeTable,
    s: &IfaceTSet,
    seen: &mut Vec<TypeId>,
) -> Result<bool, DiagCode> {
    match s {
        // terms.isAll() ⇒ 位本身
        IfaceTSet::All => Ok(false),
        IfaceTSet::Comparable => Ok(true),
        // isEmpty ⇒ hasTerms false ⇒ f(nil) ⇒ false
        IfaceTSet::Empty => Ok(false),
        IfaceTSet::Terms(ts) => {
            for (_, t) in ts {
                if !comparable_type(tt, *t, false, seen)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
}

/// 官方 `term.includes`（`typeterm.go:99`）：`t ∈ x`。
///
/// `~T` ⇒ `Identical(T, V.Underlying())`；精确项 ⇒ `Identical(T, V)`。
pub(crate) fn term_includes(
    tt: &TypeTable,
    x: &(bool, TypeId),
    v: TypeId,
) -> Result<bool, DiagCode> {
    if x.0 {
        identical(tt, x.1, underlying(tt, v))
    } else {
        identical(tt, x.1, v)
    }
}

/// 官方 `term.subsetOf`（`typeterm.go:117`）。
fn term_subset(tt: &TypeTable, x: &(bool, TypeId), y: &(bool, TypeId)) -> Result<bool, DiagCode> {
    if term_disjoint(tt, x, y)? {
        return Ok(false);
    }
    // 同底层：`~t ⊆ ~t` / `T ⊆ ~t` / `T ⊆ T` 真；`~t ⊆ T` 假
    Ok(!x.0 || y.0)
}

/// 官方 `term.intersect`（`typeterm.go:74`；`None` ⇒ ∅ 项）。
fn term_intersect(
    tt: &TypeTable,
    x: &(bool, TypeId),
    y: &(bool, TypeId),
) -> Result<Option<(bool, TypeId)>, DiagCode> {
    if term_disjoint(tt, x, y)? {
        return Ok(None);
    }
    // `~t ∩ T == T`；`T ∩ ~t == T`；否则取 x（官方 `if !x.tilde || y.tilde { return x }`）
    if !x.0 || y.0 {
        Ok(Some(*x))
    } else {
        Ok(Some(*y))
    }
}

/// 官方 `term.disjoint`（`typeterm.go:151`）：取 `tilde` 调整后的底层比同一。
fn term_disjoint(tt: &TypeTable, x: &(bool, TypeId), y: &(bool, TypeId)) -> Result<bool, DiagCode> {
    let ux = if y.0 { underlying(tt, x.1) } else { x.1 };
    let uy = if x.0 { underlying(tt, y.1) } else { y.1 };
    Ok(!identical(tt, ux, uy)?)
}

/// 类型参数的**核心类型**（官方 `coreType`，`go/types/predicates.go`）。
///
/// # 规范怎么说
///
/// 「If `T` is a type parameter, `T`'s core type is the core type of its type
/// set」；type set 的**所有**类型的 underlying **相同**时，那个 underlying 就是
/// 核心类型。有了它，`*p` / `ch <- x` / `x[i]` / `f()` 这些**操作符**才判得下去：
/// `p T` 且 `T interface{ ~*int }` 时 `*p` 合法，理由不是 `T` 是指针类型，而是
/// 「`T` 的每个类型都是指针」。
///
/// # 只做一档：项表非空且 underlying 全同
///
/// 语料里的形状就是这一档（`~int` / `~*int` / `~chan int` / `~func()` /
/// `~map[string]V` / `~[10]int` / `~[]E`）。其余（`interface{}`、只有方法集的、
/// 项表里 underlying 不一致的）⇒ `None`，调用方按「没有核心类型 ⇒ 不能这么用」
/// 处理，与官方一致。
///
/// 非类型参数 ⇒ `None`（本函数的用途就是「把类型参数往回填一层」；
/// 具体类型的行为不该被它改写 —— 那是 `underlying` 的事）。
#[must_use]
pub fn core_type(tt: &TypeTable, t: TypeId) -> Option<TypeId> {
    use crate::typ::Type;
    let Type::TypeParam(id) = tt.type_of(t) else {
        return None;
    };
    let constraint = unalias(tt, tt.type_param_def(*id).constraint);
    let Type::Interface(iid) = tt.type_of(constraint) else {
        return None;
    };
    let IfaceTSet::Terms(terms) = &tt.interface_def(*iid).tset else {
        return None;
    };
    let mut rest = terms.iter();
    let first = underlying(tt, rest.next()?.1);
    for (_, x) in rest {
        if !identical(tt, underlying(tt, *x), first).unwrap_or(false) {
            return None;
        }
    }
    Some(first)
}

/// 类型图里**任何可达位置**是类型参数 ⇒ `true`（`M12/Task 21`）。
///
/// `is_abstract`（发码口）的判据：`rgoc-load/src/mono.rs` 原来只看
/// `layout(..).is_none()` —— 直接是类型参数（`Some2·tp5`）挡得住，但
/// **named 包着的**（`issue54225` 的 `Two[Node[One.tp0]]`：实参是
/// `Node[tp0]`，struct 实例有布局）挡不住 ⇒ 被当具体实例下沉，体内
/// `v.contentLen()` 去找永远不会存在的 `Node·tp0.contentLen` ⇒ 误报。
///
/// 与官方对齐：官方从不实例化「体内引用类型参数」的泛型（`One` 的原型
/// 体不生成代码，`Two[Node[One.tp0]]` 无从发起）；发码口的抽象判定因此
/// 必须**递归** —— named 的 `targs`、named 的本体、指针/切片/数组/map/
/// chan 的元素、struct 字段、签名参数/结果、接口方法签名，任何一层碰到
/// `Type::TypeParam` 都算。
///
/// 防环：`seen` 记已访的 `TypeId`（递归类型 `type N struct{ p *N }` 终止）。
#[must_use]
pub fn type_mentions_tparam(tt: &TypeTable, t: TypeId) -> bool {
    use crate::typ::Type;
    fn go(tt: &TypeTable, t: TypeId, seen: &mut Vec<TypeId>) -> bool {
        let cur = unalias(tt, t);
        if seen.contains(&cur) {
            return false;
        }
        seen.push(cur);
        match tt.type_of(cur) {
            Type::TypeParam(_) => true,
            Type::Named(nid) => {
                let def = tt.named_def(*nid);
                def.targs.iter().any(|&a| go(tt, a, seen))
                    || def.underlying.is_some_and(|u| go(tt, u, seen))
            }
            Type::Pointer(e) | Type::Slice(e) | Type::Chan { elem: e, .. } => go(tt, *e, seen),
            Type::Array { elem, .. } => go(tt, *elem, seen),
            Type::Map { key, elem } => go(tt, *key, seen) || go(tt, *elem, seen),
            Type::Struct(sid) => tt
                .struct_def(*sid)
                .fields
                .iter()
                .any(|f| go(tt, f.typ, seen)),
            Type::Signature(sid) => {
                let def = tt.signature_def(*sid);
                def.recv.is_some_and(|r| go(tt, r, seen))
                    || go(tt, def.params, seen)
                    || go(tt, def.results, seen)
            }
            Type::Tuple(tid) => tt
                .tuple_def(*tid)
                .vars
                .iter()
                .any(|(_, ty)| go(tt, *ty, seen)),
            Type::Interface(iid) => {
                let def = tt.interface_def(*iid);
                def.methods.iter().any(|(_, sig)| go(tt, *sig, seen))
                    || def.terms.is_some_and(|u| go(tt, u, seen))
            }
            Type::Union(uid) => tt
                .union_def(*uid)
                .terms
                .iter()
                .any(|(_, ty)| go(tt, *ty, seen)),
            Type::Basic(_) | Type::Invalid => false,
            // `unalias` 已剥别名 ⇒ 此臂不可达；穷尽匹配要求写出。
            Type::Alias(_) => false,
        }
    }
    go(tt, t, &mut Vec::new())
}

#[cfg(test)]
mod tests {
    use super::type_mentions_tparam;
    use crate::table::{Field, TypeTable};
    use crate::typ::{ChanDir, Type};

    use rgoc_const::BasicId;

    fn empty_iface(t: &mut TypeTable) -> crate::typ::TypeId {
        let iface = t.add_interface(Vec::new(), None, super::IfaceTSet::All);
        t.interface_type(iface)
    }

    #[test]
    fn 直接类型参数命中() {
        let mut t = TypeTable::new();
        let iface_ty = empty_iface(&mut t);
        let tp = t.add_type_param(0, iface_ty);
        let tp_ty = t.type_param_type(tp);
        assert!(matches!(t.type_of(tp_ty), Type::TypeParam(_)));
        assert!(type_mentions_tparam(&t, tp_ty));
    }

    #[test]
    fn 具体类型不命中() {
        let mut t = TypeTable::new();
        let int = t.add_basic(BasicId::Int);
        let p = t.add_pointer(int);
        let s = t.add_slice(p);
        let m = t.add_map(int, s);
        assert!(!type_mentions_tparam(&t, m));
    }

    #[test]
    fn named实例的targs含类型参数命中_issue54225形态() {
        // `One[V Value]` 原型体里的 `Node[V]`：Node 实例的 targs = [V]
        let mut t = TypeTable::new();
        let iface_ty = empty_iface(&mut t);
        let tp = t.add_type_param(0, iface_ty);
        let tp_ty = t.type_param_type(tp);
        // 原型 Node（underlying = struct{}）
        let node = t.add_named("Node");
        let st = t.add_struct(Vec::new());
        t.set_named_underlying(node, t.struct_type(st));
        // 实例 Node[V]
        let inst = t.add_named_instance(node, vec![tp_ty]);
        assert!(type_mentions_tparam(&t, t.named_type(inst)));
        // 原型自身不命中
        assert!(!type_mentions_tparam(&t, t.named_type(node)));
    }

    #[test]
    fn named本体struct字段含类型参数命中() {
        // `type Box[T any] struct{ v T }` 的原型：underlying 的字段是 T
        let mut t = TypeTable::new();
        let iface_ty = empty_iface(&mut t);
        let tp = t.add_type_param(0, iface_ty);
        let tp_ty = t.type_param_type(tp);
        let bx = t.add_named("Box");
        let st = t.add_struct(vec![Field {
            name: Some("v".into()),
            typ: tp_ty,
            tag: None,
            embedded: false,
        }]);
        t.set_named_underlying(bx, t.struct_type(st));
        assert!(type_mentions_tparam(&t, t.named_type(bx)));
    }

    #[test]
    fn 各容器壳都下钻() {
        let mut t = TypeTable::new();
        let iface_ty = empty_iface(&mut t);
        let tp = t.add_type_param(0, iface_ty);
        let v = t.type_param_type(tp);
        let arr = t.add_array(None, v);
        let ch = t.add_chan(ChanDir::SendRecv, v);
        let tid = t.add_tuple(vec![(None, v)]);
        let tup = t.tuple_type(tid);
        assert!(type_mentions_tparam(&t, arr));
        assert!(type_mentions_tparam(&t, ch));
        assert!(type_mentions_tparam(&t, tup));
    }

    #[test]
    fn 递归类型不挂() {
        // `type N struct{ p *N }`：走访终止
        let mut t = TypeTable::new();
        let n = t.add_named("N");
        let ptr = t.add_pointer(t.named_type(n));
        let st = t.add_struct(vec![Field {
            name: Some("p".into()),
            typ: ptr,
            tag: None,
            embedded: false,
        }]);
        t.set_named_underlying(n, t.struct_type(st));
        assert!(!type_mentions_tparam(&t, t.named_type(n)));
    }
}
