//! 值分类：一个静态类型在**槽**与**寄存器**里的形状（`C5` §6/§7）。
//!
//! # 两个问题，两个答案
//!
//! 帧里的**槽**与调用时的**寄存器**不是一回事：
//!
//! | 问题 | 由谁回答 | 依据 |
//! |---|---|---|
//! | 这个值在帧里占几个字节、按几字节对齐 | [`slot_of`] | `C5` §7.2（槽宽表） |
//! | 传参时进 `x` 还是 `v`、占几个寄存器 | [`arg_kind`] / [`reg_count`] | `C5` §6.1（寄存器表） |
//!
//! 两者刻意分开：`complex64` 在帧里是 8 字节（一个槽），传参时是 **1 个 `v`**；
//! `complex128` 在帧里是 16 字节（**一个**槽，但跨两个 8 字节字），传参时是 **2 个 `v`**。
//! 把它们合成一张表就会写出「complex128 占两个槽」这种既不对齐也不好用的东西。
//!
//! # 槽宽一律 8 的倍数
//!
//! `C5` §7.2 冻结：`bool`/整数/指针/浮点 = 8；`complex128` = 16；聚合 = `size_of` 向上取 8 的倍数。
//! 理由不是「对齐好看」，而是 `D-M7-3` 的前提 —— 值全在栈槽里，槽边界一旦不是 8 的倍数，
//! 「这个值的第 k 个字」就要按类型算偏移，而那是**第二套布局**（第一套在 `rgoc-types`）。
//!
//! # 零大小类型
//!
//! `struct{}` / `[0]int` 的槽宽是 **0**，但它**仍然有地址**（`&x` 合法），
//! 而且 `zerosize.go` 要求 `new([0]int)` 两次拿到**同一个地址**（`runtime.zerobase`）。
//! 所以「槽宽 0」不等于「没有地址」—— 地址由分配器给（`rt_arena_alloc(0)` 返回固定哨兵）。

use rgoc_types::layout::layout;
use rgoc_types::typ::Type;
use rgoc_types::{BasicId, TypeId, TypeTable};

/// 值在帧里的类别（决定槽宽与指令族）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotClass {
    /// 8 字节标量：`bool` / 各宽度整数 / 指针 / `float32` / `float64` / `string`(两字) 以外的标量。
    Int,
    /// 浮点（`f32` 存低 32 位，`f64` 存整个字）。
    Float,
    /// `complex64`：8 字节，**align 4**（实测），传参占 **1 个 `v`**。
    Complex64,
    /// `complex128`：16 字节，传参占 **2 个 `v`**。
    Complex128,
    /// 聚合（数组 / struct）：按 `size_of` 定槽宽，传参**按指针**。
    Agg,
}

/// 一个值在帧里的形状。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// 类别。
    pub class: SlotClass,
    /// **槽宽**（字节，恒为 8 的倍数；零大小类型为 0）。
    pub size: u32,
    /// 槽对齐（`C5` §7.2 冻结为 8；保留字段是为了 `FRAME-MAP` 能写出来）。
    pub align: u32,
}

/// 传参时的寄存器族。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    /// 整数/指针寄存器 `x0..x7`。
    X,
    /// 浮点寄存器 `v0..v7`。
    V,
    /// 聚合：**按指针**传（`C5` §6.1 与 AAPCS64 的**故意**差异）。
    Ptr,
}

/// 取一个类型的槽形状。`None` ⇒ 该类型没有运行期布局（泛型/元组/非法）。
#[must_use]
pub fn slot_of(tt: &TypeTable, t: TypeId) -> Option<Slot> {
    let l = layout(tt, t)?;
    let class = class_of(tt, t)?;
    let size = match class {
        // 标量一律一个 8 字节槽：`int8` 也占 8（浪费 7 字节，换来「槽宽恒定」）。
        // ⚠️ 但 `string` 是 **16 字节**的标量（`(ptr, len)` 两个字）—— 一律写 8 会把它截断。
        SlotClass::Int => round_up_8(l.size)?.max(8),
        SlotClass::Float | SlotClass::Complex64 => 8,
        SlotClass::Complex128 => 16,
        SlotClass::Agg => round_up_8(l.size)?,
    };
    Some(Slot {
        class,
        size,
        align: 8,
    })
}

