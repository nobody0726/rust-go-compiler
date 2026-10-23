//! 任意精度整数与运行时值 —— T-S1-03「值表示不能只在 64 位上凑巧成立」的载体。
//!
//! # 为什么要自己写一个最小 BigInt，而不是直接用 `i128`
//!
//! 三个 fixture 需要的精度是 **`int64` 极值 + 一个超出 `i64` 的常量**（`1 << 100`）。
//! `i128` 装得下 `1 << 100`，那为什么不用它？
//!
//! 因为 **`i128` 只能证明「比 i64 宽」，证明不了「按类型收敛的时机对」**。
//! 用 `i128` 时，「常量以任意精度存在」这件事实际上被悄悄退化成了
//! 「常量是 i128，只是比机器字宽一点」—— 于是将来遇到 `1 << 200` 时又要改一次表示，
//! 而 M3（常量 / 大整数策略）的设计前提是「宿主管理内存但仍监控运行预算」，
//! 预算上限该由**一处**说了算。本文件把这件事显式化：
//!
//! - 常量以 [`BigInt`]（符号 + `u32` 尾数，base 2^32）存在，**精度由分配点决定**；
//! - 收敛到具体类型只发生在 [`Val::to_int64`] 这一个函数里，且**溢出即报错**，
//!   不做静默截断（这正是 Go 报 `overflows` 的那一步）。
//!
//! # 为什么是 base 2^32 而不是 2^64
//!
//! `u32` 尾数让「乘 10 / 加小值」这类十进制转换在 `u64` 里做乘法时**不会溢出**
//! （`(2^32-1) * 10 + 9 < 2^64`），而 base 2^64 需要 `u128`，在 `-D warnings` 下要多一处
//! `#[allow]` 与更多边界推理。三个 fixture 用不到性能，这个取舍是划算的。

use std::fmt;

/// 任意精度有符号整数：符号 + base 2^32 尾数（小端，`mag` 末位非零，零表示为 `mag = []`）。
///
/// 只实现三个 fixture 需要到的运算。**刻意不实现** `Mul` / `Div` / `Cmp` 之外的
/// 完备算术 —— 未实现的运算不是「返回 0」，而是**根本不提供**，这样调用方无法误用。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BigInt {
    /// `true` 表示负数。零的 `neg` 恒为 `false`（`normalize` 保证）。
    neg: bool,
    /// 尾数，小端序，base 2^32。零为空。
    mag: Vec<u32>,
}

/// 十进制字符串解析失败。**不静默回退** —— 解析不了就报错，让调用方显式分类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// 字符串为空或只有符号。
    Empty,
    /// 出现了非十进制数字。
    NotDecimal(char),
    /// 除以 10 得到的商不是正数（逻辑上不该发生，属于内部不变式被破坏）。
    NonPositiveQuotient,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("空串（无数字）"),
            Self::NotDecimal(c) => write!(f, "非十进制字符 {c:?}"),
            Self::NonPositiveQuotient => f.write_str("商非正：BigInt 内部不变式被破坏"),
        }
    }
}

impl BigInt {
    /// 零。
    pub fn zero() -> Self {
        Self {
            neg: false,
            mag: Vec::new(),
        }
    }

    /// 由 `i64` 构造（用于字面量与类型化值）。
    pub fn from_i64(v: i64) -> Self {
        if v == 0 {
            return Self::zero();
        }
        let neg = v < 0;
        // `unsigned_abs` 对 i64::MIN 安全（不会溢出），而 `-v` 会在 i64::MIN 上 panic。
        let mut rest = v.unsigned_abs();
        let mut mag = Vec::new();
        while rest > 0 {
            mag.push((rest & 0xFFFF_FFFF) as u32);
            rest >>= 32;
        }
        Self { neg, mag }
    }

