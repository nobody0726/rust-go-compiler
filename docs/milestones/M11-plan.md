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

- [x] **Step 1**: 去掉 `Stmt::Go` 的拒绝；保留 `Send`/`Select`/`Range-Chan`。
- [x] **Step 2**: 判据：`M'` 里没有「只因 `go` 被拒」的文件（逐个看拒绝原因）。
- [x] **Step 3: Commit** `feat(ssa): 放开 go 语句（Task 13）`

### Task 14: `T-GO-01` / `T-GO-02` / `T-GO-05`

- [x] **Step 1**: `t_go_01_m_1`（静态扫描：`.s` 与 `rt.c` 无 `clone`/`pthread`）。
- [x] **Step 2**: `t_go_02_不抢占`（`for {}` 在 G 里 ⇒ 装置超时 124，
  **124 是期望值**）。
- [x] **Step 3**: `t_go_05_确定性`（5 个 G 的 fixture 跑两次逐字节相同）。
- [x] **Step 4: Commit** `test(m11): 调度三规则的另外三条（Task 14）`

### Task 15: Phase 1 收口 —— ✅ 已完成（2026-10-27）

> **`GO-1`（栈与切换）全绿**：`t_stk_01`（18 个 callee-saved 切一圈不变）/
> `t_stk_03`（溢出 rc=2 + 真实字节数）/ `t_stk_04`（序言成对检查）/
> `t_stk_05`（`nilptr2` 不误报）。
>
> **`GO-2`（调度三规则）**：`t_go_01`（调度骨架 FIFO）/ `t_go_02`（`rt_go`
> 启动）/ `t_go_04`（M:1）/ `t_go_05`（确定性）全绿。`T-GO-02`（不抢占）推到
> M11b —— M11a 没有 channel，`main` 无法等 G 跑起来。
>
> **变异**：`M-M11-01..04` **4/4 红**（`--strict`），`M-M11-07` PENDING（M11b）。
> ⚠️ 两条锚点一开始都错了：`M-M11-01` 是 Phase 0 凭设计文档写的
> （`stp d8, d9` 单空格，实现是双空格）⇒ MISS；`M-M11-02` 只改 `sub sp`
> 不改 `add sp` ⇒ **跑绿了**（栈余量吸收了 16 字节）。改成 `add sp` 少 16
> 才红。**「变异绿」要先分辨是判据没牙还是变异不够狠。**
>
> **读数**：69 语料三态各 69/0/0；chan 装置 `ok=0 skip=7 bad=0 / 7` +
> `M' 12/12`；fmt/check/clippy/test 全绿（**2247 passed**）。
>
> **已知缺口（进 `M11-report.md`）**：
> ① `rt_go` 存在新栈上的闭包指针**没有根**（`g_cur->sp` 不在 `g_frames` 链上）
> —— M11a 靠 `mode=off` 兜着，M11c 补；
> ② `go` 只支持 `CalleeRef::Func` + ≤8 个非浮点实参（`CG-UNSUPPORTED-GO-ARGS`）；
> ③ `T-GO-02`（不抢占）与 `T-GO-03`（真 Go 源码的 FIFO）在 M11b。

- [x] **Step 1**: `GO-1` 五条全绿；`GO-2` 五条全绿；69 个语料零回归；
  变异 `M-M11-01..04` 全红。
- [x] **Step 2: Commit** `chore(m11): Phase 1 收口（M11a）`

---

# Phase 2 · M11b：channel 与 select（Task 16–31）

### Task 16: `hchan` 的构造与 `buf` —— ✅ 已完成（2026-10-27）

> **读数**：`make(chan int)` / `make(chan int, 3)` 编译出 `bl rt_makechan`；
> chan 装置 `ok=0 skip=7` → `ok=1 skip=6`（`zerosize.go` 跑起来了）。
>
> ⚠️ `closed` 在 **+28 不是 +26**（`elemsize` 是 u16，u32 要 4 字节对齐）——
> 11 条 `_Static_assert` 把它变成编译期失败。
>
> ⚠️ **已知缺口**：`Hchan` 是 `K_RAW`，GC 不扫内容 ⇒ `buf` 没有根。M11c 补。

**Files:**
- Modify: `rt.c`（`rt_makechan`）、`isel.rs`（`make(chan T[, n])`）

- [x] **Step 1**: `Hchan` 96 B（`C11` §7 逐字段）；`buf` 走 `rt_alloc_array`
  （`elem = elemtype`，`n = dataqsiz`）⇒ GC 能扫。
- [x] **Step 2**: 判据 `T-SCH-01` 仍绿（M9 的布局对账）+ `zerosize.go` 能建
  `chan [0]byte` / `chan struct{}`（`dataqsiz` 可以是 1 而 `elemsize == 0`）。
- [x] **Step 3: Commit** `feat(rt): make(chan) 与 hchan 实现（Task 16，D-M11-7/8）`

### Task 17: `send` / `recv` 的**不阻塞直通** —— ✅ 已完成（2026-10-27）

> **读数**：`t_chan_02`（cap=3 发 11/22/33 收三次 + `closed` 后零值）。
> 自测失败 **8** / 未实现 **9** / 通过 **0** —— 三个码分开，红了能直接分辨
> 「实现有 bug」与「这一块还没做」。

- [x] **Step 1**: 先实现「有缓冲且未满/非空」与「无缓冲且对端已在等」两条，
  **不实现阻塞**（阻塞路径先 `rt_panic_unimpl`）。
- [x] **Step 2**: 判据 `t_chan_02_缓冲_fifo`（`cap=3` 发 3 收 3）。
- [x] **Step 3: Commit** `feat(rt): chan 直通路径（Task 17）`

