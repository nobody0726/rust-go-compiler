//! 错误消息复刻（`T-AP16`，`T-A-66`）—— 官方 `syntaxErrorAt`（`parser.go:252-297`）。
//!
//! # 这个文件在验什么
//!
//! `M2-design.md` §7.2 要求「从官方 `syntaxError` 的**调用点**枚举全部消息模板，
//! 冻结条目数」——本文件是那条冻结的**可执行形式**：每条消息至少有一个
//! **端到端**用例，让「消息变了」这件事**红灯**，而不是留到 `T-AP18` 对撞才发现。
//!
//! # 官方 `syntaxErrorAt` 的**三支**（`parser.go:262-273`）
//!
//! ```text
//! msg == ""                                → 什么都不加
//! HasPrefix(msg, "in "/"at "/"after ")      → msg = " " + msg
//! HasPrefix(msg, "expected ")              → msg = ", " + msg
//! 其余                                      → 直接 "syntax error: " + msg 并 return
//! ```
//!
//! 前两支之后都要**拼上当前词元**（`:275-293`）：
//!
//! | 当前词元 | 拼出的 `tok` |
//! |---|---|
//! | `_Name` | `name ` + 字面原文 |
//! | `_Semi` | `p.lit` —— `"newline"` / `"semicolon"` / `"EOF"` |
//! | `_Literal` | `literal ` + 字面原文 |
//! | `_Operator` | `op.String()` |
//! | `_AssignOp` | `op.String()` + `"="` |
//! | `_IncOp` | `op.String()` |
//! | 其余 | `tokstring(p.tok)`（`:304-316`） |
//!
//! 最终消息是 `syntax error: unexpected <tok><中缀>`。
//!
//! # 为什么 `tokstring` 的三个特例必须逐条钉住
//!
//! `tokstring`（`parser.go:304-316`）只有三个特例：
//!
//! - `_Comma` → `"comma"`（**不是** `","`）
//! - `_Semi` → `"semicolon or newline"`（**不是** `";"`）
//! - `break..var` 区间（25 个关键字）→ `"keyword "` + `tok.String()`
//!
//! 这三条**每一个都容易按直觉写错**（写成 `","` / `";"` / `"chan"`），
//! 而且**写错时语法上完全正常** —— 症状只是消息里几个字符不同，
//! 要到 `T-AP18` 对撞才暴露。所以每条一个用例。

use rgoc_lex::diag::DiagCode;
use rgoc_parse::ParseResult;
use rgoc_parse::parse_file;

/// 解析并返回首条诊断的消息（没有诊断就 panic —— 「没报错」不能被当成「消息对」）。
fn msg_of(src: &str) -> String {
    let mut r = parse_file(src.as_bytes(), "t.go");
    diags_of(&mut r)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("应至少报一条，源码：{src:?}"))
}

fn diags_of(r: &mut ParseResult) -> Vec<String> {
    // `DiagBag` **刻意**不实现 `Clone`（`C1` §4.3：取用只有 `into_sorted` 一条路，
    // 它**消费** bag）。所以这里 `mem::take` 把 bag 搬出来 —— 顺带也验证了
    // 「搬走之后 bag 为空」这条契约仍然成立（下面的用例只读 `messages`）。
    std::mem::take(&mut r.diags)
        .into_sorted()
        .into_iter()
        .map(|d| d.message)
        .collect()
}

fn codes_of(r: &mut ParseResult) -> Vec<DiagCode> {
    std::mem::take(&mut r.diags)
        .into_sorted()
        .into_iter()
        .map(|d| d.code)
        .collect()
}

/// 全仓**唯一**的 `syntax error:` 前缀断言助手。
///
/// 官方 `errorAt`（如 `expression in go must not be parenthesized`）**不带**这个
/// 前缀 ⇒ 用它验「前缀规则」等于没验。本助手只服务 `syntaxError` 路径。
fn syntax_msgs(src: &str) -> Vec<String> {
    let mut r = parse_file(src.as_bytes(), "t.go");
    diags_of(&mut r)
        .into_iter()
        .filter(|m| m.starts_with("syntax error: "))
        .collect()
}

