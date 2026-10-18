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
use crate::identical::{identical, underlying};
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
        (Terms(xs), Terms(ys)) => {
            // 每个 x 都必须被 ys 里某个 y 包含（官方 `supersetOf`）
            for x in xs {
                let mut ok = false;
                for y in ys {
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
    }
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
