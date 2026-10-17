//! 作用域（M3/Task 15 建基元；**Task 23 改为竞技场**）。
//!
//! 五层嵌套：`universe ⊃ package ⊃ file ⊃ function ⊃ block`（`SCP-01`）。
//! 本文件只做**基元**（建层、插名字、沿链查名字），层次语义（哪些声明进哪一层、
//! 包级 vs 块级重名规则）归检查器（Task 24+）。
//!
//! # 为什么是**竞技场**（`Task 23` Ruling 23-1）
//!
//! Task 15 的第一版是**借用式**（`parent: Option<&'a Scope<'a>>`），理由是
//! 「检查器沿 AST 下行时天然父比子活得久」。那个理由对**递归下行**成立，但
//! Task 23 的装载器必须**返回**一个含作用域树的 `Package` —— 而借用式树
//! **不能从函数里返回**（那是自引用）。于是只有三条路：
//!
//! | 方案 | 为什么没选 |
//! |---|---|
//! | `Rc<RefCell<Scope>>` | 把「结构错」推迟成运行期 `borrow_mut` panic；本仓不要这种未知 |
//! | 每个 `rgoc-load` 自带一套作用域 | 同一条语义两份实现 —— pre-flight 5 正是要避免它 |
//! | **`ScopeId` + `ScopeTable`** | 与同 crate 的 [`crate::table::TypeTable`] **同形**；计划 Task 23 的接口本来就是 `ScopeId` |
//!
//! 竞技场把「父」从**引用**换成**下标**，于是：
//! 树可以被拥有、被返回、被 `Vec` 稳定寻址，而「不能有环」由
//! [`ScopeTable::child`] 只能指向**已存在**的父这一事实保证（新建的层不可能被更早的层指到）。
//!
//! # 为什么对象**按值**存在层里（没有 `ObjectTable`）
//!
//! 计划 Task 23 写的是 `insert(scope: ScopeId, obj: ObjectId)`。M3 **没有**任何
//! 消费者需要「对象下标」：查名字得到的是 `&Object`，装载器要填的字段直接
//! `get_mut` 就够。多一个竞技场只为「计划这么写」是净负债 ⇒ 按值存
//! （`BTreeMap<String, Object>`），见 Ruling 23-1 的后半句。
//!
//! # 为什么是 `BTreeMap`
//!
//! 名字 → 对象要**确定性遍历**（诊断顺序、`open_gaps` 的报告、测试断言都靠它），
//! `HashMap` 的迭代序不稳定。作用域里的名字是几十个量级，BTree 的对数因子不是问题。

use std::collections::BTreeMap;
use std::ops::Deref;

use crate::object::Object;

/// 作用域的**层**（`SCP-01` 的五层）。
///
/// 它与「第几层」不是一回事：`depth()` 说的是**结构**（沿父链数几跳），
/// 本枚举说的是**语义**（这一层是包还是块）——诊断措辞与重名规则都看它。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ScopeKind {
    /// universe（预声明对象）。
    Universe,
    /// 包级（顶层声明）。
    Package,
    /// 文件级（`import` 名只在本层可见）。
    File,
    /// 函数级（参数、结果、具名接收者）。
    Func,
    /// 块级（`{}`、`if`/`for`/`switch` 的隐式块）。
    Block,
}

impl ScopeKind {
    /// 诊断文本里用的拼写。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ScopeKind::Universe => "universe",
            ScopeKind::Package => "package",
            ScopeKind::File => "file",
            ScopeKind::Func => "function",
            ScopeKind::Block => "block",
        }
    }
}

/// 竞技场下标。
///
/// 与 [`crate::typ::TypeId`] 同形：`u32` 带一个不变量（「只能由本表产出」）
/// 就是 newtype 的全部职责。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ScopeId(u32);

impl ScopeId {
    /// 从下标构造 —— 给 `ScopeTable` 与测试用。
    ///
    /// # Panics
    ///
    /// 下标超出 `u32` 时 panic（下标来自 `Vec::len`，不会真的超）。
    #[must_use]
    pub fn from_index(i: usize) -> Self {
        Self(u32::try_from(i).expect("作用域表下标超出 u32"))
    }

