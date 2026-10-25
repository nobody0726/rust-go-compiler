//! 顺序解释器（`M5-design.md` §3.4）。
//!
//! # 一台显式栈机器（`D-M5-1`）
//!
//! 整个执行器只有四个可枚举的状态载体，全部是 `Vec`：
//!
//! | 载体 | 内容 |
//! |---|---|
//! | [`Interp::frames`] | 帧栈；每帧 = 槽数组 + **语句游标栈** + defer 栈 |
//! | [`Interp::eval`] | 表达式求值栈（`Task`） |
//! | [`Interp::values`] | 值栈 |
//! | [`Interp::targets`] | 赋值目标栈（左值先解析、后写） |
//!
//! 三条硬约束：
//!
//! 1. `step_*` **不递归**。表达式靠 `Task` 栈展开，语句靠帧内的
//!    `bodies: Vec<Body>` 游标栈展开 —— 没有一处用 Rust 栈保存执行位置。
//!    （`next_stmt` → `step_*` → `apply` → `exec_stmt` 是单向的；相位推进
//!    在 `step_*` 内部用**循环**完成，不是自调用。）
//! 2. 「等子表达式求值」的中间态在 `Pending` 里，不在 Rust 局部变量里。
//! 3. 帧深 / 求值栈深 / 步数三道计数上限（[`Options`]）。
//!
//! # 为什么语句游标是**栈**而不是单个 PC
//!
//! `if` / `for` / `switch` 的体是嵌套的语句列表，块结束后要回到外层继续。
//! 把「回到外层」编码成一个返回地址就是 Rust 的调用栈换皮；编码成
//! `bodies: Vec<Body>` 就是数据。`break` / `continue` / `fallthrough` 全部
//! 变成「弹到某一层」—— 这也是 M8 挂起时唯一需要保存的东西。
//!
//! # 赋值的两阶段（Go 规范原文）
//!
//! 「先求左侧的下标/指针间接与右侧所有表达式，再从左到右逐个赋值」。
//! 所以 [`Stmt::Assign`] 的执行是：**先**把所有 `Place` 解析成 [`Target`]
//! （这一步会求值 `a[i]` 里的 `i`），**再**求值右侧，**最后**写入。
//! 少了第一阶段，`a[i], i = 1, 2` 会写错位置。
//!
//! # 值的所有权约定
//!
//! 值栈上的每个 `Value` 都**独占**它的数组/结构体单元（读槽/读字段/读元素
//! 一律 [`Value::deep_copy`]，复合字面量新建）。这条约定是「数组赋值是拷贝」
//! 的唯一保证 —— 破坏它不会报错，只会让 `b := a; b[0] = 1` 改到 `a`。

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

use rgoc_const::BasicId;
use rgoc_hir::lower::DISCARD_SLOT;
use rgoc_hir::{
    BinOp, Block, BranchKind, BuiltinKind, Callee, Case, Composite, CompositeItem, Expr, ExprKind,
    FuncId, GlobalId, MakeKind, MethodRef, Place, Program, RangeKind, SlotId, Stmt, TypeCase, UnOp,
};
use rgoc_types::{Type, TypeId, TypeTable, identical, implements, unalias, underlying};

use crate::chan::{ChanObj, CloseOutcome, RecvOutcome, SendOutcome};
use crate::ops::{self, RtError};
use crate::print;
use crate::value::{Cell, Closure, MapObj, NilKind, SliceVal, Value, cell, is_iface, zero_of};

/// 资源上限（`D-M5-12`）。
#[derive(Clone, Debug)]
pub struct Options {
    /// 步数预算（默认 1e8）。
    pub max_steps: u64,
    /// 帧深上限（默认 1000）。
    pub max_frames: usize,
    /// 求值栈深上限（默认 10000）。
    pub max_eval: usize,
    /// goroutine 数上限（默认 10 000；`T-CON-06`）。
    pub max_goroutines: usize,
    /// `select` 多就绪时的选择种子（M8/Task 5；`0` = 源码顺序，`D-M8-3`）。
    ///
    /// 只喂给 [`Shared::rng`]；调度本身的顺序与它无关（`C11` §1）。
    pub seed: u64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            max_steps: 100_000_000,
            max_frames: 1_000,
            max_eval: 10_000,
            max_goroutines: 10_000,
            seed: 0,
        }
    }
}

/// 一次运行的结果。
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    /// 合并输出流（`print` / `println` 与未捕获 panic 的文本）。
    pub output: String,
    /// 退出码：`0` 正常 / `2` 未捕获 panic 或资源超限。
    pub exit: i32,
    /// 诊断（资源超限的原因；未捕获 panic 走 `output`，见 §3.7）。
    pub diag: Option<String>,
    /// 实际消耗的步数。
    pub steps: u64,
}

/// 停机原因。
enum Halt {
    Panic(Value),
    Budget(&'static str),
}

/// 顶层推进阶段（全局初始化 → 各 `init()` → `main`）。
#[derive(Clone, Copy)]
enum TopPhase {
    Globals(usize),
    Inits(usize),
    Main,
    Done,
}

// ───────────────────────── 表达式求值栈 ─────────────────────────

/// 一个待执行的求值步骤。
enum Task<'a> {
    /// 求一个表达式的值，压到值栈。
    Eval(&'a Expr),
    /// 一元运算。
    Unary { op: UnOp, typ: TypeId },
    /// 二元运算。
    Binary { op: BinOp, typ: TypeId },
    /// `&&` / `||` 的短路点（`y` 只在需要时求值）。
    Short { is_and: bool, y: &'a Expr },
    /// 索引读。
    ///
    /// `comma_ok` ⇒ 压 `(元素, bool)` **两个**值（`v, ok := m[k]`）。
    Index { typ: TypeId, comma_ok: bool },
    /// 切片。
    Slice {
        typ: TypeId,
        lo: bool,
        hi: bool,
        max: bool,
    },
    /// 结构体字段读。
    Field { index: usize },
    /// `*p`。
    Deref,
    /// `&T{...}`：把刚求出的值装进一个新 `Cell`，压它的指针。
    AddrOf,
    /// 类型转换 / 接口装箱。
    Convert { to: TypeId, from: TypeId },
    /// 类型断言。
    Assert {
        to: TypeId,
        iface: TypeId,
        comma_ok: bool,
    },
    /// `make`。
    ///
    /// `nargs` 是**实参个数**（`make(chan int)` 是 0，`make([]int, 3, 5)` 是 2）。
    /// 必须显式带着它：靠「栈上还有没有值」去猜会吃掉调用方栈上的值 ——
    /// `f(make(map[string]int))` 里那个 `f` 的实参就是这么被吃掉过。
    Make {
        kind: MakeKind,
        typ: TypeId,
        nargs: usize,
    },
    /// 接收 `<-ch`（M8/Task 15）。
    Recv {
        typ: TypeId,
        comma_ok: bool,
        /// 已经登记过的等待者（`None` ⇒ 还没试过阻塞）。
        ///
        /// 有这一位，「重试」就不需要数次数（`C11` §3.1：`done` 是唯一判据），
        /// 也不会重复登记同一个接收方。
        waiter: Option<crate::chan::WaiterHandle>,
    },
    /// 发送 `ch <- v`（M8/Task 18）。
    ///
    /// 值栈上依次是 `chan`、`value`（`value` 在上）。阻塞时两者**都留在栈上**：
    /// 任务不弹出、值也不动，醒来重跑同一段代码 —— 这就是 `D-M8-5` 的全部内容。
    Send {
        typ: TypeId,
        waiter: Option<crate::chan::WaiterHandle>,
    },
    /// `go f(args)`（M8/Task 24）。
    ///
    /// 与 [`Task::Invoke`] 的差别只有一处：帧建好之后**不压进当前 G**，
    /// 而是搬进一个新 goroutine（`R5`：实参已经由当前 G 求值完）。
    Go {
        target: InvokeTarget<'a>,
        nargs: usize,
        extra: usize,
        dots: bool,
    },
    /// 内建调用。
    Builtin {
        kind: BuiltinKind,
        args: &'a [Expr],
        variadic: bool,
    },
    /// 调用。
    Invoke {
        target: InvokeTarget<'a>,
        nargs: usize,
        extra: usize,
        dots: bool,
    },
    /// 复合字面量。
    Build(&'a Composite),
    /// 方法值（`x.M` 不调用）。
    Bind {
        method: &'a MethodRef,
        recv_typ: TypeId,
    },

    // ── 左值解析（先解析、后赋值，符合 Go 的两阶段语义）──
    /// 把一个 [`Place`] 解析成 [`Target`]。
    ResolvePlace(&'a Place),
    /// 结构体字段（左值）。
    ResolveField { index: usize },
    /// `*p`（左值）。
    ResolveDeref,
    /// 索引（左值；数组元素 / slice 元素 / map 元素）。
    ResolveIndex,
    /// 「尽量求成引用」：可寻址 ⇒ `Ptr(Cell)`，否则求值。
    EvalRef(&'a Expr),
    /// 结构体字段（引用）。
    RefField { index: usize },
    /// 索引（引用）。
    RefIndex,
    /// `Target` → `Value::Ptr`。
    ToPtr,
}

/// 被调方（求值期形态）。
enum InvokeTarget<'a> {
    Func(FuncId),
    Method(FuncId),
    Iface(&'a str),
    Value,
    Builtin(BuiltinKind),
}

/// 赋值目标。
enum Target {
    /// 一个可写单元。
    Cell(Cell),
    /// map 元素（map 元素**不可寻址**，只能整体写入）。
    MapElem {
        map: Rc<RefCell<MapObj>>,
        key: Value,
    },
    /// `_` 或解析失败的黑洞。
    Discard,
}

// ───────────────────────── 语句续体 ─────────────────────────

/// 「等这批表达式求值完，接下来做什么」。
///
/// 全部作用于**栈顶 body**：设置 `pending` 时栈顶一定是设置它的那一层，
/// 被调用的函数有独立的帧（`pending` 在 `call_func` 里被换出/换回），
/// 所以这里不需要带「我是哪个语句」。
enum Pending<'a> {
    Assign {
        op: Option<(BinOp, TypeId)>,
        vbase: usize,
        tbase: usize,
    },
    ShortDecl {
        slots: &'a [SlotId],
        vbase: usize,
    },
    ExprStmt {
        vbase: usize,
    },
    Return {
        vbase: usize,
    },
    Defer {
        callee: &'a Callee,
        extra: usize,
        vbase: usize,
    },
    /// `defer println(...)` —— 内建不是 `Callee` 里的程序实体，单独一支。
    ///
    /// `Stmt::Defer` 的 `call` 既可能是 `ExprKind::Call`（用户函数/方法/函数值），
    /// 也可能是 `ExprKind::Builtin`（`print`/`println`/`panic`/`recover`）。
    /// 后者原先落进 `let ... else { return }`，被**静默丢弃** ——
    /// `print.go` 里 17 个 `defer println(...)` 一行都没打印出来。
    DeferBuiltin {
        kind: BuiltinKind,
        vbase: usize,
    },
    Globals {
        ids: &'a [GlobalId],
        vbase: usize,
    },
    IfCond {
        then: &'a Block,
        els: Option<&'a Stmt>,
    },
    ForCond,
    RangeInit,
    RangeBind,
    SwitchTag,
    SwitchCase,
    TypeSwitchX,
}

// ───────────────────────── 语句游标 ─────────────────────────

/// 语句游标的分相标记。
///
/// 每个 `Body::{If,For,Range,Switch,TypeSwitch}` 自带一个 `phase`，含义按
/// 形态不同而不同，但都遵循「一个 phase 只表示一件事」：
///
/// * `PH_INIT` —— 还没跑 `init`；
/// * `PH_COND` —— 该判条件（`if`/`for`）/ 该取下一个迭代（`range`）；
/// * `PH_BODY` —— 体正在跑，或体刚跑完（`for` 的「跑 post」入口）；
/// * `PH_POST` —— **只由 `continue` 写入**：跳到 `for` 的体末尾。
///
/// `PH_POST` 与 `PH_BODY` 在 `step_for` 里走同一个分支，分开命名是因为它们
/// 的**来路**不同：`PH_BODY` 是「体自然结束」，`PH_POST` 是「`continue`
/// 主动跳到体末尾」。混成一个值会让 `continue` 与「体结束」的语义在 M8
/// 保存/恢复时不可区分。
const PH_INIT: u8 = 0;
const PH_COND: u8 = 1;
const PH_BODY: u8 = 2;
const PH_POST: u8 = 3;

/// 一层语句游标。
enum Body<'a> {
    Seq {
        stmts: &'a [Stmt],
        idx: usize,
        label: Option<&'a str>,
    },
    If(IfState<'a>),
    For(ForState<'a>),
    Range(Box<RangeState<'a>>),
    Switch(SwitchState<'a>),
    TypeSwitch(TypeSwitchState<'a>),
}

struct IfState<'a> {
    init: Option<&'a Stmt>,
    cond: &'a Expr,
    then: &'a Block,
    els: Option<&'a Stmt>,
    phase: u8,
}

struct ForState<'a> {
    init: Option<&'a Stmt>,
    cond: Option<&'a Expr>,
    post: Option<&'a Stmt>,
    body: &'a Block,
    label: Option<&'a str>,
    /// Go 1.22 的 per-iteration 循环变量（见 `Stmt::For::loopvars`）。
    loopvars: &'a [rgoc_hir::SlotId],
    phase: u8,
}

struct RangeState<'a> {
    key: Option<&'a Place>,
    value: Option<&'a Place>,
    x: &'a Expr,
    body: &'a Block,
    kind: RangeKind,
    define: bool,
    label: Option<&'a str>,
    phase: u8,
    iter: Option<RangeIter>,
    kv: Option<(Value, Value)>,
}

struct SwitchState<'a> {
    init: Option<&'a Stmt>,
    tag: Option<&'a Expr>,
    cases: &'a [Case],
    label: Option<&'a str>,
    phase: u8,
    tag_val: Value,
    ci: usize,
    ei: usize,
    default_ci: Option<usize>,
    fallthrough: bool,
}

struct TypeSwitchState<'a> {
    init: Option<&'a Stmt>,
    x: &'a Expr,
    cases: &'a [TypeCase],
    label: Option<&'a str>,
    phase: u8,
}

/// `range` 的迭代器（形态在 lowering 期已定死）。
enum RangeIter {
    Array {
        cells: Rc<Vec<Cell>>,
        idx: usize,
    },
    Slice {
        arr: Rc<Vec<Cell>>,
        off: usize,
        idx: usize,
        len: usize,
    },
    Str {
        s: Rc<str>,
        pos: usize,
    },
    Map {
        entries: Vec<(Value, Value)>,
        idx: usize,
    },
    Int {
        i: i128,
        n: i128,
    },
}

// ───────────────────────── 帧 ─────────────────────────

/// 一个已登记的 defer。
struct Deferred<'a> {
    callee: DeferredCallee<'a>,
    args: Vec<Value>,
}

enum DeferredCallee<'a> {
    Func(FuncId),
    Method { func: FuncId, recv: Value },
    Iface { recv: Value, name: &'a str },
    Value(Value),
    Builtin(BuiltinKind),
}

/// 一帧。
struct Frame<'a> {
    fid: FuncId,
    slots: Vec<Cell>,
    results: Vec<SlotId>,
    bodies: Vec<Body<'a>>,
    deferred: Vec<Deferred<'a>>,
    /// 正在退出（return 或 panic 展开）：先跑 defer，再弹帧。
    exiting: bool,
    /// 本帧是**被 panic 展开**的（而不是正常 return）。
    ///
    /// 与 `exiting` 分开记的理由：panic 展开期间，被 defer 调用的函数会
    /// **正常**跑完（它们也要 return）。如果弹帧时只看「有没有 pending panic」，
    /// 那些正常返回的帧会把展开状态传染给调用者 —— 于是 defer 里调用任何函数
    /// 都会立刻中断后面的语句（`print("C"); bad(); print("D")` 只剩 `C`，
    /// `T-EX-35` 实测）。判据必须是「这一帧自己在展开」。
    unwinding: bool,
    /// 本帧是「被 defer 调用的函数」（`recover` 只在这样的帧里有效）。
    is_defer: bool,
    /// 返回时丢弃结果（defer 的调用用它）。
    drop_result: bool,
    saved_eval: Vec<Task<'a>>,
    saved_values: Vec<Value>,
    saved_targets: Vec<Target>,
    saved_pending: Option<Pending<'a>>,
}

// ───────────────────────── 机器 ─────────────────────────

/// `select` 的伪随机源（`D-M8-3`，M8/Task 5）。
///
/// # 为什么是 xorshift64 而不是引一个 crate
///
/// 只需要「同样的 seed 给同样的序列」+「不同 seed 给不同序列」，两行代码的事。
/// 引 PRNG crate 会把「可回放」这件事的判据交给第三方版本的实现细节 ——
/// 而 `CON-7` 要的恰恰是**逐字节**可回放，升级依赖就可能换序列。
///
/// # `seed = 0` 是**模式**，不是第 0 号种子
///
/// 见 [`Options::seed`]。`0` 时 [`Rng::below`] 恒返回 `0`（= 取源码顺序的
/// 第一个），并且 [`Rng::next_u64`] 会 `debug_assert` —— 谁在源码顺序模式下
/// 抽了随机数，那就是实现 bug，不是「碰巧抽到 0」。
#[derive(Clone, Debug)]
pub(crate) struct Rng {
    /// 原始种子（判「是不是源码顺序模式」用）。
    seed: u64,
    /// xorshift64 的状态（`seed = 0` 时恒为 0，且永不被推进）。
    state: u64,
}

// 消费方是 Task 37 的 `select`；在此之前只有测试读它。
// 这里不写 `#[allow(dead_code)]` 在 `struct` 上，而是逐个方法标，
// 好处是「哪天 `select` 接上了、某个方法仍然没人用」会重新报出来。
#[allow(dead_code, reason = "select 的选择要到 M8/Task 37 才接线")]
impl Rng {
    pub(crate) const fn new(seed: u64) -> Self {
        Self { seed, state: seed }
    }

    /// 源码顺序模式（`seed = 0`）。
    pub(crate) const fn is_source_order(&self) -> bool {
        self.seed == 0
    }

