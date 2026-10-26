# M10 设计：精确 STW GC

> **一句话**：给 native 通路加一个**单线程、精确到字、不移动**的 mark-sweep 收集器 ——
> 消费 M9 交的 `STACK-MAP` / `RtFuncInfo` / `rt_globals`，把「活对象」真的判出来，
> 并让堆**复用**（M9 的 arena 只增不减）。

> **本文档的状态**：草案（2026-10-26）。§2.4 有三件事需要拍板，拍完才冻结。

---

## 0. 本文档在流程中的位置

```text
03-roadmap.md §M10  ──►  本文档（怎么建）  ──►  M10-tests.md（怎么验）
                                            └─►  M10-plan.md（怎么干）
                                                  └─►  M10-manifest.json（机器可读事实）
                                                        └─►  M10-report.md（交付报告）
```

`M10-tests.md` 的判据 ID 与本文档 §4 的门禁一一对应；`M10-plan.md` 的每个 Task
必须能回答「它让哪条门禁前进一格」。**顺序不能反**：先有门禁再有 Task。

---

## 1. 目标与非目标

### 1.1 输入与输出

| 项 | 值 |
|---|---|
| 输入 | M9 收口态（native 语料 69/0/0、`cargo test --workspace` 2184 passed） |
| 输出 | 一个能自动触发、能回收、能被统计的 STW GC；**69 个语料在三种 GC 状态（关 / 开 / 压力）下逐字节不变** |
| 新契约 | `C12-heap-and-gc.md`（堆形状 / 块头 / 根集合 / 触发与统计） |
| 改的契约 | `C5` §11（块头 8 → 16 字节）、`C9`（统计行只进 stderr，不进合并流） |
| 不改 | `C4`（IR 一个变体都不加）、`C8`、`C10`、`C11` |

### 1.2 范围（`03-roadmap.md` §M10 逐句展开）

| roadmap 的话 | 本设计落在哪 |
|---|---|
| 「独立小堆 + 已登记 roots 的手工触发 mark/sweep，验证扫描算法」 | Phase 1：托管堆 + `rt_gc()` 手工触发，只扫**全局**根 |
| 「接入全局/静态、栈、保存寄存器、闭包/接口等全部支持类型 roots」 | Phase 2：帧记录链 + 每帧位点 + defer/panic/map/closure/array 六类根 |
| 「分配点/调用/回边 safepoint 和暂停协议」 | Phase 2：位点索引（M9 的 `live.rs` 已经产出，M10 只负责**写进帧**并**读出来**）|
| 「整程序回收、统计、压力/长跑、多次 collection、碎片与零初始化回归」 | Phase 3：自动触发 + `RGOC_GC_STRESS` + 长跑 + 三态零回归 |
| 「记录 RSS 而不要求 RSS 每次下降」 | Phase 3 的 `T-GC-09`：只**记录**，不设下降判据 |

### 1.3 明确不做（M10 边界）

| 不做 | 为什么 | 记在哪 |
|---|---|---|
| 并发标记 / 写屏障 | 单线程、无抢占 ⇒ 不需要；写屏障是 M11 之后的事 | `U-M10-01` |
| 移动（compact / copying） | 移动就要 forwarding，而**内部指针**（`s[2:]`、`&a[i]`）没有地方存转发地址 | `U-M10-02` |
| GC pacing（Go 的 `GOGC` 那套比例控制） | 没有并发、没有 `runtime` 包；用「分配失败才收 + 阈值」够用 | `U-M10-03` |
| 栈增长 / goroutine 栈 | native 没有 goroutine（M11）| `U-M10-04` |
| 终结器 / `runtime.SetFinalizer` | 没有 `runtime` 包 | `U-M10-05` |
| 解释器侧 GC | 解释器用 `Rc` 自动回收；GC 在语义上**不可观测** | `U-M10-06` |
| 内存统计的完整面（`HeapAlloc`/`HeapObjects`/`PauseNs`） | 只给一行汇总（`RGOC_GC_STATS`）| `U-M10-07` |
| 碎片整理 | 不移动就没有整理；碎片只能**测**（`T-GC-06`）| `U-M10-08` |

