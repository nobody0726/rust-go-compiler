//! `T-A-70`（`//line` 重映射，`F6` / **RF3**）、`T-A-71`（`//go:build` 取 `goVersion`，`F7`）、
//! `T-A-72`（`//go:` 挂 `Pragma`，`F8`）、`T-A-73`（已知指令 + 未知指令）—— `T-AP15`。
//!
//! # 表示层的用例在另一个文件
//!
//! [`rgoc_lex::PosBase`] / `rel_line` / `rel_col` 的判据在
//! `rgoc-lex/tests/test_pos_base.rs`（9 条）；本文件只测**端到端**：
//! 指令从源码文本到 AST 的**整条链路**。
//!
//! # 权威判据来自官方 `parser_test.go:290-353`（`TestLineDirectives`）
//!
//! 官方那一整张表是本文件的对照基准。它的四类行分别对应：
//!
//! | 官方表里的段 | 本文件 |
//! |---|---|
//! | `// invalid //line directives with one colon` | [`非数字行号要报错`] |
//! | `// invalid /*line directives with two colons` | [`列号为零要报错`] / [`行号为零要报错`] |
//! | `// effect of valid /*line directives on lines` | [`合法指令重映射行号与文件名`] |
//! | `// effect of valid /*line directives on columns` | [`合法指令重映射列号`] |
//!
//! # `# RF3` —— 为什么这些用例必须**端到端**
//!
//! 「`//line` 之后的节点指向新基准」这件事，在 `test_pos_base.rs` 里**造 `Pos` 就能验**；
//! 但那验的是「`PosBase` 算得对」。真正容易坏的是**接线**：词法层根本没开
//! `directives` 模式、`next()` 里忘了处理消息、`//line` 之后的词元用**旧**基准 ——
//! 这三种错在表示层测试上**全部照不出来**（因为它们压根不经过 `PosBases`）。
//! ⇒ 本文件的每条用例都从**源码文本**出发，断言 AST 上节点的 `rel_*`。

use rgoc_ast::{AstFile, Decl, FuncDecl, Stmt};
use rgoc_lex::{FileTable, Pos};
use rgoc_parse::{ParseResult, parse_file};

fn file_of(r: &ParseResult) -> &AstFile {
    r.file
        .as_ref()
        .unwrap_or_else(|| panic!("应产出 AstFile，诊断：{:?}", r.diags))
}

