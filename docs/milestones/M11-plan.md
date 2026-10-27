# M11 计划 · native 并发（栈 / 调度 / channel / select）

> 输入：`M11-design.md`（`D-M11-1..12` / `F1..F12` / `GO-1..GO-7`）、
> `M11-tests.md`（`T-STK-*` / `T-GO-*` / `T-CHAN-*` / `T-SEL-*` / `T-EXIT-*` /
> `T-GROOT-*` / `T-GCORP-*` / 15 条变异）。
> 范围：**M11a / M11b / M11c**。M11d/e/f 不在本计划内（`M11-design.md` §1.3）。
> **前置**：`Phase -1 / Task 0`（查清 `G-M10-06`）**已完成**（2026-10-27），门禁解除
> （`M11-design.md` §2.5）。

---

## Global Constraints

1. **M:1 协作式，不抢占**（`D-M11-1`）。任何「按时间/步数切走」的实现都越界。
2. **活值只在栈上**：切换把 callee-saved 压进被挂起 G 的栈（`D-M11-3`）。
   **`gc_precise.rs::t_gc_10` 不许为 M11 改** —— 它红了说明切换方案错了。
3. **`hchan`/`Sudog` 的布局冻结**（`C11` §7）：`T-SCH-01` 必须一直绿。
4. **a/b 阶段强制关 GC**（`D-M11-12`）：并发模块 ⇒ `mode=off`，
   即使设了 `RGOC_GC_STRESS`。
5. **单线程语料 69 个零回归**：每个 Phase 收尾都要跑 `run-native.sh` 三态。
6. **每一步都要能回退**：chan 的四操作先「不阻塞直通」，再加阻塞。
7. **判据先写、变异实证**：新增判据必须配一条变异；绿的变异先怀疑判据。
8. **Task 0 已完成**（2026-10-27）：`G-M10-06` 查清 —— 根因是 `rt_defer_push`
   的「分配未发布」窗口，修法 `gc_pause`（`C12` §6.1 / `R-10-24`）。
   `t_gc_12` 是它的守卫；**改 `rt_defer_push` 时这条必须一直绿**。

---

## Review Focus

- 阶段收口时只看三件事：**门禁读数**（`GO-*`）、**语料三层一致**、**变异全红**。
- 每个 Phase 的 commit 必须写「为什么先前没抓到」（M9/M10 的惯例）。
- `mode=` 的翻转（a/b → c）是**唯一**允许的判据翻转，且必须两侧都留判据。

---

# Phase -1 · 前置调查（Task 0）—— ✅ 已完成（2026-10-27）

> **这一 Phase 不属于 M11 的实现范围**，它是 `M10` 留下的 `G-M10-06`。
> 2026-10-27 拍板：**查清才能开工**（`M11-design.md` §2.5）。
> 产出是「证据 + 一句机制说明」，不是代码功能。判据 `T-INV-01..05`。
>
> **结果**：根因是 `rt_defer_push` 的「分配未发布」窗口，不是位图；
> 修法 `gc_pause`（`C12` §6.1 / `R-10-24`）+ 守卫 `t_gc_12`。
> 完整记录 `scripts/g-m10-06-probe.md`。**门禁解除，Phase 0 可开工。**

### Task 0: 查清 `G-M10-06`（真 live-in 位图为什么让四个语料坏掉）

**Files:**
- Modify（**临时**，查完按结论定稿）: `rgoc/crates/rgoc-ssa/src/live.rs`（第 330–339 行）
- Modify: `docs/milestones/M10-manifest.json`（`deferred_gaps` → `closed_gaps`）
- Modify: `docs/contracts/C12-gc.md`（若结论是「必须保留超集」则写成不变式）
- Create: `scripts/g-m10-06-probe.md`（实验记录，**不是**脚本产物，是留档）

- [x] **Step 1: 先留原文副本，再动手。**
  `cp rgoc/crates/rgoc-ssa/src/live.rs /tmp/live.rs.orig`。
  ⚠️ **不许用 `git checkout` 复原** —— M10 Phase 0 踩过，未提交的改动会被一起丢掉。
  复原一律 `cp` 回来 + **`touch`**（`mv`/`cp -p` 保留旧 mtime 会让 cargo 复用旧 rlib）。

- [x] **Step 2: 写复现装置（`T-INV-01`）。**
  一个脚本，把 `set` 改成 `(after[ii] − ins.dsts()) ∪ ins.uses()`，`cargo clean -p
  rgoc-ssa`，重编译，然后对四个语料逐个跑 `run-native.sh` 的单条通路，**把
  stdout / stderr / rc 三样原样存下来**。四个语料名取自
  `M10-manifest.json::deferred_gaps[0].what`：`defernil` / `print` / `nilptr2` / `method5`。