### 1.4 退出条件（门禁九条）

| 门禁 | 判定 |
|---|---|
| `GC-1` 对象头与块形状 | 块头 16 字节；`rt_alloc` 返回 data；`new(T)` 读字段 0 是零（**已由 `R-10-1` 完成前置**）|
| `GC-2` 托管堆 | 分配 / 复用 / 合并 / 新 chunk 四条路径都有判据；`RGOC_GC=0` 时与 M9 等价 |
| `GC-3` 精确根 | 七类根（栈 / 全局 / defer / panic 值 / map 槽 / 闭包捕获 / 数组元素）各一条可达性判据 |
| `GC-4` 精确性 | **整帧扫描会崩**的 fixture 不崩；且不可达对象真的被回收（`live_bytes` 降）|
| `GC-5` 触发与统计 | 阈值触发、`STRESS`、`STATS` 三条；统计数字与探针自洽 |
| `GC-6` 零回归 | 69 语料 × {off, on, stress} **逐字节一致**；`--via=ssa` 不受影响 |
| `GC-7` 长跑与碎片 | 1e5 次分配后 `live_bytes` / `heap_committed` 有界；RSS 只记录 |
| `GC-8` 契约与文档 | `C12` 定稿；`C5` §11 修订；`C9` 增统计行 |
| `GC-9` 变异 | 每条门禁至少一条变异**实证变红** |

---

## 2. 决策记录

### 2.1 决策一览

| # | 决策 | 一句话 |
|---|---|---|
| `D-M10-1` | 托管堆与运行时 arena **分离** | Go 对象走托管堆；不含指针的运行时结构留在 `rt_arena_alloc` |
| `D-M10-2` | 块头 **8 → 16 字节** | `{desc\|flags, n\|kind}`；数组要「元素个数 + 元素 desc」，8 字节装不下 |
| `D-M10-3` | chunk **16 MiB 对齐** | `p >> 24` 直接判「是不是堆指针」+ 找 chunk，O(1) |
| `D-M10-4` | 每 chunk 一张**起始位图**（1 位 / 8 字节）+ **页摘要**（1 位 / 4 KiB）| 内部指针（`s[2:]`）能回到块首，且有界 |
| `D-M10-5` | 标记位放**块头** bit1 | 块头本来就要读（拿 desc），省一次侧表访问 |
| `D-M10-6` | 空闲块 bit0 + `size<<32`，`+8` 存 next；**清扫时重建 free list 并合并** | 分配时不合并（简单），清扫时一次做完（有序）|
| `D-M10-7` | 根走**帧记录链**（每帧 24 字节，全局 `g_frames`）| 不引入 pclntab（`U-M9-06` 仍归 M12）|
| `D-M10-8` | 只有**含 safepoint** 的函数发帧记录 | 不含 call/alloc/回边的叶子函数不可能停在 GC 点 |
| `D-M10-9` | **没有寄存器根** | 「每个值一个栈槽」的推论；调用点的 `uses()` 已在位图里 |
| `D-M10-10` | `RtFuncInfo` **不加字段** | `sp_words` 是导出量（`ceil(frame_size/8/64)`），两侧各有断言 |
| `D-M10-11` | 触发：分配失败 + 超过阈值才收 | `RGOC_GC=0/STRESS/STATS` 三个环境变量 |
| `D-M10-12` | 不移动 | 内部指针没有转发地址 |
| `D-M10-13` | map / 闭包 / 数组 / 字符串都进托管堆（四种 kind）| 否则它们的存储永远不回收（长跑会涨）|
| `D-M10-14` | 解释器不做 GC | `Rc` 自动回收；GC 语义不可观测 |
| `D-M10-15` | 统计只走 **stderr 一行**，退出时打 | 合并流（`C9`）一字不改 |

### 2.2 只对**非显然**的五条展开

#### `D-M10-2` 块头 16 字节（**需要拍板**）

