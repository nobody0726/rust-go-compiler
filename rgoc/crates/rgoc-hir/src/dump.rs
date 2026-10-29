//! HIR 的**中性 dump**（`--dump-hir` / `T-EX-04`）。
//!
//! # 格式（定死；改动等于契约变更）
//!
//! ```text
//! # program
//! # funcs: <N>
//! # globals: <N>
//! # init: <name>,<name>
//! # func <序号> <名字>
//! <depth>\t<kind>\t<detail>
//! ```
//!
//! * 每行三列，TAB 分隔；`depth` 从 0 起（函数体语句是 0，其子节点 +1）。
//! * `#` 行是头（人读 + 分段），不进对撞 —— 与 `C8` 的 `# file:` 同一条纪律。
//! * **确定性**：全部容器都是 `Vec` / `BTreeMap`，同一份 HIR 两次 dump 逐字节相同
//!   （`T-EX-04` 的两条断言）。
//! * 不打印 `TypeId` 的数字 —— 那是类型表的内部编号，跨进程不稳定；需要类型的
//!   地方打印**变体标签 + 下标**（`named#3` 这种），它在同一次 dump 内自洽。

use std::fmt::Write as _;

use crate::types::{
    BinOp, Block, BranchKind, BuiltinKind, Callee, CompositeItem, Expr, ExprKind, FuncId, MakeKind,
    MethodRef, Place, Program, RangeKind, SelectKind, Stmt, UnOp,
};

/// dump 一个程序。
#[must_use]
pub fn dump_program(p: &Program) -> String {
    let mut w = String::new();
    let _ = writeln!(w, "# program");
    let _ = writeln!(w, "# funcs: {}", p.funcs.len());
    let _ = writeln!(w, "# globals: {}", p.globals.len());
    let init: Vec<&str> = p
        .init_order
        .iter()
        .flat_map(|g| {
            g.globals
                .iter()
                .map(|id| id.map_or("_", |id| p.globals[id.index()].name.as_str()))
        })
        .collect();
    let _ = writeln!(w, "# init: {}", init.join(","));
    let inits: Vec<String> = p.inits.iter().map(|f| f.0.to_string()).collect();
    let _ = writeln!(w, "# inits: {}", inits.join(","));
    let _ = writeln!(
        w,
        "# main: {}",
        p.main.map_or_else(|| "-".to_string(), |f| f.0.to_string())
    );
    for (i, f) in p.funcs.iter().enumerate() {
        let _ = writeln!(w, "# func {i} {}", f.name);
        let _ = writeln!(
            w,
            "0\tFunc\tparams={} results={} slots={} variadic={} captured={}",
            f.params.len(),
            f.results.len(),
            f.nslots,
            u8::from(f.variadic),
            f.captured.len()
        );
        dump_block(&mut w, 1, &f.body);
    }
    w
}

fn line(w: &mut String, depth: usize, kind: &str, detail: impl std::fmt::Display) {
    let _ = writeln!(w, "{depth}\t{kind}\t{detail}");
}

fn dump_block(w: &mut String, depth: usize, b: &Block) {
    line(w, depth, "Block", format!("stmts={}", b.stmts.len()));
    for s in &b.stmts {
        dump_stmt(w, depth + 1, s);
    }
}