- [x] **Step 3: 记录失败形态，逐个填表。**
  每个语料填：`改了之后 rc 从 ? 变成 ?` / `stdout 差在哪一行` / `是不是只在
  stress 态坏`。⚠️ **四份都要留**，包括「改了没坏」的那个 —— 如果某个语料
  其实没坏，那 manifest 的描述就是错的（`T-INV-01` 的注）。

- [x] **Step 4: 最小化（`T-INV-02`）。**
  挑最小的那个语料，用「删到不再坏」的方式切出最小复现。目标是把病灶钉到
  **一条指令**上（`MakeClosure` / `Convert` / `Bin` 三选一），并写出这条指令前后
  的 `sp_idx`、位图、以及「GC 在哪一刻跑」。

- [x] **Step 5: 三条假设各自的判定实验（`T-INV-03`）。**
  **必须三条都做**，不能「A 看起来对就收工」：

  | 假设 | 判定实验 | 证实的样子 |
  |---|---|---|
  | **A** 被调方内部二次 GC | 给 `gc_alloc` 加探针，记录「本次 GC 时，触发方 `sp_idx` 对应的位点是否含本条指令的 `dst`」，且记录 dst 槽**当时的值**（0 / 新地址 / 旧地址）| GC 跑时 dst 槽**已是新地址** ⇒ A 成立（位图必须显式含 def）|
  | **B** `sp_idx` 配对偏差 | 拿 `.stackmap` 的位点序列与 `.s` 里每条 `sp_idx` 的写入值**逐条对账**（M10 的 `nat7` 已有这个骨架）| 出现「写入值 ≠ 位点下标」⇒ B 成立（修 `sp_idx`，不改位图语义）|
  | **C** 槽复用 | 在最小复现上，把 dst 槽**执行前**的值打印出来（不是猜），看它是否等于某个活对象的地址 | dst 槽旧值 = 活对象唯一引用 ⇒ C 成立（超集是**必须**的，写成不变式）|

  ⚠️ **「三条都可能」不算结论**。恰好一条证实、两条证伪，且证伪要写理由
  （例：「C 证伪：最小复现里 dst 槽执行前是 0」）。

- [x] **Step 6: 结论落地（`T-INV-04`）。**
  - `M10-manifest.json`：`G-M10-06` 移出 `deferred_gaps` → `closed_gaps`，
    `resolution` 写机制（一句话能让人复现的那种），或
  - 若结论是「超集是必须的」：`C12` 加一条**显式不变式**（位图 = 执行前 ∪ 执行后，
    并说明为什么不是 live-in），`G-M10-06` 进 `closed_gaps` 且 `resolution`
    指向那条不变式。
  - **`live.rs` 第 330–339 行那段注释：「机制没查清」必须删掉**，换成机制说明
    + 一条能反向验证的判据 ID。

- [x] **Step 7: 复原 + 不回归（`T-INV-05`）。**
  `cp /tmp/live.rs.orig` 回原位 + `touch` + `cargo clean -p rgoc-ssa`；
  `run-native.sh` 三态各 69/0/0；四条 cargo 门禁全绿。

- [x] **Step 8: Commit** `docs(m10): 查清 G-M10-06 —— <一句话机制>（M11/Task 0）`

> ⚠️ **Task 0 没过不许开 Task 1**（`M11-design.md` §2.5 的硬门禁）。
> 如果 Step 5 三条假设全被证伪 —— 那不是失败，是「病灶在第四处」，
> 回来改 §2.5 的假设表再查，**不许**带着「不知道」进 Phase 0。

---

# Phase 0 · 契约与装置（Task 1–7）—— ✅ 已完成（2026-10-27）

### Task 1: 修拒绝面（`F3`）

**Files:**
- Modify: `rgoc/crates/rgoc-ssa/src/effect.rs`、`rgoc/crates/rgoc-ssa/src/build.rs`

- [x] **Step 1: 写会红的判据** `go_corpus.rs::t_gcorp_03_并发语料不_panic`。
  ⚠️ **靶子修正（实测）**：初稿写「12 个 `M'` 的退出码没有 101」，但那 12 个
  本来就全是 rc=1（走 `import`/`check` 路径，**到不了** `effect.rs`）；真正
  rc=101 的是 **`N'` 里的 6 个**：`select` / `select4` / `select6` / `select8` /
  `sendstmt` / `sieve1`。判据两半都查（`M'` 守既有面、`N'` 查 `F3`），并用
  `catch_unwind` 逐个跑 —— 否则第一个 panic 就打死测试进程，只能看到一个文件。
