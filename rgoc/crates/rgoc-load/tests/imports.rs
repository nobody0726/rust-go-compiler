//! 源码 import（`T-TY-62` / `T-TY-63` / `T-TY-85`）—— M3/Task 27。
//!
//! 判据来源：`docs/milestones/M3-plan.md` Task 27 + `M3-tests.md` §3.6。
//!
//! # 夹具（`testdata/multi-pkg/`）与它们要打的点
//!
//! | 目录 | 内容 | 打什么 |
//! |---|---|---|
//! | `lib/` | `lib.go`（真源码）+ `lib_test.go`（`package lib_test`）+ `_skip.go`（`package skipped`） | 只装**可编译**的源码：后两个若被装进来，包名不一致会立刻报错 |
//! | `app/` | `import "lib"` | 正向：按显式路径装载 |
//! | `app2/` | `import "app"` | **传递**依赖（app2 → app → lib） |
//! | `pkga/` + `pkgb/` | 互相 import | 环：`pkga → pkgb → pkga` |
//! | `both/` | 同时 import `pkga` 与 `pkgb` | 环只报一次（**缓存**的守卫） |
//! | `broken/` | `import "empty"` | 目录在、但没有 `.go` ⇒ `TYP-BROKEN-IMPORT` |
//! | `dup/` | 两个文件各 import 一次 `empty` | 失败的 import **每条都报**（官方对每条 spec 各报一次） |
//! | `unsup/` | `import "fmt"` + `import "example.com/x"` | 不支持的形式 ⇒ `TYP-UNSUPPORTED-IMPORT-FORM`（**不是**「找不到」） |
//! | `empty/` | 只有个 `README` | 上两条的反向用例 |
//!
//! # 三条纪律
//!
//! 1. **正向都有反向**：装载成功必须**零**诊断（`lib_test.go` 那条尤其关键 ——
//!    它和 `lib.go` 的包名不同，被误装进来必然出声）；
//! 2. **条数用精确值**（环是**两条**：环上每个包一条；`dup` 是**两条**：每条 spec 一条）；
//! 3. **位置钉到列**（用会话共享的文件表渲染 —— 被依赖包里的位置也该能渲染）。

use std::fs;
use std::path::{Path, PathBuf};

use rgoc_lex::{Diag, DiagCode};
use rgoc_load::{ImportResolver, MAX_IMPORT_DEPTH, Package, load_with_imports};
use rgoc_types::{ObjKind, TypeTable};

// ── 基础设施 ─────────────────────────────────────────────────────────────────

/// 夹具根（**绝对路径**：`cargo test` 的工作目录可能是 crate 目录，别依赖它）。
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/multi-pkg")
}

/// 夹具里的一个文件。
fn f(rel: &str) -> PathBuf {
    root().join(rel)
}

/// 夹具文件的原文（用来算列号）。
fn text(rel: &str) -> String {
    fs::read_to_string(f(rel)).unwrap_or_else(|e| panic!("读不到 {rel}：{e}"))
}

/// `1` 起的列号（**字节**单位，与 `Pos::col` 同）—— 先证候选不相等，再断言。
fn col_of(source: &str, line: u32, needle: &str) -> u32 {
    let l = source
        .lines()
        .nth((line - 1) as usize)
        .unwrap_or_else(|| panic!("fixture 没有第 {line} 行"));
    let at = l
        .find(needle)
        .unwrap_or_else(|| panic!("第 {line} 行里没有 {needle:?}：{l:?}"));
    (at as u32) + 1
}

/// 装载入口文件列表（`roots` = 夹具根）。
fn load(entry: &[&str]) -> (Package, Vec<Diag>) {
    let files: Vec<PathBuf> = entry.iter().map(|e| f(e)).collect();
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, &files, &[root()], None);
    (pkg, bag.into_sorted())
}

/// 一条诊断的位置，渲染成 `「倒数两级目录/文件.go:行:列」`。
///
/// 用**入口包**那张表就够了 —— 会话共用一张 `FileTable`（`importer` 模块文档第 1 条），
/// 被依赖包里的位置也在里面。
fn at(pkg: &Package, d: &Diag) -> String {
    let name = pkg.file_table.name(d.pos.file);
    let mut seg = name.rsplit('/');
    let last = seg.next().unwrap_or(name);
    let prev = seg.next().unwrap_or("");
    format!("{prev}/{last}:{}:{}", d.pos.line(), d.pos.col())
}

