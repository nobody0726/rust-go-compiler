//! 语句静态规则（M4/Task 25；`T-CK-35`）。
//!
//! # 对照面
//!
//! 官方 `go/types/stmt.go`（`stmt` / `stmtList` / `caseValues` / `caseTypes` /
//! `multipleDefaults` / `suspendedCall`）、`assignments.go`（`assignVars` /
//! `initVars` / `assignError` / `returnError` / `shortVarDecl`）、`call.go`
//! （`arguments` 的个数与逐参面）与 `return.go`（`isTerminating` / `hasBreak`）。
//! 位置的逐条实证见 `probe/task25-reject.go`（35 行）与 `tests/stmt.rs` 的
//! `EXPECT_REJECT` 文档表。
//!
//! # 接进来的面
//!
//! | 语句 | 判什么 | 码 |
//! |---|---|---|
//! | 表达式语句 | 值未被使用（调用 / 接收除外） | `ChkUnusedExpr`（`unused`） |
//! | `ch <- v` | 元素面通道、值可赋值 | `ChkInvalidOperation` / `ChkInvalidAssign` |
//! | 局部声明 | 把名字与类型记进 [`Env`] | — |
//! | `=` / `:=` / 复合 / `++` | 个数、左值性、可赋值性、新变量 | `ChkInvalidAssign` / `ChkNoNewVar` / `ChkInvalidOperation` |
//! | `if` / `for` | 条件必须是布尔 | `ChkInvalidCond` |
//! | `switch` | case 值可赋值 / 重复 / 默认子句唯一 | `ChkInvalidCase` |
//! | 类型 `switch` | 守卫必须是接口、case 必须是类型 | `ChkInvalidOperation` / `ChkNotAType` |
//! | `select` | 子句必须是发送 / 接收、`default` 唯一 | `ChkInvalidSelectCase` |
//! | `go` / `defer` | 必须是调用、结果不能有 | `ChkInvalidGoDefer` |
//! | `return` | 个数 + 逐个可赋值 | `ChkWrongResultCount` / `ChkInvalidAssign` |
//! | 函数体收尾 | 有结果且不终止 ⇒ 缺 `return` | `ChkMissingReturn` |
//!
//! # 刻意不做（台账登记；负向缺口由 `U-M4` 面承担）
//!
//! * **函数字面量的体**不在这里走（`Expr::FuncLit`）：它的参数/结果类型没有
//!   已解析的签名可依，归 Task 27 的复合字面量/字面量面一并接；
//! * `range` 只接「变量声明 / 赋值 + 求值」（迭代形态归 Task 26）；
//! * 复合赋值**不再**自建近似：M5/Task 5 起走 `expr.rs` 的
//!   [`ExprChecker::binary_assign`]（官方 `stmt.go:524` 的 `binary(…, nil, …)`）
//!   + `assignVar`，于是移位计数类型、未类型化常量截断、除零、`&^=` 全部对齐；
//! * `switch` 的 tag 不做「可比性」判定（`InvalidExprSwitch`，归 Task 28）；
//! * `break`/`continue`/`goto`/`fallthrough` 的**位置合法性**在 M2 的
//!   `checkBranches`（`SYN-BRANCH-*`）与 M3 的 `labels`（`TYP-*`）已报 ——
//!   本层不重复；
//! * 未使用变量 / 标签（`UnusedVar` / `UnusedLabel`）不在本任务（`T-CK-76`）。

use rgoc_ast::{
    AssignOp, AssignStmt, BlockStmt, CallStmt, CallTok, CaseClause, CommClause, Decl, DeclStmt,
    Expr, ForStmt, FuncDecl, IfStmt, Op, RangeClause, ReturnStmt, SelectStmt, SimpleStmt, Stmt,
    SwitchStmt,
};
use rgoc_const::{BasicId, Sizes, default_type_of, representable};
use rgoc_lex::{Diag, DiagCode, FileId, Pos};
use rgoc_types::{ChanDir, Type, TypeId, TypeParamId, underlying};

use crate::check::Session;
use crate::env::Env;
use crate::expr::{
    At, BinSrc, ExprChecker, all_boolean, all_integer, all_numeric, basic_kind, common_under,
    expr_label, is_non_typeparam_interface, is_untyped, operand_text, type_str,
};
use crate::operand::{Mode, Operand};
use crate::version::{self, Feat};

/// 包级函数体入口（`check_package` 在表达式面之后调用）。
pub(crate) fn check_package_bodies(s: &mut Session<'_>) {
    let pkg = s.pkg;
    let mut cx = StmtChecker {
        s,
        env: Env::new(),
        results: Vec::new(),
    };
    for (i, ast) in pkg.asts.iter().enumerate() {
        let file = pkg.files[i];
        for decl in &ast.decls {
            if let Decl::Func(f) = decl {
                cx.func_decl(file, f);
            }
        }
    }
}

/// 语句检查器。
struct StmtChecker<'s, 'p> {
    s: &'s mut Session<'p>,
    /// 局部作用域（见 [`Env`]）。
    env: Env,
    /// 当前函数的结果表：`(名字, 类型)`（无名结果 ⇒ `None`）。
    results: Vec<(Option<String>, TypeId)>,
}

impl<'s, 'p> StmtChecker<'s, 'p> {
    // ─────────────────────────── 基础设施 ───────────────────────────

    fn report(&mut self, pos: Pos, code: DiagCode, message: String) {
        self.s.report_hard(Diag { pos, code, message });
    }

    /// 走一个表达式的求值（局部作用域随行 —— 与 [`ExprChecker`] 共用一张栈）。
    fn eval(&mut self, file: FileId, e: &Expr) -> Operand {
        let mut cx = ExprChecker {
            s: &mut *self.s,
            env: Some(&self.env),
        };
        cx.expr(file, e)
    }

    /// 官方 `check.assignment` 的转发（可赋值性 + 报点在**表达式位**）。
    fn assign_to(&mut self, x: &mut Operand, xpos: Pos, target: Option<TypeId>, ctx: &str) -> bool {
        let mut cx = ExprChecker {
            s: &mut *self.s,
            env: Some(&self.env),
        };
        cx.assignment(x, xpos, target, ctx)
    }

    /// 包级函数 / 方法的签名。
    ///
    /// 方法名**不在**包作用域 ⇒ 走接收者基类型的方法表
    /// （`TypeTable::named_def(..).methods`，装载面 `add_named_method` 的唯一
    /// 消费者面）。早先这里对方法一律返回 `None`，`func_decl` 又
    /// `unwrap_or_default()` ⇒ 结果表被清空 ⇒ **每个带结果的方法体都误报**
    /// `too many return values`（Task 33 实测：`check/init1.go` 6 行、
    /// `check/init2.go` 20 行假诊断，把真环诊断全淹了）。
    fn func_sig(&mut self, file: FileId, f: &FuncDecl) -> Option<(TupleVars, TupleVars)> {
        let t = if let Some(recv) = &f.recv {
            self.method_sig(&recv.type_, &f.name.value)?
        } else if let Some(obj) = self.s.pkg.lookup(&f.name.value) {
            obj.typ?
        } else {
            // 名字**不在**包作用域：`func init`（Task 34：官方有意不声明）、
            // `func _`、以及重名声明里输掉的那个。它们的体照样要按**真实**
            // 签名检查 —— 退回 AST 求签名，别用空结果表（那会把
            // `func init() int { return 0 }` 误报成 too many return values）。
            return self.ast_sig(file, f);
        };
        let tt = &*self.s.tt;
        match tt.type_of(underlying(tt, t)) {
            Type::Signature(id) => {
                let sig = tt.signature_def(*id);
                Some((tuple_vars(tt, sig.params), tuple_vars(tt, sig.results)))
            }
            _ => None,
        }
    }

    /// 直接从 AST 的 `FuncType` 求签名（作用域里查不到名字时的兜底）。
    fn ast_sig(&mut self, file: FileId, f: &FuncDecl) -> Option<(TupleVars, TupleVars)> {
        let Expr::FuncType(ft) = &f.type_ else {
            return None;
        };
        let mut params: TupleVars = Vec::with_capacity(ft.params.len());
        for p in &ft.params {
            // 变参的末位类型是 **切片**（与装载面 `resolve_signature` 同口径）
            let t = match &p.type_ {
                Expr::DotsType(d) => {
                    let elem = self.type_of_expr(file, &d.elem)?;
                    self.s.tt.add_slice(elem)
                }
                other => self.type_of_expr(file, other)?,
            };
            params.push((p.name.as_ref().map(|n| n.value.clone()), t));
        }
        let mut results: TupleVars = Vec::with_capacity(ft.results.len());
        for r in &ft.results {
            let t = self.type_of_expr(file, &r.type_)?;
            results.push((r.name.as_ref().map(|n| n.value.clone()), t));
        }
        Some((params, results))
    }

