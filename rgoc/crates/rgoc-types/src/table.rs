//! 类型表（arena）与各「定义/构造」的附属表（M3/Task 14）。
//!
//! # 为什么是 arena
//!
//! 类型的引用是**图**不是树：`type N struct{ p *N }` 里 `*N` 指回 `N`，
//! `typeof` 里的 `interface{ ... }` 互相引用。用 `Box` 造树会要求先有子后有父，
//! 递归类型就写不出来。arena（`Vec` + 下标）允许**先占号、后填本体** ——
//! 这正是 `C7` §3 规则 2 说的「定义型先占号、再下钻本体」在数据模型上的对应物。
//!
//! # 分表与 `self_ty`
//!
//! 每类构造有自己的附属表（`structs` / `signatures` / …），并且**同时**往
//! `types` 里放一个对应变体的条目。附属表里存一份 `self_ty` 指回那个条目 ——
//! 这样「拿到 `StructId` 想要它的 `TypeId`」是 O(1) 的（[`TypeTable::struct_type`]），
//! 而不是「反向搜一遍 `types`」。
//!
//! ```
//! # use rgoc_types::{TypeTable, ChanDir};
//! let mut t = TypeTable::new();
//! let i = t.add_basic(rgoc_types::BasicId::Int);
//! let p = t.add_pointer(i);
//! let c = t.add_chan(ChanDir::RecvOnly, p);
//! assert_eq!(t.type_of(c).variant_label(), "chan");
//! ```
//!
//! # **不去重**（`C7` §3 的实测结论）
//!
//! `add_*` 一律压新槽。实测 oracle（`go/types`）：`var a []byte` 与 `var b []byte`
//! 是**两个**编号（`1 slice 0` 与 `2 slice 0`），`var e S` 与 `var f S` 指向同一个
//! `named` 只占**一个**号。所以：
//!
//! * 结构型（`pointer`/`slice`/`array`/…/`union`）⇒ **绝不**去重，一次出现一个槽
//! * 定义型（`named`/`alias`）⇒ 由它们自己的 `add_*` 保证「一个定义一个槽」，
//!   引用时用 [`TypeTable::named_type`] / [`TypeTable::alias_type`] 取**同一个**槽
//! * `basic` / `invalid` ⇒ 由调用方复用（[`TypeTable::invalid`] 是单例）；
//!   `basic` 没有去重表，因为调用方手上的就是 `BasicId`（同一个 `BasicId` 想要
//!   同一个 `TypeId` 时该复用哪个由调用方决定 —— 见 `C7` 的 oracle 事实）
//!
//! ⚠️ 最后一条是**有意留白**：`go/types` 的 `*types.Basic` 是单例（所以 dump 里
//! `int` 只占一个号），而我们这一层不替调用方做这个复用。Task 16 的 dump 走访
//! 因此在「同一 `BasicId` 复用同一 `TypeId`」之外还**按 `BasicId` 去重**
//! （`C7` §3 规则 1 明写「仅定义型/基本型去重」）—— 两处判据一致，不冲突。

use rgoc_const::{BasicId, ConstId, ConstTable};
use rgoc_lex::Pos;

use crate::typ::{
    AliasId, ChanDir, InterfaceId, NamedId, SignatureId, StructId, TupleId, Type, TypeId,
    TypeParamId, UnionId,
};
use crate::typeset::IfaceTSet;

/// 结构体字段（`C7` §4.1 的 `struct` 嵌套项一行一个）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Field {
    /// 字段名；`None` ⇒ 嵌入字段（嵌入字段在 dump 里名字写 `-`）。
    pub name: Option<String>,
    /// 字段类型。
    pub typ: TypeId,
    /// 结构体标签原文（含反引号或引号**不含** —— 存的是字面量内容）；
    /// `None` ⇒ 没有标签。
    pub tag: Option<String>,
    /// 是不是嵌入字段。**判据看语义**：嵌入字段即使有名字（`T`）也不是 `name`。
    pub embedded: bool,
}

