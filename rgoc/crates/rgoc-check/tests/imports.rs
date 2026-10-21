//! `T-CK-74` / `T-CK-75`：import 三形态与跨包可见性（M4/Task 35）。
//!
//! # 判据来源
//!
//! 官方 `go/types`：
//!
//! * `resolver.go:262-334`（import spec 的名字进入规则：别名 / `_` / `.` / `import init`）
//! * `resolver.go:461-478`（文件层 × 包层同名：`already declared through import/dot-import of %s`）
//! * `call.go:698-760`（`pkg.X`：`UndeclaredImportedName` / `UnexportedName` / 操作数模式）
//!
//! # 证据：为什么要自己写探针
//!
//! `tools/types-oracle --check` 的 importer **只认 `unsafe`**
//! （`tools/types-oracle/check.go` 的 ③），所以语料里任何跨包引用都只会得到
//! `could not import`。这一族语义只能靠带源码 importer 的探针实测 ——
//! `.superpowers/sdd/M4-plan/probe/t35/`（`go/importer.ForCompiler(fset, "source", nil)`
//! 与 `GOPATH=probe/t35/gopath`）。本文件的 `cases/s*.go` 与探针的 `cases/` **逐字节一致**，
//! 期望值就是探针输出（`probe/t35/run.log`）。
//!
//! # 已知缺口（`open_gaps`，不影响本文件判据）
//!
//! 官方对每条 import spec 还有一条 **`"lib" imported and not used`**（`unused` 类）。
//! 那要**使用追踪**（`usedPkgNames`），本阶段没有 —— 所以 `s5`/`s8`/`s9` 的期望
//! 只有冲突那一条，比探针少一行。缺口登记在 `M4-manifest.json` 的 `open_gaps`。

use std::path::{Path, PathBuf};

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_lex::DiagCode;
use rgoc_load::{Package, load_with_imports};
use rgoc_types::TypeTable;

/// 夹具根。
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/imports")
}

/// 装载 `cases/<name>` 并跑检查，返回 `(C8 渲染数据行, 诊断码序列)`。
fn check_case(name: &str) -> (Vec<String>, Vec<DiagCode>) {
    let mut tt = TypeTable::new();
    let file = root().join("cases").join(name);
    let (pkg, bag) = load_with_imports(&mut tt, std::slice::from_ref(&file), &[root()], None);
    checked(&pkg, &mut tt, bag, name)
}

fn checked(
    pkg: &Package,
    tt: &mut TypeTable,
    bag: rgoc_lex::DiagBag,
    name: &str,
) -> (Vec<String>, Vec<DiagCode>) {
    let c = check_package(pkg, tt, bag, &CheckOptions::default());
    let rows = c
        .render(name)
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(str::to_owned)
        .collect();
    let codes = c.diags.iter().map(|d| d.diag.code).collect();
    (rows, codes)
}

/// 别名 import：导出性 + 未定义（`pkg.X` 的三条判据）。
///
/// 探针原文（`probe/t35/run.log`，`cases/s1.go`）**五行**：
/// `6:11 name hidden not exported by package lib` /
/// `7:11 undefined: l.Nope` /
/// `8:11 undefined: l.exported (but have Exported)` /
/// `12:11 name g not exported by package lib` /
/// `14:19 l.T{}.m undefined (type lib.T has no field or method m)`
///
/// ⚠️ 本断言只取**前四行**：第五行是官方的 `sameId` 规则
/// （`object.go:139-157`：非导出名要求「同一个包」），而我们的类型/方法表
/// **不带包身份**（`Ruling 8-4` 的 `objset` 键不含包）⇒ `l.T{}.m` 现在会被
/// 当成找到了 `m`。缺口登记在 `M4-manifest.json` 的 `open_gaps`
/// （`U-M4-12`：跨包 `sameId`）。语料是单包面，打不到这里。
#[test]
fn t_ck_74_别名导入_导出性向量() {
    let (rows, _) = check_case("s1.go");
    assert_eq!(
        rows,
        vec!["6:11\tundef", "7:11\tundef", "8:11\tundef", "12:11\tundef"],
        "vs 探针（`probe/t35`，带源码 importer 的 go/types）—— 见文档的第五行缺口"
    );
}

