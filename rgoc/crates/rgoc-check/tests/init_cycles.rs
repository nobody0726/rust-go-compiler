//! `T-CK-72` 初始化环诊断（M4/Task 33）。
//!
//! # 判据来源
//!
//! 官方 `go/types/initorder.go`：`findPath` 找环 → `reportCycle` 写消息。
//! 本仓的图在 `rgoc_check::initorder`（Task 32 的顺序面 + 本任务的环面）。
//!
//! # 四条实测语义
//!
//! 1. **报点** = 环上**第一个被弹出的对象**的名字位置（不是声明处、
//!    不是初始化式）。`var x6, x7 = f2()` 报在 `x7` —— 因为环是
//!    `x7 → f2 → f3 → x8 → x7`，`x6` 只是**依赖**环，不在环上。
//! 2. **每个环只报一次**：报完把前驱的 `ndeps` 减一，环上其余成员不再报。
//! 3. **消息带续行链**：`initialization cycle for X\n\tX refers to Y\n\t…`。
//!    渲染层只认主行（续行不以 `\t` 开头、也不含 `": \t"`）⇒ C8 一行。
//! 4. **自引用**用短文案：`initialization cycle: X refers to itself`。
//!
//! # 方法必须进图
//!
//! `check/init2.go` 的 26 个环里 24 个经过方法（方法表达式 / 方法值 /
//! 嵌入提升 / 变量接收者）。只认标识符的图一条边都补不上 ⇒ 26 行全漏
//! （`Ruling 33-2` 的实测记录）。
//!
//! # 常量环不重复报
//!
//! 常量环由**装载面**的常量求值报（`decls.rs::hit_cycle`）。检查面若也报，
//! 同一行会出两条（C8 保留重复行）⇒ `solve` 只对 `Kind::Var` 报环。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_check::initorder::{analyze, compute};
use rgoc_lex::DiagCode;
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 主夹具（与 `probe/t33/t33-cycles.go` **逐字节一致**）。
///
/// oracle `--check` 实测 7 行：`5:5` / `8:5` / `12:5` / `16:5` / `21:5` /
/// `26:5` / `28:5`，全 `cycle`。第 34 行的 `y7`（接口接收者）与第 36 行的
/// `z1`（只依赖环、不在环上）**不报**。
const SRC: &str = r#"// T-CK-72 初始化环 —— 夹具（Task 33）。
package p

func f1() int { _ = x1; return 0 }
var x1 = f1

func f2() int { _ = x2; return 0 }
var x2 = f2()

type T3 int
func (T3) m() int { _ = x3; return 0 }
var x3 = T3.m

type T4 int
func (T4) m() int { _ = x4; return 0 }
var x4 = T4.m(0)

type E5 int
func (E5) m() int { _ = x5; return 0 }
type T5 struct{ E5 }
var x5 = T5.m

type T6 int
func (T6) m() int { _ = x6; return 0 }
var x6var T6
var x6 = x6var.m

var s1 = s1

type T7 int
func (T7) m() int { _ = y7; return 0 }
var x7 interface{ m() int } = T7(0)
var y7 = x7.m()

var z1 = x1
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

