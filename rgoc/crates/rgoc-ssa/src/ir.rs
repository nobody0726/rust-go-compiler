//! IR 的标识符与容器（M6/Task 2 骨架；完整形状在 Task 7 落地）。
//!
//! # 为什么 ID 是 newtype 而不是 `usize`
//!
//! `C4` §1.1 的结论：**「唯一定义点」要由类型系统保证**。`ValueId` 只能由构建器产出，
//! 且不能与 `BlockId` / `RootId` 混用 —— 后者是「块」和「内存根」，混用一次就会
//! 让 verifier 的 `V1`/`V7`/`V9` 同时失效，而且**编译期查不出来**。
//!
//! 三个 ID 都是 `u32`（单函数值数 / 块数 / 根数都在 10^4 量级），
//! `index()` 是唯一允许的「降级」入口，便于 `Vec` 下标。

/// 值下标（`SsaFunc` 内唯一）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ValueId(pub u32);

impl ValueId {
    /// 转成 `Vec` 下标。
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// 块下标（`SsaFunc::blocks` 的索引）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct BlockId(pub u32);

impl BlockId {
    /// 转成 `Vec` 下标。
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// 内存根下标（`SsaFunc::roots` 的索引；`D-M6-5` 的「按根分区」）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RootId(pub u32);

impl RootId {
    /// 转成 `Vec` 下标。
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// 一个模块（单包；`D-M6-2`）。
#[derive(Clone, Debug, Default)]
pub struct Module {
    /// 全部函数。
    pub funcs: Vec<SsaFunc>,
}

/// 一个函数的 SSA 形态（Task 2 只带 dump 需要的三个字段；Task 7 补全）。
#[derive(Clone, Debug, Default)]
pub struct SsaFunc {
    /// 显示名（`main` / `f` / `T.M`）。
    pub name: String,
    /// 块数。
    pub blocks: usize,
    /// 值数。
    pub values: usize,
}
