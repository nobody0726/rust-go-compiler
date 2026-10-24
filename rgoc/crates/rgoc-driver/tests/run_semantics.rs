//! 甲腿：自有 fixture 的**语义断言**（`M5-tests.md` §3.1 的 `S` 腿）。
//!
//! # 这一层为什么存在
//!
//! 乙腿（`run_corpus.rs` + `run-collide.sh`）证明的是「官方语料跑得跟官方一样」，
//! 但它**说不出**为什么：语料里没有「`int8(127)+1 == -128`」这种**单点**断言，
//! 一条宽泛的 `panic(r)` 只能告诉你「错了」，不能告诉你「哪条语义错了」。
//! 甲腿就是那些单点 —— 一条断言一条语义，红了立刻知道是哪条。
//!
//! # 每个测试的名字就是它的 ID
//!
//! 测试函数名带 `t_ex_NN_` 前缀，`cargo test` 的输出直接能对回
//! `M5-tests.md` §3.1 的表格。这样「某条 ID 到底有没有测」不需要靠人记。
//!
//! # 为什么写在 `rgoc-driver` 而不是 `rgoc-interp`
//!
//! 判据是**端到端**的：`run` 的退出码 + 合并流。`rgoc-interp` 只看得到
//! `run()` 的返回值，看不到 `run` 命令怎么把三条通道翻成字节与退出码
//! （`§4.2` / `§4.3` 的格式表与退出码表就在这一层）。写在 `rgoc-interp`
//! 里会绕开 CLI 的编排，测试就与门禁口径漂开了。
//!
//! # fixture 怎么放
//!
//! 内联字符串 + 写到 `target/tmp` 下（**不**进 `go_source_code/`：那是只读语料）。
//! 每条 fixture 自带唯一文件名，避免并行测试互相覆盖。

use std::path::PathBuf;

use rgoc_driver::cli::RunCommand;
use rgoc_driver::run_cmd::{RunOutcome, run_run};

/// 把源码写到临时目录再跑（返回三条通道）。
fn run_src(name: &str, src: &str) -> RunOutcome {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/run-semantics");
    std::fs::create_dir_all(&dir).expect("建不了 target/tmp/run-semantics");
    let path = dir.join(format!("{name}.go"));
    std::fs::write(&path, src).expect("写不了 fixture");
    run_run(&RunCommand {
        files: vec![path],
        dump_hir: false,
        max_steps: None,
        lang: None,
        via: rgoc_driver::cli::Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
    })
    .expect("内部错（`run_run` 返回 Err ⇒ 退出码 2 的那一类）")
}

/// 跑一个**应当正常结束**的 fixture，返回合并流。
///
/// 三道断言合一：退出码 0、stderr 空、没有未捕获 panic。任何一条不满足都
/// 直接把三条通道打出来 —— 只报「期望 A 得到 B」在调试时不够用。
#[track_caller]
fn out(name: &str, src: &str) -> String {
    let o = run_src(name, src);
    assert_eq!(o.exit, 0, "{name}: 退出码 {}，stderr={}", o.exit, o.stderr);
    assert!(o.stderr.is_empty(), "{name}: stderr 非空：{}", o.stderr);
    assert!(
        !o.stdout.contains("panic:"),
        "{name}: 输出里有 panic：{}",
        o.stdout
    );
    o.stdout
}

/// 跑一个**应当被拒**的 fixture，断言退出码 1 且 stderr 含稳定码。
#[track_caller]
fn rejected(name: &str, src: &str, code: &str) {
    let o = run_src(name, src);
    assert_eq!(
        o.exit, 1,
        "{name}: 期望退出码 1（静态拒绝），实际 {}",
        o.exit
    );
    assert!(
        o.stdout.is_empty(),
        "{name}: 被拒时不该有 stdout：{}",
        o.stdout
    );
    assert!(
        o.stderr.contains(code),
        "{name}: stderr 里没有稳定码 `{code}`：{}",
        o.stderr
    );
}

// ═══════════════════════ EX-1：HIR 与最小求值 ═══════════════════════

/// `T-EX-02`：未支持构造**显式拒绝**（退出码 1 + 稳定码），且进程不 abort。
///
/// 五条：`go` / `chan` / `select` / `range` over func / `close`。每条的码
/// 都在 `RejectKind::code()` 里冻结过 —— 改码等于契约变更。
#[test]
fn t_ex_02_unsupported_constructs_are_rejected_loudly() {
    rejected(
        "t_ex_02_go",
        "package main\nfunc f() {}\nfunc main() { go f() }\n",
        "HIR-UNSUPPORTED-GO",
    );
    rejected(
        "t_ex_02_chan",
        "package main\nfunc main() { _ = make(chan int) }\n",
        "HIR-UNSUPPORTED-CHAN",
    );
    rejected(
        "t_ex_02_select",
        "package main\nfunc main() {\n\tselect {\n\tdefault:\n\t}\n}\n",
        "HIR-UNSUPPORTED-SELECT",
    );
    rejected(
        "t_ex_02_rangefunc",
        "package main\nfunc seq(y func(int) bool) {}\nfunc main() {\n\tfor range seq {\n\t}\n}\n",
        "HIR-UNSUPPORTED-RANGE-FUNC",
    );
    rejected(
        "t_ex_02_close",
        "package main\nfunc main() { var c chan int; close(c) }\n",
        "HIR-UNSUPPORTED-CLOSE",
    );
}

/// `T-EX-04`：`--dump-hir` 的**确定性**：同一输入两次逐字节相同。
///
/// 确定性是 dump 能当对撞入口的前提。任何 `HashMap` 迭代序漏进来都会让
/// 两次 dump 不同 —— 而那种差异在 diff 里长得像「语义变了」，极难排查。
#[test]
fn t_ex_04_dump_is_deterministic() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/run-semantics");
    std::fs::create_dir_all(&dir).expect("建不了目录");
    let path = dir.join("t_ex_04_dump.go");
    std::fs::write(
        &path,
        "package main\n\
         type S struct{ A, B int }\n\
         var g = 3\n\
         func add(a, b int) int { return a + b }\n\
         func main() {\n\
         \ts := S{1, 2}\n\
         \tfor i := 0; i < 2; i++ {\n\
         \t\ts.A = add(s.A, i)\n\
         \t}\n\
         \tif s.A > g {\n\
         \t\tprint(s.A)\n\
         \t}\n\
         }\n",
    )
    .expect("写不了 fixture");
    let dump = |name: &str| {
        run_run(&RunCommand {
            files: vec![path.clone()],
            dump_hir: true,
            max_steps: None,
            lang: None,
            via: rgoc_driver::cli::Via::Hir,
            opt: rgoc_ssa::opt::Config::all(),
        })
        .unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    let a = dump("第一次");
    let b = dump("第二次");
    assert_eq!(a.exit, 0);
    assert_eq!(a.stderr, "");
    assert!(!a.stdout.is_empty(), "dump 是空的");
    assert_eq!(a.stdout, b.stdout, "两次 dump 不一致 ⇒ 有非确定性的迭代序");
    assert!(a.stdout.starts_with("# program"), "dump 头行格式变了");
}

