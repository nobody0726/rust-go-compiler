//! 声明收集 / 求值顺序 / 声明环（M3/Task 24）。
//!
//! 判据来源：`docs/milestones/M3-plan.md` Task 24（前向引用、常量组依赖）、
//! `M3-tests.md` `T-TY-83`（拒绝侧双向）、`T-TY-67`（包级 `const` 跨文件引用）、
//! `T-TY-47`（溢出不截断）、`T-TY-29`（实例化退化）、`T-TY-71..75` 的**接线**。
//!
//! # 这个文件为什么这么写
//!
//! 1. **位置断言用「行文本里找子串」**（[`col_of`]）而不是手数列号 —— 手数会在
//!    改 fixture 时静默错位，而位置错的诊断「看起来是对的」。
//! 2. **每条断言都先确认它可能为假**：位置类断言先断言两个候选位置**不相等**
//!    （`Task 21` 的恒真断言教训：报在名字处还是表达式处，只有两者不等才说明问题）。
//! 3. **诊断条数用精确值**（`== 1` 而不是 `>= 1`）：重复报告是本任务最容易犯的错
//!    （环里几个成员各报一条、`validType` 被逐根调用重复报），`>= 1` 抓不到。

use rgoc_const::Const;
use rgoc_lex::{Diag, DiagCode};
use rgoc_load::{Package, load_sources};
use rgoc_types::{BasicId, IfaceTSet, Type, TypeId, TypeTable};

/// `(文件名, 源码)` 对。
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

/// 装载一组内存文件并返回（类型表, 包, 已排序诊断）。
fn load(files: &[(&str, &str)]) -> (TypeTable, Package, Vec<Diag>) {
    let mut tt = TypeTable::new();
    let srcs = src(files);
    let (pkg, bag) = load_sources(&mut tt, &borrowed(&srcs), None);
    let diags = bag.into_sorted();
    (tt, pkg, diags)
}