- [x] **Step 2: 定位**：`effect.rs::scan_stmt` 对 `Stmt::Go/Send/Select` 是
  `unreachable!()`，而**效果摘要在 `unsupported()` 之前算**（`build` 的调用序）。
- [x] **Step 3: 修法二选一**（选哪个写进 commit）：
  a. 把 `unsupported()` 前移到效果摘要之前；
  b. `effect.rs` 对这三条**不 panic**（记 `writes_all = true; may_panic = true`），
     让拒绝面继续由 `unsupported()` 负责。
  建议 **b**：效果摘要是**保守近似**，它对任何语句都该有定义；`unreachable!`
  把「预扫描漏了」当成不可达，而预扫描的顺序是调用方的约定，不是类型系统的保证。
- [x] **Step 4**: 12 个全绿 + 69 个单线程语料零回归。
- [x] **Step 5: Commit** `fix(ssa): go/select/send 由 panic 改成干净拒绝（Task 1，F3）`

### Task 2: `C11` 修订

**Files:**
- Modify: `docs/contracts/C11-concurrency.md`

- [x] **Step 1**: §7 开头那句「多核抢占、真锁、真 park/unpark」改成
  「M11a–c 是 **M:1 协作式**（与 §2 同构），`lock` 留位给 M11e」——
  并说明「**布局**与**执行策略**是两件事」（§7 自己已经这么讲，只是开头那句自相矛盾）。
- [x] **Step 2**: §9（变更记录）加一条，引用 `M11-design.md` `D-M11-7`。
- [x] **Step 3: Commit** `docs(m11): C11 §7 措辞修正（M11a–c 是 M:1）`

### Task 3: `C5` / `C9` 的增补

**Files:**
- Modify: `docs/contracts/C5-abi.md`、`docs/contracts/C9-values-and-output.md`

- [x] **Step 1**: `C5` §7 补「**序言栈检查**」：`frame_size > 0` 且非 `off_stack`
  的函数，`sub sp` 之后 `cmp sp, g_stack_lo` + `b.lo`（4 条指令），冷路径
  `bl rt_panic_stack_overflow`。
- [x] **Step 2**: `C5` §6.1（寄存器表）补一句「`d8..d15` 是 callee-saved，
  `rt_switch` 必须保存」—— 否则「没有寄存器根」会被误读成「不用管浮点寄存器」。
- [x] **Step 3**: `C9` §5 的退出码表补两行：栈溢出、goroutine 数超限（都是 2）。
- [x] **Step 4: Commit** `docs(m11): C5 序言栈检查 + C9 两条新文本（Task 3）`

### Task 4: 装置 `run-native-chan.sh`

**Files:**
- Create: `tools/run-oracle/run-native-chan.sh`

- [x] **Step 1**: 沿 `run-native.sh` 的骨架（slug 产物名、`timeout`、三栏汇总）。
- [x] **Step 2**: `N'` 清单**算出来**：`chan-input.txt − chan-rejected.txt`
  （`T-GDEV-01` 断言这一点，不抄第二份清单）。
- [x] **Step 3**: `M'` 的跑法：只 `build`，断言退出码 1（`T-GCORP-02/03`）。
- [x] **Step 4: Commit** `test(m11): 并发语料装置（Task 4）`

### Task 5: 判据骨架（七个测试文件）

**Files:**
- Create: `rgoc/crates/rgoc-driver/tests/go_stack.rs` / `go_sched.rs` /
  `go_chan.rs` / `go_select.rs` / `go_exit.rs` / `go_root.rs` / `go_corpus.rs`

- [x] **Step 1**: 每个文件先放**一条**会红的判据（`T-STK-03` 溢出、
  `T-GO-03` FIFO、`T-CHAN-01` 同步、`T-SEL-01` 求值、`T-EXIT-01` 死锁、
  `T-GROOT-01` 挂起栈、`T-GCORP-03` 不 panic）。
- [x] **Step 2**: 共用装置 `support/go_probe.rs`：`build_and_run_chan(name, src, env)`，
  把「native / 解释器 / 官方」三次跑的结果包成 `Triple { native, interp, oracle }`
  并给出 `assert_same()`。
- [x] **Step 3: Commit** `test(m11): 判据骨架与三层差分装置（Task 5）`

### Task 6: 变异脚本骨架

**Files:**
- Create: `scripts/mutation-m11.sh`（沿 `mutation-m10.sh`，含 `probe` / `probe_rs`）

- [x] **Step 1**: 先把 `M-M11-01`（不存 `d8..d15`）与 `M-M11-07`（`closed` 顺序）
  两条放进去 —— 它们分别对应「切换」与「状态机」两个最贵的面。
- [x] **Step 2: Commit** `test(m11): 变异脚本骨架（Task 6）`

### Task 7: Phase 0 收口

