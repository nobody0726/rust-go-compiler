# C12 契约：托管堆与 GC

> **状态**：草案（2026-10-26，M10/Task 1）。§6 与 §5.2 在 Phase 1/3 会各改一次，
> 定稿在 M10/Task 38。
>
> **范围**：native 通路的**堆形状**、**对象头**、**根集合**、**帧记录 ABI**、
> **触发与统计**。它约束 `runtime/native/aarch64-unknown-linux-gnu/rt.c` 与
> `rgoc-arm64` 的帧发射；**不**约束解释器（`U-M10-06`）。
>
> **与其它契约的关系**：
> - `C5` §11.1 给 `TypeDesc` 的 40 字节布局 —— 本契约**只消费**它，不改。
> - `C5` §8.4 给 `STACK-MAP v1` 的位序 —— 本契约的帧记录用同一份位图，不改格式。
> - `C5` §7 给帧布局 —— 本契约在帧顶**多要 24 字节**（§4.2），值槽整体下移。
> - `C9` §5 给合并流 —— 本契约的统计行**只进 stderr**，合并流一字不改。

---

## 1. 堆形状

### 1.1 chunk

```text
Chunk（data 区 **16 MiB 对齐**）
base ─┬─ [+0]   Chunk 头 64 字节 {next, base, size, bump, free_head,
        │                          meta_bm, meta_summary, live_bytes}
       ├─ [+64]  块 0
       ├─ ...
       └─ [+size) 块 n           （size = data 容量，16 MiB 或更大，4096 的倍数）

旁挂（每 chunk 单独 mmap，计入 heap_committed）：
  start_bitmap  256 KiB  （1 位 / 8 字节，覆盖 data 区）
  page_summary  512 B    （1 位 / 4 KiB，覆盖 data 区）
```

- `Chunk` 头**在数据区内**（前 64 字节），所以 `base` 到 `base+64` 不是块。
- 对齐做法：`mmap(size + 16 MiB)` → `base = (r + 16MiB - 1) & ~(16MiB-1)` →
  前缀与后缀各 `munmap`。前缀不进 `heap_committed`（已还）。
- `id = base >> 24`。**判「是不是堆指针」= `chunk_by_id(p >> 24) != null`**（O(1)）。
  `p >> 24 == base >> 24` 对所有 `base <= p < base + 16 MiB` 成立 —— 因为
  `base` 16 MiB 对齐而 `size >= 16 MiB`。**`size > 16 MiB` 的 chunk 只认首 16 MiB**
  （`p >> 24` 只覆盖第一个 16 MiB 段）；超过 16 MiB 的 chunk 在
  `chunk_by_id` 里登记**每一段**的 id（`size >> 24` 项），这是 §1.4 的唯一例外。
- `chunk_by_id`：64 槽开放寻址哈希表，键 `id`，值 `*Chunk`。**满 64 个 chunk
  不再增长**（64 × 16 MiB = 1 GiB > 缺省上限 256 MiB，够用；撞满就 OOM）。

### 1.2 记账口径

| 量 | 定义 | 谁读 |
|---|---|---|
| `heap_committed` | Σ 每个 chunk 的（data 容量 + 64 头 + 元数据 288 KiB），**不含**已 munmap 的对齐前缀 | `rt_heap_used()` / `RGOC_HEAP_MAX` / 统计行 `heap=` |
| `live_bytes` | 上次清扫后**存活块的块总字节（含 16 字节头）** | 统计行 `live=` |
| `chunk_count` | chunk 数 | 统计行 `chunks=` |

`RGOC_HEAP_MAX`（缺省 256 MiB）按 `heap_committed` 算 —— **与 M9 同口径**，
只是多算了元数据的 288 KiB / chunk。所以同样的 `RGOC_HEAP_MAX` 下，M10 能装的
对象比 M9 少一点（1.8%），这是可接受的（`T-GC-07` 的判据是「文本与退出码不变」，
不是「能装同样多」）。

### 1.3 分配

```text
gc_alloc(need, kind, desc, n):
  1. 遍历所有 chunk 的 free list，first-fit（余量 >= 32 字节才切分；否则整块给出）
  2. 都不行 ⇒ 遍历所有 chunk 的 bump
  3. 都不行 ⇒ 新 chunk，从它的 bump 切
  4. 仍不行（`rt_grow` 里超上限）⇒ rt_fatal_oom(heap_limit)（`C9` §6.1 文本不变）
```

⚠️ **free list 必须排在 bump 之前**（`R-10-12`）。第一版写成「先 bump、bump
用完再用 free list」，结果是 **free list 形同虚设**：bump 一用完就开新 chunk，
于是回收的块**永远不会被复用**。实测症状：`G` 的块进了 free list 但内容从没
被覆盖，「全局根保活」的变异测试假绿三次。语义上也该如此 —— free list 里的块
是已经付过 mmap 代价的。

