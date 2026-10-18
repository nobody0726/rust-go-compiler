//! `T-TY-21..28` + `T-TY-81`：同一性判定的验收（M3/Task 17+18；
//! `T-TY-81` 的拒面由 M4/Task 10 改写为**判定面**，`U-M3-05` 解除）。
//!
//! 期望值来自 `go/types/predicates.go` 的 `identical` **逐条读出来的语义**
//! （不是抄 oracle 输出）：每条测试的注释都写明对齐的是官方哪一条。
//! `T-TY-92` 的 oracle 对撞由 `tools/types-oracle` 驱动（Task 31/34 接线）。

use rgoc_const::{BasicId, BigInt, Const};
use rgoc_types::table::Field;
use rgoc_types::{ChanDir, IfaceTSet, Type, TypeId, TypeTable, identical, unalias};

/// 判定并断言「判得出」（M4/Task 10 起全部形态都判得出）。
fn same(tt: &TypeTable, x: TypeId, y: TypeId) -> bool {
    identical(tt, x, y).expect("M4/Task 10 起同一性无拒面，应判得出")
}

/// 具名 + 非嵌入字段。
fn fld(name: &str, typ: TypeId) -> Field {
    Field {
        name: Some(name.to_string()),
        typ,
        tag: None,
        embedded: false,
    }
}

#[test]
fn t_ty_21_basic_两两比较() {
    let mut tt = TypeTable::new();
    let ids: Vec<TypeId> = BasicId::ALL.iter().map(|b| tt.add_basic(*b)).collect();
    for (i, x) in ids.iter().enumerate() {
        for (j, y) in ids.iter().enumerate() {
            assert_eq!(
                same(&tt, *x, *y),
                i == j,
                "{} vs {}",
                BasicId::ALL[i].as_str(),
                BasicId::ALL[j].as_str()
            );
        }
    }

    // `add_basic` 不去重 ⇒ 同一类型的**两个条目**必须判等（官方：`*Basic` 是单例，
    // 只能比 `kind`，不能靠 `x == y`）
    let a = tt.add_basic(BasicId::Int);
    let b = tt.add_basic(BasicId::Int);
    assert_ne!(a, b);
    assert!(same(&tt, a, b));

    // `byte`/`rune` 在**表里就不存在**（builder 折到 `uint8`/`int32`）⇒
    // 「byte ≡ uint8」这条由表示层保证，判定层只需比 `BasicId`
    assert!(!BasicId::ALL.iter().any(|k| k.as_str() == "byte"));
    assert!(!BasicId::ALL.iter().any(|k| k.as_str() == "rune"));
    let u1 = tt.add_basic(BasicId::Uint8);
    let u2 = tt.add_basic(BasicId::Uint8);
    assert!(same(&tt, u1, u2));

    // `invalid` vs `invalid`（同一张表的单例）与 vs 具体类型
    let inv = tt.invalid();
    assert!(same(&tt, inv, inv));
    assert!(!same(&tt, inv, a));
}

#[test]
fn t_ty_22_具名与底层不等() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let a1 = tt.add_named("A");
    tt.set_named_underlying(a1, int);
    // 同名、同底层，但**另一个定义**
    let a2 = tt.add_named("A");
    tt.set_named_underlying(a2, int);

    assert!(same(&tt, tt.named_type(a1), tt.named_type(a1)));
    // 官方：`if y, ok := y.(*Named); ok { return x.obj == y.obj }` ⇒ 与底层**不等**
    assert!(!same(&tt, tt.named_type(a1), int));
    // 定义身份（不是名字、也不是结构）
    assert!(!same(&tt, tt.named_type(a1), tt.named_type(a2)));
}

