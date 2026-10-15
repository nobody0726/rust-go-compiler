//! `T-A-40` / `T-A-41` / `T-A-42` / `T-A-43` / `T-A-44` / `T-A-45` —— 类型族（`AST-24..32`）。
//!
//! 全部走 **`parse_file`**：类型唯一能从文件入口观察到的位置是 `var x <TYPE>` 的
//! `Type` 槽（`const x T = v` 也行，但 `var` 没有 `=` 的歧义；`type T ...` 要
//! `T-AP09`，用不了）。表达式路径（`<-chan` 的重新关联）走 `var x = <expr>`。
//!
//! # 断言的组织方式
//!
//! 每条都**双向**：既钉「变体对不对」（`Expr::ArrayType` 而非 `Expr::Index`），
//! 也钉「子结构」（`Len` 是 `Some` 而非 `None`、`Dir` 是 `SendOnly` 而非 `None`）。
//! 只钉一侧的话，把 `[...]T` 与 `[N]T` 写成同一支仍然全绿。

use rgoc_ast::{ChanDir, Decl, Expr, Field, LitKind, Op};

use rgoc_parse::parse_file;

/// 解析 `package p` + `var x <TYPE>`，返回 `(类型表达式, 报错条数)`。
fn var_type(type_src: &str) -> (Expr, usize) {
    let src = format!("package p\n\nvar x {type_src}\n");
    let r = parse_file(src.as_bytes(), "t.go");
    let f = r
        .file
        .unwrap_or_else(|| panic!("应产出 AstFile：{:?}", r.diags));
    match f.decls.as_slice() {
        [Decl::Var(v)] => (v.type_.clone().expect("`var x T` 必有类型"), r.errors),
        other => panic!("期望恰好一条 VarDecl，got {other:?}"),
    }
}

/// 干净输入（0 报错）的类型。
fn ty(type_src: &str) -> Expr {
    let (t, n) = var_type(type_src);
    assert_eq!(n, 0, "`{type_src}` 该是干净的（0 报错）");
    t
}

/// 解析 `package p` + `var x = <expr>`，返回 `(值表达式, 报错条数)`。
fn var_value(expr_src: &str) -> (Expr, usize) {
    let src = format!("package p\n\nvar x = {expr_src}\n");
    let r = parse_file(src.as_bytes(), "t.go");
    let f = r
        .file
        .unwrap_or_else(|| panic!("应产出 AstFile：{:?}", r.diags));
    match f.decls.as_slice() {
        [Decl::Var(v)] => {
            assert_eq!(v.values.len(), 1, "`{expr_src}` 只有一个值：{:?}", v.values);
            (v.values[0].clone(), r.errors)
        }
        other => panic!("期望恰好一条 VarDecl，got {other:?}"),
    }
}

// ── 拆包小工具（每个只服务一种变体，用不到的不定义）──────────────────────

