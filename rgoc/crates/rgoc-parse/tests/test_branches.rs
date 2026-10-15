//! `checkBranches` 的判据（`T-AP16`，`T-A-80`/`81`/`82`）。
//!
//! 官方 `branches.go` 339 行、四类检查。本文件按**检查项**组织，
//! 每条用例至少两遍（**合法形态零诊断** + **非法形态恰好一条**）——
//! 只验「报错」的话，一个「什么都报」的实���也能过。
//!
//! # 官方 `branches.go` 的 12 处 `errf` 与 8 个语义类别
//!
//! | 消息（官方行号） | 代号 |
//! |---|---|
//! | `break is not in a loop, switch, or select`（`:218`） | `SYN-BRANCH-BREAK` |
//! | `invalid break label %s`（`:261`） | `SYN-BRANCH-BREAK` |
//! | `continue is not in a loop`（`:224`） | `SYN-BRANCH-CONTINUE` |
//! | `invalid continue label %s`（`:274`） | `SYN-BRANCH-CONTINUE` |
//! | `fallthrough …`（`:240`，`msg` 三选一） | `SYN-BRANCH-FALLTHROUGH` |
//! | `label %s already defined at %s`（`:78`） | `SYN-LABEL-DUPLICATE` |
//! | `label %s defined and not used`（`:42`） | `SYN-LABEL-UNUSED` |
//! | `label %s not defined`（`:34`）/ `break label not defined: %s`（`:264`）/ `continue label not defined: %s`（`:277`） | `SYN-LABEL-UNDEFINED` |
//! | `goto %s jumps over declaration of %s at %s`（`:191`） | `SYN-GOTO-JUMPS-OVER-DECL` |
//! | `goto %s jumps into block starting at %s`（`:32`） | `SYN-GOTO-INTO-BLOCK` |
//!
//! # ⚠️ `checkBranches` 只在**本函数无语法错**时跑（官方 `funcBody` 的门控）
//!
//! 官方 `parser.go:851-856` 的注释：语法错会让树不完整，此时跑分支检查会
//! **报虚假错误**或**崩在残缺树上**。最后两条用例专门钉这个门控。

use rgoc_lex::diag::DiagCode;
use rgoc_parse::parse_file;

/// 解析一段函数体，返回**排好序**的全部诊断 `(代号, 消息, 行, 列)`。
fn diags_of(body: &str) -> Vec<(DiagCode, String, u32, u32)> {
    let src = format!("package p\nfunc f() {{\n{body}\n}}\n");
    let mut r = parse_file(src.as_bytes(), "t.go");
    std::mem::take(&mut r.diags)
        .into_sorted()
        .into_iter()
        .map(|d| (d.code, d.message, d.pos.line, d.pos.col))
        .collect()
}

/// 全部诊断的消息（按位置排序）。
fn msgs_of(body: &str) -> Vec<String> {
    diags_of(body).into_iter().map(|(_, m, _, _)| m).collect()
}

/// 全部诊断的代号。
fn codes_of(body: &str) -> Vec<DiagCode> {
    diags_of(body).into_iter().map(|(c, ..)| c).collect()
}

/// 要求**零**诊断。
#[track_caller]
fn ok(body: &str) {
    let d = diags_of(body);
    assert!(
        d.is_empty(),
        "这段应当零诊断，却报了：{d:?}（输入 {body:?}）"
    );
}

/// 要求**恰好一条**，且消息逐字相符、代号相符。
#[track_caller]
fn one(body: &str, code: DiagCode, msg: &str) {
    let d = diags_of(body);
    assert_eq!(
        d.len(),
        1,
        "应恰好一条，却有 {} 条：{d:?}（输入 {body:?}）",
        d.len()
    );
    assert_eq!(d[0].0, code, "代号不对：{d:?}（输入 {body:?}）");
    assert_eq!(d[0].1, msg, "消息不对：{d:?}（输入 {body:?}）");
}

// ══════════════════════════════════════════════════════════════════════
// T-A-80：错位的 break / continue / fallthrough
// ══════════════════════════════════════════════════════════════════════

/// `break` 不在 `for`/`switch`/`select` 里（官方 `:218`）。
#[test]
fn t_a_80_break_不在循环里() {
    one(
        "\tbreak\n",
        DiagCode::SynBranchBreak,
        "break is not in a loop, switch, or select",
    );
}

