//! `T-TY-71..75`：版本门控 —— M3/Task 21。
//!
//! 期望值来自 `go/types/check.go:312-348`（`F13`）与 `version.go`（`F3`）：
//! ① `-lang` 缺省/非法 ⇒ 不门控；② 文件级 `//go:build go1.N` ⇒
//! `max(fileVersion, go1.21)`；③ 两处 `TooNew`（包级取 `files[0]`、
//! 文件级取 `package` 名字）。`GO_CURRENT` 钉死为 `go1.27`（语料快照 go1.27.1）。

use rgoc_ast::{AstFile, Name, Pragma};
use rgoc_lex::diag::{Diag, DiagCode};
use rgoc_lex::{FileId, Pos};
use rgoc_types::{GO_CURRENT, GO1_21, GoVersion, pkg_versions};

fn pos(fid: u32, line: u32) -> Pos {
    pos_col(fid, line, 1)
}

fn pos_col(fid: u32, line: u32, col: u32) -> Pos {
    Pos {
        base: None,
        file: FileId::from_index(fid),
        line,
        col,
    }
}

/// 一个最小 `AstFile`：`package p`，可选 `//go:build` 版本。
fn file(fid: u32, package_line: u32, go_version: Option<&str>) -> AstFile {
    AstFile {
        pos: pos(fid, package_line),
        pragma: Pragma {
            directives: Vec::new(),
        },
        // ⚠️ 包名位置**必须**与文件起点（`package` 关键字）**不同** ——
        // 否则「文件级 TooNew 报在包名上」这条断言**区分不出**实现
        // （反向验证实测：把位置改成 `file.pos` 时它照样绿 ⇒ 恒真断言）。
        pkg_name: Name {
            value: "p".to_string(),
            pos: pos_col(fid, package_line, 9),
        },
        decls: Vec::new(),
        eof: pos(fid, package_line + 1),
        go_version: go_version.map(str::to_string),
    }
}

fn only(diags: &[Diag], code: DiagCode) -> bool {
    diags.len() == 1 && diags[0].code == code
}

#[test]
fn t_ty_71_lang_门控与不门控() {
    let f = file(0, 1, None);
    let files = [&f];
    let fid = f.pos.file;

    // `-lang go1.18` ⇒ 允许 go1.18 的特性（`TYP-17` 的类型参数）
    let (vs, bag) = pkg_versions(Some("go1.18"), &files);
    assert!(bag.into_sorted().is_empty());
    assert!(vs.allow(fid, GoVersion::new(18)));
    assert!(!vs.allow(fid, GoVersion::new(19)));

    // `-lang go1.17` ⇒ **拒绝** go1.18 的特性，并给出稳定拒绝码
    let (vs, mut bag) = pkg_versions(Some("go1.17"), &files);
    assert!(!vs.allow(fid, GoVersion::new(18)));
    let ok = vs.require(
        fid,
        GoVersion::new(18),
        pos(0, 9),
        "type parameters",
        &mut bag,
    );
    assert!(!ok);
    let diags = bag.into_sorted();
    assert!(
        only(&diags, DiagCode::TypFeatureRequiresGoVersion),
        "{diags:?}"
    );
    // 消息形状对齐官方 `"%s requires %s or later"`（文本不承诺逐字，D-M1-9）
    assert_eq!(diags[0].message, "type parameters requires go1.18 or later");
    assert_eq!(diags[0].pos, pos(0, 9), "位置由调用方给（特性所在处）");

    // `-lang` **缺省** ⇒ 不门控（官方 `!version.isValid()` 那支）
    let (vs, bag) = pkg_versions(None, &files);
    assert!(bag.into_sorted().is_empty());
    assert!(vs.allow(fid, GoVersion::new(99)));
    assert!(vs.get(fid).is_none());

    // `-lang` **非法** ⇒ 同样不门控（`1.20` 少了 `go` 前缀）
    let (vs, bag) = pkg_versions(Some("1.20"), &files);
    assert!(bag.into_sorted().is_empty());
    assert!(vs.allow(fid, GoVersion::new(99)));

    // 包级 `-lang` 带 dot-release / 预发布 ⇒ 剥成语言版本（官方 `Lang`）
    let (vs, _) = pkg_versions(Some("go1.20.1"), &files);
    assert_eq!(vs.get(fid), Some(GoVersion::new(20)));
    assert!(vs.allow(fid, GoVersion::new(20)));
    assert!(!vs.allow(fid, GoVersion::new(21)));

    // 文件不在表里 ⇒ 按「不门控」兜底（不做假诊断）
    let empty = pkg_versions(None, &[]).0;
    assert!(empty.allow(fid, GoVersion::new(99)));
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
}

