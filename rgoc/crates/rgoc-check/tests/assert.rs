//! `T-CK-38` 类型断言静态约束（M4/Task 28）：`v.(T)` 的**接口性**与
//! `assertableTo`（官方 `expr.go:1091` / `lookup.go:600`）。
//!
//! # 判据
//!
//! 1. 操作数必须是接口（`underlying` 是 `Interface`）—— 否则
//!    `invalid operation: … is not an interface`（类别 `op-invalid`）；
//! 2. 目标是接口 ⇒ **不做静态判定**（spec：「If T is an interface type,
//!    x.(T) asserts that the dynamic type of x implements T」）——
//!    `i.(J)` 即使 `I` 与 `J` 无交集也合法；
//! 3. 目标是具体类型 ⇒ `implements(T, I)`（**目标**必须实现**源**接口）——
//!    否则 `impossible type assertion`（类别 `comma-ok`）。
//!
//! # 夹具与 oracle 证据
//!
//! 夹具与 `.superpowers/sdd/M4-plan/probe/task28-assert.go` **逐字节一致**；
//! oracle `--check` 实测 3 行：
//! `17:9 op-invalid` / `19:9 comma-ok` / `20:9 comma-ok`。
//!
//! # 报点
//!
//! 官方 `check.errorf(e, …)`，`e` 是 `*ast.TypeAssertExpr` ⇒ `Pos()` 是
//! **操作数起点**（`17:9` 的 `n`，不是 `(`）。本仓 `AssertExpr.pos` 存的
//! 是括号面，故一律走 `e.x.start_pos()`。
//!
//! # 刻意不测（Ruling 28-x，台账登记）
//!
//! - 类型参数操作数（`func f[P any](x P) { x.(I) }`）：`D-M3-2` 窄面，
//!   检查器**不报**（`TypeParam` 分支直接 `invalid`）；
//! - `x.(type)` 在 type switch 之外（官方 `BadTypeKeyword`）：守卫面归
//!   Task 25 的 type switch 检查，本任务不重复报。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_lex::DiagCode;
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 拒绝 / 接受混合夹具（与 `probe/task28-assert.go` 逐字节一致）。
const SRC: &str = r#"// T-CK-38 类型断言静态约束 —— 拒绝/接受夹具（Task 28）。
// 与 `rgoc/crates/rgoc-check/tests/assert.rs` 的 `SRC` 逐字节一致。
package p

type I interface{ M() }
type J interface{ P() }

type T struct{}

func (T) M()  {}
func (*T) P() {}

var i I
var j J
var n int

var _ = n.(I)
var _ = i.(J)
var _ = i.(int)
var _ = j.(T)
var _ = i.(T)
var _ = i.(*T)
var _, _ = i.(T)
var _, _ = i.(J)
"#;

/// 仅接受面（零诊断守卫）。
const ACCEPT_SRC: &str = r#"// T-CK-38 类型断言静态约束 —— 接受夹具（Task 28）。
// 目标是接口（`I` / `J` / `any`）或实现源接口的具体类型（`T` / `*T`）。
package p

type I interface{ M() }
type J interface{ P() }

type T struct{}

func (T) M()  {}
func (*T) P() {}

var i I
var j J

var _ = i.(J)
var _ = j.(I)
var _ = i.(T)
var _ = i.(*T)
var _ = i.(any)
var _, _ = i.(T)
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

/// 拒绝向量：`(行,列,码)` 逐条 + 渲染行与 oracle 逐字节一致。
#[test]
fn t_ck_38_拒绝向量_码与位置逐条一致() {
    let (mut tt, pkg, bag) = load(SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());

    let mut got: Vec<(u32, u32, DiagCode)> = checked
        .diags
        .iter()
        .map(|r| (r.diag.pos.line, r.diag.pos.col, r.diag.code))
        .collect();
    got.sort();
    let mut want = vec![
        (17, 9, DiagCode::ChkInvalidOperation),
        (19, 9, DiagCode::ChkImpossibleAssert),
        (20, 9, DiagCode::ChkImpossibleAssert),
    ];
    want.sort();
    assert_eq!(got, want, "逐条 (行,列,码)");

    assert_eq!(
        rows(&checked.render("t.go")),
        vec!["17:9\top-invalid", "19:9\tcomma-ok", "20:9\tcomma-ok"],
        "渲染行 vs oracle（`probe/task28-assert.go` 实测）"
    );
}

/// 接受向量：目标接口**不做**静态判定；`T` / `*T` 各自实现 `I` ⇒ 零诊断。
#[test]
fn t_ck_38_接受向量_零诊断() {
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

/// `comma-ok` 形态：`var v, ok = i.(T)` 与单值形态同样零诊断
/// （结果模式 = commaok；官方 `singleValue` 对 commaok 不报）。
#[test]
fn t_ck_38_comma_ok_形态零诊断() {
    const SRC2: &str = "package p\n\ntype I interface{ M() }\ntype T struct{}\n\nfunc (T) M() {}\n\nvar i I\nvar v, ok = i.(T)\n";
    let (mut tt, pkg, bag) = load(SRC2);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert!(
        checked.diags.is_empty(),
        "comma-ok 形态必须零诊断，got {:?}",
        checked
            .diags
            .iter()
            .map(|r| (r.diag.pos.line, r.diag.pos.col, r.diag.code))
            .collect::<Vec<_>>()
    );
}

/// `*T` 的指针接收者方法在目标类型是 `T` 时**不计入**方法集 ——
/// `j.(T)` 的官方原因（`T does not implement J (method P has pointer receiver)`）。
/// 这里钉住「断言失败」而不是「断言成功」这一侧。
#[test]
fn t_ck_38_指针接收者方法不计入值方法集() {
    const SRC3: &str = "package p\n\ntype J interface{ P() }\ntype T struct{}\n\nfunc (*T) P() {}\n\nvar j J\nvar _ = j.(T)\n";
    let (mut tt, pkg, bag) = load(SRC3);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(
        rows(&checked.render("t.go")),
        vec!["9:9\tcomma-ok"],
        "`T` 不含 `*T` 的方法 ⇒ 断言不可能"
    );
    // 反过来 `*T` 是合法的。
    const SRC4: &str = "package p\n\ntype J interface{ P() }\ntype T struct{}\n\nfunc (*T) P() {}\n\nvar j J\nvar _ = j.(*T)\n";
    let (mut tt, pkg, bag) = load(SRC4);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert!(checked.diags.is_empty(), "*T 实现 J ⇒ 合法");
}
