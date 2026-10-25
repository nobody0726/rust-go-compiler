# M8 报告 —— 并发解释执行（goroutine / channel / select）

> **状态**：✅ **已收口**（2026-10-25）—— 九条门禁 **9/9 pass**
> **上游**：`M8-design.md`（`D-M8-1..16` / 门禁 `CON-1..9`）/ `M8-tests.md`（`T-CON-01..62`）/ `M8-plan.md`（Task 1–63）
> **机器可读事实**：`M8-manifest.json`（由 `manifest_m8.rs` 对账，13 种合成反例）
> **下游**：M9（寄存器分配与对象模型）/ M10（GC 与扫描器）—— 并发的**语义**在本里程碑冻结

---

## 1. 读数（本报告写就时实测）

| 项 | 值 |
|---|---|
| workspace 测试 | **2110 passed / 0 failed / 1 ignored**（210 suite；M7 收口时 1974） |
| 四条 cargo | `fmt --all -- --check` 0 / `check --workspace --all-targets` 0 error / `clippy --all-targets -D warnings` 0 / `test --workspace --no-fail-fast` **0 failed** |
| 并发判据 | `con_cli` 5 + `con_lower` 13 + `con_single` 13 + `con_chan` 17 + `con_select` 15 + `con_exit` 9 + `con_misc` 7 + `con_slice` 4 + `con_rangefunc` 15 + `chan_obj` 17 = **115 条** |
| 语料（chan） | `自检 0/0 + 仅有输出 0/0 + 空跑 7/7（ok=7 skip=12 bad=0 / 19）` |
| M5 零回归 | `run-collide --run` **69 绿 / 0 红**；`--ssa` **46 绿 / 23 跳过 / 0 红** |
| M7 零回归 | `run-native.sh` **39 ok / 7 skip / 0 bad** |
| 变异 | `scripts/mutation-m8.sh` ⇒ **红 4 / 失手 0**；执行期另跑 17 条（15 红 / 2 绿，结论见 §5.3） |
| 自检 | `check-m1-consistency.py` 全部通过 |
| 拒绝码 | `RejectKind::ALL` **5 条**（M8 下线 `RANGE-FUNC`、新增 `RANGEFUNC-RETURN`）；`SsaCode::ALL` **8 条**（下线 `RANGE-FUNC`） |

---

## 2. 交付物

| 类别 | 内容 |
|---|---|
| 扩 crate | `rgoc-hir`（六个并发节点 + `RejectKind::ALL` + range-over-func 合成函数）、`rgoc-interp`（`Shared`/`G` 拆分、M:1 调度、`ChanObj` 四操作、`Task::{Recv,Send,Go,Select}`、`Halt::Deadlock`）、`rgoc-check`（`recv` 的 commaok 操作数模式 / `select` 子句 `env.push` 时机）、`rgoc-ssa`（下线死码）、`rgoc-driver`（`--sched=seed:N`） |
| HIR 新节点 | `Stmt::{Go, Send, Select}` / `ExprKind::{Recv, Make(chan)}` / `RangeKind::{Chan, Func}` |
| 契约 | `C11`（并发）：M:1 协作式调度、退出码三分、`select` 三阶段与撤销、死锁判定 |
| 装置 | `run-chan.sh` + `chan-input.txt` + `chan-rejected.txt` + `mutation-m8.sh` + `mutation-m8-notes.md` + `shrink-m8.sh` + `shrink_driver.py` |
| 测试 | `con_cli` / `con_lower` / `con_single` / `con_chan` / `con_select` / `con_exit` / `con_misc` / `con_slice` / `con_rangefunc` / `chan_obj` / `manifest_m8` / `comma_ok` |

---

## 3. 关键决策的实测复核（**六条都是官方源码/官方行为教的**）

设计稿在这六处都是错的或含糊的。每条都在容器里用官方 `go` 或读官方源码核对过。

