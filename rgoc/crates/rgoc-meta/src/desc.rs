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
use rgoc_types::typ::{Type, TypeId};
use std::collections::BTreeMap;

/// 类型的 kind —— **官方 `reflect.Kind` 的完整 27 档**。
///
/// M9 只需要「怎么比、怎么 hash、怎么扫」，当时用了 15 档的分组子集
/// （`Int` 吞掉所有宽度的有符号整型）。M12/Task 31 的反射读面要求
/// `Type.Kind()` 与官方逐值一致（`int8` 必须 3 而不是 2 —— 实测
/// `dump(reflect.TypeOf(int8(1)))` 打 2），换成官方编号。
///
/// 值是 `u8` 且**进契约**（`C5` §10，M12 起从 15 档子集扩成官方 27 档）：
/// 加一个 kind 要同步 `ALL` 与计数测试。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum Kind {
    /// 非法的 / 未知的类型（`Invalid`、`Tuple`、`[...]T` 之类）。
    Invalid = 0,
    Bool = 1,
    Int = 2,
    Int8 = 3,
    Int16 = 4,
    Int32 = 5,
    Int64 = 6,
    Uint = 7,
    Uint8 = 8,
    Uint16 = 9,
    Uint32 = 10,
    Uint64 = 11,
    Uintptr = 12,
    Float32 = 13,
    Float64 = 14,
    Complex64 = 15,
    Complex128 = 16,
    Array = 17,
    Chan = 18,
    Func = 19,
    Interface = 20,
    Map = 21,
    Pointer = 22,
    Slice = 23,
    String = 24,
    Struct = 25,
    UnsafePointer = 26,
}