#[test]
fn t_ty_23_别名归一() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let b = tt.add_named("B");
    tt.set_named_underlying(b, int);
    let a = tt.add_alias("A");
    tt.set_alias_rhs(a, tt.named_type(b));
    let x = tt.add_alias("X");
    tt.set_alias_rhs(x, tt.alias_type(a));
    let c = tt.add_alias("C");
    tt.set_alias_rhs(c, int);

    // 官方 `identical` 第一件事就是 `Unalias(x)`/`Unalias(y)` ⇒ 别名完全透明
    assert!(same(&tt, tt.alias_type(a), tt.named_type(b)));
    assert!(same(&tt, tt.alias_type(x), tt.named_type(b)));
    assert_eq!(unalias(&tt, tt.alias_type(x)), tt.named_type(b));
    // `type C = int` 与 `int`（**另一个** `basic int` 条目）相等
    let int2 = tt.add_basic(BasicId::Int);
    assert!(same(&tt, tt.alias_type(c), int2));
    // 别名不等价于「它指向的定义」以外的具名类型
    assert!(!same(&tt, tt.alias_type(c), tt.named_type(b)));
}

#[test]
fn t_ty_24_指针切片映射通道() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);

    // ⚠️ 结构型**不去重**（两个 `*int` 是两个条目），但**同一性判定必须相等**。
    //    这两件事分开正是「表示做宽、判定做窄」：编号按出现位置，语义按结构。
    let p1 = tt.add_pointer(int);
    let p2 = tt.add_pointer(int);
    assert_ne!(p1, p2);
    assert!(same(&tt, p1, p2));

    let s1 = tt.add_slice(int);
    let s2 = tt.add_slice(string);
    let s3 = tt.add_slice(int);
    assert!(same(&tt, s1, s3));
    assert!(!same(&tt, s1, s2));
    // 不同种类之间一律不等（`[]int` vs `*int`）
    assert!(!same(&tt, s1, p1));
    // 嵌套：`[]*int` vs `[]*int`
    let n1 = tt.add_slice(p1);
    let n2 = tt.add_slice(p2);
    assert!(same(&tt, n1, n2));

    // 通道**方向**必须一致
    let c1 = tt.add_chan(ChanDir::SendRecv, int);
    let c2 = tt.add_chan(ChanDir::SendRecv, int);
    let c3 = tt.add_chan(ChanDir::SendOnly, int);
    let c4 = tt.add_chan(ChanDir::RecvOnly, int);
    assert!(same(&tt, c1, c2));
    assert!(!same(&tt, c1, c3));
    assert!(!same(&tt, c3, c4));

    // 映射是 key+elem 有序对
    let m1 = tt.add_map(int, string);
    let m2 = tt.add_map(int, string);
    let m3 = tt.add_map(string, int);
    assert!(same(&tt, m1, m2));
    assert!(!same(&tt, m1, m3));
}

#[test]
fn t_ty_25_数组长度按值比_未定长度宽容() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);
    let three = tt.consts_mut().add(Const::Int(BigInt::from_i64(3)));
    let three2 = tt.consts_mut().add(Const::Int(BigInt::from_i64(3)));
    let four = tt.consts_mut().add(Const::Int(BigInt::from_i64(4)));
    let unknown = tt.consts_mut().add(Const::Unknown);

    let a3 = tt.add_array(Some(three), int);
    let a3b = tt.add_array(Some(three2), int);
    let a4 = tt.add_array(Some(four), int);
    assert!(same(&tt, a3, a3b), "长度是**两个常量条目**，按值比 ⇒ 等");
    assert!(!same(&tt, a3, a4));

    // 官方：`x.len < 0 || y.len < 0 || x.len == y.len` ⇒ 长度**未定**时不当不等。
    // `[...]T`（`None`）与求值失败（`Const::Unknown`）都走这条宽容分支。
    let unlen = tt.add_array(None, int);
    let aunk = tt.add_array(Some(unknown), int);
    assert!(same(&tt, a3, unlen));
    assert!(same(&tt, unlen, a4));
    assert!(same(&tt, a3, aunk));

    // 元素类型参与判定
    let a3s = tt.add_array(Some(three), string);
    assert!(!same(&tt, a3, a3s));
    let sl = tt.add_slice(int);
    assert!(!same(&tt, a3, sl));
}

