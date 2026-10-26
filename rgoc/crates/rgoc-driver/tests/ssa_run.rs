//! SSA 求值器的**端到端**判据（`M6-plan.md` Task 33 起；`T-SSA-32/34/35/36/37` 的雏形）。
//!
//! # 为什么在 `rgoc-driver` 而不是 `rgoc-interp`
//!
//! `rgoc-interp` 的 `tests/ssa_exec.rs` 手搓 IR，测的是**执行器**（块参数、边实参、
//! 内存根这些形态）。这一层测的是**整条链**：Go 源码 → 检查 → lowering → 建图 → 跑，
//! 判据是「输出与 `go run` 一致」的雏形 —— 只有 `rgoc-driver` 能同时看到前后两端。
//!
//! # 判据口径
//!
//! 与 M5 的 `run_semantics.rs` 同源：合并流**逐字节**比较（`print`/`println` 都进
//! 同一股），退出码一起比。唯一容差 `\r\n → \n`。

use std::path::PathBuf;

use rgoc_check::CheckOptions;
use rgoc_load::load_with_imports;
use rgoc_ssa::build::build;
use rgoc_types::TypeTable;

/// 跑一段源码的 SSA 路径，返回 `(输出, 退出码, 诊断)`。
fn run_ssa_src(name: &str, src: &str) -> (String, i32, Option<String>) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/ssa-run");
    std::fs::create_dir_all(&dir).expect("建不了 target/tmp/ssa-run");
    let path = dir.join(format!("{name}.go"));
    std::fs::write(&path, src).expect("写不了 fixture");

    let mut tt = TypeTable::new();
    let (pkg, bag) = load_with_imports(&mut tt, &[path], &[], None);
    let opts = CheckOptions { test_hooks: false };
    let (checked, lowered) =
        rgoc_check::check::check_package_with(&pkg, &mut tt, bag, &opts, |s| {
            rgoc_hir::lower(&pkg, s)
        });
    assert_eq!(
        checked.rendered_count(),
        0,
        "fixture 不该有静态诊断（有诊断说明 fixture 写错了）"
    );
    let program = lowered.expect("fixture 不该被 lowering 拒绝");
    let out = build(&program, &mut tt);
    assert!(
        out.rejected.is_empty(),
        "fixture 不该被 SSA 拒绝：{:?}",
        out.rejected
    );
    let o = rgoc_interp::run_ssa(&out.module, &tt, rgoc_interp::Options::default());
    (o.output, o.exit, o.diag)
}

/// 断言「输出 == 期望」且退出码 0。
fn expect_out(name: &str, src: &str, want: &str) {
    let (out, exit, diag) = run_ssa_src(name, src);
    assert_eq!(out, want, "输出不符（{name}）；diag = {diag:?}");
    assert_eq!(exit, 0, "退出码该是 0（{name}）；diag = {diag:?}");
}

// ── Task 33：纯值 ───────────────────────────────────────────────────

#[test]
fn t_ssa_33_算术与优先级() {
    // `1 + x*3`：`x` 是变量 ⇒ 不会被检查器折叠成常量，真走 `Bin`
    expect_out(
        "arith",
        "package main\n\nfunc main() {\n\tx := 2\n\tprintln(1 + x*3)\n}\n",
        "7\n",
    );
}

#[test]
fn t_ssa_33_int8_溢出回绕() {
    // Go 的整数运算是按类型宽度回绕的：`int8(127) + 1 == -128`
    expect_out(
        "wrap",
        "package main\n\nfunc main() {\n\tvar x int8 = 127\n\tprintln(x + 1)\n}\n",
        "-128\n",
    );
}

#[test]
fn t_ssa_33_字符串拼接() {
    expect_out(
        "strcat",
        "package main\n\nfunc main() {\n\ts := \"a\"\n\tprintln(s + \"b\")\n}\n",
        "ab\n",
    );
}

