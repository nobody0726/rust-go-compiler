//! M12 检查面判据（`T-GEN-*` 的检查面部分，`M12-tests.md` §3）。
//!
//! 这一族判据只管**前端**：`check` 退出 0 就说「这段代码在静态面上是对的」。
//! 它们全部拿命令行真身（`CARGO_BIN_EXE_rgoc-driver`）跑，不调用库函数 ——
//! 因为要判的是「用户看到的行为」，而 M12 已知的第一个真错（`T-GEN-08a`）
//! 恰恰是**两个子命令对同一份源码给出不同结论**，那种错只在命令行边界上可见。
//!
//! # 为什么门禁文件清单是**算**出来的
//!
//! `M12-baseline.md` §1 记过三次「谓词打错靶」的翻车（`^// run$` 漏掉带尾随
//! 参数的文件、`grep '^import'` 被块注释骗、`'^import|"'` 把带字符串字面量的
//! 文件全误排除）。所以这里不抄一份文件名清单，而是让 `m12-classify.sh` 现算
//! 一份到临时目录再读 —— 语料一变，这里跟着变。

use std::collections::BTreeSet;
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

fn corpus() -> PathBuf {
    repo_root().join("go_source_code/test/typeparam")
}

/// 现算一份清单到 `out`（`m12-classify.sh --out out`）。
fn classify(out: &Path) {
    let st = Command::new("bash")
        .arg(repo_root().join("tools/run-oracle/m12-classify.sh"))
        .arg("--out")
        .arg(out)
        .status()
        .expect("跑不了 m12-classify.sh（bash 不在 PATH？）");
    assert!(st.success(), "分类装置自己失败了（划分不自洽？）");
}

/// 读一份清单（一行一个文件名）。
fn read_list(dir: &Path, name: &str) -> Vec<String> {
    let p = dir.join(format!("typeparam-{name}.txt"));
    let s = fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不了 {}：{e}", p.display()));
    s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// 从诊断渲染里取**检查面**的 `[...]` 码（去重、保序）。
///
/// 只认全大写的码（`CHK-*` / `TYP-*` / `HIR-*` / `SYN-*`）—— clang 的输出里有
/// 大量方括号（`[-Werror]` 之类），混进来会把「下游失败」误判成「检查面失败」。
///
/// **排除 `SSA-*` / `CG-*`**：那是后端能力面（`M11` 的
/// `SSA-UNSUPPORTED-CHAN`、`M12` 的 `CG-UNSUPPORTED-GO-ARGS`）。它们出现说明
/// 「前端过了、后端不支持」，是**另一个面**的账 —— 混进来会让这条判据去催
/// 后端，而不是催检查面。
fn codes_of(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] != '[' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut j = start;
        while j < b.len() && b[j] != ']' {
            j += 1;
        }
        if j < b.len() {
            let c: String = b[start..j].iter().collect();
            let ok = c.len() >= 5
                && c.chars().next().is_some_and(|x| x.is_ascii_uppercase())
                && c.chars()
                    .all(|x| x.is_ascii_uppercase() || x.is_ascii_digit() || x == '-')
                && c.contains('-')
                && !c.starts_with("SSA-")
                && !c.starts_with("CG-");
            if ok && !out.contains(&c) {
                out.push(c);
            }
        }
        i = j + 1;
    }
    out
}

/// `check` 的结论：`(退出码, 码集合)`。
fn check_verdict(file: &Path) -> (i32, Vec<String>) {
    let out = Command::new(env!("CARGO_BIN_EXE_rgoc-driver"))
        .arg("check")
        .arg(file)
        .output()
        .expect("起不了 rgoc-driver check");
    let mut all = String::from_utf8_lossy(&out.stdout).into_owned();
    all.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), codes_of(&all))
}

/// `build` 的结论：`(退出码, 码集合)`。
///
/// `bin_dir` 只用来放产物 —— 判据不看产物，看的是「build 的前置 check 说了什么」。
fn build_verdict(file: &Path, bin_dir: &Path) -> (i32, Vec<String>) {
    let stem = file.file_stem().unwrap_or_default().to_string_lossy();
    let out = Command::new(env!("CARGO_BIN_EXE_rgoc-driver"))
        .args(["build", "--emit=elf", "-o"])
        .arg(bin_dir.join(stem.as_ref()))
        .arg(file)
        .output()
        .expect("起不了 rgoc-driver build");
    let mut all = String::from_utf8_lossy(&out.stdout).into_owned();
    all.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), codes_of(&all))
}

