//! `T-A-51`（ExprStmt/SendStmt）、`T-A-52`（AssignStmt）、`T-A-56`（IfStmt）、
//! `T-A-57`（ForStmt）、`T-A-58`（SwitchStmt/TypeSwitch）、`T-A-59`（SelectStmt /
//! RangeClause / CaseClause / CommClause）—— 即控制流与简单语句（`T-AP13`）。
//!
//! # 断言的组织方式
//!
//! 每一族都同时钉**变体 + 标志**与**子结构**：只钉 `Stmt::If(_)` 看不出
//! `cond` 是不是 `None`、看不出 `else` 是嵌套 `if` 还是块；
//! 只钉 `Stmt::For(_)` 看不出三个槽里哪个是 `range` 子句。
//!
//! # 位置判据全部来自官方 `nodes_test.go` 的 `@` 标记（权威，不是推测）
//!
//! | 节点 | 官方标记 | 结论 |
//! |---|---|---|
//! | `AssignStmt` | `x @= y` / `x@++` | pos 在**运算符**上（**不是**最左词元） |
//! | `RangeClause` | `i := @range s` | pos 在 **`range`** 上 |
//! | `IfStmt` / `ForStmt` / `SwitchStmt` / `SelectStmt` | `@if` / `@for` / `@switch` / `@select` | pos 在关键字上 |
//! | `CaseClause` / `CommClause` | `@case` / `@default` | pos 在 `case`/`default` 上 |
//! | `TypeSwitchGuard` | `x@.(type)` | pos 在 **`.`** 上 |
//!
//! # 为什么 `if x == T{} { f() }` 有一条专属用例（RF1）
//!
//! `header` 把 `xnest` 置 `-1`，语义是「语句头部里禁复合字面量」。那条用例是
//! 该规则**唯一**的端到端观测点：两种读法**都不报错**，只有树形不同 ⇒
//! 少了 `xnest = -1` 的实现不会让"能编译/不报错"类断言变红，只会让树形悄悄变样。

use rgoc_ast::{
    AssignOp, AssignStmt, AstFile, BlockStmt, Decl, Expr, ForStmt, FuncDecl, IfStmt, Op,
    SelectStmt, SimpleStmt, Stmt, SwitchStmt, dump,
};
use rgoc_parse::{ParseResult, parse_file};

fn parse(src: &str) -> ParseResult {
    parse_file(src.as_bytes(), "t.go")
}

/// 把语句体包进 `func f() { ... }` 里 —— 语句只能从函数体里观察到
/// （`stmtOrNil` 的生产入口是 `funcBody`，`T-AP09` 之后已接通）。
fn wrap(body: &str) -> String {
    format!("package p\nfunc f() {{\n{body}\n}}\n")
}

fn file_of(r: &ParseResult) -> &AstFile {
    r.file
        .as_ref()
        .unwrap_or_else(|| panic!("应产出 AstFile，诊断：{:?}", r.diags))
}

fn one_func(r: &ParseResult) -> &FuncDecl {
    let v: Vec<&FuncDecl> = file_of(r)
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Func(f) => Some(f),
            _ => None,
        })
        .collect();
    assert_eq!(v.len(), 1, "应恰好一条 func 声明，诊断：{:?}", r.diags);
    v[0]
}

fn body_block(r: &ParseResult) -> &BlockStmt {
    one_func(r).body.as_ref().expect("应有的函数体")
}

/// 解析一段语句体，**要求零诊断**，返回语句表（**拥有所有权**的克隆 ——
/// 借用 `ParseResult` 里的树会和随后的断言抢生命周期，而这个量级复制无成本）。
fn stmts(body: &str) -> Vec<Stmt> {
    let src = wrap(body);
    let r = parse(&src);
    assert_eq!(r.errors, 0, "干净输入不该报错：{body:?} ⇒ {:?}", r.diags);
    body_block(&r).stmts.clone()
}

/// [`stmts`] 的宽松版：只放过 `checkBranches` 的诊断（`T-AP16` 起）。
///
/// `L:` 这种「定义了但没用」的标签会被官方 `branches.go:42` 报成
/// `label L defined and not used`。本文件验的是**树形**（`pos` 落点、
/// `L: }` 造空语句），那些诊断与树形无关。
///
/// ⚠️ 精确分类（只放过 `SYN-BRANCH-*` / `SYN-LABEL-*` / `SYN-GOTO-*` 三族），
/// 不做成「吞掉全部诊断」—— 后者会让「`checkBranches` 悄悄不工作了」
/// **看不出来**。分支诊断本身由 `tests/test_branches.rs` 验。
fn stmts_branchy(body: &str) -> Vec<Stmt> {
    let src = wrap(body);
    let mut r = parse(&src);
    let diags: Vec<rgoc_lex::diag::Diag> = std::mem::take(&mut r.diags).into_sorted();
    let unexpected: Vec<&rgoc_lex::diag::Diag> = diags
        .iter()
        .filter(|d| {
            let c = d.code.as_str();
            !(c.starts_with("SYN-BRANCH-")
                || c.starts_with("SYN-LABEL-")
                || c.starts_with("SYN-GOTO-"))
        })
        .collect();
    assert!(
        unexpected.is_empty(),
        "只允许分支检查诊断：{body:?} ⇒ {unexpected:?}"
    );
    body_block(&r).stmts.clone()
}

/// 解析一段语句体，返回 `ParseResult`（允许有错 —— 用于恢复路径）。
fn stmts_raw(body: &str) -> ParseResult {
    parse(&wrap(body))
}