// ══════════════════════════════════════════════════════════════════════
// T-A-66①：`expected ` 那一支 ⇒ `unexpected <tok>, expected <Y>`
// ══════════════════════════════════════════════════════════════════════

/// `want(_Semi)`（官方 `parser.go:423`，package 子句收尾）—— **`_Semi` 特例**。
///
/// `package p func f() {}`：package 名之后直接跟 `func`。
///
/// 官方 `tokstring(_Semi) = "semicolon or newline"` —— **不是** `";"`。
/// 写成 `";"` 时消息仍读得通，只是与 oracle 差一段 ⇒ 本文件专门盯这条。
#[test]
fn t_a_66_expected_分号是_semicolon_or_newline() {
    assert_eq!(
        msg_of("package p func f() {}\n"),
        "syntax error: unexpected keyword func, expected semicolon or newline",
        "官方 `tokstring(_Semi) = \"semicolon or newline\"`（`parser.go:309`）"
    );
}

/// ⚠️ **反向验证**：分号那一条**不能**写成 `expected ;`。
///
/// 与上一条同输入、断言相反的结论：只写上一条时，一个「无差别用 `;`」
/// 的 `tokstring` 同样能过。
#[test]
fn t_a_66_分号_不是_分号字符() {
    for m in syntax_msgs("package p func f() {}\n") {
        assert!(
            !m.contains("expected ;"),
            "官方给的是 `semicolon or newline`；got {m:?}"
        );
    }
}

/// `want(_Chan)`（官方 `parser.go:1367`）—— **关键字前缀**那条。
///
/// `var x <- 42`：`←` 之后必须是 `chan`。
///
/// 官方 `tokstring(_Chan) = "keyword chan"` ⇒ **期望侧**带 `keyword ` 前缀；
/// 而**当前词元**侧（`42` 是字面量）走 `_Literal` 分支 ⇒ `"literal 42"`。
/// 一个用例同时照出两档拼法。
#[test]
fn t_a_66_expected_关键字带_keyword_前缀() {
    let msgs = syntax_msgs("package p\nfunc f() {\n\tvar x <- 42\n}\n");
    assert!(
        msgs.iter()
            .any(|m| m == "syntax error: unexpected literal 42, expected keyword chan"),
        "官方 `tokstring` 的关键字特例 ⇒ 期望侧是 `keyword chan`；got {msgs:?}"
    );
}

/// ⚠️ **反向验证**：关键字那一条**不能**是裸 `expected chan`。
#[test]
fn t_a_66_关键字_不是_裸拼写() {
    for m in syntax_msgs("package p\nfunc f() {\n\tvar x <- 42\n}\n") {
        assert!(
            !m.contains(", expected chan"),
            "`tokstring` 给关键字加 `keyword ` 前缀（官方 `parser.go:313`）；got {m:?}"
        );
    }
}

/// `comma` 特例：走 `list` 的 `possibly missing %s or %s`（官方 `parser.go:517`）。
///
/// `func f(a int b int)` —— 参数表里 `a int` 之后缺 `,`。
///
/// ⚠️ 这条**不走 `expected ` 那一支**（官方 29 处 `p.want(` 里**没有**
/// `_Comma`）⇒ 形态是 `unexpected <tok> in <ctx>; possibly missing <sep> or <close>`。
/// 而 `<sep>` 那一格就是 `tokstring(_Comma) = "comma"` —— 官方**不写** `","`。
#[test]
fn t_a_66_逗号是_comma() {
    let msgs = syntax_msgs("package p\nfunc f(a int b int) {}\n");
    assert!(
        msgs.iter().any(|m| m
            == "syntax error: unexpected name b in parameter list; possibly missing comma or )"),
        "官方 `tokstring(_Comma) = \"comma\"`；got {msgs:?}"
    );
}

