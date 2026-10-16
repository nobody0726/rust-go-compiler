//! 常量值表示与字面量构造（M3/Task 5）。
//!
//! # 值域（`D-M3-4`）
//!
//! ```text
//! Const = Int(BigInt) | Float(BigRat) | Complex{re, im} | Str(Vec<u8>) | Bool(bool) | Unknown
//! ```
//!
//! **没有「精度」这一维** —— 这正是 `D-M3-4` 的「不引入实现限制」：整数任意精度、
//! 浮点是**精确有理数**。官方的 `constant.Value` 有 `int64/uint64/float32/float64`
//! 这些「已经落到机器类型」的形态，那是**转换的产物**，本层不表示它们：
//! 转换只产出**判定**（[`RoundResult`]）与**诊断**，值本身仍是精确的
//! （`T-TY-47`：溢出后「常量仍是精确原值」）。
//!
//! # `Optional` 与无类型常量
//!
//! 官方 `constant` 在运算结果不确定时（如 `1/0`）给出 `Unknown`。这里的
//! [`Const::Unknown`] 有两类来源：① 语义上的「无值」；② 词法层已判 `bad`
//! 的畸形字面量（[`rgoc_ast::BasicLit::bad`]）。两者的共同性质是**不能再参与折叠**。
//!
//! # `Str` 装字节不装 `String`（`D-M1-12` 的延续）
//!
//! `"a\xffb"` 是**合法** Go 源码，其常量值含非 UTF-8 字节。用 `String` 装会被
//! `from_utf8_lossy` 悄悄换成一个替换字符 —— 那是**静默数据损坏**。用 `Vec<u8>`。

use rgoc_ast::{BasicLit, LitKind};

use crate::bigint::BigInt;
use crate::bigrat::BigRat;

/// 一个常量值。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Const {
    /// 整数常量（任意精度）
    Int(BigInt),
    /// 浮点常量（**精确**有理数，尚未量化到具体浮点类型）
    Float(BigRat),
    /// 复数常量（实部与虚部各自精确）
    Complex {
        /// 实部
        re: BigRat,
        /// 虚部
        im: BigRat,
    },
    /// 字符串常量（**字节**，不保证是 UTF-8）
    Str(Vec<u8>),
    /// 布尔常量
    Bool(bool),
    /// 无值 / 不可用（见模块头）
    Unknown,
}

/// 从 AST 的字面量节点构造常量。
///
/// # 为什么返回 `Const` 而不是 `Result`
///
/// 字面量的**合法性**是词法层判过的（[`rgoc_ast::BasicLit::bad`]）。到这里
/// 「解析失败」只剩两种情况：① `bad == true`；② **本函数覆盖不到的形式**
/// （例如尚未支持的浮点写法）。两者都归 [`Const::Unknown`] —— 但它们是**不同**的
/// 东西，**调用方（类型检查器）必须把「非 `bad` 却得 `Unknown`」当成内部不一致
/// 上报**，不能当成「这个常量没有值」继续算下去。这条约定由
/// [`const_from_literal`] 的文档承担，因为这里没有诊断通道（签名由
/// `M3-plan.md` Task 5 给定）。
pub fn const_from_literal(lit: &BasicLit) -> Const {
    if lit.bad {
        return Const::Unknown;
    }
    match lit.kind {
        LitKind::Int => parse_int(&lit.value).map_or(Const::Unknown, Const::Int),
        LitKind::Float => parse_float(&lit.value).map_or(Const::Unknown, Const::Float),
        LitKind::Imag => {
            // `2i` → 纯虚复数（虚部是数值部分）
            let text = lit.value.strip_suffix('i').unwrap_or("");
            parse_float(text).map_or(Const::Unknown, |im| Const::Complex {
                re: BigRat::from_i64(0),
                im,
            })
        }
        LitKind::Rune => unquote_rune(&lit.value)
            .map_or(Const::Unknown, |cp| Const::Int(BigInt::from_u64(cp as u64))),
        LitKind::String => unquote_string(&lit.value).map_or(Const::Unknown, Const::Str),
    }
}

// ── 数值字面量 ────────────────────────────────────────────────────────────

/// 整数字面量 → `BigInt`。进制规则照 `spec:L633-739`。
fn parse_int(s: &str) -> Option<BigInt> {
    if let Some(rest) = strip_prefix_ci(s, "0x") {
        BigInt::from_hex(rest)
    } else if let Some(rest) = strip_prefix_ci(s, "0o") {
        BigInt::from_oct(rest)
    } else if let Some(rest) = strip_prefix_ci(s, "0b") {
        BigInt::from_bin(rest)
    } else if let Some(rest) = s.strip_prefix('0') {
        // 前导 `0` ⇒ **八进制**（`017 == 15`）；单独一个 `0` 就是零。
        if rest.is_empty() {
            Some(BigInt::zero())
        } else {
            BigInt::from_oct(&rest.replace('_', ""))
        }
    } else {
        BigInt::from_dec(s)
    }
}

