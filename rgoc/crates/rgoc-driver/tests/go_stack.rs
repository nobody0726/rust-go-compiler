//! `GO-1` 栈与切换（M11a）—— 判据骨架。
//!
//! # 为什么 Phase 0 只放骨架
//!
//! native 侧现在**没有** goroutine 栈：`gdesc` / `rt_switch` 在 Task 8–9 才出现，
//! 序言栈检查在 Task 10。所以这里先写判据的**形状**（fixture + 期望文本），
//! `#[ignore]` 到 Task 15（Phase 1 收口）再去掉。
//!
//! 先写形状不是形式主义：判据说「递归到超过 64 KiB ⇒ stderr 恰好
//! `runtime error: goroutine stack exceeded 65536 bytes` + 退出 2」，才知道
//! `rt_panic_stack_overflow` 要打**哪个数**（是配置值还是实测值 —— 见 `T-STK-06`）。

mod support;

/// 深递归：64 KiB 栈必然溢出（每帧 ≥ 48 字节 ⇒ 不到 1400 层）。
const DEEP: &str = r#"
package main

func rec(n int) int {
	if n == 0 {
		return 0
	}
	return rec(n-1) + 1
}

func main() {
	println(rec(1000000))
}
"#;

/// 编译 + 跑，**允许非 0 退出码**。
///
/// `native_harness::build_and_run_env` 把「退出码非 0」当错误返回（它服务的是
/// 「程序应该跑通」那类判据）。而栈溢出**就是** rc=2 —— 用那个装置会把
/// 「正确地报了溢出」判成「装置失败」。这里直接调 `run_build` + `Command`。
fn build_then_run(name: &str, src: &str) -> (String, String, i32) {
    use rgoc_driver::build_cmd;
    use rgoc_driver::cli::{BuildCommand, Emit};
    use std::process::Command;

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/go-stack")
        .join(name);
    std::fs::create_dir_all(&dir).expect("建 scratch 目录失败");
    let go = dir.join("main.go");
    std::fs::write(&go, src).expect("写 fixture 失败");
    let exe = dir.join("prog");
    let cmd = BuildCommand {
        files: vec![go],
        output: Some(exe.clone()),
        emit: Emit::Elf,
        keep_asm: false,
        opt: rgoc_ssa::opt::Config::all(),
        lang: None,
    };
    let out = build_cmd::run_build(&cmd).expect("build 调用失败");
    assert_eq!(out.exit, 0, "构建失败：\n{}", out.stderr);
    let o = Command::new(&exe).output().expect("产物跑不起来");
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
        o.status.code().unwrap_or(-1),
    )
}

#[test]
fn t_stk_03_溢出报错() {
    let (stdout, stderr, exit) = build_then_run("stk03", DEEP);
    assert_eq!(
        exit, 2,
        "栈溢出必须是退出码 2，实际 {exit}（stdout={stdout:?} stderr={stderr:?}）"
    );
    // ⚠️ 期望的数字是**当前 G 的栈字节数**，不是硬编码 65536。
    //
    // M11a 阶段 `main` 用进程栈（`stack_lo = 入口 sp - 4 MiB`），所以这里是
    // `4194304`。Task 12 之后 `main` 也会换到 mmap 的 64 KiB 栈，那时这条
    // 判据的期望改成 `65536` —— **两个数字都是真的**，而「文本与真实限制
    // 一致」正是这条判据要钉的（`rt.c` 里刻意不写死字面量）。
    assert!(
        stderr.contains("runtime error: goroutine stack exceeded 4194304 bytes"),
        "溢出文本不对（`C9` §5，数字必须是当前 G 的栈字节数）：\n{stderr}"
    );
    assert!(stdout.is_empty(), "溢出前不应有半截输出：{stdout:?}");
}

/// 最小程序：只为把 `rt_switch` 自测跑起来（程序本身什么都不做）。
const MIN: &str = r#"
package main

func main() {
}
"#;

