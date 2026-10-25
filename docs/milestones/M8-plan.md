# M8 计划：解释器并发、异常补齐与冻结

> **For agentic workers:** 按 Task 顺序执行，一个 Task 一个 commit。每个 Task 都先写会红的判据，
> 再实现。**不许**跳过 `Verify` 那一行。
>
> **Goal:** 让解释器支持 `go`/`chan`/`select`/`close` 与 range-over-chan/func，把 defer/panic/recover
> 与 map 迭代的边界补齐，并给顺序语义基线出**冻结报告**。
>
> **Architecture:** 把 `Interp` 拆成「共享状态 `Shared` + 每 goroutine 状态 `G`」，调度器 M:1 单线程
> FIFO、只在阻塞点让出；阻塞编码成「求值栈顶的任务不弹出、醒来重试」；通道是带等待队列的环形缓冲；
> select 走「求值 → 试探 → 登记」三阶段。HIR 只加六个节点，SSA/native 侧一律不动。
>
> **Tech Stack:** Rust 2021 workspace（`rgoc-hir` / `rgoc-interp` / `rgoc-driver`）、`Rc<RefCell<>>`
> 单线程共享、官方 `go` toolchain 做期望值。
>
> **Spec:** [`M8-design.md`](./M8-design.md)（`D-M8-1..14` / `F1..F16` / `CON-1..9`）+
> [`M8-tests.md`](./M8-tests.md)（69 个 `T-CON-*`）。执行者两份都要读。

---

## Global Constraints

- **只在容器里跑 cargo**：`RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && …'`。
  宿主上没有 toolchain，任何「在宿主跑 cargo」的尝试都会失败。
- **`Stmt` / `ExprKind` / `RangeKind` / `Instr` 的 `match` 不许通配**（`_ =>`）：加一个变体就要
  编译失败，逼你在每一处决定它的位置。**唯一的例外**是已经写了理由的分支。
- **`ChanObj` 的四个操作（recv/send/close/len-cap）只许有一处实现**。调用方与被调方各写一遍，
  就是 M7 `pass_args` 那类「两边一起错、端到端仍绿」的错。
- **`DiagCode` 保持 103**。并发相关的拒绝走 crate-local 枚举（`RejectKind`），不新增 `DiagCode`。
- **Phase 0 结束时 `cargo test --workspace` 必须 1974 passed / 0 failed / 1 ignored，且一个测试文件都不许改**。
  改了就是重构动了语义。
- **退出码只有 0/1/2**：`1` = 没跑（静态拒绝），`2` = 跑了没跑完。死锁是 `2`，不是新码。
- **每条「算对了」的判据必须带真输出或自带断言**；纯「退出码 0」只许用在「本就没有输出」的判据上。
- **变异前 `cargo clean -p <crate>`；变异与恢复同一调用内完成；恢复后再 clean 一次**（`M7-report.md` §7）。
- 容器 `/tmp` 调用即焚；跨调用要保留的东西一律放仓库内路径。

## Review Focus

| # | 输入/条件 | 一个正常人会期待什么 | 判据落在 |
|---|---|---|---|
| 1 | `close(c)` 之后仍有缓冲值 | 接收方**先拿完缓冲**再拿零值，`range` 也先把值吐完 | `T-CON-17/18` |
| 2 | `case v, ok := <-c` | 未关闭 `ok=true`、关闭 `ok=false`；不报「2 variables but 1 value」 | `T-CON-34`（并修 `rgoc-check`） |
| 3 | `go f(g())` | `g()` 在**当前** goroutine 跑完，f 才在新 goroutine 开始 | `T-CON-02` |
| 4 | 两个 case 都就绪的 `select` | 不崩、不挂，有一个确定的选中结果（默认第一个） | `T-CON-28` |
| 5 | goroutine 里 `panic` 没人接 | 整个程序退出 2，而不是只静默杀掉那个 goroutine | `T-CON-37` |
| 6 | `var c chan int; <-c` | 永久阻塞 ⇒ 死锁，**不是** panic | `T-CON-22` |
| 7 | main 返回时还有 goroutine 在跑 | 立即退出 0，不等、不跑它的 defer | `T-CON-36/40` |

---

# Phase 0：契约与纯重构（Task 1–6）

> 这一 Phase 的唯一目标是「把结构改对，且一行语义都不动」。

### Task 1: `C11-concurrency.md` 契约骨架

**Files:**
- Create: `docs/contracts/C11-concurrency.md`
- Modify: `docs/README.md`（契约清单登记 C11）

**Interfaces:**
- Produces: 五节正文（调度 / 通道状态机 / select / 死锁与退出 / 不承诺），供 Task 17–44 逐条对齐

- [ ] 写 §1 调度：M:1、单 runq FIFO、**只在阻塞点让出**、`go f()` 不切走、seed 只影响 select
- [ ] 写 §2 通道状态机：`ChanObj` 的四个操作真值表（照 `M8-design.md` §3.3 抄，含 close 的「先排空 buf」）
- [ ] 写 §3 select：三阶段 + token 撤销 + `U-M8-02`（源码顺序，"官方是均匀随机"）
- [ ] 写 §4 死锁与退出：四情形表 + 退出码 2 的理由
- [ ] 写 §5 不承诺：无事件日志 / 无抢占 / 无 `-race` / 无定时器
- [ ] 自洽检查：§5 的每条「不承诺」都在 §1–§4 里被提到过一次（不许只有 §5 说）
- [ ] Commit：`docs(m8): C11 并发契约骨架（Task 1）`

**Verify:** `grep -c '^| ' docs/contracts/C11-concurrency.md` ⇒ ≥ 15；`grep -n 'C11' docs/README.md` 有命中

### Task 2: HIR 六个节点（只加类型）

**Files:**
- Modify: `rgoc/crates/rgoc-hir/src/types.rs`（`Stmt` / `ExprKind` / `RangeKind` + 新结构）
- Modify: `rgoc/crates/rgoc-hir/src/lib.rs`（导出）
- Test: `rgoc/crates/rgoc-hir/tests/types_concurrency.rs`

**Interfaces:**
- Produces: `Stmt::{Go{call},Send{chan,value},Select{clauses}}`、`ExprKind::Recv{ch,comma_ok}`、
  `RangeKind::{Chan,Func}`、`SelectClause{kind,chan,recv_target,recv_comma_ok,send_value,body}`、
  `SelectKind::{Send,Recv,Default}`
- Consumes: 无（纯数据）

