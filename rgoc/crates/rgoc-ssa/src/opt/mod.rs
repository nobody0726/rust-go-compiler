//! SSA 优化 passes（`M6-design.md` §3.6；`D-M6-8`；`M6-plan.md` Phase 4）。
//!
//! # 四个 pass 与它们的边界
//!
//! | pass | 做什么 | 边界 |
//! |---|---|---|
//! | `copy` | `Copy` 链合并、块参数实参里的 `Copy` 消解 | 并行语义不许破（`a,b=b,a`） |
//! | `fold` | 常量折叠（`Bin`/`Un`/`Convert`）+ `If` 常量条件简化成 `Goto` | 只折**语义完全确定**的情形；除零/负移位一律不折 |
//! | `cse` | **块内**纯值公共子表达式消除 | 跨块不合（那要可用表达式分析，属 M7） |
//! | `dce` | 删无使用且无副作用的值；删不可达块 | `Store` 与有 effect 的 `Call` 不删 |
//!
//! # 为什么每个 pass 之后立刻 `verify`
//!
//! `SSA-5` 要求「每 pass 后 verifier 绿」。这不是洁癖：pass 破坏的不变式
//! （删了还有人用的值 = `V1`，块参数与实参不等长 = `V7`，死块 = `V8`）在
//! **求值器里表现为「碰巧对」或「静默算错」**，而不是崩溃。把 verify 挂在
//! pass 边界上，等于给每个 pass 一条「要么合法、要么当场红」的硬线。
//!
//! 代价是 O(passes × 函数数) 次 verify —— 语料 46 个函数级别，实测可忽略。
//!
//! # 顺序
//!
//! `copy` → `fold` → `cse` → `dce`：
//!
//! * `copy` 先做，`fold` 才看得见「常量经 `Copy` 传过来」的形态；
//! * `cse` 在 `dce` 前，免得「同一表达式出现两次」因为中间有死值而看不出来；
//! * `dce` 最后收垃圾（包括被 `fold` 改短的控制流留下的死块）。

pub mod copy;
pub mod cse;
pub mod dce;
pub mod fold;

use std::collections::{BTreeMap, BTreeSet};

use rgoc_types::TypeTable;

use crate::cfg::Cfg;
use crate::ir::{Block, BlockId, Effect, Instr, Module, SsaFunc, Term, ValueId};
use crate::verify::{VerifyError, verify};

/// 开关（`--opt=all|none|fold,dce,…`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// 常量折叠。
    pub fold: bool,
    /// 死代码删除。
    pub dce: bool,
    /// 复制传播。
    pub copy: bool,
    /// 块内公共子表达式消除。
    pub cse: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self::all()
    }
}

impl Config {
    /// 四个全开（默认）。
    #[must_use]
    pub const fn all() -> Self {
        Self {
            fold: true,
            dce: true,
            copy: true,
            cse: true,
        }
    }

    /// 四个全关（`--opt=none`：A/B 对照的另一半）。
    #[must_use]
    pub const fn none() -> Self {
        Self {
            fold: false,
            dce: false,
            copy: false,
            cse: false,
        }
    }

    /// 解析 `all` / `none` / 逗号分隔的 pass 名。
    ///
    /// # 为什么要拒空串与未知名
    ///
    /// `--opt=` 写成空值，最"友好"的处理是当 `all` —— 但那是**猜**：用户可能
    /// 想写 `none` 却漏了字。同理 `--opt=fold,dcel` 里的 `dcel` 若是静默忽略，
    /// 用户会以为关掉了 `dce`。所以两种都返回 `None`（调用方报 `BadOpt`）。
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "all" => return Some(Self::all()),
            "none" => return Some(Self::none()),
            "" => return None,
            _ => {}
        }
        let mut c = Self::none();
        for part in s.split(',') {
            match part.trim() {
                "fold" => c.fold = true,
                "dce" => c.dce = true,
                "copy" => c.copy = true,
                "cse" => c.cse = true,
                _ => return None,
            }
        }
        Some(c)
    }

    /// 有任何一个 pass 开着吗（`none` ⇒ 整个 `run` 是空操作）。
    #[must_use]
    pub const fn any(self) -> bool {
        self.fold || self.dce || self.copy || self.cse
    }

    /// 稳定文本（诊断与测试断言用）。
    #[must_use]
    pub fn as_str(self) -> String {
        if !self.any() {
            return "none".to_string();
        }
        if self == Self::all() {
            return "all".to_string();
        }
        let mut v = Vec::new();
        if self.fold {
            v.push("fold");
        }
        if self.dce {
            v.push("dce");
        }
        if self.copy {
            v.push("copy");
        }
        if self.cse {
            v.push("cse");
        }
        v.join(",")
    }
}

