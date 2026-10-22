//! `T-EX-60` D 族之一：复合赋值的**移位**面（M5/Task 5）。
//!
//! # 规范原文（`go_spec.html` §Operators / Shift operators）
//!
//! > The right operand in a shift expression must have integer type or be an
//! > untyped constant representable by a value of type `uint`.
//!
//! 注意它**只要求整数**，不要求与左操作数同类型 —— `var i int; var c uint;
//! i <<= c` 合法。语料里的形态是 `ken/shift.go:82` 的 `i <<= c`。
//!
//! # 官方实现路径（`go/types`）
//!
//! `stmt.go:518-530` 把复合赋值整条交给 `binary`：
//!
//! ```text
//! op := assignOp(s.Tok)                                  // `<<=` ⇒ `<<`
//! var x operand
//! check.binary(&x, nil, s.Lhs[0], s.Rhs[0], op, s.TokPos) // 第二个参数 = nil
//! if !x.isValid() { return }
//! check.assignVar(s.Lhs[0], nil, &x, "assignment")
//! ```
//!
//! `binary` 见 `op` 是移位就转 `shift`（`expr.go:802`），**完全不看两侧类型是否
//! 相同**；`shift`（`expr.go:618`）对计数只查「整数 / 可表示于 `uint` / 非负」。
//!
//! # 我们原先错在哪
//!
//! `stmt.rs::compound_assign` 是自建近似：`identical(tx, ty) || assignable(ty, tx)`
//! ⇒ `i <<= c` 判成 `mismatched types int and uint`（假阳性）。同一近似还有第二处
//! 分歧：`i += 1.5` 官方是 `convert` @ **右操作数**（`matchTypes` → `convertUntyped`
//! 截断），近似是 `op-invalid` @ 左操作数。
//!
//! 修法是**删掉近似、回到官方结构**（`ExprChecker::binary_assign` + `assign_lhs`），
//! 于是移位计数类型、未类型化常量截断、除零、`&^=` 一次全对。
//!
//! # oracle 证据
//!
//! 两个夹具与 `.superpowers/m5-probe/d2-{accept,reject}.go` 逐字节一致，oracle
//! （`tools/types-oracle --check`）实测：
//!
//! ```text
//! 拒绝夹具：8 行 —— 8:8 shift / 9:8 convert / 10:8 shift /
//!                  14:2 op-invalid / 15:2 op-invalid / 16:7 convert /
//!                  17:2 op-invalid / 18:2 op-invalid
//! 接受夹具：0 行
//! ```
//!
//! 与下表逐项相同。

use rgoc_check::check::{CheckOptions, check_package};
use rgoc_load::{Package, load_sources};
use rgoc_types::TypeTable;

/// 接受向量：移位计数为**任意**整数类型，以及常规复合赋值。
const ACCEPT: &str = r#"package p

var i int
var s string
var u uint
var f float64
var c uint
var ch byte

func shiftOK() {
	i <<= c
	i <<= 2
	i >>= u
	i >>= 2.0
	i <<= ch
	i = i << c
	i <<= 1 + 1
}

func compoundOK() {
	i += 1
	f += 1
	s += "a"
	i *= 2.0
	i &= 3
	i &^= 1
	i /= 2
	i %= 3
	i++
	i--
	_ = i
	_ = f
	_ = s
}
"#;

/// 拒绝向量：计数非整数 / 截断 / 负数，与各运算符的谓词失配。
const REJECT: &str = r#"package p

var i int
var s string
var f float64

func shiftBad() {
	i <<= f
	i <<= 1.5
	i <<= -1
}

func compoundBad() {
	s += 1
	i += s
	i += 1.5
	f %= 2
	s %= "a"
}
"#;

/// 期望 `(行, 列, 类别)` —— 与 oracle 渲染行**逐项相同**（`C8` §6-5）。
const EXPECT: [(u32, u32, &str); 8] = [
    (8, 8, "shift"),
    (9, 8, "convert"),
    (10, 8, "shift"),
    (14, 2, "op-invalid"),
    (15, 2, "op-invalid"),
    (16, 7, "convert"),
    (17, 2, "op-invalid"),
    (18, 2, "op-invalid"),
];

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

#[test]
fn t_ex_60_移位计数只需整数_接受面零诊断() {
    let (mut tt, pkg, bag) = load(ACCEPT);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    assert_eq!(rows(&checked.render("t.go")), Vec::<String>::new());
}

#[test]
fn t_ex_60_拒绝面位置与类别对齐_oracle() {
    let (mut tt, pkg, bag) = load(REJECT);
    let checked = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    let got = rows(&checked.render("t.go"));
    assert_eq!(got.len(), EXPECT.len(), "实际渲染行：{got:?}");
    for (i, (line, col, cat)) in EXPECT.iter().enumerate() {
        // 渲染行形态：`<行>:<列>\t<类别>`（`C8` §3；文件名不在行内 ——
        // `render(file)` 的参数只用于筛文件）。
        let want = format!("{line}:{col}\t{cat}");
        assert_eq!(got[i], want, "第 {i} 条（期望 {want}）");
    }
}
