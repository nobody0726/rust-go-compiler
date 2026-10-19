//! `T-CK-32` 表达式运算符面（M4/Task 21）：拒绝向量的（行,列,码）逐条对账 +
//! oracle 类别对撞，接受向量零诊断。
//!
//! # 夹具与 oracle 证据
//!
//! 两个夹具与 `.superpowers/sdd/M4-plan/probe/task21-{reject,accept}.go`
//! 逐字节一致。oracle 实测（`tools/types-oracle --check`）：
//!
//! - 拒绝夹具：**53 行**（见 [`ORACLE_REJECT_ROWS`] —— 原样输出，位+类别）；
//! - 接受夹具：**0 行**（整个文件零诊断 —— 假阳性守卫）。
//!
//! 断言分两层：① 我方 `(行,列,码)` 与手写期望表逐条相同（码是**本仓**的
//! 划分）；② 我方渲染行与 oracle 行**逐字节**相同（证明「位置 + 类别」对齐，
//! 而不是码表自洽）。`ChkInvalidOperation` 是唯一新增码；其余拒绝复用 M3
//! 已有码（`Typ{DivByZero,InvalidShiftCount,InvalidShiftOperand,InvalidConversion,
//! NumericOverflow}`）—— 探针证明它们的类别/位置与官方一致。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_lex::DiagCode;
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 拒绝夹具（与 probe 副本逐字节一致）。
const REJECT_SRC: &str = r#"// T-CK-32 表达式运算符面 —— 拒绝夹具（Task 21）。
// 与 `rgoc/crates/rgoc-check/tests/expr_ops.rs` 的 REJECT_SRC 逐字节一致。
package p

type S string
type MyInt int

var a int
var a2 int32
var b string
var s S
var f float64
var c complex64
var mi MyInt
var u uint8
var m map[int]int
var sl []int
var p *int
var e error
var fn func()
var i interface{}
var ch chan int
var sch chan<- int

const ci8 = int8(3)
const cm = int8(-128)
const cu8 = uint8(1)
const cfl = float64(2)

var _ = b + a
var _ = b + 1
var _ = 1 + "a"
var _ = "x" - "y"
var _ = "x" + 1
var _ = nil + 1
var _ = u + 300
var _ = a / 0
var _ = 1.0 / 0.0
var _ = f % 2
var _ = 1.0 % 2
var _ = a & 1.5
var _ = a | f
var _ = a && true
var _ = mi + a
var _ = 1.5 | 2
var _ = true + false
var _ = cm - 1
var _ = -b
var _ = !a
var _ = ^f
var _ = ^1.5
var _ = *a
var _ = &1
var _ = <-a
var _ = <-sch
var _ = ci8 << 7
var _ = 1 << 512
var _ = 1 << -1
var _ = 1 << 1075
var _ = 1 << f
var _ = 1 << 1.5
var _ = f << 1
var _ = 1.5 << 2
var _ = m == m
var _ = sl == sl
var _ = fn == fn
var _ = nil == nil
var _ = a == nil
var _ = nil == 1
var _ = true < false
var _ = c < c
var _ = e < e
var _ = a < b
var _ = a2 == a
var _ = mi == a
var _ = a < p
var _ = p == m
var _ = 1 == "a"
var _ = u == 300
var _ = 300 == u
var _ = nil < nil
var _ = <-p
"#;

/// 接受夹具（与 probe 副本逐字节一致）。
const ACCEPT_SRC: &str = r#"// T-CK-32 表达式运算符面 —— 接受夹具（Task 21）。
// 与 `rgoc/crates/rgoc-check/tests/expr_ops.rs` 的 ACCEPT_SRC 逐字节一致。
package p

type S string
type MyInt int

var a int
var a2 int32
var b string
var s S
var f float64
var c complex64
var mi MyInt
var u uint8
var pp **int
var p *int
var e error
var i interface{}
var ch chan int
var fn func()
var sl []int
var m map[int]int

