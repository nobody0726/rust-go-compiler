//! `T-A-30` / `T-A-31` / `T-A-34` / `T-A-37` —— 表达式族（`AST-10..23`）。
//!
//! 全部走 **`parse_file`**：`const` / `var` 的 `Values` 是 M2 里表达式**唯一**能从
//! 文件入口观察到的位置（函数体要等 `T-AP09`/`T-AP13`）。这样本文件不需要
//! 「在 crate 内开 `#[cfg(test)]` 直接驱动 `pub(crate)` 函数」这类例外。
//!
//! # 断言的组织方式
//!
//! 每条断言都**双向**：既钉「进来的形状」（`Expr::Binary` 而非 `Expr::Unary`、
//! `y` 是 `Some` 而非 `None`），也钉「子结构」（`*` 在内、`+` 在外）。
//! 只钉一侧的话，把 `binary_expr` 写成错误结合方向仍然全绿。

use rgoc_ast::{Decl, Expr, LitKind, Op};
use rgoc_parse::{ParseResult, parse_file};

/// 解析 `package p` + 一条 `const x = <expr>`，返回那条声明的值列表。
fn const_values(expr_src: &str) -> Vec<Expr> {
    let src = format!("package p\n\nconst x = {expr_src}\n");
    let r = parse_file(src.as_bytes(), "t.go");
    assert_eq!(r.errors, 0, "`{expr_src}` 该是干净的：{:?}", r.diags);
    let f = r
        .file
        .unwrap_or_else(|| panic!("应产出 AstFile：{:?}", r.diags));
    match f.decls.as_slice() {
        [Decl::Const(c)] => c.values.clone(),
        other => panic!("期望恰好一条 ConstDecl，got {other:?}"),
    }
}

/// 取 `const x = <expr>` 里**唯一**的那个值表达式。
fn one(expr_src: &str) -> Expr {
    let v = const_values(expr_src);
    assert_eq!(v.len(), 1, "`{expr_src}` 只有一个值，got {v:?}");
    v.into_iter().next().expect("len 已断言为 1")
}

/// 拆一个**二元** `Operation` → `(op, x, y)`。
fn bin(e: &Expr) -> (Op, Expr, Expr) {
    match e {
        Expr::Binary(o) => {
            let y = o.y.as_deref().expect("二元运算必须有 y").clone();
            (o.op, (*o.x).clone(), y)
        }
        other => panic!("期望**二元** Operation，got {other:?}"),
    }
}

/// 拆一个**一元** `Operation` → `(op, x)`，并断言 `y == None`。
///
/// 「`y == None`」这一半不是废话：一元/二元在官方是**同一个** `Operation`
/// （靠 `Y == nil` 区分），我们拆成两个变体 ⇒ **拆分本身**必须被钉住。
fn un(e: &Expr) -> (Op, Expr) {
    match e {
        Expr::Unary(o) => {
            assert!(o.y.is_none(), "一元运算的 `Y` 必须是 None，got {:?}", o.y);
            (o.op, (*o.x).clone())
        }
        other => panic!("期望**一元** Operation，got {other:?}"),
    }
}

fn name_of(e: &Expr) -> &str {
    match e {
        Expr::Name(n) => &n.value,
        other => panic!("期望 Name，got {other:?}"),
    }
}

// ── `T-A-30` Name / BasicLit（`AST-10/11`）────────────────────────────────

/// 标识符。**位置**落在标识符起点。
#[test]
fn t_a_30_标识符() {
    let e = one("y");
    match &e {
        Expr::Name(n) => {
            assert_eq!(n.value, "y");
            assert_eq!(n.pos.line, 3, "值表达式在 `const x = y` 那一行");
        }
        other => panic!("期望 Name，got {other:?}"),
    }
}

