//! verifier：九条不变式（`M6-design.md` §3.5 / `M6-tests.md` §3.1）。
//!
//! # 跑法与顺序
//!
//! 按 `V1 → V9` 顺序跑，**第一个失败即返回**。顺序固定是为了让断言能锚定：
//! 一张同时违反 `V1` 与 `V4` 的图必须**只**报 `V1`，否则同一张图在不同实现下
//! 报不同码，反例测试就退化成「反正它是红的」。
//!
//! # 当前实现范围（Task 13）
//!
//! `V1`（每值恰好定义一次）/ `V2`（定义支配使用）/ `V3`（类型自洽）/
//! `V4`（前驱后继互逆、入口无前驱）/ `V5`（调用与签名一致）/ `V6`（mem 链）/
//! `V7`（块参数计数）/ `V8`（可达性）/ `V9`（取址即建根）。**九条全在**（Task 14）。
//!
//! # 九条之外**没有**查的：调用与 mem 链的关系
//!
//! `Instr::Call` 没有 mem 操作数、也不产出 mem。所以「一个写 `writes` 里那个根的
//! 调用，和它前后的 load/store 之间怎么排序」在 IR 里**没有结构性的位置**。
//! M6 的口径是：`V6` 只管**本函数自己的 load/store 链**；跨调用的顺序由 `effect`
//! 摘要约束，那是 **pass 侧**的义务（M6d），不是 verifier 能查的不变式。
//! 这条不自欺 —— 它记在 `G-M6-01`（收口时登记），并有 `T-SSA-36` 的语料做行为侧证据。
//!
//! # 为什么要 `&TypeTable`
//!
//! `SsaFunc` 只存 `TypeId`；`V3` 要判「`i64 + string`」这种，必须能解析出真实类型。
//! 类型表**不**放进 `SsaFunc`（一个函数持有一张表是荒唐的：表是包级的）。
//! 所以签名是 `verify(f, tt)` —— 这与 `M6-design.md` §3.5 原稿的 `verify(f)`
//! 差一个参数，已在 `M6-tests.md` 的修订行 `R-12-1` 登记。
//!
//! # 为什么 `V4` 自己从 `Term` 重推一遍边
//!
//! `cfg::Cfg` 的 `preds/succs` 也是从 `Term` 推的，但它按 `(from,to)` **去重**
//! （一张图里同一对块可能有两个不同实参的边）。`V4` 要查的正是「同一目标两条边、
//! 实参还不同」这种 IR 表达不了的情形，所以它读 `Term` 原文；两条路径互相校验，
//! 顺带把「CFG 缓存与 IR 不一致」也变成可查项。
//!
//! # 位置口径
//!
//! `VerifyError::idx` 是**块内位置序号**：先块参数（`0..params.len()`），
//! 再指令（`params.len()..`）。没有哨兵值，也不需要知道「这是参数还是指令」。

use std::collections::BTreeMap;
use std::fmt;

use crate::cfg::Cfg;
use crate::dom::DomTree;
use crate::ir::{
    BlockId, CalleeRef, Instr, Root, RootId, SsaFunc, Term, ValueId, kind_of_instr, kind_of_root,
    kind_of_term,
};
use rgoc_types::TypeTable;

/// 九条不变式的编号（顺序即执行顺序；进 dump 与断言，不许重排）。
pub const INVARIANTS: [&str; 9] = ["V1", "V2", "V3", "V4", "V5", "V6", "V7", "V8", "V9"];

/// 一次验证失败（**开发者面**：不进 `DiagCode`，见 `D-M6-14`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyError {
    /// 违反的不变式（`"V1".."V9"`）。
    pub invariant: &'static str,
    /// 出问题的块。
    pub block: BlockId,
    /// 块内位置（先块参数，后指令）。
    pub idx: usize,
    /// 人读说明（必须带具体编号，否则断言只能看到不变式名）。
    pub detail: String,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at b{}[{}]：{}",
            self.invariant, self.block.0, self.idx, self.detail
        )
    }
}

