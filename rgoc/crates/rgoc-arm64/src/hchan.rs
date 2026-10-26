//! native 侧 `hchan` 的**影子**（`M9-design.md` §3.12；契约 [`C11`](../../../docs/contracts/C11-concurrency.md) §7）。
//!
//! # M9 只交结构与协议，不写执行（`D-M9-14`）
//!
//! 这里有**两个**类型，各自回答一个问题：
//!
//! | 类型 | 回答 | 判据 |
//! |---|---|---|
//! | [`Hchan`]（`#[repr(C)]`） | 这个对象在内存里**长什么样** | `T-SCH-01`：逐字段 `offset_of` == `C11` §7 的表 |
//! | [`ChanState`] | 四个操作在每种状态下**该发生什么** | `T-SCH-02/03`：与 `rgoc-interp` 的 `ChanObj` **逐格差分** |
//!
//! # 为什么不是一个 struct 带三操作（计划里是这么写的）
//!
//! `R-9-37`：本仓 workspace 级 `unsafe_code = "forbid"`，而「在 `Hchan` 上直接
//! 实现三操作」必须**解引用** `*mut Sudog`（取 `next`、写 `done`）—— 那一步
//! 没有 safe 写法。两个选项：
//!
//! 1. 给这个 crate 开 `unsafe_code = "allow"`；
//! 2. 把「布局」与「状态机」拆成两个类型，状态机用安全表示（队列存
//!    `(id, token)`，`done` 由返回值交给调用方）。
//!
//! 选 2。理由：「整个编译器不含 `unsafe`」比「一个 struct 更省事」值钱得多 ——
//! 尤其这一层**不执行**任何东西，`unsafe` 的收益只有「省一个类型」。
//! 两者的字段名必须一一对应（`state_is_projection_of_layout` 是它的守卫）。
//!
//! # 字节搬运刻意不在这一层
//!
//! `try_send` / `try_recv` 只判**状态机**（谁被唤醒、下标怎么走、`qcount` 变多少），
//! 不碰 `buf` 里的字节。搬运是「执行」，归 M11；这一层交的是「给定状态，
//! 该发生什么」。
//!
//! # 与解释器那张表的唯一差别
//!
//! `ChanObj` 的等待者是 `Rc<RefCell<Waiter>>` —— 它需要共享，因为对端置 `done`
//! 之后那个 G 醒来要**看得见**。这里没有「醒来」，`done` 由返回值
//! （`woke: Option<WaiterRef>`）交给调用方去置。**判据不变**：`done` 仍然是
//! 「这一侧已完成」的唯一标志（`C11` §3.1）。

use std::collections::VecDeque;

use rgoc_meta::desc::TypeDesc;

/// 队列里一个等待者的**身份**。
///
/// 用「下标 + token」而不是指针：这一层不持有等待者的存储（它归 M11 的
/// `Sudog`），只需要能说出「是哪一个」以及「属于哪个 `select`」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WaiterRef {
    /// 调用方给的下标（M11 的 `sudog` 编号 / 解释器的 `Gid`）。
    pub id: u32,
    /// `select` 的撤销句柄（`C11` §4.2）。非 `select` 的等待用 0。
    pub token: u64,
}

/// 等待队列（`waitq`）—— **布局**形态（`C11` §7 冻结）。
///
/// M9 不实现链表操作（那要解引用 `*mut Sudog`）；这个类型存在的意义是
/// 「`recvq` / `sendq` 各占 16 字节、head 在前」这一条能对上。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Waitq {
    /// 队首。
    pub head: *mut Sudog,
    /// 队尾（O(1) 入队）。
    pub tail: *mut Sudog,
}

/// 一个阻塞在通道上的 goroutine —— **布局**形态（`C11` §7 冻结）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Sudog {
    /// 等着的那个 goroutine（M11 的 `gdesc*`；M9 当不透明指针）。
    pub g: *mut u8,
    /// 值的存放位置（发送方：值在这；接收方：对端写到这）。
    pub elem: *mut u8,
    /// 队列后继。
    pub next: *mut Sudog,
    /// 队列前驱（`revoke` 要 O(1) 摘链）。
    pub prev: *mut Sudog,
    /// `select` 的撤销句柄。
    pub token: u64,
    /// 是否已被对端完成（`C11` §3.1 的**唯一**判据）。
    pub done: u32,
}

