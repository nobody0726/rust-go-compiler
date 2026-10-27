# M11 设计 · native 并发（栈 / 调度 / channel / select）

> 输入：`03-roadmap.md` §M11（M11a–f）、`C11` §2–§7（M8 冻结的并发语义 + M9 冻结的
> `hchan` 布局）、`C5` §7（帧）、`C12` §4（根协议）、`C9` §5（退出码与合并流）。
> 输出：`M11-tests.md`（判据）、`M11-plan.md`（Task 1–46）。
> 范围：**只做 M11a / M11b / M11c**。M11d（定时器）/ M11e（多线程）/ M11f（并发 GC）
> 是可选阶段，本里程碑**不做**，理由见 §1.3。

---

## 0. 本文档在流程中的位置

```text
03-roadmap.md §M11  ──┐
C11 §2–§7（语义）     ├──► 本文档（M11-design.md）──► M11-tests.md ──► M11-plan.md
C5 §7 / C12 §4（帧与根）┘        决策 D-M11-*           判据 GO-*          Task 1–46
```

**和 M10 的关系**：M10 交的是「**单栈**精确 STW GC」。M11 一上来就有多栈 ——
但 roadmap 的 **GC 启用门禁** 明确要求 M11a/b **强制关掉 GC**，只有 M11c 验证完
「所有 G 的栈都能被扫」才重新打开。这不是保守，是判据问题：M10 的
`g_frames` 是**单链**，多 G 时它只描述当前 G，扫不到挂起的 G ⇒ 活对象被回收。

---

## 1. 目标与非目标

### 1.1 输入与输出

| 项 | 内容 |
|---|---|
| 输入 | `.go` 源码（含 `go` / `chan` / `select` / `range chan`）；`C11` 的语义；`C11` §7 的 `hchan`/`Sudog` 布局 |
| 输出 | `aarch64` ELF，**行为与 `rgoc run`（解释器）逐字节一致**；7 个并发语料与官方 `go run` 也一致 |
| 判据面 | 门禁 `GO-1..GO-7`（§1.4） |
| 不做 | 定时器 / 多线程 / 并发 GC / 栈增长（§1.3）|

### 1.2 范围（`03-roadmap.md` §M11 逐句展开）

| roadmap 原句 | 落到本设计 |
|---|---|
| **M11a** 栈上下文切换、寄存器/SP/PC/栈对齐 fixture；固定大小 goroutine 栈，溢出明确报错 | §3.2 栈布局 + §3.3 `rt_switch` + §3.4 序言检查（`D-M11-2/3/4`）；判据 `GO-1` |
| **M11b** M:1 协作调度、`go` 参数求值/复制、channel/select 等待与取消、`main` 结束；把 M8 的语义用例转入 native 门禁 | §3.5 调度器 + §3.6 `go` 的参数打包 + §3.7 `hchan` 四操作 + §3.8 select 三阶段 + §3.9 死锁（`D-M11-5..9`）；判据 `GO-2..GO-5`、`GO-7` |
| **M11c** 停所有任务的 STW、调度栈/root 管理；栈增长先选可证明的方案 | §3.10 多 G 根扫描（`D-M11-10/11`）；判据 `GO-6`。**栈增长不做**（`U-M11-04`）|
| **GC 启用门禁**：a/b 强制禁用整程序 GC，退回有预算上限的 arena；只有 c 验证完才重开 | §3.11（`D-M11-12`）；判据 `GO-6` 的反向半边（a/b 阶段 `mode=off` 必须被强制）|
| **门禁**：每子阶段明确模型/范围/压力预算；不把 M:1 当多线程正确 | §1.3 明确「M:1 就是全部」；§4.4 预算表 |
| **M11d/e/f（可选）| **不做**，见 §1.3 |

### 1.3 明确不做（M11 边界）

| 不做 | 为什么 |
|---|---|
| **定时器 / `time` 包**（M11d）| 需要 stdlib 面（`import "time"`），而 stdlib 面归 M12 的 import 扩展。**没有墙钟**是 `C11` §6 第 5 条已冻结的不承诺 —— M11 不改它 |
| **多线程 / GMP / work-stealing**（M11e）| 判据面会立刻崩：`C11` §2.1 第 1 条（M:1）是「同 seed 同结果」的前提，也是「native 与解释器逐字节一致」的前提。真并发下这个判据**不存在**，只能换成「性质判据」（不死锁、不丢值），那是另一个里程碑的量级 |
| **并发标记 / 写屏障**（M11f）| STW + 单线程 ⇒ 不需要写屏障（`C12` `U-M10-04`）。M11 的 STW 仍然是「在分配点同步跑」，只是要扫**所有** G 的栈 |
| **栈增长 / 栈复制** | 复制式栈要重定位 stack map、内部栈指针、`defer`/闭包捕获 —— 而这些正是 M10 刚冻结的东西。M11 用**固定大小 + 明确报错**（`U-M11-04`）|
| **抢占** | `C11` §2.1 第 2 条冻结「只在阻塞点让出」。native 照抄：`for {}` 在 G 里会真的挂死 ⇒ 靠**装置侧墙钟超时**（§4.4），不做步数预算（`U-M11-05`）|
| **`-race`** | `C11` §6 第 4 条：单线程没有数据竞争这回事 |

### 1.4 退出条件（门禁七条）

