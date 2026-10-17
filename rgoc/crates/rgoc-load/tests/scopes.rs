//! 块作用域 / 遮蔽 / 未定义 / 重复声明（M3/Task 25）。
//!
//! 判据来源：`docs/milestones/M3-plan.md` Task 25（`T-TY-53/54/55`，
//! `SCP-02/03/04`）+ `.superpowers/sdd/M3-plan/probe/m3-24/testdata/` 的官方实测
//! （`dup_block.go` / `dup_pkg.go` / `undef.go` / `shadow.go`）。
//!
//! # 这个文件的三条纪律（与 `decl_collect.rs` 同）
//!
//! 1. **位置断言先证「两个候选不相等」**：`q := q` 的 LHS 与 RHS 是两个不同的列，
//!    不先证不等就分不出「报在声明处」还是「报在引用处」。
//! 2. **条数用精确值**：重复报告（每个引用各报一条、跨文件各报一条）是这一族
//!    最典型的错，`>= 1` 抓不到。
//! 3. **每条正向都有反向**：遮蔽/已声明/不同层同名 —— 该不报的必须不报
//!    （`M2` 抓过 6+ 条恒真断言，`M3` 的规矩是成对写）。
//!
//! # 官方实测（`go/types` 1.27.1，位置逐条对齐）
//!
//! ```text
//! ERR: dup_block.go:5:6: x redeclared in this block      ← 报在**后来**那个
//! ERR: dup_block.go:4:6:     other declaration of x      ← 附加行，M3 不产出（open_gaps）
//! ERR: dup_pkg.go:4:5:   x redeclared in this block      ← 包级也写 "block"
//! ERR: undef.go:8:7:     undefined: T                    ← 类型位置
//! ERR: undef.go:4:2:     undefined: y                    ← 值位置，**每次出现各一条**
//! ERR: undef.go:5:6:     undefined: y
//! ```

use rgoc_lex::{Diag, DiagCode};
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

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

/// 装载一组内存文件，返回（包, 已排序诊断）。
fn load(files: &[(&str, &str)]) -> (Package, Vec<Diag>) {
    let mut tt = TypeTable::new();
    let srcs = src(files);
    let (pkg, bag) = load_sources(&mut tt, &borrowed(&srcs), None);
    (pkg, bag.into_sorted())
}

/// `1` 起的列号：`line` 行里 `needle` 第一次出现的位置（**字节**，与 `Pos::col` 同单位）。
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

/// 诊断的 `(code, 行, 列, 消息)` 四元组 —— 断言里一眼看得全。
fn brief(ds: &[Diag]) -> Vec<(DiagCode, u32, u32, &str)> {
    ds.iter()
        .map(|d| (d.code, d.pos.line(), d.pos.col(), d.message.as_str()))
        .collect()
}

// ── `T-TY-53` 遮蔽 ────────────────────────────────────────────────────────

/// 内层同名**不是**重复声明：包级 `x`、函数局部 `x := 1`、块内 `var x string`
/// 三层同名各不相干 ⇒ 一条诊断都不该有。
///
/// ⚠️ 这条**不是**恒真的：把局部声明放进包作用域（或让 `if` 体复用外层块），
/// 立刻会出 `x redeclared …`。
#[test]
fn t_ty_53_遮蔽_三层同名都不报() {
    let a = "package p\n\nvar x int\n\nfunc f() {\n\tx := 1\n\tif true {\n\t\tvar x string\n\t\t_ = x\n\t}\n\t_ = x\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert!(ds.is_empty(), "遮蔽不是重复声明：{:?}", brief(&ds));
}