    /// 由十进制字符串构造。接受可选前导 `-`；不接受 `+`、不接受下划线分隔符
    /// （fixture 不需要，且 Go 源码里下划线是词法层的去掉的，这里不越界处理）。
    pub fn parse_decimal(s: &str) -> Result<Self, FormatError> {
        let (neg, digits) = match s.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, s),
        };
        if digits.is_empty() {
            return Err(FormatError::Empty);
        }
        let mut mag: Vec<u32> = Vec::new();
        for c in digits.chars() {
            let d = c.to_digit(10).ok_or(FormatError::NotDecimal(c))?;
            mul_add_small(&mut mag, 10, d);
        }
        let mut out = Self { neg, mag };
        out.normalize();
        Ok(out)
    }

    /// 是否为零。
    pub fn is_zero(&self) -> bool {
        self.mag.is_empty()
    }

    /// 是否为负。
    pub fn is_negative(&self) -> bool {
        self.neg
    }

    /// 十进制有效位数（零算 1 位，与 `ilog10` 的常见约定一致）。
    pub fn decimal_len(&self) -> usize {
        if self.is_zero() {
            return 1;
        }
        let mut cur = self.mag.clone();
        let mut n = 0;
        while !cur.is_empty() {
            cur = div_small(&cur, 10);
            n += 1;
        }
        n
    }

    /// 左移 `bits` 位（`1 << 100` 的来源）。
    ///
    /// 移位量 ≥ 256 位时返回 `None`：三个 fixture 用不到，且**静默产生一个天文数字的
    /// 尾数分配**是 OOM 的好办法。显式拒绝。
    pub fn shl(&self, bits: u32) -> Option<Self> {
        if self.is_zero() {
            return Some(Self::zero());
        }
        if bits >= 256 {
            return None;
        }
        let whole = (bits / 32) as usize;
        let bits_in = bits % 32;
        // ⚠️ 小端存储的方向陷阱：左移 whole 个 limb 等于**乘 2^(32·whole)**，
        // 而小端里「乘 2^32」是把数值推向**更高索引** —— 所以零 limb 要插在**开头**。
        // （曾在这里反向改成「末尾追加」，结果 `1 << 100` 变成 16：等于把数**除以**了 2^96。
        //  与 `normalize` 的方向 bug 是同一类，凑在一起时症状极具迷惑性 ——
        //  值仍是合法整数，只有大常量才错。）
        let mut mag = vec![0u32; whole];
        if bits_in == 0 {
            mag.extend_from_slice(&self.mag);
        } else {
            let mut carry = 0u32;
            for &w in &self.mag {
                mag.push((w << bits_in) | carry);
                carry = w >> (32 - bits_in);
            }
            if carry != 0 {
                mag.push(carry);
            }
        }
        let mut out = Self { neg: self.neg, mag };
        out.normalize();
        Some(out)
    }

    /// 加法。异号时转为绝对值相减。
    pub fn add(&self, other: &Self) -> Self {
        if self.neg == other.neg {
            let mag = add_mag(&self.mag, &other.mag);
            let mut out = Self { neg: self.neg, mag };
            out.normalize();
            out
        } else {
            // 异号：结果符号取决于谁的绝对值大
            match cmp_mag(&self.mag, &other.mag) {
                std::cmp::Ordering::Equal => Self::zero(),
                std::cmp::Ordering::Greater => {
                    let mag = sub_mag(&self.mag, &other.mag);
                    let mut out = Self { neg: self.neg, mag };
                    out.normalize();
                    out
                }
                std::cmp::Ordering::Less => {
                    let mag = sub_mag(&other.mag, &self.mag);
                    let mut out = Self {
                        neg: other.neg,
                        mag,
                    };
                    out.normalize();
                    out
                }
            }
        }
    }

    /// 取相反数（零的相反数仍是零，符号不会脏）。
    pub fn neg(&self) -> Self {
        if self.is_zero() {
            return Self::zero();
        }
        Self {
            neg: !self.neg,
            mag: self.mag.clone(),
        }
    }

    /// 收敛到 `i64`，**溢出即 `None`**。
    ///
    /// 这一步对应 Go 报 `overflows` 的位置：`untyped constant` 在被赋给具体类型时收敛。
    /// 之所以在这里就把「溢出」变成一个显式信号而不是饱和或回绕：T-C-04
    /// （`printbig.go`）的冻结期望是 `int64` 两个极值，任何静默截断都会让它变成别的数，
    /// 而**一个错的数在多数用例里不会立刻暴露**。
    pub fn to_int64(&self) -> Option<i64> {
        if self.mag.len() > 2 {
            return None;
        }
        let mut v: u128 = 0;
        for (i, &w) in self.mag.iter().enumerate() {
            v |= (w as u128) << (32 * i);
        }
        if self.neg {
            // 2^63 恰好是 i64::MIN 的绝对值，故 `>=` 成立时才合法。
            if v > 1u128 << 63 {
                None
            } else {
                Some((v as i128).wrapping_neg() as i64)
            }
        } else if v > i64::MAX as u128 {
            None
        } else {
            Some(v as i64)
        }
    }
}

