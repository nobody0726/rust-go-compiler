//! 显式实例化（M4/Task 12；官方 `go/types/instantiate.go` 的 v1 子集）。
//!
//! # 判据来源与打包口径
//!
//! 官方把这件事拆在调用面三层：`validateTArgLen`（数量）、`verify`（约束）、
//! `instance`（memo + 建实例 + 展开）。本函数**打包**这三层 —— 调用面
//! （检查器）只需要「给我实例，或者告诉我为什么不行」。
//!
//! 逐条对齐：
//!
//! 1. **数量**：`tparams.len() != args.len()` ⇒ [`DiagCode::TypWrongTypeArgCount`]
//!    （官方 `WrongTypeArgCount`；消息 `too many/not enough type arguments for
//!    type %s: have %d, want %d` 由渲染面拼）。
//! 2. **memo**：`(origin, args)` 命中（实参按
//!    [`identical`](crate::identical::identical) 比对 —— 官方 `Context.lookup`
//!    的「哈希 + 同一性」口径）⇒ 复用既有实例，**不**再校验。
//! 3. **约束**：逐形参 `bound' = substitute(bound, map)`
//!    （官方 `makeSubstMap(tparams, targs)`），再
//!    `implements(arg, bound', constraint=true)`；判假 ⇒
//!    [`DiagCode::TypInvalidTypeArg`]（官方 `InvalidTypeArg`，消息是
//!    implements 的 cause：`%s does not satisfy %s`）。底层判不出（`Err`）时
//!    **原样透传**（不判 ≠ 答错；收窄由后续任务做）。
//! 4. **本体**：替换 `origin.underlying`（官方 `expandNamed` 的替换面；官方懒
//!    展开、本层急切，结果一致）。替换走 [`crate::subst::substitute_inst`]：
//!    本体里嵌的**实例**若实参被改动 ⇒ 重新实例化（`M4/Task 16`），链在 memo
//!    上闭合（`type A[T any] struct{ next *A[T] }`）⇒ 自引用指向实例自身。
//! 5. **方法**：逐方法按**接收者类型参数**位置对齐实参替换
//!    （官方 `named.go::expandMethod` 的 `makeSubstMap(rtpars, rtargs)`；
//!    `rtpars` 从签名的新栏 [`crate::table::SignatureDef::recv_tparams`] 读）。
//!    `rtpars.len() != args.len()`（畸形源）⇒ 不替换（官方同：不健全的签名不管）。
//! 6. **无界链**（`M4/Task 16`）：嵌套实例化按 [`MAX_INSTANTIATION_DEPTH`]
//!    守门；越界 ⇒ [`DiagCode::TypInvalidInstanceCycle`]（官方
//!    `InvalidInstanceCycle` 的**下界** —— 官方用 `mono.go` 的实例化图在更早
//!    的位置报，本层没有图，只有深度这个可判定面；`U-M4-09`）。展开失败时
//!    **回滚 memo**（[`TypeTable::remove_instance`]）—— 半成品实例不可见，
//!    同一 `(origin, args)` 再问仍是同一条拒绝（`T-CK-25`）。
//!
//! # v1 边界
//!
//! * 非泛型 `origin`：空实参 ⇒ **恒等返回**（官方对 `*Alias` 的 `return orig`
//!   同精神；源语言里 `X[]` 不可写，这个形态只为内部调用面兜底）；非空实参 ⇒
//!   数量错（官方对非泛型 `X[int]` 也报 `have 1, want 0`）。
//! * 实例**不再**作为 `instantiate` 的原点（实例的形参表为空 ⇒ 空实参恒等、
//!   非空实参数量错）；官方 `Instantiate` 对非泛型 `orig` 同样拒绝。
//! * 装载器仍把源码 `X[A]` 退化为 `X`（`T-TY-29`；D-M4-11 冻结），所以本层
//!   的嵌套实例面由**调用方直接给实例条目**驱动（检查器/测试），不从源码来。

