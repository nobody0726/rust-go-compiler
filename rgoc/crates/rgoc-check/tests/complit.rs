//! `T-CK-37` 复合字面量静态面（M4/Task 27）：`compositeLit` +
//! `indexedElts`（官方 `go/types/literals.go:111` / `:349`）的形态拒绝矩阵
//! 与 1.27 的 promoted field key 门控。
//!
//! # 夹具与 oracle 证据
//!
//! 两个夹具与 `.superpowers/sdd/M4-plan/probe/task27-{reject,accept}.go`
//! **逐字节一致**（`t27-scratch.go` 先行验证过 `f0({1})` 两边都当**语法**
//! 错误 —— 拒绝夹具不含裸 `{}` 实参）。oracle 实测（`tools/types-oracle
//! --check`）：
//!
//! - 拒绝夹具：**22 行**（位 + 类别如 [`ORACLE_REJECT_ROWS`]）；
//! - 接受夹具：**0 行**（假阳性守卫）；
//! - 门控夹具（`//go:build go1.26`）：**1 行** `16:11 version`。
//!
//! # 类别口径（C8 适配表）
//!
//! | 官方消息家族 | 类别 | 本仓码 |
//! |---|---|---|
//! | `missing type` / `invalid composite literal type` / `missing key in map literal` / `mixture of field` | decl | `ChkInvalidLit` |
//! | `duplicate field name` / `duplicate index` / `duplicate key` | decl | `ChkDuplicateLitField` / `ChkDuplicateLitIndex` |
//! | `invalid field name` / `unknown field` / `cannot specify (promoted|embedded) field` | selector | `ChkInvalidLitField` |
//! | `too many|too few values in struct literal` | assign | `ChkStructLitCount` |
//! | `index %s must be integer constant` / `index %d is out of bounds (>= %d)` | index | `ChkInvalidLitIndex` |
//! | `index %d out of bounds [0:%d]` / `index %s must be integer` | index | `ChkInvalidIndex`（Task 23 既有） |
//! | cannot-use（结构 / map / 数组切片语境） | assign | `ChkInvalidAssign`（Task 25 既有） |
//! | promoted field key 的 `requires go1.27 or later` | version | `TypFeatureRequiresGoVersion`（`Feat::PromotedFieldKey`） |
//!
//! # 报点（官方 `literals.go` 逐条 + oracle 实测）
//!
//! - **too few** 报在 `}`（Rbrace）—— `inNode(e, e.Rbrace)`；
//! - **too many** 报在多出来的**值**上（`errorf(x, …)`，`x` 是该值操作数）；
//! - KV 的 `invalid field name` / 重复 / 门控报在**键**（`kv.Key` 位）；
//! - KV 的 mixture 报在**键值对自身**（`kv` 位 = 键起点）；值元的 mixture
//!   报在**普通元素**上；
//! - 下标类（`must be integer` / `[0:n]` 越界）报在**下标表达式**位；
//!   位置越界（`>= n`）报在**该元素**位。
//!
//! # 刻意不测（Ruling 27-x，台账登记）
//!
//! - 同一官方码 `UnexportedLitField`（implicit assignment to unexported
//!   field）：只在 `fld.pkg != check.pkg` 时触发 —— 本检查面是**单包**
//!   （官方 `lookupFieldOrMethod` 的 `pkg` 维度同样缺席，见 `lookup.rs`
//!   模块文档差异 1），单包内**不可达**；import 缝合进来自 Task 30+ 再收。
//! - 泛型实例化字面量面（`slice[int]{1, 2, 3}` / 类型参数 `S{}`）：语料
//!   `check/compliterals.go` 在 oracle 上 **0 行** —— 检查面对未接线形态
//!   **静默 `invalid`** 即可同验；完整接线归 Task 42（CH-9）收口。
//! - interface 键类型的 map 重复键（`keyIsInterface` 的类型敏感去重）：窄面
//!   用常量值单一维度去重；interface 键在能力边界不入夹具。
//! - `f0({1})` 的 missing type 面：两边都是**语法**错误
//!   （`t27-scratch.go` oracle 与本仓语法器同拒实测），不属本任务。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_lex::DiagCode;
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 拒绝夹具（与 `probe/task27-reject.go` 逐字节一致）。
const REJECT_SRC: &str = r#"// T-CK-37 复合字面量静态面 —— 拒绝夹具（Task 27）。
// 与 `rgoc/crates/rgoc-check/tests/complit.rs` 的 `REJECT_SRC` 逐字节一致。
package p