- **块大小恒 16 的倍数且 ≥ 16**：`blk = (16 + data + 15) & ~15`。
- 分配出来的 `data = blk + 16`，**恒 16 对齐**（`blk` 是 16 的倍数、`base+64`
  是 16 的倍数）。
- `need_zero`：**复用的块必须清零**；新 bump 出来的块在零页上，可省（但实现
  **不省** —— 一条 memset 比「判断这块是新页还是复用」便宜，且是唯一的防线）。
- **零大小对象不进托管堆**（`rt_zerobase`）。

#### 「已分配但还没发布」的两条出路（`R-10-18`）

`gc_alloc` 在 stress 下会在**分配之前**先跑一次 GC。所以「刚分配出来、还没写进
任何可达位置」的块是**看不见的** —— 多块构造函数必然有这么一个瞬间。

| 场景 | 机制 | 例子 |
|---|---|---|
| 中间**还要再分配一次** | `gc_pin[]`（深度 8）：`mark_roots` 扫它 | `map_new` 先建槽块、再建 `MapObj` |
| 一次分配 + **立即发布** | `gc_pause++` / `--`：抑制触发 | `rt_alloc_escaped`（§4.5）|

⚠️ 实测（`R-10-18`）：`map_new` 先建槽块再建 `MapObj`，第二次分配前的 GC 把
槽块收掉，而 `MapObj` 恰好**复用了同一块**（free list 优先）⇒ `m->slots` 指向
自己的中间 ⇒ `m[0]` 读出 0。`gc_pin` 就是为这个加的。

溢出**不静默**：`gc_pin_push` 超深直接 abort —— 静默丢一个 pin 的后果是
「随机时刻丢对象」，比崩难查一百倍。

### 1.4 不变量

1. 活块的 `blk` 是 16 的倍数，且 `blk >= 16`。
2. 活块的 `data` 落在 `[base+64, base+size)` 内，且 `data + data_size <= base + size`。
3. 块与块在 chunk 内**不重叠、无缝隙**（bump 与 free list 合起来铺满 data 区）。
4. `live_bytes <= Σ 活块 blk`，等号在「清扫刚结束」时成立。
5. `heap_committed >= 16 MiB × chunk_count`。

---

## 2. 块头与 kind

### 2.1 16 字节块头（`D-M10-2`）

```text
块 = [+0 u64 header][+8 u64 n|kind|blk][+16 data ...]

header（活块）：
  bit0  = 0
  bit1  = 已标记（marked）
  bit2  = raw（**不扫描**的快速位）
  bit3..63 = desc 指针         （TypeDesc 8 对齐 ⇒ 低 3 位恒 0）
header（空闲块）：
  bit0  = 1
  bit1..31 = 0
  bit32..63 = 本块总字节数（含头；<= 16 MiB 故装得下）
+8（活块）：n | (kind << 32) | ((blk >> 4) << 35)
  bit0..31  = n
  bit32..34 = kind
  bit35..63 = 块总字节数 >> 4（块恒 16 的倍数；16 MiB ⇒ 只需 20 位）
+8（空闲块）：next 空闲块地址（0 = 链尾）
```

⚠️ **块大小必须能从块头读出来**（`R-10-9`，Phase 1 实测改的）。一开始想按
`kind + n + desc` 算（`object` 用 `desc->size`、`array` 用 `elem->size * n`…），
但 **`desc` 非法时算不出来** —— 而 `sweep` 是按地址顺序走的，算错一步就会
**走到块中间**、把中间当块首，之后整条链都错位。契约里说「宁可泄漏也不误扫」，
所以大小必须**自证**。代价是 `n` 从 32 位让到 32 位（不变）、`kind` 只要 3 位、
剩下 29 位给 `blk >> 4`（最大 8 GiB，够）。

**读 desc 一律 `(const TypeDesc *)(header & ~7UL)`。** 任何地方直接 `(TypeDesc*)header`
都是 bug（tag 位会带进去）。

`n` 的语义按 kind 而定（§2.2）。`kind` 占 `n` 的高 32 位 —— `n` 因此上限 2^32，
足够（`n` 是元素个数 / 捕获个数 / 字节数 / 槽数）。

### 2.2 五种 kind

| kind | 名字 | `desc` | `n` | data 布局 | 扫描 |
|---|---|---|---|---|---|
| 0 | `object` | 对象类型 desc | 0 | 值本体 | `[0, desc->ptrdata)` 按 `desc->gcdata` |
| 1 | `array` | **元素** desc | 元素个数 | 元素数组 | `n` 个元素，每个按 `elem->gcdata` |
| 2 | `closure` | null | 捕获个数 | `[fn, cap0, cap1…]` | **跳过字 0**（代码地址），扫 `n` 个捕获字 |
| 3 | `raw` | null | 字节数 | 字节 | **不扫** |
| 4 | `mapslots` | null | 槽数（cap） | `[+0 kdesc][+8 vdesc][+16 槽…]` | 每个 LIVE 槽的 key 与 value |
| 5 | `arena` | null | 字节数 | 字节 | **不扫、永不回收**（`R-10-6`）|