const ci8 = int8(3)
const cu8 = uint8(1)
const cfl = float64(2)

var _ = a + a
var _ = b + b
var _ = s + "x"
var _ = mi + mi
var _ = mi + 1
var _ = mi < mi
var _ = mi & 1
var _ = 'a' + 'b'
var _ = 'a' + 1.5
var _ = 1 + 2i
var _ = 1i == 2i
var _ = 'a' < 'b'
var _ = a == a
var _ = mi == 1
var _ = u < 1
var _ = u == 1
var _ = b == "x"
var _ = s < "x"
var _ = f < 1
var _ = c == c
var _ = c != 1i
var _ = p == p
var _ = p == nil
var _ = nil == p
var _ = e == e
var _ = i == i
var _ = ch == ch
var _ = ch != nil
var _ = fn == nil
var _ = sl == nil
var _ = m == nil
var _ = a << 300
var _ = u << 100
var _ = a << 1075
var _ = 2.0 << 1
var _ = 1 << cfl
var _ = 1 << 2.0
var _ = ^a
var _ = ^u
var _ = ^ci8
var _ = -a
var _ = -mi
var _ = +mi
var _ = !true
var _ = u - 1
var _ = mi - 1
var _ = **pp
var _ = *p + 1
var _ = &mi
var _ = <-ch
var _ = ci8 << 3
var _ = cu8 << 2
var _ = f / 0.0
var _ = a / 1
var _ = s + s
var _ = b < b
"#;

/// oracle 对拒绝夹具的**原样输出行**（`--check`，剥 `#` 头）。
const ORACLE_REJECT_ROWS: [&str; 53] = [
    "30:9\top-invalid",
    "31:9\top-invalid",
    "32:9\top-invalid",
    "33:9\top-invalid",
    "34:9\top-invalid",
    "35:9\top-invalid",
    "36:13\toverflow",
    "37:13\top-invalid",
    "38:15\top-invalid",
    "39:9\top-invalid",
    "40:9\top-invalid",
    "41:13\tconvert",
    "42:9\top-invalid",
    "43:9\top-invalid",
    "44:9\top-invalid",
    "45:9\top-invalid",
    "46:9\top-invalid",
    "47:9\toverflow",
    "48:10\top-invalid",
    "49:10\top-invalid",
    "50:10\top-invalid",
    "51:10\top-invalid",
    "52:10\top-invalid",
    "53:10\top-invalid",
    "54:11\top-invalid",
    "55:11\top-invalid",
    "56:9\toverflow",
    "57:11\tshift",
    "58:14\tshift",
    "59:14\tshift",
    "60:14\tshift",
    "61:14\tconvert",
    "62:9\tshift",
    "63:9\tshift",
    "64:9\top-invalid",
    "65:9\top-invalid",
    "66:9\top-invalid",
    "67:16\top-invalid",
    "68:14\top-invalid",
    "69:16\top-invalid",
    "70:9\top-invalid",
    "71:9\top-invalid",
    "72:9\top-invalid",
    "73:13\top-invalid",
    "74:15\top-invalid",
    "75:15\top-invalid",
    "76:13\top-invalid",
    "77:14\top-invalid",
    "78:14\top-invalid",
    "79:14\toverflow",
    "80:9\toverflow",
    "81:9\top-invalid",
    "82:11\top-invalid",
];