/// `T-EX-05`：`Place` 的五种形态都能取址（slot / global / field / index / deref）。
///
/// 取址不是「加个 `&`」那么简单：每种形态对应解释器一条不同的 `Target`
/// 路径，漏一种就是「别名丢失」——写进副本，读原值不变，静默算错。
#[test]
fn t_ex_05_all_place_shapes_are_addressable() {
    let got = out(
        "t_ex_05_place",
        "package main\n\
         type S struct{ A int }\n\
         var g int\n\
         func main() {\n\
         \tlocal := 1\n\
         \tpl := &local\n\
         \t*pl = 2\n\
         \tpg := &g\n\
         \t*pg = 3\n\
         \ts := S{4}\n\
         \tpf := &s.A\n\
         \t*pf = 5\n\
         \tarr := [3]int{6, 7, 8}\n\
         \tpa := &arr[1]\n\
         \t*pa = 9\n\
         \tprint(local, g, s.A, arr[1])\n\
         }\n",
    );
    assert_eq!(got, "2359");
}

/// `T-EX-06`：常量在 HIR 里**保留任意精度**，只在收敛点按静态类型截断。
///
/// 五行各钉一个收敛点：
///
/// | 行 | 收敛 | 为什么必须逐位对 |
/// |---|---|---|
/// | `1<<62 → int64` | 常量超 `int32`，必须靠 i128 才不丢 | 用 `i64` 装就会在 `<<` 处溢出 |
/// | `int64(-9223372036854775808)` | `i64::MIN` 的**取负**在 `i64` 里是 UB/回绕 | 只有先算成 i128 再收才拿得到这个值 |
/// | `int64(9223372036854775807)` | `i64::MAX` | 边界不漏一 |
/// | `1<<100 → float64` | 整数常量转浮点要**正确舍入** | `f64` 直接乘 2^100 会得到另一个末位 |
/// | `uint64(18446744073709551615)` | `u64::MAX` | `i128` 能装下整个 u64 域 |
///
/// 期望值由容器内 `go run`（go1.27.1）实测，不是手抄。
///
/// ⚠️ **这条只盖住「收敛无损」那一半。** 判据的另一半是「`1<<100` 赋给 `int`
/// ⇒ 静态 overflow」，而本仓**还没有**常量溢出诊断（`int8` 赋 200 也照收）——
/// 见下面 `#[ignore]` 的那条与 `M5-manifest.json` 的 `G-M5-01`。所以 `T-EX-06`
/// 的 verdict 是 **partial**，不是 pass。
#[test]
fn t_ex_06_constant_convergence_keeps_every_bit() {
    let got = out(
        "t_ex_06_const",
        "package main\n\
         func main() {\n\
         \tconst c = 1 << 62\n\
         \tvar x int64 = c\n\
         \tprintln(x)\n\
         \tprintln(int64(-9223372036854775808))\n\
         \tprintln(int64(9223372036854775807))\n\
         \tconst d = 1 << 100\n\
         \tvar f float64 = d\n\
         \tprintln(f)\n\
         \tprintln(uint64(18446744073709551615))\n\
         }\n",
    );
    assert_eq!(
        got,
        "4611686018427387904\n\
         -9223372036854775808\n\
         9223372036854775807\n\
         1.2676506002282294e+30\n\
         18446744073709551615\n"
    );
}

/// `T-EX-06` 的**未兑现半边**：常量溢出必须被静态拒绝。
///
/// 这条**故意**是 `#[ignore]` 的。它断言的是**规范要求**（`go run` 实测：
/// `cannot use 200 (untyped int constant) as int8 value in variable declaration
/// (overflows)`），而本仓当前**照收不误**、并且照收之后还打印了未截断的值。
///
/// 为什么不用 `assert!(accepted)` 把现状写进测试：那等于把 bug 变成判据。
/// 为什么留在这里而不是删掉：删掉之后，「这条判据存在」这件事就只剩文档里
/// 一句话；留成 `#[ignore]`，谁修了 M4 的常量检查，`cargo test -- --ignored`
/// 就会立刻变绿 —— 那时把 `#[ignore]` 去掉即可。
///
/// 归属：这是 **M4 的面**（`M5-tests.md` `T-EX-06` 原文写的就是「M4 面」），
/// 登记为 `G-M5-01`，卡 `EX-1`。
#[test]
#[ignore = "G-M5-01：常量溢出诊断属 M4 面，尚未实现（本仓目前照收）"]
fn t_ex_06_constant_overflow_is_rejected() {
    rejected(
        "t_ex_06_ovf_int8",
        "package main\nfunc main() {\n\tvar y int8 = 200\n\tprintln(y)\n}\n",
        "CHK-CONST-OVERFLOW",
    );
    rejected(
        "t_ex_06_ovf_shift",
        "package main\nfunc main() {\n\tvar x int = 1 << 100\n\tprintln(x)\n}\n",
        "CHK-CONST-OVERFLOW",
    );
}

// ═══════════════════════ EX-3：复合值与内建 ═══════════════════════

/// `T-EX-07`：整数算术的**宽度回绕**与除零 panic。
///
/// `int8(127)+1 == -128` 是「槽里存 i128、每次运算后按静态宽度截断」
/// 这条设计的唯一可测判据。不截断就永远是 128。
#[test]
fn t_ex_07_integer_wraparound_and_division() {
    let got = out(
        "t_ex_07_int",
        "package main\n\
         func main() {\n\
         \tvar a int8 = 127\n\
         \ta = a + 1\n\
         \tvar b uint8 = 0\n\
         \tb = b - 1\n\
         \tprint(a, \" \", b, \" \")\n\
         \tvar c int64 = -8\n\
         \tprint(c>>1, \" \", -7/2, \" \", -7%2, \" \")\n\
         \tprint(1<<10, \" \", uint64(1)<<63)\n\
         }\n",
    );
    assert_eq!(got, "-128 255 -4 -3 -1 1024 9223372036854775808");

    // 除零是 Go 语义的 panic（退出码 2），不是宿主 panic。
    let o = run_src(
        "t_ex_07_divzero",
        "package main\nfunc main() { x := 1; y := 0; print(x / y) }\n",
    );
    assert_eq!(o.exit, 2, "除零应当 panic（退出码 2）");
    assert!(
        o.stdout.contains("integer divide by zero"),
        "除零文本不对：{}",
        o.stdout
    );
}

