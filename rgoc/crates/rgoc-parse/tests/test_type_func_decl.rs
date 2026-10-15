//! `T-A-23`（`TypeDecl`，`AST-06`）与 `T-A-25`（`FuncDecl`，`AST-08`）。
//!
//! 两个声明的共同点：都是**唯一**在 `file()` 里直接产出、且带**可选/二义片段**的
//! 声明 —— `type` 的 `[` 既可能是数组长度又可能是类型参数表（§5.6 / `T-A-23`），
//! `func` 的可选 receiver、可选类型参数、**可选函数体**（前向声明）交织在一起。
//!
//! # 断言的组织方式
//!
//! 每一族都既钉**变体/标志**（`alias`、`recv`、`body`、`tparams`）又钉**子结构**
//! （`Type` 是 `ArrayType` 而非 `SliceType`；`tparams[0].type_` 是 `~` 约束）。
//! 只钉一侧的话，「数组 vs 泛型」写反仍然全绿。
//!
//! # 判据来源
//!
//! - 官方 `parser.go`：`typeDecl`（`:608`）、`extractName`（`:690`）、`isTypeElem`（`:747`）、
//!   `funcDeclOrNil`（`:799`）、`funcBody`（`:845`）。
//! - 位置规则见官方 `nodes_test.go`（`@` 标记）与 `M2-design.md` §3.3：
//!   `func @f() {}`（`FuncDecl.pos` 在**名字**）、`func @(T) f() {}`（在 **`(`**）、
//!   `type @T int`（在**名字**，与是否分组无关）。

use rgoc_ast::{ArrayType, AstFile, Decl, Expr, Field, FuncDecl, Op, SliceType, TypeDecl, dump};
use rgoc_parse::{ParseResult, parse_file};

fn parse(src: &str) -> ParseResult {
    parse_file(src.as_bytes(), "t.go")
}

fn file_of(r: &ParseResult) -> &AstFile {
    r.file
        .as_ref()
        .unwrap_or_else(|| panic!("应产出 AstFile，诊断：{:?}", r.diags))
}

fn types(r: &ParseResult) -> Vec<&TypeDecl> {
    file_of(r)
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Type(t) => Some(t),
            _ => None,
        })
        .collect()
}

fn funcs(r: &ParseResult) -> Vec<&FuncDecl> {
    file_of(r)
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Func(f) => Some(f),
            _ => None,
        })
        .collect()
}

/// 取**恰好一条** type 声明（多于/少于一条都炸 —— 免得"少解析了"被当成"解析对了"）。
fn one_type(r: &ParseResult) -> &TypeDecl {
    let v = types(r);
    assert_eq!(v.len(), 1, "应恰好一条 type 声明，诊断：{:?}", r.diags);
    v[0]
}

fn one_func(r: &ParseResult) -> &FuncDecl {
    let v = funcs(r);
    assert_eq!(v.len(), 1, "应恰好一条 func 声明，诊断：{:?}", r.diags);
    v[0]
}

fn name_of(e: &Expr) -> &str {
    match e {
        Expr::Name(n) => n.value.as_str(),
        other => panic!("不是 Name：{other:?}"),
    }
}

fn field_name(f: &Field) -> Option<&str> {
    f.name.as_ref().map(|n| n.value.as_str())
}

/// 首条诊断的消息（`DiagBag` 不可索引，且这里每条用例都恰好一条错）。
fn first_msg(r: &ParseResult) -> String {
    r.first
        .as_ref()
        .map(|d| d.message.clone())
        .unwrap_or_default()
}

// ───────────────────────── T-A-23 TypeDecl ─────────────────────────

#[test]
fn t_a_23_非别名类型声明() {
    let r = parse("package p\ntype T int\n");
    assert_eq!(r.errors, 0, "干净输入：{:?}", r.diags);

    let t = one_type(&r);
    assert_eq!(t.name.value, "T");
    assert!(!t.alias, "`type T int` 不是别名");
    assert!(t.tparams.is_empty(), "无类型参数");
    assert_eq!(name_of(&t.type_), "int");
    assert!(t.group.is_none(), "单条声明不属于任何分组");
    // 官方 `nodes_test.go`：`type @T int` ⇒ pos 在**名字**上（不是 `type`）。
    assert_eq!((t.pos.line, t.pos.col), (2, 6), "pos 落在名字 `T`");
}

