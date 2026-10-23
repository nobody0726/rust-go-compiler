//! verifier：九条不变式（`M6-design.md` §3.5 / `M6-tests.md` §3.1）。
//!
//! # 跑法与顺序
//!
//! 按 `V1 → V9` 顺序跑，**第一个失败即返回**。顺序固定是为了让断言能锚定：
//! 一张同时违反 `V1` 与 `V4` 的图必须**只**报 `V1`，否则同一张图在不同实现下
//! 报不同码，反例测试就退化成「反正它是红的」。
//!
//! # 当前实现范围（Task 10）
//!
//! `V1`（每值恰好定义一次）/ `V4`（前驱后继互逆、入口无前驱）/ `V8`（可达性）。
//! `V2/V3/V5/V6/V7/V9` 分别在 Task 11–14 落地 —— **这里不写「占位通过」**，
//! 未实现的检查直接不跑，且本注释必须随实现一起删改（否则注释会变成谎言）。
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
use crate::ir::{BlockId, Instr, SsaFunc, Term, ValueId};

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
pub fn verify(f: &SsaFunc) -> Result<(), VerifyError> {
    let cfg = Cfg::build(f);
    check_v1(f)?;
    // V2 / V3：Task 11 / 12
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
/// 「从未定义」不归 `V2` 管 —— 支配检查只能看到**存在**的定义点，看不见
/// 「这个值根本没有定义点」，于是 `v9` 会一路滑到求值期变成「取值失败」。
fn check_v1(f: &SsaFunc) -> Result<(), VerifyError> {
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

    for (v, site) in def.iter().enumerate() {
        if site.is_none() {
            return Err(err(
                "V1",
                f.entry,
                0,
                format!("v{v} 从未被定义（块参数与指令结果里都没有它）"),
            ));
        }
    }
    Ok(())
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
