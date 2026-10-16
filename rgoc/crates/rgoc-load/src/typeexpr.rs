//! 类型表达式解析：语法树 `Expr` → 类型表 `TypeId`（M3/Task 24）。
//!
//! # 为什么单开一个文件
//!
//! [`crate::decls`] 管的是**顺序与环**（谁先求值、前向引用、声明环），
//! 本文件管的是**产生式覆盖**（`typeOrNil` 的每一种写法 → 哪一种 `Type`）。
//! 两者都是 `impl Checker` 的方法，拆开只是因为失败面不同：
//! 顺序错了是「诊断不见了」，覆盖漏了是「某个类型形状悄悄变成 `invalid`」。
//!
//! # 与官方的对应
//!
//! | 这里 | 官方 |
//! |---|---|
//! | [`Checker::resolve_type`] | `Checker.typExpr` / `typeInternal`（`go/types/expr.go`） |
//! | `resolve_struct` | `Checker.structType`（`struct.go`） |
//! | `resolve_interface` / `collect_iface_elem` | `Checker.interfaceType` + `embeddedElem`（`interface.go`） |
//! | `resolve_signature` | `Checker.signature`（`signature.go`） |
//! | `declare_tparams` | `Checker.typeParams`（`tparams.go`） |
//!
//! # 刻意**不**做的事（都登记在 `M3-manifest.json` 的 `open_gaps`）
//!
//! * **类型位置的未定义名** ⇒ `invalid`，**不报诊断**（`Undefined` 的归属是
//!   Task 25 的块作用域，那里才知道「这个名字是不是某个内层作用域漏出来的」）；
//! * **非类型表达式出现在类型位置**（`var x 1+2`）⇒ `invalid`，**不报诊断**
//!   （官方 `NotAType` 需要「值 vs 类型」的完整判定，那是 M4）；
//! * **限定名**（`pkg.T`）⇒ `invalid` —— M3 不解析 import 路径（Task 27 才做），
//!   这里没有「哪个包」的信息，编一个名字出来比 `invalid` 更坏；
//! * **实例化**（`X[A]`）⇒ 退化为 `X` 的类型（`T-TY-29` 钉住的边界：
//!   参数化具名的 `origin + targs` 表示不在 M3）；类型实参**照样解析**，
//!   因为它们是真实的名字引用（前向引用必须能被触发）。
//!
//! # 接口的简化（`D-M3-13` 的直接后果）
//!
//! 项集（`~T` / 并集）与嵌入**照样表示**（`Type::Union` + `InterfaceDef::terms`），
//! 但**不判定**它们的同一性。嵌入式接口的合并按「**当时**已求出的本体」：
//! 具名接口嵌入先触发一次惰性解析，所以前向引用也能合并到 —— 除非那个名字
//! 本身就解析失败（那时合并为空集，与官方「不产生后续错误」的口径一致）。

use rgoc_ast::{
    ArrayType, ChanType, Expr, Field as AstField, FuncType, InterfaceType, MapType, Op, StructType,
};
use rgoc_const::Const;
use rgoc_types::{ChanDir, Field as TField, ObjKind, Object, ScopeId, Type, TypeId};

use crate::decls::Checker;

/// 接口的三个**部件**：方法集（名字已排序）、项集（`Type::Union` 的 `TypeId`）、
/// `comparable` 位。抽成别名只是因为它在签名里出现三次。
type IfaceParts = (Vec<(String, TypeId)>, Option<TypeId>, bool);

