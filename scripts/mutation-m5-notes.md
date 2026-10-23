# M5 变异反向验证记录（`T-EX-63..70`，`M5-tests.md` §3.5 反腿）

> 人读记录，纪律同 `mutation-m3-notes.md` / `mutation-m4-notes.md`：每条 = **施加点 +
> 命令 + 变红摘要 + 复原确认**。全部在容器内跑（`scripts/in-container.sh`），
> 每条命令都是 `cd /work/rgoc` 之后的形态（`R-9-1`）。
>
> 施加方式：临时改一处 → 跑**目标测试**（cargo 自己会重编，`Compiling` 出现即证明
> 跑的是新二进制）→ 复原 → 再跑一次确认回到绿。
>
> **编号与文档的对应**：`T-EX-63..70` 在 `M5-tests.md` §3.5 是**判据行**，本文件是
> 它们的**执行记录**。下表「期望红在」是文档冻结时的预测，第四列是实测 ——
> 两者不一致的地方在第 8 条，那是本轮唯一一个**预测没兑现**的，处置见该条。

## 汇总

| ID | 变异 | 施加点 | 实测 |
|---|---|---|---|
| `T-EX-63` | 数组赋值改成共享 `Rc`（不深拷贝） | `rgoc-interp/src/value.rs`：`Value::Array` 的赋值/传参改 `Rc::clone` | **红** `t_ex_11`（`9 9 2 7 7` vs `1 9 2 1 1`） |
| `T-EX-64` | slice 赋值改成深拷贝 | `rgoc-interp/src/value.rs`：`Value::Slice` 走 `deep_copy` | **红** `t_ex_13`（`1 1 2 3 3 3 0` vs `9 1 2 3 3 3 9`） |
| `T-EX-65` | defer 参数改成**执行时**求值 | `rgoc-interp/src/interp.rs`：`Pending::Defer` 记下裸槽实参，teardown 时重读 | **红** `t_ex_34`（`body 3 c3 b3 a3 ` vs `body 3 c3 b2 a1 `） |
| `T-EX-66` | `recover` 改成「任何 defer 里都有效」 | `rgoc-interp/src/interp.rs`：两处 `Recover` 分支的 `ok` 恒 `true` | **红** `t_ex_35`（`Atrue Bfalse Ctrue` vs `Atrue Bfalse Cfalse`） |
| `T-EX-67` | 帧栈改成 Rust 递归（`D-M5-1` 的反向） | `rgoc-interp/src/interp.rs`：`drive` 拆出 `drive_to(floor)`、`call_func` 尾部就地递归、删掉帧深守卫 | **红** `t_ex_17` + `t_ex_52`（`has overflowed its stack` / SIGABRT） |
| `T-EX-68` | 合并流改成只比 stdout | `rgoc-interp/src/interp.rs`：`println` 的字节改走侧管，合并流只剩 `print` | **红** `t_ex_06` / `t_ex_24` / `t_ex_50` |
| `T-EX-69` | 未支持构造改成静默跳过 | `rgoc-hir/src/lower.rs`：`reject()` 变成空函数 | **红** `t_ex_02` |
| `T-EX-70` | `run-oracle` 把「无 `.out`」当成「不比」 | `rgoc-driver/tests/run_corpus.rs`：`NotFound` 分支 `String::new()` → `continue` | **先绿（缺口）**，补分母后**红**（`5` vs `69`） |

**8/8 全部实证变红**（`T-EX-70` 是补上分母之后才红的，见该条 —— 这正是本轮
唯一有价值的发现）。

## 逐条现场

### `T-EX-63` 数组改成共享

```
test t_ex_11_array_value_semantics ... FAILED
  left: "9 9 2 7 7"
 right: "1 9 2 1 1"
```

数组是**值**：`b := a` 之后改 `b[0]` 不能影响 `a`。共享 `Rc` 让两侧看见同一个底层
数组 ⇒ 第一个 `print` 就打错。`t_ex_12`（struct 拷贝/嵌入）同族，未单列。

### `T-EX-64` slice 改成深拷贝

```
test t_ex_13_slice_alias_and_growth ... FAILED
  left: "1 1 2 3 3 3 0"
 right: "9 1 2 3 3 3 9"
```

slice 与数组**相反**：头是值、底层数组是引用。深拷贝让 `append` 之后写 `s[0]`
不再回传给原 slice ⇒ 期望的 `9 … 9` 变成 `1 … 0`。

