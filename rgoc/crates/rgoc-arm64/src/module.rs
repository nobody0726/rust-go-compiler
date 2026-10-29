//! 模块级发射：`_start`、包级全局、逐函数指令选择（`M7-plan.md` Task 49 的骨架）。
//!
//! # 顺序是判据的一部分
//!
//! `_start` 里的 `bl` 序列**必须**是 `init` → `main`（`Program::init_order` 在 M5 就定了，
//! SSA 只把 `inits` 合成一个 `Module::init`，`R-32-1`）。这里不排序、不重排 ——
//! 按模块给的 `main` / `init` 两个 `FuncId` 直接发射。重排就等于把「初始化顺序」
//! 这个语义从 M5 搬到 M7，而 M7 没有判据能验它。
//!
//! # 全局变量
//!
//! 每个 `GlobalId` 一个 `.data` 标签 `g<id>`，大小按 `global_types` 的布局给、
//! 内容全零（Go 的包级变量零值语义）。零大小全局只发标签不发字节。

use std::collections::{BTreeMap, BTreeSet};

use rgoc_meta::desc::{DescBuilder, name_hash, sig_hash, type_hash};
use rgoc_ssa::ir::{Hop, Module, SsaFunc};
use rgoc_types::TypeTable;
use rgoc_types::layout::{layout, ptr_mask};
use rgoc_types::typ::TypeId;

use crate::abi::{SlotClass, slot_of};
use crate::diag::CgCode;
use crate::emit::Asm;
use crate::frame::{self, FrameError};
use crate::isel;

/// 编译失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// M7 明确不支持的面。
    Cg(CgCode),
    /// 某个值的类型没有运行期布局（第二个字段是**哪个函数** —— 只有 `ValueId`
    /// 定位不到，同一个号在几十个函数里都出现过）。
    Frame(FrameError, String),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cg(c) => write!(f, "{c}"),
            Self::Frame(e, func) => write!(f, "函数 {func}：{e}"),
        }
    }
}

/// 把一个 SSA 模块编成 `.s` 文本。
///
/// # Errors
///
/// 不支持的面（`[]T` / 间接调用）或没有运行期布局的类型。
pub fn compile(m: &Module, tt: &TypeTable) -> Result<String, CompileError> {
    let mut asm = Asm::new();

    // ① 零大小对象的**唯一地址**（`runtime.zerobase` 的等价物）。
    //
    // `zerosize.go` 要求 `&x == new([0]int)`：零大小全局与 `new` 出来的零大小对象
    // 必须是**同一个地址**。所以这个符号由**我们**定义（`rt.c` 里只 `extern` 引用），
    // 零大小全局用 `.set` 别名到它 —— 让「同一个地址」只有一处来源。
    asm.global("rt_zerobase");
    asm.data_named("rt_zerobase", &[0u8; 8]);

    // ② 包级全局（`.data`，全零）。
    for (i, g) in m.globals.iter().enumerate() {
        let ty = m.global_types.get(i).copied();
        let size = ty.and_then(|t| layout(tt, t)).map_or(8, |l| l.size);
        if size == 0 {
            // 零大小：别名到 `rt_zerobase`，不占独立地址。
            asm.alias(&format!("g{}", g.0), "rt_zerobase");
        } else {
            let zeros = vec![0u8; size as usize];
            asm.data_named(&format!("g{}", g.0), &zeros);
        }
    }

    // ②a 全局 roots 表（`M9-design.md` §3.11）。
    //
    // **必须在 desc 之后**：表的每一条要引用 `__desc_<hash>`，而那些符号由
    // `emit_descs` 定义（GNU as 的 `.quad` 允许前向引用，但先定义后引用能
    // 让「符号名怎么拼」只有一处来源）。
    // 先把 `FuncId → 函数` 建起来：调用点的寄存器分配由**被调方的形参类型**
    // 决定（`R-13-3`），`isel` 需要能按 `FuncId` 查到那些类型。
    let siblings: std::collections::BTreeMap<rgoc_hir::FuncId, &rgoc_ssa::ir::SsaFunc> =
        m.funcs.iter().map(|f| (f.src, f)).collect();

    // ②b 方法表（`M9/Task 27`）与静态 ITab（`M9/Task 28`）。
    //
    // 顺序是硬的：方法表先（desc 的 `methods` 字段要引用它的符号），
    // itab 后（它引用 desc 与方法表两侧的符号）。GNU as 允许前向引用，
    // 但「符号名一处来源」这条纪律要求先定义后引用。
    EMITTED_THUNKS.with(|s| s.borrow_mut().clear());
    emit_funcvals(&mut asm, m);
    let mt = emit_method_tables(&mut asm, m, tt, &siblings);
    emit_itabs(&mut asm, m, tt, &siblings);
    emit_descs(&mut asm, m, tt, &mt);
    emit_global_roots(&mut asm, m, tt);

    // ③ 逐函数；顺手收栈图（`M9/Phase 6`，`Task 59`）。
    //
    // 帧只算一次，代码与栈图共用同一个 `fr` —— 两处各 layout 一遍就会出现
    // 「位图按 A 帧算、指令按 B 帧发」这种只在改过帧布局之后才露头的错。
    let mut stackmaps: Vec<FuncStackMap> = Vec::with_capacity(m.funcs.len());
    for (i, f) in m.funcs.iter().enumerate() {
        let fr = frame::layout_with(f, tt, &siblings)
            .map_err(|e| CompileError::Frame(e, f.name.clone()))?;
        // ⚠️ **M10：`Liveness` 只算一次**，`isel` 的 `sp_idx` 与这里的栈图
        // 共用同一份（`M10-design.md` §8 风险 1）。两处各算一遍会错位，而
        // 错位的症状是「扫了错的字」—— 随机崩或静默算错，极难查。
        let cfg = rgoc_ssa::cfg::Cfg::build(f);
        let lv = rgoc_ssa::live::analyze(f, &cfg, tt);
        isel::func(f, &fr, tt, &siblings, &lv, i, &mut asm).map_err(CompileError::Cg)?;
        stackmaps.push(stack_map_of(i, f, &fr, &lv, tt, &siblings));
    }
    emit_stackmap_rodata(&mut asm, &stackmaps);

    // ④ 除零的统一出口。
    //
    // `isel` 在 `sdiv` 前发 `cbz x11, .Ldiv0`（`M7-design.md` §3.9 的检查点之一）。
    // 这个标签必须**定义一次**且全模块可见 —— 每条除法都跳到同一处，
    // 而不是每处内联一份 panic 文案（那会让 `.s` 随除法个数膨胀）。
    asm.label(".Ldiv0");
    asm.ins("bl rt_panic_div_zero");
    asm.ins("brk #1");

    // ⑤ `_start`：init → main → exit(0)。
    asm.global("_start");
    asm.label("_start");
    // ① 把进程入口的 sp 交给 runtime（M9/Task 7）：它指向 argc，
    //    后面是 argv 与 envp —— freestanding 下读环境变量的唯一入口。
    //    **必须**在 init / main 之前（它们可能一上来就分配）。
    asm.ins("mov x0, sp");
    asm.ins("bl rt_init");
    // `Module::init` 是**在 `funcs` 里的下标**（不是 `FuncId`）；`main` 是 `FuncId`。
    if let Some(f) = m.init.and_then(|idx| m.funcs.get(idx)) {
        asm.ins(&format!("bl fn{}", f.src.0));
    }
    if let Some(main) = m.main {
        asm.ins(&format!("bl fn{}", main.0));
    }
    // `main` 返回后：**没有**未恢复的 panic 才 exit 0。
    //
    // `rt_panic_*` 只置状态（好让 defer 有机会 `recover`），所以「打一行 +
    // 退出码 2」这条 `C9` §4.5 的契约落在这里 —— 全局唯一一处退出点。
    //
    // `bl rt_gc_stats_line` 在它**之前**：统计行是「退出时打」（`C12` §5.2），
    // 而这里就是全程序唯一的退出点 —— 正常退出与 panic 退出都经过它。
    // `RGOC_GC_STATS` 没设时该函数立即返回，一个字节都不打。
    asm.ins("bl rt_gc_stats_line");
    asm.ins("bl rt_panicking");
    asm.ins("cbz x0, .Lexit0");
    asm.ins("bl rt_print_panic");
    asm.ins("mov x0, #2");
    asm.ins("bl rt_exit");
    asm.label(".Lexit0");
    asm.ins("mov x0, #0");
    asm.ins("bl rt_exit");

    Ok(asm.render())
}

/// desc 符号名。**唯一来源** —— `isel` 与 M10 都按它找。
///
/// 用 `hash` 而不是 `TypeId`：`TypeId` 是容器内的身份，同一个类型在两次编译里
/// 可以是不同的号；而 hash 是类型自己的身份（`T-META-05`）。
#[must_use]
pub fn desc_symbol(hash: u32) -> String {
    format!("__desc_{hash:08x}")
}

thread_local! {
    /// 已发射的 `__imeth_*` 桩（去重用；每次编译开头清空）。
    static EMITTED_THUNKS: std::cell::RefCell<std::collections::BTreeSet<String>> =
        const { std::cell::RefCell::new(std::collections::BTreeSet::new()) };
}

/// 静态 funcval 的符号（**没有捕获**的函数值，`M9/Task 37`）。
///
/// # 为什么不是「直接用函数符号」
///
/// funcval 的布局是 `{ fn: *u8, caps: [*u8; n] }`，间接调用一律
/// `ldr x16, [x0]`（`x0` = funcval）。所以**即使没有捕获**，函数值的第一个字
/// 也必须是 `fn` —— 直接把函数符号当 funcval 用，`ldr` 读到的是函数的第一条
/// 指令（实测 `Bus error`）。所以给每个被当作值用的函数发一个 8 字节的静态
/// funcval（`D-M9-10` 的最小形态）。
#[must_use]
pub fn fv_symbol(fid: rgoc_hir::FuncId) -> String {
    format!("__fv_{}", fid.0)
}

/// 发射全部静态 funcval（每个「带 ctx ABI」的函数一条）。
///
/// 判据用 `ctx_abi` 而不是「扫 `FuncAddr`」：两者等价（`build()` 的收尾扫描
/// 就是按 `FuncAddr`/`MakeClosure` 定的 `ctx_abi`），但这里只拿得到 `Module`。
fn emit_funcvals(asm: &mut Asm, m: &Module) {
    for f in &m.funcs {
        if !f.ctx_abi {
            continue;
        }
        let body = vec![format!(".quad {}", isel::fn_sym(f.src))];
        asm.rodata_named_raw(&fv_symbol(f.src), &body);
    }
}

/// 方法表符号。
///
/// 键是**类型 hash**（不是 `TypeId`）：`__desc_*` 同理，理由见 [`desc_symbol`]。
#[must_use]
pub fn mt_symbol(hash: u32) -> String {
    format!("__mt_{hash:08x}")
}