fn func_of(e: &Expr) -> rgoc_ast::FuncType {
    match e {
        Expr::FuncType(t) => t.clone(),
        other => panic!("期望 FuncType，got {other:?}"),
    }
}
fn array_of(e: &Expr) -> rgoc_ast::ArrayType {
    match e {
        Expr::ArrayType(t) => t.clone(),
        other => panic!("期望 ArrayType，got {other:?}"),
    }
}
fn slice_of(e: &Expr) -> rgoc_ast::SliceType {
    match e {
        Expr::SliceType(t) => t.clone(),
        other => panic!("期望 SliceType，got {other:?}"),
    }
}
fn struct_of(e: &Expr) -> rgoc_ast::StructType {
    match e {
        Expr::StructType(t) => t.clone(),
        other => panic!("期望 StructType，got {other:?}"),
    }
}
fn iface_of(e: &Expr) -> rgoc_ast::InterfaceType {
    match e {
        Expr::InterfaceType(t) => t.clone(),
        other => panic!("期望 InterfaceType，got {other:?}"),
    }
}
fn map_of(e: &Expr) -> rgoc_ast::MapType {
    match e {
        Expr::MapType(t) => t.clone(),
        other => panic!("期望 MapType，got {other:?}"),
    }
}
fn chan_of(e: &Expr) -> rgoc_ast::ChanType {
    match e {
        Expr::ChanType(t) => t.clone(),
        other => panic!("期望 ChanType，got {other:?}"),
    }
}
fn dots_of(e: &Expr) -> rgoc_ast::DotsType {
    match e {
        Expr::DotsType(t) => t.clone(),
        other => panic!("期望 DotsType，got {other:?}"),
    }
}
fn un_of(e: &Expr) -> (Op, Expr) {
    match e {
        Expr::Unary(o) => {
            assert!(o.y.is_none(), "一元运算的 `y` 必须是 None，got {:?}", o.y);
            (o.op, (*o.x).clone())
        }
        other => panic!("期望 Unary，got {other:?}"),
    }
}
fn bin_of(e: &Expr) -> (Op, Expr, Expr) {
    match e {
        Expr::Binary(o) => {
            let y = o.y.as_deref().expect("二元运算必须有 y").clone();
            (o.op, (*o.x).clone(), y)
        }
        other => panic!("期望 Binary，got {other:?}"),
    }
}
fn name_of(e: &Expr) -> &str {
    match e {
        Expr::Name(n) => &n.value,
        other => panic!("期望 Name，got {other:?}"),
    }
}
/// 字段名：`None` → 嵌入字段/无名参数。
fn field_name(f: &Field) -> Option<&str> {
    f.name.as_ref().map(|n| n.value.as_str())
}
/// 一条诊断里的消息（用于「报对了没有」）。
fn msgs(r_src: &str) -> Vec<String> {
    let src = format!("package p\n\nvar x {r_src}\n");
    parse_file(src.as_bytes(), "t.go")
        .diags
        .into_sorted()
        .into_iter()
        .map(|d| d.message)
        .collect()
}

// ── `T-A-40` ArrayType / SliceType（`AST-24/25`）──────────────────────────

/// `[3]int`：`Len` 是**表达式**（`Some`），`Elem` 是 `int`。位置落在 `[`。
#[test]
fn t_a_40_数组类型_长度是表达式() {
    let t = ty("[3]int");
    let a = array_of(&t);
    match a.len.as_deref() {
        Some(Expr::BasicLit(l)) => {
            assert_eq!(l.kind, LitKind::Int);
            assert_eq!(l.value, "3", "存原文");
        }
        other => panic!("`Len` 该是 Int 字面量，got {other:?}"),
    }
    assert_eq!(name_of(&a.elem), "int");
    assert_eq!(a.pos.line, 3, "类型在 `var x [3]int` 那一行");
    assert_eq!(a.pos.col, 7, "数组类型的位置落在 `[`（`var x ` 是 6 列）");
}

/// `[...]int`：`Len == None`（官方 `Len = nil` 表示 `...`）。
#[test]
fn t_a_40_数组类型_省略长度是_none() {
    let a = array_of(&ty("[...]int"));
    assert!(a.len.is_none(), "`[...]T` 的 Len 必须是 None");
    assert_eq!(name_of(&a.elem), "int");
}

/// `[]int`：`SliceType`（**不是** `Len = None` 的 `ArrayType`）。
#[test]
fn t_a_40_切片类型() {
    let s = slice_of(&ty("[]int"));
    assert_eq!(name_of(&s.elem), "int");
    assert_eq!(s.pos.col, 7, "切片类型的位置落在 `[`");
}

/// 数组长度可以是**名字**（`const N = 3; var x [N]int`）。
#[test]
fn t_a_40_数组长度可以是名字() {
    let a = array_of(&ty("[n]int"));
    match a.len.as_deref() {
        Some(l) => assert_eq!(name_of(l), "n"),
        other => panic!("`Len` 该是 Name，got {other:?}"),
    }
}

/// 数组长度可以是**完整表达式**（`[n + 1]int`）——
/// 官方在这里显式 `p.xnest++`，所以 `[f(x)]int` 里的 `(` 是调用而不是参数表。
#[test]
fn t_a_40_数组长度可以是表达式() {
    let a = array_of(&ty("[n + 1]int"));
    let (op, x, y) = bin_of(a.len.as_deref().expect("Len 是 Some"));
    assert_eq!(op, Op::Add);
    assert_eq!(name_of(&x), "n");
    match y {
        Expr::BasicLit(l) => assert_eq!(l.value, "1"),
        other => panic!("`+` 的右操作数该是字面量，got {other:?}"),
    }
}