    /// 方法签名：接收者**基类型**的方法表里按名字取。
    ///
    /// 接收者只可能是 `T` / `*T` / `T[P]` 三种写法（语法层已保证），
    /// 所以取基类型名**不需要**完整求值 —— 直接读类型表达式。
    fn method_sig(&self, recv_ty: &Expr, name: &str) -> Option<TypeId> {
        let base = recv_base_name(recv_ty)?;
        let t = self.s.pkg.lookup(base)?.typ?;
        let tt = &*self.s.tt;
        let nid = match tt.type_of(t) {
            Type::Named(id) => *id,
            Type::Alias(a) => match tt.type_of(tt.alias_rhs(*a)) {
                Type::Named(id) => *id,
                _ => return None,
            },
            _ => return None,
        };
        tt.named_def(nid)
            .methods
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, sig)| *sig)
    }

    /// 类型表达式 → 类型（不是类型 ⇒ `None`）。
    /// 类型表达式 → `TypeId`（转发到 [`ExprChecker::type_expr`]；含匿名
    /// `struct{…}` / `interface{…}` / `func(…)`，M4/Task 37 补）。
    fn type_expr(&mut self, file: FileId, e: &Expr) -> Option<TypeId> {
        let mut cx = ExprChecker {
            s: &mut *self.s,
            env: Some(&self.env),
        };
        cx.type_expr(file, e)
    }

    fn type_of_expr(&mut self, file: FileId, e: &Expr) -> Option<TypeId> {
        let x = self.eval(file, e);
        if x.mode == Mode::TypeExpr {
            x.typ
        } else {
            None
        }
    }

    /// 无类型常量取**默认型**（`y := 1` 的 `y` 是 `int`）；其余返回自身。
    ///
    /// ⚠️ 非基本类型（签名 / 结构 / map / 指针 …）**必须**走 `Some(t)` 分支 ——
    /// 早先这里对 `basic_kind` 用了 `?`，于是 `m1 := T.M` 把 `m1` 登记成
    /// 「类型未知」，后续（`for range m1` / `m1 + 1`）全部静默。
    /// 实测：`spec/range.go` 的 `for range m1`（方法表达式）因此漏报
    /// （oracle `30:12 range`）。
    fn default_type(&mut self, x: &Operand) -> Option<TypeId> {
        let t = x.typ?;
        match basic_kind(&*self.s.tt, t) {
            Some(b) if b.is_untyped() => Some(self.s.tt.add_basic(default_type_of(b))),
            _ => Some(t),
        }
    }

    /// 名字在**局部**环境里的操作数（没有 ⇒ `None`）。
    fn local(&self, name: &str) -> Option<&Operand> {
        self.env.lookup(name)
    }

    // ─────────────────────────── 函数体 ───────────────────────────

    fn func_decl(&mut self, file: FileId, f: &FuncDecl) {
        let Some(body) = &f.body else {
            return;
        };
        let (params, results) = self.func_sig(file, f).unwrap_or_default();
        self.results = results.clone();

        // 函数层：参数 + 命名结果（类型取自签名 —— 装载面不给它们类型）
        self.env.push();
        self.declare_tparams(f);
        self.declare_recv_tparams(f);
        if let Expr::FuncType(ft) = &f.type_ {
            for (i, p) in ft.params.iter().enumerate() {
                if let Some(n) = &p.name {
                    let t = params.get(i).map(|(_, t)| *t);
                    self.env.declare(&n.value, Operand::variable(t));
                }
            }
            for (i, p) in ft.results.iter().enumerate() {
                if let Some(n) = &p.name {
                    let t = results.get(i).map(|(_, t)| *t);
                    self.env.declare(&n.value, Operand::variable(t));
                }
            }
        }

        // 体块：与参数**同一层**（Go 规范「同一块或函数体的参数表」——
        // 参数与体首层共享「已声明」判据，`func f(x int) { x := 1 }` 必须报
        // `no new variables`）。块内的 `{}` 才另开一层。
        self.stmts(file, &body.stmts);
        self.env.pop();

        // 官方 `stmt.go:48`：有结果且体不终止 ⇒ 在**右花括号**处报缺 return。
        if !self.results.is_empty() && !terminating_list(&body.stmts, "") {
            self.report(
                body.rbrace,
                DiagCode::ChkMissingReturn,
                "missing return".to_owned(),
            );
        }
    }

    /// 把泛型函数的类型参数声明进**函数层**作用域（`M12/Task 9`）。
    ///
    /// # 为什么必须有
    ///
    /// 官方把类型参数声明在「函数声明的作用域」里，函数体在其内 ⇒ 体里能看见
    /// `T`。本仓的 [`rgoc_load::Package::lookup_in_file`] 只有「文件 → 包 →
    /// universe」三层，**看不见函数级类型参数**，于是泛型函数体里的
    /// `type_expr(T)` 直接失败。
    ///
    /// 那个失败**不报错**（`type_expr` 返回 `None`，各调用方各自决定怎么办），
    /// 而 `make` 的写法是 `let Some(t) = cx.type_expr(…) else { return
    /// Operand::invalid() }` ⇒ 整个 `make` 面在泛型函数体里等于**没检查**。
    /// 实测：`test/typeparam/builtins.go` 的 `make(S1, 10)` / `make(M1)` 一处
    /// 诊断都没有（同一个文件的 `make(M1)` 报错是因为 `M1` 是**包级**接口）。
    ///
    /// # 名字与 id 怎么配对
    ///
    /// `pkg.generic_funcs` 存的是**声明序**的 `TypeParamId` 表（Task 6 建的），
    /// 没有名字；AST 的 `tparams` 有名字且**逐个名字一个 `Field`**。两边都按
    /// 声明序走，逐位配对。
    fn declare_tparams(&mut self, f: &FuncDecl) {
        let Some(ids) = self.s.pkg.generic_funcs.get(&f.name.value).cloned() else {
            return;
        };
        // `Field` 是**逐名字**的（parser 把 `[T, U any]` 拆成两个 `Field`），
        // 所以名字与 id 是按下标一一对应。
        for (k, field) in f.tparams.iter().enumerate() {
            let Some(name) = &field.name else { continue };
            if let Some(id) = ids.get(k).copied() {
                let self_ty = self.s.tt.type_param_def(id).self_ty;
                self.env.declare(&name.value, Operand::type_expr(self_ty));
            }
        }
    }

    /// 把**方法接收者**声明的类型参数声明进函数层（`M12/Task 12`）。
    ///
    /// `func (s Stream[T]) DropWhile(…)` 的 `T` 由**接收者**声明
    /// （官方 `collectRecv` 的 `declareTypeParam`，`signature.go:276`），
    /// 它与方法自己的 `[P any]` 是**两组**。Task 9 只补了自己那组
    /// ⇒ 接收者的 `T` 在体里查不到，于是 `Stream[T]{it: it}` 这种复合字面量
    /// 的类型报 `value is not a type`（`issue48645b.go:35` / `issue54225.go:13`）。
    ///
    /// # id 从哪来
    ///
    /// 装载面已经把接收者的类型参数写进 `SignatureDef::recv_tparams`
    /// （M4/Task 12 的 `set_signature_recv_tparams`；按**位置**对齐，
    /// 供 `rgoc-types::instantiate` 的逐方法替换用）。这里取同一份：
    /// 名字从 AST 的接收者写法按序读，id 从签名读，逐位配对。
    ///
    /// ⚠️ 两边个数不必相等 —— `recv_tparams` 空（不是泛型接收者 / 装载期
    /// 拆不出）时 `zip` 自然截断，不会造出错的绑定。
    fn declare_recv_tparams(&mut self, f: &FuncDecl) {
        let Some(recv) = &f.recv else { return };
        let names = recv_tparam_names(&recv.type_);
        if names.is_empty() {
            return;
        }
        let Some(t) = self.method_sig(&recv.type_, &f.name.value) else {
            return;
        };
        let ids: Vec<TypeParamId> = {
            let tt = &*self.s.tt;
            match tt.type_of(underlying(tt, t)) {
                Type::Signature(id) => tt.signature_def(*id).recv_tparams.clone(),
                _ => Vec::new(),
            }
        };
        for (name, id) in names.iter().zip(ids) {
            let self_ty = self.s.tt.type_param_def(id).self_ty;
            self.env.declare(name, Operand::type_expr(self_ty));
        }
    }

    fn stmts(&mut self, file: FileId, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(file, s);
        }
    }

    fn stmt(&mut self, file: FileId, s: &Stmt) {
        match s {
            Stmt::Empty(_) => {}
            // `L: stmt` —— 标签的合法性在 M2/M3 两趟里判过，这里只走下去
            Stmt::Labeled(l) => self.stmt(file, &l.stmt),
            Stmt::Block(b) => self.block(file, b),
            Stmt::Expr(e) => {
                let x = self.eval(file, &e.x);
                self.expr_stmt(file, Some(&e.x), &x, e.pos);
            }
            Stmt::Send(sd) => self.send(file, sd.chan.as_ref(), sd.value.as_ref(), sd.pos),
            Stmt::Decl(d) => self.decl_stmt(file, d),
            Stmt::Assign(a) => self.assign(file, a),
            Stmt::Branch(_) => {}
            Stmt::Call(c) => self.call_stmt(file, c),
            Stmt::Return(r) => self.return_stmt(file, r),
            Stmt::If(i) => self.if_stmt(file, i),
            Stmt::For(f) => self.for_stmt(file, f),
            Stmt::Switch(sw) => self.switch_stmt(file, sw),
            Stmt::Select(sel) => self.select_stmt(file, sel),
        }
    }

    fn block(&mut self, file: FileId, b: &BlockStmt) {
        self.env.push();
        self.stmts(file, &b.stmts);
        self.env.pop();
    }

    /// 表达式语句：官方 `stmt.go:432` —— 只有**调用**与**接收**能当语句。
    fn expr_stmt(&mut self, file: FileId, x_ast: Option<&Expr>, x: &Operand, pos: Pos) {
        // 接收语句（`<-ch`）合法；调用（`novalue`）合法。
        if let Some(e) = x_ast
            && is_receive(e)
        {
            return;
        }
        // 调用语句：**结果被丢弃也合法**（官方 `stmt.go:439-461`：`rawExpr` 对
        // 函数/方法调用返回 `statement` 这一 `exprKind`，于是 `switch x.mode()`
        // 的 `default` 分支直接 `return`，不看结果个数）。
        //
        // M5/Task 2 修：原先只放行 `Mode::NoValue`（无结果调用），有结果的调用
        // 当语句会被报成 `value of type T is not used` —— `divmod.go:126`、
        // `escape3.go:16`、`ken/rob1.go:64` 三个语料文件共 7 行的形态。
        //
        // ⚠️ **转换不是调用**：`int(x)` 也是 `Expr::Call`，官方对它返回
        // `conversion` 而不是 `statement` ⇒ 仍报 `is not used`。所以这里必须
        // 用**被调方是不是类型表达式**来区分，不能只看语法形状。
        if let Some(e) = x_ast
            && self.is_call_stmt(file, e)
        {
            return;
        }
        match x.mode {
            Mode::NoValue | Mode::Builtin | Mode::TypeExpr | Mode::Invalid => {}
            _ => {
                let what = operand_text(&*self.s.tt, x);
                let at = x_ast.map_or(pos, |e| e.start_pos());
                self.report(at, DiagCode::ChkUnusedExpr, format!("{what} is not used"));
            }
        }
    }

    /// 语法上是调用、且**被调方不是类型表达式** ⇒ 合法的表达式语句。
    ///
    /// 括号可嵌套（官方 `rawExpr` 会穿过 `ParenExpr`），所以递归剥括号。
    fn is_call_stmt(&self, file: FileId, e: &Expr) -> bool {
        match e {
            Expr::Paren(p) => self.is_call_stmt(file, &p.x),
            Expr::Call(c) => !self.is_type_expr_ast(file, &c.fun),
            _ => false,
        }
    }

    /// **只读**判定：这个表达式在类型位置上是类型表达式吗（`T` / `pkg.T` / `[]T` / `*T` / `T[P]` …）。
    ///
    /// 与 [`crate::expr::ExprChecker::expr`] 的区别：这里**不求值、不报错**，
    /// 只查名字的对象种类 —— 因为它在「已经求值过一次」之后被调用（表达式语句
    /// 的结果操作数已经算好），再求值一次会把诊断重复上报。
    fn is_type_expr_ast(&self, file: FileId, e: &Expr) -> bool {
        match e {
            Expr::Paren(p) => self.is_type_expr_ast(file, &p.x),
            // 类型字面量族
            Expr::ArrayType(_)
            | Expr::StructType(_)
            | Expr::FuncType(_)
            | Expr::InterfaceType(_)
            | Expr::MapType(_)
            | Expr::ChanType(_) => true,
            // `*T`（一元 `*`）
            Expr::Unary(u) => u.op == rgoc_ast::Op::Mul && self.is_type_expr_ast(file, &u.x),
            // `T` / `pkg.T` / `T[int]`
            Expr::Name(n) => self.peek_mode(file, &n.value) == Some(Mode::TypeExpr),
            Expr::Selector(s) => match &*s.x {
                Expr::Name(n) => self.peek_mode(file, &n.value) == Some(Mode::TypeExpr),
                _ => false,
            },
            Expr::Index(i) => self.is_type_expr_ast(file, &i.x),
            _ => false,
        }
    }

    /// 名字 → 操作数模式（**只读**；与 [`crate::expr`] 的 `ident` 同一查找序：
    /// 局部作用域 → 文件 → 包 → universe）。
    fn peek_mode(&self, file: FileId, name: &str) -> Option<Mode> {
        if let Some(op) = self.env.lookup(name) {
            return Some(op.mode);
        }
        self.s
            .pkg
            .lookup_in_file(file, name)
            .and_then(Operand::from_object)
            .map(|op| op.mode)
    }

    // ─────────────────────────── 发送 / 局部声明 ───────────────────────────

    fn send(&mut self, file: FileId, ch: &Expr, value: &Expr, arrow: Pos) {
        let c = self.eval(file, ch);
        let mut v = self.eval(file, value);
        if c.mode == Mode::Invalid || v.mode == Mode::Invalid {
            return;
        }
        let Some(ct) = c.typ else { return };
        let elem = {
            let tt = &*self.s.tt;
            match tt.type_of(underlying(tt, ct)) {
                Type::Chan {
                    dir: ChanDir::SendRecv | ChanDir::SendOnly,
                    elem,
                } => Some(*elem),
                _ => None,
            }
        };
        match elem {
            None => {
                let desc = type_str(&*self.s.tt, ct);
                self.report(
                    arrow,
                    DiagCode::ChkInvalidOperation,
                    format!("invalid operation: cannot send to non-channel {desc}"),
                );
            }
            Some(e) => {
                self.assign_to(&mut v, value.pos(), Some(e), "send");
            }
        }
    }

    fn decl_stmt(&mut self, file: FileId, d: &DeclStmt) {
        for decl in &d.decls {
            match decl {
                Decl::Var(v) => {
                    // 显式类型走 `type_expr`（含匿名 `struct{…}` / `func(…)`），
                    // 不走 `eval` —— 后者对类型字面量是 invalid（Task 37）。
                    let declared = match &v.type_ {
                        Some(t) => self
                            .type_expr(file, t)
                            .or_else(|| self.type_of_expr(file, t)),
                        None => None,
                    };
                    // 求值首值 ⇒ 无类型显式语句时用它的（默认）类型
                    let mut val_ty = None;
                    for e in &v.values {
                        let x = self.eval(file, e);
                        if val_ty.is_none() {
                            val_ty = self.default_type(&x);
                        }
                    }
                    let ty = declared.or(val_ty);
                    for n in &v.names {
                        self.env.declare(&n.value, Operand::variable(ty));
                    }
                }
                Decl::Const(c) => {
                    let declared = match &c.type_ {
                        Some(t) => self
                            .type_expr(file, t)
                            .or_else(|| self.type_of_expr(file, t)),
                        None => None,
                    };
                    // 值也要留下（M5/Task 15）：lowering 复查 `A != 0` 时若拿到
                    // `val: None`，常量就折不出来，只能退化成运行期读一个不存在的
                    // 槽（`const8.go` 的局部 `iota` 组实测）。
                    let vals: Vec<Option<rgoc_const::Const>> =
                        c.values.iter().map(|e| self.eval(file, e).val).collect();
                    for (i, n) in c.names.iter().enumerate() {
                        self.env.declare(
                            &n.value,
                            Operand {
                                mode: Mode::Constant,
                                typ: declared,
                                val: vals.get(i).cloned().flatten(),
                                pos: Some(n.pos),
                            },
                        );
                    }
                }
                Decl::Type(t) => {
                    let under = self
                        .type_expr(file, &t.type_)
                        .or_else(|| self.type_of_expr(file, &t.type_));
                    // 局部**具名**类型（`type S struct{…}`）必须建成 named：
                    // 名字参与同一性，`S` 与结构相同的 `T` **不**可赋值
                    // （官方 `typexpr.go` 的 `definedType`）。别名
                    // （`type A = B`）不建 named，直接绑本体。
                    //
                    // **记忆化**（M5/Task 15）：同一份 AST 的同一个 `Decl::Type`
                    // 只许有一个 `TypeId` —— 检查期建、lowering 期读。否则
                    // lowering 按 `expr_in` 复查 `var x S` 时会再建一个 named，
                    // 两个 id 的同一性判断分叉（`range3.go` 的局部 `MyInt`、
                    // `closure2.go` 的局部 `X` 实测）。
                    let key = (t.pos.file, t.pos.line, t.pos.col);
                    let ty = match self.s.local_types.get(&key).copied() {
                        Some(cached) => cached,
                        None => {
                            let ty = match under {
                                Some(u) if !t.alias => {
                                    let n = self.s.tt.add_named(&t.name.value);
                                    self.s.tt.set_named_underlying(n, u);
                                    self.s.tt.named_type(n)
                                }
                                Some(u) => u,
                                None => self.s.tt.invalid(),
                            };
                            self.s.local_types.insert(key, ty);
                            ty
                        }
                    };
                    self.env.declare(&t.name.value, Operand::type_expr(ty));
                }
                // 体内的 `func` 声明（Go 不允许；语法面已报）
                Decl::Func(f) => {
                    self.env.declare(&f.name.value, Operand::value(None));
                    self.func_decl(file, f);
                }
                Decl::Import(_) => {}
            }
        }
    }

    // ─────────────────────────── 赋值 ───────────────────────────

    fn assign(&mut self, file: FileId, a: &AssignStmt) {
        match a.op {
            AssignOp::Inc | AssignOp::Dec => self.inc_dec(file, a),
            AssignOp::Define => self.short_var_decl(file, a),
            AssignOp::Assign => self.assign_vars(file, a),
            _ => self.compound_assign(file, a),
        }
    }

    fn inc_dec(&mut self, file: FileId, a: &AssignStmt) {
        let Some(lhs) = a.lhs.first() else { return };
        let x = self.eval(file, lhs);
        if x.mode == Mode::Invalid {
            return;
        }
        let Some(t) = x.typ else { return };
        if !all_numeric(&*self.s.tt, t) {
            let desc = type_str(&*self.s.tt, t);
            let op = if a.op == AssignOp::Inc { "++" } else { "--" };
            let label = expr_label(lhs).unwrap_or_else(|| "expression".to_owned());
            self.report(
                lhs.start_pos(),
                DiagCode::ChkInvalidOperation,
                format!("invalid operation: {label}{op} (non-numeric type {desc})"),
            );
        }
    }

    /// 复合赋值（`x op= y`）：官方 `stmt.go:518-530` —— 先按**隐式二元运算**
    /// 走 `binary`（`e == nil`），再走 `assignVar`。
    ///
    /// # M5/Task 5 的修正
    ///
    /// 这里是自建近似（「两侧类型匹配 + 谓词 `op_defined`」），与官方两处分歧，
    /// 都由 `ken/shift.go` / 一组探针实测暴露：
    ///
    /// ① **移位**：`i <<= c`（`c` 为 `uint`）被误报
    ///    `mismatched types int and uint`。官方对移位**只要求计数是整数**，
    ///    不要求与左操作数同类型（`F3-D`；`go_spec`「The right operand in a shift
    ///    expression must have integer type or be an untyped constant representable
    ///    by a value of type uint」）。近似实现把移位当普通二元运算判同型 ⇒ 假阳性。
    /// ② **可转换的未类型化常量**：`i += 1.5` 官方报 `convert` @ **右操作数**
    ///    （`matchTypes` → `convertUntyped` 截断），近似报 `op-invalid` @ 左操作数。
    ///
    /// 改走 [`ExprChecker::binary_assign`] 后两处自动对齐 —— 也顺带接上了
    /// 除零、常量折叠、`&^=` 等原先漏掉的面。
    fn compound_assign(&mut self, file: FileId, a: &AssignStmt) {
        let (Some(lhs), Some(rhs)) = (a.lhs.first(), a.rhs.first()) else {
            return;
        };
        let x = self.eval(file, lhs);
        let y = self.eval(file, rhs);
        if x.mode == Mode::Invalid || y.mode == Mode::Invalid {
            return;
        }
        let Some(op) = base_op(a.op) else { return };
        let lx = expr_label(lhs).unwrap_or_else(|| "expression".to_owned());
        let ly = expr_label(rhs).unwrap_or_else(|| "expression".to_owned());
        let xpos = lhs.start_pos();
        let ypos = rhs.start_pos();
        let mut v = {
            let mut cx = ExprChecker {
                s: &mut *self.s,
                env: Some(&self.env),
            };
            let src = BinSrc::Assign {
                lhs: lx,
                rhs: ly,
                sym: compound_sym(a.op),
            };
            let (xa, ya) = (At { op: x, pos: xpos }, At { op: y, pos: ypos });
            cx.binary_core(xa, ya, op, src, xpos)
        };
        if v.mode == Mode::Invalid {
            return;
        }
        // 官方 `assignVar(s.Lhs[0], nil, &x, "assignment")`：可赋值性 + 左值性。
        self.assign_lhs(file, lhs, xpos, &mut v, "assignment");
    }

    /// 纯 `=`（官方 `assignVars`）。
    fn assign_vars(&mut self, file: FileId, a: &AssignStmt) {
        let vals = self.assign_rhs(file, &a.rhs, a.lhs.len());
        let l = a.lhs.len();
        if vals.len() != l {
            self.assign_error(a.pos, &a.rhs, l, vals.len());
            return;
        }
        for (i, lhs) in a.lhs.iter().enumerate() {
            if is_blank(lhs) {
                continue;
            }
            let mut v = vals[i].0.clone();
            if self.assign_lhs(file, lhs, vals[i].1, &mut v, "assignment") {
                continue;
            }
        }
    }

    /// 短变量声明（官方 `shortVarDecl`）。
    fn short_var_decl(&mut self, file: FileId, a: &AssignStmt) {
        // ① 分类：新变量 / 已有 / 非名字（`RepeatedDecl` 已由 M3 报）
        let mut fresh: Vec<(String, bool)> = Vec::new();
        let mut has_err = false;
        for lhs in &a.lhs {
            match lhs {
                Expr::Name(n) => {
                    if n.value == "_" {
                        fresh.push((n.value.clone(), false));
                        continue;
                    }
                    // 「新变量」= **本层**没有（Go 规范：同一块或函数体的参数表里
                    // 没有声明过）。同名重复在 M3 已报 RepeatedDecl，这里不重复判。
                    let is_new = !self.env.has_local(&n.value);
                    fresh.push((n.value.clone(), is_new));
                }
                _ => {
                    let _ = self.eval(file, lhs);
                    self.report(
                        lhs.start_pos(),
                        DiagCode::ChkInvalidAssign,
                        "non-name on left side of :=".to_owned(),
                    );
                    has_err = true;
                }
            }
        }

        // ② 右侧求值（此时新变量还**不在**作用域里）
        let vals = self.assign_rhs(file, &a.rhs, a.lhs.len());
        let l = a.lhs.len();
        if vals.len() != l {
            self.assign_error(a.pos, &a.rhs, l, vals.len());
        } else {
            // ③ 逐个赋值 / 定类型
            for (i, lhs) in a.lhs.iter().enumerate() {
                if is_blank(lhs) {
                    continue;
                }
                let mut v = vals[i].0.clone();
                let is_new = fresh.get(i).is_some_and(|(_, n)| *n);
                // ⚠️ 只有**非新**变量才走 `assignVar`（官方 `initVars`：
                // `if obj != nil { assignVar } else { declare }`）。
                //
                // M5/Task 5 修正：原先无条件 `self.local(…)`，而 `local` 是
                // **沿栈查**（`Env::lookup`）—— 于是内层块里 `p := …` 会取到
                // **外层**同名 `p` 的类型去判可赋值性，误报
                // `cannot use … as … value in assignment`。
                // 语料形态：`convert4.go:73` 的 `if p := (*Int4)(ii); …`，
                // 外层是 `var p *[]byte`（第 61 行）。
                let target = if is_new {
                    None
                } else {
                    self.local(&name_of(lhs)).and_then(|op| op.typ)
                };
                // 已有变量：官方走 `assignVars`（报点在**右值**）；
                // 新变量下面按右侧类型登记。
                if let Some(t) = target {
                    self.assign_to(&mut v, vals[i].1, Some(t), "assignment");
                }
                if let Some((name, true)) = fresh.get(i) {
                    let ty = self.default_type(&vals[i].0);
                    self.env.declare(name, Operand::variable(ty));
                }
            }
        }

        // ④ 一个都没新 ⇒ `no new variables`（官方 softErrorf，位置 = `:=`）
        if fresh.iter().all(|(_, is_new)| !is_new) && !has_err {
            self.s.report_soft(Diag {
                pos: a.pos,
                code: DiagCode::ChkNoNewVar,
                message: "no new variables on left side of :=".to_owned(),
            });
        }
    }

    /// 右侧求值 + 多值展开（官方 `initVars` 的 n:1 面）。
    ///
    /// `want` 是**左侧变量个数**，只用于判断逗号-ok 是否成立；
    /// 传 `0` 表示这一面不允许逗号-ok（`return` 语句：Go 里
    /// `return m[k]` 不能给两个结果值，官方同样只认多值调用）。
    ///
    /// 返回 `(操作数, 位置)` 序列：
    /// * 单值明细逐项求值；
    /// * 唯一一项且是**多值调用** ⇒ 展开成元组的各元素；
    /// * 唯一一项、左侧恰好两个、且操作数是逗号-ok 模式（map 索引 /
    ///   类型断言）⇒ 展开成 `(值, bool)`；
    /// * 其余（含元组出现在多值位置）⇒ 单值语境报错并标 `invalid`。
    fn assign_rhs(&mut self, file: FileId, rhs: &[Expr], want: usize) -> Vec<(Operand, Pos)> {
        if rhs.len() != 1 {
            return rhs
                .iter()
                .map(|e| {
                    let mut x = self.eval(file, e);
                    self.single_value(&mut x, e.start_pos());
                    (x, e.start_pos())
                })
                .collect();
        }
        let e = &rhs[0];
        let x = self.eval(file, e);
        let pos = e.start_pos();
        if let Some(elems) = tuple_elems(&*self.s.tt, &x) {
            return elems
                .into_iter()
                .map(|t| (Operand::value(Some(t)), pos))
                .collect();
        }
        // 逗号-ok：`v, ok := m[k]` / `v, ok := i.(T)`。
        //
        // 官方在 `initVars`/`assignVars` 里对「右侧一项、左侧两项」单开一支，
        // 按模式（`mapindex` / `commaok`）补一个 `bool`。这里同形：第二个
        // 值的类型固定是 `bool`，第一个值是操作数自己的类型。
        if want == 2
            && matches!(x.mode, Mode::MapIndex | Mode::CommaOk | Mode::CommaErr)
            && let Some(t) = x.typ
        {
            let ok = Operand::value(Some(self.bool_type()));
            return vec![(Operand::value(Some(t)), pos), (ok, pos)];
        }
        vec![(x, pos)]
    }

    /// 预声明 `bool` 的类型 id。
    fn bool_type(&mut self) -> TypeId {
        self.s.tt.add_basic(rgoc_const::BasicId::Bool)
    }

    /// 官方 `assignError`：报点 = **右侧第一个**表达式。
    ///
    /// `fallback` 是语句本身的位置 —— 官方在这一支里直接 `rhs[0].Pos()`（语法器
    /// 保证右侧非空），我们的 AST 允许空表（语法器已报错），所以退到语句位。
    fn assign_error(&mut self, fallback: Pos, rhs: &[Expr], l: usize, r: usize) {
        let at = rhs.first().map_or(fallback, |e| e.start_pos());
        let vars = measure(l, "variable");
        let vals = measure(r, "value");
        let msg = match rhs {
            [e] if matches!(e, Expr::Call(_)) => {
                let fun = match e {
                    Expr::Call(c) => expr_label(&c.fun).unwrap_or_else(|| "call".to_owned()),
                    _ => unreachable!(),
                };
                format!("assignment mismatch: {vars} but {fun} returns {vals}")
            }
            _ => format!("assignment mismatch: {vars} but {vals}"),
        };
        self.report(at, DiagCode::ChkInvalidAssign, msg);
    }

    /// 求值一个左值并做可赋值性判定（官方 `assignVar`）。
    ///
    /// `vpos` 是**右值**的位置 —— 官方 `assignment(&z, T, "assignment")` 里的
    /// `z` 是右值操作数，报点取 `z.pos`（`x = s` 报在 `s` 上，不是 `x` 上）。
    /// 左值本身的错（`cannot assign to`）仍报在左值位。
    ///
    /// 返回 `true` ⇒ 已报错（调用方不必再判）。
    fn assign_lhs(
        &mut self,
        file: FileId,
        lhs: &Expr,
        vpos: Pos,
        v: &mut Operand,
        ctx: &str,
    ) -> bool {
        self.assign_lhs_impl(file, lhs, vpos, v, ctx, false)
    }

    /// 同 [`Self::assign_lhs`]，但末端走 [`Self::range_assign`] —— `range` 的
    /// 迭代变量位要**额外**判未类型化常量的可表示性（`range.go:171` 的
    /// `assignVar` 走的是完整 `assignment`，不只是可赋值性）。
    fn assign_lhs_range(
        &mut self,
        file: FileId,
        lhs: &Expr,
        vpos: Pos,
        v: &mut Operand,
        ctx: &str,
    ) -> bool {
        self.assign_lhs_impl(file, lhs, vpos, v, ctx, true)
    }

    /// [`Self::assign_lhs`] / [`Self::assign_lhs_range`] 的公共体。
    ///
    /// `range_ctx` 只影响末端那一次判定（见两个包装的文档）。
    fn assign_lhs_impl(
        &mut self,
        file: FileId,
        lhs: &Expr,
        vpos: Pos,
        v: &mut Operand,
        ctx: &str,
        range_ctx: bool,
    ) -> bool {
        let x = self.eval(file, lhs);
        if x.mode == Mode::Invalid {
            return true;
        }
        let addressable = matches!(x.mode, Mode::Variable | Mode::MapIndex);
        if !addressable {
            let label = expr_label(lhs).unwrap_or_else(|| "expression".to_owned());
            self.report(
                lhs.start_pos(),
                DiagCode::ChkInvalidAssign,
                format!(
                    "cannot assign to {label} (neither addressable nor a map index expression)"
                ),
            );
            return true;
        }
        let t = x.typ;
        let ok = if range_ctx {
            self.range_assign(v, vpos, t, ctx)
        } else {
            self.assign_to(v, vpos, t, ctx)
        };
        if !ok && x.mode != Mode::Variable {
            return true;
        }
        false
    }

    /// 多值操作数落在单值语境（官方 `singleValue`）。
    fn single_value(&mut self, x: &mut Operand, pos: Pos) {
        if let Some(elems) = tuple_elems(&*self.s.tt, x) {
            let n = elems.len();
            let what = operand_text(&*self.s.tt, x);
            self.report(
                pos,
                DiagCode::ChkSingleValueContext,
                format!("multiple-value {what} ({n}-valued) in single-value context"),
            );
            x.mode = Mode::Invalid;
        }
    }

    // ─────────────────────────── 调用语句 / 返回 ───────────────────────────

    fn call_stmt(&mut self, file: FileId, c: &CallStmt) {
        let keyword = match c.tok {
            CallTok::Go => "go",
            CallTok::Defer => "defer",
        };
        let at = c.call.start_pos();
        let Expr::Call(call) = &c.call else {
            self.report(
                at,
                DiagCode::ChkInvalidGoDefer,
                format!("expression in {keyword} must be function call"),
            );
            return;
        };
        let f = self.eval(file, &call.fun);
        if f.mode == Mode::TypeExpr {
            let _ = self.eval(file, &c.call);
            let what = operand_text(&*self.s.tt, &f);
            self.report(
                at,
                DiagCode::ChkInvalidGoDefer,
                format!("{keyword} requires function call, not conversion ({what})"),
            );
            return;
        }
        let x = self.eval(file, &c.call);
        if matches!(x.mode, Mode::NoValue | Mode::Invalid) {
            return;
        }
        let what = operand_text(&*self.s.tt, &x);
        self.report(
            at,
            DiagCode::ChkInvalidGoDefer,
            format!("{keyword} discards result of {what}"),
        );
    }

    fn return_stmt(&mut self, file: FileId, r: &ReturnStmt) {
        let n = self.results.len();
        let named = self.results.first().is_some_and(|(name, _)| name.is_some());

        // 裸 `return`：有命名结果 ⇒ 合法；否则报「不够」
        if r.results.is_empty() {
            if n > 0 && !named {
                let want = results_text(&*self.s.tt, &self.results);
                self.report(
                    r.pos,
                    DiagCode::ChkWrongResultCount,
                    format!("not enough return values\n\thave ()\n\twant {want}"),
                );
            }
            return;
        }

        let vals = self.assign_rhs(file, &r.results, 0);
        if vals.len() != n {
            let (at, qualifier) = if vals.len() > n {
                (vals[n].1, "too many")
            } else {
                (vals.last().map_or(r.pos, |v| v.1), "not enough")
            };
            let have: Vec<String> = vals
                .iter()
                .map(|(x, _)| operand_type_text(&*self.s.tt, x))
                .collect();
            let want = results_text(&*self.s.tt, &self.results);
            self.report(
                at,
                DiagCode::ChkWrongResultCount,
                format!(
                    "{qualifier} return values\n\thave ({})\n\twant {want}",
                    have.join(", ")
                ),
            );
            return;
        }
        for (i, (mut v, pos)) in vals.into_iter().enumerate() {
            let t = self.results[i].1;
            self.assign_to(&mut v, pos, Some(t), "return statement");
        }
    }

    // ─────────────────────────── 控制流 ───────────────────────────

    fn if_stmt(&mut self, file: FileId, i: &IfStmt) {
        self.env.push();
        if let Some(init) = &i.init {
            self.simple(file, init);
        }
        if let Some(c) = &i.cond {
            let x = self.eval(file, c);
            self.cond_check(&x, c.start_pos(), "if");
        }
        self.block(file, &i.then);
        match i.els.as_deref() {
            None => {}
            Some(Stmt::If(nested)) => self.if_stmt(file, nested),
            Some(other) => {
                self.env.push();
                self.stmt(file, other);
                self.env.pop();
            }
        }
        self.env.pop();
    }

    fn cond_check(&mut self, x: &Operand, pos: Pos, keyword: &str) {
        if x.mode == Mode::Invalid {
            return;
        }
        let Some(t) = x.typ else { return };
        if !all_boolean(&*self.s.tt, t) {
            self.report(
                pos,
                DiagCode::ChkInvalidCond,
                format!("non-boolean condition in {keyword} statement"),
            );
        }
    }

    fn for_stmt(&mut self, file: FileId, f: &ForStmt) {
        self.env.push();
        match &f.init {
            Some(SimpleStmt::Range(r)) => self.range_clause(file, r),
            other => {
                if let Some(init) = other {
                    self.simple(file, init);
                }
                if let Some(c) = &f.cond {
                    let x = self.eval(file, c);
                    self.cond_check(&x, c.start_pos(), "for");
                }
                if let Some(post) = &f.post {
                    self.simple(file, post);
                }
            }
        }
        self.block(file, &f.body);
        self.env.pop();
    }

    /// `range` 子句（官方 `rangeStmt`，`range.go:26-197`；M4/Task 26）。
    ///
    /// 四段与官方逐条对应：
    /// 1. **求值**被 range 的表达式（`range.go:38`）；
    /// 2. **key/value 面** [`Self::range_key_val`]（官方 `rangeKeyVal`）——
    ///    不可 range ⇒ `cannot range over …`；版本门控 ⇒ `version` 类别；
    /// 3. **迭代变量个数**（官方 `range.go:76-82` 三个 `softErrorf`）；
    /// 4. **声明 / 赋值**（官方 `range.go:100-194`）：`:=` 走 `initVar`，
    ///    `=` 走 `assignVar`；整型 range 另有「迭代变量必须是整数」的复查
    ///    （`go.dev/issues/67027`）。
    ///
    /// ⚠️ 官方在此**开 range 作用域**（`range.go:88`，在子句**之后**）。
    /// 我们的 `for_stmt` 已在子句前开层 —— 两者对查找的差别只体现在
    /// 「子句里声明的名字在子句内部可见」，而声明发生在求值之后，
    /// 观察等价（`for i := range i` 的 `i` 仍解析到外层）。
    fn range_clause(&mut self, file: FileId, r: &RangeClause) {
        // ── ① 求值（官方 `check.expr(nil, &x, rangeVar)`）──────────────
        let x = self.eval(file, &r.x);
        // 报点 = 操作数**最左 token**（官方 `operand.pos` = `ast.Expr.Pos()`）——
        // 走 `version::pos_of_range_operand`，与门控位置同一个函数，免得两处漂移。
        let x_pos = version::pos_of_range_operand(r);
        let known = x.mode != Mode::Invalid && x.typ.is_some();

        // ── ② key/value 面 + ③ 迭代变量个数 ───────────────────────────
        let mut key: Option<TypeId> = None;
        let mut val: Option<TypeId> = None;
        if known {
            let t = x.typ.unwrap_or_else(|| self.s.tt.invalid());
            match self.range_key_val(file, t) {
                RangeKV::Ok { key: k, val: v } => {
                    key = k;
                    val = v;
                    // 官方 switch 的后三支（`range.go:76-82`）—— 互斥。
                    let what = operand_text(&*self.s.tt, &x);
                    if key.is_none() && !r.lhs.is_empty() {
                        self.report_soft_at(
                            r.lhs[0].start_pos(),
                            DiagCode::ChkInvalidIterVar,
                            format!("range over {what} permits no iteration variables"),
                        );
                    } else if val.is_none() && r.lhs.len() >= 2 {
                        self.report_soft_at(
                            r.lhs[1].start_pos(),
                            DiagCode::ChkInvalidIterVar,
                            format!("range over {what} permits only one iteration variable"),
                        );
                    } else if r.lhs.len() > 2 {
                        self.report_soft_at(
                            r.lhs[2].start_pos(),
                            DiagCode::ChkInvalidIterVar,
                            "range clause permits at most two iteration variables".to_owned(),
                        );
                    }
                }
                // 版本门控：官方把它并进 `cannot range over x: requires go1.N
                // or later`（`range.go:72`），而 C8 适配表里 `requires go1\.`
                // 是**最高优先**规则 ⇒ 类别 `version`（`go1_22_21.go` 实测
                // `14:16 version`）。所以这里走 version 码，不报 range 码。
                RangeKV::Gated(feat) => {
                    let d =
                        version::verify_version_soft(&self.s.pkg.versions, file, x_pos, feat, "");
                    if let Some(d) = d {
                        self.s.report(d);
                    }
                }
                RangeKV::Bad(cause) => {
                    let what = operand_text(&*self.s.tt, &x);
                    let message = match cause {
                        Some(c) => format!("cannot range over {what}: {c}"),
                        None => format!("cannot range over {what}"),
                    };
                    self.report_soft_at(x_pos, DiagCode::ChkInvalidRangeExpr, message);
                }
            }
        }

        // 官方 `rangeOverInt := isInteger(x.typ())`
        let over_int = known && x.typ.is_some_and(|t| all_integer(&*self.s.tt, t));

        // ── ④ 声明 / 赋值 ────────────────────────────────────────────
        if r.is_define {
            // 短变量声明（`range.go:100-155`）。⚠️ 官方**总是**声明：
            // 已有同名变量不影响（range 作用域是新的）；只有「一个非 `_`
            // 名字都没有」才报 `no new variables`。
            let mut vars: Vec<(String, Option<TypeId>)> = Vec::new();
            for (i, lhs) in r.lhs.iter().enumerate() {
                let ty = if over_int {
                    // 官方 `initVar(obj, &x, "range clause")` ⇒ 默认型
                    if i == 0 { self.default_type(&x) } else { None }
                } else if i == 0 {
                    key
                } else {
                    val
                };
                let ty = ty.filter(|t| !self.is_invalid_type(*t));
                match lhs {
                    Expr::Name(n) => {
                        if over_int && i == 0 {
                            // 可表示性：`for i := range maxUint` ⇒
                            // `cannot use maxUint … as int value in range clause`
                            let mut v = x.clone();
                            self.range_assign(&mut v, x_pos, ty, "range clause");
                        }
                        if n.value != "_" {
                            vars.push((n.value.clone(), ty));
                        }
                    }
                    other => {
                        let _ = self.eval(file, other);
                        let label = expr_label(other).unwrap_or_else(|| "expression".to_owned());
                        self.report(
                            other.start_pos(),
                            DiagCode::ChkInvalidAssign,
                            format!("cannot declare {label}"),
                        );
                    }
                }
            }
            if vars.is_empty() {
                self.report(
                    r.pos,
                    DiagCode::ChkNoNewVar,
                    "no new variables on left side of :=".to_owned(),
                );
            } else {
                for (name, ty) in vars {
                    self.env.declare(&name, Operand::variable(ty));
                }
            }
        } else if !r.lhs.is_empty() {
            // 普通赋值（`range.go:156-185`）
            for (i, lhs) in r.lhs.iter().enumerate() {
                if is_blank(lhs) {
                    continue;
                }
                if over_int {
                    // 官方只可能有一个迭代变量（多余的那个已报 `permits only one`）
                    if i != 0 {
                        continue;
                    }
                    let mut v = x.clone();
                    if !self.assign_lhs_range(file, lhs, x_pos, &mut v, "range clause") {
                        // 赋值成功后 x 的类型被改成左值类型，必须仍是整数
                        // （`go.dev/issues/67027`：`var f float64; f = range 10`）
                        if let Some(lt) = self.eval(file, lhs).typ
                            && !all_integer(&*self.s.tt, lt)
                        {
                            let desc = type_str(&*self.s.tt, lt);
                            self.report_soft_at(
                                lhs.start_pos(),
                                DiagCode::ChkInvalidRangeExpr,
                                format!("cannot use iteration variable of type {desc}"),
                            );
                        }
                    }
                } else {
                    let ty = if i == 0 { key } else { val };
                    let Some(ty) = ty.filter(|t| !self.is_invalid_type(*t)) else {
                        continue;
                    };
                    let mut y = Operand::value(Some(ty));
                    let at = lhs.start_pos();
                    self.assign_lhs(file, lhs, at, &mut y, "assignment");
                }
            }
        } else if over_int {
            // 没有迭代变量时仍要判 x（官方 `range.go:186-194`）：
            // `for range maxUint` ⇒ `as int value in range clause (overflows)`
            let mut v = x.clone();
            let target = self.default_type(&v);
            self.range_assign(&mut v, x_pos, target, "range clause");
        }
    }

    /// 官方 `rangeKeyVal`（`range.go:205-287`）：被 range 表达式的 key/value 类型。
    ///
    /// 三种结局分开编码是**刻意的**：`Gated` 与 `Bad` 的**类别不同**
    /// （`version` vs `range`），混成一个 `Option` 就分不出该报哪个码。
    ///
    /// 窄面（台账登记）：官方用 `commonUnder` 合并**类型参数**的项集
    /// （`~int` 一族）；`Type::TypeParam` 在 M3 是「表示有位置、判定不做」
    /// （`D-M3-2`）⇒ 这里按 `underlying` 单跳，泛型实参面不判（不报假错）。
    fn range_key_val(&mut self, file: FileId, t: TypeId) -> RangeKV {
        // `int` / `rune` 的号与两个门控判定都要在**取不可变借用之前**算好：
        // `add_basic` 要 `&mut`，而下面的 match 一直持有 `&*self.s.tt`。
        // `rune` 在本仓的 universe 里就是 `BasicId::Int32`（官方
        // `universeRune = Universe.Lookup("rune").Type()`，同为 int32 别名，
        // 只是名字叫 rune —— 消息面差异，类别/位置不受影响）。
        let int_ty = self.s.tt.add_basic(BasicId::Int);
        let rune_ty = self.s.tt.add_basic(BasicId::Int32);
        let gated_int = !version::allow(&self.s.pkg.versions, file, Feat::RangeOverInt);
        let gated_func = !version::allow(&self.s.pkg.versions, file, Feat::RangeOverFunc);
        if matches!(self.s.tt.type_of(t), Type::Invalid) {
            return RangeKV::Bad(None);
        }
        // ⚠️ 官方 `rangeKeyVal`（`range.go:212`）第一句是 **`commonUnder(orig, …)`**，
        // 不是 `under(orig)`（`M12/Task 10`）。`under(TypeParam)` 在官方返回类型参数
        // **自己**（`(*TypeParam).Underlying()` 就是它自己）⇒ 直接 `switch` 会落
        // default ⇒ 把 `for k, v := range m`（`m` 是 `~map[K]V`）误报成
        // `cannot range over value of type type parameter`（`issue48453.go`）。
        //
        // `common_under` 的「共同」语义正好是官方那一条：类型集没有共同底层
        // （`~int | ~string`）⇒ `None` ⇒ 官方同样 `bad`。非类型参数走
        // `underlying` 一支，与改之前逐字等价。
        let Some(u) = common_under(&mut *self.s.tt, t) else {
            return RangeKV::Bad(None);
        };
        let tt = &*self.s.tt;

        // 只写通道不能 range（官方 `receive from send-only channel %s`）
        if let Type::Chan {
            dir: ChanDir::SendOnly,
            ..
        } = tt.type_of(u)
        {
            return RangeKV::Bad(Some(format!(
                "receive from send-only channel {}",
                type_str(tt, t)
            )));
        }

        // `arrayPtrDeref`：`*[N]T` 当数组（`range.go:221`）
        let r = array_ptr_deref(tt, u);

        match tt.type_of(r) {
            Type::Basic(b) => {
                if b.is_string() {
                    return RangeKV::Ok {
                        key: Some(int_ty),
                        val: Some(rune_ty),
                    };
                }
                if b.is_integer() {
                    // 1.22 门控（`range.go:227`）
                    if gated_int {
                        return RangeKV::Gated(Feat::RangeOverInt);
                    }
                    // 键类型 = **原类型**（`for i := range MyInt(10)` ⇒ `MyInt`）
                    return RangeKV::Ok {
                        key: Some(t),
                        val: None,
                    };
                }
                RangeKV::Bad(None)
            }
            Type::Array { elem, .. } => RangeKV::Ok {
                key: Some(int_ty),
                val: Some(*elem),
            },
            Type::Slice(elem) => RangeKV::Ok {
                key: Some(int_ty),
                val: Some(*elem),
            },
            Type::Map { key, elem } => RangeKV::Ok {
                key: Some(*key),
                val: Some(*elem),
            },
            Type::Chan { dir, elem } => {
                debug_assert!(*dir != ChanDir::SendOnly);
                RangeKV::Ok {
                    key: Some(*elem),
                    val: None,
                }
            }
            Type::Signature(_) => {
                // 1.23 门控（`range.go:242`）
                if gated_func {
                    return RangeKV::Gated(Feat::RangeOverFunc);
                }
                self.range_func_key_val(r)
            }
            _ => RangeKV::Bad(None),
        }
    }

    /// `range over func` 的迭代器形态判据（官方 `range.go:245-284`）。
    ///
    /// 合法形态 = `func(yield func(K, V) bool)`：一参、无结果、参数是签名、
    /// 该签名 ≤2 参、恰一个 `bool` 结果、非变参。
    fn range_func_key_val(&mut self, t: TypeId) -> RangeKV {
        const PFX: &str = "func must be func(yield func(...) bool)";
        let bool_ty = self.s.tt.add_basic(BasicId::Bool);
        let tt = &*self.s.tt;
        let Type::Signature(sid) = tt.type_of(t) else {
            return RangeKV::Bad(None);
        };
        let sig = tt.signature_def(*sid).clone();
        let params = tuple_vars(tt, sig.params);
        let results = tuple_vars(tt, sig.results);
        if params.len() != 1 {
            return RangeKV::Bad(Some(format!("{PFX}: wrong argument count")));
        }
        if !results.is_empty() {
            return RangeKV::Bad(Some(format!("{PFX}: unexpected results")));
        }
        let yield_ty = params[0].1;
        let Type::Signature(yid) = tt.type_of(underlying(tt, yield_ty)) else {
            return RangeKV::Bad(Some(format!("{PFX}: argument is not func")));
        };
        let y = tt.signature_def(*yid).clone();
        let yp = tuple_vars(tt, y.params);
        let yr = tuple_vars(tt, y.results);
        if yp.len() > 2 {
            return RangeKV::Bad(Some(format!("{PFX}: yield func has too many parameters")));
        }
        // 官方 `Identical(cb.Results().At(0).Type(), universeBool)` ——
        // **必须**是预声明的 `bool`，用户定义的 `type MyBool bool` 不算。
        let one_bool = yr.len() == 1 && rgoc_types::identical(tt, yr[0].1, bool_ty).unwrap_or(true);
        if !one_bool {
            if yr.len() == 1 && all_boolean(&*self.s.tt, yr[0].1) {
                return RangeKV::Bad(Some(format!(
                    "{PFX}: yield func returns user-defined boolean, not bool"
                )));
            }
            return RangeKV::Bad(Some(format!("{PFX}: yield func does not return bool")));
        }
        if y.variadic {
            return RangeKV::Bad(Some(format!(
                "yield func of type {} cannot be variadic",
                type_str(tt, yield_ty)
            )));
        }
        RangeKV::Ok {
            key: yp.first().map(|(_, t)| *t),
            val: yp.get(1).map(|(_, t)| *t),
        }
    }

    /// 该 `TypeId` 是不是 `invalid`（官方 `Typ[Invalid]`）。
    fn is_invalid_type(&self, t: TypeId) -> bool {
        matches!(self.s.tt.type_of(t), Type::Invalid)
    }

    fn report_soft_at(&mut self, pos: Pos, code: DiagCode, message: String) {
        self.s.report_soft(Diag { pos, code, message });
    }

    /// 官方 `assignment`（`assignments.go:24-118`）里**未类型化常量**的那一段
    /// （`implicitTypeAndValue`，`assignments.go:70-84`）。
    ///
    /// Task 25 的 [`Self::assign_to`] 只判**可赋值性**；`untyped int → uint8`
    /// 在类型层面是可赋值的，**可表示性**要单独判 —— 缺了这一段，
    /// `var u8 uint8; for u8 = range 256` 会漏报（ocracle 实测 `85:16 assign`）。
    ///
    /// 报点与后缀照官方：`cannot use {x} as {T} value in {context} (overflows)`；
    /// 类别仍走 `assign`（C8 适配表 `^cannot use .+ as .+` 在 `overflow` 规则之前）。
    fn range_assign(
        &mut self,
        v: &mut Operand,
        vpos: Pos,
        target: Option<TypeId>,
        ctx: &str,
    ) -> bool {
        if v.mode == Mode::Invalid {
            return false;
        }
        let Some(t) = target else {
            return true;
        };
        if let Some(vt) = v.typ
            && is_untyped(&*self.s.tt, vt)
            && let Some(val) = &v.val
            && let Some(to) = basic_kind(&*self.s.tt, t)
            && let Err(code) = representable(val, to, &Sizes::HOST_ARM64)
        {
            let suffix = if code == DiagCode::TypTruncatedFloat {
                "(truncated)"
            } else {
                "(overflows)"
            };
            let desc = type_str(&*self.s.tt, t);
            let what = operand_text(&*self.s.tt, v);
            self.report(
                vpos,
                DiagCode::ChkInvalidAssign,
                format!("cannot use {what} as {desc} value in {ctx} {suffix}"),
            );
            v.mode = Mode::Invalid;
            return false;
        }
        self.assign_to(v, vpos, target, ctx)
    }

    fn switch_stmt(&mut self, file: FileId, sw: &SwitchStmt) {
        self.env.push();
        if let Some(init) = &sw.init {
            self.simple(file, init);
        }

        // 类型 switch 走单独一条（官方 `case *ast.TypeSwitchStmt`）
        if let Some(Expr::TypeSwitchGuard(g)) = &sw.tag {
            self.type_switch(file, sw, g);
            self.env.pop();
            return;
        }

        // tag：缺省 ⇒ `true`（无类型 bool）
        let tag = match &sw.tag {
            Some(t) => {
                let x = self.eval(file, t);
                Some((x, expr_label(t)))
            }
            None => None,
        };
        self.multiple_defaults_switch(&sw.cases);

        let mut seen: Vec<(rgoc_const::Const, TypeId, Pos)> = Vec::new();
        for c in &sw.cases {
            let mut cvals: Vec<&Expr> = Vec::new();
            if let Some(vals) = &c.cases {
                match vals {
                    Expr::List(l) => cvals.extend(l.elems.iter()),
                    other => cvals.push(other),
                }
            }
            for v in cvals {
                let x = self.eval(file, v);
                let Some(tag_op) = tag.as_ref().and_then(|(x, _)| x.typ) else {
                    continue;
                };
                if x.mode == Mode::Invalid {
                    continue;
                }
                let Some(tv) = x.typ else { continue };

                // 先按 tag 类型换算无类型常量（官方 `convertUntyped`）
                let mut eff = tv;
                if rgoc_types::assignable(&*self.s.tt, tv, tag_op).unwrap_or(true) {
                    if basic_kind(&*self.s.tt, tv).is_some_and(|b| b.is_untyped()) {
                        eff = tag_op;
                    }
                } else {
                    let what = operand_text(&*self.s.tt, &x);
                    let tag_label = tag
                        .as_ref()
                        .and_then(|(_, l)| l.clone())
                        .unwrap_or_else(|| "switch".to_owned());
                    let st = type_str(&*self.s.tt, tag_op);
                    let vt = type_str(&*self.s.tt, tv);
                    self.report(
                        v.start_pos(),
                        DiagCode::ChkInvalidCase,
                        format!(
                            "invalid case {what} in switch on {tag_label} (mismatched types {vt} and {st})"
                        ),
                    );
                    continue;
                }

                // 重复 case（仅常量；官方 `goVal` 面）
                if x.mode == Mode::Constant
                    && let Some(val) = x.val.clone()
                {
                    let dup = seen.iter().any(|(cv, ct, _)| {
                        *cv == val && rgoc_types::identical(&*self.s.tt, *ct, eff).unwrap_or(false)
                    });
                    if dup {
                        let what = operand_text(&*self.s.tt, &x);
                        let st = type_str(&*self.s.tt, eff);
                        self.report(
                            v.start_pos(),
                            DiagCode::ChkInvalidCase,
                            format!(
                                "duplicate case {what} (constant of type {st}) in expression switch"
                            ),
                        );
                        continue;
                    }
                    seen.push((val, eff, v.start_pos()));
                }
            }
            self.env.push();
            self.stmts(file, &c.body);
            self.env.pop();
        }
        self.env.pop();
    }

    /// 类型 switch（官方 `case *ast.TypeSwitchStmt`）。
    fn type_switch(&mut self, file: FileId, sw: &SwitchStmt, g: &rgoc_ast::TypeSwitchGuard) {
        let sx = self.eval(file, &g.x);
        if sx.mode != Mode::Invalid
            && let Some(t) = sx.typ
            && !is_non_typeparam_interface(&*self.s.tt, t)
        {
            let what = operand_text(&*self.s.tt, &sx);
            self.report(
                g.x.start_pos(),
                DiagCode::ChkInvalidOperation,
                format!("{what} is not an interface"),
            );
        }
        self.multiple_defaults_switch(&sw.cases);

        for c in &sw.cases {
            let mut cvals: Vec<&Expr> = Vec::new();
            if let Some(vals) = &c.cases {
                match vals {
                    Expr::List(l) => cvals.extend(l.elems.iter()),
                    other => cvals.push(other),
                }
            }
            for v in cvals {
                if matches!(v, Expr::Name(n) if n.value == "nil") {
                    let _ = self.eval(file, v);
                    continue;
                }
                let x = self.eval(file, v);
                if x.mode == Mode::TypeExpr || x.mode == Mode::Invalid {
                    continue;
                }
                let what = operand_text(&*self.s.tt, &x);
                self.report(
                    v.start_pos(),
                    DiagCode::ChkNotAType,
                    format!("{what} is not a type"),
                );
            }
            self.env.push();
            // 隐式变量：每个 case 一份（类型面归 Task 28）
            if let Some(n) = &g.lhs {
                self.env.declare(&n.value, Operand::variable(None));
            }
            self.stmts(file, &c.body);
            self.env.pop();
        }
    }

    /// 官方 `multipleDefaults`：第二个 `default` 报错（报在**该子句**位）。
    fn multiple_defaults_switch(&mut self, cases: &[CaseClause]) {
        let mut first: Option<Pos> = None;
        for c in cases {
            if c.cases.is_some() {
                continue;
            }
            match first {
                None => first = Some(c.pos),
                Some(p) => self.report(
                    c.pos,
                    DiagCode::ChkInvalidCase,
                    format!("multiple defaults (first at {}:{})", p.line, p.col),
                ),
            }
        }
    }

    fn select_stmt(&mut self, file: FileId, sel: &SelectStmt) {
        let mut first_default: Option<Pos> = None;
        for c in &sel.clauses {
            if c.comm.is_none() {
                match first_default {
                    None => first_default = Some(c.pos),
                    Some(p) => self.report(
                        c.pos,
                        DiagCode::ChkInvalidSelectCase,
                        format!("multiple defaults (first at {}:{})", p.line, p.col),
                    ),
                }
                self.env.push();
                self.stmts(file, &c.body);
                self.env.pop();
                continue;
            }
            let comm = c.comm.as_ref().expect("已判非 default");
            // 子句的作用域必须在**通信语句之前**就压上：`case v := <-a:` 的
            // `v` 声明在这个子句里（官方 `selectStmt` 对每个 comm clause
            // `check.scope` 包一层）。反过来做会得到「no new variables on
            // left side of :=」—— 因为 `v` 落到了外层作用域，而外层已经有
            // 一个同名变量。
            self.env.push();
            if !valid_comm(comm) {
                self.report(
                    simple_pos(comm).unwrap_or(c.pos),
                    DiagCode::ChkInvalidSelectCase,
                    "select case must be send or receive (possibly with assignment)".to_owned(),
                );
            } else {
                self.simple(file, comm);
            }
            self.stmts(file, &c.body);
            self.env.pop();
        }
    }

    fn simple(&mut self, file: FileId, s: &SimpleStmt) {
        match s {
            SimpleStmt::Empty => {}
            SimpleStmt::Expr(e) => {
                let x = self.eval(file, &e.x);
                self.expr_stmt(file, Some(&e.x), &x, e.pos);
            }
            SimpleStmt::Send(sd) => self.send(file, sd.chan.as_ref(), sd.value.as_ref(), sd.pos),
            SimpleStmt::Assign(a) => self.assign(file, a),
            SimpleStmt::Range(r) => self.range_clause(file, r),
        }
    }
}

