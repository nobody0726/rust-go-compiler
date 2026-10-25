//! 调用摘要（`Task 27`）：一个函数「可能写哪些根 / 会不会 panic / 会不会回调」。
//!
//! # 为什么需要它
//!
//! `Instr::Call` 在 IR 里**没有 mem 操作数**（`R-13-1` 的 `SsaFunc` 自足设计的代价）。
//! 所以「调用写在它前面的 `Store` 与写在它后面的 `Load` 之间怎么排序」不能靠 mem 链
//! 表达，只能靠 `Effect` 摘要。没有摘要，构建器只能把每个调用都标成「什么都能写」，
//! 那等于让所有 pass 都不敢动任何跨调用的顺序 —— 也就丢掉了 M6d 的意义。
//!
//! # 口径（写清楚，别让读的人自己猜）
//!
//! | 字段 | 含义 | 保守边界 |
//! |---|---|---|
//! | `writes` | **精确**写到的全局变量（含经被调方传递） | 只收 `Root::Global` |
//! | `writes_all` | 「可能写任何根」 | 指针/slice/map 写、`append` 等内建、未知被调方（间接/接口调用） |
//! | `may_panic` | 显式 `panic` 或经被调方传递 | **不含**除零、越界这类指令级 panic（那要逐指令分析，M6 不做） |
//! | `may_call_back` | 会调用函数值 / 接口方法（可重入本包任意函数） | 只看直接出现的 `Callee::Value` / `Callee::Iface` |
//!
//! **本函数自己的帧槽（`Root::Slot`）不进摘要** —— 它对调用方不可见，写它不会
//! 影响任何跨函数的东西。
//!
//! # 为什么用「并集到不动点」而不是「递归 + 环检测」
//!
//! 递归函数调用自己时，`writes` 的正确答案是「自己的并集」—— 那是一个单调
//! 方程，迭代到不动点就收敛（最坏 O(环长) 轮）。用环检测 + `writes_all = true`
//! 会把纯递归函数（`fib`）误标成「什么都能写」，白扔掉优化机会。

use std::collections::BTreeSet;

use rgoc_hir::{
    Block as HirBlock, BuiltinKind, Callee, CompositeItem, Expr, ExprKind, FuncId, GlobalId,
    MakeKind, Place, Program, SlotId, Stmt,
};
use rgoc_types::{TypeId, TypeTable};

use crate::ir::{Effect, RootId};

/// 一个函数的副作用摘要。
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct Summary {
    /// 精确写到的全局变量（含经被调方传递）。
    pub writes: BTreeSet<GlobalId>,
    /// 「可能写任何根」：指针写、内建写、未知被调方。
    pub writes_all: bool,
    /// 显式 `panic` 或经被调方传递。
    pub may_panic: bool,
    /// 会调用函数值 / 接口方法。
    pub may_call_back: bool,
    /// 会往 stdout 写字节（`print` / `println`，含经被调方传递）。
    pub prints: bool,
}

impl Summary {
    fn merge_from(&mut self, o: &Summary) -> bool {
        let before = (
            self.writes.len(),
            self.writes_all,
            self.may_panic,
            self.may_call_back,
            self.prints,
        );
        self.writes.extend(o.writes.iter().copied());
        self.writes_all |= o.writes_all;
        self.prints |= o.prints;
        self.may_panic |= o.may_panic;
        self.may_call_back |= o.may_call_back;
        before
            != (
                self.writes.len(),
                self.writes_all,
                self.may_panic,
                self.may_call_back,
                self.prints,
            )
    }
}

/// 一个 place 的「存储归属」（只分摘要关心的三类）。
enum Owner {
    /// 包级变量。
    Global(GlobalId),
    /// 本函数的帧槽 —— 调用方看不见，不影响摘要。
    Local,
    /// 堆：可能别名任何全局。
    Heap,
}

/// 摘要扫描要的静态上下文。
///
/// # 为什么必须带类型
///
/// 「写的是本地还是堆」取决于**底座是不是指针**：`s.f = 1`（`s` 是结构体值）
/// 只改本地存储，`p.f = 1`（`p *S`）改的是**别人**的存储。HIR 的 place 两者
/// 长得一样（`Field { base: Slot }`，Go 自动解引用），所以只能看槽的静态类型。
///
/// 少了这条，`func (p *P) add(d int) { p.x += d }` 的摘要就是「什么都不写」——
/// 于是 `dce` 会把这个调用整个删掉（实测：`p.add(4)` 消失，输出少加一次）。
struct Ctx<'a> {
    /// 类型表。
    tt: &'a TypeTable,
    /// 本函数的槽类型（`Func::slot_types`）。
    slots: &'a [TypeId],
}