/// `T-EX-08`：浮点按 `float32` 舍入 + 复数 `real`/`imag`。
///
/// `float32(1)/3` 必须打 `0.33333334`（f32 的最短往返），而 `1.0/3`
/// 打 `0.3333333333333333`。两者差在「每一步之后有没有按 f32 舍入」。
#[test]
fn t_ex_08_float_and_complex() {
    let got = out(
        "t_ex_08_float",
        "package main\n\
         func main() {\n\
         \tvar f float32 = 1\n\
         \tprint(f/3, \" \", 1.0/3, \" \")\n\
         \tc := complex(3, 4)\n\
         \tprint(real(c), \" \", imag(c), \" \")\n\
         \tprint(1000000.0, \" \", 0.0001, \" \", 0.00001)\n\
         }\n",
    );
    assert_eq!(got, "0.33333334 0.3333333333333333 3 4 1e+06 0.0001 1e-05");
}

/// `T-EX-09`：`&&` / `||` 的**短路**（用副作用计数验证）。
///
/// 短路不是优化，是语义：`p != nil && p.f()` 靠它才不炸。
#[test]
fn t_ex_09_short_circuit() {
    let got = out(
        "t_ex_09_short",
        "package main\n\
         var n int\n\
         func bump() bool { n++; return true }\n\
         func main() {\n\
         \t_ = false && bump()\n\
         \tprint(n, \" \")\n\
         \t_ = true || bump()\n\
         \tprint(n, \" \")\n\
         \t_ = true && bump()\n\
         \tprint(n, \" \")\n\
         \t_ = false || bump()\n\
         \tprint(n)\n\
         }\n",
    );
    assert_eq!(got, "0 0 1 2");
}

/// `T-EX-10`：字符串的字节语义 —— 索引得 `byte`、`len` 按字节、`range` 得 rune。
///
/// 「`len` 按字节、`range` 按 rune」这一对是 Go 字符串最容易写错的地方：
/// 只要索引与 `len` 一致、`range` 解码正确，两者就能同时对上。
#[test]
fn t_ex_10_string_bytes_and_runes() {
    let got = out(
        "t_ex_10_string",
        "package main\n\
         func main() {\n\
         \ts := \"hellô\"\n\
         \tprint(len(s), \" \", s[0], \" \", s[4], \" \", s[5], \" \")\n\
         \tt := s[1:3]\n\
         \tprint(t, \" \", len(t), \" \")\n\
         \tu := s + \"!\"\n\
         \tprint(u, \" \")\n\
         \tfor i, r := range \"aé\" {\n\
         \t\tprint(i, \":\", r, \" \")\n\
         \t}\n\
         }\n",
    );
    assert_eq!(got, "6 104 195 180 el 2 hellô! 0:97 1:233 ");
}

/// `T-EX-11`：数组是**值语义**（赋值拷贝），且拷贝后互不影响。
#[test]
fn t_ex_11_array_value_semantics() {
    let got = out(
        "t_ex_11_array",
        "package main\n\
         func main() {\n\
         \tarr := [3]int{1, 2, 3}\n\
         \tcp := arr\n\
         \tcp[0] = 9\n\
         \tprint(arr[0], \" \", cp[0], \" \")\n\
         \tvar lit = [...]int{4, 5}\n\
         \tprint(len(lit), \" \")\n\
         \tp := &arr[0]\n\
         \tcp2 := arr\n\
         \tcp2[0] = 7\n\
         \tprint(*p, \" \", arr[0])\n\
         }\n",
    );
    assert_eq!(got, "1 9 2 1 1");
}

/// `T-EX-12`：结构体的值语义 + 嵌入字段的**提升选择器**。
#[test]
fn t_ex_12_struct_copy_and_embedding() {
    let got = out(
        "t_ex_12_struct",
        "package main\n\
         type Inner struct{ A int }\n\
         type Outer struct {\n\
         \tInner\n\
         \tB int\n\
         }\n\
         func main() {\n\
         \to := Outer{Inner{1}, 2}\n\
         \tcp := o\n\
         \tcp.A = 9\n\
         \tprint(o.A, \" \", cp.A, \" \", cp.B, \" \")\n\
         \to.A = 5\n\
         \tprint(o.Inner.A, \" \")\n\
         \tpo := &o\n\
         \tpo.A = 6\n\
         \tprint(o.A)\n\
         }\n",
    );
    assert_eq!(got, "1 9 2 5 6");
}

/// `T-EX-13`：slice 共享底层数组；`append` 超 cap 后**不再共享**。
///
/// 后半句是关键：只测「共享」会漏掉「增长时忘了换新底层数组」。
#[test]
fn t_ex_13_slice_alias_and_growth() {
    let got = out(
        "t_ex_13_slice",
        "package main\n\
         func main() {\n\
         \ts1 := []int{1, 2, 3}\n\
         \ts2 := s1\n\
         \ts2[0] = 9\n\
         \tprint(s1[0], \" \")\n\
         \tt := make([]int, 2, 2)\n\
         \tt[0], t[1] = 1, 2\n\
         \tu := t\n\
         \tu = append(u, 3)\n\
         \tu[0] = 8\n\
         \tprint(t[0], \" \", len(t), \" \", len(u), \" \")\n\
         \tfull := s1[0:3:3]\n\
         \tprint(cap(full), \" \")\n\
         \tdst := make([]int, 3)\n\
         \tprint(copy(dst, s1), \" \", dst[0])\n\
         }\n",
    );
    assert_eq!(got, "9 1 2 3 3 3 9");
}

