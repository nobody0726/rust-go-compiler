//! 常量折叠：算术 / 比较 / 逻辑 / 按位（M3/Task 6–7）。
//!
//! # 「贪心地算，拒绝猜测」
//!
//! 折叠器**只**在「两侧都是常量」时被调用（这一点由调用方保证）。对每个算子：
//!
//! - 语义**定义清楚** ⇒ 算出精确结果；
//! - 语义**不成立**（类型不匹配、除零、对浮点取余、对复数做 `<`）⇒
//!   [`Const::Unknown`]。**绝不**猜一个近似值。
//!
//! 为什么这样切：`T-TY-47` 要求「溢出后常量仍是**精确原值**」，而
//! `M3-report`/`M3-design` 的 `D-M3-11` 要求「未实现走稳定诊断码」。
//! 折叠器本身没有诊断通道（它只知道值），所以它只做一件事：
//! **要么给出确定的值，要么说「我这里没有值」**。诊断由调用方（Task 11 检查器 /
//! Task 12 求值器）根据 `Unknown` 补上 —— 那里知道位置与语境。
//!
//! # 无类型常量的「种类提升」
//!
//! 照 spec「Constant expressions」：两个**不同种类**的无类型常量做二元运算，
//! 结果取两者中**靠后**的种类，顺序是
//! `整数 → rune → 浮点 → 复数`。
//!
//! 本层用 `Const` 的形状表达这件事：`Int`（整数/rune）、`Float`（浮点）、
//! `Complex`（复数）。**rune 与整数的区分不在这里** —— 它是**类型**层的属性
//! （无类型常量有 `untyped rune` 这个类型），见 `D-M3-3` 与 Task 8 的记录。
//!
//! # 复数的「不降级」
//!
//! `Const::Complex { im: 0 }` 与 `Const::Float` 在**值**上相等，但**种类**不同
//! （`complex(1,0)` 的默认类型是 `complex128`，`1.0` 是 `float64`）。
//! 所以只要有任一侧是 `Complex`，结果就保持 `Complex`。

use std::cmp::Ordering;

use rgoc_ast::Op;
use rgoc_lex::DiagCode;

use crate::basic::BasicId;
use crate::bigint::{BigInt, BitOp};
use crate::bigrat::BigRat;
use crate::sizes::Sizes;
use crate::value::Const;

/// 常量移位计数的上界（**与官方 `go/types` 同界**，见 [`shift_ok`]）。
pub const SHIFT_BOUND: u32 = 1023 - 1 + 52;

/// 移位计数的合法性检查；合法则给出计数。
///
/// # 三条规则全部来自**实测 oracle**，不是推断（`go/types/expr.go:644-700`）
///
/// | 输入 | 官方行为 |
/// |---|---|
/// | `1 << -1` | 拒绝 `InvalidShiftCount`（negative shift count） |
/// | `1 << 2.0` | **接受**，`== 4`（无类型浮点，值是整数 ⇒ 可表示成 `uint`） |
/// | `1 << 1.5` | 拒绝 |
/// | `1 << 2i` | 拒绝（"truncated to uint"） |
/// | `1 << 1074` | 接受（`s <= 1074`） |
/// | `1 << 1075` | 拒绝（"invalid shift count"） |
///
/// ⚠️ **偏离 `M3-plan.md` Task 7 的签名**：计划写 `shift_ok(x: &Const)`，
/// 且把检查对象当成**左值**。实测**左值可以是负数**（`-1 << 2 == -4` 合法，
/// `go build` 与 `go/types` 一致），要检查的是**计数**（右操作数）。
/// 返回 `u32` 而非 `()` 是因为移位**需要这个数**；返回 `DiagCode` 是因为
/// 诊断属于调用方，而代号必须唯一（`C1`：按代号比对）。
pub fn shift_ok(count: &Const) -> Result<u32, DiagCode> {
    let invalid = DiagCode::TypInvalidShiftCount;
    let n = as_int_value(count).ok_or(invalid)?;
    if n.is_neg() {
        return Err(invalid);
    }
    let s = n.to_i64().ok_or(invalid)?;
    if s > i64::from(SHIFT_BOUND) {
        return Err(invalid);
    }
    Ok(s as u32)
}