- [x] **Step 1**: `M'` 12/12 干净拒绝；69 个单线程语料三态仍 69/0/0；
  四条门禁全绿。
- [x] **Step 2: Commit** `chore(m11): Phase 0 收口`

---

# Phase 1 · M11a：栈与上下文切换（Task 8–15）

### Task 8: `gdesc` 与栈分配 —— ✅ 已完成（2026-10-27）

> ⚠️ **`t_stk_02`（新 G 的初始 sp 16 对齐）推到 Task 12**：M11a 现在只有 `main`
> 一个 G，`rt_gstack_alloc` 还没有调用者。Task 12 的 `go` 语句才第一次造新 G。
>
> ⚠️ **`gdesc` 用静态数组而不是 `rt_arena_alloc`**：`rt_init` 会调它，走 arena
> 就等于在 `_start` 里触发第一次 `rt_grow`（16 MiB mmap），「空程序只在第一次
> 分配时才 mmap」这条 M10 的惰性行为就变了 ⇒ `allocs`/`heap` 读数全偏移。

**Files:**
- Modify: `runtime/native/aarch64-unknown-linux-gnu/rt.c`

- [x] **Step 1**: `GDesc` 64 字节（`M11-design.md` §3.2），放 **arena**；
  `rt_gnew()`：`mmap` 64 KiB 栈 + 分配 `gdesc` + 填 `stack_lo/hi/sp`。
- [x] **Step 2**: `g_stack_lo` 全局（序言检查读它）；`rt_gset_current(g)` 更新它。
- [x] **Step 3**: 判据 `go_stack.rs::t_stk_02_sp_对齐`：新 G 的初始 `sp` 16 对齐。
- [x] **Step 4: Commit** `feat(rt): gdesc 与 goroutine 栈（Task 8，D-M11-2）`

### Task 9: `rt_switch`（汇编）—— ✅ 已完成（2026-10-27）

> **读数**：`RGOC_G11_SELFTEST=1` ⇒ rc=0（18 个 callee-saved 逐个不变）；
> 变异「不存 d8/d9」⇒ rc=**9**（自测失败，判据有牙）。
>
> ⚠️ **两个坑**（都写进了 `rt.c` 的注释）：
> ① 链接必须 `-no-pie` —— PIE 下全局符号不能用 `adrp + :lo12:` 直接寻址
> （`relocation R_AARCH64_ADR_PREL_PG_HI21 … can not be used when making a
> shared object`），而 `rt_switch` 自测要直接引用全局；
> ② **自测函数自己必须守 ABI** —— `rt_sc_run` 往 18 个 callee-saved 里填值，
> 不先保存 C 的那份就会返回时踩烂调用方（`rc=139`，而且现象**看起来像
> `rt_switch` 切坏了**）。

**Files:**
- Modify: `rt.c`（`__asm__` 内联或独立 `.s` 段）

- [x] **Step 1**: 160 字节保存区（`x19..x28` / `x29` / `x30` / `d8..d15`），
  对称的 `stp`/`ldp`（`M11-design.md` §3.3）。
- [x] **Step 2**: 判据 `t_stk_01_callee_saved_全保住`：fixture 往每个寄存器放
  **可区分值**（`0x1111…` / `1.5` / `-2.25` …），切 1000 次后逐个比。
- [x] **Step 3**: 判据 `t_stk_02`：切 1000 次后 `sp % 16 == 0`。
- [x] **Step 4: Commit** `feat(rt): rt_switch（Task 9，D-M11-3）`

### Task 10: 序言栈检查 —— ✅ 已完成（2026-10-27）