/// 静态 ITab 符号。
#[must_use]
pub fn itab_symbol(iface_hash: u32, concrete_hash: u32) -> String {
    format!("__itab_{iface_hash:08x}_{concrete_hash:08x}")
}

/// 接收者适配桩符号（`itab.fn[slot]` 指它）。
#[must_use]
pub fn imeth_symbol(concrete_hash: u32, slot: usize) -> String {
    format!("__imeth_{concrete_hash:08x}_{slot}")
}

/// 接收者适配的一步（`x0` 是当前地址）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// `x0 = *(void **)(x0 + off)`（嵌入链上的指针字段）
    Load(u32),
    /// `x0 = x0 + off`
    Add(u32),
    /// `x0 = *(T *)(x0 + off)`：把**标量接收者的值**读出来。
    ///
    /// # 为什么要带宽度（`R-9-27`）
    ///
    /// 原来这里一律发 `ldr x0, [x0]`（8 字节）。对 `func (v byte) M()` 就是
    /// 从 1 字节的盒子里读 8 字节 —— 高 56 位是垃圾（实测 `method5.go` 的
    /// `V byte` + 接口派发：`i.M(100)` 打出 `187650223333225`，解释器对、native 错）。
    /// 宽度只能从**布局**来（`slot_of().size` 是 8，是槽宽不是值宽）。
    ///
    /// `width > 8` 只有 `string` 这一类 16 字节的「标量」：它的两个字都要读
    /// （`x0` = data、`x1` = len）。
    LoadScalar {
        /// 字节偏移。
        off: u32,
        /// 值宽（1 / 2 / 4 / 8 / 16）。
        width: u32,
        /// 有符号 ⇒ 末尾 `sxt*`，否则 `uxt*`。
        signed: bool,
    },
}

/// `data`（指向动态类型的存储）→ 方法第 0 个实参的适配步骤。
///
/// # 三条规则（`M9-design.md` §3.8 的落地）
///
/// 1. **嵌入链**：每一跳按「字段是不是指针」选 `Load` / `Add`；
/// 2. **指针接收者**（`*U`）：`data` 本身就是那个指针 ⇒ 不再动；
/// 3. **聚合值接收者**（`U` 且 `SlotClass::Agg`）：本后端的聚合**按指针传参**
///    ⇒ `data` 正好是它要的，不再动；
/// 4. **其余值接收者**（标量 / `string` 这类走整数寄存器的）：要把**值**读出来
///    ⇒ 末尾加一条 `Load(0)`。
///
/// 第 3 条是 `R-9-11` 记下的「借来的便宜」：官方 Go 在这里要生成一份
/// `func (u U) M` 的包装（把指针解引用成值再调），我们因为聚合本来就按指针传，
/// 白拿一层。
fn recv_steps(
    tt: &TypeTable,
    siblings: &BTreeMap<rgoc_hir::FuncId, &SsaFunc>,
    hops: &[Hop],
    func: rgoc_hir::FuncId,
) -> Vec<Step> {
    let mut out: Vec<Step> = hops
        .iter()
        .map(|h| {
            if h.via_ptr {
                Step::Load(h.off)
            } else {
                Step::Add(h.off)
            }
        })
        .collect();
    let Some(f) = siblings.get(&func) else {
        return out;
    };
    // 第 0 个形参是接收者（`build.rs::call` 的约定），它的类型在 `value_types` 里
    let Some(pid) = f.params.first() else {
        return out;
    };
    let Some(&recv_ty) = f.value_types.get(pid.index()) else {
        return out;
    };
    let is_ptr = matches!(
        tt.type_of(rgoc_types::underlying(tt, recv_ty)),
        rgoc_types::typ::Type::Pointer(_)
    );
    let agg = slot_of(tt, recv_ty).map(|s| s.class) == Some(SlotClass::Agg);
    if !is_ptr && !agg {
        // 值的**真实宽度**（不是槽宽）：`slot_of().size` 把标量都算成 8。
        let width = layout(tt, recv_ty).map_or(8, |l| l.size);
        let signed = !matches!(
            isel::basic_of(tt, recv_ty),
            Some(rgoc_const::basic::BasicId::Uint8)
                | Some(rgoc_const::basic::BasicId::Uint16)
                | Some(rgoc_const::basic::BasicId::Uint32)
                | Some(rgoc_const::basic::BasicId::Uint64)
                | Some(rgoc_const::basic::BasicId::Uint)
                | Some(rgoc_const::basic::BasicId::Uintptr)
                | Some(rgoc_const::basic::BasicId::Bool)
        );
        out.push(Step::LoadScalar {
            off: 0,
            width,
            signed,
        });
    }
    out
}

/// 发射一块方法表，返回 `类型 hash → 符号`（`emit_descs` 用它填 `methods`）。
///
/// # 两类表
///
/// | 谁 | 条目 | `func` 槽 |
/// |---|---|---|
/// | **具体类型** | 名字 hash + 签名 hash | 真实方法（或 thunk）|
/// | **接口类型** | 名字 hash + 签名 hash | `0`（接口没有实现）|
///
/// 接口表只有 `rt_itab` 用（`x.(J)` 要按 J 的方法名去具体类型的表里查）。
fn emit_method_tables(
    asm: &mut Asm,
    m: &Module,
    tt: &TypeTable,
    siblings: &BTreeMap<rgoc_hir::FuncId, &SsaFunc>,
) -> BTreeMap<u32, String> {
    let mut labels: BTreeMap<u32, String> = BTreeMap::new();
    // ① 具体类型
    for ms in &m.methods {
        let h = type_hash(tt, ms.ty);
        // 同一个 hash 可能来自**两个 TypeId**（`*T` 在类型表里可以有多个条目，
        // 只要结构相同 hash 就相同）⇒ 只发一块，否则 GNU as 报
        // `symbol '__mt_xxx' is already defined`（实测 `ken/interbasic.go`）。
        if labels.contains_key(&h) {
            continue;
        }
        let mut body: Vec<String> = Vec::new();
        body.push(format!(".long {}", ms.methods.len()));
        body.push(".long 0".to_string());
        for (i, mi) in ms.methods.iter().enumerate() {
            body.push(format!(".long {}", name_hash(&mi.name)));
            body.push(format!(".long {}", sig_hash(tt, &mi.params, &mi.results)));
            body.push(format!(
                ".quad {}",
                method_entry_symbol(asm, tt, siblings, h, i, mi.func, &[])
            ));
        }
        asm.rodata_named_raw(&mt_symbol(h), &body);
        labels.insert(h, mt_symbol(h));
    }
    // ② 接口类型：闭包里每个**有方法**的接口都发一块（`rt_itab` 要读它）
    let closure = type_closure(m, tt);
    for (h, t) in &closure {
        if labels.contains_key(h) {
            continue;
        }
        let names = rgoc_ssa::ir::iface_methods(tt, *t);
        if names.is_empty() {
            continue;
        }
        let mut body: Vec<String> = Vec::new();
        body.push(format!(".long {}", names.len()));
        body.push(".long 0".to_string());
        for n in &names {
            body.push(format!(".long {}", name_hash(n)));
            body.push(format!(".long {}", iface_sig_hash(tt, *t, n)));
            body.push(".quad 0".to_string());
        }
        asm.rodata_named_raw(&mt_symbol(*h), &body);
        labels.insert(*h, mt_symbol(*h));
    }
    labels
}

/// 接口方法 `name` 的签名 hash（与具体类型那边**同编码**）。
fn iface_sig_hash(tt: &TypeTable, iface: TypeId, name: &str) -> u32 {
    use rgoc_types::typ::Type;
    let mut cur = iface;
    for _ in 0..64 {
        match tt.type_of(cur) {
            Type::Named(id) => cur = tt.named_underlying(*id),
            Type::Alias(id) => cur = tt.alias_rhs(*id),
            Type::Interface(iid) => {
                let def = tt.interface_def(*iid);
                let Some((_, sig)) = def.methods.iter().find(|(n, _)| n == name) else {
                    return 0;
                };
                let Some(Type::Signature(sid)) = Some(tt.type_of(*sig)) else {
                    return 0;
                };
                let sd = tt.signature_def(*sid);
                let params = tuple_types(tt, sd.params);
                let results = tuple_types(tt, sd.results);
                return sig_hash(tt, &params, &results);
            }
            _ => return 0,
        }
    }
    0
}

/// 元组的元素类型（非元组 ⇒ 空）。
fn tuple_types(tt: &TypeTable, t: TypeId) -> Vec<TypeId> {
    match tt.type_of(t) {
        rgoc_types::typ::Type::Tuple(tid) => {
            tt.tuple_def(*tid).vars.iter().map(|(_, x)| *x).collect()
        }
        _ => Vec::new(),
    }
}

/// 方法表某一格的符号：`Identity` ⇒ 真实方法；否则发一段尾跳桩。
fn method_entry_symbol(
    asm: &mut Asm,
    tt: &TypeTable,
    siblings: &BTreeMap<rgoc_hir::FuncId, &SsaFunc>,
    concrete_hash: u32,
    slot: usize,
    func: rgoc_hir::FuncId,
    hops: &[Hop],
) -> String {
    let steps = recv_steps(tt, siblings, hops, func);
    if steps.is_empty() {
        return isel::fn_sym(func);
    }
    let sym = imeth_symbol(concrete_hash, slot);
    // 同一个 `(具体类型, 槽)` 可能出现在**多张** itab 里（`ddd.go`：同一类型
    // 同时喂给两个接口）⇒ 桩只能发一次，否则 GNU as 报 `symbol already defined`。
    let fresh = EMITTED_THUNKS.with(|s| s.borrow_mut().insert(sym.clone()));
    if !fresh {
        return sym;
    }
    asm.label(&sym);
    for st in &steps {
        match st {
            Step::Load(off) => load_off(asm, *off, "ldr x0, [x0, x9]", "ldr x0, [x0, #"),
            Step::Add(off) => add_off(asm, *off),
            Step::LoadScalar { off, width, signed } => load_scalar(asm, *off, *width, *signed),
        }
    }
    asm.ins(&format!("b {}", isel::fn_sym(func)));
    sym
}

/// 把标量接收者的**值**读进 `x0`（`x1` 只在 16 字节的 `string` 上用）。
fn load_scalar(asm: &mut Asm, off: u32, width: u32, signed: bool) {
    if off != 0 {
        add_off(asm, off);
    }
    match width {
        1 => {
            asm.ins("ldrb w0, [x0]");
            asm.ins(if signed { "sxtb x0, w0" } else { "uxtb x0, w0" });
        }
        2 => {
            asm.ins("ldrh w0, [x0]");
            asm.ins(if signed { "sxth x0, w0" } else { "uxth x0, w0" });
        }
        4 => {
            asm.ins("ldr w0, [x0]");
            asm.ins(if signed { "sxtw x0, w0" } else { "uxtw x0, w0" });
        }
        8 => asm.ins("ldr x0, [x0]"),
        // `string`（`(ptr, len)` 两个字）：第二个字进 `x1`。
        // 其它 >8 的标量不存在；真出现的话这条也只会多读一个字，不会静默截断。
        _ => {
            asm.ins("ldr x1, [x0, #8]");
            asm.ins("ldr x0, [x0]");
        }
    }
}