```text
块 = [+0 u64 header][+8 u64 n|kind][+16 data...]
header:  bit0 = 空闲块        bit1 = 已标记
         bit2 = raw（不扫描） 其余位 = desc 指针
```

三条理由：

1. **数组**：`make([]T, n)` 的 n 是运行期的，编译期没有 `[n]T` 这个 desc。
   GC 扫它必须知道「元素 desc + 个数」。
2. **闭包**：`{fn, cap0, cap1…}` 的捕获个数也是运行期的。
3. **字符串 / `[]byte`**：不扫描（里面没有指针），但仍要能被回收 ⇒ 需要一个 kind 位。

备选：**侧表**（`块首 → (desc, n)` 的哈希表）。每次标记多一次查表，且表本身
要跟着分配/释放维护 —— 等于把块头换成一张更大的表。**不选。**

代价：每个对象 +8 字节。`new(int)` 从 16 字节变 24 字节（+50%），
但 M10 没有性能/占用门禁，而**正确性**（能扫）比这 8 字节值钱。

#### `D-M10-4` 起始位图 + 页摘要（内部指针怎么回块首）

Go 的答案叫 span（页对齐 + 每页记 size class）。我们没有 size class，所以用位图：

```text
start_bitmap : 1 位 / 8 字节   （16 MiB chunk ⇒ 256 KiB，1.5%）
page_summary : 1 位 / 4 KiB    （16 MiB chunk ⇒ 512 B）
```

解析一个（可能是内部的）指针 `p`：

```text
1. p 在哪个 chunk？（D-M10-3 的 O(1) 判定；不在 ⇒ 不是堆指针，直接忽略）
2. 本页内从 p 的字往回扫 start_bitmap（≤ 512 位 = 8 个 u64）
3. 本页没有起点 ⇒ 往前找 page_summary 为 1 的页，从该页顶往回扫
```

**为什么要页摘要**：没有它，「一个 1 MiB 数组的尾指针」要回扫 13 万个字。
有了它，回扫的代价是「跳过的页数」（最多 4096 次位测试，都在一张 512 字节的表上）。

#### `D-M10-7` 帧记录链（栈根怎么找）

```text
每帧（有 safepoint 的函数才发）在 fp-16 起留 24 字节：
  [fp-16] = prev     上一帧的记录地址（链头是全局 g_frames）
  [fp-24] = fi       *RtFuncInfo（M9 已经发在 .rodata）
  [fp-32] = sp_idx   本帧**当前**的 safepoint 下标

prologue：prev = g_frames; fi = &__funcinfo_N; sp_idx = 0; g_frames = fp
epilogue：g_frames = prev
每个 safepoint 之前：sp_idx = <该位点的下标>
```

**为什么需要 `sp_idx`**：位图是**逐位点**的。用「整个函数的并集」会踩到
「槽里现在是整数、位图上却是根」—— 那是**保守扫描**，不是精确扫描，而且会把
一个像地址的整数当指针解引用（`T-PREC-01` 就是钉这个的）。

**为什么不做 pclntab**：返回地址 → 函数的映射要 `.eh_frame` 或自建表；而
「记录链」只需要每帧 3 条 store。M12 做 traceback 时再补 pclntab，两者不冲突。

#### `D-M10-9` 没有寄存器根（一个必须写下来的推论）

AAPCS64 的 `x19..x28` 是 callee-saved，但本后端**每个值一个栈槽**（`C5` §7），
所以任何跨调用的活值都同时在槽里。调用点的位图取的是
`live_out ∪ ins.uses()`（M9 `live.rs`），**实参也在里面**，而实参在寄存器里的
同时仍留在自己的槽里（`isel` 是先 `ld` 到寄存器再 `bl`，不覆盖槽）。

⇒ 扫栈就够，不需要保存寄存器集合。**这条要写成判据**（`T-GC-10`）：
将来若加寄存器分配，这条会红 —— 而红的时候必须补寄存器根，不能改判据。

#### `D-M10-13` 四种 kind（map / 闭包 / 数组 / 字符串）

