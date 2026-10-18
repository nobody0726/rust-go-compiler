//! 可转换性（M4/Task 9；官方 `go/types/conversions.go::(*operand).convertibleTo`
//! 的**静态类型对**面）。
//!
//! # 面
//!
//! 入参是**静态类型对**（没有表达式、没有常量值）。官方 `conversion` 里的
//! 常量特例（`representableConst` / 整数→字符串的码点替换，`conversions.go:23–40`）
//! 属于**检查器**面（Task 22），不在这里。
//!
//! # 规则序（官方逐条；v1 只加窗口，不动顺序）
//!
//! 1. `assignableTo(V, T)` ⇒ 真；
//! 2. `Unalias` 两侧，取底层 `Vu`/`Tu` 与类型参数标记 `Vp`/`Tp`；
//! 3. `IdenticalIgnoreTags(Vu, Tu)` 且两侧都**不是**类型参数 ⇒ 真；
//! 4. 两侧都是指针：`base.Underlying()` 的 `IdenticalIgnoreTags` 且 base
//!    都不是类型参数 ⇒ 真；
//! 5. 两侧整数或浮点 ⇒ 真；6. 两侧复数 ⇒ 真；
//! 7. V 整数 / `[]byte` / `[]rune`，T 字符串 ⇒ 真；
//! 8. V 字符串，T `[]byte` / `[]rune` ⇒ 真；
//! 9. V 指针 / `uintptr`，T `unsafe.Pointer` ⇒ 真；
//! 10. V `unsafe.Pointer`，T 指针 / `uintptr` ⇒ 真；
//! 11. V slice、T 数组或数组指针、元素 `identical`（**不比长度**）⇒ 真；
//! 12. 两侧都无类型参数 ⇒ 假；
//! 13. 类型参数面子句（官方 `Vp.is` / `Tp.is` 的逐项展开，见 [`tp_terms`]）。
//!
//! # v1 的两处刻意口径
//!
//! * **版本门恒允许**：slice→array 的 go1.20 门、slice→数组指针的 go1.17 门
//!   在 v1 **不加** —— 那是官方 `check == nil`（`convertibleTo` 直调）的口径；
//!   检查器（Task 22）按文件版本另行门控。与 `Ruling 8-3` 的 `comparable`
//!   门同款。
//! * **第 1 条的 `Err` 吞掉继续**：`assignable` 对类型参数形态会回能力边界
//!   （`TypUnsupportedAssignability` / `TypUnsupportedConstraintIdentity`，
//!   Task 10/12 收窄）。官方此步只是**快路径**；类型参数形态的正确性由第 13
//!   条**逐项展开**独立给出。论证：「答不出可赋值」不蕴含「答不出可转换」——
//!   `assignable` 的 `Err` 只出现在含类型参数的分支（其规则 ②④⑤⑦），而那些
//!   形态**必然**落进第 13 条的三支之一并给出结论；第 13 条内部的递归若再
//!   答不出，会**透传 `Err`**（响亮，不静默）。所以吞掉第 1 条的 `Err` 不会
//!   让「官方为真」的答案落成 `Ok(false)`。
//!
//! # `Err` 透传的点
//!
//! 第 3/4/11 条的 `identical` / `identical_ignore_tags` 形态边界（v1 对
//! 非 `All` 类型集的接口、裸类型参数拒，`U-M3-05`）会透传；Task 10 升级
//! `identical` 后自然收窄。

use rgoc_const::BasicId;
use rgoc_lex::diag::DiagCode;

use crate::assignable::assignable;
use crate::identical::{identical, identical_ignore_tags, unalias, underlying};
use crate::table::TypeTable;
use crate::typ::{Type, TypeId};
use crate::typeset::IfaceTSet;