/// `T-EX-14`：map 的引用语义 + 键相等 + `delete` + 双值 + 插入序。
///
/// 插入序是**我们自己的**承诺（`U-M5-04`：官方故意随机），所以这一条只能
/// 断言「我们的序 == 插入序」，不能拿官方输出对撞。
#[test]
fn t_ex_14_map_semantics() {
    let got = out(
        "t_ex_14_map",
        "package main\n\
         type K struct{ A int }\n\
         func main() {\n\
         \tm := map[string]int{\"a\": 1}\n\
         \tm2 := m\n\
         \tm2[\"b\"] = 2\n\
         \tprint(m[\"b\"], \" \")\n\
         \tv, ok := m[\"a\"]\n\
         \tprint(v, ok, \" \")\n\
         \t_, ok2 := m[\"zz\"]\n\
         \tprint(ok2, \" \")\n\
         \tdelete(m, \"a\")\n\
         \t_, ok3 := m[\"a\"]\n\
         \tprint(ok3, \" \")\n\
         \tks := map[K]int{{1}: 10}\n\
         \tprint(ks[K{1}], \" \")\n\
         \torder := map[int]int{3: 0, 1: 0, 2: 0}\n\
         \tfor k := range order {\n\
         \t\tprint(k)\n\
         \t}\n\
         }\n",
    );
    assert_eq!(got, "2 1true false false 10 312");
}

/// `T-EX-15`：指针的取址 / 解引用 / 写穿 / nil 解引用 panic。
#[test]
fn t_ex_15_pointer_semantics() {
    let got = out(
        "t_ex_15_ptr",
        "package main\n\
         type S struct{ A int }\n\
         func main() {\n\
         \tx := 1\n\
         \tp := &x\n\
         \t*p = 2\n\
         \tprint(x, \" \")\n\
         \ts := S{3}\n\
         \tps := &s\n\
         \tps.A = 4\n\
         \tprint(s.A, \" \")\n\
         \ta := [2]int{5, 6}\n\
         \tpa := &a[1]\n\
         \t*pa = 7\n\
         \tprint(a[1], \" \")\n\
         \tsl := []int{8}\n\
         \tpsl := &sl[0]\n\
         \t*psl = 9\n\
         \tprint(sl[0], \" \")\n\
         \tvar np *int\n\
         \tprint(np == nil)\n\
         }\n",
    );
    assert_eq!(got, "2 4 7 9 true");

    let o = run_src(
        "t_ex_15_nil",
        "package main\nfunc main() { var p *int; print(*p) }\n",
    );
    assert_eq!(o.exit, 2, "nil 解引用应当 panic");
    assert!(
        o.stdout.contains("nil pointer dereference"),
        "文本不对：{}",
        o.stdout
    );
}

/// `T-EX-16`：类型转换矩阵 —— 数值截断/符号扩展、`string`↔`[]byte`/`[]rune`。
#[test]
fn t_ex_16_conversions() {
    let got = out(
        "t_ex_16_conv",
        "package main\n\
         func main() {\n\
         \tvar i32 int32 = -1\n\
         \tprint(uint32(i32), \" \", uint8(i32), \" \")\n\
         \tvar u8 uint8 = 200\n\
         \tprint(int8(u8), \" \")\n\
         \tb := []byte(\"hi\")\n\
         \tprint(len(b), \" \", b[0], \" \")\n\
         \tr := []rune(\"é\")\n\
         \tprint(len(r), \" \", r[0], \" \")\n\
         \tprint(string(b), \" \", string(rune(65)), \" \")\n\
         \tvar f float64 = 3.9\n\
         \tprint(int(f), \" \")\n\
         \tprint(byte(65))\n\
         }\n",
    );
    assert_eq!(got, "4294967295 255 -56 2 104 1 233 hi A 3 65");
}

// ═══════════════════════ EX-2：函数、帧与初始化 ═══════════════════════

/// `T-EX-17`：1000 层递归（证明**不依赖 Rust 栈**，`EX-7`）+ 变参。
///
/// 如果哪天有人把 `step_*` 改成 Rust 递归，这条会栈溢出 —— 而那正是
/// `D-M5-1` 要防的形态。所以它同时是 `T-EX-52`/`T-EX-67` 的判据。
#[test]
fn t_ex_17_recursion_and_variadic() {
    let got = out(
        "t_ex_17_rec",
        "package main\n\
         func down(n int) int {\n\
         \tif n == 0 {\n\
         \t\treturn 0\n\
         \t}\n\
         \treturn down(n-1) + 1\n\
         }\n\
         func sum(xs ...int) int {\n\
         \tt := 0\n\
         \tfor _, x := range xs {\n\
         \t\tt += x\n\
         \t}\n\
         \treturn t\n\
         }\n\
         func main() {\n\
         \tprint(down(998), \" \")\n\
         \tprint(sum(), \" \", sum(1, 2, 3), \" \")\n\
         \tnums := []int{4, 5}\n\
         \tprint(sum(nums...))\n\
         }\n",
    );
    assert_eq!(got, "998 0 6 9");
}

/// `T-EX-18`：多返回值 —— `a, b := f()`、`a, b = f()`、`g(f())`、`return f()`。
#[test]
fn t_ex_18_multiple_returns() {
    let got = out(
        "t_ex_18_multi",
        "package main\n\
         func two() (int, int) { return 1, 2 }\n\
         func sum2(a, b int) int { return a + b }\n\
         func fwd() (int, int) { return two() }\n\
         func main() {\n\
         \ta, b := two()\n\
         \tprint(a, b, \" \")\n\
         \ta, b = b, a\n\
         \tprint(a, b, \" \")\n\
         \tprint(sum2(two()), \" \")\n\
         \tc, d := fwd()\n\
         \tprint(c, d)\n\
         }\n",
    );
    assert_eq!(got, "12 21 3 12");
}

/// `T-EX-19`：命名返回值 + 裸 `return`。
#[test]
fn t_ex_19_named_results() {
    let got = out(
        "t_ex_19_named",
        "package main\n\
         func f() (x int, y string) {\n\
         \tx = 7\n\
         \ty = \"ok\"\n\
         \treturn\n\
         }\n\
         func g() (n int) {\n\
         \tdefer func() { n = n * 2 }()\n\
         \treturn 3\n\
         }\n\
         func main() {\n\
         \ta, b := f()\n\
         \tprint(a, b, \" \", g())\n\
         }\n",
    );
    assert_eq!(got, "7ok 6");
}

/// `T-EX-20`：有返回值的调用**当语句** ⇒ 合法且结果丢弃（B 族修复的正向 fixture）。
#[test]
fn t_ex_20_call_statement_drops_result() {
    let got = out(
        "t_ex_20_drop",
        "package main\n\
         var n int\n\
         func bump() int { n++; return n }\n\
         func main() {\n\
         \tbump()\n\
         \tbump()\n\
         \tprint(n)\n\
         }\n",
    );
    assert_eq!(got, "2");
}

