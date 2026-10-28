//! 单态化的**实例命名**与**请求队列**（M12/Task 13，`T-GEN-04`）。
//!
//! 判据来源：`docs/milestones/M12-plan.md` Task 13。
//!
//! # 为什么「命名」必须单独当判据
//!
//! 实例名是**同一性**的载体。两个不同实参若撞名（`Sum·int` 与 `Sum·string`
//! 都叫 `Sum`），后者的体会覆盖前者，调用点全部调到同一个函数 —— 而编译器
//! **一条错都不会报**。这是本仓反复记的「静默失败」形状，所以判据不能只判
//! 「非空 / 不 panic」，要判四件事：
//!
//! 1. **拼写**：`Sum·int`（`·` = U+00B7，与 Go 的限定名 `pkg.T` 同一个观感）；
//! 2. **不同实参不同名** —— `M-M12-02` 的靶子（把 `mangled` 退化成只拼
//!    `origin_name`，这条必须红）；
//! 3. **名字里没有 `_` / `$`**：`_` 与用户符号的「空标识符」约定混淆，
//!    `$` 是汇编器保留字符。⚠️ 这条**不是**恒真的：`BasicId::as_str()` 对
//!    `unsafe.Pointer` 返回 `unsafe_pointer`，漏了消掉就直接漏进实例名；
//! 4. **零大小类型实参**（`struct{}`）也有唯一、可读、**结构性**的名字 ——
//!    「结构性」是硬要求：`add_struct` 不去重，每个调用点重新解析一次
//!    `struct{}` 就得到一个新的 `StructId`；若名字跟着 `StructId` 走，
//!    `request` 的 memo（按 `identical` 比）虽然命中，**名字却不同** ⇒
//!    不动点（Task 14）每轮都在造新实例。
//!
//! # 队列的判据
//!
//! `request` 的返回值就是「这次要不要新造一个实例」，它是 Task 14 那个
//! `while let Some(req) = mono.take()` 的唯一终止条件 —— 所以「第二次同参数
//! 返回 `false`」与「别名实参命中同一份 memo」都必须钉住。

use rgoc_load::mono::{FuncKey, Mono, mangled};
use rgoc_types::{BasicId, Field, TypeId, TypeTable};

/// `struct{}` —— 零大小类型实参。
fn empty_struct(tt: &mut TypeTable) -> TypeId {
    let id = tt.add_struct(Vec::new());
    tt.struct_type(id)
}

/// `struct{a struct{}}` —— 另一个零大小类型（含一个零大小的嵌入字段）。
fn zero_struct_with_field(tt: &mut TypeTable) -> TypeId {
    let inner = empty_struct(tt);
    let id = tt.add_struct(vec![Field {
        name: Some("a".to_string()),
        typ: inner,
        tag: None,
        embedded: false,
    }]);
    tt.struct_type(id)
}

// ── `T-GEN-04` 命名 ──────────────────────────────────────────────────────

/// 拼写：`Sum·int` / `Box·string`；不同实参不同名；没有 `_` / `$`。
#[test]
fn t_gen_04_实例名() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);
    let sum_named = tt.add_named("Sum");
    let sum = tt.named_type(sum_named);
    let boxed_named = tt.add_named("Box");
    let boxed = tt.named_type(boxed_named);

    // ① 拼写（`·` 是 U+00B7，不是 `.`、也不是 `-`）
    assert_eq!(mangled(&tt, "Sum", &[int]), "Sum\u{00b7}int");
    assert_eq!(mangled(&tt, "Box", &[string]), "Box\u{00b7}string");
    // 多实参按声明序拼接
    assert_eq!(
        mangled(&tt, "Pair", &[int, string]),
        "Pair\u{00b7}int\u{00b7}string"
    );

    // ② 不同实参不同名（`M-M12-02`：只拼 origin_name 会在这里红）
    let a = mangled(&tt, "Sum", &[int]);
    let b = mangled(&tt, "Sum", &[string]);
    assert_ne!(a, b, "两个不同实参撞名 ⇒ 实例互相覆盖，且一条诊断都没有");

    // ③ 名字里没有 `_` / `$`
    let cases: [(&str, TypeId); 4] = [
        ("basic", int),
        ("named", sum),
        ("named", boxed),
        ("string", string),
    ];
    for (what, t) in cases {
        let n = mangled(&tt, "F", &[t]);
        assert!(!n.contains('_'), "{what} 类型的实例名里有 `_`：{n}");
        assert!(!n.contains('$'), "{what} 类型的实例名里有 `$`：{n}");
    }
    // ⚠️ 这一条有牙：`BasicId::UnsafePointer.as_str()` 是 `unsafe_pointer`
    let up = tt.add_basic(BasicId::UnsafePointer);
    let n = mangled(&tt, "F", &[up]);
    assert!(!n.contains('_'), "basic 名里的 `_` 漏进了实例名：{n}");
    // 复合类型也不例外
    let ptr = tt.add_pointer(int);
    let slice = tt.add_slice(ptr);
    for t in [ptr, slice] {
        let n = mangled(&tt, "F", &[t]);
        assert!(
            !n.contains('_') && !n.contains('$'),
            "复合类型的实例名不干净：{n}"
        );
    }

    // 没有实参时原样返回（非泛型函数不该被 request，但命名函数自己不能崩）
    assert_eq!(mangled(&tt, "Main", &[]), "Main");
}