impl Checker<'_> {
    /// 解析一个类型表达式。
    ///
    /// 覆盖官方 `typeOrNil` 的全部产生式（见模块头）；认不出的写法一律
    /// `invalid` —— **不猜**（猜出来的类型会污染同一性与 dump，比 `invalid` 坏得多）。
    pub(crate) fn resolve_type(&mut self, scope: ScopeId, e: &Expr) -> TypeId {
        match e {
            // 错误恢复占位：上游已经报过错，这里不再造第二条
            Expr::Bad(_) => self.tt.invalid(),
            Expr::Name(n) => self.lookup_type(scope, &n.value),
            Expr::Paren(p) => self.resolve_type(scope, &p.x),

            // 一元 `*T`（指针）。`Operation.y == None` 是一元的判据。
            Expr::Unary(op) if op.op == Op::Mul && op.y.is_none() => {
                let elem = self.resolve_type(scope, &op.x);
                self.tt.add_pointer(elem)
            }
            // `~T` 只在接口项集里有意义（`Op::Tilde` 的唯一产地，见 `aux.rs`）
            Expr::Unary(_) => self.tt.invalid(),

            Expr::ArrayType(a) => self.resolve_array(scope, a),
            Expr::SliceType(s) => {
                let elem = self.resolve_type(scope, &s.elem);
                self.tt.add_slice(elem)
            }
            // `...T` 只在参数表的末位有意义（`resolve_signature` 直接认它），
            // 出现在别处是语法错误的漏网，不是类型
            Expr::DotsType(_) => self.tt.invalid(),
            Expr::StructType(s) => self.resolve_struct(scope, s),
            Expr::MapType(m) => self.resolve_map(scope, m),
            Expr::ChanType(c) => self.resolve_chan(scope, c),
            Expr::InterfaceType(i) => self.resolve_interface(scope, i),
            Expr::FuncType(f) => self.resolve_signature(scope, f, None),

            // 实例化 `X[A, B]`：**退化**为 `X` 自身（`T-TY-29`）。
            // 类型实参照样解析 —— 它们是真名字引用，前向引用与环都要能被触发。
            Expr::Index(ix) => {
                let base = self.resolve_type(scope, &ix.x);
                let _ = self.resolve_type(scope, &ix.index);
                base
            }

            // 其余（限定名 `pkg.T`、二元并集、索引/切片/调用/字面量…）不是类型
            _ => self.tt.invalid(),
        }
    }

    /// `[N]T` / `[...]T`。
    ///
    /// 长度必须是**可折叠的非负整数**（`spec:L1521`）。折不出来时记 `None`
    /// （「未定」），**不报诊断**：真实语料里 `[unsafe.Sizeof(x)]byte` 这类
    /// 非常常见，而「数组长度不是常量」的官方代号（`InvalidArrayLen`）
    /// M3 不引入（没有别的触发者）⇒ 进 `open_gaps`。
    fn resolve_array(&mut self, scope: ScopeId, a: &ArrayType) -> TypeId {
        let len = a.len.as_deref().and_then(|e| self.array_len(e));
        let elem = self.resolve_type(scope, &a.elem);
        self.tt.add_array(len, elem)
    }

    /// 数组长度表达式 → `ConstId`（不可折叠 / 负数 / 非整数 ⇒ `None`）。
    fn array_len(&mut self, e: &Expr) -> Option<rgoc_const::ConstId> {
        // 类型位置没有 `iota`（`[iota]int` 是错的），所以 `iota` 传 `None`
        let v = self.eval_const_in(e, None)?;
        let Const::Int(n) = v else { return None };
        if n.is_neg() {
            return None;
        }
        Some(self.tt.consts_mut().add(Const::Int(n)))
    }

    /// `struct{...}`。
    ///
    /// 字段名 / 标签 / 嵌入标记的判据（`C7` §2.3）：
    /// * 嵌入字段（`name == None`）⇒ 名字位 `-`，`embedded = true`；
    /// * 标签存**字面量内容**（去掉反引号 / 引号），与 `dump.rs` 的重新加引号配对。
    fn resolve_struct(&mut self, scope: ScopeId, s: &StructType) -> TypeId {
        let mut fields: Vec<TField> = Vec::with_capacity(s.fields.len());
        for f in &s.fields {
            let embedded = f.name.is_none();
            let typ = self.resolve_type(scope, &f.type_);
            fields.push(TField {
                name: if embedded {
                    None
                } else {
                    Some(f.name.as_ref().map(|n| n.value.clone()).unwrap_or_default())
                },
                typ,
                tag: f.tag.as_ref().and_then(tag_text),
                embedded,
            });
        }
        let id = self.tt.add_struct(fields);
        self.tt.struct_type(id)
    }

    /// `map[K]V`。
    fn resolve_map(&mut self, scope: ScopeId, m: &MapType) -> TypeId {
        let key = self.resolve_type(scope, &m.key);
        let elem = self.resolve_type(scope, &m.value);
        self.tt.add_map(key, elem)
    }

    /// `chan T` / `chan<- T` / `<-chan T`（`dir == None` ⇒ 双向）。
    fn resolve_chan(&mut self, scope: ScopeId, c: &ChanType) -> TypeId {
        let elem = self.resolve_type(scope, &c.elem);
        let dir = match c.dir {
            None => ChanDir::SendRecv,
            Some(rgoc_ast::ChanDir::SendOnly) => ChanDir::SendOnly,
            Some(rgoc_ast::ChanDir::RecvOnly) => ChanDir::RecvOnly,
        };
        self.tt.add_chan(dir, elem)
    }

    /// `interface{...}`：方法集 + 项集 + `comparable` 位。
    ///
    /// 三类元素（官方 `interfaceType` 的 `embedded` 分支）：
    /// 1. 有名字 ⇒ **方法**，其类型是签名；
    /// 2. 无名且是**接口**（含具名接口 / 别名）⇒ **嵌入合并**（方法 + 项集 + comparable 位）；
    /// 3. 其余无名 ⇒ **项**（`~T`、`A | B`、单个非接口类型名都归这里）。
    fn resolve_interface(&mut self, scope: ScopeId, i: &InterfaceType) -> TypeId {
        let mut methods: Vec<(String, TypeId)> = Vec::new();
        let mut terms: Vec<(bool, TypeId)> = Vec::new();
        let mut comparable = false;

        for f in &i.methods {
            match &f.name {
                Some(n) => {
                    let sig = self.resolve_type(scope, &f.type_);
                    methods.push((n.value.clone(), sig));
                }
                None => self.collect_iface_elem(
                    scope,
                    &f.type_,
                    &mut methods,
                    &mut terms,
                    &mut comparable,
                ),
            }
        }

        self.build_iface(methods, terms, comparable)
    }

    /// 把三样部件落成表里的一条 `interface` —— 显式接口与**隐式约束接口**
    /// （`declare_tparams` 里包 `~T` / `A | B` 用的那种）共用这条路。
    ///
    /// 方法排序放在**这里**而不是渲染层：`C7` §4 把它写成契约位
    /// （`interface` 行的 `method` 子行按名字升序），渲染层排序会漏掉
    /// 「同一条接口被两处引用时顺序也要一致」这条。
    fn build_iface(
        &mut self,
        mut methods: Vec<(String, TypeId)>,
        terms: Vec<(bool, TypeId)>,
        comparable: bool,
    ) -> TypeId {
        methods.sort_by(|a, b| a.0.cmp(&b.0));

        let terms = if terms.is_empty() {
            None
        } else {
            let id = self.tt.add_union(terms);
            Some(self.tt.union_type(id))
        };
        let id = self.tt.add_interface(methods, terms, comparable);
        self.tt.interface_type(id)
    }

    /// 接口里的一个**无名元素**：并入方法 / 项集 / `comparable`。
    fn collect_iface_elem(
        &mut self,
        scope: ScopeId,
        e: &Expr,
        methods: &mut Vec<(String, TypeId)>,
        terms: &mut Vec<(bool, TypeId)>,
        comparable: &mut bool,
    ) {
        match e {
            Expr::Paren(p) => self.collect_iface_elem(scope, &p.x, methods, terms, comparable),
            // `~T`：近似项
            Expr::Unary(op) if op.op == Op::Tilde => {
                let t = self.resolve_type(scope, &op.x);
                terms.push((true, t));
            }
            // `A | B`：并集（逐层展平，项的**声明序**就是遍历序）
            Expr::Binary(op) if op.op == Op::Or => {
                if let Some(y) = &op.y {
                    self.collect_iface_elem(scope, &op.x, methods, terms, comparable);
                    self.collect_iface_elem(scope, y, methods, terms, comparable);
                }
            }
            // 单个类型名：是接口就**嵌入合并**，否则是「恰好这一个类型」的项
            Expr::Name(n) => {
                let t = self.lookup_type(scope, &n.value);
                if let Some((m, ts, cmp)) = self.iface_parts(t) {
                    methods.extend(m);
                    if let Some(u) = ts {
                        terms.extend(self.union_terms(u));
                    }
                    *comparable |= cmp;
                } else {
                    terms.push((false, t));
                }
            }
            // 其余（`[]byte`、`map[K]V` …）按「非接口类型」的项处理
            _ => {
                let t = self.resolve_type(scope, e);
                terms.push((false, t));
            }
        }
    }

    /// 取一个类型的**接口部件**（沿别名/具名下钻到 `Type::Interface`）。
    ///
    /// `None` ⇒ 它不是接口类型（⇒ 在项集语境里是「恰好这一个类型」的项）。
    /// 具名接口的**本体未填**时也返回 `None` —— 调用方（`Expr::Name` 分支）
    /// 已经先触发过惰性解析，所以那只可能是「解析失败」，合并成空集正合
    /// 官方「不产生后续错误」的口径。
    fn iface_parts(&self, t: TypeId) -> Option<IfaceParts> {
        match self.tt.type_of(t) {
            Type::Interface(id) => {
                let d = self.tt.interface_def(*id);
                Some((d.methods.clone(), d.terms, d.comparable))
            }
            Type::Named(n) => {
                let u = self.tt.named_def(*n).underlying?;
                self.iface_parts(u)
            }
            Type::Alias(a) => {
                let r = self.tt.alias_def(*a).rhs?;
                self.iface_parts(r)
            }
            _ => None,
        }
    }

    /// 并集条目 → 它的项（嵌入一个并集时把项并进来）。
    fn union_terms(&self, u: TypeId) -> Vec<(bool, TypeId)> {
        match self.tt.type_of(u) {
            Type::Union(id) => self.tt.union_def(*id).terms.clone(),
            _ => Vec::new(),
        }
    }

    /// 函数类型 / 方法签名：参数元组 + 结果元组 + 变参位（`recv` 非空 ⇒ 方法）。
    ///
    /// ⚠️ **类型参数不进签名**：官方 `Signature` 有 `TypeParams()`，而本 crate 的
    /// `SignatureDef` 没有这一栏（M3 的类型参数只作为**对象**存在于作用域里，
    /// 通过字段/参数的类型位置被引用到 ⇒ dump 里照样能走到它们）。
    pub(crate) fn resolve_signature(
        &mut self,
        scope: ScopeId,
        ft: &FuncType,
        recv: Option<TypeId>,
    ) -> TypeId {
        let mut variadic = false;
        let mut params: Vec<(Option<String>, TypeId)> = Vec::with_capacity(ft.params.len());
        for f in &ft.params {
            let (t, dots) = match &f.type_ {
                // 变参：末位参数的 `...T`（parser 已保证只在末位）
                Expr::DotsType(d) => (self.resolve_type(scope, &d.elem), true),
                other => (self.resolve_type(scope, other), false),
            };
            variadic |= dots;
            params.push((field_name(f), t));
        }
        let mut results: Vec<(Option<String>, TypeId)> = Vec::with_capacity(ft.results.len());
        for f in &ft.results {
            let t = self.resolve_type(scope, &f.type_);
            results.push((field_name(f), t));
        }
        let pt = self.tt.add_tuple(params);
        let rt = self.tt.add_tuple(results);
        let id = self.tt.add_signature(recv, pt, rt, variadic);
        self.tt.signature_type(id)
    }

    /// 声明类型参数，返回**类型参数作用域**（`[]`为空时调用方不该调它）。
    ///
    /// 两遍是**必须**的：`[T any, U ~[]T]` 里 `U` 的约束引用了 `T`，
    /// 而 `T` 必须先成为对象；同时也支持 `[T U, U any]` 这种互引（官方同样允许）。
    /// 先占号后回填（`set_type_param_constraint`）是递归约束的唯一手段。
    pub(crate) fn declare_tparams(&mut self, parent: ScopeId, tparams: &[AstField]) -> ScopeId {
        let scope = self.scopes.child(rgoc_types::ScopeKind::Block, parent);
        let mut ids = Vec::with_capacity(tparams.len());
        for (i, f) in tparams.iter().enumerate() {
            let inv = self.tt.invalid();
            let tp = self.tt.add_type_param(i as u32, inv);
            let t = self.tt.type_param_type(tp);
            ids.push((tp, t));
            // 类型参数也是对象（官方 `*TypeName`），进**自己的**作用域：遮蔽关系正确
            if let Some(n) = &f.name
                && n.value != "_"
            {
                let obj = Object {
                    pos: Some(n.pos),
                    ..Object::with_type(ObjKind::TypeName, n.value.clone(), t)
                };
                self.scopes.insert(scope, obj);
            }
        }
        for ((tp, _), f) in ids.iter().zip(tparams.iter()) {
            // 约束位置上，`~T` / `A | B` 这两种写法在官方那里会被包成
            // **隐式接口**（实测 oracle：`type C[T ~[]int]` ⇒ `interface 0 1`
            // 一行 `term <union>`）—— 所以这两条**不能**走 `resolve_type`
            // （那条路对 `~T` 给 `invalid`，环表里就成了「约束 = invalid」）。
            // 显式 `interface{…}` 与纯类型名（`any` / `comparable` / `int`）走
            // `resolve_type`：实测 `[T any]` 的约束位置上出现的是 **`alias`** 条目
            // （`C7` §4.2 的注已钉），`[T comparable]` 是 universe 里那个特殊接口。
            //
            // 注意 `[T int]`（**裸**非接口类型）这类**没得对撞**：oracle 自己会
            // 在编号阶段报「类型图成环」而崩（实测，见 `M3-manifest.json`
            // 的 `open_gaps`）⇒ M3 照写出的类型表示，判定不做（`D-M3-2`）。
            let c = if is_type_set(&f.type_) {
                let mut methods = Vec::new();
                let mut terms = Vec::new();
                let mut comparable = false;
                self.collect_iface_elem(scope, &f.type_, &mut methods, &mut terms, &mut comparable);
                self.build_iface(methods, terms, comparable)
            } else {
                self.resolve_type(scope, &f.type_)
            };
            self.tt.set_type_param_constraint(*tp, c);
        }
        scope
    }
}

/// 这个类型表达式是不是**类型集**写法（`~T` / `A | B`）？
///
/// 只判这两条：显式 `interface{…}` 与纯类型名都走 `resolve_type`（见调用处的说明）。
fn is_type_set(e: &Expr) -> bool {
    match e {
        Expr::Paren(p) => is_type_set(&p.x),
        Expr::Unary(op) => op.op == Op::Tilde,
        Expr::Binary(op) => op.op == Op::Or,
        _ => false,
    }
}

/// 一个 `_` 标识符 → `None`？**不**：`_` 是合法参数名，官方如实保留。
/// 只有真正没写名字（`name == None`）才是「无名」。
fn field_name(f: &AstField) -> Option<String> {
    f.name.as_ref().map(|n| n.value.clone())
}

/// 结构体标签字面量 → **内容**（去掉反引号或引号；转义按 Go 字符串规则解开）。
///
/// 与 `dump.rs` 配对：那边对内容重新加引号，所以这里**不能**留引号，
/// 否则两侧都会多一层（`""json:\"a\"""`）。
fn tag_text(lit: &rgoc_ast::BasicLit) -> Option<String> {
    match rgoc_const::const_from_literal(lit) {
        Const::Str(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        _ => None,
    }
}
