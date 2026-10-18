//! 类型推断（M4/Task 14；官方 `go/types/infer.go` 的算法面投影 + 形参改名）。
//!
//! # 三个入口 ↔ 官方三个调用形状
//!
//! | 本层 | 官方 | 语言版本 |
//! |---|---|---|
//! | [`infer_from_args`] | `check.infer`（`infer.go:35`；`call.go:626` 的调用） | 1.18 |
//! | [`infer_for_call`] | `arguments()` 的「泛型函数实参并入推断集」（`call.go:570-614`） | 1.21（`implicitly instantiated function as argument`） |
//! | [`infer_from_assignment`] | `funcInst` 的 reverse 形状（`call.go:71-130`；触发点 `expr.go:1011` / `:1073-1079`） | 1.21（赋值/接口方法语境）—— 语料快照 1.27 的同一算法面 |
//!
//! [`rename_tparams`] 是官方 `renameTParams`（`infer.go:471`）的投影：自递归
//! `f[P](x P) { f(x) }` 里「同一个 `P` 既是求解对象又出现在实参里」的隔离。
//! 官方把该步放在 **call 面**（`call.go:570` 对普通调用、`:597` 对每个泛型实参），
//! 所以 [`infer_from_args`] **不**自带改名；[`infer_for_call`] 与
//! [`infer_from_assignment`] 是「整段 call 面」的入口，自带。
//!
//! # 算法（`infer_from_args` 的三段 + 收尾）
//!
//! ① 实参与形参逐位统一（`u.unify(par, arg, assign)`）。untyped 实参不参与
//!   这里 —— 只有当形参是**直接类型参数**时被收集，留给 ③。
//! ② 约束信息（反复到没有进展为止）：约束的单精确项直接置位；有核心项时把
//!   已绑定的 `tx` 与核心项统一；已绑定且**非参数化**的 `tx` 还要对约束的
//!   方法集逐一 `u.unify(…, exact)`（这一步能反推出额外实参）。
//! ③ untyped 实参：同一形参取「最大种类」（`untyped int < rune < float <
//!   complex`），再按 `Default` 落到具体类型；种类互斥 ⇒ 失败。
//! 收尾：`kill_cycles`（推断图里绕回本批形参 ⇒ 置空）→ 迭代 `substitute`
//! 化简到不动点 → 终检（全绑定且不再含本批形参）。
//!
//! # 返回形态
//!
//! * `Ok(Some(targs))` —— 成功（长度 == `tparams.len()`）；
//! * `Ok(None)` —— **静默放弃**：实参含 `invalid`（官方 `infer.go:65-71`
//!   「错误已报，不长连锁诊断」）；
//! * `Err(TypCannotInferTypeArgs)` —— 真失败。官方 `CannotInferTypeArgs`；
//!   `infer.go:141-144` 的注释明确：**用户所给实参的毛病也统一落这条**
//!   （它也可能更该归 `InvalidTypeArg`，无法分辨 ⇒ 取更泛者）。
//!
//! # v1 边界（与 `tests/infer.rs` 的「v1 边界」一节对应）
//!
//! * **约束复验不在此层**：官方把约束校验放进 `check.later` 的 verify 面
//!   （`call.go:154`，码 `InvalidTypeArg`）。实测 go1.27.1：
//!   `f[P int8|int16](x P); f(1)` 在推断层得 `P=int`（③ 段默认化后不回头），
//!   由 verify 面拒绝。本层同口径（复验属 check 面）。
//! * 项表**不归一**（`typeset.rs` 的既有偏离，M4/Task 8）：单元素判定用项表
//!   原始长度；重复项的非法源（`int | int`）可能把单元素判成普通无核心 ⇒
//!   最终以稳定拒绝收场（官方 `norm()` 后是 1 项、会置位）。
//! * `substitute` 不凿 `Named` 实例（`subst.rs` 的既有边界，M4/Task 12）：
//!   形参里的平实例（`Box[P]`）不参与改名与化简。
//! * 官方收尾里「化简后签名不再含形参 ⇒ 清空其形参表」（`infer.go:441-452`）
//!   不可投影：本层签名的**函数形参表不在表里**（只有接收者的 `recv_tparams`，
//!   见 `SignatureDef` 文档），该步无可清空。
//! * 接口推断开关默认**开**（= go1.21+；官方 `newUnifier(…, allowVersion(go1_21))`
//!   `infer.go:117`）；<1.21 的收窄由 check 面直控 [`crate::unify::Unifier`]。

