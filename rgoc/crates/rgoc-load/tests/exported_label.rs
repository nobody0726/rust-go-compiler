//! 导出标识符（`SCP-06`）/ 标签（`T-TY-57`）/ 空标识符（`T-TY-58`）—— M3/Task 26。
//!
//! 判据来源：`docs/milestones/M3-plan.md` Task 26 + `M3-tests.md` §3.5，
//! 位置与消息逐条对齐 `go/types` 1.27.1 的**实测输出**
//! （`.superpowers/sdd/M3-plan/probe/m3-26/testdata/s1..s9.go`，本文件的
//! `S1..S9` 常量就是那九个文件**原文**）。
//!
//! # 官方实测（`go/types`，位置逐行照抄）
//!
//! ```text
//! s1: 4:7  label L not declared
//! s2: 5:1  label L already declared        (+ 4:1 other declaration of L —— 附加行，不产出)
//! s3: 6:9  invalid break label L
//!     4:1  label L declared and not used   ← **两条**：非法的 break 不算「用过」
//! s4: 4:1  label L declared and not used
//! s5: （无）
//! s6: 7:9  label L not declared            ← 函数字面量里看不见外层的 L
//! s7: 4:7  goto L jumps into block
//! s8: 1:9  invalid package name _
//! s9: 8:1  label L already declared        (+ 7:1 other declaration of L)
//! ```
//!
//! # 为什么断言时要**先滤掉 `SYN-*`**
//!
//! 标签有**两套**官方检查：语法器 `checkBranches`（M2 镜像，`SYN-LABEL-*`）与
//! 类型检查器 `labels.go`（本任务，`TYP-*`）。我们的流水线两层都跑（官方 `gc`
//! 在语法层报完就不再类型检查），所以同一个 fixture 可能同时出现两条 —— 那是
//! **层次镜像**，不是重复实现。本文件只断言 `TYP-*` 这一侧；`SYN-*` 那一侧由
//! `rgoc-parse` 自己的用例钉（详见 `src/labels.rs` 的模块文档）。
//!
//! # 三条纪律（与 `scopes.rs` / `decl_collect.rs` 同）
//!
//! 1. **位置断言先证两个候选不相等**（`break L` 的标签位 vs `L:` 的声明位）；
//! 2. **条数用精确值**（`s3` 是**两条**，不是一条）；
//! 3. **每条正向都有反向**（合法的标签程序必须**零**诊断；`Ⅰ` 不导出要有反证）。

use rgoc_lex::{Diag, DiagCode};
use rgoc_load::{Package, is_exported, load_sources};
use rgoc_types::TypeTable;

// ── 夹具是怎么来的 ────────────────────────────────────────────────────────────

/// `testdata/s1.go`（原文，含 tab）
const S1: &str = "package p\n\nfunc f() {\n\tgoto L\n}\n";
/// `testdata/s2.go`
const S2: &str = "package p\n\nfunc f() {\nL:\nL:\n\tfor {\n\t\tbreak L\n\t}\n}\n";
/// `testdata/s3.go`
const S3: &str = "package p\n\nfunc f() {\nL:\n\t{\n\t\tbreak L\n\t}\n}\n";
/// `testdata/s4.go`
const S4: &str = "package p\n\nfunc f() {\nL:\n\tfor {\n\t\tbreak\n\t}\n}\n";
/// `testdata/s5.go` —— **合法**：前向 `goto`、`continue L1`、`break L3`、case 里的标签
const S5: &str = "package p\n\nfunc f(n int) {\nL1:\n\tfor i := 0; i < n; i++ {\n\t\tif i > 2 {\n\t\t\tbreak L1\n\t\t}\n\t\tgoto L2\n\tL2:\n\t\tcontinue L1\n\t}\n\tswitch n {\n\tcase 1:\n\tL3:\n\t\tfor {\n\t\t\tbreak L3\n\t\t}\n\t}\n}\n";
/// `testdata/s6.go` —— 函数字面量里的 `goto L`
const S6: &str = "package p\n\nfunc f(n int) {\nL:\n\tfor i := 0; i < n; i++ {\n\t\tfunc() {\n\t\t\tgoto L\n\t\t}()\n\t\tbreak L\n\t}\n}\n";
/// `testdata/s7.go`
const S7: &str = "package p\n\nfunc f() {\n\tgoto L\n\t{\n\tL:\n\t}\n}\n";
/// `testdata/s8.go`
const S8: &str = "package _\n\nfunc f() {}\n";
/// `testdata/s9.go`
const S9: &str = "package p\n\nfunc f(n int) {\n\tfor i := 0; i < n; i++ {\n\t\tcontinue\n\t}\nL:\nL:\n\tfor {\n\t\tbreak L\n\t}\n}\n";
/// `testdata/s21.go` —— **向后** `goto`（同一块内，标签已声明）
const S21: &str =
    "package p\n\nfunc f(n int) {\n\ti := 0\nL:\n\tif i < n {\n\t\ti++\n\t\tgoto L\n\t}\n}\n";
