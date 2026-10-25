# M8 测试清单：解释器并发、异常补齐与冻结

> **状态**：**判据面已冻结**（2026-10-25）　|　**设计**：[`M8-design.md`](./M8-design.md)（`D-M8-1..14`）　|　**计划**：[`M8-plan.md`](./M8-plan.md)　|　**事实**：[`M8-manifest.json`](./M8-manifest.json)
>
> **本文档定「怎么算通过」**，不定「怎么做」。凡是本文档与 `M8-design.md` 冲突的，以**本文档**为准（判据优先），并按 §9 的修订流程改设计。

| 项 | 值 |
|---|---|
| 阶段 | M8（M:1 调度 / 通道 / select / 死锁 / range-over-func 窄面 / map 与 defer 补齐 / 冻结） |
| 判据来源 | `S`（自有 fixture，Go 源走完整流水线）、`O`（官方语料：`run` 与 `go run` 逐字节）、`H`（装置 / CLI / 可回放 / manifest） |
| 输出判据 | 合并流**逐字节** + 退出码；唯一容差 `\r\n → \n`（沿 `M5-tests.md` §4.1） |
| 语义基准 | 官方 `go run`（**并发语料只有这一条基准**：`--via=ssa` 在 chan 上仍拒绝，见 §10） |
| 冻结日 | 2026-10-25（判据面 + 并发语料 `N'`/`M'`，沿 `R-6-1`） |
| 门禁 | §7，`CON-1..9` |
| 白名单 | §5，**8 条**，开工前冻结；跳过项一律**不得**记为 pass |
| 变异 | §6.3，**8 条**，每条必须实证变红并复原 |

---

## 1. 判据来源（三种，含名与实）

| 代号 | 含义 | 载体 |
|---|---|---|
| `S` | 自有 fixture：**Go 源**走完整流水线（装载 → 检查 → HIR → 解释执行） | `rgoc-driver/tests/con_*.rs`、`rgoc-interp/tests/*.rs`、`rgoc/crates/rgoc-driver/fixtures/concurrency/` |
| `O` | 官方语料：`rgoc run` 的合并流与退出码 = `go run` 的 | `tools/run-oracle/run-chan.sh` |
| `H` | 装置与可回放：`--sched=seed:N`、同 seed 两次逐字节、manifest 对账、三栏计数 | `rgoc-driver/tests/*.rs`、`scripts/*` |

> **`S` 腿是 M8 的主腿，不是补充**：并发语料**7 个的 stdout 全是 0 字节**
> （`M8-design.md` `F12`），光靠 `O` 只能证明「没崩」。凡是要证明「算对了」的判据，
> **必须**有一条带真输出或自带断言的 `S` fixture。
>
> 好消息是并发语料的「退出码 0」比 M7 强一档：**7 个里 5 个自带断言**
> （`sieve1` 会用链式 goroutine 验 25 个素数，算错就 `panic("fail")`）。§6.1 的三栏
> 必须把这件事写出来，**不许**把空跑那 2 个算进「算对了」。

---

## 2. 覆盖映射

### 2.1 `03-roadmap.md` §M8 十二句 ↔ 测试 ID

| # | roadmap 句子 | 覆盖它的 ID |
|---|---|---|
| 1 | M8a 显式 continuation/解释帧、M:1 队列 | `T-CON-01..05`、`T-CON-55` |
| 2 | M8a goroutine 创建/退出 | `T-CON-01/02/06/10` |
| 3 | M8a 无缓冲 channel | `T-CON-11/12/13` |
| 4 | M8a 可暂停/恢复设计须在契约留位置 | `T-CON-04`（G 状态不串味 = 暂停/恢复的正确性） |
| 5 | M8b 缓冲队列、nil/满/空、close、唤醒/发送 panic、range | `T-CON-14..24` |
| 6 | M8c select ready/default/阻塞 | `T-CON-25..28/30` |
| 7 | M8c 重复 channel、取消其他候选等待者 | `T-CON-31` |
| 8 | M8c 公平性多 seed 测试 | `T-CON-29`、`T-CON-56` |
| 9 | M8d defer/panic/recover 边界 | `T-CON-10/37/38/39/40/51` |
| 10 | M8d range over func | `T-CON-43..48` |
| 11 | M8d map 迭代更新/删除 | `T-CON-49/50` |
| 12 | M8e 同 seed 可回放、多 seed、差分失败可缩减 | `T-CON-55/56/61` |
| 13 | 门禁：`test/chan` 适用子集全绿 | `T-CON-52/53` |
| 14 | 门禁：import 未支持 stdlib 的不混入分母 | `T-CON-54` |
| 15 | 门禁：阻塞/死锁/main 结束可区分 | `T-CON-35/36` |
| 16 | 冻结后只修已声明语义与差分阻塞 bug | `T-CON-60`（manifest 的冻结字段） |