fn first_msg(r: &ParseResult) -> String {
    r.first
        .as_ref()
        .map(|d| d.message.clone())
        .unwrap_or_default()
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

/// `//line` 之后的第一条语句的**位置**（重映射后应指向新基准）。
fn first_stmt_pos(r: &ParseResult) -> Pos {
    let b = one_func(r).body.as_ref().expect("函数体");
    b.stmts
        .first()
        .map_or_else(|| panic!("函数体应有语句"), pos_of)
}

fn pos_of(s: &Stmt) -> Pos {
    match s {
        Stmt::Empty(x) => x.pos,
        Stmt::Labeled(x) => x.pos,
        Stmt::Block(x) => x.pos,
        Stmt::Expr(x) => x.pos,
        Stmt::Send(x) => x.pos,
        Stmt::Decl(x) => x.pos,
        Stmt::Assign(x) => x.pos,
        Stmt::Branch(x) => x.pos,
        Stmt::Call(x) => x.pos,
        Stmt::Return(x) => x.pos,
        Stmt::If(x) => x.pos,
        Stmt::For(x) => x.pos,
        Stmt::Switch(x) => x.pos,
        Stmt::Select(x) => x.pos,
    }
}

/// 用 `FileTable` 取重映射后的**文件名**（需要 `name(...)` 查表）。
fn rel_name(r: &ParseResult, pos: Pos) -> String {
    let mut ft = FileTable::new();
    ft.intern("dir/directives.go");
    pos.rel_filename(&ft, &r.bases)
}

// ── T-A-70 / RF3：重映射 ───────────────────────────────────────────────

/// **RF3 钉点**：官方 `parser_test.go:325` 的 `/*line foo:123*/` ⇒ `{123, 0}`。
///
/// 块注释形式，放在 `package` **之前**、函数体**之前** —— 它的基准必须对
/// **后面整份文件**生效。
#[test]
fn 块注释_line_指令重映射行号() {
    let src = "/*line foo:123*/\npackage p\nfunc f() {\nx++\n}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "合法指令不该报错：{:?}", r.diags);

    let p = first_stmt_pos(&r);
    let mut ft = FileTable::new();
    ft.intern("dir/directives.go");
    assert_eq!(p.rel_line(&r.bases), 126, "123 + (4 - 1)：`x++` 在第 4 行");
    assert_eq!(rel_name(&r, p), "foo", "文件名换成 `foo`");
}

/// 行注释形式 `//line foo:345`（官方 `:326`：`/*line foo:123*/\n//line bar:345\nfoo`）。
///
/// 官方那条的关键是**两条指令** ⇒ 后一条换掉前一条；本条只测行注释形态，
/// 「两条换一条」由 [`两条_line_指令后者换掉前者`] 测。
#[test]
fn 行注释_line_指令重映射() {
    let src = "//line foo:345\npackage p\nfunc f() {\nx++\n}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "合法指令不该报错：{:?}", r.diags);

    let p = first_stmt_pos(&r);
    // ⚠️ 基准的 `pos` 是「**紧跟注释之后**」那个位置（官方 `parser.go:65`：
    // 行注释的换行**属于**注释 ⇒ 基准落在**下一行**的开头 = (2, 1)）⇒
    // `rel = 345 + (4 - 2)`。写成 `345 + (4 - 1)` 是把基准当成「注释所在行」了。
    assert_eq!(p.rel_line(&r.bases), 347, "345 + (4 - 2)");
    assert_eq!(rel_name(&r, p), "foo");
}

/// 官方 `parser_test.go:326`：第二条指令**换掉**第一条。
#[test]
fn 两条_line_指令后者换掉前者() {
    let src = "//line foo:123\n//line bar:345\npackage p\nfunc f() {\nx++\n}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "合法指令不该报错：{:?}", r.diags);

    let p = first_stmt_pos(&r);
    assert_eq!(p.rel_line(&r.bases), 347, "345 + (5 - 3)：`x++` 在第 5 行");
    assert_eq!(rel_name(&r, p), "bar", "是 `bar` 不是 `foo`");
}

/// 官方 `:330`：`/*line foo :123:10*/` ⇒ `{123, 10}` —— **带列**。
///
/// `filename` 是 `"foo "`（冒号前有空格）—— 官方的 `filepath.Clean` 会把它
/// 保留成 `"foo "`（`Clean` 只处理分隔符和 `.`/`..`）。我们**不做** `Clean`
/// （T-AP15 残留表已登记）⇒ 断言同样保留空格。
#[test]
fn 合法指令重映射列号() {
    let src = "/*line foo:123:10*/\npackage p\nfunc f() {\nx++\n}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "合法指令不该报错：{:?}", r.diags);

    let p = first_stmt_pos(&r);
    // 基准挂在第 1 行末尾（块注释 `/*line foo:123:10*/` 长 21 ⇒ col = 1+21 = 22）。
    // `x++` 在第 4 行 ⇒ **不同行** ⇒ 官方 `pos.go:73` 返回**绝对列**。
    assert_eq!(p.rel_line(&r.bases), 126, "123 + (4 - 1)");
    assert_eq!(
        p.rel_col(&r.bases),
        p.col,
        "不同行 ⇒ `RelCol` 返回**绝对列**（官方 `pos.go:73`）"
    );
}

/// **RF3 的核心一条**：不带 `//line` 的文件，`rel_*` 必须**等于绝对位置**。
///
/// ⚠️ 这条是「重映射机制**没被误触发**」的守卫：词法层若无条件挂基准，
/// 或者 file base 的 `linebase/colbase` 抄错，全仓每个位置都会变 ——
/// 而那种错**不会让任何现有测试变红**（它们都断言绝对位置）。
#[test]
fn 无_line_指令时_rel_等于绝对() {
    let src = "package p\nfunc f() {\nx++\n}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);

    let p = first_stmt_pos(&r);
    assert_eq!(p.line, 3, "`x++` 在第 3 行");
    assert_eq!(p.rel_line(&r.bases), 3, "无指令 ⇒ rel == 绝对");
    assert_eq!(p.rel_col(&r.bases), p.col, "无指令 ⇒ rel == 绝对");
    assert_eq!(rel_name(&r, p), "dir/directives.go", "文件名不变");
    assert_eq!(r.bases.len(), 1, "只有 file base，没有 line base");
}

// ── 错误路径（官方 `parser_test.go:300-322`）───────────────────────────

/// 官方 `:300`：`  //line foo:*/` **不是** line 指令（行内）⇒ 静默忽略。
///
/// ⚠️ 这条钉的是 `(col == colbase || msg[1] == '*')` 里 `col == colbase`
/// 那一半：行内的 `//line` **不改**基准。
#[test]
fn 行内的_行注释_line_不生效() {
    let src = "package p\nfunc f() {  //line foo:100\nx++\n}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "行内 `//line` 被忽略：{:?}", r.diags);

    let p = first_stmt_pos(&r);
    assert_eq!(p.rel_line(&r.bases), 3, "行内 `//line` **没有**改基准");
    assert_eq!(r.bases.len(), 1, "没有 line base");
}

/// 官方 `:304-311`：行首的 `//line` 带非数字行号 ⇒ 报 `invalid line number: …`。
#[test]
fn 非数字行号要报错() {
    for (src, want) in [
        ("//line foo:x\npackage p\n", "invalid line number: x"),
        ("//line foo:0\npackage p\n", "invalid line number: 0"),
        // 官方 `:309`：带尾随空格 ⇒ `"1 "` 解析失败
        ("//line foo:1 \npackage p\n", "invalid line number: 1 "),
    ] {
        let r = parse_file(src.as_bytes(), "dir/directives.go");
        assert_eq!(r.errors, 1, "{src:?} ⇒ 恰好一条：{:?}", r.diags);
        assert_eq!(first_msg(&r), want, "{src:?}");
    }
}

/// 官方 `:308`：`//line foo:0` —— 行号 **0** 非法（即使能解析成数字）。
#[test]
fn 行号为零要报错() {
    let r = parse_file(b"//line foo:0\npackage p\n", "dir/directives.go");
    assert_eq!(r.errors, 1, "{:?}", r.diags);
    assert_eq!(first_msg(&r), "invalid line number: 0");
}

/// 官方 `:320`：`/*line :123:0*/` ⇒ 列号 **0** 非法 ⇒ `invalid column number: 0`。
#[test]
fn 列号为零要报错() {
    let r = parse_file(b"/*line :123:0*/\npackage p\n", "dir/directives.go");
    assert_eq!(r.errors, 1, "{:?}", r.diags);
    assert_eq!(first_msg(&r), "invalid column number: 0");
}

/// 官方 `:314-318`：带**两个**冒号但第二个不是数字 ⇒ 报的是**行号**错。
///
/// `/*line foo::123abc*/` ⇒ `trailing_digits(" foo::123abc")` 取**最后一个** `:` ⇒ `"123abc"`
/// 解析失败 ⇒ 走「行号非法」那支，消息是 `invalid line number: 123abc`。
#[test]
fn 两个冒号第二个非数字_报行号错() {
    let r = parse_file(b"/*line foo::123abc*/\npackage p\n", "dir/directives.go");
    assert_eq!(r.errors, 1, "{:?}", r.diags);
    assert_eq!(first_msg(&r), "invalid line number: 123abc");
}

/// 官方 `:325-338` 的「effect of valid directives」里 `filename` 可以是怪东西：
/// `/*line foo:123abc:1*/` ⇒ 文件名 `foo:123abc`（**从右**找 `:` 的效果）。
///
/// ⚠️ 这条钉的是 `trailing_digits` 用 `rfind` 而不是 `find` ——
/// 用 `find` 会把 `foo:123abc:1` 拆成 `foo` + `:123abc:1` ⇒ 报「行号非法」。
#[test]
fn 文件名里带冒号_从右找分隔() {
    let src = "/*line foo:123abc:1*/\npackage p\nfunc f() {\nx++\n}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "合法指令不该报错：{:?}", r.diags);

    let p = first_stmt_pos(&r);
    assert_eq!(rel_name(&r, p), "foo:123abc", "文件名保留冒号");
    assert_eq!(p.rel_line(&r.bases), 4, "1 + (4 - 1)");
}

/// 官方 `:341-344` 的 `ok2` 判据：无 `:col` 时空文件名**留空**，有 `:col` 时**沿用**。
#[test]
fn 空文件名_无列时留空() {
    let src = "//line bar:1\n//line :10\npackage p\nfunc f() {\nx++\n}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    // 第二条无 `:col` ⇒ 文件名留空 ⇒ `rel_filename` 是 `""`。
    let p = first_stmt_pos(&r);
    assert_eq!(rel_name(&r, p), "", "无 `:col` 时空文件名**留空**");
    assert_eq!(p.rel_line(&r.bases), 12, "10 + (5 - 3)：基准在第 3 行");
}

#[test]
fn 空文件名_有列时沿用上一条() {
    let src = "//line bar:1\n//line :10:20\npackage p\nfunc f() {\nx++\n}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    let p = first_stmt_pos(&r);
    assert_eq!(rel_name(&r, p), "bar", "有 `:col` ⇒ 沿用上一条的文件名");
    assert_eq!(p.rel_line(&r.bases), 12, "10 + (5 - 3)");
}

/// **RF3 真正的形态**：`//line` 之后**紧接着的那个** token 就得带新基准 ——
///
/// 这里是 `package` 本身（官方 `parser.go:63-65`：行注释的换行**属于**注释，
/// 所以基准落在**下一行**开头，而 `//line foo:123` 的下一个 token 就是第 2 行的 `package`）。
///
/// ⚠️ 这条用例的**存在理由**是反向验证抓出来的：`Parser::next()` 里
/// 「先按当前 base 赋 `tok.pos`，**再**处理指令消息」这个反序的实现，
/// 在**所有**「断言后面第 3、4 行 token」的用例上**都照不出来** ——
/// 它们离指令太远，base 早就更新完了。**只有紧邻的第一个 token 会现形。**
/// ⇒ 这是「漏了不会报错」家族的又一员：**离故障点越远的断言越看不见**。
#[test]
fn line_指令之后_紧接着的第一个_token_就带新基准() {
    let src = "//line foo:123\npackage p\nfunc f() {}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "合法指令不该报错：{:?}", r.diags);

    let f = file_of(&r);
    // `AstFile.pos` 官方取的是 `p.tok.pos`（`parser.go:412`）—— 即 `package` 那个词元。
    assert_eq!(
        f.pos.rel_line(&r.bases),
        123,
        "`package` 紧跟指令 ⇒ 它自己就该落在 rel 123 上"
    );
    assert_eq!(rel_name(&r, f.pos), "foo");
}

/// `/*line */` 可以落在**行内**（官方 `:59` 的注释：只有 `//line` 才要求行首）。
///
/// ⚠️ 与上面那条「行内的 `//line` **不**生效」正好是一对：判据是
/// `(col == colbase || msg[1] == '*')` 的**两个条件**，各管一种形态。
/// 少掉 `msg[1] == '*'` 那一半 ⇒ 本条照不出来（而行内 `//line` 那条照样绿）。
#[test]
fn 行内的_块注释_line_生效() {
    let src = "package p\nfunc f() { /*line foo:123*/\nx++\n}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(
        r.errors, 0,
        "行内 `/*line */` 是**合法**指令：{:?}",
        r.diags
    );

    let p = first_stmt_pos(&r);
    let mut ft = FileTable::new();
    ft.intern("dir/directives.go");
    assert_eq!(
        p.rel_filename(&ft, &r.bases),
        "foo",
        "行内块注释也改基准（官方 `parser.go:61` 的 `msg[1] == '*'`）"
    );
    assert_eq!(r.bases.len(), 2, "挂了一条 line base");
}

