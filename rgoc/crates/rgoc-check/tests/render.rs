//! `T-CK-91` 渲染面：`C8` §2 的**字节格式**（M4/Task 20）。
//!
//! # 判据（`C8-check-diags.md` §2，冻结）
//!
//! * 头两行固定：`# source: rgoc` + `# file: <原样路径>`；**零诊断也输出**
//!   （规则 1——否则「两侧都空」会是假绿）。
//! * 数据行 `<行号>:<列>\t<类别>`；按（行号, 列, 类别）**稳定排序**；
//!   重复行**保留**（规则 4–6）。
//! * UTF-8、LF、无 BOM、文件末尾**恰一个**换行（§2 引言）。
//! * 次级消息**不进渲染**（规则 5 的 rgoc 侧等价物）。官方口径（`F2`④，
//!   `check_test.go:169`）：错误回调剔除**格式化字符串**含 `": \t"` 的消息；
//!   格式化串 = `<pos>: ` + `msg`，所以含 `": \t"` ⇔ `msg` 以 `\t` 开头
//!   （go/types 给带合法位置的子错误发 `Msg = "\t"+…`，`errors.go:186-188`）
//!   **或** `msg` 自身含 `": \t"`。两条都要挡，少滤 ⇒ 多行、误滤 ⇒ 丢行
//!   （`M4-plan.md` Review Focus #3）。
//! * 确定性：同输入重复运行逐字节相同（规则 7；`T-CK-92`）。
//!
//! # 这一层与上报层为什么分开
//!
//! 诊断流**内部保留**原始次级链（人读：澄清行跟着主错误才有意义），过滤只
//! 发生在渲染出口。对外可见的行集合与「回调剔除」等价 —— 差别只在流里还
//! 找不找得到那条链。

use rgoc_check::diag::RawDiag;
use rgoc_check::render::{is_rendered, is_secondary, render};
use rgoc_lex::{Diag, DiagCode, FileId, FileTable, Pos};

/// 合成位置用的文件。本文件的断言全部只看 `line/col`（文件维度由
/// `FileTable` 在 CLI 层渲染），所以 `FileId` 只需**有效**、不必可读。
fn fid() -> FileId {
    FileTable::new().intern("t.go")
}

/// 合成一条诊断（位置 + 代号 + 消息）。
fn raw(line: u32, col: u32, code: DiagCode, msg: &str) -> RawDiag {
    RawDiag::hard(Diag {
        pos: Pos {
            base: None,
            file: fid(),
            line,
            col,
        },
        code,
        message: msg.to_owned(),
    })
}

/// 数据行（剥掉 `#` 注释行）——want 与本侧输出两边都只比这个。
fn rows(text: &str) -> Vec<&str> {
    text.lines().filter(|l| !l.starts_with('#')).collect()
}

/// `C8` §8 的最小对撞目标：头两行 + 排序后的数据行 + 末尾恰一个换行。
///
/// 输入故意乱序（4:20 → 3:9 → 4:9）：渲染必须先排序 —— 如果实现只做了
/// 「原序输出」，这条会红。
#[test]
fn t_ck_91_渲染_头两行与数据行字节格式() {
    let diags = vec![
        raw(4, 20, DiagCode::TypUndeclaredName, "undefined: y"),
        raw(3, 9, DiagCode::TypUndeclaredName, "undefined: x"),
        raw(4, 9, DiagCode::TypUndeclaredName, "undefined: y"),
    ];
    let got = render(&diags, "testdata/check-diags-min.go");
    assert_eq!(
        got,
        "# source: rgoc\n# file: testdata/check-diags-min.go\n3:9\tundef\n4:9\tundef\n4:20\tundef\n",
        "必须是 C8 §2 的字节形态（头两行 + 排序数据行 + 末尾恰一个换行）"
    );
}

/// 零诊断：**只有头行**（规则 8）—— 头行不是「有输出」，是防假绿的钉子。
#[test]
fn t_ck_91_零诊断_只有头行() {
    let got = render(&[], "x.go");
    assert_eq!(got, "# source: rgoc\n# file: x.go\n");
}

/// 同一位置按**类别 token 字典序**排；重复键的输入**置换不变**。
///
/// 「置换不变」是排序语义的可观察部分：键覆盖了渲染行的全部字段，
/// 所以只要键序正确，任意输入次序都必须产出同一串字节。
#[test]
fn t_ck_91_排序按_行_列_类别_且置换不变() {
    let asc = vec![
        raw(9, 1, DiagCode::TypUndeclaredName, "undefined: z"),
        raw(
            9,
            1,
            DiagCode::TypUnsupportedAssignability,
            "assignability refused",
        ),
    ];
    let desc: Vec<RawDiag> = asc.iter().rev().cloned().collect();
    assert_eq!(
        render(&asc, "t.go"),
        render(&desc, "t.go"),
        "输入次序不影响输出"
    );
    assert_eq!(
        rows(&render(&asc, "t.go")),
        vec!["9:1\tassign", "9:1\tundef"],
        "同行同列 ⇒ 按类别 token 字典序（assign < undef）"
    );
}

/// 重复行**保留**（多重集语义，规则 6）—— 不去重。
#[test]
fn t_ck_91_重复行保留() {
    let twice = vec![
        raw(1, 2, DiagCode::TypDivByZero, "division by zero"),
        raw(1, 2, DiagCode::TypDivByZero, "division by zero"),
    ];
    assert_eq!(rows(&render(&twice, "t.go")), vec!["1:2\top-invalid"; 2]);
}

/// 次级消息：**流内保留、渲染剔除**（两条判据都钉）。
///
/// 三个向量分别对应 `": \t"` 等价判据的三个分支：`\t` 开头（官方子错误
/// 的原生形态）、消息自身含 `": \t"`（格式化后同样命中）、以及一个**必须
/// 保留**的普通消息（防恒真断言：如果谓词恒返回 true，这条会红）。
#[test]
fn t_ck_91_次级消息_流内保留_渲染剔除() {
    assert!(
        is_secondary("\tother declaration of x"),
        "`\\t` 开头 = 次级"
    );
    assert!(
        is_secondary("位置: \t澄清行"),
        "消息自身含 `\": \\t\"` 时，格式化串必然含它"
    );
    assert!(!is_secondary("undefined: x"), "普通消息不许被误滤");

    let diags = vec![
        raw(3, 9, DiagCode::TypUndeclaredName, "undefined: x"),
        raw(3, 5, DiagCode::TypDuplicateDecl, "\tother declaration of x"),
    ];
    assert!(is_rendered(&diags[0]) && !is_rendered(&diags[1]));
    assert_eq!(rows(&render(&diags, "t.go")), vec!["3:9\tundef"]);
    assert_eq!(diags.len(), 2, "流内两份都在 —— 过滤点是渲染，不是上报");
}

/// `# source` 行如实标注 `rgoc`（与 oracle 侧是**刻意保留**的那一处不同）。
#[test]
fn t_ck_91_来源标注为_rgoc() {
    let got = render(&[], "any.go");
    assert!(
        got.starts_with("# source: rgoc\n"),
        "来源必须如实标注（对撞脚本按此分辨两侧）：{got:?}"
    );
}
