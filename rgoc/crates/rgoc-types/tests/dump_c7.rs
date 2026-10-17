//! `T-TY-12..19`：`C7` 类型 dump 的验收（M3/Task 16）。
//!
//! 判据分两层：
//!
//! * **本文件**（`H` = 形状判据）：给定一张手工构造的类型表，输出必须**逐字节**
//!   等于手算的期望 —— 手算的依据是 `C7` §3 的编号规则与 §4 的逐标签行语法；
//! * **对撞**（`O` = oracle 判据）：`tools/types-oracle/run-collide.sh` 驱动两侧
//!   真跑（`T-TY-91`，Task 30/31 接线）。
//!
//! 这里的期望值**逐条手算**（不复制 oracle 输出），否则「两侧一致」会退化成
//! 「同一份错」。手算时踩过的两个坑记在各测试的注释里。

use rgoc_const::{BasicId, BigInt, Const};
use rgoc_types::table::Field;
use rgoc_types::{ChanDir, Type, TypeTable, declare_universe, dump_types, universe};

/// 去掉 `#` 注释行与空行（`C7` §2 规则 1：**两侧都忽略**）。
fn strip(s: &str) -> String {
    s.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| format!("{l}\n"))
        .collect()
}

/// 期望的正文（每行自带 `\n`）。
fn want(lines: &[&str]) -> String {
    lines.iter().map(|l| format!("{l}\n")).collect()
}

#[test]
fn t_ty_12_basic_全集与_invalid() {
    let mut tt = TypeTable::new();
    let mut roots: Vec<_> = BasicId::ALL.iter().map(|b| tt.add_basic(*b)).collect();
    roots.push(tt.invalid());

    let got = strip(&dump_types(&tt, &roots));
    let mut expect: Vec<String> = BasicId::ALL
        .iter()
        .enumerate()
        .map(|(i, b)| format!("{i}\tbasic {}", b.as_str()))
        .collect();
    expect.push(format!("{}\tinvalid", BasicId::ALL.len()));
    let expect: String = expect.iter().map(|l| format!("{l}\n")).collect();
    assert_eq!(got, expect);

    // 25 个 basic 名字 + `invalid`：**25 项**（`C7` §4.1 的实测订正）
    assert_eq!(BasicId::ALL.len(), 25);
    // `invalid` 不写 `basic ...`（oracle 侧也是特判）
    assert!(!got.contains("basic invalid"));
    // `byte`/`rune` 的名字不出现在 dump 里（它们折到 `uint8`/`int32`）
    assert!(got.contains("basic uint8"));
    assert!(got.contains("basic int32"));
    assert!(!got.contains("basic byte"));
    assert!(!got.contains("basic rune"));
}

#[test]
fn t_ty_13_复合类型_pointer_slice_array_map_chan() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let three = tt.consts_mut().add(Const::Int(BigInt::from_i64(3)));
    let ptr = tt.add_pointer(int);
    let slice = tt.add_slice(int);
    let arr = tt.add_array(Some(three), int);
    let arr_unlen = tt.add_array(None, int);
    let map = tt.add_map(int, ptr);
    let ch_sr = tt.add_chan(ChanDir::SendRecv, int);
    let ch_so = tt.add_chan(ChanDir::SendOnly, int);
    let ch_ro = tt.add_chan(ChanDir::RecvOnly, int);

    let got = strip(&dump_types(
        &tt,
        &[int, ptr, slice, arr, arr_unlen, map, ch_sr, ch_so, ch_ro],
    ));
    // 手算：根是插入序；`int` 第 0 个占号 0；其后每个结构型的子节点都已占号 ⇒ 依次 +1
    assert_eq!(
        got,
        want(&[
            "0\tbasic int",
            "1\tpointer 0",
            "2\tslice 0",
            "3\tarray 3 0",
            "4\tarray - 0",
            "5\tmap 0 1",
            "6\tchan sendrecv 0",
            "7\tchan sendonly 0",
            "8\tchan recvonly 0",
        ])
    );
}

