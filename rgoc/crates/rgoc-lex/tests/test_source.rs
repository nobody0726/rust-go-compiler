//! `SourceReader` 的验收测试（T-L06，`M1-design.md` §5.1）。
//!
//! # 这个类型只有三条真语义
//!
//! 1. **`(line, col)` 是「当前 `ch` 的位置」**，而不是"下一个待读的位置"。
//!    官方 `source.pos()` 返回 `(linebase+line, colbase+col)`，`linebase = colbase = 1`。
//! 2. **`col` 的单位是字节**（G4）。`变量` 占 6 列；`😀` 占 4 列。
//! 3. **`segment()` 不含当前 `ch`**（`buf[b : r-chw]`）——
//!    "切出来的词元天然不含终止符"就是这条的直接后果。
//!
//! # 期望值全部来自 oracle，不是推的
//!
//! 下面中文 / emoji 两条用例的位置，都是用 `tools/token-oracle` 对同样的源码跑出来的
//! （`变量 := 1` ⇒ `:=` @ **1:8**；`x = "😀"` ⇒ 字面量 @ **1:5**，分号 @ **1:11**）。
//! 其中一个数字还纠正了计划里的笔误：计划写「`:=` 的起点 @1:7」——
//! 那是**漏算了一个空格**（`变量` 6 字节 + 空格 1 字节），实测是 **1:8**。
//!
//! **光看"列号差 1"是发现不了这种错的** —— 所以位置类断言必须有 oracle 对照。

use rgoc_lex::{DiagCode, FileId, Pos, SourceReader};

fn reader(src: &[u8]) -> SourceReader {
    SourceReader::new(src.to_vec(), FileId::from_index(0))
}

/// 走一遍并收集**诊断**（位置 + 代号）。
///
/// 与 `walk()` 分开：诊断是"旁路产物"，与字符序列是两条不同的流。
/// 只断言其中一条就会漏掉一半行为（本仓对负例的纪律：诊断与后续词元**两条都要断言**）。
fn diags_of(src: &[u8]) -> Vec<(u32, u32, DiagCode)> {
    let mut r = reader(src);
    let mut out = Vec::new();
    loop {
        for d in r.advance() {
            out.push((d.pos.line, d.pos.col, d.code));
        }
        if r.ch().is_none() {
            break;
        }
    }
    out
}

/// 逐字符走一遍，返回 `(字符, 行, 列, 字节宽度)`。
///
/// 刻意**不**包含 EOF 那一次（`ch() == None`）—— 用单独的用例钉它。
fn walk(src: &[u8]) -> Vec<(char, u32, u32, u8)> {
    let mut r = reader(src);
    let mut out = Vec::new();
    loop {
        let _ = r.advance();
        match r.ch() {
            Some(c) => {
                let p = r.pos();
                out.push((c, p.line, p.col, r.chw()));
            }
            None => break,
        }
    }
    out
}

// ── ① 位置推进 ─────────────────────────────────────────────────────

#[test]
fn 纯_ascii_逐字符的位置() {
    // 计划给的就是这条：a@1:1 b@1:2 换行@1:3 c@2:1
    assert_eq!(
        walk(b"ab\nc"),
        vec![
            ('a', 1, 1, 1),
            ('b', 1, 2, 1),
            ('\n', 1, 3, 1),
            ('c', 2, 1, 1)
        ]
    );
}

#[test]
fn 换行让行号加一且列归零() {
    // 连续两个换行：第二行是空的，第三行从 3:1 开始
    assert_eq!(
        walk(b"a\n\nb"),
        vec![
            ('a', 1, 1, 1),
            ('\n', 1, 2, 1),
            ('\n', 2, 1, 1),
            ('b', 3, 1, 1),
        ]
    );
    // 以换行开头：第一行只有那个换行
    assert_eq!(walk(b"\na"), vec![('\n', 1, 1, 1), ('a', 2, 1, 1)]);
}

#[test]
fn 中文标识符让列按字节前进() {
    // oracle 实测：`变量` @1:1（6 字节），`:=` @1:8
    assert_eq!(
        walk("变量 := 1\n".as_bytes()),
        vec![
            ('变', 1, 1, 3),
            ('量', 1, 4, 3),
            (' ', 1, 7, 1),
            (':', 1, 8, 1),
            ('=', 1, 9, 1),
            (' ', 1, 10, 1),
            ('1', 1, 11, 1),
            ('\n', 1, 12, 1),
        ]
    );
}

#[test]
fn emoji_占四个字节列() {
    // oracle 实测：字面量 @1:5（开引号）、分号 @1:11 ⇒ `"😀"` 占 5..10 共 6 列
    assert_eq!(
        walk("x = \"😀\"\n".as_bytes()),
        vec![
            ('x', 1, 1, 1),
            (' ', 1, 2, 1),
            ('=', 1, 3, 1),
            (' ', 1, 4, 1),
            ('"', 1, 5, 1),
            ('😀', 1, 6, 4),
            ('"', 1, 10, 1),
            ('\n', 1, 11, 1),
        ]
    );
}

