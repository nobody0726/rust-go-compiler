//! `T-CK-11` 底座：可赋值性判定的验收（M4/Task 6）。
//!
//! # 期望值从哪来
//!
//! 语义从 `go/types/operand.go::assignableTo`（go1.27，`F6`）与
//! `go/types/expr.go::implicitTypeAndValue` 的**非 constant 分支**逐条读出，
//! 并用容器内 `types.AssignableTo` 真机探针复核过边界形态
//! （untyped 六类 / 接口指针 / `Invalid` / 具名-非具名 / 通道方向；
//! 记录见 `.superpowers/sdd/M4-plan/progress.md` 的 Task 6 行）。
//! **不是**抄 oracle 输出 —— oracle 对撞（`types --cmp assignable`）由 Task 15 接线。
//!
//! # 三档断言
//!
//! * [`yes`] / [`no`]：判得出（`Ok(true)` / `Ok(false)`）；
//! * [`refused`]：显式拒绝（`Err`）—— Task 6 落地的**窄面**，Task 7/8/10/12 逐步
//!   收窄（每条拒绝断言上方的注释都标明「谁在什么时候把它翻成判定」）。
//!
//! # 取型
//!
//! 官方 `go_source_code/src/internal/types/testdata/spec/assignability.go`
//! 的各小节形状（identity / 底层同一 / untyped / 通道 / 类型参数）与 `examples/*`。

use rgoc_const::BasicId;
use rgoc_lex::diag::DiagCode;
use rgoc_types::table::Field;
use rgoc_types::{ChanDir, IfaceTSet, TypeId, TypeTable, assignable};

/// 判得出且为真。
fn yes(tt: &TypeTable, v: TypeId, t: TypeId) {
    assert_eq!(assignable(tt, v, t), Ok(true), "应判「可赋值」");
}

/// 判得出且为假（**明确回答**：不是「不判」）。
fn no(tt: &TypeTable, v: TypeId, t: TypeId) {
    assert_eq!(assignable(tt, v, t), Ok(false), "应判「不可赋值」");
}

/// 显式拒绝：Task 6 窄面内「不判」的形态（返回拒绝码，供断言码本身）。
fn refused(tt: &TypeTable, v: TypeId, t: TypeId) -> DiagCode {
    assignable(tt, v, t).expect_err("Task 6 窄面内应显式拒绝，不许答错")
}

/// `type Name Underlying` —— 先占号再填底层（`table.rs` 的构造协议）。
fn named(tt: &mut TypeTable, name: &str, under: TypeId) -> TypeId {
    let n = tt.add_named(name);
    tt.set_named_underlying(n, under);
    tt.named_type(n)
}

/// 无参无结果签名 `func()`。
fn sig0(tt: &mut TypeTable) -> TypeId {
    let t0 = tt.add_tuple(vec![]);
    let s = tt.add_signature(None, t0, t0, false);
    tt.signature_type(s)
}

/// 普通接口（无项集、非 comparable）。
fn iface(tt: &mut TypeTable, methods: Vec<(&str, TypeId)>) -> TypeId {
    let iid = tt.add_interface(
        methods
            .into_iter()
            .map(|(n, s)| (n.to_string(), s))
            .collect(),
        None,
        IfaceTSet::All,
    );
    tt.interface_type(iid)
}

#[test]
fn t_ck_11_规则一_identical_面() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let int8 = tt.add_basic(BasicId::Int8);
    let string = tt.add_basic(BasicId::String);
    let int32 = tt.add_basic(BasicId::Int32);
    // `rune` 与 `int32` 是同一个基本种类（`C7` §4.1 的实测订正）
    let rune_ = tt.add_basic(BasicId::Int32);
    yes(&tt, int_, int_);
    yes(&tt, string, string);
    yes(&tt, int32, rune_);
    no(&tt, int_, string);
    no(&tt, int8, int_);
    // 具名型的「同一」按定义身份；与基本型之间**不**因为底层相同就可赋值
    //（官方 `hasName`：`int` 也算「有名字」⇒ 规则 5 要求「至少一侧非具名」不成立）
    let n = named(&mut tt, "N", int_);
    yes(&tt, n, n);
    no(&tt, n, int_);
    no(&tt, int_, n);
}