impl std::error::Error for VerifyError {}

fn err(
    invariant: &'static str,
    block: BlockId,
    idx: usize,
    detail: impl Into<String>,
) -> VerifyError {
    VerifyError {
        invariant,
        block,
        idx,
        detail: detail.into(),
    }
}

/// 验证一个函数。
///
/// # Errors
///
/// 任一条已实现的不变式被违反（顺序见模块注释）。
pub fn verify(f: &SsaFunc, tt: &TypeTable) -> Result<(), VerifyError> {
    let cfg = Cfg::build(f);
    let def = check_v1(f)?;
    let dom = DomTree::build(f, &cfg);
    check_v2(f, &cfg, &dom, &def)?;
    check_v3(f, &cfg, tt)?;
    check_v4(f, &cfg)?;
    check_v5(f)?;
    check_v6(f, &dom, &def)?;
    check_v7(f)?;
    check_v8(f, &cfg)?;
    check_v9(f)?;
    Ok(())
}

/// 一条指令定义的全部值（**不带通配的 `match`**：加变体编译就报错）。
fn dsts_of(i: &Instr) -> Vec<ValueId> {
    match i {
        Instr::Const { dst, .. }
        | Instr::Copy { dst, .. }
        | Instr::Bin { dst, .. }
        | Instr::Un { dst, .. }
        | Instr::Convert { dst, .. }
        | Instr::AddrOf { dst, .. }
        | Instr::Field { dst, .. }
        | Instr::Index { dst, .. }
        | Instr::Slice { dst, .. }
        | Instr::Composite { dst, .. }
        | Instr::Make { dst, .. }
        | Instr::Load { dst, .. }
        | Instr::Store { dst, .. } => vec![*dst],
        Instr::Call { dsts, .. } => dsts.clone(),
        Instr::FuncAddr { dst, .. } | Instr::CapturePtr { dst, .. } => vec![*dst],
        Instr::Defer { .. } => Vec::new(),
        Instr::NilCheck { .. } => Vec::new(),
        // 捕获是**操作数**不是定义（它们在 `uses` 里）
        Instr::MakeClosure { dst, .. } => vec![*dst],
        Instr::TypeAssert { dst, ok, .. } => {
            let mut v = vec![*dst];
            v.extend(ok.iter().copied());
            v
        }
    }
}

/// 终止符上的出边（按源码顺序；**不去重**，`V4` 要的就是重复）。
fn term_edges(t: &Term) -> Vec<(BlockId, &[ValueId])> {
    match t {
        Term::Goto { to, args } => vec![(*to, args.as_slice())],
        Term::If { then_, else_, .. } => {
            vec![(then_.0, then_.1.as_slice()), (else_.0, else_.1.as_slice())]
        }
        Term::Return(_) | Term::Panic(_) | Term::Unreachable => Vec::new(),
    }
}

/// `V1`：每个值**恰好**定义一次。
///
/// 定义点只有两处：块参数、指令的结果。三者都要报：重复定义、越界、从未定义。
/// 返回 `ValueId → (块, 块内位置)`，供 `V2` 直接用 —— 定义表只算一次，
/// 免得 `V1` 与 `V2` 对「定义点」有两套口径。
/// 「从未定义」不归 `V2` 管 —— 支配检查只能看到**存在**的定义点，看不见
/// 「这个值根本没有定义点」，于是 `v9` 会一路滑到求值期变成「取值失败」。
fn check_v1(f: &SsaFunc) -> Result<Vec<(BlockId, usize)>, VerifyError> {
    let n = f.value_count();
    let mut def: Vec<Option<(BlockId, usize)>> = vec![None; n];

    for (bi, blk) in f.blocks.iter().enumerate() {
        let b = BlockId(bi as u32);
        for (k, &p) in blk.params.iter().enumerate() {
            define(&mut def, n, b, k, p, "块参数")?;
        }
        let base = blk.params.len();
        for (ii, ins) in blk.instrs.iter().enumerate() {
            let idx = base + ii;
            for d in dsts_of(ins) {
                define(&mut def, n, b, idx, d, "指令结果")?;
            }
        }
    }

    let mut out: Vec<(BlockId, usize)> = Vec::with_capacity(n);
    for (v, site) in def.iter().enumerate() {
        match site {
            Some(s) => out.push(*s),
            None => {
                return Err(err(
                    "V1",
                    f.entry,
                    0,
                    format!("v{v} 从未被定义（块参数与指令结果里都没有它）"),
                ));
            }
        }
    }
    Ok(out)
}

