//! AST + 检查会话 → [`Program`]（`M5-design.md` §3.2 的落地）。
//!
//! # 两条纪律
//!
//! 1. **类型只从会话取**（`D-M5-2`）：每个节点的 `typ` 都来自
//!    `Session::expr_in` 的 `Operand.typ`。拿不到 ⇒ [`RejectKind::MissingType`]
//!    （响亮失败），**不**在 lowering 里补推断。
//! 2. **求值顺序显式化**：赋值先求 rhs 再写 lhs、`defer` 的实参登记时求值、
//!    复合字面量的 keyed 元素在下标上定死。
//!
//! # 两遍
//!
//! * 第一遍：建**函数表**（包级函数 + 方法）与**全局表** —— 前向引用靠它；
//! * 第二遍：lower 函数体（`main` / `init` / 其余按声明序）。
//!
//! # 窄面（都登记在 `M5-manifest.json` 的 `open_gaps`）
//!
//! * 顶层块以外的 `goto` ⇒ `HIR-UNSUPPORTED-GOTO`；
//! * 泛型实例化 ⇒ `HIR-UNSUPPORTED-GENERIC`（只做非泛型子集）；
//! * 跨包引用 ⇒ `HIR-MISSING-TYPE`（M5 语料全是单包）。

use std::collections::BTreeMap;

use rgoc_ast::{
    AssignOp, BlockStmt, CallExpr, CaseClause, CompositeLit, Decl, DeclStmt, Expr as AExpr,
    ForStmt, FuncDecl, IfStmt, Name, Op, RangeClause, SimpleStmt, Stmt as AStmt, SwitchStmt,
};
use rgoc_check::Session;
use rgoc_check::env::Env;
use rgoc_check::operand::{Mode, Operand};
use rgoc_const::{BasicId, Const, default_type_of};
use rgoc_lex::{FileId, Pos};
use rgoc_load::Package;
use rgoc_types::{Type, TypeId, TypeTable, unalias, underlying};

use crate::diag::{HirReject, RejectKind};
use crate::types::{
    BinOp, Block, BranchKind, BuiltinKind, Callee, Capture, Case, Composite, CompositeItem,
    ConstVal, Expr, ExprKind, Func, FuncId, Global, GlobalId, InitGroup, MakeKind, MethodRef,
    Place, Program, RangeKind, SlotId, Stmt, TypeCase, UnOp,
};

/// `_` 的槽占位（永远不会被读写）。
pub const DISCARD_SLOT: SlotId = SlotId(u32::MAX);

/// lower 一个已通过检查的包。
///
/// # Errors
///
/// 任一构造超出 M5 的支持面 ⇒ 返回**全部**拒绝（不是第一条）：
/// 一次跑完能看见「还有多少处不支持」，只报第一条会让人反复重跑。
pub fn lower<'p>(pkg: &'p Package, s: &mut Session<'p>) -> Result<Program, Vec<HirReject>> {
    let mut lw = Lowerer::new(pkg, s);
    lw.collect();
    lw.lower_all();
    lw.finish()
}

/// 一个局部名字：槽 + 静态类型。
#[derive(Clone, Copy, Debug)]
struct Local {
    slot: SlotId,
    typ: TypeId,
}

/// 正在 lower 的函数。
struct FnCtx {
    fid: FuncId,
    next_slot: u32,
    /// 每个槽的静态类型（与 `next_slot` 同步增长）。
    slot_types: Vec<TypeId>,
    /// 作用域栈（每层一个名字表；栈底是函数层）。
    scopes: Vec<BTreeMap<String, Local>>,
    /// 每层的**局部常量**（`const` 声明；名字 → 类型 + 值）。
    ///
    /// 常量**没有槽**：它在 lowering 期就折成 `Expr::Const`（`D-M5-5` 的
    /// 「常量保留任意精度直到收敛点」），运行期不占帧。
    consts: Vec<BTreeMap<String, (TypeId, Const)>>,
    /// 每层的**局部具名类型**（`type S struct{…}`；名字 → `TypeId`）。
    ///
    /// `TypeId` 由检查期定下并记忆化（`Session::local_type_at`）：lowering
    /// **不许**自己建 named（否则同一性分叉，`D-M5-2`）。
    types: Vec<BTreeMap<String, TypeId>>,
    captures: Vec<Capture>,
    labels: BTreeMap<String, usize>,
}

impl FnCtx {
    fn new(fid: FuncId) -> Self {
        Self {
            fid,
            next_slot: 0,
            slot_types: Vec::new(),
            scopes: vec![BTreeMap::new()],
            consts: vec![BTreeMap::new()],
            types: vec![BTreeMap::new()],
            captures: Vec::new(),
            labels: BTreeMap::new(),
        }
    }

    fn alloc_t(&mut self, typ: TypeId) -> SlotId {
        let s = SlotId(self.next_slot);
        self.next_slot += 1;
        self.slot_types.push(typ);
        s
    }

    fn declare(&mut self, name: &str, local: Local) {
        if name == "_" {
            return;
        }
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.to_owned(), local);
        }
    }

    fn lookup(&self, name: &str) -> Option<Local> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }
}

struct Lowerer<'a, 'p> {
    pkg: &'p Package,
    s: &'a mut Session<'p>,
    prog: Program,
    /// 包级函数名 → 函数。
    funcs_by_name: BTreeMap<String, FuncId>,
    /// `(接收者基类型名, 方法名)` → 函数。
    methods: BTreeMap<(String, String), FuncId>,
    /// 包级变量名 → 全局。
    globals_by_name: BTreeMap<String, GlobalId>,
    /// 初始化组（声明序）与「名字 → 组」。
    groups: Vec<InitGroup>,
    group_of_name: BTreeMap<String, usize>,
    /// 待 lower 的函数体：`(FuncId, ast 下标, decl 下标)`。
    pending: Vec<(FuncId, usize, usize)>,
    /// 函数栈（栈顶 = 正在 lower 的函数）。
    stack: Vec<FnCtx>,
    file: FileId,
    rejects: Vec<HirReject>,
    /// `L: for …` / `L: switch …` 的标签（`Labeled` 包着循环时由它转交）。
    pending_label: Option<String>,
    /// 外层给的**元素类型提示**（只在 lower 一个省略类型的嵌套复合字面量时非空）。
    ///
    /// 省略类型的复合字面量（`[2]T{{0,1}, {0,2}}` 里的 `{0,1}`）自己**没有**
    /// 类型可查：单独 `expr_in` 求值会让检查器报
    /// `missing type in composite literal`（`align.go` / `escape3.go` /
    /// `ken/convert.go` / `nilptr2.go` 实测）。类型只能来自外层元素类型。
    hint: Option<TypeId>,
}

impl<'a, 'p> Lowerer<'a, 'p> {
    fn new(pkg: &'p Package, s: &'a mut Session<'p>) -> Self {
        Self {
            pkg,
            s,
            prog: Program {
                funcs: Vec::new(),
                globals: Vec::new(),
                init_order: Vec::new(),
                inits: Vec::new(),
                main: None,
            },
            funcs_by_name: BTreeMap::new(),
            methods: BTreeMap::new(),
            globals_by_name: BTreeMap::new(),
            groups: Vec::new(),
            group_of_name: BTreeMap::new(),
            pending: Vec::new(),
            stack: Vec::new(),
            file: pkg
                .files
                .first()
                .copied()
                .unwrap_or_else(|| FileId::from_index(0)),
            rejects: Vec::new(),
            pending_label: None,
            hint: None,
        }
    }

    // ───────────────────────── 类型小工具 ─────────────────────────

    fn tt(&self) -> &TypeTable {
        self.s.tt
    }

    fn is_iface(&self, t: TypeId) -> bool {
        let tt = self.tt();
        matches!(tt.type_of(underlying(tt, t)), Type::Interface(_))
    }

    /// 是**数组**类型（不含切片；切片本身已是引用语义，切片时无需取址）。
    fn is_array(&self, t: TypeId) -> bool {
        let tt = self.tt();
        matches!(tt.type_of(underlying(tt, t)), Type::Array { .. })
    }

    fn basic_of(&self, t: TypeId) -> Option<BasicId> {
        let tt = self.tt();
        match tt.type_of(underlying(tt, t)) {
            Type::Basic(b) => Some(*b),
            _ => None,
        }
    }

    fn is_nil_type(&self, t: TypeId) -> bool {
        self.basic_of(t) == Some(BasicId::UntypedNil)
    }

    fn is_untyped(&self, t: TypeId) -> bool {
        matches!(
            self.basic_of(t),
            Some(
                BasicId::UntypedBool
                    | BasicId::UntypedInt
                    | BasicId::UntypedRune
                    | BasicId::UntypedFloat
                    | BasicId::UntypedComplex
                    | BasicId::UntypedString
                    | BasicId::UntypedNil
            )
        )
    }

    /// 默认型（无类型常量在 `:=` / 实参位置的落地类型）。
    fn default_type(&mut self, t: TypeId) -> TypeId {
        match self.basic_of(t) {
            Some(b) if self.is_untyped(t) => {
                let d = default_type_of(b);
                if d == BasicId::UntypedNil {
                    t
                } else {
                    self.s.tt.add_basic(d)
                }
            }
            _ => t,
        }
    }

    /// 结构体的字段表（穿透命名/指针/别名）。
    fn struct_fields(&self, t: TypeId) -> Option<Vec<rgoc_types::Field>> {
        let tt = self.tt();
        let mut cur = unalias(tt, t);
        if let Type::Pointer(e) = tt.type_of(cur) {
            cur = unalias(tt, *e);
        }
        match tt.type_of(underlying(tt, cur)) {
            Type::Struct(sid) => Some(tt.struct_def(*sid).fields.clone()),
            _ => None,
        }
    }

    /// 在结构体里找字段（含**嵌入提升**），返回下标链。
    fn find_field(&self, t: TypeId, name: &str) -> Option<Vec<usize>> {
        let fields = self.struct_fields(t)?;
        for (i, f) in fields.iter().enumerate() {
            if f.name.as_deref() == Some(name) {
                return Some(vec![i]);
            }
            // 嵌入字段的**名字**就是类型名，但装载面可能不给 `name`
            // （`struct{ Val }` 的字段名是 `Val`）。按类型名再比一次，
            // 否则 `zv.Val = …` 这种提升字段赋值会报「左值字段找不到」
            // （`method.go` 实测）。
            if f.embedded && f.name.is_none() && self.type_name_is(f.typ, name) {
                return Some(vec![i]);
            }
        }
        // 嵌入提升：只认一层（再深也能递归，但 M5 的语料用不到，且深链要配环守卫）
        for (i, f) in fields.iter().enumerate() {
            if f.embedded
                && let Some(mut rest) = self.find_field(f.typ, name)
            {
                let mut path = vec![i];
                path.append(&mut rest);
                return Some(path);
            }
        }
        None
    }

    /// 接口是否声明了方法 `name`。
    fn iface_has_method(&self, t: TypeId, name: &str) -> bool {
        let tt = self.tt();
        match tt.type_of(underlying(tt, unalias(tt, t))) {
            Type::Interface(iid) => tt
                .interface_def(*iid)
                .methods
                .iter()
                .any(|(m, _)| m == name),
            _ => false,
        }
    }

    /// 在**嵌入字段链**上找一个接口字段，其接口类型声明了方法 `name`。
    ///
    /// 返回 `(下标链, 接口类型)`。`struct{ I }` 的 `s.Sum(...)` 走这里：
    /// 方法不在具体类型上，而在嵌入的**接口字段**上，调用必须落成按动态类型
    /// 分派的 `Callee::Iface`，接收者是那个字段（`ddd.go` 实测）。
    fn find_iface_method_path(&self, t: TypeId, name: &str) -> Option<(Vec<usize>, TypeId)> {
        let fields = self.struct_fields(t)?;
        for (i, f) in fields.iter().enumerate() {
            if !f.embedded {
                continue;
            }
            if self.iface_has_method(f.typ, name) {
                return Some((vec![i], f.typ));
            }
            if let Some((mut rest, it)) = self.find_iface_method_path(f.typ, name) {
                let mut path = vec![i];
                path.append(&mut rest);
                return Some((path, it));
            }
        }
        None
    }

    /// 把 `base` 表达式沿下标链取到字段。
    fn field_chain(&mut self, ast: &AExpr, path: &[usize]) -> Expr {
        let mut e = self.lower_expr(ast);
        for &i in path {
            let ft = self
                .struct_fields(e.typ)
                .and_then(|fs| fs.get(i).map(|f| f.typ));
            let Some(ft) = ft else { break };
            e = Expr::new(
                ExprKind::Field {
                    base: Box::new(e),
                    index: i,
                },
                ft,
            );
        }
        e
    }

    /// 方法的函数体（含嵌入提升）。
    fn find_method(&self, t: TypeId, name: &str) -> Option<FuncId> {
        self.find_method_path(t, name).map(|(f, _)| f)
    }

    /// 方法 + **嵌入提升的字段下标链**。
    ///
    /// 链非空说明方法声明在嵌入字段的类型上：接收者必须**先取到那个字段**，
    /// 否则把外层结构体整个绑进内层方法的接收者槽 —— 类型对不上，解释器
    /// 在字段访问处读到 nil（`method5.go` 的 `T1{T2{*T3{*T4{}}}}` 实测
    /// `nil pointer dereference (M)`）。
    fn find_method_path(&self, t: TypeId, name: &str) -> Option<(FuncId, Vec<usize>)> {
        let tt = self.tt();
        let mut cur = unalias(tt, t);
        if let Type::Pointer(e) = tt.type_of(cur) {
            cur = unalias(tt, *e);
        }
        if let Type::Named(nid) = tt.type_of(cur) {
            let def = tt.named_def(*nid);
            if def.methods.iter().any(|(m, _)| m == name) {
                let fid = self
                    .methods
                    .get(&(def.name.clone(), name.to_owned()))
                    .copied()?;
                return Some((fid, Vec::new()));
            }
        }
        // 嵌入字段的方法提升：**不限具名类型**。`struct{ T }.m2`（字面量结构体
        // 类型的方法表达式）也走这里 —— 之前这段嵌在 `Type::Named` 分支里，
        // 字面量类型一律找不到方法（`method7.go` 实测）。
        let fields = self.struct_fields(cur)?;
        for (i, f) in fields.iter().enumerate() {
            if f.embedded
                && let Some((fid, mut rest)) = self.find_method_path(f.typ, name)
            {
                let mut path = vec![i];
                path.append(&mut rest);
                return Some((fid, path));
            }
        }
        None
    }