/// ⚠️ **反向验证**：逗号**不能**写成字符 `,`（两处形态都要查）。
#[test]
fn t_a_66_逗号_不是_逗号字符() {
    for src in [
        "package p\nfunc f(a int b int) {}\n",
        "package p\nfunc f() {\n\tvar x <- 42\n}\n",
        "package p func f() {}\n",
    ] {
        for m in syntax_msgs(src) {
            assert!(
                !m.contains("missing , ") && !m.contains("expected ,"),
                "官方 `tokstring(_Comma) = \"comma\"`（单词）；got {m:?}"
            );
        }
    }
}

/// 期望词元是普通分隔符 ⇒ **不加**任何前缀（`tokstring(_Lbrace) = "{"`）。
///
/// `var x struct 42` ⇒ `p.want(_Lbrace)`（官方 `parser.go:1551`）不中。
#[test]
fn t_a_66_expected_普通分隔符不加前缀() {
    let msgs = syntax_msgs("package p\nfunc f() {\n\tvar x struct 42\n}\n");
    assert!(
        msgs.iter()
            .any(|m| m == "syntax error: unexpected literal 42, expected {"),
        "`tokstring(_Lbrace) = \"{{\"` —— 无任何前缀；got {msgs:?}"
    );
}

/// `want(keyword)`（官方 `parser.go:2353`，`if`/`for`/`switch` 的头部）。
///
/// 这是**本任务修掉的第一个偏差**：改之前我们传的是裸 `for`/`if`/`switch`
/// （`keyword.keyword_str()`），官方是 `keyword for` / `keyword if`。
#[test]
fn t_a_66_want_keyword_用_tokstring_派生() {
    // `for` 头部：官方 `header` 的 `p.want(keyword)` 在缺 `for` 时不中。
    // `x for y` —— `exprList` 之后 `want(_For)` 不中。
    let msgs = syntax_msgs("package p\nfunc f() {\n\tx for y\n}\n");
    assert!(
        msgs.iter().any(|m| m.contains("expected keyword for")),
        "官方 `tokstring(_For) = \"keyword for\"`；got {msgs:?}"
    );
}

/// ⚠️ **反向验证**：`want(keyword)` **不能**给裸 `for`。
#[test]
fn t_a_66_want_keyword_不是_裸拼写() {
    for m in syntax_msgs("package p\nfunc f() {\n\tx for y\n}\n") {
        assert!(
            !m.contains("expected for"),
            "`want(keyword)` 经 `tokstring` ⇒ `expected keyword for`；got {m:?}"
        );
    }
}

// ══════════════════════════════════════════════════════════════════════
// T-A-66②：`in ` / `at ` / `after ` 那一支 ⇒ `unexpected <tok> <msg>`
// ══════════════════════════════════════════════════════════════════════

/// `in ` 那一支 ⇒ **只**前置一个空格（无逗号），且 `<sep>/<close>` 走 `tokstring`。
#[test]
fn t_a_66_in_那一支只加空格() {
    let msgs = syntax_msgs("package p\nfunc f(a int b int) {}\n");
    assert!(
        msgs.iter().any(|m| m
            == "syntax error: unexpected name b in parameter list; possibly missing comma or )"),
        "官方 `:265-266`：`in `/`at `/`after ` ⇒ 前置 `\" \"`；got {msgs:?}"
    );
}

/// ⚠️ **反向验证**：`in ` 分支**不能**带逗号 —— 与 `expected ` 分支的判据相反。
#[test]
fn t_a_66_in_那一支不带逗号() {
    for src in [
        "package p\nfunc f(a int b int) {}\n",
        "package p\nfunc f() {\n\tvar x struct{ a int b int }\n}\n",
    ] {
        for m in syntax_msgs(src) {
            if m.contains(" in ") {
                assert!(
                    !m.contains(", in "),
                    "`in ` 分支前置的是空格不是 `\", \"`；got {m:?}"
                );
            }
        }
    }
}