/// 遮蔽的**边界**：块一结束，内层名字就没了 ⇒ 外层那句 `_ = x` 必须是
/// `undefined: x`（证明内层声明**没有**漏进函数作用域）。
#[test]
fn t_ty_53_块结束后内层名字失效() {
    let a = "package p\n\nfunc f() {\n\tif true {\n\t\tvar x string\n\t\t_ = x\n\t}\n\t_ = x\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert_eq!(
        brief(&ds),
        vec![(
            DiagCode::TypUndeclaredName,
            8,
            col_of(a, 8, "x"),
            "undefined: x"
        )],
        "内层 `var x` 不该漏到函数层"
    );
}

/// **裸块** `{ … }` 自成一层（官方 `stmt.go:600` 的 `"block"`）。
///
/// 为什么单列一条：`if`/`for` 的体虽然是块，但它们的作用域是**外层那个**
/// （`walk_if`/`walk_for` 直接建），走的不是 `Stmt::Block` 那条分派 ——
/// 只测 `if` 体抓不到「裸块没开作用域」这个错（变异 M4 实测漏过）。
#[test]
fn t_ty_53_裸块自成一层() {
    let a = "package p\n\nfunc f() {\n\t{\n\t\tvar x string\n\t\t_ = x\n\t}\n\t_ = x\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert_eq!(
        brief(&ds),
        vec![(
            DiagCode::TypUndeclaredName,
            8,
            col_of(a, 8, "x"),
            "undefined: x"
        )],
        "块内的 `var x` 不该漏到函数层"
    );
}

/// `if` / `for` / `for…range` 三个头部的**作用域边界**（官方 `stmt.go:606`/`833`、
/// `range.go:88` 各开一层）：头里声明的变量出去就没了，而**头里面**用得到。
#[test]
fn t_ty_53_if_for_range_各自成层且不外漏() {
    let a = "package p\n\nfunc f(m map[int]int) {\n\tif v := 1; v > 0 {\n\t\t_ = v\n\t}\n\tfor i := 0; i < 1; i++ {\n\t\t_ = i\n\t}\n\tfor k, v := range m {\n\t\t_, _ = k, v\n\t}\n\t_ = i\n\t_ = k\n\t_ = v\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert_eq!(
        brief(&ds),
        vec![
            (
                DiagCode::TypUndeclaredName,
                13,
                col_of(a, 13, "i"),
                "undefined: i"
            ),
            (
                DiagCode::TypUndeclaredName,
                14,
                col_of(a, 14, "k"),
                "undefined: k"
            ),
            (
                DiagCode::TypUndeclaredName,
                15,
                col_of(a, 15, "v"),
                "undefined: v"
            ),
        ],
        "三处外层引用各一条；**头里面**的引用一条都不该有（否则说明变量没声明进作用域）"
    );
}

/// 类型 switch 的隐式变量：官方把它声明进**每个 case** 的作用域
/// （`stmt.go:762`）⇒ case 里可见、switch 之后不可见。
#[test]
fn t_ty_53_类型switch的隐式变量只在case里() {
    let a = "package p\n\nvar x any\n\nfunc f() {\n\tswitch v := x.(type) {\n\tcase int:\n\t\t_ = v\n\t}\n\t_ = v\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert_eq!(
        brief(&ds),
        vec![(
            DiagCode::TypUndeclaredName,
            10,
            col_of(a, 10, "v"),
            "undefined: v"
        )],
        "case 里可见（无诊断）、switch 之后不可见（这条）"
    );
}

/// `select` 的通信子句自成一层（官方 `stmt.go:786` 起）。
#[test]
fn t_ty_53_select子句自成一层() {
    let a = "package p\n\nvar c chan int\n\nfunc f() {\n\tselect {\n\tcase v := <-c:\n\t\t_ = v\n\t}\n\t_ = v\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert_eq!(
        brief(&ds),
        vec![(
            DiagCode::TypUndeclaredName,
            10,
            col_of(a, 10, "v"),
            "undefined: v"
        )]
    );
}

