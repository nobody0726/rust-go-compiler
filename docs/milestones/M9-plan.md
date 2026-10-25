# M9 实施计划：native 对象模型、接口与 metadata

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 native 通路从「只支持顺序 + 基本类型」推到「M10 的 GC 能直接开工」——
对象模型（`TypeDesc` + 分配器）、复合类型（slice/map/iface/funcval）、native 的异常
（defer/panic/recover）、精确到 safepoint 的根协议，并把 native 语料从 **39** 推到 **69**。

**Architecture:** 堆对象统一带 8 字节 `*TypeDesc` 头（`M9-design.md` `D-M9-1`），
desc 是编译期生成的 40 字节静态表（`D-M9-2`）；分配走单 bump arena + mmap 增长（`D-M9-3`）。
SSA 只净增 3 个 `Instr`（`FuncAddr`/`MakeClosure`/`Defer`），map 与 iface 全部 `Call` 到
`rt_*`（`D-M9-5`）；panic 用返回码逐层传播而非 unwinding（`D-M9-12`），因为 `.framemap`
是并列产物、运行期读不到。

**Tech Stack:** Rust 1.98（workspace 11 → 12 个 crate，新增 `rgoc-meta`）；
freestanding C（`rt.c` 815 → ~2200 行，零 libc，`svc #0`）；AArch64 / `clang` 14.0.6；
容器内构建（`RGOC_TTY=0 scripts/in-container.sh`）。

**Spec:** `docs/milestones/M9-design.md`（`D-M9-1..16` / `F1..F16`）。
判据面：`docs/milestones/M9-tests.md`（`T-MEM-*` / `T-META-*` / `T-SLICE-*` / `T-MAP-*` /
`T-IFACE-*` / `T-FUNC-*` / `T-EXC-*` / `T-ROOT-*` / `T-CORP-*` / `T-SCH-*`）。
**执行期两者一起读**：本文档说「做什么、按什么顺序」，`M9-tests.md` 说「怎么算通过」。

## Global Constraints

- **一切构建/测试在容器内**：`RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && …'`
  （`cwd` 必须是 `/work/rgoc`；探不到 daemon 就报错，**不降级**）。
- **四条 cargo 门禁**：`fmt --all -- --check` / `check --workspace --all-targets` /
  `clippy --all-targets -D warnings` / `test --workspace --no-fail-fast` —— 每条 Task 的
  Verify 里至少跑与它相关的那一条，**Phase 收口时四条全跑**。
- **语料门禁 = 69**（`M9-design.md` `D-M9-15`）：`N' + M' == 69` 恒成立；
  **不许**为了让数字好看而改分母。
- **零 libc**：`rt.c` 只用 `svc #0` 直调与自写的 `memcpy/memset`；
  不许 `#include <string.h>` / `<stdlib.h>`（沿 M7 `D-M7-1`）。
- **产物确定性**：同一份源码两次 `rgoc build` 的 `.s` / ELF / `.stackmap` 必须逐字节相同
  （沿 `NAT-1`；新增的 `.stackmap` 同样受约束）。
- **只读 `go_source_code/`**：语料一个字节都不许改。fixture 只写 `target/tmp/` 与
  `rgoc/crates/*/fixtures/`。
- **提交纪律**：每个 Task 一个 commit；Phase 收口额外一个「台账」commit。
- **不许引入外部 crate**：`rgoc-meta` 只用 `rgoc-types` / `rgoc-const` 与 `std`。
- **文档与代码同步**：改了 `TypeDesc` 的字段集 / `STACK-MAP` 的语法 / `hchan` 的字段序
  ⇒ 同一次 commit 里改 `C5` / `C11`，并按 `M9-tests.md` §9 记 `R-9-y`。

## Review Focus

> 以下是 spec 隐含、但**没有哪条 Task 的测试会自然覆盖**的五类输入 —— 最可能伤到使用者，
> 按可能性排序。每一类已经在「拥有该代码的 Task」里加了判据（列在后面）。

1. **零大小类型**（`struct{}` / `[0]byte`）作为 slice 元素或 map 值：分配 0 字节、
   指针不前进、`len` 仍要算。语料里有 `zerosize.go`，但它**不打印**（空输出），
   所以「跑完」不等于「对」。→ **Task 11**（`T-SLICE-01` 的 `make([]struct{}, 3)` 那段）。
2. **空 struct 的相等与作键**：所有 `struct{}` 值相等，哈希必须相同、`==` 必须真。
   → **Task 18**（`rt_hash`/`rt_equal` 的 `size == 0 ⇒ hash 恒 0、equal 恒真`）。
3. **闭包捕获多字值**（`string` 两字 / `iface` 两字）：只搬第一个字是 M7 「`string` 丢 len」
   的同款错，而症状是「打印出来的字符串少一半」。→ **Task 38**（捕获 `int + string + *int`
   的 fixture 里 `len(s)` 必须对）。
4. **defer 里再 panic / recover 之后再 panic**：`rt_panic` 的状态是全局的，
   重入时旧状态必须先处理。→ **Task 48**（重入覆盖）与 **Task 51**（recover 之后再 panic）。
5. **`map[any]int`**（接口作键）：键相等要走 `rt_equal` 的 iface 分支（比 `(tab,data)`），
   而 `data` 是 boxed 指针 —— 两个内容相同的 `any` 的 `data` 不同，必须按 `rt_equal` 比较。
   → **Task 36**（`T-IFACE-09` 在 `OBJ-3` 收口里，依赖 Task 29 的 `rt_box` 已完成）。

---

# Phase 0 · M9a 骨架（分配器 + `TypeDesc` + ptrmask）

> **本 Phase 不涨语料数字**（39 → 39）。它的价值是：后面每一段都依赖
> `TypeDesc` / `ptr_mask` / `rt_alloc`，**绕不过**。
>
> ⚠️ **执行期调整（`R-9-1`）**：原计划的 Task 5/6/7 顺序是
> `rt_alloc` → chunk 增长 → desc 发射。但 `rt_alloc` 的判据必须让 `new(T)` 真的
> 走它，而 `new(T)` 要传 desc —— desc 还没发射时那条判据会**假绿**
> （`new` 仍在调 `rt_arena_alloc`，测试照样过）。所以顺序改成
> **desc 发射 → rt_alloc → chunk 增长**。内容一字未改，只是编号换了位。

### Task 1: `rgoc-meta` crate 与 `TypeDesc`

**Files:**
- Create: `rgoc/crates/rgoc-meta/Cargo.toml`、`rgoc/crates/rgoc-meta/src/lib.rs`、`src/desc.rs`
- Modify: `rgoc/Cargo.toml`（workspace members）
- Test: `rgoc/crates/rgoc-meta/src/desc.rs`（`#[cfg(test)] mod tests`）

**Interfaces:**
- Produces: `TypeDesc { size: u32, ptrdata: u32, hash: u32, kind: u8, flags: u8, gcdata: *const u8, elem: *const TypeDesc, methods: *const MethodTable }`；
  `DescBuilder::new(tt: &TypeTable, t: TypeId) -> Option<Self>`；`DescBuilder::build(self) -> TypeDesc`
- Consumes: `rgoc_types::{layout, TypeTable, TypeId}`

- [x] **Step 1: 写会红的测试**（`T-META-01`）
- [x] **Step 2: 跑它确认红**（编译失败：`TypeDesc` 不存在）
- [x] **Step 3: 建 crate 与结构体**（`#[repr(C)]` 必须；`DescBuilder` 先只填 `size`）
- [x] **Step 4: 跑它确认绿**
- [x] **Step 5: Commit** `feat(meta): rgoc-meta crate 与 TypeDesc（Task 1）`

### Task 2: `ptr_mask` —— 从类型算指针位图

**Files:**
- Modify: `rgoc/crates/rgoc-types/src/layout.rs`（追加 `pub fn ptr_mask`）
- Test: `rgoc/crates/rgoc-types/tests/ptr_mask.rs`（新建）

**Interfaces:**
- Produces: `pub fn ptr_mask(tt: &TypeTable, t: TypeId) -> Option<Vec<bool>>` —— 每 8 字节一格
- Consumes: `layout()`、`Type::{Pointer, Slice, Basic, Struct, Array, Interface, Signature, Map, Chan}`

- [x] **Step 1: 写会红的测试**（`T-META-04`：五个特例 + 标量/空类型 + 数组 + 嵌套 + 单字引用 + 具名/别名）
- [x] **Step 2: 跑它确认红**
- [x] **Step 3: 实现 `ptr_mask`**（递归四支；`iface` 的 `[true,true]` 里字 0 是元数据指针）
- [x] **Step 4: 跑它确认绿**
- [x] **Step 5: Commit** `feat(types): ptr_mask（Task 2，T-META-04）`

### Task 3: `kind_of` / `needs_zero`

**Files:**
- Modify: `rgoc/crates/rgoc-meta/src/desc.rs`
- Test: 同文件

**Interfaces:**
- Produces: `fn kind_of(tt, t) -> Kind`；`fn needs_zero(tt, t) -> bool`；`DescBuilder` 填完 `kind` 与三个 `flags`
- Consumes: `ptr_mask`、`comparable`

- [x] **Step 1: 写会红的测试**（`T-MEM-04` + kind 与变体一一对应 + 无布局不给假 desc + builder 三 flag）
- [x] **Step 2: 跑它确认红**
- [x] **Step 3: 实现**：`needs_zero = has_pointers || has_float`；浮点判定**递归**
- [x] **Step 4: 跑它确认绿**
- [x] **Step 5: Commit** `feat(meta): kind_of / needs_zero（Task 3，T-MEM-04）`

### Task 4: 类型 hash