/// 消息**逐字**（官方的两句 + 带 `(but have …)` 的那句）。
#[test]
fn t_ck_74_消息逐字() {
    let mut tt = TypeTable::new();
    let file = root().join("cases/s1.go");
    let (pkg, bag) = load_with_imports(&mut tt, std::slice::from_ref(&file), &[root()], None);
    let c = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    let got: Vec<(u32, &str)> = c
        .diags
        .iter()
        .map(|d| (d.diag.pos.line, d.diag.message.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (6, "name hidden not exported by package lib"),
            (7, "undefined: l.Nope"),
            (8, "undefined: l.exported (but have Exported)"),
            (12, "name g not exported by package lib"),
        ],
        "消息与报点（第五行 l.T 的非导出方法见上一条测试的缺口说明）"
    );
}

/// 诊断码：`UnexportedName` 与 `UndeclaredImportedName` 是**两条独立码**
/// （官方就是两条），不复用 `TYP-UNDECLARED-NAME`。
#[test]
fn t_ck_74_两条独立码() {
    let (_, codes) = check_case("s1.go");
    assert_eq!(
        codes,
        vec![
            DiagCode::TypUnexportedName,
            DiagCode::TypUndeclaredImportedName,
            DiagCode::TypUndeclaredImportedName,
            DiagCode::TypUnexportedName,
        ]
    );
}

/// `T-CK-75` 点导入：被导入包的**导出名**注入文件作用域。
///
/// 探针：`6:9 undefined: hidden` —— 只有**非导出**名找不到，导出名
/// （`Exported` / `T` / `F`）全部可见，且 `var _ T` 能用（类型位置）。
#[test]
fn t_ck_75_点导入_注入导出名() {
    let (rows, _) = check_case("s2.go");
    assert_eq!(rows, vec!["6:9\tundef"], "vs 探针 cases/s2.go");
}

/// `T-CK-75` 空导入：**不引入包名**（`import _ "lib"` 之后 `lib.X` 是
/// `undefined: lib`，不是 `undefined: lib.X`）。
///
/// 探针：`5:9 undefined: lib`（报点在整个 `lib` 标识符上）。
#[test]
fn t_ck_75_空导入不引入包名() {
    let (rows, codes) = check_case("s3.go");
    assert_eq!(rows, vec!["5:9\tundef"], "vs 探针 cases/s3.go");
    assert_eq!(
        codes,
        vec![DiagCode::TypUndeclaredName],
        "不是 ImportedName 那条"
    );
}

/// 无别名 import 的**真名**取被导入包的 `package` 子句（这里是 `lib`）。
///
/// 探针：`6:13 name hidden not exported by package lib` —— 报在 `.` 后的名字位。
#[test]
fn t_ck_74_默认名取包名() {
    let (rows, _) = check_case("s4.go");
    assert_eq!(rows, vec!["6:13\tundef"], "vs 探针 cases/s4.go");
}

/// 别名与**包层**同名 ⇒ `already declared through import of package %s`
/// （官方 `resolver.go:466-470`），报在**包层那个对象**处。
///
/// 探针 `cases/s5.go` 两条：`3:8 "lib" imported and not used`（见模块文档的缺口）
/// + `5:5 lib already declared through import of package lib ("lib")`。
#[test]
fn t_ck_74_别名与包层同名() {
    let mut tt = TypeTable::new();
    let file = root().join("cases/s5.go");
    let (pkg, bag) = load_with_imports(&mut tt, std::slice::from_ref(&file), &[root()], None);
    let c = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    let got: Vec<(u32, u32, String)> = c
        .diags
        .iter()
        .map(|d| (d.diag.pos.line, d.diag.pos.col, d.diag.message.clone()))
        .collect();
    assert_eq!(
        got,
        vec![(
            5,
            5,
            "lib already declared through import of package lib (\"lib\")".to_owned()
        )]
    );
    assert_eq!(c.diags[0].diag.code, DiagCode::TypDuplicateDecl);
}

/// 点导入注入的名字与**包层**同名 ⇒ `already declared through dot-import of package %s`。
///
/// 探针 `cases/s6.go`：`5:5 Exported already declared through dot-import of package lib ("lib")`
/// （+ 那条 `unused`）。
#[test]
fn t_ck_75_点导入与包层同名() {
    let mut tt = TypeTable::new();
    let file = root().join("cases/s6.go");
    let (pkg, bag) = load_with_imports(&mut tt, std::slice::from_ref(&file), &[root()], None);
    let c = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    let got: Vec<(u32, u32, String)> = c
        .diags
        .iter()
        .map(|d| (d.diag.pos.line, d.diag.pos.col, d.diag.message.clone()))
        .collect();
    assert_eq!(
        got,
        vec![(
            5,
            5,
            "Exported already declared through dot-import of package lib (\"lib\")".to_owned()
        )]
    );
}