#[test]
fn t_ty_14_struct_字段名_tag_嵌入_顺序() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);
    let st = tt.add_struct(vec![
        Field {
            name: Some("x".to_string()),
            typ: int,
            tag: None,
            embedded: false,
        },
        Field {
            name: Some("y".to_string()),
            typ: string,
            // Tag 是**源码里的字符串**（不含反引号）⇒ 走 `C7` §2.3 的转义
            tag: Some("json:\"y\"".to_string()),
            embedded: false,
        },
        Field {
            // 嵌入字段：即使 AST 上给了名字，名字位也必须写 `-`（`C7` §2.3）
            name: Some("z".to_string()),
            typ: int,
            tag: None,
            embedded: true,
        },
    ]);

    let got = strip(&dump_types(&tt, &[tt.struct_type(st)]));
    assert_eq!(
        got,
        want(&[
            "0\tbasic int",
            "1\tbasic string",
            "2\tstruct 3",
            "  \"x\" 0 - false",
            "  \"y\" 1 \"json:\\\"y\\\"\" false",
            "  - 0 - true",
        ])
    );
    // 字段数是**声明序**、不是排序后的
    assert!(got.find("\"x\"").unwrap() < got.find("\"y\"").unwrap());
}

#[test]
fn t_ty_15_signature_与_tuple_变参与多结果() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);
    let params = tt.add_tuple(vec![(Some("a".to_string()), int), (None, string)]);
    let results = tt.add_tuple(vec![(None, int), (None, string)]);
    let sig = tt.add_signature(None, params, results, true);
    let sig_ty = tt.signature_type(sig);

    let got = strip(&dump_types(&tt, &[sig_ty]));
    // 下钻顺序：recv（无）→ params → results；tuple 先占号、signature 最后
    assert_eq!(
        got,
        want(&[
            "0\tbasic int",
            "1\tbasic string",
            "2\ttuple 2",
            "  \"a\" 0",
            "  - 1",
            "3\ttuple 2",
            "  - 0",
            "  - 1",
            "4\tsignature - 2 3 true",
        ])
    );

    // 方法签名：`recv` 是具名类型 ⇒ 写 `#<序号>`；**空元组是单例** ⇒ 参数与结果
    // 两个空元组条目共用同一个号（实测：`func()` 的参数与结果元组是同一个实例）
    let t = tt.add_named("T");
    tt.set_named_underlying(t, int);
    let m_params = tt.add_tuple(Vec::new());
    let m_results = tt.add_tuple(Vec::new());
    let m = tt.add_signature(Some(tt.named_type(t)), m_params, m_results, false);
    let got = strip(&dump_types(&tt, &[tt.signature_type(m)]));
    assert_eq!(
        got,
        want(&[
            "0\tnamed 1",
            "1\tbasic int",
            "2\ttuple 0",
            "3\tsignature #0 2 2 false",
        ])
    );
}

#[test]
fn 空元组是单例_结构型不是() {
    // 实测（`probe/emptytup.go`）：`var x func(); var y func(); var z func() ()`
    // ⇒ 只有**一条** `0 tuple 0`，三个签名全引用它；而 `struct{}` / `interface{}`
    // 出现两次就是两个号。这条差别直接决定编号，必须钉住。
    let mut tt = TypeTable::new();
    let p1 = tt.add_tuple(Vec::new());
    let r1 = tt.add_tuple(Vec::new());
    let s1 = tt.add_signature(None, p1, r1, false);
    let p2 = tt.add_tuple(Vec::new());
    let r2 = tt.add_tuple(Vec::new());
    let s2 = tt.add_signature(None, p2, r2, false);
    let st1 = tt.add_struct(Vec::new());
    let st2 = tt.add_struct(Vec::new());
    let if1 = tt.add_interface(Vec::new(), None, false);
    let if2 = tt.add_interface(Vec::new(), None, false);

    let got = strip(&dump_types(
        &tt,
        &[
            tt.signature_type(s1),
            tt.signature_type(s2),
            tt.struct_type(st1),
            tt.struct_type(st2),
            tt.interface_type(if1),
            tt.interface_type(if2),
        ],
    ));
    assert_eq!(
        got,
        want(&[
            "0\ttuple 0",
            "1\tsignature - 0 0 false",
            "2\tsignature - 0 0 false",
            "3\tstruct 0",
            "4\tstruct 0",
            "5\tinterface 0 0",
            "6\tinterface 0 0",
        ])
    );
}