use rgoc_lex::DiagCode;

use crate::implements::implements;
use crate::subst::{SubstMap, substitute_inst};
use crate::table::TypeTable;
use crate::typ::{NamedId, Type, TypeId};

/// 实例化链的深度上限（`T-CK-25`；`U-M4-09` 的拒绝形态）。
///
/// 为什么是深度而不是「重复的 `(origin, args)`」：官方 `mono.go` 检测的是
/// **实例化图**里的环，而本层（类型表）没有图；无界链的每个节点实参都不同
/// （`A[int]` → `A[A[int]]` → …），按「同一实参再次进入」判不出来。深度是
/// 可判定且与官方裁决同向的下界：闭合的链在 memo 处**立即**返回（深度不增长），
/// 只有每轮都造新实参的链才会撞线。50 与 `unify` 的深度上限同量级。
pub const MAX_INSTANTIATION_DEPTH: usize = 50;

/// 把泛型定义 `origin` 按实参 `args` 实例化。
///
/// 成功返回实例的**类型条目**（`Type::Named` 指向一个实例定义）；
/// 同一 `(origin, 按同一性等价的实参组)` 有且只有一个实例条目（memo）。
///
/// # Errors
///
/// * [`DiagCode::TypWrongTypeArgCount`] —— 实参个数 ≠ 形参个数；
/// * [`DiagCode::TypInvalidTypeArg`] —— 实参不满足对应约束；
/// * [`DiagCode::TypInvalidInstanceCycle`] —— 嵌套实例化链无界增长
///   （见 [`MAX_INSTANTIATION_DEPTH`]）；
/// * [`DiagCode::TypUnsupportedPendingInstanceArg`] —— 递归链上以**展开进行中**
///   的实例作受约束实参（约束非空接口），本层的急切展开看不到本体
///   （`U-M4-09` 的显式拒绝形态）；
/// * 其余码 —— [`implements`] 的边界判不出（原样透传，不吞不换）。
pub fn instantiate(
    tt: &mut TypeTable,
    origin: NamedId,
    args: &[TypeId],
) -> Result<TypeId, DiagCode> {
    instantiate_at(tt, origin, args, 0)
}

