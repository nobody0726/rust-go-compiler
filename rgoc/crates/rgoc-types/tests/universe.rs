//! `T-TY-51`：universe 的预声明对象（M3/Task 15）。
//!
//! 判据是「**与 oracle 一致**」——但 universe **不能**用 dump 对撞：
//! ① 官方侧没有「枚举 universe」的公开接口；② 与 universe 无关的 fixture 里
//! 它的条目根本不会出现在 dump 里（dump 只走访根可达的类型）。
//!
//! 所以这里的做法是：**把实测到的身份写进断言**。每条断言都能追到一次
//! oracle 实测（`tools/types-oracle` + `C7`）或一处规范/官方源码：
//!
//! | 断言 | 出处 |
//! |---|---|
//! | 名字全集（44） | Go 规范 Universe block 一节 |
//! | `byte` ⇒ `basic uint8`、`rune` ⇒ `basic int32` | oracle 实测（`var a byte`…） |
//! | `error` ⇒ `named` + `interface` + `method "Error"` + recv 是自己 | oracle 实测 |
//! | `any` ⇒ `alias` + 右侧 `interface 0 0` | oracle 实测 |
//! | `comparable` 是**带 comparable 的类型集**、不是普通类型名 | `F8`（`types2/universe.go:110-112,253`） |

use rgoc_const::{BasicId, Const};
use rgoc_types::object::ObjKind;
use rgoc_types::{IfaceTSet, Type, TypeTable, declared_names, universe};

#[test]
fn 名字全集恰好是那_44_个() {
    let (st, u, _tt) = universe();
    let s = st.scope(u);
    let mut got: Vec<&str> = s.names().collect();
    got.sort_unstable();
    let mut want: Vec<&str> = vec![
        // 类型名 22
        "bool",
        "byte",
        "comparable",
        "complex64",
        "complex128",
        "error",
        "float32",
        "float64",
        "int",
        "int8",
        "int16",
        "int32",
        "int64",
        "rune",
        "string",
        "uint",
        "uint8",
        "uint16",
        "uint32",
        "uint64",
        "uintptr",
        "any",
        // 常量 3
        "true",
        "false",
        "iota",
        // nil 1
        "nil",
        // 内建 18（照规范 Universe block）
        "append",
        "cap",
        "clear",
        "close",
        "complex",
        "copy",
        "delete",
        "imag",
        "len",
        "make",
        "max",
        "min",
        "new",
        "panic",
        "print",
        "println",
        "real",
        "recover",
    ];
    want.sort_unstable();
    assert_eq!(got, want);
    assert_eq!(got.len(), 44);
    assert_eq!(got, declared_names());
}

#[test]
fn 种类分区正确() {
    let (st, u, _tt) = universe();
    let s = st.scope(u);
    // 内建与 Func **分开**：内建不能当值传递
    for n in ["len", "cap", "make", "new", "append"] {
        assert_eq!(s.lookup(n).map(|o| o.kind), Some(ObjKind::Builtin), "{n}");
    }
    // 三个常量
    for n in ["true", "false", "iota"] {
        assert_eq!(s.lookup(n).map(|o| o.kind), Some(ObjKind::Const), "{n}");
    }
    assert_eq!(s.lookup("nil").map(|o| o.kind), Some(ObjKind::Nil));
    assert_eq!(s.lookup("int").map(|o| o.kind), Some(ObjKind::TypeName));
    assert_eq!(s.lookup("error").map(|o| o.kind), Some(ObjKind::TypeName));
    // 名字大小写敏感：`Int` 不在
    assert!(s.lookup("Int").is_none());
}