/// 浮点字面量 → `BigRat`（十进制十进制记法或 `0x…p…` 十六进制记法）。
fn parse_float(s: &str) -> Option<BigRat> {
    if strip_prefix_ci(s, "0x").is_some() {
        parse_hex_float(s)
    } else {
        BigRat::from_dec(s)
    }
}

/// `0x MANTISSA p EXP` —— 十六进制浮点，指数**必需**（`0x1.8p1 == 3`）。
fn parse_hex_float(s: &str) -> Option<BigRat> {
    let body = strip_prefix_ci(s, "0x")?;
    let (mant, exp_text) = body.split_once(['p', 'P'])?;
    let exp = parse_signed_dec(exp_text)?;
    let (int_part, frac_part) = match mant.split_once('.') {
        Some((a, b)) => (a, b),
        None => (mant, ""),
    };
    let int_digits = strip_us(int_part, |c| c.is_ascii_hexdigit())?;
    let frac_digits = strip_us(frac_part, |c| c.is_ascii_hexdigit())?;
    if int_digits.is_empty() && frac_digits.is_empty() {
        return None;
    }
    let m = BigInt::from_hex(&format!("{int_digits}{frac_digits}"))?;
    // value = m × 2^exp / 16^frac_len = m × 2^(exp − 4·frac_len)
    let shift = exp - 4 * frac_digits.len() as i64;
    let (num, den) = if shift >= 0 {
        (m.shl(shift as u32), BigInt::from_i64(1))
    } else {
        (m, BigInt::from_i64(1).shl((-shift) as u32))
    };
    BigRat::new(num, den)
}

/// `[±] 十进制数位`（允许 `_`）。注意这里**不复用** [`BigRat::from_dec`]：
/// 指数部分不是「浮点」，`1e+3` 的 `+3` 走的是整数语法。
fn parse_signed_dec(s: &str) -> Option<i64> {
    let (neg, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let digits = strip_us(digits, |c| c.is_ascii_digit())?;
    if digits.is_empty() {
        return None;
    }
    let v: i64 = digits.parse().ok()?;
    Some(if neg { -v } else { v })
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        s.get(prefix.len()..)
    } else {
        None
    }
}

/// 校验「数位 + `_`」并去掉 `_`；空串合法（`.5` 的整数部分为空）。
fn strip_us(s: &str, is_digit: fn(char) -> bool) -> Option<String> {
    if s.is_empty() {
        return Some(String::new());
    }
    if s.starts_with('_') || s.ends_with('_') || s.contains("__") {
        return None;
    }
    if !s.chars().all(|c| is_digit(c) || c == '_') {
        return None;
    }
    Some(s.chars().filter(|c| *c != '_').collect())
}

// ── 字符串 / rune 字面量 ──────────────────────────────────────────────────

/// 字符串字面量 → 字节。`raw` **含两侧引号**。
pub(crate) fn unquote_string(raw: &str) -> Option<Vec<u8>> {
    let inner = match raw.as_bytes().first() {
        Some(b'`') if raw.ends_with('`') => {
            // 原始字符串：**不解释转义**，但 `\r` 按 spec 丢弃
            return Some(
                raw.as_bytes()[1..raw.len() - 1]
                    .iter()
                    .copied()
                    .filter(|&b| b != b'\r')
                    .collect(),
            );
        }
        Some(b'"') if raw.ends_with('"') => &raw.as_bytes()[1..raw.len() - 1],
        _ => return None,
    };
    let mut out = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        if inner[i] == b'\\' {
            i += 1;
            escape(inner, &mut i, &mut out)?;
        } else {
            out.push(inner[i]);
            i += 1;
        }
    }
    Some(out)
}

