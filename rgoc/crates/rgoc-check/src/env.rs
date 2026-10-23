//! 检查面的**局部作用域**（M4/Task 25）。
//!
//! # 为什么检查器要自己维护一份
//!
//! M3 的 [`rgoc_load::scope`] 已经为每个函数体/块建了作用域（`ScopeTable`），
//! 但那张表是**装载期**的产物：它按 `ScopeId` 组织，**没有位置索引**，也不
//! 携带 M4 需要的「操作数」（`Operand`：模式 + 类型 + 常量值）。检查器要回答的
//! 是「这个 `y` 现在是什么类型的值」，而装载表里局部变量的 `typ` 多半是
//! `invalid`（`:=` 与参数都不求类型 —— 那是 M3 的既定分工）。
//!
//! 所以检查面按官方 `Checker.scope` 的同一节奏**再走一遍**：进入块开一层、
//! 声明就记一条、离开块弹一层。两层各管各的：
//!
//! | 层 | 负责 | 谁消费 |
//! |---|---|---|
//! | 装载表（M3） | 「名字有没有声明」「重复声明」「`undefined`」 | 诊断的**存在性** |
//! | 本表（M4） | 「这个名字此刻是什么操作数」 | 类型判定 / 可赋值性 |
//!
//! # 窄面（台账登记）
//!
//! 只装**已求值**的操作数：`var x T` 的类型（`T` 是名字 ⇒ [`crate::expr`] 能
//! 给出类型）、`x := v` 的 `v` 的类型（无类型常量取默认型）、参数 / 命名结果
//! （签名里的类型）、`range` 迭代变量与类型 switch 的隐式变量（`invalid`，
//! 与官方「每个 case 各一份、类型随 case 变」同形但类型面归 Task 26/28）。
//! **不**装「被赋过值就换类型」这类流敏感信息 —— Go 的类型不随赋值变。

use std::collections::BTreeMap;

use crate::operand::Operand;

/// 局部作用域栈（每层一个名字表）。
#[derive(Clone, Debug, Default)]
pub struct Env {
    /// 栈底 = 函数层（参数 / 命名结果），往上依次是块。
    scopes: Vec<BTreeMap<String, Operand>>,
}

impl Env {
    /// 空栈（调用方必须先 [`Env::push`] 一层再声明）。
    pub fn new() -> Self {
        Self { scopes: Vec::new() }
    }

    /// 开一层。
    pub fn push(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    /// 关一层（栈空时是 no-op —— 调用方的 push/pop 必须配对，这里只是不炸）。
    pub fn pop(&mut self) {
        self.scopes.pop();
    }

    /// 在当前层声明一个名字（`_` 不绑定；同层重复由 M3 报，这里**不覆盖**）。
    pub fn declare(&mut self, name: &str, op: Operand) {
        if name == "_" {
            return;
        }
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.to_owned(), op);
        }
    }

    /// 沿栈查（内层优先）。
    pub fn lookup(&self, name: &str) -> Option<&Operand> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    /// **本层**是否已有该名字（短变量声明判「新变量」用；不看外层）。
    pub fn has_local(&self, name: &str) -> bool {
        self.scopes.last().is_some_and(|s| s.contains_key(name))
    }
}
