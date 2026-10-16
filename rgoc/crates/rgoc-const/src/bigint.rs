//! 任意精度整数（M3/Task 4）。
//!
//! # 为什么不引 `num-bigint`
//!
//! `AGENTS.md` §4.3 的红线：**不默认新增第三方依赖**。而且本 crate 需要的能力
//! 很窄 —— 十进制/十六进制字面量、加减乘、截断除、移位、比较、`to_i64` —— 这在
//! base 2³² 上用小学算法就是百来行。引入一整棵依赖树换这点代码不划算，
//! 也破掉「离线可构建」（`M3-design.md` `D-M3-12`）。
//!
//! # 表示
//!
//! `limbs: Vec<u32>`（**小端**：`limbs[0]` 是最低 32 位） + `neg: bool`。
//! **规范形式**：`limbs` 无尾随零（`limbs.last() != Some(&0)`）；零 = 空 `Vec` 且
//! `neg == false`（⇒ 零只有一个表示，`Eq` 才成立）。
//!
//! # 符号语义（照 Go）
//!
//! - `/` 与 `%` 都是**截断**语义（`%` 取**被除数**的符号）：`-7 / 2 == -3`、`-7 % 2 == -1`；
//! - `>>` 是**算术**右移（对负数是下取整）：`-3 >> 1 == -2`。
//!
//! # 为什么方法名长得像 trait
//!
//! `add` / `sub` / `mul` / `div_mod` / `shl` / `shr` 是 `M3-plan.md` Task 4 与
//! Task 6 约定好的名字，下游按这些名字调用。`clippy::should_implement_trait`
//! 会因为「像 `Add`/`Sub`」而报警，但这里**刻意不用运算符重载**：
//! 常量的运算在折叠器里是**数据流**（拿 `&BigInt` 造新值），隐式的 `+` 会掩盖
//! 「这一步生成了多少个中间值」，而折叠器恰恰要能看见这一点。

use std::cmp::Ordering;
use std::fmt;
use std::ops::Neg;

/// 按位运算类别（**无限精度补码**语义，照 Go 的常量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BitOp {
    /// `&`
    And,
    /// `|`
    Or,
    /// `^`
    Xor,
    /// `&^`
    AndNot,
}

/// 任意精度整数。
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct BigInt {
    /// 绝对值（小端 32 位肢），无尾随零
    pub(crate) limbs: Vec<u32>,
    /// 符号（零恒为 `false`）
    pub(crate) neg: bool,
}

// ── 肢层原语（都不看符号）──────────────────────────────────────────────────

/// 去掉尾随零肢。
fn trim(v: &mut Vec<u32>) {
    while let Some(&0) = v.last() {
        v.pop();
    }
}

/// 绝对值的位数（零 = 0）。
fn bit_len_mag(a: &[u32]) -> u64 {
    match a.last() {
        None => 0,
        Some(&top) => (a.len() as u64 - 1) * 32 + (32 - top.leading_zeros()) as u64,
    }
}

fn bit_mag(a: &[u32], i: u64) -> bool {
    let w = (i / 32) as usize;
    w < a.len() && (a[w] >> (i % 32)) & 1 == 1
}

fn set_bit(v: &mut [u32], i: u64) {
    let w = (i / 32) as usize;
    if w < v.len() {
        v[w] |= 1 << (i % 32);
    }
}

fn cmp_mag(a: &[u32], b: &[u32]) -> Ordering {
    if a.len() != b.len() {
        return a.len().cmp(&b.len());
    }
    for i in (0..a.len()).rev() {
        if a[i] != b[i] {
            return a[i].cmp(&b[i]);
        }
    }
    Ordering::Equal
}

fn add_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut out = Vec::with_capacity(long.len() + 1);
    let mut carry = 0u64;
    for i in 0..long.len() {
        let x = long[i] as u64 + carry + if i < short.len() { short[i] as u64 } else { 0 };
        out.push(x as u32);
        carry = x >> 32;
    }
    if carry != 0 {
        out.push(carry as u32);
    }
    out
}

/// 要求 `a >= b`。
fn sub_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    debug_assert!(cmp_mag(a, b) != Ordering::Less, "sub_mag: a < b");
    let mut out = Vec::with_capacity(a.len());
    let mut borrow = 0i64;
    for i in 0..a.len() {
        let x = a[i] as i64 - borrow - if i < b.len() { b[i] as i64 } else { 0 };
        if x < 0 {
            out.push((x + (1i64 << 32)) as u32);
            borrow = 1;
        } else {
            out.push(x as u32);
            borrow = 0;
        }
    }
    trim(&mut out);
    out
}