/// `(渲染位置, 代号, 消息)` 三元组。
fn triples(pkg: &Package, ds: &[Diag]) -> Vec<(String, DiagCode, String)> {
    ds.iter()
        .map(|d| (at(pkg, d), d.code, d.message.clone()))
        .collect()
}

// ── `T-TY-62` 源码 import ───────────────────────────────────────────────────

#[test]
fn t_ty_62_按显式路径装载被依赖包() {
    let (pkg, ds) = load(&["app/app.go"]);
    assert!(ds.is_empty(), "正向装载必须零诊断：{ds:#?}");
    assert_eq!(pkg.name, "app");

    let lib = pkg
        .imports
        .get("lib")
        .expect("`import \"lib\"` 的装载结果应当挂在 `Package::imports` 上");
    assert_eq!(lib.name, "lib", "包名来自被依赖包自己的 `package` 子句");
    // 被依赖包的声明都在（类型住在**共享的 TypeTable** 里 ⇒ 跨包同一性有底座）
    assert!(matches!(
        lib.lookup("T").map(|o| o.kind),
        Some(ObjKind::TypeName)
    ));
    assert!(matches!(
        lib.lookup("N").map(|o| o.kind),
        Some(ObjKind::Const)
    ));
    // **反向**：`lib_test.go`（`package lib_test`）与 `_skip.go`（`package skipped`）
    // 都不该被装进来 —— 装进来的话包名一致性会报错，这里也会先因为文件数而失败
    assert_eq!(lib.file_count(), 1, "只有 `lib.go` 属于这个包");
    assert!(lib.imports.is_empty(), "lib 自己不 import 任何东西");
    assert_eq!(pkg.imports.len(), 1);
}

#[test]
fn t_ty_62_传递依赖也要装载() {
    let (pkg, ds) = load(&["app2/app2.go"]);
    assert!(ds.is_empty(), "{ds:#?}");
    let app = pkg.imports.get("app").expect("app 被装载");
    let lib = app
        .imports
        .get("lib")
        .expect("传递依赖 lib 也要被装载（app2 → app → lib）");
    assert_eq!(lib.name, "lib");
    assert!(matches!(
        lib.lookup("T").map(|o| o.kind),
        Some(ObjKind::TypeName)
    ));
}

// ── `T-TY-63` import 环 ─────────────────────────────────────────────────────

/// 环的口径：**环上每个包一条**，位置取「把这个包拉进环的那条 import」；
/// 入口包不是被 import 进来的 ⇒ 借用**闭合边**（发现环的那条 import）。
///
/// `pkga → pkgb → pkga` ⇒ 两条，各落在两条 import 上 `3:8`（`import "xxxx"` 的引号处）。
#[test]
fn t_ty_63_环上每个包一条() {
    let (pkg, ds) = load(&["pkga/a.go"]);
    assert_eq!(
        triples(&pkg, &ds),
        vec![
            (
                format!("pkga/a.go:3:{}", col_of(&text("pkga/a.go"), 3, "\"pkgb\"")),
                DiagCode::TypImportCycle,
                "import cycle through package \"pkga\"".to_owned(),
            ),
            (
                format!("pkgb/b.go:3:{}", col_of(&text("pkgb/b.go"), 3, "\"pkga\"")),
                DiagCode::TypImportCycle,
                "import cycle through package \"pkga\"".to_owned(),
            ),
        ]
    );

    // 从环上的**另一个**包出发，同样两条（消息里的包名跟着换）。
    // 顺序按**文件 Id**（入口文件先入表 ⇒ 它先出）。
    let (pkg, ds) = load(&["pkgb/b.go"]);
    assert_eq!(
        triples(&pkg, &ds),
        vec![
            (
                format!("pkgb/b.go:3:{}", col_of(&text("pkgb/b.go"), 3, "\"pkga\"")),
                DiagCode::TypImportCycle,
                "import cycle through package \"pkgb\"".to_owned(),
            ),
            (
                format!("pkga/a.go:3:{}", col_of(&text("pkga/a.go"), 3, "\"pkgb\"")),
                DiagCode::TypImportCycle,
                "import cycle through package \"pkgb\"".to_owned(),
            ),
        ]
    );

    // 反向：**不成环**的长依赖链零诊断（别把「都指向同一个包」误判成环）
    let (_pkg, ds) = load(&["app2/app2.go"]);
    assert!(ds.is_empty(), "{ds:#?}");
    // 反向：不 import 任何东西的包也零诊断
    let (_pkg, ds) = load(&["lib/lib.go"]);
    assert!(ds.is_empty(), "{ds:#?}");
}

