# M10 实施计划：精确 STW GC

> 输入：`M10-design.md`（决策与门禁）、`M10-tests.md`（判据 ID）。
> 每个 Task 的 Step 1 都是「写会红的判据」；没有判据的 Task 不许开工。

---

## Global Constraints

1. **只改 native 通路**：`rgoc-interp`（`--via=hir` / `--via=ssa`）一行不动。
2. **合并流不变**：`C9` §5 的输出字节表一字不改；统计只进 stderr。
3. **位点顺序是契约**：`live.rs` 的 `(block, idx)` 升序是 `STACK-MAP v1` 的一部分，
   M10 **只读**它，不改它。
4. **`RtFuncInfo` 不加字段**：`sp_words = ceil(frame_size/8/64)` 是导出量。
5. **GC 期间不分配**：标记与清扫里出现 `gc_alloc` 就是 bug（`H5`）。
6. **三态语料每 Phase 收尾都跑**：`off` / `on` / `stress`，不攒到最后。
7. **Phase 1 不许开自动触发**：那时只有全局根，自动收会收掉栈上的活对象。
   自动触发在 Phase 3 打开（`D-M10-11` 的落地顺序）。
8. **容器纪律**：`RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && …'`；
   变异与恢复同一调用内完成，恢复后 `touch`（M9 的教训）。

## Review Focus

1. **块头 tag 位**：bit0 空闲 / bit1 标记 / bit2 raw。desc 指针必须 8 对齐 ⇒
   低 3 位可用。任何读块头的地方都要 `& ~7` 再当指针用。
2. **内部指针**：`s[2:]` / `&a[i]` / 子串三个来源都要有 fixture；`bad_ptr` 必须 0。
3. **`sp_idx` 与 `.stackmap` 同源**：`Liveness` 只算一次，两处都从它派生。
4. **帧记录只发给含 safepoint 的函数**：不含 call/alloc/回边的叶子函数不发
   （它不可能停在 GC 点）。这条要有判据：`.s` 里没有 `g_frames` 的函数集合 == 没有 safepoint 的函数集合。
5. **清扫的合并**：run 是有序的，合并只发生在相邻块之间；错一次就会两块重叠。

---

# Phase 0 · 契约与骨架

### Task 1: `C12` 草案

**Files:**
- Create: `docs/contracts/C12-heap-and-gc.md`

**Interfaces:**
- Produces: 契约（堆形状 / 块头 / kind / 起始位图 / free list / 根集合 / 帧记录 ABI / 环境变量）

- [ ] **Step 1**: 写 §1 堆形状、§2 块头与 kind、§3 起始位图与内部指针解析、
  §4 根集合与帧记录 ABI、§5 触发与统计、§6 `rt_arena_alloc` 的保留面（逐条列出
  现在还用它且**不含**堆指针的结构）、§7 变更记录。
- [ ] **Step 2**: 在 `docs/README.md` 的契约清单里加一行（状态「草案」）。
- [ ] **Step 3: Commit** `docs(m10): C12 草案（堆形状与根协议）`

### Task 2: `C5` §11.1 与 `C9` 的修订

**Files:**
- Modify: `docs/contracts/C5-abi.md`（§11.1 块头 8 → 16）
- Modify: `docs/contracts/C9-values-and-output.md`（增「统计只进 stderr」）

- [ ] **Step 1**: `C5` §11.1 写清 16 字节块头的三行布局与 tag 位；变更记录加一行。
- [ ] **Step 2**: `C9` §5 末尾加一句：`RGOC_GC_STATS` 的行**只进 stderr**，
  合并流与退出码不变。
- [ ] **Step 3: Commit** `docs(m10): C5 块头 16 字节 + C9 统计行（Task 2）`

### Task 3: 环境变量与统计骨架

**Files:**
- Modify: `runtime/native/.../rt.c`
- Test: `rgoc/crates/rgoc-driver/tests/gc_stats.rs`（新建）