/// `add x0, x0, #off`（超过 12 位立即数就走寄存器）。
fn add_off(asm: &mut Asm, off: u32) {
    if off <= 4095 {
        asm.ins(&format!("add x0, x0, #{off}"));
    } else {
        isel::load_imm(asm, "x9", i64::from(off));
        asm.ins("add x0, x0, x9");
    }
}

/// `ldr x0, [x0, #off]`（`off` 必须是 8 的倍数且 ≤ 32760，否则走寄存器）。
fn load_off(asm: &mut Asm, off: u32, _via_reg: &str, _prefix: &str) {
    if off.is_multiple_of(8) && off <= 32760 {
        asm.ins(&format!("ldr x0, [x0, #{off}]"));
    } else {
        isel::load_imm(asm, "x9", i64::from(off));
        asm.ins("ldr x0, [x0, x9]");
    }
}

/// 发射全部静态 ITab（`M9/Task 28`）。
///
/// 布局（`M9-design.md` §3.8）：`{ iface: *TypeDesc, concrete: *TypeDesc, fn: [*u8] }`。
/// `fn` 从偏移 **16** 起 —— `isel` 的 `tab.fn[slot]` 就是这个 `16 + 8*slot`。
fn emit_itabs(
    asm: &mut Asm,
    m: &Module,
    tt: &TypeTable,
    siblings: &BTreeMap<rgoc_hir::FuncId, &SsaFunc>,
) {
    let mut done: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for it in &m.itabs {
        let ih = type_hash(tt, it.iface);
        let ch = type_hash(tt, it.concrete);
        // 同上：`(I, T)` 的 TypeId 对可能有多份，hash 对只有一份
        if !done.insert(itab_symbol(ih, ch)) {
            continue;
        }
        let mut body: Vec<String> = Vec::new();
        body.push(format!(".quad {}", desc_symbol(ih)));
        body.push(format!(".quad {}", desc_symbol(ch)));
        for (slot, e) in it.fns.iter().enumerate() {
            let sym = method_entry_symbol(asm, tt, siblings, ch, slot, e.func, &e.hops);
            body.push(format!(".quad {sym}"));
        }
        asm.rodata_named_raw(&itab_symbol(ih, ch), &body);
    }
}

/// 收集「需要发射 desc 的类型闭包」。
///
/// # 闭包怎么算
///
/// 起点是模块里**出现过的**类型：包级变量的类型 + 每个值的类型 + 取过地址的槽的
/// 类型。然后沿 **`elem` 链**递归 —— 因为 `TypeDesc.elem` 是个指针，被指的那块
/// 必须也存在。
///
/// | 类型 | 闭包带谁 |
/// |---|---|
/// | `*T` / `[]T` / `[n]T` / `chan T` | `T` |
/// | `map[K]V` | `K` **与** `V`（`MapObj` 的 `kdesc` / `vdesc` 要指它们）|
/// | `struct{...}` | **不带**字段类型（字段的根由本块的 `gcdata` 表达，不需要单独一块）|
/// | `func` | 不带（签名没有 desc）|
///
/// # 去重键是 `hash`
///
/// 同一个类型（同一个 `hash`）只发一块。这不只是省体积：**两块地址不同**会让
/// 「接口断言 `x.(int)`」在两条路径上拿到不同的 desc 指针，从而判成不同类型。
#[must_use]
pub fn type_closure(m: &Module, tt: &TypeTable) -> BTreeMap<u32, TypeId> {
    let mut out: BTreeMap<u32, TypeId> = BTreeMap::new();
    let mut stack: Vec<TypeId> = Vec::new();
    // **无类型常量不作为根**：它们没有运行期表示（`R-13-1` 在写入点收敛成默认
    // 类型），给它们发 desc 只会得到「每个含字面量的程序多一块死数据」。
    //
    // ⚠️ 但**不能**在闭包扩展里也过滤：`*untyped int` 这种指针真的会出现
    // （`newexpr.go` 里就有，指向一个常量槽），而它的 `elem` 必须存在，
    // 否则链接期 `undefined reference to __desc_…`。所以过滤只在根上做。
    let mut roots: Vec<TypeId> = Vec::new();
    roots.extend(m.global_types.iter().copied());
    for f in &m.funcs {
        roots.extend(f.value_types.iter().copied());
        roots.extend(f.slot_types.values().copied());
    }
    for t in roots {
        if !is_untyped_basic(tt, t) {
            stack.push(t);
        }
    }
    // **itab 的两侧都是根**（`M12/Task 18`）：`__itab_<iface>_<concrete>` 的头部
    // 就是 `{iface: *TypeDesc, concrete: *TypeDesc}`，两个都会被 `.quad` 引用。
    //
    // 它们不会自己出现在上面的根里：`X I = 42` 的 `I` 有根（包级变量类型），
    // 而 `42` 在 IR 里的类型是 `untyped int` —— 被 `is_untyped_basic` 过滤掉，
    // 收敛后的 `int` 从来没有作为**值**出现过。于是 itab 指着一个没发射的
    // `__desc_…`，链接期 `undefined reference`（`issue52124` 实测）。
    for it in &m.itabs {
        stack.push(it.iface);
        stack.push(it.concrete);
    }
    while let Some(t) = stack.pop() {
        // 没有布局的类型不进闭包（它们本就该被拒，`kind_of` 会给 `Invalid`）。
        if layout(tt, t).is_none() {
            continue;
        }
        let h = type_hash(tt, t);
        if out.contains_key(&h) {
            continue;
        }
        out.insert(h, t);
        match tt.type_of(t) {
            rgoc_types::typ::Type::Pointer(e) | rgoc_types::typ::Type::Slice(e) => stack.push(*e),
            rgoc_types::typ::Type::Array { elem, .. } => stack.push(*elem),
            rgoc_types::typ::Type::Chan { elem, .. } => stack.push(*elem),
            rgoc_types::typ::Type::Map { key, elem } => {
                stack.push(*key);
                stack.push(*elem);
            }
            // `Named` **不**主动带本体：具名类型的 desc 自带 size/kind/gcdata，
            // 不需要本体那块。而 `type S struct{...}` 若把匿名结构体也发一块，
            // 就会有两块 `kind=Struct` 的 desc（大小相同、hash 不同），
            // 任何「按 kind 找」的调试与判据都会歧义。
            // 本体真的被用到时它自己会在 `value_types` 里出现。
            rgoc_types::typ::Type::Alias(id) => stack.push(tt.alias_rhs(*id)),
            // struct 的**字段类型**必须进闭包（M12/Task 30）：`ReflectMeta`
            // 的字段表每个 `.quad __desc_…` 都指着它，字段类型没出现过的话
            // 链接期 `undefined reference`。具名本体照旧不展开（理由同上）。
            rgoc_types::typ::Type::Struct(id) => {
                for f in &tt.struct_def(*id).fields {
                    stack.push(f.typ);
                }
            }
            // ⚠️ **具名** struct 的字段表从**本体**发（`emit_reflect_meta`
            // 取 `underlying`），而本体不进闭包 ⇒ 匿名 struct 臂够不着它，
            // 字段类型的 desc 同样缺。这里只把**字段类型**带进来（本体
            // 依旧不带 —— 否则两块 `kind=Struct` 的 desc，歧义照旧）。
            // 实测缺失形态：`issue51840` 链接期
            // `undefined reference to __desc_d38d3a69`（来自 `__reflectfields_*`）。
            rgoc_types::typ::Type::Named(_) => {
                if let rgoc_types::typ::Type::Struct(sid) =
                    tt.type_of(rgoc_types::identical::underlying(tt, t))
                {
                    for f in &tt.struct_def(*sid).fields {
                        stack.push(f.typ);
                    }
                }
            }
            // M12/Task 33：签名类型的 `ReflectMeta.params/results` 每个
            // `.quad __desc_…` 都指着形参/结果的 desc —— 不进闭包就是
            // 链接期 `undefined reference`（与 struct 字段同族）。
            rgoc_types::typ::Type::Signature(sid) => {
                let sd = tt.signature_def(*sid);
                let elems = tuple_types(tt, sd.params)
                    .into_iter()
                    .chain(tuple_types(tt, sd.results));
                for e in elems {
                    stack.push(e);
                }
            }
            _ => {}
        }
    }
    out
}

/// 是无类型常量的基本类型吗（`untyped int` / `untyped string` …）。
fn is_untyped_basic(tt: &TypeTable, t: TypeId) -> bool {
    match tt.type_of(t) {
        rgoc_types::typ::Type::Basic(b) => b.is_untyped(),
        _ => false,
    }
}

/// `ptrdata`：最后一个为根的字**之后**的字节数（`string` 是 8，不是 16）。
///
/// 官方 `internal/abi/type.go` 的 `PtrBytes` 就是这个含义 —— `gcdata` 只覆盖
/// `[0, ptrdata)`，再往后的位没人读。写大了不会错但会浪费；写小了会**漏扫**。
#[must_use]
pub fn ptrdata_of(mask: &[bool]) -> u32 {
    let last = mask.iter().rposition(|b| *b);
    match last {
        Some(i) => u32::try_from((i + 1) * 8).unwrap_or(u32::MAX),
        None => 0,
    }
}

/// 把 ptrmask 打成字节位图（每字 1 位，**LSB first**）。
///
/// 位序与官方 `gcdata` 一致：字 `i` 对应字节 `i/8` 的位 `i%8`。
#[must_use]
pub fn gcdata_bytes(mask: &[bool]) -> Vec<u8> {
    let mut out = vec![0u8; mask.len().div_ceil(8)];
    for (i, b) in mask.iter().enumerate() {
        if *b {
            out[i / 8] |= 1 << (i % 8);
        }
    }
    out
}

