//! 指令选择：SSA → arm64 文本汇编（`C5` §6/§7；`M7-design.md` §3.4）。
//!
//! # 寄存器分工（`C5` §6.1）
//!
//! | 寄存器 | 用途 |
//! |---|---|
//! | `x0..x7` / `v0..v7` | 调用实参与返回值 |
//! | `x8` | 间接结果（聚合返回值缓冲地址） |
//! | `x9` | **地址临时**：每条访存前 `add x9, x29, #off` |
//! | `x10..x15` / `d10..d15` | 指令内临时（活不过一条指令） |
//! | `x29` / `x30` | `fp` / `lr` |
//!
//! **不碰 `x19..x28`**：它们是 callee-saved，而生成代码不需要跨指令保活任何值
//! （值全在栈槽里，`D-M7-3`）—— 于是序言里没有 save/restore，帧里只有 `fp`/`lr`。
//!
//! # 槽里的值一律「符号/零扩展到 64 位」
//!
//! `int8` 的槽也占 8 字节，里面存的是**符号扩展后的** 64 位值。于是：
//!
//! - 算术做完要按结果类型**掩码/扩展**一次（`mask_to`），否则 `int8(127)+1` 会得 128；
//! - 读内存时按宽度取（`ldrb`/`ldrh`/`ldr`）并**再次扩展**。
//!
//! 这条约定让「槽宽恒定 8」与「宽度语义」同时成立。

use rgoc_hir::types::ConstVal;
use rgoc_hir::{BinOp, BuiltinKind, UnOp};
use rgoc_ssa::ir::{Block, CalleeRef, Instr, RuntimeFn, SsaFunc, Term, ValueId};
use rgoc_types::{BasicId, TypeId, TypeTable};

use crate::abi::{ArgLoc, SlotClass, place_args, slot_of};
use crate::diag::CgCode;
use crate::emit::Asm;
use crate::frame::{Frame, Slot, callee_param_tys};
use crate::parallel_copy;

/// 地址临时寄存器。
const ADDR: &str = "x9";

/// 一次指令选择需要的上下文。
#[derive(Debug)]
pub struct Ctx<'a> {
    /// 被编译的函数。
    pub f: &'a SsaFunc,
    /// 它的帧。
    pub fr: &'a Frame,
    /// 包级类型表。
    pub tt: &'a TypeTable,
    /// `FuncId → 该函数`（调用点要按**被调方**的形参类型决定寄存器，`R-13-3`）。
    pub siblings: &'a std::collections::BTreeMap<rgoc_hir::FuncId, &'a SsaFunc>,
}

impl<'a> Ctx<'a> {
    /// 值 → 静态类型。
    fn ty(&self, v: ValueId) -> TypeId {
        self.f.value_types[v.index()]
    }

    /// 值 → 槽类别。
    fn class(&self, v: ValueId) -> SlotClass {
        slot_of(self.tt, self.ty(v)).map_or(SlotClass::Int, |s| s.class)
    }

    /// 值 → 槽。
    fn slot(&self, v: ValueId) -> Slot {
        self.fr.values[&v]
    }

    /// 类型是不是 `float32`（决定用 `s` 还是 `d` 寄存器）。
    fn is_f32(&self, t: TypeId) -> bool {
        basic_of(self.tt, t) == Some(BasicId::Float32)
    }

    /// 类型的位宽（字节）；非整数返回 8。
    fn width(&self, t: TypeId) -> u32 {
        match basic_of(self.tt, t) {
            Some(BasicId::Bool) | Some(BasicId::Int8) | Some(BasicId::Uint8) => 1,
            Some(BasicId::Int16) | Some(BasicId::Uint16) => 2,
            Some(BasicId::Int32) | Some(BasicId::Uint32) | Some(BasicId::Float32) => 4,
            _ => 8,
        }
    }

    /// 类型是不是**无符号**（决定 `sdiv` 还是 `udiv`、`cset` 的条件）。
    fn is_unsigned(&self, t: TypeId) -> bool {
        matches!(
            basic_of(self.tt, t),
            Some(BasicId::Uint8)
                | Some(BasicId::Uint16)
                | Some(BasicId::Uint32)
                | Some(BasicId::Uint64)
                | Some(BasicId::Uint)
                | Some(BasicId::Uintptr)
        )
    }
}

/// 剥命名/别名后的 `BasicId`（只认基本类型）。
fn basic_of(tt: &TypeTable, t: TypeId) -> Option<BasicId> {
    use rgoc_types::typ::Type;
    let mut cur = t;
    for _ in 0..64 {
        match tt.type_of(cur) {
            Type::Basic(b) => return Some(*b),
            Type::Named(id) => cur = tt.named_underlying(*id),
            Type::Alias(id) => cur = tt.alias_rhs(*id),
            _ => return None,
        }
    }
    None
}

// ── 访存小工具 ────────────────────────────────────────────────────
//
// 所有槽偏移都是**相对 `fp` 的负数**，而 `LDR/STR` 的 scaled imm12 要求偏移非负
// （`R-10-1` 的实测）。所以统一 `add x9, x29, #off` 再 `[x9]`：一条路径、没有边界。

/// `add x9, x29, #off`
fn addr(asm: &mut Asm, off: i32) {
    // AArch64 的 `add` 立即数只有 12 位（±4095）。帧大于 4 KiB 时
    // （`var a [1024]int` 就 8 KiB）直接发 `add x9, x29, #-8048` 汇编不过 ——
    // 这是 M7 的既有局限，M9 的切片/数组语料一撞就现形（`ken/convert.go`）。
    //
    // 大偏移走「先把偏移装进 x9，再 add x9, x29, x9」：多两条指令，
    // 但把「帧不能超过 4 KiB」这条**隐形上限**彻底去掉。
    if (-4095..=4095).contains(&off) {
        asm.ins(&format!("add {ADDR}, x29, #{off}"));
    } else {
        load_imm(asm, ADDR, i64::from(off));
        asm.ins(&format!("add {ADDR}, x29, {ADDR}"));
    }
}

/// 读一个字到 `x` 寄存器。
fn ld_x(asm: &mut Asm, reg: &str, off: i32) {
    addr(asm, off);
    asm.ins(&format!("ldr {reg}, [{ADDR}]"));
}

/// 写一个字。
fn st_x(asm: &mut Asm, reg: &str, off: i32) {
    addr(asm, off);
    asm.ins(&format!("str {reg}, [{ADDR}]"));
}

/// 读一个 `d`（64 位浮点）。
fn ld_d(asm: &mut Asm, reg: &str, off: i32) {
    addr(asm, off);
    asm.ins(&format!("ldr {reg}, [{ADDR}]"));
}

/// 写一个 `d`。
fn st_d(asm: &mut Asm, reg: &str, off: i32) {
    addr(asm, off);
    asm.ins(&format!("str {reg}, [{ADDR}]"));
}

/// 读一个 `s`（32 位浮点）。
fn ld_s(asm: &mut Asm, reg: &str, off: i32) {
    addr(asm, off);
    asm.ins(&format!("ldr {reg}, [{ADDR}]"));
}

/// 写一个 `s`。
fn st_s(asm: &mut Asm, reg: &str, off: i32) {
    addr(asm, off);
    asm.ins(&format!("str {reg}, [{ADDR}]"));
}

/// 把一个 64 位立即数装进寄存器（`movz` + 最多三条 `movk`）。
pub(crate) fn load_imm(asm: &mut Asm, reg: &str, v: i64) {
    let u = v as u64;
    let chunks = [
        (u & 0xffff) as u16,
        ((u >> 16) & 0xffff) as u16,
        ((u >> 32) & 0xffff) as u16,
        ((u >> 48) & 0xffff) as u16,
    ];
    // 第一条必须用 `movz`（其余位清零），后面非零的用 `movk`。
    asm.ins(&format!("movz {reg}, #{}", chunks[0]));
    for (i, c) in chunks.iter().enumerate().skip(1) {
        if *c != 0 {
            asm.ins(&format!("movk {reg}, #{c}, lsl #{}", i * 16));
        }
    }
}

/// 按类型把寄存器里的值**规整到 64 位**（`int8` 的 `127+1` 必须回绕成 `-128`）。
fn mask_to(asm: &mut Asm, reg: &str, tt: &TypeTable, t: TypeId) {
    let Some(b) = basic_of(tt, t) else { return };
    let w = reg.replace('x', "w");
    match b {
        BasicId::Int8 => asm.ins(&format!("sxtb {reg}, {w}")),
        BasicId::Int16 => asm.ins(&format!("sxth {reg}, {w}")),
        BasicId::Int32 => asm.ins(&format!("sxtw {reg}, {w}")),
        BasicId::Uint8 => asm.ins(&format!("uxtb {reg}, {w}")),
        BasicId::Uint16 => asm.ins(&format!("uxth {reg}, {w}")),
        // `uint32` / `uint`：写 `w` 寄存器天然清零高 32 位（`mov w, w`）。
        BasicId::Uint32 => asm.ins(&format!("mov {w}, {w}")),
        _ => {}
    }
}

/// 编译一个函数（含序言、块体、尾声）。
///
/// # Errors
///
/// 遇到 M7 明确不支持的面（`[]T` / 间接调用 / 方法值）。
pub fn func(
    f: &SsaFunc,
    fr: &Frame,
    tt: &TypeTable,
    siblings: &std::collections::BTreeMap<rgoc_hir::FuncId, &SsaFunc>,
    asm: &mut Asm,
) -> Result<(), CgCode> {
    // M9/Task 11 起切片真的被选择（`make`/`Index`/`Slice`/`append`），
    // 所以 M7 的「整个函数一碰到切片就拒」（`U-M7-01`）在这里下线了。
    let cx = Ctx {
        f,
        fr,
        tt,
        siblings,
    };
    // ⚠️ 符号名用 `fn<HIR FuncId>`，**不用** `f.name`：
    // ① 名字会撞（两个类型各有一个 `M` 方法）；
    // ② 调用点只能看到 `FuncId`（`CalleeRef::Func`），看不到名字 ——
    //    两边用不同来源命名，链接期就是一堆 `undefined reference`。
    let sym = fn_sym(f.src);
    asm.global(&sym);
    asm.label(&sym);
    prologue(fr, asm);

    // **隐式第 0 实参（ctx）**（`M9/Task 40`）：`CapturePtr` 会在任意位置读它，
    // 所以必须先落进帧里保活。⚠️ 必须在 `bind_params` 之前 —— 聚合形参分支
    // 会把 `x0` 当临时用（`mov x0, x9`）。
    if let Some(off) = fr.ctx_off {
        st_x(asm, "x0", off);
    }

    // 形参：入口块的 `params` 从寄存器落到槽里。
    bind_params(&cx, asm);

    // 逃逸的取址槽：存储从 arena 拿，指针存进帧里的指针格。
    //
    // **必须在 `bind_params` 之后**：`rt_arena_alloc` 是普通 C 函数，
    // 会按 AAPCS64 覆盖 `x0..x3`（也就是形参）。放在前面等于把形参丢了。
    for h in fr.heap.values() {
        load_imm(asm, "x0", i64::from(h.size));
        load_imm(asm, "x1", i64::from(h.align));
        asm.ins("bl rt_arena_alloc");
        st_x(asm, "x0", h.cell);
    }

    for (bi, blk) in f.blocks.iter().enumerate() {
        // 入口块没有前驱 ⇒ 直接落下；其余块要有标签（可能被多处跳转）。
        if bi != f.entry.index() {
            asm.label(&block_label(&sym, bi));
        }
        for ins in &blk.instrs {
            instr(&cx, ins, asm)?;
        }
        term(&cx, blk, asm)?;
    }
    Ok(())
}

/// 序言：`stp` 保存 `fp`/`lr`，`mov fp, sp`，`sub sp`，取址槽清零。
fn prologue(fr: &Frame, asm: &mut Asm) {
    asm.ins("stp x29, x30, [sp, #-16]!");
    asm.ins("mov x29, sp");
    let imm = fr.sub_sp_imm();
    if imm > 0 {
        // ⚠️ `add/sub` 的立即数是 12 位（0..4095）。大帧必须走寄存器 ——
        // 实测：`sub sp, sp, #9936` 会被 as 拒绝（`ken/convert.go` 的帧就这么大）。
        if imm <= 4095 {
            asm.ins(&format!("sub sp, sp, #{imm}"));
        } else {
            load_imm(asm, "x10", i64::from(imm));
            asm.ins("sub sp, sp, x10");
        }
    }
    fr.zero_init(asm);
}

/// 尾声：`add sp`、`ldp` 恢复、`ret`。
fn epilogue(fr: &Frame, asm: &mut Asm) {
    let imm = fr.sub_sp_imm();
    if imm > 0 {
        if imm <= 4095 {
            asm.ins(&format!("add sp, sp, #{imm}"));
        } else {
            load_imm(asm, "x10", i64::from(imm));
            asm.ins("add sp, sp, x10");
        }
    }
    asm.ins("ldp x29, x30, [sp], #16");
    asm.ins("ret");
}

/// 函数的汇编符号名。
pub(crate) fn fn_sym(id: rgoc_hir::FuncId) -> String {
    format!("fn{}", id.0)
}

/// 块标签（`<sym>.b<k>`）。
fn block_label(sym: &str, bi: usize) -> String {
    format!("{sym}.b{bi}")
}