#[test]
fn t_ck_11_别名_两侧透明() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);
    // `type T = int`：两侧都判前透明
    let t = tt.add_alias("T");
    tt.set_alias_rhs(t, int_);
    let ta = tt.alias_type(t);
    yes(&tt, ta, int_);
    yes(&tt, int_, ta);
    // 别名链（`type U = T`）整条剥掉
    let u = tt.add_alias("U");
    tt.set_alias_rhs(u, ta);
    let ua = tt.alias_type(u);
    yes(&tt, ua, int_);
    yes(&tt, int_, ua);
    yes(&tt, ta, ua);
    no(&tt, ua, string);
    // 别名指向**具名**类型：规则 5 照常（别名透明 ⇒ 与具名自身等效）
    let sl = tt.add_slice(int_);
    let m = named(&mut tt, "M", sl);
    let am = tt.add_alias("AM");
    tt.set_alias_rhs(am, m);
    let ama = tt.alias_type(am);
    yes(&tt, sl, ama);
    yes(&tt, ama, sl);
    yes(&tt, ama, m);
}

#[test]
fn t_ck_11_底层同一_至少一侧非具名() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    // 具名切片 ↔ 裸切片：两向皆真（底层同一 + 一侧非具名）
    let sl = tt.add_slice(int_);
    let m = named(&mut tt, "M", sl);
    yes(&tt, sl, m);
    yes(&tt, m, sl);
    yes(&tt, m, m);
    // 具名映射
    let mp = tt.add_map(int_, int_);
    let mm = named(&mut tt, "MM", mp);
    yes(&tt, mp, mm);
    yes(&tt, mm, mp);
    // 具名签名
    let sig = sig0(&mut tt);
    let f = named(&mut tt, "F", sig);
    yes(&tt, sig, f);
    yes(&tt, f, sig);
    // 具名结构体
    let sid = tt.add_struct(vec![Field {
        name: Some("x".to_string()),
        typ: int_,
        tag: None,
        embedded: false,
    }]);
    let st = tt.struct_type(sid);
    let s = named(&mut tt, "S", st);
    yes(&tt, st, s);
    yes(&tt, s, st);
    // 两侧都具名 ⇒ 假（哪怕底层相同、甚至两个都是 `int`）
    let a = named(&mut tt, "A", int_);
    let b = named(&mut tt, "B", int_);
    no(&tt, a, b);
    no(&tt, a, int_);
    no(&tt, int_, a);
    // 具名切片两枚互不相容；具名指针两枚互不相容
    let m2 = named(&mut tt, "M2", sl);
    no(&tt, m, m2);
    let p = tt.add_pointer(int_);
    let pa = named(&mut tt, "PA", p);
    let pb = named(&mut tt, "PB", p);
    yes(&tt, p, pa);
    yes(&tt, pa, p);
    no(&tt, pa, pb);
}

#[test]
fn t_ck_11_通道面_双向到单向() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);
    let ch = tt.add_chan(ChanDir::SendRecv, int_);
    let recv = tt.add_chan(ChanDir::RecvOnly, int_);
    let send = tt.add_chan(ChanDir::SendOnly, int_);
    let chstr = tt.add_chan(ChanDir::SendRecv, string);
    // 双向 → 单向：真（方向规则 + 一侧非具名）
    yes(&tt, ch, recv);
    yes(&tt, ch, send);
    yes(&tt, ch, ch);
    // 单向 → 双向 / 异元素：假
    no(&tt, recv, ch);
    no(&tt, send, ch);
    no(&tt, ch, chstr);
    no(&tt, chstr, ch);
    // 具名通道 ↔ 裸通道（一侧非具名 ⇒ 真）
    let c = named(&mut tt, "C", ch);
    yes(&tt, c, recv);
    yes(&tt, ch, c);
    yes(&tt, c, c);
    // 两侧具名 ⇒ 假
    let d = named(&mut tt, "D", ch);
    no(&tt, c, d);
    // 具名单向通道：与裸**同向**通道照规则 5 真（与方向规则无关 —— V 非双向）；
    // 到裸双向通道为假（官方：x 必须是双向通道值）
    let r = named(&mut tt, "R", recv);
    yes(&tt, r, recv);
    yes(&tt, recv, r);
    no(&tt, r, ch);
    // 裸双向 → 具名单向：方向规则命中（一侧非具名）
    yes(&tt, ch, r);
}