> **读数**：`t_stk_03`（溢出 rc=2 + 文本）/ `t_stk_04`（序言成对检查）/
> `t_stk_05`（`nilptr2` 不误报）全绿。
>
> ⚠️ **`t_stk_06`（溢出深度实测）推到 Task 12**：现在只有 `main`，它用进程栈
> （4 MiB）。「64 KiB 够不够 `sieve1.go`」要在**新 G 的 64 KiB 栈**上测 —— 那
> 是 Task 12 的 `go` 语句之后的事。
>
> ⚠️ **两个实测修正**：
> ① `stack_lo = sp - 8 MiB` **来不及**（正好在栈底之外，进程先踩 guard page
> ⇒ rc=139）⇒ 改成 **4 MiB**（留一半余量）；
> ② 溢出文本里的 `<N>` 必须**动态算**（`g_cur->stack_hi - stack_lo`），不能写死
> 65536 —— 否则 `main` 的溢出会报一个假数字。而两个 `u64 *` 相减是**按元素
> 个数**算的（C 的指针算术）⇒ 实测把 4 MiB 报成 512 KiB，必须先转 `u64`。

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/isel.rs`（`prologue`）、`rt.c`（`rt_panic_stack_overflow`）

- [x] **Step 1**: `frame_size > 0 && !fr.off_stack()` 时发 4 条检查 + 冷路径。
- [x] **Step 2**: `rt_panic_stack_overflow()`：`runtime error: goroutine stack
  exceeded <N> bytes` + 退出 2（`C9` §5 新行）。
- [x] **Step 3**: 判据 `t_stk_03_溢出报错`（深递归 fixture）+ `t_stk_04_序言有检查`
  （扫 `.s`）+ `t_stk_05_超大帧不查`（`nilptr2.go` 仍绿且 `.s` 里没有检查）
  + **`t_stk_06_溢出深度实测`**（`§2.4 ①` 的修正：实测第 N 层溢出、N 可复现、
  `sieve1.go` 的深度有富余；N 与 M 两个数字进 manifest 的 `measured`）。
- [x] **Step 4: Commit** `feat(arm64): 序言栈检查（Task 10，D-M11-4）`

### Task 11: 切换的最小闭环（单 G 来回切）

**Files:**
- Modify: `rt.c`（`rt_schedule()` 的骨架 + `rt_goexit`）

- [x] **Step 1**: `rt_schedule()`：`g_cur->sp` 由 `rt_switch` 保存，
  从 `runq` 取下一个；队列空 ⇒ §3.9 的判定。
- [x] **Step 2**: 判据：一个 fixture 手工建两个 G（A 打印 1、B 打印 2），
  切一轮后输出 `1 2`（顺序由 FIFO 决定）。
- [x] **Step 3: Commit** `feat(rt): 调度骨架（Task 11）`

### Task 12: `Instr::Go` 与参数打包

**Files:**
- Modify: `rgoc-ssa/src/ir.rs`（`Instr::Go`）、`build.rs`、`live.rs`（safepoint）、
  `isel.rs`、`rt.c`（`rt_go` / `rt_go_trampoline`）

- [ ] **Step 1**: IR：`Go { func, args, nargs, argbytes }`；`dsts()` 空；
  `uses()` = args + func；**是 safepoint**（`live.rs` 的穷尽匹配会强制回答）。
- [ ] **Step 2**: isel：实参在当前 G 的栈上按普通调用布局算好，`bl rt_go`。
- [ ] **Step 3**: `rt_go`：新栈 + 启动帧 `{fn, argbytes, args…}` +
  入队**队尾**（不切走）。
- [ ] **Step 4**: 判据 `t_go_03_就绪队列_fifo` + `t_go_04_参数深拷贝`。
- [ ] **Step 5: Commit** `feat(m11): go 语句与参数打包（Task 12，D-M11-6）`

### Task 13: `unsup_stmt` 放开 `go`

- [ ] **Step 1**: 去掉 `Stmt::Go` 的拒绝；保留 `Send`/`Select`/`Range-Chan`。
- [ ] **Step 2**: 判据：`M'` 里没有「只因 `go` 被拒」的文件（逐个看拒绝原因）。
- [ ] **Step 3: Commit** `feat(ssa): 放开 go 语句（Task 13）`

### Task 14: `T-GO-01` / `T-GO-02` / `T-GO-05`

- [ ] **Step 1**: `t_go_01_m_1`（静态扫描：`.s` 与 `rt.c` 无 `clone`/`pthread`）。
- [ ] **Step 2**: `t_go_02_不抢占`（`for {}` 在 G 里 ⇒ 装置超时 124，
  **124 是期望值**）。
- [ ] **Step 3**: `t_go_05_确定性`（5 个 G 的 fixture 跑两次逐字节相同）。
- [ ] **Step 4: Commit** `test(m11): 调度三规则的另外三条（Task 14）`

### Task 15: Phase 1 收口

- [ ] **Step 1**: `GO-1` 五条全绿；`GO-2` 五条全绿；69 个语料零回归；
  变异 `M-M11-01..04` 全红。
- [ ] **Step 2: Commit** `chore(m11): Phase 1 收口（M11a）`

---

# Phase 2 · M11b：channel 与 select（Task 16–31）

### Task 16: `hchan` 的构造与 `buf`

**Files:**
- Modify: `rt.c`（`rt_makechan`）、`isel.rs`（`make(chan T[, n])`）

- [ ] **Step 1**: `Hchan` 96 B（`C11` §7 逐字段）；`buf` 走 `rt_alloc_array`
  （`elem = elemtype`，`n = dataqsiz`）⇒ GC 能扫。
- [ ] **Step 2**: 判据 `T-SCH-01` 仍绿（M9 的布局对账）+ `zerosize.go` 能建
  `chan [0]byte` / `chan struct{}`（`dataqsiz` 可以是 1 而 `elemsize == 0`）。