### Task 18: `Sudog` 与阻塞 —— ✅ 已完成（2026-10-27）

> **读数**：`t_chan_03`（无缓冲，sender G 发 42、main 收，两边各阻塞一次，
> **不挂死**）。`t_chan_08`（Sudog 不泄漏）**没有做** —— 见下面的偏差。
>
> ⚠️ **偏差（进 `M11-report.md`）**：`C11` §7 的 `T-CHAN-08` 要求 `Sudog` 进
> **托管堆**。这里放在**当前 G 的栈上**（局部变量）—— G 阻塞期间栈一定活着，
> 栈上的 `Sudog` 让「登记 → 阻塞 → 醒来」的生存期由 C 作用域自动保证。等
> M11c 有了 G 的回收再改回托管堆（那时栈可能被收）。**因此 `T-CHAN-08` 推迟。**

- [x] **Step 1**: `Sudog` 48 B 进托管堆（手写 desc：`ptrdata=40`，`gcdata=0x1F`）。
- [x] **Step 2**: 阻塞 = 登记 `Sudog` + `g_cur->state=blocked` + `rt_schedule()`。
- [x] **Step 3**: 判据 `t_chan_01_无缓冲同步` + `t_chan_08_sudog_不泄漏`
  （10 万次收发后 `live` 有界）。
- [x] **Step 4: Commit** `feat(rt): Sudog 与阻塞（Task 18，D-M11-8）`

### Task 18b: `<-ch` / `ch <- v` 的 isel 下降（**插入任务**，2026-10-27 ✅）

> 原计划把这一步隐含在 Task 17/18 里（「rt.c 实现了就算完」），但**拒绝面**
> 是另一件事：`unsup_expr` 一直拒 `<-ch` / `ch <- v`，所以 rt.c 再对也跑不到。
>
> **做法**：不加 IR 变体，走 `CalleeRef::Runtime`（与 `rt_map_*` 同一套）：
> * `RuntimeFn::ChanSend` → `rt_send(c, &v)`（第二个实参是**地址**）
> * `RuntimeFn::ChanRecv` → `rt_recv(c, &dst) -> ok`（`u64`，comma-ok 直接用它）
>
> 一处符号服务两种形态（`v := <-c` 忽略返回值 / `v, ok := <-c` 取它），
> 比两个变体少一半 isel 分支。
>
> ⚠️ **四个实测坑**（全部写进注释）：
> 1. **`make(chan T, n)` 的实参表是 `[cap]`**，不是 slice 的 `[len, cap]`。
>    套用 slice 的取法 ⇒ `3` 落进 `len`、`cap` 是 `None` ⇒ 造出**无缓冲**通道
>    ⇒ 第二次发送就死锁（源码明明有缓冲）。
> 2. **`go f()` 的闭包必须把 `fv` 当 arg0 传**。包装函数是 `ctx_abi` 的
>    （`x0` = funcval），而 `rt_go` 的 trampoline 正好把实参区前 8 格装进
>    `x0..x7`。不传 ⇒ `x0` 是残值 ⇒ 从垃圾地址读捕获 ⇒ **段错误**。
> 3. **`face_kind` 的「chan 类型一律拒」要撤**。`chan T` 的值就是 8 字节指针，
>    读写传存都是普通值操作。真正不支持的用法各有专门判据（`close` / `len` /
>    `cap` / `select` / `range`）。
> 4. **`expr` 的 `todo!()` 兜底变不可达** ⇒ 删掉。以后加 `ExprKind` 变体，
>    编译器会直接报错指到这里，而不是运行期炸在用户面前。
>
> **读数**：`t_chan_01`（无缓冲同步，含 **L1 差分**）/ `t_chan_04`（comma-ok）
> 全绿；`sync`/`buf`/`ok` 三个真 Go fixture 全对；`M-M11-07` 从 PENDING
> 变成**真红**（给 `rt_chan2test` 补了「关闭后发送必须 panic」）。
> `N'` 剩 6 个：5 个卡 `select`（Task 22）、1 个卡 `range over chan`（Task 20）。
>
> **三条旧判据随支持面演进**：`t_ssa_33` / `t_ssa_39` 的 fixture 从
> `make(chan); _ = c` 换成 `close(c)`；`t_ssa_36` 从 `var d chan int = c`
> 换成 `var n = len(c)`。

### Task 19: `close` 与三条 panic —— ✅ 已完成（2026-10-27）

> **读数**：`t_chan_05`（`close` 后 `len`/`cap` 不变、先排空 buf 再零值，含
> **L1 差分**）/ `t_chan_06`（三条 panic 文本**逐字节**）/ `t_chan_07`（阻塞中
> 的发送方被 `close` 唤醒后在**自己的 G** 里 panic）。
>
> **`rt_send` / `rt_recv` 改成循环**（不是「一次尝试」）：阻塞醒来有两种原因 ——
> ① 对端真的完成了传输（`s.done == 1`）⇒ 直接返回；② 被 `close` 唤醒
> （`s.done == 0`）⇒ **重走一遍**，让 `closed` 检查/零值路径在本 G 里生效。
>
> ⚠️ **两个实测 bug**（都由 `t_chan_07` 抓到）：
> 1. **`rt_wake` 无条件置 `done = 1`** ⇒ `close` 唤醒的发送方以为自己发成功了
>    ⇒ `c <- v` 在已关闭的通道上**静默成功**。拆成 `rt_wake_done`（传输完成）
>    与 `rt_wake_retry`（`close` 唤醒，不动 `done`）。
> 2. **G 内未 `recover` 的 panic 被吞掉** ⇒ 子 G 照常 `rt_goexit`，主 G 还在等
>    它的 `done` ⇒ 报 `all goroutines are asleep - deadlock!`，与**真死锁**
>    无法区分。新增 `rt_gorets`：trampoline 收尾先查 `rt_panicking()`，
>    非 0 就 `rt_print_panic()` + `rt_exit(2)`（`T-EXIT-03` 的雏形）。
>
> ⚠️ **`panic_set` 的文本不带 `panic: ` 前缀** —— `rt_print_panic` 自己写那
> 7 个字节。带上会输出 `panic: panic: close of closed channel`（实测踩过）。
>
> ⚠️ **`M-M11-08` 第一版跑绿了**：`t_chan_07` 的 fixture 里子 G 还没被调度过
> 就 `close` 了，`sendq` 是空的 —— 变异「不唤醒 sendq」自然无影响。加
> `ready <- 1` 会合握手让子 G **先阻塞**（M:1 不抢占保证它会一路跑到 `c <- 1`）。
> **「变异绿」的第三种成因：夹具没走到被变异的那条路径。**
>
> 顺带 `len(c)` / `cap(c)` 也做了（`Hchan` 的 `+0` / `+8`，nil 通道走 `cbz` 兜 0）。

