//! 精确有理数 + 向偶数舍入（M3/Task 4）。
//!
//! # 为什么不引 `bigdecimal` / `rug`
//!
//! 同 [`crate::bigint`]：红线是**不默认新增第三方依赖**（`AGENTS.md` §4.3），
//! 且这里要的是**精确**（不是任意精度浮点）—— `D-M3-4`：常量层的浮点**不引入
//! 实现限制**，整数任意精度、浮点**精确有理数**，只在**转换**到具体浮点类型时
//! 才按「向偶数舍入」量化。
//!
//! # 表示
//!
//! `num / den`，**规范形式**：`den > 0`、`gcd(|num|, den) == 1`、零写作 `0/1`。
//! 规范形式是 `Eq` 成立的前提（`1/2` 与 `2/4` 必须相等）。
//!
//! # 舍入（[`BigRat::round_to_nearest_even`]）
//!
//! 输入 `prec` / `emin` / `emax` 来自目标浮点类型（float32 = 24 / -126 / 127）。
//! 语义照 IEEE-754 的 **round-half-to-even**，含**次正规**区间：
//! 量化步长 `2^s`，其中 `s = max(e, emin) - prec + 1`、`e = ⌊log2|x|⌋`。

use std::cmp::Ordering;
use std::fmt;

use crate::bigint::BigInt;

/// 精确有理数。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BigRat {
    /// 分子（带符号）
    pub(crate) num: BigInt,
    /// 分母（**恒正**，且与分子互素）
    pub(crate) den: BigInt,
}

/// 舍入判定（`T-TY-4x` / `T-TY-93` 的判据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoundResult {
    /// 目标格式能**精确**表示（无需舍入）
    Exact,
    /// 舍入到相邻可表示值（含次正规区）
    Rounded,
    /// 舍入后指数超出 `emax`（无限大方向）
    Overflow,
}

/// 舍入的完整结果：**值** + **判定**。
///
/// ⚠️ **偏离 `M3-plan.md` Task 4 的签名**（那里写的是 `-> Option<RoundResult>`）：
/// 计划里的判据「2²⁴+1 必须舍成 16777216 而不是 16777218」**需要看见值**，
/// 只回判定就测不出来。判定照旧可用（后续任务只读 `.status`），值供测试与
/// 将来的转换路径使用。`Overflow` 时 `value` **无意义**（= 输入原样），
/// 只有 `status` 可用 —— 写明是为了避免调用方误用。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RoundOutcome {
    /// 舍入后的值（仍是精确有理数 —— 浮点格式只提供「量化步长」）
    pub value: BigRat,
    /// 判定
    pub status: RoundResult,
}

#[allow(clippy::should_implement_trait)] // 同 bigint：名字来自计划约定，刻意不用运算符重载
impl BigRat {
    /// 构造并**规范化**（约分、分母转正）。分母为零返回 `None`。
    pub fn new(num: BigInt, den: BigInt) -> Option<Self> {
        if den.is_zero() {
            return None;
        }
        Some(Self::normalize(num, den))
    }

    /// 整数（`den = 1`）。
    pub fn from_bigint(v: BigInt) -> Self {
        BigRat {
            num: v,
            den: BigInt::from_i64(1),
        }
    }

    /// 从 `i64` 构造。
    pub fn from_i64(v: i64) -> Self {
        Self::from_bigint(BigInt::from_i64(v))
    }