/// 发射全局 roots 表：`rt_globals`（`(desc, addr)` 两列）+ `rt_nglobals`。
///
/// # 为什么零大小全局不进表
///
/// 它们别名到 `rt_zerobase`（没有自己的存储），**没有可扫描的字**。
/// 进表只会让 M10 每次 STW 都去扫一块 0 字节的东西 —— 而且 `desc.size == 0`
/// 时「扫几个字」这个问题本身没有答案。
///
/// # 为什么符号必须恒存在
///
/// M10 的扫描器无条件读 `rt_globals` / `rt_nglobals`。空程序（没有包级变量）
/// 是最容易被漏的那类 —— 少了符号就是链接期 `undefined reference`，
/// 而那会发生在**别的**里程碑里，排查起来完全看不到这里。
fn emit_global_roots(asm: &mut Asm, m: &Module, tt: &TypeTable) {
    let mut body: Vec<String> = Vec::new();
    let mut n: u64 = 0;
    for (i, g) in m.globals.iter().enumerate() {
        let Some(ty) = m.global_types.get(i).copied() else {
            continue;
        };
        let size = layout(tt, ty).map_or(8, |l| l.size);
        if size == 0 {
            continue;
        }
        let desc = desc_symbol(type_hash(tt, ty));
        body.push(format!(".quad {desc}"));
        body.push(format!(".quad g{}", g.0));
        n += 1;
    }
    asm.global("rt_globals");
    asm.global("rt_nglobals");
    asm.data_named_raw("rt_globals", &body);
    asm.data_named_raw("rt_nglobals", &[format!(".quad {n}")]);
}

/// 把类型闭包的 desc 全部发射进 `.rodata`。
fn emit_descs(asm: &mut Asm, m: &Module, tt: &TypeTable, mt: &BTreeMap<u32, String>) {
    let closure = type_closure(m, tt);
    // 位图去重：同一份字节只发一份（`[]int` 与 `[3]int` 的位图可能相同）。
    let mut bitmaps: BTreeMap<Vec<u8>, String> = BTreeMap::new();
    for t in closure.values() {
        let Some(mask) = ptr_mask(tt, *t) else {
            continue;
        };
        // 没有根的类型**不发位图**：`ptrdata == 0` 时 `gcdata` 必须是 null
        // （`C5` §10）。发一块全零的位图不算错，但它会让「`gcdata != null`
        // 等价于有指针」这条推断失效 —— 而 M10 的扫描器很可能就这么写。
        if !mask.iter().any(|b| *b) {
            continue;
        }
        let bytes = gcdata_bytes(&mask);
        if bytes.is_empty() {
            continue;
        }
        if let std::collections::btree_map::Entry::Vacant(e) = bitmaps.entry(bytes.clone()) {
            e.insert(asm.rodata_bytes(&bytes));
        }
    }
    // 反射元数据的**下标分配**（1 起；`0` = 没有）。发射序按 hash 序 ——
    // 与 desc 同序，`.s` 保持确定性（`T-GEN-04` 的口径）。
    let mut meta_idx_of: BTreeMap<u32, u16> = BTreeMap::new();
    let mut metas: Vec<(u32, TypeId)> = Vec::new();
    for (h, t) in &closure {
        if needs_reflect_meta(tt, *t) {
            meta_idx_of.insert(*h, (metas.len() + 1) as u16);
            metas.push((*h, *t));
        }
    }
    for (h, t) in &closure {
        let Some(d) = DescBuilder::new(tt, *t).map(DescBuilder::build) else {
            continue;
        };
        let mask = ptr_mask(tt, *t).unwrap_or_default();
        let mut header: Vec<u8> = Vec::with_capacity(16);
        header.extend_from_slice(&d.size.to_le_bytes());
        header.extend_from_slice(&ptrdata_of(&mask).to_le_bytes());
        header.extend_from_slice(&d.hash.to_le_bytes());
        header.push(d.kind);
        header.push(d.flags);
        // +14..16：原来是对齐填充，M12（`C13` §5.1）语义改为 `meta_idx`
        // （1 起；0 = 没有反射元数据）。**偏移一个没动** —— M9 的判据不受影响。
        let meta_idx = meta_idx_of.get(h).copied().unwrap_or(0);
        header.extend_from_slice(&meta_idx.to_le_bytes());
        let byte_list: Vec<String> = header.iter().map(u8::to_string).collect();
        let gc_label = if mask.iter().any(|b| *b) {
            bitmaps
                .get(&gcdata_bytes(&mask))
                .cloned()
                .unwrap_or_else(|| "0".to_string())
        } else {
            "0".to_string()
        };
        let elem_label = elem_of(tt, *t)
            .map(|e| desc_symbol(type_hash(tt, e)))
            .unwrap_or_else(|| "0".to_string());
        // 反射扩展区（`C13` §5.4）：desc 本体 40 字节之后按 kind 追加 quad。
        // 本体消费方（rt / 位图 / itab）零改动；M9 的 `t_desc_*` 判据只读
        // header + 三根 `.quad`，照旧不动。
        //
        // 规则：扩展区镜像 **underlying 的形状**（named 聚合的 +24 elem 恒为
        // null —— `elem_of` 只认表面形状），`meta` 指针（有的话）固定**收尾**；
        // 消费者按 `meta_idx != 0`（⇒ named）区分「本体 elem」与「扩展 elem」。
        let reflect_ext: Vec<String> = {
            use rgoc_types::typ::Type;
            let named = matches!(tt.type_of(*t), Type::Named(_));
            let under_t = if named {
                rgoc_types::identical::underlying(tt, *t)
            } else {
                *t
            };
            let meta_q = || format!(".quad {}", meta_symbol(type_hash(tt, *t)));
            match tt.type_of(under_t) {
                Type::Map { key, elem } => {
                    let mut v = vec![
                        format!(".quad {}", desc_symbol(type_hash(tt, *key))),
                        format!(".quad {}", desc_symbol(type_hash(tt, *elem))),
                    ];
                    if named {
                        v.push(meta_q());
                    }
                    v
                }
                Type::Chan { elem, .. } => {
                    let mut v = vec![format!(".quad {}", desc_symbol(type_hash(tt, *elem)))];
                    if named {
                        v.push(meta_q());
                    }
                    v
                }
                Type::Array { elem, .. } => {
                    let mut v = vec![format!(
                        ".quad {}",
                        rgoc_types::layout::array_len_of(tt, *t).unwrap_or(0)
                    )];
                    if named {
                        v.push(format!(".quad {}", desc_symbol(type_hash(tt, *elem))));
                        v.push(meta_q());
                    }
                    v
                }
                Type::Pointer(e) | Type::Slice(e) if named => vec![
                    format!(".quad {}", desc_symbol(type_hash(tt, *e))),
                    meta_q(),
                ],
                _ => {
                    // struct（匿名/具名）或 named 基本类型 / func / 接口。
                    if needs_reflect_meta(tt, *t) {
                        vec![meta_q()]
                    } else {
                        Vec::new()
                    }
                }
            }
        };
        let mut body = vec![
            format!(".byte {}", byte_list.join(",")),
            format!(".quad {gc_label}"),
            format!(".quad {elem_label}"),
            // methods：无方法表 ⇒ `.quad 0`（`C5` §10：`methods == null` 等价于「没有方法」）。
            //
            // `*T` 要**借用** `T` 的表：`rt_itab(desc(J), desc(*T))` 拿的是动态类型
            // 的 desc，而指针接收者的方法全在 `T` 的表里（`collect_methods` 按
            // 基类型分组）。不借的话 `x.(J)` 里 `x` 装着 `*T` 就永远「没实现」
            // （实测 `ken/interfun.go`：`interface conversion: failed`）。
            format!(
                ".quad {}",
                mt.get(h)
                    .or_else(|| match tt.type_of(*t) {
                        rgoc_types::typ::Type::Pointer(e) => mt.get(&type_hash(tt, *e)),
                        _ => None,
                    })
                    .map_or_else(|| "0".to_string(), std::string::String::clone)
            ),
        ];
        body.extend(reflect_ext);
        asm.rodata_named_raw(&desc_symbol(*h), &body);
    }
    // ── ReflectMeta 发射（`C13` §5.2；M12/Task 30）───────────────────────
    //
    // 顺序在 desc 之后但**先分配下标**（见上面的 `meta_idx_of`）—— desc 头
    // 里的 `meta_idx` 必须在 desc 发射前就定下来。块的发射顺序按 hash 序
    //（与 desc 同序），保证 `.s` 的确定性（`T-GEN-04` 的口径）。
    //
    // `meta_idx` 是**下标**不是指针 ⇒ rt 侧要一张 index→指针 的表
    //（`rt_reflect_metas`，1 起的下标减 1 取）。空表也发（rt.c 无条件
    // extern 引用它，缺符号 = 链接错）。
    let mut meta_labels: Vec<String> = Vec::new();
    // 签名 desc 去重（`__sigdesc_<hash>`）：同一签名只发一块。
    let mut sigs: BTreeMap<u32, String> = BTreeMap::new();
    for (h, t) in metas {
        let l = emit_reflect_meta(
            asm,
            tt,
            h,
            t,
            &meta_idx_of,
            &m.methods,
            &mut sigs,
            &m.pkgpath,
        );
        meta_labels.push(l);
    }
    // ⚠️ rt.c 编译成独立的 `.rt.o`，而 `.s` 单独汇编成另一个 `.o` ——
    // **局部符号不能跨目标文件解析**。所以这两个符号必须 `.global`
    //（先例：`rt_zerobase` / `rt_globals` 同样是 `asm.global` + `*_named_raw`）。
    asm.global("rt_reflect_metas");
    asm.rodata_named_raw(
        "rt_reflect_metas",
        &meta_labels
            .iter()
            .map(|l| format!(".quad {l}"))
            .collect::<Vec<_>>(),
    );
    asm.global("rt_reflect_nmetas");
    asm.rodata_named_raw(
        "rt_reflect_nmetas",
        &[format!(".quad {}", meta_labels.len())],
    );
}

/// 该类型要不要发 `ReflectMeta`（`C13` §5.1：`meta_idx == 0` ⇒ 最小信息）。
///
/// * **具名类型**（含泛型实例 `Box[int]`）：`Name` / `PkgPath` / `String` 需要；
/// * **struct**（含匿名）：`NumField` / `Field(i)` 需要字段表；
/// * **签名**（含匿名 `func(int) string`）：`Value.Call` 需要
///   `params/results` 的 ABI 类（`C13` §5.2，M12/Task 33）；
/// * 其余（基本 / 指针 / 切片 / 数组 / map / chan）：从 desc
///   本体（size / kind / elem）就够 —— 不发，省 `.rodata`。
fn needs_reflect_meta(tt: &TypeTable, t: TypeId) -> bool {
    use rgoc_types::typ::Type;
    matches!(
        tt.type_of(t),
        Type::Named(_) | Type::Struct(_) | Type::Signature(_)
    )
}

/// 反射元数据块符号（`C13` §5.4：desc 扩展区与块本体共用这一个名字）。
#[must_use]
pub fn meta_symbol(hash: u32) -> String {
    format!("__reflectmeta_{hash:08x}")
}

