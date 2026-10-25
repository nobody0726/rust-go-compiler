# M8 设计：解释器并发、异常补齐与冻结

> **状态**：✅ **已确认（2026-10-25）** —— 决策 `D-M8-1..14` / 事实基线 `F1..F16` / 门禁 `CON-1..9`
> **权威范围**：`docs/03-roadmap.md` §M8（本文件只做**展开与决策**，不改范围）
> **上游**：`M5-design.md` §3.4（显式栈机 `D-M5-1`）、`M5-report.md` §5（缺口 `G-M5-*`）、
> `C3-hir.md` / `C9-values-and-output.md`、`M6-design.md`（拒绝面先例）、
> `M7-report.md` §5（「十九条真错」的判据教训）
> **下游**：`M8-tests.md`（判据）、`M8-plan.md`（任务分解）、`M8-manifest.json`（机器可读事实）
>
> **四条已签字**（§2.4）：状态抽 `G` 结构 / 确定性 FIFO + seed 只选 select /
> range-over-func 先做窄面 / 语料 7 进 12 拒。

---

## 0. 本文档在流程中的位置

```text
03-roadmap §M8（做什么）→ 本文档（怎么定、边界在哪）→ M8-tests.md（怎么算通过）→ M8-plan.md（按什么顺序做）
```

三件套齐备才开工（`03` §6.2）。M8 与前七个里程碑的**根本差别**：M1–M7 都是「把新东西
造出来」，M8 有**一半是兑现旧账**——

| 旧账 | 记在哪 | M8 怎么还 |
|---|---|---|
| 「显式帧，挂起 = `FrameState::Blocked`」 | `M5-design.md` §457 | 真的加 `Blocked`，真的换 goroutine |
| defer/panic/recover 边界 | `M5-report.md` §5（`T-EX-65/66` 部分兑现） | 补判据 + 补跨 goroutine 的语义 |
| 「顺序语义基线只是**候选**，正式冻结等 M8e」 | `M5-design.md` `D-M5-14` | M8e 出**冻结报告** |
| `HIR-UNSUPPORTED-{GO,CHAN,SELECT,CLOSE,RANGE-FUNC}` | `rgoc-hir/src/lower.rs` | 五条码**下线**（其中 range-over-func 留一个更窄的码） |

因此本文档的每一处「新增」都要先回答一句：**它是不是在把 M5 留的口子用掉**。
不是的话，就要说明为什么要新开一个口子。

---

## 1. 目标与非目标

### 1.1 输入与输出

| | 内容 |
|---|---|
| **输入 ①** | M5 的**顺序解释器**：`Interp` 的五个 `Vec`（帧栈 / 求值栈 / 值栈 / 目标栈 / `pending`）+ `Body` 游标栈（`F6`/`F7`） |
| **输入 ②** | M5 的 **HIR**（`Stmt`/`ExprKind`/`RangeKind`/`MakeKind`/`BuiltinKind`，`F5`）—— 并发节点挂在这里，不新建 IR |
| **输入 ③** | M4/M3 的**类型面**：`Type::Chan(dir)` 已经在 `rgoc-types` 里，布局是 8/8 |
| **输入 ④** | `check` 已经**放行**并发（`F1`）—— 唯一挡路的是 HIR lowering，不是类型系统 |
| **输入 ⑤** | `C9` §4/§5 的输出字节与退出码表（`F12`） |
| **输出 ⑥** | `rgoc-hir`：`Stmt::{Go,Send,Select}` + `ExprKind::Recv` + `RangeKind::{Chan,Func}`（`D-M8-10`） |
| **输出 ⑦** | `rgoc-interp`：`Shared` / `G` / `ChanObj` / `Waiter`（§3.2–§3.6） |
| **输出 ⑧** | 新契约 **`C11-concurrency.md`**：调度语义、通道状态机、select 规则、死锁判定、白名单 |
| **输出 ⑨** | `C3` 增补（并发节点的求值顺序）与 `C9` 增补（死锁/goroutine-panic 两行 + map 迭代口径） |
| **输出 ⑩** | 装置：`run-chan.sh`（并发语料对撞）、`M8-tests.md` / `M8-plan.md` / `M8-manifest.json`、**冻结报告**（M8e） |

### 1.2 范围（`03-roadmap.md` §M8 逐句展开）

| # | roadmap 句子 | 本文档的落点 |
|---|---|---|
| 1 | M8a「显式 continuation/解释帧、M:1 队列、goroutine 创建/退出、无缓冲 channel」 | §3.2（抽 `G`）、§3.6（M:1 队列）、§3.3（无缓冲 handoff）、§3.7（创建/退出） |
| 2 | M8a「可暂停/恢复设计须在 M5/M6 契约留位置」 | **已经留了**（`D-M5-1`），M8 是**用掉**它：见 §0 的旧账表与 §3.4 |
| 3 | M8b「缓冲队列、nil/满/空 channel、close、唤醒/发送 panic、range」 | §3.3 的四个操作真值表 + §3.7 的 panic 面 + `RangeKind::Chan` |
| 4 | M8c「select ready/default/阻塞、重复 channel、取消其他候选等待者、公平性多 seed」 | §3.5（三阶段算法 + token 撤销）+ §3.6（seed）；公平性按 `D-M8-3` 的口径 |
| 5 | M8d「defer/panic/recover 边界、range over func、map 迭代更新/删除」 | §3.11（defer 边界）、§3.9（range-over-func **窄面**）、§3.10（map 快照口径） |
| 6 | M8d「泛型 run 按已支持语义选取，不要求字典/内联测试全过」 | 不动：M5 的泛型面就是「`any` 约束 + 推断」（`G-M5-03`），M8 不扩也不缩 |
| 7 | M8e「同 seed 可回放、性质测试多 seed 无已知错误、差分失败可缩减」 | §4.2：`(源码, seed, 预算)` 唯一决定执行，**不记事件日志**（`D-M8-4`） |
| 8 | M8e「形成冻结候选报告」 | `M8-report.md` 的「冻结报告」节 + `C9` 升正式（去掉「候选」字样） |
| 9 | 门禁「`test/chan` 的适用子集全绿」 | §4.1：**7 个**（实测），三栏报 |
| 10 | 门禁「导入未支持 stdlib 的测试不混入必需分母」 | §4.1：12 个进 `M'`，逐行写理由 |
| 11 | 门禁「阻塞/死锁/进程 main 结束行为可区分」 | §3.7 的四情形表 + `CON-5` |
| 12 | 「冻结后只修已声明语义与差分阻塞 bug」 | §4.4 的冻结纪律（与 `R-6-2` 同源） |

### 1.3 明确不做（M8 边界）