**Interfaces:**
- Produces:
  - `static u64 gc_on, gc_stress, gc_stats, gc_threshold;`（`rt_init` 里解析）
  - `void rt_gc_stats_line(void)`（退出时打一行，`RT_ATEXIT` 不存在 ⇒ 由 `main` 的
    返回路径调；实现见 Step 3）
  - `u64 rt_gc_collections(void)` 等四个探针

- [ ] **Step 1: 写会红的测试**（`T-GC-02`）

```go
package main
func main() { println("hi") }
```
`RGOC_GC_STATS=1` ⇒ stdout 必须是 `hi\n`（一个字节不多），stderr 恰好一行且
以 `gc: collections=` 开头。

- [ ] **Step 2: 跑它确认红**（现在没有那一行）
- [ ] **Step 3: 实现**：`rt_init` 解析 `RGOC_GC` / `RGOC_GC_STRESS` /
  `RGOC_GC_STATS` / `RGOC_GC_THRESHOLD`；统计行由**生成的 `main` 出口**调
  （`module.rs` 在 `main` 的 `ret` 之前插一个 `bl rt_gc_stats_line`，
  且只在 `gc_stats` 非 0 时打印 —— 这样不依赖 libc 的 `atexit`）。
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(rt): GC 环境变量与统计骨架（Task 3，T-GC-02）`

### Task 4: 探针与测试装置

**Files:**
- Modify: `runtime/native/.../rt.c`（四个探针）
- Create: `rgoc/crates/rgoc-driver/tests/gc_probe.rs`

**Interfaces:**
- Produces: `rt_gc_collections()` / `rt_gc_live_bytes()` / `rt_gc_bad_sp()` /
  `rt_gc_bad_ptr()` / `rt_gc_bad_desc()`（都是 `u64`）
- 装置：`gc_probe.rs` 里 `run_with_env(name, src, env) -> Outcome`（跑 native 并
  取 stdout/stderr/exit），沿 `support/native_harness.rs` 的写法。

- [ ] **Step 1**: 装置先写「编译失败要报错」那条（沿 `native_smoke.rs` 的先例，
  防止「编译失败 = 空输出 = 通过」）。
- [ ] **Step 2**: 探针怎么读？**不做新内建**：用 `RGOC_GC_STATS` 那一行解析
  （`collections` / `live` / `heap`），`bad_*` 三个只在非 0 时**追加**到同一行
  （`bad_sp=N bad_ptr=N bad_desc=N`）。这样测试只需要读 stderr。
- [ ] **Step 3: Commit** `feat(rt): GC 探针与测试装置（Task 4）`

### Task 5: 三态语料脚本

**Files:**
- Modify: `tools/run-oracle/run-native.sh`（加 `--gc=off|on|stress`）
- Test: `rgoc/crates/rgoc-driver/tests/gc_corpus.rs`（新建，`#[ignore]` 之外的三态断言）

- [ ] **Step 1**: `--gc=stress` ⇒ 导出 `RGOC_GC_STRESS=1`；`--gc=off` ⇒ `RGOC_GC=0`。
- [ ] **Step 2**: 现在跑三态应**全绿**（GC 还没接，等价）—— 这条是「装置本身没错」的基线。
- [ ] **Step 3: Commit** `test(m10): 三态语料入口（Task 5）`

---

# Phase 1 · M10a：托管堆 + 手工 GC（只扫全局根）

> **本 Phase 结束前 `gc_auto` 恒 0**：GC 只能由 `RGOC_GC_STRESS` 或探针触发。

### Task 6: chunk 16 MiB 对齐 + 元数据

**Files:**
- Modify: `runtime/native/.../rt.c`

**Interfaces:**
- Produces: `static Chunk *gc_chunk_new(u64 cap)`（`mmap(cap + 16MiB)` → 对齐 →
  `munmap` 前缀）；`Chunk` 增加 `{base, bump, free_head, start_bitmap, summary, live_bytes}`
