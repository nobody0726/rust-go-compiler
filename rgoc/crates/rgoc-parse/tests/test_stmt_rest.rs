//! `T-A-50`（Empty / Labeled / Block）、`T-A-52`（DeclStmt）、`T-A-53`（BranchStmt）、
//! `T-A-54`（CallStmt）、`T-A-55`（ReturnStmt）—— 即 `stmtOrNil` 剩下的六条臂（`T-AP14`）。
//!
//! # 与 `T-AP13` 的分界
//!
//! `T-AP13` 交付的是「以 **`_Name` 开头** 的语句 + 四条控制流」；本任务交付的是
//! 「**其它起始词元**的语句」。所以两个文件**不重叠**：
//! `test_stmt_ctrl.rs` 测 `x = 1` / `if` / `for` / `switch` / `select`，
//! 这里测 `;` / `L:` / `{...}` / `break` / `go` / `return` / `var`。
//!
//! ⚠️ 唯一的例外是 `_Name` 臂**后面**接 `:` 的那一支（labeled）—— 它按 `progress.md`
//! 的 Ruling 整体归 `T-AP13`，本文件只补它**还没覆盖**的两个出口
//! （`L:` 后紧跟 `}`、`L:` 后嵌块语句）。
//!
//! # 位置判据全部来自官方 `nodes_test.go`（权威，不是推测）
//!
//! | 节点 | 官方标记 | 结论 |
//! |---|---|---|
//! | `EmptyStmt` | ——（官方 `nodes_test.go` 无标记） | pos 在**那个分号**上（`parser.go:2705`） |
//! | `LabeledStmt` | ——（同上） | pos 在 **`:`** 上（`s.pos = p.pos()` 在 `want(_Colon)` **之前**） |
//! | `BlockStmt` | —— | pos 在 **`{`** 上（`parser.go:2305`） |
//! | `DeclStmt` | `@const x = 0` / `@var (a, b = 1, 2)` / `@type T int` | pos 在**声明关键字**上 |
//! | `BranchStmt` | `@break` / `@break L` / `@continue` / `@fallthrough` / `@goto L` | pos 在**关键字**上 |
//! | `CallStmt` | `@defer f()` / `@go f()` | pos 在 **`go`/`defer`** 上 |
//! | `ReturnStmt` | `@return` / `@return x` / `@return a, b, a + b*f(1, 2, 3)` | pos 在 **`return`** 上 |
//!
//! # 关于断言里的消息串（**重要，先读**）
//!
//! 官方 `syntaxErrorAt`（`parser.go:262-297`）按**消息前缀**分三支，追加当前词元：
//!
//! | 消息前缀 | 官方最终消息 |
//! |---|---|
//! | `expected ` | `syntax error: unexpected <tok>, <msg>` |
//! | `in ` / `at ` / `after ` | `syntax error: unexpected <tok> <msg>` |
//! | 其余（含 `msg == ""`） | **`syntax error: <msg>`** ← 唯一能逐字断言的一支 |
//!
//! 我们目前**只实现了 `default` 那一支**（`recovery.rs` 的 `syntax_error_at`，见其文档），
//! 另两支的 `unexpected <tok>` 中缀归 **`T-AP16`**（`tokstring` 也在那里）。
//!
//! 所以本文件里：
//! - `missing statement after label` / `expression in …` ⇒ 走 `default` 支，**逐字断言**；
//! - `expected name` / `at end of statement` ⇒ 官方另有中缀，这里断言的是**我们已交付的那一段**，
//!   T-AP16 补中缀时这些断言需要同步放宽（**不是**现在放宽 —— 现在放宽就成了恒真）。
//!
//! ⚠️ `expression in go must not be parenthesized`（`T-A-54`）走的是官方
//! `p.errorAt`（`:997`）**不是** `syntaxErrorAt` ⇒ 它**没有** `syntax error: ` 前缀，
//! 且**不受** EOF 抑制（`F5`）。断言里必须体现这个差别 —— 用一个走 `errorAt` 的
//! 消息去验证「前缀规则」，等于没验证。

use rgoc_ast::{
    AstFile, BlockStmt, BranchStmt, BranchTok, CallStmt, CallTok, Decl, DeclStmt, Expr, FuncDecl,
    LabeledStmt, ReturnStmt, Stmt, count_nodes, dump,
};
use rgoc_parse::{ParseResult, parse_file};

fn parse(src: &str) -> ParseResult {
    parse_file(src.as_bytes(), "t.go")
}

/// 把语句体包进 `func f() { ... }` 里 —— 语句只能从函数体里观察到
/// （`stmtOrNil` 的生产入口是 `funcBody`）。
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

/// 解析一段语句体，**要求零诊断**，返回语句表（拥有所有权的克隆）。
fn stmts(body: &str) -> Vec<Stmt> {
    let src = wrap(body);
    let r = parse(&src);
    assert_eq!(r.errors, 0, "干净输入不该报错：{body:?} ⇒ {:?}", r.diags);
    body_block(&r).stmts.clone()
}

