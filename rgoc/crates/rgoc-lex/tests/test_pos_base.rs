//! `T-AP15` 的第一半：`PosBase` 与 `Rel*`（`//line` 重映射的**表示层**）。
//!
//! # 为什么要动 M1 冻结的 `Pos`（用户 2026-10-15 拍板）
//!
//! M1 的 `Pos` 是 `{file, line, col}` —— **没有 base 概念**；而官方的 `Pos` 是
//! `{base *PosBase, line, col}`，`updateBase` / `RelLine` / `RelCol` 全建立在
//! base 链上。照抄官方的 `//line` 语义就得先有 base，否则「重映射后的位置」
//! 无处安放。
//!
//! # 与官方的对应关系
//!
//! | 官方 | 我们 | 说明 |
//! |---|---|---|
//! | `Pos{base, line, col}` | `Pos { base: Option<BaseId>, file: FileId, line, col }` | **`file` 保留** —— 它是 `FileBase()` 的信息（官方 `pos.go:40-47` 沿 base 链回溯的终点） |
//! | `NewFileBase(filename)` | [`PosBases::file_base`] | 链的根，**自指** |
//! | `NewLineBase(pos, filename, trimmed, line, col)` | [`PosBases::line_base`] | 挂一条新 base |
//! | `p.posAt(line, col)` = `MakePos(p.base, …)` | 本文件的 [`at`] | **携带当前 base** |
//! | `pos.RelFilename()` | [`Pos::rel_filename`] | base 为空 ⇒ 退回 `file` |
//! | `pos.RelLine()` | [`Pos::rel_line`] | `b.line + (pos.line - b.pos.line)` |
//! | `pos.RelCol()` | [`Pos::rel_col`] | 同行才做列偏移，否则 `b.col` |
//! | `base.IsFileBase()` | [`PosBases::is_file_base`] | `base.pos.base == base` |
//!
//! # 官方那几条「列偏移为 0 ⇒ 未知」的分支，本仓**照搬**
//!
//! `pos.go:53-56` 的 `b.Line() == 0 ⇒ RelLine 返回 0`、`pos.go:62-68` 的
//! `b.Col() == 0 ⇒ RelCol 返回 0`：官方在 `//line foo:123`（无列）时把 `col`
//! 存成 **0** 而不是 1，于是 `RelCol()` 对**后续所有行**都返回 **0**（未知）——
//! 这不是 bug 而是官方规定的语义（注释原文：*"the current specification for
//! line directives requires this to apply until the next PosBase/line directive,
//! not just until the new newline"*）。照搬它，`//line foo:123` 之后的节点列号
//! 会是 0 —— 看着像 bug，其实是**对的行为**。
//!
//! ⚠️ file base 的 `line`/`col` 必须是 **`1`/`1`**（官方 `linebase`/`colbase`，
//! `pos.go:162`）而不是 0 —— 否则「无 `//line` 时 rel 退回绝对」这条不变量不成立：
//! `RelLine() = b.line + (pos.line - b.pos.line) = 1 + (line - 1) = line`。
//! 本文件的 [`无_line_指令时_rel_等于绝对`] 就是这条不变量的守卫。
//!
//! 权威判据：官方 `parser_test.go:325-338` 的 `/*line foo:123*/` ⇒ `{123, 0}`、
//! `/*line :x:1:10*/` ⇒ `{1, 10}`。

use rgoc_lex::{FileId, FileTable, Pos, PosBases};

/// 官方 `p.posAt(line, col)` = `MakePos(p.base, line, col)` —— **携带当前 base**。
///
/// ⚠️ 这不是「方便」而是**语义**：官方的 `Pos` 在构造那一刻就把当时的 base 钉进去，
/// 所以 base 之后变了也不影响已造好的 `Pos`。本文件的每条用例都靠它
/// （例如「先在第 1 行挂 `foo:123` 的基准，再问第 5 行的 rel」）。
#[track_caller]
fn at(bs: &PosBases, f: FileId, line: u32, col: u32) -> Pos {
    Pos {
        base: bs.current(),
        file: f,
        line,
        col,
    }
}