| ID | 名称 | 一句话判据 |
|---|---|---|
| `GO-1` | 栈与切换 | 切 1000 次之后 callee-saved（x19–x28、d8–d15）与 `sp` 全保住；栈溢出报确定文本 + 退出 2；**溢出发生在第 N 层且 N 可复现**（`T-STK-06`）|
| `GO-2` | 调度三规则 | M:1（只有 1 个 OS 线程）/ 不抢占（`go` 不切走当前 G）/ 就绪队列 FIFO —— 三条各有 fixture |
| `GO-3` | `hchan` 四操作 | `C11` §3.2 的四张表逐格差分（与解释器同一套输入序列）；`closed` 检查排第一 |
| `GO-4` | `select` | 三阶段（求值/试探/阻塞）+ token 撤销 + 重复 channel 只有一个 winner |
| `GO-5` | 死锁与退出 | `fatal error: all goroutines are asleep - deadlock!` + 退出 2 逐字节；`main` 返回即退出（不跑其它 G 的 defer）|
| `GO-6` | 多 G 根扫描（M11c）| 挂起 G 的栈上对象不被回收；a/b 阶段 `mode=off` 被**强制** |
| `GO-7` | 语料 | 7 个 `N'` 三层一致（native / 解释器 / 官方）；12 个 `M'` 仍拒绝且**不 panic**（现在 3 个是 rc=101）|

---

## 2. 决策记录

### 2.1 决策一览

| # | 决策 | 一句话理由 |
|---|---|---|
| `D-M11-1` | **模型 = M:1 协作式，不抢占**（照抄 `C11` §2.1 三条）| 判据是「与解释器逐字节一致」；真并发下这条判据不存在 |
| `D-M11-2` | **每个 G 一块固定栈**（默认 64 KiB，**已拍板**，溢出深度实测钉住），`mmap` 拿、`munmap` 还 | 固定大小 ⇒ 溢出判据可写；不做复制增长（§1.3）。「够 1000 层」的估算已作废，见 §2.4 ① |
| `D-M11-3` | **切换时 callee-saved 压在挂起 G 自己的栈上**，`sp` 存进 `gdesc` | 保住 M10 的「没有寄存器根」不变式（`t_gc_10` 不用改）|
| `D-M11-4` | **栈溢出用序言检查**，不用 guard page + 信号 | guard page 要装 `rt_sigaction`（freestanding 下脆弱）且「明确报错」不可测；序言 4 条指令可测 |
| `D-M11-5` | 调度器**在 `rt.c`**（不在 Rust）| 切换栈必须汇编 + 必须在运行时；`isel` 只发 `bl` |
| `D-M11-6` | **`go f(args…)` 的实参在发起方求值并深拷贝**到新 G 的栈 | 官方语义（`STM-05`）；不拷的话 `go f(x)` 会看到之后改的 `x` |
| `D-M11-7` | `hchan` = `C11` §7 的 96 B **逐字段**；`lock` 留位不用 | M:1 不需要锁；但布局冻结，M11e 才用 |
| `D-M11-8` | `buf` 与 `Sudog` **进托管堆**，各带 desc | `buf` 的元素要能被 GC 扫（`elemtype` 已在 `hchan` 里）；`Sudog` 会被丢弃，arena 会泄漏 |
| `D-M11-9` | `select` 的 `token` = 一个托管堆上的 `WaitSet` 块 | `C11` §4.2 的撤销需要「一次登记 N 个队列、只让一个 winner」|
| `D-M11-10` | M11c 起 `g_frames` 从**全局**变成 **`g_cur->frames`** | 多 G 时全局只能描述当前 G；挂起 G 的链必须跟着它自己走 |
| `D-M11-11` | 扫根时遍历**所有** `gdesc`（含 blocked），从 `gdesc->frames` 起 | 挂起 G 的栈上还有活值（等待中的 `sudog->elem` 指向它的栈）|
| `D-M11-12` | **M11a/b 强制关 GC**：编译期发现 `go`/`select`/`send`/`recv` 就发 `rt_gc_force_off()` | roadmap 的 GC 启用门禁；判据是统计行 `mode=off` |

### 2.2 只对**非显然**的六条展开

#### `D-M11-3` 为什么 callee-saved 要压在自己的栈上

「切换」有两种实现：

| 方案 | 存哪 | 后果 |
|---|---|---|
| A | 存进 `gdesc.regs[]` | GC 必须扫 `gdesc` 的寄存器槽 ⇒ **`t_gc_10`（没有寄存器根）直接失效**，`C12` §4 的整条根协议要加一类根 |
| B | 压到被挂起 G 的**栈**上（`sp` 往下移 160 字节）| 值仍然全在栈里 ⇒ M10 的根协议**一个字都不用改** |

选 B。代价是每次切换 160 字节的 `stp`/`ldp`（20 个 8 字节槽：`x19..x28`、
`x29`、`x30`、`d8..d15`），以及「栈溢出判据要把这 160 字节算进去」。

> 这条的判据很硬：`t_gc_10` 扫 `.s` 找 callee-saved 的保存 —— 方案 A 会让它在
> 切换函数上红。而 `t_gc_10` 是 M10 留下的守卫，**不允许为了 M11 改判据**。

#### `D-M11-4` 为什么不用 guard page

「栈溢出明确报错」有三条路：

1. **guard page + SIGSEGV handler**：`mmap` 栈块时把最低一页设 `PROT_NONE`，
   溢出时内核发 SIGSEGV，handler 里打一行再 `exit(2)`。问题：freestanding 下要
   自己写 `rt_sigaction`（syscall 134）+ 信号栈 + 保存/恢复，而且**信号帧的布局
   是内核 ABI**，与「首发 Linux/arm64」绑死；更麻烦的是「明确报错」的**文本与
   退出码**要靠 handler 保证，测起来是「大概率」而不是「确定」。
