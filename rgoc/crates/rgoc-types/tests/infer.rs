//! `T-CK-22/23/24` 底座：类型推断三入口（`infer`）与形参改名（M4/Task 14）。
//!
//! # 判据来源
//!
//! 官方 `go/types/infer.go`：
//!
//! * `check.infer`（`:35`）—— 唯一主入口。三段式：
//!   ① 实参与形参统一（`u.unify(par, arg, assign)`；untyped 实参只可能对上
//!   **直接类型参数**形参，收集进 ③ 段）；
//!   ② 约束信息（单精确项 ⇒ `u.set`；核心项 ⇒ `u.unify(tx, core, loose)`；
//!   已绑定且**非参数化**的 `tx` ⇒ 对约束方法集逐一 `u.unify(…, exact)`）；
//!   ③ 未定型常量（`maxType` 取最大种类 ⇒ `Default` 置位）；
//!   随后 `killCycles` + 迭代化简（`u.inferred` → `subst` 至不动点）+ 终检。
//! * `renameTParams`（`:471`）—— 自递归「同一个 `P` 既是求解对象又出现在实参里」
//!   的隔离（`f[P](x P) { f(x) }`）；调用点在 call 面（`call.go:570/597`），
//!   本层提供同名助手，且 ② 入口自带一份。
//! * 三个入口对应官方三个调用形状：
//!   ① `call.go:626`（普通实参推断；1.18 语义）；
//!   ② `call.go:580-617`（泛型函数实参并入推断集：`f(g)`、`f(g, g)`、
//!   `f(g2[int])` ——「implicitly instantiated function as argument」）；
//!   ③ `funcInst` 的 reverse 形状（`call.go:71-130`；触发点 `expr.go:1011`
//!   nonGeneric 与 `:1073-1079` IndexExpr）—— 赋值/接口方法语境（1.21）
//!   与赋值上下文函数类型（1.27；`enableReverseTypeInference`「Available
//!   with go1.21」，`infer.go:24`）。
//!
//! # 返回形态
//!
//! * `Ok(Some(targs))` —— 推断成功（长度 == 形参数）；
//! * `Ok(None)` —— **静默放弃**：实参含 `invalid`（官方 `:65-71`「错误已报，
//!   不长连锁诊断」）；
//! * `Err(TypCannotInferTypeArgs)` —— 真失败。官方码 `CannotInferTypeArgs`；
//!   `infer.go:141-144` 的注释明确：**用户所给实参的毛病也统一落这条**
//!   （它也可能更该归 `InvalidTypeArg`，无法分辨 ⇒ 取更泛者）。
//!
//! # v1 边界（在模块文档声明、测试不覆盖）
//!
//! * **约束复验不在此层**：官方把约束校验放进 `check.later` 的 verify 面
//!   （`call.go:154`，码 `InvalidTypeArg`）。实测 go1.27.1：
//!   `f[P int8|int16](x P); f(1)` 在推断层得 `P=int`（③ 段默认化，不回头），
//!   由 verify 报 `InvalidTypeArg`（消息 `int does not satisfy int8 | int16
//!   (int missing in int8 | int16)`，**无** `in call to` 前缀）。本层同口径
//!   （`t_ck_22_untyped实参_约束不复验` 把这层边界钉成向量）。
//! * 项表**不归一**（M4/Task 8 既有偏离）：单元素判定用项表**原始长度**；
//!   重复项的非法源（`int | int`）可能把单元素判成普通无核心 —— 以稳定拒绝
//!   收场（官方 `norm()` 后是 1 项、会置位）。
//! * `substitute` 不凿 `Named` 实例（M4/Task 12 既有边界）：形参里的平实例
//!   （`Box[P]`）不参与改名与化简。
//! * 接口推断默认**开**（= go1.21+；官方 `newUnifier(…, allowVersion(go1_21))`
//!   `infer.go:117`）—— <1.21 的收窄留给 check 面直控 `Unifier`。

use rgoc_const::BasicId;
use rgoc_lex::DiagCode;
use rgoc_types::{
    Field, GenericArgSite, IfaceTSet, NamedId, Type, TypeId, TypeParamId, TypeTable,
    infer_for_call, infer_from_args, infer_from_assignment, rename_tparams,
};

