//! SSA 的 IR 形状（`M6-design.md` §3.2；`C10` 规定它的 dump 形态）。
//!
//! # 五个被形状固定下来的决定
//!
//! 1. **phi 是块参数**（`Block::params` + `Term` 边上的实参），不是 phi 指令（`D-M6-3`）。
//!    于是 `V7` 只需数字段长度 —— 结构性可查，不必顺着 CFG 反查 phi 配对。
//! 2. **多返回值是多结果指令**（`Call { dsts }`），tuple **不进 IR**（`D-M6-4`）；
//!    `return f()` / `g(f())` 由构建器做「结果直连」，不需要打包/拆包。
//! 3. **访存的根是显式字段**（`Load/Store { root }`），不从地址反推（`D-M6-5/6`）——
//!    否则 verifier 要重跑一遍别名分析才能查 `V6`。
//! 4. **`Store` 产出新 mem**（`dst`）：同根保序靠 mem 链，不靠「指令顺序」这种口头约定。
//! 5. **结果不是 `SsaFunc` 的字段**，而是 `Term::Return(Vec<ValueId>)` 的操作数 ——
//!    返回值就是值，不需要额外槽。
//!
//! # 「变体穷尽」怎么保证
//!
//! `kind_of_*` 一律写成**不带通配的 `match`**：加一个变体，编译立刻逼你补名字；
//! 再配合 `tests/ir_shape.rs` 的冻结清单，新变体必须回来决定它在 dump / 构建 /
//! 求值器里的位置（`T-SSA-01`）。

use std::collections::BTreeMap;

use rgoc_hir::{BinOp, BuiltinKind, FuncId, GlobalId, SlotId, UnOp, types::ConstVal};
use rgoc_types::TypeId;

/// 值下标（`SsaFunc` 内唯一）。
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ValueId(pub u32);

impl ValueId {
    /// 转成 `Vec` 下标。
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// 块下标（`SsaFunc::blocks` 的索引）。
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct BlockId(pub u32);

impl BlockId {
    /// 转成 `Vec` 下标。
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// 内存根下标（`SsaFunc::roots` 的索引）。
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RootId(pub u32);

impl RootId {
    /// 转成 `Vec` 下标。
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// 一个内存根（`D-M6-5`：按根分区，根是**构造成分**而不是别名推理的产物）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Root {
    /// 包级变量。
    Global(GlobalId),
    /// **取过地址**的帧槽（没取过址的槽会被提升成值，不建根）。
    Slot(SlotId),
    /// 堆：map / slice 底层 / 复合字面量逃逸 —— 当前用一个根代表全部。
    Heap,
}

/// 被调函数的签名摘要（`V5` 的比对面）。
///
/// # 为什么记在这个函数里，而不是模块级符号表
///
/// `V5` 要判「`Call.dsts.len()` == 被调方的结果数」。`CalleeRef::Func(FuncId)` 只是
/// 一个 HIR 编号，本 crate 看不到 HIR 的 `Program`。两条路：把模块级符号表传进
/// `verify`（会污染每个签名），或者让**每个函数记下自己调到的签名**。
///
/// 选后者：`SsaFunc` 因此**自足**（dump 确定性、单函数测试、后续 passes 都不必
/// 拖着模块级上下文）。代价是同被调方在多个调用者里各记一份 —— 一个 `usize`
/// 三元组的量级，换掉一整个上下文参数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sig {
    /// 形参个数（**含**变参那一个）。
    pub params: usize,
    /// 结果个数。
    pub results: usize,
    /// 是否变参（`args.len()` 允许 ≥ `params - 1`）。
    pub variadic: bool,
}

/// 一个模块（单包；`D-M6-2`）。
#[derive(Clone, Debug, Default)]
pub struct Module {
    /// 包路径（`ReflectMeta.pkgpath` 用；M12/Task 30）。
    pub pkgpath: String,
    /// 全部函数（顺序 = HIR 的 `Program::funcs` 顺序，dump 与求值都靠它保持确定性）。
    pub funcs: Vec<SsaFunc>,
    /// 包级变量（与 HIR 的 `GlobalId` 同编号空间）。
    pub globals: Vec<GlobalId>,
    /// 每个包级变量的类型（与 `globals` 同序）。
    ///
    /// # 为什么求值器需要它
    ///
    /// 全局的存储要在跑 `init` 之前**按类型建零值**：`var a [3]int` 的
    /// `a[1] = 7` 走 `Index(AddrOf(r_g))`，若那块存储是 `nil`，取下标当场失败。
    /// 而 `Root::Global` 只有一个 `GlobalId`，类型信息不在 IR 里 ——
    /// 补这张表，比让求值器回头去问 HIR 便宜得多（`D-M6-2` 的单包前提）。
    pub global_types: Vec<TypeId>,
    /// `main`（可执行程序必有）。
    pub main: Option<FuncId>,
    /// **包初始化函数**在 `funcs` 里的下标（`Task 32b`）。
    ///
    /// # 它是什么
    ///
    /// HIR 的包级初始化**不在任何函数体里**：`Program::init_order` 是
    /// 「`var g = f()` 的右侧表达式」的列表，解释器在跑 `main` 之前逐组求值。
    /// SSA 侧必须把它也变成图 —— 否则全局永远是零值，而**没有任何一处会报错**
    /// （实测：`var g = 7` 的 SSA 模块里 `main` 读到 0）。
    ///
    /// 所以构建期合成一个无参无结果的函数（`name == "init"`，`src` 是哨兵
    /// [`SYNTHETIC_FUNC`]）：按 `init_order` 顺序写全局根，再依次调用各文件的
    /// `init()`，最后 `Return`。执行顺序由 `ssa_exec` 保证：**init → main**。
    ///
    /// # 与 mem 种子的关系
    ///
    /// mem 值是**函数内**的：init 里的 `Store` 产出的是 init 的 mem，`main` 看不到它。
    /// 这没问题 —— mem 种子表示「进入本函数时的内存状态」，而 init 一定先跑完。
    /// 跨函数的内存效果由 `Call.effect` 表达，不是 mem 链。
    pub init: Option<usize>,
    /// 每个**有方法的具名类型**一块方法表（`M9/Task 27`）。
    ///
    /// 顺序按 `type_hash` 升序 —— 发射的 `.rodata` 与它逐条对应，顺序必须是确定的。
    pub methods: Vec<MethodSet>,
    /// 每个 `(接口, 具体类型)` 对一张静态 ITab（`M9/Task 28`）。
    ///
    /// 顺序按 `(iface_hash, concrete_hash)` 升序，理由同上。
    pub itabs: Vec<ItabDef>,
}

/// 沿**嵌入字段**下降到声明类型的一跳。
///
/// # 为什么在 IR 里记这个
///
/// 接口派发时 `data` 指向**动态类型**的盒子，而方法要的接收者是**声明类型**的
/// 那份存储。中间差多少，由嵌入链决定：`struct{ *U }` 要 `load`、`struct{ U }`
/// 要 `add`。这是纯**布局**事实（不依赖 `SlotClass`），所以归 IR；
/// 而「最后要不要把值读出来」（标量 vs 聚合）才是 ABI 知识，归 `rgoc-arm64`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hop {
    /// 字段在结构体里的**下标**（求值器按它下钻 `Value`）。
    pub index: usize,
    /// 字段在结构体里的字节偏移。
    pub off: u32,
    /// 那个嵌入字段是不是**指针**（`struct{ *U }` ⇒ 要 `load`，否则 `add`）。
    pub via_ptr: bool,
}

