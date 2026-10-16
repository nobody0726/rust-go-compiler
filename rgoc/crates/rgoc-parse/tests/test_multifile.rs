//! 多文件解析：`parse_file_in` 必须把每个文件登记进**共享**文件表。
//!
//! 这是 `Task 23` Ruling 23-4 在 **parser** 这一侧的可执行判据
//! （词法侧在同名测试在 `rgoc-lex/tests/test_filetable.rs`）。

use rgoc_lex::FileTable;
use rgoc_parse::parse_file_in;

#[test]
fn parse_file_in_让两个文件落在不同_file_id_上() {
    let mut table = FileTable::new();
    let a = parse_file_in(&mut table, b"package p\n\ntype T int\n", "a.go");
    let b = parse_file_in(&mut table, b"package p\n\nvar x T\n", "b.go");

    let fa = a.file.as_ref().expect("a.go 解析成功");
    let fb = b.file.as_ref().expect("b.go 解析成功");
    assert_eq!(fa.pos.file.index(), 0);
    assert_eq!(fb.pos.file.index(), 1, "b.go 必须落在 1 号（共享表生效）");
    assert_eq!(table.name(fb.pos.file), "b.go");
    // 文件**内部**的位置也要带正确的 file（不只是文件起点）
    assert_eq!(
        fb.pkg_name.pos.file, fb.pos.file,
        "包名标识符的位置同属本文件"
    );
    assert!(!fb.decls.is_empty(), "b.go 有声明");
}
