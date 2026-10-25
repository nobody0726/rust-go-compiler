//! 类型判等与归类（`V3` 的原料；`M6-design.md` §3.5 `V3`）。
//!
//! # 为什么这里**不重建**「`BinOp` → 允许的操作数类型」那张表
//!
//! 计划稿（`M6-plan.md` Task 12）原本要在这里写一张 `allowed_operands` 表，
//! 但那样会出现**两套类型规则**：检查器一套、verifier 一套。它们迟早会分叉，
//! 而分叉的方向永远是对 verifier 有利（它自己那套更宽松），于是「verifier 绿」
//! 就变成一句自我安慰。
//!
//! 更关键的是**职责分工**：SSA 的输入是**已通过检查器的 HIR**。
//! `%` 不能用于浮点、`&` 不能用于字符串 —— 这些合法性**检查器已经判过**，
//! verifier 再判一次只是重复；而 verifier 真正该抓的是**构建器的错**，
//! 那是「同一指令内类型不自洽」（`i64 + string`、`dst` 与 `typ` 不符、
//! 块参数交接类型不匹配），**不需要 op 表就能抓**。
//!
//! 所以本模块只提供两类东西：
//! ① 判等（走 `rgoc_types::identical`，全仓唯一一份类型同一性）；
//! ② 归类（`bool` / 整数 / 数值 / 字符串），从 `Type::Basic` 读，不发明规则。
//!
//! # 与 `rgoc-check` 的关系
//!
//! `rgoc-check` 里的二元运算规则是**诊断**（要不要报 `mismatched types`），
//! 本模块的是**不变式**（IR 是否自洽）。两者判的东西不同，不算重复实现。
//! 如果哪天发现两边确实在判同一件事，正确做法是把判据下沉到 `rgoc-types`，
//! **不是**让 `rgoc-ssa` 依赖 `rgoc-check`（那是逆着依赖图的）。

use rgoc_hir::{BinOp, UnOp, types::ConstVal};
use rgoc_types::{BasicId, TypeId, TypeTable, identical, typ::Type};

use crate::ir::{Instr, SsaFunc};

/// 一个值的静态类型（越界返回 `None` —— `V1` 已保证不越界，这里只为不 panic）。
#[must_use]
pub fn type_of(f: &SsaFunc, v: crate::ir::ValueId) -> Option<TypeId> {
    f.value_types.get(v.index()).copied()
}

/// 是否 `bool`（含未类型化布尔常量）。
///
/// # 为什么判据要认「未类型化」
///
/// 检查器会把常量表达式**折叠**掉：`1 + 2*3` 到 HIR 时已经是一个 `Const(7)`，
/// 而它的静态类型是 `untyped int`。所以 SSA 里确实会出现未类型化的基本类型 ——
/// 它们不是「脏数据」，而是「还没被上下文定型的常量」。判据按**类别**算：
/// 未类型化整数是整数，未类型化布尔是布尔。真正的定型由 M9 的对象模型负责，
/// 不是 verifier 的活（它不能替检查器决定 `7` 该是 `int` 还是 `int64`）。
#[must_use]
pub fn is_bool(tt: &TypeTable, t: TypeId) -> bool {
    matches!(basic(tt, t), Some(BasicId::Bool | BasicId::UntypedBool))
}

/// 是否整数（含 `uintptr`）。
#[must_use]
pub fn is_integer(tt: &TypeTable, t: TypeId) -> bool {
    matches!(
        basic(tt, t),
        Some(
            BasicId::Int
                | BasicId::UntypedInt
                | BasicId::UntypedRune
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
        )
    )
}

/// 是否数值（整数 / 浮点 / 复数）。
#[must_use]
pub fn is_numeric(tt: &TypeTable, t: TypeId) -> bool {
    is_integer(tt, t)
        || matches!(
            basic(tt, t),
            Some(
                BasicId::Float32
                    | BasicId::Float64
                    | BasicId::Complex64
                    | BasicId::Complex128
                    | BasicId::UntypedFloat
                    | BasicId::UntypedComplex
            )
        )
}

/// 是否字符串。
#[must_use]
pub fn is_string(tt: &TypeTable, t: TypeId) -> bool {
    matches!(basic(tt, t), Some(BasicId::String | BasicId::UntypedString))
}