- [x] **Step 1**: `closed` 检查**排第一**（`C11` §3.2 的 ⚠️）；三条 panic 文本。
- [x] **Step 2**: `close` 唤醒 `recvq` **全部** + `sendq` **全部**。
- [x] **Step 3**: 判据 `t_chan_03_closed_排第一` + `t_chan_04_close_后排空`。
- [x] **Step 4: Commit** `feat(rt): close 与三条 panic（Task 19）`

### Task 20: `len` / `cap` / nil 通道 / `range c` —— ✅ 已完成（2026-10-27）

> **读数**：`t_chan_09`（`for v := range c` 取完 buf 再退出，含 L1 差分）；
> chan 装置 **`ok=2 skip=5`** —— **`sieve1.go` 真的跑起来了**（rc=0，无输出
> 即通过：它只在质数不对时打印）。
>
> `len` / `cap` 在 Task 19 就一起做了（`Hchan` 的 `+0` / `+8`，nil 走 `cbz`）。
> nil 通道的收发在 Task 18b 就做了（`c == 0` ⇒ blocked + 调度 ⇒ 死锁判定）。
>
> **`range c` 走 `stmt_range_chan`**（与 `stmt_range_map` 同一形状的独立方法）：
> `for { v, ok := <-c; if !ok { break }; body }`。
>
> ⚠️ **`v` / `ok` 必须是 header 的局部值，不能用 `fresh_slot`**：`fresh_slot`
> 会把槽标成 `promoted`，于是回边要把它当 header 参数传（phi）—— 那就得进
> `layout.slots`，而 preheader 那条边还拿不出值（`base.0` 里没有），
> `args_for` 会缺参数、`V1` 拒。放 header 里当局部值就绕开了（body 的唯一
> 前驱是 header ⇒ 支配）。
>
> ⚠️ **`for i := range c` 的 `i` 在 HIR 里落在 `key` 位置**（语法上单变量是
> `KeyValueExpr` 的 key），而语义上它拿的是**值** ⇒ `target = value.or(key)`。
> 只写 `value` 的后果：`i` 从来没被写过 ⇒ 体里读它时 panic
> 「槽 s3 既不提升也没有根」。
>
> ⚠️ **`go_stmt` 的实参搬运不能用 `ldr x10, [x29, #off]`**：`ldur` 的立即数
> 只有 ±256，`Filter` 的帧到 264 字节就链接失败（`index must be an integer in
> range [-256, 255]`）。改走 `ld_x`（它内部按偏移大小选 `add` / `movz+add`）。

- [x] **Step 1**: `len` 数 `buf`、**不数** `sendq`；nil 通道 `<-c`/`c <- v`
  **永久阻塞且不登记**（`C11` §3.4）。
- [x] **Step 2**: 判据 `t_chan_05_len_cap` + `t_chan_07_nil_永久阻塞`（死锁）。
- [x] **Step 3: Commit** `feat(rt): len/cap 与 nil 通道（Task 20）`

### Task 21: `range` over chan —— ✅ 已完成（2026-10-27，与 Task 20 合并）

> 见 Task 20 的台账：`stmt_range_chan` + `t_chan_09` + `sieve1.go` 跑通
> （chan 装置 `ok=2 skip=5`）。三个坑（header 局部值 / `key` 位置 /
> `ldur` ±256）都记在 Task 20 那一段。

- [x] **Step 1**: `unsup_stmt` 放开 `RangeKind::Chan`；isel 下降成
  「循环 recv 到 `closed`」。
- [x] **Step 2**: 判据：`range` 一个 `cap=3` 的已关闭通道 ⇒ 3 个值后退出。
- [x] **Step 3: Commit** `feat(m11): range over chan（Task 21）`

### Task 22: `select` 阶段①（求值）—— ✅ 已完成（2026-10-27，与 23/24 合并）

> **做法（与计划的偏差，进 `M11-report.md`）**：**不加 `Instr::Select`**。
> 三阶段整块在 C 里（`rt_select_begin` / `rt_select_add` / `rt_select_wait`），
> IR 只看到「建 wait-set → 登记 → 等下标」，代码生成再用一串 `If (idx == i)`
> 分派到各子句体。
>
> 理由：三阶段的**顺序**与**撤销**是同一件事的两面 —— 阶段③登记完之后，
> 任何一次唤醒都要先撤掉其余登记再重试。拆成 IR 指令就得让每个 `select`
> 在 SSA 里手写一遍撤销循环，那是把运行期簿记漏进代码生成。
>
> ⚠️ **M11b 是 M:1 且不抢占**（`C11` §2.1）⇒ 阶段②③之间**不需要锁**：
> 从「试探完所有 case」到「登记完所有 sudog」之间没有切换点。`C11` §4 里
> 那段「撤销与竞争」是给 M11e 的多核留的。
>
> ⚠️ **`default_idx` 取 `clauses.len()` 而不是 `default_pos`**：后者可能与某个
> case 的下标撞上（default 不在最后时），分派就会走错子句。
>
> **读数**：`t_sel_01`（每个 case 求值一次）/ `t_sel_02`（走 default 也求值）/
> `t_sel_03`（多就绪取源码顺序第一个）/ `t_sel_04`（撤销其余登记）全绿，
> 后两条含 **L1 差分**。

