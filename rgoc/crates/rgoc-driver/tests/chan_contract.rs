//! native 侧 `hchan` 的契约（`T-SCH-01..04`；门禁 `SCH-1`）。
//!
//! # 这一组测试回答三个不同的问题
//!
//! | 判据 | 问题 | 手段 |
//! |---|---|---|
//! | `T-SCH-01` | 对象在内存里长什么样 | 从 `C11` §7 的**表**读偏移，与 `offset_of!` 比 |
//! | `T-SCH-02/03` | 四个操作在每种状态下该发生什么 | 与 `rgoc-interp` 的 `ChanObj` **逐格差分** |
//! | `T-SCH-04` | M9 有没有偷偷交 native 并发 | 扫源码找 `rt_go` / `rt_select` |
//!
//! # 为什么布局期望值从**文档**读（`R-51-1`）
//!
//! 抄进测试的期望值只能证明「实现与那份副本一致」。M6 的教训（`R-51-1`）是：
//! 把 `C10` 的示例改成从文档读之后，一次抓出三处文档错。这里同理 ——
//! `C11` §7 的表格被 `<!-- hchan-layout:begin/end -->` 包住，测试直接解它。
//! 于是「文档与实现漂了」和「实现与文档漂了」是**同一条**判据。
//!
//! # 为什么是差分而不是「把表抄两遍」
//!
//! `M8` 的 `ChanObj` 已经有一张被 115 条断言钉过的真值表。再在这里抄一遍，
//! 两份抄件会各自漂；**跑同一张状态矩阵、逐格比结果**才叫「同一张表」。
//! 这一条也是 `M8` 那张表的**再验证**：如果 M8 的某个格子是错的，这里会红。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rgoc_arm64::hchan::{
    ChanState, CloseOutcome, HCHAN_SIZE, RecvOutcome, SUDOG_SIZE, SendOutcome,
};
use rgoc_interp::chan::{
    ChanObj, CloseOutcome as M8Close, RecvOutcome as M8Recv, SendOutcome as M8Send,
};
use rgoc_interp::chan::{Waiter, WaiterHandle};
use rgoc_interp::value::Value;
use rgoc_types::TypeId;

// ── 装置 ────────────────────────────────────────────────────────────

/// 仓库根：`rgoc/crates/rgoc-driver` 往上三层。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("仓库根不可达")
}

/// 取 `C11` 里被 `<!-- <tag>:begin -->` / `<!-- <tag>:end -->` 包住的那张表。
///
/// 返回 `(偏移, 大小, 字段名)`；`大小` 为 `None` 的是「对象总大小」行。
/// 填充行（字段名是 `_（填充）_`）跳过 —— 它不是字段，是编译器的空档。
fn c11_table(tag: &str) -> Vec<(usize, Option<usize>, String)> {
    let path = repo_root().join("docs/contracts/C11-concurrency.md");
    let text = std::fs::read_to_string(&path).expect("读不到 C11");
    let begin = format!("<!-- {tag}:begin -->");
    let end = format!("<!-- {tag}:end -->");
    let body = text
        .split(begin.as_str())
        .nth(1)
        .unwrap_or_else(|| panic!("C11 里找不到 {begin}"))
        .split(end.as_str())
        .next()
        .expect("有 begin 就该有 end");
    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() < 3 || cells[0] == "偏移" || cells[0].starts_with("---") {
            continue;
        }
        let Ok(off) = cells[0].parse::<usize>() else {
            continue;
        };
        let size = cells[1].parse::<usize>().ok();
        // 字段名带反引号；`_（填充）_` / `_（对象总大小）_` 是说明行
        let Some(name) = cells[2].strip_prefix('`').and_then(|s| s.strip_suffix('`')) else {
            if let Some(total) = cells[2].strip_prefix("_（对象总大小）_") {
                let _ = total;
                out.push((off, None, String::new()));
            }
            continue;
        };
        out.push((off, size, name.to_string()));
    }
    assert!(!out.is_empty(), "C11 的 {tag} 表解析为空（标记被删了？）");
    out
}