    /// 下一个 64 位随机数（xorshift64）。
    ///
    /// 状态为 0 时 xorshift 会永远停在 0 —— 那不是「随机性差」，是**误用**，
    /// 所以这里直接 `debug_assert`。
    pub(crate) fn next_u64(&mut self) -> u64 {
        debug_assert!(
            self.state != 0,
            "seed=0 是源码顺序模式，不该从这里抽随机数（D-M8-3）"
        );
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// `[0, n)` 里挑一个；`n == 0` 是调用方的 bug。
    ///
    /// 源码顺序模式恒返回 `0`（不抽随机数，也不推进状态）。
    pub(crate) fn below(&mut self, n: usize) -> usize {
        assert!(n > 0, "below(0) 没有定义");
        if self.is_source_order() || n == 1 {
            return 0;
        }
        // 取模偏差在 n 远小于 2^64 时可忽略；这里只需要「seed 真的起作用」。
        usize::try_from(self.next_u64() % (n as u64)).expect("取模结果小于 n")
    }
}

/// **共享状态**：所有 goroutine 看到同一份（`D-M8-1`）。
///
/// 与 [`G`] 的分界线只有一条：**「换一个 goroutine 之后它还该在吗」**。
/// `globals` / `out` / `steps` / `halted` / `methods` 都在，因为它们跨 goroutine；
/// `frames` / `eval` / `values` / `targets` / `pending` 不在，因为它们属于「正在跑的那一个」。
pub struct Shared<'a> {
    prog: &'a Program,
    tt: &'a TypeTable,
    globals: Vec<Cell>,
    out: String,
    steps: u64,
    opts: Options,
    halted: Option<Halt>,
    methods: std::collections::BTreeMap<(String, String), FuncId>,
    /// 全部 goroutine。`None` = 已退出（槽位保留，编号不回收）。
    gs: Vec<Option<G<'a>>>,
    /// 就绪队列（FIFO，`D-M8-2`）。存的是 `gs` 的下标。
    runq: std::collections::VecDeque<usize>,
    /// 下一个 goroutine 编号（只增不减，便于日志与断言）。
    next_g: u64,
    /// `select` 的伪随机源（`D-M8-3`）。归 `Shared`：它是全程序一份的状态，
    /// 跟着某个 G 走会让「同 seed 同结果」在换 goroutine 时失效。
    ///
    /// 现在只被写、还没人读 —— 读它的是 Task 37 的 `select`。
    #[allow(dead_code, reason = "读取方是 M8/Task 37 的 select")]
    rng: Rng,
}

/// **一个 goroutine 的执行状态**（`D-M8-1`）。
///
/// 这五个 `Vec` 就是「保存/恢复一个 goroutine」的全部内容 —— M5 的 `D-M5-1`
/// 把执行位置做成了数据，M8 才有资格谈「挂起」。加字段时问一句：
/// 换一个 goroutine 之后它该不该跟着走？该 ⇒ 放这里；不该 ⇒ 放 [`Shared`]。
struct G<'a> {
    /// 自己的编号（= `Shared.gs` 的下标）。登记 waiter 时要写进 `Waiter.g`。
    id: usize,
    frames: Vec<Frame<'a>>,
    /// 正在展开的 panic 值（`None` = 没在展开）。
    ///
    /// 归 `G` 不归 `Shared`：panic 是**一个 goroutine 的**展开状态。
    /// 未捕获时由 `one_step` 升格成 `Halt::Panic`（那才是全程序的），
    /// 但展开途中它必须跟着这个 goroutine 走。
    panic_val: Option<Value>,
    eval: Vec<Task<'a>>,
    values: Vec<Value>,
    targets: Vec<Target>,
    pending: Option<Pending<'a>>,
    phase: TopPhase,
    /// 是否阻塞在某个等待上。
    ///
    /// 与 `waiters` 的关系：`blocked == true` ⇔ `waiters` 非空 **或者**
    /// 阻塞在一个没有队列可登记的东西上（nil 通道，Task 23）。
    blocked: bool,
    /// 这个 G 当前登记的等待者（`C11` §3.1）。
    ///
    /// 普通 `recv`/`send` 只有一个；`select` 会有一串（Task 30），撤销靠
    /// `token` 把它们从各通道的队列里删掉。唤醒之后**必须**清空 ——
    /// 留着的话下一次阻塞会带着上一轮的 waiter 去撤销。
    waiters: Vec<crate::chan::WaiterHandle>,
}

/// 手写 `Debug`：机器状态是四万个值，逐字段展开只会把日志淹掉。
impl std::fmt::Debug for Shared<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared")
            .field("goroutines", &self.gs.len())
            .field("live", &self.gs.iter().filter(|g| g.is_some()).count())
            .field("steps", &self.steps)
            .field("out_len", &self.out.len())
            .field("halted", &self.halted.is_some())
            .finish()
    }
}

impl std::fmt::Debug for G<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("G")
            .field("frames", &self.frames.len())
            .field("eval", &self.eval.len())
            .field("values", &self.values.len())
            .field("pending", &self.pending.is_some())
            .finish()
    }
}

/// 跑一个程序。
///
/// 签名与 M5 **完全一致** —— `rgoc-driver` 一行都不用改（`M8-plan.md` Task 3）。
#[must_use]
pub fn run(prog: &Program, tt: &TypeTable, opts: Options) -> Outcome {
    let mut sh = Shared::new(prog, tt, opts);
    sh.setup();
    // main goroutine：`TopPhase::Globals(0)` 起 —— 全局初始化是**它的**第一阶段。
    sh.gs.push(Some(G {
        id: 0,
        frames: Vec::new(),
        panic_val: None,
        eval: Vec::new(),
        values: Vec::new(),
        targets: Vec::new(),
        pending: None,
        phase: TopPhase::Globals(0),
        blocked: false,
        waiters: Vec::new(),
    }));
    // main 是第 0 个 goroutine，也是 runq 的第一个元素。
    sh.runq.push_back(0);
    sh.next_g = 1;
    sh.drive();
    sh.finish()
}

impl<'a> Shared<'a> {
    // ───────────── 准备 ─────────────

    /// 建一台**空机器**（没有 goroutine、没有全局）。
    ///
    /// 抽出来是为了让「`Options.seed` 有没有真的进到 `rng`」这件事有一个
    /// 可判的位置（Task 5 的判据落在这里，不用等到 Task 37 的 `select`）。
    fn new(prog: &'a Program, tt: &'a TypeTable, opts: Options) -> Self {
        Self {
            prog,
            tt,
            globals: Vec::new(),
            out: String::new(),
            steps: 0,
            rng: Rng::new(opts.seed),
            opts,
            halted: None,
            methods: std::collections::BTreeMap::new(),
            gs: Vec::new(),
            runq: std::collections::VecDeque::new(),
            next_g: 0,
        }
    }

    fn setup(&mut self) {
        for g in &self.prog.globals {
            self.globals.push(cell(zero_of(self.tt, g.typ)));
        }
        for (i, f) in self.prog.funcs.iter().enumerate() {
            if let Some(base) = &f.method_of {
                self.methods
                    .insert((base.clone(), f.name.clone()), FuncId(i as u32));
            }
        }
    }

    // ───────────── 主循环 ─────────────

    /// 推进整个程序直到结束（`D-M8-2`：当前 goroutine 一直跑，阻塞/退出才换人）。
    ///
    /// Task 3 时 `gs` 里只有 main 一个 —— 循环体就是 [`G::one_step`]，
    /// 语义与 M5 的单循环**逐条对应**（判据：全量测试零改动全绿）。
    /// 推进整个程序直到结束（`D-M8-2`：当前 goroutine 一直跑，阻塞/退出才换人）。
    ///
    /// # 与 M5 单循环的关系
    ///
    /// M5 的循环体原样搬进了 [`G::one_step`]（每个 `continue` → `return true`、
    /// 每个 `return` → `return false`）。这里只是多了一层「谁在跑」：
    /// 从 `runq` 取一个、把它**跑到底**（直到 `one_step` 返回 `false`）、再取下一个。
    ///
    /// 「跑到底」在 Task 4 是对的：还没有任何操作会阻塞（`go` 也还没实现），
    /// 所以一个 goroutine 就是「一直跑到退出」—— 这正是 `D-M8-2` 的「不抢占」。
    fn drive(&mut self) {
        while let Some(id) = self.pick() {
            let mut g = self.gs[id].take().expect("runq 里的 goroutine 必须活着");
            let step = loop {
                match g.one_step(self) {
                    Step::Continue => {}
                    other => break other,
                }
            };
            if self.halted.is_some() {
                // 预算超限 / panic：整个程序停，剩下的 goroutine 直接丢弃（`D-M8-8`）。
                return;
            }
            match step {
                // 阻塞：**不回收、不入队**。它就停在这里，等对端 `wake` 它
                // （`C11` §2.3：唤醒只入队尾，不保证立刻跑）。
                Step::Blocked => {
                    g.blocked = true;
                    self.gs[id] = Some(g);
                }
                _ => {
                    if g.frames.is_empty() && matches!(g.phase, TopPhase::Done) {
                        self.reap(id);
                    } else {
                        self.gs[id] = Some(g);
                    }
                }
            }
        }

        // 队列空了。两种可能：
        //   ① `main` 已返回 ⇒ 程序正常结束（`D-M8-8`：不等其它 goroutine）；
        //   ② 还有人活着但没人可跑 ⇒ 死锁（Task 39 接上 `Halt::Deadlock`）。
        //
        // Task 4 还造不出 ②（没有任何操作会阻塞），所以这里只判 ①；
        // ② 用 `debug_assert` 钉住「到了那一步该做什么」，别让它悄悄变成空转。
        debug_assert!(
            !self.all_blocked() || self.main_done(),
            "全员阻塞且 main 未结束 ⇒ 死锁；Task 39 在这里接 Halt::Deadlock"
        );
        let _ = self.main_done();
    }

    // ───────────── 调度（`C11` §2） ─────────────

    /// 把一个 goroutine 放回就绪队列（`C11` §2.3：只入队尾、清 `blocked`）。
    ///
    /// 幂等**不**成立，也不该成立：同一个 waiter 被唤醒两次就是「重复唤醒」，
    /// 那是调度器的错，会表现为同一个 G 在 `runq` 里出现两次。`Task 27` 的
    /// 不变式会抓它。
    fn wake(&mut self, gid: usize) {
        if let Some(Some(g)) = self.gs.get_mut(gid) {
            g.blocked = false;
        }
        self.runq.push_back(gid);
    }

    /// 从就绪队列取一个 goroutine（FIFO，`C11` §2.1 规则 3）。
    fn pick(&mut self) -> Option<usize> {
        self.runq.pop_front()
    }

    /// 回收一个已退出的 goroutine（槽位保留，编号不回收）。
    fn reap(&mut self, id: usize) {
        self.gs[id] = None;
    }

    /// `main` 是否已经结束（槽位空 ⇒ 结束）。
    fn main_done(&self) -> bool {
        match self.gs.first() {
            None | Some(None) => true,
            Some(Some(g)) => g.frames.is_empty() && matches!(g.phase, TopPhase::Done),
        }
    }

    /// 是否**全员**阻塞（`C11` §5.2 死锁判定的第二条）。
    fn all_blocked(&self) -> bool {
        self.gs.iter().flatten().all(|g| g.blocked)
    }

    fn finish(self) -> Outcome {
        let mut out = self.out;
        let (exit, diag) = match self.halted {
            None => (0, None),
            Some(Halt::Panic(v)) => {
                out.push_str("panic: ");
                out.push_str(&print::panic_text(&v));
                out.push('\n');
                (2, None)
            }
            Some(Halt::Budget(m)) => (2, Some(m.to_string())),
        };
        Outcome {
            output: out,
            exit,
            diag,
            steps: self.steps,
        }
    }
}

/// 一步推进的结局（`D-M8-5`）。
///
/// M5 时只有「继续 / 结束」两态；M8 多出 `Blocked` —— 它是**唯一**会让出
/// CPU 的结局（`C11` §2.1 规则 2：不抢占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    /// 还有事可做，接着跑。
    Continue,
    /// 卡住了：任务留在求值栈上，等对端 `wake`。
    Blocked,
    /// 这个 goroutine 结束了（或整个程序被 halt）。
    Exited,
}

impl<'a> G<'a> {
    /// 推进**一步**。
    ///
    /// 逐行对应 M5 的 `drive()` 循环体：原来的每个 `continue` 变成
    /// `Step::Continue`，每个 `return` 变成 `Step::Exited`。**顺序不能改** ——
    /// 先判退出帧、再求值、再 `resume`、再语句、最后顶层动作。
    fn one_step(&mut self, sh: &mut Shared<'a>) -> Step {
        sh.steps += 1;
        if sh.steps > sh.opts.max_steps {
            sh.halted = Some(Halt::Budget("step budget exceeded"));
            return Step::Exited;
        }
        if sh.halted.is_some() {
            return Step::Exited;
        }
        if self.eval.len() > sh.opts.max_eval {
            sh.halted = Some(Halt::Budget("eval stack depth exceeded"));
            return Step::Exited;
        }
        if let Some(f) = self.frames.last()
            && f.exiting
        {
            self.teardown(sh);
            return Step::Continue;
        }
        if !self.eval.is_empty() {
            return if self.step_eval(sh) {
                Step::Continue
            } else {
                Step::Blocked
            };
        }
        if self.pending.is_some() {
            self.resume(sh);
            return Step::Continue;
        }
        if !self.frames.is_empty() {
            self.next_stmt(sh);
            return Step::Continue;
        }
        if self.next_top(sh) {
            Step::Continue
        } else {
            Step::Exited
        }
    }

    /// 帧栈空时推进一个顶层动作。返回 `false` ⇒ 程序结束。
    fn next_top(&mut self, sh: &mut Shared<'a>) -> bool {
        match self.phase {
            TopPhase::Globals(gi) => {
                if let Some(g) = sh.prog.init_order.get(gi) {
                    self.phase = TopPhase::Globals(gi + 1);
                    let vbase = self.values.len();
                    self.pending = Some(Pending::Globals {
                        ids: &g.globals,
                        vbase,
                    });
                    for e in g.rhs.iter().rev() {
                        self.eval.push(Task::Eval(e));
                    }
                } else {
                    self.phase = TopPhase::Inits(0);
                }
                true
            }
            TopPhase::Inits(i) => {
                if let Some(&fid) = sh.prog.inits.get(i) {
                    self.phase = TopPhase::Inits(i + 1);
                    self.call_func(sh, fid, None, Vec::new(), Vec::new(), false, false, false);
                } else {
                    self.phase = TopPhase::Main;
                }
                true
            }
            TopPhase::Main => {
                self.phase = TopPhase::Done;
                match sh.prog.main {
                    Some(m) => {
                        self.call_func(sh, m, None, Vec::new(), Vec::new(), false, false, false);
                        true
                    }
                    None => false,
                }
            }
            TopPhase::Done => false,
        }
    }

    // ───────────── 求值 ─────────────

    /// 推进求值栈顶的一个任务。返回 `false` ⇒ 这个任务**卡住了**
    /// （任务已经放回栈顶，等对端 `wake` 之后重跑）。
    fn step_eval(&mut self, sh: &mut Shared<'a>) -> bool {
        let Some(task) = self.eval.pop() else {
            return true;
        };
        match task {
            Task::Eval(e) => self.eval_expr(sh, e),
            Task::Unary { op, typ } => {
                let v = self.pop(sh);
                self.values.push(apply_unary(op, v, typ, sh.tt));
            }
            Task::Binary { op, typ } => {
                let y = self.pop(sh);
                let x = self.pop(sh);
                self.apply_binary(sh, op, x, y, typ);
            }
            Task::Short { is_and, y } => {
                let x = self.pop(sh);
                let b = x.as_bool().unwrap_or(false);
                // 短路条件是「左侧已经决定了结果」：
                //   `&&` 左侧 false ⇒ 结果 false（不必求右侧）
                //   `||` 左侧 true  ⇒ 结果 true （不必求右侧）
                // 即 `b != is_and`。写成 `b == is_and` 会把 `true && y` 直接
                // 当 true、把 `false || y` 直接当 false —— 右侧永不求值
                // （`ken/simpbool.go` 的 `panic(22)` 实测）。
                if b != is_and {
                    self.values.push(Value::Bool(b));
                } else {
                    self.eval.push(Task::Eval(y));
                }
            }
            Task::Index { typ, comma_ok } => {
                let idx = self.pop(sh);
                let base = self.pop(sh);
                let r = if comma_ok {
                    self.index_comma_ok(sh, &base, &idx, typ)
                } else {
                    self.index_value(sh, &base, &idx, typ).map(|v| {
                        self.values.push(v);
                    })
                };
                if let Err(e) = r {
                    self.fail(sh, e);
                }
            }
            Task::Slice { typ, lo, hi, max } => {
                let mx = if max { Some(self.pop(sh)) } else { None };
                let h = if hi { Some(self.pop(sh)) } else { None };
                let l = if lo { Some(self.pop(sh)) } else { None };
                let base = self.pop(sh);
                match slice_value(&base, l, h, mx, typ) {
                    Ok(v) => self.values.push(v),
                    Err(e) => self.fail(sh, e),
                }
            }
            Task::Field { index } => {
                let base = self.pop(sh);
                match field_of(&base, index) {
                    Some(v) => self.values.push(v),
                    None => self.fail(sh, RtError::NilDeref),
                }
            }
            Task::Deref => {
                let p = self.pop(sh);
                match p {
                    Value::Ptr(c) => {
                        let v = c.borrow().deep_copy();
                        self.values.push(v);
                    }
                    Value::Nil(_) => self.fail(sh, RtError::NilDeref),
                    other => self.values.push(other),
                }
            }
            Task::AddrOf => {
                // `&T{...}`：新分配一个 `Cell` 装住刚求出的值。
                // 不能复用临时值本身 —— 那个值可能立刻被别处改掉。
                let v = self.pop(sh);
                self.values.push(Value::Ptr(cell(v)));
            }
            Task::Convert { to, from } => {
                let v = self.pop(sh);
                let out = convert(sh.tt, v, to, from);
                self.values.push(out);
            }
            Task::Assert {
                to,
                iface,
                comma_ok,
            } => self.type_assert(sh, to, iface, comma_ok),
            Task::Make { kind, typ, nargs } => self.make(sh, kind, typ, nargs),
            Task::Recv {
                typ,
                comma_ok,
                waiter,
            } => return self.task_recv(sh, typ, comma_ok, waiter),
            Task::Send { typ, waiter } => return self.task_send(sh, typ, waiter),
            Task::Go {
                target,
                nargs,
                extra,
                dots,
            } => return self.do_go(sh, target, nargs, extra, dots),
            Task::Builtin {
                kind,
                args,
                variadic,
            } => self.do_builtin(sh, kind, args, variadic),
            Task::Invoke {
                target,
                nargs,
                extra,
                dots,
            } => self.do_invoke(sh, target, nargs, extra, dots),
            Task::Build(comp) => self.build_composite(sh, comp),
            Task::Bind { method, recv_typ } => self.bind_method(sh, method, recv_typ),

            Task::ResolvePlace(p) => self.resolve_place(sh, p),
            Task::ResolveField { index } => {
                let t = self.pop_target(sh);
                match t {
                    Target::Cell(c) => match field_cell(&c, index) {
                        Some(fc) => self.targets.push(Target::Cell(fc)),
                        None => self.fail(sh, RtError::NilDeref),
                    },
                    other => self.targets.push(other),
                }
            }
            Task::ResolveDeref => {
                let v = self.pop(sh);
                match v {
                    Value::Ptr(c) => self.targets.push(Target::Cell(c)),
                    Value::Nil(_) => self.fail(sh, RtError::NilDeref),
                    _ => self.targets.push(Target::Discard),
                }
            }
            Task::ResolveIndex => {
                let idx = self.pop(sh);
                let base = self.pop(sh);
                match resolve_index(base, idx) {
                    Ok(t) => self.targets.push(t),
                    Err(e) => self.fail(sh, e),
                }
            }
            Task::EvalRef(e) => self.eval_ref(sh, e),
            Task::RefField { index } => {
                let v = self.pop(sh);
                match v {
                    Value::Ptr(c) => match field_cell(&c, index) {
                        Some(fc) => self.values.push(Value::Ptr(fc)),
                        None => self.fail(sh, RtError::NilDeref),
                    },
                    other => self.values.push(other),
                }
            }
            Task::RefIndex => {
                let idx = self.pop(sh);
                let base = self.pop(sh);
                match ref_index(base, idx) {
                    Ok(c) => self.values.push(Value::Ptr(c)),
                    Err(e) => self.fail(sh, e),
                }
            }
            Task::ToPtr => {
                let t = self.pop_target(sh);
                match t {
                    Target::Cell(c) => self.values.push(Value::Ptr(c)),
                    _ => self.values.push(Value::Nil(NilKind::Ptr)),
                }
            }
        }
        true
    }