/// 把一个类型的 `ReflectMeta`（含字段表、方法表与字符串）发射进 `.rodata`，
/// 返回块符号名（给 `rt_reflect_metas` 表用）。
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn emit_reflect_meta(
    asm: &mut Asm,
    tt: &TypeTable,
    h: u32,
    t: TypeId,
    meta_idx_of: &BTreeMap<u32, u16>,
    methods: &[rgoc_ssa::ir::MethodSet],
    sigs: &mut BTreeMap<u32, String>,
    pkgpath: &str,
) -> String {
    use rgoc_types::typ::Type;
    let cstr = |asm: &mut Asm, s: &str| {
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        asm.rodata_bytes(&bytes)
    };
    // `name`：具名类型渲染实例名（`Box[int]`）；匿名 struct 是空串。
    // `pkgpath`：发射面取**包名**（跨包进 HIR 之前恒为 `main`）。
    let name = match tt.type_of(t) {
        Type::Named(_) => reflect_type_name(tt, t),
        _ => String::new(),
    };
    let name_label = cstr(asm, &name);
    let pkg_label = cstr(asm, pkgpath);

    // 字段表（只有 struct 有；具名非 struct 的 `nfields` 是 0）。
    //
    // ⚠️ 具名 struct 的 `tt.type_of(t)` 是 `Type::Named` —— 必须**先取
    // underlying**（`G-M12-01` 同族坑：`core_type` 当年就在 `Named` 上
    // 翻过车）。匿名 struct 才直接是 `Type::Struct`。
    let (fields_label, nfields) =
        if let Type::Struct(sid) = tt.type_of(rgoc_types::identical::underlying(tt, t)) {
            let def = tt.struct_def(*sid);
            let offsets = rgoc_types::layout::field_offsets(tt, *sid).unwrap_or_default();
            let mut labels: Vec<String> = Vec::new();
            for (i, f) in def.fields.iter().enumerate() {
                let fname = if f.embedded {
                    // 嵌入字段：官方 `Name()` 返回**类型名**（`T` 的 `T`）。
                    f.name.clone().unwrap_or_else(|| {
                        reflect_type_name(tt, f.typ)
                            .rsplit('/')
                            .next()
                            .unwrap_or("")
                            .to_string()
                    })
                } else {
                    f.name.clone().unwrap_or_default()
                };
                let name_l = cstr(asm, &fname);
                let tag_l = cstr(asm, f.tag.as_deref().unwrap_or(""));
                let desc_l = desc_symbol(type_hash(tt, f.typ));
                let exported = fname.starts_with(|c: char| c.is_ascii_uppercase()) && !f.embedded;
                let flags = (u8::from(f.embedded)) | (u8::from(exported) << 1);
                let mut tail: Vec<u8> = Vec::with_capacity(8);
                let off = offsets.get(i).copied().unwrap_or(0);
                tail.extend_from_slice(&off.to_le_bytes());
                tail.push(flags);
                tail.extend_from_slice(&[0, 0, 0]);
                labels.push(format!(".quad {name_l}"));
                labels.push(format!(".quad {tag_l}"));
                labels.push(format!(".quad {desc_l}"));
                labels.push(format!(
                    ".byte {}",
                    tail.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
                ));
            }
            let n = def.fields.len() as u32;
            let fl = format!("__reflectfields_{h:08x}");
            asm.rodata_named_raw(&fl, &labels);
            (fl, n)
        } else {
            ("0".to_string(), 0)
        };

    let meta_label = meta_symbol(h);
    let idx = meta_idx_of.get(&h).copied().unwrap_or(0);
    debug_assert_ne!(idx, 0, "发射中的 ReflectMeta 必须有非零下标");

    // ── 方法表（`C13` §5.2 的 MethodMeta；M12/Task 32）─────────────────
    //
    // * **具名接口**：方法来自 `InterfaceDef.methods`（sig 是 Signature
    //   TypeId），`fn` 恒 0 —— 接口没有实现。
    // * **具名具体类型**：方法来自 `MethodSet`（`m.methods`，按基类型分组、
    //   名字升序）。⚠️ `MethodSet` 把**指针接收者**的方法也并进基类型
    //   （`collect_methods` 按 `strip_ptr(recv)` 分组 —— itab 要），而官方
    //   的值类型方法集只有值接收者方法 ⇒ 值类型的 meta 要**滤掉**指针
    //   接收者。指针性从 `named_def.methods` 的签名 `recv` 读（实例的
    //   方法表由 `refresh_instance_methods` 填好，签名已实例化）。
    // * 签名 desc：`sig_hash` 编码与 `MethodTable` 同源 —— `Implements`
    //   在运行期比它的 +8（hash）字段，两侧同码才比得起来。
    let mut method_labels: Vec<String> = Vec::new();
    let mut nmethods: u32 = 0;
    let under = rgoc_types::identical::underlying(tt, t);
    match (tt.type_of(t), tt.type_of(under)) {
        (Type::Named(_), Type::Interface(iid)) => {
            let idef = tt.interface_def(*iid);
            for (name, sig) in &idef.methods {
                let name_l = cstr(asm, name);
                let sig_l = match tt.type_of(*sig) {
                    Type::Signature(sid) => {
                        let sd = tt.signature_def(*sid);
                        sig_desc_symbol(
                            asm,
                            tt,
                            sigs,
                            &tuple_types(tt, sd.params),
                            &tuple_types(tt, sd.results),
                        )
                    }
                    _ => "0".to_string(),
                };
                method_labels.push(format!(".quad {name_l}"));
                method_labels.push(format!(".quad {sig_l}"));
                method_labels.push(".quad 0".to_string());
                nmethods += 1;
            }
        }
        (Type::Named(nid), _) => {
            let ptr_recv: BTreeSet<String> = tt
                .named_def(*nid)
                .methods
                .iter()
                .filter(|(_, sig)| {
                    let Type::Signature(sid) = tt.type_of(*sig) else {
                        return false;
                    };
                    matches!(
                        tt.signature_def(*sid).recv.map(|r| tt.type_of(r)),
                        Some(Type::Pointer(_))
                    )
                })
                .map(|(n, _)| n.clone())
                .collect();
            // ⚠️ `MethodSet.ty` 是**具名基类型**（`collect_methods` 按
            // `strip_ptr(recv)` 分组），不是 underlying —— `type Num int`
            // 的方法是挂在 `Num` 下，按 `underlying`（=int）找永远落空。
            let base_h = type_hash(tt, t);
            // 没有方法体（原型不发码的泛型方法、或类型根本没方法）⇒ 空表。
            if let Some(ms) = methods.iter().find(|ms| type_hash(tt, ms.ty) == base_h) {
                for mi in &ms.methods {
                    // 值类型的方法集：官方只含值接收者方法（指针接收者的归 `*T`）。
                    if ptr_recv.contains(&mi.name) {
                        continue;
                    }
                    let name_l = cstr(asm, &mi.name);
                    let sig_l = sig_desc_symbol(asm, tt, sigs, &mi.params, &mi.results);
                    method_labels.push(format!(".quad {name_l}"));
                    method_labels.push(format!(".quad {sig_l}"));
                    method_labels.push(format!(".quad {}", isel::fn_sym(mi.func)));
                    nmethods += 1;
                }
            }
        }
        _ => {}
    }
    let methods_label = if method_labels.is_empty() {
        "0".to_string()
    } else {
        let ml = format!("__reflectmethods_{h:08x}");
        asm.rodata_named_raw(&ml, &method_labels);
        ml
    };

    // ── params / results 的 ABI 类（`C13` §5.2 的 ParamMeta；Task 33）──
    //
    // **编译期算好**（`D-M12-5`：运行时不重算 ABI 分类）：对签名类型的
    // meta，逐形参发 `{typ desc, abi_class}`。类的编码与 `place_args`
    // 的落位一一对应（`X`⇒0 / `V`⇒1 / `Stack`⇒2；`3`=零大小保留 ——
    // rgoc 的约定里零大小形参**照占**一个槽（`place_args` 的 Agg 臂），
    // 按它的结果编码才能让运行期的重放与真调用逐槽对齐）。
    let mut params_label = "0".to_string();
    let mut nparams: u32 = 0;
    let mut results_label = "0".to_string();
    let mut nresults: u32 = 0;
    // `Call` 的 ABI 桥（`C13` §5.5）：只有签名类型有。
    let mut tramp_label = "0".to_string();
    if let Type::Signature(sid) = tt.type_of(t) {
        let sd = tt.signature_def(*sid);
        let params = tuple_types(tt, sd.params);
        let results = tuple_types(tt, sd.results);
        (params_label, nparams) = emit_param_metas(asm, tt, sigs, &params, h, "p");
        (results_label, nresults) = emit_param_metas(asm, tt, sigs, &results, h, "r");
        tramp_label = emit_reflect_trampoline(asm, tt, h, &params, &results);
    }
    asm.rodata_named_raw(
        &meta_label,
        &[
            format!(".quad {name_label}"),
            format!(".quad {pkg_label}"),
            format!(".quad {fields_label}"),
            // +24 nfields u32 / +28 nmethods u32（Task 32 填实）。
            format!(".byte {}", {
                let mut b: Vec<u8> = Vec::with_capacity(8);
                b.extend_from_slice(&nfields.to_le_bytes());
                b.extend_from_slice(&nmethods.to_le_bytes());
                b.iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            }),
            // +32 methods *MethodMeta（Task 32）/ +40 params *ParamMeta（Task 33）。
            format!(".quad {methods_label}"),
            format!(".quad {params_label}"),
            // +48 nparams u32 / +52 nresults u32（Task 33）。
            format!(".byte {}", {
                let mut b: Vec<u8> = Vec::with_capacity(8);
                b.extend_from_slice(&nparams.to_le_bytes());
                b.extend_from_slice(&nresults.to_le_bytes());
                b.iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            }),
            // +56 results *ParamMeta（Task 33）。
            format!(".quad {results_label}"),
            // +64 tramp：`Call` 的 ABI 桥入口（非签名类型为 0；`C13` §5.5）。
            format!(".quad {tramp_label}"),
        ],
    );
    meta_label
}

/// 一组形参/结果的 `ParamMeta` 块（`C13` §5.2；M12/Task 33）。
///
/// 每条 16 字节：`{typ *TypeDesc, abi_class u8, pad[7]}`。类编码与
/// `place_args` 的落位一一对应（`X`⇒0 / `V`⇒1 / `Stack`⇒2；`3`=零大小
/// 保留 —— rgoc 的约定里零大小形参**照占**一个槽（`place_args` 的 Agg
/// 臂），按它的结果编码才能让运行期的重放与真调用逐槽对齐）。
/// 返回（块符号，条数）；空表返回（"0"，条数）—— `nparams` 照实发，
/// `Call` 靠它判参数个数。
fn emit_param_metas(
    asm: &mut Asm,
    tt: &TypeTable,
    _sigs: &mut BTreeMap<u32, String>,
    tys: &[TypeId],
    h: u32,
    tag: &str,
) -> (String, u32) {
    let (locs, _) = crate::abi::place_args(tt, tys);
    let mut labels: Vec<String> = Vec::new();
    for (loc, ty) in locs.iter().zip(tys) {
        let cls: u8 = match loc {
            crate::abi::ArgLoc::X { .. } => 0,
            crate::abi::ArgLoc::V { .. } => 1,
            crate::abi::ArgLoc::Stack { .. } => 2,
        };
        let desc_l = desc_symbol(type_hash(tt, *ty));
        labels.push(format!(".quad {desc_l}"));
        labels.push(format!(".byte {},0,0,0,0,0,0,0", cls));
    }
    let n = tys.len() as u32;
    if labels.is_empty() {
        return ("0".to_string(), n);
    }
    let l = format!("__reflectparams_{h:08x}_{tag}");
    asm.rodata_named_raw(&l, &labels);
    (l, n)
}