/// `[][]int`：切片套切片（逐层断言，防止「只解析了一层」）。
#[test]
fn t_a_40_嵌套切片() {
    let outer = slice_of(&ty("[][]int"));
    let inner = slice_of(&outer.elem);
    assert_eq!(name_of(&inner.elem), "int");
}

/// `[3][]string`：数组套切片。
#[test]
fn t_a_40_数组套切片() {
    let a = array_of(&ty("[3][]string"));
    assert!(a.len.is_some(), "有长度");
    let s = slice_of(&a.elem);
    assert_eq!(name_of(&s.elem), "string");
}

// ── `T-A-41` DotsType（`AST-26`）─────────────────────────────────────────

/// `func(...int)`：`...T` 只可能出现在**参数表**里（官方 `typeOrNil` 没有 `...` 分支）。
#[test]
fn t_a_41_变参类型() {
    let f = func_of(&ty("func(...int)"));
    assert_eq!(f.params.len(), 1);
    assert_eq!(field_name(&f.params[0]), None, "未命名参数");
    let d = dots_of(&f.params[0].type_);
    assert_eq!(name_of(&d.elem), "int");
}

/// `func(a ...int)`：带名字的变参。
#[test]
fn t_a_41_命名变参() {
    let f = func_of(&ty("func(a ...int)"));
    assert_eq!(field_name(&f.params[0]), Some("a"));
    let d = dots_of(&f.params[0].type_);
    assert_eq!(name_of(&d.elem), "int");
}

/// `...` 只能在**最后一个**参数上，且此时类型**退回**成元素类型（官方 `f.Type = t.Elem`）。
///
/// ⚠️ 这里不能走 `ty()` —— 该输入**本就有 1 条报错**，`ty()` 会断言"干净"而假红。
/// 官方只报**第一条**（`first := true`，`parser.go:2122`）。
#[test]
fn t_a_41_变参必须在末尾() {
    let ms = msgs("func(a ...int, b int)");
    assert!(
        ms.iter()
            .any(|m| m.contains("can only use ... with final parameter")),
        "该报「... 只能在最后」：{ms:?}"
    );
    let (t, n) = var_type("func(a ...int, b int)");
    assert_eq!(n, 1, "只报第一条（官方 `first := true`）");
    let f = func_of(&t);
    assert_eq!(f.params.len(), 2);
    assert_eq!(name_of(&f.params[0].type_), "int", "非法 `...T` 退回 `T`");
    assert_eq!(name_of(&f.params[1].type_), "int");
}

// ── `T-A-42` StructType / Field（`AST-27/28`）────────────────────────────

/// `struct{}`：空字段表。
#[test]
fn t_a_42_空结构体() {
    let s = struct_of(&ty("struct{}"));
    assert!(s.fields.is_empty(), "got {:?}", s.fields);
    assert_eq!(s.pos.col, 7, "结构体类型的位置落在 `struct`");
}

/// `struct{ a int }`：一个具名字段，无标签。
#[test]
fn t_a_42_单字段() {
    let s = struct_of(&ty("struct{ a int }"));
    assert_eq!(s.fields.len(), 1);
    assert_eq!(field_name(&s.fields[0]), Some("a"));
    assert!(s.fields[0].tag.is_none(), "无标签");
    assert_eq!(name_of(&s.fields[0].type_), "int");
}

/// `struct{ a, b int }`：**多名字共享一个类型** ⇒ 摊成两条 `Field`，各自的 `pos` 是自己的名字。
#[test]
fn t_a_42_多名字共享类型() {
    let s = struct_of(&ty("struct{ a, b int }"));
    assert_eq!(s.fields.len(), 2, "got {:?}", s.fields);
    assert_eq!(field_name(&s.fields[0]), Some("a"));
    assert_eq!(field_name(&s.fields[1]), Some("b"));
    assert_eq!(name_of(&s.fields[0].type_), "int");
    assert_eq!(name_of(&s.fields[1].type_), "int");
    assert_ne!(
        s.fields[0].pos.col, s.fields[1].pos.col,
        "两条字段的位置该分别是自己的名字"
    );
}

