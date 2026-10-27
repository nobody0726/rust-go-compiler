# M11 判据 · native 并发（栈 / 调度 / channel / select）

> 配套：`M11-design.md`（`D-M11-1..12` / `F1..F12` / 门禁 `GO-1..GO-7`）。
> 本文只写**怎么验**，不写怎么实现。每条判据都写清「红了说明什么」——
> 因为 M10 的教训是「判据没牙」比「实现有 bug」贵得多（`M10-report.md` §5）。

---

## 0. 语料（所有判据的地基）

### 0.1 三组语料

| 组 | 清单 | 个数 | 用法 |
|---|---|---|---|
| `N'` | `chan-input.txt` − `chan-rejected.txt` | **7** | native / 解释器 / 官方 **三层逐字节** |
| `M'` | `chan-rejected.txt` | **12** | 静态拒绝且**不 panic**（rc=1，不是 101）|
| `FIX` | 新增 fixture（§1–§6 各条内联）| ~15 | 每个门禁至少一条 |

`N'` 的 7 个：`select.go` / `select4.go` / `select6.go` / `select8.go` /
`sendstmt.go` / `sieve1.go` / `zerosize.go`。

### 0.2 为什么 `N'` 只有 7 个、且多半「输出为空」

`chan-rejected.txt` 的 12 个全是**静态面**拒的（11 个 `import` stdlib、1 个
`errorcheck` 用例）。所以 `N'` 不是「我们挑出来的」，是「**能过检查面的全部**」。

7 个里有几个输出为空（`zerosize.go` 只 `make` 不打印）。**空输出不构成证据** ——
所以 `run-native-chan.sh` 的汇总行分三栏（自检 / 有输出 / 空跑），
沿用 `run-chan.sh` 的口径：只写「7/7 绿」会把三种情况混成一句。

### 0.3 `mode=` 的用法（`D-M11-12`）

| 阶段 | 期望 `mode=` | 判据 |
|---|---|---|
| M11a/b | **`off`**（强制，即使设了 `RGOC_GC_STRESS`）| `T-GROOT-04` |
| M11c | `on` / `stress`（`RGOC_GC` 恢复正常语义）| `T-GROOT-05` |

这条**不是**「顺带看一眼」：它是「GC 重新启用」的唯一可观测开关。
a/b 阶段如果 `mode` 打出 `on`，说明 `rt_gc_force_off()` 没接上，而
「关 GC」就成了一句空话。

### 0.4 Task 0（`G-M10-06` 调查）的验收判据

> ✅ **2026-10-27 全部通过。** 记录见 `scripts/g-m10-06-probe.md`。

Task 0 不是实现任务，它的产出是**证据 + 一句机制说明**。判据不是 `GO-*` 门禁，
但**没过 Task 0 不许开 Task 1**（`M11-design.md` §2.5）。

| ID | 判据 | 红了说明什么 |
|---|---|---|
| `T-INV-01` | **复现**：把 `live.rs` 的 `set` 改成 `(after[ii] − ins.dsts()) ∪ ins.uses()`，四个语料（`defernil` / `print` / `nilptr2` / `method5`）**逐个**记录失败形态（stdout diff / rc / stderr），四份都留档 | 复现不出来 ⇒ 当时的观察本身要重看（可能不是位图语义导致的）|
| `T-INV-02` | **最小化**：从四个语料里挑最小的那个切出**最小复现**，并写清它命中的是 `MakeClosure` / `Convert` / `Bin` 里的哪一个 | 只有「大语料坏」没有最小复现 ⇒ 机制未定位，Task 0 不算完 |
| `T-INV-03` | **归因**：三条候选机制（A 分配未发布窗口 / B `sp_idx` 配对偏差 / C 槽复用）里**恰好一条**被实测证实，另外两条被证伪（各附证伪理由，不是「大概不是」）| 三条都「有可能」⇒ 说明探针没打准靶（M10 的六次变异绿就是这个形态）|
| `T-INV-04` | **结论落地**：`G-M10-06` 从 `M10-manifest.json` 的 `deferred_gaps` 移出（进 `closed_gaps` 或改写成 `C12` 的显式不变式）；`live.rs` 第 330–339 行那段注释里的「机制没查清」被换成机制说明 | 代码里留一句「没查清」⇒ 下一个人会再踩一次 |
| `T-INV-05` | **不回归**：Task 0 结束时 `run-native.sh` 三态仍各 69/0/0，四条 cargo 门禁全绿 | 调查把主线弄红了 ⇒ 先复原再谈结论 |
| `T-INV-06` | **守卫**（`t_gc_12`）：`rt_defer_push` 里断言 `gc_pause++` < `rt_arena_alloc` < `g_defer_top = r;` < `gc_pause--` | 删掉/挪错 `gc_pause` ⇒ 窗口又回来了。**只查「出现」不够** —— 写在发布之后也照样绿 |

