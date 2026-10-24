//! 对象布局：`size_of` / `align_of` / 字段偏移（M7/Task 8 的前置，`R-8-1`）。
//!
//! # 为什么这里必须存在
//!
//! `M7-design.md` §7.3 原本写「偏移一律来自 `rgoc-types` 的布局」，但 M7 开工时
//! **`rgoc-types` 里没有任何布局代码** —— 解释器用的是「字段下标」（`Value::Struct(Rc<Vec<Cell>>)`），
//! 从来不需要字节偏移。所以 M7 的第一件事是**把布局建起来**，而不是去消费一个不存在的东西。
//!
//! # 每个数字都是实测的
//!
//! 下表来自 `go1.27.1 / linux-arm64` 的 `unsafe.Sizeof` / `Alignof` / `Offsetof` 实测
//! （2026-10-24，容器内 `go run`）—— **不是**从规范推的：
//!
//! | 类型 | size | align | 备注 |
//! |---|---|---|---|
//! | `bool` | 1 | 1 | |
//! | `int8/16/32/64`、`uint8/16/32/64` | 1/2/4/8 | 同 size | |
//! | `int` / `uint` / `uintptr` | 8 | 8 | 由 [`Sizes::HOST_ARM64`] 给 |
//! | `float32` / `float64` | 4 / 8 | 同 size | |
//! | `complex64` | 8 | **4** | align 不是 8 —— 这是最容易写错的一格 |
//! | `complex128` | 16 | 8 | |
//! | `string` | 16 | 8 | `(ptr, len)` |
//! | 指针 / `map` / `chan` / `func` / `unsafe.Pointer` | 8 | 8 | |
//! | 接口 | 16 | 8 | `(type, data)`；`error` 同 |
//! | `[]T` | 24 | 8 | `(ptr, len, cap)` |
//! | `[n]T` | `n * size(T)` | `align(T)` | `[0]int` ⇒ **0 / 8**（size 0 但 align 仍是元素对齐） |
//! | `struct{...}` | 见 §结构体 | `max(field align, 1)` | 空结构体 ⇒ **0 / 1** |
//!
//! # 结构体有两条容易漏的规则（都有实测锚点）
//!
//! 1. **末尾零大小字段要多占 1 字节**：`struct{a int64; b struct{}}` 实测 **16**（不是 8），
//!    `Offsetof(b) == 8`。理由是零大小字段也必须有**不与他人重叠的地址**，
//!    所以 `8 + 1` 再向上取整到 8 = 16。
//!    ⚠️ 但 `struct{b struct{}}` 实测是 **0**（不是 1）—— 整条结构体本来就是零大小，
//!    再加 1 就凭空多出一个字节。所以这条规则的**前提是「前面已有非零内容」**。
//! 2. **前导零大小字段不占位**：`struct{a struct{}; b int8}` 实测 **1**，`Offsetof(b) == 0`。
//!
//! # 这里**不做**的事
//!
//! - 不处理泛型（`TypeParam` / `Union` ⇒ `None`）：M7 的输入里没有（`G-M5-03` 未解）。
//! - 不处理 `Tuple`（它不是运行期类型）。
//! - 不猜 `[...]T`（`len == None` ⇒ `None`）：长度未定就没有布局可言。

use crate::table::TypeTable;
use crate::typ::{StructId, Type, TypeId};
use rgoc_const::basic::BasicId;
use rgoc_const::sizes::Sizes;

/// 一个类型的内存布局（**字节**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layout {
    /// 占多少字节。
    pub size: u32,
    /// 对齐要求（字节，恒为 1 或 2 的幂）。
    pub align: u32,
}

impl Layout {
    /// 零大小、对齐 1（空结构体）。
    pub const ZERO: Layout = Layout { size: 0, align: 1 };
}