### 2.2 五子阶段 ↔ 门禁 ↔ 测试 ID

| 子阶段 | 门禁 | 测试 ID |
|---|---|---|
| M8a 显式帧与 M:1 | `CON-1` | `T-CON-01..13` |
| M8b 缓冲与 close | `CON-2` / `CON-3` | `T-CON-14..24` |
| M8c select | `CON-4` | `T-CON-25..34` |
| M8d 异常与三块补齐 | `CON-5` / `CON-8` / `CON-9` | `T-CON-35..51` |
| M8e 冻结 | `CON-6` / `CON-7` | `T-CON-52..61` |
| 横切（变异） | — | `T-CON-62..69` |

---

## 3. 测试 ID 全表

### 3.1 甲腿：调度与状态切分（`S`+`H`，10 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-CON-01` | goroutine 创建顺序 | `go a(); go b()` 之后两者都跑；`a` 的 printf 在 `b` 之前（FIFO 入队）。三个 goroutine 的完成顺序与创建顺序一致（无阻塞时） |
| `T-CON-02` | `go` 的实参在**旧** G 求值 | `go f(g())`：`g()` 的副作用（打印）出现在 `go` 语句**之前**；`f` 在之后。再加一条顺序判据：`go f(h(), k())` 里 `h` 先于 `k` |
| `T-CON-03` | 主 goroutine 阻塞即让出 | `main` 先 `<-c`，另一个 G 才 `c <- 1`：若不让出就是**死锁**（红），让出则打印 `1` 并退出 0 |
| `T-CON-04` | 两个 G 的槽不串味 | A 与 B 各自 20 个局部变量、交错写读；用「B 在 A 阻塞点之后改自己的变量」的形态，断言 A 醒来后自己的 20 个值全不变。这条是「暂停/恢复」的正确性判据 |
| `T-CON-05` | 闭包捕获跨 G 共享 | `x := 0; go func(){ x++ }(); <-done; println(x)` ⇒ `1`（同一个 `Cell`）；对照组：`go func(x int){ x++ }(x)` ⇒ 仍是 `0`（传值） |
| `T-CON-06` | goroutine 数上限 | 创建 **10001** 个 ⇒ stderr `goroutine limit exceeded\n` + 退出码 2（不是 OOM、不是挂死） |
| `T-CON-07` | 步数预算**全局**累计 | 两个 G 各忙 6e7 步 ⇒ 合计超 1e8 ⇒ 退出码 2。对照组：单 G 6e7 步 ⇒ 退出 0（证明不是「每个 G 各 1e8」） |
| `T-CON-08` | 帧深**按 G** 计 | 两个 G 各递归 800 层且都成功；单 G 递归 1001 层 ⇒ 退出码 2（沿用 `EX-7` 的形态） |
| `T-CON-09` | 无并发程序零回归 | 一个纯顺序程序（含 defer/panic/map/闭包）在 M8 后逐字节等于 M5 的输出。这条是「拆分没改语义」的最小样本 |
| `T-CON-10` | goroutine 退出跑自己的 defer | G 里 `defer print("d")` 后正常返回 ⇒ 打印 `d`；两个 G 的 defer 各跑一次（按各自退出顺序） |