/// 函数字面量：参数进它自己的作用域，闭包**看得见**外层局部；
/// 看不见的只有外部也不存在的名字。
#[test]
fn t_ty_53_函数字面量与闭包() {
    let a = "package p\n\nfunc f() {\n\tx := 1\n\tg := func(y int) int {\n\t\treturn x + y\n\t}\n\t_ = g\n\th := func() int {\n\t\treturn z\n\t}\n\t_ = h\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert_eq!(
        brief(&ds),
        vec![(
            DiagCode::TypUndeclaredName,
            10,
            col_of(a, 10, "z"),
            "undefined: z"
        )],
        "闭包能看外层 `x`；`z` 哪都没有"
    );
}

/// 接收者 / 参数 / 结果 / 类型参数名都进函数作用域（官方 `funcType` 那一路）。
#[test]
fn t_ty_53_接收者参数与类型参数都是名字() {
    let a = "package p\n\ntype T struct{}\n\nfunc (r T) M(x int) int {\n\treturn x\n}\n\nfunc g[U any](x U) U {\n\treturn x\n}\n\nfunc h[U ~[]int](x U) U {\n\treturn x\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert!(ds.is_empty(), "全都有声明：{:?}", brief(&ds));
}

/// 接收者**自己声明**的类型参数（`TYP-22`）：`func (b *Box[T]) M()` 里的 `T`
/// 由**接收者的实参列表**声明（官方 `recv.TypeParams()`），不是从别处借的。
///
/// # 谁发现的
///
/// `Task 35` 的语料普查：`types/testdata/fixedbugs/issue39755.go`、`issue40038.go`、
/// `issue66751.go` 三个文件在 rgoc 侧退出码 1（`undefined: T`），而 oracle 侧 0。
/// 三例同一个形状：`func (pp *PathParams[T]) IsNil()` / `func (A[T]) m(A[T])` /
/// `func (f *fooer[A]) foo(A)`。
///
/// # 两处都要可见（体里也常引用它）
///
/// 签名（接收者类型 / 参数 / 结果）由**包级**那一趟解析，体由**体内**那一趟走；
/// 两趟各建各的作用域 ⇒ 只修一处会留下「签名好了、体里还是 `undefined: T`」
/// 的半修状态。所以这条用例把 `var x T` 放进体里。
///
/// # 约束按**位置**取
///
/// `func (n *NBox[U]) Alt() U` —— 实参名可以与声明处**不同**（`U` vs `T`），
/// 约束取类型声明里**同位置**那个（`Num`）。官方就是这么做的（实测 oracle exit 0）。
#[test]
fn t_ty_53_接收者声明的类型参数在签名与体内可见() {
    let a = "package p\n\ntype Box[T any] struct{ v T }\n\nfunc (b *Box[T]) Get() T {\n\tvar x T\n\treturn x\n}\n\ntype Num interface{ ~int }\n\ntype NBox[T Num] struct{ v T }\n\nfunc (n *NBox[U]) Alt() U {\n\treturn n.v\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert!(
        ds.is_empty(),
        "接收者的类型参数该在签名与体内都可见：{:?}",
        brief(&ds)
    );
}

/// 反向：基类型**不是泛型**（`Box` 没有 `[T]`）⇒ 实参里的 `T` 仍然未定义。
///
/// 没有这条，「凡接收者带实参就不报」的实现也会让上一条通过。
#[test]
fn t_ty_53_非泛型接收者的实参仍是未定义() {
    let a = "package p\n\ntype Box struct{}\n\nfunc (b *Box[T]) M() {}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    let col = col_of(a, 5, "T");
    assert_eq!(
        brief(&ds),
        vec![(DiagCode::TypUndeclaredName, 5, col, "undefined: T")]
    );
}

// ── `T-TY-54` 未定义 ──────────────────────────────────────────────────────

