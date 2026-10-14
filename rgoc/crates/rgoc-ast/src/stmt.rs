//! 语句族（`AST-33..49`）。
//!
//! **官方无 `BadStmt`**（`G19`）：语句错误时 parser 返回 `None`，错误位置由诊断流承载。

use crate::aux::{AssignOp, BranchTok, CallTok, Name};
use crate::decl::Decl;
use crate::expr::{CallExpr, Expr};
use rgoc_lex::pos::Pos;

/// 语句（`AST-33..46`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Stmt {
    Empty(EmptyStmt),
    Labeled(LabeledStmt),
    Block(BlockStmt),
    Expr(ExprStmt),
    Send(SendStmt),
    Decl(DeclStmt),
    Assign(AssignStmt),
    Branch(BranchStmt),
    Call(CallStmt),
    Return(ReturnStmt),
    If(IfStmt),
    For(ForStmt),
    Switch(SwitchStmt),
    Select(SelectStmt),
}

/// 简单语句（`if`/`for`/`switch` 的 init 位专用）。官方是 `Stmt` 子接口，Rust 侧独立成 enum。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SimpleStmt {
    Empty,
    Expr(ExprStmt),
    Send(SendStmt),
    Assign(AssignStmt),
    Range(RangeClause),
}

/// 空语句 `;`（`AST-33`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EmptyStmt {
    pub pos: Pos,
}

/// 标签语句 `Label: stmt`（`AST-34`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LabeledStmt {
    pub label: Name,
    pub stmt: Box<Stmt>,
    pub pos: Pos,
}

/// 块 `{ ... }`（`AST-35`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BlockStmt {
    pub stmts: Vec<Stmt>,
    /// 右花括号位置（官方 `nodes.go:374`）。位置不参与 dump（C6 规则 4），由 `@` 腿测。
    pub rbrace: Pos,
    pub pos: Pos,
}

/// 表达式语句（`AST-36`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExprStmt {
    pub x: Expr,
    pub pos: Pos,
}

/// 发送语句 `ch <- v`（`AST-37`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SendStmt {
    pub chan: Box<Expr>,
    pub value: Box<Expr>,
    pub pos: Pos,
}

/// 声明语句（局部 `var`/`const`/`type`）（`AST-38`）。
///
/// 官方是 `DeclList []Decl`（`nodes.go:389`）—— **列表**，不是单条：
/// `func f() { var (a int; b int) }` 里那对分组括号会产出 **2 条** `VarDecl`
/// （`appendGroup` 每行 `append` 一次，`parser.go:2661-2683`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeclStmt {
    pub decls: Vec<Decl>,
    pub pos: Pos,
}

/// 赋值/自增自减（`AST-39`）。`Inc`/`Dec` 时 `rhs` 为空。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AssignStmt {
    pub op: AssignOp,
    pub lhs: Vec<Expr>,
    pub rhs: Vec<Expr>,
    pub pos: Pos,
}

/// 分支语句（`AST-40`）。`Target` 由 `checkBranches` 计算，不在此结构。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BranchStmt {
    pub tok: BranchTok,
    pub label: Option<Name>,
    pub pos: Pos,
}

/// `go`/`defer` 调用（`AST-41`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CallStmt {
    pub tok: CallTok,
    pub call: CallExpr,
    pub pos: Pos,
}

/// `return` 语句（`AST-42`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ReturnStmt {
    pub results: Vec<Expr>,
    pub pos: Pos,
}

/// `if` 语句（`AST-43`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IfStmt {
    pub init: Option<SimpleStmt>,
    pub cond: Expr,
    pub then: BlockStmt,
    /// `else` 分支（`else if` 时是嵌套的 `Stmt::If`）。
    pub els: Option<Box<Stmt>>,
    pub pos: Pos,
}

/// `for` 语句（`AST-44`）。`range` 时 `init` 是 `Some(SimpleStmt::Range)`。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ForStmt {
    pub init: Option<SimpleStmt>,
    pub cond: Option<Expr>,
    pub post: Option<SimpleStmt>,
    pub body: BlockStmt,
    pub pos: Pos,
}

/// `switch` 语句（`AST-45`）。类型 switch 时 `tag` 是 `TypeSwitchGuard`。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SwitchStmt {
    pub init: Option<SimpleStmt>,
    pub tag: Option<Expr>,
    pub cases: Vec<CaseClause>,
    /// 右花括号位置（官方 `nodes.go:443`）。
    pub rbrace: Pos,
    pub pos: Pos,
}

/// `select` 语句（`AST-46`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SelectStmt {
    pub clauses: Vec<CommClause>,
    /// 右花括号位置（官方 `nodes.go:449`）。
    pub rbrace: Pos,
    pub pos: Pos,
}

/// `range` 子句（`AST-47`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RangeClause {
    pub lhs: Vec<Expr>,
    /// `:=` 形式。
    pub is_define: bool,
    pub x: Expr,
    pub pos: Pos,
}

/// `case` 子句（`AST-48`）。`cases == None` → `default`。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CaseClause {
    /// `None` → `default`；多值 case 时为 `ListExpr`。
    pub cases: Option<Expr>,
    pub body: Vec<Stmt>,
    /// `case` 后的冒号位置（官方 `nodes.go:465`）。
    pub colon: Pos,
    pub pos: Pos,
}

/// `select` 的 `case`/`default` 子句（`AST-49`）。`comm == None` → `default`。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CommClause {
    pub comm: Option<SimpleStmt>,
    pub body: Vec<Stmt>,
    /// `case`/`default` 后的冒号位置（官方 `nodes.go:472`）。
    pub colon: Pos,
    pub pos: Pos,
}
