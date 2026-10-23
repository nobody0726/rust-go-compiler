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

/// 一个模块（单包；`D-M6-2`）。
#[derive(Clone, Debug, Default)]
pub struct Module {
    /// 全部函数（顺序 = HIR 的 `Program::funcs` 顺序，dump 与求值都靠它保持确定性）。
    pub funcs: Vec<SsaFunc>,
    /// 包级变量（与 HIR 的 `GlobalId` 同编号空间）。
    pub globals: Vec<GlobalId>,
    /// `main`（可执行程序必有）。
    pub main: Option<FuncId>,
}

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
    /// `ValueId → TypeId`（长度必须等于值个数；`V3` 查它）。
    pub value_types: Vec<TypeId>,
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
}

/// 被调方。
#[derive(Clone, Debug)]
pub enum CalleeRef {
    /// 直接调用已知函数。
    Func(FuncId),
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
    /// `make`。
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
        /// 副作用摘要。
        effect: Effect,
    },
}

/// 终止符。**边上的实参挂在终止符上**（`V7` 的比对面）。
#[derive(Clone, Debug, Default)]
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
        CalleeRef::Indirect(_) => "indirect",
    }
}
