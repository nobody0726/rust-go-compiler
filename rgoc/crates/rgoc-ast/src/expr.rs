//! 表达式族（`AST-09..32`）。
//!
//! **类型与表达式同族**（`M2-design.md` §3.1 / **G18**）：parser 阶段无法区分类型与表达式
//! （`T` 既可能是类型名也可能是变量名；`x.(T)` 的 `T` 要到 M3 才定），所以官方 `type_()`
//! 返回 `Expr`，Rust 侧同样**不设独立的 `TypeExpr`** —— `AST-24..32` 的类型节点是
//! [`Expr`] 的变体。

use crate::aux::{BasicLit, ChanDir, Field, Name, Op};
use crate::stmt::BlockStmt;
use rgoc_lex::pos::Pos;

/// 表达式和类型（同族）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Expr {
    /// 错误恢复占位（`AST-09`）。**类型错误也复用它**（类型属本族）。
    Bad(BadExpr),
    /// 标识符（`AST-10`）。
    Name(Name),
    /// 基本字面量（`AST-11`）。
    BasicLit(BasicLit),
    CompositeLit(CompositeLit),
    KeyValue(KeyValueExpr),
    FuncLit(FuncLit),
    Paren(ParenExpr),
    Selector(SelectorExpr),
    Index(IndexExpr),
    Slice(SliceExpr),
    TypeAssert(AssertExpr),
    TypeSwitchGuard(TypeSwitchGuard),
    /// 一元运算（`AST-21`）。
    Unary(Operation),
    /// 二元运算（`AST-21`）。
    Binary(Operation),
    Call(CallExpr),
    List(ListExpr),

    // ── 类型族（`AST-24..32`，落在 `Expr` 族） ──
    /// `AST-24` `[n]T` / `[...]T`（`len == None` → `[...]`）。
    ArrayType(ArrayType),
    /// `AST-25` `[]T`。
    SliceType(SliceType),
    /// `AST-26` `...T`。
    DotsType(DotsType),
    /// `AST-27` `struct{...}`。
    StructType(StructType),
    /// `AST-29` `interface{...}`。
    InterfaceType(InterfaceType),
    /// `AST-30` `func(...)...`。
    FuncType(FuncType),
    /// `AST-31` `map[K]V`。
    MapType(MapType),
    /// `AST-32` `chan T` / `chan<- T` / `<-chan T`。
    ChanType(ChanType),
}

/// 错误占位（`AST-09`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BadExpr {
    pub pos: Pos,
}

/// 复合字面量（`AST-12`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CompositeLit {
    /// `None` → 省略类型（如嵌套字面量 `{{1}}`）。
    pub type_: Option<Box<Expr>>,
    pub elems: Vec<Expr>,
    /// 右花括号位置（官方 `nodes.go:185`）。
    ///
    /// ⚠️ 官方还有一个 `NKeys int`（键值元素计数，`:184`）—— **刻意不建模**：
    /// 它只在 `complitexpr` 里对键值元素 `x.NKeys++`（`parser.go:1309`），
    /// 因此**恒等于** `elems` 里 `KeyValueExpr` 的个数，是**可推导**的冗余缓存。
    pub rbrace: Pos,
    pub pos: Pos,
}

/// 键值对（`AST-13`），用于 `map`/`struct` 字面量。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KeyValueExpr {
    pub key: Box<Expr>,
    pub value: Box<Expr>,
    pub pos: Pos,
}

/// 函数字面量（`AST-14`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FuncLit {
    pub type_: Box<Expr>,
    pub body: Box<BlockStmt>,
    pub pos: Pos,
}

/// 括号表达式（`AST-15`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ParenExpr {
    pub x: Box<Expr>,
    pub pos: Pos,
}

/// 选择器 `x.sel`（`AST-16`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SelectorExpr {
    pub x: Box<Expr>,
    pub sel: Name,
    pub pos: Pos,
}

