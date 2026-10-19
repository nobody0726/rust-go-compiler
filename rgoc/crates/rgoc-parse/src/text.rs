//! 官方 `String()` 的**最小组件子集** —— 只为 `header` 的错误消息服务。
//!
//! # 为什么这里会有一个"渲染器"（而 `D-M2-7` 明明说"不做 printer"）
//!
//! 官方 `header`（`parser.go:2420-2449`）在「条件位上是赋值/发送语句」时报：
//!
//! ```text
//! cannot use assignment x = (y || z) as value
//! ```
//!
//! 那个 `%s` 是 `printer.go` 的 `String()`（一个 ~370 行的 Go 源码短格式打印器）。
//! `D-M2-7` 明确**不在 M2 做 printer**（设计文档 §2.2：官方 `printer.go` 22 KB，
//! 是独立能力，超出 M2 必需范围）—— 于是这条消息的 `%s` 在 M2 里**无法逐字复刻**。
//!
//! # 为什么还是写了一个，而不是留占位符
//!
//! 因为 `syntax/testdata` 里有**两条**用例把这条消息写成了**近乎精确**的期望：
//!
//! ```text
//! issue60599.go:8   if x /* ERROR cannot use assignment x = y as value */ = y {}
//! issue60599.go:9   if x || y /* ERROR cannot use assignment \(x || y\) = z as value */ = z {}
//! issue60599.go:10  if x /* ERROR cannot use assignment x = \(y || z\) as value */ = y || z {}
//! ```
//!
//! （`issue23385.go` 的两条用 `.*`，宽松。）留纯占位符会让这两条**必然对不上**。
//!
//! # 那"写一个"和"偷偷做 printer"的界线在哪
//!
//! 界线是**覆盖范围**，并且它必须**可观测**：
//!
//! - 本模块**只**实现 `header` 消息真正会走到的形态（见 [`expr_text`] 的覆盖清单），
//!   而不是"尽力而为地打印一切"。
//! - 覆盖不到的形态**不猜** —— 输出一个**显眼的**标记 [`UNRENDERED`]（`⟨…⟩`），
//!   绝不输出"看起来像对的"文本。这样：① 消息永远不撒谎；② 一旦真的走到未覆盖形态，
//!   对撞/errorcheck 会以"某行 diff"暴露它，而不是静默通过。
//!
//! # 归属
//!
//! 本子集是 **`T-AP13` 的临时产物**，已在 `M2-plan/progress.md` 的残留表里登记：
//! `T-AP16`（错误消息复刻）复核消息文本，`T-AP18`（对撞回路）用 testdata 的
//! `errorcheck` 反向确认覆盖是否够用。**完整 `printer.go` 不在 M2 范围**（`D-M2-7`）。

use rgoc_ast::aux::{AssignOp, Op};
use rgoc_ast::expr::Expr;
use rgoc_ast::stmt::{RangeClause, SendStmt, SimpleStmt, Stmt};
use rgoc_lex::pos::Pos;

/// 未覆盖形态的标记 —— **故意显眼**。
///
/// 不用空串（看起来像"没有这个字段"）、不用 `?`（看起来像"值未知"）、
/// 更不用任何"像 Go 代码"的东西（那才是真正的陷阱：一条错的但合理的消息，
/// 会让对撞里的一行 diff 看起来像"消息措辞差异"而不是"渲染器没覆盖"）。
pub(crate) const UNRENDERED: &str = "⟨…⟩";

/// 运算符的源码写法（官方 `token.String()` 的对应值）。
///
/// ⚠️ 与官方 `tokens.go` 的 `opString` 逐项对齐。写成穷尽 `match`（不写 `_` 兜底）：
/// 将来 `Op` 加变体会在**编译期**报错，而不是静默渲染成别的符号。
fn op_str(op: Op) -> &'static str {
    match op {
        Op::Add => "+",
        Op::Sub => "-",
        Op::Mul => "*",
        Op::Quo => "/",
        Op::Rem => "%",
        Op::And => "&",
        Op::Or => "|",
        Op::Xor => "^",
        Op::Shl => "<<",
        Op::Shr => ">>",
        Op::AndNot => "&^",
        Op::LAnd => "&&",
        Op::LOr => "||",
        Op::Not => "!",
        Op::Eql => "==",
        Op::Neq => "!=",
        Op::Lss => "<",
        Op::Leq => "<=",
        Op::Gtr => ">",
        Op::Geq => ">=",
        Op::Arrow => "<-",
        Op::Tilde => "~",
    }
}

