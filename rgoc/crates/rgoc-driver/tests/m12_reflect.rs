//! M12 b 线（有限反射）的判据。
//!
//! # T-REF-00：三个子命令都收 `-I`
//!
//! `types` 从 M3 起就有 `-I <root>`（import 解析根，`Ruling 27-1`），但
//! `check` / `run` / `build` 一直传 `&[]` —— b 线要在 `stdlib/reflect`
//! 上做文章，而反射包自己要被 import，没有 `-I` 就什么都摆不开。
//! 判据：`-I <tmp>/root` 里放一个 `fmt/fmt.go` 桩，三个子命令都必须认
//! 这个旗标并把根透传进 `load_with_imports`。
//!
//! # 为什么 run/build 的 main 只 import 不使用
//!
//! 检查面能解析跨包引用（① 用 `fmt.Println`），但 **HIR lowering 还在
//! M5 的单包面**（实测：`跨包引用 fmt.Println 不在 M5 面`）—— 跨包调用
//! 进 HIR 是另一笔账，不在这个 Task 的范围里。run/build 这里要钉的只有
//! 「旗标被接受 + 根透传 + import 不再被拒」这一件事，所以用
//! 「import 但不使用」的 main（未使用 import 今天不报诊断，与官方的
//! 「imported and not used」的差距属于检查面覆盖，不在本判据管）。
//!
//! 这条判据**不是**反射语义的判据 —— 它只是 b 线的地基：没有 `-I`，
//! `import "reflect"` 连第一步都走不进去。

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn driver() -> &'static str {
    env!("CARGO_BIN_EXE_rgoc-driver")
}

/// `root/fmt/fmt.go` 桩 + 一个 `import "fmt"` 的 main，返回 `(root, main)`。
///
/// `use_it` = true ⇒ main 里真的调 `fmt.Println`（给检查面用）；
/// false ⇒ 只 import（给 run/build 用，见模块注释）。
fn setup(root_tag: &str, use_it: bool) -> (PathBuf, PathBuf) {
    let tmp = std::env::temp_dir().join(format!(
        "rgoc-m12-reflect-{root_tag}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("root/fmt")).expect("建 root/fmt 失败");
    fs::write(
        tmp.join("root/fmt/fmt.go"),
        "package fmt\n\nfunc Println(s string) {}\n",
    )
    .expect("写 fmt 桩失败");
    let body = if use_it {
        "func main() { fmt.Println(\"x\") }\n"
    } else {
        "func main() {}\n"
    };
    let main = tmp.join("main.go");
    fs::write(&main, format!("package main\n\nimport \"fmt\"\n\n{body}")).expect("写 main 失败");
    (tmp.join("root"), main)
}

/// 旗标写成 `-I root`（分参数，与 `types` 同款）。
#[test]
fn t_ref_00_import_root() {
    // ── ① check：跨包引用在检查面必须解析到 ─────────────────────────
    let (root, main) = setup("check", true);
    let out = Command::new(driver())
        .args(["check", "-I"])
        .arg(&root)
        .arg(&main)
        .output()
        .expect("起不了 check");
    let _ = fs::remove_dir_all(root.parent().unwrap());
    assert_eq!(
        out.status.code(),
        Some(0),
        "check -I 应当认旗标并解析到 fmt 桩：\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // ── ② run：旗标被接受 + import 不再被拒 ─────────────────────────
    let (root, main) = setup("run", false);
    let out = Command::new(driver())
        .args(["run", "-I"])
        .arg(&root)
        .arg(&main)
        .output()
        .expect("起不了 run");
    let _ = fs::remove_dir_all(root.parent().unwrap());
    assert_eq!(
        out.status.code(),
        Some(0),
        "run -I 应当认旗标并解析到 fmt 桩：\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // ── ③ build：同上 ───────────────────────────────────────────────
    let (root, main) = setup("build", false);
    let out = Command::new(driver())
        .args(["build", "-I"])
        .arg(&root)
        .arg(&main)
        .output()
        .expect("起不了 build");
    let _ = fs::remove_dir_all(root.parent().unwrap());
    assert_eq!(
        out.status.code(),
        Some(0),
        "build -I 应当认旗标并解析到 fmt 桩：\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // ── ④ 负例：没有 -I 时 import 必须照旧被拒（旗标不是万能开关）────
    let (root, main) = setup("negative", false);
    let out = Command::new(driver())
        .args(["check"])
        .arg(&main)
        .output()
        .expect("起不了 check（负例）");
    let _ = fs::remove_dir_all(root.parent().unwrap());
    assert_eq!(
        out.status.code(),
        Some(1),
        "没有 -I 时 import \"fmt\" 必须照旧被拒：\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("TYP-UNSUPPORTED-IMPORT-FORM"),
        "拒绝理由应当还是 import 形式不支持：\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
