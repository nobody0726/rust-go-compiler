//! `T-TY-83`：常量表达式求值的**双向**验收（`D-M3-19`）。
//!
//! 正向 = 子集之内的表达式**必须不报错**且值正确；
//! 反向 = 需要类型信息 / 运行期值的表达式**必须**是 `NotAConstant`。
//! 两侧都写在同一处，因为「过宽」与「过窄」是同一个天平的两端。
//!
//! 诊断只断言**代号**（`M3-tests.md`：对齐官方**类别**，不复刻文本）。

use std::collections::HashMap;

use rgoc_ast::{BasicLit, CallExpr, Expr, LitKind, Name, Op, Operation, ParenExpr};
use rgoc_const::{Const, ConstEnv, EvalOutcome, Sizes, eval_const_expr};
use rgoc_lex::{Diag, DiagBag, DiagCode, FileTable, Pos};

const S: Sizes = Sizes::HOST_ARM64;

fn pos() -> Pos {
    let mut ft = FileTable::new();
    let file = ft.intern("t.go");
    Pos {
        base: None,
        file,
        line: 1,
        col: 1,
    }
}

fn lit(text: &str, kind: LitKind) -> Expr {
    Expr::BasicLit(BasicLit {
        value: text.to_string(),
        kind,
        bad: false,
        pos: pos(),
    })
}

fn ilit(text: &str) -> Expr {
    lit(text, LitKind::Int)
}

fn flit(text: &str) -> Expr {
    lit(text, LitKind::Float)
}

fn slit(text: &str) -> Expr {
    lit(text, LitKind::String)
}

fn name(n: &str) -> Expr {
    Expr::Name(Name {
        value: n.to_string(),
        pos: pos(),
    })
}

fn bin(op: Op, x: Expr, y: Expr) -> Expr {
    Expr::Binary(Operation {
        op,
        x: Box::new(x),
        y: Some(Box::new(y)),
        pos: pos(),
    })
}

fn un(op: Op, x: Expr) -> Expr {
    Expr::Unary(Operation {
        op,
        x: Box::new(x),
        y: None,
        pos: pos(),
    })
}

fn paren(x: Expr) -> Expr {
    Expr::Paren(ParenExpr {
        x: Box::new(x),
        pos: pos(),
    })
}

fn call(fun: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(CallExpr {
        fun: Box::new(name(fun)),
        args,
        has_dots: false,
        pos: pos(),
    })
}

/// 测试用环境：名字 → 常量值。
struct Env(HashMap<String, Const>);

impl Env {
    fn new(pairs: &[(&str, Const)]) -> Self {
        Env(pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect())
    }
}

impl ConstEnv for Env {
    fn lookup_const(&self, ident: &str) -> Option<&Const> {
        self.0.get(ident)
    }
}

fn eval(e: &Expr) -> (EvalOutcome, DiagBag) {
    eval_const_expr(e, &Env::new(&[]), &S)
}

fn eval_in(e: &Expr, env: &Env) -> (EvalOutcome, DiagBag) {
    eval_const_expr(e, env, &S)
}

fn codes(bag: DiagBag) -> Vec<DiagCode> {
    bag.into_sorted()
        .into_iter()
        .map(|d: Diag| d.code)
        .collect()
}

/// 值必须正确且**没有诊断**（正向）。
fn value(e: &Expr) -> Const {
    let (out, bag) = eval(e);
    let cs = codes(bag);
    assert!(cs.is_empty(), "不该报错，却报了 {cs:?}");
    match out {
        EvalOutcome::Const(v) => {
            assert_ne!(v, Const::Unknown, "形状上是常量但没有值");
            v
        }
        EvalOutcome::NotAConstant => panic!("应当是可折叠的常量表达式"),
    }
}