fn mul_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0u32; a.len() + b.len()];
    for (i, &x) in a.iter().enumerate() {
        let mut carry = 0u64;
        for (j, &y) in b.iter().enumerate() {
            let cur = out[i + j] as u64 + x as u64 * y as u64 + carry;
            out[i + j] = cur as u32;
            carry = cur >> 32;
        }
        let mut k = i + b.len();
        while carry != 0 {
            let cur = out[k] as u64 + carry;
            out[k] = cur as u32;
            carry = cur >> 32;
            k += 1;
        }
    }
    trim(&mut out);
    out
}

/// 绝对值除：返回 `(商, 余)`。`b` 非零由调用方保证。
///
/// 用**二进制长除**（逐位试减）。不追 Knuth 的 D 算法：常量的位宽是几百到几千位，
/// O(bits · limbs) 在实测里是毫秒级，而 D 算法的表位与规范化是**缺陷高发区** ——
/// 用一个自己能读懂的算法换掉一个必须信任的算法。
fn div_mod_mag(a: &[u32], b: &[u32]) -> (Vec<u32>, Vec<u32>) {
    debug_assert!(!b.is_empty(), "div_mod_mag: 除零");
    let bits = bit_len_mag(a);
    let mut q = vec![0u32; a.len()];
    let mut r: Vec<u32> = Vec::new();
    for i in (0..bits).rev() {
        // r = r*2 + a[i]
        let mut shifted = Vec::with_capacity(r.len() + 1);
        let mut carry = 0u32;
        for &limb in &r {
            shifted.push((limb << 1) | carry);
            carry = limb >> 31;
        }
        if carry != 0 {
            shifted.push(carry);
        }
        trim(&mut shifted);
        r = shifted;
        if bit_mag(a, i) {
            if r.is_empty() {
                r.push(1);
            } else {
                r[0] |= 1;
            }
        }
        if cmp_mag(&r, b) != Ordering::Less {
            r = sub_mag(&r, b);
            set_bit(&mut q, i);
        }
    }
    trim(&mut q);
    (q, r)
}

fn shl_mag(a: &[u32], n: u32) -> Vec<u32> {
    if a.is_empty() || n == 0 {
        return a.to_vec();
    }
    let words = (n / 32) as usize;
    let bits = n % 32;
    let mut out = vec![0u32; words];
    if bits == 0 {
        out.extend_from_slice(a);
    } else {
        let mut carry = 0u32;
        for &limb in a {
            out.push((limb << bits) | carry);
            carry = limb >> (32 - bits);
        }
        if carry != 0 {
            out.push(carry);
        }
    }
    trim(&mut out);
    out
}

fn shr_mag(a: &[u32], n: u32) -> Vec<u32> {
    let words = (n / 32) as usize;
    if words >= a.len() {
        return Vec::new();
    }
    let bits = n % 32;
    let src = &a[words..];
    let mut out = Vec::with_capacity(src.len());
    if bits == 0 {
        out.extend_from_slice(src);
    } else {
        for i in 0..src.len() {
            let hi = if i + 1 < src.len() {
                src[i + 1] << (32 - bits)
            } else {
                0
            };
            out.push((src[i] >> bits) | hi);
        }
    }
    trim(&mut out);
    out
}

fn mul_small_mag(a: &[u32], m: u32) -> Vec<u32> {
    if m == 0 || a.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(a.len() + 1);
    let mut carry = 0u64;
    for &limb in a {
        let x = limb as u64 * m as u64 + carry;
        out.push(x as u32);
        carry = x >> 32;
    }
    if carry != 0 {
        out.push(carry as u32);
    }
    trim(&mut out);
    out
}

fn add_small_mag(a: &[u32], s: u32) -> Vec<u32> {
    let mut out = a.to_vec();
    let mut carry = s as u64;
    let mut i = 0;
    while carry != 0 {
        if i == out.len() {
            out.push(0);
        }
        let x = out[i] as u64 + carry;
        out[i] = x as u32;
        carry = x >> 32;
        i += 1;
    }
    trim(&mut out);
    out
}

