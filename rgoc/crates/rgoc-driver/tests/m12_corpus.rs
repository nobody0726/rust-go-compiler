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
    let out = cmd
        .output()
        .expect("跑不了 m12-classify.sh（bash 不在 PATH？）");
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
    assert_eq!(
        count_of(&out, "sum"),
        Some(266),
        "七份之和不是 266：\n{out}"
    );

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

    assert_eq!(
        rc, 1,
        "合成语料只有 3 个文件，`--check` 必须红：\nstdout:\n{out}\nstderr:\n{err}"
    );
    assert!(
        err.contains("!= 266"),
        "红的原因必须是「总数不对」，实际 stderr：\n{err}"
    );
    // 顺便证明三份都归到了对的地方（3 个文件：run 1 / rejected 1 / dir 1）
    assert_eq!(count_of(&out, "run"), Some(1), "a.go 没进 run：\n{out}");
    assert_eq!(
        count_of(&out, "rejected"),
        Some(1),
        "b.go 没进 rejected：\n{out}"
    );
    assert_eq!(count_of(&out, "dir"), Some(1), "c.go 没进 dir：\n{out}");
}

// ════════════════════════════════════════════════════════════════════
// T-M12-02 装置打的是真靶
// ════════════════════════════════════════════════════════════════════

/// 跑 `run-native-typeparam.sh`，返回 `(退出码, 合并输出)`。
fn run_oracle(args: &[&str]) -> (i32, String) {
    let out = Command::new("bash")
        .arg(repo_root().join("tools/run-oracle/run-native-typeparam.sh"))
        .args(args)
        .output()
        .expect("跑不了 run-native-typeparam.sh");
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), s)
}

/// 跑装置，但用**临时语料 / 临时清单 / 临时 OUT**（脚本头声明的三个环境变量）。
fn run_oracle_in(tmp: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new("bash")
        .arg(repo_root().join("tools/run-oracle/run-native-typeparam.sh"))
        .args(args)
        .env("RGOC_TYPEPARAM_DIR", tmp.join("corpus"))
        .env("RGOC_M12_LISTS", tmp.join("lists"))
        .env("RGOC_M12_ORACLE", tmp.join("out"))
        .output()
        .expect("跑不了 run-native-typeparam.sh（合成语料）");
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), s)
}

/// **红**的必须报 BAD，**绿**的必须报 OK。
///
/// # 为什么这条是「装置自身」的判据而不是语料的判据
///
/// 「装置能跑起来」不构成证据：一个永远报 OK 的装置也能跑。两边同时钉住
/// （红的必须红、绿的必须绿）才排除了「恒 OK」与「恒 BAD」两种坏法。
///
/// # 为什么红靶改成了**合成反例**（§17.2 之后）
///
/// 这条判据的旧版拿 `dottype.go` 当「已知红」—— 但 Task 24（§17.2）把
/// `U-M12-09` / `U-M12-S15` 的 8 条 native 输出级老缺口**冻结进白名单**之后，
/// run 族已经**没有天然红的文件**了（71 条 = 63 绿 + 8 SKIP）。而历史教训
/// （Task 15 时 absdiff 从红翻绿）恰好说明：把判据绑在「今天恰好红」的
/// 语料上，状态一过期判据就跟着红 —— 装置没坏，是登记的状态过期了。
///
/// 所以照 `M10` / `M11` manifest 的同款思路（「合成反例都必须红」）：
/// 红靶改用**自己造**的语料 —— 一个保证 `build` 失败的文件 + 一个真绿的
/// `absdiff.go`，通过装置自己的环境变量口子（`RGOC_TYPEPARAM_DIR` /
/// `RGOC_M12_LISTS` / `RGOC_M12_ORACLE`）喂进去。合成反例**永远**红，
/// 不再随实现进展翻绿；绿靶继续在真实语料上验 `--filter` 生效。
#[test]
fn t_m12_02_装置打的是真靶() {
    // ── 红靶：合成语料（1 绿 + 1 必红）───────────────────────────────
    let tmp = std::env::temp_dir().join(format!("rgoc-m12-oracle-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("corpus")).expect("建临时语料目录失败");
    fs::create_dir_all(tmp.join("lists")).expect("建临时清单目录失败");
    // 绿：真语料的 absdiff.go 原样拷入（官方 go run 跑得通 ⇒ 走完整 L1+L2）。
    fs::copy(corpus().join("absdiff.go"), tmp.join("corpus/absdiff.go"))
        .expect("拷 absdiff.go 失败");
    // 红：`undefined` 必然 check 失败 ⇒ 装置在 build 步就报 BAD。
    fs::write(
        tmp.join("corpus/zzmutant.go"),
        "// run\npackage main\n\nfunc main() { undefined() }\n",
    )
    .expect("写 zzmutant.go 失败");
    // 清单只有这两个文件 —— 分母是 2，顺带证明临时清单真被用上了。
    fs::write(
        tmp.join("lists/typeparam-run.txt"),
        "absdiff.go\nzzmutant.go\n",
    )
    .expect("写临时清单失败");

    let (rc, out) = run_oracle_in(&tmp, &["--list", "--only", "run"]);
    let _ = fs::remove_dir_all(&tmp);
    assert_eq!(rc, 1, "含必红文件的语料，装置必须退出 1：\n{out}");
    assert!(
        out.contains("BAD") && out.contains("zzmutant.go"),
        "红的理由必须指名道姓点到 zzmutant.go：\n{out}"
    );
    assert!(
        out.contains("OK") && out.contains("absdiff.go"),
        "absdiff.go 必须照常报 OK：\n{out}"
    );
    assert!(
        out.contains("run 1/2"),
        "两个文件必须 1 绿 1 红（不是 0/2 或 2/2）：\n{out}"
    );

    // ── 绿靶：真实语料 + `--filter` ─────────────────────────────────
    let (rc_ok, out_ok) = run_oracle(&["--list", "--only", "run", "--filter", "absdiff.go"]);
    assert_eq!(rc_ok, 0, "absdiff.go 今天应当绿：\n{out_ok}");
    assert!(out_ok.contains("OK"), "应当有一行 OK：\n{out_ok}");
    assert!(
        out_ok.contains("run 1/1"),
        "只筛了 1 个文件，分母必须是 1（不是 71 —— 否则 `--filter` 没生效）：\n{out_ok}"
    );
}