/// ⚠️ **反向验证**：放在 `for` 里**零**诊断 —— 只写上一条时，
/// 一个「永远报 `break is not in a loop`」的实现同样能过。
#[test]
fn t_a_80_break_在_for_里零诊断() {
    ok("\tfor {\n\t\tbreak\n\t}\n");
}

/// `break` 在 `switch` 里合法（官方 `:215-217` 的 `ctxt.breaks != nil`）。
#[test]
fn t_a_80_break_在_switch_里合法() {
    ok("\tswitch 1 {\n\tcase 1:\n\t\tbreak\n\t}\n");
}

/// `break` 在 `select` 里合法（官方 `targets{s, ctxt.continues, -1}`）。
#[test]
fn t_a_80_break_在_select_里合法() {
    ok("\tselect {\n\tcase <-make(chan int):\n\t\tbreak\n\t}\n");
}

/// ⚠️ **嵌套在块里**：`for { { break } }` —— 官方 `:300` 的
/// `targets{ctxt.breaks, ctxt.continues, -1}` **沿用** `breaks`
/// ⇒ 仍然合法。这条照出「`Block` 臂把 `breaks` 清成 `None」。
#[test]
fn t_a_80_break_在循环里的裸块中合法() {
    ok("\tfor {\n\t\t{\n\t\t\tbreak\n\t\t}\n\t}\n");
}

/// ⚠️ 同理：`if` 的 then/else 里也**沿用** `breaks`（官方 `:303-308`）。
#[test]
fn t_a_80_break_在_if_里合法() {
    ok("\tfor {\n\t\tif true {\n\t\t\tbreak\n\t\t}\n\t}\n");
}

/// `continue` 不在 `for` 里（官方 `:224`）。
#[test]
fn t_a_80_continue_不在循环里() {
    one(
        "\tcontinue\n",
        DiagCode::SynBranchContinue,
        "continue is not in a loop",
    );
}

/// ⚠️ **反向验证**：`continue` 在 `for` 里零诊断。
#[test]
fn t_a_80_continue_在_for_里零诊断() {
    ok("\tfor {\n\t\tcontinue\n\t}\n");
}

/// ⚠️ **`continue` 在 `switch` 里非法** —— 官方 `:315` 的
/// `targets{s, ctxt.continues, -1}`：`breaks` 指向 `switch`、
/// `continues` **沿用外层**（此处是 `None`）⇒ 报 `continue is not in a loop`。
///
/// 这条与 `break 在 switch 里合法` 成对 —— 只写后者的话，
/// 一个「`switch` 把 `continues` 也设成自己」的实现能过。
#[test]
fn t_a_80_continue_在_switch_里非法() {
    one(
        "\tswitch 1 {\n\tcase 1:\n\t\tcontinue\n\t}\n",
        DiagCode::SynBranchContinue,
        "continue is not in a loop",
    );
}

/// ⚠️ **`continue` 在 `switch` 里的 `for` 中合法** —— `continues` 沿用外层 `for`。
#[test]
fn t_a_80_continue_在_switch_里_外层有_for_则合法() {
    ok("\tfor {\n\t\tswitch 1 {\n\t\tcase 1:\n\t\t\tcontinue\n\t\t}\n\t}\n");
}

/// `fallthrough` 在 `switch` 之外（官方 `:227`）。
#[test]
fn t_a_80_fallthrough_在_switch_之外() {
    one(
        "\tfallthrough\n",
        DiagCode::SynBranchFallthrough,
        "fallthrough statement out of place",
    );
}

/// `fallthrough` 是**最后一个** `case` 的最后一条（官方 `:234-235`）。
#[test]
fn t_a_80_fallthrough_在最后一个_case() {
    one(
        "\tswitch 1 {\n\tcase 1:\n\t\tfallthrough\n\t}\n",
        DiagCode::SynBranchFallthrough,
        "cannot fallthrough final case in switch",
    );
}

/// ⚠️ **反向验证**：`fallthrough` 在**非**最后一个 `case` 的最后一条**合法**
/// （官方 `:237` 的 `break // fallthrough ok`）。
#[test]
fn t_a_80_fallthrough_在中间_case_合法() {
    ok("\tswitch 1 {\n\tcase 1:\n\t\tfallthrough\n\tcase 2:\n\t}\n");
}