/// 索引/实例化 `x[index]`（`AST-17`）。索引 or 泛型实例化的二义由 M3 消解。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IndexExpr {
    pub x: Box<Expr>,
    pub index: Box<Expr>,
    pub pos: Pos,
}

/// 切片 `x[low:high]` / `x[low:high:max]`（`AST-18`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SliceExpr {
    pub x: Box<Expr>,
    pub low: Option<Box<Expr>>,
    pub high: Option<Box<Expr>>,
    pub max: Option<Box<Expr>>,
    /// `true` → 三下标形式 `x[a:b:c]`。
    pub full: bool,
    pub pos: Pos,
}

/// 类型断言 `x.(T)` / `x.(type)`（`AST-19`）。`type_ == None` → `x.(type)`。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AssertExpr {
    pub x: Box<Expr>,
    pub type_: Option<Box<Expr>>,
    pub pos: Pos,
}

/// 类型 switch 守卫 `v := x.(type)` / `x.(type)`（`AST-20`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TypeSwitchGuard {
    pub lhs: Option<Name>,
    pub x: Box<Expr>,
    pub pos: Pos,
}

/// 一元/二元运算（`AST-21`）。一元时 `y == None`。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Operation {
    pub op: Op,
    pub x: Box<Expr>,
    pub y: Option<Box<Expr>>,
    pub pos: Pos,
}

/// 调用（`AST-22`）。类型转换语法同形。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CallExpr {
    pub fun: Box<Expr>,
    pub args: Vec<Expr>,
    /// 末参带 `...`。
    pub has_dots: bool,
    pub pos: Pos,
}

/// 列表（`AST-23`）。多返回值/泛型类型实参列表载体。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ListExpr {
    pub elems: Vec<Expr>,
    pub pos: Pos,
}

/// 数组类型（`AST-24`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ArrayType {
    /// `None` → `[...]T`。
    pub len: Option<Box<Expr>>,
    pub elem: Box<Expr>,
    pub pos: Pos,
}

/// 切片类型（`AST-25`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SliceType {
    pub elem: Box<Expr>,
    pub pos: Pos,
}

/// 变参类型 `...T`（`AST-26`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DotsType {
    pub elem: Box<Expr>,
    pub pos: Pos,
}

/// 结构体类型（`AST-27`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StructType {
    pub fields: Vec<Field>,
    pub pos: Pos,
}

/// 接口类型（`AST-29`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InterfaceType {
    /// 方法集 / 嵌入接口 / 类型约束元素。
    pub methods: Vec<Field>,
    pub pos: Pos,
}

/// 函数类型（`AST-30`）。
///
/// ⚠️ **没有 `tparams`**：官方 `funcType` 的签名是
/// `funcType(context string) ([]*Field, *FuncType)`（`parser.go:1433`）——
/// 类型参数列表被**返回给调用方**，由它挂到 `FuncDecl.TParamList` / `TypeDecl.TParamList`，
/// `FuncType` 自己完全没有这个字段。`rgoc_ast::FuncDecl::tparams` 才是它的家。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FuncType {
    pub params: Vec<Field>,
    pub results: Vec<Field>,
    pub pos: Pos,
}

/// map 类型（`AST-31`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MapType {
    pub key: Box<Expr>,
    pub value: Box<Expr>,
    pub pos: Pos,
}

/// 通道类型（`AST-32`）。`dir == None` → 双向 `chan T`。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChanType {
    pub dir: Option<ChanDir>,
    pub elem: Box<Expr>,
    pub pos: Pos,
}