    fn eval_expr(&mut self, sh: &mut Shared<'a>, e: &'a Expr) {
        match &e.kind {
            ExprKind::Const(c) => {
                let v = const_value(c, e.typ, sh.tt);
                self.values.push(v);
            }
            ExprKind::Nil => self.values.push(nil_of(sh.tt, e.typ)),
            ExprKind::Slot(s) => {
                let fi = self.frames.len() - 1;
                let v = match self.frames[fi].slots.get(s.index()) {
                    Some(c) => c.borrow().deep_copy(),
                    None => Value::Nil(NilKind::Ptr),
                };
                self.values.push(v);
            }
            ExprKind::Global(g) => {
                let v = match sh.globals.get(g.index()) {
                    Some(c) => c.borrow().deep_copy(),
                    None => Value::Nil(NilKind::Ptr),
                };
                self.values.push(v);
            }
            ExprKind::Field { base, index } => {
                self.eval.push(Task::Field { index: *index });
                self.eval.push(Task::Eval(base));
            }
            ExprKind::Index {
                base,
                index,
                comma_ok,
            } => {
                self.eval.push(Task::Index {
                    typ: e.typ,
                    comma_ok: *comma_ok,
                });
                self.eval.push(Task::Eval(index));
                self.eval.push(Task::Eval(base));
            }
            ExprKind::Slice { base, lo, hi, max } => {
                self.eval.push(Task::Slice {
                    typ: e.typ,
                    lo: lo.is_some(),
                    hi: hi.is_some(),
                    max: max.is_some(),
                });
                if let Some(m) = max {
                    self.eval.push(Task::Eval(m));
                }
                if let Some(h) = hi {
                    self.eval.push(Task::Eval(h));
                }
                if let Some(l) = lo {
                    self.eval.push(Task::Eval(l));
                }
                self.eval.push(Task::Eval(base));
            }
            ExprKind::Deref(x) => {
                self.eval.push(Task::Deref);
                self.eval.push(Task::Eval(x));
            }
            ExprKind::Addr(p) => {
                self.eval.push(Task::ToPtr);
                self.eval.push(Task::ResolvePlace(p));
            }
            ExprKind::AddrOf(x) => {
                self.eval.push(Task::AddrOf);
                self.eval.push(Task::Eval(x));
            }
            ExprKind::Unary { op, x } => {
                self.eval.push(Task::Unary {
                    op: *op,
                    typ: e.typ,
                });
                self.eval.push(Task::Eval(x));
            }
            ExprKind::Binary { op, x, y } => match op {
                BinOp::LAnd | BinOp::LOr => {
                    self.eval.push(Task::Short {
                        is_and: *op == BinOp::LAnd,
                        y,
                    });
                    self.eval.push(Task::Eval(x));
                }
                _ => {
                    self.eval.push(Task::Binary {
                        op: *op,
                        typ: e.typ,
                    });
                    self.eval.push(Task::Eval(y));
                    self.eval.push(Task::Eval(x));
                }
            },
            ExprKind::Call {
                callee,
                args,
                variadic,
                spread,
            } => {
                let extra = callee_extra(callee);
                let target = match callee {
                    Callee::Func(f) => InvokeTarget::Func(*f),
                    Callee::Method { func, .. } => InvokeTarget::Method(*func),
                    Callee::Iface { name, .. } => InvokeTarget::Iface(name),
                    Callee::Value(_) => InvokeTarget::Value,
                    Callee::Builtin(k) => InvokeTarget::Builtin(*k),
                };
                // `f(g())`：实参个数是 `g` 的结果个数，不是 `args.len()`（=1）。
                let nargs = if *spread == 0 {
                    args.len()
                } else {
                    usize::from(*spread)
                };
                self.eval.push(Task::Invoke {
                    target,
                    nargs,
                    extra,
                    dots: *variadic,
                });
                self.push_call_parts(sh, callee, args);
            }
            ExprKind::Convert { to, x } => {
                self.eval.push(Task::Convert {
                    to: *to,
                    from: x.typ,
                });
                self.eval.push(Task::Eval(x));
            }
            ExprKind::Composite(comp) => {
                self.eval.push(Task::Build(comp));
                for item in comp.items.iter().rev() {
                    match item {
                        CompositeItem::Value(v) => self.eval.push(Task::Eval(v)),
                        CompositeItem::Key(k, v) => {
                            self.eval.push(Task::Eval(v));
                            self.eval.push(Task::Eval(k));
                        }
                        CompositeItem::Field(_, v) | CompositeItem::Index(_, v) => {
                            self.eval.push(Task::Eval(v));
                        }
                    }
                }
            }
            ExprKind::FuncLit(fid) => {
                let captured = self.capture_cells(sh, *fid);
                self.values.push(Value::Func(Rc::new(Closure {
                    func: *fid,
                    captured,
                    recv: None,
                })));
            }
            ExprKind::Bound { recv, method } => {
                self.eval.push(Task::Bind {
                    method,
                    recv_typ: recv.typ,
                });
                self.eval.push(Task::Eval(recv));
            }
            ExprKind::TypeAssert { x, to, comma_ok } => {
                self.eval.push(Task::Assert {
                    to: *to,
                    iface: x.typ,
                    comma_ok: *comma_ok,
                });
                self.eval.push(Task::Eval(x));
            }
            ExprKind::Make { kind, args } => {
                self.eval.push(Task::Make {
                    kind: *kind,
                    typ: e.typ,
                    nargs: args.len(),
                });
                for a in args.iter().rev() {
                    self.eval.push(Task::Eval(a));
                }
            }
            ExprKind::Recv { ch, comma_ok } => {
                self.eval.push(Task::Recv {
                    typ: e.typ,
                    comma_ok: *comma_ok,
                    waiter: None,
                });
                self.eval.push(Task::Eval(ch));
            }
            ExprKind::New(pointee) => {
                let v = cell(zero_of(sh.tt, *pointee));
                self.values.push(Value::Ptr(v));
            }
            ExprKind::Builtin {
                kind,
                args,
                variadic,
            } => {
                self.eval.push(Task::Builtin {
                    kind: *kind,
                    args,
                    variadic: *variadic,
                });
                for a in args.iter().rev() {
                    self.eval.push(Task::Eval(a));
                }
            }
        }
    }

    /// 压入「被调方侧 + 实参」的求值任务（逆序压 ⇒ 正序求值）。
    fn push_call_parts(&mut self, _sh: &mut Shared<'a>, callee: &'a Callee, args: &'a [Expr]) {
        for a in args.iter().rev() {
            self.eval.push(Task::Eval(a));
        }
        match callee {
            Callee::Func(_) | Callee::Builtin(_) => {}
            Callee::Method { recv, .. } | Callee::Iface { recv, .. } => {
                self.eval.push(Task::Eval(recv));
            }
            Callee::Value(f) => self.eval.push(Task::Eval(f)),
        }
    }

    /// 把表达式**尽量**求成引用：可寻址 ⇒ `Ptr(Cell)`；否则退化成求值。
    fn eval_ref(&mut self, sh: &mut Shared<'a>, e: &'a Expr) {
        match &e.kind {
            ExprKind::Slot(s) => {
                let fi = self.frames.len() - 1;
                match self.frames[fi].slots.get(s.index()) {
                    Some(c) => self.values.push(Value::Ptr(c.clone())),
                    None => self.values.push(Value::Nil(NilKind::Ptr)),
                }
            }
            ExprKind::Global(g) => match sh.globals.get(g.index()) {
                Some(c) => self.values.push(Value::Ptr(c.clone())),
                None => self.values.push(Value::Nil(NilKind::Ptr)),
            },
            ExprKind::Field { base, index } => {
                self.eval.push(Task::RefField { index: *index });
                self.eval.push(Task::EvalRef(base));
            }
            ExprKind::Index { base, index, .. } => {
                self.eval.push(Task::RefIndex);
                self.eval.push(Task::Eval(index));
                self.eval.push(Task::EvalRef(base));
            }
            ExprKind::Deref(x) => self.eval.push(Task::Eval(x)),
            _ => self.eval.push(Task::Eval(e)),
        }
    }

    fn resolve_place(&mut self, sh: &mut Shared<'a>, p: &'a Place) {
        match p {
            Place::Slot(s) => {
                if *s == DISCARD_SLOT {
                    self.targets.push(Target::Discard);
                    return;
                }
                let fi = self.frames.len() - 1;
                match self.frames[fi].slots.get(s.index()) {
                    Some(c) => self.targets.push(Target::Cell(c.clone())),
                    None => self.targets.push(Target::Discard),
                }
            }
            Place::Global(g) => match sh.globals.get(g.index()) {
                Some(c) => self.targets.push(Target::Cell(c.clone())),
                None => self.targets.push(Target::Discard),
            },
            Place::Field { base, index } => {
                self.eval.push(Task::ResolveField { index: *index });
                self.eval.push(Task::ResolvePlace(base));
            }
            Place::Index { base, index } => {
                self.eval.push(Task::ResolveIndex);
                self.eval.push(Task::Eval(index));
                self.eval.push(Task::EvalRef(base));
            }
            Place::Deref(x) => {
                self.eval.push(Task::ResolveDeref);
                self.eval.push(Task::Eval(x));
            }
        }
    }

    // ───────────── 语句推进 ─────────────

    fn next_stmt(&mut self, sh: &mut Shared<'a>) {
        let fi = self.frames.len() - 1;
        if self.frames[fi].bodies.is_empty() {
            self.do_return(sh, Vec::new());
            return;
        }
        let bi = self.frames[fi].bodies.len() - 1;
        let kind = match &self.frames[fi].bodies[bi] {
            Body::Seq { .. } => 0u8,
            Body::If(_) => 1,
            Body::For(_) => 2,
            Body::Range(_) => 3,
            Body::Switch(_) => 4,
            Body::TypeSwitch(_) => 5,
        };
        match kind {
            0 => self.step_seq(sh, fi),
            1 => self.step_if(sh, fi),
            2 => self.step_for(sh, fi),
            3 => self.step_range(sh, fi),
            4 => self.step_switch(sh, fi),
            _ => self.step_type_switch(sh, fi),
        }
    }

    fn step_seq(&mut self, sh: &mut Shared<'a>, fi: usize) {
        let st = {
            let bi = self.frames[fi].bodies.len() - 1;
            let Body::Seq { stmts, idx, .. } = &mut self.frames[fi].bodies[bi] else {
                return;
            };
            let stmts: &'a [Stmt] = stmts;
            if *idx < stmts.len() {
                let s = &stmts[*idx];
                *idx += 1;
                Some(s)
            } else {
                None
            }
        };
        match st {
            Some(s) => self.exec_stmt(sh, s),
            None => {
                self.frames[fi].bodies.pop();
            }
        }
    }

    fn step_if(&mut self, sh: &mut Shared<'a>, fi: usize) {
        let bi = self.frames[fi].bodies.len() - 1;
        let act = {
            let Body::If(s) = &mut self.frames[fi].bodies[bi] else {
                return;
            };
            loop {
                match s.phase {
                    PH_INIT => {
                        s.phase = PH_COND;
                        if let Some(i) = s.init {
                            break Some(Act::Exec(i));
                        }
                    }
                    PH_COND => {
                        s.phase = PH_BODY;
                        break Some(Act::IfCond {
                            cond: s.cond,
                            then: s.then,
                            els: s.els,
                        });
                    }
                    _ => break Some(Act::Pop),
                }
            }
        };
        self.apply(sh, fi, act);
    }

    fn step_for(&mut self, sh: &mut Shared<'a>, fi: usize) {
        let bi = self.frames[fi].bodies.len() - 1;
        let (act, refresh) = {
            let Body::For(s) = &mut self.frames[fi].bodies[bi] else {
                return;
            };
            // `loopvars` 借的是程序（`'a`），不是 `self` —— 先把它拷出来，
            // 下面才能在不结束 `bodies` 可变借用的前提下改 `slots`。
            let loopvars = s.loopvars;
            let mut refresh = false;
            let act = loop {
                match s.phase {
                    PH_INIT => {
                        s.phase = PH_COND;
                        if let Some(i) = s.init {
                            break Some(Act::Exec(i));
                        }
                    }
                    PH_COND => {
                        s.phase = PH_BODY;
                        break Some(match s.cond {
                            Some(c) => Act::ForCond(c),
                            None => Act::Enter(s.body),
                        });
                    }
                    _ => {
                        // `phase == PH_BODY` ⇒ 循环体刚跑完，正要进后置语句。
                        // 这一步是 Go 1.22 per-iteration 语义的落点。
                        refresh = s.phase == PH_BODY;
                        s.phase = PH_COND;
                        if let Some(p) = s.post {
                            break Some(Act::Exec(p));
                        }
                    }
                }
            };
            (act, if refresh { loopvars } else { &[][..] })
        };
        // Go 1.22：每轮给循环变量换一个**新 `Cell`**，初值取上一轮的值。
        // 不换的话循环体里创建的闭包共享同一个 `Cell`，退出后全部看到末值。
        for &slot in refresh {
            if let Some(c) = self.frames[fi].slots.get(slot.index()) {
                let v = c.borrow().deep_copy();
                self.frames[fi].slots[slot.index()] = cell(v);
            }
        }
        self.apply(sh, fi, act);
    }

    fn step_range(&mut self, sh: &mut Shared<'a>, fi: usize) {
        let bi = self.frames[fi].bodies.len() - 1;
        let act = {
            let Body::Range(s) = &mut self.frames[fi].bodies[bi] else {
                return;
            };
            match s.phase {
                PH_INIT => {
                    s.phase = PH_COND;
                    Some(Act::RangeInit(s.x))
                }
                // PH_COND（首轮）与 PH_BODY（体刚结束）做的是**同一件事**：
                // 取下一对 key/value。以前 PH_BODY 走 `Act::RangeBind`，那会
                // 拿空的 `kv` 重新绑一次并把体再进一遍 —— `range` 多跑一轮。
                _ => match range_next(&mut s.iter) {
                    Some((k, v)) => {
                        s.kv = Some((k, v));
                        s.phase = PH_BODY;
                        Some(Act::RangeBind)
                    }
                    None => Some(Act::Pop),
                },
            }
        };
        self.apply(sh, fi, act);
    }

    fn step_switch(&mut self, sh: &mut Shared<'a>, fi: usize) {
        let bi = self.frames[fi].bodies.len() - 1;
        let act = {
            let Body::Switch(s) = &mut self.frames[fi].bodies[bi] else {
                return;
            };
            loop {
                match s.phase {
                    PH_INIT => {
                        s.phase = PH_COND;
                        if let Some(i) = s.init {
                            break Some(Act::Exec(i));
                        }
                    }
                    PH_COND => {
                        s.phase = PH_BODY;
                        break Some(match s.tag {
                            Some(t) => Act::SwitchTag(t),
                            None => {
                                s.tag_val = Value::Bool(true);
                                Act::SwitchScan
                            }
                        });
                    }
                    _ => {
                        if s.fallthrough {
                            s.fallthrough = false;
                            s.ci += 1;
                            s.ei = 0;
                            break Some(match s.cases.get(s.ci) {
                                Some(c) => Act::Enter(&c.body),
                                None => Act::Pop,
                            });
                        }
                        break Some(Act::Pop);
                    }
                }
            }
        };
        self.apply(sh, fi, act);
    }

    fn step_type_switch(&mut self, sh: &mut Shared<'a>, fi: usize) {
        let bi = self.frames[fi].bodies.len() - 1;
        let act = {
            let Body::TypeSwitch(s) = &mut self.frames[fi].bodies[bi] else {
                return;
            };
            loop {
                match s.phase {
                    PH_INIT => {
                        s.phase = PH_COND;
                        if let Some(i) = s.init {
                            break Some(Act::Exec(i));
                        }
                    }
                    PH_COND => {
                        s.phase = PH_BODY;
                        break Some(Act::TypeSwitchX(s.x));
                    }
                    _ => break Some(Act::Pop),
                }
            }
        };
        self.apply(sh, fi, act);
    }

    fn apply(&mut self, sh: &mut Shared<'a>, fi: usize, act: Option<Act<'a>>) {
        match act {
            None => {}
            Some(Act::Pop) => {
                self.frames[fi].bodies.pop();
            }
            Some(Act::Exec(s)) => self.exec_stmt(sh, s),
            Some(Act::Enter(b)) => self.push_seq(sh, fi, b),
            Some(Act::IfCond { cond, then, els }) => {
                self.pending = Some(Pending::IfCond { then, els });
                self.eval.push(Task::Eval(cond));
            }
            Some(Act::ForCond(cond)) => {
                self.pending = Some(Pending::ForCond);
                self.eval.push(Task::Eval(cond));
            }
            Some(Act::RangeInit(x)) => {
                self.pending = Some(Pending::RangeInit);
                self.eval.push(Task::Eval(x));
            }
            Some(Act::RangeBind) => self.start_range_bind(sh, fi),
            Some(Act::SwitchTag(t)) => {
                self.pending = Some(Pending::SwitchTag);
                self.eval.push(Task::Eval(t));
            }
            Some(Act::SwitchScan) => self.switch_scan(sh, fi),
            Some(Act::TypeSwitchX(x)) => {
                self.pending = Some(Pending::TypeSwitchX);
                self.eval.push(Task::Eval(x));
            }
        }
    }

    fn push_seq(&mut self, _sh: &mut Shared<'a>, fi: usize, b: &'a Block) {
        self.frames[fi].bodies.push(Body::Seq {
            stmts: &b.stmts,
            idx: 0,
            label: None,
        });
    }

