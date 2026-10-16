//! 中立 dump 器（`D-M2-5` / C6 草案）。
//!
//! 把 AST 渲染成**与 oracle 侧逐字节可 diff** 的中立缩进文本。
//! 五条格式规则 + 三条规范化规则见 `docs/milestones/M2-tests.md` §5.2/§5.3。
//!
//! | 规则 | 内容 |
//! |---|---|
//! | 1 | 节点名用中立名（`ForStmt`/`BinaryExpr`/`ArrayType`…） |
//! | 2 | 列表显式标 `[n entries]` |
//! | 3 | 可选字段用 `-` 占位 |
//! | 4 | 位置默认不 dump |
//! | 5 | 字面量只 dump 原文（值 M3 折叠） |
//!
//! 规范化（§5.3）：`Operation` 统一输出（一元 `Y: null`）；`ListExpr` 扁平化；
//! `AssignStmt` 的 `IncDec` 显式标注。

use crate::aux::{BasicLit, ChanDir, Field, LitKind, Name, Op};
use crate::decl::{AstFile, Decl};
use crate::expr::Expr;
use crate::stmt::{SimpleStmt, Stmt};

/// 渲染整个文件。
pub fn dump(f: &AstFile) -> String {
    let mut d = Dumper {
        out: String::new(),
        indent: 0,
    };
    d.file(f);
    d.out
}

/// 渲染单个表达式（调试与单测用）。
pub fn dump_expr(e: &Expr) -> String {
    let mut d = Dumper {
        out: String::new(),
        indent: 0,
    };
    d.expr(e);
    d.out
}

struct Dumper {
    out: String,
    indent: usize,
}

impl Dumper {
    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn opt_dump<T>(&mut self, label: &str, v: &Option<T>, f: impl Fn(&mut Self, &T)) {
        match v {
            Some(x) => {
                self.line(&format!("{label}:"));
                self.indent += 1;
                f(self, x);
                self.indent -= 1;
            }
            None => self.line(&format!("{label}: -")),
        }
    }

    fn list<T>(&mut self, label: &str, items: &[T], f: impl Fn(&mut Self, &T)) {
        self.line(&format!("{label}: [{} entries]", items.len()));
        self.indent += 1;
        for it in items {
            f(self, it);
        }
        self.indent -= 1;
    }

    fn file(&mut self, f: &AstFile) {
        self.line("File");
        self.indent += 1;
        self.line(&format!("PkgName: {}", name_str(&f.pkg_name)));
        self.line(&format!(
            "GoVersion: {}",
            f.go_version.as_deref().unwrap_or("-")
        ));
        self.list("Decls", &f.decls, |d, x| d.decl(x));
        self.indent -= 1;
    }

    fn decl(&mut self, d: &Decl) {
        match d {
            Decl::Import(x) => {
                self.line("ImportDecl");
                self.indent += 1;
                self.line(&format!("Name: {}", opt_name_str(&x.name)));
                self.line(&format!("Path: {}", opt_lit_str(&x.path)));
                self.indent -= 1;
            }
            Decl::Const(x) => {
                self.line("ConstDecl");
                self.indent += 1;
                self.list("Names", &x.names, |d, nm| d.line(&name_str(nm)));
                self.opt_expr_field("Type", &x.type_);
                self.list("Values", &x.values, |d, e| d.expr(e));
                self.indent -= 1;
            }
            Decl::Type(x) => {
                self.line("TypeDecl");
                self.indent += 1;
                self.line(&format!("Name: {}", name_str(&x.name)));
                self.line(&format!("Alias: {}", x.alias));
                self.list("TParams", &x.tparams, |d, f| d.dump_field(f));
                self.expr_field("Type", &x.type_);
                self.indent -= 1;
            }
            Decl::Var(x) => {
                self.line("VarDecl");
                self.indent += 1;
                self.list("Names", &x.names, |d, n| d.line(&name_str(n)));
                self.opt_expr_field("Type", &x.type_);
                self.list("Values", &x.values, |d, e| d.expr(e));
                self.indent -= 1;
            }
            Decl::Func(x) => {
                self.line("FuncDecl");
                self.indent += 1;
                self.opt_dump("Recv", &x.recv, |d, f| d.dump_field(f));
                self.line(&format!("Name: {}", name_str(&x.name)));
                self.list("TParams", &x.tparams, |d, f| d.dump_field(f));
                self.expr_field("Type", &x.type_);
                self.opt_dump("Body", &x.body, |d, b| d.block(b));
                self.indent -= 1;
            }
        }
    }

