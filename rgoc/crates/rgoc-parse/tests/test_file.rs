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