### 3.2 乙腿：通道（`S`，14 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-CON-11` | 无缓冲 handoff | `c := make(chan int)`；A `c <- 42`、B `<-c` ⇒ B 拿到 42。判据必须含「A 在 B 到场前**阻塞**」：在 A 的发送后打印、B 的接收前打印，断言顺序是 `recv` 在 `send` 之前 |
| `T-CON-12` | 无缓冲的 len/cap | `cap == 0`、`len == 0`（发送方阻塞时 len 仍是 0，值在等待者手里不在 buf 里） |
| `T-CON-13` | 值的**双次**深拷贝 | `a := [3]int{1,2,3}; c <- a; a[0] = 9` ⇒ 接收方读到 `1`（发送时拷贝）；接收方 `v[0] = 7` 后再收一次 ⇒ 仍是 `1`（接收时拷贝）。两条都要有 |
| `T-CON-14` | 缓冲 FIFO | `cap=3` 发 1,2,3、收三次 ⇒ 1,2,3（顺序，不是集合） |
| `T-CON-15` | 缓冲满则阻塞 | `cap=2`，发 1、2（不阻塞）后发 3 ⇒ 阻塞；`len == 2`；收一个后 3 进入 `buf` |
| `T-CON-16` | 接收后阻塞发送方补位 | `cap=2`：发 1、2、3（3 阻塞）→ 收一个 ⇒ 收到 1，且 `len` 立刻回到 **2**（3 已被搬进 buf，发送方被唤醒） |
| `T-CON-17` | closed 上 recv | `close(c)` 后 `<-c` ⇒ 零值 + `ok == false`；缓冲通道要**先排空**（`cap=2` 发两个再 close：前两次 `ok=true`，第三次 `ok=false`） |
| `T-CON-18` | closed 上 range | `for v := range c` 在 close 后正常结束；缓冲里的值**全部先出来**再结束（`1 2` 而不是空） |
| `T-CON-19` | closed 上 send | `close` 后 `c <- 1` ⇒ `panic: send on closed channel` + 退出码 2 |
| `T-CON-20` | double close | 两次 `close(c)` ⇒ `panic: close of closed channel` + 退出码 2 |
| `T-CON-21` | close(nil) | `var c chan int; close(c)` ⇒ `panic: close of nil channel` + 退出码 2 |
| `T-CON-22` | nil 通道永久阻塞 | `var c chan int; <-c` ⇒ 死锁（不是 panic）；`c <- 1` 同理。两条分开，因为它们的阻塞点在两个不同的 `Task` 上 |
| `T-CON-23` | chan 的 len/cap | `make(chan int, 5)` ⇒ `cap == 5`、`len` 随收发变化；`make(chan int)` ⇒ `cap == 0` |
| `T-CON-24` | 聚合元素的深拷贝 | `chan [3]int` 上发一个数组，发送后改源、接收后改副本 ⇒ buf/对端都不受影响（`T-CON-13` 的聚合版，防「只拷了第一个字」） |

### 3.3 丙腿：select（`S`，10 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-CON-25` | 单就绪 | 两个 case，只有一个能收 ⇒ 选中它；另一个 case 的 body 一次都不跑 |
| `T-CON-26` | default 立即走 | 没有就绪 case 且有 `default` ⇒ 立刻走 `default`（不阻塞、不消耗预算到超限） |
| `T-CON-27` | 全阻塞后由别人唤醒 | 两个 case 都不可收，第三个 G 稍后往其中一个发 ⇒ 选中它，且**不是** `default`（无 default 的版本） |
| `T-CON-28` | 多就绪按源码顺序 | 两个都就绪 ⇒ 默认 seed 下**总是**第一个 case。跑 8 次断言 8 次同一个结果（这条同时是 `U-M8-02` 的守卫） |
| `T-CON-29` | 多 seed 的选择分布 | `--sched=seed:1..8`：两个就绪 case 的选中结果**至少出现两个不同的 case**（不需要均匀，只需要「seed 真的起作用」） |
| `T-CON-30` | nil case 永不就绪 | `var c chan int` 作为 case ⇒ 即使另一个 case 阻塞，也只在另一个上等；两者全 nil 且无 default ⇒ 死锁 |
| `T-CON-31` | 重复 channel 的撤销 | `select { case <-c: ...; case v := <-c: ... }`；一次发送后**恰好**一个 case 跑，另一个的登记被撤销（再发一次，另一个才可能跑）。判据必须覆盖「两个 case 的 body 各跑一次」的两轮形态 |
| `T-CON-32` | 空 select | `select {}` ⇒ 死锁 + 退出码 2（不是挂到预算超限） |
| `T-CON-33` | 所有 case 的通道表达式都求值 | 用带副作用的通道表达式（`func() chan int { print("e"); return c }()`）：走 `default` 时**两个** `e` 都要打印（`STM-15` 的原文） |
| `T-CON-34` | recv case 的 comma-ok | `case v, ok := <-c`：未关闭时 `ok == true`，关闭后 `ok == false`。这条同时钉住 `F2`（check 面的 bug）已修 |

