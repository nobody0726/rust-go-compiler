//! `T-CK-76` —— 9 缺测试项的 M4 段自有 fixture（M4/Task 36）。
//!
//! `docs/02-test-inventory.md` 列了 9 个「官方没有专用测试」的项。其中 5 项的
//! **静态面**落在本阶段，必须由本仓自有断言补齐（`SCP-06` 已在 M3；`RT-*` 三项
//! 留 Runtime 阶段）：
//!
//! | 项 | 本文件的断言 | 判据来源 |
//! |---|---|---|
//! | `TYP-26` 类型统一算法 | 三模式 × 结构递归 / 数组长度 / 映射 / 具名剥本体 + 对称性 | 官方 `go/types/unify.go`（M4/Task 13 已实现，这里补**直接**向量） |
//! | `EXP-16` 运算符优先级 | 8 条表达式的 **AST 嵌套形状**（5 级 + 左结合 + 一元最高） | 官方 `syntax/parser.go` 的 `prec` 表（`spec:L4886-4925`） |
//! | `EXP-22` 逻辑运算符静态面 | 可类型化 + 非布尔拒绝 + **常量折叠不短路** | oracle 实测（`probe/t36/exp22.go`，6 行） |
//! | `PKG-04` 零值 / 类型完备性 | 13 种类型零值声明零诊断 + `valid_type` 完备性谓词 | 官方 `go/types/validtype.go`（`spec:L8179-8228`） |
//! | `PKG-06` 初始化依赖循环 | 三节点环 1 条 + 合法链 0 条（消息链逐字） | 官方 `go/types/initorder.go`（`spec:L8229-8394`） |
//!
//! # 为什么 `EXP-16` 走 `parse_file` 而不是 `check_package`
//!
//! 优先级是**语法**事实：`a + b*c` 与 `(a+b)*c` 在检查面都可能合法（只要类型
//! 允许），诊断条数一模一样。唯一能钉住它的观察点是 **AST 形状** —— 所以本项
//! 必须看 parser 的产物，不能看诊断。这也是 `T-CK-76` 要在 `rgoc-check` 里
//! 加 `rgoc-parse` 开发依赖的原因（`Cargo.toml` 的 `[dev-dependencies]`）。
//!
//! # `EXP-22` 的「不短路」是实测结论，不是猜的
//!
//! `probe/t36/exp22.go` 里 `const c1 = false && (1/0 == 0)` 在官方 `go/types`
//! 下**照样**报 `invalid operation: division by zero`（16:24 / 17:23 / 18:23）。
//! 即：常量折叠对 `&&` / `||` **不做**短路求值。短路是**运行时**语义
//! （`M4-design.md` §3.2 对 `EXP-22` 的静态面定义），常量面必须两侧都求值。

use rgoc_ast::{Decl, Expr, Op};
use rgoc_check::check::{CheckOptions, check_package};
use rgoc_const::{BasicId, BigInt, Const};
use rgoc_lex::DiagCode;
use rgoc_load::{Package, load_sources};
use rgoc_parse::parse_file;
use rgoc_types::{
    ChanDir, Field, IfaceTSet, TypeId, TypeTable,
    UnifyMode::{Assign, Exact, Loose},
    unify, valid_type,
};

// ── 通用助手 ────────────────────────────────────────────────────────

/// 装载一段源码，返回 `(表, 包, 装载诊断)`。
fn load(src: &str) -> (TypeTable, Package, rgoc_lex::DiagBag) {
    let mut tt = TypeTable::new();
    let (pkg, bag) = load_sources(&mut tt, &[("t.go", src.as_bytes())], None);
    (tt, pkg, bag)
}

