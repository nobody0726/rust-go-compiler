//! `T-TY-52` / `T-TY-61` / `T-TY-66`：单包多文件的作用域与包名一致性（M3/Task 23）。
//!
//! 判据来源：`docs/milestones/M3-tests.md` §4（`T-TY-52` 包作用域 vs 文件作用域一致 /
//! `T-TY-61` 同包跨文件可见 / `T-TY-66` 包名不一致命中诊断），
//! 与 `M3-design.md` 的 `D-M3-14`（显式文件列表、不评估构建约束）。

use std::fs;

use rgoc_lex::DiagCode;
use rgoc_load::{load_from_explicit_files, load_sources};
use rgoc_types::{ObjKind, ScopeKind, TypeTable};

/// 把 `(文件名, 源码)` 对转成装载器的入参。
fn src(pairs: &[(&str, &str)]) -> Vec<(String, Vec<u8>)> {
    pairs
        .iter()
        .map(|(n, s)| ((*n).to_string(), s.as_bytes().to_vec()))
        .collect()
}

fn borrowed(srcs: &[(String, Vec<u8>)]) -> Vec<(&str, &[u8])> {
    srcs.iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect()
}

/// `T-TY-52` + `T-TY-61`：两个文件共用一个**包作用域**，跨文件可见；
/// 文件作用域只在链上（`file ⊃ package ⊃ universe`）。
#[test]
fn t_ty_52_61_同包跨文件可见且是包级() {
    let mut tt = TypeTable::new();
    let srcs = src(&[
        ("a.go", "package p\n\ntype T int\n"),
        ("b.go", "package p\n\nvar x T\n"),
    ]);
    let (pkg, bag) = load_sources(&mut tt, &borrowed(&srcs), None);

    assert!(bag.into_sorted().is_empty(), "两个合法文件不该有诊断");
    assert_eq!(pkg.name, "p");
    assert_eq!(pkg.file_count(), 2);

    // 包作用域里有**两个文件各自**的声明（`a.go` 的 T 与 `b.go` 的 x 同层）
    assert_eq!(pkg.lookup("T").map(|o| o.kind), Some(ObjKind::TypeName));
    assert_eq!(pkg.lookup("x").map(|o| o.kind), Some(ObjKind::Var));
    assert_eq!(
        pkg.scopes.get(pkg.scope).kind(),
        ScopeKind::Package,
        "这一层是包作用域"
    );

    // 从 b.go 的**文件作用域**沿链也看得到 a.go 声明的 T
    let (fa, fb) = (pkg.files[0], pkg.files[1]);
    assert_eq!(
        pkg.lookup_in_file(fb, "T").map(|o| o.kind),
        Some(ObjKind::TypeName),
        "T-TY-61：同包跨文件可见"
    );
    assert_eq!(
        pkg.lookup_in_file(fa, "x").map(|o| o.kind),
        Some(ObjKind::Var),
        "双向可见（包作用域不区分文件）"
    );
    // 链的底仍然是 universe
    assert_eq!(pkg.lookup("len").map(|o| o.kind), Some(ObjKind::Builtin));
    // 文件层是包层的**子**（depth：universe=1, package=2, file=3）
    let d = pkg.file_scope(fb).expect("b.go 有文件作用域");
    assert_eq!(pkg.scopes.depth(d), 3);
    assert_eq!(pkg.scopes.get(d).parent(), Some(pkg.scope));
}

/// `T-TY-52` 的另一半：`import` 名住在**文件作用域**，不是包级。
#[test]
fn t_ty_52_import_名只在本文件可见() {
    let mut tt = TypeTable::new();
    let srcs = src(&[
        ("a.go", "package p\n\nimport f \"fmt\"\n\ntype A int\n"),
        ("b.go", "package p\n\nimport \"os\"\n"),
    ]);
    let (pkg, bag) = load_sources(&mut tt, &borrowed(&srcs), None);
    assert!(bag.into_sorted().is_empty());

    let (fa, fb) = (pkg.files[0], pkg.files[1]);
    assert_eq!(
        pkg.lookup_in_file(fa, "f").map(|o| o.kind),
        Some(ObjKind::PkgName),
        "a.go 显式别名"
    );
    assert_eq!(
        pkg.lookup_in_file(fb, "os").map(|o| o.kind),
        Some(ObjKind::PkgName),
        "b.go 无别名 ⇒ 取路径最后一段"
    );
    assert!(
        pkg.lookup_in_file(fb, "f").is_none(),
        "T-TY-52：文件 b 看不到 a 的 import 名"
    );
    assert!(
        pkg.lookup("f").is_none() && pkg.lookup("os").is_none(),
        "import 名**不是**包级声明"
    );
    // 但非 import 的顶层声明仍然是包级的（同一批文件里对比）
    assert_eq!(pkg.lookup("A").map(|o| o.kind), Some(ObjKind::TypeName));
}