**Files:**
- Modify: `rgoc/crates/rgoc-meta/src/desc.rs`
- Test: 同文件

**Interfaces:**
- Produces: `fn type_hash(tt: &TypeTable, t: TypeId) -> u32`
- Consumes: 无（纯函数）

- [x] **Step 1: 写会红的测试**（`T-META-05`：稳定 / 区分 / 结构敏感 / 跨表同值 / 别名透明具名不透明）
- [x] **Step 2: 跑它确认红**
- [x] **Step 3: 实现**：`fnv1a` 过「type_tag + 递归子项」的字节流；**不许**用 `TypeId` 下标
- [x] **Step 4: 跑它确认绿**
- [x] **Step 5: Commit** `feat(meta): type_hash（Task 4，T-META-05）`

### Task 5: desc 表发射进 `.rodata`（原 Task 7）

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/module.rs`（新增 `emit_descs`）、`emit.rs`（`rodata_named_raw`）、
  `rgoc/crates/rgoc-arm64/Cargo.toml`（加 `rgoc-meta`）
- Test: `rgoc/crates/rgoc-driver/tests/desc_emit.rs`（新建）

**Interfaces:**
- Produces: 每个类型一个符号 `__desc_<hash>`；产物里 `.rodata` 的 `TypeDesc` 块（40 B，
  字段序 == `C5` §10）；`gcdata` 位图各一个 `.Lgc<N>` 标签
- Consumes: Task 1–4

- [x] **Step 1: 写会红的测试**（`T-META-02` 的一半：产物里的 `size` / `kind` / `gcdata` 位图）

```rust
#[test]
fn rodata_里的_desc_size_与_layout_一致() {
    let elf = build_and_read("struct S{ a int; b string }");
    assert_eq!(desc_field(&elf, "S", "size"), 24);   // 8 + 16
    assert_eq!(desc_field(&elf, "S", "kind"), 9);    // Kind::Struct
    assert_eq!(desc_bitmap(&elf, "S"), "0b10");      // 字 0 = int 不是根，字 1 = string 的 ptr 是
}
```

- [x] **Step 2: 跑它确认红**
- [x] **Step 3: 实现**：按「类型闭包」去重（`hash` 作键）后逐个发射；
  指针字段用 `.quad __desc_<hash>` 或 `0`（无子类型时）
- [x] **Step 4: 跑它确认绿**
- [x] **Step 5: Commit** `feat(arm64): desc 表进 .rodata（Task 5，T-META-02）`

### Task 6: `rt_alloc` —— 分配、对齐、递增（原 Task 5）

**Files:**
- Modify: `runtime/native/aarch64-unknown-linux-gnu/rt.c`（新增一节）、
  `rgoc/crates/rgoc-arm64/src/isel.rs`（`new_alloc` 改走 `rt_alloc`）
- Test: `rgoc/crates/rgoc-driver/tests/mem_alloc.rs`（新建，走 `rgoc build` + 跑）

**Interfaces:**
- Produces: `void *rt_alloc(const TypeDesc *desc, int need_zero)`；`u64 rt_probe_align(void)`
- Consumes: Task 5

- [x] **Step 1: 写会红的 fixture + 断言**（`T-MEM-01/02/05`）

```go
package main

func main() {
	var ps [100]*int
	for i := range ps {
		ps[i] = new(int)
	}
	*ps[0] = 7
	println(*ps[0], len(ps))
}
```

判据是**三条**：① 跑通并打印 `7 100`；② 产物里 `bl rt_alloc` 出现（不是
`rt_arena_alloc`）—— 否则「new 走了新分配器」是假绿；③ `rt_alloc` 内部对
`(p & 7) != 0` 硬断言（misaligned ⇒ `panic: misaligned allocation` + 退出 2）。

- [x] **Step 2: 跑它确认红**（`rt_alloc` 未定义 ⇒ 链接期 `undefined reference`）
- [x] **Step 3: 实现**：bump + `align_up(8)` + `*p = desc` + `need_zero` 时
  `rt_memset(p+8, 0, desc->size)`；chunk 不够时调 `rt_grow`（Task 7）
- [x] **Step 4: 跑它确认绿**
- [x] **Step 5: Commit** `feat(rt): rt_alloc（Task 6，T-MEM-01/02/05）`

### Task 7: chunk 增长与堆上限（原 Task 6）

**Files:**
- Modify: `runtime/native/.../rt.c`（`rt_arena_alloc` 改成新堆之上的薄壳）
- Test: `rgoc/crates/rgoc-driver/tests/mem_limit.rs`（新建）

**Interfaces:**
- Produces: `void rt_grow(size_t need)`；`size_t rt_heap_used(void)`；`size_t rt_heap_limit(void)`；
  环境变量 `RGOC_HEAP_MAX`（字节，缺省 256 MiB）
- Consumes: Task 6

- [x] **Step 1: 写会红的测试**（`T-MEM-06/07`）：一个吃 ~8 MiB 的程序
  ① 缺省上限下跑通；② `RGOC_HEAP_MAX=1048576` 时退出 **2** + stderr 含
  `runtime error: out of memory (heap limit 1048576 bytes)`
- [x] **Step 2: 跑它确认红**
- [x] **Step 3: 实现**：`mmap` 新 chunk（16 MiB 起，`max(16MiB, need)`），挂链；
  超限走 `rt_fatal_oom(limit)`。**`rt_arena_alloc` 必须改成同一套堆上的薄壳** ——
  否则一个进程里有两个预算，`RGOC_HEAP_MAX` 只管一半
- [x] **Step 4: 跑它确认绿**
- [x] **Step 5: Commit** `feat(rt): chunk 增长与堆上限（Task 7，T-MEM-06/07）`

### Task 8: 全局 roots 表

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/module.rs`（`emit_globals` 扩）
- Test: `rgoc/crates/rgoc-driver/tests/global_roots.rs`（新建，`T-ROOT-07`）

**Interfaces:**
- Produces: 符号 `rt_globals`（`GlobalRoot[]`）与 `rt_nglobals`（`u64`）；
  `GlobalRoot { desc: *TypeDesc, addr: *u8 }`（16 B）
- Consumes: Task 5

- [x] **Step 1: 写会红的测试**：`var a int; var b string; var c *int` ⇒
  `rt_nglobals == 3`，且第 2 条的 `desc` 是 `string` 的（按 `size==16` 认）
- [x] **Step 2: 跑它确认红**
- [x] **Step 3: 实现**：包级变量逐个进表；`desc` 取它的静态类型；零大小全局**不进表**
  （它没有可扫描的字）
- [x] **Step 4: 跑它确认绿**
- [x] **Step 5: Commit** `feat(arm64): 全局 roots 表（Task 8，T-ROOT-07）`

### Task 9: 元数据对撞（69 个语料的类型闭包）

**Files:**
- Test: `rgoc/crates/rgoc-driver/tests/desc_layout.rs`（新建，`T-META-02/03/06`）

**Interfaces:**
- Consumes: Task 1–5

- [x] **Step 1: 写会红的测试**：对 `run-selected.txt` 的每个文件，枚举类型闭包，
  逐类型比 `TypeDesc.size` == `layout().size`、`gcdata` 解出的位图 == `ptr_mask()`。
  失败时**报第一个差异的类型名 + 两侧的值**。
- [x] **Step 2: 跑它确认红**（预期会有差异 —— 这正是它存在的理由）
- [x] **Step 3: 按差异修**（**不许**改测试去迁就）
- [x] **Step 4: 跑它确认绿**
- [x] **Step 5: Commit** `test(meta): 69 语料的元数据对撞（Task 9，T-META-02/03/06）`

### Task 10: `run-native.sh` 切到 69 清单

**Files:**
- Modify: `tools/run-oracle/run-native.sh`（默认清单改 `run-selected.txt`）

- [x] **Step 1: 改默认输入**：`--list` 缺省值 `native-input.txt` → `run-selected.txt`
- [x] **Step 2: 跑** `bash tools/run-oracle/run-native.sh`
  Expected: `ok=39 skip=30 bad=0 / 69`（7 个 slice + 23 个 SSA 拒）
- [x] **Step 3: Commit** `test(native): 语料清单切到 69（Task 10）`

**Verify:** `… bash tools/run-oracle/run-native.sh` ⇒ `ok=39 skip=30 bad=0 / 69`；
`… cargo test -p rgoc-meta -p rgoc-arm64 -p rgoc-driver` ⇒ 全绿。

---

# Phase 1 · slice（39 → **46**）

### Task 11: `rt_makeslice`

**Files:**
- Modify: `runtime/native/.../rt.c`
- Test: `rgoc/crates/rgoc-driver/tests/obj_slice.rs`（新建）

**Interfaces:**
- Produces: `void rt_makeslice(Slice *out, const TypeDesc *elem, size_t len, size_t cap)`
  （`Slice { void *ptr; u64 len; u64 cap; }`）
- Consumes: `rt_alloc`

- [x] **Step 1: 写会红的测试**（`T-SLICE-01/02` + Review Focus ①）

```go
package main

func main() {
	a := make([]int, 3)
	b := make([]int, 1, 5)
	var c []int
	println(len(a), cap(a), len(b), cap(b), c == nil)
	d := make([]struct{}, 3)      // Review Focus ①：零大小元素
	println(len(d), cap(d))
}
```
Expected: `3 3 1 5 true\n3 3\n`

**追加一条断言**（spec §3.4 第 3 条「新对象一律 arena」的唯一可观测面）：
`make([]int, 3)` 之后 `rt_heap_used()` 必须**增长** —— 即使这个 slice 没逃出去。
（不做逃逸优化是**契约**，不是疏忽；这条断言把「不做」钉成判据。）

