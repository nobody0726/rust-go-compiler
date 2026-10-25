//! `ChanObj` 的四操作真值表（`T-CON-11..21` 的地基，`M8-plan.md` Task 6）。
//!
//! # 这一层测什么
//!
//! `chan.rs` 是**纯数据结构**：不认 goroutine、不认步数、不 panic。它只回答
//! 「这次 `recv`/`send`/`close` 在当前状态下该发生什么」，把「panic 到哪个 G」
//! 留给解释器。所以这里的断言全部落在 `Outcome` 上，一条 `#[should_panic]`
//! 都没有 —— 那三处 panic（closed 上 send / double close / close(nil)）在
//! Task 22 由解释器在自己那个 G 里发。
//!
//! # 为什么逐格列表而不是「跑一遍看看」
//!
//! 通道的错法几乎全是**顺序错**：`closed` 排在 `buf` 之后、`close` 不清 `sendq`、
//! `recv` 补位时不唤醒 —— 每一种都能让语料照样跑绿（没有并发的程序根本走不到
//! 这些分支）。表里的每一行都是「一个特定顺序」的判据。
//!
//! # nil 通道为什么不在这个表里
//!
//! `ChanObj` 不可能是 nil —— nil 是 `Value::Chan` 这一层的事（`C11` §3.4），
//! 而 `Value` 里的通道要到 Task 15 才有。nil 的「永久阻塞、不登记」由 Task 23
//! 在解释器里判；在这里加一个 `Option` 包装只会让两处都实现一遍。

use std::cell::RefCell;
use std::rc::Rc;

use rgoc_const::BasicId;
use rgoc_interp::chan::{
    ChanObj, CloseOutcome, Gid, RecvOutcome, SendOutcome, Waiter, WaiterHandle,
};
use rgoc_interp::value::{Value, cell};
use rgoc_types::{TypeId, TypeTable};

fn int_ty() -> TypeId {
    let mut tt = TypeTable::new();
    tt.add_basic(BasicId::Int)
}

/// 造一个通道（`cap = 0` 就是无缓冲）。
fn chan(cap: usize) -> ChanObj {
    ChanObj::new(int_ty(), cap)
}

/// 造一个已经装了 `vals` 的缓冲通道（`cap` 必须 ≥ `vals.len()`）。
fn filled(cap: usize, vals: &[i128]) -> ChanObj {
    let mut c = chan(cap);
    for v in vals {
        assert!(
            matches!(c.try_send(&Value::Int(*v)), SendOutcome::Done { .. }),
            "装填阶段不该阻塞"
        );
    }
    c
}

fn waiter(g: Gid, token: u64) -> WaiterHandle {
    Rc::new(RefCell::new(Waiter {
        g,
        token,
        slot: None,
        done: false,
    }))
}

/// 阻塞的**发送方**：值已经在它的 `slot` 里（`C11` §3.1）。
fn blocked_sender(g: Gid, v: i128) -> WaiterHandle {
    let w = waiter(g, 0);
    w.borrow_mut().slot = Some(Value::Int(v));
    w
}

/// 收一次，断言「拿到值」，返回 `(值, 被唤醒的 G)`。
fn got(c: &mut ChanObj) -> (i128, Option<Gid>) {
    match c.try_recv() {
        RecvOutcome::Got { value, woke } => (value.as_int().expect("整数"), woke),
        other => panic!("应当拿到值，实际 {other:?}"),
    }
}

/// 收一次，断言「已关闭且缓冲空 ⇒ 零值 + `ok=false`」。
fn got_zero_closed(c: &mut ChanObj) {
    assert!(
        matches!(c.try_recv(), RecvOutcome::GotZeroClosed),
        "应当是零值 + ok=false"
    );
}

/// 把缓冲**按 FIFO 抽干**，返回抽到的值（阻塞或零值就停）。
///
/// 用它而不是「读一个 `buf` 快照」：快照只能证明「里面有这两个数」，
/// 抽干能证明「先出来的必须是先放进去的那个」。
fn drain(c: &mut ChanObj) -> Vec<i128> {
    let mut out = Vec::new();
    while let RecvOutcome::Got { value, .. } = c.try_recv() {
        out.push(value.as_int().expect("缓冲里只放了整数"));
    }
    out
}

// ───────────────────────── ① 空（无缓冲） ─────────────────────────