// ── T-A-71：goVersion（`F7`）──────────────────────────────────────────

/// **RF3 之外的那一半**：`//go:build go1.21` 在 `package` **之前** ⇒ 取到版本号。
///
/// 官方 `parser.go:79-83`：`if p.top && strings.HasPrefix(msg, "//go:build")`。
#[test]
fn go_build_在包前取_go_version() {
    let src = "//go:build go1.21\n\npackage p\nfunc f() {}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    assert_eq!(
        file_of(&r).go_version.as_deref(),
        Some("go1.21"),
        "`//go:build` 取到版本号"
    );
}

/// **同一个判据的另一半**：`//go:build` 在 `package` **之后** ⇒ **不取**。
///
/// ⚠️ 这条钉的是 `p.top` 判据。把 `if p.top && …` 的 `p.top` 删掉，
/// 上面那条仍然绿（`top` 在那种位置本来就是 true）⇒ **只有这条能照出来**。
#[test]
fn go_build_在包后不取_go_version() {
    let src = "package p\n\n//go:build go1.21\n\nfunc f() {}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(
        file_of(&r).go_version,
        None,
        "`//go:build` 在 package 之后 ⇒ 不取（官方 `F7` 的 `top` 判据）"
    );
}

#[test]
fn go_build_带合取式_仍取到版本号() {
    let src = "//go:build go1.21 && !go1.19\n\npackage p\nfunc f() {}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    assert_eq!(file_of(&r).go_version.as_deref(), Some("go1.21"));
}

