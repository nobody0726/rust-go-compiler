//! 包级初始化依赖图、顺序与环（M4/Task 32–33；`T-CK-71` / `T-CK-72`）。
//!
//! # 判据来源
//!
//! 官方 `go/types/initorder.go`（`dependencyGraph` + `nodeQueue` + `findPath`）
//! 与 `go/types/check.go:174` 的 `addDeclDep`。本模块同时产出两件东西：
//!
//! * **顺序** —— `types.Info.InitOrder` 的对应物（Task 32）；
//! * **环诊断** —— 官方 `InvalidInitCycle`，C8 类别 `cycle`（Task 33）。
//!
//! # 图的口径（与官方逐条对齐）
//!
//! * **节点**：包级 `const` / `var` / `func` / **方法**。方法按
//!   `接收者基类型.方法名` 标识（方法名不在包作用域，裸名字会撞车）。
//! * **边** `from → to`：`from` 的初始化式（或函数体）**引用**了 `to`。
//!   引用的三条来路：① 标识符；② 选择器（方法表达式 `T.m` / 方法值
//!   `x.m`）；③ 函数体（`from` 是函数时）。
//! * **函数节点消去**：`dependencyGraph` 把 `*Func` 节点从图里摘掉，把每个
//!   前驱直接接到每个后继。不摘的话，互递归函数会把 `ndeps` 灌爆
//!   （官方注释：`Mutually recursive functions ... may incorrectly inflate`）。
//! * **出队**：`const` 优先 → `ndeps` 最小 → 声明序最小。`ndeps > 0` 时若该
//!   节点**在环上**就报一条；每个环只报一次（环上第一个被弹出的对象），
//!   报完照常把它的前驱 `ndeps` 减一（官方 `n.pred` 那一圈）。
//!
//! # 为什么方法必须进图（Task 33 的实测）
//!
//! `check/init2.go` 的 26 个环里，除前两个之外**全部**经过方法：
//! `var x3 = T3.m`（方法表达式）、`var x7 = T7(0).m`（方法值）、
//! `var x5 = T5.m`（嵌入提升）、`var x7v = x7var.m`（变量接收者）。
//! 只认名字的图一条边都补不上 ⇒ 26 行全漏。⇒ 选择器必须做**方法集解析**
//! （`resolve_method`：本类型 → 嵌入字段的传递闭包）。
//!
//! # 窄面（`Ruling 32-2` / `Ruling 33-2`，台账登记）
//!
//! * 标识符只认**名字**，不做作用域消解 —— 函数体里的同名局部会**多**算依赖
//!   （多算只让顺序更保守，不会漏依赖）。
//! * 接收者类型只从**语法**上认（声明类型 / 复合字面量 / `new(T)` / 转换
//!   `T(...)` / 变量名链），不求值。认不出 ⇒ 补不上边（**可能漏**，但漏的
//!   方向是「少报环」，不会误报）。
//! * 方法解析**忽略值/指针接收者的差别**（提升方法一律算在方法集里）。
//!   严格的方法集规则会把 `(*T5p).m` 漏掉（`T5p` 的值方法集不含 `*E5p` 的
//!   方法），而多认一条边只会让环更早被发现。
//! * 接口类型的选择器**不补边**（动态派发，官方同样不补 ——
//!   `check/init1.go:27` 的 `x1.m()` 是钉子）。
//! * 类型表达式里的名字（`[N]T` 的 `N`）不收集 —— 那里只可能是常量。

use std::collections::{BTreeMap, BTreeSet};

use rgoc_ast::{BlockStmt, Decl, Expr, FuncDecl, Op, SimpleStmt, Stmt, VarDecl};
use rgoc_lex::Pos;
use rgoc_load::Package;

/// 一个初始化器（`types.Initializer` 的对应物）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Initializer {
    /// `Lhs` 的名字（`var a, b = f()` ⇒ `["a", "b"]`）。
    pub names: Vec<String>,
    /// 依赖的**包级对象**下标（本模块内部编号；仅供调试/断言）。
    pub deps: Vec<usize>,
}

/// 初始化顺序（`order` 即官方 `InitOrder` 的顺序）。
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

/// 一条初始化环诊断（官方 `InvalidInitCycle`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cycle {
    /// 报点：环上第一个被弹出的对象（变量的**名字**位置）。
    pub pos: Pos,
    /// 消息：主行 + `\n\t` 续行链（渲染层按 `": \t"` 过滤，只留主行）。
    pub message: String,
}