/// 可转换性（静态类型对）。
///
/// # Errors
///
/// 透传 `assignable` / `identical` 的能力边界码（见模块文档）。
pub fn convertible(tt: &TypeTable, v: TypeId, tgt: TypeId) -> Result<bool, DiagCode> {
    // 1. 「V 可赋值给 T」（快路径；`Err` 吞掉 —— 论证见模块文档）
    if let Ok(true) = assignable(tt, v, tgt) {
        return Ok(true);
    }

    // 2. Unalias 两侧；Vu/Tu 底层；Vp/Tp 标记
    let v = unalias(tt, v);
    let t = unalias(tt, tgt);
    let vu = underlying(tt, v);
    let tu = underlying(tt, t);
    let vp = matches!(tt.type_of(v), Type::TypeParam(_));
    let tp = matches!(tt.type_of(t), Type::TypeParam(_));

    // 3. 底层 IgnoreTags 同一（两侧都不是类型参数）
    if !vp && !tp && identical_ignore_tags(tt, vu, tu)? {
        return Ok(true);
    }

    // 4. 两侧指针：base 的底层 IgnoreTags 同一（base 都不是类型参数）
    if let Type::Pointer(bv) = tt.type_of(v)
        && let Type::Pointer(bt) = tt.type_of(t)
    {
        let bp = matches!(tt.type_of(*bv), Type::TypeParam(_));
        let bq = matches!(tt.type_of(*bt), Type::TypeParam(_));
        if !bp && !bq && identical_ignore_tags(tt, underlying(tt, *bv), underlying(tt, *bt))? {
            return Ok(true);
        }
    }

    // 5/6. 数值面（整数或浮点 × 整数或浮点；复数 × 复数）
    if is_integer_or_float(tt, vu) && is_integer_or_float(tt, tu) {
        return Ok(true);
    }
    if is_complex(tt, vu) && is_complex(tt, tu) {
        return Ok(true);
    }

    // 7. V 整数 / []byte / []rune ⇒ T 字符串
    if (is_integer(tt, vu) || is_bytes_or_runes(tt, vu)) && is_string(tt, tu) {
        return Ok(true);
    }
    // 8. V 字符串 ⇒ T []byte / []rune
    if is_string(tt, vu) && is_bytes_or_runes(tt, tu) {
        return Ok(true);
    }
    // 9. V 指针 / uintptr ⇒ T unsafe.Pointer
    if (is_pointer(tt, vu) || is_uintptr(tt, vu)) && is_unsafe_pointer(tt, tu) {
        return Ok(true);
    }
    // 10. V unsafe.Pointer ⇒ T 指针 / uintptr
    if is_unsafe_pointer(tt, vu) && (is_pointer(tt, tu) || is_uintptr(tt, tu)) {
        return Ok(true);
    }

    // 11. slice ⇒ 数组 / 数组指针（元素 `identical`；**只比元素不比长度**；
    //     版本门 v1 恒允许 —— 见模块文档）
    if let Type::Slice(ex) = tt.type_of(vu) {
        let te = match tt.type_of(tu) {
            Type::Array { elem, .. } => Some(*elem),
            Type::Pointer(base) => match tt.type_of(underlying(tt, *base)) {
                Type::Array { elem, .. } => Some(*elem),
                _ => None,
            },
            _ => None,
        };
        if let Some(te) = te
            && identical(tt, *ex, te)?
        {
            return Ok(true);
        }
    }

    // 12/13. 类型参数面（官方 `switch { case Vp != nil && Tp != nil: … }`）
    match (vp, tp) {
        (false, false) => Ok(false),
        (true, true) => {
            let (Some(vts), Some(tts)) = (tp_terms(tt, v), tp_terms(tt, t)) else {
                return Ok(false);
            };
            for vt in vts {
                for tt_term in &tts {
                    if !convertible(tt, vt, *tt_term)? {
                        return Ok(false);
                    }
                }
            }
            Ok(true)
        }
        (true, false) => {
            let Some(vts) = tp_terms(tt, v) else {
                return Ok(false);
            };
            for vt in vts {
                if !convertible(tt, vt, t)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        (false, true) => {
            let Some(tts) = tp_terms(tt, t) else {
                return Ok(false);
            };
            for tt_term in tts {
                if !convertible(tt, v, tt_term)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
}

/// 类型参数的「具体项」—— 官方 `TypeParam.is` 的迭代源
/// （`typeparam.go:157` → `_TypeSet.is`，`typeset.go:138`）。
///
/// * `Some(ts)` ⇒ 逐项展开；`~T` 的 tilde **不带进递归**（官方用 `term.typ`）；
/// * `None` ⇒ 无具体项：官方 `f(nil)`，而 `convertibleTo` 的回调一律
///   `return false` ⇒ 判定为假（`[T comparable]` / `[T any]` 即此形态）。
///
/// 约束的取法对齐官方 `TypeParam.iface()`：剥到约束的**底层**；若仍不是接口
/// （手工表里的裸约束），按官方「隐式包成单项接口」处理。
fn tp_terms(tt: &TypeTable, tp: TypeId) -> Option<Vec<TypeId>> {
    let Type::TypeParam(p) = tt.type_of(tp) else {
        return None;
    };
    let constraint = tt.type_param_def(*p).constraint;
    let u = underlying(tt, constraint);
    match tt.type_of(u) {
        Type::Interface(i) => match &tt.interface_def(*i).tset {
            IfaceTSet::Terms(ts) => Some(ts.iter().map(|(_, x)| *x).collect()),
            // `All` / `Comparable` / `Empty` ⇒ 无具体项
            _ => None,
        },
        // 防御：约束若是裸 union（loader 已包成接口，正常到不了）
        Type::Union(un) => Some(tt.union_def(*un).terms.iter().map(|(_, x)| *x).collect()),
        Type::Invalid => None,
        // 防御：官方 `iface()` 对「非接口约束」隐式包成单项接口（`[T int]`
        // ⇒ `interface{ int }`）；loader 已替我们包好，这条只为手工表一致性。
        _ => Some(vec![constraint]),
    }
}

/// `t.Underlying()` 是哪个基本型（官方 `isBasic(t, …)` 的取型；
/// **不穿透类型参数** —— 类型参数的 `Underlying()` 是约束接口，不是 `Basic`）。
fn basic_of(tt: &TypeTable, t: TypeId) -> Option<BasicId> {
    match tt.type_of(underlying(tt, t)) {
        Type::Basic(b) => Some(*b),
        _ => None,
    }
}

/// 官方 `isInteger` 的 kind 面（含 `UntypedInt` / `UntypedRune`；`Byte = Uint8`）。
fn is_integer_kind(b: BasicId) -> bool {
    use BasicId::{
        Int, Int8, Int16, Int32, Int64, Uint, Uint8, Uint16, Uint32, Uint64, Uintptr, UntypedInt,
        UntypedRune,
    };
    matches!(
        b,
        Int | Int8
            | Int16
            | Int32
            | Int64
            | Uint
            | Uint8
            | Uint16
            | Uint32
            | Uint64
            | Uintptr
            | UntypedInt
            | UntypedRune
    )
}

/// 官方 `isFloat` 的 kind 面。
fn is_float_kind(b: BasicId) -> bool {
    matches!(
        b,
        BasicId::Float32 | BasicId::Float64 | BasicId::UntypedFloat
    )
}

/// 官方 `isComplex` 的 kind 面。
fn is_complex_kind(b: BasicId) -> bool {
    matches!(
        b,
        BasicId::Complex64 | BasicId::Complex128 | BasicId::UntypedComplex
    )
}

/// 官方 `isString` 的 kind 面。
fn is_string_kind(b: BasicId) -> bool {
    matches!(b, BasicId::String | BasicId::UntypedString)
}

/// 官方 `isInteger`。
fn is_integer(tt: &TypeTable, t: TypeId) -> bool {
    basic_of(tt, t).is_some_and(is_integer_kind)
}

/// 官方 `isIntegerOrFloat`。
fn is_integer_or_float(tt: &TypeTable, t: TypeId) -> bool {
    basic_of(tt, t).is_some_and(|b| is_integer_kind(b) || is_float_kind(b))
}

/// 官方 `isComplex`。
fn is_complex(tt: &TypeTable, t: TypeId) -> bool {
    basic_of(tt, t).is_some_and(is_complex_kind)
}

/// 官方 `isString`。
fn is_string(tt: &TypeTable, t: TypeId) -> bool {
    basic_of(tt, t).is_some_and(is_string_kind)
}

/// 官方 `isUintptr`（**底层**是 `uintptr` —— `type U uintptr` 也算）。
fn is_uintptr(tt: &TypeTable, t: TypeId) -> bool {
    basic_of(tt, t) == Some(BasicId::Uintptr)
}

/// 官方 `isUnsafePointer`（底层是 `unsafe.Pointer`）。
fn is_unsafe_pointer(tt: &TypeTable, t: TypeId) -> bool {
    basic_of(tt, t) == Some(BasicId::UnsafePointer)
}

/// 官方 `isPointer`（底层是指针）。
fn is_pointer(tt: &TypeTable, t: TypeId) -> bool {
    matches!(tt.type_of(underlying(tt, t)), Type::Pointer(_))
}

/// 官方 `isBytesOrRunes`：`[]T` 且 `T` 的**底层**是 `uint8`/`byte` 或
/// `int32`/`rune`（`Byte = Uint8`、`Rune = Int32`，kind 常量相等 ⇒
/// 本层的 `Uint8`/`Int32` 条目就是它们）。
fn is_bytes_or_runes(tt: &TypeTable, t: TypeId) -> bool {
    match tt.type_of(underlying(tt, t)) {
        Type::Slice(e) => {
            basic_of(tt, *e).is_some_and(|b| matches!(b, BasicId::Uint8 | BasicId::Int32))
        }
        _ => false,
    }
}