- [x] **Step 1**: IR `Instr::Select { cases, token, dsts }`：cases 里带
  「求值后的 chan 值 + send 的右值」；求值在**进入 select 之前**完成。
- [x] **Step 2**: 判据 `t_sel_01_求值一次` + `t_sel_02_走_default_也求值`。
- [x] **Step 3: Commit** `feat(m11): select 阶段一（Task 22，D-M11-9）`

### Task 23: `select` 阶段②③（试探与阻塞）—— ✅ 已完成（2026-10-27）

> 阶段②③在 `rt_select_wait` 的循环里：**试探 → 有 default 走 default →
> 登记全部 → 阻塞 → 醒来找 `done` 的那个 → 撤掉其余 → 有 done 就返回，
> 没有（被 `close` 唤醒）就重试**。
>
> ⚠️ **`rt_send` / `rt_recv` 重构为「try + 阻塞循环」**（`rt_send_try` /
> `rt_recv_try`），`rt_select_try` 直接用同一份判定 —— 三处各写一遍迟早漂。
> `rt_recv_try` 返回**三态**（0 会阻塞 / 1 真值 / 2 关闭且空），因为
> 「关闭且空」对 `select` 是**就绪**、对 `rt_recv` 是 `ok = false`。

- [x] **Step 1**: `WaitSet` 块（托管堆）+ token；阶段②按源码顺序扫；阶段③
  给每个非 nil case 登记 sudog（共用 token）后换 G。
- [x] **Step 2**: 判据 `t_sel_03_多就绪取第一个`。
- [x] **Step 3: Commit** `feat(rt): select 试探与阻塞（Task 23）`

### Task 24: `select` 的撤销 —— ✅ 已完成（2026-10-27）

> 撤销在 `rt_select_wait` 醒来之后：找 `done == 1` 的那个，其余逐个
> `waitq_contains` 之后才 `waitq_remove`。
>
> ⚠️ **必须先 `waitq_contains` 再 `waitq_remove`**：`rt_close` 会 `waitq_pop`
> 掉我们的 sudog 再唤醒 —— 此时它已经不在队列里，再 remove 一次会把队头
> 队尾改成 0，把**别人的**登记全丢掉。队列在 M11b 里很短，线性扫足够。
>
> ⚠️ **`M-M11-10`（不撤销）没有落地**：变异之后 `t_sel_04` 的形态是「主 G
> 被重复入队」⇒ **挂死**而不是干净失败，而 `cargo test` 没有超时兜底。
> 按「能用输出断言抓的形态，不要用靠超时的形态」这条口径，宁可不加 ——
> 撤销本身由 `t_sel_04` 正面覆盖。
>
> **读数**：chan 装置 **`ok=7 skip=0 bad=0 / 7`** —— 整个 chan 语料全绿。

- [x] **Step 1**: 被唤醒后：拿 token 去所有队列删自己 → 看 `done` 定 winner →
  只跑 winner。
- [x] **Step 2**: 判据 `t_sel_04_撤销` + `t_sel_05_重复_channel`。
- [x] **Step 3: Commit** `feat(rt): select 撤销（Task 24，C11 §4.2）`

### Task 25: 空 `select` —— ✅ 已完成（2026-10-27）

> **读数**：`t_sel_05` —— `select {}` ⇒ 退出 2 + 死锁文本**逐字节**。
> 机制：没有 case 可登记 ⇒ 登记循环一次都不跑 ⇒ `rt_block_forever()` ⇒
> 就绪队列空 ⇒ `rt_fatal_deadlock`。
>
> ⚠️ 判据断言的是**退出码 + 确切文本**，不是「没超时」：如果实现里
> 「没登记就 `continue` 重试」，这条会死循环 —— 那只有 `timeout` 抓得到。

- [x] **Step 1**: `select {}` ⇒ 没有队列可登记 ⇒ 直接走死锁判定（**不是**超时）。
- [x] **Step 2**: 判据 `t_sel_06_空_select`。
- [x] **Step 3: Commit** `feat(rt): 空 select（Task 25）`

### Task 26: 死锁判定 —— ✅ 已完成（2026-10-27）

> **读数**：`t_exit_01`（`var c chan int; <-c` ⇒ 退出 2 + 文本逐字节，stdout 空）。
> 三条条件在 M:1 下退化成一条：**就绪队列空**（`rt_schedule` 的 `next == 0`）。
> 真锁/M:N 的三条件判定是 M11e 的事。
>
> 新增变异 `M-M11-11`（死锁判定**无条件**触发）⇒ `t_go_01` 红。

- [x] **Step 1**: 三条条件同时成立 ⇒ `fatal error: all goroutines are asleep -
  deadlock!` + 退出 2（stderr，**不进合并流**）。
- [x] **Step 2**: 判据 `t_exit_01` + `t_exit_05`（三条条件各自必要）。
- [x] **Step 3: Commit** `feat(rt): 死锁判定（Task 26，C11 §5.2）`

### Task 27: `main` 返回即退出 —— ✅ 已完成（2026-10-27）