/// 值位置的未定义：**每一次出现各一条**（官方 `undef.go` 实测两条）。
#[test]
fn t_ty_54_值位置未定义每次出现各一条() {
    let a = "package p\n\nfunc f() {\n\ty = 1\n\t_ = y\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    let c4 = col_of(a, 4, "y");
    let c5 = col_of(a, 5, "y");
    assert_ne!(c4, c5, "两处引用必须先证明列号不同，否则这条断言是恒真的");
    assert_eq!(
        brief(&ds),
        vec![
            (DiagCode::TypUndeclaredName, 4, c4, "undefined: y"),
            (DiagCode::TypUndeclaredName, 5, c5, "undefined: y"),
        ]
    );
}

/// 反向：声明过就不报（否则上一条可能只是「凡有 y 就报」）。
#[test]
fn t_ty_54_已声明就不报() {
    let a = "package p\n\nfunc f() {\n\tvar y int\n\ty = 1\n\t_ = y\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert!(ds.is_empty(), "{:?}", brief(&ds));
}

/// 类型位置与值位置**同一条代号**（官方：类型 `typexpr.go:30`、值由解析器收尾），
/// 位置各报在**出现处**。
#[test]
fn t_ty_54_类型位置与值位置各一条() {
    let a = "package p\n\nvar z T\nvar y = Q\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert_eq!(
        brief(&ds),
        vec![
            (
                DiagCode::TypUndeclaredName,
                3,
                col_of(a, 3, "T"),
                "undefined: T"
            ),
            (
                DiagCode::TypUndeclaredName,
                4,
                col_of(a, 4, "Q"),
                "undefined: Q"
            ),
        ]
    );
}

/// ⚠️ **克隆的类型表达式只报一次**：前端把 `x, y Undef` 拆成**两份** `Field`，
/// 类型表达式是克隆的（`Pos` 相同）⇒ 官方报一条。三条路各来一条：
/// 包级函数签名（`collect_decls` 那条路）、函数字面量（体那条路）、结构体字段。
#[test]
fn t_ty_54_同一书写位置的克隆类型只报一次() {
    let a = "package p\n\nfunc f(x, y Undef) {}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert_eq!(
        brief(&ds),
        vec![(
            DiagCode::TypUndeclaredName,
            3,
            col_of(a, 3, "Undef"),
            "undefined: Undef"
        )],
        "`x, y Undef` 是一次书写"
    );

    let b = "package p\n\nvar g = func(x, y Undef) {}\n";
    let (_pkg, ds) = load(&[("a.go", b)]);
    assert_eq!(
        brief(&ds),
        vec![(
            DiagCode::TypUndeclaredName,
            3,
            col_of(b, 3, "Undef"),
            "undefined: Undef"
        )],
        "函数字面量那一路也要去重"
    );

    let c = "package p\n\ntype S struct {\n\ta, b Undef\n}\n";
    let (_pkg, ds) = load(&[("a.go", c)]);
    assert_eq!(
        brief(&ds),
        vec![(
            DiagCode::TypUndeclaredName,
            4,
            col_of(c, 4, "Undef"),
            "undefined: Undef"
        )],
        "结构体字段那一路也要去重"
    );
}

// ── `T-TY-55` 重复声明 ────────────────────────────────────────────────────

/// 块级重复：官方报在**后来**那个声明处（`dup_block.go:5:6`），且只有一条
/// （官方另有 `other declaration of x` 附加行，归 `open_gaps`）。
#[test]
fn t_ty_55_块级重复声明() {
    let a = "package p\n\nfunc f() {\n\tvar x int\n\tvar x int\n\t_ = x\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert_eq!(
        brief(&ds),
        vec![(
            DiagCode::TypDuplicateDecl,
            5,
            col_of(a, 5, "x"),
            "x redeclared in this block"
        )]
    );
}

/// 包级重复（同一文件）：官方 `dup_pkg.go:4:5` 也是 `redeclared in this block`
/// —— 代号同一条（`DuplicateDecl`），只是我方按 SCP 口径把措辞分开。
#[test]
fn t_ty_55_包级重复声明() {
    let a = "package p\n\nvar x int\nvar x int\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert_eq!(
        brief(&ds),
        vec![(
            DiagCode::TypDuplicateDecl,
            4,
            col_of(a, 4, "x"),
            "x redeclared in this package"
        )]
    );
}

