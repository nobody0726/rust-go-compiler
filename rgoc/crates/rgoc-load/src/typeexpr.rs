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
//! * **类型位置的未定义名** ⇒ 报 [`DiagCode::TypUndeclaredName`]（`undefined: X`）
//!   并给 `invalid` —— Task 25 落地；**只看「这个名字在作用域链里存不存在」**，
//!   「存在但不是类型」是官方 `NotAType`（M4，`open_gaps`）；
//! * **非类型表达式出现在类型位置**（`var x 1+2`）⇒ `invalid`，**不报诊断**
//!   （官方 `NotAType` 需要「值 vs 类型」的完整判定，那是 M4）；
//! * **限定名**（`pkg.T`）⇒ `invalid` —— M3 不解析 import 路径（Task 27 才做），
//!   这里没有「哪个包」的信息，编一个名字出来比 `invalid` 更坏；
//! * **实例化**（`X[A]`）⇒ 退化为 `X` 的类型（`T-TY-29` 钉住的边界：
//!   参数化具名的 `origin + targs` 表示不在 M3）；类型实参**照样解析**，
//!   因为它们是真实的名字引用（前向引用必须能被触发）。
//!
//! # 接口的**两面**（M4/Task 8 起）
//!
//! [`IfaceTSet`] 是**语义**面（交集 / `comparable` 过滤 / ∅ 可达 —— 判定吃它），
//! `InterfaceDef::terms` 是**语法**面（`~T` / `A | B` 拍平 + 嵌入元素 as-written
//! —— `C7` 渲染与 oracle 对撞吃它）。两面**不必同形**
//! （`interface{ comparable; ~[]byte }`：语法两条、语义 ∅）。
//!
//! 嵌入的合并序**对齐官方** `computeInterfaceTypeSet`：显式方法全部先行、
//! 再按源码序处理无名元素；方法按名去重**首见保留**（官方 `objset` 键是
//! `Id(pkg, name)`；v1 不带包 —— 已知差异，台账 `Ruling 8-4`）。环上成员
//!（本体未填）按官方 `!ityp.complete` ⇒ `topTypeSet` 的宽容口径处理：
//! **合并空**、照记语法项，不 panic、不产生后续错误。

use rgoc_ast::{
    ArrayType, ChanType, Expr, Field as AstField, FuncType, InterfaceType, MapType, Op, StructType,
};
use rgoc_const::{BasicId, Const};
use rgoc_lex::{Diag, DiagCode};
use rgoc_types::{
    ChanDir, Field as TField, IfaceTSet, InterfaceId, ObjKind, Object, ScopeId, Type, TypeId,
    TypeParamId, identical, typeset, unalias, underlying,
};

use crate::decls::Checker;

/// [`Checker::drill_iface`] 的分类结果。
enum Drill {
    /// 本体是接口（可嵌入合并）。
    Iface(InterfaceId),
    /// 本体**未填**（正在解析的环成员 —— 官方 `!ityp.complete` 面）。
    Unresolved,
    /// 确定不是接口。
    NotIface,
}