/// `T-EX-21`：控制流 —— if / for 三形态 / switch（有 tag、无 tag、`fallthrough`）/ goto / 标签。
#[test]
fn t_ex_21_control_flow() {
    let got = out(
        "t_ex_21_flow",
        "package main\n\
         func main() {\n\
         \ts := 0\n\
         \tfor i := 0; i < 3; i++ {\n\
         \t\ts += i\n\
         \t}\n\
         \tj := 0\n\
         \tfor j < 2 {\n\
         \t\tj++\n\
         \t}\n\
         \tfor {\n\
         \t\tj++\n\
         \t\tif j > 3 {\n\
         \t\t\tbreak\n\
         \t\t}\n\
         \t}\n\
         \tprint(s, j, \" \")\n\
         \tswitch 2 {\n\
         \tcase 1:\n\
         \t\tprint(\"a\")\n\
         \tcase 2:\n\
         \t\tprint(\"b\")\n\
         \t\tfallthrough\n\
         \tcase 3:\n\
         \t\tprint(\"c\")\n\
         \tdefault:\n\
         \t\tprint(\"d\")\n\
         \t}\n\
         \tswitch {\n\
         \tcase s > 100:\n\
         \t\tprint(\"X\")\n\
         \tdefault:\n\
         \t\tprint(\"Y\")\n\
         \t}\n\
         \tn := 0\n\
         loop:\n\
         \tfor i := 0; i < 3; i++ {\n\
         \t\tfor {\n\
         \t\t\tn++\n\
         \t\t\tbreak loop\n\
         \t\t}\n\
         \t}\n\
         \tprint(n, \" \")\n\
         \ti := 0\n\
         again:\n\
         \ti++\n\
         \tif i < 2 {\n\
         \t\tgoto again\n\
         \t}\n\
         \tprint(i)\n\
         }\n",
    );
    assert_eq!(got, "34 bcY1 2");
}

/// `T-EX-22`：`range` 的四形态（array / slice / string / map / int）。
#[test]
fn t_ex_22_range_forms() {
    let got = out(
        "t_ex_22_range",
        "package main\n\
         func main() {\n\
         \tarr := [2]int{10, 20}\n\
         \tfor i, v := range arr {\n\
         \t\tprint(i, \":\", v, \" \")\n\
         \t}\n\
         \tsl := []int{30}\n\
         \tfor i := range sl {\n\
         \t\tprint(i, \" \")\n\
         \t}\n\
         \tfor i, r := range \"ab\" {\n\
         \t\tprint(i, \":\", r, \" \")\n\
         \t}\n\
         \tm := map[string]int{\"x\": 1}\n\
         \tfor k, v := range m {\n\
         \t\tprint(k, \":\", v, \" \")\n\
         \t}\n\
         \tfor i := range 3 {\n\
         \t\tprint(i)\n\
         \t}\n\
         }\n",
    );
    assert_eq!(got, "0:10 1:20 0 0:97 1:98 x:1 012");
}

/// `T-EX-23`：内建全集各一条。
#[test]
fn t_ex_23_builtins() {
    let got = out(
        "t_ex_23_builtin",
        "package main\n\
         func main() {\n\
         \tsl := make([]int, 2, 5)\n\
         \tprint(len(sl), cap(sl), \" \")\n\
         \tp := new(int)\n\
         \t*p = 4\n\
         \tprint(*p, \" \")\n\
         \tsl = append(sl, 1, 2)\n\
         \tprint(len(sl), \" \")\n\
         \tc := make([]int, 2)\n\
         \tprint(copy(c, sl), \" \")\n\
         \tm := map[string]int{\"a\": 1, \"b\": 2}\n\
         \tdelete(m, \"a\")\n\
         \tprint(len(m), \" \")\n\
         \tclear(m)\n\
         \tprint(len(m), \" \")\n\
         \tprint(min(3, 1), max(3, 1), \" \")\n\
         \tcn := complex(1, 2)\n\
         \tprint(real(cn), imag(cn))\n\
         }\n",
    );
    assert_eq!(got, "25 4 4 2 1 0 13 12");
}

/// `T-EX-24`：`print` / `println` 的**格式表**（`F10` 冻结的 7 个点）。
#[test]
fn t_ex_24_print_format_table() {
    let got = out(
        "t_ex_24_print",
        "package main\n\
         func main() {\n\
         \tprint(1, 2)\n\
         \tprintln(\"a\", \"b\")\n\
         \tprintln(1000000.0)\n\
         \tprintln(0.0001)\n\
         \tprintln(0.00001)\n\
         \tprintln(1e-7)\n\
         \tvar f float32 = 1\n\
         \tprintln(f / 3)\n\
         \tprintln(complex(-1, -2))\n\
         \tvar p *int\n\
         \tprintln(p)\n\
         \tprintln('A')\n\
         \tprintln(byte(65))\n\
         }\n",
    );
    assert_eq!(
        got,
        "12a b\n1e+06\n0.0001\n1e-05\n1e-07\n0.33333334\n(-1-2i)\n0x0\n65\n65\n"
    );
}

// ═══════════════════════ EX-4：闭包与接口 ═══════════════════════

/// `T-EX-25`：闭包**按变量捕获** —— 外部改动可见，两个闭包共享同一变量。
#[test]
fn t_ex_25_closure_captures_variable() {
    let got = out(
        "t_ex_25_closure",
        "package main\n\
         func main() {\n\
         \tx := 1\n\
         \tinc := func() { x++ }\n\
         \tget := func() int { return x }\n\
         \tinc()\n\
         \tprint(get(), \" \")\n\
         \tx = 10\n\
         \tprint(get(), \" \")\n\
         \tinc()\n\
         \tprint(x)\n\
         }\n",
    );
    assert_eq!(got, "2 10 11");
}

/// `T-EX-26`：闭包的**生命周期** —— 函数返回后捕获变量仍活着。
#[test]
fn t_ex_26_closure_outlives_frame() {
    let got = out(
        "t_ex_26_life",
        "package main\n\
         func counter() func() int {\n\
         \tn := 0\n\
         \treturn func() int {\n\
         \t\tn++\n\
         \t\treturn n\n\
         \t}\n\
         }\n\
         func main() {\n\
         \tc := counter()\n\
         \tc()\n\
         \tc()\n\
         \tprint(c(), \" \")\n\
         \td := counter()\n\
         \tprint(d())\n\
         }\n",
    );
    assert_eq!(got, "3 1");
}

/// `T-EX-27`：`loopvar`（Go 1.22）—— 每轮**新建**迭代变量。
#[test]
fn t_ex_27_loopvar_122() {
    let got = out(
        "t_ex_27_loopvar",
        "package main\n\
         func main() {\n\
         \tvar fs []func()\n\
         \tfor i := 0; i < 3; i++ {\n\
         \t\tfs = append(fs, func() { print(i) })\n\
         \t}\n\
         \tfor _, f := range fs {\n\
         \t\tf()\n\
         \t}\n\
         }\n",
    );
    assert_eq!(got, "012");
}