/// 形参落槽（`C5` §6.1；分配表由 `abi::place_args` 给，与调用方**同一张**）。
///
/// # 栈上的形参从哪读
///
/// 入口处 `stp x29, x30, [sp, #-16]!` + `mov x29, sp` ⇒ `[fp]` 是调用方的 `fp`、
/// `[fp+8]` 是返回地址、`[fp+16]` 起是**调用方写的栈实参**。所以第 `off` 字节在
/// `[x29, #(16 + off)]`。
///
/// # 聚合形参要**拷贝**
///
/// 我们的 ABI 把聚合按指针传（`C5` §6.1），但 Go 的语义是**按值**：
/// 被调方改了形参不能影响调用方。所以入口要把指针指向的内容拷进自己的槽 ——
/// 直接把指针存进槽是错的（后续的 `field`/`load` 会把指针字节当字段值）。
fn bind_params(cx: &Ctx<'_>, asm: &mut Asm) {
    let want: Vec<TypeId> = cx.f.params.iter().map(|p| cx.ty(*p)).collect();
    // `ctx_abi` 的函数多一个**隐式第 0 实参**（ctx，占 `x0`）⇒ 形参整体后移一格
    // （`M9/Task 40`）。浮点不受影响（ctx 是整数类）。
    let (locs, _) = if cx.f.ctx_abi {
        let mut w = vec![TypeId::from_index(0)];
        w.extend(want.iter().copied());
        place_args(cx.tt, &w)
    } else {
        place_args(cx.tt, &want)
    };
    let shift = usize::from(cx.f.ctx_abi);
    for (i, p) in cx.f.params.iter().enumerate() {
        let s = cx.slot(*p);
        let class = cx.class(*p);
        let loc = locs
            .get(i + shift)
            .copied()
            .unwrap_or(ArgLoc::X { reg: 0, n: 1 });
        match loc {
            ArgLoc::X { reg, n } => match class {
                // 聚合：从寄存器里拿到**指针**，把内容拷进本帧的槽（值语义）。
                //
                // ⚠️ 顺序不能反：源指针可能在 `x0`（第 0 个形参就是聚合时），
                // 而 `addr` 之后 `mov x0, x9` 会把 `x0` 覆盖掉。先取源、再设目的。
                //
                // 反了会变成 `memcpy(dst, dst, size)` —— 拷的是本帧未初始化的槽。
                // 症状：`len(s)`/`cap(s)` 是垃圾（切片形参），或者一索引就段错误
                // （垃圾 ptr）。而 `string` 形参**不**受影响（它是 `Int` 类、走
                // 两个字），所以 M7 的语料全绿也没抓到。
                SlotClass::Agg => {
                    addr(asm, s.off);
                    asm.ins(&format!("mov x1, x{reg}"));
                    asm.ins("mov x0, x9");
                    load_imm(asm, "x2", i64::from(s.size));
                    asm.ins("bl rt_memcpy");
                }
                _ => {
                    st_x(asm, &format!("x{reg}"), s.off);
                    if n > 1 {
                        st_x(asm, &format!("x{}", reg + 1), s.off + 8);
                    }
                }
            },
            ArgLoc::V { reg, n: _ } => match class {
                // `complex64` 是**两个 `f32` 的 HFA** ⇒ 两个 `s`（`R-13-2`）。
                SlotClass::Complex64 => {
                    st_s(asm, &format!("s{reg}"), s.off);
                    st_s(asm, &format!("s{}", reg + 1), s.off + 4);
                }
                SlotClass::Float => {
                    if cx.is_f32(cx.ty(*p)) {
                        st_s(asm, &format!("s{reg}"), s.off);
                    } else {
                        st_d(asm, &format!("d{reg}"), s.off);
                    }
                }
                SlotClass::Complex128 => {
                    st_d(asm, &format!("d{reg}"), s.off);
                    st_d(asm, &format!("d{}", reg + 1), s.off + 8);
                }
                // `place_args` 只会给浮点/复数分 `v`。
                SlotClass::Int | SlotClass::Agg => {}
            },
            ArgLoc::Stack { off } => {
                let base = 16 + off;
                match class {
                    SlotClass::Agg => {
                        // 栈上那一格是指针；拷内容进槽。
                        asm.ins(&format!("ldr x1, [x29, #{base}]"));
                        addr(asm, s.off);
                        asm.ins("mov x0, x9");
                        load_imm(asm, "x2", i64::from(s.size));
                        asm.ins("bl rt_memcpy");
                    }
                    _ => {
                        // 按「字」搬：`string` 两个、`complex128` 两个、其余一个。
                        let words = if s.size > 8 { 2 } else { 1 };
                        for k in 0..words {
                            let at = base + k * 8;
                            asm.ins(&format!("ldr x10, [x29, #{at}]"));
                            st_x(asm, "x10", s.off + (k as i32) * 8);
                        }
                    }
                }
            }
        }
    }
}

/// 一条指令。
fn instr(cx: &Ctx<'_>, ins: &Instr, asm: &mut Asm) -> Result<(), CgCode> {
    match ins {
        Instr::Const { dst, c, typ } => konst(cx, *dst, c, *typ, asm),
        Instr::Copy { dst, src } => copy(cx, *dst, *src, asm),
        Instr::Bin {
            dst,
            op,
            lhs,
            rhs,
            typ,
        } => bin(cx, *dst, *op, *lhs, *rhs, *typ, asm),
        Instr::Un { dst, op, x, typ } => un(cx, *dst, *op, *x, *typ, asm),
        Instr::Convert { dst, x, typ } => convert(cx, *dst, *x, *typ, asm),
        // 函数值（`M9/Task 37`）：没有捕获 ⇒ 就是一个符号地址。
        Instr::FuncAddr { dst, func } => {
            load_addr(asm, "x0", &crate::module::fv_symbol(*func));
            st_x(asm, "x0", cx.slot(*dst).off);
        }
        // 捕获指针（`M9/Task 38`）：`dst = [ctx + 8 + 8*i]`（funcval 的第 i 个
        // 捕获槽，首字是 `fn`）。
        Instr::CapturePtr { dst, index } => {
            let d = cx.slot(*dst);
            let Some(off) = cx.fr.ctx_off else {
                unreachable!("CapturePtr 出现在没有 ctx 格的帧里（构建 bug）")
            };
            ld_x(asm, "x10", off);
            asm.ins(&format!("ldr x10, [x10, #{}]", 8 + 8 * index));
            st_x(asm, "x10", d.off);
        }
        // 造闭包（`M9/Task 38`）：`rt_arena_alloc(8 + 8n, 8)`，填 fn 与各捕获。
        //
        // ⚠️ 寄存器纪律：`st_x`/`ld_x` 拿 `x9` 当**地址**暂存（`ADDR`），所以
        // funcval 的基地址放 `x10`，数据放 `x11` —— 用 `x9` 装数据会被下一次
        // 访存指令自己打掉（`R-9-12` 记过 `st_x(x9, ..)` 的同款坑）。
        Instr::MakeClosure {
            dst,
            func,
            captures,
        } => {
            let d = cx.slot(*dst);
            let total = 8 + 8 * captures.len();
            load_imm(asm, "x0", i64::try_from(total).unwrap_or(i64::MAX));
            load_imm(asm, "x1", 8);
            asm.ins("bl rt_arena_alloc");
            st_x(asm, "x0", d.off);
            ld_x(asm, "x10", d.off);
            load_addr(asm, "x11", &fn_sym(*func));
            asm.ins("str x11, [x10]");
            for (i, c) in captures.iter().enumerate() {
                let s = cx.slot(*c);
                ld_x(asm, "x11", s.off);
                let off = 8 + 8 * i;
                if off <= 32760 {
                    asm.ins(&format!("str x11, [x10, #{off}]"));
                } else {
                    load_imm(asm, "x9", i64::try_from(off).unwrap_or(i64::MAX));
                    asm.ins("str x11, [x10, x9]");
                }
            }
        }
        Instr::TypeAssert { dst, ok, x, typ } => type_assert(cx, *dst, *ok, *x, *typ, asm),
        Instr::AddrOf { dst, root } => addr_of(cx, *dst, root, asm),
        Instr::Field { dst, base, index } => field(cx, *dst, *base, *index, asm),
        Instr::Index { dst, base, i } => index(cx, *dst, *base, *i, asm),
        // `new(T)` 也是 `Make`，但它的 `typ` 是**指针**（`build.rs` 的约定）——
        // 那不是切片，走 arena 分配（`D-M7-7`）。只有 `make([]T,…)` 才是 `[]T`。
        // `make(map[K]V)`（M9/Task 19）：单字指针，指向 runtime 的 `MapObj`。
        Instr::Make { dst, typ, .. } if map_kv(cx.tt, *typ).is_some() => {
            map_make(cx, *dst, *typ, asm);
        }
        Instr::Make {
            dst,
            typ,
            len: None,
            cap: None,
        } if is_pointer_ty(cx.tt, *typ) => new_alloc(cx, *dst, *typ, asm),
        // `make([]T, len[, cap])`（M9/Task 11）。
        Instr::Make { dst, typ, len, cap } => makeslice(cx, *dst, *typ, *len, *cap, asm),
        // `s[lo:hi:max]`（M9/Task 13）。
        Instr::Slice {
            dst,
            base,
            lo,
            hi,
            max,
        } => slice_expr(cx, *dst, *base, *lo, *hi, *max, asm),
        Instr::Composite { dst, typ, items } => composite(cx, *dst, *typ, items, asm),
        Instr::Load {
            dst, addr: a, typ, ..
        } => load(cx, *dst, *a, *typ, asm),
        Instr::Store { addr: a, val, .. } => store(cx, *a, *val, asm),
        Instr::Call {
            dsts,
            callee,
            args,
            dots,
            ..
        } => call(cx, dsts, callee, args, *dots, asm)?,
    }
    Ok(())
}

/// `const`：整数装立即数、浮点/复数走 `.rodata`、字符串写 `(ptr, len)`。
fn konst(cx: &Ctx<'_>, dst: ValueId, c: &ConstVal, typ: TypeId, asm: &mut Asm) {
    let s = cx.slot(dst);
    // 聚合槽先整体清零（M9/Task 13 抓到的既有 bug）。
    //
    // `build.rs` 用 `Const Int(0)` 表示**任意类型**的零值（`R-20-1`）——
    // 包括 `struct{X,Y,Z int}` 这种 24 字节的聚合。而下面各分支只写一个字：
    // 聚合的其余字节留着槽里的旧内容，于是「零值」变成了垃圾。
    //
    // 症状很隐蔽：`[]T1{{X: f(), Z: f()}}` 里 Y 的零值来自这条常量，
    // 而它只被 memcpy 走 —— 打印出来是 1（上一条指令留在槽里的值）。
    if s.size > 8 {
        // 用 `rt_memset` 而不是逐字 `str xzr, [x9, #k*8]`：后者的立即数上限是
        // 4095，`[8192]int` 这类聚合会汇编不过（`ken/convert.go` 就是这条）。
        addr(asm, s.off);
        asm.ins("mov x0, x9");
        asm.ins("mov x1, xzr");
        load_imm(asm, "x2", i64::from(s.size));
        asm.ins("bl rt_memset");
    }
    match c {
        ConstVal::Bool(b) => {
            load_imm(asm, "x10", i64::from(*b));
            st_x(asm, "x10", s.off);
        }
        // ⚠️ **整数值的常量也可能是浮点类型**：`if s1 != 35` 里的 `35` 在 SSA 里
        // 就是 `const 35 : float32`（常量已按上下文收敛了类型，但 `ConstVal` 仍是
        // `Int`）。当整数装立即数会让槽里是 `35` 而不是 `35.0` 的位模式 ——
        // 浮点比较/运算拿到 `0x23`（≈ 5e-44）。
        ConstVal::Int(i) if cx.class(dst) == SlotClass::Float => {
            if cx.is_f32(typ) {
                let bits = u64::from((*i as f32).to_bits());
                let lbl = asm.rodata_f64(bits);
                load_addr(asm, "x10", &lbl);
                asm.ins("ldr s10, [x10]");
                st_s(asm, "s10", s.off);
            } else {
                let lbl = asm.rodata_f64((*i as f64).to_bits());
                load_addr(asm, "x10", &lbl);
                asm.ins("ldr d10, [x10]");
                st_d(asm, "d10", s.off);
            }
        }
        ConstVal::Int(i) => {
            load_imm(asm, "x10", *i as i64);
            mask_to(asm, "x10", cx.tt, typ);
            st_x(asm, "x10", s.off);
        }
        ConstVal::Float(f) => {
            if cx.is_f32(typ) {
                let bits = u64::from((*f as f32).to_bits());
                let lbl = asm.rodata_f64(bits);
                load_addr(asm, "x10", &lbl);
                asm.ins("ldr s10, [x10]");
                st_s(asm, "s10", s.off);
            } else {
                let lbl = asm.rodata_f64(f.to_bits());
                load_addr(asm, "x10", &lbl);
                asm.ins("ldr d10, [x10]");
                st_d(asm, "d10", s.off);
            }
        }
        ConstVal::Complex(re, im) => {
            // ⚠️ `complex64` 是 4+4 字节、`complex128` 是 8+8 —— 步长必须按类型选。
            // 一律写 8 会让 `complex64` 的虚部落到**槽外**（`probe`：`booltest(5+6i,…)`
            // 收到 `(0+0i)`，因为虚部覆盖了相邻槽）。
            if cx.class(dst) == SlotClass::Complex64 {
                let lo = asm.rodata_f64(u64::from((*re as f32).to_bits()));
                let hi = asm.rodata_f64(u64::from((*im as f32).to_bits()));
                load_addr(asm, "x10", &lo);
                asm.ins("ldr s10, [x10]");
                st_s(asm, "s10", s.off);
                load_addr(asm, "x10", &hi);
                asm.ins("ldr s10, [x10]");
                st_s(asm, "s10", s.off + 4);
            } else {
                let lo = asm.rodata_f64(re.to_bits());
                let hi = asm.rodata_f64(im.to_bits());
                load_addr(asm, "x10", &lo);
                asm.ins("ldr d10, [x10]");
                st_d(asm, "d10", s.off);
                load_addr(asm, "x10", &hi);
                asm.ins("ldr d10, [x10]");
                st_d(asm, "d10", s.off + 8);
            }
        }
        ConstVal::Str(bytes) => {
            let lbl = asm.rodata_bytes(bytes.as_bytes());
            load_addr(asm, "x10", &lbl);
            st_x(asm, "x10", s.off);
            load_imm(asm, "x10", bytes.len() as i64);
            st_x(asm, "x10", s.off + 8);
        }
    }
}

/// `adrp` + `add :lo12:` 取符号地址（PIE 安全形式，沿 `T-S3-01`）。
fn load_addr(asm: &mut Asm, reg: &str, label: &str) {
    asm.ins(&format!("adrp {reg}, {label}"));
    asm.ins(&format!("add {reg}, {reg}, :lo12:{label}"));
}

/// `copy`：8 字节以内一个字，超过就 `memcpy`。
///
/// # ⚠️ 这条基本是死代码（2026-10-25 实测）
///
/// `Instr::Copy` 在语料 46 个文件的 SSA 里出现 **0 次** —— `build.rs` 从不产生它
/// （赋值走 `Load`/`Store`，聚合的值复制在那两条里各自 `memcpy`）。
/// 所以这条只剩 `convert` 的兜底分支（`_ => copy(...)`）可能用到，而那个也到不了：
/// 指针/整数的转换都落在 `(Int, Int)`，复数有专支，聚合转换在 Go 里非法。
///
/// 留着不是因为有用，而是**删它属于改 IR**（`Instr::Copy` 是 `C4` 的变体）。
/// 但别把「聚合拷贝」的判据挂在这里 —— `native_artifacts.rs` 的两条值语义测试
/// 走的是 `load`/`store`，把 `store` 的 `memcpy` 改坏才会让它们红（已实测）。
fn copy(cx: &Ctx<'_>, dst: ValueId, src: ValueId, asm: &mut Asm) {
    let (d, s) = (cx.slot(dst), cx.slot(src));
    if d.size <= 8 {
        ld_x(asm, "x10", s.off);
        st_x(asm, "x10", d.off);
    } else {
        memcpy(asm, d.off, s.off, d.size);
    }
}

/// 用 runtime 的 `rt_memcpy` 拷一块（`dst`/`src` 都是相对 `fp` 的偏移）。
fn memcpy(asm: &mut Asm, dst: i32, src: i32, size: u32) {
    addr(asm, dst);
    asm.ins("mov x0, x9");
    addr(asm, src);
    asm.ins("mov x1, x9");
    load_imm(asm, "x2", i64::from(size));
    asm.ins("bl rt_memcpy");
}