- [ ] 先写会红的测试：构造六个节点各一次，断言 `Debug` 输出含关键字段名（形状判据，不测行为）
- [ ] 加类型定义；`Stmt`/`ExprKind` 的所有 `match` 补分支 —— **先全部 `todo!("Task 7+")`**
      （这样 Task 3 的重构能在「lowering 还不产出」的前提下编译通过）
- [ ] `RangeKind` 加 `Chan`/`Func`；`lower.rs` 里对 `RangeKind` 的 `match` 补 `todo!`
- [ ] Commit：`feat(hir): 并发六节点（Task 2）`

**Verify:** `… cargo test -p rgoc-hir --test types_concurrency`（≥1）；`… cargo check --workspace` 零错

### Task 3: `Shared` / `G` 纯重构（本阶段最重的一步）

**Files:**
- Create: `rgoc/crates/rgoc-interp/src/sched.rs`
- Modify: `rgoc/crates/rgoc-interp/src/interp.rs`（`Interp` 消失，方法挪到 `impl G`）

**Interfaces:**
- Consumes: 现有 `Interp` 的 13 个字段
- Produces: `Shared<'a>{prog,tt,opts,globals,out,steps,halted,methods,chans,runq,gs,next_g,rng}`、
  `G<'a>{id,frames,eval,values,targets,pending,phase,blocked,is_defer}`、
  `pub fn run(prog,tt,opts) -> Outcome`（**签名不变**）、`G::one_step(&mut self, sh: &mut Shared<'a>) -> Step`
- Produces: `Step { Continue, Blocked, Exited }`

- [ ] 建 `Shared` / `G`，字段搬过去；`gs: Vec<Option<G>>` 里先只放一个 main
- [ ] `impl Interp` 的方法逐个挪到 `impl G`，加 `sh: &mut Shared<'a>` 参数（编译器逐个报）
- [ ] `drive()` 挪到 `impl Shared`，先写成「永远只跑 gs[0]，直到它退出」
- [ ] `finish()` 挪到 `impl Shared`
- [ ] 跑全量：**必须 1974 passed / 0 failed / 1 ignored，且 `git diff --stat` 里 `tests/` 一个文件都没有**
- [ ] Commit：`refactor(interp): 拆 Shared/G，Interp 消失（Task 3，D-M8-1）`

**Verify:** `… cargo test --workspace --no-fail-fast 2>&1 | awk '/^test result/ {p+=$4; f+=$6} END {print p, f}'` ⇒ `1974 0`

### Task 4: 调度循环（单 G 直通）

**Files:**
- Modify: `rgoc/crates/rgoc-interp/src/sched.rs`
- Test: `rgoc/crates/rgoc-interp/tests/sched_loop.rs`

**Interfaces:**
- Produces: `Shared::pick(&mut self) -> Option<Gid>`（先「永远返回当前 G ∋ 阻塞则换下一个」）、
  `Shared::park(gid, g)`、`Shared::reap(gid)`、`Shared::all_blocked() -> bool`、`Shared::main_done() -> bool`

- [ ] 先写会红的测试：造两个假 G（各跑一条 `print` 后 `Exited`），断言两个都跑、顺序是入队序
- [ ] 实现 `pick`：`runq` 非空就弹队首；否则若当前 G 还没退出就继续它
- [ ] `park`：`g.blocked` 非空 ⇒ 存回 `gs[gid]`，不重新入队；`reap`：`gs[gid] = None`
- [ ] `main_done`：`gs[0]` 为 `None` 或 main 的 `frames` 空且 `phase == Done`
- [ ] Commit：`feat(interp): M:1 调度循环（Task 4）`

**Verify:** `… cargo test -p rgoc-interp --test sched_loop`（≥3）

### Task 5: `--sched=seed:N` 参数面

**Files:**
- Modify: `rgoc/crates/rgoc-driver/src/cli.rs`（`RunCommand.sched`）、`run_cmd.rs`
- Test: `rgoc/crates/rgoc-driver/tests/con_cli.rs`

**Interfaces:**
- Produces: `enum Sched { Deterministic { seed: u64 } }`、`CliError::BadSched(String)`；
  `RunCommand{ …, sched: Sched }`（默认 `Deterministic{ seed: 0 }`）
- Consumes: `Shared.rng`

- [ ] 先写会红的测试：`--sched=seed:7` 解析出 `seed=7`；`--sched=seed:x` / `--sched=random` ⇒ `BadSched` + 退出码 2
- [ ] 实现解析（**非法值不许静默回落到默认**）；`USAGE` 补一行
- [ ] `Shared.rng` 由 seed 初始化（`xorshift64`，seed=0 时视为「源码顺序」模式，见 Task 37）
- [ ] Commit：`feat(cli): --sched=seed:N（Task 5）`

**Verify:** `… cargo test -p rgoc-driver --test con_cli`（≥4）

### Task 6: `ChanObj` / `Waiter` 数据结构（不接解释器）

**Files:**
- Create: `rgoc/crates/rgoc-interp/src/chan.rs`
- Test: `rgoc/crates/rgoc-interp/tests/chan_obj.rs`

**Interfaces:**
- Produces: `ChanObj{elem,cap,buf,sendq,recvq}`、`Waiter{g,token,slot,done}`、
  `ChanObj::try_recv(&mut self) -> RecvOutcome`、`ChanObj::try_send(&mut self, v: Value) -> SendOutcome`、
  `ChanObj::close(&mut self) -> CloseOutcome`、`ChanObj::len()/cap()`
- Produces: `RecvOutcome { Got(Value), GotZeroClosed, Blocked }`、`SendOutcome { Done, Blocked, Closed }`、
  `CloseOutcome { Woken(Vec<Gid>), AlreadyClosed }`

- [ ] 先写会红的表驱动测试：14 个状态（空/满/半/closed/nil）× 3 个操作，逐格断言 `Outcome`
- [ ] 实现四个操作（**这是全项目唯一一处**，后面 Task 20–23 只接线）
- [ ] `close` 的 `Woken` 必须包含 recvq **与** sendq 两侧（`M8-design.md` §3.3）
- [ ] Commit：`feat(interp): 通道对象与四操作（Task 6）`

**Verify:** `… cargo test -p rgoc-interp --test chan_obj`（≥14）

---

# Phase 1：lowering 放行与单 goroutine 语义（Task 7–16）

### Task 7: `make(chan T, n)` lowering

**Files:** Modify `rgoc/crates/rgoc-hir/src/lower.rs`（`lower_make`），Test `rgoc-driver/tests/con_lower.rs`

**Interfaces:**
- Produces: `ExprKind::Make { kind: MakeKind::Chan, typ, len: Some(n) }`（`n` 缺省 ⇒ `len: None`，cap=0）
- Consumes: 无

