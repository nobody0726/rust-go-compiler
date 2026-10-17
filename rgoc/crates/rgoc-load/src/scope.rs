//! 函数体 / 块的作用域建立与标识符引用解析（M3/Task 25：`SCP-02/03/04`、`PIPE-09`）。
//!
//! # 这一段在整条流水线里的位置
//!
//! `collect_decls`（Task 24）管**包级**：类型表达式、常量求值、dump 根。
//! 本文件管**体内**：`func` 体、块、`if`/`for`/`switch`/`select` 的隐式块、
//! 局部声明（`var`/`const`/`type`/`:=`/参数/接收者/类型参数/`range` 迭代变量），
//! 以及**每一次名字引用**的解析。
//!
//! 为什么必须分开：包级的顺序与环由 Task 24 的状态机负责，而体内只需要
//! 「按源码顺序走一遍、见到声明就插、见到引用就查」—— 两者共用一个 `Checker`
//! （[`crate::decls::Checker`]）但**状态必须清空**：体内出现「包级项正在求值」
//! 是 bug，不是特性。
//!
//! # 作用域的层级（逐条照官方 `go/types/stmt.go` 的 `openScope` 调用点）
//!
//! | 语法节点 | 官方注释 | 这里 |
//! |---|---|---|
//! | `{ … }` | `"block"` | `BlockStmt` ⇒ 新块层 |
//! | `if` | `"if"` | init/cond/else 链都在这一层；`then`/`else` 块再各开一层 |
//! | `for` | `"for"` | init/cond/post + body |
//! | `for … range` | `"range"` | **range 子句之后**才开（`go.dev/issue/51437`）⇒ 迭代变量住这层 |
//! | `switch` | `"switch"` | init/tag + 各 case 的**值** |
//! | `case` | `"case"` | 每个 case 子句各一层（类型 switch 的隐式变量也在这里） |
//! | `select` 的通信子句 | `"case"` | 同上 |
//! | 函数 | — | 类型参数层 ⇒ 函数层（接收者/参数/结果名）⇒ 体块层 |
//!
//! # 刻意**不**做的事（进 `M3-manifest.json` 的 `open_gaps`）
//!
//! * **结构体字面量的 `Key: value` 里那个裸名字 key** ⇒ 不当作引用
//!   （它多半是**字段名**：`T{Field: 1}`。没有类型信息分不出字段名与 map 键，
//!   而「字段名当引用」会把每一个结构体字面量都误报成 `undefined` ⇒
//!   取假阴性那一侧。`value` 部分**照样**走）；
//! * **`NoNewVar`**（`x := 1; x := 2`，官方 `softErrorf`）⇒ 不报；
//!   `RepeatedDecl`（`x, y, y := …`）**报**，因为它按官方规则是硬错误；
//! * **体内声明的类型名**的 `typ` 取 RHS 的**本体**（不做包级那种「具名实例」）：
//!   局部类型不参与 dump 根、也不进同一性验收面 ⇒ 造一个 Named 只会多一处对不上的地方。
//!
//! # 标签走**另一趟**（[`crate::labels`]，M3/Task 26）
//!
//! `LabeledStmt` 只走下去、`BranchStmt` 整块跳过 —— 因为标签**不是值位置的名字**：
//! `goto L` 的 `L` 与变量同名不冲突、且可以**前向**引用（先 `goto` 后声明）。
//! 硬塞进这一趟会同时错掉「同名不冲突」与「前向可见」两条。
//! 两趟都在**每个函数体**各跑一次（含函数字面量）⇒ 标签跨函数不可见。
//!
//! # 与 `resolve_type` 的分工（**不重复报**的关键）
//!
//! 包级声明的类型表达式已由 `collect_decls` 解析过 ⇒ 这里只**声明名字**、
//! **不重解析**（再解析一遍会让同一个 `undefined: T` 报两次）。
//! 函数字面量的参数/结果类型没有任何人解析过 ⇒ 这里全部解析。
//! 两条路都走同一个「报过就不再报」的集合 [`Checker::undefined_reported`]，
//! 因为参数表里 `x, y int` 会被前端的 `fieldDecl` **克隆成两份**（官方只写了一次）。

