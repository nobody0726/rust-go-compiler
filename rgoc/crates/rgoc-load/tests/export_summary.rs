//! 导出符号摘要（`ExportSummary`，`T-TY-64`）—— M3/Task 29。
//!
//! 判据来源：`M3-plan.md` Task 29 + `M3-tests.md` §3.6 的 `T-TY-64`（来源 `H`）
//! 与 `D-M3-18`（**不**冻文本格式、**不**落盘 ⇒ 这里全是**结构断言**）。
//!
//! # 这个用例要打的四件事
//!
//! 1. **只有导出名进摘要** —— 非导出名一个都不在；
//! 2. **内层名字不过滤** —— `type S struct { x int; Y string }` 的 `x` / `Y`
//!    不是顶层符号（不在摘要里），但它们是**类型的一部分** ⇒ 照常出现在
//!    `S` 的 dump 里。这两条一起才说明白「筛的是顶层符号，不是类型里的每个名字」；
//! 3. **值 = 该对象类型的 C7 小 dump**（函数 = 签名；无类型 ⇒ `-`；不带 `#` 头行）；
//! 4. **确定**：`BTreeMap` 序（`names()` 是字典序）+ 两次调用逐字节相同。
//!
//! ⚠️ 反向断言（最容易写成恒真）：「不是本包的符号」那一串里放了
//! `int` / `string` / `error`（**universe** 的名字）与 `lib`（**文件作用域**的
//! `import` 名）—— 若实现顺手沿父链查名字，它们就会漏进摘要。

use rgoc_lex::Diag;
use rgoc_load::{ExportEntry, ExportSummary, Package, export_summary, load_sources};
use rgoc_types::TypeTable;

// ── 夹具 ─────────────────────────────────────────────────────────────────────

/// 一个把六种情况都摆上的包：
///
/// | 声明 | 打什么 |
/// |---|---|
/// | `import "lib"` | `import` 名住**文件**作用域 ⇒ 不进摘要 |
/// | `const c` / `const C` | 非导出/导出；值的**类型**进摘要（`untyped_int`） |
/// | `type s …` / `type S …` | 非导出/导出；`S` 的字段名（含非导出 `x`）照样进 dump |
/// | `var u int` / `var V struct{ P int }` | 非导出/导出（结构字面量类型） |
/// | `var W = 1` | 导出但**没有类型**（M3 不评估 var 初始化式 ⇒ `typ: None`）⇒ 值 `-` |
/// | `func f()` / `func F(n int) (int, string)` | 非导出/导出；函数的值是**签名** |
const SRC: &str = "\
package p

import \"lib\"

const c = 1
const C = 2_000

type s struct{ x int }
type S struct {
	x int
	Y string
}

var u int
var V struct{ P int }
var W = 1

func f() {}
func F(n int) (int, string) { return 0, \"\" }
";

fn load(files: &[(&str, &str)]) -> (Package, TypeTable, Vec<Diag>) {
    let mut tt = TypeTable::new();
    let srcs: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(n, s)| ((*n).to_string(), s.as_bytes().to_vec()))
        .collect();
    let borrowed: Vec<(&str, &[u8])> = srcs
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let (pkg, bag) = load_sources(&mut tt, &borrowed, None);
    (pkg, tt, bag.into_sorted())
}

// ── `T-TY-64` ────────────────────────────────────────────────────────────────