| 不做 | 为什么 | 归谁 |
|---|---|---|
| native 的 `go` / `select` / channel 对象 | roadmap §M9d 明确「channel 对象与调度契约在此设计与测试，native 执行门禁放 M11」 | M9d（契约）/ M11（门禁） |
| SSA 侧的并发（`SSA-UNSUPPORTED-CHAN` 保持） | M6 的 IR 没有「等待」这个概念；加进去要重做 memory 分区与 verifier | M11 |
| 抢占（异步/协作式安全点抢占） | 解释器在**阻塞点**让出就够了；加抢占会把确定性砍掉（`D-M8-13`） | 不做，登记 `U-M8-01` |
| 真并行（多 OS 线程） | 直接杀死「同 seed 可回放」（M8e），且 `D-M5-1` 已否决递归/线程栈 | 不立项 |
| 定时器 / `time.Sleep` / `select` 超时 | 没有 stdlib；引入墙钟会毁掉可回放（同 seed 不同机器不同结果） | 不立项 |
| 竞态检测（race detector） | 需要影子内存 + happens-before 图；解释器是单线程，只会看到「交错」不会看到「并发写」 | 不立项 |
| goroutine 栈的按需增长 | 帧深上限 1000，按 G 计（`D-M8-9`）；增长是 native 侧的事 | M11 |
| `perm.go` 这类 `// errorcheck` 语料 | 官方要求它**编不过**；它测的是错误信息，不是运行时 | 拒绝面（M4 已有码） |

### 1.4 退出条件（门禁 `CON-1..9`）

| 门禁 | 名字 | 判定 |
|---|---|---|
| `CON-1` | 无缓冲 handoff | 发送方阻塞到接收方到场；值**深拷贝**（改了发送方的副本，接收方不变）；`cap == 0` |
| `CON-2` | 缓冲 FIFO | 满则阻塞、按出队顺序收；收走一个后阻塞的发送方补位；`len`/`cap` 与 `C9` 一致 |
| `CON-3` | close 三态 | closed 上 recv 立即返回零值 + `ok=false`；`range` 正常结束；closed 上 send ⇒ panic；double-close ⇒ panic；close(nil) ⇒ panic |
| `CON-4` | select 五形态 | 单就绪 / `default` / 全阻塞后由别人唤醒 / `nil` case 永不就绪 / **同一 channel 两个 case**（撤销其余登记） |
| `CON-5` | 死锁与退出 | 死锁 ⇒ `fatal error: all goroutines are asleep - deadlock!` + 退出码 2；`main` 先返回 ⇒ 退出码 0 且**不等** goroutine；goroutine 内未捕获 panic ⇒ 退出码 2；`select {}` ⇒ 死锁 |
| `CON-6` | 并发语料 | `test/chan` 的 **7** 个可跑文件全绿，且**三栏报**（自检 5 / 仅有输出 0 / 空跑 2），输出与官方 `go run` 逐字节一致 |
| `CON-7` | 可回放 | 同 `(源码, seed, 预算)` 两次运行 stdout + stderr + 退出码**逐字节相同**；`seed=1..8` 全部无失败；失败可缩减（§4.2） |
| `CON-8` | range-over-func 窄面 | `yield` 正常返回 / `break` / `continue` / 嵌套；body 内 `return`·`goto` ⇒ 静态码 + 退出码 1 |
| `CON-9` | 三块补齐与零回归 | map 迭代快照口径 / defer 边界 4 条 / **M5 的 69 语料逐字节零回归** / M6 的 `--via=ssa` 46 绿 23 跳过不变 / 四条 cargo 全绿 |

---

## 2. 已确认决策记录

### 2.1 决策一览

| # | 决策 | 一句话 |
|---|---|---|
| `D-M8-1` | 状态切分 = **抽 `G`**，方法挪到 `G` 上，`Shared` 作参数 | 300 处 `self.frames` 一处不改；64 处共享字段 + 约 40 个签名要改 |
| `D-M8-2` | 调度 = **M:1 单线程**，只在阻塞点让出 | 不抢占；`go f()` 只入队尾，不切走 |
| `D-M8-3` | select 多就绪 = **源码顺序**（默认 seed=0），`--sched=seed:N` 打乱 | 官方是「均匀随机」但**禁止依赖**；我们固定一种实现，golden 与 seed 无关 |
| `D-M8-4` | 回放 = `(源码, seed, 预算)` 唯一决定 | 不记事件日志；「缩减」= 对源码做语句级删除再跑 |
| `D-M8-5` | 阻塞 = **任务不弹出** + `Waiter` 双向共享 | `Task::Recv`/`Task::Send` 提交前无副作用，醒来重试同一个任务 |
| `D-M8-6` | 通道值语义 = **双次深拷贝** | 发送时拷进通道，接收时拷出去（与数组赋值同约定） |
| `D-M8-7` | 死锁 = 全员阻塞 + main 未结束 | `Halt::Deadlock`；不新增诊断码，只加 `C9` 一行 |
| `D-M8-8` | `main` 返回 = **立即退出 0** | 不等 goroutine（Go 语义）；未跑完的 goroutine 直接丢弃 |
| `D-M8-9` | 预算 = 步数**全局累计**，帧深**按 G** | 否则 N 个 goroutine 各 1e8 会让预算失效 |
| `D-M8-10` | HIR 只加**六个节点**，不新建 IR | `Stmt::{Go,Send,Select}` / `ExprKind::Recv` / `RangeKind::{Chan,Func}` |
| `D-M8-11` | range-over-func = **窄面**（body 下沉合成函数） | `return`/`goto` 在 body 内静态拒；理由见 §2.2 |
| `D-M8-12` | map 迭代 = **range 开始快照 key 列表** | 删除立刻生效；新增不访问；写进 `C9` |
| `D-M8-13` | **不抢占** | 官方有异步抢占；我们只在阻塞点让出，登记 `U-M8-01` |
| `D-M8-14` | 语料 = **7 进 / 12 拒**，三栏报 | `perm.go` 是 `// errorcheck`；11 个 import stdlib |

### 2.2 五字段（只对**非显然**的六条展开）

#### `D-M8-1` 抽 `G`（而不是「银行切换」）

| 字段 | 内容 |
|---|---|
| **背景** | `Interp` 现在是「一台机器」：5 个 `Vec` 是执行状态，另外 8 个字段是共享状态。要变成 M 台 |
| **选项** | ① 抽 `G`：5 个 `Vec` 进 `G`，方法挪到 `impl G`，`Shared` 作参数；② 银行切换：`Interp` 明面留 5 个字段当「当前 G」，另存 `parked: Vec<Parked>`，切换 = 5 次 `mem::swap`；③ 每 goroutine 一个 OS 线程 + `Condvar` |
| **决定** | ① |
| **理由** | ② 的改动是 0 处，但**加一个 per-goroutine 字段忘了 swap** 就是静默串味——正是 M7 花整个里程碑在抓的那类错（`M7-report.md` §5）。① 的实际代价比想象小：`self.frames`/`self.values`/`self.eval`/`self.pending`/`self.targets` 共 **300 处一处不用改**（它们本来就该是 `self.`），要改的只有 `self.prog|tt|opts|methods|out|steps` 共 **64 处**，加上约 40 个方法的签名；且全部由编译器逐个兜底。③ 直接杀死 `CON-7` |
| **失效条件** | 若借用检查在「`&mut self` + `&mut Shared`」混用处炸出**无法用局部变量拆解**的借用冲突（超过 20 处），说明 `G` 与 `Shared` 的边界划错了——那时把冲突的那几个字段挪进 `Shared`（用 `RefCell`），但**不许**退回② |

#### `D-M8-3` select 多就绪 = 源码顺序