/// 反射 `Value.Call` 的 ABI 桥：`__reflecttramp_<hash>`（`C13` §5.5；T-REF-03）。
///
/// # 契约
///
/// ```text
/// void tramp(void *funcval, const void *args, void *results);
/// ```
///
/// * `funcval` 是**闭包块**的地址（`{code, ctx…}`）—— 与 `CalleeRef::Indirect`
///   的 `ldr x16, [x0]` 同一口径；它同时就是 `x0` 里的隐式 ctx（`ctx_abi`
///   的约定：凡是当过值的函数都带 ctx 形参）。所以**真形参从 `x1` 起**。
/// * `args` / `results` 是**缓冲**：第 i 个形参在 `off_i = Σ max(8, 槽宽)`，
///   结果同口径（Go 侧 `slotOf` 用的是同一个公式 —— 两侧一旦分叉，
///   参数就会错位到「读起来像垃圾」的程度）。
///
/// # 为什么按签名发直线代码，而不是在 rt.c 里做通用打包
///
/// 寄存器落位（`place_args`）与类型分类在**编译期**就定了（`D-M12-5`：
/// 运行时不重算 ABI 分类）。通用打包要把「寄存器号」变成数据，再在 C 里
/// 绕一圈回到 `blr` —— 同一件事做两遍，还多一份运行期分支。
///
/// # 结果寄存器先存到帧里再分发
///
/// 聚合结果 >16 字节时被调方返回的是**堆地址**，分发要 `rt_memcpy`
/// （clobber `x0/x1/x2`）。先把 `x0..x7` / `d0..d7` 整体落帧，后面的
/// 分发就只读这份快照 —— 否则第二个结果读到的就是 memcpy 的残留。
fn emit_reflect_trampoline(
    asm: &mut Asm,
    tt: &TypeTable,
    h: u32,
    params: &[TypeId],
    results: &[TypeId],
) -> String {
    use rgoc_types::typ::Type;
    // 缓冲槽宽：`max(8, slot 宽)`。零大小类型也**照占 8 字节** ——
    // `place_args` 的 Agg 臂照样给它一个寄存器，缓冲侧必须能接下它。
    let buf_slot = |t: TypeId| -> u32 { slot_of(tt, t).map_or(8, |s| s.size).max(8) };
    let mut poff: Vec<u32> = Vec::with_capacity(params.len());
    let mut cur = 0u32;
    for p in params {
        poff.push(cur);
        cur += buf_slot(*p);
    }
    let mut roff: Vec<u32> = Vec::with_capacity(results.len());
    let mut cur = 0u32;
    for r in results {
        roff.push(cur);
        cur += buf_slot(*r);
    }
    // 落位：第 0 个是 ctx 占位（`x0`），真形参从下标 1 起。
    let mut want: Vec<TypeId> = vec![TypeId::from_index(0)];
    want.extend_from_slice(params);
    let (locs, stackbytes) = crate::abi::place_args(tt, &want);
    let save_x = stackbytes + 32;
    let save_d = save_x + 64;
    let frame = (save_d + 64 + 15) & !15;
    // 浮点细分：`place_args` 只给到 `V` 族，`s`/`d` 与实虚部个数在这里定。
    let fkind = |t: TypeId| -> u8 {
        match tt.type_of(rgoc_types::identical::underlying(tt, t)) {
            Type::Basic(rgoc_types::BasicId::Float32) => 1,
            Type::Basic(rgoc_types::BasicId::Complex64) => 2,
            Type::Basic(rgoc_types::BasicId::Complex128) => 3,
            _ => 0,
        }
    };
    let sym = format!("__reflecttramp_{h:08x}");
    asm.label(&sym);
    asm.comment(&format!("reflect Call 的 ABI 桥（sig {h:08x}）"));
    asm.ins(&format!("sub sp, sp, #{frame}"));
    asm.ins(&format!("stp x29, x30, [sp, #{stackbytes}]"));
    asm.ins(&format!("stp x19, x20, [sp, #{}]", stackbytes + 16));
    asm.ins("mov x29, sp");
    asm.ins("mov x20, x1"); // 参数缓冲（x1 后面会被形参寄存器覆盖）
    asm.ins("mov x19, x2"); // 结果缓冲
    asm.ins("mov x16, x0"); // funcval（同时留在 x0 当 ctx）
    asm.ins("ldr x16, [x16]");
    for (i, loc) in locs.iter().enumerate().skip(1) {
        let ty = params[i - 1];
        let off = poff[i - 1];
        match loc {
            crate::abi::ArgLoc::X { reg, n } => {
                if slot_of(tt, ty).is_some_and(|s| s.class == SlotClass::Agg) {
                    asm.ins(&format!("add x{reg}, x20, #{off}"));
                } else {
                    asm.ins(&format!("ldr x{reg}, [x20, #{off}]"));
                    if *n > 1 {
                        asm.ins(&format!("ldr x{}, [x20, #{}]", reg + 1, off + 8));
                    }
                }
            }
            crate::abi::ArgLoc::V { reg, .. } => match fkind(ty) {
                1 => asm.ins(&format!("ldr s{reg}, [x20, #{off}]")),
                2 => {
                    asm.ins(&format!("ldr s{reg}, [x20, #{off}]"));
                    asm.ins(&format!("ldr s{}, [x20, #{}]", reg + 1, off + 4));
                }
                3 => {
                    asm.ins(&format!("ldr d{reg}, [x20, #{off}]"));
                    asm.ins(&format!("ldr d{}, [x20, #{}]", reg + 1, off + 8));
                }
                _ => asm.ins(&format!("ldr d{reg}, [x20, #{off}]")),
            },
            crate::abi::ArgLoc::Stack { off: soff } => {
                if slot_of(tt, ty).is_some_and(|s| s.class == SlotClass::Agg) {
                    asm.ins(&format!("add x9, x20, #{off}"));
                    asm.ins(&format!("str x9, [sp, #{soff}]"));
                } else {
                    let n = crate::abi::reg_count(tt, ty).unwrap_or(1).max(1);
                    for k in 0..n {
                        asm.ins(&format!("ldr x9, [x20, #{}]", off + k * 8));
                        asm.ins(&format!("str x9, [sp, #{}]", soff + k * 8));
                    }
                }
            }
        }
    }
    asm.ins("blr x16");
    for k in 0..4 {
        asm.ins(&format!(
            "stp x{}, x{}, [sp, #{}]",
            2 * k,
            2 * k + 1,
            save_x + 16 * k
        ));
        asm.ins(&format!(
            "stp d{}, d{}, [sp, #{}]",
            2 * k,
            2 * k + 1,
            save_d + 16 * k
        ));
    }
    // 结果分发：**逐类独立计数**（与 `isel::take_results` 同一张表）。
    let (mut xi, mut vi) = (0u32, 0u32);
    for (j, r) in results.iter().enumerate() {
        let off = roff[j];
        let s = slot_of(tt, *r);
        match s.map(|s| s.class) {
            Some(SlotClass::Complex64) => {
                asm.ins(&format!("ldr s9, [sp, #{}]", save_d + 8 * vi));
                asm.ins(&format!("str s9, [x19, #{off}]"));
                asm.ins(&format!("ldr s10, [sp, #{}]", save_d + 8 * (vi + 1)));
                asm.ins(&format!("str s10, [x19, #{}]", off + 4));
                vi += 2;
            }
            Some(SlotClass::Float) => {
                let reg = if fkind(*r) == 1 { "s9" } else { "d9" };
                asm.ins(&format!("ldr {reg}, [sp, #{}]", save_d + 8 * vi));
                asm.ins(&format!("str {reg}, [x19, #{off}]"));
                vi += 1;
            }
            Some(SlotClass::Complex128) => {
                asm.ins(&format!("ldr d9, [sp, #{}]", save_d + 8 * vi));
                asm.ins(&format!("str d9, [x19, #{off}]"));
                asm.ins(&format!("ldr d10, [sp, #{}]", save_d + 8 * (vi + 1)));
                asm.ins(&format!("str d10, [x19, #{}]", off + 8));
                vi += 2;
            }
            _ => {
                let size = s.map_or(8, |s| s.size);
                if size > 16 {
                    // 被调方返回**堆地址**（`isel::return_values` 同口径）。
                    asm.ins(&format!("ldr x1, [sp, #{}]", save_x + 8 * xi));
                    asm.ins("mov x0, x19");
                    if off > 0 {
                        asm.ins(&format!("add x0, x0, #{off}"));
                    }
                    isel::load_imm(asm, "x2", i64::from(size));
                    asm.ins("bl rt_memcpy");
                    xi += 1;
                } else if size > 8 {
                    asm.ins(&format!("ldr x9, [sp, #{}]", save_x + 8 * xi));
                    asm.ins(&format!("str x9, [x19, #{off}]"));
                    asm.ins(&format!("ldr x9, [sp, #{}]", save_x + 8 * (xi + 1)));
                    asm.ins(&format!("str x9, [x19, #{}]", off + 8));
                    xi += 2;
                } else {
                    asm.ins(&format!("ldr x9, [sp, #{}]", save_x + 8 * xi));
                    asm.ins(&format!("str x9, [x19, #{off}]"));
                    xi += 1;
                }
            }
        }
    }
    asm.ins(&format!("ldp x19, x20, [sp, #{}]", stackbytes + 16));
    asm.ins(&format!("ldp x29, x30, [sp, #{stackbytes}]"));
    asm.ins(&format!("add sp, sp, #{frame}"));
    asm.ins("ret");
    sym
}

