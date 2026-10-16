//! 平台尺寸（M3/Task 8，`F14`）。
//!
//! 只有三项**随平台变**的宽度：`int` / `uint` / `uintptr`。
//! 其余基本类型的宽度由语言/`IEEE-754` 钉死（`int8` 恒 8 位，`float64` 恒 64 位），
//! 所以**不建整张尺寸表** —— 一张大部分恒定的表会让人以为它有内容。
//!
//! # 为什么住在这里（`D-M3-16`）
//!
//! `representable()` 吃 `&Sizes`，而它住在 `rgoc-const` ⇒ 定义也落在这里，
//! `rgoc-types` 转导出。理由与 [`crate::basic::BasicId`] 同一个（解环）。
//!
//! # 首发目标
//!
//! 首发平台是 **Linux / arm64**（`AGENTS.md` §0），所以 [`HOST_ARM64`] 是
//! 三个 8。`Sizes` 是**值**而不是常量表 —— 32 位目标以后加一个关联常量即可，
//! 不用改任何签名。

/// 三项随平台变的基本类型宽度（**字节**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sizes {
    /// `int` 的宽度（字节）
    pub int: u32,
    /// `uint` 的宽度（字节）
    pub uint: u32,
    /// `uintptr` 的宽度（字节）
    pub uintptr: u32,
}

impl Sizes {
    /// 首发平台：`aarch64-unknown-linux-gnu` ⇒ `int` / `uint` / `uintptr` 都是 8 字节。
    pub const HOST_ARM64: Sizes = Sizes {
        int: 8,
        uint: 8,
        uintptr: 8,
    };

    /// 按 [`crate::basic::BasicId`] 取位宽（**位**，不是字节）。
    ///
    /// `None` ⇒ 该类型没有「随平台变的位宽」（`bool`/`string`/`unsafe.Pointer` …），
    /// 或者它是**无类型**的（无类型常量不分宽度）。调用方必须先判
    /// [`crate::basic::BasicId::is_untyped`]，别把 `None` 当成 0 位。
    #[must_use]
    pub fn width_of(self, b: crate::basic::BasicId) -> Option<u32> {
        use crate::basic::BasicId as B;
        let bytes = match b {
            B::Int => self.int,
            B::Uint => self.uint,
            B::Uintptr => self.uintptr,
            // 与平台无关的定宽类型：写出来是为了「漏一个变体编译期就炸」
            B::Int8 | B::Uint8 => 1,
            B::Int16 | B::Uint16 => 2,
            B::Int32 | B::Uint32 | B::Float32 => 4,
            B::Int64 | B::Uint64 | B::Float64 | B::Complex64 => 8,
            B::Complex128 => 16,
            B::Bool | B::String | B::UnsafePointer => return None,
            B::UntypedBool
            | B::UntypedInt
            | B::UntypedRune
            | B::UntypedFloat
            | B::UntypedComplex
            | B::UntypedString
            | B::UntypedNil => return None,
        };
        Some(bytes * 8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::basic::BasicId as B;

    #[test]
    fn 首发平台三个都是_8_字节() {
        assert_eq!(Sizes::HOST_ARM64.int, 8);
        assert_eq!(Sizes::HOST_ARM64.uint, 8);
        assert_eq!(Sizes::HOST_ARM64.uintptr, 8);
    }

    #[test]
    fn 位宽_平台相关三项跟着_sizes_走() {
        let s = Sizes {
            int: 4,
            uint: 4,
            uintptr: 4,
        };
        assert_eq!(Sizes::HOST_ARM64.width_of(B::Int), Some(64));
        assert_eq!(s.width_of(B::Int), Some(32), "32 位目标：不改签名就能表达");
        assert_eq!(s.width_of(B::Uintptr), Some(32));
    }

    #[test]
    fn 位宽_定宽类型与无类型() {
        assert_eq!(Sizes::HOST_ARM64.width_of(B::Int8), Some(8));
        assert_eq!(Sizes::HOST_ARM64.width_of(B::Int64), Some(64));
        assert_eq!(Sizes::HOST_ARM64.width_of(B::Float32), Some(32));
        assert_eq!(Sizes::HOST_ARM64.width_of(B::Complex128), Some(128));
        // 无「位宽」这一维 ⇒ None（**不是** 0）
        assert_eq!(Sizes::HOST_ARM64.width_of(B::Bool), None);
        assert_eq!(Sizes::HOST_ARM64.width_of(B::String), None);
        assert_eq!(Sizes::HOST_ARM64.width_of(B::UntypedInt), None);
        assert_eq!(Sizes::HOST_ARM64.width_of(B::UntypedNil), None);
    }
}