/// 一个具体类型的**一个方法实现**（`MethodTable` 的一条）。
#[derive(Clone, Debug)]
pub struct MethodImpl {
    /// 方法名（不含类型前缀）。
    pub name: String,
    /// 形参类型（**不含**接收者）。
    pub params: Vec<TypeId>,
    /// 结果类型。
    pub results: Vec<TypeId>,
    /// 方法体。
    pub func: FuncId,
    /// 从**本类型的存储**到**声明类型的存储**的嵌入链（空 ⇒ 方法声明在本类型上）。
    pub hops: Vec<Hop>,
}

/// 一个具体类型的方法集（`MethodTable` 一整块）。
#[derive(Clone, Debug)]
pub struct MethodSet {
    /// 具体类型（`T`，不是 `*T`）。
    pub ty: TypeId,
    /// 方法，**按名字升序**（`MethodTable` 的条目序；查表靠它做二分也好、
    /// 线性扫也好，顺序必须确定）。
    pub methods: Vec<MethodImpl>,
}

/// 一个 `(接口, 具体类型)` 对的静态 ITab（`M9/Task 28`）。
#[derive(Clone, Debug)]
pub struct ItabDef {
    /// 接口类型。
    pub iface: TypeId,
    /// 动态类型（可能是 `*T`）。
    pub concrete: TypeId,
    /// `fn[]`：按**接口的方法序**（见 [`iface_methods`]）逐个填。
    pub fns: Vec<ItabEntry>,
}

/// ITab 的一个槽。
#[derive(Clone, Debug)]
pub struct ItabEntry {
    /// 真实方法（`itab.fn[i]` 可能指它，也可能指一段 `__imeth_` 尾部跳转桩 ——
    /// 「要不要桩」是 **ABI 知识**（`SlotClass` / 嵌入偏移），归 `rgoc-arm64`）。
    pub func: FuncId,
    /// 嵌入链（见 [`Hop`]）。
    pub hops: Vec<Hop>,
}

/// 在 `from`（或它的嵌入字段链）上找方法 `name`，返回 `(FuncId, 嵌入链)`。
///
/// # 为什么要走嵌入
///
/// `method5.go` 的 `T1 ⊃ T2 ⊃ *T3 ⊃ *T4` 里 `M` 声明在 `T4` 上，而接口的动态类型
/// 是 `T1`。只查 `(T1, "M")` 会查不到 —— 然后填一个 `FuncId(u32::MAX)` 进 itab，
/// 运行期跳到垃圾地址。
///
/// # 两个消费者必须同源
///
/// `rgoc-arm64` 用它决定「`data` 要加多少偏移」（发射 thunk），
/// `rgoc-interp::ssa_exec` 用它决定「接收者沿链取到哪」（执行）。
/// 抄两份的话 `--via=ssa` 与 native 会在嵌入类型上分叉。
#[must_use]
pub fn resolve_method(
    tt: &rgoc_types::TypeTable,
    methods: &[MethodSet],
    from: rgoc_types::TypeId,
    name: &str,
) -> Option<(FuncId, Vec<Hop>)> {
    let mut hops = Vec::new();
    let fid = walk_method(tt, methods, from, name, &mut hops, 0)?;
    Some((fid, hops))
}

