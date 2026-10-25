//! 死代码删除（`M6-design.md` §3.6；`T-SSA-61`）。
//!
//! # 什么算「死」
//!
//! 一个值如果**没有任何使用点**，并且产生它的指令**没有副作用**，就可以删。
//! 两条都要：
//!
//! * 只看「没人用」会把 `*p = 1` 删掉（它产的 mem 令牌没人用，但写是真的）；
//! * 只看「没副作用」会把还在被读的值删掉（`V1` 立刻红）。
//!
//! # `Store` 与 `Call` 的特殊待遇
//!
//! | 指令 | 删吗 | 为什么 |
//! |---|---|---|
//! | `Store` | ❌ 永不 | 写内存是**唯一**可观察的副作用，它的 mem 令牌没人用是常态 |
//! | `Call`（effect 全空） | ✅ 若 dsts 全没用 | `len`/`cap` 这类纯调用 |
//! | `Call`（有写 / 可能 panic / 可能回调） | ❌ | panic 与写都是可观察的 |
//! | `Call Indirect` | ❌ | 被调方未知 ⇒ effect 只是保守估计，删了可能删掉真副作用 |
//! | `Load` | ✅ 若 dst 没人用 | 读没有副作用 |
//!
//! # 死块
//!
//! `V8` 要求所有块从入口可达。`fold` 把 `if true` 改成 `goto` 之后另一条边
//! 就断了，所以本 pass 收尾时统一清一次（[`super::prune_dead_blocks`]）。

use std::collections::BTreeSet;

use crate::ir::{CalleeRef, Instr, SsaFunc, ValueId};

use super::{compact_values, prune_dead_blocks, pure_effect, used_values};

/// 跑一遍 DCE（到不动点：删一条可能让它的操作数变死）。
pub fn run(f: &mut SsaFunc) {
    loop {
        let used = used_values(f);
        let mut removed = false;
        for b in &mut f.blocks {
            let before = b.instrs.len();
            b.instrs.retain(|i| keep(i, &used));
            if b.instrs.len() != before {
                removed = true;
            }
        }
        if !removed {
            break;
        }
    }
    prune_dead_blocks(f);
    compact_values(f);
}

fn keep(i: &Instr, used: &BTreeSet<ValueId>) -> bool {
    match i {
        // 写内存：永远留（它的 mem 令牌没人用是常态，不是死代码）
        Instr::Store { .. } => true,
        // 类型断言：单值形态**会 panic**（失败即 `rt_panic_assert`），
        // comma-ok 形态的 `ok` 也是一个可观察的结果 ⇒ 两个 dst 都没人用才删。
        Instr::TypeAssert { dst, ok, .. } => {
            used.contains(dst) || ok.is_some_and(|o| used.contains(&o))
        }
        Instr::Call {
            dsts,
            callee,
            effect,
            ..
        } => {
            // 间接调用的 effect 是保守估计（`writes_all`），删它等于赌「没有副作用」
            if matches!(callee, CalleeRef::Indirect(_)) {
                return true;
            }
            if !pure_effect(effect) {
                return true;
            }
            dsts.iter().any(|d| used.contains(d))
        }
        other => dst_of(other).is_some_and(|d| used.contains(&d)),
    }
}

fn dst_of(i: &Instr) -> Option<ValueId> {
    Some(match i {
        Instr::Const { dst, .. }
        | Instr::Copy { dst, .. }
        | Instr::Bin { dst, .. }
        | Instr::Un { dst, .. }
        | Instr::Convert { dst, .. }
        | Instr::AddrOf { dst, .. }
        | Instr::Field { dst, .. }
        | Instr::Index { dst, .. }
        | Instr::Slice { dst, .. }
        | Instr::Composite { dst, .. }
        | Instr::Make { dst, .. }
        | Instr::Load { dst, .. }
        | Instr::Store { dst, .. } => *dst,
        Instr::TypeAssert { dst, .. } => *dst,
        // `Call` 由调用方特判（多结果），走到这里说明有人漏了
        Instr::Call { .. } => return None,
    })
}