    fn dump_field(&mut self, f: &Field) {
        self.line("Field");
        self.indent += 1;
        self.line(&format!("Name: {}", opt_name_str(&f.name)));
        self.expr_field("Type", &f.type_);
        // 结构体标签是**真数据**（不是位置），必须 dump；无标签用 `-` 占位（规则 3）。
        // 官方的 `TagList` 是平行数组，C6 对撞时按位 zip 到这里。
        match &f.tag {
            Some(t) => self.line(&format!("Tag: {}", lit_str(t))),
            None => self.line("Tag: -"),
        }
        self.indent -= 1;
    }

    fn opt_expr_field(&mut self, label: &str, e: &Option<Expr>) {
        match e {
            Some(x) => self.expr_field(label, x),
            None => self.line(&format!("{label}: -")),
        }
    }

    fn expr_field(&mut self, label: &str, e: &Expr) {
        if let Some(inline) = inline_expr(e) {
            self.line(&format!("{label}: {inline}"));
        } else {
            self.line(&format!("{label}:"));
            self.indent += 1;
            self.expr(e);
            self.indent -= 1;
        }
    }

    fn block(&mut self, b: &crate::stmt::BlockStmt) {
        self.line("BlockStmt");
        self.indent += 1;
        self.list("Stmts", &b.stmts, |d, s| d.stmt(s));
        self.indent -= 1;
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Empty(_) => self.line("EmptyStmt"),
            Stmt::Labeled(x) => {
                self.line("LabeledStmt");
                self.indent += 1;
                self.line(&format!("Label: {}", name_str(&x.label)));
                self.line("Stmt:");
                self.indent += 1;
                self.stmt(&x.stmt);
                self.indent -= 1;
                self.indent -= 1;
            }
            Stmt::Block(b) => self.block(b),
            Stmt::Expr(x) => {
                self.line("ExprStmt");
                self.indent += 1;
                self.expr_field("X", &x.x);
                self.indent -= 1;
            }
            Stmt::Send(x) => {
                self.line("SendStmt");
                self.indent += 1;
                self.expr_field("Chan", &x.chan);
                self.expr_field("Value", &x.value);
                self.indent -= 1;
            }
            Stmt::Decl(x) => {
                self.line("DeclStmt");
                self.indent += 1;
                self.list("Decls", &x.decls, |d, x| d.decl(x));
                self.indent -= 1;
            }
            Stmt::Assign(x) => {
                self.line("AssignStmt");
                self.indent += 1;
                self.line(&format!("Op: {}", assign_op_str(x.op)));
                self.list("Lhs", &x.lhs, |d, e| d.expr(e));
                self.list("Rhs", &x.rhs, |d, e| d.expr(e));
                self.indent -= 1;
            }
            Stmt::Branch(x) => {
                self.line("BranchStmt");
                self.indent += 1;
                self.line(&format!("Tok: {}", branch_tok_str(x.tok)));
                self.line(&format!("Label: {}", opt_name_str(&x.label)));
                self.indent -= 1;
            }
            Stmt::Call(x) => {
                self.line("CallStmt");
                self.indent += 1;
                self.line(&format!("Tok: {}", call_tok_str(x.tok)));
                self.expr_field("Call", &x.call);
                self.indent -= 1;
            }
            Stmt::Return(x) => {
                self.line("ReturnStmt");
                self.indent += 1;
                self.list("Results", &x.results, |d, e| d.expr(e));
                self.indent -= 1;
            }
            Stmt::If(x) => {
                self.line("IfStmt");
                self.indent += 1;
                self.opt_simple("Init", &x.init);
                self.opt_expr_field("Cond", &x.cond);
                self.line("Then:");
                self.indent += 1;
                self.block(&x.then);
                self.indent -= 1;
                self.opt_dump("Else", &x.els, |d, s| d.stmt(s));
                self.indent -= 1;
            }
            Stmt::For(x) => {
                self.line("ForStmt");
                self.indent += 1;
                self.opt_simple("Init", &x.init);
                self.opt_expr_field("Cond", &x.cond);
                self.opt_simple("Post", &x.post);
                self.line("Body:");
                self.indent += 1;
                self.block(&x.body);
                self.indent -= 1;
                self.indent -= 1;
            }
            Stmt::Switch(x) => {
                self.line("SwitchStmt");
                self.indent += 1;
                self.opt_simple("Init", &x.init);
                self.opt_expr_field("Tag", &x.tag);
                self.list("Cases", &x.cases, |d, c| {
                    d.line("CaseClause");
                    d.indent += 1;
                    d.opt_expr_field("Cases", &c.cases);
                    d.list("Body", &c.body, |d, s| d.stmt(s));
                    d.indent -= 1;
                });
                self.indent -= 1;
            }
            Stmt::Select(x) => {
                self.line("SelectStmt");
                self.indent += 1;
                self.list("Clauses", &x.clauses, |d, c| {
                    d.line("CommClause");
                    d.indent += 1;
                    d.opt_simple("Comm", &c.comm);
                    d.list("Body", &c.body, |d, s| d.stmt(s));
                    d.indent -= 1;
                });
                self.indent -= 1;
            }
        }
    }

    fn opt_simple(&mut self, label: &str, s: &Option<SimpleStmt>) {
        match s {
            Some(x) => {
                self.line(&format!("{label}:"));
                self.indent += 1;
                self.simple(x);
                self.indent -= 1;
            }
            None => self.line(&format!("{label}: -")),
        }
    }

    fn simple(&mut self, s: &SimpleStmt) {
        match s {
            SimpleStmt::Empty => self.line("EmptyStmt"),
            SimpleStmt::Expr(x) => {
                self.line("ExprStmt");
                self.indent += 1;
                self.expr_field("X", &x.x);
                self.indent -= 1;
            }
            SimpleStmt::Send(x) => {
                self.line("SendStmt");
                self.indent += 1;
                self.expr_field("Chan", &x.chan);
                self.expr_field("Value", &x.value);
                self.indent -= 1;
            }
            SimpleStmt::Assign(x) => {
                self.line("AssignStmt");
                self.indent += 1;
                self.line(&format!("Op: {}", assign_op_str(x.op)));
                self.list("Lhs", &x.lhs, |d, e| d.expr(e));
                self.list("Rhs", &x.rhs, |d, e| d.expr(e));
                self.indent -= 1;
            }
            SimpleStmt::Range(x) => {
                self.line("RangeClause");
                self.indent += 1;
                self.line(&format!("Define: {}", x.is_define));
                self.list("Lhs", &x.lhs, |d, e| d.expr(e));
                self.expr_field("X", &x.x);
                self.indent -= 1;
            }
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Bad(_) => self.line("BadExpr"),
            Expr::Name(n) => self.line(&name_str(n)),
            Expr::BasicLit(l) => self.line(&lit_str(l)),
            // §5.3：ListExpr 扁平化（不出现 `ListExpr` 行，直接展开元素）
            Expr::List(x) => {
                for el in &x.elems {
                    self.expr(el);
                }
            }
            Expr::CompositeLit(x) => {
                self.line("CompositeLit");
                self.indent += 1;
                self.opt_expr_field("Type", &x.type_.as_deref().cloned());
                self.list("Elems", &x.elems, |d, e| d.expr(e));
                self.indent -= 1;
            }
            Expr::KeyValue(x) => {
                self.line("KeyValueExpr");
                self.indent += 1;
                self.expr_field("Key", &x.key);
                self.expr_field("Value", &x.value);
                self.indent -= 1;
            }
            Expr::FuncLit(x) => {
                self.line("FuncLit");
                self.indent += 1;
                self.expr_field("Type", &x.type_);
                self.line("Body:");
                self.indent += 1;
                self.block(&x.body);
                self.indent -= 1;
                self.indent -= 1;
            }
            Expr::Paren(x) => {
                self.line("ParenExpr");
                self.indent += 1;
                self.expr_field("X", &x.x);
                self.indent -= 1;
            }
            Expr::Selector(x) => {
                self.line("SelectorExpr");
                self.indent += 1;
                self.expr_field("X", &x.x);
                self.line(&format!("Sel: {}", name_str(&x.sel)));
                self.indent -= 1;
            }
            Expr::Index(x) => {
                self.line("IndexExpr");
                self.indent += 1;
                self.expr_field("X", &x.x);
                self.expr_field("Index", &x.index);
                self.indent -= 1;
            }
            Expr::Slice(x) => {
                self.line("SliceExpr");
                self.indent += 1;
                self.expr_field("X", &x.x);
                self.opt_expr_field("Low", &x.low.as_deref().cloned());
                self.opt_expr_field("High", &x.high.as_deref().cloned());
                self.opt_expr_field("Max", &x.max.as_deref().cloned());
                self.indent -= 1;
            }
            Expr::TypeAssert(x) => {
                self.line("AssertExpr");
                self.indent += 1;
                self.expr_field("X", &x.x);
                self.opt_expr_field("Type", &x.type_.as_deref().cloned());
                self.indent -= 1;
            }
            Expr::TypeSwitchGuard(x) => {
                self.line("TypeSwitchGuard");
                self.indent += 1;
                self.line(&format!("Lhs: {}", opt_name_str(&x.lhs)));
                self.expr_field("X", &x.x);
                self.indent -= 1;
            }
            // §5.3：Operation 统一输出；一元时 `Y: null`
            Expr::Unary(x) | Expr::Binary(x) => {
                self.line("Operation");
                self.indent += 1;
                self.line(&format!("Op: {}", op_str(x.op)));
                self.expr_field("X", &x.x);
                match &x.y {
                    Some(y) => self.expr_field("Y", y),
                    None => self.line("Y: null"),
                }
                self.indent -= 1;
            }
            Expr::Call(x) => {
                self.line("CallExpr");
                self.indent += 1;
                self.expr_field("Fun", &x.fun);
                self.list("Args", &x.args, |d, e| d.expr(e));
                self.line(&format!("HasDots: {}", x.has_dots));
                self.indent -= 1;
            }
            Expr::ArrayType(x) => {
                self.line("ArrayType");
                self.indent += 1;
                self.opt_expr_field("Len", &x.len.as_deref().cloned());
                self.expr_field("Elem", &x.elem);
                self.indent -= 1;
            }
            Expr::SliceType(x) => {
                self.line("SliceType");
                self.indent += 1;
                self.expr_field("Elem", &x.elem);
                self.indent -= 1;
            }
            Expr::DotsType(x) => {
                self.line("DotsType");
                self.indent += 1;
                self.expr_field("Elem", &x.elem);
                self.indent -= 1;
            }
            Expr::StructType(x) => {
                self.line("StructType");
                self.indent += 1;
                self.list("Fields", &x.fields, |d, f| d.dump_field(f));
                self.indent -= 1;
            }
            Expr::InterfaceType(x) => {
                self.line("InterfaceType");
                self.indent += 1;
                self.list("Methods", &x.methods, |d, f| d.dump_field(f));
                self.indent -= 1;
            }
            Expr::FuncType(x) => {
                self.line("FuncType");
                self.indent += 1;
                // 注意：**没有 TParams** —— 类型参数属于 `FuncDecl`/`TypeDecl`，
                // 不属于 `FuncType`（官方 `funcType` 把它们返回给调用方）。
                self.list("Params", &x.params, |d, f| d.dump_field(f));
                self.list("Results", &x.results, |d, f| d.dump_field(f));
                self.indent -= 1;
            }
            Expr::MapType(x) => {
                self.line("MapType");
                self.indent += 1;
                self.expr_field("Key", &x.key);
                self.expr_field("Value", &x.value);
                self.indent -= 1;
            }
            Expr::ChanType(x) => {
                self.line("ChanType");
                self.indent += 1;
                self.line(&format!("Dir: {}", chan_dir_str(x.dir)));
                self.expr_field("Elem", &x.elem);
                self.indent -= 1;
            }
        }
    }
}