/// `T-EX-28`：方法值（`x.M` 绑定接收者）与方法表达式（`T.M` 首参是接收者）。
#[test]
fn t_ex_28_method_value_and_expression() {
    let got = out(
        "t_ex_28_method",
        "package main\n\
         type S struct{ A int }\n\
         func (s S) Get() int { return s.A }\n\
         func (s *S) Set(v int) { s.A = v }\n\
         func main() {\n\
         \ts := S{1}\n\
         \tget := s.Get\n\
         \tset := s.Set\n\
         \tset(2)\n\
         \tprint(get(), s.A, \" \")\n\
         \tf := S.Get\n\
         \tprint(f(S{3}), \" \")\n\
         \tg := (*S).Set\n\
         \tg(&s, 4)\n\
         \tprint(s.A)\n\
         }\n",
    );
    assert_eq!(got, "12 3 4");
}

/// `T-EX-29`：接口与 **typed nil** —— `var p *T; var i I = p` ⇒ `i != nil`。
#[test]
fn t_ex_29_interface_typed_nil() {
    let got = out(
        "t_ex_29_typednil",
        "package main\n\
         type I interface{ M() int }\n\
         type T struct{ A int }\n\
         func (t *T) M() int {\n\
         \tif t == nil {\n\
         \t\treturn -1\n\
         \t}\n\
         \treturn t.A\n\
         }\n\
         func main() {\n\
         \tvar p *T\n\
         \tvar i I = p\n\
         \tprint(i != nil, \" \", i.M(), \" \")\n\
         \tvar j I\n\
         \tprint(j == nil, \" \")\n\
         \tk := I(&T{5})\n\
         \tprint(k.M(), \" \", k != nil)\n\
         }\n",
    );
    assert_eq!(got, "true -1 true 5 true");
}

/// `T-EX-30`：类型断言 —— 单值（失败 panic）/ 双值（失败 `ok=false`）/ 接口到接口。
#[test]
fn t_ex_30_type_assertion() {
    let got = out(
        "t_ex_30_assert",
        "package main\n\
         type I interface{ M() int }\n\
         type J interface{ M() int }\n\
         type T struct{ A int }\n\
         func (t *T) M() int { return t.A }\n\
         func main() {\n\
         \tvar i I = &T{7}\n\
         \tt := i.(*T)\n\
         \tprint(t.A, \" \")\n\
         \t_, ok := i.(*T)\n\
         \tprint(ok, \" \")\n\
         \tvar j J = i.(J)\n\
         \tprint(j.M())\n\
         }\n",
    );
    assert_eq!(got, "7 true 7");

    let o = run_src(
        "t_ex_30_fail",
        "package main\n\
         type I interface{ M() int }\n\
         type T struct{ A int }\n\
         func (t *T) M() int { return t.A }\n\
         type U struct{ B int }\n\
         func (u *U) M() int { return u.B }\n\
         func main() { var i I = &T{1}; _ = i.(*U) }\n",
    );
    assert_eq!(o.exit, 2, "单值断言失败应当 panic");
    assert!(
        o.stdout.contains("interface conversion"),
        "文本不对：{}",
        o.stdout
    );
}

/// `T-EX-31`：`type switch` 的各 case 与绑定变量的类型。
#[test]
fn t_ex_31_type_switch() {
    let got = out(
        "t_ex_31_typeswitch",
        "package main\n\
         func kind(v interface{}) string {\n\
         \tswitch x := v.(type) {\n\
         \tcase nil:\n\
         \t\treturn \"nil\"\n\
         \tcase int:\n\
         \t\treturn \"int\"\n\
         \tcase string:\n\
         \t\treturn \"str:\" + x\n\
         \tdefault:\n\
         \t\treturn \"other\"\n\
         \t}\n\
         }\n\
         func main() {\n\
         \tprint(kind(nil), \" \", kind(1), \" \", kind(\"a\"), \" \", kind(1.5))\n\
         }\n",
    );
    assert_eq!(got, "nil int str:a other");
}

/// `T-EX-32`：泛型**单态化**（已兑现的子集：`any` 约束 + 类型推断）。
///
/// 显式实例化 `F[int]` 与泛型类型 `Box[T]` 实测**未兑现**（检查器把 `F[int]`
/// 当成「对函数取下标」）。这一格登记在 `M5-report.md` §5.6 与 manifest 的
/// `open_gaps` 里，本测试**只**断言已兑现的部分 —— 白名单纪律：未兑现的不得
/// 记为 pass。
#[test]
fn t_ex_32_generics_monomorphization() {
    let got = out(
        "t_ex_32_generic",
        "package main\n\
         func Id[T any](x T) T { return x }\n\
         func main() {\n\
         \tprint(Id(1), \" \")\n\
         \tprint(Id(\"s\"))\n\
         }\n",
    );
    assert_eq!(got, "1 s");
}

