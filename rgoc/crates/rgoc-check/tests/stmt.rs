//! `T-CK-35` 语句静态面（M4/Task 25）：拒绝向量的（行,列,码）逐条对账 +
//! oracle 类别对撞，接受向量零诊断。
//!
//! # 夹具与 oracle 证据
//!
//! 两个夹具与 `.superpowers/sdd/M4-plan/probe/task25-{reject,accept}.go`
//! **逐字节一致**。oracle 实测（`tools/types-oracle --check`）：
//!
//! - 拒绝夹具：**35 行**（`probe/task25-oracle-pin.log` 原样输出 —— 位+类别）；
//! - 接受夹具：**0 行**（假阳性守卫；首轮曾在 `163:11` 多报一条
//!   `var f3 string` 之后的 `e3, f3 = pair()`，修夹具后复跑零诊断）。
//!
//! # 断言分两层
//!
//! ① 我方 `(行,列,码)` 与手写期望表逐条相同（码是**本仓**的划分）；
//! ② 我方渲染行（`C8`：位置 + 类别）与 oracle 行**逐字节**相同 ——
//!    证明「位置 + 类别」对齐，而不是码表自洽。
//!
//! ⚠️ 流内顺序**不是**源序：`118/125` 两条 `fallthrough` 由**语法器**报
//! （`SYN-BRANCH-FALLTHROUGH`，随装载诊断先入流），其余 33 条由检查面报。
//! 所以期望表**按 (行,列) 排序后**比较，而不是按流序。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_lex::DiagCode;
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 拒绝夹具（与 probe 副本逐字节一致）。
const REJECT_SRC: &str = r#"// T-CK-35 语句面 —— 拒绝夹具（Task 25）。
// 与 `rgoc/crates/rgoc-check/tests/stmt.rs` 的 `REJECT_SRC` 逐字节一致。
package p

var x int
var s string
var ch chan int
var iv interface{}

func nores() {}

func assignType() {
	x = s
}

func assignCount() {
	var a, b int
	a, b = 1
	_, _ = a, b
}

func compoundType() {
	s += 1
}

func compoundType2() {
	x += s
}

func incDecNonNumeric() {
	s++
}

func assignToLiteral() {
	1 = 2
}

func shortNoNew() {
	y := 1
	y := 2
	_ = y
}

func shortNoNew2() {
	p, q := 1, 2
	p, q := 3, 4
	_, _ = p, q
}

func condIf() {
	if 1 {
	}
}

func condFor() {
	for "s" {
	}
}

func switchCaseMismatch() {
	switch x {
	case s:
	}
}

func switchDuplicateCase() {
	switch x {
	case 1:
	case 1:
	}
}

func selectBadComm() {
	select {
	case x:
	}
}

func selectTwoDefaults() {
	select {
	default:
	default:
	}
}

func goDiscards() {
	go len(s)
}

func deferConversion() {
	defer int(x)
}

func retTypeMismatch() int {
	return s
}

func retFewValues() (int, int) {
	return 1
}

func retManyValues() int {
	return 1, 2
}

func missingReturn() int {
}

func missingReturn2() string {
	if x > 0 {
		return "a"
	}
}

func fallthroughFinal() {
	switch x {
	case 1:
		fallthrough
	}
}

func fallthroughTypeSwitch() {
	switch iv.(type) {
	case int:
		fallthrough
	}
}

func tsGuardNotInterface() {
	switch v := x.(type) {
	case int:
		_ = v
	}
}

func tsCaseNotType() {
	switch iv.(type) {
	case 1:
	}
}

func exprNotUsed() {
	x + 1
}

func exprNotUsed2() {
	s
}

func sendNonChan() {
	x <- 1
}

func sendElemMismatch() {
	ch <- s
}

func argFn(a int) {}

func callBadArg() {
	argFn(s)
}

func callFewArgs() {
	argFn()
}

func callManyArgs() {
	argFn(1, 2)
}

func pair() (int, string) { return 1, "a" }

func multiBadAssign() {
	x = pair()
}

func multiBadShort() {
	z := pair()
	_ = z
}

func multiSingle() {
	var a2, b2 int
	a2, b2 = pair(), 1
	_, _ = a2, b2
}
"#;