    fn exec_stmt(&mut self, sh: &mut Shared<'a>, s: &'a Stmt) {
        match s {
            // `go f(...)`（M8/Task 24）：`f` 与**全部实参**在当前 G 求值完，
            // 再把帧搬进新 goroutine（`STM-14` / `T-CON-02`）。
            Stmt::Go { call } => {
                if let ExprKind::Call {
                    callee,
                    args,
                    variadic,
                    spread,
                } = &call.kind
                {
                    let extra = callee_extra(callee);
                    let target = match callee {
                        Callee::Func(f) => InvokeTarget::Func(*f),
                        Callee::Method { func, .. } => InvokeTarget::Method(*func),
                        Callee::Iface { name, .. } => InvokeTarget::Iface(name),
                        Callee::Value(_) => InvokeTarget::Value,
                        Callee::Builtin(k) => InvokeTarget::Builtin(*k),
                    };
                    let nargs = if *spread == 0 {
                        args.len()
                    } else {
                        usize::from(*spread)
                    };
                    self.eval.push(Task::Go {
                        target,
                        nargs,
                        extra,
                        dots: *variadic,
                    });
                    self.push_call_parts(sh, callee, args);
                } else {
                    self.fail(
                        sh,
                        RtError::Message("runtime error: go 的目标不是调用".to_string()),
                    );
                }
            }
            // `ch <- v`（M8/Task 18）：chan 先求值、value 后（与 lowering 的
            // 求值顺序一致）。**不用 `Pending`** —— 阻塞时任务要留在求值栈上
            // （`D-M8-5`），而 `Pending` 是「语句已经跑完、只差提交」的状态。
            Stmt::Send { chan, value } => {
                self.eval.push(Task::Send {
                    typ: chan.typ,
                    waiter: None,
                });
                self.eval.push(Task::Eval(value));
                self.eval.push(Task::Eval(chan));
            }
            Stmt::Select { .. } => todo!("M8/Task 30：select 的解释执行"),
            Stmt::Empty => {}
            Stmt::Assign { lhs, op, rhs } => {
                let vbase = self.values.len();
                let tbase = self.targets.len();
                let op = op.map(|o| (o, rhs.first().map_or_else(invalid_id, |r| r.typ)));
                self.pending = Some(Pending::Assign { op, vbase, tbase });
                for e in rhs.iter().rev() {
                    self.eval.push(Task::Eval(e));
                }
                for p in lhs.iter().rev() {
                    self.eval.push(Task::ResolvePlace(p));
                }
            }
            Stmt::ShortDecl { slots, rhs } => {
                let vbase = self.values.len();
                self.pending = Some(Pending::ShortDecl { slots, vbase });
                for e in rhs.iter().rev() {
                    self.eval.push(Task::Eval(e));
                }
            }
            Stmt::Expr(e) => {
                let vbase = self.values.len();
                self.pending = Some(Pending::ExprStmt { vbase });
                self.eval.push(Task::Eval(e));
            }
            Stmt::If {
                init,
                cond,
                then,
                els,
            } => self.push_body(
                sh,
                Body::If(IfState {
                    init: init.as_deref(),
                    cond,
                    then,
                    els: els.as_deref(),
                    phase: PH_INIT,
                }),
            ),
            Stmt::For {
                init,
                cond,
                post,
                body,
                label,
                loopvars,
            } => self.push_body(
                sh,
                Body::For(ForState {
                    init: init.as_deref(),
                    cond: cond.as_ref(),
                    post: post.as_deref(),
                    body,
                    label: label.as_deref(),
                    loopvars,
                    phase: PH_INIT,
                }),
            ),
            Stmt::Range {
                key,
                value,
                x,
                body,
                kind,
                define,
                label,
            } => self.push_body(
                sh,
                Body::Range(Box::new(RangeState {
                    key: key.as_ref(),
                    value: value.as_ref(),
                    x,
                    body,
                    kind: *kind,
                    define: *define,
                    label: label.as_deref(),
                    phase: PH_INIT,
                    iter: None,
                    kv: None,
                })),
            ),
            Stmt::Switch {
                init,
                tag,
                cases,
                label,
            } => self.push_body(
                sh,
                Body::Switch(SwitchState {
                    init: init.as_deref(),
                    tag: tag.as_ref(),
                    cases,
                    label: label.as_deref(),
                    phase: PH_INIT,
                    tag_val: Value::Bool(false),
                    ci: 0,
                    ei: 0,
                    default_ci: cases.iter().position(|c| c.is_default),
                    fallthrough: false,
                }),
            ),
            Stmt::TypeSwitch {
                init,
                x,
                cases,
                label,
            } => self.push_body(
                sh,
                Body::TypeSwitch(TypeSwitchState {
                    init: init.as_deref(),
                    x,
                    cases,
                    label: label.as_deref(),
                    phase: PH_INIT,
                }),
            ),
            Stmt::Return(exprs) => {
                let vbase = self.values.len();
                self.pending = Some(Pending::Return { vbase });
                for e in exprs.iter().rev() {
                    self.eval.push(Task::Eval(e));
                }
            }
            Stmt::Branch { kind, label } => self.branch(sh, *kind, label.as_deref()),
            Stmt::Labeled { label, stmt } => match &**stmt {
                Stmt::Block(b) => self.push_body(
                    sh,
                    Body::Seq {
                        stmts: &b.stmts,
                        idx: 0,
                        label: Some(label),
                    },
                ),
                other => self.exec_stmt(sh, other),
            },
            Stmt::Block(b) => self.push_body(
                sh,
                Body::Seq {
                    stmts: &b.stmts,
                    idx: 0,
                    label: None,
                },
            ),
            Stmt::Defer { call } => {
                // `variadic` / `spread` 都不用看：defer 的实参是按
                // `vbase..` 整段切走的，多值展开天然成立。
                let vbase = self.values.len();
                match &call.kind {
                    ExprKind::Call { callee, args, .. } => {
                        let extra = callee_extra(callee);
                        self.pending = Some(Pending::Defer {
                            callee,
                            extra,
                            vbase,
                        });
                        self.push_call_parts(sh, callee, args);
                    }
                    ExprKind::Builtin { kind, args, .. } => {
                        // `defer println(...)`：内建没有函数帧，登记成
                        // `DeferredCallee::Builtin`，在拆帧时于**当前帧**执行。
                        // 实参照样**现在**求值（`D-M5-9`：defer 的实参在登记时
                        // 求值，不是执行时）。
                        self.pending = Some(Pending::DeferBuiltin { kind: *kind, vbase });
                        for a in args.iter().rev() {
                            self.eval.push(Task::Eval(a));
                        }
                    }
                    _ => self.fail(
                        sh,
                        RtError::Message("runtime error: defer 的目标不是调用".to_string()),
                    ),
                }
            }
        }
    }

    fn push_body(&mut self, _sh: &mut Shared<'a>, b: Body<'a>) {
        let fi = self.frames.len() - 1;
        self.frames[fi].bodies.push(b);
    }

    // ───────────── 续体 ─────────────

    fn resume(&mut self, sh: &mut Shared<'a>) {
        let Some(p) = self.pending.take() else { return };
        let fi = self.frames.len().saturating_sub(1);
        match p {
            Pending::Assign { op, vbase, tbase } => {
                let targets: Vec<Target> = self.targets.split_off(tbase);
                let vals: Vec<Value> = self.values.split_off(vbase);
                if targets.len() != vals.len() {
                    self.fail(
                        sh,
                        RtError::Message(format!(
                            "runtime error: assignment mismatch: {} variables but {} values",
                            targets.len(),
                            vals.len()
                        )),
                    );
                    return;
                }
                match op {
                    None => {
                        for (t, v) in targets.into_iter().zip(vals) {
                            if let Err(e) = self.write_target(sh, t, v) {
                                self.fail(sh, e);
                                return;
                            }
                        }
                    }
                    Some((op, typ)) => {
                        let (Some(t), Some(v)) =
                            (targets.into_iter().next(), vals.into_iter().next())
                        else {
                            return;
                        };
                        let cur = match read_target(&t) {
                            Ok(c) => c,
                            Err(e) => {
                                self.fail(sh, e);
                                return;
                            }
                        };
                        match ops::bin_arith(op, &cur, &v, typ, sh.tt) {
                            Ok(nv) => {
                                if let Err(e) = self.write_target(sh, t, nv) {
                                    self.fail(sh, e);
                                }
                            }
                            Err(e) => self.fail(sh, e),
                        }
                    }
                }
            }
            Pending::ShortDecl { slots, vbase } => {
                let vals: Vec<Value> = self.values.split_off(vbase);
                for (i, slot) in slots.iter().enumerate() {
                    if *slot == DISCARD_SLOT {
                        continue;
                    }
                    let Some(v) = vals.get(i) else { break };
                    if let Some(c) = self.frames[fi].slots.get(slot.index()) {
                        *c.borrow_mut() = v.clone();
                    }
                }
            }
            Pending::ExprStmt { vbase } => self.values.truncate(vbase),
            Pending::Return { vbase } => {
                let vals = self.values.split_off(vbase);
                self.do_return(sh, vals);
            }
            Pending::Defer {
                callee,
                extra,
                vbase,
            } => {
                let mut vals: Vec<Value> = self.values.split_off(vbase);
                let head = if extra == 1 && !vals.is_empty() {
                    Some(vals.remove(0))
                } else {
                    None
                };
                let dc = match callee {
                    Callee::Func(f) => Some(DeferredCallee::Func(*f)),
                    Callee::Method { func, .. } => head.map(|r| DeferredCallee::Method {
                        func: *func,
                        recv: r,
                    }),
                    Callee::Iface { name, .. } => head.map(|r| DeferredCallee::Iface {
                        recv: r,
                        name: name.as_str(),
                    }),
                    Callee::Value(_) => head.map(DeferredCallee::Value),
                    Callee::Builtin(k) => Some(DeferredCallee::Builtin(*k)),
                };
                if let Some(c) = dc {
                    self.frames[fi].deferred.push(Deferred {
                        callee: c,
                        args: vals,
                    });
                }
            }
            Pending::DeferBuiltin { kind, vbase } => {
                let args: Vec<Value> = self.values.split_off(vbase);
                self.frames[fi].deferred.push(Deferred {
                    callee: DeferredCallee::Builtin(kind),
                    args,
                });
            }
            Pending::Globals { ids, vbase } => {
                let vals: Vec<Value> = self.values.split_off(vbase);
                for (i, g) in ids.iter().enumerate() {
                    if let (Some(v), Some(c)) = (vals.get(i), sh.globals.get(g.index())) {
                        *c.borrow_mut() = v.clone();
                    }
                }
            }
            Pending::IfCond { then, els } => {
                let c = self.pop(sh);
                if c.as_bool().unwrap_or(false) {
                    self.push_seq(sh, fi, then);
                } else if let Some(e) = els {
                    self.exec_stmt(sh, e);
                }
            }
            Pending::ForCond => {
                let c = self.pop(sh);
                if c.as_bool().unwrap_or(false) {
                    let bi = self.frames[fi].bodies.len() - 1;
                    let body = match &self.frames[fi].bodies[bi] {
                        Body::For(s) => Some(s.body),
                        _ => None,
                    };
                    if let Some(b) = body {
                        self.push_seq(sh, fi, b);
                    }
                } else {
                    self.frames[fi].bodies.pop();
                }
            }
            Pending::RangeInit => {
                let v = self.pop(sh);
                let bi = self.frames[fi].bodies.len() - 1;
                if let Body::Range(s) = &mut self.frames[fi].bodies[bi] {
                    s.iter = Some(build_iter(&v, s.kind));
                }
            }
            Pending::RangeBind => self.finish_range_bind(sh, fi),
            Pending::SwitchTag => {
                let v = self.pop(sh);
                let bi = self.frames[fi].bodies.len() - 1;
                if let Body::Switch(s) = &mut self.frames[fi].bodies[bi] {
                    s.tag_val = v;
                }
                self.switch_scan(sh, fi);
            }
            Pending::SwitchCase => {
                let v = self.pop(sh);
                let bi = self.frames[fi].bodies.len() - 1;
                let hit = match &mut self.frames[fi].bodies[bi] {
                    Body::Switch(s) => {
                        let hit = crate::value::go_eq(&s.tag_val, &v);
                        if !hit {
                            s.ei += 1;
                        }
                        hit
                    }
                    _ => false,
                };
                if hit {
                    self.switch_enter(sh, fi);
                } else {
                    self.switch_scan(sh, fi);
                }
            }
            Pending::TypeSwitchX => {
                let v = self.pop(sh);
                self.type_switch_enter(sh, fi, v);
            }
        }
    }

    /// 逐 case、逐表达式地扫 `switch` 的 tag 比较式，直到找到第一条要**求值**的
    /// 表达式，或者所有 case 都试完（退到 `default` / 整个 switch 出栈）。
    ///
    /// 每次调用**恰好**前进一小步然后返回（`D-M5-1`：不在 Rust 栈上循环求值）。
    /// 这里没有外层 `loop` —— 两个出口都是 `return`，写成循环只会让人以为
    /// 它会转第二圈。
    fn switch_scan(&mut self, sh: &mut Shared<'a>, fi: usize) {
        let bi = self.frames[fi].bodies.len() - 1;
        let next = match &mut self.frames[fi].bodies[bi] {
            Body::Switch(s) => loop {
                let Some(c) = s.cases.get(s.ci) else {
                    break ScanStep::Fallback;
                };
                if c.is_default {
                    s.ci += 1;
                    s.ei = 0;
                    continue;
                }
                if s.ei < c.exprs.len() {
                    break ScanStep::Eval(&c.exprs[s.ei]);
                }
                s.ci += 1;
                s.ei = 0;
            },
            _ => return,
        };
        match next {
            ScanStep::Eval(e) => {
                self.pending = Some(Pending::SwitchCase);
                self.eval.push(Task::Eval(e));
            }
            ScanStep::Fallback => {
                let bi = self.frames[fi].bodies.len() - 1;
                let has_default = match &mut self.frames[fi].bodies[bi] {
                    Body::Switch(s) => match s.default_ci {
                        Some(d) => {
                            s.ci = d;
                            s.ei = 0;
                            true
                        }
                        None => false,
                    },
                    _ => return,
                };
                if has_default {
                    self.switch_enter(sh, fi);
                } else {
                    self.frames[fi].bodies.pop();
                }
            }
        }
    }

    fn switch_enter(&mut self, sh: &mut Shared<'a>, fi: usize) {
        let bi = self.frames[fi].bodies.len() - 1;
        let body = match &mut self.frames[fi].bodies[bi] {
            Body::Switch(s) => {
                s.ei = 0;
                s.cases.get(s.ci).map(|c| &c.body)
            }
            _ => return,
        };
        match body {
            Some(b) => self.push_seq(sh, fi, b),
            None => {
                self.frames[fi].bodies.pop();
            }
        }
    }

    fn type_switch_enter(&mut self, sh: &mut Shared<'a>, fi: usize, v: Value) {
        let bi = self.frames[fi].bodies.len() - 1;
        let (dyn_typ, data) = match v {
            Value::Iface { typ, data } => (typ, *data),
            other => (None, other),
        };
        let mut pick: Option<usize> = None;
        let mut concrete = false;
        {
            let Body::TypeSwitch(s) = &self.frames[fi].bodies[bi] else {
                return;
            };
            for (i, c) in s.cases.iter().enumerate() {
                if c.is_default {
                    continue;
                }
                if c.is_nil {
                    if dyn_typ.is_none() {
                        pick = Some(i);
                        break;
                    }
                    continue;
                }
                let hit = dyn_typ.is_some_and(|dt| {
                    c.types.iter().any(|t| {
                        if is_iface(sh.tt, *t) {
                            implements(sh.tt, dt, *t, false).unwrap_or(false)
                        } else {
                            identical(sh.tt, dt, *t).unwrap_or(false)
                        }
                    })
                });
                if hit {
                    concrete = c.types.len() == 1 && !is_iface(sh.tt, c.types[0]);
                    pick = Some(i);
                    break;
                }
            }
            if pick.is_none() {
                pick = s.cases.iter().position(|c| c.is_default);
            }
        }
        let Some(ci) = pick else {
            self.frames[fi].bodies.pop();
            return;
        };
        let bi = self.frames[fi].bodies.len() - 1;
        let (slot, body) = match &self.frames[fi].bodies[bi] {
            Body::TypeSwitch(s) => {
                let c = &s.cases[ci];
                (c.bind, &c.body)
            }
            _ => return,
        };
        if let Some(slot) = slot
            && slot != DISCARD_SLOT
        {
            let bound = if concrete {
                data.clone()
            } else {
                Value::Iface {
                    typ: dyn_typ,
                    data: Box::new(data.clone()),
                }
            };
            if let Some(c) = self.frames[fi].slots.get(slot.index()) {
                *c.borrow_mut() = bound;
            }
        }
        self.push_seq(sh, fi, body);
    }

    fn start_range_bind(&mut self, sh: &mut Shared<'a>, fi: usize) {
        let bi = self.frames[fi].bodies.len() - 1;
        let (key, value, define, kv) = match &mut self.frames[fi].bodies[bi] {
            Body::Range(s) => (s.key, s.value, s.define, s.kv.take()),
            _ => return,
        };
        let (k, v) = kv.unwrap_or((Value::Nil(NilKind::Ptr), Value::Nil(NilKind::Ptr)));
        if define {
            bind_define(&mut self.frames[fi].slots, key, k);
            bind_define(&mut self.frames[fi].slots, value, v);
            self.enter_range_body(sh, fi);
            return;
        }
        // `=` 形态：目标可以是任意左值 ⇒ 走异步解析（先 value 后 key，压栈逆序）
        let bi = self.frames[fi].bodies.len() - 1;
        if let Body::Range(s) = &mut self.frames[fi].bodies[bi] {
            s.kv = Some((k, v));
        }
        self.pending = Some(Pending::RangeBind);
        if let Some(p) = value {
            self.eval.push(Task::ResolvePlace(p));
        }
        if let Some(p) = key {
            self.eval.push(Task::ResolvePlace(p));
        }
    }

    fn finish_range_bind(&mut self, sh: &mut Shared<'a>, fi: usize) {
        let bi = self.frames[fi].bodies.len() - 1;
        let kv = match &mut self.frames[fi].bodies[bi] {
            Body::Range(s) => s.kv.take(),
            _ => None,
        };
        let (k, v) = kv.unwrap_or((Value::Nil(NilKind::Ptr), Value::Nil(NilKind::Ptr)));
        let targets: Vec<Target> = std::mem::take(&mut self.targets);
        let mut it = targets.into_iter();
        if let Some(t) = it.next()
            && let Err(e) = self.write_target(sh, t, k)
        {
            self.fail(sh, e);
            return;
        }
        if let Some(t) = it.next()
            && let Err(e) = self.write_target(sh, t, v)
        {
            self.fail(sh, e);
            return;
        }
        self.enter_range_body(sh, fi);
    }

    fn enter_range_body(&mut self, sh: &mut Shared<'a>, fi: usize) {
        let bi = self.frames[fi].bodies.len() - 1;
        let body = match &self.frames[fi].bodies[bi] {
            Body::Range(s) => Some(s.body),
            _ => None,
        };
        if let Some(b) = body {
            self.push_seq(sh, fi, b);
        }
    }

    fn branch(&mut self, sh: &mut Shared<'a>, kind: BranchKind, label: Option<&str>) {
        let fi = self.frames.len() - 1;
        match kind {
            BranchKind::Goto => {
                let fid = self.frames[fi].fid;
                let Some(&target) = sh
                    .prog
                    .funcs
                    .get(fid.index())
                    .and_then(|f| f.labels.get(label.unwrap_or("")))
                else {
                    return;
                };
                while self.frames[fi].bodies.len() > 1 {
                    self.frames[fi].bodies.pop();
                }
                if let Some(Body::Seq { idx, .. }) = self.frames[fi].bodies.last_mut() {
                    *idx = target;
                }
            }
            BranchKind::Fallthrough => loop {
                let n = self.frames[fi].bodies.len();
                if n == 0 {
                    return;
                }
                if matches!(self.frames[fi].bodies[n - 1], Body::Switch(_)) {
                    if let Body::Switch(s) = &mut self.frames[fi].bodies[n - 1] {
                        s.fallthrough = true;
                        s.phase = PH_BODY;
                    }
                    return;
                }
                self.frames[fi].bodies.pop();
            },
            BranchKind::Break => loop {
                let Some(b) = self.frames[fi].bodies.pop() else {
                    return;
                };
                let hit = match &b {
                    Body::Seq { label: l, .. } => label.is_some() && *l == label,
                    Body::If(_) => false,
                    Body::For(s) => label.is_none() || s.label == label,
                    Body::Range(s) => label.is_none() || s.label == label,
                    Body::Switch(s) => label.is_none() || s.label == label,
                    Body::TypeSwitch(s) => label.is_none() || s.label == label,
                };
                if hit {
                    return;
                }
            },
            BranchKind::Continue => loop {
                let Some(mut b) = self.frames[fi].bodies.pop() else {
                    return;
                };
                let hit = match &mut b {
                    // `for` 的 `continue` 必须回到**体末尾**（先跑 post 再判 cond），
                    // 不是回到 cond：`for i := 0; i <= 10; i++ { … continue … }`
                    // 若直接回 cond，`i++` 永不执行 ⇒ 死循环（`for.go` 实测）。
                    Body::For(s) if label.is_none() || s.label == label => {
                        s.phase = PH_POST;
                        true
                    }
                    // `range` 没有 post，回 cond 就是「取下一个」。
                    Body::Range(s) if label.is_none() || s.label == label => {
                        s.phase = PH_COND;
                        true
                    }
                    _ => false,
                };
                if hit {
                    self.frames[fi].bodies.push(b);
                    return;
                }
            },
        }
    }

