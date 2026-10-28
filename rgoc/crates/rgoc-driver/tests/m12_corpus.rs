//! M12 语料装置的判据（`T-M12-01`，`M12-tests.md` §8）。
//!
//! 装置是 `tools/run-oracle/m12-classify.sh` —— 它把 `test/typeparam` 的 266 个
//! `.go` 划成**七份清单**。判据两件事：
//!
//! 1. 划分**自洽**（七份之和 == 266、两两不相交、每份从语料算出来）；
//! 2. 装置**有牙**（语料变了就红）—— 只查「脚本能跑」等于没查。
//!
//! # 为什么第 2 条要单独一条判据
//!
//! 第 1 条在脚本写对之后**恒真**：脚本自己就在断言和 == 266。一条恒真的判据
//! 抓不到「脚本把断言写漏了」。所以第 2 条拿一个**合成的** 3 文件语料去跑，
//! 断言它必须**红**且报出「!= 266」—— 这才证明那个断言真的在。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达（cargo 的 CARGO_MANIFEST_DIR 变了？）")
}

fn script() -> PathBuf {
    repo_root().join("tools/run-oracle/m12-classify.sh")
}

fn corpus() -> PathBuf {
    repo_root().join("go_source_code/test/typeparam")
}

/// 跑 `m12-classify.sh --check`，返回 `(退出码, stdout, stderr)`。
///
/// `corpus_override` 非空时用 `RGOC_TYPEPARAM_DIR` 换语料目录。
fn run_check(corpus_override: Option<&Path>) -> (i32, String, String) {
    let mut cmd = Command::new("bash");
    cmd.arg(script()).arg("--check");
    if let Some(dir) = corpus_override {
        cmd.env("RGOC_TYPEPARAM_DIR", dir);
    }
    let out = cmd.output().expect("跑不了 m12-classify.sh（bash 不在 PATH？）");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// 从汇总输出里取某一份清单的条数（脚本的行格式是 `  名字  <n>  （…）`）。
fn count_of(stdout: &str, name: &str) -> Option<usize> {
    stdout.lines().find_map(|line| {
        let mut it = line.split_whitespace();
        if it.next()? != name {
            return None;
        }
        it.next()?.parse().ok()
    })
}

// ════════════════════════════════════════════════════════════════════
// T-M12-01 七份清单自洽
// ════════════════════════════════════════════════════════════════════

#[test]
fn t_m12_01_七份清单自洽() {
    let (rc, out, err) = run_check(None);
    assert_eq!(rc, 0, "划分不自洽：\nstdout:\n{out}\nstderr:\n{err}");

    // ① 七份的条数（`M12-baseline.md` §1 的实测值）
    let want = [
        ("run", 71usize),
        ("compile", 40),
        ("errorcheck", 5),
        ("errorcheck-m", 2),
        ("rejected", 76),
        ("dir", 67),
        ("none", 5),
    ];
    for (name, n) in want {
        assert_eq!(
            count_of(&out, name),
            Some(n),
            "清单 `{name}` 的条数不是 {n}：\n{out}"
        );
    }

    // ② 脚本自报的和
    assert_eq!(count_of(&out, "sum"), Some(266), "七份之和不是 266：\n{out}");

    // ③ **自己**数一遍语料（不信任脚本的自报）
    let total = fs::read_dir(corpus())
        .expect("读不到语料目录")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "go"))
        .count();
    assert_eq!(total, 266, "语料文件总数不是 266（语料变了 ⇒ F1 要重测）");

    // ④ 七份之和 == 自己数的总数
    let sum: usize = want.iter().map(|(_, n)| *n).sum();
    assert_eq!(sum, total, "七份之和 {sum} != 语料总数 {total}");
}

// ════════════════════════════════════════════════════════════════════
// T-M12-01b 装置有牙：合成语料必须被拒
// ════════════════════════════════════════════════════════════════════

/// 造一个只有 3 个文件的语料，断言 `--check` **必须红**且报「!= 266」。
///
/// 三个文件各代表一类，确保红的原因是**总数不对**而不是某个解析崩了：
/// `a.go`（`// run`，无 import）、`b.go`（`// compile`，带 import ⇒ 进拒绝面）、
/// `c.go`（`// rundir` 桩）。
#[test]
fn t_m12_01b_装置有牙() {
    let tmp = std::env::temp_dir().join(format!("rgoc-m12-classify-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("建临时语料目录失败");
    fs::write(tmp.join("a.go"), "// run\npackage main\n\nfunc main() {}\n").unwrap();
    fs::write(
        tmp.join("b.go"),
        "// compile\npackage p\n\nimport \"fmt\"\n\nvar _ = fmt.Println\n",
    )
    .unwrap();
    fs::write(tmp.join("c.go"), "// rundir\npackage ignored\n").unwrap();

    let (rc, out, err) = run_check(Some(&tmp));
    let _ = fs::remove_dir_all(&tmp);

    assert_eq!(rc, 1, "合成语料只有 3 个文件，`--check` 必须红：\nstdout:\n{out}\nstderr:\n{err}");
    assert!(
        err.contains("!= 266"),
        "红的原因必须是「总数不对」，实际 stderr：\n{err}"
    );
    // 顺便证明三份都归到了对的地方（3 个文件：run 1 / rejected 1 / dir 1）
    assert_eq!(count_of(&out, "run"), Some(1), "a.go 没进 run：\n{out}");
    assert_eq!(count_of(&out, "rejected"), Some(1), "b.go 没进 rejected：\n{out}");
    assert_eq!(count_of(&out, "dir"), Some(1), "c.go 没进 dir：\n{out}");
}