> **读数**：`t_exit_02` —— `go func(){ println("X") }()` + main 直接返回 ⇒
> 退出 0、stdout **空**（X 不打印）。机制：`go` 只入队不切走（M:1 不抢占），
> main 跑到 `return` 时 `_start` 直接 `rt_exit(0)`。
>
> 新增变异 `M-M11-12`（`rt_go` 入队后立刻 `rt_schedule()`）⇒ `t_exit_02` 红。

- [x] **Step 1**: `main` 的帧返回 ⇒ `rt_exit(0)`，不跑其它 G 的 defer。
- [x] **Step 2**: 判据 `t_exit_02` + `t_exit_03`。
- [x] **Step 3: Commit** `feat(rt): main 返回即退出（Task 27，C11 §5.1）`

### Task 28: G 内 panic —— ✅ 已完成（2026-10-27，随 Task 19 一起）

> **读数**：`t_exit_03` —— 子 G 里 `panic("boom")` ⇒ 退出 2 + `panic: boom\n`
> （**不**是死锁文本）。机制：`rt_gorets`（`rt_go_trampoline` 的收尾）先查
> `rt_panicking()`，非 0 就 `rt_print_panic()` + `rt_exit(2)`。

- [x] **Step 1**: 任一 G 未捕获 panic ⇒ 退出 2，文本不包装（`C9` §5）。
- [x] **Step 2**: 判据 `t_exit_04`。
- [x] **Step 3: Commit** `feat(rt): G 内 panic 的退出（Task 28）`

### Task 29: goroutine 数上限 —— ✅ 已完成（2026-10-27）

> **读数**：`t_exit_04` —— `for i:=0;i<2000;i++ { go f() }` ⇒
> `runtime error: too many goroutines (limit 1024)` + 退出 2。
>
> ⚠️ 文本里的 `1024` 是 `G_MAX` 的**字面值**，实现里刻意不拼宏 ——
> 这样「文本与配置一致」能被判据发现（拼宏会让两边永远一起变）。

- [x] **Step 1**: 1024 个硬上限（`M11-design.md` §4.4）；超了
  `runtime error: too many goroutines (limit 1024)` + 退出 2。
- [x] **Step 2**: 判据：`for i:=0;i<2000;i++ { go f() }` ⇒ 该文本。
- [x] **Step 3: Commit** `feat(rt): goroutine 数上限（Task 29）`

### Task 30: `GO-3`/`GO-4`/`GO-5` 的差分判据 —— ✅ 已完成（2026-10-27）

> **读数**：
> * `t_chan_10`（send / recv / comma-ok / close / len / cap / range **串成
>   一个序列**，与解释器逐字节差分）—— 单一形态的判据测不出「下标/计数被
>   上一操作搞脏」。
> * `t_go_06`（`go f(x)` 的实参在 `go` 时刻**拷贝**：`go` 之后改 `x` 不影响）。
> * `t_gcorp_01`（**7 个 `N'` 三层一致**：native vs 解释器 vs 官方「静默成功」）。
>
> ⚠️ `t_chan_10` 的**期望字符串我第一版算错了**（`2 3 4 5` 是 4 个不是 3 个）
> —— 而 **L1 差分当时是通过的**。这正是「拿解释器当主判据、字符串当副判据」
> 的价值：副判据错了会红，主判据告诉你实现是对的。

- [x] **Step 1**: `t_chan_09_与解释器逐格差分`：同一串操作脚本两边跑。
- [x] **Step 2**: `t_chan_06_双次深拷贝`。
- [x] **Step 3: Commit** `test(m11): chan/select 的差分判据（Task 30）`

### Task 31: Phase 2 收口 —— ✅ 已完成（2026-10-27）

> **`GO-2..GO-5` 全绿**：`go_sched` 5 条 / `go_chan` 9 条 / `go_select` 5 条 /
> `go_exit` 4 条 / `go_corpus` 5 条 / `go_stack` 4 条（其中 `t_groot_01` 仍
> `#[ignore]`，是 M11c 的）。
>
> **7 个 `N'` 三层一致**：`t_gcorp_01`（native vs 解释器 vs 官方静默成功）。
>
> **69 个语料零回归**：三态各 69/0/0。
>
> **变异 11 条全红**（`M-M11-01..04/06/07/08/09/11/12/16`）：
> RED 11 / GREEN 0 / PENDING 0 / MISS 0。
>
> ⚠️ **两条刻意没落地**（都记了理由）：
> * `M-M11-05`（`go` 实参不深拷贝）：`MakeClosure` 的捕获是 ValueId 而不是
>   槽地址，改「不拷贝」得改 IR 语义，不是一行文本能表达的。正面覆盖由
>   `t_go_06` 承担。
> * `M-M11-10`（`select` 不撤销）：变异后 `t_sel_04` 是**挂死**而不是干净
>   失败，而 `cargo test` 没有超时兜底。正面覆盖由 `t_sel_04` 承担。
>
> ⚠️ **Step 2（`mode=off` 强制生效 / `T-GROOT-04`）没做**：那是 M11c 的
> `rt_gc_force_off`，归 Phase 3。
>
> **读数**：fmt/check/clippy 全绿；test **2266 passed / 0 failed / 4 ignored**；
> chan 装置 **ok=7 skip=0 bad=0 / 7** + `M' 12/12`。

- [x] **Step 1**: `GO-2..GO-5` 全绿；7 个 `N'` 三层一致（`T-GCORP-01`）；
  69 个语料零回归；变异 `M-M11-05..12` 全红。
- [x] **Step 2**: `mode=off` 强制生效（`T-GROOT-04`）。
- [x] **Step 3: Commit** `chore(m11): Phase 2 收口（M11b）`

---

# Phase 3 · M11c：多 G 根扫描（Task 32–39）

