//! 可赋值性判定（M4/Task 6；官方 `go/types` 的 `operand.go::assignableTo`）。
//!
//! # 判据来源
//!
//! * `operand.go:328` 起 `assignableTo` —— 规则**顺序**逐条对齐：
//!   ① identical（两侧先 `unalias`）→ ② untyped 面（`implicitTypeAndValue`）→
//!   ③ 底层同一且至少一侧非具名（两侧非类型参数）→ ④ 接口 `implements` 面 →
//!   ⑤ 「V 是接口」的提示面（结果恒假，只改 cause）→ ⑥ 通道方向面 →
//!   ⑦ 类型参数面 → ⑧ 假。
//! * `expr.go:388` 起 `implicitTypeAndValue` —— 只取**非 constant 分支**：
//!   本函数没有「值」参数；**常量可表示性**（`1 << 100` 进不进 `int`）是检查器
//!   拿值去问 `rgoc-const` 的事（Task 22），不在本层。
//! * `types.AssignableTo` 的实机探针复核（Task 6 记录见
//!   `.superpowers/sdd/M4-plan/progress.md`）。
//!
//! # 窄面（`Ruling 6-2` 的 M4 收窄）
//!
//! 需要**类型集枚举**（Task 10/12）才能判的形态**显式拒绝**
//! `Err(DiagCode::TypUnsupportedAssignability)`，绝不猜答案：
//!
//! | 形态 | 现行为 | 谁翻成判定 |
//! |---|---|---|
//! | 非空接口作 `T` | **判定**（Task 8：`implements`） | —（本层透传其 `Err` 边界） |
//! | `V` 为类型参数（除 `V == T` 同一条目） | 拒绝\* | Task 10（`identical` 升级）→ Task 12 |
//! | `T` 为类型参数（除已被前面规则判出的面） | 拒绝 | Task 12（类型集枚举） |
//! | 含项集 / `comparable` 的接口参与 `identical` | 拒绝\*\* | Task 10 |
//!
//! \* `V` 为类型参数时，规则 ① 的 `identical` 会先在它的 M3 边界上拒
//! （`TypUnsupportedConstraintIdentity`，`U-M3-05`）——本层透传该码。
//! \*\* 同上：`identical` 的拒点先于本层的一切窄面。
//!
//! 非空接口面（Task 8 起）：`implements(tt, v, t, false)` 直接给答案；
//! 只有方法签名 / 严格可比触到 M3 遗留约束面时透传其能力边界码。
//! `V` 是类型参数且 `implements` 假 ⇒ 官方继续走类型参数面（Task 12/13）⇒ 本层拒绝。
//!
//! # invalid 面
//!
//! 官方开头是 `!x.isValid() || !isValid(T)`：`x.isValid()` 是**操作数模式**面
//! （纯类型 API 里恒真 —— `types.AssignableTo` 实测：`V = Invalid` **不**提前真）；
//! `!isValid(T)` 是类型面（`Unalias(T) == Invalid ⇒ true`，「避免连锁错误」）。
//! **不对称**是官方行为（探针：`Invalid → int` 假、`int → Invalid` 真），
//! 不要「修正」它。

use rgoc_const::BasicId;
use rgoc_lex::diag::DiagCode;

use crate::identical::{identical, unalias, underlying};
use crate::implements::implements;
use crate::table::TypeTable;
use crate::typ::{ChanDir, Type, TypeId};