/// 移位**左操作数**是否为整数（官方 `allInteger(typ) || ToInt(val).Kind()==Int`）。
///
/// ⚠️ **负数算「是整数」** —— `-1 << 2` 合法（oracle 实测）。
/// 这里只判「**值**是不是整数」，因为折叠器只看得到无类型常量；
/// 有类型的操作数由 Task 11 的检查器在类型层判。
pub fn shift_operand_ok(x: &Const) -> bool {
    as_int_value(x).is_some()
}

/// 二元运算折叠（算术 / 按位 / 逻辑 / 字符串拼接）。
pub fn fold_binary(op: Op, x: &Const, y: &Const) -> Const {
    // `Unknown` 传染：**不**把「没有值」当成 0 / ""。
    if *x == Const::Unknown || *y == Const::Unknown {
        return Const::Unknown;
    }
    match (x, y) {
        (Const::Str(a), Const::Str(b)) if op == Op::Add => {
            let mut out = Vec::with_capacity(a.len() + b.len());
            out.extend_from_slice(a);
            out.extend_from_slice(b);
            Const::Str(out)
        }
        (Const::Bool(a), Const::Bool(b)) => match op {
            Op::LAnd => Const::Bool(*a && *b),
            Op::LOr => Const::Bool(*a || *b),
            // `true + true` 之类：种类不匹配，折叠器不给值
            _ => Const::Unknown,
        },
        _ => match op {
            Op::And | Op::Or | Op::Xor | Op::AndNot => bitwise(op, x, y),
            Op::Shl | Op::Shr => shift(op, x, y),
            Op::Add | Op::Sub | Op::Mul | Op::Quo | Op::Rem => arithmetic(op, x, y),
            // 比较走 [`fold_compare`]
            _ => Const::Unknown,
        },
    }
}

/// 一元运算折叠（`+x` / `-x` / `!x` / `^x`）。
pub fn fold_unary(op: Op, x: &Const) -> Const {
    if *x == Const::Unknown {
        return Const::Unknown;
    }
    match op {
        Op::Add => x.clone(),
        Op::Sub => negate(x),
        Op::Not => match x {
            Const::Bool(v) => Const::Bool(!v),
            _ => Const::Unknown,
        },
        Op::Xor => match x {
            Const::Int(v) => Const::Int(v.bit_not()),
            _ => Const::Unknown,
        },
        _ => Const::Unknown,
    }
}

/// 带**操作数类型**的一元折叠（M3/Task 30：`^` 不是纯值运算）。
///
/// 只有 `^` 需要它 —— 官方语义**按类型分岔**：
///
/// | 操作数 | 官方语义 | 实测 |
/// |---|---|---|
/// | 有类型**无符号** | 按宽度取补 `m ^ x`（`m` = 该宽度全 1）⇒ **非负** | `^uint32(0)` = 4294967295 |
/// | 有类型**有符号** | 无限精度补码 `-x-1` | `^int8(0)` = -1 |
/// | **无类型** | 同上（无类型常量不分宽度） | `^0` = -1 |
///
/// 前两行的官方判据由 go1.27.1 实测（`const a = ^uint32(0)` 合法，
/// 值 4294967295）。`ty` 是**调用方能确定**的操作数类型：`None` ⇒ 不知道 ⇒
/// 按无类型语义算，**不猜**（本层没有类型表，见 [`crate::eval`] 的模块文档）。
///
/// ⚠️ **已知边界**：`ty` 目前只在「操作数是**转换到预声明基本类型**」时才给得出
/// （[`crate::eval`] 的 `operand_basic_ty`）⇒ `^x` 里 `x` 是**具名**的无符号常量
/// （`const u uint32 = 0; const b = ^u`）时仍按无类型算，值偏 `-x-1`。
/// 那条记在 `M3-manifest.json` 的 `open_gaps`（修它要 `ConstEnv` 报出名字的类型）。
#[must_use]
pub fn fold_unary_typed(op: Op, x: &Const, ty: Option<BasicId>, sizes: &Sizes) -> Const {
    if op == Op::Xor
        && let Some(b) = ty
        && b.is_unsigned()
        && let (Const::Int(v), Some(w)) = (x, sizes.width_of(b))
    {
        // `m - v`：`v` 来自转换到该无符号类型 ⇒ 转换时已判过可表示 `v ∈ [0, m]`
        // ⇒ 结果非负。**不**在值层做「超出宽度就回绕」：那是转换/可表示的事。
        let one = BigInt::from_u64(1);
        let m = one.shl(w).sub(&one);
        return Const::Int(m.sub(v));
    }
    fold_unary(op, x)
}