2. **序言检查**：`sub sp` 之后 `cmp sp, g_stack_lo; b.lo panic`。4 条指令
   （`adrp` + `ldr` + `cmp` + `b.lo`），**确定**、可测、与 `C9` 的文本契约同源。
3. 只在「大帧函数」检查：不成立（小帧累积溢出会绕过所有大帧）。

选 2。代价是每个函数序言多 4 条指令 —— 与 M9 的 `check_panic`（每个调用点 2 条）
同量级，且**可以用 `.s` 的指令数钉住**（`GO-1` 的第三条判据）。

> ⚠️ 不用寄存器存 `g_stack_lo`：那会让 `t_gc_10` 红（见 `D-M11-3` 的表）。
> 用全局，多 2 条指令，换「M10 的守卫原样保留」。

#### `D-M11-6` `go` 的参数为什么要**深**拷贝

```go
x := [2]int{1, 2}
go f(x)          // f 看到的是 {1,2}
x[0] = 9         // 不影响 f
```

「求值」在发起方做（`go f(g())` 里 `g()` 在**当前 G** 跑），然后**按字节拷**到新
G 的栈。`C11` §3.3 的「双次深拷贝」是 channel 的规则，这里是第三条：`go` 的实参
也拷一次。

不拷的后果不是「慢」，是 `x[0] = 9` 会改到 `f` 看到的东西 —— 而解释器是拷的
（`C11` 冻结语义），于是 **native 与解释器不一致**，`GO-7` 立刻红。

#### `D-M11-8` 为什么 `Sudog` 不能进 arena

`Sudog` 的生命周期是「登记 → 被唤醒 → 摘链 → 丢弃」。`select` 一次登记 N 个、
只留一个 winner ⇒ 另外 N−1 个**立刻**变垃圾。arena 不回收 ⇒ 一个 `for { select {} }`
会把 arena 吃光。

所以 `Sudog` 进托管堆，desc 手写：

```text
Sudog = [+0 g][+8 elem][+16 next][+24 prev][+32 token][+40 done:u32][+44 pad]
        ptrdata = 40，gcdata = bit0..bit4（五个指针字）
```

`elem` 指向 `buf` 里的槽或**发送方 G 的栈**。后者是**栈地址** ⇒ `mark_ptr` 忽略
（`chunk_of` 返回 0）—— 这是对的：那个值由**发送方的栈**保活，不由 `elem` 保活。

#### `D-M11-10/11` 多 G 根扫描的最小改动

M10 的根协议是：

```text
g_frames ──► [帧] ──► [帧] ──► 0
```

多 G 时它必须变成：

```text
g_cur ──► gdesc ──► frames ──► [帧] ──► … ──► 0        （当前 G）
   │
   └──► runq / blocked 链上的每个 gdesc ──► frames ──► …  （挂起的 G）
```

两种改法：

| 改法 | 改动面 | 风险 |
|---|---|---|
| 保留全局 `g_frames`，切换时 `g_cur->frames = g_frames` / `g_frames = next->frames` | `isel` 的序言/尾声**不动**，只改 `rt_switch` | 切换路径上多 2 条 store/load；但**序言里的 3 条 store 一个字节不改** ⇒ M10 的栈图判据全部照旧 |
| 去掉 `g_frames`，序言写 `g_cur->frames` | 序言/尾声各改一次（多一次 load）| 所有 `.s` 里的 `g_frames` 变成 `g_cur->frames`，`t_root_*` 的 fixture 全要重看 |

选**第一种**：`g_frames` 的语义从「全局链头」变成「**当前 G** 的链头」，切换时
搬进/搬出 `gdesc`。这样 M10 的判据面（`t_root_09/10/11/13/14`、`stackmap.rs`）
**一条都不用改** —— 而它们正是 M10 最贵的东西。

#### `D-M11-12` GC 门禁怎么「强制」

不是「文档里说别开」，而是**可观测**的强制：

1. `module.rs` 在生成 `.s` 时，若发现本模块有 `Go`/`Select`/`Send`/`Recv`（或
   `range chan`），在 `_start` 里插一条 `bl rt_gc_force_off`；
2. `rt_gc_force_off()` 把 `gc_on = 0` 并置一个 `gc_forced_off` 标记；
3. 统计行的 `mode=` 因此打 **`off`** —— 即使调用者设了 `RGOC_GC_STRESS=1`。

判据：`GO-6` 的第二条断言 —— 并发语料 + `RGOC_GC_STRESS=1` ⇒ `mode=off` 且
`collections=0`。M11c 收口时把这条**翻过来**（`mode=stress` 且 `collections>0`），
这就是「GC 重新启用」的可观测开关。

### 2.3 事实基线（`F1..F12`，全部实测或源码指认）