/// 零大小类型实参：唯一、可读、**结构性**（同结构 ⇒ 同名）。
#[test]
fn t_gen_13b_零大小类型实参() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let e = empty_struct(&mut tt);
    let z = zero_struct_with_field(&mut tt);

    let ne = mangled(&tt, "F", &[e]);
    // 可读：看得出是结构体
    assert!(
        ne.starts_with("F\u{00b7}struct"),
        "零大小实参的名字不可读：{ne}"
    );
    assert!(!ne.contains('_') && !ne.contains('$'), "名字不干净：{ne}");

    // 唯一：与「别的实参」「没有实参」都不撞
    assert_ne!(
        ne,
        mangled(&tt, "F", &[int]),
        "`struct{{}}` 与 `int` 撞名：{ne}"
    );
    assert_ne!(
        ne,
        mangled(&tt, "F", &[]),
        "`struct{{}}` 与「无实参」撞名：{ne}"
    );
    let nz = mangled(&tt, "F", &[z]);
    assert_ne!(ne, nz, "两个不同的零大小结构体撞名：{ne}");

    // 结构性：**另一个** `struct{}` 条目（`add_struct` 不去重）必须同名。
    // 这一条是 Task 14 不动点能收敛的前提 —— 名字跟着 `StructId` 走就会每轮
    // 造一个新实例。
    let e2 = empty_struct(&mut tt);
    assert_ne!(e, e2, "fixture 没生效：两次 `add_struct` 拿到了同一个条目");
    assert_eq!(
        mangled(&tt, "F", &[e]),
        mangled(&tt, "F", &[e2]),
        "结构相同的空结构体命名不同 ⇒ 每个调用点各造一个实例（不动点不收敛）"
    );
}

// ── 队列 ────────────────────────────────────────────────────────────────

/// `request` / `take`：同一 `(origin, args)` 只入队一次，取出按入队序。
#[test]
fn t_gen_04b_请求队列() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);

    let mut mono = Mono::new();
    let sum: FuncKey = mono.declare("Sum");
    let box_: FuncKey = mono.declare("Box");

    // 首次请求 ⇒ 入队
    assert!(mono.request(&mut tt, sum, &[int]), "首次请求必须入队");
    // 同参数再来 ⇒ memo 命中，不再入队（否则 Task 14 的不动点不终止）
    assert!(!mono.request(&mut tt, sum, &[int]), "同参数重复入队");
    // 不同实参 ⇒ 另一个实例
    assert!(mono.request(&mut tt, sum, &[string]));
    // 不同 origin + 同实参 ⇒ 又是另一个实例（memo 的键必须含 origin）
    assert!(mono.request(&mut tt, box_, &[int]));
    assert_eq!(mono.pending(), 3);

    // FIFO：取出序 = 入队序，名字由 origin 的名字 + 实参拼出
    let r1 = mono.take().expect("还有一个");
    assert_eq!(r1.origin, sum);
    assert_eq!(r1.name, "Sum\u{00b7}int");
    assert_eq!(r1.args, vec![int]);

    let r2 = mono.take().expect("还有两个");
    assert_eq!(r2.origin, sum);
    assert_eq!(r2.name, "Sum\u{00b7}string");

    let r3 = mono.take().expect("还有一个");
    assert_eq!(r3.origin, box_);
    assert_eq!(r3.name, "Box\u{00b7}int");

    assert!(mono.take().is_none(), "队列已空却还能取");
    assert_eq!(mono.pending(), 0);
}

/// 别名实参与底层实参是**同一个**请求（官方：别名不是独立类型）。
///
/// 没有这条，`type MyInt = int` 与 `int` 会各造一份实例 —— 语义一样、代码两份，
/// 而且 `T-GEN-05`（无类型参数残留）查不出这种重复。
#[test]
fn t_gen_04c_别名实参命中同一份_memo() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let alias = tt.add_alias("MyInt");
    tt.set_alias_rhs(alias, int);
    let my_int = tt.alias_type(alias);
    assert_ne!(my_int, int, "fixture 没生效：别名与底层是同一个 TypeId");

    let mut mono = Mono::new();
    let f = mono.declare("F");
    assert!(mono.request(&mut tt, f, &[int]), "首个请求必须入队");
    assert!(
        !mono.request(&mut tt, f, &[my_int]),
        "别名实参没有命中 memo ⇒ 同一类型造了两份实例"
    );
    assert_eq!(mono.pending(), 1);

    // 反过来：先别名、后底层，同样只入队一次
    let mut mono2 = Mono::new();
    let g = mono2.declare("G");
    assert!(mono2.request(&mut tt, g, &[my_int]));
    assert!(!mono2.request(&mut tt, g, &[int]), "方向反过来就漏了");
    assert_eq!(mono2.pending(), 1);
    let r = mono2.take().expect("还有一个");
    // 名字取**底层**的名字（实例名与别名无关）
    assert_eq!(r.name, "G\u{00b7}int");
    // 实参也取**底层**：`InstRequest.args` 是 Task 14 替换的输入，它必须是
    // 规范化后的类型 —— 否则下游看到的是 `MyInt`，而它没有自己的底层布局。
    assert_eq!(r.args, vec![int], "实参没有规范化：下游会拿到别名");
}