fn walk_method(
    tt: &rgoc_types::TypeTable,
    methods: &[MethodSet],
    cur: rgoc_types::TypeId,
    name: &str,
    hops: &mut Vec<Hop>,
    depth: u32,
) -> Option<FuncId> {
    if depth > 32 {
        return None;
    }
    let base = strip_ptr(tt, cur);
    if let Some(ms) = methods.iter().find(|m| m.ty == base)
        && let Some(mi) = ms.methods.iter().find(|m| m.name == name)
    {
        return Some(mi.func);
    }
    // 嵌入提升：逐个 `embedded` 字段递归（接收者随之下降一层）
    let sid = match tt.type_of(rgoc_types::underlying(tt, base)) {
        rgoc_types::typ::Type::Struct(sid) => *sid,
        _ => return None,
    };
    let fields = tt.struct_def(sid).fields.clone();
    for (i, f) in fields.iter().enumerate() {
        if !f.embedded {
            continue;
        }
        let off = rgoc_types::layout::field_offset(tt, sid, i)?;
        let via_ptr = matches!(
            tt.type_of(rgoc_types::underlying(tt, f.typ)),
            rgoc_types::typ::Type::Pointer(_)
        );
        hops.push(Hop {
            index: i,
            off,
            via_ptr,
        });
        if let Some(fid) = walk_method(tt, methods, f.typ, name, hops, depth + 1) {
            return Some(fid);
        }
        hops.pop();
    }
    None
}

/// 剥一层指针（`*T` ⇒ `T`；其余原样）。
#[must_use]
pub fn strip_ptr(tt: &rgoc_types::TypeTable, t: rgoc_types::TypeId) -> rgoc_types::TypeId {
    match tt.type_of(t) {
        rgoc_types::typ::Type::Pointer(e) => *e,
        _ => t,
    }
}

/// 接口类型的方法名，**按名字升序**。
///
/// # 为什么是字典序而不是声明序
///
/// 设计稿 §3.8 写的是「`I` 的方法声明序」。实现时发现 `InterfaceDef.methods`
/// 是**检查器插入的顺序**，没有对外承诺（`C7` §4 只承诺 dump 按名字升序）。
/// 槽序**只要两侧一致**就正确（静态 ITab 填序 == 调用点取槽序 == `rt_itab` 查序），
/// 而「插入序」是个内部细节。用字典序把这个细节钉死（`R-9-10`）。
#[must_use]
pub fn iface_methods(tt: &rgoc_types::TypeTable, iface: TypeId) -> Vec<String> {
    use rgoc_types::typ::Type;
    let mut cur = iface;
    for _ in 0..64 {
        match tt.type_of(cur) {
            Type::Named(id) => cur = tt.named_underlying(*id),
            Type::Alias(id) => cur = tt.alias_rhs(*id),
            Type::Interface(iid) => {
                let mut v: Vec<String> = tt
                    .interface_def(*iid)
                    .methods
                    .iter()
                    .map(|(n, _)| n.clone())
                    .collect();
                v.sort();
                return v;
            }
            _ => return Vec::new(),
        }
    }
    Vec::new()
}

/// 合成函数的 `src` 哨兵（`Task 32b`）。
///
/// 包初始化函数在 HIR 里**没有**对应的 `Func`（它由 `Program::init_order` 合成），
/// 而 `SsaFunc::src` 是 `FuncId`。用 `u32::MAX` 当哨兵而不是把它改成 `Option`：
/// 只有诊断定位用它，而哨兵值不会与任何真实下标相撞（`funcs.len()` 远小于它）。
pub const SYNTHETIC_FUNC: FuncId = FuncId(u32::MAX);

/// 一个函数的 SSA 形态。
#[derive(Clone, Debug)]
pub struct SsaFunc {
    /// 回指 HIR 的函数下标（诊断定位用）。
    pub src: FuncId,
    /// 显示名（`main` / `f` / `T.M`）。
    pub name: String,
    /// 函数形参的值（**即入口块的 `params`**，`C10` 规则 2）。
    pub params: Vec<ValueId>,
    /// 全部块（下标即 `BlockId`）。
    pub blocks: Vec<Block>,
    /// 入口块。
    pub entry: BlockId,
    /// 内存根表（下标即 `RootId`）。
    pub roots: Vec<Root>,
    /// 取过地址的槽 → 根（`V9` 查它）。
    pub slot_root: BTreeMap<SlotId, RootId>,
    /// 槽 → 静态类型（只有**取过地址**的槽需要：它们的存储是本帧的根，
    /// 进帧时要按类型建零值）。
    ///
    /// 用 `BTreeMap` 而不是 `Vec`：与 `slot_root` 同形，且不必为没登记的槽
    /// 编一个「占位类型」——那种占位会在运行期变成「零值建错了但没人报」。
    pub slot_types: BTreeMap<SlotId, TypeId>,
    /// `ValueId → TypeId`（长度必须等于值个数；`V3` 查它）。
    pub value_types: Vec<TypeId>,
    /// 本函数**调用到的**已知函数签名（`V5` 查它；只记直接调用与方法调用）。
    pub call_sigs: BTreeMap<FuncId, Sig>,
    /// 本函数的**捕获表**（`M9/Task 38`）。
    ///
    /// 每一条是「外层某槽 ↔ 本帧某槽」：本帧那个槽的存储**不在本帧**，
    /// 而在外层（`CapturePtr` 拿到它的地址）。所以本帧不给它建根。
    pub captured: Vec<rgoc_hir::Capture>,
    /// 调用约定带**隐式第 0 实参（ctx）**吗（`M9/Task 40`）。
    ///
    /// # 谁会带
    ///
    /// * 有捕获的函数（闭包）—— `ctx` 就是 funcval，捕获从 `[ctx+8+8i]` 读；
    /// * **被当作值用过的**函数（`f := add`）—— 它可能经 funcval 被调用，
    ///   而调用点不知道静态目标，所以两边的约定必须一致。
    ///
    /// 带 `ctx` 的函数，整数实参从 `x1` 起（`x0` 留给 ctx）；浮点仍从 `v0` 起
    /// （ctx 是整数类，不占浮点寄存器）。
    pub ctx_abi: bool,
}

