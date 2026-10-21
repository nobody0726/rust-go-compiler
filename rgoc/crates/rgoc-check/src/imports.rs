//! `pkg.X` 的可见性缝合（M4/Task 35，`T-CK-74` / `T-CK-75`）。
//!
//! # 这一层管什么、不管什么
//!
//! | 面 | 归谁 |
//! |---|---|
//! | `.` / `_` / 别名的**名字进入**规则、`import init`、文件层 × 包层同名 | **装载面**（`rgoc_load::importer::stitch_imports`）—— 那是**作用域构造**，检查器看到的时候已经缝好了 |
//! | `pkg.X` 在**表达式**里的解析（操作数 + 导出性 + `undefined`） | 本模块 |
//! | `pkg.T` 在**类型表达式**里的解析 | 装载面（`rgoc_load::typeexpr::resolve_qualified`）—— 类型表达式在阶段 B 就解析完了，检查器不重走 |
//!
//! 两侧共用同一份措辞（[`rgoc_load::exported::unexported_message`] /
//! [`rgoc_load::exported::undeclared_imported_message`]），免得「同一条官方错误」
//! 在我们这里长出两个版本。
//!
//! # 与官方的对应
//!
//! 官方 `go/types/call.go:686-760` 的 `selector` 把 `pkg.X` **整段**接在
//! 选择器的开头（`if ident, ok := e.X.(*ast.Ident); ok { obj := check.lookup(ident.Name); if pname, _ := obj.(*PkgName); pname != nil { … } }`），
//! 理由写在注释里：**包名不是一个操作数模式**（`operand.go` 的 11 个模式里没有
//! `package`），所以它只能在选择器语境里被消费。我们照这个形状做：本模块只在
//! [`crate::expr`] 的 `selector` 开头被调用一次。
//!
//! # 三条判据（`call.go:731-751`）
//!
//! | 情形 | 官方 | 这里 |
//! |---|---|---|
//! | 包里没有 `X` | `UndeclaredImportedName`：`undefined: pkg.X`（忽略大小写撞上导出名时 `undefined: pkg.X (but have Y)`） | 同（报在 `.` 后的名字位） |
//! | 有 `X` 但没导出 | `UnexportedName`：`name X not exported by package Y`，然后**照常继续** | 同（不把操作数降级） |
//! | 有且导出 | 按对象种类给操作数模式 | 同（[`Operand::from_object`]） |
//!
//! # 什么时候**不**报
//!
//! `pkg.imports` 里没有这条路径 ⇒ 这个 import 没装载成功（环 / 找不到源码 /
//! 目录里没 `.go`）。官方在 `importPackage` 失败时会造一个 **fake 包**
//! （`pkg.fake = true`，见 `call.go:734` 的 `if !pkg.fake && isValidName(sel)`），
//! 于是 `pkg.X` **既不解析也不报错** —— 报错的是 import 那一侧
//! （`could not import`）。这里照做：返回 `invalid` 操作数、**零诊断**。
//!
//! ⚠️ 这与「`pkg` 根本不是包名」是两件事：那种情况返回 `None`，
//! 让 [`crate::expr::ExprChecker::expr`] 的普通名字查找去报 `undefined: pkg`。

use rgoc_ast::{Expr, SelectorExpr};
use rgoc_lex::{DiagCode, FileId};

use crate::expr::ExprChecker;
use crate::operand::Operand;

/// `pkg.X` 的**全部**裁决。返回 `None` ⇒ `e.x` 不是本文件的 import 包名，
/// 调用方继续走普通选择器（字段 / 方法查找）。
pub(crate) fn select(
    c: &mut ExprChecker<'_, '_>,
    file: FileId,
    e: &SelectorExpr,
) -> Option<Operand> {
    let Expr::Name(n) = &*e.x else { return None };
    let pkg = c.s.pkg;
    // ① `n` 必须是**本文件的** import 名（`import` 名住文件作用域）
    let path = pkg
        .import_bindings(file)
        .into_iter()
        .find(|(name, _)| *name == n.value)
        .map(|(_, p)| p)?;
    // ② 依赖没装载 ⇒ 官方造 fake 包：`pkg.X` 静默 invalid（见模块文档末段）
    let Some(dep) = pkg.imports.get(&path) else {
        return Some(Operand::invalid());
    };
    let sel = e.sel.value.as_str();
    let scope = dep.scopes.get(dep.scope);
    let Some(obj) = scope.lookup_local(sel).cloned() else {
        // 官方 `call.go:736-744`：先试忽略大小写的**导出**名，只报第一个
        let lower = sel.to_lowercase();
        let hint = scope
            .names()
            .filter(|nm| rgoc_load::exported::is_exported(nm))
            .find(|nm| nm.to_lowercase() == lower)
            .map(str::to_owned);
        c.report(
            e.sel.pos,
            DiagCode::TypUndeclaredImportedName,
            rgoc_load::exported::undeclared_imported_message(&n.value, sel, hint.as_deref()),
        );
        return Some(Operand::invalid());
    };
    // ③ 导出性（`call.go:746-750`）：报了**照常继续**（官方 `// ok to continue`）
    if !rgoc_load::exported::is_exported(sel) {
        c.report(
            e.sel.pos,
            DiagCode::TypUnexportedName,
            rgoc_load::exported::unexported_message(sel, &dep.name),
        );
    }
    // ④ 按对象种类给操作数模式（官方 `call.go:753-760` 的 switch）
    Some(Operand::from_object(&obj).unwrap_or_else(Operand::invalid))
}
