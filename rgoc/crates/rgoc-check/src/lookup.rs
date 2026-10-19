//! 字段 / 方法查找（M4/Task 24；官方 `go/types/lookup.go::lookupFieldOrMethod`
//! 的窄面逐条对齐）。
//!
//! # 判据来源
//!
//! * `lookup.go:97` 起 `lookupFieldOrMethod`（具名指针类型的特例）；
//! * `lookup.go:147` 起 `lookupFieldOrMethodImpl`（逐深度 BFS / `seen` 遮蔽 /
//!   同级碰撞 / 指针接收者的可寻址回退）；
//! * `lookup.go:283` `consolidateMultiples` / `lookup.go:629` `deref`。
//!
//! # 与官方的已知差异（都不在 M4 判定面上）
//!
//! 1. **无 `pkg` 维度**：本检查面是单包（导入可见性缝合归 Task 30+）——
//!    名字匹配用**裸串相等**，不查导出性 / 大小写折叠（官方 `foldCase` 的
//!    `lookupError` 备选拼写建议因此不产生）。
//! 2. **嵌入字段名的推导线**：本表嵌入字段只存类型（`name == None`）⇒ 派生名
//!    是剥壳后的具名名（与 `methodset` 模块文档第 1 条同源）。
//! 3. **`seen` 用 `TypeId` 相等**（官方按 `identical`）：泛型实例未进入本面
//!    （实例化是 `rgoc-types` 的 Task 12 面），无同构异条目。
//!
//! # 结果三态（官方返回 `(obj, index, indirect)` 的含义表）
//!
//! | 情形 | `obj` | `index` | `indirect` |
//! |---|---|---|---|
//! | 找到 | `Some` | `Some(路径)` | 路径上有指针解引用 |
//! | 同级歧义 | `None` | `Some(碰撞路径)` | `false` |
//! | 需指针接收者但不可寻址 | `None` | `None` | `true` |
//! | 没找到 | `None` | `None` | `false` |

use rgoc_types::{Field, Type, TypeId, TypeTable, identical, unalias, underlying};

/// 查到的条目（官方结果是 `*Var`（字段）或 `*Func`（方法））。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Found {
    /// 字段（含嵌入字段自身）。
    Field {
        /// 字段类型（官方 `Var.typ`）。
        typ: TypeId,
    },
    /// 方法（含接口方法）；`sig` 是带接收者的方法签名 ——
    /// 指针性判据是 `recv` 底层是不是指针（官方 `hasPtrRecv`）。
    Method {
        /// 签名类型条目。
        sig: TypeId,
    },
}

/// 查找结果（见模块文档的三态表）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Lookup {
    /// 命中的对象；`None` ⇒ 没找到 / 歧义 / 需指针接收者。
    pub obj: Option<Found>,
    /// 找到（或碰撞）的路径：逐层嵌入字段下标 + 末位字段 / 方法下标。
    pub index: Option<Vec<usize>>,
    /// 成功时 = 路径上有指针；失败时 = 需要指针接收者但接收者不可寻址。
    pub indirect: bool,
}

impl Lookup {
    /// 「没找到」态。
    fn none() -> Self {
        Self {
            obj: None,
            index: None,
            indirect: false,
        }
    }

    /// 「同级歧义」态。
    fn collision(index: Vec<usize>) -> Self {
        Self {
            obj: None,
            index: Some(index),
            indirect: false,
        }
    }

    /// 「需指针接收者」态。
    fn need_ptr_recv() -> Self {
        Self {
            obj: None,
            index: None,
            indirect: true,
        }
    }
}

/// 官方 `lookupFieldOrMethod`（`lookup.go:97`）。
///
/// `addressable`：接收者是不是**可寻址变量**（只影响方法查找 ——
/// 指针接收者方法在不可寻址值上找不到）。
pub(crate) fn lookup_field_or_method(
    tt: &TypeTable,
    typ: TypeId,
    addressable: bool,
    name: &str,
) -> Lookup {
    // 具名指针类型（`type P *T`）：用**底层指针**查，但方法结果作废
    // （官方注释：方法不能挂在具名指针类型上）。
    let t0 = unalias(tt, typ);
    if matches!(tt.type_of(t0), Type::Named(_)) {
        let under = underlying(tt, t0);
        if matches!(tt.type_of(under), Type::Pointer(_)) {
            let r = lookup_impl(tt, under, false, name);
            return if matches!(r.obj, Some(Found::Method { .. })) {
                Lookup::none()
            } else {
                r
            };
        }
    }
    lookup_impl(tt, typ, addressable, name)
}