/// `at ` 那一支：`at end of statement`（官方 `parser.go:2743`）。
///
/// `var 1x int` ⇒ `1x` 被拆成字面量 `1` + 名字 `x`，`x` 成了没有分隔符的尾随。
#[test]
fn t_a_66_at_那一支只加空格() {
    let msgs = syntax_msgs("package p\nfunc f() {\n\tvar 1x int\n}\n");
    assert!(
        msgs.iter()
            .any(|m| m == "syntax error: unexpected name int at end of statement"),
        "官方 `at ` 前缀 ⇒ 前置一个空格；got {msgs:?}"
    );
}

/// ⚠️ **反向验证**：`at ` 分支也不带逗号。
#[test]
fn t_a_66_at_那一支不带逗号() {
    for m in syntax_msgs("package p\nfunc f() {\n\tvar 1x int\n}\n") {
        if m.contains(" at end of statement") {
            assert!(
                !m.contains(", at end of statement"),
                "`at ` 分支前置的是空格；got {m:?}"
            );
        }
    }
}

/// `after ` 那一支：官方 `parser.go:460` 的
/// `unexpected keyword func after top level declaration`。
#[test]
fn t_a_66_after_那一支只加空格() {
    let msgs = syntax_msgs("package p\nimport \"a\" func f() {}\n");
    assert!(
        msgs.iter()
            .any(|m| m == "syntax error: unexpected keyword func after top level declaration"),
        "官方 `after ` 前缀 ⇒ 前置一个空格；got {msgs:?}"
    );
}

/// ⚠️ **反向验证**：`after ` 分支不带逗号。
#[test]
fn t_a_66_after_那一支不带逗号() {
    for m in syntax_msgs("package p\nimport \"a\" func f() {}\n") {
        if m.contains(" after top level declaration") {
            assert!(
                !m.contains(", after top level declaration"),
                "`after ` 分支前置的是空格；got {m:?}"
            );
        }
    }
}

/// `default` 那一支 ⇒ **不拼当前词元**。
///
/// 官方 `parser.go:465` 的 `unexpected semicolon or newline before {`
/// —— 消息不以 `in `/`at `/`after `/`expected ` 开头 ⇒ 走 `default` 支，
/// **原样**加 `syntax error: ` 前缀（`:271-272`）。
#[test]
fn t_a_66_default_那一支不拼当前词元() {
    let msgs = syntax_msgs("package p\nfunc f()\n{ }\n");
    assert!(
        msgs.iter()
            .any(|m| m == "syntax error: unexpected semicolon or newline before {"),
        "官方 `:271` 走 `default` 支 ⇒ 消息里**没有** `unexpected <当前词元>` 重复段；got {msgs:?}"
    );
}

/// ⚠️ **反向验证**：`default` 支的消息**不能**被改写成 `unexpected <tok> …`。
///
/// `import path must be a string`（官方 `parser.go:668`）也是 `default` 支 ——
/// 用它做对照：消息**原样**透传，一个 `unexpected …` 都不许有。
#[test]
fn t_a_66_default_那一支不引入_当前词元() {
    for m in syntax_msgs("package p\nimport 42\n") {
        assert_eq!(
            m, "syntax error: import path must be a string",
            "`default` 支的消息原样透传（官方 `:271`）；got {m:?}"
        );
    }
}

// ══════════════════════════════════════════════════════════════════════
// T-A-66③：`<tok>` 的六种拼法（`parser.go:275-293`）
// ══════════════════════════════════════════════════════════════════════

/// `_Semi` 的 `tok` 用 **`p.lit`** —— 分号有**三种来源**，各给一个英文单词。
///
/// 官方 `scanner.go`：`:154` 显式 `;` ⇒ `"semicolon"`；`:121` 换行补 ⇒ `"newline"`；
/// `:113` 文件尾补 ⇒ `"EOF"`。
///
/// ⚠️ 这三个字符串**都不是** `";"` ⇒ 一个「`_Semi` 一律用 `;`」的实现
/// **不会被**「`tokstring` 的 `semicolon or newline`」那条照出来
/// （那一条走的是 `tokstring` 分支，不是本分支）。所以必须单独钉。
///
/// 输入：`func f() (int; int)` —— 参数表里显式 `;` 出现在 `want(_Comma)` 不中的位置。
#[test]
fn t_a_66_semi_的_tok_显式分号是_semicolon() {
    let msgs = syntax_msgs("package p\nfunc f() (int; int) { return 0 }\n");
    assert!(
        msgs.iter()
            .any(|m| m.starts_with("syntax error: unexpected semicolon ")),
        "官方 `scanner.go:154`：显式 `;` 的 `lit` 是 `semicolon`；got {msgs:?}"
    );
}