/// 二元运算。
fn bin(
    cx: &Ctx<'_>,
    dst: ValueId,
    op: BinOp,
    lhs: ValueId,
    rhs: ValueId,
    typ: TypeId,
    asm: &mut Asm,
) {
    let d = cx.slot(dst);
    let (l, r) = (cx.slot(lhs), cx.slot(rhs));

    // ⚠️ 字符串必须先分流：它的 `SlotClass` 是 `Int`（16 字节标量），
    // 落到下面就会把 `(ptr,len)` 当整数做**指针运算** —— 静默错，不崩。
    // 实测 `probe2.go`：`mk("ab") == mk("ab")` 给 0、`mk("a")+"b"` 给空串。
    // 两个操作数的类型才是判据（`dst` 在比较里是 `bool`）。
    if is_string_ty(cx.tt, cx.ty(lhs)) {
        ld_x(asm, "x0", l.off);
        ld_x(asm, "x1", l.off + 8);
        ld_x(asm, "x2", r.off);
        ld_x(asm, "x3", r.off + 8);
        match op {
            // `rt_strcat` 按 AAPCS64 用 `x0/x1` 返回 16 字节的 `{ptr,len}`。
            BinOp::Add => {
                asm.ins("bl rt_strcat");
                st_x(asm, "x0", d.off);
                st_x(asm, "x1", d.off + 8);
            }
            // `rt_strcmp` 返回 -1/0/1（字典序，按**字节**比较 —— Go 的 `<` 是字节序）。
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                asm.ins("bl rt_strcmp");
                asm.ins("cmp x0, #0");
                let cond = icmp_cond(op, false).unwrap_or("eq");
                asm.ins(&format!("cset x12, {cond}"));
                st_x(asm, "x12", d.off);
            }
            _ => {}
        }
        return;
    }

    // 接口比较（`M9/Task 35`）：比的是**动态值**，不是 `data` 字的位。
    //
    // `any("abc")` 两次装箱各有各的盒子 ⇒ 按字节比 `data` 永远不等
    // （实测 `ken/interbasic.go` 的 `panic("a")`）。
    if is_iface_ty(cx.tt, cx.ty(lhs)) && is_iface_ty(cx.tt, cx.ty(rhs)) {
        let eface = iface_arity(cx.tt, cx.ty(lhs)) == 0;
        ld_x(asm, "x0", l.off);
        ld_x(asm, "x1", l.off + 8);
        ld_x(asm, "x2", r.off);
        ld_x(asm, "x3", r.off + 8);
        load_imm(asm, "x4", i64::from(eface));
        asm.ins("bl rt_iface_eq");
        asm.ins("cmp x0, #0");
        let cond = if op == BinOp::Ne { "eq" } else { "ne" };
        asm.ins(&format!("cset x12, {cond}"));
        st_x(asm, "x12", d.off);
        return;
    }

    // ⚠️ 比较的**结果**是 `bool`（`Int` 类），所以按 `dst` 分类会把浮点比较
    // 送进整数分支 —— `1.5 < 2` 变成「两个位模式比大小」，静默错。
    // 比较一律按**操作数**分类；算术仍按 `dst`（两者同型）。
    let is_cmp = matches!(
        op,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
    );
    let opnd = cx.ty(lhs);
    let class = if is_cmp { cx.class(lhs) } else { cx.class(dst) };
    match class {
        SlotClass::Float if cx.is_f32(opnd) => {
            // `float32`：`s` 寄存器 + 每次运算后按 `f32` 舍入（`ops::normalize` 同口径）。
            ld_s(asm, "s10", l.off);
            ld_s(asm, "s11", r.off);
            let ins = match op {
                BinOp::Add => "fadd",
                BinOp::Sub => "fsub",
                BinOp::Mul => "fmul",
                BinOp::Div => "fdiv",
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    asm.ins("fcmp s10, s11");
                    let cond = fcmp_cond(op);
                    asm.ins(&format!("cset x12, {cond}"));
                    st_x(asm, "x12", d.off);
                    return;
                }
                _ => return,
            };
            asm.ins(&format!("{ins} s12, s10, s11"));
            st_s(asm, "s12", d.off);
        }
        SlotClass::Float => {
            ld_d(asm, "d10", l.off);
            ld_d(asm, "d11", r.off);
            let ins = match op {
                BinOp::Add => "fadd",
                BinOp::Sub => "fsub",
                BinOp::Mul => "fmul",
                BinOp::Div => "fdiv",
                // 比较：`fcmp` 后 `cset`
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    asm.ins("fcmp d10, d11");
                    let cond = fcmp_cond(op);
                    asm.ins(&format!("cset x12, {cond}"));
                    st_x(asm, "x12", d.off);
                    return;
                }
                // 浮点没有位运算/取模（前端已拒），到这里就是内部不一致。
                _ => return,
            };
            asm.ins(&format!("{ins} d12, d10, d11"));
            st_d(asm, "d12", d.off);
        }
        SlotClass::Complex128 => complex_bin(op, l.off, r.off, d.off, asm),
        SlotClass::Complex64 => complex_bin_f32(op, l.off, r.off, d.off, asm),
        SlotClass::Agg => {
            // 聚合的比较按**字节**（Go 的可比较聚合就是逐字段/逐元素比）。
            //
            // 不做这一步，`[2]int{1,2} == [2]int{1,2}` 的结果槽永远是垃圾 ——
            // 实测 `newexpr.go` 的 `*p != x`（`x` 是 `[2]int`）直接 panic。
            // 结构体的**填充字节**在 Go 里不参与比较，但我们的槽在写入前已清零
            // （`composite` 先清），所以逐字节比等价于逐字段比。
            if is_cmp {
                addr(asm, l.off);
                asm.ins("mov x0, x9");
                addr(asm, r.off);
                asm.ins("mov x1, x9");
                load_imm(asm, "x2", i64::from(l.size));
                asm.ins("bl rt_memcmp");
                asm.ins("cmp x0, #0");
                let cond = icmp_cond(op, false).unwrap_or("eq");
                asm.ins(&format!("cset x12, {cond}"));
                st_x(asm, "x12", d.off);
            }
        }
        SlotClass::Int => {
            ld_x(asm, "x10", l.off);
            ld_x(asm, "x11", r.off);
            if let Some(cond) = icmp_cond(op, cx.is_unsigned(opnd)) {
                asm.ins("cmp x10, x11");
                asm.ins(&format!("cset x12, {cond}"));
                st_x(asm, "x12", d.off);
                return;
            }
            let signed = !cx.is_unsigned(typ);
            match op {
                BinOp::Add => asm.ins("add x12, x10, x11"),
                BinOp::Sub => asm.ins("sub x12, x10, x11"),
                BinOp::Mul => asm.ins("mul x12, x10, x11"),
                BinOp::And => asm.ins("and x12, x10, x11"),
                BinOp::Or => asm.ins("orr x12, x10, x11"),
                BinOp::Xor => asm.ins("eor x12, x10, x11"),
                BinOp::AndNot => {
                    asm.ins("bic x12, x10, x11");
                }
                // ⚠️ **移位计数达到位宽时结果由 Go 规定，不是硬件行为**。
                //
                // Go：`x << n` 等价于「移 n 次 1 位」，n **没有上限** ——
                // 所以 n ≥ 位宽时结果恒为 0（`asr` 则全是符号位）。
                // AArch64 的 `lsl/asr/lsr` 只取寄存器操作数的低 6 位，于是
                // `1234 << 1025` 变成 `1234 << 1 = 2468`（实测 `ken/shift.go`）。
                BinOp::Shl | BinOp::Shr => {
                    let bits = i64::from(cx.width(opnd).max(1) * 8);
                    asm.ins(&format!("cmp x11, #{bits}"));
                    match op {
                        BinOp::Shl => {
                            asm.ins("lsl x12, x10, x11");
                            asm.ins("mov x14, #0");
                        }
                        _ if signed => {
                            asm.ins("asr x12, x10, x11");
                            // 全是符号位 = 把符号位铺满（`asr #63` 得到 0 / -1）。
                            asm.ins("asr x14, x10, #63");
                        }
                        _ => {
                            asm.ins("lsr x12, x10, x11");
                            asm.ins("mov x14, #0");
                        }
                    }
                    // `csel` 用的是 `cmp` 留下的标志（移位不设标志，`mov` 也不设）。
                    asm.ins("csel x12, x12, x14, lo");
                }
                BinOp::Div | BinOp::Rem => {
                    // 除零是运行期 panic（`C9` §5：退出码 2）。
                    asm.ins("cbz x11, .Ldiv0");
                    let ins = if signed { "sdiv" } else { "udiv" };
                    asm.ins(&format!("{ins} x12, x10, x11"));
                    if op == BinOp::Rem {
                        // x12 = x10 - x12*x11
                        asm.ins("msub x12, x12, x11, x10");
                    }
                }
                // `&&` / `||` 在 SSA 里是控制流（`R-33-2`），不该出现在 `Bin`。
                BinOp::LAnd | BinOp::LOr => return,
                _ => return,
            }
            mask_to(asm, "x12", cx.tt, typ);
            st_x(asm, "x12", d.off);
        }
    }
}

/// 整数比较的 `cset` 条件。
fn icmp_cond(op: BinOp, unsigned: bool) -> Option<&'static str> {
    let c = match op {
        BinOp::Eq => "eq",
        BinOp::Ne => "ne",
        BinOp::Lt => {
            if unsigned {
                "lo"
            } else {
                "lt"
            }
        }
        BinOp::Le => {
            if unsigned {
                "ls"
            } else {
                "le"
            }
        }
        BinOp::Gt => {
            if unsigned {
                "hi"
            } else {
                "gt"
            }
        }
        BinOp::Ge => {
            if unsigned {
                "hs"
            } else {
                "ge"
            }
        }
        _ => return None,
    };
    Some(c)
}

/// 浮点比较的 `cset` 条件（`NaN` 让六个运算符全 false，除 `!=`）。
fn fcmp_cond(op: BinOp) -> &'static str {
    match op {
        BinOp::Eq => "eq",
        BinOp::Ne => "ne",
        BinOp::Lt => "mi",
        BinOp::Le => "ls",
        BinOp::Gt => "gt",
        _ => "ge",
    }
}

/// `complex128` 的加减乘除（实部虚部各一个 `d`）。
fn complex_bin(op: BinOp, l: i32, r: i32, d: i32, asm: &mut Asm) {
    match op {
        BinOp::Add | BinOp::Sub => {
            let ins = if op == BinOp::Add { "fadd" } else { "fsub" };
            for k in [0, 8] {
                ld_d(asm, "d10", l + k);
                ld_d(asm, "d11", r + k);
                asm.ins(&format!("{ins} d12, d10, d11"));
                st_d(asm, "d12", d + k);
            }
        }
        BinOp::Mul => {
            // (a+bi)(c+di) = (ac-bd) + (ad+bc)i
            ld_d(asm, "d10", l); // a
            ld_d(asm, "d11", r); // c
            asm.ins("fmul d12, d10, d11"); // ac
            ld_d(asm, "d13", l + 8); // b
            ld_d(asm, "d14", r + 8); // d
            asm.ins("fmul d15, d13, d14"); // bd
            asm.ins("fsub d12, d12, d15"); // ac-bd
            st_d(asm, "d12", d);
            asm.ins("fmul d12, d10, d14"); // ad
            asm.ins("fmul d15, d13, d11"); // bc
            asm.ins("fadd d12, d12, d15");
            st_d(asm, "d12", d + 8);
        }
        BinOp::Div => {
            // Smith 形式：(a+bi)/(c+di) = ((ac+bd) + (bc-ad)i) / (c²+d²)
            ld_d(asm, "d10", l);
            ld_d(asm, "d13", l + 8);
            ld_d(asm, "d11", r);
            ld_d(asm, "d14", r + 8);
            asm.ins("fmul d15, d11, d11");
            asm.ins("fmul d16, d14, d14");
            asm.ins("fadd d15, d15, d16"); // den = c²+d²
            asm.ins("fmul d12, d10, d11"); // ac
            asm.ins("fmul d16, d13, d14"); // bd
            asm.ins("fadd d12, d12, d16");
            asm.ins("fdiv d12, d12, d15");
            st_d(asm, "d12", d);
            asm.ins("fmul d12, d13, d11"); // bc
            asm.ins("fmul d16, d10, d14"); // ad
            asm.ins("fsub d12, d12, d16");
            asm.ins("fdiv d12, d12, d15");
            st_d(asm, "d12", d + 8);
        }
        _ => {}
    }
}

/// `complex64` 的算术（两个 `f32` 并排放在一个 8 字节槽里）。
///
/// # 为什么按 `f32` 算，不「升到 f64 算完再降回来」
///
/// Go 规定 `complex64` 的四则按 `float32` 做（每一步都舍入到 `f32`）。
/// 用 f64 中间量只在最后舍入一次 —— 结果**更准**，但不是 Go 的结果，
/// 末位会差。判据是「与解释器逐字节相同」，所以必须照 Go 的精度做。
fn complex_bin_f32(op: BinOp, l: i32, r: i32, d: i32, asm: &mut Asm) {
    match op {
        BinOp::Add | BinOp::Sub => {
            let ins = if op == BinOp::Add { "fadd" } else { "fsub" };
            for k in [0, 4] {
                ld_s(asm, "s10", l + k);
                ld_s(asm, "s11", r + k);
                asm.ins(&format!("{ins} s12, s10, s11"));
                st_s(asm, "s12", d + k);
            }
        }
        BinOp::Mul => {
            // (a+bi)(c+di) = (ac-bd) + (ad+bc)i
            ld_s(asm, "s10", l); // a
            ld_s(asm, "s11", l + 4); // b
            ld_s(asm, "s12", r); // c
            ld_s(asm, "s13", r + 4); // d
            asm.ins("fmul s14, s10, s12"); // ac
            asm.ins("fmul s15, s11, s13"); // bd
            asm.ins("fsub s16, s14, s15"); // re
            asm.ins("fmul s14, s10, s13"); // ad
            asm.ins("fmul s15, s11, s12"); // bc
            asm.ins("fadd s17, s14, s15"); // im
            st_s(asm, "s16", d);
            st_s(asm, "s17", d + 4);
        }
        BinOp::Div => {
            // (a+bi)/(c+di) = ((ac+bd) + (bc-ad)i) / (c²+d²)
            ld_s(asm, "s10", l);
            ld_s(asm, "s11", l + 4);
            ld_s(asm, "s12", r);
            ld_s(asm, "s13", r + 4);
            asm.ins("fmul s18, s12, s12"); // c²
            asm.ins("fmul s19, s13, s13"); // d²
            asm.ins("fadd s18, s18, s19"); // den
            asm.ins("fmul s14, s10, s12"); // ac
            asm.ins("fmul s15, s11, s13"); // bd
            asm.ins("fadd s16, s14, s15"); // ac+bd
            asm.ins("fdiv s16, s16, s18");
            asm.ins("fmul s14, s11, s12"); // bc
            asm.ins("fmul s15, s10, s13"); // ad
            asm.ins("fsub s17, s14, s15"); // bc-ad
            asm.ins("fdiv s17, s17, s18");
            st_s(asm, "s16", d);
            st_s(asm, "s17", d + 4);
        }
        BinOp::Eq | BinOp::Ne => {
            ld_s(asm, "s10", l);
            ld_s(asm, "s11", r);
            asm.ins("fcmp s10, s11");
            asm.ins("cset x10, eq");
            ld_s(asm, "s10", l + 4);
            ld_s(asm, "s11", r + 4);
            asm.ins("fcmp s10, s11");
            asm.ins("cset x11, eq");
            asm.ins("and x10, x10, x11");
            if op == BinOp::Ne {
                asm.ins("eor x10, x10, #1");
            }
            st_x(asm, "x10", d);
        }
        // `complex` 没有 `<` / `>`（Go 直接拒），走到这里是上游违约。
        _ => {}
    }
}