/// 解析一段语句体，**只允许 `checkBranches` 的诊断**（`T-AP16` 起）。
///
/// # 为什么需要这个变体
///
/// `T-AP16` 落地 `checkBranches` 之后，`break` / `continue` / `fallthrough` /
/// `goto` / 标签**在语法上合法但在语义上非法**的输入会开始报错 ——
/// 而本文件（`T-A-50`/`T-A-53`）验的是**树形**，那些诊断与树形无关。
///
/// ⚠️ 这里**不放宽**「零诊断」的语义，而是**精确分类**：只放过
/// `SYN-BRANCH-*` / `SYN-LABEL-*` / `SYN-GOTO-*` 三族，其余照旧红。
/// 一个「把诊断全部吞掉」的 helper 会让「`checkBranches` 悄悄不工作了」
/// 这件事**看不出来** —— 那正是 `AGENTS.md` 记的静默失败形状。
///
/// `tests/test_branches.rs` 才是验这些诊断本身的地方。
fn stmts_branchy(body: &str) -> Vec<Stmt> {
    let src = wrap(body);
    let mut r = parse(&src);
    let diags: Vec<rgoc_lex::diag::Diag> = std::mem::take(&mut r.diags).into_sorted();
    let unexpected: Vec<&rgoc_lex::diag::Diag> = diags
        .iter()
        .filter(|d| !is_branch_diag(d.code.as_str()))
        .collect();
    assert!(
        unexpected.is_empty(),
        "只允许分支检查诊断，其余都算意外：{body:?} ⇒ {unexpected:?}"
    );
    body_block(&r).stmts.clone()
}

/// 诊断代号是否属于 `checkBranches`（`T-A-80`/`81`/`82`）那一族。
fn is_branch_diag(code: &str) -> bool {
    code.starts_with("SYN-BRANCH-")
        || code.starts_with("SYN-LABEL-")
        || code.starts_with("SYN-GOTO-")
}

/// 解析一段语句体，返回 `ParseResult`（允许有错 —— 用于恢复路径）。
fn stmts_raw(body: &str) -> ParseResult {
    parse(&wrap(body))
}

/// 首条诊断的消息。
fn first_msg(r: &ParseResult) -> String {
    r.first
        .as_ref()
        .map(|d| d.message.clone())
        .unwrap_or_default()
}

/// 首条诊断的位置 `(line, col)`。
fn first_pos(r: &ParseResult) -> (u32, u32) {
    r.first
        .as_ref()
        .map(|d| (d.pos.line, d.pos.col))
        .unwrap_or_else(|| panic!("应有首条诊断，诊断：{:?}", r.diags))
}

/// 取恰好一条语句（多于/少于一条都炸 —— 免得"少解析了"被当成"解析对了"）。
fn one(body: &str) -> Stmt {
    let v = stmts(body);
    assert_eq!(v.len(), 1, "应恰好一条语句，got {v:?}");
    v.into_iter().next().expect("上面判过 len == 1")
}

/// [`one`] 的宽松版：只放过 `checkBranches` 的诊断（`T-AP16` 起）。
fn one_branchy(body: &str) -> Stmt {
    let v = stmts_branchy(body);
    assert_eq!(v.len(), 1, "应恰好一条语句，got {v:?}");
    v.into_iter().next().expect("上面判过 len == 1")
}

fn pos_of(s: &Stmt) -> (u32, u32) {
    match s {
        Stmt::Empty(x) => (x.pos.line, x.pos.col),
        Stmt::Labeled(x) => (x.pos.line, x.pos.col),
        Stmt::Block(x) => (x.pos.line, x.pos.col),
        Stmt::Expr(x) => (x.pos.line, x.pos.col),
        Stmt::Send(x) => (x.pos.line, x.pos.col),
        Stmt::Decl(x) => (x.pos.line, x.pos.col),
        Stmt::Assign(x) => (x.pos.line, x.pos.col),
        Stmt::Branch(x) => (x.pos.line, x.pos.col),
        Stmt::Call(x) => (x.pos.line, x.pos.col),
        Stmt::Return(x) => (x.pos.line, x.pos.col),
        Stmt::If(x) => (x.pos.line, x.pos.col),
        Stmt::For(x) => (x.pos.line, x.pos.col),
        Stmt::Switch(x) => (x.pos.line, x.pos.col),
        Stmt::Select(x) => (x.pos.line, x.pos.col),
    }
}

fn as_labeled(s: Stmt) -> LabeledStmt {
    match s {
        Stmt::Labeled(x) => x,
        other => panic!("期望 LabeledStmt，got {other:?}"),
    }
}

fn as_block(s: Stmt) -> BlockStmt {
    match s {
        Stmt::Block(x) => x,
        other => panic!("期望 BlockStmt，got {other:?}"),
    }
}

fn as_decl(s: Stmt) -> DeclStmt {
    match s {
        Stmt::Decl(x) => x,
        other => panic!("期望 DeclStmt，got {other:?}"),
    }
}

fn as_branch(s: Stmt) -> BranchStmt {
    match s {
        Stmt::Branch(x) => x,
        other => panic!("期望 BranchStmt，got {other:?}"),
    }
}