// ── 构造助手 ────────────────────────────────────────────────────────

fn int(tt: &mut TypeTable) -> TypeId {
    tt.add_basic(BasicId::Int)
}

fn string(tt: &mut TypeTable) -> TypeId {
    tt.add_basic(BasicId::String)
}

fn int8(tt: &mut TypeTable) -> TypeId {
    tt.add_basic(BasicId::Int8)
}

fn int16(tt: &mut TypeTable) -> TypeId {
    tt.add_basic(BasicId::Int16)
}

/// `untyped int` / `untyped float` / `untyped string` / `untyped nil`。
fn untyped(tt: &mut TypeTable, k: BasicId) -> TypeId {
    tt.add_basic(k)
}

/// `interface{}`（空接口，`tset = All`）—— `any` 的表示。
fn any(tt: &mut TypeTable) -> TypeId {
    let i = tt.add_interface(Vec::new(), None, IfaceTSet::All);
    tt.interface_type(i)
}

/// 单 `~T` 项接口。
fn tilde(tt: &mut TypeTable, t: TypeId) -> TypeId {
    let u = tt.add_union(vec![(true, t)]);
    let ut = tt.union_type(u);
    let i = tt.add_interface(Vec::new(), Some(ut), IfaceTSet::Terms(vec![(true, t)]));
    tt.interface_type(i)
}

/// 项表接口（`tset = Terms`；语法项集也建 `union`）。
fn terms_iface(tt: &mut TypeTable, terms: &[(bool, TypeId)]) -> TypeId {
    let u = tt.add_union(terms.to_vec());
    let ut = tt.union_type(u);
    let i = tt.add_interface(Vec::new(), Some(ut), IfaceTSet::Terms(terms.to_vec()));
    tt.interface_type(i)
}

/// 记录一个形参条目：`T(idx)`，约束为 `c`。
fn param(tt: &mut TypeTable, idx: u32, c: TypeId) -> TypeParamId {
    tt.add_type_param(idx, c)
}

/// `func(参数…) 结果…` 的签名条目。
fn sig_n(tt: &mut TypeTable, params: &[TypeId], results: &[TypeId], variadic: bool) -> TypeId {
    let p = tt.add_tuple(params.iter().map(|t| (None, *t)).collect());
    let r = tt.add_tuple(results.iter().map(|t| (None, *t)).collect());
    let s = tt.add_signature(None, p, r, variadic);
    tt.signature_type(s)
}

/// `struct{字段…}`。
fn strct(tt: &mut TypeTable, fields: Vec<(&str, TypeId)>) -> TypeId {
    let fs = fields
        .into_iter()
        .map(|(n, t)| Field {
            name: Some(n.to_string()),
            typ: t,
            tag: None,
            embedded: false,
        })
        .collect();
    let s = tt.add_struct(fs);
    tt.struct_type(s)
}

/// 裸具名：`type Name Under`。
fn plain(tt: &mut TypeTable, name: &str, under: TypeId) -> NamedId {
    let n = tt.add_named(name);
    tt.set_named_underlying(n, under);
    n
}

/// 带方法集的接口。
fn iface_of(tt: &mut TypeTable, methods: Vec<(&str, TypeId)>) -> TypeId {
    let i = tt.add_interface(
        methods
            .into_iter()
            .map(|(n, s)| (n.to_string(), s))
            .collect(),
        None,
        IfaceTSet::All,
    );
    tt.interface_type(i)
}

/// 取签名第一个参数的类型（断言改名结果用）。
fn sig_first_param(tt: &TypeTable, sig: TypeId) -> TypeId {
    let Type::Signature(s) = tt.type_of(sig) else {
        panic!("不是签名：{sig:?}");
    };
    let d = tt.signature_def(*s);
    let Type::Tuple(t) = tt.type_of(d.params) else {
        panic!("参数不是元组");
    };
    tt.tuple_def(*t).vars[0].1
}

// ── T-CK-22 ①：1.18 实参模式（官方 `call.go:626` 形状）────────────────

#[test]
fn t_ck_22_实参对形参_基本() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let pt = tt.type_param_type(p);
    let i = int(&mut tt);
    let res = infer_from_args(&mut tt, &[p], &[], &[pt], &[i]);
    assert_eq!(res, Ok(Some(vec![i])));
}

