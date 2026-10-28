//! 算术、比较、移位（宽度 / 回绕 / 除零）。
//!
//! # 三条 Go 语义的落点
//!
//! 1. **宽度掩码**：`int8(127) + 1 == -128`。槽里是 `i128`，每次运算后按静态类型
//!    的宽度截断并做符号扩展（[`normalize`]）。
//! 2. **移位计数**：`x << 64` 在 `uint64` 上是 **0**（不是「未定义」）。计数 ≥ 宽度
//!    时：左移/无符号右移得 0，有符号右移得 `0` 或 `-1`（符号填充）。
//! 3. **除零是 panic**（Go 语义），不是宿主 panic —— 由 [`RtError`] 转成
//!    `panic: runtime error: integer divide by zero` + 退出码 2。

use rgoc_const::BasicId;
use rgoc_types::{TypeId, TypeTable};

use crate::value::{Value, int_shape, is_complex, is_float, is_float32, is_string};

/// 运行时错误（都会转成 Go 的 panic 文本）。
#[derive(Clone, Debug, PartialEq)]
pub enum RtError {
    /// 整数除零 / 模零。
    DivideByZero,
    /// 索引越界。
    IndexOutOfRange {
        /// 下标。
        index: i128,
        /// 长度。
        len: i128,
    },
    /// 切片越界。
    SliceBounds {
        /// 哪一个界（`low` / `high` / `max`）。
        what: &'static str,
        /// 值。
        val: i128,
        /// 上界。
        max: i128,
    },
    /// nil 指针解引用。
    NilDeref,
    /// 向 nil map 写入。
    NilMapWrite,
    /// 类型断言失败。
    AssertFail(String),
    /// 其它（消息已经是 Go 的 panic 文本）。
    Message(String),
}

impl RtError {
    /// Go 的 panic 文本（**不含** `panic: ` 前缀）。
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            RtError::DivideByZero => "runtime error: integer divide by zero".to_string(),
            RtError::IndexOutOfRange { index, len } => {
                format!("runtime error: index out of range [{index}] with length {len}")
            }
            RtError::SliceBounds { what, val, max } => format!(
                "runtime error: slice bounds out of range [{what}:{val}] with capacity {max}"
            ),
            RtError::NilDeref => {
                "runtime error: invalid memory address or nil pointer dereference".to_string()
            }
            RtError::NilMapWrite => "assignment to entry in nil map".to_string(),
            RtError::AssertFail(m) => m.clone(),
            RtError::Message(m) => m.clone(),
        }
    }
}

/// 按类型的宽度规范化一个整数。
#[must_use]
pub fn normalize(v: i128, b: BasicId) -> i128 {
    match int_shape(b) {
        Some((8, true)) => i128::from(v as i8),
        Some((16, true)) => i128::from(v as i16),
        Some((32, true)) => i128::from(v as i32),
        Some((64, true)) => i128::from(v as i64),
        Some((8, false)) => i128::from(v as u8),
        Some((16, false)) => i128::from(v as u16),
        Some((32, false)) => i128::from(v as u32),
        Some((64, false)) => i128::from(v as u64),
        _ => v,
    }
}

/// 取类型的 `BasicId`（穿透命名/别名；非基本型 ⇒ `None`）。
#[must_use]
pub fn basic_of(tt: &TypeTable, t: TypeId) -> Option<BasicId> {
    match tt.type_of(rgoc_types::underlying(tt, rgoc_types::unalias(tt, t))) {
        rgoc_types::Type::Basic(b) => Some(*b),
        _ => None,
    }
}

