//! 统一（M4/Task 13；官方 `go/types/unify.go` 的 `(*unifier).nify` 子集）。
//!
//! # 判据来源
//!
//! * `unify.go:45` 起的常量：`unificationDepthLimit = 50`、
//!   `enableCoreTypeUnification = true`；
//! * `join` / 句柄（`setHandle` / `at` / `set` / `unknowns` / `inferred`）——
//!   `unify.go:186-272`：**已记录**（= 推断变量）的类型参数共享「句柄」，
//!   `join` 在两绑定冲突时失败并回落到**绑定值之间的比较**；
//! * `nify` 主结构（`unify.go:287-809`）：`x == y` 快径 → 深度护栏 →
//!   「具名进 y、已记录形参进 x」交换 → 非精确下剥具名的本体（接口 + 接口
//!   推断开时除外）→ 已记录形参分支（含「具名/通道偏好」）→ 接口推断 →
//!   未记录形参换到 x → `emode` → 分派；
//! * `under.go:79` 的 `commonUnder`：核心类型（`~[]int` 一族；通道方向合并
//!   规则逐条照抄）。
//!
//! # 三模式与官方取值的对应
//!
//! | 本层 | 官方 | 调用点 |
//! |---|---|---|
//! | [`UnifyMode::Loose`] | `unifyMode(0)` | `infer.go:262`（实参统一） |
//! | [`UnifyMode::Exact`] | `exact` | `infer.go:305`（方法签名统一） |
//! | [`UnifyMode::Assign`] | `assign` | `infer.go:180`（赋值位置） |
//!
//! `Assign` 的语义差**只在元素层**：`emode = Assign | Exact`（`unify.go:580`）；
//! 顶层仍走宽容路（所以 `MyT` 对 `int` 在 Assign 下也真——这**不是**笔误，
//! 而是「顶层剥本体、元素收紧」的官方行为）。
//!
//! # 「已记录」与「未记录」
//!
//! 只有 [`Unifier::record`] / [`Unifier::preset`] 登记过的类型参数是**推断
//! 变量**（官方 `newUnifier` 的 `tparams` 实参）。未登记的走到 `TypeParam`
//! 臂 → **核心类型**（`commonUnder`）→ 与 `y` 在 `Assign` 下比较。
//!
//! 自由函数 [`unify`] 是便捷入口：把**两侧出现的全部类型参数**记为推断变量。
//! 需要精确控制登记集（官方 `infer.go` 的用法）时用 [`Unifier`]。
//! 选择动因：计划的签名固定为 `unify(t, mode, x, y, infer)`，没有登记集参数；
//! 「全记」是使 `infer` 回填自洽的最简语义（记在台账 Task 13 行）。
//!
//! # 与官方的偏离（都在护栏/表示层，不在判定语义）
//!
//! 1. **深度护栏**：官方越限 panic（`panicAtUnificationDepthLimit = true`），
//!    本层**安静返回假** —— 发行库不 panic；该形态本该不可达（实现护栏）。
//! 2. **`union` 直达统一**：官方 `default:` panic（对合法程序不可达——union
//!    只活在约束接口里）；本层返回假。
//! 3. **接口 `ifacePair` 栈**：本层表**结构上不可能**成环（匿名接口自引用
//!    需要先有自身条目；经 `named` 的环在 `Named` 臂停步——与
//!    [`crate::identical`] 的同一条论证）。栈照官方保留，纯防御。
//! 4. **方法 Id 无包名**：官方 `Func.Id()` 对非导出名带包前缀；本层按
//!    **名字**比（v1 已知差异，`Ruling 8-4` 同族）。
//! 5. **泛型方法不满接口**：官方在一接口面的 `LookupFieldOrMethod` 里按
//!    `Signature().TypeParams() != nil` 拒绝；本层签名不携带方法级类型参数
//!    （M3 划界），无从判起——登记为 v1 能力边界。
//!
//! # `# Errors`
//!
//! `Err` 只透传自 `identical` / `subset_of` / `methodset` 的能力边界；
//! M4/Task 10 之后这些面实际不再产出（`Result` 形态为调用面稳定保留）。

