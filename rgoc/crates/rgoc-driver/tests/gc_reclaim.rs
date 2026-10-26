//! 回收判据（M10/Task 11–13、15；`C12` §3.5–§3.6）。
//!
//! # Phase 1 的 GC **只扫全局根**
//!
//! 栈根在 Phase 2（帧记录链）。所以这一轮的 fixture 必须满足一条硬约束：
//! **活对象全部能从全局变量到达**。局部变量持有的指针在 stress 下会被收掉
//! —— 那不是 bug，是 Phase 1 的已知边界（`M10-plan.md` Global Constraint 7）。
//!
//! 这条约束让判据变窄，但**不能**因此跳过：`churn()` 里的死块回收、
//! 全局根保活、确定性三件事都不依赖栈根，它们现在就该绿。
//!
//! # 为什么 `stress=1`（每次分配都收）
//!
//! 小 fixture 只有几百次分配，`=1` 完全跑得动。语料用 `=64`（`R-10-10`：
//! `gc1.go` 的 1e5 次分配在 `=1` 下是 O(n²)，跑不完）。

mod support;

use support::gc_probe::run_with_env;

/// 全局根保活（`T-ROOT-12` 的 Phase 1 形态）。
///
/// # 这条钉的是什么
///
/// `G` 是**全局** `*[1024]byte`（块 = `align16(16+1024)` = **1040 字节**）。
/// 如果 `mark_roots` 没扫 `rt_globals`，`G` 指向的块会被回收并**被 churn
/// 复用**（`gc_alloc` 恒清零）⇒ `println` 打出来的不是 49。
///
/// # ⚠️ 两个条件缺一不可（这条判据的牙，踩了两次）
///
/// 第一版：`2000 × new([64]byte)` = 160 KB —— bump 就够，`free_take` 根本不会
/// 被调用 ⇒ 「回收了但没复用」，`G[0]` 照样读到 42，变异**是绿的**。
///
/// 第二版：`4000 × new([8192]byte)` —— 分配量够了，但 **8208 > 1040**，
/// `free_take` 的 first-fit 装不进 `G` 那块 ⇒ 还是不复用，变异**还是绿的**。
///
/// 所以 churn 的**单块大小必须 <= `G` 的块大小**（`new([1000]byte)` ⇒ 1024），
/// 且次数要够多（2 万次 = 20 MB）。`stress=16` 而不是 `=1`：2 万次 × 平均
/// 块数在 `=1` 下是 O(n²)。
#[test]
fn t_root_12_全局根保活() {
    let src = r#"package main

var G *[1024]byte

func churn() {
	for i := 0; i < 20000; i++ {
		_ = new([1000]byte)
	}
}

func main() {
	G = new([1024]byte)
	G[0] = 42
	G[1023] = 7
	churn()
	println(G[0] + G[1023])
}
"#;
    let (out, s) = run_with_env("gc_root_global", src, &[("RGOC_GC_STRESS", "16")])
        .expect("编译 + 运行不该失败");
    assert_eq!(
        out.stdout, "49\n",
        "全局根没被扫 ⇒ `G` 指向的块被回收后内容被复用（stderr: {}）",
        out.stderr
    );
    s.assert_clean().expect("自检计数器");
    assert!(s.collections >= 1, "stress 下必须真的收过：{s:?}");
}

/// 死块被回收 + 堆**复用**（`T-GC-04` 的 Phase 1 形态）。
///
/// 1000 次 `new([64]byte)` 都没人持有 ⇒ 每次 stress 都该收掉前一个。
/// 三条断言合起来才说明「回收 + 复用」都生效：
///
/// * `frees >= 900`：真的回收了（不是「没扫到所以没死」）；
/// * `live <= 1024`：存活字节有界（只有最后一次分配的那一个块活着）；
/// * `chunks == 1`：**复用**生效 —— 1000 × 80 字节 = 80 KB，不复用的话
///   也只要一个 chunk，所以这条单独不够；真正钉复用是 `heap` 不涨到 2 个
///   chunk（下面那条 `t_heap_04`）。
#[test]
fn t_gc_04_死块被回收() {
    let src = r#"package main

func main() {
	for i := 0; i < 1000; i++ {
		_ = new([64]byte)
	}
	println("ok")
}
"#;
    let (out, s) =
        run_with_env("gc_reclaim_churn", src, &[("RGOC_GC_STRESS", "1")]).expect("编译 + 运行");
    assert_eq!(out.stdout, "ok\n");
    assert!(
        s.frees >= 900,
        "只回收了 {} 块 —— 清扫没生效：{s:?}",
        s.frees
    );
    assert!(
        s.live <= 1024,
        "存活字节 {} 太多（应当只剩最后一次分配的那个块）：{s:?}",
        s.live
    );
    assert_eq!(s.chunks, 1, "1000 次小分配只该要一个 chunk：{s:?}");
    s.assert_clean().expect("自检计数器");
}