/// `named` 定义（`type D ...`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NamedDef {
    /// 在本表 `types` 里的条目下标。
    pub self_ty: TypeId,
    /// 类型名（**不进 dump** —— `C7` §2 规则 5；诊断与 `Identical` 要它）。
    pub name: String,
    /// 本体。构造协议：`add_named` 之后**立即** `set_named_underlying`
    /// （递归类型要求先占号，所以没做成必填参数）。
    pub underlying: Option<TypeId>,
    /// 类型名**声明处**的位置（诊断用；`None` = 交给本表的手工构造没给位置）。
    ///
    /// 与官方一致：`go/types` 的 `cycleError` 报在**环路径起点的对象**上，
    /// 而那个对象是类型名声明 —— 所以位置挂在**定义**上，而不是判定入口的参数上
    /// （Task 22 的 `valid_type` 签名因此没有 `Pos` 参数）。
    pub pos: Option<Pos>,
    /// 挂在**接收者基类型**上的方法：`(方法名, 签名)`，按**声明序**追加
    /// （跨文件按文件的处理序；不排序 —— 排序是消费者的事）。
    ///
    /// * 与 [`InterfaceDef::methods`] 同形；那份在构造期排序（`C7` §4 的
    ///   `interface` 行规约），这份**不**排 —— 方法集（`methodset`）与选择器
    ///   （`rgoc-check`）各自决定顺序。
    /// * 只收**本包定义型**的接收者（官方：接收者基类型必须是本包定义、非指针、
    ///   非接口）；接收者的**指针性**不在这里存 —— 判据是签名 `recv` 是不是
    ///   指针类型（官方 `hasPtrRecv` 同口径），一个数据一份真相。
    /// * `C7` 的 dump **不渲染**它（named 行的渲染面与 M3 冻结版一致）；
    ///   消费者是 M4 的方法集 / 接口实现 / 选择器。
    /// * 生产者：`rgoc-load` 的 `Decl::Func` 臂（Task 7 的唯一生产者）。
    pub methods: Vec<(String, TypeId)>,
}

/// `alias` 定义（`type D = T`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AliasDef {
    /// 在本表 `types` 里的条目下标。
    pub self_ty: TypeId,
    /// 别名（不进 dump）。
    pub name: String,
    /// 右侧类型。
    pub rhs: Option<TypeId>,
}

/// `struct` 条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StructDef {
    /// 在本表 `types` 里的条目下标。
    pub self_ty: TypeId,
    /// 字段（**声明序** —— `C7` §3 的子节点顺序）。
    pub fields: Vec<Field>,
}

/// `tuple` 条目（参数 / 结果）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TupleDef {
    /// 在本表 `types` 里的条目下标。
    pub self_ty: TypeId,
    /// 元素：`(名字, 类型)`，名字 `None` ⇒ 无名参数（dump 写 `-`）。
    pub vars: Vec<(Option<String>, TypeId)>,
}

/// `signature` 条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SignatureDef {
    /// 在本表 `types` 里的条目下标。
    pub self_ty: TypeId,
    /// 接收者类型（`None` ⇒ 不是方法）。
    pub recv: Option<TypeId>,
    /// 参数元组。
    pub params: TypeId,
    /// 结果元组。
    pub results: TypeId,
    /// 是不是变参（`...T`）。
    pub variadic: bool,
}

/// `interface` 条目。
///
/// # 两面：`terms` 是**语法**，`tset` 是**语义**（M4/Task 8）
///
/// * [`terms`](Self::terms)：接口字面量里 `~T` / `A | B` / 嵌入元素**按官方
///   dump 口径拍平**的项（单元素是 union 才 splice，否则记 as-written 的
///   exact 项）—— **渲染**（`C7`）与 oracle 对撞吃这一面。
/// * [`tset`](Self::tset)：官方 `_TypeSet` 的语义投影（交集 / `comparable`
///   过滤 / ∅ 可达）—— **判定**（`implements` / 同一性）吃这一面。
///
/// 两面**不必同形**：`interface{ comparable; ~[]byte }` 的语法面是两条项、
/// 语义面是 ∅。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InterfaceDef {
    /// 在本表 `types` 里的条目下标。
    pub self_ty: TypeId,
    /// 方法：`(方法名, 签名)`，**按名字升序**存（`C7` §4 的 `interface` 行规约）。
    pub methods: Vec<(String, TypeId)>,
    /// **语法**项集（`~T | U` 那个 `union` 的条目）；`None` ⇒ 无项线。
    pub terms: Option<TypeId>,
    /// **语义**类型集（官方 `_TypeSet` 的项面 + `comparable` 位，见
    /// [`crate::typeset::IfaceTSet`]）。
    ///
    /// 预声明的 `comparable` 是 `Comparable`；`interface{}` 是 `All` ——
    /// 两者的**表示**差别只在这里。
    /// ⚠️ 它在 `C7` 的 dump 里**不可见**（两者都写 `interface 0 0`）——
    /// 因为 `comparable` 不能作变量类型，dump 的根永远到不了它。
    /// 但同一性（`D-M3-13`）与实现判定（`implements`）要消费它。
    pub tset: IfaceTSet,
}