/// [`instantiate`] 的深度携带版（`crate::subst::substitute_inst` 的重入入口）。
pub(crate) fn instantiate_at(
    tt: &mut TypeTable,
    origin: NamedId,
    args: &[TypeId],
    depth: usize,
) -> Result<TypeId, DiagCode> {
    let tparams = tt.named_def(origin).type_params.clone();

    // ① 非泛型：空实参恒等（见模块文档「v1 边界」）；非空 ⇒ 数量错。
    if tparams.is_empty() {
        return if args.is_empty() {
            Ok(tt.named_type(origin))
        } else {
            Err(DiagCode::TypWrongTypeArgCount)
        };
    }
    // ② 数量。
    if tparams.len() != args.len() {
        return Err(DiagCode::TypWrongTypeArgCount);
    }
    // ③ memo：命中即复用（不再校验 —— 建时已校验过）。memo 在深度守门**之前**：
    //    已闭合的链（自引用同参）永远不触发上限。
    if let Some(inst) = tt.lookup_instance(origin, args) {
        return Ok(tt.named_type(inst));
    }
    // ③′ 无界链守门（Task 16）。
    if depth > MAX_INSTANTIATION_DEPTH {
        return Err(DiagCode::TypInvalidInstanceCycle);
    }
    // ④ 先建实例并登记 memo —— **在约束校验之前**（官方 `context.instance` 的
    //    顺序：`newNamed(orig, targs, nil)` + 登记 `ctxt.instances`，然后才
    //    `expandNamed` 与 `verify`）。
    //
    //    ⚠️ 顺序反过来（先校验后登记）会让**自引用约束**无界重入：
    //    `type T[U interface{ M() T[U] }] int` 校验 `X` 是否实现 `M() T[X]` 时
    //    要问 `T[X]` 是谁 —— memo 里还没有 ⇒ 再走一遍 instantiate ⇒ 再校验 ⇒ …
    //    直到触到深度上限，于是 `T[X]` 解析失败、签名结果是 `invalid`
    //    （`issue46461` 实测：`帧布局失败：v1 的类型 t32`，t32 = `Invalid`）。
    //    登记之后这一步在 memo 处闭合，拿到的是同一条（本体尚未填的）实例。
    let inst = tt.add_named_instance(origin, args.to_vec());
    tt.record_instance(origin, args.to_vec(), inst);

    // ⑤ 约束：bound 先按**全量**映射替换（官方 verify 的 smap）。
    let map: SubstMap = tparams.iter().copied().zip(args.iter().copied()).collect();
    for (i, tp) in tparams.iter().enumerate() {
        let bound = tt.type_param_def(*tp).constraint;
        let bound = match substitute_inst(tt, bound, &map, depth) {
            Ok(b) => b,
            Err(code) => {
                tt.remove_instance(origin, inst);
                return Err(code);
            }
        };
        // ⑤′ 递归链上的实参可能是**展开进行中**的实例（本体未填）：除空接口外
        //     的约束必须在看本体后才能判，而本层（急切展开）此刻看不到 ——
        //     按 `D-M4-5` 显式拒绝，不 panic、不静默答真（`U-M4-09`）。
        if !is_trivially_any(tt, bound) && contains_pending_instance(tt, args[i]) {
            tt.remove_instance(origin, inst);
            return Err(DiagCode::TypUnsupportedPendingInstanceArg);
        }
        // ⑤″ **自引用约束**（`type T[U interface{ M() T[U] }] int`）：约束的方法
        //     签名里提到了 `origin` 自己的实例 ⇒ 判定要拿「实参的方法集」来比，
        //     而装载期的方法集**此刻还没挂满** —— 方法挂载发生在签名解析**之后**
        //     （`resolve_func_sig` 先 `resolve_signature`、再 `add_named_method`），
        //     于是 `X.M()` 的签名正在解析、方法集里还没有 `M`。
        //
        //     官方能过是因为它**先声明方法对象、后填签名**，比签名时两侧是同一个
        //     壳；本层的 `SignatureDef` 是一次成型、没有可变壳。这里按
        //     「未知 ⇒ 满足」跳过（与 `implements` ①「无效 ⇒ 真」同口径）——
        //     真的不满足的约束会在**检查面**（方法集已完整）照常报出来。
        if !is_trivially_any(tt, bound) && mentions_origin(tt, bound, origin, &mut Vec::new()) {
            continue;
        }
        match implements(tt, args[i], bound, true) {
            Ok(true) => {}
            Ok(false) => {
                tt.remove_instance(origin, inst);
                return Err(DiagCode::TypInvalidTypeArg);
            }
            Err(code) => {
                tt.remove_instance(origin, inst);
                return Err(code);
            }
        }
    }
    let under = tt.named_underlying(origin);
    let methods = tt.named_def(origin).methods.clone();
    // ⑥⑦ 展开；失败回滚 memo（半成品实例不可见）。
    if let Err(code) = expand_instance(tt, inst, under, &map, args, depth, methods) {
        tt.remove_instance(origin, inst);
        return Err(code);
    }
    Ok(tt.named_type(inst))
}