    fn do_return(&mut self, _sh: &mut Shared<'a>, vals: Vec<Value>) {
        let fi = self.frames.len() - 1;
        let slots = self.frames[fi].results.clone();
        for (i, slot) in slots.iter().enumerate() {
            let Some(v) = vals.get(i) else { break };
            if let Some(c) = self.frames[fi].slots.get(slot.index()) {
                *c.borrow_mut() = v.clone();
            }
        }
        self.frames[fi].exiting = true;
    }

    // ───────────── 帧的建立与拆除 ─────────────

    #[allow(clippy::too_many_arguments)]
    fn call_func(
        &mut self,
        sh: &mut Shared<'a>,
        fid: FuncId,
        recv: Option<Value>,
        args: Vec<Value>,
        captured: Vec<Cell>,
        is_defer: bool,
        drop_result: bool,
        dots: bool,
    ) {
        if self.frames.len() >= sh.opts.max_frames {
            sh.halted = Some(Halt::Budget("frame depth exceeded"));
            return;
        }
        let prog = sh.prog;
        let tt = sh.tt;
        let Some(func) = prog.funcs.get(fid.index()) else {
            sh.halted = Some(Halt::Budget("frame depth exceeded"));
            return;
        };

        let mut slots: Vec<Cell> = Vec::with_capacity(func.nslots);
        for i in 0..func.nslots {
            slots.push(cell(match func.slot_types.get(i).copied() {
                Some(t) => zero_of(tt, t),
                None => Value::Nil(NilKind::Ptr),
            }));
        }
        for (i, cap) in func.captured.iter().enumerate() {
            if let (Some(c), Some(dst)) = (captured.get(i), slots.get_mut(cap.slot.index())) {
                *dst = c.clone();
            }
        }

        let np = func.params.len();
        let nrecv = usize::from(recv.is_some());
        if let Some(r) = recv
            && let Some(&slot) = func.params.first()
        {
            let want = func.param_types.first().copied();
            if let Some(dst) = slots.get_mut(slot.index()) {
                *dst = cell(adjust_recv(tt, r, want));
            }
        }

        let mut it = args.into_iter();
        let fixed_total = if func.variadic {
            np.saturating_sub(1)
        } else {
            np
        };
        let fixed_args = fixed_total.saturating_sub(nrecv);
        for i in 0..fixed_args {
            if let (Some(v), Some(&slot)) = (it.next(), func.params.get(nrecv + i))
                && let Some(dst) = slots.get_mut(slot.index())
            {
                *dst = cell(v);
            }
        }
        if func.variadic && !dots {
            let rest: Vec<Cell> = it.map(cell).collect();
            let n = rest.len();
            let sv = Value::Slice(SliceVal {
                arr: Rc::new(rest),
                off: 0,
                len: n,
                cap: n,
            });
            if let (Some(&slot), true) = (func.params.last(), np > 0)
                && let Some(dst) = slots.get_mut(slot.index())
            {
                *dst = cell(sv);
            }
        } else {
            for i in fixed_args..np.saturating_sub(nrecv) {
                if let (Some(v), Some(&slot)) = (it.next(), func.params.get(nrecv + i))
                    && let Some(dst) = slots.get_mut(slot.index())
                {
                    *dst = cell(v);
                }
            }
        }

        let frame = Frame {
            fid,
            slots,
            results: func.results.clone(),
            bodies: Vec::new(),
            deferred: Vec::new(),
            exiting: false,
            unwinding: false,
            is_defer,
            drop_result,
            saved_eval: std::mem::take(&mut self.eval),
            saved_values: std::mem::take(&mut self.values),
            saved_targets: std::mem::take(&mut self.targets),
            saved_pending: self.pending.take(),
        };
        self.frames.push(frame);
        let body = &prog.funcs[fid.index()].body;
        let fi = self.frames.len() - 1;
        self.push_seq(sh, fi, body);
    }

    fn teardown(&mut self, sh: &mut Shared<'a>) {
        let Some(fi) = self.frames.len().checked_sub(1) else {
            return;
        };
        if let Some(d) = self.frames[fi].deferred.pop() {
            self.run_deferred(sh, d);
            return;
        }
        let f = self.frames.pop().expect("刚判过非空");
        if f.unwinding && self.panic_val.is_some() {
            if let Some(caller) = self.frames.last_mut() {
                caller.exiting = true;
                caller.unwinding = true;
            } else {
                let v = self.panic_val.take().unwrap_or(Value::Nil(NilKind::Ptr));
                sh.halted = Some(Halt::Panic(v));
            }
            return;
        }
        // 正常返回：恢复调用者的三栈 + 续体
        self.eval = f.saved_eval;
        self.values = f.saved_values;
        self.targets = f.saved_targets;
        self.pending = f.saved_pending;
        if !f.drop_result {
            let vals: Vec<Value> = f
                .results
                .iter()
                .filter_map(|s| f.slots.get(s.index()))
                .map(|c| c.borrow().deep_copy())
                .collect();
            for v in vals {
                self.values.push(v);
            }
        }
    }

    fn run_deferred(&mut self, sh: &mut Shared<'a>, d: Deferred<'a>) {
        match d.callee {
            DeferredCallee::Func(f) => {
                self.call_func(sh, f, None, d.args, Vec::new(), true, true, false);
            }
            DeferredCallee::Method { func, recv } => {
                self.call_func(sh, func, Some(recv), d.args, Vec::new(), true, true, false);
            }
            DeferredCallee::Iface { recv, name } => match self.resolve_iface_method(sh, &recv, name)
            {
                Some((fid, r)) => {
                    self.call_func(sh, fid, Some(r), d.args, Vec::new(), true, true, false);
                }
                None => self.fail(
                    sh,
                    RtError::Message(format!(
                        "runtime error: invalid memory address or nil pointer dereference ({name})"
                    )),
                ),
            },
            DeferredCallee::Value(v) => match v {
                Value::Func(cl) => {
                    self.call_func(
                        sh,
                        cl.func,
                        cl.recv.clone(),
                        d.args,
                        cl.captured.clone(),
                        true,
                        true,
                        false,
                    );
                }
                _ => self.fail(sh, RtError::NilDeref),
            },
            DeferredCallee::Builtin(k) => {
                // 内建不是函数帧 ⇒ 在**当前帧**里执行。
                // `recover` 因此看不到 `is_defer`，`defer recover()` 无效（§3.7）。
                self.invoke_builtin_direct(sh, k, d.args);
            }
        }
    }

    // ───────────── 调用 ─────────────

    /// `go f(args)`：实参已经在**当前** G 求值完（`STM-14`），这里只负责建帧
    /// 并搬进新 goroutine。
    ///
    /// # 为什么要「借 `call_func` 再拆」
    ///
    /// 形参绑定（变参展开、接收者占 `params[0]`、`dots` 语义）是 M5 里最细的
    /// 一段，重写一遍必然与 `call_func` 走偏 —— 那正是「两边一起错、端到端仍绿」
    /// 的经典形态。所以这里让 `call_func` 照常建帧，建完把**调用者的三栈**
    /// 还回去（`call_func` 会把它们 `take` 进帧里），帧本身搬给新 G。
    fn do_go(
        &mut self,
        sh: &mut Shared<'a>,
        target: InvokeTarget<'a>,
        nargs: usize,
        extra: usize,
        dots: bool,
    ) -> bool {
        let total = nargs + extra;
        if self.values.len() < total {
            self.fail(
                sh,
                RtError::Message("runtime error: eval stack underflow".to_string()),
            );
            return true;
        }
        let mut vals = self.values.split_off(self.values.len() - total);
        let head = if extra == 1 && !vals.is_empty() {
            Some(vals.remove(0))
        } else {
            None
        };
        match target {
            InvokeTarget::Func(f) => {
                self.spawn(sh, f, None, vals, Vec::new(), dots);
                true
            }
            InvokeTarget::Method(f) => match head {
                Some(r) => {
                    self.spawn(sh, f, Some(r), vals, Vec::new(), dots);
                    true
                }
                None => {
                    self.fail(sh, RtError::NilDeref);
                    true
                }
            },
            InvokeTarget::Iface(name) => match head {
                Some(r) => match self.resolve_iface_method(sh, &r, name) {
                    Some((f, recv)) => {
                        self.spawn(sh, f, Some(recv), vals, Vec::new(), dots);
                        true
                    }
                    None => {
                        self.fail(
                            sh,
                            RtError::Message(format!(
                                "runtime error: invalid memory address or nil pointer dereference ({name})"
                            )),
                        );
                        true
                    }
                },
                None => {
                    self.fail(sh, RtError::NilDeref);
                    true
                }
            },
            InvokeTarget::Value => match head {
                Some(Value::Func(cl)) => {
                    self.spawn(
                        sh,
                        cl.func,
                        cl.recv.clone(),
                        vals,
                        cl.captured.clone(),
                        dots,
                    );
                    true
                }
                Some(_) | None => {
                    self.fail(sh, RtError::NilDeref);
                    true
                }
            },
            // `go println(1)` 是合法的（内建调用）：内建没有帧可搬，
            // 直接在**当前** G 执行。
            InvokeTarget::Builtin(k) => {
                self.invoke_builtin_direct(sh, k, vals);
                true
            }
        }
    }

    /// 建一个新 goroutine（帧由已求值的实参建，`R5`）。
    fn spawn(
        &mut self,
        sh: &mut Shared<'a>,
        fid: FuncId,
        recv: Option<Value>,
        args: Vec<Value>,
        captured: Vec<Cell>,
        dots: bool,
    ) -> Option<usize> {
        let live = sh.gs.iter().filter(|g| g.is_some()).count();
        if live >= sh.opts.max_goroutines {
            sh.halted = Some(Halt::Budget("goroutine limit exceeded"));
            return None;
        }
        let before = self.frames.len();
        self.call_func(sh, fid, recv, args, captured, false, true, dots);
        if self.frames.len() != before + 1 {
            // `call_func` 自己失败了（帧深超限）—— 它已经写好 `halted`。
            return None;
        }
        let mut frame = self.frames.pop().expect("刚 push 的帧");
        // 把调用者的三栈还回去：`call_func` 为了「返回时恢复现场」把它们
        // `take` 进了帧里，但新 goroutine 不该带着**别人的**续体。
        self.eval = std::mem::take(&mut frame.saved_eval);
        self.values = std::mem::take(&mut frame.saved_values);
        self.targets = std::mem::take(&mut frame.saved_targets);
        self.pending = frame.saved_pending.take();

        let gid = sh.gs.len();
        sh.gs.push(Some(G {
            id: gid,
            frames: vec![frame],
            panic_val: None,
            eval: Vec::new(),
            values: Vec::new(),
            targets: Vec::new(),
            pending: None,
            phase: TopPhase::Done,
            blocked: false,
            waiters: Vec::new(),
        }));
        // `go` **不切走**当前 G（`C11` §2.1）：只把新 G 入队尾。
        sh.runq.push_back(gid);
        Some(gid)
    }

    fn do_invoke(
        &mut self,
        sh: &mut Shared<'a>,
        target: InvokeTarget<'a>,
        nargs: usize,
        extra: usize,
        dots: bool,
    ) {
        let total = nargs + extra;
        if self.values.len() < total {
            self.fail(
                sh,
                RtError::Message("runtime error: eval stack underflow".to_string()),
            );
            return;
        }
        let mut vals = self.values.split_off(self.values.len() - total);
        let head = if extra == 1 && !vals.is_empty() {
            Some(vals.remove(0))
        } else {
            None
        };
        match target {
            InvokeTarget::Func(f) => {
                self.call_func(sh, f, None, vals, Vec::new(), false, false, dots);
            }
            InvokeTarget::Method(f) => match head {
                Some(r) => self.call_func(sh, f, Some(r), vals, Vec::new(), false, false, dots),
                None => self.fail(sh, RtError::NilDeref),
            },
            InvokeTarget::Iface(name) => match head {
                Some(r) => match self.resolve_iface_method(sh, &r, name) {
                    Some((f, recv)) => {
                        self.call_func(sh, f, Some(recv), vals, Vec::new(), false, false, dots);
                    }
                    None => self.fail(sh, RtError::Message(format!(
                        "runtime error: invalid memory address or nil pointer dereference ({name})"
                    ))),
                },
                None => self.fail(sh, RtError::NilDeref),
            },
            InvokeTarget::Value => match head {
                Some(Value::Func(cl)) => {
                    self.call_func(sh,
                        cl.func,
                        cl.recv.clone(),
                        vals,
                        cl.captured.clone(),
                        false,
                        false,
                        dots,
                    );
                }
                Some(_) | None => self.fail(sh, RtError::NilDeref),
            },
            InvokeTarget::Builtin(k) => self.invoke_builtin_direct(sh, k, vals),
        }
    }

    fn resolve_iface_method(
        &self,
        sh: &Shared<'a>,
        recv: &Value,
        name: &str,
    ) -> Option<(FuncId, Value)> {
        let (dt, data) = match recv {
            Value::Iface { typ, data } => ((*typ)?, data.as_ref().clone()),
            _ => return None,
        };
        // 先直接查动态类型自己的方法；查不到再沿**嵌入字段**提升。
        //
        // 为什么解释器也要走一遍提升：接口的动态类型可能是外层结构体，而方法
        // 声明在嵌入字段的类型上（`method5.go` 的 `T1 ⊃ T2 ⊃ *T3 ⊃ *T4`，
        // `M` 在 `T4` 上）。只查 `(base_name(dt), name)` 会失败，然后被当成
        // 「nil 指针解引用」报出去 —— 一个错误的诊断掩盖了真正的原因。
        let (fid, recv_val) = self.resolve_method_deep(sh, dt, &data, name)?;
        let want = sh
            .prog
            .funcs
            .get(fid.index())
            .and_then(|f| f.param_types.first().copied());
        Some((fid, adjust_recv(sh.tt, recv_val, want)))
    }

    /// 在 `t`（或它的嵌入字段链）上找方法，并把接收者**沿链取到位**。
    fn resolve_method_deep(
        &self,
        sh: &Shared<'a>,
        t: TypeId,
        data: &Value,
        name: &str,
    ) -> Option<(FuncId, Value)> {
        if let Some(base) = base_name(sh.tt, t)
            && let Some(fid) = sh.methods.get(&(base, name.to_string()))
        {
            return Some((*fid, data.clone()));
        }
        // 嵌入提升：逐个嵌入字段递归，接收者随之下降一层。
        let mut cur = unalias(sh.tt, t);
        if let Type::Pointer(e) = sh.tt.type_of(cur) {
            cur = unalias(sh.tt, *e);
        }
        let fields = struct_fields_of(sh.tt, cur)?;
        for (i, f) in fields.iter().enumerate() {
            if !f.embedded {
                continue;
            }
            let Some(sub) = field_of(data, i) else {
                continue;
            };
            if let Some(hit) = self.resolve_method_deep(sh, f.typ, &sub, name) {
                return Some(hit);
            }
        }
        None
    }

    fn bind_method(&mut self, sh: &mut Shared<'a>, method: &'a MethodRef, recv_typ: TypeId) {
        let recv = self.pop(sh);
        match method {
            MethodRef::Func(fid) => {
                let want = sh
                    .prog
                    .funcs
                    .get(fid.index())
                    .and_then(|f| f.param_types.first().copied());
                self.values.push(Value::Func(Rc::new(Closure {
                    func: *fid,
                    captured: Vec::new(),
                    recv: Some(adjust_recv(sh.tt, recv, want)),
                })));
            }
            MethodRef::Iface(name) => match self.resolve_iface_method(sh, &recv, name) {
                Some((fid, r)) => self.values.push(Value::Func(Rc::new(Closure {
                    func: fid,
                    captured: Vec::new(),
                    recv: Some(r),
                }))),
                None => self.fail(
                    sh,
                    RtError::Message(format!(
                        "runtime error: method {name} not found on {}",
                        type_name(sh.tt, recv_typ)
                    )),
                ),
            },
        }
    }

    fn capture_cells(&self, sh: &Shared<'a>, fid: FuncId) -> Vec<Cell> {
        let Some(func) = sh.prog.funcs.get(fid.index()) else {
            return Vec::new();
        };
        let Some(frame) = self.frames.last() else {
            return Vec::new();
        };
        func.captured
            .iter()
            .map(|c| {
                frame
                    .slots
                    .get(c.outer_slot.index())
                    .cloned()
                    .unwrap_or_else(|| cell(Value::Nil(NilKind::Ptr)))
            })
            .collect()
    }

    // ───────────── 内建 ─────────────

    /// 没有 `Expr` 信息的调用（defer 里的内建）：只能按值猜类型。
    fn invoke_builtin_direct(&mut self, sh: &mut Shared<'a>, kind: BuiltinKind, args: Vec<Value>) {
        match kind {
            BuiltinKind::Recover => {
                let ok = self.frames.last().map(|f| f.is_defer).unwrap_or(false);
                let v = if ok { self.panic_val.take() } else { None };
                self.values.push(match v {
                    Some(v) => v,
                    None => Value::Iface {
                        typ: None,
                        data: Box::new(Value::Nil(NilKind::Iface)),
                    },
                });
            }
            BuiltinKind::Panic => {
                let v = args
                    .into_iter()
                    .next()
                    .unwrap_or(Value::Nil(NilKind::Iface));
                self.raise(sh, v);
            }
            BuiltinKind::Print | BuiltinKind::Println => {
                for (i, v) in args.iter().enumerate() {
                    if kind == BuiltinKind::Println && i > 0 {
                        sh.out.push(' ');
                    }
                    sh.out
                        .push_str(&print::format_operand(v, sh.tt, invalid_id()));
                }
                if kind == BuiltinKind::Println {
                    sh.out.push('\n');
                }
            }
            _ => {}
        }
    }

