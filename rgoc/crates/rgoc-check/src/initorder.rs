//! 包级初始化依赖图与顺序（M4/Task 32；`T-CK-71`）。
//!
//! # 判据来源
//!
//! 官方 `go/types/initorder.go`。本模块只做**顺序**（`types.Info.InitOrder`
//! 的对应物）；**环**的诊断归 Task 33（`T-CK-72`）。
//!
//! # 四条实测语义（`Ruling 32-1`，`probe/t32-*.go` 的 oracle 输出钉死）
//!
//! 1. **渲染面**：每行一个初始化器，取 `Lhs` 的名字列表。`var i, j = 1, 2`
//!    被官方拆成**两个**初始化器（`i`、`j` 各一行）；`var a, b = f()` 才是
//!    一个初始化器带两个 `Lhs`。
//! 2. **函数字面量体计入依赖**：`var p1 = func() int { return q1 }` ⇒ `q1`
//!    排在 `p1` 之前（即使 `q1` 声明在后）。
//! 3. **被调用函数的函数体也计入依赖**：`var p2 = f2()` 且 `f2` 体里用 `q2`
//!    ⇒ `q2` 排在 `p2` 之前。⇒ 依赖图要走**调用图的传递闭包**。
//! 4. **裸函数体引用不计入**：`func m() int { return g }` 不构成任何依赖
//!    （没人从初始化式里调 `m`）。
//!
//! # 算法
//!
//! ① 按「文件序 → 声明序」收集包级 `var` 初始化器与 `func` 体；
//! ② 每个初始化器的**直接**依赖 = 初始化式里出现的包级 `var` 名；
//! ③ 每个 `func` 的依赖 = 体内出现的包级 `var` 名 ∪ 体内**被调函数**的依赖
//!    （传递闭包，`visiting` 防环）；
//! ④ 初始化器依赖 = ② ∪ ③（对初始化式里出现的函数名）；
//! ⑤ 按声明序 DFS：先递归依赖，再吐自己 ⇒ 拓扑序。
//!
//! # 窄面（`Ruling 32-2`，台账登记）
//!
//! * 依赖只认**名字**，不做作用域消解 —— 局部变量同名会**多**算依赖
//!   （多算只会让顺序更保守，不会漏依赖；初始化式里本就没有局部变量，
//!   函数体里的同名局部是唯一的来路）；
//! * 方法（`recv != None`）不进调用图（初始化式只能调包级函数）；
//! * 类型表达式里的名字（`[N]T` 的 `N`）不收集 —— 那里只可能是常量。

use std::collections::{BTreeMap, BTreeSet};

use rgoc_ast::{Decl, Expr, SimpleStmt, Stmt};
use rgoc_load::Package;

/// 一个初始化器（`types.Initializer` 的对应物）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Initializer {
    /// `Lhs` 的名字（`var a, b = f()` ⇒ `["a", "b"]`）。
    pub names: Vec<String>,
    /// 依赖的初始化器下标（去重、升序）。
    pub deps: Vec<usize>,
}

/// 初始化顺序（`groups` 即官方 `InitOrder` 的顺序）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InitOrder {
    /// 按初始化顺序排列的初始化器。
    pub order: Vec<Initializer>,
}

impl InitOrder {
    /// 渲染：每行一个初始化器的名字列表（`Ruling 32-1` 第 1 条）。
    #[must_use]
    pub fn render(&self) -> Vec<String> {
        self.order.iter().map(|i| i.names.join(",")).collect()
    }
}

