//! `T-CK-34` 选择器（M4/Task 24 后半）：字段 / 方法 / 提升 / 歧义 /
//! 方法值 / 方法表达式 / 指针自动解引用（含 mapindex 不可寻址面）。
//!
//! # 夹具与 oracle 证据
//!
//! 两个夹具与 `.superpowers/sdd/M4-plan/probe/task24-{reject,accept}.go`
//! 逐字节一致。oracle 实测（`go/types`，`checkmsg`）：
//!
//! - 拒绝夹具：**7 行**（见 [`ORACLE_REJECT_ROWS`] —— 原样输出，位+类别）；
//! - 接受夹具：**0 行**（整文件零诊断 —— 假阳性守卫，含提升 / 方法值 /
//!   方法表达式 / `(*T).P` / 指针嵌入字段 / 接口方法）。
//!
//! # 判据（官方 `lookup.go::lookupFieldOrMethod` + `call.go::selector`）
//!
//! - **深度最小者胜**：逐深度 BFS；同级找到多个 ⇒ `ambiguous selector`；
//!   具名类型在更浅深度出现过 ⇒ 深层同名忽略（`seen`）；
//! - **指针自动解引用**：`x.f`（x 是 `*T`）与 `s.P`（s 可寻址、P 指针接收者）
//!   合法；不可寻址（如 mapindex）遇指针接收者 ⇒ `cannot call pointer method`；
//! - **方法值 / 方法表达式**：`x.M` ⇒ 签名去接收者；`T.M` ⇒ 接收者作首参；
//!   `T.P`（值类型选指针接收者方法）⇒ `invalid method expression`；
//! - **typexpr 选字段** ⇒ `operand for field selector`（报在 `X` 位）。
//!
//! # 码面（C8 类别）
//!
//! 新增合并码 `CHK-INVALID-SELECTOR`（类别 `selector`；官方
//! `MissingFieldOrMethod` / `AmbiguousSelector` / `InvalidMethodExpr` /
//! `NonFieldSelector` 四条消息合一；`M4-tests.md` `R-4-5`，码表 74→75）。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_lex::DiagCode;
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 拒绝夹具（与 probe 副本逐字节一致）。
const REJECT_SRC: &str = r#"// T-CK-34 选择器面 —— 拒绝夹具（Task 24）。
// 与 `rgoc/crates/rgoc-check/tests/selection.rs` 的 REJECT_SRC 逐字节一致。
package p

type A struct{ X int }
type B struct{ X int }
type C struct {
	A
	B
}

type T struct{ N int }

func (T) V()  {}
func (*T) P() {}

var c C
var s T
var mm map[string]T
var x int

var _ = c.X
var _ = s.Nope
var _ = x.NoField
var _ = T.P
var _ = mm["k"].P
var _ = T.N
var _ = int.Nope
"#;

/// 接受夹具（与 probe 副本逐字节一致）。
const ACCEPT_SRC: &str = r#"// T-CK-34 选择器面 —— 接受夹具（Task 24）。
// 与 `rgoc/crates/rgoc-check/tests/selection.rs` 的 ACCEPT_SRC 逐字节一致。
package p

type E struct{ F int }
type M struct{ E }
type Deep struct{ M }

type T struct{ N int }

func (T) V()  {}
func (*T) P() {}

type PE struct{ G int }
type WithPtr struct{ *PE }

type EA struct{}

func (EA) MA()   {}
func (*EA) MPa() {}
type EB struct{ EA }

type I interface{ IM() }

type A struct{ X int }
type C struct{ A }

var c C
var s T
var mm map[string]T
var d Deep
var wp WithPtr
var eb EB
var ic I

var _ = s.N
var _ = s.V
var _ = s.P
var _ = T.V
var _ = (*T).P
var _ = (*T).V
var _ = d.F
var _ = d.M.E.F
var _ = wp.G
var _ = eb.MA
var _ = eb.MPa
var _ = ic.IM
var _ = c.A
var _ = mm["k"].V
"#;

/// `(行, 列, 码)` —— 本仓划分（oracle 的 7 行逐条换成码）。
const EXPECT_REJECT: [(u32, u32, DiagCode); 7] = [
    (22, 11, DiagCode::ChkInvalidSelector),
    (23, 11, DiagCode::ChkInvalidSelector),
    (24, 11, DiagCode::ChkInvalidSelector),
    (25, 11, DiagCode::ChkInvalidSelector),
    (26, 17, DiagCode::ChkInvalidSelector),
    (27, 9, DiagCode::ChkInvalidSelector),
    (28, 13, DiagCode::ChkInvalidSelector),
];

/// oracle 原样输出（C8 渲染面：`行:列<TAB>类别`）。
const ORACLE_REJECT_ROWS: [&str; 7] = [
    "22:11\tselector",
    "23:11\tselector",
    "24:11\tselector",
    "25:11\tselector",
    "26:17\tselector",
    "27:9\tselector",
    "28:13\tselector",
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
fn t_ck_34_选择器_拒绝向量_码与位置逐条一致() {
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
fn t_ck_34_选择器_接受向量_零诊断() {
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