/// `T-STK-01`：切换后 `x19..x28` 与 `d8..d15` 逐个不变（`M11-design.md` §3.3）。
///
/// # 为什么这条判据是「跑产物 + 看退出码」而不是直接调 `rt_switch`
///
/// 填寄存器、比对寄存器**只能是汇编** —— C 语言看不到寄存器分配，编译器可以
/// 随时复用它们。所以自测写在 `rt.c` 的顶层 `__asm__` 块里（`rt_sc_run`），
/// 由 `rt_init` 在 `RGOC_G11_SELFTEST=1` 时调用，失败 **退出 9**。
///
/// 9 这个码是刻意的：`2` 是运行期错误（`C9` §5），而自测失败只可能来自切换 ——
/// 用不同的码，判据红了就能直接分辨「切换坏了」与「程序自己坏了」。
///
/// # 自测自己也要守 ABI（`R-11-1`，实测踩过）
///
/// `rt_sc_run` 往 18 个 callee-saved 寄存器里填值，**必须先保存 C 的那份**。
/// 第一版没存 ⇒ 返回后 C 的局部变量全成了 `0x1111` ⇒ `rc=139`，而现象
/// **看起来像 `rt_switch` 切坏了**。查了半天才发现是自测函数自己违约。
#[test]
fn t_stk_01_callee_saved_全保住() {
    let out =
        support::native_harness::build_and_run_env("stk01", MIN, &[("RGOC_G11_SELFTEST", "1")])
            .unwrap_or_else(|e| {
                panic!(
                    "`rt_switch` 自测失败（rc=9 表示切换漏存了某个 callee-saved）：\n{e}\n\
                 期望：18 个寄存器（x19..x28 / d8..d15）切一圈回来逐个不变。"
                )
            });
    assert_eq!(out.exit, 0, "自测应干净退出");
    assert!(out.stderr.is_empty(), "自测不该有输出：{:?}", out.stderr);
}

/// `T-STK-04`：**序言有检查** —— 每个有帧的函数，`sub sp` 之后都有
/// `cmp sp, x9` + 条件跳转（`C5` §7.4）。
///
/// # 为什么要扫汇编而不是只看「有没有溢出」
///
/// 「溢出能报错」只证明**某一条路径**上有检查。漏发一个函数（比如只给大帧发）
/// 时，溢出会绕过所有大帧、在小帧累积处踩到 guard page —— 那时**没有**任何
/// 断言会红，只有段错误。所以这条判据数**指令**，不数行为。
///
/// # 为什么数「`cmp sp, x9` 与 `b.hs .Lso_` 的条数相等」
///
/// 两者必须成对：只发 `cmp` 不发跳转 = 检查了但没反应（比不检查更糟，白花
/// 指令）；只发跳转不发 `cmp` = 跳到未定义的条件上。
#[test]
fn t_stk_04_序言有检查() {
    let out = support::native_harness::build_and_run_env("stk04", MIN, &[])
        .expect("最小程序应能编译并跑起来");
    let cmps = out.asm.matches("cmp sp, x9").count();
    let brs = out.asm.matches("b.hs .Lso_").count();
    let calls = out.asm.matches("bl rt_panic_stack_overflow").count();
    assert!(cmps > 0, "一个栈检查都没发：\n{}", out.asm);
    assert_eq!(
        cmps, brs,
        "`cmp sp, x9` 与 `b.hs .Lso_` 的条数不等（{cmps} vs {brs}）—— 检查与跳转必须成对"
    );
    assert_eq!(
        cmps, calls,
        "每条检查后面都应有一个 `bl rt_panic_stack_overflow`（{cmps} vs {calls}）"
    );
    // 最小程序也有 `main`（有帧）⇒ 至少一条。
    assert!(cmps >= 1, "`main` 的帧非空，应该有检查");
}