/// `testdata/s22.go` —— **向后** `goto` 跳到**外层**块的标签（合法：只有「跳进块」非法）
const S22: &str = "package p\n\nfunc f(n int) {\nL:\n\tfor i := 0; i < n; i++ {\n\t\tif i > 2 {\n\t\t\tgoto L\n\t\t}\n\t}\n}\n";

// ── 基础设施 ─────────────────────────────────────────────────────────────────

fn src(pairs: &[(&str, &str)]) -> Vec<(String, Vec<u8>)> {
    pairs
        .iter()
        .map(|(n, s)| ((*n).to_string(), s.as_bytes().to_vec()))
        .collect()
}

fn borrowed(srcs: &[(String, Vec<u8>)]) -> Vec<(&str, &[u8])> {
    srcs.iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect()
}

fn load(files: &[(&str, &str)]) -> (Package, Vec<Diag>) {
    let mut tt = TypeTable::new();
    let srcs = src(files);
    let (pkg, bag) = load_sources(&mut tt, &borrowed(&srcs), None);
    (pkg, bag.into_sorted())
}

/// `1` 起的列号（**字节**单位，与 `Pos::col` 同）。
fn col_of(source: &str, line: u32, needle: &str) -> u32 {
    let l = source
        .lines()
        .nth((line - 1) as usize)
        .unwrap_or_else(|| panic!("fixture 没有第 {line} 行"));
    let at = l
        .find(needle)
        .unwrap_or_else(|| panic!("第 {line} 行里没有 {needle:?}：{l:?}"));
    (at as u32) + 1
}

/// 只留 `TYP-*` 那一层（`SYN-*` 归 `rgoc-parse` 的用例，见模块文档）。
fn typed(ds: &[Diag]) -> Vec<(DiagCode, u32, u32, &str)> {
    ds.iter()
        .filter(|d| d.code.as_str().starts_with("TYP-"))
        .map(|d| (d.code, d.pos.line(), d.pos.col(), d.message.as_str()))
        .collect()
}

/// 断言「只有这些 `TYP-*`」，并返回它们（顺便让 `SYN-*` 的存在**可见**）。
#[track_caller]
fn assert_typed(files: &[(&str, &str)], want: &[(DiagCode, &str, u32, &str, &str)]) {
    let (_pkg, ds) = load(files);
    let got = typed(&ds);
    let want: Vec<(DiagCode, u32, u32, &str)> = want
        .iter()
        .map(|(c, src, line, needle, msg)| (*c, *line, col_of(src, *line, needle), *msg))
        .collect();
    assert_eq!(got, want, "TYP-* 诊断集合对不上（全部诊断：{ds:#?}）");
}

// ── `SCP-06` 导出标识符（`T-TY-56`，9 缺口之一）──────────────────────────────

#[test]
fn scp_06_ascii_只看首字母() {
    assert!(is_exported("Foo"));
    assert!(is_exported("F"));
    assert!(!is_exported("foo"));
    assert!(!is_exported("f"));
    // 首字母之后的大小写**不影响**（官方只看 `DecodeRuneInString` 的第一个码点）
    assert!(is_exported("Fooo"));
    assert!(!is_exported("fOO"));
    assert!(!is_exported("xÉ"));
}