/// 赋值运算符的源码写法。
fn assign_op_str(op: AssignOp) -> &'static str {
    match op {
        AssignOp::Assign => "=",
        AssignOp::Define => ":=",
        AssignOp::Add => "+=",
        AssignOp::Sub => "-=",
        AssignOp::Mul => "*=",
        AssignOp::Quo => "/=",
        AssignOp::Rem => "%=",
        AssignOp::And => "&=",
        AssignOp::Or => "|=",
        AssignOp::Xor => "^=",
        AssignOp::Shl => "<<=",
        AssignOp::Shr => ">>=",
        AssignOp::AndNot => "&^=",
        AssignOp::Inc => "++",
        AssignOp::Dec => "--",
    }
}

/// 表达式 → Go 短格式文本（官方 `printer.go` 的 `ShortForm`）。
///
/// # 覆盖清单（**就这些**，其余返回 [`UNRENDERED`]）
///
/// | 变体 | 输出 | 官方 `printNode` |
/// |---|---|---|
/// | `Name` | `x` | `p.writeString(n.Value)` |
/// | `BasicLit` | `42`（**原文**） | `p.writeString(n.Value)` |
/// | `Unary` | `!x` / `-x` / `<-ch` | `p.print(n.Op, n.X)` |
/// | `Binary` | `x \|\| y` | `p.print(n.X, blank, n.Op, blank, n.Y)` |
/// | `Paren` | `(x)` | `p.print(_Lparen, n.X, _Rparen)` |
/// | `Selector` | `x.sel` | `p.print(n.X, _Dot, n.Sel)` |
///
/// 为什么要那两组空白：官方用 `blank` 控制符**强制**一个空格，所以 `x||y` 渲染成
/// `x || y`。少了空格，`issue60599.go` 的期望（`\(x || y\)`）就对不上。
///
/// `pub(crate)` 是因为 [`crate::branches`] 要用它拼 `goto X jumps over
/// declaration of Y at Z`（官方 `branches.go:194`）—— 那个 `Y` 就是
/// `String(varName)`。**两份 `expr_text` 会让 `text.rs` 的边界逐渐模糊**。
pub(crate) fn expr_text(x: &Expr) -> String {
    match x {
        Expr::Name(n) => n.value.clone(),
        Expr::BasicLit(l) => l.value.clone(),
        Expr::Unary(o) => match o.y {
            // `y` 为 `Some` 的"一元"节点不存在（见 `Expr` 的拆分说明）——
            // 但**不**用 `unreachable!`：这是渲染路径，宁可退化成标记也不要炸。
            Some(_) => UNRENDERED.to_string(),
            None => format!("{}{}", op_str(o.op), expr_text(&o.x)),
        },
        Expr::Binary(o) => match &o.y {
            Some(y) => format!("{} {} {}", expr_text(&o.x), op_str(o.op), expr_text(y)),
            None => UNRENDERED.to_string(),
        },
        Expr::Paren(p) => format!("({})", expr_text(&p.x)),
        Expr::Selector(s) => format!("{}.{}", expr_text(&s.x), s.sel.value),
        _ => UNRENDERED.to_string(),
    }
}

/// 官方 `emphasize`（`parser.go:2457`）：顶层是**二元** `Operation` 就套一层括号。
///
/// ```text
/// emphasize(x)          = "x"
/// emphasize(x || y)     = "(x || y)"
/// emphasize(-x)         = "-x"        ← 一元 Operation 的 `Y == nil`，不套
/// ```
pub(crate) fn emphasize(x: &Expr) -> String {
    let s = expr_text(x);
    match x {
        Expr::Binary(o) if o.y.is_some() => format!("({s})"),
        _ => s,
    }
}

