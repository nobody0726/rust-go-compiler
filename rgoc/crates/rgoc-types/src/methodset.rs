//! 方法集（M4/Task 7；官方 `go/types/methodset.go::NewMethodSet` 逐条对齐）。
//!
//! # 判据来源
//!
//! * `methodset.go:72` 起 `NewMethodSet` —— 开头两条早退（具名指针类型 /
//!   `*I`）、逐深度收集（`named` 方法先收、`struct` 记字段 + 嵌出下一层、
//!   `interface` 方法 join）、`mset`/`fset` 合并、`consolidateMultiples`；
//! * `lookup.go:269` 起 `methodSet.addOne` / `add`、`embeddedType`、
//!   `consolidateMultiples`、`lookupType`；
//! * 语义探针（Task 7 记录见 `.superpowers/sdd/M4-plan/progress.md`）复核了
//!   「指针接收者在值方法集里**占名遮蔽**」「同级歧义」「多倍数」「字段遮蔽」
//!   「具名指针类型」「递归类型」「`*I`」这七类边界，与三方 `go/types` 一致。
//!
//! # 本层与官方的两处已知差异（都不在 M4 判定面上）
//!
//! 1. **嵌入别名的字段名**：官方 `Field.Id()` 是源码里写的类型名（
//!    `type A = T; struct{ A }` 记 `"A"`）；本表对嵌入字段只存类型
//!    （`name == None`）⇒ 派生出的名字是**剥壳后的具名名**（`"T"`）。
//!    影响面仅限「字段名与更深层方法同名」的遮蔽判定，且别名嵌入是罕见写法；
//!    真需要时在 `Field` 上加存声明名（`C7` 不受影响）。
//! 2. **`identical` 的 Err 透传**：`consolidateMultiples` 的同一性快径走
//!    `TypeId` 相等；慢径 `identical` 在含项集/`comparable` 的接口对上会
//!    显式拒绝（`U-M3-05` 边界）⇒ 本函数**透传**该 `Err`。这类形态做嵌入
//!    字段本来就是非法 Go（类型集接口不能作普通类型），检查器会先报错。
//!
//! # 早退与输入面
//!
//! * `invalid` / 非类型形态 ⇒ 空集（官方 `Underlying()` 走不进任何分支）；
//! * 类型参数 ⇒ 官方走 `TypeParam.Underlying()`（= 约束接口），本层
//!   `underlying` 同样会剥到约束 —— 方法集 = 约束声明的方法（与官方近似一致）。
//! * `seen` 只在**具名**条目上记录（官方 `asNamed` 口径），递归类型因此不挂。

use std::collections::{BTreeMap, BTreeSet};

use rgoc_lex::diag::DiagCode;

use crate::identical::{identical, unalias, underlying};
use crate::table::{Field, TypeTable};
use crate::typ::{Type, TypeId};

/// 方法集的一条：名字 + 签名。
///
/// `sig` 存**签名类型在本表里的 `TypeId`**（与官方 `Selection.obj.Type()` 同层）。
/// ⚠️ 计划（`M4-plan.md` Task 7）写的是 `SignatureId`，实现取 `TypeId`
/// （**Ruling 7-2**）：① 免掉「拆包 fail ⇒ panic」一层 —— 别名 / 错误恢复的
/// 签名可能不是直接 `Type::Signature`；② 消费者（`implements` / 选择器）的
/// 签名比较本来就用 `identical`（吃 `TypeId`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MSetEntry {
    /// 方法名。
    pub name: String,
    /// 签名类型条目（`Type::Signature` 的那个 `TypeId`；`recv` 决定指针性）。
    pub sig: TypeId,
}

/// 某个深度上的一个「条目」（官方 `embeddedType`；`index` 不参与判定，省略）。
#[derive(Clone, Copy, Debug)]
struct Embedded {
    /// 已经**剥过一层指针**的类型条目。
    typ: TypeId,
    /// 到达它的路径上出现过指针（值方法集 ⇒ `*T` 嵌入也提升指针接收者方法）。
    indirect: bool,
    /// 同一深度上出现过**多次**同一类型（`consolidateMultiples` 标记）。
    multiples: bool,
}

