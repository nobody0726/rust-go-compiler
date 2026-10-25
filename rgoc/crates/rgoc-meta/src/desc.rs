//! `TypeDesc` —— 每个类型一块的静态元数据。
//!
//! 布局是 `C5` §10 冻结的**内存**契约（不是 Rust 结构体的自然大小）：
//! 40 字节，字段偏移见下。`#[repr(C)]` 是必须的。
//!
//! # 为什么 `size` 不含对象头
//!
//! 堆对象的形状是 `[+0 desc][+8 data...]`（`M9-design.md` §3.2），
//! `desc.size` 只数 **data** 部分。分配器负责加 8。这条分工让同一个 desc
//! 既能描述「栈上的值」（没有头）也能描述「堆上的对象」（有头）。

use rgoc_types::TypeTable;
use rgoc_types::layout;
use rgoc_types::typ::TypeId;

/// 类型的 kind —— 与 `reflect.Kind` 对齐的**子集**。
///
/// 为什么不用完整的 24 种：M9 只需要「怎么比、怎么 hash、怎么扫」这三件事
/// 用得到的分组。`reflect` 的完整 kind 归 M12（`U-M9-04`）。
///
/// 值是 `u8` 且**进契约**（`C5` §10）：加一个 kind 要同步 `ALL` 与计数测试。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum Kind {
    /// 非法的 / 未知的类型（`Invalid`、`Tuple`、`[...]T` 之类）。
    Invalid = 0,
    /// 布尔。
    Bool = 1,
    /// 有符号整数。
    Int = 2,
    /// 无符号整数（含 `uintptr`）。
    Uint = 3,
    /// 浮点。
    Float = 4,
    /// 复数。
    Complex = 5,
    /// `string`（16 字节：ptr + len）。
    String = 6,
    /// 指针（含 `unsafe.Pointer`）。
    Pointer = 7,
    /// 数组（值语义）。
    Array = 8,
    /// 结构体（值语义）。
    Struct = 9,
    /// 切片（24 字节：ptr + len + cap）。
    Slice = 10,
    /// map（单字指针）。
    Map = 11,
    /// 通道（单字指针）。
    Chan = 12,
    /// 函数值（单字指针，指向 funcval）。
    Func = 13,
    /// 接口（16 字节：tab/type + data）。
    Interface = 14,
}

impl Kind {
    /// 全部 kind（**按 `u8` 值序**，即字典序）。
    pub const ALL: [Kind; 15] = [
        Kind::Invalid,
        Kind::Bool,
        Kind::Int,
        Kind::Uint,
        Kind::Float,
        Kind::Complex,
        Kind::String,
        Kind::Pointer,
        Kind::Array,
        Kind::Struct,
        Kind::Slice,
        Kind::Map,
        Kind::Chan,
        Kind::Func,
        Kind::Interface,
    ];
}

/// `TypeDesc.flags` 的位。
pub mod flags {
    /// 该类型是可比较的（`==` 有意义）。M9 的 map 键与接口断言都读它。
    pub const COMPARABLE: u8 = 1 << 0;
    /// 该类型的 data 里含 Go 指针（`gcdata` 非空）。
    pub const HAS_POINTERS: u8 = 1 << 1;
    /// 分配后必须清零（含指针，或含浮点 —— `0.0` 的位模式不是全零）。
    pub const NEEDS_ZERO: u8 = 1 << 2;
}

/// 每个类型一块的静态元数据。**40 字节**，`#[repr(C)]`。
///
/// 字段序与偏移是 `C5` §10 的冻结内容 —— 改一个字段就要同步
/// `C5`、`M9-tests.md` 的 `T-META-01` 与 M10 的扫描器。
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct TypeDesc {
    /// data 部分的字节数（**不含** 8 字节对象头）。
    pub size: u32,
    /// `[0, ptrdata)` 前缀里可能有指针；`gcdata` 只覆盖这段。
    pub ptrdata: u32,
    /// 类型 hash（接口 `==` 与 map 键都用它）。
    pub hash: u32,
    /// [`Kind`] 的 `u8` 值。
    pub kind: u8,
    /// [`flags`] 的位组合。
    pub flags: u8,
    /// ptrmask：每 8 字节 1 位，`ptrdata == 0` 时为 `null`。
    pub gcdata: *const u8,
    /// `*T` / `[]T` / `[n]T` 的元素 desc；其余为 `null`。
    pub elem: *const TypeDesc,
    /// 方法表；无方法为 `null`。运行期 itab 构造读它。
    pub methods: *const MethodTable,
}

impl TypeDesc {
    /// 空 desc（`size = 0`、`kind = Invalid`）。测试与「无类型」占位用。
    pub const EMPTY: TypeDesc = TypeDesc {
        size: 0,
        ptrdata: 0,
        hash: 0,
        kind: Kind::Invalid as u8,
        flags: 0,
        gcdata: core::ptr::null(),
        elem: core::ptr::null(),
        methods: core::ptr::null(),
    };