- [ ] **Step 3: Commit** `feat(rt): make(chan) 与 hchan 实现（Task 16，D-M11-7/8）`

### Task 17: `send` / `recv` 的**不阻塞直通**

- [ ] **Step 1**: 先实现「有缓冲且未满/非空」与「无缓冲且对端已在等」两条，
  **不实现阻塞**（阻塞路径先 `rt_panic_unimpl`）。
- [ ] **Step 2**: 判据 `t_chan_02_缓冲_fifo`（`cap=3` 发 3 收 3）。
- [ ] **Step 3: Commit** `feat(rt): chan 直通路径（Task 17）`

### Task 18: `Sudog` 与阻塞

- [ ] **Step 1**: `Sudog` 48 B 进托管堆（手写 desc：`ptrdata=40`，`gcdata=0x1F`）。
- [ ] **Step 2**: 阻塞 = 登记 `Sudog` + `g_cur->state=blocked` + `rt_schedule()`。
- [ ] **Step 3**: 判据 `t_chan_01_无缓冲同步` + `t_chan_08_sudog_不泄漏`
  （10 万次收发后 `live` 有界）。
- [ ] **Step 4: Commit** `feat(rt): Sudog 与阻塞（Task 18，D-M11-8）`

### Task 19: `close` 与三条 panic

- [ ] **Step 1**: `closed` 检查**排第一**（`C11` §3.2 的 ⚠️）；三条 panic 文本。
- [ ] **Step 2**: `close` 唤醒 `recvq` **全部** + `sendq` **全部**。
- [ ] **Step 3**: 判据 `t_chan_03_closed_排第一` + `t_chan_04_close_后排空`。
- [ ] **Step 4: Commit** `feat(rt): close 与三条 panic（Task 19）`

### Task 20: `len` / `cap` / nil 通道

- [ ] **Step 1**: `len` 数 `buf`、**不数** `sendq`；nil 通道 `<-c`/`c <- v`
  **永久阻塞且不登记**（`C11` §3.4）。
- [ ] **Step 2**: 判据 `t_chan_05_len_cap` + `t_chan_07_nil_永久阻塞`（死锁）。
- [ ] **Step 3: Commit** `feat(rt): len/cap 与 nil 通道（Task 20）`

### Task 21: `range` over chan

- [ ] **Step 1**: `unsup_stmt` 放开 `RangeKind::Chan`；isel 下降成
  「循环 recv 到 `closed`」。
- [ ] **Step 2**: 判据：`range` 一个 `cap=3` 的已关闭通道 ⇒ 3 个值后退出。
- [ ] **Step 3: Commit** `feat(m11): range over chan（Task 21）`

### Task 22: `select` 阶段①（求值）

- [ ] **Step 1**: IR `Instr::Select { cases, token, dsts }`：cases 里带
  「求值后的 chan 值 + send 的右值」；求值在**进入 select 之前**完成。
- [ ] **Step 2**: 判据 `t_sel_01_求值一次` + `t_sel_02_走_default_也求值`。
- [ ] **Step 3: Commit** `feat(m11): select 阶段一（Task 22，D-M11-9）`

### Task 23: `select` 阶段②③（试探与阻塞）

- [ ] **Step 1**: `WaitSet` 块（托管堆）+ token；阶段②按源码顺序扫；阶段③
  给每个非 nil case 登记 sudog（共用 token）后换 G。
- [ ] **Step 2**: 判据 `t_sel_03_多就绪取第一个`。
- [ ] **Step 3: Commit** `feat(rt): select 试探与阻塞（Task 23）`

### Task 24: `select` 的撤销

- [ ] **Step 1**: 被唤醒后：拿 token 去所有队列删自己 → 看 `done` 定 winner →
  只跑 winner。
- [ ] **Step 2**: 判据 `t_sel_04_撤销` + `t_sel_05_重复_channel`。
- [ ] **Step 3: Commit** `feat(rt): select 撤销（Task 24，C11 §4.2）`

### Task 25: 空 `select`

- [ ] **Step 1**: `select {}` ⇒ 没有队列可登记 ⇒ 直接走死锁判定（**不是**超时）。
- [ ] **Step 2**: 判据 `t_sel_06_空_select`。
- [ ] **Step 3: Commit** `feat(rt): 空 select（Task 25）`

### Task 26: 死锁判定

- [ ] **Step 1**: 三条条件同时成立 ⇒ `fatal error: all goroutines are asleep -
  deadlock!` + 退出 2（stderr，**不进合并流**）。
