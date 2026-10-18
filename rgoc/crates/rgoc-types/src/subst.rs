//! 类型参数替换（M4/Task 12；官方 `go/types/subst.go` 的 v1 子集）。
//!
//! # 判据来源
//!
//! 官方 `subst.go` 的 `(*subster).typ` 分派；逐条对齐的要点：
//!
//! 1. **只重建受影响的节点**：未命中的子树**复用原条目**（同一 `TypeId`）——
//!    「替换没动它 ⇒ 还是原条目」。这条是**判据**（表不膨胀 + 同一性维护），
//!    不是优化。
//! 2. `Type::TypeParam(tp)`：命中替换表 ⇒ 实参条目；未命中 ⇒ 原样
//!    （官方对「不是替换对象的类型参数」在多数语境保留原样；「换新名」的
//!    签名语境在 v1 判据里不可见）。
//! 3. `Type::Named` / `Type::Alias`：**停步**（不走访本体）。官方对无类型参数的
//!    `*Named` 返回原样；**平实例**（`X[A]` 形态的**重实例化**）是 M4 后续任务
//!    （本仓装载期把 `X[A]` 退化为 `X` —— `T-TY-29`；D-M4-11 保持装载口径）。
//! 4. `Type::Signature`：**接收者保持**（官方同 —— 接收者的替换在
//!    [`crate::instantiate::instantiate`] 的方法面单独做：按**接收者类型参数**
//!    位置对齐实参，官方 `named.go::expandMethod` 的 `makeSubstMap(rtpars, rtargs)`）。
//!    重建成的新签名**保留** `recv_tparams`（v1 简化：官方 `expandMethod` 产出的
//!    实例方法不再带 rparams，但该差异在 v1 的同一性/dump 判据里不可见）。
//! 5. `Type::Interface`：方法签名、语法项集（`terms`）、语义项集（`tset` 的
//!    `Terms`）都要替换；`All`/`Comparable`/`Empty` 无子项不动作。
//! 6. 共享结构用 `visited` 记忆化（同一个子树多次出现只重建一次）。
//!
//! # v1 边界
//!
//! 本函数**不递归**进 `Named`/`Alias` 的本体，因此不可能遇到类型图里的环
//! （访到的每个匿名构造都以 `Named` 引用收束）；返回 `TypeId` 而非 `Result`。

use std::collections::HashMap;

use crate::table::{Field, TypeTable};
use crate::typ::{Type, TypeId, TypeParamId};
use crate::typeset::IfaceTSet;

/// 替换表：类型参数条目 → 实参（类型条目）。
pub type SubstMap = HashMap<TypeParamId, TypeId>;

/// 对 `typ` 施加替换 `map`，返回**替换后**的条目（见模块文档的口径）。
///
/// 空映射 ⇒ 恒等（直接返回 `typ`，不进递归）。
pub fn substitute(tt: &mut TypeTable, typ: TypeId, map: &SubstMap) -> TypeId {
    if map.is_empty() {
        return typ;
    }
    let mut visited: HashMap<TypeId, TypeId> = HashMap::new();
    subst_rec(tt, typ, map, &mut visited)
}