/// 必须**恰好**报一条给定代号的诊断（反向，且值退化成 `Unknown`）。
fn diag(e: &Expr, code: DiagCode) {
    let (out, bag) = eval(e);
    assert_eq!(codes(bag), vec![code], "期望恰好一条 {code:?}");
    assert_eq!(
        out,
        EvalOutcome::Const(Const::Unknown),
        "有错 ⇒ 没有值（照官方 avoid follow-up errors）"
    );
}

/// 必须是 `NotAConstant` 且**不报错**（判据在别处，本层闭嘴）。
fn not_const(e: &Expr) {
    let (out, bag) = eval(e);
    assert_eq!(codes(bag), Vec::<DiagCode>::new(), "本层不该报错");
    assert_eq!(out, EvalOutcome::NotAConstant);
}

fn int(v: i64) -> Const {
    Const::Int(rgoc_const::BigInt::from_i64(v))
}

fn s(v: &str) -> Const {
    Const::Str(v.as_bytes().to_vec())
}

// ── 正向：常量子集 ──────────────────────────────────────────────────────

#[test]
fn 子集_算术与优先级() {
    assert_eq!(
        value(&bin(Op::Add, ilit("1"), bin(Op::Mul, ilit("2"), ilit("3")))),
        int(7)
    );
    assert_eq!(value(&paren(bin(Op::Add, ilit("1"), ilit("2")))), int(3));
    // 截断除与取模的符号照 Go
    assert_eq!(value(&bin(Op::Quo, ilit("-7"), ilit("2"))), int(-3));
    assert_eq!(value(&bin(Op::Rem, ilit("-7"), ilit("2"))), int(-1));
    // 两个**整数**字面量相除 ⇒ **整数除法**（照 Go：`const A = 1/3` 的值是 `0`）
    assert_eq!(value(&bin(Op::Quo, ilit("1"), ilit("3"))), int(0));
    // 任一侧是浮点 ⇒ 有理数**精确**，不量化
    assert_eq!(
        value(&bin(Op::Quo, flit("1.0"), ilit("3"))),
        Const::Float(
            rgoc_const::BigRat::new(
                rgoc_const::BigInt::from_i64(1),
                rgoc_const::BigInt::from_i64(3)
            )
            .unwrap()
        )
    );
}

#[test]
fn 子集_位运算与移位() {
    assert_eq!(value(&bin(Op::And, ilit("6"), ilit("3"))), int(2));
    assert_eq!(value(&bin(Op::Or, ilit("6"), ilit("3"))), int(7));
    assert_eq!(value(&bin(Op::Xor, ilit("6"), ilit("3"))), int(5));
    assert_eq!(value(&bin(Op::AndNot, ilit("6"), ilit("3"))), int(4));
    assert_eq!(value(&un(Op::Xor, ilit("0"))), int(-1));
    assert_eq!(value(&bin(Op::Shl, ilit("1"), ilit("10"))), int(1024));
    assert_eq!(value(&bin(Op::Shr, ilit("1024"), ilit("10"))), int(1));
    // 移位计数上界 1074 **包含**在内
    let big = value(&bin(Op::Shl, ilit("1"), ilit("1074")));
    match big {
        Const::Int(v) => assert_eq!(v.bit_len(), 1075),
        other => panic!("期望整数，得到 {other:?}"),
    }
}

#[test]
fn 子集_比较与逻辑() {
    assert_eq!(
        value(&bin(Op::Lss, ilit("1"), ilit("2"))),
        Const::Bool(true)
    );
    assert_eq!(
        value(&bin(Op::Geq, ilit("1"), ilit("2"))),
        Const::Bool(false)
    );
    assert_eq!(
        value(&bin(Op::Eql, slit("\"a\""), slit("\"a\""))),
        Const::Bool(true)
    );
    assert_eq!(
        value(&bin(Op::Neq, slit("\"a\""), slit("\"b\""))),
        Const::Bool(true)
    );
    assert_eq!(
        value(&bin(Op::LAnd, name("true"), name("false"))),
        Const::Bool(false)
    );
    assert_eq!(
        value(&bin(Op::LOr, name("true"), name("false"))),
        Const::Bool(true)
    );
    assert_eq!(value(&un(Op::Not, name("false"))), Const::Bool(true));
    // 字符串按字节拼接
    assert_eq!(value(&bin(Op::Add, slit("\"a\""), slit("\"b\""))), s("ab"));
}