    /// 取下标。
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// 一层作用域（**节点**）。
///
/// 节点只做**本层**的事（插名字、查本层）。**沿父链**的查询在
/// [`ScopeRef`] / [`ScopeTable`] 上 —— 因为那条链要走表，节点自己没有表。
#[derive(Debug, Clone)]
pub struct Scope {
    /// 这一层是什么。
    kind: ScopeKind,
    /// 外层；最外层（universe）是 `None`。
    parent: Option<ScopeId>,
    /// 本层定义的名字 → 对象。
    objects: BTreeMap<String, Object>,
}

impl Scope {
    /// 这一层是什么。
    #[must_use]
    pub fn kind(&self) -> ScopeKind {
        self.kind
    }

    /// 外层（universe ⇒ `None`）。
    #[must_use]
    pub fn parent(&self) -> Option<ScopeId> {
        self.parent
    }

    /// 本层插入一个对象；返回**被同名顶替掉的那个**（有就是同层重复声明）。
    ///
    /// 「返回而不是报错」是刻意的：报不报、报什么代号，取决于层（包级与块级
    /// 规则不同 —— `T-TY-55`）。本层只负责**如实报告顶替发生了**。
    pub fn insert(&mut self, obj: Object) -> Option<Object> {
        self.objects.insert(obj.name.clone(), obj)
    }

    /// 只在**本层**找。
    #[must_use]
    pub fn lookup_local(&self, name: &str) -> Option<&Object> {
        self.objects.get(name)
    }

    /// 本层的名字（**字典序**）。
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.objects.keys().map(String::as_str)
    }

    /// 本层有多少个名字。
    #[must_use]
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// 本层是不是空的。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

/// 作用域表（竞技场）。
///
/// 只增层、只作子层（[`child`](ScopeTable::child) 指向**已存在**的父）——
/// 环在结构上不可能。
///
/// `Clone` 的触发者：`rgoc-load` 的**源码 import**（Task 27）—— 一个被依赖包可能被
/// 同一批文件 import 多次，而 `rgoc-load` 的 `Package` 是**按值**持有作用域树的，
/// 于是「第二次 import」只能克隆（装载会话里缓存了首次结果）。M3 的 fixture 很小，
/// 克隆代价可忽略；等 M4 需要按引用共享时再换形态。
#[derive(Debug, Default, Clone)]
pub struct ScopeTable {
    scopes: Vec<Scope>,
}

impl ScopeTable {
    /// 建一张空表。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 建一层**没有父**的根（正常只有 universe 是根）。
    pub fn root(&mut self, kind: ScopeKind) -> ScopeId {
        self.push(Scope {
            kind,
            parent: None,
            objects: BTreeMap::new(),
        })
    }

    /// 建一层子作用域。
    ///
    /// # Panics
    ///
    /// `parent` 不属于本表时 panic（与 [`ScopeTable::get`] 同一个判据：
    /// 混用两表的 `ScopeId` 是调用方的缺陷，静默兜底只会让错误藏起来）。
    pub fn child(&mut self, kind: ScopeKind, parent: ScopeId) -> ScopeId {
        let _ = self.get(parent); // 越界即 panic
        self.push(Scope {
            kind,
            parent: Some(parent),
            objects: BTreeMap::new(),
        })
    }

    fn push(&mut self, s: Scope) -> ScopeId {
        let id = ScopeId::from_index(self.scopes.len());
        self.scopes.push(s);
        id
    }

    /// 表里有几层。
    #[must_use]
    pub fn len(&self) -> usize {
        self.scopes.len()
    }

    /// 表里一层都没有。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }

    /// 层节点本身（**只读**；要沿链查名字用 [`ScopeTable::scope`]）。
    ///
    /// # Panics
    ///
    /// `id` 不属于本表时 panic（理由同 [`ScopeTable::child`]）。
    #[must_use]
    pub fn get(&self, id: ScopeId) -> &Scope {
        match self.scopes.get(id.index()) {
            Some(s) => s,
            None => panic!("ScopeId({}) 不属于本表", id.index()),
        }
    }

    /// 层节点本身（**可写**）—— 给装载器/检查器填字段用。
    ///
    /// # Panics
    ///
    /// 同 [`ScopeTable::get`]。
    #[must_use]
    pub fn get_mut(&mut self, id: ScopeId) -> &mut Scope {
        match self.scopes.get_mut(id.index()) {
            Some(s) => s,
            None => panic!("ScopeId({}) 不属于本表", id.index()),
        }
    }