// ════════════════════════════════════════════════════════════════════
// T-GEN-08a `check` 与 `build` 的口径一致
// ════════════════════════════════════════════════════════════════════

/// 同一份源码，`check` 与 `build` 的**前置检查结论**必须相同。
///
/// # 判据形态
///
/// * `check` 过（rc=0）⇒ `build` 的 stderr 里**没有**任何检查面诊断码。
///   （build 可以因为下游原因失败 —— `clang` 链接失败、`SSA` 拒、verifier
///   拒 —— 但那些不带 `CHK-*` / `TYP-*` 的码。）
/// * `check` 不过 ⇒ `build` 也报出那些码（检查面结论不会被 `build` 放宽）。
///
/// # 为什么它算 M12 的判据而不是「早就该有的卫生检查」
///
/// 这两条路径走的是**同一个** `check_package_with`（`build_cmd.rs:84` /
/// `check_cmd.rs`），唯一区别是 `build` 传了 lowering 闭包。所以「结论不同」
/// 只可能来自 lowering 期新报的诊断 —— 而那些诊断在 `M12a` 里会**成片增加**
/// （实例化后 body 才真正被检查）。没有这条判据，`M12a` 的进展就会被
/// 「check 绿了、build 红着」这种分裂吃掉，且**看不出来是谁的错**。
///
/// # 今天的状态（`F14` 的**完整**口径）
///
/// `M12-baseline.md` 记的是「7 个不一致」，那是按 `build_kind = other` 数的
/// （`TYP-INVALID-CONVERSION` 1 / `CHK-INVALID-OPERATION` 5 / clang 链接失败 1）。
/// 本判据的口径更宽，于是**当天实测 29 个**：
///
/// * `HIR-MISSING-TYPE` 6 —— lowering 拿不到类型（类型参数没实例化）；
/// * `CHK-INVALID-SELECTOR` 7 —— 实例上的字段/方法选择器在 lowering 期才报
///   （就是记忆里那条「类型表达式两份实现」的坑，Task 15）；
/// * `CHK-INVALID-OPERATION` 4 / `TYP-INVALID-CONVERSION` 1 —— 类型参数上的
///   运算与转换（Task 8）；
/// * 其余是同一批文件的不同码。
///
/// 宽口径是**故意**的：这条判据要当 `M12a` 的**总账**用 —— 它必须收敛到 0，
/// 而不是「`build_kind` 的分类里没有 other」。窄口径会让 22 个文件在
/// 「check 绿 / build 红」的分裂里躺着，而没人看见。
///
/// 第 7 个（`issue52124.go`）是 **clang 链接失败**，不带检查面码 ⇒ 按本判据
/// **算一致**。这条差别写在断言消息里，免得下一个人以为是漏了。
#[test]
fn t_gen_08a_check_与_build_口径一致() {
    let lists = repo_root().join("target/m12-checkface-lists");
    let bins = repo_root().join("target/m12-checkface-bins");
    let _ = fs::remove_dir_all(&lists);
    let _ = fs::remove_dir_all(&bins);
    fs::create_dir_all(&bins).expect("建不了产物目录");

    classify(&lists);
    let mut files: Vec<String> = read_list(&lists, "run");
    files.extend(read_list(&lists, "compile"));
    assert_eq!(files.len(), 111, "门禁文件数变了（run + compile 应当 111）");

    let mut split: Vec<String> = Vec::new();
    for b in &files {
        let f = corpus().join(b);
        let (crc, ccodes) = check_verdict(&f);
        let (brc, bcodes) = build_verdict(&f, &bins);
        if crc == 0 && !bcodes.is_empty() {
            split.push(format!(
                "{b}: check 过（rc=0），但 build 报 {:?}（build rc={brc}）",
                bcodes
            ));
        } else if crc != 0 {
            let missing: Vec<&String> = ccodes.iter().filter(|c| !bcodes.contains(c)).collect();
            if !missing.is_empty() {
                split.push(format!(
                    "{b}: check 报 {:?}（rc={crc}），build 少了 {:?}（build rc={brc}）",
                    ccodes, missing
                ));
            }
        }
    }

    assert!(
        split.is_empty(),
        "{} 个文件的 check / build 口径不一致。\n这两条路径共用同一个 \
         `check_package_with`，差别只有 lowering —— 所以差异来自 lowering 期\
         新报的诊断。\n（注：clang 链接失败**不算**不一致：它不带检查面诊断码。）\n\n{}",
        split.len(),
        split.join("\n")
    );
}