| # | 事实 | 证据 |
|---|---|---|
| `F1` | 解释器已完整支持 `go`/`chan`/`select`，7 个 `N'` 与官方逐字节一致 | `run-chan.sh` ⇒ 7/12/0；`C11` §2–§6 |
| `F2` | **native 完全没有**：`build` 对 chan 类型直接拒 | `SSA-UNSUPPORTED-CHAN`（`build.rs::unsup_stmt`）|
| `F3` | ⚠️ **`build` 对 `go`/`select`/`send` 是 panic（rc=101），不是拒绝** | 实测：`test/chan/select`、`sieve1`、`sendstmt` ⇒ `panicked at effect.rs:211`。原因：`effect.rs::scan_stmt` 对 `Stmt::Go/Send/Select` 是 `unreachable!()`，而**效果摘要在 `unsupported()` 之前算** |
| `F4` | `zerosize.go` 走的是「chan 类型出现在表达式里」那条，**干净拒绝**（rc=1） | 实测；它没有 `go`/`select` 语句 |
| `F5` | `hchan` 96 B / `Sudog` 48 B 布局**已冻结**，`T-SCH-01` 逐字段对账 | `C11` §7 + `chan_contract.rs` 6 条 |
| `F6` | `Hchan` 只是 `#[repr(C)]` 影子，**一个字节可执行代码都没有** | `rgoc-arm64/src/hchan.rs`；`t_sch_04` 钉住「代码里没有 `rt_go`/`rt_select`」|
| `F7` | 帧是**定长**的（`C5` §7），超大帧走 `mmap`（`R-9-22`）| `frame.rs::off_stack` |
| `F8` | 根协议是**单链** `g_frames`；`mark_frame` 从链头走到底 | `C12` §4.2；`rt.c::mark_frames` |
| `F9` | M10 的「没有寄存器根」守卫 `t_gc_10` 扫 `.s` 找 `x19..x28` 的保存 | `gc_precise.rs` |
| `F10` | 死锁文本与退出码**已在 `C9` 冻结** | `C9` §5 表：`fatal error: all goroutines are asleep - deadlock!` + 退出 2，**不进合并流** |
| `F11` | `main` 返回即退出（`C11` §5.1），不跑其它 G 的 defer | `C9` §5；`T-CON-36` |
| `F12` | 解释器有 `--sched=seed:N`；native 侧没有（`build` 没有这个开关）| `rgoc-driver run --sched=` |

### 2.4 已拍板的三件事（2026-10-27）

> 本节三件事在设计初稿里是「待拍板」。三件已于 2026-10-27 由项目方拍板，
> 结论**全部采纳设计建议**，但 ① 的执行口径有一处修正（见下）。

**① goroutine 栈多大？→ 采纳 64 KiB，但「够 1000 层」的估算作废**

| 选项 | 代价 | 风险 |
|---|---|---|
| **a. 64 KiB**（**已采纳**）| 1000 个 G = 64 MiB 虚拟内存（实际只碰几页）| 深递归提前溢出 |
| b. 8 KiB | 1000 个 G = 8 MiB | 语料 `sieve1.go` 那种递归筛法可能不够 |
| c. 256 KiB | 更安全 | `for i:=0;i<10000;i++ { go f() }` 直接 2.4 GiB 虚拟内存 |

⚠️ **初稿的推理是错的，已修正**：初稿写「`C9` 的帧深上限是 1000，而每帧最多几百
字节 ⇒ 64 KiB 够 1000 层以上」。算一下就知道不成立 —— 1000 层 × 200 B = 200 KB，
是 64 KiB 的三倍。而 `C9` 的 1000 是**解释器**的帧深限制，native 侧根本没有帧深
检查，只有栈大小这一个约束。

所以 64 KiB 是**实测钉住的数字，不是估算出来的**：

- 判据 `T-STK-06`（新增）：一条递归 fixture，断言**实测溢出发生在第 N 层**，
  `N` 写进判据且必须能复现；同时断言「语料里递归最深的那条（`sieve1.go`）
  在 64 KiB 下**不**溢出」。
- 两层数字都进 `M11-manifest.json` 的 `measured`：`stack_bytes = 65536`、
  `overflow_at_depth = N`、`deepest_corpus_depth = M`，且 `M < N` 有富余。

**② 接受「a/b 阶段没有 GC」吗？→ 接受**

roadmap 写死了这条门禁。后果：M11a/b 收口时，并发语料的判据是**单态**
（`mode=off`），而不是 M10 的三态。三态要到 M11c 才有。

替代方案是「先把多 G 根扫描做了再开调度」—— 但那要求 `gdesc` 与 `rt_switch` 先存在，
顺序上做不到。

a/b 阶段的门禁是 `GO-1..GO-5` + `GO-7`（不含 `GO-6`），`GO-6` 只在 c 阶段生效；
且 a/b 收口时**必须有**一条判据钉住「`mode` 真的是 off」（`T-GROOT-04`），
否则「关 GC」会变成一句空话。

**③ `C11` §7 的措辞要不要改？→ 改**

`C11` §7 开头写的是「native 运行时（M11）…… 那里是**多核抢占**、真锁、真 park/unpark」。
而 M11a–c 是 **M:1 协作式**，`lock` 字段根本不用。这是**契约与实现的冲突**。

改为「M11a–c 是 M:1 协作式（与 §2 同构），`lock` 留位给 M11e；字段序仍然按官方
冻结」。理由：把「多核」写进 §7 会让后来的人以为 `lock` 现在就该用，而 `hchan`
的**布局**与**执行策略**是两件事（§7 自己也这么说）。落地在 `M11-plan.md` Task 2。

---

### 2.5 开工前的前置：`G-M10-06` 必须先查清（2026-10-27 拍板）

