//! 声明收集与求值顺序（M3/Task 24）。
//!
//! # 职责
//!
//! Task 23 只把**名字**放进作用域（占位对象：`typ: None` / `val: None`）。
//! 本文件做第二遍：
//!
//! ```text
//! ① 收条目  名字 → Item（类型/常量的「待求值形态」；同层重名先声明者胜）
//! ② 求值    按**声明序**走：解析类型表达式、求常量、给对象填 typ/val、收 dump 根
//! ③ 收口    `valid_type`（非法递归 / 嵌套深度）—— 本体全填好之后才能跑
//! ```
//!
//! # 两条顺序纪律（各自的失败形态不同）
//!
//! * **前向引用**（`type A B` 写在 `type B int` 之前）⇒ 由
//!   [`Checker::lookup_type`] 触发**惰性求值**：碰到一个「types 表里知道的
//!   包级条目，但 `typ` 还是占位」的名字，就地把它求出来。官方同样是
//!   「先建对象、后填类型」（`objDecl`）。
//! * **声明环**（`const A = B; const B = A`）⇒ 靠 `stack` 检出（`hit_cycle`）。
//!   不给 `ConstEnv` 加回调是**故意的**：它的 `lookup_const(&self)` 收 `&self`，
//!   要触发求值就得把整个求值器塞进 `RefCell`，那会把「顺序」从类型层面
//!   降级成运行期约定（Ruling `24-2`）。
//!
//! # 环的三分类（照官方 `decl.go` 的 `cycle()` 逐条翻译）
//!
//! | 环里有什么 | 官方谁报 | 我方代号 |
//! |---|---|---|
//! | **全是值** | `initorder.go`（`findCycle`） | [`DiagCode::TypInvalidInitCycle`] |
//! | **无值、且至少一个定义型** | `validType`（下一步才报） | **本层不报**（交给 [`Checker::pass3_validate`]） |
//! | 其余（纯别名 / 值+别名混合） | `decl.go` 的 `cycleError` | [`DiagCode::TypInvalidDeclCycle`] |
//!
//! 第二行是**必须**照搬的：`type A B; type B A` 在本层看来就是「两个具名类型
//! 互引」，但官方允许它走到 `validType`（由「按值包含」判非法），
//! 而 `type A = [10]A`（纯别名）在官方那里是 `cycleError` 当场报。
//! 分界线就是官方的 `nval == 0 && ndef > 0`。
//!
//! # `const` 的能力边界（`D-M3-19`，`U-M3-08`）
//!
//! | 初始化式 | 处理 |
//! |---|---|
//! | 常量子集内 | 求值 → `Object::val`；声明了类型还要过 `representable` |
//! | **需类型信息**（`pkg.C`、`len(s)`、`f()`、`H + 1`） | `TypInvalidConstInit`（稳定拒绝码，`T-TY-83` 正向） |
//! | 值算不出来（`1/0`、畸形字面量） | 上游已报（`DivByZero` / `InvalidConstVal`），本层**不**补第二条 |
//!
//! 反向（`T-TY-83` 的另一半）：常量子集内的初始化式**不得**报任何诊断。

use std::collections::{BTreeMap, BTreeSet};

use rgoc_ast::{AstFile, Decl, Expr, Field as AstField, LitKind, Op};
use rgoc_const::{
    BigInt, Const, ConstEnv, EvalOutcome, Sizes, eval_const_expr, expand_const_group_specs,
    representable,
};
use rgoc_lex::{Diag, DiagBag, DiagCode, FileId, Pos};
use rgoc_types::{
    BasicId, NamedId, ObjKind, Object, ScopeId, ScopeTable, Type, TypeId, TypeParamId, TypeTable,
    valid_type,
};

use crate::package::Package;

/// 包级声明的一项（**名字 → 待求值的形态**）。
#[derive(Clone, Debug)]
enum Item {
    /// `type D <T>`（`alias = false`）或 `type D = <T>`（`alias = true`）。
    Type {
        /// 声明处名字的位置（`validType` / 环诊断报在**对象**上，即这里）。
        pos: Pos,
        alias: bool,
        tparams: Vec<AstField>,
        rhs: Expr,
    },
    /// `const n <T> = <e>` 里的**一个名字**。
    Const {
        pos: Pos,
        /// 本条规格的 `iota`（[`expand_const_group_specs`] 给的）。
        iota: usize,
        /// 写出的声明类型（`const x int = 1` 的 `int`）。
        ty_expr: Option<Expr>,
        /// 声明类型解析结果（**记忆化**：根与对象共用同一个 `TypeId`，
        /// 解析两次会多出两个 `struct`/`[]T` 条目 —— 结构型**不**去重）。
        ty: Option<TypeId>,
        /// 本名字对应的初始化式；`None` ⇒ 名字多于值（arity 已报）。
        value: Option<Expr>,
    },
}

impl Item {
    /// 声明处名字的位置。
    fn pos(&self) -> Pos {
        match self {
            Item::Type { pos, .. } | Item::Const { pos, .. } => *pos,
        }
    }

    /// 环分类用的种类。
    fn kind(&self) -> ItemKind {
        match self {
            Item::Type { alias: false, .. } => ItemKind::TypeDef,
            Item::Type { alias: true, .. } => ItemKind::Alias,
            Item::Const { .. } => ItemKind::Const,
        }
    }
}

/// 环分类的第二维（照官方 `nval` / `ndef` 的口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ItemKind {
    /// **定义型** —— 官方 `ndef` 计的是它（别名**不算**，见官方 `cycle()` 的注释）。
    TypeDef,
    /// 别名（`type A = B`）。
    Alias,
    /// 值（`const`）。
    Const,
}

/// 一个条目的求值状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    /// 还没动过。
    Unresolved,
    /// **正在**求值 —— 它就是环的判据（`stack` 的成员）。
    InProgress,
    /// 求完了（**无论成功失败**：失败了也得标 Done，否则会被反复重试）。
    Done,
}

/// `const` 求值看到的**常量环境**：就是包作用域里那些**已经求完**的常量对象。
///
/// 不需要另建 `名字 → 值` 的表：对象就是那份表（`Object::val`），
/// 两份表会分叉（一份有值一份没有是必然发生的中间态）。
struct ConstView<'a> {
    scopes: &'a ScopeTable,
    scope: ScopeId,
    /// 当前规格的 `iota`（类型位置求值时是 `None` —— 类型里没有 `iota`）。
    iota: Option<Const>,
}

impl ConstEnv for ConstView<'_> {
    fn lookup_const(&self, ident: &str) -> Option<&Const> {
        if ident == "iota" {
            return self.iota.as_ref();
        }
        let o = self.scopes.lookup(self.scope, ident)?;
        if o.kind != ObjKind::Const {
            return None;
        }
        match &o.val {
            Some(v) => Some(v),
            // 常量**对象已建但没有值**（声明环 / 不可表示 / arity 已报）：
            // 官方把这种值的对象置为 `unknown`（`obj.val = unknown`），
            // 于是上层看到的是「常量，但没有值」而**不是**「不是常量」。
            // 这个区别是**诊断条数**的区别：返回 `None` 会让每个引用者
            // 各报一条 `InvalidConstInit`（follow-up 噪声），而官方只报一条。
            None => Some(&Const::Unknown),
        }
    }
}