/// 方法签名的**描述块**（kind=Func 的 40 字节 desc；`C13` §5.2 的
/// `MethodMeta.sig` 指它）。
///
/// 它不进 `type_closure`（方法签名不是值类型），所以 `emit_descs` 的
/// 主循环看不见它 —— 这里按需发、按 `sig_hash` 去重。布局照 `C5` §10：
/// `size=8`（函数值是一个字）、`hash` 就是 `sig_hash` 编码 —— 运行期的
/// `Implements` 读 +8 比签名，两侧同码是硬前提。
fn sig_desc_symbol(
    asm: &mut Asm,
    tt: &TypeTable,
    sigs: &mut BTreeMap<u32, String>,
    params: &[TypeId],
    results: &[TypeId],
) -> String {
    let h = sig_hash(tt, params, results);
    if let Some(l) = sigs.get(&h) {
        return l.clone();
    }
    let sym = format!("__sigdesc_{h:08x}");
    let mut head: Vec<u8> = Vec::with_capacity(16);
    head.extend_from_slice(&8u32.to_le_bytes()); // +0 size（函数值 8 字节）
    head.extend_from_slice(&8u32.to_le_bytes()); // +4 ptrdata
    head.extend_from_slice(&h.to_le_bytes()); // +8 hash
    head.push(19); // +12 kind = Func（官方枚举序）
    head.push(0); // +13 flags
    head.extend_from_slice(&0u16.to_le_bytes()); // +14 meta_idx
    asm.rodata_named_raw(
        &sym,
        &[
            format!(
                ".byte {}",
                head.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
            ),
            ".quad 0".to_string(), // +16 gcdata
            ".quad 0".to_string(), // +24 elem
            ".quad 0".to_string(), // +32 methods
        ],
    );
    sigs.insert(h, sym.clone());
    sym
}

/// 官方 `reflect.Type.String()` 形态的类型名渲染（`C13` §4：实参按 **Go
/// 语法**渲染，实例是 `Box[int]`）。
///
/// 只服务 ReflectMeta 的 `name` 字段 —— 不追求与官方**逐字节**一致（那是
/// `T-REF-05` 的账），但形状要对：具名带实参、指针带 `*`、切片 `[]`、
/// map / chan / 数组 / struct 按官方的空格习惯。
fn reflect_type_name(tt: &TypeTable, t: TypeId) -> String {
    use rgoc_types::typ::Type;
    match tt.type_of(t) {
        Type::Basic(b) => {
            let s = b.as_str();
            // 无类型常量按默认类型渲染（官方没有 untyped 的运行期类型）。
            match s {
                "untyped_bool" => "bool".to_string(),
                "untyped_int" => "int".to_string(),
                "untyped_rune" => "int32".to_string(),
                "untyped_float" => "float64".to_string(),
                "untyped_complex" => "complex128".to_string(),
                "untyped_string" => "string".to_string(),
                "unsafe_pointer" => "unsafe.Pointer".to_string(),
                _ => s.to_string(),
            }
        }
        Type::Named(nid) => {
            let def = tt.named_def(*nid);
            if def.targs.is_empty() {
                def.name.clone()
            } else {
                let args: Vec<String> = def
                    .targs
                    .iter()
                    .map(|&a| reflect_type_name(tt, a))
                    .collect();
                format!("{}[{}]", def.name, args.join(", "))
            }
        }
        Type::Alias(id) => reflect_type_name(tt, tt.alias_rhs(*id)),
        Type::Pointer(e) => format!("*{}", reflect_type_name(tt, *e)),
        Type::Slice(e) => format!("[]{}", reflect_type_name(tt, *e)),
        Type::Array { elem, .. } => {
            // len 是常量表下标；数组长度渲染按十进制数值。这里没有常量表
            // 的取值口 —— 用语料里唯一可行的途径：数组长度的 hash 无关紧要，
            // 名字走 desc 的 hash 而不是这段渲染。发射面只对「进 ReflectMeta
            // 的类型」调它，数组长度渲染错了会被 `T-REF-05` 抓。
            let n = rgoc_types::layout::array_len_of(tt, t).unwrap_or(0);
            format!("[{n}]{}", reflect_type_name(tt, *elem))
        }
        Type::Map { key, elem } => format!(
            "map[{}]{}",
            reflect_type_name(tt, *key),
            reflect_type_name(tt, *elem)
        ),
        Type::Chan { dir, elem } => match dir {
            rgoc_types::ChanDir::RecvOnly => format!("<-chan {}", reflect_type_name(tt, *elem)),
            rgoc_types::ChanDir::SendOnly => format!("chan<- {}", reflect_type_name(tt, *elem)),
            rgoc_types::ChanDir::SendRecv => format!("chan {}", reflect_type_name(tt, *elem)),
        },
        Type::Struct(sid) => {
            let def = tt.struct_def(*sid);
            if def.fields.is_empty() {
                return "struct {}".to_string();
            }
            let parts: Vec<String> = def
                .fields
                .iter()
                .map(|f| {
                    let n = f.name.clone().unwrap_or_default();
                    format!("{n} {}", reflect_type_name(tt, f.typ))
                })
                .collect();
            format!("struct {{ {} }}", parts.join("; "))
        }
        Type::Signature(sid) => {
            // 官方渲染 `func(int) string`（无名参数形态）。
            let def = tt.signature_def(*sid);
            let render_tuple = |tid: rgoc_types::TypeId| -> Vec<String> {
                match tt.type_of(tid) {
                    rgoc_types::typ::Type::Tuple(tid2) => tt
                        .tuple_def(*tid2)
                        .vars
                        .iter()
                        .map(|(_, t)| reflect_type_name(tt, *t))
                        .collect(),
                    _ => Vec::new(),
                }
            };
            let pl = render_tuple(def.params);
            let rl = render_tuple(def.results);
            match rl.len() {
                0 => format!("func({})", pl.join(", ")),
                1 => format!("func({}) {}", pl.join(", "), rl[0]),
                _ => format!("func({}) ({})", pl.join(", "), rl.join(", ")),
            }
        }
        Type::Interface(_) => "interface {}".to_string(),
        _ => String::new(),
    }
}

/// `TypeDesc.elem` 该指谁（`None` ⇒ 发 `.quad 0`）。
fn elem_of(tt: &TypeTable, t: TypeId) -> Option<TypeId> {
    use rgoc_types::typ::Type;
    match tt.type_of(t) {
        Type::Pointer(e) | Type::Slice(e) | Type::Array { elem: e, .. } => Some(*e),
        // map / chan / struct / signature / iface / 标量：设计 §3.2 说「其余为 null」。
        // map 的 K/V desc 由 `isel` 按 `desc_symbol(type_hash(...))` 直接引用。
        _ => None,
    }
}

/// 渲染整个模块的 `FRAME-MAP v1`（`C5` §8）。
///
/// # 为什么要单独一份，而不是塞进 `.s` 的注释
///
/// `C5` §8 冻结的行前缀是 `#`，而 AArch64 的 GNU as 把 `#` 当**立即数前缀**
/// （注释是 `//`）。塞进 `.s` 要么改契约，要么让产物汇编不过。
///
/// 所以它是**并列的产物**：`<out>.s` 旁边一份 `<out>.framemap`。
/// 消费者（M10 的栈扫描器）要的是这份表，不是 `.s`。
///
/// # Errors
///
/// 帧布局失败（某个类型的静态类型没有运行期布局）。
pub fn frame_maps(m: &Module, tt: &TypeTable) -> Result<String, CompileError> {
    use std::fmt::Write as _;
    let mut out = String::new();
    // 用与 `compile` **同一套** sibling 表：`outgoing` 的大小依赖被调方的形参表
    // （`R-13-3`），帧图与产物必须算在同一份帧上，否则「表里的偏移」对不上「代码里的偏移」。
    let siblings: std::collections::BTreeMap<rgoc_hir::FuncId, &rgoc_ssa::ir::SsaFunc> =
        m.funcs.iter().map(|f| (f.src, f)).collect();
    for f in &m.funcs {
        let fr = frame::layout_with(f, tt, &siblings)
            .map_err(|e| CompileError::Frame(e, f.name.clone()))?;
        let cfg = rgoc_ssa::cfg::Cfg::build(f);
        let lv = rgoc_ssa::live::analyze(f, &cfg, tt);
        out.push_str(&frame::render_frame_map(f, &fr, &lv));
    }
    let _ = writeln!(out, "# end");
    Ok(out)
}

// ── STACK-MAP v1（`M9-design.md` §3.11；门禁 `ROOT-1`）──────────────────

/// 协议版本行（改动等于契约变更）。
pub const STACKMAP_VERSION: &str = "STACK-MAP v1";

/// 一个 safepoint 的位图：`words[i]` 的第 `k` 位 ↔ 帧内地址
/// `fp - frame_size + 8*(64*i + k)`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpBitmap {
    /// 位点所在的块。
    pub block: u32,
    /// 块内指令下标；`== instrs.len()` 表示**终止符**（回边）。
    pub idx: usize,
    /// 活值个数（信息性；位图才是根的真集）。
    pub live: u32,
    /// 位图（字为单位）。
    pub words: Vec<u64>,
}

/// 一个函数的栈图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuncStackMap {
    /// 在 `Module::funcs` 里的下标（M9 只用它做「函数索引」）。
    pub index: usize,
    /// 二进制里的符号（`fn<FuncId>`）—— `FRAME-MAP` 的 `sym=` 同一口径。
    pub sym: String,
    /// 帧字节数（与 `FRAME-MAP` 的 `frame=` 同一个数）。
    pub frame_size: u32,
    /// 逐 safepoint（顺序 = `live` 的排序键：块号 + 指令下标）。
    pub sps: Vec<SpBitmap>,
}

impl FuncStackMap {
    /// 每个 safepoint 的位图占几个字。
    ///
    /// ⚠️ 一位 = 一个 8 字节字，所以位数是 `frame_size / 8`，不是 `frame_size`。
    /// 写成 `frame_size.div_ceil(64)` 会把 656 字节的帧算成 11 个字（实际 2 个）——
    /// 于是 `off` 的步长与真实位图宽度对不上，M10 会读到别的 safepoint 的位。
    #[must_use]
    pub fn words_per_sp(&self) -> usize {
        (self.frame_size as usize / 8).div_ceil(64)
    }

    /// 拼成一维的 `sp_bits` 流（第 `i` 个 safepoint 从 `i * words_per_sp` 起）。
    #[must_use]
    pub fn flat_bits(&self) -> Vec<u64> {
        let mut out = Vec::with_capacity(self.sps.len() * self.words_per_sp());
        for sp in &self.sps {
            out.extend_from_slice(&sp.words);
        }
        out
    }
}