- `chunk_by_id`：64 槽哈希表（`id = (u64)p >> 24`）

- [ ] **Step 1: 写会红的测试**（`T-HEAP-03`）：`RGOC_GC_STATS=1` 的一行里加
  `chunks=N`，断言 `N == heap_committed / 16MiB`（向上取整）；另加探针
  `unaligned=0`。
- [ ] **Step 2: 实现**：对齐 + 元数据 mmap（256 KiB + 512 B）+ 哈希表插入。
- [ ] **Step 3: Commit** `feat(rt): chunk 16 MiB 对齐与元数据（Task 6，T-HEAP-03）`

### Task 7: 起始位图与页摘要

**Files:**
- Modify: `runtime/native/.../rt.c`

**Interfaces:**
- Produces:
  - `static void bm_set(Chunk*, u64 off)` / `bm_clear` / `bm_test`
  - `static u8 *resolve_block(Chunk*, u64 off)` —— 内部指针 → 块首（§2.2 的三步）
- Consumes: `start_bitmap` / `page_summary`

- [ ] **Step 1: 写会红的测试**（`T-HEAP-05`）：`make([]byte, 1<<20)`，取
  `&s[1<<20-1]`，用探针 `resolve=` 打出解析到的块首；断言它等于 `&s[0]`。
- [ ] **Step 2: 实现**：位图置位/清除 + 页摘要维护 + 三步回扫。
- [ ] **Step 3: Commit** `feat(rt): 起始位图与内部指针解析（Task 7，T-HEAP-05）`

### Task 8: 块头 16 字节与 `gc_alloc`

**Files:**
- Modify: `runtime/native/.../rt.c`（`rt_alloc` 改走 `gc_alloc`）

**Interfaces:**
- Produces: `static void *gc_alloc(u64 need, u64 kind, const TypeDesc *desc, u64 n)`
- 块头：`header = desc | flags`，`+8 = n | (kind << 32)`

- [ ] **Step 1: 写会红的测试**（`T-HDR-02`）：

```go
package main

type S struct{ a, b int }

func main() {
	p := new(int)
	q := new(int)
	println(uintptr(unsafe.Pointer(q))-uintptr(unsafe.Pointer(p)) >= 24)
	println(uintptr(unsafe.Pointer(p))%16, uintptr(unsafe.Pointer(q))%16)
}
```
Expected: `true\n0 0\n`

- [ ] **Step 2: 实现**：`need = align16(16 + size)`；置起始位；写块头。
- [ ] **Step 3: Commit** `feat(rt): 16 字节块头与 gc_alloc（Task 8，T-HDR-02）`

### Task 9: 四种分配入口

**Files:**
- Modify: `runtime/native/.../rt.c`（`rt_makeslice` / `rt_growslice` / `rt_concat` /
  `rt_itoa` / `rt_makemap` / `map_alloc_slots` / 闭包分配点）
- Modify: `rgoc/crates/rgoc-arm64/src/isel.rs`（`MakeClosure` 改调 `rt_alloc_closure`）

**Interfaces:**
- Produces: `rt_alloc_array(elem, len, cap, need_zero)` / `rt_alloc_raw(bytes)` /
  `rt_alloc_closure(fn, ncaps)` / `rt_alloc_mapslots(kdesc, vdesc, cap)`

- [ ] **Step 1: 写会红的测试**（`T-HDR-03`）：五种 kind 各造一个，用
  `RGOC_GC_STATS` 的 `kinds=o,a,c,r,m` 计数断言每种都出现过。
- [ ] **Step 2: 实现**：逐个替换 `rt_arena_alloc` 调用点（`T-GC-11` 的列表同步更新）。
- [ ] **Step 3: 跑语料**：69/0/0（GC 还没自动跑，纯分配路径变更）。
- [ ] **Step 4: Commit** `feat(rt): 四种托管分配入口（Task 9，T-HDR-03）`

### Task 10: free list 与复用

**Files:**
- Modify: `runtime/native/.../rt.c`

