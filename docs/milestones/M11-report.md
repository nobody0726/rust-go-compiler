# M11 报告 · native 并发运行时

> 契约：`C11`（并发语义）/ `C12`（堆与 GC，M11c 增补）/ `C9`（输出与退出码）。
> 台账：`M11-plan.md`（Task 0–46）/ `scripts/mutation-m11-notes.md`（16 条变异）。
> 清单：`M11-manifest.json`（数字都在那儿，由 `manifest_m11.rs` 对账）。

---

## 1. 读数（全部实测，可复跑）

| 项 | 读数 | 复跑命令 |
|---|---|---|
| 四条门禁 | fmt / check / clippy 全绿；**242 suite / 2278 passed / 0 failed / 2 ignored** | `cargo test --workspace --no-fail-fast` |
| 69 个单线程语料 × 三态 | **各 69/0/0** | `bash tools/run-oracle/run-native.sh --gc=off\|on\|stress` |
| 7 个并发语料 × 三态 | **各 7/0/0** + `M'` 12/12 每态 | `bash tools/run-oracle/run-native-chan.sh --gc=…` |
| 解释器侧（M8 的装置，不改） | ok=7 skip=12 bad=0 / 19 | `bash tools/run-oracle/run-chan.sh tools/run-oracle/chan-input.txt` |
| 三层一致 | 7 个 `N'`：native vs 解释器 vs 官方（静默成功） | `cargo test -p rgoc-driver --test go_corpus t_gcorp_01` |
| 变异 | **RED 16 / GREEN 0 / PENDING 0 / MISS 0** | `bash scripts/mutation-m11.sh --strict` |
| **溢出深度**（64 KiB 栈，新 G） | **N = 452**，N+1 = 453 溢出，两次跑逐字节相同 | `cargo test -p rgoc-driver --test go_stack t_stk_06 -- --nocapture` |
| 10 万个 G（依次创建+退出） | `heap=17 MB`（1 chunk）/ `rss=2848 KB` / `live=176 B` / `collections=300` | `t_groot_07` |
| 5000 次阻塞 select | `collections=2` / `frees=6026` / `live=176` | `t_chan_08` |
| 两个 ignored | `gc_corpus`（207 次编译）、`run_semantics`（`G-M5-01`）—— 都是刻意的 | `grep -rn "#\[ignore" rgoc/crates/*/tests/` |

---

## 2. 交付

### 2.1 栈与切换（M11a）

* **每个 G 一块固定栈**：`mmap(64 KiB + 一页余量)`，`stack_lo = base + 4096`。
  选固定大小而不是复制式增长：复制要重定位 stack map / 内部栈指针 / 闭包捕获，
  而这些正是 M10 刚冻结的东西（`U-M11-04`）。
* **`rt_switch`**：顶层 `__asm__`（整个函数，不是内联汇编），保存
  `x19..x28` + `x29/x30` + `d8..d15` = **160 字节**，压在**被挂起 G 自己的栈**上。
  存进 `gdesc.regs[]` 的话 GC 就得扫寄存器槽 ⇒ `t_gc_10`（没有寄存器根）失效。
* **序言栈检查**：`cmp sp, stack_lo; b.hs`，冷路径先 `mov sp, x9` 再报错（见 §4.4）。

### 2.2 调度（M11a）

M:1 / 不抢占 / 就绪队列 FIFO。`rt_schedule` 只在 `rt_goexit` 与阻塞点被调用。
`go` 只入队不切走 —— main 返回即退出（`t_exit_02`）。

### 2.3 `hchan` 与 `select`（M11b）

* `Hchan` **96 字节布局冻结**，11 条 `_Static_assert` 钉住每个偏移；
  `Sudog` 48 字节、`WaitSet` 内嵌 `Sudog`。
* 四操作的三态（`0` 阻塞 / `1` 真值 / `2` 关闭且空）+ `rt_send`/`rt_recv` 的
  **阻塞循环**（醒来要重新判断「是被取走」还是「被 close」）。
* `close` **只唤醒不代做**：唤醒 `recvq` 与 `sendq` 全部，各自的 G 醒来重走
  自己的路径（recv 先排空再零值；send 看到 closed 就在**自己的** G 里 panic）。
* `select` 三阶段整块在 C 里：`begin(n, default_idx)` / `add(token, idx, kind, chan, ptr)` /
  `wait(token) -> idx`，撤销时先 `waitq_contains` 再 remove。

### 2.4 多 G 根扫描（M11c）

* `g_frames` 变成「**当前 G** 的链头」：`rt_schedule` 两条赋值
  （`prev->frames = g_frames` / `g_frames = next->frames`），**序言/尾声一个字节不改**。
* `mark_all_g_stacks` 遍历**所有** gdesc（ready / blocked / exited 三种都扫），
  外加 `WaitSet` 与里面的 `Hchan*`。