- `object` 的 `n` 恒 0 —— 元素个数只在 `array` 有意义。
- `array` 的 `desc` 是**元素** desc（`[]T` 的 `elem`），不是切片类型 desc。
- `closure` 的 `n` = 捕获个数；data 字 0 是 `fn`（代码地址，`mark_ptr` 会忽略，
  但**不扫更省**）。
- `mapslots` 的 data 前 16 字节是**两个 desc 指针**（`kdesc`/`vdesc`），
  槽数组从 `+16` 起。槽的 stride = `8 + align8(kdesc->size) + align8(vdesc->size)`，
  槽字 0 是 `state`（0=空 / 1=LIVE / 2=墓碑）。**GC 只扫 `state == 1` 的槽。**
  这样 mapslots 块**自身完备**（不需要回到 `MapObj` 才能扫）。

### 2.3 `gcdata` 与 `ptrdata` 的一致性（**新增不变量**）

`C5` §11.1 说「`ptrdata == 0` 时 `gcdata` 为 null」。本契约补上反方向：

> **`ptrdata > 0` ⇒ `gcdata` 必须非 null，且 `gcdata` 至少覆盖
> `ceil(ptrdata / 8)` 个位。**

违反时的行为：`bad_desc++`，该块按 **`raw`** 处理（不扫）。宁可泄漏也不误扫。

⚠️ 已知违反点：`rt.c` 的 `g_panic_str_desc`（`string`，`ptrdata=8` 而 `gcdata=0`）。
M10/Task 24 必须把它改成 `gcdata = 1`（bit0 = 第 0 个字是指针）。

### 2.4 `raw` 位的用途

`bit2` 与 `kind == 3` 是**同一件事的两种编码**。保留 bit2 是因为扫描器
**只看第一个字**就能跳过 raw 块；读 `+8` 拿 kind 要多一次访存，而 raw 块
（字符串、`[]byte`）在语料里占多数。

两者不一致时的行为：以 `kind` 为准（`bit2` 只当提示）。

---

## 3. 起始位图与内部指针

### 3.1 为什么需要（`D-M10-4`）

Go 的指针**可以指向块中部**：`s[2:]`、`&a[i]`、子串 `t[1:]`。而 GC 需要块首
才能读块头。**不移动**（`D-M10-12`）意味着没有 forwarding 可查 ⇒ 必须能
从任意内部地址**回扫**到块首。

### 3.2 位图

| 表 | 粒度 | 大小 / chunk | 置位时机 |
|---|---|---|---|
| `start_bitmap` | 1 位 / 8 字节 | 256 KiB | 分配（bump 或 free list 切出）时置位；块合并成空闲 run 时清除 |
| `page_summary` | 1 位 / 4 KiB | 512 B | 该页内**出现第一个**块起点时置位；该页**没有任何**起点时清除 |

位下标一律用**相对 `base` 的字节偏移**：`start_bitmap[off >> 6] & (1 << ((off >> 3) & 63))`
覆盖 `[base + off, base + off + 8)`。

### 3.3 `resolve_block`（三步）

```text
resolve_block(c, p) -> u8 *（块首）| null
  1. off = (u64)p - (u64)c->base；若 off < 64 或 off >= c->size ⇒ null
  2. page = off >> 12
     若 page_summary(page)：
        从字 w = off & ~7UL 起，向下到 page << 12，逐个 bm_test(w >> 3)；
        命中 ⇒ 返回 base + w
        （**必然命中**：本页有起点且本页的起点是 <= p 的最大起点，见 3.4）
  3. 否则 q 从 page-1 递减到 0，找第一个 page_summary(q) 为真的页：
        从字 w = ((q+1) << 12) - 8 起，向下到 q << 12 逐个 bm_test；
        命中 ⇒ 返回 base + w
      q 到 0 都没找到 ⇒ null
```

### 3.4 正确性论证（写下来，因为它就是 `T-HEAP-05` 的推理）

设块起点为 `s`，`p` 在块内（`s <= p`）。块内除 `s` 外没有起点（不变量 §1.4.3）。