impl Kind {
    /// 全部 kind（**按 `u8` 值序**，即字典序）。
    pub const ALL: [Kind; 27] = [
        Kind::Invalid,
        Kind::Bool,
        Kind::Int,
        Kind::Int8,
        Kind::Int16,
        Kind::Int32,
        Kind::Int64,
        Kind::Uint,
        Kind::Uint8,
        Kind::Uint16,
        Kind::Uint32,
        Kind::Uint64,
        Kind::Uintptr,
        Kind::Float32,
        Kind::Float64,
        Kind::Complex64,
        Kind::Complex128,
        Kind::Array,
        Kind::Chan,
        Kind::Func,
        Kind::Interface,
        Kind::Map,
        Kind::Pointer,
        Kind::Slice,
        Kind::String,
        Kind::Struct,
        Kind::UnsafePointer,
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
    /// 反射元数据表的下标（**1 起**；`0` = 没有 —— `C13` §5.1）。
    ///
    /// 这两个字节在 `C5` §10 里是**对齐填充**（`pad u16`）—— M12 把它的
    /// **语义**改成 `meta_idx`，一个偏移都不动（40 字节不变，M9 的
    /// `t_desc_*` 判据一条不用改）。`meta_idx == 0` ⇒ 反射只拿得到
    /// size/kind/align（基本类型够用）；具名类型与 struct 必须有表。
    pub meta_idx: u16,
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
        meta_idx: 0,
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

// ── 反射元数据（`C13` §5.2；M12/Task 30）──────────────────────────────
//
// 下面四个结构是**发射面与 rt.c 共用的内存形状**（`.rodata` 里的字节布局，
// 不是 Rust 侧的解析结构）。`meta_idx` 指向的就是 [`ReflectMeta`]。

const _: () = assert!(
    core::mem::size_of::<TypeDesc>() == 40,
    "TypeDesc 必须是 40 字节（C5 §10 / C13 §5.1）"
);

/// 反射元数据块（64 字节，`.rodata`；`TypeDesc.meta_idx` 指它）。
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct ReflectMeta {
    /// 类型名（不含包名；实例渲染成 `Box[int]`，`C13` §4）。
    pub name: *const u8,
    /// 定义包路径（发射面取**包名**；跨包进 HIR 之前恒为 `main`）。
    pub pkgpath: *const u8,
    /// 字段表（`nfields` 条 [`FieldMeta`]，连续）。
    pub fields: *const FieldMeta,
    /// 字段数。
    pub nfields: u32,
    /// 方法数（Task 32 填实；骨架期 0）。
    pub nmethods: u32,
    /// 方法表（`nmethods` 条 [`MethodMeta`]）。
    pub methods: *const MethodMeta,
    /// 参数 ABI 表（Task 33 填实；骨架期 null）。
    pub params: *const ParamMeta,
    /// 参数数。
    pub nparams: u32,
    /// 结果数。
    pub nresults: u32,
    /// 结果 ABI 表（Task 33 填实；骨架期 null）。
    pub results: *const ParamMeta,
}

/// 反射字段表里的一条（32 字节）。
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct FieldMeta {
    /// 字段名（嵌入字段为空串）。
    pub name: *const u8,
    /// 结构体标签原文（无标签为空串）。
    pub tag: *const u8,
    /// 字段类型的 desc。
    pub typ: *const TypeDesc,
    /// 字段偏移（字节；`layout::field_offsets` 算好的）。
    pub offset: u32,
    /// 位 0 = anonymous（嵌入）；位 1 = exported。
    pub flags: u8,
    // 隐式 3 字节对齐填充到 32。
}

/// 反射方法表里的一条（24 字节；Task 32 填实）。
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct MethodMeta {
    /// 方法名。
    pub name: *const u8,
    /// 签名的 desc。
    pub sig: *const TypeDesc,
    /// 入口（实例化后）。
    pub func: *const u8,
}

/// 反射参数 ABI 表里的一条（16 字节；Task 33 填实）。
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct ParamMeta {
    /// 参数类型的 desc。
    pub typ: *const TypeDesc,
    /// ABI 类（0=GPR 1=FPR 2=栈 3=零大小 —— M7 `abi_classify` 编译期算好）。
    pub abi_class: u8,
    // 隐式对齐填充。
}

const _: () = assert!(
    core::mem::size_of::<ReflectMeta>() == 64,
    "ReflectMeta 必须是 64 字节（C13 §5.2）"
);
const _: () = assert!(
    core::mem::size_of::<FieldMeta>() == 32,
    "FieldMeta 必须是 32 字节（C13 §5.2）"
);
const _: () = assert!(
    core::mem::size_of::<MethodMeta>() == 24,
    "MethodMeta 必须是 24 字节（C13 §5.2）"
);

/// 类型 → [`Kind`]。布局给不出来的类型 ⇒ [`Kind::Invalid`]。
///
/// # 为什么先看 `layout` 再分派
///
/// `[...]T` / `Tuple` / 泛型都**没有**运行期表示；给它们一个「看着像」的 kind
/// 会让调用方以为能分配。先要布局，就是要它们在这里就出局。
#[must_use]
pub fn kind_of(tt: &TypeTable, t: TypeId) -> Kind {
    kind_at(tt, t, 0)
}

fn kind_at(tt: &TypeTable, t: TypeId, depth: u32) -> Kind {
    use rgoc_const::basic::BasicId as B;
    // 布局是「有没有运行期表示」的唯一判据（与 `DescBuilder::new` 同一口径）。
    if depth > 64 || layout::layout(tt, t).is_none() {
        return Kind::Invalid;
    }
    match tt.type_of(t) {
        Type::Basic(b) => match b {
            B::Bool | B::UntypedBool => Kind::Bool,
            B::Int => Kind::Int,
            B::Int8 => Kind::Int8,
            B::Int16 => Kind::Int16,
            B::Int32 => Kind::Int32,
            B::Int64 => Kind::Int64,
            // 无类型默认类型（官方 `defaultType` 口径）：untyped int/rune ⇒ int。
            B::UntypedInt | B::UntypedRune => Kind::Int,
            B::Uint => Kind::Uint,
            B::Uint8 => Kind::Uint8,
            B::Uint16 => Kind::Uint16,
            B::Uint32 => Kind::Uint32,
            B::Uint64 => Kind::Uint64,
            B::Uintptr => Kind::Uintptr,
            B::Float32 => Kind::Float32,
            // untyped float 的默认类型是 float64。
            B::Float64 | B::UntypedFloat => Kind::Float64,
            B::Complex64 => Kind::Complex64,
            // untyped complex 的默认类型是 complex128。
            B::Complex128 | B::UntypedComplex => Kind::Complex128,
            B::String | B::UntypedString => Kind::String,
            B::UnsafePointer => Kind::UnsafePointer,
            B::UntypedNil => Kind::Invalid,
        },
        Type::Named(id) => kind_at(tt, tt.named_underlying(*id), depth + 1),
        Type::Alias(id) => kind_at(tt, tt.alias_rhs(*id), depth + 1),
        Type::Pointer(_) => Kind::Pointer,
        Type::Array { .. } => Kind::Array,
        Type::Struct(_) => Kind::Struct,
        Type::Slice(_) => Kind::Slice,
        Type::Map { .. } => Kind::Map,
        Type::Chan { .. } => Kind::Chan,
        Type::Signature(_) => Kind::Func,
        Type::Interface(_) => Kind::Interface,
        Type::Tuple(_) | Type::TypeParam(_) | Type::Union(_) | Type::Invalid => Kind::Invalid,
    }
}

/// 该类型是否**必须**在分配后清零。
///
/// 两条独立的理由，缺一条就会错：
///
/// 1. **含指针** —— 全零恰好是 `nil`，但那是运气；真正的理由是「不清零就会
///    留下上一条命的指针，GC 会追它」。
/// 2. **含浮点** —— `0.0` 的位模式是**全零**（IEEE 754 的正零），但
///    `-0.0`、`NaN` 不是；而「零值」按规范是 `+0.0`。所以必须先清成全零
///    再当 `0.0` 用。`complex64/128` 同理（实部虚部各是浮点）。
///
/// 全整数 / 布尔的聚合**不需要**（零值就是全零）。
#[must_use]
pub fn needs_zero(tt: &TypeTable, t: TypeId) -> bool {
    has_pointers(tt, t) || has_float(tt, t)
}

/// 类型里是否含 Go 指针（= `ptr_mask` 有任一位为真）。
fn has_pointers(tt: &TypeTable, t: TypeId) -> bool {
    layout::ptr_mask(tt, t).is_some_and(|m| m.iter().any(|b| *b))
}

/// 类型里是否（递归地）含浮点。
fn has_float(tt: &TypeTable, t: TypeId) -> bool {
    has_float_at(tt, t, 0)
}

fn has_float_at(tt: &TypeTable, t: TypeId, depth: u32) -> bool {
    use rgoc_const::basic::BasicId as B;
    if depth > 64 {
        return false;
    }
    match tt.type_of(t) {
        Type::Basic(b) => matches!(
            b,
            B::Float32
                | B::Float64
                | B::Complex64
                | B::Complex128
                | B::UntypedFloat
                | B::UntypedComplex
        ),
        Type::Named(id) => has_float_at(tt, tt.named_underlying(*id), depth + 1),
        Type::Alias(id) => has_float_at(tt, tt.alias_rhs(*id), depth + 1),
        Type::Array { elem, .. } => has_float_at(tt, *elem, depth + 1),
        Type::Struct(id) => tt
            .struct_def(*id)
            .fields
            .iter()
            .any(|f| has_float_at(tt, f.typ, depth + 1)),
        // 指针 / slice / map / chan / func / iface 里**装的是指针**，不是浮点值
        // —— 浮点只可能出现在「按值存」的位置。
        _ => false,
    }
}

// ───────────────────────── 类型 hash ─────────────────────────

/// FNV-1a 32 位的初始值。
const FNV_OFFSET: u32 = 2_166_136_261;
/// FNV-1a 32 位的乘子。
const FNV_PRIME: u32 = 16_777_619;

/// 往 hash 里喂一个字节（FNV-1a）。
fn mix(h: &mut u32, byte: u8) {
    *h ^= u32::from(byte);
    *h = h.wrapping_mul(FNV_PRIME);
}

/// 往 hash 里喂一个 `u32`（小端 —— 与宿主字节序无关，保证跨机稳定）。
fn mix_u32(h: &mut u32, v: u32) {
    for b in v.to_le_bytes() {
        mix(h, b);
    }
}

/// 往 hash 里喂一段字节。
fn mix_bytes(h: &mut u32, s: &[u8]) {
    mix_u32(h, u32::try_from(s.len()).unwrap_or(u32::MAX));
    for b in s {
        mix(h, *b);
    }
}

/// 方法**名字**的 hash（`MethodEntry.name_hash`）。
///
/// # 为什么是 hash 而不是名字指针
///
/// `TypeDesc` 是 `#[repr(C)]` 的 40 字节冻结面（`C5` §10），里面放不下变长字符串。
/// 而且真名字归 M12（`U-M9-04` 的 reflect 面）—— M9 只需要「两个方法是不是同一个」。
///
/// 名字**不**进 `sig_hash`（两者分开存）：`interface{ M(x int) }` 与
/// `func (t T) M(y int)` 是同一个签名，参数名不同不影响实现关系。
#[must_use]
pub fn name_hash(name: &str) -> u32 {
    let mut h = FNV_OFFSET;
    mix_bytes(&mut h, name.as_bytes());
    h
}

/// 方法**签名**的 hash（`MethodEntry.sig_hash`）。
///
/// # 编码
///
/// `param 个数 | 每个 param 的 type_hash | result 个数 | 每个 result 的 type_hash`。
/// 两侧的「个数」是**边界**：不写个数，`M(int) bool` 与 `M(bool) int`
/// 会喂出同一串字节。
///
/// # 调用方要剥接收者
///
/// 具体类型那边 `Func::param_types[0]` 是接收者，接口那边 `SignatureDef.params`
/// **不含**接收者。两边必须喂同一形状，否则合法的实现会被判成「没实现」。
#[must_use]
pub fn sig_hash(tt: &TypeTable, params: &[TypeId], results: &[TypeId]) -> u32 {
    let mut h = FNV_OFFSET;
    mix(&mut h, b'(');
    mix_u32(&mut h, u32::try_from(params.len()).unwrap_or(u32::MAX));
    for p in params {
        mix_u32(&mut h, type_hash(tt, *p));
    }
    mix(&mut h, b')');
    mix_u32(&mut h, u32::try_from(results.len()).unwrap_or(u32::MAX));
    for r in results {
        mix_u32(&mut h, type_hash(tt, *r));
    }
    h
}

/// 类型 hash（接口 `==` 与 map 键都用它）。
///
/// # 三条硬规矩
///
/// 1. **不许用 `TypeId` 的下标**。同一个类型在两个 `TypeTable` 里下标不同
///    （`T-META-05` 的 `跨_type_table` 那条就是它的牙）。下标是**容器内**的
///    身份，不是类型的身份。
/// 2. **别名透明、具名不透明**：`type Q = []int` 与 `[]int` 同 hash；
///    `type MyInt int` 与 `int` 不同 hash（前者是新类型，规范如此）。
/// 3. **结构敏感**：字段名、字段序、参数名都进 hash ——
///    `struct{a,b int}` 与 `struct{b,a int}` 是不同的类型。
///
/// 不承诺的东西：hash 值本身（会随编码方式变）、抗碰撞（32 位，够 M9 用；
/// map 的键相等最终走 `rt_equal`，hash 只用来选桶）。
#[must_use]
pub fn type_hash(tt: &TypeTable, t: TypeId) -> u32 {
    let mut memo: BTreeMap<TypeId, Option<u32>> = BTreeMap::new();
    hash_node(tt, t, &mut memo, 0)
}

/// 环标记（进行中的节点再被遇到时喂这个值）。
fn hash_cycle() -> u32 {
    let mut h = FNV_OFFSET;
    mix(&mut h, b'#');
    h
}

/// 单节点的**独立 hash**（`type_hash` 的内部形态）。
///
/// # 为什么按 `TypeId` 记忆化（M12/Task 31）
///
/// 自引用接口（`reflect.Type` 的 `Elem() Type` / `Key() Type`）会让朴素
/// 结构递归**指数爆炸**：`Type` 有 13 个方法，每个方法签名又含 `Type`，
/// 深度守卫防住无限递归、防不住 `13^64`（实测 `dump(t reflect.Type)`
/// 的原型卡死在 `sig_hash(Type.Elem)`，2026-10-28）。让每个节点在一次
/// `type_hash` 里只算一遍（进行中 ⇒ 环标记），总代价 O(|types|)。
///
/// 已知边界：两个**同构**的自引用类型（`I{M() I}` 与 `J{M() J}`）在环
/// 标记处截断后同 hash —— 32 位 hash 本就不承诺抗碰撞，此边界登记
/// `G-M12-04`。
fn hash_node(
    tt: &TypeTable,
    t: TypeId,
    memo: &mut BTreeMap<TypeId, Option<u32>>,
    depth: u32,
) -> u32 {
    // 深度守卫：合法的 Go 类型不可能嵌套 64 层（检查器先拒环），
    // 但 `type_hash` 是 `pub` 的 —— 手搓的超长链不该让它栈溢出。
    if depth > 64 {
        let mut h = FNV_OFFSET;
        mix(&mut h, 0xFF);
        return h;
    }
    if let Some(v) = memo.get(&t) {
        return v.unwrap_or_else(hash_cycle);
    }
    memo.insert(t, None); // 进行中：再遇到它就是环
    // 别名透明：**直接返回右边的 hash**（不加 tag、不多包一层）——
    // 别名不是新类型，`hash(Q)` 必须逐位等于 `hash(rhs)`
    // （`别名透明而具名不透明` 实测：包一层就破）。
    if let Type::Alias(id) = tt.type_of(t) {
        let hv = hash_node(tt, tt.alias_rhs(*id), memo, depth + 1);
        memo.insert(t, Some(hv));
        return hv;
    }
    let mut h = FNV_OFFSET;
    hash_body(tt, t, memo, depth, &mut h);
    memo.insert(t, Some(h));
    h
}

/// `hash_node` 的 match 主体（拆出来只为控制函数长度）。
fn hash_body(
    tt: &TypeTable,
    t: TypeId,
    memo: &mut BTreeMap<TypeId, Option<u32>>,
    depth: u32,
    h: &mut u32,
) {
    match tt.type_of(t) {
        Type::Basic(b) => {
            mix(h, b'B');
            // 用**文本**而不是判别值：`BasicId` 的变体序不进契约，而 `as_str`
            // 是 `C7` 冻结的（dump 里就写这个）。
            mix_bytes(h, b.as_str().as_bytes());
        }
        // （别名在 `hash_node` 里已截获 —— 走不到这。）
        Type::Alias(_) => unreachable!("别名已在 hash_node 截获"),
        // 具名不透明：名字进 hash，本体也进（两个同名的不同类型不会撞）。
        // **实例的 targs 也进**（`M12/Task 21`，`issue46591`）：实例的身份由
        // `(origin, targs)` 决定（`identical` 的 Named 臂同口径）——
        // `T[struct{ int }]` 与 `T[struct{ int "x" }]` 的 underlying 都是
        // `struct{}`，漏 targs ⇒ 同一个 desc ⇒ interface key 判同 ⇒
        // map 少一个条目（`len(m)=1` 官方是 2）。
        Type::Named(id) => {
            mix(h, b'N');
            mix_bytes(h, tt.named_def(*id).name.as_bytes());
            let def = tt.named_def(*id);
            for &a in &def.targs {
                mix_u32(h, hash_node(tt, a, memo, depth + 1));
            }
            mix_u32(h, hash_node(tt, tt.named_underlying(*id), memo, depth + 1));
        }
        Type::Pointer(e) => {
            mix(h, b'P');
            mix_u32(h, hash_node(tt, *e, memo, depth + 1));
        }
        Type::Slice(e) => {
            mix(h, b'S');
            mix_u32(h, hash_node(tt, *e, memo, depth + 1));
        }
        Type::Array { len, elem } => {
            mix(h, b'A');
            match array_len_u32(tt, *len) {
                Some(n) => mix_u32(h, n),
                None => mix(h, b'?'),
            }
            mix_u32(h, hash_node(tt, *elem, memo, depth + 1));
        }
        Type::Map { key, elem } => {
            mix(h, b'M');
            mix_u32(h, hash_node(tt, *key, memo, depth + 1));
            mix_u32(h, hash_node(tt, *elem, memo, depth + 1));
        }
        Type::Chan { dir, elem } => {
            mix(h, b'C');
            mix(h, *dir as u8);
            mix_u32(h, hash_node(tt, *elem, memo, depth + 1));
        }
        Type::Struct(id) => {
            mix(h, b'T');
            let def = tt.struct_def(*id);
            mix_u32(h, u32::try_from(def.fields.len()).unwrap_or(u32::MAX));
            for f in &def.fields {
                match &f.name {
                    Some(n) => mix_bytes(h, n.as_bytes()),
                    None => mix(h, b'-'),
                }
                mix(h, u8::from(f.embedded));
                // **tag 进 hash**（`M12/Task 21`，`issue46591`）：tag 参与类型
                // 身份（`identical` 严格面比 `f.tag`）—— `T[struct{ int }]{}`
                // 与 `T[struct{ int "x" }]{}` 是**两个**类型，map 里两个 key；
                // 漏了 tag ⇒ 同一个 desc ⇒ 值相等的零值被判同 key ⇒ len=1。
                match &f.tag {
                    Some(t) => mix_bytes(h, t.as_bytes()),
                    None => mix(h, b'~'),
                }
                mix_u32(h, hash_node(tt, f.typ, memo, depth + 1));
            }
        }
        Type::Signature(id) => {
            mix(h, b'F');
            let def = tt.signature_def(*id);
            match def.recv {
                Some(r) => {
                    mix(h, b'R');
                    mix_u32(h, hash_node(tt, r, memo, depth + 1));
                }
                None => mix(h, b'r'),
            }
            mix(h, u8::from(def.variadic));
            mix_u32(h, hash_node(tt, def.params, memo, depth + 1));
            mix_u32(h, hash_node(tt, def.results, memo, depth + 1));
        }
        Type::Tuple(id) => {
            mix(h, b'U');
            let def = tt.tuple_def(*id);
            mix_u32(h, u32::try_from(def.vars.len()).unwrap_or(u32::MAX));
            for (name, ty) in &def.vars {
                match name {
                    Some(n) => mix_bytes(h, n.as_bytes()),
                    None => mix(h, b'-'),
                }
                mix_u32(h, hash_node(tt, *ty, memo, depth + 1));
            }
        }
        Type::Interface(id) => {
            mix(h, b'I');
            let def = tt.interface_def(*id);
            // 方法表按名字升序存（`C7` §4），所以这里直接按存储序喂 —— 稳定。
            mix_u32(h, u32::try_from(def.methods.len()).unwrap_or(u32::MAX));
            for (name, sig) in &def.methods {
                mix_bytes(h, name.as_bytes());
                mix_u32(h, hash_node(tt, *sig, memo, depth + 1));
            }
            // 类型集进 hash：`interface{}` 与 `comparable` 的表示差别只在这里。
            hash_tset(tt, &def.tset, h, memo, depth);
        }
        Type::TypeParam(_) | Type::Union(_) | Type::Invalid => {
            // 没有运行期表示的类型：喂一个稳定但独特的值，别让它们撞上正常类型。
            mix(h, b'X');
            mix_u32(h, u32::try_from(t.index()).unwrap_or(u32::MAX));
        }
    }
}

fn hash_tset(
    tt: &TypeTable,
    ts: &rgoc_types::IfaceTSet,
    h: &mut u32,
    memo: &mut BTreeMap<TypeId, Option<u32>>,
    depth: u32,
) {
    use rgoc_types::IfaceTSet as S;
    match ts {
        S::All => mix(h, b'a'),
        S::Comparable => mix(h, b'c'),
        S::Empty => mix(h, b'e'),
        S::Terms(terms) => {
            mix(h, b't');
            mix_u32(h, u32::try_from(terms.len()).unwrap_or(u32::MAX));
            for (tilde, ty) in terms {
                mix(h, u8::from(*tilde));
                mix_u32(h, hash_node(tt, *ty, memo, depth + 1));
            }
        }
    }
}

/// 数组长度（与 `layout::array_len_of` 同口径，但这里只要一个 `u32`）。
fn array_len_u32(tt: &TypeTable, len: Option<rgoc_const::consts::ConstId>) -> Option<u32> {
    use rgoc_const::value::Const;
    match tt.consts().get(len?) {
        Const::Int(bi) => u32::try_from(bi.to_i64()?).ok(),
        _ => None,
    }
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
        let mut flags = 0u8;
        if rgoc_types::comparable(tt, t).unwrap_or(false) {
            flags |= flags::COMPARABLE;
        }
        if has_pointers(tt, t) {
            flags |= flags::HAS_POINTERS;
        }
        if needs_zero(tt, t) {
            flags |= flags::NEEDS_ZERO;
        }
        Some(DescBuilder {
            tt,
            t,
            desc: TypeDesc {
                size: l.size,
                hash: type_hash(tt, t),
                kind: kind_of(tt, t) as u8,
                flags,
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
    use super::{Kind, MethodEntry, MethodTable, TypeDesc, name_hash, sig_hash};

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
        assert_eq!(Kind::ALL.len(), 27, "改数量要同步 C5 §10");
        // 官方编号锚点（reflect 包的 iota 序）：抽头尾与几个易混档。
        assert_eq!(Kind::Invalid as u8, 0);
        assert_eq!(Kind::Bool as u8, 1);
        assert_eq!(Kind::Int as u8, 2);
        assert_eq!(Kind::Int8 as u8, 3);
        assert_eq!(Kind::Uint8 as u8, 8);
        assert_eq!(Kind::Float32 as u8, 13);
        assert_eq!(Kind::Complex128 as u8, 16);
        assert_eq!(Kind::UnsafePointer as u8, 26);
    }

    // ───────────────── Task 3：needs_zero 与 kind ─────────────────

    use super::{kind_of, needs_zero};
    use rgoc_const::BigInt;
    use rgoc_const::value::Const;
    use rgoc_types::table::{Field, TypeTable};
    use rgoc_types::{BasicId, TypeId};

    fn f(name: &str, typ: TypeId) -> Field {
        Field {
            name: Some(name.to_string()),
            typ,
            tag: None,
            embedded: false,
        }
    }

    fn array_of(tt: &mut TypeTable, n: i64, elem: TypeId) -> TypeId {
        let c = tt.consts_mut().add(Const::Int(BigInt::from_i64(n)));
        tt.add_array(Some(c), elem)
    }

    /// `T-MEM-04`：需要清零的类型集合。
    ///
    /// 判据的**两侧都要有**：只说「含指针要清」会漏浮点（`0.0` 的位模式不是
    /// 全零，`math.Float64bits(0.0) == 0` 只是巧合），只说「含浮点要清」会漏
    /// 指针。所以这条测试里两组都在。
    #[test]
    fn 需要清零的类型集合() {
        let mut tt = TypeTable::new();
        let int_ = tt.add_basic(BasicId::Int);
        let f64_ = tt.add_basic(BasicId::Float64);
        let f32_ = tt.add_basic(BasicId::Float32);
        let c64 = tt.add_basic(BasicId::Complex64);
        let string_ = tt.add_basic(BasicId::String);
        let p_int = tt.add_pointer(int_);
        let s_int = tt.add_slice(int_);
        let a2_int = array_of(&mut tt, 2, int_);
        let a2_f32 = array_of(&mut tt, 2, f32_);

        // 不需要清：纯整数/布尔的标量与聚合
        assert!(!needs_zero(&tt, int_), "纯整数不需要");
        let bool_ = tt.add_basic(BasicId::Bool);
        assert!(!needs_zero(&tt, bool_), "布尔不需要");
        let pure = tt.add_struct(vec![f("a", int_), f("b", int_)]);
        let pure_ty = tt.struct_type(pure);
        assert!(!needs_zero(&tt, pure_ty), "纯整数聚合不需要");
        assert!(!needs_zero(&tt, a2_int), "[2]int 不需要");

        // 需要清：含指针
        assert!(needs_zero(&tt, p_int), "指针需要（nil 是全零，但要明确）");
        assert!(needs_zero(&tt, string_), "string 含指针");
        assert!(needs_zero(&tt, s_int), "slice 含指针");
        let with_ptr = tt.add_struct(vec![f("a", int_), f("p", p_int)]);
        let with_ptr_ty = tt.struct_type(with_ptr);
        assert!(needs_zero(&tt, with_ptr_ty), "含指针的结构体");

        // 需要清：含浮点（这一组是「只判指针」的实现会漏掉的）
        assert!(needs_zero(&tt, f64_), "0.0 的位模式不是全零");
        assert!(needs_zero(&tt, c64), "complex64 含浮点");
        assert!(needs_zero(&tt, a2_f32), "[2]float32 含浮点");
        let with_f = tt.add_struct(vec![f("a", int_), f("f", f32_)]);
        let with_f_ty = tt.struct_type(with_f);
        assert!(needs_zero(&tt, with_f_ty), "浮点判定必须递归进结构体");
    }

    /// `T-META-01` 的邻居：`kind` 逐个对上 `Type` 的变体。
    #[test]
    fn kind_与类型变体一一对应() {
        let mut tt = TypeTable::new();
        let int_ = tt.add_basic(BasicId::Int);
        let uint_ = tt.add_basic(BasicId::Uint);
        let f64_ = tt.add_basic(BasicId::Float64);
        let string_ = tt.add_basic(BasicId::String);
        let p_int = tt.add_pointer(int_);
        let s_int = tt.add_slice(int_);
        let m = tt.add_map(int_, int_);
        let c = tt.add_chan(rgoc_types::typ::ChanDir::SendRecv, int_);
        let params = tt.add_tuple(vec![]);
        let sig = tt.add_signature(None, params, params, false);
        let iid = tt.add_interface(vec![], None, rgoc_types::IfaceTSet::All);

        assert_eq!(kind_of(&tt, int_), Kind::Int);
        assert_eq!(kind_of(&tt, uint_), Kind::Uint);
        assert_eq!(kind_of(&tt, f64_), Kind::Float64);
        assert_eq!(kind_of(&tt, string_), Kind::String);
        assert_eq!(kind_of(&tt, p_int), Kind::Pointer);
        assert_eq!(kind_of(&tt, s_int), Kind::Slice);
        assert_eq!(kind_of(&tt, m), Kind::Map);
        assert_eq!(kind_of(&tt, c), Kind::Chan);
        assert_eq!(kind_of(&tt, tt.signature_type(sig)), Kind::Func);
        assert_eq!(kind_of(&tt, tt.interface_type(iid)), Kind::Interface);
        let bool_ = tt.add_basic(BasicId::Bool);
        assert_eq!(kind_of(&tt, bool_), Kind::Bool);

        // 逐宽度基本类型（M12/Task 31 起官方 27 档）：int8 必须是 3 而不是
        // 2 —— 15 档分组子集时代的真错（实测 `dump(reflect.TypeOf(int8(1)))`
        // 打 2，`int8` 的 Size 对、Kind/Name 全错位）。
        let i8_ = tt.add_basic(BasicId::Int8);
        let i16_ = tt.add_basic(BasicId::Int16);
        let i32_ = tt.add_basic(BasicId::Int32);
        let i64_ = tt.add_basic(BasicId::Int64);
        let u8_ = tt.add_basic(BasicId::Uint8);
        let up_ = tt.add_basic(BasicId::Uintptr);
        let f32_ = tt.add_basic(BasicId::Float32);
        let c128_ = tt.add_basic(BasicId::Complex128);
        let uptr_ = tt.add_basic(BasicId::UnsafePointer);
        assert_eq!(kind_of(&tt, i8_), Kind::Int8);
        assert_eq!(kind_of(&tt, i16_), Kind::Int16);
        assert_eq!(kind_of(&tt, i32_), Kind::Int32);
        assert_eq!(kind_of(&tt, i64_), Kind::Int64);
        assert_eq!(kind_of(&tt, u8_), Kind::Uint8);
        assert_eq!(kind_of(&tt, up_), Kind::Uintptr);
        assert_eq!(kind_of(&tt, f32_), Kind::Float32);
        assert_eq!(kind_of(&tt, c128_), Kind::Complex128);
        assert_eq!(kind_of(&tt, uptr_), Kind::UnsafePointer);

        // 具名类型看本体（`type MyInt int` ⇒ Int）
        let n = tt.add_named("MyInt");
        tt.set_named_underlying(n, int_);
        assert_eq!(kind_of(&tt, tt.named_type(n)), Kind::Int);
    }

    /// 布局给不出来的类型 ⇒ `kind_of` 返回 `Invalid`，`DescBuilder::new` 返回 `None`。
    #[test]
    fn 无布局的类型不给假_desc() {
        let mut tt = TypeTable::new();
        let int_ = tt.add_basic(BasicId::Int);
        let an = tt.add_array(None, int_); // [...]int
        assert_eq!(kind_of(&tt, an), Kind::Invalid);
        assert!(
            super::DescBuilder::new(&tt, an).is_none(),
            "[...]T 不许给假尺寸"
        );
    }

    /// `DescBuilder` 把三个 flag 填对（`T-META-03` 的一半）。
    #[test]
    fn desc_builder_填_size_kind_flags() {
        let mut tt = TypeTable::new();
        let int_ = tt.add_basic(BasicId::Int);
        let string_ = tt.add_basic(BasicId::String);
        let p_int = tt.add_pointer(int_);

        let d = super::DescBuilder::new(&tt, int_)
            .expect("int 有布局")
            .build();
        assert_eq!(d.size, 8);
        assert_eq!(d.kind, Kind::Int as u8);
        assert!(d.is_comparable(), "int 可比较");
        assert!(!d.has_pointers(), "int 无指针");
        assert!(!d.needs_zero(), "int 不用清零");

        let d = super::DescBuilder::new(&tt, string_)
            .expect("string 有布局")
            .build();
        assert_eq!(d.size, 16);
        assert_eq!(d.kind, Kind::String as u8);
        assert!(d.is_comparable(), "string 可比较");
        assert!(d.has_pointers(), "string 有指针");
        assert!(d.needs_zero(), "string 要清零");

        let d = super::DescBuilder::new(&tt, p_int)
            .expect("指针有布局")
            .build();
        assert_eq!(d.size, 8);
        assert_eq!(d.kind, Kind::Pointer as u8);
        assert!(d.is_comparable(), "指针可比较");
        assert!(d.has_pointers());

        // 不可比较的两类：slice 与 map（`==` 只在 nil 上有意义，`C8` 已管）
        let s_int = tt.add_slice(int_);
        let d = super::DescBuilder::new(&tt, s_int).unwrap().build();
        assert!(!d.is_comparable(), "slice 不可比较");
    }

    // ───────────────── Task 4：类型 hash ─────────────────

    use super::type_hash;

    /// `T-META-05`：hash 稳定且能区分。
    ///
    /// 三条独立的性质，各自能单独失败：
    /// ① 同一类型两次同值（纯函数）；② 相邻类型不撞车（区分度）；
    /// ③ 结构敏感（字段序进 hash）。
    #[test]
    fn 类型_hash_稳定且区分() {
        let mut tt = TypeTable::new();
        let int_ = tt.add_basic(BasicId::Int);
        let i64_ = tt.add_basic(BasicId::Int64);
        let s_int = tt.add_slice(int_);
        let s_i64 = tt.add_slice(i64_);

        assert_eq!(type_hash(&tt, int_), type_hash(&tt, int_), "同类型两次同值");

        let four = [
            type_hash(&tt, int_),
            type_hash(&tt, i64_),
            type_hash(&tt, s_int),
            type_hash(&tt, s_i64),
        ];
        let uniq: std::collections::BTreeSet<u32> = four.iter().copied().collect();
        assert_eq!(uniq.len(), 4, "四者互不相同：{four:?}");

        // 字段序进 hash
        let ab = tt.add_struct(vec![f("a", int_), f("b", int_)]);
        let ba = tt.add_struct(vec![f("b", int_), f("a", int_)]);
        assert_ne!(
            type_hash(&tt, tt.struct_type(ab)),
            type_hash(&tt, tt.struct_type(ba)),
            "字段序进 hash"
        );

        // 字段名进 hash（`struct{a int}` 与 `struct{b int}` 是不同的类型）
        let bx = tt.add_struct(vec![f("x", int_)]);
        let by = tt.add_struct(vec![f("y", int_)]);
        assert_ne!(
            type_hash(&tt, tt.struct_type(bx)),
            type_hash(&tt, tt.struct_type(by)),
            "字段名进 hash"
        );
    }

    /// **这条是「不许用 `TypeId` 下标」的牙**。
    ///
    /// 同一个类型在两个独立的 `TypeTable` 里下标必然不同（一个是 1，另一个
    /// 可能是 7）。用下标算 hash 的实现会在这里红 —— 而症状在生产里是
    /// 「同一个接口的两个实现被判定成不同类型」，或者 map 键莫名找不到。
    #[test]
    fn 跨_type_table_的同一类型_hash_相同() {
        let mk = |n_pad: usize| {
            let mut tt = TypeTable::new();
            for _ in 0..n_pad {
                let _ = tt.add_basic(BasicId::Bool);
            }
            let int_ = tt.add_basic(BasicId::Int);
            let s = tt.add_slice(int_);
            let sid = tt.add_struct(vec![f("p", s), f("n", int_)]);
            let ty = tt.struct_type(sid);
            (tt, ty)
        };
        let (t1, a) = mk(0);
        let (t2, b) = mk(5);
        assert_eq!(type_hash(&t1, a), type_hash(&t2, b), "hash 不能依赖下标");
    }

    /// 别名透明、具名不透明 —— 两者是**相反**的方向，容易写反。
    ///
    /// `type Q = []int` 与 `[]int` 是**同一个**类型（规范：别名不是新类型）；
    /// `type MyInt int` 与 `int` 是**不同**的类型（可赋值但不可互换）。
    #[test]
    fn 别名透明而具名不透明() {
        let mut tt = TypeTable::new();
        let int_ = tt.add_basic(BasicId::Int);
        let s_int = tt.add_slice(int_);

        let alias = tt.add_alias("Q");
        tt.set_alias_rhs(alias, s_int);
        assert_eq!(
            type_hash(&tt, tt.alias_type(alias)),
            type_hash(&tt, s_int),
            "type Q = []int 与 []int 是同一个类型"
        );

        let named = tt.add_named("MyInt");
        tt.set_named_underlying(named, int_);
        assert_ne!(
            type_hash(&tt, tt.named_type(named)),
            type_hash(&tt, int_),
            "type MyInt int 与 int 是不同的类型"
        );

        // 两个**同名本体、不同名字**的具名类型也必须不同 ——
        // 这条才是「名字进 hash」的牙：只靠 tag 与本体，`type A int` 与
        // `type B int` 会撞成同一个 hash（而它们是两个可区分的类型，
        // 接口断言 `x.(A)` 与 `x.(B)` 的结果不同）。
        let na = tt.add_named("A");
        tt.set_named_underlying(na, int_);
        let nb = tt.add_named("B");
        tt.set_named_underlying(nb, int_);
        assert_ne!(
            type_hash(&tt, tt.named_type(na)),
            type_hash(&tt, tt.named_type(nb)),
            "type A int 与 type B int 的 hash 必须不同"
        );
    }

    // ── 方法表的两条 hash（`M9/Task 27`）────────────────────────────

    /// 名字 hash：同名同 hash、不同名不同 hash。
    ///
    /// 「不同名不同 hash」是**判据的牙**：`rt_itab` 靠它查方法，撞了就会调错方法
    /// （症状是「方法被调了但算错」，比崩溃更难查）。
    #[test]
    fn 方法名_hash_有区分度() {
        assert_eq!(name_hash("M"), name_hash("M"), "同名必须同 hash");
        assert_ne!(name_hash("M"), name_hash("N"));
        assert_ne!(name_hash("A"), name_hash("B"));
        // 长度不同的前缀不撞（`mix_bytes` 喂了长度）
        assert_ne!(name_hash("M"), name_hash("MM"));
        assert_ne!(name_hash(""), name_hash("M"));
    }

    /// 签名 hash：`(params, results)` 的**结构**进 hash，名字不进。
    ///
    /// 为什么名字不进：接口 `interface{ M(x int) }` 与实现 `func (t T) M(y int)`
    /// 的参数名可以不同，但它们是**同一个**签名 —— 名字进 hash 会让合法的实现
    /// 被判成「没实现」。
    #[test]
    fn 签名_hash_只看结构不看参数名() {
        let mut tt = TypeTable::new();
        let int_ = tt.add_basic(BasicId::Int);
        let str_ = tt.add_basic(BasicId::String);
        let bool_ = tt.add_basic(BasicId::Bool);

        // 同名同结构 ⇒ 同 hash（与参数名无关：这里干脆用切片，没有名字）
        assert_eq!(
            sig_hash(&tt, &[int_], &[str_]),
            sig_hash(&tt, &[int_], &[str_])
        );

        // 参数类型不同 ⇒ 不同
        assert_ne!(sig_hash(&tt, &[int_], &[]), sig_hash(&tt, &[str_], &[]));
        // 结果类型不同 ⇒ 不同
        assert_ne!(sig_hash(&tt, &[], &[int_]), sig_hash(&tt, &[], &[str_]));
        // 参数个数不同 ⇒ 不同
        assert_ne!(
            sig_hash(&tt, &[int_], &[]),
            sig_hash(&tt, &[int_, int_], &[])
        );
        // 结果个数不同 ⇒ 不同
        assert_ne!(
            sig_hash(&tt, &[], &[int_]),
            sig_hash(&tt, &[], &[int_, int_])
        );
        // 参数与结果**不能互换**：`M(int) bool` 与 `M(bool) int` 必须不同
        assert_ne!(
            sig_hash(&tt, &[int_], &[bool_]),
            sig_hash(&tt, &[bool_], &[int_]),
            "参数/结果的边界必须进 hash"
        );
    }

    /// `MethodEntry` 是 16 字节、`MethodTable` 表头 8 字节 —— `C5` §10 的冻结形状。
    ///
    /// 表头之后**紧跟**变长的 `entries`（不是指针），所以 `entries` 的偏移是 8。
    /// 这条判据是 `rt_itab` 的地址算术（`(char*)mt + 8 + 16*i`）的唯一依据。
    #[test]
    fn 方法表的字节形状() {
        assert_eq!(core::mem::size_of::<MethodEntry>(), 16);
        assert_eq!(core::mem::size_of::<MethodTable>(), 8);
        assert_eq!(core::mem::offset_of!(MethodTable, count), 0);
        assert_eq!(core::mem::offset_of!(MethodEntry, name_hash), 0);
        assert_eq!(core::mem::offset_of!(MethodEntry, sig_hash), 4);
        assert_eq!(core::mem::offset_of!(MethodEntry, func), 8);
    }
}