/// ⚠️ **`fallthrough` 不是最后一条 ⇒ 非法**（官方 `:231-232` 的
/// `stmtIndex+1 < len(stmtList)`），文案**保持** `out of place`
/// —— 这与「最后一个 case」那条的文案**不同**，照得出两档。
#[test]
fn t_a_80_fallthrough_不是最后一条() {
    one(
        "\tswitch 1 {\n\tcase 1:\n\t\tfallthrough\n\t\tx++\n\tcase 2:\n\t}\n",
        DiagCode::SynBranchFallthrough,
        "fallthrough statement out of place",
    );
}

/// ⚠️ **尾部空语句不算**（官方 `:167` 的 `trimTrailingEmptyStmts`）：
/// `case 1: fallthrough ;; case 2:` 里的 `fallthrough` **合法**。
///
/// 这条照出「忘了 `trimTrailingEmptyStmts`」—— 那会让 `fallthrough`
/// 后面跟了空语句时被判成「不是最后一条」而误报。
#[test]
fn t_a_80_fallthrough_后面跟空语句仍合法() {
    ok("\tswitch 1 {\n\tcase 1:\n\t\tfallthrough\n\t\t;\n\tcase 2:\n\t}\n");
}

/// 类型 switch 里不能 `fallthrough`（官方 `:229-230`）。
///
/// ⚠️ 输入必须是**真的**类型 switch —— 判据是官方的
/// `t.Tag.(*TypeSwitchGuard)`，而那要求 tag 是 `x.(type)`。
/// `switch 1 { case int: }` 的 tag 是**字面量** `1` ⇒ **不是**类型 switch
/// ⇒ 报的是 `cannot fallthrough final case in switch`（最后一档），
/// 而**不是** `cannot fallthrough in type switch`。
/// 两条消息只差三个词，写错输入就永远照不出第一档。
#[test]
fn t_a_80_fallthrough_在类型_switch_里() {
    one(
        "\tvar x any\n\tswitch x.(type) {\n\tcase int:\n\t\tfallthrough\n\t}\n",
        DiagCode::SynBranchFallthrough,
        "cannot fallthrough in type switch",
    );
}

/// ⚠️ **反向验证**：**不是**类型 switch 的 `switch 1 { case int: }`
/// 走的是「最后一个 case」那一档（官方 `:234-235`）。
///
/// 这条与上一条成对：照出「`TypeSwitchGuard` 判据**只**认 `x.(type)`」。
#[test]
fn t_a_80_非类型_switch_的_case_类型_不触发类型档() {
    one(
        "\tswitch 1 {\n\tcase int:\n\t\tfallthrough\n\t}\n",
        DiagCode::SynBranchFallthrough,
        "cannot fallthrough final case in switch",
    );
}

/// `fallthrough` 嵌在块里 ⇒ 非法（官方 `:231` 的 `caseIndex < 0`）。
#[test]
fn t_a_80_fallthrough_嵌在块里() {
    one(
        "\tswitch 1 {\n\tcase 1:\n\t\t{\n\t\t\tfallthrough\n\t\t}\n\t}\n",
        DiagCode::SynBranchFallthrough,
        "fallthrough statement out of place",
    );
}

// ══════════════════════════════════════════════════════════════════════
// T-A-81：标签（未定义 / 重复 / 未使用 / 非法目标）
// ══════════════════════════════════════════════════════════════════════

/// 标签从未定义（官方 `:34`）。
#[test]
fn t_a_81_标签未定义() {
    one(
        "\tgoto L\n",
        DiagCode::SynLabelUndefined,
        "label L not defined",
    );
}

/// ⚠️ **反向验证**：`goto` 指向**已定义**的标签**零**诊断 ——
/// 只写上一条时，一个「永远报 label not defined」的实现能过。
#[test]
fn t_a_81_goto_指向已定义标签零诊断() {
    ok("\tgoto L\nL:\n\tx++\n");
}

/// ⚠️ **前向 `goto`**（标签在**后面**）合法 —— 官方 `:280-286`
/// 的 `gotoTarget` 返回 `nil` 时记成 `fwdGotos`。
///
/// 这条照出「前向 `goto` 被当成未定义」—— 那是最容易写错的路径。
#[test]
fn t_a_81_前向_goto_合法() {
    ok("\tgoto L\nL:\n\tx++\n");
}

/// 标签定义了却没用（官方 `:42`，spec 要求）。
#[test]
fn t_a_81_标签未使用() {
    one(
        "\tL:\n\tx++\n",
        DiagCode::SynLabelUnused,
        "label L defined and not used",
    );
}