- [x] **Step 2: 跑它确认红**

- [x] **Step 3: 实现**：`ptr = rt_alloc` 按 `len==0 && cap==0` 判 `nil`（`ptr=NULL`）；
  **零大小元素**：`elem->size == 0` ⇒ 不分配（`ptr = &zerobase`），`len/cap` 照记。

- [x] **Step 4: 跑它确认绿**

- [x] **Step 5: Commit** `feat(rt): rt_makeslice（Task 11，T-SLICE-01/02）`

### Task 12: `rt_growslice` 与 `append`

**Files:**
- Modify: `runtime/native/.../rt.c`、`rgoc/crates/rgoc-arm64/src/isel.rs`（`append` 内建）
- Test: 同 Task 11 的文件

**Interfaces:**
- Produces: `void rt_growslice(Slice *out, Slice old, const TypeDesc *elem, size_t extra)`
- Consumes: Task 11

- [x] **Step 1: 写会红的测试**（`T-SLICE-05/06`）

```go
func main() {
	s := make([]int, 2, 4)
	old := s
	s = append(s, 7)          // cap 够 ⇒ ptr 不变
	println(s[2], old[0])
	t := make([]int, 2, 2)
	u := t
	t = append(t, 9)          // 扩容 ⇒ 新底层数组，u 不受影响
	println(t[2], len(u))
}
```
Expected: `7 0\n9 2\n`

- [x] **Step 2: 跑它确认红**

- [x] **Step 3: 实现**：cap 够 ⇒ 原地写 + `len+1`；不够 ⇒ 新分配
  （`max(2*cap, len+extra)`）+ `memcpy(len*elem->size)` + 写新元素。

- [x] **Step 4: 跑它确认绿**

- [x] **Step 5: Commit** `feat(rt): rt_growslice 与 append（Task 12，T-SLICE-05/06）`

### Task 13: isel 的 slice 三处（`Make` / `Index` / `Slice`）

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/isel.rs`（三个 match 分支）
- Test: 同文件 + Task 11 的 fixture 扩

**Interfaces:**
- Produces: `Make`（slice kind）⇒ 调 `rt_makeslice` 到 3 字结果槽；
  `Index` 对 slice ⇒ 取字 0 作基址；`Slice` ⇒ 算新 `ptr/len/cap`
- Consumes: Task 11–12

- [x] **Step 1: 写会红的测试**（`T-SLICE-03/04`）

```go
func main() {
	s := []int{1, 2, 3, 4, 5}
	t := s[1:3]
	println(len(t), cap(t), t[0])
	u := s[1:3:4]
	println(cap(u))
	t[0] = 9
	println(s[1])
}
```
Expected: `2 4 2\n3\n9\n`

- [x] **Step 2: 跑它确认红**

- [x] **Step 3: 实现**：`Slice` 的三下标 `cap = max-lo`（无 `max` 时 `cap = cap(s)-lo`）。
  同时补 `string(r)` 的 isel：走 `rt_rune2utf8(r) -> [4]byte + len`（spec §7 `RT-CMP-02`
  点名要补的边界；判据加在同一个 fixture：`println(string(rune(0x4e2d)))` ⇒ `中`）

- [x] **Step 4: 跑它确认绿**

- [x] **Step 5: Commit** `feat(arm64): slice 的 Make/Index/Slice（Task 13，T-SLICE-03/04）`

### Task 14: 越界检查与 `rt_panic_bounds`

**Files:**
- Modify: `runtime/native/.../rt.c`、`rgoc/crates/rgoc-arm64/src/isel.rs`
- Test: Task 11 的文件

**Interfaces:**
- Produces: `void rt_panic_bounds(u64 i, u64 n)`（文本与解释器一致）
- Consumes: `D-M9-12` 的 panic 机制（Phase 5 才有；**本 Task 先直接 `exit(2)` 打文本**，
  Phase 5 再接上返回码传播 —— 那是唯一的「临时形态」，Task 47 会改它）

- [x] **Step 1: 写会红的测试**（`T-SLICE-07/08`）

```go
func main() { s := []int{1,2,3}; i := 3; println(s[i]) }
```
Expected: 退出 2 + `panic: runtime error: index out of range [3] with length 3`

- [x] **Step 2: 跑它确认红**

- [x] **Step 3: 实现**：`Index` 与 `Slice` 的两处比较 + 分支；文本逐字节照 `C9` §4.5。

- [x] **Step 4: 跑它确认绿**

- [x] **Step 5: Commit** `feat(rt): 越界检查（Task 14，T-SLICE-07/08）`

### Task 15: 拆掉 `CG-UNSUPPORTED-SLICE`

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/diag.rs`（删变体）、`isel.rs`（删拒绝分支）、
  `rgoc/crates/rgoc-driver/tests/native_slice.rs`（守卫）
- Test: `native_slice.rs`

**Interfaces:**
- Consumes: Task 11–14
- Produces: `CgCode::ALL` 从 3 条降到 2 条；**守卫**：源码里扫不到 `CG-UNSUPPORTED-SLICE`

- [x] **Step 1: 写会红的守卫**（沿 M8 的「五条码不许再出现」做法）

- [x] **Step 2: 跑它确认红**（现在还能扫到）

- [x] **Step 3: 删码 + 删拒绝分支 + 清 `native-rejected.txt` 的 7 行**

- [x] **Step 4: 跑它确认绿** + `cargo test -p rgoc-arm64 -p rgoc-driver --test native_slice`

- [x] **Step 5: Commit** `feat(arm64): 下线 CG-UNSUPPORTED-SLICE（Task 15）`

### Task 16: 语料 46

**Files:**
- Test: `rgoc/crates/rgoc-driver/tests/native_corpus_run.rs`（扩到 69 清单，本 Phase 预期 46）

- [x] **Step 1: 改 `native_corpus_run` 读 `run-selected.txt`**，逐文件断言
  「`ok` 的 stdout == 官方 `.exp`」；`skip` 的必须有码。
- [x] **Step 2: 跑** Expected: `ok=46 skip=23 bad=0`
- [x] **Step 3: 逐文件确认 39 个的字节没变**（`T-CORP-03`）
- [x] **Step 4: Commit** `test(native): 语料 46（Task 16，T-CORP-01/03）`

### Task 17: `D` 腿（native vs 解释器）

**Files:**
- Test: `rgoc/crates/rgoc-driver/tests/native_vs_interp.rs`（新建）

**Interfaces:**
- Consumes: `rgoc_driver::run_cmd::run_run`（解释器）与 `build_cmd`（native）

- [x] **Step 1: 写会红的测试**：对 46 个可跑文件，`native` 的合并流 == `rgoc run` 的；
  不等时报「第一个不同的行号 + 两侧内容」。
- [x] **Step 2: 跑它确认红**（预期会有差异 —— 这是 `D` 腿第一天就该做的事）
- [x] **Step 3: 按差异修**（不要改测试）
- [x] **Step 4: 跑它确认绿**
- [x] **Step 5: Commit** `test(native): D 腿 = native vs 解释器（Task 17）`

**Verify:** `… bash tools/run-oracle/run-native.sh` ⇒ `ok=46 skip=23 bad=0 / 69`；
`… cargo test --workspace --no-fail-fast` ⇒ 0 failed。

---

# Phase 2 · map（46 → **51**）

### Task 18: `rt_hash` 与 `rt_equal`

**Files:**
- Modify: `runtime/native/.../rt.c`
- Test: `rgoc/crates/rgoc-driver/tests/obj_map.rs`（新建）

**Interfaces:**
- Produces: `u32 rt_hash(const TypeDesc *d, const void *p)`；`int rt_equal(const TypeDesc *d, const void *a, const void *b)`
- Consumes: `TypeDesc`（Task 1）

- [ ] **Step 1: 写会红的测试**（`T-MAP-06/07` + Review Focus ②）

```go
type P struct{ a, b int }
func main() {
	m := map[P]int{{1, 2}: 10, {2, 1}: 20}
	println(m[P{1, 2}], m[P{2, 1}], len(m))   // 字段序不同的键必须不同
	e := map[struct{}]int{{}: 7}
	println(e[struct{}{}])                     // 所有空 struct 相等
	println(map[string]int{"ab": 1}["ab"])
}
```
Expected: `10 20 2\n7\n1\n`

- [ ] **Step 2: 跑它确认红**

- [ ] **Step 3: 实现**：`rt_hash` 按 `kind` 分派（标量按宽度、string 走字节、
  指针按值、struct 逐字段、array 逐元素、iface 见 Task 29 后补）；
  `rt_equal` 同形。**空 struct**：`size == 0` ⇒ hash 恒 0、equal 恒真。

- [ ] **Step 4: 跑它确认绿**

- [ ] **Step 5: Commit** `feat(rt): rt_hash / rt_equal（Task 18，T-MAP-06/07）`

### Task 19: `rt_map_make` 与 nil map

**Files:**
- Modify: `runtime/native/.../rt.c`
- Test: 同 Task 18

**Interfaces:**
- Produces: `void *rt_map_make(const TypeDesc *kdesc, const TypeDesc *vdesc)`；nil map 是 `NULL`
- Consumes: `rt_alloc`、Task 18

- [ ] **Step 1: 写会红的测试**（`T-MAP-01`）

```go
func main() {
	var m map[int]int
	println(m[1], len(m))     // nil map 读零值，不 panic
	n := make(map[int]int)
	println(len(n))
	n[1] = 2
	println(n[1])
}
```
Expected: `0 0\n0\n2\n`

- [ ] **Step 2: 跑它确认红**

