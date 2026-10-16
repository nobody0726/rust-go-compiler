//! `T-TY-52` 的前置：**多文件共享一张 `FileTable`**（Task 23 Ruling 23-4）。
//!
//! # 为什么需要这个测试
//!
//! `Lexer::new` 每次自带一张空表 ⇒ 每个文件的 `FileId` 都是 `0`。
//! 单文件时无害，**多文件时是错的**：`C1` §4 的诊断排序**第一个键就是 `file`**，
//! 所有文件挤在 `FileId(0)` 上 ⇒ 跨文件的诊断顺序退化成「按行号排」，
//! 而「指不出是哪个文件」还会让 `T-TY-66`（包名不一致）无法定位。
//!
//! `pos.rs` 的 `FileId` 文档早就记下了这个坑，并给了两条出路
//! （共享表 / `FileId` 加表标识）。这里走**共享表**：不动 `FileId` 的表示。

use rgoc_lex::{FileTable, Lexer};

#[test]
fn 共享文件表让两个文件拿到不同_file_id() {
    let mut table = FileTable::new();
    let a = Lexer::new_in(&mut table, b"package a\n".to_vec(), "a.go");
    let b = Lexer::new_in(&mut table, b"package b\n".to_vec(), "b.go");

    let (ia, ib) = (a.file(), b.file());
    assert_ne!(ia, ib, "两个文件必须落在不同的 FileId 上");
    assert_eq!(table.name(ia), "a.go");
    assert_eq!(table.name(ib), "b.go");
    // 词法位置也带上正确的文件号（诊断排序的第一键）
    let pos = a.pos();
    assert_eq!(pos.file, ia, "a 的位置指向 a.go");
}

#[test]
fn 同名文件重复登记得到同一个_id() {
    let mut table = FileTable::new();
    let a = Lexer::new_in(&mut table, b"package a\n".to_vec(), "x.go");
    let b = Lexer::new_in(&mut table, b"package b\n".to_vec(), "x.go");
    assert_eq!(
        a.file(),
        b.file(),
        "同名字 ⇒ 同一个 FileId（FileTable::intern）"
    );
}

#[test]
fn 自带表的旧入口行为不变() {
    // 回归：`new` 仍是「自己一张表、第一个文件」
    let a = Lexer::new(b"package a\n".to_vec(), "a.go");
    let b = Lexer::new(b"package b\n".to_vec(), "b.go");
    assert_eq!(a.file(), b.file(), "各自一张表 ⇒ 都是 0");
    assert_eq!(
        a.table().name(a.file()),
        "a.go",
        "本地表仍解释得了自己的位置"
    );
}

#[test]
fn 共享表下第二个文件的位置不再是_file_id_0() {
    // 共享表之前不存在的东西：**第二个**文件的位置带 1 号文件。
    // 这也是 `Parser::new` 里那个哨兵 `Pos`（曾写死 `FileId::from_index(0)`）
    // 必须改成「本文件的 id」的理由 —— 否则共享表下 0 号是**别的文件**。
    let mut table = FileTable::new();
    let _first = Lexer::new_in(&mut table, b"package a\n".to_vec(), "a.go");
    let second = Lexer::new_in(&mut table, b"package b\n".to_vec(), "b.go");
    let pos = second.pos();
    assert_eq!(pos.file.index(), 1, "b.go 不是 0 号文件");
    assert_eq!(table.name(pos.file), "b.go");
}