#[test]
fn scp_06_unicode_大小写字母() {
    // 官方样本里的非 ASCII：都是 Unicode 类别 `Lu`
    for n in ["É", "École", "Ł", "Ω", "Å", "Ξ", "Ǆ"] {
        assert!(is_exported(n), "{n} 应当导出");
    }
    for n in ["é", "école", "ł", "ω", "å", "ξ", "ǆ"] {
        assert!(!is_exported(n), "{n} 不该导出");
    }
    // `Lo`（汉字/假名）没有大小写之分 ⇒ 不导出
    for n in ["中", "中x", "あ", "한"] {
        assert!(!is_exported(n), "{n} 不该导出");
    }
}

/// **拒绝 Rust std `char::is_uppercase` 会多收的那 120 个码点**。
///
/// 这是本节的核心断言：std 的是 derived `Uppercase`（`Lu` ∪ `Other_Uppercase`），
/// 多出 5 段共 120 个码点。若这里用了 std 的谓词，导出面就会把 `Ⅰx` 这种
/// **不导出**的名字判成导出。
#[test]
fn scp_06_拒绝_rust_std_会多收的那批() {
    const EXTRAS: [(char, &str); 5] = [
        ('\u{2160}', "罗马数字 Ⅰ（Nl）"),
        ('\u{24B6}', "带圈 Ⓐ（So）"),
        ('\u{1F130}', "带框 🄰（So）"),
        ('\u{1F150}', "反白 🅐（So）"),
        ('\u{1F170}', "方块 🅰（So）"),
    ];
    for (c, what) in EXTRAS {
        let name = format!("{c}x");
        assert!(!is_exported(&name), "{what} 不是 Lu ⇒ 不导出");
        assert!(
            c.is_uppercase(),
            "反证失败：Rust std 也不认为 {what} 是大写，本断言失去意义"
        );
    }
}

#[test]
fn scp_06_空标识符与空串都不导出() {
    assert!(!is_exported(""));
    assert!(!is_exported("_"));
    assert!(!is_exported("_x"));
    assert!(!is_exported("_É")); // 首字符是 `_` ⇒ 不导出（后面的 É 不算）
}

// ── `T-TY-57` 标签 ───────────────────────────────────────────────────────────

/// `s5` 是**合法**程序：前向 `goto L2`、`continue L1`、`break L3`、case 里的标签、
/// 三种标签都真的被用到 ⇒ **零**诊断（含 `SYN-*`：语法层同样认为它干净）。
#[test]
fn t_ty_57_合法标签程序零诊断() {
    let (_pkg, ds) = load(&[("s5.go", S5)]);
    assert!(
        ds.is_empty(),
        "合法的标签程序不该有任何诊断，实际：{:?}",
        ds.iter()
            .map(|d| (d.code, d.pos.line(), d.pos.col(), &d.message))
            .collect::<Vec<_>>()
    );
}

/// `s1`：`goto L` 在整个函数体里找不到 `L` ⇒ `UndeclaredLabel`
/// （消息是 “not **declared**”，不是 “undefined”）。
#[test]
fn t_ty_57_未声明的标签() {
    assert_typed(
        &[("s1.go", S1)],
        &[(
            DiagCode::TypUndeclaredLabel,
            S1,
            4,
            "L",
            "label L not declared",
        )],
    );
}

/// `s21` / `s22`：**向后** `goto`（标签在跳转**之前**就已声明）必须**立刻**解析成功 ——
/// 它既不进前向跳转表，也不可能在收尾被判成「跳进块」。
///
/// ⚠️ 这两条是变异 `M12` 抓出来的**缺口**：初版 23 条用例里只有**前向** `goto`
/// （`s5` 的 `goto L2`），于是「`goto` 一律当前向跳转」这个实现错误**无人可抓**。
/// 官方对这两个夹具零诊断（实测）。
#[test]
fn t_ty_57_向后goto立即解析() {
    // ① 同一块内向后跳
    let (_pkg, ds) = load(&[("s21.go", S21)]);
    assert!(ds.is_empty(), "合法的向后 `goto` 不该有诊断：{ds:#?}");
    // ② 跳到**外层**块（跳出块合法，跳进块才非法）
    let (_pkg, ds) = load(&[("s22.go", S22)]);
    assert!(ds.is_empty(), "合法的向后 `goto` 不该有诊断：{ds:#?}");
}