/// 一元运算。
fn un(cx: &Ctx<'_>, dst: ValueId, op: UnOp, x: ValueId, typ: TypeId, asm: &mut Asm) {
    let (d, s) = (cx.slot(dst), cx.slot(x));
    match (op, cx.class(dst)) {
        (UnOp::Neg, SlotClass::Float) => {
            ld_d(asm, "d10", s.off);
            asm.ins("fneg d12, d10");
            st_d(asm, "d12", d.off);
        }
        // 复数取负：实部虚部各自 `fneg`。
        // 落到下面的整数分支会把 16 字节的复数当整数 `neg` —— 实部变成
        // 「虚部的补码」，虚部原样不动，产物是彻底的垃圾。
        (UnOp::Neg, SlotClass::Complex128) => {
            for k in [0, 8] {
                ld_d(asm, "d10", s.off + k);
                asm.ins("fneg d12, d10");
                st_d(asm, "d12", d.off + k);
            }
        }
        (UnOp::Neg, SlotClass::Complex64) => {
            for k in [0, 4] {
                ld_s(asm, "s10", s.off + k);
                asm.ins("fneg s12, s10");
                st_s(asm, "s12", d.off + k);
            }
        }
        (UnOp::Neg, _) => {
            ld_x(asm, "x10", s.off);
            asm.ins("neg x12, x10");
            mask_to(asm, "x12", cx.tt, typ);
            st_x(asm, "x12", d.off);
        }
        (UnOp::Pos, _) => copy(cx, dst, x, asm),
        (UnOp::BitNot, _) => {
            ld_x(asm, "x10", s.off);
            asm.ins("mvn x12, x10");
            mask_to(asm, "x12", cx.tt, typ);
            st_x(asm, "x12", d.off);
        }
        // `!x`：`x == 0`
        (UnOp::Not, _) => {
            ld_x(asm, "x10", s.off);
            asm.ins("cmp x10, #0");
            asm.ins("cset x12, eq");
            st_x(asm, "x12", d.off);
        }
        // `<-ch` 在 M5 就被拒（`U-M5-01`）。
        (UnOp::Recv, _) => {}
    }
}

// ------------------------ 接口（M9/Task 31-35）------------------------

/// 是不是接口类型（剥具名 / 别名；不剥指针 —— `*I` 不是接口）。
fn is_iface_ty(tt: &TypeTable, t: TypeId) -> bool {
    use rgoc_types::typ::Type;
    let mut cur = t;
    for _ in 0..64 {
        match tt.type_of(cur) {
            Type::Named(id) => cur = tt.named_underlying(*id),
            Type::Alias(id) => cur = tt.alias_rhs(*id),
            Type::Interface(_) => return true,
            _ => return false,
        }
    }
    false
}

/// 接口的**方法数**。`0` 表示 `any` / `interface{}`（值是 `{type, data}`），
/// 否则是 `{tab, data}`。
///
/// 槽序的**唯一来源**是 `rgoc_ssa::ir::iface_methods` —— 静态 ITab 的填序、
/// 调用点的取槽序、`rt_itab` 的查序都用它（`R-9-10`）。
fn iface_arity(tt: &TypeTable, t: TypeId) -> usize {
    rgoc_ssa::ir::iface_methods(tt, t).len()
}

/// 具体值到接口值（`M9/Task 31`）。
///
/// | 动态类型 | `data` 字 | 为什么 |
/// |---|---|---|
/// | 指针 `*C` | 指针**值本身** | 官方同口径；装箱会让「两个 nil 指针的接口」不相等 |
/// | 其余 `C` | `rt_box(desc(C), &v)` | 值必须活在堆上（接口可能比它活得久）|
fn box_iface(cx: &Ctx<'_>, dst: ValueId, x: ValueId, to: TypeId, asm: &mut Asm) {
    let (d, s) = (cx.slot(dst), cx.slot(x));
    let from = cx.ty(x);
    let fh = rgoc_meta::desc::type_hash(cx.tt, from);
    let is_ptr = matches!(
        cx.tt.type_of(rgoc_types::underlying(cx.tt, from)),
        rgoc_types::typ::Type::Pointer(_)
    );
    if is_ptr {
        ld_x(asm, "x10", s.off);
        st_x(asm, "x10", d.off + 8);
    } else {
        load_addr(asm, "x0", &crate::module::desc_symbol(fh));
        addr(asm, s.off);
        asm.ins("mov x1, x9");
        asm.ins("bl rt_box");
        st_x(asm, "x0", d.off + 8);
    }
    if iface_arity(cx.tt, to) == 0 {
        // `any`：第 0 个字是**类型描述符**，不是 itab
        load_addr(asm, "x10", &crate::module::desc_symbol(fh));
    } else {
        load_addr(
            asm,
            "x10",
            &crate::module::itab_symbol(rgoc_meta::desc::type_hash(cx.tt, to), fh),
        );
    }
    st_x(asm, "x10", d.off);
}

/// 接口到接口（`var j J = i` / `var a any = i`）。
///
/// * 目标 `any`：`type = 源的 concrete desc`（不重新装箱）；
/// * 目标有方法：`tab = rt_itab(desc(J), 源的 concrete desc)`，null 则 panic。
fn retag_iface(cx: &Ctx<'_>, dst: ValueId, x: ValueId, to: TypeId, asm: &mut Asm) {
    let (d, s) = (cx.slot(dst), cx.slot(x));
    let from = cx.ty(x);
    // 源的具体类型 desc 放进 x11
    if iface_arity(cx.tt, from) == 0 {
        ld_x(asm, "x11", s.off);
    } else {
        ld_x(asm, "x9", s.off);
        asm.ins("ldr x11, [x9, #8]");
    }
    // data 先落位（后面要 bl，x10 会被打掉）
    ld_x(asm, "x10", s.off + 8);
    st_x(asm, "x10", d.off + 8);
    if iface_arity(cx.tt, to) == 0 {
        st_x(asm, "x11", d.off);
        return;
    }
    load_addr(
        asm,
        "x0",
        &crate::module::desc_symbol(rgoc_meta::desc::type_hash(cx.tt, to)),
    );
    asm.ins("mov x1, x11");
    asm.ins("bl rt_itab");
    let ok = format!(".Lretag{}", asm.fresh_label());
    asm.ins(&format!("cbnz x0, {ok}"));
    asm.ins("bl rt_panic_assert");
    asm.label(&ok);
    st_x(asm, "x0", d.off);
}

/// `x.{typ}`（`M9/Task 33`）。`ok == None` 时失败 panic；`Some` 时失败给零值 + false。
fn type_assert(
    cx: &Ctx<'_>,
    dst: ValueId,
    ok: Option<ValueId>,
    x: ValueId,
    typ: TypeId,
    asm: &mut Asm,
) {
    let (d, s) = (cx.slot(dst), cx.slot(x));
    let from = cx.ty(x);
    let dsize = rgoc_types::layout::layout(cx.tt, typ).map_or(0, |l| l.size);
    let to_iface = is_iface_ty(cx.tt, typ);
    // 源的具体类型 desc 放进 x11
    if iface_arity(cx.tt, from) == 0 {
        ld_x(asm, "x11", s.off);
    } else {
        ld_x(asm, "x9", s.off);
        asm.ins("ldr x11, [x9, #8]");
    }

    if to_iface {
        // 到接口：tab = rt_itab(desc(J), concrete)；null 表示失败
        load_addr(
            asm,
            "x0",
            &crate::module::desc_symbol(rgoc_meta::desc::type_hash(cx.tt, typ)),
        );
        asm.ins("mov x1, x11");
        asm.ins("bl rt_itab");
        // data 与 tab 一起落位（失败时 tab 是 0，正好是零值）
        st_x(asm, "x0", d.off);
        ld_x(asm, "x10", s.off + 8);
        st_x(asm, "x10", d.off + 8);
        finish_assert(cx, ok, asm, "x0");
        return;
    }

    // 到具体类型：比 desc
    load_addr(
        asm,
        "x9",
        &crate::module::desc_symbol(rgoc_meta::desc::type_hash(cx.tt, typ)),
    );
    asm.ins("cmp x11, x9");
    let hit = format!(".Lass_hit{}", asm.fresh_label());
    let done = format!(".Lass_end{}", asm.fresh_label());
    asm.ins(&format!("b.eq {hit}"));
    // 未命中
    match ok {
        None => {
            asm.ins("bl rt_panic_assert");
            asm.ins(&format!("b {done}"));
        }
        Some(o) => {
            // `v, ok := x.(T)` 失败：v 是**零值**，ok = false
            zero_slot(asm, d.off, dsize.max(8));
            st_x(asm, "xzr", cx.slot(o).off);
            asm.ins(&format!("b {done}"));
        }
    }
    asm.label(&hit);
    // 命中：data 指向值本身（指针动态类型时**就是**那个指针）
    let is_ptr = matches!(
        cx.tt.type_of(rgoc_types::underlying(cx.tt, typ)),
        rgoc_types::typ::Type::Pointer(_)
    );
    if is_ptr {
        // ⚠️ 不能用 `x9` 当数据寄存器：`ld_x`/`st_x` 拿它当**地址**暂存
        // （`ADDR`），`st_x(x9, ..)` 会把「dst 的地址」写进 dst
        // （实测 `ken/interfun.go`：`i1.(*S)` 得到的是栈地址）。
        ld_x(asm, "x10", s.off + 8);
        st_x(asm, "x10", d.off);
    } else if dsize > 0 {
        // 先把整个槽清零，再拷 `dsize` 字节。
        //
        // ⚠️ 不先清零的话，`x.(bool)`（size 1）只覆盖 1 字节，槽里另外 7 字节
        // 是上一个值的残留 ⇒ `b != true` 按 8 字节比就**不等**
        // （实测 `ken/interbasic.go`：打出 `2 true` 却 panic "fail"）。
        zero_slot(asm, d.off, dsize.div_ceil(8) * 8);
        // x0 = dst 地址，x1 = 源（盒子），x2 = 字节数
        addr(asm, d.off);
        asm.ins("mov x0, x9");
        ld_x(asm, "x1", s.off + 8);
        load_imm(asm, "x2", i64::from(dsize));
        asm.ins("bl rt_memcpy");
    }
    if let Some(o) = ok {
        load_imm(asm, "x10", 1);
        st_x(asm, "x10", cx.slot(o).off);
    }
    asm.label(&done);
}

/// 断言的收尾：`ok` 有值时 `ok = (reg != 0)`；没有值时 `reg == 0` 就 panic。
fn finish_assert(cx: &Ctx<'_>, ok: Option<ValueId>, asm: &mut Asm, reg: &str) {
    match ok {
        Some(o) => {
            asm.ins(&format!("cmp {reg}, #0"));
            asm.ins("cset x10, ne");
            st_x(asm, "x10", cx.slot(o).off);
        }
        None => {
            let good = format!(".Lass_nz{}", asm.fresh_label());
            asm.ins(&format!("cbnz {reg}, {good}"));
            asm.ins("bl rt_panic_assert");
            asm.label(&good);
        }
    }
}

/// 类型转换。
fn convert(cx: &Ctx<'_>, dst: ValueId, x: ValueId, typ: TypeId, asm: &mut Asm) {
    let (d, s) = (cx.slot(dst), cx.slot(x));
    let from = cx.ty(x);

    // 接口的三条转换（M9/Task 31 / 33）。**必须排在最前面**：接口是
    // `SlotClass::Agg`（16 字节），落到下面按 class 分派的表里只会被按字节搬，
    // 而「装箱」根本不是搬字节。
    let to_if = is_iface_ty(cx.tt, typ);
    let from_if = is_iface_ty(cx.tt, from);
    match (from_if, to_if) {
        (false, true) => {
            box_iface(cx, dst, x, typ, asm);
            return;
        }
        (true, true) => {
            retag_iface(cx, dst, x, typ, asm);
            return;
        }
        (true, false) => {
            // 接口到具体：单值形态（失败 panic）。comma-ok 走 `Instr::TypeAssert`。
            type_assert(cx, dst, None, x, typ, asm);
            return;
        }
        (false, false) => {}
    }
    let to_class = cx.class(dst);
    let from_class = cx.class(x);

    // `string(i)`：整数按**码点**编码成 UTF-8（`C9` §4.3）。
    //
    // 这一条必须先判：`string` 与整数同属 `Int` 类，落到下面那条
    // `(Int, Int)` 分支会原样搬 8 字节 —— 于是「字符串」的 `ptr` 是个小整数、
    // `len` 是上一个值的残留。实测 `probe3.go`：`println(string(53))` 打空行，
    // `"x" + s` 拿一个巨大的 `len` 去分配 ⇒ `panic: out of arena`。
    // `string([]byte)` / `string([]rune)`（M9/Task 13）。
    //
    // 必须在 `string(int)` 那条**之前**：切片的 `SlotClass` 是 `Agg`
    // （24 字节），落不到 `Int` 分支，但顺序上把它放前面更省心。
    if is_string_ty(cx.tt, typ)
        && let Some(elem) = slice_elem(cx.tt, from)
    {
        {
            let esz = rgoc_types::layout::layout(cx.tt, elem).map_or(1, |l| l.size);
            ld_x(asm, "x0", s.off);
            ld_x(asm, "x1", s.off + 8);
            if esz == 1 {
                asm.ins("bl rt_bytes2str");
            } else {
                asm.ins("bl rt_runes2utf8");
            }
            st_x(asm, "x0", d.off);
            st_x(asm, "x1", d.off + 8);
            return;
        }
    }
    if is_string_ty(cx.tt, typ) && !is_string_ty(cx.tt, from) && from_class == SlotClass::Int {
        ld_x(asm, "x0", s.off);
        asm.ins("bl rt_i64_to_str");
        st_x(asm, "x0", d.off);
        st_x(asm, "x1", d.off + 8);
        return;
    }

    match (from_class, to_class) {
        (SlotClass::Int, SlotClass::Int) => {
            ld_x(asm, "x10", s.off);
            mask_to(asm, "x10", cx.tt, typ);
            st_x(asm, "x10", d.off);
        }
        (SlotClass::Int, SlotClass::Float) => {
            ld_x(asm, "x10", s.off);
            let ins = if cx.is_unsigned(from) {
                "ucvtf"
            } else {
                "scvtf"
            };
            if cx.is_f32(typ) {
                asm.ins(&format!("{ins} s12, x10"));
                st_s(asm, "s12", d.off);
            } else {
                asm.ins(&format!("{ins} d12, x10"));
                st_d(asm, "d12", d.off);
            }
        }
        (SlotClass::Float, SlotClass::Int) => {
            let ins = if cx.is_unsigned(typ) {
                "fcvtzu"
            } else {
                "fcvtzs"
            };
            if cx.is_f32(from) {
                ld_s(asm, "s10", s.off);
                asm.ins(&format!("{ins} x12, s10"));
            } else {
                ld_d(asm, "d10", s.off);
                asm.ins(&format!("{ins} x12, d10"));
            }
            mask_to(asm, "x12", cx.tt, typ);
            st_x(asm, "x12", d.off);
        }
        (SlotClass::Float, SlotClass::Float) => {
            let (fw, tw) = (cx.is_f32(from), cx.is_f32(typ));
            match (fw, tw) {
                (true, false) => {
                    ld_s(asm, "s10", s.off);
                    asm.ins("fcvt d12, s10");
                    st_d(asm, "d12", d.off);
                }
                (false, true) => {
                    ld_d(asm, "d10", s.off);
                    asm.ins("fcvt s12, d10");
                    st_s(asm, "s12", d.off);
                }
                _ => {
                    ld_d(asm, "d10", s.off);
                    st_d(asm, "d10", d.off);
                }
            }
        }
        // 复数之间 / 实数到复数的转换。
        //
        // 落到下面的 `_ => copy` 会把 8 字节当整数搬：`complex128(ce)` 的实部
        // 变成「两个 f32 的位模式拼起来」，虚部是相邻槽的内容。实测 `cplx2.go`：
        // `real(complex128(ce))` 给 `8192.0019…`（正好是两个 f32 的位模式）。
        (SlotClass::Complex64, SlotClass::Complex128) => {
            ld_s(asm, "s10", s.off);
            ld_s(asm, "s11", s.off + 4);
            asm.ins("fcvt d10, s10");
            asm.ins("fcvt d11, s11");
            st_d(asm, "d10", d.off);
            st_d(asm, "d11", d.off + 8);
        }
        (SlotClass::Complex128, SlotClass::Complex64) => {
            ld_d(asm, "d10", s.off);
            ld_d(asm, "d11", s.off + 8);
            asm.ins("fcvt s10, d10");
            asm.ins("fcvt s11, d11");
            st_s(asm, "s10", d.off);
            st_s(asm, "s11", d.off + 4);
        }
        // `complex128(x)` / `complex64(x)`（`x` 是实数）：虚部为 0。
        (SlotClass::Float, SlotClass::Complex128) => {
            if cx.is_f32(from) {
                ld_s(asm, "s10", s.off);
                asm.ins("fcvt d10, s10");
            } else {
                ld_d(asm, "d10", s.off);
            }
            st_d(asm, "d10", d.off);
            asm.ins("fmov d11, xzr");
            st_d(asm, "d11", d.off + 8);
        }
        (SlotClass::Float, SlotClass::Complex64) => {
            if cx.is_f32(from) {
                ld_s(asm, "s10", s.off);
            } else {
                ld_d(asm, "d10", s.off);
                asm.ins("fcvt s10, d10");
            }
            st_s(asm, "s10", d.off);
            asm.ins("fmov s11, wzr");
            st_s(asm, "s11", d.off + 4);
        }
        // 其余组合（指针/聚合转换）在 Go 里要么非法、要么归 M9。
        _ => copy(cx, dst, x, asm),
    }
}