#[test]
fn t_ssa_33_复合字面量() {
    // `[2]int{1,2}` 走 `Composite`。**注意**：Go 的内建 `println` 对聚合值打印的是
    // **地址**（不可复现），`C9` 的契约用占位 `0xc000000000` 表示它
    // （`print::PTR_TEXT`）。所以这条只断言「Composite 跑得通、输出是占位地址」；
    // 元素级的判据要等 `Index`/`Field`（Task 35）能读之后。
    expect_out(
        "composite",
        "package main\n\nfunc main() {\n\tprintln([2]int{1, 2})\n}\n",
        "0xc000000000\n",
    );
}

#[test]
fn t_ssa_33_短路的值() {
    // 值层面：`true && false` = false，`false || true` = true
    // （「右侧不被求值」是 Task 34 之后的运行期判据）
    expect_out(
        "short-value",
        "package main\n\nfunc main() {\n\tprintln(true && false, false || true)\n}\n",
        "false true\n",
    );
}

#[test]
fn t_ssa_33_转换与浮点() {
    expect_out(
        "convert",
        "package main\n\nfunc main() {\n\tx := 1\n\tprintln(float64(x) / 2)\n}\n",
        "0.5\n",
    );
}

#[test]
fn t_ssa_33_一元运算() {
    expect_out(
        "unary",
        "package main\n\nfunc main() {\n\tx := 5\n\tprintln(-x, ^x, !(x == 5))\n}\n",
        "-5 -6 false\n",
    );
}

// ── Task 34：控制流、块参数与 Panic ─────────────────────────────────

#[test]
fn t_ssa_34_if_else_分支() {
    expect_out(
        "if-else",
        "package main\n\nfunc main() {\n\tx := 3\n\tif x > 2 {\n\t\tprintln(\"big\")\n\t} else {\n\t\tprintln(\"small\")\n\t}\n}\n",
        "big\n",
    );
}

#[test]
fn t_ssa_34_循环累加() {
    // 回边 + 块参数：`sum` 每轮携带新值
    expect_out(
        "loop",
        "package main\n\nfunc main() {\n\tsum := 0\n\tfor i := 1; i <= 5; i++ {\n\t\tsum += i\n\t}\n\tprintln(sum)\n}\n",
        "15\n",
    );
}

#[test]
fn t_ssa_34_并行赋值交换() {
    // `a, b = b, a`：边实参必须**先全算再全写**（`T-SSA-28` 的求值面）
    expect_out(
        "swap",
        "package main\n\nfunc main() {\n\ta, b := 1, 2\n\ta, b = b, a\n\tprintln(a, b)\n}\n",
        "2 1\n",
    );
}

#[test]
fn t_ssa_34_switch_与_fallthrough() {
    expect_out(
        "switch",
        "package main\n\nfunc main() {\n\tx := 2\n\tswitch x {\n\tcase 1:\n\t\tprintln(\"one\")\n\tcase 2:\n\t\tprintln(\"two\")\n\t\tfallthrough\n\tcase 3:\n\t\tprintln(\"three\")\n\tdefault:\n\t\tprintln(\"other\")\n\t}\n}\n",
        "two\nthree\n",
    );
}

#[test]
fn t_ssa_34_break_与_continue() {
    expect_out(
        "break-continue",
        "package main\n\nfunc main() {\n\tsum := 0\n\tfor i := 0; i < 10; i++ {\n\t\tif i == 5 {\n\t\t\tbreak\n\t\t}\n\t\tif i%2 == 0 {\n\t\t\tcontinue\n\t\t}\n\t\tsum += i\n\t}\n\tprintln(sum)\n}\n",
        // 1 + 3 = 4（0/2/4 被 continue 跳过，5 处 break）
        "4\n",
    );
}

#[test]
fn t_ssa_34_labeled_break() {
    expect_out(
        "labeled-break",
        "package main\n\nfunc main() {\n\tn := 0\nouter:\n\tfor i := 0; i < 3; i++ {\n\t\tfor j := 0; j < 3; j++ {\n\t\t\tif j == 1 {\n\t\t\t\tbreak outer\n\t\t\t}\n\t\t\tn++\n\t\t}\n\t}\n\tprintln(n)\n}\n",
        // 只有 (0,0) 会 +1
        "1\n",
    );
}

