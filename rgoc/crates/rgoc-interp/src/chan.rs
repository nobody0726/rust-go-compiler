//! 通道对象（`D-M8-6`，契约 [`C11`](../../../docs/contracts/C11-concurrency.md) §3）。
//!
//! # 这一层为什么是纯数据
//!
//! 它**不认** goroutine 调度、不认步数预算、也**不 panic**。四个操作只回答
//! 「当前状态下该发生什么」，把「panic 到哪个 G」「谁入就绪队列」留给解释器。
//! 理由有两条：
//!
//! 1. **只有一处实现**（`M8-plan.md` Global Constraints）。调用方与被调方各写一遍
//!    通道状态机，就是 M7 `pass_args` 那类「两边一起错、端到端仍绿」的错。
//! 2. **可判**。`Outcome` 是值，`assert_eq` 就能钉死 14 个格子；而「panic 在哪个
//!    G 里」要靠端到端跑程序才看得出来，那是 Task 22 的事。
//!
//! # 两个容易写反的顺序
//!
//! - **`send` 的 `closed` 检查必须排第一**（官方 `chansend` 加锁后第一件事就是它，
//!   `runtime/chan.go:224`）。排在「buf 未满」之后，`close(c); c <- v`
//!   （`cap > 0` 且缓冲未满）会**静默把值塞进缓冲** —— 不 panic、不报错。
//! - **`close` 必须清空两侧队列**。只置 `closed = true` 的话，`recvq` 里的接收方
//!   会一直等到天荒地老（`closed` 分支永远轮不到它们），而 `sendq` 里那笔已经被
//!   拒掉的发送会在下一次 `recv` 时被「直接接收」交出去。
//!
//! # `done` 是谁的判据
//!
//! `done` 只由**真的完成了这次操作的对端**置位（`C11` §3.1）。`close` 只唤醒、
//! **不**置 `done`：被 close 唤醒的接收方重试的是「通道操作」，它按
//! `buf` → `closed` 的顺序自己拿到零值；把 `done` 置上的话，`select` 会以为
//! 「这个 case 已经成功交付了一个值」，而实际上什么都没交付。

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use rgoc_types::TypeId;

use crate::value::Value;

/// goroutine 编号 —— 就是 `Shared.gs` 的下标（`D-M8-1`）。
///
/// 不用 `next_g` 那个只增不减的编号：队列里存下标才能 O(1) 找到状态，
/// 而「第几个被创建」这种编号一旦有 goroutine 退出就会出现空洞。
pub type Gid = usize;

/// 等待者句柄。
///
/// 队列与「等着的那个 G」共享同一个对象（所以是 `Rc<RefCell<_>>`）：对端
/// 填 `slot`、置 `done` 之后，那个 G 醒来要能看见。用值放进队列的话，
/// 对端只能改队列里那份副本，G 永远等不到。
pub type WaiterHandle = Rc<RefCell<Waiter>>;

/// 一个阻塞在通道上的 goroutine。
#[derive(Debug)]
pub struct Waiter {
    /// 谁在等。
    pub g: Gid,
    /// 同一个 `select` 的所有登记共用一个 token（`C11` §4.3 的撤销判据）。
    pub token: u64,
    /// 阻塞**发送方**放它的值；阻塞**接收方**留 `None` 等对端填。
    pub slot: Option<Value>,
    /// 这次操作是否已经被对端完成（`C11` §3.1：**唯一**判据，不许数重试次数）。
    pub done: bool,
}

/// 一个通道（`C11` §3.1）。
#[derive(Debug)]
pub struct ChanObj {
    /// 元素类型（`make(chan T, n)` 的 `T`）。
    ///
    /// 四个操作都不用它 —— 接收零值用的是**接收方**的静态类型（HIR 里有）。
    /// 留着是因为它是通道身份的一部分（`chan int` 与 `chan string` 不能混），
    /// 且 `print(c)` 之类要能说出来。
    elem: TypeId,
    /// 容量；无缓冲 = 0。
    cap: usize,
    /// 环形缓冲（`VecDeque` 的 `pop_front`/`push_back` 就是 FIFO）。
    /// 不变式：`buf.len() <= cap`。
    buf: VecDeque<Value>,
    /// 是否已关闭（单向：`false → true`）。
    closed: bool,
    /// 阻塞的发送方（FIFO）。
    sendq: VecDeque<WaiterHandle>,
    /// 阻塞的接收方（FIFO）。
    recvq: VecDeque<WaiterHandle>,
}