impl SsaFunc {
    /// 值个数。
    #[must_use]
    pub fn value_count(&self) -> usize {
        self.value_types.len()
    }

    /// 块个数。
    #[must_use]
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }
}

impl Instr {
    /// 这条指令定义的**值**（`Call` 有多个；其余恰好一个）。
    ///
    /// # 为什么放在 IR 而不是 `opt`
    ///
    /// 「一条指令定义哪些值」是 IR 的**形状**，不是某个 pass 的私有知识。
    /// 活跃性（`live`）与四个 pass（`opt`）都要用同一份答案 —— 抄两份的话，
    /// 新增一个指令变体时必然只改一处，而漏的那一处会安静地把值算错。
    #[must_use]
    pub fn dsts(&self) -> Vec<ValueId> {
        match self {
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
            | Instr::Store { dst, .. } => vec![*dst],
            Instr::Call { dsts, .. } => dsts.clone(),
            Instr::FuncAddr { dst, .. } | Instr::CapturePtr { dst, .. } => vec![*dst],
            // `defer` / `go` 都不定义任何值
            Instr::Defer { .. } | Instr::Go { .. } => Vec::new(),
            // 空指针检查只读地址，不产值
            Instr::NilCheck { .. } => Vec::new(),
            // 捕获是**操作数**不是定义（它们在 `uses` 里）
            Instr::MakeClosure { dst, .. } => vec![*dst],
            // `ok` 也是这条指令**定义**的值（`V1` 要求每个值有定义点）
            Instr::TypeAssert { dst, ok, .. } => {
                let mut out = vec![*dst];
                out.extend(ok.iter().copied());
                out
            }
        }
    }

    /// 这条指令**读**的值（程序顺序；`Call` 的间接被调方与接收者在前）。
    #[must_use]
    pub fn uses(&self) -> Vec<ValueId> {
        let mut out = Vec::new();
        match self {
            Instr::Const { .. } | Instr::AddrOf { .. } => {}
            Instr::Copy { src, .. } => out.push(*src),
            Instr::Bin { lhs, rhs, .. } => {
                out.push(*lhs);
                out.push(*rhs);
            }
            Instr::Un { x, .. } | Instr::Convert { x, .. } => out.push(*x),
            Instr::Field { base, .. } => out.push(*base),
            Instr::Index { base, i, .. } => {
                out.push(*base);
                out.push(*i);
            }
            Instr::Slice {
                base, lo, hi, max, ..
            } => {
                out.push(*base);
                out.extend(lo.iter().chain(hi.iter()).chain(max.iter()).copied());
            }
            Instr::Composite { items, .. } => out.extend(items.iter().copied()),
            Instr::Make { len, cap, .. } => {
                out.extend(len.iter().chain(cap.iter()).copied());
            }
            // `mem` 也是操作数：它是「这条读看在哪个时间点」的令牌（`V6`）
            Instr::Load { mem, addr, .. } => {
                out.push(*mem);
                out.push(*addr);
            }
            Instr::Store { mem, addr, val, .. } => {
                out.push(*mem);
                out.push(*addr);
                out.push(*val);
            }
            Instr::Call { callee, args, .. } => {
                match callee {
                    CalleeRef::Indirect(v) => out.push(*v),
                    CalleeRef::Method { recv, .. } => out.push(*recv),
                    // 接口值也是被读的（`tab` / `data` 两个字都在里面）
                    CalleeRef::Iface { recv, .. } => out.push(*recv),
                    CalleeRef::Func(_) | CalleeRef::Builtin(_) | CalleeRef::Runtime(_) => {}
                }
                out.extend(args.iter().copied());
            }
            // `go` 与 `Call` 读同一套：被调方 + 实参。差别在 isel —— `go` 要
            // 把实参**深拷贝**到新 G 的栈上，`Call` 留在调用方帧里。
            Instr::Go { callee, args, .. } => {
                match callee {
                    CalleeRef::Indirect(v) => out.push(*v),
                    CalleeRef::Method { recv, .. } => out.push(*recv),
                    CalleeRef::Iface { recv, .. } => out.push(*recv),
                    CalleeRef::Func(_) | CalleeRef::Builtin(_) | CalleeRef::Runtime(_) => {}
                }
                out.extend(args.iter().copied());
            }
            Instr::TypeAssert { x, .. } => out.push(*x),
            // `FuncAddr` 只读**符号**，不读值（`func` 是重定位，不是操作数）
            Instr::FuncAddr { .. } => {}
            Instr::Defer { fv } => out.push(*fv),
            Instr::NilCheck { addr } => out.push(*addr),
            // `CapturePtr` 读的是隐式 ctx（不是 `Instr` 的操作数）
            Instr::CapturePtr { .. } => {}
            Instr::MakeClosure { captures, .. } => out.extend(captures.iter().copied()),
        }
        out
    }
}

impl Term {
    /// 终止符读的值（边实参**也算**：它们在跳转时被读走）。
    #[must_use]
    pub fn uses(&self) -> Vec<ValueId> {
        match self {
            Term::Goto { args, .. } => args.clone(),
            Term::If { cond, then_, else_ } => {
                let mut out = vec![*cond];
                out.extend(then_.1.iter().chain(else_.1.iter()).copied());
                out
            }
            Term::Return(vs) => vs.clone(),
            Term::Panic(v) => vec![*v],
            Term::Unreachable => Vec::new(),
        }
    }
}

/// 一个基本块。
#[derive(Clone, Debug, Default)]
pub struct Block {
    /// 块参数（phi 的容身处；入口块里就是函数形参）。
    pub params: Vec<ValueId>,
    /// 指令（程序顺序）。
    pub instrs: Vec<Instr>,
    /// 终止符。
    pub term: Term,
}

