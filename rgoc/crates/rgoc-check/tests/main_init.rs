//! `T-CK-73` `main` / `init` 约束（M4/Task 34）。
//!
//! # 判据来源
//!
//! 官方 `go/types/resolver.go`：`:111`/`:118`（`declarePkgObj` 的两条早退）、
//! `:427`（类型参数面）、`:433`（参数/结果面）、`:437`（`init` 必须有体）。
//!
//! # 夹具与 oracle 证据
//!
//! 主夹具与 `.superpowers/sdd/M4-plan/probe/t33/t34-main.go` **逐字节一致**；
//! oracle `--check` 实测 9 行：
//! `4:6 ret` / `6:6 ret` / `8:6 ret` / `10:6 decl` / `10:11 decl` /
//! `12:11 decl` / `14:6 decl` / `16:5 decl` / `18:5 decl`。
//!
//! 接受夹具与 `probe/t33/t34-accept.go` 一致，oracle **零诊断**。
//!
//! # 三条实测口径（`Ruling 34-2`）
//!
//! 1. **只对非方法**：`func (T) init()` / `func (T) main()` 合法。
//! 2. **`main` 只在 `package main` 里查**。
//! 3. **报点**：类型参数面报在**第一个类型参数字段**；参数/结果面与「无体」
//!    报在**函数名**。
//!
//! # 类别为什么分家
//!
//! 签名面落 `ret`（oracle 适配表的 `return values` 规则先命中），形态面落
//! `decl` —— 见 `main_check` 的模块文档与 C8 §4 的规则序。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_check::main_check;
use rgoc_lex::DiagCode;
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 拒绝夹具（与 `probe/t33/t34-main.go` 逐字节一致）。
const SRC: &str = r#"// T-CK-73 main/init 约束 —— 夹具（Task 34）。
package main

func main(a int) {}

func init(a int) {}

func init() int { return 0 }

func main[T any]() {}

func init[T any]() {}

func init()

var init = 1

var main = 1

type T int

func (T) init() {}

func (T) main() {}
"#;

/// 接受夹具（与 `probe/t33/t34-accept.go` 逐字节一致）：多个 `init`、方法叫
/// `init`/`main`、`main` 可被引用 —— oracle 零诊断。
const ACCEPT_SRC: &str = r#"// T-CK-73 main/init 约束 —— 接受夹具（Task 34）。
package main

func init() {}

func init() {}

type T int

func (T) init() {}

func (T) main() {}

func main() {}

var _ = main
"#;

fn load(src: &str) -> (TypeTable, Package, rgoc_lex::DiagBag) {
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_sources(&mut tt, &[("t.go", src.as_bytes())], None);
    (tt, pkg, bag)
}

fn rows(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// 拒绝向量：9 行，与 oracle 剥列逐项一致。
#[test]
fn t_ck_73_拒绝向量与_oracle_逐行一致() {
    let (mut tt, pkg, bag) = load(SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(
        rows(&checked.render("t.go")),
        vec![
            "4:6\tret",
            "6:6\tret",
            "8:6\tret",
            "10:6\tdecl",
            "10:11\tdecl",
            "12:11\tdecl",
            "14:6\tdecl",
            "16:5\tdecl",
            "18:5\tdecl",
        ],
        "vs oracle（`probe/t33/t34-main.go` 实测）"
    );
}

/// 接受向量：零诊断（多个 `init` / 方法叫 init、main / `main` 可引用）。
#[test]
fn t_ck_73_接受向量零诊断() {
    let (mut tt, pkg, bag) = load(ACCEPT_SRC);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert!(
        checked.diags.is_empty(),
        "got {:?}",
        checked
            .diags
            .iter()
            .map(|r| (r.diag.pos.line, r.diag.pos.col, r.diag.code))
            .collect::<Vec<_>>()
    );
}

/// 消息逐字（官方三句 + `main` 的两句）。
#[test]
fn t_ck_73_消息逐字() {
    let (_tt, pkg, _bag) = load(SRC);
    let diags = main_check::check(&pkg);
    let got: Vec<(u32, &str)> = diags
        .iter()
        .map(|d| (d.pos.line, d.message.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (4, "func main must have no arguments and no return values"),
            (6, "func init must have no arguments and no return values"),
            (8, "func init must have no arguments and no return values"),
            (10, "func main must have no type parameters"),
            (12, "func init must have no type parameters"),
            (14, "func init must have a body"),
            (16, "cannot declare init - must be func"),
            (18, "cannot declare main - must be func"),
        ],
        "消息与报点"
    );
}

/// `init` **不进包作用域**（官方 `resolver.go:437`）⇒ 引用它是 `undefined`。
#[test]
fn t_ck_73_init_不在包作用域() {
    const S: &str = "package main\n\nfunc init() {}\n\nvar x = init\n";
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_sources(&mut tt, &[("t.go", S.as_bytes())], None);
    assert!(pkg.lookup("init").is_none(), "`init` 不该在包作用域里");
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(
        rows(&checked.render("t.go")),
        vec!["5:9\tundef"],
        "引用 `init` ⇒ undefined"
    );
}

/// 多个 `init` **不**报重名（不进作用域就没有「重名」这一说）。
#[test]
fn t_ck_73_多个_init_不报重名() {
    const S: &str = "package p\n\nfunc init() {}\nfunc init() {}\nfunc init() {}\n";
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_sources(&mut tt, &[("t.go", S.as_bytes())], None);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert!(
        checked.diags.is_empty(),
        "got {:?}",
        checked
            .diags
            .iter()
            .map(|r| (r.diag.pos.line, r.diag.code))
            .collect::<Vec<_>>()
    );
}

/// `package p` 里的 `func main()` 不受约束（官方只在 `package main` 查）。
#[test]
fn t_ck_73_非_main_包的_main_不受约束() {
    const S: &str = "package p\n\nfunc main(a int) int { return 0 }\n";
    let (_tt, pkg, _bag) = load(S);
    assert!(main_check::check(&pkg).is_empty(), "非 main 包不查");
}

/// 方法面不受约束：`func (T) main[T any]()` 之类只在**函数**面查。
#[test]
fn t_ck_73_方法面不受约束() {
    const S: &str = "package main\n\ntype T int\n\nfunc (T) main(a int) int { return 0 }\n";
    let (_tt, pkg, _bag) = load(S);
    assert!(main_check::check(&pkg).is_empty(), "方法不查 main/init");
}

/// 非函数 `main` 只报一条（**不**再叠加「重名」—— 官方早退、不进作用域）。
#[test]
fn t_ck_73_非函数_main_只报一条() {
    const S: &str = "package main\n\nvar main = 1\n\nfunc main() {}\n";
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_sources(&mut tt, &[("t.go", S.as_bytes())], None);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(
        rows(&checked.render("t.go")),
        vec!["3:5\tdecl"],
        "只有 cannot declare main 一条"
    );
    assert_eq!(checked.diags[0].diag.code, DiagCode::TypInvalidMainDecl);
}