/// `recv` 的结果（`C11` §3.2）。
#[derive(Debug)]
pub enum RecvOutcome {
    /// 拿到一个值。
    Got {
        /// 值的**独立副本**（`C11` §3.3 的第二次深拷贝）。
        value: Value,
        /// 因为这次接收而被唤醒的发送方（`buf` 补位 / 无缓冲 handoff）。
        woke: Option<Gid>,
    },
    /// 通道已关闭且缓冲已空 ⇒ 零值 + `ok = false`。
    ///
    /// **不带值**：零值由调用方按接收方的静态类型造（`zero_of`）。通道的
    /// `elem` 与接收方的类型在 `recv` 位置总是同一个，但造零值需要 `TypeTable`，
    /// 那是解释器手上的东西。
    GotZeroClosed,
    /// 没有可拿的值 ⇒ 调用方**自己**登记 waiter（`try_recv` 不登记，见模块头）。
    Blocked,
}

/// `send` 的结果（`C11` §3.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendOutcome {
    /// 交付成功。
    Done {
        /// 因为这次发送而被唤醒的接收方。
        woke: Option<Gid>,
    },
    /// 缓冲满（或本来就是无缓冲）且没有等待的接收方 ⇒ 调用方登记 waiter 并阻塞。
    Blocked,
    /// 通道已关闭 ⇒ 调用方在**自己**的 G 里 `panic: send on closed channel`。
    Closed,
}

/// `close` 的结果（`C11` §3.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseOutcome {
    /// 关闭成功，并按 `recvq` → `sendq` 的 FIFO 顺序唤醒了这些 G。
    ///
    /// 两侧都要有：接收方要醒来拿零值，发送方要醒来**在自己那个 G 里** panic。
    Woken(Vec<Gid>),
    /// 已经关过了 ⇒ 调用方 `panic: close of closed channel`。
    AlreadyClosed,
}

impl ChanObj {
    /// 造一个通道。
    #[must_use]
    pub fn new(elem: TypeId, cap: usize) -> Self {
        Self {
            elem,
            cap,
            buf: VecDeque::new(),
            closed: false,
            sendq: VecDeque::new(),
            recvq: VecDeque::new(),
        }
    }

    /// 元素类型。
    #[must_use]
    pub fn elem(&self) -> TypeId {
        self.elem
    }

    /// `len(c)`：缓冲里有多少值。
    ///
    /// 阻塞发送方的值**不算**（它在 `Waiter.slot` 里，还没有被通道接收）。
    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// 缓冲是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// `cap(c)`：容量（无缓冲 = 0）。
    #[must_use]
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 是否已关闭。
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 接收队列是否为空。
    #[must_use]
    pub fn recvq_is_empty(&self) -> bool {
        self.recvq.is_empty()
    }

    /// 发送队列是否为空。
    #[must_use]
    pub fn sendq_is_empty(&self) -> bool {
        self.sendq.is_empty()
    }

    /// 把一个**发送方**登记到队尾（`C11` §3.2 send ④）。
    ///
    /// 登记由调用方发起（`try_send` 只回 `Blocked`）：它才知道 `token` 与
    /// 这个 waiter 属于哪个 `select`。
    pub fn park_send(&mut self, w: WaiterHandle) {
        self.sendq.push_back(w);
    }

    /// 把一个**接收方**登记到队尾（`C11` §3.2 recv ④）。
    pub fn park_recv(&mut self, w: WaiterHandle) {
        self.recvq.push_back(w);
    }