fn div_mod_small_mag(a: &[u32], d: u32) -> (Vec<u32>, u32) {
    debug_assert!(d != 0);
    let mut q = vec![0u32; a.len()];
    let mut rem = 0u64;
    for i in (0..a.len()).rev() {
        let cur = (rem << 32) | a[i] as u64;
        q[i] = (cur / d as u64) as u32;
        rem = cur % d as u64;
    }
    trim(&mut q);
    (q, rem as u32)
}

/// 判断 `a` 是否有低于第 `n` 位的有效位（用于算术右移的「丢位」判断）。
fn any_bit_below(a: &[u32], n: u32) -> bool {
    let words = (n / 32) as usize;
    let bits = n % 32;
    for (i, &limb) in a.iter().enumerate() {
        if i < words {
            if limb != 0 {
                return true;
            }
        } else if i == words {
            if bits != 0 && (limb & ((1u32 << bits) - 1)) != 0 {
                return true;
            }
        } else {
            break;
        }
    }
    false
}

// ── BigInt ────────────────────────────────────────────────────────────────

#[allow(clippy::should_implement_trait)] // 见模块头「为什么方法名长得像 trait」
impl BigInt {
    /// 零。
    pub fn zero() -> Self {
        BigInt {
            limbs: Vec::new(),
            neg: false,
        }
    }

    /// 从 `i64` 构造（含 `i64::MIN`）。
    pub fn from_i64(v: i64) -> Self {
        if v == 0 {
            return Self::zero();
        }
        Self {
            limbs: build_from_u64(v.unsigned_abs()),
            neg: v < 0,
        }
    }

    /// 从 `u64` 构造。
    pub fn from_u64(v: u64) -> Self {
        if v == 0 {
            return Self::zero();
        }
        Self {
            limbs: build_from_u64(v),
            neg: false,
        }
    }

    /// 十进制解析（允许前导 `+`/`-` 与肢体间的 `_`）。
    pub fn from_dec(s: &str) -> Option<Self> {
        Self::from_radix(s, 10)
    }

    /// 十六进制解析（同上，另允许前缀位后的前导 `_`，照 Go 的 `0x_FF`）。
    pub fn from_hex(s: &str) -> Option<Self> {
        Self::from_radix(s, 16)
    }

    /// 八进制解析。
    pub fn from_oct(s: &str) -> Option<Self> {
        Self::from_radix(s, 8)
    }

    /// 二进制解析。
    pub fn from_bin(s: &str) -> Option<Self> {
        Self::from_radix(s, 2)
    }