fn dump_stmt(w: &mut String, depth: usize, s: &Stmt) {
    match s {
        Stmt::Assign { lhs, op, rhs } => {
            line(
                w,
                depth,
                "Assign",
                format!(
                    "lhs={} rhs={} op={}",
                    lhs.len(),
                    rhs.len(),
                    op.map_or("-".to_string(), |o| format!("{o:?}"))
                ),
            );
            for p in lhs {
                dump_place(w, depth + 1, p);
            }
            for e in rhs {
                dump_expr(w, depth + 1, e);
            }
        }
        Stmt::ShortDecl { slots, rhs } => {
            line(
                w,
                depth,
                "ShortDecl",
                format!("slots={} rhs={}", slots.len(), rhs.len()),
            );
            for e in rhs {
                dump_expr(w, depth + 1, e);
            }
        }
        Stmt::Expr(e) => {
            line(w, depth, "ExprStmt", "");
            dump_expr(w, depth + 1, e);
        }
        Stmt::If {
            init,
            cond,
            then,
            els,
        } => {
            line(w, depth, "If", "");
            if let Some(i) = init {
                dump_stmt(w, depth + 1, i);
            }
            dump_expr(w, depth + 1, cond);
            dump_block(w, depth + 1, then);
            if let Some(e) = els {
                dump_stmt(w, depth + 1, e);
            }
        }
        Stmt::For {
            init,
            cond,
            post,
            body,
            label,
            // 只影响运行时的 `Cell` 生命周期，不改变语句结构 ⇒ 不进 dump
            // （dump 是对撞入口，多一个派生字段就多一处会漂的字节）。
            loopvars: _,
        } => {
            line(
                w,
                depth,
                "For",
                format!("label={}", label.as_deref().unwrap_or("-")),
            );
            if let Some(i) = init {
                dump_stmt(w, depth + 1, i);
            }
            if let Some(c) = cond {
                dump_expr(w, depth + 1, c);
            }
            if let Some(p) = post {
                dump_stmt(w, depth + 1, p);
            }
            dump_block(w, depth + 1, body);
        }
        Stmt::Range {
            key,
            value,
            x,
            body,
            kind,
            define,
            label,
        } => {
            line(
                w,
                depth,
                "Range",
                format!(
                    "kind={kind:?} key={} value={} define={} label={}",
                    u8::from(key.is_some()),
                    u8::from(value.is_some()),
                    u8::from(*define),
                    label.as_deref().unwrap_or("-")
                ),
            );
            dump_expr(w, depth + 1, x);
            if let Some(k) = key {
                dump_place(w, depth + 1, k);
            }
            if let Some(v) = value {
                dump_place(w, depth + 1, v);
            }
            dump_block(w, depth + 1, body);
        }
        Stmt::Switch {
            init,
            tag,
            cases,
            label,
        } => {
            line(
                w,
                depth,
                "Switch",
                format!(
                    "cases={} label={}",
                    cases.len(),
                    label.as_deref().unwrap_or("-")
                ),
            );
            if let Some(i) = init {
                dump_stmt(w, depth + 1, i);
            }
            if let Some(t) = tag {
                dump_expr(w, depth + 1, t);
            }
            for c in cases {
                line(
                    w,
                    depth + 1,
                    "Case",
                    format!("exprs={} default={}", c.exprs.len(), u8::from(c.is_default)),
                );
                for e in &c.exprs {
                    dump_expr(w, depth + 2, e);
                }
                dump_block(w, depth + 2, &c.body);
            }
        }
        Stmt::TypeSwitch {
            init,
            x,
            cases,
            label,
        } => {
            line(
                w,
                depth,
                "TypeSwitch",
                format!(
                    "cases={} label={}",
                    cases.len(),
                    label.as_deref().unwrap_or("-")
                ),
            );
            if let Some(i) = init {
                dump_stmt(w, depth + 1, i);
            }
            dump_expr(w, depth + 1, x);
            for c in cases {
                line(
                    w,
                    depth + 1,
                    "TypeCase",
                    format!(
                        "types={} default={} nil={} bind={}",
                        c.types.len(),
                        u8::from(c.is_default),
                        u8::from(c.is_nil),
                        u8::from(c.bind.is_some())
                    ),
                );
                dump_block(w, depth + 2, &c.body);
            }
        }
        Stmt::Return(es) => {
            line(w, depth, "Return", format!("n={}", es.len()));
            for e in es {
                dump_expr(w, depth + 1, e);
            }
        }
        Stmt::Branch { kind, label } => {
            let k = match kind {
                BranchKind::Break => "break",
                BranchKind::Continue => "continue",
                BranchKind::Goto => "goto",
                BranchKind::Fallthrough => "fallthrough",
            };
            line(
                w,
                depth,
                "Branch",
                format!("{k} {}", label.as_deref().unwrap_or("")),
            );
        }
        Stmt::Labeled { label, stmt } => {
            line(w, depth, "Labeled", label.clone());
            dump_stmt(w, depth + 1, stmt);
        }
        Stmt::Block(b) => dump_block(w, depth, b),
        Stmt::Defer { call } => {
            line(w, depth, "Defer", "");
            dump_expr(w, depth + 1, call);
        }
        Stmt::Go { call } => {
            line(w, depth, "Go", "");
            dump_expr(w, depth + 1, call);
        }
        Stmt::Send { chan, value } => {
            line(w, depth, "Send", "");
            dump_expr(w, depth + 1, chan);
            dump_expr(w, depth + 1, value);
        }
        Stmt::Select { clauses } => {
            line(w, depth, "Select", format!("clauses={}", clauses.len()));
            for (i, c) in clauses.iter().enumerate() {
                let kind = match c.kind {
                    SelectKind::Send => "send",
                    SelectKind::Recv => "recv",
                    SelectKind::Default => "default",
                };
                line(
                    w,
                    depth + 1,
                    "Clause",
                    format!(
                        "#{i} {kind} targets={} send={}",
                        c.recv_targets.len(),
                        c.send_value.is_some()
                    ),
                );
                if let Some(ch) = &c.chan {
                    dump_expr(w, depth + 2, ch);
                }
                if let Some(v) = &c.send_value {
                    dump_expr(w, depth + 2, v);
                }
                dump_block(w, depth + 2, &c.body);
            }
        }
        Stmt::Empty => line(w, depth, "Empty", ""),
    }
}