/// 逐深度 BFS 的骨架（官方 `lookupFieldOrMethodImpl`，`lookup.go:147`）。
fn lookup_impl(tt: &TypeTable, bare: TypeId, addressable: bool, name: &str) -> Lookup {
    // 空白标识符永远找不到（官方首行）。
    if name == "_" {
        return Lookup::none();
    }

    let (typ0, is_ptr) = deref(tt, bare);
    // `*I`（接口指针）没有方法（官方 `lookup.go:196`）。
    if is_ptr && matches!(tt.type_of(underlying(tt, typ0)), Type::Interface(_)) {
        return Lookup::none();
    }

    let mut obj: Option<Found> = None;
    let mut index: Option<Vec<usize>> = None;
    let mut indirect = false;
    let mut seen: Vec<TypeId> = Vec::new();
    let mut current = vec![Embedded {
        typ: typ0,
        index: Vec::new(),
        indirect: is_ptr,
        multiples: false,
    }];

    while !current.is_empty() {
        let mut next: Vec<Embedded> = Vec::new();
        for e in &current {
            let t = e.typ;
            let tn = unalias(tt, t);

            // 具名类型：先查挂载的方法，并做 `seen` 遮蔽。
            if let Type::Named(nid) = tt.type_of(tn) {
                if seen.contains(&tn) {
                    // 更浅深度出现过 ⇒ 本处忽略（官方注释：shadowed）。
                    continue;
                }
                seen.push(tn);
                if let Some((i, (_, sig))) = tt
                    .named_def(*nid)
                    .methods
                    .iter()
                    .enumerate()
                    .find(|(_, (n, _))| n == name)
                {
                    let mut idx = e.index.clone();
                    idx.push(i);
                    if obj.is_some() || e.multiples {
                        return Lookup::collision(idx);
                    }
                    index = Some(idx);
                    obj = Some(Found::Method { sig: *sig });
                    indirect = e.indirect;
                    // 有方法就不会再有同名字段 —— 跳过本条的 underlying 面。
                    continue;
                }
            }

            // 官方 switch 用的是 `typ.Underlying()`（`typ` 是未剥壳的 `e.typ`；
            // `underlying` 对匿名类型返回自身）。
            match tt.type_of(underlying(tt, t)) {
                Type::Struct(sid) => {
                    let sdef = tt.struct_def(*sid);
                    for (i, f) in sdef.fields.iter().enumerate() {
                        if field_name(tt, f).as_deref() == Some(name) {
                            let mut idx = e.index.clone();
                            idx.push(i);
                            if obj.is_some() || e.multiples {
                                return Lookup::collision(idx);
                            }
                            index = Some(idx);
                            obj = Some(Found::Field { typ: f.typ });
                            indirect = e.indirect;
                            // 同名冲突已定 / 唯一命中 —— 本字段之后不再收集。
                            continue;
                        }
                        // 嵌入字段：收集到下一深度（仅当还没有命中）。
                        if obj.is_none() && f.embedded {
                            let (ft, fptr) = deref(tt, f.typ);
                            let mut idx = e.index.clone();
                            idx.push(i);
                            next.push(Embedded {
                                typ: ft,
                                index: idx,
                                indirect: e.indirect || fptr,
                                multiples: e.multiples,
                            });
                        }
                    }
                }
                Type::Interface(iid) => {
                    let idef = tt.interface_def(*iid);
                    if let Some((i, (_, sig))) = idef
                        .methods
                        .iter()
                        .enumerate()
                        .find(|(_, (n, _))| n == name)
                    {
                        let mut idx = e.index.clone();
                        idx.push(i);
                        if obj.is_some() || e.multiples {
                            return Lookup::collision(idx);
                        }
                        index = Some(idx);
                        obj = Some(Found::Method { sig: *sig });
                        indirect = e.indirect;
                    }
                }
                _ => {}
            }
        }

        if let Some(o) = &obj {
            // 指针接收者但路径无指针、接收者又不可寻址 ⇒ 「需指针接收者」
            // （官方 `lookup.go:265`；`selector` 据此报两条不同消息）。
            if let Found::Method { sig } = o
                && has_ptr_recv(tt, *sig)
                && !indirect
                && !addressable
            {
                return Lookup::need_ptr_recv();
            }
            return Lookup {
                obj,
                index,
                indirect,
            };
        }

        current = consolidate_multiples(tt, next);
    }

    Lookup::none()
}

/// 一个深度上的条目（官方 `embeddedType`）。
#[derive(Clone, Debug)]
struct Embedded {
    /// 已剥**一层**指针的类型条目。
    typ: TypeId,
    /// 到达它的嵌入字段下标链。
    index: Vec<usize>,
    /// 路径上出现过指针。
    indirect: bool,
    /// 同一深度上该类型出现过多次（`consolidate_multiples` 标记）。
    multiples: bool,
}

/// 官方 `consolidateMultiples`（`lookup.go:283`）：同类型的多个条目合并、
/// 打 `multiples` 标。
fn consolidate_multiples(tt: &TypeTable, list: Vec<Embedded>) -> Vec<Embedded> {
    if list.len() <= 1 {
        return list;
    }
    let mut out: Vec<Embedded> = Vec::with_capacity(list.len());
    for e in list {
        let mut merged = false;
        for o in &mut out {
            let same = o.typ == e.typ || matches!(identical(tt, o.typ, e.typ), Ok(true));
            if same {
                o.multiples = true;
                merged = true;
                break;
            }
        }
        if !merged {
            out.push(e);
        }
    }
    out
}

/// 官方 `deref`（`lookup.go:629`）：透明别名后剥**一层** `*`；**具名**指针
/// 类型不剥（那条由 [`lookup_field_or_method`] 的特例处理）。
fn deref(tt: &TypeTable, t: TypeId) -> (TypeId, bool) {
    let t = unalias(tt, t);
    if let Type::Pointer(e) = tt.type_of(t) {
        (unalias(tt, *e), true)
    } else {
        (t, false)
    }
}

/// 字段的**名字**：命名字段取 `name`；嵌入字段推导（剥壳后的具名名）。
fn field_name(tt: &TypeTable, f: &Field) -> Option<String> {
    if let Some(n) = &f.name {
        return Some(n.clone());
    }
    if !f.embedded {
        return None;
    }
    let (base, _) = deref(tt, f.typ);
    match tt.type_of(unalias(tt, base)) {
        Type::Named(nid) => Some(tt.named_def(*nid).name.clone()),
        _ => None,
    }
}

/// 官方 `Func.hasPtrRecv`：签名有接收者且接收者底层是指针。
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