* `Hchan` 从 `K_RAW` 换成带 desc 的 `K_OBJECT`（`buf` 之前没有根）。
* **`go` 的实参区**（第 10 类根）：`gdesc.argbytes` + 按字保守标 + trampoline 读完就清零。
* 退出 G 的栈 `munmap`、`gdesc` 槽复用（僵尸链表 + `rt_reap`）。

---

## 3. 关键决策的实测复核

| 决策 | 当初的理由 | 实测结果 |
|---|---|---|
| `D-M11-2` 64 KiB 固定栈 | 「够 1000 层」的估算**作废**，改成实测 | **452 层**（每帧 ~144 B）⇒ 估算高了一倍多；`sieve1`（25 个 G 的链）远够 |
| `D-M11-3` 保存区压栈上 | 存 `gdesc` 会让 `t_gc_10` 失效 | `t_gc_10` 原样通过；`t_stk_01` 逐个寄存器验对称 |
| `D-M11-10` `g_frames` 跟 G 走 | 不改序言/尾声 ⇒ M10 的 14 条栈根判据原样有效 | 全部原样通过（`gc_stack` 5/5 + `gc_reclaim` 6/6）|
| `D-M11-11` 扫所有 gdesc | 挂起 G 的栈上有活值 | `t_groot_01` 实现前**先红**（空输出），实现后绿 |
| `D-M11-12` 「a/b 强制关 GC」| roadmap 的启用门禁 | **从来没接线**：a/b 的对照靠装置侧 `RGOC_GC=0` ⇒ `T-GROOT-04` 退役 |
| `C11` §4.3 多就绪取源码第一个 | 与解释器一致（L1 才是主判据）| `t_sel_03` + 变异 `M-M11-09` 双向钉住 |

---

## 4. 真错清单（按「为什么先前没抓到」分类）

### 4.1 「判据没写」——台账打了勾，测试不存在

| 错 | 症状 | 怎么抓到的 |
|---|---|---|
| `t_stk_02` / `t_stk_06` 从未实现 | `GO-1` 挂着两条判据却打勾 | Task 40 全量回归时对着 `M11-tests.md` 逐条点名 |
| `t_go_03` 一直 `#[ignore]` | 「M11b 之后启用」然后没人回来 | 同上；启用时它本来就是绿的（说明行为对，只是没人验） |

补 `t_stk_06` 时**立刻抓到下面这条**：

### 4.2 「判据没见过那种形态」——溢出报错在新 G 上从来没生效过

序言检查写的是「`sp` 掉到界外才报」，而 `rt_panic_stack_overflow` 自己有栈帧
（`stp x29, x30, [sp, #-16]!`）⇒ 它的写入落在 `stack_lo` **之下** ⇒ 那里没映射
⇒ **SIGSEGV（`rc=-1`）**，不是「确定的文本 + 退出 2」。

`main` 之所以没事：它的 `stack_lo` 取「入口 `sp` − 4 MiB」，下面还有 4 MiB 真栈。
**新 G 的 `stack_lo` 就是映射边界**，一字节余量都没有。

修法两条：`G_STACK_SLACK`（`stack_lo` 之下留一页）+ 冷路径 `mov sp, x9`（一帧
最大 1 MiB，光靠一页不够）。判据 `t_stk_06` / `t_stk_07` + 变异 `M-M11-18`。

### 4.3 「根协议漏了一类」——`go` 的实参区

`go f(p)` 的实参被拷进**新 G 的栈顶**，而那块区域不在任何帧记录里
（trampoline 还没建帧）⇒ 闭包块（`fv` 在 arg0）与按值传的指针都没有根。
症状不是「读到垃圾」而是 **trampoline 调 `[fv+8]` 时撞 nil 检查 panic**。

定位过程（弯路都记下来）：先怀疑 `x29` 被踩 → 再怀疑 slot 复用 →
最后靠「panic 时的 `allocs` 读数 = 20005 = 全部已完成的分配」把范围锁到
send 之后，才想到「实参区根本不是根」。

同类还有：`Hchan` 是 `K_RAW` ⇒ 有缓冲通道的 `buf` 没根（`chan_desc` 修好）。

### 4.4 「契约与实现漂移」——`RGOC_GC_STRESS` 是布尔不是间隔

`C12` §5.1 一直写「每 n 次」，代码存的是 `parse_u64(...) != 0`。症状**看不出来**：
判据只断言 `collections > 0`，两种口径都满足。抓到它靠一个反常读数 ——
`STRESS=1000` + 30 万分配跑出 `collections=300000`。

### 4.5 「判据恒真」——`t_chan_08` 第一版