/// 方法集（官方 `NewMethodSet`；结果按名字升序）。
///
/// * `Ok(list)` —— 判得出（含「空集」这一合法答案）；
/// * `Err` —— `consolidateMultiples` 的同一性慢径撞上 `identical` 的 M3
///   边界（含项集/`comparable` 的接口对；见模块文档）。
///
/// # Errors
///
/// 只在上述边界形态上出错；普通类型（含 `invalid`）一律 `Ok`。
pub fn methodset(tt: &TypeTable, typ: TypeId) -> Result<Vec<MSetEntry>, DiagCode> {
    // ① 具名指针类型（`type P *T`）**没有**方法集（官方函数首段）。
    let t0 = unalias(tt, typ);
    if matches!(tt.type_of(t0), Type::Named(_))
        && matches!(tt.type_of(underlying(tt, t0)), Type::Pointer(_))
    {
        return Ok(Vec::new());
    }

    // ② `deref`：剥**一层**指针（剥不出来 ⇒ `indirect = false`）。
    let (cur, is_ptr) = deref(tt, t0);

    // ③ `*I` 没有方法（官方 `isPtr && IsInterface`）。
    if is_ptr && is_iface(tt, cur) && !matches!(tt.type_of(cur), Type::TypeParam(_)) {
        return Ok(Vec::new());
    }

    // 当前深度的方法组 `mset` / 字段名组 `fset`；累计面 `base`（`None` = 碰撞位）。
    let mut base: BTreeMap<String, Option<MSetEntry>> = BTreeMap::new();
    // `seen`：**具名**类型在**更浅深度**出现过的记录（后见者被遮蔽）。
    let mut seen: Vec<TypeId> = Vec::new();
    let mut current: Vec<Embedded> = vec![Embedded {
        typ: cur,
        indirect: is_ptr,
        multiples: false,
    }];

    while !current.is_empty() {
        let mut next: Vec<Embedded> = Vec::new();
        let mut fset: BTreeSet<String> = BTreeSet::new();
        let mut mset: BTreeMap<String, Option<MSetEntry>> = BTreeMap::new();

        for e in &current {
            let e_ty = unalias(tt, e.typ);

            // 具名：先收它的方法（官方「look for a matching method first」）。
            if let Type::Named(nid) = tt.type_of(e_ty) {
                if seen.contains(&e_ty) {
                    continue; // 更浅的深度见过 ⇒ 被遮蔽
                }
                seen.push(e_ty);
                for (mname, msig) in &tt.named_def(*nid).methods {
                    add_one(tt, &mut mset, mname, *msig, e.indirect, e.multiples);
                }
            }

            match tt.type_of(underlying(tt, e_ty)) {
                Type::Struct(sid) => {
                    for f in &tt.struct_def(*sid).fields {
                        // 字段名（含嵌字段的派生名）占名：在**同一深度**与方法
                        // 碰撞，还会遮蔽更深处的方法。
                        fset.insert(field_name(tt, f));
                        if f.embedded {
                            let (ft, fptr) = deref(tt, f.typ);
                            next.push(Embedded {
                                typ: ft,
                                indirect: e.indirect || fptr,
                                multiples: e.multiples,
                            });
                        }
                    }
                }
                Type::Interface(iid) => {
                    // 接口方法无条件可提升 ⇒ `indirect = true`（官方同）。
                    for (mname, msig) in &tt.interface_def(*iid).methods {
                        add_one(tt, &mut mset, mname, *msig, true, e.multiples);
                    }
                }
                _ => {}
            }
        }

        // 本深度的方法并入累计面：已存在的名字（更浅）优先。
        for (k, m) in mset {
            // 同深度的字段与方法同名 ⇒ 方法当场作废（官方碰撞语义）。
            let m = if fset.contains(&k) { None } else { m };
            base.entry(k).or_insert(m);
        }
        // 本深度的字段名剩下的全部记为碰撞（它们遮蔽更深处的方法）。
        for k in fset {
            base.entry(k).or_insert(None);
        }

        current = consolidate_multiples(tt, next)?;
    }

    Ok(base.into_values().flatten().collect())
}