**实测结果**（2026-10-27）：

| 判据 | 结果 |
|---|---|
| `T-INV-01` | 四语料全坏：`defernil` rc=2 / `nilptr2` rc=139 / `method5` rc=139 / `print` rc=139（83 B 就停）|
| `T-INV-02` | 最小复现 = `MakeClosure` 的 dst（只对它去 def，四个照旧坏）；`probe1.go` 20 行，`A: in main defer` 不打印 |
| `T-INV-03` | A 证实（形态修正为「登记动作自己的分配」）/ B 证伪 / C 证伪 |
| `T-INV-04` | `G-M10-06` → `closed_gaps`；`C12` §4.4.1 + §6.1（`R-10-24`）；`live.rs` 注释改写 |
| `T-INV-05` | 三态各 **69/0/0**；fmt / check / clippy / test（**2235 passed / 0 failed**）|
| `T-INV-06` | 绿；**反向验证**：删掉两行 `gc_pause` ⇒ 红（`找不到 gc_pause++;（R-10-24）`）|

> `T-INV-01` 的四个语料名来自 `M10-manifest.json` 的 `deferred_gaps[0].what`，
> 不是从别处抄的。**如果复现出来的不是这四个**，那是发现 —— 要改的是 manifest
> 的描述，不是把判据迁就成「另外四个」。（实测：就是这四个。）

---

## 1. `GO-1` 栈与切换

| ID | 判据 | 红了说明什么 |
|---|---|---|
| `T-STK-01` | **callee-saved 全保住**：fixture 往 `x19..x28` 与 `d8..d15` 各放一个**可区分值**，切 1000 次（`rt_switch` 直接调）后逐个比对 | 切换漏存某个寄存器 |
| `T-STK-02` | **`sp` 对齐**：切换后 `sp % 16 == 0`（AAPCS64 要求）；切 1000 次仍成立 | 保存区大小不是 16 的倍数 |
| `T-STK-03` | **溢出报错**：递归到超过 64 KiB ⇒ stderr `runtime error: goroutine stack exceeded 65536 bytes` + 退出 2 | 序言检查没发 / 文本不对 |
| `T-STK-04` | **序言有检查**：`.s` 里每个 `frame_size > 0` 且非 `off_stack` 的函数，序言含 `cmp sp, …` + `b.lo`（扫汇编）| 只在部分函数发（漏了小帧）|
| `T-STK-05` | **超大帧不查**：`nilptr2.go`（128 MiB 帧走 mmap）仍绿，且它的 `.s` 里**没有**栈检查 | 把 mmap 帧也查了 ⇒ 误报溢出 |
| `T-STK-06` | **溢出深度实测钉住**（`§2.4 ①` 的修正）：递归 fixture 在 64 KiB 下**实测**溢出发生在第 `N` 层；`N` 写进判据并**必须能复现**（跑两次相同）；同时断言语料里递归最深的 `sieve1.go` 在 64 KiB 下**不**溢出，且 `depth(sieve1) < N` 有富余 | 要么栈太小（语料都溢出了）、要么「64 KiB 够用」是估算而不是实测 |

> `T-STK-01` 的「可区分值」要选**能被误用暴露**的：`x19 = 0x1111…`、
> `x20 = 0x2222…`、`d8 = 1.5`、`d9 = -2.25` …… 只用 0/1 的话「没保存」和
> 「保存对了」看不出来。