/// 比较运算折叠（结果恒为 [`Const::Bool`] 或 [`Const::Unknown`]）。
pub fn fold_compare(op: Op, x: &Const, y: &Const) -> Const {
    if *x == Const::Unknown || *y == Const::Unknown {
        return Const::Unknown;
    }
    // 布尔只可 `==` / `!=`（Go 里 `true < false` 不是合法表达式）。
    if let (Const::Bool(a), Const::Bool(b)) = (x, y) {
        return match op {
            Op::Eql => Const::Bool(a == b),
            Op::Neq => Const::Bool(a != b),
            _ => Const::Unknown,
        };
    }
    // 复数只可 `==` / `!=`（无全序）。
    if matches!(x, Const::Complex { .. }) || matches!(y, Const::Complex { .. }) {
        let (Some((ar, ai)), Some((br, bi))) = (as_cplx(x), as_cplx(y)) else {
            return Const::Unknown;
        };
        let eq = ar == br && ai == bi;
        return match op {
            Op::Eql => Const::Bool(eq),
            Op::Neq => Const::Bool(!eq),
            _ => Const::Unknown,
        };
    }
    let ord = match (x, y) {
        (Const::Str(a), Const::Str(b)) => a.cmp(b), // 按字节序，照 Go
        _ => {
            let (Some(a), Some(b)) = (as_rat(x), as_rat(y)) else {
                return Const::Unknown;
            };
            a.cmp(&b)
        }
    };
    match op {
        Op::Eql => Const::Bool(ord == Ordering::Equal),
        Op::Neq => Const::Bool(ord != Ordering::Equal),
        Op::Lss => Const::Bool(ord == Ordering::Less),
        Op::Leq => Const::Bool(ord != Ordering::Greater),
        Op::Gtr => Const::Bool(ord == Ordering::Greater),
        Op::Geq => Const::Bool(ord != Ordering::Less),
        _ => Const::Unknown,
    }
}

// ── 内部 ──────────────────────────────────────────────────────────────────

/// `Const` → 「整数值」（照官方 `constant.ToInt`）。
pub(crate) fn as_int_value(c: &Const) -> Option<BigInt> {
    match c {
        Const::Int(v) => Some(v.clone()),
        Const::Float(v) => v.to_int(),
        // `1+0i` 也是整数（官方 `ToInt` 对有零虚部的复常量成立）
        Const::Complex { re, im } if im.is_zero() => re.to_int(),
        _ => None,
    }
}

/// 移位：**结果种类修正为整数**（`1.0 << 2 == 4`，官方把结果类型改成 `untyped int`）。
fn shift(op: Op, x: &Const, y: &Const) -> Const {
    let Ok(count) = shift_ok(y) else {
        return Const::Unknown;
    };
    let Some(v) = as_int_value(x) else {
        return Const::Unknown;
    };
    match op {
        Op::Shl => Const::Int(v.shl(count)),
        Op::Shr => Const::Int(v.shr(count)),
        _ => Const::Unknown,
    }
}

/// `Const` → 实数有理数（`Int` / `Float`）。
fn as_rat(c: &Const) -> Option<BigRat> {
    match c {
        Const::Int(v) => Some(BigRat::from_bigint(v.clone())),
        Const::Float(v) => Some(v.clone()),
        _ => None,
    }
}

