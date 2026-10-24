//! SSA 求值器（`M6-design.md` §3.8；`D-M6-7`；`M6-plan.md` Task 32 起）。
//!
//! # 为什么它住在 `rgoc-interp`
//!
//! `D-M6-7`：**值模型只有一个 crate**。SSA 求值器要复用 M5 的 [`Value`] / [`Cell`] /
//! `ops` / `print`，所以它必须与解释器同 crate —— 放在 `rgoc-ssa` 会让「纯数据」
//! 的 crate 拖上运行期，放第三个 crate 会造出第二套 `Value`。依赖图上就不允许
//! 第三套 runtime 存在。
//!
//! # 执行形态：显式帧 + 按块直线走
//!
//! 与 M5 的解释器同一条纪律（`D-M5-1`）：**不递归**。每个调用一帧，帧里记
//! `(函数, 当前块, 块内下标, 值表)`；主循环取「当前块的下一条指令」执行，
//! `Term` 决定下一块。这样 M8 的 goroutine 直接存/取帧栈即可。
//!
//! # 内存：运行期按**真实地址**访存，`root` 只用于静态定序
//!
//! IR 的 `Load/Store` 带显式的 `root`（`D-M6-5`），但那是**静态**信息：
//! 它让 verifier 不跑别名分析就能查「同根保序」。运行期不能按 `RootId` 分区 ——
//! `RootId` 是**函数内**的下标，而 `&a` 传给 `inc(p *int)` 之后，被调方拿到的
//! 指针必须指回**调用方**的槽。所以运行期一律走 [`Value::Ptr`] 里的真实 `Cell`，
//! `root` 字段不参与取址。
//!
//! # 当前进度（Task 32：骨架）
//!
//! 只做「帧 + 主循环 + `Return`」：空函数能跑（`main` 立即返回，退出码 0）。
//! 指令执行（Task 33）、控制流（Task 34）、调用与访存（Task 35）都还没进来 ——
//! 遇到就 `todo!()`，**不静默跳过**（跳过一条指令等于算错，而算错不会报错）。

use rgoc_hir::FuncId;
use rgoc_ssa::ir::{BlockId, Module, Term};
use rgoc_types::TypeTable;

use crate::interp::{Options, Outcome};
use crate::value::Value;

/// 一个 SSA 帧。
///
/// 字段顺序按「定位 → 状态」排：先三个下标（函数 / 块 / 指令），再值表。
struct SFrame {
    /// `Module::funcs` 的下标（不是 `FuncId` —— 被拒的函数不进 `funcs`）。
    func: usize,
    /// 当前块。
    block: BlockId,
    /// 块内**下一条**待执行指令的下标（`== instrs.len()` ⇒ 该走终止符了）。
    idx: usize,
    /// 值表（下标即 `ValueId`；块参数写在这里）。
    ///
    /// 骨架期只分配不读（`Task 33` 起每条指令往这里写结果）。
    /// `expect` 而不是 `allow`：等它真的被读了，这条期望会失效并提醒删掉。
    #[expect(dead_code, reason = "Task 33 起写值表")]
    values: Vec<Value>,
}

/// 执行状态。
struct Exec<'a> {
    m: &'a Module,
    frames: Vec<SFrame>,
    /// 合并输出流（与 M5 的解释器同一个语义：`print`/`println` 都进这里）。
    output: String,
    steps: u64,
    max_steps: u64,
}

/// 跑一个 SSA 模块（`Module::init` → `main`）。
///
/// `_tt` 现在用不到（骨架只跑空函数）；`Task 33` 起按 `TypeId` 决定运算宽度，
/// 签名先冻结在这里 —— 免得后面每个调用点都改一遍。
#[must_use]
pub fn run_ssa(m: &Module, _tt: &TypeTable, opts: Options) -> Outcome {
    let mut ex = Exec {
        m,
        frames: Vec::new(),
        output: String::new(),
        steps: 0,
        max_steps: opts.max_steps,
    };

    // ① 包初始化（`Task 32b` 合成的那个函数）
    if let Some(i) = m.init
        && let Err(e) = ex.call(i)
    {
        return ex.finish(Err(e));
    }
    // ② main
    let Some(main) = m.main else {
        return ex.finish(Err(Halt::NoMain));
    };
    let Some(i) = index_of(m, main) else {
        return ex.finish(Err(Halt::NoMain));
    };
    let r = ex.call(i);
    ex.finish(r)
}

/// 停机原因（运行期的「非正常结束」）。
enum Halt {
    /// 模块没有 `main`（或被拒了，所以 `funcs` 里找不到它）。
    NoMain,
    /// 步数超限。
    Budget,
    /// 跑到 `Unreachable`（构建 bug：可达的块不该以它终止）。
    Unreachable,
}

impl Exec<'_> {
    /// 建一帧并把它跑到底（`main` / `init` 用；被调方的调用在 `Task 35` 加）。
    fn call(&mut self, func: usize) -> Result<(), Halt> {
        let f = &self.m.funcs[func];
        self.frames.push(SFrame {
            func,
            block: f.entry,
            idx: 0,
            values: Vec::new(),
        });
        self.drive()
    }

    /// 主循环：一直执行到帧栈空。
    fn drive(&mut self) -> Result<(), Halt> {
        while let Some(frame) = self.frames.last() {
            if self.steps >= self.max_steps {
                return Err(Halt::Budget);
            }
            self.steps += 1;
            let f = &self.m.funcs[frame.func];
            let block = &f.blocks[frame.block.index()];
            if frame.idx < block.instrs.len() {
                let _ = &block.instrs[frame.idx];
                self.frames.last_mut().expect("刚判过非空").idx += 1;
                todo!("M6/Task 33：指令执行还没进来（骨架只跑空函数）");
            }
            // 指令走完了 ⇒ 终止符
            match &block.term {
                Term::Return(_) => {
                    self.frames.pop();
                }
                Term::Goto { .. } | Term::If { .. } => {
                    todo!("M6/Task 34：控制流（Goto / If）还没进来")
                }
                Term::Panic(_) => todo!("M6/Task 34：Panic 还没进来"),
                Term::Unreachable => return Err(Halt::Unreachable),
            }
        }
        Ok(())
    }

    /// 收口成 [`Outcome`]。
    fn finish(&mut self, r: Result<(), Halt>) -> Outcome {
        match r {
            Ok(()) => Outcome {
                output: std::mem::take(&mut self.output),
                exit: 0,
                diag: None,
                steps: self.steps,
            },
            Err(Halt::Budget) => Outcome {
                output: std::mem::take(&mut self.output),
                exit: 2,
                diag: Some(format!("步数超限（--max-steps {}）", self.max_steps)),
                steps: self.steps,
            },
            Err(Halt::NoMain) => Outcome {
                output: std::mem::take(&mut self.output),
                exit: 2,
                diag: Some("模块里没有 main（或被拒绝，没进 funcs）".to_string()),
                steps: self.steps,
            },
            Err(Halt::Unreachable) => Outcome {
                output: std::mem::take(&mut self.output),
                exit: 2,
                diag: Some("执行到了 Unreachable 块（构建 bug：可达块不该以它终止）".to_string()),
                steps: self.steps,
            },
        }
    }
}

/// `FuncId` → `funcs` 下标（被拒的函数不在 `funcs` 里，所以可能找不到）。
fn index_of(m: &Module, fid: FuncId) -> Option<usize> {
    m.funcs.iter().position(|f| f.src == fid)
}