/// 接受夹具（与 probe 副本逐字节一致）。
const ACCEPT_SRC: &str = r#"// T-CK-35 语句面 —— 接受夹具（Task 25）。
// 与 `rgoc/crates/rgoc-check/tests/stmt.rs` 的 `ACCEPT_SRC` 逐字节一致。
package p

var x int
var s string
var ch chan int
var iv interface{}

func nores() {}

func assignOK() {
	x = 1
	x += 2
	s += "a"
	x++
	x--
	_ = x
}

func shortOK() {
	y := 1
	y, z := 2, 3
	_, w := 4, 5
	_, _, _, _ = y, z, w, s
}

func declOK() {
	var a int
	const c = 3
	type myint int
	a = c
	var m myint
	_ = a
	_ = m
}

func condOK() {
	if true {
		nores()
	}
	for x > 0 {
		break
	}
	if y := x + 1; y > 0 {
		_ = y
	}
}

func switchOK() {
	switch x {
	case 1, 2:
	default:
	}
	switch {
	case x > 0:
	}
	switch v := iv.(type) {
	case int:
		_ = v
	case string, nil:
	}
}

func selectOK() {
	select {
	case ch <- 1:
	case v := <-ch:
		_ = v
	default:
	}
}

func loopOK() {
	for {
		<-ch
		x = 1
	}
}

func goOK() {
	go nores()
	defer nores()
}

func retOK() int {
	x = 1
	return x
}

func retNamedOK() (r int) {
	return
}

func retNamedOK2() (r int) {
	r = 2
	return
}

func retPairOK() (int, string) {
	return 1, "a"
}

func termOK1() int {
	for {
	}
}

func termOK2() int {
	if x > 0 {
		return 1
	} else {
		return 2
	}
}

func termOK3() int {
	switch x {
	case 1:
		return 1
	default:
		return 2
	}
}

func termOK4() int {
	panic("boom")
}

func termOK5() (int, error) {
	return 1, nil
}

func termOK6() int {
	switch x {
	case 1:
		fallthrough
	case 2:
		return 1
	default:
		return 2
	}
}

func argFn(a int) {}

func callOK() {
	argFn(1)
	argFn(x)
	go argFn(x)
	defer argFn(x)
}

func pair() (int, string) { return 1, "a" }

func multiOK() {
	a3, b3 := pair()
	_, _ = a3, b3
	var c3, d3 int
	c3, d3 = 1, 2
	_, _ = c3, d3
	var e3 int
	var f3 string
	e3, f3 = pair()
	_, _ = e3, f3
}
"#;

/// oracle 对拒绝夹具的**原样输出行**（`--check`，剥 `#` 头）。
const ORACLE_REJECT_ROWS: [&str; 35] = [
    "13:6	assign",
    "18:9	assign",
    "23:2	op-invalid",
    "27:2	op-invalid",
    "31:2	op-invalid",
    "35:2	assign",
    "40:4	decl",
    "46:7	decl",
    "51:5	stmt",
    "56:6	stmt",
    "62:7	stmt",
    "69:7	stmt",
    "75:7	stmt",
    "82:2	stmt",
    "87:5	stmt",
    "91:8	stmt",
    "95:9	assign",
    "99:9	ret",
    "103:12	ret",
    "107:1	terminate",
    "113:1	terminate",
    "118:3	stmt",
    "125:3	stmt",
    "130:14	op-invalid",
    "138:7	not-type",
    "143:2	unused",
    "147:2	unused",
    "151:4	op-invalid",
    "155:8	assign",
    "161:8	assign",
    "165:8	assign",
    "169:11	assign",
    "175:6	assign",
    "179:7	assign",
    "185:11	comma-ok",
];