#[test]
fn 中文字符是字母且宽度为三() {
    // oracle 实测 `a中b` 整体是一个 name ⇒ `中` 必须被当作字母（D-M1-6 的表）
    let got = walk("a中b".as_bytes());
    assert_eq!(got[1], ('中', 1, 2, 3));
    assert!(rgoc_lex::unicode_tables::is_letter('中'));
}

// ── ② 段（segment）─────────────────────────────────────────────────

#[test]
fn segment_不含当前字符() {
    let mut r = reader(b"abc");
    let _ = r.advance(); // ch = 'a'
    r.start_segment(); // 段从 'a' 开始
    assert_eq!(r.segment(), b"", "刚开段时当前字符还不算进去");

    let _ = r.advance(); // ch = 'b'
    assert_eq!(r.segment(), b"a");

    let _ = r.advance(); // ch = 'c'
    assert_eq!(r.segment(), b"ab");

    let _ = r.advance(); // EOF —— 段到文件尾
    assert_eq!(r.segment(), b"abc");
    assert_eq!(r.ch(), None);
}

#[test]
#[should_panic(expected = "先调用 start_segment")]
fn 没开段就取_segment_必须炸而不是返回空() {
    // 静默返回空切片会让"空标识符"这种不可能的东西看起来正常
    let r = reader(b"abc");
    let _ = r.segment();
}

// ── ③ rewind ──────────────────────────────────────────────────────

#[test]
fn rewind_回到段起点并重读段首字符() {
    // 模拟 `..` 的用法：段从第一个 `.` 开始，读到第二个 `.` 之后回退
    let mut r = reader(b"..x");
    r.start_segment(); // b = 0
    let _ = r.advance(); // ch = 第 1 个 '.'
    assert_eq!(
        r.pos(),
        Pos {
            file: FileId::from_index(0),
            line: 1,
            col: 1
        }
    );
    let _ = r.advance(); // ch = 第 2 个 '.'
    assert_eq!(
        r.pos(),
        Pos {
            file: FileId::from_index(0),
            line: 1,
            col: 2
        }
    );
    assert_eq!(r.segment(), b".");

    let _ = r.rewind();

    assert_eq!(r.ch(), Some('.'), "rewind 之后段首字符必须被重新读出来");
    assert_eq!(
        r.pos(),
        Pos {
            file: FileId::from_index(0),
            line: 1,
            col: 1
        }
    );
    assert_eq!(r.chw(), 1);
    assert_eq!(r.segment(), b"", "回到段首 ⇒ 段又变空");

    // 再往前读，应当与第一次完全一致（这才是"回退"的意义）
    let _ = r.advance();
    assert_eq!(r.ch(), Some('.'));
    assert_eq!(
        r.pos(),
        Pos {
            file: FileId::from_index(0),
            line: 1,
            col: 2
        }
    );
}

#[test]
#[should_panic(expected = "rewind")]
fn 没有活动段时_rewind_必须炸() {
    let mut r = reader(b"..");
    let _ = r.rewind();
}

// ── ④ EOF ─────────────────────────────────────────────────────────

#[test]
fn eof_时_ch_为_none_且_chw_为零() {
    let mut r = reader(b"a");
    let _ = r.advance();
    assert_eq!(r.ch(), Some('a'));
    assert_eq!(r.chw(), 1);

    let _ = r.advance(); // 越过后到 EOF
    assert_eq!(r.ch(), None);
    assert_eq!(r.chw(), 0, "EOF 的宽度是 0 —— 位置不再前进");

    // EOF 之后继续 advance 是幂等的
    let before = r.pos();
    let _ = r.advance();
    assert_eq!(r.ch(), None);
    assert_eq!(r.pos(), before);
}

#[test]
fn 空文件一开始就是_eof() {
    let mut r = reader(b"");
    let _ = r.advance();
    assert_eq!(r.ch(), None);
    assert_eq!(r.chw(), 0);
    assert_eq!(r.pos().line, 1);
    assert_eq!(r.pos().col, 1);
}

// ── ⑤ 诊断必须交出去，不能就地丢掉（P1 裁决的修正）──────────────────

#[test]
fn advance_把诊断交给调用方() {
    // NUL：官方的 nextch 报错后 `goto redo` 继续读 —— 诊断不是"返回值"，是旁路产物
    let mut r = reader(b"x\0y");
    let _ = r.advance(); // 'x'
    let diags = r.advance(); // 跳过 NUL
    assert_eq!(diags.len(), 1, "一个 NUL 一条诊断");
    assert_eq!(diags[0].code, rgoc_lex::DiagCode::InvalidNul);
    assert_eq!(diags[0].pos.col, 2, "诊断位置是被污染的那个字节");
    assert_eq!(r.ch(), Some('y'), "报错之后必须继续，不是停下");
}