| # | 设计稿写的 | 实测/官方 | 处置 |
|---|---|---|---|
| 1 | `send` 的 `closed` 检查排在 `buf` 之后 | 官方 `runtime/chan.go:224`（`chansend`）加锁后**第一件事**就是查 `closed` | 两份文档 + 实现都改。按原表实现，`close(c); c <- v`（cap>0 未满）会**静默把值塞进缓冲** |
| 2 | `recv` 的操作数模式是 `value` | 官方 `go/types/expr.go:150` 是 **commaok** | 改之前 `v, ok := <-c` 直接报 "2 variables but 1 value" |
| 3 | `select` 选中一个 case 后不管其余登记 | 官方 `sudog.dequeue` 用 `g.selectDone.CAS(0,1)` 把输掉的 waiter **直接跳过** | 不撤销 ⇒ 发送方的下一笔发送落到作废的 waiter 上，**值丢失**（`T-CON-31` 的 fixture 抓到：输出 "first 1" 后死锁） |
| 4 | （未涉及）登记表放 `G` 上 | 撤销发生在**对端**那个 G 里，而被撤销的 G 已被 `drive` `take()` 走 | `select_regs` 必须放 `Shared` |
| 5 | （未涉及）`next_token` 从 0 起 | 0 是「普通 waiter 没有 token」的含义 | 撞车 ⇒ `revoke(0)` 把阻塞的普通发送方一起删掉 ⇒ 死锁。改成从 **1** 起 |
| 6 | `range` over chan 的元素落在 `value` 位 | 官方 `rangeKeyVal` 对通道返回 `(elem, nil)` ⇒ 落在 **key** 位 | 写进 `value` 位会让 `lower_range` 把它当第二个变量，而检查面只给了一个 |

另有三条**文本/期望值**的实测修正：`close` 的 panic 文本**不带** `runtime error:` 前缀
（容器里 `go run` 实测）；`--sched=seed=N` 应为 `seed:N`；`RangeIter::Map` 必须快照
**key 列表**而不是 `(k,v)` 对（迭代中 `delete` 的 key 不该再产出，`T-CON-49`）。

---

## 4. 并发语料三栏（这一栏只证明「没崩」）

```
=== chan: 自检 0/0 + 仅有输出 0/0 + 空跑 7/7（合计 ok=7 skip=12 bad=0 / 19）
```

**必须说清楚的一句话**：`N'` 的 7 个文件，官方 `.exp` **全是 0 字节** ——
`go_source_code/test/chan/*.go` 是编译器测试用例，大多带 `// errorcheck` 指令、
本身不打印。所以这一栏的结论只能是「跑完 / 退出码 0 / 两流皆空」，
**不构成「并发算对了」的证据**。

并发语义的真正证据是 `con_chan` / `con_select` / `con_exit` / `con_rangefunc`
的 **115 条输出顺序断言**（每条都是「打印是探针、通道是闸门」的形状）。

`M'` 12 个：11 个 import stdlib（`TYP-UNSUPPORTED-IMPORT-FORM`）+ 1 个 errorcheck
（`perm.go`：`<-chan int` 赋给 `chan int` ⇒ `CHK-INVALID-ASSIGN`，拒它是对的）。
计划（`M8-tests` §6.1 的 Q4）签的是 `N'=8`，实测 **7** —— 差的就是 `perm.go`，
已记入 manifest 的 `measurement_deviations`。

---

## 5. 真错清单（按「为什么先前没抓到」分类）

### 5.1 语料与判据都抓不到的（**并发语料 7/7 全绿时仍然错**）

