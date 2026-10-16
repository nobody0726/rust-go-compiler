//! `Representable` —— 常量能不能放进某个基本类型（M3/Task 10，`T-TY-40..44`，`CON-04/05`）。
//!
//! # 与 `convert_const` 的分工（`D-M3-4` 的落点）
//!
//! | | 问什么 | 出什么 |
//! |---|---|---|
//! | [`crate::convert::convert_const`] | 「换一种**精确**表示」行不行 | `Const`（无范围判定） |
//! | **本文件 `representable`** | 「放得进目标类型吗 + 舍入成什么」 | `Represented{value, status}` |
//!
//! 所以 `int8(200)` 在 `convert_const` 那里是 `Ok`（`200` 是精确值），
//! 在这里才被拒。**最终接受/拒绝的结论不变**，只是责任被拆到两个可分别观察的地方 ——
//! `T-TY-47` 要求「溢出后常量仍是**精确原值**」，那就必须有一个函数**只判定、不改值**。
//!
//! # 照抄官方 `representableConst`
//!
//! 逐条对齐 `go/types/const.go:76` 的 `representableConst`：
//!
//! 1. **`Unknown` 直接算通过**（官方注释：`avoid follow-up errors`）。
//!    真正的错在上游已经报过，这里再报一次只会刷屏。
//!    ⚠️ [`crate::convert::convert_const`] 对 `Unknown` 是**报错**的 ——
//!    那个函数不给「上游已报错」的语境兜底，两条口径**有意不同**。
//! 2. 整数目标：先 `ToInt`（`1.5` 不是整数 ⇒ 不通过；`1+0i` 是整数），再查范围。
//! 3. 浮点目标：先 `ToFloat`（复数取实部 ⇒ `float64(1+0i)` 成立、
//!    `float64(1i)` 不成立），再按目标精度量化。
//! 4. 复数目标：先 `ToComplex`，`complex64` 的**实部与虚部各自**按 float32 量化。
//! 5. 字符串/布尔：只看种类。
//! 6. 其余一律不通过 —— 这涵盖 `unsafe.Pointer` / 无类型 `nil`
//!    （`Const` 本来就表达不出 nil）。
//!
//! # 失败用哪条代号
//!
//! 官方 `representation()`（`go/types/const.go` 尾部）按**两端是不是数值**分：
//!
//! ```text
//! 源是数值 && 目标是数值:
//!     源不是整数 && 目标是整数 ⇒ TruncatedFloat     （`int(1.5)`）
//!     否则                     ⇒ NumericOverflow     （`int8(200)` / `float32(1e40)`）
//! 否则                         ⇒ InvalidConstVal     （`int("a")` / `bool(1)`）
//! ```
//!
//! ⚠️ **转换语境**下官方把 `TruncatedFloat` 与 `InvalidConstVal` 都渲染成
//! 「cannot convert … to type T」（实测：`int(1.5)`、`float64(1i)`），
//! 而 `NumericOverflow` 保持「constant … overflows T」。这个**语境差别**归
//! Task 11 的检查器：它拿到本层的 `DiagCode` 后按语境换成对外的那个。
//!
//! # 量化参数
//!
//! `float32 = (24, -126, 127)` / `float64 = (53, -1022, 1023)`，
//! 与官方 `roundFloat32/64` 同义（那两边最终都落到 IEEE-754 的
//! `prec` / `emin` / `emax`）。复数的量化参数**取半宽格式**：
//! `complex64` 的两个分量是 float32 ⇒ `(24, -126, 127)`。
//!
//! # 有意偏离（已记入 `open_gaps`）
//!
//! 官方 `representableConst` 的整数分支 `switch` 里**没有 `UntypedRune`**，
//! 于是 `representable(x, untyped rune)` 会掉到最后 `return false` 而**不通过**。
//! 那是官方的一个洞（`IsInteger(UntypedRune)` 明明是 true）。我们按
//! `IsInteger` 的语义处理：`untyped rune` 与 `untyped int` 同档、无范围限制。
//! 只有当**目标类型本身是无类型**时这条才可见，而 M3 的检查器不会这么调。

use rgoc_lex::DiagCode;

use crate::basic::BasicId;
use crate::bigint::BigInt;
use crate::bigrat::{BigRat, RoundResult};
use crate::fold::as_int_value;
use crate::sizes::Sizes;
use crate::value::Const;