    fn from_radix(s: &str, radix: u32) -> Option<Self> {
        let (neg, digits) = match s.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, s.strip_prefix('+').unwrap_or(s)),
        };
        if digits.is_empty() || digits.ends_with('_') {
            return None;
        }
        let mut acc = Self::zero();
        let mut n_digits = 0usize;
        let mut prev_us = false;
        for c in digits.chars() {
            if c == '_' {
                if prev_us {
                    return None;
                }
                prev_us = true;
                continue;
            }
            let d = c.to_digit(radix)?;
            acc.limbs = add_small_mag(&mul_small_mag(&acc.limbs, radix), d);
            n_digits += 1;
            prev_us = false;
        }
        if n_digits == 0 {
            return None;
        }
        acc.neg = neg && !acc.limbs.is_empty();
        Some(acc)
    }

    /// 十进制文本：**无前导 0**、零写 `0`、负数带 `-`。
    ///
    /// 任意精度 ⇒ **不能**走 [`BigInt::to_i64`]（`[1<<40]int` 是合法的数组长度，
    /// 而且诊断要打印「constant 1000 overflows int8」里的那个值）。
    /// `C7` §4.3 的数组长度文本取这里。
    #[must_use]
    pub fn to_dec(&self) -> String {
        if self.limbs.is_empty() {
            return "0".to_string();
        }
        let mut mag = self.limbs.clone();
        let mut digits: Vec<u8> = Vec::new();
        while !mag.is_empty() {
            let (q, r) = div_mod_small_mag(&mag, 10);
            digits.push(b'0' + r as u8);
            mag = q;
        }
        digits.reverse();
        let mut s = String::from_utf8(digits).expect("十进制数字必是 UTF-8");
        if self.neg {
            s.insert(0, '-');
        }
        s
    }

    /// 转 `i64`；**超出范围返回 `None`**（不截断）。
    pub fn to_i64(&self) -> Option<i64> {
        match bit_len_mag(&self.limbs) {
            0 => Some(0),
            n if n < 64 => {
                let mut mag = 0u64;
                for (i, &limb) in self.limbs.iter().enumerate() {
                    mag |= (limb as u64) << (32 * i);
                }
                Some(if self.neg { -(mag as i64) } else { mag as i64 })
            }
            64 => {
                // 只有 -2^63 落在范围里
                if self.neg
                    && self.limbs.len() == 2
                    && self.limbs[1] == 0x8000_0000
                    && self.limbs[0] == 0
                {
                    Some(i64::MIN)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// 是否为零。
    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    /// 是否为负（零不是负）。
    pub fn is_neg(&self) -> bool {
        self.neg
    }

    /// 绝对值。
    pub fn abs(&self) -> Self {
        BigInt {
            limbs: self.limbs.clone(),
            neg: false,
        }
    }

    /// 绝对值位数（零 = 0）。
    pub fn bit_len(&self) -> u64 {
        bit_len_mag(&self.limbs)
    }

    /// 是否为偶数（零算偶数）。
    pub fn is_even(&self) -> bool {
        self.limbs.first().is_none_or(|&l| l & 1 == 0)
    }

    /// 加法。
    pub fn add(&self, other: &Self) -> Self {
        if self.neg == other.neg {
            return BigInt {
                limbs: add_mag(&self.limbs, &other.limbs),
                neg: self.neg,
            }
            .canonical();
        }
        match cmp_mag(&self.limbs, &other.limbs) {
            Ordering::Equal => Self::zero(),
            Ordering::Greater => BigInt {
                limbs: sub_mag(&self.limbs, &other.limbs),
                neg: self.neg,
            }
            .canonical(),
            Ordering::Less => BigInt {
                limbs: sub_mag(&other.limbs, &self.limbs),
                neg: other.neg,
            }
            .canonical(),
        }
    }

    /// 减法。
    pub fn sub(&self, other: &Self) -> Self {
        self.add(&other.negated())
    }

    /// 乘法。
    pub fn mul(&self, other: &Self) -> Self {
        BigInt {
            limbs: mul_mag(&self.limbs, &other.limbs),
            neg: self.neg != other.neg,
        }
        .canonical()
    }

    /// 截断除：`(商, 余)`，余数取**被除数**符号。除数为零返回 `None`。
    pub fn div_mod(&self, other: &Self) -> Option<(Self, Self)> {
        if other.is_zero() {
            return None;
        }
        let (q, r) = div_mod_mag(&self.limbs, &other.limbs);
        let quo = BigInt {
            limbs: q,
            neg: self.neg != other.neg,
        }
        .canonical();
        let rem = BigInt {
            limbs: r,
            neg: self.neg,
        }
        .canonical();
        Some((quo, rem))
    }

    /// 左移（`2^n` 倍）。
    pub fn shl(&self, n: u32) -> Self {
        BigInt {
            limbs: shl_mag(&self.limbs, n),
            neg: self.neg,
        }
        .canonical()
    }

    /// **算术**右移（对负数是下取整，照 Go）。
    pub fn shr(&self, n: u32) -> Self {
        if !self.neg {
            return BigInt {
                limbs: shr_mag(&self.limbs, n),
                neg: false,
            };
        }
        // 负数：floor(x / 2^n) = -ceil(|x| / 2^n)
        let q = shr_mag(&self.limbs, n);
        let q = if any_bit_below(&self.limbs, n) {
            add_small_mag(&q, 1)
        } else {
            q
        };
        BigInt {
            limbs: q,
            neg: true,
        }
        .canonical()
    }

    /// 按位补：`^x == -x - 1`（**无限精度补码**，照 Go 常量：`^0 == -1`）。
    pub fn bit_not(&self) -> Self {
        BigInt::from_i64(-1).sub(self)
    }

    /// 按位运算（**无限精度补码**语义）。
    ///
    /// 做法：两个操作数各自在 `n = max(肢数) + 1` 肢的宽度上展开成补码
    /// （负数在高位**补 1**），逐肢运算，再从补码判回符号。多出的那一肢是
    /// **必需**的 —— 它保证最高位确实是结果的符号位（`-1 & 5 == 5` 这类
    /// 跨符号用例靠它成立）。
    pub fn bitwise(&self, op: BitOp, other: &Self) -> Self {
        let n = self.limbs.len().max(other.limbs.len()) + 1;
        let a = self.to_twos(n);
        let b = other.to_twos(n);
        let mut out = vec![0u32; n];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = match op {
                BitOp::And => a[i] & b[i],
                BitOp::Or => a[i] | b[i],
                BitOp::Xor => a[i] ^ b[i],
                BitOp::AndNot => a[i] & !b[i],
            };
        }
        Self::from_twos(&out)
    }

    /// 在 `n` 肢宽度上展开成补码（负数高位补 1）。
    fn to_twos(&self, n: usize) -> Vec<u32> {
        let mut v = vec![0u32; n];
        let take = self.limbs.len().min(n);
        v[..take].copy_from_slice(&self.limbs[..take]);
        if self.neg {
            for limb in v.iter_mut() {
                *limb = !*limb;
            }
            let mut carry = 1u64;
            for limb in v.iter_mut() {
                let s = *limb as u64 + carry;
                *limb = s as u32;
                carry = s >> 32;
            }
        }
        v
    }

    /// 补码 → 符号-绝对值。
    fn from_twos(v: &[u32]) -> Self {
        let neg = v.last().is_some_and(|&top| top >> 31 == 1);
        if !neg {
            let mut limbs = v.to_vec();
            trim(&mut limbs);
            return BigInt { limbs, neg: false };
        }
        let mut mag: Vec<u32> = v.iter().map(|l| !l).collect();
        let mut carry = 1u64;
        for limb in mag.iter_mut() {
            let s = *limb as u64 + carry;
            *limb = s as u32;
            carry = s >> 32;
        }
        trim(&mut mag);
        let neg = !mag.is_empty();
        BigInt { limbs: mag, neg }.canonical()
    }

    /// 内部：整除（用于约分；不满足整除即违反内部不变量）。
    pub(crate) fn div_exact(&self, d: &Self) -> Self {
        let (q, r) = self.div_mod(d).expect("div_exact: 除零");
        debug_assert!(r.is_zero(), "div_exact: 不整除");
        q
    }

    fn negated(&self) -> Self {
        BigInt {
            limbs: self.limbs.clone(),
            neg: !self.neg,
        }
        .canonical()
    }

    /// 规范化：零的符号必须清零、肢必须去尾随零。
    fn canonical(mut self) -> Self {
        trim(&mut self.limbs);
        if self.limbs.is_empty() {
            self.neg = false;
        }
        self
    }
}

fn build_from_u64(v: u64) -> Vec<u32> {
    let mut out = vec![v as u32, (v >> 32) as u32];
    trim(&mut out);
    out
}

impl Neg for BigInt {
    type Output = BigInt;
    fn neg(self) -> BigInt {
        self.negated()
    }
}

impl PartialOrd for BigInt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BigInt {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.neg, other.neg) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => cmp_mag(&self.limbs, &other.limbs),
            (true, true) => cmp_mag(&other.limbs, &self.limbs),
        }
    }
}