    fn do_builtin(
        &mut self,
        sh: &mut Shared<'a>,
        kind: BuiltinKind,
        args: &'a [Expr],
        variadic: bool,
    ) {
        let n = args.len();
        if self.values.len() < n {
            self.fail(
                sh,
                RtError::Message("runtime error: eval stack underflow".to_string()),
            );
            return;
        }
        let vals: Vec<Value> = self.values.split_off(self.values.len() - n);
        let t0 = args.first().map_or_else(invalid_id, |a| a.typ);
        match kind {
            BuiltinKind::Print | BuiltinKind::Println => {
                for (i, v) in vals.iter().enumerate() {
                    if kind == BuiltinKind::Println && i > 0 {
                        sh.out.push(' ');
                    }
                    let t = args.get(i).map_or(t0, |a| a.typ);
                    sh.out.push_str(&print::format_operand(v, sh.tt, t));
                }
                if kind == BuiltinKind::Println {
                    sh.out.push('\n');
                }
            }
            BuiltinKind::Len => {
                let v = vals.into_iter().next().unwrap_or(Value::Int(0));
                self.values.push(Value::Int(len_of(&v) as i128));
            }
            BuiltinKind::Cap => {
                let v = vals.into_iter().next().unwrap_or(Value::Int(0));
                self.values.push(Value::Int(cap_of(&v) as i128));
            }
            BuiltinKind::Append => self.builtin_append(sh, vals, t0, variadic),
            BuiltinKind::Copy => {
                let mut it = vals.into_iter();
                let dst = it.next().unwrap_or(Value::Nil(NilKind::Slice));
                let src = it.next().unwrap_or(Value::Nil(NilKind::Slice));
                match copy_into(dst, &src) {
                    Ok(k) => self.values.push(Value::Int(k as i128)),
                    Err(e) => self.fail(sh, e),
                }
            }
            BuiltinKind::Delete => {
                let mut it = vals.into_iter();
                let m = it.next().unwrap_or(Value::Nil(NilKind::Map));
                let k = it.next().unwrap_or(Value::Nil(NilKind::Ptr));
                if let Value::Map(obj) = m {
                    obj.borrow_mut().remove(&k);
                }
            }
            BuiltinKind::Clear => {
                let v = vals.into_iter().next().unwrap_or(Value::Nil(NilKind::Map));
                match v {
                    Value::Map(obj) => obj.borrow_mut().entries.clear(),
                    Value::Slice(sv) => {
                        for i in 0..sv.len {
                            *sv.arr[sv.off + i].borrow_mut() = Value::Int(0);
                        }
                    }
                    _ => {}
                }
            }
            BuiltinKind::Min | BuiltinKind::Max => {
                let want_max = kind == BuiltinKind::Max;
                let mut it = vals.into_iter();
                let mut best = it.next().unwrap_or(Value::Int(0));
                for v in it {
                    let ord = cmp_values(&best, &v);
                    if (want_max && ord == Ordering::Less)
                        || (!want_max && ord == Ordering::Greater)
                    {
                        best = v;
                    }
                }
                self.values.push(best);
            }
            BuiltinKind::Complex => {
                let mut it = vals.into_iter();
                let re = to_f64(&it.next().unwrap_or(Value::Int(0)));
                let im = to_f64(&it.next().unwrap_or(Value::Int(0)));
                self.values.push(Value::Complex(re, im));
            }
            BuiltinKind::Real => {
                let v = vals.into_iter().next().unwrap_or(Value::Int(0));
                let (re, _) = as_complex(&v);
                self.values.push(Value::Float(re));
            }
            BuiltinKind::Imag => {
                let v = vals.into_iter().next().unwrap_or(Value::Int(0));
                let (_, im) = as_complex(&v);
                self.values.push(Value::Float(im));
            }
            BuiltinKind::Panic => {
                let v = vals
                    .into_iter()
                    .next()
                    .unwrap_or(Value::Nil(NilKind::Iface));
                self.raise(sh, v);
            }
            BuiltinKind::Recover => {
                let ok = self.frames.last().map(|f| f.is_defer).unwrap_or(false);
                let v = if ok { self.panic_val.take() } else { None };
                self.values.push(match v {
                    Some(v) => v,
                    None => Value::Iface {
                        typ: None,
                        data: Box::new(Value::Nil(NilKind::Iface)),
                    },
                });
            }
            BuiltinKind::New => {
                let v = vals.into_iter().next().unwrap_or(Value::Int(0));
                self.values.push(Value::Ptr(cell(v)));
            }
            // `close(c)`（M8/Task 16，`C11` §3.2 close）。
            //
            // 三个 panic 里两个在这里：double close 与 close(nil)。第三个
            // （closed 上 send）在 `Pending::Send` 的分支里。
            BuiltinKind::Close => {
                let v = vals.into_iter().next().unwrap_or(Value::Nil(NilKind::Chan));
                match v {
                    Value::Chan(obj) => {
                        let outcome = obj.borrow_mut().close();
                        match outcome {
                            // 被唤醒的 G 由调度器接手（Task 20 起入 `runq`）；
                            // 单 goroutine 下没有人可唤醒。
                            CloseOutcome::Woken(_) => {}
                            CloseOutcome::AlreadyClosed => self
                                .fail(sh, RtError::Message("close of closed channel".to_string())),
                        }
                    }
                    // 官方是 `plainError("close of nil channel")` —— 不带
                    // `runtime error:` 前缀（`C9` §4.5 的 panic 文本按官方）。
                    Value::Nil(NilKind::Chan) => {
                        self.fail(sh, RtError::Message("close of nil channel".to_string()))
                    }
                    _ => self.fail(sh, RtError::Message("close of non-channel".to_string())),
                }
            }
        }
    }

    fn builtin_append(
        &mut self,
        sh: &mut Shared<'a>,
        mut vals: Vec<Value>,
        slice_typ: TypeId,
        dots: bool,
    ) {
        if vals.is_empty() {
            self.values.push(Value::Nil(NilKind::Slice));
            return;
        }
        let base = vals.remove(0);
        let extra: Vec<Value> = if dots {
            expand_spread(vals.pop())
        } else {
            vals
        };
        match append_value(sh.tt, base, extra, slice_typ) {
            Ok(v) => self.values.push(v),
            Err(e) => self.fail(sh, e),
        }
    }

    // ───────────── 值构造 ─────────────

    fn make(&mut self, sh: &mut Shared<'a>, kind: MakeKind, typ: TypeId, nargs: usize) {
        match kind {
            MakeKind::Slice => {
                // `make([]T, len[, cap])`：**栈顶是最后一个实参**。
                // `make([]int, 3, 5)` ⇒ 栈 `[3, 5]` ⇒ 先弹出的是 `cap = 5`。
                let (len, cap) = match nargs {
                    0 => (0, 0),
                    1 => {
                        let n = self.pop(sh).as_int().unwrap_or(0).max(0) as usize;
                        (n, n)
                    }
                    _ => {
                        let cap = self.pop(sh).as_int().unwrap_or(0).max(0) as usize;
                        let len = self.pop(sh).as_int().unwrap_or(0).max(0) as usize;
                        (len, cap)
                    }
                };
                let len = len.min(cap);
                let elem = slice_elem(sh.tt, typ);
                let cells: Vec<Cell> = (0..cap).map(|_| cell(zero_of(sh.tt, elem))).collect();
                self.values.push(Value::Slice(SliceVal {
                    arr: Rc::new(cells),
                    off: 0,
                    len,
                    cap,
                }));
            }
            MakeKind::Map => {
                // `make(map[K]V[, hint])`：hint 只是个提示，求值完丢掉。
                if nargs >= 1 {
                    let _ = self.pop(sh);
                }
                self.values
                    .push(Value::Map(Rc::new(RefCell::new(MapObj::default()))));
            }
            MakeKind::Chan => {
                // `make(chan T[, cap])`：没有第二个实参 ⇒ 无缓冲（`C11` §3.1）。
                let cap = if nargs >= 1 {
                    self.pop(sh).as_int().unwrap_or(0).max(0) as usize
                } else {
                    0
                };
                let elem = chan_elem_ty(sh.tt, typ);
                let obj = ChanObj::new(elem, cap);
                self.values.push(Value::Chan(Rc::new(RefCell::new(obj))));
            }
        }
    }

    /// 接收 `<-ch`（`C11` §3.2 recv 的 ①→④）。
    ///
    /// 三条纪律（`D-M8-5`）：
    /// 1. **提交前无副作用**：`Blocked` 时值栈一个值都不许动（通道值也留着，
    ///    醒来还要用它）；
    /// 2. **`done` 是唯一判据**：重试先看自己那个 waiter 有没有被对端完成；
    /// 3. 任务卡住时**放回栈顶**，不能弹掉 —— 弹掉就再也回不来了。
    fn task_recv(
        &mut self,
        sh: &mut Shared<'a>,
        typ: TypeId,
        comma_ok: bool,
        waiter: Option<crate::chan::WaiterHandle>,
    ) -> bool {
        // ① 对端已经把值放进我的 `slot`（无缓冲 handoff 的唯一落地方式）。
        if let Some(w) = &waiter
            && w.borrow().done
        {
            let v = w
                .borrow_mut()
                .slot
                .take()
                .unwrap_or_else(|| zero_of(sh.tt, typ));
            self.values.pop(); // 通道值已经用完了
            self.values.push(v);
            if comma_ok {
                self.values.push(Value::Bool(true));
            }
            self.waiters.clear();
            return true;
        }
        // ② 常规尝试（值栈顶是通道，**不弹出**直到成功）。
        let Some(top) = self.values.last().cloned() else {
            self.fail(
                sh,
                RtError::Message("runtime error: receive from non-channel".to_string()),
            );
            return true;
        };
        let Some(obj) = chan_of(&top) else {
            // nil 通道：永久阻塞且**不登记**（没有队列可登记，Task 23）。
            if top.is_nil() {
                self.eval.push(Task::Recv {
                    typ,
                    comma_ok,
                    waiter,
                });
                return false;
            }
            self.fail(
                sh,
                RtError::Message("runtime error: receive from non-channel".to_string()),
            );
            return true;
        };
        let outcome = obj.borrow_mut().try_recv();
        match outcome {
            RecvOutcome::Got { value, woke } => {
                self.values.pop();
                self.values.push(value);
                if comma_ok {
                    self.values.push(Value::Bool(true));
                }
                self.waiters.clear();
                if let Some(g) = woke {
                    sh.wake(g);
                }
                true
            }
            RecvOutcome::GotZeroClosed => {
                self.values.pop();
                self.values.push(zero_of(sh.tt, typ));
                if comma_ok {
                    self.values.push(Value::Bool(false));
                }
                self.waiters.clear();
                true
            }
            RecvOutcome::Blocked => {
                let waiter = match waiter {
                    Some(w) => Some(w),
                    None => {
                        let w = self.park(sh, &obj, false);
                        self.waiters.push(Rc::clone(&w));
                        Some(w)
                    }
                };
                // 放回栈顶：醒来重跑这一支（`done` 或通道状态都会变）。
                self.eval.push(Task::Recv {
                    typ,
                    comma_ok,
                    waiter,
                });
                false
            }
        }
    }

    /// 发送 `ch <- v`（`C11` §3.2 send 的 ①→④）。
    fn task_send(
        &mut self,
        sh: &mut Shared<'a>,
        typ: TypeId,
        waiter: Option<crate::chan::WaiterHandle>,
    ) -> bool {
        // ① 已经登记过：只有两种醒来方式。
        if let Some(w) = &waiter {
            if w.borrow().done {
                // 对端取走了我的值 ⇒ 这次发送完成。
                self.values.pop();
                self.values.pop();
                self.waiters.clear();
                return true;
            }
            // 没被取走 ⇒ 只可能是 `close` 唤醒的（`close` 不置 `done`）。
            // 判据落在通道自己的 `closed` 上，不靠「醒来几次」。
            if let Some(obj) = self.values.iter().rev().nth(1).and_then(chan_of) {
                let closed = obj.borrow().is_closed();
                if closed {
                    self.values.pop();
                    self.values.pop();
                    self.waiters.clear();
                    self.fail(sh, RtError::Message("send on closed channel".to_string()));
                    return true;
                }
            }
            self.eval.push(Task::Send { typ, waiter });
            return false;
        }
        // ② 首次：栈顶是 value，下面是 chan。
        let value = self
            .values
            .last()
            .cloned()
            .unwrap_or(Value::Nil(NilKind::Ptr));
        let Some(obj) = self.values.iter().rev().nth(1).and_then(chan_of) else {
            // nil 通道：永久阻塞且不登记。
            let is_nil = self.values.iter().rev().nth(1).is_some_and(Value::is_nil);
            if is_nil {
                self.eval.push(Task::Send { typ, waiter });
                return false;
            }
            self.fail(
                sh,
                RtError::Message("runtime error: send to non-channel".to_string()),
            );
            return true;
        };
        // 先取 outcome 再 match：`match obj.borrow_mut()...` 会让 `RefMut`
        // 活到整个 match 结束，`Blocked` 分支里再 `borrow_mut` 就炸
        // （`already borrowed`）。
        let outcome = obj.borrow_mut().try_send(&value);
        match outcome {
            SendOutcome::Done { woke } => {
                self.values.pop();
                self.values.pop();
                self.waiters.clear();
                if let Some(g) = woke {
                    sh.wake(g);
                }
                true
            }
            SendOutcome::Closed => {
                self.values.pop();
                self.values.pop();
                self.waiters.clear();
                self.fail(sh, RtError::Message("send on closed channel".to_string()));
                true
            }
            SendOutcome::Blocked => {
                let w = self.park(sh, &obj, true);
                self.waiters.push(Rc::clone(&w));
                self.eval.push(Task::Send {
                    typ,
                    waiter: Some(w),
                });
                false
            }
        }
    }

    /// 造一个 waiter 并登记到通道的对应队列（`C11` §3.2 的 ④）。
    ///
    /// 发送方的 waiter 要带值（`slot = Some(v)`）—— 接收方直接从这个 `slot`
    /// 取，值不经过缓冲。
    fn park(
        &mut self,
        _sh: &mut Shared<'a>,
        obj: &Rc<RefCell<ChanObj>>,
        is_send: bool,
    ) -> crate::chan::WaiterHandle {
        let slot = if is_send {
            self.values.last().cloned()
        } else {
            None
        };
        let w = Rc::new(RefCell::new(crate::chan::Waiter {
            g: self.id,
            token: 0,
            slot,
            done: false,
        }));
        let mut c = obj.borrow_mut();
        if is_send {
            c.park_send(Rc::clone(&w));
        } else {
            c.park_recv(Rc::clone(&w));
        }
        w
    }

    fn build_composite(&mut self, sh: &mut Shared<'a>, comp: &'a Composite) {
        // 栈上值的个数**不是** items 的个数：map 的 `Key(k, v)` 一项压两个值
        // （先 k 后 v，与 `eval_expr` 的压栈顺序一致）。按 items 数取会把
        // 前面的值留在栈上，`m := map[string]int{"a":1}` 的 `m` 就变成 `"a"`。
        let n: usize = comp
            .items
            .iter()
            .map(|it| match it {
                CompositeItem::Key(..) => 2,
                _ => 1,
            })
            .sum();
        if self.values.len() < n {
            self.fail(
                sh,
                RtError::Message("runtime error: eval stack underflow".to_string()),
            );
            return;
        }
        let mut vals = self.values.split_off(self.values.len() - n).into_iter();
        match composite_shape(sh.tt, comp.typ) {
            Shape::Struct(fields) => {
                let cells: Vec<Cell> = fields.iter().map(|f| cell(zero_of(sh.tt, f.typ))).collect();
                for item in &comp.items {
                    let Some(v) = vals.next() else { break };
                    if let CompositeItem::Field(idx, _) = item
                        && let Some(c) = cells.get(*idx)
                    {
                        *c.borrow_mut() = v;
                    }
                }
                self.values.push(Value::Struct(Rc::new(cells)));
            }
            Shape::Seq(elem, len) => {
                let count = match len {
                    Some(n) => n,
                    None => comp
                        .items
                        .iter()
                        .map(|it| match it {
                            CompositeItem::Index(i, _) => i + 1,
                            _ => 0,
                        })
                        .max()
                        .unwrap_or(0)
                        .max(comp.items.len()),
                };
                let mut cells: Vec<Cell> = (0..count).map(|_| cell(zero_of(sh.tt, elem))).collect();
                let mut next = 0usize;
                for item in &comp.items {
                    let Some(v) = vals.next() else { break };
                    let idx = match item {
                        CompositeItem::Index(i, _) => *i,
                        _ => next,
                    };
                    next = idx + 1;
                    if let Some(c) = cells.get_mut(idx) {
                        *c = cell(v);
                    }
                }
                match len {
                    Some(_) => self.values.push(Value::Array(Rc::new(cells))),
                    None => {
                        let cap = cells.len();
                        self.values.push(Value::Slice(SliceVal {
                            arr: Rc::new(cells),
                            off: 0,
                            len: cap,
                            cap,
                        }));
                    }
                }
            }
            Shape::Map => {
                let obj = Rc::new(RefCell::new(MapObj::default()));
                for item in &comp.items {
                    // 顺序必须是 key 先、value 后（`eval_expr` 先压 k 再压 v）。
                    let Some(k) = vals.next() else { break };
                    let Some(v) = vals.next() else { break };
                    if let CompositeItem::Key(_, _) = item {
                        obj.borrow_mut().set(k, v);
                    }
                }
                self.values.push(Value::Map(obj));
            }
            Shape::Other => self.fail(
                sh,
                RtError::Message(
                    "runtime error: composite literal of unsupported type".to_string(),
                ),
            ),
        }
    }

    fn type_assert(&mut self, sh: &mut Shared<'a>, to: TypeId, iface: TypeId, comma_ok: bool) {
        let v = self.pop(sh);
        let (dt, data) = match v {
            Value::Iface { typ, data } => (typ, *data),
            other => {
                self.values.push(other);
                if comma_ok {
                    self.values.push(Value::Bool(true));
                }
                return;
            }
        };
        let ok = match dt {
            None => false,
            Some(dt) => {
                if is_iface(sh.tt, to) {
                    implements(sh.tt, dt, to, false).unwrap_or(false)
                } else {
                    identical(sh.tt, dt, to).unwrap_or(false)
                }
            }
        };
        if ok {
            // 目标是**接口**时，结果是一个新的接口值（动态类型与动态值都不变），
            // 不是裸动态值：`i2 = i1.(I2)` 之后 `i2` 仍是接口，`i2.f()` 走
            // 接口派发。若在这里直接压 `data`，`i2` 就成了 `*S`，
            // 随后按接口调用取接收者拿到 nil（`ken/interfun.go` /
            // `method5.go` 的 `invalid memory address or nil pointer dereference`）。
            let out = if is_iface(sh.tt, to) {
                Value::Iface {
                    typ: dt,
                    data: Box::new(data),
                }
            } else {
                data
            };
            if comma_ok {
                self.values.push(out);
                self.values.push(Value::Bool(true));
            } else {
                self.values.push(out);
            }
            return;
        }
        if comma_ok {
            self.values.push(zero_of(sh.tt, to));
            self.values.push(Value::Bool(false));
            return;
        }
        let got = match dt {
            Some(t) => type_name(sh.tt, t),
            None => "nil".to_string(),
        };
        self.fail(
            sh,
            RtError::Message(format!(
                "interface conversion: {} is {}, not {}",
                type_name(sh.tt, iface),
                got,
                type_name(sh.tt, to)
            )),
        );
    }

    /// 逗号-ok 索引：压 `(元素, 键是否存在)` 两个值。
    ///
    /// 只有 map 索引是双值的（数组/slice/string 的 `v, ok :=` 在 checker 就
    /// 被拒了），所以这里只处理 map；非 map 底座退化成「值 + `true`」，
    /// 让错误以「越界 panic」而不是「值栈错位」的形式暴露。
    fn index_comma_ok(
        &mut self,
        sh: &mut Shared<'a>,
        base: &Value,
        idx: &Value,
        typ: TypeId,
    ) -> Result<(), RtError> {
        match peel_value(base) {
            Value::Map(obj) => {
                let m = obj.borrow();
                match m.get(idx) {
                    Some(v) => {
                        self.values.push(v.deep_copy());
                        self.values.push(Value::Bool(true));
                    }
                    None => {
                        self.values.push(zero_of(sh.tt, typ));
                        self.values.push(Value::Bool(false));
                    }
                }
                Ok(())
            }
            Value::Nil(NilKind::Map) => {
                self.values.push(zero_of(sh.tt, typ));
                self.values.push(Value::Bool(false));
                Ok(())
            }
            other => {
                let v = self.index_value(sh, &other, idx, typ)?;
                self.values.push(v);
                self.values.push(Value::Bool(true));
                Ok(())
            }
        }
    }