/// 空标识符与点导入不引入名字。
#[test]
fn 空标识符与点导入不引入名字() {
    let mut tt = TypeTable::new();
    let srcs = src(&[(
        "a.go",
        "package p\n\nimport _ \"fmt\"\nimport . \"os\"\n\nvar _ T\n",
    )]);
    let (pkg, bag) = load_sources(&mut tt, &borrowed(&srcs), None);
    assert!(bag.into_sorted().is_empty());

    let fa = pkg.files[0];
    let s = pkg.scopes.scope(pkg.file_scope(fa).unwrap());
    assert!(s.lookup_local("_").is_none(), "`_` 永不绑定");
    assert!(s.is_empty(), "两个 import 都不引入名字：{s:?}");
    // 顶层 `var _ T` 也不绑定
    assert!(pkg.lookup("_").is_none());
    assert_eq!(pkg.scopes.get(pkg.scope).len(), 0);
}

/// `T-TY-66`：同包多文件包名不一致 ⇒ 命中诊断，且**位置指向出错的那个文件**。
#[test]
fn t_ty_66_包名不一致命诊断且忽略该文件() {
    let mut tt = TypeTable::new();
    let srcs = src(&[
        ("a.go", "package p\n\ntype A int\n"),
        ("b.go", "package q\n\ntype S int\n"),
    ]);
    let (pkg, bag) = load_sources(&mut tt, &borrowed(&srcs), None);

    let diags = bag.into_sorted();
    assert_eq!(diags.len(), 1, "只有一条：{diags:?}");
    let d = &diags[0];
    assert_eq!(d.code, DiagCode::TypMismatchedPkgName);
    // 位置 = b.go 的 `package` 关键字（官方 `check.go:307` 取 `file.Package`）
    assert_eq!(pkg.file_table.name(d.pos.file), "b.go");
    assert_eq!((d.pos.line, d.pos.col), (1, 1));
    assert!(
        d.message.contains('q') && d.message.contains('p'),
        "消息要说清「谁、期望谁」：{}",
        d.message
    );

    // 包名以**第一个**文件为准；不一致的文件被整个忽略
    assert_eq!(pkg.name, "p");
    assert_eq!(pkg.file_count(), 1);
    assert_eq!(pkg.file_table.name(pkg.files[0]), "a.go");
    assert!(pkg.lookup("S").is_none(), "被忽略文件里的声明不进作用域");
    assert_eq!(pkg.lookup("A").map(|o| o.kind), Some(ObjKind::TypeName));
}

/// 包名以第一个文件为准（反向：把顺序倒过来，胜者随之改变）。
#[test]
fn 包名以第一个文件为准() {
    let mut tt = TypeTable::new();
    let srcs = src(&[("a.go", "package q\n"), ("b.go", "package p\n")]);
    let (pkg, bag) = load_sources(&mut tt, &borrowed(&srcs), None);
    let diags = bag.into_sorted();
    assert_eq!(diags.len(), 1);
    assert_eq!(pkg.name, "q");
    assert_eq!(pkg.file_table.name(diags[0].pos.file), "b.go");
}

/// 三个文件：中间那个不一致，两侧仍然同包（不一致不是「其余全部作废」）。
#[test]
fn 不一致的文件只影响它自己() {
    let mut tt = TypeTable::new();
    let srcs = src(&[
        ("a.go", "package p\n\nvar A int\n"),
        ("b.go", "package q\n\nvar B int\n"),
        ("c.go", "package p\n\nvar C int\n"),
    ]);
    let (pkg, bag) = load_sources(&mut tt, &borrowed(&srcs), None);
    assert_eq!(bag.into_sorted().len(), 1);
    assert_eq!(pkg.file_count(), 2);
    let names: Vec<&str> = pkg.files.iter().map(|f| pkg.file_table.name(*f)).collect();
    assert_eq!(names, vec!["a.go", "c.go"]);
    assert!(pkg.lookup("A").is_some() && pkg.lookup("C").is_some());
    assert!(pkg.lookup("B").is_none());
}