type S struct {
	a int
	b string
}

type N struct {
	S
	c int
}

type A [3]int
type M map[int]string
type T []int

var badKey = "a"

var _ = S{a: 1, 2}
var _ = S{1, "x": "y"}
var _ = S{1: 0}
var _ = S{z: 0}
var _ = S{a: 1, a: 2}
var _ = N{S: S{}, a: 1}
var _ = S{1, "x", 2}
var _ = S{1}
var _ = S{"x", 1}
var _ = A{1, 2, 3, 4}
var _ = A{5: 1}
var _ = A{badKey: 1}
var _ = T{1: 0, 1: 1}
var _ = T{"x"}
var _ = M{1: "x", 1: "y"}
var _ = M{"a"}
var _ = M{badKey: "x"}
var _ = M{1: 2}
var _ = int{1}
var _ = []S{{1}, {2}}
"#;

/// 接受夹具（与 `probe/task27-accept.go` 逐字节一致）。
const ACCEPT_SRC: &str = r#"// T-CK-37 复合字面量静态面 —— 接受夹具（Task 27）。
// 与 `rgoc/crates/rgoc-check/tests/complit.rs` 的 `ACCEPT_SRC` 逐字节一致。
// 无版本旗标 ⇒ `Feat::PromotedFieldKey`（1.27）不门控。
package p

type S struct {
	a int
	b string
}

type N struct {
	S
	c int
}

type A [3]int
type M map[int]string
type T []int

var _ = S{}
var _ = S{1, "x"}
var _ = S{a: 1, b: "x"}
var _ = S{b: "x", a: 1}
var _ = N{S: S{a: 1, b: "x"}, c: 2}
var _ = N{a: 1, b: "x", c: 2}
var _ = A{1, 2, 3}
var _ = A{0: 1, 2: 3}
var _ = A{1: 2}
var _ = A{}
var _ = M{}
var _ = M{1: "x", 2: "y"}
var _ = T{1, 2, 3}
var _ = T{0: 1, 2: 3}
var _ = []S{{1, "x"}, {2, "y"}}
var _ = []S{{a: 1}, {b: "x"}}
var _ = []A{{1, 2, 3}}
var _ = []T{{1}, {2}}
var _ = map[string]S{"k": {1, "x"}}
var _ = map[int]M{1: {2: "x"}}
var _ = map[bool]int{true: 1, false: 0}
var _ = [...]int{1, 2, 3}
var _ = [...]A{{1, 2, 3}}
var _ = [2]S{{}, {1, "x"}}
var _ = []*S{nil, {1, "x"}}
"#;

/// oracle 实测渲染行（`--check` 原样；按位置稳定排序）。
const ORACLE_REJECT_ROWS: [&str; 22] = [
    "21:17\tdecl",
    "22:14\tdecl",
    "23:11\tselector",
    "24:11\tselector",
    "25:17\tdecl",
    "26:19\tselector",
    "27:19\tassign",
    "28:12\tassign",
    "29:11\tassign",
    "29:16\tassign",
    "30:20\tindex",
    "31:11\tindex",
    "32:11\tindex",
    "33:17\tdecl",
    "34:11\tassign",
    "35:19\tdecl",
    "36:11\tdecl",
    "37:11\tassign",
    "38:14\tassign",
    "39:9\tdecl",
    "40:15\tassign",
    "40:20\tassign",
];

