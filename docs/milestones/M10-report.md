# M10 报告 · 精确 STW GC

> 生成于 2026-10-27。所有数字都**实测可复跑**（每条后面给了命令）。
> 台账（`.superpowers/sdd/M10-plan/progress.md`）不入库；本报告与
> `M10-manifest.json` 是入库的收口物。

## 1. 读数（全部实测，可复跑）

| 项 | 值 | 怎么复跑 |
|---|---|---|
| workspace 测试 | **235 suite / 2234 passed / 0 failed / 2 ignored** | `cargo test --workspace --no-fail-fast` |
| 四条门禁 | fmt / check / clippy / test 全绿 | `cargo fmt --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets -- -D warnings` |
| 语料 `off` | **69/0/0** | `run-native.sh --gc=off` |
| 语料 `on` | **69/0/0** | `run-native.sh --gc=on` |
| 语料 `stress` | **69/0/0** | `run-native.sh --gc=stress` |
| 变异 | **20/20 全红** | `bash scripts/mutation-m10.sh` |
| 长跑（1e5 × 1 KiB，累计 100 MB） | `collections=97 live=1040 heap=17039872 chunks=1 rss=2772KB` | `gc_longrun::t_gc_05` / `manifest_m10::t_manifest_10_长跑读数必须能复现` |
| 长跑 stress=64 | `collections=100000 live=1040 heap=17039872` | 同一 fixture + `RGOC_GC_STRESS=64` |

**100 MB 累计分配压在 1 个 17 MB 的 chunk 里，存活 1040 字节、峰值 RSS 2.7 MB。**
这是「回收 + 复用」同时工作的证据：单靠回收不压 chunk 数，单靠复用不压 `live`。

### 1.1 三个阶段各自的读数

| 阶段 | suite/passed | 语料 off | on | stress |
|---|---|---|---|---|
| M9 终态（起点） | 225 / 2184 | 69/0/0 | —（没有 GC）| — |
| Phase 0 收口 | 228 / 2199 | 69/0/0 | 69/0/0 | 69/0/0（三态等价，装置判据靠 `mode=`）|
| Phase 1 收口 | 230 / 2213 | 69/0/0 | 69/0/0 | **57/12**（只扫全局根）|
| Phase 2 收口 | 233 / 2225 | 69/0/0 | 69/0/0 | **69/0/0** |
| Phase 3 收口 | 234 / 2231 | 69/0/0 | 69/0/0 | 69/0/0 |
| **M10 收口（Phase 4）** | **235 / 2234** | 69/0/0 | 69/0/0 | 69/0/0 |

⚠️ Phase 3 之前 `on` 与 `off` 是**逐字节相同**的 —— 因为自动触发还没接上。
「两态一致」在那时**不携带信息**（`R-10-3` 就是为这个加了 `mode=` 字段）。

## 2. 交付

| 层 | 内容 |
|---|---|
| 契约 | `C12`（新建，七节）；`C5` §11.1 对象头 8→16、§7 帧布局 `SAVED` 16→32；`C9` §5 统计行只进 stderr |
| 运行时 | `rt.c` 的托管堆（chunk / 起始位图 / 页摘要 / free list / 标记 / 清扫 / 帧链 / pin / pause / `rt_alloc_escaped` / 自动触发 / RSS）|
| 后端 | `isel`（帧记录、`sp_idx`、摘链、逃逸槽）、`frame`（`REC_*`、`SAVED=32`、帧图补 addr-slot）、`module`（`Liveness` 前移、取址槽保守标注）|
| SSA | `live::is_safepoint` 穷尽匹配、`dce`/`effect` 不再把分配当纯 |
| 判据 | `gc_stats` / `gc_probe` / `gc_heap` / `gc_reclaim` / `gc_stack` / `gc_precise` / `gc_arena` / `gc_longrun` / `gc_corpus` / `manifest_m10` |
| 装置 | `support/gc_probe.rs`（`Stats` / `parse_stats` / `run_with_env` / `assert_clean`）、`run-native.sh --gc=`、`scripts/mutation-m10.sh`（20 条）|

## 3. 关键决策的实测复核

| 决策 | 计划 | 实测后 | 依据 |
|---|---|---|---|
| 对象头 | 8 字节 + 侧表 | **16 字节**，`+8` 里塞 `n | kind | blk>>4` | `R-10-9`：块大小必须**自证**，不能按 `kind+n+desc` 算（desc 非法时算不出 ⇒ sweep 走到块中间）|
| 根 | 逐位点精确（live-in）| **live-out 的保守超集** | `R-10-21`：真 live-in 让 `defernil`/`print`/`nilptr2`/`method5` 全坏，机制未查清 |
| map 槽 | 进托管堆 | 进托管堆，**`count` 留在 `+0`** | `R-10-17`：`len(m)` 是 `ldr [m]`，挪字段让 `indirect.go` 报 `wrong maplen` |
| 逃逸槽 | （计划没提）| **必须进托管堆** | `R-10-23`：M9 放在 arena ⇒ 闭包捕获 `&槽` 时槽里的指针没有根 |
| 自动触发 | 分配失败才收 | **每次分配前按 `gc_since` 判** | `R-10-25`：初稿条件在 16 MiB chunk 下永远不触发 |