impl fmt::Display for BigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            return f.write_str("0");
        }
        if self.neg {
            f.write_str("-")?;
        }
        // 每 9 位十进制一肢（10^9 < 2^32）
        const CHUNK: u32 = 1_000_000_000;
        let mut chunks = Vec::new();
        let mut cur = self.limbs.clone();
        while !cur.is_empty() {
            let (q, r) = div_mod_small_mag(&cur, CHUNK);
            chunks.push(r);
            cur = q;
        }
        let mut out = String::new();
        out.push_str(&chunks.last().unwrap().to_string());
        for c in chunks.iter().rev().skip(1) {
            out.push_str(&format!("{c:09}"));
        }
        f.write_str(&out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2^100 —— `T-TY-31` 的底座：十进制解析必须任意精度。
    #[test]
    fn from_dec_到_2_的_100_次方() {
        let want = BigInt::from_i64(1).shl(100);
        assert_eq!(
            BigInt::from_dec("1267650600228229401496703205376"),
            Some(want)
        );
    }

    #[test]
    fn to_dec_与_from_dec_互逆() {
        for s in [
            "0",
            "1",
            "-1",
            "7",
            "-7",
            "255",
            "256",
            "-256",
            "18446744073709551615",
            "-18446744073709551615",
            "1099511627776",
            "1267650600228229401496703205376",
            "-99999999999999999999999999999999999999",
        ] {
            let v = BigInt::from_dec(s).expect(s);
            assert_eq!(v.to_dec(), s, "{s}");
        }
    }

    #[test]
    fn to_dec_的四处边界() {
        assert_eq!(BigInt::zero().to_dec(), "0");
        assert_eq!(BigInt::from_i64(0).to_dec(), "0");
        // `-0` 读进来就是零 ⇒ 写出去**不带**负号
        assert_eq!(BigInt::from_dec("-0").expect("-0").to_dec(), "0");
        // `2^40` —— 合法的数组长度（`to_i64` 也能表示，但 `2^100` 不能 ⇒ 必须走十进制）
        assert_eq!(BigInt::from_i64(1).shl(40).to_dec(), "1099511627776");
        // `u64::MAX`（0 号 limb 全 1）
        assert_eq!(BigInt::from_u64(u64::MAX).to_dec(), "18446744073709551615");
        // `2^100`：`to_i64` 是 `None`，但十进制必须打得出来
        let big = BigInt::from_i64(1).shl(100);
        assert!(big.to_i64().is_none());
        assert_eq!(big.to_dec(), "1267650600228229401496703205376");
        // 负的大数
        assert_eq!(
            BigInt::zero().sub(&big).to_dec(),
            "-1267650600228229401496703205376"
        );
    }

    #[test]
    fn to_dec_非十进制字面量也走同一条路() {
        // 十六进制 / 八进制 / 二进制字面量读进来之后，写出去一律十进制（C7 §4.3）
        assert_eq!(BigInt::from_hex("ff").expect("ff").to_dec(), "255");
        assert_eq!(BigInt::from_oct("777").expect("777").to_dec(), "511");
        assert_eq!(BigInt::from_bin("100000000").expect("bin").to_dec(), "256");
    }

    #[test]
    fn shl_1000_的十进制位数是_302() {
        // 2^1000 有 ⌊1000·log10 2⌋+1 = 302 位十进制
        assert_eq!(BigInt::from_i64(1).shl(1000).to_string().len(), 302);
    }

    #[test]
    fn 进制解析与下划线() {
        assert_eq!(BigInt::from_hex("ff"), Some(BigInt::from_i64(255)));
        assert_eq!(BigInt::from_hex("_FF"), Some(BigInt::from_i64(255)));
        assert_eq!(BigInt::from_oct("17"), Some(BigInt::from_i64(15)));
        assert_eq!(BigInt::from_bin("1010"), Some(BigInt::from_i64(10)));
        assert_eq!(
            BigInt::from_dec("1_000_000"),
            Some(BigInt::from_i64(1_000_000))
        );
        // 上界：0xFFFF_FFFF 不是 u32 能装的「多肢」边界，取再大一位
        assert_eq!(
            BigInt::from_hex("1_0000_0000"),
            Some(BigInt::from_u64(1u64 << 32))
        );
    }

    #[test]
    fn 非法输入返回_none() {
        for bad in ["", "+", "-", "1x", "0x", "0b2", "1.5", "--1", "1 "] {
            assert_eq!(BigInt::from_dec(bad), None, "dec {bad:?}");
        }
        assert_eq!(BigInt::from_hex("g"), None);
        assert_eq!(BigInt::from_bin("2"), None);
    }

    #[test]
    fn 零的唯一表示() {
        let z = BigInt::from_i64(0);
        assert!(z.is_zero());
        assert_eq!(BigInt::from_dec("-0"), Some(z.clone()));
        assert_eq!(BigInt::from_dec("000"), Some(z.clone()));
        assert!(!z.is_neg(), "零不是负数（否则 Eq 会坏）");
        assert_eq!(z.to_string(), "0");
    }

    /// Go 的 `/` 与 `%` 都是**截断**语义（`%` 取被除数符号）。
    #[test]
    fn 除法截断且余数取被除数符号() {
        let (q, r) = BigInt::from_i64(-7).div_mod(&BigInt::from_i64(2)).unwrap();
        assert_eq!(q, BigInt::from_i64(-3));
        assert_eq!(r, BigInt::from_i64(-1));

        let (q, r) = BigInt::from_i64(7).div_mod(&BigInt::from_i64(-2)).unwrap();
        assert_eq!(q, BigInt::from_i64(-3));
        assert_eq!(r, BigInt::from_i64(1), "% 取被除数符号 ⇒ +1");
    }

    #[test]
    fn 除零返回_none() {
        assert!(BigInt::from_i64(1).div_mod(&BigInt::from_i64(0)).is_none());
    }

    #[test]
    fn 算术右移_对负数是下取整() {
        assert_eq!(BigInt::from_i64(-3).shr(1), BigInt::from_i64(-2));
        assert_eq!(BigInt::from_i64(-4).shr(1), BigInt::from_i64(-2));
        assert_eq!(BigInt::from_i64(-1).shr(10), BigInt::from_i64(-1));
        assert_eq!(BigInt::from_i64(-1).shl(10), BigInt::from_i64(-1024));
        assert_eq!(BigInt::from_i64(3).shr(1), BigInt::from_i64(1));
    }

    #[test]
    fn to_i64_边界往返() {
        for v in [
            0i64,
            1,
            -1,
            127,
            -128,
            i64::MAX,
            i64::MIN,
            1 << 32,
            -(1 << 40),
        ] {
            assert_eq!(BigInt::from_i64(v).to_i64(), Some(v), "{v}");
        }
        // 超出 i64 必须 None（不是截断）
        assert_eq!(BigInt::from_i64(1).shl(100).to_i64(), None);
        assert_eq!((-BigInt::from_i64(1)).shl(100).to_i64(), None);
        assert_eq!(
            BigInt::from_i64(i64::MAX)
                .add(&BigInt::from_i64(1))
                .to_i64(),
            None
        );
    }

    #[test]
    fn 比较与符号() {
        assert!(BigInt::from_i64(-1) < BigInt::from_i64(0));
        assert!(BigInt::from_i64(-2) < BigInt::from_i64(-1));
        assert!(BigInt::from_i64(2) > BigInt::from_i64(1));
        assert_eq!(
            BigInt::from_i64(1)
                .shl(100)
                .cmp(&BigInt::from_i64(1).shl(99)),
            Ordering::Greater
        );
        let n = (-BigInt::from_i64(5)).abs();
        assert_eq!(n, BigInt::from_i64(5));
    }

    #[test]
    fn 乘_加减_大数() {
        let a = BigInt::from_i64(1).shl(64);
        let b = a.mul(&BigInt::from_i64(3));
        assert_eq!(b.to_string(), "55340232221128654848"); // 3·2^64
        assert_eq!(b.sub(&a), a.add(&a));
    }

    #[test]
    fn bit_len() {
        assert_eq!(BigInt::from_i64(0).bit_len(), 0);
        assert_eq!(BigInt::from_i64(1).bit_len(), 1);
        assert_eq!(BigInt::from_i64(255).bit_len(), 8);
        assert_eq!(BigInt::from_i64(256).bit_len(), 9);
        assert_eq!(BigInt::from_i64(1).shl(100).bit_len(), 101);
        assert_eq!((-BigInt::from_i64(1)).bit_len(), 1, "符号不进 bit_len");
    }

    /// **无限精度补码**：跨符号与跨肢两类用例都必须对。
    #[test]
    fn 按位运算是无限精度补码() {
        let i = BigInt::from_i64;
        let bw = |a: i64, op: BitOp, b: i64| i(a).bitwise(op, &i(b));
        assert_eq!(bw(-1, BitOp::And, 5), i(5));
        assert_eq!(bw(-5, BitOp::And, 3), i(3));
        assert_eq!(bw(-5, BitOp::Or, 3), i(-5));
        assert_eq!(bw(-1, BitOp::Or, 0), i(-1));
        assert_eq!(bw(5, BitOp::Xor, 3), i(6));
        assert_eq!(bw(5, BitOp::AndNot, 3), i(4));
        assert_eq!(bw(-1, BitOp::AndNot, 5), i(-6), "&^ 等价于 a & ^b");
        // `^x == -x-1`
        assert_eq!(i(0).bit_not(), i(-1));
        assert_eq!(i(5).bit_not(), i(-6));
        assert_eq!(i(-1).bit_not(), i(0));
        // 跨肢：符号位必须由多出的那一肢定住
        let big = i(1).shl(100);
        assert_eq!(big.bitwise(BitOp::And, &big), big);
        assert_eq!((-big.clone()).bitwise(BitOp::And, &big), big);
        assert_eq!(big.bit_not().add(&i(1)), -big);
    }
}