#[test]
fn t_ty_26_结构体_名_tag_嵌入_顺序() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);

    let s1 = tt.add_struct(vec![fld("x", int), fld("y", string)]);
    let s1_type = tt.struct_type(s1);
    let s2 = tt.add_struct(vec![fld("x", int), fld("y", string)]);
    assert!(same(&tt, s1_type, tt.struct_type(s2)));

    // Tag 参与
    let s3 = tt.add_struct(vec![
        fld("x", int),
        Field {
            name: Some("y".to_string()),
            typ: string,
            tag: Some("json:\"y\"".to_string()),
            embedded: false,
        },
    ]);
    assert!(!same(&tt, s1_type, tt.struct_type(s3)));

    // 字段名参与
    let s4 = tt.add_struct(vec![fld("x", int), fld("z", string)]);
    assert!(!same(&tt, s1_type, tt.struct_type(s4)));

    // 字段**顺序**参与（官方的循环是按声明序逐项比）
    let s5 = tt.add_struct(vec![fld("y", string), fld("x", int)]);
    assert!(!same(&tt, s1_type, tt.struct_type(s5)));

    // 字段数参与
    let s6 = tt.add_struct(vec![fld("x", int)]);
    assert!(!same(&tt, s1_type, tt.struct_type(s6)));

    // 嵌入标记参与：`struct{ Z }` 与 `struct{ Z Z }` 的**名字与类型都一样**，
    // 只有 `embedded` 不同 ⇒ 官方仍判不等
    let z = tt.add_named("Z");
    let z_int = tt.add_basic(BasicId::Int);
    tt.set_named_underlying(z, z_int);
    let emb = tt.add_struct(vec![Field {
        name: Some("Z".to_string()),
        typ: tt.named_type(z),
        tag: None,
        embedded: true,
    }]);
    let named = tt.add_struct(vec![Field {
        name: Some("Z".to_string()),
        typ: tt.named_type(z),
        tag: None,
        embedded: false,
    }]);
    assert!(!same(&tt, tt.struct_type(emb), tt.struct_type(named)));

    // 嵌入字段的名字 = **被嵌入类型的名字** ⇒ `struct{ Z }` 与 `struct{ W }`
    // 即使 `Z`/`W` 底层相同也不等
    let w = tt.add_named("W");
    let w_int = tt.add_basic(BasicId::Int);
    tt.set_named_underlying(w, w_int);
    let emb_w = tt.add_struct(vec![Field {
        name: Some("W".to_string()),
        typ: tt.named_type(w),
        tag: None,
        embedded: true,
    }]);
    // 底层相同（都是 int）先确认一下
    assert!(same(&tt, z_int, w_int));
    assert!(!same(&tt, tt.struct_type(emb), tt.struct_type(emb_w)));
}

#[test]
fn t_ty_27_签名_名字与接收者不参与() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);

    // 参数名 / 结果名不同：不算不同
    let p1 = tt.add_tuple(vec![(Some("a".to_string()), int)]);
    let r1 = tt.add_tuple(vec![(None, int)]);
    let p2 = tt.add_tuple(vec![(Some("b".to_string()), int)]);
    let r2 = tt.add_tuple(vec![(Some("res".to_string()), int)]);
    let f1 = tt.add_signature(None, p1, r1, false);
    let f2 = tt.add_signature(None, p2, r2, false);
    assert!(same(&tt, tt.signature_type(f1), tt.signature_type(f2)));

    // 接收者**不参与**（官方 `identical` 只比 variadic/params/results）
    let t = tt.add_named("T");
    let t_int = tt.add_basic(BasicId::Int);
    tt.set_named_underlying(t, t_int);
    let m = tt.add_signature(Some(tt.named_type(t)), p1, r1, false);
    assert!(same(&tt, tt.signature_type(f1), tt.signature_type(m)));

    // 变参参与
    let ps = tt.add_slice(int);
    let p3 = tt.add_tuple(vec![(Some("a".to_string()), ps)]);
    let f3 = tt.add_signature(None, p3, r1, true);
    assert!(!same(&tt, tt.signature_type(f1), tt.signature_type(f3)));

    // 结果个数 / 类型参与
    let r3 = tt.add_tuple(vec![(None, int), (None, string)]);
    let f4 = tt.add_signature(None, p1, r3, false);
    assert!(!same(&tt, tt.signature_type(f1), tt.signature_type(f4)));

    // 参数个数参与
    let p4 = tt.add_tuple(Vec::new());
    let f5 = tt.add_signature(None, p4, r1, false);
    assert!(!same(&tt, tt.signature_type(f1), tt.signature_type(f5)));
}