/// 把**原型上后来才挂上的方法**补进已存在的实例（`M12/Task 18`）。
///
/// # 什么时候需要
///
/// 声明是**按序**求值的：`type Cache[T] struct{ l *List[entry[T]] }` 在第 11 行
/// 就把 `List[entry[T]]` 实例化了，而 `func (l *List[T]) Len()` 在第 16 行才挂到
/// 原型上。`expand_instance` 那一刻照 `origin.methods` 抄了一份 —— 抄到的是**空表**，
/// 于是实例 `List·entry·error` 的方法集永远缺 `Len`。
///
/// 症状出现在**方法选择器**上：`c.l.Len()` 在 lowering 里找不到方法
/// （`issue55101.go` 实测）。以前看不到是因为泛型方法原型不发码，那两个体根本
/// 没被下沉过。
///
/// 本函数在装载收尾跑一次：实例的方法集 = 原型当前的方法集（按实参替换），
/// 缺的补上，已有的不动。幂等。
pub fn refresh_instance_methods(tt: &mut TypeTable) {
    let list: Vec<(NamedId, NamedId, Vec<TypeId>)> = tt
        .named_instances()
        .map(|(inst, origin, targs)| (inst, origin, targs.to_vec()))
        .collect();
    for (inst, origin, args) in list {
        let methods = tt.named_def(origin).methods.clone();
        for (name, sig) in methods {
            if tt.named_def(inst).methods.iter().any(|(m, _)| *m == name) {
                continue;
            }
            let rtps = match tt.type_of(sig) {
                Type::Signature(sid) => tt.signature_def(*sid).recv_tparams.clone(),
                _ => Vec::new(),
            };
            let msig = if !rtps.is_empty() && rtps.len() == args.len() {
                let mmap: SubstMap = rtps.iter().copied().zip(args.iter().copied()).collect();
                match substitute_inst(tt, sig, &mmap, 1) {
                    Ok(s) => s,
                    // 替换失败（实参没布局 / 约束判不出）⇒ 不补。真错由别处报，
                    // 这里补一个半成品只会把症状推到更远的地方。
                    Err(_) => continue,
                }
            } else {
                sig
            };
            tt.add_named_method(inst, name, msig);
        }
    }
}

/// 展开实例的本体与方法（[`instantiate_at`] 的第 ⑥⑦ 步；失败由调用方回滚）。
fn expand_instance(
    tt: &mut TypeTable,
    inst: NamedId,
    under: TypeId,
    map: &SubstMap,
    args: &[TypeId],
    depth: usize,
    methods: Vec<(String, TypeId)>,
) -> Result<(), DiagCode> {
    let under = substitute_inst(tt, under, map, depth)?;
    tt.set_named_underlying(inst, under);
    for (name, sig) in methods {
        let rtps = match tt.type_of(sig) {
            Type::Signature(sid) => tt.signature_def(*sid).recv_tparams.clone(),
            _ => Vec::new(),
        };
        let msig = if !rtps.is_empty() && rtps.len() == args.len() {
            let mmap: SubstMap = rtps.iter().copied().zip(args.iter().copied()).collect();
            substitute_inst(tt, sig, &mmap, depth)?
        } else {
            sig
        };
        tt.add_named_method(inst, name, msig);
    }
    Ok(())
}