/// 确定性（`T-GC-08` 的 Phase 1 形态）：同程序两次跑，三个数完全相同。
///
/// # 为什么这条重要
///
/// 判据里到处在断言 `collections >= 1` / `live <= N` 这种**区间**。区间判据
/// 在「每次跑都不一样」时也可能恒真。这条把「同一个程序的行为是确定的」
/// 变成可断言的 —— 它也是 M10 之后所有 GC 判据可信的前提。
#[test]
fn t_gc_08_确定性() {
    let src = r#"package main

var G *[256]byte

func main() {
	for i := 0; i < 500; i++ {
		_ = new([128]byte)
	}
	G = new([256]byte)
	G[0] = 1
	println(G[0])
}
"#;
    let env: &[(&str, &str)] = &[("RGOC_GC_STRESS", "1")];
    let (o1, s1) = run_with_env("gc_det_a", src, env).expect("第一次");
    let (o2, s2) = run_with_env("gc_det_b", src, env).expect("第二次");
    assert_eq!(o1.stdout, "1\n");
    assert_eq!(o2.stdout, "1\n");
    assert_eq!(
        (s1.collections, s1.live, s1.heap, s1.frees),
        (s2.collections, s2.live, s2.heap, s2.frees),
        "同一程序两次跑的 GC 读数必须完全相同\n第一次 {s1:?}\n第二次 {s2:?}"
    );
    assert!(s1.collections >= 1);
}

/// `RGOC_GC=0` 时一次都不收（`T-CORP-05` 的对照态）。
///
/// 这条同时是「`off` 态与 M9 等价」的**结构**证据：`collections == 0` 且
/// `frees == 0` ⇒ 清扫路径根本没跑过。
#[test]
fn t_gc_off_不收集() {
    let src = r#"package main

func main() {
	for i := 0; i < 200; i++ {
		_ = new([64]byte)
	}
	println("ok")
}
"#;
    let (out, s) = run_with_env("gc_off", src, &[("RGOC_GC", "0"), ("RGOC_GC_STRESS", "1")])
        .expect("编译 + 运行");
    assert_eq!(out.stdout, "ok\n");
    assert_eq!(s.collections, 0, "`RGOC_GC=0` 必须一次都不收：{s:?}");
    assert_eq!(s.frees, 0, "`RGOC_GC=0` 时清扫不该跑：{s:?}");
    assert_eq!(s.live, 0, "`RGOC_GC=0` 时 live 没有来源：{s:?}");
    assert_eq!(s.mode, support::gc_probe::GcMode::Off);
}

/// 堆**复用**（`T-HEAP-04`，Task 10）：累计分配 100 MB 但只占 1 个 chunk。
///
/// # 为什么分配量必须远超 chunk 容量
///
/// 一个 chunk 是 16 MiB。分配 4 MB 的话，**不复用**也只要一个 chunk ——
/// 判据区分不出「复用」与「没复用」。所以这里分配 100 MB（1e5 × 1 KiB），
/// 不复用需要 7 个 chunk，复用只需要 1 个。
///
/// # 为什么用 `stress=64` 而不是 `=1`
///
/// `=1` 是每次分配都收：1e5 次 × 平均 8000 块 = 8e8 次块扫描，太慢。
/// `=64` 是每 64 次收一次 ⇒ 1563 次 GC × 16000 块 = 2.5e7，够快且够密
/// （任何漏扫的根最多在 64 次分配后暴露）。
#[test]
fn t_heap_04_堆复用() {
    let src = r#"package main

func main() {
	for i := 0; i < 100000; i++ {
		_ = new([1024]byte)
	}
	println("ok")
}
"#;
    let (out, s) =
        run_with_env("gc_reuse", src, &[("RGOC_GC_STRESS", "64")]).expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "ok\n");
    assert!(
        s.frees >= 90000,
        "回收了 {} 块（应当接近 1e5）：{s:?}",
        s.frees
    );
    assert_eq!(
        s.chunks, 1,
        "100 MB 的累计分配只该占 1 个 16 MiB chunk —— 多于 1 说明没复用：{s:?}"
    );
    assert!(
        s.live <= 2 * 1024 * 1024,
        "存活字节 {} 超过 2 MiB：{s:?}",
        s.live
    );
    s.assert_clean().expect("自检计数器");
}