/// `&x`：帧槽取 `sp` 相对地址；全局取符号地址。
fn addr_of(cx: &Ctx<'_>, dst: ValueId, root: &rgoc_ssa::ir::RootId, asm: &mut Asm) {
    use rgoc_ssa::ir::Root;
    let d = cx.slot(dst);
    let root = cx.f.roots[root.index()];
    match root {
        Root::Slot(slot) => {
            if cx.fr.heap.contains_key(&slot) {
                // 逃逸槽：帧里那一格是 arena 指针，直接读出来（`R-13-5`）。
                if let Some(h) = cx.fr.heap.get(&slot) {
                    ld_x(asm, "x10", h.cell);
                    st_x(asm, "x10", d.off);
                }
            } else if let Some(s) = cx.fr.addr_slots.get(&slot) {
                // ⚠️ 不能用 `addr()` 再 `st_x(asm, "x9", …)`：`st_x` 自己会先
                // `add x9, x29, #off`，把刚算好的地址覆盖掉。所以地址进 `x10`。
                // 同上：`add` 的立即数只有 12 位。大偏移走寄存器。
                if (-4095..=4095).contains(&s.off) {
                    asm.ins(&format!("add x10, x29, #{}", s.off));
                } else {
                    load_imm(asm, "x10", i64::from(s.off));
                    asm.ins("add x10, x29, x10");
                }
                st_x(asm, "x10", d.off);
            }
        }
        Root::Global(g) => {
            let sym = format!("g{}", g.0);
            load_addr(asm, "x10", &sym);
            st_x(asm, "x10", d.off);
        }
        Root::Heap => {
            // M7 不支持堆分配（`U-M7-01` 的邻居）—— 复合字面量逃逸到堆属于 M9a。
        }
    }
}

/// `base.field`：基址 + 字段偏移。
fn field(cx: &Ctx<'_>, dst: ValueId, base: ValueId, index: usize, asm: &mut Asm) {
    let d = cx.slot(dst);
    let b = cx.slot(base);
    let off = field_offset_of(cx.tt, cx.ty(base), index);
    ld_x(asm, "x10", b.off);
    if off != 0 {
        asm.ins(&format!("add x10, x10, #{off}"));
    }
    st_x(asm, "x10", d.off);
}

/// 字段偏移：走 `rgoc_types::layout`（唯一来源）。
fn field_offset_of(tt: &TypeTable, ptr_ty: TypeId, index: usize) -> u32 {
    use rgoc_types::layout::field_offset;
    use rgoc_types::typ::Type;
    let mut cur = ptr_ty;
    for _ in 0..64 {
        match tt.type_of(cur) {
            Type::Pointer(e) => cur = *e,
            Type::Named(id) => cur = tt.named_underlying(*id),
            Type::Alias(id) => cur = tt.alias_rhs(*id),
            // `Index` 的 dst 类型沿用**底座**类型（`build.rs` 的 `new_value(t)`），
            // 所以 `s[0].X` 里 `field` 的 base 类型是 `[]T1` 而不是 `*T1`。
            // 而 `Index` 的语义就是「元素地址」⇒ 剥掉容器这层拿元素类型。
            //
            // 不剥的症状：`[]T1{{X:1,Y:2,Z:3}}[0].Y` 读出 `X`（偏移恒 0），
            // 而 `X` 与 `.Z` 都对 —— 只有中间那个字段错，极难看出规律。
            Type::Slice(e) => cur = *e,
            Type::Array { elem, .. } => cur = *elem,
            Type::Struct(sid) => return field_offset(tt, *sid, index).unwrap_or(0),
            _ => return 0,
        }
    }
    0
}

/// `base[i]`：基址 + `i * elem_size`。
fn index(cx: &Ctx<'_>, dst: ValueId, base: ValueId, i: ValueId, asm: &mut Asm) {
    let (d, b, iv) = (cx.slot(dst), cx.slot(base), cx.slot(i));
    let bt = cx.ty(base);
    let esz = elem_size_of(cx.tt, bt);
    ld_x(asm, "x10", b.off);
    ld_x(asm, "x11", iv.off);
    // 越界检查（M9/Task 14）：`i >= len` ⇒ panic。
    //
    // 下标按**无符号**比：负数下标（`i` 是有符号的）会变成一个巨大的无符号数，
    // 于是同一条 `b.hs` 就把它抓了 —— 不需要第二条「i < 0」的分支。
    //
    // 长度的来源按底座类型分三种：切片/字符串是头里的字 1，数组是长度常量。
    let len_src = if is_string_ty(cx.tt, bt) || slice_elem(cx.tt, bt).is_some() {
        Some(LenSrc::Word(8))
    } else {
        array_len_of_base(cx.tt, bt).map(|n| LenSrc::Const(i64::from(n)))
    };
    if let Some(src) = len_src {
        match src {
            LenSrc::Word(off) => ld_x(asm, "x12", b.off + off),
            LenSrc::Const(n) => load_imm(asm, "x12", n),
        }
        let ok = format!(".Lidx_ok{}", asm.fresh_label());
        asm.ins("cmp x11, x12");
        asm.ins(&format!("b.lo {ok}"));
        asm.ins("mov x0, x11");
        asm.ins("mov x1, x12");
        asm.ins("bl rt_panic_bounds");
        asm.label(&ok);
    }
    load_imm(asm, "x12", i64::from(esz));
    asm.ins("madd x10, x11, x12, x10");
    st_x(asm, "x10", d.off);
}

/// 底座是数组时的长度（`[n]T` **或** `*[n]T`）。
///
/// # 为什么必须剥一层指针
///
/// `a[1:3]` 里的 `a` 若是个**全局/局部数组**，SSA 给底座的是 `AddrOf(a)`，
/// 其类型是 `*[10]byte` —— 直接 `array_len_of` 会返回 `None`（它不是数组），
/// 于是「hi 缺省」那一支去读底座槽的第二个字（那是地址的高位），
/// 长度变成垃圾。`ken/slicearray.go` 的 `by = bx[0:]` 就是这条。
fn array_len_of_base(tt: &TypeTable, t: TypeId) -> Option<u32> {
    use rgoc_types::typ::Type;
    if let Some(n) = rgoc_types::layout::array_len_of(tt, t) {
        return Some(n);
    }
    let mut cur = t;
    for _ in 0..64 {
        match tt.type_of(cur) {
            Type::Pointer(e) => return rgoc_types::layout::array_len_of(tt, *e),
            Type::Named(id) => cur = tt.named_underlying(*id),
            Type::Alias(id) => cur = tt.alias_rhs(*id),
            _ => return None,
        }
    }
    None
}

/// 越界检查里「长度从哪来」。
enum LenSrc {
    /// 底座槽里第 `off` 个字节处的字（切片/字符串的 len）。
    Word(i32),
    /// 常量（数组长度）。
    Const(i64),
}

/// 元素宽度（字节）。
fn elem_size_of(tt: &TypeTable, ptr_ty: TypeId) -> u32 {
    use rgoc_types::layout::layout;
    use rgoc_types::typ::Type;
    let mut cur = ptr_ty;
    for _ in 0..64 {
        match tt.type_of(cur) {
            Type::Pointer(e) => cur = *e,
            Type::Named(id) => cur = tt.named_underlying(*id),
            Type::Alias(id) => cur = tt.alias_rhs(*id),
            // `s[i]` 里的 `s` 是 `string`：元素是 **1 字节**。
            // 落到 `_ => 8` 会按 8 字节步长走 —— 实测 `turing.go`：
            // `prog[pc]` 全读到字符串尾之后，brainfuck 程序一个字符都不执行，
            // 结果 `r` 是空串。
            Type::Basic(rgoc_types::BasicId::String | rgoc_types::BasicId::UntypedString) => {
                return 1;
            }
            Type::Array { elem, .. } => return layout(tt, *elem).map_or(8, |l| l.size.max(1)),
            // `[]T` 的步长是 `sizeof(T)`（M9/Task 13 抓到的既有 bug）。
            //
            // 落到 `_ => 8` 的症状：`[]byte` 的 `b[1]` 读到 `b[8]`。
            // 而 `b[0]` 永远是对的（偏移 0）—— 所以「只测第一个元素」的判据
            // 抓不到它。`ken/slicearray.go` 的 `by[i-lb]` 就是这条。
            Type::Slice(elem) => return layout(tt, *elem).map_or(1, |l| l.size.max(1)),
            _ => return 8,
        }
    }
    8
}

/// 类型是不是 `string`（含 `untyped string`；剥命名/别名）。
///
/// `string` 在 ABI 里是 `Int` 类的 16 字节标量 —— 类别看不出它是字符串，
/// 所以「是不是字符串」必须单独判，不能靠 `SlotClass`。
fn is_string_ty(tt: &TypeTable, t: TypeId) -> bool {
    matches!(
        basic_of(tt, t),
        Some(BasicId::String | BasicId::UntypedString)
    )
}

/// 类型是不是指针（剥命名/别名后看 `Type::Pointer`）。
fn is_pointer_ty(tt: &TypeTable, t: TypeId) -> bool {
    use rgoc_types::typ::Type;
    let mut cur = t;
    for _ in 0..64 {
        match tt.type_of(cur) {
            Type::Pointer(_) => return true,
            Type::Named(id) => cur = tt.named_underlying(*id),
            Type::Alias(id) => cur = tt.alias_rhs(*id),
            _ => return false,
        }
    }
    false
}

/// `map[K]V` 的 `(K, V)`（剥命名/别名）；不是 map ⇒ `None`。
fn map_kv(tt: &TypeTable, t: TypeId) -> Option<(TypeId, TypeId)> {
    use rgoc_types::typ::Type;
    let mut cur = t;
    for _ in 0..64 {
        match tt.type_of(cur) {
            Type::Map { key, elem } => return Some((*key, *elem)),
            Type::Named(id) => cur = tt.named_underlying(*id),
            Type::Alias(id) => cur = tt.alias_rhs(*id),
            _ => return None,
        }
    }
    None
}

/// `make(map[K]V)` ⇒ `rt_map_make(kdesc, vdesc)`，单字结果进槽。
///
/// 容量提示（`make(map[K]V, 100)`）**不求值**：`build.rs` 已经为它建了值，
/// 但 M9 忽略它（`rt_map_make` 从 8 个槽起步，靠 rehash 长）。忽略的代价只是
/// 「大 map 多几次 rehash」，没有可观测语义差异。
fn map_make(cx: &Ctx<'_>, dst: ValueId, typ: TypeId, asm: &mut Asm) {
    let d = cx.slot(dst);
    let (kt, vt) = map_kv(cx.tt, typ).expect("调用方刚判过是 map");
    let kd = crate::module::desc_symbol(rgoc_meta::desc::type_hash(cx.tt, kt));
    let vd = crate::module::desc_symbol(rgoc_meta::desc::type_hash(cx.tt, vt));
    load_addr(asm, "x0", &kd);
    load_addr(asm, "x1", &vd);
    asm.ins("bl rt_map_make");
    st_x(asm, "x0", d.off);
}

/// 把一个槽清成零（`size` 字节）。
///
/// # 为什么 runtime 的出参槽必须先清零
///
/// `rt_map_get` / `rt_map_iter` 只写 `val_size` 字节 —— 而 SSA 的槽按**8 字节**
/// 一格（`byte` 的槽也是 8 字节）。不清零的话高 7 字节留着上一条指令的垃圾，
/// 而读这个槽的人（`println` / `int` 转换）读的是**整字**。
///
/// 实测：`map[int]byte` 的 `for k := range m { v := m[k] }` 里
/// `v` 打印出 `281474823995395` 这种数字（`ken/range.go`）。
fn zero_slot(asm: &mut Asm, off: i32, size: u32) {
    if size <= 8 {
        st_x(asm, "xzr", off);
    } else {
        addr(asm, off);
        asm.ins("mov x0, x9");
        asm.ins("mov x1, xzr");
        load_imm(asm, "x2", i64::from(size));
        asm.ins("bl rt_memset");
    }
}