- [ ] 先写会红的测试：`p_make.go`（三种形态：无缓冲 / 带 cap / 带变量 cap）在 `run` 下**不再**报 `HIR-UNSUPPORTED-CHAN`
- [ ] 删掉 `make(chan)` 的 `reject`，改成产出 `MakeKind::Chan`
- [ ] Commit：`feat(hir): make(chan) 放行（Task 7）`

**Verify:** `… cargo test -p rgoc-driver --test con_lower`（≥1）

### Task 8: `close(c)` 与 `len`/`cap` 的 chan 分支

**Files:** Modify `lower.rs`（`close` 的 `reject` 删掉、`builtin_len_cap`），Test `con_lower.rs`

**Interfaces:**
- Produces: `BuiltinKind::Close` 真的产出；`len`/`cap` 对 `chan` 走同一 `BuiltinKind::{Len,Cap}`（解释器按实参类型分流）

- [ ] 先写会红的测试：`close(c)` 不再报 `HIR-UNSUPPORTED-CLOSE`；`len(c)`/`cap(c)` 不再报类型错
- [ ] 删 `RejectKind::Close` 的调用点；`len`/`cap` 的类型分支加 `Chan`
- [ ] Commit：`feat(hir): close 与 chan 的 len/cap（Task 8）`

**Verify:** `… cargo test -p rgoc-driver --test con_lower`（≥2）

### Task 9: `ch <- v` lowering

**Files:** Modify `lower.rs`（两处 `reject`），Test `con_lower.rs`

**Interfaces:**
- Produces: `Stmt::Send { chan, value }`（求值顺序：chan 先、value 后 —— 写注释钉住）

- [ ] 先写会红的测试：`c <- 1` 不再报 `HIR-UNSUPPORTED-CHAN`；且 lowering 出的语句是 `Send`
- [ ] 删两处 send 的 `reject`（`lower.rs:946` 与 `:1344`）
- [ ] Commit：`feat(hir): 发送语句（Task 9）`

**Verify:** `… cargo test -p rgoc-driver --test con_lower`（≥2）

### Task 10: `<-ch` lowering（含 comma-ok）

**Files:** Modify `lower.rs`（`Unary` 的 `Recv` 分支，`:1772`），Test `con_lower.rs`

**Interfaces:**
- Produces: `ExprKind::Recv { ch, comma_ok }`；`comma_ok` 的置位规则与 `mark_comma_ok` 同源（右唯一项、左两个）

- [ ] 先写会红的测试：`<-c` 与 `v, ok := <-c` 各一条，断言 lowering 成功且 `comma_ok` 正确
- [ ] 删 recv 的 `reject`；接上 `comma_ok` 的判定
- [ ] Commit：`feat(hir): 接收表达式（Task 10）`

**Verify:** `… cargo test -p rgoc-driver --test con_lower`（≥2）

### Task 11: `go f(...)` lowering

**Files:** Modify `lower.rs`（`:939`），Test `con_lower.rs`

**Interfaces:**
- Produces: `Stmt::Go { call }`；`call` 复用现有的调用 lowering（实参在**当前** G 求值，写注释）

- [ ] 先写会红的测试：`go f(1)` 与 `go func(){ }()` 两条 lowering 成功
- [ ] 删 `RejectKind::Go` 的调用点；把调用表达式包成 `Stmt::Go`
- [ ] Commit：`feat(hir): go 语句（Task 11）`

**Verify:** `… cargo test -p rgoc-driver --test con_lower`（≥2）

### Task 12: `select` lowering

**Files:** Modify `lower.rs`（`:950`），Test `con_lower.rs`

**Interfaces:**
- Produces: `Stmt::Select { clauses: Vec<SelectClause> }`；`SelectKind::{Send,Recv,Default}`；
  recv 分支的 `recv_target` 支持 `v` / `v, ok` 两种

- [ ] 先写会红的测试：五种形态各 lowering 一次（单 recv / recv+ok / send / default / 两个 case）
- [ ] 实现 clauses 的转换（**顺序必须与源码一致**，后面 `T-CON-28` 依赖它）
- [ ] Commit：`feat(hir): select 语句（Task 12）`

**Verify:** `… cargo test -p rgoc-driver --test con_lower`（≥5）

### Task 13: `range` over chan

**Files:** Modify `lower.rs`（`range` 形态判定），Test `con_lower.rs`

**Interfaces:**
- Produces: `RangeKind::Chan`；`Stmt::Range { kind: Chan, key: None, value: Some(place) }`

- [ ] 先写会红的测试：`for v := range c` 不再报 `HIR-UNSUPPORTED-CHAN`，`kind == Chan`
- [ ] 加形态判定（`unaliased` 是 `Type::Chan` ⇒ `Chan`）；注意 `chan<- T` 上 range 是**类型错**（check 已管）
- [ ] Commit：`feat(hir): range over chan（Task 13）`

**Verify:** `… cargo test -p rgoc-driver --test con_lower`（≥2）

### Task 14: 五条码下线与白名单登记

**Files:** Modify `rgoc/crates/rgoc-hir/src/diag.rs`（`RejectKind` 删 5 个）、`docs/milestones/M8-tests.md`（§5）
- Test: `rgoc-driver/tests/con_lower.rs`（守卫）

**Interfaces:**
- Produces: `RejectKind` 只剩 `Goto` / `Generic` / `MissingType` / `InconsistentCheck` / `RangeFuncReturn`(Task 47 加)

- [ ] 先写会红的守卫测试：断言 `HIR-UNSUPPORTED-{GO,CHAN,SELECT,CLOSE}` 这四个码**不再出现**（源码里 grep 不到 `as_str` 的返回值）
- [ ] 删掉 5 个变体与它们的 `as_str`；`RejectKind::ALL` 计数测试同步
- [ ] Commit：`refactor(hir): 下线 5 条并发拒绝码（Task 14）`

**Verify:** `… cargo test -p rgoc-hir -p rgoc-driver`（全绿）；`grep -rn 'HIR-UNSUPPORTED-GO' rgoc/` ⇒ 无命中

### Task 15: 单 goroutine 语义（没有调度也要对）

**Files:** Modify `rgoc-interp/src/interp.rs`（`Stmt::Go` / `Stmt::Send` / `ExprKind::Recv` 的直接执行），
Test `rgoc-driver/tests/con_single.rs`

**Interfaces:**
- Consumes: Task 6 的 `ChanObj`
- Produces: `G::exec_go(&mut self, sh, call)`：**同步**执行（先跑完再继续）；`G::exec_send` / `G::exec_recv`
  的**非阻塞路径**（buf 有空 / buf 有值 / closed）