/// `typeparam` 条目（泛型类型参数）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TypeParamDef {
    /// 在本表 `types` 里的条目下标。
    pub self_ty: TypeId,
    /// 序号（在参数表里的位置）。
    pub index: u32,
    /// 约束。
    pub constraint: TypeId,
}

/// `union` 条目（`~T | U`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnionDef {
    /// 在本表 `types` 里的条目下标。
    pub self_ty: TypeId,
    /// 项：`(~, 类型)`；`true` ⇒ `~T`，`false` ⇒ 精确项。
    pub terms: Vec<(bool, TypeId)>,
}

/// 类型表（arena）—— 所有类型条目的唯一所有者。
///
/// 常量表也挂在这里（`array` 的长度是常量），因为「类型的常量」本该与类型同寿命：
/// 分家会让 `ConstId` 存在两张表之间的对应关系需要额外维护。
#[derive(Default, Clone, Debug)]
pub struct TypeTable {
    types: Vec<Type>,
    consts: ConstTable,
    named: Vec<NamedDef>,
    aliases: Vec<AliasDef>,
    structs: Vec<StructDef>,
    tuples: Vec<TupleDef>,
    signatures: Vec<SignatureDef>,
    interfaces: Vec<InterfaceDef>,
    type_params: Vec<TypeParamDef>,
    unions: Vec<UnionDef>,
    /// `invalid` 的单例槽（懒创建）。实测 oracle 把 `invalid` 当基本型去重。
    invalid: Option<TypeId>,
}

impl TypeTable {
    /// 空表。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    // ── 读 ───────────────────────────────────────────────────────────

    /// 取类型条目。
    ///
    /// # Panics
    ///
    /// 下标越界时 panic（`TypeId` 只由本表产出 ⇒ 越界是内部缺陷）。
    #[must_use]
    pub fn type_of(&self, id: TypeId) -> &Type {
        &self.types[id.index()]
    }

