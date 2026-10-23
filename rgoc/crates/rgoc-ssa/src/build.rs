//! HIR → SSA 构建（`M6-design.md` §3.3；`M6-plan.md` Task 17 起）。
//!
//! # 当前进度（Task 17：骨架）
//!
//! 建**每个函数一个 `SsaFunc`**：入口块 + 形参（`Func::params`/`param_types` →
//! 入口块的块参数）+ 空体函数的 `Term::Return`。函数体的语句、表达式、
//! 取址分析、槽提升都还没进来 —— 遇到非空体就 `todo!()`，**不静默产出空函数**。
//!
//! # 为什么非空体用 `todo!()` 而不是「尽力而为」
//!
//! 「尽力而为」在这里等于**语义错**：一个丢掉了语句的函数仍然能过 verifier
//! （它是一张合法的空图），于是 `ssa --verify` 绿、`--via=ssa` 却算出错结果。
//! 项目纪律是「不用 `None` 冒充未实现」—— 这里同理：没实现就响，别静默降级。
//!
//! # `Module::funcs` 的下标 ≠ `FuncId`
//!
//! 被拒绝的函数**不进** `Module::funcs`（拒绝要显式上报，不许塞个空函数顶位）。
//! 所以「第 i 个 SsaFunc 是哪个 HIR 函数」只能看 [`crate::ir::SsaFunc::src`] ——
//! 调用指令里的 `CalleeRef::Func` 也仍然是 **HIR 编号**。

use std::collections::BTreeMap;

use rgoc_hir::{Func, FuncId, GlobalId, Program};
use rgoc_types::{TypeId, TypeTable};

use crate::diag::Reject;
use crate::ir::{Block, BlockId, Module, SsaFunc, Term, ValueId};

/// 构建结果：建成的模块 + 被拒绝的函数（**两条通道都要报**，不许只返回一半）。
#[derive(Clone, Debug)]
pub struct BuildOutcome {
    /// 建成的模块（`funcs` 只含建成的函数；顺序 == HIR 顺序去掉被拒的那些）。
    pub module: Module,
    /// 被拒绝的函数（每条带稳定码；`SSA-7` 要求它们出现在 stderr）。
    pub rejected: Vec<Reject>,
}

/// 把一个 HIR 程序建成 SSA 模块。
#[must_use]
pub fn build(program: &Program, tt: &TypeTable) -> BuildOutcome {
    let mut funcs = Vec::with_capacity(program.funcs.len());
    let mut rejected = Vec::new();
    for (i, f) in program.funcs.iter().enumerate() {
        let id = FuncId(i as u32);
        match build_func(f, id, tt) {
            Ok(sf) => funcs.push(sf),
            Err(r) => rejected.push(r),
        }
    }
    BuildOutcome {
        module: Module {
            funcs,
            globals: (0..program.globals.len())
                .map(|i| GlobalId(i as u32))
                .collect(),
            main: program.main,
        },
        rejected,
    }
}

/// 建一个函数（Task 17：只支持**空体 + 无结果**）。
fn build_func(f: &Func, id: FuncId, _tt: &TypeTable) -> Result<SsaFunc, Reject> {
    let mut value_types: Vec<TypeId> = Vec::with_capacity(f.param_types.len());
    let mut params: Vec<ValueId> = Vec::with_capacity(f.param_types.len());
    for &t in &f.param_types {
        params.push(ValueId(value_types.len() as u32));
        value_types.push(t);
    }

    let (roots, slot_root) = roots_of(_tt, &f.body);

    if !f.body.stmts.is_empty() {
        todo!("M6/Task 19–31：函数体构建（当前只有空体）");
    }
    if !f.results.is_empty() {
        todo!("M6/Task 21：空体 + 有结果 ⇒ 零值返回");
    }

    let entry = Block {
        params: params.clone(),
        instrs: Vec::new(),
        term: Term::Return(Vec::new()),
    };
    Ok(SsaFunc {
        src: id,
        name: f.name.clone(),
        params,
        blocks: vec![entry],
        entry: BlockId(0),
        roots,
        slot_root,
        value_types,
        call_sigs: BTreeMap::new(),
    })
}