/// 取基本类型（非基本类型返回 `None`）。
fn basic(tt: &TypeTable, t: TypeId) -> Option<BasicId> {
    if t.index() >= tt.len() {
        return None;
    }
    // 走 **underlying**：`type MyFloat float64; var f MyFloat = 0.0` 的常量
    // 静态类型是具名类型（`Type::Named`），不比 underlying 就会被判成
    // 「Float(0.0) 不可能有类型 MyFloat」—— 实测 `ken/cplx0.go` / `float_lit.go`
    // 一片这种假拒绝。
    match tt.type_of(rgoc_types::underlying(tt, t)) {
        Type::Basic(b) => Some(*b),
        _ => None,
    }
}

/// 两个 `TypeId` 是否**同一个类型**（走 `rgoc_types::identical`，不是比下标）。
///
/// # 为什么不能比下标
///
/// `TypeTable::add_basic` **不去重**：同一个 `int` 在表里可以有多个条目
/// （检查器与 lowering 各建一个，实测 `1` 和 `2` 就拿到了 `t45` / `t46`，
/// 两个都打印成 `int`）。比下标会把「同一个类型」判成「不同类型」，
/// 于是构建器插一堆 `int → int` 的假转换、`V3` 还会拒掉合法的边。
#[must_use]
pub fn same_type(tt: &TypeTable, a: TypeId, b: TypeId) -> bool {
    identical(tt, a, b).unwrap_or(false)
}

/// 判等（`identical` 的 `Err` 一律当「不等」，并把原因带上）。
fn same(tt: &TypeTable, a: TypeId, b: TypeId) -> Result<bool, String> {
    identical(tt, a, b).map_err(|e| format!("判等失败（{e:?}）：{a:?} vs {b:?}"))
}