#[test]
fn t_ty_72_文件构建标签抬高版本_且不低于_go1_21() {
    // `//go:build go1.22` + 缺省 `-lang` ⇒ 生效版本 = go1.22
    let f = file(0, 4, Some("go1.22"));
    let files = [&f];
    let fid = f.pos.file;
    let (vs, bag) = pkg_versions(None, &files);
    assert!(bag.into_sorted().is_empty());
    assert_eq!(vs.get(fid), Some(GoVersion::new(22)));
    assert!(vs.allow(fid, GoVersion::new(22)));
    assert!(!vs.allow(fid, GoVersion::new(23)));

    // `//go:build go1.18` ⇒ max(go1.18, go1.21) = **go1.21**（官方 `check.go:332-341`）
    let f = file(0, 4, Some("go1.18"));
    let (vs, _) = pkg_versions(None, &[&f]);
    assert_eq!(vs.get(f.pos.file), Some(GO1_21));

    // 恰好 go1.21 ⇒ max 是它自己（边界两侧都测）
    let f = file(0, 4, Some("go1.21"));
    let (vs, _) = pkg_versions(None, &[&f]);
    assert_eq!(vs.get(f.pos.file), Some(GoVersion::new(21)));

    // 文件级版本**覆盖** `-lang`（官方：`v = max(fileVersion, go1.21)`）
    let f = file(0, 4, Some("go1.22"));
    let (vs, _) = pkg_versions(Some("go1.18"), &[&f]);
    assert_eq!(vs.get(f.pos.file), Some(GoVersion::new(22)));

    // 带 dot-release 的文件版本 ⇒ 剥成语言版本（官方 `Lang`）
    let f = file(0, 4, Some("go1.22.1"));
    let (vs, bag) = pkg_versions(Some("go1.18"), &[&f]);
    assert!(bag.into_sorted().is_empty());
    assert_eq!(vs.get(f.pos.file), Some(GoVersion::new(22)));
    // 已知窄化：后缀**不校验**（`go1.22.1.0.x` 我们接受、官方判非法）——
    // 输入形状由本仓产出（`-lang` 形参 / `M2` 的 `//go:build` 解析），
    // 故在现有数据上不可能产生差异；登记在 manifest 的 `open_gaps`。
    let f = file(0, 4, Some("go1.22.1.0.x"));
    let (vs, _) = pkg_versions(Some("go1.18"), &[&f]);
    assert_eq!(vs.get(f.pos.file), Some(GoVersion::new(22)));
    let f = file(0, 4, Some("2.0"));
    let (vs, _) = pkg_versions(Some("go1.18"), &[&f]);
    assert_eq!(
        vs.get(f.pos.file),
        Some(GoVersion::new(18)),
        "非法 ⇒ 退回 -lang"
    );
    let f = file(0, 4, Some("garbage"));
    let (vs, _) = pkg_versions(None, &[&f]);
    assert_eq!(vs.get(f.pos.file), None, "非法且无 -lang ⇒ 不门控");
}