// `goto` 的运行期判据在 Task 35：`goto_mode` 会把**所有槽**建成根
// （`R-25-1`），于是 `i++` 走 `AddrOf`/`Load`/`Store` —— 那是 Task 35 的面。

#[test]
fn t_ssa_34_panic_走合并流且退出码_2() {
    let (out, exit, diag) = run_ssa_src(
        "panic",
        "package main\n\nfunc main() {\n\tprintln(\"before\")\n\tpanic(\"boom\")\n}\n",
    );
    assert_eq!(out, "before\npanic: boom\n", "panic 文本进合并流");
    assert_eq!(exit, 2, "未捕获 panic ⇒ 退出码 2");
    assert!(diag.is_none(), "panic 不是诊断（与 M5 同口径）");
}

#[test]
fn t_ssa_34_短路真的不执行右侧() {
    // `f() && g()`：`g` 不能被调用。这里用「调用有输出」的可见副作用验证：
    // `side()` 打印一次，`false && side()` 的右支不该跑。
    let (out, exit, _) = run_ssa_src(
        "short-side-effect",
        "package main\n\nfunc side() bool {\n\tprintln(\"side\")\n\treturn true\n}\n\nfunc main() {\n\tif false && side() {\n\t\tprintln(\"never\")\n\t}\n\tprintln(\"done\")\n}\n",
    );
    assert_eq!(exit, 0, "正常退出");
    assert_eq!(
        out, "done\n",
        "`false && side()` 的右支不该被求值（短路必须是控制流）"
    );
}

// ── Task 35：访存、调用与内建 ───────────────────────────────────────

#[test]
fn t_ssa_35_函数调用与递归() {
    // 递归同时验证三件事：压帧、`Return` 结果交接、帧深不靠宿主栈
    expect_out(
        "call-fib",
        "package main\n\nfunc fib(n int) int {\n\tif n < 2 {\n\t\treturn n\n\t}\n\treturn fib(n-1) + fib(n-2)\n}\n\nfunc main() {\n\tprintln(fib(10))\n}\n",
        "55\n",
    );
}

#[test]
fn t_ssa_35_多返回值与结果直连() {
    expect_out(
        "call-multi",
        "package main\n\nfunc two() (int, int) {\n\treturn 3, 4\n}\n\nfunc sum(a, b int) int {\n\treturn a + b\n}\n\nfunc main() {\n\tx, y := two()\n\tprintln(x, y, sum(two()))\n}\n",
        "3 4 7\n",
    );
}

#[test]
fn t_ssa_35_方法调用_指针接收者写回() {
    expect_out(
        "call-method",
        "package main\n\ntype Counter struct{ n int }\n\nfunc (c *Counter) Add(d int) {\n\tc.n += d\n}\n\nfunc main() {\n\tvar c Counter\n\tc.Add(3)\n\tc.Add(4)\n\tprintln(c.n)\n}\n",
        "7\n",
    );
}

#[test]
fn t_ssa_35_指针形参写回调用方() {
    // `&x` 的地址要能穿过帧边界指回调用方的槽（`RootId` 是函数内的，
    // 所以运行期只能靠 `Value::Ptr` 里的真实 `Cell`）
    expect_out(
        "call-ptr",
        "package main\n\nfunc inc(p *int) {\n\t*p = *p + 1\n}\n\nfunc main() {\n\tx := 41\n\tinc(&x)\n\tprintln(x)\n}\n",
        "42\n",
    );
}

#[test]
fn t_ssa_35_数组与结构体字段() {
    expect_out(
        "mem-agg",
        "package main\n\ntype P struct {\n\tx int\n\ty int\n}\n\nfunc main() {\n\tvar a [3]int\n\ta[1] = 7\n\tvar p P\n\tp.x = 2\n\tp.y = a[1] + 1\n\tprintln(a[1], p.x, p.y)\n}\n",
        "7 2 8\n",
    );
}