    /// `t` 的类型名是否等于 `name`（嵌入字段的名字来源）。
    fn type_name_is(&self, t: TypeId, name: &str) -> bool {
        let tt = self.tt();
        let mut cur = unalias(tt, t);
        if let Type::Pointer(e) = tt.type_of(cur) {
            cur = unalias(tt, *e);
        }
        match tt.type_of(cur) {
            Type::Named(nid) => tt.named_def(*nid).name == name,
            _ => false,
        }
    }

    // ───────────────────────── 第一遍：声明 ─────────────────────────

    fn collect(&mut self) {
        let pkg = self.pkg;
        // ① **名字面**：函数 / 方法 / 全局变量（**不含初始化式**）。
        //
        // 必须先走完这一遍再 lower 任何初始化式：`var tests = []struct{…}{
        // {"m1.F()", func(){ m1.F() }}, … }` 里的方法 `func (m *M) F()` 可能
        // 声明在**后面**，而 lowering 找方法靠 `self.methods`。边收边 lower
        // 会让前向声明的方法在 lowering 期「不存在」（`nilptr2.go` 实测：
        // 四个 `.F` 全部报「选择器找不到字段或方法」）。
        for (ai, ast) in pkg.asts.iter().enumerate() {
            for (di, decl) in ast.decls.iter().enumerate() {
                match decl {
                    Decl::Func(f) => {
                        let fid = FuncId(u32::try_from(self.prog.funcs.len()).unwrap_or(u32::MAX));
                        self.prog
                            .funcs
                            .push(Func::placeholder(f.name.value.clone()));
                        if let Some(recv) = &f.recv {
                            if let Some(base) = recv_base_name(&recv.type_) {
                                self.methods.insert((base, f.name.value.clone()), fid);
                            }
                        } else if f.name.value == "init" {
                            self.prog.inits.push(fid);
                        } else {
                            self.funcs_by_name.insert(f.name.value.clone(), fid);
                        }
                        self.pending.push((fid, ai, di));
                    }
                    Decl::Var(v) => {
                        for n in &v.names {
                            let typ = pkg
                                .lookup(&n.value)
                                .and_then(|o| o.typ)
                                .unwrap_or_else(|| self.s.tt.invalid());
                            let gid = GlobalId(
                                u32::try_from(self.prog.globals.len()).unwrap_or(u32::MAX),
                            );
                            self.prog.globals.push(Global {
                                name: n.value.clone(),
                                typ,
                            });
                            self.globals_by_name.insert(n.value.clone(), gid);
                        }
                    }
                    _ => {}
                }
            }
        }
        self.prog.main = self.funcs_by_name.get("main").copied();

        // ② **初始化式**：函数表与全局表都已就绪，前向引用随便用。
        for (ai, ast) in pkg.asts.iter().enumerate() {
            let file = pkg.files[ai];
            for decl in &ast.decls {
                let Decl::Var(v) = decl else { continue };
                if v.values.is_empty() {
                    continue;
                }
                let ids: Vec<GlobalId> = v
                    .names
                    .iter()
                    .filter_map(|n| self.globals_by_name.get(&n.value).copied())
                    .collect();
                if ids.is_empty() {
                    continue;
                }
                let gi = self.groups.len();
                for n in &v.names {
                    self.group_of_name.insert(n.value.clone(), gi);
                }
                let rhs: Vec<Expr> = v
                    .values
                    .iter()
                    .map(|e| self.lower_global_expr(file, e))
                    .collect();
                // 全局的类型**兜底**：装载面只给得出显式声明型的
                // 全局（`var p, q = new([0]int), new([0]int)` 的
                // 类型在右侧，装载面留空）。空着会让 `check_env`
                // 声明一个 `invalid` 变量，之后每个用到它的表达式都
                // 判成 invalid（`zerosize.go` 实测）。
                for (i, gid) in ids.iter().enumerate() {
                    let cur = self.prog.globals[gid.0 as usize].typ;
                    if cur == self.s.tt.invalid()
                        && let Some(e) = rhs.get(i)
                        && e.typ != self.s.tt.invalid()
                    {
                        let t = self.default_type(e.typ);
                        self.prog.globals[gid.0 as usize].typ = t;
                    }
                }
                self.groups.push(InitGroup { globals: ids, rhs });
            }
        }
    }

    /// 包级初始化式（无局部作用域）。
    fn lower_global_expr(&mut self, file: FileId, e: &AExpr) -> Expr {
        let saved = self.file;
        self.file = file;
        self.stack.push(FnCtx::new(FuncId(u32::MAX))); // 伪上下文：没有槽可分配
        let out = self.lower_expr(e);
        self.stack.pop();
        self.file = saved;
        out
    }

    // ───────────────────────── 第二遍：函数体 ─────────────────────────

    fn lower_all(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for (fid, ai, di) in pending {
            let file = self.pkg.files[ai];
            let Decl::Func(f) = &self.pkg.asts[ai].decls[di] else {
                continue;
            };
            let f = f.clone();
            self.file = file;
            self.lower_func(fid, &f);
        }
        self.prog.init_order = self.order_inits();
    }

    /// 按 `initorder::analyze` 的顺序排初始化组。
    fn order_inits(&self) -> Vec<InitGroup> {
        let analysis = rgoc_check::initorder::analyze(self.pkg);
        let mut out = Vec::new();
        let mut used = vec![false; self.groups.len()];
        for init in &analysis.order {
            if let Some(first) = init.names.first()
                && let Some(&gi) = self.group_of_name.get(first)
                && !used[gi]
            {
                used[gi] = true;
                out.push(self.groups[gi].clone());
            }
        }
        // 没被图提到的组（无初始化式的 var 不会进图）按声明序补在后面。
        for (i, g) in self.groups.iter().enumerate() {
            if !used[i] {
                out.push(g.clone());
            }
        }
        out
    }

    fn lower_func(&mut self, fid: FuncId, f: &FuncDecl) {
        let mut ctx = FnCtx::new(fid);
        let mut params = Vec::new();
        let mut param_types = Vec::new();
        let mut results = Vec::new();
        let mut result_types = Vec::new();
        let mut named_results = false;

        // 接收者先占一个槽（方法）。
        if let Some(recv) = &f.recv {
            let typ = self
                .s
                .type_expr_of(self.file, &recv.type_)
                .unwrap_or_else(|| self.s.tt.invalid());
            let slot = ctx.alloc_t(typ);
            param_types.push(typ);
            params.push(slot);
            if let Some(n) = &recv.name {
                ctx.declare(&n.value, Local { slot, typ });
            }
        }

        let mut variadic = false;
        if let AExpr::FuncType(ft) = &f.type_ {
            for p in &ft.params {
                let (typ, dots) = self.param_type(&p.type_);
                variadic |= dots;
                let slot = ctx.alloc_t(typ);
                params.push(slot);
                param_types.push(typ);
                if let Some(n) = &p.name {
                    ctx.declare(&n.value, Local { slot, typ });
                }
            }
            for r in &ft.results {
                let typ = self
                    .s
                    .type_expr_of(self.file, &r.type_)
                    .unwrap_or_else(|| self.s.tt.invalid());
                let slot = ctx.alloc_t(typ);
                results.push(slot);
                result_types.push(typ);
                if let Some(n) = &r.name {
                    named_results = true;
                    ctx.declare(&n.value, Local { slot, typ });
                }
            }
        } else {
            // 没有 FuncType 的形态（错误恢复）—— 拒绝而不是猜。
            self.reject(
                f.pos,
                RejectKind::MissingType,
                format!("函数 {} 的签名不是 FuncType", f.name.value),
            );
        }

        self.stack.push(ctx);
        // 进了新函数体，外层复合字面量的元素类型提示就不再适用。
        let saved_hint = self.hint.take();
        let body = match &f.body {
            Some(b) => self.lower_body_block(b, true),
            None => Block::default(),
        };
        self.hint = saved_hint;
        let ctx = self.stack.pop().expect("lower_func 的栈不平衡");

        let func = &mut self.prog.funcs[fid.index()];
        func.params = params;
        func.param_types = param_types;
        func.results = results;
        func.result_types = result_types;
        func.nslots = ctx.next_slot as usize;
        func.slot_types = ctx.slot_types;
        func.body = body;
        func.captured = ctx.captures;
        func.variadic = variadic;
        func.named_results = named_results;
        func.method_of = f.recv.as_ref().and_then(|r| recv_base_name(&r.type_));
        func.labels = ctx.labels;
    }

    /// 形参类型：`...T` ⇒ `[]T` + 变参位。
    fn param_type(&mut self, e: &AExpr) -> (TypeId, bool) {
        if let AExpr::DotsType(d) = e {
            let elem = self
                .s
                .type_expr_of(self.file, &d.elem)
                .unwrap_or_else(|| self.s.tt.invalid());
            let slice = self.s.tt.add_slice(elem);
            return (slice, true);
        }
        (
            self.s
                .type_expr_of(self.file, e)
                .unwrap_or_else(|| self.s.tt.invalid()),
            false,
        )
    }

    // ───────────────────────── 作用域 ─────────────────────────

    fn cur(&self) -> &FnCtx {
        self.stack.last().expect("lowering 必须有当前函数")
    }

    fn cur_mut(&mut self) -> &mut FnCtx {
        self.stack.last_mut().expect("lowering 必须有当前函数")
    }

    fn push_scope(&mut self) {
        let c = self.cur_mut();
        c.scopes.push(BTreeMap::new());
        c.consts.push(BTreeMap::new());
        c.types.push(BTreeMap::new());
    }

    fn pop_scope(&mut self) {
        let c = self.cur_mut();
        c.scopes.pop();
        c.consts.pop();
        c.types.pop();
    }

    fn alloc_slot(&mut self, typ: TypeId) -> SlotId {
        self.cur_mut().alloc_t(typ)
    }

    fn declare(&mut self, name: &str, typ: TypeId) -> SlotId {
        let slot = self.alloc_slot(typ);
        self.cur_mut().declare(name, Local { slot, typ });
        slot
    }

    /// `:=` 的**重声明**：名字已在**当前**作用域里 ⇒ 复用它的槽。
    ///
    /// Go 规范（Short variable declarations）：短变量声明可以重声明同一块里
    /// 已有的变量，只要至少有一个非空白的新名字。`decl.go` 的
    /// `func x() (s string) { a, b, s := f3(); return }` 就是靠这条：
    /// `s` 必须是那个具名结果，否则裸 `return` 返回零值。
    ///
    /// 只看最内层作用域是**正确**的：外层同名变量必须被遮住（那才是新声明），
    /// 而函数形参/具名结果与函数体同属一层（见 `lower_body_block`）。
    ///
    /// 返回 `(槽, 是否新声明)`。
    fn declare_reuse(&mut self, name: &str, typ: TypeId) -> (SlotId, bool) {
        if let Some(top) = self.cur().scopes.last()
            && let Some(l) = top.get(name)
        {
            return (l.slot, false);
        }
        (self.declare(name, typ), true)
    }

    /// 查一个名字：本函数 → 外层函数（逐层建捕获链）。
    fn resolve(&mut self, name: &str) -> Option<Local> {
        let n = self.stack.len();
        for i in (0..n).rev() {
            let hit = self.stack[i].lookup(name);
            if let Some(local) = hit {
                let mut outer = local.slot;
                for j in (i + 1)..n {
                    let existing = self.stack[j]
                        .captures
                        .iter()
                        .find(|c| c.outer_slot == outer)
                        .map(|c| c.slot);
                    outer = match existing {
                        Some(s) => s,
                        None => {
                            let slot = self.stack[j].alloc_t(local.typ);
                            self.stack[j].captures.push(Capture {
                                outer_slot: outer,
                                slot,
                            });
                            self.stack[j]
                                .scopes
                                .first_mut()
                                .expect("作用域栈非空")
                                .insert(
                                    name.to_owned(),
                                    Local {
                                        slot,
                                        typ: local.typ,
                                    },
                                );
                            slot
                        }
                    };
                }
                return Some(Local {
                    slot: outer,
                    typ: local.typ,
                });
            }
        }
        None
    }

    /// 查一个**局部常量**（逐层由内向外，再退到包级）。
    fn resolve_const(&mut self, name: &str) -> Option<(TypeId, Const)> {
        for ctx in self.stack.iter().rev() {
            for layer in ctx.consts.iter().rev() {
                if let Some(hit) = layer.get(name) {
                    return Some(hit.clone());
                }
            }
        }
        let obj = self.pkg.lookup(name)?;
        if obj.kind != rgoc_types::ObjKind::Const {
            return None;
        }
        let v = obj.val.as_ref()?;
        let t = obj.typ.unwrap_or_else(|| self.s.tt.invalid());
        Some((t, v.clone()))
    }

    /// 当前词法链里是否已有同名**局部常量**（不含包级）。
    fn has_local_const(&self, name: &str) -> bool {
        self.stack
            .iter()
            .rev()
            .any(|c| c.consts.iter().rev().any(|l| l.contains_key(name)))
    }