/// 算一个包的初始化顺序。
#[must_use]
pub fn compute(pkg: &Package) -> InitOrder {
    // ① 收集（文件序 → 声明序）
    let mut inits: Vec<Initializer> = Vec::new();
    // 名字 → 初始化器下标（**先声明先赢**；重复声明由装载面报）
    let mut var_slot: BTreeMap<String, usize> = BTreeMap::new();
    // 初始化器的值表达式（与 `inits` 同下标；`None` ⇒ 无值，官方不产初始化器）
    let mut exprs: Vec<Vec<&Expr>> = Vec::new();
    // 包级函数体
    let mut funcs: BTreeMap<String, &rgoc_ast::BlockStmt> = BTreeMap::new();

    for ast in &pkg.asts {
        for decl in &ast.decls {
            match decl {
                Decl::Var(v) => {
                    // `var a, b = f()`（单值多名）⇒ 一个初始化器；
                    // 其余（逐名配对 / 无值）⇒ 逐名拆开（`Ruling 32-1` 第 1 条）。
                    if v.values.len() == 1 && v.names.len() > 1 {
                        let idx = inits.len();
                        let names: Vec<String> = v
                            .names
                            .iter()
                            .filter(|n| n.value != "_")
                            .map(|n| n.value.clone())
                            .collect();
                        if names.is_empty() {
                            continue;
                        }
                        inits.push(Initializer {
                            names: names.clone(),
                            deps: Vec::new(),
                        });
                        exprs.push(vec![&v.values[0]]);
                        for n in names {
                            var_slot.entry(n).or_insert(idx);
                        }
                    } else {
                        for (k, name) in v.names.iter().enumerate() {
                            let Some(value) = v.values.get(k) else {
                                // 无值：官方不为它产初始化器（零值不参与排序）
                                continue;
                            };
                            if name.value == "_" {
                                continue;
                            }
                            let idx = inits.len();
                            inits.push(Initializer {
                                names: vec![name.value.clone()],
                                deps: Vec::new(),
                            });
                            exprs.push(vec![value]);
                            var_slot.entry(name.value.clone()).or_insert(idx);
                        }
                    }
                }
                Decl::Func(f) if f.recv.is_none() => {
                    if let Some(body) = &f.body {
                        funcs.entry(f.name.value.clone()).or_insert(body);
                    }
                }
                _ => {}
            }
        }
    }

    // ②③ 依赖
    let func_deps = FuncDeps {
        funcs: &funcs,
        var_slot: &var_slot,
    };
    let mut memo: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for i in 0..inits.len() {
        let mut names: BTreeSet<String> = BTreeSet::new();
        let mut called: BTreeSet<String> = BTreeSet::new();
        for e in &exprs[i] {
            collect_expr(e, &mut |n| {
                names.insert(n.to_string());
            });
            collect_calls(e, &mut |n| {
                called.insert(n.to_string());
            });
        }
        let mut deps: BTreeSet<usize> = BTreeSet::new();
        for n in &names {
            if let Some(&j) = var_slot.get(n)
                && j != i
            {
                deps.insert(j);
            }
        }
        for f in &called {
            for n in func_deps.of(f, &mut memo, &mut BTreeSet::new()) {
                if let Some(&j) = var_slot.get(&n)
                    && j != i
                {
                    deps.insert(j);
                }
            }
        }
        inits[i].deps = deps.into_iter().collect();
    }

    // ⑤ 拓扑序（声明序 DFS）
    let mut order: Vec<Initializer> = Vec::with_capacity(inits.len());
    let mut done = vec![false; inits.len()];
    for i in 0..inits.len() {
        visit(i, &inits, &mut done, &mut order);
    }
    InitOrder { order }
}

/// 声明序 DFS：先依赖、后自己。环上的成员按「谁先被访问谁先出」兜底
/// （真环的诊断归 Task 33，这里只保证**不死循环**）。
fn visit(i: usize, inits: &[Initializer], done: &mut [bool], order: &mut Vec<Initializer>) {
    if done[i] {
        return;
    }
    done[i] = true;
    for &d in &inits[i].deps {
        visit(d, inits, done, order);
    }
    order.push(inits[i].clone());
}

/// 包级函数的依赖闭包（调用图的传递闭包）。
struct FuncDeps<'a> {
    funcs: &'a BTreeMap<String, &'a rgoc_ast::BlockStmt>,
    var_slot: &'a BTreeMap<String, usize>,
}