| 错 | 为什么绿了也没抓到 |
|---|---|
| **`select` 子句里的无标签 `break` 穿透到外层 for**（官方 `33`，我们算出 `1`） | `select` 不在 body 栈上（由 `Task::Select` 驱动），子句体是唯一能拦 break 的东西，第一版没拦。语料里没有一个文件在 select 子句里写 `break`；它是写 `T-CON-49e`（range-over-func 里的 select）时**顺带**发现的 |
| **`Task::Make` 靠「栈上还有没有值」猜实参个数** ⇒ `f(make(map[string]int))` 吃掉 `f` 的实参 | 只在「嵌套调用的内层是 make」时现形，语料里没有 |
| **`RefCell` 的 `RefMut` 活到 match 结束** ⇒ 分支里再 `borrow()` 报 `already borrowed`（踩了**两次**：`task_send` 与 select 的 `GotZeroClosed`） | 编译期错，不是语义错；但它让「第一次跑通阻塞路径」晚了一个迭代 |

### 5.2 静默错（不崩不报，只是结果不对）

* `select` 不撤销其余登记 ⇒ 发送方的下一笔发送落到作废的 waiter 上，**值丢失**（§3.3）
* `next_token` 从 0 起 ⇒ `revoke(0)` 误删阻塞的普通发送方 ⇒ 死锁
* `RangeIter::Map` 快照 `(k,v)` 对 ⇒ 迭代中 `delete` 的 key 照样产出（`T-CON-49`）
* `range` over chan 的值落在 `value` 位 ⇒ 与检查面给的变量数不符

### 5.3 装置自己在说谎（这一类最危险）

1. **变异探针的测试目标写错**：`M-M8-03` 第一版写 `--lib`，而判据在
   `crates/rgoc-interp/tests/chan_obj.rs`（集成测试）⇒ **一条判据都没跑到**，
   探针报 GREEN。改 `--test chan_obj` 立刻红 2 条。
   `--lib` / `--test <name>` / `-p <crate>` 三者不是一回事。
2. **我的算术错**：`t_con_60b` 的期望值写成 `333`，逐轮算是 `110+101+110=321`
   （官方 go 也是 321）。**期望值必须算两遍**，这次是断言自己错、不是实现错。
3. **两条变异是绿的，都有结论**（`mutation-m8-notes.md` §3）：
   * 「`break` 不翻译」—— 与解释器的 break 落空行为**重合**（弹空 body 栈 ⇒
     `do_return(空)` ⇒ 结果槽零值 `false`，恰是翻译目标）。保留显式翻译，
     另用「译成 `true` 会红」钉住它。
   * 「`breakable` 不看标签」—— **本前端不可达**：具名块的标签只能靠 `goto` 用，
     而 goto 窄面拒循环内的标签；块在顶层则 break 一定先被内层循环拦住。

---

## 6. 白名单 8 条（`U-M8-01..08`）

| id | 内容 | 形态 | 守卫 |
|---|---|---|---|
| `U-M8-01` | 抢占（goroutine 被切走） | 不存在入口 | `t_con_07`：`go func(){ for {} }()` + main 阻塞 ⇒ 预算烧完退出 2，**不**切走 |
| `U-M8-02` | select 多就绪的均匀随机 | 源码顺序（seed=0） | `T-CON-28`：跑 8 次同一结果；seed≠0 才按 rng 打乱 |
| `U-M8-03` | range-over-func body 里 `return`/`goto` | `HIR-UNSUPPORTED-RANGEFUNC-RETURN` + 退出码 1 | `t_con_48a/48b` + `t_ex_02`。**官方 go 接受**这两种写法 ⇒ 有记录的窄化 |
| `U-M8-04` | 11 个 import stdlib 的语料 | 静态 import 拒绝 | `t_con_54` 的冻结断言 |
| `U-M8-05` | 定时器 / `time.Sleep` / select 超时 | 无 stdlib ⇒ import 期失败 | 沿用 `U-M5-01` |
| `U-M8-06` | goroutine 栈按需增长 | 帧深 1000 超了退出 2 | 沿用 `EX-7` 形态 |
| `U-M8-07` | 竞态检测（`-race`） | 不适用（单线程） | — |
| `U-M8-08` | panic 穿过 `go` 边界的包装 | 直接终止、不包装 | `T-CON-39` |

---

## 7. 与 M5 / M6 / M7 的边界

