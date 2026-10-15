//! 语句族（`AST-33..49`）。
//!
//! **官方无 `BadStmt`**（`G19`）：语句错误时 parser 返回 `None`，错误位置由诊断流承载。

use crate::aux::{AssignOp, BranchTok, CallTok, Name};
use crate::decl::Decl;
use crate::expr::Expr;
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
    /// **任意表达式**，不一定是 [`crate::expr::CallExpr`]。
    ///
    /// # 为什么不是 `CallExpr`（`T-AP14` 修正的 `T-AP03` 建模缺口）
    ///
    /// 官方是 `Call Expr`（`nodes.go:413`），而 `callStmt`（`parser.go:995`）
    /// 只是把 `pexpr` 的返回值**直接赋进去**、不做任何形状检查：
    ///
    /// ```text
    /// go f        // 语法合法（类型检查才拒绝）⇒ Call == Name(f)
    /// defer (f()) // ⇒ Call == ParenExpr，另报 "must not be parenthesized"
    /// ```
    ///
    /// 三处官方证据一致：`positions.go:296` 的 `m = n.Call`、`walk.go:247` 的
    /// `w.node(n.Call)`、`printer.go:554` 的 `p.print(n.Tok, blank, n.Call)` ——
    /// 全都按 `Expr` 用。写成 `CallExpr` 会在 `go f` 上**无路可走**（要么炸、
    /// 要么造一个假调用），而两种症状都是"看起来合理"。
    ///
    /// 与 PA-1 / `Op::Tilde` / `IfStmt.cond` 同形：**字段的**类型**也必须与官方逐字对齐**，
    /// 不只是字段名。
    ///
    /// ⚠️ 官方的第三个字段 `DeferAt Expr`（`nodes.go:414`，`runtime.deferprocat` 的实参）
    /// **刻意不建模** —— 全 `syntax` 包内**没有任何一处写或读它**（`grep DeferAt` 只有那一行声明），
    /// 它属于后面编译器阶段。这与本条的缺口**不是**同一回事：缺的是"官方 parser 会赋值的字段"。
    pub call: Expr,
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
    /// 条件表达式。**可以是 `None`** —— 官方 `Cond Expr` 在恢复路径上会留 `nil`。
    ///
    /// # 为什么是 `Option`（`T-AP13` 修正的 `T-AP03` 建模缺口）
    ///
    /// 官方 `header`（`parser.go:2352`）的 `cond` 是一个**只在被赋值时才有值**的
    /// 具名返回值。绝大多数路径都会设它（正常条件、`if {` 的 `badExpr`、
    /// `if x; {` 的 `missing condition` + `badExpr`），但有一条路径**不会**：
    ///
    /// ```text
    /// if x y {}      // `want(_Lbrace)` 报错 → advance() 恰好停在 `{` 上
    ///                // ⇒ `else if p.tok != _Lbrace` 不成立 ⇒ condStmt 保持 nil
    ///                // ⇒ semi 也没记（没走 `_Semi` 那条）⇒ cond 永远没被赋值
    /// ```
    ///
    /// 于是官方的 `IfStmt.Cond == nil`。这不是「未实现」，是**该错误路径的确切语义**
    /// （因而不能用 `Expr::Bad` 冒充 —— `Bad` 自带位置，且 `String()`/`Pos()` 行为不同）。
    /// 与 PA-1／`Op::Tilde` 同形：**「能编译 + 断言全绿」对数据模型不构成正确性证据**
    /// （`T-AP03` 的断言全是构造字面量，照不出「哪个字段可为 nil」）。
    pub cond: Option<Expr>,
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