---

## 2. `GO-2` 调度三规则

| ID | 判据 | 红了说明什么 |
|---|---|---|
| `T-GO-01` | **M:1**：整个程序只创建 **1 个** OS 线程。判据是 `.s` + `rt.c` 里不出现 `clone`/`clone3`/`pthread_create`，且 `/proc/self/status` 的 `Threads: 1`（fixture 里读不到就退化成静态扫描）| 有人偷偷起了线程 |
| `T-GO-02` | **不抢占**：`go func(){ for {} }()` + `main` 里 `println(1)` ⇒ 若 `go` 不切走当前 G，则 `main` 先打完 1 再挂死（装置超时 124）；反例：`go` 之后 `main` 立刻阻塞 ⇒ 那个 `for {}` 永远跑（也超时）| 按步数/时间切走了 |
| `T-GO-03` | **就绪队列 FIFO**：`for i:=0;i<5;i++ { go print(i) }` + `main` 阻塞 ⇒ 输出必须是 `0 1 2 3 4`（**不是**倒序、不是乱序）| 入队到队头 / 用了栈 |
| `T-GO-04` | **`go` 的参数深拷贝**：`x := [2]int{1,2}; go f(x); x[0]=9` ⇒ `f` 看到 `{1,2}` | 传了引用（`D-M11-6`）|
| `T-GO-05` | **确定性**：同一程序跑两次逐字节相同（含 5 个 G 的 fixture）| 有非确定性来源（地址、时间、哈希序）|

> `T-GO-02` 是唯一**故意超时**的判据：它断言的是「挂死」这件事本身。
> 装置必须把 `timeout` 的退出码（124）当成**期望值**，而不是失败。

---

## 3. `GO-3` `hchan` 四操作

| ID | 判据 | 红了说明什么 |
|---|---|---|
| `T-CHAN-01` | **无缓冲同步**：`go func(){ println(<-c) }(); c <- 42` ⇒ 打印 42，且**顺序固定**（发送方阻塞后接收方跑）| 队列语义错 |
| `T-CHAN-02` | **缓冲 FIFO**：`cap=3`，发 3 个再收 3 个 ⇒ 顺序相同 | 环形下标（`sendx`/`recvx`）错 |
| `T-CHAN-03` | **`closed` 排第一**：`c := make(chan int, 1); close(c); c <- 1` ⇒ `panic: send on closed channel`（**不是**静默塞进缓冲）| `C11` §3.2 的 ⚠️ 被违反 |
| `T-CHAN-04` | **close 后 recv 排空**：`cap=2` 发 2 个 → close → 连续 recv ⇒ 先拿到两个值（`ok=true`），再拿零值（`ok=false`）| 唤醒顺序错 |
| `T-CHAN-05` | **`len`/`cap`**：`len` 数缓冲、**不数**阻塞的发送方；`cap` = `dataqsiz` | 把 `sendq` 长度算进 `len` |
| `T-CHAN-06` | **值语义双次拷贝**：`ch <- a; a[0]=9` ⇒ 接收方看到原值；接收方 `v[0]=7` ⇒ 缓冲里不变 | 少拷一次 |
| `T-CHAN-07` | **nil 通道永久阻塞**：`var c chan int; <-c` ⇒ 死锁（不是 panic）| 把 nil 当「空缓冲」|
| `T-CHAN-08` | **`Sudog` 不泄漏**：10 万次「发送阻塞→被接收」之后 `live` 有界（`RGOC_GC` 关闭时用 arena 预算；M11c 起用 `live`）| `Sudog` 进了 arena（`D-M11-8`）|

### 3.1 与解释器的逐格差分（`T-CHAN-09`）

`C11` §3.2 的四张表是**状态机**，两边各实现一遍就会分叉。所以：

```text
喂同一串操作序列（send/recv/close/len/cap 的脚本化序列）
  ├─ 解释器：rgoc run --via=ssa? 不 —— 走 HIR 求值器
  └─ native：编译后跑
比较每一步之后的可观测状态（打印 len/cap/值/是否阻塞）
```

