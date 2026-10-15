//! `T-A-32` / `T-A-33` / `T-A-36`（+ `T-A-38` 的组合）—— 表达式族的**高级形态**（`T-AP11`）。
//!
//! 与 `test_expr.rs` 同一套入口：`package p` + 一条 `const x = <expr>`。
//! `const` 的 `Values` 是 M2 里表达式**唯一**能从文件入口观察到的位置
//! （`simpleStmt`/`funcBody` 要等 `T-AP13`/`T-AP09`）。
//!
//! # 断言的组织方式
//!
//! 每条断言都**双向**：
//! - 既钉「变体对不对」（`Expr::Call` 而非 `Expr::Index`），
//! - 也钉「子结构 + 那个容易写错的布尔/位置字段」（`has_dots`、`type_` 是 `Some`
//!   还是 `None`、`pos` 落在**哪个词元**上）。
//!
//! 只钉一侧的话，把 `has_dots` 写成恒 `false`、把 `CompositeLit.type_` 恒置 `None`
//! 都仍然全绿 —— 那正是本仓记的「漏了不会报错」形状。
//!
//! # 位置断言为什么写死列号
//!
//! 官方每个节点的位置都是「**最左标示其种类**的那个词元」（`M2-design.md` §3.3），
//! 而这条规则是**可错**的（`CallExpr.pos` 是 `(` 而不是 `f`；`KeyValueExpr.pos`
//! 是 `:` 而不是 key）。写死列号才能把「取错了词元」与「取对了」区分开；
//! 只断言「行号」的测试对取错词元**恒真**。列号随源串改动是**期望**的 ——
//! 它就在同一行注释里标着。

use rgoc_ast::{Decl, Expr, LitKind, Op};
use rgoc_parse::parse_file;

/// 解析 `package p\n\nconst x = <expr>\n`，返回那条 `ConstDecl` 的值列表。
///
/// ⚠️ **不带错的入口**（同 `test_expr.rs` 的 `const_values`）：`vals()` 允许报错，
/// `one()` 要求干净。两者分开是有意的 —— 拿「干净输入」的入口去测**本就该报错**的
/// 输入，红/绿都失去含义（`T-AP12` 的 `t_a_41_变参必须在末尾` 就栽在这上面）。
fn vals(expr_src: &str) -> (Vec<Expr>, usize, String) {
    let src = format!("package p\n\nconst x = {expr_src}\n");
    let r = parse_file(src.as_bytes(), "t.go");
    let msgs = format!("{:?}", r.diags);
    let v = match r.file {
        Some(f) => match f.decls.as_slice() {
            [Decl::Const(c)] => c.values.clone(),
            other => panic!("`{expr_src}` 期望恰好一条 ConstDecl，got {other:?}"),
        },
        None => panic!("`{expr_src}` 应产出 AstFile：{msgs}"),
    };
    (v, r.errors, msgs)
}

/// 取 `const x = <expr>` 里**唯一**的那个值表达式；要求**零报错**。
fn one(expr_src: &str) -> Expr {
    let (v, n, msgs) = vals(expr_src);
    assert_eq!(n, 0, "`{expr_src}` 该是干净的：{msgs}");
    assert_eq!(v.len(), 1, "`{expr_src}` 只有一个值，got {v:?}");
    v.into_iter().next().expect("len 已断言为 1")
}

/// 拆一个 `Expr::Name` → 值。
fn name_of(e: &Expr) -> &str {
    match e {
        Expr::Name(n) => &n.value,
        other => panic!("期望 Name，got {other:?}"),
    }
}

// 下面三个拆包助手返回**拥有所有权的克隆**，而不是 `&T`。
//
// 理由纯属 Rust：`call_of(&one("f(a)"))` 里的 `one(…)` 是**临时值**，
// 返回引用会让它在语句末尾就被 drop（`E0716`）。写成 `let e = one(…); call_of(&e)`
// 能绕开，但那样每条用例都多一行纯噪声；而这些结构的 `Clone` 是**免费**的
// （`#[derive(Clone)]`），测试也不在乎一次克隆。

/// 拆一个 `Expr::Call`。
fn call_of(e: &Expr) -> rgoc_ast::CallExpr {
    match e {
        Expr::Call(c) => c.clone(),
        other => panic!("期望 CallExpr，got {other:?}"),
    }
}

/// 拆一个 `Expr::CompositeLit`。
fn complit_of(e: &Expr) -> rgoc_ast::CompositeLit {
    match e {
        Expr::CompositeLit(c) => c.clone(),
        other => panic!("期望 CompositeLit，got {other:?}"),
    }
}