/// 首条诊断的消息（`DiagBag` **不可索引**）。
///
/// ⚠️ 官方 `syntaxError` 会在消息前**统一加** `syntax error: `
/// （`parser.go:271`：`p.errorAt(pos, "syntax error: "+msg)`）⇒ 期望串必须带上它。
/// 断言里不带前缀就等于**只**在比 `msg` 本体，会把「消息不是从 `syntaxError` 出来的」
/// 这一类错误放过去。
fn first_msg(r: &ParseResult) -> String {
    r.first
        .as_ref()
        .map(|d| d.message.clone())
        .unwrap_or_default()
}

/// 取恰好一条语句（多于/少于一条都炸 —— 免得"少解析了"被当成"解析对了"）。
fn one(body: &str) -> Stmt {
    let v = stmts(body);
    assert_eq!(v.len(), 1, "应恰好一条语句，got {v:?}");
    v.into_iter().next().expect("上面判过 len == 1")
}

fn name_of(e: &Expr) -> &str {
    match e {
        Expr::Name(n) => n.value.as_str(),
        other => panic!("不是 Name：{other:?}"),
    }
}

fn as_assign(s: Stmt) -> AssignStmt {
    match s {
        Stmt::Assign(a) => a,
        other => panic!("期望 AssignStmt，got {other:?}"),
    }
}

fn as_if(s: Stmt) -> IfStmt {
    match s {
        Stmt::If(i) => i,
        other => panic!("期望 IfStmt，got {other:?}"),
    }
}

fn as_for(s: Stmt) -> ForStmt {
    match s {
        Stmt::For(f) => f,
        other => panic!("期望 ForStmt，got {other:?}"),
    }
}

fn as_switch(s: Stmt) -> SwitchStmt {
    match s {
        Stmt::Switch(x) => x,
        other => panic!("期望 SwitchStmt，got {other:?}"),
    }
}

fn as_select(s: Stmt) -> SelectStmt {
    match s {
        Stmt::Select(x) => x,
        other => panic!("期望 SelectStmt，got {other:?}"),
    }
}

// ───────────────────────── T-A-51 ExprStmt / SendStmt ─────────────────────────

#[test]
fn t_a_51_表达式语句_pos_取表达式的_pos() {
    let v = stmts("f(x)");
    assert_eq!(v.len(), 1, "got {v:?}");
    match &v[0] {
        Stmt::Expr(e) => {
            // 官方 `simpleStmt` 的 `default` 臂（`:2196`）是 `s.pos = lhs.Pos()` ——
            // **不是** `p.pos()`（那一支里 `p.pos()` 确实被算出来了，但只给
            // `_AssignOp`/`_IncOp`/`_Arrow` 用）。
            //
            // 而 `lhs.Pos()` 对 `CallExpr` 是**那个 `(`**：
            // 官方 `pexpr`（`:1115/1212`）在循环顶取 `pos := p.pos()` 再
            // `t.pos = pos`，此时当前词元正是 `(`；
            // `nodes_test.go:158` 的 `f@()` 把这件事钉死了（`@` 在 `(` 上）。
            //
            // ⇒ 这条断言同时排除了两种错法：
            //   ① 用 `p.pos()`（此时是自动补出的 `;`，第 5 列）
            //   ② 用 `Fun.Pos()`（第 1 列，即 `f`）
            assert_eq!(
                (e.pos.line, e.pos.col),
                (3, 2),
                "pos 落在 `(`（官方 `ExprStmt.pos = lhs.Pos()`）"
            );
            assert!(matches!(e.x, Expr::Call(_)), "got {:?}", e.x);
        }
        other => panic!("期望 ExprStmt，got {other:?}"),
    }
}

/// 同一个 `lhs.Pos()` 在**非调用**表达式上退化成"最左词元" ——
/// 与上一条合起来才说明"取的是表达式的位置"而不是某个特例。
#[test]
fn t_a_51_表达式语句_pos_在裸名字上() {
    let v = stmts("x");
    assert_eq!(v.len(), 1, "got {v:?}");
    match &v[0] {
        Stmt::Expr(e) => assert_eq!((e.pos.line, e.pos.col), (3, 1), "pos 落在 `x`"),
        other => panic!("期望 ExprStmt，got {other:?}"),
    }
}

#[test]
fn t_a_51_发送语句() {
    let s = one("ch <- v");
    match s {
        Stmt::Send(s) => {
            assert_eq!(name_of(&s.chan), "ch");
            assert_eq!(name_of(&s.value), "v");
            // 官方 `pos := p.pos()` 在 `p.next()` **之前** ⇒ pos 是 `<-`。
            assert_eq!((s.pos.line, s.pos.col), (3, 4), "pos 落在 `<-`");
        }
        other => panic!("期望 SendStmt，got {other:?}"),
    }
}

/// 接收表达式 `<-ch` 当**语句**（`stmtOrNil` 的 `_Arrow` 臂 → `simpleStmt` →
/// 既不是 `=`/`:=` 也不是 `++`/`--`/`<-` ⇒ 裸表达式语句）。
#[test]
fn t_a_51_单个接收表达式是表达式语句() {
    let s = one("<-ch");
    match s {
        Stmt::Expr(e) => match e.x {
            Expr::Unary(ref o) => assert_eq!(o.op, Op::Arrow, "`<-ch` 是一元 `<-`"),
            ref other => panic!("期望一元 `<-`，got {other:?}"),
        },
        other => panic!("期望 ExprStmt，got {other:?}"),
    }
}

// ───────────────────────── T-A-52 AssignStmt ─────────────────────────

