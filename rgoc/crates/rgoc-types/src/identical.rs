//! 类型同一性判定（M3/Task 17+18；官方 `go/types` 的 `Identical`）。
//!
//! # 判据来源
//!
//! `go/types/predicates.go` 的 `(*comparer).identical`（`F4` 的分派顺序）。
//! 逐条对齐的要点：
//!
//! 1. **第一步就是 `Unalias`**（两侧）—— 别名在判定里**完全透明**。
//!    所以 [`unalias`] 是公开的（`T-TY-23` 直接断言它）。
//! 2. **定义型按定义身份**（`*Named` 比 `obj`）：`type A int` 与 `int` **不等**，
//!    两个同名但不同定义的 `A` 也**不等**。**实例**另有官方口径（M4/Task 12）：
//!    同来源 + 实参**逐对同一** ⇒ 同一（`predicates.go` 的 `*Named` 臂）；
//!    实例与其**原点**（实参数不同）⇒ 不等。
//! 3. **数组长度**：官方是 `len < 0 || x.len == y.len` —— 长度**未定**时**不当**不等
//!    （错误恢复的宽容面）。本层 `len == None`（`[...]T`）与 `Const::Unknown`
//!    （求值失败）都按「未定」处理。
//! 4. **结构体**：字段数 + 逐字段（**嵌入标记 / 名字 / 类型**；**Tag** 只在严格面
//!    参与 —— [`identical_ignore_tags`] 是官方 `IdenticalIgnoreTags`
//!    （`comparer{ignoreTags: true}`），标签不参与）。
//!    名字也要比 —— 嵌入字段的名字是**被嵌入类型的名字**，
//!    `struct{ Z }` 与 `struct{ W }` 即使 `Z`/`W` 底层相同也**不等**。
//! 5. **元组**：只比元素类型，**名字不参与**（spec：「Parameter and result names
//!    are not required to match」）。
//! 6. **签名**：`variadic` + params + results；**接收者不参与**（官方也没比）。
//!    M3 没有泛型签名 ⇒ 无类型参数替换那一段（`D-M3-2` 的划界）。
//! 7. **接口**：比官方**完整形式**（`predicates.go:401`）：`comparable` 位 →
//!    项集（`termlist.equal` = 双向 `subsetOf`；term 间是官方 `term.subsetOf`
//!    里的 **plain** `Identical` —— `ignore_tags` **不穿透**到这一层）→
//!    方法集（名 + 签名，**与顺序无关**）。
//!    M4/Task 10 起不再有「只判方法集版」的窄面 —— `U-M3-05` 已解除。
//! 8. **类型参数 / 并集**（官方独立形态）：类型参数只有**同一条目**才真
//!    （官方 `case *TypeParam:` 注释：nothing to do）；并集比项表相等
//!    （`computeUnionTypeSet(...).terms.equal(...)`，`predicates.go:383`）。
//!
//! # 为什么返回 `Result` 而不是 `bool`
//!
//! **M3 的历史**：`D-M3-13` 对**约束接口**（含项集 `~T`/并集，或 `comparable`）
//! 的同一性 M3 **不判**（`U-M3-05`）。「不判」与「不等」是两回事：
//!
//! * 返回 `Ok(false)` 会在上游造出一条**假的不匹配诊断**；
//! * 返回 `Ok(true)` 更糟 —— 把「没查」伪装成「查过且相等」。
//!
//! 所以当时走显式拒绝码 `TypUnsupportedConstraintIdentity`（`T-TY-81`），
//! 计划里写的 `-> bool` 表达不了那个「稳定拒绝码」——这是**刻意的接口偏离**
//! （记在 `.superpowers/sdd/M3-plan/progress.md`）。
//!
//! **M4 的现状（Task 10）**：类型集版判定已落地（上一节 7/8 的口径），本函数
//! **不再产出 `Err`** —— 拒点已删、码已从 `DiagCode` 移除。`Result` 形态**保留**：
//! 调用面（`assignable` / `convertible` / `implements` / `methodset` / `typeset`）
//! 全以 `?` 串接，没有真实的 `Err` 用例为一次签名 churn 买单。
//!
//! # 为什么不需要官方的 `ifacePair`
//!
//! 官方要一个「正在比较的接口对」栈来挡环（`type I interface{ M() I }`）。
//! 本层**结构上不可能**成环：`Type` 里唯一能自引用的入口是 `named`/`alias`，
//! 而 `alias` 被 [`unalias`] 剥掉、`named` **按定义身份比**、**不下钻本体**
//! ⇒ 接口只有在「两侧都是具名接口」时才不会被比到本体，匿名递归接口写不出来。
//! 方法签名与类型集（[`crate::typeset::subset_of`]）两条递归路都走本函数，
//! 同一条论证对它们同样成立。（`dump` 侧曾踩过 oracle 的成环坑，见 `C7` §5.1；
//! 那条路是**非法程序**才有的。）