/// 可赋值性（官方 `assignableTo` 的静态面）。
///
/// * `Ok(true)` / `Ok(false)` —— 判得出；
/// * `Err(DiagCode::TypUnsupportedAssignability)` —— Task 6 窄面（见模块文档）；
/// * `Err(DiagCode::TypUnsupportedConstraintIdentity)` —— `identical` 的 M3 边界
///   在规则 ① 处透传（`U-M3-05`；Task 10 解除）。
///
/// # Errors
///
/// 只在「需要方法集 / 类型集枚举」的形态上出错；其余一律 `Ok`。
pub fn assignable(tt: &TypeTable, v: TypeId, tgt: TypeId) -> Result<bool, DiagCode> {
    // 「避免连锁错误」的类型面：`!isValid(T)`（官方在 unalias 之后比较 Invalid 单例）。
    let t = unalias(tt, tgt);
    if matches!(tt.type_of(t), Type::Invalid) {
        return Ok(true);
    }
    let v = unalias(tt, v);

    // ① V is identical to T（同一条目直接命中，绕过 identical 的类型参数边界）。
    if v == t || identical(tt, v, t)? {
        return Ok(true);
    }

    let vu = underlying(tt, v);
    let tu = underlying(tt, t);
    let vp = matches!(tt.type_of(v), Type::TypeParam(_));
    let tp = matches!(tt.type_of(t), Type::TypeParam(_));

    // ② untyped 面（官方此分支内 `assert(Vp == nil)`）。
    if let Type::Basic(vb) = tt.type_of(vu)
        && is_untyped(*vb)
    {
        if tp {
            // 官方按「T 的每个 specific type 都可隐式转换」判 ⇒ 要类型集枚举
            // （Task 12/13）。
            return Err(DiagCode::TypUnsupportedAssignability);
        }
        return untyped_to(tt, *vb, t, tu);
    }

    // ③ V 与 T 底层同一，且至少一侧非具名，且两侧都不是类型参数。
    //（官方的求值顺序是 Identical 在比较子里；本层先把便宜且不会出错的守卫放前面，
    // 免得在「两侧都是类型参数」时白报 identical 的边界码。）
    if !vp && !tp && identical(tt, vu, tu)? && (!has_name(tt, v) || !has_name(tt, t)) {
        return Ok(true);
    }

    // ④ T 是接口（且非类型参数），或 T 是指向接口的指针（官方为了拿 implements 的 cause）。
    //
    // Task 8 起接口面**真判**（`implements`）：空接口恒真、方法集 / 约束集给答案；
    // 只有能力边界（签名同一性 / 严格可比触到约束面）才透传 `Err`。
    if !tp {
        if let Type::Interface(_) = tt.type_of(tu) {
            if implements(tt, v, t, false)? {
                return Ok(true);
            }
            if vp {
                // 官方：implements 假而 V 是类型参数时**继续**后面的面
                //（特定类型递归 / 收尾报错）—— 那是 Task 12/13 的类型参数面。
                return Err(DiagCode::TypUnsupportedAssignability);
            }
            return Ok(false);
        }
        if is_interface_ptr(tt, tu) {
            // 官方 implements(V, *I)：Ti == nil ⇒ 恒 false。
            // Vp == nil 时当场定假；Vp != nil 时官方会继续走后面的类型参数面 ⇒ 一并拒。
            if !vp {
                return Ok(false);
            }
            return Err(DiagCode::TypUnsupportedAssignability);
        }
    }

    // ⑤ V is an interface：官方此面只为 cause（「需要类型断言」提示）——
    // implements(T, V) 真 ⇒ 返回 false；假 ⇒ 继续。而 V 是接口时通道面不会命中、
    // 收尾也必假 ⇒ **结果恒为 false**。V 是**类型参数**时官方**不进**此面
    //（守卫 `Vp == nil`，`operand.go:394`）⇒ 落收尾的类型参数面（⑦）。
    //（今天到不了这里：规则 ① 的 `identical` 会先在类型参数上回拒；Task 10
    // 升级 `identical` 后本守卫才起效。）
    if !vp && matches!(tt.type_of(vu), Type::Interface(_)) {
        if tp {
            return Err(DiagCode::TypUnsupportedAssignability);
        }
        return Ok(false);
    }

    // ⑥ V 是双向通道值、T 是通道、元素同一，且至少一侧非具名。
    if let Type::Chan {
        dir: ChanDir::SendRecv,
        elem: ve,
    } = tt.type_of(vu)
        && let Type::Chan { elem: te, .. } = tt.type_of(tu)
        && identical(tt, *ve, *te)?
    {
        return Ok(!has_name(tt, v) || !has_name(tt, t));
    }

    // ⑦ 类型参数面（官方两条按 specific type 递归的规则）⇒ 要类型集（Task 12）。
    if vp || tp {
        return Err(DiagCode::TypUnsupportedAssignability);
    }

    // ⑧
    Ok(false)
}