#[test]
fn t_ssa_35_全局变量与初始化顺序() {
    // `b` 的初始化式读 `a`：init 里必须同根保序（读用写产出的 mem）
    expect_out(
        "mem-global",
        "package main\n\nvar a = 1\n\nvar b = a + 1\n\nfunc main() {\n\tprintln(a, b)\n\ta = 10\n\tprintln(b, a)\n}\n",
        "1 2\n2 10\n",
    );
}

#[test]
fn t_ssa_35_切片共享底层数组() {
    expect_out(
        "mem-slice-share",
        "package main\n\nfunc main() {\n\ts := []int{1, 2, 3}\n\tt := s\n\tt[0] = 9\n\tprintln(s[0], t[0], len(s))\n}\n",
        "9 9 3\n",
    );
}

#[test]
fn t_ssa_35_内建_len_cap_append_copy() {
    expect_out(
        "builtin-seq",
        "package main\n\nfunc main() {\n\ts := make([]int, 2, 4)\n\ts[0] = 1\n\ts[1] = 2\n\ts = append(s, 3)\n\td := make([]int, len(s))\n\tn := copy(d, s)\n\tprintln(len(s), cap(s), n, d[2])\n}\n",
        "3 4 3 3\n",
    );
}

#[test]
fn t_slice_06_append_扩容要拷旧元素() {
    // 判据 `T-SLICE-06` 的**求值器半边**（M9 收口时补）。
    //
    // 原有的 `t_ssa_35_内建_len_cap_append_copy` 只 append 到 **cap 够**的切片
    // （`make([]int,2,4)` ⇒ `len=3,cap=4` 不扩容），所以「扩容时不拷旧元素」
    // 这条**只能靠 native 的 `obj_slice` 抓**。而 `append_value` 是两套求值器
    // 共用的一份实现（`D-M6-7`）—— 它坏了解释器侧就没人管（实测：变异
    // `M-M9-03` 在 `ssa_run` 上原本是**绿的**）。
    //
    // 期望值来自官方 `go run`：`[]int{1,2}` 上 `append` 会扩到 cap 4，
    // 新数组前两个必须是**拷过来的** 1 和 2，旧切片 `s` 仍是 len 2。
    expect_out(
        "slice-append-grow",
        "package main\n\nfunc main() {\n\ts := []int{1, 2}\n\tt := append(s, 3)\n\tprintln(len(t), t[0], t[1], t[2], len(s), s[0], s[1])\n}\n",
        "3 1 2 3 2 1 2\n",
    );
}

#[test]
fn t_ssa_35_内建_min_max_complex() {
    expect_out(
        "builtin-num",
        "package main\n\nfunc main() {\n\tprintln(min(3, 1, 2), max(3, 1, 2))\n\tc := complex(1, 2)\n\tprintln(real(c), imag(c))\n}\n",
        "1 3\n1 2\n",
    );
}

#[test]
fn t_ssa_35_append_展开() {
    // `append(s, xs...)` 展开**一串**元素；`append(s, xs)` 只追加**一个**
    // （后者对 []int 是类型错，所以只测前者）。dots 位是 IR 的语义（`R-35-1`）。
    expect_out(
        "builtin-append-dots",
        "package main\n\nfunc main() {\n\ta := []int{1, 2}\n\tb := []int{3, 4}\n\tc := append(a, b...)\n\tprintln(len(c), c[3])\n}\n",
        "4 4\n",
    );
}

#[test]
fn t_ssa_35_new_与_clear() {
    expect_out(
        "builtin-new",
        "package main\n\nfunc main() {\n\tp := new(int)\n\t*p = 5\n\tprintln(*p)\n\ts := []int{1, 2}\n\tclear(s)\n\tprintln(s[0], s[1])\n}\n",
        "5\n0 0\n",
    );
}

#[test]
fn t_ssa_35_goto_循环() {
    // `goto_mode` 把全部槽建成根（`R-25-1`）⇒ 全走 AddrOf/Load/Store
    expect_out(
        "goto-loop",
        "package main\n\nfunc main() {\n\ti := 0\nloop:\n\tif i < 3 {\n\t\tprintln(i)\n\t\ti++\n\t\tgoto loop\n\t}\n}\n",
        "0\n1\n2\n",
    );
}