- [ ] 先写会红的测试：`go f()` 之后 f 的打印出现在下一句之前（同步语义）；`cap=2` 的通道发两个收两个顺序正确
- [ ] 实现三条路径；**阻塞路径先 `todo!("Task 20")`**（此时不许有任何 fixture 能触发阻塞）
- [ ] Commit：`feat(interp): 单 goroutine 的 go/chan 语义（Task 15）`

**Verify:** `… cargo test -p rgoc-driver --test con_single`（≥4）

### Task 16: `make`/`close`/`len`/`cap` 的解释执行

**Files:** Modify `interp.rs`（`MakeKind::Chan` / `BuiltinKind::Close` / `Len`/`Cap` 的 chan 分支），
Test `con_single.rs`

**Interfaces:**
- Produces: `Value::Chan(Rc<RefCell<ChanObj>>)`（新变体，加进 `value.rs` 的 `deep_copy`/`print`）
- Produces: `print` 一个 chan ⇒ `0xc000000000` 占位（`C9` §4.3 的指针口径）

- [ ] 先写会红的测试：`len`/`cap` 四个组合；`print(c)` 的字节；`close` 后 `len` 不变、`cap` 不变
- [ ] 加 `Value::Chan`；`deep_copy` 对它是**共享**（不是拷贝通道本身，只拷引用）
- [ ] Commit：`feat(interp): 通道值与三个内建（Task 16）`

**Verify:** `… cargo test -p rgoc-driver --test con_single`（≥4）

---

# Phase 2：通道与调度（Task 17–28）

### Task 17: `Task::Recv` 接入求值栈

**Files:** Modify `interp.rs`（`Task` 加变体、`step_eval` 分派），Test `con_run.rs`

**Interfaces:**
- Produces: `Task::Recv { ch: ValueId 等价物, comma_ok: bool, waiter: Option<Rc<RefCell<Waiter>>> }`

- [ ] 先写会红的测试：`<-c`（非阻塞路径）走 `Task::Recv` 后值栈恰好压一个值；`comma_ok` 时压两个
- [ ] 实现 `Task::Recv` 的「就绪即提交」分支（`done == false` 时**不许**动值栈）
- [ ] Commit：`feat(interp): Task::Recv（Task 17）`

**Verify:** `… cargo test -p rgoc-driver --test con_run`（≥2）

### Task 18: `Task::Send`

**Files:** 同上

**Interfaces:**
- Produces: `Task::Send { ch, value: Value, waiter: Option<…> }`

- [ ] 先写会红的测试：发送语句在非阻塞路径下不压值、不留垃圾到值栈（发送后值栈深度回到语句前）
- [ ] 实现；`Stmt::Send` 改成「求值 chan + value → 压 `Task::Send` → 由它提交」（这样阻塞时任务留在栈上）
- [ ] Commit：`feat(interp): Task::Send（Task 18）`

**Verify:** `… cargo test -p rgoc-driver --test con_run`（≥2）

### Task 19: park / ready 骨架

**Files:** Modify `sched.rs`、`interp.rs`（`one_step` 返回 `Blocked`）

**Interfaces:**
- Produces: `Shared::ready(&mut self, gid)`（入队尾 + 清 `blocked`）、`G.blocked: Option<Block>`、
  `Block { reason: BlockReason, waiters: Vec<Rc<RefCell<Waiter>>> }`

- [ ] 先写会红的测试：一个「A 阻塞 → B 跑 → B 唤醒 A → A 继续」的三步 fixture，`T-CON-03` 的雏形
- [ ] 实现（此时 wakeup 由 `ChanObj` 的 `Woken` 列表驱动）
- [ ] Commit：`feat(interp): park/ready（Task 19）`

**Verify:** `… cargo test -p rgoc-driver --test con_run`（≥3）

### Task 20: 无缓冲 handoff

**Files:** Modify `interp.rs`（`Task::Recv`/`Task::Send` 的阻塞分支），Test `con_chan.rs`

**Interfaces:**
- Consumes: Task 6 的 `try_recv`/`try_send` 的 `Blocked` 结果
- Produces: 阻塞时登记 `Waiter{g, token, slot, done}` 到对应队列，返回 `Step::Blocked`

- [ ] 先写会红的测试 `T-CON-11`（`recv` 的打印必须在 `send` 的打印之前）与 `T-CON-12`
- [ ] 实现；`Waiter.done` 由对端置位
- [ ] Commit：`feat(interp): 无缓冲 handoff（Task 20，T-CON-11）`

**Verify:** `… cargo test -p rgoc-driver --test con_chan`（≥3）

### Task 21: 缓冲 FIFO 与补位

**Files:** 同上

- [ ] 先写会红的测试 `T-CON-14/15/16`（含「收一个后 len 立刻回到 cap」）
- [ ] 实现 recv 的「取完 buf 后把阻塞发送方的值搬进 buf 并唤醒它」
- [ ] Commit：`feat(interp): 缓冲 FIFO 与补位（Task 21，T-CON-14..16）`

**Verify:** `… cargo test -p rgoc-driver --test con_chan`（≥3）

### Task 22: close 三态与三个 panic

**Files:** 同上

- [ ] 先写会红的测试 `T-CON-17/18/19/20/21`（两条 close 的 panic 要在**自己**的 G 里）
- [ ] 实现：closed 上 send ⇒ 唤醒后 panic；double close / close(nil) ⇒ 立刻 panic
- [ ] Commit：`feat(interp): close 三态（Task 22，T-CON-17..21）`

**Verify:** `… cargo test -p rgoc-driver --test con_chan`（≥5）

### Task 23: nil 通道永久阻塞

**Files:** 同上

- [ ] 先写会红的测试 `T-CON-22`（两条：nil recv / nil send ⇒ 死锁，不是 panic）
- [ ] 实现：`Value::Chan` 为 nil 时直接 `Blocked` 且**不登记**（没有队列可登记）
- [ ] Commit：`feat(interp): nil 通道永久阻塞（Task 23，T-CON-22）`

**Verify:** `… cargo test -p rgoc-driver --test con_chan`（≥2）

### Task 24: goroutine 创建与退出

**Files:** Modify `sched.rs`（`spawn`）、`interp.rs`（`Stmt::Go` 改成入队）

**Interfaces:**
- Produces: `Shared::spawn(&mut self, call) -> Gid`：新建 `G`（**直接把调用压成它的帧**）、入队尾、`next_g += 1`