// ── T-SCH-01：布局 == C11 §7 的表 ────────────────────────────────────

#[test]
fn t_sch_01_hchan_布局与_c11_一致() {
    use std::mem::{offset_of, size_of};

    let mut total = None;
    let mut seen = 0usize;
    for (off, size, name) in c11_table("hchan-layout") {
        if name.is_empty() {
            total = Some(off);
            continue;
        }
        seen += 1;
        // `recvq.head` 这种复合名：先到字段，再到子字段
        let got_off = match name.as_str() {
            "qcount" => offset_of!(rgoc_arm64::hchan::Hchan, qcount),
            "dataqsiz" => offset_of!(rgoc_arm64::hchan::Hchan, dataqsiz),
            "buf" => offset_of!(rgoc_arm64::hchan::Hchan, buf),
            "elemsize" => offset_of!(rgoc_arm64::hchan::Hchan, elemsize),
            "closed" => offset_of!(rgoc_arm64::hchan::Hchan, closed),
            "elemtype" => offset_of!(rgoc_arm64::hchan::Hchan, elemtype),
            "sendx" => offset_of!(rgoc_arm64::hchan::Hchan, sendx),
            "recvx" => offset_of!(rgoc_arm64::hchan::Hchan, recvx),
            "lock" => offset_of!(rgoc_arm64::hchan::Hchan, lock),
            "recvq.head" => {
                offset_of!(rgoc_arm64::hchan::Hchan, recvq)
                    + offset_of!(rgoc_arm64::hchan::Waitq, head)
            }
            "recvq.tail" => {
                offset_of!(rgoc_arm64::hchan::Hchan, recvq)
                    + offset_of!(rgoc_arm64::hchan::Waitq, tail)
            }
            "sendq.head" => {
                offset_of!(rgoc_arm64::hchan::Hchan, sendq)
                    + offset_of!(rgoc_arm64::hchan::Waitq, head)
            }
            "sendq.tail" => {
                offset_of!(rgoc_arm64::hchan::Hchan, sendq)
                    + offset_of!(rgoc_arm64::hchan::Waitq, tail)
            }
            other => panic!("C11 里出现了实现没有的字段 `{other}`"),
        };
        assert_eq!(
            got_off, off,
            "字段 `{name}` 的偏移：实现 {got_off} vs C11 {off}"
        );
        // 大小：用 `size_of` 家族逐个给（`size_of_val` 要实例，这里不造实例）
        let got_size = match name.as_str() {
            "qcount" | "dataqsiz" | "sendx" | "recvx" => size_of::<usize>(),
            "buf" => size_of::<*mut u8>(),
            "elemsize" => size_of::<u16>(),
            "closed" => size_of::<u32>(),
            "elemtype" => size_of::<*const rgoc_meta::desc::TypeDesc>(),
            "lock" => size_of::<u64>(),
            // 队列的 head/tail 就是指针
            _ => size_of::<*mut rgoc_arm64::hchan::Sudog>(),
        };
        assert_eq!(
            Some(got_size),
            size,
            "字段 `{name}` 的大小：实现 {got_size} vs C11 {size:?}"
        );
    }
    assert_eq!(
        seen, 13,
        "C11 的 hchan 表应有 13 个具名字段（含 4 个队列指针）"
    );
    assert_eq!(
        total,
        Some(size_of::<rgoc_arm64::hchan::Hchan>()),
        "对象总大小"
    );
    assert_eq!(
        size_of::<rgoc_arm64::hchan::Hchan>(),
        HCHAN_SIZE,
        "常量 `HCHAN_SIZE` 要与 `size_of` 一致"
    );

    // Sudog
    let mut sudog_total = None;
    let mut sudog_seen = 0usize;
    for (off, _size, name) in c11_table("sudog-layout") {
        if name.is_empty() {
            sudog_total = Some(off);
            continue;
        }
        sudog_seen += 1;
        let got = match name.as_str() {
            "g" => offset_of!(rgoc_arm64::hchan::Sudog, g),
            "elem" => offset_of!(rgoc_arm64::hchan::Sudog, elem),
            "next" => offset_of!(rgoc_arm64::hchan::Sudog, next),
            "prev" => offset_of!(rgoc_arm64::hchan::Sudog, prev),
            "token" => offset_of!(rgoc_arm64::hchan::Sudog, token),
            "done" => offset_of!(rgoc_arm64::hchan::Sudog, done),
            other => panic!("C11 的 Sudog 表里出现了实现没有的字段 `{other}`"),
        };
        assert_eq!(got, off, "Sudog.`{name}` 的偏移：实现 {got} vs C11 {off}");
    }
    assert_eq!(sudog_seen, 6, "Sudog 应有 6 个具名字段");
    assert_eq!(
        sudog_total,
        Some(size_of::<rgoc_arm64::hchan::Sudog>()),
        "Sudog 总大小"
    );
    assert_eq!(size_of::<rgoc_arm64::hchan::Sudog>(), SUDOG_SIZE);
}