// ═══════════════════════════ 纯函数 ═══════════════════════════

/// 元组的逐项表：`(名字, 类型)`（官方 `Tuple.vars`）。
type TupleVars = Vec<(Option<String>, TypeId)>;

/// `range` 的 key/value 判定（官方 `rangeKeyVal` 的三档结局；M4/Task 26）。
///
/// 为什么把「门控拒绝」单列一档：官方的门控拒绝也写成 `cannot range over x:
/// requires go1.N or later`，但 C8 适配表把带 `requires go1\.` 的消息**最高优先**
/// 归到 `version`（`go1_22_21.go` 实测 `14:16 version`），而普通拒绝归 `range`。
/// 合成一个 `Bad` 就没法在调用面选对码。
enum RangeKV {
    /// 可 range；`None` ⇒ 该位置没有迭代变量。
    Ok {
        /// 键类型（官方 `key`）。
        key: Option<TypeId>,
        /// 值类型（官方 `val`）。
        val: Option<TypeId>,
    },
    /// 版本门控拒绝（整型 1.22 / func 1.23）—— 报 `version` 类别。
    Gated(Feat),
    /// 不可 range；`Some(cause)` ⇒ 官方冒号后的原因。
    Bad(Option<String>),
}

/// 官方 `arrayPtrDeref`：`*[N]T` 在 range 面按 `[N]T` 处理（`range.go:221`）。
fn array_ptr_deref(tt: &rgoc_types::TypeTable, t: TypeId) -> TypeId {
    if let Type::Pointer(p) = tt.type_of(t) {
        let u = rgoc_types::underlying(tt, *p);
        if matches!(tt.type_of(u), Type::Array { .. }) {
            return u;
        }
    }
    t
}