- [ ] **Step 3: 实现**：`MapObj` 按设计 §3.7 的 40 B 头 + `slots` 数组（初始 cap 8）

- [ ] **Step 4: 跑它确认绿**

- [ ] **Step 5: Commit** `feat(rt): rt_map_make（Task 19，T-MAP-01）`

### Task 20: `rt_map_get`（sret + comma-ok）

**Files:**
- Modify: `runtime/native/.../rt.c`、`rgoc/crates/rgoc-arm64/src/isel.rs`（内建映射）
- Test: 同 Task 18

**Interfaces:**
- Produces: `int rt_map_get(const void *m, const void *k, void *out)` —— 返回 `ok`，值写 `*out`
- Consumes: Task 19

- [ ] **Step 1: 写会红的测试**（`T-MAP-02/03`）

```go
func main() {
	m := map[string]int{"a": 1}
	println(m["a"], m["b"])
	v, ok := m["a"]
	println(v, ok)
	v, ok = m["b"]
	println(v, ok)
}
```
Expected: `1 0\n1 true\n0 false\n`

- [ ] **Step 2: 跑它确认红**

- [ ] **Step 3: 实现**：线性探测，`state == live && rt_equal` 命中；
  `tomb` 不终止探测（**必须继续**，否则删过的键会「消失」）。

- [ ] **Step 4: 跑它确认绿**

- [ ] **Step 5: Commit** `feat(rt): rt_map_get（Task 20，T-MAP-02/03）`

### Task 21: `rt_map_set` / `rt_map_del`

**Files:**
- Modify: `runtime/native/.../rt.c`
- Test: 同 Task 18

**Interfaces:**
- Produces: `void rt_map_set(void *m, const void *k, const void *v)`；`void rt_map_del(void *m, const void *k)`
- Consumes: Task 20

- [ ] **Step 1: 写会红的测试**（`T-MAP-04`）

```go
func main() {
	m := map[int]int{}
	for i := 0; i < 10; i++ { m[i] = i * i }
	delete(m, 3)
	println(len(m), m[3])
	delete(m, 99)          // 删不存在的键：不 panic
	println(len(m))
}
```
Expected: `9 0\n9\n`

- [ ] **Step 2: 跑它确认红**

- [ ] **Step 3: 实现**：`set` 找到 `live` ⇒ 覆盖；`tomb` 或 `empty` ⇒ 写入并 `count++`
  （若是 `tomb` 复用则不加 count）；`del` ⇒ 置 `tomb` + `count--`。
  nil map 上的 `set` ⇒ `rt_panic_nilmap`（Phase 5 前先直接打文本退 2）。

- [ ] **Step 4: 跑它确认绿**

- [ ] **Step 5: Commit** `feat(rt): rt_map_set / rt_map_del（Task 21，T-MAP-04）`

### Task 22: rehash

**Files:**
- Modify: `runtime/native/.../rt.c`
- Test: 同 Task 18

**Interfaces:**
- Produces: `fn rt_map_grow(m: *MapObj)`（内部）
- Consumes: Task 21

- [ ] **Step 1: 写会红的测试**（`T-MAP-05`）

```go
func main() {
	m := map[int]int{}
	for i := 0; i < 100; i++ { m[i] = i }
	sum := 0
	for i := 0; i < 100; i++ { sum += m[i] }
	println(len(m), sum)
}
```
Expected: `100 4950\n`

- [ ] **Step 2: 跑它确认红**

- [ ] **Step 3: 实现**：`count*4 >= cap*3` ⇒ cap 翻倍 + 全量重插（跳过 `tomb`）。

- [ ] **Step 4: 跑它确认绿**

- [ ] **Step 5: Commit** `feat(rt): map rehash（Task 22，T-MAP-05）`

### Task 23: `rt_map_iter`

**Files:**
- Modify: `runtime/native/.../rt.c`、`rgoc/crates/rgoc-arm64/src/isel.rs`（`range` 对 map）
- Test: 同 Task 18

**Interfaces:**
- Produces: `int rt_map_iter(void *it)` —— `it` 是 `{m: *MapObj, idx: u32}`；
  返回 `ok`，键/值写进 `it` 之后的两个槽（布局由 isel 定，见 `C5` §10）
- Consumes: Task 21

- [ ] **Step 1: 写会红的测试**（`T-MAP-08/09`）

```go
func main() {
	m := map[int]int{0: 0, 1: 1, 2: 2, 3: 3}
	n, sum := 0, 0
	for k, v := range m {
		n++
		sum += k + v
		if k == 1 { delete(m, 2) }   // 删一个可能还没访问的
	}
	println(n, sum)
	println(len(m))
}
```
断言：`len(m) == 3`；`n` 是 **3 或 4**（删 2 之前是否已访问过不可知）；
**`sum` 不含 `2+2`** 时 `n==3`（用「`sum` 与 `n` 自洽」的断言，而不是固定数字）。

- [ ] **Step 2: 跑它确认红**

- [ ] **Step 3: 实现**：按 `idx` 递增扫 `slots`，`state == live` 才产出。

- [ ] **Step 4: 跑它确认绿**

- [ ] **Step 5: Commit** `feat(rt): rt_map_iter（Task 23，T-MAP-08/09）`

### Task 24: 内建映射（`MapGet`/`MapSet`/`MapDel`/`MapLen`/`MapRange`）

**Files:**
- Modify: `rgoc/crates/rgoc-ssa/src/build.rs`（拆 `SSA-UNSUPPORTED-MAP`）、
  `rgoc/crates/rgoc-arm64/src/isel.rs`（内建 → 调用序列）
- Test: `rgoc/crates/rgoc-driver/tests/obj_map.rs`

**Interfaces:**
- Produces: `BuiltinKind::{MapLen, MapDel}`（若还没有）；`Index`/`Assign` 的 map 分支落成
  `Call(rt_map_get/rt_map_set)`；`range` 的 map 分支落成 `rt_map_iter` 的循环
- Consumes: Task 18–23

- [ ] **Step 1: 写会红的测试**：`map[string]int` 的 `m[k]`、`m[k]=v`、`len(m)`、`delete`、`range`
  五件事各一条，全部走 `rgoc build` 端到端。
- [ ] **Step 2: 跑它确认红**（现在是 `SSA-UNSUPPORTED-MAP`）
- [ ] **Step 3: 实现** + 删拒绝面 + 守卫（源码扫不到 `SsaCode::Map`）
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(ssa): map 建图与 isel（Task 24）`

### Task 25: 语料 51

- [ ] **Step 1: 跑** `bash tools/run-oracle/run-native.sh`
  Expected: `ok=51 skip=18 bad=0 / 69`
- [ ] **Step 2: 逐文件对 `D` 腿**（`native_vs_interp`）
- [ ] **Step 3: 撞别的墙的记 `M'`**（写进 `native-rejected.txt` 三栏）
- [ ] **Step 4: Commit** `test(native): 语料 51（Task 25，T-CORP-01）`

### Task 26: `T-MAP-*` 收口与 `OBJ-2`

- [ ] **Step 1: 逐条核 `T-MAP-01..09`** 都有对应测试
- [ ] **Step 2: 跑 `cargo test -p rgoc-driver --test obj_map`** ⇒ 全绿
- [ ] **Step 3: Commit** `test(obj): OBJ-2 收口（Task 26）`

**Verify:** `… bash tools/run-oracle/run-native.sh` ⇒ `ok=51 skip=18 bad=0 / 69`；
`… cargo test -p rgoc-driver --test obj_map --test native_vs_interp` ⇒ 全绿。

---

# Phase 3 · iface（51 → **60**）

### Task 27: `MethodTable` 生成

**Files:**
- Modify: `rgoc/crates/rgoc-meta/src/desc.rs`（新增 `MethodTable` / `MethodEntry`）
- Test: `rgoc/crates/rgoc-meta/src/desc.rs`

**Interfaces:**
- Produces: `MethodTable { count: u32, pad: u32, entries: [MethodEntry] }`；
  `MethodEntry { name_hash: u32, sig_hash: u32, fn: *const u8 }`（16 B）
- Consumes: `TypeDesc`（Task 1）

- [ ] **Step 1: 写会红的测试**（`T-META-01` 的邻居 + 名字哈希的区分度）

```rust
#[test]
fn 方法表的条目与签名_hash() {
    assert_eq!(core::mem::size_of::<MethodEntry>(), 16);
    assert_eq!(core::mem::size_of::<MethodTable>(), 8);   // count+pad+entries 紧跟其后
    assert_ne!(name_hash("M"), name_hash("N"));
}
```

- [ ] **Step 2: 跑它确认红**

- [ ] **Step 3: 实现**：名字哈希与签名哈希都用 Task 4 的 fnv1a；
  `entries` 紧跟表头（变长）。

- [ ] **Step 4: 跑它确认绿**

- [ ] **Step 5: Commit** `feat(meta): MethodTable（Task 27）`