/// ⚠️ 同一个分支的**另一个来源**：换行补出来的分号 ⇒ `newline`（不是 `semicolon`）。
///
/// `func f() (int\n; int)` —— `int` 之后的换行被词法层补成 `_Semi`。
///
/// 两条一起才钉住「按来源区分」这件事 —— 只写上一条时，一个「`_Semi` 无条件
/// `semicolon`」的实现同样能过。
#[test]
fn t_a_66_semi_的_tok_换行补是_newline() {
    let msgs = syntax_msgs("package p\nfunc f() (int\n; int) { return 0 }\n");
    assert!(
        msgs.iter()
            .any(|m| m.starts_with("syntax error: unexpected newline ")),
        "官方 `scanner.go:121`：换行补的分号 `lit` 是 `newline`；got {msgs:?}"
    );
}

/// ⚠️ **反向验证**：`_Semi` 的 `tok` **既不是** `";"`，**也不**是 `;` 字符。
#[test]
fn t_a_66_semi_的_tok_不是_分号字符() {
    for src in [
        "package p\nfunc f() (int; int) { return 0 }\n",
        "package p\nfunc f() (int\n; int) { return 0 }\n",
        "package p\nvar x = []int{1; 2}\n",
        "package p\nfunc f() { var x map[int; ]int }\n",
    ] {
        for m in syntax_msgs(src) {
            assert!(
                !m.contains("unexpected ;"),
                "官方 `scanner.go` 给 `_Semi` 的 `lit` 是 `newline`/`semicolon`/`EOF`；got {m:?}"
            );
        }
    }
}

/// `_Name` 的 `tok` = `"name "` + **字面原文**（不是类别名 `name`）。
///
/// # 为什么这条**逐字**断言，而**不**另写一条「反向验证」
///
/// 反向验证（变异「丢掉字面原文」）时我连写三条**都**失败了：
///
/// 1. 「**如果**以 `unexpected name ` 开头（**带空格**），**就**断言它不等于裸
///    `name`」—— 变异后是 `unexpected name, …`（**逗号**）⇒ 前置条件不成立
///    ⇒ 整段被跳过 ⇒ **恒真**。
/// 2. 改成 `!contains("unexpected name,")` —— 变异落在 `at ` 分支时是
///    `unexpected name at end of statement`（**空格**）⇒ **漏**。
/// 3. 改成「后面不是 `,` / ` ` / 结束」—— 正确消息是 `unexpected name int …`
///    （空格后跟着字面 `int`）⇒ **误命中**，红在**基线**上。
///
/// 根因：`<tok>`（`name int`）与它后面的消息**之间没有可判别的界** ——
/// 字面原文本身也是空格分隔的 ⇒ 任何「按分隔符切 `<tok>`」的写法都会
/// 把字面切掉、进而误判。要断这件事只能**逐字比对** ⇒ 那正是本条做的事。
///
/// ⇒ **纪律：一条「反向验证」用例写不出判据时，正确做法是**删掉它**并写下
/// 「为什么写不出」，而不是硬凑一条恒真断言。**
#[test]
fn t_a_66_name_的_tok_带字面原文() {
    let msgs = syntax_msgs("package p\nfunc f() {\n\tvar 1x int\n}\n");
    assert!(
        msgs.iter().any(|m| m.contains("unexpected name int")),
        "官方 `:279`：`\"name \" + p.lit` ⇒ 必须带**这个标识符的字面**；got {msgs:?}"
    );
}

