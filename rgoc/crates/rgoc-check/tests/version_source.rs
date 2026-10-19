//! `T-CK-62/63`：`-lang` 与 `//go:build` **同源进入**（M4/Task 18）。
//!
//! # 判据
//!
//! * **harness 旗标**（`F2`①）：只读**第一个文件的首行**，且必须是
//!   `//` 后（可含空白）紧跟 `-` 的行注释；认 `-lang` / `-goexperiment` /
//!   `-fakeImportC` 三个旗标（`check_test.go:108-126`）。后两个在本仓
//!   **响亮失败**（`U-M4-04` / `U-M4-05`）。
//! * **文件版本**（`F11`）：`//go:build go1.N` ⇒ 生效版本 =
//!   `max(go1.N, go1.21)` 并**取代**包版本；没有文件版本才用 `-lang`。
//! * **缺省**（`F12`）：`-lang` 缺省/非法且无文件版本 ⇒ **不门控**。
//!
//! # 四个语料向量（`M4-plan` Task 18 Step 1 点名）
//!
//! | 文件 | `-lang` | `//go:build` | 生效 | 判定 |
//! |---|---|---|---|---|
//! | `check/go1_19_20.go` | go1.19 | go1.20 | **1.21** | 切片→数组（1.20）**接受** |
//! | `check/go1_22_21.go` | go1.22 | go1.21 | **1.21** | range over int（1.22）**拒绝** |
//! | `check/go1_xx_19.go` | — | go1.19 | **1.21** | 切片→数组（1.20）**接受** |
//! | `check/decls0.go` | go1.17 | — | **1.17** | 类型参数（1.18）**拒绝** |
//!
//! 本任务只验**版本源与判定**；具体门控点的位置/类别收口在 Task 19/26/37。

use std::path::{Path, PathBuf};

use rgoc_check::check::{parse_harness_flags, version_source, versions};
use rgoc_check::version::{Feat, allow};
use rgoc_load::load_sources;
use rgoc_types::TypeTable;
use rgoc_types::version::GoVersion;

// ── 夹具 ────────────────────────────────────────────────────────────

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
        "语料目录不存在：{}\n\
         这条向量必须真的读语料，**不允许**静默跳过。\
         可用 RGOC_CORPUS_SRC_DIR 指向 go_source_code/src。",
        dir.display()
    );
    dir
}

fn read_corpus(rel: &str) -> Vec<u8> {
    let p = corpus_root().join(rel);
    std::fs::read(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()))
}

struct Loaded {
    pkg: rgoc_load::Package,
    lang: Option<String>,
}

/// 走**检查面的**同一条路：首行旗标 → 版本源 → 装载（`//go:build` 由装载器读）。
fn load(rel: &str, cli_lang: Option<&str>) -> Loaded {
    let src = read_corpus(rel);
    let lang = version_source(cli_lang, &src).expect("harness 旗标应可解析");
    let mut tt = TypeTable::new();
    let (pkg, _bag) = load_sources(&mut tt, &[(rel, src.as_slice())], lang.as_deref());
    Loaded { pkg, lang }
}

fn only_file(x: &Loaded) -> rgoc_lex::FileId {
    assert_eq!(x.pkg.files.len(), 1, "向量应是单文件包");
    x.pkg.files[0]
}

// ── ① harness 旗标解析（`F2`①）─────────────────────────────────────

#[test]
fn t_ck_62_首行旗标_等号与空格两种形态() {
    let f = parse_harness_flags(b"// -lang=go1.19\npackage p\n").expect("合法首行");
    assert_eq!(f.lang.as_deref(), Some("go1.19"));
    assert_eq!(f.goexperiment, None);
    assert!(!f.fake_import_c);

    // Go `flag` 包两种赋值形态都认；注释到 `-` 之间允许空白。
    let f = parse_harness_flags(b"//   -lang go1.22 -goexperiment=regabi\npackage p\n")
        .expect("合法首行");
    assert_eq!(f.lang.as_deref(), Some("go1.22"));
    assert_eq!(f.goexperiment.as_deref(), Some("regabi"));

    let f = parse_harness_flags(b"// -fakeImportC\npackage p\n").expect("布尔旗标");
    assert!(f.fake_import_c);
    let f = parse_harness_flags(b"// -fakeImportC=false\npackage p\n").expect("布尔赋值");
    assert!(!f.fake_import_c);
}

#[test]
fn t_ck_62_首行旗标_只认第一行且注释必须紧跟旗标() {
    // ① 旗标在第二行 ⇒ 忽略（官方只读 `srcs[0]` 首行）。
    for src in [
        b"package p\n// -lang=go1.9\n".as_slice(),
        // ② 首行不是行注释。
        b"/* -lang=go1.9 */\npackage p\n".as_slice(),
        // ③ 注释不以 `-` 开头。
        b"// copyright\npackage p\n".as_slice(),
        b"//\npackage p\n".as_slice(),
    ] {
        let f = parse_harness_flags(src).expect("非旗标首行不算错");
        assert_eq!(f.lang, None, "{:?}", String::from_utf8_lossy(src));
    }
}

