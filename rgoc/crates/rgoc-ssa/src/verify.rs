//! verifier：九条不变式（`M6-design.md` §3.5 / `M6-tests.md` §3.1）。
//!
//! # 跑法与顺序
//!
//! 按 `V1 → V9` 顺序跑，**第一个失败即返回**。顺序固定是为了让断言能锚定：
//! 一张同时违反 `V1` 与 `V4` 的图必须**只**报 `V1`，否则同一张图在不同实现下
//! 报不同码，反例测试就退化成「反正它是红的」。
//!
//! # 当前实现范围（Task 12）
//!
//! `V1`（每值恰好定义一次）/ `V2`（定义支配使用）/ `V3`（类型自洽）/
//! `V4`（前驱后继互逆、入口无前驱）/ `V8`（可达性）。`V5/V6/V7/V9` 在 Task 13–14
//! 落地 —— **这里不写「占位通过」**，未实现的检查直接不跑，且本注释必须随实现
//! 一起删改（否则注释会变成谎言）。
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

use std::fmt;

use crate::cfg::Cfg;
use crate::dom::DomTree;
use crate::ir::{BlockId, CalleeRef, Instr, SsaFunc, Term, ValueId, kind_of_instr, kind_of_term};
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
    // V5 / V6 / V7：Task 13 / 14
    check_v8(f, &cfg)?;
    // V9：Task 14
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

/// 一条指令用到的全部值（**不带通配的 `match`**）。
///
/// 口径：`Call` 的接收者（`Method.recv` / `Indirect`）**算使用** —— 它们是值；
/// `Func` / `Builtin` 不是值，不算。
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
        Instr::Load { mem, addr, .. } => {
            out.push(*mem);
            out.push(*addr);
        }
        Instr::Store { mem, addr, val, .. } => {
            out.push(*mem);
            out.push(*addr);
            out.push(*val);
        }
        Instr::Call { callee, args, .. } => {
            match callee {
                CalleeRef::Method { recv, .. } => out.push(*recv),
                CalleeRef::Indirect(v) => out.push(*v),
                CalleeRef::Func(_) | CalleeRef::Builtin(_) => {}
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
    let Some(&(db, didx)) = def.get(v.index()) else {
        // 到不了：`V1` 已要求每个值都有定义点。留着是为了不 panic。
        return Err(err(
            "V2",
            b,
            idx,
            format!("v{} 没有定义点（这条本该由 V1 报）", v.0),
        ));
    };
    if db == b {
        if didx >= idx {
            return Err(err(
                "V2",
                b,
                idx,
                format!(
                    "v{} 在 b{}[{didx}] 定义，却在它之前（[{idx}]，{where_}）被使用",
                    v.0, b.0
                ),
            ));
        }
        return Ok(());
    }
    if !dom.dominates(db, b) {
        return Err(err(
            "V2",
            b,
            idx,
            format!(
                "v{} 在 b{}[{didx}] 定义，但 b{} 不支配 b{}（{where_}）",
                v.0, db.0, db.0, b.0
            ),
        ));
    }
    Ok(())
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
                if ta != tp {
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