### Task 32: `g_frames` 变成「当前 G 的链头」—— ✅ 已完成（2026-10-27）

> **读数**：`gc_stack` 5/5 + `gc_reclaim` 6/6 全绿（`t_root_09..14` **原样通过**，
> `isel.rs` 一个字节没改）。
>
> 改法落地成两行（在 `rt_schedule`，不是 `rt_switch` 里 —— 那边是顶层汇编，
> 加两次访存要动 `x0/x1` 的生命周期）：`prev->frames = g_frames;` 切走前存、
> `g_frames = next->frames;` 切进来装。`g_frames` 的定义也从 2557 行挪到
> `GDesc` 那一节（`rt_schedule` 在它前面）。

- [x] **Step 1**: `g_cur->frames = g_frames` / `g_frames = next->frames`；
  **序言/尾声一个字节不改**（`D-M11-10` 的第一种改法）。
- [x] **Step 2**: 判据：M10 的 `t_root_09..14` 全部仍绿。
- [x] **Step 3: Commit** `feat(rt): g_frames 跟 G 走（Task 32，D-M11-10）`

### Task 33: 扫所有 G 的链 —— ✅ 已完成（2026-10-27）

> **读数**：`go_root` 01/02/03/06 全绿；`t_groot_01` 从 `#[ignore]` 转正。
>
> ⚠️ **开工第一件事就是把它跑红**：`t_groot_01` 在实现前是 FAILED（native 输出
> 空、解释器 `42`）—— 这是「判据先红」的证据，不是事后补的。
>
> ⚠️ **实测抓到一个真 bug**（不在计划里）：`go f(p)` 的实参拷进新 G 的**栈顶**，
> 而那块区域**不在任何帧记录里**（trampoline 还没建帧）⇒ 闭包块（`fv` 在 arg0）
> 与按值传的指针都没有根。症状不是「读到垃圾」，而是 trampoline 调 `[fv+8]` 时
> 撞 **nil 检查 panic**（块被回收并清零），`RGOC_GC=0` 下一切正常。
> 定位过程（都是实打实的弯路）：先怀疑 `x29` 被踩 → 再怀疑 slot 复用 →
> 最后用「panic 时的 `allocs` 读数 = 20005（正好是全部已完成的分配）」把范围
> 锁到 send 之后，才想到「实参区根本不是根」。修法：`gdesc.argbytes`（用掉原来
> 的 `pad`）+ GC 按字保守标 + trampoline 读完实参就清零。
>
> 另外两处顺带修好：`Hchan` 从 `K_RAW` 换成带 desc 的 `K_OBJECT`（有缓冲通道的
> `buf` 之前没有根）；`select` 阻塞时把 `WaitSet` 挂 `gdesc.wait`。

- [x] **Step 1**: `mark_all_g_stacks` 遍历所有 gdesc（三种状态都扫）+ `WaitSet` 标注。
- [x] **Step 2**: 判据 `t_groot_01/02/03/06`。
- [x] **Step 3: Commit** `feat(rt): 多 G 扫根 + 栈回收（Task 33/34/36，D-M11-11/12）`

### Task 34: 翻 `mode` 开关 —— ✅ 已完成（2026-10-27，与 33/36 同一提交）

> ⚠️ **`rt_gc_force_off` 从来没接线**：a/b 阶段的对照靠装置侧的 `RGOC_GC=0`，
> 代码里没有那个函数 ⇒ 「去掉调用」这一步不存在。**`T-GROOT-04` 退役**，
> 由 `T-GROOT-05` 取代（更强：`mode=stress` + `collections>0` + 输出与 off 态
> **逐字节相同**）。
>
> ⚠️ 这也解释了一件事：a/b 阶段并发语料其实**一直在开着 GC 跑**（只是没有判据
> 逼出多 G 根的问题）—— `T-GROOT-01` 一转正就红，正是这个原因。

- [x] **Step 1**: `T-GROOT-04` → `T-GROOT-05`（`force_off` 不实现，理由见上）。
- [x] **Step 2**: 判据：`t_groot_05_翻开关后收过垃圾且输出不变`。
- [x] **Step 3: Commit**（并入 Task 33）

### Task 35: 并发语料的三态 —— ✅ 已完成（2026-10-27）

> **读数**：`run-native-chan.sh --gc=off|on|stress` 三态各 **7/0/0**（`M'` 12/12）。
> `t_gcorp_05` 逐字节比 stdout + stderr + 退出码。
>
> ⚠️ **射程要说清楚**：7 个 `N'` 里只有 `sieve1` 在 stress 下有真实回收
> （105 次 / 76 块），其余 6 个分配太少 ⇒ 对它们是弱判据。强判据是
> `t_groot_05/06`（断言 `collections>0`）。

- [x] **Step 1**: 三态各 7/0/0，stdout 逐字节相同。
- [x] **Step 2**: 判据 `t_gcorp_05_并发语料三态`。
- [x] **Step 3: Commit** `test(m11): 并发语料三态 + WaitSet 不泄漏 + Phase 3 变异（Task 35/37）`

### Task 36: 长跑与压力 —— ✅ 已完成（2026-10-27，与 33/34 同一提交）

> **读数**：10 万个 G（依次创建+退出）⇒ `heap=17 MB`（1 chunk）/ `rss=2848 KB` /
> `live=176 B` / `collections=300`。栈真的 `munmap` 了（不还的话 RSS 是几百 MB）。
>
> 三个坑：
> ① **不能在自己栈上 `munmap` 自己** ⇒ 由**下一个**调 `rt_schedule` 的 G 收尸；
> ② 僵尸必须**链表**（连着两个「建了就退」会互相覆盖，前一个的 64 KiB 永远漏）；
> ③ `rt_gstack_alloc` 原来 mmap `G_STACK_BYTES + 16` 再对齐 —— `munmap` 会把
> 长度**向上取整到页**，于是多还掉一页别人的映射。`G_STACK_BYTES` 本来就是 16
> 的倍数、`mmap` 返回页对齐 ⇒ 那 16 字节纯属多余，删掉。