### 3.4 丁腿：死锁与退出（`S`+`O`，8 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-CON-35` | 死锁判定 | 所有 G 阻塞且 main 未结束 ⇒ stderr `fatal error: all goroutines are asleep - deadlock!\n` + 退出码 2。反例（必须**不**判死锁）：G 阻塞但 main 还能跑（退出 0）、G 阻塞但 main 已返回（退出 0） |
| `T-CON-36` | main 返回即退出 | `go func(){ for { print("x") } }()` 之后 main 立刻返回 ⇒ 退出码 0，且 `x` 的个数**不是**无限（进程已终止）。判据用「输出长度 < 某个上界」+ 退出码 0 |
| `T-CON-37` | goroutine 内未捕获 panic | G 里 `panic("boom")`，main 在 `<-done` 上阻塞 ⇒ 整个程序退出码 2，合并流含 `panic: boom` |
| `T-CON-38` | goroutine 内 recover | G 里 `defer func(){ recover() }()` 后 panic ⇒ 程序退出码 0，且 panic 之后的语句**不跑** |
| `T-CON-39` | panic 不包装 | G 里 `panic(7)` ⇒ stderr/stdout 的 panic 行是 `panic: 7`（不是 `panic: &PanicError{...}`） |
| `T-CON-40` | 被丢弃的 G 不跑 defer | main 返回时有 G 阻塞在 recv 且它有 `defer print("d")` ⇒ 输出里**没有** `d` |
| `T-CON-41` | 1 与 2 的分界 | range-over-func body 内 `return` ⇒ 退出码 **1**（没跑）；同一程序的死锁版本 ⇒ 退出码 **2**（跑了没跑完）。两者 stdout 都不许有半截输出（1 的时候） |
| `T-CON-42` | 合并流的交错顺序确定 | 两个 G 各打印 5 行、按确定的阻塞点交替 ⇒ 输出行序列**完全确定**（同 seed 两次逐字节相同）；这条与 `T-CON-55` 一起构成「输出可复现」 |

### 3.5 戊腿：三块补齐（`S`，9 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-CON-43` | range-over-func 基本 | `for v := range seq` 打印 0,1,2；迭代器里 `yield` 返回 `true` 时继续、`false` 时提前返回（两形态各一条） |
| `T-CON-44` | range-over-func 的 break | body 里 `break` ⇒ 迭代器看到 `yield` 返回 `false` ⇒ 不再产出（打印不到后面的值） |
| `T-CON-45` | range-over-func 的 continue | body 里 `continue` ⇒ 跳过本轮后半，迭代继续（打印出的值集合正确） |
| `T-CON-46` | range-over-func 嵌套 | 两层 `for range` 套同一个迭代器函数 ⇒ 内层不干扰外层游标（用「外层 3 次、内层 2 次」的计数钉住） |
| `T-CON-47` | range-over-func 的 seq2 | `func(yield func(int, int) bool)` ⇒ `for k, v := range seq2` 两个变量都对 |
| `T-CON-48` | body 内 return / goto 静态拒 | `return` ⇒ `HIR-UNSUPPORTED-RANGEFUNC-RETURN` + 退出码 1；`goto` 同理。两条各一个 fixture，**不许**静默降级 |
| `T-CON-49` | map 迭代：删除 | 迭代中 `delete` 一个尚未访问的 key ⇒ 该 key 不产生迭代；已访问过的 key 被删不影响已打印的 |
| `T-CON-50` | map 迭代：新增不访问 | 迭代中插入新 key ⇒ 迭代次数等于快照长度（新增的被忽略）；同一 map 上第二次 `range` 才看得到 |
| `T-CON-51` | defer 边界 4 条 | ① 命名返回值被 defer 改（`T-EX-34` 的邻居）；② 嵌套 defer 的倒序；③ panic 展开时非当前帧的 defer 也跑；④ defer 里的 `recover` 只对**同一帧**的 panic 有效。补 M5 的 `T-EX-65/66` |