/// 门禁清单与语料对得上：每条都是真存在的文件，且 run / compile 不相交。
///
/// 这条是给上面那条**兜底**的：若 `classify` 因为环境变量指错目录而返回一份
/// 空清单，上面那条会「0 个不一致」地通过 —— 一个不跑的判据。
#[test]
fn t_gen_08a_门禁清单非空且文件存在() {
    let lists = repo_root().join("target/m12-checkface-lists2");
    let _ = fs::remove_dir_all(&lists);
    classify(&lists);
    let run = read_list(&lists, "run");
    let comp = read_list(&lists, "compile");
    assert!(run.len() >= 60, "run 清单只有 {} 条（应当 ~71）", run.len());
    assert!(comp.len() >= 30, "compile 清单只有 {} 条", comp.len());
    let a: BTreeSet<&String> = run.iter().collect();
    let b: BTreeSet<&String> = comp.iter().collect();
    assert!(a.is_disjoint(&b), "run 与 compile 清单有交集");
    for f in run.iter().chain(comp.iter()) {
        assert!(corpus().join(f).is_file(), "清单里的 {f} 不存在");
    }
}

// ════════════════════════════════════════════════════════════════════
// 探针装置：把一段源码落到 `target/` 下再走命令行
// ════════════════════════════════════════════════════════════════════

/// 把 `src` 写成 `target/m12-checkface-src/<name>.go`，返回路径。
///
/// 落盘而不走 stdin：`check` 的入参就是文件（`C1` §2.2 的位置渲染也按它），
/// 走一条判据里不存在的通道会验不到真东西。
fn probe(name: &str, src: &str) -> PathBuf {
    let dir = repo_root().join("target/m12-checkface-src");
    fs::create_dir_all(&dir).expect("建不了探针目录");
    let p = dir.join(format!("{name}.go"));
    fs::write(&p, src).expect("写不了探针");
    p
}

