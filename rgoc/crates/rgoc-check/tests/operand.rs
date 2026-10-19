//! `T-CK-91` 操作数模式（M4/Task 20）：`go/types/operand.go:24-37` 的 11 模式 +
//! **值层类型接线**的落点。
//!
//! # 为什么这层是「值层没有类型接线」的落点
//!
//! `M3-manifest.json` `open_gaps` 记录：`-uint32(5)` / `uint32(0)-1` 漏报的
//! 根因是**值层没有类型接线** —— M3 的常量求值只留了值（`Object.val`），
//! 没有一处「名字 → 带类型的操作数」的映射。本文件把这条线接上：
//! [`Operand::from_object`] 把装载出的对象翻成操作数时**类型随值一起走**
//! （`typ` + `val`），Task 21 起的表达式检查从这里读「这个常量是什么类型」，
//! 溢出判定才有落点。
//!
//! # 模式枚举照官方（11 项）
//!
//! `invalid / novalue / builtin / typexpr / constant / variable / mapindex /
//! value / nilvalue / commaok / commaerr` —— 官方第 12 项 `cgofunc` **不进**
//! 本枚举（cgo 是 `-fakeImportC` 面，装置响亮失败，`U-M4-05`）。
//! 模式文本逐字照 `operandModeString`（`operand.go:38-52`）—— 它出现在官方
//! 诊断措辞里（`operand is nil`、`(value of type %s)` 一族），措辞即语义。

use rgoc_check::operand::{Mode, Operand};
use rgoc_const::{BigInt, Const};
use rgoc_load::{Package, load_sources};
use rgoc_types::{BasicId, ObjKind, Object, Type, TypeId, TypeTable};

/// 装载一个内存单文件包。
fn load(src: &str) -> (TypeTable, Package) {
    let mut tt = TypeTable::new();
    let (pkg, _bag) = load_sources(&mut tt, &[("t.go", src.as_bytes())], None);
    (tt, pkg)
}

/// `TypeId` 必须指到基本类型的哪一支。
fn basic_of(tt: &TypeTable, t: TypeId) -> BasicId {
    match tt.type_of(t) {
        Type::Basic(b) => *b,
        other => panic!("期望基本类型，实际 {other:?}"),
    }
}

/// 模式枚举 = 官方 11 项、字典序文本逐字一致，且**不含** `cgofunc`。
#[test]
fn operand_模式枚举_11_项_照官方文本() {
    assert_eq!(Mode::ALL.len(), 11, "官方 12 项去掉 cgofunc（U-M4-05）");
    let texts: Vec<&'static str> = Mode::ALL.iter().map(|m| m.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "invalid operand",
            "no value",
            "built-in",
            "type",
            "constant",
            "variable",
            "map index expression",
            "value",
            "nil",
            "comma, ok expression",
            "comma, error expression",
        ],
        "文本逐字照 operandModeString（operand.go:38-52）"
    );
    let uniq: std::collections::BTreeSet<&str> = texts.iter().copied().collect();
    assert_eq!(uniq.len(), 11, "文本不得重复");
}