/// 运行期助手（`rt_*`）的调用序列（M9/Phase 2）。
///
/// # 为什么每个变体单独写
///
/// 它们的实参形状不一样：`MapGet` 要三个（值走 sret 风格的出参），`MapIter`
/// 要四个（两个出参 + 返回下一站），`MapSet` 的值可能超过 8 字节（整块地址）。
/// 用一张「参数个数表」去泛化只会把这些差异藏进数据里。
///
/// # 出参一律传**槽的地址**
///
/// 多字值（`[1000]byte` 的 map 值、结构体键）必须在内存里传，不能进寄存器。
/// 这也让 `MapIter` 不必在 IR 层引入「取地址」指令 —— `isel` 有槽偏移，
/// 直接 `addr` 就行。
fn runtime_call(cx: &Ctx<'_>, f: RuntimeFn, args: &[ValueId], dsts: &[ValueId], asm: &mut Asm) {
    match f {
        RuntimeFn::MapMake => {
            let Some(d) = dsts.first() else { return };
            let t = cx.ty(*d);
            map_make(cx, *d, t, asm);
        }
        RuntimeFn::MapGet => {
            let (Some(m), Some(k), Some(v)) = (args.first(), args.get(1), dsts.first()) else {
                return;
            };
            let (sm, sk, sv) = (cx.slot(*m), cx.slot(*k), cx.slot(*v));
            // 出参槽先清零（窄值的槽是 8 字节，runtime 只写 val_size 字节）。
            zero_slot(asm, sv.off, sv.size);
            ld_x(asm, "x0", sm.off);
            addr(asm, sk.off);
            asm.ins("mov x1, x9");
            addr(asm, sv.off);
            asm.ins("mov x2, x9");
            load_imm(asm, "x3", i64::from(sv.size));
            asm.ins("bl rt_map_get");
            // 第二个结果是 comma-ok 的 `bool`（单值位置时没有它）。
            if let Some(ok) = dsts.get(1) {
                let so = cx.slot(*ok);
                st_x(asm, "x0", so.off);
            }
        }
        RuntimeFn::MapSet => {
            let (Some(m), Some(k), Some(v)) = (args.first(), args.get(1), args.get(2)) else {
                return;
            };
            let (sm, sk, sv) = (cx.slot(*m), cx.slot(*k), cx.slot(*v));
            ld_x(asm, "x0", sm.off);
            addr(asm, sk.off);
            asm.ins("mov x1, x9");
            addr(asm, sv.off);
            asm.ins("mov x2, x9");
            asm.ins("bl rt_map_set");
        }
        RuntimeFn::MapDel => {
            let (Some(m), Some(k)) = (args.first(), args.get(1)) else {
                return;
            };
            let (sm, sk) = (cx.slot(*m), cx.slot(*k));
            ld_x(asm, "x0", sm.off);
            addr(asm, sk.off);
            asm.ins("mov x1, x9");
            asm.ins("bl rt_map_del");
        }
        RuntimeFn::MapIter => {
            let (Some(m), Some(idx)) = (args.first(), args.get(1)) else {
                return;
            };
            let (Some(kd), Some(vd), Some(nd)) = (dsts.first(), dsts.get(1), dsts.get(2)) else {
                return;
            };
            let (sm, si) = (cx.slot(*m), cx.slot(*idx));
            let (sk, sv, sn) = (cx.slot(*kd), cx.slot(*vd), cx.slot(*nd));
            // 同上：`map[int]byte` 的值只有 1 字节，槽有 8 字节。
            zero_slot(asm, sk.off, sk.size);
            zero_slot(asm, sv.off, sv.size);
            ld_x(asm, "x0", sm.off);
            ld_x(asm, "x1", si.off);
            addr(asm, sk.off);
            asm.ins("mov x2, x9");
            addr(asm, sv.off);
            asm.ins("mov x3, x9");
            asm.ins("bl rt_map_iter");
            st_x(asm, "x0", sn.off);
        }
    }
}

/// `[]T` 的元素类型（剥命名/别名）；不是切片 ⇒ `None`。
fn slice_elem(tt: &TypeTable, t: TypeId) -> Option<TypeId> {
    use rgoc_types::typ::Type;
    let mut cur = t;
    for _ in 0..64 {
        match tt.type_of(cur) {
            Type::Slice(e) => return Some(*e),
            Type::Named(id) => cur = tt.named_underlying(*id),
            Type::Alias(id) => cur = tt.alias_rhs(*id),
            _ => return None,
        }
    }
    None
}

/// `make([]T, len[, cap])` ⇒ `rt_makeslice(&dst, elem_desc, len, cap)`。
///
/// # `cap` 缺省就是 `len`
///
/// `build.rs` 对 `make([]T, n)` 只给 `len`（`cap` 是 `None`）—— 规范里
/// 「省略 cap ⇒ cap == len」。这里就地补上，**不**回头改 IR：
/// 「`None` 是什么意思」在 IR 层已经有约定（`new(T)` 也是两个 `None`），
/// 再给它一个含义会让同一个形状有两种解释。
fn makeslice(
    cx: &Ctx<'_>,
    dst: ValueId,
    typ: TypeId,
    len: Option<ValueId>,
    cap: Option<ValueId>,
    asm: &mut Asm,
) {
    let d = cx.slot(dst);
    let elem = slice_elem(cx.tt, typ).unwrap_or(typ);
    let desc = crate::module::desc_symbol(rgoc_meta::desc::type_hash(cx.tt, elem));
    addr(asm, d.off);
    asm.ins("mov x0, x9");
    load_addr(asm, "x1", &desc);
    match len {
        Some(v) => ld_x(asm, "x2", cx.slot(v).off),
        None => load_imm(asm, "x2", 0),
    }
    match cap {
        Some(v) => ld_x(asm, "x3", cx.slot(v).off),
        // 省略 cap ⇒ cap = len（`x2` 已经是 len）。
        None => asm.ins("mov x3, x2"),
    }
    asm.ins("bl rt_makeslice");
}

/// `s[lo:hi]` / `s[lo:hi:max]` ⇒ 新三字头（M9/Task 13）。
///
/// # 底座有两种形状
///
/// * **切片**：底座槽里就是 `{ptr,len,cap}`，字 0 是数据指针
/// * **数组**（`a[1:3]`）：底座是**地址**（`AddrOf`），字 0 不是数据指针
/// * **字符串**（`s[1:3]`）：两字 `{ptr,len}`，结果是**字符串**（不是切片）
///
/// 三者都「从字 0 取基址」，但 `cap` 的算法不同 —— 数组没有 cap 字段，
/// 它的 cap 是**长度常量**。
fn slice_expr(
    cx: &Ctx<'_>,
    dst: ValueId,
    base: ValueId,
    lo: Option<ValueId>,
    hi: Option<ValueId>,
    max: Option<ValueId>,
    asm: &mut Asm,
) {
    let d = cx.slot(dst);
    let b = cx.slot(base);
    let bt = cx.ty(base);
    let is_string = is_string_ty(cx.tt, bt);
    let arr_len = array_len_of_base(cx.tt, bt);
    let esz = elem_size_of(cx.tt, bt);

    // x10 = 基址（数据指针）
    ld_x(asm, "x10", b.off);
    // x11 = lo（缺省 0）
    match lo {
        Some(v) => ld_x(asm, "x11", cx.slot(v).off),
        None => load_imm(asm, "x11", 0),
    }
    // x12 = hi（缺省 = len）
    match hi {
        Some(v) => ld_x(asm, "x12", cx.slot(v).off),
        None => {
            if let Some(n) = arr_len {
                load_imm(asm, "x12", i64::from(n));
            } else {
                ld_x(asm, "x12", b.off + 8);
            }
        }
    }
    // 数据指针 += lo * esz
    load_imm(asm, "x13", i64::from(esz));
    asm.ins("madd x10, x11, x13, x10");
    // len = hi - lo
    asm.ins("sub x14, x12, x11");
    st_x(asm, "x10", d.off);
    st_x(asm, "x14", d.off + 8);
    if is_string {
        // 字符串只有两字：没有 cap 要写。
        return;
    }
    // cap = max - lo（给了 max）；否则 cap(s) - lo；数组则是 len(arr) - lo
    match max {
        Some(v) => {
            ld_x(asm, "x15", cx.slot(v).off);
            asm.ins("sub x15, x15, x11");
        }
        None => {
            if let Some(n) = arr_len {
                load_imm(asm, "x15", i64::from(n));
                asm.ins("sub x15, x15, x11");
            } else {
                ld_x(asm, "x15", b.off + 16);
                asm.ins("sub x15, x15, x11");
            }
        }
    }
    st_x(asm, "x15", d.off + 16);
}

/// `new(T)`：`rt_arena_alloc(size_of(*T), align)`，结果存进槽。
///
/// 零大小类型（`new([0]int)`）也走这里 —— runtime 对 `size == 0` 返回**固定哨兵**
/// （`zerosize.go` 要求两次 `new` 拿到同一地址，`C5` §7.2）。
fn new_alloc(cx: &Ctx<'_>, dst: ValueId, ptr_ty: TypeId, asm: &mut Asm) {
    use rgoc_types::layout::layout;
    use rgoc_types::typ::Type;
    let d = cx.slot(dst);
    // 剥到指针，取 pointee 的布局。
    let mut cur = ptr_ty;
    let mut elem = ptr_ty;
    for _ in 0..64 {
        match cx.tt.type_of(cur) {
            Type::Pointer(e) => {
                elem = *e;
                break;
            }
            Type::Named(id) => cur = cx.tt.named_underlying(*id),
            Type::Alias(id) => cur = cx.tt.alias_rhs(*id),
            _ => break,
        }
    }
    let _ = layout(cx.tt, elem);
    // M9/Task 6 起走 `rt_alloc(desc, need_zero)`：分配器按 desc 记账（大小、
    // 有没有指针），M10 的扫描器靠对象头里的 desc 找到 gcdata。
    //
    // `need_zero` 由**编译期**算（`rgoc_meta::needs_zero`），不是运行期读
    // desc 的 flags —— 两份信息同源，但走编译期能让它出现在 `.s` 里，
    // 于是「判定对不对」有可观测的判据（`mem_alloc::含指针的类型要清零`）。
    let desc = crate::module::desc_symbol(rgoc_meta::desc::type_hash(cx.tt, elem));
    load_addr(asm, "x0", &desc);
    load_imm(
        asm,
        "x1",
        i64::from(rgoc_meta::desc::needs_zero(cx.tt, elem)),
    );
    asm.ins("bl rt_alloc");
    st_x(asm, "x0", d.off);
}

/// 复合字面量：先清零，再按元素落位。
fn composite(cx: &Ctx<'_>, dst: ValueId, typ: TypeId, items: &[ValueId], asm: &mut Asm) {
    let d = cx.slot(dst);
    // `[]T{...}`：先 `make` 出底层数组，再逐元素写进去（M9/Task 13）。
    //
    // 与数组/结构体**完全不同**：后两者的元素就长在值里，而切片的元素在
    // 另一个对象上 —— 所以这里不能「清零 dst 再按偏移写」。
    if let Some(elem) = slice_elem(cx.tt, typ) {
        let n = items.len();
        let desc = crate::module::desc_symbol(rgoc_meta::desc::type_hash(cx.tt, elem));
        let esz = rgoc_types::layout::layout(cx.tt, elem).map_or(8, |l| l.size);
        addr(asm, d.off);
        asm.ins("mov x0, x9");
        load_addr(asm, "x1", &desc);
        load_imm(asm, "x2", i64::try_from(n).unwrap_or(0));
        asm.ins("mov x3, x2");
        asm.ins("bl rt_makeslice");
        // 逐元素：dst.ptr + i*esz
        for (k, item) in items.iter().enumerate() {
            let s = cx.slot(*item);
            ld_x(asm, "x10", d.off);
            load_imm(asm, "x12", i64::try_from(k).unwrap_or(0) * i64::from(esz));
            asm.ins("add x10, x10, x12");
            if s.size <= 8 {
                ld_x(asm, "x11", s.off);
                asm.ins("str x11, [x10]");
            } else {
                asm.ins("mov x0, x10");
                addr(asm, s.off);
                asm.ins("mov x1, x9");
                load_imm(asm, "x2", i64::from(s.size));
                asm.ins("bl rt_memcpy");
            }
        }
        return;
    }
    // 清零（`Composite` 的语义是「没写的元素为零值」）。
    let words = d.size.div_ceil(8);
    addr(asm, d.off);
    for k in 0..words {
        asm.ins(&format!("str xzr, [x9, #{}]", k * 8));
    }
    // 元素偏移：数组按下标，结构体按字段。
    for (k, item) in items.iter().enumerate() {
        let off = composite_item_off(cx.tt, typ, k);
        let s = cx.slot(*item);
        if s.size <= 8 {
            ld_x(asm, "x10", s.off);
            st_x(asm, "x10", d.off + off as i32);
        } else {
            memcpy(asm, d.off + off as i32, s.off, s.size);
        }
    }
}

/// 复合字面量第 `k` 项的字节偏移。
fn composite_item_off(tt: &TypeTable, typ: TypeId, k: usize) -> u32 {
    use rgoc_types::layout::{field_offset, layout};
    use rgoc_types::typ::Type;
    let mut cur = typ;
    for _ in 0..64 {
        match tt.type_of(cur) {
            Type::Named(id) => cur = tt.named_underlying(*id),
            Type::Alias(id) => cur = tt.alias_rhs(*id),
            _ => break,
        }
    }
    match tt.type_of(cur) {
        Type::Array { elem, .. } => {
            let esz = layout(tt, *elem).map_or(8, |l| l.size);
            esz * u32::try_from(k).unwrap_or(0)
        }
        Type::Struct(sid) => field_offset(tt, *sid, k).unwrap_or(0),
        _ => 0,
    }
}

/// 读内存（按目标宽度）。
fn load(cx: &Ctx<'_>, dst: ValueId, addr_v: ValueId, typ: TypeId, asm: &mut Asm) {
    let (d, a) = (cx.slot(dst), cx.slot(addr_v));
    if d.size > 8 {
        // 聚合：`memcpy(dst, addr, size)`。
        addr(asm, d.off);
        asm.ins("mov x0, x9");
        ld_x(asm, "x1", a.off);
        load_imm(asm, "x2", i64::from(d.size));
        asm.ins("bl rt_memcpy");
        return;
    }
    ld_x(asm, "x10", a.off);
    match cx.width(typ) {
        1 => asm.ins("ldrb w11, [x10]"),
        2 => asm.ins("ldrh w11, [x10]"),
        4 => asm.ins("ldr w11, [x10]"),
        _ => asm.ins("ldr x11, [x10]"),
    }
    // 窄类型要按符号/零扩展（槽里恒为 64 位）。
    //
    // ⚠️ 扩展宽度必须是**该类型自己的宽度**，不是 `sxtw`（32 位）。
    // 实测 `intcvt.go`：`int8(-128)` 存在 `.data` 里是 `0x80`，`ldrb` 零扩展成
    // `w11 = 0x80`，再 `sxtw` 得到 **128** 而不是 `-128`。`sxtw` 只从第 31 位
    // 复制符号位，对 1/2 字节的类型等于「符号位在 8/16 位、扩展点在 32 位」。
    if cx.width(typ) < 8 {
        let unsigned = cx.is_unsigned(typ) || basic_of(cx.tt, typ) == Some(BasicId::Bool);
        let ins = match (cx.width(typ), unsigned) {
            (1, true) => "uxtb x11, w11",
            (1, false) => "sxtb x11, w11",
            (2, true) => "uxth x11, w11",
            (2, false) => "sxth x11, w11",
            (_, true) => "uxtw x11, w11",
            (_, false) => "sxtw x11, w11",
        };
        asm.ins(ins);
    }
    st_x(asm, "x11", d.off);
}