### `T-EX-65` defer 参数执行时求值

```
test t_ex_34_defer_args_and_lifo ... FAILED
  left: "body 3 c3 b3 a3 "
 right: "body 3 c3 b2 a1 "
```

`D-M5-9`：`defer f(x)` 的实参在**注册那一刻**求值（Go 规范原文 "the arguments are
evaluated when the defer statement is executed"）。改成 teardown 时重读裸槽 ⇒
三个 defer 都拿到最终值 `3`，LIFO 顺序本身没坏（`c3 b3 a3` 仍是逆序），所以这条
测试**同时**钉住了两件事：求值时机与顺序 —— 只有时机错时顺序看起来还是对的。

### `T-EX-66` recover 放宽

```
test t_ex_35_recover_direct_call_only ... FAILED
  left: "Atrue Bfalse Ctrue "
 right: "Atrue Bfalse Cfalse "
```

三处 `recover()` 的位置各不相同：① defer 里直接调 ⇒ 有效（`Atrue`）；② defer 里
再套一层 defer ⇒ 外层已把 panic 取走（`Bfalse`）；③ 经普通函数 `bad()` 转一手 ⇒
**无效**（`Cfalse`）。变异把 ③ 也放行 ⇒ `Ctrue`。判据是「本帧是不是**被 defer 调用**
的那一帧」（`Frame::is_defer`），不是「当前有没有 defer 在跑」。

### `T-EX-67` 帧栈改 Rust 递归

```
thread 't_ex_17_recursion_and_variadic' has overflowed its stack
fatal runtime error: stack overflow, aborting
(signal: 6, SIGABRT)

thread 't_ex_52_budget_limits_fail_loudly' has overflowed its stack
fatal runtime error: stack overflow, aborting
```

施加方式（三处一起改才算「把帧栈换成 Rust 栈」）：

1. `drive()` 拆成 `drive()` + `drive_to(floor)`；`floor > 0` 时本层新建的帧一弹出
   就返回（不接管调用者的后续语句）；
2. `call_func` 尾部 `self.drive_to(fi)` —— 每层 Go 调用占一层 Rust 栈帧；
3. 删掉 `frames.len() >= max_frames` 那条显式守卫（深度改由 Rust 栈兜底）。

`t_ex_17` 只递归 998 层（`down(998)`，加上 `main` 共 999 帧，**刚好在 1000 的预算
之内**）—— 这正是它作为 `D-M5-1` 判据的原因：合法程序也会炸。`t_ex_52` 的无限
递归更直接。**注意**这两条是 SIGABRT（进程被杀），不是断言失败 —— 判红判据要接受
「测试二进制被信号终止」这一形态（`M1-design.md` §12 `R32` 记过第三形态）。

### `T-EX-68` 合并流只比 stdout

```
test t_ex_06_constant_convergence_keeps_every_bit ... FAILED
  left: ""
 right: "4611686018427387904\n-9223372036854775808\n...\n"

test t_ex_24_print_format_table ... FAILED
  left: "12"
 right: "12a b\n1e+06\n0.0001\n1e-05\n1e-07\n0.33333334\n(-1-2i)\n0x0\n65\n65\n"

test t_ex_50_merged_stream_interleaving ... FAILED
  left: "ab"
 right: "a1\nb"
```

施加方式：`println` 的字节另开一根侧管（`Interp::side`），`out` 里只剩 `print`。
这**等价于**「比较时只读 stdout」—— 因为 `println` 恰好是 M5 里唯一会被丢掉的
那一半。

`t_ex_50` 的 `"ab"` vs `"a1\nb"` 最能说明问题：**只比 stdout 不会报错，只会静默
少比一半字节**，交错顺序（`a` → `1\n` → `b`）也随之丢失。`t_ex_24` 是格式表，
几乎全用 `println`，所以直接塌成 `"12"`。

> 文档冻结时预测的是 `T-EX-24/42/50`；实测 `T-EX-06` 也红（它用 `println` 逐行
> 打印收敛后的常量）。`T-EX-42` 在 `run_corpus.rs` 里走的是另一条路径（它判
> 退出码/stderr/panic，不比字节），所以没红 —— 字节比对面是 `T-EX-43`。预测与
> 实测的差异只影响「期望红在」这一列，判据本身没变。

### `T-EX-69` 未支持构造静默跳过

```
test t_ex_02_unsupported_constructs_are_rejected_loudly ... FAILED
test result: FAILED. 40 passed; 1 failed; 1 ignored
```