// ── 取址分析（Task 18）──────────────────────────────────────────────

use std::collections::BTreeSet;

use rgoc_hir::{
    Block as HirBlock, BranchKind, CompositeItem, Expr, ExprKind, MakeKind, Place, SlotId, Stmt,
};

use crate::ir::{Root, RootId};

/// 收集函数体里**被取过地址**的槽（`Task 18` / `T-SSA-27` 的前半）。
///
/// # 为什么必须递归到 `Place::Field` / `Place::Index` 里面
///
/// `&s.f` 取的是**槽 s 的一部分**的地址 —— 一旦有人能通过这个指针写 `s.f`，
/// 整个 `s` 就不再是「值」而是「可变存储」。只认 `&x` 这一种形态会把
/// `&s.f` 漏成「s 没取过址」，于是 `s` 被提升成 SSA 值、写穿丢效果 ——
/// 这正是 `M6-design.md` §8 风险第 2 条说的「语义错且大多数 fixture 仍绿」。
///
/// # 递归的两条边界
///
/// * `Place::Deref(e)`：`&(*p)` 取的是 `p` 指向的存储，**不是** `p` 这个槽的地址
///   —— 所以只扫 `e`（`p` 的读），不把任何槽记成「取过址」。指针目标属于
///   **堆根**（或别的函数的根），不是本函数的槽。
/// * `Place::Global(_)`：包级变量有它自己的根（`Root::Global`），不进 `slot_root`。
#[must_use]
pub fn scan_addressed(tt: &TypeTable, b: &HirBlock) -> BTreeSet<SlotId> {
    let mut set = BTreeSet::new();
    scan_block(tt, b, &mut set);
    set
}

/// 按槽号升序建根表（`Root::Slot`），并给出 `slot_root` 映射（`V9` 要求两向互逆）。
///
/// 顺序固定（槽号升序 ⇒ `RootId` 升序）：dump 要确定性，`RootId` 的分配顺序
/// 进了 `C10` 的 `r<k>` 编号，不能随遍历顺序漂。
#[must_use]
pub fn roots_of(tt: &TypeTable, b: &HirBlock) -> (Vec<Root>, BTreeMap<SlotId, RootId>) {
    let mut roots = Vec::new();
    let mut slot_root = BTreeMap::new();
    for slot in scan_addressed(tt, b) {
        let id = RootId(roots.len() as u32);
        roots.push(Root::Slot(slot));
        slot_root.insert(slot, id);
    }
    (roots, slot_root)
}

fn scan_block(tt: &TypeTable, b: &HirBlock, set: &mut BTreeSet<SlotId>) {
    for s in &b.stmts {
        scan_stmt(tt, s, set);
    }
}