/// 状态机的字段必须是布局的**投影**（不许凭空多一个字段）。
///
/// 两个类型分开写（`R-9-37` 的代价），所以要有这条守卫：新增一个
/// `ChanState` 字段而忘了在 `Hchan` 里留位，就会红。
#[test]
fn t_sch_01b_状态机是布局的投影() {
    use rgoc_arm64::hchan::{LAYOUT_FIELDS, STATE_FIELDS};
    for f in STATE_FIELDS {
        assert!(
            LAYOUT_FIELDS.contains(&f),
            "`ChanState` 的字段 `{f}` 在 `Hchan` 的布局表里没有对应项 —— \
             两者漂了（`R-9-37` 的代价就是这个守卫）"
        );
    }
    // 反向不要求相等，但纯布局项必须**不**进状态机（它们归 M11）
    for f in ["buf", "elemsize"] {
        assert!(
            !STATE_FIELDS.contains(&f),
            "`{f}` 是纯布局项，状态机不该碰它（这一层不搬字节）"
        );
    }
}

// ── T-SCH-02/03：与 M8 的 `ChanObj` 差分 ─────────────────────────────

/// 一个格子里的状态（两侧用同一份描述构造）。
#[derive(Debug, Clone, Copy)]
struct Cell {
    cap: usize,
    filled: usize,
    closed: bool,
    recv_waiters: usize,
    send_waiters: usize,
}

impl Cell {
    fn new(
        cap: usize,
        filled: usize,
        closed: bool,
        recv_waiters: usize,
        send_waiters: usize,
    ) -> Self {
        assert!(filled <= cap, "填的不能超过容量");
        Self {
            cap,
            filled,
            closed,
            recv_waiters,
            send_waiters,
        }
    }
}

fn handle(gid: usize, token: u64) -> WaiterHandle {
    Rc::new(RefCell::new(Waiter {
        g: gid,
        token,
        slot: None,
        done: false,
    }))
}

/// M8 侧：按 `Cell` 造一个通道。
fn m8_of(c: Cell) -> (ChanObj, Vec<WaiterHandle>) {
    let mut ch = ChanObj::new(TypeId::from_index(0), c.cap);
    for i in 0..c.filled {
        assert_eq!(
            ch.try_send(&Value::Int(i as i128)),
            M8Send::Done { woke: None },
            "装填阶段不该阻塞"
        );
    }
    let mut hs = Vec::new();
    for k in 0..c.recv_waiters {
        let h = handle(100 + k, 0);
        ch.park_recv(h.clone());
        hs.push(h);
    }
    for k in 0..c.send_waiters {
        let h = handle(200 + k, 0);
        // 阻塞发送方登记时 `slot` 里得有值（`ChanObj` 的 recv 会取走它）
        h.borrow_mut().slot = Some(Value::Int(-1 - k as i128));
        ch.park_send(h.clone());
        hs.push(h);
    }
    if c.closed {
        let _ = ch.close();
    }
    (ch, hs)
}