use std::collections::HashMap;

use rgoc_lex::diag::DiagCode;

use crate::identical::{array_len_same, identical, unalias, underlying};
use crate::methodset::methodset;
use crate::table::TypeTable;
use crate::typ::{ChanDir, InterfaceId, NamedId, Type, TypeId, TypeParamId};
use crate::typeset::{IfaceTSet, subset_of};

/// 官方 `unificationDepthLimit`（`unify.go:48`）。
const UNIFICATION_DEPTH_LIMIT: u32 = 50;

/// 统一模式（官方 `unifyMode` 的三个实到取值；见模块文档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnifyMode {
    /// 官方 `unifyMode(0)`：具名可对本体的中间形态、通道方向忽略、接口推断开。
    Loose,
    /// 官方 `exact`：逐点同一性语义（不下钻具名、方向必须相同）。
    Exact,
    /// 官方 `assign`：顶层宽容、**元素层收紧为 `Exact`**。
    Assign,
}

/// 统一器（官方 `unifier` 的句柄机制 + `nify` 递归）。
///
/// 生命周期借 `&TypeTable`：判定全程**只读**（不建新条目）。
pub struct Unifier<'a> {
    tt: &'a TypeTable,
    /// 官方 `enableInterfaceInference`（`infer.go` 按语言版本 ≥1.21 传入；
    /// 本层默认开，测试可关）。
    iface_inference: bool,
    /// 类型参数 → 句柄号。
    handles: HashMap<TypeParamId, usize>,
    /// 句柄号 → 绑定值（`None` = 未知）。
    vals: Vec<Option<TypeId>>,
    /// 递归深度（官方 `u.depth`）。
    depth: u32,
}

impl std::fmt::Debug for Unifier<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Unifier")
            .field("recorded", &self.handles.len())
            .field("unknowns", &self.unknowns())
            .field("depth", &self.depth)
            .finish_non_exhaustive()
    }
}

impl<'a> Unifier<'a> {
    /// 新统一器：无已记录形参、接口推断开。
    #[must_use]
    pub fn new(tt: &'a TypeTable) -> Self {
        Self {
            tt,
            iface_inference: true,
            handles: HashMap::new(),
            vals: Vec::new(),
            depth: 0,
        }
    }

    /// 开关接口推断（官方 `newUnifier` 的第 4 个实参）。
    pub fn set_interface_inference(&mut self, on: bool) {
        self.iface_inference = on;
    }

    /// 登记**推断变量**（官方 `newUnifier(check, tparams, ...)` 的 `tparams`）。
    pub fn record(&mut self, tps: impl IntoIterator<Item = TypeParamId>) {
        for tp in tps {
            if !self.handles.contains_key(&tp) {
                self.vals.push(None);
                self.handles.insert(tp, self.vals.len() - 1);
            }
        }
    }

    /// 记录并**预置**绑定（官方 `newUnifier` 由显式实参给出的 `targs` 初值）。
    pub fn preset(&mut self, tp: TypeParamId, ty: TypeId) {
        self.record([tp]);
        self.set(tp, ty);
    }

    /// 形参的当前绑定（未登记 / 未知 ⇒ `None`）。
    #[must_use]
    pub fn at(&self, tp: TypeParamId) -> Option<TypeId> {
        self.handles.get(&tp).and_then(|h| self.vals[*h])
    }

    /// 仍未绑定的形参个数（官方 `unknowns`）。
    #[must_use]
    pub fn unknowns(&self) -> usize {
        self.handles
            .values()
            .filter(|h| self.vals[**h].is_none())
            .count()
    }

    /// 按给定形参表取绑定（官方 `inferred`；与输入同序、未知为 `None`）。
    #[must_use]
    pub fn inferred(&self, tps: &[TypeParamId]) -> Vec<Option<TypeId>> {
        tps.iter().map(|tp| self.at(*tp)).collect()
    }