/// 一次调用的副作用摘要（`D-M6-9`）。
///
/// `writes` 只可能含 `Root::Global` / `Root::Heap` —— **slot 根是帧内的**，
/// 跨函数不可见，所以不可能出现在被调方的写集里。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Effect {
    /// 可能被写的根（`writes_all` 为真时此字段无意义，但保留已有的具体项便于诊断）。
    pub writes: Vec<RootId>,
    /// 保守档：写「所有根」（递归、未知被调方、间接调用）。
    pub writes_all: bool,
    /// 可能 panic（影响 `panic` 之后的可达性分析）。
    pub may_panic: bool,
    /// 可能回调（间接调用 / 接口派发 —— M6 虽不做，但字段留着给 M7）。
    pub may_call_back: bool,
    /// 有**输出**副作用（`print` / `println`，含经被调方传递）。
    ///
    /// # 为什么内存效果描述不了它
    ///
    /// `print` 不写任何根、不 panic、不回调 —— 在 `writes/may_panic` 三个维度上
    /// 它都是「纯」的。可它**会往 stdout 写字节**，而那正是 `C9` 契约里
    /// 「合并流」的全部内容。少了这一位，`dce` 会把没人用的 `println` 删掉：
    /// 图仍然合法、verifier 全绿、程序**少一行输出**（实测：`via=ssa` 的输出
    /// 直接变成空串）。
    pub prints: bool,
}

/// 运行期助手（`rt_*`）的**唯一清单**（M9）。
///
/// 每个变体对应 `runtime/native/<target>/rt.c` 里的一个 C 函数。
/// [`RuntimeFn::symbol`] 是唯一把变体翻成符号名的地方 —— 加一个变体要同时改那里
/// 与 `rt.c`，否则链接期 `undefined reference`（比静默错好）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuntimeFn {
    /// `rt_map_make(kdesc, vdesc) -> *MapObj`
    MapMake,
    /// `rt_map_get(m, k, out) -> bool`（写 `*out`）
    MapGet,
    /// `rt_map_set(m, k, v)`
    MapSet,
    /// `rt_map_del(m, k)`
    MapDel,
    /// `rt_map_iter(m, idx, kout, vout) -> u64`（0 = 结束）
    MapIter,
    /// `rt_send(c, val)` —— `c <- v`（M11b/Task 19）。`val` 是值的**地址**。
    ///
    /// # 为什么是 runtime 而不是 `Instr::Send`
    ///
    /// 它与 `rt_map_set` 同形（两个实参、无返回值），加一个 IR 变体只会多出
    /// 十几处 `match` 而换不到任何东西。**唯一的语义差别**（可能阻塞换 G）
    /// 在 C 侧，IR 看不见。
    ChanSend,
    /// `rt_recv(c, out) -> ok` —— `<-c`（M11b/Task 19）。写 `*out`，返回
    /// 「是否真的收到值」（`closed && 空` ⇒ 0）。
    ///
    /// 返回 `ok` 而不是两个变体：**同一个符号**同时服务 `v := <-c` 与
    /// `v, ok := <-c` —— 前者忽略返回值，后者取它。
    ChanRecv,
    /// `rt_select_begin(n, default_idx) -> WaitSet*`（M11b/Task 22）。
    ///
    /// `select` 的三阶段整块在 C 里（见 `rt.c` 的注释）：IR 只看到
    /// 「建 wait-set / 登记一个 case / 等结果」三个动作。拆成 IR 指令的话，
    /// 每个 `select` 都要在 SSA 里手写一遍「撤销其余登记」的循环 ——
    /// 那是把运行期簿记漏进代码生成。
    SelectBegin,
    /// `rt_select_add(token, idx, kind, chan, val)`：`kind` 0 = recv / 1 = send，
    /// `val` 是**地址**（recv 的输出 / send 的输入）。
    SelectAdd,
    /// `rt_select_wait(token) -> idx`：跑完三阶段，返回选中的下标。
    SelectWait,
    /// `rt_recover(out)` —— 写 16 字节接口到 `out`（`M9/Phase 5`）。
    ///
    /// # 为什么是 runtime 而不是内建
    ///
    /// `recover()` 要看「当前帧是不是被 defer 调用的」+「有没有挂起的 panic」，
    /// 两件事都只有运行期知道。IR 里它就是一个普通的值调用。
    Recover,
}

impl RuntimeFn {
    /// 全部（顺序进 `dump` 与守卫测试）。
    pub const ALL: [RuntimeFn; 11] = [
        RuntimeFn::MapMake,
        RuntimeFn::MapGet,
        RuntimeFn::MapSet,
        RuntimeFn::MapDel,
        RuntimeFn::MapIter,
        RuntimeFn::ChanSend,
        RuntimeFn::ChanRecv,
        RuntimeFn::SelectBegin,
        RuntimeFn::SelectAdd,
        RuntimeFn::SelectWait,
        RuntimeFn::Recover,
    ];

    /// `rt.c` 里的符号名（**唯一来源**）。
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            RuntimeFn::MapMake => "rt_map_make",
            RuntimeFn::MapGet => "rt_map_get",
            RuntimeFn::MapSet => "rt_map_set",
            RuntimeFn::MapDel => "rt_map_del",
            RuntimeFn::MapIter => "rt_map_iter",
            RuntimeFn::ChanSend => "rt_send",
            RuntimeFn::ChanRecv => "rt_recv",
            RuntimeFn::SelectBegin => "rt_select_begin",
            RuntimeFn::SelectAdd => "rt_select_add",
            RuntimeFn::SelectWait => "rt_select_wait",
            RuntimeFn::Recover => "rt_recover",
        }
    }

    /// dump 里的拼写（`C10` 的 `op` 语法）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            RuntimeFn::MapMake => "map_make",
            RuntimeFn::MapGet => "map_get",
            RuntimeFn::MapSet => "map_set",
            RuntimeFn::MapDel => "map_del",
            RuntimeFn::MapIter => "map_iter",
            RuntimeFn::ChanSend => "chan_send",
            RuntimeFn::ChanRecv => "chan_recv",
            RuntimeFn::SelectBegin => "select_begin",
            RuntimeFn::SelectAdd => "select_add",
            RuntimeFn::SelectWait => "select_wait",
            RuntimeFn::Recover => "recover",
        }
    }
}

