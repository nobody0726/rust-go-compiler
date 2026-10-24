//! 块参数的并行拷贝（`D-M7-6`；`M6-design.md` §3.3.2 的机器码版）。
//!
//! # 为什么这件事需要一整个模块
//!
//! 块参数的实参是**并行赋值**：`goto b1(v2, v1)` 要把 `p0 ← v2, p1 ← v1` 同时完成。
//! 顺序搬在「源与目标不重叠」时碰巧正确，在重叠时**静默错**：
//!
//! ```text
//! a, b = b, a          pairs: [(a←b), (b←a)]
//! 顺序搬： a←b  ⇒ 此时 a 已是旧 b；再 b←a ⇒ b 也成了旧 b   ⇒ (b, b)  ✗
//! ```
//!
//! 而 90% 的 fixture 不会交换两个值 —— 所以这个错只有在专门的用例上才露头。
//! 本仓 M6 已经在 IR 层踩过一次（`D-M6-3` 的失效条件），这里是同一个坑的第二次。
//!
//! # 正确的判据不是「源没被覆盖」，是「目标不被后面读到」
//!
//! 一条 `d ← s` 可以安全发射的充要条件是：**`d` 不再是任何剩余对的源**。
//! 因为搬完之后 `d` 的旧值就没了，而后面若还有 `x ← d`，它要的是旧值。
//!
//! （反过来「`s` 不再是任何剩余对的目标」是不够的：那样 `d` 的旧值仍可能被后面的对读走。）
//!
//! 没有这样的对 ⇒ 剩下的对构成一个环。环用**一个临时格**打断：
//! `tmp ← s`，然后把所有「读 `s`」的对改成「读 `tmp`」—— 于是环上多出一条
//! 出口（`d ← tmp`），下一次迭代就能发射。
//!
//! # 临时格从哪来
//!
//! `Frame::tmp_slots` 已经按「所有边里最多的实参个数」预留了（Task 9），
//! 所以这里只借用**一格**（`tmp` 参数），不需要按环长要空间。

/// 一条搬运：`dst ← src`（都是相对 `fp` 的偏移）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    /// 目标槽的偏移。
    pub dst: i32,
    /// 源槽的偏移。
    pub src: i32,
}

/// 排出一串**安全的**搬运序列。
///
/// # 参数
///
/// - `pairs`：`(目标偏移, 源偏移)`，**目标必须互不相同**（块参数就是互不相同的）。
/// - `tmp`：打断环用的临时格偏移。
///
/// # 返回
///
/// 按**发射顺序**排好的搬运。顺序本身是判据的一部分：同一个 `pairs` 必须得到
/// 同一个序列（`.s` 的逐字节可复现，`T-NAT-48`）。
///
/// # 复杂度
///
/// O(n²)（每轮线性扫）。边上的实参个数是个位数，不值得为它建索引。
#[must_use]
pub fn schedule(pairs: &[(i32, i32)], tmp: i32) -> Vec<Move> {
    let mut pending: Vec<(i32, i32)> = pairs.to_vec();
    let mut out: Vec<Move> = Vec::with_capacity(pending.len() + 1);

    while !pending.is_empty() {
        // ① 找一条「目标不再是**别的**剩余对的源」的对。
        //
        // 用下标排除自己：`(d ← s)` 自己当然「源里没有 d」这回事要看别的对。
        let ready = pending.iter().enumerate().position(|(i, (d, _))| {
            !pending
                .iter()
                .enumerate()
                .any(|(j, (_, s2))| j != i && s2 == d)
        });
        if let Some(i) = ready {
            let (d, s) = pending.remove(i);
            // 自赋值（`d ← d`）不必发。
            if d != s {
                out.push(Move { dst: d, src: s });
            }
            continue;
        }

        // ② 没有可发射的 ⇒ 剩下的是一个环。用临时格打断。
        //
        // 取第一个对 `(d, s)`：先把 `s` 存进临时格，再把所有「读 `s`」的源改成 `tmp`。
        // 于是 `(d, s)` 变成 `(d, tmp)`，而 `tmp` 不是任何目标 ⇒ 下一轮必可发射。
        let (_, s) = pending[0];
        out.push(Move { dst: tmp, src: s });
        for p in &mut pending {
            if p.1 == s {
                p.1 = tmp;
            }
        }
    }
    out
}

/// 模拟执行：把 `moves` 依次作用到 `values`（偏移 → 值）上。
///
/// **只给测试用** —— 生产代码里这些搬运会变成 `ldr`/`str`，没有「模拟」这一步。
/// 放在这里是为了让判据能直接问「执行完这些搬运，每个槽里是什么」，
/// 而不必去读汇编。
#[doc(hidden)]
pub fn simulate(moves: &[Move], values: &mut std::collections::BTreeMap<i32, i64>) {
    for m in moves {
        let v = values.get(&m.src).copied().unwrap_or_default();
        values.insert(m.dst, v);
    }
}