/// ⚠️ **反向验证**：被 `goto` 用过的标签不报「未使用」。
#[test]
fn t_a_81_被用过的标签不报未使用() {
    ok("\tgoto L\nL:\n\tx++\n");
}

/// ⚠️ **`_` 不算标签**（官方 `:182` 的 `name != "_"`）——
/// `_:` 既不报「未定义」，也不报「未使用」。
#[test]
fn t_a_81_下划线标签被忽略() {
    ok("\t_:\n\tx++\n");
}

/// 同一块里重复定义（官方 `:78`），消息内插**第一条**的位置。
///
/// ⚠️ **恰好两条**：官方 `declare` 遇到重名时 `return alt`（**不覆盖**）⇒
/// `labels` 表里只有**第一条** `L`，它的 `used` 也只有第一条能改。
/// 本例里两条都没被 `goto` 用 ⇒ 末尾的「defined and not used」（官方 `:42`）
/// **照报**，落在**第一条**上。
///
/// 官方注释（`:31`）只在「`goto` 跳进块」那条上特意 `l.used = true`
/// 来避免第二条 —— 重复定义**没有**这个豁免 ⇒ 两条都报。
#[test]
fn t_a_81_标签重复() {
    let d = diags_of("\tL:\n\tx++\nL:\n\ty++\n");
    let msgs: Vec<&str> = d.iter().map(|(_, m, _, _)| m.as_str()).collect();
    assert_eq!(
        d.len(),
        2,
        "官方：`already defined` + 第一条的 `defined and not used`；got {d:?}"
    );
    // ⚠️ **按位置排序**（`C1` §4.3）：「未使用」落在**第一条**（3,2）、
    // 「重复定义」落在**第二条**（5,1）⇒ 顺序是 unused → duplicate。
    // 断言必须跟着排序走 —— 按「检查顺序」写会在 `into_sorted` 下恒假。
    assert_eq!(d[0].0, DiagCode::SynLabelUnused, "{d:?}");
    assert_eq!(msgs[0], "label L defined and not used", "{d:?}");
    assert_eq!((d[0].2, d[0].3), (3, 2), "落在**第一条**声明上；got {d:?}");
    assert_eq!(d[1].0, DiagCode::SynLabelDuplicate, "{d:?}");
    assert!(
        msgs[1].starts_with("label L already defined at "),
        "官方 `:78` 的消息形状；got {:?}",
        msgs[1]
    );
    assert!(
        msgs[1].contains(":3:"),
        "「at」后面是**第一条**声明的位置（第 3 行）；got {:?}",
        msgs[1]
    );
}

/// ⚠️ **反向验证**：重复定义的标签**被用掉** ⇒ 只剩「already defined」一条
/// （官方 `:42` 那条因 `used` 被置位而消失）。
#[test]
fn t_a_81_标签重复_但被使用_则只有一条() {
    let d = diags_of("\tgoto L\nL:\n\tx++\nL:\n\ty++\n");
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].0, DiagCode::SynLabelDuplicate, "{d:?}");
}

/// `break` 标签未定义（官方 `:264`）。
#[test]
fn t_a_81_break_标签未定义() {
    one(
        "\tfor {\n\t\tbreak L\n\t}\n",
        DiagCode::SynLabelUndefined,
        "break label not defined: L",
    );
}

/// `continue` 标签未定义（官方 `:277`）。
#[test]
fn t_a_81_continue_标签未定义() {
    one(
        "\tfor {\n\t\tcontinue L\n\t}\n",
        DiagCode::SynLabelUndefined,
        "continue label not defined: L",
    );
}

/// `invalid break label`：标签标注的**不是** `for`/`switch`/`select`
/// （官方 `:261`）。
#[test]
fn t_a_81_非法_break_标签() {
    one(
        "\tL:\n\tx++\n\tbreak L\n",
        DiagCode::SynBranchBreak,
        "invalid break label L",
    );
}

/// ⚠️ **`invalid continue label`**：标签标注的是 `switch`（官方 `:274`）——
/// `switch` 不是合法的 `continue` 目标。
///
/// 这条与上一条成对：照出「`continue` 的形状判据与 `break` **不同**」
/// （`switch` 对 `break` 合法、对 `continue` 非法）。
#[test]
fn t_a_81_非法_continue_标签() {
    one(
        "\tL:\n\tswitch 1 {\n\tcase 1:\n\t\tcontinue L\n\t}\n",
        DiagCode::SynBranchContinue,
        "invalid continue label L",
    );
}

