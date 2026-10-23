//! CFG：前驱/后继、边实参、可达性（M6/Task 8；`V4` / `V7` / `V8` 的原料）。
//!
//! # 为什么这些字段是**算出来的**而不是存进 IR
//!
//! `preds`/`succs` 完全由 `Term` 决定。存进 `SsaFunc` 会立刻出现「两处表示同一件事」
//! 的问题：改了一条边忘了改缓存，verifier 就查不出真问题（它查的是缓存）。
//! 所以 `Cfg::build` 每次现算 —— 代价是一次 O(边数) 的遍历，换来的是**只有一个真相**。
//!
//! # 排序纪律
//!
//! `succs` 保持**终止符里的顺序**（`if` 的 then 在前、else 在后）—— dump 与
//! 诊断要靠它稳定。`preds` 按**块号升序**（构造顺序），因为「谁先到这个块」
//! 在 CFG 里没有语义顺序，可复现比「源码顺序」更重要。

use std::collections::BTreeMap;

use crate::ir::{BlockId, SsaFunc, Term, ValueId};

/// 一个函数的控制流图。
#[derive(Clone, Debug)]
pub struct Cfg {
    preds: Vec<Vec<BlockId>>,
    succs: Vec<Vec<BlockId>>,
    /// `(from, to) -> 边上实参`
    edges: BTreeMap<(BlockId, BlockId), Vec<ValueId>>,
    reachable: Vec<bool>,
}

impl Cfg {
    /// 从一个函数建图。
    #[must_use]
    pub fn build(f: &SsaFunc) -> Cfg {
        let n = f.blocks.len();
        let mut preds: Vec<Vec<BlockId>> = vec![Vec::new(); n];
        let mut succs: Vec<Vec<BlockId>> = vec![Vec::new(); n];
        let mut edges: BTreeMap<(BlockId, BlockId), Vec<ValueId>> = BTreeMap::new();

        for (i, blk) in f.blocks.iter().enumerate() {
            let from = BlockId(i as u32);
            let mut out: Vec<BlockId> = Vec::new();
            match &blk.term {
                Term::Goto { to, args } => {
                    out.push(*to);
                    edges.insert((from, *to), args.clone());
                }
                Term::If { then_, else_, .. } => {
                    out.push(then_.0);
                    edges.insert((from, then_.0), then_.1.clone());
                    out.push(else_.0);
                    edges.insert((from, else_.0), else_.1.clone());
                }
                Term::Return(_) | Term::Panic(_) | Term::Unreachable => {}
            }
            for to in &out {
                // 目标块号可能越界（手写 IR 可以造出来）—— 那一侧由 verifier 的 V4 抓，
                // 这里只记「有这条边」，不为越界目标建 preds 槽，避免 panic。
                if let Some(slot) = preds.get_mut(to.index()) {
                    slot.push(from);
                }
            }
            succs[i] = out;
        }

        // preds 按块号升序（BTreeMap 无用，这里显式排）
        for p in &mut preds {
            p.sort_unstable();
        }

        // 可达性：从入口 BFS
        let mut reachable = vec![false; n];
        if let Some(first) = reachable.first_mut() {
            *first = true;
        }
        let mut stack: Vec<BlockId> = vec![BlockId(0)];
        while let Some(cur) = stack.pop() {
            if let Some(ss) = succs.get(cur.index()).cloned() {
                for to in ss {
                    if let Some(r) = reachable.get_mut(to.index())
                        && !*r
                    {
                        *r = true;
                        stack.push(to);
                    }
                }
            }
        }

        Cfg {
            preds,
            succs,
            edges,
            reachable,
        }
    }

    /// 前驱（按块号升序）。
    #[must_use]
    pub fn preds(&self, b: BlockId) -> &[BlockId] {
        self.preds.get(b.index()).map_or(&[], Vec::as_slice)
    }

    /// 后继（按终止符里的顺序）。
    #[must_use]
    pub fn succs(&self, b: BlockId) -> &[BlockId] {
        self.succs.get(b.index()).map_or(&[], Vec::as_slice)
    }

    /// 从入口是否可达。
    #[must_use]
    pub fn reachable(&self, b: BlockId) -> bool {
        self.reachable.get(b.index()).copied().unwrap_or(false)
    }

    /// 不可达（`V8` 的判据；`builder` 本不该产出这种块）。
    #[must_use]
    pub fn is_dead(&self, b: BlockId) -> bool {
        !self.reachable(b)
    }

    /// 边上实参（不存在的边返回空切片）。
    #[must_use]
    pub fn edge_args(&self, from: BlockId, to: BlockId) -> &[ValueId] {
        self.edges.get(&(from, to)).map_or(&[], Vec::as_slice)
    }
}
