//! `T-CK-64`：版本门控的**落点**与 **soft 面**（M4/Task 19）。
//!
//! # 判据（实测钉死）
//!
//! | 站点 | 官方位置来源 | 语料实测 |
//! |---|---|---|
//! | `new(expr)` | `verifyVersionf(call.Fun, go1_26, "new(%s)", arg)`（`builtins.go:672`） | `go1_25.go` 4 条全在 **`:6`**（`new` 标识符） |
//! | `range` over int | `softErrorf(&x, InvalidRangeExpr, "cannot range over %s: %s", …)`（`range.go:72`） | `go1_22_21.go` 在 **`14:16`**（操作数 `10`，**不是** `range` 关键字） |
//!
//! 后者是官方 **soft 错误**（`types.Error.Soft`，`api.go:55`）：soft **不**改变
//! 判定与渲染（`C8` 只比位置+类别），但内部流必须留着它 —— 丢掉 soft 位就是
//! 丢掉官方公开字段，将来任何需要区分软硬的面都拿不回来。
//!
//! # 为什么用真实语料
//!
//! 位置不是「看起来合理」就行：`range` 关键字与操作数差 6 列，`new` 的 `(` 与
//! 标识符差 3 列。只有拿 oracle 的 `--check` 实测坐标（本次：`14:6` ×4、`14:16`）
//! 才能钉死。表驱动只能证明「我们和自己的假设一致」。

use std::path::{Path, PathBuf};

use rgoc_ast::decl::Decl;
use rgoc_ast::expr::Expr;
use rgoc_ast::stmt::{SimpleStmt, Stmt};
use rgoc_check::check::version_source;
use rgoc_check::diag::{Category, RawDiag, category_of};
use rgoc_check::version::{
    Feat, pos_of_new_call, pos_of_range_operand, verify_version, verify_version_soft,
};
use rgoc_lex::{DiagBag, FileId};
use rgoc_load::load_sources;
use rgoc_types::TypeTable;
use rgoc_types::version::{GoVersion, Versions};

fn corpus_root() -> PathBuf {
    let src = match std::env::var("RGOC_CORPUS_SRC_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../go_source_code/src")
            .to_path_buf(),
    };
    let dir = src.join("internal/types/testdata");
    assert!(
        dir.is_dir(),
        "语料目录不存在：{}\n这条向量必须真的读语料，**不允许**静默跳过。",
        dir.display()
    );
    dir
}

/// 装载一个语料文件（版本源走 Task 18 的同一条路）。
fn load(rel: &str) -> (rgoc_load::Package, FileId) {
    let p = corpus_root().join(rel);
    let src = std::fs::read(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()));
    let lang = version_source(None, &src).expect("harness 旗标");
    let mut tt = TypeTable::new();
    let (pkg, _bag) = load_sources(&mut tt, &[(rel, src.as_slice())], lang.as_deref());
    assert_eq!(pkg.files.len(), 1, "{rel} 应是单文件包");
    let fid = pkg.files[0];
    (pkg, fid)
}

// ── ① new(expr)：4 条门控的行列向量 ─────────────────────────────────

