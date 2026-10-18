//! 接口实现 / 约束满足（M4/Task 8；官方 `go/types/instantiate.go::implements`）。
//!
//! # 判据来源
//!
//! 官方 `implements(V, T, constraint)` 的**守卫序**逐条对齐（`instantiate.go:252+`）：
//!
//! 1. `V` 或 `T` 的底层无效 ⇒ **真**（避免连锁错误）；
//! 2. `V` 是指向无效基的指针 ⇒ **真**（go.dev/issue/49541）；
//! 3. `T` 的底层不是接口 ⇒ **假**（含**指向接口的指针**作 `T` —— `isInterfacePtr`
//!    面由 `assignable` 自己处理，本函数对 `*I` 恒假，与官方 `Ti == nil` 一致）；
//! 4. `T` 是**空接口**（无方法且类型集为 𝓤）⇒ **真**（任意 V，含类型参数）；
//! 5. `V` 是接口且**类型集为空** ⇒ **真**（空集是任何集合的子集）；
//! 6. `T` 的类型集为空（而 V 非空集）⇒ **假**；
//! 7. 方法面（`has_all_methods`）：`V` 是接口 ⇒ 比其方法表；否则比**方法集**
//!    （`methodset`，与官方 `lookupFieldOrMethod` 的 v1 等价面）；
//! 8. `T` 无项集（`hasTerms` 假：𝓤 或 `comparable` 位）⇒ 只看可比性；
//! 9. `V` 是接口（`T` 有项集）⇒ `V.tset ⊆ T.tset`（**只比项面，不比
//!    `comparable` 位** —— 官方 `subsetOf` 原注释）⇒ 再看可比性；
//! 10. `V` 非接口（`T` 有项集）⇒ **成员判**：∃ 项包含 `V`
//!     （官方把 `is(...)` 的谓词取反写成「全不包含」再整体取反，等价于 ∃）；
//! 11. `checkComparability`：`T` 的类型集**非**全可比 ⇒ 真；严格可比 `V` ⇒ 真；
//!     `constraint && 规范可比 V` ⇒ 真；否则假。
//!
//! # v1 的已知差异（台账 `Ruling 8-4`）
//!
//! * 方法名比较**不带包**（官方 `Id(pkg, name)`）⇒ 跨包未导出同名会误配；
//! * 不做 `hasInvalidEmbeddedFields` 豁免（V 的嵌入字段无效时官方宽容判真）；
//! * 类型集**不归一**（`norm()`）—— 消费面都是集合语义，列表形状不进对撞面；
//! * `constraint` 的 go1.20 版本门**恒允许**（按文件版本门控是检查器 Task 20+ 的事）。
//!
//! # 能力边界
//!
//! `Err` 只在「方法签名同一性」或「严格可比」触到 M3 遗留的约束面时透传
//! （那两处的 `Err` 由 Task 10 解除）。

use rgoc_lex::diag::DiagCode;

use crate::comparable::{comparable, strictly_comparable};
use crate::identical::{identical, underlying};
use crate::methodset::methodset;
use crate::table::TypeTable;
use crate::typ::{InterfaceId, Type, TypeId};
use crate::typeset;