/// `Const` → 复数（`Int` / `Float` 的虚部是 0）。
fn as_cplx(c: &Const) -> Option<(BigRat, BigRat)> {
    let zero = BigRat::from_i64(0);
    match c {
        Const::Int(v) => Some((BigRat::from_bigint(v.clone()), zero)),
        Const::Float(v) => Some((v.clone(), zero)),
        Const::Complex { re, im } => Some((re.clone(), im.clone())),
        _ => None,
    }
}

/// 一元取负（保持种类）。
fn negate(x: &Const) -> Const {
    let zero = BigRat::from_i64(0);
    match x {
        Const::Int(v) => Const::Int(BigInt::from_i64(0).sub(v)),
        Const::Float(v) => Const::Float(zero.sub(v)),
        Const::Complex { re, im } => Const::Complex {
            re: zero.sub(re),
            im: zero.sub(im),
        },
        _ => Const::Unknown,
    }
}

/// 按位运算 —— **只对整数常量成立**（浮点/字符串 ⇒ `Unknown`）。
fn bitwise(op: Op, x: &Const, y: &Const) -> Const {
    let (Const::Int(a), Const::Int(b)) = (x, y) else {
        return Const::Unknown;
    };
    let kind = match op {
        Op::And => BitOp::And,
        Op::Or => BitOp::Or,
        Op::Xor => BitOp::Xor,
        _ => BitOp::AndNot,
    };
    Const::Int(a.bitwise(kind, b))
}