### 3.6 己腿：语料与装置（`O`+`H`，10 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-CON-52` | 三栏计数 | `run-chan.sh` 的汇总行必须是 `自检 a/5 + 仅有输出 b/0 + 空跑 c/2（合计 ok/skip/bad / 19）`；**不许**只写「N'/N' 绿」。`c` 必须**恰好** 2（多了说明有语料变成空跑） |
| `T-CON-53` | 7 个语料与官方逐字节 | 每个的 stdout 与 `go run` 的输出逐字节相同（都是空）+ 退出码 0；`sieve1` 单独加一条「它确实自检」（改成错期望会 panic ⇒ 用反向 fixture 证明断言有牙） |
| `T-CON-54` | 12 个拒绝逐行 | `chan-rejected.txt` 三栏（路径/类别/理由）；两向断言：`N' + M' == 19`、`M'` 集合 == 实测集合、每条类别 ∈ {`errorcheck`,`stdlib`} |
| `T-CON-55` | 同 seed 两次逐字节 | 三个带交错的 fixture 各跑两次，`stdout + stderr + 退出码` 逐字节相同 |
| `T-CON-56` | 多 seed 无失败 | `seed = 1..8` 跑全部 7 个语料 + `T-CON-25..31` 的 fixture ⇒ 全部与期望一致（不是「都退出 0」就完） |
| `T-CON-57` | `--sched` 参数面 | `--sched=seed:7` 生效；`--sched=seed:x` ⇒ 参数错 + 退出码 2；`--sched=random` ⇒ 退出码 2（未实现的模式不静默接受） |
| `T-CON-58` | M5 的 69 语料零回归 | `run-collide.sh --run` ⇒ 69 绿；且每个的 stdout 与 M7 收口时**逐字节**相同（存档对比，不只比计数） |
| `T-CON-59` | M6 的 SSA 面零回归 | `run-collide.sh --ssa` ⇒ 46 绿 / 23 跳过 / 0 红；`SSA-UNSUPPORTED-CHAN` 仍在（守卫断言：chan 程序的 SSA 拒绝**不是**新码） |
| `T-CON-60` | manifest 对账 | `manifest_m8.rs`：五类谎 + 冻结字段（`N'`/`M'` 与清单文件一致、`CON-*` verdict 与缺口同真同假、`done` 与九条全 pass 同真同假）；合成反例 ≥ 9 种 |
| `T-CON-61` | 缩减流程 | 一条故意失败的多 goroutine fixture：删语句（从后往前）能缩到 3 行以内仍失败；脚本 `scripts/shrink-m8.sh` 输出最小复现并**退出码 0**（缩减是工具，不是判据） |

### 3.7 反腿：变异（8 条，每条实证变红并复原）

| ID | 变异 | 必须变红的用例 |
|---|---|---|
| `T-CON-62` | 去掉 `Waiter.done`（改成「被唤醒即弹任务」） | `T-CON-11`（阻塞语义）/ `T-CON-31`（重复 channel） |
| `T-CON-63` | `close` 不唤醒 `recvq` | `T-CON-17` / `T-CON-18` |
| `T-CON-64` | select 不撤销其余登记 | `T-CON-31`（两个 case 各跑一次的两轮形态） |
| `T-CON-65` | 死锁判定改成「有阻塞即死锁」 | `T-CON-35` 的反例（G 阻塞但 main 能跑 ⇒ 必须退出 0） |
| `T-CON-66` | `go` 的实参改在**新** G 求值 | `T-CON-02` |
| `T-CON-67` | 通道值不深拷贝（只存引用） | `T-CON-13` / `T-CON-24` |
| `T-CON-68` | 步数预算改成**按 G** | `T-CON-07` |
| `T-CON-69` | map 迭代改成现场扫（不快照） | `T-CON-50` |