/// 镜像侧：按 `Cell` 造一个状态机。
fn mirror_of(c: Cell) -> ChanState {
    let mut st = ChanState::new(c.cap);
    for _ in 0..c.filled {
        assert_eq!(st.try_send(), SendOutcome::Done { woke: None });
    }
    for k in 0..c.recv_waiters {
        st.park_recv(rgoc_arm64::hchan::WaiterRef {
            id: (100 + k) as u32,
            token: 0,
        });
    }
    for k in 0..c.send_waiters {
        st.park_send(rgoc_arm64::hchan::WaiterRef {
            id: (200 + k) as u32,
            token: 0,
        });
    }
    if c.closed {
        let _ = st.close();
    }
    st
}

/// 一个 `Cell` 的全部状态，转成可比的字符串（失败时能直接看出差在哪）。
fn snapshot_m8(ch: &ChanObj) -> String {
    format!(
        "len={} cap={} closed={} recvq={} sendq={}",
        ch.len(),
        ch.cap(),
        ch.is_closed(),
        !ch.recvq_is_empty(),
        !ch.sendq_is_empty()
    )
}

fn snapshot_mirror(st: &ChanState) -> String {
    format!(
        "len={} cap={} closed={} recvq={} sendq={}",
        st.qcount(),
        st.cap(),
        st.is_closed(),
        st.recvq_len() > 0,
        st.sendq_len() > 0
    )
}

/// 状态矩阵：(空 / 半 / 满 / 已关) × (有无等待者)。
fn matrix() -> Vec<Cell> {
    let mut out = Vec::new();
    for cap in [0usize, 1, 3] {
        for filled in 0..=cap {
            for closed in [false, true] {
                for recv_waiters in 0..=1 {
                    for send_waiters in 0..=1 {
                        out.push(Cell::new(cap, filled, closed, recv_waiters, send_waiters));
                    }
                }
            }
        }
    }
    out
}