use rgoc_const::Const;
use rgoc_lex::diag::DiagCode;

use crate::table::TypeTable;
use crate::typ::{Type, TypeId};
use crate::typeset::{IfaceTSet, subset_of, terms_equal};

/// 剥掉**别名链**（官方 `types.Unalias`）。
///
/// `type A = B; type B = C; type C = int` ⇒ 返回 `int` 那个条目。
///
/// 别名链成环（`type A = B; type B = A`）是**非法 Go**，本层不管
/// （检查器必须先诊断它）；这里只保证**不静默死循环** —— 成环即响亮 panic。
///
/// # Panics
///
/// 别名链成环时 panic（那是上游的缺陷，不是用户输入的问题）。
#[must_use]
pub fn unalias(tt: &TypeTable, t: TypeId) -> TypeId {
    let mut cur = t;
    let mut seen: Vec<TypeId> = Vec::new();
    while let Type::Alias(a) = tt.type_of(cur) {
        if seen.contains(&cur) {
            panic!(
                "别名链成环（`type A = B; type B = A`）—— 非法 Go，\
                 应在检查器里诊断；判定层不接受这种表"
            );
        }
        seen.push(cur);
        cur = tt.alias_rhs(*a);
    }
    cur
}

/// 剥到**底层类型**（官方 `types.Type.Underlying`）。
///
/// * `named` 沿 [`TypeTable::named_underlying`] 逐层剥 —— 构造协议存的是**规约过**
///   的底层，但表是开放的（测试可以塞链条），递归循环仍要防：环是上游缺陷 ⇒
///   响亮 panic，与 [`unalias`] 同规；
/// * `typeparam` 取**约束**（官方 `TypeParam.underlying()` 返回类型集的接口形态，
///   本层取约束本身；约束若为具名接口，下一圈再剥到接口本体）；
/// * `Alias` 每圈都剥（构造协议不该在链上留别名，留了也透明处理）；
/// * 其余形态（基本型 / 结构型 / 接口 / 通道 / …）就是自身。
///
/// # Panics
///
/// `named` / `typeparam` 的底层链成环（非法 Go）时 panic —— 与 [`unalias`] 同理。
#[must_use]
pub fn underlying(tt: &TypeTable, t: TypeId) -> TypeId {
    let mut cur = unalias(tt, t);
    let mut seen: Vec<TypeId> = Vec::new();
    loop {
        let next = match tt.type_of(cur) {
            Type::Named(n) => tt.named_underlying(*n),
            Type::TypeParam(p) => tt.type_param_def(*p).constraint,
            // 不该出现（`unalias` 与本表构造协议都会先处理），留作透明兜底
            Type::Alias(_) => {
                cur = unalias(tt, cur);
                continue;
            }
            _ => return cur,
        };
        if seen.contains(&cur) {
            panic!(
                "底层链成环（`named`/`typeparam` 自引用）—— 非法 Go，\
                 应在检查器里诊断；判定层不接受这种表"
            );
        }
        seen.push(cur);
        cur = next;
    }
}