> **变异的纪律**（`M7` 的血）：① 变异前 `cargo clean -p <crate>`（绑定挂载上 mtime 不可靠）；
> ② 变异与恢复**同一调用内**完成（容器 `/tmp` 调用即焚）；③ 变异后**再 clean 一次**才跑全量，
> 否则 `.rlib` 还是变异版（`M7-report.md` §7 实测踩过）。

---

## 4. 判据口径

### 4.1 输出与退出码

| 项 | 口径 |
|---|---|
| 合并流 | `print`/`println`/`panic: <值>` 进**同一个** buffer，顺序 = 写入顺序（`C9` §4.1） |
| 容差 | 只有 `\r\n → \n`；其余逐字节 |
| 退出码 | `0` 跑完 / `1` 没跑（静态拒绝）/ `2` 跑了没跑完（panic、死锁、预算、goroutine 上限） |
| 死锁 | **stderr** 一行 `fatal error: all goroutines are asleep - deadlock!`（不进合并流：它不是 Go 的 panic） |
| 预算/上限 | **stderr** 稳定消息（`step budget exceeded` / `goroutine limit exceeded` / `frame depth exceeded`） |

### 4.2 三条「判据必须有牙」的规矩

1. **凡「算对了」的判据必须带真输出或自带断言**：`T-CON-11..34` 的每条都要打印一个**可区分**的值；
   纯「退出码 0」的用例只许出现在 `T-CON-36/40` 这类「本就没有输出」的判据上。
2. **每条阻塞点都要有「顺序判据」而不是「结果判据」**：`T-CON-11` 要求 `recv` 先于 `send`，
   不然「先跑完 A 再跑 B」也能得到 42，而那证明不了 handoff。
3. **反例与正例同权**：`T-CON-35` 的死锁判定必须同时有「不该判死锁」的两个反例；
   `T-CON-31` 必须有两轮（两个 case 各跑一次），只有一轮时「另一个 case 永远不跑」也绿。

### 4.3 三栏语料（`CON-6` 的牙）

| 栏 | 数量 | 文件 | 能证明什么 |
|---|---|---|---|
| 自检（算错就 panic） | **5** | `select` / `select6` / `select8` / `sendstmt` / `sieve1` | 退出码 0 = **算对了** |
| 仅有输出 | **0** | — | — |
| 空跑 | **2** | `select4` / `zerosize` | 只证明「不崩、不死锁、退出码 0」 |

**纪律**：汇总行必须三栏都写；`c`（空跑）一变就说明语料的性质变了，那条要红。

---

## 5. 白名单（8 条，开工前冻结）

| 码 | 不支持什么 | 触发 | 行为 | 判据 |
|---|---|---|---|---|
| `U-M8-01` | **抢占** | 不存在入口 | 只在阻塞点让出 | 守卫：`for {}` 在 G 里 ⇒ 预算超限退出 2，且**不**切走（`T-CON-07` 的邻居） |
| `U-M8-02` | select **均匀随机** | 多就绪 | 源码顺序（seed=0） | 守卫：`T-CON-28` 跑 8 次同一结果 |
| `U-M8-03` | range-over-func body 内 `return`/`goto` | lowering 见到 | `HIR-UNSUPPORTED-RANGEFUNC-RETURN` + 退出码 1 | `T-CON-48` |
| `U-M8-04` | 11 个 import stdlib 的语料 | 静态 import | 不在 `N'` | `T-CON-54` |
| `U-M8-05` | 定时器 / `time.Sleep` / select 超时 | 无 stdlib | import 期就该失败 | 沿用 `U-M5-01`，不新增判据 |
| `U-M8-06` | goroutine 栈按需增长 | 无 | 帧深 1000，超了退出 2 | 沿用 `EX-7` 形态 |
| `U-M8-07` | 竞态检测 | 无 | 不提供 `-race` | 不适用（单线程） |
| `U-M8-08` | panic 穿过 `go` 边界的包装 | G 内未捕获 | 直接终止（不包装） | `T-CON-39` |

**纪律**：白名单项**不得**记为 pass；`U-M8-01/02` 的「不存在入口」是**守卫测试**——
断言当前行为，行为一变就红。`U-M8-05/06/07` 是「不存在面」，只需在 manifest 里登记，
不需要新判据（沿 `R-31-2` 的守卫先例）。

---

