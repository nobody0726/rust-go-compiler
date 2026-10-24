//! 帧布局：把 `SsaFunc` 的每个值与每个取址槽落到**具体偏移**（`C5` §7）。
//!
//! # 形状（`C5` §7.1）
//!
//! ```text
//! 高地址
//!   ┌──────────────────────────┐
//!   │ 调用方的 outgoing 实参区   │
//!   ├──────────────────────────┤ ← fp (x29)
//!   │ saved fp                 │  +0
//!   │ saved lr                 │  +8
//!   ├──────────────────────────┤
//!   │ 值槽区（按 ValueId 升序）  │  -16, …
//!   ├──────────────────────────┤
//!   │ 取址槽区（slot_types）     │
//!   ├──────────────────────────┤
//!   │ 并行拷贝临时区             │
//!   └──────────────────────────┘ ← sp = fp - frame_size
//! ```
//!
//! # 三条被实现固定下来的事
//!
//! 1. **偏移一律是「相对 `fp` 的负数」**，且等于 `add x, sp, #off` 里那个立即数
//!    （因为 `sp = fp - frame_size`，而 `off` 已经是相对 `fp` 的）。
//!    `FRAME-MAP v1` 与 `objdump` 的对账（`T-NAT-04`）靠的就是这个等式。
//! 2. **按 `ValueId` 升序分配**：顺序是**确定性**的一部分（`T-NAT-48`）。
//!    任何「按活跃区间/按类型分组」的排法都会让偏移依赖别的分析结果，
//!    于是同一份 SSA 在两次编译里可能得到不同的帧。
//! 3. **零大小值不占空间但仍有偏移**：`struct{}` 的槽宽 0，`off` 与下一个槽的末尾重合。
//!    它的**地址**由分配器给（`zerobase`），不是由帧给 —— 见 `C5` §7.2。

use std::collections::BTreeMap;
use std::fmt;

use rgoc_hir::SlotId;
use rgoc_ssa::ir::{SsaFunc, Term, ValueId};
use rgoc_types::{TypeId, TypeTable};

use crate::abi::slot_of;

/// 一个槽在帧里的位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// 相对 `fp` 的偏移（**负数**）。
    pub off: i32,
    /// 槽宽（字节，8 的倍数；零大小类型为 0）。
    pub size: u32,
}

/// 一个函数的帧。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// 值 → 槽（`ValueId` 升序）。
    pub values: BTreeMap<ValueId, Slot>,
    /// **取过地址**的槽 → 槽（`slot_types` 里登记的那些）。
    pub addr_slots: BTreeMap<SlotId, Slot>,
    /// 并行拷贝临时区的起点（相对 `fp`，负数）；`None` ⇒ 该函数没有边实参。
    pub tmp_base: Option<i32>,
    /// 临时区有几个 8 字节格。
    pub tmp_slots: u32,
    /// `sub sp, sp, #(frame_size - 16)` 里的那个数（**16 的倍数**）。
    pub frame_size: u32,
}

impl Frame {
    /// 序言里 `sub sp, sp, #imm` 的立即数。
    ///
    /// `stp x29, x30, [sp, #-16]!` 已经动了 16，所以这里减掉它。
    #[must_use]
    pub fn sub_sp_imm(&self) -> u32 {
        self.frame_size.saturating_sub(16)
    }

    /// 某个值槽的偏移（测试与 `FRAME-MAP` 用）。
    #[must_use]
    pub fn off_of(&self, v: ValueId) -> Option<i32> {
        self.values.get(&v).map(|s| s.off)
    }