    /// 十进制解析：`[±] 整数 [. 小数] [e[±]指数]`，允许 `_`。
    ///
    /// 量级守卫：`|指数| > 2²⁰` 返回 `None`（**拒绝解析**，不是静默截断）。
    /// 规范**允许**实现限制指数（「signed binary exponent of at least 16 bits」），
    /// 这个上限远高于规范要求，只是为了不让一个病态字面量把内存吃光。
    pub fn from_dec(s: &str) -> Option<Self> {
        const MAX_EXP: i64 = 1 << 20;
        let (neg, body) = match s.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, s.strip_prefix('+').unwrap_or(s)),
        };
        if body.is_empty() {
            return None;
        }
        let (mant, exp) = match body.find(['e', 'E']) {
            Some(i) => (body.get(..i)?, parse_exp(body.get(i + 1..)?)?),
            None => (body, 0i64),
        };
        if exp.abs() > MAX_EXP {
            return None;
        }
        let (int_part, frac_part) = match mant.find('.') {
            Some(i) => (mant.get(..i)?, mant.get(i + 1..)?),
            None => (mant, ""),
        };
        let int_digits = digits_of(int_part)?;
        let frac_digits = digits_of(frac_part)?;
        if int_digits.is_empty() && frac_digits.is_empty() {
            return None;
        }
        let mut num = BigInt::from_dec(&format!("{int_digits}{frac_digits}"))?;
        if neg {
            num = -num;
        }
        let mut den = ten_pow(frac_digits.len() as u32);
        if exp >= 0 {
            num = num.mul(&ten_pow(exp as u32));
        } else {
            den = den.mul(&ten_pow((-exp) as u32));
        }
        Some(Self::normalize(num, den))
    }

    /// 分子。
    pub fn num(&self) -> &BigInt {
        &self.num
    }

    /// 分母（恒正）。
    pub fn den(&self) -> &BigInt {
        &self.den
    }

    /// 是否为零。
    pub fn is_zero(&self) -> bool {
        self.num.is_zero()
    }

    /// 是否为负。
    pub fn is_neg(&self) -> bool {
        self.num.is_neg()
    }

    /// 绝对值。
    pub fn abs(&self) -> Self {
        BigRat {
            num: self.num.abs(),
            den: self.den.clone(),
        }
    }

    /// 加法（精确）。
    pub fn add(&self, other: &Self) -> Self {
        Self::normalize(
            self.num.mul(&other.den).add(&other.num.mul(&self.den)),
            self.den.mul(&other.den),
        )
    }

    /// 减法（精确）。
    pub fn sub(&self, other: &Self) -> Self {
        Self::normalize(
            self.num.mul(&other.den).sub(&other.num.mul(&self.den)),
            self.den.mul(&other.den),
        )
    }

    /// 乘法（精确）。
    pub fn mul(&self, other: &Self) -> Self {
        Self::normalize(self.num.mul(&other.num), self.den.mul(&other.den))
    }

    /// 除法（精确）。除数为零返回 `None`。
    pub fn div(&self, other: &Self) -> Option<Self> {
        if other.is_zero() {
            return None;
        }
        Some(Self::normalize(
            self.num.mul(&other.den),
            self.den.mul(&other.num),
        ))
    }

    /// 是否是**整数值**；是则给出该整数（照官方 `constant.ToInt`）。
    ///
    /// `2.0` ⇒ `Some(2)`；`1.5` ⇒ `None`。移位与「无类型浮点常量可作整数用」
    /// 这两处都要靠它 —— 官方 `go/types/expr.go:625` 的
    /// `constant.ToInt(x.val).Kind() == constant.Int` 就是这条。
    pub fn to_int(&self) -> Option<BigInt> {
        if self.den == BigInt::from_i64(1) {
            Some(self.num.clone())
        } else {
            None
        }
    }

    /// `⌊log₂|x|⌋`（零上无定义 ⇒ panic；调用方先查零）。
    pub fn floor_log2(&self) -> i64 {
        assert!(!self.num.is_zero(), "floor_log2(0) 无定义");
        let n = self.num.abs();
        let mut e = n.bit_len() as i64 - self.den.bit_len() as i64;
        let fits = if e >= 0 {
            n >= self.den.shl(e as u32)
        } else {
            n.shl((-e) as u32) >= self.den
        };
        if !fits {
            e -= 1;
        }
        e
    }

    /// 按目标格式（`prec` 有效位、指数 `[emin, emax]`）做 **round-half-to-even**。
    pub fn round_to_nearest_even(&self, prec: u32, emin: i32, emax: i32) -> RoundOutcome {
        assert!(prec > 0, "prec 必须 > 0");
        if self.num.is_zero() {
            return RoundOutcome {
                value: self.clone(),
                status: RoundResult::Exact,
            };
        }
        let e = self.floor_log2();
        // 次正规区：指数被钉在 emin 上（量化步长不再随 e 变小）
        let s = e.max(emin as i64) - prec as i64 + 1;
        // ⚠️ `round_to_pow2_multiple` 只看**绝对值**（比大小与进位与符号无关），
        // 符号必须在这一层贴回去 —— 漏掉它会让全部负数舍成正数（本任务的实测缺陷）。
        let (mag, exact) = self.round_to_pow2_multiple(s);
        let mant = if self.num.is_neg() { -mag } else { mag };
        let value = BigRat::from_bigint(mant.clone()).scale_pow2(s);
        if !mant.is_zero() {
            let ve = mant.bit_len() as i64 - 1 + s;
            if ve > emax as i64 {
                return RoundOutcome {
                    value: self.clone(),
                    status: RoundResult::Overflow,
                };
            }
        }
        RoundOutcome {
            value,
            status: if exact {
                RoundResult::Exact
            } else {
                RoundResult::Rounded
            },
        }
    }

    /// `round(|x| / 2^s)`（half-to-even）；返回值与「是否恰好整除」。
    ///
    /// ⚠️ 只看**绝对值**：返回的是**无符号**尾数，符号由调用方贴回。
    /// 只用整数：`|x|/2^s = numer / denom`，再把 `numer` 截断除 `denom`，
    /// 用余数与 `denom/2` 比大小决定进位。
    fn round_to_pow2_multiple(&self, s: i64) -> (BigInt, bool) {
        let numer = self.num.abs();
        let (numer, denom) = if s >= 0 {
            (numer, self.den.shl(s as u32))
        } else {
            (numer.shl((-s) as u32), self.den.clone())
        };
        let (q, r) = numer.div_mod(&denom).expect("den > 0");
        if r.is_zero() {
            return (q, true);
        }
        let twice = r.shl(1);
        let mant = match twice.cmp(&denom) {
            Ordering::Greater => q.add(&BigInt::from_i64(1)),
            Ordering::Equal => {
                if q.is_even() {
                    q
                } else {
                    q.add(&BigInt::from_i64(1))
                }
            }
            Ordering::Less => q,
        };
        (mant, false)
    }

    /// `value × 2^s`（保持精确）。
    fn scale_pow2(self, s: i64) -> Self {
        if s >= 0 {
            Self::normalize(self.num.shl(s as u32), self.den)
        } else {
            Self::normalize(self.num, self.den.shl((-s) as u32))
        }
    }

    /// 规范形式：分母转正 → 零归 `0/1` → 约分。
    fn normalize(num: BigInt, den: BigInt) -> Self {
        let (num, den) = if den.is_neg() {
            (-num, -den)
        } else {
            (num, den)
        };
        if num.is_zero() {
            return BigRat {
                num: BigInt::zero(),
                den: BigInt::from_i64(1),
            };
        }
        let g = gcd(&num.abs(), &den);
        if g == BigInt::from_i64(1) {
            BigRat { num, den }
        } else {
            BigRat {
                num: num.div_exact(&g),
                den: den.div_exact(&g),
            }
        }
    }
}