/// 拆一个 `Expr::FuncLit`。
fn funclit_of(e: &Expr) -> rgoc_ast::FuncLit {
    match e {
        Expr::FuncLit(f) => f.clone(),
        other => panic!("期望 FuncLit，got {other:?}"),
    }
}

// ══════════════════════════════════════════════════════════════════════════
// `T-A-32` CallExpr（`AST-22`）
// ══════════════════════════════════════════════════════════════════════════

/// 普通调用：`fun` 与 `args` 两侧都要钉。
#[test]
fn t_a_32_普通调用() {
    let e = one("f(a, b)");
    let c = call_of(&e);
    assert_eq!(name_of(&c.fun), "f", "`Fun` 是被调用的名字");
    assert_eq!(c.args.len(), 2, "两个实参，got {:?}", c.args);
    assert_eq!(name_of(&c.args[0]), "a");
    assert_eq!(name_of(&c.args[1]), "b");
    assert!(!c.has_dots, "`f(a, b)` 没有 `...`");
}

/// 空实参表：`args` 为空**且**不是「解析失败」。
#[test]
fn t_a_32_空实参表() {
    let e = one("f()");
    let c = call_of(&e);
    assert!(c.args.is_empty(), "无实参，got {:?}", c.args);
    assert!(!c.has_dots);
}

/// **`...` 变参**：同一个 `f(a, b…)` 形状，`has_dots` 必须真的跟着变。
///
/// 两行对照是刻意的：只测「`f(a...)` 的 `has_dots` 为真」的话，
/// 把 `has_dots` 写成恒 `true` 也能过。
#[test]
fn t_a_32_变参调用与普通调用对照() {
    let dotted = one("f(a, b...)");
    let c = call_of(&dotted);
    assert_eq!(c.args.len(), 2, "`...` 不增加实参个数，got {:?}", c.args);
    assert!(c.has_dots, "`b...` ⇒ HasDots 为真");

    let plain = one("f(a, b)");
    assert!(!call_of(&plain).has_dots, "同一个形状没有 `...` ⇒ 为假");
}

/// **位置**：`CallExpr.pos` 落在 **`(`** 上，不是 `f` 上。
///
/// `const x = f(a)` → `f` 在第 11 列、`(` 在第 12 列。
#[test]
fn t_a_32_调用位置落在左括号() {
    let e = one("f(a)");
    assert_eq!(call_of(&e).pos.col, 12, "`(` 所在列（不是 `f` 的 11）");
}

/// 调用**链**：`f(a)(b)` ⇒ 外层 `Fun` 本身是一个 `CallExpr`。
#[test]
fn t_a_32_调用链() {
    let e = one("f(a)(b)");
    let outer = call_of(&e);
    assert_eq!(outer.args.len(), 1);
    assert_eq!(name_of(&outer.args[0]), "b", "外层实参是 `b`");
    let inner = call_of(&outer.fun);
    assert_eq!(name_of(&inner.fun), "f", "内层被调用者是 `f`");
    assert_eq!(name_of(&inner.args[0]), "a", "内层实参是 `a`");
}

/// 泛型实例化后调用：`f[int](a)` ⇒ `Fun` 是 `IndexExpr`。
#[test]
fn t_a_32_泛型实例化后调用() {
    let e = one("f[int](a)");
    let c = call_of(&e);
    match &*c.fun {
        Expr::Index(i) => {
            assert_eq!(name_of(&i.x), "f");
            assert_eq!(name_of(&i.index), "int");
        }
        other => panic!("`Fun` 应是 IndexExpr（`f[int]`），got {other:?}"),
    }
    assert_eq!(c.args.len(), 1);
    assert_eq!(name_of(&c.args[0]), "a");
}

/// **类型转换与调用同形**：`T(x)` 就是 `CallExpr`（官方不区分，判别归 M3）。
#[test]
fn t_a_32_裸类型名的转换与调用同形() {
    let c = call_of(&one("T(x)"));
    assert_eq!(name_of(&c.fun), "T");
    assert_eq!(name_of(&c.args[0]), "x");
}