/// 写内存（按源宽度）。
fn store(cx: &Ctx<'_>, addr_v: ValueId, val: ValueId, asm: &mut Asm) {
    let (a, v) = (cx.slot(addr_v), cx.slot(val));
    if v.size > 8 {
        ld_x(asm, "x0", a.off);
        addr(asm, v.off);
        asm.ins("mov x1, x9");
        load_imm(asm, "x2", i64::from(v.size));
        asm.ins("bl rt_memcpy");
        return;
    }
    ld_x(asm, "x10", a.off);
    ld_x(asm, "x11", v.off);
    match cx.width(cx.ty(val)) {
        1 => asm.ins("strb w11, [x10]"),
        2 => asm.ins("strh w11, [x10]"),
        4 => asm.ins("str w11, [x10]"),
        _ => asm.ins("str x11, [x10]"),
    }
}

/// 调用。
fn call(
    cx: &Ctx<'_>,
    dsts: &[ValueId],
    callee: &CalleeRef,
    args: &[ValueId],
    dots: bool,
    asm: &mut Asm,
) -> Result<(), CgCode> {
    match callee {
        CalleeRef::Builtin(b) => builtin(cx, *b, args, dsts, dots, asm),
        CalleeRef::Runtime(f) => {
            runtime_call(cx, *f, args, dsts, asm);
            Ok(())
        }
        CalleeRef::Func(fid) => {
            if ctx_abi(cx, *fid) {
                // 被调方带隐式 ctx（`M9/Task 40`）：实参从 `x1` 起，`x0` 传 0
                // （直接调用没有 funcval）。
                pass_args_ctx(cx, Some(*fid), args, asm);
                asm.ins("mov x0, xzr");
            } else {
                pass_args(cx, Some(*fid), args, asm);
            }
            asm.ins(&format!("bl {}", cx.tt_fn_name(*fid)));
            take_results(cx, dsts, asm);
            Ok(())
        }
        CalleeRef::Method { recv, func } => {
            // ⚠️ **接收者是第 0 个实参**（`Func.params[0]`，见 `build.rs::call` 的注释）。
            // `CalleeRef::Method` 把它单独放一个字段，所以这里必须自己拼回实参表。
            //
            // 漏掉它 ⇒ 被调方从 `x0` 读到一个没写过的寄存器，`t.x` 落在野地址上，
            // 表现为 `Segmentation fault`。这个错在「不访问接收者」的方法上完全看不出来，
            // 所以 `func.go`（`(t *T) m10`）是唯一能抓到它的语料。
            let mut all: Vec<ValueId> = Vec::with_capacity(args.len() + 1);
            all.push(*recv);
            all.extend_from_slice(args);
            pass_args(cx, Some(*func), &all, asm);
            asm.ins(&format!("bl {}", cx.tt_fn_name(*func)));
            take_results(cx, dsts, asm);
            Ok(())
        }
        // 接口方法调用（`M9/Task 32`）：`fn = tab.fn[slot]`，第 0 个实参是 `data`。
        CalleeRef::Iface { recv, slot } => {
            let r = cx.slot(*recv);
            // 形参表 = [data] + 用户实参。`data` 的类型用 `Invalid` 占位：
            // `place_args` 只看 class/size，`Invalid` 落 `Int`（8 字节、1 个 `x`），
            // 正是指针的样子 —— 而本 crate 没有可变 `TypeTable`，造不出 `*T`。
            let mut want: Vec<TypeId> = Vec::with_capacity(args.len() + 1);
            want.push(TypeId::from_index(0));
            want.extend(args.iter().map(|a| cx.ty(*a)));
            let (locs, _) = place_args(cx.tt, &want);
            match locs.first().copied() {
                Some(ArgLoc::X { reg, .. }) => ld_x(asm, &format!("x{reg}"), r.off + 8),
                Some(ArgLoc::Stack { off }) => {
                    ld_x(asm, "x10", r.off + 8);
                    asm.ins(&format!("str x10, [sp, #{off}]"));
                }
                // `data` 是指针，永远落 `x` 或栈 —— 落到 `v` 说明布局算错了
                Some(ArgLoc::V { .. }) | None => unreachable!("接口 data 不是浮点类"),
            }
            pass_args_from(cx, &want, args, 1, asm);
            // `fn` **最后**取：上面装实参用过 `x9`/`x10`。`x16` 是 IP0（链接器
            // 用来做 veneer 的临时寄存器），被调方不会指望它保活。
            ld_x(asm, "x9", r.off);
            asm.ins(&format!("ldr x16, [x9, #{}]", 16 + 8 * slot));
            asm.ins("blr x16");
            take_results(cx, dsts, asm);
            Ok(())
        }
        // 间接调用（`M9/Task 37`）：funcval 在 `x0`，`fn` 在它的偏移 0。
        //
        // 调用点**不知道**静态目标，所以约定是硬的：凡是被当作值用过的函数
        // 都带 ctx ABI（`build()` 的收尾扫描保证这一点）。
        CalleeRef::Indirect(v) => {
            let fv = cx.slot(*v);
            pass_args_ctx(cx, None, args, asm);
            ld_x(asm, "x0", fv.off);
            asm.ins("ldr x16, [x0]");
            asm.ins("blr x16");
            take_results(cx, dsts, asm);
            Ok(())
        }
    }
}

impl Ctx<'_> {
    /// 被调函数的汇编符号名。
    ///
    /// # 为什么不能直接用 `FuncId`
    ///
    /// `FuncId` 是 HIR 的下标，不是名字；`_start` 要 `bl main`，所以至少 `main` 与
    /// `init$k` 必须是稳定符号。M7 统一用 `fn<id>`，`main` 与 `init$k` 由模块级
    /// 发射器（Task 49）单独登记别名。
    fn tt_fn_name(&self, id: rgoc_hir::FuncId) -> String {
        fn_sym(id)
    }
}

/// 装实参（`C5` §6.1：`x`/`v` 两个序列独立计数，装不下的走 `sp`）。
///
/// # 类别必须来自**被调方**的形参，不是实参
///
/// `R-13-3`：同一个值在不同上下文里可以有不同的表示，而寄存器分配只由
/// **形参类型**决定。实测 `cplx1.go`：`booltest(5+6i, true)` 的实参是
/// **无类型复数**（默认类型 `complex128` ⇒ 走 `d0`），形参却是 `complex64`
/// （⇒ 期望 `s0/s1`）。按实参走，被调方读到 `d0` 的低 32 位 —— 也就是 `0`。
///
/// # 落位表由 `abi::place_args` 给
///
/// 调用方与被调方（`bind_params`）必须用**同一个函数**算同一张表。各写一遍就是
/// `string` 的两个字与 `complex64` 的两个 `s` 会错位 —— 而错位的表现是
/// 「参数值看起来对、比较全错」，最难查。
fn pass_args(cx: &Ctx<'_>, callee: Option<rgoc_hir::FuncId>, args: &[ValueId], asm: &mut Asm) {
    let arg_tys: Vec<TypeId> = args.iter().map(|a| cx.ty(*a)).collect();
    let want = callee_param_tys(&arg_tys, callee, cx.siblings);
    pass_args_from(cx, &want, args, 0, asm);
}

/// 被调方带隐式 ctx 吗（`M9/Task 40`）。
///
/// 静态目标查 `siblings`；**间接调用**没有静态目标 ⇒ 一律按 ctx ABI 处理
/// （凡是被当作值用过的函数都带它，见 `build()` 的收尾扫描）。
fn ctx_abi(cx: &Ctx<'_>, fid: rgoc_hir::FuncId) -> bool {
    cx.siblings.get(&fid).is_some_and(|f| f.ctx_abi)
}

/// 按 ctx ABI 装实参：`x0` 留给 ctx，用户实参从 `x1` 起。
fn pass_args_ctx(cx: &Ctx<'_>, callee: Option<rgoc_hir::FuncId>, args: &[ValueId], asm: &mut Asm) {
    let arg_tys: Vec<TypeId> = args.iter().map(|a| cx.ty(*a)).collect();
    let mut want: Vec<TypeId> = vec![TypeId::from_index(0)];
    want.extend(callee_param_tys(&arg_tys, callee, cx.siblings));
    pass_args_from(cx, &want, args, 1, asm);
}

/// 按**给定的形参类型表**装实参；`args[0]` 落在 `locs[first]`。
///
/// # 为什么需要 `first`
///
/// 接口派发的第 0 个实参是 `data`（不由调用者给，直接从接口值的第 2 个字取），
/// 而其余实参的位置要**排在它后面**。传 `first = 1` 就把这件事说清楚了 ——
/// 比在 `pass_args` 里加一个 `Option<data>` 更少分支。
fn pass_args_from(cx: &Ctx<'_>, want: &[TypeId], args: &[ValueId], first: usize, asm: &mut Asm) {
    let (locs, _) = place_args(cx.tt, want);
    for (i, a) in args.iter().enumerate() {
        let i = i + first;
        let s = cx.slot(*a);
        let acls = cx.class(*a);
        let wty = want.get(i).copied().unwrap_or_else(|| cx.ty(*a));
        let wcls = slot_of(cx.tt, wty).map_or(SlotClass::Int, |x| x.class);
        let wsize = slot_of(cx.tt, wty).map_or(s.size, |x| x.size);
        let loc = locs.get(i).copied().unwrap_or(ArgLoc::X { reg: 0, n: 1 });
        match loc {
            ArgLoc::X { reg, n } => match wcls {
                // 聚合按指针传：把**本帧槽的地址**交出去（被调方入口会拷贝）。
                //
                // ⚠️ 但实参**自己就是指针**时（方法表达式 `(*T).M(&t)`：形参是
                // `T`、实参是 `*T`）不能再取地址 —— 那会传一个 `**T` 进去，
                // 被调方 `len(t)` 读到的是指针的位（实测 `method3.go`：
                // `(*T).Len 187650284476264`）。
                SlotClass::Agg if acls != SlotClass::Agg => {
                    ld_x(asm, &format!("x{reg}"), s.off);
                }
                SlotClass::Agg => {
                    addr(asm, s.off);
                    asm.ins(&format!("mov x{reg}, x9"));
                }
                _ => {
                    ld_x(asm, &format!("x{reg}"), s.off);
                    if n > 1 {
                        ld_x(asm, &format!("x{}", reg + 1), s.off + 8);
                    }
                }
            },
            ArgLoc::V { reg, .. } => match wcls {
                SlotClass::Complex64 => {
                    if acls == SlotClass::Complex128 {
                        // 实参可能是 `complex128`（无类型复数的默认类型）—— 必须**降精度**。
                        ld_d(asm, "d10", s.off);
                        ld_d(asm, "d11", s.off + 8);
                        asm.ins(&format!("fcvt s{reg}, d10"));
                        asm.ins(&format!("fcvt s{}, d11", reg + 1));
                    } else {
                        ld_s(asm, &format!("s{reg}"), s.off);
                        ld_s(asm, &format!("s{}", reg + 1), s.off + 4);
                    }
                }
                SlotClass::Complex128 => {
                    if acls == SlotClass::Complex64 {
                        ld_s(asm, "s10", s.off);
                        ld_s(asm, "s11", s.off + 4);
                        asm.ins(&format!("fcvt d{reg}, s10"));
                        asm.ins(&format!("fcvt d{}, s11", reg + 1));
                    } else {
                        ld_d(asm, &format!("d{reg}"), s.off);
                        ld_d(asm, &format!("d{}", reg + 1), s.off + 8);
                    }
                }
                _ => match (cx.is_f32(cx.ty(*a)), cx.is_f32(wty)) {
                    (true, true) => ld_s(asm, &format!("s{reg}"), s.off),
                    (false, false) => ld_d(asm, &format!("d{reg}"), s.off),
                    (true, false) => {
                        ld_s(asm, "s10", s.off);
                        asm.ins(&format!("fcvt d{reg}, s10"));
                    }
                    (false, true) => {
                        ld_d(asm, "d10", s.off);
                        asm.ins(&format!("fcvt s{reg}, d10"));
                    }
                },
            },
            ArgLoc::Stack { off } => {
                // 写到帧**最底部**的 outgoing 区（`Frame::outgoing` 已经算进帧大小）。
                match wcls {
                    SlotClass::Agg => {
                        addr(asm, s.off);
                        asm.ins(&format!("str x9, [sp, #{off}]"));
                    }
                    _ => {
                        let words = if wsize > 8 { 2 } else { 1 };
                        for k in 0..words {
                            ld_x(asm, "x10", s.off + (k as i32) * 8);
                            asm.ins(&format!("str x10, [sp, #{}]", off + k * 8));
                        }
                    }
                }
            }
        }
    }
}

/// 取回结果（`x0..` / `v0..`）。
fn take_results(cx: &Ctx<'_>, dsts: &[ValueId], asm: &mut Asm) {
    let mut xi = 0usize;
    let mut vi = 0usize;
    for d in dsts {
        let s = cx.slot(*d);
        match cx.class(*d) {
            SlotClass::Complex64 => {
                st_s(asm, &format!("s{vi}"), s.off);
                st_s(asm, &format!("s{}", vi + 1), s.off + 4);
                vi += 2;
            }
            SlotClass::Float => {
                if cx.is_f32(cx.ty(*d)) {
                    st_s(asm, &format!("s{vi}"), s.off);
                } else {
                    st_d(asm, &format!("d{vi}"), s.off);
                }
                vi += 1;
            }
            SlotClass::Complex128 => {
                st_d(asm, &format!("d{vi}"), s.off);
                st_d(asm, &format!("d{}", vi + 1), s.off + 8);
                vi += 2;
            }
            _ => {
                if s.size > 8 {
                    st_x(asm, &format!("x{xi}"), s.off);
                    st_x(asm, &format!("x{}", xi + 1), s.off + 8);
                    xi += 2;
                } else {
                    st_x(asm, &format!("x{xi}"), s.off);
                    xi += 1;
                }
            }
        }
    }
}