## 6. 冻结

### 6.1 语料切片（`R-6-1` 同源）

| 项 | 值 | 冻结物 |
|---|---|---|
| 输入清单 | `test/chan/*.go` **19** 个 | `tools/run-oracle/chan-input.txt` |
| `N'`（可跑） | **7** | `run-chan.sh` 的实测结果 |
| `M'`（拒） | **12** | `tools/run-oracle/chan-rejected.txt`（三栏） |
| 口径修正 | `Q4` 签字时按 8，实测 7 | `M8-manifest.json` 的 `measurement_deviations` |

### 6.2 门禁（`CON-1..9`）

见 §7。每条门禁的 `evidence` 必须点名**测试文件名**（不写一个与代码不同步的 ID 列表，
沿 `R-59-1`）。

### 6.3 变异（8 条）

见 §3.7。`scripts/mutation-m8.sh` 只放**回归代价低**的 4 条，其余 4 条记录在
`scripts/mutation-m8-notes.md`（沿 M7 的做法）。

---

## 7. 门禁 ↔ 测试 ID

| 门禁 | 判定 | 覆盖它的 ID |
|---|---|---|
| `CON-1` | 无缓冲 handoff + 值深拷贝 | `T-CON-11/12/13/24` |
| `CON-2` | 缓冲 FIFO + 补位 | `T-CON-14/15/16/23` |
| `CON-3` | close 三态 + 三个 panic | `T-CON-17/18/19/20/21/22` |
| `CON-4` | select 五形态 + 撤销 | `T-CON-25..34` |
| `CON-5` | 死锁与四情形退出 | `T-CON-35/36/37/38/39/40/41` |
| `CON-6` | 并发语料 7/7 + 三栏 | `T-CON-52/53/54` |
| `CON-7` | 同 seed 可回放 + 多 seed + 缩减 | `T-CON-42/55/56/61` |
| `CON-8` | range-over-func 窄面 | `T-CON-43..48` |
| `CON-9` | 三块补齐 + 零回归 | `T-CON-49/50/51/58/59` |

---

## 8. 不变式与自检（`CON-5` 的牙）

`RGOC_SCHED_DEBUG=1` 时每步校验四条（`M8-design.md` §4.3）：

| # | 不变式 | 抓什么 | 判据 |
|---|---|---|---|
| 1 | 每个 blocked G 的每个 waiter 要么在某个队列里，要么 `done` | 唤醒漏删 | 变异 `T-CON-62` 必须变红 |
| 2 | 队列里每个 waiter 的 G 存在且 blocked 含它 | 删了没唤醒 | 变异 `T-CON-63` 必须变红 |
| 3 | 每个 G 恰处于 runnable / blocked / running 之一 | 状态机漏洞 | 构造一个「唤醒已退出 G」的 fixture |
| 4 | `buf.len() <= cap` | 缓冲越界 | `T-CON-15/16` 的 Debug 版 |

**这四条本身要做变异验证**——不变式恒真就等于没有不变式。

---

## 9. 修订流程与修订表

**流程**：执行期发现判据写错/写漏 ⇒ 在 §9.2 追加一行 `R-x-y`（日期 / 改了什么 / 触发证据），
同步改 `M8-design.md` 或 `M8-plan.md`，**不许**只改代码不改文档。

### 9.2 修订