/// `t` 的结构里是否提到 `origin` 的**实例**（自引用约束的判据）。
///
/// 走签名 / 元组 / 具名实例实参 / 接口方法；后者的签名是自引用最常见的落点
/// （`interface{ M() T[U] }`）。环形结构用 `seen` 兜住。
fn mentions_origin(tt: &TypeTable, t: TypeId, origin: NamedId, seen: &mut Vec<TypeId>) -> bool {
    let t = crate::identical::unalias(tt, t);
    if seen.contains(&t) {
        return false;
    }
    seen.push(t);
    match tt.type_of(t) {
        Type::Named(n) => {
            let d = tt.named_def(*n);
            if d.origin == Some(origin) {
                return true;
            }
            let args = d.targs.clone();
            args.iter().any(|a| mentions_origin(tt, *a, origin, seen))
        }
        Type::Pointer(e) | Type::Slice(e) | Type::Array { elem: e, .. } => {
            mentions_origin(tt, *e, origin, seen)
        }
        Type::Map { key, elem } => {
            mentions_origin(tt, *key, origin, seen) || mentions_origin(tt, *elem, origin, seen)
        }
        Type::Chan { elem, .. } => mentions_origin(tt, *elem, origin, seen),
        Type::Tuple(tu) => {
            let vars = tt.tuple_def(*tu).vars.clone();
            vars.iter()
                .any(|(_, v)| mentions_origin(tt, *v, origin, seen))
        }
        Type::Struct(sd) => {
            let fields = tt.struct_def(*sd).fields.clone();
            fields
                .iter()
                .any(|f| mentions_origin(tt, f.typ, origin, seen))
        }
        Type::Signature(sd) => {
            let d = tt.signature_def(*sd).clone();
            mentions_origin(tt, d.params, origin, seen)
                || mentions_origin(tt, d.results, origin, seen)
        }
        Type::Interface(id) => {
            let d = tt.interface_def(*id).clone();
            d.methods
                .iter()
                .any(|(_, sig)| mentions_origin(tt, *sig, origin, seen))
        }
        Type::Basic(_) | Type::TypeParam(_) | Type::Union(_) | Type::Alias(_) | Type::Invalid => {
            false
        }
    }
}

/// `bound` 是**空接口**（`any` / `interface{}`）—— 一切类型都满足，判定不需要
/// 实参的本体。
fn is_trivially_any(tt: &TypeTable, bound: TypeId) -> bool {
    let u = crate::identical::underlying(tt, bound);
    match tt.type_of(u) {
        Type::Interface(iid) => {
            let d = tt.interface_def(*iid);
            d.methods.is_empty() && d.tset.is_all()
        }
        _ => false,
    }
}

/// `t` 的结构里是否**含有**展开进行中的实例（本体未填）。只有递归实例化链里
/// 可能见到 —— `instantiate_at` 先登记 memo 再展开，展开期间该实例的本体为空。
///
/// **不走访已完成实例的本体**：完成 ⇒ 它展开时嵌的实例都已落定；走访还会顺着
/// 声明环绕圈。匿名构造（指针/切片/…）要递归 —— 约束判定（方法集 / 项集）
/// 会通过 `underlying` 走到它们里面去，漏掉就是 panic 而不是拒绝。
fn contains_pending_instance(tt: &TypeTable, t: TypeId) -> bool {
    match *tt.type_of(t) {
        Type::Basic(_) | Type::Invalid | Type::TypeParam(_) => false,
        Type::Named(id) => tt.named_def(id).underlying.is_none(),
        Type::Alias(id) => tt
            .alias_def(id)
            .rhs
            .is_some_and(|r| contains_pending_instance(tt, r)),
        Type::Pointer(e) | Type::Slice(e) | Type::Chan { elem: e, .. } => {
            contains_pending_instance(tt, e)
        }
        Type::Array { elem, .. } => contains_pending_instance(tt, elem),
        Type::Map { key, elem } => {
            contains_pending_instance(tt, key) || contains_pending_instance(tt, elem)
        }
        Type::Struct(sid) => tt
            .struct_def(sid)
            .fields
            .iter()
            .any(|f| contains_pending_instance(tt, f.typ)),
        Type::Tuple(tid) => tt
            .tuple_def(tid)
            .vars
            .iter()
            .any(|(_, v)| contains_pending_instance(tt, *v)),
        Type::Signature(sid) => {
            let d = tt.signature_def(sid);
            contains_pending_instance(tt, d.params) || contains_pending_instance(tt, d.results)
        }
        Type::Interface(iid) => {
            let d = tt.interface_def(iid);
            d.methods
                .iter()
                .any(|(_, sig)| contains_pending_instance(tt, *sig))
                || d.terms.is_some_and(|x| contains_pending_instance(tt, x))
        }
        Type::Union(uid) => tt
            .union_def(uid)
            .terms
            .iter()
            .any(|(_, x)| contains_pending_instance(tt, *x)),
    }
}