/// 结构体标签：**原文保留**（含反引号）。
#[test]
fn t_a_42_字段标签() {
    let s = struct_of(&ty(r#"struct{ a int `json:"a"` }"#));
    assert_eq!(
        field_name(&s.fields[0]),
        Some("a"),
        "带标签的字段仍是具名字段"
    );
    let tag = s.fields[0].tag.as_ref().expect("该有标签");
    assert_eq!(tag.value, "`json:\"a\"`", "含两端的反引号");
}

/// 嵌入字段：限定名 `io.Reader` ⇒ `name == None`。
#[test]
fn t_a_42_嵌入_限定名() {
    let s = struct_of(&ty("struct{ io.Reader }"));
    assert_eq!(s.fields.len(), 1);
    assert_eq!(field_name(&s.fields[0]), None, "嵌入字段没有名字");
    match &s.fields[0].type_ {
        Expr::Selector(sel) => {
            assert_eq!(name_of(&sel.x), "io");
            assert_eq!(sel.sel.value, "Reader");
        }
        other => panic!("期望 Selector，got {other:?}"),
    }
}

/// 嵌入字段：指针 `*T` ⇒ 一元 `*`（`y == None`）。
#[test]
fn t_a_42_嵌入_指针() {
    let s = struct_of(&ty("struct{ *T }"));
    assert_eq!(field_name(&s.fields[0]), None);
    let (op, x) = un_of(&s.fields[0].type_);
    assert_eq!(op, Op::Mul);
    assert_eq!(name_of(&x), "T");
}

/// 嵌入字段：**实例化的**类型 `T[int]` ⇒ `IndexExpr`（`fieldDecl` 的「小心舞步」）。
#[test]
fn t_a_42_嵌入_实例化类型() {
    let s = struct_of(&ty("struct{ T[int] }"));
    assert_eq!(field_name(&s.fields[0]), None, "是嵌入类型，不是字段 T");
    match &s.fields[0].type_ {
        Expr::Index(ix) => {
            assert_eq!(name_of(&ix.x), "T");
            assert_eq!(name_of(&ix.index), "int");
        }
        other => panic!("期望 IndexExpr，got {other:?}"),
    }
}

/// 对照：`a [3]int` 是**具名字段**而非嵌入实例化 —— 同一个 `[` 的两种含义。
///
/// 少了这条，把「小心舞步」简写成「见 `[` 就当实例化」仍然全绿。
#[test]
fn t_a_42_具名字段的方括号是数组() {
    let s = struct_of(&ty("struct{ a [3]int }"));
    assert_eq!(
        field_name(&s.fields[0]),
        Some("a"),
        "是 `a [3]int` 不是 `a[3]`"
    );
    let a = array_of(&s.fields[0].type_);
    assert!(a.len.is_some());
    assert_eq!(name_of(&a.elem), "int");
}

// ── `T-A-43` InterfaceType（`AST-29`）────────────────────────────────────

/// `interface{}`：空方法集。
#[test]
fn t_a_43_空接口() {
    let i = iface_of(&ty("interface{}"));
    assert!(i.methods.is_empty(), "got {:?}", i.methods);
    assert_eq!(i.pos.col, 7, "接口类型的位置落在 `interface`");
}

/// `interface{ M() }`：**方法** ⇒ 有名字 + `FuncType`。
#[test]
fn t_a_43_方法() {
    let i = iface_of(&ty("interface{ M() }"));
    assert_eq!(i.methods.len(), 1);
    assert_eq!(field_name(&i.methods[0]), Some("M"));
    let f = func_of(&i.methods[0].type_);
    assert!(f.params.is_empty() && f.results.is_empty());
}

/// `interface{ M(a int) string }`：方法的参数与结果。
#[test]
fn t_a_43_带参方法() {
    let i = iface_of(&ty("interface{ M(a int) string }"));
    let f = func_of(&i.methods[0].type_);
    assert_eq!(f.params.len(), 1);
    assert_eq!(field_name(&f.params[0]), Some("a"));
    assert_eq!(name_of(&f.params[0].type_), "int");
    assert_eq!(f.results.len(), 1);
    assert_eq!(name_of(&f.results[0].type_), "string");
}

/// `interface{ io.Reader }`：**嵌入接口** ⇒ 无名字（与「方法」是两条路）。
#[test]
fn t_a_43_嵌入接口() {
    let i = iface_of(&ty("interface{ io.Reader }"));
    assert_eq!(field_name(&i.methods[0]), None, "嵌入接口没有名字");
    match &i.methods[0].type_ {
        Expr::Selector(sel) => assert_eq!(sel.sel.value, "Reader"),
        other => panic!("期望 Selector，got {other:?}"),
    }
}

/// 类型约束 `~int`：一元 `Tilde`。
#[test]
fn t_a_43_类型约束_tilde() {
    let i = iface_of(&ty("interface{ ~int }"));
    assert_eq!(field_name(&i.methods[0]), None);
    let (op, x) = un_of(&i.methods[0].type_);
    assert_eq!(op, Op::Tilde);
    assert_eq!(name_of(&x), "int");
}

/// 类型约束并集 `int | string`：二元 `Or`，**左结合**。
#[test]
fn t_a_43_类型约束_并集() {
    let i = iface_of(&ty("interface{ int | string }"));
    let (op, x, y) = bin_of(&i.methods[0].type_);
    assert_eq!(op, Op::Or);
    assert_eq!(name_of(&x), "int");
    assert_eq!(name_of(&y), "string");
}

/// `~int | ~string`：两个 `Tilde` 各自挂在自己的 `Or` 操作数上。
#[test]
fn t_a_43_类型约束_tilde_并集() {
    let i = iface_of(&ty("interface{ ~int | ~string }"));
    let (op, x, y) = bin_of(&i.methods[0].type_);
    assert_eq!(op, Op::Or);
    assert_eq!(un_of(&x).0, Op::Tilde);
    assert_eq!(un_of(&y).0, Op::Tilde);
    assert_eq!(name_of(&un_of(&x).1), "int");
    assert_eq!(name_of(&un_of(&y).1), "string");
}

/// `any` **不是**关键字：`interface{ any }` 走「嵌入名字」那条路。
#[test]
fn t_a_43_any_是普通名字() {
    let i = iface_of(&ty("interface{ any }"));
    assert_eq!(field_name(&i.methods[0]), None);
    assert_eq!(name_of(&i.methods[0].type_), "any");
}

// ── `T-A-44` FuncType（`AST-30`）─────────────────────────────────────────

/// `func()`：空参数、空结果。位置落在 **`(`**（官方 `funcType` 在 `func` 已消费后取 `p.pos()`）。
#[test]
fn t_a_44_无参无结果() {
    let f = func_of(&ty("func()"));
    assert!(f.params.is_empty() && f.results.is_empty());
    assert_eq!(f.pos.col, 11, "`var x func(` ⇒ `(` 在第 11 列");
}

/// `func(int)`：未命名参数。
#[test]
fn t_a_44_单参数() {
    let f = func_of(&ty("func(int)"));
    assert_eq!(f.params.len(), 1);
    assert_eq!(field_name(&f.params[0]), None);
    assert_eq!(name_of(&f.params[0].type_), "int");
}

/// `func(a int)`：命名参数。
#[test]
fn t_a_44_命名参数() {
    let f = func_of(&ty("func(a int)"));
    assert_eq!(field_name(&f.params[0]), Some("a"));
    assert_eq!(name_of(&f.params[0].type_), "int");
}

/// `func(a, b int)`：多名字共享类型。
#[test]
fn t_a_44_多名字共享类型() {
    let f = func_of(&ty("func(a, b int)"));
    assert_eq!(f.params.len(), 2);
    assert_eq!(field_name(&f.params[0]), Some("a"));
    assert_eq!(field_name(&f.params[1]), Some("b"));
    assert_eq!(name_of(&f.params[0].type_), "int");
    assert_eq!(name_of(&f.params[1].type_), "int");
}

/// `func() string`：单值结果（**不是**括号形式）。
#[test]
fn t_a_44_单结果() {
    let f = func_of(&ty("func() string"));
    assert_eq!(f.results.len(), 1);
    assert_eq!(name_of(&f.results[0].type_), "string");
}

/// `func() (int, error)`：括号结果表。
#[test]
fn t_a_44_多结果() {
    let f = func_of(&ty("func() (int, error)"));
    assert_eq!(f.results.len(), 2);
    assert_eq!(name_of(&f.results[0].type_), "int");
    assert_eq!(name_of(&f.results[1].type_), "error");
}

/// `func() (x int, y error)`：命名结果。
#[test]
fn t_a_44_命名结果() {
    let f = func_of(&ty("func() (x int, y error)"));
    assert_eq!(field_name(&f.results[0]), Some("x"));
    assert_eq!(field_name(&f.results[1]), Some("y"));
}

/// `func(a)`：**只有一个名字**的例外 —— 官方按「名字当类型」分布 ⇒
/// 结果是一条**无名**参数、类型是 `a`（`name` 被搬去当 `Type`）。
///
/// 这条钉的是 `paramList` 的 `named == 0 && !requireNames` 分支：
/// 少了它，`func(a)` 会得到「名为 `a` 的未知类型参数」。
#[test]
fn t_a_44_裸名字被当成类型() {
    let f = func_of(&ty("func(a)"));
    assert_eq!(f.params.len(), 1);
    assert_eq!(field_name(&f.params[0]), None, "名字搬去当类型了");
    assert_eq!(name_of(&f.params[0].type_), "a");
}

/// 对照：`func(a, b)` ⇒ 两条无名参数，类型分别是 `a` / `b`。
#[test]
fn t_a_44_两个裸名字都是类型() {
    let f = func_of(&ty("func(a, b)"));
    assert_eq!(f.params.len(), 2);
    assert_eq!(field_name(&f.params[0]), None);
    assert_eq!(field_name(&f.params[1]), None);
    assert_eq!(name_of(&f.params[0].type_), "a");
    assert_eq!(name_of(&f.params[1].type_), "b");
}

/// **类型位置**的 `func` 不许带类型参数：`func[T any]()` ⇒ 报一条，但**照常解析**。
///
/// 官方 `funcType(context)` 在 `context != ""` 时 `syntaxErrorAt(..., context + " must have
/// no type parameters")`。这条把 `context` 这个参数**真正接上**（否则它是死参数）。
#[test]
fn t_a_44_类型位置的函数类型禁止类型参数() {
    let ms = msgs("func[T any]()");
    assert!(
        ms.iter()
            .any(|m| m.contains("must have no type parameters")),
        "该报「类型位置不许有类型参数」：{ms:?}"
    );
    let (t, n) = var_type("func[T any]()");
    assert_eq!(n, 1, "恰好一条错误");
    let f = func_of(&t);
    assert!(f.params.is_empty() && f.results.is_empty(), "其余照常解析");
}

// ── `T-A-45` MapType / ChanType（`AST-31/32`）────────────────────────────

/// `map[string]int`。
#[test]
fn t_a_45_map() {
    let m = map_of(&ty("map[string]int"));
    assert_eq!(name_of(&m.key), "string");
    assert_eq!(name_of(&m.value), "int");
    assert_eq!(m.pos.col, 7, "map 类型的位置落在 `map`");
}

/// `chan int`：双向 ⇒ `dir == None`。
#[test]
fn t_a_45_chan_双向() {
    let c = chan_of(&ty("chan int"));
    assert_eq!(c.dir, None, "双向通道没有方向");
    assert_eq!(name_of(&c.elem), "int");
    assert_eq!(c.pos.col, 7);
}

/// `chan<- int`：只发 ⇒ `SendOnly`（`F16` = 1）。
#[test]
fn t_a_45_chan_只发() {
    let c = chan_of(&ty("chan<- int"));
    assert_eq!(c.dir, Some(ChanDir::SendOnly));
    assert_eq!(c.dir.expect("已断言") as u8, 1, "F16：SendOnly = 1");
    assert_eq!(name_of(&c.elem), "int");
}

/// `<-chan int`：只收 ⇒ `RecvOnly`（`F16` = 2）。位置落在 `<-`。
#[test]
fn t_a_45_chan_只收() {
    let c = chan_of(&ty("<-chan int"));
    assert_eq!(c.dir, Some(ChanDir::RecvOnly));
    assert_eq!(c.dir.expect("已断言") as u8, 2, "F16：RecvOnly = 2");
    assert_eq!(name_of(&c.elem), "int");
    assert_eq!(c.pos.col, 7);
}

/// `chan chan int`：嵌套，外层双向、内层双向。
#[test]
fn t_a_45_chan_嵌套() {
    let outer = chan_of(&ty("chan chan int"));
    assert_eq!(outer.dir, None);
    let inner = chan_of(&outer.elem);
    assert_eq!(inner.dir, None);
    assert_eq!(name_of(&inner.elem), "int");
}

/// `map[string][]int`：值可以是任意类型。
#[test]
fn t_a_45_map_的值可以是切片() {
    let m = map_of(&ty("map[string][]int"));
    assert_eq!(name_of(&m.key), "string");
    assert_eq!(name_of(&slice_of(&m.value).elem), "int");
}

/// **表达式路径**的 `<-chan int`：`unaryExpr` 的 `_Arrow` 重新关联。
///
/// 与 `t_a_45_chan_只收` 是**两条不同的代码路径**：那条走 `typeOrNil` 的 `_Arrow` 分支，
/// 这条走 `unaryExpr` 的 `<-` 分支（`<-x` 接收 vs `<-chan E` 通道，要等 `x` 解析完才知道）。
#[test]
fn t_a_45_表达式里的只收通道被重新关联() {
    let (e, n) = var_value("<-chan int");
    assert_eq!(n, 0, "合法：`<-chan int` 是一个类型");
    let c = chan_of(&e);
    assert_eq!(c.dir, Some(ChanDir::RecvOnly));
    assert_eq!(name_of(&c.elem), "int");
}

/// `<-chan<- int`：**非法**（`<-chan<-E` 不成立）——
/// 官方沿 `SendOnly` 链走到尽头仍没找到非通道元素时报一条，但**照常产出节点**。
#[test]
fn t_a_45_双向只发通道的只收形式报错但仍产出节点() {
    let (e, n) = var_value("<-chan<- int");
    assert_eq!(n, 1, "恰好一条错误");
    let c = chan_of(&e);
    assert_eq!(c.dir, Some(ChanDir::RecvOnly), "外层被改写成只收");
    assert_eq!(name_of(&c.elem), "int", "元素保持 `int`");
}

/// 对照：`<-x` 是**接收运算**，不是通道类型（`x` 不是通道时走接收那条路）。
#[test]
fn t_a_45_接收运算不是通道类型() {
    let (e, n) = var_value("<-ch");
    assert_eq!(n, 0);
    let (op, x) = un_of(&e);
    assert_eq!(op, Op::Arrow);
    assert_eq!(name_of(&x), "ch");
}

// ── 补漏：`qualifiedName` 尾部的 `typeInstance` ───────────────────────────

/// `T[int]` 作为**类型名** ⇒ `IndexExpr`（官方 `qualifiedName` 尾部那段）。
///
/// 这条是 `T-AP12` 回源读官方时才发现的**漏项**：`qualifiedName` 不只认「名字 `.` 名字」，
/// 还接一段 `typeInstance`。少了它，`var x T[int]` 会在 `[` 处报错。
#[test]
fn t_a_34_限定名尾部的方括号是类型实例化() {
    let t = ty("T[int]");
    match &t {
        Expr::Index(ix) => {
            assert_eq!(name_of(&ix.x), "T");
            assert_eq!(name_of(&ix.index), "int");
            assert_eq!(ix.pos.col, 8, "位置落在 `[`");
        }
        other => panic!("期望 IndexExpr，got {other:?}"),
    }
}

/// 多类型实参 `Pair[K, V]` ⇒ 索引是 `ListExpr`。
#[test]
fn t_a_34_多类型实参是列表() {
    let t = ty("Pair[K, V]");
    match &t {
        Expr::Index(ix) => {
            assert_eq!(name_of(&ix.x), "Pair");
            match ix.index.as_ref() {
                Expr::List(l) => {
                    assert_eq!(l.elems.len(), 2);
                    assert_eq!(name_of(&l.elems[0]), "K");
                    assert_eq!(name_of(&l.elems[1]), "V");
                }
                other => panic!("期望 ListExpr，got {other:?}"),
            }
        }
        other => panic!("期望 IndexExpr，got {other:?}"),
    }
}