/// untyped 值到 `T` 的**类型层**隐式转换（官方 `implicitTypeAndValue` 非 constant 分支）。
///
/// 本函数没有「值」，所以只答类型兼容面；常量可表示性属检查器（Task 22）。
fn untyped_to(tt: &TypeTable, vk: BasicId, t: TypeId, tu: TypeId) -> Result<bool, DiagCode> {
    debug_assert!(is_untyped(vk), "调用点保证 V 是 untyped");
    // 两侧都 untyped：官方 `maxType` —— 同型 ⇒ 自身；双 untyped 数值 ⇒ 较大 kind；
    // 否则 nil（不可赋值）。（同型几乎总被规则 ① 先行判掉，这里兜种类的底。）
    if let Type::Basic(tb) = tt.type_of(t)
        && is_untyped(*tb)
    {
        return Ok(*tb == vk || (is_untyped_numeric(vk) && is_untyped_numeric(*tb)));
    }
    Ok(match tt.type_of(tu) {
        Type::Basic(b) => match vk {
            BasicId::UntypedBool => is_boolean(*b),
            BasicId::UntypedInt
            | BasicId::UntypedRune
            | BasicId::UntypedFloat
            | BasicId::UntypedComplex => is_numeric(*b),
            BasicId::UntypedString => is_string(*b),
            // `unsafe.Pointer` 是「含 nil 的基本型」（官方 `hasNil` 的 Basic 分支）
            BasicId::UntypedNil => *b == BasicId::UnsafePointer,
            _ => unreachable!("is_untyped(vk) 已排除其余种类"),
        },
        Type::Interface(i) => {
            // nil 进**任何**接口都真（官方 `x.isNil()` 先于 `Empty` 检查；探针复核）
            if vk == BasicId::UntypedNil {
                return Ok(true);
            }
            // 非 nil untyped 只能进**空**接口（官方：cannot assign untyped to non-empty iface）
            let d = tt.interface_def(*i);
            d.methods.is_empty() && d.tset.is_all()
        }
        // pointer / func / slice / map / chan：只有 nil 进得去
        Type::Pointer(_)
        | Type::Signature(_)
        | Type::Slice(_)
        | Type::Map { .. }
        | Type::Chan { .. } => vk == BasicId::UntypedNil,
        // 官方 default：InvalidUntypedConversion
        _ => false,
    })
}

/// 官方 `hasName`：`Unalias(t)` 是 `*Basic` / `*Named` / `*TypeParam`。
///
/// ⚠️ **基本型也算「有名字」** —— `A(具名 int) → int` 因此为假（规则 ③ 要求
/// 「至少一侧非具名」不成立）。
fn has_name(tt: &TypeTable, t: TypeId) -> bool {
    matches!(
        tt.type_of(t),
        Type::Basic(_) | Type::Named(_) | Type::TypeParam(_)
    )
}

/// 官方 `isInterfacePtr`：`T.Underlying()` 是 `*I`（`I` 为接口）。
///
/// ⚠️ 官方的 `IsInterface(p.base)` 对 `p.base` 是**类型参数**时恒假
/// （`IsInterface` 不往里看）——本层照抄这个边界。
fn is_interface_ptr(tt: &TypeTable, t: TypeId) -> bool {
    let u = underlying(tt, t);
    let Type::Pointer(e) = tt.type_of(u) else {
        return false;
    };
    let e = unalias(tt, *e);
    if matches!(tt.type_of(e), Type::TypeParam(_)) {
        return false;
    }
    matches!(tt.type_of(underlying(tt, e)), Type::Interface(_))
}

/// 官方 `isUntyped`（含全部 untyped 种类，`UntypedNil` 在内）。
fn is_untyped(b: BasicId) -> bool {
    matches!(
        b,
        BasicId::UntypedBool
            | BasicId::UntypedInt
            | BasicId::UntypedRune
            | BasicId::UntypedFloat
            | BasicId::UntypedComplex
            | BasicId::UntypedString
            | BasicId::UntypedNil
    )
}

/// 官方 `isUntypedNumeric`（= untyped ∧（integer | float | complex））。
fn is_untyped_numeric(b: BasicId) -> bool {
    matches!(
        b,
        BasicId::UntypedInt
            | BasicId::UntypedRune
            | BasicId::UntypedFloat
            | BasicId::UntypedComplex
    )
}

/// 官方 `isBoolean`（按 `info` 标志位 —— **untyped 含在内**）。
fn is_boolean(b: BasicId) -> bool {
    matches!(b, BasicId::Bool | BasicId::UntypedBool)
}

/// 官方 `isNumeric`（整型含 `uintptr`；untyped 数值含在内）。
fn is_numeric(b: BasicId) -> bool {
    matches!(
        b,
        BasicId::Int
            | BasicId::Int8
            | BasicId::Int16
            | BasicId::Int32
            | BasicId::Int64
            | BasicId::Uint
            | BasicId::Uint8
            | BasicId::Uint16
            | BasicId::Uint32
            | BasicId::Uint64
            | BasicId::Uintptr
            | BasicId::Float32
            | BasicId::Float64
            | BasicId::Complex64
            | BasicId::Complex128
            | BasicId::UntypedInt
            | BasicId::UntypedRune
            | BasicId::UntypedFloat
            | BasicId::UntypedComplex
    )
}

/// 官方 `isString`（untyped string 含在内）。
fn is_string(b: BasicId) -> bool {
    matches!(b, BasicId::String | BasicId::UntypedString)
}