#[test]
fn 空_无缓冲_recv_阻塞且不登记() {
    let mut c = chan(0);
    assert!(matches!(c.try_recv(), RecvOutcome::Blocked));
    assert_eq!(c.len(), 0);
    assert!(
        c.recvq_is_empty(),
        "登记 waiter 是调用方的事（Task 20）：`try_recv` 只回 Blocked"
    );
}

#[test]
fn 空_无缓冲_send_阻塞() {
    let mut c = chan(0);
    assert!(matches!(c.try_send(&Value::Int(1)), SendOutcome::Blocked));
    assert_eq!(c.len(), 0, "阻塞的发送不许悄悄把值留在缓冲里");
    assert_eq!(c.cap(), 0);
}

#[test]
fn 空_无缓冲_close_没有唤醒对象() {
    let mut c = chan(0);
    assert_eq!(c.close(), CloseOutcome::Woken(Vec::new()));
    assert!(c.is_closed());
    // 关掉之后 recv 立刻拿零值（不是阻塞）。
    got_zero_closed(&mut c);
}

// ───────────────────────── ② 半 / 满（有缓冲） ─────────────────────────

#[test]
fn 半_send_入队尾() {
    let mut c = filled(3, &[1]);
    assert_eq!(c.len(), 1);
    assert_eq!(c.try_send(&Value::Int(2)), SendOutcome::Done { woke: None });
    assert_eq!(c.len(), 2);
    assert_eq!(drain(&mut c), vec![1, 2], "FIFO：新值进队尾");
}

#[test]
fn 满_send_阻塞() {
    let mut c = filled(2, &[1, 2]);
    assert!(matches!(c.try_send(&Value::Int(3)), SendOutcome::Blocked));
    assert_eq!(drain(&mut c), vec![1, 2], "满的时候缓冲不许变");
}

#[test]
fn 满_recv_取出队首() {
    let mut c = filled(2, &[1, 2]);
    assert_eq!(got(&mut c), (1, None));
    assert_eq!(c.len(), 1);
    assert_eq!(drain(&mut c), vec![2], "剩下的是后进去的那个");
}

#[test]
fn 半_recv_取完变空() {
    let mut c = filled(3, &[7]);
    assert_eq!(got(&mut c).0, 7);
    assert_eq!(c.len(), 0);
    // 空且没关闭 ⇒ 下一次阻塞（不是拿零值）。
    assert!(matches!(c.try_recv(), RecvOutcome::Blocked));
}

// ───────────────────────── ③ closed ─────────────────────────

#[test]
fn closed_空_recv_零值() {
    let mut c = chan(1);
    assert_eq!(c.close(), CloseOutcome::Woken(Vec::new()));
    got_zero_closed(&mut c);
    got_zero_closed(&mut c); // 关一次可以一直拿零值
}

#[test]
fn closed_有缓冲_recv_先排空再零值() {
    let mut c = filled(2, &[1, 2]);
    assert_eq!(c.close(), CloseOutcome::Woken(Vec::new()));
    // `C11` §3.2 close ②：已关闭但缓冲还有值 ⇒ 先吐值。
    assert_eq!(got(&mut c).0, 1);
    assert_eq!(got(&mut c).0, 2);
    got_zero_closed(&mut c); // 排空之后才是零值
}

#[test]
fn closed_send_必须报_closed_哪怕缓冲有空间() {
    // ⚠️ 这条是「`closed` 排在 `buf` 之后」那个顺序错的牙：
    // 官方 `chansend` 加锁后第一件事就是查 `closed`（`runtime/chan.go:224`）。
    // 顺序写反的话，这里会返回 `Done` 并**静默**把值塞进缓冲。
    let mut c = filled(2, &[1]);
    assert_eq!(c.close(), CloseOutcome::Woken(Vec::new()));
    assert_eq!(c.try_send(&Value::Int(9)), SendOutcome::Closed);
    assert_eq!(drain(&mut c), vec![1], "被拒的发送不许进缓冲");
}

#[test]
fn closed_再_close_报已关闭() {
    let mut c = chan(0);
    assert_eq!(c.close(), CloseOutcome::Woken(Vec::new()));
    assert_eq!(c.close(), CloseOutcome::AlreadyClosed);
}

// ───────────────────────── ④ 有等待者（handoff） ─────────────────────────

