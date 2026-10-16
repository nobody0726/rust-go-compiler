//! 作用域（M3/Task 15；pre-flight Ruling ⑤）。
//!
//! 五层嵌套：`universe ⊃ package ⊃ file ⊃ function ⊃ block`（`SCP-01`）。
//! 本文件只做**基元**（建层、插名字、沿链查名字），层次语义（哪些声明进哪一层、
//! 包级 vs 块级重名规则）归检查器（Task 19+）。
//!
//! # 为什么用「借父」而不是 `Rc<RefCell<..>>`
//!
//! ```text
//! let universe = declare_universe(&mut tt);   // 活到本例结束
//! let mut file = Scope::child(&universe);     // 借它
//! let mut block = Scope::child(&file);        // 再借
//! ```
//!
//! 检查器沿 AST 下行时**天然**是「父比子活得久」（递归的栈就是这条链），
//! 所以借用检查器能静态地保证「没有孤儿作用域」，而且零成本 ——
//! 用 `Rc<RefCell>` 换来的只是「可以随手克隆/乱挂父」，那是本层不需要的自由，
//! 代价是运行期才能发现环。
//!
//! # 为什么是 `BTreeMap`
//!
//! 名字 → 对象要**确定性遍历**（诊断顺序、`open_gaps` 的报告、测试断言都靠它），
//! `HashMap` 的迭代序不稳定。作用域里的名字是几十个量级，BTree 的对数因子不是问题。

use std::collections::BTreeMap;

use crate::object::Object;

/// 一层作用域。
#[derive(Debug)]
pub struct Scope<'a> {
    /// 外层；最外层（universe）是 `None`。
    parent: Option<&'a Scope<'a>>,
    /// 本层定义的名字 → 对象。
    objects: BTreeMap<String, Object>,
}

impl<'a> Scope<'a> {
    /// 最外层作用域（没有父）。
    #[must_use]
    pub fn root() -> Self {
        Self {
            parent: None,
            objects: BTreeMap::new(),
        }
    }

    /// 建一层子作用域。
    #[must_use]
    pub fn child(parent: &'a Scope<'a>) -> Self {
        Self {
            parent: Some(parent),
            objects: BTreeMap::new(),
        }
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

    /// 沿**父链**找（含本层）。
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&Object> {
        let mut s = Some(self);
        while let Some(cur) = s {
            if let Some(o) = cur.objects.get(name) {
                return Some(o);
            }
            s = cur.parent;
        }
        None
    }

    /// `name` 是在**哪一层**被找到的（`None` ⇒ 没找到）。
    ///
    /// 「在哪一层」有用：诊断要说「与包级声明的 `x` 冲突」还是「本块内重复」。
    #[must_use]
    pub fn lookup_level(&self, name: &str) -> Option<usize> {
        let mut s = Some(self);
        let mut level = 0usize;
        while let Some(cur) = s {
            if cur.objects.contains_key(name) {
                return Some(level);
            }
            level += 1;
            s = cur.parent;
        }
        None
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

    /// 到最外层一共有几层（本层算 1）。
    #[must_use]
    pub fn depth(&self) -> usize {
        let mut d = 1;
        let mut s = self.parent;
        while let Some(cur) = s {
            d += 1;
            s = cur.parent;
        }
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::ObjKind;

    #[test]
    fn 建层与深度() {
        let root = Scope::root();
        assert_eq!(root.depth(), 1);
        assert!(root.is_empty());
        let file = Scope::child(&root);
        assert_eq!(file.depth(), 2);
        let block = Scope::child(&file);
        assert_eq!(block.depth(), 3, "universe ⊃ package ⊃ file");
    }

    #[test]
    fn 插入与沿链查名字() {
        let mut root = Scope::root();
        root.insert(Object::new(ObjKind::TypeName, "int"));
        let mut file = Scope::child(&root);
        file.insert(Object::new(ObjKind::Var, "x"));
        let block = Scope::child(&file);

        assert_eq!(block.lookup("x").map(|o| o.kind), Some(ObjKind::Var));
        assert_eq!(block.lookup("int").map(|o| o.kind), Some(ObjKind::TypeName));
        assert!(block.lookup("y").is_none());
        // 只有本层找得到 / 找不到
        assert!(block.lookup_local("x").is_none());
        assert!(file.lookup_local("x").is_some());
        // 找到的层号：本层 0、父 1、祖父 2
        assert_eq!(block.lookup_level("x"), Some(1));
        assert_eq!(block.lookup_level("int"), Some(2));
        assert_eq!(block.lookup_level("z"), None);
    }

    #[test]
    fn 遮蔽_内层盖住外层() {
        let mut root = Scope::root();
        root.insert(Object::new(ObjKind::Var, "x"));
        let mut file = Scope::child(&root);
        file.insert(Object::new(ObjKind::Const, "x"));
        // 从内层看是 Const（遮蔽生效）
        assert_eq!(file.lookup("x").map(|o| o.kind), Some(ObjKind::Const));
        // 外层自己看还是 Var（遮蔽不改外层的绑定）
        assert_eq!(root.lookup("x").map(|o| o.kind), Some(ObjKind::Var));
        // 两层都有 ⇒ 「在哪一层」问得出来（诊断要用）
        assert_eq!(file.lookup_level("x"), Some(0));
    }

    #[test]
    fn 同层重复声明返回被顶替的那个() {
        let mut s = Scope::root();
        assert!(s.insert(Object::new(ObjKind::Var, "x")).is_none());
        let old = s.insert(Object::new(ObjKind::Const, "x"));
        assert_eq!(old.map(|o| o.kind), Some(ObjKind::Var));
        assert_eq!(s.len(), 1, "同层重名是**顶替**不是新增");
        assert_eq!(s.lookup_local("x").map(|o| o.kind), Some(ObjKind::Const));
    }

    #[test]
    fn 名字按字典序遍历() {
        let mut s = Scope::root();
        for n in ["zeta", "alpha", "mu"] {
            s.insert(Object::new(ObjKind::Var, n));
        }
        let got: Vec<&str> = s.names().collect();
        assert_eq!(got, vec!["alpha", "mu", "zeta"]);
    }
}