/// 登记一次定义（`V1` 的公共路径）。
fn define(
    def: &mut [Option<(BlockId, usize)>],
    n: usize,
    b: BlockId,
    idx: usize,
    v: ValueId,
    what: &str,
) -> Result<(), VerifyError> {
    if v.index() >= n {
        return Err(err(
            "V1",
            b,
            idx,
            format!("v{} 越界：本函数只有 {n} 个值（下标 0..{n}）", v.0),
        ));
    }
    if let Some((pb, pidx)) = def[v.index()] {
        return Err(err(
            "V1",
            b,
            idx,
            format!(
                "v{} 被定义了两次：先 b{}[{pidx}]，再此处（{what}）",
                v.0, pb.0
            ),
        ));
    }
    def[v.index()] = Some((b, idx));
    Ok(())
}

/// 一条指令用到的**普通值**（**不带通配的 `match`**）。
///
/// 口径：`Call` 的接收者（`Method.recv` / `Indirect`）**算使用** —— 它们是值；
/// `Func` / `Builtin` 不是值，不算。
///
/// # `mem` 为什么**不**在这里
///
/// `mem` 是**线性令牌**，不是普通值：它的正确性判据是「同根串行 + 支配」
/// （`V6`），不是「被定义支配」这一条。若把它算进 `V2` 的使用集，`V2` 会先报，
/// 而 `V6` 的 ① 永远不可达 —— 一条**永远不可能失败**的断言等于没有。
/// （这类「两条不变式覆盖同一件事 ⇒ 一条永远绿」的坑，M4 收口时踩过：
/// 当时那条断言与另一条逻辑等价，合成反例才抓住。）
///
/// 所以分工是硬的：`V2` 判普通值的支配，`V6` 判 mem 的支配与串行。
fn uses_of(i: &Instr) -> Vec<ValueId> {
    let mut out = Vec::new();
    match i {
        Instr::Const { .. } | Instr::AddrOf { .. } => {}
        Instr::Copy { src, .. } => out.push(*src),
        Instr::Bin { lhs, rhs, .. } => {
            out.push(*lhs);
            out.push(*rhs);
        }
        Instr::Un { x, .. } | Instr::Convert { x, .. } => out.push(*x),
        Instr::TypeAssert { x, .. } => out.push(*x),
        Instr::FuncAddr { .. } | Instr::CapturePtr { .. } => {}
        Instr::Defer { fv } => out.push(*fv),
        Instr::NilCheck { addr } => out.push(*addr),
        Instr::MakeClosure { captures, .. } => out.extend(captures.iter().copied()),
        Instr::Field { base, .. } => out.push(*base),
        Instr::Index { base, i, .. } => {
            out.push(*base);
            out.push(*i);
        }
        Instr::Slice {
            base, lo, hi, max, ..
        } => {
            out.push(*base);
            out.extend(lo.iter().copied());
            out.extend(hi.iter().copied());
            out.extend(max.iter().copied());
        }
        Instr::Composite { items, .. } => out.extend(items.iter().copied()),
        Instr::Make { len, cap, .. } => {
            out.extend(len.iter().copied());
            out.extend(cap.iter().copied());
        }
        // mem 交给 V6（见函数头）
        Instr::Load { addr, .. } => out.push(*addr),
        Instr::Store { addr, val, .. } => {
            out.push(*addr);
            out.push(*val);
        }
        Instr::Call { callee, args, .. } => {
            match callee {
                CalleeRef::Method { recv, .. } => out.push(*recv),
                CalleeRef::Iface { recv, .. } => out.push(*recv),
                CalleeRef::Indirect(v) => out.push(*v),
                CalleeRef::Func(_) | CalleeRef::Builtin(_) | CalleeRef::Runtime(_) => {}
            }
            out.extend(args.iter().copied());
        }
    }
    out
}