impl Checker<'_> {
    /// 解析一个类型表达式。
    ///
    /// 覆盖官方 `typeOrNil` 的全部产生式（见模块头）；认不出的写法一律
    /// `invalid` —— **不猜**（猜出来的类型会污染同一性与 dump，比 `invalid` 坏得多）。
    pub(crate) fn resolve_type(&mut self, scope: ScopeId, e: &Expr) -> TypeId {
        match e {
            // 错误恢复占位：上游已经报过错，这里不再造第二条
            Expr::Bad(_) => self.tt.invalid(),
            // 类型位置的**未定义名**在这里与值位置走同一条代号（官方也是同一条
            // `UndeclaredName`：类型位置 `typexpr.go:30`、值位置由解析器收尾统一报）。
            // ⚠️ 只有「整个名字都找不到」才报：找到了但不是类型（`var x int; var y x`）
            // 是官方 `NotAType`，归 M4（`open_gaps`）。
            Expr::Name(n) => {
                // 空标识符**不是类型**：官方 `typexpr.go:28` 的 `InvalidBlank`
                // （“cannot use _ as value **or type**”）——不能走 `undefined: _`。
                if n.value == "_" {
                    self.bag.push(Diag {
                        pos: n.pos,
                        code: DiagCode::TypInvalidBlank,
                        message: "cannot use _ as value or type".to_owned(),
                    });
                    return self.tt.invalid();
                }
                if self.scopes.lookup(scope, &n.value).is_none() {
                    self.report_undefined(n.pos, &n.value);
                }
                self.lookup_type(scope, &n.value)
            }
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

            // `unsafe.Pointer`（M4/Task 31）：`unsafe` 是**编译器伪包** ——
            // 标准库里没有 `src/unsafe` 目录，装载面装不出它的对象表，所以
            // 限定名一律 `invalid`（上面那个通配臂）。但 `Pointer` 不是包的
            // 导出类型名，它是**预声明基本类型** `BasicId::UnsafePointer`
            // （`C7` §4.1），所以这一条限定名可以就地认。
            //
            // ⚠️ 这里**不看**该文件有没有 `import "unsafe"`（`resolve_type`
            // 只拿得到作用域）：官方对未导入的 `unsafe.Pointer` 会报
            // `undefined: unsafe`，而装载面本来就把限定名静默降级成 `invalid`
            // ⇒ 认下来**不比现状差**，且让 `var p unsafe.Pointer` 有了真类型
            // （否则 `unsafe.Sizeof(p)` 算不出尺寸，Task 30 的折叠面跟着瞎）。
            Expr::Selector(s) if is_unsafe_pointer(s) => self.tt.add_basic(BasicId::UnsafePointer),

            // 限定名 `pkg.T`（M4/Task 35，`T-CK-74`）。
            Expr::Selector(s) => self.resolve_qualified(s),

            // 其余（二元并集、索引/切片/调用/字面量…）不是类型
            _ => self.tt.invalid(),
        }
    }

    /// `pkg.X` 出现在**类型位置**：先判 `pkg` 是不是本文件的 import 名，
    /// 再按官方 `call.go:718-760` 的三条判据取对象。
    ///
    /// | 情形 | 官方 | 这里 |
    /// |---|---|---|
    /// | `pkg` 不是包名（普通变量的选择器） | 走字段/方法查找 | `None` 臂 ⇒ 交给 [`Expr::Selector`] 的默认 `invalid` |
    /// | 包在、`X` 不在 | `undefined: pkg.X`（`UndeclaredImportedName`） | [`DiagCode::TypUndeclaredImportedName`] |
    /// | 包在、`X` 在但**没导出** | `name X not exported by package Y`（`UnexportedName`） | [`DiagCode::TypUnexportedName`]（**照常取类型**，官方 `// ok to continue`） |
    /// | 包在、`X` 在且导出 | 取该对象的类型 | 同 |
    ///
    /// 报点一律是 `s.sel`（`.` 后那个名字），与官方 `check.errorf(e.Sel, …)` 一致。
    fn resolve_qualified(&mut self, s: &rgoc_ast::SelectorExpr) -> TypeId {
        let Expr::Name(n) = &*s.x else {
            return self.tt.invalid();
        };
        let Some((dep_name, obj)) = self.imported_member(&n.value, &s.sel.value) else {
            return self.tt.invalid();
        };
        let Some(obj) = obj else {
            let hint = self.imported_hint(&n.value, &s.sel.value);
            self.bag.push(Diag {
                pos: s.sel.pos,
                code: DiagCode::TypUndeclaredImportedName,
                message: crate::exported::undeclared_imported_message(
                    &n.value,
                    &s.sel.value,
                    hint.as_deref(),
                ),
            });
            return self.tt.invalid();
        };
        if !crate::exported::is_exported(&s.sel.value) {
            self.bag.push(Diag {
                pos: s.sel.pos,
                code: DiagCode::TypUnexportedName,
                message: crate::exported::unexported_message(&s.sel.value, &dep_name),
            });
        }
        match obj.typ {
            Some(t) => t,
            None => self.tt.invalid(),
        }
    }

    /// 忽略大小写的近似名（官方 `lookupIgnoringCase(sel, true)`：**只看导出名**）。
    ///
    /// `exported = true` 那个参数是官方 `call.go:739` 传的常量，含义是
    /// 「候选必须是导出名」—— 因为非导出名对包外本来就不存在，提示它等于泄露。
    fn imported_hint(&self, name: &str, sel: &str) -> Option<String> {
        let file = self.cur_file?;
        let path = self
            .bindings
            .get(&file)?
            .iter()
            .find(|(n, _)| n == name)?
            .1
            .clone();
        let dep = self.imports.get(&path)?;
        let lower = sel.to_lowercase();
        dep.scopes
            .get(dep.scope)
            .names()
            .filter(|n| crate::exported::is_exported(n))
            .find(|n| n.to_lowercase() == lower)
            .map(str::to_owned)
    }

    /// `name.sel` 是不是一个**限定名**：`name` 是本文件引入的包名。
    ///
    /// 返回 `(被导入包的包名, 该包作用域本层里 `sel` 的对象)`：
    ///
    /// * `None` 整体 ⇒ `name` **不是**包名 —— 调用方走普通选择器臂；
    /// * `Some((_, None))` ⇒ 是包名、但包里没有 `sel`（调用方报 `undefined: pkg.X`）；
    /// * `Some((pkg, Some(obj)))` ⇒ 找到了。
    ///
    /// ⚠️ 只查**包作用域本层**（`lookup_local`）：官方是 `pkg.scope.Lookup(sel)`，
    /// 而 `Scope.Lookup` **不**沿父链走（universe 的 `int` 不是 `lib.int`）。
    fn imported_member(&self, name: &str, sel: &str) -> Option<(String, Option<Object>)> {
        let file = self.cur_file?;
        let path = self
            .bindings
            .get(&file)?
            .iter()
            .find(|(n, _)| n == name)?
            .1
            .clone();
        let dep = self.imports.get(&path)?;
        let obj = dep.scopes.get(dep.scope).lookup_local(sel).cloned();
        Some((dep.name.clone(), obj))
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

    /// `interface{...}`：方法集 + **两面**项集（语法 `terms` / 语义 `tset`）。
    ///
    /// 官方 `computeInterfaceTypeSet` 的两段式（`typeset.go:240+`）：
    ///
    /// 1. **显式方法全部先行**（`ityp.methods` 序，`addMethod(..., true)`），
    ///    按名去重**首见保留**；
    /// 2. 再按源码序处理**无名元素**（`addMethod(..., false)` 做方法合并 +
    ///    `intersectTermLists` 做项面与 `comparable` 位求交）。
    ///
    /// 语法面（`terms`，oracle `synthUnion` 口径）：无名元素**逐条拍平** ——
    /// 嵌入具名 / 别名 ⇒ 一条 **as-written 的 exact 项**；`~T` / `A | B` ⇒
    /// 原始操作数逐条 splice；非接口字面量 ⇒ 一条 exact 项。
    fn resolve_interface(&mut self, scope: ScopeId, i: &InterfaceType) -> TypeId {
        let mut methods: Vec<(String, TypeId)> = Vec::new();
        let mut terms: Vec<(bool, TypeId)> = Vec::new();
        let mut tset = IfaceTSet::All;

        // ① 显式方法先行（官方 `for _, m := range ityp.methods { addMethod(m, true) }`）
        for f in &i.methods {
            if let Some(n) = &f.name {
                let sig = self.resolve_type(scope, &f.type_);
                if !methods.iter().any(|(m, _)| m == &n.value) {
                    methods.push((n.value.clone(), sig));
                }
            }
        }
        // ② 无名元素按源码序
        for f in &i.methods {
            if f.name.is_none() {
                self.collect_iface_elem(scope, &f.type_, &mut methods, &mut terms, &mut tset);
            }
        }

        self.build_iface(methods, terms, tset)
    }

    /// 把三样部件落成表里的一条 `interface` —— 显式接口与**隐式约束接口**
    /// （`declare_tparams` 里包 `~T` / `A | B` 用的那种）共用这条路。
    ///
    /// 方法排序放在**这里**而不是渲染层：`C7` §4 把它写成契约位
    /// （`interface` 行的 `method` 子行按名字升序），渲染层排序会漏掉
    /// 「同一条接口被两处引用时顺序也要一致」这条。
    ///
    /// `tset` 是 [`IfaceTSet`]（**语义**面）；语法面 `terms` 为空 ⇒
    /// `InterfaceDef::terms = None`（「无项线」）。
    fn build_iface(
        &mut self,
        mut methods: Vec<(String, TypeId)>,
        terms: Vec<(bool, TypeId)>,
        tset: IfaceTSet,
    ) -> TypeId {
        methods.sort_by(|a, b| a.0.cmp(&b.0));

        let terms = if terms.is_empty() {
            None
        } else {
            let id = self.tt.add_union(terms);
            Some(self.tt.union_type(id))
        };
        let id = self.tt.add_interface(methods, terms, tset);
        self.tt.interface_type(id)
    }

    /// 接口里的一个**无名元素**：方法拍平 + 语法项 + 语义类型集求交。
    fn collect_iface_elem(
        &mut self,
        scope: ScopeId,
        e: &Expr,
        methods: &mut Vec<(String, TypeId)>,
        terms: &mut Vec<(bool, TypeId)>,
        tset: &mut IfaceTSet,
    ) {
        match e {
            Expr::Paren(p) => self.collect_iface_elem(scope, &p.x, methods, terms, tset),
            // `~T`：一个**单元素 union**（官方 `case *Union`）
            Expr::Unary(op) if op.op == Op::Tilde => {
                let t = self.resolve_type(scope, &op.x);
                let items = vec![(true, t)];
                terms.extend(items.iter().copied());
                let elem = self.union_elem_tset(&items);
                self.merge_tset(tset, elem);
            }
            // `A | B`：并集（逐层展平，项的**声明序**就是遍历序）
            Expr::Binary(op) if op.op == Op::Or => {
                if let Some(y) = &op.y {
                    let mut items: Vec<(bool, TypeId)> = Vec::new();
                    self.collect_union_operands(scope, &op.x, &mut items);
                    self.collect_union_operands(scope, y, &mut items);
                    terms.extend(items.iter().copied());
                    let elem = self.union_elem_tset(&items);
                    self.merge_tset(tset, elem);
                }
            }
            // 单个类型名：是接口就**嵌入合并**，否则是「恰好这一个类型」的项
            Expr::Name(n) => {
                let t = self.lookup_type(scope, &n.value);
                self.merge_embedded(t, methods, terms, tset);
            }
            // 其余（`[]byte`、`map[K]V` …）按「非接口类型」的元素处理
            _ => {
                let t = self.resolve_type(scope, e);
                self.merge_embedded(t, methods, terms, tset);
            }
        }
    }

    /// union 元素的**操作数**（`~T` / `A | B` 展平）。
    ///
    /// 与 [`Self::collect_iface_elem`] 的区别：**不并入方法** —— 官方
    /// `computeUnionTypeSet` 断言 union 无方法（并集操作数只贡献项）。
    fn collect_union_operands(
        &mut self,
        scope: ScopeId,
        e: &Expr,
        items: &mut Vec<(bool, TypeId)>,
    ) {
        match e {
            Expr::Paren(p) => self.collect_union_operands(scope, &p.x, items),
            Expr::Unary(op) if op.op == Op::Tilde => {
                let t = self.resolve_type(scope, &op.x);
                items.push((true, t));
            }
            Expr::Binary(op) if op.op == Op::Or => {
                if let Some(y) = &op.y {
                    self.collect_union_operands(scope, &op.x, items);
                    self.collect_union_operands(scope, y, items);
                }
            }
            Expr::Name(n) => {
                let t = self.lookup_type(scope, &n.value);
                items.push((false, t));
            }
            _ => {
                let t = self.resolve_type(scope, e);
                items.push((false, t));
            }
        }
    }

    /// 一个**普通元素**：本体是接口 ⇒ 嵌入合并；其余 ⇒ 单类型项。
    ///
    /// 四档（官方 `computeInterfaceTypeSet` 的嵌入分派）：
    /// * 本体是接口（穿透别名 / 具名）⇒ 方法逐条合并（按名去重首见）
    ///   + 项面 / `comparable` 位求交；语法面 = **as-written exact 项**；
    /// * 本体**未填**（正在解析的环成员）⇒ 官方 `!ityp.complete` ⇒ `topTypeSet`：
    ///   合并空，**照记语法项**，不 panic、不产生后续错误；
    /// * 无效类型 ⇒ **两面都跳过**（官方 `if !isValid(u) { continue }`）；
    /// * 其余有效的非接口类型 ⇒ 单类型项 `(false, t)`（as-written）。
    fn merge_embedded(
        &mut self,
        t: TypeId,
        methods: &mut Vec<(String, TypeId)>,
        terms: &mut Vec<(bool, TypeId)>,
        tset: &mut IfaceTSet,
    ) {
        match self.drill_iface(t) {
            Drill::Iface(iid) => {
                let (their_methods, their_tset) = {
                    let d = self.tt.interface_def(iid);
                    (d.methods.clone(), d.tset.clone())
                };
                for (name, sig) in their_methods {
                    if !methods.iter().any(|(m, _)| m == &name) {
                        methods.push((name, sig));
                    }
                }
                terms.push((false, t));
                self.merge_tset(tset, their_tset);
            }
            Drill::Unresolved => {
                terms.push((false, t));
            }
            Drill::NotIface => {
                // 穿透后底层无效 —— 两态同一个码面：① **解析中的环成员**占位是
                // `Some(invalid)`（官方 `!ityp.complete` ⇒ `topTypeSet`：合并空）；
                // ② 真·无效名（官方 `if !isValid(u) { continue }`：语义面也不加）。
                // 两种都**只留语法项**（与旧 M3 行为一致；对撞面不含非法码）。
                if matches!(self.tt.type_of(underlying(self.tt, t)), Type::Invalid) {
                    terms.push((false, t));
                    return;
                }
                terms.push((false, t));
                self.merge_tset(tset, IfaceTSet::Terms(vec![(false, t)]));
            }
        }
    }

    /// 沿别名 / 具名下钻分类（带双环卫；环成员报 `Unresolved`，不 panic）。
    fn drill_iface(&self, t: TypeId) -> Drill {
        let mut seen_n: Vec<rgoc_types::NamedId> = Vec::new();
        let mut seen_a: Vec<rgoc_types::AliasId> = Vec::new();
        let mut cur = t;
        loop {
            match self.tt.type_of(cur) {
                Type::Interface(id) => return Drill::Iface(*id),
                Type::Named(n) => {
                    if seen_n.contains(n) {
                        return Drill::Unresolved;
                    }
                    seen_n.push(*n);
                    match self.tt.named_def(*n).underlying {
                        Some(u) => cur = u,
                        None => return Drill::Unresolved,
                    }
                }
                Type::Alias(a) => {
                    if seen_a.contains(a) {
                        return Drill::Unresolved;
                    }
                    seen_a.push(*a);
                    match self.tt.alias_def(*a).rhs {
                        Some(r) => cur = r,
                        None => return Drill::Unresolved,
                    }
                }
                _ => return Drill::NotIface,
            }
        }
    }

    /// 一个 **union 元素**的语义类型集（官方 `computeUnionTypeSet`）。
    ///
    /// 逐操作数：
    /// * 本体是接口（穿透别名 / 具名）⇒ 并入该接口的**项面**
    ///   （方法不并入；`All` / `Comparable` 的项面是 𝓤 ⇒ 整元素 𝓤）；
    /// * 底层无效 ⇒ 跳过；
    /// * `~T` 而 `T` 与 `underlying(T)` 不同一（`~MyInt`）⇒ ∅ 项（跳过）；
    /// * 其余 ⇒ 保项 `(tilde, T)`（**as-written**，与语法面同项）。
    ///
    /// 结果**不归一**（`norm`）—— 消费面是集合语义（见 `typeset` 模块文档）。
    fn union_elem_tset(&self, items: &[(bool, TypeId)]) -> IfaceTSet {
        let mut out: Vec<(bool, TypeId)> = Vec::new();
        let mut any_all = false;
        for (tilde, t) in items {
            if let Drill::Iface(iid) = self.drill_iface(*t) {
                match &self.tt.interface_def(iid).tset {
                    IfaceTSet::All | IfaceTSet::Comparable => any_all = true,
                    IfaceTSet::Empty => {}
                    IfaceTSet::Terms(ts) => out.extend(ts.iter().copied()),
                }
                continue;
            }
            let u = underlying(self.tt, *t);
            if matches!(self.tt.type_of(u), Type::Invalid) {
                continue;
            }
            if *tilde {
                match identical(self.tt, *t, u) {
                    Ok(true) => {}
                    // `~MyInt`：不存在以 MyInt 为底层的类型 ⇒ ∅ 项
                    Ok(false) => continue,
                    // 能力边界：按漂移跳过（保守；Task 10 起可判）
                    Err(_) => continue,
                }
            }
            out.push((*tilde, *t));
        }
        if any_all {
            IfaceTSet::All
        } else if out.is_empty() {
            IfaceTSet::Empty
        } else {
            IfaceTSet::Terms(out)
        }
    }

    /// 项面 / `comparable` 位求交（官方 `intersectTermLists`）。
    ///
    /// `Err`（能力边界）⇒ 降级为 𝓤 并继续（官方对 incomplete 的 `topTypeSet`
    /// 同款宽容；记录在台账 Task 8 行）。
    fn merge_tset(&self, acc: &mut IfaceTSet, elem: IfaceTSet) {
        match typeset::intersect(self.tt, acc, &elem) {
            Ok(v) => *acc = v,
            Err(_) => *acc = IfaceTSet::All,
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
                // 变参：末位参数的 `...T`（parser 已保证只在末位）。
                //
                // ⚠️ 参数类型是 **`[]T` 切片**，不是元素类型 `T` —— 这是 go/types
                // 实测口径（`Signature.Params()` 的末位就是切片），也是 C7 对撞的
                // 判据（`variadic` 位只表示「可这样调用」，不改变参数本身的类型）。
                // 写成 `T` 会让两侧编号整体错位（M3/Task 33 实测）。
                Expr::DotsType(d) => {
                    let elem = self.resolve_type(scope, &d.elem);
                    (self.tt.add_slice(elem), true)
                }
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

    /// 声明类型参数，返回（**类型参数作用域**，**条目表** —— 与形参表同序）。
    ///
    /// 两遍是**必须**的：`[T any, U ~[]T]` 里 `U` 的约束引用了 `T`，
    /// 而 `T` 必须先成为对象；同时也支持 `[T U, U any]` 这种互引（官方同样允许）。
    /// 先占号后回填（`set_type_param_constraint`）是递归约束的唯一手段。
    ///
    /// 条目表是 **M4/Task 12** 加的：`resolve_type_item` 把类型声明的形参表
    /// 挂到 `NamedDef::type_params`、`resolve_func_sig` 把接收者的挂到
    /// `SignatureDef::recv_tparams`（`instantiate` 的两个消费者）。
    pub(crate) fn declare_tparams(
        &mut self,
        parent: ScopeId,
        tparams: &[AstField],
    ) -> (ScopeId, Vec<TypeParamId>) {
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
            // `resolve_type`，随后按「**是不是接口类型**」决定要不要包一层：
            //   * `any`（alias→接口）、`comparable`（named，本体是接口）、
            //     用户写的 `type Num interface{…}` ⇒ 本来就有接口类型，**原样**用；
            //   * `int` / `[]int` / 具名结构体等**裸非接口**类型 ⇒ 官方包成
            //     隐式接口 `interface{ T }`（`interface 0 1` + 一行 `term exact`）。
            //
            // ⚠️ `[T int]` 这条口径**订正过一次**（M3/Task 33）：原先不包，理由是
            // 「oracle 在编号阶段报『类型图成环』而崩，没得对撞」。那个 oracle 崩溃
            // 已在 Task 31 修掉（根因是合成 union 的指针不稳定 + 接口方法下钻
            // 接收者），于是这一条从「照写出的表示」变成**可对撞**，实测官方
            // 就是包成接口 —— 不包会让 `type-dump-generics.go` 从那一行起错位。
            let c = if is_type_set(&f.type_) {
                let mut methods = Vec::new();
                let mut terms = Vec::new();
                let mut tset = IfaceTSet::All;
                self.collect_iface_elem(scope, &f.type_, &mut methods, &mut terms, &mut tset);
                self.build_iface(methods, terms, tset)
            } else {
                let t = self.resolve_type(scope, &f.type_);
                if self.is_interface_type(t) {
                    t
                } else {
                    // 隐式 `interface{ T }`（官方 `[T int]` 的包法）：语义 = 单类型项；
                    // T 无效时语义退化为 𝓤（官方对无效嵌入跳过 ⇒ 全集），语法面照记。
                    let elem_tset = if matches!(self.tt.type_of(t), Type::Invalid) {
                        IfaceTSet::All
                    } else {
                        IfaceTSet::Terms(vec![(false, t)])
                    };
                    self.build_iface(Vec::new(), vec![(false, t)], elem_tset)
                }
            };
            self.tt.set_type_param_constraint(*tp, c);
        }
        (scope, ids.iter().map(|(tp, _)| *tp).collect())
    }

    /// 这个类型**是不是接口类型**（官方 `Underlying().(*Interface)` 的对应物）。
    ///
    /// 别名与具名都要**穿透**：`any` 是 alias（右侧 `interface{}`）、`comparable`
    /// 与用户的 `type Num interface{…}` 是 named（本体是接口）。`type A B`
    /// （B 具名）会让本体又是一层 named ⇒ 逐层走。
    ///
    /// `seen` 是**环卫**：`type A B; type B A` 这类非法程序在报告之前本体确实
    /// 互指（`validType` 的环诊断另有其人），这里不能跟着绕 —— 环不是接口类型。
    fn is_interface_type(&self, t: TypeId) -> bool {
        let mut seen: Vec<rgoc_types::NamedId> = Vec::new();
        let mut cur = unalias(self.tt, t);
        loop {
            match self.tt.type_of(cur) {
                Type::Interface(_) => return true,
                Type::Named(n) => {
                    if seen.contains(n) {
                        return false;
                    }
                    seen.push(*n);
                    cur = self.tt.named_underlying(*n);
                }
                _ => return false,
            }
        }
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

/// `unsafe.Pointer` 这条限定名（`X` 是 `unsafe`、`Sel` 是 `Pointer`）。
///
/// 与 `rgoc-check::unsafe_pkg` 的白名单同名同拼写 —— 两处都改才算改了契约。
fn is_unsafe_pointer(s: &rgoc_ast::SelectorExpr) -> bool {
    matches!(&*s.x, Expr::Name(n) if n.value == "unsafe") && s.sel.value == "Pointer"
}