/// 跑 `check`，要求**退出 0**；失败时把诊断原文带出来。
fn assert_check_ok(name: &str, src: &str) {
    let p = probe(name, src);
    let out = Command::new(env!("CARGO_BIN_EXE_rgoc-driver"))
        .arg("check")
        .arg(&p)
        .output()
        .expect("起不了 check");
    assert_eq!(
        out.status.code(),
        Some(0),
        "`{name}` 应当通过静态检查，实际退出 {:?}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// 跑 `check`，要求**退出 1** 且诊断码里含 `want`。
fn assert_check_code(name: &str, src: &str, want: &str) {
    let p = probe(name, src);
    let out = Command::new(env!("CARGO_BIN_EXE_rgoc-driver"))
        .arg("check")
        .arg(&p)
        .output()
        .expect("起不了 check");
    let mut all = String::from_utf8_lossy(&out.stdout).into_owned();
    all.push_str(&String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        out.status.code(),
        Some(1),
        "`{name}` 应当被拒（退出 1），实际 {:?}\n{all}",
        out.status.code()
    );
    assert!(
        codes_of(&all).iter().any(|c| c == want),
        "`{name}` 的诊断码应当含 {want}，实际 {:?}\n{all}",
        codes_of(&all)
    );
}

// ════════════════════════════════════════════════════════════════════
// T-GEN-06 显式实例化
// ════════════════════════════════════════════════════════════════════

/// 六个形态：`Id[int](7)` / `Box[int]{v:3}` + 选择器 / 双形参 / 嵌套
/// `Wrap[Wrap[int]]` / 实例**当值传** / 带容器的实例。
///
/// # 为什么这一条值 47 个文件
///
/// `M12-baseline.md` §3.1：门禁族 64 个 check 失败里 **47 个**的**第一个**诊断
/// 都是 `CHK-INVALID-INDEX`（全语料 121 处 `cannot index value of type func`）
/// —— 也就是「`F[int]` 被当成索引表达式」。修好这一处，那 47 个文件才有机会
/// 露出它们**真正的**下一个错。
#[test]
fn t_gen_06_显式实例化() {
    assert_check_ok(
        "gen06a",
        r#"
package p

func Id[T any](x T) T { return x }

func use() { var _ int = Id[int](7) }
"#,
    );
    assert_check_ok(
        "gen06b",
        r#"
package p

type Box[T any] struct{ v T }

func use() {
	b := Box[int]{v: 3}
	var _ int = b.v
}
"#,
    );
    assert_check_ok(
        "gen06c",
        r#"
package p

type Pair[K any, V any] struct {
	k K
	v V
}

func use() { _ = Pair[string, int]{k: "a", v: 1} }
"#,
    );
    assert_check_ok(
        "gen06d",
        r#"
package p

type Wrap[T any] struct{ x T }

func use() { _ = Wrap[Wrap[int]]{x: Wrap[int]{x: 1}} }
"#,
    );
    assert_check_ok(
        "gen06e",
        r#"
package p

func Id[T any](x T) T { return x }

func use() {
	var g func(int) int = Id[int]
	_ = g
}
"#,
    );
    assert_check_ok(
        "gen06f",
        r#"
package p

type M[K comparable, V any] struct{ m map[K]V }

func use() {
	x := M[string, int]{m: map[string]int{"a": 1}}
	var _ int = x.m["a"]
}
"#,
    );
}

/// 反向：**类型实参写成值** / **实参个数不对** 必须被拒。
///
/// 这条挡的是「把所有 `x[...]` 都当实例化放过去」—— 那会让真正的错静默通过，
/// 而 `T-GEN-03` 的拒绝面正是靠这类判据有牙。
#[test]
fn t_gen_06_负例() {
    assert_check_code(
        "gen06n1",
        r#"
package p

func Id[T any](x T) T { return x }

func use() { _ = Id[7](7) }
"#,
        "CHK-NOT-A-TYPE",
    );
    assert_check_code(
        "gen06n2",
        r#"
package p

func Id[T any](x T) T { return x }

func use() { _ = Id[int, string](7) }
"#,
        "TYP-WRONG-TYPE-ARG-COUNT",
    );
}

// ════════════════════════════════════════════════════════════════════
// T-GEN-07 实例化后的可赋值
// ════════════════════════════════════════════════════════════════════

/// 五种「形参里带类型参数」的实参形态。
///
/// # 为什么这一族今天会红（`M12-baseline.md` §3.1）
///
/// 检查面对**类型参数的可赋值性**一直是「能力边界 ⇒ 不判」（
/// `assignable` 返回 `Err` 时按通过处理），于是：
///
/// * `[]int` vs `[]T` ⇒ **报** `cannot use value of type []int as []type parameter`；
/// * `func(int) int` vs `func(T) T` ⇒ **报** `cannot use value of type func as func`。
///
/// 两边的形状根本不同：前者是「`assignable` 判出了假」，后者是「两边都印成
/// `func` 但签名不同」。共同点是**没人把形参按推断结果替换过** —— 拿 `[]T`
/// 去和 `[]int` 比，永远不可能相等。
///
/// 所以这一条要的不是「放宽判定」，而是**先替换、再判定**：推断出 `T = int`
/// 之后，形参就是 `[]int`，判定是**精确**的（该拒的仍然拒）。
#[test]
fn t_gen_07_实例化后可变赋值() {
    // ① 切片元素
    assert_check_ok(
        "gen07a",
        r#"
package p

func Len[T any](xs []T) int { return len(xs) }

func use() {
	var x int
	x = Len([]int{1, 2})
	_ = x
}
"#,
    );
    // ② 函数实参（`func(int) int` 对上 `func(T) T`）
    assert_check_ok(
        "gen07b",
        r#"
package p

func Apply[T any](f func(T) T, x T) T { return f(x) }

func use() {
	var x int
	x = Apply(func(v int) int { return v }, 1)
	_ = x
}
"#,
    );
    // ③ 具名泛型类型作形参
    assert_check_ok(
        "gen07c",
        r#"
package p

type Box[T any] struct{ v T }

func Get[T any](b Box[T]) T { return b.v }

func use() {
	var x int
	x = Get(Box[int]{v: 1})
	_ = x
}
"#,
    );
    // ④ map 的键与值
    assert_check_ok(
        "gen07d",
        r#"
package p

func Keys[K comparable, V any](m map[K]V) int { return len(m) }

func use() {
	var x int
	x = Keys(map[string]int{"a": 1})
	_ = x
}
"#,
    );
    // ⑤ 类型参数切片上的索引（元素类型的来源是形参 `[]T`）
    assert_check_ok(
        "gen07e",
        r#"
package p

func At[T any](xs []T, i int) T { return xs[i] }

func use() {
	var x int
	x = At([]int{1, 2}, 0)
	_ = x
}
"#,
    );
}

/// 变参被调者**不能把进程打崩**（`M12/Task 7` 引入的活儿）。
///
/// `instantiate_callee` 一上线，**每个**泛型调用都会进推断面，于是变参调用
/// （形参 2、实参 3）撞上 `infer_for_call` 的「等长」断言 —— 那是 `assert!`，
/// 不是诊断：`test/typeparam/slices.go` 实测 **rc=101**，而它本该只是
/// 「变参面没做」的普通能力边界。
///
/// 判据只要求**不崩且不误报**（rc=0）：变参的逐参可赋值性本来就是
/// `call_value` 的窄面（不判），这里不替它加戏。
#[test]
fn t_gen_07_变参被调者不炸() {
    assert_check_ok(
        "gen07v",
        r#"
package p

func Append[T any](s []T, vs ...T) []T { return s }

func use() { Append([]int{1}, 2, 3) }
"#,
    );
}

/// 替换之后判定必须是**精确**的：形状对不上仍然要拒。
///
/// 这条挡的是「为了让 ①–⑤ 过，把类型参数面的判定整个放宽」—— 那样
/// `Len(42)` 会静默通过，而它显然是错代码。
#[test]
fn t_gen_07_替换后仍然精确() {
    assert_check_code(
        "gen07n1",
        r#"
package p

func Len[T any](xs []T) int { return len(xs) }

func use() { var _ int = Len(42) }
"#,
        "CHK-INVALID-ASSIGN",
    );
    // `gen07n2` 的牙**只在返回类型面**，所以形参特意选**裸 `T`**：
    //
    // * 不替换时：实参 `string` vs 形参 `T` ⇒ `assignable` 给 `Err`（能力边界）
    //   ⇒ **不报**；返回 `T` 赋给 `int` 同样 `Err` ⇒ 不报 ⇒ 退出 0 ⇒ 这条测试红；
    // * 替换后：`T = string`，实参面精确通过，返回 `string` 赋给 `int` ⇒ 报。
    //
    // 反过来若形参写成 `[]T`，**旧路径自己就会报**（`[]string` vs `[]T` 判假），
    // 那条负例就成了「不管替换有没有生效都绿」的空断言 —— 实测过：
    // 注入「不替换」变异时它照样绿。
    assert_check_code(
        "gen07n2",
        r#"
package p

func Id[T any](v T) T { return v }

func use() {
	var x int
	x = Id("a")
	_ = x
}
"#,
        "CHK-INVALID-ASSIGN",
    );
}

// ════════════════════════════════════════════════════════════════════
// Task 8 的局部判据：类型参数上的二元运算（core type 面）
// ════════════════════════════════════════════════════════════════════
//
// ⚠️ 名字里的 `gen_08` **不是** `M12-tests.md` 的 `T-GEN-08a/b`（那条讲「同一份
// 源码 `check` 与 `build` 同口径」，由 `t_gen_08a_*` 承担）。这里是 `M12-plan.md`
// Task 8 自己起的名字，撞号了 —— 记在台账里，收口时若改名只影响这一条。

/// 类型参数上的 `+` / `<`：按**类型集的每个 term** 判，不是按 core type。
///
/// # 为什么先看官方源码再写
///
/// `M12-plan.md` Task 8 写的是「按类型集取 core type」，但官方
/// `go/types/predicates.go:62` 的 `allBasic` 干的是别的事：
///
/// ```go
/// func allBasic(t Type, info BasicInfo) bool {
///     if tpar, _ := Unalias(t).(*TypeParam); tpar != nil {
///         return tpar.is(func(t *term) bool { return t != nil && isBasic(t.typ, info) })
///     }
///     return isBasic(t, info)
/// }
/// ```
///
/// 而 `_TypeSet.is(f)` 是「**所有** term 都满足 f」且 `!hasTerms() ⇒ false`。
/// 所以 `~int | ~float64` **有** `+`（两个 term 都是 numeric）——
/// 真按 core type 判反而会拒（两个 term 的 core type 不同 ⇒ 官方的
/// `coreType` 返回 nil），那是错的。
#[test]
fn t_gen_08_core_type() {
    // ① 混合数值集上的 `+`：两个 term 都是 numeric ⇒ 通过。
    assert_check_ok(
        "gen08a",
        r#"
package p

func Add[T ~int | ~float64](a, b T) T { return a + b }

func use() { _ = Add(1, 2) }
"#,
    );
    // ② 单个 term 上的 `<`。
    assert_check_ok(
        "gen08b",
        r#"
package p

func Less[T ~int](a, b T) bool { return a < b }

func use() { _ = Less(1, 2) }
"#,
    );
    // ③ `int | string` 上的 `<`：两个 term 都 ordered ⇒ 通过（官方的
    //    `allOrdered` = 整数 ∪ 浮点 ∪ 字符串，字符串是 ordered）。
    assert_check_ok(
        "gen08c",
        r#"
package p

func Min[T int | string](a, b T) T {
	if a < b {
		return a
	}
	return b
}

func use() { _ = Min(1, 2) }
"#,
    );
    // ④ 类型集里有一个 term 不支持 `+` ⇒ 拒。这是「别把判定整个放宽」的牙：
    //    `~bool` 不是 numeric，`allBasic(IsNumeric)` 必须给假。
    assert_check_code(
        "gen08n",
        r#"
package p

func Add[T ~int | ~bool](a, b T) T { return a + b }

func use() { _ = Add(1, 2) }
"#,
        "CHK-INVALID-OPERATION",
    );
}

// ════════════════════════════════════════════════════════════════════
// Task 9 的局部判据：类型参数上的内建
// ════════════════════════════════════════════════════════════════════

/// 六个内建在类型参数上必须认得出「形状」。
///
/// # 官方是两套谓词，不是一套
///
/// * `close` / `delete` / `len` / `cap` / `append` 用官方 `underIs`
///   （`go/types/under.go:14`）= 「类型集里**每个 term 的底层类型**都满足」；
/// * `make` 用官方 `commonUnder`（`under.go:79`）= 「所有 term 的**共同**底层」，
///   因为它要拿具体形状定参数个数（切片 2..3、map/chan 1..2）。
///
/// 两者对 `~[]E` 这种单 term 集合同判，但对 `map[string]int | map[string]float64`
/// 这类集合不同：`delete` 只需「都是 map」（`underIs`），`make` 要「同一个 map」。
#[test]
fn t_gen_09_内建() {
    // ① `len` / `cap` 在切片类型参数上。
    assert_check_ok(
        "gen09a",
        r#"
package p

func Len[T any, S ~[]T](s S) int { return len(s) + cap(s) }

func use() { _ = Len([]int{1}) }
"#,
    );
    // ② `append`（含 `t...` 展开）。
    assert_check_ok(
        "gen09b",
        r#"
package p

func Add[S ~[]T, T any](s S, vs ...T) S { return append(s, vs...) }

func use() { _ = Add([]int{1}, 2) }
"#,
    );
    // ③ `close` 在通道类型参数上。
    assert_check_ok(
        "gen09c",
        r#"
package p

func Shut[T ~chan int](ch T) { close(ch) }

func use() { Shut(make(chan int)) }
"#,
    );
    // ④ `delete`：类型集里多个 map，**key 都相同** ⇒ 通过。
    assert_check_ok(
        "gen09d",
        r#"
package p

type MV interface{ map[string]int | map[string]float64 }

func Del[T MV](m T) { delete(m, "k") }

func use() { Del(map[string]int{}) }
"#,
    );
    // ⑤ `make`：要的是**共同底层**（`~[]T` 只有一个 term）。
    assert_check_ok(
        "gen09e",
        r#"
package p

func New[S ~[]T, T any]() S { return make(S, 0, 8) }

func use() { _ = New[[]int, int]() }
"#,
    );
    // ⑥ 扩展元素类型与 `~[]E` 的索引（`append.go` 的形态）。
    assert_check_ok(
        "gen09f",
        r#"
package p

type sliceOf[E any] interface{ ~[]E }

func At[S sliceOf[T], T any](s S, i int) T { return s[i] }

func use() { _ = At([]int{1}, 0) }
"#,
    );
    // ⑦ 负例：类型集的 term 不是切片 ⇒ `len` 要拒（别把判定放宽）。
    assert_check_code(
        "gen09n1",
        r#"
package p

func Bad[T ~int](x T) int { return len(x) }

func use() { _ = Bad(1) }
"#,
        "CHK-INVALID-BUILTIN-ARG",
    );
    // ⑧ 负例：类型集里**有一个** term 不是切片 ⇒ 拒。这条钉的是
    // `under_is` 的 `all` 语义（换成 `any` 它就绿了 —— 真验过）。
    assert_check_code(
        "gen09n3",
        r#"
package p

func Bad[T ~int | ~[]byte](x T) int { return len(x) }

func use() { _ = Bad(1) }
"#,
        "CHK-INVALID-BUILTIN-ARG",
    );
    // ⑨ 负例：`close` 的类型集里混了非通道 ⇒ 拒（同一条 `all` 语义）。
    assert_check_code(
        "gen09n4",
        r#"
package p

func Bad[T ~chan int | ~int](ch T) { close(ch) }

func use() { Bad(make(chan int)) }
"#,
        "CHK-INVALID-OPERATION",
    );
    // ⑩ 负例：`make` 的类型集有两个**不同**的 map ⇒ 没有共同底层 ⇒ 拒。
    assert_check_code(
        "gen09n2",
        r#"
package p

type MV interface{ map[string]int | map[string]float64 }

func Bad[T MV]() T { return make(T) }

func use() { _ = Bad[map[string]int]() }
"#,
        "CHK-INVALID-BUILTIN-ARG",
    );
}

// ════════════════════════════════════════════════════════════════════
// T-GEN-10 两个边角（`_` 与 `range`）
// ════════════════════════════════════════════════════════════════════

/// 两个**边角位置**，各自只值两三个文件，但都是「把合法代码误判成错」。
///
/// # ① 接收者的类型实参位置：`_` 合法
///
/// `func (b Box[_]) M()` 里的 `_` **不是引用**。官方 `collectRecv`
/// （`signature.go:216`）先用 `unpackRecv` 把 `Box[_]` 拆成
/// 「基 `Box` + 形参表 `[_]`」，然后 `declareTypeParam`（`decl.go:662`）
/// 把 `_` 当**类型参数声明**——`recvType = instance(baseType, recvTParams)`
/// 用的是刚声明的类型参数**自己**，形参表那棵表达式根本没过 `typ()`。
///
/// 我们这两条路都会把它当引用：`rgoc-load` 的 `use_name`（`scope.rs:723`）
/// 与 `rgoc-check` 的 `resolve_type(Name "_")`（`typeexpr.rs:84`）。
/// 本仓 `recv_tparam_fields`（`decls.rs:621`）已经按官方口径把实参当声明收了，
/// 但 `resolve_func_sig` 紧接着又把**整棵** `Box[_]` 交给 `resolve_type` ⇒ 撞上。
///
/// 语料：`issue47710.go` / `issue53406.go` / `smoketest.go` / `list2.go`（4 个文件）。
///
/// # ② `range` 一个类型参数：按**共同底层**判
///
/// 官方 `rangeKeyVal`（`range.go:212`）第一句就是
/// `commonUnder(orig, …)`，不是 `under(orig)` —— `under(TypeParam)` 在官方
/// 返回**类型参数自己**（`*TypeParam.Underlying()` 就是它自己），直接
/// `switch` 会落到 default ⇒ 误报 `cannot range over`。
///
/// 语料：`issue48453.go`（`CopyMap[M ~map[K]V]` 里 `for k, v := range m`）。
///
/// # 两条都要**留牙**
///
/// 放开的只是「声明位置」与「类型参数」两个面：
/// `var x _` / `func f(x _) {}` 仍然要 `TYP-INVALID-BLANK`；
/// 类型集**没有共同底层**（`~int | ~string`）或**只写通道**的 range 仍然要拒。
#[test]
fn t_gen_10_边角() {
    // ① 接收者的类型实参位置（含指针、多形参、部分 `_`）。
    assert_check_ok(
        "gen10a",
        r#"
package p

type Box[T any] int

func (b Box[T]) A() {}
func (b Box[_]) B() {}
func (b *Box[_]) C() {}

type Pair[A, B any] int

func (p Pair[A, B]) X() {}
func (p Pair[_, B]) Y() {}
func (p Pair[_, _]) Z() {}
"#,
    );
    // ② `_` 作为**类型参数名**与**类型名**声明（`smoketest.go:28` / `issue53406.go:20`）。
    assert_check_ok(
        "gen10b",
        r#"
package p

type _ int
type _ []string

type Inner[_ any] int

func (Inner[_]) M() {}

func (i Inner[int]) N() {}
"#,
    );
    // ③ 负例：`_` 当**类型引用**仍然拒（别把牙拔了）。
    assert_check_code(
        "gen10n1",
        r#"
package p

var x _
"#,
        "TYP-INVALID-BLANK",
    );
    // ④ 负例：`_` 当**形参类型**仍然拒。
    assert_check_code(
        "gen10n2",
        r#"
package p

func f(x _) {}
"#,
        "TYP-INVALID-BLANK",
    );
    // ⑤ `range` 类型参数：map / slice / chan / 数组（`issue48453.go` 的形态）。
    assert_check_ok(
        "gen10c",
        r#"
package p

func R1[M ~map[K]V, K comparable, V any](m M) int {
	n := 0
	for k, v := range m {
		_, _ = k, v
		n++
	}
	return n
}

func R2[S ~[]E, E any](s S) int {
	n := 0
	for i, v := range s {
		_, _ = i, v
		n++
	}
	return n
}

func R3[C ~chan int](ch C) int {
	n := 0
	for v := range ch {
		_ = v
		n++
	}
	return n
}

func R4[A ~[4]E, E any](a A) int {
	n := 0
	for i := range a {
		_ = i
		n++
	}
	return n
}

func use() {
	_ = R1(map[string]int{})
	_ = R2([]int{1})
	_ = R3(make(chan int))
	_ = R4([4]int{})
}
"#,
    );
    // ⑥ 负例：类型集**没有共同底层**（`~int | ~string`）⇒ 拒。
    // 这条钉的是 `common_under` 的「共同」语义：换成「取第一个 term」它就绿了。
    assert_check_code(
        "gen10n3",
        r#"
package p

func Bad[T ~int | ~string](x T) int {
	n := 0
	for range x {
		n++
	}
	return n
}

func use() { _ = Bad(1) }
"#,
        "CHK-INVALID-RANGE-EXPR",
    );
    // ⑦ 负例：只写通道的类型参数仍然拒（`commonUnder` 之后 SendOnly 判定还得在）。
    assert_check_code(
        "gen10n4",
        r#"
package p

func Bad[C ~chan<- int](ch C) int {
	n := 0
	for range ch {
		n++
	}
	return n
}

func use() { _ = Bad(make(chan<- int)) }
"#,
        "CHK-INVALID-RANGE-EXPR",
    );
}