> ✅ **2026-10-27 已查清，门禁解除。** 完整记录见 `scripts/g-m10-06-probe.md`。
>
> **结论一句话**：根因**不在位图**，在 `rt_defer_push`。它用 `rt_arena_alloc`
> 分配 `DeferRec`，而那条路径走 `gc_alloc`，后者的 stress 钩子在**分配之前**
> 跑 GC —— 此刻新记录还没挂上 `g_defer_top`（`mark_defer` 看不到闭包），
> 而 `Defer` 不是 safepoint（`sp_idx` 还停在 `MakeClosure` 的位点）。
> 于是闭包块的**唯一**根就是那张位图：超集恰好含 `MakeClosure` 的 dst，
> 所以 M10 一直没露 —— 那是**巧合**。真 live-in 下闭包被回收 ⇒ 堆地址被
> 下一个闭包复用 ⇒ `rt_deferrun` 调到**别人的**闭包 ⇒ panic 且 `recover()`
> 救不回来。
>
> **修法**：`rt_defer_push` 加 `gc_pause`（`C12` §6.1 / `R-10-24`）。修完之后
> 真 live-in 也能跑绿；但位图**仍保留超集**（策略选择，不是补丁）。
> 守卫判据 `t_gc_12`（断言 `gc_pause++` < 分配 < 发布 < `gc_pause--`）。
>
> **对 M11 的意义**：`GO-6` 的多 G 扫根现在有一条**已知的、已修的**前提，
> 而不是一个未解释的例外。`T-GROOT-06` 从「未查清的未知」变成**回归面**。

**决定**：`G-M10-06` **阻塞 M11 开工**。这一条排在所有实现任务之前
（`M11-plan.md` 的 **Task 0**），且**不允许**用「先写代码、后面再查」的方式绕过。

**为什么它可以阻塞、而且值得阻塞**：M11 的全部判据都是「native 与解释器逐字节
一致」。而 `G-M10-06` 的病灶就在**位图语义**上 —— 位图是 native 侧独有的东西，
解释器没有。位图错了，`GO-1..GO-7` 的绿都不作数（M10 的六次「变异绿」里
有两次就是这个形态）。M11 一上来要把「1 条帧链」变成「N 条帧链」，
如果位图语义本身还带着一个未解释的例外，**多 G 之后这个例外会被放大到无法定位**。

**现状（`live.rs` 第 330–339 行的原注释）**：

```rust
let mut set = after[ii].clone();      // after[ii] = live-OUT
set.extend(ins.uses());
// 试过把 def 去掉（真 live-in 语义），实测会让
// defernil / print / nilptr2 / method5 全部坏掉。
// 机制没查清，所以这里保留超集。
```

「去掉 def」= `(after[ii] − ins.dsts()) ∪ ins.uses()`，即真正的 live-in。

**设计初稿漏了这一条**（本节是新加的）：初稿的 `F1..F12` 事实基线里没有 `G-M10-06`，
§8 风险登记里也没有它。这是**设计缺口**，不是实现缺口。

**三条候选机制的实际判定**（Task 0 的产出，证据见 `scripts/g-m10-06-probe.md` §3）：

| # | 假设 | 判定 | 实测依据 |
|---|---|---|---|
| A | `sp_idx` 在调用期间不变，而**某个分配点**在 `dst` 还没发布时触发 GC ⇒ 新对象没人保活 | ✅ **证实**（形态修正）| trace 里 `push-fv` 出现在 `gc-begin:3` **之前**，而 3 号 GC **没有** `mark-fv` ⇒ GC 在 `rt_defer_push` **内部**跑，此时记录未挂链。`rt_arena_alloc` 的实现是 `return gc_alloc(size, K_ARENA, 0, size)` |
| B | 位图与 `sp_idx` 的配对有偏差 | ❌ 证伪 | `.stackmap` 差集只有闭包 `dst` 一处；若 B 成立会看到**别的**位点漂移 |
| C | **槽复用**：`dst` 槽的旧值是活对象的唯一引用 | ❌ 证伪（「地址复用」是真的，但那是**回收的后果**）| `main` 的 `fp-88` 在 `MakeClosure` 之前是零页（`init` 只用到 `-64`）。被复用的是**堆地址**：闭包 A 回收后 f 的闭包拿到同一块 `ffff80000070` |

⚠️ **A 的措辞要修正**：不是「被调方内部再触发一次 GC」，而是
**「登记动作自己的分配触发了 GC，而登记尚未完成」** —— 同一个 `gc_pause` 缺口，
但位置在**调用方**，不在被调方。这个区别很重要：按原措辞去查「被调方内部」，
会一直找不到。

假设 A 的解释力最强（能解释「`MakeClosure` 的 dst 尤其明显」这句原注释），
但**没有实测就不算结论** —— Task 0 的产出是证据，不是推理。

**Task 0 的验收（写完才能开 Task 1）**：

1. 三条假设各自被**证实或证伪**，附实测输出（不是「看起来像」）；
2. `G-M10-06` 从 `M10-manifest.json` 的 `deferred_gaps` 移出 —— 要么进
   `closed_gaps`（查清了，附根因），要么**改写成一条明确的 `C12` 不变式**
   （如果结论是「必须保留超集，理由 X」）；
3. 无论结论如何，`live.rs` 那段注释里的「机制没查清」**必须删掉**，
   换成查清后的机制说明。一句「没查清」留在源码里，下一个改 GC 的人会再踩一次。

---

## 3. 架构与数据模型

### 3.1 crate 与模块变化