/// 欧几里得求最大公约数（只对非负量有意义）。
fn gcd(a: &BigInt, b: &BigInt) -> BigInt {
    let mut a = a.abs();
    let mut b = b.abs();
    while !b.is_zero() {
        let (_, r) = a.div_mod(&b).expect("gcd: 除零");
        a = b;
        b = r;
    }
    a
}

/// `10^k`（快速幂，避免 k 次小乘）。
pub(crate) fn ten_pow(k: u32) -> BigInt {
    let mut result = BigInt::from_i64(1);
    let mut base = BigInt::from_i64(10);
    let mut e = k;
    while e > 0 {
        if e & 1 == 1 {
            result = result.mul(&base);
        }
        e >>= 1;
        if e > 0 {
            base = base.mul(&base);
        }
    }
    result
}

/// 取一串「十进制数位 + `_`」，返回去掉 `_` 的数位；空串合法（`.5` 的整数部分是空）。
fn digits_of(s: &str) -> Option<String> {
    if s.is_empty() {
        return Some(String::new());
    }
    if s.starts_with('_') || s.ends_with('_') || s.contains("__") {
        return None;
    }
    if !s.chars().all(|c| c.is_ascii_digit() || c == '_') {
        return None;
    }
    Some(s.chars().filter(|c| *c != '_').collect())
}