/// 顺序 + 环。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Analysis {
    pub order: Vec<Initializer>,
    pub cycles: Vec<Cycle>,
}

/// 算一个包的初始化顺序（Task 32 的入口）。
#[must_use]
pub fn compute(pkg: &Package) -> InitOrder {
    InitOrder {
        order: analyze(pkg).order,
    }
}

/// 算一个包的初始化顺序**与**环诊断（Task 33 的入口）。
#[must_use]
pub fn analyze(pkg: &Package) -> Analysis {
    let nodes = build(pkg);
    solve(&nodes)
}

// ────────────────────────────── 节点 ──────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Const,
    Var,
    Func,
}

#[derive(Clone, Debug)]
struct Node {
    kind: Kind,
    /// 显示名（方法带 `接收者.` 前缀）。
    name: String,
    /// 名字位置（环诊断的报点）。
    pos: Pos,
    /// 声明序（官方 `Object.order()`）。
    order: usize,
    /// 依赖的节点下标（**含**函数节点，环链要用）。
    deps: BTreeSet<usize>,
    /// 初始化器组（`var a, b = f()` 的两个节点同组；无 ⇒ `usize::MAX`）。
    group: usize,
    /// 有初始化式（`var`）/ 有函数体（`func`）。官方 `hasInitializer()`。
    has_init: bool,
}

/// 接收者/表达式的**语法类型**（够用来解析方法集即可）。
#[derive(Clone, PartialEq, Eq, Debug)]
enum TypeRef {
    Named(String),
    Ptr(Box<TypeRef>),
    Iface,
}

/// 具名类型的形状：嵌入字段（方法提升要用）+ 是否接口。
#[derive(Clone, Debug, Default)]
struct Shape {
    embedded: Vec<TypeRef>,
    iface: bool,
}

// ────────────────────────────── 构图 ──────────────────────────────