    fn index_value(
        &mut self,
        sh: &mut Shared<'a>,
        base: &Value,
        idx: &Value,
        typ: TypeId,
    ) -> Result<Value, RtError> {
        match peel_value(base) {
            Value::Str(s) => {
                let i = idx.as_int().unwrap_or(0);
                let bytes = s.as_bytes();
                let i = bounds(i, bytes.len())?;
                Ok(Value::Int(i128::from(bytes[i])))
            }
            Value::Array(cells) => {
                let i = bounds(idx.as_int().unwrap_or(0), cells.len())?;
                Ok(cells[i].borrow().deep_copy())
            }
            Value::Slice(sv) => {
                let i = bounds(idx.as_int().unwrap_or(0), sv.len)?;
                Ok(sv.arr[sv.off + i].borrow().deep_copy())
            }
            Value::Nil(NilKind::Slice) => Err(RtError::IndexOutOfRange {
                index: idx.as_int().unwrap_or(0),
                len: 0,
            }),
            Value::Map(obj) => {
                let obj = obj.borrow();
                Ok(match obj.get(idx) {
                    Some(v) => v.deep_copy(),
                    None => zero_of(sh.tt, typ),
                })
            }
            Value::Nil(NilKind::Map) => Ok(zero_of(sh.tt, typ)),
            _ => Err(RtError::IndexOutOfRange {
                index: idx.as_int().unwrap_or(0),
                len: 0,
            }),
        }
    }

    // ───────────── 目标读写 ─────────────

    fn write_target(&mut self, _sh: &mut Shared<'a>, t: Target, v: Value) -> Result<(), RtError> {
        match t {
            Target::Cell(c) => {
                *c.borrow_mut() = v;
                Ok(())
            }
            Target::MapElem { map, key } => {
                map.borrow_mut().set(key, v);
                Ok(())
            }
            Target::Discard => Ok(()),
        }
    }

    // ───────────── 栈工具 ─────────────

    fn pop(&mut self, _sh: &mut Shared<'a>) -> Value {
        self.values.pop().unwrap_or(Value::Nil(NilKind::Ptr))
    }

    fn pop_target(&mut self, _sh: &mut Shared<'a>) -> Target {
        self.targets.pop().unwrap_or(Target::Discard)
    }

    fn fail(&mut self, sh: &mut Shared<'a>, e: RtError) {
        self.raise(sh, Value::Str(e.message().into()));
    }

    fn raise(&mut self, sh: &mut Shared<'a>, v: Value) {
        self.panic_val = Some(v);
        self.eval.clear();
        self.values.clear();
        self.targets.clear();
        self.pending = None;
        match self.frames.last_mut() {
            Some(f) => {
                f.exiting = true;
                f.unwinding = true;
            }
            None => {
                let v = self.panic_val.take().unwrap_or(Value::Nil(NilKind::Ptr));
                sh.halted = Some(Halt::Panic(v));
            }
        }
    }

    fn apply_binary(&mut self, sh: &mut Shared<'a>, op: BinOp, x: Value, y: Value, typ: TypeId) {
        match op {
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let r = ops::compare(op, &x, &y).unwrap_or(false);
                self.values.push(Value::Bool(r));
            }
            _ => match ops::bin_arith(op, &x, &y, typ, sh.tt) {
                Ok(v) => self.values.push(v),
                Err(e) => self.fail(sh, e),
            },
        }
    }
}

// ───────────────────────── 语句相位动作 ─────────────────────────