| crate | 变化 |
|---|---|
| `rgoc-ssa` | **新增** `Instr::Go`/`Send`/`Recv`/`Select`（或复用 `Call` + `CalleeRef::Runtime`，见 §3.5）；`unsup_stmt` 去掉三条拒绝；`effect.rs` 的 `unreachable!()` 换成真实扫描 |
| `rgoc-arm64` | `hchan.rs` 从影子变成**真实现**（`ChanState` 补 `select`）；`isel` 加 `Go`/`Send`/`Recv`/`Select` 的下降；`module` 发 `gdesc`/`rt_gc_force_off` 标记 |
| `rgoc-driver` | `build` 的拒绝面收窄；新增 `run-native-chan.sh` 装置 |
| `rt.c` | 调度器 / `rt_switch` / `hchan` 四操作 / `select` / 死锁 / 栈分配 / 多 G 扫根 |

### 3.2 G 描述符与栈布局

```c
typedef struct GDesc {
    u64 *sp;            /* +0  挂起时的栈顶（切换点） */
    u64 *stack_lo;      /* +8  栈低地址（含 guard 区） */
    u64 *stack_hi;      /* +16 栈高地址 */
    void *frames;       /* +24 本 G 的 g_frames 链头（M11c 起用） */
    void *wait;         /* +32 select 的 WaitSet（阻塞时） */
    struct GDesc *qnext;/* +40 就绪队列后继 */
    u32 state;          /* +48 0=ready 1=running 2=blocked 3=exited */
    u32 id;             /* +52 单调递增（调试与确定性） */
    u64 pad;            /* +56 */
} GDesc;                /* 64 字节 */
```

栈（`mmap`，`stack_size + 16` 对齐）：

```text
stack_hi  ┌────────────────┐  ← 初始 sp（G 启动时）
          │ 启动帧          │     {fn, argbytes, args…}
          ├────────────────┤
          │  …帧（向下长）  │
          ├────────────────┤  ← sp（运行时）
          │ 切换保存区 160B │     只在挂起时存在
stack_lo  └────────────────┘
          ↑ 序言检查：sp >= stack_lo + 160 + 64（余量）
```

### 3.3 上下文切换（`rt_switch`）

```asm
// void rt_switch(u64 **save_sp, u64 *new_sp)
rt_switch:
    sub  sp, sp, #160
    stp  x19, x20, [sp, #0]
    stp  x21, x22, [sp, #16]
    stp  x23, x24, [sp, #32]
    stp  x25, x26, [sp, #48]
    stp  x27, x28, [sp, #64]
    stp  x29, x30, [sp, #80]
    stp  d8,  d9,  [sp, #96]
    stp  d10, d11, [sp, #112]
    stp  d12, d13, [sp, #128]
    stp  d14, d15, [sp, #144]
    mov  x9, sp
    str  x9, [x0]          // *save_sp = sp
    mov  sp, x1
    ldp  x19, x20, [sp, #0]
    …（对称）
    add  sp, sp, #160
    ret
```

⚠️ **`d8..d15` 必须保存**：浮点/复数的值会跨调用留在这些寄存器里（`C5` §6.1），
而「每个值一个栈槽」只保证**整数**值落槽 —— `isel` 把 `f64` 从槽 `ldr` 到 `d0`
用完即弃，但被调用方**返回后**调用方可能还指望 `d8` 里的东西？不，我们不用 `d8`
做值寄存器。真正的原因是 **AAPCS64 规定 d8–d15 是 callee-saved**，而 `rt_switch`
是被「调用」的（`bl`）—— 不保存就违反了 ABI，G 之间会串味。

判据 `GO-1` 第一条：切换 fixture 里 `d8..d15` 各放一个**可区分的**双精度值，
切 1000 次后逐个比对。

### 3.4 栈溢出检查（序言）

```text
sub  sp, sp, #frame_size
adrp x9, g_stack_lo
add  x9, x9, :lo12:g_stack_lo
ldr  x9, [x9]
cmp  sp, x9
b.lo .Lstack_overflow_<fn>
```

`.Lstack_overflow_<fn>` 在函数尾（冷路径）：`bl rt_panic_stack_overflow`。

⚠️ **只在 `frame_size > 0` 时发**（空帧不可能溢出）；且**超大帧（mmap 帧）不发**
（`off_stack` 的帧底不在 `sp` 上，判据无意义 —— 它们由 `rt_stack_frame_alloc` 的
OOM 路径管）。

### 3.5 IR 面：四条新指令

| 指令 | 语义 | 为什么不能复用 `Call` |
|---|---|---|
| `Instr::Go { func, args, nargs, argbytes }` | 求值实参 → 打包 → 入就绪队列 | `Call` 有返回值、要等结果；`go` **不等**，而且实参要**拷**到新栈 |
| `Instr::Recv { dst, ch, ok }` | `<-ch` / `v, ok := <-ch` | 可能**阻塞**（换 G），`Call` 不能 |
| `Instr::Send { ch, val }` | `ch <- v` | 同上；且要**深拷贝** `val` |
| `Instr::Select { cases, token, dsts }` | 三阶段 + 撤销 | 一个指令要表达 N 个 case 的求值/试探/阻塞 |

四条都是 **safepoint**（`live::is_safepoint` 的穷尽匹配会强制我回答这个问题）：
`Recv`/`Send`/`Select` 可能分配（`Sudog`、`WaitSet`），`Go` 必然分配（栈 + `gdesc`）。

### 3.6 `go` 的参数打包

```text
当前 G 的栈上：
  [argbytes 的实参区]     ← isel 已经算好（与普通调用的实参布局同一套）
  ↓ rt_go(fn, &实参区, argbytes)
rt.c：
  1. mmap 新栈（64 KiB）
  2. 在新栈顶构造「启动帧」：{fn, argbytes, args…}
  3. gdesc->sp = 指向 rt_go_trampoline 的入口状态
  4. 入就绪队列**队尾**（不切走当前 G）
```