/// `import init "lib"` ⇒ `cannot import package as init - init must be a func`
/// （官方 `resolver.go:288`，代号 `InvalidInitDecl`，报在 spec 的名字位）。
#[test]
fn t_ck_75_不能把包名导入成_init() {
    let (rows, codes) = check_case("s7.go");
    assert_eq!(rows, vec!["3:8\tdecl"], "vs 探针 cases/s7.go");
    assert_eq!(codes, vec![DiagCode::TypInvalidInitDecl]);
}

/// 同一文件里重复 import 同一路径 ⇒ `lib redeclared in this block`
/// （官方 `resolver.go:323`，`DuplicateDecl`，报在**第二条** spec）。
#[test]
fn t_ck_75_重复_import_同路径() {
    let (rows, codes) = check_case("s8.go");
    assert_eq!(
        rows,
        vec!["4:8\tdecl"],
        "vs 探针 cases/s8.go（少一条 unused）"
    );
    assert_eq!(codes, vec![DiagCode::TypDuplicateDecl]);
}

/// 同一文件里两个 import 用同一个别名 ⇒ `a redeclared in this block`。
#[test]
fn t_ck_75_重复_import_同别名() {
    let (rows, codes) = check_case("s9.go");
    assert_eq!(
        rows,
        vec!["4:8\tdecl"],
        "vs 探针 cases/s9.go（少一条 unused）"
    );
    assert_eq!(codes, vec![DiagCode::TypDuplicateDecl]);
}

/// 接受向量：三形态 import 各用其法 ⇒ **零诊断**。
#[test]
fn t_ck_74_接受向量零诊断() {
    const SRC: &str = r#"package p

import (
	l "lib"
	. "lib"
	_ "lib"
)

var _ = l.Exported
var _ = Exported
var _ = T{}
var _ int = F(1)
var _ int = l.T{}.M()
var _ lib.T
var _ = l.V
"#;
    let dir = root().join("cases");
    let path = dir.join("_accept_tmp.go");
    std::fs::write(&path, SRC).expect("写临时夹具");
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, std::slice::from_ref(&path), &[root()], None);
    let (rows, _) = checked(&pkg, &mut tt, bag, "_accept_tmp.go");
    let _ = std::fs::remove_file(&path);
    assert!(rows.is_empty(), "接受向量应当零诊断，得到 {rows:?}");
}

/// `-I` 根指错（路径不存在）与**不支持的形式**在诊断上分家：
/// 目录在但没 `.go` ⇒ `TYP-BROKEN-IMPORT`；根下根本没这个目录 ⇒
/// `TYP-UNSUPPORTED-IMPORT-FORM`（`D-M3-11` 的稳定拒绝码）。
#[test]
fn t_ck_74_根指错与不支持形式分家() {
    let file = root().join("cases/importonly.go");
    // 根指到一个**不存在**的目录 ⇒ `lib` 找不到 ⇒ 不支持的形式
    let bad_root = root().join("no-such-root");
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, std::slice::from_ref(&file), &[bad_root], None);
    let codes: Vec<DiagCode> = bag.into_sorted().iter().map(|d| d.code).collect();
    assert_eq!(codes, vec![DiagCode::TypUnsupportedImportForm]);
    assert!(pkg.imports.is_empty(), "装载不上就没有 import 结果");

    // 根对、但目录里没有 `.go` ⇒ `TYP-BROKEN-IMPORT`
    let empty_root = root().join("empty-root");
    std::fs::create_dir_all(empty_root.join("lib")).expect("建空目录");
    let mut tt2 = TypeTable::new();
    let (pkg2, bag2) = load_with_imports(
        &mut tt2,
        std::slice::from_ref(&file),
        std::slice::from_ref(&empty_root),
        None,
    );
    let codes2: Vec<DiagCode> = bag2.into_sorted().iter().map(|d| d.code).collect();
    let _ = std::fs::remove_dir_all(&empty_root);
    assert_eq!(codes2, vec![DiagCode::TypBrokenImport]);
    assert!(pkg2.imports.is_empty());
}