#[test]
fn t_a_52_赋值_等于() {
    let a = as_assign(one("x = 1"));
    assert_eq!(a.op, AssignOp::Assign);
    assert_eq!(a.lhs.len(), 1);
    assert_eq!(name_of(&a.lhs[0]), "x");
    assert_eq!(a.rhs.len(), 1);
    assert!(matches!(a.rhs[0], Expr::BasicLit(_)), "got {:?}", a.rhs[0]);
    // 官方 `nodes_test.go`：`x @= y` ⇒ pos 在 `=`（**不是**最左的 `x`）。
    assert_eq!((a.pos.line, a.pos.col), (3, 3));
}

#[test]
fn t_a_52_短变量声明() {
    let a = as_assign(one("x := 1"));
    assert_eq!(a.op, AssignOp::Define);
    assert_eq!((a.pos.line, a.pos.col), (3, 3), "pos 在 `:=`");
}

#[test]
fn t_a_52_多值赋值() {
    let a = as_assign(one("a, b, c = 1, 2, 3"));
    // 官方 `a, b, x @= 1, 2, 3` ⇒ lhs/rhs 都是**列表**（我们拆成 `Vec`）。
    assert_eq!(a.lhs.len(), 3);
    assert_eq!(a.rhs.len(), 3);
    assert_eq!(name_of(&a.lhs[2]), "c");
    assert_eq!((a.pos.line, a.pos.col), (3, 9), "pos 在 `=`");
}

#[test]
fn t_a_52_复合赋值() {
    for (src, op) in [
        ("x += 1", AssignOp::Add),
        ("x -= 1", AssignOp::Sub),
        ("x *= 1", AssignOp::Mul),
        ("x /= 1", AssignOp::Quo),
        ("x %= 1", AssignOp::Rem),
        ("x &= 1", AssignOp::And),
        ("x |= 1", AssignOp::Or),
        ("x ^= 1", AssignOp::Xor),
        ("x <<= 1", AssignOp::Shl),
        ("x >>= 1", AssignOp::Shr),
        ("x &^= 1", AssignOp::AndNot),
    ] {
        let a = as_assign(one(src));
        assert_eq!(a.op, op, "{src}");
        assert_eq!(a.rhs.len(), 1, "{src}");
        assert_eq!((a.pos.line, a.pos.col), (3, 3), "{src}: pos 在运算符上");
    }
}

#[test]
fn t_a_52_自增自减() {
    let a = as_assign(one("x++"));
    assert_eq!(a.op, AssignOp::Inc);
    assert!(a.rhs.is_empty(), "自增没有 rhs（`AST-39` 的约定）");
    // 官方 `nodes_test.go`：`x@++` ⇒ pos 在 `++`。
    assert_eq!((a.pos.line, a.pos.col), (3, 2));

    let d = as_assign(one("a[i]--"));
    assert_eq!(d.op, AssignOp::Dec);
    assert!(d.rhs.is_empty());
    assert_eq!((d.pos.line, d.pos.col), (3, 5), "pos 在 `--`");
}

/// `x, y z` —— 表达式列表后面跟了既不是 `=`/`:=` 也不是结束词元的东西。
///
/// ⚠️ 这条是 [`Parser::simple_stmt`] **路径二** 的 `default` 分支：
/// `lhs` 是列表时路径一被跳过，而后面又不是 `=`/`:=`。官方「make the best of
/// what we have」取列表**第一个**元素当表达式语句。
#[test]
fn t_a_52_表达式列表后不是赋值报错并取首元素() {
    let r = stmts_raw("x, y z");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    assert_eq!(
        first_msg(&r),
        "syntax error: unexpected name z, expected := or = or comma",
        "官方 `:2234` 的 `expected := or = or comma`；`T-AP16` 起 `expected ` 分支\
         ⇒ 前置 `unexpected <当前词元>, `（当前词元是那个多出来的 `z`）"
    );

    let b = body_block(&r);
    assert_eq!(b.stmts.len(), 1, "got {:?}", b.stmts);
    match &b.stmts[0] {
        Stmt::Expr(e) => assert_eq!(name_of(&e.x), "x", "取列表首元素"),
        other => panic!("期望 ExprStmt，got {other:?}"),
    }
}

// ───────────────────────── T-A-56 IfStmt ─────────────────────────

#[test]
fn t_a_56_无条件_if_报错并造_bad_expr() {
    let r = stmts_raw("if {}");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    assert_eq!(
        first_msg(&r),
        "syntax error: missing condition in if statement"
    );

    let i = as_if(body_block(&r).stmts[0].clone());
    let cond = i.cond.as_ref().expect("官方会造一个 badExpr");
    assert!(matches!(cond, Expr::Bad(_)), "got {cond:?}");
    assert_eq!(
        (cond.pos().line, cond.pos().col),
        (3, 4),
        "`badExpr` 的位置是那个 `{{`（走的是 `if p.tok == _Lbrace` 早退支）"
    );
}

#[test]
fn t_a_56_条件式_if() {
    let i = as_if(one("if x > 0 {}"));
    assert!(i.init.is_none(), "没有 init");
    // ⚠️ 条件是一个**二元式**，不是裸名字 —— 曾经的 `name_of(cond)` 会在这里炸。
    match i.cond.as_ref().expect("有条件") {
        Expr::Binary(o) => {
            assert_eq!(o.op, Op::Gtr);
            assert_eq!(name_of(&o.x), "x");
        }
        other => panic!("期望 `x > 0` 的二元式，got {other:?}"),
    }
    assert!(i.then.stmts.is_empty(), "空 then 块");
    assert!(i.els.is_none());
    // 官方 `nodes_test.go`：`@if cond {}` ⇒ pos 在 `if`。
    assert_eq!((i.pos.line, i.pos.col), (3, 1));
}

