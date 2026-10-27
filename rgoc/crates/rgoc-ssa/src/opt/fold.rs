//! 常量折叠（`M6-design.md` §3.6；`T-SSA-60`）。
//!
//! # 折什么、不折什么
//!
//! 只折**语义完全确定**的情形：
//!
//! | 情形 | 折吗 | 为什么 |
//! |---|---|---|
//! | 整数 `+ - * / % & \| ^ &^ << >>` | ✅ | Go 定义成按宽度回绕，`wrap()` 一处实现 |
//! | 整数除零 / 模零 | ❌ | 那是**运行期 panic**，折掉就把 panic 变没了 |
//! | 移位计数 < 0 | ❌ | 运行期 panic（Go 规范） |
//! | 浮点 `+ - * /` 与比较 | ✅ | 与运行期同一个 IEEE 语义（除零得 `±Inf`，不 panic） |
//! | 字符串 `+` 与比较 | ✅ | |
//! | 复数 | ❌ | M6 的复数折叠没有判据，宁可不折 |
//! | `Convert` 浮点→整数 | ❌ | Go 说「实现定义」，折了就是替实现做决定 |
//! | `Convert` 整数↔整数 / 整数→浮点 / 浮点→float32 | ✅ | 语义精确（截断到宽度 / 向偶数舍入由 `as` 保证） |
//!
//! # 为什么「不折」比「折错」重要
//!
//! 折叠是**语义保持**变换：折错一个值，程序就在没有任何报错的情况下算错。
//! 所以边界一律取保守侧 —— 少折一点只是慢，折错一点就是错。
//!
//! # `If` 常量条件
//!
//! `if true` 改成 `Goto` 之后，另一条边指向的块会变成不可达 ⇒ `V8` 立刻红。
//! 所以这条变换**必须**跟一次死块清理（[`super::prune_dead_blocks`]）。

use std::collections::BTreeMap;

use rgoc_const::BasicId;
use rgoc_hir::{BinOp, UnOp, types::ConstVal};
use rgoc_types::TypeTable;

use crate::ir::{Instr, SsaFunc, Term, ValueId};

use super::{compact_values, prune_dead_blocks};

/// 折到不动点（链式常量：`(1+2)+3`）。
const ROUNDS: usize = 8;

/// 跑一遍常量折叠。
pub fn run(f: &mut SsaFunc, tt: &TypeTable) {
    for _ in 0..ROUNDS {
        let consts = const_map(f);
        let mut changed = false;
        for b in &mut f.blocks {
            for ins in &mut b.instrs {
                if let Some((c, typ)) = try_fold(ins, &consts, tt) {
                    let Some(dst) = dst_of(ins) else { continue };
                    *ins = Instr::Const { dst, c, typ };
                    changed = true;
                }
            }
        }
        // `If` 常量条件 ⇒ `Goto`（另一条边随后会被 prune 掉）
        for b in &mut f.blocks {
            if let Term::If { cond, then_, else_ } = &b.term
                && let Some(ConstVal::Bool(c)) = consts.get(cond)
            {
                let (to, args) = if *c { then_.clone() } else { else_.clone() };
                b.term = Term::Goto { to, args };
                changed = true;
            }
        }
        if prune_dead_blocks(f) {
            // 死块带走的值没有定义点了 ⇒ 必须压实（`V1` 要求每个值都有定义）
            compact_values(f);
            changed = true;
        }
        if !changed {
            return;
        }
    }
}

/// 每个 `Const` 指令的 `dst → 值`。
fn const_map(f: &SsaFunc) -> BTreeMap<ValueId, ConstVal> {
    let mut m = BTreeMap::new();
    for b in &f.blocks {
        for i in &b.instrs {
            if let Instr::Const { dst, c, .. } = i {
                m.insert(*dst, c.clone());
            }
        }
    }
    m
}

fn dst_of(i: &Instr) -> Option<ValueId> {
    match i {
        // 断言的主结果（`ok` 不进 fold：它没有可折叠的运算）
        Instr::TypeAssert { dst, .. } => Some(*dst),
        Instr::FuncAddr { dst, .. } | Instr::CapturePtr { dst, .. } => Some(*dst),
        // `defer` 没有 dst。`try_fold` 永远不会折它，所以这个返回值没人用 ——
        // 给 `fv` 只是为了让 match 穷尽。
        Instr::Defer { fv } => Some(*fv),
        // 同上：`nil-check` 没有 dst，`try_fold` 也不折它
        Instr::NilCheck { addr } => Some(*addr),
        Instr::MakeClosure { dst, .. } => Some(*dst),
        Instr::Const { dst, .. }
        | Instr::Copy { dst, .. }
        | Instr::Bin { dst, .. }
        | Instr::Un { dst, .. }
        | Instr::Convert { dst, .. }
        | Instr::AddrOf { dst, .. }
        | Instr::Field { dst, .. }
        | Instr::Index { dst, .. }
        | Instr::Slice { dst, .. }
        | Instr::Composite { dst, .. }
        | Instr::Make { dst, .. }
        | Instr::Load { dst, .. }
        | Instr::Store { dst, .. } => Some(*dst),
        Instr::Call { dsts, .. } => dsts.first().copied(),
        // `go` 没有可折的 dst —— `try_fold` 永不折它
        Instr::Go { .. } => None,
    }
}