/// 取一个类型的布局。
///
/// # 返回 `None` 的三种情形
///
/// ① 泛型相关（`TypeParam` / `Union`）；② `Tuple`（不是运行期类型）；
/// ③ 长度未定的数组（`[...]T`）。**`Invalid` 也返回 `None`** ——
/// 给它编一个尺寸等于把「源码有错」悄悄变成「能算」。
#[must_use]
pub fn layout(tt: &TypeTable, t: TypeId) -> Option<Layout> {
    layout_at(tt, t, 0)
}

/// 递归深度的上限。
///
/// 合法的 Go 类型不可能嵌套 64 层指针/别名（`type A = B; type B = A` 这种环
/// 检查器会先拒），但布局函数是 `pub` 的 —— 一个手搓的环会让它栈溢出。
/// 与其 `panic`，不如返回 `None`（调用方按「不支持」处理）。
const MAX_DEPTH: u32 = 64;

fn layout_at(tt: &TypeTable, t: TypeId, depth: u32) -> Option<Layout> {
    if depth > MAX_DEPTH {
        return None;
    }
    match tt.type_of(t) {
        Type::Basic(b) => basic_layout(*b),
        // `type D ...` 看本体；`type D = T` 看右边。
        Type::Named(id) => layout_at(tt, tt.named_underlying(*id), depth + 1),
        Type::Alias(id) => layout_at(tt, tt.alias_rhs(*id), depth + 1),
        Type::Pointer(_) | Type::Map { .. } | Type::Chan { .. } | Type::Signature(_) => {
            Some(Layout { size: 8, align: 8 })
        }
        Type::Interface(_) => Some(Layout { size: 16, align: 8 }),
        Type::Slice(_) => Some(Layout { size: 24, align: 8 }),
        Type::Array { len, elem } => {
            let n = array_len(tt, *len)?;
            let e = layout_at(tt, *elem, depth + 1)?;
            // 元素 size 恒为 align 的倍数（Go 保证），所以步长就是 size。
            let size = e.size.checked_mul(n)?;
            Some(Layout {
                size,
                align: e.align,
            })
        }
        Type::Struct(id) => struct_layout(tt, *id, depth),
        Type::Tuple(_) | Type::TypeParam(_) | Type::Union(_) | Type::Invalid => None,
    }
}

/// 基本类型的布局。
fn basic_layout(b: BasicId) -> Option<Layout> {
    use BasicId as B;
    let one = |size: u32| Some(Layout { size, align: size });
    match b {
        B::Bool => Some(Layout { size: 1, align: 1 }),
        B::String => Some(Layout { size: 16, align: 8 }),
        B::UnsafePointer => Some(Layout { size: 8, align: 8 }),
        // `complex64` 的 align 是 **4**（实测），不是 8。
        B::Complex64 => Some(Layout { size: 8, align: 4 }),
        B::Complex128 => Some(Layout { size: 16, align: 8 }),
        // 其余定宽标量：size == align。
        B::Int8 | B::Uint8 => one(1),
        B::Int16 | B::Uint16 => one(2),
        B::Int32 | B::Uint32 | B::Float32 => one(4),
        B::Int64 | B::Uint64 | B::Float64 => one(8),
        // 平台相关的三项走 `Sizes`（首发 arm64 ⇒ 都是 8）。
        B::Int | B::Uint | B::Uintptr => {
            let s = Sizes::HOST_ARM64.width_of(b)? / 8;
            one(s)
        }
        // 无类型常量按 Go 的**默认类型**取布局（`R-13-1`）。
        //
        // 它们本该在 lowering 的写入点被收敛（`R-22-2`），但**直接当内建实参**时
        // 没有写入点：`print("hi")` 的 SSA 里那个常量就带着 `untyped string`。
        // 后端要么按默认类型处理，要么整类程序编不出来 —— 而默认类型是规范
        // 明确规定的（`untyped int` ⇒ `int`、`untyped rune` ⇒ `int32`…），不是猜。
        B::UntypedBool => basic_layout(B::Bool),
        B::UntypedInt => basic_layout(B::Int),
        B::UntypedRune => basic_layout(B::Int32),
        B::UntypedFloat => basic_layout(B::Float64),
        B::UntypedComplex => basic_layout(B::Complex128),
        B::UntypedString => basic_layout(B::String),
        // `untyped nil` 没有默认类型（`nil` 必须由上下文给类型）。
        B::UntypedNil => None,
    }
}