| 字段 | 内容 |
|---|---|
| **背景** | 官方 `STM-15` 写「就绪 case **均匀随机**」；但同一个 spec 也写「程序不得依赖」 |
| **选项** | ① 源码顺序（默认）＋ seed 打乱（可选）；② 总是 PRNG（固定默认 seed）；③ 真随机（`rand` 不固定） |
| **决定** | ① |
| **理由** | ③ 直接违反 `CON-7`。② 表面上更贴 spec，但**golden 会绑死在「默认 seed 是几」**上：改一次默认 seed，所有多就绪 fixture 的期望值全变，而它们本来没有语义差别。① 让「默认行为」与「源码顺序」重合，读起来就是「先写的先选中」；同时 `--sched=seed:N` 仍然能测公平性（`CON-7` 的多 seed 那一半） |
| **登记** | `U-M8-02`：与 `STM-15` 的「均匀随机」**显式不一致**；理由 = 「禁止依赖」给了实现自由，我们选**最可复现**的那一种 |

#### `D-M8-5` 阻塞 = 任务不弹出

| 字段 | 内容 |
|---|---|
| **背景** | `x := <-c` 的停顿发生在**求值栈中间**。M5 的 `drive()` 每一步都「弹一个任务、执行它」 |
| **选项** | ① 阻塞时把任务**留在栈顶**，醒来重新执行；② 新增 `Pending::Blocked`，把「我在等什么、醒来干什么」编码成一个续延；③ 让 `Recv` 变成两段式（先查、再取），阻塞在第二段 |
| **决定** | ① |
| **理由** | ① 只要求 `Task::Recv`/`Task::Send` **在提交前没有副作用**；醒来重跑同一个任务是幂等的。② 是「给每个阻塞点写一个续延类型」——`select` 一进来就要写第 4 个、第 5 个，最后变成一个平行的小解释器。③ 与①等价但要改 `Task` 的形状 |
| **代价** | 必须给 `Task::Recv`/`Task::Send` 一个**明确的「是否已提交」判据**：靠 `Waiter.done`（§3.4），不是靠重试次数 |

#### `D-M8-11` range-over-func 窄面

| 字段 | 内容 |
|---|---|
| **背景** | `for v := range f` 里 `f` 是 `func(yield func(T) bool)`。body 里的 `return` 按 spec 要**返回外层函数**，且**穿过迭代函数的 defer** |
| **选项** | ① body 下沉成合成函数（捕获外层槽，复用闭包机制）；body 内 `return`/`goto` 静态拒；② 帧栈拼接（`yield` 时把 f 的帧段挪到一边，body 跑在调用者帧上）；③ 整体推迟 |
| **决定** | ① |
| **理由** | ① 的 `yield` 就是一次普通调用，**零新机制**；`break`/`continue` 映射成 yield 返回 `false`/`true`。代价是 `return` 表达不了——而在合成函数里 `return` 只能「返回自己」，这不是偷懒，是**下沉这个动作本身**的语义后果，所以静态拒是诚实的。② 能全做，但要新增一种「帧段搬移」状态，且 `return` 仍需一个 `Unwind::Return` 信号穿过 f 的 defer——那是 M8 里最贵的一块。③ 直接不兑现 roadmap 的 M8d |
| **判据** | 仓库里**零 rangefunc 语料**（`F13`）⇒ 窄面的证据只能来自自有 fixture；这条要写进报告，不许说「已支持 range over func」 |

#### `D-M8-12` map 迭代 = 快照

| 字段 | 内容 |
|---|---|
| **背景** | 官方：迭代中新加的 key「可能访问也可能不访问」，删除的 key「保证不访问」 |
| **选项** | ① 开始快照 key 列表；② 每次取下一个都现场扫（`Vec` 活游标）；③ 随机起点（官方的实现自由度） |
| **决定** | ① |
| **理由** | ③ 与 `CON-7` 冲突。② 也能满足「删除立刻生效」，但「新增不访问」这一条要靠额外记账（记住起始长度），且 `Vec` 中间删除后游标要回退——一个 off-by-one 就会漏 key。① 是**两行代码 + 一个不可变快照**，`delete` 时目标不在 map 里就跳过 |
| **写进 `C9`** | 「map 迭代固定为快照语义（官方不承诺）；同一程序两次运行顺序相同」——比官方**更强**，用于可复现 |

#### `D-M8-13` 不抢占

| 字段 | 内容 |
|---|---|
| **背景** | 官方运行时有异步抢占；`for {}` 在 goroutine 里也会被切走 |
| **选项** | ① 只在阻塞点让出；② 每 N 步（或每条回边）检查一次「该换人了吗」 |
| **决定** | ① |
| **理由** | ② 会让 `(源码, seed, 预算)` **不再唯一决定执行**——`N` 也是输入，而它一旦变化，所有 golden 都变。① 的代价是「`for {}` 在 main 里跑不完 ⇒ 预算超限退出 2」，那正是 `C9` §6 已有的行为（与 `R-36-1` 同一类） |
| **登记** | `U-M8-01` |

### 2.3 事实基线（`F1..F16`，全部实测或源码指认）

| # | 事实 | 来源 |
|---|---|---|
| `F1` | **`check` 已经放行并发**：`make(chan int,3)` / `go gen(5,c)` / `for v := range c` / `select{...}` / `close(c)` / `len(c)`/`cap(c)` 全部 **0 诊断** | 容器内 `rgoc-driver check` 实测（2026-10-25） |
| `F2` | **`check` 有一个真 bug**：`v, ok := <-c` 报 `CHK-INVALID-ASSIGN`（`assignment mismatch: 2 variables but 1 value`），而它是合法 Go（`perm.go` 里标了 `ok`） | 同上 + `go_source_code/test/chan/perm.go:27` |
| `F3` | **方向检查是对的**：`<-chan int` 上发送 ⇒ `CHK-INVALID-OPERATION`（`cannot send to non-channel`） | 同一批实测 |
| `F4` | **拒绝点全在 `rgoc-hir/src/lower.rs`**：`Go`(939) / send(946,1344) / `Select`(950) / recv(1772) / `Close`(2046) / `RangeFunc`(1491)；码是 `HIR-UNSUPPORTED-{GO,CHAN,SELECT,CLOSE,RANGE-FUNC}` | 源码指认 |
| `F5` | **HIR 已经有 `MakeKind::Chan`**（注释「M5 不出」）与 `BuiltinKind::Close`（注释「lowering 不产出」）；`RangeKind`（`types.rs:299`）只有 `Array/Slice/Str/Map/Int`，**没有 Chan/Func** | `rgoc-hir/src/types.rs:671`(MakeKind) / `:677`(Chan) / `:682`(BuiltinKind) / `:299`(RangeKind) |
| `F6` | 解释器的 per-goroutine 状态共 **300 处** `self.xxx`：frames 96 / values 87 / eval 81 / pending 19 / targets 17 | `grep -c` |
| `F7` | 共享状态 **64 处**：`prog|tt|opts|methods` 53 / `out` 7 / `steps` 4 | 同上 |
| `F8` | `drive()` 是单循环，「帧在退出 → 求值 → pending → 语句 → 顶层」五段；`TopPhase = Globals/Inits/Main/Done` | `interp.rs:496` |
| `F9` | **69 个冻结语料里 0 个用并发**（`turing.go` 的 `<-` 是 Brainfuck 字符串字面量里的） | 逐文件扫 |
| `F10` | `test/chan` **19** 个：**8** 个 import-free（`perm`/`select`/`select4`/`select6`/`select8`/`sendstmt`/`sieve1`/`zerosize`），**11** 个 import stdlib（`flag`/`os`/`time`/`bufio`/`fmt`/`container/*`） | 逐个看 import |
| `F11` | 8 个里 `perm.go` **官方 `go run` 也编不过**（`// errorcheck`，测的是错误信息）⇒ 可跑 **7** | 容器内 `go run` 实测 |
| `F12` | 那 7 个官方跑完 **stdout 全是 0 字节**；但 **5 个自带断言**（`select` 4 个 panic、`select6` 1、`select8` 7、`sendstmt` 3、`sieve1` 1）；不自检的 2 个是 `select4`（两行 println）与 `zerosize`（空） | `grep -c 'panic('` + `go run` |
| `F13` | 仓库里**没有任何 rangefunc 语料**（`func(yield` 零命中） | 全仓扫 |
| `F14` | `Frame` 已有 `saved_eval/saved_values/saved_targets/saved_pending`（调用时保存外层求值状态）⇒ 「求值栈可嵌套」已有先例，切 goroutine 不需要它 | `interp.rs:414` |
| `F15` | defer/panic/recover **已在 M5 实现**（`Deferred`/`unwinding`/`is_defer`/`drop_result`） | `interp.rs:380,409,411,413` |
| `F16` | `Halt` 只有 `Panic` / `Budget` 两个变体（M8 加第三个 `Deadlock`）；`C9` §5 的退出码只有 0/1/2 三档 | `interp.rs:94`、`C9` §5 |