- [ ] 先写会红的测试 `T-CON-01/02`（创建顺序 + 实参在旧 G 求值）
- [ ] 实现：`go f()` → 在**当前** G 求值 `f` 与实参 → 用求出的值建新 G 的栈帧
- [ ] `G::one_step` 在「帧空且非 main」时返回 `Exited`
- [ ] Commit：`feat(interp): goroutine 创建与退出（Task 24，T-CON-01/02）`

**Verify:** `… cargo test -p rgoc-driver --test con_chan`（≥2）

### Task 25: 值的双次深拷贝

**Files:** Modify `interp.rs`（send/recv 的提交点）

- [ ] 先写会红的测试 `T-CON-13`（发送后改源、接收后改副本，两向都不影响）
- [ ] 实现：`try_send` 前 `deep_copy`，`try_recv` 后 `deep_copy`
- [ ] Commit：`fix(interp): 通道值双次深拷贝（Task 25，T-CON-13）`

**Verify:** `… cargo test -p rgoc-driver --test con_chan`（≥2）

### Task 26: 聚合元素的深拷贝

**Files:** 同上（`deep_copy` 本身可能已够，要验）

- [ ] 先写会红的测试 `T-CON-24`（`chan [3]int`，改第 2、3 个元素）
- [ ] 若 `deep_copy` 只拷第一个字就修（M7 踩过同款）
- [ ] Commit：`fix(interp): 聚合元素深拷贝（Task 26，T-CON-24）`

**Verify:** `… cargo test -p rgoc-driver --test con_chan`（≥1）

### Task 27: 四条不变式

**Files:** Create `sched.rs` 的 `check_invariants()`；Test `rgoc-interp/tests/sched_invariants.rs`

**Interfaces:**
- Produces: `Shared::check_invariants(&self) -> Result<(), String>`（`RGOC_SCHED_DEBUG=1` 时每步调用）

- [ ] 先写会红的测试：人工构造四种坏状态（漏删 / 删了没唤醒 / 两态并存 / buf 越界），断言各报出**指名道姓**的消息
- [ ] 实现四条（`M8-design.md` §4.3）
- [ ] Commit：`feat(interp): 调度不变式（Task 27）`

**Verify:** `… cargo test -p rgoc-interp --test sched_invariants`（≥4）

### Task 28: 资源上限（goroutine / 步数 / 帧深）

**Files:** Modify `sched.rs`（`spawn` 的上限）、`interp.rs`（帧深按 G）

- [ ] 先写会红的测试 `T-CON-06/07/08`
- [ ] 实现：`max_goroutines`（默认 10 000）、`steps` 全局、帧深看 `g.frames.len()`
- [ ] 加 `U-M8-01` 的守卫：`go func(){ for {} }()` + main 在 `<-done` 上阻塞 ⇒ 预算超限退出 2
      （证明「不抢占」：若抢占，main 就有机会跑完并退出 0）
- [ ] Commit：`feat(interp): 资源上限（Task 28，T-CON-06..08）`

**Verify:** `… cargo test -p rgoc-driver --test con_chan`（≥3）

---

# Phase 3：select（Task 29–38）

### Task 29: select 的求值阶段

**Files:** Modify `interp.rs`（`Stmt::Select` 的第一段）

**Interfaces:**
- Produces: `Task::Select { chans: Vec<Option<Value>>, sends: Vec<Option<Value>>, chosen: Option<usize> }`
  —— `chans`/`sends` 与 `clauses` **同长同序**（顺序是 `T-CON-28` 的判据基础）

- [ ] 先写会红的测试 `T-CON-33`（走 `default` 时**所有** case 的通道表达式都求值过）
- [ ] 实现：按源码顺序把所有 `chan` 表达式与所有 `send_value` 求值，结果存进 `Task::Select` 的位置表
- [ ] Commit：`feat(interp): select 求值阶段（Task 29，T-CON-33）`

**Verify:** `… cargo test -p rgoc-driver --test con_select`（≥1）

### Task 30: 试探阶段（nil / default / 就绪）

**Files:** 同上

- [ ] 先写会红的测试 `T-CON-25/26/30`
- [ ] 实现：按源码顺序扫；`nil` 跳过；`default` 记「有」；就绪的记「可做」
- [ ] Commit：`feat(interp): select 试探阶段（Task 30，T-CON-25/26/30）`

**Verify:** `… cargo test -p rgoc-driver --test con_select`（≥3）

### Task 31: 阻塞阶段与多路登记

**Files:** 同上

- [ ] 先写会红的测试 `T-CON-27`（两个 case 都不可做，第三个 G 稍后发）
- [ ] 实现：给每个非 nil 的 case 登记一个 waiter，共用一个 `token`
- [ ] Commit：`feat(interp): select 阻塞与多路登记（Task 31，T-CON-27）`

**Verify:** `… cargo test -p rgoc-driver --test con_select`（≥1）

### Task 32: token 撤销

**Files:** 同上 + `chan.rs`（`revoke(token)`）

**Interfaces:**
- Produces: `ChanObj::revoke(&mut self, token: u64)`（幂等）、`Shared::revoke_all(&mut self, g: &G, token)`

- [ ] 先写会红的测试 `T-CON-31` 的第一轮（被唤醒后另一个 case 的登记消失：再发一次才轮到它）
- [ ] 实现撤销（找不到 waiter 也算成功）
- [ ] Commit：`feat(interp): select 撤销（Task 32，T-CON-31）`

**Verify:** `… cargo test -p rgoc-driver --test con_select`（≥1）

### Task 33: 重复 channel

**Files:** 同上（主要是 fixture 与断言）

- [ ] 先写会红的测试 `T-CON-31` 的两轮形态（两个 case 的 body 各跑一次）
- [ ] 修：两个 case 各自独立登记，靠 `done` 判 winner
- [ ] Commit：`test(interp): select 重复 channel（Task 33，T-CON-31）`

**Verify:** `… cargo test -p rgoc-driver --test con_select`（≥1）

### Task 34: 空 select

**Files:** 同上

- [ ] 先写会红的测试 `T-CON-32`（`select {}` ⇒ 死锁，不是预算超限）
- [ ] 实现：无 case ⇒ `Blocked` 且不登记（同 nil 通道的路子）
- [ ] Commit：`feat(interp): 空 select（Task 34，T-CON-32）`

**Verify:** `… cargo test -p rgoc-driver --test con_select`（≥1）

### Task 35: recv case 的 comma-ok

**Files:** Modify `interp.rs`（`SelectClause.recv_comma_ok` 的落位）