/// 元组元素的类型表（非元组 ⇒ `None`）。
///
/// 官方把多值结果放在 `operand.typ` 的 `*Tuple` 里；我们也一样 ——
/// 这是「单值语境」判据（`singleValue`）与多值展开的唯一来源。
fn tuple_elems(tt: &rgoc_types::TypeTable, x: &Operand) -> Option<Vec<TypeId>> {
    let t = x.typ?;
    let t = rgoc_types::unalias(tt, t);
    match tt.type_of(t) {
        Type::Tuple(id) => Some(tt.tuple_def(*id).vars.iter().map(|(_, t)| *t).collect()),
        _ => None,
    }
}

/// 接收者类型表达式的**基类型名**（`T` / `*T` / `T[P]` / `(T)`）。
fn recv_base_name(e: &Expr) -> Option<&str> {
    match e {
        Expr::Name(n) => Some(&n.value),
        Expr::Paren(p) => recv_base_name(&p.x),
        Expr::Unary(u) if u.op == rgoc_ast::Op::Mul => recv_base_name(&u.x),
        Expr::Index(i) => recv_base_name(&i.x),
        _ => None,
    }
}

/// 接收者写出的**类型参数名**（`func (s *Stream[T, U]) M()` ⇒ `["T", "U"]`）。
///
/// 形状由 Go 语法保证：`"(" [标识符] ["*"] Base [ "[" 标识符表 "]" ] ")"`。
/// 剥掉括号与 `*` 之后不是「基名 + 纯标识符表」⇒ **空表**（`Stream[[]int]`
/// 这种不是接收者声明的类型参数；装载面的 `recv_type_args` 同样返回 `None`，
/// 于是 `recv_tparams` 也是空的 —— 两侧同判，不会错位）。
fn recv_tparam_names(e: &Expr) -> Vec<String> {
    let mut cur = e;
    loop {
        cur = match cur {
            Expr::Paren(p) => &p.x,
            Expr::Unary(u) if u.op == rgoc_ast::Op::Mul && u.y.is_none() => &u.x,
            _ => break,
        };
    }
    let Expr::Index(ix) = cur else {
        return Vec::new();
    };
    match &*ix.index {
        Expr::Name(n) => vec![n.value.clone()],
        Expr::List(l) => {
            let mut out = Vec::with_capacity(l.elems.len());
            for el in &l.elems {
                let Expr::Name(n) = el else {
                    return Vec::new();
                };
                out.push(n.value.clone());
            }
            out
        }
        _ => Vec::new(),
    }
}