#[test]
fn 无_line_指令时_rel_等于绝对() {
    let mut ft = FileTable::new();
    let mut bs = PosBases::new();
    ft.intern("dir/directives.go");
    bs.file_base("dir/directives.go");
    let f = ft.intern("dir/directives.go");

    // 没有任何 `//line` ⇒ 该 `Pos` 的 base 是那个**自指的 file base**
    // ⇒ `b.line == 0` / `b.col == 0` 两条「未知」分支成立 ⇒ rel 就是绝对。
    let p = at(&bs, f, 123, 7);
    assert_eq!(p.rel_filename(&ft, &bs), "dir/directives.go");
    assert_eq!(
        p.rel_line(&bs),
        123,
        "file base 的 line 是 0 ⇒ 退回绝对行号"
    );
    assert_eq!(p.rel_col(&bs), 7, "file base 的 col 是 0 ⇒ 退回绝对列号");
}

#[test]
fn line_指令_重映射行号与文件名() {
    let mut ft = FileTable::new();
    let mut bs = PosBases::new();
    ft.intern("dir/directives.go");
    bs.file_base("dir/directives.go");
    let f = ft.intern("dir/directives.go");

    // 官方 `parser_test.go:325`：`/*line foo:123*/` 在第 1 行
    // ⇒ base 的 pos 是「紧跟注释之后」= (1, 16)，目标行 123、**列未知(0)**。
    // ⚠️ `pos` 携带的是**挂之前**的 base（官方 `updateBase` 的 `pos` 实参由
    // `p.posAt` 在 `p.base = NewLineBase(...)` **之前**求值）。
    let p = at(&bs, f, 1, 16);
    bs.line_base(p, "foo", false, 123, 0);

    // 注释**之后**那个位置就是新基准本身。
    let q = at(&bs, f, 1, 16);
    assert_eq!(
        q.rel_line(&bs),
        123,
        "rel_line = b.line + (pos.line - b.pos.line) = 123 + (1-1)"
    );
    assert_eq!(q.rel_filename(&ft, &bs), "foo");
    assert_eq!(
        q.rel_col(&bs),
        0,
        "官方 `b.Col() == 0 ⇒ RelCol 返回 0`（未知）—— 见 `parser_test.go:325` 期望的「行 123、列 0」"
    );
}

#[test]
fn 列_在有列基准时按_offset_累加() {
    let mut ft = FileTable::new();
    let mut bs = PosBases::new();
    ft.intern("dir/directives.go");
    bs.file_base("dir/directives.go");
    let f = ft.intern("dir/directives.go");

    // 官方 `parser_test.go:334`：`/*line :x:1:10*/` ⇒ `{1, 10}`
    let p = at(&bs, f, 1, 9);
    bs.line_base(p, ":x", false, 1, 10);

    let same_line = at(&bs, f, 1, 9);
    assert_eq!(same_line.rel_line(&bs), 1);
    assert_eq!(same_line.rel_col(&bs), 10, "同行 ⇒ 列相对 base 偏移");

    // 再往后一行：官方 `RelCol` 在 `pos.Line() != b.Pos().Line()` 时返回
    // **`pos.Col()`（绝对列）**，见 `pos.go:73`。
    let next_line = at(&bs, f, 2, 3);
    assert_eq!(next_line.rel_line(&bs), 2, "1 + (2 - 1)");
    assert_eq!(
        next_line.rel_col(&bs),
        3,
        "不同行 ⇒ **绝对列** `pos.col`（`pos.go:73`），不是 `b.col`"
    );
}

#[test]
fn 后续行沿用同一条_line_指令的基准() {
    let mut ft = FileTable::new();
    let mut bs = PosBases::new();
    ft.intern("dir/directives.go");
    bs.file_base("dir/directives.go");
    let f = ft.intern("dir/directives.go");

    // `/*line foo:100*/` 在第 1 行 ⇒ 第 5 行落在 rel 104
    let p = at(&bs, f, 1, 16);
    bs.line_base(p, "foo", false, 100, 0);

    let q = at(&bs, f, 5, 1);
    assert_eq!(q.rel_line(&bs), 100 + (5 - 1), "偏移量沿用，不重置");
    assert_eq!(q.rel_filename(&ft, &bs), "foo");
}