#[test]
fn byte_与_rune_折到本体() {
    let (st, u, tt) = universe();
    let s = st.scope(u);
    let byte_ty = s.lookup("byte").and_then(|o| o.typ).expect("byte 有类型");
    let rune_ty = s.lookup("rune").and_then(|o| o.typ).expect("rune 有类型");
    // 实测 oracle：`var a byte` ⇒ `basic uint8`、`var b rune` ⇒ `basic int32`
    assert_eq!(tt.type_of(byte_ty), &Type::Basic(BasicId::Uint8));
    assert_eq!(tt.type_of(rune_ty), &Type::Basic(BasicId::Int32));
}

#[test]
fn error_是_named_且本体是_error_方法集() {
    let (st, u, tt) = universe();
    let s = st.scope(u);
    let ty = s.lookup("error").and_then(|o| o.typ).expect("error 有类型");
    // 实测 oracle：`var e error` ⇒ `named` + `interface 1 0` + `method "Error"`
    let Type::Named(nid) = tt.type_of(ty) else {
        panic!("error 应当是 named（实测：不是基本类型、也不是 alias）")
    };
    assert_eq!(tt.named_def(*nid).name, "error");
    let under = tt.named_underlying(*nid);
    let Type::Interface(iid) = tt.type_of(under) else {
        panic!("error 的本体应当是 interface")
    };
    let def = tt.interface_def(*iid);
    assert_eq!(def.methods.len(), 1, "只有一个 Error 方法");
    assert_eq!(def.methods[0].0, "Error");
    assert_eq!(def.tset, IfaceTSet::All, "error 不是带 comparable 的类型集");
    assert!(def.terms.is_none(), "error 没有项集");
    // 方法的 recv 是 **None**：接口方法没有接收者。
    // ⚠️ 这里订正过一次口径（M3/Task 33 对撞）：**旧**写法给接口方法也设了
    // 接收者（go/types 内部确实这么存 —— 实测 `Signature.Recv()` 是接口自己），
    // 但 C7 §4.2 的**渲染口径**是 `-`（「非方法（普通函数 / **接口方法**）」），
    // oracle 侧也按 `-` 出 —— 两侧不一致的根因就在这一行。
    let Type::Signature(sig_id) = tt.type_of(def.methods[0].1) else {
        panic!("方法的类型应当是签名")
    };
    let sig = tt.signature_def(*sig_id);
    assert_eq!(sig.recv, None, "接口方法没有接收者（C7 §4.2）");
    assert!(!sig.variadic);
    // 结果是 `string`
    let Type::Tuple(results) = tt.type_of(sig.results) else {
        panic!("结果应当是元组")
    };
    assert_eq!(tt.tuple_def(*results).vars.len(), 1);
    assert_eq!(
        tt.type_of(tt.tuple_def(*results).vars[0].1),
        &Type::Basic(BasicId::String)
    );
    // 参数是空元组
    let Type::Tuple(params) = tt.type_of(sig.params) else {
        panic!("参数应当是元组")
    };
    assert!(tt.tuple_def(*params).vars.is_empty());
}

#[test]
fn any_是_alias_且右侧是空接口() {
    let (st, u, tt) = universe();
    let s = st.scope(u);
    let ty = s.lookup("any").and_then(|o| o.typ).expect("any 有类型");
    // 实测 oracle：`var y any` ⇒ `alias 9` + `9 interface 0 0`
    let Type::Alias(aid) = tt.type_of(ty) else {
        panic!("any 应当是 alias（实测）")
    };
    assert_eq!(tt.alias_def(*aid).name, "any");
    let rhs = tt.alias_rhs(*aid);
    let Type::Interface(iid) = tt.type_of(rhs) else {
        panic!("any 的右侧应当是 interface")
    };
    let def = tt.interface_def(*iid);
    assert!(def.methods.is_empty());
    assert!(def.terms.is_none());
    assert_eq!(
        def.tset,
        IfaceTSet::All,
        "any 的右侧是**空接口**，不是 comparable"
    );
}