/// **缓存**的守卫：`both` 同时 import `pkga` 与 `pkgb`，而两者本来就成环 ⇒
/// 环**只**报一次（否则第二条 import 会再走一遍环，报出四条）。
#[test]
fn t_ty_63_同一个包只装载一次() {
    let (pkg, ds) = load(&["both/both.go"]);
    let got = triples(&pkg, &ds);
    assert_eq!(got.len(), 2, "环只报一次：{got:#?}");
    assert_eq!(
        got,
        vec![
            (
                format!(
                    "both/both.go:4:{}",
                    col_of(&text("both/both.go"), 4, "\"pkga\"")
                ),
                DiagCode::TypImportCycle,
                "import cycle through package \"pkga\"".to_owned(),
            ),
            (
                format!("pkga/a.go:3:{}", col_of(&text("pkga/a.go"), 3, "\"pkgb\"")),
                DiagCode::TypImportCycle,
                "import cycle through package \"pkga\"".to_owned(),
            ),
        ]
    );
    // 两个直接依赖都拿到了装载结果，而 `pkgb` 是**复用**的那一份（不是又装一遍）
    assert_eq!(pkg.imports.len(), 2);
    assert_eq!(pkg.imports["pkgb"].name, "pkgb");
}

// ── `T-TY-85` 不支持的 import 形式 / 装不上 ──────────────────────────────────

/// `D-M3-5`：M3 只认「roots 下能找到**源码目录**」这一种形式。
/// `fmt`（标准库，只有二进制 export data）与 `example.com/x`（模块路径）
/// 都是**我们不支持的形式** ⇒ 稳定拒绝码，**不是**「找不到」。
#[test]
fn t_ty_85_不支持的_import_形式_是稳定拒绝码() {
    let (pkg, ds) = load(&["unsup/u.go"]);
    let got = triples(&pkg, &ds);
    assert_eq!(got.len(), 2, "{got:#?}");
    for (i, path) in [(0, "fmt"), (1, "example.com/x")].iter() {
        let (pos, code, msg) = &got[*i];
        let line = 4 + *i as u32;
        // 位置取 **spec**（即引号处），所以 needle 带引号 —— 先证两个候选不相等
        let needle = format!("\"{path}\"");
        assert_eq!(
            pos,
            &format!(
                "unsup/u.go:{line}:{}",
                col_of(&text("unsup/u.go"), line, &needle)
            ),
            "位置要落在**那条 import** 上"
        );
        assert_eq!(*code, DiagCode::TypUnsupportedImportForm, "{msg}");
        assert!(msg.contains(path), "{msg}");
        // 反向：它不是「装不上」（那会把**能力边界**说成用户的错）
        assert_ne!(*code, DiagCode::TypBrokenImport, "{msg}");
    }
}

/// 反向：目录**在**、但没有可编译的 `.go` ⇒ 那是「装不上」（官方 `BrokenImport`），
/// 而不是「不支持的形式」——两者的分界是**目录在不在**。
#[test]
fn t_ty_85_反向_目录在但没有源码() {
    let (pkg, ds) = load(&["broken/main.go"]);
    let got = triples(&pkg, &ds);
    assert_eq!(got.len(), 1, "{got:#?}");
    let (pos, code, msg) = &got[0];
    assert_eq!(
        pos,
        &format!(
            "broken/main.go:3:{}",
            col_of(&text("broken/main.go"), 3, "\"empty\"")
        )
    );
    assert_eq!(*code, DiagCode::TypBrokenImport);
    assert!(
        msg.contains("empty") && msg.contains("no Go files"),
        "{msg}"
    );
}