fn as_call(s: Stmt) -> CallStmt {
    match s {
        Stmt::Call(x) => x,
        other => panic!("期望 CallStmt，got {other:?}"),
    }
}

fn as_return(s: Stmt) -> ReturnStmt {
    match s {
        Stmt::Return(x) => x,
        other => panic!("期望 ReturnStmt，got {other:?}"),
    }
}

fn name_of(e: &Expr) -> &str {
    match e {
        Expr::Name(n) => n.value.as_str(),
        other => panic!("不是 Name：{other:?}"),
    }
}

// ── T-A-50：EmptyStmt ────────────────────────────────────────────────────

#[test]
fn t_a_50_空语句() {
    let v = stmts(";");
    assert_eq!(v.len(), 1, "got {v:?}");
    assert!(matches!(v[0], Stmt::Empty(_)), "got {:?}", v[0]);
    // 官方 `:2705`：`s.pos = p.pos()` —— 落在**那个分号**上。
    assert_eq!(pos_of(&v[0]), (3, 1), "pos 落在 `;`");
}

/// 连续的 `;;` 是**两条**空语句 —— 官方 `_Semi` 臂**不消费**分号
/// （消费在 `stmtList` 的 `p.got(_Semi)` 里），所以下一个循环又见到一个 `;`。
///
/// ⚠️ 这条用例的鉴别力在于：如果实现改成「在 `stmtOrNil` 里就消费分号」，
/// 第一条 `;` 会被吃掉、第二条变成语句起始，**结果仍是 2 条**，
/// 但**第三条**（下面那条用例）就会现形 —— 两条一起才钉得住「谁消费」。
#[test]
fn t_a_50_连续分号是两条空语句() {
    let v = stmts(";;");
    assert_eq!(v.len(), 2, "got {v:?}");
    assert!(matches!(v[0], Stmt::Empty(_)) && matches!(v[1], Stmt::Empty(_)));
    assert_eq!(pos_of(&v[0]), (3, 1));
    assert_eq!(pos_of(&v[1]), (3, 2), "第二个 `;` 在第 2 列");
}

/// **谁消费分号**的判据：`f();;` 里第二个 `;` **不**是「上一条语句的收尾」
/// （那条已被 `stmtList` 收走），而是**一条新的空语句**的起始。
#[test]
fn t_a_50_分号只被_stmt_list_收走一次() {
    let v = stmts("f();;");
    assert_eq!(v.len(), 2, "ExprStmt + EmptyStmt，got {v:?}");
    assert!(matches!(v[0], Stmt::Expr(_)), "got {:?}", v[0]);
    assert!(matches!(v[1], Stmt::Empty(_)), "got {:?}", v[1]);
    assert_eq!(pos_of(&v[1]), (3, 5), "第二个 `;` 在第 5 列");
}

// ── T-A-50：LabeledStmt ──────────────────────────────────────────────────

/// 官方 `labeledStmtOrNil`（`:2272`）：`s.pos = p.pos()` 在 `p.want(_Colon)` **之前**
/// ⇒ pos 落在 **`:`** 上（`L: x++` 里是第 2 列，不是第 1 列）。
#[test]
fn t_a_50_标签语句的_pos_在冒号上() {
    // ⚠️ `one_branchy`：`L:` 没被 `goto`/`break` 用到 ⇒ `checkBranches`
    // 会报 `label L defined and not used`（官方 `branches.go:42`）。
    // 那条诊断与本用例要验的 `pos` 无关，用宽松版避开。
    let s = as_labeled(one_branchy("L: x++"));
    assert_eq!(s.label.value, "L");
    assert_eq!((s.pos.line, s.pos.col), (3, 2), "pos 落在 `:`");
    assert!(matches!(*s.stmt, Stmt::Assign(_)), "got {:?}", s.stmt);
}

/// 官方 `:2277-2285`：`L:` 后面紧跟 `}` ⇒ 因为 `}` 前的分号可省，
/// 这被读成**空语句**（**不报错**）。
#[test]
fn t_a_50_标签后紧跟右花括号是空语句() {
    let v = stmts_branchy("L:");
    assert_eq!(v.len(), 1, "got {v:?}");
    let s = as_labeled(v.into_iter().next().expect("len == 1"));
    match *s.stmt {
        Stmt::Empty(e) => {
            // 官方 `:2282`：`e.pos = p.pos()` —— 落在那个 `}` 上。
            assert_eq!((e.pos.line, e.pos.col), (4, 1), "空语句的 pos 落在 `}}`");
        }
        other => panic!("期望 EmptyStmt，got {other:?}"),
    }
}

/// 标签后面可以跟**任意**语句，不只是简单语句 —— 这里跟一个块语句。
#[test]
fn t_a_50_标签后面可以跟块语句() {
    let v = stmts_branchy("L: {\n f()\n}");
    assert_eq!(v.len(), 1, "got {v:?}");
    let s = as_labeled(v.into_iter().next().expect("len == 1"));
    let b = as_block(*s.stmt);
    assert_eq!(b.stmts.len(), 1, "块里有一条 `f()`");
}