    /// 表里有多少类型条目。
    #[must_use]
    pub fn len(&self) -> usize {
        self.types.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    /// 常量表（`array` 长度的老家）。
    #[must_use]
    pub fn consts(&self) -> &ConstTable {
        &self.consts
    }

    /// 常量表（可变）—— 往 `array` 长度里放值要走这里。
    pub fn consts_mut(&mut self) -> &mut ConstTable {
        &mut self.consts
    }

    /// `named` 定义。
    #[must_use]
    pub fn named_def(&self, id: NamedId) -> &NamedDef {
        &self.named[id.index()]
    }

    /// `named` 的本体。构造协议保证已填（`add_named` 后立即 set）。
    ///
    /// # Panics
    ///
    /// 本体未填时 panic —— 那是**内部缺陷**（半构造的表不该被读）。
    #[must_use]
    pub fn named_underlying(&self, id: NamedId) -> TypeId {
        self.named_def(id)
            .underlying
            .expect("named 本体未填：构造协议是 add_named 之后立即 set_named_underlying")
    }

    /// `alias` 定义。
    #[must_use]
    pub fn alias_def(&self, id: AliasId) -> &AliasDef {
        &self.aliases[id.index()]
    }

    /// `struct` 定义。
    #[must_use]
    pub fn struct_def(&self, id: StructId) -> &StructDef {
        &self.structs[id.index()]
    }

    /// `tuple` 定义。
    #[must_use]
    pub fn tuple_def(&self, id: TupleId) -> &TupleDef {
        &self.tuples[id.index()]
    }

    /// `signature` 定义。
    #[must_use]
    pub fn signature_def(&self, id: SignatureId) -> &SignatureDef {
        &self.signatures[id.index()]
    }

    /// `interface` 定义。
    #[must_use]
    pub fn interface_def(&self, id: InterfaceId) -> &InterfaceDef {
        &self.interfaces[id.index()]
    }

    /// `typeparam` 定义。
    #[must_use]
    pub fn type_param_def(&self, id: TypeParamId) -> &TypeParamDef {
        &self.type_params[id.index()]
    }

    /// `union` 定义。
    #[must_use]
    pub fn union_def(&self, id: UnionId) -> &UnionDef {
        &self.unions[id.index()]
    }

    // ── 基本型 / invalid ─────────────────────────────────────────────

    /// 压入一个基本类型条目。
    ///
    /// **不去重**：同一个 `BasicId` 压两次是两个槽。想要复用就自己留一份
    /// `TypeId`（dump 侧另按 `BasicId` 去重，见模块文档）。
    pub fn add_basic(&mut self, b: BasicId) -> TypeId {
        self.push(Type::Basic(b))
    }

    /// `invalid` 的单例条目。
    pub fn invalid(&mut self) -> TypeId {
        match self.invalid {
            Some(id) => id,
            None => {
                let id = self.push(Type::Invalid);
                self.invalid = Some(id);
                id
            }
        }
    }

    // ── 定义型 ───────────────────────────────────────────────────────

    /// 新建一个 `named` 定义（**只有名字**；本体随后用
    /// [`TypeTable::set_named_underlying`] 填）。
    pub fn add_named(&mut self, name: impl Into<String>) -> NamedId {
        let self_ty = self.push(Type::Named(NamedId::from_index(self.named.len())));
        let id = NamedId::from_index(self.named.len());
        self.named.push(NamedDef {
            self_ty,
            name: name.into(),
            underlying: None,
            pos: None,
            methods: Vec::new(),
        });
        id
    }

    /// 填 `named` 的本体。
    pub fn set_named_underlying(&mut self, id: NamedId, t: TypeId) {
        self.named[id.index()].underlying = Some(t);
    }

    /// 填 `named` 的**声明位置**（诊断用；`M1` 的 `Pos`）。
    ///
    /// 官方 `types.TypeName` 悬挂在作用域对象上，位置随对象走；本表不持有作用域，
    /// 所以把位置挂在**定义**上 —— 见 [`NamedDef::pos`]。
    pub fn set_named_pos(&mut self, id: NamedId, pos: Pos) {
        self.named[id.index()].pos = Some(pos);
    }

    /// 给 `named` 挂一个方法（`(方法名, 签名类型)`）。
    ///
    /// 追加式（声明序）；同名重复**不**在这里裁决（那属于检查器的诊断面），
    /// 方法集对同名的处理与官方 `methodSet.addOne` 一致（碰撞 ⇒ 排除）。
    /// 接收者的指针性由调用方保证体现在签名的 `recv` 里（见 [`NamedDef::methods`]）。
    pub fn add_named_method(&mut self, id: NamedId, name: impl Into<String>, sig: TypeId) {
        self.named[id.index()].methods.push((name.into(), sig));
    }

    /// `named` 定义 → 它在 `types` 里的条目（**同一个定义恒等** ⇒ 引用要复用它）。
    #[must_use]
    pub fn named_type(&self, id: NamedId) -> TypeId {
        self.named_def(id).self_ty
    }

    /// 新建一个 `alias` 定义（`type D = T`），右侧用
    /// [`TypeTable::set_alias_rhs`] 填 —— 别名同样可以参与递归，所以先占号。
    pub fn add_alias(&mut self, name: impl Into<String>) -> AliasId {
        let self_ty = self.push(Type::Alias(AliasId::from_index(self.aliases.len())));
        let id = AliasId::from_index(self.aliases.len());
        self.aliases.push(AliasDef {
            self_ty,
            name: name.into(),
            rhs: None,
        });
        id
    }

    /// 填 `alias` 的右侧。
    pub fn set_alias_rhs(&mut self, id: AliasId, t: TypeId) {
        self.aliases[id.index()].rhs = Some(t);
    }

    /// `alias` 的右侧（构造协议保证已填）。
    ///
    /// # Panics
    ///
    /// 未填时 panic（内部缺陷）。
    #[must_use]
    pub fn alias_rhs(&self, id: AliasId) -> TypeId {
        self.alias_def(id)
            .rhs
            .expect("alias 右侧未填：构造协议是 add_alias 之后立即 set_alias_rhs")
    }

    /// `alias` 定义 → 它在 `types` 里的条目。
    #[must_use]
    pub fn alias_type(&self, id: AliasId) -> TypeId {
        self.alias_def(id).self_ty
    }

    // ── 结构型 ───────────────────────────────────────────────────────

    /// `*T`。
    pub fn add_pointer(&mut self, elem: TypeId) -> TypeId {
        self.push(Type::Pointer(elem))
    }

    /// `[]T`。
    pub fn add_slice(&mut self, elem: TypeId) -> TypeId {
        self.push(Type::Slice(elem))
    }

    /// `[n]T` / `[...]T`（`None` ⇒ 长度未定）。
    pub fn add_array(&mut self, len: Option<ConstId>, elem: TypeId) -> TypeId {
        self.push(Type::Array { len, elem })
    }

    /// `map[K]V`。
    pub fn add_map(&mut self, key: TypeId, elem: TypeId) -> TypeId {
        self.push(Type::Map { key, elem })
    }

    /// `chan T`（含方向）。
    pub fn add_chan(&mut self, dir: ChanDir, elem: TypeId) -> TypeId {
        self.push(Type::Chan { dir, elem })
    }

    /// `struct{...}`。
    pub fn add_struct(&mut self, fields: Vec<Field>) -> StructId {
        let id = StructId::from_index(self.structs.len());
        let self_ty = self.push(Type::Struct(id));
        self.structs.push(StructDef { self_ty, fields });
        id
    }

    /// `struct` 定义 → 它在 `types` 里的条目。
    #[must_use]
    pub fn struct_type(&self, id: StructId) -> TypeId {
        self.struct_def(id).self_ty
    }

    /// `(a, b)` 参数/结果元组。
    pub fn add_tuple(&mut self, vars: Vec<(Option<String>, TypeId)>) -> TupleId {
        let id = TupleId::from_index(self.tuples.len());
        let self_ty = self.push(Type::Tuple(id));
        self.tuples.push(TupleDef { self_ty, vars });
        id
    }

    /// `tuple` 定义 → 它在 `types` 里的条目。
    #[must_use]
    pub fn tuple_type(&self, id: TupleId) -> TypeId {
        self.tuple_def(id).self_ty
    }

    /// 函数/方法签名。
    ///
    /// 参数与结果**按 `TupleId` 收**（签名的那两个位置在 Go 里就是元组，
    /// 类型上收紧能让「传了个 `[]int` 当参数表」当场编译不过）；表内转成
    /// `TypeId` 存，渲染时直接就能引用。
    pub fn add_signature(
        &mut self,
        recv: Option<TypeId>,
        params: TupleId,
        results: TupleId,
        variadic: bool,
    ) -> SignatureId {
        let id = SignatureId::from_index(self.signatures.len());
        let self_ty = self.push(Type::Signature(id));
        self.signatures.push(SignatureDef {
            self_ty,
            recv,
            params: self.tuple_type(params),
            results: self.tuple_type(results),
            variadic,
        });
        id
    }

    /// `signature` 定义 → 它在 `types` 里的条目。
    #[must_use]
    pub fn signature_type(&self, id: SignatureId) -> TypeId {
        self.signature_def(id).self_ty
    }

    /// 接口。
    ///
    /// `methods` **不在这里排序** —— 调用方给什么顺序就存什么顺序（表是数据，
    /// 排序是渲染规约；见 `C7` §4 的 `interface` 行）。这样
    /// 「表的顺序」与「dump 的顺序」在需要时能分开断言。
    ///
    /// `tset` 是**语义**类型集（`IfaceTSet::Comparable` ⇒ 预声明的 `comparable`；
    /// `All` ⇒ 普通无项接口）；`terms` 是**语法**项集（渲染面），两者见
    /// [`InterfaceDef`] 的文档。
    pub fn add_interface(
        &mut self,
        methods: Vec<(String, TypeId)>,
        terms: Option<TypeId>,
        tset: IfaceTSet,
    ) -> InterfaceId {
        let id = InterfaceId::from_index(self.interfaces.len());
        let self_ty = self.push(Type::Interface(id));
        self.interfaces.push(InterfaceDef {
            self_ty,
            methods,
            terms,
            tset,
        });
        id
    }

    /// `interface` 定义 → 它在 `types` 里的条目。
    #[must_use]
    pub fn interface_type(&self, id: InterfaceId) -> TypeId {
        self.interface_def(id).self_ty
    }

    /// 类型参数。
    pub fn add_type_param(&mut self, index: u32, constraint: TypeId) -> TypeParamId {
        let id = TypeParamId::from_index(self.type_params.len());
        let self_ty = self.push(Type::TypeParam(id));
        self.type_params.push(TypeParamDef {
            self_ty,
            index,
            constraint,
        });
        id
    }

    /// `typeparam` 定义 → 它在 `types` 里的条目。
    #[must_use]
    pub fn type_param_type(&self, id: TypeParamId) -> TypeId {
        self.type_param_def(id).self_ty
    }

    /// 回填类型参数的**约束**（`Task 24` Ruling 24-7）。
    ///
    /// 为什么不是构造参数：约束可以引用**后声明**的类型参数
    /// （`[T any, U ~[]T]`），而 `TypeParam` 必须先占号才能被引用 ⇒
    /// 与 `set_named_underlying` / `set_alias_rhs` 同一形状（先占号、后回填）。
    pub fn set_type_param_constraint(&mut self, id: TypeParamId, constraint: TypeId) {
        self.type_params[id.index()].constraint = constraint;
    }

    /// 并集（`~T | U`）。
    pub fn add_union(&mut self, terms: Vec<(bool, TypeId)>) -> UnionId {
        let id = UnionId::from_index(self.unions.len());
        let self_ty = self.push(Type::Union(id));
        self.unions.push(UnionDef { self_ty, terms });
        id
    }

    /// `union` 定义 → 它在 `types` 里的条目。
    #[must_use]
    pub fn union_type(&self, id: UnionId) -> TypeId {
        self.union_def(id).self_ty
    }

    /// 压入一个类型条目（唯一写入口）。
    fn push(&mut self, t: Type) -> TypeId {
        let id = TypeId::from_index(self.types.len());
        self.types.push(t);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgoc_const::{BigInt, Const};

    #[test]
    fn 空表() {
        let t = TypeTable::new();
        assert!(t.is_empty());
        assert_eq!(t.len(), 0);
        assert!(t.consts().is_empty());
    }

    #[test]
    fn 结构型不去重_一次出现一个槽() {
        let mut t = TypeTable::new();
        let byte = t.add_basic(BasicId::Uint8);
        // 实测 oracle：`var a []byte` 与 `var b []byte` 是两个编号
        let a = t.add_slice(byte);
        let b = t.add_slice(byte);
        assert_ne!(a, b);
        assert_eq!(t.type_of(a), t.type_of(b));
        assert_eq!(t.type_of(a), &Type::Slice(byte));
        // 指针同理
        let c = t.add_pointer(byte);
        let d = t.add_pointer(byte);
        assert_ne!(c, d);
        assert_eq!(t.type_of(c), t.type_of(d));
    }

    #[test]
    fn 定义型复用同一个槽() {
        let mut t = TypeTable::new();
        let int = t.add_basic(BasicId::Int);
        let s = t.add_named("S");
        t.set_named_underlying(s, int);
        // 引用两次拿到**同一个** TypeId（oracle：`var e S` / `var f S` 只占一个号）
        assert_eq!(t.named_type(s), t.named_type(s));
        assert_eq!(t.named_underlying(s), int);
        assert_eq!(t.named_def(s).name, "S");
        assert_eq!(t.type_of(t.named_type(s)), &Type::Named(s));
        // 别名同样：先占号，再填右侧
        let a = t.add_alias("A");
        assert_eq!(t.alias_def(a).rhs, None, "先占号 ⇒ 右侧暂时是 None");
        t.set_alias_rhs(a, int);
        assert_eq!(t.alias_rhs(a), int);
        assert_eq!(t.alias_type(a), t.alias_def(a).self_ty);
    }

    #[test]
    fn 递归类型可表示() {
        // `type N struct{ p *N }`：指针的 elem 是 N，而 N 的本体是包含该指针的 struct
        let mut t = TypeTable::new();
        let n = t.add_named("N");
        let ptr = t.add_pointer(t.named_type(n));
        let st = t.add_struct(vec![Field {
            name: Some("p".to_string()),
            typ: ptr,
            tag: None,
            embedded: false,
        }]);
        t.set_named_underlying(n, t.struct_type(st));
        // 走访**终止**且形状正确
        let under = t.named_underlying(n);
        assert_eq!(t.type_of(under).variant_label(), "struct");
        let Type::Struct(sid) = t.type_of(under) else {
            panic!("本体应当是 struct")
        };
        let Type::Pointer(elem) = t.type_of(t.struct_def(*sid).fields[0].typ) else {
            panic!("字段应当是 pointer")
        };
        assert_eq!(t.type_of(*elem), &Type::Named(n), "指回 N 自己");
    }

    #[test]
    fn invalid_是单例() {
        let mut t = TypeTable::new();
        let a = t.invalid();
        let b = t.invalid();
        assert_eq!(a, b, "实测 oracle：invalid 当基本型去重");
        assert_eq!(t.len(), 1);
        assert_eq!(t.type_of(a), &Type::Invalid);
        assert_eq!(t.type_of(a).variant_label(), "invalid");
    }

    #[test]
    fn 数组长度是_constid_且能回查() {
        let mut t = TypeTable::new();
        let n = t.consts_mut().add(Const::Int(BigInt::from_i64(3)));
        let int = t.add_basic(BasicId::Int);
        let arr = t.add_array(Some(n), int);
        let Type::Array { len, elem } = t.type_of(arr) else {
            panic!("应当是 array")
        };
        assert_eq!(*elem, int);
        assert_eq!(
            t.consts().get(len.expect("有长度")),
            &Const::Int(BigInt::from_i64(3))
        );
        // `[...]T` ⇒ 长度未定
        let sl = t.add_array(None, int);
        let Type::Array { len: none, .. } = t.type_of(sl) else {
            panic!("应当是 array")
        };
        assert!(none.is_none());
    }

    #[test]
    fn 常量表与类型表同寿命() {
        let mut t = TypeTable::new();
        let c = t.consts_mut().add(Const::Bool(true));
        let s = t.consts_mut().add(Const::Str(b"a".to_vec()));
        assert_eq!(t.consts().len(), 2);
        assert_eq!(t.consts().get(c), &Const::Bool(true));
        assert_eq!(t.consts().get(s), &Const::Str(b"a".to_vec()));
        // 类型表自己不受常量表影响
        assert!(t.is_empty());
    }

    #[test]
    fn 各构造的标签与_self_ty_闭合() {
        let mut t = TypeTable::new();
        let int = t.add_basic(BasicId::Int);
        let str_ty = t.add_basic(BasicId::String);
        let params = t.add_tuple(vec![(Some("a".to_string()), int)]);
        let results = t.add_tuple(vec![]);
        let sig = t.add_signature(None, params, results, false);
        let sig_ty = t.signature_type(sig);
        let iface = t.add_interface(vec![("M".to_string(), sig_ty)], None, IfaceTSet::All);
        let un = t.add_union(vec![(true, int), (false, str_ty)]);
        let un_ty = t.union_type(un);
        let tp = t.add_type_param(0, un_ty);
        // 每个 `*_type` 取到的条目，标签与附属表对得上
        let empty_struct = t.add_struct(vec![]);
        let empty_struct_ty = t.struct_type(empty_struct);
        assert_eq!(t.type_of(empty_struct_ty).variant_label(), "struct");
        assert_eq!(t.type_of(t.tuple_type(params)).variant_label(), "tuple");
        assert_eq!(t.type_of(sig_ty).variant_label(), "signature");
        assert_eq!(
            t.type_of(t.interface_type(iface)).variant_label(),
            "interface"
        );
        assert_eq!(t.type_of(un_ty).variant_label(), "union");
        assert_eq!(
            t.type_of(t.type_param_type(tp)).variant_label(),
            "typeparam"
        );
        assert_eq!(t.tuple_def(params).vars.len(), 1);
        assert!(!t.signature_def(sig).variadic);
        assert_eq!(t.interface_def(iface).methods[0].0, "M");
        assert!(t.interface_def(iface).terms.is_none());
        assert_eq!(t.union_def(un).terms, vec![(true, int), (false, str_ty)]);
        assert_eq!(t.type_param_def(tp).index, 0);
        assert_eq!(t.type_param_def(tp).constraint, un_ty);
    }
}
