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

use std::collections::BTreeMap;

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
    /// 某个值的类型没有运行期布局。
    Frame(FrameError),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cg(c) => write!(f, "{c}"),
            Self::Frame(e) => write!(f, "{e}"),
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
    emit_funcvals(&mut asm, m);
    let mt = emit_method_tables(&mut asm, m, tt, &siblings);
    emit_itabs(&mut asm, m, tt, &siblings);
    emit_descs(&mut asm, m, tt, &mt);
    emit_global_roots(&mut asm, m, tt);

    // ③ 逐函数。
    for f in &m.funcs {
        let fr = frame::layout_with(f, tt, &siblings).map_err(CompileError::Frame)?;
        isel::func(f, &fr, tt, &siblings, &mut asm).map_err(CompileError::Cg)?;
    }

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
    /// `x0 = *(void **)(x0 + off)`
    Load(u32),
    /// `x0 = x0 + off`
    Add(u32),
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
        out.push(Step::Load(0));
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
    asm.label(&sym);
    for st in &steps {
        match st {
            Step::Load(off) => load_off(asm, *off, "ldr x0, [x0, x9]", "ldr x0, [x0, #"),
            Step::Add(off) => add_off(asm, *off),
        }
    }
    asm.ins(&format!("b {}", isel::fn_sym(func)));
    sym
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
        header.extend_from_slice(&[0, 0]); // 对齐填充（`kind` + `flags` = 2 字节）
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
        asm.rodata_named_raw(
            &desc_symbol(*h),
            &[
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
            ],
        );
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
        let fr = frame::layout_with(f, tt, &siblings).map_err(CompileError::Frame)?;
        let cfg = rgoc_ssa::cfg::Cfg::build(f);
        let lv = rgoc_ssa::live::analyze(f, &cfg, tt);
        out.push_str(&frame::render_frame_map(f, &fr, &lv));
    }
    let _ = writeln!(out, "# end");
    Ok(out)
}
