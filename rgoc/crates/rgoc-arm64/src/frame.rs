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
use rgoc_ssa::ir::{CalleeRef, Instr, Root, SsaFunc, Term, ValueId};
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

/// 一个**逃逸**的取址槽：存储不在帧里，在 arena 里。
///
/// 帧里只留一个 8 字节的**指针格**（`cell`），指向 arena 里那块存储。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeapSlot {
    /// 指针格在帧里的偏移（负数）。
    pub cell: i32,
    /// 存储大小（字节，取自 `rgoc_types::layout`）。
    pub size: u32,
    /// 存储对齐。
    pub align: u32,
}

/// 一个函数的帧。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// 值 → 槽（`ValueId` 升序）。
    pub values: BTreeMap<ValueId, Slot>,
    /// **取过地址**的槽 → 槽（`slot_types` 里登记的那些）。
    ///
    /// 逃逸槽在这里的 `size` 是 **8**（指针格），存储本身见 [`Frame::heap`]。
    pub addr_slots: BTreeMap<SlotId, Slot>,
    /// 逃逸的取址槽（存储进 arena，`R-13-5`）。
    pub heap: BTreeMap<SlotId, HeapSlot>,
    /// 并行拷贝临时区的起点（相对 `fp`，负数）；`None` ⇒ 该函数没有边实参。
    pub tmp_base: Option<i32>,
    /// **调用方 outgoing 区**的字节数（在帧的最底部，`[sp, sp+n)`）。
    ///
    /// `G-M7-01`：AAPCS64 规定「寄存器装不下的实参由**调用方**在 `sp` 处预留」。
    /// 这块必须算进帧大小 —— 不算就等于往自己的槽上写。
    pub outgoing: u32,
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
        for (slot, s) in &self.addr_slots {
            if s.size == 0 {
                // 零大小槽没有字节要清（它的地址由分配器给，见 `C5` §7.2）。
                continue;
            }
            // 逃逸槽的帧里那一格是**指针**，arena 那块存储由 `rt_arena_alloc`
            // 建零值。清它会把刚存进去的指针抹掉 ⇒ `addr-of` 拿到 0。
            if self.heap.contains_key(slot) {
                continue;
            }
            // `add` 的立即数只有 12 位：帧 > 4 KiB 时 `#-4232` 汇编不过
            // （`bigmap.go` 的 `[1000]byte` 槽就是这条）。大偏移走 `x10`。
            if (-4095..=4095).contains(&s.off) {
                asm.ins(&format!("add x9, x29, #{}", s.off));
            } else {
                asm.ins(&format!("movz x10, #{}", s.off.unsigned_abs() & 0xFFFF));
                let hi = (s.off.unsigned_abs() >> 16) & 0xFFFF;
                if hi != 0 {
                    asm.ins(&format!("movk x10, #{hi}, lsl #16"));
                }
                if s.off < 0 {
                    asm.ins("sub x9, x29, x10");
                } else {
                    asm.ins("add x9, x29, x10");
                }
            }
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
    layout_with(f, tt, &BTreeMap::new())
}