    /// 统一 `x` 与 `y`（官方 `(*unifier).unify`）。
    ///
    /// # Errors
    ///
    /// 见模块文档的 `# Errors`。
    pub fn unify(&mut self, mode: UnifyMode, x: TypeId, y: TypeId) -> Result<bool, DiagCode> {
        let mut p: Vec<(InterfaceId, InterfaceId)> = Vec::new();
        self.nify(x, y, mode, &mut p)
    }

    // ── 句柄机制（官方 join / setHandle / at / set）────────────────

    pub(crate) fn set(&mut self, tp: TypeParamId, t: TypeId) {
        let h = self.handles[&tp];
        self.vals[h] = Some(t);
    }

    /// 官方 `setHandle`：把 `x` 所在句柄组的**全部**形参改挂到 `h`。
    fn set_handle(&mut self, x: TypeParamId, h: usize) {
        let hx = self.handles[&x];
        let keys: Vec<TypeParamId> = self
            .handles
            .iter()
            .filter(|(_, v)| **v == hx)
            .map(|(k, _)| *k)
            .collect();
        for k in keys {
            self.handles.insert(k, h);
        }
    }

    /// 官方 `join`（`unify.go:189`）：两形参合并句柄；都已有绑定 ⇒ 失败。
    fn join(&mut self, x: TypeParamId, y: TypeParamId) -> bool {
        let (hx, hy) = (self.handles[&x], self.handles[&y]);
        if hx == hy {
            return true;
        }
        if self.vals[hx].is_some() && self.vals[hy].is_some() {
            return false;
        }
        if self.vals[hx].is_some() {
            // 只有 x 有绑定：y 组挂到 x（官方 `setHandle(y, hx)`）
            self.set_handle(y, hx);
        } else {
            // 其余（含「只有 y 有绑定」）：x 组挂到 y
            self.set_handle(x, hy);
        }
        true
    }

    /// 官方 `asBoundTypeParam`：是不是**已记录**的类型参数（剥别名后）。
    fn as_bound_type_param(&self, t: TypeId) -> Option<TypeParamId> {
        let u = unalias(self.tt, t);
        if let Type::TypeParam(tp) = self.tt.type_of(u)
            && self.handles.contains_key(tp)
        {
            return Some(*tp);
        }
        None
    }

    // ── nify ────────────────────────────────────────────────────────

    fn nify(
        &mut self,
        x: TypeId,
        y: TypeId,
        mode: UnifyMode,
        p: &mut Vec<(InterfaceId, InterfaceId)>,
    ) -> Result<bool, DiagCode> {
        self.depth += 1;
        let r = self.nify_inner(x, y, mode, p);
        self.depth -= 1;
        r
    }