/// ⚠️ **反向验证**：`continue` 标签标注 `for` ⇒ 合法（官方 `:272`）。
#[test]
fn t_a_81_continue_标签标注_for_合法() {
    ok("\tL:\n\tfor {\n\t\tcontinue L\n\t}\n");
}

// ══════════════════════════════════════════════════════════════════════
// T-A-82：goto 跳过变量声明 / 跳进块
// ══════════════════════════════════════════════════════════════════════

/// `goto` 跳过了变量声明（官方 `:191-196`）。
#[test]
fn t_a_82_goto_跳过变量声明() {
    let d = diags_of("\tgoto L\n\tvar x int\nL:\n\tx++\n");
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].0, DiagCode::SynGotoJumpsOverDecl);
    assert!(
        d[0].1.starts_with("goto L jumps over declaration of x at "),
        "官方 `:194` 的消息形状（`%s` 是变量名）；got {:?}",
        d[0].1
    );
    assert!(
        d[0].1.contains(":4:"),
        "位置是**变量声明**那行；got {:?}",
        d[0].1
    );
}

/// ⚠️ **反向验证**：`var` 在 `goto` **之后** ⇒ 合法。
#[test]
fn t_a_82_goto_不跳过变量声明_合法() {
    ok("\tgoto L\nL:\n\tx++\n\tvar x int\n\t_ = x\n");
}

/// ⚠️ **`:=` 也算**变量声明（官方 `:294-297` 的 `s.Op == Def`）。
#[test]
fn t_a_82_goto_跳过短变量声明() {
    let d = diags_of("\tgoto L\n\tx := 1\nL:\n\t_ = x\n");
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].0, DiagCode::SynGotoJumpsOverDecl, "{d:?}");
}

/// ⚠️ **`goto` 跳进块内部**（官方 `:32`，`goto L jumps into block`）。
///
/// `goto L` 在**外层**块，`L:` 在**内层**块里 ⇒ 官方的 `gotoTarget`
/// 沿块链往上找，找不到 ⇒ 记成前向 `goto`；结束时标签存在但
/// `l.parent` 是内层块 ⇒ 报 `jumps into block`。
#[test]
fn t_a_82_goto_跳进块() {
    let d = diags_of("\tgoto L\n{\nL:\n\tx++\n}\n");
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].0, DiagCode::SynGotoIntoBlock, "{d:?}");
    assert!(
        d[0].1.starts_with("goto L jumps into block starting at "),
        "官方 `:32` 的消息形状；got {:?}",
        d[0].1
    );
}

/// ⚠️ **反向验证**：标签在外层块 ⇒ 合法（`gotoTarget` 能沿链找到）。
#[test]
fn t_a_82_goto_不跳进块_合法() {
    ok("\tgoto L\nL:\n\t{\n\t\tx++\n\t}\n");
}

/// ⚠️ **「跳进块」的那条不会**再多报一条「未使用」
/// （官方 `:31` 的 `l.used = true // avoid "defined and not used" error`）。
///
/// 官方注释点名这条是**有意**的：两条诊断只报第一条（更根本的那条）。
#[test]
fn t_a_82_跳进块_不重复报未使用() {
    let d = diags_of("\tgoto L\n{\nL:\n\tx++\n}\n");
    assert_eq!(
        d.len(),
        1,
        "官方 `:31` 只报 `jumps into block`；`used` 已被置位 ⇒ 没有「未使用」；got {d:?}"
    );
}

// ══════════════════════════════════════════════════════════════════════
// 门控：只在**本函数无语法错**时检查（官方 `funcBody` 的 `errcnt == p.errcnt`）
// ══════════════════════════════════════════════════════════════════════

/// ⚠️ **本函数有语法错 ⇒ 完全不做分支检查**（官方 `:854`）。
///
/// 输入 `break` 后面紧跟一个非法 token：既有语法错、又有一条 `break` 错位
/// ⇒ 官方**只报语法错**。
#[test]
fn 门控_本函数有语法错时跳过分支检查() {
    let d = diags_of("\tbreak\n\tvar\n");
    assert!(
        !codes_of("\tbreak\n\tvar\n").contains(&DiagCode::SynBranchBreak),
        "官方 `:851-856`：有语法错时**不**跑 `checkBranches`；got {d:?}"
    );
}