/// 可表示性判定的结果：**目标类型上的值** + **有没有舍入**。
///
/// ⚠️ **偏离 `M3-plan.md` Task 10 的签名**（那里写的是
/// `-> Result<RoundResult, DiagCode>`）：只给判定的话，`T-TY-44` 那条
/// 「`float32(1<<24 + 1)` 要舍成 `1<<24`」就只能证明「发生了舍入」，
/// **证明不了舍到哪儿**。所以带上值 —— 与 Task 4 的
/// [`crate::bigrat::RoundOutcome`] 同一个理由。
///
/// `status` **只有** [`RoundResult::Exact`] / [`RoundResult::Rounded`] 两种取值：
/// 溢出走 `Err`，不会出现在这里（`RoundResult::Overflow` 在返回前就被翻译成
/// [`DiagCode::TypNumericOverflow`] 了）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Represented {
    /// 目标类型上的值。浮点/复数目标是**量化后**的；整数目标精确。
    pub value: Const,
    /// 量化判定。
    pub status: RoundResult,
}

/// `c` 能不能作为 `to` 的值；能的话**目标类型上的值**是什么。
///
/// `sizes` 只在目标是 `int`/`uint`/`uintptr` 时被读（它们的宽度随平台变）。
pub fn representable(c: &Const, to: BasicId, sizes: &Sizes) -> Result<Represented, DiagCode> {
    // ① 官方 `representableConst` 第一条：`Unknown` 直接算通过（避免二次报错）。
    if *c == Const::Unknown {
        return Ok(Represented {
            value: Const::Unknown,
            status: RoundResult::Exact,
        });
    }

    // ② 整数目标：先 `ToInt`，再查范围。
    if to.is_integer() {
        let Some(n) = as_int_value(c) else {
            return Err(failure_code(c, to));
        };
        // 无类型整数 / rune 没有范围限制（`UntypedRune` 见文件头「有意偏离」）
        if matches!(to, BasicId::UntypedInt | BasicId::UntypedRune) {
            return Ok(Represented {
                value: Const::Int(n),
                status: RoundResult::Exact,
            });
        }
        let bits = sizes.width_of(to).unwrap_or(0);
        if !int_fits(&n, to.is_unsigned(), bits) {
            return Err(failure_code(c, to));
        }
        return Ok(Represented {
            value: Const::Int(n),
            status: RoundResult::Exact,
        });
    }

    // ③ 浮点目标：先 `ToFloat`（复数取实部，虚部非零则 `Unknown`），再量化。
    if to.is_float() {
        let Some(r) = as_real(c) else {
            return Err(failure_code(c, to));
        };
        if to == BasicId::UntypedFloat {
            return Ok(Represented {
                value: Const::Float(r),
                status: RoundResult::Exact,
            });
        }
        let (prec, emin, emax) = float_params(to).expect("浮点目标必有量化参数");
        let out = r.round_to_nearest_even(prec, emin, emax);
        if out.status == RoundResult::Overflow {
            return Err(failure_code(c, to));
        }
        return Ok(Represented {
            value: Const::Float(out.value),
            status: out.status,
        });
    }

    // ④ 复数目标：实部虚部**各自**按分量格式量化。
    if to.is_complex() {
        let Some((re, im)) = as_complex(c) else {
            return Err(failure_code(c, to));
        };
        if to == BasicId::UntypedComplex {
            return Ok(Represented {
                value: c.clone(),
                status: RoundResult::Exact,
            });
        }
        let (prec, emin, emax) = float_params(to).expect("复数目标的分量参数");
        let a = re.round_to_nearest_even(prec, emin, emax);
        let b = im.round_to_nearest_even(prec, emin, emax);
        if a.status == RoundResult::Overflow || b.status == RoundResult::Overflow {
            return Err(failure_code(c, to));
        }
        let status = if a.status == RoundResult::Rounded || b.status == RoundResult::Rounded {
            RoundResult::Rounded
        } else {
            RoundResult::Exact
        };
        return Ok(Represented {
            value: Const::Complex {
                re: a.value,
                im: b.value,
            },
            status,
        });
    }

    // ⑤ 字符串 / 布尔：只看种类（**不**做 `string(65)` 那种码点转换 —— 那是转换，不是可表示性）
    if to.is_string() || to.is_bool() {
        return match c {
            Const::Str(bytes) if to.is_string() => Ok(Represented {
                value: Const::Str(bytes.clone()),
                status: RoundResult::Exact,
            }),
            Const::Bool(v) if to.is_bool() => Ok(Represented {
                value: Const::Bool(*v),
                status: RoundResult::Exact,
            }),
            _ => Err(failure_code(c, to)),
        };
    }

    // ⑥ 其余（`unsafe.Pointer` / 无类型 `nil`）—— `Const` 本来就表达不出这些值
    Err(failure_code(c, to))
}