    /// 本层插一个对象；返回被顶替者（见 [`Scope::insert`]）。
    ///
    /// # Panics
    ///
    /// 同 [`ScopeTable::get`]。
    pub fn insert(&mut self, id: ScopeId, obj: Object) -> Option<Object> {
        self.get_mut(id).insert(obj)
    }

    /// 只读句柄 —— 沿父链查名字的**唯一入口**。
    ///
    /// # Panics
    ///
    /// 同 [`ScopeTable::get`]。
    #[must_use]
    pub fn scope(&self, id: ScopeId) -> ScopeRef<'_> {
        let _ = self.get(id);
        ScopeRef { table: self, id }
    }

    /// 沿**父链**找（含本层）。
    #[must_use]
    pub fn lookup(&self, id: ScopeId, name: &str) -> Option<&Object> {
        let mut cur = Some(id);
        while let Some(i) = cur {
            let s = self.get(i);
            if let Some(o) = s.lookup_local(name) {
                return Some(o);
            }
            cur = s.parent();
        }
        None
    }

    /// `name` 在**哪一层**被找到（本层 0、父 1……；`None` ⇒ 没找到）。
    #[must_use]
    pub fn lookup_level(&self, id: ScopeId, name: &str) -> Option<usize> {
        let mut cur = Some(id);
        let mut level = 0usize;
        while let Some(i) = cur {
            let s = self.get(i);
            if s.lookup_local(name).is_some() {
                return Some(level);
            }
            level += 1;
            cur = s.parent();
        }
        None
    }

    /// 到最外层一共有几层（本层算 1）。
    #[must_use]
    pub fn depth(&self, id: ScopeId) -> usize {
        let mut d = 1;
        let mut cur = self.get(id).parent();
        while let Some(i) = cur {
            d += 1;
            cur = self.get(i).parent();
        }
        d
    }
}

/// 一层作用域的**只读句柄**：节点 + 它所在的那张表。
///
/// 读操作按「贴哪张表」分工：
///
/// - [`Deref`] 到 [`Scope`] 的**本层**操作（`lookup_local` / `names` / `len` / `kind` / `parent`）；
/// - 句柄自己的**沿链**操作（[`lookup`](ScopeRef::lookup) / [`lookup_level`](ScopeRef::lookup_level) / [`depth`](ScopeRef::depth)）。
///
/// ⚠️ 它**不能**改（`insert` 要 `&mut`，句柄只有 `&Scope`）——「查询不改状态」
/// 在类型上成立，而不是靠约定。
#[derive(Debug)]
pub struct ScopeRef<'a> {
    table: &'a ScopeTable,
    id: ScopeId,
}

impl ScopeRef<'_> {
    /// 这层的下标。
    #[must_use]
    pub fn id(&self) -> ScopeId {
        self.id
    }

    /// 沿**父链**找（含本层）。
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&Object> {
        self.table.lookup(self.id, name)
    }

    /// `name` 是在**哪一层**被找到的（本层 0、父 1……；`None` ⇒ 没找到）。
    ///
    /// 「在哪一层」有用：诊断要说「与包级声明的 `x` 冲突」还是「本块内重复」。
    #[must_use]
    pub fn lookup_level(&self, name: &str) -> Option<usize> {
        self.table.lookup_level(self.id, name)
    }

    /// 到最外层一共有几层（本层算 1）。
    #[must_use]
    pub fn depth(&self) -> usize {
        self.table.depth(self.id)
    }
}