/// `Vec<Expr>` 版 [`emphasize`] —— 对应官方的「`emphasize(as.Lhs)`，而 `Lhs` 是**一个** `Expr`」。
///
/// # 单元素与多元素**行为不同**，不能写成"逐元素 emphasize 再 join"
///
/// 官方多值赋值时 `Lhs` 是一个 `ListExpr`。`emphasize` 判断的是
/// `x.(*Operation)` —— `ListExpr` **不是** `*Operation`，所以**不套括号**，
/// 随后 `String(ListExpr)` 逐元素打印（也不套）。于是：
///
/// ```text
/// lhs = [a]            → emphasize(a)                 → "a"
/// lhs = [x||y]         → emphasize(x||y)              → "(x || y)"
/// lhs = [x||y, p||q]   → String(ListExpr{x||y, p||q}) → "x || y, p || q"   ← 无括号!
/// ```
///
/// 写成"逐元素 emphasize 再 join"会把最后一行变成 `(x || y), (p || q)` ——
/// 多两个括号，而这条消息是要跟 testdata 的期望做**子串**匹配的。
pub(crate) fn emphasize_list(xs: &[Expr]) -> String {
    if xs.len() == 1 {
        emphasize(&xs[0])
    } else {
        expr_vec_text(xs)
    }
}

/// `Vec<Expr>` 的渲染 —— 我们的 `AssignStmt.lhs/rhs` 是 `Vec`（官方是**一个**
/// 表达式，多值时是 `ListExpr`）。
///
/// 官方的 `ListExpr` 由 `printNode` 打印成 `a, b`（逗号 + 空格），
/// 所以这里用 `", "` 连接是**同形**的（C6 归一）。空表渲染成空串。
fn expr_vec_text(xs: &[Expr]) -> String {
    xs.iter().map(expr_text).collect::<Vec<_>>().join(", ")
}

fn send_text(s: &SendStmt) -> String {
    // 官方 `p.print(n.Chan, blank, _Arrow, blank, n.Value)`（`printer.go:540`）。
    format!("{} <- {}", expr_text(&s.chan), expr_text(&s.value))
}

fn range_text(r: &RangeClause) -> String {
    // 官方 `RangeClause` 只有落在 `simpleStmt` 的 cond 位才可能被 `String()`
    // （而 `header` 一见 RangeClause 就早退）⇒ 实际上**到不了这里**。
    // 仍然给一个形状正确的输出，而不是 `unreachable!`：渲染路径不炸。
    let lhs = expr_vec_text(&r.lhs);
    let sep = if r.is_define { ":=" } else { "=" };
    if lhs.is_empty() {
        format!("range {}", expr_text(&r.x))
    } else {
        format!("{lhs} {sep} range {}", expr_text(&r.x))
    }
}

/// 简单语句 → Go 短格式文本（官方 `String(s)`，`s` 是 `SimpleStmt`）。
///
/// 覆盖 `simpleStmt` 真正会产出的四种（`ExprStmt` / `SendStmt` / `AssignStmt` / `RangeClause`）。
pub(crate) fn simple_stmt_text(s: &SimpleStmt) -> String {
    match s {
        SimpleStmt::Expr(x) => expr_text(&x.x),
        SimpleStmt::Send(x) => send_text(x),
        // 官方 `printer.go:542-551`：`Rhs == nil` 时是 `++`/`--`（**不加空格**），
        // 否则 `Lhs <op>= Rhs`（两侧各一个空格）。
        SimpleStmt::Assign(a) => {
            let lhs = expr_vec_text(&a.lhs);
            if a.rhs.is_empty() {
                format!("{lhs}{}", assign_op_str(a.op))
            } else {
                format!("{lhs} {} {}", assign_op_str(a.op), expr_vec_text(&a.rhs))
            }
        }
        SimpleStmt::Range(r) => range_text(r),
        // 官方 `simpleStmt` **从不**产出 `EmptyStmt`（它只是 `SimpleStmt` 接口的
        // 实现之一，由 `stmtOrNil` 的 `_Semi` 臂产出）。这里给标记而不是 `unreachable!`：
        // 渲染路径不该把"某个不该出现的值"升级成崩溃。
        SimpleStmt::Empty => UNRENDERED.to_string(),
    }
}