/// 手写期望表：`(行, 列, 码)`，与 [`REJECT_SRC`] 逐行对应（流内顺序 = 源序）。
///
/// 位置来源（探针钉死）：
/// - 一元拒绝 → **操作数**位；
/// - 二元不匹配 / 运算符未定义 / 除零以外的谓词拒绝 → **左操作数起点**；
/// - 比较不匹配 → **右操作数**位；
/// - `nil == nil` → 右侧 `nil`；`nil < nil` → 左侧起点；
/// - 无类型常量转换失败（溢出 / 截断）→ **该无类型操作数**位；
/// - 除零 → **除数**位；
/// - 移位计数类 → **计数**位；左操作数类 → **左操作数起点**；
/// - 有类型常量折叠溢出 → **左操作数起点**（= 表达式起点）；
/// - `1 << 512` 的 512 位增长守卫 → **操作符**位（`57:11`）。
const EXPECT_REJECT: [(u32, u32, DiagCode); 53] = [
    (30, 9, DiagCode::ChkInvalidOperation),
    (31, 9, DiagCode::ChkInvalidOperation),
    (32, 9, DiagCode::ChkInvalidOperation),
    (33, 9, DiagCode::ChkInvalidOperation),
    (34, 9, DiagCode::ChkInvalidOperation),
    (35, 9, DiagCode::ChkInvalidOperation),
    (36, 13, DiagCode::TypNumericOverflow),
    (37, 13, DiagCode::TypDivByZero),
    (38, 15, DiagCode::TypDivByZero),
    (39, 9, DiagCode::ChkInvalidOperation),
    (40, 9, DiagCode::ChkInvalidOperation),
    (41, 13, DiagCode::TypInvalidConversion),
    (42, 9, DiagCode::ChkInvalidOperation),
    (43, 9, DiagCode::ChkInvalidOperation),
    (44, 9, DiagCode::ChkInvalidOperation),
    (45, 9, DiagCode::ChkInvalidOperation),
    (46, 9, DiagCode::ChkInvalidOperation),
    (47, 9, DiagCode::TypNumericOverflow),
    (48, 10, DiagCode::ChkInvalidOperation),
    (49, 10, DiagCode::ChkInvalidOperation),
    (50, 10, DiagCode::ChkInvalidOperation),
    (51, 10, DiagCode::ChkInvalidOperation),
    (52, 10, DiagCode::ChkInvalidOperation),
    (53, 10, DiagCode::ChkInvalidOperation),
    (54, 11, DiagCode::ChkInvalidOperation),
    (55, 11, DiagCode::ChkInvalidOperation),
    (56, 9, DiagCode::TypNumericOverflow),
    (57, 11, DiagCode::TypInvalidShiftCount),
    (58, 14, DiagCode::TypInvalidShiftCount),
    (59, 14, DiagCode::TypInvalidShiftCount),
    (60, 14, DiagCode::TypInvalidShiftCount),
    (61, 14, DiagCode::TypInvalidConversion),
    (62, 9, DiagCode::TypInvalidShiftOperand),
    (63, 9, DiagCode::TypInvalidShiftOperand),
    (64, 9, DiagCode::ChkInvalidOperation),
    (65, 9, DiagCode::ChkInvalidOperation),
    (66, 9, DiagCode::ChkInvalidOperation),
    (67, 16, DiagCode::ChkInvalidOperation),
    (68, 14, DiagCode::ChkInvalidOperation),
    (69, 16, DiagCode::ChkInvalidOperation),
    (70, 9, DiagCode::ChkInvalidOperation),
    (71, 9, DiagCode::ChkInvalidOperation),
    (72, 9, DiagCode::ChkInvalidOperation),
    (73, 13, DiagCode::ChkInvalidOperation),
    (74, 15, DiagCode::ChkInvalidOperation),
    (75, 15, DiagCode::ChkInvalidOperation),
    (76, 13, DiagCode::ChkInvalidOperation),
    (77, 14, DiagCode::ChkInvalidOperation),
    (78, 14, DiagCode::ChkInvalidOperation),
    (79, 14, DiagCode::TypNumericOverflow),
    (80, 9, DiagCode::TypNumericOverflow),
    (81, 9, DiagCode::ChkInvalidOperation),
    (82, 11, DiagCode::ChkInvalidOperation),
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
fn t_ck_32_拒绝向量_码与位置逐条一致() {
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
fn t_ck_32_接受向量_零诊断() {
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