#[test]
fn t_ck_64_new_门控四条向量_位置与类别() {
    let (pkg, fid) = load("check/go1_25.go");
    // 生效版本 1.25（-lang 与 //go:build 都是 1.25）⇒ new(expr)（1.26）全拒。
    assert_eq!(pkg.versions.get(fid), Some(GoVersion::new(25)));

    let Decl::Func(f) = &pkg.asts[0].decls[0] else {
        panic!("go1_25.go 的首个声明应是 func f")
    };
    let body = f.body.as_ref().expect("func f 有体");

    // 期望来自 oracle 实测（`--check go1_25.go`）：14:6 / 15:6 / 16:6 / 17:6。
    let want = [(14, 6, "123"), (15, 6, "x"), (16, 6, "f"), (17, 6, "1 < 2")];
    assert_eq!(body.stmts.len(), want.len(), "四条 `_ = new(…)`");

    for (stmt, (line, col, subject)) in body.stmts.iter().zip(want) {
        let Stmt::Assign(a) = stmt else {
            panic!("应是赋值语句")
        };
        let Expr::Call(call) = &a.rhs[0] else {
            panic!("右值应是调用")
        };
        let at = pos_of_new_call(call);
        assert_eq!(
            (at.line, at.col),
            (line, col),
            "new(expr) 的位置是 `new` 标识符（官方 call.Fun）"
        );
        assert_eq!(at.file, fid);

        let mut bag = DiagBag::new();
        assert!(
            !verify_version(&pkg.versions, fid, at, Feat::NewExpr, subject, &mut bag),
            "1.25 必须拒 1.26 的 new(expr)"
        );
        let d = bag.into_sorted().remove(0);
        assert_eq!(
            d.message,
            format!("new({subject}) requires go1.26 or later"),
            "消息主体沿官方 `new(%s)` 形态"
        );
        assert_eq!(category_of(d.code), Category::Version);
    }
}

// ── ② range over int：soft + 操作数落点 ─────────────────────────────

#[test]
fn t_ck_64_range_门控_soft_落在操作数() {
    let (pkg, fid) = load("check/go1_22_21.go");
    // 文件版本 1.21 **取代** -lang 1.22（F11）⇒ range over int（1.22）被拒。
    assert_eq!(pkg.versions.get(fid), Some(GoVersion::new(21)));

    let Decl::Func(f) = &pkg.asts[0].decls[0] else {
        panic!("go1_22_21.go 的首个声明应是 func f")
    };
    let body = f.body.as_ref().expect("func f 有体");
    let Stmt::For(for_stmt) = &body.stmts[0] else {
        panic!("应是 for 语句")
    };
    let Some(SimpleStmt::Range(r)) = &for_stmt.init else {
        panic!("for 的 init 应是 range 子句")
    };

    let at = pos_of_range_operand(r);
    assert_eq!(
        (at.line, at.col),
        (14, 16),
        "官方 softErrorf(&x, …) 落在操作数 10，不是 range 关键字"
    );

    let raw = verify_version_soft(&pkg.versions, fid, at, Feat::RangeOverInt, "")
        .expect("1.21 必须拒 1.22 的 range over int");
    assert!(raw.soft, "range 门控是官方 softErrorf 面");
    assert_eq!(raw.diag.pos, at);
    assert!(raw.diag.message.ends_with("requires go1.22 or later"));
    assert_eq!(raw.render_row(), "14:16\tversion");

    // soft 与 hard 的**渲染行同形**（C8 只比位置+类别）：soft 位不进渲染。
    let mut bag = DiagBag::new();
    assert!(!verify_version(
        &pkg.versions,
        fid,
        at,
        Feat::RangeOverInt,
        "",
        &mut bag
    ));
    let hard = RawDiag::hard(bag.into_sorted().remove(0));
    assert_eq!(hard.render_row(), raw.render_row());
}

// ── ③ soft 只在**拒绝**时产生；允许时是 None ─────────────────────

#[test]
fn t_ck_64_soft_允许时无诊断() {
    let (_, fid) = load("check/go1_22_21.go");
    let at = rgoc_lex::Pos {
        base: None,
        file: fid,
        line: 3,
        col: 4,
    };
    let allowed = Versions::from_entries([(fid, Some(GoVersion::new(23)))]);
    assert!(
        verify_version_soft(&allowed, fid, at, Feat::RangeOverInt, "").is_none(),
        "1.23 已允许 range over int（1.22）"
    );

    let denied = Versions::from_entries([(fid, Some(GoVersion::new(21)))]);
    assert!(
        verify_version_soft(&denied, fid, at, Feat::RangeOverInt, "").is_some(),
        "1.21 必须给出 soft 诊断"
    );
}