/// 类型字面量作被转换者：`[]int(x)` ⇒ `Fun` 是 `SliceType`。
///
/// 这条把「`operand` 的 `case _Lbrack` 走 `type_()`」与「`pexpr` 的 `(` 后缀」
/// **串起来**钉住 —— 分开各自全绿、拼起来接不上，是最容易发生的一类错。
#[test]
fn t_a_32_类型字面量的转换() {
    let c = call_of(&one("[]int(x)"));
    match &*c.fun {
        Expr::SliceType(s) => assert_eq!(name_of(&s.elem), "int"),
        other => panic!("`Fun` 应是 SliceType，got {other:?}"),
    }
    assert_eq!(name_of(&c.args[0]), "x");
}

/// 实参里可以有复合字面量：`f(T{1})`。
///
/// 这条同时证明 `argList` 里的 `xnest++` 生效（否则 `T{` 会被当成块的开头）。
#[test]
fn t_a_32_实参可为复合字面量() {
    let c = call_of(&one("f(T{1})"));
    assert_eq!(c.args.len(), 1);
    let lit = complit_of(&c.args[0]);
    assert_eq!(lit.elems.len(), 1);
    match lit.type_.as_deref() {
        Some(Expr::Name(n)) => assert_eq!(n.value, "T"),
        other => panic!("实参的复合字面量该带类型 `T`，got {other:?}"),
    }
}

// ══════════════════════════════════════════════════════════════════════════
// `T-A-33` CompositeLit + KeyValueExpr（`AST-12/13`）
// ══════════════════════════════════════════════════════════════════════════

/// 裸类型名：`T{1, 2}` —— `type_` 是 `Some(Name(T))`（**不是** `None`）。
#[test]
fn t_a_33_裸类型名的复合字面量() {
    let lit = complit_of(&one("T{1, 2}"));
    assert_eq!(lit.elems.len(), 2, "got {:?}", lit.elems);
    match lit.type_.as_deref() {
        Some(Expr::Name(n)) => assert_eq!(n.value, "T"),
        other => panic!("`Type` 该是 Name(T)，got {other:?}"),
    }
}

/// 切片字面量 `[]int{1, 2}` —— 类型是 `SliceType`（不是 `Name`、也不是 `None`）。
#[test]
fn t_a_33_切片字面量() {
    let lit = complit_of(&one("[]int{1, 2}"));
    assert_eq!(lit.elems.len(), 2);
    match lit.type_.as_deref() {
        Some(Expr::SliceType(s)) => assert_eq!(name_of(&s.elem), "int"),
        other => panic!("`Type` 该是 SliceType，got {other:?}"),
    }
}

/// 数组字面量 `[3]int{1}` —— 长度是 `Some`（与 `[...]T` 的 `None` 对照。
/// `[` 的二义消解在 `T-AP12` 已钉，这里只钉它在**复合字面量前缀**位置也对）。
#[test]
fn t_a_33_数组字面量() {
    let lit = complit_of(&one("[3]int{1}"));
    assert_eq!(lit.elems.len(), 1);
    match lit.type_.as_deref() {
        Some(Expr::ArrayType(a)) => {
            assert!(a.len.is_some(), "`[3]` 的长度该是 Some");
            assert_eq!(name_of(&a.elem), "int");
        }
        other => panic!("`Type` 该是 ArrayType，got {other:?}"),
    }
}

/// map 字面量的**键值对**：`map[string]int{"a": 1}`。
#[test]
fn t_a_33_map_字面量的键值对() {
    let lit = complit_of(&one("map[string]int{\"a\": 1}"));
    assert_eq!(lit.elems.len(), 1, "一个键值元素，got {:?}", lit.elems);
    match &lit.elems[0] {
        Expr::KeyValue(kv) => {
            match &*kv.key {
                Expr::BasicLit(l) => {
                    assert_eq!(l.kind, LitKind::String);
                    assert_eq!(l.value, "\"a\"", "字面量存原文（含引号）");
                }
                other => panic!("`Key` 该是字符串字面量，got {other:?}"),
            }
            match &*kv.value {
                Expr::BasicLit(l) => assert_eq!(l.kind, LitKind::Int),
                other => panic!("`Value` 该是整数字面量，got {other:?}"),
            }
        }
        other => panic!("元素该是 KeyValueExpr，got {other:?}"),
    }
}

/// **位置**：`KeyValueExpr.pos` 落在 **`:`** 上（官方 `:1304` 的 `l.pos = p.pos()`）。
///
/// `const x = map[string]int{"a": 1}` → `:` 在第 29 列。
/// 取成 key 或 value 的位置都会红。
#[test]
fn t_a_33_键值对位置落在冒号() {
    let lit = complit_of(&one("map[string]int{\"a\": 1}"));
    match &lit.elems[0] {
        Expr::KeyValue(kv) => assert_eq!(kv.pos.col, 29, "`:` 所在列"),
        other => panic!("期望 KeyValueExpr，got {other:?}"),
    }
}

