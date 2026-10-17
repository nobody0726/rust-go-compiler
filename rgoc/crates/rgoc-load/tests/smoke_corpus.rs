//! 冒烟：语料里**真正零 import** 的单包（`T-TY-96`）—— M3/Task 30。
//!
//! 判据来源：`M3-plan.md` Task 30 + `M3-tests.md` §3.6 的 `T-TY-96`
//! （「能装载、不崩、诊断数可解释」）+ 门禁 `TY-5`（包模型）。
//!
//! # 为什么语料要**挑**零 import 的包
//!
//! `load_from_explicit_files` 收的是**显式文件列表**（`D-M3-14`）：
//! 它不筛构建约束、不解析 import。要冒烟的是「装载 + 作用域 + 声明求值」这条链，
//! 不是 import 解析（那条由 `tests/imports.rs` 的夹具覆盖，Task 27）。
//! 所以夹具选**语料里非测试文件全都不含 `import`** 的包 —— 由
//! `.superpowers/sdd/M3-plan/probe/find-zero-import-pkgs.py` 在语料里扫出来
//! （全语料共 42 个这样的包，这里挑 8 个：泛型 / 方法 / 联合约束 / 大表 /
//! 多文件 / 空字段名 / 语句密集，各占一样）。
//!
//! # 判据为什么敢**精确**
//!
//! 语料被 `corpus-manifest.sha256` 锁死（`AGENTS.md` §3.1）⇒ 文件内容、
//! 声明数量都不变 ⇒ 这里的「名字数 / dump 根数」可以写**精确值**。
//! 任何一处悄悄少收声明、少建类型，都会在这里变红。
//!
//! # 这个用例真的抓到过一个 bug
//!
//! 首跑时 `unicode/utf8` **不是**零诊断：`utf8.go:459` 的
//! `const ptrSize = 4 << (^uintptr(0) >> 63)` 被判成 `TYP-INVALID-SHIFT-COUNT`。
//! 根因是 `^` 的类型依赖：无符号**有类型**操作数要按宽度取补（`^uintptr(0)` =
//! 2^64-1 ⇒ `>> 63` = 1 ⇒ `ptrSize = 8`），而 `rgoc-const` 的折叠器一律用
//! 无限精度补码（`-1`）⇒ `>> 63` 还是 `-1` ⇒ 报「移位计数非法」。
//! 修在 `fold_unary_typed`（`rgoc-const`），回归钉在
//! `rgoc-const/tests/eval_const.rs` 的 `无符号有类型操作数的补按宽度取补`。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rgoc_load::load_from_explicit_files;
use rgoc_types::TypeTable;

/// 一个**零 import** 的语料包。
struct Pkg {
    /// 相对 `go_source_code/src` 的目录。
    dir: &'static str,
    /// **显式文件列表**（`_test.go` 不在内：`D-M3-14` 说测试文件语义归 M4+，
    /// 而且 `package x_test` 会撞包名一致性）。
    files: &'static [&'static str],
    /// `package` 子句 —— 装载结果的包名必须等于它（否则说明装错了文件）。
    name: &'static str,
    /// **包作用域本层**的名字数（含非导出名；方法不在内）。
    names: usize,
    /// `C7` dump 根数（顶层声明的类型 + 值声明，声明序）。
    roots: usize,
}

const PKGS: &[Pkg] = &[
    // 泛型 + 联合约束（`Ordered` 是 `~int | …`）+ 非导出函数 `isNaN`
    Pkg {
        dir: "cmp",
        files: &["cmp.go"],
        name: "cmp",
        names: 5,
        roots: 5,
    },
    // 泛型类型 + 方法集（方法**不**进包作用域）
    Pkg {
        dir: "container/list",
        files: &["list.go"],
        name: "list",
        names: 3,
        roots: 25,
    },
    // 指针型数据结构 + 方法
    Pkg {
        dir: "container/ring",
        files: &["ring.go"],
        name: "ring",
        names: 2,
        roots: 10,
    },
    // **多文件**（两个文件互相引用）
    Pkg {
        dir: "internal/coverage",
        files: &["defs.go", "pkid.go"],
        name: "coverage",
        names: 41,
        roots: 19,
    },
    // **多文件**（大 `switch` / `map` / 常量块）
    Pkg {
        dir: "internal/platform",
        files: &["supported.go", "zosarch.go"],
        name: "platform",
        names: 17,
        roots: 16,
    },
    // **空字段名** `_`（`type HostLayout struct { _ hostLayout }`）+ 前向引用
    Pkg {
        dir: "structs",
        files: &["doc.go", "hostlayout.go"],
        name: "structs",
        names: 2,
        roots: 2,
    },
    // 语料里最大的零 import 单包：25 万字节的**表格**（`var` 大复合字面量）
    Pkg {
        dir: "unicode",
        files: &[
            "casetables.go",
            "digit.go",
            "graphic.go",
            "letter.go",
            "tables.go",
        ],
        name: "unicode",
        names: 593,
        roots: 39,
    },
    // 语句密集（`for range` / 位运算 / 泛型函数）+ **那个 bug 的现场**
    Pkg {
        dir: "unicode/utf8",
        files: &["utf8.go"],
        name: "utf8",
        names: 58,
        roots: 21,
    },
];