#[test]
fn 空_recvq_有等待者_send_直接交付() {
    let mut c = chan(0);
    let r = waiter(5, 0);
    c.park_recv(Rc::clone(&r));

    assert_eq!(
        c.try_send(&Value::Int(42)),
        SendOutcome::Done { woke: Some(5) }
    );
    let r = r.borrow();
    assert_eq!(
        r.slot.as_ref().and_then(Value::as_int),
        Some(42),
        "值直接进接收方的 slot"
    );
    assert!(r.done, "`done` 由对端置位");
    assert!(c.recvq_is_empty(), "被满足的接收方必须出队");
    assert_eq!(c.len(), 0, "无缓冲的 handoff 不经过 buf");
}

#[test]
fn 空_sendq_有等待者_recv_直接接收() {
    let mut c = chan(0);
    let s = blocked_sender(7, 42);
    c.park_send(Rc::clone(&s));

    assert_eq!(got(&mut c), (42, Some(7)));
    let s = s.borrow();
    assert!(s.done);
    assert!(s.slot.is_none(), "值已经被取走");
    assert!(c.sendq_is_empty());
}

#[test]
fn 满_sendq_有等待者_recv_补位并唤醒() {
    let mut c = filled(1, &[1]);
    let s = blocked_sender(9, 2);
    c.park_send(Rc::clone(&s));

    // 取走 buf 里的 1，同时把阻塞发送方的 2 搬进 buf（保持 buf 满）。
    assert_eq!(got(&mut c), (1, Some(9)));
    assert_eq!(c.len(), 1, "收一个之后 len 立刻回到 cap");
    assert!(s.borrow().done);
    assert_eq!(drain(&mut c), vec![2], "补位：取走一个立刻补一个");
}

// ───────────────────────── ⑤ close 的两侧唤醒 ─────────────────────────

#[test]
fn close_清空两侧队列且唤醒全部() {
    let mut c = chan(0);
    let r1 = waiter(1, 0);
    let r2 = waiter(2, 0);
    let s1 = blocked_sender(3, 30);
    let s2 = blocked_sender(4, 40);
    c.park_recv(Rc::clone(&r1));
    c.park_recv(Rc::clone(&r2));
    c.park_send(Rc::clone(&s1));
    c.park_send(Rc::clone(&s2));

    // 两侧都要唤醒，顺序是**各自队列的 FIFO**（`C11` §2.1 规则 3）。
    assert_eq!(c.close(), CloseOutcome::Woken(vec![1, 2, 3, 4]));
    assert!(
        c.recvq_is_empty() && c.sendq_is_empty(),
        "close 必须清空两侧队列"
    );
    // 被唤醒的接收方重试时按「先排空 buf（空的）再拿零值」。
    got_zero_closed(&mut c);
    // 被唤醒的发送方重试时看到 closed ⇒ 在自己的 G 里 panic（Task 22）。
    assert_eq!(c.try_send(&Value::Int(30)), SendOutcome::Closed);
}

#[test]
fn close_清掉的阻塞发送方的值不得被_recv_取走() {
    // 若 close 只置 `closed = true` 而不清 `sendq`，下面这次 recv 会走
    // 「② sendq 非空 ⇒ 直接接收」把 30 交出去 —— 那笔 send 明明已经被
    // close 拒掉了（它醒来会 panic），值不该出现在接收方手里。
    let mut c = chan(0);
    let s = blocked_sender(3, 30);
    c.park_send(Rc::clone(&s));
    assert_eq!(c.close(), CloseOutcome::Woken(vec![3]));

    got_zero_closed(&mut c);
    assert_eq!(
        s.borrow().slot.as_ref().and_then(Value::as_int),
        Some(30),
        "值留在发送方手里"
    );
}

// ───────────────────────── ⑥ 值语义 ─────────────────────────

#[test]
fn 值语义_发送时深拷贝() {
    // 少了这一次拷贝：`ch <- a` 之后 `a[0] = 9` 会改到通道里那一份，
    // 接收方拿到 9 而不是 1。
    let mut c = chan(1);
    let a = Value::Array(Rc::new(vec![cell(Value::Int(1))]));
    assert!(matches!(c.try_send(&a), SendOutcome::Done { .. }));
    if let Value::Array(cells) = &a {
        *cells[0].borrow_mut() = Value::Int(9);
    }

    let value = match c.try_recv() {
        RecvOutcome::Got { value, .. } => value,
        other => panic!("应当拿到值，实际 {other:?}"),
    };
    let Value::Array(got_cells) = &value else {
        panic!("拿到的应当是数组");
    };
    assert_eq!(
        got_cells[0].borrow().as_int(),
        Some(1),
        "发送时的拷贝：源数组改了不该影响通道里的值"
    );
}
