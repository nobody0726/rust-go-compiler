//! 类型的**表示**（M3/Task 13，`T-TY-11`）。
//!
//! 15 个变体，与 `docs/contracts/C7-type-dump.md` §2.1 的标签集**一一对应** ——
//! 这是刻意的：契约里的「变体数即标签数」在两侧都是可执行的
//! （oracle 的 `line()` 是穷尽 `switch`，本文件的 `variant_label()` 是穷尽 `match`，
//! `T-TY-11` 再钉住数字 15）。
//!
//! # 表示做宽、判定做窄（`D-M3-2`）
//!
//! `typeparam` / `union` **在这里有位置**（要能 dump、要能表示 `any` 与 `~T`）；
//! 同一性判定（[`crate::identical`]）自 M4/Task 10 起也按官方独立形态判它们
//! （`U-M3-05` 解除 —— 类型参数「同一条目才真」、并集比项表集合相等）。
//! 「表示」与「判定」分开，是为了先让 dump 面覆盖泛型形状，
//! 再让判定面按可证明的范围逐段扩大。
//!
//! # 三种「定义」的区分
//!
//! | | 是什么 | 同一性 | dump 标签 |
//! |---|---|---|---|
//! | [`BasicId`] | 预声明基本类型（`int`、`string`…） | 按**种类**（每个只剩一个实例） | `basic` |
//! | [`NamedId`] | `type D ...` 定义出的类型 | 按**定义身份**（`D` 与 `E` 同名不同定义不相等） | `named` |
//! | [`AliasId`] | `type D = T` 定义出的别名 | 按**定义身份**，但同一性判定要**穿透**到 RHS | `alias` |
//!
//! `Invalid`（`invalid` 标签）是**第 15 个变体**，不在 `BasicId` 里 ——
//! `C7` §4.1 的实测订正：`go/types` 的 `BasicKind` 有 26 个，其中 `Invalid`
//! 单独走 `invalid` 标签，`BasicId` 只对账 25 个。

use rgoc_const::{BasicId, ConstId};
/// 定义一组 `u32` 下标 newtype。
///
/// 为什么**不用** `enum` 做标识：这些下标要能 `Copy`、要能当 `HashMap` 键、
/// 要能比较大小（dump 的编号是单调走访出来的）。`u32` 带一个不变量
/// （「只能由本表产出」）就是 newtype 的全部职责。
macro_rules! id_newtype {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub struct $name(u32);

        impl $name {
            /// 从下标构造 —— 给 `TypeTable` 与测试用。
            ///
            /// # Panics
            ///
            /// 下标超出 `u32` 时 panic（下标来自 `Vec::len`，不会真的超）。
            /// **越界的「有效下标」不在这里查**：`TypeId` 的不变量由
            /// 「只能由类型表产出」保证，用别处的数字构造它会得到一张查不到的表 ——
            /// 那是调用方的缺陷，不是这里能判的。
            #[must_use]
            pub fn from_index(i: usize) -> Self {
                Self(u32::try_from(i).expect("类型表下标超出 u32"))
            }

            /// 取下标。
            #[must_use]
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id_newtype! {
    /// 类型条目的下标（`C7` 的 `<序号>` 与它同源但**不是**同一个编号，
    /// 见 [`crate::dump`] 的说明）。
    TypeId
}
id_newtype! {
    /// `named` 定义的标识（`type D ...`）。
    NamedId
}
id_newtype! {
    /// `alias` 定义的标识（`type D = T`）。
    AliasId
}
id_newtype! {
    /// `struct` 条目的标识。
    StructId
}
id_newtype! {
    /// `signature` 条目的标识。
    SignatureId
}
id_newtype! {
    /// `tuple` 条目的标识。
    TupleId
}
id_newtype! {
    /// `interface` 条目的标识。
    InterfaceId
}
id_newtype! {
    /// `typeparam` 条目的标识。
    TypeParamId
}
id_newtype! {
    /// `union` 条目的标识。
    UnionId
}

/// 通道方向（`C7` §4.1 的 `chan <dir>`；三个拼写与 oracle 一致）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ChanDir {
    SendRecv,
    SendOnly,
    RecvOnly,
}

impl ChanDir {
    /// dump 文本（`C7` §4.1）。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ChanDir::SendRecv => "sendrecv",
            ChanDir::SendOnly => "sendonly",
            ChanDir::RecvOnly => "recvonly",
        }
    }
}

/// 类型条目的**表示**。
///
/// 凡是能互相引用的地方一律用 [`TypeId`]（arena 下标）—— 用 `Box` 会造出
/// 「同一类型两个身份」的树，而类型必须能按身份去重（`C7` §3 的 `named` 去重）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Type {
    /// 预声明基本类型（25 种；`invalid` **不在这里**）。
    Basic(BasicId),
    /// `type D ...` 定义的类型。本体经 [`NamedId`] 回查（可以被后填 ⇒ 递归类型可表示）。
    Named(NamedId),
    /// `type D = T` 定义的别名。
    Alias(AliasId),
    /// `*T`。
    Pointer(TypeId),
    /// `[]T`。
    Slice(TypeId),
    /// `[n]T` / `[...]T`（`len == None` ⇒ 长度未定，dump 写 `-`）。
    Array {
        /// 长度：常量表下标；`None` ⇒ `[...]T`。
        len: Option<ConstId>,
        /// 元素类型。
        elem: TypeId,
    },
    /// `map[K]V`。
    Map {
        /// 键类型。
        key: TypeId,
        /// 值类型。
        elem: TypeId,
    },
    /// `chan T` / `<-chan T` / `chan<- T`。
    Chan {
        /// 方向。
        dir: ChanDir,
        /// 元素类型。
        elem: TypeId,
    },
    /// `struct{...}`。**不去重**（`C7` §3）。
    Struct(StructId),
    /// 函数 / 方法签名。**不去重**。
    Signature(SignatureId),
    /// 参数 / 结果元组。**不去重**。
    Tuple(TupleId),
    /// 接口（方法集与可选的项集）。
    Interface(InterfaceId),
    /// 类型参数（泛型）。表示有位置、判定不做（`D-M3-2`）。
    TypeParam(TypeParamId),
    /// 类型集合的并集（`~int | ~string`）。
    Union(UnionId),
    /// 错误的占位类型（源码有错时的静默载体）。
    Invalid,
}