use std::collections::BTreeSet;

use rgoc_ast::{
    AssignOp, AssignStmt, AstFile, Decl, DeclStmt, Expr, Field as AstField, ForStmt, FuncDecl,
    FuncLit, FuncType, IfStmt, Name, RangeClause, SelectStmt, SimpleStmt, Stmt, SwitchStmt,
};
use rgoc_lex::{Diag, DiagBag, DiagCode, Pos};
use rgoc_types::{ObjKind, Object, ScopeId, ScopeKind, ScopeTable, TypeId, TypeTable};

use crate::decls::Checker;
use crate::package::Package;

/// 体内作用域与名字引用（`T-TY-53..55`）。
///
/// 前置条件：`collect_decls` 已经跑过（包级对象已求值、`Package::type_roots` 已填）。
/// 后置条件：每个函数体/块的局部名字都在作用域里；每一次**找不到**的名字引用
/// 都对应一条 [`DiagCode::TypUndeclaredName`]；同层重复声明对应
/// [`DiagCode::TypDuplicateDecl`]。
pub fn resolve_bodies(pkg: &mut Package, tt: &mut TypeTable, bag: &mut DiagBag) {
    // 与 `collect_decls` 同一个理由：`Checker` 要 `&mut` 借作用域表，
    // 而收尾还要写回 `pkg` 的字段。
    let asts = std::mem::take(&mut pkg.asts);
    let mut scopes = std::mem::take(&mut pkg.scopes);
    // `file_scopes` 与 `asts` 同序（`load_sources` 同一个循环里 push 的）
    let file_scopes: Vec<ScopeId> = pkg.file_scopes.iter().map(|(_, s)| *s).collect();

    let mut ck = Checker::for_bodies(tt, &mut scopes, bag, pkg.scope);
    ck.walk_files(&asts, &file_scopes);
    drop(ck);

    pkg.scopes = scopes;
    pkg.asts = asts;
}

/// 把一个名字声明进某个作用域 —— **先声明者胜**，重复则报
/// [`DiagCode::TypDuplicateDecl`]。
///
/// `typ == None` ⇒ 只占位（包级对象留给 `collect_decls` 回填；Task 23 的
/// [`crate::package::load_sources`] 就是这么用的）。
///
/// 消息按**作用域的层**分措辞：包级 `… redeclared in this package`、
/// 其余 `… redeclared in this block`。⚠️ 官方 `go/types` 对**两者**都写
/// “block”（实测：同文件与跨文件的包级重复都是 `x redeclared in this block`），
/// 这里跟 `SCP` 的口径走（见 ledger 的 Task 25 Ruling）；代号是同一条
/// `DuplicateDecl` —— 官方本来就是一条。
pub(crate) fn declare_name(
    scopes: &mut ScopeTable,
    scope: ScopeId,
    kind: ObjKind,
    name: &str,
    pos: Pos,
    typ: Option<TypeId>,
    bag: &mut DiagBag,
) {
    // 空标识符**不引入绑定**（Go 规范；官方 `check.declare` 的第一句就是它）
    if name == "_" {
        return;
    }
    if scopes.get(scope).lookup_local(name).is_some() {
        let where_ = match scopes.get(scope).kind() {
            ScopeKind::Package => "package",
            _ => "block",
        };
        bag.push(Diag {
            pos,
            code: DiagCode::TypDuplicateDecl,
            message: format!("{name} redeclared in this {where_}"),
        });
        return;
    }
    let base = match typ {
        Some(t) => Object::with_type(kind, name.to_owned(), t),
        None => Object::new(kind, name.to_owned()),
    };
    scopes.insert(
        scope,
        Object {
            pos: Some(pos),
            ..base
        },
    );
}

/// 这个表达式是不是**空标识符** `_`。
///
/// 只用在**容许空标识符的左值位**上（`_ = x`、`for _ = range x`）——
/// 官方在这两处是**先特判 `_`、不走 `check.expr`**，所以那里不该报
/// [`DiagCode::TypInvalidBlank`]。
fn is_blank(e: &Expr) -> bool {
    matches!(e, Expr::Name(n) if n.value == "_")
}