/// 失败用哪条代号 —— 逐字照官方 `representation()` 尾部（`go/types/const.go`）。
///
/// 实测（go1.27.1，声明语境）：`1.5 → int` 「truncated」、`1i → int`
/// 「truncated」（**复数也算「非整数数值」**）、`200 → int8` 「overflows」、
/// `1e40 → float32` 「overflows」、`1e400 → complex64` 「overflows」、
/// `-1 → uint8` 「overflows」。
fn failure_code(src: &Const, to: BasicId) -> DiagCode {
    let src_numeric = matches!(src, Const::Int(_) | Const::Float(_) | Const::Complex { .. });
    if src_numeric && to.is_numeric() {
        // 「源不是整数 && 目标是整数」⇒ 截断；其余数值问题都是范围
        if !matches!(src, Const::Int(_)) && to.is_integer() {
            DiagCode::TypTruncatedFloat
        } else {
            DiagCode::TypNumericOverflow
        }
    } else {
        DiagCode::TypInvalidConstVal
    }
}

/// 浮点/复数量化用的 `(prec, emin, emax)`。
///
/// 复数的参数**取分量格式**：`complex64` 的两个分量是 float32。
fn float_params(to: BasicId) -> Option<(u32, i32, i32)> {
    match to {
        BasicId::Float32 | BasicId::Complex64 => Some((24, -126, 127)),
        BasicId::Float64 | BasicId::Complex128 => Some((53, -1022, 1023)),
        _ => None,
    }
}

/// 整数范围判定：`n` 是否是宽度 `bits` 的（有/无符号）整数。
///
/// 用「需要的位数」而不是构造上下界来比，`1 << 100` 这种大数也是 O(1) 判定
/// （符号判定的那个分支要减一次，但只对负数发生，且是一次线性减法）。
fn int_fits(n: &BigInt, unsigned: bool, bits: u32) -> bool {
    if bits == 0 {
        return false; // 不该发生：调用方已按 BasicId 取过宽度
    }
    if unsigned {
        if n.is_neg() {
            return false;
        }
        return n.bit_len() <= u64::from(bits);
    }
    let magnitude_bits = u64::from(bits) - 1;
    if n.is_neg() {
        // 下界是 -2^(bits-1)（它自己 bit_len == bits，是**合法**的）
        let limit = BigInt::from_i64(1).shl(u32::try_from(magnitude_bits).expect("宽度 < 2^32"));
        !limit.sub(&n.abs()).is_neg()
    } else {
        n.bit_len() <= magnitude_bits
    }
}

/// 取**实数**值（整数 / 浮点 / **零虚部**的复数）。照官方 `ToFloat`。
fn as_real(c: &Const) -> Option<BigRat> {
    match c {
        Const::Int(v) => Some(BigRat::from_bigint(v.clone())),
        Const::Float(r) => Some(r.clone()),
        Const::Complex { re, im } if im.is_zero() => Some(re.clone()),
        _ => None,
    }
}

