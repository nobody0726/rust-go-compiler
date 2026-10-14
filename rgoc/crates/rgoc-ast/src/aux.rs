//! 辅助类型（`M2-design.md` §3.2）。
//!
//! 运算符/字面量类别/通道方向等**共享枚举**，以及 `Name`/`BasicLit`/`Field`/
//! `Group`/`Comment`/`Pragma` 等**辅助结构**。

use crate::expr::Expr;
use rgoc_lex::pos::Pos;

/// 基本字面量类别（`AST-11`）。只标类别，值 M3 折叠。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LitKind {
    Int,
    Float,
    Imag,
    Rune,
    String,
}

/// 一元/二元运算符（`AST-21` `Operation` 的 `op` 字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Quo,
    Rem,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    AndNot,
    LAnd,
    LOr,
    Not,
    Eql,
    Neq,
    Lss,
    Leq,
    Gtr,
    Geq,
    /// 通道运算符 `<-`
    Arrow,
}

/// 赋值/自增自减运算符（`AST-39` `AssignStmt` 的 `op` 字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssignOp {
    /// `=`
    Assign,
    /// `:=`
    Define,
    /// `+=` 等复合赋值（这里存基础运算符）
    Add,
    Sub,
    Mul,
    Quo,
    Rem,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    AndNot,
    /// `++`
    Inc,
    /// `--`
    Dec,
}

/// 分支语句关键字（`AST-40` `BranchStmt`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BranchTok {
    Break,
    Continue,
    Fallthrough,
    Goto,
}

/// `go` / `defer` 调用（`AST-41` `CallStmt`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallTok {
    Go,
    Defer,
}

/// 通道方向（`F16`：`SendOnly=1`、`RecvOnly=2`；**无方向 = `Option::None`**，对应 `chan T`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ChanDir {
    SendOnly = 1,
    RecvOnly = 2,
}

/// 注释位置类别（`F17`：`Above=0`/`Below`/`Left`/`Right`）（`AST-03`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommentKind {
    Above,
    Below,
    Left,
    Right,
}

/// 标识符（`AST-10`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Name {
    pub value: String,
    pub pos: Pos,
}

/// 基本字面量（`AST-11`）。存**原文**，`bad` 标记词法阶段的畸形字面量。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BasicLit {
    pub value: String,
    pub kind: LitKind,
    pub bad: bool,
    pub pos: Pos,
}

/// 注释（`AST-03`）。`next` 串起同组的连续注释。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Comment {
    pub kind: CommentKind,
    pub text: String,
    pub next: Option<Box<Comment>>,
}

/// 文件/函数级编译指令（`PIPE-05`）。M2 只**解析并挂载**，**不生效**（效果 M7+）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Pragma {
    pub directives: Vec<String>,
}

/// 字段/参数/结果/接口方法/嵌入元素**统一载体**（`AST-28`）。
/// `name == None` → 匿名字段/嵌入元素。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Field {
    pub name: Option<Name>,
    pub type_: Expr,
    /// 结构体字段的标签（`` `json:"x"` ``），**原文保留**；非结构体字段恒为 `None`。
    ///
    /// # 为什么内联在这里，而不是官方的 `TagList []*BasicLit`
    ///
    /// 官方 `StructType` 把标签放成与 `FieldList` **下标对齐的平行数组**，
    /// 由 `addField` 用 `nil` 补齐（`parser.go:1608-1621`）。Rust 里内联是
    /// **同信息**的表示，但**不可能长度失配** —— 平行数组的长度不变式只能靠纪律维持，
    /// 而纪律会被忘（`AGENTS.md` 的「漏了不会报错」形状）。
    /// C6 对撞时把官方 `TagList[i]` 按位 zip 进第 `i` 个 `Field` 即可。
    pub tag: Option<BasicLit>,
    pub pos: Pos,
}

/// 分组声明括号（`AST-02`）。只记括号位置；分组声明**共享 iota 上下文**。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Group {
    pub open: Pos,
    pub close: Pos,
}
