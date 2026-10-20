//! `T-CK-36` range 静态面（M4/Task 26）：被 range 表达式的形态矩阵
//! （数组 / 切片 / 字符串 / map / chan / 整型 1.22 / func 1.23）、迭代变量的
//! 个数与类型、以及两条版本门控的落点。
//!
//! # 夹具与 oracle 证据
//!
//! 两个夹具与 `.superpowers/sdd/M4-plan/probe/task26-{reject,accept}.go`
//! **逐字节一致**。oracle 实测（`tools/types-oracle --check`）：
//!
//! - 拒绝夹具：**16 行**（`probe/task26-oracle-pin.log` 原样输出 —— 位+类别）；
//! - 接受夹具：**0 行**（假阳性守卫）。
//!
//! # 断言分三层
//!
//! ① 我方 `(行,列,码)` 与手写期望表逐条相同（码是**本仓**的划分）；
//! ② 我方渲染行（`C8`：位置 + 类别）与 oracle 行**逐字节**相同；
//! ③ 版本门控（`T-CK-64` 的 range 两档）：`range over int` 在 go1.21 被拒、
//!    `range over func` 在 go1.22 被拒，两次都落在**操作数最左 token**、
//!    类别都是 `version`（C8 适配表里 `requires go1\.` 最高优先）。
//!
//! # 为什么值类型是 `int32` 而不是 `rune`
//!
//! 官方 `rangeKeyVal` 对字符串返回 `universeRune`（名字叫 rune 的 int32 别名）；
//! 本仓 universe 把 `rune` 折进 `BasicId::Int32`，所以 `type_str` 写 `int32`。
//! 类型**同一性**不受影响（都是 `BasicId::Int32`）—— 只差消息拼写，
//! 而 `C8` 判据是「位置 + 类别」。
//!
//! # 已知的**上游**分歧（不在本任务修）
//!
//! `for a, b, c := range x`（三个迭代变量）官方**语法器**就拒（类目 `parse`），
//! 而本仓语法器允许三元素 `lhs`、由检查面报 `range` 类 `permits at most two`。
//! 位置与类别因此与 oracle 不同（实测 `35:12`：oracle `parse` / 我方 `range`）。
//! 根因在 `rgoc-parse` 的 `new_range_clause`（M2 面），已登记为开放缺口；
//! 拒绝夹具**不含**该形态，免得把上游分歧伪装成本任务的失败。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_lex::DiagCode;
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 拒绝夹具（与 `probe/task26-reject.go` 逐字节一致）。
const REJECT_SRC: &str = r#"// T-CK-36 range 静态形态 —— 拒绝夹具（Task 26）。
// 与 `rgoc/crates/rgoc-check/tests/range.rs` 的 `REJECT_SRC` 逐字节一致。
package p

var ch chan int
var sch chan<- int
var m map[string]int
var arr [3]int
var arrp *[3]int
var sl []int
var st string

var it1 func(func(int) bool)
var badArity func(func(int) bool, int)
var badNotFunc func(int)
var badYield func(func() int)

func overFloatConst() {
	for range 1.0 {
	}
}

func overBoolConst() {
	for range true {
	}
}

func overSendOnly() {
	for range sch {
	}
}

func chanTwoVars() {
	for k, v := range ch {
		_, _ = k, v
	}
}

func intTwoVars() {
	for i, j := range 10 {
		_, _ = i, j
	}
}

func mapBadTypes() {
	var k int
	var v string
	_ = k
	_ = v
	for k, v = range m {
	}
	_, _ = k, v
}

func iterVarFloat() {
	var f float64
	_ = f
	for f = range 10 {
	}
}

func iterVarBadType() {
	var s string
	_ = s
	for s = range sl {
	}
}

func iterVarOverflow() {
	var u uint8
	_ = u
	for u = range 256 {
	}
}

func noVarOverflow() {
	for range 1 << 63 {
	}
}

func defineOverflow() {
	for i := range 1 << 63 {
		_ = i
	}
}

func funcNotFunc() {
	for range badNotFunc {
	}
}

func funcBadArity() {
	for range badArity {
	}
}

func funcYieldNotBool() {
	for range badYield {
	}
}

func funcTwoVars() {
	for _, _ = range it1 {
	}
}
"#;