/// 五种字面量类别（`AST-11`）。**值不解码**（M3 折叠），只钉类别与 `bad`。
#[test]
fn t_a_30_五种字面量类别() {
    for (src, want) in [
        ("1", LitKind::Int),
        ("1.5", LitKind::Float),
        ("1i", LitKind::Imag),
        ("'a'", LitKind::Rune),
        ("\"s\"", LitKind::String),
    ] {
        match one(src) {
            Expr::BasicLit(l) => {
                assert_eq!(l.kind, want, "`{src}` 的类别");
                assert!(!l.bad, "`{src}` 是合法字面量，bad 不该为真");
            }
            other => panic!("`{src}` 期望 BasicLit，got {other:?}"),
        }
    }
}

/// 字面量原文**含引号**（`T-A-30` 的「原文」半边；值 M3 才折叠）。
#[test]
fn t_a_30_字面量存原文() {
    match one("\"ab\"") {
        Expr::BasicLit(l) => assert_eq!(l.value, "\"ab\"", "含两端的双引号"),
        other => panic!("期望 BasicLit，got {other:?}"),
    }
}

// ── `T-A-31` Operation：优先级爬升 / 结合性 / 一元（`AST-21`）──────────────

/// **RF2 钉点**：`a + b * c` —— `*` 在内、`+` 在外。
///
/// 两侧都断言：只看「外层是 `+`」被「外层 `*`」的实现骗过；
/// 只看「内层是 `*`」被「只有一个节点」的实现骗过。
#[test]
fn t_a_31_rf2_优先级爬升_乘在内加在外() {
    let e = one("a + b * c");
    let (op, x, y) = bin(&e);
    assert_eq!(op, Op::Add, "外层是 `+`");
    assert_eq!(name_of(&x), "a");
    let (op2, x2, y2) = bin(&y);
    assert_eq!(op2, Op::Mul, "内层是 `*`");
    assert_eq!(name_of(&x2), "b");
    assert_eq!(name_of(&y2), "c");
}

/// 对照：`a * b + c` —— 这颗树**外层是 `+`**，但内层在**左**边上。
///
/// 少了这条，把 `binary_expr` 写成"永远往右挂"也能过上面那条。
#[test]
fn t_a_31_优先级爬升_对照_乘在左() {
    let e = one("a * b + c");
    let (op, x, y) = bin(&e);
    assert_eq!(op, Op::Add, "`+` 优先级最低 ⇒ 它是外层");
    let (op2, x2, y2) = bin(&x);
    assert_eq!(op2, Op::Mul);
    assert_eq!(name_of(&x2), "a");
    assert_eq!(name_of(&y2), "b");
    assert_eq!(name_of(&y), "c");
}

/// **左结合**：`a - b - c` ⇒ `(a - b) - c`（不是 `a - (b - c)`）。
#[test]
fn t_a_31_同优先级左结合() {
    let e = one("a - b - c");
    let (op, x, y) = bin(&e);
    assert_eq!(op, Op::Sub);
    assert_eq!(name_of(&y), "c", "最外层右边是**最后**那个操作数");
    let (op2, x2, y2) = bin(&x);
    assert_eq!(op2, Op::Sub, "左边又是一个 `-` ⇒ 左结合");
    assert_eq!(name_of(&x2), "a");
    assert_eq!(name_of(&y2), "b");
}

/// 三档优先级的相对高低：`a || b && c == d` ⇒ `||` 最外，`&&` 居中，`==` 最内。
///
/// 一条链把 `precOrOr(1) < precAndAnd(2) < precCmp(3)` 全部钉住。
#[test]
fn t_a_31_三档优先级层层包裹() {
    let e = one("a || b && c == d");
    let (op, x, y) = bin(&e);
    assert_eq!(op, Op::LOr);
    assert_eq!(name_of(&x), "a");
    let (op2, x2, y2) = bin(&y);
    assert_eq!(op2, Op::LAnd);
    assert_eq!(name_of(&x2), "b");
    let (op3, x3, y3) = bin(&y2);
    assert_eq!(op3, Op::Eql);
    assert_eq!(name_of(&x3), "c");
    assert_eq!(name_of(&y3), "d");
}