/// `T-EX-33`：泛型**超出支持面**的形态必须被拒（退出码 1、stdout 空、带稳定码）。
///
/// 三种超面形态各一条，覆盖三条不同的失败路径：
///
/// | 形态 | 实测拒绝码 | 卡在哪 |
/// |---|---|---|
/// | 显式实例化 `F[int](1)` | `CHK-INVALID-INDEX` | 检查器把 `F[int]` 读成「对函数取下标」 |
/// | 泛型类型字面量 `Box[int]{…}` | `CHK-INVALID-SELECTOR` | 实例化后的类型是 invalid，取不到字段 |
/// | 带类型集约束的泛型函数 `Sum([]int{…})` | `CHK-INVALID-ASSIGN` | 推断不出 `T`，实参对不上 |
///
/// ⚠️ **判据的措辞要收紧**：`M5-tests.md` 要求的是「**稳定拒绝码**」，而这三种
/// 拿到的都是**通用的**检查诊断码（`CHK-*`），不是专属的
/// `HIR-UNSUPPORTED-GENERIC`。后者在 HIR 层**存在**且能用（`RejectKind::code()`），
/// 但只在「check 过了、lower 才拒」时才会走到 —— 这三种形态连 check 都过不了。
///
/// 所以 `T-EX-33` 的 verdict 是 **partial**（`G-M5-04`）：
///
/// * **兑现**：拒绝是**确定**的（同一输入每次同一个码）、**响亮**的（退出码 1、
///   stdout 空）、**不会静默算错**。这三条正是这条判据想防的事。
/// * **未兑现**：码不是专属的。要让 `F[int]` 走到 `HIR-UNSUPPORTED-GENERIC`，
///   得先让检查器认识实例化表达式 —— 那是 M4/M6 的类型表示工作。
///
/// 把三条码**写死**在这里：它们一旦变了（比如以后真接上专属码），这条测试会红，
/// 提醒改 manifest 的 verdict 与 `G-M5-04`，而不是悄悄漂移。
#[test]
fn t_ex_33_out_of_surface_generics_are_rejected() {
    // 显式实例化：官方 `go run` 打印 1，本仓拒。
    rejected(
        "t_ex_33_explicit",
        "package main\n\
         func F[T any](v T) T { return v }\n\
         func main() {\n\
         \tprintln(F[int](1))\n\
         }\n",
        "CHK-INVALID-INDEX",
    );
    // 泛型类型字面量：官方打印 7，本仓拒。
    rejected(
        "t_ex_33_gentype",
        "package main\n\
         type Box[T any] struct{ v T }\n\
         func main() {\n\
         \tb := Box[int]{v: 7}\n\
         \tprintln(b.v)\n\
         }\n",
        "CHK-INVALID-SELECTOR",
    );
    // 类型集约束 + 切片实参：官方打印 6 / 4，本仓拒。
    rejected(
        "t_ex_33_typeset",
        "package main\n\
         type Num interface{ ~int | ~float64 }\n\
         func Sum[T Num](xs []T) T {\n\
         \tvar s T\n\
         \tfor _, v := range xs {\n\
         \t\ts += v\n\
         \t}\n\
         \treturn s\n\
         }\n\
         func main() {\n\
         \tprintln(Sum([]int{1, 2, 3}))\n\
         }\n",
        "CHK-INVALID-ASSIGN",
    );
}

// ═══════════════════════ EX-5：defer / panic / recover ═══════════════════════

/// `T-EX-34`：`defer` 的**参数在注册时求值** + LIFO。
#[test]
fn t_ex_34_defer_args_and_lifo() {
    let got = out(
        "t_ex_34_defer",
        "package main\n\
         func show(tag string, v int) { print(tag, v, \" \") }\n\
         func main() {\n\
         \tx := 1\n\
         \tdefer show(\"a\", x)\n\
         \tx = 2\n\
         \tdefer show(\"b\", x)\n\
         \tx = 3\n\
         \tdefer show(\"c\", x)\n\
         \tprint(\"body \", x, \" \")\n\
         }\n",
    );
    assert_eq!(got, "body 3 c3 b2 a1 ");
}

/// `T-EX-35`：`recover` 的**直接调用限制** —— 只有 defer 里的直接调用有效。
#[test]
fn t_ex_35_recover_direct_call_only() {
    let got = out(
        "t_ex_35_recover",
        "package main\n\
         func bad() (r interface{}) { return recover() }\n\
         func a() {\n\
         \tdefer func() { print(\"A\", recover() != nil, \" \") }()\n\
         \tpanic(\"x\")\n\
         }\n\
         func b() {\n\
         \tdefer func() {\n\
         \t\tdefer func() { print(\"B\", recover() != nil, \" \") }()\n\
         \t\trecover()\n\
         \t}()\n\
         \tpanic(\"y\")\n\
         }\n\
         func c() {\n\
         \tdefer func() {\n\
         \t\tprint(\"C\", bad() != nil, \" \")\n\
         \t\trecover()\n\
         \t}()\n\
         \tpanic(\"z\")\n\
         }\n\
         func main() {\n\
         \ta()\n\
         \tb()\n\
         \tc()\n\
         }\n",
    );
    assert_eq!(got, "Atrue Bfalse Cfalse ");
}

/// `T-EX-36`：`recover` 后改写**命名**返回值；未命名结果不可改写。
#[test]
fn t_ex_36_recover_named_result() {
    let got = out(
        "t_ex_36_named",
        "package main\n\
         func f() (n int) {\n\
         \tdefer func() {\n\
         \t\tif recover() != nil {\n\
         \t\t\tn = 42\n\
         \t\t}\n\
         \t}()\n\
         \tpanic(\"boom\")\n\
         }\n\
         func g() int {\n\
         \tv := 0\n\
         \tdefer func() {\n\
         \t\tif recover() != nil {\n\
         \t\t\tv = 42\n\
         \t\t}\n\
         \t}()\n\
         \tpanic(\"boom\")\n\
         }\n\
         func main() {\n\
         \tprint(f(), \" \", g())\n\
         }\n",
    );
    assert_eq!(got, "42 0");
}

/// `T-EX-37`：展开中再 `panic` ⇒ 覆盖原值继续展开。
#[test]
fn t_ex_37_nested_panic_overrides() {
    let o = run_src(
        "t_ex_37_nested",
        "package main\n\
         func main() {\n\
         \tdefer func() {\n\
         \t\tif r := recover(); r != nil {\n\
         \t\t\tprint(\"got \", r)\n\
         \t\t}\n\
         \t}()\n\
         \tdefer func() { panic(\"second\") }()\n\
         \tpanic(\"first\")\n\
         }\n",
    );
    assert_eq!(o.exit, 0, "内层 recover 应当接住第二次 panic");
    assert_eq!(o.stdout, "got second");
}

/// `T-EX-38`：未捕获 `panic` ⇒ 合并流含 `panic: <值>` 前缀、退出码 2。
#[test]
fn t_ex_38_uncaught_panic() {
    let o = run_src(
        "t_ex_38_uncaught",
        "package main\nfunc main() { panic(\"boom\") }\n",
    );
    assert_eq!(o.exit, 2);
    assert!(
        o.stdout.starts_with("panic: boom"),
        "未捕获 panic 的合并流应以 `panic: boom` 开头：{:?}",
        o.stdout
    );

    // 非字符串值也走同一条格式：`panic: 7`
    let o2 = run_src("t_ex_38_int", "package main\nfunc main() { panic(7) }\n");
    assert_eq!(o2.exit, 2);
    assert!(o2.stdout.starts_with("panic: 7"), "{:?}", o2.stdout);
}

/// `T-EX-39`：单包的初始化顺序 —— 全局变量按 `InitOrder` 求值，`init` 在 `main` 之前。
#[test]
fn t_ex_39_init_order() {
    let got = out(
        "t_ex_39_init",
        "package main\n\
         var a = mark(\"a\", 1)\n\
         var b = mark(\"b\", a+1)\n\
         var c = mark(\"c\", b+1)\n\
         func mark(name string, v int) int {\n\
         \tprint(name)\n\
         \treturn v\n\
         }\n\
         func init() { print(\"i1\") }\n\
         func init() { print(\"i2\") }\n\
         func main() { print(\"m\", c) }\n",
    );
    assert_eq!(got, "abci1i2m3");
}