impl Deref for ScopeRef<'_> {
    type Target = Scope;

    fn deref(&self) -> &Scope {
        self.table.get(self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::ObjKind;

    #[test]
    fn 建层与深度() {
        let mut st = ScopeTable::new();
        let root = st.root(ScopeKind::Universe);
        assert_eq!(st.depth(root), 1);
        assert!(st.get(root).is_empty());
        let file = st.child(ScopeKind::File, root);
        assert_eq!(st.depth(file), 2);
        let block = st.child(ScopeKind::Block, file);
        assert_eq!(st.depth(block), 3, "universe ⊃ file ⊃ block");
        assert_eq!(st.get(block).kind(), ScopeKind::Block);
        assert_eq!(st.get(block).parent(), Some(file));
        assert_eq!(st.get(root).parent(), None, "universe 没有父");
    }

    #[test]
    fn 插入与沿链查名字() {
        let mut st = ScopeTable::new();
        let root = st.root(ScopeKind::Universe);
        st.insert(root, Object::new(ObjKind::TypeName, "int"));
        let file = st.child(ScopeKind::File, root);
        st.insert(file, Object::new(ObjKind::Var, "x"));
        let block = st.child(ScopeKind::Block, file);
        let b = st.scope(block);

        assert_eq!(b.lookup("x").map(|o| o.kind), Some(ObjKind::Var));
        assert_eq!(b.lookup("int").map(|o| o.kind), Some(ObjKind::TypeName));
        assert!(b.lookup("y").is_none());
        // 只有本层找得到 / 找不到
        assert!(b.lookup_local("x").is_none());
        assert!(st.get(file).lookup_local("x").is_some());
        // 找到的层号：本层 0、父 1、祖父 2
        assert_eq!(b.lookup_level("x"), Some(1));
        assert_eq!(b.lookup_level("int"), Some(2));
        assert_eq!(b.lookup_level("z"), None);
        // 句柄不改状态：`insert` 要 `&mut`，句柄只有 `&Scope`
        assert_eq!(b.id(), block);
    }

    #[test]
    fn 遮蔽_内层盖住外层() {
        let mut st = ScopeTable::new();
        let root = st.root(ScopeKind::Universe);
        st.insert(root, Object::new(ObjKind::Var, "x"));
        let file = st.child(ScopeKind::File, root);
        st.insert(file, Object::new(ObjKind::Const, "x"));

        // 从内层看是 Const（遮蔽生效）
        assert_eq!(st.lookup(file, "x").map(|o| o.kind), Some(ObjKind::Const));
        // 外层自己看还是 Var（遮蔽不改外层的绑定）
        assert_eq!(st.lookup(root, "x").map(|o| o.kind), Some(ObjKind::Var));
        // 两层都有 ⇒ 「在哪一层」问得出来（诊断要用）
        assert_eq!(st.scope(file).lookup_level("x"), Some(0));
    }

    #[test]
    fn 同层重复声明返回被顶替的那个() {
        let mut st = ScopeTable::new();
        let s = st.root(ScopeKind::Package);
        assert!(st.insert(s, Object::new(ObjKind::Var, "x")).is_none());
        let old = st.insert(s, Object::new(ObjKind::Const, "x"));
        assert_eq!(old.map(|o| o.kind), Some(ObjKind::Var));
        assert_eq!(st.get(s).len(), 1, "同层重名是**顶替**不是新增");
        assert_eq!(
            st.get(s).lookup_local("x").map(|o| o.kind),
            Some(ObjKind::Const)
        );
    }

    #[test]
    fn 名字按字典序遍历() {
        let mut st = ScopeTable::new();
        let s = st.root(ScopeKind::Package);
        for n in ["zeta", "alpha", "mu"] {
            st.insert(s, Object::new(ObjKind::Var, n));
        }
        let got: Vec<&str> = st.get(s).names().collect();
        assert_eq!(got, vec!["alpha", "mu", "zeta"]);
    }

    #[test]
    fn 独立的兄弟层互不可见() {
        // 两个文件层共父（包级）——这正是 `T-TY-52` 要在装载器里用到的形状
        let mut st = ScopeTable::new();
        let root = st.root(ScopeKind::Universe);
        let pkg = st.child(ScopeKind::Package, root);
        let a = st.child(ScopeKind::File, pkg);
        let b = st.child(ScopeKind::File, pkg);
        st.insert(a, Object::new(ObjKind::PkgName, "f"));

        assert!(st.lookup(a, "f").is_some(), "文件 a 看得到自己的 import 名");
        assert!(
            st.lookup(b, "f").is_none(),
            "文件 b **看不到** a 的 import 名（import 不是包级声明）"
        );
        assert!(st.lookup(pkg, "f").is_none(), "包级也看不到");
    }

    #[test]
    #[should_panic(expected = "不属于本表")]
    fn 外来_scope_id_查表即_panic() {
        let mut st = ScopeTable::new();
        let root = st.root(ScopeKind::Universe);
        let foreign = ScopeId::from_index(99);
        let _ = st.get(foreign);
        let _ = root; // 上面那行就该 panic
    }
}