- 若 `s` 与 `p` 同页：本页有起点 ⇒ 走第 2 步，本页内 `<= p` 的最大起点就是 `s`。
- 若 `s` 在更早的页：则**本页不可能有起点** —— 因为本页若有起点 `s'`，
  则 `s' > s` 且 `s' <= p`（本页起点必 ≤ p，否则它在本页内但 > p… 注意本页
  起点可能 > p，那就不是「≤p 的最大」）。等等 —— 本页可能有起点但都 > p。
  此时第 2 步在本页扫到页顶也找不到 ⇒ **必须继续往前找**。
  ⇒ **第 2 步找不到时要退化成第 3 步**，不能直接返回 null。

修正后的第 2 步：本页找不到就进第 3 步（从 `page-1` 起）。这个修正
**必须**在实现里体现 —— 漏了它，症状是「1 MiB 数组里 `&s[3]` 返回 null」，
而 `&s[1MiB-1]`（在本页末尾、起点在页头附近但跨页）反而可能碰巧对。

`bad_ptr` 计数器：`resolve_block` 返回 null 但 `p` 在某个 chunk 范围内 ⇒ `bad_ptr++`。
它是「位图漏置位」的唯一可观测信号。

---

## 4. 根集合与帧记录 ABI

### 4.1 八类根

| # | 根 | 来源 | 扫描 |
|---|---|---|---|
| 1 | 栈 | `g_frames` 链 | §4.2 |
| 2 | 全局 | `rt_globals[i] = {desc, addr}` | `[addr, addr + desc->ptrdata)` 按 `gcdata`；零大小全局不在表里 |
| 3 | defer 链 | `g_defer_top->fv` | `mark_ptr(fv)`（闭包块或 rodata funcval）|
| 4 | panic 值 | `g_p_text` / `g_saved_text` | `mark_ptr`（托管 raw 块）|
| 5 | map 槽 | `MapObj` → `slots` 块 | 由 kind `mapslots` 的扫描负责（§2.2）|
| 6 | 闭包捕获 | kind `closure` | §2.2 |
| 7 | 数组元素 | kind `array` | §2.2 |
| 8 | **pin 槽** | `gc_pin[]`（深度 8）| `mark_ptr` —— 「已分配但还没发布」的块（§1.3）|

**不扫**：帧记录的 `prev`（栈地址）/ `fi`（rodata）、`rt_zerobase`、itab（rodata）、
静态 funcval（rodata）、`rt_arena_alloc` 的结构（§6 逐条核）。

`mark_ptr(p)` 的第一步永远是「在堆里吗」：
`chunk_by_id((u64)p >> 24)` 为 null ⇒ 直接返回。**这一步是全部 rodata/栈地址
被忽略的唯一机制** —— 没有它，`mark_ptr` 会把 `desc` 指针当对象。

### 4.2 帧记录（`D-M10-7`）

```text
fp-16: prev      上一帧的 fp（链头是全局 g_frames；0 = 链尾）
fp-24: fi        *RtFuncInfo（`C5` §11.4；M9 已发在 .rodata）
fp-32: sp_idx    u64，本帧**当前**的 safepoint 下标
```

- 记录区 `[-32, -8)`：`fp-8` 是 M7 就留着的间隙（仍未用），`fp-16/-24/-32`
  三个字是记录。**值槽从 `-40` 起**（M9 是 `-16`）⇒ `SAVED` 16 → **32**。
- `frame_size = 32 + sub_sp_imm`（空函数也要 `sub 16`，那 16 字节就是记录区）。
- **只有 `lv.safepoints` 非空的函数发记录**（`D-M10-8`）。理由：没有 call /
  alloc / 回边 ⇒ 不可能停在 GC 点。判据：`.s` 里出现 `g_frames` 的函数集合
  == 有 safepoint 的函数集合。
- prologue（在 `sub sp` 与存 `fp/lr` **之后**）：
  `prev = g_frames; fi = &__funcinfo_N; sp_idx = 0; g_frames = fp`
- epilogue（`epilogue()` 是唯一出口，`F14`）：`g_frames = prev`
- 每个 safepoint **之前**：`sp_idx = <该位点的下标>`
- `sp_idx` 的下标空间 = `.stackmap` 里该函数的 `sp` 行序（**`(block, idx)` 升序**，
  `F6`）。两处**必须同源**：`Liveness` 只算一次，`stack_map_of` 与 `sp_index`
  都从它派生（`M10-design.md` §8 风险 1）。

### 4.3 扫一帧

```text
fi = *(RtFuncInfo**)(fp - 24)
idx = *(u64*)(fp - 32)
if (idx >= fi->n_sp) { bad_sp++; continue; }        # 跳过该帧，不崩
bits = fi->sp_bits + fi->sp_off[idx]                # sp_off 是**字**下标（不是位偏移！）
base = fp - fi->frame_size
for i in 0..sp_words:
    w = bits[i]
    while (w) { j = ctz(w); w &= w-1; mark_ptr(*(void**)(base + 8*(64*i + j))); }
```

