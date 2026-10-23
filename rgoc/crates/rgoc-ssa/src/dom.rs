//! 支配树（M6/Task 9；`V2` 的唯一前提）。
//!
//! # 用 Cooper–Harvey–Kennedy，不用 Lengauer–Tarjan
//!
//! 后者的近线性复杂度在这个规模（单函数、块数 ≤ 10^4）没有意义，而它的实现
//! 有四个数组要互相配合 —— 写错的概率远高于省下的时间。CHK 只有一条循环：
//! 逆后序（RPO）编号 + `intersect` 迭代到不动点。
//!
//! # 不可达块
//!
//! builder 本不该产出它们（`V8`），但手写 IR 可以。不可达块**没有 idom**，
//! `dominates` 对它一律为假 —— 而不是「随便给个祖先」，那会让 `V2` 放过非法图。
//!
//! # 语义约定
//!
//! * `idom(entry) == None`；
//! * `dominates(x, x) == true`（自反；`V2` 查「定义点支配使用点」，同点必须算通过）；
//! * 不可达的 `b`：`idom(b) == None`，`dominates(_, b) == false`。

use crate::cfg::Cfg;
use crate::ir::{BlockId, SsaFunc};

/// 支配树。
#[derive(Clone, Debug)]
pub struct DomTree {
    /// 内部用：入口指向自己，其余 `None` 表示「不可达」。
    idom: Vec<Option<BlockId>>,
    /// RPO 编号（越小越靠前；不可达块为 `usize::MAX`）。
    rpo: Vec<usize>,
}

impl DomTree {
    /// 建树。
    #[must_use]
    pub fn build(f: &SsaFunc, cfg: &Cfg) -> DomTree {
        let n = f.blocks.len();
        let rpo = Self::rpo_numbers(n, cfg);
        // 反查：rpo 序 → 块
        let mut order: Vec<BlockId> = (0..n)
            .map(|i| BlockId(i as u32))
            .filter(|b| rpo[b.index()] != usize::MAX)
            .collect();
        order.sort_by_key(|b| rpo[b.index()]);

        let mut idom: Vec<Option<BlockId>> = vec![None; n];
        if n > 0 {
            // 入口的 idom 指向自己（内部哨兵；对外 `idom()` 返回 None）
            idom[0] = Some(BlockId(0));
        }

        // 迭代到不动点
        let mut changed = true;
        while changed {
            changed = false;
            for &b in &order {
                if b == BlockId(0) {
                    continue;
                }
                let mut new_idom: Option<BlockId> = None;
                for &p in cfg.preds(b) {
                    if idom[p.index()].is_none() {
                        continue; // 前驱不可达（或还没算出来）
                    }
                    new_idom = Some(match new_idom {
                        None => p,
                        Some(cur) => Self::intersect(p, cur, &idom, &rpo),
                    });
                }
                if new_idom.is_some() && idom[b.index()] != new_idom {
                    idom[b.index()] = new_idom;
                    changed = true;
                }
            }
        }

        DomTree { idom, rpo }
    }

    /// 逆后序编号（不可达 = `usize::MAX`）。迭代 DFS，不递归。
    fn rpo_numbers(n: usize, cfg: &Cfg) -> Vec<usize> {
        if n == 0 {
            return Vec::new();
        }
        let mut post: Vec<BlockId> = Vec::new();
        let mut seen = vec![false; n];
        // 显式栈：(块, 下一个待访问的后继下标)
        let mut stack: Vec<(BlockId, usize)> = vec![(BlockId(0), 0)];
        seen[0] = true;
        while let Some((b, i)) = stack.pop() {
            let succs = cfg.succs(b);
            if i < succs.len() {
                stack.push((b, i + 1));
                let nxt = succs[i];
                if let Some(s) = seen.get_mut(nxt.index())
                    && !*s
                {
                    *s = true;
                    stack.push((nxt, 0));
                }
            } else {
                post.push(b);
            }
        }
        let mut rpo = vec![usize::MAX; n];
        for (k, b) in post.iter().rev().enumerate() {
            rpo[b.index()] = k;
        }
        rpo
    }

    /// 沿 RPO 向上找两个块的最近公共支配者。
    fn intersect(
        mut a: BlockId,
        mut b: BlockId,
        idom: &[Option<BlockId>],
        rpo: &[usize],
    ) -> BlockId {
        while a != b {
            while rpo[a.index()] > rpo[b.index()] {
                a = idom[a.index()].expect("在 RPO 上的块必有 idom");
            }
            while rpo[b.index()] > rpo[a.index()] {
                b = idom[b.index()].expect("在 RPO 上的块必有 idom");
            }
        }
        a
    }

    /// 直接支配者（入口与不可达块都返回 `None`）。
    #[must_use]
    pub fn idom(&self, b: BlockId) -> Option<BlockId> {
        if b.index() == 0 {
            return None;
        }
        self.idom.get(b.index()).copied().flatten()
    }

    /// `a` 是否支配 `b`（自反；`b` 不可达时恒假）。
    #[must_use]
    pub fn dominates(&self, a: BlockId, b: BlockId) -> bool {
        if self.rpo.get(b.index()).copied().unwrap_or(usize::MAX) == usize::MAX {
            return false;
        }
        if a == b {
            return true;
        }
        let mut cur = b;
        while let Some(next) = self.idom(cur) {
            if next == a {
                return true;
            }
            cur = next;
        }
        false
    }
}