/// 终止符用到的值（边实参**也是使用**：它们喂给目标块的参数）。
fn term_uses(t: &Term, out: &mut Vec<ValueId>) {
    match t {
        Term::Goto { args, .. } => out.extend(args.iter().copied()),
        Term::If { cond, then_, else_ } => {
            out.push(*cond);
            out.extend(then_.1.iter().copied());
            out.extend(else_.1.iter().copied());
        }
        Term::Return(vs) => out.extend(vs.iter().copied()),
        Term::Panic(v) => out.push(*v),
        Term::Unreachable => {}
    }
}

/// `V2`：每个使用点都必须被其定义点**支配**。
///
/// 两条子判据：
/// ① 同一个块里 —— 定义位置必须在使用位置**之前**（块参数位置为 `0..n`，
///    所以「用块参数」天然满足）；
/// ② 跨块 —— 定义块必须支配使用块（`DomTree::dominates`）。
///
/// **块参数不要求支配前驱**：它的定义点在块入口，由前驱边提供实参。
/// 该实参本身是「前驱里的使用」，由前驱那一侧检查 —— 这条分工是 `V7` 的前提。
///
/// **跳过不可达块**：不可达块的支配关系无定义（`dominates` 恒假），
/// 若在此报 `V2`，死块的真实问题（`V8`）就会被一个假的不变式盖住。
fn check_v2(
    f: &SsaFunc,
    cfg: &Cfg,
    dom: &DomTree,
    def: &[(BlockId, usize)],
) -> Result<(), VerifyError> {
    for (bi, blk) in f.blocks.iter().enumerate() {
        let b = BlockId(bi as u32);
        if cfg.is_dead(b) {
            continue;
        }
        let base = blk.params.len();
        for (ii, ins) in blk.instrs.iter().enumerate() {
            let idx = base + ii;
            for v in uses_of(ins) {
                check_one_use(dom, def, b, idx, v, kind_of_instr(ins))?;
            }
        }
        let mut tu = Vec::new();
        term_uses(&blk.term, &mut tu);
        let at = base + blk.instrs.len();
        for v in tu {
            check_one_use(dom, def, b, at, v, kind_of_term(&blk.term))?;
        }
    }
    Ok(())
}

/// `V2` 的单点判定（`where_` 用于把「谁在用」写进 detail）。
fn check_one_use(
    dom: &DomTree,
    def: &[(BlockId, usize)],
    b: BlockId,
    idx: usize,
    v: ValueId,
    where_: &str,
) -> Result<(), VerifyError> {
    if def.get(v.index()).is_none() {
        // 到不了：`V1` 已要求每个值都有定义点。留着是为了不 panic。
        return Err(err(
            "V2",
            b,
            idx,
            format!("v{} 没有定义点（这条本该由 V1 报）", v.0),
        ));
    }
    match use_violation(dom, def, b, idx, v, where_) {
        Some(detail) => Err(err("V2", b, idx, detail)),
        None => Ok(()),
    }
}