#[test]
fn 一次推进可以产出多条诊断() {
    // ★ 这条是 P1 裁决（`advance() -> Option<Diag>`）被实测推翻的证据。
    //
    // 官方 `nextch` 的 `redo:` 标签在**位置推进之前**，所以连续两个 NUL 会在
    // **同一次** nextch 里各自报一次错。用 `Option<Diag>` 只能带回一条 ⇒
    // 第二条被**静默丢弃**。
    // oracle 实测（`x\0\0y`）：`1:2 LEX-INVALID-NUL` 与 `1:3 LEX-INVALID-NUL` **两条**。
    let mut r = reader(b"x\0\0y");
    let _ = r.advance(); // 'x'
    let diags = r.advance();
    assert_eq!(diags.len(), 2, "两个 NUL 必须带回两条诊断");
    assert_eq!(diags[0].code, rgoc_lex::DiagCode::InvalidNul);
    assert_eq!(diags[1].code, rgoc_lex::DiagCode::InvalidNul);
    assert_eq!(
        (diags[0].pos.col, diags[1].pos.col),
        (2, 3),
        "两条诊断的位置分别是两个 NUL 自己"
    );
    assert_eq!(r.ch(), Some('y'));
}

#[test]
fn 非法_utf8_也走同一条通道() {
    // oracle 实测 `x\xff\xffy` ⇒ 两条 LEX-INVALID-UTF8 @1:2 @1:3
    let mut r = reader(b"x\xff\xffy");
    let _ = r.advance();
    let diags = r.advance();
    assert_eq!(diags.len(), 2);
    assert!(
        diags
            .iter()
            .all(|d| d.code == rgoc_lex::DiagCode::InvalidUtf8)
    );
    assert_eq!(r.ch(), Some('y'));
}

// ── ⑥ 边界输入（T-L07，LEX-01/02）────────────────────────────────────
//
// 期望值**全部**取自 `fixtures/char_golden.txt`（oracle = go1.27.1 的 `source.nextch`），
// 不是推的。整段字节空间的密集对撞在 `test_char_golden.rs`；这里是**可读的少量代表**，
// 每条钉住一个具体机制 —— 对撞测试红了的时候，先看这里能立刻判断"错的是哪个机制"。
//
// ⚠️ 一句容易写错的话：**非法 / 被跳过的字节仍然占一个列号**。
// 官方 `nextch` 的 `redo:` 标签在 `col += chw` 的**上方**，所以每次"跳过并重读"
// 都会先推进一次列。`x\0\0y` 里 `y` 在 1:4 而不是 1:2，就是这条的直接后果。

#[test]
fn bom_仅允许文件首字符且计入字节列() {
    // BOM 被吃掉（不作为字符送出），但它**占了 3 个字节列** ⇒ x 在 1:4
    assert_eq!(walk("\u{feff}x".as_bytes()), vec![('x', 1, 4, 1)]);
    assert!(
        diags_of("\u{feff}x".as_bytes()).is_empty(),
        "文件首的 BOM 不报错"
    );

    // 文件只有 BOM：不产出任何字符，EOF 落在 1:4
    assert!(walk("\u{feff}".as_bytes()).is_empty());
    assert!(diags_of("\u{feff}".as_bytes()).is_empty());
}

#[test]
fn bom_在文件中报错但仍被跳过() {
    // 实测事件流：C 1:1 'x'、D 1:2 BOM-MIDFILE、C 1:5 'y'。
    // 也就是说文件中的 BOM **和文件首一样被跳过**（都不作为字符送出），
    // 区别**只在报不报错** —— 这点靠 oracle 才看得出来：
    // 光看官方那行 `goto redo` 会以为"报错就走另一条路"，实际两条路都走 redo。
    assert_eq!(
        walk("x\u{feff}y".as_bytes()),
        vec![('x', 1, 1, 1), ('y', 1, 5, 1)]
    );
    assert_eq!(
        diags_of("x\u{feff}y".as_bytes()),
        vec![(1, 2, DiagCode::BomMidFile)]
    );
}

#[test]
fn 双_bom_的第二个才报错() {
    // 判据是 `line > 0 || col > 0` —— 不是"第几个字符"。
    // 第二个 BOM 起 col 已经是 3 ⇒ 报错，且位置是 1:4（它自己的位置）
    assert_eq!(walk("\u{feff}\u{feff}x".as_bytes()), vec![('x', 1, 7, 1)]);
    assert_eq!(
        diags_of("\u{feff}\u{feff}x".as_bytes()),
        vec![(1, 4, DiagCode::BomMidFile)],
        "只有第二个 BOM 报错（第一个在 1:1，line=col=0）"
    );
}