## 4. 真错清单（按「为什么先前没抓到」分类）

### 4.1 「被测代码没执行」

- **`R-10-4` DCE 删 `Instr::Make`**：`_ = new([100]byte)` 落到 `keep` 的兜底分支
  （「结果没人用就删」）。`gc1.go` 的 1e5 次分配在 `--opt=all` 下 `heap=0`，
  `--opt=none` 下 16 MiB。
- **`R-10-11` 函数级 effect 摘要说 `new`/`make` 是纯的**：`R-10-4` 只修了指令级，
  `effect.rs` 还写着「纯分配：new/make 只写新分配的存储」。于是 `churn()`
  （只分配、不写、不打印）的摘要是纯的 ⇒ **整个调用被删** ⇒ `allocs=1`
  （2 万次分配一次没跑）。这一条让「全局根保活」的变异**连绿三次**。

### 4.2 「执行了但没可观察效果」

- **`R-10-12` free list 排在 bump 之后**：bump 一用完就开新 chunk，于是回收的块
  **永远不会被复用**。症状：`G` 的块进了 free list 但内容从没被覆盖 ⇒
  「全局根保活」的变异还是绿。

### 4.3 「形状不对」

- `t_root_12` 的 churn 用 8 KiB 块，而 `G` 的块是 1040 字节 ⇒ first-fit 装不进
  ⇒ 跳过它 ⇒ 不复用 ⇒ 变异绿。
- `t_gc_06` 的碎片判据拿 `PEAK_LIVE*2 + CHUNK` 当上界，忘了 chunk 元数据
  ⇒ 17 MB 比 16.7 MB 大 ⇒ 绿不了（这条是「判据太紧」，反向的同类）。

### 4.4 「漏了一条指令」（`R-10-13` / `R-10-19`，四次）

`is_safepoint` 必须与「会分配的指令」一一对应。M10 里漏了四次，每次症状都不同：

| 漏的 | 症状 |
|---|---|
| `Instr::Make`（DCE 层）| `heap=0`，stress 空跑 |
| `MakeClosure` / `Convert` / `Bin` | `interbasic` 的 `ia == ib` 成立（接口 data 被回收后两块复用成同一块）|
| `Instr::Composite`（切片字面量）| `complit` 的 `sc.a/b/c` 全读到 `sc.c` |

第四次之后 `is_safepoint` 改成**穷尽匹配**（不留 `_ => false`）：新增 `Instr`
变体会编译失败。

### 4.5 「位图不变式破了」（`R-10-20`）

起始位图的语义是「**这里有一个块头**」，不区分活块与空闲块。两处漏了：

- `free_push` 没 `bm_set` ⇒ 指向空闲区的指针往前找到**残留旧位**（某个更早被
  回收的块）⇒ 把空闲区里的旧字节当块头读（旧 desc 指针 bit0 是 0 ⇒ 当成活块）
  ⇒ `scan_block` 拿旧 desc 扫垃圾 ⇒ **段错误**。`ken/simpswitch.go` 就是这样崩的。
- `sweep` 里「已经空闲的块」没 `bm_clear`（它会并进更大的 run）。

补了一条 `mark_ptr` 的第三道防线（块头结构校验），把「假阳性」变成 `bad_ptr`
而不是段错误 —— 契约的原话是「宁可泄漏也不误扫」。

### 4.6 「值域算错」

- **`R-10-23` 逃逸槽在 arena**：`p := &x` 且 `p` 逃出函数时，`x` 的存储走
  `rt_arena_alloc`（**不扫描**）。闭包块里的捕获字是 `&槽`（栈地址），
  `mark_ptr` 直接忽略 ⇒ 槽里那个指针一个字都没被标。最小复现打印 `204`
  而不是 `42`。
- **`R-10-18` 多块构造函数的发布窗口**：`map_new` 先建槽块再建 `MapObj`，
  第二次分配前的 GC 把槽块收掉，而 `MapObj` 恰好**复用了同一块** ⇒
  `m->slots` 指向自己的中间 ⇒ `m[0]` 读出 0。修法是 `gc_pin`（深度 8）。
- **`R-10-17` `MapObj` 的字段序**：为了「gcdata 只有 bit0」把 `slots` 挪到字 0，
  结果 `len(m)` 读出槽块指针（`indirect.go` 立刻报 `wrong maplen`）。
  改回来，用 gcdata 的 bit5 指 `slots`。