/// 一条指令的类型自洽（`V3`；`Ok(())` 表示没问题，`Err` 是**人读原因**）。
///
/// # 检查面
///
/// | 指令 | 判据 |
/// |---|---|
/// | `Const` | 值类别与静态类型一致（整数/浮点/复数/字符串/布尔） |
/// | `Copy` | 两侧同型 |
/// | `Bin` | 两操作数同型；结果 `dst` 与 `typ` 同型；比较得 `bool`；移位计数是整数 |
/// | `Un` | `-`/`+` 要数值、`!` 要 `bool`、`^` 要整数；结果同型 |
/// | `Convert` | `dst` 的类型 == `typ`（转换本来就是换类型，只查「记的是不是目标型」） |
/// | `Composite` / `Make` / `Load` | `dst` 的类型 == `typ` |
/// | `Field` / `Index` / `Slice` / `AddrOf` / `Store` / `Call` | **不查**（见下） |
///
/// # 不查的四类及理由
///
/// `Field`/`Index`/`Slice` 要沿类型结构走（结构体字段表、数组元素型、切片元素型），
/// 那是 `rgoc-types` 的事，`V3` 重复一遍只会制造第二套「字段怎么找」的逻辑；
/// `AddrOf` 的结果类型由「根的变量类型 → 指针」决定，而根上的变量类型不在
/// `SsaFunc` 里（在 HIR）；`Call` 的结果数由 `V5` 查，结果类型要签名表 —— 那是
/// Task 13 与构建器的事。**这四类不是「忘了」**，是判据的原料不在本层。
pub fn check_instr(f: &SsaFunc, i: &Instr, tt: &TypeTable) -> Result<(), String> {
    match i {
        Instr::Const { dst, c, typ } => {
            let t = need(f, *dst)?;
            if t != *typ {
                return Err(format!("const 的 dst 类型 {t:?} 与 typ {typ:?} 不符"));
            }
            let ok = match c {
                // `Int(0)` 在**任意**类型上表示「该类型的零值」（`R-20-1`）：
                // `nil`（指针/slice/map/func/接口）、结构体的零值、以及 mem 令牌的种子
                // 都用它。SSA 没有独立的 nil 常量，而 `ConstVal` 属于 M5 的契约，
                // 不为 SSA 加第 5 个变体 —— 所以「0 = 零值」这条约定必须写在这里。
                ConstVal::Int(n) if *n == 0 => true,
                ConstVal::Int(_) => is_integer(tt, t),
                // 未类型化的浮点/复数是**合法**的静态类型：`0.0` 在上下文定型前
                // 就是 `untyped float`（与 `Int` 走 `is_integer` 认 untyped 同理）。
                // 不认它会把 `float_lit.go` / `ken/cplx*.go` 判成「常量类型不符」。
                ConstVal::Float(_) => matches!(
                    basic(tt, t),
                    Some(BasicId::Float32 | BasicId::Float64 | BasicId::UntypedFloat)
                ),
                ConstVal::Complex(..) => matches!(
                    basic(tt, t),
                    Some(BasicId::Complex64 | BasicId::Complex128 | BasicId::UntypedComplex)
                ),
                ConstVal::Str(_) => is_string(tt, t),
                ConstVal::Bool(_) => is_bool(tt, t),
            };
            if !ok {
                return Err(format!(
                    "常量 {c:?} 不可能有类型 {t:?}（值类别与静态类型不符）"
                ));
            }
            Ok(())
        }
        Instr::Copy { dst, src } => {
            let (a, b) = (need(f, *dst)?, need(f, *src)?);
            if !same(tt, a, b)? {
                return Err(format!("copy 两侧类型不同：{a:?} ← {b:?}"));
            }
            Ok(())
        }
        Instr::Bin {
            dst,
            op,
            lhs,
            rhs,
            typ,
        } => check_bin(f, tt, *dst, *op, *lhs, *rhs, *typ),
        Instr::Un { dst, op, x, typ } => check_un(f, tt, *dst, *op, *x, *typ),
        Instr::Convert { dst, typ, .. } => match_type(f, *dst, *typ, "convert"),
        Instr::Composite { dst, typ, .. } => match_type(f, *dst, *typ, "composite"),
        Instr::Make { dst, typ, .. } => match_type(f, *dst, *typ, "make"),
        Instr::Load { dst, typ, .. } => match_type(f, *dst, *typ, "load"),
        // 类型断言（`M9/Task 33`）：dst 的类型必须等于 `typ`；源必须是接口；
        // `ok` 必须是 bool。三条都是「原料在本层」的判定（不像 `Field` 要字段表）。
        Instr::TypeAssert { dst, ok, x, typ } => {
            match_type(f, *dst, *typ, "assert")?;
            let xt = need(f, *x)?;
            if !is_iface_ty(tt, xt) {
                return Err(format!("assert 的源类型 {xt:?} 不是接口"));
            }
            if let Some(o) = ok {
                let ot = need(f, *o)?;
                if !is_bool(tt, ot) {
                    return Err(format!("assert 的 ok 必须是 bool，实际 {ot:?}"));
                }
            }
            Ok(())
        }
        // `FuncAddr`：dst 必须是 `func(...)` 类型（符号面不需要更多判据）。
        Instr::FuncAddr { dst, .. } => {
            let dt = need(f, *dst)?;
            if !matches!(
                tt.type_of(rgoc_types::underlying(tt, dt)),
                rgoc_types::typ::Type::Signature(_)
            ) {
                return Err(format!("funcaddr 的 dst 类型 {dt:?} 不是函数类型"));
            }
            Ok(())
        }
        // 闭包两条：`MakeClosure` 的捕获必须是**指针**、`CapturePtr` 的 dst
        // 必须是指针。两者都是「形状对不上就是构建 bug」的判据。
        Instr::MakeClosure { dst, captures, .. } => {
            let dt = need(f, *dst)?;
            if !matches!(
                tt.type_of(rgoc_types::underlying(tt, dt)),
                rgoc_types::typ::Type::Signature(_)
            ) {
                return Err(format!("closure 的 dst 类型 {dt:?} 不是函数类型"));
            }
            for c in captures {
                let ct = need(f, *c)?;
                if !matches!(
                    tt.type_of(rgoc_types::underlying(tt, ct)),
                    rgoc_types::typ::Type::Pointer(_)
                ) {
                    return Err(format!("closure 的捕获 {ct:?} 不是指针"));
                }
            }
            Ok(())
        }
        Instr::CapturePtr { dst, .. } => {
            let dt = need(f, *dst)?;
            if !matches!(
                tt.type_of(rgoc_types::underlying(tt, dt)),
                rgoc_types::typ::Type::Pointer(_)
            ) {
                return Err(format!("capture 的 dst 类型 {dt:?} 不是指针"));
            }
            Ok(())
        }
        Instr::AddrOf { .. }
        | Instr::Field { .. }
        | Instr::Index { .. }
        | Instr::Slice { .. }
        | Instr::Store { .. }
        | Instr::Call { .. } => Ok(()),
    }
}

/// 是不是接口类型（剥具名 / 别名，不剥指针）。
fn is_iface_ty(tt: &TypeTable, t: TypeId) -> bool {
    use rgoc_types::typ::Type;
    let mut cur = t;
    for _ in 0..64 {
        match tt.type_of(cur) {
            Type::Named(id) => cur = tt.named_underlying(*id),
            Type::Alias(id) => cur = tt.alias_rhs(*id),
            Type::Interface(_) => return true,
            _ => return false,
        }
    }
    false
}