    /// 撤销同一个 `token` 的全部登记（`C11` §4.3 的取消其他候选等待者）。
    ///
    /// **幂等**：找不到也算成功。`select` 被唤醒后要把它在**所有**通道上的
    /// 登记都删掉 —— 但其中获胜的那个已经被对端取走并出队了，再删一次必须
    /// 无害，否则唤醒路径上任何一次重入都会炸。
    pub fn revoke(&mut self, token: u64) {
        self.sendq.retain(|w| w.borrow().token != token);
        self.recvq.retain(|w| w.borrow().token != token);
    }

    /// 收一个（`C11` §3.2 recv 的 ①→④）。
    pub fn try_recv(&mut self) -> RecvOutcome {
        // ① 缓冲非空：先给值，再让一个阻塞发送方补位（保持 buf 满）。
        if let Some(v) = self.buf.pop_front() {
            let woke = self.refill_from_sendq();
            return RecvOutcome::Got {
                value: v.deep_copy(),
                woke,
            };
        }
        // ② 有阻塞发送方：无缓冲 handoff（值不经过 buf）。
        if let Some(w) = self.sendq.pop_front() {
            let mut w = w.borrow_mut();
            w.done = true;
            let value = w
                .slot
                .take()
                .expect("阻塞发送方的 slot 必须有值（登记时就放了）");
            let g = w.g;
            return RecvOutcome::Got {
                value: value.deep_copy(),
                woke: Some(g),
            };
        }
        // ③ 已关闭且缓冲已空：零值 + ok=false。
        if self.closed {
            return RecvOutcome::GotZeroClosed;
        }
        // ④ 其余：调用方登记并阻塞。
        RecvOutcome::Blocked
    }

    /// 送一个（`C11` §3.2 send 的 ①→④）。
    ///
    /// 值用**引用**传：`Blocked` 分支要求调用方还拿着它去登记 waiter。
    /// 传值的话那笔发送的值就被 `try_send` 吞掉了。
    pub fn try_send(&mut self, v: &Value) -> SendOutcome {
        // ① 已关闭：**第一件事**（见模块头）。
        if self.closed {
            return SendOutcome::Closed;
        }
        // ② 有阻塞接收方：直接进它的 slot。
        if let Some(w) = self.recvq.pop_front() {
            let mut w = w.borrow_mut();
            w.slot = Some(v.deep_copy());
            w.done = true;
            return SendOutcome::Done { woke: Some(w.g) };
        }
        // ③ 缓冲有空间：入队尾。
        if self.cap > 0 && self.buf.len() < self.cap {
            self.buf.push_back(v.deep_copy());
            return SendOutcome::Done { woke: None };
        }
        // ④ 其余：调用方登记并阻塞。
        SendOutcome::Blocked
    }

    /// 关闭（`C11` §3.2 close 的 ①→②）。
    pub fn close(&mut self) -> CloseOutcome {
        if self.closed {
            return CloseOutcome::AlreadyClosed;
        }
        self.closed = true;
        // 先接收方、后发送方：接收方重试时会先把 `buf` 排空再拿零值，
        // 发送方重试时看到 `closed` 直接 panic —— 两边的顺序互不依赖，
        // 但「都在同一次 close 里唤醒」是硬要求。
        let mut woke = Vec::with_capacity(self.recvq.len() + self.sendq.len());
        while let Some(w) = self.recvq.pop_front() {
            woke.push(w.borrow().g);
        }
        while let Some(w) = self.sendq.pop_front() {
            woke.push(w.borrow().g);
        }
        CloseOutcome::Woken(woke)
    }

    /// 把一个阻塞发送方的值搬进 `buf`（recv ① 的补位），返回它的 G。
    fn refill_from_sendq(&mut self) -> Option<Gid> {
        let w = self.sendq.pop_front()?;
        let mut w = w.borrow_mut();
        w.done = true;
        let v = w
            .slot
            .take()
            .expect("阻塞发送方的 slot 必须有值（登记时就放了）");
        let g = w.g;
        self.buf.push_back(v.deep_copy());
        Some(g)
    }
}