/// 跑全部启用的 pass（每个 pass 之后立刻 verify）。
///
/// # Errors
///
/// 任一 pass 之后图不合法 ⇒ 把 verifier 的错误原样抛出去（那是 pass 的 bug，
/// 不是用户的错，所以要带 `V*` 编号与块/指令下标）。
pub fn run(m: &mut Module, tt: &TypeTable, cfg: Config) -> Result<(), VerifyError> {
    for f in &mut m.funcs {
        if cfg.copy {
            copy::run(f);
            verify(f, tt)?;
        }
        if cfg.fold {
            fold::run(f, tt);
            verify(f, tt)?;
        }
        if cfg.cse {
            cse::run(f);
            verify(f, tt)?;
        }
        if cfg.dce {
            dce::run(f);
            verify(f, tt)?;
        }
    }
    Ok(())
}

/// 遍历一个函数的**全部使用点**（操作数；定义点不算）。
///
/// 三个 pass 都要它：`copy`/`cse` 改使用点，`dce` 数使用点。写一份，
/// 免得「某个 pass 忘了新指令的某个操作数」——那种漏在测试里极难看出来。
pub(crate) fn for_each_use(f: &mut SsaFunc, mut g: impl FnMut(&mut ValueId)) {
    for b in &mut f.blocks {
        for i in &mut b.instrs {
            instr_uses(i, &mut g);
        }
        term_uses(&mut b.term, &mut g);
    }
}

/// 一条指令的**使用点**（`copy`/`cse` 改它们，`dce` 数它们）。
pub(crate) fn instr_uses(i: &mut Instr, g: &mut impl FnMut(&mut ValueId)) {
    match i {
        Instr::Const { .. } | Instr::AddrOf { .. } => {}
        Instr::Copy { src, .. } => g(src),
        Instr::Bin { lhs, rhs, .. } => {
            g(lhs);
            g(rhs);
        }
        Instr::Un { x, .. } | Instr::Convert { x, .. } => g(x),
        // 断言只读 `x`（`ok` 是**结果**不是操作数）
        Instr::TypeAssert { x, .. } => g(x),
        Instr::FuncAddr { .. } | Instr::CapturePtr { .. } => {}
        Instr::Defer { fv } => g(fv),
        Instr::MakeClosure { captures, .. } => {
            for c in captures {
                g(c);
            }
        }
        Instr::Field { base, .. } => g(base),
        Instr::Index { base, i, .. } => {
            g(base);
            g(i);
        }
        Instr::Slice {
            base, lo, hi, max, ..
        } => {
            g(base);
            for x in [lo, hi, max].into_iter().flatten() {
                g(x);
            }
        }
        Instr::Composite { items, .. } => {
            for x in items {
                g(x);
            }
        }
        Instr::Make { len, cap, .. } => {
            for x in [len, cap].into_iter().flatten() {
                g(x);
            }
        }
        Instr::Load { mem, addr, .. } => {
            g(mem);
            g(addr);
        }
        Instr::Store { mem, addr, val, .. } => {
            g(mem);
            g(addr);
            g(val);
        }
        Instr::Call { callee, args, .. } => {
            // 三种被调方各带一个「值操作数」：`Indirect(fv)` / `Method { recv }`。
            // 漏掉 `recv` 的后果是**双重**的：`dce` 会以为那个地址值没人用而删掉它，
            // 压实又会把 `recv` 留在旧编号上（实测：`p.add(4)` 的接收者变成一个
            // 整数常量，运行期报「地址不是指针」）。
            match callee {
                crate::ir::CalleeRef::Indirect(v) => g(v),
                crate::ir::CalleeRef::Method { recv, .. } => g(recv),
                // 接口值（`tab` / `data` 两个字）也是被读的
                crate::ir::CalleeRef::Iface { recv, .. } => g(recv),
                crate::ir::CalleeRef::Func(_)
                | crate::ir::CalleeRef::Builtin(_)
                // 运行期助手没有值操作数（map 的键/值都走 `args`）。
                | crate::ir::CalleeRef::Runtime(_) => {}
            }
            for x in args {
                g(x);
            }
        }
    }
}