#[test]
fn t_ck_22_嵌套泛型类型() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let q = param(&mut tt, 1, a);
    let pt = tt.type_param_type(p);
    let qt = tt.type_param_type(q);
    let sl_p = tt.add_slice(pt);
    let mp_qp = tt.add_map(qt, pt);
    let i = int(&mut tt);
    let s = string(&mut tt);
    let sl_i = tt.add_slice(i);
    let mp_si = tt.add_map(s, i);
    // `[]P`/`map[Q]P` 对 `[]int`/`map[string]int`：P=int、Q=string。
    let res = infer_from_args(&mut tt, &[p, q], &[], &[sl_p, mp_qp], &[sl_i, mp_si]);
    assert_eq!(res, Ok(Some(vec![i, s])));
}

#[test]
fn t_ck_22_部分显式实参_回填其余() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let q = param(&mut tt, 1, a);
    let pt = tt.type_param_type(p);
    let qt = tt.type_param_type(q);
    let i = int(&mut tt);
    let s = string(&mut tt);
    // 显式给出 P=int，Q 由实参回填。
    let res = infer_from_args(&mut tt, &[p, q], &[Some(i)], &[pt, qt], &[i, s]);
    assert_eq!(res, Ok(Some(vec![i, s])));
}

#[test]
fn t_ck_22_部分显式实参_缺信息者失败() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let q = param(&mut tt, 1, a);
    let pt = tt.type_param_type(p);
    let i = int(&mut tt);
    // Q 既无显式实参、也不出现在形参里 ⇒ 终检失败。
    let res = infer_from_args(&mut tt, &[p, q], &[Some(i)], &[pt], &[i]);
    assert_eq!(res, Err(DiagCode::TypCannotInferTypeArgs));
}

#[test]
fn t_ck_22_约束单精确项_自足() {
    let mut tt = TypeTable::new();
    let i8 = int8(&mut tt);
    let c = terms_iface(&mut tt, &[(false, i8)]);
    let p = param(&mut tt, 0, c);
    // `[P interface{ int8 }]`：② 段单精确项直接置位（`single && !tilde`）。
    let res = infer_from_args(&mut tt, &[p], &[], &[], &[]);
    assert_eq!(res, Ok(Some(vec![i8])));
}

#[test]
fn t_ck_22_约束波浪项_不置位() {
    let mut tt = TypeTable::new();
    let i8 = int8(&mut tt);
    let c = tilde(&mut tt, i8);
    let p = param(&mut tt, 0, c);
    // `~int8` 的 tilde ⇒ 不满足 `!core.tilde`，不得置位；无其他信息 ⇒ 失败。
    let res = infer_from_args(&mut tt, &[p], &[], &[], &[]);
    assert_eq!(res, Err(DiagCode::TypCannotInferTypeArgs));
}

#[test]
fn t_ck_22_约束项驱动_嵌套绑定() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let pa = param(&mut tt, 0, a);
    let at = tt.type_param_type(pa);
    // B 的约束 `~struct{f []A}`。
    let sl_a = tt.add_slice(at);
    let st_a = strct(&mut tt, vec![("f", sl_a)]);
    let bc = tilde(&mut tt, st_a);
    let pb = param(&mut tt, 1, bc);
    let bt = tt.type_param_type(pb);
    // 实参 `struct{f []string}`：B 绑定后，② 段用核心项把 A 反推为 string。
    let s = string(&mut tt);
    let sl_s = tt.add_slice(s);
    let st_s = strct(&mut tt, vec![("f", sl_s)]);
    let res = infer_from_args(&mut tt, &[pa, pb], &[], &[bt], &[st_s]);
    assert_eq!(res, Ok(Some(vec![s, st_s])));
}

#[test]
fn t_ck_22_冲突绑定_失败() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let pt = tt.type_param_type(p);
    let i = int(&mut tt);
    let s = string(&mut tt);
    // 同一个 P 先对 int 再对 string ⇒ ② 段外冲突（① 段 unify 直接失败）。
    let res = infer_from_args(&mut tt, &[p], &[], &[pt, pt], &[i, s]);
    assert_eq!(res, Err(DiagCode::TypCannotInferTypeArgs));
}