fn tuple_vars(tt: &rgoc_types::TypeTable, t: TypeId) -> Vec<(Option<String>, TypeId)> {
    match tt.type_of(rgoc_types::unalias(tt, t)) {
        Type::Tuple(id) => tt.tuple_def(*id).vars.clone(),
        _ => Vec::new(),
    }
}

fn results_text(tt: &rgoc_types::TypeTable, results: &[(Option<String>, TypeId)]) -> String {
    let items: Vec<String> = results.iter().map(|(_, t)| type_str(tt, *t)).collect();
    format!("({})", items.join(", "))
}

fn operand_type_text(tt: &rgoc_types::TypeTable, x: &Operand) -> String {
    match x.typ {
        Some(t) => type_str(tt, t),
        None => "value".to_owned(),
    }
}

/// 官方 `measure`：`1 variable` / `2 variables`。
fn measure(n: usize, unit: &str) -> String {
    if n == 1 {
        format!("{n} {unit}")
    } else {
        format!("{n} {unit}s")
    }
}

fn is_blank(e: &Expr) -> bool {
    matches!(e, Expr::Name(n) if n.value == "_")
}

fn name_of(e: &Expr) -> String {
    match e {
        Expr::Name(n) => n.value.clone(),
        _ => String::new(),
    }
}