/// 取复数（整数/浮点 → 零虚部）。照官方 `ToComplex`。
fn as_complex(c: &Const) -> Option<(BigRat, BigRat)> {
    let zero = BigRat::from_i64(0);
    match c {
        Const::Int(v) => Some((BigRat::from_bigint(v.clone()), zero)),
        Const::Float(r) => Some((r.clone(), zero)),
        Const::Complex { re, im } => Some((re.clone(), im.clone())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Sizes = Sizes::HOST_ARM64;

    fn i(v: i64) -> Const {
        Const::Int(BigInt::from_i64(v))
    }

    fn ipow2(k: u32) -> Const {
        Const::Int(BigInt::from_i64(1).shl(k))
    }

    fn err(c: &Const, to: BasicId, code: DiagCode) {
        assert_eq!(
            representable(c, to, &S),
            Err(code),
            "{c:?} -> {to:?} 期望 {code:?}"
        );
    }

    fn ok_value(c: &Const, to: BasicId) -> (Const, RoundResult) {
        let r = representable(c, to, &S).unwrap_or_else(|e| panic!("{c:?} -> {to:?} 被拒: {e:?}"));
        (r.value, r.status)
    }

    /// `T-TY-41`：整数目标的**边界**（含上下各一格的越界）。
    #[test]
    fn 整数目标边界() {
        assert_eq!(ok_value(&i(-128), BasicId::Int8).0, i(-128));
        assert_eq!(ok_value(&i(127), BasicId::Int8).0, i(127));
        assert_eq!(ok_value(&i(-128), BasicId::Int8).1, RoundResult::Exact);
        err(&i(128), BasicId::Int8, DiagCode::TypNumericOverflow);
        err(&i(-129), BasicId::Int8, DiagCode::TypNumericOverflow);

        // uint64：上界是 2^64-1，不是 2^63-1
        let u64max = Const::Int(BigInt::from_i64(1).shl(64).sub(&BigInt::from_i64(1)));
        assert_eq!(ok_value(&u64max, BasicId::Uint64).0, u64max);
        err(&ipow2(64), BasicId::Uint64, DiagCode::TypNumericOverflow);
        err(&i(-1), BasicId::Uint64, DiagCode::TypNumericOverflow);
        err(&i(-1), BasicId::Uint8, DiagCode::TypNumericOverflow);

        // int64 / int32
        assert_eq!(ok_value(&i(i64::MIN), BasicId::Int64).0, i(i64::MIN));
        assert_eq!(ok_value(&i(i64::MAX), BasicId::Int64).0, i(i64::MAX));
        err(
            &Const::Int(BigInt::from_i64(i64::MAX).add(&BigInt::from_i64(1))),
            BasicId::Int64,
            DiagCode::TypNumericOverflow,
        );
    }

    /// `T-TY-41`：**`byte` 就是 `uint8`、`rune` 就是 `int32`** ——
    /// 它们不是独立类型，所以边界也共用。
    #[test]
    fn byte_与_rune_不是独立类型() {
        assert_eq!(BasicId::Uint8.as_str(), "uint8");
        assert_eq!(BasicId::Int32.as_str(), "int32");
        assert_eq!(ok_value(&i(255), BasicId::Uint8).0, i(255));
        err(&i(256), BasicId::Uint8, DiagCode::TypNumericOverflow);
        assert_eq!(
            ok_value(&i(i32::MIN as i64), BasicId::Int32).0,
            i(i32::MIN as i64)
        );
        assert_eq!(
            ok_value(&i(i32::MAX as i64), BasicId::Int32).0,
            i(i32::MAX as i64)
        );
        err(
            &i(i32::MAX as i64 + 1),
            BasicId::Int32,
            DiagCode::TypNumericOverflow,
        );
    }

    /// `T-TY-41`：`int`/`uint`/`uintptr` 的宽度**跟着 `Sizes` 走**。
    #[test]
    fn 平台宽度跟着_sizes() {
        let s32 = Sizes {
            int: 4,
            uint: 4,
            uintptr: 4,
        };
        // arm64：int 是 64 位 ⇒ 1<<40 放得进
        assert!(representable(&ipow2(40), BasicId::Int, &S).is_ok());
        // 32 位目标：1<<40 放不进 int / uint / uintptr
        for t in [BasicId::Int, BasicId::Uint, BasicId::Uintptr] {
            assert_eq!(
                representable(&ipow2(40), t, &s32),
                Err(DiagCode::TypNumericOverflow),
                "{t:?} 在 32 位目标上装不下 1<<40"
            );
        }
        assert!(representable(&ipow2(31), BasicId::Uint, &s32).is_ok());
        assert_eq!(
            representable(&Const::Int(BigInt::from_i64(-1)), BasicId::Uintptr, &s32),
            Err(DiagCode::TypNumericOverflow)
        );
    }

    /// `T-TY-42`：`1<<100` 放不进任何定宽整数，但**无类型**整数没有上限。
    #[test]
    fn 大整数_定宽拒绝_无类型通过() {
        err(&ipow2(100), BasicId::Int8, DiagCode::TypNumericOverflow);
        err(&ipow2(100), BasicId::Int64, DiagCode::TypNumericOverflow);
        err(&ipow2(100), BasicId::Uint64, DiagCode::TypNumericOverflow);
        assert_eq!(ok_value(&ipow2(100), BasicId::UntypedInt).0, ipow2(100));
        assert_eq!(
            ok_value(&ipow2(100), BasicId::UntypedInt).1,
            RoundResult::Exact
        );
    }

    /// `T-TY-43`：浮点目标 —— 精确、量化、溢出三种。
    #[test]
    fn 浮点目标_精确与量化与溢出() {
        // 2^24 恰是 float32 的精度上限 ⇒ 精确
        let (v, st) = ok_value(&ipow2(24), BasicId::Float32);
        assert_eq!(st, RoundResult::Exact);
        assert_eq!(
            v,
            Const::Float(BigRat::from_bigint(BigInt::from_i64(1).shl(24)))
        );

        // 2^24 + 1 舍到 2^24（向偶数）—— **这是值，不只是判定**
        let x = Const::Int(BigInt::from_i64(1).shl(24).add(&BigInt::from_i64(1)));
        let (v, st) = ok_value(&x, BasicId::Float32);
        assert_eq!(st, RoundResult::Rounded);
        assert_eq!(
            v,
            Const::Float(BigRat::from_bigint(BigInt::from_i64(1).shl(24)))
        );

        // float64 能精确收下 2^53，收不下 2^53+1
        assert_eq!(ok_value(&ipow2(53), BasicId::Float64).1, RoundResult::Exact);
        let y = Const::Int(BigInt::from_i64(1).shl(53).add(&BigInt::from_i64(1)));
        assert_eq!(ok_value(&y, BasicId::Float64).1, RoundResult::Rounded);

        // 溢出：2^128 超出 float32 的 emax
        err(&ipow2(128), BasicId::Float32, DiagCode::TypNumericOverflow);
        assert!(representable(&ipow2(128), BasicId::Float64, &S).is_ok());

        // 下溢到 0 是**合法**且记为 Rounded（实测 float32(1e-50) == 0）
        let tiny = Const::Float(BigRat::from_dec("1e-50").expect("1e-50 可解析"));
        let (v, st) = ok_value(&tiny, BasicId::Float32);
        assert_eq!(st, RoundResult::Rounded);
        assert_eq!(v, Const::Float(BigRat::from_i64(0)));
    }

    /// `T-TY-44`：向偶数舍入的**边界**（舍上去还是舍下来由最低保留位决定）。
    #[test]
    fn 向偶数舍入边界() {
        // 2^24 处步长是 2 ⇒ **奇数**才是中点。
        // 2^24 + 1 = 16777217：m = 16777217/2 = 8388608.5，
        // 8388608 是偶数 ⇒ 舍**下**到 2^24。
        let x = Const::Int(BigInt::from_i64(1).shl(24).add(&BigInt::from_i64(1)));
        let (v, st) = ok_value(&x, BasicId::Float32);
        assert_eq!(st, RoundResult::Rounded);
        assert_eq!(
            v,
            Const::Float(BigRat::from_bigint(BigInt::from_i64(1).shl(24))),
            "中点且低位为偶 ⇒ 舍下"
        );
        // 2^24 + 3 = 16777219：m = 8388609.5，8388609 是奇数 ⇒ 进位到 8388610 ⇒ 2^24 + 4
        let y = Const::Int(BigInt::from_i64(1).shl(24).add(&BigInt::from_i64(3)));
        let (v2, st2) = ok_value(&y, BasicId::Float32);
        assert_eq!(st2, RoundResult::Rounded);
        assert_eq!(
            v2,
            Const::Float(BigRat::from_bigint(
                BigInt::from_i64(1).shl(24).add(&BigInt::from_i64(4))
            )),
            "中点且低位为奇 ⇒ 进位到偶"
        );
    }

    /// `T-TY-44`：`1/3` 在两种精度上量化成**各自**的最近值。
    ///
    /// 期望值是 IEEE-754 的知名值，不是算出来的：
    /// `float32(1/3) = 11184811 × 2⁻²⁵`、`float64(1/3) = 6004799503160661 × 2⁻⁵⁴`。
    #[test]
    fn 三分之一_两种精度() {
        let third =
            Const::Float(BigRat::new(BigInt::from_i64(1), BigInt::from_i64(3)).expect("非零"));
        let (v32, st32) = ok_value(&third, BasicId::Float32);
        assert_eq!(st32, RoundResult::Rounded);
        assert_eq!(
            v32,
            Const::Float(
                BigRat::new(BigInt::from_i64(11184811), BigInt::from_i64(1).shl(25)).expect("非零")
            ),
            "float32 的 1/3"
        );
        let (v64, st64) = ok_value(&third, BasicId::Float64);
        assert_eq!(st64, RoundResult::Rounded);
        assert_eq!(
            v64,
            Const::Float(
                BigRat::new(
                    BigInt::from_i64(6004799503160661),
                    BigInt::from_i64(1).shl(54)
                )
                .expect("非零")
            ),
            "float64 的 1/3"
        );
    }

    /// 复数目标：实部虚部**各自**量化；`complex64(1e400)` 溢出（实测官方报错）。
    #[test]
    fn 复数目标() {
        let one_i = Const::Complex {
            re: BigRat::from_i64(0),
            im: BigRat::from_i64(1),
        };
        let (v, st) = ok_value(&one_i, BasicId::Complex128);
        assert_eq!(v, one_i);
        assert_eq!(st, RoundResult::Exact);
        // 整数/浮点也能进复数
        assert_eq!(
            ok_value(&i(2), BasicId::Complex64).0,
            Const::Complex {
                re: BigRat::from_i64(2),
                im: BigRat::from_i64(0)
            }
        );
        // 实部虚部各自量化
        let third = BigRat::new(BigInt::from_i64(1), BigInt::from_i64(3)).expect("非零");
        let (v, st) = ok_value(
            &Const::Complex {
                re: third.clone(),
                im: third.clone(),
            },
            BasicId::Complex64,
        );
        assert_eq!(st, RoundResult::Rounded);
        assert_ne!(
            v,
            Const::Complex {
                re: third.clone(),
                im: third
            }
        );
        // 溢出
        err(
            &Const::Float(BigRat::from_dec("1e400").expect("1e400 可解析")),
            BasicId::Complex64,
            DiagCode::TypNumericOverflow,
        );
    }

    /// `T-TY-47`：**溢出 ⇒ 稳定代号 + 常量仍是精确原值**（判定不改值）。
    #[test]
    fn 溢出后原常量不变() {
        let big = Const::Int(BigInt::from_i64(1).shl(100));
        let before = big.clone();
        assert_eq!(
            representable(&big, BasicId::Int8, &S),
            Err(DiagCode::TypNumericOverflow)
        );
        assert_eq!(big, before, "判定**不得**就地截断/改写常量");
        // 无类型整数目标照样能收下它（值没被前面的失败污染）
        assert_eq!(ok_value(&big, BasicId::UntypedInt).0, before);
    }

    /// 截断 vs 范围：`int(1.5)` 是 TruncatedFloat，`int8(200)` 是 NumericOverflow。
    ///
    /// ⚠️ 官方的分界**看的是源常量的类型，不是值**（实测 go1.27.1）：
    /// `const q int8 = 1e3` ⇒ 「truncated」（`1e3` 是**浮点**常量，哪怕它是整数 `1000`），
    /// 而 `const t int8 = 1000` ⇒ 「overflows」。所以 `2e40 → int` 也是 truncated。
    /// 这条反直觉，但它是 oracle 的事实 —— 本函数逐字照抄官方的 `representation()`。
    #[test]
    fn 截断与范围两种代号() {
        let one_half =
            Const::Float(BigRat::new(BigInt::from_i64(3), BigInt::from_i64(2)).expect("非零"));
        err(&one_half, BasicId::Int, DiagCode::TypTruncatedFloat);
        err(&one_half, BasicId::Int8, DiagCode::TypTruncatedFloat);
        // 2.0 是整数 ⇒ 走范围那条路（这里放得进 ⇒ Ok）
        let two = Const::Float(BigRat::from_i64(2));
        assert_eq!(ok_value(&two, BasicId::Int).0, i(2));
        assert_eq!(ok_value(&two, BasicId::Int).1, RoundResult::Exact);
        // **浮点**常量装不进整数 ⇒ 永远是 truncated（值是否是整数不影响）
        let huge = Const::Float(BigRat::from_dec("2e40").expect("可解析"));
        err(&huge, BasicId::Int, DiagCode::TypTruncatedFloat);
        let thousand_float = Const::Float(BigRat::from_dec("1e3").expect("可解析"));
        err(&thousand_float, BasicId::Int8, DiagCode::TypTruncatedFloat);
        // **整数**常量装不进整数 ⇒ 永远是 overflows
        err(&i(1000), BasicId::Int8, DiagCode::TypNumericOverflow);
    }

    /// 种类不匹配 ⇒ `InvalidConstVal`；`unsafe.Pointer` / 无类型 `nil` 也在这条里。
    #[test]
    fn 种类不匹配() {
        let s = Const::Str(b"a".to_vec());
        err(&s, BasicId::Int, DiagCode::TypInvalidConstVal);
        err(&s, BasicId::Float64, DiagCode::TypInvalidConstVal);
        err(
            &Const::Bool(true),
            BasicId::Int,
            DiagCode::TypInvalidConstVal,
        );
        err(&i(1), BasicId::Bool, DiagCode::TypInvalidConstVal);
        err(&i(1), BasicId::String, DiagCode::TypInvalidConstVal);
        err(&i(1), BasicId::UnsafePointer, DiagCode::TypInvalidConstVal);
        err(&i(1), BasicId::UntypedNil, DiagCode::TypInvalidConstVal);
        // 复数**虚部非零** ⇒ 既不是实数也不是整数。代号照官方 `representation()`：
        // 两边都是数值 ⇒ 走「截断 / 范围」两条，**不是** `InvalidConstVal`
        // （实测声明语境：`const a int = 1i` ⇒ 「(truncated)」）。
        let one_i = Const::Complex {
            re: BigRat::from_i64(0),
            im: BigRat::from_i64(1),
        };
        err(&one_i, BasicId::Float64, DiagCode::TypNumericOverflow);
        err(&one_i, BasicId::Int, DiagCode::TypTruncatedFloat);
        // 但**零虚部**的复数就是实数（实测 `float64(1+0i)` 通过）
        let one = Const::Complex {
            re: BigRat::from_i64(1),
            im: BigRat::from_i64(0),
        };
        assert_eq!(
            ok_value(&one, BasicId::Float64).0,
            Const::Float(BigRat::from_i64(1))
        );
        assert_eq!(ok_value(&one, BasicId::Int).0, i(1));
        // 字符串/布尔目标
        assert_eq!(ok_value(&s, BasicId::String).0, s);
        assert_eq!(ok_value(&s, BasicId::UntypedString).1, RoundResult::Exact);
        assert_eq!(
            ok_value(&Const::Bool(false), BasicId::Bool).0,
            Const::Bool(false)
        );
    }

    /// 官方 `representableConst` 第一条：`Unknown` **直接算通过**（避免二次报错）。
    #[test]
    fn unknown_直接通过() {
        assert_eq!(
            representable(&Const::Unknown, BasicId::Int8, &S),
            Ok(Represented {
                value: Const::Unknown,
                status: RoundResult::Exact
            })
        );
        assert!(representable(&Const::Unknown, BasicId::String, &S).is_ok());
        assert!(representable(&Const::Unknown, BasicId::UnsafePointer, &S).is_ok());
    }

    /// 整数与浮点之间：整数进浮点精确、浮点进整数要真的是整数。
    #[test]
    fn 数值种类互转() {
        // `float64(1e40)` 通过；`float32(1e40)` 溢出
        let e40 = Const::Float(BigRat::from_dec("1e40").expect("可解析"));
        assert!(representable(&e40, BasicId::Float64, &S).is_ok());
        err(&e40, BasicId::Float32, DiagCode::TypNumericOverflow);
        // 整数 → 浮点
        assert_eq!(
            ok_value(&i(1), BasicId::Float64).0,
            Const::Float(BigRat::from_i64(1))
        );
        // 浮点 → 无类型（无上限）
        assert_eq!(ok_value(&e40, BasicId::UntypedFloat).1, RoundResult::Exact);
        assert_eq!(
            ok_value(&ipow2(200), BasicId::UntypedFloat).0,
            Const::Float(BigRat::from_bigint(BigInt::from_i64(1).shl(200)))
        );
    }
}
