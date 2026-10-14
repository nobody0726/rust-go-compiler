//! 声明族（`AST-01..08`）。

use crate::aux::{BasicLit, Field, Group, Name, Pragma};
use crate::expr::Expr;
use crate::stmt::BlockStmt;
use rgoc_lex::pos::Pos;

/// 编译单元（`AST-01`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AstFile {
    /// 文件起点位置。
    pub pos: Pos,
    /// 文件级 `//go:` 指令。
    pub pragma: Pragma,
    /// `package` 子句里的包名。
    pub pkg_name: Name,
    /// 顶层声明。
    pub decls: Vec<Decl>,
    /// EOF 位置。
    pub eof: Pos,
    /// 来自 `//go:build` 的版本（`F7`；仅 `top` 时取）。
    pub go_version: Option<String>,
}

/// 顶层声明（`AST-04..08`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Decl {
    Import(ImportDecl),
    Const(ConstDecl),
    Type(TypeDecl),
    Var(VarDecl),
    Func(FuncDecl),
}

/// `import` 声明（`AST-04`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ImportDecl {
    /// 分组括号（`import ( ... )` 时为 `Some`）。
    pub group: Option<Group>,
    /// 声明级 `//go:` 指令（官方 `nodes.go:66` 的 `Pragma`）。
    pub pragma: Pragma,
    /// 别名（`. "pkg"` 的 `.` 也记在此，值分别为 `"."` / `"_"`）；无别名 → `None`。
    pub name: Option<Name>,
    /// 路径字面量（**只验合法字符串语法，不加载包**）。
    pub path: BasicLit,
    pub pos: Pos,
}

/// `const` 声明（`AST-05`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConstDecl {
    pub group: Option<Group>,
    /// 声明级 `//go:` 指令（官方 `nodes.go:77`）。
    pub pragma: Pragma,
    /// 名字**列表**（官方 `NameList []*Name`，`nodes.go:78`）：`const a, b = 1, 2`。
    pub names: Vec<Name>,
    /// `None` → 省略类型（从值推断）。
    pub type_: Option<Expr>,
    /// 值表达式（`iota` 是 `Expr::Name(Name{value:"iota"})`；值 M3 折叠）。
    pub values: Vec<Expr>,
    pub pos: Pos,
}

/// `type` 声明（`AST-06`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TypeDecl {
    pub group: Option<Group>,
    /// 声明级 `//go:` 指令（官方 `nodes.go:88`）。
    pub pragma: Pragma,
    pub name: Name,
    /// `type A = B`（别名）为 `true`。
    pub alias: bool,
    /// 泛型类型参数列表 `[T any]`。
    pub tparams: Vec<Field>,
    pub type_: Expr,
    pub pos: Pos,
}

/// `var` 声明（`AST-07`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VarDecl {
    pub group: Option<Group>,
    /// 声明级 `//go:` 指令（官方 `nodes.go:100`）。
    pub pragma: Pragma,
    pub names: Vec<Name>,
    pub type_: Option<Expr>,
    pub values: Vec<Expr>,
    pub pos: Pos,
}

/// `func` 声明（`AST-08`）。`recv != None` → 方法。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FuncDecl {
    /// 接收者（方法时为 `Some`）。
    pub recv: Option<Field>,
    pub name: Name,
    /// 泛型类型参数 `[T any]`。
    pub tparams: Vec<Field>,
    /// 函数类型（参数/结果）。
    pub type_: Expr,
    /// `None` → 外部声明（无 body）。
    pub body: Option<BlockStmt>,
    /// 函数级 `//go:` 指令。
    pub pragma: Pragma,
    pub pos: Pos,
}
