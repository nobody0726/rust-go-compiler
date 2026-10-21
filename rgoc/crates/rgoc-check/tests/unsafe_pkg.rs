//! `T-CK-44` `unsafe` 白名单（M4/Task 31；`U-M4-06` 守卫）。
//!
//! # 判据
//!
//! `unsafe` 是**编译器伪包**（标准库没有 `src/unsafe`），所以：
//!
//! * **白名单**（`D-M4-8`）：只接 `Pointer` / `Sizeof` / `Alignof` /
//!   `Offsetof`。尺寸模型是 `StdSizes{WordSize: 8, MaxAlign: 8}`（首发平台
//!   Linux/arm64）。
//! * **白名单外显式拒绝**：`Add` / `Slice` / `SliceData` / `String` /
//!   `StringData` 一律报 `CHK-UNSUPPORTED-UNSAFE`（类别 `unsafe`）。
//!   `U-M4-06` 的纪律是「白名单项**不得**记为 pass，每项要有正向断言证明它
//!   确实被显式拒绝」—— 静默接受是更糟的通过。
//! * **装载诊断豁免**：`import "unsafe"` 的 `TypUnsupportedImportForm` 在
//!   检查面不再转述（装载面本身不改）。
//!
//! # 为什么拒绝面没有 oracle 行
//!
//! 官方**全部接受**这些成员（go1.27 的 `unsafe` 有 `Add`/`Slice`/…），所以
//! 拒绝面是本仓的**能力边界**，不是与官方对撞的面。这里的期望值是**我方**
//! 的落点，逐条由本测试钉住；`M4-tests.md` §7.2 要求的就是这种正向断言。
//!
//! # 尺寸面的可观测性（为什么接受面能钉住「值算出来了」）
//!
//! `const sz = unsafe.Sizeof(s)` 走 Task 30 的折叠预演：只有 `size_of`
//! **真的算出数**（`Const::Int`，不是 `Const::Unknown`）时，装载面那条
//! `TypInvalidConstInit` 才会被过滤。所以「接受面零诊断」等价于「尺寸算对了」——
//! 算不出来这条会红。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_lex::DiagCode;
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 白名单接受面（零诊断守卫）。
const ACCEPT_SRC: &str = r#"// T-CK-44 unsafe 白名单 —— 接受夹具（Task 31）。
package p

import "unsafe"

type S struct {
	a int32
	b int64
	c [3]int32
}

var s S
var p unsafe.Pointer
var q *S

const sz = unsafe.Sizeof(s)
const al = unsafe.Alignof(s)
const of = unsafe.Offsetof(s.b)
const sz2 = unsafe.Sizeof(q)
const sz3 = unsafe.Sizeof(p)

var _ = unsafe.Sizeof(s)
var _ = unsafe.Alignof(int64(0))
var _ = unsafe.Offsetof(s.c)
"#;

/// 白名单**外**的拒绝面（5 个成员各一条）。
const REJECT_SRC: &str = r#"// T-CK-44 unsafe 白名单 —— 拒绝夹具（Task 31）。
package p

import "unsafe"

var p unsafe.Pointer

var _ = unsafe.Add(p, 1)
var _ = unsafe.Slice(p, 1)
var _ = unsafe.String(nil, 1)
var _ = unsafe.SliceData(p)
var _ = unsafe.StringData("x")
"#;

/// 未 `import "unsafe"` 时**不**走白名单分派（`unsafe` 是普通未声明名字）。
const NO_IMPORT_SRC: &str = r#"// T-CK-44 unsafe 白名单 —— 未导入夹具（Task 31）。
package p

var _ = unsafe.Sizeof(1)
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

/// 白名单四个成员（含尺寸 / 偏移的常量折叠）⇒ 零诊断。
#[test]
fn t_ck_44_白名单接受_零诊断() {
    let (mut tt, pkg, bag) = load(ACCEPT_SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert!(
        checked.diags.is_empty(),
        "白名单面必须零诊断，got {:?}",
        checked
            .diags
            .iter()
            .map(|r| (r.diag.pos.line, r.diag.pos.col, r.diag.code))
            .collect::<Vec<_>>()
    );
    assert_eq!(checked.rendered_count(), 0);
}

/// 白名单外 5 个成员 ⇒ 各一条 `unsafe` 类诊断，报在 `.` 后的名字位。
#[test]
fn t_ck_44_白名单外显式拒绝() {
    let (mut tt, pkg, bag) = load(REJECT_SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(
        rows(&checked.render("t.go")),
        vec![
            "8:16\tunsafe",
            "9:16\tunsafe",
            "10:16\tunsafe",
            "11:16\tunsafe",
            "12:16\tunsafe",
        ],
        "白名单外成员的落点（`U-M4-06` 的正向断言）"
    );
    assert!(
        checked
            .diags
            .iter()
            .all(|r| r.diag.code == DiagCode::ChkUnsupportedUnsafe),
        "全部必须是 `ChkUnsupportedUnsafe`，got {:?}",
        checked
            .diags
            .iter()
            .map(|r| r.diag.code)
            .collect::<Vec<_>>()
    );
    // 消息要**点名**成员，否则「unsafe 类报错」分不清是哪一个。
    for (d, name) in checked
        .diags
        .iter()
        .zip(["Add", "Slice", "String", "SliceData", "StringData"])
    {
        assert!(
            d.diag.message.contains(&format!("unsafe.{name}")),
            "消息应点名 unsafe.{name}：{}",
            d.diag.message
        );
    }
}

/// 没 `import "unsafe"` ⇒ 不走白名单分派（`unsafe` 只是未声明的名字，
/// 不报 `unsafe` 类 —— 那种诊断会把「忘了 import」误诊成「能力不足」）。
#[test]
fn t_ck_44_未导入时不走白名单() {
    let (mut tt, pkg, bag) = load(NO_IMPORT_SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert!(
        checked
            .diags
            .iter()
            .all(|r| r.diag.code != DiagCode::ChkUnsupportedUnsafe),
        "未导入时不得报 `unsafe` 类，got {:?}",
        checked
            .diags
            .iter()
            .map(|r| (r.diag.pos.line, r.diag.code))
            .collect::<Vec<_>>()
    );
}

/// `unsafe.Pointer` 是**类型名**：能作 `var` 的声明类型、能进复合类型。
#[test]
fn t_ck_44_pointer_是类型名() {
    const SRC: &str = r#"package p

import "unsafe"

var p unsafe.Pointer
var pp *unsafe.Pointer
var s []unsafe.Pointer
var m map[string]unsafe.Pointer
var _ = p
var _ = pp
var _ = s
var _ = m
"#;
    let (mut tt, pkg, bag) = load(SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert!(
        checked.diags.is_empty(),
        "`unsafe.Pointer` 必须能当类型名用，got {:?}",
        checked
            .diags
            .iter()
            .map(|r| (r.diag.pos.line, r.diag.pos.col, r.diag.code))
            .collect::<Vec<_>>()
    );
}