/// 按静态类型定类别。
///
/// `None` ⇒ 布局不可算（与 [`slot_of`] 同源）。
fn class_of(tt: &TypeTable, t: TypeId) -> Option<SlotClass> {
    // 先剥命名/别名：`type MyFloat float64` 必须按 `float64` 走浮点寄存器。
    let mut cur = t;
    for _ in 0..64 {
        match tt.type_of(cur) {
            Type::Named(id) => cur = tt.named_underlying(*id),
            Type::Alias(id) => cur = tt.alias_rhs(*id),
            _ => break,
        }
    }
    match tt.type_of(cur) {
        Type::Basic(b) => match b {
            BasicId::Float32 | BasicId::Float64 => Some(SlotClass::Float),
            BasicId::Complex64 => Some(SlotClass::Complex64),
            BasicId::Complex128 => Some(SlotClass::Complex128),
            // `bool` / 整数 / `uintptr` / `string`(16 字节两字) / `unsafe.Pointer`
            // 都按整数寄存器走（`string` 占 **两个** `x`，见 `reg_count`）。
            BasicId::Bool
            | BasicId::Int
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
            | BasicId::String
            | BasicId::UnsafePointer => Some(SlotClass::Int),
            // 无类型常量没有运行期形状。
            BasicId::UntypedBool
            | BasicId::UntypedInt
            | BasicId::UntypedRune
            | BasicId::UntypedFloat
            | BasicId::UntypedComplex
            | BasicId::UntypedString
            | BasicId::UntypedNil => None,
        },
        // 指针 / map / chan / func / slice / 接口 在 M7 的**值槽**里都是标量（≤24 字节）。
        // 其中只有 slice/接口会跨多个字，但 M7 不支持 slice、接口被 M6 拒 —— 见下。
        Type::Pointer(_) | Type::Map { .. } | Type::Chan { .. } | Type::Signature(_) => {
            Some(SlotClass::Int)
        }
        Type::Interface(_) => Some(SlotClass::Agg),
        Type::Slice(_) => Some(SlotClass::Agg),
        Type::Array { .. } | Type::Struct(_) => Some(SlotClass::Agg),
        // 剥了 64 层还是命名/别名 ⇒ 环（`type A = A`）。返回 None 而不是继续剥。
        Type::Named(_) | Type::Alias(_) => None,
        Type::Tuple(_) | Type::TypeParam(_) | Type::Union(_) | Type::Invalid => None,
    }
}

/// 传参时进哪个寄存器族。
#[must_use]
pub fn arg_kind(class: SlotClass) -> ArgKind {
    match class {
        SlotClass::Int => ArgKind::X,
        SlotClass::Float | SlotClass::Complex64 | SlotClass::Complex128 => ArgKind::V,
        SlotClass::Agg => ArgKind::Ptr,
    }
}

/// 传参时占几个寄存器。
///
/// - 整数/指针/浮点：1
/// - `complex64`：1（两个 `f32` 打包在一个 64 位 `v` 里）
/// - `complex128`：2（实部、虚部各一个 `v`）
/// - 聚合：1（只传地址）
/// - `string`：**2**（`(ptr, len)` 两个 `x`）
#[must_use]
pub fn reg_count(tt: &TypeTable, t: TypeId) -> Option<u32> {
    let s = slot_of(tt, t)?;
    // `complex128`：实部与虚部各占一个 `v`。
    if s.class == SlotClass::Complex128 {
        return Some(2);
    }
    // 8 字节以上的**标量**只有 `string`（`(ptr, len)` 两个字）。
    if s.class == SlotClass::Int && layout(tt, t).is_some_and(|l| l.size > 8) {
        return Some(2);
    }
    Some(1)
}

/// 向上取整到 8 的倍数。
fn round_up_8(n: u32) -> Option<u32> {
    n.checked_add(7).map(|v| v & !7)
}