    /// 生成**取址槽**的零值初始化（`C5` §7.2）。
    ///
    /// # 为什么只清取址槽，不清值槽
    ///
    /// 值槽的每一次读都被 `V2`（定义支配使用）保证「前面写过」，所以清它是**多余的**
    /// —— 而且会白花指令。取址槽不同：`var a [3]int` 在源码里就没有写，`&a` 之后
    /// 通过指针读到的必须是**零值**。所以只有它们需要进帧时清零。
    ///
    /// # 为什么用 `add` + `str` 而不是 `str [x29, #负偏移]`
    ///
    /// AArch64 的 `STR`（无符号 scaled imm12）**偏移必须是非负的 8 的倍数**；
    /// 负偏移要用 `STUR`（unscaled，±256）。两条路都写，就多出一个「偏移超了怎么办」的分支。
    /// 统一走 `add x9, x29, #off`（GAS 会把负立即数编成 `sub`）+ `str [x9, #k]`（k ≥ 0），
    /// 一条路径、没有边界。
    pub fn zero_init(&self, asm: &mut crate::emit::Asm) {
        for s in self.addr_slots.values() {
            if s.size == 0 {
                // 零大小槽没有字节要清（它的地址由分配器给，见 `C5` §7.2）。
                continue;
            }
            asm.ins(&format!("add x9, x29, #{}", s.off));
            let mut k: u32 = 0;
            while k < s.size {
                asm.ins(&format!("str xzr, [x9, #{k}]"));
                k += 8;
            }
        }
    }
}

/// 布局失败：某个值的静态类型没有运行期布局。
///
/// 这不是「不支持面」的一张码（那些在 `CgCode` 里），而是**上游给了一个不该出现的类型**
/// （泛型 / `Invalid` / 元组）。带着 `ValueId` 报出来，才能定位到是谁。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// 某个值没有布局。
    Value {
        /// 值号。
        value: ValueId,
        /// 它的静态类型。
        typ: TypeId,
    },
    /// 某个取址槽没有布局。
    AddrSlot {
        /// 槽号。
        slot: SlotId,
        /// 它的静态类型。
        typ: TypeId,
    },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value { value, typ } => {
                write!(f, "v{} 的类型 t{} 没有运行期布局", value.0, typ.index())
            }
            Self::AddrSlot { slot, typ } => {
                write!(f, "槽 s{} 的类型 t{} 没有运行期布局", slot.0, typ.index())
            }
        }
    }
}

/// `saved fp` + `saved lr` 占的 16 字节。
const SAVED: u32 = 16;

/// 布局一个函数。
///
/// # Errors
///
/// 任一值或取址槽的类型没有运行期布局（泛型 / `Invalid` / 元组）。
pub fn layout(f: &SsaFunc, tt: &TypeTable) -> Result<Frame, FrameError> {
    let mut cur = SAVED;
    let mut values = BTreeMap::new();

    // ① 值槽：按 `ValueId` 升序（`value_types` 的下标就是 `ValueId`）。
    for (i, typ) in f.value_types.iter().enumerate() {
        let v = ValueId(u32::try_from(i).expect("值号超出 u32"));
        let s = slot_of(tt, *typ).ok_or(FrameError::Value {
            value: v,
            typ: *typ,
        })?;
        // 所有槽 8 字节对齐（`C5` §7.2）。零大小槽不推进游标。
        cur = round_up(cur, 8);
        cur = cur.checked_add(s.size).expect("帧大小溢出 u32");
        values.insert(
            v,
            Slot {
                off: neg(cur),
                size: s.size,
            },
        );
    }

    // ② 取址槽：`slot_types` 是 `BTreeMap`，天然按 `SlotId` 升序。
    let mut addr_slots = BTreeMap::new();
    for (slot, typ) in &f.slot_types {
        let s = slot_of(tt, *typ).ok_or(FrameError::AddrSlot {
            slot: *slot,
            typ: *typ,
        })?;
        cur = round_up(cur, 8);
        cur = cur.checked_add(s.size).expect("帧大小溢出 u32");
        addr_slots.insert(
            *slot,
            Slot {
                off: neg(cur),
                size: s.size,
            },
        );
    }

    // ③ 并行拷贝临时区：格数 = 所有边里最多的实参个数。
    let tmp_slots = max_edge_args(f);
    let tmp_base = if tmp_slots == 0 {
        None
    } else {
        cur = cur.checked_add(tmp_slots * 8).expect("帧大小溢出 u32");
        Some(neg(cur))
    };

    // ④ 帧大小向上取 16（`sp` 恒 16 对齐）。
    let frame_size = round_up(cur, 16).max(SAVED);

    Ok(Frame {
        values,
        addr_slots,
        tmp_base,
        tmp_slots,
        frame_size,
    })
}