`sp_words = ceil(frame_size / 8 / 64)` 是**导出量**（`D-M10-10`：`RtFuncInfo`
不加字段）。

⚠️ 位序：bit `i` ↔ `fp - frame_size + 8i`（`C5` §8.4 冻结）。**不是** `fp - 8(i+1)`。
判据 `T-ROOT-09`（把 `base` 写成 `fp - 8` 就红）。

**`mark_ptr` 有三道防线**（`R-10-20`）：

1. `chunk_of(p)` 为 null ⇒ 返回（非堆地址）；
2. `resolve_block` 返回 0 ⇒ `bad_ptr++` 并返回（起始位图漏置位）；
3. **块头结构校验**：`kind <= K_ARENA`、`blk_size >= BLK_MIN`、
   `offset + blk_size <= chunk->bump`。任一不满足 ⇒ `bad_ptr++` 并返回。

第 3 道是 Phase 2 加的第二防线：`scan_block` 拿一个假 desc 去读 `ptrdata`
会**直接段错误**，而「跳过」最坏只是泄漏 —— 契约里说得很清楚：宁可泄漏也不误扫。

### 4.4 位图是 live-**out** 的保守超集（`R-10-21`）

⚠️ **这不是精确的 live-in**。`live.rs` 用的是 `after[ii]`（执行完第 `i` 条之后
还活着的值），而 GC 在指令**执行之前**跑 ⇒ 「本指令自己的 def」也在集合里，
那一刻它的槽里还是**上一条命的值**。

**代价不对等**：多标 = 泄漏一轮；漏标 = 活对象被回收。所以保守优先。

#### 4.4.1 「真 live-in 会坏」已查清（`G-M10-06`，2026-10-27）

曾经试过把 def 从集合里去掉，实测让 `defernil` / `print` / `nilptr2` /
`method5` 全部坏掉，当时记为「机制没查清」并保留超集。**现在查清了，而且
根因不在位图。**

```text
MakeClosure  ──► 闭包块分配，写入 dst 槽
Defer        ──► bl rt_defer_push(fp, fv)
                    └─ rt_arena_alloc  ──► gc_alloc(K_ARENA)
                                              └─ stress 钩子在**分配之前**跑 GC
                                                 ① 新 DeferRec 还没挂上 g_defer_top
                                                 ② Defer 不是 safepoint
                                                    ⇒ sp_idx 停在 MakeClosure 位点
                                                 ⇒ 闭包块的唯一根 = 那张位图
```

超集恰好含 `MakeClosure` 的 dst，所以 M10 一直没露 —— 那是**巧合**，不是保证。
去掉 def ⇒ 闭包被回收 ⇒ 堆地址被下一个闭包复用 ⇒ `rt_deferrun` 调到**别人的**
闭包 ⇒ panic，且 `recover()` 救不回来（`defernil` 就是这么坏的）。

**修法**：`rt_defer_push` 加 `gc_pause`（`R-10-24`，见 §6.1）。修完之后真
live-in 也能跑绿 —— 四个语料 × 四种组合的读数在 `scripts/g-m10-06-probe.md`。

⚠️ **修完仍然保留超集**：这是策略选择，不是给那个缺口打的补丁。理由还是
「代价不对等」，不是「位图必须这样」。

**副作用**：扫到「指向已回收块的指针」成为常态。所以 `bad_ptr` **只计真错**
（上面第 2、3 道防线），「指向空闲块」不计 —— 否则探针恒非 0，没人再信它。
判据 `T-ROOT-10` 因此改成读**内容**（`sp_idx` 卡在 0 会让后面的指针漏标）。

### 4.5 逃逸槽必须进托管堆（`R-10-23`）

`p := &x` 而 `p` 逃出函数（被闭包捕获 / 存进全局）时，`x` 的存储不在帧里：
帧里只留一个 8 字节**指针格**，存储本身走 `rt_alloc_escaped(desc)`。

⚠️ **M9 用的是 `rt_arena_alloc`** —— arena 不扫描，于是「闭包捕获 `&x`、
而 `x` 里是指针」这种形状就丢了根：闭包块里的捕获字是 `&槽`（**栈地址**），
`mark_ptr` 直接忽略，槽里那个指针一个字都没被标。

实测：`p := new([1024]byte); f := func() int { return p[0] }; churn(); f()`
在 stress 下打印 `204` 而不是 `42`。判据 `T-ROOT-14`。

`rt_alloc_escaped` 里 `gc_pause++`：它在**序言**里分配，块要等 `st_x` 之后才
可达，中间不能有 GC。关掉触发是安全的（从分配到发布之间没有别的分配点）。
`gc_pause` 与 `gc_pin`（§1.3）的分工：`gc_pin` 解决「中间还要再分配一次」
（map 的两块），`gc_pause` 解决「一次分配 + 立即发布」。