#[test]
fn 子集_引用常量与_iota() {
    let env = Env::new(&[("A", int(7)), ("iota", int(2))]);
    let e = bin(Op::Shl, name("A"), name("iota"));
    let (out, _) = eval_in(&e, &env);
    assert_eq!(out, EvalOutcome::Const(int(28)), "A << iota");
    // env 里没有的名字 ⇒ NotAConstant（**不是**「未知值」）
    assert_eq!(eval(&name("B")).0, EvalOutcome::NotAConstant);
    assert_eq!(eval(&name("nil")).0, EvalOutcome::NotAConstant);
}

#[test]
fn 子集_转换到预声明基本类型() {
    assert_eq!(value(&call("int64", vec![ilit("8")])), int(8));
    assert_eq!(
        value(&bin(Op::Mul, call("int64", vec![ilit("8")]), ilit("3"))),
        int(24)
    );
    assert_eq!(value(&call("int", vec![flit("2.0")])), int(2));
    assert_eq!(
        value(&call("float64", vec![ilit("1")])),
        Const::Float(rgoc_const::BigRat::from_i64(1))
    );
    // `string(65) == "A"`（码点语义）
    assert_eq!(value(&call("string", vec![ilit("65")])), s("A"));
    // 别名：byte == uint8、rune == int32
    assert_eq!(value(&call("byte", vec![ilit("255")])), int(255));
    assert_eq!(value(&call("rune", vec![ilit("97")])), int(97));
    // 转换在常量表达式中**保留值**；`float32(1)/3` 的值仍是精确 1/3
    // （typed 常量算术要量化 —— M3 未实现，见 open_gaps）
    assert_eq!(
        value(&bin(Op::Quo, call("float32", vec![ilit("1")]), ilit("3"))),
        Const::Float(
            rgoc_const::BigRat::new(
                rgoc_const::BigInt::from_i64(1),
                rgoc_const::BigInt::from_i64(3)
            )
            .unwrap()
        )
    );
    // 复数转换
    assert_eq!(
        value(&call("complex128", vec![ilit("1")])),
        Const::Complex {
            re: rgoc_const::BigRat::from_i64(1),
            im: rgoc_const::BigRat::from_i64(0)
        }
    );
}

// ── 反向（一）：需要类型信息 / 运行期值 ⇒ NotAConstant ─────────────────

#[test]
fn 非常量_函数调用与变量引用() {
    not_const(&call("f", vec![]));
    not_const(&name("x"));
    not_const(&bin(Op::Add, ilit("1"), name("x")));
    // 形如转换但目标是**具名**类型 ⇒ 要类型表（`T-TY-83` 正向）
    not_const(&call("MyInt", vec![ilit("1")]));
    // 数组/切片长度之外的类型字面量
    not_const(&Expr::Bad(rgoc_ast::BadExpr { pos: pos() }));
}

#[test]
fn 非常量_内建函数与复合表达式() {
    // 官方**会**折叠它们（实测 `min(1,2)==1`、`len("ab")==2`），
    // 但 M3 的常量子集不含内建 ⇒ 稳定地判为「非常量」（记入 open_gaps）
    not_const(&call("len", vec![slit("\"ab\"")]));
    not_const(&call("min", vec![ilit("1"), ilit("2")]));
    not_const(&call("real", vec![ilit("1")]));
    // 多参 / 带 `...` 的调用不是常量转换
    not_const(&call("int", vec![ilit("1"), ilit("2")]));
    // 转换目标不是基本类型名
    not_const(&call("unsafe", vec![ilit("1")]));
}