/// 被调方。
#[derive(Clone, Debug)]
pub enum CalleeRef {
    /// 直接调用已知函数。
    Func(FuncId),
    /// 调用运行期的 C 助手（`rt_*`，M9）。
    ///
    /// # 为什么不走 `Builtin`
    ///
    /// `BuiltinKind` 是**语言内建**（`len` / `append` / `delete` …），由 HIR 决定；
    /// 而 `RuntimeFn` 是**后端契约**（`rt_map_get` 之类），HIR 里没有这些名字。
    /// 混在一起会让「HIR 支持哪些内建」与「runtime 提供哪些助手」变成同一张表。
    ///
    /// # 为什么不是 `Func(FuncId)`
    ///
    /// 那些函数没有 HIR 函数体（它们是 C），没有 `FuncId` 可指。`isel` 直接把
    /// 变体翻成一个符号名 —— 这也是唯一需要维护的映射。
    Runtime(RuntimeFn),
    /// 接口方法调用（`i.M(...)`，`M9/Task 32`）。
    ///
    /// # 为什么不是 `Indirect(fn)`
    ///
    /// 取 `itab.fn[slot]` 要**读两个字**（`tab` 从 `data` 旁边取、`fn` 从 `tab + 16 + 8*slot`），
    /// 而且第 0 个实参必须是 `data`（不是调用者写的）。这些都依赖接口的**布局**，
    /// 而布局是后端的知识（`C5` §10）。写成 `Indirect` 会把这些泄漏进 build，
    /// 还要在 IR 里造两条 `Field`/`Index` —— 而 `Field` 只认结构体。
    ///
    /// `slot` 是方法名在 [`iface_methods`] 序里的下标。
    Iface {
        /// 接口值（两个字：`tab` / `data`）。
        recv: ValueId,
        /// 方法槽下标。
        slot: u32,
    },
    /// 静态方法调用（接收者已按值/指针语义准备好）。
    Method {
        /// 接收者值。
        recv: ValueId,
        /// 方法体。
        func: FuncId,
    },
    /// 内建。
    Builtin(BuiltinKind),
    /// 函数值调用（`f()` 里 `f` 是变量）—— M6 支持建图，但 `effect` 记 `writes_all`。
    Indirect(ValueId),
}