- [ ] **Step 2**: 判据 `t_exit_01` + `t_exit_05`（三条条件各自必要）。
- [ ] **Step 3: Commit** `feat(rt): 死锁判定（Task 26，C11 §5.2）`

### Task 27: `main` 返回即退出

- [ ] **Step 1**: `main` 的帧返回 ⇒ `rt_exit(0)`，不跑其它 G 的 defer。
- [ ] **Step 2**: 判据 `t_exit_02` + `t_exit_03`。
- [ ] **Step 3: Commit** `feat(rt): main 返回即退出（Task 27，C11 §5.1）`

### Task 28: G 内 panic

- [ ] **Step 1**: 任一 G 未捕获 panic ⇒ 退出 2，文本不包装（`C9` §5）。
- [ ] **Step 2**: 判据 `t_exit_04`。
- [ ] **Step 3: Commit** `feat(rt): G 内 panic 的退出（Task 28）`

### Task 29: goroutine 数上限

- [ ] **Step 1**: 1024 个硬上限（`M11-design.md` §4.4）；超了
  `runtime error: too many goroutines (limit 1024)` + 退出 2。
- [ ] **Step 2**: 判据：`for i:=0;i<2000;i++ { go f() }` ⇒ 该文本。
- [ ] **Step 3: Commit** `feat(rt): goroutine 数上限（Task 29）`

### Task 30: `GO-3`/`GO-4`/`GO-5` 的差分判据

- [ ] **Step 1**: `t_chan_09_与解释器逐格差分`：同一串操作脚本两边跑。
- [ ] **Step 2**: `t_chan_06_双次深拷贝`。
- [ ] **Step 3: Commit** `test(m11): chan/select 的差分判据（Task 30）`

### Task 31: Phase 2 收口

- [ ] **Step 1**: `GO-2..GO-5` 全绿；7 个 `N'` 三层一致（`T-GCORP-01`）；
  69 个语料零回归；变异 `M-M11-05..12` 全红。
- [ ] **Step 2**: `mode=off` 强制生效（`T-GROOT-04`）。
- [ ] **Step 3: Commit** `chore(m11): Phase 2 收口（M11b）`

---

# Phase 3 · M11c：多 G 根扫描（Task 32–39）

### Task 32: `g_frames` 变成「当前 G 的链头」

**Files:**
- Modify: `rt.c`（`rt_switch` 里搬 `frames`）、`isel.rs`（**不改**）

- [ ] **Step 1**: `rt_switch` 前后：`g_cur->frames = g_frames` /
  `g_frames = next->frames`。**序言/尾声一个字节不改**（`D-M11-10` 的第一种改法）。
- [ ] **Step 2**: 判据：M10 的 `t_root_09..14` 全部仍绿（这是**不许改判据**的那批）。
- [ ] **Step 3: Commit** `feat(rt): g_frames 跟 G 走（Task 32，D-M11-10）`

### Task 33: 扫所有 G 的链

- [ ] **Step 1**: `mark_roots` 增加「遍历所有 gdesc（ready/blocked/exited）→
  `mark_frames(g->frames)`」+ `mark_ptr(g->wait)`。
- [ ] **Step 2**: 判据 `t_groot_01_挂起_G_的栈是根` + `t_groot_02_就绪_G` +
  `t_groot_03_waitset_保活` + **`t_groot_06_多G超集扫描不坏`**
  （Task 0 结论的回归面：N 个 G 轮流阻塞/唤醒、stress 态跑 ⇒ 与 `off` 态逐字节相同、
  `bad_ptr == 0`；每条 `bad_stale` 都能归因到「超集位图」）。
- [ ] **Step 3: Commit** `feat(rt): 多 G 扫根（Task 33，D-M11-11）`

### Task 34: 翻 `mode` 开关

- [ ] **Step 1**: 去掉 `rt_gc_force_off()` 的调用（保留函数，供 `RGOC_GC_FORCE_OFF=1`
  调试用）；`T-GROOT-04` 改成 `T-GROOT-05`（`mode=stress` 且 `collections>0`）。
- [ ] **Step 2**: 判据：并发语料 + `RGOC_GC_STRESS=8` ⇒ 输出不变 + 收过。
- [ ] **Step 3: Commit** `feat(rt): M11c 重新启用 GC（Task 34，D-M11-12）`

### Task 35: 并发语料的三态

- [ ] **Step 1**: `run-native-chan.sh --gc=off|on|stress`：7 个 `N'` 三态各
  7/0/0，且三态 stdout 逐字节相同。
- [ ] **Step 2**: 判据 `t_gcorp_05_并发语料三态`。
- [ ] **Step 3: Commit** `test(m11): 并发语料三态（Task 35）`

### Task 36: 长跑与压力

