//! 包级 `const` 的**检查面**折叠（M4/Task 30；`T-CK-43`，解除 `U-M3-08` 的
//! 检查面半边）。
//!
//! # 为什么要有这一层
//!
//! 装载面（`rgoc-load::decls`）的常量求值器只认**常量子集**（字面量算式、
//! `iota`、已折叠常量的名字）—— 它没有类型信息，判不了 `len(s)` / `f()` /
//! `H + 1`，于是统一报 `TypInvalidConstInit`（`D-M3-19` / `T-TY-83`）。
//!
//! M4 的检查面**有**类型信息了（`rgoc-check` 的 `expr` 能算 `len` / `cap` 的
//! 常量结果），所以 `const x = len("abc")` 这一类在检查面**应当合法**。本模块
//! 就是那条接线：
//!
//! 1. **先**在装载诊断入流之前跑一遍「折叠预演」，把能折成常量的 `const`
//!    初始化式记下来（名字 → `(类型, 值)`）；
//! 2. **再**让 [`crate::check::Session::seed`] 丢掉**恰好落在这些位置**的
//!    `TypInvalidConstInit`。
//!
//! 装载面本身**不改**：`rgoc types`（`T-TY-83` 正向）照样拒绝，只是 `rgoc
//! check` 不再跟着报。这是「解除 `U-M3-08`」在 M4 计划里的原文口径。
//!
//! # 预演的副作用
//!
//! 预演会**真的**跑一遍表达式检查，因而可能产出诊断。那些诊断在这里**全部
//! 丢弃**（快照 + 回滚 `diags` 与 `reported_any`）—— 折叠面只回答「这个
//! 初始化式是不是常量」，报错与否归主检查（Task 25 的语句面 / Task 32 的
//! 初始化顺序面）。留下的副作用只有类型表上新增的 `TypeId`，那是幂等的。
//!
//! # 窄面（台账登记，`Ruling 30-1`）
//!
//! * `unsafe.Sizeof/Alignof/Offsetof`：**尚未**接入（Task 31 的 `unsafe_pkg`
//!   落地后自动接上，本模块不需要改）；
//! * 声明了类型的常量（`const x int = len("abc")`）只判「是否常量」，不在这里
//!   做 `representable` —— 那是装载面的既有职责，本模块不重复；
//! * 依赖**已折叠**常量的链（`const a = len("x"); const b = a`）按声明序
//!   递推：`b` 求值时 `a` 已经在 [`crate::check::Session::consts`] 里，所以
//!   链是通的（`probe/task30-chain.go` 三行实测）。

use std::collections::BTreeSet;

use rgoc_ast::{Decl, Expr};
use rgoc_const::Const;
use rgoc_lex::Pos;

use crate::check::Session;
use crate::expr::ExprChecker;
use crate::operand::Mode;

/// 折叠预演：返回**可折叠**的初始化式位置集合（供 `seed` 过滤装载诊断）。
///
/// 顺带把「名字 → `(类型, 值)`」写进 [`Session::consts`] —— 主检查阶段
/// 遇到这些名字时，[`crate::expr`] 的 `ident` 会用折叠值顶替装载面留下的
/// 「常量无值」。
pub(crate) fn fold_package_consts(s: &mut Session<'_>) -> BTreeSet<Pos> {
    let pkg = s.pkg;
    let mut foldable: BTreeSet<Pos> = BTreeSet::new();

    // 预演诊断全部丢弃（见模块头「预演的副作用」）。
    let diag_mark = s.diag_len();
    let reported_mark = s.reported_any();

    {
        let mut cx = ExprChecker { s, env: None };
        for (i, ast) in pkg.asts.iter().enumerate() {
            let file = pkg.files[i];
            // 分组继承：`const (a = 1; b)` 的 `b` 沿用上一条的值表达式
            // （官方 `expand_const_group_specs` 的语义）。解析器把每个 spec
            // 拆成独立的 `Decl::Const`，所以这里按「连续 const 声明」维持。
            let mut inherited: Option<Vec<Expr>> = None;
            for decl in &ast.decls {
                let Decl::Const(c) = decl else {
                    inherited = None;
                    continue;
                };
                let values: &[Expr] = if c.values.is_empty() {
                    match &inherited {
                        Some(v) => v,
                        None => continue,
                    }
                } else {
                    inherited = Some(c.values.clone());
                    &c.values
                };
                for (k, name) in c.names.iter().enumerate() {
                    if name.value == "_" {
                        continue;
                    }
                    let Some(value) = values.get(k) else { continue };
                    let at = value.start_pos();
                    let op = cx.expr(file, value);
                    if op.mode != Mode::Constant {
                        continue;
                    }
                    let Some(val) = op.val else { continue };
                    if val == Const::Unknown {
                        continue;
                    }
                    let Some(typ) = op.typ else { continue };
                    cx.s.consts.insert(name.value.clone(), (typ, val));
                    foldable.insert(at);
                }
            }
        }
    }

    s.rollback_diags(diag_mark, reported_mark);
    foldable
}