### 4.6 为什么没有寄存器根（`D-M10-9`）

每个值一个栈槽（`C5` §7）⇒ 任何跨调用的活值都同时在槽里；`isel` 是
`ld` 到寄存器再 `bl`，不覆盖槽。所以扫栈就够。

**这条要写成判据**（`T-GC-10`：`.s` 里不出现 `stp x19` / `str x19`）。
将来加寄存器分配时这条会红 —— 红的时候必须**补寄存器根**，不能改判据。

---

## 5. 触发与统计

### 5.1 环境变量

| 变量 | 值 | 作用 |
|---|---|---|
| `RGOC_GC` | `0` | **关**：`rt_gc()` 立即返回，分配失败直接 OOM（与 M9 等价）|
| | 缺省 / 其它 | 开：自动触发（§5.3）|
| `RGOC_GC_STRESS` | `<n>` | **每 n 次** `gc_alloc` 之前先 `rt_gc()`；`1` = 每次（最强，小 fixture 用）|
| `RGOC_GC_STATS` | `1` | 退出时向 **stderr** 打一行（§5.2）|
| `RGOC_GC_THRESHOLD` | 字节数 | 自动触发阈值（缺省 4 MiB）；`0` 表示「每次分配失败都收」|

`RGOC_HEAP_MAX` 沿用 M9（缺省 256 MiB）。

### 5.2 统计行（**语法冻结**）

一行，`\n` 结尾，字段顺序固定，**所有字段总是出现**：

```text
gc: collections=<n> allocs=<n> frees=<n> live=<n> heap=<n> chunks=<n> rss=<n> kinds=<o>,<a>,<c>,<r>,<m> bad_sp=<n> bad_ptr=<n> bad_desc=<n> mode=<on|off|stress>
```

- 前缀 `gc: `（含空格），字段间单个空格，键值间 `=`，无空格。
- `kinds` 是**五个**逗号分隔的十进制计数，顺序 `object,array,closure,raw,mapslots`。
- `bad_*` 三个**总是出现**（不是「非 0 才追加」）。
- `rss` 是**峰值** RSS（KB），`getrusage(RUSAGE_SELF).ru_maxrss`（偏移 32，
  Linux aarch64）。取不到就是 0 —— 字段必须出现，值可以是 0。
- `mode` 是**解析后的**生效态：`RGOC_GC=0` ⇒ `off`（即使同时设了 `STRESS`）；
  否则 `RGOC_GC_STRESS=1` ⇒ `stress`；否则 `on`。

> ⚠️ **推翻 `M10-plan.md` Task 4 Step 2 的初稿口径**（「`bad_*` 只在非 0 时追加」）。
> 理由：「不出现」与「=0」是两件事，判据要写两遍；总是出现让
> `bad_sp=0 bad_ptr=0 bad_desc=0` 成为**一条**断言，而且 diff 更稳。
> 记 `R-10-2`。
>
> ⚠️ **`mode` 是 Task 5 追加的**（`R-10-3`）。理由：Phase 0 的三态**必然等价**
> （`rt_gc()` 还不存在），所以「三态跑出同样的字节」这件事在 Phase 0 里
> **不携带信息** —— 如果 `run-native.sh --gc=` 的分支写错了（比如 `stress`
> 分支漏了变量），三态照样一致。`mode` 让「环境变量真的生效」变成可观测的，
> 于是 `T-CORP-00` 能断言「三个态确实是三个态」。它是**装置判据**，不是
> 实现判据 —— 而装置说谎比实现有 bug 难查得多。

**写入点**：由**生成的 `main` 出口**调用 `rt_gc_stats_line()`（`module.rs` 在
`main` 的 `ret` 之前插一条 `bl`），**不**依赖 libc 的 `atexit`（freestanding 没有）。
`gc_stats == 0` 时该函数立即返回 ⇒ 不打任何字节。

### 5.3 自动触发

```text
每次 gc_alloc（分配**之前**）：
  gc_since += need
  next = max(gc_threshold, gc_live_bytes * 2)
  gc_since >= next  ⇒  rt_gc()
```

⚠️ **推翻 `M10-plan.md` Task 33 的初稿口径**（「分配失败且 `heap_committed >
threshold` ⇒ 收一次再重试」）。那条**永远不会触发**：chunk 是 16 MiB，而语料里
的程序分配量都在 16 MiB 以内 ⇒ bump 永远够 ⇒ 自动态退化成关态，`off` 与 `on`
两态**逐字节相同**而没人发现。记 `R-10-25`。

现在的口径看 **`gc_since`（上次 GC 之后分配的数据字节）**，不看 mmap 量：
`heap_committed` 一跳就是 16 MiB，粒度太粗。阈值随存活集自适应
（`gc_live_bytes * 2`）—— 堆越大收得越少，与 Go 的 `GOGC=100` 同形。