fn dump_place(w: &mut String, depth: usize, p: &Place) {
    match p {
        Place::Slot(s) => line(w, depth, "Place", format!("slot#{}", s.0)),
        Place::Global(g) => line(w, depth, "Place", format!("global#{}", g.0)),
        Place::Field { base, index } => {
            line(w, depth, "Place", format!("field#{index}"));
            dump_place(w, depth + 1, base);
        }
        Place::Index { base, index } => {
            line(w, depth, "Place", "index");
            dump_expr(w, depth + 1, base);
            dump_expr(w, depth + 1, index);
        }
        Place::Deref(e) => {
            line(w, depth, "Place", "deref");
            dump_expr(w, depth + 1, e);
        }
    }
}

fn callee_text(c: &Callee) -> String {
    match c {
        Callee::Func(f) => format!("func#{f}", f = f.0),
        Callee::Method { func, .. } => format!("method#{f}", f = func.0),
        Callee::Iface { name, .. } => format!("iface.{name}"),
        Callee::Value(_) => "value".to_string(),
        Callee::Builtin(b) => format!("builtin:{}", builtin_text(*b)),
    }
}

fn builtin_text(b: BuiltinKind) -> &'static str {
    match b {
        BuiltinKind::Len => "len",
        BuiltinKind::Cap => "cap",
        BuiltinKind::Append => "append",
        BuiltinKind::Copy => "copy",
        BuiltinKind::Delete => "delete",
        BuiltinKind::Clear => "clear",
        BuiltinKind::New => "new",
        BuiltinKind::Min => "min",
        BuiltinKind::Max => "max",
        BuiltinKind::Complex => "complex",
        BuiltinKind::Real => "real",
        BuiltinKind::Imag => "imag",
        BuiltinKind::Print => "print",
        BuiltinKind::Println => "println",
        BuiltinKind::Panic => "panic",
        BuiltinKind::Recover => "recover",
        BuiltinKind::Close => "close",
        BuiltinKind::ReflectCall => "rt_reflect_call",
    }
}