* **对 M5（解释器）**：零回归 —— `run-collide --run` 69 绿 / 0 红。M5 的
  `RejectKind` 从 5 条变 5 条（下线 `RANGE-FUNC`、新增 `RANGEFUNC-RETURN`）；
  `T-EX-02` 从「五条构造全拒」收缩成「range-over-func 窄面」。
* **对 M6（SSA）**：`--via=ssa` 三方差分仍 46 绿 / 23 跳过 / 0 红。
  `SsaCode::RangeFunc` 下线（HIR 不再产出对应节点，SSA 对 range-over-func 的
  拒绝入口换成 `FUNC-VALUE`）；`ssa-rejected.txt` 的 23 行**没有一行**靠它分类。
* **对 M7（native）**：`run-native.sh` 39 ok / 7 skip / 0 bad，逐字节不变。
  并发的六个 HIR 节点在 native 通路仍然走既有拒绝面（`CG-UNSUPPORTED-*`）。

---

## 8. 冻结报告

**冻结**：`C11` 的并发语义（M:1 协作式调度、无缓冲 handoff 的双次深拷贝、缓冲 FIFO
与补位、`close` 三态、`select` 三阶段与撤销、死锁判定、`--sched=seed:N` 的可回放性）；
`con_*` 的 115 条输出顺序断言；`chan-input.txt`/`chan-rejected.txt` 的 19/12 两个分母；
`RejectKind::ALL` 与 `SsaCode::ALL` 的码集合。

**不承诺**：
* **并发语料的三栏不是语义证据**（§4）—— 官方 `.exp` 全空。
* **没有真实并行**：解释器单线程，`go` 只是「入队尾」。数据竞争检测不提供（`U-M8-07`），
  「没有竞态」不等于「没有竞态 bug」。
* **没有抢占**（`U-M8-01`）：一个 `for {}` 会烧完预算把整个程序拖下水（退出 2）。
* **`select` 多就绪不是均匀随机**（`U-M8-02`）：`seed=0` 时恒取源码顺序第一个。
* **range-over-func 的 `return`/`goto` 被拒**（`U-M8-03`），官方 go 接受。
* **goroutine panic 不打栈**（`U-M8-08`），只有一行 `panic: <值>`。
* **缩减脚本的下限是 4 个非空行**（`G-M8-02`）：只做语句级删除，不做字面量内联。

---

## 9. 接手指南

1. **先跑这四条**（都在容器内）：
   ```bash
   RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo test -p rgoc-driver --test con_chan --test con_select --test con_exit --test con_rangefunc'
   RGOC_TTY=0 scripts/in-container.sh bash /work/scripts/mutation-m8.sh
   RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work && bash tools/run-oracle/run-chan.sh --list tools/run-oracle/chan-input.txt'
   RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work && bash tools/run-oracle/run-collide.sh --run --list tools/run-oracle/run-selected.txt'
   ```
2. **改调度之前读** `C11` §2：M:1、FIFO、只在阻塞点让出、`go` 不切走。
   这四条合起来让**单就绪**的交错完全确定 —— 所以 `con_*` 的期望值能写死成字符串。
   哪一条被破坏，症状是「期望值偶发不对」，极难排查。
3. **改 `ChanObj` 之前读**模块头那三条硬规矩：`closed` 检查排第一、
   `close` 清空两侧队列、`close` 不置 `done`。三条都有专门的牙。
4. **加拒绝码**：`RejectKind` 必须同步 `ALL` + `code()` + 计数测试 +
   `con_lower::t_task14_五条码不许再出现` 的清单。**产不出来的码一律删**。
5. **写并发判据**：每个动作先 `println` 再做通信 —— 打印是「这个 G 跑到这里了」的
   探针，通道是「谁先谁后」的闸门。直接写 `<-c` 永远拿不到想要的交错
   （`t_con_14` 就是靠一个 `gate` 通道才逼出「缓冲满 + 发送方已阻塞」那一帧）。