impl FuncDeps<'_> {
    fn of(
        &self,
        name: &str,
        memo: &mut BTreeMap<String, BTreeSet<String>>,
        visiting: &mut BTreeSet<String>,
    ) -> BTreeSet<String> {
        if let Some(hit) = memo.get(name) {
            return hit.clone();
        }
        // 环：官方在这里报初始化环（Task 33）；本层先返回空集防死循环。
        if !visiting.insert(name.to_string()) {
            return BTreeSet::new();
        }
        let mut out: BTreeSet<String> = BTreeSet::new();
        if let Some(body) = self.funcs.get(name) {
            collect_block(body, &mut |n| {
                out.insert(n.to_string());
            });
            let mut called: BTreeSet<String> = BTreeSet::new();
            collect_calls_block(body, &mut |n| {
                called.insert(n.to_string());
            });
            for c in &called {
                if c != name {
                    out.extend(self.of(c, memo, visiting));
                }
            }
        }
        visiting.remove(name);
        // 只留**包级变量**名（其余（类型名 / 常量名 / 局部名）对顺序无意义）
        out.retain(|n| self.var_slot.contains_key(n));
        memo.insert(name.to_string(), out.clone());
        out
    }
}

// ────────────────────────── 名字收集（表达式面）──────────────────────────

/// 收集表达式里出现的**标识符**（含 `FuncLit` 体；不含选择器的字段名）。
fn collect_expr(e: &Expr, f: &mut impl FnMut(&str)) {
    match e {
        Expr::Bad(_) => {}
        Expr::Name(n) => f(&n.value),
        Expr::BasicLit(_) => {}
        Expr::CompositeLit(c) => {
            if let Some(t) = &c.type_ {
                collect_expr(t, f);
            }
            for el in &c.elems {
                collect_expr(el, f);
            }
        }
        Expr::KeyValue(kv) => {
            collect_expr(&kv.key, f);
            collect_expr(&kv.value, f);
        }
        Expr::FuncLit(fl) => {
            collect_expr(&fl.type_, f);
            collect_block(&fl.body, f);
        }
        Expr::Paren(p) => collect_expr(&p.x, f),
        // 选择器的 `sel` 是字段 / 方法名，**不是**名字引用。
        Expr::Selector(s) => collect_expr(&s.x, f),
        Expr::Index(ix) => {
            collect_expr(&ix.x, f);
            collect_expr(&ix.index, f);
        }
        Expr::Slice(s) => {
            collect_expr(&s.x, f);
            for o in [&s.low, &s.high, &s.max].into_iter().flatten() {
                collect_expr(o, f);
            }
        }
        Expr::TypeAssert(a) => {
            collect_expr(&a.x, f);
            if let Some(t) = &a.type_ {
                collect_expr(t, f);
            }
        }
        Expr::TypeSwitchGuard(g) => collect_expr(&g.x, f),
        Expr::Unary(op) | Expr::Binary(op) => {
            collect_expr(&op.x, f);
            if let Some(y) = &op.y {
                collect_expr(y, f);
            }
        }
        Expr::Call(c) => {
            collect_expr(&c.fun, f);
            for a in &c.args {
                collect_expr(a, f);
            }
        }
        Expr::List(l) => {
            for el in &l.elems {
                collect_expr(el, f);
            }
        }
        // 类型族：只下钻可能含表达式的位（数组长度、结构体字段类型、
        // 接口项集、函数签名、map/chan 元素）。
        Expr::ArrayType(a) => {
            if let Some(l) = &a.len {
                collect_expr(l, f);
            }
            collect_expr(&a.elem, f);
        }
        Expr::SliceType(s) => collect_expr(&s.elem, f),
        Expr::DotsType(d) => collect_expr(&d.elem, f),
        Expr::StructType(s) => {
            for fd in &s.fields {
                collect_expr(&fd.type_, f);
            }
        }
        Expr::InterfaceType(i) => {
            for fd in &i.methods {
                collect_expr(&fd.type_, f);
            }
        }
        Expr::FuncType(ft) => {
            for fd in ft.params.iter().chain(ft.results.iter()) {
                collect_expr(&fd.type_, f);
            }
        }
        Expr::MapType(m) => {
            collect_expr(&m.key, f);
            collect_expr(&m.value, f);
        }
        Expr::ChanType(c) => collect_expr(&c.elem, f),
    }
}