/// `<-ch`（接收）—— 表达式语句与 select 子句的合法形态。
fn is_receive(e: &Expr) -> bool {
    match e {
        Expr::Unary(o) => o.op == rgoc_ast::Op::Arrow,
        Expr::Paren(p) => is_receive(&p.x),
        _ => false,
    }
}

/// 简单语句的位置（`Empty` 没有自己的位置 —— 由调用方退到子句位）。
fn simple_pos(s: &SimpleStmt) -> Option<Pos> {
    match s {
        SimpleStmt::Empty => None,
        SimpleStmt::Expr(e) => Some(e.pos),
        SimpleStmt::Send(sd) => Some(sd.pos),
        SimpleStmt::Assign(a) => Some(a.pos),
        SimpleStmt::Range(r) => Some(r.pos),
    }
}

/// select 子句合法性（官方 `stmt.go:796-815`）。
fn valid_comm(s: &SimpleStmt) -> bool {
    match s {
        SimpleStmt::Send(_) => true,
        SimpleStmt::Expr(e) => is_receive(&e.x),
        SimpleStmt::Assign(a) => a.rhs.len() == 1 && is_receive(&a.rhs[0]),
        _ => false,
    }
}

/// 复合赋值的**基础**符号文本（`+`、`<<` …；`=` 由消息侧拼）。
///
/// 官方 `assignOp`（`stmt.go:165`）靠 token 枚举相邻性算
/// `op + (token.ADD - token.ADD_ASSIGN)`；本仓 `AssignOp` 直接存基础运算符
/// （`rgoc-ast/src/aux.rs:71`），所以这里只做映射。`Assign` / `Define` /
/// `Inc` / `Dec` 不是复合赋值 —— 由 [`base_op`] 先挡掉，这里退 `"="` 只为穷尽。
fn compound_sym(op: AssignOp) -> &'static str {
    match op {
        AssignOp::Add => "+",
        AssignOp::Sub => "-",
        AssignOp::Mul => "*",
        AssignOp::Quo => "/",
        AssignOp::Rem => "%",
        AssignOp::And => "&",
        AssignOp::Or => "|",
        AssignOp::Xor => "^",
        AssignOp::Shl => "<<",
        AssignOp::Shr => ">>",
        AssignOp::AndNot => "&^",
        _ => "=",
    }
}

