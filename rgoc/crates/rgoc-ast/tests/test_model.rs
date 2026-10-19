//! T-A-00..T-A-06 —— AST 数据模型与辅助类型的可编译副本。
//!
//! 本文件是 `M2-tests.md` §3.1（切面 ①）的可执行副本：
//! 每个断言对应一个 `T-A-0x` 测试 ID。

use rgoc_ast::*;
use rgoc_lex::pos::{FileId, Pos};

fn pos(line: u32, col: u32) -> Pos {
    Pos {
        base: None, // 无 `//line` 基准
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

#[test]
fn t_a_05_field_anonymous_and_named() {
    // 匿名字段：name = None
    let anon = Field {
        name: None,
        type_: Expr::Name(n("int")),
        tag: None,
        pos: pos(1, 1),
    };
    assert!(anon.name.is_none());

    // 命名字段
    let named = Field {
        name: Some(n("x")),
        type_: Expr::Name(n("int")),
        tag: None,
        pos: pos(1, 1),
    };
    assert_eq!(named.name.as_ref().unwrap().value, "x");
}

/// 结构体标签（官方 `StructType.TagList`，`nodes.go:293`）。
///
/// 官方把它做成**与 `FieldList` 下标对齐的平行数组**；我们内联进 `Field`。
/// 断言「标签装得下、且与字段同处一格」—— 这是**信息不丢失**的判据，
/// 单看「能编译」是看不出来的。
#[test]
fn t_a_05_field_tag_holds_struct_tag() {
    let tagged = Field {
        name: Some(n("X")),
        type_: Expr::Name(n("int")),
        tag: Some(BasicLit {
            value: "`json:\"x\"`".to_string(),
            kind: LitKind::String,
            bad: false,
            pos: pos(1, 1),
        }),
        pos: pos(1, 1),
    };
    let tag = tagged.tag.as_ref().expect("结构体字段的标签必须装得下");
    assert_eq!(tag.kind, LitKind::String);
    assert!(
        tag.value.contains("json"),
        "标签原文必须原样保留：{}",
        tag.value
    );

    // 非结构体字段（参数/结果/方法）恒为 None
    let param = Field {
        name: Some(n("x")),
        type_: Expr::Name(n("int")),
        tag: None,
        pos: pos(1, 1),
    };
    assert!(param.tag.is_none());
}

#[test]
fn t_a_05_chan_dir_values() {
    // F16：SendOnly=1、RecvOnly=2（0 表示无方向，用 Option::None）
    assert_eq!(ChanDir::SendOnly as u8, 1);
    assert_eq!(ChanDir::RecvOnly as u8, 2);
}

#[test]
fn t_a_05_comment_kind_values() {
    // F17：Above=0/Below/Left/Right
    assert_eq!(CommentKind::Above as u8, 0);
    assert_eq!(CommentKind::Below as u8, 1);
    assert_eq!(CommentKind::Left as u8, 2);
    assert_eq!(CommentKind::Right as u8, 3);
}

#[test]
fn t_a_05_group_and_comment() {
    let g = Group { open: pos(1, 1) };
    assert_eq!(g.open.line, 1);

    let c = Comment {
        kind: CommentKind::Above,
        text: "// hello".to_string(),
        next: Some(Box::new(Comment {
            kind: CommentKind::Below,
            text: "// world".to_string(),
            next: None,
        })),
    };
    assert_eq!(c.kind, CommentKind::Above);
    assert!(c.next.is_some());
}

#[test]
fn t_a_05_pragma_default_empty() {
    let p = Pragma::default();
    assert!(p.directives.is_empty());
}

/// `Group` 的**身份语义**（官方 `nodes.go:138-141`）。
///
/// 官方 `Group` 是**零字段**的：它存在的唯一目的是让「是不是同一组」可以用
/// **指针相等**判定（注释原话："All declarations belonging to the same group
/// point to the same Group node"）。Rust 没指针身份，我们用 `open` 当值身份。
///
/// ⚠️ 断言的重点是**反向**那半边：两个不同的组**必须不相等**。
/// 若 `Group` 退化成零字段（或 `PartialEq` 恒真），「同组」判别就失效 ——
/// 而**前向半边照样绿**（同组当然相等）。所以只写 `assert_eq!(a, b)` 是不够的。
#[test]
fn t_a_05_group_identity_is_value_based() {
    let a = Group { open: pos(1, 1) };
    let b = Group { open: pos(1, 1) };
    let c = Group { open: pos(5, 1) };

    assert_eq!(a, b, "同一个 `(` ⇒ 同一组");
    assert_ne!(a, c, "不同的 `(` ⇒ 不同组");
}

#[test]
fn t_a_00_ast_file() {
    let f = AstFile {
        pos: pos(1, 1),
        pragma: Pragma::default(),
        pkg_name: n("main"),
        decls: vec![],
        eof: pos(1, 20),
        go_version: None,
    };
    assert_eq!(f.pkg_name.value, "main");
    assert!(f.decls.is_empty());
    assert_eq!(f.eof.line, 1);
}

#[test]
fn t_a_01_decl_variants() {
    let lit = |s: &str| BasicLit {
        value: s.to_string(),
        kind: LitKind::String,
        bad: false,
        pos: pos(1, 1),
    };

    let import = Decl::Import(ImportDecl {
        group: None,
        pragma: Pragma::default(),
        name: None,
        path: Some(lit("\"math\"")),
        pos: pos(1, 1),
    });
    let konst = Decl::Const(ConstDecl {
        group: None,
        pragma: Pragma::default(),
        names: vec![n("K")],
        type_: None,
        values: vec![],
        pos: pos(1, 1),
    });
    let ty = Decl::Type(TypeDecl {
        group: None,
        pragma: Pragma::default(),
        name: n("T"),
        alias: false,
        tparams: vec![],
        type_: Expr::Name(n("int")),
        pos: pos(1, 1),
    });
    let var = Decl::Var(VarDecl {
        group: None,
        pragma: Pragma::default(),
        names: vec![n("v")],
        type_: None,
        values: vec![],
        pos: pos(1, 1),
    });
    let func = Decl::Func(FuncDecl {
        recv: None,
        name: n("f"),
        tparams: vec![],
        type_: Expr::FuncType(FuncType {
            params: vec![],
            results: vec![],
            pos: pos(1, 1),
        }),
        body: None,
        pragma: Pragma::default(),
        pos: pos(1, 1),
    });

    assert!(matches!(import, Decl::Import(_)));
    assert!(matches!(konst, Decl::Const(_)));
    assert!(matches!(ty, Decl::Type(_)));
    assert!(matches!(var, Decl::Var(_)));
    assert!(matches!(func, Decl::Func(_)));
}

/// `ConstDecl` 装的是**名字列表**（官方 `NameList []*Name`，`nodes.go:78`，
/// 由 `parser.go` 的 `nameList()` 收多个）—— `const a, b = 1, 2` 必须能装下。
///
/// 这条断言存在的理由：**单名字的模型编译也过**，只有真放两个名字进去才暴露。
#[test]
fn t_a_01_const_decl_names_is_a_list() {
    let c = ConstDecl {
        group: None,
        pragma: Pragma::default(),
        names: vec![n("a"), n("b")],
        type_: None,
        values: vec![Expr::Name(n("x"))],
        pos: pos(1, 1),
    };
    assert_eq!(
        c.names.len(),
        2,
        "`const a, b = ...` 的 NameList 必须能装两个"
    );
    assert_eq!(c.names[1].value, "b");
}

/// 四条声明的 `Pragma` 字段（官方 `nodes.go` `:66`/`:77`/`:88`/`:100` 各一个）。
/// 缺了它，挂在 `var`/`const`/`type`/`import` 上的 `//go:` 指令**无处可放**。
#[test]
fn t_a_01_decl_pragmas_exist() {
    let lit = BasicLit {
        value: "\"math\"".to_string(),
        kind: LitKind::String,
        bad: false,
        pos: pos(1, 1),
    };
    let mut p = Pragma::default();
    p.directives.push("noinline".to_string());

    let import = ImportDecl {
        group: None,
        pragma: p.clone(),
        name: None,
        path: Some(lit),
        pos: pos(1, 1),
    };
    let konst = ConstDecl {
        group: None,
        pragma: p.clone(),
        names: vec![n("K")],
        type_: None,
        values: vec![],
        pos: pos(1, 1),
    };
    let ty = TypeDecl {
        group: None,
        pragma: p.clone(),
        name: n("T"),
        alias: false,
        tparams: vec![],
        type_: Expr::Name(n("int")),
        pos: pos(1, 1),
    };
    let var = VarDecl {
        group: None,
        pragma: p.clone(),
        names: vec![n("v")],
        type_: None,
        values: vec![],
        pos: pos(1, 1),
    };

    for got in [
        import.pragma.directives.len(),
        konst.pragma.directives.len(),
        ty.pragma.directives.len(),
        var.pragma.directives.len(),
    ] {
        assert_eq!(got, 1, "四条声明的 Pragma 都必须真的装得下东西");
    }
}

#[test]
fn t_a_02_expr_variants() {
    // 表达式族
    let _name = Expr::Name(n("x"));
    let _lit = Expr::BasicLit(BasicLit {
        value: "1".to_string(),
        kind: LitKind::Int,
        bad: false,
        pos: pos(1, 1),
    });
    let _binary = Expr::Binary(Operation {
        op: Op::Add,
        x: Box::new(Expr::Name(n("a"))),
        y: Some(Box::new(Expr::Name(n("b")))),
        pos: pos(1, 1),
    });
    let _unary = Expr::Unary(Operation {
        op: Op::Sub,
        x: Box::new(Expr::Name(n("a"))),
        y: None,
        pos: pos(1, 1),
    });
    let _bad = Expr::Bad(BadExpr { pos: pos(1, 1) });
    let _call = Expr::Call(CallExpr {
        fun: Box::new(Expr::Name(n("f"))),
        args: vec![],
        has_dots: false,
        pos: pos(1, 1),
        rparen: pos(1, 3),
    });
    let _paren = Expr::Paren(ParenExpr {
        x: Box::new(Expr::Name(n("x"))),
        pos: pos(1, 1),
    });
    let _list = Expr::List(ListExpr {
        elems: vec![Expr::Name(n("a")), Expr::Name(n("b"))],
        pos: pos(1, 1),
    });

    // 类型族（G18：落在 Expr 族，无独立 TypeExpr）
    let _array = Expr::ArrayType(ArrayType {
        len: None,
        elem: Box::new(Expr::Name(n("int"))),
        pos: pos(1, 1),
    });
    let _slice = Expr::SliceType(SliceType {
        elem: Box::new(Expr::Name(n("int"))),
        pos: pos(1, 1),
    });
    let _chan = Expr::ChanType(ChanType {
        dir: Some(ChanDir::SendOnly),
        elem: Box::new(Expr::Name(n("int"))),
        pos: pos(1, 1),
    });
    let _map = Expr::MapType(MapType {
        key: Box::new(Expr::Name(n("string"))),
        value: Box::new(Expr::Name(n("int"))),
        pos: pos(1, 1),
    });
}

#[test]
fn t_a_03_stmt_variants() {
    let b = || BlockStmt {
        stmts: vec![],
        rbrace: pos(1, 2),
        pos: pos(1, 1),
    };

    let _empty = Stmt::Empty(EmptyStmt { pos: pos(1, 1) });
    let _block = Stmt::Block(b());
    let _expr = Stmt::Expr(ExprStmt {
        x: Expr::Name(n("f")),
        pos: pos(1, 1),
    });
    let _ret = Stmt::Return(ReturnStmt {
        results: vec![],
        pos: pos(1, 1),
    });
    let _if = Stmt::If(IfStmt {
        init: None,
        cond: Some(Expr::Name(n("x"))),
        then: b(),
        els: None,
        pos: pos(1, 1),
    });
    let _for = Stmt::For(ForStmt {
        init: None,
        cond: None,
        post: None,
        body: b(),
        pos: pos(1, 1),
    });
    let _branch = Stmt::Branch(BranchStmt {
        tok: BranchTok::Break,
        label: None,
        pos: pos(1, 1),
    });

    // 官方无 BadStmt/BadDecl（G19）：此处编译通过即证明未引入这些变体
}

/// `DeclStmt` 装的是**声明列表**（官方 `DeclList []Decl`，`nodes.go:389`）。
/// `func f() { var (a int; b int) }` 里那对括号会产生 **2 条** `VarDecl`
/// （`appendGroup` 每行 `append` 一次）。单数的模型**编译也过** —— 只有放两条才暴露。
#[test]
fn t_a_03_decl_stmt_holds_a_list() {
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
    let s = DeclStmt {
        decls: vec![d("a"), d("b")],
        pos: pos(1, 1),
    };
    assert_eq!(s.decls.len(), 2, "分组声明必须能装多条");
}

/// 官方 6 处**位置字段**（`nodes.go` `:185`/`:374`/`:443`/`:449`/`:465`/`:472`）。
///
/// 它们不参与 dump（C6 规则 4：位置默认不 dump），由 `@` 腿单独测 ——
/// 但**字段必须存在**，否则 `@` 腿连测的机会都没有。
#[test]
fn t_a_03_position_fields_exist() {
    let blk = BlockStmt {
        stmts: vec![],
        rbrace: pos(1, 9),
        pos: pos(1, 1),
    };
    assert_eq!(blk.rbrace.col, 9, "`Rbrace` 是**右花括号**的位置");
    assert_eq!(
        blk.pos.col, 1,
        "`pos` 是 `{{` 的位置（F9：最左标识其种类的 token）"
    );

    let cl = CompositeLit {
        type_: None,
        elems: vec![],
        rbrace: pos(2, 9),
        pos: pos(2, 1),
    };
    let sw = SwitchStmt {
        init: None,
        tag: None,
        cases: vec![],
        rbrace: pos(3, 9),
        pos: pos(3, 1),
    };
    let sel = SelectStmt {
        clauses: vec![],
        rbrace: pos(4, 9),
        pos: pos(4, 1),
    };
    let cc = CaseClause {
        cases: None,
        body: vec![],
        colon: pos(5, 5),
        pos: pos(5, 1),
    };
    let mc = CommClause {
        comm: None,
        body: vec![],
        colon: pos(6, 5),
        pos: pos(6, 1),
    };

    assert_eq!(cl.rbrace.line, 2);
    assert_eq!(sw.rbrace.line, 3);
    assert_eq!(sel.rbrace.line, 4);
    assert_eq!(cc.colon.line, 5);
    assert_eq!(mc.colon.line, 6);
}

#[test]
fn t_a_04_simple_stmt_variants() {
    let _empty = SimpleStmt::Empty;
    let _assign = SimpleStmt::Assign(AssignStmt {
        op: AssignOp::Define,
        lhs: vec![Expr::Name(n("x"))],
        rhs: vec![Expr::Name(n("y"))],
        pos: pos(1, 1),
    });
    let _range = SimpleStmt::Range(RangeClause {
        lhs: vec![Expr::Name(n("k"))],
        is_define: true,
        x: Expr::Name(n("m")),
        pos: pos(1, 1),
    });
    // 断言 SimpleStmt 与 Stmt 是分开的两族
    let _is_simple: SimpleStmt = SimpleStmt::Empty;
}

#[test]
fn t_a_06_walk_is_defined() {
    // 遍历器存在（具体行为在实现后补）
    let f = AstFile {
        pos: pos(1, 1),
        pragma: Pragma::default(),
        pkg_name: n("p"),
        decls: vec![],
        eof: pos(1, 1),
        go_version: None,
    };
    assert_eq!(count_nodes(&f), 2); // File + PkgName（无 decls）
}

/// 官方 `ImportDecl.Path` 是 `*BasicLit`，注释明说 `nil means no path`（`nodes.go:68`）。
///
/// # 触发条件是一段**语法上照常产出节点**的源码
///
/// `import x`（缺路径）：官方 `importDecl` 里 `d.Path = p.oliteral()` 拿到 nil，
/// 报一条 `missing import path`，然后**照常返回这个 `ImportDecl`**（`parser.go:571-575`）。
/// 也就是说 `Path == nil` 的节点**真的会被送到 dump**。
///
/// 我们的模型若用不可空的 `BasicLit`，这一格就只能**编一个假字面量**顶上 ——
/// 那是「用哨兵值冒充缺失」：节点数、dump 行数都会与官方分叉，
/// 而分叉点在**错误路径**上，正常语料测不到。
#[test]
fn t_a_01_import_path_can_be_none() {
    let no_path = ImportDecl {
        group: None,
        pragma: Pragma::default(),
        name: None,
        path: None,
        pos: pos(1, 1),
    };
    assert!(no_path.path.is_none(), "`import x` 的 Path 是 nil");

    let with_path = ImportDecl {
        group: None,
        pragma: Pragma::default(),
        name: None,
        path: Some(BasicLit {
            value: "\"math\"".to_string(),
            kind: LitKind::String,
            bad: false,
            pos: pos(2, 1),
        }),
        pos: pos(2, 1),
    };
    assert_eq!(
        with_path.path.as_ref().unwrap().value,
        "\"math\"",
        "有路径时存原文（含引号）"
    );
}

// ---------------------------------------------------------------------------
// `Expr::start_pos` —— 官方 `positions.go` 的 StartPos（Expr 子集）。
// 判据来源：positions.go:10-111；官方 parser.go 唯一使用点 `:2077`。
// 对撞实证：`missing type parameter name` 的 errPos 用 pos() 会右偏一列。

#[test]
fn start_pos_修正到子节点的那些变体() {
    // IndexExpr `t[t]`：存储位在 `[`，起点在 `t`（tparams.go 对撞的案发现场）。
    let index = Expr::Index(IndexExpr {
        x: Box::new(Expr::Name(n("t"))),
        index: Box::new(Expr::Name(n("t"))),
        pos: pos(1, 2),
    });
    assert_eq!(index.pos(), pos(1, 2), "存储位 = 官方 Pos()（[ 的位置）");
    assert_eq!(
        index.start_pos(),
        pos(1, 1),
        "起点修正到 X（positions.go:45）"
    );

    // CallExpr `f(x)`：存储位在 `(`，起点在 `f`。
    let call = Expr::Call(CallExpr {
        fun: Box::new(Expr::Name(n("f"))),
        args: vec![],
        has_dots: false,
        pos: pos(2, 3),
        rparen: pos(2, 5),
    });
    assert_eq!(
        call.start_pos(),
        pos(1, 1),
        "起点修正到 Fun（positions.go:62）"
    );

    // 二元 Operation：起点在 X；一元回到自己的存储位。
    let binary = Expr::Binary(Operation {
        op: Op::Add,
        x: Box::new(Expr::Name(n("a"))),
        y: Some(Box::new(Expr::Name(n("b")))),
        pos: pos(3, 3),
    });
    assert_eq!(binary.start_pos(), pos(1, 1));
    let unary = Expr::Unary(Operation {
        op: Op::Not,
        x: Box::new(Expr::Name(n("a"))),
        y: None,
        pos: pos(4, 4),
    });
    assert_eq!(
        unary.start_pos(),
        pos(4, 4),
        "一元不修正（positions.go:56-61）"
    );
}

#[test]
fn start_pos_无条件回退到存储位的变体() {
    let lit = Expr::BasicLit(BasicLit {
        value: "1".to_string(),
        kind: LitKind::Int,
        bad: false,
        pos: pos(1, 1),
    });
    assert_eq!(lit.start_pos(), lit.pos(), "Name/BasicLit/Bad 不修正");
}