### 4.7 「判据随实现演进失效」

- `bad_ptr` 收窄（不再计「指向已回收的块」）之后，`t_root_10` 的
  `bad_ptr == 0` **没牙了** —— 「`sp_idx` 恒 0」的变异实测**是绿的**。
  改成读内容（`p[0]+q[0]` 必须等于 49）。
- `t_root_11`（三层帧的链）在「三层返回后直接 println」下，漏摘链的变异
  **是绿的** —— 因为没有 GC 去读那条死链。加一段 `churn()` 覆盖死帧内存。

## 5. 判据的「牙」是怎么长出来的（20 条变异）

`scripts/mutation-m10.sh` 的 20 条，按阶段：

| 阶段 | 变异 | 抓的是 |
|---|---|---|
| Phase 1（1–9）| 块大小 / 起始位 / sweep 全活 / 标记位 / 堆判定 / 退化路径 / 清零 / free list / 全局根 | 分配器与清扫的基本正确性 |
| Phase 2（10–19）| `Composite` 位点 / mapslots 扫描 / `free_push` 位 / TOMB 过滤 / `mark_defer` / 逃逸槽 / 帧链摘 / `sp_idx` / 整帧保守 / 位序 | 根协议与 map |
| Phase 3（20）| 自动触发永不触发 | 自动态不是关态 |

**一条「该绿」的**：把 `next` 恒设 0（每次分配都收）实测**绿** —— 这是
「压力与自动等价」的证据（自动态与 stress 态在语义上一致，只是频率不同）。
它写在本节而不是脚本里：变异脚本的约定是「绿 = 失手」。

## 6. 不承诺（`M10-manifest.json` 的 `unsupported` 八条）

并发/增量 GC、对象移动/压缩、分代、写屏障、size-class 分配器、finalizer/weak、
栈收缩/复制、碎片治理。

其中两条值得单独说：

- **不移动**（`D-M10-5`）是**设计选择**，不是省事：内部指针没有 forwarding 表，
  而 Go 的 `&s[3]` 这种形状到处都是。代价是碎片只能记录不能修 ——
  `t_gc_06` 的注释里写清了「单 chunk 时 99.99% 的碎片率怎么读」。
- **不做写屏障**是 STW 的直接推论：全堆扫描时「扫的时候现读」，所以赋值不需要
  通知 GC。M11 一上并发，这条立刻变成最贵的债。

## 7. 与 M5–M9 的边界

| 判据面 | 谁管 |
|---|---|
| 对象**形状**（size / ptrmask / desc）| M9（`MEM-2`）—— M10 只消费，不改 |
| 对象**生命周期** | M10（`GC-1..GC-9`）|
| 栈图**格式**（`STACK-MAP v1`）| M9（`ROOT-1`）—— M10 只消费 `sp_off` / `sp_bits` |
| 栈图**何时用** | M10（帧记录链 + `sp_idx`）|
| `hchan` 布局 | M9（`SCH-1`）—— M10 不碰 |
| 解释器语义 | 不受影响（M10 只改 native 通路）|

## 8. 接手指南

1. **先看 `C12` §4**（根集合与帧记录 ABI）—— 它是全部正确性的地基。
2. **改任何分配器代码前**读 §1.3 的两条：「free list 在 bump 之前」与
   「`gc_pin` / `gc_pause` 的分工」。这两条各踩过一次。
3. **加任何 `Instr` 变体**：`live::is_safepoint` 会编译失败 —— 那是故意的。
   回答「它会不会分配」，不要图省事写 `false`。
4. **加任何 `rt_arena_alloc` 调用点**：`gc_arena::t_gc_11` 会红 —— 也是故意的。
   先确认「不含堆指针」或「有别的根」，再登记。
5. **改判据之后**：跑 `scripts/mutation-m10.sh`。判据没牙的三种成因
   （没执行 / 没可观察效果 / 形状不对）**读代码看不出来**，只能靠变异。
6. **`bad_ptr` 的口径**：只计「解析不到块首」与「块头结构非法」。
   「指向已回收的块」是保守位图的常态，不计 —— 别把它加回去，
   否则这个探针会恒非 0、然后就没人看了。

## 9. 遗留

- **`G-M10-06`**（`deferred_gaps`）：真 live-in 位图会让四个语料坏掉，机制未查清。
  保留超集是安全的（多标 = 泄漏一轮），但 M11 的并发标记对「多标」的容忍度更低
  （拖慢终止判定）—— **进 M11 之前必须查清**。
- 碎片率只有记录没有治理（`U-M10-08`）；长跑判据量的是「有界」而不是「停顿短」。