// ── `T-EX-40`（双包初始化顺序）**未兑现** ──────────────────────────────
//
// 实测：`import _ "side"` 之后，`side` 的 `init` **不执行**（只输出本包的
// `I` 和 `M`）。原因是 HIR 是**单包**的 —— `rgoc_hir::lower` 只 lower 入口包，
// 跨包引用本身在 M5 面外（`跨包引用 side.V 不在 M5 面（单包）`）。
//
// 所以这一格**没有**通过的测试撑着：manifest 里记 `partial`，`open_gaps`
// 点名，`M5-report.md` §5.7 写清「双包 init 序未兑现」。留一条永远红的测试
// 在套件里只会让人习惯性忽略红 —— 那比缺一条测试更坏。
//
// 单包内的初始化顺序（`T-EX-39`）是**兑现**的，见上面那条。

// ═══════════════════════ 丙腿：装置与 CLI（`H` 的一部分） ═══════════════════════

/// `T-EX-49`：`run` 的退出码表 —— 0 正常 / 1 静态诊断 / 2 未捕获 panic 与预算。
#[test]
fn t_ex_49_exit_code_table() {
    // 0：跑通
    assert_eq!(
        run_src("t_ex_49_ok", "package main\nfunc main() {}\n").exit,
        0
    );
    // 1：静态诊断（未定义的名字）
    let bad = run_src(
        "t_ex_49_diag",
        "package main\nfunc main() { print(nosuchname) }\n",
    );
    assert_eq!(bad.exit, 1, "静态诊断应当退出 1");
    assert!(!bad.stderr.is_empty(), "静态诊断必须进 stderr");
    // 2：未捕获 panic
    assert_eq!(
        run_src("t_ex_49_panic", "package main\nfunc main() { panic(1) }\n").exit,
        2
    );
    // 2：参数/io 错（文件不存在）⇒ `run_run` 返回 Err，由 CLI 翻成 2
    let err = run_run(&RunCommand {
        files: vec![PathBuf::from("/nonexistent/rgoc-fixture.go")],
        dump_hir: false,
        max_steps: None,
        lang: None,
        via: rgoc_driver::cli::Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
    });
    assert!(err.is_err(), "读不到文件应当返回 Err（CLI 侧退出码 2）");
}

/// `T-EX-50`：合并流的**交错顺序**不能丢（`print`/`println` 写同一根管子）。
#[test]
fn t_ex_50_merged_stream_interleaving() {
    let got = out(
        "t_ex_50_merged",
        "package main\nfunc main() { print(\"a\"); println(1); print(\"b\") }\n",
    );
    assert_eq!(got, "a1\nb");
}

/// `T-EX-51`：`--dump-hir` 与诊断**通道分离**（dump 只进 stdout，诊断只进 stderr）。
#[test]
fn t_ex_51_dump_channel_separation() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/run-semantics");
    std::fs::create_dir_all(&dir).expect("建不了目录");
    let good = dir.join("t_ex_51_good.go");
    std::fs::write(&good, "package main\nfunc main() { print(\"ZZZ\") }\n").expect("写不了");
    let o = run_run(&RunCommand {
        files: vec![good],
        dump_hir: true,
        max_steps: None,
        lang: None,
        via: rgoc_driver::cli::Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
    })
    .expect("内部错");
    assert_eq!(o.exit, 0);
    assert!(
        o.stderr.is_empty(),
        "dump 模式下 stderr 应为空：{}",
        o.stderr
    );
    assert!(o.stdout.contains("# program"), "stdout 应是 dump");
    assert_ne!(
        o.stdout, "ZZZ",
        "给了 --dump-hir 就**不该**跑程序（stdout 就是程序输出本身）"
    );
    assert!(
        o.stdout.ends_with('\n'),
        "dump 必须以换行结束（最后一条节点也要有行尾）"
    );

    // 有诊断时：stdout 空、stderr 有内容、退出码 1
    let bad = dir.join("t_ex_51_bad.go");
    std::fs::write(&bad, "package main\nfunc main() { print(nope) }\n").expect("写不了");
    let o2 = run_run(&RunCommand {
        files: vec![bad],
        dump_hir: true,
        max_steps: None,
        lang: None,
        via: rgoc_driver::cli::Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
    })
    .expect("内部错");
    assert_eq!(o2.exit, 1);
    assert!(o2.stdout.is_empty(), "有诊断时不该有 dump");
    assert!(!o2.stderr.is_empty());
}

/// `T-EX-52`：三道上限**各自响亮失败**（退出码 2 + 稳定消息 + 不挂死）。
///
/// 帧深与步数各造一个超限 fixture。求值栈那道（10000）由解释器内部判，
/// 这里不单独造 —— 它撞的是同一个 `Halt::Budget` 出口。
#[test]
fn t_ex_52_budget_limits_fail_loudly() {
    // 帧深：无限递归（每层 +1，不返回）
    let o = run_src(
        "t_ex_52_frames",
        "package main\n\
         func f(n int) int { return f(n+1) }\n\
         func main() { print(f(0)) }\n",
    );
    assert_eq!(o.exit, 2, "帧深超限应当退出 2");
    assert!(
        o.stderr.contains("frame depth") || o.stdout.contains("frame depth"),
        "帧深超限的消息不稳定：stdout={:?} stderr={:?}",
        o.stdout,
        o.stderr
    );

    // 步数：死循环（`--max-steps` 调到很小，避免测试跑满 1e8）
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/run-semantics");
    std::fs::create_dir_all(&dir).expect("建不了目录");
    let spin = dir.join("t_ex_52_steps.go");
    std::fs::write(&spin, "package main\nfunc main() { for {} }\n").expect("写不了");
    let o2 = run_run(&RunCommand {
        files: vec![spin],
        dump_hir: false,
        max_steps: Some(10_000),
        lang: None,
        via: rgoc_driver::cli::Via::Hir,
        opt: rgoc_ssa::opt::Config::all(),
    })
    .expect("内部错");
    assert_eq!(o2.exit, 2, "步数超限应当退出 2");
    assert!(
        o2.stderr.contains("step budget") || o2.stdout.contains("step budget"),
        "步数超限的消息不稳定：stdout={:?} stderr={:?}",
        o2.stdout,
        o2.stderr
    );
}