/// 收集表达式里**被调用**的函数名（`f(...)` 的 `f`；含 `FuncLit` 体）。
fn collect_calls(e: &Expr, f: &mut impl FnMut(&str)) {
    match e {
        Expr::Call(c) => {
            if let Expr::Name(n) = &*c.fun {
                f(&n.value);
            }
            collect_calls(&c.fun, f);
            for a in &c.args {
                collect_calls(a, f);
            }
        }
        Expr::FuncLit(fl) => collect_calls_block(&fl.body, f),
        Expr::Paren(p) => collect_calls(&p.x, f),
        Expr::Unary(op) | Expr::Binary(op) => {
            collect_calls(&op.x, f);
            if let Some(y) = &op.y {
                collect_calls(y, f);
            }
        }
        Expr::CompositeLit(c) => {
            if let Some(t) = &c.type_ {
                collect_calls(t, f);
            }
            for el in &c.elems {
                collect_calls(el, f);
            }
        }
        Expr::KeyValue(kv) => {
            collect_calls(&kv.key, f);
            collect_calls(&kv.value, f);
        }
        Expr::Selector(s) => collect_calls(&s.x, f),
        Expr::Index(ix) => {
            collect_calls(&ix.x, f);
            collect_calls(&ix.index, f);
        }
        Expr::Slice(s) => {
            collect_calls(&s.x, f);
            for o in [&s.low, &s.high, &s.max].into_iter().flatten() {
                collect_calls(o, f);
            }
        }
        Expr::TypeAssert(a) => collect_calls(&a.x, f),
        Expr::TypeSwitchGuard(g) => collect_calls(&g.x, f),
        Expr::List(l) => {
            for el in &l.elems {
                collect_calls(el, f);
            }
        }
        _ => {}
    }
}

// ────────────────────────── 名字收集（语句面）──────────────────────────

fn collect_block(b: &rgoc_ast::BlockStmt, f: &mut impl FnMut(&str)) {
    for s in &b.stmts {
        collect_stmt(s, f);
    }
}

fn collect_stmt(s: &Stmt, f: &mut impl FnMut(&str)) {
    match s {
        Stmt::Empty(_) => {}
        Stmt::Labeled(l) => collect_stmt(&l.stmt, f),
        Stmt::Block(b) => collect_block(b, f),
        Stmt::Expr(e) => collect_expr(&e.x, f),
        Stmt::Send(s) => {
            collect_expr(&s.chan, f);
            collect_expr(&s.value, f);
        }
        Stmt::Decl(d) => {
            for decl in &d.decls {
                collect_decl(decl, f);
            }
        }
        Stmt::Assign(a) => {
            for e in a.lhs.iter().chain(a.rhs.iter()) {
                collect_expr(e, f);
            }
        }
        Stmt::Branch(_) => {}
        Stmt::Call(c) => collect_expr(&c.call, f),
        Stmt::Return(r) => {
            for e in &r.results {
                collect_expr(e, f);
            }
        }
        Stmt::If(i) => {
            if let Some(init) = &i.init {
                collect_simple(init, f);
            }
            if let Some(c) = &i.cond {
                collect_expr(c, f);
            }
            collect_block(&i.then, f);
            if let Some(e) = &i.els {
                collect_stmt(e, f);
            }
        }
        Stmt::For(fo) => {
            if let Some(init) = &fo.init {
                collect_simple(init, f);
            }
            if let Some(c) = &fo.cond {
                collect_expr(c, f);
            }
            if let Some(post) = &fo.post {
                collect_simple(post, f);
            }
            collect_block(&fo.body, f);
        }
        Stmt::Switch(sw) => {
            if let Some(init) = &sw.init {
                collect_simple(init, f);
            }
            if let Some(t) = &sw.tag {
                collect_expr(t, f);
            }
            for c in &sw.cases {
                if let Some(e) = &c.cases {
                    collect_expr(e, f);
                }
                for st in &c.body {
                    collect_stmt(st, f);
                }
            }
        }
        Stmt::Select(sel) => {
            for c in &sel.clauses {
                if let Some(comm) = &c.comm {
                    collect_simple(comm, f);
                }
                for st in &c.body {
                    collect_stmt(st, f);
                }
            }
        }
    }
}

