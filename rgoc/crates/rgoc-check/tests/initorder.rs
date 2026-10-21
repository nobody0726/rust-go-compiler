//! `T-CK-71` 初始化依赖图与顺序（M4/Task 32）。
//!
//! # 判据来源
//!
//! 官方 `go/types/initorder.go`（`types.Info.InitOrder` 的对应物）。本任务的
//! 输出**不是** C8 诊断行，所以没有 `run-collide --check` 的对撞面；证据是
//! 一次性探针 `probe/t32-msgs.go` 打印的官方 `InitOrder`，逐行钉在这里。
//!
//! # 四条实测语义（`Ruling 32-1`）
//!
//! 1. 每行一个初始化器，取 `Lhs` 的名字列表；`var i, j = 1, 2` 被官方拆成
//!    **两个**初始化器，`var a, b = f()` 才是一个带两个 `Lhs`。
//! 2. `FuncLit` 体里的引用**计入**依赖。
//! 3. 被调用函数的函数体**也计入**依赖（调用图的传递闭包）。
//! 4. 裸函数体引用**不**计入（没人从初始化式里调它）。
//!
//! # 证据
//!
//! `probe/t32-initorder.go`（与 [`SRC`] 逐字节一致）的 oracle 输出 9 行：
//! `c` / `a` / `b` / `d` / `e` / `h` / `g` / `i` / `j`。
//! `probe/t32-funclit.go`（与 [`FUNCLIT_SRC`] 逐字节一致）输出 4 行：
//! `q1` / `p1` / `q2` / `p2`。
//!
//! # 环
//!
//! 真环的诊断归 Task 33（`T-CK-72`）；本任务只保证 DFS **不死循环**。

use rgoc_check::initorder::compute;
use rgoc_load::load_sources;
use rgoc_types::TypeTable;

/// 主夹具（与 `probe/t32-initorder.go` 逐字节一致）。
const SRC: &str = r#"// T-CK-71 初始化依赖图与顺序 —— 夹具（Task 32）。
// 与 `rgoc/crates/rgoc-check/tests/initorder.rs` 的 `SRC` 逐字节一致。
package p

var a = c + 1
var b = a + 1
var c = 1

var d = k()
var e = d

func k() int { return 0 }

// 函数体引用**不**计入依赖：`g` 依赖 `h`，`h` 在 `g` 之后声明。
var g = h
var h = 1

func m() int { return g }

// 多名字初始化器。
var i, j = 1, 2
"#;

/// `FuncLit` / 被调用函数体的依赖面（与 `probe/t32-funclit.go` 逐字节一致）。
const FUNCLIT_SRC: &str = r#"// T-CK-71 FuncLit 是否计入依赖 —— 探针（Task 32）。
package p

var p1 = func() int { return q1 }
var q1 = 1

var p2 = f2()
var q2 = 1

func f2() int { return q2 }
"#;

fn order(src: &str) -> Vec<String> {
    let mut tt = TypeTable::new();
    let (pkg, _bag) = load_sources(&mut tt, &[("t.go", src.as_bytes())], None);
    compute(&pkg).render()
}

/// 主夹具：依赖链 + 跨声明前向引用 + 多名字拆分，逐行与 oracle 一致。
#[test]
fn t_ck_71_顺序与_oracle_逐行一致() {
    assert_eq!(
        order(SRC),
        vec!["c", "a", "b", "d", "e", "h", "g", "i", "j"],
        "顺序 vs oracle（`probe/t32-initorder.go` 实测）"
    );
}

/// `FuncLit` 体与**被调用函数体**都计入依赖（`Ruling 32-1` 第 2/3 条）。
#[test]
fn t_ck_71_函数面依赖计入() {
    assert_eq!(
        order(FUNCLIT_SRC),
        vec!["q1", "p1", "q2", "p2"],
        "顺序 vs oracle（`probe/t32-funclit.go` 实测）"
    );
}

/// `var a, b = f()` ⇒ **一个**初始化器带两个 `Lhs`（`Ruling 32-1` 第 1 条）。
#[test]
fn t_ck_71_单值多名_合成一个初始化器() {
    const S: &str =
        "package p\n\nfunc f() (int, int) { return 0, 0 }\n\nvar a, b = f()\nvar c, d = 1, 2\n";
    assert_eq!(
        order(S),
        vec!["a,b", "c", "d"],
        "单值多名合成一行；逐名配对拆成多行"
    );
}

/// 裸函数体引用不构成依赖：`m` 用了 `g`，但没人调 `m` ⇒ `g` 不动。
#[test]
fn t_ck_71_裸函数体引用不计入() {
    const S: &str = "package p\n\nvar g = 1\nvar h = 2\n\nfunc m() int { return g + h }\n";
    assert_eq!(order(S), vec!["g", "h"], "函数体引用不影响顺序");
}

/// 跨文件：依赖按「文件序 → 声明序」排（`a.go` 的 `x` 依赖 `b.go` 的 `y`）。
#[test]
fn t_ck_71_跨文件依赖() {
    let mut tt = TypeTable::new();
    let (pkg, _bag) = load_sources(
        &mut tt,
        &[
            ("a.go", "package p\n\nvar x = y + 1\n".as_bytes()),
            ("b.go", "package p\n\nvar y = 1\n".as_bytes()),
        ],
        None,
    );
    assert_eq!(compute(&pkg).render(), vec!["y", "x"]);
}

/// 环：不死循环（诊断归 Task 33），且每个成员都出现在输出里。
#[test]
fn t_ck_71_环不死循环() {
    const S: &str = "package p\n\nvar a = b\nvar b = a\n";
    let got = order(S);
    assert_eq!(got.len(), 2, "环上两个成员都要出现在顺序里：{got:?}");
    let mut sorted = got.clone();
    sorted.sort();
    assert_eq!(sorted, vec!["a", "b"]);
}
