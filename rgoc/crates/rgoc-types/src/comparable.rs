//! 可比性判定（M4/Task 8；官方 `go/types/predicates.go::comparableType`）。
//!
//! # 判据来源
//!
//! 官方 `comparableType(T, dynamic, seen)` 逐条对齐：
//!
//! 1. **`seen` 防环先行**：`T` 已见过 ⇒ 视为可比（环上的互递归不再展开）；
//!    `seen` 键是**原 T**（不是它的底层）。
//! 2. 之后 `switch T.Underlying()`：
//!    * `Basic`：`UntypedNil` **不可比**（官方注释：动态类型为 nil 的接口值
//!      不可比较）；其余基本型（**含 `invalid` 种类**）可比；
//!    * `Pointer` / `Chan`：恒可比（不展开元素）；
//!    * `Array`：看元素；
//!    * `Struct`：**全字段**；
//!    * `Interface`：`dynamic && !isTypeParam(T)` ⇒ 恒真（普通接口的**规范**
//!      比较面 —— 动态类型在运行时可能不可比，官方语义如此）；否则看
//!      `tset.IsComparable(seen)`（**严格**面，共享 `seen`）；
//!    * 其余（`slice` / `map` / `func` / 元组 / 签名 / union …）：不可比。
//! 3. `types.Comparable(T)` = `comparableType(T, true, nil)`。
//!
//! # `dynamic` 的两个消费点
//!
//! * `strictly_comparable`（`dynamic = false`）：`implements` 的
//!   `checkComparability` 第一步；
//! * `comparable`（`dynamic = true`）：`constraint && comparableType(V, true)`
//!   那一步（go1.20 门 —— v1 恒允许，`Ruling 8-3`）。

use rgoc_const::BasicId;
use rgoc_lex::diag::DiagCode;

use crate::identical::underlying;
use crate::table::TypeTable;
use crate::typ::{Type, TypeId};
use crate::typeset;

/// 官方 `types.Comparable`：**规范**可比（`dynamic = true`）。
///
/// # Errors
///
/// 项面的严格可比判定透传能力边界码。
pub fn comparable(tt: &TypeTable, t: TypeId) -> Result<bool, DiagCode> {
    comparable_type(tt, t, true, &mut Vec::new())
}

/// **严格**可比（`dynamic = false`）：普通接口不算可比，类型参数看约束集的严格面。
///
/// # Errors
///
/// 项面的严格可比判定透传能力边界码。
pub fn strictly_comparable(tt: &TypeTable, t: TypeId) -> Result<bool, DiagCode> {
    comparable_type(tt, t, false, &mut Vec::new())
}

/// 官方 `comparableType`（`seen` 显式传入 —— 数组 / 结构体 / 类型集内部的
/// 递归**共享**同一份，环上第二次遇到即视为可比）。
pub(crate) fn comparable_type(
    tt: &TypeTable,
    t: TypeId,
    dynamic: bool,
    seen: &mut Vec<TypeId>,
) -> Result<bool, DiagCode> {
    if seen.contains(&t) {
        return Ok(true); // 环：假定可比
    }
    seen.push(t);

    let u = underlying(tt, t);
    match tt.type_of(u) {
        // `invalid` 视为可比（避免连锁错误；官方注释同款）
        Type::Invalid => Ok(true),
        Type::Basic(b) => Ok(*b != BasicId::UntypedNil),
        // 指针 / 通道：恒可比
        Type::Pointer(_) | Type::Chan { .. } => Ok(true),
        Type::Array { elem, .. } => comparable_type(tt, *elem, dynamic, seen),
        Type::Struct(s) => {
            for f in &tt.struct_def(*s).fields {
                if !comparable_type(tt, f.typ, dynamic, seen)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        // 接口：`dynamic && !isTypeParam(T)` ⇒ 恒真；否则看类型集（严格面，共享 seen）。
        //
        // ⚠️ `isTypeParam` 的 `T` 是**原参数**（`underlying` 已把类型参数剥到约束，
        // 所以这里回看 `t` 本身）—— 官方注释：「a non-type-parameter interface
        // is always comparable」。
        Type::Interface(i) => {
            if dynamic && !matches!(tt.type_of(t), Type::TypeParam(_)) {
                return Ok(true);
            }
            typeset::is_comparable_seen(tt, &tt.interface_def(*i).tset, seen)
        }
        // slice / map / func / 元组 / 签名 / union / 类型参数（已剥过）……
        _ => Ok(false),
    }
}
