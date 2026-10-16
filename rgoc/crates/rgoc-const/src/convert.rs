//! 常量转换与无类型默认类型（M3/Task 8，`T-TY-38` / `T-TY-3b`，`CON-03`）。
//!
//! # 本层**不舍入**
//!
//! `convert_const` 做的是「**换一种精确表示**」：`1` → `1.0`、`2.0` → `2`、
//! `65` → `"A"`。它**不**把 `1/3` 量化成 `float64` 的最近值 —— 那件事在
//! Task 10（[`representable`]，返回带舍入结果的 `RoundOutcome`）。
//!
//! 这条边界是 `D-M3-4` 说的「只在转换时按向偶数舍入」的落点：
//! **判定与舍入在 Task 10，本层只负责种类**。
//!
//! # 与官方的分工差异（有意）
//!
//! 官方 `go/types` 的转换**同时**查可表示性（`conversions.go` 里
//! `float64(1e400)` 直接报 overflow）。我们把两件事拆开，因为
//! `T-TY-47` 要求「溢出后常量仍是**精确原值**」—— 那要求「值」与「判定」
//! 是**两个**可分别观察的东西。所以：
//!
//! ```text
//! convert_const(Const, BasicId) -> Result<Const, DiagCode>          ← 种类（本文件）
//! representable(&Const, BasicId, &Sizes) -> Result<RoundOutcome, _>  ← 范围 + 舍入（Task 10）
//! ```
//!
//! ⇒ `convert_const(Int(200), Int8)` 是 **Ok**（`200` 是精确值），
//! 由 Task 10 判「放不进 `int8`」。**接受/拒绝的最终结论不变**，
//! 只是责任的落点不同；Task 11 的检查器按顺序调用两者。
//!
//! # 实测钉住的语义（全部来自 go1.27.1 的 oracle 探针）
//!
//! | 表达式 | 官方结果 |
//! |---|---|
//! | `string(65)` | `"A"` |
//! | `string(-1)` | `"\uFFFD"`（**不是错误**） |
//! | `string(0x110000)` | `"\uFFFD"`（越界码点 ⇒ 替换字符） |
//! | `string(1 << 20)` | `"\U00100000"`（合法 4 字节 UTF-8） |
//! | `int(2.0)` | `2`（值是整数 ⇒ 合法） |
//! | `int(1.5)` | 拒绝（`cannot convert 1.5 … to type int`） |
//! | `float64(1i)` | 拒绝（`cannot convert 1i … to type float64`） |
//! | `bool(1)` | 拒绝（`cannot convert 1 … to type bool`） |
//! | `float32(1) / 3` | **舍入**成 float32（本层给精确 `1/3`，见上文分工） |
//! | `int8(200)` | 拒绝（`constant 200 overflows int8`，Task 10 的活） |

use rgoc_lex::DiagCode;

use crate::basic::BasicId;
use crate::bigint::BigInt;
use crate::bigrat::BigRat;
use crate::value::Const;

/// 无类型常量的**默认类型**（照官方的 `Default`）。
///
/// ⚠️ **`'a'` 的默认类型是 `rune`（= `Int32`），不是 `int`** —— 而本函数只看
/// 得到**值**，`'a'` 与 `97` 在这里都是 [`Const::Int`]，所以它给不出 rune。
/// 无类型性属于**类型**层（`D-M3-3`）：字面量的种类在 AST 的
/// [`rgoc_ast::LitKind::Rune`] 上，检查器据此把类型定成 `UntypedRune`，
/// 再用 [`default_type_of`] 落到 `Int32`。**本函数只服务「值形状」这条路。**
///
/// `None` ⇒ [`Const::Unknown`]：它**没有**默认类型。这不是偷懒 ——
/// 「没有值」和「有一个默认类型的值」是两件事，返回 `UntypedNil` 之类会
/// 让调用方以为可以继续。**计划里这里写的是 `-> BasicId`，见 ledger Task 8 Ruling。**
#[must_use]
pub fn untyped_default_type(c: &Const) -> Option<BasicId> {
    match c {
        Const::Int(_) => Some(BasicId::Int),
        Const::Float(_) => Some(BasicId::Float64),
        Const::Complex { .. } => Some(BasicId::Complex128),
        Const::Str(_) => Some(BasicId::String),
        Const::Bool(_) => Some(BasicId::Bool),
        Const::Unknown => None,
    }
}