- [ ] 先写会红的测试 `T-CON-34`（未关闭 `ok=true`、关闭后 `ok=false`）
- [ ] 实现：winner 是 recv case 时按 `recv_target` 写 1 或 2 个位置
- [ ] Commit：`feat(interp): select 的 comma-ok（Task 35，T-CON-34）`

**Verify:** `… cargo test -p rgoc-driver --test con_select`（≥1）

### Task 36: send case

**Files:** 同上

- [ ] 先写会红的测试：`select { case c <- 1: }` 在无缓冲、有接收方时成功；无接收方且无 `default` 时阻塞
- [ ] 实现（走 `try_send` 的三种 `Outcome`）
- [ ] Commit：`feat(interp): select 的 send case（Task 36）`

**Verify:** `… cargo test -p rgoc-driver --test con_select`（≥2）

### Task 37: seed 驱动的选择

**Files:** Modify `interp.rs`（多就绪时的选择）

- [ ] 先写会红的测试 `T-CON-28`（默认 seed ⇒ 8 次同一个结果）与 `T-CON-29`（seed=1..8 ⇒ 至少两种结果）
- [ ] 实现：`rng == 0` ⇒ 取第一个可做的；否则 `xorshift` 取模
- [ ] Commit：`feat(interp): select 的 seed 选择（Task 37，T-CON-28/29）`

**Verify:** `… cargo test -p rgoc-driver --test con_select`（≥2）

### Task 38: select 的守卫与不变式

**Files:** Test `rgoc-interp/tests/sched_invariants.rs`（加 select 场景）

- [ ] 加测试：select 阻塞后不变式仍成立（四个 case 各登记一次，撤销后队列干净）
- [ ] 加 `U-M8-02` 的守卫：同一 fixture 跑 8 次结果相同
- [ ] Commit：`test(interp): select 的守卫（Task 38）`

**Verify:** `… cargo test -p rgoc-interp -p rgoc-driver`（全绿）

---

# Phase 4：死锁与退出（Task 39–44）

### Task 39: `Halt::Deadlock` 与判定

**Files:** Modify `interp.rs`（`Halt`）、`sched.rs`（`drive` 的分支）

- [ ] 先写会红的测试 `T-CON-35` 的**正例**（真死锁）
- [ ] 实现：`runq` 空 ∧ 存在 blocked ∧ main 未结束 ⇒ `halted = Deadlock`
- [ ] Commit：`feat(interp): 死锁判定（Task 39，T-CON-35）`

**Verify:** `… cargo test -p rgoc-driver --test con_exit`（≥1）

### Task 40: main 返回即退出

**Files:** Modify `sched.rs`（`main_done` 优先于死锁判定）

- [ ] 先写会红的测试 `T-CON-36`（G 无限打印、main 立刻返回 ⇒ 退出 0 且输出有限）
- [ ] 实现：先判 `main_done`，再判死锁
- [ ] Commit：`feat(interp): main 返回即退出（Task 40，T-CON-36）`

**Verify:** `… cargo test -p rgoc-driver --test con_exit`（≥1）

### Task 41: goroutine 内 panic 终止全程序

**Files:** Modify `interp.rs`（`Halt::Panic` 不区分是哪个 G 提的）

- [ ] 先写会红的测试 `T-CON-37/39`（panic 值不包装、退出码 2）
- [ ] 实现：任何 G 的未捕获 panic 都直接 `halted`
- [ ] Commit：`feat(interp): goroutine 内 panic（Task 41，T-CON-37/39）`

**Verify:** `… cargo test -p rgoc-driver --test con_exit`（≥2）

### Task 42: 被丢弃的 G 不跑 defer

**Files:** Modify `sched.rs`（`reap` 不跑 defer；只有正常退出才跑）

- [ ] 先写会红的测试 `T-CON-40`
- [ ] 实现：main 结束时直接丢 `gs`，不做任何 teardown
- [ ] Commit：`feat(interp): 丢弃 G 不跑 defer（Task 42，T-CON-40）`

**Verify:** `… cargo test -p rgoc-driver --test con_exit`（≥1）

### Task 43: 死锁消息的字节面

**Files:** Modify `interp.rs`（`finish()` 的 stderr 文本）

- [ ] 先写会红的测试 `T-CON-35` 的**字节**断言：`fatal error: all goroutines are asleep - deadlock!\n` 在 **stderr**，不在合并流
- [ ] 实现；`C9` §5 加两行（死锁、goroutine 内 panic）
- [ ] Commit：`feat(interp): 死锁消息与 C9 增补（Task 43，T-CON-35）`

**Verify:** `… cargo test -p rgoc-driver --test con_exit`（≥2）

### Task 44: 死锁与退出的反例集

**Files:** Test `con_exit.rs`

- [ ] 加两个**不该判死锁**的 fixture（G 阻塞但 main 能跑 / main 已返回）
- [ ] 加 `T-CON-41`（退出码 1 vs 2 的分界）
- [ ] 加 `T-CON-42`（交错顺序确定）
- [ ] Commit：`test(interp): 死锁反例集（Task 44，T-CON-41/42）`

**Verify:** `… cargo test -p rgoc-driver --test con_exit`（≥4）

---

# Phase 5：三块补齐（Task 45–52）

### Task 45: range-over-func 下降（合成函数）

**Files:** Modify `lower.rs`（`RangeKind::Func` 的 lowering），Test `con_rangefunc.rs`

**Interfaces:**
- Produces: 合成函数 `Func{ params: [v], results: [bool], captured: [...], name: "__range_body_<n>" }`；
  `Stmt::Range{ kind: Func }` 只保留「调用迭代器 + 传合成函数」

- [ ] 先写会红的测试 `T-CON-43`（打印 0,1,2）
- [ ] 实现：把 body 下沉成合成函数（复用闭包的 capture 机制），`yield` 就是它的调用
- [ ] Commit：`feat(hir): range-over-func 下降（Task 45，T-CON-43）`

**Verify:** `… cargo test -p rgoc-driver --test con_rangefunc`（≥2）

### Task 46: break / continue 映射

**Files:** 同上（合成函数的返回值）

- [ ] 先写会红的测试 `T-CON-44/45`
- [ ] 实现：`break` ⇒ `return false`；`continue` ⇒ `return true`；正常结束 ⇒ `true`
- [ ] Commit：`feat(hir): rangefunc 的 break/continue（Task 46，T-CON-44/45）`

**Verify:** `… cargo test -p rgoc-driver --test con_rangefunc`（≥2）

### Task 47: body 内 `return`/`goto` 静态拒

**Files:** Modify `lower.rs`、`diag.rs`（加 `RejectKind::RangeFuncReturn`）