/// `_Literal` 的 `tok` = `"literal "` + **字面原文**（含引号）。
///
/// ⚠️ 同上的结论：反向验证那条**写不出判据**（`<tok>` 与后续消息无界）⇒ 已删，
/// 逐字断言本身就是判据。见 [`t_a_66_name_的_tok_带字面原文`] 的文档。
#[test]
fn t_a_66_literal_的_tok_带字面原文() {
    let msgs = syntax_msgs("package p\nfunc f() {\n\tvar x <- 42\n}\n");
    assert!(
        msgs.iter().any(|m| m.contains("unexpected literal 42")),
        "官方 `:283`：`\"literal \" + p.lit`；got {msgs:?}"
    );
}

/// `_IncOp` 的 `tok` = `op.String()`（**不加** `"="`）—— 与 `_AssignOp` 相反。
///
/// 官方 `:287-289`：
///
/// ```text
/// _AssignOp → p.op.String() + "="
/// _IncOp    → p.op.String()          ← 不加
/// ```
///
/// 这两条只差一个 `=`，写反了消息仍读得通。
///
/// # ⚠️ 一个反直觉但**正确**的输出
///
/// ⇒ 消息是 `unexpected + at end of statement` —— **一个 `+`**。
/// 官方 `operator_string.go` 印证：`Operator` 里**没有** `IncDec` 变体，
/// `++` 走 `OpIncDec` + `op = Add`（官方 `scanner.go` 的 `switchIncDec`）。
#[test]
fn t_a_66_inc_op_的_tok_是_裸_op_不加等号() {
    let msgs = syntax_msgs("package p\nfunc f() {\n\tx ++ ++ 1\n}\n");
    assert!(
        msgs.iter()
            .any(|m| m.starts_with("syntax error: unexpected + ")),
        "官方 `:289` 的 `_IncOp` 分支不加 `\"=\"`；而 `++` 的 `op` 是 `Add` ⇒ 拼出**一个** `+`；got {msgs:?}"
    );
}

/// ⚠️ **反向验证**：`_IncOp` 的 `tok` 既**不带** `=`（那是 `_AssignOp`），
/// **也不带第二个 `+`**（`++` 的 `op` 只有语义、没有完整拼写）。
///
/// 这条与上一条同输入、断言相反的结论：只写上一条时，
/// 一个「给 `_IncOp` 补全成 `++`」的实现同样能过。
#[test]
fn t_a_66_inc_op_的_tok_既不带等号也不带第二个加号() {
    for m in syntax_msgs("package p\nfunc f() {\n\tx ++ ++ 1\n}\n") {
        if m.starts_with("syntax error: unexpected ") {
            let tok = m.split("unexpected ").nth(1).unwrap();
            let tok = tok.split([' ', ',']).next().unwrap_or("");
            assert_ne!(tok, "++", "`++` 的 `op` 是 `Add` ⇒ 拼出 `+`；got {m:?}");
            assert_ne!(tok, "++=", "官方 `:289` 不加 `\"=\"`；got {m:?}");
        }
    }
}

/// `_AssignOp` 的 `tok` = `op.String()` + `"="` —— 与上一条相反。
///
/// `x += += 1` 里第二个 `+=` 作为**当前词元**走 `_AssignOp` 分支 ⇒ `+=`。
#[test]
fn t_a_66_assign_op_的_tok_加等号() {
    let msgs = syntax_msgs("package p\nfunc f() {\n\tx += += 1\n}\n");
    assert!(
        msgs.iter()
            .any(|m| m.starts_with("syntax error: unexpected +=,")),
        "官方 `:287`：`_AssignOp` ⇒ `op.String() + \"=\"`（`+=`）；got {msgs:?}"
    );
}

/// ⚠️ **反向验证**：`_AssignOp` 的 `tok` **不带第二个 `=`**（`+==` / `+= =` 都不对）。
#[test]
fn t_a_66_assign_op_的_tok_只有一个等号() {
    for m in syntax_msgs("package p\nfunc f() {\n\tx += += 1\n}\n") {
        if m.starts_with("syntax error: unexpected ") {
            let tok = m.split("unexpected ").nth(1).unwrap();
            let tok = tok.split([' ', ',']).next().unwrap_or("");
            assert_ne!(tok, "+==", "只补**一个** `\"=\"`；got {m:?}");
            assert_ne!(tok, "+", "补了 `\"=\"` 才对；got {m:?}");
        }
    }
}