/// 无类型基本类型 → 默认类型（照官方 `Default`）。
///
/// 已是具体类型 ⇒ **恒等**（官方 `Default` 对非 untyped 也返回自身）。
/// `UntypedNil` 恒等（官方对 nil 不做默认化）。
#[must_use]
pub fn default_type_of(untyped: BasicId) -> BasicId {
    match untyped {
        BasicId::UntypedBool => BasicId::Bool,
        BasicId::UntypedInt => BasicId::Int,
        BasicId::UntypedRune => BasicId::Int32,
        BasicId::UntypedFloat => BasicId::Float64,
        BasicId::UntypedComplex => BasicId::Complex128,
        BasicId::UntypedString => BasicId::String,
        // 官方 `Default` 不碰 `untyped nil`
        BasicId::UntypedNil => BasicId::UntypedNil,
        other => other,
    }
}

/// 常量转换：**换一种精确表示**，不查范围、不舍入。
///
/// ⚠️ **偏离计划签名**：计划写 `convert_const(c, to, sizes: &Sizes)`。
/// `Sizes` 在这个函数里**永远用不到**（平台宽度只在判范围时要，那是 Task 10），
/// 一个从不被读的参数是**谎言**（读代码的人会以为这里有平台逻辑）。
/// 所以去掉它，`&Sizes` 由 Task 10 的 `representable` 接收。见 ledger Task 8 Ruling。
pub fn convert_const(c: &Const, to: BasicId) -> Result<Const, DiagCode> {
    // 「没有值」是**上游的不一致**，不是「转换不合法」—— 两条代号必须分开，
    // 否则 Task 11 会把内部错误报成用户错误。
    if *c == Const::Unknown {
        return Err(DiagCode::TypInvalidConstVal);
    }
    let zero = BigRat::from_i64(0);
    match c {
        Const::Bool(v) => {
            if to.is_bool() {
                Ok(Const::Bool(*v))
            } else {
                Err(DiagCode::TypInvalidConversion)
            }
        }
        Const::Str(bytes) => {
            if to.is_string() {
                Ok(Const::Str(bytes.clone()))
            } else {
                Err(DiagCode::TypInvalidConversion)
            }
        }
        Const::Int(v) => {
            if to.is_integer() {
                Ok(Const::Int(v.clone()))
            } else if to.is_float() {
                Ok(Const::Float(BigRat::from_bigint(v.clone())))
            } else if to.is_complex() {
                Ok(Const::Complex {
                    re: BigRat::from_bigint(v.clone()),
                    im: zero,
                })
            } else if to.is_string() {
                // `string(65) == "A"`；非法码点给 U+FFFD（**不是错误**，实测）
                Ok(Const::Str(int_to_string(v)))
            } else {
                Err(DiagCode::TypInvalidConversion)
            }
        }
        Const::Float(r) => {
            if to.is_float() {
                // **不**舍入：`float64(1)/3` 在这里仍是精确的 `1/3`
                Ok(Const::Float(r.clone()))
            } else if to.is_complex() {
                Ok(Const::Complex {
                    re: r.clone(),
                    im: zero,
                })
            } else if to.is_integer() {
                // `int(2.0)` 合法、`int(1.5)` 拒绝（实测官方走 `InvalidConversion`）
                r.to_int()
                    .map(Const::Int)
                    .ok_or(DiagCode::TypInvalidConversion)
            } else {
                Err(DiagCode::TypInvalidConversion)
            }
        }
        Const::Complex { re, im } => {
            if to.is_complex() {
                Ok(c.clone())
            } else if to.is_float() {
                // 虚部非零 ⇒ 实测官方拒绝（`cannot convert 1i to type float64`）
                if im.is_zero() {
                    Ok(Const::Float(re.clone()))
                } else {
                    Err(DiagCode::TypInvalidConversion)
                }
            } else if to.is_integer() {
                if im.is_zero() {
                    re.to_int()
                        .map(Const::Int)
                        .ok_or(DiagCode::TypInvalidConversion)
                } else {
                    Err(DiagCode::TypInvalidConversion)
                }
            } else {
                Err(DiagCode::TypInvalidConversion)
            }
        }
        Const::Unknown => unreachable!("上面已经拦掉 Unknown"),
    }
}