#[test]
fn t_a_56_带_init_的_if() {
    let i = as_if(one("if x := f(); x > 0 { g() }"));
    let init = i.init.as_ref().expect("有 init");
    match init {
        SimpleStmt::Assign(a) => {
            assert_eq!(a.op, AssignOp::Define);
            assert_eq!(name_of(&a.lhs[0]), "x");
        }
        other => panic!("期望 AssignStmt，got {other:?}"),
    }
    match i.cond.as_ref().expect("有条件") {
        Expr::Binary(o) => {
            assert_eq!(o.op, Op::Gtr);
            assert_eq!(name_of(&o.x), "x");
        }
        other => panic!("期望 `x > 0` 的二元式，got {other:?}"),
    }
    assert_eq!(i.then.stmts.len(), 1, "then 里有一条 `g()`");
    assert!(i.els.is_none(), "`if` 的 `header` 不产出 post");
}

#[test]
fn t_a_56_else_块() {
    let i = as_if(one("if x > 0 { f() } else { g() }"));
    let els = i.els.as_ref().expect("有 else");
    match &**els {
        Stmt::Block(b) => assert_eq!(b.stmts.len(), 1, "else 块里有一条 `g()`"),
        other => panic!("`else {{...}}` 应是 BlockStmt，got {other:?}"),
    }
}

#[test]
fn t_a_56_else_if_链() {
    let i = as_if(one("if a { } else if b { }"));
    let els = i.els.as_ref().expect("有 else");
    match &**els {
        // 官方递归调 `ifStmt` ⇒ `else if` 是**嵌套的** `IfStmt`，不是块。
        Stmt::If(inner) => {
            assert_eq!(name_of(inner.cond.as_ref().expect("内层有条件")), "b");
            assert_eq!((inner.pos.line, inner.pos.col), (3, 15), "内层 `if` 的位置");
        }
        other => panic!("`else if` 应是嵌套 IfStmt，got {other:?}"),
    }
}

/// **RF1：语句头部禁复合字面量。**
///
/// ```text
/// if x == T{}
/// { f() }
/// ```
///
/// - `xnest = -1`（官方 `header` `:2364-2365`）：条件在 `T` 处结束 ⇒ 第一个 `{}` 是
///   **then 块**（空），下一行的 `{ f() }` 是**另一条块语句**。
/// - 少了 `xnest = -1`：`x == T{}` 被读成一个整体 ⇒ then 块是 `{ f() }`（只有 1 条语句）。
///
/// ⚠️ **两种读法都不报错** ⇒ 只有树形断言能分辨。这正是"漏了不会报错"的典型，
/// 也是本用例存在的唯一理由（`M2-tests.md` 的 RF1）。
///
/// ⚠️ **两个语句之间的那个换行是必需的**：写在同一行
/// （`if x == T{} { f() }`）时它是一段**非法 Go** —— 官方 `stmtList`（`:2726-2730`）
/// 在两条语句之间要一个 `;`，缺失时先报 `at end of statement`，
/// 再 `advance(_Semi, _Rbrace, _Case, _Default)` **一路冲到后面那个 `}`**，
/// 把 `{ f() }` 整块**吞掉** ⇒ 树里只剩 1 条语句，两种读法又变得不可分辨。
/// （实测：同一行输入下我们的输出与官方逐字吻合 —— 恰好两条诊断
/// `at end of statement` + `non-declaration statement outside function body`。）
#[test]
fn t_a_56_头部禁复合字面量_rf1() {
    let v = stmts("if x == T{}\n{ f() }");
    assert_eq!(v.len(), 2, "If + 后面那条块语句，got {v:?}");

    let i = as_if(v[0].clone());
    let cond = i.cond.as_ref().expect("有条件");
    match cond {
        Expr::Binary(o) => {
            assert_eq!(name_of(&o.x), "x");
            // ⚠️ 右操作数必须是**裸名字** `T`，不能是 `CompositeLit`。
            assert_eq!(name_of(o.y.as_deref().expect("二元的 Y")), "T");
        }
        other => panic!("期望 `x == T` 的二元式，got {other:?}"),
    }
    assert!(i.then.stmts.is_empty(), "then 是 `{{}}`，空的");

    match &v[1] {
        Stmt::Block(b) => assert_eq!(b.stmts.len(), 1, "后面那条块里有 `f()`"),
        other => panic!("期望块语句，got {other:?}"),
    }
}

/// `xnest` 的**恢复**（正常收尾那条出口）。
///
/// 第一个语句是 `if`（`header` 把 `xnest` 置 -1 又恢复），第二条的 `T{}`
/// 必须**仍然**是复合字面量。少了 `self.xnest = outer`，副作用会一直留在
/// **之后的所有语句**上（症状是树形变浅且**不报错**）。
#[test]
fn t_a_56_头部退出后恢复_xnest() {
    let v = stmts("if c { f() }\n_ = T{}");
    assert_eq!(v.len(), 2, "got {v:?}");
    let a = as_assign(v[1].clone());
    assert!(
        matches!(a.rhs[0], Expr::CompositeLit(_)),
        "`xnest` 没恢复的话 `T{{}}` 就不再是复合字面量，got {:?}",
        a.rhs[0]
    );
}

/// `xnest` 的**恢复**（range 子句那条**早退**出口）。
///
/// ⚠️ 这条与上一条**不同**：`for range x {}` 走的是 `header` 里
/// `if matches!(first, SimpleStmt::Range(_)) { self.xnest = outer; return ... }`
/// 那个**提前返回**，漏恢复只在这条路径上暴露。
#[test]
fn t_a_56_range_早退出口也恢复_xnest() {
    let v = stmts("for range ch {}\n_ = T{}");
    assert_eq!(v.len(), 2, "got {v:?}");
    let a = as_assign(v[1].clone());
    assert!(
        matches!(a.rhs[0], Expr::CompositeLit(_)),
        "range 早退那条出口也必须恢复 `xnest`，got {:?}",
        a.rhs[0]
    );
}