/// `_Operator` 的 `tok` = `op.String()`，**不加** `=`。
///
/// `x := && 1` 里 `&&` 作为**当前词元**走 `_Operator` 分支（官方 `:285`）。
#[test]
fn t_a_66_普通运算符不加等号() {
    let msgs = syntax_msgs("package p\nfunc f() {\n\tx := && 1\n}\n");
    assert!(
        msgs.iter()
            .any(|m| m.starts_with("syntax error: unexpected &&,")),
        "官方 `:285`：`_Operator` ⇒ `p.op.String()`（裸 `&&`）；got {msgs:?}"
    );
}

/// ⚠️ **反向验证**：`_Operator` 的 `tok` 不是 `&&=`。
///
/// 这条与上一条同输入、断言相反的结论。
#[test]
fn t_a_66_普通运算符_不加等号_反向() {
    for m in syntax_msgs("package p\nfunc f() {\n\tx := && 1\n}\n") {
        if m.starts_with("syntax error: unexpected ") {
            let tok = m.split("unexpected ").nth(1).unwrap();
            let tok = tok.split([' ', ',']).next().unwrap_or("");
            assert_ne!(
                tok, "&&=",
                "官方 `:285` 的 `_Operator` 分支不加 `\"=\"`；got {m:?}"
            );
        }
    }
}

// ══════════════════════════════════════════════════════════════════════
// T-A-66④：`DiagCode` 冻结（`M2-design.md` §7.2）
// ══════════════════════════════════════════════════════════════════════

/// **`SYN-` 前缀的代号必须**逐条覆盖官方 `branches.go` 的报错点。
///
/// 官方 `branches.go` 共 **12 处** `ls.errf` 调用（`grep -n "ls.errf" | wc -l`），
/// 收敛成 **8 个**语义类别：
///
/// | 官方消息（行号） | 代号 |
/// |---|---|
/// | `break is not in a loop, switch, or select`（`:218`） | `SYN-BRANCH-BREAK` |
/// | `invalid break label %s`（`:261`） | `SYN-BRANCH-BREAK` |
/// | `continue is not in a loop`（`:224`） | `SYN-BRANCH-CONTINUE` |
/// | `invalid continue label %s`（`:274`） | `SYN-BRANCH-CONTINUE` |
/// | `fallthrough statement out of place` / `cannot fallthrough in type switch` / `cannot fallthrough final case in switch`（`:240` 那一处 `errf`，`msg` 三选一） | `SYN-BRANCH-FALLTHROUGH` |
/// | `label %s already defined at %s`（`:78`） | `SYN-LABEL-DUPLICATE` |
/// | `label %s defined and not used`（`:42`） | `SYN-LABEL-UNUSED` |
/// | `label %s not defined`（`:34`）/ `break label not defined: %s`（`:264`）/ `continue label not defined: %s`（`:277`） | `SYN-LABEL-UNDEFINED` |
/// | `goto %s jumps over declaration of %s at %s`（`:191`） | `SYN-GOTO-JUMPS-OVER-DECL` |
/// | `goto %s jumps into block starting at %s`（`:32`） | `SYN-GOTO-INTO-BLOCK` |
///
/// ⇒ **8 个**新代号。⚠️ 「12 处调用 / 8 个类别」这个差别是**有意的**：
/// `fallthrough` 的三条文案走**同一次** `errf`（`msg` 是变量），
/// 而 `label not defined` 的三条是**同一类**缺陷（标签查不到）。
#[test]
fn t_a_66_diagcode_冻结() {
    let mut r = parse_file(b"package p\nfunc f() {\n\tbreak\n}\n", "t.go");
    let codes = codes_of(&mut r);
    assert_eq!(
        codes,
        vec![DiagCode::SynBranchBreak],
        "`break` 不在循环里 ⇒ 官方 `branches.go:218`；代号 `SYN-BRANCH-BREAK`"
    );
}

