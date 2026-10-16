//! `T-TY-76..78`：类型图合法性（非法递归 / 嵌套深度）—— M3/Task 22。
//!
//! 期望值来自 `go/types/validtype.go` 逐条读出的语义（`F9`），不是抄输出：
//! **「按值包含」的四条边（array / struct / union / interface）+ 具名类型本体**
//! 才可能成非法环；指针/切片/映射/通道/函数是**引用**，环经它们一律合法。

use rgoc_const::BasicId;
use rgoc_lex::diag::{Diag, DiagBag, DiagCode};
use rgoc_lex::{FileId, Pos};
use rgoc_types::table::Field;
use rgoc_types::{MAX_TYPE_NEST_DEPTH, TypeId, TypeTable, valid_type};

/// 跑一次判定，返回（是否合法，排序后的诊断）。
fn judge(tt: &TypeTable, t: TypeId) -> (bool, Vec<Diag>) {
    let mut bag = DiagBag::new();
    let ok = valid_type(tt, t, &mut bag);
    (ok, bag.into_sorted())
}

fn pos(line: u32) -> Pos {
    Pos {
        base: None,
        file: FileId::from_index(0),
        line,
        col: 1,
    }
}

/// 只断言「恰好一条、且是某个代号」，顺带把消息打出来便于失败时定位。
fn only_code(diags: &[Diag], code: DiagCode) -> bool {
    diags.len() == 1 && diags[0].code == code
}

#[test]
fn t_ty_76_非法递归_按值包含才成环() {
    // ① `type A [1]A` —— 环经 array ⇒ **非法**（官方 `validRecursive` 的典型例）
    let mut tt = TypeTable::new();
    let one = tt
        .consts_mut()
        .add(rgoc_const::Const::Int(rgoc_const::BigInt::from_i64(1)));
    let a = tt.add_named("A");
    tt.set_named_pos(a, pos(3));
    let arr = tt.add_array(Some(one), tt.named_type(a)); // 先占号：本体里引自己
    tt.set_named_underlying(a, arr);
    let (ok, diags) = judge(&tt, tt.named_type(a));
    assert!(!ok, "`type A [1]A` 必须判非法");
    assert!(only_code(&diags, DiagCode::TypInvalidRecursiveType));
    assert_eq!(diags[0].message, "invalid recursive type A");
    // 位置 = **环起点那个类型名声明处**（官方报在对象上）
    assert_eq!(diags[0].pos, pos(3));

    // ② `type A *A` —— 环经 pointer ⇒ **合法**（指针不「按值包含」被指对象）
    let mut tt = TypeTable::new();
    let a = tt.add_named("A");
    let p = tt.add_pointer(tt.named_type(a));
    tt.set_named_underlying(a, p);
    let (ok, diags) = judge(&tt, tt.named_type(a));
    assert!(ok, "`type A *A` 是合法 Go");
    assert!(diags.is_empty(), "不该报任何诊断，实际 {diags:?}");

    // ③ 切片 / 映射 / 通道 / 函数 / 元组：同理都合法
    let mut tt = TypeTable::new();
    let a = tt.add_named("A");
    let s = tt.add_slice(tt.named_type(a));
    tt.set_named_underlying(a, s);
    assert!(judge(&tt, tt.named_type(a)).1.is_empty());

    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let a = tt.add_named("A");
    let m = tt.add_map(int, tt.named_type(a));
    tt.set_named_underlying(a, m);
    assert!(judge(&tt, tt.named_type(a)).1.is_empty());

    let mut tt = TypeTable::new();
    let a = tt.add_named("A");
    let c = tt.add_chan(rgoc_types::ChanDir::SendRecv, tt.named_type(a));
    tt.set_named_underlying(a, c);
    assert!(judge(&tt, tt.named_type(a)).1.is_empty());

    // `type A func(A)` —— 官方不下钻签名 ⇒ 合法（且它在 Go 里确实合法）
    let mut tt = TypeTable::new();
    let a = tt.add_named("A");
    let empty = tt.add_tuple(Vec::new());
    let r = tt.add_tuple(vec![(None, tt.named_type(a))]);
    let sig = tt.add_signature(None, empty, r, false);
    tt.set_named_underlying(a, tt.signature_type(sig));
    assert!(judge(&tt, tt.named_type(a)).1.is_empty());

    // `type A interface{ M() A }` —— 方法集不进内存布局 ⇒ 合法
    let mut tt = TypeTable::new();
    let a = tt.add_named("A");
    let empty = tt.add_tuple(Vec::new());
    let r = tt.add_tuple(vec![(None, tt.named_type(a))]);
    let sig = tt.add_signature(None, empty, r, false);
    let i = tt.add_interface(vec![("M".to_string(), tt.signature_type(sig))], None, false);
    tt.set_named_underlying(a, tt.interface_type(i));
    assert!(judge(&tt, tt.named_type(a)).1.is_empty());

    // ④ `type A [1]*A` —— 环**经**array 但那里有 pointer 断开 ⇒ 合法
    let mut tt = TypeTable::new();
    let one = tt
        .consts_mut()
        .add(rgoc_const::Const::Int(rgoc_const::BigInt::from_i64(1)));
    let a = tt.add_named("A");
    let p = tt.add_pointer(tt.named_type(a));
    let arr = tt.add_array(Some(one), p);
    tt.set_named_underlying(a, arr);
    assert!(judge(&tt, tt.named_type(a)).1.is_empty());

    // ⑤ `type A struct{ a [2]A }` —— 环经 struct+array（都是按值）⇒ 非法
    let mut tt = TypeTable::new();
    let two = tt
        .consts_mut()
        .add(rgoc_const::Const::Int(rgoc_const::BigInt::from_i64(2)));
    let a = tt.add_named("A");
    let arr = tt.add_array(Some(two), tt.named_type(a));
    let st = tt.add_struct(vec![Field {
        name: Some("a".to_string()),
        typ: arr,
        tag: None,
        embedded: false,
    }]);
    tt.set_named_underlying(a, tt.struct_type(st));
    let (ok, diags) = judge(&tt, tt.named_type(a));
    assert!(!ok);
    assert!(only_code(&diags, DiagCode::TypInvalidRecursiveType));
}

