//! M8/Task 2：六个并发节点的**形状**判据。
//!
//! 这一层只判「节点装得下信息」，不判行为 —— 行为在 lowering（Task 7–13）
//! 与解释器（Task 15+）。所以断言用 `Debug` 里的字段名，而不是执行结果：
//! 如果哪天有人把 `recv_targets` 改回 `Option<Place>`（`R2` 之前的形状），
//! `case v, ok := <-c` 就装不下了，而这条断言是唯一会红的地方。

use rgoc_hir::{Block, Expr, ExprKind, Place, RangeKind, SelectClause, SelectKind, SlotId, Stmt};
use rgoc_types::TypeId;

fn nil() -> Expr {
    Expr::new(ExprKind::Nil, TypeId::from_index(0))
}

fn slot(i: u32) -> Place {
    Place::Slot(SlotId(i))
}

#[test]
fn go_语句装得下调用的表达式() {
    let s = Stmt::Go { call: nil() };
    let text = format!("{s:?}");
    assert!(text.contains("Go"), "{text}");
    assert!(text.contains("call"), "必须能装下调用的表达式：{text}");
}

#[test]
fn 发送语句装得下通道与值() {
    let s = Stmt::Send {
        chan: nil(),
        value: nil(),
    };
    let text = format!("{s:?}");
    assert!(text.contains("Send"), "{text}");
    assert!(
        text.contains("chan") && text.contains("value"),
        "两个操作数各要一个字段（求值顺序 chan 先、value 后）：{text}"
    );
}

#[test]
fn select_的三种子句都装得下() {
    let clauses = vec![
        // case v, ok := <-c:  —— 两个目标（`R2`）
        SelectClause {
            kind: SelectKind::Recv,
            chan: Some(nil()),
            recv_targets: vec![slot(1), slot(2)],
            send_value: None,
            body: Block::default(),
        },
        // case c <- 1:
        SelectClause {
            kind: SelectKind::Send,
            chan: Some(nil()),
            recv_targets: Vec::new(),
            send_value: Some(nil()),
            body: Block::default(),
        },
        // default:
        SelectClause {
            kind: SelectKind::Default,
            chan: None,
            recv_targets: Vec::new(),
            send_value: None,
            body: Block::default(),
        },
    ];
    let s = Stmt::Select { clauses };
    let text = format!("{s:?}");
    assert!(text.contains("Select"), "{text}");
    assert!(
        text.contains("recv_targets"),
        "`case v, ok := <-c` 需要**两个**目标位（`R2`）：{text}"
    );

    // 三个 kind 互不相等 —— 防止有人把它们合成一个 bool。
    assert_ne!(SelectKind::Send, SelectKind::Recv);
    assert_ne!(SelectKind::Recv, SelectKind::Default);
}

#[test]
fn 接收表达式带_comma_ok() {
    let e = Expr::new(
        ExprKind::Recv {
            ch: Box::new(nil()),
            comma_ok: true,
        },
        TypeId::from_index(0),
    );
    let text = format!("{e:?}");
    assert!(text.contains("Recv"), "{text}");
    assert!(text.contains("comma_ok"), "{text}");
    assert!(text.contains("true"), "comma_ok 要能是 true：{text}");
}

#[test]
fn range_多了_chan_与_func_两种形态() {
    let all = [
        RangeKind::Array,
        RangeKind::Slice,
        RangeKind::Str,
        RangeKind::Map,
        RangeKind::Int,
        RangeKind::Chan,
        RangeKind::Func,
    ];
    // 两两不等：`Chan` 与 `Func` 必须是**新**的判别值，不是复用了 Int/Array。
    for (i, a) in all.iter().enumerate() {
        for b in all.iter().skip(i + 1) {
            assert_ne!(a, b, "{a:?} 与 {b:?} 撞了");
        }
    }
}