/// 官方 `methodSet.addOne`：加一条，或**记碰撞**（`None`）。
///
/// 关键语义（探针复核）：`!indirect && 指针接收者` 的方法**不加入**，但仍然
/// 占名 —— 同一深度后来的同名方法也进不来，更深处嵌出的同名方法同样被遮蔽。
fn add_one(
    tt: &TypeTable,
    mset: &mut BTreeMap<String, Option<MSetEntry>>,
    name: &str,
    sig: TypeId,
    indirect: bool,
    multiples: bool,
) {
    if !multiples && !mset.contains_key(name) && (indirect || !has_ptr_recv(tt, sig)) {
        mset.insert(
            name.to_string(),
            Some(MSetEntry {
                name: name.to_string(),
                sig,
            }),
        );
        return;
    }
    mset.insert(name.to_string(), None); // 碰撞
}

/// 官方 `consolidateMultiples`：同一深度上**同类型**的多个条目合并成一个、
/// 打上 `multiples`（它的方法将全部作废）。
///
/// 同一性快径 = `TypeId` 相等（本表的具名/接口条目是规范条目）；慢径
/// `identical`（官方 `lookupType` 的慢径同形 —— 官方为等价实例准备）。
fn consolidate_multiples(tt: &TypeTable, list: Vec<Embedded>) -> Result<Vec<Embedded>, DiagCode> {
    if list.len() <= 1 {
        return Ok(list);
    }
    let mut out: Vec<Embedded> = Vec::with_capacity(list.len());
    for e in list {
        let mut merged = false;
        for o in &mut out {
            if o.typ == e.typ || identical(tt, o.typ, e.typ)? {
                o.multiples = true;
                merged = true;
                break;
            }
        }
        if !merged {
            out.push(e);
        }
    }
    Ok(out)
}

/// 官方 `deref`：先透明别名，再剥**一层**指针。
fn deref(tt: &TypeTable, t: TypeId) -> (TypeId, bool) {
    let t = unalias(tt, t);
    if let Type::Pointer(e) = tt.type_of(t) {
        (unalias(tt, *e), true)
    } else {
        (t, false)
    }
}

/// 官方 `IsInterface`（`Underlying` 是接口；**类型参数除外**）。
fn is_iface(tt: &TypeTable, t: TypeId) -> bool {
    matches!(tt.type_of(underlying(tt, t)), Type::Interface(_))
}

/// 官方 `Func.hasPtrRecv`：签名有接收者且接收者类型**底层是指针**。
///
/// 拆不出签名（畸形 / 错误恢复 / 接口方法的 `recv == None`）⇒ `false`。
fn has_ptr_recv(tt: &TypeTable, sig: TypeId) -> bool {
    let s = unalias(tt, sig);
    let Type::Signature(sid) = tt.type_of(s) else {
        return false;
    };
    let Some(recv) = tt.signature_def(*sid).recv else {
        return false;
    };
    matches!(tt.type_of(underlying(tt, recv)), Type::Pointer(_))
}

/// 字段名（官方 `Field.Id()`）。
///
/// * 普通字段：表里的名字；
/// * 嵌入字段：本表 `name == None` ⇒ 派生「剥壳后的具名名」
///   （官方记的是**源码里写的类型名** —— 别名嵌入的差异见模块文档第 1 条；
///   派生不出来的（非法 Go）退化为空串，不与任何方法名碰撞）。
fn field_name(tt: &TypeTable, f: &Field) -> String {
    if let Some(n) = &f.name {
        return n.clone();
    }
    let (t, _) = deref(tt, f.typ);
    match tt.type_of(unalias(tt, t)) {
        Type::Named(n) => tt.named_def(*n).name.clone(),
        _ => String::new(),
    }
}