/// 整数 → 字符串：按**码点**语义（照 Go 的 `string(int)`）。
///
/// 非法码点（负 / 越界 / 代理区）⇒ **U+FFFD 替换字符**，不是错误 ——
/// 这是官方对常量也成立的行为（实测 `string(-1)`、`string(0x110000)`）。
fn int_to_string(v: &BigInt) -> Vec<u8> {
    let cp = v
        .to_i64()
        .and_then(|n| u32::try_from(n).ok())
        .and_then(char::from_u32);
    match cp {
        Some(ch) => {
            let mut buf = [0u8; 4];
            ch.encode_utf8(&mut buf).as_bytes().to_vec()
        }
        None => "\u{fffd}".as_bytes().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i(v: i64) -> Const {
        Const::Int(BigInt::from_i64(v))
    }

    fn r(n: i64, d: i64) -> Const {
        Const::Float(BigRat::new(BigInt::from_i64(n), BigInt::from_i64(d)).unwrap())
    }

    fn s(v: &str) -> Const {
        Const::Str(v.as_bytes().to_vec())
    }

    /// `T-TY-3b`：值形状 → 默认类型。
    #[test]
    fn 默认类型_值形状() {
        assert_eq!(untyped_default_type(&i(1)), Some(BasicId::Int));
        assert_eq!(untyped_default_type(&r(1, 2)), Some(BasicId::Float64));
        assert_eq!(
            untyped_default_type(&Const::Complex {
                re: BigRat::from_i64(0),
                im: BigRat::from_i64(1)
            }),
            Some(BasicId::Complex128)
        );
        assert_eq!(untyped_default_type(&s("a")), Some(BasicId::String));
        assert_eq!(
            untyped_default_type(&Const::Bool(true)),
            Some(BasicId::Bool)
        );
        assert_eq!(
            untyped_default_type(&Const::Unknown),
            None,
            "没有值 ⇒ 没有默认类型"
        );
    }

    /// `T-TY-3b`：**`'a'` 的默认类型是 `rune`** —— 走的是类型层那条路。
    #[test]
    fn 默认类型_无类型_rune_落_int32() {
        assert_eq!(default_type_of(BasicId::UntypedRune), BasicId::Int32);
        assert_eq!(default_type_of(BasicId::UntypedInt), BasicId::Int);
        assert_eq!(default_type_of(BasicId::UntypedFloat), BasicId::Float64);
        assert_eq!(
            default_type_of(BasicId::UntypedComplex),
            BasicId::Complex128
        );
        assert_eq!(default_type_of(BasicId::UntypedString), BasicId::String);
        assert_eq!(default_type_of(BasicId::UntypedBool), BasicId::Bool);
        assert_eq!(default_type_of(BasicId::UntypedNil), BasicId::UntypedNil);
        // 已是具体类型 ⇒ 恒等
        for b in BasicId::ALL {
            if !b.is_untyped() {
                assert_eq!(default_type_of(b), b, "{b:?} 不该被默认化");
            }
        }
    }

    /// `T-TY-38`：整数 → 字符串是**码点**语义，非法码点 ⇒ 替换字符（**不是错误**）。
    #[test]
    fn 整数到字符串_码点语义() {
        assert_eq!(convert_const(&i(65), BasicId::String), Ok(s("A")));
        assert_eq!(convert_const(&i(0x4e2d), BasicId::String), Ok(s("中")));
        assert_eq!(
            convert_const(&i(0x1f600), BasicId::String),
            Ok(Const::Str("\u{1f600}".as_bytes().to_vec()))
        );
        // 实测：负值与越界码点都得到 U+FFFD，**不报错**
        assert_eq!(convert_const(&i(-1), BasicId::String), Ok(s("\u{fffd}")));
        assert_eq!(
            convert_const(&i(0x11_0000), BasicId::String),
            Ok(s("\u{fffd}"))
        );
        assert_eq!(
            convert_const(&Const::Int(BigInt::from_i64(1).shl(100)), BasicId::String),
            Ok(s("\u{fffd}")),
            "超出码点范围 ⇒ 替换字符"
        );
        // 1<<20 是合法码点 ⇒ 4 字节 UTF-8
        assert_eq!(
            convert_const(&i(1 << 20), BasicId::String),
            Ok(Const::Str(vec![0xf4, 0x80, 0x80, 0x80]))
        );
    }

    /// `T-TY-38`：数值↔数值转换**不丢精度**。**范围**不是本函数的活。
    #[test]
    fn 数值转换不丢精度() {
        assert_eq!(convert_const(&i(1), BasicId::Float64), Ok(r(1, 1)));
        assert_eq!(
            convert_const(&r(1, 3), BasicId::Float64),
            Ok(r(1, 3)),
            "**不**在此处舍入"
        );
        assert_eq!(
            convert_const(&r(2, 1), BasicId::Int),
            Ok(i(2)),
            "2.0 转 int 合法"
        );
        assert_eq!(
            convert_const(&Const::Int(BigInt::from_i64(1).shl(100)), BasicId::Int64),
            Ok(Const::Int(BigInt::from_i64(1).shl(100))),
            "范围归 Task 10；本层照样给精确值"
        );
        assert_eq!(convert_const(&i(200), BasicId::Int8), Ok(i(200)), "同上");
        // 复数 → 实数是**换表示**（虚部为 0）
        assert_eq!(
            convert_const(
                &Const::Complex {
                    re: BigRat::from_i64(1),
                    im: BigRat::from_i64(0)
                },
                BasicId::Float64
            ),
            Ok(r(1, 1))
        );
    }

    /// 不允许的转换 ⇒ `TYP-INVALID-CONVERSION`；截断 ⇒ 也走它（实测：官方这条给的是转换错误）。
    #[test]
    fn 非法转换() {
        let inv = Err(DiagCode::TypInvalidConversion);
        assert_eq!(convert_const(&r(3, 2), BasicId::Int), inv, "1.5 → int");
        assert_eq!(convert_const(&r(1, 2), BasicId::Int8), inv);
        assert_eq!(
            convert_const(
                &Const::Complex {
                    re: BigRat::from_i64(0),
                    im: BigRat::from_i64(1)
                },
                BasicId::Float64
            ),
            inv,
            "1i → float64（实测官方拒绝）"
        );
        assert_eq!(convert_const(&i(1), BasicId::Bool), inv);
        assert_eq!(convert_const(&Const::Bool(true), BasicId::Int), inv);
        assert_eq!(convert_const(&s("a"), BasicId::Int), inv);
        assert_eq!(convert_const(&s("a"), BasicId::Uintptr), inv);
        assert_eq!(convert_const(&i(1), BasicId::UnsafePointer), inv);
        assert_eq!(convert_const(&i(1), BasicId::UntypedNil), inv);
    }

    /// 恒等转换（同类）必须原样通过；`Unknown` 有独立代号。
    #[test]
    fn 恒等与_unknown() {
        assert_eq!(convert_const(&i(1), BasicId::Int), Ok(i(1)));
        assert_eq!(convert_const(&i(1), BasicId::UntypedInt), Ok(i(1)));
        assert_eq!(convert_const(&s("a"), BasicId::String), Ok(s("a")));
        assert_eq!(
            convert_const(&Const::Bool(false), BasicId::Bool),
            Ok(Const::Bool(false))
        );
        assert_eq!(
            convert_const(&Const::Unknown, BasicId::Int),
            Err(DiagCode::TypInvalidConstVal),
            "「没有值」与「转换不合法」是**两件事**"
        );
    }
}