/// `s7`：`L` **存在**，但在嵌套块里 ⇒ 前向跳转收尾时判成 `JumpIntoBlock`；
/// 而且该标签被标记「已用」⇒ **不**再报 `UnusedLabel`（官方同一行为）。
#[test]
fn t_ty_57_跳进块_且不再报未用() {
    assert_typed(
        &[("s7.go", S7)],
        &[(
            DiagCode::TypJumpIntoBlock,
            S7,
            4,
            "L",
            "goto L jumps into block",
        )],
    );
}

/// `s2`：同一个函数体里声明两次 `L` ⇒ `DuplicateLabel`，位置取**后**声明处
/// （`5:1`，不是 `4:1`）。
///
/// ⚠️ 先证两个候选位置**不相等** —— 否则「报在第二个」与「报在第一个」分不出来。
#[test]
fn t_ty_57_重复标签报在后声明处() {
    // ⚠️ `S2` 里两个 `L:` **同列**（都在第 1 列）⇒「报在后声明处」只能靠**行号**分辨。
    // 先把「同列」这个前提钉死：哪天有人调了 `S2` 的缩进，这条会先红，
    // 提示下面那条「行 5」的断言已经失去判别力。
    assert_eq!(
        col_of(S2, 4, "L:"),
        col_of(S2, 5, "L:"),
        "本用例靠**行号**分辨先后，前提是两个 `L:` 同列"
    );

    let (_pkg, ds) = load(&[("s2.go", S2)]);
    // 两个声明都在第 1 列 ⇒ 只靠列分不出先后来，所以这里换成**行**断言
    assert_eq!(
        typed(&ds),
        vec![(
            DiagCode::TypDuplicateLabel,
            5,
            1,
            "label L already declared"
        )],
        "重复标签只报一条、报在**后**声明处（官方另有一条 `other declaration of L` 附加行，不产出）"
    );
}

/// `s9`：重复标签出现在**更靠后**的位置（`8:1`）；第二个仍然只报一条，
/// 且 `break L` 让 `L` 算「已用」⇒ 不报未用。
#[test]
fn t_ty_57_重复标签_用了就不额外报未用() {
    assert_typed(
        &[("s9.go", S9)],
        &[(
            DiagCode::TypDuplicateLabel,
            S9,
            8,
            "L:",
            "label L already declared",
        )],
    );
}

/// `s4`：`L:` 声明了但没有任何分支引用它 ⇒ `UnusedLabel`（官方 `soft` 错）。
#[test]
fn t_ty_57_未使用标签() {
    assert_typed(
        &[("s4.go", S4)],
        &[(
            DiagCode::TypUnusedLabel,
            S4,
            4,
            "L:",
            "label L declared and not used",
        )],
    );
}

/// `s3`：`L:` 标的是**块**，`break L` 非法 ⇒ **两条**：`MisplacedLabel`（在
/// `break` 的标签位）+ `UnusedLabel`（在声明位）—— 因为非法的 `break` **不算用过**
/// （官方在那条分支上 `return`，不置 `used`）。
///
/// ⚠️ 这一条同时是「位置先证不等」的用例：两条诊断的位置必须不同。
#[test]
fn t_ty_57_break标签位置非法() {
    let decl = col_of(S3, 4, "L:");
    let brk = col_of(S3, 6, "L");
    assert_ne!(decl, brk, "声明位与 `break L` 的标签位必须不同");

    assert_typed(
        &[("s3.go", S3)],
        &[
            (
                DiagCode::TypUnusedLabel,
                S3,
                4,
                "L:",
                "label L declared and not used",
            ),
            (
                DiagCode::TypMisplacedLabel,
                S3,
                6,
                "L",
                "invalid break label L",
            ),
        ],
    );
}

