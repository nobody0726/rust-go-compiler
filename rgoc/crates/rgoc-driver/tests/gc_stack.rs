//! 栈根（M10/Task 22–23；`C12` §4.2–§4.4）。
//!
//! # Phase 1 与 Phase 2 的分界
//!
//! Phase 1 的 GC **只扫全局根** —— 局部变量持有的指针会被收掉。所以 Phase 1
//! 的 fixture 全都写成「活对象挂在全局变量上」。
//!
//! Phase 2 接上**帧记录链**之后，这一条不再是限制：下面三个程序里的 `p`
//! 都是**局部变量**，而 `churn()` 期间的每一次 GC 都必须把它们标记成活的。
//!
//! # 三条判据各钉什么
//!
//! | 判据 | 钉的 |
//! |---|---|
//! | `t_root_09` | 帧记录链**通**了：`main` 的 `p` 活过 2 万次分配 |
//! | `t_root_10` | **逐位点**（不是函数级并集）：像地址的整数 `u` 不能被当根 |
//! | `t_root_11` | 三层调用：每层的 epilogue 都摘链（漏一层就扫到已返回的帧）|

mod support;

use support::gc_probe::run_with_env;

/// 栈上的局部指针活过 churn（`T-ROOT-09`）。
///
/// # 这条为什么是 Phase 2 的里程碑
///
/// `p` 只在 `main` 的帧里。GC 在 `churn()` 的分配点触发，此刻栈上有两帧
/// （`main` + `churn`）。要保住 `*p`，必须：
///
/// 1. `main` 的序言把 `prev/fi/sp_idx` 写进帧、`g_frames = fp`；
/// 2. `churn` 的序言接着链上去；
/// 3. GC 从 `g_frames` 走到 `main` 的帧，用**当前位点**的位图找到 `p` 的槽；
/// 4. `mark_ptr` 标记 `p` 指向的块。
///
/// 任何一环漏了，`*p` 读到的是被回收后复用成 0 的内存。
#[test]
fn t_root_09_局部指针活过_churn() {
    let src = r#"package main

func churn() {
	for i := 0; i < 20000; i++ {
		_ = new([64]byte)
	}
}

func main() {
	p := new(int)
	*p = 7
	churn()
	println(*p)
}
"#;
    let (out, s) = run_with_env("gc_stack_local", src, &[("RGOC_GC_STRESS", "16")])
        .expect("编译 + 运行不该失败");
    assert_eq!(
        out.stdout, "7\n",
        "栈根没扫到 ⇒ `*p` 读到被复用的内存（stderr: {}）",
        out.stderr
    );
    s.assert_clean().expect("自检计数器（bad_sp 必须 0）");
    assert!(s.collections >= 1, "stress 下必须真的收过：{s:?}");
}

/// **每个位点用自己那张位图**（`T-ROOT-10`，`C12` §4.4）。
///
/// # 这条钉的是什么
///
/// `sp_idx` 必须在**每条 safepoint 之前**更新。写一次就不动（或者写错下标），
/// GC 会用**别的位点**的位图去扫这一帧 —— 那一刻活着的指针不在那张位图里
/// ⇒ 漏标 ⇒ 活对象被回收。
///
/// 这里造两个「只在后面才活」的指针：`p` 和 `q` 都在**第一条** `new` 之后
/// 才有值，所以函数第 0 号位点（第一条 `new`）的位图里**没有**它们。
/// 如果 `sp_idx` 卡在 0，`churn()` 里的 GC 会用 0 号位图 ⇒ `p` / `q` 全被回收。
///
/// # ⚠️ 为什么判据是**内容**而不是 `bad_ptr`
///
/// 早先这条用 `bad_ptr == 0`（「像地址的整数被当根扫」）。M10 后期把
/// `bad_ptr` 收窄成「解析不到块首 / 块头非法」——「指向已回收的块」不再计入
/// （保守位图下那是常态，见 `R-10-21`）。于是那条判据**没牙了**：
/// 「`sp_idx` 恒 0」的变异实测**是绿的**。
///
/// 现在改成读**内容**：`p[0] + q[0]` 必须等于 42 + 7。漏标时读到的是复用后
/// 的垃圾（`gc_alloc` 清零 ⇒ 通常是 0）。
#[test]
fn t_root_10_每个位点用自己那张位图() {
    let src = r#"package main

func churn() {
	for i := 0; i < 3000; i++ {
		_ = new([64]byte)
	}
}

func main() {
	p := new([1024]byte)
	p[0] = 42
	q := new([1024]byte)
	q[0] = 7
	churn()
	println(p[0] + q[0])
}
"#;
    let (out, s) = run_with_env("gc_root_spidx", src, &[("RGOC_GC_STRESS", "8")])
        .expect("编译 + 运行不该失败");
    assert_eq!(
        out.stdout, "49\n",
        "`sp_idx` 没跟着指令走（用了别的位点的位图？）stderr: {}",
        out.stderr
    );
    s.assert_clean().expect("自检计数器");
}