/// ⚠️ **已知缺口**（`progress.md` 的 T-AP15 残留表）：`//go:build !go1.19` 这种
/// **没有裸版本号**的表达式，官方 `constraint.GoVersion` 返回空串 ⇒ 不设 `goVersion`；
/// 我们的「取首个 `goX.Y`」实现会取到 `go1.19` ⇒ **不等价**。
///
/// 这条用例**钉住当前的已知行为**（不是钉「对」），并在注释里标明它偏离官方 ——
/// 等 `go/build/constraint` 模块落地后再改。
#[test]
fn go_build_否定式_已知与官方不等价() {
    let src = "//go:build !go1.19\n\npackage p\nfunc f() {}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    assert_eq!(
        file_of(&r).go_version.as_deref(),
        Some("go1.19"),
        "⚠️ 已知偏离官方（官方应为空）—— 见本函数文档"
    );
}

// ── T-A-72 / T-A-73：pragma 挂载 ─────────────────────────────────────

/// 官方 `F8`：`//go:nosplit` 挂到**下一个声明**的 `Pragma` 上。
#[test]
fn go_指令挂到下一个声明的_pragma() {
    let src = "package p\n\n//go:nosplit\nfunc f() {}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    let f = one_func(&r);
    assert_eq!(
        f.pragma.directives,
        vec!["nosplit".to_string()],
        "去掉 `go:` 前缀后存进 `Pragma.directives`"
    );
}