/// C8 渲染的**数据行**（去掉 `#` 注释行）。
fn rows(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// 跑一次检查，返回 `(渲染数据行, 诊断码序列)`。
fn check(src: &str) -> (Vec<String>, Vec<DiagCode>) {
    let (mut tt, pkg, bag) = load(src);
    let c = check_package(&pkg, &mut tt, bag, &CheckOptions::default());
    let codes = c.diags.iter().map(|d| d.diag.code).collect();
    (rows(&c.render("t.go")), codes)
}

/// 解析 `package p` + `const x = <expr>`，返回唯一的值表达式。
fn one_expr(expr_src: &str) -> Expr {
    let src = format!("package p\n\nconst x = {expr_src}\n");
    let r = parse_file(src.as_bytes(), "t.go");
    assert_eq!(r.errors, 0, "`{expr_src}` 该是干净的：{:?}", r.diags);
    let f = r
        .file
        .unwrap_or_else(|| panic!("应产出 AstFile：{:?}", r.diags));
    match f.decls.as_slice() {
        [Decl::Const(c)] => {
            assert_eq!(c.values.len(), 1, "`{expr_src}` 只有一个值");
            c.values.first().cloned().expect("len 已断言为 1")
        }
        other => panic!("期望恰好一条 ConstDecl，got {other:?}"),
    }
}

/// 拆二元运算 → `(op, x, y)`。
fn bin(e: &Expr) -> (Op, Expr, Expr) {
    match e {
        Expr::Binary(o) => {
            let y = o.y.as_deref().expect("二元运算必须有 y").clone();
            (o.op, (*o.x).clone(), y)
        }
        other => panic!("期望二元 Operation，got {other:?}"),
    }
}

/// 拆一元运算 → `(op, x)`。
fn un(e: &Expr) -> (Op, Expr) {
    match e {
        Expr::Unary(o) => {
            assert!(o.y.is_none(), "一元运算的 `Y` 必须是 None");
            (o.op, (*o.x).clone())
        }
        other => panic!("期望一元 Operation，got {other:?}"),
    }
}

/// 标识符名（不是名字就 panic）。
fn name_of(e: &Expr) -> &str {
    match e {
        Expr::Name(n) => &n.value,
        other => panic!("期望 Name，got {other:?}"),
    }
}

// ── TYP-26：类型统一算法（直接向量）─────────────────────────────────

fn int(tt: &mut TypeTable) -> TypeId {
    tt.add_basic(BasicId::Int)
}

fn string(tt: &mut TypeTable) -> TypeId {
    tt.add_basic(BasicId::String)
}

fn field(name: &str, typ: TypeId) -> Field {
    Field {
        name: Some(name.to_string()),
        typ,
        tag: None,
        embedded: false,
    }
}

/// 常量 `1` 的 id（数组长度用）。
fn len_one(tt: &mut TypeTable) -> rgoc_const::ConstId {
    tt.consts_mut().add(Const::Int(BigInt::from_i64(1)))
}

/// `[1]int`。
fn arr_one(tt: &mut TypeTable) -> TypeId {
    let c = len_one(tt);
    let i = tt.add_basic(BasicId::Int);
    tt.add_array(Some(c), i)
}

#[test]
fn t_ck_76_typ26_统一_结构递归与数组长度() {
    let mut tt = TypeTable::new();
    let i = int(&mut tt);
    let s = string(&mut tt);

    // ① 同形结构体：三模式全真
    let st1 = tt.add_struct(vec![field("f", i)]);
    let s1 = tt.struct_type(st1);
    let st2 = tt.add_struct(vec![field("f", i)]);
    let s2 = tt.struct_type(st2);
    for mode in [Loose, Exact, Assign] {
        assert!(
            unify(&tt, mode, s1, s2, &mut vec![]).unwrap(),
            "{mode:?}：同形结构体必须统一"
        );
    }

    // ② 字段类型不同：三模式全假（字段**类型**参与同一性，不只是名字）
    let st3 = tt.add_struct(vec![field("f", s)]);
    let s3 = tt.struct_type(st3);
    for mode in [Loose, Exact, Assign] {
        assert!(
            !unify(&tt, mode, s1, s3, &mut vec![]).unwrap(),
            "{mode:?}：字段类型不同必须不统一"
        );
    }

    // ③ 数组：长度是**类型的一部分**（`[1]int` ≠ `[2]int`）
    let c2 = tt.consts_mut().add(Const::Int(BigInt::from_i64(2)));
    let a1 = arr_one(&mut tt);
    let ii = tt.add_basic(BasicId::Int);
    let a2 = tt.add_array(Some(c2), ii);
    assert!(unify(&tt, Exact, a1, a1, &mut vec![]).unwrap());
    assert!(!unify(&tt, Exact, a1, a2, &mut vec![]).unwrap());

    // ④ 映射：键与值都递归
    let m1 = tt.add_map(i, s);
    let m2 = tt.add_map(i, s);
    let m3 = tt.add_map(s, s);
    assert!(unify(&tt, Exact, m1, m2, &mut vec![]).unwrap());
    assert!(!unify(&tt, Exact, m1, m3, &mut vec![]).unwrap());

    // ⑤ 具名剥本体：Loose/Assign 真、Exact 假（与 `T-CK-24` 的三模式表同源，
    //    这里换成**结构体**本体，避免与已有向量重复）
    let n = tt.add_named("NS");
    tt.set_named_underlying(n, s1);
    let nt = tt.named_type(n);
    assert!(unify(&tt, Loose, nt, s2, &mut vec![]).unwrap());
    assert!(unify(&tt, Assign, nt, s2, &mut vec![]).unwrap());
    assert!(!unify(&tt, Exact, nt, s2, &mut vec![]).unwrap());

    // ⑥ 对称性：统一不是单向的
    assert!(unify(&tt, Loose, s2, nt, &mut vec![]).unwrap());

    // ⑦ 通道方向：只在 Exact 参与（`Assign` 的收紧只针对元素）
    let both = tt.add_chan(ChanDir::SendRecv, i);
    let send = tt.add_chan(ChanDir::SendOnly, i);
    assert!(unify(&tt, Loose, both, send, &mut vec![]).unwrap());
    assert!(unify(&tt, Assign, both, send, &mut vec![]).unwrap());
    assert!(!unify(&tt, Exact, both, send, &mut vec![]).unwrap());
}

// ── EXP-16：运算符优先级（AST 形状）────────────────────────────────

/// 五级优先级矩阵：每条都**双向**钉住「谁在外、谁在内」。
///
/// 官方 `prec` 表（`spec:L4886-4925`）：
/// `* / % << >> & &^`(5) > `+ - | ^`(4) > `== != < <= > >=`(3) >
/// `&&`(2) > `||`(1)；一元最高、同级左结合。
#[test]
fn t_ck_76_exp16_优先级矩阵() {
    // ① 乘高于加
    let (op, x, y) = bin(&one_expr("a + b*c"));
    assert_eq!(op, Op::Add, "`+` 必须在外层");
    assert_eq!(name_of(&x), "a");
    let (op2, x2, y2) = bin(&y);
    assert_eq!(op2, Op::Mul, "`*` 必须在内层");
    assert_eq!((name_of(&x2), name_of(&y2)), ("b", "c"));

    // ② 反序：`a*b + c` ⇒ `*` 在内、`+` 在外
    let (op, x, y) = bin(&one_expr("a*b + c"));
    assert_eq!(op, Op::Add);
    assert_eq!(bin(&x).0, Op::Mul);
    assert_eq!(name_of(&y), "c");

    // ③ 移位(5) 高于加(4)：`a + b<<2` ⇒ `a + (b<<2)`
    let (op, x, y) = bin(&one_expr("a + b<<2"));
    assert_eq!(op, Op::Add);
    assert_eq!(name_of(&x), "a");
    assert_eq!(bin(&y).0, Op::Shl, "移位必须整体落在右操作数里");

    // ④ 同级左结合：`a - b - c` ⇒ `(a-b) - c`
    let (op, x, y) = bin(&one_expr("a - b - c"));
    assert_eq!(op, Op::Sub);
    assert_eq!(bin(&x).0, Op::Sub, "左结合：内层在**左**边");
    assert_eq!(name_of(&y), "c");

    // ⑤ `|`/`^` 同级(4)、`&` 高一级(5)：`a | b ^ c & d` ⇒ `(a|b) ^ (c&d)`
    let (op, x, y) = bin(&one_expr("a | b ^ c & d"));
    assert_eq!(op, Op::Xor, "同级左结合 ⇒ 最外层是 `^`");
    assert_eq!(bin(&x).0, Op::Or);
    assert_eq!(bin(&y).0, Op::And);

    // ⑥ 比较(3) > 逻辑与(2) > 逻辑或(1)：`a == b && c || d` ⇒ `((a==b) && c) || d`
    let (op, x, y) = bin(&one_expr("a == b && c || d"));
    assert_eq!(op, Op::LOr);
    assert_eq!(name_of(&y), "d");
    let (op2, x2, y2) = bin(&x);
    assert_eq!(op2, Op::LAnd);
    assert_eq!(bin(&x2).0, Op::Eql, "比较必须比 `&&` 更深");
    assert_eq!(name_of(&y2), "c");

    // ⑦ 一元最高：`!a && b` ⇒ `(!a) && b`
    let (op, x, y) = bin(&one_expr("!a && b"));
    assert_eq!(op, Op::LAnd);
    let (uop, ux) = un(&x);
    assert_eq!(uop, Op::Not);
    assert_eq!(name_of(&ux), "a");
    assert_eq!(name_of(&y), "b");

    // ⑧ `&^`(5) 高于 `+`(4)：`a &^ b + c` ⇒ `(a&^b) + c`
    let (op, x, _y) = bin(&one_expr("a &^ b + c"));
    assert_eq!(op, Op::Add);
    assert_eq!(bin(&x).0, Op::AndNot);
}

// ── EXP-22：逻辑运算符的静态面 ──────────────────────────────────────

/// 可类型化面 + 非布尔拒绝 + 常量折叠**不短路**（与 `probe/t36/exp22.go`
/// 逐字节一致的 6 行）。
#[test]
fn t_ck_76_exp22_逻辑运算符静态面() {
    // ① 可类型化：`&&`/`||`/`!` 在 bool 上零诊断
    const OK: &str = r#"package p

var a bool
var b bool

var ok1 = true && false
var ok2 = true || false
var ok3 = !true
var ok4 = a && b
var ok5 = a || !b
"#;
    let (r, codes) = check(OK);
    assert!(r.is_empty(), "布尔逻辑运算必须零诊断，got {r:?}");
    assert!(codes.is_empty(), "got {codes:?}");

    // ② 拒绝面：非布尔操作数 ⇒ 3 条 `op-invalid`（位置 = 整个表达式起点）
    const BAD: &str = r#"package p

var bad1 = 1 && true
var bad2 = true || 1
var bad3 = !1
"#;
    let (r, codes) = check(BAD);
    assert_eq!(
        r,
        vec!["3:12\top-invalid", "4:12\top-invalid", "5:13\top-invalid"],
        "非布尔操作数三条（oracle 实测 12:12 / 13:12 / 14:13）"
    );
    assert!(
        codes.iter().all(|c| *c == DiagCode::ChkInvalidOperation),
        "全部应是 CHK-INVALID-OPERATION，got {codes:?}"
    );

    // ③ 常量折叠**不短路**：`false && (1/0==0)` 与 `true || (1/0==0)` 都要报
    //    （oracle 实测三条都在**除数**位置：16:24 / 17:23 / 18:23）
    const FOLD: &str = r#"package p

const c1 = false && (1/0 == 0)
const c2 = true || (1/0 == 0)
const c3 = true && (1/0 == 0)
"#;
    let (r, codes) = check(FOLD);
    assert_eq!(
        r,
        vec!["3:24\top-invalid", "4:23\top-invalid", "5:23\top-invalid"],
        "常量折叠对 `&&`/`||` 不短路（oracle 实测）"
    );
    assert!(
        codes.iter().all(|c| *c == DiagCode::TypDivByZero),
        "三条都是除零（类别投影到 `op-invalid`），got {codes:?}"
    );
}

// ── PKG-04：零值与类型完备性 ────────────────────────────────────────

/// 零值矩阵：13 种类型各一条 `var`，全部零诊断（「每种类型都有零值」是
/// 静态完备性的直接体现）。
#[test]
fn t_ck_76_pkg04_零值矩阵() {
    const SRC: &str = r#"package p

type T struct {
	a int
	b string
}

var i int
var f float64
var c complex128
var s string
var b bool
var p *int
var sl []int
var m map[string]int
var ch chan int
var fn func()
var arr [3]int
var st T
var iface interface{}
var e error
"#;
    let (r, codes) = check(SRC);
    assert!(r.is_empty(), "零值声明必须零诊断，got {r:?}");
    assert!(codes.is_empty(), "got {codes:?}");
}

/// 类型完备性谓词：**按值包含**才成环（`type A [1]A` 非法），
/// 经指针 / 切片 / 映射 / 通道的环合法。
#[test]
fn t_ck_76_pkg04_类型完备性谓词() {
    // ① `type A [1]A` ⇒ 非法（`TypInvalidRecursiveType`）
    let mut tt = TypeTable::new();
    let a = tt.add_named("A");
    let c1 = len_one(&mut tt);
    let arr = tt.add_array(Some(c1), tt.named_type(a));
    tt.set_named_underlying(a, arr);
    let mut bag = rgoc_lex::DiagBag::new();
    assert!(
        !valid_type(&tt, tt.named_type(a), &mut bag),
        "`[1]A` 必须非法"
    );
    let diags = bag.into_sorted();
    assert_eq!(diags.len(), 1, "恰好一条，got {diags:?}");
    assert_eq!(diags[0].code, DiagCode::TypInvalidRecursiveType);

    // ② 引用边（指针/切片/映射/通道）不成环 ⇒ 合法且零诊断
    for kind in 0..4 {
        let mut tt = TypeTable::new();
        let a = tt.add_named("A");
        let t = match kind {
            0 => tt.add_pointer(tt.named_type(a)),
            1 => tt.add_slice(tt.named_type(a)),
            2 => {
                let i = int(&mut tt);
                tt.add_map(i, tt.named_type(a))
            }
            _ => tt.add_chan(ChanDir::SendRecv, tt.named_type(a)),
        };
        tt.set_named_underlying(a, t);
        let mut bag = rgoc_lex::DiagBag::new();
        assert!(
            valid_type(&tt, tt.named_type(a), &mut bag),
            "引用边（kind={kind}）必须合法"
        );
        assert!(bag.into_sorted().is_empty(), "kind={kind} 不该有诊断");
    }

    // ③ 空接口 / 项集接口本身完备（`IfaceTSet::All` 与 `Terms`）
    let mut tt = TypeTable::new();
    let i = int(&mut tt);
    let anyi = tt.add_interface(Vec::new(), None, IfaceTSet::All);
    let any = tt.interface_type(anyi);
    let mut bag = rgoc_lex::DiagBag::new();
    assert!(valid_type(&tt, any, &mut bag));
    let termi = tt.add_interface(Vec::new(), None, IfaceTSet::Terms(vec![(true, i)]));
    let terms = tt.interface_type(termi);
    let mut bag = rgoc_lex::DiagBag::new();
    assert!(
        valid_type(&tt, terms, &mut bag),
        "`interface{{ ~int }}` 完备"
    );
}

// ── PKG-06：初始化依赖循环 ──────────────────────────────────────────

/// 三节点环 ⇒ **1 条**（官方 `reportCycle` 只报一次，消息带续行链）；
/// 合法链 ⇒ 0 条。
#[test]
fn t_ck_76_pkg06_初始化依赖循环() {
    const CYCLE: &str = r#"package p

var a = b
var b = c
var c = a
"#;
    let (r, codes) = check(CYCLE);
    assert_eq!(
        r,
        vec!["3:5\tcycle"],
        "三节点环只报一条（报点 = 环起点变量名）"
    );
    assert_eq!(codes, vec![DiagCode::TypInvalidInitCycle]);

    // 消息链逐字：`a refers to b` / `b refers to c` / `c refers to a`
    let (mut tt, pkg, _bag) = load(CYCLE);
    let an = rgoc_check::initorder::analyze(&pkg);
    assert_eq!(an.cycles.len(), 1, "一个环一条");
    assert_eq!(
        an.cycles[0].message,
        "initialization cycle for a\n\ta refers to b\n\tb refers to c\n\tc refers to a"
    );
    let _ = &mut tt;

    // 合法链：`d = e` / `e = 1` ⇒ 依赖有序、无环
    const DAG: &str = r#"package p

var d = e
var e = 1
"#;
    let (r, codes) = check(DAG);
    assert!(r.is_empty(), "合法链零诊断，got {r:?}");
    assert!(codes.is_empty(), "got {codes:?}");
}