/// 折一条指令 ⇒ `(常量, 结果类型)`。
///
/// 类型**取自被折的那条指令**（`Bin.typ` / `Un.typ` / `Convert.typ`）：折叠只换
/// 「这条指令算什么」，不换类型。`Const.typ` 只给 dump 看，真类型在 `value_types`。
fn try_fold(
    i: &Instr,
    consts: &BTreeMap<ValueId, ConstVal>,
    tt: &TypeTable,
) -> Option<(ConstVal, rgoc_types::TypeId)> {
    match i {
        Instr::Bin {
            op, lhs, rhs, typ, ..
        } => fold_bin(*op, consts.get(lhs)?, consts.get(rhs)?, *typ, tt).map(|c| (c, *typ)),
        Instr::Un { op, x, typ, .. } => fold_un(*op, consts.get(x)?, *typ, tt).map(|c| (c, *typ)),
        Instr::Convert { x, typ, .. } => {
            let from = consts.get(x)?;
            // `Convert` 没有源类型字段 ⇒ 用**常量自己的形状**推（Int/Float/Str/Bool）
            fold_convert(from, *typ, tt).map(|c| (c, *typ))
        }
        _ => None,
    }
}

/// 基本类型（走 `underlying`：具名类型也要认）。
fn basic(tt: &TypeTable, t: rgoc_types::TypeId) -> Option<BasicId> {
    if t.index() >= tt.len() {
        return None;
    }
    match tt.type_of(rgoc_types::underlying(tt, t)) {
        rgoc_types::typ::Type::Basic(b) => Some(*b),
        _ => None,
    }
}

/// 整数类型的 `(位数, 有符号)`。
fn int_shape(b: BasicId) -> Option<(u32, bool)> {
    Some(match b {
        BasicId::Int8 => (8, true),
        BasicId::Int16 => (16, true),
        BasicId::Int32 => (32, true),
        BasicId::Int64 => (64, true),
        BasicId::Int => (64, true),
        BasicId::Uint8 => (8, false),
        BasicId::Uint16 => (16, false),
        BasicId::Uint32 => (32, false),
        BasicId::Uint64 => (64, false),
        BasicId::Uint => (64, false),
        BasicId::Uintptr => (64, false),
        // 未定型常量没有宽度上限（Go 的「任意精度」在 SSA 层退化成 i128）
        BasicId::UntypedInt | BasicId::UntypedRune => (128, true),
        _ => return None,
    })
}

/// 按 Go 的宽度规则回绕（`int8(127)+1 == -128`）。
fn wrap(bits: u32, signed: bool, v: i128) -> i128 {
    if bits >= 128 {
        return v;
    }
    let mask = (1i128 << bits) - 1;
    let r = v & mask;
    if signed && (r >> (bits - 1)) & 1 == 1 {
        r - (1i128 << bits)
    } else {
        r
    }
}

fn fold_bin(
    op: BinOp,
    x: &ConstVal,
    y: &ConstVal,
    typ: rgoc_types::TypeId,
    tt: &TypeTable,
) -> Option<ConstVal> {
    // ① 比较：结果类型是 `bool`，**不能**拿它去查整数宽度
    if matches!(
        op,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
    ) {
        return fold_cmp(op, x, y);
    }
    // ② 算术 / 位运算：两个操作数与结果同型
    match (x, y) {
        (ConstVal::Int(a), ConstVal::Int(b)) => {
            let (bits, signed) = int_shape(basic(tt, typ)?)?;
            let r = match op {
                BinOp::Add => a.wrapping_add(*b),
                BinOp::Sub => a.wrapping_sub(*b),
                BinOp::Mul => a.wrapping_mul(*b),
                BinOp::Div => {
                    if *b == 0 || (*a == i128::MIN && *b == -1) {
                        return None;
                    }
                    a.wrapping_div(*b)
                }
                BinOp::Rem => {
                    if *b == 0 || (*a == i128::MIN && *b == -1) {
                        return None;
                    }
                    a.wrapping_rem(*b)
                }
                BinOp::And => a & b,
                BinOp::Or => a | b,
                BinOp::Xor => a ^ b,
                BinOp::AndNot => a & !b,
                // 负计数在 Go 里是运行期 panic；≥128 的计数在 i128 上没法直接移
                BinOp::Shl | BinOp::Shr => {
                    if *b < 0 || *b >= 128 {
                        return None;
                    }
                    let n = u32::try_from(*b).ok()?;
                    if op == BinOp::Shl {
                        a.wrapping_shl(n)
                    } else {
                        a.wrapping_shr(n)
                    }
                }
                // 比较已在 ① 处理；短路是控制流（`Task 33b`），不该以 `Bin` 出现
                BinOp::Eq
                | BinOp::Ne
                | BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge
                | BinOp::LAnd
                | BinOp::LOr => return None,
            };
            Some(ConstVal::Int(wrap(bits, signed, r)))
        }
        (ConstVal::Float(a), ConstVal::Float(b)) => match op {
            BinOp::Add => Some(ConstVal::Float(a + b)),
            BinOp::Sub => Some(ConstVal::Float(a - b)),
            BinOp::Mul => Some(ConstVal::Float(a * b)),
            BinOp::Div => Some(ConstVal::Float(a / b)),
            _ => None,
        },
        (ConstVal::Str(a), ConstVal::Str(b)) if op == BinOp::Add => {
            Some(ConstVal::Str(format!("{a}{b}")))
        }
        _ => None,
    }
}