### Task 28: 静态 ITab（按**声明序**）

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/module.rs`（`emit_itabs`）
- Test: `rgoc/crates/rgoc-driver/tests/obj_iface.rs`（新建）

**Interfaces:**
- Produces: 符号 `__itab_<iface_hash>_<concrete_hash>`；
  `ITab { iface: *TypeDesc, concrete: *TypeDesc, fn: [*const u8] }`
- Consumes: Task 27

- [ ] **Step 1: 写会红的测试**（`T-IFACE-02`）

```go
type I interface { A(); B() }
type T struct{ n int }
func (t T) A() { println("A", t.n) }
func (t T) B() { println("B", t.n) }
func main() { var i I = T{7}; i.A(); i.B() }
```
Expected: `A 7\nB 7\n` —— **顺序错了（字典序 B,A）这里就会打印反**。

- [ ] **Step 2: 跑它确认红**

- [ ] **Step 3: 实现**：按 `I` 的方法**声明序**填 `fn[]`；用 `MethodTable` 的
  `name_hash` + `sig_hash` 在 `T` 的表里查。

- [ ] **Step 4: 跑它确认绿**

- [ ] **Step 5: Commit** `feat(arm64): 静态 ITab（Task 28，T-IFACE-02）`

### Task 29: `rt_box`

**Files:**
- Modify: `runtime/native/.../rt.c`
- Test: 同 Task 28

**Interfaces:**
- Produces: `void *rt_box(const TypeDesc *d, const void *src)` —— 分配 + `memcpy`
- Consumes: `rt_alloc`

- [ ] **Step 1: 写会红的测试**（`T-IFACE-01`）

```go
func main() {
	var a any = 42
	var b any = "hi"
	var c any = 1.5
	println(a.(int), b.(string), c.(float64))
}
```
Expected: `42 hi 1.5\n`

- [ ] **Step 2: 跑它确认红**

- [ ] **Step 3: 实现**：`size == 0`（空 struct）⇒ 返回 `&zerobase` 不分配。

- [ ] **Step 4: 跑它确认绿**

- [ ] **Step 5: Commit** `feat(rt): rt_box（Task 29，T-IFACE-01）`

### Task 30: `rt_itab`（运行期兜底）

**Files:**
- Modify: `runtime/native/.../rt.c`
- Test: 同 Task 28

**Interfaces:**
- Produces: `void *rt_itab(const TypeDesc *iface, const TypeDesc *concrete)` —— 现算、**不缓存**
- Consumes: Task 27–29

- [ ] **Step 1: 写会红的测试**（`T-IFACE-06`）

```go
type J interface{ A() }
type I interface{ A(); B() }
type T struct{}
func (T) A() {}
func (T) B() {}
func main() {
	var i I = T{}
	j := i.(J)        // (J, T) 对在编译期枚举不出来 ⇒ 必须走 rt_itab
	j.A()
	println("ok")
}
```
Expected: `ok\n`

- [ ] **Step 2: 跑它确认红**

- [ ] **Step 3: 实现**：按 `J` 的方法序，用 `concrete->methods` 逐个查名字+签名。

- [ ] **Step 4: 跑它确认绿**

- [ ] **Step 5: Commit** `feat(rt): rt_itab 运行期兜底（Task 30，T-IFACE-06）`

### Task 31: isel 的装箱（`Convert` 到接口）

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/isel.rs`
- Test: 同 Task 28

- [ ] **Step 1: 写会红的测试**：`var i I = T{1}; var a any = i`（接口到 `any` 的转箱）
- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：`eface ← iface` 时 `type = tab.concrete`、`data` 原样（**不重新 box**）
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(arm64): 接口装箱的 isel（Task 31）`

### Task 32: 方法调用（`data` 作 receiver）

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/isel.rs`、`rgoc/crates/rgoc-ssa/src/build.rs`
- Test: 同 Task 28

- [ ] **Step 1: 写会红的测试**（`T-IFACE-03`：指针接收者改对象）

```go
type C struct{ n int }
func (c *C) Inc() { c.n++ }
type Incer interface{ Inc() }
func main() { c := &C{}; var i Incer = c; i.Inc(); i.Inc(); println(c.n) }
```
Expected: `2\n`

- [ ] **Step 2: 跑它确认红**

- [ ] **Step 3: 实现**：取 `fn[slot]` 后 `Indirect` 调用，第 0 实参是 `data`
  （指针接收者直接用 `data`；值接收者要先解引用一份 —— 由 `CalleeRef::Method` 的
  既有处置负责，见 `F2`）

- [ ] **Step 4: 跑它确认绿**

- [ ] **Step 5: Commit** `feat(arm64): 接口方法调用（Task 32，T-IFACE-03）`

### Task 33: 断言 `x.(T)` / `x.(J)`

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/isel.rs`
- Test: 同 Task 28

- [ ] **Step 1: 写会红的测试**（`T-IFACE-05`）

```go
func main() {
	var a any = 42
	v, ok := a.(int)
	println(v, ok)
	w, ok2 := a.(string)
	println(w, ok2)
}
```
Expected: `42 true\n false\n`（空字符串 + false）

- [ ] **Step 2: 跑它确认红**

- [ ] **Step 3: 实现**：`eface` 比 `type` 指针；`iface` 比 `tab.concrete`；
  到接口走 `rt_itab` 并判 `!= null`

- [ ] **Step 4: 跑它确认绿**

- [ ] **Step 5: Commit** `feat(arm64): 类型断言（Task 33，T-IFACE-05）`

### Task 34: type switch

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/isel.rs`、`rgoc/crates/rgoc-ssa/src/build.rs`
- Test: 同 Task 28

- [ ] **Step 1: 写会红的测试**（`T-IFACE-07`）：四分支（int / string / nil / default）
- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：拆成逐个 `x.(T)` 的比较链（`switch` 的块结构已由 M6 建好）
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(arm64): type switch（Task 34，T-IFACE-07）`

### Task 35: typed nil

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/isel.rs`（`== nil` 的接口分支）
- Test: 同 Task 28

- [ ] **Step 1: 写会红的测试**（`T-IFACE-04`）

```go
type I interface{ M() }
type T struct{}
func (T) M() {}
func main() {
	var p *T
	var i I = p
	println(i == nil, p == nil)
}
```
Expected: `false true\n`（**接口非 nil**，因为 tab 非空）

- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：接口与 `nil` 比较 = 「两个字都为空」；
  `iface` 的 `tab` 非空即非 nil
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(arm64): typed nil（Task 35，T-IFACE-04）`

### Task 36: 语料 60 与 `T-IFACE-*` 收口

- [ ] **Step 1: 跑** `bash tools/run-oracle/run-native.sh` ⇒ `ok=60 skip=9 bad=0 / 69`
- [ ] **Step 2: 逐条核 `T-IFACE-01..10`**（含 `T-IFACE-08/09/10`：接口比较 / 接口作 map 键 / 2 字布局）
- [ ] **Step 3: `D` 腿全绿**
- [ ] **Step 4: Commit** `test(obj): OBJ-3 收口与语料 60（Task 36）`

**Verify:** `ok=60 skip=9 bad=0 / 69`；`cargo test -p rgoc-driver --test obj_iface` 全绿。

---

# Phase 4 · funcval（60 → **67**）

### Task 37: `Instr::FuncAddr`

**Files:**
- Modify: `rgoc/crates/rgoc-ssa/src/ir.rs`、`build.rs`、`dump.rs`、`verify.rs`、
  `rgoc/crates/rgoc-arm64/src/isel.rs`
- Test: `rgoc/crates/rgoc-driver/tests/obj_funcval.rs`（新建）

**Interfaces:**
- Produces: `Instr::FuncAddr { dst: ValueId, func: FuncId }`；isel ⇒ `adrp/add`
- Consumes: 无

- [ ] **Step 1: 写会红的测试**（`T-FUNC-01`）

```go
func add(a, b int) int { return a + b }
func main() { f := add; println(f(1, 2)) }
```
Expected: `3\n`

- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现** + `C10` 追加 `funcaddr` 行语法 + `C4` §10 的签名
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(ssa): Instr::FuncAddr（Task 37，T-FUNC-01）`

### Task 38: `Instr::MakeClosure` 与 `V12`

**Files:**
- Modify: `ir.rs`、`build.rs`、`verify.rs`、`isel.rs`、`runtime/.../rt.c`
- Test: 同 Task 37

**Interfaces:**
- Produces: `Instr::MakeClosure { dst: ValueId, func: FuncId, captures: Vec<ValueId> }`；
  `V12`：`captures.len() == 被调方 captured.len()` 且类型逐个 identical
- Consumes: Task 37

- [ ] **Step 1: 写会红的测试**（`T-FUNC-02/05` + Review Focus ③）

```go
func main() {
	x := 5
	s := "hello"
	p := &x
	f := func() { println(x, s, len(s), *p) }   // 捕获 int + string（两字）+ *int
	f()
}
```
Expected: `5 hello 5 5\n` —— **string 只搬第一个字就会打印乱码/少一半**。

- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：`rt_alloc` 一个变长 funcval（`8 + 8*n`），填 `fn` 与各捕获
  （**多字捕获按整字搬**）
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(ssa): Instr::MakeClosure 与 V12（Task 38，T-FUNC-02/05）`

### Task 39: funcval 的 desc（变长 + captures 的 ptrmask）

**Files:**
- Modify: `rgoc/crates/rgoc-meta/src/desc.rs`（`closure_desc`）
- Test: `rgoc/crates/rgoc-meta/src/desc.rs`

**Interfaces:**
- Produces: `fn closure_desc(tt: &TypeTable, captures: &[TypeId]) -> TypeDesc`
  （`size = 8 + Σ align8(size_of(capture))` —— **不是** `8*(1+n)`：多字捕获要按实际宽度）
- Consumes: Task 2

- [ ] **Step 1: 写会红的测试**：捕获 `[int, string]` ⇒ `size = 8(fn) + 8(int) + 16(string) = 32`，
  位图 4 个字 `[true, false, true, false]`（fn 是指针、int 不是、string 的 ptr 是、len 不是）；
  再补一条 `[string, string]` ⇒ `size = 8 + 16 + 16 = 40`、位图 `[true, true, false, true, false]`
- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(meta): funcval 的 desc（Task 39）`