fn subst_rec(
    tt: &mut TypeTable,
    x: TypeId,
    map: &SubstMap,
    visited: &mut HashMap<TypeId, TypeId>,
) -> TypeId {
    if let Some(&out) = visited.get(&x) {
        return out;
    }
    let out = match *tt.type_of(x) {
        // 原子：基本型 / 无效 / 定义引用（原点与实例都停步 —— 平实例的重
        // 实例化面留后续任务）。
        Type::Basic(_) | Type::Invalid | Type::Named(_) | Type::Alias(_) => x,
        // 类型参数：命中 ⇒ 实参；未命中 ⇒ 原样。
        Type::TypeParam(tp) => map.get(&tp).copied().unwrap_or(x),
        Type::Pointer(e) => {
            let e2 = subst_rec(tt, e, map, visited);
            if e2 == e { x } else { tt.add_pointer(e2) }
        }
        Type::Slice(e) => {
            let e2 = subst_rec(tt, e, map, visited);
            if e2 == e { x } else { tt.add_slice(e2) }
        }
        Type::Array { len, elem } => {
            let e2 = subst_rec(tt, elem, map, visited);
            if e2 == elem { x } else { tt.add_array(len, e2) }
        }
        Type::Map { key, elem } => {
            let k2 = subst_rec(tt, key, map, visited);
            let e2 = subst_rec(tt, elem, map, visited);
            if k2 == key && e2 == elem {
                x
            } else {
                tt.add_map(k2, e2)
            }
        }
        Type::Chan { dir, elem } => {
            let e2 = subst_rec(tt, elem, map, visited);
            if e2 == elem { x } else { tt.add_chan(dir, e2) }
        }
        Type::Struct(sid) => {
            let def = tt.struct_def(sid).clone();
            let mut changed = false;
            let mut fields = Vec::with_capacity(def.fields.len());
            for f in def.fields {
                let t2 = subst_rec(tt, f.typ, map, visited);
                changed |= t2 != f.typ;
                fields.push(Field { typ: t2, ..f });
            }
            if changed {
                let sid = tt.add_struct(fields);
                tt.struct_type(sid)
            } else {
                x
            }
        }
        Type::Tuple(tid) => {
            let def = tt.tuple_def(tid).clone();
            let mut changed = false;
            let mut vars = Vec::with_capacity(def.vars.len());
            for (n, t) in def.vars {
                let t2 = subst_rec(tt, t, map, visited);
                changed |= t2 != t;
                vars.push((n, t2));
            }
            if changed {
                let tid2 = tt.add_tuple(vars);
                tt.tuple_type(tid2)
            } else {
                x
            }
        }
        Type::Signature(sid) => {
            let def = tt.signature_def(sid).clone();
            let p2 = subst_rec(tt, def.params, map, visited);
            let r2 = subst_rec(tt, def.results, map, visited);
            if p2 == def.params && r2 == def.results {
                x
            } else {
                let Type::Tuple(pt) = *tt.type_of(p2) else {
                    unreachable!("签名的参数位必须是 tuple");
                };
                let Type::Tuple(rt) = *tt.type_of(r2) else {
                    unreachable!("签名的结果位必须是 tuple");
                };
                let nsid = tt.add_signature(def.recv, pt, rt, def.variadic);
                tt.set_signature_recv_tparams(nsid, def.recv_tparams.clone());
                tt.signature_type(nsid)
            }
        }
        Type::Interface(iid) => {
            let def = tt.interface_def(iid).clone();
            let mut changed = false;
            let mut methods = Vec::with_capacity(def.methods.len());
            for (name, sig) in &def.methods {
                let s2 = subst_rec(tt, *sig, map, visited);
                changed |= s2 != *sig;
                methods.push((name.clone(), s2));
            }
            let terms = match def.terms {
                Some(t) => {
                    let t2 = subst_rec(tt, t, map, visited);
                    changed |= t2 != t;
                    Some(t2)
                }
                None => None,
            };
            let tset = match &def.tset {
                IfaceTSet::Terms(ts) => {
                    let mut ts2 = Vec::with_capacity(ts.len());
                    for (tilde, t) in ts {
                        let t2 = subst_rec(tt, *t, map, visited);
                        changed |= t2 != *t;
                        ts2.push((*tilde, t2));
                    }
                    IfaceTSet::Terms(ts2)
                }
                other => other.clone(),
            };
            if changed {
                let iid2 = tt.add_interface(methods, terms, tset);
                tt.interface_type(iid2)
            } else {
                x
            }
        }
        Type::Union(uid) => {
            let def = tt.union_def(uid).clone();
            let mut changed = false;
            let mut terms = Vec::with_capacity(def.terms.len());
            for (tilde, t) in def.terms {
                let t2 = subst_rec(tt, t, map, visited);
                changed |= t2 != t;
                terms.push((tilde, t2));
            }
            if changed {
                let uid2 = tt.add_union(terms);
                tt.union_type(uid2)
            } else {
                x
            }
        }
    };
    visited.insert(x, out);
    out
}
