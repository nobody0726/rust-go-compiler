//! `T-CON-34` / Task 51：`rgoc-check` 的 **comma-ok 接收**面。
//!
//! # 这条判据的来历
//!
//! `v, ok := <-c` 在 M5–M7 一直报 `CHK-INVALID-ASSIGN`（"2 variables but
//! 1 value"）—— 因为 `recv` 的操作数模式是 `Mode::Value`，而官方是
//! **`commaok`**（`go/types/expr.go:150` 的 `case token.ARROW`）。
//! M8/Task 10 把模式改了过来，这一层把三条边界钉死：
//!
//! | 形态 | 期望 |
//! |---|---|
//! | `v, ok := <-c` | 零诊断 |
//! | `v, ok := m[k]` | 零诊断（**回归**：不许把 map 索引那条路带坏） |
//! | `v, ok := f()`（`f` 单返回值） | 报错（不是 comma-ok，是真不匹配） |

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_lex::DiagCode;
use rgoc_load::load_sources;
use rgoc_types::TypeTable;

/// 装载 + 检查一个内存单文件包，返回诊断的 `(行, 列, 码)`。
fn diags(src: &str) -> Vec<(u32, u32, DiagCode)> {
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_sources(&mut tt, &[("t.go", src.as_bytes())], None);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    checked
        .diags
        .iter()
        .map(|r| (r.diag.pos.line, r.diag.pos.col, r.diag.code))
        .collect()
}

#[test]
fn t_con_34_接收的双值形态零诊断() {
    let got = diags(
        r#"
package p

var c chan int

func f() {
	v, ok := <-c
	_ = v
	_ = ok
	x, y := <-c
	_ = x
	_ = y
	z, w := m[k]
	_ = z
	_ = w
}

var m map[string]int
var k string
"#,
    );
    assert!(
        got.is_empty(),
        "`v, ok := <-c` 与 `x, y := <-c` 必须零诊断：{got:?}"
    );
}

#[test]
fn t_con_34b_map_索引的双值形态仍是零诊断() {
    // 回归：recv 的模式改成 commaok 之后，map 索引那条路不许被带坏。
    let got = diags(
        r#"
package p

var m map[string]int

func f() {
	v, ok := m["a"]
	_ = v
	_ = ok
}
"#,
    );
    assert!(got.is_empty(), "`v, ok := m[k]` 必须零诊断：{got:?}");
}

#[test]
fn t_con_34c_单返回值函数不能当双值() {
    // 反向：`f` 只返回一个值 ⇒ 这条**该报错**（comma-ok 不是万能创可贴）。
    let got = diags(
        r#"
package p

func one() int { return 1 }

func f() {
	v, ok := one()
	_ = v
	_ = ok
}
"#,
    );
    assert_eq!(got.len(), 1, "单返回值函数当双值用必须报一条：{got:?}");
    assert_eq!(got[0].2, DiagCode::ChkInvalidAssign);
}

#[test]
fn t_con_34d_非通道上接收仍然报错() {
    let got = diags(
        r#"
package p

var n int

func f() {
	<-n
}
"#,
    );
    assert_eq!(got.len(), 1, "非通道接收必须报一条：{got:?}");
    assert_eq!(got[0].2, DiagCode::ChkInvalidOperation);
}