**Interfaces:**
- Produces: `HIR-UNSUPPORTED-RANGEFUNC-RETURN`；`U-M8-03`

- [ ] 先写会红的测试 `T-CON-48`（`return` 与 `goto` 各一条 ⇒ 退出码 1 + 码）
- [ ] 实现：下沉前扫 body，见到 `Return`/`Branch{Goto}` 就 `reject`
- [ ] Commit：`feat(hir): rangefunc 的 return 拒绝（Task 47，T-CON-48）`

**Verify:** `… cargo test -p rgoc-driver --test con_rangefunc`（≥2）

### Task 48: seq2 与嵌套

**Files:** 同上

- [ ] 先写会红的测试 `T-CON-47`（两参数 yield）与 `T-CON-46`（两层嵌套不互相干扰）
- [ ] 实现：合成函数的形参个数按 yield 的签名定
- [ ] Commit：`feat(hir): rangefunc 的 seq2 与嵌套（Task 48，T-CON-46/47）`

**Verify:** `… cargo test -p rgoc-driver --test con_rangefunc`（≥2）

### Task 49: map 迭代快照

**Files:** Modify `interp.rs`（`RangeKind::Map` 的 `RangeIter`）

- [ ] 先写会红的测试 `T-CON-49/50`（删除跳过、新增不访问）
- [ ] 实现：进入 range 时快照 key 列表；取下一个时「key 还在 map 里才产出」
- [ ] Commit：`feat(interp): map 迭代快照（Task 49，T-CON-49/50）`

**Verify:** `… cargo test -p rgoc-driver --test con_rangefunc`（≥2）

### Task 50: defer 边界判据

**Files:** Test `con_defer.rs`（实现不动，除非判据揭出真错）

- [ ] 先写 `T-CON-51` 的四条（命名返回值 / 嵌套倒序 / panic 展开 / recover 同帧）
- [ ] 若任一条红 ⇒ 先确认是**实现错**还是**判据错**，再修（不许改判据迁就实现）
- [ ] Commit：`test(interp): defer 边界四条（Task 50，T-CON-51）`

**Verify:** `… cargo test -p rgoc-driver --test con_defer`（≥4）

### Task 51: 修 `check` 的 comma-ok 接收

**Files:** Modify `rgoc/crates/rgoc-check/src/*`（赋值数判定），Test `rgoc-check/tests/*`

**Interfaces:**
- Consumes: `F2` 的实测（`v, ok := <-c` 报 `CHK-INVALID-ASSIGN`）
- Produces: 接收表达式在「右唯一项、左两个」时算**两个值**

- [ ] 先写会红的测试：`v, ok := <-c` 零诊断；`v, ok := m[k]` 仍需零诊断（回归）；`v, ok := f()` 单返回值仍报错
- [ ] 实现：把 `<-c` 的「值个数」判定接进现有的 comma-ok 判定（与 `m[k]` 同源）
- [ ] Commit：`fix(check): comma-ok 接收（Task 51，T-CON-34）`

**Verify:** `… cargo test -p rgoc-check`（全绿，新增 ≥3）

### Task 52: 三块补齐后的 M5 零回归

**Files:** 无（判据）

- [ ] 跑 `run-collide.sh --run` ⇒ 69 绿，且 stdout 与 M7 收口时逐字节相同
- [ ] 若有一条变红 ⇒ 定位是 map 快照还是 defer 边界引起的，写进 `M8-tests.md` §9.2
- [ ] Commit：`test(m8): M5 顺序语义零回归（Task 52，T-CON-58）`

**Verify:** `… bash tools/run-oracle/run-collide.sh --run` ⇒ `69 绿 / 0 红`

---

# Phase 6：语料与冻结（Task 53–60）

### Task 53: `run-chan.sh`

**Files:** Create `tools/run-oracle/run-chan.sh`

- [ ] 抄 `run-native.sh` 的骨架：`build` 换成 `run`，退出码三分（0 通过 / 1 静态拒绝 / 其它 = 坏）
- [ ] **沿用 M7 学到的两条**：产物名用 slug（不是 basename）、`if ! cmd; then rc=$?` **不许**用
- [ ] Commit：`feat(m8): run-chan.sh（Task 53）`

**Verify:** `bash tools/run-oracle/run-chan.sh tools/run-oracle/chan-input.txt` 能跑出汇总行

### Task 54: 切片清单与冻结

**Files:** Create `tools/run-oracle/chan-input.txt`、`chan-rejected.txt`

- [ ] `chan-input.txt` = `test/chan/*.go` 的 19 行
- [ ] 实测：`N' = 7`、`M' = 12`，逐行进 `chan-rejected.txt`（三栏：路径/类别/理由）
- [ ] Commit：`test(m8): 并发语料切片（Task 54，R-6-1 同源）`

**Verify:** `wc -l tools/run-oracle/chan-input.txt` ⇒ 20（含 1 行注释）；`chan-rejected.txt` 12 行

### Task 55: 三栏汇总

**Files:** Modify `run-chan.sh` 的汇总行

- [ ] 汇总行固定写 `自检 a/5 + 仅有输出 b/0 + 空跑 c/2（合计 ok/skip/bad / 19）`
- [ ] `c` 不等于 2 ⇒ 打印警告并退出码 1（语料性质变了）
- [ ] Commit：`feat(m8): 三栏汇总（Task 55，T-CON-52）`

**Verify:** 汇总行三栏齐全，`ok=7 skip=12 bad=0`

### Task 56: `con_slice.rs` 两向断言

**Files:** Create `rgoc/crates/rgoc-driver/tests/con_slice.rs`

- [ ] `N' + M' == 19`；`M'` 集合 == `chan-rejected.txt`；每条类别 ∈ {`errorcheck`,`stdlib`}；`perm` 在 `M'` 里且理由含 `errorcheck`
- [ ] Commit：`test(m8): 切片冻结断言（Task 56，T-CON-54）`

**Verify:** `… cargo test -p rgoc-driver --test con_slice`（≥4）

### Task 57: manifest 骨架

**Files:** Create `docs/milestones/M8-manifest.json`

- [ ] 照 `M7-manifest.json` 的形状：`status` / `gate`(9) / `unsupported`(8) / `corpus_lists` / `budget` / `open_gaps` / `measured` / `phase_summary`
- [ ] `gate` 全 `pending`；`done: false`
- [ ] Commit：`docs(m8): manifest 骨架（Task 57）`

**Verify:** `python3 -c "import json;json.load(open('docs/milestones/M8-manifest.json'))"`

### Task 58: `manifest_m8.rs` 对账