/// 一元运算：`*` / `+` / `-` / `!` / `^` 都是合法的**前缀**运算符。
///
/// 这五个是官方 `unaryExpr` 里 `case Mul, Add, Sub, Not, Xor` 那一条的全部。
#[test]
fn t_a_31_五种一元运算() {
    for (src, want, operand) in [
        ("-y", Op::Sub, "y"),
        ("+y", Op::Add, "y"),
        ("!y", Op::Not, "y"),
        ("^y", Op::Xor, "y"),
        ("*p", Op::Mul, "p"),
    ] {
        let (op, x) = un(&one(src));
        assert_eq!(op, want, "`{src}` 的运算符");
        assert_eq!(name_of(&x), operand, "`{src}` 的操作数");
    }
}

/// `&x` 单独一类：官方先 `Unparen` 再包 `&`（为了让 `&T{}` 拿不到括号）。
/// 对纯名字而言结果一样，但**它走的是不同分支**，所以要单独钉。
#[test]
fn t_a_31_取地址运算() {
    let (op, x) = un(&one("&y"));
    assert_eq!(op, Op::And);
    assert_eq!(name_of(&x), "y");
}

/// `&` 会**剥掉**操作数外层的括号（官方 `x.X = Unparen(p.unaryExpr())`）。
///
/// 用 `var` 而不是 `const`：`&a` 不是常量（`&(a)` 走的是地址取用）。
/// go1.27.1 实测接受 `var x = &(a)`。
///
/// 少了这条，「`&` 支不调 `unparen`」的实现照样全绿（`&y` 本来就没有括号）。
#[test]
fn t_a_31_取地址先剥括号() {
    let src = "package p\n\nvar a = 1\n\nvar x = &(a)\n";
    let r = parse_file(src.as_bytes(), "t.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    let f = r.file.expect("应产出文件");
    let last_var = f
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Var(v) => Some(v),
            _ => None,
        })
        .next_back()
        .expect("第二条 `var`");
    let (op, x) = un(&last_var.values[0]);
    assert_eq!(op, Op::And);
    // `name_of` 会对非 `Name` 的情形炸出来，消息里带实际节点 ⇒ 足够定位
    assert_eq!(name_of(&x), "a", "外层括号必须被剥掉（官方 `Unparen`）");
}

/// 一元**优先于**二元：`-a * b` ⇒ `(-a) * b`。
#[test]
fn t_a_31_一元比二元紧() {
    let e = one("-a * b");
    let (op, x, y) = bin(&e);
    assert_eq!(op, Op::Mul, "外层必须是 `*`");
    assert_eq!(name_of(&y), "b");
    let (op2, x2) = un(&x);
    assert_eq!(op2, Op::Sub, "左边是先算出来的一元 `-`");
    assert_eq!(name_of(&x2), "a");
}

/// 一元**可叠加**：`- -a` ⇒ `-(-a)`（官方 `x.X = p.unaryExpr()` 是递归）。
///
/// # ⚠️ 这里**必须**写成 `- -a`（中间留空格），不能写 `--a`
///
/// go1.27.1 实测（本次用容器里的 `go build` 核过）：
///
/// | 写法 | 结果 |
/// |---|---|
/// | `const x = - -a` | ✅ 两个一元 `-` |
/// | `const x = -(-a)` | ✅ |
/// | `const x = --a` | ❌ **词法**就把 `--` 收成一个 token（`_IncOp`） |
///
/// 即 `--` / `++` 是**单个词元** ⇒ `--a` 不是「两次取负」，`unaryExpr` 根本看不到
/// 两个 `-`。写成 `--a` 会让这条用例变成「测词法」，而且会以一条 `expected expression`
/// 的形态红 —— 看起来像解析器坏了。
#[test]
fn t_a_31_一元可叠加() {
    let (op, x) = un(&one("- -a"));
    assert_eq!(op, Op::Sub, "外层 `-`");
    let (op2, x2) = un(&x);
    assert_eq!(op2, Op::Sub, "内层又是一个 `-`");
    assert_eq!(name_of(&x2), "a");
}