#[test]
fn t_ty_64_只有导出名进摘要() {
    let (pkg, tt, ds) = load(&[("p.go", SRC)]);
    assert!(ds.is_empty(), "夹具本身必须零诊断：{ds:#?}");
    let s = export_summary(&pkg, &tt);

    // ① 名字集合：五个导出名，**字典序**（`BTreeMap`）
    let names: Vec<&str> = s.names().collect();
    assert_eq!(names, ["C", "F", "S", "V", "W"]);
    assert_eq!(s.len(), 5);
    assert!(!s.is_empty());
    // `iter()` 与 `names()` 同序（同一次单调序）
    let iter_names: Vec<&str> = s.iter().map(|(n, _)| n).collect();
    assert_eq!(iter_names, names);

    // ② 非导出名一个都不在；**内层**导出名 `Y` 也不在（它不是顶层符号）；
    //    `int` / `string` / `error` 是 **universe** 的、`lib` 是**文件作用域**的 ——
    //    这三类都不属于「本包的导出符号」
    for n in [
        "c", "f", "s", "u", "v", "w", "x", "Y", "lib", "int", "string", "error", "_",
    ] {
        assert!(s.get(n).is_none(), "`{n}` 不该出现在导出摘要里：{s:#?}");
    }
}

#[test]
fn t_ty_64_值的种类与内容() {
    let (pkg, tt, ds) = load(&[("p.go", SRC)]);
    assert!(ds.is_empty(), "{ds:#?}");
    let s = export_summary(&pkg, &tt);
    let val = |n: &str| -> &str {
        s.get(n)
            .unwrap_or_else(|| panic!("`{n}` 应当在摘要里"))
            .value()
    };

    // ③ 种类（四种变体各自对上一种声明）
    assert!(matches!(s.get("C"), Some(ExportEntry::Const(_))));
    assert!(matches!(s.get("F"), Some(ExportEntry::Func(_))));
    assert!(matches!(s.get("S"), Some(ExportEntry::Type(_))));
    assert!(matches!(s.get("V"), Some(ExportEntry::Var(_))));

    // 类型：值是该类型的 C7 小 dump（`named` + 本体），**内层字段名照常出现**
    // ——`x` 是**非导出**字段、`Y` 是导出字段，两者都在 dump 里（口径 2）
    let sv = val("S");
    assert!(sv.contains("named"), "S 是具名类型：{sv}");
    assert!(sv.contains("struct"), "S 的本体是结构体：{sv}");
    assert!(
        sv.contains("\"x\""),
        "**非导出**字段名照样进类型 dump：{sv}"
    );
    assert!(sv.contains("\"Y\""), "导出字段名也进类型 dump：{sv}");
    assert!(
        !sv.contains("# source"),
        "值是自足小 dump，不带来源标记：{sv}"
    );

    // 常量：值是**类型**（`untyped_int`），不是值本身
    let cv = val("C");
    assert!(cv.contains("basic untyped_int"), "常量的类型：{cv}");

    // 变量：字面结构体类型
    let vv = val("V");
    assert!(vv.contains("struct"), "{vv}");
    assert!(vv.contains("\"P\""), "{vv}");

    // 函数：值是**签名**（参数名 / 结果都在）
    let fv = val("F");
    assert!(fv.contains("signature"), "{fv}");
    assert!(fv.contains("\"n\""), "参数名：{fv}");
    assert!(fv.contains("basic int"), "{fv}");
    assert!(fv.contains("basic string"), "{fv}");

    // `var W = 1`：M3 不评估 var 初始化式 ⇒ 没有类型 ⇒ 值 `-`
    assert_eq!(val("W"), "-", "`var W` 没有类型 ⇒ `-`");
}

#[test]
fn t_ty_64_确定性与空包() {
    let (pkg, tt, ds) = load(&[("p.go", SRC)]);
    assert!(ds.is_empty(), "{ds:#?}");
    // ④ 两次调用逐字节相同（`BTreeMap` + `dump_types` 都是确定的）
    assert_eq!(export_summary(&pkg, &tt), export_summary(&pkg, &tt));

    // 反向：一个导出名都没有的包 ⇒ 空摘要（不是 panic、不是 `None`）
    let (pkg, tt, ds) = load(&[("q.go", "package q\n\nvar x int\n")]);
    assert!(ds.is_empty(), "{ds:#?}");
    let s = export_summary(&pkg, &tt);
    assert!(s.is_empty());
    assert_eq!(s.names().count(), 0);
    assert_eq!(s, ExportSummary::default());
}