/// 算术 —— 三条路径：复数 / 整数 / 有理数。
fn arithmetic(op: Op, x: &Const, y: &Const) -> Const {
    // ① 复数（任一侧是 `Complex` ⇒ 结果**保持**复数种类，不降级为实数）
    if matches!(x, Const::Complex { .. }) || matches!(y, Const::Complex { .. }) {
        let (Some((ar, ai)), Some((br, bi))) = (as_cplx(x), as_cplx(y)) else {
            return Const::Unknown;
        };
        return match op {
            Op::Add => Const::Complex {
                re: ar.add(&br),
                im: ai.add(&bi),
            },
            Op::Sub => Const::Complex {
                re: ar.sub(&br),
                im: ai.sub(&bi),
            },
            Op::Mul => Const::Complex {
                re: ar.mul(&br).sub(&ai.mul(&bi)),
                im: ar.mul(&bi).add(&ai.mul(&br)),
            },
            // (a+bi)/(c+di) = ((ac+bd) + (bc-ad)i) / (c²+d²)
            Op::Quo => {
                let den = br.mul(&br).add(&bi.mul(&bi));
                if den.is_zero() {
                    return Const::Unknown;
                }
                match (
                    ar.mul(&br).add(&ai.mul(&bi)).div(&den),
                    ai.mul(&br).sub(&ar.mul(&bi)).div(&den),
                ) {
                    (Some(re), Some(im)) => Const::Complex { re, im },
                    _ => Const::Unknown,
                }
            }
            // `%` 与位运算对复数不成立
            _ => Const::Unknown,
        };
    }
    // ② 两侧都是整数 ⇒ 结果是**整数**（`7/2 == 3`、`%` 取被除数符号）
    if let (Const::Int(a), Const::Int(b)) = (x, y) {
        return match op {
            Op::Add => Const::Int(a.add(b)),
            Op::Sub => Const::Int(a.sub(b)),
            Op::Mul => Const::Int(a.mul(b)),
            Op::Quo => match a.div_mod(b) {
                Some((q, _)) => Const::Int(q),
                None => Const::Unknown,
            },
            Op::Rem => match a.div_mod(b) {
                Some((_, r)) => Const::Int(r),
                None => Const::Unknown,
            },
            _ => Const::Unknown,
        };
    }
    // ③ 其余数值组合（含 `Int`/`Float` 混合）⇒ 结果种类是**浮点**，且**精确**
    let (Some(a), Some(b)) = (as_rat(x), as_rat(y)) else {
        return Const::Unknown;
    };
    match op {
        Op::Add => Const::Float(a.add(&b)),
        Op::Sub => Const::Float(a.sub(&b)),
        Op::Mul => Const::Float(a.mul(&b)),
        Op::Quo => a.div(&b).map_or(Const::Unknown, Const::Float),
        // 浮点取余不是 Go 的运算
        Op::Rem => Const::Unknown,
        _ => Const::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i(v: i64) -> Const {
        Const::Int(BigInt::from_i64(v))
    }

    fn f(n: i64, d: i64) -> Const {
        Const::Float(BigRat::new(BigInt::from_i64(n), BigInt::from_i64(d)).unwrap())
    }

    fn s(v: &str) -> Const {
        Const::Str(v.as_bytes().to_vec())
    }

    fn b(v: bool) -> Const {
        Const::Bool(v)
    }

    /// `T-TY-35`：整数除法截断、`%` 取**被除数**符号。
    #[test]
    fn 整数算术_截断与余数符号() {
        assert_eq!(fold_binary(Op::Quo, &i(7), &i(2)), i(3));
        assert_eq!(fold_binary(Op::Quo, &i(-7), &i(2)), i(-3));
        assert_eq!(fold_binary(Op::Rem, &i(-7), &i(2)), i(-1));
        assert_eq!(fold_binary(Op::Rem, &i(7), &i(-2)), i(1));
        assert_eq!(fold_binary(Op::Add, &i(3), &i(4)), i(7));
        assert_eq!(fold_binary(Op::Sub, &i(3), &i(4)), i(-1));
        assert_eq!(fold_binary(Op::Mul, &i(3), &i(4)), i(12));
    }

    /// `T-TY-35`：有理数精确 —— `1.5 * 2` 是**精确的** `3`，不是 `3.0000000000000004`。
    #[test]
    fn 浮点算术是精确的() {
        let three = f(3, 1);
        assert_eq!(fold_binary(Op::Mul, &f(3, 2), &i(2)), three);
        // `Int` 与 `Float` 混合 ⇒ 结果种类是**浮点**
        assert_eq!(fold_binary(Op::Add, &i(1), &f(1, 2)), f(3, 2));
        assert_eq!(fold_binary(Op::Quo, &f(1, 1), &i(3)), f(1, 3));
    }

    /// **不引入实现限制**（`D-M3-4`）：`1e400 * 1e400` 仍是精确有理数。
    #[test]
    fn 浮点不溢出为无穷() {
        let e400 = BigRat::from_dec("1e400").unwrap();
        let got = fold_binary(
            Op::Mul,
            &Const::Float(e400.clone()),
            &Const::Float(e400.clone()),
        );
        assert_eq!(got, Const::Float(e400.mul(&e400)));
        match got {
            Const::Float(v) => assert_eq!(v.num().to_string().len(), 801, "1e800 有 801 位"),
            other => panic!("应为 Float，实得 {other:?}"),
        }
    }

    /// 复数：算术保持复种类，`<` 之类不成立 ⇒ `Unknown`。
    #[test]
    fn 复数算术与非法比较() {
        let c = |re: i64, im: i64| Const::Complex {
            re: BigRat::from_i64(re),
            im: BigRat::from_i64(im),
        };
        assert_eq!(
            fold_binary(Op::Mul, &c(0, 1), &c(0, 1)),
            c(-1, 0),
            "(0+1i)² == -1"
        );
        assert_eq!(fold_binary(Op::Add, &c(1, 2), &i(3)), c(4, 2));
        assert_eq!(
            fold_binary(Op::Quo, &i(1), &c(0, 0)),
            Const::Unknown,
            "除零"
        );
        assert_eq!(fold_compare(Op::Lss, &c(0, 1), &i(0)), Const::Unknown);
        assert_eq!(fold_compare(Op::Eql, &c(1, 0), &i(1)), b(true));
        assert_eq!(fold_compare(Op::Eql, &c(1, 1), &i(1)), b(false));
    }

    /// 字符串拼接；`Str` 是字节，**不**看 UTF-8。
    #[test]
    fn 字符串拼接() {
        assert_eq!(fold_binary(Op::Add, &s("a"), &s("b")), s("ab"));
        assert_eq!(fold_binary(Op::Add, &s(""), &s("x")), s("x"));
        let raw = Const::Str(vec![0xff, 0x00]);
        assert_eq!(
            fold_binary(Op::Add, &raw, &s("z")),
            Const::Str(vec![0xff, 0x00, b'z'])
        );
    }

    /// 逻辑运算；`!` 只对布尔成立。
    #[test]
    fn 逻辑运算() {
        assert_eq!(fold_binary(Op::LAnd, &b(true), &b(false)), b(false));
        assert_eq!(fold_binary(Op::LOr, &b(true), &b(false)), b(true));
        assert_eq!(fold_unary(Op::Not, &b(true)), b(false));
        assert_eq!(fold_unary(Op::Not, &i(1)), Const::Unknown);
        assert_eq!(fold_binary(Op::Add, &b(true), &b(true)), Const::Unknown);
    }

    /// `T-TY-36`：比较 —— 跨种类（`Int` vs `Float`）也能比。
    #[test]
    fn 比较() {
        assert_eq!(fold_compare(Op::Lss, &i(1), &f(2, 1)), b(true));
        assert_eq!(fold_compare(Op::Lss, &f(3, 2), &i(2)), b(true));
        assert_eq!(fold_compare(Op::Eql, &i(1), &f(1, 1)), b(true));
        assert_eq!(fold_compare(Op::Neq, &i(1), &f(1, 2)), b(true));
        assert_eq!(fold_compare(Op::Gtr, &i(2), &i(1)), b(true));
        assert_eq!(fold_compare(Op::Geq, &i(2), &i(2)), b(true));
        assert_eq!(fold_compare(Op::Leq, &i(2), &i(3)), b(true));
        assert_eq!(fold_compare(Op::Eql, &s("a"), &s("a")), b(true));
        assert_eq!(fold_compare(Op::Lss, &s("a"), &s("b")), b(true));
        assert_eq!(fold_compare(Op::Eql, &b(true), &b(true)), b(true));
        assert_eq!(fold_compare(Op::Lss, &b(false), &b(true)), Const::Unknown);
        // 跨种类**不成立**：数字与字符串不可比
        assert_eq!(fold_compare(Op::Eql, &i(1), &s("1")), Const::Unknown);
    }

    /// 一元：`-x` 保持种类；`^x` 只对整数成立。
    #[test]
    fn 一元运算() {
        assert_eq!(fold_unary(Op::Sub, &i(5)), i(-5));
        assert_eq!(fold_unary(Op::Sub, &f(1, 2)), f(-1, 2));
        assert_eq!(fold_unary(Op::Add, &i(5)), i(5));
        assert_eq!(fold_unary(Op::Xor, &i(0)), i(-1));
        assert_eq!(fold_unary(Op::Xor, &f(1, 1)), Const::Unknown);
        assert_eq!(
            fold_unary(
                Op::Sub,
                &Const::Complex {
                    re: BigRat::from_i64(1),
                    im: BigRat::from_i64(2)
                }
            ),
            Const::Complex {
                re: BigRat::from_i64(-1),
                im: BigRat::from_i64(-2)
            }
        );
    }

    /// `^` 的**类型依赖**（M3/Task 30）：无符号**有类型** ⇒ 按宽度取补（非负）；
    /// 有符号 / 无类型 / **不知道类型** ⇒ 无限精度补码 `-x-1`。
    ///
    /// 判据来自 go1.27.1 实测：`^uint32(0)` 合法且 = 4294967295。
    #[test]
    fn 一元补_按操作数类型分岔() {
        let s = Sizes::HOST_ARM64;
        // 无符号有类型：`m - x`，宽度随类型
        assert_eq!(
            fold_unary_typed(Op::Xor, &i(0), Some(BasicId::Uintptr), &s),
            Const::Int(BigInt::from_u64(u64::MAX))
        );
        assert_eq!(
            fold_unary_typed(Op::Xor, &i(0), Some(BasicId::Uint32), &s),
            Const::Int(BigInt::from_u64(0xFFFF_FFFF))
        );
        assert_eq!(
            fold_unary_typed(Op::Xor, &i(1), Some(BasicId::Uint8), &s),
            Const::Int(BigInt::from_u64(0xFE))
        );
        // 有符号 / 无类型 / 不知道 ⇒ `-x-1`，与 [`fold_unary`] 一致
        assert_eq!(
            fold_unary_typed(Op::Xor, &i(0), Some(BasicId::Int64), &s),
            i(-1)
        );
        assert_eq!(
            fold_unary_typed(Op::Xor, &i(0), Some(BasicId::UntypedInt), &s),
            i(-1)
        );
        assert_eq!(fold_unary_typed(Op::Xor, &i(0), None, &s), i(-1));
        // 非 `^` 的一元运算与 [`fold_unary`] 逐字相同
        assert_eq!(
            fold_unary_typed(Op::Sub, &i(5), Some(BasicId::Uint32), &s),
            fold_unary(Op::Sub, &i(5))
        );
        // 种类不匹配仍然是 `Unknown`（不因为给了类型就乱给值）
        assert_eq!(
            fold_unary_typed(Op::Xor, &Const::Bool(true), Some(BasicId::Uint32), &s),
            Const::Unknown
        );
        assert_eq!(
            fold_unary_typed(Op::Xor, &Const::Unknown, Some(BasicId::Uint32), &s),
            Const::Unknown
        );
    }

    /// 除零 ⇒ `Unknown`（诊断归调用方）。
    #[test]
    fn 除零是_unknown() {
        assert_eq!(fold_binary(Op::Quo, &i(1), &i(0)), Const::Unknown);
        assert_eq!(fold_binary(Op::Rem, &i(1), &i(0)), Const::Unknown);
        assert_eq!(fold_binary(Op::Quo, &f(1, 1), &f(0, 1)), Const::Unknown);
    }

    /// 按位（无限精度补码）。
    #[test]
    fn 按位运算() {
        assert_eq!(fold_binary(Op::And, &i(-1), &i(5)), i(5));
        assert_eq!(fold_binary(Op::Or, &i(-5), &i(3)), i(-5));
        assert_eq!(fold_binary(Op::Xor, &i(5), &i(3)), i(6));
        assert_eq!(fold_binary(Op::AndNot, &i(5), &i(3)), i(4));
        assert_eq!(
            fold_binary(Op::And, &f(1, 1), &i(1)),
            Const::Unknown,
            "浮点不可按位"
        );
    }

    /// `Unknown` 传染：任何一侧无值，结果就无值（**不**静默当成 0）。
    #[test]
    fn unknown_传染() {
        assert_eq!(fold_binary(Op::Add, &Const::Unknown, &i(1)), Const::Unknown);
        assert_eq!(fold_binary(Op::Add, &i(1), &Const::Unknown), Const::Unknown);
        assert_eq!(fold_unary(Op::Sub, &Const::Unknown), Const::Unknown);
        assert_eq!(
            fold_compare(Op::Eql, &Const::Unknown, &i(0)),
            Const::Unknown
        );
    }

    /// `T-TY-37`：常量移位 —— `1 << 100` **精确**（不是 0、不 panic）。
    #[test]
    fn 移位_长移位精确() {
        assert_eq!(
            fold_binary(Op::Shl, &i(1), &i(100)),
            Const::Int(BigInt::from_i64(1).shl(100))
        );
        // 2^1000 的十进制有 302 位 ⇒ 确实没有截断
        match fold_binary(Op::Shl, &i(1), &i(1000)) {
            Const::Int(v) => assert_eq!(v.to_string().len(), 302),
            other => panic!("应为 Int，实得 {other:?}"),
        }
        assert_eq!(fold_binary(Op::Shl, &i(1), &i(0)), i(1));
        assert_eq!(fold_binary(Op::Shr, &i(1024), &i(10)), i(1));
        // 算术右移（下取整）：oracle 实测 `-7 >> 1 == -4`
        assert_eq!(fold_binary(Op::Shr, &i(-7), &i(1)), i(-4));
        assert_eq!(fold_binary(Op::Shr, &i(-1), &i(1)), i(-1));
    }

    /// 左操作数是**负数**时移位**合法**（oracle 推翻冻结稿的「非负左值检查」）。
    #[test]
    fn 移位_负左值合法() {
        // `go build` 与 `go/types` 一致：`-1 << 2 == -4`
        assert_eq!(fold_binary(Op::Shl, &i(-1), &i(2)), i(-4));
        assert_eq!(fold_binary(Op::Shl, &i(-3), &i(2)), i(-12));
        assert_eq!(fold_binary(Op::Shr, &i(-8), &i(2)), i(-2));
        assert!(shift_operand_ok(&i(-1)), "负左值是**整数**，合法");
    }

    /// 左操作数是**无类型浮点但值为整数** ⇒ 合法，且结果种类**修正为整数**。
    #[test]
    fn 移位_值有整数的无类型浮点() {
        // oracle：`1.0 << 2 == 4`，类型 `untyped int`
        assert_eq!(fold_binary(Op::Shl, &f(1, 1), &i(2)), i(4));
        assert_eq!(fold_binary(Op::Shr, &f(8, 1), &i(1)), i(4));
        // 值不是整数 ⇒ 不给值（官方 "shifted operand 1.5 must be integer"）
        assert_eq!(fold_binary(Op::Shl, &f(3, 2), &i(1)), Const::Unknown);
        assert_eq!(fold_binary(Op::Shl, &s("a"), &i(1)), Const::Unknown);
        assert_eq!(fold_binary(Op::Shl, &b(true), &i(1)), Const::Unknown);
        assert!(!shift_operand_ok(&f(3, 2)));
        assert!(!shift_operand_ok(&s("a")));
        assert!(!shift_operand_ok(&b(true)));
        assert!(!shift_operand_ok(&Const::Unknown));
    }

    /// 计数非法 ⇒ 不给值。
    #[test]
    fn 移位_计数非法() {
        assert_eq!(fold_binary(Op::Shl, &i(1), &i(-1)), Const::Unknown);
        assert_eq!(fold_binary(Op::Shl, &i(1), &f(3, 2)), Const::Unknown);
        assert_eq!(fold_binary(Op::Shl, &i(1), &s("2")), Const::Unknown);
    }

    /// `CON-08`：计数检查的**逐条 oracle 依据**。
    #[test]
    fn shift_ok_与上界() {
        assert_eq!(shift_ok(&i(3)), Ok(3));
        assert_eq!(shift_ok(&i(0)), Ok(0));
        assert_eq!(shift_ok(&f(2, 1)), Ok(2), "`1 << 2.0` 合法");
        assert_eq!(shift_ok(&i(1074)), Ok(SHIFT_BOUND), "上界**含**");
        assert_eq!(shift_ok(&i(1075)), Err(DiagCode::TypInvalidShiftCount));
        assert_eq!(shift_ok(&i(-1)), Err(DiagCode::TypInvalidShiftCount));
        assert_eq!(shift_ok(&f(3, 2)), Err(DiagCode::TypInvalidShiftCount));
        assert_eq!(shift_ok(&s("2")), Err(DiagCode::TypInvalidShiftCount));
        assert_eq!(
            shift_ok(&Const::Complex {
                re: BigRat::from_i64(1),
                im: BigRat::from_i64(0)
            }),
            Ok(1),
            "`1 << (1+0i)`：ToInt 成立"
        );
        assert_eq!(
            shift_ok(&Const::Complex {
                re: BigRat::from_i64(0),
                im: BigRat::from_i64(2)
            }),
            Err(DiagCode::TypInvalidShiftCount),
            "`1 << 2i` 拒绝"
        );
        assert_eq!(
            shift_ok(&Const::Unknown),
            Err(DiagCode::TypInvalidShiftCount)
        );
    }
}
