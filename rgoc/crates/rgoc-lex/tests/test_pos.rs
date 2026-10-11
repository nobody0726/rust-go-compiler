//! T-L01 的验收测试 —— C1 契约的位置表示。
//!
//! 判据来源见 `docs/milestones/M1-tests.md`。四条断言分别钉住：
//! 「同名字得同 Id」「格式由文件表提供」「排序键顺序」「他用 Id 必须炸而不是静默」。

use rgoc_lex::{FileId, FileTable, Pos};

#[test]
fn file_table_同名字重复_intern_得到同一个_id() {
    let mut t = FileTable::new();
    let a = t.intern("a.go");
    let b = t.intern("a.go");
    let c = t.intern("b.go");

    assert_eq!(a, b, "同一个名字必须得到同一个 FileId");
    assert_ne!(a, c, "不同名字必须得到不同 FileId");
    assert_eq!(t.name(a), "a.go");
    assert_eq!(t.name(c), "b.go");
    assert_eq!(a.index(), 0);
    assert_eq!(c.index(), 1);

    // 批量重复 intern 必须幂等 —— 用上 FileId 这个类型名本身
    let ids: Vec<FileId> = (0..3).map(|_| t.intern("a.go")).collect();
    assert!(ids.iter().all(|&id| id == a), "重复 intern 不得产生新 Id");
}

#[test]
fn format_pos_形如_文件_行_列() {
    let mut t = FileTable::new();
    let f = t.intern("test/helloworld.go");
    let p = Pos {
        file: f,
        line: 3,
        col: 7,
    };
    // C1 §2.2：这条格式必须与 oracle 的「文件:行号」前缀兼容
    assert_eq!(t.format_pos(p), "test/helloworld.go:3:7");
}

#[test]
fn pos_的_ord_顺序是_文件_行_列() {
    let mut t = FileTable::new();
    let a = t.intern("a.go");
    let b = t.intern("b.go");

    let mut v = [
        Pos {
            file: b,
            line: 1,
            col: 1,
        },
        Pos {
            file: a,
            line: 2,
            col: 1,
        },
        Pos {
            file: a,
            line: 1,
            col: 9,
        },
        Pos {
            file: a,
            line: 1,
            col: 2,
        },
    ];
    v.sort();

    // C1 §4 的排序键：file → line → col（code 是 Diag 才有的第 4 键）
    assert_eq!(
        v[0],
        Pos {
            file: a,
            line: 1,
            col: 2
        }
    );
    assert_eq!(
        v[1],
        Pos {
            file: a,
            line: 1,
            col: 9
        }
    );
    assert_eq!(
        v[2],
        Pos {
            file: a,
            line: 2,
            col: 1
        }
    );
    assert_eq!(
        v[3],
        Pos {
            file: b,
            line: 1,
            col: 1
        }
    );
}

#[test]
#[should_panic(expected = "不属于本表")]
fn 越界的_file_id_必须炸而不是返回空名() {
    // 静默返回 "" 会让错误位置变成「<空>:3:7」—— 看起来正常，其实丢了文件。
    // 这条断言把那个失败模式钉死。
    //
    // 注意：本测试只覆盖【越界】。跨表混用在 `FileId` 只是下标时**检测不出来**
    // （两张表的 FileId(0) 相等）—— 那是已知局限，见 `pos.rs` 的模块文档。
    let mut t = FileTable::new();
    let _ = t.intern("a.go");
    let _ = t.name(FileId::from_index(99));
}