/// `continue L` 的标签必须是**外层 `for`**（`break` 还认 switch/select）
/// ⇒ 标在 `switch` 上的 `L` 对 `break` 合法、对 `continue` 非法。
///
/// 这条**不是**恒真的：它同时钉住「break/continue 的判据不同」这件事。
#[test]
fn t_ty_57_continue标签必须标在外层for() {
    let a = "package p\n\nfunc f(n int) {\nL:\n\tswitch n {\n\tcase 1:\n\t\tbreak L\n\tcase 2:\n\t\tcontinue L\n\t}\n}\n";
    assert_typed(
        &[("a.go", a)],
        &[(
            DiagCode::TypMisplacedLabel,
            a,
            9,
            // ⚠️ 位置取 `break`/`continue` 的**标签**处，不是关键字处（`break L` 的 `L`
            // 才是官方 `check.errorf(s.Label, …)` 的实参）
            "L",
            "invalid continue label L",
        )],
    );
}

/// `s6`：函数**字面量**里的 `goto L` 看不见外层函数的 `L` ⇒ `UndeclaredLabel`
/// （标签空间是**每个函数体一个**）；而外层那个 `break L` 照旧合法 ⇒
/// 外层的 `L` 算「已用」，不报未用。
#[test]
fn t_ty_57_标签跨函数不可见() {
    assert_typed(
        &[("s6.go", S6)],
        &[(
            DiagCode::TypUndeclaredLabel,
            S6,
            7,
            "L",
            "label L not declared",
        )],
    );
}