#[test]
fn t_ck_11_接口面_窄实现与拒绝() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let m_sig = sig0(&mut tt);
    let any = iface(&mut tt, vec![]);
    let mi = iface(&mut tt, vec![("M", m_sig)]);
    let mj = iface(&mut tt, vec![("J", m_sig)]);
    // 空接口恒实现（官方 implements：`Ti.Empty()` ⇒ true）
    yes(&tt, int_, any);
    let n = named(&mut tt, "N", int_);
    yes(&tt, n, any);
    let p = tt.add_pointer(int_);
    yes(&tt, p, any);
    // 接口值（非空）也实现空接口
    yes(&tt, mi, any);
    yes(&tt, mi, mi);
    // 指向接口的指针是**普通值**，照样实现空接口（探针：`*iface → iface{}` = true）
    let pany = tt.add_pointer(any);
    yes(&tt, pany, any);
    // 「空接口 V → 带方法的 T」：V 没有任何方法 ⇒ 恒假
    //（官方 `hasAllMethods` 先于可比性检查）
    no(&tt, any, mi);
    // 「可证无方法的 V（基本型 / 无名字面量 / 指向它们）→ 带方法的 T」：恒假
    let sl = tt.add_slice(int_);
    no(&tt, int_, mi);
    no(&tt, sl, mi);
    // V 是**具名**类型：Task 8 起走真方法集 —— N 没有方法 ⇒ 缺 M ⇒ 假
    no(&tt, n, mi);
    // 两个非空接口：方法名不同（M vs J）⇒ 缺 J ⇒ 假
    no(&tt, mi, mj);
    // 具名类型**带**方法 M ⇒ 真（签名与 T 的同一条目）
    let k_n = tt.add_named("K");
    tt.set_named_underlying(k_n, int_);
    tt.add_named_method(k_n, "M", m_sig);
    yes(&tt, tt.named_type(k_n), mi);
    // 同名但签名不同 ⇒ 假（`identical` 判签名：func() ≠ func() int）
    let p0 = tt.add_tuple(vec![]);
    let r1 = tt.add_tuple(vec![(None, int_)]);
    let s1 = tt.add_signature(None, p0, r1, false);
    let sig_ret_int = tt.signature_type(s1);
    let l_n = tt.add_named("L");
    tt.set_named_underlying(l_n, int_);
    tt.add_named_method(l_n, "M", sig_ret_int);
    no(&tt, tt.named_type(l_n), mi);
    // 指向接口的指针作**目标**：官方 `implements(V, *I)` 恒假 ⇒ `Vp == nil` 时直接定假
    let pmi = tt.add_pointer(mi);
    no(&tt, int_, pmi);
    no(&tt, mi, pmi);
    no(&tt, any, pmi);
}

#[test]
fn t_ck_11_untyped_两侧都_untyped() {
    let mut tt = TypeTable::new();
    let ub = tt.add_basic(BasicId::UntypedBool);
    let ui = tt.add_basic(BasicId::UntypedInt);
    let ur = tt.add_basic(BasicId::UntypedRune);
    let uf = tt.add_basic(BasicId::UntypedFloat);
    let uc = tt.add_basic(BasicId::UntypedComplex);
    let us = tt.add_basic(BasicId::UntypedString);
    let un = tt.add_basic(BasicId::UntypedNil);
    // 两侧都是 untyped numeric：官方 `maxType` ⇒ 任意方向皆真
    yes(&tt, ui, uf);
    yes(&tt, uf, ui);
    yes(&tt, ur, ui);
    yes(&tt, ui, uc);
    yes(&tt, uc, ui);
    // 一侧非 numeric / 含 nil ⇒ 假
    no(&tt, ui, us);
    no(&tt, us, ui);
    no(&tt, ub, ui);
    no(&tt, ub, us);
    no(&tt, un, ui);
    no(&tt, un, us);
}