/// 所有终止符里最多的「一条边给几个实参」。
fn max_edge_args(f: &SsaFunc) -> u32 {
    let mut n = 0usize;
    for b in &f.blocks {
        let here = match &b.term {
            Term::Goto { args, .. } => args.len(),
            Term::If { then_, else_, .. } => then_.1.len().max(else_.1.len()),
            Term::Return(_) | Term::Panic(_) | Term::Unreachable => 0,
        };
        n = n.max(here);
    }
    u32::try_from(n).expect("边实参个数超出 u32")
}

/// 向上取整到 `align` 的倍数。
fn round_up(n: u32, align: u32) -> u32 {
    debug_assert!(align.is_power_of_two());
    let mask = align - 1;
    n.saturating_add(mask) & !mask
}

/// `u32` 偏移 → 相对 `fp` 的负偏移。
fn neg(n: u32) -> i32 {
    i32::try_from(n).map_or(i32::MIN, |v| -v)
}

/// 渲染 `FRAME-MAP v1`（`C5` §8）。
///
/// # 四件事决定了这份文本的形状
///
/// 1. **行语法与排序键**：`slot` 按 `ValueId` 升序、`safepoint` 按 `(块号, 指令下标)` 升序 ——
///    后者 `rgoc_ssa::live` 已经排好，这里不重排（重排就等于有第二个排序口径）。
/// 2. **`offs` 是偏移列表**，不是值号列表：这份文本的消费者（M10 的栈扫描器）
///    要的是「扫哪几个字」，值号对它没用。
/// 3. **`ptrs` 是 `offs` 的子集**：只收指针类型的值（`live` 已经算好）。
/// 4. **空集写 `[]`，不省略字段** —— 消费方不必猜「缺了是空还是没有」（`C10` 同口径）。
///
/// `type=` 写 `t<id>`（与 `C10` 的类型引用同形）：一份帧图里不需要把类型的结构展开，
/// 要展开的场合有 `C7` 的 dump。
#[must_use]
pub fn render_frame_map(f: &SsaFunc, fr: &Frame, lv: &rgoc_ssa::live::Liveness) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str("# FRAME-MAP v1\n");
    let _ = writeln!(
        out,
        "# func {} frame={} slots={}",
        f.name,
        fr.frame_size,
        fr.values.len()
    );
    for (v, s) in &fr.values {
        let typ = f
            .value_types
            .get(v.index())
            .map_or_else(|| "-".to_string(), |t| format!("t{}", t.index()));
        let _ = writeln!(
            out,
            "# slot v{} off={} size={} type={}",
            v.0, s.off, s.size, typ
        );
    }
    for sp in &lv.safepoints {
        let offs = offsets_of(fr, &sp.live);
        let ptrs = offsets_of(fr, &sp.ptrs);
        let _ = writeln!(
            out,
            "# safepoint b{}:{} offs=[{}] ptrs=[{}]",
            sp.block.0,
            sp.idx,
            join(&offs),
            join(&ptrs)
        );
    }
    out
}

/// 一组值 → 它们的偏移（**数值升序**、去重）。
fn offsets_of(fr: &Frame, vals: &[ValueId]) -> Vec<i32> {
    let mut v: Vec<i32> = vals
        .iter()
        .filter_map(|x| fr.values.get(x).map(|s| s.off))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// 逗号连接（空集 ⇒ 空串，外层写 `[]`）。
fn join(v: &[i32]) -> String {
    v.iter().map(i32::to_string).collect::<Vec<_>>().join(",")
}