#[test]
fn t_ty_77_环路径取后缀_且兄弟不算环() {
    // `type A B; type B A` ⇒ 两个类型的环；消息是 `A refers to B refers to A`
    let mut tt = TypeTable::new();
    let a = tt.add_named("A");
    let b = tt.add_named("B");
    tt.set_named_pos(a, pos(7));
    tt.set_named_pos(b, pos(8));
    tt.set_named_underlying(a, tt.named_type(b));
    tt.set_named_underlying(b, tt.named_type(a));
    let (ok, diags) = judge(&tt, tt.named_type(a));
    assert!(!ok);
    assert!(only_code(&diags, DiagCode::TypInvalidRecursiveType));
    assert_eq!(
        diags[0].message,
        "invalid recursive type: A refers to B refers to A"
    );
    assert_eq!(diags[0].pos, pos(7), "报在**环起点**（A），不是终点");

    // 起点不在根上：`C -> A -> B -> A` ⇒ 后缀从 A 开始（不是 `C refers to …`）
    let mut tt = TypeTable::new();
    let a = tt.add_named("A");
    let b = tt.add_named("B");
    let c = tt.add_named("C");
    tt.set_named_underlying(c, tt.named_type(a));
    tt.set_named_underlying(a, tt.named_type(b));
    tt.set_named_underlying(b, tt.named_type(a));
    let (ok, diags) = judge(&tt, tt.named_type(c));
    assert!(!ok);
    assert_eq!(
        diags[0].message,
        "invalid recursive type: A refers to B refers to A"
    );

    // **兄弟**不是环：`type B int; type A struct{ p B; q B }`
    // （官方用 `nest`（正在展开）而不是 `path`（走过）查环，所以出栈后再见不算）
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let b = tt.add_named("B");
    tt.set_named_underlying(b, int);
    let a = tt.add_named("A");
    let f1 = Field {
        name: Some("p".to_string()),
        typ: tt.named_type(b),
        tag: None,
        embedded: false,
    };
    let f2 = Field {
        name: Some("q".to_string()),
        typ: tt.named_type(b),
        tag: None,
        embedded: false,
    };
    let st = tt.add_struct(vec![f1, f2]);
    tt.set_named_underlying(a, tt.struct_type(st));
    let (ok, diags) = judge(&tt, tt.named_type(a));
    assert!(ok, "同一个类型作**两个兄弟**字段不是环");
    assert!(diags.is_empty());

    // 别名环归**声明收集**（官方注释：`type A = [10]A` 由 objDecl 的循环检测抓）
    // ⇒ 本层刻意**不**报（分工写进模块文档与 `open_gaps`）
    let mut tt = TypeTable::new();
    let one = tt
        .consts_mut()
        .add(rgoc_const::Const::Int(rgoc_const::BigInt::from_i64(10)));
    let al = tt.add_alias("AL");
    let arr = tt.add_array(Some(one), tt.alias_type(al));
    tt.set_alias_rhs(al, arr);
    let (ok, diags) = judge(&tt, tt.alias_type(al));
    assert!(ok, "别名环不在本层的职责内（有意的分工）");
    assert!(diags.is_empty());
}