// ── 反向（二）：值域错误 ⇒ 稳定代号 ───────────────────────────────────

#[test]
fn 零除_四个变体都是_divbyzero() {
    assert_eq!(DiagCode::TypDivByZero.as_str(), "TYP-DIV-BY-ZERO");
    diag(&bin(Op::Quo, ilit("1"), ilit("0")), DiagCode::TypDivByZero);
    diag(&bin(Op::Rem, ilit("1"), ilit("0")), DiagCode::TypDivByZero);
    diag(
        &bin(Op::Quo, flit("1.0"), ilit("0")),
        DiagCode::TypDivByZero,
    );
    diag(
        &bin(Op::Quo, ilit("1"), flit("0.0")),
        DiagCode::TypDivByZero,
    );
}

#[test]
fn 移位计数三种非法() {
    diag(
        &bin(Op::Shl, ilit("1"), ilit("-1")),
        DiagCode::TypInvalidShiftCount,
    );
    diag(
        &bin(Op::Shl, ilit("1"), flit("1.5")),
        DiagCode::TypInvalidShiftCount,
    );
    diag(
        &bin(Op::Shl, ilit("1"), ilit("1075")),
        DiagCode::TypInvalidShiftCount,
    );
    diag(
        &bin(Op::Shr, ilit("1"), ilit("99999")),
        DiagCode::TypInvalidShiftCount,
    );
}

#[test]
fn 移位左操作数非整数() {
    diag(
        &bin(Op::Shl, slit("\"a\""), ilit("1")),
        DiagCode::TypInvalidShiftOperand,
    );
    diag(
        &bin(Op::Shl, name("true"), ilit("1")),
        DiagCode::TypInvalidShiftOperand,
    );
}

#[test]
fn 转换失败的两条代号() {
    // 转换语境：官方把「cannot convert …」归 `InvalidConversion`
    // （实测 `int(1.5)` / `bool(1)` / `float64(1i)` 都是这一类）
    diag(
        &call("int", vec![flit("1.5")]),
        DiagCode::TypInvalidConversion,
    );
    diag(
        &call("int", vec![slit("\"a\"")]),
        DiagCode::TypInvalidConversion,
    );
    diag(
        &call("bool", vec![ilit("1")]),
        DiagCode::TypInvalidConversion,
    );
}

#[test]
fn 转换溢出保留_overflow_代号() {
    // 官方实测：`int8(200)` ⇒ 「constant 200 overflows int8」**两种语境都是 overflows**
    diag(
        &call("int8", vec![ilit("200")]),
        DiagCode::TypNumericOverflow,
    );
    diag(
        &call("float32", vec![flit("1e400")]),
        DiagCode::TypNumericOverflow,
    );
    diag(
        &call("uint8", vec![ilit("-1")]),
        DiagCode::TypNumericOverflow,
    );
}

/// `0 + 1i`：用运算构造一个虚部非零的常量（不走字面量路径）。
fn one_i_expr() -> Expr {
    Expr::Binary(Operation {
        op: Op::Add,
        x: Box::new(ilit("0")),
        y: Some(Box::new(lit("1i", LitKind::Imag))),
        pos: pos(),
    })
}

#[test]
fn 复数情境的转换失败() {
    // `float64(1i)` 实测官方拒绝（"cannot convert …"）；
    // `float64(1+0i)`（零虚部）**通过** —— 两条都验
    diag(
        &call("float64", vec![one_i_expr()]),
        DiagCode::TypInvalidConversion,
    );
    assert_eq!(
        value(&call(
            "float64",
            vec![bin(Op::Add, ilit("1"), lit("0i", LitKind::Imag))]
        )),
        Const::Float(rgoc_const::BigRat::from_i64(1))
    );
}