- [ ] **Step 1: 写会红的测试**（`T-HEAP-04`）：大块 → 释放（手工 GC）→ 三个小块；
  断言 `heap_committed` **不涨**。
- [ ] **Step 2: 实现**：`free_head` 单链 + first-fit + 切分（余量 ≥ 32 才切）。
- [ ] **Step 3: Commit** `feat(rt): free list 与复用（Task 10，T-HEAP-04）`

### Task 11: 标记

**Files:**
- Modify: `runtime/native/.../rt.c`

**Interfaces:**
- Produces: `static void mark_ptr(void *p)` / `static void mark_block(u8 *blk)` /
  `static void drain(void)`

- [ ] **Step 1: 写会红的测试**（`T-ROOT-12` 的雏形）：全局 `var G *[1024]byte`，
  `G = new(...)`，`churn()`，`G[0]` 仍是原值。
- [ ] **Step 2: 实现**：堆判定（`chunk_by_id`）→ `resolve_block` → 标记位 →
  压 worklist；`drain` 出栈扫块（object 走 `gcdata`）。
- [ ] **Step 3: Commit** `feat(rt): 标记与 worklist（Task 11，T-ROOT-12）`

### Task 12: 清扫

**Files:**
- Modify: `runtime/native/.../rt.c`

- [ ] **Step 1: 写会红的测试**（`T-GC-04` 雏形）：分配-丢弃 1000 次 ⇒ `live` 不涨。
- [ ] **Step 2: 实现**：逐页逐块；`marked` ⇒ 清位；`!marked` ⇒ 并入 run；
  遇活块 flush run 进 free list；顺带统计 `live_bytes`。
- [ ] **Step 3: Commit** `feat(rt): 清扫与 free list 重建（Task 12，T-GC-04）`

### Task 13: `rt_gc()` 与全局根

**Files:**
- Modify: `runtime/native/.../rt.c`

- [ ] **Step 1: 写会红的测试**：`RGOC_GC_STRESS=1` + `T-ROOT-12`。
- [ ] **Step 2: 实现**：`rt_gc()` = `mark_roots()` + `drain()` + `sweep()`；
  `mark_roots` 只扫 `rt_globals`（`desc->ptrdata` + `gcdata`）。
- [ ] **Step 3: Commit** `feat(rt): rt_gc 与全局根（Task 13）`

### Task 14: 堆判据补齐

**Files:**
- Create: `rgoc/crates/rgoc-driver/tests/gc_heap.rs`

- [ ] **Step 1**: `T-HEAP-01`（500 轮复用 + 清零）、`T-HEAP-03`（对齐）、
  `T-HEAP-04`（切分）、`T-HEAP-05`（跨页尾指针）四条落进 `gc_heap.rs`。
- [ ] **Step 2**: 每条都跑一次 `RGOC_GC_STRESS=1`（复用路径要在压力下也过）。
- [ ] **Step 3: Commit** `test(m10): gc_heap（Task 14，T-HEAP-01/03/04/05）`

### Task 15: 回收判据

**Files:**
- Create: `rgoc/crates/rgoc-driver/tests/gc_reclaim.rs`

- [ ] **Step 1**: `T-PREC-02`（死局部被回收）、`T-PREC-03`（`uintptr` 不是根）、
  `T-GC-08`（确定性）三条。
- [ ] **Step 2: Commit** `test(m10): gc_reclaim（Task 15，T-PREC-02/03、T-GC-08）`

### Task 16: 自检计数器

**Files:**
- Modify: `runtime/native/.../rt.c`（`bad_sp` / `bad_ptr` / `bad_desc`）

- [ ] **Step 1**: 三个计数器在「能发现错」的地方加：`resolve_block` 失败、
  块头 desc 非法、帧记录 `sp_idx` 越界（Phase 2 才有）。