#[test]
fn t_ck_22_untyped实参_默认类型() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let pt = tt.type_param_type(p);
    let ui = untyped(&mut tt, BasicId::UntypedInt);
    // ③ 段：untyped int 的默认类型 = int。默认类型由推断内部**新建**
    // （`add_basic` 不去重 ⇒ 事后建的 `int` 不是同一条目）——按语义断言。
    let res = infer_from_args(&mut tt, &[p], &[], &[pt], &[ui]);
    let got = res.expect("推断失败").expect("静默放弃");
    assert_eq!(got.len(), 1);
    assert!(matches!(tt.type_of(got[0]), Type::Basic(BasicId::Int)));
}

#[test]
fn t_ck_22_untyped数值_取最大种类() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let pt = tt.type_param_type(p);
    let ui = untyped(&mut tt, BasicId::UntypedInt);
    let uf = untyped(&mut tt, BasicId::UntypedFloat);
    // `maxType(untyped int, untyped float) = untyped float` ⇒ float64。
    let res = infer_from_args(&mut tt, &[p], &[], &[pt, pt], &[ui, uf]);
    let got = res.expect("推断失败").expect("静默放弃");
    assert_eq!(got.len(), 1);
    assert!(matches!(tt.type_of(got[0]), Type::Basic(BasicId::Float64)));
}

#[test]
fn t_ck_22_untyped种类冲突_失败() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let pt = tt.type_param_type(p);
    let ui = untyped(&mut tt, BasicId::UntypedInt);
    let us = untyped(&mut tt, BasicId::UntypedString);
    // `maxType(untyped int, untyped string) = nil` ⇒ 官方「mismatched types」失败。
    let res = infer_from_args(&mut tt, &[p], &[], &[pt, pt], &[ui, us]);
    assert_eq!(res, Err(DiagCode::TypCannotInferTypeArgs));
}

#[test]
fn t_ck_22_untyped_nil_不参与() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let pt = tt.type_param_type(p);
    let un = untyped(&mut tt, BasicId::UntypedNil);
    // untyped nil 不入 ③ 段（官方 `!arg.isNil()`），无信息 ⇒ 失败。
    let res = infer_from_args(&mut tt, &[p], &[], &[pt], &[un]);
    assert_eq!(res, Err(DiagCode::TypCannotInferTypeArgs));
}

#[test]
fn t_ck_22_无任何信息_失败() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    // `d[P any](...func(P))` 的 `d()` 形态：无形参无实参。
    let res = infer_from_args(&mut tt, &[p], &[], &[], &[]);
    assert_eq!(res, Err(DiagCode::TypCannotInferTypeArgs));
}

#[test]
fn t_ck_22_无效实参_静默放弃() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let pt = tt.type_param_type(p);
    let inv = tt.invalid();
    // 官方 `:65-71`：invalid 实参 ⇒ 静默（前序错误已报，不长连锁诊断）。
    let res = infer_from_args(&mut tt, &[p], &[], &[pt], &[inv]);
    assert_eq!(res, Ok(None));
}

#[test]
fn t_ck_22_untyped实参_约束不复验() {
    // 边界钉：`f[P int8|int16](x P); f(1)` 在**推断层**成功得 P=int
    // （官方 ③ 段默认化后不回头复验；拒绝由 verify 面以 `InvalidTypeArg` 报）。
    let mut tt = TypeTable::new();
    let i8 = int8(&mut tt);
    let i16 = int16(&mut tt);
    let c = terms_iface(&mut tt, &[(false, i8), (false, i16)]);
    let p = param(&mut tt, 0, c);
    let pt = tt.type_param_type(p);
    let ui = untyped(&mut tt, BasicId::UntypedInt);
    let res = infer_from_args(&mut tt, &[p], &[], &[pt], &[ui]);
    let got = res.expect("推断失败").expect("静默放弃");
    assert_eq!(got.len(), 1);
    assert!(matches!(tt.type_of(got[0]), Type::Basic(BasicId::Int)));
}

// ── T-CK-23 ②：1.21 实参位置与赋值/接口方法语境 ──────────────────────