#[test]
fn t_sch_02_三操作与_m8_逐格一致() {
    let cells = matrix();
    let mut compared = 0usize;
    let mut bad: Vec<String> = Vec::new();

    for c in &cells {
        let cell = *c;
        // ⚠️ `closed` 与「有等待者」**互斥**：`close` 会清空两侧队列（`C11` §3.2），
        // 所以「已关 + 队列非空」是不可达状态 —— 那两个组合在造状态时就退化成
        // 「已关 + 队列空」。这里**明说**这件事，并断言「退化确实发生了」，
        // 免得把 56 个格子读成「覆盖了 56 种状态」。
        if cell.closed {
            let (ch, _) = m8_of(cell);
            let st = mirror_of(cell);
            assert!(
                ch.recvq_is_empty() && ch.sendq_is_empty(),
                "`close` 之后队列必空（M8）"
            );
            assert_eq!(
                (st.recvq_len(), st.sendq_len()),
                (0, 0),
                "`close` 之后队列必空"
            );
        }
        // ── recv
        let (mut ch, _hs) = m8_of(cell);
        let mut st = mirror_of(cell);
        let pre_filled = ch.len();
        let m8 = ch.try_recv();
        let mine = st.try_recv();
        compared += 1;
        let same = match (&m8, &mine) {
            (M8Recv::Got { woke, .. }, RecvOutcome::Got { from_buf, woke: w }) => {
                // `from_buf` 不是 M8 的字段（它不关心值从哪来），用**操作前**的
                // 缓冲长度推：非空就一定是从缓冲取的。这条推断本身也是判据 ——
                // 若状态机把「从缓冲取」写成「从发送方取」，这里会红。
                let want_from_buf = pre_filled > 0;
                woke.map(|g| g as u32) == w.map(|x| x.id) && *from_buf == want_from_buf
            }
            (M8Recv::GotZeroClosed, RecvOutcome::ZeroClosed) => true,
            (M8Recv::Blocked, RecvOutcome::Blocked) => true,
            _ => false,
        };
        if !same {
            bad.push(format!("recv {cell:?}：M8={m8:?} 镜像={mine:?}"));
        }
        if snapshot_m8(&ch) != snapshot_mirror(&st) {
            bad.push(format!(
                "recv {cell:?} 之后的态：M8={} 镜像={}",
                snapshot_m8(&ch),
                snapshot_mirror(&st)
            ));
        }

        // ── send
        let (mut ch, _hs) = m8_of(cell);
        let mut st = mirror_of(cell);
        let m8 = ch.try_send(&Value::Int(7));
        let mine = st.try_send();
        compared += 1;
        let same = match (&m8, &mine) {
            (M8Send::Done { woke }, SendOutcome::Done { woke: w }) => {
                woke.map(|g| g as u32) == w.map(|x| x.id)
            }
            (M8Send::Blocked, SendOutcome::Blocked) => true,
            (M8Send::Closed, SendOutcome::Closed) => true,
            _ => false,
        };
        if !same {
            bad.push(format!("send {cell:?}：M8={m8:?} 镜像={mine:?}"));
        }
        if snapshot_m8(&ch) != snapshot_mirror(&st) {
            bad.push(format!(
                "send {cell:?} 之后的态：M8={} 镜像={}",
                snapshot_m8(&ch),
                snapshot_mirror(&st)
            ));
        }

        // ── close
        let (mut ch, _hs) = m8_of(cell);
        let mut st = mirror_of(cell);
        let m8 = ch.close();
        let mine = st.close();
        compared += 1;
        let same = match (&m8, &mine) {
            (M8Close::Woken(gs), CloseOutcome::Woken { recv, send }) => {
                // M8 的顺序是「先接收方、后发送方」
                let want_recv = gs.iter().filter(|g| **g < 200).count();
                let want_send = gs.len() - want_recv;
                want_recv == *recv && want_send == *send && *recv == cell.recv_waiters
            }
            (M8Close::AlreadyClosed, CloseOutcome::AlreadyClosed) => true,
            _ => false,
        };
        if !same {
            bad.push(format!("close {cell:?}：M8={m8:?} 镜像={mine:?}"));
        }
        if snapshot_m8(&ch) != snapshot_mirror(&st) {
            bad.push(format!(
                "close {cell:?} 之后的态：M8={} 镜像={}",
                snapshot_m8(&ch),
                snapshot_mirror(&st)
            ));
        }
    }

    assert!(bad.is_empty(), "与 M8 的真值表不一致：\n{}", bad.join("\n"));
    assert_eq!(compared, cells.len() * 3, "每个格子三个操作都要比");
    assert!(
        compared >= 60,
        "矩阵太小（{compared} 次比较），不足以覆盖四张表"
    );
}

#[test]
fn t_sch_03_环形下标与撤销() {
    // ① 环形推进：cap=3，连着送 3 个（sendx 0→1→2→0），再收 2 个（recvx 0→1→2）
    let mut st = ChanState::new(3);
    for _ in 0..3 {
        assert_eq!(st.try_send(), SendOutcome::Done { woke: None });
    }
    assert_eq!(
        (st.qcount(), st.sendx()),
        (3, 0),
        "送满 3 个后 sendx 转回 0"
    );
    assert_eq!(st.try_send(), SendOutcome::Blocked, "满 ⇒ 阻塞");
    for _ in 0..2 {
        assert!(matches!(
            st.try_recv(),
            RecvOutcome::Got {
                from_buf: true,
                woke: None
            }
        ));
    }
    assert_eq!((st.qcount(), st.recvx()), (1, 2), "收 2 个后 recvx 在 2");

    // ② `revoke` 幂等且只删同名 token
    let mut st = ChanState::new(0);
    use rgoc_arm64::hchan::WaiterRef;
    st.park_recv(WaiterRef { id: 1, token: 7 });
    st.park_recv(WaiterRef { id: 2, token: 8 });
    st.park_send(WaiterRef { id: 3, token: 7 });
    st.revoke(7);
    assert_eq!((st.recvq_len(), st.sendq_len()), (1, 0), "只删 token=7 的");
    st.revoke(7);
    assert_eq!((st.recvq_len(), st.sendq_len()), (1, 0), "幂等");

    // ③ 撤销之后 `try_recv` 拿到的仍是**留下的那个**（FIFO 顺序没被打乱）
    let woke = match st.try_send() {
        SendOutcome::Done { woke } => woke.expect("有等待的接收方"),
        other => panic!("应当交付给等待者：{other:?}"),
    };
    assert_eq!(woke.id, 2, "留下的那个是 id=2");
}