/// 标签**不在值作用域里**：`L:` 与 `L := 1` 可以同名共存、互不干扰。
///
/// 反证方式：若把标签也声明进值作用域，这里要么出 `DuplicateDecl`、
/// 要么 `break L` / `_ = L` 之一报 `undefined` —— 三种症状本用例都能抓到。
#[test]
fn t_ty_57_标签与变量同名不冲突() {
    let a = "package p\n\nfunc f() {\nL:\n\tfor {\n\t\tbreak L\n\t}\n\t{\n\t\tL := 1\n\t\t_ = L\n\t}\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert!(ds.is_empty(), "实际：{ds:#?}");
}

/// 标签的重复判定范围是**整个函数体**，不是「同一个块」：两个平级块里各一个 `L`
/// 照样是重复（官方 `all` 是体的全集）。
///
/// ⚠️ 两个 `L` 各被自己的 `break L` 用到 ⇒ 除了重复那条**不该**再有别的诊断。
/// （初版夹具写成 `L: { goto L }`，官方实测会**额外**多一条
/// `goto L jumps into block` —— 因为第二个 `L` 没能插进任何块，
/// 那个 `goto` 匹配不上、收尾时按「全集里有这个名字」落到 `JumpIntoBlock`。
/// 这里换成「标签 `for` + `break L`」，把那一支隔离掉。）
#[test]
fn t_ty_57_重复标签跨块也算() {
    let a = "package p\n\nfunc f() {\n\t{\n\tL:\n\t\tfor {\n\t\t\tbreak L\n\t\t}\n\t}\n\t{\n\tL:\n\t\tfor {\n\t\t\tbreak L\n\t\t}\n\t}\n}\n";
    assert_typed(
        &[("a.go", a)],
        &[(
            DiagCode::TypDuplicateLabel,
            a,
            11,
            "L:",
            "label L already declared",
        )],
    );
}

/// `case` 子句里的 `break L` / `continue L` 能跳到**外层**那个带标签的语句
/// （官方：case 块 `lstmt = nil`，但 `parent` 指向 switch 体、再指向 `L: for`）。
///
/// 反证方式：若把 case 子句当成独立的根块，这里会冒出 `invalid break label L`。
#[test]
fn t_ty_57_case子句里的break跳到外层标签() {
    let a = "package p\n\nfunc f(x int) {\nL:\n\tfor {\n\t\tswitch x {\n\t\tcase 1:\n\t\t\tbreak L\n\t\tdefault:\n\t\t\tcontinue L\n\t\t}\n\t}\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert!(ds.is_empty(), "实际：{ds:#?}");
}

/// `select` 的子句同理（官方对 `CommClause` 也是 `blockBranches(nil, …)`）。
#[test]
fn t_ty_57_select子句里的break跳到外层标签() {
    let a = "package p\n\nfunc f(c chan int) {\nL:\n\tfor {\n\t\tselect {\n\t\tcase <-c:\n\t\t\tbreak L\n\t\t}\n\t}\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert!(ds.is_empty(), "实际：{ds:#?}");
}

// ── `T-TY-58` 空标识符 ──────────────────────────────────────────────────────

/// `_` 声明多次**不冲突**（它从不绑定），而且它在**左值位**合法。
#[test]
fn t_ty_58_空标识符不绑定也不冲突() {
    let a = "package p\n\nvar _ int\nvar _ string\n\nfunc f(_ int, _ string) {\n\t_ = 1\n\t_ = 2\n\tfor _ = range []int{} {\n\t}\n\tfor _, _ = range []int{} {\n\t}\n}\n";
    let (pkg, ds) = load(&[("a.go", a)]);
    assert!(ds.is_empty(), "实际：{ds:#?}");
    // `lookup` 从包作用域沿链到 universe ⇒ `_` 在任何一层都不存在
    assert!(pkg.lookup("_").is_none(), "`_` 永不绑定");
}

/// `_` 当**值**用 ⇒ `InvalidBlank`（官方 `typexpr.go:28`），**不是** `undefined: _`。
#[test]
fn t_ty_58_空标识符当值报_invalid_blank() {
    let a = "package p\n\nfunc f() {\n\tx := _\n\t_ = x\n}\n";
    assert_typed(
        &[("a.go", a)],
        &[(
            DiagCode::TypInvalidBlank,
            a,
            4,
            "_",
            "cannot use _ as value or type",
        )],
    );
}

/// `_` 当**类型**用 ⇒ 同一条 `InvalidBlank`（官方那条消息就是 “value **or type**”）。
#[test]
fn t_ty_58_空标识符当类型报_invalid_blank() {
    let a = "package p\n\nvar x _\n";
    assert_typed(
        &[("a.go", a)],
        &[(
            DiagCode::TypInvalidBlank,
            a,
            3,
            "_",
            "cannot use _ as value or type",
        )],
    );
}

/// `_` 当**标签**：官方整个跳过（既不声明、也不报未用）。
#[test]
fn t_ty_58_空标识符标签被跳过() {
    let a = "package p\n\nfunc f() {\n\t_:\n\tfor {\n\t\tbreak\n\t}\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert!(ds.is_empty(), "实际：{ds:#?}");
}

/// 包名是 `_` ⇒ `BlankPkgName`，位置取**包名标识符**处（不是 `package` 关键字处）。
#[test]
fn t_ty_58_包名为空标识符() {
    assert_typed(
        &[("a.go", S8)],
        &[(
            DiagCode::TypBlankPkgName,
            S8,
            1,
            "_",
            "invalid package name _",
        )],
    );
}

/// 触发面**受 `pkg.name` 是否为空约束**（官方 `initFiles` 的 `switch`）：
/// 第一个文件定下真名之后，再出现 `package _` 走的是 `MismatchedPkgName`。
///
/// 这条是反向：同一段文字，**顺序不同**结果就不同（两种诊断各出现一次）。
#[test]
fn t_ty_58_包名空标识符只在包名尚未确定时报() {
    let blank = "package _\n\nfunc f() {}\n";
    let named = "package p\n\nfunc g() {}\n";
    // ① `_` 在前 ⇒ BlankPkgName（`p` 随后定名，不报不一致）
    assert_typed(
        &[("a.go", blank), ("b.go", named)],
        &[(
            DiagCode::TypBlankPkgName,
            blank,
            1,
            "_",
            "invalid package name _",
        )],
    );
    // ② 真名在前 ⇒ 后面的 `package _` 落进 `MismatchedPkgName`
    assert_typed(
        &[("a.go", named), ("b.go", blank)],
        &[(
            DiagCode::TypMismatchedPkgName,
            blank,
            1,
            "package",
            "package _; expected package p",
        )],
    );
}