- [ ] **Step 2**: 统计行非 0 才追加；测试断言全 0（`T-GC-12`）。
- [ ] **Step 3: Commit** `feat(rt): 三个自检计数器（Task 16，T-GC-12）`

### Task 17: 变异四条

**Files:**
- Create: `scripts/mutation-m10.sh`

- [ ] **Step 1**: `M-M10-02`（不置起始位）/ `M-M10-03`（不合并）/ `M-M10-04`
  （不写标记位）/ `M-M10-05`（去掉堆判定）四条，逐条实测变红。
- [ ] **Step 2: Commit** `test(m10): 变异脚本骨架（Task 17）`

### Task 18: `C12` §1–§3 定稿

- [ ] **Step 1**: 把实现期与设计稿不一致的地方改回契约（块头位、对齐、切分阈值）。
- [ ] **Step 2: Commit** `docs(m10): C12 §1–§3 定稿（Task 18）`

### Task 19: Phase 1 收尾

- [ ] **Step 1**: 三态语料（此时 `on` 与 `off` 等价 —— `gc_auto` 还是 0）；
  69/0/0。
- [ ] **Step 2**: 四条 cargo 全绿；`cargo test --workspace` 不回归。
- [ ] **Step 3: Commit** `chore(m10): Phase 1 收尾`

---

# Phase 2 · M10b：根协议（栈 + 六类记录根）