/// 主夹具：7 行 `cycle`，与 oracle 逐行一致（含**不该报**的两行）。
#[test]
fn t_ck_72_环行向量与_oracle_逐行一致() {
    let (mut tt, pkg, bag) = load(SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(
        rows(&checked.render("t.go")),
        vec![
            "5:5\tcycle",
            "8:5\tcycle",
            "12:5\tcycle",
            "16:5\tcycle",
            "21:5\tcycle",
            "26:5\tcycle",
            "28:5\tcycle",
        ],
        "vs oracle（`probe/t33/t33-cycles.go` 实测）"
    );
    assert!(
        checked
            .diags
            .iter()
            .all(|r| r.diag.code == DiagCode::TypInvalidInitCycle),
        "全部应是 TYP-INVALID-INIT-CYCLE"
    );
}

/// 消息形状：主行 + `\n\t` 续行链（`reportCycle` 逐字）。
#[test]
fn t_ck_72_消息形状_主行加续行链() {
    const S: &str = "package p\n\nvar a = b\nvar b = a\n";
    let mut tt = TypeTable::new();
    let (pkg, _bag) = load_sources(&mut tt, &[("t.go", S.as_bytes())], None);
    let a = analyze(&pkg);
    assert_eq!(a.cycles.len(), 1, "一个环只报一条");
    assert_eq!(
        a.cycles[0].message,
        "initialization cycle for a\n\ta refers to b\n\tb refers to a"
    );
}

/// 自引用：短文案（官方 `initialization cycle: X refers to itself`）。
#[test]
fn t_ck_72_自引用短文案() {
    const S: &str = "package p\n\nvar s = s\n";
    let mut tt = TypeTable::new();
    let (pkg, _bag) = load_sources(&mut tt, &[("t.go", S.as_bytes())], None);
    let a = analyze(&pkg);
    assert_eq!(a.cycles.len(), 1);
    assert_eq!(
        a.cycles[0].message,
        "initialization cycle: s refers to itself"
    );
    assert_eq!(a.cycles[0].pos.line, 3);
}

/// 环外依赖不报：`z = x` 里 `x` 在环上，但 `z` 不在。
#[test]
fn t_ck_72_环外依赖不报() {
    const S: &str = "package p\n\nvar x = y\nvar y = x\nvar z = x\n";
    let mut tt = TypeTable::new();
    let (pkg, _bag) = load_sources(&mut tt, &[("t.go", S.as_bytes())], None);
    let a = analyze(&pkg);
    assert_eq!(a.cycles.len(), 1, "只报环上的那一条");
    assert_eq!(a.cycles[0].pos.line, 3, "报在 x（不是 z）");
}

/// 接口接收者**不补边**：`x.m()` 是动态派发，`m` 的体里用 `y` 不构成环。
#[test]
fn t_ck_72_接口接收者不构成环() {
    const S: &str = r#"package p

type T int

func (T) m() int { return y }

var i interface{ m() int } = T(0)
var y = i.m()
"#;
    let mut tt = TypeTable::new();
    let (pkg, _bag) = load_sources(&mut tt, &[("t.go", S.as_bytes())], None);
    let a = analyze(&pkg);
    assert!(a.cycles.is_empty(), "接口方法值无静态依赖：{:?}", a.cycles);
}

/// 嵌入提升：`T5.m` 解析到 `E5.m`（方法集要穿透嵌入字段）。
#[test]
fn t_ck_72_嵌入提升的方法() {
    const S: &str = r#"package p

type E int

func (E) m() int { return x }

type T struct{ E }

var x = T.m
"#;
    let mut tt = TypeTable::new();
    let (pkg, _bag) = load_sources(&mut tt, &[("t.go", S.as_bytes())], None);
    let a = analyze(&pkg);
    assert_eq!(a.cycles.len(), 1, "提升方法要进环：{:?}", a.cycles);
    assert!(
        a.cycles[0].message.contains("E.m"),
        "{}",
        a.cycles[0].message
    );
}

/// 常量环**不重复报**：装载面已经报过一条（`hit_cycle`），检查面不再补。
#[test]
fn t_ck_72_常量环不重复报() {
    const S: &str = "package p\n\nconst a = b\nconst b = a\n";
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_sources(&mut tt, &[("t.go", S.as_bytes())], None);
    let a = analyze(&pkg);
    assert!(a.cycles.is_empty(), "常量环归装载面：{:?}", a.cycles);
    let mut n = 0;
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    for r in &checked.diags {
        if r.diag.code == DiagCode::TypInvalidInitCycle {
            n += 1;
        }
    }
    assert_eq!(n, 1, "一条，不是两条（C8 保留重复行）");
}

/// 顺序面不受环影响：环上的成员仍然全部出现在 `InitOrder` 里
/// （官方在有环时「顺序不可信」但不崩 —— Task 32 的守卫在这里再钉一次）。
#[test]
fn t_ck_72_环上成员仍进顺序() {
    const S: &str = "package p\n\nvar a = b\nvar b = a\nvar c = 1\n";
    let mut tt = TypeTable::new();
    let (pkg, _bag) = load_sources(&mut tt, &[("t.go", S.as_bytes())], None);
    let got = compute(&pkg).render();
    let mut sorted = got.clone();
    sorted.sort();
    assert_eq!(sorted, vec!["a", "b", "c"], "三个都要在：{got:?}");
}