/// 五个**可观察名类**经装载对象翻成操作数：常量 / 变量 / 类型 / `nil` / 内建。
///
/// 位置也随对象走（`Object.pos`）—— 诊断要指得到。
#[test]
fn operand_从装载对象_常量_变量_类型_nil_内建() {
    let (tt, pkg) = load("package p\n\nconst c = 1\nvar v int\ntype T int\n");

    let c = Operand::from_object(pkg.lookup("c").expect("常量对象")).expect("常量 ⇒ 操作数");
    assert_eq!(c.mode, Mode::Constant);
    assert_eq!(
        basic_of(&tt, c.typ.expect("常量带类型")),
        BasicId::UntypedInt
    );
    assert_eq!(
        c.val,
        Some(Const::Int(BigInt::from_i64(1))),
        "值随操作数一起走（值层接线的「值」半）"
    );
    assert!(c.pos.is_some(), "对象位置要带出来");

    let v = Operand::from_object(pkg.lookup("v").expect("变量对象")).expect("变量 ⇒ 操作数");
    assert_eq!(v.mode, Mode::Variable);
    assert_eq!(basic_of(&tt, v.typ.expect("变量带类型")), BasicId::Int);
    assert_eq!(v.val, None, "变量没有常量值");

    let t = Operand::from_object(pkg.lookup("T").expect("类型对象")).expect("类型名 ⇒ 操作数");
    assert_eq!(t.mode, Mode::TypeExpr);
    assert!(t.typ.is_some());

    let nil =
        Operand::from_object(pkg.lookup("nil").expect("universe 里的 nil")).expect("nil ⇒ 操作数");
    assert_eq!(nil.mode, Mode::NilValue);
    assert_eq!(
        basic_of(&tt, nil.typ.expect("nil 的类型")),
        BasicId::UntypedNil
    );

    let len =
        Operand::from_object(pkg.lookup("len").expect("universe 里的内建")).expect("内建 ⇒ 操作数");
    assert_eq!(
        len.mode,
        Mode::Builtin,
        "内建函数与一般函数分开（ObjKind::Builtin）"
    );
    assert_eq!(len.typ, None, "M3 没有内建签名（open_gaps 已登记）");
}

/// **值层类型接线**的核心向量：有类型常量必须带出**声明类型**。
///
/// `const u = uint32(5)`：M3 常量求值已折出值 `5`；本测试钉的是「类型也
/// 一起出来」。漏掉类型 ⇒ `-uint32(5)` 一族的溢出判定没有输入。
#[test]
fn operand_有类型常量_类型随值走() {
    let (tt, pkg) = load("package p\n\nconst u = uint32(5)\nconst s = \"x\"\n");
    let u = Operand::from_object(pkg.lookup("u").expect("u")).expect("操作数");
    assert_eq!(u.mode, Mode::Constant);
    assert_eq!(
        basic_of(&tt, u.typ.expect("uint32(5) 是有类型常量")),
        BasicId::Uint32,
        "常量操作数的类型 = 声明类型（不是无类型基本类型）"
    );
    assert_eq!(u.val, Some(Const::Int(BigInt::from_i64(5))));

    let s = Operand::from_object(pkg.lookup("s").expect("s")).expect("操作数");
    assert_eq!(
        basic_of(&tt, s.typ.expect("无类型字符串常量")),
        BasicId::UntypedString
    );
}

/// 包名 / 标签**不产生操作数**（语境消费：`pkg.X` 的选择器底座、`goto` 目标）。
#[test]
fn operand_包名与标签_不是操作数() {
    assert!(Operand::from_object(&Object::new(ObjKind::PkgName, "fmt")).is_none());
    assert!(Operand::from_object(&Object::new(ObjKind::Label, "L")).is_none());
}

/// 构造器向量 ≥12：11 个模式各一枚 + `Func` 对象一枚（官方 `*Func ⇒ value`）。
#[test]
fn operand_构造器_十二向量() {
    let (_tt, pkg) = load("package p\n\nfunc f() {}\n");
    let t = pkg.lookup("f").expect("函数对象").typ.unwrap();
    let vectors = [
        (Operand::invalid(), Mode::Invalid),
        (Operand::no_value(), Mode::NoValue),
        (Operand::builtin(), Mode::Builtin),
        (Operand::type_expr(t), Mode::TypeExpr),
        (Operand::constant(t, Const::Bool(true)), Mode::Constant),
        (Operand::variable(Some(t)), Mode::Variable),
        (Operand::map_index(t), Mode::MapIndex),
        (Operand::value(Some(t)), Mode::Value),
        (Operand::nil_value(t), Mode::NilValue),
        (Operand::comma_ok(t), Mode::CommaOk),
        (Operand::comma_err(t), Mode::CommaErr),
        (
            Operand::from_object(pkg.lookup("f").expect("f")).expect("函数 ⇒ 值"),
            Mode::Value,
        ),
    ];
    assert_eq!(vectors.len(), 12);
    for (op, want) in vectors {
        assert_eq!(op.mode, want, "构造器与模式必须一一对应：{op:?}");
    }
}