/// `next_stmt` 里「先把借用放掉、再动手」的中转。
enum Act<'a> {
    Pop,
    Exec(&'a Stmt),
    Enter(&'a Block),
    IfCond {
        cond: &'a Expr,
        then: &'a Block,
        els: Option<&'a Stmt>,
    },
    ForCond(&'a Expr),
    RangeInit(&'a Expr),
    RangeBind,
    SwitchTag(&'a Expr),
    SwitchScan,
    TypeSwitchX(&'a Expr),
}

/// `switch_scan` 的下一步。
enum ScanStep<'x> {
    Eval(&'x Expr),
    Fallback,
}

// ───────────────────────── 自由函数 ─────────────────────────

fn invalid_id() -> TypeId {
    TypeId::from_index(0)
}

fn callee_extra(c: &Callee) -> usize {
    match c {
        Callee::Func(_) | Callee::Builtin(_) => 0,
        _ => 1,
    }
}

/// 常量 → 运行时值。
///
/// `float32` 的常量必须**按 f32 舍入**：Go 的 `var a float32 = 1e-10` 存的是
/// 最接近的 f32，不是 f64 的 `1e-10`。若这里原样保留 f64，随后 `a == 1/b`
/// （两侧都是 float32 运算，`bin_arith` 会舍入到 f32）就会判成不等
/// （`literal.go` 的 `f09` / `f13` / `f15` 实测）。
pub(crate) fn const_value(c: &rgoc_hir::types::ConstVal, typ: TypeId, tt: &TypeTable) -> Value {
    use rgoc_hir::types::ConstVal as C;
    let basic = ops::basic_of(tt, typ);
    let f32_ctx = matches!(basic, Some(BasicId::Float32) | Some(BasicId::Complex64));
    let r32 = |v: f64| {
        if f32_ctx { f64::from(v as f32) } else { v }
    };
    match c {
        C::Bool(b) => Value::Bool(*b),
        C::Int(i) => Value::Int(*i),
        C::Float(f) => Value::Float(r32(*f)),
        C::Complex(re, im) => Value::Complex(r32(*re), r32(*im)),
        C::Str(s) => Value::Str(s.as_str().into()),
    }
}

pub(crate) fn nil_of(tt: &TypeTable, typ: TypeId) -> Value {
    match tt.type_of(underlying(tt, unalias(tt, typ))) {
        Type::Map { .. } => Value::Nil(NilKind::Map),
        Type::Slice(_) => Value::Nil(NilKind::Slice),
        Type::Signature(_) => Value::Nil(NilKind::Func),
        Type::Chan { .. } => Value::Nil(NilKind::Chan),
        Type::Interface(_) => Value::Iface {
            typ: None,
            data: Box::new(Value::Nil(NilKind::Iface)),
        },
        _ => Value::Nil(NilKind::Ptr),
    }
}

/// 沿 `Ptr` 链剥到底（Go 的自动解引用）。
fn peel(c: &Cell) -> Cell {
    let mut cur = c.clone();
    for _ in 0..64 {
        let next = {
            let b = cur.borrow();
            match &*b {
                Value::Ptr(inner) => Some(inner.clone()),
                _ => None,
            }
        };
        match next {
            Some(n) => cur = n,
            None => break,
        }
    }
    cur
}

pub(crate) fn peel_value(v: &Value) -> Value {
    let mut cur = v.clone();
    for _ in 0..64 {
        match cur {
            Value::Ptr(c) => cur = c.borrow().clone(),
            other => return other,
        }
    }
    cur
}

fn field_cell(c: &Cell, index: usize) -> Option<Cell> {
    let inner = peel(c);
    let b = inner.borrow();
    match &*b {
        Value::Struct(cells) => cells.get(index).cloned(),
        _ => None,
    }
}

/// 结构体字段表（穿透命名 / 指针 / 别名）。非结构体 ⇒ `None`。
fn struct_fields_of(tt: &TypeTable, t: TypeId) -> Option<Vec<rgoc_types::Field>> {
    let mut cur = unalias(tt, t);
    if let Type::Pointer(e) = tt.type_of(cur) {
        cur = unalias(tt, *e);
    }
    match tt.type_of(underlying(tt, cur)) {
        Type::Struct(sid) => Some(tt.struct_def(*sid).fields.clone()),
        _ => None,
    }
}

fn field_of(v: &Value, index: usize) -> Option<Value> {
    match peel_value(v) {
        Value::Struct(cells) => cells.get(index).map(|c| c.borrow().deep_copy()),
        _ => None,
    }
}

fn resolve_index(base: Value, idx: Value) -> Result<Target, RtError> {
    let base = match base {
        Value::Ptr(c) => peel(&c).borrow().clone(),
        other => other,
    };
    match base {
        Value::Map(obj) => Ok(Target::MapElem { map: obj, key: idx }),
        Value::Nil(NilKind::Map) => Err(RtError::NilMapWrite),
        // `&(*p)[0]`（`p` 是 nil 的 `*[]T` / `*[N]T`）：`eval_ref` 把 `*p`
        // 交给 `RefIndex` 的是**指针本身**而不是它的内容 —— 指针为 nil 时
        // 这里必须响亮失败，否则 `Task::ToPtr` 会把它悄悄变成 nil 指针，
        // `&(*slicep)[0]` 于是「不 panic」（`nilptr2.go` 实测）。
        Value::Nil(NilKind::Ptr) => Err(RtError::NilDeref),
        other => index_target(other, idx),
    }
}

fn index_target(base: Value, idx: Value) -> Result<Target, RtError> {
    match base {
        Value::Array(cells) => {
            let i = idx.as_int().unwrap_or(0);
            match usize::try_from(i).ok().and_then(|i| cells.get(i)) {
                Some(c) => Ok(Target::Cell(c.clone())),
                None => Err(RtError::IndexOutOfRange {
                    index: i,
                    len: cells.len() as i128,
                }),
            }
        }
        Value::Slice(sv) => {
            let i = idx.as_int().unwrap_or(0);
            match usize::try_from(i)
                .ok()
                .filter(|i| *i < sv.len)
                .map(|i| sv.off + i)
                .and_then(|i| sv.arr.get(i))
            {
                Some(c) => Ok(Target::Cell(c.clone())),
                None => Err(RtError::IndexOutOfRange {
                    index: i,
                    len: sv.len as i128,
                }),
            }
        }
        Value::Map(obj) => Ok(Target::MapElem { map: obj, key: idx }),
        // 其余底座（字符串、nil 接口、…）不可赋值 ⇒ 按越界处理。
        _ => Err(RtError::IndexOutOfRange {
            index: idx.as_int().unwrap_or(0),
            len: 0,
        }),
    }
}

fn ref_index(base: Value, idx: Value) -> Result<Cell, RtError> {
    match resolve_index(base, idx)? {
        Target::Cell(c) => Ok(c),
        Target::MapElem { .. } => Err(RtError::Message(
            "runtime error: cannot take the address of map index expression".to_string(),
        )),
        Target::Discard => Err(RtError::NilDeref),
    }
}

fn read_target(t: &Target) -> Result<Value, RtError> {
    match t {
        Target::Cell(c) => Ok(c.borrow().deep_copy()),
        Target::MapElem { map, key } => {
            let m = map.borrow();
            Ok(m.get(key).map_or(Value::Int(0), Value::deep_copy))
        }
        Target::Discard => Ok(Value::Int(0)),
    }
}

pub(crate) fn bounds(i: i128, len: usize) -> Result<usize, RtError> {
    match usize::try_from(i) {
        Ok(i) if i < len => Ok(i),
        _ => Err(RtError::IndexOutOfRange {
            index: i,
            len: len as i128,
        }),
    }
}

pub(crate) fn len_of(v: &Value) -> usize {
    match peel_value(v) {
        Value::Str(s) => s.len(),
        Value::Array(c) => c.len(),
        Value::Slice(sv) => sv.len,
        Value::Map(m) => m.borrow().len(),
        // `len(c)`：缓冲里有多少值（阻塞发送方的值不算，`C11` §3.2）。
        Value::Chan(c) => c.borrow().len(),
        _ => 0,
    }
}

pub(crate) fn cap_of(v: &Value) -> usize {
    match peel_value(v) {
        Value::Array(c) => c.len(),
        Value::Slice(sv) => sv.cap,
        // `cap(c)`：容量；无缓冲 = 0（`C11` §3.2）。
        Value::Chan(c) => c.borrow().cap(),
        _ => 0,
    }
}

pub(crate) fn grow_cap(cap: usize, need: usize) -> usize {
    if cap == 0 {
        return need;
    }
    let mut c = cap;
    while c < need {
        c *= 2;
    }
    c
}

pub(crate) fn copy_into(dst: Value, src: &Value) -> Result<usize, RtError> {
    let (darr, doff, dlen) = match dst {
        Value::Slice(sv) => (sv.arr, sv.off, sv.len),
        _ => return Ok(0),
    };
    let (sarr, soff, slen) = match peel_value(src) {
        Value::Slice(sv) => (sv.arr, sv.off, sv.len),
        Value::Array(c) => {
            let n = c.len();
            (c, 0, n)
        }
        Value::Str(s) => {
            let bytes = s.as_bytes();
            let n = bytes.len().min(dlen);
            for i in 0..n {
                *darr[doff + i].borrow_mut() = Value::Int(i128::from(bytes[i]));
            }
            return Ok(n);
        }
        _ => return Ok(0),
    };
    let n = dlen.min(slen);
    // 重叠时必须按 memmove 语义：先全部读出来
    let snap: Vec<Value> = (0..n)
        .map(|i| sarr[soff + i].borrow().deep_copy())
        .collect();
    for (i, v) in snap.into_iter().enumerate() {
        *darr[doff + i].borrow_mut() = v;
    }
    Ok(n)
}

fn bind_define(slots: &mut [Cell], place: Option<&Place>, v: Value) {
    let Some(Place::Slot(s)) = place else { return };
    if *s == DISCARD_SLOT {
        return;
    }
    if let Some(c) = slots.get_mut(s.index()) {
        *c = cell(v);
    }
}

fn range_next(iter: &mut Option<RangeIter>) -> Option<(Value, Value)> {
    let it = iter.as_mut()?;
    match it {
        RangeIter::Array { cells, idx } => {
            let i = *idx;
            if i >= cells.len() {
                return None;
            }
            *idx += 1;
            Some((Value::Int(i as i128), cells[i].borrow().deep_copy()))
        }
        RangeIter::Slice { arr, off, idx, len } => {
            let i = *idx;
            if i >= *len {
                return None;
            }
            *idx += 1;
            Some((Value::Int(i as i128), arr[*off + i].borrow().deep_copy()))
        }
        RangeIter::Str { s, pos } => {
            if *pos >= s.len() {
                return None;
            }
            let start = *pos;
            let (r, w) = decode_rune(&s.as_bytes()[start..]);
            *pos = start + w;
            Some((Value::Int(start as i128), Value::Int(i128::from(r))))
        }
        RangeIter::Map { entries, idx } => {
            let i = *idx;
            if i >= entries.len() {
                return None;
            }
            *idx += 1;
            let (k, v) = &entries[i];
            Some((k.deep_copy(), v.deep_copy()))
        }
        RangeIter::Int { i, n } => {
            if *i >= *n {
                return None;
            }
            let cur = *i;
            *i += 1;
            Some((Value::Int(cur), Value::Int(cur)))
        }
    }
}

fn build_iter(v: &Value, kind: RangeKind) -> RangeIter {
    match (kind, peel_value(v)) {
        (RangeKind::Array, Value::Array(cells)) => RangeIter::Array { cells, idx: 0 },
        (RangeKind::Slice, Value::Slice(sv)) => RangeIter::Slice {
            arr: sv.arr,
            off: sv.off,
            idx: 0,
            len: sv.len,
        },
        (RangeKind::Str, Value::Str(s)) => RangeIter::Str { s, pos: 0 },
        (RangeKind::Map, Value::Map(m)) => {
            let entries = m.borrow().entries.clone();
            RangeIter::Map { entries, idx: 0 }
        }
        (RangeKind::Int, Value::Int(n)) => RangeIter::Int { i: 0, n },
        _ => RangeIter::Int { i: 0, n: 0 },
    }
}

/// UTF-8 解码一个 rune；坏字节按 `U+FFFD` 宽度 1 处理（与 `range` 一致）。
fn decode_rune(b: &[u8]) -> (u32, usize) {
    let head = b.len().min(4);
    match std::str::from_utf8(&b[..head]) {
        Ok(s) => match s.chars().next() {
            Some(c) => (c as u32, c.len_utf8()),
            None => (0xFFFD, 1),
        },
        Err(e) => match e.valid_up_to() {
            0 => (0xFFFD, 1),
            n => {
                let s = std::str::from_utf8(&b[..n]).unwrap_or("");
                match s.chars().next() {
                    Some(c) => (c as u32, c.len_utf8()),
                    None => (0xFFFD, 1),
                }
            }
        },
    }
}

pub(crate) fn convert(tt: &TypeTable, v: Value, to: TypeId, from: TypeId) -> Value {
    if is_iface(tt, to) {
        if is_iface(tt, from) {
            return v;
        }
        // **typed nil** 不是 nil 接口（`T-EX-29` 的核心）：
        // `var p *T; var i I = p` ⇒ `i != nil`，动态类型是 `*T`、动态值是
        // nil 指针。判据必须看**静态类型**是不是无类型 nil，而不是看值
        // 「长得像 nil」—— 后者会把每个 nil 指针 / nil map / nil slice
        // 装箱成 nil 接口，`val(pt)`（`pt` 为 nil `*T1`）随即在接口分派处
        // 报「nil 指针解引用」（`method.go` 实测）。
        let from_untyped_nil = matches!(
            tt.type_of(unalias(tt, from)),
            Type::Basic(BasicId::UntypedNil)
        );
        if from_untyped_nil || matches!(v, Value::Nil(NilKind::Iface)) {
            return Value::Iface {
                typ: None,
                data: Box::new(Value::Nil(NilKind::Iface)),
            };
        }
        return Value::Iface {
            typ: Some(from),
            data: Box::new(v),
        };
    }
    // `[]byte(s)` / `[]rune(s)`：把字符串**拆**成字节或 rune。
    //
    // 为什么必须在 `basic_of` 之前分出去：`to` 是 slice，`basic_of` 给
    // `None`，于是落到下面那条「不是基本型就原样返回」的兜底 ——
    // `[]rune("é")` 会变成那个字符串本身，`len` 得 2（字节数）而不是 1
    // （`T-EX-16` 实测）。
    if let Value::Str(sv) = &v
        && let Type::Slice(elem) = tt.type_of(underlying(tt, unalias(tt, to)))
    {
        let e = ops::basic_of(tt, *elem);
        if e == Some(BasicId::Uint8) {
            let cells: Vec<Cell> = sv
                .as_bytes()
                .iter()
                .map(|b| cell(Value::Int(i128::from(*b))))
                .collect();
            let n = cells.len();
            return Value::Slice(SliceVal {
                arr: Rc::new(cells),
                off: 0,
                len: n,
                cap: n,
            });
        }
        if e == Some(BasicId::Int32) {
            let cells: Vec<Cell> = sv
                .chars()
                .map(|c| cell(Value::Int(i128::from(u32::from(c)))))
                .collect();
            let n = cells.len();
            return Value::Slice(SliceVal {
                arr: Rc::new(cells),
                off: 0,
                len: n,
                cap: n,
            });
        }
    }
    let Some(b) = ops::basic_of(tt, to) else {
        if v.is_nil() {
            return nil_of(tt, to);
        }
        return v;
    };
    if b.is_integer() {
        let i = to_i128(&v);
        return Value::Int(ops::normalize(i, b));
    }
    if b.is_float() {
        let f = to_f64(&v);
        return Value::Float(if b == BasicId::Float32 {
            f64::from(f as f32)
        } else {
            f
        });
    }
    if b.is_complex() {
        let (re, im) = as_complex(&v);
        return Value::Complex(re, im);
    }
    match b {
        BasicId::String => string_convert(tt, &v, from),
        BasicId::Bool => Value::Bool(v.as_bool().unwrap_or(false)),
        _ => v,
    }
}

/// `append` 的核心（**两套求值器共用一份**，`D-M6-7`）。
///
/// `extra` 已经展开过（`dots` 的处理在 [`expand_spread`]）：这里只管
/// 「够不够容量 / 要不要重新分配」。
pub(crate) fn append_value(
    tt: &TypeTable,
    base: Value,
    extra: Vec<Value>,
    slice_typ: TypeId,
) -> Result<Value, RtError> {
    let elem_typ = slice_elem(tt, slice_typ);
    let (arr, off, len, cap) = match base {
        Value::Slice(sv) => (sv.arr, sv.off, sv.len, sv.cap),
        Value::Nil(_) => (Rc::new(Vec::new()), 0, 0, 0),
        other => {
            return Err(RtError::Message(format!(
                "runtime error: append to non-slice {}",
                print::format_operand(&other, tt, slice_typ)
            )));
        }
    };
    let need = len + extra.len();
    if need <= cap {
        // 容量够 ⇒ 原地写进底层数组（与 M5 的语义一致：共享底层数组）
        for (i, v) in extra.into_iter().enumerate() {
            let idx = off + len + i;
            if let Some(c) = arr.get(idx) {
                *c.borrow_mut() = v;
            }
        }
        return Ok(Value::Slice(SliceVal {
            arr,
            off,
            len: need,
            cap,
        }));
    }
    let newcap = grow_cap(cap, need);
    let mut cells: Vec<Cell> = Vec::with_capacity(newcap);
    for i in 0..newcap {
        if i < len {
            cells.push(cell(arr[off + i].borrow().deep_copy()));
        } else if i - len < extra.len() {
            cells.push(cell(extra[i - len].clone()));
        } else {
            cells.push(cell(zero_of(tt, elem_typ)));
        }
    }
    Ok(Value::Slice(SliceVal {
        arr: Rc::new(cells),
        off: 0,
        len: need,
        cap: newcap,
    }))
}

/// `xs...` 的展开：`append(s, xs...)` / `println(xs...)` / `append(b, "bar"...)`。
///
/// 字符串展开成字节（Go 规范：`append([]byte, string...)` 合法）。
pub(crate) fn expand_spread(v: Option<Value>) -> Vec<Value> {
    match v {
        Some(Value::Slice(sv)) => (0..sv.len)
            .map(|i| sv.arr[sv.off + i].borrow().deep_copy())
            .collect(),
        Some(Value::Array(cells)) => cells.iter().map(|c| c.borrow().deep_copy()).collect(),
        Some(Value::Str(s)) => s.bytes().map(|b| Value::Int(i128::from(b))).collect(),
        Some(Value::Nil(_)) | None => Vec::new(),
        Some(other) => vec![other],
    }
}

pub(crate) fn slice_value(
    base: &Value,
    lo: Option<Value>,
    hi: Option<Value>,
    max: Option<Value>,
    _typ: TypeId,
) -> Result<Value, RtError> {
    let as_usize = |v: &Option<Value>, dflt: usize| -> i128 {
        v.as_ref().and_then(Value::as_int).unwrap_or(dflt as i128)
    };
    match peel_value(base) {
        Value::Str(s) => {
            let n = s.len();
            let l = as_usize(&lo, 0);
            let h = as_usize(&hi, n);
            if l < 0 || h < l || h as usize > n {
                return Err(RtError::SliceBounds {
                    what: "string",
                    val: h,
                    max: n as i128,
                });
            }
            let out = &s.as_bytes()[l as usize..h as usize];
            Ok(Value::Str(String::from_utf8_lossy(out).into_owned().into()))
        }
        Value::Array(cells) => {
            let n = cells.len();
            let l = as_usize(&lo, 0);
            let h = as_usize(&hi, n);
            let c = as_usize(&max, n);
            if l < 0 || h < l || c < h || c as usize > n {
                return Err(RtError::SliceBounds {
                    what: "array",
                    val: h,
                    max: n as i128,
                });
            }
            Ok(Value::Slice(SliceVal {
                arr: cells,
                off: l as usize,
                len: (h - l) as usize,
                cap: (c - l) as usize,
            }))
        }
        Value::Slice(sv) => {
            let l = as_usize(&lo, 0);
            let h = as_usize(&hi, sv.len);
            let c = as_usize(&max, sv.cap);
            if l < 0 || h < l || c < h || c as usize > sv.cap {
                return Err(RtError::SliceBounds {
                    what: "slice",
                    val: h,
                    max: sv.cap as i128,
                });
            }
            Ok(Value::Slice(SliceVal {
                arr: sv.arr,
                off: sv.off + l as usize,
                len: (h - l) as usize,
                cap: (c - l) as usize,
            }))
        }
        Value::Nil(NilKind::Slice) => {
            let l = as_usize(&lo, 0);
            let h = as_usize(&hi, 0);
            if l != 0 || h != 0 {
                return Err(RtError::SliceBounds {
                    what: "slice",
                    val: h,
                    max: 0,
                });
            }
            Ok(Value::Nil(NilKind::Slice))
        }
        _ => Err(RtError::SliceBounds {
            what: "slice",
            val: 0,
            max: 0,
        }),
    }
}

pub(crate) fn apply_unary(op: UnOp, v: Value, typ: TypeId, tt: &TypeTable) -> Value {
    let b = ops::basic_of(tt, typ);
    match op {
        UnOp::Pos => v,
        UnOp::Neg => match v {
            Value::Int(i) => Value::Int(match b {
                Some(b) => ops::normalize(i.wrapping_neg(), b),
                None => i.wrapping_neg(),
            }),
            Value::Float(f) => Value::Float(-f),
            Value::Complex(r, i) => Value::Complex(-r, -i),
            other => other,
        },
        UnOp::Not => Value::Bool(!v.as_bool().unwrap_or(false)),
        UnOp::BitNot => match v {
            Value::Int(i) => Value::Int(match b {
                Some(b) => ops::normalize(!i, b),
                None => !i,
            }),
            other => other,
        },
        UnOp::Recv => v,
    }
}

pub(crate) fn cmp_values(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Str(x), Value::Str(y)) => x.as_bytes().cmp(y.as_bytes()),
        _ => to_f64(a).partial_cmp(&to_f64(b)).unwrap_or(Ordering::Equal),
    }
}

fn to_i128(v: &Value) -> i128 {
    match v {
        Value::Int(i) => *i,
        Value::Float(f) => *f as i128,
        Value::Bool(b) => i128::from(*b),
        Value::Str(s) => s.parse::<i128>().unwrap_or(0),
        _ => 0,
    }
}

pub(crate) fn to_f64(v: &Value) -> f64 {
    match v {
        Value::Float(f) => *f,
        Value::Int(i) => *i as f64,
        Value::Complex(r, _) => *r,
        _ => 0.0,
    }
}

pub(crate) fn as_complex(v: &Value) -> (f64, f64) {
    match v {
        Value::Complex(r, i) => (*r, *i),
        Value::Float(f) => (*f, 0.0),
        Value::Int(i) => (*i as f64, 0.0),
        _ => (0.0, 0.0),
    }
}

pub(crate) fn slice_elem(tt: &TypeTable, t: TypeId) -> TypeId {
    match tt.type_of(underlying(tt, unalias(tt, t))) {
        Type::Slice(e) => *e,
        Type::Array { elem, .. } => *elem,
        _ => t,
    }
}

/// `chan T` 的 `T`（拿不到就原样返回，调用方只在 `make` 里用）。
pub(crate) fn chan_elem_ty(tt: &TypeTable, t: TypeId) -> TypeId {
    match tt.type_of(underlying(tt, unalias(tt, t))) {
        Type::Chan { elem, .. } => *elem,
        _ => t,
    }
}

/// 从值里取通道句柄（`nil` 通道 ⇒ `None`，那是 Task 23 的「永久阻塞」）。
pub(crate) fn chan_of(v: &Value) -> Option<Rc<RefCell<ChanObj>>> {
    match v {
        Value::Chan(c) => Some(Rc::clone(c)),
        _ => None,
    }
}

enum Shape {
    Struct(Vec<rgoc_types::Field>),
    Seq(TypeId, Option<usize>),
    Map,
    Other,
}

fn composite_shape(tt: &TypeTable, typ: TypeId) -> Shape {
    let mut cur = unalias(tt, typ);
    if let Type::Pointer(e) = tt.type_of(cur) {
        cur = unalias(tt, *e);
    }
    match tt.type_of(underlying(tt, cur)) {
        Type::Struct(sid) => Shape::Struct(tt.struct_def(*sid).fields.clone()),
        Type::Array { len, elem } => {
            let n = len.map(|id| match tt.consts().get(id) {
                rgoc_const::Const::Int(b) => b.to_dec().parse::<usize>().unwrap_or(0),
                _ => 0,
            });
            Shape::Seq(*elem, n)
        }
        Type::Slice(elem) => Shape::Seq(*elem, None),
        Type::Map { .. } => Shape::Map,
        _ => Shape::Other,
    }
}

/// 动态类型的基名（穿透指针；命名类型 ⇒ 名字）。
fn base_name(tt: &TypeTable, t: TypeId) -> Option<String> {
    let mut cur = unalias(tt, t);
    if let Type::Pointer(e) = tt.type_of(cur) {
        cur = unalias(tt, *e);
    }
    match tt.type_of(cur) {
        Type::Named(nid) => Some(tt.named_def(*nid).name.clone()),
        _ => None,
    }
}

/// 接收者按方法声明补齐（解释器侧兜底；lowering 已尽量做对）。
fn adjust_recv(tt: &TypeTable, v: Value, want: Option<TypeId>) -> Value {
    let Some(want) = want else { return v };
    let want_ptr = matches!(tt.type_of(unalias(tt, want)), Type::Pointer(_));
    if !want_ptr && let Value::Ptr(c) = &v {
        return peel(c).borrow().deep_copy();
    }
    v
}

fn string_convert(tt: &TypeTable, v: &Value, from: TypeId) -> Value {
    match v {
        Value::Str(_) => v.clone(),
        Value::Int(i) => {
            let c = u32::try_from(*i).ok().and_then(char::from_u32);
            Value::Str(match c {
                Some(c) => c.to_string().into(),
                None => "\u{FFFD}".into(),
            })
        }
        Value::Slice(sv) => {
            let elem = slice_elem(tt, from);
            let is_rune = ops::basic_of(tt, elem) == Some(BasicId::Int32);
            if is_rune {
                let mut s = String::new();
                for i in 0..sv.len {
                    let n = sv.arr[sv.off + i].borrow().as_int().unwrap_or(0);
                    s.push(
                        u32::try_from(n)
                            .ok()
                            .and_then(char::from_u32)
                            .unwrap_or('\u{FFFD}'),
                    );
                }
                Value::Str(s.into())
            } else {
                let mut bytes = Vec::with_capacity(sv.len);
                for i in 0..sv.len {
                    let n = sv.arr[sv.off + i].borrow().as_int().unwrap_or(0);
                    bytes.push(n as u8);
                }
                Value::Str(String::from_utf8_lossy(&bytes).into_owned().into())
            }
        }
        _ => Value::Str("".into()),
    }
}

/// 类型名（诊断用；覆盖官方 `TypeString` 的常见子集）。
fn type_name(tt: &TypeTable, t: TypeId) -> String {
    let t = unalias(tt, t);
    match tt.type_of(t) {
        Type::Basic(b) => b.as_str().to_string(),
        Type::Named(nid) => tt.named_def(*nid).name.clone(),
        Type::Pointer(e) => format!("*{}", type_name(tt, *e)),
        Type::Slice(e) => format!("[]{}", type_name(tt, *e)),
        Type::Array { len, elem } => {
            let n = len
                .map(|id| match tt.consts().get(id) {
                    rgoc_const::Const::Int(b) => b.to_dec(),
                    _ => "?".to_string(),
                })
                .unwrap_or_else(|| "?".to_string());
            format!("[{n}]{}", type_name(tt, *elem))
        }
        Type::Map { key, elem } => {
            format!("map[{}]{}", type_name(tt, *key), type_name(tt, *elem))
        }
        Type::Interface(_) => "interface {}".to_string(),
        Type::Signature(_) => "func(...)".to_string(),
        Type::Chan { dir, elem } => {
            let arrow = match dir {
                rgoc_types::ChanDir::SendOnly => "chan<- ",
                rgoc_types::ChanDir::RecvOnly => "<-chan ",
                rgoc_types::ChanDir::SendRecv => "chan ",
            };
            format!("{arrow}{}", type_name(tt, *elem))
        }
        Type::Struct(_) => "struct {...}".to_string(),
        _ => "invalid type".to_string(),
    }
}

// ───────────────────────── 调度器（Task 4） ─────────────────────────
//
// 为什么是 **unit test** 而不是 `tests/sched_loop.rs`：`G` 是私有类型，
// 集成测试构造不出来；而 `go` 语句要到 Task 24 才能从源码触发，
// 所以「队列怎么走」这一层现在只能在这里判。等 Task 24 有了 `go`，
// 端到端的判据由 `con_chan.rs` 承担，这里留着当**单元级**的守门员。

#[cfg(test)]
mod sched_tests {
    use super::*;

    fn empty_prog() -> (Program, TypeTable) {
        (
            Program {
                funcs: Vec::new(),
                globals: Vec::new(),
                init_order: Vec::new(),
                inits: Vec::new(),
                main: None,
            },
            TypeTable::new(),
        )
    }

    fn shared<'a>(prog: &'a Program, tt: &'a TypeTable) -> Shared<'a> {
        Shared::new(prog, tt, Options::default())
    }

    /// 一个「跑一步就结束」的假 goroutine（帧空 + `Done`）。
    fn idle(phase: TopPhase) -> G<'static> {
        G {
            id: 0,
            frames: Vec::new(),
            eval: Vec::new(),
            values: Vec::new(),
            targets: Vec::new(),
            pending: None,
            phase,
            blocked: false,
            waiters: Vec::new(),
            panic_val: None,
        }
    }

    #[test]
    fn pick_是_fifo() {
        let (prog, tt) = empty_prog();
        let mut sh = shared(&prog, &tt);
        for id in [2usize, 0, 1] {
            sh.gs.push(Some(idle(TopPhase::Done)));
            sh.runq.push_back(id);
        }
        // 入队顺序 2,0,1 ⇒ 出队顺序也必须是 2,0,1（`C11` §2.1 规则 3）。
        assert_eq!(sh.pick(), Some(2));
        assert_eq!(sh.pick(), Some(0));
        assert_eq!(sh.pick(), Some(1));
        assert_eq!(sh.pick(), None);
    }

    #[test]
    fn drive_把队列里每个_goroutine_都跑到退出并回收() {
        let (prog, tt) = empty_prog();
        let mut sh = shared(&prog, &tt);
        for id in 0..3usize {
            sh.gs.push(Some(idle(TopPhase::Done)));
            sh.runq.push_back(id);
        }
        sh.drive();

        assert!(sh.runq.is_empty(), "队列必须被抽干");
        assert!(
            sh.gs.iter().all(Option::is_none),
            "三个都跑完了，槽位必须全部回收"
        );
        assert_eq!(
            sh.steps, 3,
            "每个假 goroutine 恰好推进一次（`one_step` 的第一件事就是 +1）"
        );
        assert!(sh.halted.is_none(), "没有预算超限也没有 panic");
    }

    #[test]
    fn main_done_的四种形态() {
        let (prog, tt) = empty_prog();
        let mut sh = shared(&prog, &tt);
        assert!(sh.main_done(), "还没有 goroutine ⇒ 视为结束");

        sh.gs.push(None);
        assert!(sh.main_done(), "main 的槽位是 None ⇒ 已结束");

        sh.gs[0] = Some(idle(TopPhase::Done));
        assert!(sh.main_done(), "帧空 + phase=Done ⇒ 已结束");

        sh.gs[0] = Some(idle(TopPhase::Globals(0)));
        assert!(!sh.main_done(), "还在跑全局初始化 ⇒ 没结束");
    }
}

// ───────────────────────── 伪随机源（Task 5） ─────────────────────────

#[cfg(test)]
mod rng_tests {
    use super::*;

    fn empty_prog() -> (Program, TypeTable) {
        (
            Program {
                funcs: Vec::new(),
                globals: Vec::new(),
                init_order: Vec::new(),
                inits: Vec::new(),
                main: None,
            },
            TypeTable::new(),
        )
    }

    #[test]
    fn seed_0_是源码顺序模式() {
        let mut r = Rng::new(0);
        assert!(r.is_source_order());
        // 不管问几次、问多大的 n，答案都是 0（= 源码顺序的第一个）。
        for _ in 0..8 {
            assert_eq!(r.below(5), 0, "源码顺序模式必须恒选第一个");
        }
        assert_eq!(r.state, 0, "源码顺序模式不许推进状态");
    }

    #[test]
    #[should_panic(expected = "源码顺序")]
    fn seed_0_抽随机数必须炸() {
        // `debug_assert` 在测试 profile 下是开的：谁在源码顺序模式下抽随机数，
        // 那就是实现 bug（不是「碰巧抽到 0」），必须响亮地炸。
        let _ = Rng::new(0).next_u64();
    }

    #[test]
    fn 同_seed_同序列() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        let seq_a: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
        let seq_b: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
        assert_eq!(seq_a, seq_b, "同 seed 必须逐位可回放（`CON-7`）");
        assert!(
            seq_a.iter().all(|&x| x != 0),
            "xorshift64 在非零种子上不会产出 0（否则状态会被卡死）"
        );
        assert!(
            seq_a.windows(2).all(|w| w[0] != w[1]),
            "连续两次不许相同（xorshift64 的周期是 2^64-1）"
        );
    }

    #[test]
    fn 不同_seed_不同序列() {
        let mut seen = std::collections::BTreeSet::new();
        for seed in 1..=8u64 {
            seen.insert(Rng::new(seed).next_u64());
        }
        assert_eq!(seen.len(), 8, "8 个 seed 必须给出 8 个不同的首值");
    }

    #[test]
    fn below_落在区间内() {
        for seed in 1..=4u64 {
            let mut r = Rng::new(seed);
            for n in 1..=8usize {
                for _ in 0..16 {
                    assert!(r.below(n) < n, "below({n}) 必须 < n");
                }
            }
        }
        // n == 1 不抽随机数（省一次状态推进，也让「只有一个候选」不消耗序列）。
        let mut r = Rng::new(7);
        let before = r.state;
        assert_eq!(r.below(1), 0);
        assert_eq!(r.state, before, "n=1 不许推进状态");
    }

    #[test]
    fn options_seed_进入_rng() {
        let (prog, tt) = empty_prog();
        // 默认 ⇒ 源码顺序（语料期望值就是在它上面冻结的）。
        let sh = Shared::new(&prog, &tt, Options::default());
        assert!(sh.rng.is_source_order(), "默认 seed=0 ⇒ 源码顺序");

        // 显式 seed ⇒ 与同 seed 的独立 Rng 走同一条序列（证明是**真的**接上了，
        // 不是「新建了一个默认 Rng」）。
        let mut sh = Shared::new(
            &prog,
            &tt,
            Options {
                seed: 7,
                ..Options::default()
            },
        );
        assert!(!sh.rng.is_source_order());
        let mut expect = Rng::new(7);
        for _ in 0..4 {
            assert_eq!(sh.rng.below(10), expect.below(10));
        }
    }
}
