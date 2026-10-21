//! `T-CK-41/42` 内建函数的签名与静态约束（M4/Task 29）。
//!
//! # 判据
//!
//! 官方 `go/types/builtins.go` 的 `builtin` 主分派 + `universe.go:180` 的
//! `predeclaredFuncs`（元数表）。逐支对齐见 [`rgoc_check`] 的
//! `builtins` 模块头。
//!
//! # 类别（`C8` 适配表）
//!
//! | 官方消息前缀 | 本仓码 | 类别 |
//! |---|---|---|
//! | `invalid argument: ` / `invalid append: ` / `invalid copy: ` | `ChkInvalidBuiltinArg` | `builtin` |
//! | `invalid operation: `（`cannot close …` / `complex(…) (mismatched types …)`） | `ChkInvalidOperation` | `op-invalid` |
//! | `cannot use … as … value in argument to append` | `ChkInvalidAssign` | `assign` |
//! | `invalid argument: index … must not be negative`（`make` 尺寸面） | `ChkInvalidIndex` | `index` |
//!
//! # 夹具与 oracle 证据
//!
//! 拒绝夹具与 `.superpowers/sdd/M4-plan/probe/task29-builtins.go` **逐字节
//! 一致**；oracle `--check` 实测 14 行（本文件第一条测试的 `want` 即其原样）。
//! 元数夹具与 `probe/task29-arity.go` 逐字节一致（10 行，全部 `builtin`）。
//!
//! # 报点
//!
//! - 实参面：官方 `check.errorf(x, …)`，`x` 是该实参的 `*operand` ⇒ 报在
//!   **实参起点**（`18:13` 的 `n`）；
//! - 元数面：官方报在**调用右括号**（`8:17` 的 `)`）—— 这条容易被写成
//!   「第 N+1 个实参」，实测不是；
//! - `append` 的元素面走 `check.assignment` ⇒ 报在**元素实参起点**
//!   （`21:19` 的 `"x"`），类别随之为 `assign`。
//!
//! # 刻意不测（Ruling 29-x，台账登记）
//!
//! - 类型参数操作数（`func f[P ~[]int](s P) { _ = len(s) }`）：`D-M3-2` 窄面，
//!   检查器不判；
//! - `append(b, s...)` / `copy([]byte, string)` 的 `...` 展开形态：元素判定
//!   跳过（不假报），只接非 `...` 形态；
//! - `unsafe` 的 `Add/Sizeof/Alignof/Offsetof/Slice/String/StringData`：
//!   归 Task 31（`unsafe_pkg` 面）。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_lex::DiagCode;
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 拒绝 / 接受混合夹具（与 `probe/task29-builtins.go` 逐字节一致）。
const SRC: &str = r#"// T-CK-41/42 内建签名与静态约束 —— 拒绝/接受夹具（Task 29）。
// 与 `rgoc/crates/rgoc-check/tests/builtins.rs` 的 `SRC` 逐字节一致。
package p

type S []int
type M map[string]int

var s []int
var m map[string]int
var ch chan int
var ro <-chan int
var n int
var str string

var _ = len(s)
var _ = len("abc")
var _ = cap(s)
var _ = len(n)
var _ = cap(n)
var _ = append(s, 1)
var _ = append(s, "x")
var _ = append(n, 1)
var _ = make([]int, 3)
var _ = make([]int, n)
var _ = make([]int, -1)
var _ = make(int)
var _ = new(int)
var _ = copy(s, s)
var _ = copy(n, s)
var _ = complex(1, 2)
var _ = complex(1.0, "x")
var _ = real(1 + 2i)
var _ = imag(1 + 2i)
var _ = real(n)
var _ = min(1, 2)
var _ = max(1.0, 2)
var _ = min("a", 1)

func stmts() {
	close(ch)
	close(ro)
	close(n)
	delete(m, "k")
	delete(s, 1)
	clear(m)
	clear(s)
	clear(n)
	println(1, "x")
}
"#;

/// 元数夹具（与 `probe/task29-arity.go` 逐字节一致）。
const ARITY_SRC: &str = r#"// T-CK-41 内建元数面 —— 探针（Task 29）。
package p

var s []int
var m map[string]int

var _ = len()
var _ = len(s, s)
var _ = copy(s)
var _ = append()
var _ = complex(1)
var _ = make()
var _ = new()
var _ = close()
var _ = recover(1)
var _ = delete(m)
"#;

/// 仅接受面（零诊断守卫）。
const ACCEPT_SRC: &str = r#"// T-CK-42 内建签名 —— 接受夹具（Task 29）。
package p

type S []int
type M map[string]int

var s []int
var m map[string]int
var ch chan int
var n int
var str string