    fn nify_inner(
        &mut self,
        mut x: TypeId,
        mut y: TypeId,
        mode: UnifyMode,
        p: &mut Vec<(InterfaceId, InterfaceId)>,
    ) -> Result<bool, DiagCode> {
        let tt = self.tt;

        // ① 同一条目（或剥别名后同条目）⇒ 真（官方 300 行）。
        if x == y || unalias(tt, x) == unalias(tt, y) {
            return Ok(true);
        }
        // 深度护栏（官方 305 行；此处是 panic 的安静化 —— 见模块文档）。
        if self.depth > UNIFICATION_DEPTH_LIMIT {
            return Ok(false);
        }

        // ② 交换：具名进 y、已记录形参进 x（官方 319 行）。
        if as_named(tt, x).is_some() || self.as_bound_type_param(y).is_some() {
            std::mem::swap(&mut x, &mut y);
        }

        // ③ 非精确模式 + y 是具名 + x 是字面型（且非「接口 + 接口推断开」）：
        //    换成 y 的本体继续（官方 343 行 —— 顶层的「具名可对本体」就靠这条）。
        if mode != UnifyMode::Exact
            && let Some(ny) = as_named(tt, y)
            && is_type_lit(tt, x)
            && !(self.iface_inference && is_interface(tt, x))
        {
            y = tt.named_underlying(ny);
            if x == y || unalias(tt, x) == unalias(tt, y) {
                return Ok(true);
            }
        }

        // ④ 已记录形参的分支（官方 363-451 行）。
        match (self.as_bound_type_param(x), self.as_bound_type_param(y)) {
            (Some(a), Some(b)) => {
                if self.join(a, b) {
                    return Ok(true);
                }
                // join 失败（两者都有绑定）⇒ 比较绑定值
                let (va, vb) = (
                    self.at(a).expect("join 失败 ⇒ a 必有绑定"),
                    self.at(b).expect("join 失败 ⇒ b 必有绑定"),
                );
                return self.nify(va, vb, mode, p);
            }
            (Some(a), None) => {
                let Some(vx) = self.at(a) else {
                    // 未绑定 ⇒ 直接记为 y（官方 449 行）
                    self.set(a, y);
                    return Ok(true);
                };
                // 已有绑定 ⇒ 绑定值必须与 y 统一（官方 374 行起）
                if !self.nify(vx, y, mode, p)? {
                    return Ok(false);
                }
                let xi = as_interface(tt, vx);
                let yi = as_interface(tt, y);
                let xn = as_named(tt, vx).is_some();
                let yn = as_named(tt, y).is_some();
                if let (Some(ix), Some(iy)) = (xi, yi) {
                    // 两侧都是接口：都具名 ⇒ 必须同一；否则方法数须相等
                    // （官方 384-401 行）
                    if xn && yn {
                        return identical(tt, vx, y);
                    }
                    if tt.interface_def(ix).methods.len() != tt.interface_def(iy).methods.len() {
                        return Ok(false);
                    }
                } else if xi.is_some() || yi.is_some() {
                    // 恰一侧是接口 ⇒ 无法二选 ⇒ 假（官方 402-408，issue 60933）
                    return Ok(false);
                }
                if mode != UnifyMode::Exact {
                    // 非精确：具名优先（什么都不做）；否则定向通道优先
                    // （官方 409-443 行）
                    if xn {
                        // 保持既有绑定（具名）
                    } else if yn {
                        self.set(a, y);
                    } else if let Type::Chan { dir, .. } = tt.type_of(underlying(tt, y))
                        && *dir != ChanDir::SendRecv
                    {
                        self.set(a, y);
                    }
                }
                return Ok(true);
            }
            (None, Some(_)) => unreachable!("交换不变量：已记录形参必在 x 侧"),
            (None, None) => {
                // ⑤ 接口推断（官方 461 行；非精确 + 开关开）
                if self.iface_inference && mode != UnifyMode::Exact {
                    let xi = as_interface(tt, x);
                    let yi = as_interface(tt, y);
                    if let (Some(ix), Some(iy)) = (xi, yi) {
                        let xs = tt.interface_def(ix).tset.clone();
                        let ys = tt.interface_def(iy).tset.clone();
                        // comparable 位 + 项集相等（官方 471-478 行）
                        if matches!(xs, IfaceTSet::Comparable)
                            != matches!(ys, IfaceTSet::Comparable)
                        {
                            return Ok(false);
                        }
                        if !subset_of(tt, &xs, &ys)? || !subset_of(tt, &ys, &xs)? {
                            return Ok(false);
                        }
                        if iface_pair_seen(p, ix, iy) {
                            return Ok(true); // 该对比较过 ⇒ 视为相等（官方 ifacePair）
                        }
                        p.push((ix, iy));
                        let mut res = Ok(true);
                        // 小方法集必须是子集；公共方法签名在 **Exact** 下统一
                        // （官方 508-528 行）
                        let xm = tt.interface_def(ix).methods.clone();
                        let ym = tt.interface_def(iy).methods.clone();
                        let (small, big) = if xm.len() <= ym.len() {
                            (&xm, &ym)
                        } else {
                            (&ym, &xm)
                        };
                        let big_map: HashMap<&str, TypeId> =
                            big.iter().map(|(n, s)| (n.as_str(), *s)).collect();
                        for (name, s1) in small {
                            let Some(s2) = big_map.get(name.as_str()) else {
                                res = Ok(false);
                                break;
                            };
                            match self.nify(*s1, *s2, UnifyMode::Exact, p) {
                                Ok(true) => {}
                                Ok(false) => {
                                    res = Ok(false);
                                    break;
                                }
                                Err(e) => {
                                    res = Err(e);
                                    break;
                                }
                            }
                        }
                        p.pop();
                        return res;
                    }
                    if xi.is_some() || yi.is_some() {
                        // 恰一个接口：其方法必须都能在另一侧的**方法集**里
                        // 找到，签名在 Exact 下统一（官方 531-557 行）
                        let ix = xi.or(yi).expect("上面已排除两侧皆非接口");
                        let other = if xi.is_some() { y } else { x };
                        let ms = tt.interface_def(ix).methods.clone();
                        if !ms.is_empty() {
                            let oset = methodset(tt, other)?;
                            for (name, s1) in &ms {
                                let Some(e) = oset.iter().find(|e| e.name == *name) else {
                                    return Ok(false);
                                };
                                if !self.nify(*s1, e.sig, UnifyMode::Exact, p)? {
                                    return Ok(false);
                                }
                            }
                        }
                        return Ok(true);
                    }
                }
            }
        }

        // ⑥ 未记录形参也换到 x（官方 571 行 —— 早先的交换只管已记录的）。
        if is_type_param(tt, y) {
            std::mem::swap(&mut x, &mut y);
        }
        // ⑦ 元素型模式（官方 580 行）：Assign 的元素层收紧为 Exact。
        let emode = if mode == UnifyMode::Assign {
            UnifyMode::Exact
        } else {
            mode
        };
        let yorig = y;

        // ⑧ 剥别名后分派（官方 586-809 行）。
        let xu = unalias(tt, x);
        let yu = unalias(tt, y);
        match *tt.type_of(xu) {
            // `invalid`：官方是 `*Basic{kind: Invalid}` ⇒ 按「同类」判
            // （与 `identical` 的宽容面一致）。
            Type::Invalid => Ok(matches!(tt.type_of(yu), Type::Invalid)),
            Type::Basic(k) => Ok(matches!(tt.type_of(yu), Type::Basic(k2) if *k2 == k)),
            Type::Pointer(ex) => match tt.type_of(yu) {
                Type::Pointer(ey) => self.nify(ex, *ey, emode, p),
                _ => Ok(false),
            },
            Type::Slice(ex) => match tt.type_of(yu) {
                Type::Slice(ey) => self.nify(ex, *ey, emode, p),
                _ => Ok(false),
            },
            Type::Array { len: lx, elem: ex } => match tt.type_of(yu) {
                Type::Array { len: ly, elem: ey } => {
                    // 长度未定（`None` / 未知常量）视为同 —— 与 `identical` 共口径
                    if !array_len_same(tt, lx, *ly) {
                        return Ok(false);
                    }
                    self.nify(ex, *ey, emode, p)
                }
                _ => Ok(false),
            },
            Type::Struct(sx) => match tt.type_of(yu) {
                Type::Struct(sy) => {
                    let a = tt.struct_def(sx);
                    let b = tt.struct_def(*sy);
                    if a.fields.len() != b.fields.len() {
                        return Ok(false);
                    }
                    // 嵌入标记 / 标签 / 名字 / 类型（官方 613-631 行 ——
                    // 与 `identical` 的严格面同判，tag **恒**参与）
                    for (f, g) in a.fields.iter().zip(&b.fields) {
                        if f.embedded != g.embedded
                            || f.tag != g.tag
                            || f.name != g.name
                            || !self.nify(f.typ, g.typ, emode, p)?
                        {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                _ => Ok(false),
            },
            Type::Tuple(tx) => match tt.type_of(yu) {
                Type::Tuple(ty) => {
                    let a = tt.tuple_def(tx);
                    let b = tt.tuple_def(*ty);
                    if a.vars.len() != b.vars.len() {
                        return Ok(false);
                    }
                    // 名字不参与；元素按 **mode**（非 emode —— 官方 647 行）
                    for ((_, px), (_, py)) in a.vars.iter().zip(&b.vars) {
                        if !self.nify(*px, *py, mode, p)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                _ => Ok(false),
            },
            Type::Signature(sx) => match tt.type_of(yu) {
                Type::Signature(sy) => {
                    let a = tt.signature_def(sx);
                    let b = tt.signature_def(*sy);
                    // 接收者 / 签名级类型参数不参与（官方 656-666 行同）
                    let (va, ap, ar) = (a.variadic, a.params, a.results);
                    let (vb, bp, br) = (b.variadic, b.params, b.results);
                    Ok(va == vb && self.nify(ap, bp, emode, p)? && self.nify(ar, br, emode, p)?)
                }
                _ => Ok(false),
            },
            Type::Interface(ix) => match tt.type_of(yu) {
                Type::Interface(iy) => {
                    let iy = *iy;
                    let xs = tt.interface_def(ix).tset.clone();
                    let ys = tt.interface_def(iy).tset.clone();
                    if matches!(xs, IfaceTSet::Comparable) != matches!(ys, IfaceTSet::Comparable) {
                        return Ok(false);
                    }
                    if !subset_of(tt, &xs, &ys)? || !subset_of(tt, &ys, &xs)? {
                        return Ok(false);
                    }
                    let xm = tt.interface_def(ix).methods.clone();
                    let ym = tt.interface_def(iy).methods.clone();
                    if xm.len() != ym.len() {
                        return Ok(false);
                    }
                    if iface_pair_seen(p, ix, iy) {
                        return Ok(true);
                    }
                    p.push((ix, iy));
                    let mut res = Ok(true);
                    // 按名排序后逐位比（装载期已排；手工表未必 —— 官方恒假定排好）
                    let mut a2: Vec<(&str, TypeId)> =
                        xm.iter().map(|(n, s)| (n.as_str(), *s)).collect();
                    let mut b2: Vec<(&str, TypeId)> =
                        ym.iter().map(|(n, s)| (n.as_str(), *s)).collect();
                    a2.sort_unstable_by_key(|(n, _)| *n);
                    b2.sort_unstable_by_key(|(n, _)| *n);
                    for ((n1, s1), (n2, s2)) in a2.iter().zip(&b2) {
                        if n1 != n2 {
                            res = Ok(false);
                            break;
                        }
                        match self.nify(*s1, *s2, UnifyMode::Exact, p) {
                            Ok(true) => {}
                            Ok(false) => {
                                res = Ok(false);
                                break;
                            }
                            Err(e) => {
                                res = Err(e);
                                break;
                            }
                        }
                    }
                    p.pop();
                    res
                }
                _ => Ok(false),
            },
            Type::Map { key: kx, elem: ex } => match tt.type_of(yu) {
                Type::Map { key: ky, elem: ey } => {
                    Ok(self.nify(kx, *ky, emode, p)? && self.nify(ex, *ey, emode, p)?)
                }
                _ => Ok(false),
            },
            Type::Chan { dir: dx, elem: ex } => match tt.type_of(yu) {
                Type::Chan { dir: dy, elem: ey } => {
                    // 方向只在 Exact 参与（官方 736-742 行）
                    Ok((mode != UnifyMode::Exact || dx == *dy) && self.nify(ex, *ey, emode, p)?)
                }
                _ => Ok(false),
            },
            Type::Named(nx) => {
                let Some(ny) = as_named(tt, yu) else {
                    return Ok(false);
                };
                let ax = tt.named_def(nx).targs.clone();
                let ay = tt.named_def(ny).targs.clone();
                // 实参个数 + 逐对统一（按 **mode** —— 官方 744-762 行）；
                // 原点（同一声明）最后比 —— 官方注释：先比实参，报错信息更好
                if ax.len() != ay.len() {
                    return Ok(false);
                }
                for (ta, tb) in ax.iter().zip(&ay) {
                    if !self.nify(*ta, *tb, mode, p)? {
                        return Ok(false);
                    }
                }
                Ok(origin_of(tt, nx) == origin_of(tt, ny))
            }
            Type::TypeParam(_) => {
                // x 必是**未记录**形参（已记录的在上面的分支消化了）
                if let Some(cx) = common_under(tt, xu)? {
                    // 核心类型是本体层 ⇒ 与具名 y 的匹配走 Assign（官方 796 行）
                    return self.nify(cx, yorig, UnifyMode::Assign, p);
                }
                // 无核心类型 ⇒ 无从匹配（官方 799 行）
                Ok(false)
            }
            // 官方 `default:` 会 panic：union 直达窗口对合法程序不可达
            // （union 只活在约束接口里）—— 本层安静返回假（见模块文档）。
            Type::Union(_) => Ok(false),
            Type::Alias(_) => unreachable!("unalias 之后不该还有 alias"),
        }
    }
}

/// 自由函数便捷入口（见模块文档「已记录与未记录」）。
///
/// 成功（`Ok(true)`）时把绑定追加进 `infer`（按两侧类型参数的**首遇序**；
/// 未绑定者不出现）；`Ok(false)` / `Err` 时 `infer` 不动。
///
/// # Errors
///
/// 见模块文档的 `# Errors`。
pub fn unify(
    tt: &TypeTable,
    mode: UnifyMode,
    x: TypeId,
    y: TypeId,
    infer: &mut Vec<(TypeParamId, TypeId)>,
) -> Result<bool, DiagCode> {
    let mut u = Unifier::new(tt);
    let mut tps: Vec<TypeParamId> = Vec::new();
    collect_type_params(tt, x, &mut tps);
    collect_type_params(tt, y, &mut tps);
    u.record(tps.iter().copied());
    let ok = u.unify(mode, x, y)?;
    if ok {
        for tp in tps {
            if let Some(t) = u.at(tp) {
                infer.push((tp, t));
            }
        }
    }
    Ok(ok)
}

// ── 辅助 ─────────────────────────────────────────────────────────────

/// 收集两侧出现的类型参数（首遇序、去重）。
///
/// **不下钻** `named`/`alias` 的本体（与判定面同一边界）；`named` 的
/// **实例实参**要下钻（`Pair[T, string]` 里的 `T` 是推断变量）。
fn collect_type_params(tt: &TypeTable, t: TypeId, out: &mut Vec<TypeParamId>) {
    let t = unalias(tt, t);
    match *tt.type_of(t) {
        Type::TypeParam(tp) => {
            if !out.contains(&tp) {
                out.push(tp);
            }
        }
        Type::Named(n) => {
            for a in &tt.named_def(n).targs {
                collect_type_params(tt, *a, out);
            }
        }
        Type::Pointer(e) | Type::Slice(e) => collect_type_params(tt, e, out),
        Type::Array { elem, .. } => collect_type_params(tt, elem, out),
        Type::Map { key, elem } => {
            collect_type_params(tt, key, out);
            collect_type_params(tt, elem, out);
        }
        Type::Chan { elem, .. } => collect_type_params(tt, elem, out),
        Type::Struct(s) => {
            for f in &tt.struct_def(s).fields {
                collect_type_params(tt, f.typ, out);
            }
        }
        Type::Tuple(tu) => {
            for (_, v) in &tt.tuple_def(tu).vars {
                collect_type_params(tt, *v, out);
            }
        }
        Type::Signature(s) => {
            let d = tt.signature_def(s);
            collect_type_params(tt, d.params, out);
            collect_type_params(tt, d.results, out);
        }
        Type::Interface(i) => {
            let d = tt.interface_def(i);
            for (_, sg) in &d.methods {
                collect_type_params(tt, *sg, out);
            }
            if let Some(terms) = d.terms {
                collect_type_params(tt, terms, out);
            }
            if let IfaceTSet::Terms(ts) = &d.tset {
                for (_, x) in ts {
                    collect_type_params(tt, *x, out);
                }
            }
        }
        Type::Union(u) => {
            for (_, x) in &tt.union_def(u).terms {
                collect_type_params(tt, *x, out);
            }
        }
        _ => {}
    }
}

/// 剥别名后的 `Type::Named` 条目（官方 `asNamed`）。
fn as_named(tt: &TypeTable, t: TypeId) -> Option<NamedId> {
    if let Type::Named(n) = tt.type_of(unalias(tt, t)) {
        Some(*n)
    } else {
        None
    }
}

/// 官方 `isTypeParam`。
fn is_type_param(tt: &TypeTable, t: TypeId) -> bool {
    matches!(tt.type_of(unalias(tt, t)), Type::TypeParam(_))
}

/// 官方 `isTypeLit`（`predicates.go:78`）。
fn is_type_lit(tt: &TypeTable, t: TypeId) -> bool {
    !matches!(
        tt.type_of(unalias(tt, t)),
        Type::Named(_) | Type::TypeParam(_)
    )
}

/// 官方 `IsInterface`：本体是接口（**含**形参——其本体是约束）。
fn is_interface(tt: &TypeTable, t: TypeId) -> bool {
    matches!(tt.type_of(underlying(tt, t)), Type::Interface(_))
}

/// 官方 `asInterface`：剥别名、**排除形参**后取本体接口。
fn as_interface(tt: &TypeTable, t: TypeId) -> Option<InterfaceId> {
    let u = unalias(tt, t);
    if matches!(tt.type_of(u), Type::TypeParam(_)) {
        return None;
    }
    if let Type::Interface(i) = tt.type_of(underlying(tt, u)) {
        Some(*i)
    } else {
        None
    }
}

/// 实例的**原点**；非实例即自身（官方 `Named.Origin` + `identicalOrigin`）。
fn origin_of(tt: &TypeTable, n: NamedId) -> NamedId {
    tt.named_def(n).origin.unwrap_or(n)
}

/// 官方 `ifacePair.identical`（**对称**比对）。
fn iface_pair_seen(p: &[(InterfaceId, InterfaceId)], x: InterfaceId, y: InterfaceId) -> bool {
    p.iter()
        .any(|(a, b)| (*a == x && *b == y) || (*a == y && *b == x))
}

/// 官方 `commonUnder(t, nil)`（`under.go:79`）：核心类型（含通道合并）。
///
/// 返回 `Ok(None)` ⇔ 官方里的「无具体类型 / 无共同本体 / 冲突」三态合一
/// （调用面 `cx != nil` 只关心有与没有）。
pub(crate) fn common_under(tt: &TypeTable, t: TypeId) -> Result<Option<TypeId>, DiagCode> {
    let t = unalias(tt, t);
    let Type::TypeParam(tp) = tt.type_of(t) else {
        // 非形参：视作单项集 ⇒ 本体（官方 77-78 行）
        return Ok(Some(underlying(tt, t)));
    };
    let constraint = tt.type_param_def(*tp).constraint;
    // 约束的类型集项面；非接口约束（非法源）⇒ 无核心类型
    let Some(IfaceTSet::Terms(terms)) =
        (if let Type::Interface(i) = tt.type_of(underlying(tt, constraint)) {
            Some(tt.interface_def(*i).tset.clone())
        } else {
            None
        })
    else {
        // 𝓤 / ∅：无具体项 ⇒ 无核心类型（官方「no specific type」）
        return Ok(None);
    };
    let mut cu: Option<TypeId> = None;
    for (_tilde, term) in &terms {
        let u = underlying(tt, *term);
        match cu {
            None => cu = Some(u),
            Some(prev) => {
                // 通道对通道：元素必须同一、方向合并且取受限者
                // （官方 under.go:98-116）
                if let Type::Chan { dir: pd, elem: pe } = tt.type_of(prev)
                    && let Type::Chan { dir: ud, elem: ue } = tt.type_of(u)
                {
                    if !identical(tt, *pe, *ue)? {
                        return Ok(None);
                    }
                    if pd == ud {
                        continue;
                    }
                    if *pd == ChanDir::SendRecv {
                        cu = Some(u);
                        continue;
                    }
                    if *ud != ChanDir::SendRecv {
                        return Ok(None);
                    }
                    continue;
                }
                // 否则本体必须同一（官方 under.go:119-121）
                if !identical(tt, prev, u)? {
                    return Ok(None);
                }
            }
        }
    }
    Ok(cu)
}