/// 手写期望表：`(行, 列, 码)`，**按 (行,列) 排序**（见模块头的流序说明）。
///
/// 位置来源（oracle 钉死 + 官方源码双证）：
/// - `cannot use ... in <语境>` ⇒ **该操作数**位（`assignment` 的 `x`）；
/// - `assignment mismatch` ⇒ **右侧第一个**表达式位（`assignError` 的 `rhs0`）；
/// - 复合赋值 / `++`、`--` 的 `invalid operation` ⇒ **左操作数起点**；
/// - `cannot assign to` ⇒ **左值**位；
/// - `no new variables` ⇒ **`:=`** 词元位（官方 `inNode(s, s.TokPos)`；
///   `AssignStmt.pos` 存的正是操作符位）；
/// - `non-boolean condition` ⇒ **条件表达式**位；
/// - `invalid/duplicate case` ⇒ **case 值**位；
/// - `select case must be` ⇒ **通信子句**起点；`multiple defaults` ⇒ 第二个
///   `default` 的 `default` 关键字位（子句 `pos`）；
/// - `go discards` / `defer requires` ⇒ **调用表达式**位（= `(` 位）；
/// - `cannot use ... in return statement` ⇒ **结果表达式**位；
/// - 返回值个数不足 ⇒ **最后一个实际值**；过多 ⇒ **第一个多出来的值**；
/// - `missing return` ⇒ **`func` 关键字**位（函数起点）；
/// - `is not an interface`（类型 switch 守卫）⇒ **守卫的 `x`** 位；
/// - `is not a type`（类型 switch case）⇒ **case 值**位；
/// - `is not used` ⇒ **表达式语句**起点；
/// - `cannot send to non-channel` ⇒ **`<-`** 位（`inNode(s, s.Arrow)`；
///   `SendStmt.pos` 存的正是箭头位）；
/// - 实参不足 ⇒ **`)`** 位（`call.go:541` 的 `atPos(call.Rparen)`）；实参过多
///   ⇒ **第一个多余实参**位；实参类型不符 ⇒ **该实参**位；
/// - 多值落在单值语境 ⇒ **该多值表达式**位。
const EXPECT_REJECT: [(u32, u32, DiagCode); 35] = [
    (13, 6, DiagCode::ChkInvalidAssign),
    (18, 9, DiagCode::ChkInvalidAssign),
    (23, 2, DiagCode::ChkInvalidOperation),
    (27, 2, DiagCode::ChkInvalidOperation),
    (31, 2, DiagCode::ChkInvalidOperation),
    (35, 2, DiagCode::ChkInvalidAssign),
    (40, 4, DiagCode::ChkNoNewVar),
    (46, 7, DiagCode::ChkNoNewVar),
    (51, 5, DiagCode::ChkInvalidCond),
    (56, 6, DiagCode::ChkInvalidCond),
    (62, 7, DiagCode::ChkInvalidCase),
    (69, 7, DiagCode::ChkInvalidCase),
    (75, 7, DiagCode::ChkInvalidSelectCase),
    (82, 2, DiagCode::ChkInvalidSelectCase),
    (87, 5, DiagCode::ChkInvalidGoDefer),
    (91, 8, DiagCode::ChkInvalidGoDefer),
    (95, 9, DiagCode::ChkInvalidAssign),
    (99, 9, DiagCode::ChkWrongResultCount),
    (103, 12, DiagCode::ChkWrongResultCount),
    (107, 1, DiagCode::ChkMissingReturn),
    (113, 1, DiagCode::ChkMissingReturn),
    (118, 3, DiagCode::SynBranchFallthrough),
    (125, 3, DiagCode::SynBranchFallthrough),
    (130, 14, DiagCode::ChkInvalidOperation),
    (138, 7, DiagCode::ChkNotAType),
    (143, 2, DiagCode::ChkUnusedExpr),
    (147, 2, DiagCode::ChkUnusedExpr),
    (151, 4, DiagCode::ChkInvalidOperation),
    (155, 8, DiagCode::ChkInvalidAssign),
    (161, 8, DiagCode::ChkInvalidAssign),
    (165, 8, DiagCode::ChkInvalidAssign),
    (169, 11, DiagCode::ChkInvalidAssign),
    (175, 6, DiagCode::ChkInvalidAssign),
    (179, 7, DiagCode::ChkInvalidAssign),
    (185, 11, DiagCode::ChkSingleValueContext),
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
fn t_ck_35_拒绝向量_码与位置逐条一致() {
    let (mut tt, pkg, bag) = load(REJECT_SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());

    // ① 码 + 位置（本仓划分）。流序 = 装载诊断（语法器的 fallthrough）在前，
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

    // ② 渲染行 == oracle 原样输出（位置 + 类别逐字节；渲染层按位置稳定排序）。
    let rendered_text = checked.render("t.go");
    let rendered: Vec<&str> = rows(&rendered_text);
    assert_eq!(rendered, ORACLE_REJECT_ROWS.to_vec(), "渲染行 vs oracle");
}

/// 接受向量：整文件零诊断（假阳性守卫；oracle 实测同样 0 行）。
#[test]
fn t_ck_35_接受向量_零诊断() {
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