/// **嵌套**字面量省略类型：`[][]int{{1}, {2}}` ⇒ 内层 `type_ == None`。
///
/// 这条钉 `bare_complitexpr`：见到 `{` 直接进 `complitexpr`（**无**类型前缀）。
#[test]
fn t_a_33_嵌套字面量省略类型() {
    let outer = complit_of(&one("[][]int{{1}, {2}}"));
    assert_eq!(outer.elems.len(), 2, "两个内层元素，got {:?}", outer.elems);
    for (i, el) in outer.elems.iter().enumerate() {
        let inner = complit_of(el);
        assert!(
            inner.type_.is_none(),
            "第 {i} 个内层字面量该**省略**类型，got {:?}",
            inner.type_
        );
        assert_eq!(inner.elems.len(), 1, "第 {i} 个内层各有一个元素");
    }
}

/// 尾随逗号可选：`[]int{1, 2,}` 与没有尾逗号**结果相同**。
#[test]
fn t_a_33_尾随逗号可选() {
    let a = complit_of(&one("[]int{1, 2,}"));
    let b = complit_of(&one("[]int{1, 2}"));
    assert_eq!(a.elems.len(), 2, "尾逗号不产生第三个元素");
    assert_eq!(a.elems.len(), b.elems.len());
    assert_eq!(a.elems, b.elems);
}

/// 空元素表：`T{}` —— `elems` 为空**且** `type_` 为 `Some`（不是 `None`）。
#[test]
fn t_a_33_空元素表() {
    let lit = complit_of(&one("T{}"));
    assert!(lit.elems.is_empty(), "got {:?}", lit.elems);
    assert!(lit.type_.is_some(), "空元素表**不**意味着省略类型");
}

/// 位置：`CompositeLit.pos` 落在 **`{`**、`rbrace` 落在 **`}`**。
///
/// `const x = T{}` → `{` 第 12 列、`}` 第 13 列。
/// 两者**必须不同**：都取 `{` 的实现会让「同一个节点」看起来自洽。
#[test]
fn t_a_33_位置落在左右花括号() {
    let lit = complit_of(&one("T{}"));
    assert_eq!(lit.pos.col, 12, "`{{` 所在列");
    assert_eq!(lit.rbrace.col, 13, "`}}` 所在列");
}

/// **括号包住的类型**：`(T){}` 报一条，且 `type_` 保留**带括号**的那棵树。
///
/// 官方 `:1242-1245` 报 `cannot parenthesize type in composite literal`，
/// 然后 `n.Type = x`（`x` 是**原样**的 `ParenExpr`，不是 `Unparen` 后的 `T`）。
/// 只报错却把括号丢掉，会让 dump 与 oracle 少一层、逐行 diff 歪。
#[test]
fn t_a_33_括号包住的类型报错且保留括号() {
    let (v, n, msgs) = vals("(T){}");
    assert_eq!(n, 1, "恰好一条（`cannot parenthesize…`），got {msgs}");
    assert!(
        msgs.contains("cannot parenthesize type in composite literal"),
        "消息该是官方那条，got {msgs}"
    );
    let lit = complit_of(&v[0]);
    match lit.type_.as_deref() {
        Some(Expr::Paren(p)) => assert_eq!(name_of(&p.x), "T"),
        other => panic!("`Type` 该保留 ParenExpr，got {other:?}"),
    }
}

// ══════════════════════════════════════════════════════════════════════════
// `T-A-36` FuncLit（`AST-14`）
// ══════════════════════════════════════════════════════════════════════════

/// 匿名函数字面量：`func() {}` —— `type_` 是 `FuncType`，`body` 是空块。
#[test]
fn t_a_36_匿名函数字面量() {
    let f = funclit_of(&one("func() {}"));
    match &*f.type_ {
        Expr::FuncType(ft) => {
            assert!(ft.params.is_empty(), "无参数，got {:?}", ft.params);
            assert!(ft.results.is_empty(), "无返回值，got {:?}", ft.results);
        }
        other => panic!("`Type` 该是 FuncType，got {other:?}"),
    }
    assert!(f.body.stmts.is_empty(), "空函数体，got {:?}", f.body.stmts);
}