/// 四种顶层声明都进包作用域（`func` 有接收者时**不**进）。
#[test]
fn 四类顶层声明与方法的归属() {
    let mut tt = TypeTable::new();
    let srcs = src(&[(
        "a.go",
        "package p\n\ntype T int\nconst C = 1\nvar V int\nfunc F() {}\nfunc (T) M() {}\n",
    )]);
    let (pkg, bag) = load_sources(&mut tt, &borrowed(&srcs), None);
    assert!(bag.into_sorted().is_empty(), "语法本身合法");

    for (n, k) in [
        ("T", ObjKind::TypeName),
        ("C", ObjKind::Const),
        ("V", ObjKind::Var),
        ("F", ObjKind::Func),
    ] {
        assert_eq!(pkg.lookup(n).map(|o| o.kind), Some(k), "{n}");
    }
    assert!(
        pkg.lookup("M").is_none(),
        "方法名属于接收者的方法集，不进包作用域（M3 不做方法集）"
    );
}

/// 同层重名：**先声明者胜**（确定性），且被丢掉的那个不覆盖先来的。
#[test]
fn 同层重名先声明者胜() {
    let mut tt = TypeTable::new();
    let srcs = src(&[
        ("a.go", "package p\n\ntype T int\n"),
        ("b.go", "package p\n\nvar T int\n"),
    ]);
    let (pkg, _bag) = load_sources(&mut tt, &borrowed(&srcs), None);
    assert_eq!(
        pkg.lookup("T").map(|o| o.kind),
        Some(ObjKind::TypeName),
        "a.go 的 T 胜（Task 25 会在这里报 DuplicateDecl）"
    );
    assert_eq!(
        pkg.file_table
            .name(pkg.lookup("T").unwrap().pos.unwrap().file),
        "a.go"
    );
}

/// 占位对象**没有**类型与值（那两样归 Task 24 求值）。
#[test]
fn 占位对象没有类型与值() {
    let mut tt = TypeTable::new();
    let srcs = src(&[("a.go", "package p\n\ntype T int\nconst C = 1\n")]);
    let (pkg, _bag) = load_sources(&mut tt, &borrowed(&srcs), None);
    let t = pkg.lookup("T").unwrap();
    assert_eq!(t.typ, None, "类型求值归 Task 24");
    assert_eq!(t.val, None);
    assert!(t.pos.is_some(), "但有位置（诊断要用）");
    assert!(!t.has_value());
}

/// `T-TY-52` 的**盘上**路径：显式文件列表（`D-M3-14`）能真的读文件，
/// 且 `FileId` 按传入顺序唯一 —— `T-TY-66` 能指认文件的前提。
#[test]
fn 显式文件列表读盘且_file_id_有序() {
    let dir = std::env::temp_dir().join(format!("rgoc-load-t23-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.go");
    let b = dir.join("b.go");
    fs::write(&a, "package p\n\ntype T int\n").unwrap();
    fs::write(&b, "package p\n\nvar x T\n").unwrap();

    let mut tt = TypeTable::new();
    let (pkg, bag) = load_from_explicit_files(&mut tt, &[a.clone(), b.clone()], None);

    assert!(bag.into_sorted().is_empty());
    assert_eq!(pkg.file_count(), 2);
    assert!(pkg.skipped.is_empty());
    // 顺序 == 传入顺序；两个文件号不同（Ruling 23-4）
    assert_ne!(pkg.files[0], pkg.files[1]);
    assert!(pkg.file_table.name(pkg.files[0]).ends_with("a.go"));
    assert!(pkg.file_table.name(pkg.files[1]).ends_with("b.go"));
    assert_eq!(
        pkg.lookup_in_file(pkg.files[1], "T").map(|o| o.kind),
        Some(ObjKind::TypeName)
    );

    // 语料/环境不变：读不到的文件如实记录，不伪造诊断（无对应代号）
    let gone = dir.join("nope.go");
    let (pkg2, bag2) = load_from_explicit_files(&mut tt, &[gone], None);
    assert!(bag2.into_sorted().is_empty(), "没有代号 ⇒ 不报诊断");
    assert_eq!(pkg2.skipped.len(), 1);
    assert!(pkg2.skipped[0].0.ends_with("nope.go"));
    assert_eq!(pkg2.file_count(), 0);

    fs::remove_dir_all(&dir).ok();
}
