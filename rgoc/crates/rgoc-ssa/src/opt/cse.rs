//! 块内公共子表达式消除（`M6-design.md` §3.6；`T-SSA-63`）。
//!
//! # 为什么只做**块内**
//!
//! 跨块 CSE 要回答「这个表达式在上游所有路径上都算过吗」—— 那是**可用表达式**
//! 分析（数据流到不动点），属 M7。块内不需要：同一个块里，先出现的指令必然
//! 支配后出现的指令，`V1/V2` 保证操作数在两条指令处是同一个值。
//!
//! # 哪些指令能合
//!
//! 只合**纯**指令：同样的操作数 ⇒ 同样的结果，且没有副作用。
//! `Load` 不行（中间可能有 `Store`）、`Store` 不行（副作用）、`Call` 不行
//! （除非 effect 全空 —— 但那样 `dce` 会先删掉它，这里不必重复判断）。
//!
//! `Bin` **不**利用交换律（`a+b` 与 `b+a` 合一）：浮点加法不满足结合/交换的
//! 位级等价（`NaN` 载荷、`-0.0`），为省一条指令引入这种不确定性不划算。

use std::collections::BTreeMap;

use crate::ir::{Instr, SsaFunc, ValueId};

use super::{compact_values, instr_uses, term_uses};

/// 块内的「表达式指纹」。字段顺序即比较顺序（`Ord` 用来做 `BTreeMap` 的键）。
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Bin(u8, ValueId, ValueId, u32),
    Un(u8, ValueId, u32),
    Convert(ValueId, u32),
    AddrOf(u32),
    Field(ValueId, usize),
    Index(ValueId, ValueId),
}

fn key_of(i: &Instr) -> Option<Key> {
    Some(match i {
        Instr::Bin {
            op, lhs, rhs, typ, ..
        } => Key::Bin(*op as u8, *lhs, *rhs, typ.index() as u32),
        Instr::Un { op, x, typ, .. } => Key::Un(*op as u8, *x, typ.index() as u32),
        Instr::Convert { x, typ, .. } => Key::Convert(*x, typ.index() as u32),
        Instr::AddrOf { root, .. } => Key::AddrOf(root.0),
        Instr::Field { base, index, .. } => Key::Field(*base, *index),
        Instr::Index { base, i, .. } => Key::Index(*base, *i),
        // `Load` 依赖内存状态；`Composite`/`Make`/`Slice` 有分配语义；`Call` 可能有效果
        _ => return None,
    })
}

fn dst_of(i: &Instr) -> Option<ValueId> {
    Some(match i {
        Instr::Bin { dst, .. }
        | Instr::Un { dst, .. }
        | Instr::Convert { dst, .. }
        | Instr::AddrOf { dst, .. }
        | Instr::Field { dst, .. }
        | Instr::Index { dst, .. } => *dst,
        _ => return None,
    })
}

/// 跑一遍块内 CSE。
pub fn run(f: &mut SsaFunc) {
    for b in &mut f.blocks {
        let mut seen: BTreeMap<Key, ValueId> = BTreeMap::new();
        let mut map: BTreeMap<ValueId, ValueId> = BTreeMap::new();
        let mut keep: Vec<Instr> = Vec::with_capacity(b.instrs.len());
        for mut i in std::mem::take(&mut b.instrs) {
            // 同一块里先出现的支配后出现的 ⇒ 就地替换操作数是安全的
            instr_uses(&mut i, &mut |v| {
                if let Some(&n) = map.get(v) {
                    *v = n;
                }
            });
            if let Some(k) = key_of(&i) {
                let d = dst_of(&i).expect("key_of 只认有 dst 的指令");
                if let Some(&prev) = seen.get(&k) {
                    map.insert(d, prev);
                    continue;
                }
                seen.insert(k, d);
            }
            keep.push(i);
        }
        b.instrs = keep;
        // 终止符也在同一块里：它的操作数同样要换
        term_uses(&mut b.term, &mut |v| {
            if let Some(&n) = map.get(v) {
                *v = n;
            }
        });
    }
    compact_values(f);
}