| kind | 谁分配 | 怎么扫 |
|---|---|---|
| `object`（n=0）| `rt_alloc`（`new`、`rt_box`）| `desc->ptrdata` 字节按 `gcdata` |
| `array` | `rt_makeslice` / `rt_growslice` / 复合字面量 | `n` 个元素，每个按 `elem` 的 `gcdata` |
| `closure` | `MakeClosure` | `n` 个捕获，每个都是指针 |
| `raw` | `rt_concat` / `rt_itoa` / `[]byte` 的底层数组 | **不扫**（里面没有指针）|
| `mapslots` | `rt_map_make` / `map_grow` | 按 `{kdesc, vdesc, stride}` 扫**每个 LIVE 槽**的 key 与 value |

`MapObj` 本身变成**托管对象**，带一张手写的静态 desc（`rt_desc_mapobj`）：
它的 `{slots, kdesc, vdesc}` 三个字是指针，其余是标量。这样 map 的存储也能被回收 ——
否则「循环里 make 一个 map」会一直涨，`T-GC-05`（长跑）过不去。

### 2.3 事实基线（`F1..F14`，全部实测或源码指认）

| # | 事实 | 出处 |
|---|---|---|
| `F1` | 块形状 `[+0 *TypeDesc][+8 data]`；**`rt_alloc` 返回 data** | `C5` §11.1 + `R-10-1`（本轮修）|
| `F2` | arena = chunk 链；chunk = `max(16 MiB, need)`；`heap_committed` 记账；上限 `RGOC_HEAP_MAX`（缺省 256 MiB）| `rt.c` `rt_grow` |
| `F3` | 只有 `new(T)` / `rt_box` 走 `rt_alloc`；切片 / map / 闭包 / 字符串全走 `rt_arena_alloc`（**无头**）| `rt.c` 各函数 |
| `F4` | 栈图已交：`<out>.stackmap` + `.rodata` 的 `RtFuncInfo{frame_size, n_sp, sp_off, sp_bits}` + `rt_funcs` / `rt_nfuncs` | M9 Phase 6，`C5` §8.4 |
| `F5` | `rt_globals` = `(desc, addr)` 对；零大小全局不进表 | `module.rs::emit_global_roots` |
| `F6` | safepoint 顺序 = `(block, idx)` 升序；`stack_map_of` 与 `live.rs` 同序 | `live.rs` / `module.rs` |
| `F7` | 帧：`[fp]`=saved fp、`[fp+8]`=lr、值槽从 `-16` 起；`frame_size` 是 16 的倍数；超大帧整块走 mmap | `frame.rs` / `R-9-22` |
| `F8` | 每个值一个栈槽，**无寄存器分配** | `C5` §7 |
| `F9` | `rt_defer_push(fp, fv)` 全局链；`DeferRec{next, fp, fv}` | `rt.c` |
| `F10` | 无 goroutine、无抢占 ⇒ STW 平凡成立（GC 只在分配点同步跑）| M9 白名单 `U-M9-07` |
| `F11` | 解释器用 `Rc`；`--via=ssa` / `--via=hir` 与 native 共用 `Value` | `rgoc-interp` |
| `F12` | `TypeDesc` 40 B；`ptr_mask` 与 `gcdata` **同一个函数** | `C5` §11.1 / `T-META-04` |
| `F13` | 探针有先例：`rt_probe_align` / `rt_heap_used` | `rt.c` |
| `F14` | `epilogue()` 是唯一的出口发射点（`Term::Return` 与 panic 出口都调它）| `isel.rs` |

### 2.4 需要拍板的三件事

| # | 选项 | 建议 |
|---|---|---|
| ① `D-M10-2` 块头 8 → 16 字节 | (a) 16 字节块头（+8 B/对象，扫得动数组/闭包）<br>(b) 8 字节块头 + 侧表 | **(a)** —— 侧表要在每次标记时查表，且表本身要维护；8 字节买不到「元素个数」 |
| ② `D-M10-7` 帧记录 + 每位点一条 store | (a) 逐位点（+32 B 帧、每 safepoint 2 条指令，**精确**）<br>(b) 函数级并集位图（不加帧开销，但会**误标**）| **(a)** —— (b) 会把「现在是整数」的槽当根，那是保守扫描，`T-PREC-01` 直接红 |
| ③ `D-M10-13` map 存储进托管堆 | (a) `MapObj` + slots 都托管（可回收）<br>(b) 留在 arena + 登记表（map 存储**永不回收**）| **(a)** —— (b) 让「循环里 make map」无界增长，`T-GC-07` 过不去 |