use rgoc_const::{BasicId, default_type_of};
use rgoc_lex::DiagCode;

use crate::identical::{unalias, underlying};
use crate::methodset::methodset;
use crate::subst::{SubstMap, substitute};
use crate::table::TypeTable;
use crate::typ::{InterfaceId, Type, TypeId, TypeParamId};
use crate::typeset::IfaceTSet;
use crate::unify::{Unifier, UnifyMode, common_under};

/// 一次调用里的一个**泛型函数实参**（官方 `call.go:580-612` 的收集项）。
///
/// 本层签名不携带**函数形参表**（`SignatureDef` 只存接收者的），所以调用方
/// 必须把实参函数声明的形参一起交进来；本入口按官方口径**逐实参**改名
/// （同一函数出现两次 ⇒ 两份互不干扰的形参，`f(g, g)` 靠这条）。
#[derive(Clone, Debug)]
pub struct GenericArgSite {
    /// 实参下标（`args` 里的位置）。
    pub index: usize,
    /// 该实参函数声明的**形参表**（未改名；本入口负责改名）。
    pub tparams: Vec<TypeParamId>,
    /// 该实参的**部分**显式实参（`g2[int]` 形态；前缀对齐，未给者缺省）。
    pub targs: Vec<Option<TypeId>>,
}