### Task 40: 隐式第 0 实参（`C5` §10）

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/abi.rs`（`place_args` 的 funcval 分支）、
  `isel.rs`（被调方读 `[x0+8+8i]`）
- Test: 同 Task 37

**Interfaces:**
- Produces: 调用 funcval 时 `x0 = funcval`，捕获从 `[x0+8+8i]` 读；
  `place_args` 的整数寄存器起点从 `x1` 开始（**仅当**被调方是闭包）
- Consumes: Task 38

- [ ] **Step 1: 写会红的测试**（`T-FUNC-06`）

```go
func mkAdder(n int) func(int) int { return func(x int) int { return x + n } }
func apply(f func(int) int, v int) int { return f(v) }
func main() { println(apply(mkAdder(10), 5)) }
```
Expected: `15\n`

- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现** + `C5` §10 写「funcval 的隐式第 0 实参」
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(arm64): funcval 隐式 ctx（Task 40，T-FUNC-06）`

### Task 41: 方法值

**Files:**
- Modify: `rgoc/crates/rgoc-ssa/src/build.rs`（`MethodRef` → `MakeClosure`）
- Test: 同 Task 37

- [ ] **Step 1: 写会红的测试**（`T-FUNC-04`）

```go
type T struct{ n int }
func (t T) Get() int { return t.n }
func (t *T) Inc()    { t.n++ }
func main() {
	t := T{7}
	f := t.Get          // 值接收者：捕获副本
	t.n = 9
	println(f(), t.Get())
	g := t.Inc          // 指针接收者：同一对象
	g(); g()
	println(t.n)
}
```
Expected: `7 9\n11\n`

- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：`MakeClosure{func: 方法体, captures: [recv]}`（值接收者要先拷一份）
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(ssa): 方法值（Task 41，T-FUNC-04）`

### Task 42: 捕获写回（与 `Frame::heap` 的交互）

**Files:**
- Modify: `rgoc/crates/rgoc-ssa/src/build.rs`、`rgoc/crates/rgoc-arm64/src/frame.rs`
- Test: 同 Task 37

- [ ] **Step 1: 写会红的测试**（`T-FUNC-03` + `RM-3` 的牙）

```go
func main() {
	x := 0
	f := func() { x++ }
	f(); f(); f()
	println(x)          // 闭包写的必须外层可见
	x = 100
	f()
	println(x)
}
```
Expected: `3\n101\n`

- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：被捕获的槽必须走 `Frame::heap`（取址槽），捕获的是**同一个 `Cell`**
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(frame): 捕获写回（Task 42，T-FUNC-03）`

### Task 43: 拆 `SSA-UNSUPPORTED-FUNC-VALUE`

**Files:**
- Modify: `rgoc/crates/rgoc-ssa/src/build.rs`、`diag.rs`（`ALL` 8 → 7）、守卫测试
- Test: `rgoc/crates/rgoc-driver/tests/ssa_reject.rs`

- [ ] **Step 1: 改守卫**：源码扫不到 `SsaCode::FuncValue`（沿 M8 的「五条码不许再出现」）
- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 删码 + 删拒绝分支**
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(ssa): 下线 SSA-UNSUPPORTED-FUNC-VALUE（Task 43）`

### Task 44: 语料 67 与 `T-FUNC-*` 收口

- [ ] **Step 1: 跑** ⇒ `ok=67 skip=2 bad=0 / 69`
- [ ] **Step 2: 逐条核 `T-FUNC-01..08`**（含 `T-FUNC-07` 嵌套闭包、`T-FUNC-08` 指针语义）
- [ ] **Step 3: `D` 腿全绿**
- [ ] **Step 4: Commit** `test(obj): OBJ-4 收口与语料 67（Task 44）`

**Verify:** `ok=67 skip=2 bad=0 / 69`；`cargo test -p rgoc-driver --test obj_funcval` 全绿。

---

# Phase 5 · defer / panic（67 → **69**）

> **本 Phase 改一条既有的临时形态**：Task 14 的 `rt_panic_bounds` 当时是「直接打文本 +
> `exit(2)`」，Task 51 把它改接到 `rt_panic` 的返回码传播上。

### Task 45: `Instr::Defer` 与 `V11`

**Files:**
- Modify: `ir.rs`、`build.rs`、`dump.rs`、`verify.rs`
- Test: `rgoc/crates/rgoc-driver/tests/exc_native.rs`（新建）

**Interfaces:**
- Produces: `Instr::Defer { call: Box<Instr> }`；`V11`：`call` 必须是 `Instr::Call`
- Consumes: 无

- [ ] **Step 1: 写会红的测试**（`T-EXC-01`）

```go
func main() {
	defer println(1)
	defer println(2)
	defer println(3)
	println(0)
}
```
Expected: `0\n3\n2\n1\n`

- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现** + `C10` 追加 `defer` 行语法 + `C4` §10
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(ssa): Instr::Defer 与 V11（Task 45，T-EXC-01）`

### Task 46: `rt_defer_push` 与全局链

**Files:**
- Modify: `runtime/native/.../rt.c`、`isel.rs`
- Test: 同 Task 45

**Interfaces:**
- Produces: `void rt_defer_push(void *fn, void *fp, u64 nargs, ...)`；
  `DeferRec { next: *DeferRec, fp: *u8, fn: *code, args... }`
- Consumes: `rt_alloc`

- [ ] **Step 1: 写会红的测试**：`defer` 带 3 个实参（含一个 `string`）；
  断言「实参在**注册时**求值」——`x := 1; defer println(x); x = 2` ⇒ 打印 `1`
- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：记录压到全局链头；`fp` 用 `x29`（M7 已有帧指针）
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(rt): rt_defer_push（Task 46）`

### Task 47: `rt_deferrun` 与 epilogue

**Files:**
- Modify: `runtime/native/.../rt.c`、`rgoc/crates/rgoc-arm64/src/isel.rs`（每个 `Return` 前插）
- Test: 同 Task 45

**Interfaces:**
- Produces: `void rt_deferrun(void *cur_fp)` —— 从链顶弹出所有 `fp == cur_fp` 的，逆序调
- Consumes: Task 46

- [ ] **Step 1: 写会红的测试**（`T-EXC-02`：命名返回值被 defer 改）

```go
func f() (r int) { defer func() { r++ }(); return 1 }
func main() { println(f()) }
```
Expected: `2\n`

- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：`Return` 的 epilogue 里先调 `rt_deferrun(x29)` 再 `ret`
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(arm64): rt_deferrun 与 epilogue（Task 47，T-EXC-02）`

### Task 48: `rt_panic` 与全局状态

**Files:**
- Modify: `runtime/native/.../rt.c`
- Test: 同 Task 45

**Interfaces:**
- Produces: `void rt_panic(const TypeDesc *d, void *val)`；
  `int rt_panicking(void)`；全局 `current_panic { desc, val }`
- Consumes: Task 47

- [ ] **Step 1: 写会红的测试**（`T-EXC-03`）

```go
func main() {
	defer println("d")
	panic("boom")
}
```
Expected: `d\npanic: boom\n` + 退出 2

- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：设状态后**返回**（不 unwind）；重入时若已有 panic 则覆盖
  （Review Focus ④ 的第一半）
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(rt): rt_panic（Task 48，T-EXC-03）`

### Task 49: `panic_check` pass 与 `V10`

**Files:**
- Modify: `rgoc/crates/rgoc-ssa/src/`（新建 `panic_check.rs`，`lib.rs` 挂上）、`verify.rs`（`V10`）
- Test: `rgoc/crates/rgoc-driver/tests/exc_native.rs`

**Interfaces:**
- Produces: `pub fn inject(f: &mut SsaFunc)` —— 在每个可达 `Call` 之后追加
  `If(rt_panicking()) -> goto <epilogue 块>`；`V10` 反查「每个 `Call` 后都有」
- Consumes: Task 48

- [ ] **Step 1: 写会红的测试**（`T-EXC-04`，`RM-1` 的牙）

```go
func h() { defer println("h"); panic(1) }
func g() { defer println("g"); h() }
func f() { defer println("f"); g() }
func main() { f() }
```
Expected: `h\ng\nf\npanic: 1\n` + 退出 2 —— **少一层检查就少一行**。

- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：epilogue 块 = 「跑 defer + return」；注入是**幂等**的
  （重复跑 pass 不重复插）
- [ ] **Step 4: 跑它确认绿** + `cargo test -p rgoc-ssa --lib`（`V10` 的守卫）
- [ ] **Step 5: Commit** `feat(ssa): panic_check pass 与 V10（Task 49，T-EXC-04）`

### Task 50: `rt_recover`

**Files:**
- Modify: `runtime/native/.../rt.c`
- Test: 同 Task 45

**Interfaces:**
- Produces: `int rt_recover(void **out)` —— 有效时清状态、写 `*out`、返回 1
- Consumes: Task 48

- [ ] **Step 1: 写会红的测试**（`T-EXC-05/06/07`）

```go
func main() {
	func() {
		defer func() { println("recovered", recover()) }()
		panic("x")
	}()
	println("after")
	func() { println("no-defer", recover()) }()   // 非 defer 里：无效
}
```
Expected: `recovered x\nafter\nno-defer <nil>\n` + 退出 **0**

- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：「正在跑 defer」用 `rt_deferrun` 期间置的全局位
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(rt): rt_recover（Task 50，T-EXC-05/06/07）`

### Task 51: 四条运行时检查接到 `rt_panic`

**Files:**
- Modify: `runtime/native/.../rt.c`（`rt_panic_bounds` 改接、新增 `rt_panic_div0` /
  `rt_panic_nil` / `rt_panic_nilmap`）
- Test: 同 Task 45

**Interfaces:**
- Produces: 四条的文本与解释器逐字节（`C9` §4.5）
- Consumes: Task 48