#[test]
fn 坏字面量_没有值但不报错() {
    // 错由上游（parser）报过；本层照官方 avoid follow-up errors 闭嘴
    let bad = Expr::BasicLit(BasicLit {
        value: "1_".to_string(),
        kind: LitKind::Int,
        bad: true,
        pos: pos(),
    });
    let (out, bag) = eval(&bad);
    assert_eq!(codes(bag), Vec::<DiagCode>::new());
    assert_eq!(out, EvalOutcome::Const(Const::Unknown));
    assert!(!out.has_value());
    // 传染：参与运算后仍然没有值，也不报第二条错
    let (out2, bag2) = eval(&bin(Op::Add, bad, ilit("1")));
    assert_eq!(codes(bag2), Vec::<DiagCode>::new());
    assert_eq!(out2, EvalOutcome::Const(Const::Unknown));
}

// ── `^` 的类型依赖（Task 30 语料冒烟顺带发现）────────────────────────────

/// `^x` 的值**依赖 x 的类型**：无符号有类型 ⇒ 按宽度取补（非负）；
/// 有符号 / 无类型 ⇒ 无限精度补码 `-x-1`。
///
/// 真实触发点在语料 `unicode/utf8/utf8.go:459`：
/// `const ptrSize = 4 << (^uintptr(0) >> 63)`（官方 = `0` ⇒ `ptrSize = 8`）。
/// 折叠器早先对 `^` 一律用无限精度补码 ⇒ `^uintptr(0)` 得到 `-1`、
/// `>> 63` 得到 `-1`，于是**在合法代码上报了 `TYP-INVALID-SHIFT-COUNT`**。
#[test]
fn 无符号有类型操作数的补按宽度取补() {
    // ① 有类型**无符号**：结果非负，按该类型的宽度（本层只知道预声明基本类型）
    assert_eq!(
        value(&un(Op::Xor, call("uintptr", vec![ilit("0")]))),
        Const::Int(rgoc_const::BigInt::from_u64(u64::MAX)),
        "`^uintptr(0)` = 2^64-1（**不是** -1）"
    );
    assert_eq!(
        value(&un(Op::Xor, call("uint32", vec![ilit("0")]))),
        Const::Int(rgoc_const::BigInt::from_u64(0xFFFF_FFFF))
    );
    assert_eq!(
        value(&un(Op::Xor, call("uint8", vec![ilit("1")]))),
        Const::Int(rgoc_const::BigInt::from_u64(0xFE))
    );
    // 别名 `byte` 折到 `uint8`（`basic_from_name` 的口径）
    assert_eq!(
        value(&un(Op::Xor, call("byte", vec![ilit("1")]))),
        Const::Int(rgoc_const::BigInt::from_u64(0xFE))
    );
    // ② 有符号 / 无类型：无限精度补码，与宽度无关
    assert_eq!(value(&un(Op::Xor, ilit("0"))), int(-1));
    assert_eq!(value(&un(Op::Xor, call("int8", vec![ilit("0")]))), int(-1));
    assert_eq!(value(&un(Op::Xor, call("int64", vec![ilit("0")]))), int(-1));
    // ③ 语料里那一行的**逐字**形状：零诊断 + 值 8
    let ptr_size = bin(
        Op::Shl,
        ilit("4"),
        paren(bin(
            Op::Shr,
            un(Op::Xor, call("uintptr", vec![ilit("0")])),
            ilit("63"),
        )),
    );
    let (out, bag) = eval(&ptr_size);
    assert_eq!(codes(bag), Vec::<DiagCode>::new(), "合法表达式不许报错");
    assert_eq!(
        out,
        EvalOutcome::Const(int(8)),
        "`4 << (^uintptr(0) >> 63)` = 8"
    );
    // ④ 同形的 32 位版本：`^uint32(0) >> 63` = 0 ⇒ `4 << 0` = 4
    let p32 = bin(
        Op::Shl,
        ilit("4"),
        paren(bin(
            Op::Shr,
            un(Op::Xor, call("uint32", vec![ilit("0")])),
            ilit("63"),
        )),
    );
    assert_eq!(value(&p32), int(4));
}