/// ① 1.18 实参模式 —— 官方 `check.infer`（`infer.go:35`，唯一主入口）。
///
/// # 约定（官方断言面的对应物）
///
/// * `tparams` 为空 ⇒ 直接 `Ok(Some(vec![]))`（调用方本应守 `n > 0`，
///   官方 `infer.go:55` 是断言；本层从宽）；
/// * `targs` 是**前缀对齐**的显式实参（长度 ≤ 形参数；`None` = 缺）；
/// * `params.len() == args.len()`（变参调整已在调用方完成，官方 `:58`）。
///
/// # Errors
///
/// 推断失败 ⇒ `TypCannotInferTypeArgs`；能力边界码（`unify` / `methodset`
/// 的稳定拒绝）原样透传。
pub fn infer_from_args(
    tt: &mut TypeTable,
    tparams: &[TypeParamId],
    targs: &[Option<TypeId>],
    params: &[TypeId],
    args: &[TypeId],
) -> Result<Option<Vec<TypeId>>, DiagCode> {
    let n = tparams.len();
    assert!(
        targs.len() <= n,
        "显式实参数（{}）不得超过形参数（{n}）",
        targs.len()
    );
    assert_eq!(params.len(), args.len(), "形参表与实参表必须等长");
    if n == 0 {
        return Ok(Some(Vec::new()));
    }
    // 全部显式给定 ⇒ 原样返回（官方 `:60-63`）。
    if targs.len() == n && targs.iter().all(Option::is_some) {
        return Ok(Some(targs.iter().filter_map(|t| *t).collect()));
    }
    // 实参里有无效类型 ⇒ 静默放弃（官方 `:65-71`）。
    if args.iter().any(|a| matches!(tt.type_of(*a), Type::Invalid)) {
        return Ok(None);
    }
    // 显式实参先替换进形参（官方 `:88-95`；`None` 项不替换）。
    let subst_params: Vec<TypeId>;
    let params: &[TypeId] = if targs.iter().any(Option::is_some) {
        let smap: SubstMap = tparams
            .iter()
            .zip(targs)
            .filter_map(|(tp, t)| t.map(|t| (*tp, t)))
            .collect();
        subst_params = params.iter().map(|p| substitute(tt, *p, &smap)).collect();
        &subst_params
    } else {
        params
    };
    // ③ 段的默认类型先在表上建好（unifier 借表之后不能再建条目）。
    let mut defaults: Vec<(BasicId, TypeId)> = Vec::new();
    for a in args {
        if let Type::Basic(k) = tt.type_of(*a)
            && is_untyped_kind(*k)
            && *k != BasicId::UntypedNil
            && !defaults.iter().any(|(b, _)| *b == default_type_of(*k))
        {
            let d = default_type_of(*k);
            defaults.push((d, tt.add_basic(d)));
        }
    }
    // 起手：登记全部形参、预置显式实参（官方 `:117` 的 `newUnifier`）。
    let mut u = Unifier::new(tt);
    u.record(tparams.iter().copied());
    for (tp, t) in tparams.iter().zip(targs) {
        if let Some(t) = t {
            u.preset(*tp, *t);
        }
    }

    // ── ① 实参与形参统一（官方 `:170-203`）──────────────────────────
    let mut untyped: Vec<usize> = Vec::new();
    for (idx, (par, arg)) in params.iter().zip(args).enumerate() {
        if !is_parameterized(tt, tparams, *par) && !is_parameterized(tt, tparams, *arg) {
            continue;
        }
        if !is_untyped(tt, *arg) {
            if !u.unify(UnifyMode::Assign, *par, *arg)? {
                return Err(DiagCode::TypCannotInferTypeArgs);
            }
        } else if matches!(tt.type_of(*par), Type::TypeParam(_)) && !is_untyped_nil(tt, *arg) {
            // untyped 实参只可能对上**直接类型参数**形参（官方断言无别名）。
            untyped.push(idx);
        }
    }

    // ── ② 约束信息（官方 `:205-316`）────────────────────────────────
    loop {
        let before = u.unknowns();
        for tp in tparams {
            let tx = u.at(*tp);
            let (core, single) = core_term(tt, *tp)?;
            if let Some((tilde, ctyp)) = core {
                if let Some(tx) = tx {
                    // 已知 tx：与核心项统一（官方 `:248-268`）。
                    if !u.unify(UnifyMode::Loose, tx, ctyp)? {
                        return Err(DiagCode::TypCannotInferTypeArgs);
                    }
                } else if single && !tilde {
                    // 单精确项 ⇒ 置位（官方 `:269-277`）。
                    u.set(*tp, ctyp);
                }
            }
            if let Some(tx) = tx
                && !is_parameterized(tt, tparams, tx)
            {
                // 已绑定的非参数化 tx 必须满足约束的方法集（官方 `:293-309`；
                // 这一步的 exact 统一能反推出额外实参）。
                let c = tt.type_param_def(*tp).constraint;
                let cu = underlying(tt, c);
                if let Type::Interface(cid) = tt.type_of(cu)
                    && !has_all_methods_unify(tt, &mut u, tx, *cid)?
                {
                    return Err(DiagCode::TypCannotInferTypeArgs);
                }
            }
        }
        if u.unknowns() == before {
            break;
        }
    }

    // ── ③ untyped 实参的默认类型（官方 `:323-361`）──────────────────
    let mut max_untyped: Vec<(TypeParamId, BasicId)> = Vec::new();
    for idx in untyped {
        let Type::TypeParam(tp) = tt.type_of(params[idx]) else {
            unreachable!("untyped 列表只收直接类型参数形参");
        };
        if u.at(*tp).is_some() {
            continue;
        }
        let Type::Basic(k) = tt.type_of(args[idx]) else {
            unreachable!("untyped 列表只收 untyped 实参");
        };
        match max_untyped.iter_mut().find(|(t, _)| t == tp) {
            Some((_, max)) => match max_untyped_kind(*max, *k) {
                Some(m) => *max = m,
                // 种类互斥 ⇒ 官方「mismatched types … (cannot infer …)」。
                None => return Err(DiagCode::TypCannotInferTypeArgs),
            },
            None => max_untyped.push((*tp, *k)),
        }
    }
    for (tp, k) in max_untyped {
        let d = default_type_of(k);
        let ty = defaults
            .iter()
            .find(|(b, _)| *b == d)
            .map(|(_, t)| *t)
            .expect("默认类型已在起手时建好");
        u.set(tp, ty);
    }

    // ── 化简（官方 `:363-472`）──────────────────────────────────────
    let mut inferred = u.inferred(tparams);
    drop(u); // 之后要改表（`substitute`）—— 借期到此为止。
    kill_cycles(tt, tparams, &mut inferred);
    // 迭代替换直至不动点；只盯非显式给定项（官方 `dirty` 列表，`:408-438`）。
    let mut dirty: Vec<usize> = inferred
        .iter()
        .enumerate()
        .filter(|(i, t)| t.is_some() && (*i >= targs.len() || targs[*i].is_none()))
        .map(|(i, _)| i)
        .collect();
    while !dirty.is_empty() {
        let smap: SubstMap = tparams
            .iter()
            .zip(&inferred)
            .filter_map(|(tp, t)| t.map(|t| (*tp, t)))
            .collect();
        let mut next = Vec::with_capacity(dirty.len());
        for i in dirty {
            let t0 = inferred[i].expect("dirty 只收已绑定项");
            let t1 = substitute(tt, t0, &smap);
            if t1 != t0 {
                inferred[i] = Some(t1);
                next.push(i);
            }
        }
        dirty = next;
    }
    // ── 终检（官方 `:474-492`）：全绑定，且不再含本批形参 ───────────
    let mut out = Vec::with_capacity(n);
    for t in inferred {
        let Some(t) = t else {
            return Err(DiagCode::TypCannotInferTypeArgs);
        };
        if is_parameterized(tt, tparams, t) {
            return Err(DiagCode::TypCannotInferTypeArgs);
        }
        out.push(t);
    }
    Ok(Some(out))
}