### 2.4 四条签字（2026-10-25 已确认）

| # | 问题 | 决定 | 理由（用户选定） |
|---|---|---|---|
| `Q1` | goroutine 的状态怎么切 | **抽 `G` 结构** | 银行切换的「加字段忘 swap」是静默串味，正是 M7 在抓的那类错 |
| `Q2` | 调度语义与可回放口径 | **确定性 FIFO + seed 只选 select** | `(源码, seed, 预算)` 唯一决定执行，不需要事件日志 |
| `Q3` | M8d 三块补齐切多宽 | **range-over-func 先做窄面** | 零语料 + `return` 穿越是最贵的一块，没有外部证据 |
| `Q4` | 并发语料口径 | **8 个 import-free + 自有 fixture**（签约时按 8，实测**修正为 7**，见 `F11`） | 给 stdlib 做桩会吞掉整个工期 |

> **`Q4` 的口径修正**：签字时算的是 8（`perm` 也算进去了）。实测发现 `perm.go` 是
> `// errorcheck` 文件——官方自己都编不过。所以 **7 进 / 12 拒**。这条偏差记在
> `M8-manifest.json` 的 `measurement_deviations`，不静默改数。

---

## 3. 架构与数据模型

### 3.1 crate 与模块变化

| crate | 变化 | 判据 |
|---|---|---|
| `rgoc-hir` | `types.rs` 加 6 个节点（`D-M8-10`）；`lower.rs` 删 5 处 `reject`、加 4 个 lowering 函数；`diag.rs` 加 1 个码（`RangeFuncReturn`） | `CON-8` 的静态半边 |
| `rgoc-interp` | 新模块 `sched.rs`（`Shared`）、`chan.rs`（`ChanObj`/`Waiter`）；`interp.rs` 的 `G` 与 40 个方法加 `sh` 参数 | Phase 0 的重构判据：`CON-9` 的零回归 |
| `rgoc-types` | **不动**（`Chan` 类型与 8/8 布局已有） | — |
| `rgoc-check` | 修 `F2`（`v, ok := <-c` 的赋值数判定） | 新增 2 条 check 测试 |
| `rgoc-driver` | `run` 加 `--sched=seed:N`；新装置脚本 | `CON-7` |
| `rgoc-arm64` / `rgoc-ssa` | **不动**（SSA 继续 `SSA-UNSUPPORTED-CHAN`） | `CON-9` 的「M6 零回归」 |

### 3.2 状态切分（`D-M8-1`）

```rust
pub struct Shared<'a> {
    prog: &'a Program, tt: &'a TypeTable, opts: Options,
    globals: Vec<Cell>,                       // 共享：全局变量
    out: String,                              // 共享：合并输出流（C9 §4.1）
    steps: u64,                               // 共享：全局累计（D-M8-9）
    halted: Option<Halt>,
    methods: BTreeMap<(String, String), FuncId>,
    chans: Vec<ChanObj>,                      // 共享：所有通道
    runq: VecDeque<Gid>,
    gs: Vec<Option<G<'a>>>,
    next_g: u64,                              // goid 递增
    rng: u64,                                 // seed（D-M8-3）
}

struct G<'a> {
    id: u64,
    frames: Vec<Frame<'a>>,
    eval: Vec<Task<'a>>,
    values: Vec<Value>,
    targets: Vec<Target>,
    pending: Option<Pending<'a>>,
    phase: TopPhase,                          // 只有 main 用；新 G 直接 Done
    blocked: Option<Block>,                   // 在等什么（D-M8-5）
    is_defer: bool,                           // 沿用：recover 只在 defer 里有效
}

struct Block {
    reason: BlockReason,                      // Recv / Send / Select
    waiters: Vec<Rc<RefCell<Waiter>>>,        // 我在哪些队列上登记了
}
```

**与切分一起新增/改动的类型**（这些是 §3.4–§3.6 的前提，缺一个那几节就读不通）

| 类型 | 变化 |
|---|---|
| `Task`（求值栈元素） | **加三个变体**：`Recv { ch, comma_ok, waiter }` / `Send { ch, value, waiter }` / `Select { n, chosen }` |
| `Step` | **新增**：`Continue` / `Blocked` / `Exited` —— `one_step` 的返回值 |
| `Halt` | **加 `Deadlock`**（原有的 `Panic` / `Budget` 不动） |
| `Gid` | `u64` 别名；`go` 创建时 `next_g += 1`，**只增不减**（便于日志与断言） |

**迁移步骤（Phase 0 的 Task 顺序）**

1. 新建 `Shared` / `G`，字段搬过去；`Interp` 这个类型**消失** ——
   `pub fn run(prog, tt, opts) -> Outcome` 的**签名不变**（内部构造 `Shared` 再 `drive()`），
   所以 `rgoc-driver` 一行都不用改；
2. 把 `impl Interp` 的方法逐个挪到 `impl G`，第一个参数加 `sh: &mut Shared<'a>`；
3. `drive()` 挪到 `impl Shared`（它要同时看队列和所有 G），改成「取一个可运行的 G →
   推进它 → 放回去」（§3.6）；
4. `finish()` 也挪到 `impl Shared`（它只读共享面）。

**判据**：Phase 0 **结束时** `cargo test --workspace` 必须 **1974 passed / 0 failed / 1 ignored**，
且**一个测试都不许改**。改了就说明重构动了语义。

### 3.3 通道对象（`D-M8-6`）

