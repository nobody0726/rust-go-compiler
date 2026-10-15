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
///
/// # 变体集**逐项对齐**官方 `tokens.go:108-149` 的 `Operator`
///
/// | 官方 | 这里 | 备注 |
/// |---|---|---|
/// | `Not` `Recv` `Tilde` | `Not` `Arrow` `Tilde` | `Recv` 改名为 `Arrow`（更贴近源码写法 `<-`） |
/// | `OrOr` `AndAnd` | `LOr` `LAnd` | 同上 |
/// | `Div` | `Quo` | 同上（`Quo` 是官方 `nodes.go` 里 `Operation` 的写法） |
/// | 其余 17 个 | 同名 | — |
///
/// ⚠️ **官方的 `Def`（`:=` 里的 `:`）刻意不收**：它只可能出现在赋值语境，
/// 而赋值用的 `AssignOp`（[`AssignOp::Define`]）是**另一个**枚举。
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
    /// 通道运算符 `<-`（官方 `Recv`）
    Arrow,
    /// 类型约束里的 `~T`（官方 `Tilde`）。
    ///
    /// 它**不在**任何优先级里（官方 `prec` 为 0），唯一产地是官方
    /// `embeddedTerm()`（`parser.go:1884-1891`）—— 即 `interface` 的类型约束。
    /// 补它是**回源逐项 diff** 官方 `Operator` 时发现漏项的结果（`T-AP10`）。
    Tilde,
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

impl Field {
    /// 节点起点位置（官方 `Field.Pos() Pos`）。
    ///
    /// 官方是 `if f.Name != nil { return f.Name.Pos() } ; return f.Type.Pos()`。
    /// 唯一用到的地方是 `methodDecl` 造 `ListExpr` 时取 `list[0].Pos()`
    /// （`parser.go:1836`）—— 那一支里 `list[0]` 恰好没有名字（`Name == nil`），
    /// 所以写成 `f.type_.pos()` 也**碰巧**对。**照抄官方**是为了将来那一支改了
    /// 名字传递之后不会静默错位（错了只会让某个 `pos` 差几列，dump 里看不出来）。
    pub fn pos(&self) -> Pos {
        match &self.name {
            Some(n) => n.pos,
            None => self.type_.pos(),
        }
    }
}

/// 分组声明括号（`AST-02`）。
///
/// # 官方是**零字段的身份标识**，我们必须造一个「值身份」
///
/// `nodes.go:138-141`：
///
/// ```go
/// // All declarations belonging to the same group point to the same Group node.
/// type Group struct {
///     _ int // not empty so we are guaranteed different Group instances
/// }
/// ```
///
/// 也就是说官方**不存位置**，`Group` 存在的唯一目的是让「是不是同一组」可以用
/// **指针相等**判定。Rust 没有指针身份（`Box` 树里同一格不会有多个所有者），
/// 所以用 `open`（`(` 的位置）当**值身份**：
/// **同一文件里 `(` 的位置唯一** ⇒ 「`group` 相等」⟺「同组」，与官方的指针相等同义。
///
/// # 为什么不留 `close`
///
/// 官方不存它。而更实际的理由是**实现形状**：官方 `appendGroup` 里的 `g` 是个
/// 可变对象，组内每条声明都指向它，所以「读完 `)` 再补上」是自然的；
/// 我们的 `Group` 是**值**，回调里构造它时 `)` 还没读到 —— 留着 `close` 会逼出
/// 「先收集、再回填」的两遍写法，凭空多一处能与官方分叉的地方。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Group {
    /// 分组括号里 **`(` 的位置** —— 同时充当这一组的身份。
    pub open: Pos,
}