/// 带参数与返回值：`func(a int) int {}`（函数体为空，所以不依赖 `T-AP14`）。
#[test]
fn t_a_36_带参数与返回值() {
    let f = funclit_of(&one("func(a int) int {}"));
    match &*f.type_ {
        Expr::FuncType(ft) => {
            assert_eq!(ft.params.len(), 1, "一个参数，got {:?}", ft.params);
            assert_eq!(
                ft.params[0].name.as_ref().map(|n| n.value.as_str()),
                Some("a")
            );
            assert_eq!(
                name_of(&ft.params[0].type_),
                "int",
                "`Field.type_` 是 `Expr`（**不是** Option）⇒ `func(a)` 那种「只给名字不给类型」的中途形态由 `PartialField` 在 `paramList` 内部承担"
            );
            assert_eq!(ft.results.len(), 1, "一个返回值，got {:?}", ft.results);
        }
        other => panic!("`Type` 该是 FuncType，got {other:?}"),
    }
}

/// **位置**：`FuncLit.pos` 落在 **`func`** 关键字上（官方 `:1054` 的 `pos := p.pos()` 在 `p.next()` **之前**）。
///
/// `const x = func() {}` → `func` 在第 11 列。
#[test]
fn t_a_36_位置落在_func_关键字() {
    let f = funclit_of(&one("func() {}"));
    assert_eq!(f.pos.col, 11, "`func` 所在列");
}

/// **没有函数体就不是字面量**：`func(a int) int` 是**函数类型**（`Expr::FuncType`）。
///
/// 对照上一组：同一个前缀、只差一个 `{`，变体必须不同。
#[test]
fn t_a_36_无函数体是函数类型() {
    let e = one("func(a int) int");
    match e {
        Expr::FuncType(ft) => {
            assert_eq!(ft.params.len(), 1);
            assert_eq!(ft.results.len(), 1);
        }
        other => panic!("无 `{{` ⇒ 该是 FuncType，不是 FuncLit，got {other:?}"),
    }
}

/// 函数体是一个**块语句**（`block_stmt`），`pos`/`rbrace` 都要落在真花括号上。
#[test]
fn t_a_36_函数体是块语句() {
    let f = funclit_of(&one("func() {}"));
    assert_eq!(f.body.pos.col, 18, "函数体 `{{` 所在列");
    assert_eq!(f.body.rbrace.col, 19, "函数体 `}}` 所在列");
    assert!(f.body.rbrace.col > f.body.pos.col);
}

/// 字面量可作实参，**且后面的实参照常被解析**。
///
/// 这条钉 `funcBody` 之后 `xnest`/`fnest` 都**回到原值** ——
/// 少一次 `-= 1` 的话后续实参仍在「函数体内」，`stopset` 会提前把解析截断。
#[test]
fn t_a_36_作实参且后续实参照常解析() {
    let c = call_of(&one("f(func() {}, 1)"));
    assert_eq!(c.args.len(), 2, "两个实参，got {:?}", c.args);
    assert!(matches!(c.args[0], Expr::FuncLit(_)), "第一个是函数字面量");
    match &c.args[1] {
        Expr::BasicLit(l) => assert_eq!(l.kind, LitKind::Int),
        other => panic!("第二个该是整数字面量（证明没被吞掉），got {other:?}"),
    }
}

/// 函数**类型**作参数类型：`func(g func(int) int) {}`。
///
/// 这条证明 `paramList` → `type_` → `funcType` 与 `funcBody` 三者在同一棵树里接得上。
#[test]
fn t_a_36_参数类型可以是函数类型() {
    let f = funclit_of(&one("func(g func(int) int) {}"));
    match &*f.type_ {
        Expr::FuncType(ft) => {
            assert_eq!(ft.params.len(), 1);
            match &ft.params[0].type_ {
                Expr::FuncType(inner) => {
                    assert_eq!(inner.params.len(), 1);
                    assert_eq!(inner.results.len(), 1);
                }
                other => panic!("参数的 `Type` 该是 FuncType，got {other:?}"),
            }
        }
        other => panic!("`Type` 该是 FuncType，got {other:?}"),
    }
}

// ══════════════════════════════════════════════════════════════════════════
// `T-A-38` 与 `AST-09`：`args` 是**逐元素**解析，坏元素产出 `BadExpr`
// ══════════════════════════════════════════════════════════════════════════

