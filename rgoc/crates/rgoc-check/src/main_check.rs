//! `main` / `init` 的约束（M4/Task 34；`T-CK-73`）。
//!
//! ⚠️ **模块名为什么是 `main_check` 而不是计划里的 `main`**：Cargo 把
//! `src/main.rs` 当**二进制目标**自动发现，`pub mod main;` 还会触发
//! `special_module_name` 警告（lib 里不能有 `main` 模块）。语义不变，
//! 只是把名字让开工具链（台账 `Ruling 34-1`）。
//!
//! # 判据来源
//!
//! 官方 `go/types/resolver.go` 的两处：
//!
//! | 位置 | 触发 | 官方消息 | 代号 |
//! |---|---|---|---|
//! | `:111`（`declarePkgObj`） | 非函数叫 `init` | `cannot declare init - must be func` | `InvalidInitDecl` |
//! | `:118`（`declarePkgObj`） | 非函数叫 `main` 且包名 `main` | `cannot declare main - must be func` | `InvalidMainDecl` |
//! | `:427`（`funcDecl`） | `func init/main` 有类型参数 | `func %s must have no type parameters` | 同上（按名字分） |
//! | `:433`（`funcDecl`） | `func init/main` 有参数或结果 | `func %s must have no arguments and no return values` | 同上 |
//! | `:437`（`funcDecl`） | `func init` 无体 | `func init must have a body` | `InvalidInitDecl`（`MissingInitBody` 的 C8 侧） |
//!
//! # 三条实测口径（`probe/t33/t34-main.go`，oracle 9 行）
//!
//! 1. **只对非方法**：`func (T) init()` / `func (T) main()` 合法 —— 官方那条
//!    `if d.decl.Recv.NumFields() == 0` 分支才查。
//! 2. **`main` 只在 `package main` 里查**；`package p` 里的 `func main()` 合法。
//! 3. **报点**：类型参数面报在**第一个类型参数字段**（`10:11` 的 `T`）；
//!    参数/结果面与「无体」都报在**函数名**（`4:6` / `14:6`）。
//!
//! # 与装载面的分工
//!
//! 装载面（`package.rs::collect_top_level_names`）只负责**不声明** `init`
//! 与非函数的 `init`/`main`（官方同样是「不声明 + 报一条」）；**消息全在本模块**
//! —— 否则两侧各报一条，C8 保留重复行会当场翻倍。
//!
//! # C8 类别为什么分家
//!
//! 签名面（`… no arguments and no return values`）落 `ret`，形态面
//! （`cannot declare …` / `must have no type parameters` / `must have a body`）
//! 落 `decl` —— 这不是本仓的选择，是 oracle 适配表的**规则序**：
//! `check.go:260` 的 `return values` 规则排在 `:278` 的 `func (main|init) must`
//! 之前，所以前者的消息被 `ret` 先截获。`T-CK-73` 的 9 行向量就是这条的钉子。

use rgoc_ast::{Decl, Expr, Field, FuncDecl};
use rgoc_lex::{Diag, DiagCode, Pos};
use rgoc_load::Package;

/// 扫一个包的 `main` / `init` 约束。
#[must_use]
pub fn check(pkg: &Package) -> Vec<Diag> {
    let mut out = Vec::new();
    let in_main = pkg.name == "main";
    for ast in &pkg.asts {
        for decl in &ast.decls {
            match decl {
                Decl::Type(d) => reserved(&mut out, &d.name.value, d.name.pos, in_main),
                Decl::Const(d) => {
                    for n in &d.names {
                        reserved(&mut out, &n.value, n.pos, in_main);
                    }
                }
                Decl::Var(d) => {
                    for n in &d.names {
                        reserved(&mut out, &n.value, n.pos, in_main);
                    }
                }
                Decl::Func(f) => func(&mut out, f, in_main),
                Decl::Import(_) => {}
            }
        }
    }
    out
}

/// 非函数声明占用 `init` / `main` 的名字（官方 `declarePkgObj` 的两条早退）。
fn reserved(out: &mut Vec<Diag>, name: &str, pos: Pos, in_main: bool) {
    if name == "_" {
        return;
    }
    if name == "init" {
        out.push(Diag {
            pos,
            code: DiagCode::TypInvalidInitDecl,
            message: "cannot declare init - must be func".to_owned(),
        });
    } else if name == "main" && in_main {
        out.push(Diag {
            pos,
            code: DiagCode::TypInvalidMainDecl,
            message: "cannot declare main - must be func".to_owned(),
        });
    }
}

/// `func` 声明面（官方 `funcDecl` 的 `Recv.NumFields() == 0` 分支）。
fn func(out: &mut Vec<Diag>, f: &FuncDecl, in_main: bool) {
    // 方法不受限（`func (T) init()` 合法）。
    if f.recv.is_some() {
        return;
    }
    let name = f.name.value.as_str();
    let is_init = name == "init";
    let is_main = name == "main" && in_main;
    if !is_init && !is_main {
        return;
    }
    let (decl_code, sig_code) = if is_init {
        (DiagCode::TypInvalidInitDecl, DiagCode::TypInvalidInitSig)
    } else {
        (DiagCode::TypInvalidMainDecl, DiagCode::TypInvalidMainSig)
    };

    // ① 类型参数面：报在**第一个类型参数字段**（官方 `tparam0`）。
    if let Some(tp) = f.tparams.first() {
        out.push(Diag {
            pos: field_pos(tp),
            code: decl_code,
            message: format!("func {name} must have no type parameters"),
        });
    }

    // ② 参数/结果面：报在**函数名**。
    if let Expr::FuncType(ft) = &f.type_
        && (!ft.params.is_empty() || !ft.results.is_empty())
    {
        out.push(Diag {
            pos: f.name.pos,
            code: sig_code,
            message: format!("func {name} must have no arguments and no return values"),
        });
    }

    // ③ 体：只有 `init` 必须有体（`main` 允许前向声明 —— `main0.go:7`）。
    if is_init && f.body.is_none() {
        out.push(Diag {
            pos: f.name.pos,
            code: DiagCode::TypInvalidInitDecl,
            message: "func init must have a body".to_owned(),
        });
    }
}

/// 类型参数字段的起点（`[T any]` ⇒ `T` 的位置）。
///
/// `Field` 没有自己的 `pos`（见 `rgoc-ast/aux.rs`）：有名取名字的位置，
/// 无名（`[any]` 这种约束项）取类型表达式的起点。
fn field_pos(f: &Field) -> Pos {
    f.name
        .as_ref()
        .map_or_else(|| f.type_.start_pos(), |n| n.pos)
}