/// ② 1.21 实参位置 —— 官方 `arguments()` 的「泛型函数实参并入推断集」。
///
/// 做法照官方 `call.go:570-614`：被调者的形参先改名（`:570`）；每个泛型
/// 函数实参再**各自**改名（`:597`，修 `f(g, g)` 与递归），其签名替换进实参
/// 位置，形参与部分显式实参并入推断集；最后**一次**统一（`:626`）。
///
/// # 返回形态
///
/// 长度 = `callee_tparams.len()` + 各 `GenericArgSite.tparams.len()`，
/// 顺序：被调者在前，泛型实参按**实参下标升序**随后（官方 `:619-621` 的
/// 分段口径；调用方按同一顺序切回）。
///
/// # Errors
///
/// 同 [`infer_from_args`]。
pub fn infer_for_call(
    tt: &mut TypeTable,
    callee_tparams: &[TypeParamId],
    callee_targs: &[Option<TypeId>],
    callee_params: &[TypeId],
    args: &[TypeId],
    generic_args: &[GenericArgSite],
) -> Result<Option<Vec<TypeId>>, DiagCode> {
    assert!(
        callee_targs.len() <= callee_tparams.len(),
        "被调者的显式实参数不得超过形参数"
    );
    // 被调者改名 + 形参替换（官方 `call.go:570-571`）。
    let (callee_tps2, cmap) = rename_map(tt, callee_tparams);
    let params2: Vec<TypeId> = callee_params
        .iter()
        .map(|p| substitute(tt, *p, &cmap))
        .collect();
    let mut args2 = args.to_vec();
    let mut tparams_all = callee_tps2;
    let mut targs_all = callee_targs.to_vec();
    targs_all.resize(tparams_all.len(), None);
    // 泛型函数实参：逐位改名并并集（官方 `call.go:580-612`）。
    let mut sites: Vec<&GenericArgSite> = generic_args.iter().collect();
    sites.sort_by_key(|s| s.index);
    for site in sites {
        assert!(site.index < args.len(), "泛型实参下标越界");
        assert!(
            site.targs.len() <= site.tparams.len(),
            "泛型实参的显式实参数不得超过其形参数"
        );
        let (tps2, amap) = rename_map(tt, &site.tparams);
        args2[site.index] = substitute(tt, args[site.index], &amap);
        tparams_all.extend(tps2);
        targs_all.extend(site.targs.iter().copied());
        targs_all.resize(tparams_all.len(), None);
    }
    infer_from_args(tt, &tparams_all, &targs_all, &params2, &args2)
}