#[test]
fn t_ty_28_接口方法集_与顺序无关() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);
    let empty = tt.add_tuple(Vec::new());
    let r_int = tt.add_tuple(vec![(None, int)]);
    let r_str = tt.add_tuple(vec![(None, string)]);

    let sig_a = tt.add_signature(None, empty, r_int, false);
    let sig_b = tt.add_signature(None, empty, r_str, false);
    let sa = tt.signature_type(sig_a);
    let sb = tt.signature_type(sig_b);

    // 两份**互不相同条目**的签名 + **相反的方法顺序**
    let p2 = tt.add_tuple(Vec::new());
    let q_int = tt.add_tuple(vec![(None, int)]);
    let q_str = tt.add_tuple(vec![(None, string)]);
    let sig_a2 = tt.add_signature(None, p2, q_int, false);
    let sig_b2 = tt.add_signature(None, p2, q_str, false);

    let i1 = tt.add_interface(
        vec![("A".to_string(), sa), ("B".to_string(), sb)],
        None,
        IfaceTSet::All,
    );
    let i2 = tt.add_interface(
        vec![
            ("B".to_string(), tt.signature_type(sig_b2)),
            ("A".to_string(), tt.signature_type(sig_a2)),
        ],
        None,
        IfaceTSet::All,
    );
    // 官方：`The order of the methods is irrelevant`
    assert!(same(&tt, tt.interface_type(i1), tt.interface_type(i2)));

    // 方法名不同 ⇒ 不等
    let i3 = tt.add_interface(
        vec![("A".to_string(), sa), ("C".to_string(), sb)],
        None,
        IfaceTSet::All,
    );
    assert!(!same(&tt, tt.interface_type(i1), tt.interface_type(i3)));

    // 方法数不同 ⇒ 不等
    let i4 = tt.add_interface(vec![("A".to_string(), sa)], None, IfaceTSet::All);
    assert!(!same(&tt, tt.interface_type(i1), tt.interface_type(i4)));

    // 签名不同 ⇒ 不等（`B() string` vs `B() int`）
    let r_int2 = tt.add_tuple(vec![(None, int)]);
    let sig_b3 = tt.add_signature(None, empty, r_int2, false);
    let i5 = tt.add_interface(
        vec![
            ("A".to_string(), sa),
            ("B".to_string(), tt.signature_type(sig_b3)),
        ],
        None,
        IfaceTSet::All,
    );
    assert!(!same(&tt, tt.interface_type(i1), tt.interface_type(i5)));

    // 空接口 vs 空接口（两个条目）⇒ 相等
    let e1 = tt.add_interface(Vec::new(), None, IfaceTSet::All);
    let e2 = tt.add_interface(Vec::new(), None, IfaceTSet::All);
    assert!(same(&tt, tt.interface_type(e1), tt.interface_type(e2)));
}

#[test]
fn t_ty_29_实例化在_m3_退化为同一_id() {
    // M3 **没有实例化**（`D-M3-2` 的「表示做宽」不含类型实参那一层）
    // ⇒ `Type::Named` 不带类型实参，判定**退化为「同一 Id」**。
    //
    // 这是**已知边界**（`M3-tests.md` §3.2 的 `T-TY-29` 判据 = 设计 §5.2 表），
    // 不是漏做：官方侧 `G[int]` 与 `G[string]` 是**两个** `*Named`
    // （带不同的类型实参），本层表示不出这个差别 ⇒ 对撞集里**不放**实例化样本
    // （白名单 `U-M3-04`）。
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let g = tt.add_named("G");
    tt.set_named_underlying(g, int);
    let g_ty = tt.named_type(g);
    assert!(same(&tt, g_ty, g_ty));
    // 同一个定义**只有一个**条目 ⇒ 官方眼里「G 的两个实例」在这里是同一个值
    assert_eq!(tt.named_type(g), g_ty);
}