`select` 的 `WaitSet` 分配在 `gc_pause` 里，而 `gc_pause != 0` 时**连 stress
钩子都跳过** ⇒ 只做 select 的程序一次 GC 都不跑 ⇒ `live=0 <= 1MiB` 恒真
（而 heap 涨到 170 MB）。修法：末尾 churn 一轮把 GC 顶出来 + 三条断言。

### 4.6 「变异靶子打错」——`M-M11-18` 第一版

打在 `t_stk_06` 上**是绿的**：小帧越界量最多一帧（144 B），一页余量兜住了。
换成 12000 字节帧（越界 6752 B）才露出来。**「绿」的正确用途就是暴露这个。**

---

## 5. 判据的「牙」是怎么长出来的（16 条变异）

见 `scripts/mutation-m11-notes.md`。三条要点：

1. **绿过两次**（`M-M11-18` 打完靶子才红；`t_chan_08` 是判据本身恒真）。
2. **两条没落地**（`M-M11-05` 要改 IR 语义、`M-M11-10` 变异后是挂死），
   正面覆盖分别在 `t_go_06` / `t_sel_04`。
3. **「变异全红」不证明判据齐全**：M11 的两次「没牙」都是**主动跑红**抓到的
   （`t_groot_01` / `t_stk_06` / `t_chan_08`），不是变异抓到的。

---

## 6. 不承诺（`M11-manifest.json` 的 `unsupported` 十条）

`U-M11-01` 多线程/GMP · `02` 抢占 · `03` 定时器 · `04` 栈增长 ·
`05` 步数预算 · `06` `-race` · `07` select 均匀随机 · `08` 并发 GC ·
**`09` `case v, ok := <-c`**（实测 `SSA-UNSUPPORTED-CHAN`）·
**`10` `go` 的聚合/浮点实参**（`CG-UNSUPPORTED-GO-ARGS`）。

> `09/10` 是 M11c 收口时才编号的：它们不是「故意不做」，而是「支持面窄了一截」。
> 窄化也要号码 —— 否则下一个人会当 bug 报。

---

## 7. 与 M5–M10 的边界

* **`C9`**：三条 chan panic 的文本**不带** `panic: ` 前缀（`rt_print_panic` 自己加），
  也不带 `runtime error: `（那三条是**可 recover** 的）。
* **`C12`**：M11 只增了 §4.1 的第 9/10 类根与 §4.1.1；块形状 / 位图 / 清扫
  一个字节没动。`t_gc_09`（长跑读数）等 M10 判据原样通过。
* **`t_gc_10`（没有寄存器根）**：这是 `D-M11-3` 选「保存区压栈」的唯一理由，
  它**不许为 M11 改**，也确实没改。

---

## 8. 接手指南

```bash
# 1) 全量：四条门禁（容器内）
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && \
  cargo fmt --all --check && cargo check --workspace --all-targets && \
  cargo clippy --workspace --all-targets -- -D warnings && \
  cargo test --workspace --no-fail-fast'

# 2) 两套语料 × 三态（容器内，注意 cwd 是 /work 而不是 /work/rgoc）
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work && \
  for m in off on stress; do bash tools/run-oracle/run-native.sh --gc=$m; done; \
  for m in off on stress; do bash tools/run-oracle/run-native-chan.sh --gc=$m; done'

# 3) 变异（约 3 分钟）
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work && bash scripts/mutation-m11.sh --strict'

# 4) 只想看某个判据的实测数字
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && \
  cargo test -p rgoc-driver --test go_stack t_stk_06 -- --nocapture'
```

**改 native 运行时的三条铁律**：

1. **`is_safepoint` 是穷尽匹配**（`rgoc-ssa/src/live.rs`）：新增会分配/阻塞的
   `Instr` 变体必须回答「是不是位点」，否则编译失败 —— 那是故意的。
2. **改了 `gdesc` 的字段就检查 `mark_all_g_stacks`**：`frames` / `wait` /
   `argbytes` 三个字段都是根，漏一个是「随机时刻丢对象」。
3. **`gc_pause` 期间不要指望 GC 会跑**：自动触发与 stress 钩子**都**跳过它。
   「只在 pause 里分配」的程序（比如纯 select 循环）跑一万年也不收一次。

---

## 9. 遗留

见 `M11-manifest.json` 的 `open_gaps`（5 条）与 `deferred_gaps`（1 条）。最值得
下一阶段先看的两条：

* **并发语料的 GC 覆盖率低**（`G-M11-01`）：7 个 `N'` 里只有 `sieve1` 在 stress
  下有真实回收。要给每个 `N'` 补一条「churn 版」变体，否则「GC 在多 G 下正确」
  这件事只有 3 条判据在守（`t_groot_01/02/06`）。
* **`go` 实参区的根是保守的**（`G-M11-02`）：精确化要把「哪些实参是指针」
  从 SSA 传到运行期；`RuntimeFn::Go` 加一张位图即可，但 M11 没做。