/// ②b / ③ 赋值与接口方法语境 —— 官方 `funcInst` 的 reverse 形状。
///
/// 官方把「泛型函数用在需要非泛型函数类型的位置」化成一次**合成的调用推断**
/// （`call.go:76-105`）：视作 `func g[形参们](func_type_of_x)` 的 `g(目标)`，
/// 于是复用同一套 `infer`。触发点：`expr.go:1011`（nonGeneric，普通赋值）与
/// `expr.go:1073-1079`（IndexExpr，部分显式实例化的函数值），门控
/// `enableReverseTypeInference`（`infer.go:24`「Available with go1.21」）。
/// 语料快照 1.27 的算法面与 1.21 相同（无独立 1.27 门控）。
///
/// `target` 允许是**具名函数类型**（经本体的签名参与统一）；`fn_sig` 须是
/// 签名条目。目标本体不是签名 ⇒ `Ok(None)`（没有函数语境可推，不是错误）。
///
/// # Errors
///
/// 同 [`infer_from_args`]。
pub fn infer_from_assignment(
    tt: &mut TypeTable,
    tparams: &[TypeParamId],
    targs: &[Option<TypeId>],
    fn_sig: TypeId,
    target: TypeId,
) -> Result<Option<Vec<TypeId>>, DiagCode> {
    // 目标本体必须是签名（官方 `T.sig` 的来源）；否则静默。
    let tu = underlying(tt, target);
    if !matches!(tt.type_of(tu), Type::Signature(_)) {
        return Ok(None);
    }
    // 函数签名（官方 `x.typ().(*Signature)`）。
    let fsu = underlying(tt, fn_sig);
    let Type::Signature(fsid) = tt.type_of(fsu) else {
        return Ok(None);
    };
    // 合成 g 的形参签名：同参数/结果/变参，不带接收者与形参携带。
    let (pid, rid, variadic) = {
        let d = tt.signature_def(*fsid);
        let (Type::Tuple(p), Type::Tuple(r)) = (tt.type_of(d.params), tt.type_of(d.results)) else {
            return Ok(None);
        };
        (*p, *r, d.variadic)
    };
    let gsid = tt.add_signature(None, pid, rid, variadic);
    let gsig = tt.signature_type(gsid);
    // 改名就在此处（官方 `funcInst` 在合成 g 之后、`infer` 之前改名）。
    let (tps2, gsig2) = rename_tparams(tt, tparams, gsig);
    infer_from_args(tt, &tps2, targs, &[gsig2], &[tu])
}

/// 形参改名 —— 官方 `renameTParams`（`infer.go:471`）。
///
/// 为每个形参建一条**新条目**（序号与约束照抄），再把 `typ` 里的旧条目换成
/// 新条目；新形参的约束里若引用了**兄弟形参**，也一并替换（官方 `:500-519`
/// 的 bound 替换）。自递归 `f[P](x P) { f(x) }` 靠它把「求解对象」与
/// 「实参里的外来形参」分开。
///
/// 返回 `(新形参表, 替换后的 typ)`；`tparams` 为空时原样返回。
pub fn rename_tparams(
    tt: &mut TypeTable,
    tparams: &[TypeParamId],
    typ: TypeId,
) -> (Vec<TypeParamId>, TypeId) {
    let (fresh, map) = rename_map(tt, tparams);
    if fresh.is_empty() {
        return (fresh, typ);
    }
    (fresh, substitute(tt, typ, &map))
}