`rt_go_trampoline`：从启动帧取 `fn`/实参 → `blr fn` → 返回后 `rt_goexit()`。

### 3.7 `hchan` 四操作（照 `C11` §3.2，M:1 化）

状态机**逐格照抄** `C11` §3.2 的四张表。M:1 化只改「阻塞」的实现：

| `C11` 的动作 | M:1 的落地 |
|---|---|
| 登记 `Waiter` | 构造 `Sudog`（托管堆），挂到 `sendq`/`recvq` 尾部 |
| 阻塞 | `g_cur->state = blocked`；`rt_schedule()` 换下一个 G |
| 唤醒 | `sudog->done = 1`；`g->state = ready`；**入队尾** |

`buf` 是托管堆块（`K_ARRAY`，`elem = hchan->elemtype`，`n = dataqsiz`）；
`dataqsiz == 0` 时 `buf = 0`。

### 3.8 `select` 三阶段（照 `C11` §4）

```text
① 求值：所有 case 的 chan 表达式 + 所有 send 的右值，按源码顺序各求值一次
② 试探：源码顺序扫描；nil 跳过；default 记「有」；就绪的记「可做」
③ 阻塞：无「可做」且无 default ⇒ 给每个非 nil case 登记 sudog（共用 token）⇒ 换 G
```

`token` = `WaitSet` 块（托管堆）：

```text
WaitSet = [+0 done:u32][+4 winner:i32][+8 n:u64][+16 entries[n]:{chan, sudog}]
```

被任一通道唤醒后：拿 token 去所有队列删自己 → 看谁的 `done` 为真 → 只跑 winner。

### 3.9 死锁与退出

| 事件 | 行为 |
|---|---|
| `runq` 空 且 有 blocked 且 `main` 未结束 | `fatal error: all goroutines are asleep - deadlock!` + 退出 2（**不进合并流**）|
| `main` 返回 | **立即** `rt_exit(0)`；不跑其它 G 的 defer |
| 任一 G 未捕获 panic | 退出 2，文本**不包装**（`C9` §5）|

### 3.10 多 G 根扫描（M11c）

```text
mark_roots:
  mark_frames(g_frames)                 # 当前 G（M10 原样）
  for g in 所有 gdesc:                  # 就绪 + 阻塞 + 已退出（栈还在的）
      mark_frames(g->frames)
  mark_defer(); mark_pin(); mark_globals()
```

⚠️ **`gdesc` 本身也要被扫**：它含 `frames`（栈地址，`mark_ptr` 忽略）、
`wait`（`WaitSet` 块，托管堆）、`qnext`（`gdesc`，不在堆）。所以 `WaitSet`
必须由 `gdesc` 保活 —— `mark_ptr(g->wait)` 就够了。`gdesc` 自己放 arena
（永不回收，量小：每个 G 64 字节，且 G 数有上限）。

### 3.11 GC 门禁的可观测开关

见 `D-M11-12`。一句话：**并发模块 ⇒ `mode=off`**（a/b），M11c 收口时翻成
`mode=on|stress`。判据在 `GO-6`。

---

## 4. 判据与装置

### 4.1 三层差分

| 层 | 比什么 | 覆盖面 |
|---|---|---|
| L1：native vs **解释器** | stdout + stderr + 退出码，逐字节 | 7 个 `N'` + 全部新 fixture（这是**主判据**）|
| L2：native vs **官方 `go run`** | 同上 | 7 个 `N'`（`F1` 已证解释器与官方一致 ⇒ L1 ∧ L2 互相印证）|
| L3：解释器 vs 官方 | 同上 | M8 已绿（7/7），作为传递性证据，M11 只**复跑**不改 |

### 4.2 差分口径

- **合并流**：stdout 与 stderr 各自独立比较，**不合并**（`C9` §5 的死锁文本只进
  stderr；合并了就分不清「谁打的」）。
- **退出码**：`0` / `2` 必须逐字节相同；`1` 只出现在静态拒绝。
- **确定性**：同一份源码跑两次必须逐字节相同（`GO-2` 第三条）。

### 4.3 探针（沿 `C12` §5.2 的统计行）

| 字段 | 并发下的用途 |
|---|---|
| `mode=` | **GC 门禁的开关**（`D-M11-12`）|
| `collections` | a/b 阶段必须 0 |
| 新增 `goroutines=`？| **不加**。统计行语法冻结（`C12` §5.2，13 字段）；G 数用 `rt_goroutines()` 单独探针 |

### 4.4 预算与资源

| 资源 | 上限 | 判据 |
|---|---|---|
| 每个 G 的栈 | 64 KiB（`D-M11-2`，**已拍板**）| 溢出 ⇒ 确定文本 + 退出 2；溢出深度实测（`T-STK-06`）|
| goroutine 数 | **1024**（硬上限）| 超了 `rt_panic_goroutines` + 退出 2 |
| 帧深 | 1000（`C9` §5，按 G 计）| 已有 |
| 墙钟 | **装置侧** `timeout`（native 没有步数预算，`U-M11-05`）| `run-native-chan.sh` 用 `timeout 20` |

### 4.5 语料