/// 支配判定的**共用内核**（`V2` 与 `V6` 都用它，只是报不同不变式）。
///
/// 只返回 `Option<String>`（人读原因），不造 `VerifyError` —— 否则这里就得知道
/// 调用方是 `V2` 还是 `V6`，那会让「同一条判据」被写两遍。
fn use_violation(
    dom: &DomTree,
    def: &[(BlockId, usize)],
    b: BlockId,
    idx: usize,
    v: ValueId,
    where_: &str,
) -> Option<String> {
    let &(db, didx) = def.get(v.index())?;
    if db == b {
        if didx >= idx {
            return Some(format!(
                "v{} 在 b{}[{didx}] 定义，却在它之前（[{idx}]，{where_}）被使用",
                v.0, b.0
            ));
        }
        return None;
    }
    if !dom.dominates(db, b) {
        return Some(format!(
            "v{} 在 b{}[{didx}] 定义，但 b{} 不支配 b{}（{where_}）",
            v.0, db.0, db.0, b.0
        ));
    }
    None
}

/// `V3`：类型自洽（判据全在 [`crate::typing`]，这里只负责定位与打包）。
///
/// 三块：① 逐指令（`typing::check_instr`）；② `if` 的条件必须是 `bool`；
/// ③ 边实参的类型必须与目标块参数的类型逐个相同（**这是块参数的正确性判据**，
/// 只有个数由 `V7` 管）。③ 按两边较短的个数比 —— 个数不等归 `V7`，
/// 若这里也报，同一张图会因检查顺序不同报出不同的不变式。
fn check_v3(f: &SsaFunc, cfg: &Cfg, tt: &TypeTable) -> Result<(), VerifyError> {
    for (bi, blk) in f.blocks.iter().enumerate() {
        let b = BlockId(bi as u32);
        let base = blk.params.len();
        for (ii, ins) in blk.instrs.iter().enumerate() {
            if let Err(detail) = crate::typing::check_instr(f, ins, tt) {
                return Err(err("V3", b, base + ii, detail));
            }
        }
        let at = base + blk.instrs.len();
        if let Term::If { cond, .. } = &blk.term {
            match crate::typing::type_of(f, *cond) {
                Some(t) if crate::typing::is_bool(tt, t) => {}
                Some(t) => {
                    return Err(err(
                        "V3",
                        b,
                        at,
                        format!("if 的条件必须是 bool，v{} 的类型是 {t:?}", cond.0),
                    ));
                }
                None => {
                    return Err(err("V3", b, at, format!("if 的条件 v{} 没有类型", cond.0)));
                }
            }
        }
        for (to, args) in term_edges(&blk.term) {
            let params = f
                .blocks
                .get(to.index())
                .map_or(&[][..], |x| x.params.as_slice());
            for (k, (a, p)) in args.iter().zip(params.iter()).enumerate() {
                let (ta, tp) = match (crate::typing::type_of(f, *a), crate::typing::type_of(f, *p))
                {
                    (Some(x), Some(y)) => (x, y),
                    _ => continue, // 越界值由 V1 报
                };
                // 类型相同 ≠ `TypeId` 相同：`add_basic` 不去重（同一个 `int` 可有多条）
                if !crate::typing::same_type(tt, ta, tp) {
                    return Err(err(
                        "V3",
                        b,
                        at,
                        format!(
                            "边 b{}→b{} 的第 {} 个实参 v{} 是 {ta:?}，但目标参数 v{} 是 {tp:?}",
                            b.0,
                            to.0,
                            k + 1,
                            a.0,
                            p.0
                        ),
                    ));
                }
            }
        }
    }
    let _ = cfg; // 目前不需要 CFG；留着签名以便 V7 复用
    Ok(())
}