/// 简单节点的单行形式（供父节点内联）；复合节点返回 `None`。
fn inline_expr(e: &Expr) -> Option<String> {
    match e {
        Expr::Bad(_) => Some("BadExpr".to_string()),
        Expr::Name(n) => Some(name_str(n)),
        Expr::BasicLit(l) => Some(lit_str(l)),
        _ => None,
    }
}

fn name_str(n: &Name) -> String {
    format!("Name {:?}", n.value)
}

fn opt_name_str(n: &Option<Name>) -> String {
    match n {
        Some(x) => name_str(x),
        None => "-".to_string(),
    }
}

fn lit_str(l: &BasicLit) -> String {
    let k = match l.kind {
        LitKind::Int => "int",
        LitKind::Float => "float",
        LitKind::Imag => "imag",
        LitKind::Rune => "rune",
        LitKind::String => "string",
    };
    format!("BasicLit({k}) {:?}", l.value)
}

/// 可选字面量：缺席用 `-`（规则 3）。与 [`opt_name_str`] 同形。
///
/// ⚠️ **不能用哨兵字面量顶替** —— 那会让「缺路径」在 dump 里消失，
/// 而官方那边是 `nil`（`nodes.go:68`）。
fn opt_lit_str(l: &Option<BasicLit>) -> String {
    match l {
        Some(x) => lit_str(x),
        None => "-".to_string(),
    }
}