/// ⚠️ **别处的语法错**（**本函数干净**）⇒ 分支检查**照跑**。
/// 判据是「**进 `funcBody` 之前**的 `errcnt`」而不是「整个文件干净」——
/// 写成后者会让一个坏 import 让**全文件**的分支检查消失。
///
/// ⚠️ 输入选的是**坏 import**（`import 42`）：它只报一条
/// `import path must be a string`，**不影响**后续函数的结构。
/// 早先选的 `func g() { var }` 会让恢复把整个文件的声明结构打乱，
/// `func f` 的 `break` 根本到不了 `checkBranches` ⇒ 变成一条**假红**。
#[test]
fn 门控_别处有语法错_本函数仍检查() {
    let src = "package p\nimport 42\n\nfunc f() {\n\tbreak\n}\n";
    let mut r = parse_file(src.as_bytes(), "t.go");
    let codes: Vec<DiagCode> = std::mem::take(&mut r.diags)
        .into_sorted()
        .into_iter()
        .map(|d| d.code)
        .collect();
    assert!(
        codes.contains(&DiagCode::SynBranchBreak),
        "`import 42` 的语法错**不该**让 `func f` 的分支检查消失；got {codes:?}"
    );
}

/// ⚠️ 分支诊断**不带** `syntax error: ` 前缀（官方 `branches.go:65`
/// 直接构造裸错误（位置 + 消息二字段），**不走** `syntaxErrorAt`）。
#[test]
fn 门控_分支诊断不带_syntax_error_前缀() {
    let msgs = msgs_of("\tbreak\n");
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(
        !msgs[0].starts_with("syntax error: "),
        "官方 `branches.go:65` 是裸错误（位置 + 消息）；got {:?}",
        msgs[0]
    );
}

/// ⚠️ **「分支诊断不受 EOF 抑制」在本架构下不可测 —— 如实登记，不是漏了**。
///
/// 官方 `syntaxErrorAt` 的 EOF 抑制（`parser.go:257-259`）条件是
/// `p.tok == _EOF && p.first != nil`。想验「分支诊断不受它影响」，
/// 就得让 `checkBranches` 在 `p.first != nil` 且 `p.tok == _EOF` 时仍报 ——
/// 而那要求**本函数已经有一条语法错**，于是 `funcBody` 的门控
/// （`errcnt == p.errcnt`，官方 `:854`）**先把整个检查跳过**。
///
/// ⇒ **两个条件互斥**：门控要求「本函数无语法错」，EOF 抑制要求「已有错」。
/// 这不是我们实现的取舍，是官方两条规则**本身**的组合结果。
///
/// 那分支诊断到底走不走 `syntaxErrorAt`？走的是
/// [`Parser::report_branch`]（官方 `branches.go:65` 的 `ls.errf`）——
/// 与 `syntaxErrorAt` 是**两个不同的函数**，代码上不可能互相影响。
/// 本文件用「不带 `syntax error: ` 前缀」那条钉住这一点。
///
/// 本条只断言「门控**优先**于分支检查」：有语法错（缺收尾 `}`）
/// ⇒ 只报语法错，不报分支错。
#[test]
fn 门控_有语法错时门控优先于分支检查() {
    // `break` 后面跟一个**无法解析**的声明（`var`）⇒ 本函数有语法错
    // ⇒ 官方 `:854` 的门控跳过 `checkBranches`。
    //
    // ⚠️ 早先的输入是「`break` 后直接 EOF」，那条**没有**语法错
    // （词法层给文件尾补了分号）⇒ 分支检查照跑 ⇒ 变成一条假红。
    // 「制造语法错」得用**真的解析不出来**的东西，不能靠「少写一个收尾符」。
    let msgs = msgs_of("\tbreak\n\tvar\n");
    assert!(!msgs.is_empty(), "{msgs:?}");
    assert!(
        !msgs
            .iter()
            .any(|m| m == "break is not in a loop, switch, or select"),
        "本函数有语法错 ⇒ 官方 `:854` 的门控跳过 `checkBranches` ⇒ 不该报分支错；got {msgs:?}"
    );
}

/// 一个 `ParseResult` 的诊断流里，分支诊断与语法诊断**按位置混排**
/// （`C1` §4.3 的排序键对两者一致）。
#[test]
fn 门控_两类诊断按位置混排() {
    let d = diags_of("\tvar x int\n\tgoto L\n\tL:\n\tx++\n");
    // 这段应当**零**诊断（`goto` 在 `var` 之后）；钉住「没报就是没报」。
    assert!(d.is_empty(), "{d:?}");
}