/// `DiagCode::ALL` 的**长度**要跟着新代号一起涨 —— 让「漏了一个代号」有地方可查。
#[test]
fn t_a_66_diagcode_总数已更新() {
    // M1 的 23 个 `LEX-` + M2 的 12 个 `SYN-` + M3 的 13 个 `TYP-`
    // = 3 个早先的（SynExpectedSemicolon / SynInvalidLineDirective / SynUnexpectedToken）
    // + T-AP16 新增 8 个（branches 的 6 类 + goto 的 2 类）
    // + T-AP20 新增 1 个（SynNestingTooDeep，H 判据：官方无此行为）
    // + M3 新增 7 个（全部与官方 `go/types` 的 Code 一一对应，**刻意不拆细**：
    //   TypDivByZero / TypInvalidConstVal / TypInvalidConversion /
    //   TypInvalidShiftCount / TypInvalidShiftOperand / TypNumericOverflow /
    //   TypTruncatedFloat）
    // + M3/Task 17 新增 1 个（TypUnsupportedConstraintIdentity —— 这条**没有**
    //   官方对应物：官方判得了约束接口同一性，M3 形似显式拒绝，见 D-M3-13；
    //   该码由 M4/Task 10 升级 `identical` 后**移除** —— 见本函数末尾的净额）
    // + M3/Task 22 新增 2 个（TypInvalidRecursiveType 对官方 `InvalidRecursiveType`；
    //   TypNestingTooDeep 是 **H 判据**，与 SynNestingTooDeep 同理）
    // + M3/Task 21 新增 3 个（版本门控：FeatureRequiresGoVersion 对官方
    //   `UnsupportedFeature`；File/PackageRequiresNewerGo 拆自官方同一个 `TooNew`）。
    // + M3/Task 23 新增 1 个（TypMismatchedPkgName = 官方 `MismatchedPkgName`，
    //   同包多文件包名不一致）
    // + M3/Task 24 新增 4 个（TypInvalidConstInit = 官方 `InvalidConstInit`；
    //   TypInvalidDeclCycle = 官方 `InvalidDeclCycle`；TypInvalidInitCycle =
    //   官方 `InvalidInitCycle`；TypWrongAssignCount = 官方 `WrongAssignCount`）
    // + M3/Task 25 新增 3 个（TypDuplicateDecl = 官方 `DuplicateDecl`；
    //   TypUndeclaredName = `UndeclaredName`；TypRepeatedDecl = `RepeatedDecl`）
    // + M3/Task 26 新增 7 个（TypBlankPkgName / TypDuplicateLabel / TypInvalidBlank /
    //   TypJumpIntoBlock / TypMisplacedLabel / TypUndeclaredLabel / TypUnusedLabel）
    // + M3/Task 27 新增 3 个（TypBrokenImport = 官方 `BrokenImport`；
    //   TypImportCycle / TypUnsupportedImportForm 官方**无**代号，我方命名）
    //   ⇒ 共 66。
    // + M4/Task 2 新增 1 个（ChkUnimplemented —— `CHK-` 前缀首码，rgoc-check
    //   骨架期的占位诊断；官方无对应物）⇒ 共 67。
    // + M4/Task 6 新增 1 个（TypUnsupportedAssignability —— 可赋值性窄面的稳定
    //   拒绝码；官方无对应物）⇒ 共 68。
    // + M4/Task 10 **移除** 1 个（TypUnsupportedConstraintIdentity —— `identical`
    //   升级为类型集版、`U-M3-05` 解除）⇒ 共 67。
    assert_eq!(
        DiagCode::ALL.len(),
        67,
        "M1 的 23 个 + M2 早先 3 个 + T-AP16 的 8 个 + T-AP20 的 1 个（NESTING）+ M3 的 31 个（TYP-）+ M4 的 2 个（CHK- 与 TYP- 各 1）再 -1（Task 10 移除 TypUnsupportedConstraintIdentity）"
    );
}