/// 一条指令。
#[derive(Clone, Debug)]
pub enum Instr {
    /// 常量。
    Const {
        /// 结果。
        dst: ValueId,
        /// 值。
        c: ConstVal,
        /// 静态类型。
        typ: TypeId,
    },
    /// 值拷贝（块参数交接 / 结果直连的落点）。
    Copy {
        /// 结果。
        dst: ValueId,
        /// 源。
        src: ValueId,
    },
    /// 二元运算。
    Bin {
        /// 结果。
        dst: ValueId,
        /// 运算符。
        op: BinOp,
        /// 左。
        lhs: ValueId,
        /// 右。
        rhs: ValueId,
        /// 结果类型。
        typ: TypeId,
    },
    /// 一元运算。
    Un {
        /// 结果。
        dst: ValueId,
        /// 运算符。
        op: UnOp,
        /// 操作数。
        x: ValueId,
        /// 结果类型。
        typ: TypeId,
    },
    /// 类型转换。
    Convert {
        /// 结果。
        dst: ValueId,
        /// 源值。
        x: ValueId,
        /// 目标类型。
        typ: TypeId,
    },
    /// 取根的地址（`&x` / `&g`）。
    AddrOf {
        /// 结果（指针值）。
        dst: ValueId,
        /// 指向的根。
        root: RootId,
    },
    /// 结构体字段地址。
    Field {
        /// 结果。
        dst: ValueId,
        /// 底座（指针或聚合地址）。
        base: ValueId,
        /// 字段下标（lowering 定死）。
        index: usize,
    },
    /// 数组 / slice 元素地址。
    Index {
        /// 结果。
        dst: ValueId,
        /// 底座。
        base: ValueId,
        /// 下标。
        i: ValueId,
    },
    /// 切片（三索引可缺省）。
    Slice {
        /// 结果。
        dst: ValueId,
        /// 底座。
        base: ValueId,
        /// `lo`。
        lo: Option<ValueId>,
        /// `hi`。
        hi: Option<ValueId>,
        /// `max`。
        max: Option<ValueId>,
    },
    /// 复合字面量。
    Composite {
        /// 结果。
        dst: ValueId,
        /// 类型。
        typ: TypeId,
        /// 元素值（顺序 == 源码顺序）。
        items: Vec<ValueId>,
    },
    /// `make`，以及 `new(T)`。
    ///
    /// # `new(T)` 的约定
    ///
    /// `len` 与 `cap` **都为 `None`** 且 `typ` 是**指针** ⇒ `new(T)`：
    /// 分配一个 `T`（零值）并返回它的地址。没有单独的 `New` 变体，是因为
    /// 「分配一块存储」就是这一条指令的语义 —— 分配几个由 `len/cap` 说，
    /// 返回地址还返回值由 `typ` 说。
    Make {
        /// 结果。
        dst: ValueId,
        /// 类型。
        typ: TypeId,
        /// `len`。
        len: Option<ValueId>,
        /// `cap`。
        cap: Option<ValueId>,
    },
    /// 读内存。
    Load {
        /// 结果。
        dst: ValueId,
        /// 这条读用的是哪个 mem 值。
        mem: ValueId,
        /// 根（显式）。
        root: RootId,
        /// 地址。
        addr: ValueId,
        /// 类型。
        typ: TypeId,
    },
    /// 写内存，产出**新的** mem（`dst`）。
    Store {
        /// 新的 mem。
        dst: ValueId,
        /// 写之前用的 mem。
        mem: ValueId,
        /// 根（显式）。
        root: RootId,
        /// 地址。
        addr: ValueId,
        /// 写的值。
        val: ValueId,
    },
    /// 调用（多结果；`D-M6-4`）。
    Call {
        /// 结果值（个数 == 签名结果数）。
        dsts: Vec<ValueId>,
        /// 被调方。
        callee: CalleeRef,
        /// 实参。
        args: Vec<ValueId>,
        /// 调用点是否写了 `...`（`append(s, xs...)` / `println(xs...)`）。
        ///
        /// # 为什么这条必须进 IR
        ///
        /// 用户函数的变参在**调用点**就打包成 slice（`pack_variadic`），所以
        /// `f(xs...)` 与 `f(xs)` 到被调方手上是同一个东西。内建不行：`append(s, xs)`
        /// 追加**一个**元素，`append(s, xs...)` 展开**一串**元素 —— 两者实参个数
        /// 一样，差别只在「要不要展开」。不记这个位，IR 就表达不出语义，
        /// 求值器只能猜（猜错的后果是静默算错，而不是报错）。
        dots: bool,
        /// 副作用摘要。
        effect: Effect,
    },
    /// 启动一个 goroutine（`go f(args)`，M11/Task 12）。
    ///
    /// # 为什么不复用 `Call` 加一个标志位
    ///
    /// 三件事都不同（`M11-design.md` §3.6）：
    /// 1. `Call` 有返回值、调用方**等**结果；`go` 没有返回值、**不等**；
    /// 2. `go` 的实参要**深拷贝到新 G 的栈**上（`Call` 的实参留在调用方帧里）；
    /// 3. `go` 之后当前 G **继续跑**（`C11` §2.1 的「不抢占」）。
    ///
    /// 合成一个变体会让 `isel` 里到处是 `if is_go { … }`，而其中一处漏判的
    /// 后果是「实参没拷」—— 表现为被调方读到调用方栈上的旧值，**不崩、只是算错**。
    ///
    /// ⚠️ 实参区字节数**不入 IR**：它由 `args` 的类型算出来（`Ctx::ty`）。
    /// 存一份就多一个可能与类型不一致的字段。
    Go {
        /// 被调方。
        callee: CalleeRef,
        /// 实参（**深拷贝**到新 G 的栈）。
        args: Vec<ValueId>,
        /// 副作用摘要。
        effect: Effect,
    },
    /// 取一个**具名函数**的地址（`f := g`，`M9/Task 37`）。
    ///
    /// 结果是 `func(...)` 类型的值 —— 运行期就是一个函数指针（`fn` 在偏移 0）。
    /// 没有捕获，所以它不需要分配：`dst` 里直接放 `adrp/add` 出来的符号地址。
    ///
    /// # 为什么它是指令而不是常量
    ///
    /// 常量面（`ConstVal`）只有标量：函数符号是**重定位**，不是数值。
    /// 塞进 `Const` 会让 dump 与 `C10` 的常量语法撒谎。
    FuncAddr {
        /// 结果值（`func(...)` 类型）。
        dst: ValueId,
        /// 目标函数。
        func: FuncId,
    },
    /// 造一个**闭包**（有捕获的函数值，`M9/Task 38`）。
    ///
    /// 布局（`D-M9-10`）：`{ fn: *u8, caps: [*u8; n] }`，堆上分配，`size = 8 + 8n`。
    ///
    /// # 捕获是**按引用**的（`R-9-15`）
    ///
    /// `captures[i]` 是**指向被捕获变量存储的指针**，不是它的副本。Go 的闭包
    /// 捕获的是**变量**（`closure2.go` 整份文件就是在钉这件事：改要改到外面去），
    /// 所以 funcval 里只能放地址。被捕获的变量因此必须**活在堆上**（逃逸）——
    /// 这件事由「它有了根」自动成立（`Root::Slot` 的存储本来就在 arena）。
    MakeClosure {
        /// 结果值（`func(...)` 类型）。
        dst: ValueId,
        /// 函数体。
        func: FuncId,
        /// 每个捕获变量的**地址**（顺序 == 被调方 `captured` 的顺序）。
        captures: Vec<ValueId>,
    },
    /// 取**本帧**第 `index` 个捕获的地址（`M9/Task 38`）。
    ///
    /// 只有带捕获的函数里才出现。它的值来自隐式第 0 实参（ctx = funcval）：
    /// `[ctx + 8 + 8*index]`。
    CapturePtr {
        /// 结果值（`*T`，`T` 是被捕获变量的类型）。
        dst: ValueId,
        /// 第几个捕获（== `SsaFunc::captured` 的下标）。
        index: u32,
    },
    /// 登记一条 **defer**（`M9/Phase 5`）。
    ///
    /// 只存 **funcval**：`defer f(args...)` 的实参由 build 打包进一个闭包
    /// （`defer` 的实参在**登记时**求值 —— Go 规范），所以运行期不需要知道任何
    /// ABI 布局，`rt_deferrun` 只做「调一个 funcval」。
    ///
    /// # 为什么不是 `Instr::Call`
    ///
    /// `Call` 会在**当前位置**执行；`Defer` 只在**帧退出时**执行。两者在控制流上
    /// 完全无关，用同一条件表示就得再加一个「什么时候执行」的位 —— 而那个位
    /// 只有两个取值。
    Defer {
        /// 被登记的 funcval（`func()` 类型）。
        fv: ValueId,
    },
    /// 类型断言（`x.(T)` / `x.(J)`，`M9/Task 33`）。
    ///
    /// # 两种形态
    ///
    /// | `ok` | 源码 | 失败时 |
    /// |---|---|---|
    /// | `None` | `v := x.(T)` | **panic**（`rt_panic_assert`）|
    /// | `Some(v)` | `v, ok := x.(T)` | `ok = false`，`dst` 是零值 |
    ///
    /// # 为什么不是 `Convert`
    ///
    /// `Convert` 有一个 `dst`，而 comma-ok 形态要**两个**结果；并且「失败」在
    /// `Convert` 里没有位置（它要么成功要么 panic）。`Instr` 是多结果的
    /// （`Call` 的 `dsts` 就是），但 `Convert` 的语义已经冻结成单值。
    TypeAssert {
        /// 结果值（`typ` 类型）。
        dst: ValueId,
        /// `ok`（comma-ok 形态）。
        ok: Option<ValueId>,
        /// 被断言的接口值。
        x: ValueId,
        /// 目标类型（具体类型或接口）。
        typ: TypeId,
    },
    /// 空指针检查：`addr == nil` ⇒ **可恢复的 panic**（`M9/Phase 5c`）。
    ///
    /// # 为什么需要一条没有结果的指令
    ///
    /// Go 里「取一个 place 的地址」可以**不读内存**却仍然 panic：
    /// `&*p`（`p == nil`）和 `&p.f`（`p == nil`）都不产生 `Load`，可它们
    /// 在 Go 里都是运行时错误。IR 里 `&*p` 更是连指令都没有 —— 它就是
    /// `p` 这个指针值本身（`build.rs::place_addr_root(Place::Deref)`），
    /// 于是「这里要检查空指针」在图上**无处可写**。
    ///
    /// 加这条指令而不是让后端在 `Field`/`Index` 里各自加检查：后端各写一份
    /// 必然分叉（`R-9-13` 的教训），而这条检查是**语义**，不是 ABI 细节。
    ///
    /// # 它没有 dst
    ///
    /// 检查不改任何值，只在失败时离开本帧 ⇒ 不需要结果，也就不能用「没人用
    /// 就删」的 DCE 规则（`opt::dce` 把它列进「永不删」）。
    NilCheck {
        /// 被检查的地址。
        addr: ValueId,
    },
}