/// 语料源码根目录。默认按 crate 位置推算（`<repo>/go_source_code/src`），
/// 可用 `RGOC_CORPUS_SRC_DIR` 覆盖。
///
/// ⚠️ **找不到就 panic，不静默跳过** —— 本文件的价值就是真的把语料装进装载器；
/// 语料缺失时静默 skip 等于把这条冒烟悄悄关掉（与 `test_corpus.rs` 同一纪律）。
fn corpus_src_root() -> PathBuf {
    if let Ok(d) = std::env::var("RGOC_CORPUS_SRC_DIR") {
        return PathBuf::from(d);
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../go_source_code/src")
        .to_path_buf();
    assert!(
        dir.is_dir(),
        "语料目录不存在：{}\n\
         这条冒烟必须真的读语料，**不允许**静默跳过。\
         可用 RGOC_CORPUS_SRC_DIR 指向 go_source_code/src。",
        dir.display()
    );
    dir
}

/// **不超时**的退出保障：单个包 30 s。实测单个包 ≤ 30 ms（最大的 `unicode`
/// 是 28 ms）⇒ 这个界不是性能判据，而是「哪里挂住了」的刹车。
const PER_PKG_BUDGET: Duration = Duration::from_secs(30);

#[test]
fn t_ty_96_零import语料包_装载零诊断() {
    let root = corpus_src_root();
    for p in PKGS {
        let dir = root.join(p.dir);
        let files: Vec<PathBuf> = p.files.iter().map(|f| dir.join(f)).collect();
        for f in &files {
            assert!(f.is_file(), "语料文件不存在：{}", f.display());
        }

        let mut tt = TypeTable::new();
        let started = Instant::now();
        let (pkg, bag) = load_from_explicit_files(&mut tt, &files, None);
        let elapsed = started.elapsed();
        let diags = bag.into_sorted();

        // ① **不超时**（刹车，不是性能判据）
        assert!(
            elapsed < PER_PKG_BUDGET,
            "{} 装载耗时 {elapsed:?} ≥ {PER_PKG_BUDGET:?} —— 疑似挂住",
            p.dir
        );
        // ② **零诊断**。合法语料上出现任何诊断都是**误报**（能力边界不能
        //    伪装成用户的错）⇒ 这里不许白名单：发现一条就说明我们错了
        assert!(
            diags.is_empty(),
            "{} 装载出 {} 条诊断（合法语料应当零诊断）：\n{:#?}",
            p.dir,
            diags.len(),
            diags
                .iter()
                .map(|d| format!(
                    "  {:?} {}:{}:{}  {}",
                    d.code,
                    pkg.file_table.name(d.pos.file),
                    d.pos.line(),
                    d.pos.col(),
                    d.message
                ))
                .collect::<Vec<_>>()
        );
        // ③ 装进来的是**这个包**：文件数与包名都对得上
        assert_eq!(pkg.files.len(), p.files.len(), "{} 装载的文件数", p.dir);
        assert_eq!(pkg.name, p.name, "{} 的 `package` 子句", p.dir);
        // ④ **真的干了活**：声明与 dump 根都非空，且数量与冻结口径一致
        //    （少收一个名字 / 少建一个根都会红 —— 静默跳过是这里最该防的）
        assert_eq!(
            pkg.scopes.get(pkg.scope).len(),
            p.names,
            "{} 包作用域本层名字数",
            p.dir
        );
        assert_eq!(pkg.type_roots.len(), p.roots, "{} dump 根数", p.dir);
    }
}