/// 简单语句的节点位置（官方 `s.Pos()`，用在 `cannot use %s as value` 那条消息上）。
pub(crate) fn simple_stmt_pos(s: &SimpleStmt) -> Pos {
    match s {
        SimpleStmt::Expr(x) => x.pos,
        SimpleStmt::Send(x) => x.pos,
        SimpleStmt::Assign(x) => x.pos,
        SimpleStmt::Range(x) => x.pos,
        // 官方 `simpleStmt` **从不**产出 `EmptyStmt`。这里 `unreachable!` 而不是
        // 返回一个假 `Pos`：假位置会让"谁把 Empty 塞进来了"看起来像"位置算错了"，
        // 而这种"看起来像别的问题"正是 `AGENTS.md` 记的静默失败形状。
        SimpleStmt::Empty => {
            unreachable!("`simpleStmt` 不产出 EmptyStmt（官方 `:2703` 的 `_Semi` 臂才产出它）")
        }
    }
}

/// 语句的节点位置（官方 `s.Pos()`，用在 `block_branches` 的 `start` 参数上）。
///
/// 官方 `branches.go` 有三处要「一条语句的位置」：
///
/// - `:307` `innerBlock(inner, s.Else.Pos(), []Stmt{s.Else})`
/// - `:205` `v.Pos()`（`VarDecl`，走的是 `DeclStmt` 那条，不经这里）
/// - `:296` `recordVarDecl(s.Pos(), s.Lhs)`（`AssignStmt`，同上）
///
/// 我们的每个 `Stmt` 变体都带 `pos`（`PA-1` 建模时逐个对齐的）⇒ 一个
/// **穷尽 `match`** 就够，加变体时编译器会指到这里。
pub(crate) fn stmt_pos(s: &Stmt) -> Pos {
    match s {
        Stmt::Empty(x) => x.pos,
        Stmt::Labeled(x) => x.pos,
        Stmt::Block(x) => x.pos,
        Stmt::Expr(x) => x.pos,
        Stmt::Send(x) => x.pos,
        Stmt::Decl(x) => x.pos,
        Stmt::Assign(x) => x.pos,
        Stmt::Branch(x) => x.pos,
        Stmt::Call(x) => x.pos,
        Stmt::Return(x) => x.pos,
        Stmt::If(x) => x.pos,
        Stmt::For(x) => x.pos,
        Stmt::Switch(x) => x.pos,
        Stmt::Select(x) => x.pos,
    }
}

#[cfg(test)]
mod tests {
    use rgoc_ast::{
        AssignStmt, BasicLit, CallExpr, LitKind, Name, Op, Operation, ParenExpr, SelectorExpr,
        SendStmt,
    };
    use rgoc_lex::pos::FileId;

    use super::*;

    fn pos(line: u32, col: u32) -> Pos {
        Pos {
            base: None, // 无 `//line` 基准
            file: FileId::from_index(0),
            line,
            col,
        }
    }

    fn name(v: &str) -> Expr {
        Expr::Name(Name {
            value: v.to_string(),
            pos: pos(1, 1),
        })
    }

    fn bin(op: Op, x: Expr, y: Expr) -> Expr {
        Expr::Binary(Operation {
            op,
            x: Box::new(x),
            y: Some(Box::new(y)),
            pos: pos(1, 1),
        })
    }

    // ── `expr_text` 的覆盖清单（每条都要能被钉住）──────────────────────────

    /// 名字/字面量：**原文**直出（官方 `writeString(n.Value)`）。
    #[test]
    fn expr_text_名字与字面量取原文() {
        assert_eq!(expr_text(&name("foo")), "foo");
        let lit = Expr::BasicLit(BasicLit {
            value: "0x1f".to_string(),
            kind: LitKind::Int,
            bad: false,
            pos: pos(1, 1),
        });
        assert_eq!(expr_text(&lit), "0x1f", "存的是**原文**，不折叠成值");
    }

