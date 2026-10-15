//! T-A-20 —— `fileOrNil` 骨架的可执行副本。

use rgoc_parse::parse_file;

#[test]
fn t_a_20_package_clause() {
    let r = parse_file(b"package main\n", "t.go");
    let f = r.file.expect("应解析成功");
    assert_eq!(f.pkg_name.value, "main");
    assert!(f.decls.is_empty());
    assert!(r.diags.is_empty(), "无诊断，got {:?}", r.diags);
}

#[test]
fn t_a_20_package_with_trailing_semi() {
    let r = parse_file(b"package p;", "t.go");
    let f = r.file.expect("应解析成功");
    assert_eq!(f.pkg_name.value, "p");
    assert!(f.decls.is_empty());
}

/// 缺 `package` 关键字 ⇒ 报 `package statement must be first` 并**放弃整个文件**
/// （官方 `parser.go:417-420` 的 `return nil`）。
///
/// 骨架版（T-AP05）在这一格是「有就跳过、没有也照读」—— 于是**任何**文件都能
/// 解析出 `AstFile`，坏输入与好输入在 `file.is_none()` 这个判据上分不开。
#[test]
fn t_a_20_缺_package_关键字则报错并放弃() {
    let r = parse_file(b"import \"fmt\"\n", "t.go");
    assert!(r.file.is_none(), "官方直接 `return nil`，got {:?}", r.file);
    let first = r.first.as_ref().expect("首错必须留下");
    assert!(
        first.message.contains("package statement must be first"),
        "got {first:?}"
    );
}

/// package 子句里**有任何错**就放弃（官方 `parser.go:425-428`：
/// `don't bother continuing if package clause has errors`）。
///
/// ⚠️ 这条判据的价值在于**树的有无**，不是诊断条数：不做这一步，坏文件会一路
/// 产出顶层声明 —— 对撞时表现为「我们多了一棵树」，而两边诊断数可能一样。
#[test]
fn t_a_20_package_子句有错则整个文件放弃() {
    // `package` 后直接跟 `import`（缺包名）⇒ `expected name` + `expected ;`
    let r = parse_file(b"package\nimport \"fmt\"\n", "t.go");
    assert!(
        r.file.is_none(),
        "package 子句有错 ⇒ 官方 `return nil`，got {:?}",
        r.file
    );
    assert!(
        r.errors >= 2,
        "至少两条（缺包名 + 缺分号），got {}",
        r.errors
    );
}