/// native 侧的 `hchan`（`C11` §7 冻结的 96 字节）。
///
/// # 只做布局，没有任何方法
///
/// 这个类型**不**实现四个操作：见模块头（`R-9-37`）。它的全部用途是让
/// `size_of` / `offset_of` 可断言 —— 那是 M11 唯一能提前冻结的东西。
///
/// # 字段序按官方 `runtime/chan.go:34`
///
/// 不重排、不省字段（含 `sendx` / `recvx`）：`(sendx+1) % dataqsiz` 的环形
/// 推进是官方语义的一部分，省掉它就没法对着官方源码读。
#[derive(Debug)]
#[repr(C)]
pub struct Hchan {
    /// 缓冲里有几个值（`len(c)`）。
    pub qcount: usize,
    /// 环形缓冲能放几个（`cap(c)`；无缓冲 = 0）。
    pub dataqsiz: usize,
    /// `dataqsiz * elemsize` 字节的缓冲；`dataqsiz == 0` 时为 null。
    pub buf: *mut u8,
    /// 元素宽度（字节）。
    pub elemsize: u16,
    /// 是否已关闭（单向 `0 → 1`）。
    pub closed: u32,
    /// 元素类型的 desc（GC 扫堆元素时要它，`C5` §8.4）。
    pub elemtype: *const TypeDesc,
    /// 环形发送下标。
    pub sendx: usize,
    /// 环形接收下标。
    pub recvx: usize,
    /// 阻塞的接收方。
    pub recvq: Waitq,
    /// 阻塞的发送方。
    pub sendq: Waitq,
    /// 锁（M9 的镜像不实现；M11 可以是 futex）。
    pub lock: u64,
}

impl Hchan {
    /// 造一个**不拥有**缓冲的 `hchan`（`buf` 由调用方给）。
    #[must_use]
    pub fn new(elemsize: u16, dataqsiz: usize, buf: *mut u8, elemtype: *const TypeDesc) -> Self {
        Self {
            qcount: 0,
            dataqsiz,
            buf,
            elemsize,
            closed: 0,
            elemtype,
            sendx: 0,
            recvx: 0,
            recvq: Waitq {
                head: std::ptr::null_mut(),
                tail: std::ptr::null_mut(),
            },
            sendq: Waitq {
                head: std::ptr::null_mut(),
                tail: std::ptr::null_mut(),
            },
            lock: 0,
        }
    }
}

// ── 状态机 ──────────────────────────────────────────────────────────

/// `send` 的结果（与 `ChanObj::SendOutcome` 一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendOutcome {
    /// 交付成功；`woke` 是被唤醒的接收方（调用方负责搬字节 + 置 `done`）。
    Done {
        /// 被唤醒的接收方。
        woke: Option<WaiterRef>,
    },
    /// 缓冲满（或本来就是无缓冲）且没有等待的接收方 ⇒ 调用方登记并阻塞。
    Blocked,
    /// 已关闭 ⇒ 调用方在自己那个 G 里 `panic: send on closed channel`。
    Closed,
}

/// `recv` 的结果（与 `ChanObj::RecvOutcome` 一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecvOutcome {
    /// 拿到一个值。
    Got {
        /// 值来自哪里 —— 缓冲（`true`）还是某个阻塞发送方（`false`）。
        ///
        /// 搬运的目标不同：缓冲里的是 `buf[recvx]`，阻塞发送方的是它的 `elem`。
        from_buf: bool,
        /// 因为这次接收而被唤醒的发送方（缓冲补位 / 无缓冲 handoff）。
        woke: Option<WaiterRef>,
    },
    /// 已关闭且缓冲已空 ⇒ 零值 + `ok = false`。
    ZeroClosed,
    /// 没有可拿的值 ⇒ 调用方登记并阻塞。
    Blocked,
}

/// `close` 的结果（与 `ChanObj::CloseOutcome` 一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOutcome {
    /// 关闭成功；两侧队列全部被唤醒（先接收方、后发送方，各自 FIFO）。
    Woken {
        /// 被唤醒的接收方个数。
        recv: usize,
        /// 被唤醒的发送方个数。
        send: usize,
    },
    /// 已经关过了 ⇒ `panic: close of closed channel`。
    AlreadyClosed,
}

/// 通道的**状态机**（`C11` §3.2 的四张表）。
///
/// 与 [`Hchan`] 的字段一一对应（`state_is_projection_of_layout` 守着这条），
/// 只是把「链表 + 指针」换成了「队列 + 身份」：这一层不碰内存。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChanState {
    /// 缓冲里有几个值。
    qcount: usize,
    /// 容量。
    dataqsiz: usize,
    /// 是否已关闭。
    closed: bool,
    /// 环形发送下标。
    sendx: usize,
    /// 环形接收下标。
    recvx: usize,
    /// 阻塞的接收方（FIFO）。
    recvq: VecDeque<WaiterRef>,
    /// 阻塞的发送方（FIFO）。
    sendq: VecDeque<WaiterRef>,
}

impl ChanState {
    /// 造一个空通道（`dataqsiz == 0` ⇒ 无缓冲）。
    #[must_use]
    pub fn new(dataqsiz: usize) -> Self {
        Self {
            qcount: 0,
            dataqsiz,
            closed: false,
            sendx: 0,
            recvx: 0,
            recvq: VecDeque::new(),
            sendq: VecDeque::new(),
        }
    }

    /// 缓冲里有几个值（`len(c)`）。
    #[must_use]
    pub fn qcount(&self) -> usize {
        self.qcount
    }

    /// 容量（`cap(c)`）。
    #[must_use]
    pub fn cap(&self) -> usize {
        self.dataqsiz
    }

    /// 是否已关闭。
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 环形发送下标。
    #[must_use]
    pub fn sendx(&self) -> usize {
        self.sendx
    }

    /// 环形接收下标。
    #[must_use]
    pub fn recvx(&self) -> usize {
        self.recvx
    }