/// 待求依赖的 AST 面。
enum Work<'a> {
    Exprs(Vec<&'a Expr>),
    Body(&'a BlockStmt),
}

struct Build<'a> {
    nodes: Vec<Node>,
    /// 包级**值对象**名（var / const / func）→ 节点下标。
    index: BTreeMap<String, usize>,
    type_names: BTreeSet<String>,
    shapes: BTreeMap<String, Shape>,
    /// `(接收者基类型, 方法名)` → 方法声明。
    methods: BTreeMap<(String, String), &'a FuncDecl>,
    /// `var` 名 → 声明类型表达式 / 初始化式（解析接收者类型用）。
    var_decl_ty: BTreeMap<String, &'a Expr>,
    var_init: BTreeMap<String, &'a Expr>,
    work: Vec<(usize, Work<'a>)>,
    next_group: usize,
}

fn build(pkg: &Package) -> Vec<Node> {
    let mut b = Build {
        nodes: Vec::new(),
        index: BTreeMap::new(),
        type_names: BTreeSet::new(),
        shapes: BTreeMap::new(),
        methods: BTreeMap::new(),
        var_decl_ty: BTreeMap::new(),
        var_init: BTreeMap::new(),
        work: Vec::new(),
        next_group: 0,
    };

    // ① 类型名与形状（先全扫一遍：类型可以后声明）
    for ast in &pkg.asts {
        for decl in &ast.decls {
            if let Decl::Type(t) = decl {
                b.type_names.insert(t.name.value.clone());
                b.shapes.insert(t.name.value.clone(), shape_of(&t.type_));
            }
        }
    }

    // ② 对象与节点（文件序 → 声明序）
    let mut order = 0usize;
    for ast in &pkg.asts {
        for decl in &ast.decls {
            match decl {
                Decl::Var(v) => b.var_decl(v, &mut order),
                Decl::Const(c) => b.const_decl(c, &mut order),
                Decl::Func(f) => b.func_decl(f, &mut order),
                _ => {}
            }
        }
    }

    // ③ 依赖（图建完才能解析 —— 前向引用要能命中）
    let work = std::mem::take(&mut b.work);
    for (i, w) in work {
        let mut deps = BTreeSet::new();
        match w {
            Work::Exprs(es) => {
                for e in es {
                    b.walk_expr(e, &mut deps);
                }
            }
            Work::Body(body) => b.walk_block(body, &mut deps),
        }
        b.nodes[i].deps = deps;
    }

    b.nodes
}

impl<'a> Build<'a> {
    fn push_node(
        &mut self,
        kind: Kind,
        name: &str,
        pos: Pos,
        order: &mut usize,
        group: usize,
        has_init: bool,
    ) -> usize {
        let i = self.nodes.len();
        self.nodes.push(Node {
            kind,
            name: name.to_owned(),
            pos,
            order: *order,
            deps: BTreeSet::new(),
            group,
            has_init,
        });
        *order += 1;
        // 先声明先赢（重名由装载面报）
        self.index.entry(name.to_owned()).or_insert(i);
        i
    }

    fn var_decl(&mut self, v: &'a VarDecl, order: &mut usize) {
        // 类型/初始化式登记（与名字下标对齐）
        for (k, n) in v.names.iter().enumerate() {
            if n.value == "_" {
                continue;
            }
            if let Some(te) = &v.type_ {
                self.var_decl_ty.insert(n.value.clone(), te);
            }
            let val = if v.values.len() == 1 {
                v.values.first()
            } else {
                v.values.get(k)
            };
            if let Some(e) = val {
                self.var_init.insert(n.value.clone(), e);
            }
        }

        let live: Vec<&rgoc_ast::Name> = v.names.iter().filter(|n| n.value != "_").collect();
        // `var a, b = f()`（单值多名）⇒ **一个**初始化器组；
        // 其余（逐名配对 / 无值）⇒ 逐名成组（`Ruling 32-1` 第 1 条）。
        if v.values.len() == 1 && live.len() > 1 {
            let g = self.next_group;
            self.next_group += 1;
            for n in &live {
                let i = self.push_node(Kind::Var, &n.value, n.pos, order, g, true);
                self.work.push((i, Work::Exprs(vec![&v.values[0]])));
            }
        } else {
            for (k, n) in v.names.iter().enumerate() {
                if n.value == "_" {
                    continue;
                }
                let g = self.next_group;
                self.next_group += 1;
                let val = v.values.get(k);
                let i = self.push_node(Kind::Var, &n.value, n.pos, order, g, val.is_some());
                if let Some(e) = val {
                    self.work.push((i, Work::Exprs(vec![e])));
                }
            }
        }
    }

    fn const_decl(&mut self, c: &'a rgoc_ast::ConstDecl, order: &mut usize) {
        let live: Vec<&rgoc_ast::Name> = c.names.iter().filter(|n| n.value != "_").collect();
        let single = c.values.len() == 1 && live.len() > 1;
        if single {
            let g = self.next_group;
            self.next_group += 1;
            for n in &live {
                let i = self.push_node(Kind::Const, &n.value, n.pos, order, g, true);
                self.work.push((i, Work::Exprs(vec![&c.values[0]])));
            }
        } else {
            for (k, n) in c.names.iter().enumerate() {
                if n.value == "_" {
                    continue;
                }
                let g = self.next_group;
                self.next_group += 1;
                let val = c.values.get(k);
                let i = self.push_node(Kind::Const, &n.value, n.pos, order, g, val.is_some());
                if let Some(e) = val {
                    self.work.push((i, Work::Exprs(vec![e])));
                }
            }
        }
    }

    fn func_decl(&mut self, f: &'a FuncDecl, order: &mut usize) {
        match &f.recv {
            Some(recv) => {
                let Some(base) = recv_base_name(&recv.type_) else {
                    return;
                };
                let key = format!("{base}.{}", f.name.value);
                self.methods
                    .insert((base.to_owned(), f.name.value.clone()), f);
                let i = self.push_node(
                    Kind::Func,
                    &key,
                    f.name.pos,
                    order,
                    usize::MAX,
                    f.body.is_some(),
                );
                if let Some(body) = &f.body {
                    self.work.push((i, Work::Body(body)));
                }
            }
            None => {
                let i = self.push_node(
                    Kind::Func,
                    &f.name.value,
                    f.name.pos,
                    order,
                    usize::MAX,
                    f.body.is_some(),
                );
                if let Some(body) = &f.body {
                    self.work.push((i, Work::Body(body)));
                }
            }
        }
    }

    // ────────────────────── 引用 → 边 ──────────────────────

    fn add_name(&self, name: &str, out: &mut BTreeSet<usize>) {
        if let Some(&i) = self.index.get(name) {
            out.insert(i);
        }
    }

    /// 选择器 `x.sel` 的方法面：`x` 的语法类型 → 方法集 → 节点。
    fn add_method_dep(&self, x: &Expr, sel: &str, out: &mut BTreeSet<usize>) {
        let Some(tr) = self.type_of_value(x, 0) else {
            return;
        };
        // 接口接收者 ⇒ 动态派发，没有静态依赖（`check/init1.go:27`）。
        if tr == TypeRef::Iface {
            return;
        }
        if let Some(key) = self.resolve_method(&tr, sel)
            && let Some(&i) = self.index.get(&key)
        {
            out.insert(i);
        }
    }

    /// 表达式的**语法类型**（够解析方法集）。
    fn type_of_value(&self, e: &Expr, depth: u32) -> Option<TypeRef> {
        if depth > 8 {
            return None;
        }
        match e {
            Expr::Name(n) => {
                if self.type_names.contains(&n.value) {
                    return Some(TypeRef::Named(n.value.clone()));
                }
                self.var_type(&n.value, depth + 1)
            }
            Expr::Paren(p) => self.type_of_value(&p.x, depth + 1),
            // `*p`
            Expr::Unary(u) if u.op == Op::Mul => {
                Some(TypeRef::Ptr(Box::new(self.type_of_value(&u.x, depth + 1)?)))
            }
            // `&T{}`
            Expr::Unary(u) if u.op == Op::And => {
                Some(TypeRef::Ptr(Box::new(self.type_of_value(&u.x, depth + 1)?)))
            }
            Expr::CompositeLit(c) => type_ref_of_type_expr(c.type_.as_deref()?),
            Expr::Index(ix) => {
                if let Expr::Name(n) = &*ix.x
                    && self.type_names.contains(&n.value)
                {
                    return Some(TypeRef::Named(n.value.clone()));
                }
                None
            }
            Expr::Call(c) => {
                let Expr::Name(f) = &*c.fun else { return None };
                if f.value == "new" {
                    let t = type_ref_of_type_expr(c.args.first()?)?;
                    return Some(TypeRef::Ptr(Box::new(t)));
                }
                // 转换 `T(x)`（`T` 必须是本包类型名）
                if self.type_names.contains(&f.value) {
                    return Some(TypeRef::Named(f.value.clone()));
                }
                None
            }
            _ => None,
        }
    }

    /// 变量的类型：声明类型优先，否则看初始化式。
    fn var_type(&self, name: &str, depth: u32) -> Option<TypeRef> {
        if depth > 8 {
            return None;
        }
        if let Some(te) = self.var_decl_ty.get(name) {
            return type_ref_of_type_expr(te);
        }
        let e = self.var_init.get(name)?;
        self.type_of_value(e, depth + 1)
    }

    /// 方法集解析：本类型 → 嵌入字段的传递闭包。
    fn resolve_method(&self, tr: &TypeRef, name: &str) -> Option<String> {
        let base = match tr {
            TypeRef::Named(t) => t,
            TypeRef::Ptr(inner) => match &**inner {
                TypeRef::Named(t) => t,
                _ => return None,
            },
            TypeRef::Iface => return None,
        };
        self.resolve_method_on(base, name, 0)
    }

    fn resolve_method_on(&self, ty: &str, name: &str, depth: u32) -> Option<String> {
        if depth > 8 {
            return None;
        }
        if self.methods.contains_key(&(ty.to_owned(), name.to_owned())) {
            return Some(format!("{ty}.{name}"));
        }
        let shape = self.shapes.get(ty)?;
        if shape.iface {
            return None;
        }
        for f in &shape.embedded {
            if let Some(k) = self.resolve_method(f, name) {
                return Some(k);
            }
        }
        None
    }

    // ────────────────────── 走表达式 / 语句 ──────────────────────

    fn walk_expr(&self, e: &Expr, out: &mut BTreeSet<usize>) {
        match e {
            Expr::Bad(_) | Expr::BasicLit(_) => {}
            Expr::Name(n) => self.add_name(&n.value, out),
            Expr::CompositeLit(c) => {
                if let Some(t) = &c.type_ {
                    self.walk_expr(t, out);
                }
                for el in &c.elems {
                    self.walk_expr(el, out);
                }
            }
            Expr::KeyValue(kv) => {
                self.walk_expr(&kv.key, out);
                self.walk_expr(&kv.value, out);
            }
            Expr::FuncLit(fl) => {
                self.walk_expr(&fl.type_, out);
                self.walk_block(&fl.body, out);
            }
            Expr::Paren(p) => self.walk_expr(&p.x, out),
            // 选择器的 `sel` 是字段/方法名 —— 不是名字引用；方法面单算。
            Expr::Selector(s) => {
                self.walk_expr(&s.x, out);
                self.add_method_dep(&s.x, &s.sel.value, out);
            }
            Expr::Index(ix) => {
                self.walk_expr(&ix.x, out);
                self.walk_expr(&ix.index, out);
            }
            Expr::Slice(s) => {
                self.walk_expr(&s.x, out);
                for o in [&s.low, &s.high, &s.max].into_iter().flatten() {
                    self.walk_expr(o, out);
                }
            }
            Expr::TypeAssert(a) => {
                self.walk_expr(&a.x, out);
                if let Some(t) = &a.type_ {
                    self.walk_expr(t, out);
                }
            }
            Expr::TypeSwitchGuard(g) => self.walk_expr(&g.x, out),
            Expr::Unary(op) | Expr::Binary(op) => {
                self.walk_expr(&op.x, out);
                if let Some(y) = &op.y {
                    self.walk_expr(y, out);
                }
            }
            Expr::Call(c) => {
                self.walk_expr(&c.fun, out);
                for a in &c.args {
                    self.walk_expr(a, out);
                }
            }
            Expr::List(l) => {
                for el in &l.elems {
                    self.walk_expr(el, out);
                }
            }
            // 类型族：只下钻可能含表达式的位（数组长度 / 元素 / 键值 / 字段）。
            Expr::ArrayType(a) => {
                if let Some(l) = &a.len {
                    self.walk_expr(l, out);
                }
                self.walk_expr(&a.elem, out);
            }
            Expr::SliceType(s) => self.walk_expr(&s.elem, out),
            Expr::DotsType(d) => self.walk_expr(&d.elem, out),
            Expr::StructType(s) => {
                for fd in &s.fields {
                    self.walk_expr(&fd.type_, out);
                }
            }
            Expr::InterfaceType(i) => {
                for fd in &i.methods {
                    self.walk_expr(&fd.type_, out);
                }
            }
            Expr::FuncType(ft) => {
                for fd in ft.params.iter().chain(ft.results.iter()) {
                    self.walk_expr(&fd.type_, out);
                }
            }
            Expr::MapType(m) => {
                self.walk_expr(&m.key, out);
                self.walk_expr(&m.value, out);
            }
            Expr::ChanType(c) => self.walk_expr(&c.elem, out),
        }
    }

    fn walk_block(&self, b: &BlockStmt, out: &mut BTreeSet<usize>) {
        for s in &b.stmts {
            self.walk_stmt(s, out);
        }
    }

    fn walk_stmt(&self, s: &Stmt, out: &mut BTreeSet<usize>) {
        match s {
            Stmt::Empty(_) | Stmt::Branch(_) => {}
            Stmt::Labeled(l) => self.walk_stmt(&l.stmt, out),
            Stmt::Block(b) => self.walk_block(b, out),
            Stmt::Expr(e) => self.walk_expr(&e.x, out),
            Stmt::Send(s) => {
                self.walk_expr(&s.chan, out);
                self.walk_expr(&s.value, out);
            }
            Stmt::Decl(d) => {
                for decl in &d.decls {
                    match decl {
                        Decl::Var(v) => {
                            for e in &v.values {
                                self.walk_expr(e, out);
                            }
                        }
                        Decl::Const(c) => {
                            for e in &c.values {
                                self.walk_expr(e, out);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Stmt::Assign(a) => {
                for e in a.lhs.iter().chain(a.rhs.iter()) {
                    self.walk_expr(e, out);
                }
            }
            Stmt::Call(c) => self.walk_expr(&c.call, out),
            Stmt::Return(r) => {
                for e in &r.results {
                    self.walk_expr(e, out);
                }
            }
            Stmt::If(i) => {
                if let Some(init) = &i.init {
                    self.walk_simple(init, out);
                }
                if let Some(c) = &i.cond {
                    self.walk_expr(c, out);
                }
                self.walk_block(&i.then, out);
                if let Some(e) = &i.els {
                    self.walk_stmt(e, out);
                }
            }
            Stmt::For(fo) => {
                if let Some(init) = &fo.init {
                    self.walk_simple(init, out);
                }
                if let Some(c) = &fo.cond {
                    self.walk_expr(c, out);
                }
                if let Some(post) = &fo.post {
                    self.walk_simple(post, out);
                }
                self.walk_block(&fo.body, out);
            }
            Stmt::Switch(sw) => {
                if let Some(init) = &sw.init {
                    self.walk_simple(init, out);
                }
                if let Some(t) = &sw.tag {
                    self.walk_expr(t, out);
                }
                for c in &sw.cases {
                    if let Some(e) = &c.cases {
                        self.walk_expr(e, out);
                    }
                    for st in &c.body {
                        self.walk_stmt(st, out);
                    }
                }
            }
            Stmt::Select(sel) => {
                for c in &sel.clauses {
                    if let Some(comm) = &c.comm {
                        self.walk_simple(comm, out);
                    }
                    for st in &c.body {
                        self.walk_stmt(st, out);
                    }
                }
            }
        }
    }

    fn walk_simple(&self, s: &SimpleStmt, out: &mut BTreeSet<usize>) {
        match s {
            SimpleStmt::Empty => {}
            SimpleStmt::Expr(e) => self.walk_expr(&e.x, out),
            SimpleStmt::Send(s) => {
                self.walk_expr(&s.chan, out);
                self.walk_expr(&s.value, out);
            }
            SimpleStmt::Assign(a) => {
                for e in a.lhs.iter().chain(a.rhs.iter()) {
                    self.walk_expr(e, out);
                }
            }
            SimpleStmt::Range(r) => {
                for e in &r.lhs {
                    self.walk_expr(e, out);
                }
                self.walk_expr(&r.x, out);
            }
        }
    }
}

// ────────────────────── 语法类型 / 形状 ──────────────────────

/// 类型表达式 → [`TypeRef`]（只看语法，不求值）。
fn type_ref_of_type_expr(e: &Expr) -> Option<TypeRef> {
    match e {
        Expr::Name(n) => Some(TypeRef::Named(n.value.clone())),
        Expr::Paren(p) => type_ref_of_type_expr(&p.x),
        Expr::Unary(u) if u.op == Op::Mul => {
            Some(TypeRef::Ptr(Box::new(type_ref_of_type_expr(&u.x)?)))
        }
        Expr::InterfaceType(_) => Some(TypeRef::Iface),
        Expr::Index(i) => type_ref_of_type_expr(&i.x),
        _ => None,
    }
}

/// 类型声明本体的形状。
fn shape_of(e: &Expr) -> Shape {
    match e {
        Expr::Paren(p) => shape_of(&p.x),
        Expr::StructType(st) => Shape {
            // 嵌入字段 = 没有显式名字的字段（`struct { E5 }`）。
            embedded: st
                .fields
                .iter()
                .filter(|f| f.name.is_none())
                .filter_map(|f| type_ref_of_type_expr(&f.type_))
                .collect(),
            iface: false,
        },
        Expr::InterfaceType(_) => Shape {
            embedded: Vec::new(),
            iface: true,
        },
        _ => Shape::default(),
    }
}

/// 接收者类型表达式的**基类型名**（`T` / `*T` / `T[P]` / `(T)`）。
fn recv_base_name(e: &Expr) -> Option<&str> {
    match e {
        Expr::Name(n) => Some(&n.value),
        Expr::Paren(p) => recv_base_name(&p.x),
        Expr::Unary(u) if u.op == Op::Mul => recv_base_name(&u.x),
        Expr::Index(i) => recv_base_name(&i.x),
        _ => None,
    }
}

// ────────────────────────────── 求解 ──────────────────────────────

/// 官方 `initOrder()` 的整段：消去函数节点 → 优先队列 → 环。
fn solve(nodes: &[Node]) -> Analysis {
    let n = nodes.len();
    // ① 边表（succ / pred）
    let mut succ: Vec<BTreeSet<usize>> = nodes.iter().map(|x| x.deps.clone()).collect();
    let mut pred: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    for (i, node) in nodes.iter().enumerate() {
        for &d in &node.deps {
            pred[d].insert(i);
        }
    }

    // ② 消去函数节点（官方按 `cost = len(pred) * len(succ)` 升序处理）
    let mut funcs: Vec<usize> = (0..n).filter(|&i| nodes[i].kind == Kind::Func).collect();
    funcs.sort_by_key(|&i| (pred[i].len() * succ[i].len(), nodes[i].order));
    for f in funcs {
        let ps: Vec<usize> = pred[f].iter().copied().filter(|&p| p != f).collect();
        let ss: Vec<usize> = succ[f].iter().copied().filter(|&s| s != f).collect();
        for p in ps {
            for &s in &ss {
                succ[p].insert(s);
                pred[s].insert(p);
            }
            succ[p].remove(&f);
        }
        for s in ss {
            pred[s].remove(&f);
        }
    }

    // ③ 出队
    let mut ndeps: Vec<usize> = succ.iter().map(BTreeSet::len).collect();
    let mut remaining: BTreeSet<usize> = (0..n).filter(|&i| nodes[i].kind != Kind::Func).collect();
    let mut emitted: BTreeSet<usize> = BTreeSet::new();
    let mut order: Vec<Initializer> = Vec::new();
    let mut cycles: Vec<Cycle> = Vec::new();

    while !remaining.is_empty() {
        // const 优先 → ndeps 最小 → 声明序最小（官方 `nodeQueue.Less`）
        let pick = *remaining
            .iter()
            .min_by_key(|&&i| (nodes[i].kind != Kind::Const, ndeps[i], nodes[i].order))
            .expect("remaining 非空");
        remaining.remove(&pick);

        if ndeps[pick] > 0
            && let Some(cycle) = find_path(nodes, pick, pick, &mut BTreeSet::new())
        {
            // 环里全是常量 ⇒ 装载面的常量求值已经报过（`hit_cycle`），
            // 这里只补**变量**面，否则同一行会出两条（C8 保留重复行）。
            if nodes[pick].kind == Kind::Var {
                cycles.push(report_cycle(nodes, &cycle));
            }
        }

        for p in pred[pick].clone() {
            ndeps[p] = ndeps[p].saturating_sub(1);
        }

        if nodes[pick].kind == Kind::Var
            && nodes[pick].has_init
            && emitted.insert(nodes[pick].group)
        {
            // 组的名字表按声明序（`var a, b = f()` ⇒ `["a", "b"]`）
            let names = group_names(nodes, pick);
            let deps: Vec<usize> = nodes[pick].deps.iter().copied().collect();
            order.push(Initializer { names, deps });
        }
    }

    Analysis { order, cycles }
}

/// 组内名字表（按声明序）。
fn group_names(nodes: &[Node], i: usize) -> Vec<String> {
    let g = nodes[i].group;
    let mut members: Vec<&Node> = nodes
        .iter()
        .filter(|n| n.kind == Kind::Var && n.group == g)
        .collect();
    members.sort_by_key(|n| n.order);
    members.into_iter().map(|n| n.name.clone()).collect()
}

/// 官方 `findPath`：从 `from` 沿依赖找到回到 `to` 的路径。
///
/// 返回 `[to, ..., from]`（**反序**），与官方同形 —— [`report_cycle`] 依赖这个
/// 顺序把消息链写对。`seen` 在整个搜索里共用（官方同一个 map）。
fn find_path(
    nodes: &[Node],
    from: usize,
    to: usize,
    seen: &mut BTreeSet<usize>,
) -> Option<Vec<usize>> {
    if !seen.insert(from) {
        return None;
    }
    // 依赖按声明序走（官方 `sort.Slice(deps, order)`）—— 决定环链的写法
    let mut deps: Vec<usize> = nodes[from].deps.iter().copied().collect();
    deps.sort_by_key(|&d| nodes[d].order);
    for d in deps {
        if d == to {
            return Some(vec![d]);
        }
        if let Some(mut p) = find_path(nodes, d, to, seen) {
            p.push(d);
            return Some(p);
        }
    }
    None
}

/// 官方 `reportCycle` 的消息形状。
fn report_cycle(nodes: &[Node], cycle: &[usize]) -> Cycle {
    let first = cycle[0];
    let name = &nodes[first].name;
    if cycle.len() == 1 {
        return Cycle {
            pos: nodes[first].pos,
            message: format!("initialization cycle: {name} refers to itself"),
        };
    }
    let mut msg = format!("initialization cycle for {name}");
    let mut obj = first;
    for j in (0..cycle.len()).rev() {
        let next = cycle[j];
        msg.push_str(&format!(
            "\n\t{} refers to {}",
            nodes[obj].name, nodes[next].name
        ));
        obj = next;
    }
    Cycle {
        pos: nodes[first].pos,
        message: msg,
    }
}