施加方式：`Lowering::reject()` 变成空函数 ⇒ `finish()` 恒 `Ok` ⇒ `go` / `chan` /
`select` / `range` over func / `close` 五条构造**不报错也不执行**，程序"跑完"
退出码 0。

这条变异最危险的地方不是错，而是**看不出来**：静默跳过之后没有任何通道能区分
「这段代码不在 M5 面」与「这段代码本来就什么都不做」。所以 `EX-2`（诊断优先）
必须由 `T-EX-02` 这种**指名到码**的断言守，而不是靠「输出对不上」。

### `T-EX-70` 无 `.out` 当成不比 —— 本轮唯一的缺口

第一轮（**未补分母**）：

```
$ cargo test -p rgoc-driver --test run_corpus
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; finished in 3.63s
```

**变异施加了，测试全绿。** 原因：`three_way_agreement_against_dot_out` 当时只钉了
一个分母 —— `with_out == 5`（有 `.out` 的文件数）。而入选集 69 条里 **64 条是
「无 `.out` ⇒ 期望为空」**；把 `NotFound` 分支改成 `continue`，被跳过的正是这 64 条，
`with_out` 仍然等于 5 ⇒ 空集合上的全称命题，恒真。

第二轮（补上第二个分母后）：

```
test three_way_agreement_against_dot_out ... FAILED
assertion `left == right` failed: 入选集应逐条比较（含「无 `.out` ⇒ 期望为空」的 64 条），实际只比了 5 条
  left: 5
 right: 69
```

补的是 `compared` 计数 + `assert_eq!(compared, 69, …)`。两个分母分工：

| 计数 | 防的是 |
|---|---|
| `with_out == 5` | 「`.out` 全被删了/路径拼错了」⇒ 一个期望值都没有，却还绿着 |
| `compared == 69` | 「无 `.out` 直接跳过」⇒ 只剩 5 条在比，却还绿着 |

**这条是本轮唯一一个「变异打不响」的实例，也是本轮唯一一处测试改动。** 它同时说明
`M5-tests.md` §3.5 里「期望红在」这一列**不能当证据用** —— 预测是可以错的，
只有实测算数。

## 复原确认

八条全部复原后：

```
$ cargo test -p rgoc-driver --test run_semantics
test result: ok. 41 passed; 0 failed; 1 ignored

$ cargo test -p rgoc-driver --test run_corpus
test result: ok. 10 passed; 0 failed; 0 ignored
```

复原方式：施加前先 `cp <file> /tmp/mut_<file>.bak`，复原就是 `cp` 回来。
`rgoc-interp/`、`rgoc-hir/`、`run_corpus.rs` 在本轮**都还是未跟踪文件**
（M5 新增），所以 `git status` 看不出「有没有复原」—— 判据是下面两条：

```
$ grep -n '变异 #' rgoc/crates/rgoc-interp/src/*.rs \
                 rgoc/crates/rgoc-hir/src/*.rs \
                 rgoc/crates/rgoc-driver/tests/run_corpus.rs
（无输出）
```

外加两条测试回到绿（上面的 `41 passed` / `10 passed`）。**不能只靠 `git checkout`**
—— 对未跟踪文件它什么都不做，静默留着变异而"看起来复原了"。

> **不承诺**：本记录只证明「8 个关键判据各自可被单点破坏」，它**不**证明解释器的
> 全部语义正确 —— 后者由 `M5-report.md` 的差距清单与语料对撞的实测结论承担。

---

# P0 四族变异（`Task 7`，`T-EX-57..60` 的反向证据）

> **补做于 2026-10-23 收口期**（原执行漏了这一步，登记为 `G-M5-10`）。
> 这四条**不计入** `EX-9` 的 8 条门禁变异 —— 它们钉的是 Phase 0 的四处规则修复：
> 没有它们，`T-EX-57..60` 就**只有正向证据**（"修好了"），没有反向证据（"不修就一定红"）。
>
> 施加方式：`git checkout -- <file>` 复原（这四处的文件都**已入库**，与上面 8 条不同 ——
> 那 8 条当时改的是未跟踪文件，`git checkout` 对它们无效）。
> 每条都走 `cargo clean -p rgoc-check` 再跑（`mutation-m3-notes.md` §0 的产物陈旧陷阱）。

## 汇总