### Task 20: `Liveness` 前移与 `sp_index`

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/module.rs`（`compile` 里先算 `lv` 再 `isel::func`）
- Modify: `rgoc/crates/rgoc-ssa/src/live.rs`（`pub fn index_of(&self, block, idx) -> Option<u32>`）
- Modify: `rgoc/crates/rgoc-arm64/src/isel.rs`（`Ctx` 加 `sp_index: &BTreeMap<(u32, usize), u32>`）

**Interfaces:**
- Produces: `isel::func(..., lv: &Liveness)`；`Ctx::sp_of(block, idx) -> Option<u32>`

- [ ] **Step 1: 写会红的测试**（`T-ROOT-10` 的前置）：`.s` 里每个 call 之前
  必须有一条 `str` 到 `[x29, #-32]`；数量 == `.stackmap` 里该函数的 `sp` 行数。
  （用文本断言，沿 `stackmap.rs` 的口径。）
- [ ] **Step 2: 实现**：`module.rs` 只算一次 `lv`；`stack_map_of` 与 `isel` 都用它。
- [ ] **Step 3: Commit** `feat(arm64): Liveness 前移与 sp_index（Task 20）`

### Task 21: 帧记录的空间

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/frame.rs`

**Interfaces:**
- Produces: `pub const REC_SIZE: u32 = 24;` `pub const REC_OFF: i32 = -16;`
  （`prev` 在 `-16`、`fi` 在 `-24`、`sp` 在 `-32`）

- [ ] **Step 1: 写会红的测试**：`native_frame_map` 必须仍然绿（偏移整体下移，
  但 `.framemap` 与 objdump 两侧同源）。
- [ ] **Step 2: 实现**：值槽从 `-40` 起；`frame_size` 加 24 并向上取 16 的倍数。
- [ ] **Step 3: Commit** `feat(arm64): 帧顶预留 24 字节记录（Task 21）`

### Task 22: 帧记录链

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/isel.rs`（`prologue` / `epilogue`）
- Modify: `runtime/native/.../rt.c`（`g_frames` + `rt_init` 清零）

- [ ] **Step 1: 写会红的测试**（`T-ROOT-09`）：

```go
package main

func churn() {
	for i := 0; i < 20000; i++ {
		_ = new([64]byte)
	}
}

func main() {
	p := new(int)
	*p = 7
	churn()
	println(*p)
}
```
`RGOC_GC_STRESS=1` ⇒ `7`

- [ ] **Step 2: 实现**：只有**含 safepoint** 的函数发记录（`lv.safepoints.is_empty()` 判）；
  prologue 三条 store，epilogue 一条恢复。
- [ ] **Step 3: Commit** `feat(arm64): 帧记录链（Task 22，T-ROOT-09）`

### Task 23: 扫帧链

**Files:**
- Modify: `runtime/native/.../rt.c`（`mark_roots` 加帧链）

- [ ] **Step 1**: 逐帧：`fi`/`sp_idx` → 位图 → `mark_ptr`；`bad_sp` 自检。
- [ ] **Step 2**: `T-ROOT-10`（逐位点）与 `T-ROOT-11`（三层帧）两条判据。
- [ ] **Step 3: Commit** `feat(rt): 扫帧链（Task 23，T-ROOT-10/11）`

### Task 24: 记录根：defer / panic

**Files:**
- Modify: `runtime/native/.../rt.c`（`DeferRec` 改托管块或登记；panic 状态登记）

- [ ] **Step 1: 写会红的测试**（`T-ROOT-13` / `T-ROOT-14`）。
- [ ] **Step 2: 实现**：defer 链的 `fv` 作为根（`closure` 块或 rodata funcval）；
  panic 状态里的值（接口两个字）作为根。
- [ ] **Step 3: Commit** `feat(rt): defer 与 panic 的根（Task 24，T-ROOT-13/14）`

### Task 25: map 的托管化

**Files:**
- Modify: `runtime/native/.../rt.c`（`MapObj` 静态 desc、`mapslots` 块、`map_grow`）

- [ ] **Step 1: 写会红的测试**（`T-ROOT-15`）：`map[string]*[64]byte` 的值活过 GC；
  另加「循环里 make map 1000 次，`live` 不涨」。
- [ ] **Step 2: 实现**：`rt_desc_mapobj`（`{slots,kdesc,vdesc}` 三个指针字）+
  `mapslots` 块扫 LIVE 槽。
- [ ] **Step 3: Commit** `feat(rt): map 托管化（Task 25，T-ROOT-15）`

### Task 26: 闭包与数组

**Files:**
- Modify: `runtime/native/.../rt.c`（`closure` / `array` kind 的扫描）

- [ ] **Step 1: 写会红的测试**（`T-ROOT-16` / `T-ROOT-17`）。
- [ ] **Step 2: 实现**：`closure` 扫 n 个字；`array` 扫 n 个元素按 elem 的 `gcdata`。
- [ ] **Step 3: Commit** `feat(rt): 闭包与数组的根（Task 26，T-ROOT-16/17）`

### Task 27: 收窄 `rt_arena_alloc`

**Files:**
- Modify: `runtime/native/.../rt.c`（剩下的调用点逐个核）
- Test: `rgoc/crates/rgoc-driver/tests/gc_arena.rs`（源码扫描，沿 `T-SCH-04`）

- [ ] **Step 1**: `C12` §6 列出「还用 arena 且不含堆指针」的结构（itab / 超大帧 /
  chunk 元数据 / worklist …）；测试扫源码，出现未登记的调用点就红（`T-GC-11`）。
- [ ] **Step 2: Commit** `test(m10): arena 保留面守卫（Task 27，T-GC-11）`

### Task 28: 精确性

**Files:**
- Create: `rgoc/crates/rgoc-driver/tests/gc_precise.rs`

- [ ] **Step 1: 写 `T-PREC-01`**：

```go
package main

import "unsafe"

func churn() {
	for i := 0; i < 20000; i++ {
		_ = new([64]byte)
	}
}

func main() {
	p := new(int)
	*p = 42
	u := uintptr(unsafe.Pointer(p)) + 8 // 像地址、但不是块首
	churn()
	println(*p, u != 0)
}
```
`RGOC_GC_STRESS=1` ⇒ `42 true`，且 `bad_ptr == 0`。

- [ ] **Step 2**: 变异：把帧扫描改成「整帧所有字都当根」⇒ 这条必须**崩或错**
  （证明它有牙）。
- [ ] **Step 3: Commit** `test(m10): 精确性（Task 28，T-PREC-01）`

### Task 29: 无寄存器根守卫

**Files:**
- Modify: `rgoc/crates/rgoc-driver/tests/gc_precise.rs`

- [ ] **Step 1**: 扫 `.s`：函数体里不出现 `stp x19` / `str x19`（没有寄存器分配
  就没有 callee-saved 保存点）；出现就红（`T-GC-10`）。
- [ ] **Step 2: Commit** `test(m10): 无寄存器根守卫（Task 29，T-GC-10）`

### Task 30: 变异六条

- [ ] **Step 1**: `M-M10-01` / `06` / `07` / `08` / `09` / `10` / `11` 七条
  （块头换序 / 帧链断 / `sp_idx` 恒 0 / 位序反 / defer / map / 数组）逐条变红。
- [ ] **Step 2: Commit** `test(m10): 根协议的变异（Task 30）`

### Task 31: `C12` §4 定稿

- [ ] **Step 1**: 帧记录 ABI（三个偏移 + 谁发记录）写进契约；与 `C5` §7 的交叉引用。
- [ ] **Step 2: Commit** `docs(m10): C12 §4 定稿（Task 31）`

### Task 32: Phase 2 收尾

- [ ] **Step 1**: 三态语料 —— **此时 `on` 仍等价于 `off`**（`gc_auto` 还没开），
  但 `stress` 必须 69/0/0（这是根扫全了的第一次全量检验）。
- [ ] **Step 2: Commit** `chore(m10): Phase 2 收尾（stress 69/0/0）`

---

# Phase 3 · M10c：自动触发、统计、长跑

### Task 33: 自动触发

**Files:**
- Modify: `runtime/native/.../rt.c`（`gc_auto = 1`）

- [ ] **Step 1: 写会红的测试**（`T-GC-01`）：`RGOC_GC_THRESHOLD=65536` + 1 MiB 分配
  ⇒ `collections >= 1`。
- [ ] **Step 2: 实现**：分配失败且 `heap_committed > threshold` ⇒ `rt_gc()` 后重试一次。
- [ ] **Step 3: 三态语料**（`T-CORP-05`）：off / on / stress 各 69/0/0。
- [ ] **Step 4: Commit** `feat(rt): 自动触发（Task 33，T-GC-01、T-CORP-05）`

### Task 34: 长跑与碎片

**Files:**
- Create: `rgoc/crates/rgoc-driver/tests/gc_longrun.rs`

- [ ] **Step 1**: `T-GC-05`（1e5 × 1 KiB ⇒ `live <= 2 MiB`、`heap <= 64 MiB`）。
- [ ] **Step 2**: `T-GC-06`（碎片只测不修：记录 `heap_committed` 与理论最小值的比）。
- [ ] **Step 3**: `T-GC-09`（RSS 记录：`getrusage` 的 `ru_maxrss` 打进统计行）。
- [ ] **Step 4: Commit** `test(m10): 长跑与碎片（Task 34，T-GC-05/06/09）`

### Task 35: OOM 与确定性

- [ ] **Step 1**: `T-GC-07`：`RGOC_HEAP_MAX=4194304` + 长跑 ⇒ 文本与退出码不变。
- [ ] **Step 2**: `T-GC-08`：同程序两次跑，`collections/live/heap` 三个数完全相同。
- [ ] **Step 3: Commit** `test(m10): OOM 与确定性（Task 35，T-GC-07/08）`

### Task 36: 变异两条

- [ ] **Step 1**: `M-M10-12`（复用不清零）⇒ `T-GC-04` 红；
  另一条：阈值恒 0（每次分配都收，自动态下行为仍须正确 —— 这条**应该绿**，
  它是「压力与自动等价」的证据，写进 notes 而不是脚本）。
- [ ] **Step 2: Commit** `test(m10): Phase 3 变异（Task 36）`

### Task 37: 全量回归

- [ ] **Step 1**: 四条 cargo；四套语料（native 三态 + `--run` + `--via=ssa` + chan）。
- [ ] **Step 2: Commit** `chore(m10): Phase 3 收尾`

---

# Phase 4 · 收口

### Task 38: 契约终稿

- [ ] **Step 1**: `C12` 全节定稿；`C5` §11.1 与 §7 的交叉引用补齐；`C9` 统计行。
- [ ] **Step 2: Commit** `docs(m10): 契约终稿（Task 38）`

### Task 39: `M10-manifest.json`

- [ ] **Step 1**: gate `GC-1..GC-9`、`unsupported` 8 条、`corpus_lists`、
  `measured`（含 `live_bytes` / `heap_committed` / RSS）、`open_gaps`、`closed_gaps`。
- [ ] **Step 2: Commit** `docs(m10): manifest（Task 39）`

### Task 40: `manifest_m10.rs`

- [ ] **Step 1**: 沿 `manifest_m9.rs` 的六类谎 + 一条 M10 特有：
  **`measured.live_bytes` 必须能从一次真实长跑里复现**（跑 fixture 取统计行，
  与自报数字比）。
- [ ] **Step 2**: 14 条合成反例，每条要求指名道姓的错误。
- [ ] **Step 3: Commit** `test(m10): manifest 对账（Task 40）`

### Task 41: 变异脚本与笔记

- [ ] **Step 1**: `mutation-m10.sh` 收齐 12 条里回归代价低的 8 条；
  `mutation-m10-notes.md` 记全部 12 条（含「绿的那条为什么绿」）。
- [ ] **Step 2: Commit** `test(m10): 变异脚本与笔记（Task 41）`

### Task 42: `M10-report.md`

- [ ] **Step 1**: 读数 / 交付 / 决策复核 / 真错清单 / 白名单 / 边界 / 冻结 / 接手。
  **必须写清**：`T-PREC-01` 是「精确」二字的唯一强证据；碎片只测不修。
- [ ] **Step 2: Commit** `docs(m10): 报告（Task 42）`

### Task 43: `GAP-01` M10 节

- [ ] **Step 1**: `RT-MEM-04/06/07/08` 逐条 ✅/◐/✗ + 口径说明；
  `RT-MEM-03/05` 记「归 M11」。
- [ ] **Step 2: Commit** `docs(m10): GAP-01 的 M10 节（Task 43）`

### Task 44: 文档回写

- [ ] **Step 1**: `README`（M10 五行 + C12）、`AGENTS`（当前阶段 / 一句话状态 /
  M10 段）、`03-roadmap`（M10 状态块）。
- [ ] **Step 2: Commit** `docs(m10): 文档回写（Task 44）`

### Task 45: 收尾

- [ ] **Step 1**: 四条 cargo + 四套语料 + 三态语料；台账与记忆。
- [ ] **Step 2: Commit + push**

---

## 任务 ↔ 门禁对照

| 门禁 | 判据 | Task |
|---|---|---|
| `GC-1` | `T-HDR-01/02/03` | 8、9（`T-HDR-01` 已由 `R-10-1` 完成）|
| `GC-2` | `T-HEAP-01..05` | 6、7、10、14 |
| `GC-3` | `T-ROOT-09..17` | 22–26 |
| `GC-4` | `T-PREC-01/02/03` | 15、28 |
| `GC-5` | `T-GC-01/02/03/08` | 3、33、35 |
| `GC-6` | `T-CORP-05/06/07` | 5、33 |
| `GC-7` | `T-GC-05/06/07/09` | 34、35 |
| `GC-8` | `C12` / `C5` / `C9` | 1、2、18、31、38 |
| `GC-9` | 12 条变异 | 17、30、36、41 |

**Phase 0 的 5 个 Task 让 `GC-8` 前进**；Phase 1 让 `GC-1/GC-2/GC-4` 前进；
Phase 2 让 `GC-3/GC-4` 前进；Phase 3 让 `GC-5/GC-6/GC-7` 前进；Phase 4 收口 `GC-8/GC-9`。