#[test]
fn t_ck_23_实参位置_基本() {
    // `func f1(func(int)); f1(g1)`，`g1[P any](P)`。
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let pt = tt.type_param_type(p);
    let g1 = sig_n(&mut tt, &[pt], &[], false);
    let i = int(&mut tt);
    let f1p = sig_n(&mut tt, &[i], &[], false);
    let res = infer_for_call(
        &mut tt,
        &[],
        &[],
        &[f1p],
        &[g1],
        &[GenericArgSite {
            index: 0,
            tparams: vec![p],
            targs: vec![],
        }],
    );
    assert_eq!(res, Ok(Some(vec![i])));
}

#[test]
fn t_ck_23_实参位置_两实参各自改名() {
    // `f2(func(int), func(string)); f2(g1, g1)`（issue59956）：
    // 两次出现必须各自改名 —— 共享同一 P 会把第二次绑成冲突。
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let pt = tt.type_param_type(p);
    let g1 = sig_n(&mut tt, &[pt], &[], false);
    let i = int(&mut tt);
    let s = string(&mut tt);
    let f2p0 = sig_n(&mut tt, &[i], &[], false);
    let f2p1 = sig_n(&mut tt, &[s], &[], false);
    let res = infer_for_call(
        &mut tt,
        &[],
        &[],
        &[f2p0, f2p1],
        &[g1, g1],
        &[
            GenericArgSite {
                index: 0,
                tparams: vec![p],
                targs: vec![],
            },
            GenericArgSite {
                index: 1,
                tparams: vec![p],
                targs: vec![],
            },
        ],
    );
    assert_eq!(res, Ok(Some(vec![i, s])));
}

#[test]
fn t_ck_23_实参位置_与普通形参并存() {
    // `func k[C any](C, func(int)); k("x", g1)`：要求解集 = 被调者的 C + 实参的 P。
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let c = param(&mut tt, 0, a);
    let ct = tt.type_param_type(c);
    let p = param(&mut tt, 0, a);
    let pt = tt.type_param_type(p);
    let g1 = sig_n(&mut tt, &[pt], &[], false);
    let i = int(&mut tt);
    let s = string(&mut tt);
    let fp = sig_n(&mut tt, &[i], &[], false);
    let res = infer_for_call(
        &mut tt,
        &[c],
        &[],
        &[ct, fp],
        &[s, g1],
        &[GenericArgSite {
            index: 1,
            tparams: vec![p],
            targs: vec![],
        }],
    );
    // 结果 = 被调者在前（C=string），实参函数随后（P=int）。
    assert_eq!(res, Ok(Some(vec![s, i])));
}

#[test]
fn t_ck_23_实参位置_无信息失败() {
    // `d[P any](...func(P)); d()` 形态：收集不到任何信息。
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let res = infer_for_call(&mut tt, &[p], &[], &[], &[], &[]);
    assert_eq!(res, Err(DiagCode::TypCannotInferTypeArgs));
}

#[test]
fn t_ck_23_实参位置_部分显式() {
    // `func f(func(int) string); f(g2[int])`，`g2[P, Q any](P) Q`
    // （issue59958）：实参自带部分显式实参 [int]，Q 由签名统一回填。
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let q = param(&mut tt, 1, a);
    let pt = tt.type_param_type(p);
    let qt = tt.type_param_type(q);
    let g2 = sig_n(&mut tt, &[pt], &[qt], false);
    let i = int(&mut tt);
    let s = string(&mut tt);
    let fp = sig_n(&mut tt, &[i], &[s], false);
    let res = infer_for_call(
        &mut tt,
        &[],
        &[],
        &[fp],
        &[g2],
        &[GenericArgSite {
            index: 0,
            tparams: vec![p, q],
            targs: vec![Some(i)],
        }],
    );
    assert_eq!(res, Ok(Some(vec![i, s])));
}

#[test]
fn t_ck_23_赋值语境_基本() {
    // `var _ func(int) = g`，`g[P any](P)`（issue59338b）。
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let pt = tt.type_param_type(p);
    let g = sig_n(&mut tt, &[pt], &[], false);
    let i = int(&mut tt);
    let target = sig_n(&mut tt, &[i], &[], false);
    let res = infer_from_assignment(&mut tt, &[p], &[], g, target);
    assert_eq!(res, Ok(Some(vec![i])));
}