/// 终止符。**边上的实参挂在终止符上**（`V7` 的比对面）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Term {
    /// 无条件跳转。
    Goto {
        /// 目标块。
        to: BlockId,
        /// 给目标块参数的实参。
        args: Vec<ValueId>,
    },
    /// 条件跳转。
    If {
        /// 条件。
        cond: ValueId,
        /// 真分支（目标块 + 实参）。
        then_: (BlockId, Vec<ValueId>),
        /// 假分支（目标块 + 实参）。
        else_: (BlockId, Vec<ValueId>),
    },
    /// 返回（多值）。
    Return(Vec<ValueId>),
    /// `panic`。
    Panic(ValueId),
    /// 块可达但流到此为止（如 `panic` 之后的块尾）；**无操作数**。
    #[default]
    Unreachable,
}

/// 指令的 `C10` 名字（kebab-case；**不带通配的 `match`**）。
#[must_use]
pub fn kind_of_instr(i: &Instr) -> &'static str {
    match i {
        Instr::Const { .. } => "const",
        Instr::Copy { .. } => "copy",
        Instr::Bin { .. } => "bin",
        Instr::Un { .. } => "un",
        Instr::Convert { .. } => "convert",
        Instr::AddrOf { .. } => "addr-of",
        Instr::Field { .. } => "field",
        Instr::Index { .. } => "index",
        Instr::Slice { .. } => "slice",
        Instr::Composite { .. } => "composite",
        Instr::Make { .. } => "make",
        Instr::Load { .. } => "load",
        Instr::Store { .. } => "store",
        Instr::Call { .. } => "call",
        Instr::Go { .. } => "go",
        Instr::FuncAddr { .. } => "funcaddr",
        Instr::Defer { .. } => "defer",
        Instr::NilCheck { .. } => "nil-check",
        Instr::MakeClosure { .. } => "closure",
        Instr::CapturePtr { .. } => "capture",
        Instr::TypeAssert { .. } => "assert",
    }
}

/// 终止符的 `C10` 名字。
#[must_use]
pub fn kind_of_term(t: &Term) -> &'static str {
    match t {
        Term::Goto { .. } => "goto",
        Term::If { .. } => "if",
        Term::Return(_) => "return",
        Term::Panic(_) => "panic",
        Term::Unreachable => "unreachable",
    }
}

/// 根的类别名。
#[must_use]
pub fn kind_of_root(r: &Root) -> &'static str {
    match r {
        Root::Global(_) => "global",
        Root::Slot(_) => "slot",
        Root::Heap => "heap",
    }
}

/// 被调方的类别名。
#[must_use]
pub fn kind_of_callee(c: &CalleeRef) -> &'static str {
    match c {
        CalleeRef::Func(_) => "func",
        CalleeRef::Method { .. } => "method",
        CalleeRef::Builtin(_) => "builtin",
        CalleeRef::Runtime(_) => "runtime",
        CalleeRef::Indirect(_) => "indirect",
        CalleeRef::Iface { .. } => "iface",
    }
}
