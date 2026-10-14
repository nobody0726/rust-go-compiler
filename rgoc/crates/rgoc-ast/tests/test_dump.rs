//! T-A-90 + RF5 —— 中立 dump 格式（C6 草案）的可执行副本。

use rgoc_ast::*;
use rgoc_lex::pos::{FileId, Pos};

fn pos(line: u32, col: u32) -> Pos {
    Pos {
        file: FileId::from_index(0),
        line,
        col,
    }
}

fn n(s: &str) -> Name {
    Name {
        value: s.to_string(),
        pos: pos(1, 1),
    }
}

fn empty_func_type() -> Expr {
    Expr::FuncType(FuncType {
        params: vec![],
        results: vec![],
        pos: pos(1, 1),
    })
}

fn file_with(decls: Vec<Decl>) -> AstFile {
    AstFile {
        pos: pos(1, 1),
        pragma: Pragma::default(),
        pkg_name: n("p"),
        decls,
        eof: pos(1, 1),
        go_version: None,
    }
}

#[test]
fn t_a_90_dump_simple_file() {
    let f = file_with(vec![Decl::Func(FuncDecl {
        recv: None,
        name: n("f"),
        tparams: vec![],
        type_: empty_func_type(),
        body: Some(BlockStmt {
            stmts: vec![],
            rbrace: pos(1, 2),
            pos: pos(1, 1),
        }),
        pragma: Pragma::default(),
        pos: pos(1, 1),
    })]);

    let s = dump(&f);
    assert!(s.starts_with("File\n"), "got:\n{s}");
    assert!(s.contains("PkgName: Name \"p\""), "got:\n{s}");
    assert!(s.contains("Decls: [1 entries]"), "got:\n{s}");
    assert!(s.contains("FuncDecl"), "got:\n{s}");
    assert!(s.contains("Name: Name \"f\""), "got:\n{s}");
    assert!(s.contains("FuncType"), "got:\n{s}");
    assert!(s.contains("Params: [0 entries]"), "got:\n{s}");
}

#[test]
fn t_a_90_optional_field_dash() {
    let f = file_with(vec![Decl::Func(FuncDecl {
        recv: None,
        name: n("f"),
        tparams: vec![],
        type_: empty_func_type(),
        body: None,
        pragma: Pragma::default(),
        pos: pos(1, 1),
    })]);

    let s = dump(&f);
    // 可选字段用 `-` 占位（§5.2 规则 3）
    assert!(s.contains("Recv: -"), "got:\n{s}");
    assert!(s.contains("Body: -"), "got:\n{s}");
}

#[test]
fn rf5_list_expr_flattened() {
    // §5.3 规范化规则：列表节点统一扁平化，不包裹 `ListExpr`
    let e = Expr::List(ListExpr {
        elems: vec![Expr::Name(n("a")), Expr::Name(n("b"))],
        pos: pos(1, 1),
    });
    let s = dump_expr(&e);
    assert!(!s.contains("ListExpr"), "ListExpr 应扁平化，got:\n{s}");
    assert!(s.contains("Name \"a\""), "got:\n{s}");
    assert!(s.contains("Name \"b\""), "got:\n{s}");
}

#[test]
fn t_a_90_binary_operation_unified() {
    // §5.3：Operation 统一输出；一元时 Y 打 `null`
    let binary = Expr::Binary(Operation {
        op: Op::Add,
        x: Box::new(Expr::Name(n("a"))),
        y: Some(Box::new(Expr::Name(n("b")))),
        pos: pos(1, 1),
    });
    let unary = Expr::Unary(Operation {
        op: Op::Sub,
        x: Box::new(Expr::Name(n("a"))),
        y: None,
        pos: pos(1, 1),
    });

    let bs = dump_expr(&binary);
    assert!(bs.contains("Operation"), "got:\n{bs}");
    assert!(bs.contains("Op: +"), "got:\n{bs}");

    let us = dump_expr(&unary);
    assert!(us.contains("Operation"), "got:\n{us}");
    assert!(us.contains("Y: null"), "got:\n{us}");
}

/// 结构体标签必须出现在 dump 里 —— 它是**真数据**，不是位置。
/// 没有这条断言，「标签被悄悄丢掉」在 dump 层是**看不出来**的。
#[test]
fn t_a_90_dump_struct_tag() {
    let field = |tag: Option<BasicLit>| Field {
        name: Some(n("X")),
        type_: Expr::Name(n("int")),
        tag,
        pos: pos(1, 1),
    };
    let tagged = Expr::StructType(StructType {
        fields: vec![field(Some(BasicLit {
            value: "`json:\"x\"`".to_string(),
            kind: LitKind::String,
            bad: false,
            pos: pos(1, 1),
        }))],
        pos: pos(1, 1),
    });
    let s = dump_expr(&tagged);
    assert!(
        s.contains("Tag: BasicLit(string)"),
        "标签必须出现在 dump 里，got:\n{s}"
    );

    let untagged = Expr::StructType(StructType {
        fields: vec![field(None)],
        pos: pos(1, 1),
    });
    let s2 = dump_expr(&untagged);
    assert!(
        s2.contains("Tag: -"),
        "无标签用 `-` 占位（规则 3），got:\n{s2}"
    );
}

/// `ConstDecl` 的多个名字要按列表输出（`const a, b = 1, 2`）。
#[test]
fn t_a_90_dump_const_names_list() {
    let f = file_with(vec![Decl::Const(ConstDecl {
        group: None,
        pragma: Pragma::default(),
        names: vec![n("a"), n("b")],
        type_: None,
        values: vec![],
        pos: pos(1, 1),
    })]);
    let s = dump(&f);
    assert!(s.contains("Names: [2 entries]"), "got:\n{s}");
}

/// `DeclStmt` 的声明列表（分组声明）。
#[test]
fn t_a_90_dump_decl_stmt_list() {
    let d = |name: &str| {
        Decl::Var(VarDecl {
            group: None,
            pragma: Pragma::default(),
            names: vec![n(name)],
            type_: None,
            values: vec![],
            pos: pos(1, 1),
        })
    };
    let e = Expr::FuncLit(FuncLit {
        type_: Box::new(empty_func_type()),
        body: Box::new(BlockStmt {
            stmts: vec![Stmt::Decl(DeclStmt {
                decls: vec![d("a"), d("b")],
                pos: pos(1, 1),
            })],
            rbrace: pos(1, 2),
            pos: pos(1, 1),
        }),
        pos: pos(1, 1),
    });
    let s = dump_expr(&e);
    assert!(s.contains("DeclStmt"), "got:\n{s}");
    assert!(s.contains("Decls: [2 entries]"), "got:\n{s}");
}

/// **反向**断言：`FuncType` **没有** tparams。
///
/// 官方 `funcType` 返回 `([]*Field, *FuncType)`（`parser.go:1433`）—— 类型参数
/// **不属于** `FuncType`，由调用方挂到 `FuncDecl.TParamList` / `TypeDecl.TParamList`。
/// 若将来有人把 `tparams` 加回 `FuncType`，这条立刻变红。
#[test]
fn rf6_func_type_has_no_tparams() {
    let s = dump_expr(&empty_func_type());
    assert!(!s.contains("TParams"), "FuncType 不该有 TParams，got:\n{s}");
}