/// 建「旧形参 → 新形参」映射（[`rename_tparams`] 与 [`infer_for_call`] 共用）。
fn rename_map(tt: &mut TypeTable, tparams: &[TypeParamId]) -> (Vec<TypeParamId>, SubstMap) {
    if tparams.is_empty() {
        return (Vec::new(), SubstMap::new());
    }
    let mut fresh = Vec::with_capacity(tparams.len());
    for tp in tparams {
        let (index, c) = {
            let d = tt.type_param_def(*tp);
            (d.index, d.constraint)
        };
        fresh.push(tt.add_type_param(index, c));
    }
    let map: SubstMap = tparams
        .iter()
        .zip(&fresh)
        .map(|(o, n)| (*o, tt.type_param_type(*n)))
        .collect();
    for n in &fresh {
        let c0 = tt.type_param_def(*n).constraint;
        let c1 = substitute(tt, c0, &map);
        tt.set_type_param_constraint(*n, c1);
    }
    (fresh, map)
}

// ── 约束核心项（官方 `coreTerm`，`infer.go:651-694`）────────────────

/// 约束的「核心项」。
///
/// 语义照官方：单元素项表 ⇒ `(该项, true)`；多元素但各本体的共同类型存在
/// ⇒ `(该共同本体, false)`（任一项带 tilde ⇒ 返回项也带）；否则 `(None, false)`。
fn core_term(tt: &TypeTable, tp: TypeParamId) -> Result<(Option<(bool, TypeId)>, bool), DiagCode> {
    let (c, self_ty) = {
        let d = tt.type_param_def(tp);
        (d.constraint, d.self_ty)
    };
    let cu = underlying(tt, c);
    let terms = match tt.type_of(cu) {
        Type::Interface(i) => match &tt.interface_def(*i).tset {
            IfaceTSet::Terms(ts) => ts.clone(),
            _ => return Ok((None, false)),
        },
        // 约束恒为接口（`rgoc-load::declare_tparams` 的口径）；非法源 ⇒ 无核心。
        _ => return Ok((None, false)),
    };
    if terms.len() == 1 {
        return Ok((Some(terms[0]), true));
    }
    if terms.is_empty() {
        return Ok((None, false));
    }
    let tilde_any = terms.iter().any(|(t, _)| *t);
    Ok(match common_under(tt, self_ty)? {
        Some(core) => (Some((tilde_any, core)), false),
        None => (None, false),
    })
}

// ── 约束方法集统一（官方 `hasAllMethods(…, u.unify(exact))`）──────────