    /// 该类型是否可比较。
    #[must_use]
    pub const fn is_comparable(&self) -> bool {
        self.flags & flags::COMPARABLE != 0
    }

    /// 该类型的 data 里是否含 Go 指针。
    #[must_use]
    pub const fn has_pointers(&self) -> bool {
        self.flags & flags::HAS_POINTERS != 0
    }

    /// 分配后是否必须清零。
    #[must_use]
    pub const fn needs_zero(&self) -> bool {
        self.flags & flags::NEEDS_ZERO != 0
    }
}

/// 方法表（M9/Task 27 填实；这里先给形状，因为 `TypeDesc.methods` 指它）。
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct MethodTable {
    /// 条目数。
    pub count: u32,
    /// 对齐填充（让 `entries` 落在 8 的倍数上）。
    pub pad: u32,
}

/// 方法表里的一条。
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct MethodEntry {
    /// 方法**名字**的 hash（不是指针 —— 真名字归 M12）。
    pub name_hash: u32,
    /// 签名的 hash。
    pub sig_hash: u32,
    /// 入口地址。
    pub func: *const u8,
}

/// 从 `TypeId` 造 `TypeDesc`。
///
/// # 返回 `None`
///
/// `layout()` 给不出尺寸的类型（泛型 / `Tuple` / `[...]T` / `Invalid`）。
/// 这类类型在 native 通路里本就该被拒（`CG-UNSUPPORTED-*`），
/// **不许**给个假尺寸蒙过去。
#[derive(Debug)]
pub struct DescBuilder<'a> {
    tt: &'a TypeTable,
    t: TypeId,
    desc: TypeDesc,
}

impl<'a> DescBuilder<'a> {
    /// 开始造。
    ///
    /// Task 1 只填 `size`；`kind` / `flags` / `gcdata` / `elem` / `methods`
    /// 由 Task 2–4 补。
    #[must_use]
    pub fn new(tt: &'a TypeTable, t: TypeId) -> Option<Self> {
        let l = layout::layout(tt, t)?;
        Some(DescBuilder {
            tt,
            t,
            desc: TypeDesc {
                size: l.size,
                ..TypeDesc::EMPTY
            },
        })
    }

    /// 类型本身（后面几个 Task 要按它填 `gcdata` / `elem`）。
    #[must_use]
    pub fn type_id(&self) -> TypeId {
        self.t
    }

    /// 类型表（同上）。
    #[must_use]
    pub fn types(&self) -> &'a TypeTable {
        self.tt
    }

    /// 当前已填的 desc（中途读用）。
    #[must_use]
    pub fn desc(&self) -> TypeDesc {
        self.desc
    }

    /// 造完。
    #[must_use]
    pub fn build(self) -> TypeDesc {
        self.desc
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, TypeDesc};

    /// `T-META-01`：布局是 `C5` §10 冻结的 40 字节。
    ///
    /// 逐字段 `offset_of` 而不是只比 `size_of`：整体大小对了而字段错位
    /// （比如把 `kind` 和 `flags` 调换）在只看总大小时是绿的，而 M10 的
    /// 扫描器会读到错误的 `gcdata` 指针 —— 症状是随机崩，不是编译错。
    #[test]
    fn desc_布局是_c5_冻结的_40_字节() {
        assert_eq!(core::mem::size_of::<TypeDesc>(), 40);
        assert_eq!(core::mem::offset_of!(TypeDesc, size), 0);
        assert_eq!(core::mem::offset_of!(TypeDesc, ptrdata), 4);
        assert_eq!(core::mem::offset_of!(TypeDesc, hash), 8);
        assert_eq!(core::mem::offset_of!(TypeDesc, kind), 12);
        assert_eq!(core::mem::offset_of!(TypeDesc, flags), 13);
        assert_eq!(core::mem::offset_of!(TypeDesc, gcdata), 16);
        assert_eq!(core::mem::offset_of!(TypeDesc, elem), 24);
        assert_eq!(core::mem::offset_of!(TypeDesc, methods), 32);
    }

    /// `Kind` 的值序与 `ALL` 自洽（`DiagCode` 的五件套惯例）。
    ///
    /// 漏一条 ⇒ `T-META-06` 的对账会把合法 kind 判成非法；
    /// 顺序错 ⇒ `Kind::ALL` 不再是字典序，「按 kind 分组」的代码会静默错。
    #[test]
    fn kind_的_all_按值序且不重() {
        let vals: Vec<u8> = Kind::ALL.iter().map(|k| *k as u8).collect();
        let mut sorted = vals.clone();
        sorted.sort_unstable();
        assert_eq!(vals, sorted, "ALL 必须按 u8 值序");
        let mut uniq = vals.clone();
        uniq.dedup();
        assert_eq!(uniq.len(), vals.len(), "kind 值不许重复");
        assert_eq!(Kind::ALL.len(), 15, "改数量要同步 C5 §10");
    }
}