/// 复合赋值的**基础**运算符（官方 `assignOp`）。
///
/// `Assign` / `Define` / `Inc` / `Dec` 不是复合赋值 ⇒ `None`（官方 `ILLEGAL`）。
fn base_op(op: AssignOp) -> Option<Op> {
    Some(match op {
        AssignOp::Add => Op::Add,
        AssignOp::Sub => Op::Sub,
        AssignOp::Mul => Op::Mul,
        AssignOp::Quo => Op::Quo,
        AssignOp::Rem => Op::Rem,
        AssignOp::And => Op::And,
        AssignOp::Or => Op::Or,
        AssignOp::Xor => Op::Xor,
        AssignOp::Shl => Op::Shl,
        AssignOp::Shr => Op::Shr,
        AssignOp::AndNot => Op::AndNot,
        AssignOp::Assign | AssignOp::Define | AssignOp::Inc | AssignOp::Dec => return None,
    })
}

// ═══════════════════════════ 终止语句（官方 `return.go`）═══════════════════════════

fn terminating_list(list: &[Stmt], label: &str) -> bool {
    // 末尾的空语句跳过（官方 `isTerminatingList`）
    for s in list.iter().rev() {
        if matches!(s, Stmt::Empty(_)) {
            continue;
        }
        return terminating(s, label);
    }
    false
}