/// rune 字面量 → 码点。`raw` **含两侧单引号**。
pub(crate) fn unquote_rune(raw: &str) -> Option<u32> {
    let inner = raw.strip_prefix('\'')?.strip_suffix('\'')?;
    let b = inner.as_bytes();
    if b.is_empty() {
        return None;
    }
    if b[0] != b'\\' {
        // 源码里直接写字符（可能多字节）
        let mut it = inner.chars();
        let c = it.next()?;
        return if it.next().is_some() {
            None
        } else {
            Some(c as u32)
        };
    }
    let mut i = 1;
    let mut out = Vec::new();
    // `\xNN` 与 `\OOO` 产**单字节**，其值就是码点（`'\xff' == 255`）；
    // `\uNNNN` / `\UNNNNNNNN` 与其余单字符转义产字符。
    let single_byte = matches!(b[1], b'x' | b'0'..=b'7');
    escape(b, &mut i, &mut out)?;
    if i != b.len() {
        return None;
    }
    if single_byte {
        return if out.len() == 1 {
            Some(out[0] as u32)
        } else {
            None
        };
    }
    let text = std::str::from_utf8(&out).ok()?;
    let mut it = text.chars();
    let c = it.next()?;
    if it.next().is_some() {
        return None;
    }
    Some(c as u32)
}

/// 解一个转义序列（`i` 指向反斜杠**之后**的第一字节，返回时指向其**之后**）。
fn escape(b: &[u8], i: &mut usize, out: &mut Vec<u8>) -> Option<()> {
    let c = *b.get(*i)?;
    *i += 1;
    match c {
        b'a' => out.push(0x07),
        b'b' => out.push(0x08),
        b'f' => out.push(0x0c),
        b'n' => out.push(0x0a),
        b'r' => out.push(0x0d),
        b't' => out.push(0x09),
        b'v' => out.push(0x0b),
        b'\\' => out.push(b'\\'),
        b'\'' => out.push(b'\''),
        b'"' => out.push(b'"'),
        b'x' => out.push(take_hex(b, i, 2)? as u8),
        b'0'..=b'7' => {
            let mut v = (c - b'0') as u32;
            for _ in 0..2 {
                let d = *b.get(*i)?;
                if !d.is_ascii_digit() || d > b'7' {
                    return None;
                }
                v = v * 8 + (d - b'0') as u32;
                *i += 1;
            }
            // 三位八进制最多 0o377 = 255
            out.push(u8::try_from(v).ok()?);
        }
        b'u' => push_utf8(take_hex(b, i, 4)? as u32, out)?,
        b'U' => push_utf8(take_hex(b, i, 8)? as u32, out)?,
        _ => return None,
    }
    Some(())
}

/// 吃掉 `n` 个十六进制位并返回其值。
fn take_hex(b: &[u8], i: &mut usize, n: usize) -> Option<u64> {
    let mut v = 0u64;
    for _ in 0..n {
        let d = *b.get(*i)?;
        let h = (d as char).to_digit(16)?;
        v = v * 16 + h as u64;
        *i += 1;
    }
    Some(v)
}