/// 比较（结果一定是 `bool`）。
fn fold_cmp(op: BinOp, x: &ConstVal, y: &ConstVal) -> Option<ConstVal> {
    let ord = match (x, y) {
        (ConstVal::Int(a), ConstVal::Int(b)) => a.cmp(b),
        (ConstVal::Str(a), ConstVal::Str(b)) => a.cmp(b),
        (ConstVal::Bool(a), ConstVal::Bool(b)) => match op {
            BinOp::Eq => return Some(ConstVal::Bool(a == b)),
            BinOp::Ne => return Some(ConstVal::Bool(a != b)),
            _ => return None,
        },
        // 浮点没有全序（NaN）：逐条算，不能走 `cmp`
        (ConstVal::Float(a), ConstVal::Float(b)) => {
            return Some(ConstVal::Bool(match op {
                BinOp::Eq => a == b,
                BinOp::Ne => a != b,
                BinOp::Lt => a < b,
                BinOp::Le => a <= b,
                BinOp::Gt => a > b,
                BinOp::Ge => a >= b,
                _ => return None,
            }));
        }
        _ => return None,
    };
    Some(ConstVal::Bool(match op {
        BinOp::Eq => ord.is_eq(),
        BinOp::Ne => ord.is_ne(),
        BinOp::Lt => ord.is_lt(),
        BinOp::Le => ord.is_le(),
        BinOp::Gt => ord.is_gt(),
        BinOp::Ge => ord.is_ge(),
        _ => return None,
    }))
}

fn fold_un(op: UnOp, x: &ConstVal, typ: rgoc_types::TypeId, tt: &TypeTable) -> Option<ConstVal> {
    match (op, x) {
        (UnOp::Pos, v) => Some(v.clone()),
        (UnOp::Neg, ConstVal::Int(a)) => {
            let (bits, signed) = int_shape(basic(tt, typ)?)?;
            Some(ConstVal::Int(wrap(bits, signed, a.wrapping_neg())))
        }
        (UnOp::Neg, ConstVal::Float(a)) => Some(ConstVal::Float(-a)),
        (UnOp::Not, ConstVal::Bool(b)) => Some(ConstVal::Bool(!b)),
        (UnOp::BitNot, ConstVal::Int(a)) => {
            let (bits, signed) = int_shape(basic(tt, typ)?)?;
            Some(ConstVal::Int(wrap(bits, signed, !a)))
        }
        _ => None,
    }
}

fn fold_convert(x: &ConstVal, to: rgoc_types::TypeId, tt: &TypeTable) -> Option<ConstVal> {
    let target = basic(tt, to)?;
    match x {
        ConstVal::Int(v) => {
            if let Some((bits, signed)) = int_shape(target) {
                return Some(ConstVal::Int(wrap(bits, signed, *v)));
            }
            match target {
                BasicId::Float64 => Some(ConstVal::Float(*v as f64)),
                // `as f32` 就是「向偶数舍入到 f32」——与 Go 的转换同义
                BasicId::Float32 => Some(ConstVal::Float(f64::from(*v as f32))),
                _ => None,
            }
        }
        ConstVal::Float(v) => match target {
            BasicId::Float64 => Some(ConstVal::Float(*v)),
            BasicId::Float32 => Some(ConstVal::Float(f64::from(*v as f32))),
            // 浮点 → 整数是**实现定义**的（Go 规范），不折
            _ => None,
        },
        ConstVal::Bool(b) if target == BasicId::Bool || target == BasicId::UntypedBool => {
            Some(ConstVal::Bool(*b))
        }
        ConstVal::Str(s) if target == BasicId::String || target == BasicId::UntypedString => {
            Some(ConstVal::Str(s.clone()))
        }
        _ => None,
    }
}