/// 官方 `:2293` 的 `missing statement after label` 走 `syntaxErrorAt` 的
/// **`default` 支** ⇒ 可以逐字断言（见本文件头部那张表）。
///
/// ⚠️ 诊断**条数**只断言 `>= 1`：官方在这之后还会报一条 `expected }`
/// （`blockStmt` 的 `want(_Rbrace)` 收尾），那是官方同样有的后续诊断，
/// 钉死总数反而会在 T-AP16 改消息时变成无谓的绊脚石。真正的判据是
/// 「**整条 labeled 语句不进列表**」（官方 `:2295` 的 `return nil`）。
#[test]
fn t_a_50_标签后无语句时报错且整条语句被丢弃() {
    let r = stmts_raw("L: %");
    assert!(r.errors >= 1, "应报错：{:?}", r.diags);
    assert_eq!(first_msg(&r), "syntax error: missing statement after label");
    // 官方 `:2295` 的 `return nil` —— 整条 labeled 语句**不进**列表。
    assert!(
        body_block(&r).stmts.is_empty(),
        "返回 nil ⇒ 列表为空，got {:?}",
        body_block(&r).stmts
    );
}

// ── T-A-50：BlockStmt ───────────────────────────────────────────────────

#[test]
fn t_a_50_裸块语句() {
    let v = stmts("{ f() }");
    assert_eq!(v.len(), 1, "got {v:?}");
    let b = as_block(v.into_iter().next().expect("len == 1"));
    assert_eq!(b.stmts.len(), 1, "块里有一条 `f()`");
    // 官方 `:2305`：`s.pos = p.pos()` 在 `got(_Lbrace)` **之前** ⇒ 落在 `{` 上。
    assert_eq!((b.pos.line, b.pos.col), (3, 1), "pos 落在 `{{`");
}

#[test]
fn t_a_50_块里嵌块() {
    let v = stmts("{\n{\n}\n}");
    assert_eq!(v.len(), 1, "got {v:?}");
    let outer = as_block(v.into_iter().next().expect("len == 1"));
    assert_eq!(outer.stmts.len(), 1, "外层块里只有一个内层块");
    let inner = as_block(outer.stmts.into_iter().next().expect("len == 1"));
    assert!(inner.stmts.is_empty(), "内层块是空的");
}

/// `blockStmt` 的 `rbrace` 在 `want(_Rbrace)` **之前**记录 —— 这样即使 `}`
/// 缺失，位置也在树里。这里用**正常**输入钉住它落在正确的 `}` 上。
#[test]
fn t_a_50_块语句的右花括号位置() {
    let v = stmts("{\n f()\n}");
    let b = as_block(v.into_iter().next().expect("len == 1"));
    assert_eq!((b.rbrace.line, b.rbrace.col), (5, 1), "rbrace 在 `}}` 上");
}

// ── T-A-52：DeclStmt ────────────────────────────────────────────────────

#[test]
fn t_a_52_var_声明语句_pos_在关键字上() {
    let d = as_decl(one("var x = 0"));
    assert_eq!(d.decls.len(), 1, "单条声明，got {:?}", d.decls);
    assert!(matches!(d.decls[0], Decl::Var(_)), "got {:?}", d.decls[0]);
    assert_eq!((d.pos.line, d.pos.col), (3, 1), "pos 落在 `var`");
}

#[test]
fn t_a_52_const_声明语句() {
    let d = as_decl(one("const x = 0"));
    assert_eq!(d.decls.len(), 1);
    assert!(matches!(d.decls[0], Decl::Const(_)), "got {:?}", d.decls[0]);
    assert_eq!((d.pos.line, d.pos.col), (3, 1));
}

#[test]
fn t_a_52_type_声明语句() {
    let d = as_decl(one("type T int"));
    assert_eq!(d.decls.len(), 1);
    assert!(matches!(d.decls[0], Decl::Type(_)), "got {:?}", d.decls[0]);
    assert_eq!((d.pos.line, d.pos.col), (3, 1));
}

/// 官方的 `DeclList` 是**列表**（`nodes.go:389`）：分组声明每行 `append` 一次
/// ⇒ `var (a int; b int)` 产出 **2** 条 `VarDecl`。
///
/// ⚠️ 这条用例专门钉「**列表**」这件事：如果实现把 `DeclStmt` 建模成单条，
/// `var (a int; b int)` 会只剩 1 条，而「能编译 + 其余断言全绿」看不出差别。
#[test]
fn t_a_52_分组声明产生多条() {
    let d = as_decl(one("var (\n\ta int\n\tb int\n)"));
    assert_eq!(d.decls.len(), 2, "分组两行 ⇒ 两条，got {:?}", d.decls);
    assert!(
        d.decls.iter().all(|x| matches!(x, Decl::Var(_))),
        "got {:?}",
        d.decls
    );
}