---

## 3. 架构与数据模型

### 3.1 crate 与模块变化

| crate / 文件 | 变化 |
|---|---|
| `runtime/native/.../rt.c` | **主要工作量**：托管堆、标记、清扫、根扫描、统计（新增约 700 行）|
| `rgoc-arm64/src/frame.rs` | 帧顶预留 24 字节（`REC_OFF` 常量 + `frame_size` 调整）|
| `rgoc-arm64/src/isel.rs` | 序言/尾声的帧记录；每个 safepoint 前写 `sp_idx`；`MakeClosure` 改走托管分配 |
| `rgoc-arm64/src/module.rs` | `Liveness` 前移到 `compile()` 并传给 `isel::func`；`__funcinfo_*` 符号对 isel 可见 |
| `rgoc-ssa/src/live.rs` | **不改**（位点顺序已经是契约）；只补一个 `pub fn index_of(&self, block, idx) -> Option<u32>` |
| 新契约 | `docs/contracts/C12-heap-and-gc.md` |

**没有新 crate**。GC 是运行期的事，Rust 侧只负责「把位点下标写进帧」和「发符号」。

### 3.2 堆形状

```text
Chunk（16 MiB 对齐，mmap 多要 16 MiB 再对齐）
┌──────────────────────────────────────────────────────────┐
│ 0                    data（16 MiB 或更大）                 │
│  ┌─────────┬─────────┬──────────┬──────────────────────┐ │
│  │ 块(活)   │ 块(空闲) │ 块(活)    │ …                    │ │
│  └─────────┴─────────┴──────────┴──────────────────────┘ │
└──────────────────────────────────────────────────────────┘
旁挂（每 chunk 单独 mmap，不计入 heap_committed 的「对象」部分）：
  start_bitmap   256 KiB（1 位 / 8 字节）
  page_summary   512 B  （1 位 / 4 KiB）
```

`Chunk` 头（在数据区之外还是之内？**之内，前 64 字节**）：
`{next, base, size, bump, free_head, meta_start_bitmap, meta_summary, live_bytes}`。

**16 MiB 对齐**的做法：`mmap(size + 16 MiB)`，取 `(r + 16 MiB - 1) & ~(16 MiB - 1)`，
把前缀那段 `munmap` 掉（省 RSS）。这样 `p >> 24` 就是 chunk id，配一张
64 槽的哈希表（`chunk_by_id`）→ O(1) 判「是不是堆指针」。

### 3.3 块头与五种 kind

见 §2.2 的 `D-M10-2` / `D-M10-13`。补充两条不变量：

1. **块大小恒为 16 的倍数**（`align8(16 + size)` 再取 16 对齐）⇒ 每个块的
   `data` 都 16 对齐，`uintptr` 往返不乱。
2. **零大小对象不进托管堆**（继续 `rt_zerobase`）—— 它没有可扫的字，
   进堆只会让 `live_bytes` 里多出一堆 16 字节的块。

### 3.4 分配与复用

```text
gc_alloc(need, kind, desc, n):
  1. 当前 chunk 的 bump 够 ⇒ bump（顺带 bitmap_set）
  2. 否则在**当前 chunk** 的 free list 里 first-fit（切分：余量 ≥ 最小块才切）
  3. 否则下一个 chunk（free list → bump）
  4. 全都不行 ⇒ 若 heap_committed > 阈值 ⇒ rt_gc() 后重试一次
  5. 仍不行 ⇒ rt_fatal_oom（`C9` §6.1 的文本不变）
```