/// 对照（负例）：`--a` **不是**两次取负 —— 词法层已经把它收成 `_IncOp`。
///
/// 这条把上面那条的边界钉死：少了它，把 `--a` 也解析成 `-(-a)` 的实现照样全绿。
#[test]
fn t_a_31_双减号是单个词元不是两次取负() {
    // `const x = --a` ⇒ 一条「expected expression」（`--` 不能当前缀运算符）。
    let src = "package p\n\nconst x = --a\n";
    let r = parse_file(src.as_bytes(), "t.go");
    assert_eq!(r.errors, 1, "`--` 是单个词元 ⇒ 报一条，got {:?}", r.diags);
    let f = r.file.expect("报错但照常产出文件");
    match f.decls.as_slice() {
        [Decl::Const(c)] => assert!(
            matches!(c.values.as_slice(), [Expr::Bad(_)]),
            "落在 `operand` 的兜底臂 ⇒ BadExpr，got {:?}",
            c.values
        ),
        other => panic!("期望一条 ConstDecl，got {other:?}"),
    }
}

/// **位置**：`Operation` 的 `pos` 落在**运算符**上，不是左侧表达式上。
///
/// 官方 `binaryExpr` 是 `t.pos = p.pos()` 且此时尚未 `p.next()` ⇒ 位置是**运算符**。
/// `const x = a + b` 里 `+` 在第 13 列。写成「左操作数起点」是不会报错的错法。
#[test]
fn t_a_31_二元运算位置落在运算符上() {
    // `const x = a + b`
    //  1234567890123456
    //            a +
    let e = one("a + b");
    match &e {
        Expr::Binary(o) => assert_eq!(o.pos.col, 13, "`+` 所在列"),
        other => panic!("期望 Binary，got {other:?}"),
    }
}

/// **词法运算符 → AST 运算符**的映射表逐项抽查。
///
/// 两个枚举的名字**故意不同**（官方 `Div` / 我们 `Quo`，官方 `Recv` / 我们 `Arrow`）
/// ⇒ 映射函数是"照名字抄一遍"最容易抄错的地方，而抄错**不会报错**
/// （`a / b` 变成 `a % b` 仍然是一棵合法的树）。
#[test]
fn t_a_31_运算符映射逐项抽查() {
    for (src, want) in [
        ("a / b", Op::Quo),
        ("a % b", Op::Rem),
        ("a & b", Op::And),
        ("a | b", Op::Or),
        ("a ^ b", Op::Xor),
        ("a << b", Op::Shl),
        ("a >> b", Op::Shr),
        ("a &^ b", Op::AndNot),
        ("a && b", Op::LAnd),
        ("a || b", Op::LOr),
        ("a == b", Op::Eql),
        ("a != b", Op::Neq),
        ("a < b", Op::Lss),
        ("a <= b", Op::Leq),
        ("a > b", Op::Gtr),
        ("a >= b", Op::Geq),
    ] {
        let (op, _, _) = bin(&one(src));
        assert_eq!(op, want, "`{src}` 的运算符");
    }
}

// ── `T-A-37` ParenExpr（`AST-15`）─────────────────────────────────────────

/// `(x)` —— **求值分组**，保留成 `ParenExpr`。
///
/// 官方 `unaryExpr` 调 `pexpr(nil, true)`（`keep_parens = true`），
/// 所以表达式上下文里的括号**会**留下节点。
#[test]
fn t_a_37_括号保留成节点() {
    let e = one("(1)");
    match &e {
        Expr::Paren(p) => match &*p.x {
            Expr::BasicLit(l) => assert_eq!(l.kind, LitKind::Int),
            other => panic!("括号里是字面量，got {other:?}"),
        },
        other => panic!("期望 ParenExpr，got {other:?}"),
    }
}