fn need(f: &SsaFunc, v: crate::ir::ValueId) -> Result<TypeId, String> {
    type_of(f, v).ok_or_else(|| format!("v{} 没有静态类型（value_types 越界）", v.0))
}

fn match_type(f: &SsaFunc, dst: crate::ir::ValueId, typ: TypeId, what: &str) -> Result<(), String> {
    let t = need(f, dst)?;
    if t != typ {
        return Err(format!("{what} 的 dst 类型 {t:?} 与 typ {typ:?} 不符"));
    }
    Ok(())
}

fn check_bin(
    f: &SsaFunc,
    tt: &TypeTable,
    dst: crate::ir::ValueId,
    op: BinOp,
    lhs: crate::ir::ValueId,
    rhs: crate::ir::ValueId,
    typ: TypeId,
) -> Result<(), String> {
    let (xt, yt, zt) = (need(f, lhs)?, need(f, rhs)?, need(f, dst)?);
    if zt != typ {
        return Err(format!("bin 的 dst 类型 {zt:?} 与 typ {typ:?} 不符"));
    }
    match op {
        // 移位：左操作数与结果是同一整数型，**计数只需是整数**
        // （不要求与左侧同型 —— `int64 << uint` 合法，`T-EX-60` 踩过这条）。
        // 所以「两侧同型」这条**不能**放在 match 之前当公共前提。
        BinOp::Shl | BinOp::Shr => {
            if !is_integer(tt, xt) {
                return Err(format!("移位 {op:?} 的左操作数必须是整数，实际 {xt:?}"));
            }
            if !is_integer(tt, yt) {
                return Err(format!("移位 {op:?} 的计数必须是整数，实际 {yt:?}"));
            }
            if !same(tt, xt, zt)? {
                return Err(format!(
                    "移位 {op:?} 的结果类型必须等于左操作数：{xt:?} vs {zt:?}"
                ));
            }
            Ok(())
        }
        _ => {
            // 其余运算都要求两侧同型（Go 的二元运算在检查器里已做完隐式转换）
            if !same(tt, xt, yt)? {
                return Err(format!("bin {op:?} 的两个操作数类型不同：{xt:?} vs {yt:?}"));
            }
            match op {
                // 比较：结果必须是 bool
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    if !is_bool(tt, zt) {
                        return Err(format!("比较表达式的结果必须是 bool，实际 {zt:?}"));
                    }
                    Ok(())
                }
                // 逻辑：两侧 bool（上面已判同型 ⇒ 查一侧即可），结果 bool
                BinOp::LAnd | BinOp::LOr => {
                    if !is_bool(tt, xt) {
                        return Err(format!("逻辑 {op:?} 的操作数必须是 bool，实际 {xt:?}"));
                    }
                    if !is_bool(tt, zt) {
                        return Err(format!("逻辑 {op:?} 的结果必须是 bool，实际 {zt:?}"));
                    }
                    Ok(())
                }
                // 其余（含字符串 `+`）：结果同型
                _ => {
                    if !same(tt, xt, zt)? {
                        return Err(format!(
                            "bin {op:?} 的结果类型必须等于操作数类型：{xt:?} vs {zt:?}"
                        ));
                    }
                    Ok(())
                }
            }
        }
    }
}

fn check_un(
    f: &SsaFunc,
    tt: &TypeTable,
    dst: crate::ir::ValueId,
    op: UnOp,
    x: crate::ir::ValueId,
    typ: TypeId,
) -> Result<(), String> {
    let (xt, zt) = (need(f, x)?, need(f, dst)?);
    if zt != typ {
        return Err(format!("un 的 dst 类型 {zt:?} 与 typ {typ:?} 不符"));
    }
    if !same(tt, xt, zt)? {
        return Err(format!(
            "un {op:?} 的结果类型必须等于操作数类型：{xt:?} vs {zt:?}"
        ));
    }
    let ok = match op {
        UnOp::Neg | UnOp::Pos => is_numeric(tt, xt),
        UnOp::Not => is_bool(tt, xt),
        UnOp::BitNot => is_integer(tt, xt),
        // `<-ch`：M6 既不建图也不执行（`SSA-UNSUPPORTED-CHAN`），出现即错
        UnOp::Recv => false,
    };
    if !ok {
        return Err(format!("un {op:?} 不接受类型 {xt:?}"));
    }
    Ok(())
}