```rust
pub struct ChanObj {
    elem: TypeId,
    cap: usize,
    buf: VecDeque<Value>,          // 已满 ⇒ 长度 == cap
    closed: bool,
    sendq: VecDeque<Rc<RefCell<Waiter>>>,
    recvq: VecDeque<Rc<RefCell<Waiter>>>,
}
pub struct Waiter {
    g: Gid,
    token: u64,                    // 同一个 select 的所有登记共用一个 token
    slot: Option<Value>,           // 阻塞发送方的值 / 阻塞接收方的空位
    done: bool,                    // 已被对端完成（D-M8-5 的幂等判据）
}
```

**四个操作的真值表**（逐条照 `runtime/chan.go` 的顺序，不是凭直觉）：

| 操作 | 条件（按顺序判） | 动作 |
|---|---|---|
| **recv** | ① `buf` 非空 | 从 `buf` 取；若 `sendq` 非空则把一个发送方的值搬进 `buf` 并唤醒它 |
| | ② `sendq` 非空 | **直接接收**发送方的 `slot`，标记 `done`，唤醒它 |
| | ③ `closed` | 返回**零值** + `ok = false` |
| | ④ 其余 | 登记 waiter 到 `recvq`，阻塞 |
| **send** | ① `recvq` 非空 | **直接**把值放进接收方的 `slot`，标记 `done`，唤醒它 |
| | ② `cap > 0` 且 `buf` 未满 | 入 `buf`，完成 |
| | ③ `closed` | `panic: send on closed channel` |
| | ④ 其余 | 登记 waiter（`slot = Some(v)`）到 `sendq`，阻塞 |
| **close** | `closed` 已为真 | `panic: close of closed channel`（在**调用 close 的那个 G** 里） |
| | 其余 | `closed = true`；唤醒 `recvq` **全部**（它们重试时按 ①→③ 先排空 `buf`，排空后才拿零值 + `ok=false`）；唤醒 `sendq` **全部**（它们重试时看到 `closed` ⇒ 在**自己**的 G 里 `panic: send on closed channel`） |
| **len/cap** | — | `len = buf.len()`；`cap = cap`（无缓冲 = 0） |

**值语义**：`send` 时 `deep_copy` 一份进 `slot`/`buf`；`recv` 时再 `deep_copy` 一次
交给接收方。两次拷贝看起来多余，但 Go 的语义就是「发送时复制、接收时又是一份独立值」——
少一次拷贝，`ch <- a` 之后改 `a[0]` 会改到接收方看到的东西。

### 3.4 阻塞与唤醒（`D-M8-5`）

```text
阻塞：Task::Recv 留在求值栈顶 → 登记 waiter → G.blocked = Some(..) → 返回 Step::Blocked
唤醒：对端填 waiter.slot + waiter.done = true + runq.push_back(gid)
重试：同一个 Task::Recv 再执行一次 → 看到 done → 取 slot → 弹任务
```

三条必须同时成立，否则会静默错：

1. **提交前无副作用**：`Task::Recv`/`Task::Send` 在 `done == false` 时**不许**动值栈；
2. **`done` 是唯一判据**：不许用「重试第几次」判断，否则 select 的重复登记会错；
3. **撤销是幂等的**：从队列里删一个已经被对端拿走的 waiter，必须是**无害**的
   （`retain(|w| w.borrow().token != t)`，找不到就算了）。

### 3.5 select（`D-M8-3` + `D-M8-5`）

三阶段，与 Go 的 `selectgo` 同构：

| 阶段 | 做什么 | 坑 |
|---|---|---|
| **① 求值** | 所有 case 的通道表达式 + 所有 send 的右值，**按源码顺序各求值一次** | 选中的是 `default` 时，其他 case 的通道表达式**也已经求过值**（spec 明确） |
| **② 试探** | 按源码顺序扫一遍：`nil` 通道跳过；`default` 记「有」；就绪的 case 记「可做」 | 有多个「可做」⇒ 按 `D-M8-3` 取第一个（默认）或 PRNG（seed≠0） |
| **③ 阻塞** | 都没有 ⇒ 若有 `default` 直接走它；否则给**每个非 nil 的 case** 登记一个 waiter（共用 `token`），阻塞 | 被任一通道唤醒后：拿 `token` 把所有队列里的自己删掉，再按「哪个 waiter 的 `done` 为真」定 winner |

**重复 channel**（`case <-c:` 与 `case v := <-c:` 同时存在）：两个 waiter 各自独立登记，
被唤醒时**只有一个** `done`，另一个靠 token 撤掉——这就是 M8c 说的「取消其他候选等待者」。

**空 select**（`select {}`）：没有 case ⇒ 阻塞且没有任何队列可登记 ⇒ 立即判死锁（`CON-5`）。

### 3.6 调度器（`D-M8-2` / `D-M8-9` / `D-M8-13`）

```text
loop {
    sh.steps += 1;  预算检查（步数 / 求值栈深）
    if halted { return }

    let gid = match sh.pick() {          // 当前 G 继续跑；它阻塞/退出后才换人
        Some(g) => g,
        None => {
            if sh.main_done() { return }              // 正常结束（D-M8-8）
            if sh.all_blocked() { halt(Deadlock) }    // CON-5
            unreachable
        }
    };
    let mut g = sh.gs[gid].take().unwrap();
    let step = g.one_step(&mut sh);      // 就是原来的五段推进，一次一步
    match step {
        Continue => sh.gs[gid] = Some(g),
        Blocked  => sh.park(gid, g),
        Exited   => sh.reap(gid),        // 丢弃；defer 已在退出前跑完
    }
}
```

| 规则 | 内容 |
|---|---|
| 何时换人 | **只在** `Blocked` / `Exited` 时；`go f()` **不切走**（只入队尾） |
| 就绪顺序 | FIFO；`go` 创建的顺序、唤醒的顺序都追加到队尾 |
| 唤醒顺序 | 通道队列的 FIFO（先阻塞的先被满足）⇒ 单就绪下的交错固定 |
| 步数 | **全局累计**（`sh.steps`）；一个 goroutine 烧完预算，全体一起停 |
| 帧深 | 每个 G 看自己的 `frames.len()`（`D-M8-9`） |
| `false` 唤醒 | 唤醒只是「可运行」，不保证它马上跑（可能先跑别人） |

### 3.7 死锁与退出（`D-M8-7` / `D-M8-8`）

| 情形 | 判据 | 行为 |
|---|---|---|
| `main` 正常返回 | main 的 `frames` 空且 `phase == Done` | **立即**退出 0，其余 G 丢弃 |
| 死锁 | `runq` 空 ∧ 存在 `blocked` 的 G ∧ main 未结束 | stderr `fatal error: all goroutines are asleep - deadlock!\n` + 退出码 2 |
| goroutine 内未捕获 panic | panic 值没被 recover | 整个程序退出 2，`panic: <值>` 进合并流（`C9` §4.5） |
| `close` 后 send | 见 §3.3 | `panic: send on closed channel` + 退出码 2 |
| double close | 同上 | `panic: close of closed channel` + 退出码 2 |
| nil 通道 send/recv | Go 语义：**永久阻塞**（不是 panic） | 走死锁判定 |
| 预算超限 | `steps > max_steps` | 沿用 `Halt::Budget` + 退出码 2 |

