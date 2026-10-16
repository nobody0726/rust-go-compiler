//! 常量表与常量下标（`D-M3-4` 的落点；pre-flight Ruling ④）。
//!
//! # 为什么常量的引用要**多一层下标**
//!
//! [`Const`] 不是 `Copy`（`BigInt` 有堆上的 limbs），而类型表必须廉价可拷贝 ——
//! 类型本身是要被频繁比较/克隆的（`Identical`、dump 的走访都会碰到）。
//! 所以「类型里引用常量」的地方（目前只有 `array` 的长度）只存 [`ConstId`]，
//! 常量本体集中在 [`ConstTable`]。
//!
//! # 去重与否
//!
//! **不去重**（`add` 一律压新槽）。理由与结构型不去重同源（`C7` §3 的实测结论）：
//! 「同一个常量值」在源码里出现两次是**两个位置**，而去重会让
//! 「引用同一个常量」与「两个恰好同值的常量」在表里变得不可区分。
//! 类型表要的是「这个类型指向哪个常量」，不是「等价类」。
//!
//! ⚠️ 与 [`crate::value::Const`] 的 `PartialEq` 分工：`ConstId` 的相等是**位置**
//! 相等（`a == b` ⟺ 同一个槽），要判**值**相等必须取出来比 `Const`。

use crate::value::Const;

/// 常量在 [`ConstTable`] 里的下标。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ConstId(u32);

impl ConstId {
    /// 从下标构造 —— **只给 [`ConstTable`] 用**（它保证下标有效）。
    #[must_use]
    pub fn from_index(i: usize) -> Self {
        Self(u32::try_from(i).expect("常量表下标超出 u32"))
    }

    /// 取下标。
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// 常量表：`ConstId` ↔ [`Const`] 的双向入口。
#[derive(Default, Clone, Debug)]
pub struct ConstTable {
    consts: Vec<Const>,
}

impl ConstTable {
    /// 空表。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 压入一个常量，返回它的下标。**不去重**（见模块文档）。
    pub fn add(&mut self, c: Const) -> ConstId {
        let id = ConstId::from_index(self.consts.len());
        self.consts.push(c);
        id
    }

    /// 按下标取值。
    ///
    /// `id` 只能由本表产出（`ConstId` 的字段是私有的），所以越界即内部缺陷。
    #[must_use]
    pub fn get(&self, id: ConstId) -> &Const {
        &self.consts[id.index()]
    }

    /// 已有多少常量。
    #[must_use]
    pub fn len(&self) -> usize {
        self.consts.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.consts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bigint::BigInt;

    #[test]
    fn 压入与回查() {
        let mut t = ConstTable::new();
        assert!(t.is_empty());
        let a = t.add(Const::Int(BigInt::from_i64(3)));
        let b = t.add(Const::Int(BigInt::from_i64(3)));
        assert_eq!(t.len(), 2);
        // **不去重**：值相同也是两个槽 ⇒ 两个不同的下标
        assert_ne!(a, b);
        assert_eq!(t.get(a), &Const::Int(BigInt::from_i64(3)));
        assert_eq!(t.get(b), t.get(a));
        assert_eq!(a.index(), 0);
        assert_eq!(b.index(), 1);
    }

    #[test]
    fn 下标可作哈希键() {
        use std::collections::HashMap;
        let mut t = ConstTable::new();
        let a = t.add(Const::Bool(true));
        let mut m: HashMap<ConstId, &'static str> = HashMap::new();
        m.insert(a, "t");
        assert_eq!(m.get(&a), Some(&"t"));
    }
}