/// 条件位上是赋值（`=` 写成 `==` 的经典笔误）⇒ 官方换成
/// `assignment X = Y` 的措辞，且**不设 cond**（官方 `cond` 保持 nil）。
#[test]
fn t_a_56_条件位上是赋值() {
    let r = stmts_raw("if x = y {}");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    assert_eq!(
        first_msg(&r),
        "syntax error: cannot use assignment x = y as value"
    );

    let i = as_if(body_block(&r).stmts[0].clone());
    assert!(
        i.cond.is_none(),
        "官方 `header` 的 `default` 臂不给 `cond` 赋值 ⇒ nil；got {:?}",
        i.cond
    );
    assert!(i.then.stmts.is_empty());
}

/// `emphasize` 的两侧 —— 期望文本**逐字**取自官方 `testdata/issue60599.go`。
#[test]
fn t_a_56_条件位赋值的两侧都套括号() {
    // issue60599.go:9  `if x || y  = z {}`  ⇒ "cannot use assignment (x || y) = z as value"
    let r = stmts_raw("if x || y = z {}");
    assert_eq!(
        first_msg(&r),
        "syntax error: cannot use assignment (x || y) = z as value",
        "左操作数顶层是二元 ⇒ 套括号"
    );

    // issue60599.go:10 `if x = y || z {}`  ⇒ "cannot use assignment x = (y || z) as value"
    let r2 = stmts_raw("if x = y || z {}");
    assert_eq!(
        first_msg(&r2),
        "syntax error: cannot use assignment x = (y || z) as value",
        "右操作数顶层是二元 ⇒ 套括号"
    );
}

/// `if x; {` —— 见过分号但没有条件 ⇒ 另一条消息（`missing condition`）
/// **且** `badExpr` 的位置是**那个分号**（官方 `b.pos = semi.pos`）。
#[test]
fn t_a_56_分号后无条件() {
    let r = stmts_raw("if x; {}");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    assert_eq!(
        first_msg(&r),
        "syntax error: missing condition in if statement"
    );

    let i = as_if(body_block(&r).stmts[0].clone());
    let cond = i.cond.as_ref().expect("官方会造 badExpr");
    assert!(matches!(cond, Expr::Bad(_)));
    assert_eq!(
        (cond.pos().line, cond.pos().col),
        (3, 5),
        "位置是那个 `;`（**不是** `{{`）"
    );
    assert_eq!(
        name_of(match i.init.as_ref().expect("有 init") {
            SimpleStmt::Expr(e) => &e.x,
            other => panic!("期望 ExprStmt，got {other:?}"),
        }),
        "x"
    );
}

/// 语句头里禁 `var` 声明：**报错但照常继续**（官方 `got(_Var)` 已消费掉它）。
#[test]
fn t_a_56_头部不许_var_声明() {
    let r = stmts_raw("if var x = 1; x > 0 {}");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    assert_eq!(
        first_msg(&r),
        "syntax error: var declaration not allowed in if initializer"
    );
    // 「照常继续」：仍然建出了一棵 IfStmt，且条件是对的（`x > 0` 的二元式）。
    let i = as_if(body_block(&r).stmts[0].clone());
    match i.cond.as_ref().expect("有条件") {
        Expr::Binary(o) => {
            assert_eq!(o.op, Op::Gtr);
            assert_eq!(name_of(&o.x), "x");
        }
        other => panic!("期望 `x > 0` 的二元式，got {other:?}"),
    }
}

/// 消息里的 `%s` 是**关键字名** —— `for` 与 `if` 各一条，防止把 `keyword` 写死。
#[test]
fn t_a_56_头部不许_var_的消息带关键字名() {
    let r = stmts_raw("for var x = 1; ; {}");
    assert_eq!(
        first_msg(&r),
        "syntax error: var declaration not allowed in for initializer"
    );
}

// ───────────────────────── T-A-57 ForStmt ─────────────────────────

#[test]
fn t_a_57_无限循环() {
    let f = as_for(one("for {}"));
    assert!(f.init.is_none() && f.cond.is_none() && f.post.is_none());
    assert!(f.body.stmts.is_empty());
    // 官方 `nodes_test.go`：`@for {}` ⇒ pos 在 `for`。
    assert_eq!((f.pos.line, f.pos.col), (3, 1));
}

#[test]
fn t_a_57_条件_only_循环() {
    let f = as_for(one("for x < n {}"));
    // ⚠️ `for x < n {}` 走的是 `header` 的 `condStmt = init` 分支 ⇒
    // 「那段其实是条件，不是 init」。
    assert!(f.init.is_none(), "条件式 for 的 init 必须是 None");
    match f.cond.as_ref().expect("有条件") {
        Expr::Binary(o) => {
            assert_eq!(o.op, Op::Lss);
            assert_eq!(name_of(&o.x), "x");
            assert_eq!(name_of(o.y.as_deref().expect("二元的 Y")), "n");
        }
        other => panic!("期望 `x < n` 的二元式，got {other:?}"),
    }
    assert!(f.post.is_none());
}

#[test]
fn t_a_57_三段式循环() {
    let f = as_for(one("for i := 0; i < n; i++ {}"));
    match f.init.as_ref().expect("有 init") {
        SimpleStmt::Assign(a) => {
            assert_eq!(a.op, AssignOp::Define);
            assert_eq!(name_of(&a.lhs[0]), "i");
        }
        other => panic!("期望 AssignStmt，got {other:?}"),
    }
    assert!(matches!(f.cond.as_ref().expect("有条件"), Expr::Binary(_)));
    match f.post.as_ref().expect("有 post") {
        SimpleStmt::Assign(a) => assert_eq!(a.op, AssignOp::Inc, "post 是 `i++`"),
        other => panic!("期望 AssignStmt，got {other:?}"),
    }
}