| 修订 | 日期 | 内容 | 触发 |
|---|---|---|---|
| `R-8-1` | 2026-10-25 | `range` over chan 的元素落在 **key** 位（不是计划写的 `value` 位） | 官方 `rangeKeyVal` 对通道返回 `(elem, nil)`；写 `value` 位会让 `lower_range` 把它当第二个变量，而检查面只给了一个 |
| `R-8-2` | 2026-10-25 | `send` 的 `closed` 检查必须排**第一**（设计稿 `M8-design.md` §3.3 与 `C11` §3.2 原表都排在 `buf` 之后） | 官方 `runtime/chan.go:224`（`chansend`）加锁后第一件事就是查 `closed`；按原表 `close(c); c<-v`（cap>0 未满）会静默把值塞进缓冲 |
| `R-8-3` | 2026-10-25 | `recv` 的操作数模式是 **commaok** 不是 `value` | 官方 `go/types/expr.go:150`；改之前 `v, ok := <-c` 报 "2 variables but 1 value" |
| `R-8-4` | 2026-10-25 | `select` 被对端完成其中一个 case 时，必须**立刻撤销**其余登记 | 官方 `sudog.dequeue` 的 `g.selectDone.CAS(0,1)`；不撤销 ⇒ 发送方下一笔发送落到作废 waiter 上，**值丢失**（`T-CON-31` 抓到：输出 "first 1" 后死锁） |
| `R-8-5` | 2026-10-25 | `select` 的登记表（`select_regs`）放 `Shared` 不放 `G` | 撤销发生在**对端** G 里，而被撤销的 G 已被 `drive` `take()` 走 |
| `R-8-6` | 2026-10-25 | `next_token` 从 **1** 起（0 留给「普通 waiter 没有 token」） | 从 0 起 ⇒ `revoke(0)` 把阻塞的普通发送方一起删掉 ⇒ 死锁 |
| `R-8-7` | 2026-10-25 | `range` over chan 的执行接法：`RangeIter::Chan` + `step_range` 的 chan 支路，压 `Task::Recv` 进相位机（阻塞天然成立）。计划里的 `R4`（「`next()` 是同步函数，塞不进同一接口」）结论不变，但接法是**新支路**不是改接口 | `t_con_18` 抓到相位机错法：压 recv 的那一步必须直接进 `PH_POST`，否则下一步会再压一个 ⇒ 值栈两份结果 |
| `R-8-8` | 2026-10-25 | `close` 的 panic 文本**不带** `runtime error:` 前缀 | 容器里 `go run` 实测：`panic: close of closed channel` / `close of nil channel` |
| `R-8-9` | 2026-10-25 | `--sched` 的写法是 `seed:N` 不是 `seed=N` | `T-CON-29` 的 fixture 按 `seed=` 写全部解析失败 |
| `R-8-10` | 2026-10-25 | `RangeIter::Map` 必须快照 **key 列表**（不是 `(k,v)` 对） | 迭代中 `delete(m,k)` 之后 Go **不再**产出 `k`（`T-CON-49`）；值仍现读（`T-CON-50b`） |
| `R-8-11` | 2026-10-25 | `T-CON-48` 的码从 `HIR-UNSUPPORTED-RANGE-FUNC` 换成 **`HIR-UNSUPPORTED-RANGEFUNC-RETURN`**（前者随 range-over-func 实现而下线） | Task 45 落地后旧码无产出点 |
| `R-8-12` | 2026-10-25 | `select` 子句的 `env.push()` 挪到通信语句**之前** | `case v := <-a:` 的 `v` 属于该子句；挪后报 "no new variables on left side of :=" |
| `R-8-13` | 2026-10-25 | `Task::Make` 必须带 `nargs`（不靠「栈上还有没有值」猜） | `f(make(map[string]int))` 会把 `f` 的实参吃掉 |
| `R-8-14` | 2026-10-25 | `select` 子句体是**无标签 `break` 的目标**（`Body::Seq` 加 `breakable`） | 官方 33 / 我们算出 1；`select` 不在 body 栈上，子句体是唯一能拦 break 的东西 |

---

## 10. 与 M5/M6/M7 的判据边界

| 面 | M8 的口径 |
|---|---|
| M5 的顺序语义 | **零回归**：69 个语料的 stdout 逐字节不变（`T-CON-58`）。M8 **不重写**顺序语义，只加并发 |
| M5 的 defer/panic/recover | 实现不动，只补判据（`T-CON-51`） |
| M6 的 SSA | **零回归**：`--via=ssa` 46 绿 23 跳过 0 红；`SSA-UNSUPPORTED-CHAN` 保持（`T-CON-59`） |
| M7 的 native | **零回归**：`run-native.sh` 39/7/0 不变；M8 不碰 `rgoc-arm64` |
| 三方差分 | **在 chan 语料上退化为两方**（`run` vs `go run`）——因为 SSA 侧拒绝。这条写进 `M8-report.md`，不许说「三方差分」 |
| `C9` 的冻结 | M8e 之后 `C9` 去掉「候选」字样；M8 之后只修已声明语义与差分阻塞 bug |