    /// 给检查会话用的局部作用域快照。
    fn check_env(&self) -> Env {
        let mut env = Env::new();
        // **整条词法链**，不只是当前函数：函数字面量的体里引用外层局部变量时
        // （`closure2.go` 的 `x.v`），变量在外层 ctx 的 scope 里。只喂 `cur()`
        // 会让这些名字查不到 ⇒ 检查器给 `invalid` ⇒ lowering 报 MissingType。
        //
        // 每层还要带上**局部常量**与**局部具名类型**（M5/Task 15）：检查器复查
        // `A != 0`（`const8.go` 的局部 `iota` 组）与 `MyInt(4)`（`range3.go`）
        // 时，这两个名字必须能解析，否则同样是 `invalid`。
        // 包级变量也喂进去：装载面只给得出**显式声明型**的全局，`var p, q =
        // new([0]int), new([0]int)` 的类型留在右侧、包作用域里是空的。只靠
        // `pkg.lookup` 复查 `p != q` 会拿到 `variable` + 无类型 ⇒ invalid
        // （`zerosize.go` 实测）。lowering 已经从初始化式补出了类型，就把它
        // 当作这一层的事实（最外层，局部照常遮住）。
        env.push();
        for (name, gid) in &self.globals_by_name {
            let typ = self.prog.globals[gid.0 as usize].typ;
            env.declare(name, Operand::variable(Some(typ)));
        }
        for ctx in &self.stack {
            for (i, scope) in ctx.scopes.iter().enumerate() {
                env.push();
                for (name, local) in scope {
                    env.declare(name, Operand::variable(Some(local.typ)));
                }
                if let Some(layer) = ctx.consts.get(i) {
                    for (name, (typ, val)) in layer {
                        env.declare(name, Operand::constant(*typ, val.clone()));
                    }
                }
                if let Some(layer) = ctx.types.get(i) {
                    for (name, typ) in layer {
                        env.declare(name, Operand::type_expr(*typ));
                    }
                }
            }
        }
        env
    }

    // ───────────────────────── 拒绝 ─────────────────────────

    fn reject(&mut self, pos: Pos, kind: RejectKind, msg: impl Into<String>) {
        self.rejects.push(HirReject::new(pos, kind, msg));
    }

    fn finish(self) -> Result<Program, Vec<HirReject>> {
        if self.rejects.is_empty() {
            Ok(self.prog)
        } else {
            Err(self.rejects)
        }
    }

    // ───────────────────────── 语句 ─────────────────────────

    fn lower_body_block(&mut self, b: &BlockStmt, top: bool) -> Block {
        // Go 规范（Declarations and scope）：函数体的块作用域**就是**
        // 形参/具名结果所在的作用域 ——「the scope of an identifier denoting a
        // method receiver, function parameter, or result variable is the
        // function body」。所以顶层体不再压一层，否则
        // `a, b, s := f3()` 里「重声明」的 `s` 会遮住具名结果，
        // 裸 `return` 拿到的仍是零值（`decl.go` 的 `x()` 实测）。
        if !top {
            self.push_scope();
        }
        // **前向 goto**：先把这个顶层块的标签全部登记，再逐句 lower。
        //
        // 只在下标推进时登记的话，`goto gogoloop` 出现在 `gogoloop:` **之前**
        // 的形态（`ken/label.go` 整篇都是）会一律被判「目标不在顶层块」——
        // 那是把「后向」当成了唯一合法方向。标签是块级属性，与出现次序无关。
        if top {
            for (i, s) in b.stmts.iter().enumerate() {
                if let AStmt::Labeled(l) = s
                    && !is_loop_or_switch(&l.stmt)
                {
                    self.cur_mut().labels.insert(l.label.value.clone(), i);
                }
            }
        }
        let mut stmts = Vec::new();
        for (i, s) in b.stmts.iter().enumerate() {
            if top
                && let AStmt::Labeled(l) = s
                && !is_loop_or_switch(&l.stmt)
            {
                self.cur_mut().labels.insert(l.label.value.clone(), i);
            }
            if let Some(st) = self.lower_stmt(s) {
                stmts.push(st);
            }
        }
        if !top {
            self.pop_scope();
        }
        Block { stmts }
    }

    fn lower_block(&mut self, b: &BlockStmt) -> Block {
        self.lower_body_block(b, false)
    }

    fn lower_stmt(&mut self, s: &AStmt) -> Option<Stmt> {
        match s {
            AStmt::Empty(_) => Some(Stmt::Empty),
            AStmt::Expr(es) => {
                let e = self.lower_expr(&es.x);
                Some(Stmt::Expr(e))
            }
            AStmt::Assign(a) => Some(self.lower_assign(a)),
            AStmt::Decl(DeclStmt { decls, .. }) => self.lower_decl_stmt(decls),
            AStmt::Block(b) => Some(Stmt::Block(self.lower_block(b))),
            AStmt::Return(r) => {
                let n = self.cur().scopes.len();
                let _ = n;
                let results: Vec<TypeId> = self.result_types_of_current();
                let mut exprs: Vec<Expr> = r.results.iter().map(|e| self.lower_expr(e)).collect();
                // 逐个按结果类型收敛（接口装箱 / 无类型常量落地）。
                for (i, e) in exprs.iter_mut().enumerate() {
                    if let Some(t) = results.get(i).copied() {
                        let old = std::mem::replace(e, Expr::new(ExprKind::Nil, t));
                        *e = self.coerce(old, t);
                    }
                }
                Some(Stmt::Return(exprs))
            }
            AStmt::If(f) => Some(self.lower_if(f)),
            AStmt::For(f) => Some(self.lower_for(f)),
            AStmt::Switch(sw) => self.lower_switch(sw),
            AStmt::Branch(b) => {
                let kind = match b.tok {
                    rgoc_ast::BranchTok::Break => BranchKind::Break,
                    rgoc_ast::BranchTok::Continue => BranchKind::Continue,
                    rgoc_ast::BranchTok::Goto => BranchKind::Goto,
                    rgoc_ast::BranchTok::Fallthrough => BranchKind::Fallthrough,
                };
                if kind == BranchKind::Goto {
                    let label = b
                        .label
                        .as_ref()
                        .map(|l| l.value.clone())
                        .unwrap_or_default();
                    if !self.cur().labels.contains_key(&label) {
                        self.reject(
                            b.pos,
                            RejectKind::Goto,
                            format!("goto {label}：目标不在函数体顶层块（M5 窄面）"),
                        );
                        return None;
                    }
                }
                Some(Stmt::Branch {
                    kind,
                    label: b.label.as_ref().map(|l| l.value.clone()),
                })
            }
            AStmt::Labeled(l) => {
                if is_loop_or_switch(&l.stmt) {
                    // 标签转交给循环 / switch 的 `label` 字段（break/continue 的目标）。
                    self.pending_label = Some(l.label.value.clone());
                    let out = self.lower_stmt(&l.stmt);
                    self.pending_label = None;
                    out
                } else {
                    let inner = self.lower_stmt(&l.stmt)?;
                    Some(Stmt::Labeled {
                        label: l.label.value.clone(),
                        stmt: Box::new(inner),
                    })
                }
            }
            AStmt::Call(c) => {
                if c.tok == rgoc_ast::CallTok::Go {
                    self.reject(c.pos, RejectKind::Go, "go 语句不在 M5 面（U-M5-05）");
                    return None;
                }
                let call = self.lower_expr(&c.call);
                Some(Stmt::Defer { call })
            }
            AStmt::Send(s) => {
                self.reject(s.pos, RejectKind::Chan, "通道发送不在 M5 面（U-M5-05）");
                None
            }
            AStmt::Select(s) => {
                self.reject(s.pos, RejectKind::Select, "select 不在 M5 面（U-M5-05）");
                None
            }
        }
    }

    fn result_types_of_current(&self) -> Vec<TypeId> {
        let fid = self.cur().fid;
        self.prog
            .funcs
            .get(fid.index())
            .map(|f| f.result_types.clone())
            .unwrap_or_default()
    }

    fn lower_assign(&mut self, a: &rgoc_ast::AssignStmt) -> Stmt {
        // `++` / `--`
        if matches!(a.op, AssignOp::Inc | AssignOp::Dec) {
            let place = self.lower_place(&a.lhs[0]);
            let typ = self.type_of(&a.lhs[0]);
            let one = self.const_one(typ);
            let op = if a.op == AssignOp::Inc {
                BinOp::Add
            } else {
                BinOp::Sub
            };
            return Stmt::Assign {
                lhs: vec![place],
                op: Some(op),
                rhs: vec![one],
            };
        }

        let rhs: Vec<Expr> = a.rhs.iter().map(|e| self.lower_expr(e)).collect();

        if a.op == AssignOp::Define {
            let mut slots = Vec::new();
            let mut declared: Vec<(String, TypeId)> = Vec::new();
            // 先算类型：多值右侧（`b, x := f()`）按返回元数展开。
            let rhs_types = self.rhs_types(&a.rhs, &rhs);
            for (i, l) in a.lhs.iter().enumerate() {
                let name = match l {
                    AExpr::Name(n) => n.value.clone(),
                    _ => {
                        self.reject(l.pos(), RejectKind::MissingType, "`:=` 左侧必须是名字");
                        continue;
                    }
                };
                let typ = rhs_types.get(i).copied().unwrap_or_else(|| self.type_of(l));
                let typ = self.default_type(typ);
                if name == "_" {
                    slots.push(DISCARD_SLOT);
                } else {
                    // 同块内的同名变量是**重声明**（复用槽），不是新声明。
                    let (slot, _new) = self.declare_reuse(&name, typ);
                    slots.push(slot);
                    declared.push((name, typ));
                }
            }
            let _ = declared;
            let mut rhs = rhs;
            let lhs_types: Vec<TypeId> = slots
                .iter()
                .map(|s| {
                    if *s == DISCARD_SLOT {
                        self.s.tt.invalid()
                    } else {
                        self.slot_type(*s)
                    }
                })
                .collect();
            // `v, ok := m[k]` / `v, ok := i.(T)`：右侧一项、左侧两项 ⇒ 双值。
            // 这一类**跳过** `coerce_list`（元素/目标类型已经精确）：否则
            // `Convert` 会在外面包一层，标记就落不到 `Index`/`TypeAssert` 上了。
            let comma_ok =
                a.lhs.len() == 2 && rhs.len() == 1 && self.comma_ok_types(&a.rhs[0]).is_some();
            if comma_ok {
                self.mark_comma_ok(&mut rhs);
            } else {
                self.coerce_list(&mut rhs, &lhs_types);
            }
            return Stmt::ShortDecl { slots, rhs };
        }

        let lhs_types: Vec<TypeId> = a.lhs.iter().map(|e| self.type_of(e)).collect();
        let mut rhs = rhs;
        let op = assign_binop(a.op);
        if op.is_some() {
            // 复合赋值：`x += y` ⇒ `Assign { op: Some(Add), rhs: [y] }`。
            //
            // `rhs[0].typ` 被**改成左值类型**：解释器只有这一个地方能拿到
            // 「这次运算按哪个宽度回绕」（`x += 1` 的 `1` 本身是 `untyped int`，
            // 按它算会把 `int8` 的 `127 + 1` 算成 `128` 而不是 `-128`）。
            if let (Some(t), Some(r)) = (lhs_types.first().copied(), rhs.first_mut()) {
                r.typ = t;
            }
        } else if a.lhs.len() == 2 && rhs.len() == 1 && self.comma_ok_types(&a.rhs[0]).is_some() {
            self.mark_comma_ok(&mut rhs);
        } else {
            self.coerce_list(&mut rhs, &lhs_types);
        }
        let lhs: Vec<Place> = a.lhs.iter().map(|e| self.lower_place(e)).collect();
        Stmt::Assign { lhs, op, rhs }
    }

    fn slot_type(&mut self, slot: SlotId) -> TypeId {
        for scope in self.cur().scopes.iter().rev() {
            for l in scope.values() {
                if l.slot == slot {
                    return l.typ;
                }
            }
        }
        self.s.tt.invalid()
    }

    /// 右侧表达式的静态类型（多值调用按返回元数展开）。
    fn rhs_types(&mut self, asts: &[AExpr], lowered: &[Expr]) -> Vec<TypeId> {
        if asts.len() == 1 && lowered.len() == 1 {
            if let Some(t) = self.comma_ok_types(&asts[0]) {
                return t;
            }
            let call_typ = self.call_result_types(&asts[0], &lowered[0]);
            if call_typ.len() > 1 {
                return call_typ;
            }
        }
        lowered.iter().map(|e| e.typ).collect()
    }

    /// 逗号-ok 双值形式的两个类型：`m[k]` ⇒ `(元素, bool)`，`i.(T)` ⇒ `(T, bool)`。
    ///
    /// 判据来自 checker 的结果模式（`mapindex` / `commaok`），**不是**自己看
    /// 语法形状 —— D-M5-2：lowering 不自己推类型。
    fn comma_ok_types(&mut self, ast: &AExpr) -> Option<Vec<TypeId>> {
        let env = self.check_env();
        let op = self.s.expr_in(self.file, ast, &env);
        let elem = match op.mode {
            Mode::MapIndex | Mode::CommaOk => op.typ?,
            _ => return None,
        };
        Some(vec![elem, self.s.tt.add_basic(BasicId::Bool)])
    }

    /// 把「双值」标到产生它的那个节点上（`Index.comma_ok` / `TypeAssert.comma_ok`）。
    ///
    /// **只有**「右侧唯一一项、左侧恰好两个」这一种位置是双值；其余位置
    /// （`f(m[k])`、`x = m[k]`）必须保持单值，否则解释器会多压一个 `bool`。
    /// 这一条判断集中在这里，是「哪里允许双值」的唯一出处。
    fn mark_comma_ok(&mut self, rhs: &mut [Expr]) {
        let Some(e) = rhs.first_mut() else { return };
        match &mut e.kind {
            ExprKind::Index { comma_ok, .. } | ExprKind::TypeAssert { comma_ok, .. } => {
                *comma_ok = true;
            }
            _ => {}
        }
    }