| 组 | 个数 | 用途 |
|---|---|---|
| `N'`（`chan-input.txt` 减去 `chan-rejected.txt`）| **7** | L1/L2 逐字节 |
| `M'`（`chan-rejected.txt`）| **12** | 仍拒绝且**不 panic**（`F3` 的回归判据）|
| 新增 fixture | ~15 条 | 每个门禁至少一条；`GO-1` 的切换压力、`GO-4` 的重复 channel 等 |

---

## 5. 不支持面（白名单，8 条）

| ID | 不承诺 | 证据面 |
|---|---|---|
| `U-M11-01` | 多线程 / GMP / 真并行 | `t_sch_04` 的加强版：`.s` 与 `rt.c` 里不出现 `clone`/`pthread`/`futex` |
| `U-M11-02` | 抢占 | `for {}` 在 G 里烧到装置超时（退出 124），不是「被切走」|
| `U-M11-03` | 定时器 / `time` 包 | `import "time"` 仍 `TYP-UNSUPPORTED-IMPORT-FORM` |
| `U-M11-04` | 栈增长 / 栈复制 | 溢出报错；`stackmap` 不需要重定位 |
| `U-M11-05` | 步数预算（native）| 只有解释器有 `--max-steps`；native 靠装置超时 |
| `U-M11-06` | `-race` | 单线程线性化，没有数据竞争 |
| `U-M11-07` | select 的均匀随机 | 多就绪取源码顺序（`C11` §4.3，与解释器一致）|
| `U-M11-08` | 并发 GC / 写屏障 | STW + 单线程；`C12` `U-M10-04` 不变 |

---

## 6. 错误处理

| 情形 | 文本 | 退出码 | 通道 |
|---|---|---|---|
| 栈溢出 | `runtime error: goroutine stack exceeded 65536 bytes` | 2 | stderr |
| goroutine 数超限 | `runtime error: too many goroutines (limit 1024)` | 2 | stderr |
| 死锁 | `fatal error: all goroutines are asleep - deadlock!` | 2 | stderr（`C9` 已冻结）|
| `send on closed channel` | `panic: send on closed channel` | 2 | stderr（`C9`）|
| `close of closed channel` | `panic: close of closed channel` | 2 | stderr |
| `close of nil channel` | `panic: close of nil channel` | 2 | stderr |
| 静态拒绝（chan 类型不支持等）| `SSA-UNSUPPORTED-CHAN: …` | **1** | stderr（`F3` 要修的就是这条）|

---

## 7. 覆盖清单（roadmap §M11 ↔ 功能 ID）

| 功能 ID | M11 后状态 | 依据 |
|---|---|---|
| `RT-MEM-03`（栈内存管理）| ✅ 每个 G 固定栈 + 溢出判据 | `GO-1` |
| `RT-MEM-05`（并发 GC）| ✗ 仍不做（`U-M11-08`）| — |
| `RT-CMP-05`（Channel 结构）| ✅ `hchan` 从影子变成实现 | `GO-3` |
| `RT-CMP-06`（Channel 语义全套）| ✅ 四操作 + select + 死锁 | `GO-3..GO-5` |

---

## 8. 风险登记（前三条是最可能的坑）

| # | 风险 | 征兆 | 预防 |
|---|---|---|---|
| 1 | **切换保存漏了某个寄存器** | 随机算错 / 只在浮点程序上错 | `GO-1` 用**可区分值**填满 x19–x28 与 d8–d15，切 1000 次逐个比 |
| 2 | **`go` 的实参没深拷贝** | `GO-7` 与解释器不一致，且只在「发起后改实参」的 fixture 上露 | 专门 fixture（§2.2 `D-M11-6`）|
| 3 | **死锁判定条件写漏**（比如没排除「还有就绪 G」）| 正常程序报 deadlock | `GO-5` 三条 fixture：真死锁 / 有就绪 / main 已返回 |
| 4 | 多 G 扫根漏了 blocked G 的栈 | `GO-6` 红；表现为「挂起 G 醒来读到垃圾」| `mark_frames` 遍历所有 gdesc |
| 5 | `Sudog` 泄漏（arena 化）| 长跑 `for { select {} }` 吃光堆 | `D-M11-8` + `GO-3` 的「10 万次 select 后 `live` 有界」|
| 6 | 序言检查把超大帧也查了 | `nilptr2.go` 误报溢出 | `off_stack` 的帧不发检查（§3.4）|
| 7 | **位图语义带着未解释的例外进 M11**（`G-M10-06`）| 单 G 时只是「多标」，多 G 后变成「某个 G 的活对象偶发被收」，且只在压力下露 | §2.5：**Task 0 先查清，查不清不开工**；M11c 收口前 `GO-6` 必须能解释每一条 `bad_stale` |

---

## 9. 任务分解原则（`M11-plan.md` 的输入）

1. **先修拒绝面**（`F3` 的 panic）：一个 `rc=101` 的编译器在 CI 里会伪装成「测试崩了」，
   而不是「拒绝了一个不支持的程序」。这一步**独立于**并发实现，第一天做。
2. **a 阶段只做栈**：`rt_switch` + 溢出 + `gdesc`，**不碰 chan**。判据只有 `GO-1`。
3. **b 阶段按「调度 → chan → select → 死锁」的顺序**：每一步都能单独跑通一条语料。
4. **c 阶段才动根协议**：`g_frames` 的语义变更（`D-M11-10`）是**一处改动、全局影响**
   —— 放在最后，且改动后立刻跑 M10 的全部根判据（`t_root_09..14`）。
5. **每一步都要能回退**：`hchan` 的四操作是纯 C 函数，可以先用「单 G 直通」
   （不阻塞）跑通，再加阻塞。