#[test]
fn comparable_是特殊约束而不是普通类型名() {
    let (st, u, tt) = universe();
    let s = st.scope(u);
    let ty = s
        .lookup("comparable")
        .and_then(|o| o.typ)
        .expect("comparable 有类型");
    // 壳是 **named**（go1.27.1 实测：`Universe.Lookup("comparable").Type()`
    // 就是 `*types.Named`）；订正记录见 `universe.rs` ④ 的注释。
    let Type::Named(nid) = tt.type_of(ty) else {
        panic!("comparable 应当表示成 named（实测）")
    };
    assert_eq!(tt.named_def(*nid).name, "comparable");
    let Type::Interface(iid) = tt.type_of(tt.named_underlying(*nid)) else {
        panic!("comparable 的本体应当是 interface（F8）")
    };
    let def = tt.interface_def(*iid);
    // `F8`：它是**带 comparable 的类型集**，但方法集为空、无项集
    assert_eq!(
        def.tset,
        IfaceTSet::Comparable,
        "comparable 的类型集必须为真"
    );
    assert!(def.methods.is_empty());
    assert!(def.terms.is_none());
    // 与 `any` 的右侧**同形但不同标记** —— 这正是「表示做宽」要留住的那一点
    let any_rhs = tt.alias_rhs(
        *match tt.type_of(s.lookup("any").and_then(|o| o.typ).unwrap()) {
            Type::Alias(a) => a,
            other => panic!("any 应当是 alias，得到 {other:?}"),
        },
    );
    assert_ne!(tt.named_underlying(*nid), any_rhs, "两者是不同的条目");
    assert_eq!(
        tt.interface_def(match tt.type_of(any_rhs) {
            Type::Interface(i) => *i,
            other => panic!("any 的右侧应当是 interface，得到 {other:?}"),
        })
        .tset,
        IfaceTSet::All,
        "空接口的类型集是 𝓤"
    );
}

#[test]
fn 常量的值_true_false_有_iota_没有() {
    let (st, u, tt) = universe();
    let s = st.scope(u);
    assert_eq!(
        s.lookup("true").and_then(|o| o.val.clone()),
        Some(Const::Bool(true))
    );
    assert_eq!(
        s.lookup("false").and_then(|o| o.val.clone()),
        Some(Const::Bool(false))
    );
    // `iota` 有类型（untyped int）但**没有值**：它的值随组内位置变
    let iota = s.lookup("iota").expect("iota 存在");
    assert!(iota.val.is_none());
    assert!(!iota.has_value());
    assert_eq!(
        tt.type_of(iota.typ.expect("iota 有类型")),
        &Type::Basic(BasicId::UntypedInt)
    );
    // `nil` 的类型是 untyped nil
    assert_eq!(
        tt.type_of(s.lookup("nil").and_then(|o| o.typ).expect("nil 有类型")),
        &Type::Basic(BasicId::UntypedNil)
    );
}

#[test]
fn 内建函数只登记名字与种类() {
    let (st, u, _tt) = universe();
    let s = st.scope(u);
    for n in ["len", "cap", "make", "new", "panic", "recover"] {
        let o = s.lookup(n).expect(n);
        assert_eq!(o.kind, ObjKind::Builtin);
        // M3 不做调用检查 ⇒ 不编造签名（`open_gaps` 有记录）
        assert!(o.typ.is_none(), "{n} 不该有签名");
        assert!(o.pos.is_none());
    }
}

#[test]
fn 检查器的表可以自己声明_universe() {
    // `declare_universe` 的主用法：声明进检查器**同一张**表 ——
    // fixture 引用 `error` 时 dump 才能把它拉进来编号（实测 oracle 正是如此）
    let mut tt = TypeTable::new();
    let mut st = rgoc_types::ScopeTable::new();
    let u = rgoc_types::declare_universe(&mut tt, &mut st);
    let s = st.scope(u);
    assert_eq!(s.len(), 44);
    let error_ty = s.lookup("error").and_then(|o| o.typ).expect("有 error");
    assert_eq!(
        tt.type_of(error_ty).variant_label(),
        "named",
        "同表里也能取到"
    );
}