/// 单条声明里的多名字（`var x, y, z int`）**仍然是一条**声明 ——
/// 与上一条正好是一对：名字多 ≠ 声明多。
#[test]
fn t_a_52_多名字仍是单条声明() {
    let d = as_decl(one("var x, y, z int"));
    assert_eq!(d.decls.len(), 1, "got {:?}", d.decls);
    let Decl::Var(v) = &d.decls[0] else {
        panic!("期望 VarDecl，got {:?}", d.decls[0]);
    };
    assert_eq!(v.names.len(), 3, "三个名字，got {:?}", v.names);
}

/// `declStmt` 也是**语句**，所以它能和别的语句混在一个列表里
/// （`stmtOrNil` 的 `_Var`/`_Const`/`_Type` 三条臂就在 `clearPragma` **之前**，
/// 见官方 `:2626-2635` —— 顺序错了 pragma 归属会错位）。
#[test]
fn t_a_52_声明语句与其它语句混排() {
    let v = stmts("var x = 0\nx++\n_ = x");
    assert_eq!(v.len(), 3, "got {v:?}");
    assert!(matches!(v[0], Stmt::Decl(_)), "got {:?}", v[0]);
    assert!(matches!(v[1], Stmt::Assign(_)), "got {:?}", v[1]);
    assert!(matches!(v[2], Stmt::Assign(_)), "got {:?}", v[2]);
}

// ── T-A-53：BranchStmt ──────────────────────────────────────────────────

#[test]
fn t_a_53_break_无标签() {
    let b = as_branch(one_branchy("break"));
    assert_eq!(b.tok, BranchTok::Break);
    assert!(b.label.is_none(), "无标签，got {:?}", b.label);
    assert_eq!((b.pos.line, b.pos.col), (3, 1), "pos 落在 `break`");
}

#[test]
fn t_a_53_break_带标签() {
    let b = as_branch(one_branchy("break L"));
    assert_eq!(b.tok, BranchTok::Break);
    assert_eq!(b.label.as_ref().expect("有标签").value, "L");
    assert_eq!((b.pos.line, b.pos.col), (3, 1));
}

#[test]
fn t_a_53_continue_无标签与带标签() {
    let b = as_branch(one_branchy("continue"));
    assert_eq!(b.tok, BranchTok::Continue);
    assert!(b.label.is_none(), "got {:?}", b.label);

    let b = as_branch(one_branchy("continue L"));
    assert_eq!(b.tok, BranchTok::Continue);
    assert_eq!(b.label.as_ref().expect("有标签").value, "L");
}

#[test]
fn t_a_53_goto_带标签() {
    let b = as_branch(one_branchy("goto L"));
    assert_eq!(b.tok, BranchTok::Goto);
    assert_eq!(b.label.as_ref().expect("有标签").value, "L");
    assert_eq!((b.pos.line, b.pos.col), (3, 1), "pos 落在 `goto`");
}

/// 官方 `:2691`：`s.Label = p.name()` —— **无标签也照样调** `name()`。
/// 而 `name()`（`:2759`）在未命中时**报错 + 造一个 `"_"` 假名**（不是返回 `nil`）
/// ⇒ 树里出现 `Label: _`，同时报 `expected name`。
///
/// ⚠️ 输入写 `goto;` 而不是裸 `goto`：词法只在 `break`/`continue`/`fallthrough`/
/// `return`/`ident`/`literal`/`)`/`]`/`}`/`++`/`--` 之后补分号，**`goto` 后面不补** ⇒
/// 裸 `goto` 的下一个词元是 `}`，于是 `advance()`（空 followset ⇒ 消费恰好一个）
/// 会把函数体的收尾 `}` 吃掉，后面连着报 `expected }` + EOF。`goto;` 让
/// `advance` 吃的是那个**显式**分号，诊断数才是干净的 1 条。
///
/// ⚠️ 这条用例钉的是「**造假名**」这个恢复动作：如果实现改成"标签缺失就 `None`"，
/// 树里会静默少一个名字，而"能编译 + 报错了"的断言看不出差别。
#[test]
fn t_a_53_goto_缺标签时报错并造假名() {
    let r = stmts_raw("goto;");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    assert_eq!(
        first_msg(&r),
        "syntax error: unexpected semicolon, expected name",
        "官方 `parser.go:267`：`expected ` 前缀 ⇒ `unexpected <tok>, expected name`；\
         `_Semi` 的 `tok` 用 `p.lit`（显式 `;` ⇒ `semicolon`）"
    );
    let v = body_block(&r).stmts.clone();
    assert_eq!(v.len(), 1, "goto 照样建出 BranchStmt，got {v:?}");
    let b = as_branch(v.into_iter().next().expect("len == 1"));
    assert_eq!(b.tok, BranchTok::Goto);
    assert_eq!(
        b.label.as_ref().expect("造出了假名").value,
        "_",
        "官方 `NewName(p.pos(), \"_\")`"
    );
}

#[test]
fn t_a_53_fallthrough() {
    let b = as_branch(one_branchy("fallthrough"));
    assert_eq!(b.tok, BranchTok::Fallthrough);
    assert!(b.label.is_none(), "fallthrough 不读标签，got {:?}", b.label);
    assert_eq!((b.pos.line, b.pos.col), (3, 1));
}