fn terminating(s: &Stmt, label: &str) -> bool {
    match s {
        Stmt::Labeled(l) => terminating(&l.stmt, &l.label.value),
        // 调用 `panic()` 是终止语句
        Stmt::Expr(e) => is_panic_call(&e.x),
        Stmt::Return(_) => true,
        Stmt::Branch(b) => matches!(
            b.tok,
            rgoc_ast::BranchTok::Goto | rgoc_ast::BranchTok::Fallthrough
        ),
        Stmt::Block(b) => terminating_list(&b.stmts, ""),
        Stmt::If(i) => i
            .els
            .as_deref()
            .is_some_and(|els| terminating_stmt_or_block(&i.then, "") && terminating(els, "")),
        Stmt::Switch(sw) => terminating_switch(&sw.cases, label),
        Stmt::Select(sel) => sel.clauses.iter().all(|CommClause { body, .. }| {
            terminating_list(body, "") && !has_break_list(body, label, true)
        }),
        // `for {}` 无 break ⇒ 终止
        Stmt::For(f) => f.cond.is_none() && !has_break_stmt(s, label, true),
        _ => false,
    }
}

fn terminating_stmt_or_block(b: &BlockStmt, label: &str) -> bool {
    terminating_list(&b.stmts, label)
}

fn terminating_switch(cases: &[CaseClause], label: &str) -> bool {
    let mut has_default = false;
    for c in cases {
        if c.cases.is_none() {
            has_default = true;
        }
        if !terminating_list(&c.body, "") || has_break_list(&c.body, label, true) {
            return false;
        }
    }
    has_default
}

fn is_panic_call(e: &Expr) -> bool {
    match e {
        Expr::Call(c) => matches!(expr_label(&c.fun).as_deref(), Some("panic")),
        Expr::Paren(p) => is_panic_call(&p.x),
        _ => false,
    }
}

fn has_break_stmt(s: &Stmt, label: &str, implicit: bool) -> bool {
    match s {
        Stmt::Labeled(l) => has_break_stmt(&l.stmt, label, implicit),
        Stmt::Branch(b) => {
            if b.tok != rgoc_ast::BranchTok::Break {
                return false;
            }
            match &b.label {
                None => implicit,
                Some(n) => n.value == label,
            }
        }
        Stmt::Block(bl) => has_break_list(&bl.stmts, label, implicit),
        Stmt::If(i) => {
            has_break_list(&i.then.stmts, label, implicit)
                || i.els
                    .as_deref()
                    .is_some_and(|els| has_break_stmt(els, label, implicit))
        }
        Stmt::Switch(sw) => !label.is_empty() && has_break_cases(&sw.cases, label, false),
        Stmt::Select(sel) => {
            !label.is_empty()
                && sel
                    .clauses
                    .iter()
                    .any(|c| has_break_list(&c.body, label, false))
        }
        Stmt::For(f) => !label.is_empty() && has_break_list(&f.body.stmts, label, false),
        _ => false,
    }
}

fn has_break_cases(cases: &[CaseClause], label: &str, implicit: bool) -> bool {
    cases
        .iter()
        .any(|c| has_break_list(&c.body, label, implicit))
}

fn has_break_list(list: &[Stmt], label: &str, implicit: bool) -> bool {
    list.iter().any(|s| has_break_stmt(s, label, implicit))
}
