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
    MakeKind, Place, Program, Stmt,
};

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
}

impl Summary {
    fn merge_from(&mut self, o: &Summary) -> bool {
        let before = (
            self.writes.len(),
            self.writes_all,
            self.may_panic,
            self.may_call_back,
        );
        self.writes.extend(o.writes.iter().copied());
        self.writes_all |= o.writes_all;
        self.may_panic |= o.may_panic;
        self.may_call_back |= o.may_call_back;
        before
            != (
                self.writes.len(),
                self.writes_all,
                self.may_panic,
                self.may_call_back,
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

/// 算整个包的摘要（按 `FuncId` 索引，与 `Program.funcs` 同长）。
#[must_use]
pub fn summarize(program: &Program) -> Vec<Summary> {
    // ── ① 每个函数的**直接**副作用 + 直接调用边
    let mut direct: Vec<Summary> = Vec::with_capacity(program.funcs.len());
    let mut calls: Vec<Vec<FuncId>> = Vec::with_capacity(program.funcs.len());
    for f in &program.funcs {
        let mut s = Summary::default();
        let mut cs: Vec<FuncId> = Vec::new();
        scan_block(&f.body, &mut s, &mut cs);
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
        // 纯读 / 纯分配：`new`/`make` 只写新分配的存储
        BuiltinKind::Len
        | BuiltinKind::Cap
        | BuiltinKind::New
        | BuiltinKind::Min
        | BuiltinKind::Max
        | BuiltinKind::Complex
        | BuiltinKind::Real
        | BuiltinKind::Imag
        | BuiltinKind::Print
        | BuiltinKind::Println
        | BuiltinKind::Recover => {}
    }
    s
}

fn scan_block(b: &HirBlock, s: &mut Summary, calls: &mut Vec<FuncId>) {
    for st in &b.stmts {
        scan_stmt(st, s, calls);
    }
}

fn scan_stmt(st: &Stmt, s: &mut Summary, calls: &mut Vec<FuncId>) {
    match st {
        Stmt::Assign { lhs, rhs, .. } => {
            for e in rhs {
                scan_expr(e, s, calls);
            }
            for p in lhs {
                match owner_of_place(p) {
                    Owner::Global(g) => {
                        s.writes.insert(g);
                    }
                    Owner::Local => {}
                    Owner::Heap => s.writes_all = true,
                }
                // 左值里嵌的表达式（`a[i()] = 1`）也要扫
                scan_place_exprs(p, s, calls);
            }
        }
        Stmt::ShortDecl { rhs, .. } => {
            for e in rhs {
                scan_expr(e, s, calls);
            }
        }
        Stmt::Expr(e) => scan_expr(e, s, calls),
        Stmt::If {
            init,
            cond,
            then,
            els,
        } => {
            if let Some(i) = init {
                scan_stmt(i, s, calls);
            }
            scan_expr(cond, s, calls);
            scan_block(then, s, calls);
            if let Some(e) = els {
                scan_stmt(e, s, calls);
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
                scan_stmt(i, s, calls);
            }
            if let Some(c) = cond {
                scan_expr(c, s, calls);
            }
            if let Some(p) = post {
                scan_stmt(p, s, calls);
            }
            scan_block(body, s, calls);
        }
        Stmt::Range {
            key,
            value,
            x,
            body,
            ..
        } => {
            scan_expr(x, s, calls);
            for p in [key, value].into_iter().flatten() {
                if let Owner::Global(g) = owner_of_place(p) {
                    s.writes.insert(g);
                }
                scan_place_exprs(p, s, calls);
            }
            scan_block(body, s, calls);
        }
        Stmt::Switch {
            init, tag, cases, ..
        } => {
            if let Some(i) = init {
                scan_stmt(i, s, calls);
            }
            if let Some(t) = tag {
                scan_expr(t, s, calls);
            }
            for c in cases {
                for e in &c.exprs {
                    scan_expr(e, s, calls);
                }
                scan_block(&c.body, s, calls);
            }
        }
        Stmt::TypeSwitch { init, x, cases, .. } => {
            if let Some(i) = init {
                scan_stmt(i, s, calls);
            }
            scan_expr(x, s, calls);
            for c in cases {
                scan_block(&c.body, s, calls);
            }
        }
        Stmt::Return(es) => {
            for e in es {
                scan_expr(e, s, calls);
            }
        }
        Stmt::Branch { .. } | Stmt::Empty => {}
        Stmt::Labeled { stmt, .. } => scan_stmt(stmt, s, calls),
        Stmt::Block(b) => scan_block(b, s, calls),
        Stmt::Defer { call } => scan_expr(call, s, calls),
    }
}

fn scan_expr(e: &Expr, s: &mut Summary, calls: &mut Vec<FuncId>) {
    match &e.kind {
        ExprKind::Const(_) | ExprKind::Slot(_) | ExprKind::Global(_) | ExprKind::Nil => {}
        ExprKind::Field { base, .. } => scan_expr(base, s, calls),
        ExprKind::Index { base, index, .. } => {
            scan_expr(base, s, calls);
            scan_expr(index, s, calls);
        }
        ExprKind::Slice {
            base, lo, hi, max, ..
        } => {
            scan_expr(base, s, calls);
            for o in [lo, hi, max].into_iter().flatten() {
                scan_expr(o, s, calls);
            }
        }
        ExprKind::Deref(x) => scan_expr(x, s, calls),
        ExprKind::Addr(p) => scan_place_exprs(p, s, calls),
        ExprKind::AddrOf(x) => scan_expr(x, s, calls),
        ExprKind::Unary { x, .. } => scan_expr(x, s, calls),
        ExprKind::Binary { x, y, .. } => {
            scan_expr(x, s, calls);
            scan_expr(y, s, calls);
        }
        ExprKind::Call { callee, args, .. } => {
            for a in args {
                scan_expr(a, s, calls);
            }
            match callee {
                Callee::Func(fid) => calls.push(*fid),
                Callee::Method { recv, func } => {
                    scan_expr(recv, s, calls);
                    calls.push(*func);
                }
                Callee::Builtin(k) => {
                    let b = builtin_summary(*k);
                    s.writes_all |= b.writes_all;
                    s.may_panic |= b.may_panic;
                }
                // 函数值 / 接口方法：目标运行期才定 ⇒ 什么都能写、可能回调
                Callee::Value(v) => {
                    scan_expr(v, s, calls);
                    s.writes_all = true;
                    s.may_call_back = true;
                }
                Callee::Iface { recv, .. } => {
                    scan_expr(recv, s, calls);
                    s.writes_all = true;
                    s.may_call_back = true;
                }
            }
        }
        ExprKind::Convert { x, .. } => scan_expr(x, s, calls),
        ExprKind::Composite(c) => {
            for it in &c.items {
                match it {
                    CompositeItem::Value(x) => scan_expr(x, s, calls),
                    CompositeItem::Key(k, v) => {
                        scan_expr(k, s, calls);
                        scan_expr(v, s, calls);
                    }
                    CompositeItem::Field(_, x) | CompositeItem::Index(_, x) => {
                        scan_expr(x, s, calls);
                    }
                }
            }
        }
        ExprKind::FuncLit(_) => {}
        ExprKind::Bound { recv, .. } => scan_expr(recv, s, calls),
        ExprKind::TypeAssert { x, .. } => scan_expr(x, s, calls),
        ExprKind::Make { kind, args } => {
            // `make` 分配新存储；但 `make([]T, n)` 之后对它写就是堆写，
            // 那会在赋值点被 `Owner::Heap` 捕获。这里只扫实参。
            match kind {
                MakeKind::Slice | MakeKind::Map | MakeKind::Chan => {}
            }
            for a in args {
                scan_expr(a, s, calls);
            }
        }
        ExprKind::New(_) => {}
        ExprKind::Builtin { kind, args, .. } => {
            for a in args {
                scan_expr(a, s, calls);
            }
            let b = builtin_summary(*kind);
            s.writes_all |= b.writes_all;
            s.may_panic |= b.may_panic;
        }
    }
}

fn scan_place_exprs(p: &Place, s: &mut Summary, calls: &mut Vec<FuncId>) {
    match p {
        Place::Slot(_) | Place::Global(_) => {}
        Place::Field { base, .. } => scan_place_exprs(base, s, calls),
        Place::Index { base, index } => {
            scan_expr(base, s, calls);
            scan_expr(index, s, calls);
        }
        Place::Deref(x) => scan_expr(x, s, calls),
    }
}

fn owner_of_place(p: &Place) -> Owner {
    match p {
        Place::Slot(_) => Owner::Local,
        Place::Global(g) => Owner::Global(*g),
        Place::Field { base, .. } => owner_of_place(base),
        Place::Index { base, .. } => owner_of_expr(base),
        Place::Deref(_) => Owner::Heap,
    }
}

fn owner_of_expr(e: &Expr) -> Owner {
    match &e.kind {
        ExprKind::Slot(_) => Owner::Local,
        ExprKind::Global(g) => Owner::Global(*g),
        ExprKind::Field { base, .. } => owner_of_expr(base),
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
    }
}