#[test]
fn t_ck_23_赋值语境_部分显式() {
    // `var _ func(int) string = h[int]`，`h[P, Q any](P) Q`（issue59338b）。
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let q = param(&mut tt, 1, a);
    let pt = tt.type_param_type(p);
    let qt = tt.type_param_type(q);
    let h = sig_n(&mut tt, &[pt], &[qt], false);
    let i = int(&mut tt);
    let s = string(&mut tt);
    let target = sig_n(&mut tt, &[i], &[s], false);
    let res = infer_from_assignment(&mut tt, &[p, q], &[Some(i)], h, target);
    assert_eq!(res, Ok(Some(vec![i, s])));
}

#[test]
fn t_ck_23_赋值语境_结果位推断() {
    // `var _ func(int) string = g2`，`g2[P, Q any](P) Q`（issue59958）：
    // 形参位给 P=int，结果位给 Q=string。
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let q = param(&mut tt, 1, a);
    let pt = tt.type_param_type(p);
    let qt = tt.type_param_type(q);
    let g2 = sig_n(&mut tt, &[pt], &[qt], false);
    let i = int(&mut tt);
    let s = string(&mut tt);
    let target = sig_n(&mut tt, &[i], &[s], false);
    let res = infer_from_assignment(&mut tt, &[p, q], &[], g2, target);
    assert_eq!(res, Ok(Some(vec![i, s])));
}

#[test]
fn t_ck_23_赋值语境_不同一失败() {
    // issue60688：`func g[P any](P, string)`、`func f func(int, String)`，
    // `String` 是定义型（本体 string）——函数**元素层**必须同一，不得剥本体。
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let pt = tt.type_param_type(p);
    let s = string(&mut tt);
    let g = sig_n(&mut tt, &[pt, s], &[], false);
    let my = plain(&mut tt, "String", s);
    let mt = tt.named_type(my);
    let i = int(&mut tt);
    let target = sig_n(&mut tt, &[i, mt], &[], false);
    let res = infer_from_assignment(&mut tt, &[p], &[], g, target);
    assert_eq!(res, Err(DiagCode::TypCannotInferTypeArgs));
}

#[test]
fn t_ck_23_赋值语境_目标非函数静默() {
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let pt = tt.type_param_type(p);
    let g = sig_n(&mut tt, &[pt], &[], false);
    let i = int(&mut tt);
    // 目标本体不是签名 ⇒ 无函数语境可推（静默）。
    let res = infer_from_assignment(&mut tt, &[p], &[], g, i);
    assert_eq!(res, Ok(None));
}

#[test]
fn t_ck_23_自递归调用_改名隔离() {
    // `func f[P any](x P) { f(x) }`：形参与实参里的 `P` 是**同一条目**
    // （自递归）。入口 ② 内部改名后，P′ 绑定到外层的 P；不改名则
    // unify(P, P) 平凡成真、终检失败。
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let pt = tt.type_param_type(p);
    let res = infer_for_call(&mut tt, &[p], &[], &[pt], &[pt], &[]);
    assert_eq!(res, Ok(Some(vec![pt])));
}

// ── T-CK-24 ③：赋值上下文函数类型（含目标经具名/接口方法而来）─────────

#[test]
fn t_ck_24_赋值上下文_具名函数类型目标() {
    // 目标本体藏在具名类型后面：`type F func(int) string`。
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let q = param(&mut tt, 1, a);
    let pt = tt.type_param_type(p);
    let qt = tt.type_param_type(q);
    let g2 = sig_n(&mut tt, &[pt], &[qt], false);
    let i = int(&mut tt);
    let s = string(&mut tt);
    let fsig = sig_n(&mut tt, &[i], &[s], false);
    let f = plain(&mut tt, "F", fsig);
    let target = tt.named_type(f);
    let res = infer_from_assignment(&mut tt, &[p, q], &[], g2, target);
    assert_eq!(res, Ok(Some(vec![i, s])));
}