fn scan_stmt(tt: &TypeTable, s: &Stmt, set: &mut BTreeSet<SlotId>) {
    match s {
        Stmt::Assign { lhs, rhs, .. } => {
            for p in lhs {
                // 赋值目标**不是**取址：`a = 1` 不产生任何根。
                scan_lhs_place(tt, p, set);
            }
            for e in rhs {
                scan_expr(tt, e, set);
            }
        }
        Stmt::ShortDecl { rhs, .. } => {
            for e in rhs {
                scan_expr(tt, e, set);
            }
        }
        Stmt::Expr(e) => scan_expr(tt, e, set),
        Stmt::If {
            init,
            cond,
            then,
            els,
        } => {
            if let Some(i) = init {
                scan_stmt(tt, i, set);
            }
            scan_expr(tt, cond, set);
            scan_block(tt, then, set);
            if let Some(e) = els {
                scan_stmt(tt, e, set);
            }
        }
        Stmt::For {
            init,
            cond,
            post,
            body,
            ..
        } => {
            if let Some(i) = init {
                scan_stmt(tt, i, set);
            }
            if let Some(c) = cond {
                scan_expr(tt, c, set);
            }
            if let Some(p) = post {
                scan_stmt(tt, p, set);
            }
            scan_block(tt, body, set);
        }
        Stmt::Range {
            key,
            value,
            x,
            body,
            ..
        } => {
            if let Some(p) = key {
                scan_lhs_place(tt, p, set);
            }
            if let Some(p) = value {
                scan_lhs_place(tt, p, set);
            }
            scan_expr(tt, x, set);
            scan_block(tt, body, set);
        }
        Stmt::Switch {
            init, tag, cases, ..
        } => {
            if let Some(i) = init {
                scan_stmt(tt, i, set);
            }
            if let Some(t) = tag {
                scan_expr(tt, t, set);
            }
            for c in cases {
                for e in &c.exprs {
                    scan_expr(tt, e, set);
                }
                scan_block(tt, &c.body, set);
            }
        }
        Stmt::TypeSwitch { init, x, cases, .. } => {
            if let Some(i) = init {
                scan_stmt(tt, i, set);
            }
            scan_expr(tt, x, set);
            for c in cases {
                scan_block(tt, &c.body, set);
            }
        }
        Stmt::Return(es) => {
            for e in es {
                scan_expr(tt, e, set);
            }
        }
        Stmt::Branch { kind, .. } => {
            // `BranchKind` 不带表达式；显式 match 是为了加变体时编译报错
            match kind {
                BranchKind::Break
                | BranchKind::Continue
                | BranchKind::Goto
                | BranchKind::Fallthrough => {}
            }
        }
        Stmt::Labeled { stmt, .. } => scan_stmt(tt, stmt, set),
        Stmt::Block(b) => scan_block(tt, b, set),
        Stmt::Defer { call } => scan_expr(tt, call, set),
        Stmt::Empty => {}
    }
}

fn scan_expr(tt: &TypeTable, e: &Expr, set: &mut BTreeSet<SlotId>) {
    match &e.kind {
        ExprKind::Const(_) | ExprKind::Slot(_) | ExprKind::Global(_) | ExprKind::Nil => {}
        ExprKind::Field { base, .. } => scan_expr(tt, base, set),
        ExprKind::Index { base, index, .. } => {
            scan_expr(tt, base, set);
            scan_expr(tt, index, set);
        }
        ExprKind::Slice {
            base, lo, hi, max, ..
        } => {
            scan_expr(tt, base, set);
            for o in [lo, hi, max].into_iter().flatten() {
                scan_expr(tt, o, set);
            }
        }
        ExprKind::Deref(x) => scan_expr(tt, x, set),
        // 唯一产生指针的构造：`&x`
        ExprKind::Addr(p) => scan_addr_place(tt, p, set),
        // `&T{...}` 取的是**新分配的**存储，不是任何槽
        ExprKind::AddrOf(x) => scan_expr(tt, x, set),
        ExprKind::Unary { x, .. } => scan_expr(tt, x, set),
        ExprKind::Binary { x, y, .. } => {
            scan_expr(tt, x, set);
            scan_expr(tt, y, set);
        }
        ExprKind::Call { args, .. } => {
            for a in args {
                scan_expr(tt, a, set);
            }
        }
        ExprKind::Convert { x, .. } => scan_expr(tt, x, set),
        ExprKind::Composite(c) => {
            for it in &c.items {
                match it {
                    CompositeItem::Value(x) => scan_expr(tt, x, set),
                    CompositeItem::Key(k, v) => {
                        scan_expr(tt, k, set);
                        scan_expr(tt, v, set);
                    }
                    CompositeItem::Field(_, x) | CompositeItem::Index(_, x) => {
                        scan_expr(tt, x, set)
                    }
                }
            }
        }
        ExprKind::FuncLit(_) => {
            // 函数字面量已提升为独立 `Func`，它的体**不属于**本函数 ——
            // 闭包里的 `&x` 会让 `x` 逃逸到闭包帧（`Root::Slot` 的边界），
            // 但 M6 把闭包整体拒掉（`SSA-UNSUPPORTED-CLOSURE`，Task 31），
            // 所以这里不扫。
        }
        ExprKind::Bound { recv, .. } => scan_expr(tt, recv, set),
        ExprKind::TypeAssert { x, .. } => scan_expr(tt, x, set),
        ExprKind::Make { kind, args } => {
            match kind {
                MakeKind::Slice | MakeKind::Map | MakeKind::Chan => {}
            }
            for a in args {
                scan_expr(tt, a, set);
            }
        }
        ExprKind::New(_) => {}
        ExprKind::Builtin { args, .. } => {
            for a in args {
                scan_expr(tt, a, set);
            }
        }
    }
}

