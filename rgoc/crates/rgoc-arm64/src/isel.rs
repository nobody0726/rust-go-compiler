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
use rgoc_ssa::ir::{Block, CalleeRef, Instr, SsaFunc, Term, ValueId};
use rgoc_types::{BasicId, TypeId, TypeTable};

use crate::abi::{SlotClass, slot_of};
use crate::diag::CgCode;
use crate::emit::Asm;
use crate::frame::{Frame, Slot};
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
    asm.ins(&format!("add {ADDR}, x29, #{off}"));
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
fn load_imm(asm: &mut Asm, reg: &str, v: i64) {
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
pub fn func(f: &SsaFunc, fr: &Frame, tt: &TypeTable, asm: &mut Asm) -> Result<(), CgCode> {
    let cx = Ctx { f, fr, tt };
    // ⚠️ 符号名用 `fn<HIR FuncId>`，**不用** `f.name`：
    // ① 名字会撞（两个类型各有一个 `M` 方法）；
    // ② 调用点只能看到 `FuncId`（`CalleeRef::Func`），看不到名字 ——
    //    两边用不同来源命名，链接期就是一堆 `undefined reference`。
    let sym = fn_sym(f.src);
    asm.global(&sym);
    asm.label(&sym);
    prologue(fr, asm);

    // 形参：入口块的 `params` 从寄存器落到槽里。
    bind_params(&cx, asm);

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
fn fn_sym(id: rgoc_hir::FuncId) -> String {
    format!("fn{}", id.0)
}

/// 块标签（`<sym>.b<k>`）。
fn block_label(sym: &str, bi: usize) -> String {
    format!("{sym}.b{bi}")
}

/// 形参落槽：整数走 `x`、浮点走 `v`（两个序列**独立计数**，`C5` §6.1）。
fn bind_params(cx: &Ctx<'_>, asm: &mut Asm) {
    let mut xi = 0usize;
    let mut vi = 0usize;
    for p in &cx.f.params {
        let class = cx.class(*p);
        let s = cx.slot(*p);
        match class {
            SlotClass::Float | SlotClass::Complex64 => {
                if cx.is_f32(cx.ty(*p)) || class == SlotClass::Complex64 {
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
            // 聚合按指针传（`C5` §6.1），所以这里落到槽里的是**地址**。
            SlotClass::Int | SlotClass::Agg => {
                st_x(asm, &format!("x{xi}"), s.off);
                xi += 1;
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
        Instr::AddrOf { dst, root } => addr_of(cx, *dst, root, asm),
        Instr::Field { dst, base, index } => field(cx, *dst, *base, *index, asm),
        Instr::Index { dst, base, i } => index(cx, *dst, *base, *i, asm),
        // `new(T)` 也是 `Make`，但它的 `typ` 是**指针**（`build.rs` 的约定）——
        // 那不是切片，走 arena 分配（`D-M7-7`）。只有 `make([]T,…)` 才是 `[]T`。
        Instr::Make {
            dst,
            typ,
            len: None,
            cap: None,
        } if is_pointer_ty(cx.tt, *typ) => new_alloc(cx, *dst, *typ, asm),
        // `[]T` 全套归 M9b（`U-M7-01`）。
        Instr::Slice { .. } | Instr::Make { .. } => return Err(CgCode::Slice),
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
    match c {
        ConstVal::Bool(b) => {
            load_imm(asm, "x10", i64::from(*b));
            st_x(asm, "x10", s.off);
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
            let lo = asm.rodata_f64(re.to_bits());
            let hi = asm.rodata_f64(im.to_bits());
            load_addr(asm, "x10", &lo);
            asm.ins("ldr d10, [x10]");
            st_d(asm, "d10", s.off);
            load_addr(asm, "x10", &hi);
            asm.ins("ldr d10, [x10]");
            st_d(asm, "d10", s.off + 8);
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
    match cx.class(dst) {
        SlotClass::Float if cx.is_f32(typ) => {
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
            // 聚合不支持算术（前端拒），走到这里就是内部不一致。
        }
        SlotClass::Int => {
            ld_x(asm, "x10", l.off);
            ld_x(asm, "x11", r.off);
            if let Some(cond) = icmp_cond(op, cx.is_unsigned(typ)) {
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
                BinOp::Shl => asm.ins("lsl x12, x10, x11"),
                BinOp::Shr => {
                    let ins = if signed { "asr" } else { "lsr" };
                    asm.ins(&format!("{ins} x12, x10, x11"));
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

/// `complex64` 的加减（两个 `s` 打包在一个槽里）。
fn complex_bin_f32(op: BinOp, l: i32, r: i32, d: i32, asm: &mut Asm) {
    if !matches!(op, BinOp::Add | BinOp::Sub) {
        return;
    }
    let ins = if op == BinOp::Add { "fadd" } else { "fsub" };
    ld_s(asm, "s10", l);
    ld_s(asm, "s11", r);
    asm.ins(&format!("{ins} s12, s10, s11"));
    st_s(asm, "s12", d);
    ld_s(asm, "s10", l + 4);
    ld_s(asm, "s11", r + 4);
    asm.ins(&format!("{ins} s12, s10, s11"));
    st_s(asm, "s12", d + 4);
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

/// 类型转换。
fn convert(cx: &Ctx<'_>, dst: ValueId, x: ValueId, typ: TypeId, asm: &mut Asm) {
    let (d, s) = (cx.slot(dst), cx.slot(x));
    let from = cx.ty(x);
    let to_class = cx.class(dst);
    let from_class = cx.class(x);
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
        // 其余组合（指针/聚合/复数转换）在 Go 里要么非法、要么归 M9。
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
            if let Some(s) = cx.fr.addr_slots.get(&slot) {
                // ⚠️ 不能用 `addr()` 再 `st_x(asm, "x9", …)`：`st_x` 自己会先
                // `add x9, x29, #off`，把刚算好的地址覆盖掉。所以地址进 `x10`。
                asm.ins(&format!("add x10, x29, #{}", s.off));
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
            Type::Struct(sid) => return field_offset(tt, *sid, index).unwrap_or(0),
            _ => return 0,
        }
    }
    0
}

/// `base[i]`：基址 + `i * elem_size`。
fn index(cx: &Ctx<'_>, dst: ValueId, base: ValueId, i: ValueId, asm: &mut Asm) {
    let (d, b, iv) = (cx.slot(dst), cx.slot(base), cx.slot(i));
    let esz = elem_size_of(cx.tt, cx.ty(base));
    ld_x(asm, "x10", b.off);
    ld_x(asm, "x11", iv.off);
    load_imm(asm, "x12", i64::from(esz));
    asm.ins("madd x10, x11, x12, x10");
    st_x(asm, "x10", d.off);
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
            Type::Array { elem, .. } => return layout(tt, *elem).map_or(8, |l| l.size.max(1)),
            _ => return 8,
        }
    }
    8
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
    let (size, align) = layout(cx.tt, elem).map_or((8, 8), |l| (l.size, l.align));
    load_imm(asm, "x0", i64::from(size));
    load_imm(asm, "x1", i64::from(align));
    asm.ins("bl rt_arena_alloc");
    st_x(asm, "x0", d.off);
}

/// 复合字面量：先清零，再按元素落位。
fn composite(cx: &Ctx<'_>, dst: ValueId, typ: TypeId, items: &[ValueId], asm: &mut Asm) {
    let d = cx.slot(dst);
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
    if cx.width(typ) < 8 {
        if cx.is_unsigned(typ) || basic_of(cx.tt, typ) == Some(BasicId::Bool) {
            asm.ins("uxtw x11, w11");
        } else {
            asm.ins("sxtw x11, w11");
        }
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
    _dots: bool,
    asm: &mut Asm,
) -> Result<(), CgCode> {
    match callee {
        CalleeRef::Builtin(b) => builtin(cx, *b, args, asm),
        CalleeRef::Func(fid) => {
            pass_args(cx, args, asm);
            asm.ins(&format!("bl {}", cx.tt_fn_name(*fid)));
            take_results(cx, dsts, asm);
            Ok(())
        }
        CalleeRef::Method { recv, func } => {
            // 接收者按指针传（`C5` §6.1 的聚合规则）。
            pass_args(cx, args, asm);
            let _ = recv;
            asm.ins(&format!("bl {}", cx.tt_fn_name(*func)));
            take_results(cx, dsts, asm);
            Ok(())
        }
        CalleeRef::Indirect(_) => Err(CgCode::IndirectCall),
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

/// 装实参（`C5` §6.1：`x`/`v` 两个序列独立计数）。
fn pass_args(cx: &Ctx<'_>, args: &[ValueId], asm: &mut Asm) {
    let mut xi = 0usize;
    let mut vi = 0usize;
    let mut stack = 0u32;
    for a in args {
        let s = cx.slot(*a);
        match cx.class(*a) {
            SlotClass::Float | SlotClass::Complex64 => {
                if cx.is_f32(cx.ty(*a)) || cx.class(*a) == SlotClass::Complex64 {
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
            SlotClass::Agg => {
                // 聚合按指针传：地址进 `x`。
                addr(asm, s.off);
                asm.ins(&format!("mov x{xi}, x9"));
                xi += 1;
            }
            SlotClass::Int => {
                if s.size > 8 {
                    // `string`：两个字分别进 `x{xi}` 与 `x{xi+1}`。
                    ld_x(asm, &format!("x{xi}"), s.off);
                    ld_x(asm, &format!("x{}", xi + 1), s.off + 8);
                    xi += 2;
                } else if xi < 8 {
                    ld_x(asm, &format!("x{xi}"), s.off);
                    xi += 1;
                } else {
                    // 超出 8 个：走调用方 outgoing 区。
                    ld_x(asm, "x10", s.off);
                    asm.ins(&format!("str x10, [sp, #{stack}]"));
                    stack += 8;
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
            SlotClass::Float | SlotClass::Complex64 => {
                if cx.is_f32(cx.ty(*d)) || cx.class(*d) == SlotClass::Complex64 {
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
fn builtin(cx: &Ctx<'_>, b: BuiltinKind, args: &[ValueId], asm: &mut Asm) -> Result<(), CgCode> {
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
        // 其余内建（`len`/`cap`/`append`/`new`/`make`/`copy`/…）在 Task 30 前后补。
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
            } else if basic_of(cx.tt, t) == Some(BasicId::Bool) {
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
    if cx.class(v) == SlotClass::Int && s.size > 8 {
        ld_x(asm, "x0", s.off);
        ld_x(asm, "x1", s.off + 8);
        asm.ins("bl rt_panic_str");
    } else {
        ld_x(asm, "x0", s.off);
        asm.ins("bl rt_panic_i64");
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
    let pairs: Vec<(i32, i32)> = target
        .params
        .iter()
        .zip(args.iter())
        .map(|(p, a)| (cx.slot(*p).off, cx.slot(*a).off))
        .collect();
    let tmp = cx.fr.tmp_base.unwrap_or(-8);
    for m in parallel_copy::schedule(&pairs, tmp) {
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