/// 类型同一性（官方 `types.Identical` 的完整支持面；M4/Task 10 起含
/// **类型集版 interface** —— `comparable` 位 + 项集 + 方法集，`U-M3-05` 解除）。
///
/// # Errors
///
/// **不再有**：M3 的能力边界 `U-M3-05`（`TypUnsupportedConstraintIdentity`）
/// 由 M4/Task 10 解除，该码已从 `DiagCode` 移除；`Result` 形态为调用面稳定保留
/// （模块头「为什么返回 `Result`」）。
pub fn identical(tt: &TypeTable, x: TypeId, y: TypeId) -> Result<bool, DiagCode> {
    identical_cmp(tt, x, y, false)
}

/// 官方 `types.IdenticalIgnoreTags`（`comparer{ignoreTags: true}`）：
/// 结构体字段的**标签原文不参与**，其余与 [`identical`] 逐字相同。
///
/// 消费点：`convertible` 的规则 3/4（官方 `conversions.go` 直调这一面）。
///
/// # Errors
///
/// 与 [`identical`] 同。
pub fn identical_ignore_tags(tt: &TypeTable, x: TypeId, y: TypeId) -> Result<bool, DiagCode> {
    identical_cmp(tt, x, y, true)
}

fn identical_cmp(
    tt: &TypeTable,
    x: TypeId,
    y: TypeId,
    ignore_tags: bool,
) -> Result<bool, DiagCode> {
    let x = unalias(tt, x);
    let y = unalias(tt, y);
    if x == y {
        return Ok(true);
    }
    match tt.type_of(x) {
        // `invalid` 是「同一个种类」的一类（官方 `Typ[Invalid]` 是单例 ⇒ `x == y` 就中；
        // 本层有多个 `invalid` 条目会落到这里，按种类判才对得上）。
        Type::Invalid => Ok(matches!(tt.type_of(y), Type::Invalid)),
        Type::Basic(a) => Ok(matches!(tt.type_of(y), Type::Basic(b) if b == a)),
        // 定义型：`a == b`（同一条目）⇒ 真；否则看**实例**口径 ——
        // 官方 `predicates.go` 的 `*Named` 臂：同来源（本层 = 同 origin）
        // 且实参逐对同一才对。memo 保证正常路径下同 (origin, 实参组)
        // 只有一个实例条目，这里主要守「实例 vs 原点」（实参数不同 ⇒ 假）。
        Type::Named(a) => {
            let Type::Named(b) = tt.type_of(y) else {
                return Ok(false);
            };
            if a == b {
                return Ok(true);
            }
            let da = tt.named_def(*a);
            let db = tt.named_def(*b);
            let (Some(oa), Some(ob)) = (da.origin, db.origin) else {
                return Ok(false);
            };
            if oa != ob || da.targs.len() != db.targs.len() {
                return Ok(false);
            }
            for (ta, tb) in da.targs.iter().zip(&db.targs) {
                if !identical_cmp(tt, *ta, *tb, ignore_tags)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        // `unalias` 之后不可能还有别名
        Type::Alias(_) => unreachable!("unalias 之后不该还有 alias"),
        Type::Pointer(px) => match tt.type_of(y) {
            Type::Pointer(py) => identical_cmp(tt, *px, *py, ignore_tags),
            _ => Ok(false),
        },
        Type::Slice(ex) => match tt.type_of(y) {
            Type::Slice(ey) => identical_cmp(tt, *ex, *ey, ignore_tags),
            _ => Ok(false),
        },
        Type::Array { len: lx, elem: ex } => match tt.type_of(y) {
            Type::Array { len: ly, elem: ey } => {
                if !array_len_same(tt, *lx, *ly) {
                    return Ok(false);
                }
                identical_cmp(tt, *ex, *ey, ignore_tags)
            }
            _ => Ok(false),
        },
        Type::Map { key: kx, elem: ex } => match tt.type_of(y) {
            Type::Map { key: ky, elem: ey } => {
                if !identical_cmp(tt, *kx, *ky, ignore_tags)? {
                    return Ok(false);
                }
                identical_cmp(tt, *ex, *ey, ignore_tags)
            }
            _ => Ok(false),
        },
        // 方向**必须**一致（`chan int` 与 `<-chan int` 不等）
        Type::Chan { dir: dx, elem: ex } => match tt.type_of(y) {
            Type::Chan { dir: dy, elem: ey } => {
                if dx != dy {
                    return Ok(false);
                }
                identical_cmp(tt, *ex, *ey, ignore_tags)
            }
            _ => Ok(false),
        },
        Type::Struct(sx) => match tt.type_of(y) {
            Type::Struct(sy) => {
                let a = tt.struct_def(*sx);
                let b = tt.struct_def(*sy);
                if a.fields.len() != b.fields.len() {
                    return Ok(false);
                }
                for (f, g) in a.fields.iter().zip(&b.fields) {
                    // 嵌入标记 / 名字（嵌入字段的名字 = 被嵌入类型的名字）；
                    // Tag 只在**严格面**参与（`ignore_tags` 见函数文档）
                    if f.embedded != g.embedded
                        || (!ignore_tags && f.tag != g.tag)
                        || f.name != g.name
                    {
                        return Ok(false);
                    }
                    if !identical_cmp(tt, f.typ, g.typ, ignore_tags)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            _ => Ok(false),
        },
        Type::Tuple(tx) => match tt.type_of(y) {
            Type::Tuple(ty) => {
                let a = tt.tuple_def(*tx);
                let b = tt.tuple_def(*ty);
                if a.vars.len() != b.vars.len() {
                    return Ok(false);
                }
                // **名字不参与**（spec：参数/结果名不必相同）
                for ((_, p), (_, q)) in a.vars.iter().zip(&b.vars) {
                    if !identical_cmp(tt, *p, *q, ignore_tags)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            _ => Ok(false),
        },
        Type::Signature(sx) => match tt.type_of(y) {
            Type::Signature(sy) => {
                let a = tt.signature_def(*sx);
                let b = tt.signature_def(*sy);
                // 接收者**不参与**（官方 `identical` 只比 variadic / params / results）
                if a.variadic != b.variadic {
                    return Ok(false);
                }
                // `SignatureDef` 里的 params/results 已经是 `TypeId`（表内从
                // `TupleId` 转过来了）⇒ 直接递归
                if !identical_cmp(tt, a.params, b.params, ignore_tags)? {
                    return Ok(false);
                }
                identical_cmp(tt, a.results, b.results, ignore_tags)
            }
            _ => Ok(false),
        },
        Type::Interface(ix) => match tt.type_of(y) {
            Type::Interface(iy) => {
                let a = tt.interface_def(*ix);
                let b = tt.interface_def(*iy);
                // 判据逐条对齐官方（`predicates.go:401` 的 `case *Interface`）：
                // ① `comparable` 位必须相同。官方的不变量是
                //    `!comparable || terms.isAll()`（`typeset.go:32` 的 `_TypeSet`）
                //    ⇒ 四态投影里 `Comparable` 就是「位为真」的**唯一**形态。
                if matches!(a.tset, IfaceTSet::Comparable)
                    != matches!(b.tset, IfaceTSet::Comparable)
                {
                    return Ok(false);
                }
                // ② 项集相等：官方 `termlist.equal` = **双向** `subsetOf`
                //    （集合语义，不看列表形状 —— 置换序 / 未归一都判对）。
                //    `subset_of` 是官方原函数；term 间用 **plain** `Identical`，
                //    `ignore_tags` 不穿透（官方 `term.subsetOf` 就是这么写的）。
                if !subset_of(tt, &a.tset, &b.tset)? || !subset_of(tt, &b.tset, &a.tset)? {
                    return Ok(false);
                }
                // ③ 方法集（官方比 `xset.methods`，**已展开**；本层
                //    `InterfaceDef::methods` 在装载期展开 + 按名去重（Task 8），
                //    这里按名排序后逐位比；「Id 无包名前缀」是 v1 已知差异，Ruling 8-4）。
                if a.methods.len() != b.methods.len() {
                    return Ok(false);
                }
                // 方法**与顺序无关**（官方：`The order of the methods is irrelevant`）
                let mut am: Vec<(&str, TypeId)> =
                    a.methods.iter().map(|(n, s)| (n.as_str(), *s)).collect();
                let mut bm: Vec<(&str, TypeId)> =
                    b.methods.iter().map(|(n, s)| (n.as_str(), *s)).collect();
                am.sort_unstable_by_key(|(n, _)| *n);
                bm.sort_unstable_by_key(|(n, _)| *n);
                for i in 0..am.len() {
                    let (n1, s1) = am[i];
                    let (n2, s2) = bm[i];
                    if n1 != n2 {
                        return Ok(false);
                    }
                    if !identical_cmp(tt, s1, s2, ignore_tags)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            _ => Ok(false),
        },
        // 类型参数：官方 `case *TypeParam:` —— **nothing to do**（同一条目在开头的
        // `x == y` 就短路了）⇒ 不同条目**不等**（哪怕约束同形）。
        Type::TypeParam(_) => Ok(false),
        // 并集：官方 `case *Union:` 比两个**类型集**的项表
        //（`computeUnionTypeSet(...).terms.equal(...)`，集合语义双向包含）。
        // v1 口径（Ruling 8-2）：`UnionDef.terms` 是 **as-written** 项，官方则
        // 逐项展开（接口成员展开成它的项表；非法的 `~T ∧ under(T) ≠ T` 变 ∅）——
        // 只在**非法 / 极端**形态上分叉，那些形态先由检查器诊断。
        Type::Union(ux) => match tt.type_of(y) {
            Type::Union(uy) => {
                let a = tt.union_def(*ux);
                let b = tt.union_def(*uy);
                terms_equal(tt, &a.terms, &b.terms)
            }
            _ => Ok(false),
        },
    }
}

/// 数组长度是否「同」（官方 `len < 0 || x.len == y.len`）。
///
/// * `None` = 长度**未定**（`[...]T`）——比错了会冤枉合法代码，故视为「同」；
/// * `Const::Unknown` = 求值失败（官方此时长度也是负值）——同样宽容；
/// * 其余按**常量值**比（所以 `[1+1]int` 与 `[2]int` 同）。
fn array_len_same(
    tt: &TypeTable,
    x: Option<rgoc_const::ConstId>,
    y: Option<rgoc_const::ConstId>,
) -> bool {
    match (x, y) {
        (None, _) | (_, None) => true,
        (Some(a), Some(b)) => match (tt.consts().get(a), tt.consts().get(b)) {
            (Const::Unknown, _) | (_, Const::Unknown) => true,
            (p, q) => p == q,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unalias_剥掉整条链() {
        let mut tt = TypeTable::new();
        let int = tt.add_basic(rgoc_const::BasicId::Int);
        let c = tt.add_alias("C");
        tt.set_alias_rhs(c, int);
        let b = tt.add_alias("B");
        tt.set_alias_rhs(b, tt.alias_type(c));
        let a = tt.add_alias("A");
        tt.set_alias_rhs(a, tt.alias_type(b));

        assert_eq!(unalias(&tt, tt.alias_type(a)), int);
        // 非别名原样返回
        assert_eq!(unalias(&tt, int), int);
    }

    #[test]
    #[should_panic(expected = "别名链成环")]
    fn unalias_成环响亮报错() {
        let mut tt = TypeTable::new();
        // `type A = B; type B = A`（非法 Go；先占号才能造出来）
        let a = tt.add_alias("A");
        let b = tt.add_alias("B");
        tt.set_alias_rhs(a, tt.alias_type(b));
        tt.set_alias_rhs(b, tt.alias_type(a));
        let _ = unalias(&tt, tt.alias_type(a));
    }
}