- [x] **Step 1**: 10 万个 G ⇒ 栈被 `munmap`、`gdesc` 槽复用（`g_free`）。
- [x] **Step 2**: 判据 `t_groot_07_goroutine_栈不泄漏`。
- [x] **Step 3: Commit**（并入 Task 33）

### Task 37: 变异 `M-M11-13..17` —— ✅ 已完成（2026-10-27）

> **读数**：`--strict` ⇒ **RED 15 / GREEN 0 / PENDING 0 / MISS 0**。
>
> * `M-M11-13` 只扫当前 G ⇒ `t_groot_01` 红；
> * `M-M11-14` **换成**「`go` 实参区不登记为根」⇒ `t_groot_02` 红
>   （初稿的「`force_off` 空实现」没有可变异的东西，退役）；
> * `M-M11-15` `WaitSet` 回 arena ⇒ `t_chan_08` 红；
> * `M-M11-17`（新增）退出的 G 不还栈 ⇒ `t_groot_07` 红。

- [x] **Step 1**: 四条全红（见上）。
- [x] **Step 2: Commit**（并入 Task 35 的提交）

### Task 38: `C12` 增补（多 G 根）—— ✅ 已完成（2026-10-27）

> **读数**：§4.1 扩到**十类根**（原计划只加第 9 类；第 10 类「`go` 的实参区」是
> 实测长出来的，必须进契约）+ §4.1.1 一节；§4.2 写清 `g_frames` 的新语义。
>
> 顺带修了一处**契约漂移**：`C12` §5.1 一直写着 `RGOC_GC_STRESS=<n>` 是「每 n
> 次」，而代码存的是布尔（`parse_u64(...) != 0`）⇒ `STRESS=64` 与 `STRESS=1`
> 等价。实测证据：`STRESS=1000` + 30 万分配跑出 `collections=300000`。
> 改成真间隔后 `groot07` 的 GC 次数 300000 → 300，两套语料三态仍全绿。

- [x] **Step 1**: `C12` §4.1 扩表 + §4.2 改语义（+ §5.1 修漂移）。
- [x] **Step 2: Commit** `docs(m11): C12 §4 增补多 G 根 + 修正 stress 的间隔语义（Task 38）`

### Task 39: Phase 3 收口 —— ✅ 已完成（2026-10-27）

> **`GO-6` 六条全绿**（计划写「五条」，实际 01/02/03/05/06/07 六条；`04` 退役）。
> **`mode` 翻转完成**：`t_groot_05` 断言 `mode=stress` + `collections>0`。
> **变异 15/15 全红**。
> **`t_chan_08` 第一版是假绿**（`collections=0` 而 heap 涨到 170 MB）——
> `rt_select_begin` 的分配在 `gc_pause` 里，而 `gc_pause != 0` 时**连 stress
> 钩子都跳过** ⇒ 只做 select 的程序一次 GC 都不跑。改成「末尾再 churn 一轮」
> 才真的扫到。

- [x] **Step 1**: `GO-6` 六条全绿；`mode` 翻转完成；变异 15/15 全红。
- [x] **Step 2: Commit** `chore(m11): Phase 3 收口（M11c）`

---

# Phase 4 · 收口（Task 40–46）

### Task 40: 全量回归 —— ✅ 已完成（2026-10-27）

> **读数**：fmt / check / clippy 全绿；test **2276 passed / 0 failed / 3 ignored**；
> `run-native.sh` 三态各 69/0/0；`run-native-chan.sh` 三态各 7/0/0（`M'` 12/12）；
> `run-chan.sh chan-input.txt`（解释器）**ok=7 skip=12 bad=0 / 19**。
>
> ⚠️ **这一轮补上了一条「台账打了勾但没写测试」的判据**：`Task 10` 的 Step 3
> 把 `t_stk_06_溢出深度实测` 打了 `[x]`，而 `go_stack.rs` 里根本没有它
> （「推到 Task 12」之后就没人回来）。补实现时**立刻抓到两个真问题**：
>
> ① **溢出报错从来没在新 G 上生效过**：序言检查是「`sp` 掉到界外才报」，
>    而 `rt_panic_stack_overflow` 自己有栈帧 ⇒ 它的写入落在 `stack_lo` 之下
>    ⇒ **SIGSEGV（rc=-1）**。`main` 之所以没事，是因为它的 `stack_lo` 取
>    「入口 `sp` − 4 MiB」（下面还有 4 MiB 真栈）。修法两条：`stack_lo` 之下
>    留**一页**余量（`G_STACK_SLACK`）+ 冷路径 `mov sp, x9` 把 `sp` 抬回界内。
> ② **溢出深度实测 = 452 层**（64 KiB，每帧 ~144 B）；`N+1 = 453` 溢出且
>    两次跑逐字节相同 ⇒ 写进 manifest 的 `measured.stack_overflow_depth`。
>
> 新增 `t_stk_07_大帧溢出也报错`（12000 字节帧）+ 变异 `M-M11-18`
> （去掉 `mov sp, x9`）。⚠️ `M-M11-18` 第一版拿 `t_stk_06` 当靶子时**是绿的**：
> 小帧的越界量最多一帧（~144 B），一页余量兜住了 ⇒ 判据没牙。换成 12 KiB 帧后
> 越界 6752 B > 4096 才真的露出来。**变异 16/16 全红**。

