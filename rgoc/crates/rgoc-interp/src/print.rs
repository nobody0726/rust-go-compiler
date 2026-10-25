//! 输出面（`print` / `println`）。
//!
//! # 格式表不是猜的
//!
//! 下面每一条都来自 `go1.27.1` 的实测（`.probe` 用例 + `od -c` 逐字节看）：
//!
//! | 输入 | 输出 |
//! |---|---|
//! | `print(1, 2)` | `12` —— **无分隔** |
//! | `println("a", "b")` | `a b\n` —— 空格分隔、末尾无空格 |
//! | `println(1000000.0)` | `1e+06`（`%g`，`eprec = 6`） |
//! | `println(0.0001)` | `0.0001`（`exp = -4` 仍是十进制） |
//! | `println(0.00001)` | `1e-05`（`exp = -5` 转指数） |
//! | `println(1e-7)` | `1e-07` —— 指数**至少两位** |
//! | `println(float32(1)/3)` | `0.33333334`（按 `f32` 精度取最短往返） |
//! | `println(complex(-1,-2))` | `(-1-2i)` |
//! | `println(nilPtr)` / `println(nilMap)` | `0x0` |
//! | `println(nilIface)` | `(0x0,0x0)` |
//! | `println(nilSlice)` | `[0/0]0x0` |
//! | `println('A')` / `println(byte(65))` | `65` —— rune/byte 打十进制 |
//!
//! # 两个刻意的限制
//!
//! 1. `print` / `println` 的合法实参**不含** struct / array / slice 字面量的
//!    元素展开 —— 实测 `println(S{})` 与 `println([2]int{})` 是**编译错误**
//!    （`illegal types for operand`）。所以这里不需要复合值的递归格式化。
//! 2. 指针 / 非 nil 接口 / 非空 slice 打出来的地址是**运行时地址**，
//!    官方也不可复现。我们打一个固定占位（`0x0` 或 `0xc000000000`）：
//!    这类用例本来就不可能逐字节对撞，写进 `unsupported` 比假装一致诚实。
//!
//! # 浮点的 `%g`
//!
//! 官方 `print` 的浮点走 `strconv.FormatFloat(f, 'g', -1, bits)`：
//! **最短往返数字** + 「指数 < -4 或 ≥ 6 时用指数形式」（`shortest ⇒ eprec = 6`）。

use rgoc_const::BasicId;
use rgoc_hir::types::ConstVal;
use rgoc_types::{TypeId, TypeTable};

use crate::ops::basic_of;
use crate::value::{NilKind, Value};

/// 常量 → 文本（`Const` 节点直接进 `print` 时用）。
#[must_use]
pub fn const_text(c: &ConstVal, tt: &TypeTable, typ: TypeId) -> String {
    match c {
        ConstVal::Bool(b) => bool_text(*b).to_string(),
        ConstVal::Int(i) => i.to_string(),
        ConstVal::Float(f) => format_float(*f, is_f32(tt, typ)),
        ConstVal::Complex(r, i) => format_complex(*r, *i),
        ConstVal::Str(s) => s.clone(),
    }
}

/// 一个 `print` / `println` 实参的文本。
#[must_use]
pub fn format_operand(v: &Value, tt: &TypeTable, typ: TypeId) -> String {
    match v {
        Value::Bool(b) => bool_text(*b).to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => format_float(*f, is_f32(tt, typ)),
        Value::Complex(r, i) => format_complex(*r, *i),
        Value::Str(s) => s.to_string(),
        Value::Ptr(_) => PTR_TEXT.to_string(),
        Value::Func(_) => PTR_TEXT.to_string(),
        Value::Map(_) => PTR_TEXT.to_string(),
        // 通道按指针口径打印（`print(c)` ⇒ `0xc0000180c0` 那种）；`nil` 通道走
        // 下面的 `Nil(_)` 分支 ⇒ `0x0`。
        Value::Chan(_) => PTR_TEXT.to_string(),
        Value::Slice(s) => {
            let p = if s.arr.is_empty() { "0x0" } else { PTR_TEXT };
            format!("[{}/{}]{}", s.len, s.cap, p)
        }
        Value::Nil(NilKind::Iface) => "(0x0,0x0)".to_string(),
        Value::Nil(NilKind::Slice) => "[0/0]0x0".to_string(),
        Value::Nil(_) => "0x0".to_string(),
        Value::Iface { typ: None, .. } => "(0x0,0x0)".to_string(),
        Value::Iface { typ: Some(_), .. } => format!("({PTR_TEXT},{PTR_TEXT})"),
        // 不可达：`print` 的实参类型面不含复合值（见模块文档第 1 条）。
        Value::Array(_) | Value::Struct(_) => PTR_TEXT.to_string(),
    }
}

/// 指针 / 非空聚合体的占位地址（真地址不可复现，见模块文档第 2 条）。
const PTR_TEXT: &str = "0xc000000000";

fn bool_text(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

fn is_f32(tt: &TypeTable, typ: TypeId) -> bool {
    basic_of(tt, typ) == Some(BasicId::Float32)
}

fn format_complex(re: f64, im: f64) -> String {
    let sign = if im.is_sign_negative() { '-' } else { '+' };
    format!(
        "({}{}{}i)",
        format_float(re, false),
        sign,
        format_float(im.abs(), false)
    )
}

/// Go 的 `strconv.FormatFloat(f, 'g', -1, bits)`。
#[must_use]
pub fn format_float(f: f64, is32: bool) -> String {
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 { "+Inf" } else { "-Inf" }.to_string();
    }
    let neg = f.is_sign_negative();
    let a = f.abs();
    // Rust 的 `{:e}` 给**最短往返**的指数形式（与 Go 的 `-1` 精度同源）。
    let s = if is32 {
        format!("{:e}", a as f32)
    } else {
        format!("{:e}", a)
    };
    let (mant, exp) = match s.split_once('e') {
        Some(p) => p,
        None => return s,
    };
    let exp: i32 = exp.parse().unwrap_or(0);
    let mut digits: String = mant.chars().filter(|c| *c != '.').collect();
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    // Go 的判据：`exp < -4 || exp >= 6`（shortest ⇒ eprec = 6）
    if !(-4..6).contains(&exp) {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        if exp >= 0 {
            out.push('+');
        } else {
            out.push('-');
        }
        let ae = exp.unsigned_abs();
        if ae < 10 {
            out.push('0');
        }
        out.push_str(&ae.to_string());
    } else if exp >= 0 {
        let ip = exp as usize + 1;
        if digits.len() <= ip {
            out.push_str(&digits);
            for _ in digits.len()..ip {
                out.push('0');
            }
        } else {
            out.push_str(&digits[..ip]);
            out.push('.');
            out.push_str(&digits[ip..]);
        }
    } else {
        out.push_str("0.");
        for _ in 0..(-exp - 1) {
            out.push('0');
        }
        out.push_str(&digits);
    }
    out
}

/// `panic: <值>` 里 `<值>` 的文本（`F18`：只比这一行，不比 traceback）。
///
/// 不带静态类型：未捕获 panic 走到这里时帧已经弹完，只剩值本身。
#[must_use]
pub fn panic_text(v: &Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        Value::Iface { typ: Some(_), data } => panic_text(data),
        Value::Iface { typ: None, .. } | Value::Nil(_) => "nil".to_string(),
        Value::Int(i) => i.to_string(),
        Value::Bool(b) => bool_text(*b).to_string(),
        Value::Float(f) => format_float(*f, false),
        Value::Complex(r, i) => format_complex(*r, *i),
        _ => "?".to_string(),
    }
}
