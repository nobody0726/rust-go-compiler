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
//! | `Load`（地址**可证非空**） | ✅ 若 dst 没人用 | 读一块合法存储没有副作用 |
//! | `Load`（地址来历不明） | ❌ | 读空指针是 **panic**（`M9/Phase 5c`，见下） |
//!
//! # `Load` 的陷阱（`R-9-21`）
//!
//! 「读没有副作用」这句话**只在地址非空时**成立。空指针上的一条 `Load` 会
//! `rt_panic_nil` —— 而 panic 是可观察的（`recover` 能接住、退出码是 2）。
//!
//! 踩到的语料是 `method.go` 的提升链：`a.h()`（`h` 是 `D` 的**值接收者**，
//! 而 `a.B.D` 是 `*D` 且为 `nil`）在 IR 里正是 `load (load (&a.B.D))`，
//! 那条内层 `load` 的结果就是接收者。当被调方法体**是空的**（`func (D) h() {}`）
//! 时，`Call` 的 effect 全空 ⇒ 调用被删 ⇒ 接收者变死 ⇒ `load` 跟着被删 ⇒
//! **panic 消失**，程序从「退出码 2」变成「静默通过」。
//!
//! 修法不是「Load 永不删」（那会把 `a.f()` 这类合法读全钉住），而是只钉
//! **地址来历不明**的那些：地址由 `AddrOf`（根的存储）、`Make`（新分配）、
//! 或它们派生出来的 `Field`/`Index` 给出的，一定是非空；其余（形参、调用结果、
//! 读出来的指针值……）无从证明，读它就留着 —— 少删一条死读，换 panic 不丢。
//!
//! # 死块
//!
//! `V8` 要求所有块从入口可达。`fold` 把 `if true` 改成 `goto` 之后另一条边
//! 就断了，所以本 pass 收尾时统一清一次（[`super::prune_dead_blocks`]）。

use std::collections::{BTreeMap, BTreeSet};

use crate::ir::{CalleeRef, Instr, SsaFunc, ValueId};

use super::{compact_values, prune_dead_blocks, pure_effect, used_values};

/// 跑一遍 DCE（到不动点：删一条可能让它的操作数变死）。
pub fn run(f: &mut SsaFunc) {
    loop {
        let used = used_values(f);
        let defs = defs_of(f);
        let mut removed = false;
        for b in &mut f.blocks {
            let before = b.instrs.len();
            b.instrs.retain(|i| keep(i, &used, &defs));
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

/// 值 → 定义它的指令（`provably_nonnull` 要顺着定义点往回走）。
///
/// 存**摘要**而不是 `&Instr`：`keep` 在 `f.blocks` 的可变借用里跑，
/// 拿不到另一个不可变借用（`E0502`）。
fn defs_of(f: &SsaFunc) -> BTreeMap<ValueId, Def> {
    let mut m = BTreeMap::new();
    for b in &f.blocks {
        for i in &b.instrs {
            let d = match i {
                Instr::AddrOf { .. }
                | Instr::Make { .. }
                | Instr::FuncAddr { .. }
                | Instr::MakeClosure { .. } => Def::NonNull,
                Instr::Field { base, .. } | Instr::Index { base, .. } => Def::Derived(*base),
                Instr::Copy { src, .. } => Def::Derived(*src),
                Instr::Convert { x, .. } => Def::Derived(*x),
                _ => Def::Other,
            };
            for v in defined(i) {
                m.insert(v, d);
            }
        }
    }
    m
}

/// 一个地址值的「非空来历」（只在 `defs` 里出现这三类）。
#[derive(Clone, Copy)]
enum Def {
    /// 构造上非空（根的地址 / 新分配 / 符号地址）。
    NonNull,
    /// 由另一个值派生（偏移 / 改名）。
    Derived(ValueId),
    /// 其他一切（常量、调用结果、读出来的指针值、二元运算……）。
    Other,
}

/// 这条指令定义的值（多结果全列；`Store` 的 mem 令牌也算）。
fn defined(i: &Instr) -> Vec<ValueId> {
    match i {
        Instr::Call { dsts, .. } => dsts.clone(),
        Instr::Defer { .. } => Vec::new(),
        Instr::NilCheck { .. } => Vec::new(),
        other => dst_of(other).into_iter().collect(),
    }
}

/// 这个地址值**一定非空**吗？
///
/// 认两种来源：`AddrOf`（任何根都有自己的存储）与 `Make` / `FuncAddr` /
/// `MakeClosure`（新分配、符号地址），以及从它们派生出来的 `Field` / `Index`
/// （非空基址 + 偏移仍然非空）、`Copy` / `Convert`（同一块地址换个名字）。
/// 其余一律「不敢保证」—— 说 `false` 只让死读多留一条，不会让 panic 丢掉。
fn provably_nonnull(defs: &BTreeMap<ValueId, Def>, v: ValueId, depth: u32) -> bool {
    if depth > 32 {
        return false;
    }
    match defs.get(&v) {
        Some(Def::NonNull) => true,
        Some(Def::Derived(src)) => provably_nonnull(defs, *src, depth + 1),
        // 块参数 / 形参：来历不明
        Some(Def::Other) | None => false,
    }
}

fn keep(i: &Instr, used: &BTreeSet<ValueId>, defs: &BTreeMap<ValueId, Def>) -> bool {
    match i {
        // 写内存：永远留（它的 mem 令牌没人用是常态，不是死代码）
        Instr::Store { .. } => true,
        // `defer` 的**唯一效果就是登记**：它不定义值，`fv` 也可能没人再读
        // （登记完就只剩运行期的链）。删掉它就等于把 defer 变没了。
        Instr::Defer { .. } => true,
        // 空指针检查**没有结果**：它的全部意义就是「这里可能 panic」。
        // 删它 = 把 panic 变没（`nilptr2.go` 的 `&*p` 正是这样）。
        Instr::NilCheck { .. } => true,
        // 类型断言：单值形态**会 panic**（失败即 `rt_panic_assert`），
        // comma-ok 形态的 `ok` 也是一个可观察的结果 ⇒ 两个 dst 都没人用才删。
        Instr::TypeAssert { dst, ok, .. } => {
            used.contains(dst) || ok.is_some_and(|o| used.contains(&o))
        }
        // 读内存：地址可证非空才是纯的（空指针上的读 = 可恢复的 panic，`R-9-21`）
        Instr::Load { dst, addr, .. } => used.contains(dst) || !provably_nonnull(defs, *addr, 0),
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
        Instr::FuncAddr { dst, .. } | Instr::CapturePtr { dst, .. } => *dst,
        Instr::MakeClosure { dst, .. } => *dst,
        Instr::TypeAssert { dst, .. } => *dst,
        // `defer` 没有 dst（`keep` 已经保证它永远活着）
        Instr::Defer { .. } => return None,
        // `nil-check` 也没有 dst（同上）
        Instr::NilCheck { .. } => return None,
        // `Call` 由调用方特判（多结果），走到这里说明有人漏了
        Instr::Call { .. } => return None,
    })
}