**不变式（DEBUG 下每 N 步可选校验，`CON-5` 的牙）**

```text
∀ g ∈ gs:  g.blocked = Some(b) ⇒ ∀ w ∈ b.waiters:
           w 恰好出现在某个队列里 或 w.done == true
∀ w ∈ any queue:  gs[w.g] 存在且 blocked 含 w
```

第一条抓「唤醒时漏删」（会导致同一 waiter 被两个人完成），第二条抓「删了但没唤醒」
（会导致假死锁）。这两类错在单测里都不会露头，只会在特定交错下出现。

### 3.8 HIR 增补（`D-M8-10`）

```rust
pub enum Stmt {
    // …原有 16 个…
    Go { call: Expr },                                  // go f(x) / go x.M(y)
    Send { chan: Expr, value: Expr },                   // ch <- v
    Select { clauses: Vec<SelectClause> },
}
pub enum ExprKind { /* …原有… */ Recv { ch: Box<Expr>, comma_ok: bool } }

pub struct SelectClause {
    pub kind: SelectKind,       // Send | Recv | Default
    pub chan: Option<Expr>,     // Recv/Send 必有；Default 为 None
    pub recv_targets: Vec<Place>,   // 0=丢弃 / 1=case v := <-c / 2=case v, ok := <-c
                                    // （`R2`：`Option<Place>` 表达不了两个目标）
    pub send_value: Option<Expr>,
    pub body: Block,
}
pub enum RangeKind { /* …原有 5 个… */ Chan, Func }
```

**求值顺序（写进 `C3`）**

| 构造 | 求值发生在 | 说明 |
|---|---|---|
| `go f(a, b)` | **当前** goroutine：先算 `f`、再算 `a`、`b`，然后才创建新 goroutine | `STM-14` 原文；`go f(g())` 里 `g()` 在旧 goroutine 跑 |
| `ch <- v` | 当前 goroutine：先算 `ch`，再算 `v`，再阻塞 | — |
| `select` | 当前 goroutine：所有 case 的通道表达式按源码顺序各算一次 | `STM-15`；`default` 也要算别的 case |
| `<-ch` | 当前 goroutine | 与 `Index`/`Slice` 同级 |

**下线的码**（5 个）：`HIR-UNSUPPORTED-GO` / `-CHAN` / `-SELECT` / `-CLOSE` / `-RANGE-FUNC`。
**新上加的码**（1 个）：`HIR-UNSUPPORTED-RANGEFUNC-RETURN`（body 内含 `return`/`goto`）。

> **码表的纪律**（`C8` 的 26 类冻结）：`RejectKind` 是 crate-local 枚举，**不进 `DiagCode`**，
> 沿 `SsaCode` / `CgCode` 先例。下线 5 个、加 1 个 ⇒ 净减 4 个，`DiagCode` 仍是 103。

### 3.9 range-over-func 窄面（`D-M8-11`）

```text
for v := range seq {  body  }
  ⇒  合成一个函数 __range_body(v T, ...捕获的槽...) bool { body; return true }
     seq(func(v T) bool { return __range_body(v) })
```

| 形态 | 结果 |
|---|---|
| `yield(x)` 正常返回 | 合成函数返回 `true` ⇒ 迭代继续 |
| body 内 `break` | 合成函数返回 `false` ⇒ 迭代函数应当停止 |
| body 内 `continue` | 同正常返回（`true`） |
| body 内 `return` / `goto` | **lowering 期拒绝**（`HIR-UNSUPPORTED-RANGEFUNC-RETURN`），退出码 1 |
| body 内 `panic` / `recover` | 沿用 M5 的展开机制（合成函数是普通函数，defer 照常跑） |
| `seq2`（`func(yield func(K,V) bool)`） | 支持：两个参数的 yield |

**为什么 `goto` 也拒**：`goto` 目标可能在 body 外（跨函数的跳转在语法上就不存在），
留在合成函数里跳只能在 body 内跳——那反而**能支持**。但 `goto` 跳出会绕过 yield 的
「返回 false」契约，语义边界不清，先拒。这条写进白名单 `U-M8-03`。

### 3.10 map 迭代（`D-M8-12`）

| 动作 | 语义 |
|---|---|
| 进入 `range m` | 快照 `keys: Vec<Value>`（当前插入序） |
| 遍历中 `delete(m, k)` | 立刻生效；轮到该 key 时**跳过**（不产生迭代） |
| 遍历中 `m[k] = v`（新增） | **不访问**（快照里没有） |
| 遍历中 `m[k] = v`（已存在） | 值更新可见（读的是当前 map），迭代次数不变 |
| `clear(m)` / 重新赋值 | 与删除同：后续 key 全跳过 |

写进 `C9` 的新一节（§7.5 附近），并注明**比官方更强**（官方不承诺顺序）。

### 3.11 defer / panic 跨 goroutine（`D-M8-13` 的邻居）

| 场景 | 语义 | 判据 |
|---|---|---|
| goroutine 正常退出 | 跑它自己的 defer（`Frame.deferred` 已按栈序） | 新 fixture |
| goroutine 里 panic 未捕获 | **整个程序**退出 2；其它 goroutine 的 defer **不跑** | `CON-5` |
| goroutine 里 `recover` | 只在自己的 defer 里有效（`is_defer` 已有） | 沿用 `T-EX-36` |
| 阻塞中的 G 被丢弃（main 先返回） | 它的 defer **不跑**（Go 语义：进程退出） | `CON-5` |
| panic 值穿过 `go` 边界 | 不做「包装成新 panic」——直接终止 | `U-M8-04` 之外，登记在 `C11` |

M5 遗留的两条（`T-EX-65` defer 时机、`T-EX-66` recover 放宽）在 M8d 一并补判据，
**不改实现**——M5 的实现已经是「按帧登记、退出时倒序跑」，缺的是判据不是代码。

---

## 4. 判据与装置

### 4.1 语料三栏（`D-M8-14`）

| 栏 | 数量 | 文件 | 这一栏能证明什么 |
|---|---|---|---|
| **自检**（算错就 panic） | **5** | `select` / `select6` / `select8` / `sendstmt` / `sieve1` | 退出码 0 = **算对了**（`sieve1` 用链式 goroutine 验 25 个素数） |
| **仅有输出** | **0** | — | — |
| **空跑** | **2** | `select4` / `zerosize` | 只证明「不崩、不死锁、退出码 0」 |

> **这条分栏是判据的牙**。M7 的教训（`M7-report.md` §5）是「42/46 是空输出，所以
> 『全绿』可能只证明没崩」。M8 的语料**输出也是空的**，但 5/7 自带断言 —— 所以
> 「退出码 0」在这一批里比 M7 强一档。**不许**把空跑那 2 个写成「算对了」。

**拒绝面**（12 个，逐行三栏进 `tools/run-oracle/chan-rejected.txt`）：

| 类别 | 数量 | 文件 | 理由 |
|---|---|---|---|
| `errorcheck` | 1 | `perm` | 官方要求它编不过；测的是错误信息，不是运行时 |
| `stdlib` | 11 | `doubleselect`(flag,runtime) / `fifo`(os) / `goroutines`(os,strconv) / `nonblock`(runtime,time,+本地包) / `powser1`(os) / `powser2`(os) / `select2`(runtime,dummy) / `select3`(time) / `select5`(bufio,fmt,io,os,template) / `select7`(runtime) / `sieve2`(container/heap,ring) | 标准库 import 不在解释器支持面（`U-M5-01`） |