/// `tx` 的方法集能否按 **exact 统一**对上约束的方法集（可反推额外实参）。
fn has_all_methods_unify(
    tt: &TypeTable,
    u: &mut Unifier<'_>,
    v: TypeId,
    tid: InterfaceId,
) -> Result<bool, DiagCode> {
    let required = tt.interface_def(tid).methods.clone();
    if required.is_empty() {
        return Ok(true);
    }
    let vu = underlying(tt, v);
    // 无效 V：官方「不知道 ⇒ 当满足」（`lookup.go:541` 的 `isValid` 守卫）。
    if matches!(tt.type_of(vu), Type::Invalid) {
        return Ok(true);
    }
    if let Type::Interface(vi) = tt.type_of(vu) {
        let have = tt.interface_def(*vi).methods.clone();
        for (name, sig) in &required {
            let Some((_, vsig)) = have.iter().find(|(n, _)| n == name) else {
                return Ok(false);
            };
            if !u.unify(UnifyMode::Exact, *vsig, *sig)? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    let ms = methodset(tt, v)?;
    for (name, sig) in &required {
        let Some(e) = ms.iter().find(|e| e.name == *name) else {
            return Ok(false);
        };
        if !u.unify(UnifyMode::Exact, e.sig, *sig)? {
            return Ok(false);
        }
    }
    Ok(true)
}

// ── 形参出现检测（官方 `isParameterized` + `tpWalker`）───────────────

/// `t` 里是否出现 `tparams` 中任一条目（官方 `tpWalker.isParameterized`）。
///
/// 边界照官方：`Named` 只走**实例实参**（不钻本体）；签名的**函数形参表**与
/// 接收者形参不看（那是声明，不是使用）；接口看方法集与项面。
fn is_parameterized(tt: &TypeTable, tparams: &[TypeParamId], t: TypeId) -> bool {
    let mut seen: Vec<TypeId> = Vec::new();
    walk_parameterized(tt, tparams, t, &mut seen)
}

fn walk_parameterized(
    tt: &TypeTable,
    tparams: &[TypeParamId],
    t: TypeId,
    seen: &mut Vec<TypeId>,
) -> bool {
    let t = unalias(tt, t);
    if seen.contains(&t) {
        // 环上重访：官方 `seen` 预置的 false（`infer.go:578-581`）。
        return false;
    }
    seen.push(t);
    let res = match *tt.type_of(t) {
        Type::TypeParam(tp) => tparams.contains(&tp),
        Type::Named(nd) => tt
            .named_def(nd)
            .targs
            .iter()
            .any(|a| walk_parameterized(tt, tparams, *a, seen)),
        Type::Pointer(e) | Type::Slice(e) => walk_parameterized(tt, tparams, e, seen),
        Type::Array { elem, .. } => walk_parameterized(tt, tparams, elem, seen),
        Type::Map { key, elem } => {
            walk_parameterized(tt, tparams, key, seen)
                || walk_parameterized(tt, tparams, elem, seen)
        }
        Type::Chan { elem, .. } => walk_parameterized(tt, tparams, elem, seen),
        Type::Tuple(tu) => tt
            .tuple_def(tu)
            .vars
            .iter()
            .any(|(_, v)| walk_parameterized(tt, tparams, *v, seen)),
        Type::Struct(sd) => tt
            .struct_def(sd)
            .fields
            .iter()
            .any(|f| walk_parameterized(tt, tparams, f.typ, seen)),
        Type::Signature(sd) => {
            let d = tt.signature_def(sd);
            walk_parameterized(tt, tparams, d.params, seen)
                || walk_parameterized(tt, tparams, d.results, seen)
        }
        Type::Interface(id) => {
            let d = tt.interface_def(id);
            d.methods
                .iter()
                .any(|(_, s)| walk_parameterized(tt, tparams, *s, seen))
                || d.terms
                    .is_some_and(|u| walk_parameterized(tt, tparams, u, seen))
                || match &d.tset {
                    IfaceTSet::Terms(ts) => ts
                        .iter()
                        .any(|(_, ty)| walk_parameterized(tt, tparams, *ty, seen)),
                    IfaceTSet::All | IfaceTSet::Comparable | IfaceTSet::Empty => false,
                }
        }
        Type::Union(u) => tt
            .union_def(u)
            .terms
            .iter()
            .any(|(_, ty)| walk_parameterized(tt, tparams, *ty, seen)),
        Type::Alias(_) => unreachable!("unalias 之后不该还有 alias"),
        Type::Basic(_) | Type::Invalid => false,
    };
    seen.pop();
    res
}

// ── 环剔除（官方 `killCycles` + `cycleFinder`，`infer.go:695-797`）────

/// 推断结果里「类型图绕回本批形参」的项置空 ⇒ 终检报「cannot infer」。
fn kill_cycles(tt: &TypeTable, tparams: &[TypeParamId], inferred: &mut [Option<TypeId>]) {
    let mut seen: Vec<TypeId> = Vec::new();
    for tp in tparams {
        cycle_walk(tt, tparams, inferred, tt.type_param_type(*tp), &mut seen);
    }
}

fn cycle_walk(
    tt: &TypeTable,
    tparams: &[TypeParamId],
    inferred: &mut [Option<TypeId>],
    t: TypeId,
    seen: &mut Vec<TypeId>,
) {
    let t = unalias(tt, t);
    if seen.contains(&t) {
        if let Type::TypeParam(tp) = tt.type_of(t)
            && let Some(i) = tparams.iter().position(|p| *p == *tp)
        {
            inferred[i] = None;
        }
        return;
    }
    seen.push(t);
    match *tt.type_of(t) {
        Type::TypeParam(tp) => {
            if let Some(i) = tparams.iter().position(|p| *p == tp)
                && let Some(bound) = inferred[i]
            {
                cycle_walk(tt, tparams, inferred, bound, seen);
            }
        }
        Type::Named(nd) => {
            for a in &tt.named_def(nd).targs {
                cycle_walk(tt, tparams, inferred, *a, seen);
            }
        }
        Type::Pointer(e) | Type::Slice(e) => cycle_walk(tt, tparams, inferred, e, seen),
        Type::Array { elem, .. } => cycle_walk(tt, tparams, inferred, elem, seen),
        Type::Map { key, elem } => {
            cycle_walk(tt, tparams, inferred, key, seen);
            cycle_walk(tt, tparams, inferred, elem, seen);
        }
        Type::Chan { elem, .. } => cycle_walk(tt, tparams, inferred, elem, seen),
        Type::Tuple(tu) => {
            for (_, v) in &tt.tuple_def(tu).vars {
                cycle_walk(tt, tparams, inferred, *v, seen);
            }
        }
        Type::Struct(sd) => {
            for f in &tt.struct_def(sd).fields {
                cycle_walk(tt, tparams, inferred, f.typ, seen);
            }
        }
        Type::Signature(sd) => {
            let d = tt.signature_def(sd);
            cycle_walk(tt, tparams, inferred, d.params, seen);
            cycle_walk(tt, tparams, inferred, d.results, seen);
        }
        Type::Interface(id) => {
            let d = tt.interface_def(id);
            for (_, s) in &d.methods {
                cycle_walk(tt, tparams, inferred, *s, seen);
            }
            if let Some(u) = d.terms {
                cycle_walk(tt, tparams, inferred, u, seen);
            }
            if let IfaceTSet::Terms(ts) = &d.tset {
                for (_, ty) in ts {
                    cycle_walk(tt, tparams, inferred, *ty, seen);
                }
            }
        }
        Type::Union(u) => {
            for (_, ty) in &tt.union_def(u).terms {
                cycle_walk(tt, tparams, inferred, *ty, seen);
            }
        }
        Type::Alias(_) => unreachable!("unalias 之后不该还有 alias"),
        Type::Basic(_) | Type::Invalid => {}
    }
    seen.pop();
}

// ── untyped 小工具（官方 `isUntyped` / `Default` / `maxType`）────────

fn is_untyped_kind(k: BasicId) -> bool {
    matches!(
        k,
        BasicId::UntypedBool
            | BasicId::UntypedInt
            | BasicId::UntypedRune
            | BasicId::UntypedFloat
            | BasicId::UntypedComplex
            | BasicId::UntypedString
            | BasicId::UntypedNil
    )
}

fn is_untyped(tt: &TypeTable, t: TypeId) -> bool {
    matches!(tt.type_of(t), Type::Basic(k) if is_untyped_kind(*k))
}

fn is_untyped_nil(tt: &TypeTable, t: TypeId) -> bool {
    matches!(tt.type_of(t), Type::Basic(BasicId::UntypedNil))
}

fn is_untyped_numeric(k: BasicId) -> bool {
    matches!(
        k,
        BasicId::UntypedInt
            | BasicId::UntypedRune
            | BasicId::UntypedFloat
            | BasicId::UntypedComplex
    )
}

/// 官方 `maxType`（`predicates.go:549`）：同种 ⇒ 自身；两种 untyped 数值
/// ⇒ 种类靠后者（枚举声明序 = `int < rune < float < complex`）；否则无解。
fn max_untyped_kind(a: BasicId, b: BasicId) -> Option<BasicId> {
    if a == b {
        return Some(a);
    }
    if is_untyped_numeric(a) && is_untyped_numeric(b) {
        return Some(if a > b { a } else { b });
    }
    None
}
