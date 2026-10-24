//! 复制传播（`M6-design.md` §3.6；`T-SSA-62`）。
//!
//! # 做什么
//!
//! `Copy` 是**重命名**（构建期用它把「块参数交接」「结果直连」落成值）。
//! 它没有副作用，也不改变值 —— 所以可以整条消掉：把使用点改指向源，
//! 然后删掉这条 `Copy`。
//!
//! # 并行语义为什么不会破
//!
//! 会破的写法是「按顺序做替换」：`a, b = b, a` 编译成
//! `Goto { args: [b, a] }`，如果替换时**先**把 `a` 改写成某个值再拿它当 `b` 的源，
//! 交换就变成「两个都是 b」。这里的做法是：
//!
//! 1. 先把整张 `dst → 源` 映射**全部**建好（源也解析到链的终点）；
//! 2. 再对使用点做一次**同时**替换（每个使用点只查一次表）。
//!
//! 映射是一次性快照，替换顺序因此无关 —— 这就是并行语义要的「同时」。

use std::collections::BTreeMap;

use crate::ir::{Instr, SsaFunc, ValueId};

use super::{compact_values, subst};

/// 跑一遍复制传播。
pub fn run(f: &mut SsaFunc) {
    let mut map: BTreeMap<ValueId, ValueId> = BTreeMap::new();
    for b in &f.blocks {
        for i in &b.instrs {
            let Instr::Copy { dst, src } = i else {
                continue;
            };
            // 解析到链的终点（`Copy` 的源也可能是另一个 `Copy` 的 dst）
            let mut t = *src;
            let mut guard = 0;
            while let Some(&n) = map.get(&t) {
                if n == t || guard > 64 {
                    break;
                }
                t = n;
                guard += 1;
            }
            if t != *dst {
                map.insert(*dst, t);
            }
        }
    }
    // 先删 `Copy` 再替换：替换会把 `Copy.src` 也改掉，而它们马上要消失，
    // 顺序反了就得为「已删的指令」再写一遍遍历。
    for b in &mut f.blocks {
        b.instrs.retain(|i| !matches!(i, Instr::Copy { .. }));
    }
    subst(f, &map);
    compact_values(f);
}