/// 括号**参与优先级**：`(a + b) * c` ⇒ 外层 `*`、内层 `+`。
///
/// 这条与 `t_a_31_rf2_…` 恰好相反 ⇒ 两侧一起才说明括号真的改变了结合。
#[test]
fn t_a_37_括号改变结合方向() {
    let e = one("(a + b) * c");
    let (op, x, y) = bin(&e);
    assert_eq!(op, Op::Mul, "外层是 `*`");
    assert_eq!(name_of(&y), "c");
    match &x {
        Expr::Paren(p) => match &*p.x {
            Expr::Binary(o) => assert_eq!(o.op, Op::Add),
            other => panic!("括号里应是 `+`，got {other:?}"),
        },
        other => panic!("期望 ParenExpr，got {other:?}"),
    }
}

// ── `T-A-34` Selector / Index / Slice（`AST-16/17/18`）──────────────────────

#[test]
fn t_a_34_选择器() {
    let e = one("a.b");
    match &e {
        Expr::Selector(s) => {
            assert_eq!(name_of(&s.x), "a");
            assert_eq!(s.sel.value, "b");
        }
        other => panic!("期望 SelectorExpr，got {other:?}"),
    }
}

/// **位置**：`SelectorExpr.pos` 落在 **`.`** 上（官方 `pexpr` 在进 `switch` 前取 `pos`）。
#[test]
fn t_a_34_选择器位置落在点上() {
    // `const x = a.b`  →  `.` 在第 12 列
    let e = one("a.b");
    match &e {
        Expr::Selector(s) => assert_eq!(s.pos.col, 12, "`.` 所在列"),
        other => panic!("期望 SelectorExpr，got {other:?}"),
    }
}

#[test]
fn t_a_34_索引() {
    let e = one("a[1]");
    match &e {
        Expr::Index(i) => {
            assert_eq!(name_of(&i.x), "a");
            match &*i.index {
                Expr::BasicLit(l) => assert_eq!(l.kind, LitKind::Int),
                other => panic!("下标应是字面量，got {other:?}"),
            }
        }
        other => panic!("期望 IndexExpr（不是 SliceExpr），got {other:?}"),
    }
}

/// 切片的四种形态一次钉全：`[i:j]` / `[:j]` / `[i:]` / `[i:j:k]`。
///
/// `full` 标志只有三下标形式才为真 —— 它是 `SliceExpr` 上唯一一个**不是** `Option`
/// 的字段，最容易被写成恒 `false` 还全绿。
#[test]
fn t_a_34_切片四种形态() {
    for (src, low, high, full) in [
        ("a[1:2]", true, true, false),
        ("a[:2]", false, true, false),
        ("a[1:]", true, false, false),
        ("a[1:2:3]", true, true, true),
    ] {
        let e = one(src);
        match &e {
            Expr::Slice(s) => {
                assert_eq!(s.low.is_some(), low, "`{src}` 的 Low");
                assert_eq!(s.high.is_some(), high, "`{src}` 的 High");
                assert_eq!(s.full, full, "`{src}` 的 Full");
                if full {
                    assert!(s.max.is_some(), "`{src}` 有第三个下标");
                } else {
                    assert!(s.max.is_none(), "`{src}` 没有第三个下标");
                }
            }
            other => panic!("`{src}` 期望 SliceExpr，got {other:?}"),
        }
    }
}