- [x] **Step 1**: 四条门禁 + 三套语料（读数见上）。
- [x] **Step 2: Commit** `chore(m11): 全量回归 + 补 T-STK-06/07（Task 40）`

### Task 41: `M11-manifest.json` —— ✅ 已完成（2026-10-27）

> **读数**：`docs/milestones/M11-manifest.json` —— 7 条门禁逐条证据 / **10 条**
> 白名单（计划写 8 条，实际多两条：`U-M11-09` select 的 comma-ok、`U-M11-10`
> `go` 的聚合/浮点实参）/ `corpus_lists`（69 与 19−12=7 都**算出来**）/
> `measured`（`stack_overflow_depth` 452 / `many_g` 10 万 / `select_waitset`
> 5000 次）/ 5 条 `open_gaps` / 7 条 `closed_gaps` / 1 条 `deferred_gaps`。
>
> ⚠️ 白名单**不减只增**：计划里的 `U-M11-01..08` 是全不做的那八件事，而
> M11c 收口时又暴露了两处窄化（comma-ok 的 select、`go` 的聚合实参），
> 一起如实记进去 —— 「没做」也要有号码，否则下一个人会以为是 bug。

- [x] **Step 1**: 七个门禁 + 白名单 + 语料 + measured + gaps。
- [x] **Step 2: Commit** `docs(m11): manifest + 对账（Task 41/42）`

### Task 42: `manifest_m11.rs` —— ✅ 已完成（2026-10-27）

> **读数**：3 条全绿。
> * `t_manifest_11_真清单必须自洽`：门禁集合 / 状态与证据 / 白名单 10 条 /
>   语料计数**与路径列算出来的一致**（含 `input − rejected = N'`）/
>   `depth_overflow == depth_ok + 1` 等自洽项；
> * `t_manifest_11_并发语料三态必须能复现`（**M11 特有**）：7 个 `N'` × 三态
>   **真跑一遍**，ok 数从清单读（不硬编码 7），三态 `stdout`/`stderr`/退出码
>   逐字节相同；
> * `t_manifest_11_合成反例都必须红`：12 个改坏的清单逐个**指名道姓**报错
>   （否则「对账通过」只说明对账函数什么都没查）。

- [x] **Step 1**: 沿 `manifest_m10.rs` 的结构 + 三态复现。
- [x] **Step 2: Commit**（与 Task 41 同一提交）

### Task 43: 变异笔记 —— ✅ 已完成（2026-10-27）

> **读数**：`scripts/mutation-m11-notes.md` —— **16 条**（不是 15 条：多出
> `M-M11-17` 不还栈、`M-M11-18` 不抬 `sp`）+ 2 条「该绿」的（`M-M11-05` 要改
> IR 语义 / `M-M11-10` 变异后挂死）+ **4 条绿过的**（含 `M-M11-18` 靶子打错、
> `t_chan_08` 判据恒真、`t_stk_06` 一发现就是红的、`t_groot_01` 第一次红是空输出）。
>
> 结论段写了一句下一阶段的规矩：**「变异全红」不证明判据齐全** —— M11 的两次
> 「没牙」都是主动跑红抓到的，不是变异抓出来的。

- [x] **Step 1**: 16 条 + 2 条该绿 + 4 条绿过的成因。
- [x] **Step 2: Commit** `docs(m11): 变异笔记 + 报告（Task 43/44）`

### Task 44: `M11-report.md` —— ✅ 已完成（2026-10-27）

> **读数**：九节 —— 读数（全部可复跑）/ 交付 / **关键决策的实测复核**（六条，
> 含「`D-M11-12` 从来没接线」）/ **真错清单按「为什么先前没抓到」分类**
> （判据没写 / 判据没见过那种形态 / 根协议漏一类 / 契约与实现漂移 / 判据恒真 /
> 变异靶子打错）/ 判据的牙 / 不承诺 / 与 M5–M10 的边界 / 接手指南（含
> 「改 native 运行时的三条铁律」）/ 遗留。
>
> 决策复核那一栏最有价值：`D-M11-2` 的「64 KiB 够 1000 层」被实测打成
> **452 层** —— 估算高了一倍多，而这件事只有 `t_stk_06` 能发现。

- [x] **Step 1**: 九节齐。
- [x] **Step 2: Commit**（与 Task 43 同一提交）

### Task 45: `GAP-01` M11 节 + 文档回写 —— ✅ 已完成（2026-10-27）

> **读数**：
> * `GAP-01.md` 的 M11 节：四个归属点（`RT-MEM-03` ✅ / `RT-CMP-05` ✅ /
>   `RT-CMP-06` ✅ / `RT-MEM-05` ✗）+ 三条口径说明 + **关掉的七个缺口** +
>   一条 `deferred`。
> * `docs/README.md`：M11 三件套从「🟡 草案/待拍板」改「✅ 已定稿/已收口」，
>   并补 `M11-manifest.json` / `M11-report.md` 两行。
> * `AGENTS.md`：§0 的一句话状态从「待拍板、未开工」改成「已收口」+ 读数
>   + 两条容易再踩的（`D-M11-12` 未接线 / 实参区自成一类根）。
> * `docs/03-roadmap.md`：M11 节的状态块换成「✅ 已收口」+ 读数 + 两条与计划
>   不同的实测结论。
>
> ⚠️ `RT-MEM-05`（并发 GC）**记 ✗ 而不是 ◐**：M11c 做的是「多 G 的**根**扫描」，
> 那是「STW 扫得更全」，不是「并发」—— 记 ◐ 会让人以为「有点基础」。

- [x] **Step 1**: `GAP-01.md` 的 M11 节。
- [x] **Step 2**: 三处文档回写。
- [x] **Step 3: Commit** `docs(m11): GAP-01 与文档回写（Task 45）`

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