/// 三层调用：每层的 epilogue 都要摘链（`T-ROOT-11`）。
///
/// # 漏摘链的症状
///
/// `epilogue` 是唯一出口（`F14`），但如果某个函数的 `has_rec` 判定错了
/// （比如「有 safepoint 却没发记录」），它的 `g_frames` 就不会被设成自己的
/// `fp` —— 链会**跳过**它（这没问题）。反过来，如果**发了记录却没摘**
/// （epilogue 里漏了 `g_frames = prev`），链会指向一个**已经返回的帧**：
/// 那块栈随后被 `churn()` 的帧覆盖，GC 就会拿垃圾当 `fi`/`sp_idx` 去扫。
///
/// 三层是为了让「漏摘」在第 2、3 层返回之后**一定**发生（单层可能碰巧没事）。
///
/// # ⚠️ 三层返回之后**必须再分配一次**
///
/// 第一版到这里就 `println` 结束了 —— 三层返回之后**一次 GC 都没有**，
/// 于是「不摘链」的变异实测**是绿的**（`g_frames` 指着死帧，但没人去读它）。
///
/// 加一段 `churn()`：它自己的帧正好落在 `lvl3` 的旧帧地址上（同样的深度）
/// ⇒ 死帧的 `fi` / `sp_idx` 被覆盖成垃圾 ⇒ 下一次 GC 顺着链读到垃圾指针
/// ⇒ 段错误或 `bad_sp`。
#[test]
fn t_root_11_三层帧的链() {
    let src = r#"package main

func churn() {
	for i := 0; i < 20000; i++ {
		_ = new([64]byte)
	}
}

func lvl3(p *int) {
	churn()
	*p = *p + 1
}

func lvl2(p *int) {
	lvl3(p)
}

func lvl1(p *int) {
	lvl2(p)
}

func main() {
	p := new(int)
	*p = 5
	lvl1(p)
	// 三层都返回了；不摘链的话 `g_frames` 还指着 `lvl3` 的死帧。
	// 这一段 churn 的帧会覆盖那块内存，下一次 GC 就踩到垃圾。
	churn()
	println(*p)
}
"#;
    let (out, s) = run_with_env("gc_stack_three", src, &[("RGOC_GC_STRESS", "16")])
        .expect("编译 + 运行不该失败");
    assert_eq!(out.stdout, "6\n", "stderr: {}", out.stderr);
    s.assert_clean().expect("自检计数器");
}

/// `defer` 的闭包要保活（`T-ROOT-13`，`C12` §4.1 第 3 类，Task 24）。
///
/// # 这条钉的是什么
///
/// `defer` 记录（`DeferRec`）在 **arena** 里，永不回收，所以它自己不需要标记。
/// 但它的 `fv` 指向**托管堆上的闭包块** —— 那是 `mark_defer` 唯一要做的事。
///
/// 不扫它：闭包块被回收 ⇒ `rt_deferrun` 里的 `call_funcval(fv)` 读到垃圾 ⇒
/// 段错误或乱跳。实测（M10/Phase 2 之前）`closure4.go` / `defernil.go` 在
/// stress 下 rc=135。
///
/// # 为什么 `p` 只在闭包里
///
/// `p` 被**按值捕获**进闭包块，之后 `main` 的帧里就不再有活着的 `p` 了 ——
/// 所以闭包块是 `p` 指向的那块 1 KiB 的**唯一**根。这条判据因此同时钉住
/// 两件事：闭包块活着、闭包块的捕获被扫。
#[test]
fn t_root_13_defer_闭包要保活() {
    let src = r#"package main

func churn() {
	for i := 0; i < 3000; i++ {
		_ = new([64]byte)
	}
}

func main() {
	p := new([1024]byte)
	p[0] = 42
	p[1023] = 7
	defer func() { println(p[0] + p[1023]) }()
	churn()
}
"#;
    let (out, s) = run_with_env("gc_root_defer", src, &[("RGOC_GC_STRESS", "8")])
        .expect("编译 + 运行不该失败");
    assert_eq!(
        out.stdout, "49\n",
        "defer 的闭包（或它的捕获）被回收了：stderr {}",
        out.stderr
    );
    s.assert_clean().expect("自检计数器");
}

/// 闭包捕获**取址槽**时，槽里的指针要保活（`T-ROOT-14`，`R-10-23`）。
///
/// # 这条钉的是什么
///
/// `p := new([1024]byte)` 之后 `f := func() int { return p[0] }`：闭包捕获的
/// 不是 `p` 的值，而是 **`&p`**（帧槽的地址）—— 闭包体里 `capture#0` 拿到
/// 那个地址再 `load` 出 `p`。因为 `&p` 逃出了函数，`p` 的存储被搬到
/// **逃逸槽**（帧里只留一个指针格）。
///
/// 逃逸槽的存储必须进**托管堆**并被扫描。M9 把它放在 arena（不扫描）⇒
/// 闭包块里的捕获字是 `&槽`（栈地址，`mark_ptr` 直接忽略）⇒ `p` 指向的那块
/// 没有任何根 ⇒ 在 `churn()` 里被回收 ⇒ 读出来是复用后的垃圾。
///
/// 实测（修之前）：打印 `204` 而不是 `42`。
#[test]
fn t_root_14_闭包捕获取址槽() {
    let src = r#"package main

func churn() {
	for i := 0; i < 3000; i++ {
		_ = new([64]byte)
	}
}

func main() {
	p := new([1024]byte)
	p[0] = 42
	f := func() int { return p[0] }
	churn()
	println(f())
}
"#;
    let (out, s) = run_with_env("gc_root_addr_slot", src, &[("RGOC_GC_STRESS", "8")])
        .expect("编译 + 运行不该失败");
    assert_eq!(
        out.stdout, "42\n",
        "逃逸槽里的指针没被扫（存储还在 arena？）stderr: {}",
        out.stderr
    );
    s.assert_clean().expect("自检计数器");
}