/// 后缀**链式**：`a.b[1].c` ⇒ `((a.b)[1]).c`（逐层从内到外）。
///
/// 四层全部逐层断言 —— 只写「最外层是 `.c`」的话，把后缀链写成"每次从头开始"
/// 也能过。
#[test]
fn t_a_34_后缀链() {
    let e = one("a.b[1].c");
    match &e {
        Expr::Selector(s0) => {
            assert_eq!(s0.sel.value, "c", "最外层选择 `.c`");
            match &*s0.x {
                Expr::Index(i) => {
                    match &*i.x {
                        Expr::Selector(s1) => {
                            assert_eq!(name_of(&s1.x), "a", "最内层是 `a`");
                            assert_eq!(s1.sel.value, "b");
                        }
                        other => panic!("倒数第二层应是 SelectorExpr，got {other:?}"),
                    }
                    match &*i.index {
                        Expr::BasicLit(l) => assert_eq!(l.kind, LitKind::Int),
                        other => panic!("下标应是字面量，got {other:?}"),
                    }
                }
                other => panic!("中间层应是 IndexExpr，got {other:?}"),
            }
        }
        other => panic!("期望最外层 SelectorExpr，got {other:?}"),
    }
}

// ── `T-A-22` / `T-A-24` 的 Values 侧（`const` / `var` 真的接上了）──────────

/// `const x, y = 1, 2`：**两个名字、两个值**。
///
/// 官方 `Values` 是**一个** `exprList`（多值时是 `ListExpr`），我们的模型是
/// `Vec<Expr>` ⇒ 这里钉的是「C6 归一前的原生形态能被正确拆开」。
#[test]
fn t_a_22_const_多名字多值() {
    let src = "package p\n\nconst x, y = 1, 2\n";
    let r = parse_file(src.as_bytes(), "t.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    match r.file.expect("应产出文件").decls.as_slice() {
        [Decl::Const(c)] => {
            assert_eq!(c.names.len(), 2, "两个名字");
            assert_eq!(c.names[1].value, "y");
            assert_eq!(c.values.len(), 2, "两个值");
        }
        other => panic!("期望一条 ConstDecl，got {other:?}"),
    }
}

/// `const x = a, b`：**一个名字、两个值** —— 语法合法（语义错，不归 M2）。
///
/// 它专门钉 `exprList` 的 `ListExpr` 分支：少写那个分支，`b` 会被留在流里，
/// 于是顶层循环报一条 `after top level declaration` —— **诊断数与树形同时变**。
#[test]
fn t_a_22_const_一个名字两个值() {
    let v = const_values("a, b");
    assert_eq!(v.len(), 2, "`a, b` 是 ListExpr ⇒ 拆成两个值");
    assert_eq!(name_of(&v[0]), "a");
    assert_eq!(name_of(&v[1]), "b");
}

/// `var z = (3)`：`var` 的 **`=` 分支**（无类型）也要接上。
#[test]
fn t_a_24_var_无类型有值() {
    let src = "package p\n\nvar z = (3)\n";
    let r = parse_file(src.as_bytes(), "t.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    match r.file.expect("应产出文件").decls.as_slice() {
        [Decl::Var(v)] => {
            assert_eq!(v.names.len(), 1);
            assert!(v.type_.is_none(), "无类型");
            assert_eq!(v.values.len(), 1);
            assert!(
                matches!(v.values[0], Expr::Paren(_)),
                "`(3)` 要保留成 ParenExpr，got {:?}",
                v.values[0]
            );
        }
        other => panic!("期望一条 VarDecl，got {other:?}"),
    }
}

/// `var x int = 1`：**有类型**的分支。类型名走 `typeOrNil` 的 `_Name` 臂。
#[test]
fn t_a_24_var_有类型有值() {
    let src = "package p\n\nvar x int = 1\n";
    let r = parse_file(src.as_bytes(), "t.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    match r.file.expect("应产出文件").decls.as_slice() {
        [Decl::Var(v)] => {
            assert_eq!(name_of(v.type_.as_ref().expect("有类型")), "int", "类型名");
            assert_eq!(v.values.len(), 1);
        }
        other => panic!("期望一条 VarDecl，got {other:?}"),
    }
}