/// 求值器状态（跨三遍复用；字段对 `typeexpr.rs` 可见 ⇒ `pub(crate)`）。
pub(crate) struct Checker<'a> {
    /// 类型表（universe 已声明进去的那张）。
    pub(crate) tt: &'a mut TypeTable,
    /// 作用域竞技场（universe ⊃ package ⊃ file）。
    pub(crate) scopes: &'a mut ScopeTable,
    /// 诊断袋（本层只追加；排序由调用方做）。
    pub(crate) bag: &'a mut DiagBag,
    /// 包作用域（包级名字都住这里）。
    pub(crate) pkg_scope: ScopeId,
    /// 平台尺寸（首发 `linux/arm64`）。
    pub(crate) sizes: Sizes,
    /// 包级条目：名字 → 待求值形态。
    items: BTreeMap<String, Item>,
    /// 条目的**声明序**（根序 / 环的「源码序最前」都要它）。
    order: Vec<String>,
    /// 求值状态。
    states: BTreeMap<String, State>,
    /// 正在求值的名字（栈顶是当前项）。
    stack: Vec<String>,
    /// 报过环、要**降级成 `invalid`** 的名字（值环全降级；声明环只降级环起点）。
    poisoned: BTreeSet<String>,
    /// 声明环里**非环起点**的成员：`validType` 的重复报告要跳过它们。
    cycle_skip: BTreeSet<String>,
    /// dump 根（声明序）。
    roots: Vec<TypeId>,
    /// 无类型基本类型的 `TypeId` 缓存（`add_basic` 不去重，见 `TypeTable::add_basic`）。
    untyped_cache: BTreeMap<BasicId, TypeId>,
    /// 已经报过 `undefined` 的**书写位置**（Task 25）——`Pos` 唯一标识一次书写，
    /// 见 [`crate::scope`] 的 `report_undefined`。
    pub(crate) undefined_reported: BTreeSet<Pos>,
    /// **直接**依赖：`import` 路径 → 已装载的包（M4/Task 35）。
    ///
    /// 只读借自 `Package::imports` —— 会话在阶段 A 与阶段 B **之间**把它填好
    /// （见 [`crate::importer::Session`] 的四步），所以这里一定是全的。
    pub(crate) imports: &'a BTreeMap<String, Package>,
    /// 每个文件引入的**包名 → 路径**（`Package::import_bindings` 的快照）。
    ///
    /// 为什么不每次现算：`pkg.X` 的解析在类型表达式里会递归很多次，而
    /// `import_bindings` 要扫一遍语法树。
    pub(crate) bindings: BTreeMap<FileId, Vec<(String, String)>>,
    /// **正在求值的声明所属文件** —— `pkg.X` 要靠它找该文件的 import 名。
    ///
    /// 官方把这件事记在 `declInfo.file` 上（`check.objDecl` 里切 `check.scope`）；
    /// 我们的 [`Item`] 自带 `pos`，所以从 `pos.file` 推得出来
    /// （[`Checker::resolve_name`] 与 `pass2_resolve` 各设一次）。
    pub(crate) cur_file: Option<FileId>,
    /// 每个被接受文件 → 它的**文件作用域**（`import` 名与点导入注入的名字住这里）。
    pub(crate) file_scopes: BTreeMap<FileId, ScopeId>,
    /// 包级泛型函数的类型参数表（`M12/Task 6`）—— 收尾时整体交给
    /// `Package::generic_funcs`（与 `roots` 同一套路：借用期不能写 `pkg`）。
    generic_funcs: BTreeMap<String, Vec<TypeParamId>>,
    /// `func _[T …]` 的类型参数表，键 = 声明位置（`Package::blank_func_tparams`）。
    blank_func_tparams: BTreeMap<rgoc_lex::Pos, Vec<TypeParamId>>,
}

impl Checker<'_> {
    /// 「只走体」用的构造（Task 25）：包级条目 / 求值状态**全空**。
    ///
    /// 为什么必须空：[`Checker`] 的那几个私有字段是**包级求值状态机**
    /// （`items` / `states` / `stack` / `poisoned`），体内出现「某个包级项正在求值」
    /// 是 bug 而不是特性。空表让 [`Checker::lookup_type`] 的惰性解析分支
    /// 直接走不通（`items` 里没有名字）—— 而那正是想要的：包级对象在
    /// [`collect_decls`] 结束时**已经全部求值完毕**。
    pub(crate) fn for_bodies<'a>(
        tt: &'a mut TypeTable,
        scopes: &'a mut ScopeTable,
        bag: &'a mut DiagBag,
        pkg_scope: ScopeId,
        imports: &'a BTreeMap<String, Package>,
        bindings: BTreeMap<FileId, Vec<(String, String)>>,
    ) -> Checker<'a> {
        Checker {
            tt,
            scopes,
            bag,
            pkg_scope,
            sizes: Sizes::HOST_ARM64,
            items: BTreeMap::new(),
            order: Vec::new(),
            states: BTreeMap::new(),
            stack: Vec::new(),
            poisoned: BTreeSet::new(),
            cycle_skip: BTreeSet::new(),
            roots: Vec::new(),
            untyped_cache: BTreeMap::new(),
            undefined_reported: BTreeSet::new(),
            imports,
            bindings,
            cur_file: None,
            file_scopes: BTreeMap::new(),
            generic_funcs: BTreeMap::new(),
            blank_func_tparams: BTreeMap::new(),
        }
    }

    /// 当前声明**所在文件**的作用域（`cur_file` 未知 ⇒ 退回包作用域）。
    ///
    /// 官方把「声明属于哪个文件」记在 `declInfo.file` 上，求值时把
    /// `check.scope` 切到那个**文件作用域** —— 因为 `import` 名（含点导入
    /// 注入的名字）**只在该文件可见**。`var x lib.T`、点导入后的 `var x T`
    /// 都要靠它；用包作用域会看不见这些名字。
    fn decl_scope(&self) -> ScopeId {
        self.cur_file
            .and_then(|f| self.file_scopes.get(&f).copied())
            .unwrap_or(self.pkg_scope)
    }
}

/// 收集一个已装载包的**全部顶层声明**：类型、常量、dump 根。
///
/// 前置条件（`Package` 的构造协议）：`pkg.asts` 与 `pkg.files` 同序，且
/// 顶层名字已由 [`crate::package::load_sources`] 声明进包作用域（占位对象）。
///
/// 后置条件：
/// * 每个包级类型名 / 常量的对象都填好了 `typ`（失败 ⇒ `invalid`）；
/// * 每个不用 `_` 声明的常量对象填好了 `val`（失败 ⇒ `None`）；
/// * [`Package::type_roots`] 是 `C7` §3 口径的根序列；
/// * 具名类型的本体**全部非空**（`valid_type` 的前置条件）。
pub fn collect_decls(pkg: &mut Package, tt: &mut TypeTable, bag: &mut DiagBag) {
    // 作用域表与语法树都**挪出来**：Checker 要 `&mut` 借它们，而收尾还要往
    // `pkg` 上写两个新字段 —— 一边借 `pkg` 一边写字段是编译不过的。
    // 挪走而不是 `Clone`：作用域表里有大对象，克隆一份再丢是纯浪费。
    // ⚠️ 必须在 `mem::take(&mut pkg.asts)` **之前**算：`import_bindings` 要扫语法树
    let bindings: BTreeMap<FileId, Vec<(String, String)>> = pkg
        .asts
        .iter()
        .map(|a| (a.pos.file, pkg.import_bindings(a.pos.file)))
        .collect();
    let file_scopes: BTreeMap<FileId, ScopeId> = pkg.file_scopes.iter().copied().collect();
    let asts = std::mem::take(&mut pkg.asts);
    let mut scopes = std::mem::take(&mut pkg.scopes);
    let mut ck = Checker {
        tt,
        scopes: &mut scopes,
        bag,
        pkg_scope: pkg.scope,
        sizes: Sizes::HOST_ARM64,
        items: BTreeMap::new(),
        order: Vec::new(),
        states: BTreeMap::new(),
        stack: Vec::new(),
        poisoned: BTreeSet::new(),
        cycle_skip: BTreeSet::new(),
        roots: Vec::new(),
        untyped_cache: BTreeMap::new(),
        undefined_reported: BTreeSet::new(),
        imports: &pkg.imports,
        bindings,
        cur_file: None,
        file_scopes,
        generic_funcs: BTreeMap::new(),
        blank_func_tparams: BTreeMap::new(),
    };
    ck.pass1_collect(&asts);
    ck.pass2_resolve(&asts);
    ck.pass3_validate();
    let roots = std::mem::take(&mut ck.roots);
    let generic_funcs = std::mem::take(&mut ck.generic_funcs);
    let blank_func_tparams = std::mem::take(&mut ck.blank_func_tparams);
    drop(ck);
    pkg.scopes = scopes;
    pkg.type_roots = roots;
    pkg.generic_funcs = generic_funcs;
    pkg.blank_func_tparams = blank_func_tparams;
    pkg.asts = asts;
}