impl Checker<'_> {
    /// 逐文件走一遍：包级的**值**表达式 + 每个 `func` 的体。
    fn walk_files(&mut self, asts: &[AstFile], file_scopes: &[ScopeId]) {
        for (ast, &fscope) in asts.iter().zip(file_scopes) {
            for decl in &ast.decls {
                match decl {
                    // 包级 `var` 的**类型**已由 `collect_decls` 解析（`C7` §3 的根）
                    // ⇒ 这里只走值，否则同一个未定义名会被报两次。
                    Decl::Var(d) => {
                        for v in &d.values {
                            self.walk_expr(fscope, v);
                        }
                    }
                    // 包级 `const` 的值由常量求值器消费（`Task 24`）：那边对
                    // 「非常量」有**更准的**代号（`is not constant`），这里再走一遍
                    // 名字只会多出一条噪告 ⇒ 不重复走（`open_gaps`）。
                    Decl::Func(f) => self.walk_func_decl(fscope, f),
                    Decl::Const(_) | Decl::Type(_) | Decl::Import(_) => {}
                }
            }
        }
    }

    // ── 声明位置 ──────────────────────────────────────────────────────────

    /// 一个 `func` 声明的体。
    ///
    /// `resolve_sig = false`：包级函数的签名（接收者/参数/结果/类型参数约束）
    /// 在 `collect_decls` 里已经解析过，这里只补**名字**。
    fn walk_func_decl(&mut self, parent: ScopeId, f: &FuncDecl) {
        let Expr::FuncType(ft) = &f.type_ else { return };
        let Some(body) = &f.body else { return };
        let fscope = self.func_scope(parent, &f.tparams, ft, f.recv.as_ref(), false);
        let bscope = self.scopes.child(ScopeKind::Block, fscope);
        self.walk_stmts(bscope, &body.stmts);
        // 值作用域走完，再走**标签**那一趟（官方 `stmt.go:44` 的
        // `if check.hasLabel { check.labels(body) }` —— 两趟、**每个函数体各一次**）
        crate::labels::check_labels(&body.stmts, self.bag);
    }

    /// 函数字面量：**全部**解析（没有别人替它解析过）。
    fn walk_func_lit(&mut self, outer: ScopeId, fl: &FuncLit) {
        let Expr::FuncType(ft) = fl.type_.as_ref() else {
            // 语法上到不了（`FuncLit.type_` 恒 `FuncType`）；真到了也只是解析失败
            self.walk_expr(outer, &fl.type_);
            return;
        };
        // 函数字面量**没有**类型参数（Go 语法不允许）⇒ `tparams` 恒空
        let fscope = self.func_scope(outer, &[], ft, None, true);
        let bscope = self.scopes.child(ScopeKind::Block, fscope);
        self.walk_stmts(bscope, &fl.body.stmts);
        // **每个**函数体（含字面量）各有一个标签空间 ⇒ 跨函数不可见
        crate::labels::check_labels(&fl.body.stmts, self.bag);
    }

    /// 类型参数层 ⇒ 函数层（接收者 / 参数 / 结果）。
    #[allow(clippy::too_many_arguments)]
    fn func_scope(
        &mut self,
        parent: ScopeId,
        tparams: &[AstField],
        ft: &FuncType,
        recv: Option<&AstField>,
        resolve_types: bool,
    ) -> ScopeId {
        // 接收者**自己声明**的类型参数（`TYP-22`）：`func (b *Box[T]) M()` 的 `T`。
        // 条目由包级那一趟（`resolve_func_sig`）建好，这里只把**名字**补进作用域
        // —— 体内那一趟 `resolve_types == false`，类型不参与，名字必须在。
        // ⚠️ 不能在这里 `declare_tparams`：那会**再建一套**类型参数条目，
        // 编号随之后移（`C7` §3 的号是遍历序，多一个条目就全错位）。
        let recv_names: Vec<&Name> = recv
            .and_then(crate::decls::recv_type_args)
            .map(|(_, ns)| ns)
            .unwrap_or_default();
        let mut outer = parent;
        if !tparams.is_empty() || !recv_names.is_empty() {
            outer = self.scopes.child(ScopeKind::Func, parent);
            // 先只声明名字：约束的类型解析归包级（`resolve_types == false` 时）
            for n in &recv_names {
                let inv = self.tt.invalid();
                self.declare_in(outer, ObjKind::TypeName, n, inv);
            }
            for f in tparams {
                if let Some(n) = &f.name {
                    let inv = self.tt.invalid();
                    self.declare_in(outer, ObjKind::TypeName, n, inv);
                }
            }
            if resolve_types {
                for f in tparams {
                    self.resolve_type(outer, &f.type_);
                }
            }
        }
        let fscope = self.scopes.child(ScopeKind::Func, outer);
        // 接收者类型在**外层**解析（官方：接收者不能引用方法自己的类型参数）
        if let Some(r) = recv {
            self.declare_param(fscope, parent, r, resolve_types);
        }
        for f in &ft.params {
            self.declare_param(fscope, parent, f, resolve_types);
        }
        for f in &ft.results {
            self.declare_param(fscope, parent, f, resolve_types);
        }
        fscope
    }

    /// 声明一个参数 / 结果 / 接收者的名字（类型在 `outer` 解析、名字进 `scope`）。
    ///
    /// `x, y int` 这种写法在前端被拆成**两份** `Field`（同一个类型表达式被克隆）
    /// ⇒ 类型解析走「报过就不再报」的集合，不会报两条。
    fn declare_param(&mut self, scope: ScopeId, outer: ScopeId, f: &AstField, resolve_types: bool) {
        let typ = if resolve_types {
            self.resolve_type(outer, &f.type_)
        } else {
            self.tt.invalid()
        };
        if let Some(n) = &f.name {
            self.declare_in(scope, ObjKind::Var, n, typ);
        }
    }

    /// 往作用域里声明一个名字，并把「重复」变成诊断。
    fn declare_in(&mut self, scope: ScopeId, kind: ObjKind, n: &Name, typ: TypeId) {
        declare_name(
            self.scopes,
            scope,
            kind,
            &n.value,
            n.pos,
            Some(typ),
            self.bag,
        );
    }

    // ── 语句 ──────────────────────────────────────────────────────────────

    fn walk_stmts(&mut self, scope: ScopeId, stmts: &[Stmt]) {
        for s in stmts {
            self.walk_stmt(scope, s);
        }
    }

    fn walk_stmt(&mut self, scope: ScopeId, s: &Stmt) {
        match s {
            Stmt::Empty(_) => {}
            // `L: stmt` —— 标签在**另一趟**（`crate::labels`）；这里只走下去
            Stmt::Labeled(l) => self.walk_stmt(scope, &l.stmt),
            Stmt::Block(b) => {
                let inner = self.scopes.child(ScopeKind::Block, scope);
                self.walk_stmts(inner, &b.stmts);
            }
            Stmt::Expr(e) => self.walk_expr(scope, &e.x),
            Stmt::Send(sd) => {
                self.walk_expr(scope, &sd.chan);
                self.walk_expr(scope, &sd.value);
            }
            Stmt::Decl(d) => self.walk_decl_stmt(scope, d),
            Stmt::Assign(a) => self.walk_assign(scope, a),
            // `break`/`continue`/`goto`/`fallthrough`：跳转目标是**标签** ⇒ `crate::labels`
            Stmt::Branch(_) => {}
            Stmt::Call(c) => self.walk_expr(scope, &c.call),
            Stmt::Return(r) => {
                for e in &r.results {
                    self.walk_expr(scope, e);
                }
            }
            Stmt::If(i) => self.walk_if(scope, i),
            Stmt::For(f) => self.walk_for(scope, f),
            Stmt::Switch(sw) => self.walk_switch(scope, sw),
            Stmt::Select(sel) => self.walk_select(scope, sel),
        }
    }

    /// `if`：一个 `if` 层，`then`/`else` 各自再开一层（官方 `stmt.go:606`）。
    fn walk_if(&mut self, scope: ScopeId, i: &IfStmt) {
        let head = self.scopes.child(ScopeKind::Block, scope);
        if let Some(init) = &i.init {
            self.walk_simple(head, init);
        }
        if let Some(c) = &i.cond {
            self.walk_expr(head, c);
        }
        let then = self.scopes.child(ScopeKind::Block, head);
        self.walk_stmts(then, &i.then.stmts);
        match i.els.as_deref() {
            None => {}
            // `else if`：**同一个** if 层里再走一个 `if`（它自己会再开一层）
            Some(Stmt::If(nested)) => self.walk_if(head, nested),
            Some(other) => {
                let eb = self.scopes.child(ScopeKind::Block, head);
                self.walk_stmt(eb, other);
            }
        }
    }

    /// `for`：`range` 形式与普通形式的作用域**不同**（官方 `range.go:88` 单独一层）。
    fn walk_for(&mut self, scope: ScopeId, f: &ForStmt) {
        let fscope = self.scopes.child(ScopeKind::Block, scope);
        match &f.init {
            Some(SimpleStmt::Range(r)) => self.walk_range_clause(fscope, r),
            other => {
                if let Some(init) = other {
                    self.walk_simple(fscope, init);
                }
                if let Some(c) = &f.cond {
                    self.walk_expr(fscope, c);
                }
                if let Some(post) = &f.post {
                    self.walk_simple(fscope, post);
                }
            }
        }
        let body = self.scopes.child(ScopeKind::Block, fscope);
        self.walk_stmts(body, &f.body.stmts);
    }

    /// `switch`（含**类型 switch**：`switch v := x.(type)` 的 `v` 每个 case 各声明一份）。
    fn walk_switch(&mut self, scope: ScopeId, sw: &SwitchStmt) {
        let sscope = self.scopes.child(ScopeKind::Block, scope);
        if let Some(init) = &sw.init {
            self.walk_simple(sscope, init);
        }
        let mut guard_lhs: Option<&Name> = None;
        if let Some(tag) = &sw.tag {
            match tag {
                Expr::TypeSwitchGuard(g) => {
                    guard_lhs = g.lhs.as_ref();
                    self.walk_expr(sscope, &g.x);
                }
                other => self.walk_expr(sscope, other),
            }
        }
        for c in &sw.cases {
            let cscope = self.scopes.child(ScopeKind::Block, sscope);
            match (guard_lhs, &c.cases) {
                // 类型 switch：case 的值是**类型**
                (Some(lhs), Some(vals)) => {
                    self.walk_case_types(sscope, vals);
                    // 官方把隐式变量声明进 **case** 作用域（`stmt.go:762`），
                    // 类型随每个 case 而变 ⇒ M3 用 `invalid`（类型判定归 M4）
                    let inv = self.tt.invalid();
                    self.declare_in(cscope, ObjKind::Var, lhs, inv);
                }
                // 表达式 switch：case 的值是**表达式**
                (None, Some(vals)) => self.walk_case_values(sscope, vals),
                _ => {}
            }
            self.walk_stmts(cscope, &c.body);
        }
    }

    /// `select`：每个通信子句一层（官方 `stmt.go:786` 起）。
    fn walk_select(&mut self, scope: ScopeId, sel: &SelectStmt) {
        for c in &sel.clauses {
            let cscope = self.scopes.child(ScopeKind::Block, scope);
            if let Some(comm) = &c.comm {
                self.walk_simple(cscope, comm);
            }
            self.walk_stmts(cscope, &c.body);
        }
    }

    /// `case 1, 2:` 的**值**（表达式 switch）—— 官方在开 case 作用域**之前**求值
    /// （`stmt.go:663` 的 `check.caseValues`）⇒ 住在 switch 层。
    fn walk_case_values(&mut self, scope: ScopeId, vals: &Expr) {
        match vals {
            Expr::List(l) => {
                for e in &l.elems {
                    self.walk_expr(scope, e);
                }
            }
            other => self.walk_expr(scope, other),
        }
    }

    /// 类型 switch 的 `case T1, T2:` —— 那一串是**类型**。
    fn walk_case_types(&mut self, scope: ScopeId, vals: &Expr) {
        match vals {
            Expr::List(l) => {
                for e in &l.elems {
                    self.resolve_type(scope, e);
                }
            }
            other => {
                self.resolve_type(scope, other);
            }
        }
    }

    // ── 简单语句 / 声明语句 ───────────────────────────────────────────────

    fn walk_simple(&mut self, scope: ScopeId, s: &SimpleStmt) {
        match s {
            SimpleStmt::Empty => {}
            SimpleStmt::Expr(e) => self.walk_expr(scope, &e.x),
            SimpleStmt::Send(sd) => {
                self.walk_expr(scope, &sd.chan);
                self.walk_expr(scope, &sd.value);
            }
            SimpleStmt::Assign(a) => self.walk_assign(scope, a),
            // 只在 `for` 头里出现，由 [`Checker::walk_for`] 处理；这里兜底走一遍
            SimpleStmt::Range(r) => self.walk_range_clause(scope, r),
        }
    }

    fn walk_decl_stmt(&mut self, scope: ScopeId, d: &DeclStmt) {
        for decl in &d.decls {
            match decl {
                Decl::Var(v) => {
                    let typ = v
                        .type_
                        .as_ref()
                        .map(|t| self.resolve_type(scope, t))
                        .unwrap_or_else(|| self.tt.invalid());
                    for n in &v.names {
                        self.declare_in(scope, ObjKind::Var, n, typ);
                    }
                    for e in &v.values {
                        self.walk_expr(scope, e);
                    }
                }
                Decl::Const(c) => {
                    let typ = c
                        .type_
                        .as_ref()
                        .map(|t| self.resolve_type(scope, t))
                        .unwrap_or_else(|| self.tt.invalid());
                    for n in &c.names {
                        self.declare_in(scope, ObjKind::Const, n, typ);
                    }
                    // 局部常量 M3 **不求值**（常量求值面在包级，`Task 24`）⇒
                    // 值里的名字照样要查（`const k = Undef` 该报 undefined）
                    for e in &c.values {
                        self.walk_expr(scope, e);
                    }
                }
                Decl::Type(t) => {
                    let typ = if t.tparams.is_empty() {
                        self.resolve_type(scope, &t.type_)
                    } else {
                        let ts = self.tparam_scope(scope, &t.tparams);
                        self.resolve_type(ts, &t.type_)
                    };
                    // 局部类型名用**本体**当类型（见模块头的 open_gaps）
                    self.declare_in(scope, ObjKind::TypeName, &t.name, typ);
                }
                Decl::Func(f) => {
                    let inv = self.tt.invalid();
                    self.declare_in(scope, ObjKind::Func, &f.name, inv);
                    self.walk_func_decl(scope, f);
                }
                // 函数体内的 `import` 是语法错误（官方 `BadDecl`），到这儿的可能性为零
                Decl::Import(_) => {}
            }
        }
    }

    /// 局部泛型声明（`type S[T any] struct{}`）的类型参数层：这里**要**解析约束
    /// （没有任何人替它解析过）。
    fn tparam_scope(&mut self, parent: ScopeId, tparams: &[AstField]) -> ScopeId {
        let scope = self.scopes.child(ScopeKind::Func, parent);
        for f in tparams {
            if let Some(n) = &f.name {
                let inv = self.tt.invalid();
                self.declare_in(scope, ObjKind::TypeName, n, inv);
            }
        }
        for f in tparams {
            self.resolve_type(scope, &f.type_);
        }
        scope
    }

    /// 赋值 / 短变量声明。
    ///
    /// `:=` 的顺序照官方 `shortVarDecl`（`assignments.go:534`）：
    /// **先**判「哪些是新声明」（同层已有的是赋值，不是声明）、
    /// **再**求值右侧、**最后**才把新变量插进作用域。
    /// 这个顺序在 `x := x` 上可观测：外层没有 `x` 时官方报 `undefined: x`
    /// （新变量此时还没进作用域），先插后走就会漏掉。
    fn walk_assign(&mut self, scope: ScopeId, a: &AssignStmt) {
        if a.op != AssignOp::Define {
            for e in &a.lhs {
                // `_ = x` 的 `_` 是**空赋值**，不是值引用（官方 `assignVar` 里先特判 `_`）。
                // ⚠️ 只对**纯 `=`** 成立：复合赋值（`_ += 1`）与 `_++` 走的是
                // `check.binary`/`check.expr`（`stmt.go:530`）⇒ 那里报 `InvalidBlank`。
                if a.op == AssignOp::Assign && is_blank(e) {
                    continue;
                }
                self.walk_expr(scope, e);
            }
            for e in &a.rhs {
                self.walk_expr(scope, e);
            }
            return;
        }

        // ① 分类 + `RepeatedDecl`（官方 `assignments.go:552`）
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut fresh: Vec<(Name, bool)> = Vec::new();
        for e in &a.lhs {
            match e {
                Expr::Name(n) => {
                    if n.value == "_" {
                        continue;
                    }
                    if !seen.insert(n.value.clone()) {
                        self.bag.push(Diag {
                            pos: n.pos,
                            code: DiagCode::TypRepeatedDecl,
                            message: format!("{} repeated on left side of :=", n.value),
                        });
                        continue;
                    }
                    // **本层**已有 ⇒ 这是赋值（官方 `scope.Lookup` 不看父链）
                    let exists = self.scopes.get(scope).lookup_local(&n.value).is_some();
                    fresh.push((n.clone(), !exists));
                }
                // 非名字（`x.f := …`）：官方报 `BadDecl`，M3 只当引用走一遍
                other => self.walk_expr(scope, other),
            }
        }
        // ② 右侧求值（此时新变量还不在作用域里）
        for e in &a.rhs {
            self.walk_expr(scope, e);
        }
        // ③ 插入新变量
        for (n, is_new) in &fresh {
            if *is_new {
                let inv = self.tt.invalid();
                self.declare_in(scope, ObjKind::Var, n, inv);
            }
        }
    }

    /// `for k, v := range x` 的迭代变量（官方 `range.go:88` 起）。
    ///
    /// 顺序同样是**先 range 表达式、后声明变量**（官方注释：
    /// “Open the for-statement block scope now, **after the range clause**”）。
    fn walk_range_clause(&mut self, scope: ScopeId, r: &RangeClause) {
        self.walk_expr(scope, &r.x);
        if !r.is_define {
            for e in &r.lhs {
                // `for _ = range x`：`_` 同样是空赋值位（与 `=` 同一条规则）
                if is_blank(e) {
                    continue;
                }
                self.walk_expr(scope, e);
            }
            return;
        }
        for e in &r.lhs {
            match e {
                Expr::Name(n) => {
                    let inv = self.tt.invalid();
                    self.declare_in(scope, ObjKind::Var, n, inv);
                }
                other => self.walk_expr(scope, other),
            }
        }
    }

    // ── 表达式 ────────────────────────────────────────────────────────────

    /// 走一个**值位置**的表达式。类型族（转换、字面量的类型、类型断言的目标）
    /// 交给 [`Checker::resolve_type`] —— 名字的「找不找得到」是同一张作用域表、
    /// 同一条代号，只是报点在类型那条路上。
    fn walk_expr(&mut self, scope: ScopeId, e: &Expr) {
        match e {
            Expr::Bad(_) | Expr::BasicLit(_) => {}
            Expr::Name(n) => self.use_name(scope, n),
            Expr::CompositeLit(l) => {
                if let Some(t) = &l.type_ {
                    self.resolve_type(scope, t);
                }
                for el in &l.elems {
                    self.walk_composite_elem(scope, el);
                }
            }
            Expr::KeyValue(kv) => {
                self.walk_expr(scope, &kv.key);
                self.walk_expr(scope, &kv.value);
            }
            Expr::FuncLit(fl) => self.walk_func_lit(scope, fl),
            Expr::Paren(p) => self.walk_expr(scope, &p.x),
            // `sel` 是**选择子**（字段/方法名），不是作用域里的名字
            Expr::Selector(s) => self.walk_expr(scope, &s.x),
            Expr::Index(i) => {
                self.walk_expr(scope, &i.x);
                self.walk_expr(scope, &i.index);
            }
            Expr::Slice(s) => {
                self.walk_expr(scope, &s.x);
                for e in [&s.low, &s.high, &s.max].into_iter().flatten() {
                    self.walk_expr(scope, e);
                }
            }
            Expr::TypeAssert(a) => {
                self.walk_expr(scope, &a.x);
                if let Some(t) = &a.type_ {
                    self.resolve_type(scope, t);
                }
            }
            // 类型 switch 的 guard：`lhs` 是**声明**（每个 case 各一份）
            Expr::TypeSwitchGuard(g) => self.walk_expr(scope, &g.x),
            Expr::Unary(op) => {
                self.walk_expr(scope, &op.x);
                if let Some(y) = &op.y {
                    self.walk_expr(scope, y);
                }
            }
            Expr::Binary(op) => {
                self.walk_expr(scope, &op.x);
                if let Some(y) = &op.y {
                    self.walk_expr(scope, y);
                }
            }
            Expr::Call(c) => {
                self.walk_expr(scope, &c.fun);
                for a in &c.args {
                    self.walk_expr(scope, a);
                }
            }
            Expr::List(l) => {
                for e in &l.elems {
                    self.walk_expr(scope, e);
                }
            }
            // 类型族出现在**表达式位置**（转换 `[]byte(s)`、`struct{…}(x)`
            // 的 `fun`、类型断言/字面量的类型）：整体是类型位置
            Expr::ArrayType(_)
            | Expr::SliceType(_)
            | Expr::DotsType(_)
            | Expr::StructType(_)
            | Expr::InterfaceType(_)
            | Expr::FuncType(_)
            | Expr::MapType(_)
            | Expr::ChanType(_) => {
                self.resolve_type(scope, e);
            }
        }
    }

    /// 复合字面量的一个元素。
    ///
    /// ⚠️ **裸名字 key 不当引用**：`T{Field: 1}` 的 `Field` 是字段名，
    /// 而分不出「字段名 / map 键」需要类型信息（M4）。取舍见模块头。
    fn walk_composite_elem(&mut self, scope: ScopeId, el: &Expr) {
        match el {
            Expr::KeyValue(kv) => {
                if !matches!(kv.key.as_ref(), Expr::Name(_)) {
                    self.walk_expr(scope, &kv.key);
                }
                self.walk_expr(scope, &kv.value);
            }
            other => self.walk_expr(scope, other),
        }
    }

    /// 一次名字**引用**。
    ///
    /// ⚠️ 空标识符 `_` 走的是**另一条**诊断：官方 `typexpr.go:28` 的
    /// `InvalidBlank`（“cannot use _ as value or type”）——不是 `undefined: _`。
    /// `_` 从不绑定，所以任何「查作用域查不到」的通用路径都会把它误报成
    /// `undefined`；判据不能是「出现在表达式里」，只能是「出现在**哪个位置**」。
    /// 赋值（`=`）/ 短变量声明（`:=`）/ `range` 的**左值位**是官方唯一容许它的地方，
    /// 那几处由调用方**先**筛掉（见 [`Checker::walk_assign`]、[`Checker::walk_range_clause`]）。
    fn use_name(&mut self, scope: ScopeId, n: &Name) {
        if n.value == "_" {
            self.bag.push(Diag {
                pos: n.pos,
                code: DiagCode::TypInvalidBlank,
                message: "cannot use _ as value or type".to_owned(),
            });
            return;
        }
        if self.scopes.lookup(scope, &n.value).is_none() {
            self.report_undefined(n.pos, &n.value);
        }
    }

    /// 报 `undefined: X` —— **同一个位置只报一次**。
    ///
    /// 去重的必要性来自前端 [`rgoc_ast`] 的形状：`x, y int` 会被拆成两份
    /// `Field`，而类型表达式是**克隆**的 ⇒ 同一个字面位置会被走到两次，
    /// 官方只报一次。`Pos` 唯一标识一个**书写位置**，正好是去重的键。
    pub(crate) fn report_undefined(&mut self, pos: Pos, name: &str) {
        if !self.undefined_reported.insert(pos) {
            return;
        }
        self.bag.push(Diag {
            pos,
            code: DiagCode::TypUndeclaredName,
            message: format!("undefined: {name}"),
        });
    }
}