/// 运算符的拼写（`+`、`<<`、`&^` …）。
///
/// 公开是**刻意的**：诊断消息里要按人读的方式写出表达式（`H + 1 is not
/// constant`），而运算符表是**语言事实** —— 复制一份到别处就会多出一个
/// 会分叉的真相（`rgoc-load` 的 `expr_text` 直接用它）。
#[must_use]
pub fn op_str(op: Op) -> &'static str {
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

fn assign_op_str(op: crate::aux::AssignOp) -> &'static str {
    use crate::aux::AssignOp as A;
    match op {
        A::Assign => "=",
        A::Define => ":=",
        A::Add => "+=",
        A::Sub => "-=",
        A::Mul => "*=",
        A::Quo => "/=",
        A::Rem => "%=",
        A::And => "&=",
        A::Or => "|=",
        A::Xor => "^=",
        A::Shl => "<<=",
        A::Shr => ">>=",
        A::AndNot => "&^=",
        A::Inc => "++",
        A::Dec => "--",
    }
}

fn branch_tok_str(t: crate::aux::BranchTok) -> &'static str {
    use crate::aux::BranchTok as B;
    match t {
        B::Break => "break",
        B::Continue => "continue",
        B::Fallthrough => "fallthrough",
        B::Goto => "goto",
    }
}

fn call_tok_str(t: crate::aux::CallTok) -> &'static str {
    use crate::aux::CallTok as C;
    match t {
        C::Go => "go",
        C::Defer => "defer",
    }
}

fn chan_dir_str(d: Option<ChanDir>) -> &'static str {
    match d {
        None => "-",
        Some(ChanDir::SendOnly) => "send",
        Some(ChanDir::RecvOnly) => "recv",
    }
}