impl Ctx<'_> {
    /// 这个类型是**引用型**吗（指针 / 切片 / map，含具名与别名）。
    ///
    /// 三种都算：通过它们写，写的是**别人**的存储。`s[i] = v`（`s []int`）
    /// 改的是底层数组，不是 `s` 这个槽 —— 判成「本地」的话摘要就会说
    /// 「这个函数什么都不写」，`dce` 会把调用整个删掉（实测 `ken/array.go`：
    /// `setpd(a)` 被删，`t` 一直是 0）。
    fn is_ref(&self, t: TypeId) -> bool {
        if t.index() >= self.tt.len() {
            return false;
        }
        matches!(
            self.tt.type_of(rgoc_types::underlying(self.tt, t)),
            rgoc_types::typ::Type::Pointer(_)
                | rgoc_types::typ::Type::Slice(_)
                | rgoc_types::typ::Type::Map { .. }
        )
    }

    /// 这个槽是引用型吗（越界 / 没登记 ⇒ 不是）。
    fn slot_is_ref(&self, s: SlotId) -> bool {
        self.slots.get(s.index()).is_some_and(|t| self.is_ref(*t))
    }
}

/// 算整个包的摘要（按 `FuncId` 索引，与 `Program.funcs` 同长）。
#[must_use]
pub fn summarize(program: &Program, tt: &TypeTable) -> Vec<Summary> {
    // ── ① 每个函数的**直接**副作用 + 直接调用边
    let mut direct: Vec<Summary> = Vec::with_capacity(program.funcs.len());
    let mut calls: Vec<Vec<FuncId>> = Vec::with_capacity(program.funcs.len());
    for f in &program.funcs {
        let mut s = Summary::default();
        let mut cs: Vec<FuncId> = Vec::new();
        let cx = Ctx {
            tt,
            slots: &f.slot_types,
        };
        scan_block(&cx, &f.body, &mut s, &mut cs);
        cs.sort_unstable_by_key(|x| x.0);
        cs.dedup();
        direct.push(s);
        calls.push(cs);
    }

    // ── ② 并集到不动点
    let mut eff = direct;
    loop {
        let mut changed = false;
        for i in 0..eff.len() {
            for &callee in &calls[i] {
                let c = eff[callee.index()].clone();
                if eff[i].merge_from(&c) {
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    eff
}

/// 内建的副作用（不看实参，只看是哪个内建）。
///
/// `append` / `delete` / `copy` / `clear` 都写**底层数组**，而底层数组可能与
/// 某个全局 slice 共享 ⇒ 保守记 `writes_all`。
#[must_use]
pub fn builtin_summary(k: BuiltinKind) -> Summary {
    let mut s = Summary::default();
    match k {
        BuiltinKind::Append
        | BuiltinKind::Delete
        | BuiltinKind::Copy
        | BuiltinKind::Clear
        | BuiltinKind::Close => s.writes_all = true,
        BuiltinKind::Panic => s.may_panic = true,
        // 输出是**可观察**的（`C9` 的合并流）：`print` 不写内存，但删了少一行
        BuiltinKind::Print | BuiltinKind::Println => s.prints = true,
        // 纯读 / 纯分配：`new`/`make` 只写新分配的存储
        BuiltinKind::Len
        | BuiltinKind::Cap
        | BuiltinKind::New
        | BuiltinKind::Min
        | BuiltinKind::Max
        | BuiltinKind::Complex
        | BuiltinKind::Real
        | BuiltinKind::Imag
        | BuiltinKind::Recover => {}
    }
    s
}

fn scan_block(cx: &Ctx, b: &HirBlock, s: &mut Summary, calls: &mut Vec<FuncId>) {
    for st in &b.stmts {
        scan_stmt(cx, st, s, calls);
    }
}

fn scan_stmt(cx: &Ctx, st: &Stmt, s: &mut Summary, calls: &mut Vec<FuncId>) {
    match st {
        Stmt::Go { .. } | Stmt::Send { .. } | Stmt::Select { .. } => {
            // 由 `unsupported()` 在建图前拒掉（`SSA-UNSUPPORTED-CHAN`）。
            // 走到这里 = 预扫描漏了，是实现 bug，不是「还没实现」（同 build_range 的先例）。
            unreachable!("chan/go/select 应由 unsupported() 在建图前拒掉")
        }
        Stmt::Assign { lhs, rhs, .. } => {
            for e in rhs {
                scan_expr(cx, e, s, calls);
            }
            for p in lhs {
                match owner_of_place(cx, p) {
                    Owner::Global(g) => {
                        s.writes.insert(g);
                    }
                    Owner::Local => {}
                    Owner::Heap => s.writes_all = true,
                }
                // 左值里嵌的表达式（`a[i()] = 1`）也要扫
                scan_place_exprs(cx, p, s, calls);
            }
        }
        Stmt::ShortDecl { rhs, .. } => {
            for e in rhs {
                scan_expr(cx, e, s, calls);
            }
        }
        Stmt::Expr(e) => scan_expr(cx, e, s, calls),
        Stmt::If {
            init,
            cond,
            then,
            els,
        } => {
            if let Some(i) = init {
                scan_stmt(cx, i, s, calls);
            }
            scan_expr(cx, cond, s, calls);
            scan_block(cx, then, s, calls);
            if let Some(e) = els {
                scan_stmt(cx, e, s, calls);
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
                scan_stmt(cx, i, s, calls);
            }
            if let Some(c) = cond {
                scan_expr(cx, c, s, calls);
            }
            if let Some(p) = post {
                scan_stmt(cx, p, s, calls);
            }
            scan_block(cx, body, s, calls);
        }
        Stmt::Range {
            key,
            value,
            x,
            body,
            ..
        } => {
            scan_expr(cx, x, s, calls);
            for p in [key, value].into_iter().flatten() {
                if let Owner::Global(g) = owner_of_place(cx, p) {
                    s.writes.insert(g);
                }
                scan_place_exprs(cx, p, s, calls);
            }
            scan_block(cx, body, s, calls);
        }
        Stmt::Switch {
            init, tag, cases, ..
        } => {
            if let Some(i) = init {
                scan_stmt(cx, i, s, calls);
            }
            if let Some(t) = tag {
                scan_expr(cx, t, s, calls);
            }
            for c in cases {
                for e in &c.exprs {
                    scan_expr(cx, e, s, calls);
                }
                scan_block(cx, &c.body, s, calls);
            }
        }
        Stmt::TypeSwitch { init, x, cases, .. } => {
            if let Some(i) = init {
                scan_stmt(cx, i, s, calls);
            }
            scan_expr(cx, x, s, calls);
            for c in cases {
                scan_block(cx, &c.body, s, calls);
            }
        }
        Stmt::Return(es) => {
            for e in es {
                scan_expr(cx, e, s, calls);
            }
        }
        Stmt::Branch { .. } | Stmt::Empty => {}
        Stmt::Labeled { stmt, .. } => scan_stmt(cx, stmt, s, calls),
        Stmt::Block(b) => scan_block(cx, b, s, calls),
        Stmt::Defer { call } => scan_expr(cx, call, s, calls),
    }
}

fn scan_expr(cx: &Ctx, e: &Expr, s: &mut Summary, calls: &mut Vec<FuncId>) {
    match &e.kind {
        ExprKind::Recv { .. } => {
            // 由 `unsupported()` 在建图前拒掉（`SSA-UNSUPPORTED-CHAN`）。
            unreachable!("`<-ch` 应由 unsupported() 在建图前拒掉")
        }
        ExprKind::Const(_) | ExprKind::Slot(_) | ExprKind::Global(_) | ExprKind::Nil => {}
        ExprKind::Field { base, .. } => scan_expr(cx, base, s, calls),
        ExprKind::Index { base, index, .. } => {
            scan_expr(cx, base, s, calls);
            scan_expr(cx, index, s, calls);
        }
        ExprKind::Slice {
            base, lo, hi, max, ..
        } => {
            scan_expr(cx, base, s, calls);
            for o in [lo, hi, max].into_iter().flatten() {
                scan_expr(cx, o, s, calls);
            }
        }
        ExprKind::Deref(x) => scan_expr(cx, x, s, calls),
        ExprKind::Addr(p) => scan_place_exprs(cx, p, s, calls),
        ExprKind::AddrOf(x) => scan_expr(cx, x, s, calls),
        ExprKind::Unary { x, .. } => scan_expr(cx, x, s, calls),
        ExprKind::Binary { x, y, .. } => {
            scan_expr(cx, x, s, calls);
            scan_expr(cx, y, s, calls);
        }
        ExprKind::Call { callee, args, .. } => {
            for a in args {
                scan_expr(cx, a, s, calls);
            }
            match callee {
                Callee::Func(fid) => calls.push(*fid),
                Callee::Method { recv, func } => {
                    scan_expr(cx, recv, s, calls);
                    calls.push(*func);
                }
                Callee::Builtin(k) => {
                    // 逐字段手抄会漏：`prints` 就是漏掉的那一个（`println` 在函数体里
                    // 不算这个函数的副作用 ⇒ 调用它的地方被 `dce` 删掉，输出少一半）。
                    // 统一走 `merge_from`，加字段时不会再漏第二次。
                    let b = builtin_summary(*k);
                    s.merge_from(&b);
                }
                // 函数值 / 接口方法：目标运行期才定 ⇒ 什么都能写、可能回调
                Callee::Value(v) => {
                    scan_expr(cx, v, s, calls);
                    s.writes_all = true;
                    s.may_call_back = true;
                }
                Callee::Iface { recv, .. } => {
                    scan_expr(cx, recv, s, calls);
                    s.writes_all = true;
                    s.may_call_back = true;
                }
            }
        }
        ExprKind::Convert { x, .. } => scan_expr(cx, x, s, calls),
        ExprKind::Composite(c) => {
            for it in &c.items {
                match it {
                    CompositeItem::Value(x) => scan_expr(cx, x, s, calls),
                    CompositeItem::Key(k, v) => {
                        scan_expr(cx, k, s, calls);
                        scan_expr(cx, v, s, calls);
                    }
                    CompositeItem::Field(_, x) | CompositeItem::Index(_, x) => {
                        scan_expr(cx, x, s, calls);
                    }
                }
            }
        }
        ExprKind::FuncLit(_) => {}
        ExprKind::Bound { recv, .. } => scan_expr(cx, recv, s, calls),
        ExprKind::TypeAssert { x, .. } => scan_expr(cx, x, s, calls),
        ExprKind::Make { kind, args } => {
            // `make` 分配新存储；但 `make([]T, n)` 之后对它写就是堆写，
            // 那会在赋值点被 `Owner::Heap` 捕获。这里只扫实参。
            match kind {
                MakeKind::Slice | MakeKind::Map | MakeKind::Chan => {}
            }
            for a in args {
                scan_expr(cx, a, s, calls);
            }
        }
        ExprKind::New(_) => {}
        ExprKind::Builtin { kind, args, .. } => {
            for a in args {
                scan_expr(cx, a, s, calls);
            }
            // 与 `Callee::Builtin` 同一条纪律：走 `merge_from`，不逐字段手抄
            // （`prints` 就是手抄漏掉的那一个 —— `println` 在函数体里不算副作用，
            // 于是调用它的地方被 `dce` 删掉，`cplx0.go` 少打一半）。
            let b = builtin_summary(*kind);
            s.merge_from(&b);
        }
    }
}

fn scan_place_exprs(cx: &Ctx, p: &Place, s: &mut Summary, calls: &mut Vec<FuncId>) {
    match p {
        Place::Slot(_) | Place::Global(_) => {}
        Place::Field { base, .. } => scan_place_exprs(cx, base, s, calls),
        Place::Index { base, index } => {
            scan_expr(cx, base, s, calls);
            scan_expr(cx, index, s, calls);
        }
        Place::Deref(x) => scan_expr(cx, x, s, calls),
    }
}

fn owner_of_place(cx: &Ctx, p: &Place) -> Owner {
    match p {
        // 指针槽：`p.f = 1` / `p[0] = 1` 写的是它**指向**的存储
        Place::Slot(s) if cx.slot_is_ref(*s) => Owner::Heap,
        Place::Slot(_) => Owner::Local,
        Place::Global(g) => Owner::Global(*g),
        Place::Field { base, .. } => owner_of_place(cx, base),
        Place::Index { base, .. } => owner_of_expr(cx, base),
        Place::Deref(_) => Owner::Heap,
    }
}

fn owner_of_expr(cx: &Ctx, e: &Expr) -> Owner {
    match &e.kind {
        ExprKind::Slot(s) if cx.slot_is_ref(*s) => Owner::Heap,
        ExprKind::Slot(_) => Owner::Local,
        ExprKind::Global(g) => Owner::Global(*g),
        ExprKind::Field { base, .. } => owner_of_expr(cx, base),
        // `s[i]`：`s` 是 slice 时元素在底层数组（堆）；是数组时在 s 的存储里。
        // 摘要层面不追类型（那要 `TypeTable`），一律按堆算 —— 保守方向。
        _ => Owner::Heap,
    }
}

/// 把摘要转成 IR 的 `Effect`。
///
/// # `RootId` 的跨函数一致性
///
/// `Effect.writes` 是 `RootId`，而 `RootId` 本来是**函数内**编号。这里能跨函数
/// 直接用，靠的是一条构造顺序：`build_func` 把全局根排在**最前面**（`RootId(i)`
/// == `Global(i)`，见根表构造）。所以「写第 3 个全局」在任何函数里都是 `RootId(3)`。
/// 堆根是函数内动态分配的，**不能**这样映射 ⇒ 堆写一律走 `writes_all`
/// （这也符合语义：指针写本来就可能别名任何全局）。
#[must_use]
pub fn to_effect(s: &Summary) -> Effect {
    Effect {
        writes: s.writes.iter().map(|g| RootId(g.0)).collect(),
        writes_all: s.writes_all,
        may_panic: s.may_panic,
        may_call_back: s.may_call_back,
        prints: s.prints,
    }
}