/// 同 [`layout`]，但能按 `FuncId` 查到被调方的形参类型。
///
/// 为什么要它：outgoing 区的大小由**被调方的形参表**决定（`R-13-3`），
/// 只拿到实参类型算不准 —— `untyped` 常量与形参的默认类型可能不同（`complex128`
/// vs `complex64` 会差一个寄存器，进而改变「第几个实参开始走栈」）。
///
/// # Errors
///
/// 同 [`layout`]。
pub fn layout_with(
    f: &SsaFunc,
    tt: &TypeTable,
    siblings: &BTreeMap<rgoc_hir::FuncId, &SsaFunc>,
) -> Result<Frame, FrameError> {
    let outgoing = max_outgoing(f, tt, siblings);
    // `cur` 是**槽区**的游标（从 0 起，不含保存区）。
    // 槽的偏移一律是 `-(SAVED + cur)`：保存区占 `[fp-16, fp)`，槽区在它下面。
    //
    // 从 `SAVED` 起算再 `sub = round_up(cur,16)` 会让空函数也 `sub 16`（白占 16 字节），
    // 而且「`sub` 要 ≥ 最低槽」这条约束会写得不直观。
    let mut cur: u32 = 0;
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
                off: neg(SAVED + cur),
                size: s.size,
            },
        );
    }

    // ② 取址槽：`slot_types` 是 `BTreeMap`，天然按 `SlotId` 升序。
    //
    // 逃逸的那些（`R-13-5`）不占帧存储，只占一个 8 字节指针格，存储在 arena。
    let esc = escaping_slots(f);
    let mut addr_slots = BTreeMap::new();
    let mut heap = BTreeMap::new();
    for (slot, typ) in &f.slot_types {
        let s = slot_of(tt, *typ).ok_or(FrameError::AddrSlot {
            slot: *slot,
            typ: *typ,
        })?;
        let is_esc = esc.contains(slot);
        let width = if is_esc { 8 } else { s.size };
        cur = round_up(cur, 8);
        cur = cur.checked_add(width).expect("帧大小溢出 u32");
        addr_slots.insert(
            *slot,
            Slot {
                off: neg(SAVED + cur),
                size: width,
            },
        );
        if is_esc {
            let l = rgoc_types::layout::layout(tt, *typ);
            heap.insert(
                *slot,
                HeapSlot {
                    cell: neg(SAVED + cur),
                    size: s.size,
                    align: l.map_or(8, |l| l.align.max(1)),
                },
            );
        }
    }

    // ③ 并行拷贝临时区：格数 = 所有边里最多的实参个数。
    let tmp_slots = max_edge_args(f);
    let tmp_base = if tmp_slots == 0 {
        None
    } else {
        cur = cur.checked_add(tmp_slots * 8).expect("帧大小溢出 u32");
        Some(neg(SAVED + cur))
    };

    // ③.5 调用方 outgoing 区：放帧的**最底部** `[sp, sp+outgoing)`。
    // 只在最底部才成立 —— `pass_args` 写的是 `[sp, #k]`，而 `sp = fp - sub`，
    // 所以「帧底」必须是 outgoing 区，不能是值槽。
    cur = cur.checked_add(outgoing).expect("帧大小溢出 u32");

    // ④ 帧大小向上取 16（`sp` 恒 16 对齐）。
    //
    // ⚠️ `sub sp` 的量必须**覆盖到最低的槽**，不是「总大小减去 saved」。
    //
    // 槽 `off = -(SAVED + cur)` 是该槽**最低**的地址，所以全部槽的最低点是
    // `-(SAVED + cur)`。`sp = fp - sub` 必须 ≤ 那个点 ⇒ `sub ≥ cur`。
    //
    // 旧写法 `sub = round_up(cur,16) - 16` 会让 `sp` 比最低槽**高 16 字节**：
    // 被调方的 `stp x29,x30,[sp,#-16]!` 正好覆盖那一段，于是「最后一次写入的槽」
    // 在调用后变成垃圾。实测 `probe5.go`：`show(complex, bool)` 的 `bool` 槽在
    // `fp-40`、`sp` 在 `fp-32`，`rt_print_complex` 一返回 `bool` 就变成 `true`。
    //
    // 这个错在「函数里没有调用」或「最后一个槽恰好没人再读」时完全看不出来 ——
    // 46 个语料里有 32 个照样绿（`R-13-4`）。
    // 槽区从 `fp-16` 往下长 `cur`，所以最低点是 `fp-16-cur` ⇒
    // `sp = fp - sub` 必须 ≤ 它 ⇒ `sub ≥ SAVED + cur`。
    // 没有槽时不必 `sub`（空函数就是 16 字节，一条指令都不多）。
    let sub = if cur == 0 {
        0
    } else {
        round_up(SAVED + cur, 16)
    };
    let frame_size = SAVED + sub;

    Ok(Frame {
        values,
        addr_slots,
        heap,
        outgoing,
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
    // 末尾的 `sym=` 是 `R-13-8`：帧图要能对到**二进制里的符号**。
    // `# func` 那一位是 HIR 名字（给人看），而 `.s` 里的符号是 `fn<FuncId>`
    // （`isel::fn_sym`）—— 只有名字的话，消费者在反汇编里找不到对应的函数。
    // 追加在**行尾**而不是插在中间：按字段名解析的消费者不受影响，位置型的断言也不变。
    let _ = writeln!(
        out,
        "# func {} frame={} slots={} sym=fn{}",
        f.name,
        fr.frame_size,
        fr.values.len(),
        f.src.0
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

/// 哪些取址槽的地址会**活着离开这个函数**（`R-13-5`）。
///
/// # 为什么必须有这一步
///
/// 帧存储的生命周期是「一次调用」。把它的地址返回出去（`return &T{1}`）、
/// 存进全局或堆（`g = &x`）、或者传给别的函数，下一个调用就会复用同一块内存 ——
/// Go 要求每次调用得到**不同**的地址。实测 `compos.go`：
/// `f()` 两次返回 `&T{1}`，`x == y` 为真，程序自检 panic。
///
/// # 为什么是「保守判定」
///
/// 判错的代价不对称：
/// - **漏判**（该逃逸却留在帧里）⇒ 静默的别名错，最难查；
/// - **多判**（不该逃逸却进了 arena）⇒ 多分配几个字节，最坏是 `panic: out of arena`。
///
/// 所以只在「明确不逃逸」时才留在帧里。`print`/`println` 这类内建不收指针，
/// 不算逃逸位置。
fn escaping_slots(f: &SsaFunc) -> std::collections::BTreeSet<SlotId> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut origin: BTreeMap<ValueId, SlotId> = BTreeMap::new();
    let mut out: BTreeSet<SlotId> = BTreeSet::new();

    for b in &f.blocks {
        for i in &b.instrs {
            if let Instr::AddrOf { dst, root } = i {
                // `root` 是 `RootId`（函数内下标），要过 `f.roots` 才是根本身。
                if let Root::Slot(s) = f.roots[root.index()] {
                    origin.insert(*dst, s);
                }
            }
        }
    }
    // 指针的别名（`copy` / `convert` / 取字段 / 取下标）都继承同一个槽。
    // 两遍足够：SSA 里值号是定义序，跨块前向引用不存在。
    for _ in 0..2 {
        for b in &f.blocks {
            for i in &b.instrs {
                let (dst, src) = match i {
                    Instr::Copy { dst, src } => (*dst, *src),
                    Instr::Convert { dst, x, .. } => (*dst, *x),
                    Instr::Field { dst, base, .. } => (*dst, *base),
                    Instr::Index { dst, base, .. } => (*dst, *base),
                    _ => continue,
                };
                if let Some(s) = origin.get(&src).copied() {
                    origin.insert(dst, s);
                }
            }
        }
    }
    let mark = |v: &ValueId, out: &mut BTreeSet<SlotId>| {
        if let Some(s) = origin.get(v) {
            out.insert(*s);
        }
    };
    for b in &f.blocks {
        for i in &b.instrs {
            match i {
                // 传给别的函数（被调方可能留下来）。内建不算。
                Instr::Call {
                    callee: CalleeRef::Func(_) | CalleeRef::Method { .. },
                    args,
                    ..
                } => {
                    for a in args {
                        mark(a, &mut out);
                    }
                }
                // 写进全局 / 堆 ⇒ 地址活得比这一帧长。
                Instr::Store {
                    root, addr, val, ..
                } if !matches!(f.roots[root.index()], Root::Slot(_)) => {
                    mark(addr, &mut out);
                    mark(val, &mut out);
                }
                _ => {}
            }
        }
        if let Term::Return(vals) = &b.term {
            for v in vals {
                mark(v, &mut out);
            }
        }
    }
    out
}

/// 调用点的**形参类型表**：有被调方就用它的声明类型，查不到才退回实参类型。
///
/// 调用方（`isel::pass_args`）与被调方（`isel::bind_params`）必须得到**同一张表**，
/// 否则寄存器/栈的分配会错位。所以两边都走这个函数，不许各写一遍。
#[must_use]
pub fn callee_param_tys(
    arg_tys: &[TypeId],
    callee: Option<rgoc_hir::FuncId>,
    siblings: &BTreeMap<rgoc_hir::FuncId, &SsaFunc>,
) -> Vec<TypeId> {
    let Some(c) = callee.and_then(|id| siblings.get(&id)) else {
        return arg_tys.to_vec();
    };
    (0..arg_tys.len())
        .map(|i| {
            c.params
                .get(i)
                .and_then(|p| c.value_types.get(p.index()).copied())
                .unwrap_or(arg_tys[i])
        })
        .collect()
}

/// 全函数所有调用点里，**最大的 outgoing 区字节数**。
fn max_outgoing(
    f: &SsaFunc,
    tt: &TypeTable,
    siblings: &BTreeMap<rgoc_hir::FuncId, &SsaFunc>,
) -> u32 {
    let mut max = 0u32;
    for b in &f.blocks {
        for i in &b.instrs {
            let (callee, args) = match i {
                Instr::Call { callee, args, .. } => (callee, args),
                _ => continue,
            };
            let arg_tys: Vec<TypeId> = args
                .iter()
                .map(|a| {
                    f.value_types
                        .get(a.index())
                        .copied()
                        .unwrap_or_else(|| TypeId::from_index(0))
                })
                .collect();
            let fid = match callee {
                // 接口派发：被调方运行期才定 ⇒ 没有静态签名可查（出参槽按 0 算，
                // 而 `call` 自己会按接口签名写结果，见 `isel::call`）
                CalleeRef::Iface { .. } => None,
                CalleeRef::Func(id) => Some(*id),
                CalleeRef::Method { func, .. } => Some(*func),
                // 内建走 runtime 的固定签名，没有栈实参。
                // 运行期助手没有 SSA 函数体（它们是 `rt.c` 里的 C 函数）。
                CalleeRef::Builtin(_) | CalleeRef::Indirect(_) | CalleeRef::Runtime(_) => None,
            };
            let want = callee_param_tys(&arg_tys, fid, siblings);
            let (_, sp) = crate::abi::place_args(tt, &want);
            max = max.max(sp);
        }
    }
    max
}