这套「差分」的做法与 M8 的 `T-CON-*` 同源（那边是 56 格 × 3 操作 = 168 次）。
M11 的判据是**同一份脚本两边跑**，不是「各写一遍单测」。

---

## 4. `GO-4` `select`

| ID | 判据 | 红了说明什么 |
|---|---|---|
| `T-SEL-01` | **求值一次**：`select { case a <- f(): case b <- f(): }` ⇒ `f` 被调**恰好 2 次**（不是 0、不是 4）| 阶段①漏了某个 case |
| `T-SEL-02` | **走 default 也求值**：`select { case c <- f(): default: }` ⇒ `f` 调 1 次 | 同上（`STM-15`）|
| `T-SEL-03` | **多就绪取源码第一个**：两个 case 都就绪 ⇒ 永远走第一个（`C11` §4.3，`seed=0`）| 用了随机 |
| `T-SEL-04` | **撤销**：`case <-a: case <-b:`，只有 `b` 就绪 ⇒ `a` 的队列里**不留**自己的 sudog（再往 `a` 发不会唤醒幽灵）| 撤销漏了 |
| `T-SEL-05` | **重复 channel**：`case <-c: case v := <-c:` 同时存在 ⇒ 只有一个 `done`，只跑一个 body | `token` 的 winner 判定错 |
| `T-SEL-06` | **空 select**：`select {}` ⇒ 立即死锁（不是烧到超时）| 没有「没有队列可登记」这条路 |

---

## 5. `GO-5` 死锁与退出

| ID | 判据 | 红了说明什么 |
|---|---|---|
| `T-EXIT-01` | **死锁文本逐字节**：stderr 恰好 `fatal error: all goroutines are asleep - deadlock!\n`，退出 2，**stdout 为空** | 混进合并流 / 文本有前缀 |
| `T-EXIT-02` | **`main` 返回即退出**：`go func(){ println("late") }(); ` 然后 `main` 直接返回 ⇒ 输出**不含** `late`，退出 0 | 等了 goroutine |
| `T-EXIT-03` | **不跑其它 G 的 defer**：G 里 `defer println("d")`，`main` 返回 ⇒ 没有 `d` | 跑了别人的 defer |
| `T-EXIT-04` | **G 内 panic 退出 2**：G 里 `panic("x")` ⇒ stderr `panic: x`，退出 2（不包装、不跑别人 defer）| 包装了 / 吞了 |
| `T-EXIT-05` | **三条死锁条件各自必要**：① 有就绪 G ⇒ 不是死锁；② `main` 已返回 ⇒ 不是死锁（走 `T-EXIT-02`）；③ 没有 blocked ⇒ 不是死锁 | 条件写漏一条 |

---

## 6. `GO-6` 多 G 根扫描（M11c）

| ID | 判据 | 红了说明什么 |
|---|---|---|
| `T-GROOT-01` | **挂起 G 的栈是根**：G 里 `p := new([1024]byte); p[0]=42; <-c`（阻塞），`main` 里 churn 2 万次，再唤醒 ⇒ `p[0]` 仍是 42 | `mark_frames` 没走 blocked G |
| `T-GROOT-02` | **就绪 G 的栈也是根**：G 已入队未运行，它的栈上对象不能被收 | 只扫了 running/blocked |
| `T-GROOT-03` | **`WaitSet` 保活**：阻塞在 `select` 上时，`WaitSet` 里的 `chan` 指针不能被收 | `gdesc->wait` 没标 |
| `T-GROOT-04` | **a/b 阶段强制关**：并发语料 + `RGOC_GC_STRESS=1` ⇒ 统计行 `mode=off` 且 `collections=0` | `rt_gc_force_off` 没接上 |
| `T-GROOT-05` | **c 阶段翻过来**：同一语料 + `RGOC_GC_STRESS=8` ⇒ `mode=stress` 且 `collections>0`，输出不变 | 开关没翻（或翻了但扫错）|
| `T-GROOT-06` | **多 G 下的超集扫描不坏**（`G-M10-06` 的回归面）：N 个 G 各自持有栈上指针、轮流阻塞/唤醒、stress 态跑 ⇒ 输出与 `off` 态逐字节相同，且 `bad_ptr == 0`（`bad_stale` 允许非 0，但每条都要能归因到「超集位图」）| 位图语义在多 G 下放大成真错误 —— 这条是 §2.5 的调查结论在判据面的落点 |