#[test]
fn t_ty_16_named_alias_用井号序号() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let t = tt.add_named("T");
    tt.set_named_underlying(t, int);
    let t_ty = tt.named_type(t);
    let p1 = tt.add_pointer(t_ty);
    let p2 = tt.add_pointer(t_ty);

    let got = strip(&dump_types(&tt, &[t_ty, p1, p2]));
    // 三条判据一次到位：
    //   ① 定义型**先占号**（`T` 是 0，它的本体 `int` 才是 1）—— 递归类型终止的手段
    //   ② 引用的**同一个定义**复用同一个槽（两个指针都指 `#0`）
    //   ③ 结构型**不去重**（两个 `*T` 各占一个号：2 与 3）
    assert_eq!(
        got,
        want(&[
            "0\tnamed 1",
            "1\tbasic int",
            "2\tpointer #0",
            "3\tpointer #0",
        ])
    );

    // `alias` 同理先占号；两个**同名不同定义**必须编号不同（`C7` §2.2 的动机）
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let s1 = tt.add_named("S");
    tt.set_named_underlying(s1, int);
    let s2 = tt.add_named("S");
    tt.set_named_underlying(s2, int);
    let got = strip(&dump_types(&tt, &[tt.named_type(s1), tt.named_type(s2)]));
    assert_eq!(got, want(&["0\tnamed 1", "1\tbasic int", "2\tnamed 1"]));

    // alias：`type A = int` ⇒ `alias <ref>`，引用它时写 `#<该 alias 的序号>`
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let a = tt.add_alias("A");
    tt.set_alias_rhs(a, int);
    let a_ty = tt.alias_type(a);
    let ptr = tt.add_pointer(a_ty);
    let got = strip(&dump_types(&tt, &[a_ty, ptr]));
    assert_eq!(got, want(&["0\talias 1", "1\tbasic int", "2\tpointer #0"]));
}

#[test]
fn t_ty_17_interface_方法集与项集() {
    // ① 方法集版：方法**按名字升序**（存进去的顺序刻意是乱的）
    // ⚠️ 每个方法各自造一套 tuple/signature —— 官方侧每个方法有独立的
    //    `*types.Signature` 与两个独立的 `*types.Tuple`，共享会让编号少几条。
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let mk_sig = |tt: &mut TypeTable| {
        let p = tt.add_tuple(Vec::new());
        let r = tt.add_tuple(vec![(None, int)]);
        let s = tt.add_signature(None, p, r, false);
        tt.signature_type(s)
    };
    let sig_z = mk_sig(&mut tt);
    let sig_a = mk_sig(&mut tt);
    let iface = tt.add_interface(
        vec![("Z".to_string(), sig_z), ("A".to_string(), sig_a)],
        None,
        false,
    );

    let got = strip(&dump_types(&tt, &[tt.interface_type(iface)]));
    assert_eq!(
        got,
        want(&[
            "0\ttuple 0",
            "1\tbasic int",
            "2\ttuple 1",
            "  - 1",
            "3\tsignature - 0 2 false",
            "4\ttuple 1",
            "  - 1",
            "5\tsignature - 0 4 false",
            "6\tinterface 2 0",
            "  method \"A\" 3",
            "  method \"Z\" 5",
        ])
    );

    // ② 项集版（约束接口）：`interface 0 1` + 一行 `term`
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);
    let u = tt.add_union(vec![(true, int), (false, string)]);
    let iface = tt.add_interface(Vec::new(), Some(tt.union_type(u)), false);
    let got = strip(&dump_types(&tt, &[tt.interface_type(iface)]));
    assert_eq!(
        got,
        want(&[
            "0\tbasic int",
            "1\tbasic string",
            "2\tunion 2",
            "  term tilde 0",
            "  term exact 1",
            "3\tinterface 0 1",
            "  term 2",
        ])
    );

    // ③ `comparable` 在 dump 里是 **`named`**（本体 `interface 0 0`）。
    //
    //    两层都要断言，因为它们分别钉住两件不同的事：
    //    * 壳存在 —— go/types 实测 `Universe.Lookup("comparable").Type()` 就是
    //      `*types.Named`（Task 33 对撞：少了壳 ⇒ `[T comparable]` 少一行）；
    //    * **`comparable` 标记在 dump 里看不见**（C7 §4.2：它与 `interface{}`
    //      的**本体**同形）—— 这正是「表示做宽」要留住的那一点，
    //      所以用 `interface_def().comparable` 断言它，而不是看 dump。
    let (u_st, u, u_tt) = universe();
    let u_scope = u_st.scope(u);
    let cmp_ty = u_scope.lookup("comparable").and_then(|o| o.typ).unwrap();
    let any_ty = u_scope.lookup("any").and_then(|o| o.typ).unwrap();
    assert_eq!(
        strip(&dump_types(&u_tt, &[cmp_ty])),
        want(&["0\tnamed 1", "1\tinterface 0 0"])
    );
    let Type::Named(n) = u_tt.type_of(cmp_ty) else {
        panic!("comparable 该是具名类型，实际 {:?}", u_tt.type_of(cmp_ty));
    };
    let Type::Interface(i) = u_tt.type_of(u_tt.named_underlying(*n)) else {
        panic!("comparable 的本体该是接口");
    };
    assert!(u_tt.interface_def(*i).comparable, "comparable 标记必须留住");
    assert!(u_tt.interface_def(*i).terms.is_none(), "它不是项集");
    // `any` 是 alias ⇒ 先占号 0，右侧空接口 1
    assert_eq!(
        strip(&dump_types(&u_tt, &[any_ty])),
        want(&["0\talias 1", "1\tinterface 0 0"])
    );
}