    /// 阻塞的接收方个数。
    #[must_use]
    pub fn recvq_len(&self) -> usize {
        self.recvq.len()
    }

    /// 阻塞的发送方个数。
    #[must_use]
    pub fn sendq_len(&self) -> usize {
        self.sendq.len()
    }

    /// 登记一个阻塞的接收方（队尾）。
    pub fn park_recv(&mut self, w: WaiterRef) {
        self.recvq.push_back(w);
    }

    /// 登记一个阻塞的发送方（队尾）。
    pub fn park_send(&mut self, w: WaiterRef) {
        self.sendq.push_back(w);
    }

    /// 撤销同一个 `token` 的全部登记（`C11` §4.2）。
    ///
    /// **幂等**：找不到也算成功 —— `select` 被唤醒后要把它在**所有**通道上的
    /// 登记删掉，其中获胜的那个已经被对端取走并出队了，再删一次必须无害。
    pub fn revoke(&mut self, token: u64) {
        self.recvq.retain(|w| w.token != token);
        self.sendq.retain(|w| w.token != token);
    }

    /// 送一个（`C11` §3.2 send 的 ①→④）。
    pub fn try_send(&mut self) -> SendOutcome {
        // ① 已关闭：**第一件事**（`R-8-2` 的教训：排在 buf 之后就会静默塞缓冲）。
        if self.closed {
            return SendOutcome::Closed;
        }
        // ② 有阻塞接收方：直接交付（无缓冲 handoff）。
        if let Some(w) = self.recvq.pop_front() {
            return SendOutcome::Done { woke: Some(w) };
        }
        // ③ 缓冲有空间：入队尾（环形）。
        if self.dataqsiz > 0 && self.qcount < self.dataqsiz {
            self.sendx = (self.sendx + 1) % self.dataqsiz;
            self.qcount += 1;
            return SendOutcome::Done { woke: None };
        }
        // ④ 其余：调用方登记并阻塞。
        SendOutcome::Blocked
    }

    /// 收一个（`C11` §3.2 recv 的 ①→④）。
    pub fn try_recv(&mut self) -> RecvOutcome {
        // ① 缓冲非空：给值，再让一个阻塞发送方补位（保持 buf 满）。
        if self.qcount > 0 {
            // `qcount > 0` ⇒ `dataqsiz > 0`（无缓冲的通道永远装不进值），
            // 所以这里不会有机会除以 0；写成显式 `if` 只是让这条蕴含看得见。
            if self.dataqsiz > 0 {
                self.recvx = (self.recvx + 1) % self.dataqsiz;
            }
            self.qcount -= 1;
            if let Some(w) = self.sendq.pop_front() {
                if self.dataqsiz > 0 {
                    self.sendx = (self.sendx + 1) % self.dataqsiz;
                }
                self.qcount += 1;
                return RecvOutcome::Got {
                    from_buf: true,
                    woke: Some(w),
                };
            }
            return RecvOutcome::Got {
                from_buf: true,
                woke: None,
            };
        }
        // ② 有阻塞发送方：无缓冲 handoff（值不经过 buf）。
        if let Some(w) = self.sendq.pop_front() {
            return RecvOutcome::Got {
                from_buf: false,
                woke: Some(w),
            };
        }
        // ③ 已关闭且缓冲已空：零值 + ok=false。
        if self.closed {
            return RecvOutcome::ZeroClosed;
        }
        // ④ 其余：调用方登记并阻塞。
        RecvOutcome::Blocked
    }

    /// 关闭（`C11` §3.2 close 的 ①→②）。
    ///
    /// 两侧都清空：接收方醒来按 `buf` → `closed` 自己拿零值；发送方醒来看到
    /// `closed` 在自己那个 G 里 panic。**不置 `done`** —— `close` 什么都没交付。
    pub fn close(&mut self) -> CloseOutcome {
        if self.closed {
            return CloseOutcome::AlreadyClosed;
        }
        self.closed = true;
        let recv = self.recvq.len();
        let send = self.sendq.len();
        self.recvq.clear();
        self.sendq.clear();
        CloseOutcome::Woken { recv, send }
    }
}

/// [`Hchan`] 的字段名（`C11` §7 的表，去掉纯布局项）。
///
/// 给「状态机是布局的投影」这条判据用：`STATE_FIELDS` 必须**全部**出现在
/// 这张表里（状态机不许凭空多出一个字段）。反向不要求 —— `buf` / `elemsize`
/// 是纯布局项，状态机刻意不碰（它不搬字节）。
pub const LAYOUT_FIELDS: [&str; 9] = [
    "qcount", "dataqsiz", "closed", "sendx", "recvx", "recvq", "sendq", "buf", "elemsize",
];

/// [`ChanState`] 的字段名。
pub const STATE_FIELDS: [&str; 7] = [
    "qcount", "dataqsiz", "closed", "sendx", "recvx", "recvq", "sendq",
];

/// `C11` §7 的总大小（字节）。
pub const HCHAN_SIZE: usize = 96;

/// `Sudog` 的总大小（字节）。
pub const SUDOG_SIZE: usize = 48;