两向断言：`N' + M' == 19`；`M'` 集合 == 冻结文件；每条码 ∈ 拒绝码集合。

### 4.2 可回放（`D-M8-4`）

| 判据 | 做法 |
|---|---|
| 同输入两次 | 同 `(源码, seed, 预算)` 跑两遍，`stdout + stderr + 退出码` 逐字节相同 |
| 多 seed | `seed = 1..8` 各跑一遍，全部退出码 0（语料）或与期望一致（fixture） |
| 缩减 | 失败时**删语句**（从后往前，一次删一条，仍失败就保留），产出最小复现；不给自动工具，先给手工流程 |

**为什么不做事件日志**：日志要序列化「每次选了哪个 G / 哪个 case」，那是一个新的
文件格式 + 一套版本兼容；而确定性 FIFO 已经让「同 seed 同结果」**天然成立**，
日志只能证明「同 seed 同结果」这件事——冗余。代价是：无法回放「用不同调度器跑过的
那一次」。这个代价明确写在 `C11` 里，不假装没有。

### 4.3 不变式自检（`CON-5` 的牙）

`Shared` 提供 `check_invariants()`，在 `RGOC_SCHED_DEBUG=1` 时每步调用：

| 不变式 | 抓什么 |
|---|---|
| 每个 blocked G 的每个 waiter 要么在某个队列里，要么 `done` | 唤醒漏删（同一 waiter 被两人完成） |
| 队列里每个 waiter 的 G 存在且 blocked 含它 | 删了但没唤醒（假死锁） |
| 每个非 dead 的 G 恰好处于 runnable / blocked / running 之一 | 状态机漏洞 |
| `buf.len() <= cap` | 缓冲越界 |

**这些不变式本身要有变异验证**（故意破坏一处，必须红）——否则就是「恒真的自检」。

### 4.4 预算、资源与冻结

| 项 | 值 | 说明 |
|---|---|---|
| 步数 | 全局 1e8（沿用） | 跨 goroutine 累计；单 G 烧完即全体停 |
| 帧深 | 每 G 1000（沿用） | 按 G 计，不是全局 |
| 求值栈 | 每 G 10000（沿用） | 同上 |
| goroutine 数 | **10 000**（新上限，可配） | 超限 ⇒ `Halt::Budget("goroutine limit exceeded")` + 退出码 2；不加会 OOM |
| 通道数 / 缓冲总量 | 不设上限 | 与解释器的 map/slice 同口径（内存靠宿主） |
| 冻结纪律 | `R-6-2` 同源 | 冻结后只修「已声明语义」与「差分阻塞 bug」；新增行为必须显式扩基线 |

---

## 5. 不支持面（白名单，8 条）

| 码 / 项 | 不支持什么 | 触发条件 | 行为 | 语料/守卫 |
|---|---|---|---|---|
| `U-M8-01` | **抢占** | 无（不存在入口） | 只能在阻塞点让出 | 守卫测试：`for {}` 在 goroutine 里 ⇒ 预算超限退出 2，且**不**切走 |
| `U-M8-02` | select 的**均匀随机** | 多就绪时 | 取源码顺序（`D-M8-3`） | 守卫测试：同一程序跑 8 次选中同一个 case |
| `U-M8-03` | range-over-func body 内 **`return`/`goto`** | lowering 见到 | `HIR-UNSUPPORTED-RANGEFUNC-RETURN` + 退出码 1 | 2 条 fixture |
| `U-M8-04` | 11 个 import stdlib 的 chan 语料 | 静态 import | 不在 `N'` 里 | `chan-rejected.txt` |
| `U-M8-05` | 定时器 / `time.Sleep` / select 超时 | 无 stdlib | 编译期就该 import 失败 | `U-M5-01` 已覆盖 |
| `U-M8-06` | goroutine 栈按需增长 | 无 | 帧深 1000 上限，超了退出 2 | 递归 fixture 沿用 `EX-7` |
| `U-M8-07` | 竞态检测 | 无 | 不提供 `-race` | 不适用（单线程） |
| `U-M8-08` | `panic` 穿过 `go` 边界的**包装语义** | goroutine 内未捕获 | 直接终止程序（不包装成新 panic） | `CON-5` |

**纪律**：白名单项**不得**记为 pass；每条都要有「一个 fixture + 一个行为 + 一个退出码/码」。
`U-M8-01/02` 的「不存在入口」是**守卫测试**：断言当前行为，行为一变就红。

---

## 6. 错误处理

### 6.1 诊断（静态面）

| 情形 | 码 | 退出码 |
|---|---|---|
| range-over-func body 内 `return`/`goto` | `HIR-UNSUPPORTED-RANGEFUNC-RETURN` | 1 |
| `v, ok := <-c`（`F2` 的 bug 修复后**不再报**） | — | — |
| 其余并发语法 | **不再有码**（5 条下线） | — |

### 6.2 运行期

| 情形 | 输出（合并流） | 退出码 |
|---|---|---|
| 死锁 | stderr: `fatal error: all goroutines are asleep - deadlock!\n` | 2 |
| closed 上 send | `panic: send on closed channel\n` | 2 |
| double close | `panic: close of closed channel\n` | 2 |
| close(nil) | `panic: close of nil channel\n` | 2 |
| goroutine 内未捕获 panic | `panic: <值>\n`（同 `C9` §4.5） | 2 |
| goroutine 数超限 | stderr: `goroutine limit exceeded\n` | 2 |

**1 与 2 的分界不变**（`C9` §5）：`1` = 没跑（静态拒绝），`2` = 跑了但没跑完。
所以「死锁」是 `2`，不是新码——它是「跑了但没跑完」的一个特例。

---

## 7. 覆盖清单（roadmap §M8 ↔ 功能 ID）