- [ ] **Step 1**: 10 万个 G（依次创建+退出，不是同时）⇒ `live` 有界、栈被回收
  （`munmap`）。
- [ ] **Step 2**: 判据 `t_groot_06_goroutine_栈不泄漏`（`chunks`/`live` 有界）。
- [ ] **Step 3: Commit** `test(m11): goroutine 栈不泄漏（Task 36）`

### Task 37: 变异 `M-M11-13..15`

- [ ] **Step 1**: 只扫当前 G / `force_off` 空实现 / `Sudog` 回 arena —— 三条全红。
- [ ] **Step 2: Commit** `test(m11): Phase 3 变异（Task 37）`

### Task 38: `C12` 增补（多 G 根）

- [ ] **Step 1**: `C12` §4.1 的根表加第 9 类「其它 G 的栈」；§4.2 说明
  `g_frames` 的语义变成「当前 G 的链头」。
- [ ] **Step 2: Commit** `docs(m11): C12 §4 增补多 G 根（Task 38）`

### Task 39: Phase 3 收口

- [ ] **Step 1**: `GO-6` 五条全绿；`mode` 翻转完成；变异 15/15 全红。
- [ ] **Step 2: Commit** `chore(m11): Phase 3 收口（M11c）`

---

# Phase 4 · 收口（Task 40–46）

### Task 40: 全量回归

- [ ] **Step 1**: 四条 cargo 门禁；`run-native.sh` 三态各 69/0/0；
  `run-native-chan.sh` 三态各 7/0/0；`run-chan.sh`（解释器）仍 7/12/0。
- [ ] **Step 2: Commit** `chore(m11): 全量回归（Task 40）`

### Task 41: `M11-manifest.json`

- [ ] **Step 1**: 门禁 `GO-1..GO-7` 逐条证据、白名单 `U-M11-01..08`、
  `corpus_lists`（7/12）、`measured`（三层读数、G 数、栈用量）、
  `open_gaps` / `closed_gaps`。
- [ ] **Step 2: Commit** `docs(m11): manifest（Task 41）`

### Task 42: `manifest_m11.rs`

- [ ] **Step 1**: 沿 `manifest_m10.rs` 的结构；**M11 特有的一条**：
  `measured.corpus.three_states` 必须能**从一次真实跑**里复现（7/0/0）。
- [ ] **Step 2: Commit** `test(m11): manifest 对账（Task 42）`

### Task 43: 变异笔记

- [ ] **Step 1**: `scripts/mutation-m11-notes.md`：15 条 + 2 条「该绿」的 +
  本轮绿过的成因（M10 的格式）。
- [ ] **Step 2: Commit** `docs(m11): 变异笔记（Task 43）`

### Task 44: `M11-report.md`

- [ ] **Step 1**: 读数 / 真错清单（按「为什么先前没抓到」分类）/ 判据的牙 /
  接手指南 / 遗留。
- [ ] **Step 2: Commit** `docs(m11): 报告（Task 44）`

### Task 45: `GAP-01` M11 节 + 文档回写

- [ ] **Step 1**: `GAP-01.md` 加 M11 节（`RT-MEM-03` ✅、`RT-CMP-05/06` ✅、
  `RT-MEM-05` ✗）。
- [ ] **Step 2**: `README` / `AGENTS.md` / `03-roadmap.md` 的 M11 从「待开工」
  改成「已收口」。
- [ ] **Step 3: Commit** `docs(m11): GAP-01 与文档回写（Task 45）`

### Task 46: 收尾

- [ ] **Step 1**: 台账收口（`.superpowers/sdd/M11-plan/progress.md`）；
  项目记忆 `MEMORY.md` 更新（M11 的硬事实：切换保存区 160 B / `mode` 翻转 /
  `g_frames` 跟 G 走 / **Task 0 查出的位图机制一句话** / 实测栈溢出深度 N）。
- [ ] **Step 2: Commit + push** `chore(m11): 收尾（Task 46）`

---

## 任务 ↔ 门禁对照

| 门禁 | 主要 Task | 判据 |
|---|---|---|
| `GO-1` 栈与切换 | 8–10, 15 | `T-STK-01..06` |
| `GO-2` 调度三规则 | 11–14 | `T-GO-01..05` |
| `GO-3` `hchan` 四操作 | 16–21, 30 | `T-CHAN-01..09` |
| `GO-4` `select` | 22–25 | `T-SEL-01..06` |
| `GO-5` 死锁与退出 | 26–29 | `T-EXIT-01..05` |
| `GO-6` 多 G 根 | 32–36 | `T-GROOT-01..06` |
| `GO-7` 语料 | 1, 4, 31, 35, 40 | `T-GCORP-01..05` |
| **前置**（非门禁）| **0** | `T-INV-01..05` |