#[test]
fn t_a_57_全空的三段式() {
    let f = as_for(one("for ; ; {}"));
    assert!(f.init.is_none() && f.cond.is_none() && f.post.is_none());
}

#[test]
fn t_a_57_post_位不许短变量声明() {
    let r = stmts_raw("for ; ; i := 0 {}");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    assert_eq!(
        first_msg(&r),
        "syntax error: cannot declare in post statement of for loop"
    );
}

#[test]
fn t_a_57_range_无_lhs() {
    let f = as_for(one("for range ch {}"));
    match f.init.as_ref().expect("range 在 init 位") {
        SimpleStmt::Range(rc) => {
            assert!(
                rc.lhs.is_empty(),
                "`for range ch` 没有 lhs（官方 `Lhs == nil`）"
            );
            assert!(!rc.is_define, "`range` 前没有 `:=`");
            assert_eq!(name_of(&rc.x), "ch");
            // 官方 `nodes_test.go`：`@range s` ⇒ pos 在 `range`。
            assert_eq!((rc.pos.line, rc.pos.col), (3, 5));
        }
        other => panic!("期望 RangeClause，got {other:?}"),
    }
    assert!(f.cond.is_none() && f.post.is_none(), "range 子句是终点");
}

#[test]
fn t_a_57_range_带_lhs() {
    let f = as_for(one("for i := range s {}"));
    match f.init.as_ref().expect("range") {
        SimpleStmt::Range(rc) => {
            assert_eq!(rc.lhs.len(), 1);
            assert_eq!(name_of(&rc.lhs[0]), "i");
            assert!(rc.is_define, "`:=` 形式");
            assert_eq!((rc.pos.line, rc.pos.col), (3, 10), "pos 在 `range`");
        }
        other => panic!("期望 RangeClause，got {other:?}"),
    }
}

#[test]
fn t_a_57_range_多值() {
    let f = as_for(one("for k, v = range m {}"));
    match f.init.as_ref().expect("range") {
        SimpleStmt::Range(rc) => {
            assert_eq!(rc.lhs.len(), 2, "`k, v` 两个 lhs");
            assert_eq!(name_of(&rc.lhs[1]), "v");
            assert!(!rc.is_define, "`=` 形式（不是 `:=`）");
        }
        other => panic!("期望 RangeClause，got {other:?}"),
    }
}

/// **range over func**（go1.23+，`T-A-57` 点名）**不需要新的产生式**。
///
/// `for x := range seq {}` 里 `seq` 是不是函数**在语法上无从判断** ——
/// 官方 parser 也一视同仁（`newRangeClause` 末尾就是一个 `p.expr()`）。
/// 所以这条用例的价值是**把"无需改动"这件事钉住**：将来若有人为 range-over-func
/// 加分支（比如"看到调用就特殊处理"），这条会变红。
#[test]
fn t_a_57_range_over_func_与普通_range_同形() {
    let f = as_for(one("for x := range iter.Seq[int](nil) {}"));
    match f.init.as_ref().expect("range") {
        SimpleStmt::Range(rc) => {
            assert_eq!(name_of(&rc.lhs[0]), "x");
            // `X` 就是一个普通表达式：调用 + 泛型实例化 + 实参。
            assert!(matches!(rc.x, Expr::Call(_)), "got {:?}", rc.x);
        }
        other => panic!("期望 RangeClause，got {other:?}"),
    }
}

#[test]
fn t_a_57_条件位上是赋值() {
    let r = stmts_raw("for x = y {}");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    assert_eq!(
        first_msg(&r),
        "syntax error: cannot use assignment x = y as value"
    );
    let f = as_for(body_block(&r).stmts[0].clone());
    assert!(f.cond.is_none(), "官方不给 cond 赋值，got {:?}", f.cond);
}

// ───────────────────────── T-A-58 SwitchStmt / TypeSwitch ────────────────────

#[test]
fn t_a_58_空_switch() {
    let s = as_switch(one("switch {}"));
    assert!(s.init.is_none() && s.tag.is_none());
    assert!(s.cases.is_empty());
    // 官方 `nodes_test.go`：`@switch {}` ⇒ pos 在 `switch`。
    assert_eq!((s.pos.line, s.pos.col), (3, 1));
}

#[test]
fn t_a_58_带_tag_的_switch() {
    let s = as_switch(one("switch x {}"));
    assert!(s.init.is_none());
    assert_eq!(name_of(s.tag.as_ref().expect("有 tag")), "x");
}

#[test]
fn t_a_58_带_init_的_switch() {
    let s = as_switch(one("switch x := f(); x {}"));
    match s.init.as_ref().expect("有 init") {
        SimpleStmt::Assign(a) => assert_eq!(a.op, AssignOp::Define),
        other => panic!("期望 AssignStmt，got {other:?}"),
    }
    assert_eq!(name_of(s.tag.as_ref().expect("有 tag")), "x");
}

#[test]
fn t_a_58_case_与_default() {
    let s = as_switch(one("switch x { case 1: case 2, 3: default: }"));
    assert_eq!(s.cases.len(), 3, "got {:?}", s.cases);

    assert!(matches!(s.cases[0].cases, Some(Expr::BasicLit(_))));
    // 多值 case ⇒ 一个 `ListExpr`（**不**拆成 Vec）。
    match s.cases[1].cases.as_ref().expect("多值 case") {
        Expr::List(l) => assert_eq!(l.elems.len(), 2, "`2, 3`"),
        other => panic!("多值 case 应是 ListExpr，got {other:?}"),
    }
    // 官方 `CaseClause.Cases == nil` ⟺ `default`。
    assert!(s.cases[2].cases.is_none(), "`default` 的 cases 是 None");
}