/// `T-STK-05`：**超大帧不查**（`nilptr2.go` 的 128 MiB 帧走 `mmap`）。
///
/// `off_stack` 的帧底不在 `sp` 上（在 `rt_stack_frame_alloc` 给的块里），
/// `cmp sp, stack_lo` 对它**没有意义** —— 发了只会误报「栈溢出」，而真正
/// 该报的是 `out of memory`（`C9` §6.1）。
///
/// 判据用两条一起钉：
/// 1. 行为面：`nilptr2.go` 仍**跑通**（rc=0）—— 误报就会变成 rc=2；
/// 2. 静态面：它的 `.s` 里检查条数**少于**函数数（说明确实有函数被跳过）。
#[test]
fn t_stk_05_超大帧不查() {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../go_source_code/test/nilptr2.go"),
    )
    .expect("读不到 nilptr2.go");
    let out = support::native_harness::build_and_run_env("stk05", &src, &[])
        .expect("`nilptr2.go` 应能编译并跑起来（误报溢出会变成 rc=2）");
    assert_eq!(out.exit, 0, "`nilptr2.go` 应正常退出");
    let cmps = out.asm.matches("cmp sp, x9").count();
    let funcs = out.asm.matches("\nfn").count();
    assert!(
        cmps < funcs,
        "所有函数都发了栈检查（{cmps} 条 / {funcs} 个函数）—— `off_stack` 的帧不该查"
    );
}

// ════════════════════════════════════════════════════════════════════
// T-STK-06 溢出深度实测（M11c/Task 41 补 —— `Task 10` 的台账写了它却没写测试）
// ════════════════════════════════════════════════════════════════════

/// 在**新 G 的 64 KiB 栈**上递归 `n` 层。
///
/// # 为什么必须是新 G
///
/// `main` 用的是**进程栈**（`stack_lo` 取入口 `sp - 4 MiB`，`t_stk_03` 就是
/// 在它上面测的）。「64 KiB 够不够用」只能在新 G 上测 —— 而那要等 `go`
/// 语句（Task 12）之后。所以 Task 10 的台账把这条推到 Task 12，然后……**没人
/// 补**。`GO-1` 挂着一条没实现的判据而台账打勾，是「记一笔 ≠ 收口」的典型。
fn depth_src(n: u32) -> String {
    format!(
        r#"
package main

func rec(n int) int {{
	if n == 0 {{
		return 0
	}}
	return rec(n-1) + 1
}}

func main() {{
	c := make(chan int)
	go func() {{
		println(rec({n}))
		c <- 1
	}}()
	<-c
}}
"#
    )
}

fn run_depth(n: u32) -> (String, String, i32) {
    build_then_run(&format!("stk06_{n}"), &depth_src(n))
}

/// 递归 fixture 在 64 KiB 下**实测**溢出发生在第 `N` 层；`N` 可复现。
///
/// # 判据
///
/// 1. `N` 层**不**溢出（退出 0 且打印 `N`）—— 少一层就够，说明边界确实在 N+1；
/// 2. `N+1` 层溢出：退出 2 + `runtime error: goroutine stack exceeded 65536 bytes`；
/// 3. `N+1` 再跑一次，结果**逐字节相同**（「实测」不是「大概」）；
/// 4. `N ≥ 100`：留足富余（语料里递归最深的 `sieve1.go` 是 25 个 G 串成的链，
///    每层帧都浅；`N` 若掉到两位数，64 KiB 这个选择就该重新讨论）。
///
/// `N` 写进 `M11-manifest.json` 的 `measured.stack_overflow_depth`。
#[test]
fn t_stk_06_溢出深度实测() {
    // ① 倍增找到上界（帧 ~64 B ⇒ 大约 1000 层，从 128 起倍增省几轮）
    let (mut ok, mut bad) = (1u32, 128u32);
    while run_depth(bad).2 == 0 {
        ok = bad;
        bad *= 2;
        assert!(bad <= 8192, "64 KiB 下 {ok} 层都没溢出 ⇒ 栈检查没生效？");
    }
    // ② 二分：`ok` 不溢出、`bad` 溢出，找相邻的那一对
    while bad - ok > 1 {
        let mid = ok + (bad - ok) / 2;
        if run_depth(mid).2 == 0 {
            ok = mid;
        } else {
            bad = mid;
        }
    }
    let n = ok;

    // ③ `n` 层：退出 0，打印 n
    let (stdout, stderr, exit) = run_depth(n);
    assert_eq!(exit, 0, "{n} 层应当**不**溢出：stderr={stderr:?}");
    assert_eq!(stdout, format!("{n}\n"), "{n} 层的输出不对");

    // ④ `n+1` 层：溢出，文本与数字都对
    let (so2, se2, ex2) = run_depth(n + 1);
    assert_eq!(ex2, 2, "{0} 层应溢出（退出 2），实际 {ex2}", n + 1);
    assert!(
        se2.contains("runtime error: goroutine stack exceeded 65536 bytes"),
        "{0} 层的溢出文本不对（数字必须是新 G 的栈字节数 65536）：\n{se2}",
        n + 1
    );
    assert!(so2.is_empty(), "{0} 层溢出前不应有半截输出：{so2:?}", n + 1);

    // ⑤ 可复现：再跑一次，三样逐字节相同
    let (so3, se3, ex3) = run_depth(n + 1);
    assert_eq!(
        (so3, se3, ex3),
        (so2, se2, ex2),
        "同一层数跑两次结果不同 ⇒ 「实测深度」不可复现"
    );

    // ⑥ 富余
    assert!(n >= 100, "64 KiB 只够 {n} 层 ⇒ 余量太小，该重新讨论栈大小");

    println!(
        "T-STK-06: 64 KiB 下溢出深度 N = {n}（N+1 = {} 溢出）",
        n + 1
    );
}

