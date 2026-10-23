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
use crate::ir::{Block, BlockId, Instr, Module, SsaFunc, Term, ValueId};

/// 空白标识符 `_` 的槽（HIR 约定：`_` 用 `SlotId(u32::MAX)` 占位）。
///
/// 两处要用它：**不当定义**（`_ = x` 不产生值）、**不提升**（它没有存储）。
pub const BLANK_SLOT: SlotId = SlotId(u32::MAX);

/// 构建结果：建成的模块 + 被拒绝的函数（**两条通道都要报**，不许只返回一半）。
#[derive(Clone, Debug)]
pub struct BuildOutcome {
    /// 建成的模块（`funcs` 只含建成的函数；顺序 == HIR 顺序去掉被拒的那些）。
    pub module: Module,
    /// 被拒绝的函数（每条带稳定码；`SSA-7` 要求它们出现在 stderr）。
    pub rejected: Vec<Reject>,
}

/// 把一个 HIR 程序建成 SSA 模块。
///
/// # 为什么是 `&mut TypeTable`（修订 `R-20-2`）
///
/// `&x` 要产出 `AddrOf { dst }`，而 `dst` 的静态类型必须是 `*T`。指针类型只能由
/// `TypeTable::add_pointer` **新建** —— 拿一张不可变的表就只能在 dump 里编一个
/// 「假类型」出来，那等于让 IR 里的类型与实际不符。表本来就是包级的、随 build
/// 一起长大，所以这里要 `&mut`。
#[must_use]
pub fn build(program: &Program, tt: &mut TypeTable) -> BuildOutcome {
    let mut funcs = Vec::with_capacity(program.funcs.len());
    let mut rejected = Vec::new();
    for (i, f) in program.funcs.iter().enumerate() {
        let id = FuncId(i as u32);
        match build_func(f, id, program, tt) {
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

/// 建一个函数。
///
/// 三步：① 扫出**需要根**的槽（取址 ∪ 部分写）；② 算**确定赋值**（读到必已写过），
/// 其余槽提升成纯值；③ 按语句走一遍，边建块边产指令。
fn build_func(
    f: &Func,
    id: FuncId,
    program: &Program,
    tt: &mut TypeTable,
) -> Result<SsaFunc, Reject> {
    let (definite, unsafe_slots) = definite_slots(tt, f);
    // 需要根的槽 = 取址 ∪ 部分写 ∪ **读前可能未赋值**
    let mut rooted: BTreeSet<SlotId> = scan_rooted(tt, &f.body);
    rooted.extend(unsafe_slots);
    rooted.remove(&BLANK_SLOT);

    let ng = program.globals.len();
    // 根表顺序固定：先全局（按 GlobalId），再槽（按 SlotId 升序）—— dump 的 r<k> 靠它稳定
    let mut roots: Vec<Root> = (0..ng).map(|i| Root::Global(GlobalId(i as u32))).collect();
    let mut slot_root: BTreeMap<SlotId, RootId> = BTreeMap::new();
    for s in &rooted {
        slot_root.insert(*s, RootId(roots.len() as u32));
        roots.push(Root::Slot(*s));
    }

    let promoted: BTreeSet<SlotId> = definite.difference(&rooted).copied().collect();

    let mut fb = Fb {
        tt,
        f,
        globals: &program.globals,
        values: Vec::new(),
        blocks: vec![Block::default()],
        cur: BlockId(0),
        val: BTreeMap::new(),
        mem: BTreeMap::new(),
        roots,
        slot_root,
        promoted,
        ptr_ty: BTreeMap::new(),
        terminated: false,
    };
    fb.entry_params();

    let body = &f.body;
    if !body.stmts.is_empty() {
        fb.stmts(&body.stmts);
    }
    if !fb.terminated {
        // Go 里只有「无结果的函数」可以落到函数尾
        fb.set_term(Term::Return(Vec::new()));
    }
    Ok(fb.finish(id))
}

// ── 取址分析（Task 18）──────────────────────────────────────────────

use std::collections::BTreeSet;

use rgoc_hir::{
    Block as HirBlock, BranchKind, CompositeItem, Expr, ExprKind, MakeKind, Place, SlotId, Stmt,
};

use crate::ir::{Root, RootId};

/// 扫描结果：两类「不能提升成纯值」的槽。
///
/// 扫描只跑一次，同时收两类（`addressed` 与 `partial`）—— 它们的**根因不同**
/// （取过址 / 被部分写），但结论相同：这个槽必须留在内存里。
#[derive(Default)]
struct Scanned {
    /// 取过地址的槽（`&x` / `&x.f` / `&arr[i]`）。
    addressed: BTreeSet<SlotId>,
    /// 被**部分写**过的槽（`s.f = 1` / `arr[i] = 1`）。
    ///
    /// 这类槽没有 `&`，所以不在 `addressed` 里 —— 但它同样**不能**提升：
    /// SSA 没有「改一个字段」的指令（只有整值 `Composite`），把 `s` 提升成值
    /// 就再也写不进 `s.f` 了。M6 的口径是：部分写 ⇒ 整个槽走内存。
    partial: BTreeSet<SlotId>,
}

impl Scanned {
    fn rooted(&self) -> BTreeSet<SlotId> {
        self.addressed.union(&self.partial).copied().collect()
    }
}

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
    let mut sc = Scanned::default();
    scan_block(tt, b, &mut sc);
    sc.addressed
}

/// 收集**需要内存根**的槽（取址 ∪ 部分写）。
#[must_use]
pub fn scan_rooted(tt: &TypeTable, b: &HirBlock) -> BTreeSet<SlotId> {
    let mut sc = Scanned::default();
    scan_block(tt, b, &mut sc);
    sc.rooted()
}

/// 按槽号升序建根表（`Root::Slot`），并给出 `slot_root` 映射（`V9` 要求两向互逆）。
///
/// 顺序固定（槽号升序 ⇒ `RootId` 升序）：dump 要确定性，`RootId` 的分配顺序
/// 进了 `C10` 的 `r<k>` 编号，不能随遍历顺序漂。
#[must_use]
pub fn roots_of(tt: &TypeTable, b: &HirBlock) -> (Vec<Root>, BTreeMap<SlotId, RootId>) {
    let mut roots = Vec::new();
    let mut slot_root = BTreeMap::new();
    for slot in scan_rooted(tt, b) {
        let id = RootId(roots.len() as u32);
        roots.push(Root::Slot(slot));
        slot_root.insert(slot, id);
    }
    (roots, slot_root)
}

fn scan_block(tt: &TypeTable, b: &HirBlock, sc: &mut Scanned) {
    for s in &b.stmts {
        scan_stmt(tt, s, sc);
    }
}

fn scan_stmt(tt: &TypeTable, s: &Stmt, sc: &mut Scanned) {
    match s {
        Stmt::Assign { lhs, rhs, .. } => {
            for p in lhs {
                // 赋值目标**不是**取址：`a = 1` 不产生任何根。
                scan_lhs_place(tt, p, sc);
            }
            for e in rhs {
                scan_expr(tt, e, sc);
            }
        }
        Stmt::ShortDecl { rhs, .. } => {
            for e in rhs {
                scan_expr(tt, e, sc);
            }
        }
        Stmt::Expr(e) => scan_expr(tt, e, sc),
        Stmt::If {
            init,
            cond,
            then,
            els,
        } => {
            if let Some(i) = init {
                scan_stmt(tt, i, sc);
            }
            scan_expr(tt, cond, sc);
            scan_block(tt, then, sc);
            if let Some(e) = els {
                scan_stmt(tt, e, sc);
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
                scan_stmt(tt, i, sc);
            }
            if let Some(c) = cond {
                scan_expr(tt, c, sc);
            }
            if let Some(p) = post {
                scan_stmt(tt, p, sc);
            }
            scan_block(tt, body, sc);
        }
        Stmt::Range {
            key,
            value,
            x,
            body,
            ..
        } => {
            if let Some(p) = key {
                scan_lhs_place(tt, p, sc);
            }
            if let Some(p) = value {
                scan_lhs_place(tt, p, sc);
            }
            scan_expr(tt, x, sc);
            scan_block(tt, body, sc);
        }
        Stmt::Switch {
            init, tag, cases, ..
        } => {
            if let Some(i) = init {
                scan_stmt(tt, i, sc);
            }
            if let Some(t) = tag {
                scan_expr(tt, t, sc);
            }
            for c in cases {
                for e in &c.exprs {
                    scan_expr(tt, e, sc);
                }
                scan_block(tt, &c.body, sc);
            }
        }
        Stmt::TypeSwitch { init, x, cases, .. } => {
            if let Some(i) = init {
                scan_stmt(tt, i, sc);
            }
            scan_expr(tt, x, sc);
            for c in cases {
                scan_block(tt, &c.body, sc);
            }
        }
        Stmt::Return(es) => {
            for e in es {
                scan_expr(tt, e, sc);
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
        Stmt::Labeled { stmt, .. } => scan_stmt(tt, stmt, sc),
        Stmt::Block(b) => scan_block(tt, b, sc),
        Stmt::Defer { call } => scan_expr(tt, call, sc),
        Stmt::Empty => {}
    }
}

fn scan_expr(tt: &TypeTable, e: &Expr, sc: &mut Scanned) {
    match &e.kind {
        ExprKind::Const(_) | ExprKind::Slot(_) | ExprKind::Global(_) | ExprKind::Nil => {}
        ExprKind::Field { base, .. } => scan_expr(tt, base, sc),
        ExprKind::Index { base, index, .. } => {
            scan_expr(tt, base, sc);
            scan_expr(tt, index, sc);
        }
        ExprKind::Slice {
            base, lo, hi, max, ..
        } => {
            scan_expr(tt, base, sc);
            for o in [lo, hi, max].into_iter().flatten() {
                scan_expr(tt, o, sc);
            }
        }
        ExprKind::Deref(x) => scan_expr(tt, x, sc),
        // 唯一产生指针的构造：`&x`
        ExprKind::Addr(p) => scan_addr_place(tt, p, sc),
        // `&T{...}` 取的是**新分配的**存储，不是任何槽
        ExprKind::AddrOf(x) => scan_expr(tt, x, sc),
        ExprKind::Unary { x, .. } => scan_expr(tt, x, sc),
        ExprKind::Binary { x, y, .. } => {
            scan_expr(tt, x, sc);
            scan_expr(tt, y, sc);
        }
        ExprKind::Call { args, .. } => {
            for a in args {
                scan_expr(tt, a, sc);
            }
        }
        ExprKind::Convert { x, .. } => scan_expr(tt, x, sc),
        ExprKind::Composite(c) => {
            for it in &c.items {
                match it {
                    CompositeItem::Value(x) => scan_expr(tt, x, sc),
                    CompositeItem::Key(k, v) => {
                        scan_expr(tt, k, sc);
                        scan_expr(tt, v, sc);
                    }
                    CompositeItem::Field(_, x) | CompositeItem::Index(_, x) => scan_expr(tt, x, sc),
                }
            }
        }
        ExprKind::FuncLit(_) => {
            // 函数字面量已提升为独立 `Func`，它的体**不属于**本函数 ——
            // 闭包里的 `&x` 会让 `x` 逃逸到闭包帧（`Root::Slot` 的边界），
            // 但 M6 把闭包整体拒掉（`SSA-UNSUPPORTED-CLOSURE`，Task 31），
            // 所以这里不扫。
        }
        ExprKind::Bound { recv, .. } => scan_expr(tt, recv, sc),
        ExprKind::TypeAssert { x, .. } => scan_expr(tt, x, sc),
        ExprKind::Make { kind, args } => {
            match kind {
                MakeKind::Slice | MakeKind::Map | MakeKind::Chan => {}
            }
            for a in args {
                scan_expr(tt, a, sc);
            }
        }
        ExprKind::New(_) => {}
        ExprKind::Builtin { args, .. } => {
            for a in args {
                scan_expr(tt, a, sc);
            }
        }
    }
}

/// `&place` 里的 place：**标记**被取址的槽。
fn scan_addr_place(tt: &TypeTable, p: &Place, sc: &mut Scanned) {
    match p {
        Place::Slot(s) => {
            sc.addressed.insert(*s);
        }
        Place::Global(_) => {}
        Place::Field { base, .. } => scan_addr_place(tt, base, sc),
        Place::Index { base, index } => {
            // `&arr[i]`：数组**元素在数组槽的存储里** ⇒ 数组槽本身被取址。
            // `&s[i]`（s 是 slice）元素在底层数组（堆根），s 这个槽不被取址 ——
            // 所以这里必须按**类型**分叉，不能一律保守地把 base 的槽记进来
            // （那会造出假根，把纯值化的机会白扔掉，还会让 dump 与预期不符）。
            if is_array(tt, base.typ) {
                scan_value_root(tt, base, sc, true);
            } else {
                scan_expr(tt, base, sc);
            }
            scan_expr(tt, index, sc);
        }
        // `&*p`：取的是 `p` 指向的存储，不是 `p` 这个槽
        Place::Deref(e) => scan_expr(tt, e, sc),
    }
}

/// 一个**值**的存储是否被整体取址（用于 `&arr[i]` 的底座）。
///
/// 只有「值直接住在某个槽里」的形态才算：`Slot`、以及嵌在结构体里的 `Field`。
/// 其余（`Deref` / `Index` / `Slice` / 调用结果）的存储都在别处（堆根或别人的帧），
/// 不产生本函数的 `Root::Slot`。
fn scan_value_root(tt: &TypeTable, e: &Expr, sc: &mut Scanned, addressed: bool) {
    match &e.kind {
        ExprKind::Slot(s) => {
            if addressed {
                sc.addressed.insert(*s);
            } else {
                sc.partial.insert(*s);
            }
        }
        ExprKind::Field { base, .. } => scan_value_root(tt, base, sc, addressed),
        // 其余形态：存储不在本函数的槽里，但仍要扫它们内部有没有 `&x`
        _ => scan_expr(tt, e, sc),
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
fn scan_lhs_place(tt: &TypeTable, p: &Place, sc: &mut Scanned) {
    match p {
        Place::Slot(_) | Place::Global(_) => {}
        // `s.f = ...`：s 被**部分写** ⇒ s 不能提升
        Place::Field { base, .. } => scan_place_root(tt, base, sc, false),
        Place::Index { base, index } => {
            // `arr[i] = ...`（数组）改的是 arr 的存储 ⇒ arr 部分写；
            // `s[i] = ...`（slice）改的是底层数组（堆根）⇒ 与槽无关。
            if is_array(tt, base.typ) {
                scan_value_root(tt, base, sc, false);
            } else {
                scan_expr(tt, base, sc);
            }
            scan_expr(tt, index, sc);
        }
        Place::Deref(e) => scan_expr(tt, e, sc),
    }
}

/// `place` 的底座槽归入「被取址」（`addressed = true`）或「被部分写」（`false`）。
fn scan_place_root(tt: &TypeTable, p: &Place, sc: &mut Scanned, addressed: bool) {
    match p {
        Place::Slot(s) => {
            if addressed {
                sc.addressed.insert(*s);
            } else {
                sc.partial.insert(*s);
            }
        }
        Place::Global(_) => {}
        Place::Field { base, .. } => scan_place_root(tt, base, sc, addressed),
        Place::Index { base, index } => {
            if is_array(tt, base.typ) {
                scan_value_root(tt, base, sc, addressed);
            } else {
                scan_expr(tt, base, sc);
            }
            scan_expr(tt, index, sc);
        }
        Place::Deref(e) => scan_expr(tt, e, sc),
    }
}

// ── 确定赋值分析（Task 19 的判据）────────────────────────────────────

/// 算「读到之前一定已经写过」的槽（这些才能在保持正确的前提下提升成纯值）。
///
/// # 为什么要这条分析，而不是「没取址就提升」
///
/// `var a int; print(a)` 里的 `a` 没被取址，但它**先读后写**（根本没写）。
/// 提升成纯值之后，读点找不到任何定义 —— 要么 panic、要么凭空造一个「零值」，
/// 而 SSA 里没有「零值」这个指令（`ConstVal` 只有 4 类具体值）。
/// 正确做法：**没有确定赋值就留在内存里**，让帧初始化（`C9` 的零值表）负责。
///
/// # 分支与循环的保守口径
///
/// * `if/else`：两条支路各自推进，汇合取**交集**（只在一支里写过的，汇合点不算定义）；
///   没有 `else` 时，`else` 支就是「没写」，所以交集含原状态。
/// * `for` / `range`：循环体可能 0 次，所以**体的推进不传出循环**；
///   但体内部的读仍按「进入循环前的状态」判定（首轮确实可能未定义）。
/// * `return` / `break` / `goto`：不改变 `must`（保守：只会少提升，不会多提升）。
///
/// 保守的代价只是「少提升几个槽」＝ 多一点 `Load/Store`，**不会算错**；
/// 反方向（多提升）才会算错，所以所有不确定处一律取保守。
struct Def<'a> {
    f: &'a Func,
    /// 读到过「当时还没写过」的槽 ⇒ 永远不提升。
    unsafe_slots: BTreeSet<SlotId>,
    /// 当前路径上已经确定写过的槽。
    must: BTreeSet<SlotId>,
}

/// 返回 `(确定赋值的槽, 读到过未定义值的槽)`。
///
/// 第二个集合是**必须留在内存里**的另一半：读点找不到定义 ⇒ 只能靠帧初始化
/// 的零值。它和「取址 / 部分写」一起构成根表（三者的道理不同，结果一样：
/// 这个槽不能提升）。
fn definite_slots(_tt: &TypeTable, f: &Func) -> (BTreeSet<SlotId>, BTreeSet<SlotId>) {
    let mut d = Def {
        f,
        unsafe_slots: BTreeSet::new(),
        must: f.params.iter().copied().collect(),
    };
    d.stmts(&f.body.stmts);
    let definite = d
        .must
        .difference(&d.unsafe_slots)
        .copied()
        .filter(|s| *s != BLANK_SLOT)
        .collect();
    (definite, d.unsafe_slots)
}

impl Def<'_> {
    fn read(&mut self, s: SlotId) {
        if s == BLANK_SLOT {
            return;
        }
        if !self.must.contains(&s) {
            self.unsafe_slots.insert(s);
        }
    }

    fn write(&mut self, s: SlotId) {
        if s != BLANK_SLOT {
            self.must.insert(s);
        }
    }

    fn stmts(&mut self, v: &[Stmt]) {
        for s in v {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Assign { lhs, op, rhs } => {
                for e in rhs {
                    self.expr(e);
                }
                // 复合赋值（`a += 1`）先读左值
                if op.is_some() {
                    for p in lhs {
                        self.read_place(p);
                    }
                }
                for p in lhs {
                    self.write_place(p);
                }
            }
            Stmt::ShortDecl { slots, rhs } => {
                for e in rhs {
                    self.expr(e);
                }
                for s in slots {
                    self.write(*s);
                }
            }
            Stmt::Expr(e) => self.expr(e),
            Stmt::If {
                init,
                cond,
                then,
                els,
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                self.expr(cond);
                let base = self.must.clone();
                self.stmts(&then.stmts);
                let then_must = std::mem::replace(&mut self.must, base);
                if let Some(e) = els {
                    self.stmt(e);
                }
                // 交集：两条支路都写过的才算
                self.must = then_must.intersection(&self.must).copied().collect();
            }
            Stmt::For {
                init,
                cond,
                post,
                body,
                ..
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                if let Some(c) = cond {
                    self.expr(c);
                }
                let base = self.must.clone();
                self.stmts(&body.stmts);
                if let Some(p) = post {
                    self.stmt(p);
                }
                // 循环体可能一次都不跑
                self.must = base;
            }
            Stmt::Range {
                key,
                value,
                x,
                body,
                ..
            } => {
                self.expr(x);
                if let Some(p) = key {
                    self.write_place(p);
                }
                if let Some(p) = value {
                    self.write_place(p);
                }
                let base = self.must.clone();
                self.stmts(&body.stmts);
                self.must = base;
            }
            Stmt::Switch {
                init, tag, cases, ..
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                if let Some(t) = tag {
                    self.expr(t);
                }
                let base = self.must.clone();
                let mut acc: Option<BTreeSet<SlotId>> = None;
                let mut has_default = false;
                for c in cases {
                    self.must = base.clone();
                    for e in &c.exprs {
                        self.expr(e);
                    }
                    self.stmts(&c.body.stmts);
                    has_default |= c.is_default;
                    acc = Some(match acc {
                        None => self.must.clone(),
                        Some(a) => a.intersection(&self.must).copied().collect(),
                    });
                }
                self.must = if has_default {
                    acc.unwrap_or(base)
                } else {
                    // 没有 default ⇒ 「一条都没匹配」这条路径也在
                    acc.unwrap_or_default()
                        .intersection(&base)
                        .copied()
                        .collect()
                };
            }
            Stmt::TypeSwitch { init, x, cases, .. } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                self.expr(x);
                let base = self.must.clone();
                for c in cases {
                    self.must = base.clone();
                    if let Some(b) = c.bind {
                        self.write(b);
                    }
                    self.stmts(&c.body.stmts);
                }
                self.must = base;
            }
            Stmt::Return(es) => {
                if es.is_empty() {
                    // 裸 return 读命名结果槽
                    for s in &self.f.results {
                        self.read(*s);
                    }
                } else {
                    for e in es {
                        self.expr(e);
                    }
                }
            }
            Stmt::Branch { .. } => {}
            Stmt::Labeled { stmt, .. } => self.stmt(stmt),
            Stmt::Block(b) => self.stmts(&b.stmts),
            Stmt::Defer { call } => self.expr(call),
            Stmt::Empty => {}
        }
    }

    fn read_place(&mut self, p: &Place) {
        match p {
            Place::Slot(s) => self.read(*s),
            Place::Global(_) => {}
            Place::Field { base, .. } => self.read_place(base),
            Place::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            Place::Deref(e) => self.expr(e),
        }
    }

    fn write_place(&mut self, p: &Place) {
        match p {
            Place::Slot(s) => self.write(*s),
            // 写字段/元素写的是内存（整个槽已因「部分写」进根表），不是整槽定义
            Place::Global(_) | Place::Field { .. } => {}
            Place::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            Place::Deref(e) => self.expr(e),
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Const(_) | ExprKind::Nil | ExprKind::Global(_) => {}
            ExprKind::Slot(s) => self.read(*s),
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Index { base, index, .. } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Slice {
                base, lo, hi, max, ..
            } => {
                self.expr(base);
                for o in [lo, hi, max].into_iter().flatten() {
                    self.expr(o);
                }
            }
            ExprKind::Deref(x) => self.expr(x),
            ExprKind::Addr(p) => self.read_place(p),
            ExprKind::AddrOf(x) => self.expr(x),
            ExprKind::Unary { x, .. } => self.expr(x),
            ExprKind::Binary { x, y, .. } => {
                self.expr(x);
                self.expr(y);
            }
            ExprKind::Call { args, .. } => {
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Convert { x, .. } => self.expr(x),
            ExprKind::Composite(c) => {
                for it in &c.items {
                    match it {
                        CompositeItem::Value(x) => self.expr(x),
                        CompositeItem::Key(k, v) => {
                            self.expr(k);
                            self.expr(v);
                        }
                        CompositeItem::Field(_, x) | CompositeItem::Index(_, x) => self.expr(x),
                    }
                }
            }
            ExprKind::FuncLit(_) | ExprKind::New(_) => {}
            ExprKind::Bound { recv, .. } => self.expr(recv),
            ExprKind::TypeAssert { x, .. } => self.expr(x),
            ExprKind::Make { args, .. } | ExprKind::Builtin { args, .. } => {
                for a in args {
                    self.expr(a);
                }
            }
        }
    }
}

// ── 函数体构建器（Task 20 起）────────────────────────────────────────

/// 边建块边产指令的单函数构建器。
///
/// # 三个状态
///
/// * `val: 槽 → 当前值`（只对**提升**的槽；取址/部分写/未确定赋值的槽不在里面）
/// * `mem: 根 → 当前 mem 令牌`（每个根一条线；`Store` 产新令牌、`Load` 不动它）
/// * `terminated`：当前块已有终止符 ⇒ 后续语句是死代码
///
/// # 为什么 mem 令牌是懒建的
///
/// 一个没有任何 `Load/Store` 的函数不该出现 mem 常量（`T-SSA-26`：纯函数里
/// 连一条访存指令都不该有）。所以令牌在**第一次真正访存**时才建。
struct Fb<'a> {
    tt: &'a mut TypeTable,
    f: &'a Func,
    globals: &'a [rgoc_hir::Global],
    values: Vec<TypeId>,
    blocks: Vec<Block>,
    cur: BlockId,
    val: BTreeMap<SlotId, ValueId>,
    mem: BTreeMap<RootId, ValueId>,
    roots: Vec<Root>,
    slot_root: BTreeMap<SlotId, RootId>,
    promoted: BTreeSet<SlotId>,
    /// 每个根的地址类型缓存（`*T`；避免同一根建出多个相同的指针类型槽）
    ptr_ty: BTreeMap<RootId, TypeId>,
    terminated: bool,
}

impl Fb<'_> {
    fn new_value(&mut self, t: TypeId) -> ValueId {
        self.values.push(t);
        ValueId((self.values.len() - 1) as u32)
    }

    fn emit(&mut self, i: Instr) {
        self.blocks[self.cur.index()].instrs.push(i);
    }

    fn set_term(&mut self, t: Term) {
        self.blocks[self.cur.index()].term = t;
        self.terminated = true;
    }

    /// 形参 → 入口块块参数；被取址的形参要把入参**存进它的根**。
    fn entry_params(&mut self) {
        let params = self.f.params.clone();
        let types = self.f.param_types.clone();
        for (s, t) in params.iter().zip(types) {
            let v = self.new_value(t);
            self.blocks[0].params.push(v);
            if self.promoted.contains(s) {
                self.val.insert(*s, v);
            } else if let Some(&root) = self.slot_root.get(s) {
                // 形参到手就是「已赋值」：rooted 的槽必须把它落进内存
                let addr = self.addr_of(root);
                self.store_root(root, addr, v);
            }
        }
    }

    fn elem_type(&self, root: RootId) -> TypeId {
        match self.roots.get(root.index()) {
            Some(Root::Slot(s)) => self
                .f
                .slot_types
                .get(s.index())
                .copied()
                .unwrap_or_else(|| TypeId::from_index(0)),
            Some(Root::Global(g)) => self
                .globals
                .get(g.index())
                .map_or_else(|| TypeId::from_index(0), |x| x.typ),
            Some(Root::Heap) | None => self
                .f
                .slot_types
                .first()
                .copied()
                .unwrap_or_else(|| TypeId::from_index(0)),
        }
    }

    fn ptr_ty_of(&mut self, root: RootId) -> TypeId {
        if let Some(&t) = self.ptr_ty.get(&root) {
            return t;
        }
        let elem = self.elem_type(root);
        let p = self.tt.add_pointer(elem);
        self.ptr_ty.insert(root, p);
        p
    }

    /// 根的地址（`AddrOf`）。整根读写都从它起步。
    fn addr_of(&mut self, root: RootId) -> ValueId {
        let t = self.ptr_ty_of(root);
        let dst = self.new_value(t);
        self.emit(Instr::AddrOf { dst, root });
        dst
    }

    /// 根的当前 mem 令牌（懒建；类型只是占位，`C9` 的 mem 是抽象令牌）。
    fn mem_of(&mut self, root: RootId) -> ValueId {
        if let Some(&m) = self.mem.get(&root) {
            return m;
        }
        let t = self.ptr_ty_of(root);
        let dst = self.new_value(t);
        self.emit(Instr::Const {
            dst,
            c: rgoc_hir::types::ConstVal::Int(0),
            typ: t,
        });
        self.mem.insert(root, dst);
        dst
    }

    fn load_root(&mut self, root: RootId, typ: TypeId) -> ValueId {
        let mem = self.mem_of(root);
        let addr = self.addr_of(root);
        let dst = self.new_value(typ);
        self.emit(Instr::Load {
            dst,
            mem,
            root,
            addr,
            typ,
        });
        dst
    }

    fn store_root(&mut self, root: RootId, addr: ValueId, v: ValueId) {
        let mem = self.mem_of(root);
        let t = self.ptr_ty_of(root);
        let dst = self.new_value(t);
        self.emit(Instr::Store {
            dst,
            mem,
            root,
            addr,
            val: v,
        });
        self.mem.insert(root, dst);
    }

    fn read_slot(&mut self, s: SlotId) -> ValueId {
        if let Some(&v) = self.val.get(&s) {
            return v;
        }
        let root = *self.slot_root.get(&s).unwrap_or_else(|| {
            panic!(
                "槽 s{} 既不提升也没有根（不该发生：promoted ∪ rooted == 全部槽）",
                s.0
            )
        });
        let t = self
            .f
            .slot_types
            .get(s.index())
            .copied()
            .unwrap_or_else(|| TypeId::from_index(0));
        self.load_root(root, t)
    }

    /// 把值的静态类型对齐到目标类型（不同就插一条 `Convert`）。
    ///
    /// # 为什么必须有这一步
    ///
    /// HIR 的表达式带着**未定型**的常量类型（`a := 1` 里 `1` 是 `untyped int`），
    /// 而槽有明确的静态类型（`int`）。Go 的这条隐式转换在 lowering 里没有落成节点，
    /// 所以 SSA 必须在**写入点**把它物化 —— 否则同一个槽在不同分支里会带两种类型，
    /// 汇合块的参数与边实参类型对不上（`V3` 会拒，而且拒得对）。
    ///
    /// 这条同时给出一条不变式：**槽的当前值，静态类型恒等于槽的静态类型**。
    fn coerce_to(&mut self, typ: TypeId, v: ValueId) -> ValueId {
        // 判等必须走 `identical`：同一个 `int` 在类型表里可以有多个条目
        if crate::typing::same_type(self.tt, self.values[v.index()], typ) {
            return v;
        }
        let dst = self.new_value(typ);
        self.emit(Instr::Convert { dst, x: v, typ });
        dst
    }

    fn slot_typ(&self, s: SlotId) -> TypeId {
        self.f
            .slot_types
            .get(s.index())
            .copied()
            .unwrap_or_else(|| TypeId::from_index(0))
    }

    fn write_slot(&mut self, s: SlotId, v: ValueId) {
        if s == BLANK_SLOT {
            return;
        }
        let v = self.coerce_to(self.slot_typ(s), v);
        if self.promoted.contains(&s) {
            self.val.insert(s, v);
            return;
        }
        let Some(&root) = self.slot_root.get(&s) else {
            // **死写**：这个槽既不提升也没有根 ⇒ 全函数没人能观察它
            // （有读 ⇒ 它在 `definite`（提升）或 `unsafe`（成根）里）。丢掉是安全的。
            return;
        };
        let addr = self.addr_of(root);
        self.store_root(root, addr, v);
    }

    fn heap_root(&mut self) -> RootId {
        if let Some(i) = self.roots.iter().position(|r| *r == Root::Heap) {
            return RootId(i as u32);
        }
        self.roots.push(Root::Heap);
        RootId((self.roots.len() - 1) as u32)
    }

    /// 一个 place 的「底座根」（`s.f` / `arr[i]` 落到 s 的根；`*p` 落到堆根）。
    fn root_of_place(&mut self, p: &Place) -> RootId {
        match p {
            Place::Slot(s) => *self
                .slot_root
                .get(s)
                .unwrap_or_else(|| panic!("槽 s{} 没有根（不该有 place 指向它）", s.0)),
            Place::Global(g) => RootId(g.0),
            Place::Field { base, .. } => self.root_of_place(base),
            Place::Index { base, .. } => {
                if is_array(self.tt, base.typ)
                    && let ExprKind::Slot(s) = &base.kind
                    && let Some(&r) = self.slot_root.get(s)
                {
                    return r;
                }
                self.heap_root()
            }
            Place::Deref(_) => self.heap_root(),
        }
    }

    /// 一个 place 的地址值。
    fn place_addr(&mut self, p: &Place) -> ValueId {
        match p {
            Place::Slot(s) => {
                let root = *self
                    .slot_root
                    .get(s)
                    .unwrap_or_else(|| panic!("槽 s{} 没有根", s.0));
                self.addr_of(root)
            }
            Place::Global(g) => self.addr_of(RootId(g.0)),
            Place::Field { base, index } => {
                let base_v = self.place_addr(base);
                let t = self.values[base_v.index()];
                let dst = self.new_value(t);
                self.emit(Instr::Field {
                    dst,
                    base: base_v,
                    index: *index,
                });
                dst
            }
            Place::Index { base, index } => {
                let base_v = self.expr(base);
                let idx = self.expr(index);
                let t = self.values[base_v.index()];
                let dst = self.new_value(t);
                self.emit(Instr::Index {
                    dst,
                    base: base_v,
                    i: idx,
                });
                dst
            }
            Place::Deref(e) => self.expr(e),
        }
    }

    fn expr(&mut self, e: &Expr) -> ValueId {
        match &e.kind {
            ExprKind::Const(c) => {
                let dst = self.new_value(e.typ);
                self.emit(Instr::Const {
                    dst,
                    c: c.clone(),
                    typ: e.typ,
                });
                dst
            }
            // 有类型的 `nil`：`Int(0)` 在非整数类型上表示**该类型的零值**
            // （`R-20-1`：SSA 没有独立的 nil 常量，而 `ConstVal` 是 M5 契约的一部分，
            // 不为了 SSA 去加第 5 个变体）
            ExprKind::Nil => {
                let dst = self.new_value(e.typ);
                self.emit(Instr::Const {
                    dst,
                    c: rgoc_hir::types::ConstVal::Int(0),
                    typ: e.typ,
                });
                dst
            }
            ExprKind::Slot(s) => self.read_slot(*s),
            ExprKind::Global(g) => {
                let root = RootId(g.0);
                self.load_root(root, e.typ)
            }
            ExprKind::Binary { op, x, y } => {
                let lhs = self.expr(x);
                let rhs = self.expr(y);
                let dst = self.new_value(e.typ);
                self.emit(Instr::Bin {
                    dst,
                    op: *op,
                    lhs,
                    rhs,
                    typ: e.typ,
                });
                dst
            }
            ExprKind::Unary { op, x } => {
                let xv = self.expr(x);
                let dst = self.new_value(e.typ);
                self.emit(Instr::Un {
                    dst,
                    op: *op,
                    x: xv,
                    typ: e.typ,
                });
                dst
            }
            ExprKind::Convert { to, x } => {
                let xv = self.expr(x);
                let dst = self.new_value(*to);
                self.emit(Instr::Convert {
                    dst,
                    x: xv,
                    typ: *to,
                });
                dst
            }
            // `&x`：唯一产生指针的构造。`x` 一定在根表里（`scan_addressed` 保证）
            ExprKind::Addr(p) => self.place_addr(p),
            // 读字段：底座在内存（部分写/取址）⇒ 先算字段地址再 Load
            ExprKind::Field { base, index } => {
                if let ExprKind::Slot(s) = &base.kind
                    && let Some(&root) = self.slot_root.get(s)
                {
                    let a = self.addr_of(root);
                    let at = self.values[a.index()];
                    let fa = self.new_value(at);
                    self.emit(Instr::Field {
                        dst: fa,
                        base: a,
                        index: *index,
                    });
                    let mem = self.mem_of(root);
                    let ty = e.typ;
                    let dst = self.new_value(ty);
                    self.emit(Instr::Load {
                        dst,
                        mem,
                        root,
                        addr: fa,
                        typ: ty,
                    });
                    return dst;
                }
                todo!("M6/Task 24：读字段（底座不是内存里的槽）的构建")
            }
            other => todo!(
                "M6/Task 23–28：表达式 `{}` 的构建还没实现",
                expr_kind_name(other)
            ),
        }
    }

    fn new_block(&mut self) -> BlockId {
        self.blocks.push(Block::default());
        BlockId((self.blocks.len() - 1) as u32)
    }

    /// `if` / `else` 与汇合块的块参数（`Task 22`）。
    ///
    /// # 并行交接为什么不靠「并行拷贝」
    ///
    /// 块参数的**实参是 SSA 值**（`Term::Goto { args }` 里就是 `ValueId`），
    /// 落地时一次性交给目标块的参数 —— 边上的 `args` 没有先后，所以
    /// `a, b = b, a` 交换天然正确（两个 arg 分别是交换前的两个值）。
    /// 真正的「并行赋值」风险在**求值器**（`ssa_exec` 必须一次性写参数）
    /// 与 **copy 传播 pass**（不许把块的参数替换成实参），不在这里。
    ///
    /// # 什么时候不需要块参数
    ///
    /// 两侧的当前值**相同**（同一个 `ValueId`）就不建参数 —— 多建一个参数
    /// 会让 dump 里多出一串无意义的 `v_k = ...`，也白白拉长活跃区间。
    ///
    /// # 一条边终止（`return` / `panic`）
    ///
    /// 汇合块只剩一条前驱，这时**不需要参数**（没有「两个来源」要合并），
    /// 直接把落空那一侧的当前值映射接续下去。两侧都终止 ⇒ 汇合块根本不存在
    /// （`builder` 不产出死块，`V8` 会拒）。
    fn stmt_if(
        &mut self,
        init: Option<&Stmt>,
        cond: &Expr,
        then: &rgoc_hir::Block,
        els: Option<&Stmt>,
    ) {
        if let Some(i) = init {
            self.stmt(i);
        }
        let c = self.expr(cond);
        let then_b = self.new_block();
        let else_b = self.new_block();
        self.set_term(Term::If {
            cond: c,
            then_: (then_b, Vec::new()),
            else_: (else_b, Vec::new()),
        });

        let base_map = self.val.clone();

        // ── then 支
        self.cur = then_b;
        self.terminated = false;
        self.val = base_map.clone();
        self.stmts(&then.stmts);
        let then_falls = !self.terminated;
        let then_tail = self.cur;
        let then_map = self.val.clone();

        // ── else 支
        self.cur = else_b;
        self.terminated = false;
        self.val = base_map;
        if let Some(e) = els {
            self.stmt(e);
        }
        let else_falls = !self.terminated;
        let else_tail = self.cur;
        let else_map = self.val.clone();

        match (then_falls, else_falls) {
            (true, true) => {
                let join = self.new_block();
                let mut params: Vec<ValueId> = Vec::new();
                let (mut ta, mut ea): (Vec<ValueId>, Vec<ValueId>) = (Vec::new(), Vec::new());
                let mut merged: BTreeMap<SlotId, ValueId> = BTreeMap::new();
                let slots: BTreeSet<SlotId> =
                    then_map.keys().chain(else_map.keys()).copied().collect();
                for s in slots {
                    match (then_map.get(&s).copied(), else_map.get(&s).copied()) {
                        (Some(a), Some(b)) if a == b => {
                            merged.insert(s, a);
                        }
                        (Some(a), Some(b)) => {
                            let t = self.values[a.index()];
                            let p = self.new_value(t);
                            params.push(p);
                            ta.push(a);
                            ea.push(b);
                            merged.insert(s, p);
                        }
                        // 只在一侧有值：该槽没有提升（`Def` 的交集保证），不该出现
                        _ => {}
                    }
                }
                self.blocks[join.index()].params = params;
                self.blocks[then_tail.index()].term = Term::Goto { to: join, args: ta };
                self.blocks[else_tail.index()].term = Term::Goto { to: join, args: ea };
                self.cur = join;
                self.terminated = false;
                self.val = merged;
            }
            (true, false) => {
                let join = self.new_block();
                self.blocks[then_tail.index()].term = Term::Goto {
                    to: join,
                    args: Vec::new(),
                };
                self.cur = join;
                self.terminated = false;
                self.val = then_map;
            }
            (false, true) => {
                let join = self.new_block();
                self.blocks[else_tail.index()].term = Term::Goto {
                    to: join,
                    args: Vec::new(),
                };
                self.cur = join;
                self.terminated = false;
                self.val = else_map;
            }
            (false, false) => {
                // 两侧都终止 ⇒ `if` 之后不可达，不建汇合块（死块会被 V8 拒）
                self.terminated = true;
            }
        }
    }

    fn stmts(&mut self, v: &[Stmt]) {
        for s in v {
            if self.terminated {
                // 目前只有 `return` 会终止；之后的语句是死代码
                break;
            }
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::ShortDecl { slots, rhs } => {
                if rhs.len() != slots.len() {
                    todo!(
                        "M6/Task 27：多值调用展开（`{} 个槽 ← {} 个右值`）",
                        slots.len(),
                        rhs.len()
                    );
                }
                // 右值**全部先求值**（Go 的赋值语义），再逐个写
                let vs: Vec<ValueId> = rhs.iter().map(|e| self.expr(e)).collect();
                for (slot, v) in slots.iter().zip(vs) {
                    self.write_slot(*slot, v);
                }
            }
            Stmt::Assign { lhs, op, rhs } => match op {
                None => {
                    if rhs.len() != lhs.len() {
                        todo!("M6/Task 27：多值调用展开（赋值）");
                    }
                    // `a, b = b, a` 的正确性来自这里：右值先全部求值成 SSA 值，
                    // 写的时候已经不存在「先写 a 再读 b」的问题
                    let vs: Vec<ValueId> = rhs.iter().map(|e| self.expr(e)).collect();
                    for (p, v) in lhs.iter().zip(vs) {
                        self.write_place(p, v);
                    }
                }
                Some(op) => {
                    // 复合赋值：`x op= y` ⇒ `x = x op y`
                    let [p] = lhs.as_slice() else {
                        todo!("M6/Task 21：多左值复合赋值");
                    };
                    let [e] = rhs.as_slice() else {
                        todo!("M6/Task 21：多右值复合赋值");
                    };
                    let cur = self.read_place(p);
                    let v = self.expr(e);
                    let t = self.values[cur.index()];
                    let dst = self.new_value(t);
                    self.emit(Instr::Bin {
                        dst,
                        op: *op,
                        lhs: cur,
                        rhs: v,
                        typ: t,
                    });
                    self.write_place(p, dst);
                }
            },
            Stmt::Expr(e) => {
                self.expr(e);
            }
            Stmt::Return(es) => {
                let vals: Vec<ValueId> = if es.is_empty() {
                    // 裸 return：读命名结果槽（`func f() (r int) { ...; return }`）
                    self.f
                        .results
                        .clone()
                        .into_iter()
                        .map(|s| self.read_slot(s))
                        .collect()
                } else {
                    es.iter().map(|e| self.expr(e)).collect()
                };
                self.set_term(Term::Return(vals));
            }
            Stmt::Block(b) => self.stmts(&b.stmts),
            Stmt::Labeled { stmt, .. } => self.stmt(stmt),
            Stmt::Empty => {}
            Stmt::If {
                init,
                cond,
                then,
                els,
            } => self.stmt_if(init.as_deref(), cond, then, els.as_deref()),
            other => todo!(
                "M6/Task 23–30：语句 `{}` 的构建还没实现",
                stmt_kind_name(other)
            ),
        }
    }

    fn read_place(&mut self, p: &Place) -> ValueId {
        match p {
            Place::Slot(s) => self.read_slot(*s),
            Place::Global(g) => self.load_root(RootId(g.0), self.elem_type(RootId(g.0))),
            other => {
                let addr = self.place_addr(other);
                let root = self.root_of_place(other);
                let mem = self.mem_of(root);
                let t = self.values[addr.index()];
                let dst = self.new_value(t);
                self.emit(Instr::Load {
                    dst,
                    mem,
                    root,
                    addr,
                    typ: t,
                });
                dst
            }
        }
    }

    fn write_place(&mut self, p: &Place, v: ValueId) {
        match p {
            Place::Slot(s) => self.write_slot(*s, v),
            Place::Global(g) => {
                let root = RootId(g.0);
                let addr = self.addr_of(root);
                self.store_root(root, addr, v);
            }
            other => {
                let addr = self.place_addr(other);
                let root = self.root_of_place(other);
                self.store_root(root, addr, v);
            }
        }
    }

    fn finish(self, id: FuncId) -> SsaFunc {
        SsaFunc {
            src: id,
            name: self.f.name.clone(),
            params: self.blocks[0].params.clone(),
            blocks: self.blocks,
            entry: BlockId(0),
            roots: self.roots,
            slot_root: self.slot_root,
            value_types: self.values,
            call_sigs: BTreeMap::new(),
        }
    }
}

/// 表达式节点名（`todo!` 消息里用；**不带通配的 `match`**）。
fn expr_kind_name(k: &ExprKind) -> &'static str {
    match k {
        ExprKind::Const(_) => "const",
        ExprKind::Slot(_) => "slot",
        ExprKind::Global(_) => "global",
        ExprKind::Field { .. } => "field",
        ExprKind::Index { .. } => "index",
        ExprKind::Slice { .. } => "slice",
        ExprKind::Deref(_) => "deref",
        ExprKind::Addr(_) => "addr",
        ExprKind::AddrOf(_) => "addrof",
        ExprKind::Unary { .. } => "unary",
        ExprKind::Binary { .. } => "binary",
        ExprKind::Call { .. } => "call",
        ExprKind::Convert { .. } => "convert",
        ExprKind::Composite(_) => "composite",
        ExprKind::FuncLit(_) => "funclit",
        ExprKind::Bound { .. } => "bound",
        ExprKind::TypeAssert { .. } => "typeassert",
        ExprKind::Make { .. } => "make",
        ExprKind::New(_) => "new",
        ExprKind::Builtin { .. } => "builtin",
        ExprKind::Nil => "nil",
    }
}

fn stmt_kind_name(s: &Stmt) -> &'static str {
    match s {
        Stmt::Assign { .. } => "assign",
        Stmt::ShortDecl { .. } => "shortdecl",
        Stmt::Expr(_) => "expr",
        Stmt::If { .. } => "if",
        Stmt::For { .. } => "for",
        Stmt::Range { .. } => "range",
        Stmt::Switch { .. } => "switch",
        Stmt::TypeSwitch { .. } => "typeswitch",
        Stmt::Return(_) => "return",
        Stmt::Branch { .. } => "branch",
        Stmt::Labeled { .. } => "labeled",
        Stmt::Block(_) => "block",
        Stmt::Defer { .. } => "defer",
        Stmt::Empty => "empty",
    }
}
