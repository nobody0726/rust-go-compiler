//! 显式文件列表：**不评估构建约束**（`T-TY-65`，`D-M3-14`）—— M3/Task 28。
//!
//! 判据来源：`M3-plan.md` Task 28 + `M3-tests.md` §3.6 的 `T-TY-65`
//! （「带 `//go:build` 排除标记的文件**照样**被装载」）。
//!
//! # 为什么这是**反向**断言
//!
//! `D-M3-14` 把「评估构建约束」划给 M4+：M3 的装载入口收的是**显式文件列表**，
//! 谁列进来就装谁 —— 与 `go/build` 的 `ImportDir`（按 `//go:build` 与文件名筛）
//! 是**两件事**。所以这个用例要证的是「筛**没有**发生」：
//!
//! | 夹具 | 若我们（错误地）评估构建约束 | 断言 |
//! |---|---|---|
//! | `//go:build ignore` | 文件被丢掉 | 文件在包里、**零**诊断 |
//! | `//go:build windows`（宿主是 linux） | 文件被丢掉 | 同上 |
//! | `//go:build go1.21` | 这是**版本**，不是排除标记 | 文件在包里**且**版本进了 `Versions` |
//! | `c_test.go` | 测试文件被丢掉 | 文件在包里（M3 不做 `_test.go` 语义） |
//!
//! ⚠️ 与 Task 27 的 `ImportResolver` 对照：那一侧文件是**我们自己挑**的，所以要用
//! `go/build` 的**文件名**规则（`_test.go` / `_` 开头不算）；这一侧文件是**调用方
//! 给的**，一个字都不筛。两条规则不冲突，但**别互抄**。
//!
//! ⚠️ 第二条纪律：`-lang` 改的是**版本**，**不是**文件集合 —— 两条用例分别钉
//! 「筛没发生」与「`-lang` 只动版本」。

use std::path::{Path, PathBuf};

use rgoc_load::load_from_explicit_files;
use rgoc_types::{GoVersion, ObjKind, TypeTable};

/// 夹具目录（**绝对路径**）。
fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/build-constraints")
}

/// 四个文件，**顺序固定**（`Package::files` 就是输入序 ⇒ 用例能按下标对版本）。
const NAMES: [&str; 4] = ["a.go", "b.go", "c_test.go", "d.go"];

fn files() -> Vec<PathBuf> {
    NAMES.iter().map(|n| dir().join(n)).collect()
}

#[test]
fn t_ty_65_显式列表里的文件一个都不筛() {
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_from_explicit_files(&mut tt, &files(), None);
    let ds = bag.into_sorted();
    assert!(ds.is_empty(), "显式文件列表装载必须**零**诊断：{ds:#?}");
    assert_eq!(pkg.name, "p");
    assert_eq!(
        pkg.file_count(),
        NAMES.len(),
        "四个文件都进包（构建约束不筛、`_test.go` 也不筛）"
    );
    assert!(
        pkg.skipped.is_empty(),
        "四个文件都读到了：{:?}",
        pkg.skipped
    );

    // 进来的**确实是这四个文件**：每个文件的声明都在包作用域里
    for n in ["A", "B", "C", "D"] {
        let obj = pkg
            .lookup(n)
            .unwrap_or_else(|| panic!("{n} 应当进包作用域（文件被筛掉了？）"));
        assert_eq!(obj.kind, ObjKind::Var);
    }

    // 「读版本 ≠ 筛文件」：`//go:build go1.21` 的文件照样进包，**同时**版本进了 `Versions`；
    // 其余三个没有版本行 ⇒ 不门控（`-lang` 缺省）
    for (id, name) in pkg.files.iter().zip(NAMES) {
        let ver = pkg.versions.get(*id);
        if name == "d.go" {
            assert_eq!(
                ver,
                Some(GoVersion::new(21)),
                "`//go:build go1.21` 的**版本**要读进来（读版本 ≠ 筛文件）"
            );
        } else {
            assert_eq!(ver, None, "{name} 没有版本行 ⇒ 该文件不门控");
        }
    }
}

/// `-lang` 只改**生效版本**，**不**改文件集合（`D-M3-15` 的版本按文件 × 构建约束不筛）。
#[test]
fn t_ty_65_lang_只动版本不动文件集合() {
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_from_explicit_files(&mut tt, &files(), Some("go1.24"));
    let ds = bag.into_sorted();
    assert!(ds.is_empty(), "{ds:#?}");
    assert_eq!(pkg.file_count(), NAMES.len(), "`-lang` 不筛文件");
    // `-lang go1.24` ⇒ 无版本行的三个文件门控到 go1.24；`d.go` 自己的 `go1.21`
    // **覆盖**包级值（官方 `check.go:332-341`：文件级版本 = `max(文件值, go1.21)`）
    for (id, name) in pkg.files.iter().zip(NAMES) {
        let want = if name == "d.go" { 21 } else { 24 };
        assert_eq!(
            pkg.versions.get(*id),
            Some(GoVersion::new(want)),
            "{name} 的生效版本"
        );
    }
}