/// 接受夹具（与 `probe/task26-accept.go` 逐字节一致）。
const ACCEPT_SRC: &str = r#"// T-CK-36 range 静态形态 —— 接受夹具（Task 26）。
// 与 `rgoc/crates/rgoc-check/tests/range.rs` 的 `ACCEPT_SRC` 逐字节一致。
package p

type MyInt int32

var arr [3]int
var arrp *[3]int
var sl []int
var st string
var m map[string]int
var ch chan int
var rch <-chan int
var it1 func(func(int) bool)
var it2 func(func(string, int) bool)

func overArray() {
	for range arr {
	}
	for i := range arr {
		_ = i
	}
	for i, v := range arr {
		_, _ = i, v
	}
	for range arrp {
	}
}

func overSlice() {
	for range sl {
	}
	for i, v := range sl {
		_, _ = i, v
	}
}

func overString() {
	for range st {
	}
	for i, r := range st {
		_, _ = i, r
	}
	for i := range st {
		_ = i
	}
}

func overMap() {
	for range m {
	}
	for k, v := range m {
		_, _ = k, v
	}
	for k := range m {
		_ = k
	}
}

func overChan() {
	for range ch {
	}
	for v := range ch {
		_ = v
	}
	for range rch {
	}
	for v := range rch {
		_ = v
	}
}

func overInt() {
	for range 10 {
	}
	for i := range 10 {
		_ = i
	}
	var i int
	for i = range 10 {
	}
	_ = i
	for range 'x' {
	}
	for i := range 'x' {
		_ = i
	}
	var mi MyInt
	for mi = range MyInt(10) {
	}
	_ = mi
}

func overFunc() {
	for range it1 {
	}
	for v := range it1 {
		_ = v
	}
	for k, v := range it2 {
		_, _ = k, v
	}
}

func shadowing() {
	i := 5
	_ = i
	for i := range sl {
		_ = i
	}
}
"#;

/// oracle 实测渲染行（`--check` 原样；按位置稳定排序）。
const ORACLE_REJECT_ROWS: [&str; 16] = [
    "19:12\trange",
    "24:12\trange",
    "29:12\trange",
    "34:9\trange",
    "40:9\trange",
    "50:6\tassign",
    "50:9\tassign",
    "58:6\trange",
    "65:6\tassign",
    "72:16\tassign",
    "77:12\tassign",
    "82:17\tassign",
    "88:12\trange",
    "93:12\trange",
    "98:12\trange",
    "103:9\trange",
];

/// 我方 `(行,列,码)` 期望表 —— 码是本仓的划分，位置必须与 oracle 一致。
const EXPECT_REJECT: [(u32, u32, DiagCode); 16] = [
    (19, 12, DiagCode::ChkInvalidRangeExpr),
    (24, 12, DiagCode::ChkInvalidRangeExpr),
    (29, 12, DiagCode::ChkInvalidRangeExpr),
    (34, 9, DiagCode::ChkInvalidIterVar),
    (40, 9, DiagCode::ChkInvalidIterVar),
    (50, 6, DiagCode::ChkInvalidAssign),
    (50, 9, DiagCode::ChkInvalidAssign),
    (58, 6, DiagCode::ChkInvalidRangeExpr),
    (65, 6, DiagCode::ChkInvalidAssign),
    (72, 16, DiagCode::ChkInvalidAssign),
    (77, 12, DiagCode::ChkInvalidAssign),
    (82, 17, DiagCode::ChkInvalidAssign),
    (88, 12, DiagCode::ChkInvalidRangeExpr),
    (93, 12, DiagCode::ChkInvalidRangeExpr),
    (98, 12, DiagCode::ChkInvalidRangeExpr),
    (103, 9, DiagCode::ChkInvalidIterVar),
];

/// 装载一个内存单文件包（版本源缺省 = 不门控）。
fn load(src: &str) -> (TypeTable, Package, rgoc_lex::DiagBag) {
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_sources(&mut tt, &[("t.go", src.as_bytes())], None);
    (tt, pkg, bag)
}

/// 数据行（剥 `#` 注释行）。
fn rows(text: &str) -> Vec<&str> {
    text.lines().filter(|l| !l.starts_with('#')).collect()
}