#[test]
fn t_ty_78_嵌套深度上限_两侧边界与代号分开() {
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);

    // 恰 1000 层（= 上限）⇒ 合法。构造：`[1][1]…[1]int`
    let mut t = int;
    for _ in 0..MAX_TYPE_NEST_DEPTH {
        t = tt.add_array(None, t);
    }
    let (ok, diags) = judge(&tt, t);
    assert!(
        ok,
        "恰 {MAX_TYPE_NEST_DEPTH} 层合法（`> 上限` 才算超限，与 parser 同约定）"
    );
    assert!(diags.is_empty());

    // 1001 层 ⇒ 超限，**恰一条**，且代号是 `TypNestingTooDeep`
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let mut t = int;
    for _ in 0..=MAX_TYPE_NEST_DEPTH {
        t = tt.add_array(None, t);
    }
    let (ok, diags) = judge(&tt, t);
    assert!(!ok);
    assert!(only_code(&diags, DiagCode::TypNestingTooDeep), "{diags:?}");
    // ⚠️ **代号必须分开**：1001 层的 `[1]…[1]int` 在 Go 里是**合法类型**，
    //    报 `TypInvalidRecursiveType` 就是**假诊断**（说用户写错了）。
    assert_ne!(diags[0].code, DiagCode::TypInvalidRecursiveType);
    // 纯匿名嵌套没有具名类型可指 ⇒ 位置未知（登记为 open_gap）
    assert!(!diags[0].pos.is_known(), "匿名嵌套超限时位置未知");

    // 更深的也**只报一次**（1002、1100 层同样一条）
    for extra in [1usize, 100] {
        let mut tt = TypeTable::new();
        let int = tt.add_basic(BasicId::Int);
        let mut t = int;
        for _ in 0..(MAX_TYPE_NEST_DEPTH + extra + 1) {
            t = tt.add_array(None, t);
        }
        let (ok, diags) = judge(&tt, t);
        assert!(!ok);
        assert_eq!(diags.len(), 1, "一次判定只报一次，实际 {diags:?}");
        assert_eq!(diags[0].code, DiagCode::TypNestingTooDeep);
    }

    // 具名类型的深度超限：环路径起点是**外层那个具名类型** ⇒ 位置已知
    let mut tt = TypeTable::new();
    let int = tt.add_basic(BasicId::Int);
    let mut t = int;
    for _ in 0..=MAX_TYPE_NEST_DEPTH {
        t = tt.add_array(None, t);
    }
    let a = tt.add_named("A");
    tt.set_named_pos(a, pos(11));
    tt.set_named_underlying(a, t);
    let (ok, diags) = judge(&tt, tt.named_type(a));
    assert!(!ok);
    assert!(only_code(&diags, DiagCode::TypNestingTooDeep));
    assert_eq!(
        diags[0].pos,
        pos(11),
        "有具名类型时位置取它（`set_named_pos` 的作用）"
    );
}