fn dump_expr(w: &mut String, depth: usize, e: &Expr) {
    match &e.kind {
        ExprKind::Const(c) => line(w, depth, "Expr", format!("const {c:?}")),
        ExprKind::Slot(s) => line(w, depth, "Expr", format!("slot#{}", s.0)),
        ExprKind::Global(g) => line(w, depth, "Expr", format!("global#{}", g.0)),
        ExprKind::Field { base, index } => {
            line(w, depth, "Expr", format!("field#{index}"));
            dump_expr(w, depth + 1, base);
        }
        ExprKind::Index {
            base,
            index,
            comma_ok,
        } => {
            line(
                w,
                depth,
                "Expr",
                if *comma_ok { "index comma_ok" } else { "index" },
            );
            dump_expr(w, depth + 1, base);
            dump_expr(w, depth + 1, index);
        }
        ExprKind::Slice { base, lo, hi, max } => {
            line(
                w,
                depth,
                "Expr",
                format!(
                    "slice lo={} hi={} max={}",
                    u8::from(lo.is_some()),
                    u8::from(hi.is_some()),
                    u8::from(max.is_some())
                ),
            );
            dump_expr(w, depth + 1, base);
            for p in [lo, hi, max].into_iter().flatten() {
                dump_expr(w, depth + 1, p);
            }
        }
        ExprKind::Deref(x) => {
            line(w, depth, "Expr", "deref");
            dump_expr(w, depth + 1, x);
        }
        ExprKind::Addr(p) => {
            line(w, depth, "Expr", "addr");
            dump_place(w, depth + 1, p);
        }
        ExprKind::AddrOf(x) => {
            line(w, depth, "Expr", "addr-of");
            dump_expr(w, depth + 1, x);
        }
        ExprKind::Unary { op, x } => {
            line(w, depth, "Expr", format!("unary {}", unop_text(*op)));
            dump_expr(w, depth + 1, x);
        }
        ExprKind::Binary { op, x, y } => {
            line(w, depth, "Expr", format!("binary {}", binop_text(*op)));
            dump_expr(w, depth + 1, x);
            dump_expr(w, depth + 1, y);
        }
        ExprKind::Call {
            callee,
            args,
            variadic,
            spread,
        } => {
            let mut head = format!(
                "call {} args={} dots={}",
                callee_text(callee),
                args.len(),
                u8::from(*variadic)
            );
            if *spread != 0 {
                head.push_str(&format!(" spread={spread}"));
            }
            line(w, depth, "Expr", head);
            match callee {
                Callee::Method { recv, .. } | Callee::Iface { recv, .. } => {
                    dump_expr(w, depth + 1, recv)
                }
                Callee::Value(v) => dump_expr(w, depth + 1, v),
                _ => {}
            }
            for a in args {
                dump_expr(w, depth + 1, a);
            }
        }
        ExprKind::Convert { to, x } => {
            line(w, depth, "Expr", format!("convert {}", to.index()));
            dump_expr(w, depth + 1, x);
        }
        ExprKind::Composite(c) => {
            line(
                w,
                depth,
                "Expr",
                format!("composite items={}", c.items.len()),
            );
            for it in &c.items {
                match it {
                    CompositeItem::Value(v) => dump_expr(w, depth + 1, v),
                    CompositeItem::Key(k, v) => {
                        line(w, depth + 1, "Item", "key");
                        dump_expr(w, depth + 2, k);
                        dump_expr(w, depth + 2, v);
                    }
                    CompositeItem::Field(i, v) => {
                        line(w, depth + 1, "Item", format!("field#{i}"));
                        dump_expr(w, depth + 2, v);
                    }
                    CompositeItem::Index(i, v) => {
                        line(w, depth + 1, "Item", format!("index#{i}"));
                        dump_expr(w, depth + 2, v);
                    }
                }
            }
        }
        ExprKind::FuncLit(f) => line(w, depth, "Expr", format!("funclit#{}", f.0)),
        ExprKind::Bound { recv, method } => {
            let m = match method {
                MethodRef::Func(f) => format!("func#{}", f.0),
                MethodRef::Iface(n) => format!("iface.{n}"),
            };
            line(w, depth, "Expr", format!("bound {m}"));
            dump_expr(w, depth + 1, recv);
        }
        ExprKind::TypeAssert { x, to, comma_ok } => {
            line(
                w,
                depth,
                "Expr",
                format!("assert {} ok={}", to.index(), u8::from(*comma_ok)),
            );
            dump_expr(w, depth + 1, x);
        }
        ExprKind::Make { kind, args } => {
            let k = match kind {
                MakeKind::Slice => "slice",
                MakeKind::Map => "map",
                MakeKind::Chan => "chan",
            };
            line(w, depth, "Expr", format!("make {k} args={}", args.len()));
            for a in args {
                dump_expr(w, depth + 1, a);
            }
        }
        ExprKind::New(t) => line(w, depth, "Expr", format!("new {}", t.index())),
        ExprKind::Builtin {
            kind,
            args,
            variadic,
        } => {
            line(
                w,
                depth,
                "Expr",
                format!(
                    "builtin {} args={}{}",
                    builtin_text(*kind),
                    args.len(),
                    if *variadic { " dots" } else { "" }
                ),
            );
            for a in args {
                dump_expr(w, depth + 1, a);
            }
        }
        ExprKind::Recv { ch, comma_ok } => {
            line(w, depth, "Expr", format!("recv comma_ok={comma_ok}"));
            dump_expr(w, depth + 1, ch);
        }
        ExprKind::Nil => line(w, depth, "Expr", "nil"),
    }
}

fn unop_text(o: UnOp) -> &'static str {
    match o {
        UnOp::Neg => "-",
        UnOp::Pos => "+",
        UnOp::Not => "!",
        UnOp::BitNot => "^",
        UnOp::Recv => "<-",
    }
}

fn binop_text(o: BinOp) -> &'static str {
    match o {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::And => "&",
        BinOp::Or => "|",
        BinOp::Xor => "^",
        BinOp::AndNot => "&^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::LAnd => "&&",
        BinOp::LOr => "||",
    }
}

/// `FuncId` 的 dump 文本（测试与调试用）。
#[must_use]
pub fn func_text(f: FuncId) -> String {
    format!("func#{}", f.0)
}

/// `RangeKind` 的 dump 文本。
#[must_use]
pub fn range_text(k: RangeKind) -> &'static str {
    match k {
        RangeKind::Array => "array",
        RangeKind::Slice => "slice",
        RangeKind::Str => "string",
        RangeKind::Map => "map",
        RangeKind::Int => "int",
        RangeKind::Chan => "chan",
        RangeKind::Func => "func",
    }
}