#[test]
fn t_ck_24_赋值上下文_接口方法签名目标() {
    // 目标来自接口方法签名（`interface{ M(func(int) string) }` 的 `M`）。
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let q = param(&mut tt, 1, a);
    let pt = tt.type_param_type(p);
    let qt = tt.type_param_type(q);
    let g2 = sig_n(&mut tt, &[pt], &[qt], false);
    let i = int(&mut tt);
    let s = string(&mut tt);
    let msig = sig_n(&mut tt, &[i], &[s], false);
    let iface = iface_of(&mut tt, vec![("M", msig)]);
    let Type::Interface(iid) = tt.type_of(iface) else {
        panic!("不是接口");
    };
    let target = tt.interface_def(*iid).methods[0].1;
    let res = infer_from_assignment(&mut tt, &[p, q], &[], g2, target);
    assert_eq!(res, Ok(Some(vec![i, s])));
}

#[test]
fn t_ck_24_赋值上下文_嵌套函数形参() {
    // `func k[P, Q any](func(P) Q)` 对 `func(func(int) string)`：
    // 未定型在**内层**函数签名里，逐层统一。
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let q = param(&mut tt, 1, a);
    let pt = tt.type_param_type(p);
    let qt = tt.type_param_type(q);
    let inner_g = sig_n(&mut tt, &[pt], &[qt], false);
    let k = sig_n(&mut tt, &[inner_g], &[], false);
    let i = int(&mut tt);
    let s = string(&mut tt);
    let inner_t = sig_n(&mut tt, &[i], &[s], false);
    let target = sig_n(&mut tt, &[inner_t], &[], false);
    let res = infer_from_assignment(&mut tt, &[p, q], &[], k, target);
    assert_eq!(res, Ok(Some(vec![i, s])));
}

#[test]
fn t_ck_24_赋值上下文_无信息失败() {
    // `func f[P any]()` 对 `func()`：P 不出现在签名里 ⇒ 终检失败。
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let f = sig_n(&mut tt, &[], &[], false);
    let target = sig_n(&mut tt, &[], &[], false);
    let res = infer_from_assignment(&mut tt, &[p], &[], f, target);
    assert_eq!(res, Err(DiagCode::TypCannotInferTypeArgs));
}

// ── 改名助手（官方 `renameTParams`）─────────────────────────────────

#[test]
fn t_ck_23_改名助手_形参与其约束同步() {
    // `[P any, Q ~[]P]`：改名后 ① 类型里的 Q 换成 Q′；
    // ② Q′ 的约束里的 P 也要换成 P′（约束的同步替换）。
    let mut tt = TypeTable::new();
    let a = any(&mut tt);
    let p = param(&mut tt, 0, a);
    let pt = tt.type_param_type(p);
    let sl_p = tt.add_slice(pt);
    let qc = tilde(&mut tt, sl_p);
    let q = param(&mut tt, 1, qc);
    let qt = tt.type_param_type(q);
    let s = sig_n(&mut tt, &[qt], &[], false);
    let (tps2, s2) = rename_tparams(&mut tt, &[p, q], s);
    assert_eq!(tps2.len(), 2);
    assert_ne!(tps2[0], p);
    assert_ne!(tps2[1], q);
    let p2t = tt.type_param_type(tps2[0]);
    let q2t = tt.type_param_type(tps2[1]);
    // 序号保持、约束仍在位。
    assert_eq!(tt.type_param_def(tps2[0]).index, 0);
    assert_eq!(tt.type_param_def(tps2[1]).index, 1);
    // ① 签名参数里的 Q → Q′。
    assert_eq!(sig_first_param(&tt, s2), q2t);
    // ② Q′ 的约束：~[]P → ~[]P′。
    let q2c = tt.type_param_def(tps2[1]).constraint;
    let Type::Interface(i) = tt.type_of(q2c) else {
        panic!("约束不是接口");
    };
    match &tt.interface_def(*i).tset {
        IfaceTSet::Terms(ts) => {
            let (tilde_flag, term) = ts[0];
            assert!(tilde_flag, "tilde 标志保持");
            let Type::Slice(elem) = tt.type_of(term) else {
                panic!("项不是切片");
            };
            assert_eq!(*elem, p2t, "约束里的 P 应同步换成 P′");
        }
        other => panic!("约束 tset 应为 Terms：{other:?}"),
    }
}