#[test]
fn t_ty_18_typeparam_与_union_与_tilde() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let u = tt.add_union(vec![(true, int)]);
    let tp = tt.add_type_param(0, tt.union_type(u));
    let got = strip(&dump_types(&tt, &[tt.type_param_type(tp)]));
    assert_eq!(
        got,
        want(&[
            "0\tbasic int",
            "1\tunion 1",
            "  term tilde 0",
            "2\ttypeparam 0 1",
        ])
    );

    // 约束是**具名**类型 ⇒ 写 `#<序号>`（实测 oracle：`typeparam 0 #0`）
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let c = tt.add_named("C");
    tt.set_named_underlying(c, int);
    let iface = tt.add_interface(Vec::new(), None, false);
    let iface_ty = tt.interface_type(iface);
    tt.set_named_underlying(c, iface_ty);
    // 索引不为 0 也要如实写出来（官方 `TypeParam.Index()` 是它在**自身列表**里的下标）
    let tp = tt.add_type_param(1, tt.named_type(c));
    let got = strip(&dump_types(&tt, &[tt.type_param_type(tp)]));
    // `int` 是**不可达**的（`C` 的本体被改成了 interface）⇒ 它不占号
    assert_eq!(
        got,
        want(&["0\tnamed 1", "1\tinterface 0 0", "2\ttypeparam 1 #0",])
    );
}

#[test]
fn t_ty_19_dump_确定性与幂等() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);
    let u = tt.add_union(vec![(true, int), (false, string)]);
    let iface = tt.add_interface(Vec::new(), Some(tt.union_type(u)), false);
    let st = tt.add_struct(vec![Field {
        name: Some("f".to_string()),
        typ: iface_type(&tt, iface),
        tag: None,
        embedded: false,
    }]);
    let roots = [tt.struct_type(st), int];

    let a = dump_types(&tt, &roots);
    let b = dump_types(&tt, &roots);
    assert_eq!(a, b, "同输入两次必须逐字节相等");
    // 换一份**新分配**的根切片（并让 dumper 的 HashMap 从头再走一遍）
    // ⇒ 输出不依赖哈希序、也不依赖切片身份
    let roots2 = roots.to_vec();
    let c = dump_types(&tt, &roots2);
    assert_eq!(a, c);
    // 头部如实标来源；正文里没有 `#` 之外的注释
    assert!(a.starts_with("# source: rgoc\n"));
    assert!(a.ends_with('\n'), "末尾恰一个换行（C7 §2）");
    assert!(!a.ends_with("\n\n"));
}

