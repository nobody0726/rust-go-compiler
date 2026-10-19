//! 选择结果（M4/Task 24；官方 `go/types/selection.go` 的窄面）。
//!
//! 只落**判定所需**：类别（[`SelKind`]）与签名构造 ——
//! 方法值**去掉**接收者、方法表达式**接收者作首参**（官方
//! `call.go` 的两条 `x.typ_ = ...`）。索引链（`Selection.index`）在
//! [`crate::lookup`] 的结果里，本模块不复制。
//!
//! 泛型面：接收者类型参数（`recv_tparams`）的替换归 `rgoc-types` 的
//! `instantiate`（Task 12）；本面在方法表达式 / 方法值上构造的签名
//! `recv_tparams` 恒空（窄面无泛型接收者）。

use rgoc_types::{TupleId, Type, TypeId, TypeTable, unalias};

/// 元组类型条目 → `TupleId`（`SignatureDef.params/results` 存的是元组的 `TypeId`）。
fn tuple_of(tt: &TypeTable, t: TypeId) -> Option<TupleId> {
    match tt.type_of(t) {
        Type::Tuple(tid) => Some(*tid),
        _ => None,
    }
}

/// 选择类别（官方 `SelectionKind`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SelKind {
    /// 字段值（官方 `FieldVal`）。
    FieldVal,
    /// 方法值（官方 `MethodVal`）。
    MethodVal,
    /// 方法表达式（官方 `MethodExpr`）。
    MethodExpr,
}

/// 方法值：签名**去掉接收者**（官方 `sig.recv = nil`）。
///
/// 拆不出签名（畸形 / 错误恢复）⇒ 原样带回（官方此处不设防；
/// 我们的表允许别名签名，兜底避免恐慌）。
pub(crate) fn method_value_sig(tt: &mut TypeTable, sig: TypeId) -> TypeId {
    let s = unalias(tt, sig);
    let Type::Signature(sid) = tt.type_of(s) else {
        return sig;
    };
    let d = tt.signature_def(*sid).clone();
    let (Some(params), Some(results)) = (tuple_of(tt, d.params), tuple_of(tt, d.results)) else {
        return sig; // 畸形（参数 / 结果位不是元组）：原样带回
    };
    let id = tt.add_signature(None, params, results, d.variadic);
    tt.signature_type(id)
}

/// 方法表达式：接收者**作首参**（官方
/// `params = append([]*Var{NewParam(..., x.typ())}, params...)`）。
///
/// 参数名照官方「保持命名一致性」的简化面：首参无名（`None` ⇒ dump `-`）。
pub(crate) fn method_expr_sig(tt: &mut TypeTable, recv_ty: TypeId, sig: TypeId) -> TypeId {
    let s = unalias(tt, sig);
    let Type::Signature(sid) = tt.type_of(s) else {
        return sig;
    };
    let d = tt.signature_def(*sid).clone();
    let Some(results) = tuple_of(tt, d.results) else {
        return sig; // 畸形（结果位不是元组）：原样带回
    };
    let mut vars: Vec<(Option<String>, TypeId)> = vec![(None, recv_ty)];
    if let Some(tid) = tuple_of(tt, d.params) {
        vars.extend(tt.tuple_def(tid).vars.iter().cloned());
    }
    let params = tt.add_tuple(vars);
    let id = tt.add_signature(None, params, results, d.variadic);
    tt.signature_type(id)
}