/// 官方 `implements(V, T, constraint)`：`constraint = true` 时语义是
/// 「满足约束」（诊断动词 `satisfy`），否则「实现接口」。
///
/// # Errors
///
/// 方法签名 / 项身份的同一性判定与严格可比判定透传能力边界码
/// （见模块文档「能力边界」）。
pub fn implements(
    tt: &TypeTable,
    v: TypeId,
    t: TypeId,
    constraint: bool,
) -> Result<bool, DiagCode> {
    let vu = underlying(tt, v);
    let tu = underlying(tt, t);
    // ① 两侧任一无效 ⇒ 真
    if matches!(tt.type_of(vu), Type::Invalid) || matches!(tt.type_of(tu), Type::Invalid) {
        return Ok(true);
    }
    // ② 指向无效基的指针（避免连锁错误）
    if let Type::Pointer(p) = tt.type_of(vu)
        && matches!(tt.type_of(underlying(tt, *p)), Type::Invalid)
    {
        return Ok(true);
    }
    // ③ T 必须是接口
    let Type::Interface(tid) = tt.type_of(tu) else {
        return Ok(false);
    };
    let tid = *tid;

    // ④ 空接口恒真
    {
        let td = tt.interface_def(tid);
        if td.methods.is_empty() && td.tset.is_all() {
            return Ok(true);
        }
    }
    // ⑤ V 接口且类型集为空 ⇒ 恒满足
    let vi = match tt.type_of(vu) {
        Type::Interface(i) => Some(*i),
        _ => None,
    };
    if let Some(vi) = vi
        && tt.interface_def(vi).tset.is_empty()
    {
        return Ok(true);
    }
    // ⑥ T 类型集为空 ⇒ 不满足
    if tt.interface_def(tid).tset.is_empty() {
        return Ok(false);
    }
    // ⑦ 方法面
    if !has_all_methods(tt, v, tid)? {
        return Ok(false);
    }
    // ⑧–⑩ 项面
    let t_tset = tt.interface_def(tid).tset.clone();
    if !t_tset.has_terms() {
        return check_comparability(tt, v, tid, constraint);
    }
    if let Some(vi) = vi {
        let v_tset = tt.interface_def(vi).tset.clone();
        if !typeset::subset_of(tt, &v_tset, &t_tset)? {
            return Ok(false);
        }
        return check_comparability(tt, v, tid, constraint);
    }
    if !typeset::includes(tt, v, &t_tset)? {
        return Ok(false);
    }
    check_comparability(tt, v, tid, constraint)
}

/// 官方 `checkComparability` 闭包：`Ti.IsComparable()` 不成立时短路为真。
///
/// 官方注释（go1.20 起）：`comparable` 约束的**满足**允许规范可比
/// （`comparableType(V, true)`），但**实现**（`constraint = false`）要求严格可比。
fn check_comparability(
    tt: &TypeTable,
    v: TypeId,
    tid: InterfaceId,
    constraint: bool,
) -> Result<bool, DiagCode> {
    if !typeset::is_comparable(tt, &tt.interface_def(tid).tset)? {
        return Ok(true);
    }
    if strictly_comparable(tt, v)? {
        return Ok(true);
    }
    if constraint && comparable(tt, v)? {
        return Ok(true);
    }
    Ok(false)
}

/// 官方 `hasAllMethods` 的 v1 等价面（差异见模块文档）。
///
/// * `T` 侧要求方 = `InterfaceDef::methods`（构造期已拍平 + 按名去重 + 排序）；
/// * `V` 侧是接口 ⇒ 比其 `methods`（官方 `typeSet().LookupMethod`）；
/// * `V` 侧非接口 ⇒ 比 `methodset(V)`（官方 `lookupFieldOrMethod`）；
/// * 签名比较走 [`identical`]（**接收者不参与**）。
fn has_all_methods(tt: &TypeTable, v: TypeId, tid: InterfaceId) -> Result<bool, DiagCode> {
    let required = tt.interface_def(tid).methods.clone();
    if required.is_empty() {
        return Ok(true);
    }
    let vu = underlying(tt, v);
    if let Type::Interface(vi) = tt.type_of(vu) {
        let have = tt.interface_def(*vi).methods.clone();
        for (name, sig) in &required {
            let Some((_, vsig)) = have.iter().find(|(n, _)| n == name) else {
                return Ok(false);
            };
            if !identical(tt, *vsig, *sig)? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    let ms = methodset(tt, v)?;
    for (name, sig) in &required {
        let Some(e) = ms.iter().find(|e| e.name == *name) else {
            return Ok(false);
        };
        if !identical(tt, e.sig, *sig)? {
            return Ok(false);
        }
    }
    Ok(true)
}