impl Expr {
    /// 节点起点位置（官方每个节点都有 `Pos() Pos` 方法）。
    ///
    /// # 为什么现在才加
    ///
    /// M2 的大部分代码**不需要**它 —— dump 不输出位置（`M2-tests.md` §5.2 规则 4）。
    /// 但官方有两处**把位置当数据用**：
    ///
    /// ```text
    /// exprList  → l.pos = x.Pos()   // parser.go:2840
    /// typeList  → l.pos = x.Pos()   // parser.go:2877
    /// ```
    ///
    /// ⚠️ 那是**首元素自己的** `Pos()`，**不是**「解析它之前那个词元的位置」。
    /// 两者在 `a.b, c` 上**不同**：首元素是 `SelectorExpr`，它的 `Pos()` 是 **`.`**
    /// 而不是 `a`。拿「进元素前的 `tok.pos`」近似会在选择器/索引/切片上静默错位。
    ///
    /// 只在造 `ListExpr` 时用到，所以这里不加 `Decl`/`Stmt` 的同名方法 ——
    /// **按需加，不按对称感加**（用到时缺了会编译不过，不会漏）。
    pub fn pos(&self) -> Pos {
        match self {
            Expr::Bad(x) => x.pos,
            Expr::Name(x) => x.pos,
            Expr::BasicLit(x) => x.pos,
            Expr::CompositeLit(x) => x.pos,
            Expr::KeyValue(x) => x.pos,
            Expr::FuncLit(x) => x.pos,
            Expr::Paren(x) => x.pos,
            Expr::Selector(x) => x.pos,
            Expr::Index(x) => x.pos,
            Expr::Slice(x) => x.pos,
            Expr::TypeAssert(x) => x.pos,
            Expr::TypeSwitchGuard(x) => x.pos,
            Expr::Unary(x) | Expr::Binary(x) => x.pos,
            Expr::Call(x) => x.pos,
            Expr::List(x) => x.pos,
            Expr::ArrayType(x) => x.pos,
            Expr::SliceType(x) => x.pos,
            Expr::DotsType(x) => x.pos,
            Expr::StructType(x) => x.pos,
            Expr::InterfaceType(x) => x.pos,
            Expr::FuncType(x) => x.pos,
            Expr::MapType(x) => x.pos,
            Expr::ChanType(x) => x.pos,
        }
    }

    /// 官方 `positions.go` 的 **`StartPos`**（Expr 子集）：某些节点的「起点」
    /// 不是自己的存储位，而是**修正到子节点**（`CompositeLit`→Type、
    /// `KeyValue`→Key、`Selector`/`Index`/`Assert`→X、`TypeSwitchGuard`→Lhs/X、
    /// 二元 `Operation`→X、`CallExpr`→Fun、`ListExpr`→首元素）。
    ///
    /// # 为什么 `pos()` 之外还要有它
    ///
    /// 存储位（[`Self::pos`]）与官方 `Pos()` 同义；`StartPos` 是官方**另一套**
    /// 判据，两者在 `x[i]` / `a.b` / `f(x)` 上不同（存储位在 `[`/`.`/`(`，
    /// 起点在 `x`/`a`/`f`）。官方 `parser.go` **只有一处**用 `StartPos`
    /// （`paramList` 的 `errPos = StartPos(typ)`，`:2077`）—— `tparams.go` 的
    /// 对撞实测：用 `pos()` 报 `missing type parameter name` 会**右偏一列**。
    /// 与 `pos()` 同纪律：**按需加，不按对称感加**，别拿它到处替换 `pos()`。
    pub fn start_pos(&self) -> Pos {
        let mut cur = self;
        loop {
            match cur {
                Expr::CompositeLit(x) => match x.type_.as_deref() {
                    Some(t) => cur = t,
                    None => return x.pos,
                },
                Expr::KeyValue(x) => cur = &x.key,
                Expr::Selector(x) => cur = &x.x,
                Expr::Index(x) => cur = &x.x,
                Expr::TypeAssert(x) => cur = &x.x,
                Expr::TypeSwitchGuard(x) => match &x.lhs {
                    Some(n) => return n.pos,
                    None => cur = &x.x,
                },
                Expr::Binary(x) => cur = &x.x,
                Expr::Call(x) => cur = &x.fun,
                Expr::List(x) => match x.elems.first() {
                    Some(e) => cur = e,
                    None => return x.pos,
                },
                _ => return cur.pos(),
            }
        }
    }
}
