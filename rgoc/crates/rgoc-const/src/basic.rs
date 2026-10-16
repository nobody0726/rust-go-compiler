//! 基本类型标识（M3/Task 8，`T-TY-12`）。
//!
//! # 为什么这个枚举住在 `rgoc-const` 而不是 `rgoc-types`（`D-M3-16`）
//!
//! `Representable(&Const, BasicId, &Sizes)` 与 `convert_const` 都要吃它，
//! 而它们住在 `rgoc-const`。若定义在 `rgoc-types`，`rgoc-const` 就得反向依赖
//! `rgoc-types`，而后者又依赖前者 ⇒ **成环**（Cargo 直接拒绝构建）。
//! 所以定义落在这里，`rgoc-types` 只做 `pub use rgoc_const::BasicId`。
//!
//! # 变体集 = `go/types` 的 `BasicKind` **去掉 `Invalid`**
//!
//! 25 个，拼写与 `docs/contracts/C7-type-dump.md` §4.1 **逐字一致**
//! （`as_str()` 就是那边 dump 出来的文本，是两侧逐字节对撞的接口）。
//!
//! ⚠️ **`byte` / `rune` / `error` 不是独立变体**（C7 §4.1 的实测订正）：
//! 官方 `Byte = Uint8`、`Rune = Int32` 是**同一个 `*Basic`**，`error` 是 `Named`。
//! 所以 `T-TY-12` 的计数断言是 **25**。
//!
//! ⚠️ `invalid` 也**不在这里**：它是 `Type` 层的一个变体（Task 13），
//! 不是 `BasicKind` —— C7 给它单独一个 `invalid` 标签。

/// 基本类型标识（25 个）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum BasicId {
    /// `bool`
    Bool,
    /// `int`（**平台相关宽度**，由 [`crate::sizes::Sizes`] 给出）
    Int,
    /// `int8`
    Int8,
    /// `int16`
    Int16,
    /// `int32`（`rune` 是它的别名 ⇒ dump 成 `basic int32`）
    Int32,
    /// `int64`
    Int64,
    /// `uint`
    Uint,
    /// `uint8`（`byte` 是它的别名 ⇒ dump 成 `basic uint8`）
    Uint8,
    /// `uint16`
    Uint16,
    /// `uint32`
    Uint32,
    /// `uint64`
    Uint64,
    /// `uintptr`
    Uintptr,
    /// `float32`
    Float32,
    /// `float64`
    Float64,
    /// `complex64`
    Complex64,
    /// `complex128`
    Complex128,
    /// `string`
    String,
    /// `unsafe.Pointer`
    UnsafePointer,
    /// `untyped bool`
    UntypedBool,
    /// `untyped int`
    UntypedInt,
    /// `untyped rune`（**rune 字面量的类型**，`T-TY-3b` 靠它区分 `'a'` 与 `97`）
    UntypedRune,
    /// `untyped float`
    UntypedFloat,
    /// `untyped complex`
    UntypedComplex,
    /// `untyped string`
    UntypedString,
    /// `untyped nil`
    UntypedNil,
}

impl BasicId {
    /// 全部 25 个，**按声明顺序**（= `C7` §4.1 的表格顺序）。
    pub const ALL: [BasicId; 25] = [
        BasicId::Bool,
        BasicId::Int,
        BasicId::Int8,
        BasicId::Int16,
        BasicId::Int32,
        BasicId::Int64,
        BasicId::Uint,
        BasicId::Uint8,
        BasicId::Uint16,
        BasicId::Uint32,
        BasicId::Uint64,
        BasicId::Uintptr,
        BasicId::Float32,
        BasicId::Float64,
        BasicId::Complex64,
        BasicId::Complex128,
        BasicId::String,
        BasicId::UnsafePointer,
        BasicId::UntypedBool,
        BasicId::UntypedInt,
        BasicId::UntypedRune,
        BasicId::UntypedFloat,
        BasicId::UntypedComplex,
        BasicId::UntypedString,
        BasicId::UntypedNil,
    ];