/// 拒绝向量：`(行,列,码)` 逐条对账 + 渲染行与 oracle **逐字节**一致。
#[test]
fn t_ck_36_拒绝向量_码与位置逐条一致() {
    let (mut tt, pkg, bag) = load(REJECT_SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());

    // ① 码 + 位置。流序是「语句面」序（含 `range` 里的赋值），
    //    所以两侧都按 (行,列,码) 排序后再比 —— 比的是**集合**不是流序。
    let mut got: Vec<(u32, u32, DiagCode)> = checked
        .diags
        .iter()
        .map(|r| (r.diag.pos.line, r.diag.pos.col, r.diag.code))
        .collect();
    got.sort();
    let mut want = EXPECT_REJECT.to_vec();
    want.sort();
    assert_eq!(got.len(), want.len(), "诊断条数：got {got:?}");
    assert_eq!(got, want, "逐条 (行,列,码)");

    // ② 渲染行 == oracle 原样输出（位置 + 类别逐字节）。
    let rendered_text = checked.render("t.go");
    let rendered: Vec<&str> = rows(&rendered_text);
    assert_eq!(rendered, ORACLE_REJECT_ROWS.to_vec(), "渲染行 vs oracle");
}

/// 接受向量：整文件零诊断（假阳性守卫；oracle 实测同样 0 行）。
#[test]
fn t_ck_36_接受向量_零诊断() {
    let (mut tt, pkg, bag) = load(ACCEPT_SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert!(
        checked.diags.is_empty(),
        "接受夹具必须零诊断，got {:?}",
        checked
            .diags
            .iter()
            .map(|r| (r.diag.pos.line, r.diag.pos.col, r.diag.code))
            .collect::<Vec<_>>()
    );
    assert_eq!(checked.rendered_count(), 0);
}

/// `range over int`（1.22）在 `go1.21` 文件里被拒 —— 落在**操作数**、类别 `version`。
///
/// 官方 `softErrorf(&x, InvalidRangeExpr, "cannot range over %s: %s", …)`
/// 把门控并进 range 的消息里，但 C8 适配表的第一条规则是 `requires go1\.`
/// ⇒ 类别 `version`（`go1_22_21.go` 实测同形）。
#[test]
fn t_ck_36_版本门控_整型_go1_21() {
    const SRC: &str = "//go:build go1.21\n\npackage p\n\nfunc f() {\n\tfor range 10 {\n\t}\n}\n";
    let (mut tt, pkg, bag) = load(SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(
        rows(&checked.render("t.go")),
        vec!["6:12\tversion"],
        "门控落点与类别"
    );
    assert_eq!(
        checked.rendered_count(),
        1,
        "只报一条（官方同形：一个错误）"
    );
}

/// `range over func`（1.23）在 `go1.22` 文件里被拒；1.22 的整型 range 放行。
#[test]
fn t_ck_36_版本门控_func_go1_22() {
    const SRC: &str = concat!(
        "//go:build go1.22\n\n",
        "package p\n\n",
        "func f(it func(func(int) bool)) {\n",
        "\tfor range 10 {\n",
        "\t}\n",
        "\tfor range it {\n",
        "\t}\n",
        "}\n"
    );
    let (mut tt, pkg, bag) = load(SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(
        rows(&checked.render("t.go")),
        vec!["8:12\tversion"],
        "1.22 放行整型落在 7 行；func 在 8 行被 1.23 门控拒"
    );
    assert_eq!(checked.rendered_count(), 1);
}

/// 报点用**最左 token**：`for range 1 << 63` 的溢出报在 `1`，不是 `<<`。
///
/// 本仓 `Operation.pos` 存运算符位置，`Expr::pos()` 会给出 `<<` 那一列；
/// 官方的 `operand.pos` 等于 `ast.Expr.Pos()`（= 左操作数）⇒ 必须走
/// `Expr::start_pos()`（`version::pos_of_range_operand` 与 `x_pos` 同源）。
#[test]
fn t_ck_36_报点是最左_token() {
    const SRC: &str = "package p\n\nfunc f() {\n\tfor range 1 << 63 {\n\t}\n}\n";
    let (mut tt, pkg, bag) = load(SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(
        rows(&checked.render("t.go")),
        vec!["4:12\tassign"],
        "溢出报在 `1`（第 12 列）"
    );
}