/// `V4`：前驱/后继**互逆**，且入口块没有前驱。
///
/// 三条子判据：① 边的目标块存在；② 同一目标的两条边实参必须相同
/// （IR 里没有「走的是哪条边」的信息，不同实参就是不可表达）；
/// ③ 每条边在 `cfg` 的两侧都出现（IR 与 CFG 一致）。
fn check_v4(f: &SsaFunc, cfg: &Cfg) -> Result<(), VerifyError> {
    let n = f.blocks.len();
    if n == 0 {
        return Err(err("V4", BlockId(0), 0, "函数一个块都没有"));
    }

    for (bi, blk) in f.blocks.iter().enumerate() {
        let b = BlockId(bi as u32);
        let at = blk.params.len() + blk.instrs.len();
        let edges = term_edges(&blk.term);

        for (k, (to, args)) in edges.iter().enumerate() {
            if to.index() >= n {
                return Err(err(
                    "V4",
                    b,
                    at,
                    format!(
                        "终止符第 {} 条边指向不存在的 b{}（本函数 {n} 块）",
                        k + 1,
                        to.0
                    ),
                ));
            }
            for (to2, args2) in edges.iter().skip(k + 1) {
                if to2 == to && args2 != args {
                    return Err(err(
                        "V4",
                        b,
                        at,
                        format!(
                            "同一目标 b{} 有两条边且实参不同（{} 个 vs {} 个）—— IR 无法表达走的是哪条",
                            to.0,
                            args.len(),
                            args2.len()
                        ),
                    ));
                }
            }
            if !cfg.preds(*to).contains(&b) {
                return Err(err(
                    "V4",
                    b,
                    at,
                    format!("b{} 的后继里有 b{}，但它的前驱里没有 b{}", b.0, to.0, b.0),
                ));
            }
        }

        for p in cfg.preds(b) {
            if !cfg.succs(*p).contains(&b) {
                return Err(err(
                    "V4",
                    b,
                    0,
                    format!("b{} 把 b{} 当前驱，但它自己的后继里没有 b{}", p.0, b.0, b.0),
                ));
            }
        }
    }

    let entry = BlockId(0);
    if !cfg.preds(entry).is_empty() {
        return Err(err(
            "V4",
            entry,
            0,
            format!(
                "入口块有 {} 条前驱（入口不许被跳入）",
                cfg.preds(entry).len()
            ),
        ));
    }
    Ok(())
}