impl fmt::Display for BigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            return f.write_str("0");
        }
        let mut cur = self.mag.clone();
        let mut chunks: Vec<u32> = Vec::new();
        while !cur.is_empty() {
            let (q, r) = div_rem_small(&cur, 1_000_000_000);
            chunks.push(r);
            cur = q;
        }
        let mut s = String::new();
        if self.neg {
            s.push('-');
        }
        s.push_str(&chunks[chunks.len() - 1].to_string());
        for &c in chunks.iter().rev().skip(1) {
            s.push_str(&format!("{c:09}"));
        }
        f.write_str(&s)
    }
}

/// 尾数乘小值再加小值（base 2^32）。`(2^32-1)*10 + 9 < 2^64`，不会溢出 `u64`。
fn mul_add_small(mag: &mut Vec<u32>, mul: u32, add: u32) {
    let mut carry = add as u64;
    for w in mag.iter_mut() {
        let t = (*w as u64) * (mul as u64) + carry;
        *w = (t & 0xFFFF_FFFF) as u32;
        carry = t >> 32;
    }
    while carry > 0 {
        mag.push((carry & 0xFFFF_FFFF) as u32);
        carry >>= 32;
    }
}

/// 尾数除以小值，返回商（高精度在前，低位在后）。
fn div_small(mag: &[u32], d: u32) -> Vec<u32> {
    div_rem_small(mag, d).0
}

/// 尾数除以小值，返回 `(商, 余)`。
///
/// **注意尾数是小端**（`mag[0]` 是最低位），所以「去掉商的高位零」=
/// **从末尾弹出**，不是从开头 `drain`。（第一版就是在这里搞错了方向 ——
/// 小端数组按大端去零会把**有效数字**删掉，`1 << 100` 被截成 `16`。）
fn div_rem_small(mag: &[u32], d: u32) -> (Vec<u32>, u32) {
    let mut q = vec![0u32; mag.len()];
    let mut rem = 0u64;
    for i in (0..mag.len()).rev() {
        let cur = (rem << 32) | (mag[i] as u64);
        q[i] = (cur / d as u64) as u32;
        rem = cur % d as u64;
    }
    while q.last() == Some(&0) {
        q.pop();
    }
    (q, rem as u32)
}

/// 尾数相加（同号情形）。
fn add_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    let n = a.len().max(b.len());
    let mut out = Vec::with_capacity(n + 1);
    let mut carry = 0u64;
    for i in 0..n {
        let x = *a.get(i).unwrap_or(&0) as u64;
        let y = *b.get(i).unwrap_or(&0) as u64;
        let s = x + y + carry;
        out.push((s & 0xFFFF_FFFF) as u32);
        carry = s >> 32;
    }
    if carry > 0 {
        out.push(carry as u32);
    }
    out
}

/// 尾数相减（要求 `a >= b`，即 `cmp_mag(a, b) != Less`）。
fn sub_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    debug_assert!(cmp_mag(a, b) != std::cmp::Ordering::Less);
    let mut out = Vec::with_capacity(a.len());
    let mut borrow = 0i64;
    // 用 zip 而非索引：`a` 比 `b` 长时把 `b` 的缺失项当 0（借位链需要这一步）
    for (&x, &y) in a.iter().zip(b.iter().chain(std::iter::repeat(&0))) {
        let mut d = x as i64 - y as i64 - borrow;
        if d < 0 {
            d += 1 << 32;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out.push(d as u32);
    }
    out
}