impl Type {
    /// dump 标签（`C7` §2.1 的 15 个）。**契约的一部分**：改动等于契约变更。
    #[must_use]
    pub fn variant_label(&self) -> &'static str {
        match self {
            Type::Basic(_) => "basic",
            Type::Named(_) => "named",
            Type::Alias(_) => "alias",
            Type::Pointer(_) => "pointer",
            Type::Slice(_) => "slice",
            Type::Array { .. } => "array",
            Type::Map { .. } => "map",
            Type::Chan { .. } => "chan",
            Type::Struct(_) => "struct",
            Type::Signature(_) => "signature",
            Type::Tuple(_) => "tuple",
            Type::Interface(_) => "interface",
            Type::TypeParam(_) => "typeparam",
            Type::Union(_) => "union",
            Type::Invalid => "invalid",
        }
    }

    /// 是不是**定义型**（`named` / `alias`）—— `C7` §2.2 里引用要写 `#<序号>` 的两类，
    /// 也是 §3 里「去重 + 先占号再下钻本体」的两类。
    #[must_use]
    pub fn is_definition(&self) -> bool {
        matches!(self, Type::Named(_) | Type::Alias(_))
    }
}

/// [`Type`] 的变体数 —— `C7` §2.1 / `T-TY-11` 的穷尽性断言。
///
/// 为什么写成常量而不是 `#[cfg(test)]` 里的手数：契约的「变体数即标签数」
/// 是**两侧都对账**的（oracle 的穷尽 `switch` 一旦少一个分支就 `panic`），
/// 所以这个数字必须是被引用到的代码，而不是注释里的字。
pub const TYPE_VARIANT_COUNT: usize = 15;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 变体数与标签数都是_15() {
        assert_eq!(TYPE_VARIANT_COUNT, 15);
        // 15 个标签，逐个钉住拼写（`C7` §2.1）
        let labels = [
            "basic",
            "named",
            "alias",
            "pointer",
            "slice",
            "array",
            "map",
            "chan",
            "struct",
            "signature",
            "tuple",
            "interface",
            "typeparam",
            "union",
            "invalid",
        ];
        assert_eq!(labels.len(), TYPE_VARIANT_COUNT);
        assert_eq!(labels.len(), 15);
        // 无重复（重名会让「标签 → 变体」不可逆）
        let mut sorted = labels.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 15);
    }

    #[test]
    fn 每个变体都能产出自己的标签() {
        use rgoc_const::BasicId as B;
        // 这条 match 是**穷尽**的：删掉任何一个变体，编译器当场报错
        // （`T-TY-11` 的反向探针就靠这一点：见 ledger 的变异记录）。
        let all: [Type; TYPE_VARIANT_COUNT] = [
            Type::Basic(B::Int),
            Type::Named(NamedId::from_index(0)),
            Type::Alias(AliasId::from_index(0)),
            Type::Pointer(TypeId::from_index(0)),
            Type::Slice(TypeId::from_index(0)),
            Type::Array {
                len: None,
                elem: TypeId::from_index(0),
            },
            Type::Map {
                key: TypeId::from_index(0),
                elem: TypeId::from_index(0),
            },
            Type::Chan {
                dir: ChanDir::SendRecv,
                elem: TypeId::from_index(0),
            },
            Type::Struct(StructId::from_index(0)),
            Type::Signature(SignatureId::from_index(0)),
            Type::Tuple(TupleId::from_index(0)),
            Type::Interface(InterfaceId::from_index(0)),
            Type::TypeParam(TypeParamId::from_index(0)),
            Type::Union(UnionId::from_index(0)),
            Type::Invalid,
        ];
        let mut seen: Vec<&str> = all.iter().map(Type::variant_label).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), TYPE_VARIANT_COUNT, "有两个变体共用一个标签");
        assert_eq!(Type::Invalid.variant_label(), "invalid");
    }

    #[test]
    fn 只有定义型算_definition() {
        assert!(Type::Named(NamedId::from_index(0)).is_definition());
        assert!(Type::Alias(AliasId::from_index(0)).is_definition());
        assert!(!Type::Invalid.is_definition());
        assert!(!Type::Slice(TypeId::from_index(0)).is_definition());
    }

    #[test]
    fn 通道方向的三个拼写() {
        assert_eq!(ChanDir::SendRecv.as_str(), "sendrecv");
        assert_eq!(ChanDir::SendOnly.as_str(), "sendonly");
        assert_eq!(ChanDir::RecvOnly.as_str(), "recvonly");
    }
}