**Files:** Create `rgoc/crates/rgoc-driver/tests/manifest_m8.rs`

- [ ] 自洽断言：`gate` 9 条、`unsupported` 8 条、`N'+M'` 与清单文件一致、每条 unknown 码 ∈ 拒绝码集合
- [ ] 五类谎的合成反例 ≥ 9 种（标 pass 却自认未兑现 / 非 pass 无缺口 / 数字自报 / 码不在集合 / `done` 与全 pass 同真同假）
- [ ] Commit：`test(m8): manifest 对账（Task 58，T-CON-60）`

**Verify:** `… cargo test -p rgoc-driver --test manifest_m8`（≥2）

### Task 59: 变异 8 条

**Files:** Create `scripts/mutation-m8.sh`（4 条）、`scripts/mutation-m8-notes.md`（全 8 条）

- [ ] 每条：`cargo clean -p` → 改 → 跑指定测试 → 必须 `FAILED` → 恢复 → 再 clean
- [ ] 8 条全红；把「没红」的三条（若有）写进 notes 的诊断，不藏
- [ ] Commit：`test(m8): 变异 8 条（Task 59）`

**Verify:** `bash scripts/mutation-m8.sh` ⇒ 末尾 `红 4 / 失手 0`；notes 里 8 条各有记录

### Task 60: 缩减脚本

**Files:** Create `scripts/shrink-m8.sh`

- [ ] 输入一个失败的 `.go` + 命令；从后往前逐语句删除，仍失败就保留 ⇒ 输出最小复现
- [ ] 只支持函数体内的语句级删除（不做语法树级简化），文档写清
- [ ] Commit：`feat(m8): 缩减脚本（Task 60，T-CON-61）`

**Verify:** 对一条故意失败的多 goroutine fixture 跑，缩到 ≤3 行仍失败

---

# Phase 7：收口（Task 61–66）

### Task 61: 四条 cargo

- [ ] `cargo fmt --all -- --check` ⇒ 0
- [ ] `cargo check --workspace --all-targets` ⇒ 0 error
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` ⇒ 0
- [ ] `cargo test --workspace --no-fail-fast` ⇒ 0 failed（记录 passed 数）
- [ ] Commit：`chore(m8): 四条 cargo（Task 61）`

**Verify:** 四条的退出码全 0

### Task 62: M5 / M6 / M7 零回归

- [ ] `run-collide.sh --run` ⇒ 69 绿；`--ssa` ⇒ 46 绿 23 跳过 0 红
- [ ] `run-native.sh` ⇒ 39/7/0
- [ ] `check-m1-consistency.py` ⇒ 全过
- [ ] Commit：`test(m8): 三个里程碑零回归（Task 62，T-CON-58/59）`

**Verify:** 四组数字与 M7 收口时一致

### Task 63: 报告与冻结

**Files:** Create `docs/milestones/M8-report.md`

- [ ] §1 读数（测试/语料/门禁/变异）；§2 交付清单；§3 关键决策的实测复核；§4 **并发语料三栏**；§5 真错清单（按「为什么语料抓不到」分类）；§6 白名单 8 条；§7 与 M5/M6/M7 的边界；§8 **冻结报告**（冻结什么、不承诺什么）；§9 接手指南
- [ ] Commit：`docs(m8): 报告与冻结（Task 63）`

**Verify:** 报告里每个数字都能从命令复现（写命令，不写结论）

### Task 64: 契约定稿

- [ ] `C11` 定稿（把执行期的 `R-*` 折回正文）
- [ ] `C3` 增补（六节点 + 求值顺序）
- [ ] `C9` 增补（死锁、goroutine panic、map 快照）**并去掉「候选」字样**
- [ ] `C4`/`C10` 不改，写明理由（SSA 继续拒 chan）
- [ ] Commit：`docs(m8): 契约定稿（Task 64）`

**Verify:** `grep -n '候选' docs/contracts/C9-values-and-output.md` ⇒ 无「冻结候选」字样

### Task 65: 文档回写

- [ ] `docs/README.md`：M8 五行齐、契约清单加 C11
- [ ] `AGENTS.md`：一句话状态 + crate 地图（无新 crate，但要写「M8 已收口」）
- [ ] `GAP-01.md`：加 M8 节（`RT-SCH-01/04/05/06` ✅、`02/03/07/08` ✗ 逐条理由）
- [ ] `docs/03-roadmap.md`：M8 状态节
- [ ] Commit：`docs(m8): 文档回写（Task 65）`

**Verify:** `python3 scripts/check-m1-consistency.py` ⇒ 全过

### Task 66: 全量回归与 done

- [ ] 再跑一遍四条 cargo + 三套语料
- [ ] `M8-manifest.json`：`gate` 按**实测**填、`done: true`、`open_gaps` 只留真的
- [ ] `… cargo test -p rgoc-driver --test manifest_m8` ⇒ 绿
- [ ] Commit：`docs(m8): 收口（Task 66）`；push

**Verify:** `manifest_m8` 自洽 + 反例全红；`git status` 干净

---

## 附录 A：Task ↔ 门禁 ↔ 测试 ID

| Phase | Task | 门禁 | 主要 ID |
|---|---|---|---|
| 0 | 1–6 | —（装置） | — |
| 1 | 7–16 | `CON-1`（非阻塞半边） | `T-CON-01/02` |
| 2 | 17–28 | `CON-1/2/3` | `T-CON-11..24` |
| 3 | 29–38 | `CON-4` | `T-CON-25..34` |
| 4 | 39–44 | `CON-5` | `T-CON-35..42` |
| 5 | 45–52 | `CON-8/9` | `T-CON-43..51/58` |
| 6 | 53–60 | `CON-6/7` | `T-CON-52..57/60/61` |
| 7 | 61–66 | `CON-9` | `T-CON-58/59` |
| 横切 | 59 | — | `T-CON-62..69` |

## 附录 B：本计划里刻意**不**做的事

| 不做 | 理由 | 归谁 |
|---|---|---|
| 事件日志与重放 | 确定性 FIFO 已让「同 seed 同结果」自然成立；日志是冗余的新格式 | `C11` §5 |
| 抢占 | 会让 `(源码, seed, 预算)` 不再唯一决定执行 | `U-M8-01` |
| range-over-func 的 `return` 穿越 | 需要 `Unwind::Return` 穿过迭代器 defer，是 M8 最贵的一块且零语料 | M8 之后 |
| 给 11 个 stdlib chan 语料做桩 | 桩本身是另一套要判据的东西 | 不立项 |
| `ChanObj` 的容量上限 | 与 map/slice 同口径（内存靠宿主） | 不做 |