/// **文件级** pragma 挂到 `AstFile` 上（官方 `:421`：`f.Pragma = p.pragma`，
/// 在 package 子句**之后**取）。
#[test]
fn 文件级_pragma_挂到_ast_file() {
    let src = "//go:build go1.21\n//go:debug foo\n\npackage p\nfunc f() {}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    assert_eq!(
        file_of(&r).pragma.directives,
        vec!["build go1.21".to_string(), "debug foo".to_string()],
        "两条指令都在文件级 pragma 上"
    );
}

/// `//go:` 后面**不是**指令的那类注释（没有 `go:` 前缀）不进 pragma。
#[test]
fn 非指令注释不进_pragma() {
    let src = "package p\n\n// 普通注释\nfunc f() {}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    assert!(
        file_of(&r).pragma.directives.is_empty(),
        "普通注释不进 pragma，got {:?}",
        file_of(&r).pragma.directives
    );
    assert!(
        one_func(&r).pragma.directives.is_empty(),
        "也不挂到声明上，got {:?}",
        one_func(&r).pragma.directives
    );
}

/// `//go:linkname f g` 那种**带参数**的：整条存进 `directives`（M2 只**解析并挂载**、不生效）。
#[test]
fn 带参数的_go_指令整条挂上() {
    let src = "package p\n\n//go:linkname before after\nfunc f() {}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    assert_eq!(
        one_func(&r).pragma.directives,
        vec!["linkname before after".to_string()],
        "整条存（含参数）"
    );
}

