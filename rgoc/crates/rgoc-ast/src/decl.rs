//! 声明族（`AST-01..08`）。

use crate::aux::{BasicLit, Field, Group, Name, Op, Pragma};
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
///
/// # 为什么对 `clippy::large_enum_variant` 用 `#[allow]`
///
/// 五个变体对应官方的**五个同名 struct**（`PA-1` 的建模对齐），大小差异
/// （实测 `FuncDecl` 408 字节 vs `TypeDecl` 200）是**建模的直接后果**，
/// 不是可以随手修掉的实现瑕疵。
///
/// 要消掉它只能 `Box` 某个字段（最小改动是 `type_: Box<Expr>`，省 64 字节），
/// 代价有两个，都与本仓的取舍相悖：
///
/// 1. **每个访问点多一层间接** —— `Decl::Func(f)` 后 `f.type_` 还得再解一次包；
/// 2. **偏离官方结构形状** —— 与 PA-1 建立的「按官方 `nodes.go` 逐字段对齐」冲突，
///    而那份对齐正是 `T-AP19` oracle 对撞能逐行 diff 的前提。
///
/// 内存代价本身可忽略：最大的形态是一个文件几十条声明 × 408 字节。
#[allow(clippy::large_enum_variant)]
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
    ///
    /// `None` 是官方 `nil means no path`（`nodes.go:68`）—— 触发条件是**合法源码**
    /// `import x`：官方拿到 `Path == nil` 后报一条 `missing import path`，但
    /// **照常返回这个 `ImportDecl`**（`parser.go:571-575`），所以 nil 的节点真会走到 dump。
    /// 换成不可空的 `BasicLit` 就只能编个哨兵字面量顶上，对撞必然分叉。
    pub path: Option<BasicLit>,
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

impl FuncDecl {
    /// 接收者的**基类型名**与形参名（见 [`recv_base`]）。
    #[must_use]
    pub fn recv_base(&self) -> Option<(String, Vec<String>)> {
        recv_base(&self.recv.as_ref()?.type_)
    }
}

/// 接收者**类型表达式**的基类型名与形参名：`*Box[T]` → `("Box", ["T"])`。
///
/// 非泛型接收者 `T` → `("T", [])`；不是「名字 + 可选下标」的形状
/// （错误恢复的 `Bad`、`[]T` 之类非法接收者）→ `None`（调用方各自报诊断）。
///
/// # 为什么单独抽出来（`M12/Task 18`）
///
/// 同一份判断有**五处**消费者，抽出来之前是**三份**各自的实现
/// （`rgoc-hir` 一份、`rgoc-check` 两份）：
/// * `rgoc-check::Session::new` —— `Mono` 的 `FuncKey` 登记（方法实例名要带
///   接收者）与「这个方法是不是泛型的」；
/// * `rgoc-check::stmt` —— 接收者声明的类型参数（`func (s T[P]) M()`）；
/// * `rgoc-check::initorder` —— 方法节点的键 `T.M`；
/// * `rgoc-hir` —— `Func::method_of` 与方法查找表 `(基类型名, 方法名)`；
/// * `rgoc-hir` —— 泛型方法的实例函数名。
///
/// 多份实现漂移的症状是「方法实例的名字与查表键差一个字符」—— 链接期报
/// `undefined reference`，或在更坏的情况下**安静地少一个方法**。
#[must_use]
pub fn recv_base(e: &Expr) -> Option<(String, Vec<String>)> {
    let mut cur = e;
    loop {
        cur = match cur {
            Expr::Paren(p) => &p.x,
            Expr::Unary(u) if u.op == Op::Mul && u.y.is_none() => &u.x,
            _ => break,
        };
    }
    match cur {
        Expr::Name(n) => Some((n.value.clone(), Vec::new())),
        Expr::Index(ix) => {
            let Expr::Name(base) = &*ix.x else {
                return None;
            };
            let mut names = Vec::new();
            match &*ix.index {
                Expr::Name(n) => names.push(n.value.clone()),
                Expr::List(l) => {
                    for el in &l.elems {
                        let Expr::Name(n) = el else {
                            return None;
                        };
                        names.push(n.value.clone());
                    }
                }
                _ => return None,
            }
            Some((base.value.clone(), names))
        }
        _ => None,
    }
}