/// **「空文件名沿用上一个」是 parser 侧的职责**（官方 `updateBase:161-164`
/// 的 `if filename == "" && ok2 { filename = p.base.Filename() }`），
/// [`PosBases`] 只**存**给定的那一份。
///
/// ⚠️ 所以这两条用例**不断言沿用**，只断言「原样存下来」—— 沿用与否由
/// `test_directive.rs` 的端到端用例照（那里才有 `ok2` 这个判据）。
/// 在这里断言沿用会造出一个「`PosBases` 自己会去查上一条」的**私货**语义：
/// 那会让 `update_base` 里的 `ok2` 判据**永真**（沿用总是发生），
/// 官方那条「无 `:col` 时留空」的分支就再也照不出来了。
#[test]
fn base_只原样存_沿用是_parser_侧的事() {
    let mut ft = FileTable::new();
    let mut bs = PosBases::new();
    ft.intern("bar");
    bs.file_base("bar");
    let f = ft.intern("bar");

    bs.line_base(at(&bs, f, 1, 8), "bar", false, 1, 0);
    let empty = bs.line_base(at(&bs, f, 2, 10), "", false, 10, 20);

    assert_eq!(
        bs.get(empty).expect("刚挂的 base").filename,
        "",
        "空文件名**原样存** —— 沿用由 `update_base` 决定"
    );
    // 非空则原样是它自己
    let named = bs.line_base(at(&bs, f, 3, 10), "x", false, 5, 0);
    assert_eq!(bs.get(named).expect("刚挂的 base").filename, "x");
}

#[test]
fn 第二条_line_指令_换掉第一条() {
    let mut ft = FileTable::new();
    let mut bs = PosBases::new();
    ft.intern("dir/directives.go");
    bs.file_base("dir/directives.go");
    let f = ft.intern("dir/directives.go");

    // 官方 `parser_test.go:326`：`/*line foo:123*/\n//line bar:345\nfoo`
    // ⇒ 最终 base 是 `bar:345`，而 `foo` 在**第三条**行 ⇒ rel = 345 + (3 - 2) = 346。
    let p1 = at(&bs, f, 1, 16);
    bs.line_base(p1, "foo", false, 123, 0);
    let p2 = at(&bs, f, 2, 14);
    bs.line_base(p2, "bar", false, 345, 0);

    let q = at(&bs, f, 3, 1);
    assert_eq!(q.rel_filename(&ft, &bs), "bar", "第二条换掉第一条");
    assert_eq!(q.rel_line(&bs), 346, "相对**最新**那条基准算");
}

#[test]
fn is_file_base_只对_file_base_为真() {
    let mut ft = FileTable::new();
    let mut bs = PosBases::new();
    let f = ft.intern("dir/directives.go");
    let file_base = bs.file_base("dir/directives.go");
    let line_base = bs.line_base(at(&bs, f, 1, 16), "foo", false, 123, 0);

    assert!(
        bs.is_file_base(file_base),
        "file base 的 `pos.base` 自指自己（官方 `pos.go:180`）"
    );
    assert!(
        !bs.is_file_base(line_base),
        "line base 挂在 file base 下 ⇒ 不是 file base"
    );
}

#[test]
fn trimmed_被记下来() {
    let mut ft = FileTable::new();
    let mut bs = PosBases::new();
    ft.intern("dir/directives.go");
    bs.file_base("dir/directives.go");
    let f = ft.intern("dir/directives.go");
    let t = bs.line_base(at(&bs, f, 1, 16), "foo", true, 123, 0);
    assert!(
        bs.trimmed(t),
        "`//line :10:20` 的空文件名会把 trimmed 一并沿用"
    );
    let u = bs.line_base(at(&bs, f, 2, 16), "bar", false, 1, 0);
    assert!(!bs.trimmed(u), "给了新文件名 ⇒ trimmed 复位");
}

#[test]
fn base_表按插入序增长_当前指向最后一条() {
    let mut ft = FileTable::new();
    let mut bs = PosBases::new();
    assert!(bs.is_empty());
    assert!(bs.current().is_none());

    // 先 `intern` 再建 file base —— `PosBases::file_base` 不碰表（见它的文档），
    // 但 `Pos.file` 是**表下标**，两边必须落在同一张表上。
    ft.intern("a.go");
    let f0 = bs.file_base("a.go");
    assert_eq!(bs.len(), 1);
    assert_eq!(bs.current(), Some(f0));

    let f = ft.intern("a.go");
    let l1 = bs.line_base(at(&bs, f, 1, 1), "x", false, 5, 0);
    assert_eq!(bs.len(), 2);
    assert_eq!(bs.current(), Some(l1));

    let l2 = bs.line_base(at(&bs, f, 2, 1), "y", false, 6, 0);
    assert_eq!(bs.len(), 3, "旧基准**留在表里**（官方指针链也不释放）");
    assert_eq!(bs.current(), Some(l2));
    assert!(bs.is_file_base(f0), "根永远可辨认");
}