**为什么 first-fit 而不是 size class**：M10 没有性能门禁，而 size class 要
「每类一张表 + 每页一个 class」—— 那是 Go 的 span 模型，比现在这版大一倍。
记成 `U-M10-02`（不做 size class），碎片由 `T-GC-06` **测**（不修）。

### 3.5 标记与清扫

```text
rt_gc():
  if (!gc_on) return;
  collections++;
  mark_roots();          # 见 §3.6
  drain();               # worklist 出栈 → 扫块 → 压子对象
  sweep();               # 按地址顺序走位图，重建 free list
  live_bytes = Σ(存活块)
```

- **worklist**：`mmap` 一块 64K 项的数组（512 KiB），满了就翻倍重 mmap。
  **不用递归**（深链会爆栈，而 M10 的栈已经很紧）。
- **标记位**：块头 bit1。`mark_ptr` 先做「在堆里吗」（`D-M10-3`），
  不在就直接返回 —— rodata 里的 desc / funcval / itab 都走这条路。
- **清扫**：逐页逐块（位图驱动），`marked` ⇒ 清位；`!marked` ⇒ 并入「待回收 run」；
  碰到活块就把 run 挂进 chunk 的 free list（run 是**有序**的，天然合并）。
- **`need_zero`**：复用的块必须清零（M9 的注释已经预告了这一条）。

### 3.6 根集合

| 根 | 怎么扫 |
|---|---|
| **栈** | `g_frames` 链：每帧 `{prev, fi, sp_idx}` → `fi->sp_bits + fi->sp_off[sp_idx]` → 位图覆盖 `[fp-frame_size, fp)` |
| **全局** | `rt_globals` 的 `(desc, addr)`：按 `desc->ptrdata` + `gcdata` |
| **defer 链** | `DeferRec.fv`（闭包块或 `.rodata` 的 funcval）|
| **panic 值** | panic 状态里那个值（可能是接口 ⇒ 两个字）|
| **map 槽** | 由 `MapObj`（托管对象）→ `slots` 块（`mapslots` kind）→ 每个 LIVE 槽 |
| **闭包捕获** | `closure` kind：`n` 个捕获字 |
| **数组元素** | `array` kind：`n` 个元素按 `elem` 的 `gcdata` |

**不扫**：帧记录自己的 `prev` / `fi`（栈地址与 rodata 地址，`mark_ptr` 会忽略）、
`rt_zerobase`、itab（rodata）、静态 funcval（rodata）、`rt_arena_alloc` 的运行时结构
（它们不含指向堆的指针 —— 这条要逐条核，见 `T-GC-11`）。

### 3.7 触发、统计与环境变量

| 变量 | 作用 |
|---|---|
| `RGOC_GC=0` | 关掉 GC（分配失败直接 OOM）—— 用来做「与 M9 等价」的对照 |
| `RGOC_GC=1`（缺省）| 自动触发（分配失败 + 超阈值）|
| `RGOC_GC_STRESS=1` | **每次分配都收**（慢，但能把根的错误全逼出来）|
| `RGOC_GC_STATS=1` | 退出时向 **stderr** 打一行：`gc: collections=N allocs=A frees=F live=B heap=H` |
| `RGOC_GC_THRESHOLD=<bytes>` | 触发阈值（缺省 4 MiB）|

**统计行的口径**：`live` 是上一次清扫后的存活字节；`heap` 是 `heap_committed`。
两者都只用于**测试断言**与人工观察，不进合并流（`C9` §5 一字不改）。

### 3.8 与解释器的边界

解释器没有 GC（`Rc`）。GC 在 Go 语义上**不可观测**：程序看不到地址（除非打印指针，
而那本来就与 Go 不一致）、看不到回收时机、看不到内存布局。唯一可观测的是
**OOM 的时机** —— 那是 `U-M10-06`，写清「两条通路的 OOM 点不同，且都只在
`RGOC_HEAP_MAX` 附近才出现」。

---

## 4. 判据与装置

### 4.1 三态语料（`D-M10-11` 的证据面）

