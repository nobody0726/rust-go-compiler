//! 深度优先遍历（`T-A-06`）。用于统计节点数（结构不变式）。

use crate::aux::Field;
use crate::decl::{AstFile, Decl};
use crate::expr::{CallExpr, Expr};
use crate::stmt::{BlockStmt, SimpleStmt, Stmt};

/// 统计一棵 AST 的节点数（深度优先）。`File` 与 `PkgName` 各算一个节点。
pub fn count_nodes(f: &AstFile) -> usize {
    let mut n = 2; // File + PkgName
    for d in &f.decls {
        n += count_decl(d);
    }
    n
}

fn count_decl(d: &Decl) -> usize {
    match d {
        Decl::Import(x) => 1 + count_name_opt(&x.name) + usize::from(x.path.is_some()),
        Decl::Const(x) => {
            1 + x.names.len()
                + x.type_.as_ref().map_or(0, count_expr)
                + x.values.iter().map(count_expr).sum::<usize>()
        }
        Decl::Type(x) => {
            1 + 1 + x.tparams.iter().map(count_field).sum::<usize>() + count_expr(&x.type_)
        }
        Decl::Var(x) => {
            1 + x.names.len()
                + x.type_.as_ref().map_or(0, count_expr)
                + x.values.iter().map(count_expr).sum::<usize>()
        }
        Decl::Func(x) => {
            1 + x.recv.as_ref().map_or(0, count_field)
                + 1
                + x.tparams.iter().map(count_field).sum::<usize>()
                + count_expr(&x.type_)
                + x.body.as_ref().map_or(0, count_block)
        }
    }
}

fn count_name_opt(n: &Option<crate::aux::Name>) -> usize {
    usize::from(n.is_some())
}

fn count_field(f: &Field) -> usize {
    // 标签也是一个节点（`*BasicLit`），所以它算 1。
    1 + count_name_opt(&f.name) + count_expr(&f.type_) + usize::from(f.tag.is_some())
}

fn count_block(b: &BlockStmt) -> usize {
    1 + b.stmts.iter().map(count_stmt).sum::<usize>()
}

fn count_stmt(s: &Stmt) -> usize {
    match s {
        Stmt::Empty(_) => 1,
        Stmt::Labeled(x) => 1 + 1 + count_stmt(&x.stmt),
        Stmt::Block(b) => count_block(b),
        Stmt::Expr(x) => 1 + count_expr(&x.x),
        Stmt::Send(x) => 1 + count_expr(&x.chan) + count_expr(&x.value),
        Stmt::Decl(x) => 1 + x.decls.iter().map(count_decl).sum::<usize>(),
        Stmt::Assign(x) => {
            1 + x.lhs.iter().map(count_expr).sum::<usize>()
                + x.rhs.iter().map(count_expr).sum::<usize>()
        }
        Stmt::Branch(x) => 1 + count_name_opt(&x.label),
        Stmt::Call(x) => 1 + count_expr(&x.call),
        Stmt::Return(x) => 1 + x.results.iter().map(count_expr).sum::<usize>(),
        Stmt::If(x) => {
            1 + x.init.as_ref().map_or(0, count_simple)
                + x.cond.as_ref().map_or(0, count_expr)
                + count_block(&x.then)
                + x.els.as_ref().map_or(0, |e| count_stmt(e))
        }
        Stmt::For(x) => {
            1 + x.init.as_ref().map_or(0, count_simple)
                + x.cond.as_ref().map_or(0, count_expr)
                + x.post.as_ref().map_or(0, count_simple)
                + count_block(&x.body)
        }
        Stmt::Switch(x) => {
            1 + x.init.as_ref().map_or(0, count_simple)
                + x.tag.as_ref().map_or(0, count_expr)
                + x.cases
                    .iter()
                    .map(|c| {
                        1 + c.cases.as_ref().map_or(0, count_expr)
                            + c.body.iter().map(count_stmt).sum::<usize>()
                    })
                    .sum::<usize>()
        }
        Stmt::Select(x) => {
            1 + x
                .clauses
                .iter()
                .map(|c| {
                    1 + c.comm.as_ref().map_or(0, count_simple)
                        + c.body.iter().map(count_stmt).sum::<usize>()
                })
                .sum::<usize>()
        }
    }
}

fn count_simple(s: &SimpleStmt) -> usize {
    match s {
        SimpleStmt::Empty => 1,
        SimpleStmt::Expr(x) => 1 + count_expr(&x.x),
        SimpleStmt::Send(x) => 1 + count_expr(&x.chan) + count_expr(&x.value),
        SimpleStmt::Assign(x) => {
            1 + x.lhs.iter().map(count_expr).sum::<usize>()
                + x.rhs.iter().map(count_expr).sum::<usize>()
        }
        SimpleStmt::Range(x) => 1 + x.lhs.iter().map(count_expr).sum::<usize>() + count_expr(&x.x),
    }
}

fn count_call(c: &CallExpr) -> usize {
    count_expr(&c.fun) + c.args.iter().map(count_expr).sum::<usize>()
}

fn count_expr(e: &Expr) -> usize {
    match e {
        Expr::Bad(_) => 1,
        Expr::Name(_) => 1,
        Expr::BasicLit(_) => 1,
        Expr::CompositeLit(x) => {
            1 + x.type_.as_deref().map_or(0, count_expr)
                + x.elems.iter().map(count_expr).sum::<usize>()
        }
        Expr::KeyValue(x) => 1 + count_expr(&x.key) + count_expr(&x.value),
        Expr::FuncLit(x) => 1 + count_expr(&x.type_) + count_block(&x.body),
        Expr::Paren(x) => 1 + count_expr(&x.x),
        Expr::Selector(x) => 1 + count_expr(&x.x) + 1,
        Expr::Index(x) => 1 + count_expr(&x.x) + count_expr(&x.index),
        Expr::Slice(x) => {
            1 + count_expr(&x.x)
                + x.low.as_deref().map_or(0, count_expr)
                + x.high.as_deref().map_or(0, count_expr)
                + x.max.as_deref().map_or(0, count_expr)
        }
        Expr::TypeAssert(x) => 1 + count_expr(&x.x) + x.type_.as_deref().map_or(0, count_expr),
        Expr::TypeSwitchGuard(x) => 1 + count_name_opt(&x.lhs) + count_expr(&x.x),
        Expr::Unary(x) | Expr::Binary(x) => {
            1 + count_expr(&x.x) + x.y.as_deref().map_or(0, count_expr)
        }
        Expr::Call(x) => 1 + count_call(x),
        Expr::List(x) => 1 + x.elems.iter().map(count_expr).sum::<usize>(),
        Expr::ArrayType(x) => 1 + x.len.as_deref().map_or(0, count_expr) + count_expr(&x.elem),
        Expr::SliceType(x) => 1 + count_expr(&x.elem),
        Expr::DotsType(x) => 1 + count_expr(&x.elem),
        Expr::StructType(x) => 1 + x.fields.iter().map(count_field).sum::<usize>(),
        Expr::InterfaceType(x) => 1 + x.methods.iter().map(count_field).sum::<usize>(),
        Expr::FuncType(x) => {
            1 + x.params.iter().map(count_field).sum::<usize>()
                + x.results.iter().map(count_field).sum::<usize>()
        }
        Expr::MapType(x) => 1 + count_expr(&x.key) + count_expr(&x.value),
        Expr::ChanType(x) => 1 + count_expr(&x.elem),
    }
}