fn load_lang(files: &[(&str, &str)], lang: Option<&str>) -> (TypeTable, Package, Vec<Diag>) {
    let mut tt = TypeTable::new();
    let srcs = src(files);
    let (pkg, bag) = load_sources(&mut tt, &borrowed(&srcs), lang);
    let diags = bag.into_sorted();
    (tt, pkg, diags)
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

/// 包作用域里一个名字的对象类型。
fn ty(pkg: &Package, name: &str) -> TypeId {
    pkg.lookup(name)
        .and_then(|o| o.typ)
        .unwrap_or_else(|| panic!("{name} 应该有对象类型"))
}

/// 包作用域里一个名字的常量值。
fn val(pkg: &Package, name: &str) -> Option<Const> {
    pkg.lookup(name).and_then(|o| o.val.clone())
}

/// 类型条目是不是某个**基本类型**。
fn basic_of(tt: &TypeTable, t: TypeId) -> BasicId {
    match tt.type_of(t) {
        Type::Basic(b) => *b,
        other => panic!("期望基本类型，实际 {other:?}"),
    }
}

/// 具名类型的 `NamedId`（用于问「它的底层是谁」）。
fn named_id(tt: &TypeTable, t: TypeId) -> rgoc_types::NamedId {
    match tt.type_of(t) {
        Type::Named(n) => *n,
        other => panic!("期望具名类型，实际 {other:?}"),
    }
}

fn int(v: i64) -> Const {
    Const::Int(rgoc_const::BigInt::from_i64(v))
}

/// 诊断代号集合（便于 `assert_eq` 精确比对**条数与次序**）。
fn codes(diags: &[Diag]) -> Vec<DiagCode> {
    diags.iter().map(|d| d.code).collect()
}

const DIAGS: &str = "诊断";

// ───────────────────────── 前向引用 ─────────────────────────

/// `T-TY-67`：`type A B` 写在 `type B int` **之前** 也合法（先收名、后求值）。
#[test]
fn t_ty_67_类型前向引用() {
    let (tt, pkg, diags) = load(&[("a.go", "package p\n\ntype A B\n\ntype B int\n")]);
    assert!(diags.is_empty(), "{DIAGS}：{diags:?}");

    let a = ty(&pkg, "A");
    let b = ty(&pkg, "B");
    let int_ty = ty(&pkg, "int");

    // A 的底层**就是** B 的同一个 `TypeId`（不是「另一个也叫 B 的具名类型」）
    assert_eq!(
        tt.named_underlying(named_id(&tt, a)),
        b,
        "A 的底层必须与 B 是同一个类型条目"
    );
    // B 的底层是预声明 int（universe 里那个）
    assert_eq!(tt.named_underlying(named_id(&tt, b)), int_ty);
    assert_eq!(basic_of(&tt, int_ty), BasicId::Int);
}

/// `T-TY-67`：常量跨文件前向引用（`A = B + 1` 在 `a.go`，`B` 在 `b.go`）。
#[test]
fn t_ty_67_常量跨文件前向引用() {
    let (tt, pkg, diags) = load(&[
        ("a.go", "package p\n\nconst A = B + 1\n"),
        ("b.go", "package p\n\nconst B = 2\n"),
    ]);
    assert!(diags.is_empty(), "{DIAGS}：{diags:?}");
    assert_eq!(val(&pkg, "B"), Some(int(2)));
    assert_eq!(val(&pkg, "A"), Some(int(3)), "A = B + 1 = 3");
    // 无类型常量：对象类型是**无类型**基本类型（不是 int）
    assert_eq!(basic_of(&tt, ty(&pkg, "A")), BasicId::UntypedInt);
}

/// `T-TY-39` + `T-TY-67`：常量组的 `iota` 与「省略初始化式 = 重复上一条表达式」。
#[test]
fn t_ty_39_常量组的_iota_与省略() {
    let (_, pkg, diags) = load(&[("a.go", "package p\n\nconst (\n\tA = iota\n\tB\n\tC\n)\n")]);
    assert!(diags.is_empty(), "{DIAGS}：{diags:?}");
    assert_eq!(val(&pkg, "A"), Some(int(0)));
    assert_eq!(val(&pkg, "B"), Some(int(1)), "省略初始化式 ⇒ 重复 `iota`");
    assert_eq!(val(&pkg, "C"), Some(int(2)));
}

// ───────────────────── T-TY-83：拒绝侧双向 ─────────────────────

/// `T-TY-83` **正向**：需要类型信息才能判的初始化式 ⇒ 稳定拒绝码。
///
/// 位置判据：官方报在**值表达式**上（`H` 起），不是声明名（`I`）上 ——
/// 所以先断言这两个列号**不相等**，否则下面的断言恒真。
#[test]
fn t_ty_83_需类型信息_稳定拒绝码() {
    let source = "package p\n\nvar H int\n\nconst I = H + 1\n";
    let (tt, pkg, diags) = load(&[("a.go", source)]);

    assert_eq!(codes(&diags), vec![DiagCode::TypInvalidConstInit]);
    let expr_col = col_of(source, 5, "H");
    let name_col = col_of(source, 5, "I");
    assert_ne!(expr_col, name_col, "两个候选位置必须不同，否则断言恒真");
    assert_eq!(diags[0].pos.line(), 5);
    assert_eq!(diags[0].pos.col(), expr_col, "报在**值表达式**处");
    assert!(
        diags[0].message.contains("not constant"),
        "消息形状：{}",
        diags[0].message
    );
    // 官方把拒绝掉的常量置为 invalid / 无值（不是「猜一个值」）
    assert!(val(&pkg, "I").is_none(), "拒绝的常量不能有值");
    assert_eq!(tt.type_of(ty(&pkg, "I")), &Type::Invalid);
}

/// `T-TY-83` **反向**：常量子集内的初始化式 ⇒ 一个诊断都不许报（缺这一边不算过）。
#[test]
fn t_ty_83_常量子集不报错() {
    let (_, pkg, diags) = load(&[(
        "a.go",
        "package p\n\n\
         const (\n\
         \ti1 = 1 + 2\n\
         \tr1 = 'a'\n\
         \tr2 = 1 + 'a'\n\
         \tf1 = 1.0 + 2\n\
         \tc1 = 1.0 + 2i\n\
         \ts1 = \"a\" + \"b\"\n\
         \tb1 = true && false\n\
         \tsh = 1 << 3\n\
         \tbig = 1 << 100\n\
         \tneg = -1 << 2\n\
         \tblank = 7\n\
         )\n\
         \nconst _ = 9\n",
    )]);
    assert!(diags.is_empty(), "{DIAGS}：{diags:?}");
    assert_eq!(val(&pkg, "i1"), Some(int(3)));
    assert_eq!(val(&pkg, "r1"), Some(int(97)));
    assert_eq!(val(&pkg, "r2"), Some(int(98)));
    assert_eq!(val(&pkg, "s1"), Some(Const::Str(b"ab".to_vec())));
    assert_eq!(val(&pkg, "b1"), Some(Const::Bool(false)));
    assert_eq!(val(&pkg, "neg"), Some(int(-4)), "修订 R1：左操作数可以负");
    // 空标识符**不进**作用域（`SCP`），所以查不到也不要紧 —— 这里只确认没报错
}

/// 无类型常量的**种类**（`T-TY-3b` 的邻域）：种类由**操作数**决定，不由值决定。
#[test]
fn 无类型种类按操作数合并() {
    let (tt, pkg, diags) = load(&[(
        "a.go",
        "package p\n\n\
         const i1 = 1 + 2\n\
         const r1 = 'a'\n\
         const r2 = 1 + 'a'\n\
         const f1 = 1.0 + 2\n\
         const c1 = 1.0 + 2i\n\
         const s1 = \"a\" + \"b\"\n\
         const b1 = true && false\n\
         const sh = 1 << 3\n\
         const k1 = int8(1) + 1\n\
         const k2 = string(65)\n",
    )]);
    assert!(diags.is_empty(), "{DIAGS}：{diags:?}");
    let kinds = |n: &str| basic_of(&tt, ty(&pkg, n));
    assert_eq!(kinds("i1"), BasicId::UntypedInt);
    assert_eq!(kinds("r1"), BasicId::UntypedRune, "`'a'` 是 untyped rune");
    assert_eq!(kinds("r2"), BasicId::UntypedRune, "rune 比 int 宽");
    assert_eq!(kinds("f1"), BasicId::UntypedFloat);
    assert_eq!(kinds("c1"), BasicId::UntypedComplex);
    assert_eq!(kinds("s1"), BasicId::UntypedString);
    assert_eq!(
        kinds("b1"),
        BasicId::UntypedBool,
        "比较/逻辑 ⇒ untyped bool"
    );
    assert_eq!(kinds("sh"), BasicId::UntypedInt, "移位取**左**操作数的种类");
    // 转换 ⇒ **类型化**常量（实测：`int8(1) + 1` 是 int8）
    assert_eq!(kinds("k1"), BasicId::Int8);
    assert_eq!(kinds("k2"), BasicId::String);
}

// ───────────────────── T-TY-47：溢出不截断 ─────────────────────

/// `T-TY-47`：不可表示 ⇒ 稳定诊断码，且**不**把值截断成近似值。
///
/// 三个位置都在**值表达式**上（官方亦然），可表示性要**穿过具名类型的本体**
/// （`const X MyInt = 1.5` 报的是 `truncated`）。
#[test]
fn t_ty_47_溢出不截断() {
    let source = "package p\n\n\
                  type MyInt int\n\
                  \n\
                  const E int8 = 200\n\
                  \n\
                  const D float32 = 1e400\n\
                  \n\
                  const X MyInt = 1.5\n";
    let (tt, pkg, diags) = load(&[("a.go", source)]);

    assert_eq!(
        codes(&diags),
        vec![
            DiagCode::TypNumericOverflow,
            DiagCode::TypNumericOverflow,
            DiagCode::TypTruncatedFloat,
        ],
        "{DIAGS}：{diags:?}"
    );
    let e_col = col_of(source, 5, "200");
    let d_col = col_of(source, 7, "1e400");
    let x_col = col_of(source, 9, "1.5");
    let e_name = col_of(source, 5, "E");
    assert_ne!(e_col, e_name, "值列必须与名字列不同，否则断言恒真");
    assert_eq!(diags[0].pos.col(), e_col);
    assert_eq!(diags[1].pos.col(), d_col);
    assert_eq!(diags[2].pos.col(), x_col);
    assert!(
        diags[0].message.contains("constant declaration"),
        "消息形状：{}",
        diags[0].message
    );
    // 声明类型**保留**（官方：对象类型仍是被声明的类型），值**没有**
    // 且绝不是「截断后的近似值」（`T-TY-47` 的另一半）
    assert_eq!(basic_of(&tt, ty(&pkg, "E")), BasicId::Int8);
    assert_eq!(val(&pkg, "E"), None, "不截断：值必须是 None");
    assert_eq!(val(&pkg, "D"), None);
    assert_eq!(val(&pkg, "X"), None);
}

// ───────────────────────── 声明环 ─────────────────────────

/// 别名环（`type A = B; type B = A`）⇒ `TypInvalidDeclCycle`，**只报一次**。
///
/// 官方口径（探针实测）：报在**源码序最先**的成员上（A）；环上另一个成员 B
/// 保留为别名（`rhs` = invalid），所以两侧 dump 都能走。
#[test]
fn 别名环_只报一次() {
    let source = "package p\n\ntype A = B\n\ntype B = A\n";
    let (tt, pkg, diags) = load(&[("a.go", source)]);

    assert_eq!(codes(&diags), vec![DiagCode::TypInvalidDeclCycle]);
    assert_eq!(diags[0].pos.line(), 3);
    assert_eq!(diags[0].pos.col(), col_of(source, 3, "A"));
    // 环起点被作废（官方 `cycleError` 把起点对象的类型置为 invalid）
    assert_eq!(tt.type_of(ty(&pkg, "A")), &Type::Invalid);
    // 另一个成员**保留**为别名，`rhs` 指向 invalid
    let b = ty(&pkg, "B");
    let Type::Alias(bid) = tt.type_of(b) else {
        panic!("B 应该仍是别名，实际 {:?}", tt.type_of(b));
    };
    assert_eq!(
        tt.type_of(tt.alias_def(*bid).rhs.expect("别名本体不能为空")),
        &Type::Invalid
    );
}

/// 具名类型环（`type A B; type B A`）⇒ **本层不报**，由 `valid_type` 报**一条**。
///
/// 这条是「照官方 `cycle()` 分类」的核心证据：`nval == 0 && ndef > 0` 的环
/// 必须被**允许**走到 `validType`；如果本层抢先报 `TypInvalidDeclCycle`，
/// 或者两侧各报一条 `TypInvalidRecursiveType`，这条断言就红。
#[test]
fn 具名类型环_交给_valid_type_且只一条() {
    let source = "package p\n\ntype A B\n\ntype B A\n";
    let (tt, pkg, diags) = load(&[("a.go", source)]);

    assert_eq!(codes(&diags), vec![DiagCode::TypInvalidRecursiveType]);
    assert_eq!(diags[0].pos.line(), 3, "报在源码序最前的成员上");
    assert_eq!(diags[0].pos.col(), col_of(source, 3, "A"));
    // 两个具名类型的本体都**填好**了（`validType` 的前置条件：不许留空）
    let a = ty(&pkg, "A");
    let b = ty(&pkg, "B");
    assert_eq!(tt.named_underlying(named_id(&tt, a)), b, "A 的底层是 B");
    assert_eq!(tt.named_underlying(named_id(&tt, b)), a, "B 的底层是 A");
}

/// `type A [10]A`：环经数组 ⇒ 非法（`validType` 的「按值包含」），**一条**。
#[test]
fn 数组自引用环_一条() {
    let (_, _, diags) = load(&[("a.go", "package p\n\ntype A [10]A\n")]);
    assert_eq!(codes(&diags), vec![DiagCode::TypInvalidRecursiveType]);
    assert_eq!(diags[0].pos.line(), 3);
}

/// 常量环（`const A = B; const B = A`）⇒ `TypInvalidInitCycle`，只报一次。
#[test]
fn 常量环_只报一次() {
    let source = "package p\n\nconst A = B\n\nconst B = A\n";
    let (tt, pkg, diags) = load(&[("a.go", source)]);

    assert_eq!(codes(&diags), vec![DiagCode::TypInvalidInitCycle]);
    assert_eq!(diags[0].pos.line(), 3);
    assert_eq!(diags[0].pos.col(), col_of(source, 3, "A"));
    assert!(diags[0].message.contains("initialization cycle"));
    // 环上每个值都作废（既没有类型也没有值）
    for n in ["A", "B"] {
        assert!(val(&pkg, n).is_none(), "{n} 不该有值");
        assert_eq!(tt.type_of(ty(&pkg, n)), &Type::Invalid, "{n} 该是 invalid");
    }
}

// ───────────────────────── 名字 ↔ 值 个数 ─────────────────────────

/// `const` 的 `WrongAssignCount`（官方 `arityMatch`，`resolver.go:55`）。
#[test]
fn const_名字多于值_与_值多于名字() {
    let source = "package p\n\nconst a, b = 1\n";
    let (_, pkg, diags) = load(&[("a.go", source)]);
    assert_eq!(codes(&diags), vec![DiagCode::TypWrongAssignCount]);
    assert_eq!(
        diags[0].pos.col(),
        col_of(source, 3, "b"),
        "报在**缺初始化式**的那个名字上"
    );
    assert!(diags[0].message.contains("missing init expr for b"));
    assert_eq!(val(&pkg, "a"), Some(int(1)));
    assert!(val(&pkg, "b").is_none());

    // 反向：值多于名字 ⇒ 报在**多余的那个表达式**上
    let source2 = "package p\n\nconst c = 1, 2\n";
    let (_, pkg2, diags2) = load(&[("a.go", source2)]);
    assert_eq!(codes(&diags2), vec![DiagCode::TypWrongAssignCount]);
    assert_eq!(diags2[0].pos.col(), col_of(source2, 3, "2"));
    assert!(diags2[0].message.contains("extra init expr 2"));
    assert_eq!(val(&pkg2, "c"), Some(int(1)), "第一个值照样配对");
}

// ───────────────────────── 根序与版本接线 ─────────────────────────

/// `C7` §3 的根序：`var`/`const` 只在**写出类型**时才有根；`func` 的签名有根；
/// 类型声明以自身为根。顺序 = 文件序 ⊃ 声明序。
#[test]
fn dump_根序按声明序() {
    let (tt, pkg, diags) = load(&[(
        "a.go",
        "package p\n\ntype T int\n\nvar v1 T\n\nvar v2 = 1\n\nconst c int = 2\n\nfunc f(x T) T { return x }\n",
    )]);
    assert!(diags.is_empty(), "{DIAGS}：{diags:?}");
    // 5 个根：T / var v1 的 T / const c 的 int / func f 的签名 = 4 个
    // （`var v2 = 1` 没有写出的类型 ⇒ **没有**根）
    assert_eq!(pkg.type_roots.len(), 4, "根：{:?}", pkg.type_roots);
    // 第一个根是 `T` 自身（具名类型）
    let Type::Named(_) = tt.type_of(pkg.type_roots[0]) else {
        panic!("第一个根该是具名类型 T");
    };
    // dump 可用且幂等（`T-TY-19` 的轻量侧证）
    let d1 = rgoc_types::dump_types(&tt, &pkg.type_roots);
    let d2 = rgoc_types::dump_types(&tt, &pkg.type_roots);
    assert!(!d1.is_empty());
    assert_eq!(d1, d2);
}

/// `T-TY-71..75` 的**接线**：装载时算好每个文件的生效版本（机制在 Task 21）。
#[test]
fn 版本表按文件接线() {
    let (_, pkg, diags) = load(&[
        ("a.go", "//go:build go1.22\n\npackage p\n\nconst A = 1\n"),
        ("b.go", "package p\n\nconst B = 2\n"),
    ]);
    assert!(diags.is_empty(), "{DIAGS}：{diags:?}");
    assert_eq!(pkg.versions.len(), 2, "两个文件都要有版本项");
    let fa = pkg.files[0];
    let fb = pkg.files[1];
    // 写明了 `//go:build go1.22` 的文件：**门控**（≤ 22 允许，> 22 不允许）
    assert!(
        pkg.versions
            .allow(fa, rgoc_types::GoVersion::parse_lang("go1.22").unwrap())
    );
    assert!(
        !pkg.versions
            .allow(fa, rgoc_types::GoVersion::parse_lang("go1.23").unwrap())
    );
    // 没写版本的文件：**不门控** ⇒ 一律允许（`Versions` 的 `None` 语义）
    assert!(
        pkg.versions
            .allow(fb, rgoc_types::GoVersion::parse_lang("go1.99").unwrap())
    );
}

/// `T-TY-74`：包级 `TooNew` 也要在装载路径上真的报出来（接线，不是机制）。
#[test]
fn 包级_too_new_在装载路径上报出() {
    let (_, _, diags) = load_lang(&[("a.go", "package p\n\nconst A = 1\n")], Some("go1.99"));
    assert_eq!(codes(&diags), vec![DiagCode::TypPackageRequiresNewerGo]);
}

/// `T-TY-29`：实例化 `X[A]` 在 M3 退化为 `X` 自身（边界由 `M3-design.md` §5.2 钉住）。
#[test]
fn t_ty_29_实例化退化为同一_id() {
    let (tt, pkg, diags) = load(&[(
        "a.go",
        "package p\n\ntype G[T any] struct{ x T }\n\ntype H = G[int]\n",
    )]);
    assert!(diags.is_empty(), "{DIAGS}：{diags:?}");
    // `H` 是**别名** ⇒ 对象类型是别名本身，退化体现它的 `rhs` 上
    let h = ty(&pkg, "H");
    let Type::Alias(hid) = tt.type_of(h) else {
        panic!("H 该是别名，实际 {:?}", tt.type_of(h));
    };
    assert_eq!(
        tt.alias_def(*hid).rhs.expect("别名本体不能为空"),
        ty(&pkg, "G"),
        "`G[int]` 退化为 `G` 的同一个类型条目（M3 无实例化）"
    );
    assert!(matches!(tt.type_of(ty(&pkg, "G")), Type::Named(_)));
}

/// 两个具名类型互相引用（**合法**的那种：经指针/切片等间接边）不该报错。
///
/// 与 [`具名类型环_交给_valid_type_且只一条`] 成对：那条证明「按值包含的环会报」，
/// 这条证明「间接环不报」—— 只测一边就可能把判据写成「见到环就报」。
#[test]
fn 间接自引用_合法不报() {
    let (tt, pkg, diags) = load(&[(
        "a.go",
        "package p\n\ntype Node struct {\n\tnext *Node\n\tkids []Node\n}\n",
    )]);
    assert!(diags.is_empty(), "{DIAGS}：{diags:?}");
    let n = ty(&pkg, "Node");
    let Type::Named(nid) = tt.type_of(n) else {
        panic!("Node 该是具名类型");
    };
    let u = tt.named_underlying(*nid);
    assert!(matches!(tt.type_of(u), Type::Struct(_)), "底层是结构体");
}

/// 具名类型的**本体**（`type D <T>` 的 `<T>`）。
fn underlying(tt: &TypeTable, t: TypeId) -> TypeId {
    match tt.type_of(t) {
        Type::Named(n) => tt.named_underlying(*n),
        other => panic!("期望具名类型，实际 {other:?}"),
    }
}

/// 结构体：字段名 / **标签内容** / **嵌入标记** / **声明序**（`C7` §2.3 + §3）。
///
/// 三个判据都**不是**「有名字/没名字」那么简单：
/// * 嵌入字段（`*T`）的 AST 里 `name == None` ⇒ 名字位 `None`、`embedded = true`；
/// * 空标识符字段（`_ int`）**有**名字（`_`），**不是**嵌入；
/// * 标签存的是**字面量内容**（反引号已剥掉），dump 侧才会重新加引号。
#[test]
fn struct_字段名_标签_嵌入标记_声明序() {
    let (tt, pkg, diags) = load(&[(
        "a.go",
        "package p\n\n\
         type T struct{}\n\
         \n\
         type S struct {\n\
         \tA int\n\
         \tB, C string\n\
         \t_ int\n\
         \t*T\n\
         \tTagged int `json:\"t\"`\n\
         }\n",
    )]);
    assert!(diags.is_empty(), "{DIAGS}：{diags:?}");
    let Type::Struct(sid) = tt.type_of(underlying(&tt, ty(&pkg, "S"))) else {
        panic!("S 的底层该是结构体");
    };
    let fields = &tt.struct_def(*sid).fields;
    let names: Vec<Option<&str>> = fields.iter().map(|f| f.name.as_deref()).collect();
    let embedded: Vec<bool> = fields.iter().map(|f| f.embedded).collect();
    assert_eq!(
        names,
        vec![
            Some("A"),
            Some("B"),
            Some("C"),
            Some("_"),
            None, // 嵌入字段：名字位是 `-`
            Some("Tagged"),
        ],
        "字段名与**声明序**"
    );
    assert_eq!(
        embedded,
        vec![false, false, false, false, true, false],
        "只有 `*T` 是嵌入；`_ int` 有名字 ⇒ 不是嵌入"
    );
    assert_eq!(fields[4].tag, None);
    assert_eq!(fields[5].tag, Some("json:\"t\"".to_string()), "标签存内容");
    // `*T` 的类型是指针，指向 T 自己那个具名条目
    let Type::Pointer(elem) = tt.type_of(fields[4].typ) else {
        panic!("嵌入字段该是指针类型，实际 {:?}", tt.type_of(fields[4].typ));
    };
    assert_eq!(*elem, ty(&pkg, "T"));
}

/// 接口：方法集**按名字升序**（`C7` §4），嵌入的具名接口**合并**进来。
///
/// 升序是**契约位**：源码里写的是 `B(); A()`，若按声明序存，dump 与 oracle
/// 的 `interface` 行就会逐字节不同。
#[test]
fn interface_方法集排序与嵌入合并() {
    let (tt, pkg, diags) = load(&[(
        "a.go",
        "package p\n\n\
         type I1 interface {\n\
         \tB() int\n\
         \tA()\n\
         }\n\
         \n\
         type I2 interface {\n\
         \tI1\n\
         \tM(x int) (y string)\n\
         }\n",
    )]);
    assert!(diags.is_empty(), "{DIAGS}：{diags:?}");
    let names_of = |name: &str| -> Vec<String> {
        let Type::Interface(iid) = tt.type_of(underlying(&tt, ty(&pkg, name))) else {
            panic!("{name} 的底层该是接口");
        };
        tt.interface_def(*iid)
            .methods
            .iter()
            .map(|(n, _)| n.clone())
            .collect()
    };
    assert_eq!(names_of("I1"), vec!["A", "B"], "按名字升序，不是声明序");
    assert_eq!(names_of("I2"), vec!["A", "B", "M"], "嵌入的方法被合并");
}

/// 接口：**项集**（`~T` / 并集）与预声明的 `comparable`（`F5`/`F8`）。
///
/// M3 **表示**它们（`T-TY-17`/`T-TY-18` 只 dump 不判同一性，`D-M3-13`）。
#[test]
fn interface_项集与_comparable() {
    let (tt, pkg, diags) = load(&[(
        "a.go",
        "package p\n\n\
         type Num interface {\n\
         \t~int | ~int32\n\
         }\n\
         \n\
         type Cmp interface {\n\
         \tcomparable\n\
         }\n",
    )]);
    assert!(diags.is_empty(), "{DIAGS}：{diags:?}");

    let Type::Interface(num) = tt.type_of(underlying(&tt, ty(&pkg, "Num"))) else {
        panic!("Num 该是接口");
    };
    let d = tt.interface_def(*num);
    assert!(d.methods.is_empty(), "约束接口没有方法");
    let IfaceTSet::Terms(ts) = &d.tset else {
        panic!("`~int; ~int32` 的语义类型集该是 Terms，实际 {:?}", d.tset);
    };
    assert_eq!(ts.len(), 2);
    assert!(ts.iter().all(|(tilde, _)| *tilde), "两个项都带 `~`");
    assert_eq!(basic_of(&tt, ts[0].1), BasicId::Int);
    assert_eq!(basic_of(&tt, ts[1].1), BasicId::Int32);
    let terms = d.terms.expect("`~int | ~int32` 必须落成项集");
    let Type::Union(uid) = tt.type_of(terms) else {
        panic!("项集该是 union，实际 {:?}", tt.type_of(terms));
    };
    let t = &tt.union_def(*uid).terms;
    assert_eq!(t.len(), 2);
    assert!(t.iter().all(|(tilde, _)| *tilde), "两个项都带 `~`");
    assert_eq!(basic_of(&tt, t[0].1), BasicId::Int);
    assert_eq!(basic_of(&tt, t[1].1), BasicId::Int32);

    let Type::Interface(cmp) = tt.type_of(underlying(&tt, ty(&pkg, "Cmp"))) else {
        panic!("Cmp 该是接口");
    };
    let d = tt.interface_def(*cmp);
    assert_eq!(d.tset, IfaceTSet::Comparable, "`comparable` 必须落到类型集");
    // 语法面（Task 8 起）：嵌入 `comparable` 落 **exact 项** —— oracle
    // `synthUnion` 实测口径（`interface{ comparable }` ⇒ `interface 0 1` +
    // `term exact`，不是「无项线」）。
    let terms = d.terms.expect("`comparable` 嵌入必须落成语法项");
    let Type::Union(uid) = tt.type_of(terms) else {
        panic!("项集该是 union，实际 {:?}", tt.type_of(terms));
    };
    let ct = &tt.union_def(*uid).terms;
    assert_eq!(ct.len(), 1);
    assert!(!ct[0].0, "exact 项（不带 `~`）");
    assert!(d.methods.is_empty());
}

/// 签名：变参位 / 参数与结果元组 / 无名参数 / 方法的接收者。
#[test]
fn 签名_变参_无名参数_接收者() {
    let (tt, pkg, diags) = load(&[(
        "a.go",
        "package p\n\n\
         type T struct{}\n\
         \n\
         func f(a, b int, _ string, c ...int) (int, error) { return 0, nil }\n\
         \n\
         func (r T) M(x int) {}\n",
    )]);
    assert!(diags.is_empty(), "{DIAGS}：{diags:?}");

    let Type::Signature(fid) = tt.type_of(ty(&pkg, "f")) else {
        panic!("f 的类型该是签名，实际 {:?}", tt.type_of(ty(&pkg, "f")));
    };
    let d = tt.signature_def(*fid);
    assert!(d.variadic, "末参 `...int` ⇒ 变参位");
    assert!(d.recv.is_none(), "函数不是方法");
    let params: Vec<(Option<String>, TypeId)> = tt
        .tuple_def(match tt.type_of(d.params) {
            Type::Tuple(t) => *t,
            other => panic!("参数该是元组，实际 {other:?}"),
        })
        .vars
        .clone();
    assert_eq!(
        params.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
        vec![
            Some("a".into()),
            Some("b".into()),
            // 写出来的空标识符参数**保留**（它不是「没写名字」）
            Some("_".into()),
            Some("c".into()),
        ]
    );
    assert_eq!(basic_of(&tt, params[0].1), BasicId::Int);
    assert_eq!(basic_of(&tt, params[1].1), BasicId::Int);
    assert_eq!(basic_of(&tt, params[2].1), BasicId::String);
    // ⚠️ 变参参数的类型是 **`[]int` 切片**，不是元素类型 `int`。
    // 这是 Task 33 对撞**订正**的口径：go/types 的 `Signature.Params()` 末位
    // 就是切片（`variadic` 位只表示「可这样调用」）。写成 `int` 会让两侧
    // 编号从那一行起整体错位（`type-dump-func.go`）。
    let Type::Slice(elem) = tt.type_of(params[3].1) else {
        panic!("变参参数该是切片，实际 {:?}", tt.type_of(params[3].1));
    };
    assert_eq!(basic_of(&tt, *elem), BasicId::Int);
    // 结果全无名 ⇒ 名字位 `None`（dump 写 `-`），`error` 是 universe 里的具名类型
    let Type::Tuple(rid) = tt.type_of(d.results) else {
        panic!("结果该是元组");
    };
    let res = &tt.tuple_def(*rid).vars;
    assert_eq!(res.len(), 2);
    assert_eq!(
        res.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
        vec![None, None]
    );
    assert!(
        matches!(tt.type_of(res[1].1), Type::Named(_)),
        "error 是具名类型"
    );

    // 方法：名字不进包作用域，但它的签名在 **dump 根**里（且带接收者）
    assert!(pkg.lookup("M").is_none());
    let method = pkg
        .type_roots
        .iter()
        .find_map(|t| match tt.type_of(*t) {
            Type::Signature(id) if tt.signature_def(*id).recv.is_some() => Some(*id),
            _ => None,
        })
        .expect("方法签名必须是一个 dump 根");
    assert_eq!(tt.signature_def(method).recv, Some(ty(&pkg, "T")));
}

/// 类型参数：**两遍**声明（先占号、后回填约束），并作为**对象**可被本体引用。
#[test]
fn 类型参数_对象可被本体引用() {
    let (tt, pkg, diags) = load(&[(
        "a.go",
        "package p\n\n\
         type G[T any, U ~[]T] struct {\n\
         \tx T\n\
         \ty U\n\
         }\n",
    )]);
    assert!(diags.is_empty(), "{DIAGS}：{diags:?}");
    let Type::Struct(sid) = tt.type_of(underlying(&tt, ty(&pkg, "G"))) else {
        panic!("G 的底层该是结构体");
    };
    let fields = &tt.struct_def(*sid).fields;
    let tp = |i: usize| match tt.type_of(fields[i].typ) {
        Type::TypeParam(id) => {
            let d = tt.type_param_def(*id);
            (d.index, d.constraint)
        }
        other => panic!(
            "字段 {} 该是类型参数，实际 {other:?}",
            fields[i].name.clone().unwrap_or_default()
        ),
    };
    let (i0, c0) = tp(0);
    let (i1, c1) = tp(1);
    assert_eq!((i0, i1), (0, 1), "序号按声明序");
    // `T any` 的约束位置上就是 **`any` 那个别名条目本身**（不是它的本体）。
    // 实测 oracle：`typeparam 0 #<alias 条目>`（`C7` §4.2 的注已钉这条）。
    assert_eq!(c0, ty(&pkg, "any"), "约束 = `any` 的别名条目");
    assert!(matches!(tt.type_of(c0), Type::Alias(_)));
    assert!(matches!(
        tt.type_of(unalias_all(&tt, c0)),
        Type::Interface(_)
    ));
    // `U ~[]T` 的约束是「只有一个 `~[]T` 项」的接口 —— 且 `T` 必须是**同一个**
    // 类型参数（不是「另一个叫 T 的东西」）
    let Type::Interface(uid) = tt.type_of(c1) else {
        panic!("U 的约束该是接口，实际 {:?}", tt.type_of(c1));
    };
    let terms = tt.interface_def(*uid).terms.expect("`~[]T` 该落成项集");
    let Type::Union(un) = tt.type_of(terms) else {
        panic!("项集该是 union");
    };
    let t = &tt.union_def(*un).terms;
    assert_eq!(t.len(), 1);
    assert!(t[0].0, "带 `~`");
    let Type::Slice(elem) = tt.type_of(t[0].1) else {
        panic!("项该是切片，实际 {:?}", tt.type_of(t[0].1));
    };
    assert_eq!(
        *elem, fields[0].typ,
        "`[]T` 的元素必须与字段 `x T` 是同一个类型参数条目"
    );
}

/// 沿别名一路走到头的类型（`any` → `interface{}`）。
fn unalias_all(tt: &TypeTable, mut t: TypeId) -> TypeId {
    while let Type::Alias(a) = tt.type_of(t) {
        match tt.alias_def(*a).rhs {
            Some(r) => t = r,
            None => break,
        }
    }
    t
}