/// 我方 `(行,列,码)` 期望表 —— 码是本仓的划分，位置必须与 oracle 一致。
const EXPECT_REJECT: [(u32, u32, DiagCode); 22] = [
    (21, 17, DiagCode::ChkInvalidLit),
    (22, 14, DiagCode::ChkInvalidLit),
    (23, 11, DiagCode::ChkInvalidLitField),
    (24, 11, DiagCode::ChkInvalidLitField),
    (25, 17, DiagCode::ChkDuplicateLitField),
    (26, 19, DiagCode::ChkInvalidLitField),
    (27, 19, DiagCode::ChkStructLitCount),
    (28, 12, DiagCode::ChkStructLitCount),
    (29, 11, DiagCode::ChkInvalidAssign),
    (29, 16, DiagCode::ChkInvalidAssign),
    (30, 20, DiagCode::ChkInvalidLitIndex),
    (31, 11, DiagCode::ChkInvalidIndex),
    (32, 11, DiagCode::ChkInvalidIndex),
    (33, 17, DiagCode::ChkDuplicateLitIndex),
    (34, 11, DiagCode::ChkInvalidAssign),
    (35, 19, DiagCode::ChkDuplicateLitIndex),
    (36, 11, DiagCode::ChkInvalidLit),
    (37, 11, DiagCode::ChkInvalidAssign),
    (38, 14, DiagCode::ChkInvalidAssign),
    (39, 9, DiagCode::ChkInvalidLit),
    (40, 15, DiagCode::ChkStructLitCount),
    (40, 20, DiagCode::ChkStructLitCount),
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
fn t_ck_37_拒绝向量_码与位置逐条一致() {
    let (mut tt, pkg, bag) = load(REJECT_SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());

    // ① 码 + 位置。流序可能是「形状面」序，两侧都按 (行,列,码) 排序后再比
    //    —— 比的是**集合**不是流序。
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

    // ② 渲染行 == oracle 原样输出（位置 + 类别逐字节）。
    let rendered_text = checked.render("t.go");
    let rendered: Vec<&str> = rows(&rendered_text);
    assert_eq!(rendered, ORACLE_REJECT_ROWS.to_vec(), "渲染行 vs oracle");
}

/// 接受向量：整文件零诊断（假阳性守卫；oracle 实测同样 0 行）。
#[test]
fn t_ck_37_接受向量_零诊断() {
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

/// promoted field key 的 1.27 门控：`//go:build go1.26` 文件里
/// `N{a: 1}` 被拒（`use of promoted field S.a in struct literal of type
/// p.N requires go1.27 or later`），落在**键**、类别 `version`；同文件
/// `N{c: 2}`（非提升字段）不报。
#[test]
fn t_ck_37_版本门控_promoted_key_go1_26() {
    const GATE_SRC: &str = "//go:build go1.26\n\n// T-CK-37 复合字面量静态面 —— promoted field key 的 1.27 门控（Task 27）。\n// 与 `rgoc/crates/rgoc-check/tests/complit.rs` 的 `GATE_SRC` 逐字节一致。\npackage p\n\ntype S struct {\n\ta int\n}\n\ntype N struct {\n\tS\n\tc int\n}\n\nvar _ = N{a: 1}\nvar _ = N{c: 2}\n";
    let (mut tt, pkg, bag) = load(GATE_SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(
        rows(&checked.render("t.go")),
        vec!["16:11\tversion"],
        "门控落点与类别（`probe/task27-gate.go` oracle 实测 1 行）"
    );
}

/// 同门控的放行面：无版本旗标（= 不门控）时 promoted key 接受 ——
/// `N{a: 1}` 进接受夹具（`N{a: 1, b: "x", c: 2}` 第 25 行）已覆盖；
/// 这里再钉一次单行的最小变体，免得受夹具其余行影响。
#[test]
fn t_ck_37_版本门控_无旗标放行() {
    const SRC: &str = "package p\n\ntype S struct {\n\ta int\n}\n\ntype N struct {\n\tS\n\tc int\n}\n\nvar _ = N{a: 1}\n";
    let (mut tt, pkg, bag) = load(SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert!(checked.diags.is_empty());
}