/// `var x := 1`：官方 `gotAssign` 对 `:=` **报一条并照常消费**（`fallthrough`）。
///
/// go1.27.1 实测的错误正是 `syntax error: unexpected :=, expected =`（恰好一条）。
/// 只报错不消费的实现会让 `1` 留在流里 ⇒ 顶层循环再多报一条，且**值解析不出来**。
#[test]
fn t_a_24_var_用冒号等号报错但仍解析出值() {
    let src = "package p\n\nvar x := 1\n";
    let r = parse_file(src.as_bytes(), "t.go");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    match r.file.expect("报错但照常产出文件").decls.as_slice() {
        [Decl::Var(v)] => {
            assert!(v.type_.is_none(), "`:=` 之后没有类型");
            assert_eq!(v.values.len(), 1, "`:=` 被消费掉 ⇒ 值仍能解析出来");
        }
        other => panic!("期望一条 VarDecl，got {other:?}"),
    }
}

/// `var x = a[b, c]`：`[b, c]` 造的 `ListExpr` 属于 `IndexExpr.index`，
/// **不该**被 `flatten_expr_list` 当成"多值"拆开。
///
/// 这条钉的是 `decl.rs` 那个转换的**边界**：如果它递归地找 `ListExpr`
/// （或 `pexpr` 的 `[` 臂把 `List` 提到了外层），值的个数会变成 2 —— 而
/// `errors` 仍然是 0，只有**树形**能区分。go1.27.1 的 `gofmt -e` 确认语法合法。
#[test]
fn t_a_34_索引里的列表不被当多值() {
    let src = "package p\n\nvar a, b, c = 1, 2, 3\n\nvar x = a[b, c]\n";
    let r = parse_file(src.as_bytes(), "t.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    let f = r.file.expect("应产出文件");
    let last_var = f
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Var(v) => Some(v),
            _ => None,
        })
        .next_back()
        .expect("最后一条 `var`");
    assert_eq!(last_var.values.len(), 1, "**一个**值（不是两个）");
    match &last_var.values[0] {
        Expr::Index(i) => assert!(
            matches!(&*i.index, Expr::List(_)),
            "下标本身是 List，got {:?}",
            i.index
        ),
        other => panic!("期望 IndexExpr，got {other:?}"),
    }
}

// ── `AST-09` BadExpr：`operand` 的兜底臂 ───────────────────────────────────

/// `const x = ;` ⇒ **一条**诊断 + 一个 `BadExpr` 占位。
///
/// 官方的兜底臂是 `x := p.badExpr(); p.syntaxError("expected expression");
/// p.advance(_Rparen, _Rbrack, _Rbrace)` —— **先造节点再报错**，
/// 所以畸形输入仍然产出一棵完整的树（对撞时两边的**节点数**才对得上）。
#[test]
fn t_a_09_不可解析处产出_bad_expr_并报一条() {
    let src = "package p\n\nconst x = ;\n";
    let r = parse_file(src.as_bytes(), "t.go");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    match r.file.expect("报错但照常产出文件").decls.as_slice() {
        [Decl::Const(c)] => {
            assert_eq!(c.values.len(), 1, "仍要产出**一个**占位值");
            assert!(matches!(c.values[0], Expr::Bad(_)), "got {:?}", c.values[0]);
        }
        other => panic!("期望一条 ConstDecl，got {other:?}"),
    }
}

/// `ParseResult` 要在**没有声明**时也自洽（空文件只留 package 子句）。
/// 这条顺带钉住 `file()` 不会因为「没有顶层声明」而误报。
#[test]
fn 只有_package_子句时不报错也无声明() {
    let r = parse_file(b"package p\n", "t.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    assert!(r.file.expect("应产出文件").decls.is_empty());
}

/// 上面所有用例共用的入口自检：`ParseResult` 的 `first` 与 `errors` 一致。
#[test]
fn 诊断计数与首错一致() {
    let r: ParseResult = parse_file(b"package p\n\nconst x = ;\n", "t.go");
    assert_eq!(r.errors, 1);
    assert!(r.first.is_some(), "首错要留下");
}
