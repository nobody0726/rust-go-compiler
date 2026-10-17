//! C7 对撞（`T-TY-91`）—— 把**冻结下来的 oracle golden**（`.want`）与
//! **rgoc 侧现算的 dump** 逐字节对上。
//!
//! # 这为什么算「对撞」
//!
//! 腿乙的完整形态是「两侧**现跑**同一份 fixture，输出逐字节一致」，那由
//! `tools/types-oracle/run-collide.sh` 承担（它要同时驱动 `go` 与 `cargo`，
//! 只能在容器里跑）。本测试换的是同一个判据的**另一半**：
//!
//! | 谁 | 断言 |
//! |---|---|
//! | `tools/types-oracle/dump_test.go`（oracle 侧） | oracle **现跑** == `.want` |
//! | **本文件**（rgoc 侧） | rgoc **现跑** == 同一个 `.want` |
//!
//! 两条都成立 ⟹ 「rgoc == oracle」成立（传递性），而 `.want` 是**冻结在库里**的
//! 判据：它变了就是有人**有意**改了口径（要走 C7 的订正流程），不是「今天
//! oracle 心情不同」。这也是为什么这里**不**直接 `go run .` —— 那样测的会变成
//! 「这台机器的 go 版本 + 模块缓存」，且每次都要几秒启动成本。
//!
//! # 口径
//!
//! - `#` 开头的行与空行两侧都忽略（C7 §2 规则 1）—— `# source: oracle` /
//!   `# source: rgoc` 正是**刻意保留**的那一处不同；
//! - fixture 必须**合法**（rgoc 侧退出码 0、stderr 为空）：不合法的话 dump 的
//!   一半是 `invalid`，那种「对撞」没有意义（`M3-tests.md` §5.1）；
//! - fixture 集合 = `testdata/type-dump-*.go`。`cmp-*.go` 归 `collide_cmp.rs`
//!   （一个 fixture 只能有一份 `.want`）；「testdata 里每个 `.go` 都属于某一族」
//!   由 oracle 侧的 `TestTestdataNaming` 守。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达（cargo 的 CARGO_MANIFEST_DIR 变了？）")
}

fn testdata() -> PathBuf {
    repo_root().join("tools/types-oracle/testdata")
}

/// C7 §2 规则 1：丢注释行与空行，其余**逐字节**保留（含缩进与行序）。
fn body(s: &str) -> String {
    let lines: Vec<&str> = s
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .collect();
    lines.join("\n")
}

/// 首个差异行的邻域 —— 直接 `assert_eq!` 两个 String 会印出全部内容，
/// 在 30 行的 dump 上还能忍，在以后的大 fixture 上就只剩噪音。
fn first_diff(want: &str, got: &str) -> String {
    let w: Vec<&str> = want.lines().collect();
    let g: Vec<&str> = got.lines().collect();
    for i in 0..w.len().max(g.len()) {
        let wl = w.get(i).copied().unwrap_or("<无此行>");
        let gl = g.get(i).copied().unwrap_or("<无此行>");
        if wl != gl {
            let mut s = format!(
                "首个差异在第 {} 行：\n  oracle: {wl}\n  rgoc  : {gl}\n",
                i + 1
            );
            for j in i.saturating_sub(2)..(i + 3).min(w.len().max(g.len())) {
                s.push_str(&format!(
                    "  [{}] oracle={:?} rgoc={:?}\n",
                    j + 1,
                    w.get(j).copied().unwrap_or("<无>"),
                    g.get(j).copied().unwrap_or("<无>")
                ));
            }
            return s;
        }
    }
    "两侧正文相同（差异只可能在行尾/换行 —— 那也该由逐字节比对上）".to_string()
}

fn run_dump(fixture: &Path) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rgoc-driver"))
        .args(["types", "--dump"])
        .arg(fixture)
        .output()
        .expect("起 rgoc-driver 进程");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// `T-TY-91`（rgoc 侧）：`testdata/type-dump-*.go` 的现算 dump 与 oracle golden
/// **逐字节一致**，且 fixture 本身**合法**（退出码 0、stderr 空）。
#[test]
fn dump_对撞_全部_type_dump_fixture() {
    let dir = testdata();
    assert!(
        dir.is_dir(),
        "找不到 {} —— 对撞的 fixture 目录（跑测试要在仓库里）",
        dir.display()
    );

    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("读 testdata")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            let n = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            n.starts_with("type-dump-") && n.ends_with(".go")
        })
        .collect();
    fixtures.sort();
    assert!(
        !fixtures.is_empty(),
        "{} 里一个 type-dump-*.go 都没有 —— 这条对撞什么都没测",
        dir.display()
    );

    for f in &fixtures {
        let name = f
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let want_path = f.with_extension("want");
        let want_raw = std::fs::read_to_string(&want_path).unwrap_or_else(|e| {
            panic!(
                "读不到 {}（新 fixture 要在 tools/types-oracle 里跑一次 `go test ./... -update` 生成 golden）：{e}",
                want_path.display()
            )
        });

        let (code, stdout, stderr) = run_dump(f);
        assert_eq!(
            code, 0,
            "{name}：fixture 必须**合法**（`M3-tests.md` §5.1），rgoc 侧退出码是 {code}\nstderr:\n{stderr}"
        );
        assert!(
            stderr.is_empty(),
            "{name}：fixture 合法 ⟹ stderr 必须为空（诊断不许混进这条通道）\n{stderr}"
        );

        let want = body(&want_raw);
        let got = body(&stdout);
        assert!(
            want == got,
            "{name}：rgoc 侧 dump 与 oracle golden 不一致\n{}",
            first_diff(&want, &got)
        );
    }
}

/// 反向钉：**每个** `type-dump-*.go` 都得有 `.want`，且 `.want` 非空。
///
/// 少了 golden 的 fixture 在**两条腿**上都会「通过」—— 本文件因为读不到而失败
/// 算是好的；真正阴的是 `.want` **存在但被清空**（两侧都比空 ⇒ 一致）。所以这里
/// 把「非空」也钉住。
#[test]
fn dump_golden_都在且非空() {
    let dir = testdata();
    let mut n = 0;
    for e in std::fs::read_dir(&dir).expect("读 testdata") {
        let p = e.expect("目录项").path();
        let name = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if !(name.starts_with("type-dump-") && name.ends_with(".go")) {
            continue;
        }
        n += 1;
        let want = p.with_extension("want");
        let text = std::fs::read_to_string(&want)
            .unwrap_or_else(|e| panic!("{} 缺 golden：{e}", want.display()));
        assert!(
            !body(&text).is_empty(),
            "{} 的正文是空的 —— 「两侧都空」是最阴的一种通过",
            want.display()
        );
    }
    assert!(n > 0, "一个 fixture 都没有");
}