/// 二元算术（`+ - * / % & | ^ &^ << >>`）。
///
/// # Errors
///
/// 除零 / 模零 ⇒ [`RtError::DivideByZero`]。
pub fn bin_arith(
    op: rgoc_hir::BinOp,
    x: &Value,
    y: &Value,
    typ: TypeId,
    tt: &TypeTable,
) -> Result<Value, RtError> {
    use rgoc_hir::BinOp as B;
    // 字符串拼接
    if is_string(tt, typ)
        && op == B::Add
        && let (Value::Str(a), Value::Str(b)) = (x, y)
    {
        let mut s = String::with_capacity(a.len() + b.len());
        s.push_str(a);
        s.push_str(b);
        return Ok(Value::Str(s.into()));
    }
    if is_complex(tt, typ) {
        let (ar, ai) = as_complex(x);
        let (br, bi) = as_complex(y);
        // `complex64` 的每一步都要按 f32 舍入 —— 否则与检查器折叠出的
        // `complex64` 常量对不上（`ken/cplx2.go` 的 `ce != Ce` 实测：
        // 两侧打印一样，但低位不同）。
        let c64 = basic_of(tt, typ) == Some(BasicId::Complex64);
        let r = |v: f64| {
            if c64 { f64::from(v as f32) } else { v }
        };
        return Ok(match op {
            B::Add => Value::Complex(r(ar + br), r(ai + bi)),
            B::Sub => Value::Complex(r(ar - br), r(ai - bi)),
            B::Mul => Value::Complex(r(r(ar * br) - r(ai * bi)), r(r(ar * bi) + r(ai * br))),
            B::Div => {
                let den = r(r(br * br) + r(bi * bi));
                if den == 0.0 {
                    return Err(RtError::DivideByZero);
                }
                Value::Complex(
                    r(r(r(ar * br) + r(ai * bi)) / den),
                    r(r(r(ai * br) - r(ar * bi)) / den),
                )
            }
            _ => Value::Complex(ar, ai),
        });
    }
    if is_float(tt, typ) {
        let a = as_float(x);
        let b = as_float(y);
        let r = match op {
            B::Add => a + b,
            B::Sub => a - b,
            B::Mul => a * b,
            B::Div => {
                // Go 的浮点除零是 `±Inf` / `NaN`，**不是** panic。
                a / b
            }
            _ => a,
        };
        return Ok(Value::Float(if is_float32(tt, typ) {
            f64::from(r as f32)
        } else {
            r
        }));
    }
    let a = as_int(x);
    let b = as_int(y);
    let bkind = basic_of(tt, typ).unwrap_or(BasicId::Int);
    let r = match op {
        B::Add => a.wrapping_add(b),
        B::Sub => a.wrapping_sub(b),
        B::Mul => a.wrapping_mul(b),
        B::Div => {
            if b == 0 {
                return Err(RtError::DivideByZero);
            }
            a.wrapping_div(b)
        }
        B::Rem => {
            if b == 0 {
                return Err(RtError::DivideByZero);
            }
            a.wrapping_rem(b)
        }
        B::And => a & b,
        B::Or => a | b,
        B::Xor => a ^ b,
        B::AndNot => a & !b,
        B::Shl => shift_left(a, b, bkind),
        B::Shr => shift_right(a, b, bkind),
        _ => a,
    };
    Ok(Value::Int(normalize(r, bkind)))
}

fn shift_left(a: i128, b: i128, k: BasicId) -> i128 {
    let width = int_shape(k).map_or(64, |(w, _)| w);
    if b < 0 || b as u128 >= u128::from(width) {
        return 0;
    }
    a << (b as u32)
}

fn shift_right(a: i128, b: i128, k: BasicId) -> i128 {
    let width = int_shape(k).map_or(64, |(w, _)| w);
    let signed = int_shape(k).is_none_or(|(_, s)| s);
    if b < 0 || b as u128 >= u128::from(width) {
        return if signed && a < 0 { -1 } else { 0 };
    }
    // 无符号右移：先把值当作无符号宽度域的数
    if signed {
        a >> (b as u32)
    } else {
        let ua = normalize(a, k);
        (ua as u128 >> (b as u32)) as i128
    }
}

/// 比较（`== != < <= > >=`）；不可比 ⇒ `None`（调用方按 false 处理）。
#[must_use]
pub fn compare(op: rgoc_hir::BinOp, x: &Value, y: &Value, tt: &TypeTable) -> Option<bool> {
    use rgoc_hir::BinOp as B;
    match op {
        // 比较走**类型表感知**的入口：接口动态类型按 `identical` 判等
        // （`go_eq` 的裸 `TypeId` 相等会被重复的基本型条目骗过）。
        B::Eq => Some(crate::value::go_eq_tt(tt, x, y)),
        B::Ne => Some(!crate::value::go_eq_tt(tt, x, y)),
        _ => {
            let ord = match (x, y) {
                (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
                (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
                (Value::Str(a), Value::Str(b)) => Some(a.as_bytes().cmp(b.as_bytes())),
                _ => None,
            }?;
            Some(match op {
                B::Lt => ord == std::cmp::Ordering::Less,
                B::Le => ord != std::cmp::Ordering::Greater,
                B::Gt => ord == std::cmp::Ordering::Greater,
                B::Ge => ord != std::cmp::Ordering::Less,
                _ => false,
            })
        }
    }
}

fn as_int(v: &Value) -> i128 {
    match v {
        Value::Int(i) => *i,
        Value::Float(f) => *f as i128,
        _ => 0,
    }
}

fn as_float(v: &Value) -> f64 {
    match v {
        Value::Float(f) => *f,
        Value::Int(i) => *i as f64,
        _ => 0.0,
    }
}

fn as_complex(v: &Value) -> (f64, f64) {
    match v {
        Value::Complex(r, i) => (*r, *i),
        Value::Float(f) => (*f, 0.0),
        Value::Int(i) => (*i as f64, 0.0),
        _ => (0.0, 0.0),
    }
}