- [ ] **Step 1: 写会红的测试**（`T-EXC-08/09/10`）：三条各一个 fixture，
  **外加一条**「panic 被 defer recover 住之后程序继续跑」（`T-EXC-05` 的追加，
  即 Review Focus ④ 的第二半：recover 之后再 panic）
- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：四条都走 `rt_panic`（**不再** `exit(2)`）；`main` 的 epilogue 才退
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(rt): 四条运行时检查（Task 51，T-EXC-08/09/10）`

### Task 52: `main` 的 epilogue

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/module.rs`（`_start` / `main` 的收尾）
- Test: 同 Task 45

**Interfaces:**
- Produces: `main` 返回后若 `rt_panicking()` ⇒ 打印 `panic: <值>` + exit 2；
  否则 exit 0
- Consumes: Task 51

- [ ] **Step 1: 写会红的测试**：`main` 里 `panic(42)` ⇒ 退出 2 + `panic: 42`；
  `main` 正常返回 ⇒ 退出 0
- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：`main` 返回后的收尾序列 —— `if rt_panicking() { rt_print_panic();
  exit(2) } else { exit(0) }`；`rt_print_panic` 打 `panic: ` + 按 `current_panic.desc`
  格式化值（复用 `C9` §4.3 的 `print` 规则，**不是**另一套）
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(arm64): main 的 epilogue（Task 52）`

### Task 53: 拆 `SSA-UNSUPPORTED-DEFER`

**Files:**
- Modify: `rgoc/crates/rgoc-ssa/src/build.rs`、`diag.rs`（`ALL` 7 → 6）、守卫
- Test: `ssa_reject.rs`

- [ ] **Step 1: 改守卫**（源码扫不到 `SsaCode::Defer`）
- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 删码 + 删拒绝分支**
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(ssa): 下线 SSA-UNSUPPORTED-DEFER（Task 53）`

### Task 54: 语料 69

- [ ] **Step 1: 跑** `bash tools/run-oracle/run-native.sh` ⇒ `ok=69 skip=0 bad=0 / 69`
- [ ] **Step 2: 逐文件对 `D` 腿**
- [ ] **Step 3: 若有 `M'`**（撞别的墙）⇒ 记三栏，且 `T-CORP-02` 的两向断言仍成立
- [ ] **Step 4: Commit** `test(native): 语料 69（Task 54，T-CORP-01/02）`

### Task 55: `T-EXC-*` 收口与 `EXC-1`

- [ ] **Step 1: 逐条核 `T-EXC-01..10`**（含 `T-EXC-04` 的逐层展开）
- [ ] **Step 2: 跑 `cargo test -p rgoc-driver --test exc_native`** ⇒ 全绿
- [ ] **Step 3: Commit** `test(exc): EXC-1 收口（Task 55）`

**Verify:** `ok=69 skip=0 bad=0 / 69`；`cargo test -p rgoc-driver --test exc_native` 全绿；
`cargo test -p rgoc-ssa --lib`（`V10/V11`）全绿。

---

# Phase 6 · 根协议（69 不动，`ROOT-1`）

### Task 56: `live.ptrs` 升级到「字」粒度

**Files:**
- Modify: `rgoc/crates/rgoc-ssa/src/live.rs`（`Safepoint` 加 `ptr_words: Vec<(ValueId, u8)>`）
- Test: `rgoc/crates/rgoc-ssa/src/live.rs`（单测）

**Interfaces:**
- Produces: `Safepoint.ptr_words: Vec<(ValueId, u8)>` —— 每个活值里**哪几个字**是根
  （按 `TypeDesc.gcdata` 算）
- Consumes: Task 2

- [ ] **Step 1: 写会红的测试**（`T-ROOT-05`）

```rust
#[test]
fn 根的字粒度() {
    // var s []int 活在 safepoint 上 ⇒ 只有字 0 是根
    let sp = sp_of("var s []int; print(s)");
    assert_eq!(sp.ptr_words, vec![(v_s, 0)]);
    // string ⇒ 1 个字；iface ⇒ 2 个字
    assert_eq!(sp_of("var x any; print(x)").ptr_words.len(), 2);
}
```

- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：`is_ptr_ty` 的那一处换成「查 gcdata 位图」
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(ssa): 根的字粒度（Task 56，T-ROOT-05）`

### Task 57: 回边 safepoint

**Files:**
- Modify: `rgoc/crates/rgoc-ssa/src/live.rs`（`is_safepoint` 扩到终止符）
- Test: 同 Task 56

**Interfaces:**
- Produces: 回边（`h` 支配 `b` 的边 `b → h`）的**终止符**成为 safepoint
- Consumes: `dom.rs`（`F16`）

- [ ] **Step 1: 写会红的测试**（`T-ROOT-04`）：一个循环 100 次、循环体无调用无分配的
  函数 ⇒ `safepoints` 里至少有一条在回边块上
- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：`is_safepoint` 从「只看 `Instr`」改成「也看终止符」——
  `Goto`/`If` 的目标块支配本块 ⇒ 该终止符是 safepoint；`live` 集合取 `live_out ∪ 终止符的读`
  （与既有的 `after[last]` 同一份，别另算）
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(ssa): 回边 safepoint（Task 57，T-ROOT-04）`

### Task 58: `STACK-MAP v1` 渲染

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/module.rs`（`render_stackmap`）
- Test: `rgoc/crates/rgoc-driver/tests/stackmap.rs`（新建）

**Interfaces:**
- Produces: `STACK-MAP v1` 文本（设计 §3.11 的三行形态）；位序：
  bit `i` ↔ `fp - frame_size + 8*i`
- Consumes: Task 56–57

- [ ] **Step 1: 写会红的测试**（`T-ROOT-08`）：一条 fixture 的文本逐行比（含
  `# func …` 头与 `sp …` 行）
- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：逐函数逐 safepoint 算位图 —— 对每个 `(ValueId, 字下标)` 查
  `Frame::values[ValueId]` 得槽偏移 `off`，置 bit `(frame_size + off)/8 + 字下标`；
  区间按设计 §3.11 的 `[fp - frame_size, fp)`、bit 0 = 最低地址
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(arm64): STACK-MAP v1 渲染（Task 58）`

### Task 59: `.stackmap` 产物与 `.rodata` 表

**Files:**
- Modify: `rgoc/crates/rgoc-driver/src/build_cmd.rs`（多写一份产物）、`module.rs`（发射
  `RtFuncInfo`）
- Test: 同 Task 58

**Interfaces:**
- Produces: `<out>.stackmap` 文件 + `.rodata` 的 `RtFuncInfo { frame_size, n_sp, sp_off, sp_bits }`
- Consumes: Task 58

- [ ] **Step 1: 写会红的测试**：`build` 之后两个产物都存在；`.stackmap` 的行数 ==
  `Σ(n_sp + 1)`
- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(arm64): .stackmap 产物与 rodata 表（Task 59）`

### Task 60: 两方逐字节对账

**Files:**
- Test: `stackmap.rs`（`T-ROOT-01/02/03`）

**Interfaces:**
- Consumes: Task 59

- [ ] **Step 1: 写会红的测试**：解析 `.stackmap` 与 ELF 里的 `RtFuncInfo`，
  对 69 个语料**逐字节**比（沿 `native_frame_map` 的做法）
- [ ] **Step 2: 跑它确认红**（预期有差异）
- [ ] **Step 3: 按差异修**（**不许**改对账口径）
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `test(root): stackmap 两方对账（Task 60，T-ROOT-01/02/03）`

### Task 61: `RM-2` 的牙（slice 的 ptr 字是唯一根）

**Files:**
- Test: `rgoc/crates/rgoc-driver/tests/root_grain.rs`（新建，`T-ROOT-06`）

- [ ] **Step 1: 写会红的测试**：`s := make([]int, 3)` 之后 `s` 是**唯一**引用、
  活在 safepoint 上 ⇒ 位图里恰有 1 位；**再断言**「若按『值是指针类型』判则 0 位」
  （把那条错实现直接写进断言，证明这条判据有牙）
- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 修到绿**（大概率 Task 56 已经对了；红就回去修 56）
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `test(root): slice ptr 唯一根（Task 61，T-ROOT-06）`

### Task 62: `T-ROOT-*` 收口与 `U-M9-10` 的守卫

- [ ] **Step 1: 逐条核 `T-ROOT-01..08`**
- [ ] **Step 2: `U-M9-10` 的守卫**：一条 `unsafe.Pointer` 算术后不被当根的 fixture，
  断言「位图里没有它」（写清这是**已知的漏扫**）
- [ ] **Step 3: Commit** `test(root): ROOT-1 收口（Task 62）`

**Verify:** `cargo test -p rgoc-driver --test stackmap --test root_grain` 全绿；
`cargo test -p rgoc-ssa --lib`（`live` 的 3 条）全绿。

---

# Phase 7 · channel 契约（`SCH-1`，不跑 Go 代码）

### Task 63: `hchan` 的 Rust 镜像

**Files:**
- Create: `rgoc/crates/rgoc-arm64/src/hchan.rs`
- Test: `rgoc/crates/rgoc-driver/tests/chan_contract.rs`（新建）

**Interfaces:**
- Produces: `#[repr(C)] pub struct Hchan { qcount: u32, dataqsiz: u32, buf: *mut u8, elemsize: u32, closed: u32, elemtype: *const TypeDesc, sendq: Waitq, recvq: Waitq, lock: u32 }`；
  `Waitq { head: *mut Sudog, tail: *mut Sudog }`
- Consumes: `TypeDesc`

- [ ] **Step 1: 写会红的测试**（`T-SCH-01`）：`size_of::<Hchan>()` 与逐字段 `offset_of`
  等于 `C11` §8 冻结的值
- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**（字段序对齐官方 `runtime/chan.go:34`）
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(arm64): hchan 布局（Task 63，T-SCH-01）`

### Task 64: 三操作真值表

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/hchan.rs`（纯数据结构上的三操作）
- Test: 同 Task 63

- [ ] **Step 1: 写会红的测试**（`T-SCH-02/03`）：`try_recv` / `try_send` / `close` 在
  （空 / 半 / 满 / 已关）×（有无等待者）上的结果 == `M8` 的 `chan_obj` 同一张表；
  `park_*`/`revoke` 的 `head/tail` 不变式
- [ ] **Step 2: 跑它确认红**
- [ ] **Step 3: 实现**：真值表照 `M8` 的 `chan_obj` 抄（**同一张表**，不另设计）；
  `try_send` 的 `closed` 检查排**第一**（`M8` 的 `R-8-2` 教训：排在 buf 之后就静默塞缓冲）；
  `park_*` 维护 `head/tail` 双端
- [ ] **Step 4: 跑它确认绿**
- [ ] **Step 5: Commit** `feat(arm64): hchan 三操作（Task 64，T-SCH-02/03）`

### Task 65: `C11` §8

**Files:**
- Modify: `docs/contracts/C11-concurrency.md`

- [ ] **Step 1: 写 §8**：`hchan` 的字段表（逐字段标「M11 需要 / 可省」）+ 抬头
  「**参照不同构**：C11 是单线程协作式，native 是多核抢占」
- [ ] **Step 2: 核对**：`Task 63` 的 `offset_of` 与 §8 的表**同一个数**（写成测试更好：
  `T-SCH-01` 直接从 `C11` 读表 —— 沿 `R-51-1` 的文档驱动 golden）
- [ ] **Step 3: Commit** `docs(c11): hchan 契约（Task 65）`

### Task 66: `T-SCH-04` 的守卫

**Files:**
- Test: `chan_contract.rs`

- [ ] **Step 1: 写会红的守卫**：扫 `rgoc/crates/rgoc-arm64/src/**` 与 `rt.c`，
  **不许**出现 `rt_go` / `rt_select` 符号（`M9` 不交 native 并发）
- [ ] **Step 2: 跑它确认绿**（现在是绿的 —— 这条是防**未来**偷跑）
- [ ] **Step 3: Commit** `test(sch): M9 不交 native 并发的守卫（Task 66，T-SCH-04）`

**Verify:** `cargo test -p rgoc-driver --test chan_contract` ⇒ 全绿；
`grep -rn "rt_go\|rt_select" rgoc/crates/rgoc-arm64/src runtime/` ⇒ 无输出。

---

# Phase 8 · 收口

### Task 67: 契约定稿

**Files:**
- Modify: `docs/contracts/C4-ssa.md`（§10）、`C5-abi.md`（§10）、`C9-values-and-output.md`、
  `C10-ssa-dump.md`、`C11-concurrency.md`

- [ ] **Step 1: C4 §10**：3 个新 `Instr` 的签名 + `V10/V11/V12`
- [ ] **Step 2: C5 §10**：`TypeDesc` 的**内存布局**（40 B，逐字段偏移）、`MethodTable`、
  `ITab`、`GlobalRoot`、`RtFuncInfo`、`STACK-MAP v1` 语法、funcval 隐式第 0 实参、
  `rt_*` 的 sret 约定
- [ ] **Step 3: C9**：补 `out of memory` 一条；确认四条运行期文本与解释器一致
- [ ] **Step 4: C10**：确认只追加了 3 行 `op`（`funcaddr` / `makeclosure` / `defer`），
  既有 golden 全部仍有效（跑 `T-SSA-46`）
- [ ] **Step 5: Commit** `docs(contracts): M9 契约定稿（Task 67）`

### Task 68: `M9-manifest.json` 骨架

**Files:**
- Create: `docs/milestones/M9-manifest.json`

- [ ] **Step 1: 照 M8 的形状**：`status` / `gate`（10 条，全 `pending`）/ `unsupported`（10）/ `corpus_lists` /
  `budget` / `open_gaps` / `measured` / `phase_summary`（`done: false`）
- [ ] **Step 2: 校验** `python3 -c "import json;json.load(open('docs/milestones/M9-manifest.json'))"`
- [ ] **Step 3: Commit** `docs(m9): manifest 骨架（Task 68）`

### Task 69: `manifest_m9.rs` 对账

**Files:**
- Create: `rgoc/crates/rgoc-driver/tests/manifest_m9.rs`

- [ ] **Step 1: 照 `manifest_m8.rs` 的五类谎 + 合成反例**（每类至少 2 条）
- [ ] **Step 2: 把 manifest 填实**（十条门禁 `pass` + `measured` 按实测）
- [ ] **Step 3: 跑它确认绿**
- [ ] **Step 4: Commit** `test(m9): manifest 对账（Task 69）`

### Task 70: 变异 10 条

**Files:**
- Create: `scripts/mutation-m9.sh`（4 条）、`scripts/mutation-m9-notes.md`（全 10 条）
- Test: `bash scripts/mutation-m9.sh` ⇒ `红 4 / 失手 0`

- [ ] **Step 1: 写脚本**（01/02/03/08 四条：不清零 / string 只标 1 位 / append 不拷 /
  deferrun 正序）
- [ ] **Step 2: 跑** ⇒ 红 4 / 失手 0；**失手的写进 notes 的诊断，不藏**
- [ ] **Step 3: 补 notes**（另外 6 条各自的实测结论）
- [ ] **Step 4: Commit** `test(m9): 变异 4 条脚本 + 10 条笔记（Task 70）`

### Task 71: 文档回写

**Files:**
- Modify: `docs/README.md`、`AGENTS.md`、`docs/milestones/GAP-01.md`、`docs/03-roadmap.md`

- [ ] **Step 1: README**：M9 五行齐（design/tests/plan/manifest/report）+ 契约清单加 C5 §10
- [ ] **Step 2: AGENTS**：一句话状态 + M9 段（新 crate `rgoc-meta` 要进 crate 地图）
- [ ] **Step 3: GAP-01**：M9 节（`RT-MEM/CMP/POLY/EXC` 逐条 ✅/◐/✗ + 理由）
- [ ] **Step 4: roadmap**：§M9 加「已收口」状态块
- [ ] **Step 5: Commit** `docs(m9): 文档回写（Task 71）`

### Task 72: 四条 cargo + 三套语料

- [ ] **Step 1: 四条**：`fmt --check` / `check` / `clippy -D warnings` / `test`（记 passed 数）
- [ ] **Step 2: 语料**：`run-native.sh`（69） / `run-collide --run`（69 绿） /
  `run-collide --ssa`（46 绿 23 跳） / `run-chan.sh`（7/12/0）
- [ ] **Step 3: 自检** `check-m1-consistency.py`
- [ ] **Step 4: Commit** `chore(m9): 四条 cargo 与零回归（Task 72）`

### Task 73: `M9-report.md`

**Files:**
- Create: `docs/milestones/M9-report.md`

- [ ] **Step 1: 九节**：读数 / 交付 / 关键决策的实测复核 / **语料从 39 到 69 的逐段表** /
  真错清单（按「为什么先前没抓到」分类）/ 白名单 10 条 / 与 M5–M8 的边界 /
  **冻结与不承诺** / 接手指南
- [ ] **Step 2: 特别写清三件事**：① `U-M9-10`（interior pointer）是语义级拒绝；
  ② `D-M9-12` 的返回码传播让代码体积 +N%（实测数）；③ `.rodata` 实际体积
- [ ] **Step 3: Commit** `docs(m9): 报告与冻结（Task 73）`

### Task 74: 收口

- [ ] **Step 1: manifest 填实**（`done: true`、`phase_summary.tasks_done = "Task 1–74"`）
- [ ] **Step 2: 跑 `manifest_m9`** ⇒ 绿
- [ ] **Step 3: 再跑四条 cargo + 三套语料**（与 Task 72 的数字对一遍）
- [ ] **Step 4: Commit + push** `docs(m9): 收口（Task 74）`

**Verify:** `git status` 干净；`manifest_m9` 绿；四条 cargo 退出码全 0。

---

# 附录 A：Task ↔ 门禁 ↔ 语料

| Phase | Task | 门禁 | `N'` |
|---|---|---|---|
| 0 | 1–10 | `MEM-1` `MEM-2` | 39 |
| 1 | 11–17 | `OBJ-1` `CORP-1` | **46** |
| 2 | 18–26 | `OBJ-2` | **51** |
| 3 | 27–36 | `OBJ-3` | **60** |
| 4 | 37–44 | `OBJ-4` | **67** |
| 5 | 45–55 | `EXC-1` | **69** |
| 6 | 56–62 | `ROOT-1` | 69 |
| 7 | 63–66 | `SCH-1` | 69 |
| 8 | 67–74 | 全部（收口） | 69 |

# 附录 B：本计划的「可按实测重写」条款

`Q1` 选了「一次到底」，代价是 Phase 5–7 的任务写在还没写的代码上。所以：

* **Phase 0–4 收口时，Phase 5–7 的 Task 允许按实测重写**（沿 `M8-plan.md` 的做法）。
* 每次重写必须：① 在 `M9-tests.md` §9.2 记一行 `R-9-y`；② 说明「为什么原计划不可行」；
  ③ 门禁与语料数字**不许**下调。
* Phase 5 的 `Task 51`（四条检查接 `rt_panic`）是**必然要动的**：`Task 14` 的临时形态
  在 Phase 5 之前存在，这是计划里唯一一处「先写错的、后面改对的」。