| 态 | 命令 | 期望 |
|---|---|---|
| off | `RGOC_GC=0 run-native.sh` | 69/0/0（与 M9 完全一致）|
| on | `run-native.sh` | 69/0/0 |
| stress | `RGOC_GC_STRESS=1 run-native.sh` | 69/0/0 —— **这一条是「根扫全了」的最强证据** |

stress 态下每次分配都收 ⇒ 任何一个漏扫的根都会在下一次分配时变成
「指向已回收内存的指针」，输出立刻错或段错误。

### 4.2 差分口径

- native vs 官方 `.out`：逐字节（不变）
- `--via=ssa`：不受影响（GC 不碰解释器）
- 统计：`RGOC_GC_STATS=1` 的输出**只比结构**（`collections >= 1`、`live` 在界内），
  不比具体数字（阈值与 chunk 大小会变）

### 4.3 探针（`F13` 的先例）

| 探针 | 用途 |
|---|---|
| `rt_gc_collections()` | 收了几次 |
| `rt_gc_live_bytes()` | 上次清扫后的存活字节 |
| `rt_gc_alloc_bytes()` | 累计分配字节 |
| `rt_gc_bad_sp()` | 帧记录里 `sp_idx >= n_sp` 的次数（**必须是 0**）|
| `rt_gc_bad_ptr()` | `mark_ptr` 拿到「在 chunk 里但不在任何块起点」的次数（**必须是 0**）|

后两个是**自检计数器**：它们不为 0 就说明位点或位图错了 —— 比段错误好排查得多。

### 4.4 预算与资源

| 项 | 值 |
|---|---|
| 堆上限 | 仍由 `RGOC_HEAP_MAX`（缺省 256 MiB）管；**含**元数据 mmap |
| chunk | 16 MiB（对齐后 16 MiB） |
| 元数据 | 288 KiB / chunk（1.8%）|
| worklist | 512 KiB 起，可翻倍 |
| 长跑 fixture | 1e5 次 `new([1 KiB])` ⇒ 100 MiB 分配，靠复用不 OOM |

---

## 5. 不支持面（白名单，8 条）

| id | 不承诺 | 形态 |
|---|---|---|
| `U-M10-01` | 并发标记 / 写屏障 | 单线程 STW；GC 只在分配点同步跑 |
| `U-M10-02` | size class / span 分配器 | bump + first-fit free list |
| `U-M10-03` | GC pacing（`GOGC` 那套）| 阈值 + 分配失败触发 |
| `U-M10-04` | 栈增长 / goroutine 栈扫描 | native 没有 goroutine（M11）|
| `U-M10-05` | 终结器 | 没有 `runtime` 包 |
| `U-M10-06` | 解释器侧 GC / 两条通路 OOM 点一致 | `Rc` 自动回收 |
| `U-M10-07` | 完整内存统计（`PauseNs` 等）| 只给一行汇总 |
| `U-M10-08` | 碎片整理 / 压缩 | 不移动 |

---

## 6. 错误处理

| 情形 | 行为 |
|---|---|
| 分配失败且 GC 后仍不够 | `runtime error: out of memory (heap limit N bytes)` + 退出 2（`C9` §6.1，文本不变）|
| `sp_idx >= n_sp` | `bad_sp++`；该帧**跳过**（保守：漏扫会崩，跳过只是漏标，但计数器非 0 时测试红）|
| `mark_ptr` 指向块中部且解析不到起点 | `bad_ptr++`；忽略该指针（同样靠计数器红）|
| 块头 desc 为空 / 非法 | `bad_desc++`；把该块当 `raw`（不扫）—— 宁可泄漏也不误扫 |

三个计数器都是「**响**但不崩」的设计：崩了只能看到一个段错误，计数器能告诉你
「是哪一类错了、错了几次」。

---

## 7. 覆盖清单（roadmap §M10 ↔ 功能 ID）