#[test]
fn t_a_23_别名类型声明() {
    let r = parse("package p\ntype T = int\n");
    assert_eq!(r.errors, 0, "干净输入：{:?}", r.diags);

    let t = one_type(&r);
    assert!(t.alias, "`type T = int` 是别名");
    assert_eq!(name_of(&t.type_), "int");
}

#[test]
fn t_a_23_泛型类型参数列表() {
    let r = parse("package p\ntype List[T any] struct{}\n");
    assert_eq!(r.errors, 0, "干净输入：{:?}", r.diags);

    let t = one_type(&r);
    assert_eq!(t.tparams.len(), 1, "`[T any]` ⇒ 一条类型参数");
    assert_eq!(field_name(&t.tparams[0]), Some("T"));
    assert_eq!(name_of(&t.tparams[0].type_), "any", "约束是 `any`");
    assert!(!t.alias);
    assert!(
        matches!(t.type_, Expr::StructType(_)),
        "`struct{{}}` ⇒ StructType，got {:?}",
        t.type_
    );
}

#[test]
fn t_a_23_泛型类型两参数() {
    let r = parse("package p\ntype Pair[K comparable, V any] struct{}\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let t = one_type(&r);
    assert_eq!(t.tparams.len(), 2, "`[K comparable, V any]` ⇒ 两条");
    assert_eq!(field_name(&t.tparams[0]), Some("K"));
    assert_eq!(name_of(&t.tparams[0].type_), "comparable");
    assert_eq!(field_name(&t.tparams[1]), Some("V"));
    assert_eq!(name_of(&t.tparams[1].type_), "any");
}

/// 类型参数的**约束**可以是类型集写法 `~T`（`typSetOk` 只在 `]` 收尾时开）。
///
/// 这条同时钉住 `paramList` 的 `typeSetsOk` 分支：官方 `paramDeclOrNil` 在
/// `close == _Rbrack` 时走 `embeddedElem` 而不是普通类型 —— 写反了 `~int`
/// 会在 `~` 处报「expected type」，节点形状却仍是 `Name`，**看起来合理**。
#[test]
fn t_a_23_类型参数的波浪号约束() {
    let r = parse("package p\ntype E[T ~int] int\n");
    assert_eq!(r.errors, 0, "干净输入：{:?}", r.diags);

    let t = one_type(&r);
    assert_eq!(t.tparams.len(), 1);
    assert_eq!(field_name(&t.tparams[0]), Some("T"));
    match &t.tparams[0].type_ {
        Expr::Unary(op) => {
            assert_eq!(op.op, Op::Tilde, "约束是 `~int`");
            assert_eq!(name_of(&op.x), "int");
        }
        other => panic!("期望一元 `~` 约束，got {other:?}"),
    }
    assert_eq!(name_of(&t.type_), "int", "类型体是 `int`");
}

/// **二义消解的负例**：`[N]` —— 单个名字**紧跟 `]`** ⇒ 官方 `extractName` 的
/// 条件 `ptype != nil || p.tok != _Rbrack` 为假 ⇒ 判成**数组长度**，不是类型参数。
///
/// 写反（一律当泛型）的症状：`tparams` 凭空多一条、`Type` 从 `ArrayType` 变 `SliceType`，
/// **不报错**，树形看起来也像那么回事。
#[test]
fn t_a_23_单个名字加右括号是数组而非泛型() {
    let r = parse("package p\ntype A [N]int\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let t = one_type(&r);
    assert!(t.tparams.is_empty(), "`[N]` 不是类型参数表");
    match &t.type_ {
        Expr::ArrayType(a) => {
            let len = a.len.as_ref().expect("`[N]int` 的长度非 `[...]`");
            assert_eq!(name_of(len), "N", "长度就是那个名字");
            assert_eq!(name_of(&a.elem), "int");
        }
        other => panic!("期望 ArrayType，got {other:?}"),
    }
}

#[test]
fn t_a_23_数组长度可以是常量字面量() {
    let r = parse("package p\ntype A [3]int\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let t = one_type(&r);
    assert!(t.tparams.is_empty());
    match &t.type_ {
        Expr::ArrayType(a) => {
            let len = a.len.as_ref().expect("长度存在");
            assert!(
                matches!(&**len, Expr::BasicLit(l) if l.value == "3"),
                "长度是字面量 `3`，got {len:?}"
            );
        }
        other => panic!("期望 ArrayType，got {other:?}"),
    }
}

/// ⚠️ 这条钉的是官方 `typeDecl` 里**唯一**「名字开头但还得接着解析表达式」的路径：
/// `[N + 1]` 里 `N` 后面不是 `]` ⇒ 官方 `p.binaryExpr(p.pexpr(name, false), 0)`。
///
/// 少了这个展开，`A [N + 1]int` 会在 `+` 处断掉（长度只剩 `N`），而 `[N]int`
/// 那种简单情形**照样正确** ⇒ 只测简单情形发现不了。
#[test]
fn t_a_23_数组长度名字开头也要接着爬优先级() {
    let r = parse("package p\ntype A [N + 1]int\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let t = one_type(&r);
    assert!(t.tparams.is_empty(), "有 `+` ⇒ 不是类型参数表");
    let a = match &t.type_ {
        Expr::ArrayType(a) => a,
        other => panic!("期望 ArrayType，got {other:?}"),
    };
    let len = a.len.as_ref().expect("长度存在");
    match &**len {
        Expr::Binary(op) => {
            assert_eq!(op.op, Op::Add, "长度是 `N + 1`");
            assert_eq!(name_of(&op.x), "N");
            assert!(
                matches!(op.y.as_deref(), Some(Expr::BasicLit(l)) if l.value == "1"),
                "右操作数是 `1`"
            );
        }
        other => panic!("期望二元 `+` 长度，got {other:?}"),
    }
}

/// `force` 参数（官方 `extractName(x, p.tok == _Comma)`）的**唯一可观测后果**。
///
/// `type A [n * m]int` 里 `n * m` 是**数组长度**（二元 `*`），**不是** `n` 加约束 `*m` ——
/// 因为 `]` 之前没有逗号 ⇒ `force == false`，而 `is_type_elem(m)` 为假（`m` 是裸名字）
/// ⇒ `extractName` 拆不动。
///
/// 反过来（`force` 恒 `true`）会让它误判成类型参数表 `[n *m]` —— `tparams` 从空变一条，
/// `Type` 形状也跟着变，**不报错**。这条就是那个差异的对照。
#[test]
fn t_a_23_数组长度是乘法表达式时不误判为泛型() {
    let r = parse("package p\ntype A [n * m]int\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let t = one_type(&r);
    assert!(t.tparams.is_empty(), "`[n * m]` 不是类型参数表");
    let a = match &t.type_ {
        Expr::ArrayType(a) => a,
        other => panic!("期望 ArrayType，got {other:?}"),
    };
    let len = a.len.as_ref().expect("长度存在");
    match &**len {
        Expr::Binary(op) => {
            assert_eq!(op.op, Op::Mul, "长度是 `n * m`");
            assert_eq!(name_of(&op.x), "n");
            assert!(
                matches!(op.y.as_deref(), Some(Expr::Name(n)) if n.value == "m"),
                "右操作数是 `m`"
            );
        }
        other => panic!("期望二元 `*` 长度，got {other:?}"),
    }
    assert_eq!(name_of(&a.elem), "int");
}

/// 官方 `extractName` 文档里的**正例** `P*[]int` ⇒ `(P, *[]int)`。
///
/// 这条钉住 [`is_type_elem`] 的**肯定分支**：`*` 右边是 `[]int`（切片类型）⇒
/// 「确定是类型元素」⇒ 敢把左边的 `P` 当成类型参数名。
///
/// 若 `is_type_elem` 对切片类型判假（漏了 `SliceType` 那一项），`P*[]int` 会退化成
/// **数组长度**（`ArrayType` 的 `Len` 是那个二元 `*`）—— 形状全变，**不报错**。
#[test]
fn t_a_23_名字后跟指针约束() {
    let r = parse("package p\ntype A[P *[]int] int\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let t = one_type(&r);
    assert_eq!(t.tparams.len(), 1, "`[P *[]int]` ⇒ 一条类型参数");
    let f = &t.tparams[0];
    assert_eq!(field_name(f), Some("P"), "名字是 `P`");
    match &f.type_ {
        Expr::Unary(op) => {
            assert_eq!(op.op, Op::Mul, "约束是 `*[]int`");
            assert!(
                matches!(op.x.as_ref(), Expr::SliceType(s) if name_of(&s.elem) == "int"),
                "约束的核心是 `[]int`，got {:?}",
                op.x
            );
        }
        other => panic!("期望一元 `*[]int` 约束，got {other:?}"),
    }
    assert_eq!(name_of(&t.type_), "int", "类型体是 `int`");
}

/// 官方 `extractName` 的 **`|` 分支**：`P*E|F` ⇒ `(P, *E|F)`（官方文档的 `P*E|F|~G` 同形）。
///
/// 这条钉住 `Or` 分支的**成功路径**（先递归把 `x.X` 拆出 `(name, lhs)`，
/// 再把 `lhs | x.Y` 组装回去）—— 少了它，`|` 那一支只剩「拆不动」的负例，
/// 等于**没测**：成功路径写错不会让任何断言变红。
#[test]
fn t_a_23_名字后跟联合约束() {
    let r = parse("package p\ntype A[P*[]int | E] int\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let t = one_type(&r);
    assert_eq!(t.tparams.len(), 1, "`[P*[]int | E]` ⇒ 一条类型参数");
    let f = &t.tparams[0];
    assert_eq!(field_name(f), Some("P"), "名字是 `P`");
    match &f.type_ {
        Expr::Binary(op) => {
            assert_eq!(op.op, Op::Or, "约束是 `|` 联合");
            assert!(
                matches!(op.x.as_ref(), Expr::Unary(u) if u.op == Op::Mul),
                "左支是 `*[]int`，got {:?}",
                op.x
            );
            assert!(
                matches!(op.y.as_deref(), Some(Expr::Name(n)) if n.value == "E"),
                "右支是 `E`，got {:?}",
                op.y
            );
        }
        other => panic!("期望二元 `|` 约束，got {other:?}"),
    }
    assert_eq!(name_of(&t.type_), "int", "类型体是 `int`");
}

#[test]
fn t_a_23_空方括号是切片类型() {
    let r = parse("package p\ntype S []int\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let t = one_type(&r);
    assert!(t.tparams.is_empty());
    let s: &SliceType = match &t.type_ {
        Expr::SliceType(s) => s,
        other => panic!("期望 SliceType，got {other:?}"),
    };
    assert_eq!(name_of(&s.elem), "int");
}

/// 分组的 `type`：每条各是一个 `TypeDecl`，**同组共享 `Group`**，
/// 且 `pos` **与是否分组无关**（官方 `nodes_test.go`：`type (@T int)` ⇒ `@` 在名字上）。
#[test]
fn t_a_23_分组_type_共享_group() {
    let r = parse("package p\ntype (\nT int\nU = string\n)\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let v = types(&r);
    assert_eq!(v.len(), 2, "两条声明 ⇒ 两个 TypeDecl");
    assert!(v[0].group.is_some(), "分组里的声明一定带 Group");
    assert_eq!(v[0].group, v[1].group, "同组共享同一个 Group 值");
    assert!(!v[0].alias, "`T int`");
    assert!(v[1].alias, "`U = string`");
    assert_eq!(v[0].name.value, "T");
    assert_eq!(v[1].name.value, "U");
}

#[test]
fn t_a_23_分组不影响位置() {
    let r = parse("package p\ntype (T int)\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    let t = one_type(&r);
    // `type (` … `T` 在第 7 列（`t`=1..`e`=4、空格=5、`(`=6、`T`=7）。
    assert_eq!(
        (t.pos.line, t.pos.col),
        (2, 7),
        "pos 落在名字 `T`，与分组无关"
    );
}

/// `type T`（缺类型）：官方 `d.Type == nil` ⇒ 造 `badExpr`、报 `in type declaration`、
/// `advance(_Semi, _Rparen)`，**照常返回节点**。
#[test]
fn t_a_23_缺类型报错并产出_bad_expr() {
    let r = parse("package p\ntype T\n");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);

    let t = one_type(&r);
    assert_eq!(t.name.value, "T");
    assert!(
        matches!(t.type_, Expr::Bad(_)),
        "缺类型 ⇒ `Expr::Bad` 占位，got {:?}",
        t.type_
    );
}

// ───────────────────────── T-A-25 FuncDecl ─────────────────────────

#[test]
fn t_a_25_无_receiver_的空体函数() {
    let r = parse("package p\nfunc f() {}\n");
    assert_eq!(r.errors, 0, "干净输入：{:?}", r.diags);

    let f = one_func(&r);
    assert!(f.recv.is_none(), "普通函数没有 receiver");
    assert_eq!(f.name.value, "f");
    assert!(f.tparams.is_empty());
    assert!(matches!(f.type_, Expr::FuncType(_)), "签名是 FuncType");
    let b = f.body.as_ref().expect("`func f() {}` 有函数体");
    assert!(b.stmts.is_empty(), "空块");
    // 官方 `nodes_test.go`：`func @f() {}` ⇒ pos 在**名字**。
    assert_eq!((f.pos.line, f.pos.col), (2, 6), "pos 落在名字 `f`");
}

#[test]
fn t_a_25_函数类型的位置在左括号() {
    let r = parse("package p\nfunc f() {}\n");
    let f = one_func(&r);
    match &f.type_ {
        Expr::FuncType(ft) => {
            // 官方 `nodes_test.go`：`func@()` ⇒ FuncType.pos 在 `(`（第 7 列）。
            assert_eq!((ft.pos.line, ft.pos.col), (2, 7), "FuncType.pos 落在 `(`");
        }
        other => panic!("期望 FuncType，got {other:?}"),
    }
}

#[test]
fn t_a_25_函数体是空块且位置正确() {
    let r = parse("package p\nfunc f() {}\n");
    let f = one_func(&r);
    let b = f.body.as_ref().expect("有体");
    // `func f() {}`：`{` 第 10 列、`}` 第 11 列。
    assert_eq!((b.pos.line, b.pos.col), (2, 10), "块起点在 `{{`");
    assert_eq!((b.rbrace.line, b.rbrace.col), (2, 11), "右花括号在 `}}`");
}

#[test]
fn t_a_25_方法带命名_receiver() {
    let r = parse("package p\nfunc (r T) M() {}\n");
    assert_eq!(r.errors, 0, "干净输入：{:?}", r.diags);

    let f = one_func(&r);
    let recv = f.recv.as_ref().expect("方法有 receiver");
    assert_eq!(field_name(recv), Some("r"), "`(r T)` 的接收者名叫 r");
    assert_eq!(name_of(&recv.type_), "T");
    assert_eq!(f.name.value, "M");
    // 官方 `nodes_test.go`：`func @(x T) f() {}` ⇒ pos 在 **`(`**（第 6 列）。
    assert_eq!((f.pos.line, f.pos.col), (2, 6), "pos 落在 receiver 的 `(`");
}

#[test]
fn t_a_25_方法带无名_receiver() {
    let r = parse("package p\nfunc (T) M() {}\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let f = one_func(&r);
    let recv = f.recv.as_ref().expect("有 receiver");
    assert_eq!(field_name(recv), None, "`(T)` 只有类型、没有名字");
    assert_eq!(name_of(&recv.type_), "T");
    assert_eq!(f.name.value, "M");
}

/// **前向声明**（外部声明）：`func` 名字 + 签名，**没有函数体** ⇒ `body == None`，
/// 且**一条诊断都没有**（官方 `funcDeclOrNil` 只在见到 `{` 时才解析体）。
///
/// 与「有体但体是空的」是**两种形状**：前者 `None`、后者 `Some(空块)`。
/// 拿 `Some(空块)` 冒充 `None`（或反过来）都不会报错 —— 正是本仓记的「漏了不会报错」。
#[test]
fn t_a_25_无体的前向声明() {
    let r = parse("package p\nfunc f()\n");
    assert_eq!(r.errors, 0, "前向声明**不是**错误：{:?}", r.diags);

    let f = one_func(&r);
    assert!(f.body.is_none(), "没有 `{{` ⇒ 没有体");
    assert_eq!(f.name.value, "f");
    assert!(matches!(f.type_, Expr::FuncType(_)));
}

#[test]
fn t_a_25_无体方法() {
    let r = parse("package p\nfunc (r T) M()\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    let f = one_func(&r);
    assert!(f.recv.is_some());
    assert!(f.body.is_none());
    assert_eq!(f.name.value, "M");
}

#[test]
fn t_a_25_泛型函数() {
    let r = parse("package p\nfunc f[T any](x T) T {}\n");
    assert_eq!(r.errors, 0, "干净输入：{:?}", r.diags);

    let f = one_func(&r);
    assert_eq!(f.name.value, "f");
    assert_eq!(f.tparams.len(), 1, "`[T any]` ⇒ 一条类型参数");
    assert_eq!(field_name(&f.tparams[0]), Some("T"));
    assert_eq!(name_of(&f.tparams[0].type_), "any");

    match &f.type_ {
        Expr::FuncType(ft) => {
            assert_eq!(ft.params.len(), 1, "一个参数 `x T`");
            assert_eq!(field_name(&ft.params[0]), Some("x"));
            assert_eq!(name_of(&ft.params[0].type_), "T");
            assert_eq!(ft.results.len(), 1, "一个结果 `T`");
            assert_eq!(field_name(&ft.results[0]), None, "结果无名");
            assert_eq!(name_of(&ft.results[0].type_), "T");
        }
        other => panic!("期望 FuncType，got {other:?}"),
    }
    assert!(f.body.is_some());
}

#[test]
fn t_a_25_参数与结果的形状() {
    let r = parse("package p\nfunc f(a int, b string) (int, error) {}\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let f = one_func(&r);
    let ft = match &f.type_ {
        Expr::FuncType(ft) => ft,
        other => panic!("期望 FuncType，got {other:?}"),
    };
    assert_eq!(ft.params.len(), 2);
    assert_eq!(field_name(&ft.params[0]), Some("a"));
    assert_eq!(name_of(&ft.params[0].type_), "int");
    assert_eq!(field_name(&ft.params[1]), Some("b"));
    assert_eq!(name_of(&ft.params[1].type_), "string");
    assert_eq!(ft.results.len(), 2, "`(int, error)` 两个结果");
    assert_eq!(name_of(&ft.results[0].type_), "int");
    assert_eq!(name_of(&ft.results[1].type_), "error");
}

/// `func () M()` —— 官方 `funcDeclOrNil` 的 `case 0: p.error("method has no receiver")`。
///
/// 空 receiver 列表**不产出 `recv`**，但**照样继续**解析后面的名字与签名。
#[test]
fn t_a_25_空_receiver_报_没有接收者() {
    let r = parse("package p\nfunc () M() {}\n");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    assert!(
        first_msg(&r).contains("method has no receiver"),
        "消息形态：{}",
        first_msg(&r)
    );

    let f = one_func(&r);
    assert!(f.recv.is_none(), "没有 receiver");
    assert_eq!(f.name.value, "M", "名字照样解析出来");
}

/// `func (a, b T) M()` —— 官方 `default: p.error("method has multiple receivers")`
/// **`fallthrough`** 到 `case 1` ⇒ **既要报错、又要取第一个**当 `recv`。
///
/// 漏掉 `fallthrough` 的症状：报错对了，但 `recv == None`（静默丢字段）。
#[test]
fn t_a_25_多个_receiver_报错并取第一个() {
    let r = parse("package p\nfunc (a, b T) M() {}\n");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    assert!(
        first_msg(&r).contains("method has multiple receivers"),
        "消息形态：{}",
        first_msg(&r)
    );

    let f = one_func(&r);
    let recv = f
        .recv
        .as_ref()
        .expect("`fallthrough` 到 case 1 ⇒ 仍取第一个");
    assert_eq!(field_name(recv), Some("a"), "取的是**第一个** receiver");
}

/// 空函数声明后紧跟块：官方 `file()` 的 `default` 臂靠 `isEmptyFuncDecl`
/// 认出「上一条是**无体**函数声明，这个 `{` 是想给它补体」⇒
/// 报 `unexpected semicolon or newline before {`。
#[test]
fn t_a_25_空声明后跟花括号报错() {
    let r = parse("package p\nfunc f()\n{}\n");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    assert!(
        first_msg(&r).contains("unexpected semicolon or newline before {"),
        "消息形态：{}",
        first_msg(&r)
    );

    let f = one_func(&r);
    assert!(f.body.is_none(), "那个 `{{` **没有**被挂成函数体");
}

/// 顶层 `type` 与 `func` 混排：顺序与种类都要对。
#[test]
fn t_a_23_顶层_type_与_func_混排() {
    let r = parse("package p\ntype T int\nfunc f() {}\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let d = &file_of(&r).decls;
    assert_eq!(d.len(), 2);
    assert!(matches!(d[0], Decl::Type(_)), "第一条是 type");
    assert!(matches!(d[1], Decl::Func(_)), "第二条是 func");
}

/// **中立 dump 的整段形状**（这是 `D-M2-1` 对撞真正要比的东西）。
#[test]
fn t_a_23_25_dump_形状() {
    let r =
        parse("package p\ntype Pair[K comparable, V any] struct{}\nfunc (r T) M(a int) int {}\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let got = dump(file_of(&r));
    let want = "\
File
  PkgName: Name \"p\"
  GoVersion: -
  Decls: [2 entries]
    TypeDecl
      Name: Name \"Pair\"
      Alias: false
      TParams: [2 entries]
        Field
          Name: Name \"K\"
          Type: Name \"comparable\"
          Tag: -
        Field
          Name: Name \"V\"
          Type: Name \"any\"
          Tag: -
      Type:
        StructType
          Fields: [0 entries]
    FuncDecl
      Recv:
        Field
          Name: Name \"r\"
          Type: Name \"T\"
          Tag: -
      Name: Name \"M\"
      TParams: [0 entries]
      Type:
        FuncType
          Params: [1 entries]
            Field
              Name: Name \"a\"
              Type: Name \"int\"
              Tag: -
          Results: [1 entries]
            Field
              Name: -
              Type: Name \"int\"
              Tag: -
      Body:
        BlockStmt
          Stmts: [0 entries]
";
    assert_eq!(got, want);
}

/// 数组长度是表达式时，dump 里的 `Len` 必须**真出现**（且是 `Operation`）。
///
/// 这条是上一族二义消解在 dump 侧的对照：`type A [N + 1]int` 的长度若被吞成 `None`
/// （`[...]` 形态），`Len: -` 会与 `[...]T` 撞形，**不报错**。
#[test]
fn t_a_23_dump_数组长度是表达式() {
    let r = parse("package p\ntype A [N + 1]int\n");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let a = match &one_type(&r).type_ {
        Expr::ArrayType(a) => a as &ArrayType,
        other => panic!("期望 ArrayType，got {other:?}"),
    };
    assert!(a.len.is_some(), "长度不是 `[...]`");

    let got = dump(file_of(&r));
    assert!(
        got.contains("Operation") && got.contains("Op: +") && got.contains("Len:"),
        "dump 里要有 `Len:` + 二元 `+`：\n{got}"
    );
}