#[test]
fn t_a_58_case_clause_的位置() {
    let s = as_switch(one("switch x {\ncase 1:\ndefault:\n}"));
    // 官方 `nodes_test.go`：`@case x:` ⇒ CaseClause.pos 在 `case` / `default`。
    assert_eq!((s.cases[0].pos.line, s.cases[0].pos.col), (4, 1));
    assert_eq!(
        (s.cases[1].pos.line, s.cases[1].pos.col),
        (5, 1),
        "`default` 同样"
    );
    // `Colon` 在 `want(_Colon)` **之前**取 ⇒ 即使缺冒号也记在"本该是冒号"的词元上。
    assert_eq!((s.cases[0].colon.line, s.cases[0].colon.col), (4, 7));
}

#[test]
fn t_a_58_case_体是语句列表() {
    let s = as_switch(one("switch x { case 1: f(); g() }"));
    assert_eq!(s.cases[0].body.len(), 2, "got {:?}", s.cases[0].body);
}

#[test]
fn t_a_58_case_可以只有_cases_没有体() {
    let s = as_switch(one("switch x { case 1: }"));
    assert!(s.cases[0].body.is_empty(), "got {:?}", s.cases[0].body);
}

/// **类型 switch**：`switch v := x.(type) {}`。
///
/// `tag` 是一个 [`Expr::TypeSwitchGuard`]，且 `lhs` 由 `simpleStmt` 回填 ——
/// 这就是 `simpleStmt` 的 `keyword` 形参存在的第二个理由。
#[test]
fn t_a_58_类型_switch() {
    let s = as_switch(one("switch v := x.(type) {}"));
    assert!(s.init.is_none(), "守卫整体在 tag 位（不是 init）");
    match s.tag.as_ref().expect("有 tag") {
        Expr::TypeSwitchGuard(g) => {
            assert_eq!(
                g.lhs.as_ref().map(|n| n.value.as_str()),
                Some("v"),
                "lhs 被回填"
            );
            assert_eq!(name_of(&g.x), "x");
        }
        other => panic!("期望 TypeSwitchGuard，got {other:?}"),
    }
}

#[test]
fn t_a_58_类型_switch_带_case() {
    let s = as_switch(one("switch x := y.(type) { case int: f() }"));
    assert_eq!(s.cases.len(), 1);
    assert!(
        matches!(s.cases[0].cases, Some(Expr::Name(_))),
        "`case int:`"
    );
}

/// `x := y.(type)` 只在 **`switch` 的 init 位**才被改写成 `ExprStmt`（官方 `:2220`）。
///
/// 在普通语句位它就是一条**赋值**，`rhs` 是那个守卫本身 —— 官方亦然
/// （语法上合法，形状是否合理交给类型检查）。这条钉的是"**不是**一见到 `.(type)`
/// 就当类型 switch"。
#[test]
fn t_a_58_非_switch_上下文里_守卫是普通赋值() {
    let a = as_assign(one("x := y.(type)"));
    assert_eq!(a.op, AssignOp::Define);
    assert_eq!(name_of(&a.lhs[0]), "x");
    assert!(
        matches!(a.rhs[0], Expr::TypeSwitchGuard(_)),
        "rhs 是守卫本身（没有被回填 lhs），got {:?}",
        a.rhs[0]
    );
}

#[test]
fn t_a_58_switch_缺左花括号() {
    let r = stmts_raw("switch x; 1");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    assert_eq!(first_msg(&r), "syntax error: missing { after switch clause");
    let s = as_switch(body_block(&r).stmts[0].clone());
    assert!(s.cases.is_empty(), "没有 `{{` ⇒ 一个 case 也没有");
    // `tag` 是字面量 `1`（`name_of` 会在这里炸 —— 它不是个名字）。
    match s.tag.as_ref().expect("tag 已解析") {
        Expr::BasicLit(l) => assert_eq!(l.value, "1"),
        other => panic!("期望 BasicLit，got {other:?}"),
    }
}

// ───────────────────────── T-A-59 SelectStmt / CommClause ────────────────────

#[test]
fn t_a_59_空_select() {
    let s = as_select(one("select {}"));
    assert!(s.clauses.is_empty());
    // 官方 `nodes_test.go`：`@select {}` ⇒ pos 在 `select`。
    assert_eq!((s.pos.line, s.pos.col), (3, 1));
}

#[test]
fn t_a_59_select_default() {
    let s = as_select(one("select { default: }"));
    assert_eq!(s.clauses.len(), 1);
    // 官方 `CommClause.Comm == nil` ⟺ `default`。
    assert!(s.clauses[0].comm.is_none(), "`default` 的 comm 是 None");
    assert_eq!((s.clauses[0].pos.line, s.clauses[0].pos.col), (3, 10));
}

#[test]
fn t_a_59_select_三个子句() {
    let s = as_select(one("select { case <-ch: case x <- ch: default: }"));
    assert_eq!(s.clauses.len(), 3, "got {:?}", s.clauses);

    // `case <-ch:` ⇒ comm 是一元 `<-` 的表达式语句。
    match s.clauses[0].comm.as_ref().expect("有 comm") {
        SimpleStmt::Expr(e) => assert!(matches!(e.x, Expr::Unary(_)), "got {:?}", e.x),
        other => panic!("期望 ExprStmt，got {other:?}"),
    }
    // `case x <- ch:` ⇒ 发送语句。
    match s.clauses[1].comm.as_ref().expect("有 comm") {
        SimpleStmt::Send(s) => {
            assert_eq!(name_of(&s.chan), "x");
            assert_eq!(name_of(&s.value), "ch");
        }
        other => panic!("期望 SendStmt，got {other:?}"),
    }
    assert!(s.clauses[2].comm.is_none(), "第三个是 default");
}