#[test]
fn t_ssa_35_字符串下标取字节() {
    expect_out(
        "str-index",
        "package main\n\nfunc main() {\n\ts := \"abc\"\n\tprintln(s[0], s[2], len(s))\n}\n",
        "97 99 3\n",
    );
}

#[test]
fn t_ssa_35_nil_指针解引用走_panic_通道() {
    let (out, exit, _) = run_ssa_src(
        "nil-deref",
        "package main\n\nfunc main() {\n\tvar p *int\n\tprintln(\"before\")\n\tprintln(*p)\n}\n",
    );
    assert_eq!(exit, 2, "nil 解引用 ⇒ 退出码 2");
    assert_eq!(
        out, "before\npanic: runtime error: invalid memory address or nil pointer dereference\n",
        "文本要与 Go 一致"
    );
}

#[test]
fn t_ssa_35_数组赋值是拷贝() {
    // Go 的数组/结构体是**值语义**：`b := a` 之后改 `b` 不影响 `a`。
    // 运行期靠 `Store` 的 `deep_copy`；提升成纯值时靠 `Copy` —— 而 `Copy` 是
    // **重命名**（块参数交接用），所以赋值点必须自己保证拷贝（`R-35-4`）。
    expect_out(
        "mem-array-copy",
        "package main\n\nfunc main() {\n\ta := [3]int{1, 2, 3}\n\tb := a\n\tb[0] = 9\n\tprintln(a[0], b[0])\n}\n",
        "1 9\n",
    );
}

#[test]
fn t_ssa_35_结构体赋值是拷贝() {
    expect_out(
        "mem-struct-copy",
        "package main\n\ntype P struct{ x int }\n\nfunc main() {\n\tp := P{1}\n\tq := p\n\tq.x = 9\n\tprintln(p.x, q.x)\n}\n",
        "1 9\n",
    );
}

#[test]
fn t_ssa_35_range_结束后下标是最后一个() {
    // `for i = range [5]int{}` 结束后 `i == 4`（Go 规范）。计数器是**私有槽**：
    // 拿用户的下标槽当计数器会让出口值变成 5（`R-35-5`）。
    expect_out(
        "range-key-after",
        "package main\n\nfunc main() {\n\ti := 0\n\tfor i = range [5]int{} {\n\t}\n\tprintln(i)\n\tj := 0\n\tfor j := range [3]int{} {\n\t\t_ = j\n\t}\n\tprintln(j)\n}\n",
        "4\n0\n",
    );
}

#[test]
fn t_ssa_35_switch_的_case_表达式逐个短路() {
    // `case f(2), f(3):` 里 `f(2)` 命中就不该求值 `f(3)`（Go 规范：case 表达式
    // 自左向右，第一个相等的生效）。初版用一条 `bin lor` 把两个比较串起来 ——
    // 两个操作数**都已经求值**，于是 `calls == 2`（`R-35-3`）。
    expect_out(
        "switch-case-short",
        "package main\n\nvar calls int\n\nfunc f(n int) int {\n\tcalls++\n\treturn n\n}\n\nfunc main() {\n\tx := 2\n\tswitch x {\n\tcase f(2), f(3):\n\t\tprintln(\"hit\", calls)\n\tdefault:\n\t\tprintln(\"miss\", calls)\n\t}\n}\n",
        "hit 1\n",
    );
}

#[test]
fn t_ssa_35_通过指针读字段() {
    // `p.n`（`p *S`）的底座是**指针值**，不是「`p` 这个变量的地址」。
    // 写成后者就是「指针的指针取字段」，运行期当场报「底座不是结构体」（`R-35-2`）。
    expect_out(
        "ptr-field-read",
        "package main\n\ntype S struct{ n int }\n\nfunc get(p *S) int {\n\treturn p.n\n}\n\nfunc main() {\n\tvar s S\n\ts.n = 5\n\tprintln(get(&s))\n}\n",
        "5\n",
    );
}