/// 接收者写出的**类型实参名**（`func (b *Box[T])` / `func (Box[T, U])`）。
///
/// 返回 `(基类型名, 实参名)`。只有「基是**裸标识符**、实参**全是标识符**」才返回
/// `Some` —— 那正是 Go 允许出现在接收者位置的形状（规范的 receiver 语法：
/// `"(" [ identifier ] [ "*" ] BaseTypeName [ "[" identifier_list "]" ] ")"`）。
/// 别的形状（`*pkg.T[A]`、`Box[[]int]`…）⇒ `None`：不是接收者声明的类型参数。
///
/// 指针与括号都可以有（`func (b (*Box[T])) M()` 合法）⇒ 先一层层剥掉再判。
pub(crate) fn recv_type_args(recv: &AstField) -> Option<(&str, Vec<&rgoc_ast::Name>)> {
    let mut e = &recv.type_;
    loop {
        e = match e {
            Expr::Paren(p) => &p.x,
            Expr::Unary(u) if u.op == Op::Mul && u.y.is_none() => &u.x,
            _ => break,
        };
    }
    let Expr::Index(ix) = e else { return None };
    let Expr::Name(base) = &*ix.x else {
        return None;
    };
    let names = match &*ix.index {
        Expr::Name(n) => vec![n],
        Expr::List(l) => l
            .elems
            .iter()
            .map(|e| match e {
                Expr::Name(n) => Some(n),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?,
        _ => return None,
    };
    Some((base.value.as_str(), names))
}

impl Checker<'_> {
    // ── 第一遍：收条目 ────────────────────────────────────────────────────

    /// 把每个顶层声明的**待求值形态**收进 `items`（同层重名：**先声明者胜**，
    /// 与 `package.rs` 的占位对象同一条纪律 —— 重复声明的诊断归 Task 25）。
    fn pass1_collect(&mut self, asts: &[AstFile]) {
        for ast in asts {
            // 常量组的 `iota` 展开**按文件**做（组不跨文件，见 `iota.rs` 的组身份）
            let consts: Vec<rgoc_ast::ConstDecl> = ast
                .decls
                .iter()
                .filter_map(|d| match d {
                    Decl::Const(c) => Some(c.clone()),
                    _ => None,
                })
                .collect();
            let specs = expand_const_group_specs(&consts);
            let mut si = 0usize;

            for decl in &ast.decls {
                match decl {
                    Decl::Import(_) => {} // 名字在 Task 23 已进**文件**作用域
                    Decl::Type(d) => {
                        if d.name.value == "_" || self.items.contains_key(&d.name.value) {
                            continue;
                        }
                        let name = d.name.value.clone();
                        self.items.insert(
                            name.clone(),
                            Item::Type {
                                pos: d.name.pos,
                                alias: d.alias,
                                tparams: d.tparams.clone(),
                                rhs: d.type_.clone(),
                            },
                        );
                        self.order.push(name);
                    }
                    Decl::Const(d) => {
                        let (iota, values) = specs.get(si).cloned().unwrap_or_default();
                        si += 1;
                        // 名字 ↔ 值个数不匹配：官方 `arityMatch`（`resolver.go:55`）。
                        // 必须在**配对上**报 —— 不报就会静默丢名字。
                        self.arity_diags(d, &values);
                        for (j, n) in d.names.iter().enumerate() {
                            if n.value == "_" || self.items.contains_key(&n.value) {
                                continue;
                            }
                            let name = n.value.clone();
                            self.items.insert(
                                name.clone(),
                                Item::Const {
                                    pos: n.pos,
                                    iota,
                                    ty_expr: d.type_.clone(),
                                    ty: None,
                                    value: values.get(j).cloned(),
                                },
                            );
                            self.order.push(name);
                        }
                    }
                    // `var` / `func` 不建条目：var 的初始化式不求值（`open_gaps`），
                    // func 的签名在第二遍当场解析（没有「前向引用签名」这回事）
                    Decl::Var(_) | Decl::Func(_) => {}
                }
            }
        }
    }

    /// `const` 的**名字 ↔ 初始化式**个数检查（官方 `WrongAssignCount`）。
    ///
    /// ⚠️ `var` 一侧**不报**（同一官方代号）：那要求求值 var 的初始化式，
    /// 而 M3 不求值（`open_gaps`）。这里只覆盖**必须配对**的 `const`。
    fn arity_diags(&mut self, d: &rgoc_ast::ConstDecl, values: &[Expr]) {
        for n in d.names.iter().skip(values.len()) {
            self.bag.push(Diag {
                pos: n.pos,
                code: DiagCode::TypWrongAssignCount,
                message: format!("missing init expr for {}", n.value),
            });
        }
        if values.len() > d.names.len()
            && let Some(extra) = values.get(d.names.len())
        {
            self.bag.push(Diag {
                pos: extra.start_pos(),
                code: DiagCode::TypWrongAssignCount,
                message: format!("extra init expr {}", expr_text(extra)),
            });
        }
    }

    // ── 第二遍：按声明序求值 ──────────────────────────────────────────────

    /// 按**声明序**走一遍：求值、给对象填类型/值、收 dump 根。
    ///
    /// 顺序为什么是「文件序 ⊃ 声明序」：`C7` §3 的根序就是这么定的，
    /// 而它同时决定诊断的稳定顺序（诊断最后按位置排序，位置就是声明位置）。
    fn pass2_resolve(&mut self, asts: &[AstFile]) {
        for ast in asts {
            // 本文件声明的**值**面（`var` 的写出类型、`func` 的签名）就地解析，
            // 不经 `Item` ⇒ 文件要在这里显式切换（`pkg.X` 依赖它）。
            self.cur_file = Some(ast.pos.file);
            for decl in &ast.decls {
                // 每条声明都重设一次：`resolve_name` 的递归会把 `cur_file`
                // 带到**别的文件**的条目上去（跨文件前向引用）
                self.cur_file = Some(ast.pos.file);
                match decl {
                    Decl::Import(_) => {}
                    Decl::Type(d) => {
                        if d.name.value == "_" {
                            continue;
                        }
                        self.resolve_name(&d.name.value);
                        if let Some(t) = self.object_typ(&d.name.value) {
                            self.roots.push(t);
                        }
                    }
                    Decl::Const(d) => {
                        for n in &d.names {
                            if n.value == "_" {
                                continue;
                            }
                            self.resolve_name(&n.value);
                            // 写出的声明类型是根（`C7` §3）；初始化式里的类型不在范围内
                            if let Some(t) = self.const_root_ty(&n.value) {
                                self.roots.push(t);
                            }
                        }
                    }
                    Decl::Var(d) => {
                        // 只在**写出了类型**时才有根（`var x = 1` 的 int 不来初始化式）
                        let Some(te) = &d.type_ else { continue };
                        let scope = self.decl_scope();
                        let t = self.resolve_type(scope, te);
                        for n in &d.names {
                            if n.value == "_" {
                                continue;
                            }
                            self.write_object(&n.value, t, None);
                            self.roots.push(t);
                        }
                    }
                    Decl::Func(d) => {
                        let (recv, t, own_tps) = self.resolve_func_sig(d);
                        // 包级泛型函数的类型参数表（`M12/Task 6`）：显式实例化与
                        // 单态化都要按**位置**对齐「形参表 ↔ 实参表」，而
                        // `SignatureDef` 没有这一栏 ⇒ 在 `Package` 上单开一张表。
                        // 只收包级函数（方法不能显式指定类型实参）。
                        if recv.is_none() && !own_tps.is_empty() {
                            if d.name.value == "_" {
                                // `_` 可以声明很多次 ⇒ 键用位置，见
                                // `Package::blank_func_tparams`。
                                self.blank_func_tparams.insert(d.pos, own_tps);
                            } else {
                                self.generic_funcs.insert(d.name.value.clone(), own_tps);
                            }
                        }
                        // `func init` **不进包作用域**（M4/Task 34）⇒ 它的签名
                        // 也不是 dump 根：官方 dump 从包作用域枚举对象，
                        // `init` 不在里面。留着会让 `C7` 多一行 `signature`。
                        let is_init = recv.is_none() && d.name.value == "init";
                        match recv {
                            // 方法：挂到接收者基类型上（Task 7 的**唯一**生产者，
                            // 消费者是 `rgoc-types::methodset`）。空名（`func (t T) _()`）
                            // 不挂；接收者不是本包具名基底（产权 / 别名 —— 非法 Go，
                            // 或错误恢复的 `invalid`）也不挂 —— 诊断归检查器。
                            Some(r) => {
                                if d.name.value != "_"
                                    && let Some(nid) = self.recv_named(r)
                                {
                                    self.tt.add_named_method(nid, d.name.value.clone(), t);
                                }
                            }
                            None => {
                                if d.name.value != "_" {
                                    self.write_object(&d.name.value, t, None);
                                }
                            }
                        }
                        if t != self.tt.invalid() && !is_init {
                            self.roots.push(t);
                        }
                    }
                }
            }
        }
    }

    /// 一个 `func` 声明的签名（方法带接收者）。类型参数作用域只在这里建。
    ///
    /// **接收者声明的类型参数**（`TYP-22`）：`func (b *Box[T]) M()` 里的 `T` 由
    /// **接收者**声明（官方 `recv.TypeParams()`），约束取类型声明里**同一位置**
    /// 那一个。它必须在解析**接收者类型**之前进作用域 —— `*Box[T]` 里的 `T`
    /// 正是它（`Task 35` 的语料普查抓到过这条漏声明：三例 `undefined: T`）。
    ///
    /// 返回 `(接收者类型, 签名)`：接收者回给调用方做**方法挂载**（M4/Task 7）；
    /// 普通函数为 `None`。接收者只在这里解析**一次**（复解析会在表里多造
    /// 结构型条目，`C7` 的号是全表遍历序）。
    fn resolve_func_sig(
        &mut self,
        d: &rgoc_ast::FuncDecl,
    ) -> (Option<TypeId>, TypeId, Vec<TypeParamId>) {
        let (base, recv_tps, generic_recv) =
            match d.recv.as_ref().and_then(|r| self.recv_tparam_fields(r)) {
                Some(fs) => {
                    let (s, ids) = self.declare_tparams(self.pkg_scope, &fs);
                    (s, ids, true)
                }
                None => (self.pkg_scope, Vec::new(), false),
            };
        // 接收者类型在**方法自己的**类型参数之外解析（官方：接收者不能引用方法
        // 自己的 `[P any]`）—— 但**能**引用接收者声明的类型参数 ⇒ 用 `base`。
        //
        // ⚠️ 泛型接收者的**形参表那一段不解析**（`M12/Task 10`）。官方
        // `collectRecv`（`signature.go:216`）先用 `unpackRecv` 拆出
        // 「基 + 形参表」，再把形参逐个 `declareTypeParam`（`decl.go:662`），
        // `recvType = instance(baseType, recvTParams)` 的实参是刚声明的类型参数
        // **自己** —— 形参那棵表达式根本没过 `typ()`。照抄这一点：
        // `Box[_]` / `Box[T]` 都只解析基名 `Box`，`[_]` 不碰。
        // 解析它就会把 `_` 当引用 ⇒ `TYP-INVALID-BLANK`（语料 4 个文件：
        // `issue47710.go` / `issue53406.go` / `smoketest.go` / `list2.go`）。
        let recv = d.recv.as_ref().map(|r| {
            if generic_recv {
                self.resolve_recv_generic(base, &r.type_)
            } else {
                self.resolve_type(base, &r.type_)
            }
        });
        let (scope, own_tps) = if d.tparams.is_empty() {
            (base, Vec::new())
        } else {
            // 方法**自己**的类型参数（`func (b Box[T]) M[P any]()` 的 `[P any]`）：
            // 不进签名（`SignatureDef` 无同栏），但**必须回给调用方**
            // （`M12/Task 6`：包级泛型函数要进 `Package::generic_funcs`）。
            self.declare_tparams(base, &d.tparams)
        };
        let sig = match &d.type_ {
            Expr::FuncType(ft) => self.resolve_signature(scope, ft, recv),
            _ => self.tt.invalid(),
        };
        // 接收者类型参数回写给签名（M4/Task 12）：`instantiate` 的逐方法替换按
        // **位置**对齐实参（官方 `expandMethod` 的 `makeSubstMap(rtpars, rtargs)`）。
        // `C7` dump 不渲染它；普通函数没有接收者 ⇒ 恒空。
        if !recv_tps.is_empty()
            && let Type::Signature(sid) = self.tt.type_of(sig)
        {
            let sid = *sid;
            self.tt.set_signature_recv_tparams(sid, recv_tps);
        }
        (recv, sig, own_tps)
    }

    /// **泛型**接收者类型：只解析基名，形参表那一段不碰（`M12/Task 10`）。
    ///
    /// 官方 `collectRecv` 的实参是刚 `declareTypeParam` 出来的类型参数**自己**
    /// （`recvType = instance(baseType, recvTParams)`），形参那棵表达式从不过
    /// `typ()`。所以 `Box[_]` 的 `_` 在官方那里是**声明**，不是引用。
    ///
    /// 调用前提：`recv_tparam_fields` 已返回 `Some` ⇒ 剥掉 `Paren` / `*` 之后
    /// 必定是 `Index{base: Name, index: 全标识符}`，且个数与类型声明的形参表对齐。
    /// 指针层数照原样包回去（`*Box[T]` 的结果要与 `resolve_type` 走老路时同一个）。
    fn resolve_recv_generic(&mut self, scope: ScopeId, e: &rgoc_ast::Expr) -> TypeId {
        let mut ptr = 0usize;
        let mut cur = e;
        loop {
            match cur {
                Expr::Paren(p) => cur = &p.x,
                Expr::Unary(u) if u.op == Op::Mul && u.y.is_none() => {
                    ptr += 1;
                    cur = &u.x;
                }
                _ => break,
            }
        }
        // 不是 `Index` 只能是「上游换了形状」—— 兜底走全量解析，绝不假装成功
        let Expr::Index(ix) = cur else {
            return self.resolve_type(scope, cur);
        };
        let mut t = self.resolve_type(scope, &ix.x);
        for _ in 0..ptr {
            t = self.tt.add_pointer(t);
        }
        t
    }

    /// 接收者类型 → 本包具名基底（`T` / `*T`；泛型 `Box[T]` / `*Box[T]` 在
    /// M3 的表示里解析成的就是 `named Box`，同路命中）。
    ///
    /// **别名透明**：`type A = T; func (a A) M()` 是合法 Go 且 M 属于 T
    /// （官方探针实测，见 `.superpowers/sdd/M4-plan/probe/aliasrecv-main.go`）。
    /// 拆不出（`invalid` / 错误恢复 / 别名环的残缺 rhs）⇒ `None`：不挂，
    /// 诊断交给检查器。**不走 `unalias`** —— 加载期对残缺表要能容忍
    /// （链环靠步数上限兜底，不能 panic）。
    fn recv_named(&self, recv: TypeId) -> Option<NamedId> {
        let mut base = match self.tt.type_of(recv) {
            Type::Pointer(e) => *e,
            _ => recv,
        };
        for _ in 0..64 {
            match self.tt.type_of(base) {
                // 别名环的残缺 rhs（`None`）⇒ 拆不出基底
                Type::Alias(a) => base = self.tt.alias_def(*a).rhs?,
                _ => break,
            }
        }
        match self.tt.type_of(base) {
            Type::Named(n) => Some(*n),
            _ => None,
        }
    }

    /// 把接收者声明的类型参数**还原成声明处的形状**：名字用接收者写的（`U`），
    /// 约束用类型声明里**同位置**那一个（官方按位置替换，实测
    /// `func (n *NBox[U]) Alt() U` 合法且 `U` 取 `NBox` 的约束）。
    ///
    /// ⚠️ 条目是**新建**的（不复用类型声明里那个）：实测 oracle 给方法造了自己的
    /// `typeparam` 条目（`named Box[T]` 实例的字段类型指向**方法**那一个）。
    /// 复制约束表达式是安全的：具名 / universe 约束解出来还是同一个 `TypeId`。
    fn recv_tparam_fields(&self, recv: &AstField) -> Option<Vec<AstField>> {
        let (base, names) = recv_type_args(recv)?;
        let Some(Item::Type { tparams, .. }) = self.items.get(base) else {
            return None; // 基类型不是本包的类型声明 ⇒ 没有类型参数
        };
        // 个数不对 ⇒ 那是另一种写法（`Box[T]` 而 `Box` 非泛型）⇒ 交给 `undefined` 报
        if tparams.len() != names.len() {
            return None;
        }
        Some(
            names
                .into_iter()
                .zip(tparams.iter())
                .map(|(n, f)| AstField {
                    name: Some(n.clone()),
                    type_: f.type_.clone(),
                    tag: None,
                    pos: n.pos,
                })
                .collect(),
        )
    }

    /// 求一个包级条目（幂等；环检测在 [`Checker::hit_cycle`]）。
    fn resolve_name(&mut self, name: &str) {
        match self.states.get(name).copied().unwrap_or(State::Unresolved) {
            State::Done => return,
            // 正常路径不该走到这里（引用方先用 `lookup_type` 拦），
            // 但 `ConstEnv` 语境下可能绕过 ⇒ 这里兜底报环，**绝不递归**
            State::InProgress => {
                let _ = self.hit_cycle(name);
                return;
            }
            State::Unresolved => {}
        }
        let Some(item) = self.items.get(name).cloned() else {
            return;
        };
        // `pkg.X` 要知道「这条声明写在哪个文件」—— 官方记在 `declInfo.file` 上，
        // 我们这里从声明处位置推（`Pos` 自带 `FileId`）。
        self.cur_file = Some(item.pos().file);
        self.states.insert(name.to_string(), State::InProgress);
        self.stack.push(name.to_string());

        match &item {
            Item::Type { .. } => self.resolve_type_item(name, item.clone()),
            Item::Const { .. } => self.resolve_const_item(name, item.clone()),
        }

        self.stack.pop();
        self.states.insert(name.to_string(), State::Done);
        // 报过环的成员 ⇒ 对象降级（官方 `cycleError`/`initorder` 都这么做）
        if self.poisoned.contains(name) {
            let inv = self.tt.invalid();
            self.write_object(name, inv, None);
        }
    }

    /// `type D <T>` / `type D = <T>`：**先占号写回对象，再解析本体**。
    ///
    /// 顺序是硬要求：`type N struct{ p *N }` 的自引用必须在解析本体时就能查到
    /// `N` 的对象（否则前向引用会拿到 `invalid`），而 `valid_type` 又要求
    /// 本体最后**非空** ⇒ 中间态用 `invalid` 顶上，收尾再填真值。
    fn resolve_type_item(&mut self, name: &str, item: Item) {
        let Item::Type {
            pos,
            alias,
            tparams,
            rhs,
        } = item
        else {
            unreachable!("resolve_type_item 只接 Item::Type")
        };
        // 类型参数作用域（`[T any]`）：本体与约束都在里面解析。
        //
        // ⚠️ 这里**只建名字**（`declare_tparam_names`），约束留到形参表挂上
        // `NamedDef` 之后 —— 自引用约束 `type T[U interface{ M() T[U] }]` 的
        // `T[U]` 要靠 `NamedDef::type_params` 才认得出是泛型（`issue46461`）。
        let (scope, tparam_entries) = if tparams.is_empty() {
            // 无类型参数 ⇒ 本体直接在该声明的**文件作用域**里解析
            // （`import` 名与点导入注入的名字只在那里可见，M4/Task 35）
            (self.decl_scope(), Vec::new())
        } else {
            self.declare_tparam_names(self.pkg_scope, &tparams)
        };
        let tparam_ids: Vec<rgoc_types::TypeParamId> =
            tparam_entries.iter().map(|(tp, _)| *tp).collect();
        let inv = self.tt.invalid();
        if alias {
            let a = self.tt.add_alias(name);
            self.tt.set_alias_rhs(a, inv);
            let self_ty = self.tt.alias_type(a);
            self.write_object(name, self_ty, None);
            // 泛型别名的形参约束（`AliasDef` 没有 `type_params` 栏，但约束
            // 本身仍要解析 —— 里面的名字引用与环诊断都在这一遍发生）。
            if !tparams.is_empty() {
                self.resolve_tparam_constraints(scope, &tparams, &tparam_entries);
            }
            let r = self.resolve_type(scope, &rhs);
            self.tt.set_alias_rhs(a, r);
        } else {
            let n = self.tt.add_named(name);
            self.tt.set_named_underlying(n, inv);
            // 位置挂在**定义**上：`validType` 的环诊断报在环起点的对象处
            self.tt.set_named_pos(n, pos);
            // 形参表挂到定义（M4/Task 12）：`instantiate` 的数量 / 约束校验要用。
            // 泛型**别名**没有同栏（`AliasDef` 不加字段；泛型别名实例化留后续任务）。
            self.tt.set_named_type_params(n, tparam_ids);
            let self_ty = self.tt.named_type(n);
            self.write_object(name, self_ty, None);
            // 形参表已挂上 ⇒ 约束里的 `T[U]` 现在能实例化（自引用约束）。
            if !tparams.is_empty() {
                self.resolve_tparam_constraints(scope, &tparams, &tparam_entries);
            }
            let u = self.resolve_type(scope, &rhs);
            self.tt.set_named_underlying(n, u);
        }
    }

    /// `const` 的一个名字：先解依赖（含声明类型），再求值。
    fn resolve_const_item(&mut self, name: &str, item: Item) {
        let Item::Const {
            pos: _,
            iota,
            ty_expr: _,
            ty: _,
            value,
        } = item
        else {
            unreachable!("resolve_const_item 只接 Item::Const")
        };
        // ① 声明类型（记忆化：`const_root_ty` 会把它填回条目）
        let declared = self.const_root_ty(name);

        // ② 依赖：值表达式里引用的**包级条目**先求出来。
        //    `ConstEnv::lookup_const` 收 `&self`，不能在求值中途触发解析
        //    ⇒ 依赖必须在**求值之前**就绪（Ruling 24-2）。
        if let Some(e) = &value {
            for dep in const_deps(e) {
                if self.items.contains_key(&dep) {
                    self.resolve_name(&dep);
                }
            }
        }

        // ③ 求值
        let (typ, val) = match &value {
            // 名字多于值：arity 已报，这里不补第二条（avoid follow-up errors）
            None => (declared.unwrap_or_else(|| self.tt.invalid()), None),
            Some(e) => self.eval_const_decl(e, iota, declared),
        };
        self.write_object(name, typ, val);
    }

    /// 求值 + 可表示性：返回（对象类型，对象值）。
    fn eval_const_decl(
        &mut self,
        e: &Expr,
        iota: usize,
        declared: Option<TypeId>,
    ) -> (TypeId, Option<Const>) {
        let view = ConstView {
            scopes: self.scopes,
            scope: self.pkg_scope,
            iota: Some(Const::Int(BigInt::from_i64(iota as i64))),
        };
        let (out, diags) = eval_const_expr(e, &view, &self.sizes);
        for d in diags.into_sorted() {
            self.bag.push(d);
        }

        match out {
            // **不是**常量表达式（要类型信息 / 要运行期值）：M3 的稳定拒绝码
            // （`D-M3-19`、`T-TY-83` 正向）
            EvalOutcome::NotAConstant => {
                self.bag.push(Diag {
                    // 官方 `check.errorf(x, …)` 用的是 `x.Pos()`；本仓的 `pos()`
                    // 对二元节点是**运算符**位置，`Pos()` 的对应物是 `start_pos()`
                    pos: e.start_pos(),
                    code: DiagCode::TypInvalidConstInit,
                    message: format!("{} is not constant", expr_text(e)),
                });
                (declared.unwrap_or_else(|| self.tt.invalid()), None)
            }
            // 形状上是常量但没值：上游（`DivByZero` 等）已经报过 ⇒ 不补第二条
            EvalOutcome::Const(Const::Unknown) => {
                (declared.unwrap_or_else(|| self.tt.invalid()), None)
            }
            EvalOutcome::Const(v) => {
                let typ = match declared {
                    Some(t) => t,
                    // 没写类型 ⇒ 对象类型就是**无类型**基本类型（官方 `untyped int` 等）
                    None => self.const_type_of(e).unwrap_or_else(|| self.tt.invalid()),
                };
                let val = match declared {
                    None => Some(v),
                    Some(t) => self.check_representable(e, t, v),
                };
                (typ, val)
            }
        }
    }

    /// 常量对**声明类型**的可表示性（官方在 `assignment(x, lhs.typ, "constant declaration")` 里做）。
    ///
    /// 目标类型取**本体**（`Underlying`）：`type MyInt int; const X MyInt = 1.5`
    /// 官方报的是 `truncated`（实测），说明它看的是本体上的 `Basic`。
    fn check_representable(&mut self, e: &Expr, declared: TypeId, v: Const) -> Option<Const> {
        let Some(b) = self.underlying_basic(declared) else {
            // 不是常量类型（官方 `InvalidConstType`）：M3 不引这条代号（`open_gaps`）
            return Some(v);
        };
        match representable(&v, b, &self.sizes) {
            Ok(rep) => Some(rep.value),
            Err(code) => {
                // 消息**不承诺逐字一致**（`D-M1-9`）：位置与类别才是判据。
                // 官方文本形如 `cannot use 1e400 (untyped float constant …) as
                // float32 value in constant declaration (overflows)`。
                let why = match code {
                    DiagCode::TypNumericOverflow => " (overflows)",
                    DiagCode::TypTruncatedFloat => " (truncated)",
                    _ => "",
                };
                self.bag.push(Diag {
                    pos: e.start_pos(),
                    code,
                    message: format!(
                        "cannot use {} as {} value in constant declaration{why}",
                        expr_text(e),
                        self.type_name(declared)
                    ),
                });
                // 官方把值置为 `unknown`（**不**截断成近似值 —— `T-TY-47` 的另一半）
                None
            }
        }
    }

    // ── 第三遍：收口 ─────────────────────────────────────────────────────

    /// `valid_type`：按值包含的非法递归 + 嵌套深度。
    ///
    /// 只跑**定义型**（别名环归本文件第二遍；`validType` 刻意不下钻别名）。
    fn pass3_validate(&mut self) {
        let names: Vec<String> = self
            .order
            .iter()
            .filter(|n| matches!(self.items.get(*n), Some(Item::Type { alias: false, .. })))
            .cloned()
            .collect();
        for name in names {
            // 同一个环只报一次：官方 `cycleError` 报在**源码序最前**的成员上，
            // 其余成员在第二遍已被记为 skip（否则环里几个成员就报几条）
            if self.cycle_skip.contains(&name) {
                continue;
            }
            let Some(t) = self.object_typ(&name) else {
                continue;
            };
            let _ = valid_type(&*self.tt, t, &mut *self.bag);
        }
    }

    // ── 名字 / 类型 / 常量 的公共设施 ────────────────────────────────────

    /// 查一个**类型**名字。
    ///
    /// 三条纪律：
    /// * 只有 `TypeName` 类对象才算类型（值/函数/内建出现在类型位置 ⇒ `invalid`；
    ///   官方报 `NotAType`，那条代号归 M4 ⇒ `open_gaps`）；
    /// * 包级条目**正在求值**（`InProgress`）⇒ 环，交给 [`Checker::hit_cycle`]
    ///   分类。**不能只靠「对象占位 `typ == None`」判环**：具名/别名类型在解析
    ///   本体**之前**就把自己的类型写回了对象（自引用要用它），于是「对象已有类型」
    ///   并不表示「已求值完」—— 那条路会让环悄悄消失（`type A = B; type B = A`
    ///   实测过：不报任何诊断）；
    /// * 包级条目还是占位（`typ == None`）⇒ **就地触发惰性求值**（前向引用入口）；
    /// * 查不到 ⇒ `invalid`，**不报诊断**（未定义名的代号归 Task 25）。
    pub(crate) fn lookup_type(&mut self, scope: ScopeId, name: &str) -> TypeId {
        let is_type =
            matches!(self.scopes.lookup(scope, name), Some(o) if o.kind == ObjKind::TypeName);
        if !is_type {
            return self.tt.invalid();
        }
        if matches!(self.states.get(name), Some(State::InProgress)) {
            return self.hit_cycle(name);
        }
        let unresolved = matches!(self.scopes.lookup(scope, name), Some(o) if o.typ.is_none());
        if unresolved && self.items.contains_key(name) {
            self.resolve_name(name);
        }
        match self.scopes.lookup(scope, name).and_then(|o| o.typ) {
            Some(t) => t,
            None => self.tt.invalid(),
        }
    }

    /// 检出**声明环**（官方 `decl.go` 的 `cycle()` + `initorder.go` 的分类）。
    ///
    /// 返回「环上那个**正在求值**的名字的类型」或 `invalid`：
    /// 允许的环（有定义型）必须把**占位类型**交回去，否则
    /// `type A B; type B A` 会双双变成 `invalid`，`validType` 就没得判了。
    fn hit_cycle(&mut self, start: &str) -> TypeId {
        let Some(i) = self.stack.iter().position(|n| n == start) else {
            return self.tt.invalid();
        };
        let members: Vec<String> = self.stack[i..].to_vec();
        // 官方的 `nval` / `ndef` 口径（`decl.go:212-245`）
        let kinds: Vec<ItemKind> = members
            .iter()
            .filter_map(|n| self.items.get(n).map(Item::kind))
            .collect();
        let nval = kinds.iter().filter(|k| **k == ItemKind::Const).count();
        let ndef = kinds.iter().filter(|k| **k == ItemKind::TypeDef).count();
        // 官方 `firstInSrc(cycle)`：环里**源码序最前**的成员（位置含文件序 ⇒ 直接比 Pos）
        let first = members
            .iter()
            .min_by_key(|n| self.items.get(*n).map(Item::pos))
            .cloned()
            .unwrap_or_else(|| start.to_string());
        let Some(first_pos) = self.items.get(&first).map(Item::pos) else {
            return self.tt.invalid();
        };

        if nval == members.len() {
            // 全是值 ⇒ 官方由 initorder 报；环里每个值都作废
            self.bag.push(Diag {
                pos: first_pos,
                code: DiagCode::TypInvalidInitCycle,
                message: format!("initialization cycle for {first}"),
            });
            for m in &members {
                self.poisoned.insert(m.clone());
            }
            return self.tt.invalid();
        }
        if nval == 0 && ndef > 0 {
            // 有定义型 ⇒ 官方允许其走到 `validType`（「按值包含」才判非法）。
            // 本层**不报**；只把非起点成员记下来，免得 `validType` 重复报告。
            for m in &members {
                if *m != first {
                    self.cycle_skip.insert(m.clone());
                }
            }
            return self.object_typ(start).unwrap_or_else(|| self.tt.invalid());
        }
        // 纯别名 / 值+别名混合 ⇒ 官方 `cycleError`。
        // 官方两条文案：单成员 `%s refers to itself`（`decl.go:279`）、
        // 多成员列成 `A refers to B refers to A`（`decl.go:286` 那支的续行）。
        let message = if members.len() == 1 {
            format!("invalid recursive type: {first} refers to itself")
        } else {
            let chain = members
                .iter()
                .chain(std::iter::once(&members[0]))
                .cloned()
                .collect::<Vec<_>>()
                .join(" refers to ");
            format!("invalid recursive type: {chain}")
        };
        self.bag.push(Diag {
            pos: first_pos,
            code: DiagCode::TypInvalidDeclCycle,
            message,
        });
        self.poisoned.insert(first.clone());
        self.tt.invalid()
    }

    /// 取包作用域里一个对象的类型。
    fn object_typ(&self, name: &str) -> Option<TypeId> {
        self.scopes.lookup(self.pkg_scope, name)?.typ
    }

    /// 写回一个包级对象的类型 / 值（占位对象由 Task 23 建，一定在）。
    fn write_object(&mut self, name: &str, typ: TypeId, val: Option<Const>) {
        // 先把要用的两栏拷出来，别让 `lookup` 的借用活到 `insert`
        let Some((kind, pos)) = self
            .scopes
            .lookup(self.pkg_scope, name)
            .map(|o| (o.kind, o.pos))
        else {
            return;
        };
        let obj = Object {
            kind,
            name: name.to_string(),
            typ: Some(typ),
            val,
            pos,
        };
        self.scopes.insert(self.pkg_scope, obj);
    }

    /// 常量的**声明类型**（记忆化）+ 它的 dump 根。
    ///
    /// 记忆化不是优化而是**正确性**：`struct{...}` / `[]T` 这类结构型在
    /// `C7` 里**不去重**，解析两次就会多出两个条目 ⇒ dump 与 oracle 对不上。
    fn const_root_ty(&mut self, name: &str) -> Option<TypeId> {
        let Some(Item::Const { ty_expr, ty, .. }) = self.items.get(name).cloned() else {
            return None;
        };
        if let Some(t) = ty {
            return Some(t);
        }
        let e = ty_expr?;
        let scope = self.decl_scope();
        let t = self.resolve_type(scope, &e);
        if let Some(Item::Const { ty, .. }) = self.items.get_mut(name) {
            *ty = Some(t);
        }
        Some(t)
    }

    /// 没有声明类型时，初始化式**自身**的类型（无类型基本类型 / 转换目标）。
    fn const_type_of(&mut self, e: &Expr) -> Option<TypeId> {
        match e {
            Expr::BasicLit(l) => {
                let b = untyped_of_lit(l.kind)?;
                Some(self.untyped_basic(b))
            }
            Expr::Name(n) => match n.value.as_str() {
                "true" | "false" => Some(self.untyped_basic(BasicId::UntypedBool)),
                "iota" => Some(self.untyped_basic(BasicId::UntypedInt)),
                // 已求值的常量对象：它的 `typ` 就是答案（无类型常量存无类型基本类型）
                other => self
                    .scopes
                    .lookup(self.pkg_scope, other)
                    .and_then(|o| o.typ),
            },
            Expr::Paren(p) => self.const_type_of(&p.x),
            Expr::Unary(op) => match op.op {
                Op::Not => Some(self.untyped_basic(BasicId::UntypedBool)),
                // `+x` / `-x` / `^x` 保种类（实测：`-'a'` 是 `untyped rune`）
                _ => self.const_type_of(&op.x),
            },
            Expr::Binary(op) => self.const_type_of_binary(op),
            Expr::Call(c) => self.const_type_of_call(c),
            _ => None,
        }
    }

    /// 二元运算的结果类型（无类型种别的合并，实测口径见 `M3-tests.md` 的 `T-TY-83` 邻域）。
    fn const_type_of_binary(&mut self, op: &rgoc_ast::Operation) -> Option<TypeId> {
        use Op::*;
        match op.op {
            Eql | Neq | Lss | Leq | Gtr | Geq | LAnd | LOr => {
                Some(self.untyped_basic(BasicId::UntypedBool))
            }
            // 移位：种类取**左操作数**（实测 `1 << 3` 是 `untyped int`）
            Shl | Shr => self.const_type_of(&op.x),
            _ => {
                let x = self.const_type_of(&op.x)?;
                let y = op.y.as_deref().and_then(|y| self.const_type_of(y));
                // 有一侧是**类型化**常量（如 `int8(1)`）⇒ 结果就是它的类型
                if !self.is_untyped(x) {
                    return Some(x);
                }
                if let Some(y) = y
                    && !self.is_untyped(y)
                {
                    return Some(y);
                }
                // 两侧都无类型 ⇒ 取「宽」的那个（complex > float > rune > int；string 单列）
                let y = y.unwrap_or(x);
                Some(self.wider_untyped(x, y))
            }
        }
    }

    /// 转换 `T(x)` 的结果类型（`T` 是类型名 / 具名类型 ⇒ 类型化常量）。
    fn const_type_of_call(&mut self, c: &rgoc_ast::CallExpr) -> Option<TypeId> {
        if c.args.len() != 1 || c.has_dots {
            return None;
        }
        let Expr::Name(f) = &*c.fun else {
            return None; // `unsafe.Sizeof(x)` / `pkg.F(x)`：M3 不折（`U-M3-08`）
        };
        let t = self.lookup_type(self.pkg_scope, &f.value);
        if t == self.tt.invalid() {
            return None;
        }
        Some(t)
    }

    /// 是不是**无类型**基本类型条目。
    fn is_untyped(&self, t: TypeId) -> bool {
        matches!(self.tt.type_of(t), Type::Basic(b) if b.is_untyped())
    }

    /// 两个无类型基本类型里「宽」的那个。
    fn wider_untyped(&mut self, x: TypeId, y: TypeId) -> TypeId {
        let rank = |tt: &TypeTable, t: TypeId| match tt.type_of(t) {
            Type::Basic(b) => match b {
                BasicId::UntypedBool => 0,
                BasicId::UntypedInt => 1,
                BasicId::UntypedRune => 2,
                BasicId::UntypedFloat => 3,
                BasicId::UntypedComplex => 4,
                BasicId::UntypedString => 5,
                _ => 0,
            },
            _ => 0,
        };
        if rank(self.tt, y) > rank(self.tt, x) {
            y
        } else {
            x
        }
    }

    /// 无类型基本类型的 `TypeId`（按种类缓存；`add_basic` 本身不去重）。
    fn untyped_basic(&mut self, b: BasicId) -> TypeId {
        if let Some(t) = self.untyped_cache.get(&b) {
            return *t;
        }
        let t = self.tt.add_basic(b);
        self.untyped_cache.insert(b, t);
        t
    }

    /// 一个类型的**本体上的 `Basic`**（沿别名 / 具名下钻），用于可表示性判定。
    fn underlying_basic(&self, t: TypeId) -> Option<BasicId> {
        match self.tt.type_of(t) {
            Type::Basic(b) => Some(*b),
            Type::Named(n) => {
                let u = self.tt.named_def(*n).underlying?;
                self.underlying_basic(u)
            }
            Type::Alias(a) => {
                let r = self.tt.alias_def(*a).rhs?;
                self.underlying_basic(r)
            }
            _ => None,
        }
    }

    /// 诊断里用的类型名（基本类型用规范拼写；具名类型用其名字）。
    fn type_name(&self, t: TypeId) -> String {
        match self.tt.type_of(t) {
            Type::Basic(b) => b.as_str().to_string(),
            Type::Named(n) => self.tt.named_def(*n).name.clone(),
            Type::Alias(a) => self.tt.alias_def(*a).name.clone(),
            _ => "type".to_string(),
        }
    }

    /// 常量表达式求值（带 `iota`；类型位置传 `None`）。
    pub(crate) fn eval_const_in(&mut self, e: &Expr, iota: Option<usize>) -> Option<Const> {
        let view = ConstView {
            scopes: self.scopes,
            scope: self.pkg_scope,
            iota: iota.map(|i| Const::Int(BigInt::from_i64(i as i64))),
        };
        let (out, diags) = eval_const_expr(e, &view, &self.sizes);
        for d in diags.into_sorted() {
            self.bag.push(d);
        }
        out.value().cloned()
    }
}

/// 字面量种类 → 无类型基本类型。
fn untyped_of_lit(kind: LitKind) -> Option<BasicId> {
    Some(match kind {
        LitKind::Int => BasicId::UntypedInt,
        LitKind::Float => BasicId::UntypedFloat,
        LitKind::Imag => BasicId::UntypedComplex,
        LitKind::Rune => BasicId::UntypedRune,
        LitKind::String => BasicId::UntypedString,
    })
}

/// 初始化式里引用的**名字**（递归收集；调用方再按「是不是包级条目」过滤）。
///
/// 为什么不做成完整的 AST 访问器：这里只要「哪些常量可能参与求值」，
/// 多收几个无害（顶多多解析一个已经要用的类型名），漏收才会出**假**的
/// `TypInvalidConstInit`。所以只列**支持面**里的节点，其余一律不下钻。
fn const_deps(e: &Expr) -> Vec<String> {
    let mut out = Vec::new();
    fn go(e: &Expr, out: &mut Vec<String>) {
        match e {
            Expr::Name(n) => out.push(n.value.clone()),
            Expr::BasicLit(_) => {}
            Expr::Paren(p) => go(&p.x, out),
            Expr::Unary(op) => go(&op.x, out),
            Expr::Binary(op) => {
                go(&op.x, out);
                if let Some(y) = &op.y {
                    go(y, out);
                }
            }
            Expr::Call(c) => {
                go(&c.fun, out);
                for a in &c.args {
                    go(a, out);
                }
            }
            // 索引 / 切片 / 选择子里的名字也要收：表达式**整体**会落到
            // 「非常量」的拒绝侧，但里面的名字该求的还是要先求出来 ——
            // 否则那个常量会停在「对象已建、没有值」的中间态，
            // 而中间态在 `ConstEnv` 眼里是 `unknown` ⇒ 求值方**静默**拿到
            // 「常量但无值」而不报任何诊断（假阴性，且极难查）。
            Expr::Index(ix) => {
                go(&ix.x, out);
                go(&ix.index, out);
            }
            Expr::Slice(sl) => {
                go(&sl.x, out);
                for e in [&sl.low, &sl.high, &sl.max].into_iter().flatten() {
                    go(e, out);
                }
            }
            Expr::Selector(sel) => go(&sel.x, out),
            _ => {}
        }
    }
    go(e, &mut out);
    out
}

/// 表达式文本（诊断消息用；**不承诺与官方逐字一致**，`D-M1-9`）。
///
/// 支持面之外的节点渲染成 `…`：消息只在**人读**时有用，为一个诊断去写完整
/// 表达式打印机会把「另一份真相」造出来（`rgoc_ast::dump_expr` 是 C6 契约格式，
/// 多行，不适合塞进一行诊断）。
fn expr_text(e: &Expr) -> String {
    match e {
        Expr::Bad(_) => "?".to_string(),
        Expr::Name(n) => n.value.clone(),
        Expr::BasicLit(l) => l.value.clone(),
        Expr::Paren(p) => format!("({})", expr_text(&p.x)),
        Expr::Unary(op) => {
            if op.op == Op::Mul && op.y.is_none() {
                format!("*{}", expr_text(&op.x))
            } else {
                format!("{}{}", rgoc_ast::op_str(op.op), expr_text(&op.x))
            }
        }
        Expr::Binary(op) => match &op.y {
            Some(y) => format!(
                "{} {} {}",
                expr_text(&op.x),
                rgoc_ast::op_str(op.op),
                expr_text(y)
            ),
            None => expr_text(&op.x),
        },
        Expr::Call(c) => format!(
            "{}({})",
            expr_text(&c.fun),
            c.args.iter().map(expr_text).collect::<Vec<_>>().join(", ")
        ),
        Expr::Selector(s) => format!("{}.{}", expr_text(&s.x), s.sel.value),
        Expr::Index(ix) => format!("{}[{}]", expr_text(&ix.x), expr_text(&ix.index)),
        _ => "…".to_string(),
    }
}