    /// 二元运算符两侧**各一个空格**（官方 `blank` 控制符）。
    ///
    /// ⚠️ 这条是 `issue60599.go` 能过的**直接原因**：期望里写的是 `\(x || y\)`。
    /// 少了空格 ⇒ 期望的**子串**（`x || y`）不存在 ⇒ 该用例静默变红。
    #[test]
    fn expr_text_二元运算符两侧有空格() {
        assert_eq!(expr_text(&bin(Op::LOr, name("x"), name("y"))), "x || y");
        assert_eq!(expr_text(&bin(Op::Mul, name("n"), name("m"))), "n * m");
    }

    /// 一元运算符**紧贴**操作数（官方 `p.print(n.Op, n.X)`，没有 `blank`）。
    #[test]
    fn expr_text_一元运算符不加空格() {
        let neg = Expr::Unary(Operation {
            op: Op::Sub,
            x: Box::new(name("x")),
            y: None,
            pos: pos(1, 1),
        });
        assert_eq!(expr_text(&neg), "-x");
    }

    /// 括号节点**保留**（官方 `ParenExpr` 有独立节点；`unparen` 是 `typeDecl` 的事，不在这里）。
    #[test]
    fn expr_text_括号保留() {
        let p = Expr::Paren(ParenExpr {
            x: Box::new(name("x")),
            pos: pos(1, 1),
        });
        assert_eq!(expr_text(&p), "(x)");
    }

    /// 选择器 `x.sel`。
    #[test]
    fn expr_text_选择器() {
        let s = Expr::Selector(SelectorExpr {
            x: Box::new(name("math")),
            sel: Name {
                value: "Pi".to_string(),
                pos: pos(1, 1),
            },
            pos: pos(1, 1),
        });
        assert_eq!(expr_text(&s), "math.Pi");
    }

    /// **未覆盖形态必须显眼**，不能输出"看起来像对的"东西。
    ///
    /// 这是本模块唯一的安全网：覆盖清单之外的一切都走 [`UNRENDERED`]。
    /// 反向断言同样重要 —— 若哪天有人"顺手"给 `Call` 加了个像样的渲染，
    /// 这条会红，提醒他登记 / 复核（而不是让它静默变成"更多覆盖"）。
    #[test]
    fn expr_text_未覆盖形态给显眼标记() {
        let call = Expr::Call(CallExpr {
            fun: Box::new(name("f")),
            args: vec![],
            has_dots: false,
            pos: pos(1, 1),
            rparen: pos(1, 3),
        });
        assert_eq!(expr_text(&call), UNRENDERED);
        assert_ne!(UNRENDERED, "", "标记不能是空串（空串看起来像「没有值」）");
    }

    // ── `emphasize`（官方 `parser.go:2457`）───────────────────────────────

    /// **只有顶层二元** `Operation` 才套括号。
    #[test]
    fn emphasize_只给顶层二元套括号() {
        assert_eq!(emphasize(&name("x")), "x");
        assert_eq!(emphasize(&bin(Op::LOr, name("x"), name("y"))), "(x || y)");
        let neg = Expr::Unary(Operation {
            op: Op::Sub,
            x: Box::new(name("x")),
            y: None,
            pos: pos(1, 1),
        });
        assert_eq!(emphasize(&neg), "-x", "一元 `Y == nil` ⇒ 不套");
    }

    /// 两端都套 —— `issue60599.go:9/10` 分别覆盖左右两侧的括号。
    #[test]
    fn emphasize_两侧各自独立() {
        let l = bin(Op::LOr, name("x"), name("y"));
        let r = bin(Op::LOr, name("y"), name("z"));
        assert_eq!(emphasize(&l), "(x || y)");
        assert_eq!(emphasize(&r), "(y || z)");
    }

    // ── 语句渲染 ────────────────────────────────────────────────────────

    /// `x++` / `x--`：**不加空格**（官方 `printer.go:547`）。
    #[test]
    fn assign_text_自增自减不加空格() {
        let inc = SimpleStmt::Assign(AssignStmt {
            op: AssignOp::Inc,
            lhs: vec![name("i")],
            rhs: vec![],
            pos: pos(1, 1),
        });
        assert_eq!(simple_stmt_text(&inc), "i++");
        let dec = SimpleStmt::Assign(AssignStmt {
            op: AssignOp::Dec,
            lhs: vec![name("i")],
            rhs: vec![],
            pos: pos(1, 1),
        });
        assert_eq!(simple_stmt_text(&dec), "i--");
    }