- **`gc_pause > 0` ⇒ 不触发**（GC 期间、以及 `rt_alloc_escaped` 的发布窗口）。

- **`RGOC_GC=0` ⇒ `rt_gc()` 立即返回** ⇒ 第 4 步等于直接 OOM。
- **`RGOC_GC_STRESS=<n>` ⇒ 每 n 次分配前先收**（此时阈值不起作用）。
  ⚠️ `n` 是**间隔不是开关**（`R-10-10`）：`n=1` 是每次分配都收，代价 O(n²) ——
  `gc1.go` 的 1e5 次分配 × 平均块数 ≈ 1.7e10 次块扫描，语料里跑不完。
  所以小 fixture 用 `1`（最强证据），语料用 `64`。
- **GC 期间不分配**（`H5`）：标记与清扫里出现 `gc_alloc` 就是 bug。
  worklist 走 `mmap` 直调，不走 `gc_alloc`。

### 5.4 探针

| 符号 | 返回 | 用途 |
|---|---|---|
| `rt_gc_collections()` | `u64` | 收了几次 |
| `rt_gc_live_bytes()` | `u64` | 上次清扫后的存活块字节 |
| `rt_gc_alloc_bytes()` | `u64` | 累计分配字节（= Σ data 字节，不含头）|
| `rt_gc_bad_sp()` | `u64` | 帧记录 `sp_idx >= n_sp` 的次数（**必须 0**）|
| `rt_gc_bad_ptr()` | `u64` | 堆内指针**解析不到块首**或**块头结构非法**的次数（**必须 0**）。⚠️ 「指向已回收的块」**不计** —— 保守位图下那是常态（§4.4）|
| `rt_gc_bad_desc()` | `u64` | 块头 desc 非法的次数（**必须 0**）|

探针**不新增内建**：测试读 §5.2 的统计行。

---

## 6. `rt_arena_alloc` 的保留面

`D-M10-1`：托管堆只放**含 Go 值语义**的对象。下面逐条列出「现在还留在 arena」
的结构，以及为什么它可以留（**不含指向托管堆的指针**）。

| # | 调用点（`rt.c`）| 结构 | 为什么留 arena |
|---|---|---|---|
| 1 | `write_all` 的输出缓冲 | `char[nb]` | 纯字节；且 `write_all` 在 GC 期间**也会被调用**（统计行、panic 文本），它不能触发分配 |
| 2 | `rt_itab` | `{iface, concrete, fn[]}` | 三个字段全是 **rodata / 代码地址**；不含堆指针 |
| 3 | `rt_defer_push` 的 `DeferRec` | `{next, fp, fv}` | 运行时簿记。`next` 是 arena 地址、`fp` 是栈地址（`mark_ptr` 都忽略）；`fv` **是根**，但由 §4.1 的第 3 类**单独扫**，不靠扫这条链。⚠️ **分配期间必须 `gc_pause`**（`R-10-24`，见 §6.1）|
| 4 | `panic_set` 的文本 | `char[n+1]` | 纯字节 |
| 5 | `rt_recover` 的 box | `{ptr, len}` 的 string | 含一个堆指针，但它指向的文本**也在 arena**（永不回收）⇒ 不需要标记 |
| 6 | chunk 元数据 / worklist | 位图、页摘要、标记栈 | GC 自己用；参与回收就没有底了 |
| — | 超大帧（`rt_stack_frame_alloc`）| `mmap`/`munmap` 成对 | **不是 arena**，也不进 `heap_committed`；`R-9-22` 不变 |

**移出 arena（进托管堆）**：

| 结构 | 变成 | 谁改 |
|---|---|---|
| `rt_alloc`（`new`/`rt_box`）| kind `object` | Task 8 |
| `rt_makeslice` / `slice_grow` 底层数组 | kind `array` | Task 9 |
| `MakeClosure` 的闭包块 | kind `closure` | Task 9 |
| `rt_bytes2str` / `rt_runes2utf8` / `rt_strcat` / `rt_i64_to_str` | kind `raw` | Task 9 |
| `MapObj` + `m->slots` | kind `object`（`rt_desc_mapobj`）+ kind `mapslots` | Task 25 |
| **逃逸的取址槽**（`p := &x` 且 `p` 逃出函数）| kind `object`（`rt_alloc_escaped(desc)`）| Task 28（`R-10-23`）|

⚠️ **arena 分配也带 16 字节块头**（kind = `arena` = 5，`R-10-6`）。不给头的话，
`sweep` 按起始位图走时会把 arena 的区域当成「空闲」塞进 free list —— 之后被
`gc_alloc` 复用就覆盖了 itab / DeferRec。所以「托管堆与 arena 分离」（`D-M10-1`）
体现在**语义**（arena 块永不回收、不扫描），**不在物理上**（同一个 chunk 链，
省一套记账）。