/// 失败的 import **每条 spec 都报**（官方 `check.errorf(at, BrokenImport, …)`
/// 用的是 spec 的位置 ⇒ 两条 import 就是两条）—— 与「成功的包只装一次」对照着看。
#[test]
fn t_ty_85_失败的_import_每条都报() {
    let (pkg, ds) = load(&["dup/one.go", "dup/two.go"]);
    let got = triples(&pkg, &ds);
    assert_eq!(got.len(), 2, "{got:#?}");
    assert!(got.iter().all(|(_, c, _)| *c == DiagCode::TypBrokenImport));
    assert_eq!(
        got[0].0,
        format!(
            "dup/one.go:3:{}",
            col_of(&text("dup/one.go"), 3, "\"empty\"")
        )
    );
    assert_eq!(
        got[1].0,
        format!(
            "dup/two.go:3:{}",
            col_of(&text("dup/two.go"), 3, "\"empty\"")
        )
    );
}

// ── 深度上限（`M3-design.md` §7.2）───────────────────────────────────────────

/// 在临时目录里造一条 `n` 个包的 import 链：`p0 → p1 → … → p{n-1}`。
/// 返回 `(根目录, 入口文件)`。
fn chain(tag: &str, n: usize) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("rgoc-import-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for i in 0..n {
        let dir = root.join(format!("p{i}"));
        fs::create_dir_all(&dir).unwrap();
        let body = if i + 1 < n {
            format!("package p{i}\n\nimport \"p{}\"\n", i + 1)
        } else {
            format!("package p{i}\n")
        };
        fs::write(dir.join("i.go"), body).unwrap();
    }
    (root.clone(), root.join("p0/i.go"))
}

#[test]
fn import_深度上限是_100() {
    // 正好 100 层依赖 ⇒ 零诊断，而且**真装到了底**
    let (root, entry) = chain("ok", MAX_IMPORT_DEPTH + 1);
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, &[entry], std::slice::from_ref(&root), None);
    let ds = bag.into_sorted();
    assert!(ds.is_empty(), "{ds:#?}");
    let mut cur = &pkg;
    let mut depth = 0;
    while let Some(next) = cur.imports.values().next() {
        cur = next;
        depth += 1;
    }
    assert_eq!(depth, MAX_IMPORT_DEPTH, "链要装到底（不是提前截断）");
    let _ = fs::remove_dir_all(&root);

    // 多一层（101）⇒ **只报一条**，位置落在**第 100 层那个包**的那条 import 上
    let (root, entry) = chain("deep", MAX_IMPORT_DEPTH + 2);
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, &[entry], std::slice::from_ref(&root), None);
    let ds = bag.into_sorted();
    assert_eq!(ds.len(), 1, "{ds:#?}");
    assert_eq!(ds[0].code, DiagCode::TypNestingTooDeep);
    assert!(
        ds[0].message.contains("import depth exceeds 100"),
        "{}",
        ds[0].message
    );
    assert_eq!(
        at(&pkg, &ds[0]),
        format!("p{MAX_IMPORT_DEPTH}/i.go:3:8"),
        "落在**第 100 层**包里的那条 import 上"
    );
    let _ = fs::remove_dir_all(&root);
}

// ── `ImportResolver` 的两条硬规则 ────────────────────────────────────────────

#[test]
fn resolver_第一条命中的根生效且只挑可编译源码() {
    // 第一条根里没有 `lib` ⇒ 落到第二条（`D-M3-5` 的「显式路径映射」）
    let missing = root().join("empty");
    let r = ImportResolver::new(&[missing, root()]);
    let id = r.resolve("lib").expect("`lib` 应当被解析到");
    assert_eq!(
        id.files.len(),
        1,
        "`lib_test.go` / `_skip.go` 都不算：{:#?}",
        id.files
    );
    assert!(id.files[0].ends_with("lib.go"));
    assert!(id.dir.ends_with("lib"));

    // 目录**在**、但没有可用的 `.go` ⇒ `Some`（空）—— 与「没有这个目录」分开，
    // 正是 `TypBrokenImport` 与 `TypUnsupportedImportForm` 的分界
    let empty = r.resolve("empty").expect("`empty` 目录在");
    assert!(empty.files.is_empty());

    // roots 里根本没有 ⇒ `None`
    assert!(r.resolve("fmt").is_none());
    assert!(r.resolve("example.com/x").is_none());
}