/// 内建调用。
fn builtin(
    cx: &Ctx<'_>,
    b: BuiltinKind,
    args: &[ValueId],
    dsts: &[ValueId],
    dots: bool,
    asm: &mut Asm,
) -> Result<(), CgCode> {
    match b {
        BuiltinKind::Print | BuiltinKind::Println => {
            let is_ln = b == BuiltinKind::Println;
            for (i, a) in args.iter().enumerate() {
                if is_ln && i > 0 {
                    asm.ins("bl rt_print_space");
                }
                print_value(cx, *a, asm);
            }
            if is_ln {
                asm.ins("bl rt_print_nl");
            }
            Ok(())
        }
        BuiltinKind::Panic => {
            if let Some(a) = args.first() {
                panic_value(cx, *a, asm);
            }
            Ok(())
        }
        // `len` / `cap`：字符串读第二个字；数组是**常量**（长度在类型里）。
        // `copy`/`delete`/`clear`/`close`/`recover` 在 M6 的拒绝面里，走不到这里。
        BuiltinKind::Len | BuiltinKind::Cap => {
            let (Some(a), Some(d)) = (args.first(), dsts.first()) else {
                return Ok(());
            };
            let t = cx.ty(*a);
            if is_string_ty(cx.tt, t) {
                let (sa, sd) = (cx.slot(*a), cx.slot(*d));
                ld_x(asm, "x10", sa.off + 8);
                st_x(asm, "x10", sd.off);
            } else if let Some(n) = rgoc_types::layout::array_len_of(cx.tt, t) {
                let sd = cx.slot(*d);
                load_imm(asm, "x10", i64::from(n));
                st_x(asm, "x10", sd.off);
            } else if slice_elem(cx.tt, t).is_some() {
                // 切片头：字 1 = len、字 2 = cap（M9/Task 11）。
                let (sa, sd) = (cx.slot(*a), cx.slot(*d));
                let off = if b == BuiltinKind::Cap { 16 } else { 8 };
                ld_x(asm, "x10", sa.off + off);
                st_x(asm, "x10", sd.off);
            } else if map_kv(cx.tt, t).is_some() {
                // `len(m)`：`MapObj.count` 在 +0（M9/Task 19）。`cap` 对 map 不可见，
                // 检查器已经拒了 `cap(m)`，走不到那一支。
                let (sa, sd) = (cx.slot(*a), cx.slot(*d));
                ld_x(asm, "x10", sa.off);
                // nil map 的 `len` 是 0（`count` 不存在）。
                let done = format!(".Lmap_len{}", asm.fresh_label());
                let zero = format!(".Lmap_zero{}", asm.fresh_label());
                asm.ins(&format!("cbz x10, {zero}"));
                asm.ins("ldr x11, [x10]");
                asm.ins(&format!("b {done}"));
                asm.label(&zero);
                asm.ins("mov x11, xzr");
                asm.label(&done);
                st_x(asm, "x11", sd.off);
            }
            Ok(())
        }
        // `complex(re, im)`：两个分量并排放（`complex64` 是 4+4）。
        BuiltinKind::Complex => {
            let (Some(re), Some(im), Some(d)) = (args.first(), args.get(1), dsts.first()) else {
                return Ok(());
            };
            let (sr, si, sd) = (cx.slot(*re), cx.slot(*im), cx.slot(*d));
            let step = if sd.size == 8 { 4 } else { 8 };
            if step == 4 {
                ld_s(asm, "s10", sr.off);
                ld_s(asm, "s11", si.off);
                st_s(asm, "s10", sd.off);
                st_s(asm, "s11", sd.off + 4);
            } else {
                ld_d(asm, "d10", sr.off);
                ld_d(asm, "d11", si.off);
                st_d(asm, "d10", sd.off);
                st_d(asm, "d11", sd.off + 8);
            }
            Ok(())
        }
        // `real(c)` / `imag(c)`：取复数的实部 / 虚部。
        BuiltinKind::Real | BuiltinKind::Imag => {
            let (Some(c), Some(d)) = (args.first(), dsts.first()) else {
                return Ok(());
            };
            let (sc, sd) = (cx.slot(*c), cx.slot(*d));
            let off = if b == BuiltinKind::Imag {
                if sc.size == 8 { 4 } else { 8 }
            } else {
                0
            };
            if cx.is_f32(cx.ty(*d)) {
                ld_s(asm, "s10", sc.off + off);
                st_s(asm, "s10", sd.off);
            } else {
                ld_d(asm, "d10", sc.off + off);
                st_d(asm, "d10", sd.off);
            }
            Ok(())
        }
        // `min` / `max`：两元；整数走 `csel`，浮点走 `fmin/fmax`。
        BuiltinKind::Min | BuiltinKind::Max => {
            let (Some(a), Some(b2), Some(d)) = (args.first(), args.get(1), dsts.first()) else {
                return Ok(());
            };
            let (sa, sb, sd) = (cx.slot(*a), cx.slot(*b2), cx.slot(*d));
            if cx.class(*d) == SlotClass::Float {
                let is32 = cx.is_f32(cx.ty(*d));
                let ins = if b == BuiltinKind::Min {
                    "fmin"
                } else {
                    "fmax"
                };
                if is32 {
                    ld_s(asm, "s10", sa.off);
                    ld_s(asm, "s11", sb.off);
                    asm.ins(&format!("{ins} s12, s10, s11"));
                    st_s(asm, "s12", sd.off);
                } else {
                    ld_d(asm, "d10", sa.off);
                    ld_d(asm, "d11", sb.off);
                    asm.ins(&format!("{ins} d12, d10, d11"));
                    st_d(asm, "d12", sd.off);
                }
            } else {
                let unsigned = cx.is_unsigned(cx.ty(*d));
                ld_x(asm, "x10", sa.off);
                ld_x(asm, "x11", sb.off);
                asm.ins("cmp x10, x11");
                let cond = match (b, unsigned) {
                    (BuiltinKind::Min, false) => "lt",
                    (BuiltinKind::Min, true) => "lo",
                    (_, false) => "gt",
                    (_, true) => "hi",
                };
                asm.ins(&format!("csel x12, x10, x11, {cond}"));
                st_x(asm, "x12", sd.off);
            }
            Ok(())
        }
        // `append(s, v)` / `append(s, xs...)`（M9/Task 12）。
        //
        // 两个入口而不是一个：`append(s, v)` 要把**一个元素的值**写进去
        // （`val` 是它的地址，多字元素按 esz 整块拷），`append(s, xs...)` 要
        // 从另一个切片拷 `extra` 个。合成一个入口就得在 isel 里拼
        // 「源地址 + 元素个数」两个不同形状的实参，而那段寄存器调度
        // 在 `.s` 上完全看不出来 —— 分成两个 C 函数，语义各写在 C 里。
        BuiltinKind::Append => {
            let (Some(d), Some(src)) = (dsts.first(), args.first()) else {
                return Ok(());
            };
            let st = cx.ty(*src);
            let Some(elem) = slice_elem(cx.tt, st) else {
                return Ok(());
            };
            let desc = crate::module::desc_symbol(rgoc_meta::desc::type_hash(cx.tt, elem));
            let (sd, ss) = (cx.slot(*d), cx.slot(*src));
            addr(asm, sd.off);
            asm.ins("mov x0, x9");
            ld_x(asm, "x1", ss.off);
            ld_x(asm, "x2", ss.off + 8);
            ld_x(asm, "x3", ss.off + 16);
            load_addr(asm, "x4", &desc);
            if dots {
                // x5 = extra（= len(xs)）、x6 = xs.ptr
                let Some(xs) = args.get(1) else {
                    return Ok(());
                };
                let sx = cx.slot(*xs);
                ld_x(asm, "x5", sx.off + 8);
                ld_x(asm, "x6", sx.off);
                asm.ins("bl rt_append_dots");
            } else {
                // 多个元素（`append(s, 1, 2, 3)`）**不是**一次调用能做的：
                // 展开成逐个 `rt_append_one`。`dots == false && args.len() > 2`
                // 就是这种形态。
                for (k, v) in args.iter().skip(1).enumerate() {
                    if k > 0 {
                        // 第二趟起：从**新头**重新读（len/cap 都变了）
                        addr(asm, sd.off);
                        asm.ins("mov x0, x9");
                        ld_x(asm, "x1", sd.off);
                        ld_x(asm, "x2", sd.off + 8);
                        ld_x(asm, "x3", sd.off + 16);
                        load_addr(asm, "x4", &desc);
                    }
                    let sv = cx.slot(*v);
                    addr(asm, sv.off);
                    asm.ins("mov x5, x9");
                    asm.ins("bl rt_append_one");
                }
            }
            Ok(())
        }
        // 其余内建（`copy`/`delete`/`clear`/`close`/`recover`）在 M6 的
        // 拒绝面里，走不到这里；真走到了说明上游漏判 —— 宁可少发也不发错。
        _ => Ok(()),
    }
}

/// 打印一个值（按静态类型选 runtime 入口）。
fn print_value(cx: &Ctx<'_>, v: ValueId, asm: &mut Asm) {
    let s = cx.slot(v);
    let t = cx.ty(v);
    match cx.class(v) {
        SlotClass::Float => {
            if cx.is_f32(t) {
                ld_s(asm, "s0", s.off);
            } else {
                ld_d(asm, "d0", s.off);
            }
            asm.ins(&format!("mov x0, #{}", u8::from(cx.is_f32(t))));
            asm.ins("bl rt_print_f64");
        }
        SlotClass::Complex128 => {
            ld_d(asm, "d0", s.off);
            ld_d(asm, "d1", s.off + 8);
            asm.ins("bl rt_print_complex");
        }
        SlotClass::Complex64 => {
            ld_s(asm, "s0", s.off);
            ld_s(asm, "s1", s.off + 4);
            asm.ins("bl rt_print_complex64");
        }
        SlotClass::Int => {
            if s.size > 8 {
                // `string`
                ld_x(asm, "x0", s.off);
                ld_x(asm, "x1", s.off + 8);
                asm.ins("bl rt_print_str");
            } else if basic_of(cx.tt, t).is_some_and(rgoc_const::basic::BasicId::is_bool) {
                ld_x(asm, "x0", s.off);
                asm.ins("bl rt_print_bool");
            } else if cx.is_unsigned(t) {
                ld_x(asm, "x0", s.off);
                asm.ins("bl rt_print_u64");
            } else {
                ld_x(asm, "x0", s.off);
                asm.ins("bl rt_print_i64");
            }
        }
        SlotClass::Agg => {}
    }
}

/// `panic(v)`：写一行到 stderr 后 `exit(2)`。
fn panic_value(cx: &Ctx<'_>, v: ValueId, asm: &mut Asm) {
    let s = cx.slot(v);
    match cx.class(v) {
        // `panic(1.5)` 必须打成 `1.5`，不是 `4609434218613702656`。
        SlotClass::Float => {
            let is32 = cx.is_f32(cx.ty(v));
            if is32 {
                ld_s(asm, "s0", s.off);
            } else {
                ld_d(asm, "d0", s.off);
            }
            asm.ins(&format!("mov x0, #{}", u8::from(is32)));
            asm.ins("bl rt_panic_f64");
        }
        SlotClass::Complex128 => {
            ld_d(asm, "d0", s.off);
            ld_d(asm, "d1", s.off + 8);
            asm.ins("bl rt_panic_complex");
        }
        // `complex64` 没有单独的 panic 入口：升到 `complex128` 再打（值相同）。
        SlotClass::Complex64 => {
            ld_s(asm, "s0", s.off);
            ld_s(asm, "s1", s.off + 4);
            asm.ins("fcvt d0, s0");
            asm.ins("fcvt d1, s1");
            asm.ins("bl rt_panic_complex");
        }
        _ => {
            if s.size > 8 {
                ld_x(asm, "x0", s.off);
                ld_x(asm, "x1", s.off + 8);
                asm.ins("bl rt_panic_str");
            } else {
                ld_x(asm, "x0", s.off);
                asm.ins("bl rt_panic_i64");
            }
        }
    }
}

/// 终止符。
fn term(cx: &Ctx<'_>, blk: &Block, asm: &mut Asm) -> Result<(), CgCode> {
    match &blk.term {
        Term::Goto { to, args } => {
            edge_copy(cx, *to, args, asm);
            asm.ins(&format!("b {}", block_label(&fn_sym(cx.f.src), to.index())));
        }
        Term::If { cond, then_, else_ } => {
            let c = cx.slot(*cond);
            let l = format!("{}.if{}", fn_sym(cx.f.src), asm.fresh_label());
            ld_x(asm, "x10", c.off);
            asm.ins(&format!("cbnz x10, {l}"));
            edge_copy(cx, else_.0, &else_.1, asm);
            asm.ins(&format!(
                "b {}",
                block_label(&fn_sym(cx.f.src), else_.0.index())
            ));
            asm.label(&l);
            edge_copy(cx, then_.0, &then_.1, asm);
            asm.ins(&format!(
                "b {}",
                block_label(&fn_sym(cx.f.src), then_.0.index())
            ));
        }
        Term::Return(vals) => {
            return_values(cx, vals, asm);
            epilogue(cx.fr, asm);
        }
        Term::Panic(v) => {
            panic_value(cx, *v, asm);
            asm.ins("brk #1");
        }
        Term::Unreachable => {
            asm.ins("brk #1");
        }
    }
    Ok(())
}

/// 边上的并行拷贝（`D-M7-6`）。
fn edge_copy(cx: &Ctx<'_>, to: rgoc_ssa::ir::BlockId, args: &[ValueId], asm: &mut Asm) {
    let Some(target) = cx.f.blocks.get(to.index()) else {
        return;
    };
    if target.params.len() != args.len() {
        // `V7` 保证相等；不等说明上游违约，宁可少发也不要错发。
        return;
    }
    let tmp = cx.fr.tmp_base.unwrap_or(-8);
    // ⚠️ 一个参数可能有**多个字**（`string` 是 16 字节）。
    //
    // 按「一个参数 = 一条 8 字节搬运」发，`string` 只搬了指针、长度留在原地 ——
    // 于是下一块拿到的 `len` 是上一个值的残留。实测 `simpswitch.go`：
    // `switch` 的每个 case 都是一条边，`r += …` 在 case 里 ⇒ 拼接时拿一个
    // 巨大的 `len` 去分配 ⇒ `panic: out of arena`。
    //
    // 展开成**按字**的对是安全的：块参数是互不相同的槽，每个字只属于一个参数。
    let mut words: Vec<(i32, i32)> = Vec::with_capacity(target.params.len());
    for (p, a) in target.params.iter().zip(args.iter()) {
        let size = cx.slot(*p).size.max(cx.slot(*a).size);
        for k in 0..size.div_ceil(8) {
            let k = i32::try_from(k).unwrap_or(0) * 8;
            words.push((cx.slot(*p).off + k, cx.slot(*a).off + k));
        }
    }
    for m in parallel_copy::schedule(&words, tmp) {
        ld_x(asm, "x10", m.src);
        st_x(asm, "x10", m.dst);
    }
}

/// 返回值装进 `x0..` / `v0..`。
fn return_values(cx: &Ctx<'_>, vals: &[ValueId], asm: &mut Asm) {
    let mut xi = 0usize;
    let mut vi = 0usize;
    for v in vals {
        let s = cx.slot(*v);
        match cx.class(*v) {
            SlotClass::Complex64 => {
                ld_s(asm, &format!("s{vi}"), s.off);
                ld_s(asm, &format!("s{}", vi + 1), s.off + 4);
                vi += 2;
            }
            SlotClass::Float => {
                if cx.is_f32(cx.ty(*v)) {
                    ld_s(asm, &format!("s{vi}"), s.off);
                } else {
                    ld_d(asm, &format!("d{vi}"), s.off);
                }
                vi += 1;
            }
            SlotClass::Complex128 => {
                ld_d(asm, &format!("d{vi}"), s.off);
                ld_d(asm, &format!("d{}", vi + 1), s.off + 8);
                vi += 2;
            }
            _ => {
                if s.size > 8 {
                    ld_x(asm, &format!("x{xi}"), s.off);
                    ld_x(asm, &format!("x{}", xi + 1), s.off + 8);
                    xi += 2;
                } else {
                    ld_x(asm, &format!("x{xi}"), s.off);
                    xi += 1;
                }
            }
        }
    }
}
