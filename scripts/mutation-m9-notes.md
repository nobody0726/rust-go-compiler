# M9 变异探针笔记（10 条）

> 纪律：**变异 GREEN 先怀疑用例集**，不怀疑代码。这里逐条记实测结果，
> **绿的也写清为什么绿** —— 藏起来的话，下一轮会以为这条判据有牙。

复现：容器内 `bash scripts/mutation-m9.sh`（脚本里只有回归代价最低的 4 条：
01 / 02 / 03 / 08）。其余 6 条的执行期记录在本文。

## 一、脚本里那 4 条（红 4 / 失手 0）

| # | 变异 | 判据 | 结果 |
|---|---|---|---|
| `M-M9-01` | `needs_zero` 恒 `false`（⇒ `rt_alloc` 不清零） | `mem_alloc::含指针的类型要清零` | **红**（`2 passed; 1 failed`）|
| `M-M9-02` | `ptr_mask` 对 `string` 只留 1 个字 | `ptr_mask::五个特例的位图与_design_一致` | **红**（`5 passed; 1 failed`）|
| `M-M9-03` | `append_value` 扩容时不拷旧元素 | `ssa_run::t_slice_06_append_扩容要拷旧元素` | **红**（`33 passed; 2 failed`）|
| `M-M9-08` | `rt_defer_push` 改成 FIFO（defer 正序） | `native_corpus_run`（native vs 解释器逐字节） | **红**（`0 passed; 1 failed`，24.9 s）|

### `M-M9-03` 的两次尝试（一个真实的判断题）

第一版探针打的是 `-p rgoc-driver --test obj_slice`，结果是 **GREEN**。

诊断：`obj_slice` 走 `Emit::Elf`（**native** 通路），被变异的 `append_value` 在
`rgoc-interp` 里 —— 那条路根本不经过它。native 的 append 是 `rt.c` 里**另一份**
实现。所以「绿」不代表判据没牙，是**探针打错了靶**。

第二版换成 `ssa_run`，仍然 GREEN —— 这次是真问题：`ssa_run` 里唯一的 append 用例
（`t_ssa_35_内建_len_cap_append_copy`）append 的是 `make([]int,2,4)`（cap 够，**不扩容**）。
也就是说「扩容要拷旧元素」这条**只有 native 半边有判据**。

修法：补 `ssa_run::t_slice_06_append_扩容要拷旧元素`（`[]int{1,2}` 上 append，
断言 `3 1 2 3 2 1 2`）。补上之后变异立刻红。

## 二、另外 6 条

| # | 变异 | 判据（计划） | 实测 | 说明 |
|---|---|---|---|---|
| `M-M9-04` | `rt_map_del` 置 `empty` 而不是 `tomb` | `obj_map` | **GREEN** | 见 §3.1 |
| `M-M9-05` | 静态 ITab 的 `fn[]` 反序填 | `native_corpus_run` | **红**（24.9 s） | 接口派发打到错方法 |
| `M-M9-06` | `MakeClosure` 只填第一个捕获 | `ssa_run` | **GREEN** | 见 §3.2 |
| `M-M9-07` | `check_panic` 不跳本帧出口 | `native_corpus_run` | **GREEN** | 见 §3.3 |
| `M-M9-09` | `root_words` 退回「值是指针类型」 | `root_grain` + `live::t_root_05` | **红** | `[]int` 的位图变空 |
| `M-M9-10` | 回边不产 safepoint | `live::t_root_04` | **红** | `safepoints` 空 |

## 三、三条绿的诊断（都记成 open_gaps，不藏）

### 3.1 `M-M9-04`：`obj_map` 没有「删除后再插满」的用例

`rt_map_del` 置 `empty` 会把探测链**切断**：被删的键之后的键会查不到。要触发它
必须有「删一个 → 再插若干 → 查一个原先就存在的键」的形态。`obj_map` 的 7 条里
没有这一条（它测的是 delete 之后**该键**查不到，而那条在 `empty` 与 `tomb` 下
都成立）。

⇒ `G-M9-04`：`T-MAP-08` 的判据缺「探测链不被 empty 切断」。

### 3.2 `M-M9-06`：`ssa_run` 没有两个捕获的闭包

`captures.iter().take(1)` 只影响捕获数 ≥ 2 的闭包。`ssa_run` 的闭包用例都是
单捕获（`closure1.go` 那种），所以拿掉第二个捕获看不出来。

⇒ `G-M9-05`：`T-FUNC-05` 的求值器半边缺「两个捕获」的用例（native 侧有语料）。

### 3.3 `M-M9-07`：语料里的 panic 都发生在「之后本来就返回」的位置

`check_panic` 是 `R-9-24`（直接调用/方法调用之后也要查）的落点。去掉它之后，
被调方 panic、调用方继续执行 —— 但语料里所有 panic 之后调用方本来就走出口
（`defer + return`），所以**输出不变**。当初抓到这条 bug 的 fixture 是
`nat2_panic_终止符`（M9 自己写的），它不在 69 个语料里。

⇒ `G-M9-06`：`T-EXC-04` 的判据缺一条「panic 之后调用方还有可观察输出」的
native fixture。

> 三条都有一个共同形状：**判据只覆盖了「正确实现」的那一面**，没有覆盖
> 「实现错的时候会不会露头」。这正是变异存在的理由 —— 它们只有被改坏时才说话。