/// **大帧**（12000 字节数组）在新 G 上递归 —— 溢出也必须干净报错。
///
/// # 为什么单独立一条（`T-STK-06` 的补充）
///
/// `T-STK-06` 的 fixture 每帧只有 ~144 字节，**越界量最多一帧**，所以
/// 「`stack_lo` 之下那一页余量」（`G_STACK_SLACK`）就兜住了 —— 把冷路径的
/// `mov sp, x9` 去掉，它照样绿（变异 `M-M11-18` 第一版就是这么绿的）。
///
/// 大帧才会露真章：`sub sp` 一次跳 12 KiB 越过 `stack_lo`，一页余量不够 ⇒
/// `rt_panic_stack_overflow` 的栈帧落在映射外 ⇒ **SIGSEGV（rc=-1）**，而
/// 不是「确定的文本 + 退出 2」。`C5` §7 允许栈上帧到 **1 MiB**
/// （`STACK_FRAME_LIMIT`），所以这不是假想的形状。
///
/// # 为什么是 12000 这个数（不是「随手一个 8 KiB」）
///
/// 越界量 = `ceil(65536/F)*F - 65536`，其中 `F` 是帧大小 —— 它必须**大于**
/// 余量页（4096）才验得到 `mov sp`。`F = 12000 + 48 = 12048` ⇒ 6 层越过，
/// 越界 `6*12048 - 65536 = 6752 > 4096` ✓。
/// 实测教训：第一版用 8 KiB，算出越界只有 **128 字节**（`8*8208 - 65536`）
/// —— 一页余量兜住了，去掉 `mov sp` 判据照样绿（变异 `M-M11-18` 绿过一次）。
/// 帧大小在 11605..13107 之间都给 6 层、越界都 > 4096，留了 ±400 的余量。
const BIG_FRAME: &str = r#"
package main

func big(n int) int {
	var buf [12000]byte
	buf[0] = byte(n)
	if n == 0 {
		return 0
	}
	return big(n-1) + int(buf[0])
}

func main() {
	c := make(chan int)
	go func() {
		println(big(40))
		c <- 1
	}()
	<-c
}
"#;

#[test]
fn t_stk_07_大帧溢出也报错() {
    let (stdout, stderr, exit) = build_then_run("stk07", BIG_FRAME);
    assert_eq!(
        exit, 2,
        "8 KiB 帧的溢出必须退出 2（-1 = 被信号杀掉，说明 panic 路径自己踩到栈外）：\
         stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(
        stderr.contains("runtime error: goroutine stack exceeded 65536 bytes"),
        "溢出文本不对：\n{stderr}"
    );
    assert!(stdout.is_empty(), "溢出前不应有半截输出：{stdout:?}");
}