/// 一个终止符的使用点。
pub(crate) fn term_uses(t: &mut Term, g: &mut impl FnMut(&mut ValueId)) {
    match t {
        Term::Goto { args, .. } => {
            for x in args {
                g(x);
            }
        }
        Term::If { cond, then_, else_ } => {
            g(cond);
            for x in then_.1.iter_mut().chain(else_.1.iter_mut()) {
                g(x);
            }
        }
        Term::Return(vs) => {
            for x in vs {
                g(x);
            }
        }
        Term::Panic(v) => g(v),
        Term::Unreachable => {}
    }
}

/// 按 `map` 替换使用点（`map` 必须是**已解析到终点**的，不含链）。
pub(crate) fn subst(f: &mut SsaFunc, map: &BTreeMap<ValueId, ValueId>) {
    if map.is_empty() {
        return;
    }
    for_each_use(f, |v| {
        if let Some(&n) = map.get(v) {
            *v = n;
        }
    });
}

/// 全部被用到的值。
pub(crate) fn used_values(f: &mut SsaFunc) -> BTreeSet<ValueId> {
    let mut set = BTreeSet::new();
    for_each_use(f, |v| {
        set.insert(*v);
    });
    set
}

/// 一条指令的**结果**（多结果的 `Call` 有多个）。
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
        // 捕获是**操作数**不是定义（它们在 `uses` 里）
        Instr::MakeClosure { dst, .. } => vec![*dst],
        Instr::TypeAssert { dst, ok, .. } => {
            let mut v = vec![*dst];
            v.extend(ok.iter().copied());
            v
        }
    }
}

/// 一条指令的结果（可变引用，`compact_values` 用）。
fn dsts_of_mut(i: &mut Instr) -> Vec<&mut ValueId> {
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
        | Instr::Store { dst, .. } => vec![dst],
        Instr::Call { dsts, .. } => dsts.iter_mut().collect(),
        Instr::FuncAddr { dst, .. } | Instr::CapturePtr { dst, .. } => vec![dst],
        Instr::Defer { .. } => Vec::new(),
        // 捕获是**操作数**不是定义（它们在 `uses` 里）
        Instr::MakeClosure { dst, .. } => vec![dst],
        Instr::TypeAssert { dst, ok, .. } => match ok {
            Some(o) => vec![dst, o],
            None => vec![dst],
        },
    }
}