| roadmap 项 | M10 后 | 说明 |
|---|---|---|
| `RT-MEM-04` GC（STW）| ✅ | 精确、单线程、不移动 |
| `RT-MEM-05` 并发 GC | ✗ | 归 M11+（`U-M10-01`）|
| `RT-MEM-06` 堆位图与指针元数据 | ✅ | 起始位图 + `gcdata` 消费 |
| `RT-MEM-07` 统一分配入口 | ✅ | `rt_alloc` / `rt_alloc_array` / `rt_alloc_raw` / `rt_alloc_mapslots` |
| `RT-MEM-08` 内存统计 | ◐ | 四个探针 + 一行 stderr（`U-M10-07`）|
| `RT-MEM-03` 栈内存管理 | ✗ | 归 M11（native 栈）|
| `PIPE-13` 逃逸分析 | ✗ | 仍不做（`R-13-5` 的保守判定不变）|

---

## 8. 风险登记（前三条是最可能的坑）

1. **`sp_idx` 与 `.stackmap` 的下标错位**。两处各算一遍就会错位，而症状是
   「扫了错的字」—— 表现为随机崩或算错。对策：`Liveness` **只算一次**，
   `sp_index` 表与 `stack_map_of` 同源；再加 `bad_sp` 计数器。
2. **内部指针回块首**。`s[2:]`、`&a[i]`、子串 —— 三个来源都要覆盖。
   对策：`T-HEAP-05`（跨页尾指针）+ `T-ROOT-17`（`[]*T` 元素）+ `bad_ptr` 计数器。
3. **块跨页**。允许跨页（省空间），但回扫要跨页找起点。对策：页摘要 +
   `T-HEAP-05`（1 MiB 数组的尾指针）。
4. **清扫时的 free list 合并**。合并错会把两个块叠在一起 ⇒ 之后分配互相覆盖。
   对策：`T-HEAP-01`（分配-释放-再分配 500 轮，每轮校验内容）。
5. **GC 期间的分配**。标记/清扫本身不能分配（worklist 走 mmap 直调），
   否则会递归触发。
6. **`rt_arena_alloc` 的残留指针**。收窄之后要逐条核「谁还在用 arena 且含指针」
   （`T-GC-11`）。

---

## 9. 任务分解原则（`M10-plan.md` 的输入）

1. **一条判据一个 Task**：每个 Task 的 Step 1 是「写会红的判据」。
2. **前置优先**：`R-10-1`（对象头）已完成；`C12` 草案在 Phase 0 就写，边做边改。
3. **手工先于自动**：Phase 1 的 `rt_gc()` 只能手工触发（`RGOC_GC_STRESS` 除外），
   先把「扫全局根」跑通，再谈栈根。
4. **栈根单独一 Phase**：它要改 `frame` / `isel` / `module` 三个文件，且
   `FRAME-MAP` / `STACK-MAP` 的对账会跟着动。
5. **零回归贯穿**：每个 Phase 收尾都跑三态语料，不攒到最后。

---

## 10. 契约与产物影响

| 契约 / 产物 | 影响 |
|---|---|
| `C5` §11.1 | 块头 8 → **16** 字节（`{desc\|flags, n\|kind}`）；`rt_alloc` 返回 data（已由 `R-10-1` 写清）|
| `C5` §8.4 | **不改**：`STACK-MAP v1` 的行语法与位序不动（`sp_words` 是导出量）|
| `C5` §7 | 帧顶多 24 字节记录 ⇒ `FRAME-MAP v1` 的偏移整体下移（行语法不变）|
| `C9` | 增一行「`RGOC_GC_STATS` 只进 stderr」；合并流不变 |
| **`C12`（新）** | 堆形状 / 块头 / kind / 起始位图 / free list / 根集合 / 帧记录 ABI / 触发与统计 |
| `<out>.stackmap` | 不变（帧大小变了，内容跟着变，但**格式**不变）|
| `.rodata` | 多两个符号：`rt_desc_mapobj`、`rt_desc_mapslots`（C 侧定义，不进 `.s`）|

---

## 11. 变更记录

| 日期 | 内容 | 触发 |
|---|---|---|
| 2026-10-26 | 草案：`D-M10-1..15` / `F1..F14` / 九条门禁 / 白名单 8 条 | 设计期实测发现 `R-10-1`（对象头从未生效），据此确定「块头 16 字节」与「`rt_alloc` 返回 data」两条前提 |