#[test]
fn t_ck_11_untyped_到具体与接口() {
    let mut tt = TypeTable::new();
    let bool_ = tt.add_basic(BasicId::Bool);
    let int_ = tt.add_basic(BasicId::Int);
    let int32b = tt.add_basic(BasicId::Int32);
    let int64 = tt.add_basic(BasicId::Int64);
    let uint8 = tt.add_basic(BasicId::Uint8);
    let float32 = tt.add_basic(BasicId::Float32);
    let float64 = tt.add_basic(BasicId::Float64);
    let complex64 = tt.add_basic(BasicId::Complex64);
    let complex128 = tt.add_basic(BasicId::Complex128);
    let string = tt.add_basic(BasicId::String);
    let unsafe_ptr = tt.add_basic(BasicId::UnsafePointer);
    let ub = tt.add_basic(BasicId::UntypedBool);
    let ui = tt.add_basic(BasicId::UntypedInt);
    let ur = tt.add_basic(BasicId::UntypedRune);
    let uf = tt.add_basic(BasicId::UntypedFloat);
    let uc = tt.add_basic(BasicId::UntypedComplex);
    let us = tt.add_basic(BasicId::UntypedString);
    // 数值 untyped → 任意数值基底：**类型层**为真（哪怕 300 放不进 uint8 ——
    // 值层可表示性是检查器拿值问 `rgoc-const` 的事，Task 22）
    yes(&tt, ui, int_);
    yes(&tt, ui, int64);
    yes(&tt, ui, uint8);
    yes(&tt, ui, float64);
    yes(&tt, ui, complex128);
    yes(&tt, ur, int32b);
    yes(&tt, ur, int_);
    yes(&tt, uf, float32);
    yes(&tt, uc, complex64);
    yes(&tt, uc, complex128);
    yes(&tt, ub, bool_);
    yes(&tt, us, string);
    // 跨类别一律假
    no(&tt, ui, string);
    no(&tt, ui, bool_);
    no(&tt, ui, unsafe_ptr);
    no(&tt, ub, int_);
    no(&tt, us, int_);
    no(&tt, uc, bool_);
    // 具名基底走 Underlying
    let a = named(&mut tt, "A", int_);
    let s = named(&mut tt, "S", string);
    yes(&tt, ui, a);
    yes(&tt, us, s);
    no(&tt, us, a);
    // 接口目标：非 nil untyped 只进**空**接口
    let any = iface(&mut tt, vec![]);
    let m_sig = sig0(&mut tt);
    let mi = iface(&mut tt, vec![("M", m_sig)]);
    yes(&tt, ui, any);
    yes(&tt, ub, any);
    yes(&tt, us, any);
    no(&tt, ui, mi);
    no(&tt, ub, mi);
    no(&tt, us, mi);
    // 含项集的接口同样「非空」⇒ 假（untyped 面直接回答，不依赖 Task 8）
    let u = tt.add_union(vec![(true, int_), (false, string)]);
    let uterms = tt.union_type(u);
    let tid = tt.add_interface(
        vec![],
        Some(uterms),
        IfaceTSet::Terms(vec![(true, int_), (false, string)]),
    );
    let terms_iface = tt.interface_type(tid);
    no(&tt, ui, terms_iface);
}

