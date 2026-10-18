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
//! 4. **本体**：`substitute(origin.underlying, map)`（官方 `expandNamed` 的替换面；
//!    官方懒展开、本层急切，结果一致）。
//! 5. **方法**：逐方法按**接收者类型参数**位置对齐实参替换
//!    （官方 `named.go::expandMethod` 的 `makeSubstMap(rtpars, rtargs)`；
//!    `rtpars` 从签名的新栏 [`crate::table::SignatureDef::recv_tparams`] 读）。
//!    `rtpars.len() != args.len()`（畸形源）⇒ 不替换（官方同：不健全的签名不管）。
//!
//! # v1 边界
//!
//! * 非泛型 `origin`：空实参 ⇒ **恒等返回**（官方对 `*Alias` 的 `return orig`
//!   同精神；源语言里 `X[]` 不可写，这个形态只为内部调用面兜底）；非空实参 ⇒
//!   数量错（官方对非泛型 `X[int]` 也报 `have 1, want 0`）。
//! * 实例**不再**作为 `instantiate` 的原点（实例的形参表为空）——
//!   「实例的再次实例化」（官方 `X[A][B]` / 平实例替换）留后续任务。
//! * 环检测（`type A[T any] B[A[T]]` 一族）留 Task 16（计划内）。

use rgoc_lex::DiagCode;

use crate::implements::implements;
use crate::subst::{SubstMap, substitute};
use crate::table::TypeTable;
use crate::typ::{NamedId, Type, TypeId};

/// 把泛型定义 `origin` 按实参 `args` 实例化。
///
/// 成功返回实例的**类型条目**（`Type::Named` 指向一个实例定义）；
/// 同一 `(origin, 按同一性等价的实参组)` 有且只有一个实例条目（memo）。
///
/// # Errors
///
/// * [`DiagCode::TypWrongTypeArgCount`] —— 实参个数 ≠ 形参个数；
/// * [`DiagCode::TypInvalidTypeArg`] —— 实参不满足对应约束；
/// * 其余码 —— [`implements`] 的边界判不出（原样透传，不吞不换）。
pub fn instantiate(
    tt: &mut TypeTable,
    origin: NamedId,
    args: &[TypeId],
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
    // ③ memo：命中即复用（不再校验 —— 建时已校验过）。
    if let Some(inst) = tt.lookup_instance(origin, args) {
        return Ok(tt.named_type(inst));
    }
    // ④ 约束：bound 先按**全量**映射替换（官方 verify 的 smap）。
    let map: SubstMap = tparams.iter().copied().zip(args.iter().copied()).collect();
    for (i, tp) in tparams.iter().enumerate() {
        let bound = tt.type_param_def(*tp).constraint;
        let bound = substitute(tt, bound, &map);
        match implements(tt, args[i], bound, true) {
            Ok(true) => {}
            Ok(false) => return Err(DiagCode::TypInvalidTypeArg),
            Err(code) => return Err(code),
        }
    }
    // ⑤ 建实例并登记 memo。
    let inst = tt.add_named_instance(origin, args.to_vec());
    tt.record_instance(origin, args.to_vec(), inst);
    // ⑥ 本体替换。
    let under = tt.named_underlying(origin);
    let under = substitute(tt, under, &map);
    tt.set_named_underlying(inst, under);
    // ⑦ 方法替换：接收者类型参数按位置对齐实参（官方 expandMethod）。
    let methods = tt.named_def(origin).methods.clone();
    for (name, sig) in methods {
        let rtps = match tt.type_of(sig) {
            Type::Signature(sid) => tt.signature_def(*sid).recv_tparams.clone(),
            _ => Vec::new(),
        };
        let msig = if !rtps.is_empty() && rtps.len() == args.len() {
            let mmap: SubstMap = rtps.iter().copied().zip(args.iter().copied()).collect();
            substitute(tt, sig, &mmap)
        } else {
            sig
        };
        tt.add_named_method(inst, name, msig);
    }
    Ok(tt.named_type(inst))
}