/// `argList` 是 `arg { "," arg }` —— **不是** `exprList`。
///
/// 所以 `f(a, b)` 的 `args` 是**两个**元素，而**不是**一个 `Expr::List`。
/// 写成 `expr_list` 的话 dump 会多一层/少一层（`ListExpr` 扁平化规则会吃掉它），
/// 症状是「节点数悄悄变少」——不报错。
#[test]
fn t_a_38_实参表逐元素而不是列表() {
    let c = call_of(&one("f(a, b, c)"));
    assert_eq!(c.args.len(), 3, "三个**独立**实参，got {:?}", c.args);
    for (i, a) in c.args.iter().enumerate() {
        assert!(
            !matches!(a, Expr::List(_)),
            "第 {i} 个实参不该被包成 ListExpr，got {a:?}"
        );
    }
}

/// 坏实参：`f(,)` ⇒ 报一条 + 占位 `Expr::Bad`（**照常产出节点**）。
#[test]
fn t_a_38_坏实参产出_bad_expr() {
    let (v, n, msgs) = vals("f(,)");
    assert_eq!(n, 1, "恰好一条 `expected expression`，got {msgs}");
    assert!(msgs.contains("expected expression"), "got {msgs}");
    let c = call_of(&v[0]);
    assert_eq!(c.args.len(), 1, "占位元素仍要收进去，got {:?}", c.args);
    assert!(matches!(c.args[0], Expr::Bad(_)), "got {:?}", c.args[0]);
}

/// 复合字面量里的坏元素同样产出占位：`T{, 1}` ⇒ 一条 + 一个 `Bad`。
///
/// ⚠️ **`1` 会被恢复过程跳过**，所以只剩**一个**元素。这不是 bug：
/// 官方的恢复是 `advance(_Rparen, _Rbrack, _Rbrace)`，而 `,` 与 `1`
/// **都不在** followset 里 ⇒ 两个都被吃掉、停在 `}` 上。
/// 写清楚免得后来者把它当"少解析了一个元素"去修。
#[test]
fn t_a_33_坏元素产出_bad_expr() {
    let (v, n, msgs) = vals("T{, 1}");
    assert_eq!(n, 1, "恰好一条 `expected expression`，got {msgs}");
    assert!(msgs.contains("expected expression"), "got {msgs}");
    let lit = complit_of(&v[0]);
    assert_eq!(
        lit.elems.len(),
        1,
        "`,` 与 `1` 都被恢复跳过 ⇒ 只剩占位，got {:?}",
        lit.elems
    );
    assert!(
        matches!(lit.elems[0], Expr::Bad(_)),
        "got {:?}",
        lit.elems[0]
    );
    // `const x = T{, 1}` → `T` 第 11 列、`{` 第 12 列、`}` 第 16 列。
    assert_eq!(lit.rbrace.col, 16, "`}}` 在第 16 列 ⇒ 恢复停在了闭括号上");
}

/// 复合字面量**之后**后缀链必须继续长：`[]int{1}[0]` ⇒ `Index(X = CompositeLit)`。
///
/// 这条钉 `pexpr` 的 `case _Lbrace` 是 `x = n` 而不是 `return n`
/// （官方 `:1247`）。写成 `return` 的话，这个 `[0]` 会落回语句层。
#[test]
fn t_a_33_复合字面量后接后缀() {
    let e = one("[]int{1}[0]");
    match &e {
        Expr::Index(ix) => {
            match &*ix.x {
                Expr::CompositeLit(lit) => assert_eq!(lit.elems.len(), 1),
                other => panic!("`Index.X` 该是 CompositeLit，got {other:?}"),
            }
            match &*ix.index {
                Expr::BasicLit(l) => assert_eq!(l.kind, LitKind::Int),
                other => panic!("下标该是整数字面量，got {other:?}"),
            }
        }
        other => panic!("期望 IndexExpr，got {other:?}"),
    }
}

/// 复合字面量里的**一元/二元表达式**元素：`T{-1, a + b}`。
///
/// 钉 `bare_complitexpr` 的 `else` 分支走的是 `expr()`（完整表达式），
/// 而不是 `unaryExpr()` —— 后者会让 `a + b` 被拆成两个元素。
#[test]
fn t_a_33_元素可以是完整表达式() {
    let lit = complit_of(&one("T{-1, a + b}"));
    assert_eq!(lit.elems.len(), 2, "got {:?}", lit.elems);
    match &lit.elems[0] {
        Expr::Unary(o) => assert_eq!(o.op, Op::Sub),
        other => panic!("第一个元素该是一元 `-`，got {other:?}"),
    }
    match &lit.elems[1] {
        Expr::Binary(o) => assert_eq!(o.op, Op::Add),
        other => panic!("第二个元素该是二元 `+`，got {other:?}"),
    }
}