    /// 单值调用 ⇒ 空；多值调用 ⇒ 各结果类型。
    ///
    /// 多值调用的操作数类型在检查器里是**元组**（go/types 同形：`call` 的
    /// 结果是 `*Tuple`），不是签名 —— 所以先认元组，再退回「签名 → results」。
    /// 只认签名会让 `a, b, s := f3()` 的第 2、3 个变量拿不到类型
    /// （`decl.go:16` 实测）。
    fn call_result_types(&mut self, ast: &AExpr, lowered: &Expr) -> Vec<TypeId> {
        if !matches!(lowered.kind, ExprKind::Call { .. }) {
            return Vec::new();
        }
        let env = self.check_env();
        let op = self.s.expr_in(self.file, ast, &env);
        if op.mode != Mode::Value {
            return Vec::new();
        }
        let Some(t) = op.typ else { return Vec::new() };
        let tt = &*self.s.tt;
        let t = unalias(tt, t);
        let vars = match tt.type_of(t) {
            Type::Tuple(tid) => tt.tuple_def(*tid).vars.clone(),
            Type::Signature(sid) => {
                let d = tt.signature_def(*sid);
                match tt.type_of(d.results) {
                    Type::Tuple(tid) => tt.tuple_def(*tid).vars.clone(),
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        };
        vars.into_iter().map(|(_, t)| t).collect()
    }

    fn lower_decl_stmt(&mut self, decls: &[Decl]) -> Option<Stmt> {
        // 常量组要先整组展开：`iota` 的编号是**组内**属性，逐条 lower 看不到
        // 上一条规格（`const8.go` 的 `A = iota; iota = iota; B; C`）。
        // 展平顺序与 `decls` 里 `Decl::Const` 的出现顺序**一一对应**。
        let const_specs: Vec<(usize, Vec<AExpr>)> = {
            let cs: Vec<rgoc_ast::ConstDecl> = decls
                .iter()
                .filter_map(|d| match d {
                    Decl::Const(c) => Some(c.clone()),
                    _ => None,
                })
                .collect();
            rgoc_const::expand_const_group_specs(&cs)
        };
        let mut const_i = 0usize;
        let mut stmts = Vec::new();
        for d in decls {
            match d {
                Decl::Var(v) => {
                    // 作用域：`var` 说明符里的名字**在说明符之后**才进入作用域，
                    // 所以右侧的 `x` 看到的是**外层**的 `x`。
                    // `varinit.go` 的 `var x int = x + 1` 期望 2；若先 `declare`
                    // 再 lower 右侧，`x` 会解析到刚建的空槽，得到 1。
                    let mut rhs: Vec<Expr> = v.values.iter().map(|e| self.lower_expr(e)).collect();
                    let mut slots = Vec::new();
                    let mut names = Vec::new();
                    for n in &v.names {
                        let typ = match &v.type_ {
                            Some(t) => self.type_in_body(t).unwrap_or_else(|| self.s.tt.invalid()),
                            None => self.s.tt.invalid(),
                        };
                        if n.value == "_" {
                            slots.push(DISCARD_SLOT);
                        } else {
                            let slot = self.declare(&n.value, typ);
                            slots.push(slot);
                            names.push((n.value.clone(), slot));
                        }
                    }
                    if v.values.is_empty() {
                        // `var x T`：零值（帧初始化已经给零值，这里只补显式声明）。
                        continue;
                    }
                    // `var v, ok = m[k]`：与 `v, ok := m[k]` 同形，右侧一项两项。
                    let comma_ok = names.len() == 2
                        && v.values.len() == 1
                        && self.comma_ok_types(&v.values[0]).is_some();
                    let expanded = if comma_ok {
                        self.comma_ok_types(&v.values[0]).unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    // `var x = e` 的类型来自右侧；显式 `var x T = e` 的类型**不覆盖**。
                    if v.type_.is_none() {
                        for (i, (_, slot)) in names.iter().enumerate() {
                            let t = expanded
                                .get(i)
                                .copied()
                                .or_else(|| rhs.get(i).map(|e| e.typ))
                                .unwrap_or_else(|| self.s.tt.invalid());
                            let t = self.default_type(t);
                            self.set_slot_type(*slot, t);
                        }
                    }
                    let lhs_types: Vec<TypeId> =
                        names.iter().map(|(_, s)| self.slot_type(*s)).collect();
                    if comma_ok {
                        self.mark_comma_ok(&mut rhs);
                    } else {
                        self.coerce_list(&mut rhs, &lhs_types);
                    }
                    stmts.push(Stmt::ShortDecl { slots, rhs });
                }
                // 局部常量：**折成值**进作用域，运行期不占槽。
                //
                // 求值走检查器（`expr_in`）而不是自己的折叠器：常量的**类型**
                // 只有检查器知道（`untyped int` / 显式声明型 / 可表示性检查都
                // 在那一侧做过），`D-M5-2` 要求类型只从会话取。
                Decl::Const(c) => {
                    let (iota, values) =
                        const_specs.get(const_i).cloned().unwrap_or((0, Vec::new()));
                    const_i += 1;
                    let declared = c.type_.as_ref().and_then(|t| self.type_in_body(t));
                    for (j, n) in c.names.iter().enumerate() {
                        // 名字多于值 ⇒ 检查器已报 arity；这里不补第二条。
                        let Some(v) = values.get(j) else { continue };
                        let (typ, val) = self.eval_local_const(v, iota, declared);
                        if n.value == "_" {
                            continue;
                        }
                        let typ = self.default_type(typ);
                        self.cur_mut()
                            .consts
                            .last_mut()
                            .expect("作用域栈非空")
                            .insert(n.value.clone(), (typ, val));
                    }
                }
                // 局部具名类型：`TypeId` **由检查期定下并记忆化**，这里只登记。
                // 自己 `add_named` 会造出第二个 id，同一性判断随即分叉。
                Decl::Type(t) => {
                    if let Some(ty) = self.s.local_type_at(t.pos)
                        && t.name.value != "_"
                    {
                        self.cur_mut()
                            .types
                            .last_mut()
                            .expect("作用域栈非空")
                            .insert(t.name.value.clone(), ty);
                    }
                }
                Decl::Import(_) => {}
                Decl::Func(_) => {}
            }
        }
        if stmts.is_empty() {
            Some(Stmt::Empty)
        } else if stmts.len() == 1 {
            stmts.pop()
        } else {
            Some(Stmt::Block(Block { stmts }))
        }
    }

    /// 求一个**局部常量**的值与类型。
    ///
    /// 走检查器（`expr_in`）：常量表达式的类型（`untyped int` / 显式声明型 /
    /// 可表示性）只有那一侧知道。`iota` 通过**临时作用域**注入 —— 官方也是在
    /// 常量声明的作用域里把 `iota` 绑成当前下标（`go/types` 的
    /// `Checker.iota`），这里用同一手法，不另写一套 iota 代入。
    fn eval_local_const(
        &mut self,
        e: &AExpr,
        iota: usize,
        declared: Option<TypeId>,
    ) -> (TypeId, Const) {
        let int_ty = self.s.tt.add_basic(BasicId::UntypedInt);
        let mut env = self.check_env();
        // `iota` 只在**没有被同名局部常量遮住**时才注入。
        //
        // 官方把 `iota` 当预声明常量，`const ( A = iota; iota = iota; B )` 里
        // 第二条**声明**了一个同名常量，于是 `B` 的 `iota` 指的是那个常量（值 1），
        // 不是组下标 2（`const8.go` 的期望 `0 1 1` 正是这条）。无条件注入会让
        // 后两条都读成组下标（实测得到 `0 2 3`）。
        let shadowed = self.has_local_const("iota");
        if !shadowed {
            env.push();
            env.declare(
                "iota",
                Operand::constant(
                    int_ty,
                    Const::Int(rgoc_const::BigInt::from_i64(iota as i64)),
                ),
            );
        }
        let op = self.s.expr_in(self.file, e, &env);
        let typ = declared.or(op.typ).unwrap_or_else(|| self.s.tt.invalid());
        let val = op.val.unwrap_or(Const::Unknown);
        (typ, val)
    }

    fn set_slot_type(&mut self, slot: SlotId, typ: TypeId) {
        for scope in self.cur_mut().scopes.iter_mut().rev() {
            for l in scope.values_mut() {
                if l.slot == slot {
                    l.typ = typ;
                    return;
                }
            }
        }
    }

    fn lower_if(&mut self, f: &IfStmt) -> Stmt {
        self.push_scope();
        let init = f.init.as_ref().and_then(|s| self.lower_simple(s));
        let cond = match &f.cond {
            Some(c) => self.lower_expr(c),
            None => Expr::new(
                ExprKind::Const(ConstVal::Bool(true)),
                self.s.tt.add_basic(BasicId::Bool),
            ),
        };
        let then = self.lower_block(&f.then);
        let els = f
            .els
            .as_ref()
            .and_then(|e| self.lower_stmt(e))
            .map(Box::new);
        self.pop_scope();
        Stmt::If {
            init: init.map(Box::new),
            cond,
            then,
            els,
        }
    }

    fn lower_simple(&mut self, s: &SimpleStmt) -> Option<Stmt> {
        match s {
            SimpleStmt::Empty => Some(Stmt::Empty),
            SimpleStmt::Expr(e) => Some(Stmt::Expr(self.lower_expr(&e.x))),
            SimpleStmt::Assign(a) => Some(self.lower_assign(a)),
            SimpleStmt::Send(s) => {
                self.reject(s.pos, RejectKind::Chan, "通道发送不在 M5 面（U-M5-05）");
                None
            }
            SimpleStmt::Range(_) => None, // 由 lower_for 处理
        }
    }

    fn lower_for(&mut self, f: &ForStmt) -> Stmt {
        self.push_scope();
        let label = self.pending_label.take();
        if let Some(SimpleStmt::Range(rc)) = &f.init {
            let st = self.lower_range(rc, f, label);
            self.pop_scope();
            return st;
        }
        let init = f.init.as_ref().and_then(|s| self.lower_simple(s));
        // Go 1.22 的 per-iteration 变量：**只有** `init` 里 `:=` 声明的那些名字
        // 每轮新建。`_` 不占实体（`DISCARD_SLOT`）⇒ 跳过。
        let loopvars: Vec<SlotId> = match &init {
            Some(Stmt::ShortDecl { slots, .. }) => slots
                .iter()
                .copied()
                .filter(|s| *s != DISCARD_SLOT)
                .collect(),
            _ => Vec::new(),
        };
        let cond = f.cond.as_ref().map(|c| self.lower_expr(c));
        let post = f
            .post
            .as_ref()
            .and_then(|s| self.lower_simple(s))
            .map(Box::new);
        let body = self.lower_block(&f.body);
        self.pop_scope();
        Stmt::For {
            init: init.map(Box::new),
            cond,
            post,
            body,
            label,
            loopvars,
        }
    }

    fn lower_range(&mut self, rc: &RangeClause, f: &ForStmt, label: Option<String>) -> Stmt {
        let x = self.lower_expr(&rc.x);
        let xt = x.typ;
        let kind = self.range_kind(xt, rc.pos);
        let mut key = None;
        let mut value = None;
        if !rc.lhs.is_empty() {
            let t = self.range_types(xt, kind);
            for (i, l) in rc.lhs.iter().enumerate() {
                let typ = t.get(i).copied().unwrap_or_else(|| self.s.tt.invalid());
                if rc.is_define {
                    let name = match l {
                        AExpr::Name(n) => n.value.clone(),
                        _ => String::new(),
                    };
                    let slot = if name.is_empty() || name == "_" {
                        DISCARD_SLOT
                    } else {
                        self.declare(&name, typ)
                    };
                    let p = Place::Slot(slot);
                    if i == 0 {
                        key = Some(p);
                    } else {
                        value = Some(p);
                    }
                } else {
                    let p = self.lower_place(l);
                    if i == 0 {
                        key = Some(p);
                    } else {
                        value = Some(p);
                    }
                }
            }
        }
        let body = self.lower_block(&f.body);
        Stmt::Range {
            key,
            value,
            x,
            body,
            kind,
            define: rc.is_define,
            label,
        }
    }

    fn range_kind(&mut self, t: TypeId, pos: Pos) -> RangeKind {
        #[derive(Clone, Copy)]
        enum K {
            Arr,
            Sl,
            Map,
            Str,
            Int,
            Sig,
            Unknown,
        }
        let k = {
            let tt = &*self.s.tt;
            let mut cur = unalias(tt, t);
            if let Type::Pointer(e) = tt.type_of(cur) {
                cur = unalias(tt, *e);
            }
            match tt.type_of(underlying(tt, cur)) {
                Type::Array { .. } => K::Arr,
                Type::Slice(_) => K::Sl,
                Type::Map { .. } => K::Map,
                // `UntypedString` 也要算：`for i, r := range "ab"` 的 x 是
                // **无类型字符串常量**，只认 `String` 会让整条 `range` 落到
                // `K::Unknown` ⇒ 「range 的遍历形态无法判定」（实测）。
                Type::Basic(BasicId::String | BasicId::UntypedString) => K::Str,
                Type::Basic(
                    BasicId::Int
                    | BasicId::Int8
                    | BasicId::Int16
                    | BasicId::Int32
                    | BasicId::Int64
                    | BasicId::Uint
                    | BasicId::Uint8
                    | BasicId::Uint16
                    | BasicId::Uint32
                    | BasicId::Uint64
                    | BasicId::Uintptr
                    | BasicId::UntypedInt
                    // 无类型 rune 常量也是整数 range：`for i := range 'a'`
                    // （`range3.go` 的 `testint5`，规范要求 `i` 是 `rune`）。
                    | BasicId::UntypedRune,
                ) => K::Int,
                Type::Signature(_) => K::Sig,
                _ => K::Unknown,
            }
        };
        match k {
            K::Arr => RangeKind::Array,
            K::Sl => RangeKind::Slice,
            K::Map => RangeKind::Map,
            K::Str => RangeKind::Str,
            K::Int => RangeKind::Int,
            K::Sig => {
                self.reject(
                    pos,
                    RejectKind::RangeFunc,
                    "range over func 不在 M5 面（U-M5-06）",
                );
                RangeKind::Int
            }
            K::Unknown => {
                self.reject(pos, RejectKind::MissingType, "range 的遍历形态无法判定");
                RangeKind::Int
            }
        }
    }

    fn range_types(&mut self, t: TypeId, kind: RangeKind) -> Vec<TypeId> {
        let int = self.s.tt.add_basic(BasicId::Int);
        match kind {
            RangeKind::Array | RangeKind::Slice => {
                let elem = {
                    let tt = &*self.s.tt;
                    let mut cur = unalias(tt, t);
                    if let Type::Pointer(e) = tt.type_of(cur) {
                        cur = unalias(tt, *e);
                    }
                    match tt.type_of(underlying(tt, cur)) {
                        Type::Array { elem, .. } | Type::Slice(elem) => *elem,
                        _ => t,
                    }
                };
                vec![int, elem]
            }
            RangeKind::Str => {
                let rune = self.s.tt.add_basic(BasicId::Int32);
                vec![int, rune]
            }
            RangeKind::Map => {
                let (k, v) = {
                    let tt = &*self.s.tt;
                    match tt.type_of(underlying(tt, t)) {
                        Type::Map { key, elem } => (*key, *elem),
                        _ => (t, t),
                    }
                };
                vec![k, v]
            }
            // 整数 range（Go 1.22）：迭代变量与**被遍历的整数同型**。
            // 规范「For statements with range clause」：`for i := range n` 的
            // `i` 类型就是 `n` 的类型；`n` 是无类型常量时取默认型
            // （`range 'a'` ⇒ `rune`，`range 4` ⇒ `int`）。
            // 一律给 `int` 会让 `i != j`（`j` 是 `MyInt`）变成 mismatched types
            // （`range3.go` 的 `testint3` / `testint5` 实测）。
            RangeKind::Int => {
                let t = self.default_type(t);
                vec![t, t]
            }
            // `for v := range c`：**只产出一个值**（通道没有下标）。
            // 这一步在 Task 13 落地（那条 Task 同时要判 `chan<- T` 上 range 是非法）。
            RangeKind::Chan => todo!("M8/Task 13：chan 的 range 只产出元素，不产出下标"),
            // 迭代器函数的产出类型由**签名**定（`seq` 一个、`seq2` 两个），
            // 不能在这里靠 `t` 推。归 Task 45。
            RangeKind::Func => todo!("M8/Task 45：迭代器的产出类型由签名决定"),
        }
    }

    fn lower_switch(&mut self, sw: &SwitchStmt) -> Option<Stmt> {
        self.push_scope();
        let label = self.pending_label.take();
        let init = sw.init.as_ref().and_then(|s| self.lower_simple(s));
        // 类型 switch：tag 是 TypeSwitchGuard
        if let Some(AExpr::TypeSwitchGuard(g)) = &sw.tag {
            let x = self.lower_expr(&g.x);
            let mut cases = Vec::new();
            for c in &sw.cases {
                let (types, is_default, is_nil) = self.case_types(c);
                let bind_typ = match types.len() {
                    1 => types[0],
                    _ => x.typ,
                };
                let bind = g.lhs.as_ref().map(|n| {
                    if n.value == "_" {
                        DISCARD_SLOT
                    } else {
                        self.declare(&n.value, bind_typ)
                    }
                });
                self.push_scope();
                let body = self.lower_case_body(&c.body);
                self.pop_scope();
                cases.push(TypeCase {
                    types,
                    body,
                    is_default,
                    is_nil,
                    bind,
                    bind_typ,
                });
            }
            self.pop_scope();
            return Some(Stmt::TypeSwitch {
                init: init.map(Box::new),
                x,
                cases,
                label,
            });
        }
        let tag = sw.tag.as_ref().map(|t| self.lower_expr(t));
        let mut cases = Vec::new();
        for c in &sw.cases {
            let exprs: Vec<Expr> = match &c.cases {
                Some(AExpr::List(l)) => l.elems.iter().map(|e| self.lower_expr(e)).collect(),
                Some(e) => vec![self.lower_expr(e)],
                None => Vec::new(),
            };
            self.push_scope();
            let body = self.lower_case_body(&c.body);
            self.pop_scope();
            cases.push(Case {
                exprs,
                body,
                is_default: c.cases.is_none(),
            });
        }
        self.pop_scope();
        Some(Stmt::Switch {
            init: init.map(Box::new),
            tag,
            cases,
            label,
        })
    }

    fn lower_case_body(&mut self, body: &[AStmt]) -> Block {
        let mut stmts = Vec::new();
        for s in body {
            if let Some(st) = self.lower_stmt(s) {
                stmts.push(st);
            }
        }
        Block { stmts }
    }

    fn case_types(&mut self, c: &CaseClause) -> (Vec<TypeId>, bool, bool) {
        let Some(e) = &c.cases else {
            return (Vec::new(), true, false);
        };
        let list: Vec<&AExpr> = match e {
            AExpr::List(l) => l.elems.iter().collect(),
            other => vec![other],
        };
        let mut types = Vec::new();
        let mut is_nil = false;
        for t in list {
            if matches!(t, AExpr::Name(n) if n.value == "nil") {
                is_nil = true;
                continue;
            }
            match self.type_in_body(t) {
                Some(tid) => types.push(tid),
                None => self.reject(
                    t.pos(),
                    RejectKind::MissingType,
                    "type switch 的 case 不是类型",
                ),
            }
        }
        (types, false, is_nil)
    }

    // ───────────────────────── 表达式 ─────────────────────────

    fn type_of(&mut self, e: &AExpr) -> TypeId {
        // 空白标识符**没有类型**：`_ = x` 的左侧、`_, ok := m[k]` 的第一位
        // 都是丢弃目标。检查器对它给 `invalid` 是正确行为，不是 D-M5-2 失效。
        if let AExpr::Name(n) = e
            && n.value == "_"
        {
            return self.s.tt.invalid();
        }
        // 省略类型的复合字面量：类型来自外层元素类型（`Lowerer::hint`）。
        // 不能落到下面的 `expr_in`：没有提示时检查器只会报
        // `missing type in composite literal`。
        if let AExpr::CompositeLit(cl) = e
            && cl.type_.is_none()
            && let Some(h) = self.hint
        {
            return h;
        }
        let env = self.check_env();
        let op = self.s.expr_in(self.file, e, &env);
        if let Some(t) = op.typ {
            return t;
        }
        // `println(...)` / `f()`（无返回值）在检查器里是 **no-value 操作数**，
        // 它**本来就没有类型** —— 这不是 `D-M5-2` 的失效，而是「无值」。
        // 给它一个 `Invalid` 占位：`Stmt::Expr` 的求值只关心副作用，
        // 结果会被丢弃，没有任何一处读它的 `typ`。
        //
        // 若把这一支也报成 `HIR-MISSING-TYPE`，所有 `println("x")` 语句都会
        // 让 lowering 失败 —— 那不是「检查器没给类型」，是「这个表达式没有值」。
        if op.mode == Mode::NoValue {
            return self.s.tt.invalid();
        }
        self.reject(
            e.pos(),
            RejectKind::MissingType,
            format!(
                "拿不到表达式 {} 的静态类型（mode={}；D-M5-2 失效条件）",
                expr_kind_name(e),
                op.mode.as_str()
            ),
        );
        self.s.tt.invalid()
    }

    /// **函数体内**的类型表达式 → `TypeId`（带局部作用域）。
    ///
    /// 与 [`Session::type_expr_of`] 的差别只有一个：`env` 非空。函数体里的
    /// `var x X` / `new(T)` / `make([]T, n)` / `.(T)` 都可能引用**局部具名
    /// 类型**，只喂包级作用域会全部判成「不是类型」（`closure2.go` 实测）。
    fn type_in_body(&mut self, t: &AExpr) -> Option<TypeId> {
        let env = self.check_env();
        self.s.type_expr_in(self.file, t, &env)
    }

    fn lower_expr(&mut self, e: &AExpr) -> Expr {
        let typ = self.type_of(e);
        // 常量表达式在 lowering 期就折叠成宿主值。
        //
        // 检查器算常量用的是**任意精度**（`rgoc-const` 的 `BigInt`/`BigRat`）；
        // 留到解释器按有限宽度算会算错：`-128 & 0xffffffffffffffff` 在 i128
        // 里是 `18446744073709551488`，但若结果被标成 `int` 再按 64 位有符号
        // 规范化就变成 `-128`（`intcvt.go` 实测的正是这一条）。
        //
        // `BasicLit` 不走这里：它有更直接的 `const_of_lit`，避免两套字面量
        // 解析路径给出不同结果。
        //
        // 省略类型的复合字面量也不走这里：它不是常量，而 `folded_const`
        // 会用**无提示**的 `expr_in` 求值一遍 —— 检查器随即报
        // `missing type in composite literal`（`align.go` 实测）。
        // 折叠出的常量必须**对齐到操作数的静态类型**：`s2 == 45` 里 `45` 的
        // 类型是 `float64`（检查器做了隐式转换），但字面量本身是整数常量，
        // `const_of` 会给出 `Int(45)`。`Value::Int(45) != Value::Float(45.0)`，
        // 于是 `s2 == 45` 判成 false（`simpconv.go` / `g3.go` 实测）。
        // `retag_const` 只改宿主形态、不改数值，正好是这里需要的。
        let hintless_lit = matches!(e, AExpr::CompositeLit(cl) if cl.type_.is_none());
        if !matches!(e, AExpr::BasicLit(_))
            && !hintless_lit
            && let Some(c) = self.folded_const(e)
        {
            let c = self.retag_const(c, typ);
            return Expr::new(ExprKind::Const(c), typ);
        }
        match e {
            AExpr::Paren(p) => {
                let mut inner = self.lower_expr(&p.x);
                inner.typ = typ;
                inner
            }
            AExpr::BasicLit(l) => {
                let c = self.const_of_lit(l, typ);
                let c = self.retag_const(c, typ);
                Expr::new(ExprKind::Const(c), typ)
            }
            AExpr::Name(n) => self.lower_name(n, typ),
            AExpr::Unary(o) => {
                // `&x` 与 `*x` 不是「一元运算」，是地址与解引用：必须落成
                // `Addr(place)` / `Deref(expr)`，否则解释器拿到的是 `x` 的**值**，
                // `p := &a; p[1] = 55` 会静默写进副本（别名丢失）。
                if o.op == Op::And {
                    // `&T{...}`：复合字面量是规范里唯一「非位置但可寻址」的
                    // 表达式，没有 `Place` 可指 ⇒ 单独一支（求值后装箱成指针）。
                    if matches!(&*o.x, AExpr::CompositeLit(_)) {
                        let v = self.lower_expr(&o.x);
                        return Expr::new(ExprKind::AddrOf(Box::new(v)), typ);
                    }
                    let p = self.lower_place(&o.x);
                    return Expr::new(ExprKind::Addr(p), typ);
                }
                if o.op == Op::Mul {
                    let x = self.lower_expr(&o.x);
                    return Expr::new(ExprKind::Deref(Box::new(x)), typ);
                }
                let x = self.lower_expr(&o.x);
                let op = match o.op {
                    Op::Sub => UnOp::Neg,
                    Op::Add => UnOp::Pos,
                    Op::Not => UnOp::Not,
                    Op::Xor => UnOp::BitNot,
                    Op::Arrow => {
                        self.reject(o.pos, RejectKind::Chan, "接收操作不在 M5 面（U-M5-05）");
                        UnOp::Pos
                    }
                    _ => UnOp::Pos,
                };
                Expr::new(ExprKind::Unary { op, x: Box::new(x) }, typ)
            }
            AExpr::Binary(o) => {
                let rhs_ast = o.y.as_ref().expect("二元运算必有右操作数");
                let mut x = self.lower_expr(&o.x);
                let mut y = self.lower_expr(rhs_ast);
                // 无类型常量在二元表达式里会被检查器隐式转换到另一侧的类型
                // （`match_types` → `convert_untyped`），但那一步只改
                // `binary_core` 的局部副本。lowering 单独求值子表达式时看到的
                // 还是 `untyped int`，于是 `s2 == 45`（`s2` 是 `float64`）
                // 折出 `Int(45)`，与 `Float(45.0)` 比较永远不等
                // （`ken/simpconv.go`、`g3.go` 实测）。
                //
                // 这里把转换**交回检查器**（`Session::expr_as`），
                // lowering 只读转换后的 `Operand` —— 规则不复制、不猜测。
                //
                // 移位例外：Go 不把左操作数对齐到计数类型（见 `match_const_side`）。
                if !matches!(o.op, Op::Shl | Op::Shr) {
                    self.match_const_side(&o.x, rhs_ast, &mut x, &mut y);
                }
                let op = binop(o.op);
                Expr::new(
                    ExprKind::Binary {
                        op,
                        x: Box::new(x),
                        y: Box::new(y),
                    },
                    typ,
                )
            }
            AExpr::Call(c) => self.lower_call(c, typ),
            AExpr::Index(ix) => {
                // 泛型实例化 `f[T]` / `T[P]`
                let iop_env = self.check_env();
                let iop = self.s.expr_in(self.file, &ix.index, &iop_env);
                if iop.mode == Mode::TypeExpr {
                    self.reject(
                        ix.pos,
                        RejectKind::Generic,
                        "泛型实例化不在 M5 面（U-M5-10）",
                    );
                    return Expr::new(ExprKind::Nil, typ);
                }
                let base = self.lower_expr(&ix.x);
                let index = self.lower_expr(&ix.index);
                Expr::new(
                    ExprKind::Index {
                        base: Box::new(base),
                        index: Box::new(index),
                        comma_ok: false,
                    },
                    typ,
                )
            }
            AExpr::Slice(sx) => {
                let mut base = self.lower_expr(&sx.x);
                // 切**数组值**必须切它自己的存储，不能切副本：
                // `var a [80]int; setpd(a[0:]); sumpd(a[0:])` 里 `setpd` 写的是
                // `a` 本身（Go 规范：`a[lo:hi]` 对可寻址数组相当于 `&a` 上的切片）。
                // 若按值取 `a`，写进临时副本，`sumpd` 读到全 0
                // （`ken/array.go` 的 `lb=0; hb=80; t=0` 实测）。
                //
                // 落成 `Addr(place)` ⇒ `*[N]T`，解释器切片时沿指针取到原数组的
                // `Rc<Vec<Cell>>`，共享同一份底层存储。
                if self.is_array(base.typ) && is_addr_expr(&sx.x) {
                    let p = self.lower_place(&sx.x);
                    base = Expr::new(ExprKind::Addr(p), self.s.tt.add_pointer(base.typ));
                }
                let lo = sx.low.as_ref().map(|e| Box::new(self.lower_expr(e)));
                let hi = sx.high.as_ref().map(|e| Box::new(self.lower_expr(e)));
                let max = sx.max.as_ref().map(|e| Box::new(self.lower_expr(e)));
                Expr::new(
                    ExprKind::Slice {
                        base: Box::new(base),
                        lo,
                        hi,
                        max,
                    },
                    typ,
                )
            }
            AExpr::Selector(se) => self.lower_selector(se, typ),
            AExpr::CompositeLit(cl) => self.lower_composite(cl, typ),
            AExpr::FuncLit(fl) => {
                let fid = self.lower_funclit(fl);
                Expr::new(ExprKind::FuncLit(fid), typ)
            }
            AExpr::TypeAssert(a) => {
                let x = self.lower_expr(&a.x);
                let to = a
                    .type_
                    .as_ref()
                    .and_then(|t| self.type_in_body(t))
                    .unwrap_or_else(|| self.s.tt.invalid());
                Expr::new(
                    ExprKind::TypeAssert {
                        x: Box::new(x),
                        to,
                        comma_ok: false,
                    },
                    typ,
                )
            }
            AExpr::List(_) => {
                self.reject(e.pos(), RejectKind::MissingType, "列表表达式不能单独求值");
                Expr::new(ExprKind::Nil, typ)
            }
            AExpr::Bad(_) => {
                self.reject(e.pos(), RejectKind::MissingType, "错误的表达式节点");
                Expr::new(ExprKind::Nil, typ)
            }
            _ => {
                self.reject(
                    e.pos(),
                    RejectKind::MissingType,
                    "这个表达式形态不在 M5 的 lowering 面",
                );
                Expr::new(ExprKind::Nil, typ)
            }
        }
    }

    fn lower_name(&mut self, n: &Name, typ: TypeId) -> Expr {
        if n.value == "nil" {
            return Expr::new(ExprKind::Nil, typ);
        }
        if let Some(local) = self.resolve(&n.value) {
            return Expr::new(ExprKind::Slot(local.slot), typ);
        }
        // 局部常量在**局部变量之后、包级变量之前**：Go 的作用域规则里同层的
        // 常量与变量共用一个名字空间，而局部的要遮住包级的（`const8.go` 的
        // `iota = iota` 就是遮住预声明的那个）。
        if let Some((_, v)) = self.resolve_const(&n.value)
            && !matches!(v, Const::Unknown)
        {
            return Expr::new(ExprKind::Const(const_of(&v, typ)), typ);
        }
        if let Some(g) = self.globals_by_name.get(&n.value) {
            return Expr::new(ExprKind::Global(*g), typ);
        }
        // 常量（包级 const）
        if let Some(obj) = self.pkg.lookup(&n.value)
            && obj.kind == rgoc_types::ObjKind::Const
            && let Some(v) = &obj.val
        {
            return Expr::new(ExprKind::Const(const_of(v, typ)), typ);
        }
        if let Some(fid) = self.funcs_by_name.get(&n.value) {
            return Expr::new(ExprKind::FuncLit(*fid), typ);
        }
        // `true` / `false` 是 universe 常量，走上面的常量分支就够；到这里说明
        // 名字既不是局部也不是包级对象 —— 拒绝（不猜）。
        self.reject(
            n.pos,
            RejectKind::MissingType,
            format!("名字 {} 无法在 lowering 期解析", n.value),
        );
        Expr::new(ExprKind::Nil, typ)
    }

    fn lower_selector(&mut self, se: &rgoc_ast::SelectorExpr, typ: TypeId) -> Expr {
        // 方法表达式（`T.M` / `(*T).M` / `I.M`）先判：它的 `x` 是**类型**，
        // 不是值 —— 走下面的取值路径会把 `T` 当成变量名去解析
        // （`method.go` 的 `S.val(s)` 实测报「名字 S 无法解析」）。
        //
        // 语义：`T.M` 是一个函数值，**首参是接收者**。本仓的方法体里接收者
        // 本来就占第 0 个形参槽（`lower_func`），所以这里落成 `FuncLit(fid)`
        // 正好等价，不需要新节点。
        let env = self.check_env();
        let xop = self.s.expr_in(self.file, &se.x, &env);
        if xop.mode == Mode::TypeExpr {
            let base = xop.typ.unwrap_or_else(|| self.s.tt.invalid());
            if self.is_iface(base) {
                // 接口方法表达式 `I.M`：方法表按**动态类型**查，落成普通函数值
                // 需要一层「首参是接口」的包装，M5 不做（`U-M5-13`）。
                self.reject(
                    se.pos,
                    RejectKind::MissingType,
                    format!("接口方法表达式 I.{} 不在 M5 面（U-M5-13）", se.sel.value),
                );
                return Expr::new(ExprKind::Nil, typ);
            }
            if let Some(fid) = self.find_method(base, &se.sel.value) {
                return Expr::new(ExprKind::FuncLit(fid), typ);
            }
            self.reject(
                se.pos,
                RejectKind::MissingType,
                format!("类型上没有方法 {}", se.sel.value),
            );
            return Expr::new(ExprKind::Nil, typ);
        }
        let base_typ = self.type_of(&se.x);
        // 包名选择器：`pkg.X`
        if let AExpr::Name(n) = &*se.x
            && self.resolve(&n.value).is_none()
            && !self.globals_by_name.contains_key(&n.value)
            && self.pkg.imported(self.file, &n.value).is_some()
        {
            self.reject(
                se.pos,
                RejectKind::MissingType,
                format!("跨包引用 {}.{} 不在 M5 面（单包）", n.value, se.sel.value),
            );
            return Expr::new(ExprKind::Nil, typ);
        }
        let recv = self.lower_expr(&se.x);
        // 接口方法值
        if self.is_iface(base_typ) {
            return Expr::new(
                ExprKind::Bound {
                    recv: Box::new(recv),
                    method: MethodRef::Iface(se.sel.value.clone()),
                },
                typ,
            );
        }
        // 方法值
        if let Some((fid, path)) = self.find_method_path(base_typ, &se.sel.value) {
            let recv = self.recv_expr_path(&se.x, fid, &path);
            return Expr::new(
                ExprKind::Bound {
                    recv: Box::new(recv),
                    method: MethodRef::Func(fid),
                },
                typ,
            );
        }
        // 字段
        if let Some(path) = self.find_field(base_typ, &se.sel.value) {
            let mut e = recv;
            for i in path {
                e = Expr::new(
                    ExprKind::Field {
                        base: Box::new(e),
                        index: i,
                    },
                    typ,
                );
            }
            return e;
        }
        self.reject(
            se.pos,
            RejectKind::MissingType,
            format!("选择器 .{} 在 lowering 期找不到字段或方法", se.sel.value),
        );
        Expr::new(ExprKind::Nil, typ)
    }

    fn lower_call(&mut self, c: &CallExpr, typ: TypeId) -> Expr {
        let env = self.check_env();
        let fop = self.s.expr_in(self.file, &c.fun, &env);
        // 转换：`T(x)`
        if fop.mode == Mode::TypeExpr {
            let to = fop.typ.unwrap_or_else(|| self.s.tt.invalid());
            let x = match c.args.first() {
                Some(a) => self.lower_expr(a),
                None => Expr::new(ExprKind::Nil, to),
            };
            return Expr::new(ExprKind::Convert { to, x: Box::new(x) }, to);
        }
        // 内建：`make` / `new` 的实参里有**类型**，不能当值 lower，先分出去。
        if fop.mode == Mode::Builtin
            && let AExpr::Name(n) = &*c.fun
        {
            match n.value.as_str() {
                "make" => return self.lower_make(c, typ),
                "new" => return self.lower_new(c, typ),
                "close" => {
                    self.reject(c.pos, RejectKind::Close, "close 不在 M5 面（U-M5-12）");
                    return Expr::new(ExprKind::Nil, typ);
                }
                other => {
                    if let Some(kind) = builtin_kind(other) {
                        return self.lower_builtin_call(kind, c, typ);
                    }
                }
            }
        }
        // 接口方法表达式**调用**：`I.M(x, args…)`。`se.x` 是接口类型 ⇒ 第 0 个
        // 实参就是接收者，其余照常。落成 `Callee::Iface`（按动态类型查方法表），
        // 与 `v.M(args…)` 的唯一差别是接收者来自实参而不是 `se.x`。
        if let AExpr::Selector(se) = &*c.fun {
            let env = self.check_env();
            let xop = self.s.expr_in(self.file, &se.x, &env);
            if xop.mode == Mode::TypeExpr {
                let base = xop.typ.unwrap_or_else(|| self.s.tt.invalid());
                if self.is_iface(base)
                    && let Some(first) = c.args.first()
                {
                    let recv = self.lower_expr(first);
                    let recv = self.coerce(recv, base);
                    let rest: Vec<Expr> = c.args[1..].iter().map(|a| self.lower_expr(a)).collect();
                    return Expr::new(
                        ExprKind::Call {
                            callee: Callee::Iface {
                                recv: Box::new(recv),
                                name: se.sel.value.clone(),
                            },
                            args: rest,
                            variadic: c.has_dots,
                            spread: 0,
                        },
                        typ,
                    );
                }
            }
        }
        // 方法 / 函数值 / 直接调用
        let callee = self.lower_callee(c, &fop);
        let arg_types = self.callee_param_types(&callee);
        let mut args: Vec<Expr> = c.args.iter().map(|a| self.lower_expr(a)).collect();
        // `f(g())`：唯一实参是多值调用 ⇒ `g` 的多个结果直接充当 `f` 的实参列表
        // （Go 规范 Calls：`f(g())` 里 `g` 的结果必须正好匹配 `f` 的形参）。
        // `simassign.go` 的 `swap(swap(a, b))` 走的就是这条路。
        //
        // HIR 的 `Expr` 是单值的，没有「一个表达式占多个实参位」的节点，
        // 所以只记**个数**（`ExprKind::Call::spread`）：求值栈上确实会多出
        // 这么多个值，解释器按它切分实参，而不是按 `args.len()`。
        let spread = match (c.args.as_slice(), args.as_slice()) {
            ([only], [lowered]) => match self.call_result_types(only, lowered).len() {
                0 | 1 => 0,
                n => u8::try_from(n).unwrap_or(u8::MAX),
            },
            _ => 0,
        };
        // 展开时不能按位置收敛：`arg_types[0]` 是**第一个**形参，而这个实参
        // 对应的是整串形参，逐位 `coerce` 会把它整体转成第一个形参的类型。
        if !c.has_dots && spread == 0 {
            // 变参被调方：末尾那一段实参要收敛到**元素**类型，不是整个 slice 类型。
            //
            // 踩过：`sum(1, 2, 3)` 里 `arg_types[0]` 是 `[]int`，逐位收敛会把 `1`
            // 标成 `[]int` —— 解释器不看静态类型所以一直没露；SSA 的 `V3` 会当场拒
            // （`常量 Int(1) 不可能有类型 []int`）。这是 M5 留在 HIR 里的一个真错。
            let var_elem = if !c.has_dots && self.callee_is_variadic(&callee) {
                arg_types.last().copied().and_then(|t| self.slice_elem(t))
            } else {
                None
            };
            let mut targets: Vec<TypeId> = Vec::new();
            let fixed = arg_types.len().saturating_sub(1);
            for (i, a) in args.iter().enumerate() {
                let t = match var_elem {
                    Some(elem) if i >= fixed => elem,
                    _ => arg_types.get(i).copied().unwrap_or(a.typ),
                };
                targets.push(t);
            }
            self.coerce_list(&mut args, &targets);
        }
        Expr::new(
            ExprKind::Call {
                callee,
                args,
                variadic: c.has_dots,
                spread,
            },
            typ,
        )
    }

    fn lower_builtin_call(&mut self, kind: BuiltinKind, c: &CallExpr, typ: TypeId) -> Expr {
        let args: Vec<Expr> = c.args.iter().map(|a| self.lower_expr(a)).collect();
        Expr::new(
            ExprKind::Builtin {
                kind,
                args,
                variadic: c.has_dots,
            },
            typ,
        )
    }

    fn lower_new(&mut self, c: &CallExpr, typ: TypeId) -> Expr {
        // `new(T)`：分配一个零值。
        if let Some(pointee) = c.args.first().and_then(|a| self.type_in_body(a)) {
            return Expr::new(ExprKind::New(pointee), typ);
        }
        // `new(expr)`（Go 1.26 起，`newexpr.go` 实测）：分配一个**装着该值**的
        // 单元。语义与 `&expr` 的「物化临时」完全一致 —— 解释器的 `Task::AddrOf`
        // 就是「新分配一个 `Cell` 装住刚求出的值」，所以直接落成 `AddrOf`，
        // 不新增节点。
        if let Some(a) = c.args.first() {
            let v = self.lower_expr(a);
            return Expr::new(ExprKind::AddrOf(Box::new(v)), typ);
        }
        Expr::new(ExprKind::New(typ), typ)
    }

    fn lower_make(&mut self, c: &CallExpr, typ: TypeId) -> Expr {
        let target = c
            .args
            .first()
            .and_then(|a| self.type_in_body(a))
            .unwrap_or(typ);
        let kind = {
            let tt = &*self.s.tt;
            match tt.type_of(underlying(tt, target)) {
                Type::Slice(_) => MakeKind::Slice,
                Type::Map { .. } => MakeKind::Map,
                Type::Chan { .. } => MakeKind::Chan,
                _ => MakeKind::Map,
            }
        };
        let args: Vec<Expr> = c.args.iter().skip(1).map(|a| self.lower_expr(a)).collect();
        Expr::new(ExprKind::Make { kind, args }, target)
    }

    /// 方法接收者：按方法声明的接收者类型补齐 `&` / `*`（Go 的自动取址与解引用）。
    ///
    /// 为什么必须在 lowering 期做：`func (p *T) M()` 以 `v.M()` 调用时，`v` 的
    /// **值**不足以调 `M` —— 需要 `&v`。解释器拿到的是「一个已经求过值的
    /// `Expr`」，那时地址已经丢了。`Callee::Method.recv` 的类型与方法
    /// `param_types[0]` 不一致，解释器只能报错或者静默算错，两者都不可接受。
    ///
    /// 带**嵌入提升链**的接收者表达式。
    ///
    /// `path` 非空 ⇒ 方法声明在内层类型上，接收者要先沿嵌入字段走 `path`：
    /// `t1.M`（`M` 声明在 `T4`，`T1 ⊃ T2 ⊃ *T3 ⊃ *T4`）的接收者是
    /// `t1.T2.T3.T4` 而不是 `t1`。
    fn recv_expr_path(&mut self, ast: &AExpr, fid: FuncId, path: &[usize]) -> Expr {
        let mut e = self.lower_expr(ast);
        for &i in path {
            let ft = self
                .struct_fields(e.typ)
                .and_then(|fs| fs.get(i).map(|f| f.typ));
            let Some(ft) = ft else { break };
            e = Expr::new(
                ExprKind::Field {
                    base: Box::new(e),
                    index: i,
                },
                ft,
            );
        }
        let Some(want) = self
            .prog
            .funcs
            .get(fid.index())
            .and_then(|f| f.param_types.first().copied())
        else {
            return e;
        };
        let (want_ptr, have_ptr) = {
            let tt = &*self.s.tt;
            (
                matches!(tt.type_of(unalias(tt, want)), Type::Pointer(_)),
                matches!(tt.type_of(unalias(tt, e.typ)), Type::Pointer(_)),
            )
        };
        if want_ptr && !have_ptr {
            // 提升链上要取址时，地址必须来自**字段链**，不是外层对象
            // （`&t1` 的类型是 `*T1`，而方法要的是 `*T4`）。
            if !path.is_empty() {
                let mut p = self.lower_place(ast);
                let mut cur = e.typ;
                for &i in path {
                    p = Place::Field {
                        base: Box::new(p),
                        index: i,
                    };
                    cur = self
                        .struct_fields(cur)
                        .and_then(|fs| fs.get(i).map(|f| f.typ))
                        .unwrap_or(cur);
                }
                return Expr::new(ExprKind::Addr(p), want);
            }
            if is_addr_expr(ast) {
                let p = self.lower_place(ast);
                return Expr::new(ExprKind::Addr(p), want);
            }
            return e;
        }
        if !want_ptr && have_ptr {
            return Expr::new(ExprKind::Deref(Box::new(e)), want);
        }
        e
    }

    fn lower_callee(&mut self, c: &CallExpr, fop: &Operand) -> Callee {
        if let AExpr::Selector(se) = &*c.fun {
            // 方法表达式**调用**：`T.M(recv, args…)`。`se.x` 是类型 ⇒ 接收者
            // 是**实参**（第 0 个），不是 `recv`。这里必须直接给出 `Func`，
            // 否则下面会去 `lower_expr(S)`（`method.go` / `method3.go` 实测
            // 报「名字 S 无法解析」）。
            {
                let env = self.check_env();
                let xop = self.s.expr_in(self.file, &se.x, &env);
                if xop.mode == Mode::TypeExpr {
                    let base = xop.typ.unwrap_or_else(|| self.s.tt.invalid());
                    if !self.is_iface(base)
                        && let Some(fid) = self.find_method(base, &se.sel.value)
                    {
                        return Callee::Func(fid);
                    }
                }
            }
            let base_typ = self.type_of(&se.x);
            if self.is_iface(base_typ) {
                let recv = self.lower_expr(&se.x);
                return Callee::Iface {
                    recv: Box::new(recv),
                    name: se.sel.value.clone(),
                };
            }
            if let Some((fid, path)) = self.find_method_path(base_typ, &se.sel.value) {
                let recv = self.recv_expr_path(&se.x, fid, &path);
                return Callee::Method {
                    recv: Box::new(recv),
                    func: fid,
                };
            }
            // 嵌入**接口字段**上的方法：`struct{ I }.Sum(...)`
            if let Some((path, _)) = self.find_iface_method_path(base_typ, &se.sel.value) {
                let recv = self.field_chain(&se.x, &path);
                return Callee::Iface {
                    recv: Box::new(recv),
                    name: se.sel.value.clone(),
                };
            }
            // 字段里的函数值：`x.f()`
            if let Some(path) = self.find_field(base_typ, &se.sel.value) {
                let mut e = self.lower_expr(&se.x);
                let typ = self.type_of(&se.x);
                for i in path {
                    e = Expr::new(
                        ExprKind::Field {
                            base: Box::new(e),
                            index: i,
                        },
                        typ,
                    );
                }
                return Callee::Value(Box::new(e));
            }
            // 包级函数
            if let Some(fid) = self.funcs_by_name.get(&se.sel.value) {
                return Callee::Func(*fid);
            }
        }
        if let AExpr::Name(n) = &*c.fun
            && self.resolve(&n.value).is_none()
            && !self.globals_by_name.contains_key(&n.value)
            && let Some(fid) = self.funcs_by_name.get(&n.value)
        {
            return Callee::Func(*fid);
        }
        let _ = fop;
        Callee::Value(Box::new(self.lower_expr(&c.fun)))
    }

    /// 被调方是不是变参函数（`...T`）。
    fn callee_is_variadic(&self, callee: &Callee) -> bool {
        let fid = match callee {
            Callee::Func(f) => *f,
            Callee::Method { func, .. } => *func,
            _ => return false,
        };
        self.prog.funcs.get(fid.index()).is_some_and(|f| f.variadic)
    }

    /// `[]T` 的元素类型（不是 slice ⇒ `None`）。
    fn slice_elem(&self, t: TypeId) -> Option<TypeId> {
        if t.index() >= self.s.tt.len() {
            return None;
        }
        match self.s.tt.type_of(rgoc_types::underlying(self.s.tt, t)) {
            rgoc_types::typ::Type::Slice(e) => Some(*e),
            _ => None,
        }
    }

    fn callee_param_types(&self, callee: &Callee) -> Vec<TypeId> {
        let (fid, skip_recv) = match callee {
            Callee::Func(f) => (*f, false),
            // 静态方法调用：`param_types[0]` 是**接收者**，实参从 1 开始。
            // 不剥掉的话 `b.Set(n)` 会拿接收者的类型（`*Box`）去收敛 `n`
            // （`*Integer`）——`coerce` 对指针型不插转换，接口实参就**没装箱**，
            // 于是 `b.v` 存进去的是裸指针而不是接口值，`b.v.Print()` 在接口
            // 分派处查不到方法（`ken/rob1.go` 实测）。
            Callee::Method { func, .. } => (*func, true),
            _ => return Vec::new(),
        };
        let mut ts = self
            .prog
            .funcs
            .get(fid.index())
            .map(|f| f.param_types.clone())
            .unwrap_or_default();
        if skip_recv && !ts.is_empty() {
            ts.remove(0);
        }
        ts
    }

    fn lower_funclit(&mut self, fl: &rgoc_ast::FuncLit) -> FuncId {
        let fid = FuncId(u32::try_from(self.prog.funcs.len()).unwrap_or(u32::MAX));
        let name = format!("func@{}:{}", fl.pos.line, fl.pos.col);
        self.prog.funcs.push(Func::placeholder(name));

        let mut ctx = FnCtx::new(fid);
        let mut params = Vec::new();
        let mut param_types = Vec::new();
        let mut results = Vec::new();
        let mut result_types = Vec::new();
        let mut named_results = false;
        let mut variadic = false;
        if let AExpr::FuncType(ft) = &*fl.type_ {
            for p in &ft.params {
                let (typ, dots) = self.param_type(&p.type_);
                variadic |= dots;
                let slot = ctx.alloc_t(typ);
                params.push(slot);
                param_types.push(typ);
                if let Some(n) = &p.name {
                    ctx.declare(&n.value, Local { slot, typ });
                }
            }
            for r in &ft.results {
                let typ = self
                    .s
                    .type_expr_of(self.file, &r.type_)
                    .unwrap_or_else(|| self.s.tt.invalid());
                let slot = ctx.alloc_t(typ);
                results.push(slot);
                result_types.push(typ);
                if let Some(n) = &r.name {
                    named_results = true;
                    ctx.declare(&n.value, Local { slot, typ });
                }
            }
        }
        self.stack.push(ctx);
        let saved_hint = self.hint.take();
        let body = self.lower_body_block(&fl.body, true);
        self.hint = saved_hint;
        let ctx = self.stack.pop().expect("funclit 的栈不平衡");

        let func = &mut self.prog.funcs[fid.index()];
        func.params = params;
        func.param_types = param_types;
        func.results = results;
        func.result_types = result_types;
        func.nslots = ctx.next_slot as usize;
        func.slot_types = ctx.slot_types;
        func.body = body;
        func.captured = ctx.captures;
        func.variadic = variadic;
        func.named_results = named_results;
        func.labels = ctx.labels;
        fid
    }

    fn lower_composite(&mut self, cl: &CompositeLit, typ: TypeId) -> Expr {
        enum Shape {
            Struct(Vec<rgoc_types::Field>),
            Seq(TypeId),
            Map(TypeId, TypeId),
            Other,
        }
        let shape = {
            let tt = &*self.s.tt;
            let mut cur = unalias(tt, typ);
            if let Type::Pointer(e) = tt.type_of(cur) {
                cur = unalias(tt, *e);
            }
            let und = underlying(tt, cur);
            match tt.type_of(und) {
                Type::Struct(sid) => Shape::Struct(tt.struct_def(*sid).fields.clone()),
                Type::Array { elem, .. } | Type::Slice(elem) => Shape::Seq(*elem),
                Type::Map { key, elem } => Shape::Map(*key, *elem),
                _ => Shape::Other,
            }
        };
        let mut items = Vec::new();
        match shape {
            Shape::Struct(fields) => {
                let mut next = 0usize;
                for el in &cl.elems {
                    match el {
                        AExpr::KeyValue(kv) => {
                            let key = match &*kv.key {
                                AExpr::Name(n) => n.value.clone(),
                                _ => String::new(),
                            };
                            let idx = fields
                                .iter()
                                .position(|f| f.name.as_deref() == Some(key.as_str()))
                                .unwrap_or(0);
                            let ft = fields.get(idx).map(|f| f.typ).unwrap_or(typ);
                            let v = self.lower_expr_hint(&kv.value, ft);
                            let v = self.coerce(v, ft);
                            items.push(CompositeItem::Field(idx, v));
                        }
                        other => {
                            let ft = fields.get(next).map(|f| f.typ).unwrap_or(typ);
                            let v = self.lower_expr_hint(other, ft);
                            let v = self.coerce(v, ft);
                            items.push(CompositeItem::Field(next, v));
                            next += 1;
                        }
                    }
                }
            }
            Shape::Seq(elem) => {
                let mut next = 0usize;
                for el in &cl.elems {
                    match el {
                        AExpr::KeyValue(kv) => {
                            let idx = self.const_index(&kv.key).unwrap_or(0);
                            let v = self.lower_expr_hint(&kv.value, elem);
                            let v = self.coerce(v, elem);
                            items.push(CompositeItem::Index(idx, v));
                            next = idx + 1;
                        }
                        other => {
                            let v = self.lower_expr_hint(other, elem);
                            let v = self.coerce(v, elem);
                            items.push(CompositeItem::Index(next, v));
                            next += 1;
                        }
                    }
                }
            }
            Shape::Map(key, elem) => {
                for el in &cl.elems {
                    if let AExpr::KeyValue(kv) = el {
                        let k = self.lower_expr_hint(&kv.key, key);
                        let k = self.coerce(k, key);
                        let v = self.lower_expr_hint(&kv.value, elem);
                        let v = self.coerce(v, elem);
                        items.push(CompositeItem::Key(k, v));
                    } else {
                        self.reject(el.pos(), RejectKind::MissingType, "map 字面量必须有键");
                    }
                }
            }
            Shape::Other => {
                self.reject(
                    cl.pos,
                    RejectKind::MissingType,
                    "复合字面量的类型不是 struct/array/slice/map",
                );
            }
        }
        Expr::new(ExprKind::Composite(Composite { typ, items }), typ)
    }

    fn lower_expr_hint(&mut self, e: &AExpr, hint: TypeId) -> Expr {
        let env = self.check_env();
        let op = self.s.expr_hint_in(self.file, e, Some(hint), &env);
        let typ = op.typ.unwrap_or(hint);
        // 把提示交给 `type_of`：省略类型的嵌套字面量查不到类型，只能取外层
        // 元素类型（见 `Lowerer::hint`）。只在这一次 lower 期间有效。
        let saved = self.hint.replace(typ);
        let mut out = self.lower_expr(e);
        self.hint = saved;
        out.typ = typ;
        out
    }

    fn const_index(&mut self, e: &AExpr) -> Option<usize> {
        let env = self.check_env();
        let op = self.s.expr_in(self.file, e, &env);
        match op.val {
            Some(Const::Int(b)) => b.to_dec().parse::<usize>().ok(),
            _ => None,
        }
    }

    // ───────────────────────── 位置 ─────────────────────────

    fn lower_place(&mut self, e: &AExpr) -> Place {
        match e {
            AExpr::Paren(p) => self.lower_place(&p.x),
            AExpr::Name(n) => {
                // 空白标识符：`_ = x` / `_, ok := m[k]` 里它是**丢弃目标**，
                // 不是「解析不到的名字」。不给它报 `左值 _ 无法解析`。
                if n.value == "_" {
                    return Place::Slot(DISCARD_SLOT);
                }
                if let Some(local) = self.resolve(&n.value) {
                    return Place::Slot(local.slot);
                }
                if let Some(g) = self.globals_by_name.get(&n.value) {
                    return Place::Global(*g);
                }
                self.reject(
                    n.pos,
                    RejectKind::MissingType,
                    format!("左值 {} 无法解析", n.value),
                );
                Place::Slot(DISCARD_SLOT)
            }
            AExpr::Selector(se) => {
                let base_typ = self.type_of(&se.x);
                if let Some(path) = self.find_field(base_typ, &se.sel.value) {
                    let mut p = self.lower_place(&se.x);
                    for i in path {
                        p = Place::Field {
                            base: Box::new(p),
                            index: i,
                        };
                    }
                    p
                } else {
                    self.reject(
                        se.pos,
                        RejectKind::MissingType,
                        format!("左值字段 .{} 找不到", se.sel.value),
                    );
                    Place::Slot(DISCARD_SLOT)
                }
            }
            AExpr::Index(ix) => {
                let base = self.lower_expr(&ix.x);
                let index = self.lower_expr(&ix.index);
                Place::Index {
                    base: Box::new(base),
                    index: Box::new(index),
                }
            }
            AExpr::Unary(o) if o.op == Op::Mul => {
                let x = self.lower_expr(&o.x);
                Place::Deref(Box::new(x))
            }
            // `(&a).f` 这种形态：`&a` 的**位置**就是 `a` 的位置（`&a` 与 `a`
            // 指同一个对象）。取址链再往上走字段时，走 `a` 与走 `&a` 等价 ——
            // 不加这一支，提升方法在 `(&a).g()`（`g` 是指针接收者、声明在
            // 内层 `C` 上）会报「不是可寻址位置」（`method.go` 实测）。
            AExpr::Unary(o) if o.op == Op::And => self.lower_place(&o.x),
            _ => {
                self.reject(e.pos(), RejectKind::MissingType, "不是可寻址位置");
                Place::Slot(DISCARD_SLOT)
            }
        }
    }

    // ───────────────────────── 收敛与常量 ─────────────────────────

    /// 常量表达式的宿主值（检查器已经算好了，**任意精度**）。
    ///
    /// `None` ⇒ 不是常量，或者值未知（`Const::Unknown`，例如依赖未求值的
    /// `iota` 或检查器拒过的表达式）—— 那两种都必须留到运行时。
    fn folded_const(&mut self, e: &AExpr) -> Option<ConstVal> {
        let env = self.check_env();
        let op = self.s.expr_in(self.file, e, &env);
        if op.mode != Mode::Constant {
            return None;
        }
        let v = op.val.as_ref()?;
        if matches!(v, Const::Unknown) {
            return None;
        }
        let typ = op.typ.unwrap_or_else(|| self.s.tt.invalid());
        Some(const_of(v, typ))
    }

    /// 把 `e` 收敛到 `to`：接口装箱 / 无类型 nil 落地 / 无类型常量默认型。
    fn coerce(&mut self, e: Expr, to: TypeId) -> Expr {
        // 常量：转换是**精确**的（Go 规范「常量转换」：值不变，只换类型；
        // 不可表示的情形检查器已经拒了）。所以只改标签、不动值 ——
        // 否则 `var u uint64 = 大常量` 会带着 `untyped int` 的类型标签走，
        // 打印时按 64 位有符号规范化就印错了（`intcvt.go`）。
        //
        // 但**宿主表示**要跟着换：`float64` 上下文里的 `45` 必须是
        // `ConstVal::Float(45.0)`，否则解释器拿 `Int(45)` 和 `Float(45.0)`
        // 比相等永远不等（`ken/simpconv.go` 的 `s2 != 45` 实测）。
        if !self.is_iface(to)
            && let ExprKind::Const(c) = e.kind
        {
            let c = self.retag_const(c, to);
            return Expr::new(ExprKind::Const(c), to);
        }
        if self.is_iface(to) {
            if self.is_iface(e.typ) {
                return e;
            }
            if matches!(e.kind, ExprKind::Nil) || self.is_nil_type(e.typ) {
                return Expr::new(ExprKind::Nil, to);
            }
            // 常量装箱：接口里的**动态类型**必须是默认类型 —— `untyped string`
            // 不是运行期类型。不换标签的话 `i.(string)` 会失败并报
            // 「I0 is untyped_string, not string」（`ken/interbasic.go` 实测）。
            let e = if let ExprKind::Const(c) = e.kind {
                let dt = self.default_type(e.typ);
                Expr::new(ExprKind::Const(self.retag_const(c, dt)), dt)
            } else {
                e
            };
            return Expr::new(ExprKind::Convert { to, x: Box::new(e) }, to);
        }
        if self.is_nil_type(e.typ) {
            return Expr::new(ExprKind::Nil, to);
        }
        e
    }

    /// 常量值跟着类型标签换宿主表示（值本身**不变**，只是表示形态）。
    ///
    /// 只处理「同一数值的不同宿主形态」：`Int` ↔ `Float` ↔ `Complex`。
    /// 整型宽度、`rune`/`byte` 都在 `Int` 里（宽度由标签决定，值精确），
    /// 字符串/布尔没有第二种形态。
    fn retag_const(&mut self, c: ConstVal, to: TypeId) -> ConstVal {
        let tt = &*self.s.tt;
        let t = unalias(tt, to);
        let Type::Basic(b) = tt.type_of(underlying(tt, t)) else {
            return c;
        };
        match (&c, b.is_float(), b.is_complex()) {
            (ConstVal::Int(i), true, _) => ConstVal::Float(*i as f64),
            (ConstVal::Int(i), _, true) => ConstVal::Complex(*i as f64, 0.0),
            (ConstVal::Float(f), _, true) => ConstVal::Complex(*f, 0.0),
            _ => c,
        }
    }

    /// 二元表达式的两侧：把**常量**的一侧对齐到另一侧的类型。
    ///
    /// 只处理「一侧是常量、另一侧不是」的情形 —— 两侧都是常量时整棵子树
    /// 早在 `lower_expr` 顶部被 `folded_const` 折叠掉，走不到这里。
    /// 两侧类型相同则直接返回，避免给每个 `i + 1` 都白跑一次检查器。
    ///
    /// 移位**不在此列**：Go 对移位只要求计数是整数，左操作数保持自己的类型
    /// （`1 << n` 的结果类型是 `int`，不是 `n` 的 `uint`）。若在这里把 `1`
    /// 转成 `uint`，整个移位就会以 `uint` 求值再被标成 `int`（`F3-D`）。
    fn match_const_side(&mut self, xa: &AExpr, ya: &AExpr, x: &mut Expr, y: &mut Expr) {
        if x.typ == y.typ {
            return;
        }
        let (tx, ty) = (x.typ, y.typ);
        let (xc, yc) = (
            matches!(x.kind, ExprKind::Const(_)),
            matches!(y.kind, ExprKind::Const(_)),
        );
        if xc && !yc {
            if let Some(e) = self.const_as(xa, ty) {
                *x = e;
            }
        } else if yc
            && !xc
            && let Some(e) = self.const_as(ya, tx)
        {
            *y = e;
        }
    }

    /// `e` 作为常量、按检查器的隐式转换落到 `to` 之后的表达式节点。
    ///
    /// `None` ⇒ 转换被检查器拒绝、`e` 其实不是常量、或值未知
    /// （`Const::Unknown`）—— 三种情形都保持原样，绝不硬改标签。
    fn const_as(&mut self, e: &AExpr, to: TypeId) -> Option<Expr> {
        let env = self.check_env();
        let op = self.s.expr_as(self.file, e, to, &env);
        if op.mode != Mode::Constant {
            return None;
        }
        let v = op.val.as_ref()?;
        if matches!(v, Const::Unknown) {
            return None;
        }
        let t = op.typ?;
        let c = const_of(v, t);
        Some(Expr::new(ExprKind::Const(self.retag_const(c, t)), t))
    }

    fn coerce_list(&mut self, es: &mut [Expr], targets: &[TypeId]) {
        for (i, e) in es.iter_mut().enumerate() {
            if let Some(t) = targets.get(i).copied() {
                let old = std::mem::replace(e, Expr::new(ExprKind::Nil, t));
                *e = self.coerce(old, t);
            }
        }
    }

    fn const_one(&mut self, typ: TypeId) -> Expr {
        Expr::new(ExprKind::Const(ConstVal::Int(1)), typ)
    }

    fn const_of_lit(&mut self, l: &rgoc_ast::BasicLit, typ: TypeId) -> ConstVal {
        let c = rgoc_const::const_from_literal(l);
        const_of(&c, typ)
    }
}

/// 表达式节点的语法类别名（只用于诊断文本 —— 让 `HIR-MISSING-TYPE` 能指认
/// 是哪种表达式拿不到类型，而不是只给一个列号）。
fn expr_kind_name(e: &AExpr) -> &'static str {
    match e {
        AExpr::Bad(_) => "Bad",
        AExpr::Name(_) => "Name",
        AExpr::BasicLit(_) => "BasicLit",
        AExpr::CompositeLit(_) => "CompositeLit",
        AExpr::KeyValue(_) => "KeyValue",
        AExpr::FuncLit(_) => "FuncLit",
        AExpr::Paren(_) => "Paren",
        AExpr::Selector(_) => "Selector",
        AExpr::Index(_) => "Index",
        AExpr::Slice(_) => "Slice",
        AExpr::TypeAssert(_) => "TypeAssert",
        AExpr::TypeSwitchGuard(_) => "TypeSwitchGuard",
        AExpr::Unary(_) => "Unary",
        AExpr::Binary(_) => "Binary",
        AExpr::Call(_) => "Call",
        AExpr::List(_) => "List",
        AExpr::ArrayType(_) => "ArrayType",
        AExpr::SliceType(_) => "SliceType",
        AExpr::DotsType(_) => "DotsType",
        AExpr::StructType(_) => "StructType",
        AExpr::InterfaceType(_) => "InterfaceType",
        AExpr::FuncType(_) => "FuncType",
        AExpr::MapType(_) => "MapType",
        AExpr::ChanType(_) => "ChanType",
    }
}

/// 常量 → 宿主表示（按静态类型收敛）。
fn const_of(c: &Const, typ: TypeId) -> ConstVal {
    let _ = typ;
    match c {
        Const::Int(b) => ConstVal::Int(big_to_i128(b)),
        Const::Float(r) => ConstVal::Float(rat_to_f64(r)),
        Const::Complex { re, im } => ConstVal::Complex(rat_to_f64(re), rat_to_f64(im)),
        Const::Str(s) => ConstVal::Str(String::from_utf8_lossy(s).into_owned()),
        Const::Bool(b) => ConstVal::Bool(*b),
        Const::Unknown => ConstVal::Int(0),
    }
}

/// `BigInt` → `i128`（`uint64` 全域装得下；超出则按位截断到 `i128`）。
fn big_to_i128(b: &rgoc_const::BigInt) -> i128 {
    let dec = b.to_dec();
    if let Ok(v) = dec.parse::<i128>() {
        return v;
    }
    // 超范围：取低 128 位（Go 侧已被可表示性判定拦住，这里是兜底）。
    let mut acc: i128 = 0;
    for ch in dec.trim_start_matches('-').bytes() {
        if ch.is_ascii_digit() {
            acc = acc.wrapping_mul(10).wrapping_add(i128::from(ch - b'0'));
        }
    }
    if dec.starts_with('-') {
        acc.wrapping_neg()
    } else {
        acc
    }
}

/// `BigRat` → `f64`。
fn rat_to_f64(r: &rgoc_const::BigRat) -> f64 {
    let num: f64 = r.num().to_dec().parse().unwrap_or(0.0);
    let den: f64 = r.den().to_dec().parse().unwrap_or(1.0);
    if den == 0.0 { 0.0 } else { num / den }
}

fn recv_base_name(e: &AExpr) -> Option<String> {
    match e {
        AExpr::Name(n) => Some(n.value.clone()),
        AExpr::Paren(p) => recv_base_name(&p.x),
        AExpr::Unary(o) if o.op == Op::Mul => recv_base_name(&o.x),
        AExpr::Index(i) => recv_base_name(&i.x),
        _ => None,
    }
}

fn is_loop_or_switch(s: &AStmt) -> bool {
    matches!(s, AStmt::For(_) | AStmt::Switch(_))
}

/// 这个 AST 表达式是否可能可寻址（`&x` 的候选）。
fn is_addr_expr(e: &AExpr) -> bool {
    match e {
        AExpr::Name(_) | AExpr::Selector(_) | AExpr::Index(_) => true,
        AExpr::Unary(o) => o.op == Op::Mul,
        AExpr::Paren(p) => is_addr_expr(&p.x),
        _ => false,
    }
}

fn builtin_kind(name: &str) -> Option<BuiltinKind> {
    Some(match name {
        "len" => BuiltinKind::Len,
        "cap" => BuiltinKind::Cap,
        "append" => BuiltinKind::Append,
        "copy" => BuiltinKind::Copy,
        "delete" => BuiltinKind::Delete,
        "clear" => BuiltinKind::Clear,
        "new" => BuiltinKind::New,
        "min" => BuiltinKind::Min,
        "max" => BuiltinKind::Max,
        "complex" => BuiltinKind::Complex,
        "real" => BuiltinKind::Real,
        "imag" => BuiltinKind::Imag,
        "print" => BuiltinKind::Print,
        "println" => BuiltinKind::Println,
        "panic" => BuiltinKind::Panic,
        "recover" => BuiltinKind::Recover,
        "close" => BuiltinKind::Close,
        "make" => return None, // make 走 Make 节点
        _ => return None,
    })
}

fn binop(op: Op) -> BinOp {
    match op {
        Op::Add => BinOp::Add,
        Op::Sub => BinOp::Sub,
        Op::Mul => BinOp::Mul,
        Op::Quo => BinOp::Div,
        Op::Rem => BinOp::Rem,
        Op::And => BinOp::And,
        Op::Or => BinOp::Or,
        Op::Xor => BinOp::Xor,
        Op::AndNot => BinOp::AndNot,
        Op::Shl => BinOp::Shl,
        Op::Shr => BinOp::Shr,
        Op::Eql => BinOp::Eq,
        Op::Neq => BinOp::Ne,
        Op::Lss => BinOp::Lt,
        Op::Leq => BinOp::Le,
        Op::Gtr => BinOp::Gt,
        Op::Geq => BinOp::Ge,
        Op::LAnd => BinOp::LAnd,
        Op::LOr => BinOp::LOr,
        Op::Not | Op::Arrow | Op::Tilde => BinOp::Add,
    }
}

fn assign_binop(op: AssignOp) -> Option<BinOp> {
    Some(match op {
        AssignOp::Assign | AssignOp::Define | AssignOp::Inc | AssignOp::Dec => return None,
        AssignOp::Add => BinOp::Add,
        AssignOp::Sub => BinOp::Sub,
        AssignOp::Mul => BinOp::Mul,
        AssignOp::Quo => BinOp::Div,
        AssignOp::Rem => BinOp::Rem,
        AssignOp::And => BinOp::And,
        AssignOp::Or => BinOp::Or,
        AssignOp::Xor => BinOp::Xor,
        AssignOp::Shl => BinOp::Shl,
        AssignOp::Shr => BinOp::Shr,
        AssignOp::AndNot => BinOp::AndNot,
    })
}