var _ = len(s)
var _ = len("abc")
var _ = len(m)
var _ = cap(s)
var _ = cap(ch)
var _ = append(s, 1, 2)
var _ = append(s, s...)
var _ = make([]int, 3, 4)
var _ = make(map[string]int)
var _ = make(chan int, 1)
var _ = new(int)
var _ = copy(s, s)
var _ = complex(1, 2)
var _ = real(1 + 2i)
var _ = imag(1 + 2i)
var _ = min(1, 2)
var _ = max(1.0, 2)
var _ = min("a", "b")

func stmts() {
	close(ch)
	delete(m, "k")
	clear(m)
	clear(s)
	println(1, "x")
	panic("x")
	_ = recover()
}
"#;

fn load(src: &str) -> (TypeTable, Package, rgoc_lex::DiagBag) {
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_sources(&mut tt, &[("t.go", src.as_bytes())], None);
    (tt, pkg, bag)
}

fn rows(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// 拒绝向量：14 行 `(行:列, 类别)` 与 oracle 逐字节一致。
#[test]
fn t_ck_41_拒绝向量_类别与位置逐条一致() {
    let (mut tt, pkg, bag) = load(SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(
        rows(&checked.render("t.go")),
        vec![
            "18:13\tbuiltin",
            "19:13\tbuiltin",
            "21:19\tassign",
            "22:16\tbuiltin",
            "25:21\tindex",
            "26:14\tbuiltin",
            "29:14\tbuiltin",
            "31:17\top-invalid",
            "34:14\tbuiltin",
            "37:18\tbuiltin",
            "41:8\top-invalid",
            "42:8\top-invalid",
            "44:9\tbuiltin",
            "47:8\tbuiltin",
        ],
        "渲染行 vs oracle（`probe/task29-builtins.go` 实测）"
    );
}

/// 元数面：10 行全部 `builtin`，报点一律是**调用右括号**。
#[test]
fn t_ck_41_元数面_报点落在右括号() {
    let (mut tt, pkg, bag) = load(ARITY_SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(
        rows(&checked.render("t.go")),
        vec![
            "7:13\tbuiltin",
            "8:17\tbuiltin",
            "9:15\tbuiltin",
            "10:16\tbuiltin",
            "11:18\tbuiltin",
            "12:14\tbuiltin",
            "13:13\tbuiltin",
            "14:15\tbuiltin",
            "15:18\tbuiltin",
            "16:17\tbuiltin",
        ],
        "渲染行 vs oracle（`probe/task29-arity.go` 实测）"
    );
    // 码面：全部是 `ChkInvalidBuiltinArg`，没有 `ChkInvalidOperation`。
    assert!(
        checked
            .diags
            .iter()
            .all(|r| r.diag.code == DiagCode::ChkInvalidBuiltinArg),
        "元数面必须是 `ChkInvalidBuiltinArg`，got {:?}",
        checked
            .diags
            .iter()
            .map(|r| r.diag.code)
            .collect::<Vec<_>>()
    );
}

/// 接受向量：合法内建调用零诊断（含 `...` 展开、`make` 三形态、常量折叠面）。
#[test]
fn t_ck_42_接受向量_零诊断() {
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

/// `make` 的**形状相关上界**：切片 2..=3、map / chan 1..=2；报点是调用起点
/// （`make` 标识符，**不是**右括号 —— 与通用元数面不同）。
#[test]
fn t_ck_41_make_形状上界() {
    const SRC2: &str = "// T-CK-41 make 元数面 —— 探针（Task 29）。\npackage p\n\nvar _ = make([]int, 1, 2)\nvar _ = make([]int, 1, 2, 3)\nvar _ = make(map[string]int, 1, 2)\nvar _ = make(chan int, 1, 2)\n";
    let (mut tt, pkg, bag) = load(SRC2);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(
        rows(&checked.render("t.go")),
        vec!["5:9\tbuiltin", "6:9\tbuiltin", "7:9\tbuiltin"],
        "渲染行 vs oracle（`probe/task29-make.go` 实测）"
    );
}

/// `len` / `cap` / `min` / `max` 的常量折叠面**暂不在本任务收口**：
/// `rgoc-load` 的 const 初始化式只认字面量算式（`U-M3-08`），`const n =
/// len("abc")` 仍被 `TypInvalidConstInit` 拒绝。折叠结果的消费面由
/// Task 30（`T-CK-43`）随 `U-M3-08` 一并打开 —— 这里只钉住**当前边界**，
/// 免得日后有人把「折叠没生效」误判成回归。
#[test]
fn t_ck_42_常量折叠面_受_u_m3_08_限制() {
    const SRC3: &str = "package p\n\nconst n = len(\"abc\")\nvar a [n]int\nvar _ = a\n";
    let (mut tt, pkg, bag) = load(SRC3);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(
        rows(&checked.render("t.go")),
        vec!["3:11\tdecl"],
        "`U-M3-08` 的当前边界（Task 30 打开后本行会变）"
    );
    assert_eq!(checked.diags[0].diag.code, DiagCode::TypInvalidConstInit);
}