#[test]
fn 递归类型走访终止() {
    // `type N struct{ p *N }` —— 定义型先占号才可能终止
    let mut tt = TypeTable::new();
    let n = tt.add_named("N");
    let ptr = tt.add_pointer(tt.named_type(n));
    let st = tt.add_struct(vec![Field {
        name: Some("p".to_string()),
        typ: ptr,
        tag: None,
        embedded: false,
    }]);
    tt.set_named_underlying(n, tt.struct_type(st));
    let got = strip(&dump_types(&tt, &[tt.named_type(n)]));
    assert_eq!(
        got,
        want(&[
            "0\tnamed 2",
            "1\tpointer #0",
            "2\tstruct 1",
            "  \"p\" 1 - false",
        ])
    );
}

#[test]
fn basic_按_basic_id_去重_同一张表里允许多个条目() {
    // 检查器会把 universe 的类型与 fixture 自己的类型放进**同一张表**；
    // 若两个 `basic int` 条目各占一个号，dump 就会与 oracle 分叉（实测 oracle：
    // `func g() (int, error)` 把 universe 的 `error` 拉了进来 ⇒ 必须是同一个号）。
    let mut tt = TypeTable::new();
    let mut u_st = rgoc_types::ScopeTable::new();
    let u = declare_universe(&mut tt, &mut u_st);
    let universe_int = u_st
        .lookup(u, "int")
        .and_then(|o| o.typ)
        .expect("universe 有 int");
    // 模拟 fixture 又造了一个 `int`（`add_basic` 不去重）
    let fixture_int = tt.add_basic(BasicId::Int);
    assert_ne!(universe_int, fixture_int);

    let p1 = tt.add_pointer(universe_int);
    let p2 = tt.add_pointer(fixture_int);
    let got = strip(&dump_types(&tt, &[p1, p2]));
    assert_eq!(
        got,
        want(&["0\tbasic int", "1\tpointer 0", "2\tpointer 0"]),
        "两个 `basic int` 条目必须只占一个号"
    );
}

#[test]
fn 与_oracle_的_emptytup_fixture_逐字节一致() {
    // 期望值 = oracle 在 `.superpowers/sdd/M3-plan/probe/emptytup.go` 上的**原始输出**
    // （只去掉 `# source:` 头行）。这是本文件里唯一一条**直接抄 oracle** 的期望：
    // 它钉住的是「空元组单例 / 结构型不去重」在**整张表**上的累积效果
    // —— 单点规则都对、合起来错位一格，正是分片测试抓不到的那种错。
    let mut tt = TypeTable::new();
    let mk_func = |tt: &mut TypeTable| {
        // `func()`：params 与 results 各造一个空元组条目（**两个不同条目**）
        let p = tt.add_tuple(Vec::new());
        let r = tt.add_tuple(Vec::new());
        let s = tt.add_signature(None, p, r, false);
        tt.signature_type(s)
    };
    let x = mk_func(&mut tt); // var x func()
    let y = mk_func(&mut tt); // var y func()
    let z = mk_func(&mut tt); // var z func() ()
    // type S struct{}（`S` 的**本体**也要下钻 ⇒ `named` 先占号）
    let s = tt.add_named("S");
    let s_st = tt.add_struct(Vec::new());
    tt.set_named_underlying(s, tt.struct_type(s_st));
    let s_ty = tt.named_type(s);
    // var w/v struct{}；var u/t interface{} —— 结构型两次就是两个号
    let w_st = tt.add_struct(Vec::new());
    let w = tt.struct_type(w_st);
    let v_st = tt.add_struct(Vec::new());
    let v = tt.struct_type(v_st);
    let u_if = tt.add_interface(Vec::new(), None, false);
    let u = tt.interface_type(u_if);
    let t_if = tt.add_interface(Vec::new(), None, false);
    let t = tt.interface_type(t_if);

    let got = strip(&dump_types(&tt, &[x, y, z, s_ty, w, v, u, t]));
    assert_eq!(
        got,
        want(&[
            "0\ttuple 0",
            "1\tsignature - 0 0 false",
            "2\tsignature - 0 0 false",
            "3\tsignature - 0 0 false",
            "4\tnamed 5",
            "5\tstruct 0",
            "6\tstruct 0",
            "7\tstruct 0",
            "8\tinterface 0 0",
            "9\tinterface 0 0",
        ])
    );
}