/// 反向：不同层同名不是重复（块级 vs 包级）。
#[test]
fn t_ty_55_不同层同名不是重复() {
    let a = "package p\n\nvar x int\n\nfunc f() {\n\tvar x string\n\t_ = x\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert!(ds.is_empty(), "{:?}", brief(&ds));
}

/// `:=` 的官方顺序（`assignments.go:534`）：**先**分类（同层已有 = 赋值）、
/// **再**求值右侧、**最后**插入新变量。
///
/// 这条顺序在 `x := x` 上可观测：外层有 `x` 时右侧取**外层**那个（无诊断）；
/// 外层没有时右侧仍是 `undefined: x`（新变量此刻还没进作用域）。
#[test]
fn t_ty_55_短变量声明_同层已有是赋值() {
    let a = "package p\n\nvar x int\n\nfunc f() {\n\tx := x\n\tx, y := 2, 3\n\t_, _ = x, y\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert!(ds.is_empty(), "{:?}", brief(&ds));

    let b = "package p\n\nfunc f() {\n\tq := q\n\t_ = q\n}\n";
    let (_pkg, ds) = load(&[("a.go", b)]);
    let lhs = col_of(b, 4, "q");
    let rhs = lhs + "q := ".len() as u32;
    assert_ne!(lhs, rhs, "先证 LHS 与 RHS 两列不同");
    assert_eq!(
        brief(&ds),
        vec![(DiagCode::TypUndeclaredName, 4, rhs, "undefined: q")],
        "右侧在**新变量进作用域之前**求值（报在 RHS 那个 `q`）"
    );
}

/// `x, y, y := …` ⇒ 官方 `RepeatedDecl`（`assignments.go:552`），
/// 只看**这一条 `:=` 的左值列表**，与作用域里本来有什么无关。
#[test]
fn t_ty_55_短变量声明左侧重名() {
    let a = "package p\n\nfunc f() {\n\tp, q, q := 1, 2, 3\n\t_, _, _ = p, q, q\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    let first = col_of(a, 4, "q");
    let second = a
        .lines()
        .nth(3)
        .unwrap()
        .match_indices("q")
        .nth(1)
        .map(|(i, _)| i as u32 + 1)
        .expect("第 4 行有两个 q");
    assert_ne!(first, second, "先证两个 `q` 的列不同");
    assert_eq!(
        brief(&ds),
        vec![(
            DiagCode::TypRepeatedDecl,
            4,
            second,
            "q repeated on left side of :="
        )],
        "报在**第二个** `q` 处"
    );
}

// ── 假阳性守卫 ────────────────────────────────────────────────────────────

/// 一个把各条路都踩一遍的**合法**程序 ⇒ 一条诊断都不许有。
///
/// 尤其盯住**结构体字面量的裸名字 key**（`S{A: 1}`）：它长得像引用，
/// 但它是**字段名**；「当引用处理」会把每一个结构体字面量都报成 `undefined`。
#[test]
fn 合法程序零诊断() {
    let a = "package p\n\ntype S struct {\n\tA int\n\tB string\n}\n\nvar s = S{A: 1, B: \"x\"}\n\nfunc f(m map[int]int, c chan int) {\n\t_ = S{A: 1}\n\t_ = map[int]int{1: 2}\n\t_ = []int{1, 2}\n\tfor range m {\n\t}\n\tselect {\n\tcase v := <-c:\n\t\t_ = v\n\tdefault:\n\t}\n\tg := func(a int) int { return a }\n\t_ = g(1)\n\t_ = s\n}\n";
    let (_pkg, ds) = load(&[("a.go", a)]);
    assert!(ds.is_empty(), "合法程序被误报了：{:?}", brief(&ds));
}