#[test]
fn t_ck_11_untyped_nil_面() {
    let mut tt = TypeTable::new();
    let un = tt.add_basic(BasicId::UntypedNil);
    let int_ = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);
    let unsafe_ptr = tt.add_basic(BasicId::UnsafePointer);
    // 可容纳 nil 的形态：真
    let p = tt.add_pointer(int_);
    let sl = tt.add_slice(int_);
    let mp = tt.add_map(int_, int_);
    let ch = tt.add_chan(ChanDir::SendRecv, int_);
    let sig = sig0(&mut tt);
    let any = iface(&mut tt, vec![]);
    let m_sig = sig0(&mut tt);
    let mi = iface(&mut tt, vec![("M", m_sig)]);
    yes(&tt, un, p);
    yes(&tt, un, sl);
    yes(&tt, un, mp);
    yes(&tt, un, ch);
    yes(&tt, un, sig);
    yes(&tt, un, any);
    // 非空接口也真（官方 `x.isNil()` 先于 `Empty` 检查；探针复核）
    yes(&tt, un, mi);
    // `unsafe.Pointer` 是「含 nil 的基本型」（官方 `hasNil` 的 Basic 分支）
    yes(&tt, un, unsafe_ptr);
    // 具名到这些底层的形态：真（走 Underlying）
    let np = named(&mut tt, "P", p);
    let ns = named(&mut tt, "S", sl);
    let nup = named(&mut tt, "UP", unsafe_ptr);
    yes(&tt, un, np);
    yes(&tt, un, ns);
    yes(&tt, un, nup);
    // 不含 nil 的形态：假
    no(&tt, un, int_);
    no(&tt, un, string);
    let na = named(&mut tt, "A", int_);
    no(&tt, un, na);
    let sid = tt.add_struct(vec![Field {
        name: Some("x".to_string()),
        typ: int_,
        tag: None,
        embedded: false,
    }]);
    let st = tt.struct_type(sid);
    no(&tt, un, st);
    let arr = tt.add_array(None, int_);
    no(&tt, un, arr);
}

#[test]
fn t_ck_11_类型参数_边界与拒绝() {
    let mut tt = TypeTable::new();
    let int_ = tt.add_basic(BasicId::Int);
    let ui = tt.add_basic(BasicId::UntypedInt);
    let any = iface(&mut tt, vec![]);
    let m_sig = sig0(&mut tt);
    let mi = iface(&mut tt, vec![("M", m_sig)]);
    let t1id = tt.add_type_param(0, any);
    let t1 = tt.type_param_type(t1id);
    let t2id = tt.add_type_param(1, any);
    let t2 = tt.type_param_type(t2id);
    // 同一条目 ⇒ 规则 1（官方的「同一对象」）
    yes(&tt, t1, t1);
    // 类型参数出现在**左侧**（除同一条目）：`identical` 的 M3 边界先拒
    //（`U-M3-05`；Task 10 升级 `identical` 后改判 —— 届时本组断言要同步改写）
    assert_eq!(
        refused(&tt, t1, any),
        DiagCode::TypUnsupportedConstraintIdentity
    );
    assert_eq!(
        refused(&tt, t1, mi),
        DiagCode::TypUnsupportedConstraintIdentity
    );
    assert_eq!(
        refused(&tt, t1, int_),
        DiagCode::TypUnsupportedConstraintIdentity
    );
    assert_eq!(
        refused(&tt, t1, t2),
        DiagCode::TypUnsupportedConstraintIdentity
    );
    // 类型参数出现在**右侧**：`identical` 给 Ok(false)（x 是具体形态），
    // 随后落入本任务的类型参数面（官方按「每个 specific type」递归 ⇒ 要类型集枚举）
    assert_eq!(
        refused(&tt, int_, t1),
        DiagCode::TypUnsupportedAssignability
    );
    assert_eq!(refused(&tt, ui, t1), DiagCode::TypUnsupportedAssignability);
    assert_eq!(refused(&tt, any, t1), DiagCode::TypUnsupportedAssignability);
}

#[test]
fn t_ck_11_invalid_宽容面() {
    let mut tt = TypeTable::new();
    let inv = tt.invalid();
    let int_ = tt.add_basic(BasicId::Int);
    let string = tt.add_basic(BasicId::String);
    // 「避免连锁错误」面对**目标**：`!isValid(T)` ⇒ 真
    yes(&tt, int_, inv);
    yes(&tt, inv, inv);
    // 源是 invalid **不**提前真（官方的 `x.isValid()` 是操作数**模式**面；
    // `types.AssignableTo` 的实测：`Invalid → int` = false）
    no(&tt, inv, int_);
    no(&tt, inv, string);
    // 空接口面同样宽容（官方 implements 开头 `!isValid(Vu)` ⇒ true）
    let any = iface(&mut tt, vec![]);
    yes(&tt, inv, any);
}