fn collect_simple(s: &SimpleStmt, f: &mut impl FnMut(&str)) {
    match s {
        SimpleStmt::Empty => {}
        SimpleStmt::Expr(e) => collect_expr(&e.x, f),
        SimpleStmt::Send(s) => {
            collect_expr(&s.chan, f);
            collect_expr(&s.value, f);
        }
        SimpleStmt::Assign(a) => {
            for e in a.lhs.iter().chain(a.rhs.iter()) {
                collect_expr(e, f);
            }
        }
        SimpleStmt::Range(r) => {
            for e in &r.lhs {
                collect_expr(e, f);
            }
            collect_expr(&r.x, f);
        }
    }
}

fn collect_decl(d: &Decl, f: &mut impl FnMut(&str)) {
    match d {
        Decl::Var(v) => {
            for e in &v.values {
                collect_expr(e, f);
            }
        }
        Decl::Const(c) => {
            for e in &c.values {
                collect_expr(e, f);
            }
        }
        _ => {}
    }
}

fn collect_calls_block(b: &rgoc_ast::BlockStmt, f: &mut impl FnMut(&str)) {
    for s in &b.stmts {
        collect_calls_stmt(s, f);
    }
}

fn collect_calls_stmt(s: &Stmt, f: &mut impl FnMut(&str)) {
    match s {
        Stmt::Empty(_) | Stmt::Branch(_) => {}
        Stmt::Labeled(l) => collect_calls_stmt(&l.stmt, f),
        Stmt::Block(b) => collect_calls_block(b, f),
        Stmt::Expr(e) => collect_calls(&e.x, f),
        Stmt::Send(s) => {
            collect_calls(&s.chan, f);
            collect_calls(&s.value, f);
        }
        Stmt::Decl(d) => {
            for decl in &d.decls {
                if let Decl::Var(v) = decl {
                    for e in &v.values {
                        collect_calls(e, f);
                    }
                }
            }
        }
        Stmt::Assign(a) => {
            for e in a.lhs.iter().chain(a.rhs.iter()) {
                collect_calls(e, f);
            }
        }
        Stmt::Call(c) => collect_calls(&c.call, f),
        Stmt::Return(r) => {
            for e in &r.results {
                collect_calls(e, f);
            }
        }
        Stmt::If(i) => {
            if let Some(c) = &i.cond {
                collect_calls(c, f);
            }
            collect_calls_block(&i.then, f);
            if let Some(e) = &i.els {
                collect_calls_stmt(e, f);
            }
        }
        Stmt::For(fo) => {
            if let Some(c) = &fo.cond {
                collect_calls(c, f);
            }
            collect_calls_block(&fo.body, f);
        }
        Stmt::Switch(sw) => {
            if let Some(t) = &sw.tag {
                collect_calls(t, f);
            }
            for c in &sw.cases {
                if let Some(e) = &c.cases {
                    collect_calls(e, f);
                }
                for st in &c.body {
                    collect_calls_stmt(st, f);
                }
            }
        }
        Stmt::Select(sel) => {
            for c in &sel.clauses {
                for st in &c.body {
                    collect_calls_stmt(st, f);
                }
            }
        }
    }
}
