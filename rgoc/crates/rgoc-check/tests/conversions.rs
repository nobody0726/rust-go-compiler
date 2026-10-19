//! `T-CK-33` 转换与可转换性（M4/Task 22）：包级 `var` 初始化式里的转换
//! 表达式——常量边界（溢出 / 截断 / 整数→字符串码点）与非恒量转换
//! （`string([]byte)`、`T(*U)`、`P(nil)`、命名类型）。
//!
//! # 夹具与 oracle 证据
//!
//! 两个夹具与 `.superpowers/sdd/M4-plan/probe/task22-{reject,accept}.go`
//! 逐字节一致。oracle 实测（`go/types`，`checkmsg`）：
//!
//! - 拒绝夹具：**9 行**（见 [`ORACLE_REJECT_ROWS`] —— 原样输出，位+类别）；
//! - 接受夹具：**0 行**（整个文件零诊断 —— 假阳性守卫，含 `[]byte("x")` 这类
//!   类型族函数位 —— 我方 Task 22 窄面**不解析**类型族转换，但也不得报）。
//!
//! 断言分两层：① 我方 `(行,列,码)` 与手写期望表逐条相同（码是本仓的划分）；
//! ② 我方渲染行与 oracle 行**逐字节**相同（位置 + 类别对齐）。
//!
//! # 码面（C8 类别）
//!
//! 只有两个 M3 已有码参与：常量整数互转失败 ⇒ `TYP-NUMERIC-OVERFLOW`
//! （oracle 消息含 `overflows`，类别 `overflow`）；其余转换失败 ⇒
//! `TYP-INVALID-CONVERSION`（oracle 消息以 `cannot convert ` 开头，类别
//! `convert`）。`T-CK-33` **不新增码**。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_lex::DiagCode;
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 拒绝夹具（与 probe 副本逐字节一致）。
const REJECT_SRC: &str = r#"// T-CK-33 转换面 —— 拒绝夹具（Task 22）。
// 与 `rgoc/crates/rgoc-check/tests/conversions.rs` 的 REJECT_SRC 逐字节一致。
package p

type MyInt int
type S string

var p *int

const ci8 int8 = -128

var _ = int8(300)
var _ = uint8(-1)
var _ = int8(1.5)
var _ = int8(200.0)
var _ = int(1e100)
var _ = string(1.5)
var _ = int8("x")
var _ = int(p)
var _ = uint8(ci8)
"#;

/// 接受夹具（与 probe 副本逐字节一致）。
const ACCEPT_SRC: &str = r#"// T-CK-33 转换面 —— 接受夹具（Task 22）。
// 与 `rgoc/crates/rgoc-check/tests/conversions.rs` 的 ACCEPT_SRC 逐字节一致。
package p

type MyInt int
type S string
type B []byte
type P *int

var bs []byte
var rs []rune
var p *int
var f float64

const ci8 int8 = -128

var _ = int(1)
var _ = int(1.0)
var _ = float64(1)
var _ = float64(1e100)
var _ = string(65)
var _ = string(-1)
var _ = string(bs)
var _ = string(rs)
var _ = S(65)
var _ = B("x")
var _ = []byte("x")
var _ = MyInt(1)
var _ = P(nil)
var _ = any(1)
var _ = float64(ci8)
var _ = complex64(1)
var _ = int8(127 + ci8)
var _ = f
"#;

/// `(行, 列, 码)` —— 本仓划分（oracle 的 9 行逐条换成码）。
const EXPECT_REJECT: [(u32, u32, DiagCode); 9] = [
    (12, 14, DiagCode::TypNumericOverflow),
    (13, 15, DiagCode::TypNumericOverflow),
    (14, 14, DiagCode::TypInvalidConversion),
    (15, 14, DiagCode::TypInvalidConversion),
    (16, 13, DiagCode::TypInvalidConversion),
    (17, 16, DiagCode::TypInvalidConversion),
    (18, 14, DiagCode::TypInvalidConversion),
    (19, 13, DiagCode::TypInvalidConversion),
    (20, 15, DiagCode::TypNumericOverflow),
];

/// oracle 原样输出（C8 渲染面：`行:列<TAB>类别`）。
const ORACLE_REJECT_ROWS: [&str; 9] = [
    "12:14\toverflow",
    "13:15\toverflow",
    "14:14\tconvert",
    "15:14\tconvert",
    "16:13\tconvert",
    "17:16\tconvert",
    "18:14\tconvert",
    "19:13\tconvert",
    "20:15\toverflow",
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
fn t_ck_33_拒绝向量_码与位置逐条一致() {
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
fn t_ck_33_接受向量_零诊断() {
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