#[test]
fn t_ty_73_文件版本比工具链新_位置取包名() {
    // `//go:build go1.99` > GO_CURRENT（go1.27）⇒ TooNew，位置 = `package` **名字**
    let f = file(0, 7, Some("go1.99"));
    let files = [&f];
    let (vs, bag) = pkg_versions(None, &files);
    let diags = bag.into_sorted();
    assert!(only(&diags, DiagCode::TypFileRequiresNewerGo), "{diags:?}");
    assert_eq!(diags[0].pos, f.pkg_name.pos);
    assert_eq!(
        diags[0].message,
        "file requires newer Go version go1.99 (application built with go1.27)"
    );
    // 生效版本仍按 `max(v, go1.21)` 记（官方**照样**登记，只是同时报错）
    assert_eq!(vs.get(f.pos.file), Some(GoVersion::new(99)));

    // 恰好等于 GO_CURRENT ⇒ **不**报（官方是 `cmp(go_current) > 0`）
    let f = file(0, 1, Some("go1.27"));
    let (_, bag) = pkg_versions(None, &[&f]);
    assert!(bag.into_sorted().is_empty(), "恰等于工具链版本不算太新");

    // 比工具链新 **一个** 次版本就报
    let f = file(0, 1, Some("go1.28"));
    let (_, bag) = pkg_versions(None, &[&f]);
    assert!(only(&bag.into_sorted(), DiagCode::TypFileRequiresNewerGo));
    assert_eq!(GO_CURRENT, GoVersion::new(27), "钉死：语料快照 go1.27.1");
}

#[test]
fn t_ty_74_包级版本比工具链新() {
    let a = file(0, 1, None);
    let b = file(1, 3, None);
    let files = [&a, &b];
    let (_, bag) = pkg_versions(Some("go1.99"), &files);
    let diags = bag.into_sorted();
    assert!(
        only(&diags, DiagCode::TypPackageRequiresNewerGo),
        "{diags:?}"
    );
    // 位置 = `files[0]`（官方 `check.errorf(files[0], TooNew, …)`，
    // `ast.File.Pos()` 是 `package` **关键字**处）
    assert_eq!(diags[0].pos, a.pos);
    assert_ne!(diags[0].pos, b.pos);
    assert_eq!(
        diags[0].message,
        "package requires newer Go version go1.99 (application built with go1.27)"
    );

    // 恰等于工具链版本 ⇒ 不报
    let (_, bag) = pkg_versions(Some("go1.27"), &files);
    assert!(bag.into_sorted().is_empty());
    // `files` 为空 ⇒ 没有位置可指 ⇒ 也不报（官方 `len(files) > 0` 的守卫）
    let (vs, bag) = pkg_versions(Some("go1.99"), &[]);
    assert!(bag.into_sorted().is_empty());
    assert!(vs.is_empty());
}

#[test]
fn t_ty_75_按文件分别判定() {
    // 同包两文件、不同 `//go:build` 版本 ⇒ **各自**判定（官方逐文件 `versions[file]`）
    let a = file(0, 1, Some("go1.18")); // ⇒ max(go1.18, go1.21) = go1.21
    let b = file(1, 1, Some("go1.22")); // ⇒ go1.22
    let c = file(2, 1, None); // ⇒ 用 -lang
    let files = [&a, &b, &c];
    let (vs, bag) = pkg_versions(Some("go1.19"), &files);
    assert!(bag.into_sorted().is_empty());

    assert_eq!(vs.get(a.pos.file), Some(GoVersion::new(21)));
    assert_eq!(vs.get(b.pos.file), Some(GoVersion::new(22)));
    assert_eq!(vs.get(c.pos.file), Some(GoVersion::new(19)));

    // a.go 拒绝 go1.22 的特性，b.go 允许 —— 同一次编译、不同答案
    assert!(!vs.allow(a.pos.file, GoVersion::new(22)));
    assert!(vs.allow(b.pos.file, GoVersion::new(22)));
    assert!(!vs.allow(c.pos.file, GoVersion::new(22)));

    // 只有 b.go 太新 ⇒ **一条**诊断，且指向 b.go 的包名
    let b_new = file(1, 1, Some("go1.99"));
    let (_, bag) = pkg_versions(Some("go1.19"), &[&a, &b_new, &c]);
    let diags = bag.into_sorted();
    assert!(only(&diags, DiagCode::TypFileRequiresNewerGo), "{diags:?}");
    assert_eq!(diags[0].pos, b_new.pkg_name.pos);
    assert_eq!(diags[0].pos.file, b_new.pos.file, "指向的是 b.go 那个文件");
}