/// 官方 `:2666-2671` 的 `_Fallthrough` 臂**只**做 `p.next()`，**不**读标签
/// （只有 `_Break`/`_Continue`/`_Goto` 读）。所以后面跟一个名字时，
/// 那个名字**不进** `Label`，而是留给 `stmtList` 去报 `at end of statement`。
///
/// ⚠️ 这是「多读了一个词元」的**唯一**观测点：`fallthrough` 单独出现时，
/// 多读的那个词元是 `;`，看不出差别；跟一个名字才现形。
#[test]
fn t_a_53_fallthrough_不读后面的名字() {
    let r = stmts_raw("fallthrough L");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    // 官方 `stmtList`（`:2727`）的 `at end of statement`，走 `default` 支。
    assert_eq!(
        first_msg(&r),
        "syntax error: unexpected name L at end of statement",
        "官方 `:2727` 的 `at end of statement` 走 `at ` 那一支 ⇒ 前置一个空格；\
         当前词元是那个**没被吃进 Label** 的名字 `L`（`parser.go:279`）"
    );
    let v = body_block(&r).stmts.clone();
    assert_eq!(v.len(), 1, "got {v:?}");
    let b = as_branch(v.into_iter().next().expect("len == 1"));
    assert_eq!(b.tok, BranchTok::Fallthrough);
    assert!(b.label.is_none(), "`L` 没被吃进 Label，got {:?}", b.label);
}

/// 四个分支关键字的 pos **都**落在关键字上（`nodes_test.go` 的 `@` 标记）。
/// 分开断言才照得出「某个 tok 的 pos 取错」——
/// `as_branch(...).pos` 在四种 tok 上走的是**同一段**代码，但值各不相同。
#[test]
fn t_a_53_四类分支的_pos_都落在关键字上() {
    for (src, tok) in [
        ("break", BranchTok::Break),
        ("continue", BranchTok::Continue),
        ("fallthrough", BranchTok::Fallthrough),
        ("goto L", BranchTok::Goto),
    ] {
        let b = as_branch(one_branchy(src));
        assert_eq!(b.tok, tok, "{src:?}");
        assert_eq!((b.pos.line, b.pos.col), (3, 1), "{src:?} 的 pos");
    }
}

// ── T-A-54：CallStmt ────────────────────────────────────────────────────

#[test]
fn t_a_54_go_调用() {
    let c = as_call(one("go f()"));
    assert_eq!(c.tok, CallTok::Go);
    assert!(matches!(c.call, Expr::Call(_)), "got {:?}", c.call);
    assert_eq!((c.pos.line, c.pos.col), (3, 1), "pos 落在 `go`");
}

#[test]
fn t_a_54_defer_调用() {
    let c = as_call(one("defer f()"));
    assert_eq!(c.tok, CallTok::Defer);
    assert!(matches!(c.call, Expr::Call(_)), "got {:?}", c.call);
    assert_eq!((c.pos.line, c.pos.col), (3, 1), "pos 落在 `defer`");
}

/// 官方 `CallStmt.Call` 的静态类型是 **`Expr`**（`nodes.go:413`），
/// 而 `callStmt`（`:995`）只是把 `pexpr` 的返回值**直接赋进去**、不做形状检查
/// ⇒ `go f` 语法合法，`Call == Name("f")`（类型检查才拒绝它）。
///
/// ⚠️ 这条用例钉的是**字段类型**：写成 `CallExpr` 的实现在这里无路可走
/// （要么炸、要么造一个假调用）。三处官方证据一致：`positions.go:296` 的
/// `m = n.Call`、`walk.go:247` 的 `w.node(n.Call)`、`printer.go:554` 的
/// `p.print(n.Tok, blank, n.Call)` —— 全按 `Expr` 用。
#[test]
fn t_a_54_go_后面不一定是调用() {
    let c = as_call(one("go f"));
    assert_eq!(c.tok, CallTok::Go);
    assert_eq!(name_of(&c.call), "f", "Call 就是那个 Name 本身");
}

/// 官方 `:995-1000`：`keep_parens` 为真，然后
/// `if t := Unparen(x); t != x` ⇒ 报 `expression in %s must not be parenthesized`
/// 并**用剥掉括号的那个**（`x = t`，**不 advance** —— 已经前进过了）。
///
/// ⚠️ 三条容易写错的细节：
/// 1. 消息走 `p.errorAt`（**不是** `syntaxErrorAt`）⇒ **没有** `syntax error: ` 前缀；
/// 2. 报错位置是 `x.Pos()` —— **括号**的位置，不是 `go` 的位置；
/// 3. `%s` 是 `s.Tok` ⇒ 文案随 `go`/`defer` 变（下一条用例）。
#[test]
fn t_a_54_括号包裹的调用报错并剥掉括号() {
    let r = stmts_raw("go (f())");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    assert_eq!(
        first_msg(&r),
        "expression in go must not be parenthesized",
        "`errorAt` 档 ⇒ 无 `syntax error: ` 前缀"
    );
    assert_eq!(first_pos(&r), (3, 4), "报错位置是**括号**，不是 `go`");
    let v = body_block(&r).stmts.clone();
    assert_eq!(v.len(), 1, "照样建出 CallStmt，got {v:?}");
    let c = as_call(v.into_iter().next().expect("len == 1"));
    assert!(
        matches!(c.call, Expr::Call(_)),
        "括号被剥掉 ⇒ 树里是裸 CallExpr，got {:?}",
        c.call
    );
}