> `T-GROOT-04/05` 是**同一条判据的两个阶段**。a/b 收口时 04 绿、05 红（预期）；
> c 收口时两条都绿。这样「GC 什么时候重新启用」是可观测的，不靠人记。

---

## 7. `GO-7` 语料

| ID | 判据 | 红了说明什么 |
|---|---|---|
| `T-GCORP-01` | **7 个 `N'` 三层一致**：native vs 解释器 vs 官方 `go run`（stdout / stderr / 退出码）| 主判据 |
| `T-GCORP-02` | **12 个 `M'` 仍拒绝**：退出码 1 + stderr 非空（拒绝必须有理由）| 拒绝面被误放宽。⚠️ **不匹配具体诊断码**：`chan-rejected.txt` 第三列写的是码（`TYP-UNSUPPORTED-IMPORT-FORM`），而 stderr 渲染的是人话（`unsupported import form: "flag"`）—— 断码会变成「测渲染格式」，那是 `C8` 的事 |
| `T-GCORP-03` | **并发语料不 panic**：`M'` 12 个全是 rc=1（守既有面）+ **`N'` 7 个的退出码没有 101**（`F3` 的回归判据）| 效果摘要又跑到 `unsupported()` 前面。⚠️ **靶子修正**：初稿只查 `M'` 的 12 个，但那 12 个**本来就全是 rc=1**（走 import/check 路径，到不了 `effect.rs`）；实测 rc=101 的是 `N'` 里的 **6 个**（`select`/`select4`/`select6`/`select8`/`sendstmt`/`sieve1`）|
| `T-GCORP-04` | **69 个 M5 语料零回归**：`run-native.sh` 三态仍各 69/0/0 | 并发改动污染了单线程通路 |

---

## 8. 装置

| 装置 | 用途 |
|---|---|
| `tools/run-oracle/run-native-chan.sh` | `N'` 的 native 跑法 + 三层差分；汇总行三栏（自检/有输出/空跑）|
| `run-native.sh` | 69 个单线程语料（`T-GCORP-04`）|
| `rgoc-driver build --keep-asm` | `T-STK-04` / `T-GO-01` 的静态扫描面 |
| `timeout 20` | `T-GO-02` 的期望退出码 124 |

### 8.1 装置判据（装置自己也要有判据）

| ID | 判据 |
|---|---|
| `T-GDEV-01` | `run-native-chan.sh` 的 `N'` 清单与 `chan-input.txt − chan-rejected.txt` 一致（算出来，不抄）|
| `T-GDEV-02` | 汇总行的三栏相加等于分母（`a+b+c == N'`）|
| `T-GDEV-03` | `M'` 的行数等于 `chan-rejected.txt` 的行数（冻结）|

---

## 9. 变异（每条必须实证变红）

| # | 变异 | 期望红的判据 |
|---|---|---|
| `M-M11-01` | `rt_switch` 不保存 `d8..d15` | `T-STK-01` |
| `M-M11-02` | 保存区从 160 改成 144（漏一对）| `T-STK-01` |
| `M-M11-03` | 序言不发栈检查 | `T-STK-03` |
| `M-M11-04` | 栈检查对 `off_stack` 帧也发 | `T-STK-05` |
| `M-M11-05` | `go` 只拷指针不拷值 | `T-GO-04` |
| `M-M11-06` | 就绪队列改成队头插入 | `T-GO-03` |
| `M-M11-07` | `send` 的 `closed` 检查挪到「buf 未满」之后 | `T-CHAN-03` |
| `M-M11-08` | `close` 只唤醒 `recvq` 不唤醒 `sendq` | `T-CHAN-04` |
| `M-M11-09` | `select` 阶段①只求值第一个 case | `T-SEL-01` |
| `M-M11-10` | `select` 不撤销（只删 winner 的 sudog）| `T-SEL-04` |
| `M-M11-11` | 死锁判定漏「有就绪 G」 | `T-EXIT-05` |
| `M-M11-12` | `main` 返回后等 goroutine | `T-EXIT-02` |
| `M-M11-13` | `mark_roots` 只扫当前 G 的链 | `T-GROOT-01` |
| `M-M11-14` | `rt_gc_force_off` 空实现 | `T-GROOT-04` |
| `M-M11-15` | `Sudog` 改回 arena | `T-CHAN-08` |