// ── T-SCH-04：M9 不交 native 并发 ───────────────────────────────────

#[test]
fn t_sch_04_m9_不交_native_并发() {
    // 这条现在是绿的 —— 它防的是**未来**：有人顺手在 `rgoc-arm64` 或 `rt.c`
    // 里加 `rt_go` / `rt_select`，就等于 M9 悄悄交了 native 并发，
    // 而 M9 的判据（语料 69/0/0）一条都覆盖不到它。
    let root = repo_root();
    let mut files: Vec<PathBuf> = Vec::new();
    collect(&root.join("rgoc/crates/rgoc-arm64/src"), &mut files);
    collect(&root.join("runtime"), &mut files);
    assert!(files.len() > 5, "扫描面太小：{}", files.len());

    let mut hits: Vec<String> = Vec::new();
    for f in &files {
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            // ⚠️ **M11 起 `rt_go*` 是允许的**（`M11-design.md` §3.5–§3.7）：
            // `rt_goexit` / `rt_go_trampoline` / `rt_schedule` 都是调度骨架的
            // 正常符号。这条守卫原本（M9）是「不交 native 并发」，M11 改了那个
            // 前提 —— 所以它现在只防**还没做的** `select`（Task 22 之后再放开）。
            //
            // 为什么保留这条而不是删掉：它防的是「代码里悄悄出现一个没人设计的
            // 并发符号」。删了就再没人看这件事；改白名单则每次放开都要人确认一次。
            for needle in ["rt_select"] {
                // 允许在注释里说明「不交」这件事（本文件自己就写了）
                let is_comment = {
                    let t = line.trim_start();
                    t.starts_with("//") || t.starts_with("/*") || t.starts_with('*')
                };
                if line.contains(needle) && !is_comment {
                    hits.push(format!("{}:{}: {line}", f.display(), i + 1));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "M9 不交 native 并发（`SCH-1`），但出现了这些符号：\n{}",
        hits.join("\n")
    );
}

/// 递归收集 `.rs` / `.c` 文件。
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if matches!(p.extension().and_then(|s| s.to_str()), Some("rs" | "c")) {
            out.push(p);
        }
    }
}

// ── 一个容易写错的地方：`closed` 必须排第一 ─────────────────────────

#[test]
fn t_sch_02b_send_的_closed_检查排第一() {
    // `R-8-2` 的教训：`closed` 排在「buf 未满」之后，`close(c); c <- v`
    // （cap > 0 且缓冲未满）会**静默把值塞进缓冲** —— 不 panic、不报错。
    let mut st = ChanState::new(3);
    st.try_send();
    assert_eq!(st.close(), CloseOutcome::Woken { recv: 0, send: 0 });
    assert_eq!(
        st.try_send(),
        SendOutcome::Closed,
        "已关且缓冲**还有空间**时也必须是 Closed（不是 Done）"
    );
    assert_eq!(st.qcount(), 1, "被拒的发送不该改变 qcount");

    // 同一个格子走 M8，确认两边都是 Closed（这条也顺带钉住 M8 没退化）
    let mut ch = ChanObj::new(TypeId::from_index(0), 3);
    ch.try_send(&Value::Int(1));
    let _ = ch.close();
    assert_eq!(ch.try_send(&Value::Int(2)), M8Send::Closed);
    assert_eq!(ch.len(), 1);
}