/// 上一条的对照：同一个 `%s`，`defer` 时文案必须跟着变
/// （写死 `"go"` 的实现在这条上现形）。
#[test]
fn t_a_54_defer_的消息用_defer() {
    let r = stmts_raw("defer (f())");
    assert_eq!(r.errors, 1, "恰好一条：{:?}", r.diags);
    assert_eq!(
        first_msg(&r),
        "expression in defer must not be parenthesized"
    );
    assert_eq!(first_pos(&r), (3, 7), "`defer ` 六个字符 ⇒ 括号在第 7 列");
}

/// 官方 `Unparen`（`parser.go:2887`）是**递归**的（`return Unparen(x.X)`），
/// 所以 `go ((f()))` 会**剥掉两层**括号、并且只报**一条**错。
///
/// ⚠️ 这条用例是「只剥一层」的实现的**唯一**照出点：上一条只有**一层**括号，
/// 只剥一层与递归剥得到**同一个** `CallExpr` ⇒ 两种实现在那条上完全一致。
#[test]
fn t_a_54_括号剥到最里层() {
    let r = stmts_raw("go ((f()))");
    assert_eq!(r.errors, 1, "两层括号仍只报一条：{:?}", r.diags);
    assert_eq!(first_msg(&r), "expression in go must not be parenthesized");
    // 报错位置是**最外层**左括号（官方 `x.Pos()`，而 `x` 是剥之前的 `ParenExpr`）。
    assert_eq!(first_pos(&r), (3, 4));
    let v = body_block(&r).stmts.clone();
    let c = as_call(v.into_iter().next().expect("len == 1"));
    assert!(
        matches!(c.call, Expr::Call(_)),
        "两层括号都被剥掉，got {:?}",
        c.call
    );
}

/// `keep_parens` 为真的**唯一**理由就是这条错误 —— 所以「正常输入不剥括号」
/// 也要钉住：`go f()` 后面那个 `()` 是**调用**，不是分组括号。
/// （若 `keep_parens` 恒假，`go (f())` 就不会报错；若恒真且判据写错，
/// 这条正常输入会被误报。）
#[test]
fn t_a_54_正常调用不触发括号报错() {
    let r = stmts_raw("go f()\ndefer g()");
    assert_eq!(r.errors, 0, "不该报错：{:?}", r.diags);
    assert_eq!(body_block(&r).stmts.len(), 2, "两条调用语句");
}

// ── T-A-55：ReturnStmt ──────────────────────────────────────────────────

#[test]
fn t_a_55_裸_return() {
    let ret = as_return(one("return"));
    assert!(ret.results.is_empty(), "裸 return，got {:?}", ret.results);
    assert_eq!((ret.pos.line, ret.pos.col), (3, 1), "pos 落在 `return`");
}

#[test]
fn t_a_55_单值_return() {
    let ret = as_return(one("return x"));
    assert_eq!(ret.results.len(), 1, "got {:?}", ret.results);
    assert_eq!(name_of(&ret.results[0]), "x");
    // ⚠️ 这里必须**也**断言 `pos`：不写的话，`return_stmt` 把 `pos` 挪到
    // `next()` **之后**（= 结果表达式上）的变异**照不出来**
    // ——`t_a_55_裸_return` 会红（那里没有结果表、位置差得很远），
    // 而这条会静默通过。位置与结果表是**两处独立**的取值。
    assert_eq!((ret.pos.line, ret.pos.col), (3, 1), "pos 落在 `return`");
}

#[test]
fn t_a_55_多值_return() {
    let ret = as_return(one("return a, b, a + b*f(1, 2, 3)"));
    assert_eq!(ret.results.len(), 3, "got {:?}", ret.results);
    assert_eq!(name_of(&ret.results[0]), "a");
    assert_eq!(name_of(&ret.results[1]), "b");
    assert!(
        matches!(ret.results[2], Expr::Binary(_)),
        "第三个是 `a + b*f(1, 2, 3)`，got {:?}",
        ret.results[2]
    );
}

/// 官方 `:2698`：`if p.tok != _Semi && p.tok != _Rbrace { s.Results = p.exprList() }`
/// —— **两个**条件。`return` 后紧跟 `}` 时（同一行，没有自动分号）**不读**结果表。
///
/// ⚠️ 只写 `!= _Semi` 的实现会在这里试图把 `}` 当表达式解析，报
/// `expected expression` —— 而"报错了"算不上判据，才要**零诊断**那条断言。
#[test]
fn t_a_55_return_后紧跟右花括号时不读结果() {
    let src = wrap("for {\nreturn }");
    let r = parse(&src);
    assert_eq!(r.errors, 0, "不该报错：{:?}", r.diags);
    let v = body_block(&r).stmts.clone();
    assert_eq!(v.len(), 1, "got {v:?}");
    let Stmt::For(f) = &v[0] else {
        panic!("期望 ForStmt，got {:?}", v[0]);
    };
    assert_eq!(f.body.stmts.len(), 1, "for 体里一条 return");
    let ret = as_return(f.body.stmts[0].clone());
    assert!(
        ret.results.is_empty(),
        "`}}` 紧跟 ⇒ 不读结果表，got {:?}",
        ret.results
    );
}