/// 算一个函数的栈图。
///
/// # 位序（`C5` §10 冻结，两侧都按它读）
///
/// 位图覆盖整个帧区间 `[fp - frame_size, fp)`，bit `i` ↔ `fp - frame_size + 8*i`，
/// bit 0 = 帧内最低地址。**含** outgoing 区（栈实参里有指向堆的指针）与
/// tmp 区；**不含** saved `fp`/`lr`（它们在 `[fp-16, fp)`，那两位永远写 0）。
///
/// # 标哪几处
///
/// 1. 每个活值按 `ptr_words`（`live.rs`，来源 `ptr_mask`）标它的根字；
/// 2. **调用位点**的 outgoing 区：栈实参里是指针的那些字。
///
/// 帧里的 `addr_slots`（`&x` 的存储）**不单独标**：它的内容在「有人能读到」
/// 时必然以某个活值的 `Load` / `AddrOf` 形式出现；而逃逸槽在帧里只剩一个
/// arena 指针（不是堆地址），标它会让 M10 去追一块不由 GC 管的存储。
///
/// # 一个刻意的保守
///
/// 聚合实参在栈上是**指向本帧的指针**（`C5` §6.1 按指针传），不是堆根。
/// 仍然标它（1 位）：M10 扫到不在堆里的地址会忽略，而漏标的代价是「活对象
/// 被回收」。代价不对称时选保守那一侧。
#[must_use]
pub fn stack_map_of(
    index: usize,
    f: &SsaFunc,
    fr: &crate::frame::Frame,
    lv: &rgoc_ssa::live::Liveness,
    tt: &TypeTable,
    siblings: &BTreeMap<rgoc_hir::FuncId, &SsaFunc>,
) -> FuncStackMap {
    let bits = fr.frame_size as usize / 8;
    let words_per_sp = bits.div_ceil(64);
    let mut sps = Vec::with_capacity(lv.safepoints.len());
    for sp in &lv.safepoints {
        let mut words = vec![0u64; words_per_sp];
        let set = |bit: usize, words: &mut Vec<u64>| {
            debug_assert!(bit < bits, "位下标 {bit} 超出帧（{bits} 位）");
            if bit < bits {
                words[bit / 64] |= 1 << (bit % 64);
            }
        };
        for (v, w) in &sp.ptr_words {
            let Some(slot) = fr.values.get(v) else {
                continue;
            };
            // 槽偏移是**相对 fp 的负数**；帧内低位在 `fp - frame_size`。
            let base = i64::from(fr.frame_size) + i64::from(slot.off);
            debug_assert!(
                base >= 0,
                "槽偏移 {} 超出帧（frame_size = {}）",
                slot.off,
                fr.frame_size
            );
            if base >= 0 {
                set(base as usize / 8 + usize::from(*w), &mut words);
            }
        }
        // ── 取址槽：**保守标注**（`R-10-14`）──
        //
        // # 为什么必须单独标
        //
        // `live.rs` 的活跃性只覆盖 **`ValueId`**，而 `var` 声明的局部变量在
        // SSA 里是 **slot**（`Load`/`Store` 访存），它的值不在值流里 ⇒
        // **无法逐位点**。
        //
        // 漏标的代价是「活对象被回收」（不可接受）；误标的代价只是
        // `bad_ptr++`（`mark_ptr` 对非块首地址会忽略）。所以按 `ptr_mask`
        // **全部标注** —— 保守，但只标**真正可能含指针的字**。
        //
        // ⚠️ `uintptr` / `int` 那些字的 mask 是 0，不会被误标。这条边界很重要：
        // `T-PREC-01` 的 `u := uintptr(p) + 8` 是**值**（不是 slot），仍走逐位点
        // ⇒ 不会被当根；而 `var u uintptr` 的 mask 是 0 ⇒ 也不会被当根。
        //
        // 实测（M10/Phase 2）：`ken/interbasic.go` 的 `var ia, ib I0` 正是
        // slot —— 不标它们，接口的 `data` 被回收后两块复用成同一块，
        // `if ia == ib` 就成立了（`panic("4")`）。
        for (slot, s) in &fr.addr_slots {
            let base_off = i64::from(fr.frame_size) + i64::from(s.off);
            if fr.heap.contains_key(slot) {
                // 逃逸槽：帧里只有**一个指针格**，存储本身在别处（`R-13-5`）。
                if s.size == 8 && base_off >= 0 {
                    set(base_off as usize / 8, &mut words);
                }
                continue;
            }
            let Some(typ) = f.slot_types.get(slot) else {
                continue;
            };
            let Some(mask) = ptr_mask(tt, *typ) else {
                continue;
            };
            for (i, b) in mask.iter().enumerate() {
                if *b {
                    let base = base_off + 8 * i as i64;
                    if base >= 0 {
                        set(base as usize / 8, &mut words);
                    }
                }
            }
        }
        // 调用位点的 outgoing 区（`[sp, sp+outgoing)`）。
        //
        // `sp = fp - (frame_size - 16)`，所以 `[sp, #k]` 的位下标是 `(16 + k)/8`。
        if let Some(ins) = f.blocks[sp.block.index()].instrs.get(sp.idx)
            && let rgoc_ssa::ir::Instr::Call { callee, args, .. } = ins
        {
            let (stack_args, _) = crate::frame::call_stack_args(f, tt, siblings, callee, args);
            for (off, ty) in stack_args {
                let slot_class = slot_of(tt, ty).map_or(SlotClass::Int, |s| s.class);
                // 聚合在栈上只有**一个指针**（指向调用方的槽），不是它自己的位图。
                let word_ids: Vec<u8> = if slot_class == SlotClass::Agg {
                    vec![0]
                } else {
                    ptr_mask(tt, ty)
                        .map(|m| {
                            m.iter()
                                .enumerate()
                                .filter(|(_, b)| **b)
                                .map(|(i, _)| u8::try_from(i).unwrap_or(u8::MAX))
                                .collect()
                        })
                        .unwrap_or_default()
                };
                for w in word_ids {
                    set((16 + off as usize) / 8 + usize::from(w), &mut words);
                }
            }
        }
        sps.push(SpBitmap {
            block: sp.block.0,
            idx: sp.idx,
            live: u32::try_from(sp.live.len()).unwrap_or(u32::MAX),
            words,
        });
    }
    FuncStackMap {
        index,
        sym: format!("fn{}", f.src.0),
        frame_size: fr.frame_size,
        sps,
    }
}

/// 渲染一个函数的 `STACK-MAP v1` 段。
#[must_use]
pub fn render_stackmap(fsm: &FuncStackMap) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# func {} name={} frame={} safepoints={}",
        fsm.index,
        fsm.sym,
        fsm.frame_size,
        fsm.sps.len()
    );
    let wp = fsm.words_per_sp();
    for (i, sp) in fsm.sps.iter().enumerate() {
        // 位点写 `b<块>:<下标>`，与 `SSA-LIVE v1` 的 `# safepoint b<k> <idx>`
        // **同形**（`R-9-34`）：只写块内下标的话，不同块的同一下标无法区分，
        // 对账与调试都得靠猜。
        let _ = writeln!(
            out,
            "sp b{}:{} off={} live={} ptrs={}",
            sp.block,
            sp.idx,
            i * wp,
            sp.live,
            hex_words(&sp.words)
        );
    }
    out
}

/// 位图的十六进制文本：**按字节下标升序**直排（与 `gcdata` 的字节序一致），
/// 每个字节两位；**不做**大小端翻转 —— 「谁在前」靠这一句定，不靠猜。
fn hex_words(words: &[u64]) -> String {
    let mut s = String::with_capacity(words.len() * 16);
    for w in words {
        for b in w.to_le_bytes() {
            s.push_str(&format!("{b:02x}"));
        }
    }
    s
}

/// 整个模块的 `STACK-MAP v1` 文本（并列产物 `<out>.stackmap`，`Task 59`）。
///
/// # Errors
///
/// 帧布局失败（某个值的静态类型没有运行期布局）。
pub fn stack_maps(m: &Module, tt: &TypeTable) -> Result<String, CompileError> {
    let mut out = String::new();
    out.push_str(STACKMAP_VERSION);
    out.push('\n');
    let siblings: BTreeMap<rgoc_hir::FuncId, &SsaFunc> =
        m.funcs.iter().map(|f| (f.src, f)).collect();
    for (i, f) in m.funcs.iter().enumerate() {
        let fr = frame::layout_with(f, tt, &siblings)
            .map_err(|e| CompileError::Frame(e, f.name.clone()))?;
        let cfg = rgoc_ssa::cfg::Cfg::build(f);
        let lv = rgoc_ssa::live::analyze(f, &cfg, tt);
        let fsm = stack_map_of(i, f, &fr, &lv, tt, &siblings);
        out.push_str(&render_stackmap(&fsm));
    }
    Ok(out)
}

/// 把 `RtFuncInfo` 表编进 `.rodata`（`M10` 的消费面）。
///
/// ```text
/// RtFuncInfo = { frame_size: u32, n_sp: u32, sp_off: *u32, sp_bits: *u64 }
/// ```
///
/// `sp_off[i]` 是第 `i` 个 safepoint 位图在 `sp_bits` 里的**起始字下标**
/// （`R-9-33`）。选「字下标」而不是「位下标」：`sp_off` 的类型是 `*u32`，
/// 而位下标在 4 KiB 帧上会到 4096 —— 还没溢出，但字下标让
/// `sp_bits[sp_off[i] + k]` 直接可写，不用再做除法。
///
/// `n_sp == 0` 时两张表都是 null（沿 `TypeDesc.gcdata` 的「没有根就不发位图」
/// 同一条纪律）：一个指向空标签的指针比 `0` 更难判，且 M10 只判 `n_sp`。
fn emit_stackmap_rodata(asm: &mut Asm, sms: &[FuncStackMap]) {
    for fsm in sms {
        let off_lbl = format!(".Lspoff{}", fsm.index);
        let bits_lbl = format!(".Lspbits{}", fsm.index);
        let mut body = vec![
            format!(".long {}", fsm.frame_size),
            format!(".long {}", fsm.sps.len()),
        ];
        if fsm.sps.is_empty() {
            body.push(".quad 0".to_string());
            body.push(".quad 0".to_string());
        } else {
            let wp = fsm.words_per_sp();
            let offs: Vec<String> = (0..fsm.sps.len())
                .map(|i| format!(".long {}", i * wp))
                .collect();
            asm.rodata_named_raw(&off_lbl, &offs);
            let bits: Vec<String> = fsm
                .flat_bits()
                .iter()
                .map(|w| format!(".quad {w}"))
                .collect();
            asm.rodata_named_raw(&bits_lbl, &bits);
            body.push(format!(".quad {off_lbl}"));
            body.push(format!(".quad {bits_lbl}"));
        }
        asm.rodata_named_raw(&format!("__funcinfo_{}", fsm.index), &body);
    }
    // 全局索引：M10 只拿得到「函数索引」这一个数（按 PC 反查函数归 M10 的
    // pclntab），所以必须有这张表把索引变成 `RtFuncInfo` 的地址。
    let mut body: Vec<String> = sms
        .iter()
        .map(|f| format!(".quad __funcinfo_{}", f.index))
        .collect();
    if body.is_empty() {
        body.push(".quad 0".to_string());
    }
    asm.global("rt_funcs");
    asm.global("rt_nfuncs");
    asm.rodata_named_raw("rt_funcs", &body);
    asm.rodata_named_raw("rt_nfuncs", &[format!(".quad {}", sms.len())]);
}