/// 码点按 UTF-8 追加（非法码点 / 代理区 ⇒ `None`）。
fn push_utf8(cp: u32, out: &mut Vec<u8>) -> Option<()> {
    let c = char::from_u32(cp)?;
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgoc_lex::{FileTable, Pos};

    fn pos() -> Pos {
        let mut ft = FileTable::new();
        let file = ft.intern("t.go");
        Pos {
            base: None,
            file,
            line: 1,
            col: 1,
        }
    }

    fn lit(kind: LitKind, value: &str) -> BasicLit {
        BasicLit {
            value: value.to_string(),
            kind,
            bad: false,
            pos: pos(),
        }
    }

    fn int(kind: LitKind, value: &str) -> BigInt {
        match const_from_literal(&lit(kind, value)) {
            Const::Int(v) => v,
            other => panic!("{value:?} 应为 Int，实得 {other:?}"),
        }
    }

    fn float(value: &str) -> BigRat {
        match const_from_literal(&lit(LitKind::Float, value)) {
            Const::Float(v) => v,
            other => panic!("{value:?} 应为 Float，实得 {other:?}"),
        }
    }

    fn str_bytes(value: &str) -> Vec<u8> {
        match const_from_literal(&lit(LitKind::String, value)) {
            Const::Str(v) => v,
            other => panic!("{value:?} 应为 Str，实得 {other:?}"),
        }
    }

    fn i(v: i64) -> BigInt {
        BigInt::from_i64(v)
    }

    /// `T-TY-32`：进制与下划线分隔。
    #[test]
    fn 整数字面量_进制与下划线() {
        assert_eq!(int(LitKind::Int, "1_000_000"), i(1_000_000));
        assert_eq!(int(LitKind::Int, "0x_FF"), i(255));
        assert_eq!(int(LitKind::Int, "0xFF"), i(255));
        assert_eq!(int(LitKind::Int, "0Xfe"), i(254));
        assert_eq!(int(LitKind::Int, "0o17"), i(15));
        assert_eq!(int(LitKind::Int, "0O17"), i(15));
        assert_eq!(int(LitKind::Int, "0b1010"), i(10));
        assert_eq!(int(LitKind::Int, "0B1010"), i(10));
        assert_eq!(int(LitKind::Int, "0"), i(0));
        assert_eq!(int(LitKind::Int, "017"), i(15), "前导 0 是八进制");
    }

    /// `T-TY-31`：长字面量**不截断**。
    #[test]
    fn 整数字面量_不截断() {
        assert_eq!(
            int(LitKind::Int, "1267650600228229401496703205376"),
            i(1).shl(100)
        );
        assert_eq!(int(LitKind::Int, "0x10000000000000000"), i(1).shl(64));
    }

    /// `T-TY-32`：rune 字面量 → 码点。
    #[test]
    fn rune_字面量() {
        assert_eq!(int(LitKind::Rune, "'x'"), i(120));
        assert_eq!(int(LitKind::Rune, "'\\u00e9'"), i(233));
        assert_eq!(int(LitKind::Rune, "'é'"), i(233), "源码里直接写多字节字符");
        assert_eq!(int(LitKind::Rune, "'\\xff'"), i(255), "单字节转义按字节值");
        assert_eq!(int(LitKind::Rune, "'\\n'"), i(10));
        assert_eq!(int(LitKind::Rune, "'\\''"), i(39));
        assert_eq!(int(LitKind::Rune, "'\\U0001F600'"), i(128512));
    }

    /// `T-TY-32`：解释型字符串的转义。
    #[test]
    fn 字符串字面量_解释型转义() {
        assert_eq!(str_bytes("\"a\\nb\""), b"a\nb".to_vec());
        assert_eq!(str_bytes("\"\\x00\\xff\""), vec![0x00, 0xff]);
        assert_eq!(str_bytes("\"\\u00e9\""), vec![0xc3, 0xa9]);
        assert_eq!(str_bytes("\"\\101\""), b"A".to_vec(), "三位八进制");
        assert_eq!(str_bytes("\"\\\"\""), b"\"".to_vec());
        assert_eq!(str_bytes("\"\""), Vec::<u8>::new());
    }

    /// **原始字符串不解释转义**（反斜杠就是反斜杠）且**丢弃 `\r`**。
    #[test]
    fn 字符串字面量_原始型() {
        assert_eq!(str_bytes("`a\\nb`"), b"a\\nb".to_vec());
        assert_eq!(
            str_bytes("`a\r\nb`"),
            b"a\nb".to_vec(),
            "原始串里的 CR 被丢弃"
        );
        assert_eq!(str_bytes("`\n`"), b"\n".to_vec());
    }

    /// 浮点：十进制与**十六进制浮点**都精确。
    #[test]
    fn 浮点字面量() {
        assert_eq!(float("1e3"), BigRat::from_i64(1000));
        assert_eq!(float("0.1"), BigRat::new(i(1), i(10)).unwrap());
        assert_eq!(float("1_0.2_5"), BigRat::new(i(1025), i(100)).unwrap());
        assert_eq!(float("0x1p-2"), BigRat::new(i(1), i(4)).unwrap());
        assert_eq!(float("0x1.8p1"), BigRat::from_i64(3));
        assert_eq!(float("1e-3"), BigRat::new(i(1), i(1000)).unwrap());
    }

    /// 虚数字面量 → 纯虚复数。
    #[test]
    fn 虚数字面量() {
        let got = const_from_literal(&lit(LitKind::Imag, "2i"));
        assert_eq!(
            got,
            Const::Complex {
                re: BigRat::from_i64(0),
                im: BigRat::from_i64(2)
            }
        );
        let got = const_from_literal(&lit(LitKind::Imag, "0.5i"));
        assert_eq!(
            got,
            Const::Complex {
                re: BigRat::from_i64(0),
                im: BigRat::new(i(1), i(2)).unwrap(),
            }
        );
    }

    /// 词法层已判 `bad` 的字面量 → `Unknown`（**不**猜一个近似值）。
    #[test]
    fn 畸形字面量是_unknown() {
        let mut l = lit(LitKind::Int, "0x");
        l.bad = true;
        assert_eq!(const_from_literal(&l), Const::Unknown);
        let mut l = lit(LitKind::String, "\"unterminated");
        l.bad = true;
        assert_eq!(const_from_literal(&l), Const::Unknown);
    }

    /// 布尔不是字面量（`true`/`false` 是标识符），但 `Const` 要能表示它。
    #[test]
    fn 布尔常量可表示() {
        assert_ne!(Const::Bool(false), Const::Bool(true));
        assert_eq!(Const::Bool(true), Const::Bool(true));
    }
}