#[test]
fn t_a_59_comm_的短变量声明() {
    let s = as_select(one("select { case x := <-ch: x++ }"));
    match s.clauses[0].comm.as_ref().expect("有 comm") {
        SimpleStmt::Assign(a) => {
            assert_eq!(a.op, AssignOp::Define);
            assert_eq!(name_of(&a.lhs[0]), "x");
            assert!(matches!(a.rhs[0], Expr::Unary(_)), "rhs 是 `<-ch`");
        }
        other => panic!("期望 AssignStmt，got {other:?}"),
    }
    assert_eq!(s.clauses[0].body.len(), 1, "体里有一条 `x++`");
    assert_eq!((s.clauses[0].colon.line, s.clauses[0].colon.col), (3, 24));
}

#[test]
fn t_a_59_select_缺左花括号() {
    let r = stmts_raw("select x {}");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    assert_eq!(first_msg(&r), "syntax error: missing { after select clause");
    let s = as_select(body_block(&r).stmts[0].clone());
    assert!(s.clauses.is_empty());
}

// ───────────────────────── 标签语句（`_Name` 臂的另一半）─────────────────────

#[test]
fn 标签语句_pos_在冒号上() {
    let v = stmts_branchy("L: x = 1");
    assert_eq!(v.len(), 1, "got {v:?}");
    match &v[0] {
        Stmt::Labeled(l) => {
            assert_eq!(l.label.value, "L");
            // 官方 `s.pos = p.pos()` 在 `want(_Colon)` **之前** ⇒ pos 是那个 `:`。
            assert_eq!((l.pos.line, l.pos.col), (3, 2));
            assert!(matches!(&*l.stmt, Stmt::Assign(_)), "got {:?}", l.stmt);
        }
        other => panic!("期望 LabeledStmt，got {other:?}"),
    }
}

/// `L: }` —— `}` 前的分号可省 ⇒ 官方造一个**空语句**（`:2277-2285`）。
#[test]
fn 标签后紧跟右花括号是空语句() {
    let v = stmts_branchy("{ L: }");
    assert_eq!(v.len(), 1, "got {v:?}");
    match &v[0] {
        Stmt::Block(b) => match &b.stmts[0] {
            Stmt::Labeled(l) => assert!(
                matches!(&*l.stmt, Stmt::Empty(_)),
                "官方在 `_Rbrace` 处造 EmptyStmt，got {:?}",
                l.stmt
            ),
            other => panic!("期望 LabeledStmt，got {other:?}"),
        },
        other => panic!("期望外层块，got {other:?}"),
    }
}

/// 标签后面没有语句 ⇒ 报 `missing statement after label` 并返回 `None`
/// （官方 `return nil`，为的是**避免连锁错误**）。
#[test]
fn 标签后没有语句报错() {
    let r = stmts_raw("L: %");
    assert!(r.errors >= 1, "应报错：{:?}", r.diags);
    assert_eq!(first_msg(&r), "syntax error: missing statement after label");
}

// ───────────────────────── 收敛：dump 整段 ─────────────────────────

/// 整段控制流的**中立 dump** —— 钉住"每个字段都出现了"这一层
/// （逐字段断言看不见"漏 dump 了一个字段"）。
///
/// ⚠️ 期望文本是**逐字段对照官方 `nodes.go` 核对过**才固化的，不是"跑一遍抄下来"：
/// `IfStmt{Init,Cond,Then,Else}`（`nodes.go:322`）/ `ForStmt{Init,Cond,Post,Body}` /
/// `RangeClause{Lhs,Def,X}` / `BlockStmt{List}` / `ExprStmt{X}` /
/// `CallExpr{Fun,ArgList,HasDots}` / `FuncDecl{Recv,Name,TParams,Type,Body}` /
/// `FuncType{ParamList,ResultList}` / `File{PkgName,GoVersion,DeclList}`。
/// 漏一个就会在这里少一行 —— 这正是逐字段断言照不到的那一类。
#[test]
fn dump_控制流整段形状() {
    let r = parse(&wrap("if a { f() } else { g() }\nfor i := range s {}"));
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    let d = dump(file_of(&r));
    let expected = r#"File
  PkgName: Name "p"
  GoVersion: -
  Decls: [1 entries]
    FuncDecl
      Recv: -
      Name: Name "f"
      TParams: [0 entries]
      Type:
        FuncType
          Params: [0 entries]
          Results: [0 entries]
      Body:
        BlockStmt
          Stmts: [2 entries]
            IfStmt
              Init: -
              Cond: Name "a"
              Then:
                BlockStmt
                  Stmts: [1 entries]
                    ExprStmt
                      X:
                        CallExpr
                          Fun: Name "f"
                          Args: [0 entries]
                          HasDots: false
              Else:
                BlockStmt
                  Stmts: [1 entries]
                    ExprStmt
                      X:
                        CallExpr
                          Fun: Name "g"
                          Args: [0 entries]
                          HasDots: false
            ForStmt
              Init:
                RangeClause
                  Define: true
                  Lhs: [1 entries]
                    Name "i"
                  X: Name "s"
              Cond: -
              Post: -
              Body:
                BlockStmt
                  Stmts: [0 entries]
"#;
    assert_eq!(d, expected, "整段控制流的 dump 形状变了");
}