| 功能 ID | 名字 | M8 做到什么 |
|---|---|---|
| `RT-SCH-01` | 协程控制块 | ✅ 解释器形态：`G { id, frames, eval, values, targets, pending, blocked }`（不是 Go 的 `g` 结构） |
| `RT-SCH-02` | 有栈上下文切换（汇编） | ✗ **不做**：显式栈机不需要保存寄存器/SP/PC |
| `RT-SCH-03` | GMP 模型 | ✗ **不做**：M:1 单队列，没有 P、没有 work-stealing |
| `RT-SCH-04` | `go` 语句实现 | ✅ 实参在旧 G 求值，新 G 入队尾 |
| `RT-SCH-05` | 阻塞点调度 | ✅ `park`/`ready` + `Waiter` 双向共享 |
| `RT-SCH-06` | 生命周期与调度循环 | ✅ `main` 返回即结束；`Halt::Deadlock` |
| `RT-SCH-07` | 抢占与安全点 | ✗ **不做**（`U-M8-01`） |
| `RT-SCH-08` | 定时器子系统 | ✗ **不做**（无 stdlib） |
| `RT-EXC-01` | defer 链表管理 | ✅ 已有（`Frame.deferred`），M8 补跨 goroutine 判据 |
| `RT-EXC-02` | panic 级联展开 | ✅ 已有（`unwinding`）；M8 加「goroutine 内 panic 终止全程序」 |
| `RT-EXC-03` | recover 拦截器 | ✅ 已有（`is_defer`）；M8 补 `T-EX-66` 的判据 |
| `RT-EXC-04` | 运行时致命错误 | ◐ 只有死锁一种 fatal；**不做**并发 map 写检测（单线程线性化） |
| `RT-EXC-05` | 栈展开与 traceback | ✗ 不做（`U-M5-03` 已登记，M8 不改） |
| `STM-05` | 发送语句 | ✅ |
| `STM-13` | `for range` 语义 | ◐ chan ✅ / 迭代器函数 ✅（窄面）/ map 快照 ✅ / 其余沿用 M5 |
| `STM-14` | `go` 语句 | ✅ |
| `STM-15` | `select` 语句 | ✅ 五形态；均匀随机 ✗（`U-M8-02`） |
| `BIF-04` | `close` | ✅ 三态 + 两个 panic |
| `BIF-08` | `len`/`cap` | ✅ chan 的 len/cap |
| `BIF-09` | `make` | ✅ `MakeKind::Chan` 从「不产出」变「产出」 |
| `PIPE-16b` | range over func 降级 | ◐ 窄面（`U-M8-03`） |

---

## 8. 风险登记（前三条是最可能的坑）

| # | 风险 | 为什么它是真的风险 | 缓解 |
|---|---|---|---|
| `R1` | **并发语料的判据弱** | 7 个里 2 个（`select4`/`zerosize`）既无输出也无断言，退出码 0 只证明不崩（`F12`） | 强判据全压自有 fixture：`CON-1..5/8` 每条都要「带真输出或自带断言」；报告里三栏分开写 |
| `R2` | **拆分 `Interp` 时借用检查炸一片** | 40 个方法要同时拿 `&mut G` 与 `&mut Shared`，而 `self.prog` 这类访问散布在每个方法里 | Phase 0 单独做、单独 commit，判据是「1974 条测试**零改动**全绿」；炸超过 20 处就按 `D-M8-1` 的失效条件处理 |
| `R3` | **死锁假阳性/假阴性** | 「等待集合」与「通道队列」两处记账，任何一处漏就是假死锁（把活程序判死）或永不判死（挂到预算） | §3.4 的三条硬约束 + §4.3 的四条不变式 + **对不变式做变异验证** |
| `R4` | **select 的三个经典坑** | 重复 channel、nil channel、`default` 与阻塞 case 共存时的求值顺序 | `CON-4` 五种形态各一条；「`default` 也要求值其他 case」单独一条 |
| `R5` | **预算语义变化被忽略** | 步数从「每个程序 1e8」变成「所有 goroutine 合计 1e8」，`divmod.go` 这类超限文件的行为会变 | `CON-9` 要求 69 语料逐字节零回归；超限文件本来就在 `run-excluded.txt` |
| `R6` | **range-over-func 窄面被误解成「已支持」** | 零语料（`F13`），判据全在自有 fixture 上 | 报告与 `C11` 都写「窄面」；`U-M8-03` 是可见的拒绝码，不是静默降级 |

---

## 9. 任务分解原则（`M8-plan.md` 的输入）

七个 Phase，共 **约 58–66 个 Task**：

| Phase | Task 区间 | 内容 | 收口判据 |
|---|---|---|---|
| **0** | 1–6 | `C11` 契约骨架、`rgoc-hir` 六节点（只加类型，不 lowering）、`Shared`/`G` 纯重构、调度循环空壳、`run` 的 `--sched=seed:N` 参数 | 1974 条测试**零改动**全绿 |
| **1** | 7–16 | lowering：`go` / send / recv / `make(chan)` / `close` / `len`·`cap` / `range` over chan；**单 goroutine** 语义（没有调度器也能跑通 `go f()` 顺序执行） | `CON-1` 的「无竞争」半边 |
| **2** | 17–28 | 通道对象（四操作真值表）、`Waiter`、park/ready、M:1 队列、goroutine 创建/退出 | `CON-1/2` |
| **3** | 29–38 | select 三阶段、token 撤销、nil/重复/default | `CON-4` |
| **4** | 39–44 | 死锁判定、四情形退出、三个 panic、不变式自检 | `CON-3/5` |
| **5** | 45–52 | range-over-func 窄面、map 快照、defer 边界、`F2` 的 check 修复 | `CON-8` |
| **6** | 53–60 | `test/chan` 对撞装置、7 进 12 拒冻结、三栏报告、白名单、manifest、变异 | `CON-6/7` |
| **7** | 61–66 | 四条 cargo、M5/M6/M7 零回归、冻结报告、文档回写、全量回归 | `CON-9` |

**两条纪律（写进 Global Constraints）**

1. **`Stmt`/`ExprKind`/`RangeKind` 的 `match` 不许通配** —— 加变体就编译失败，
   逼你在每一处决定它的位置（M7 的同一条纪律）。
2. **`ChanObj` 的四个操作只许有一处实现** —— `send`/`recv` 的「直接递送 vs 入缓冲」
   分叉写两遍，就是 M7 `pass_args` 那类「两边各写一遍、错位才现形」的错。

---

## 10. 契约与产物影响

| 契约 | 动作 | 内容 |
|---|---|---|
| **`C11-concurrency.md`** | **新建** | §1 调度（M:1 / 不抢占 / FIFO / seed） §2 通道状态机（四操作真值表） §3 select（三阶段 + 撤销 + `U-M8-02`） §4 死锁与退出 §5 不承诺（无事件日志 / 无抢占 / 无 `-race`） |
| **`C3-hir.md`** | 增补 | 六个新节点的形状 + 求值顺序表（§3.8） + 「`go` 的实参在旧 goroutine 求值」 |
| **`C9-values-and-output.md`** | 增补 + 升正式 | §5 加「死锁」「goroutine 内 panic」两行；新增「map 迭代 = 快照」；**去掉「冻结候选」字样**（M8e 的冻结动作） |
| `C4` / `C10` | **不改** | SSA 侧继续 `SSA-UNSUPPORTED-CHAN`；写明理由（M6 的 IR 没有「等待」概念） |
| `C8` | 不改 | `DiagCode` 仍 103；`RejectKind` 是 crate-local，净减 4 个 |

**产物**：`docs/milestones/M8-{design,tests,plan}.md` + `M8-manifest.json` + `M8-report.md`（含冻结报告）
+ `tools/run-oracle/{run-chan.sh,chan-input.txt,chan-rejected.txt}` + `scripts/mutation-m8.sh`。

---

## 11. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-25 | **初稿并确认**：`D-M8-1..14`、`F1..F16`、`CON-1..9`、白名单 8 条、风险 6 条；`Q1..Q4` 全部按建议签字 | 容器内实测（`F1`–`F3`、`F9`–`F12`）+ 源码指认（`F4`–`F8`、`F14`–`F16`）+ `M5-design.md` §3.4/§457 |
| 2026-10-25 | **`Q4` 口径修正**：8 → **7**（`perm.go` 是 `// errorcheck`，官方也编不过） | `F11` 实测 |