**守卫**（`T-GC-11`）：`gc_arena.rs` 扫 `rt.c` 源码，任何 `rt_arena_alloc`
调用点都必须出现在上表里；多出来的就红（反向也查：表里有、源码里没有 ⇒ 红）。
**新增一个 arena 调用点必须同时改本表与那张 `ARENA_SITES` 表** —— 这是
「不留后门」的唯一机制。

⚠️ 表按**源码片段**认，不按行号：行号会随任何无关改动漂移，那样守卫每周都要
修一次，最后没人维护；而片段本身被改动时，那正是应该重审保留面的时刻。

### 6.1 `rt_defer_push` 必须 `gc_pause`（`R-10-24`）

```c
gc_pause++;
DeferRec *r = rt_arena_alloc(sizeof(DeferRec), 8);
r->next = g_defer_top; r->fp = fp; r->fv = fv; g_defer_top = r;
gc_pause--;
```

**为什么**：`rt_arena_alloc` 走 `gc_alloc`，而 `gc_alloc` 的 stress 钩子在
**分配之前**跑 GC。那一刻新记录还没挂上 `g_defer_top` ⇒ `mark_defer` 看不到
这个闭包；而 `Defer` 不是 safepoint ⇒ `sp_idx` 还停在 `MakeClosure` 的位点。
于是闭包块的唯一根就是那张位图。

与 `rt_alloc_escaped`（`R-10-23`）**同一个理由**：一次分配 + 立即发布，中间没有
别的分配点 ⇒ 这段时间里不产生垃圾 ⇒ 不收也安全。`gc_pin` 是「中间还要再分配
一次」时才需要，这里不需要。

**证据**：`scripts/g-m10-06-probe.md`（`G-M10-06` 的调查记录）。去掉这个
`gc_pause` 且位图取真 live-in ⇒ `defernil`/`print`/`nilptr2`/`method5` 全坏，
且坏法是「闭包块被回收 ⇒ 地址被下一个闭包复用 ⇒ defer 调到别人的闭包」。

---

## 7. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-27 | **`G-M10-06` 查清**：§4.4 加 §4.4.1（真 live-in 会坏的根因是 `rt_defer_push` 的「分配未发布」窗口，不是位图）；新增 **§6.1 `R-10-24`**（`rt_defer_push` 必须 `gc_pause`）；§6 表第 3 行补约束 | `scripts/g-m10-06-probe.md`：四个语料 × 四种组合的读数 + trace（`push-fv` 地址复用 / `gc-begin:3` 无 `mark-fv`）；修法后真 live-in 也全绿 |
| 2026-10-27 | **Phase 2 定稿**：§4.1 八类根（加 pin 槽）；§4.2 记录区改成「`[-32,-8)`、值槽从 `-40` 起、`frame_size = 32 + sub`」；§4.3 修正 `sp_off` 是**字**下标（原来写成位偏移）并补 `mark_ptr` 三道防线；新增 §4.4（保守超集）/ §4.5（逃逸槽）；§5.4 `bad_ptr` 口径收窄；§6 表补到 6 条 + 按源码片段认 | 实测：① `sp_off` 按位偏移读会扫到帧外的字；② 真 live-in 位图让 `defernil`/`print`/`nilptr2`/`method5` 全坏；③ 逃逸槽在 arena ⇒ 闭包捕获 `&槽` 时槽里的指针没有根；④ 起始位图漏维护 ⇒ 假块首 ⇒ 段错误（`ken/simpswitch.go`）|
| 2026-10-27 | **§2.1 块头编码加 `blk >> 4`**（`R-10-9`）；§1.3 分配顺序改成 **free list 优先**（`R-10-12`）；§2.2 加 `arena` kind（`R-10-6`）；§5.1 `RGOC_GC_STRESS` 改成**间隔**（`R-10-10`）；§6 补「arena 也带块头」| Phase 1 实测：① 块大小不能靠 desc 算（非法 desc 时 sweep 会走到块中间）；② bump 优先让 free list 形同虚设（判据假绿三次）；③ 不给 arena 块头会让 sweep 把 itab 当空闲；④ `stress=1` 在 1e5 次分配的语料上是 O(n²) 跑不完 |
| 2026-10-26 | **草案创建**（M10/Task 1）：§1 堆形状 / §2 块头与 kind / §3 起始位图 / §4 根集合与帧记录 / §5 触发与统计 / §6 arena 保留面 | `M10-design.md` `D-M10-1..15` / `F1..F14`；用户拍板三件事（16 字节块头 / 逐位点 `sp_idx` / map 进托管堆）|
