//! 导出符号摘要（`ExportSummary`，`D-M3-18`）—— M3/Task 29。
//!
//! # 是什么（以及**不是**什么）
//!
//! `D-M3-18` 只给**内存形态**：`导出名 → 该对象的类型（`C7` dump 文本）`。
//! **不**冻文本格式、**不**落盘、**不**做版本化 —— 那三件都等 M4 走契约流程
//! （`D-M3-5` 已排除「完整档」）。所以本模块的判据是**结构断言**
//! （名字集合 + 值里有哪些行 + 两次调用相同），不是逐字节对撞。
//!
//! # 三条口径
//!
//! 1. **只收包作用域自己的名字**。沿父链走会把 universe 的 `int` / `error` 收进来
//!    （那不是本包的导出符号）；`import` 名住**文件**作用域（`SCP-01` 的五层分工，
//!    Task 23 落地）⇒ 也不在。所以这里用「本层名字 + `lookup_local`」，
//!    **不**用会沿链查的 `lookup`。
//! 2. **只收导出名**（[`is_exported`]，`SCP-06`，Task 26 的生成表 —— 不用 Rust std
//!    的 `char::is_uppercase`，它多 120 个码点）。⚠️ **内层**名字（结构体字段、
//!    接口方法…）**不过滤**：它们是**类型的一部分**，在 `C7` dump 里照常出现 ——
//!    摘要筛的是**顶层符号**，不是类型里的每个名字。
//! 3. **值 = 该对象类型的 `C7` 自足小 dump**（[`dump_types`] 去掉 `#` 头行）：
//!    函数就是它的**签名**，其余就是它们的类型；常量只给**类型**
//!    （值在 [`Object::val`] 里，摘要里再抄一份就有两个真相）。
//!    每个条目的编号**各自从 0 起** ⇒ 单个值能独立读懂，不需要外部编号表；
//!    代价是跨条目的编号不共享 —— M4 若要**全局**编号，应当直接消费
//!    `dump_types(tt, &pkg.type_roots)`，而不是这个摘要。
//!
//! # 顺序为什么是确定的
//!
//! [`ExportSummary`] 的骨架是 `BTreeMap<String, _>` ⇒ 名字按 `String` 序
//! （= ASCII 序，`EN` 在 `en` 前）。官方 `go/types` 的 `Scope.Names()` 也是
//! **排过序**的 —— 这一点两侧无意中一致，`D-M3-18` 顺手把它定下来。

use std::collections::BTreeMap;

use rgoc_types::{ObjKind, Object, TypeTable, dump_types};

use crate::exported::is_exported;
use crate::package::Package;

/// 一个导出对象的**种类**，以及它的类型（`C7` dump 文本）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ExportEntry {
    /// `type T …` —— 值是 `T` 本身的 dump（`named` 行 + 本体）。
    Type(String),
    /// `const C …` —— 值是 `C` 的**类型**（常量的值留在 `Object::val`）。
    Const(String),
    /// `func F …` —— 值是**签名**的 dump。
    Func(String),
    /// `var V …` —— 值是 `V` 的类型。
    Var(String),
}

impl ExportEntry {
    /// 值（`C7` dump 文本）。
    #[must_use]
    pub fn value(&self) -> &str {
        match self {
            ExportEntry::Type(s)
            | ExportEntry::Const(s)
            | ExportEntry::Func(s)
            | ExportEntry::Var(s) => s,
        }
    }
}

/// 一个包的**导出符号摘要**（`D-M3-18` 的内存形态）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ExportSummary {
    entries: BTreeMap<String, ExportEntry>,
}

impl ExportSummary {
    /// 按名字取（**只**含导出名 ⇒ 非导出名恒 `None`）。
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ExportEntry> {
        self.entries.get(name)
    }

    /// 名字，**字典序**（`BTreeMap` 序）。
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// `(名字, 条目)`，字典序。
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ExportEntry)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// 有多少个导出名。
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 有没有导出名（空包 / 全非导出包都会是空）。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 算出 `pkg` 的导出符号摘要（`tt` 必须就是装载 `pkg` 时用的那张表）。
#[must_use]
pub fn export_summary(pkg: &Package, tt: &TypeTable) -> ExportSummary {
    let mut entries = BTreeMap::new();
    // 只扫**本层**（见模块文档口径 1）—— 顺着 `names()` 拿名字，再 `lookup_local`
    // 取对象；两者都只看这一层，所以 universe / 文件作用域的名字进不来。
    let scope = pkg.scopes.get(pkg.scope);
    for name in scope.names() {
        if !is_exported(name) {
            continue;
        }
        let Some(obj) = scope.lookup_local(name) else {
            continue; // 不可能：名字就来自这一层
        };
        let value = type_dump(tt, obj);
        let entry = match obj.kind {
            ObjKind::TypeName => ExportEntry::Type(value),
            ObjKind::Const => ExportEntry::Const(value),
            ObjKind::Func => ExportEntry::Func(value),
            ObjKind::Var => ExportEntry::Var(value),
            // 包作用域**结构上**只有这四种：`import` 名进文件作用域（口径 1）、
            // 标签不进值作用域（`T-TY-57`）、方法与内建也不在（`collect_top_level_names`）。
            // 真的出现第五种说明前面某处的分层坏了 ⇒ 出声比静默丢掉好。
            other => unreachable!(
                "包作用域里出现了 {other:?}（`{name}`）—— 只有 Var/Const/TypeName/Func 能进来"
            ),
        };
        entries.insert(name.to_string(), entry);
    }
    ExportSummary { entries }
}

/// 一个对象的类型，按 `C7` 渲染成**自足**小 dump。
///
/// * 没有类型（`typ: None`）⇒ `-`：与 `C7` 里「不知道 / 不适用」的写法同形
///   （数组长度、无名元组都用它）。
/// * 去掉 `# source: rgoc` 头行：那是**来源标记**，不是类型描述
///   （`C7` §2 规则 1 说 `#` 行两侧都忽略 ⇒ 摘要在语义上没丢东西）。
fn type_dump(tt: &TypeTable, obj: &Object) -> String {
    let Some(id) = obj.typ else {
        return "-".to_owned();
    };
    let full = dump_types(tt, &[id]);
    full.strip_prefix("# source: rgoc\n")
        .unwrap_or(&full)
        .to_owned()
}