> **两条「该绿」的**（写进 notes，不进脚本）：
> ① 把 `RT_GC_STRESS` 强行打开（绕过 `force_off`）⇒ `T-GROOT-04` 应**红**，
> 但 `T-GROOT-01` 在 a/b 阶段应**绿**（因为那时 `g_frames` 还是单链，只有当前 G
> 的栈被扫，而 a/b 的 fixture 里没有挂起 G 持有堆对象）。这条绿**是预期的**：
> 它证明 `T-GROOT-01` 真的依赖 c 阶段的改动，不是「本来就绿」。
> ② 死锁文本加 `runtime error:` 前缀 ⇒ `T-EXIT-01` 红；而解释器侧不动 ⇒
> `T-GCORP-01` 也红。两边一起红说明**判据连着**，不是各测各的。

---

## 10. 不变式（`M11-design.md` 的硬约束，逐条要有判据）

| # | 不变式 | 判据 |
|---|---|---|
| 1 | 活值只在栈上（**没有寄存器根**）| `t_gc_10`（M10 留下的守卫，**不许为 M11 改**）+ `T-STK-01` 的对称面 |
| 2 | 一个 G 只在一个 OS 线程上跑，且同一时刻只有一个 G 在跑 | `T-GO-01` + `T-GO-05` |
| 3 | `gdesc` 在 arena（不回收），`WaitSet`/`Sudog`/`buf` 在托管堆 | `T-CHAN-08` + `T-GROOT-03` |
| 4 | `hchan` 的 96 字节字段序与 `C11` §7 逐字段一致 | `T-SCH-01`（M9 已有，M11 复用不改）|
| 5 | 死锁的三条条件同时成立才报 | `T-EXIT-05` |
| 6 | `mode=off` 在 a/b 阶段是**强制**的 | `T-GROOT-04` |

---

## 11. 与 M5–M10 的判据边界

| 判据面 | 谁管 |
|---|---|
| 帧布局（`SAVED=32`、记录区三个字）| M10（`C12` §4.2）—— M11 不改 |
| 根协议（`sp_idx` / 位图 / `mark_frame`）| M10 —— M11 只**增加**「扫哪些 G」 |
| `hchan`/`Sudog` 的**布局** | M9（`C11` §7）—— M11 只**实现**行为 |
| 并发**语义**（三规则 / 四操作 / 三阶段 / 死锁）| M8（`C11` §2–§6）—— M11 只**移植** |
| 退出码与合并流 | M5（`C9` §5）—— M11 只复用 |
| 单线程语料 69 个 | M7–M10 —— M11 必须零回归（`T-GCORP-04`）|

---

## 12. 修订流程

1. 新增一条判据 → 先在 `M11-design.md` 的对应决策里找到它的**理由**；
   找不到理由的判据是「为了测试而测试」，删掉。
2. 新增一条**不承诺** → 必须先在 §5 白名单里出现，且在 `C11` §6 的正文里补痕迹
   （`C11` §6 自己的要求：「否则它就只是声明而不是契约」）。
3. 判据红了先分辨三类：**实现错** / **判据没牙** / **装置打错靶**。
   M10 的六次「变异绿」全是后两类（`mutation-m10-notes.md` §三）。
4. 改 `t_gc_10` 一律视为**契约变更**：它是 M10 的守卫，M11 的正确做法是
   「让切换把寄存器压进栈」，而不是「放宽守卫」。
