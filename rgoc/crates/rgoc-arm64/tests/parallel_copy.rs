//! 并行拷贝的判据（`T-NAT-06/07/08`）。
//!
//! # 判据是「模拟执行」，不是「看形状」
//!
//! 断言「产生了 3 条 move」只证明算法输出了三条指令，不证明**顺序**对。
//! 所以每条用例都把 move 序列真的执行一遍（`simulate`），再问「每个槽里是什么」。
//! 顺序错（先写后被读）在这条判据下必红，在「数条数」下可能全绿。

use std::collections::BTreeMap;

use rgoc_arm64::parallel_copy::{Move, schedule, simulate};

/// 跑一次：`pairs` 是 `(目标, 源)`，`init` 是「偏移 → 初值」。
fn run(pairs: &[(i32, i32)], init: &[(i32, i64)]) -> BTreeMap<i32, i64> {
    let mut values: BTreeMap<i32, i64> = init.iter().copied().collect();
    let moves = schedule(pairs, TMP);
    simulate(&moves, &mut values);
    values
}

const TMP: i32 = -999;

#[test]
fn 二值交换() {
    // a←b, b←a（初值 a=1, b=2）⇒ 交换后 a=2, b=1
    let out = run(&[(-24, -32), (-32, -24)], &[(-24, 1), (-32, 2)]);
    assert_eq!(out[&-24], 2, "a 应是旧 b");
    assert_eq!(out[&-32], 1, "b 应是旧 a");
}

#[test]
fn 二值交换必须用临时格() {
    // 顺序搬（不打断环）会得到 (2, 2)。这条钉住「环必须被打断」这个事实本身：
    // 交换两条的 move 序列长度必须是 3（tmp + 两条）。
    let moves = schedule(&[(-24, -32), (-32, -24)], TMP);
    assert_eq!(moves.len(), 3, "交换环需要 3 条：{moves:?}");
    assert!(
        moves.iter().any(|m| m.dst == TMP),
        "必须有一条写临时格的：{moves:?}"
    );
}

#[test]
fn 三值轮换() {
    // a←b, b←c, c←a（a=1,b=2,c=3）⇒ a=2, b=3, c=1
    let out = run(
        &[(-24, -32), (-32, -40), (-40, -24)],
        &[(-24, 1), (-32, 2), (-40, 3)],
    );
    assert_eq!((out[&-24], out[&-32], out[&-40]), (2, 3, 1));
}

#[test]
fn 无环时直接搬() {
    // a←x, b←y，源与目标不重叠 ⇒ 2 条，且不用临时格。
    let moves = schedule(&[(-24, -100), (-32, -108)], TMP);
    assert_eq!(
        moves,
        vec![
            Move {
                dst: -24,
                src: -100
            },
            Move {
                dst: -32,
                src: -108
            },
        ]
    );
    let out = run(&[(-24, -100), (-32, -108)], &[(-100, 7), (-108, 9)]);
    assert_eq!((out[&-24], out[&-32]), (7, 9));
}

#[test]
fn 链式搬不破坏中间值() {
    // a←b, b←c（a,b,c 初值 1,2,3）⇒ a=2, b=3
    // 顺序 a←b 再 b←c 也对；反过来就错。这条钉住「目标不被后面读到」这条判据。
    let out = run(&[(-24, -32), (-32, -40)], &[(-24, 1), (-32, 2), (-40, 3)]);
    assert_eq!((out[&-24], out[&-32]), (2, 3));
}

#[test]
fn 自赋值不发指令() {
    let moves = schedule(&[(-24, -24)], TMP);
    assert!(moves.is_empty(), "d←d 不必发：{moves:?}");
}

#[test]
fn 空输入() {
    assert!(schedule(&[], TMP).is_empty());
}

#[test]
fn 同一输入两次排出同一序列() {
    // 确定性（`T-NAT-48`）：顺序是 `.s` 的一部分。
    let pairs = [(-24, -32), (-32, -40), (-40, -24)];
    assert_eq!(schedule(&pairs, TMP), schedule(&pairs, TMP));
}

#[test]
fn 环里有自环也能排() {
    // a←b, b←b（b 自赋值）, c←a：b←b 是空操作，剩下 a←b, c←a 是一个环。
    let out = run(
        &[(-24, -32), (-32, -32), (-40, -24)],
        &[(-24, 1), (-32, 2), (-40, 3)],
    );
    assert_eq!((out[&-24], out[&-32], out[&-40]), (2, 2, 1));
}

#[test]
fn 四条以上的长环() {
    // 4 环：a←b, b←c, c←d, d←a（1,2,3,4）⇒ 2,3,4,1
    let out = run(
        &[(-24, -32), (-32, -40), (-40, -48), (-48, -24)],
        &[(-24, 1), (-32, 2), (-40, 3), (-48, 4)],
    );
    assert_eq!((out[&-24], out[&-32], out[&-40], out[&-48]), (2, 3, 4, 1));
}