/// 指数部分：`[±] 数位`。
fn parse_exp(s: &str) -> Option<i64> {
    let (neg, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let digits = digits_of(digits)?;
    if digits.is_empty() {
        return None;
    }
    let v: i64 = digits.parse().ok()?;
    Some(if neg { -v } else { v })
}

impl PartialOrd for BigRat {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BigRat {
    /// 交叉相乘比较（`den > 0` 保证方向不翻）。
    fn cmp(&self, other: &Self) -> Ordering {
        self.num.mul(&other.den).cmp(&other.num.mul(&self.den))
    }
}

impl fmt::Display for BigRat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == BigInt::from_i64(1) {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: i64, d: i64) -> BigRat {
        BigRat::new(BigInt::from_i64(n), BigInt::from_i64(d)).unwrap()
    }

    fn int(v: i64) -> BigRat {
        BigRat::from_i64(v)
    }

    /// float32 的三个参数，测试里反复用。
    const F32: (u32, i32, i32) = (24, -126, 127);

    #[test]
    fn from_dec_小数() {
        assert_eq!(BigRat::from_dec("0.1"), Some(r(1, 10)));
        assert_eq!(BigRat::from_dec("1.5"), Some(r(3, 2)));
        assert_eq!(BigRat::from_dec(".5"), Some(r(1, 2)));
        assert_eq!(BigRat::from_dec("-0.25"), Some(r(-1, 4)));
        assert_eq!(BigRat::from_dec("3"), Some(int(3)));
        assert_eq!(BigRat::from_dec("1e3"), Some(int(1000)));
        assert_eq!(BigRat::from_dec("1.5e-1"), Some(r(15, 100)));
        assert_eq!(BigRat::from_dec("1_0.2_5"), Some(r(1025, 100)));
        for bad in ["", ".", "e3", "1.2.3", "1e", "0x1"] {
            assert_eq!(BigRat::from_dec(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn 规范形式_约分与分母恒正() {
        let a = BigRat::new(BigInt::from_i64(2), BigInt::from_i64(4)).unwrap();
        assert_eq!(a, r(1, 2));
        let b = BigRat::new(BigInt::from_i64(1), BigInt::from_i64(-2)).unwrap();
        assert_eq!(b, r(-1, 2));
        assert_eq!(b.den().to_string(), "2", "分母恒正");
        assert_eq!(
            BigRat::new(BigInt::from_i64(0), BigInt::from_i64(5)),
            Some(int(0))
        );
        assert_eq!(
            BigRat::new(BigInt::from_i64(1), BigInt::from_i64(0)),
            None,
            "分母零"
        );
    }

    /// `0.1 + 0.2 == 0.3` —— 精确有理数**不做**二进制近似。
    #[test]
    fn 加减乘除都是精确的() {
        let s = r(1, 10).add(&r(2, 10));
        assert_eq!(s, r(3, 10));
        assert_eq!(s.to_string(), "3/10");
        assert_eq!(r(3, 2).mul(&int(2)), int(3));
        assert_eq!(r(1, 3).sub(&r(1, 6)), r(1, 6));
        assert_eq!(r(1, 3).div(&r(1, 6)), Some(int(2)));
        assert_eq!(r(1, 3).div(&int(0)), None, "除零");
    }

    /// `D-M3-4`：不设人为上限 —— 1000 位相乘必须精确。
    #[test]
    fn 大数乘法不截断() {
        let big = BigInt::from_i64(1).shl(1000);
        let x = BigRat::from_bigint(big.clone());
        let y = x.mul(&x);
        assert_eq!(y, BigRat::from_bigint(big.shl(1000)));
        assert_eq!(y.num().bit_len(), 2001);
    }

    #[test]
    fn to_int_只在整数值上成立() {
        assert_eq!(int(3).to_int(), Some(BigInt::from_i64(3)));
        assert_eq!(r(2, 1).to_int(), Some(BigInt::from_i64(2)));
        assert_eq!(r(3, 2).to_int(), None);
        assert_eq!(
            r(-4, 2).to_int(),
            Some(BigInt::from_i64(-2)),
            "规范化后 den == 1"
        );
        assert_eq!(int(0).to_int(), Some(BigInt::from_i64(0)));
    }

    #[test]
    fn floor_log2() {
        assert_eq!(int(1).floor_log2(), 0);
        assert_eq!(int(2).floor_log2(), 1);
        assert_eq!(int(3).floor_log2(), 1);
        assert_eq!(int(4).floor_log2(), 2);
        assert_eq!(r(3, 4).floor_log2(), -1);
        assert_eq!(r(1, 3).floor_log2(), -2);
        assert_eq!(int(-8).floor_log2(), 3, "取绝对值");
    }

    #[test]
    fn 舍入_2_的_24_次方加一_向偶数() {
        let (prec, emin, emax) = F32;
        let x = BigRat::from_bigint(BigInt::from_i64(1).shl(24).add(&BigInt::from_i64(1)));
        let got = x.round_to_nearest_even(prec, emin, emax);
        assert_eq!(got.status, RoundResult::Rounded);
        assert_eq!(
            got.value,
            BigRat::from_bigint(BigInt::from_i64(1).shl(24)),
            "恰好在中点 ⇒ 取偶数尾数（16777216，不是 16777218）"
        );
    }

    #[test]
    fn 舍入_可精确表示时是_exact() {
        let (prec, emin, emax) = F32;
        for v in [0i64, 1, -1, 2, 1 << 23, (1 << 24) - 2] {
            let got = BigRat::from_i64(v).round_to_nearest_even(prec, emin, emax);
            assert_eq!(got.status, RoundResult::Exact, "{v}");
            assert_eq!(got.value, BigRat::from_i64(v), "{v}");
        }
    }

    #[test]
    fn 舍入_溢出() {
        let (prec, emin, emax) = F32;
        let huge = BigRat::from_bigint(BigInt::from_i64(1).shl(128));
        let got = huge.round_to_nearest_even(prec, emin, emax);
        assert_eq!(got.status, RoundResult::Overflow);
    }

    #[test]
    fn 舍入_次正规区() {
        let (prec, emin, emax) = F32;
        // 最小次正规 |x| = 2^-149：精确
        let min_sub = BigRat::new(BigInt::from_i64(1), BigInt::from_i64(1).shl(149)).unwrap();
        let got = min_sub.round_to_nearest_even(prec, emin, emax);
        assert_eq!(got.status, RoundResult::Exact);
        assert_eq!(got.value, min_sub);
        // 2^-150 恰在中点（0 与 2^-149 之间）⇒ 向偶数 ⇒ 0
        let half = BigRat::new(BigInt::from_i64(1), BigInt::from_i64(1).shl(150)).unwrap();
        let got = half.round_to_nearest_even(prec, emin, emax);
        assert_eq!(got.status, RoundResult::Rounded);
        assert_eq!(got.value, int(0), "中点向偶数 ⇒ 0（0 的尾数是偶数）");
    }

    #[test]
    fn 舍入_三分之一() {
        let (prec, emin, emax) = F32;
        let got = r(1, 3).round_to_nearest_even(prec, emin, emax);
        assert_eq!(got.status, RoundResult::Rounded);
        // 1/3 舍到 24 位有效位 = 11184811 / 2^25
        assert_eq!(got.value, r(11184811, 1 << 25));
    }
}