/// `&place` 里的 place：**标记**被取址的槽。
fn scan_addr_place(tt: &TypeTable, p: &Place, set: &mut BTreeSet<SlotId>) {
    match p {
        Place::Slot(s) => {
            set.insert(*s);
        }
        Place::Global(_) => {}
        Place::Field { base, .. } => scan_addr_place(tt, base, set),
        Place::Index { base, index } => {
            // `&arr[i]`：数组**元素在数组槽的存储里** ⇒ 数组槽本身被取址。
            // `&s[i]`（s 是 slice）元素在底层数组（堆根），s 这个槽不被取址 ——
            // 所以这里必须按**类型**分叉，不能一律保守地把 base 的槽记进来
            // （那会造出假根，把纯值化的机会白扔掉，还会让 dump 与预期不符）。
            if is_array(tt, base.typ) {
                scan_addressed_value(tt, base, set);
            } else {
                scan_expr(tt, base, set);
            }
            scan_expr(tt, index, set);
        }
        // `&*p`：取的是 `p` 指向的存储，不是 `p` 这个槽
        Place::Deref(e) => scan_expr(tt, e, set),
    }
}

/// 一个**值**的存储是否被整体取址（用于 `&arr[i]` 的底座）。
///
/// 只有「值直接住在某个槽里」的形态才算：`Slot`、以及嵌在结构体里的 `Field`。
/// 其余（`Deref` / `Index` / `Slice` / 调用结果）的存储都在别处（堆根或别人的帧），
/// 不产生本函数的 `Root::Slot`。
fn scan_addressed_value(tt: &TypeTable, e: &Expr, set: &mut BTreeSet<SlotId>) {
    match &e.kind {
        ExprKind::Slot(s) => {
            set.insert(*s);
        }
        ExprKind::Field { base, .. } => scan_addressed_value(tt, base, set),
        // 其余形态：存储不在本函数的槽里，但仍要扫它们内部有没有 `&x`
        _ => scan_expr(tt, e, set),
    }
}

/// 是否是**数组**（含具名数组、别名；`[]T` 不算）。
fn is_array(tt: &TypeTable, t: TypeId) -> bool {
    if t.index() >= tt.len() {
        return false;
    }
    matches!(
        tt.type_of(rgoc_types::underlying(tt, t)),
        rgoc_types::typ::Type::Array { .. }
    )
}

/// 赋值目标里的 place：**不标记**槽（写不是取址），只递归找内部表达式里的 `&x`。
///
/// # 为什么必须与 [`scan_addr_place`] 分开
///
/// 实测踩过：把赋值目标也走「取址」那条路，`a = 1` 就让 `a` 变成了根 ——
/// 于是每个有局部变量的函数都会退化成「全走内存」，纯值化（`T-SSA-26`）永远不成立。
///
/// # 空白标识符 `_`（`SlotId(u32::MAX)`）为什么不用特判
///
/// HIR 用 `SlotId(u32::MAX)` 表示 `_`。它只可能出现在**赋值目标**位置
/// （`_ = x`、`_ = &x`），而这条路**不标记**任何槽 ⇒ 空白槽到不了 `roots_of`。
/// 初版在这里加过一条「跳过 `u32::MAX`」的判断，变异测试证明它**不可达**
/// （删掉判断，8 条测试全绿），于是按纪律删掉 —— 一条永远走不到的守卫等于没有。
fn scan_lhs_place(tt: &TypeTable, p: &Place, set: &mut BTreeSet<SlotId>) {
    match p {
        Place::Slot(_) | Place::Global(_) => {}
        Place::Field { base, .. } => scan_lhs_place(tt, base, set),
        Place::Index { base, index } => {
            scan_expr(tt, base, set);
            scan_expr(tt, index, set);
        }
        Place::Deref(e) => scan_expr(tt, e, set),
    }
}