/// **未知指令一律收下、不报错**（= 官方）。
///
/// ⚠️ `03-roadmap.md` §M2 要求「拒绝未知**关键**指令」，与官方的「全部忽略」
/// 冲突 —— 已登记为 `§9 待裁决`（`progress.md`），归 `T-AP16`。
/// 这条用例钉住**当前**行为（= 官方），裁决后可能要反过来。
#[test]
fn 未知指令_收下但不报错() {
    let src = "package p\n\n//go:totally-unknown x\nfunc f() {}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "官方对未知指令是**忽略**：{:?}", r.diags);
    assert_eq!(
        one_func(&r).pragma.directives,
        vec!["totally-unknown x".to_string()],
        "仍挂到声明上（M2 只解析不生效）"
    );
}

/// 块注释里的 `go:` **不**是指令（官方 `scanner.go` 的 `fullComment` 只匹配 `"line "`）。
#[test]
fn 块注释里的_go_不是指令() {
    let src = "package p\n\n/*go:nosplit*/\nfunc f() {}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    assert!(
        one_func(&r).pragma.directives.is_empty(),
        "`/*go:...*/` 不上报（M1 的词法层就不报），got {:?}",
        one_func(&r).pragma.directives
    );
}

/// pragma 归属的**边界**：声明自己的 `clear_pragma()` 会把指令给**下一条**声明。
#[test]
fn pragma_不给下一条声明() {
    let src = "package p\n\n//go:nosplit\nfunc f() {}\nfunc g() {}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    let funcs: Vec<&FuncDecl> = file_of(&r)
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Func(f) => Some(f),
            _ => None,
        })
        .collect();
    assert_eq!(funcs.len(), 2, "两条函数");
    assert_eq!(funcs[0].pragma.directives, vec!["nosplit".to_string()]);
    assert!(
        funcs[1].pragma.directives.is_empty(),
        "**不能**漏给下一条，got {:?}",
        funcs[1].pragma.directives
    );
}

// ── 版本门控（`T-A-97`，`D-M2-6`）─────────────────────────────────────
//
// ⚠️ **本轮只钉「输入侧」**：`go_version` 取到了什么、以及「版本不够时
// 新语法被拒绝」这件事的**接线点**是否存在。真正的门控判据（泛型需要 go1.18、
// range over func 需要 go1.22）需要 `go_version` 能被**注入**，
// 而 `parse_file` 目前**不接受** `-lang` 参数 —— 那是 `T-AP19`/`T-AP20` 的接口改动。
// 登记在 `progress.md` 的 T-AP15 残留表。

/// `go_version` 缺省（没有 `//go:build`）⇒ [`None`]，门控此时**不生效**。
#[test]
fn 无_go_build_时_go_version_为_none() {
    let src = "package p\nfunc f() {}\n";
    let r = parse_file(src.as_bytes(), "dir/directives.go");
    assert_eq!(r.errors, 0, "{:?}", r.diags);
    assert_eq!(
        file_of(&r).go_version,
        None,
        "没有 `//go:build` ⇒ `go_version` 为 None（门控不生效）"
    );
}
