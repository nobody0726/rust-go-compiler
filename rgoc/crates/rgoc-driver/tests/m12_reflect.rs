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

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("仓库根")
        .to_path_buf()
}

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

// ════════════════════════════════════════════════════════════════════
// T-REF-01 白名单对账（Task 29）
// ════════════════════════════════════════════════════════════════════

/// `stdlib/reflect/reflect.go` 的导出 `func`/`type` 集合必须与这里登记的
/// 集合**相等**（两边都从源码读：源码侧正则提取，判据侧下表登记）。
///
/// # 为什么是「相等」不是「包含」
///
/// 反射的面是**刻意收窄**的（`U-M12-02`：不做 `MakeFunc`）。用「包含」
/// 的话，将来有人顺手加一个导出符号，白名单纹丝不动 —— 收窄面就漏了。
/// 相等意味着：**多一个、少一个都得来改这张表**（§11.3 的冻结纪律）。
///
/// # 为什么 `MakeFunc` 单独点名
///
/// 它是官方反射里最常用的高阶入口，也最容易被「顺手补上」—— 但它需要
/// 反向 trampoline（Go 调栈 → C 调栈的 ABI 桥），是独立的一块工作。
/// 断言它**不在**集合里：想加它必须同时改这条判据（露头就红）。
///
/// # 注册集合 = Task 29 骨架的实际交付
///
/// 后续 Task（31 读面 / 32 方法表 / 33 Call / 34 写面）每加一个导出
/// `func`/`type`，这张表跟着长一行 —— 那是有意识的动作，不是静默扩散。
#[test]
fn t_ref_01_白名单对账() {
    let src = fs::read_to_string(repo_root().join("stdlib/reflect/reflect.go"))
        .expect("stdlib/reflect/reflect.go 不存在（b 线骨架还没落？）");

    // 源码侧：顶层导出 func / type（行首无缩进）。
    let mut got: BTreeSet<String> = BTreeSet::new();
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("func ") {
            let name = rest
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .next()
                .unwrap_or("");
            if name.starts_with(|c: char| c.is_ascii_uppercase()) {
                got.insert(format!("func {name}"));
            }
        }
        if let Some(rest) = line.strip_prefix("type ") {
            let name = rest
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .next()
                .unwrap_or("");
            if name.starts_with(|c: char| c.is_ascii_uppercase()) {
                got.insert(format!("type {name}"));
            }
        }
    }

    // 判据侧：登记集合（Task 29 骨架）。
    let want: BTreeSet<String> = [
        "type Kind",
        "type Type",
        "type Value",
        "func TypeOf",
        "func ValueOf",
    ]
    .into_iter()
    .map(String::from)
    .collect();

    let extra: Vec<_> = got.difference(&want).collect();
    let missing: Vec<_> = want.difference(&got).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "导出面与登记表不一致：\n多出的（要改表先说明理由）：{extra:?}\n少了的：{missing:?}"
    );

    assert!(
        !got.iter().any(|s| s.contains("MakeFunc")),
        "MakeFunc 不该出现（U-M12-02：需要反向 trampoline，独立工作）"
    );

    // 骨架必须过检查面（-I stdlib 指向仓库的 stdlib/ 根）。
    let main = std::env::temp_dir().join(format!("rgoc-ref01-main-{}.go", std::process::id()));
    fs::write(
        &main,
        "package main\n\nimport \"reflect\"\n\nfunc main() { _ = reflect.TypeOf(0); _ = reflect.ValueOf(0) }\n",
    )
    .expect("写 main 失败");
    let out = Command::new(driver())
        .args(["check", "-I"])
        .arg(repo_root().join("stdlib"))
        .arg(&main)
        .output()
        .expect("起不了 check");
    let _ = fs::remove_file(&main);
    assert_eq!(
        out.status.code(),
        Some(0),
        "reflect 骨架自己必须过检查面：\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