#[test]
fn 最小对撞目标与契约的_want_一致() {
    // `C7` §5：`docs/contracts/testdata/type-dump-min.want`（`var x int; var y *int`）
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../docs/contracts/testdata/type-dump-min.want"
    );
    let want_file = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("读不到 {path}：{e}"));
    let want_body = strip(&want_file);

    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let ptr = tt.add_pointer(int);
    let got = strip(&dump_types(&tt, &[int, ptr]));
    assert_eq!(got, want_body);
    assert_eq!(got, want(&["0\tbasic int", "1\tpointer 0"]));
}

/// 测试辅助：`InterfaceId` → `TypeId`（`TypeTable` 已有同名方法，这里只是省一层借用）。
fn iface_type(tt: &TypeTable, id: rgoc_types::InterfaceId) -> rgoc_types::TypeId {
    tt.interface_type(id)
}

// ── 环守卫（`C7` §5.1）────────────────────────────────────────────────────

/// 建一个**匿名环**：`interface{ M(T) }` 里 `T` 的约束就是那个接口。
///
/// 形状来自语料实测（`Task 35` 的普查）：`types/testdata/fixedbugs/issue43056.go`
/// 的 `func g[T interface{ Equal(T) bool }](a, b T)`。环上**没有**定义型
/// （`named`/`alias`）来「先占号」⇒ 后序走访不终止 —— rgoc 修前是
/// `fatal runtime error: stack overflow`（SIGABRT），oracle 侧按 `C7` §5.1 panic。
fn cyclic_tt() -> (TypeTable, rgoc_types::TypeId) {
    let mut tt = TypeTable::new();
    // 约束**先占位**（`invalid`），等接口建好再回填 —— 环正是在这里长出来的
    let inv = tt.invalid();
    let tp = tt.add_type_param(0, inv);
    let tp_ty = tt.type_param_type(tp);
    let params = tt.add_tuple(vec![(None, tp_ty)]);
    let results = tt.add_tuple(Vec::new());
    let sig = tt.add_signature(None, params, results, false);
    let iface = tt.add_interface(vec![("M".to_string(), tt.signature_type(sig))], None, false);
    let iface_ty = tt.interface_type(iface);
    tt.set_type_param_constraint(tp, iface_ty);
    (tt, iface_ty)
}

/// 环 ⇒ **拒绝产出**（可判定版本），而不是把栈踩爆。
#[test]
fn c7_匿名环拒绝产出() {
    let (tt, root) = cyclic_tt();
    let err = rgoc_types::try_dump_types(&tt, &[root]).unwrap_err();
    assert_eq!(err, rgoc_types::DumpError::CyclicTypeGraph);
    // 反向：同一张图上**摘掉环**（约束换成空接口）⇒ 正常产出（否则上一条
    // 可能只是「凡类型参数就拒绝」）
    let mut tt = TypeTable::new();
    let any = tt.add_interface(Vec::new(), None, false);
    let any_ty = tt.interface_type(any);
    let tp = tt.add_type_param(0, any_ty);
    let tp_ty = tt.type_param_type(tp);
    let params = tt.add_tuple(vec![(None, tp_ty)]);
    let results = tt.add_tuple(Vec::new());
    let sig = tt.add_signature(None, params, results, false);
    let iface = tt.add_interface(vec![("M".to_string(), tt.signature_type(sig))], None, false);
    let iface_ty = tt.interface_type(iface);
    assert!(
        rgoc_types::try_dump_types(&tt, &[iface_ty]).is_ok(),
        "无环的类型参数必须照常 dump"
    );
}

/// 反向：**未检查**版本（`dump_types`）撞上环时**响亮地炸** —— 它是对外的
/// `C7` 事实源（`export_summary` 用它），不许静默给半份 dump。
#[test]
#[should_panic(expected = "类型图成环")]
fn c7_匿名环_unchecked_响亮地炸() {
    let (tt, root) = cyclic_tt();
    let _ = dump_types(&tt, &[root]);
}