/// `V5`：调用与签名一致（结果个数、实参个数）。
///
/// # 三档口径（都不许「查不了就算过」）
///
/// | 被调方 | 判据 |
/// |---|---|
/// | `Func` / `Method` | 签名必须在 `call_sigs` 里；个数逐个对；**查不到就报错** |
/// | `Builtin` | 不判 —— 内建元数是 `rgoc-interp` 的实现细节（`println` 可变长、`len` 不返回值），在 IR 层既查不准也不该查 |
/// | `Indirect` | 不判 —— 签名运行期才定（M6 允许建图，`effect` 记 `writes_all`） |
///
/// 变参函数（`Sig::variadic`）：`args.len()` 允许从 `params - 1` 起（变参那一段
/// 可以是空）。**这是唯一一处「个数不相等也合法」的地方**，所以它必须由 `Sig`
/// 显式带出来，不能靠猜。
fn check_v5(f: &SsaFunc) -> Result<(), VerifyError> {
    for (bi, blk) in f.blocks.iter().enumerate() {
        let b = BlockId(bi as u32);
        let base = blk.params.len();
        for (ii, ins) in blk.instrs.iter().enumerate() {
            let Instr::Call {
                dsts, callee, args, ..
            } = ins
            else {
                continue;
            };
            let idx = base + ii;
            let fid = match callee {
                CalleeRef::Func(x) => Some(*x),
                CalleeRef::Method { func, .. } => Some(*func),
                // 接口派发没有静态签名可查（被调方运行期才定）—— `V5` 管不到它。
                // 这不是漏判：`CalleeRef::Iface` 的实参形状由 build 按**接口签名**打包，
                // 而接口签名在 HIR 里已经检查过（`M5` 的 `C4`/`C8`）。
                CalleeRef::Builtin(_)
                | CalleeRef::Indirect(_)
                | CalleeRef::Runtime(_)
                | CalleeRef::Iface { .. } => None,
            };
            let Some(fid) = fid else { continue };
            let Some(sig) = f.call_sigs.get(&fid) else {
                return Err(err(
                    "V5",
                    b,
                    idx,
                    format!(
                        "被调方 func#{} 没有签名记录 —— 「查不了」不许当成通过",
                        fid.0
                    ),
                ));
            };
            if dsts.len() != sig.results {
                return Err(err(
                    "V5",
                    b,
                    idx,
                    format!(
                        "调用 func#{} 的结果个数不符：接了 {} 个，签名有 {} 个",
                        fid.0,
                        dsts.len(),
                        sig.results
                    ),
                ));
            }
            // 方法调用的**接收者单列**（`CalleeRef::Method.recv`），所以比的时候
            // 要把它算回实参里 —— `call_sigs` 记的是完整签名（含接收者）。
            let given = args.len() + usize::from(matches!(callee, CalleeRef::Method { .. }));
            let ok = if sig.variadic {
                given + 1 >= sig.params
            } else {
                given == sig.params
            };
            if !ok {
                return Err(err(
                    "V5",
                    b,
                    idx,
                    format!(
                        "调用 func#{} 的实参个数不符：给了 {} 个，签名要 {}{}",
                        fid.0,
                        args.len(),
                        sig.params,
                        if sig.variadic {
                            "（变参，可少给末尾那段）"
                        } else {
                            ""
                        }
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// `V6`：mem 链（`M6-design.md` §3.5）。
///
/// ① 每条 `Load`/`Store` 的 `mem` 操作数必须**支配**该指令（同块则须在前）；
/// ② **同块同根**的连续访存必须逐个接续：后者的 `mem` 必须等于前者留下的 mem
///    （`Store` 留它自己的 `dst`，`Load` 不改 mem，仍留原来那个）。
///
/// # ②为什么只查同块
///
/// 跨块的「上一个同根访存在哪」不是一个结构事实（要沿支配树找最近公共前驱），
/// 它是 **mem 值的支配性**问题 —— 已经由 ① 覆盖。②只需钉住最容易写错的那一处：
/// 同一个块里 store 之后忘了改用新 mem（这时 ① 是过的，因为旧 mem 同样支配）。
fn check_v6(f: &SsaFunc, dom: &DomTree, def: &[(BlockId, usize)]) -> Result<(), VerifyError> {
    for (bi, blk) in f.blocks.iter().enumerate() {
        let b = BlockId(bi as u32);
        let base = blk.params.len();
        // 同块「每个根最近一次访存之后该用的 mem」
        let mut last: BTreeMap<RootId, ValueId> = BTreeMap::new();
        for (ii, ins) in blk.instrs.iter().enumerate() {
            let (mem, root, produced) = match ins {
                Instr::Load { mem, root, .. } => (*mem, *root, None),
                Instr::Store { dst, mem, root, .. } => (*mem, *root, Some(*dst)),
                _ => continue,
            };
            let idx = base + ii;
            let what = if produced.is_some() { "store" } else { "load" };
            if let Some(detail) = use_violation(dom, def, b, idx, mem, "mem 操作数") {
                return Err(err("V6", b, idx, format!("{what} 的 {detail}")));
            }
            if let Some(prev) = last.get(&root)
                && *prev != mem
            {
                return Err(err(
                    "V6",
                    b,
                    idx,
                    format!(
                        "{what} 对根 R{} 用的是 v{}，但同块上一次同根访存留下的是 v{} —— 跳过了新 mem",
                        root.0, mem.0, prev.0
                    ),
                ));
            }
            last.insert(root, produced.unwrap_or(mem));
        }
    }
    Ok(())
}

/// `V9`：取址即建根 —— `roots` 里每个 `Slot` 根与 `slot_root` 互为逆，且
/// 每个 `AddrOf` 指向的根都在范围内。
///
/// # 为什么是「互为逆」而不是「两端各查一次」
///
/// `slot_root: SlotId → RootId` 与 `roots: RootId → Root` 是同一件事的两种索引。
/// 只要允许一边有、另一边没有，就会出现「某个槽看起来取过址，但建图时按值处理了」
/// 这类静默错误 —— 而它的症状是**语义错、测试大部分仍绿**（M5 的 `Rc` 共享同类）。
/// 所以这条不变式要求两侧严格互逆：多一项、少一项、指错方向都算违反。
fn check_v9(f: &SsaFunc) -> Result<(), VerifyError> {
    // ① roots 里的 Slot 根必须在 slot_root 里有回指
    for (ri, r) in f.roots.iter().enumerate() {
        let Root::Slot(s) = r else { continue };
        match f.slot_root.get(s) {
            Some(back) if back.index() == ri => {}
            Some(back) => {
                return Err(err(
                    "V9",
                    f.entry,
                    0,
                    format!(
                        "槽 s{} 的根是 R{ri}，但 slot_root 指回 R{}（两边不一致）",
                        s.0, back.0
                    ),
                ));
            }
            None => {
                return Err(err(
                    "V9",
                    f.entry,
                    0,
                    format!("roots 里有槽 s{} 的根 R{ri}，但 slot_root 里没有它", s.0),
                ));
            }
        }
    }
    // ② slot_root 的每一项都必须指向一个 Slot 根
    for (s, r) in &f.slot_root {
        match f.roots.get(r.index()) {
            Some(Root::Slot(s2)) if s2 == s => {}
            Some(_) => {
                return Err(err(
                    "V9",
                    f.entry,
                    0,
                    format!(
                        "slot_root 把槽 s{} 指到 R{}，但那个根不是 slot 根（是 {}）",
                        s.0,
                        r.0,
                        f.roots.get(r.index()).map_or("?", kind_of_root)
                    ),
                ));
            }
            None => {
                return Err(err(
                    "V9",
                    f.entry,
                    0,
                    format!("slot_root 把槽 s{} 指到越界的 R{}", s.0, r.0),
                ));
            }
        }
    }
    // ③ 每个 AddrOf 的根必须在范围内
    for (bi, blk) in f.blocks.iter().enumerate() {
        let base = blk.params.len();
        for (ii, ins) in blk.instrs.iter().enumerate() {
            if let Instr::AddrOf { root, .. } = ins
                && root.index() >= f.roots.len()
            {
                return Err(err(
                    "V9",
                    BlockId(bi as u32),
                    base + ii,
                    format!(
                        "addr-of 指向不存在的根 R{}（本函数只有 {} 个根）",
                        root.0,
                        f.roots.len()
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// `V7`：每条边给的实参个数 == 目标块的参数个数。
///
/// 这是块参数方案唯一的**结构性**保证：个数对上，语义才轮得到并行拷贝去管
/// （`M6-design.md` §3.3.2）。指向不存在块的边归 `V4`（先跑），这里跳过。
fn check_v7(f: &SsaFunc) -> Result<(), VerifyError> {
    for (bi, blk) in f.blocks.iter().enumerate() {
        let b = BlockId(bi as u32);
        let at = blk.params.len() + blk.instrs.len();
        for (to, args) in term_edges(&blk.term) {
            let Some(target) = f.blocks.get(to.index()) else {
                continue; // 越界目标由 V4 报
            };
            if args.len() != target.params.len() {
                return Err(err(
                    "V7",
                    b,
                    at,
                    format!(
                        "边 b{}→b{} 给了 {} 个实参，但目标块有 {} 个参数",
                        b.0,
                        to.0,
                        args.len(),
                        target.params.len()
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// `V8`：非入口块必须从入口可达。
///
/// builder 在构建期丢弃真死块，所以这条只对手写 IR 有牙 —— 但它是
/// 「dump 里不会出现没有前驱的块」这条契约的唯一保证（`C10` 规则 6）。
fn check_v8(f: &SsaFunc, cfg: &Cfg) -> Result<(), VerifyError> {
    for bi in 1..f.blocks.len() {
        let b = BlockId(bi as u32);
        if cfg.is_dead(b) {
            return Err(err(
                "V8",
                b,
                0,
                format!("b{bi} 从入口不可达（死块应在构建期丢弃，不该留在 IR 里）"),
            ));
        }
    }
    Ok(())
}