/// 值重编号（删过指令之后**必须**做，`V1` 要求每个值都有定义点）。
///
/// # 为什么不能「删指令、留空洞」
///
/// `V1` 遍历的是**全部** `value_types`（不是「被用到的那些」）：任何下标只要
/// 没有定义点就报「v_k 从未被定义」。所以 `dce`/`cse`/`copy` 删完指令之后，
/// 必须把剩下的值压实 —— 否则下一轮 `verify` 立刻红。
///
/// 代价是 `ValueId` 会变（dump 里的编号会跳变），所以这个函数只在 pass 收尾时
/// 调一次，不在 pass 中间调（中间调会让 pass 自己持有的 id 失效）。
pub(crate) fn compact_values(f: &mut SsaFunc) {
    let mut map: BTreeMap<ValueId, ValueId> = BTreeMap::new();
    let mut types: Vec<rgoc_types::TypeId> = Vec::new();
    for b in &f.blocks {
        for p in &b.params {
            map.insert(*p, ValueId(types.len() as u32));
            types.push(f.value_types[p.index()]);
        }
        for i in &b.instrs {
            for d in dsts_of(i) {
                map.insert(d, ValueId(types.len() as u32));
                types.push(f.value_types[d.index()]);
            }
        }
    }
    if types.len() == f.value_types.len() && map.iter().all(|(k, v)| k == v) {
        return;
    }
    for b in &mut f.blocks {
        for p in &mut b.params {
            *p = map[p];
        }
        for i in &mut b.instrs {
            for d in dsts_of_mut(i) {
                *d = map[d];
            }
        }
    }
    for_each_use(f, |v| {
        *v = *map
            .get(v)
            .unwrap_or_else(|| panic!("操作数 v{} 没有定义点（pass 删了还有人用的值）", v.0));
    });
    f.value_types = types;
    // ⚠️ `SsaFunc::params` 是**入口块 params 的副本**（`C10` 规则 2）。重编号只改
    // 了块上的那份 —— 不同步的话，求值器按 `f.params` 绑形参、函数体按块参数读，
    // 两边指向**不同的值**（实测：`p.add(4)` 的实参绑到旧编号上，函数体读到
    // 未定义的值 ⇒ 结果少加一次）。
    f.params = f.blocks[f.entry.index()].params.clone();
}

/// 副作用全空吗（`dce` 用它判「这个调用能不能删」）。
pub(crate) fn pure_effect(e: &Effect) -> bool {
    e.writes.is_empty() && !e.writes_all && !e.may_panic && !e.may_call_back && !e.prints
}

/// 删掉从入口不可达的块（`V8` 要求所有块可达）。
///
/// # 为什么要重编号
///
/// `BlockId` 是 `Vec` 下标。删块之后要么留空洞（`Cfg`/`verify` 全部要处理
/// `Option<Block>`），要么重编号。选重编号：IR 的其余部分（值、根、类型）
/// 都不用动，代价只是把所有 `Term` 的目标改一遍。
///
/// 返回「是否删过」。
pub(crate) fn prune_dead_blocks(f: &mut SsaFunc) -> bool {
    let cfg = Cfg::build(f);
    if (0..f.blocks.len()).all(|i| !cfg.is_dead(BlockId(i as u32))) {
        return false;
    }
    let mut remap: Vec<Option<BlockId>> = vec![None; f.blocks.len()];
    let mut next = 0u32;
    for (i, slot) in remap.iter_mut().enumerate() {
        if !cfg.is_dead(BlockId(i as u32)) {
            *slot = Some(BlockId(next));
            next += 1;
        }
    }
    let old = std::mem::take(&mut f.blocks);
    let mut kept: Vec<Block> = Vec::with_capacity(next as usize);
    for (i, b) in old.into_iter().enumerate() {
        if remap[i].is_some() {
            kept.push(b);
        }
    }
    for b in &mut kept {
        remap_targets(&mut b.term, &remap);
    }
    f.blocks = kept;
    f.entry = remap[f.entry.index()].unwrap_or(BlockId(0));
    true
}

fn remap_targets(t: &mut Term, remap: &[Option<BlockId>]) {
    let m = |b: BlockId| remap[b.index()].unwrap_or(b);
    match t {
        Term::Goto { to, .. } => *to = m(*to),
        Term::If { then_, else_, .. } => {
            then_.0 = m(then_.0);
            else_.0 = m(else_.0);
        }
        Term::Return(_) | Term::Panic(_) | Term::Unreachable => {}
    }
}