/// 尾数比较。
fn cmp_mag(a: &[u32], b: &[u32]) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if a.len() != b.len() {
        return a.len().cmp(&b.len());
    }
    for i in (0..a.len()).rev() {
        match a[i].cmp(&b[i]) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    Ordering::Equal
}

impl BigInt {
    /// 归一化：**从末尾弹出尾数的高位零 limb**（小端存储，高位在末尾），并让零的符号恒为正。
    ///
    /// 方向搞反的后果很隐蔽：按大端习惯去「前导零」会把有效数字删掉，
    /// 而剩下的**仍然是个合法整数** —— 小数值全对，只有 `1 << 100` 这种跨 limb 的出错。
    fn normalize(&mut self) {
        while self.mag.last() == Some(&0) {
            self.mag.pop();
        }
        if self.mag.is_empty() {
            self.neg = false;
        }
    }
}

/// 运行时值 —— 解释器与 SSA 求值共用的结果类型。
///
/// 只有三个 fixture 用到的三种。**刻意的设计**：内建 print 家族的实参在这里就已经是
/// 具体的 [`Val`]，而不是 HIR 的 [`crate::Expr`] —— 这样「内建调用边界」在哪里一目了然
/// （`M0-plan.md` T42 要求记录的三件事之一）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Val {
    /// 整数。`BigInt` 而非 `i64`：printbig 的极值与「未收敛的常量」都要装得下。
    Int(BigInt),
    /// 字符串（不可变，`M5` 的正式 HIR 同样是 string 不可变）。
    Str(String),
    /// 无值（内建 `println` 的返回）。
    Void,
}

impl Val {
    /// 按内建 `print` 家族的语义渲染成字节。
    ///
    /// **实测依据（2026-10-07，容器内 go1.27.1，`od -c` 逐字节）**：
    ///
    /// ```text
    /// $ go run p.go >out.txt 2>err.txt
    /// --- stdout(od) --- 0000000            ← 长度为 0
    /// --- stderr(od) --- 0000000   3  \n
    /// ```
    ///
    /// 结论有三条，都写进代码而不是注释里的「备忘」，因为它们各自都对应一个**会写错**的写法：
    ///
    /// 1. `print` / `println` 走 **stderr**（不是 stdout）—— 故本函数由调用方接到 stderr。
    /// 2. `println` 在**每个操作数之间**加空格并在**末尾**加 `\n`；`print` 完全不加。
    /// 3. 整数按十进制渲染，**不做千位分隔**；负号在前。
    pub fn render(&self) -> String {
        match self {
            Self::Int(i) => i.to_string(),
            Self::Str(s) => s.clone(),
            Self::Void => String::new(),
        }
    }
}

impl fmt::Display for Val {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 零的三种构造都相等且符号为正() {
        let a = BigInt::zero();
        let b = BigInt::from_i64(0);
        let c = BigInt::parse_decimal("0").expect("0 应可解析");
        let d = BigInt::parse_decimal("-0").expect("-0 应可解析");
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_eq!(a, d);
        // -0 归一化成 0：符号必须被清掉，否则 Display 会输出 "-0"
        assert!(!d.is_negative());
        assert_eq!(d.to_string(), "0");
    }

    #[test]
    fn i64_极值往返无损() {
        // 这两个值就是 T-C-04（printbig.go）的冻结期望，装不进去就说明表示有 bug
        for v in [i64::MAX, i64::MIN, 0, -1, 1, 1 << 62, -(1 << 62)] {
            let b = BigInt::from_i64(v);
            assert_eq!(b.to_int64(), Some(v), "i64 {v} 应无损往返");
        }
    }

    #[test]
    fn i64_最小值的绝对值是_2_63_而不是溢出() {
        // 这是 from_i64 里用 unsigned_abs 而非 -v 的原因：
        // 后者在 i64::MIN 上会 panic。
        let b = BigInt::from_i64(i64::MIN);
        assert!(b.is_negative());
        assert_eq!(b.to_string(), "-9223372036854775808");
    }