    /// `a += b` / `a := b`：两侧**各一个空格**。
    #[test]
    fn assign_text_复合赋值两侧空格() {
        let a = SimpleStmt::Assign(AssignStmt {
            op: AssignOp::Add,
            lhs: vec![name("a")],
            rhs: vec![name("b")],
            pos: pos(1, 1),
        });
        assert_eq!(simple_stmt_text(&a), "a += b");
        let d = SimpleStmt::Assign(AssignStmt {
            op: AssignOp::Define,
            lhs: vec![name("a")],
            rhs: vec![bin(Op::LOr, name("b"), name("c"))],
            pos: pos(1, 1),
        });
        assert_eq!(
            simple_stmt_text(&d),
            "a := b || c",
            "`String` 不套括号（`emphasize` 才套）"
        );
    }

    /// 多值用 `", "` 连接 —— 对应官方 `ListExpr` 的打印（C6 归一）。
    #[test]
    fn assign_text_多值用逗号空格连接() {
        let m = SimpleStmt::Assign(AssignStmt {
            op: AssignOp::Assign,
            lhs: vec![name("a"), name("b")],
            rhs: vec![name("c"), name("d")],
            pos: pos(1, 1),
        });
        assert_eq!(simple_stmt_text(&m), "a, b = c, d");
    }

    /// 发送语句两侧各一个空格（官方 `printer.go:540`）。
    #[test]
    fn send_text_箭头两侧空格() {
        let s = SimpleStmt::Send(SendStmt {
            chan: Box::new(name("ch")),
            value: Box::new(name("v")),
            pos: pos(1, 1),
        });
        assert_eq!(simple_stmt_text(&s), "ch <- v");
    }

    /// 运算符表**逐项**对官方 `tokens.go`（`Op` 24 个 / `AssignOp` 15 个）。
    ///
    /// ⚠️ 这条是"漏了不会报错"的克星：`op_str` 写成 `_ => ""` 时，
    /// 所有消息会静默少一个符号，而没有任何断言会红。
    #[test]
    fn 运算符表逐项对齐() {
        let ops: [(Op, &str); 22] = [
            (Op::Add, "+"),
            (Op::Sub, "-"),
            (Op::Mul, "*"),
            (Op::Quo, "/"),
            (Op::Rem, "%"),
            (Op::And, "&"),
            (Op::Or, "|"),
            (Op::Xor, "^"),
            (Op::Shl, "<<"),
            (Op::Shr, ">>"),
            (Op::AndNot, "&^"),
            (Op::LAnd, "&&"),
            (Op::LOr, "||"),
            (Op::Not, "!"),
            (Op::Eql, "=="),
            (Op::Neq, "!="),
            (Op::Lss, "<"),
            (Op::Leq, "<="),
            (Op::Gtr, ">"),
            (Op::Geq, ">="),
            (Op::Arrow, "<-"),
            (Op::Tilde, "~"),
        ];
        for (op, want) in ops {
            assert_eq!(op_str(op), want, "{op:?}");
        }

        let assigns: [(AssignOp, &str); 15] = [
            (AssignOp::Assign, "="),
            (AssignOp::Define, ":="),
            (AssignOp::Add, "+="),
            (AssignOp::Sub, "-="),
            (AssignOp::Mul, "*="),
            (AssignOp::Quo, "/="),
            (AssignOp::Rem, "%="),
            (AssignOp::And, "&="),
            (AssignOp::Or, "|="),
            (AssignOp::Xor, "^="),
            (AssignOp::Shl, "<<="),
            (AssignOp::Shr, ">>="),
            (AssignOp::AndNot, "&^="),
            (AssignOp::Inc, "++"),
            (AssignOp::Dec, "--"),
        ];
        for (op, want) in assigns {
            assert_eq!(assign_op_str(op), want, "{op:?}");
        }
    }
}