/// 数组长度（元素个数）。`None` ⇒ 长度未定或不是非负整数。
fn array_len(tt: &TypeTable, len: Option<rgoc_const::consts::ConstId>) -> Option<u32> {
    use rgoc_const::value::Const;
    let id = len?;
    match tt.consts().get(id) {
        Const::Int(bi) => {
            let n = bi.to_i64()?;
            u32::try_from(n).ok()
        }
        _ => None,
    }
}

/// 类型 `t` 是数组时的元素个数（剥命名/别名）；不是数组 ⇒ `None`。
///
/// # 为什么放在这里
///
/// 数组长度存的是**常量表下标**（`Type::Array.len: Option<ConstId>`），
/// 解析它要 `TypeTable::consts()`。后端只拿得到 `&TypeTable`，如果自己解一遍，
/// 「长度怎么读」就有第二个口径 —— 而 `layout()` 已经有一个了。
#[must_use]
pub fn array_len_of(tt: &TypeTable, t: TypeId) -> Option<u32> {
    let mut cur = t;
    for _ in 0..64 {
        match tt.type_of(cur) {
            Type::Array { len, .. } => return array_len(tt, *len),
            Type::Named(id) => cur = tt.named_underlying(*id),
            Type::Alias(id) => cur = tt.alias_rhs(*id),
            _ => return None,
        }
    }
    None
}

/// 结构体的布局与每个字段的偏移。
fn struct_layout(tt: &TypeTable, id: StructId, depth: u32) -> Option<Layout> {
    let def = tt.struct_def(id);
    let mut off: u32 = 0;
    let mut align: u32 = 1;
    let mut last_size: u32 = 0;
    for f in &def.fields {
        let fl = layout_at(tt, f.typ, depth + 1)?;
        align = align.max(fl.align);
        off = round_up(off, fl.align)?;
        off = off.checked_add(fl.size)?;
        last_size = fl.size;
    }
    // 末尾零大小字段：多占 1 字节，保证它的地址不与后面的对象重叠（实测 S2 = 16）。
    // 但整条结构体本来就零大小时不加（实测 S3 = 0）。
    if last_size == 0 && off > 0 {
        off = off.checked_add(1)?;
    }
    Some(Layout {
        size: round_up(off, align)?,
        align,
    })
}

/// 结构体每个字段的字节偏移（声明序）。
///
/// # Errors（`None`）
///
/// 任一字段的类型没有布局。
#[must_use]
pub fn field_offsets(tt: &TypeTable, id: StructId) -> Option<Vec<u32>> {
    let def = tt.struct_def(id);
    let mut out = Vec::with_capacity(def.fields.len());
    let mut off: u32 = 0;
    for f in &def.fields {
        let fl = layout(tt, f.typ)?;
        off = round_up(off, fl.align)?;
        out.push(off);
        off = off.checked_add(fl.size)?;
    }
    Some(out)
}

/// 第 `idx` 个字段的字节偏移。
#[must_use]
pub fn field_offset(tt: &TypeTable, id: StructId, idx: usize) -> Option<u32> {
    field_offsets(tt, id)?.get(idx).copied()
}

/// 向上取整到 `align` 的倍数（`align` 必须是 2 的幂）。
fn round_up(n: u32, align: u32) -> Option<u32> {
    debug_assert!(align.is_power_of_two() || align == 0, "align 不是 2 的幂");
    if align <= 1 {
        return Some(n);
    }
    let mask = align - 1;
    n.checked_add(mask).map(|v| v & !mask)
}