#[test]
fn t_ck_62_首行旗标_非法形态响亮失败() {
    // 无换行（官方 `end < 0`）、超长（`end > 256`）、未知旗标、缺值。
    for src in [
        b"// -lang=go1.19".as_slice(),
        b"// -unknown=1\npackage p\n".as_slice(),
        b"// -lang\npackage p\n".as_slice(),
        b"// -fakeImportC=maybe\npackage p\n".as_slice(),
    ] {
        assert!(
            parse_harness_flags(src).is_err(),
            "应拒绝：{:?}",
            String::from_utf8_lossy(src)
        );
    }
    let mut long = b"// -lang=go1.19".to_vec();
    long.extend(std::iter::repeat_n(b' ', 300));
    long.push(b'\n');
    assert!(parse_harness_flags(&long).is_err(), "超过 256 字节必须拒");
}

// ── ② 版本源：CLI 覆盖 + 两个未支持旗标响亮失败 ────────────────────

#[test]
fn t_ck_62_显式_lang_覆盖首行旗标() {
    let src = b"// -lang=go1.17\npackage p\n";
    assert_eq!(
        version_source(Some("go1.19"), src)
            .expect("合法")
            .as_deref(),
        Some("go1.19"),
        "显式 `--lang` 是覆盖路径（`D-M3-15`）"
    );
    assert_eq!(
        version_source(None, src).expect("合法").as_deref(),
        Some("go1.17"),
        "没有显式值时用 harness 首行"
    );
    assert_eq!(version_source(None, b"package p\n").expect("合法"), None);
}

#[test]
fn t_ck_62_goexperiment_与_fake_import_c_响亮失败() {
    let e = version_source(None, b"// -goexperiment=foo\npackage p\n")
        .expect_err("-goexperiment 未支持，必须响亮失败");
    assert!(e.contains("U-M4-04"), "错误要指向登记项：{e}");
    let e = version_source(None, b"// -fakeImportC\npackage p\n")
        .expect_err("-fakeImportC 未支持，必须响亮失败");
    assert!(e.contains("U-M4-05"), "错误要指向登记项：{e}");
}

// ── ③ 四个语料向量：生效版本 ───────────────────────────────────────

#[test]
fn t_ck_62_语料四向量_生效版本() {
    for (rel, want) in [
        ("check/go1_19_20.go", 21),
        ("check/go1_22_21.go", 21),
        ("check/go1_xx_19.go", 21),
        ("check/decls0.go", 17),
    ] {
        let x = load(rel, None);
        let fid = only_file(&x);
        assert_eq!(
            x.pkg.versions.get(fid),
            Some(GoVersion::new(want)),
            "{rel} 的生效版本（-lang={:?}）",
            x.lang
        );
    }
}

// ── ④ 四个语料向量：判定面（`T-CK-63` 的接线）──────────────────────

#[test]
fn t_ck_63_语料四向量_判定面() {
    // `go1_19_20`：文件版本 1.20 抬到 1.21 ⇒ 1.20 的切片→数组已允许。
    let x = load("check/go1_19_20.go", None);
    let fid = only_file(&x);
    assert!(allow(&x.pkg.versions, fid, Feat::SliceToArray));

    // `go1_22_21`：文件版本 1.21 **取代** -lang 1.22 ⇒ range over int 被拒；
    // 但 1.21 的 clear/min/max 仍允许（不是「一刀切回 1.22 之前」）。
    let x = load("check/go1_22_21.go", None);
    let fid = only_file(&x);
    assert!(
        !allow(&x.pkg.versions, fid, Feat::RangeOverInt),
        "文件版本 1.21 必须拒 1.22 的 range over int"
    );
    assert!(allow(&x.pkg.versions, fid, Feat::Clear));

    // `go1_xx_19`：没有 -lang，只有 //go:build go1.19 ⇒ max ⇒ 1.21。
    let x = load("check/go1_xx_19.go", None);
    let fid = only_file(&x);
    assert!(allow(&x.pkg.versions, fid, Feat::SliceToArray));

    // `decls0`：-lang=go1.17，无文件版本 ⇒ 类型参数（1.18）被拒。
    let x = load("check/decls0.go", None);
    let fid = only_file(&x);
    assert!(!allow(&x.pkg.versions, fid, Feat::TypeParameter));
    assert!(allow(&x.pkg.versions, fid, Feat::AliasDecl));
}

// ── ⑤ 缺省不门控（`F12`）──────────────────────────────────────────

#[test]
fn t_ck_62_缺省不门控() {
    let src = b"package p\n\nvar x int\n";
    let lang = version_source(None, src).expect("合法");
    assert_eq!(lang, None);
    let mut tt = TypeTable::new();
    let (pkg, _bag) = load_sources(&mut tt, &[("p.go", src.as_slice())], None);
    let fid = pkg.files[0];
    assert_eq!(pkg.versions.get(fid), None, "缺省 ⇒ 表里是不门控");
    assert!(
        allow(&pkg.versions, fid, Feat::GenericMethod),
        "不门控 ⇒ 一切特性允许"
    );
}

// ── ⑥ 检查面版本入口与装载器同源 ─────────────────────────────────

#[test]
fn t_ck_62_check_versions_与装载器同源() {
    let x = load("check/go1_22_21.go", None);
    let fid = only_file(&x);
    let refs: Vec<_> = x.pkg.asts.iter().collect();
    let (v, bag) = versions(x.lang.as_deref(), &refs);
    assert!(bag.is_empty(), "向量文件不该有 TooNew");
    assert_eq!(
        v.get(fid),
        x.pkg.versions.get(fid),
        "检查面入口必须与装载器算出同一张表"
    );
}
