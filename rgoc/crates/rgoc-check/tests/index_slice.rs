//! `T-CK-34` 索引与切片（M4/Task 23 前半）：包级 `var` 初始化式里的
//! `x[i]` / `x[a:b]` / `x[a:b:c]` —— 数组 / 切片 / 字符串 / map /
//! 指向数组的指针；常量索引的越界 / 负数 / 整数性；切片三下标与次序。
//!
//! # 夹具与 oracle 证据
//!
//! 两个夹具与 `.superpowers/sdd/M4-plan/probe/task23-{reject,accept}.go`
//! 逐字节一致。oracle 实测（`go/types`，`checkmsg`）：
//!
//! - 拒绝夹具：**19 行**（见 [`ORACLE_REJECT_ROWS`] —— 原样输出，位+类别）；
//! - 接受夹具：**0 行**（整个文件零诊断 —— 假阳性守卫）。
//!
//! # 码面（C8 类别）
//!
//! 新增两个合并码（`M4-tests.md` `R-4-4`，码表 72→74）：
//!
//! - `CHK-INVALID-INDEX`（类别 `index`）：官方 `NonIndexableOperand` 与
//!   `InvalidIndex` 三条（`must be integer` / `must not be negative` /
//!   `out of bounds [0:n]`）；
//! - `CHK-INVALID-SLICE`（类别 `slice`）：官方 `NonSliceableOperand`（`cannot
//!   slice`）/ `InvalidSliceExpr`（`3-index slice of string`）/
//!   `SwappedSliceIndices`（`invalid slice indices: y < x`）。
//!
//! 索引操作数的**无类型常量转换**复用 Task 21 的 `convert_untyped`：
//! `a[1.5]` / `a[1e100]` ⇒ `truncated to int`（`convert` 类），
//! `sl["x"]` ⇒ `cannot convert ... to type int`（`convert` 类）；
//! 类型化常量超 `int` 范围（`a[u64]`，u64 = 1<<63）⇒ `overflows int`
//! （**`overflow`** 类 —— 官方消息含 `overflows`，c8Rules 的 index 规则
//! 不匹配、通用 overflow 规则接管；探针实证）。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_lex::DiagCode;
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 拒绝夹具（与 probe 副本逐字节一致）。
const REJECT_SRC: &str = r#"// T-CK-34 索引与切片面 —— 拒绝夹具（Task 23）。
// 与 `rgoc/crates/rgoc-check/tests/index_slice.rs` 的 REJECT_SRC 逐字节一致。
package p

var a [3]int
var sl []int
var m map[string]int
var s string
var x int
var fl float64
var pa *[3]int
var p *int

const u64 uint64 = 1 << 63

var _ = a[3]
var _ = a[-1]
var _ = a[1.5]
var _ = a[1e100]
var _ = a[u64]
var _ = x[0]
var _ = "abc"[5]
var _ = pa[3]
var _ = sl["x"]
var _ = sl[fl]
var _ = sl[2:1]
var _ = s[1:2:3]
var _ = 1[0:1]
var _ = m[1:2]
var _ = a[:5]
var _ = "abc"[1:5]
var _ = sl[1:2:1]
var _ = a[2:1]
var _ = p[0]
"#;

/// 接受夹具（与 probe 副本逐字节一致）。
const ACCEPT_SRC: &str = r#"// T-CK-34 索引与切片面 —— 接受夹具（Task 23）。
// 与 `rgoc/crates/rgoc-check/tests/index_slice.rs` 的 ACCEPT_SRC 逐字节一致。
package p

var a [3]int
var sl []int
var m map[string]int
var s string
var pa *[3]int
var p *int

var _ = sl[0]
var _ = a[1.0]
var _ = a[a[0]]
var _ = s[2]
var _ = "abc"[1]
var _ = m["x"]
var _ = pa[1]
var _ = sl[1:2]
var _ = sl[1:2:3]
var _ = s[1:2]
var _ = "abc"[1:2]
var _ = "abc"[1:]
var _ = a[0:3]
var _ = pa[1:2]
var _ = a[:]
var _ = sl[:2]
var _ = p
"#;

/// `(行, 列, 码)` —— 本仓划分（oracle 的 19 行逐条换成码）。
const EXPECT_REJECT: [(u32, u32, DiagCode); 19] = [
    (16, 11, DiagCode::ChkInvalidIndex),
    (17, 11, DiagCode::ChkInvalidIndex),
    (18, 11, DiagCode::TypInvalidConversion),
    (19, 11, DiagCode::TypInvalidConversion),
    (20, 11, DiagCode::TypNumericOverflow),
    (21, 9, DiagCode::ChkInvalidIndex),
    (22, 15, DiagCode::ChkInvalidIndex),
    (23, 12, DiagCode::ChkInvalidIndex),
    (24, 12, DiagCode::TypInvalidConversion),
    (25, 12, DiagCode::ChkInvalidIndex),
    (26, 14, DiagCode::ChkInvalidSlice),
    (27, 15, DiagCode::ChkInvalidSlice),
    (28, 9, DiagCode::ChkInvalidSlice),
    (29, 9, DiagCode::ChkInvalidSlice),
    (30, 12, DiagCode::ChkInvalidIndex),
    (31, 17, DiagCode::ChkInvalidIndex),
    (32, 16, DiagCode::ChkInvalidSlice),
    (33, 13, DiagCode::ChkInvalidSlice),
    (34, 9, DiagCode::ChkInvalidIndex),
];

/// oracle 原样输出（C8 渲染面：`行:列<TAB>类别`）。
const ORACLE_REJECT_ROWS: [&str; 19] = [
    "16:11\tindex",
    "17:11\tindex",
    "18:11\tconvert",
    "19:11\tconvert",
    "20:11\toverflow",
    "21:9\tindex",
    "22:15\tindex",
    "23:12\tindex",
    "24:12\tconvert",
    "25:12\tindex",
    "26:14\tslice",
    "27:15\tslice",
    "28:9\tslice",
    "29:9\tslice",
    "30:12\tindex",
    "31:17\tindex",
    "32:16\tslice",
    "33:13\tslice",
    "34:9\tindex",
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
fn t_ck_34_索引切片_拒绝向量_码与位置逐条一致() {
    let (mut tt, pkg, bag) = load(REJECT_SRC);
    assert!(
        bag.is_empty(),
        "拒绝夹具不得有装载诊断（有则证明装载面漏报，先查装载）"
    );
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());

    // ① 码 + 位置（本仓划分）。
    let got: Vec<(u32, u32, DiagCode)> = checked
        .diags
        .iter()
        .map(|r| (r.diag.pos.line, r.diag.pos.col, r.diag.code))
        .collect();
    assert_eq!(got.len(), EXPECT_REJECT.len(), "诊断条数：got {got:?}");
    assert_eq!(got, EXPECT_REJECT.to_vec(), "逐条 (行,列,码)");

    // ② 渲染行 == oracle 原样输出（位置 + 类别逐字节）。
    let rendered_text = checked.render("t.go");
    let rendered: Vec<&str> = rows(&rendered_text);
    assert_eq!(rendered, ORACLE_REJECT_ROWS.to_vec(), "渲染行 vs oracle");
}

/// 接受向量：整文件零诊断（假阳性守卫；oracle 实测同样 0 行）。
#[test]
fn t_ck_34_索引切片_接受向量_零诊断() {
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