    /// dump 文本（`C7` §4.1 的 `basic名`）。**契约的一部分**，改动等于契约变更。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            BasicId::Bool => "bool",
            BasicId::Int => "int",
            BasicId::Int8 => "int8",
            BasicId::Int16 => "int16",
            BasicId::Int32 => "int32",
            BasicId::Int64 => "int64",
            BasicId::Uint => "uint",
            BasicId::Uint8 => "uint8",
            BasicId::Uint16 => "uint16",
            BasicId::Uint32 => "uint32",
            BasicId::Uint64 => "uint64",
            BasicId::Uintptr => "uintptr",
            BasicId::Float32 => "float32",
            BasicId::Float64 => "float64",
            BasicId::Complex64 => "complex64",
            BasicId::Complex128 => "complex128",
            BasicId::String => "string",
            BasicId::UnsafePointer => "unsafe_pointer",
            BasicId::UntypedBool => "untyped_bool",
            BasicId::UntypedInt => "untyped_int",
            BasicId::UntypedRune => "untyped_rune",
            BasicId::UntypedFloat => "untyped_float",
            BasicId::UntypedComplex => "untyped_complex",
            BasicId::UntypedString => "untyped_string",
            BasicId::UntypedNil => "untyped_nil",
        }
    }

    /// 是否是无类型（`untyped *`）的基本类型。
    #[must_use]
    pub fn is_untyped(self) -> bool {
        matches!(
            self,
            BasicId::UntypedBool
                | BasicId::UntypedInt
                | BasicId::UntypedRune
                | BasicId::UntypedFloat
                | BasicId::UntypedComplex
                | BasicId::UntypedString
                | BasicId::UntypedNil
        )
    }

    /// 是否是整数类型（含 `untyped int` / `untyped rune`）。
    #[must_use]
    pub fn is_integer(self) -> bool {
        matches!(
            self,
            BasicId::Int
                | BasicId::Int8
                | BasicId::Int16
                | BasicId::Int32
                | BasicId::Int64
                | BasicId::Uint
                | BasicId::Uint8
                | BasicId::Uint16
                | BasicId::Uint32
                | BasicId::Uint64
                | BasicId::Uintptr
                | BasicId::UntypedInt
                | BasicId::UntypedRune
        )
    }

    /// 是否是无符号整数类型。
    #[must_use]
    pub fn is_unsigned(self) -> bool {
        matches!(
            self,
            BasicId::Uint
                | BasicId::Uint8
                | BasicId::Uint16
                | BasicId::Uint32
                | BasicId::Uint64
                | BasicId::Uintptr
        )
    }

    /// 是否是浮点类型。
    #[must_use]
    pub fn is_float(self) -> bool {
        matches!(
            self,
            BasicId::Float32 | BasicId::Float64 | BasicId::UntypedFloat
        )
    }

    /// 是否是复数类型。
    #[must_use]
    pub fn is_complex(self) -> bool {
        matches!(
            self,
            BasicId::Complex64 | BasicId::Complex128 | BasicId::UntypedComplex
        )
    }

    /// 是否是数值类型（整数 ∪ 浮点 ∪ 复数）。
    #[must_use]
    pub fn is_numeric(self) -> bool {
        self.is_integer() || self.is_float() || self.is_complex()
    }

    /// 是否是字符串类型。
    #[must_use]
    pub fn is_string(self) -> bool {
        matches!(self, BasicId::String | BasicId::UntypedString)
    }

    /// 是否是布尔类型。
    #[must_use]
    pub fn is_bool(self) -> bool {
        matches!(self, BasicId::Bool | BasicId::UntypedBool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `T-TY-12`：**恰好 25 个**（不是 26 —— `Invalid` 走 `Type::Invalid`）。
    #[test]
    fn 恰好_25_个且无重复() {
        assert_eq!(BasicId::ALL.len(), 25);
        let mut s = BasicId::ALL.to_vec();
        s.sort();
        s.dedup();
        assert_eq!(s.len(), 25, "有重复变体");
    }

    /// `as_str` 的拼写就是 `C7` §4.1 那张表 —— 两侧逐字节对撞的接口。
    #[test]
    fn as_str_与_c7_逐字一致() {
        let want = [
            "bool",
            "int",
            "int8",
            "int16",
            "int32",
            "int64",
            "uint",
            "uint8",
            "uint16",
            "uint32",
            "uint64",
            "uintptr",
            "float32",
            "float64",
            "complex64",
            "complex128",
            "string",
            "unsafe_pointer",
            "untyped_bool",
            "untyped_int",
            "untyped_rune",
            "untyped_float",
            "untyped_complex",
            "untyped_string",
            "untyped_nil",
        ];
        let got: Vec<&str> = BasicId::ALL.iter().map(|b| b.as_str()).collect();
        assert_eq!(got, want);
    }

    /// 分类谓词：计数写死，任一变体被误分类就红。
    #[test]
    fn 分类谓词() {
        let untyped: Vec<BasicId> = BasicId::ALL
            .iter()
            .copied()
            .filter(|b| b.is_untyped())
            .collect();
        assert_eq!(untyped.len(), 7, "7 个 untyped_*");

        let ints: Vec<BasicId> = BasicId::ALL
            .iter()
            .copied()
            .filter(|b| b.is_integer())
            .collect();
        assert_eq!(
            ints.len(),
            13,
            "5 个有符号（int/int8..int64）+ 6 个无符号 + untyped_int + untyped_rune"
        );

        let unsigned: Vec<BasicId> = BasicId::ALL
            .iter()
            .copied()
            .filter(|b| b.is_unsigned())
            .collect();
        assert_eq!(unsigned.len(), 6, "uint/uint8..uint64/uintptr");

        let floats: Vec<BasicId> = BasicId::ALL
            .iter()
            .copied()
            .filter(|b| b.is_float())
            .collect();
        assert_eq!(floats.len(), 3, "float32/float64/untyped_float");

        let cplx: Vec<BasicId> = BasicId::ALL
            .iter()
            .copied()
            .filter(|b| b.is_complex())
            .collect();
        assert_eq!(cplx.len(), 3, "complex64/complex128/untyped_complex");

        let numeric: Vec<BasicId> = BasicId::ALL
            .iter()
            .copied()
            .filter(|b| b.is_numeric())
            .collect();
        assert_eq!(numeric.len(), 19, "13 整数 + 3 浮点 + 3 复数");

        let strings: Vec<BasicId> = BasicId::ALL
            .iter()
            .copied()
            .filter(|b| b.is_string())
            .collect();
        assert_eq!(strings, [BasicId::String, BasicId::UntypedString]);

        let bools: Vec<BasicId> = BasicId::ALL
            .iter()
            .copied()
            .filter(|b| b.is_bool())
            .collect();
        assert_eq!(bools, [BasicId::Bool, BasicId::UntypedBool]);

        // 反例：谓词之间不重叠
        assert!(!BasicId::String.is_numeric());
        assert!(!BasicId::Bool.is_numeric());
        assert!(!BasicId::UnsafePointer.is_numeric());
        assert!(!BasicId::UntypedNil.is_numeric());
        assert!(!BasicId::Float64.is_integer(), "浮点不是整数类型");
        assert!(
            !BasicId::Int32.is_complex(),
            "整数不是复数（虽然复数能取其实部）"
        );
    }
}