#[test]
fn nul_报错但被跳过且仍占一列() {
    // 官方：`if s.ch == 0 { error; goto redo }` —— 报错 → 跳过 → 重新读
    assert_eq!(walk(b"x\0y"), vec![('x', 1, 1, 1), ('y', 1, 3, 1)]);
    assert_eq!(
        diags_of(b"x\0y"),
        vec![(1, 2, DiagCode::InvalidNul)],
        "诊断位置是被污染的那个字节"
    );

    // 连续两个 NUL：各自报一次，各自占一列
    assert_eq!(walk(b"x\0\0y"), vec![('x', 1, 1, 1), ('y', 1, 4, 1)]);
    assert_eq!(
        diags_of(b"x\0\0y"),
        vec![(1, 2, DiagCode::InvalidNul), (1, 3, DiagCode::InvalidNul)]
    );
}

#[test]
fn 非法_utf8_只消费一个字节() {
    // ★ 这条最容易写错：非法序列**不是**整段跳过，而是**只消费 1 字节**
    //   （Go 的 `utf8.DecodeRune` 对非法输入返回 `(RuneError, 1)`）。
    //   所以 `\xf0\x9f`（截断的 4 字节序列）会报**两条**诊断，不是一条。
    assert_eq!(walk(b"1\xff2"), vec![('1', 1, 1, 1), ('2', 1, 3, 1)]);
    assert_eq!(diags_of(b"1\xff2"), vec![(1, 2, DiagCode::InvalidUtf8)]);

    // 截断的 4 字节序列：两个字节各报一次
    assert!(walk(b"\xf0\x9f").is_empty());
    assert_eq!(
        diags_of(b"\xf0\x9f"),
        vec![(1, 1, DiagCode::InvalidUtf8), (1, 2, DiagCode::InvalidUtf8)]
    );

    // 代理区 `\xed\xa0\x80`：UTF-8 层面就是非法的 ⇒ 3 个字节各报一次
    assert_eq!(
        diags_of(b"\xed\xa0\x80"),
        vec![
            (1, 1, DiagCode::InvalidUtf8),
            (1, 2, DiagCode::InvalidUtf8),
            (1, 3, DiagCode::InvalidUtf8)
        ]
    );

    // ★ 但**合法编码的 U+FFFD** 不是错误 —— 这是最容易被"顺手统一"掉的一条：
    //   判据是 `ch == RuneError && chw == 1`，两个条件缺一不可。
    assert_eq!(walk("\u{fffd}".as_bytes()), vec![('\u{fffd}', 1, 1, 3)]);
    assert!(diags_of("\u{fffd}".as_bytes()).is_empty());
}

#[test]
fn 回车不算换行只有换行改行号() {
    // 官方 `nextch` 只在 `s.ch == '\n'` 时 `line++; col = 0`。
    // `\r` 是**下一位**（`next()` 的空白跳过）才处理的事情 —— 字符层不管它，
    // 所以 `\r` 在这里只是一个普通字符，占一列。
    assert_eq!(
        walk(b"x\ry"),
        vec![('x', 1, 1, 1), ('\r', 1, 2, 1), ('y', 1, 3, 1)]
    );
    assert!(diags_of(b"x\ry").is_empty());

    // CRLF：只有那个 `\n` 改行号，`\r` 仍占 1:2
    assert_eq!(
        walk(b"x\r\ny"),
        vec![
            ('x', 1, 1, 1),
            ('\r', 1, 2, 1),
            ('\n', 1, 3, 1),
            ('y', 2, 1, 1)
        ]
    );
}

#[test]
fn 私有区字符在字符层是合法的且宽度为三() {
    // ★ 计划把这条写成"报 LEX-BAD-IDENT-CHAR" —— 那是**标识符层**的事
    //   （官方 `scanner.go:411` 的 `atIdentChar`），不是字符层。
    //
    // 字符层对 U+E000 的态度：它是**合法 UTF-8**，宽度 3，**不报错**。
    // 这里刻意只断言字符层的那一半，把"标识符里报错"留给 T-L08 ——
    // 否则这条测试会因为"早了一层"而永远红，或者被迫在 source.rs 里塞词法逻辑。
    assert_eq!(
        walk("x\u{e000}y".as_bytes()),
        vec![('x', 1, 1, 1), ('\u{e000}', 1, 2, 3), ('y', 1, 5, 1)]
    );
    assert!(
        diags_of("x\u{e000}y".as_bytes()).is_empty(),
        "字符层不判断「这个字符能不能出现在标识符里」"
    );
}