    #[test]
    fn 移位_1_左移_100_位_装得下且不可收敛到_i64() {
        let one = BigInt::from_i64(1);
        let big = one.shl(100).expect("100 位应允许");
        assert_eq!(
            big.to_string(),
            "1267650600228229401496703205376",
            "应与 Go 报出的 untyped constant 字面量完全一致"
        );
        // 关键：它装得下 BigInt，但**装不下 i64** —— 这就是 T-S1-03 的分界
        assert_eq!(big.to_int64(), None);
        assert_eq!(big.decimal_len(), 31);
    }

    #[test]
    fn 移位超过预算显式拒绝而不是巨额分配() {
        let one = BigInt::from_i64(1);
        assert!(one.shl(255).is_some(), "255 位仍在预算内");
        assert_eq!(one.shl(256), None, "≥256 位必须显式拒绝");
    }

    #[test]
    fn 零移位仍是零() {
        assert_eq!(BigInt::zero().shl(100), Some(BigInt::zero()));
    }

    #[test]
    fn 尾数跨_limb_边界的加法不进位丢失() {
        // (2^32-1) + 1 迫使尾数进位；再做 1000 次跨边界加法
        let mut b = BigInt::parse_decimal("4294967295").expect("可解析"); // 2^32-1
        for _ in 0..1000 {
            b = b.add(&BigInt::from_i64(1));
        }
        assert_eq!(b.to_string(), "4294968295");
        assert_eq!(b.to_int64(), Some(4294968295));
    }

    #[test]
    fn 异号相减取绝对值大的符号() {
        let a = BigInt::from_i64(5);
        let b = BigInt::from_i64(-3);
        assert_eq!(a.add(&b).to_string(), "2");
        assert_eq!(b.add(&a).to_string(), "2");
        assert_eq!(b.add(&b).to_string(), "-6");
        // 异号且绝对值相等 ⇒ 精确为零，符号必须是正
        let z = BigInt::from_i64(4).add(&BigInt::from_i64(-4));
        assert!(z.is_zero());
        assert!(!z.is_negative());
        assert_eq!(z.to_string(), "0");
    }

    #[test]
    fn 负数减法跨越_limb_借位() {
        // 2^33 - 1 借位后仍要正确
        let a = BigInt::from_i64(1).shl(33).expect("ok");
        let b = BigInt::from_i64(-1);
        let s = a.add(&b);
        assert_eq!(s.to_string(), "8589934591");
        assert_eq!(s.to_int64(), Some(8589934591));
    }

    #[test]
    fn 取相反数零仍是零() {
        assert_eq!(BigInt::zero().neg(), BigInt::zero());
        assert_eq!(BigInt::from_i64(5).neg().to_string(), "-5");
    }

    #[test]
    fn 十进制解析与渲染往返() {
        for s in [
            "0",
            "1",
            "9",
            "10",
            "255",
            "4294967295",
            "4294967296",
            "-1",
            "-9223372036854775808",
            "9223372036854775807",
            "1267650600228229401496703205376",
        ] {
            let b = BigInt::parse_decimal(s).expect("应可解析");
            assert_eq!(b.to_string(), s, "{s} 应原样往返");
        }
    }

    #[test]
    fn 解析失败显式分类而不静默回退() {
        assert_eq!(BigInt::parse_decimal(""), Err(FormatError::Empty));
        assert_eq!(BigInt::parse_decimal("-"), Err(FormatError::Empty));
        assert_eq!(
            BigInt::parse_decimal("12a"),
            Err(FormatError::NotDecimal('a'))
        );
        assert_eq!(
            BigInt::parse_decimal("0x10"),
            Err(FormatError::NotDecimal('x'))
        );
        // 错误信息里带上下文，便于 spike 记录里直接引用
        assert!(!FormatError::NotDecimal('x').to_string().is_empty());
    }

    #[test]
    fn 值渲染按内建_print_语义() {
        assert_eq!(Val::Int(BigInt::from_i64(3)).render(), "3");
        assert_eq!(Val::Int(BigInt::from_i64(-5)).render(), "-5");
        assert_eq!(Val::Str("hello".to_string()).render(), "hello");
        assert_eq!(Val::Void.render(), "");
        // Display 与 render 必须一致（spike 的断言会同时用两者）
        let v = Val::Int(BigInt::from_i64(42));
        assert_eq!(v.to_string(), v.render());
    }
}