| ID | 变异（改回原状） | 施加点 | 实测（红在哪、报什么） | 复原 |
|---|---|---|---|---|
| `T-EX-57` | 调用分派只认 `Mode::Value`（函数值落到 `_` ⇒ invalid） | `rgoc-check/src/expr.rs:436`：`Mode::Value \| Mode::Variable` → `Mode::Value` | **红** `multi_value::t_ex_57_接受面_四形态零诊断`：`left: ["7:10\tassign", "14:10\tassign"]` vs `right: []` | ✅ 2 passed |
| `T-EX-58` | 表达式语句不再放行「有结果的调用」 | `rgoc-check/src/stmt.rs`：删掉 `is_call_stmt` 的提前 `return` | **红** `expr_stmt::t_ex_58_调用当语句_结果丢弃合法`：`left: ["11:2\tunused", "12:2\tunused", "13:2\tunused", "15:2\tunused", "16:2\tunused"]` | ✅ 2 passed |
| `T-EX-59` | `&` 不再承认复合字面量这个例外 | `rgoc-check/src/expr.rs:1813`：`addr(x, xpos, is_composite_lit(&e.x))` → `addr(x, xpos, false)` | **红** `addr_composite::t_ex_59_复合字面量取址合法`：9 条 `op-invalid`（`7:25` / `8:37` / `8:44` / `10:12` / `11:16` / `12:26` / `13:17` / `14:25` / `15:13`） | ✅ 2 passed |
| `T-EX-60` | 移位计数要求「与左操作数同类型」（原 bug 的形态） | `rgoc-check/src/expr.rs:2141`：`all_integer(tt, yt)` → `all_integer(tt, yt) && yt == xt` | **红** `shift_assign::t_ex_60_移位计数只需整数_接受面零诊断`：4 条 `shift`（`11:8` / `13:8` / `15:8` / `16:11`） | ✅ 2 passed |

**4/4 全部实证变红并复原**。

## 逐条现场

### `T-EX-57` 多值调用

```
---- t_ex_57_接受面_四形态零诊断 stdout ----
assertion `left == right` failed
  left: ["7:10\tassign", "14:10\tassign"]
 right: []
```

`b, x := f(1000, 99)`（`f` 是**函数类型的变量**）落进 `_ => Operand::invalid()`，
赋值面于是按「调用只返回 1 个值」报 `assign`。这正是 `method5.go:103` 的原 bug 形态。

### `T-EX-58` 调用当语句

```
---- t_ex_58_调用当语句_结果丢弃合法 stdout ----
  left: ["11:2\tunused", "12:2\tunused", "13:2\tunused", "15:2\tunused", "16:2\tunused"]
 right: []
```

5 条 `unused` 对应 5 种「有结果的调用当语句」形态。注意反向那条
（`t_ex_58` 的拒绝面：`x + 1` 当语句仍报）**在变异下也是绿的** —— 它不依赖这个分支，
说明这条变异只破坏「放行」而不破坏「该报的照报」，判据没有被顺带弄残。

### `T-EX-59` 复合字面量取址

```
---- t_ex_59_复合字面量取址合法 stdout ----
  left: ["7:25\top-invalid", "8:37\top-invalid", "8:44\top-invalid", "10:12\top-invalid",
         "11:16\top-invalid", "12:26\top-invalid", "13:17\top-invalid", "14:25\top-invalid",
         "15:13\top-invalid"]
 right: []
```

9 条覆盖 `&T{...}` 的九种写法（含括号、嵌套字段、slice/map 字面量、`&struct{...}{}`）。
反向面（`&f()` / `&1` / `&m[k]` 仍拒）同样保持绿。

### `T-EX-60` 移位计数

```
---- t_ex_60_移位计数只需整数_接受面零诊断 stdout ----
  left: ["11:8\tshift", "13:8\tshift", "15:8\tshift", "16:11\tshift"]
 right: []
```

官方只要求**计数是整数**（`F3-D`），不要求与左操作数同类型。变异把类型相等这条
加回去，`i <<= c`（`c` 为 `uint`）立刻变红 —— 与 `ken/shift.go:82` 的原 bug 同形。

> **不承诺**：这 4 条只证明「四处修复各自可被单点破坏」。`T-EX-57..60` 的**完整**判据
> 还包括语料侧（`method5.go` / `divmod.go` / `compos.go` / `ken/shift.go` 转绿），
> 那半边由 `accept_rate_m5.rs`（79/80）与 `run_corpus.rs`（69/69）承担。