/// 上一条的另一半：`return;`（显式分号）同样不读结果表。
#[test]
fn t_a_55_return_后紧跟分号时不读结果() {
    let v = stmts("return;");
    assert_eq!(v.len(), 1, "不产生多余空语句，got {v:?}");
    let ret = as_return(v.into_iter().next().expect("len == 1"));
    assert!(ret.results.is_empty(), "got {:?}", ret.results);
}

/// 官方 `walk.go:247` 是 `w.node(n.Call)` —— `CallStmt` **必须**把 `call` 子树算进节点数。
///
/// ⚠️ 这条用例的存在理由是 `walk.rs` 的 `Stmt::Call` 分支此前**没有任何判据**
/// （`rgoc-ast` 里唯一的 `count_nodes` 断言不含 `CallStmt`）⇒ 「把它改成 `1`」
/// 这类变异**照不出来**。补上之后才有照出的可能。
///
/// 用**两个文件的节点数之差**而不是绝对值：绝对值会把 `File`/`FuncDecl`/`FuncType`
/// 那一大片与本用例无关的结构也钉进来，改动无关节点时假红。
#[test]
fn walk_把_call_子树计入节点数() {
    // `return` ⇒ ReturnStmt(1)，结果表为空 ⇒ **1** 个节点。
    let n_return = count_nodes(file_of(&parse(&wrap("return"))));
    // `go f`   ⇒ CallStmt(1) + `Name("f")`(1) ⇒ **2** 个节点。
    let n_go = count_nodes(file_of(&parse(&wrap("go f"))));
    assert_eq!(
        n_go,
        n_return + 1,
        "`CallStmt` 必须遍历 `call`（官方 `walk.go:247` 的 `w.node(n.Call)`）：\
         return={n_return}、go f={n_go}"
    );
}

// ── 整段 dump ───────────────────────────────────────────────────────────

/// 整段其余语句的**中立 dump** —— 钉住"每个字段都出现了"这一层
/// （逐字段断言看不见"漏 dump 了一个字段"）。
///
/// 期望串不是**快照**（那样一改实现就得重录，掩盖了"哪一层变了"），
/// 而是把每条语句的**关键字段**都写死，让"漏了 `Tok` / `Label` / `Pos`"
/// 这类改动直接变成一处 diff。
#[test]
fn dump_其余语句整段形状() {
    // ⚠️ 输入必须**在分支语义上也干净**（`T-AP16` 起 `checkBranches` 会报）：
    // 每个标签都要被 `goto` 用掉，`break`/`continue` 要有合法的目标。
    // 早先的输入（`break L` 裸写 + 未使用的 `L2:`）会报三条分支诊断 ——
    // 那不是实现错，是这个用例的输入**不满足** `checkBranches` 的前提。
    let r = parse(&wrap(concat!(
        "var x = 0\n",
        "go f()\n",
        "defer g()\n",
        "return x\n",
        "goto L\n",
        "L:\n",
        "for {\n",
        "\tbreak L\n",
        "\tcontinue L\n",
        "}\n",
    )));
    assert_eq!(r.errors, 0, "不该报错：{:?}", r.diags);
    let d = dump(file_of(&r));
    let body = body_block(&r).stmts.clone();
    // 五个语句：Decl / Call(go) / Call(defer) / Return / Branch(goto) / Labeled
    assert_eq!(body.len(), 6, "got {body:?}");

    let lines: Vec<&str> = d.lines().collect();
    // 逐条钉住**关键字行**的相对次序与内容（子节点行由各自的家族测试覆盖）。
    let want: Vec<&str> = vec![
        "DeclStmt",
        "CallStmt",
        "Tok: go",
        "CallStmt",
        "Tok: defer",
        "ReturnStmt",
        "BranchStmt",
        "Tok: goto",
        "Label: Name \"L\"",
        "LabeledStmt",
        "Label: Name \"L\"",
        "ForStmt",
        "BranchStmt",
        "Tok: break",
        "Label: Name \"L\"",
        "BranchStmt",
        "Tok: continue",
        "Label: Name \"L\"",
    ];
    // 只保留**关键字行**（`want` 里列出那些）：子节点行（`Name "x"` / `BasicLit`
    // 等）由各自的家族测试覆盖，在这里混进来会让 diff 噪声盖过真信号。
    let got: Vec<&str> = lines
        .into_iter()
        .map(str::trim)
        .filter(|t| want.contains(t))
        .collect();
    assert_eq!(got, want, "整段形状变了");
}
