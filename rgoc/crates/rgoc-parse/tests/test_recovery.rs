//! `T-A-61`..`T-A-65` —— 错误恢复（`M2-design.md` §5.3，对齐 `F2`–`F5`）。
//!
//! 本文件只放**从 `parse_file` 外面看得见**的行为。
//! `advance` 的边界语义（空 followlist = 消费一个、sync 点上不消费）在
//! `src/recovery.rs` 的 `#[cfg(test)]` 里 —— 它们在 `T-AP06` 还没有调用方，
//! 从这里驱动不到。
//!
//! `M2-tests.md` §3.7 要求每条都断言两件事：①诊断（位置 + `DiagCode` + 消息）
//! ②其后的解析仍然正确。只断言「报错了」是假通过。

use rgoc_lex::diag::DiagCode;
use rgoc_parse::{ErrMode, STOPSET, parse_file, parse_file_with};

/// `T-A-61`（`F3`）：`stopset` 与官方 `parser.go:326-339` **逐条一致**。
///
/// # 为什么期望值在这里另写一份
///
/// 从 `STOPSET` 自己生成期望值就是「用实现验证实现」，永远绿。这里把 14 个拼写
/// 独立列出来，再经 `Keyword::as_str()` 归一 —— 实现里换错一个（比如 `Defer`
/// 写成 `Default`）就会红。
///
/// 「14」这个**数**不用断言：`STOPSET` 的类型是 `[Keyword; 14]`，
/// 13 个在**类型上**就不成立（这正是 `M2-design.md` §14 勘误想要的防护）。
#[test]
fn t_a_61_stopset_恰好十四个且与官方逐条一致() {
    let mut got: Vec<&str> = STOPSET.iter().map(|k| k.as_str()).collect();
    got.sort_unstable();

    let mut want = [
        "break",
        "const",
        "continue",
        "defer",
        "fallthrough",
        "for",
        "go",
        "goto",
        "if",
        "return",
        "select",
        "switch",
        "type",
        "var",
    ];
    want.sort_unstable();

    assert_eq!(got, want, "stopset 与官方 parser.go:326-339 不一致");
}

/// `T-A-61`（`F2`）：`advance` 是「跳到同步点」而不是「逐个报错」。
///
/// 十个 `!` 只该产出**一条**诊断。如果 `advance` 没前进（或每个词元报一次），
/// 这条会红 —— 而「没前进」的另一种症状是**死循环**，测试会挂住而不是误绿。
#[test]
fn t_a_61_advance_一路吃到_eof() {
    let r = parse_file(b"package p; !!!!!!!!!!\n", "t.go");
    assert_eq!(r.errors, 1, "十个 `!` 仍只报一次：{:?}", r.diags);
    assert!(r.file.is_some(), "恢复后仍应产出文件");
}

/// `T-A-62`（`F4`/`F19`）：`errh=nil` 档 —— 遇**第一个**错即止。
#[test]
fn t_a_62_首错即停档只留第一条错误() {
    let r = parse_file_with(b"package p; !!! var\n", "t.go", ErrMode::StopOnFirst);

    assert!(r.file.is_none(), "首错即停 ⇒ 不产出文件");
    assert_eq!(r.errors, 1, "恰好报一次");
    let d = r.diags.into_sorted();
    assert_eq!(d.len(), 1, "诊断流里也只有一条：{d:?}");

    let first = r.first.expect("应有 first error（F19）");
    assert_eq!(first.code, DiagCode::SynUnexpectedToken);
    assert!(
        first.message.starts_with("syntax error: "),
        "消息前缀应为 `syntax error: `，got {:?}",
        first.message
    );
    assert_eq!(d[0], first, "`first` 与诊断流首条必须是同一条");
}

/// `T-A-62` **对照**：同一条输入在 `Continue` 档**不**停。
///
/// 没有这条对照，上面那条在「实现根本不分档」时仍然是绿的。
#[test]
fn t_a_62_继续档不停() {
    let r = parse_file(b"package p; !!! var\n", "t.go");
    assert!(r.file.is_some(), "继续档应产出文件");
    assert_eq!(r.errors, 1);
}

/// `T-A-63`（`F4`）：`errh≠nil` 档 —— 报错后**继续**，已经解析出来的部分不受影响。
#[test]
fn t_a_63_继续档报错后仍产出_ast() {
    let r = parse_file(b"package p; !!!\n", "t.go");
    let f = r.file.expect("继续档应仍产出文件");
    assert_eq!(
        f.pkg_name.value, "p",
        "错误在声明区，package 子句不该受影响"
    );
    assert!(f.decls.is_empty(), "骨架阶段顶层声明还不产出（T-AP08/09）");

    let d = r.diags.into_sorted();
    assert_eq!(d.len(), 1, "只应有一条：{d:?}");
    assert_eq!(d[0].code, DiagCode::SynUnexpectedToken);
}

/// `T-A-64`（`F5`）：抑制的判据是「**在 EOF** 且**已经有错**」的**合取**。
///
/// 本用例落在合取的**另一半**上：空文件里第一个（也是唯一一个）错误**就发生在 EOF**，
/// 但此时 `first` 还是 `None` ⇒ **必须报出来**。
///
/// 判据的用意：偷懒的实现写成 `if at(EOF) { return; }`（漏掉 `&& first.is_some()`），
/// 空文件会**一条诊断都没有**，这条立刻红。
#[test]
fn t_a_64_eof_抑制只对已有错生效() {
    let r = parse_file(b"", "t.go");
    assert_eq!(
        r.errors, 1,
        "空文件的唯一错误就在 EOF，但 first 尚空 ⇒ 必须报"
    );
    let d = r.diags.into_sorted();
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].code, DiagCode::SynUnexpectedToken);
}

/// `T-A-65`（§5.3）：**恢复点 ①**（顶层声明之间）—— 语句出现在函数体外，报错后继续。
#[test]
fn t_a_65_恢复点一_顶层声明之间() {
    // 顶层的 `if` 只能是语句 ⇒ 官方的 "non-declaration statement outside function body"
    let r = parse_file(b"package p\nif x {}\n", "t.go");
    assert_eq!(r.errors, 1, "只报一次：{:?}", r.diags);

    let first = r.first.expect("应有 first error");
    assert!(
        first
            .message
            .contains("non-declaration statement outside function body"),
        "消息应为官方的 `non-declaration statement outside function body`，got {:?}",
        first.message
    );
    assert!(
        r.file.is_some(),
        "恢复后仍应产出文件（这正是「恢复点」要证明的事）"
    );
}