#[test]
fn t_ty_81_类型集版接口同一性_两侧都作答() {
    // M3 的拒面（`U-M3-05`）由 M4/Task 10 解除：**两侧都作答**。判据逐条对齐
    // 官方 `predicates.go` 的接口分支：
    //   comparable 位 → 项集 `termlist.equal`（双向 `subsetOf`）→ 方法集（顺序无关）。
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);
    let empty = tt.add_tuple(Vec::new());
    let r_int = tt.add_tuple(vec![(None, int)]);
    let sig = tt.add_signature(None, empty, r_int, false);
    let sig_ty = tt.signature_type(sig);

    // ① 项集：集合语义（含**置换序**相等），异项集不等。
    let u = tt.add_union(vec![(true, int), (true, string)]);
    let c1 = tt.add_interface(
        Vec::new(),
        Some(tt.union_type(u)),
        IfaceTSet::Terms(vec![(true, int), (true, string)]),
    );
    let c1b = tt.add_interface(
        Vec::new(),
        Some(tt.union_type(u)),
        IfaceTSet::Terms(vec![(true, int), (true, string)]),
    );
    let c1r = tt.add_interface(
        Vec::new(),
        Some(tt.union_type(u)),
        IfaceTSet::Terms(vec![(true, string), (true, int)]), // 置换序
    );
    let u2 = tt.add_union(vec![(true, int)]);
    let c2 = tt.add_interface(
        Vec::new(),
        Some(tt.union_type(u2)),
        IfaceTSet::Terms(vec![(true, int)]),
    );
    let plain0 = tt.add_interface(Vec::new(), None, IfaceTSet::All);
    assert_eq!(
        identical(&tt, tt.interface_type(c1), tt.interface_type(c1b)),
        Ok(true)
    );
    assert_eq!(
        identical(&tt, tt.interface_type(c1), tt.interface_type(c1r)),
        Ok(true),
        "置换序是同一集合（`termlist.equal` 不看列表形状）"
    );
    assert_eq!(
        identical(&tt, tt.interface_type(c1), tt.interface_type(c2)),
        Ok(false),
        "合成反例①：异项集 ⇒ 不等"
    );
    // 与 𝓤（`interface{}`）比：项面不同 ⇒ false（`[~int] ⊆ 𝓤` 真但反向假）
    assert_eq!(
        identical(&tt, tt.interface_type(c2), tt.interface_type(plain0)),
        Ok(false)
    );

    // ② `comparable` 位：位相同 ⇒ 判对；与 `interface{}` **同形但语义不同** ⇒ false。
    let cmp = tt.add_interface(Vec::new(), None, IfaceTSet::Comparable);
    let cmp2 = tt.add_interface(Vec::new(), None, IfaceTSet::Comparable);
    assert_eq!(
        identical(&tt, tt.interface_type(cmp), tt.interface_type(cmp2)),
        Ok(true)
    );
    // 合成反例②：`comparable` vs `interface{}` —— 不变量 `!comparable || terms.isAll()`
    // 下这就是 comparable 位差（官方 `xset.comparable != yset.comparable`）
    assert_eq!(
        identical(&tt, tt.interface_type(cmp), tt.interface_type(plain0)),
        Ok(false)
    );
    // 同一条目短路（早退 `x == y`）
    assert_eq!(
        identical(&tt, tt.interface_type(cmp), tt.interface_type(cmp)),
        Ok(true)
    );

    // ③ `union` / `typeparam` 独立形态（官方 `case *Union` / `case *TypeParam`）：
    //    - 并集比**项表相等**（集合语义）；
    //    - 类型参数**只有同一条目**才真（官方注释：nothing to do）。
    let u_r = tt.add_union(vec![(true, string), (true, int)]);
    assert_eq!(
        identical(&tt, tt.union_type(u), tt.union_type(u_r)),
        Ok(true),
        "并集置换序"
    );
    assert_eq!(
        identical(&tt, tt.union_type(u2), tt.union_type(u)),
        Ok(false)
    );
    let tp = tt.add_type_param(0, tt.union_type(u2));
    let tp_ty = tt.type_param_type(tp);
    let tp2 = tt.add_type_param(1, tt.union_type(u2));
    let tp2_ty = tt.type_param_type(tp2);
    assert_eq!(
        identical(&tt, tp_ty, tt.union_type(u2)),
        Ok(false),
        "类型参数 vs 并集：形态不同 ⇒ 不等"
    );
    assert_eq!(
        identical(&tt, tp_ty, tp2_ty),
        Ok(false),
        "不同条目 ⇒ 不等（哪怕约束同形）"
    );
    assert_eq!(identical(&tt, tp_ty, tp_ty), Ok(true), "同一条目短路");

    // ④ 方法与项集**混合**：两面都参与判定。
    let mx1 = tt.add_interface(
        vec![("M".to_string(), sig_ty)],
        Some(tt.union_type(u2)),
        IfaceTSet::Terms(vec![(true, int)]),
    );
    let mx2 = tt.add_interface(
        vec![("M".to_string(), sig_ty)],
        Some(tt.union_type(u2)),
        IfaceTSet::Terms(vec![(true, int)]),
    );
    let mx3 = tt.add_interface(
        vec![("M".to_string(), sig_ty)],
        Some(tt.union_type(u)),
        IfaceTSet::Terms(vec![(true, int), (true, string)]),
    );
    let mx4 = tt.add_interface(
        vec![("N".to_string(), sig_ty)],
        Some(tt.union_type(u2)),
        IfaceTSet::Terms(vec![(true, int)]),
    );
    assert_eq!(
        identical(&tt, tt.interface_type(mx1), tt.interface_type(mx2)),
        Ok(true)
    );
    assert_eq!(
        identical(&tt, tt.interface_type(mx1), tt.interface_type(mx3)),
        Ok(false),
        "方法同、项集异 ⇒ 不等"
    );
    assert_eq!(
        identical(&tt, tt.interface_type(mx1), tt.interface_type(mx4)),
        Ok(false),
        "项集同、方法名异 ⇒ 不等"
    );

    // ⑤ 反向（`T-TY-81` 要求成对）：纯**方法集**接口照旧判得出。
    let plain = tt.add_interface(vec![("M".to_string(), sig_ty)], None, IfaceTSet::All);
    let plain2 = tt.add_interface(vec![("M".to_string(), sig_ty)], None, IfaceTSet::All);
    assert_eq!(
        identical(&tt, tt.interface_type(plain), tt.interface_type(plain2)),
        Ok(true)
    );
    // 而且「不相等」也是**判得出来的**（不是拒绝）
    let other = tt.add_interface(vec![("N".to_string(), sig_ty)], None, IfaceTSet::All);
    assert_eq!(
        identical(&tt, tt.interface_type(plain), tt.interface_type(other)),
        Ok(false)
    );

    // ⑥ universe 里的 `comparable`：**壳是 `named`**，本体是 `Comparable` 语义。
    //    M3 时「本体 vs any」是拒点；现在**判得出来**（comparable 位不同 ⇒ false）。
    //
    //    ⚠️ 别拿 `comparable` 直接和 `any` 比：壳不同（`named` vs `alias`）⇒
    //    那就是「两个不同的类型」（`Ok(false)`），**走不到**类型集判定。
    //    官方表示同理（go1.27.1 实测 `Universe.Lookup("comparable").Type()`
    //    是 `*types.Named`，Task 33 对撞订正了此处口径）。
    let mut u_tt = TypeTable::new();
    let mut u_st = rgoc_types::ScopeTable::new();
    let u = rgoc_types::declare_universe(&mut u_tt, &mut u_st);
    let cmp_obj = u_st.lookup(u, "comparable").and_then(|o| o.typ).unwrap();
    let any_obj = u_st.lookup(u, "any").and_then(|o| o.typ).unwrap();
    let Type::Named(cmp_n) = u_tt.type_of(cmp_obj) else {
        panic!("comparable 该是具名类型，实际 {:?}", u_tt.type_of(cmp_obj));
    };
    let cmp_body = u_tt.named_underlying(*cmp_n);
    assert_eq!(
        identical(&u_tt, cmp_body, any_obj),
        Ok(false),
        "comparable 本体 vs any：位不同 ⇒ 判 false（不再是拒）"
    );
    // 壳对自己：定义恒等（与 ② 的短路同一条）
    assert_eq!(identical(&u_tt, cmp_obj, cmp_obj), Ok(true));
    // `any` = `interface{}`（空方法集）⇒ 判得出
    assert_eq!(identical(&u_tt, any_obj, any_obj), Ok(true));
}
