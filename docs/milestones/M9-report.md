# M9 报告：native 语义面（内存 / 复合类型 / 接口 / 闭包 / 异常 / 根协议 / channel 契约）

> **一句话**：M7 的 native 后端从「39 个语料能跑」扩到「**69 个全跑**，且与解释器
> 逐字节一致」；新增 `rgoc-meta` 一个 crate；十条门禁 **10/10 pass**；四条 cargo 全绿
> （224 suite / **2179 passed / 0 failed**）；四套语料零回归。
>
> 不承诺的十件事写在第 6 节，其中 `U-M9-10`（interior pointer）是**语义级拒绝**。

---

## 1. 读数（全部实测，可复跑）

| 项 | 值 | 怎么复跑 |
|---|---|---|
| native 语料 | **ok=69 skip=0 bad=0 / 69** | `bash tools/run-oracle/run-native.sh` |
| 三方差分（`--run`） | **69 绿 / 0 跳过 / 0 红** | `run-collide.sh run-selected.txt --run` |
| `--via=ssa` | **69 绿 / 0 跳过 / 0 红**（拒绝面为空） | `run-collide.sh run-selected.txt --ssa` |
| 并发语料（M8 面） | **7 ok / 12 skip / 0 bad / 19** | `run-chan.sh chan-input.txt` |
| workspace 测试 | **224 suite / 2179 passed / 0 failed / 1 ignored** | `cargo test --workspace --no-fail-fast` |
| 四条 cargo | `fmt --check` / `check --all-targets` / `clippy -D warnings` / `test` **全绿** | 四条各跑一次 |
| 变异（脚本 4 条） | **红 4 / 失手 0** | `bash scripts/mutation-m9.sh` |
| 变异（笔记 10 条） | **红 7 / 绿 3**（3 条绿是判据缺口，已记 `G-M9-04..06`） | `scripts/mutation-m9-notes.md` |
| 二进制体积（`print.go`） | `.text` 30888 B / `.rodata` 4100 B；`panic` 检查点 19 个 | `size -A` |

### 1.1 三件必须写清的量

**① `U-M9-10`（interior pointer）是语义级拒绝，不是「还没做」。**
`root_grain::u_m9_10_uintptr_不是根_守卫` 钉住当前行为：`u := uintptr(unsafe.Pointer(&x))`
之后 `u` 在栈图里**没有任何位**。这与 Go 语义一致（`uintptr` 不是引用），但它同时是
**能力边界** —— M10 拿到一个指向对象**中部**的指针时，没有对象头/区间表把它映射回
对象起点。roadmap 要求「明确支持或拒绝」，我们选**拒绝**，并在 `C5` §11 与本节各写一次。

**② 返回码传播的体积代价（`D-M9-12`）实测：很小。**
`print.go` 有 **19 个** panic 检查点（`bl rt_panicking` + `cbnz x0, <本帧出口>`）。
把 `cbnz` 去掉重建，`.text` 从 30888 降到 30816 —— **省 72 B（0.23%）**。
即整条机制（两条指令 × 19）约 **152 B ≈ 0.49%**。设计稿担心的「+N%」没有发生，
因为检查点只落在**调用/断言/越界**点，不是每条指令。

**③ `.rodata` 体积**：`print.go` 是 4100 B —— 里面是 `TypeDesc`（每个类型 40 B）+
`gcdata` 位图 + 方法表 + itab + `funcval` + `RtFuncInfo`/`sp_bits`。
**栈图很便宜**：位图宽度 = `ceil(frame_size/8/64)` 个字，几十字节的帧 ⇒ **1–2 个 u64**。

---

## 2. 交付

| 面 | 内容 |
|---|---|
| 新 crate | **`rgoc-meta`**：`TypeDesc`（40 B）/ `Kind`（15 值）/ `flags`（3 位）/ 方法表形状 |
| `rgoc-ssa` | IR 扩展（6 个 `Instr` + 2 个 `CalleeRef` + `methods`/`itabs`/`captured`/`ctx_abi`）；`build` 的接口/闭包/defer/method-value 下降；`live` 的**字粒度**根（`ptr_words`）+ 回边 safepoint |
| `rgoc-interp` | SSA 求值器的对象模型（切片/map/接口/闭包/defer/恢复点）；nil 解引用改成**可恢复** panic（`R-9-25`）；`uintptr(p)` 非 0（`R-9-35`） |
| `rgoc-arm64` | `isel`：对象/接口/闭包/defer/断言/空指针检查；`frame`：逃逸槽、超大帧 mmap；`module`：desc/itab/funcval/STACK-MAP/`hchan` |
| 新产物 | **`<out>.stackmap`**（`STACK-MAP v1`，总写）+ `.rodata` 的 `rt_funcs`/`rt_nfuncs` |
| 契约 | `C4` §10 / `C5` §8.4+§11 / `C9` §6.1 / `C10`（只追加 op）/ `C11` §7 |

---

## 3. 关键决策的实测复核

| 决策 | 设计时的说法 | 实测结论 |
|---|---|---|
| `D-M9-1` 8 字节对象头 | 所有堆对象带 `*TypeDesc` | ✅ `desc_layout` 在 69 语料的类型闭包上逐类型对上 |
| `D-M9-5` 净增 3 个 `Instr` | `FuncAddr`/`MakeClosure`/`Defer` | ❌ **实测 6 个 + 2 个 `CalleeRef`**（`R-9-38`）：还差 `CapturePtr`/`TypeAssert`/`NilCheck`/`CalleeRef::{Iface,Runtime}`。计划里的 `V10/V11/V12` **一条都没落地**，也不需要 |
| `D-M9-12` panic 用返回码传播 | 不 unwind（`.framemap` 不在 ELF 里，`longjmp` 会漏扫根） | ✅ 69 语料全过；体积代价 **0.49%**（§1.1②）；代价是**每个调用点两条指令**，且 `check_panic` 必须用 `cbnz` 直跳（`R-9-24`） |
| `D-M9-14` channel 只交契约 | 不跑 Go 代码 | ⚠️ 计划要「一个 `#[repr(C)]` struct 带三操作」，**做不到**（`unsafe_code=forbid`）⇒ 拆成 `Hchan` + `ChanState`（`R-9-37`），加守卫防漂 |
| `D-M9-13` 根协议 | `ptrs` 升到「字」粒度 | ✅ 但**计划的粒度还不够**：`ptrs` 从「值是指针类型」放宽成「至少一个字是根」（`R-9-31`），否则 `[]int` 漏根（`RM-2`） |

### 3.1 根协议的两处保守（`G-M9-03`）

1. **聚合实参**在栈上只标 1 位（那格是指向**调用方槽**的指针，不是堆根）；
2. 帧里的 **`addr_slots`** 不单独标（能读到它的内容必然以某个活值的 `Load`/`AddrOf` 出现）。

两处都选了「多标/少标都不会漏活对象」的那一侧，但都**没有**「不保守也正确」的判据。

---

## 4. 语料从 39 到 69 的逐段

| Phase | 解锁的面 | N'（计划） | 收口时 |
|---|---|---|---|
| M7 | 单字标量 / `string` / 数组 / 指针 / 全局 | 39 | 39 ✅ |
| 0 | 堆、对象头、`TypeDesc`、分配器 | 39 | 39（面在，语料不动）|
| 1 | 切片（三字值）与逃逸槽 | 46 | 46 ✅ |
| 2 | map（开放寻址 + 线性探测 + tomb） | 51 | 51 ✅ |
| 3 | 接口（装箱 / itab / 方法派发 / 断言） | 60 | 60 ✅ |
| 4 | 函数值（无捕获 → 闭包 → 方法值） | 67 | **69** ✅（`method5` 是 Phase 4c 才通的）|
| 5 | defer / panic / recover / nil 语义 | **69** | 69 ✅ |
| 6 | 根协议（`STACK-MAP`） | 69 | 69（协议面，语料不动）|
| 7 | channel 契约（不跑） | 69 | 69（协议面，语料不动）|

> Phase 4 的 67 → 69 是本轮唯一一次「计划值被实测超过」：`method5.go`（方法值）与
> `print.go`（引用型打印）在计划里算在 Phase 5，实际分别由 Phase 4c 与 5c 拿下。

---

## 5. 真错清单（按「为什么先前没抓到」分类）

M9 一共挖出 **6 类**既有 bug。分类比清单本身有用：它说明**判据的形状**哪里漏了。

| 类 | 症状 | 为什么先前没抓到 | 修订 |
|---|---|---|---|
| **A. 「纯指令」假设错了** | `a.h()` 不 panic（DCE 把调用当纯的删了） | 判据都在「有副作用」的调用上；空方法体的 `effect` 全空 | `R-9-21`（`Load` 只在地址可证非空时算纯）|
| **A'** | 没人用的**单值类型断言**被 DCE 删掉 ⇒ `_ = i.(J)` 不 panic | 同上：失败即 panic 是「可观察的副作用」，而 DCE 只看 dst 有没有人用 | `R-9-30` |
| **B. 一条指令两处算** | `tt.fn` 的类型是 `tt`（HIR 给 `Field` 标了底座类型） | 0 结果调用的 `invalid` 一路漏到帧布局才炸 | `R-9-20` |
| **C. 两边各一份实现** | `--via=ssa` 打 `false`、native 打 `true`（`uintptr(p)` 恒 0） | 语料里没有 `uintptr` 转换 | `R-9-35` |
| **C'** | `m[1]` 与 `seq(11,13)` 不等（`bigmap` 的 `BUG mismatch`）——**两边同时被截断**所以比较仍相等 | 「两边都对」不等于「两边都错得一样」也能过；只有**逐字节对官方 `.out`** 才抓得到 | `R-9-29`（大聚合返回走堆）|
| **D. 帧是定长的，栈不是** | `use(*bigstructp)` 段错误（128 MiB 帧 vs 8 MiB 栈） | M7 的语料帧最大 10 KiB | `R-9-22` |
| **E. 语言级细节** | 零大小类型的 `load`/`store` 仍读写 8 字节（踩邻居槽）；nil 接口断言段错误或静默成功；`__imeth_` 桩不按宽度读接收者 | 这些形态都没有**独立**的语料：`struct{}`、nil 接口、1 字节接收者 | `R-9-27`/`R-9-30` |

**一条元教训**：`C'` 那类（两边同时错）只能靠**外部标准**发现 —— 这里就是
`ssa_corpus_run` 与官方 `.out` 的逐字节比对。手写 fixture 与「native == 解释器」
都不够。

---

## 6. 白名单 10 条（不承诺）

见 `M9-manifest.json` 的 `unsupported`（每条带 `evidence`）。要点：

| id | 不承诺 | 形态 |
|---|---|---|
| `U-M9-01` | GC / 回收 | arena 只增，撞 `RGOC_HEAP_MAX` ⇒ `runtime error: out of memory (heap limit N bytes)` + 退 2 |
| `U-M9-02` | size-class 分配器 | 只有 bump |
| `U-M9-03` | 寄存器分配 / spill | 每值一槽（慢 2–3×，无性能门禁）|
| `U-M9-04` | 反射 | `TypeDesc` 只有 `name_hash` |
| `U-M9-05` | 泛型运行期表示 | 全量单态化；`HIR-UNSUPPORTED-GENERIC` + 退 1 |
| `U-M9-06` | traceback / pclntab | panic 只有一行 |
| `U-M9-07` | native 的 `go`/`select` | **守卫测试**（源码里不许出现 `rt_go`/`rt_select`）|
| `U-M9-08` | map 渐进搬迁 | 一次 rehash |
| `U-M9-09` | open-coded defer | 全局链（每次 defer 一次堆分配）|
| `U-M9-10` | interior pointer metadata | **语义级拒绝**（§1.1①）|

---

## 7. 与 M5–M8 的边界

| 里程碑 | 是什么 | 与 M9 的接缝 |
|---|---|---|
| M5 | HIR + 顺序解释器（`--via=hir`） | M9 **不改** HIR 语义；只在 `rgoc-interp` 里补对象模型与 nil 语义，两条通路（hir/ssa）共用一份 `Value` |
| M6 | SSA（图 + verifier + 三 pass + `SSA-LIVE v1`） | M9 加 6 个 `Instr`，**没有**新不变式（`V1/V3/V5` 已覆盖）；`SSA-LIVE` 的 `ptrs` 语义放宽（`R-9-31`），行语法不变 |
| M7 | native 代码生成（39 语料、`FRAME-MAP v1`） | M9 在同一套 isel 上补对象/接口/闭包/异常；`FRAME-MAP` 一字不改，只是 `ptrs` 的含义跟着放宽 |
| M8 | 并发解释执行（goroutine/channel/select，`C11`） | M9 **不接** HIR 的并发节点（SSA 侧继续拒 `SSA-UNSUPPORTED-CHAN`）；只交 `hchan` 的**布局与状态机影子**，执行归 M11 |

**一条硬边界**：native 侧没有 goroutine —— `chan` 语料在 native 上就是 `M'`
（`run-native.sh` 的 69 里**不含** chan 文件）。

---

## 8. 冻结与不承诺

**冻结**（改一个就要重跑全量对账）：

| 项 | 在哪 |
|---|---|
| `TypeDesc` 40 B 逐字段偏移 | `C5` §11.1 + `desc_emit`/`desc_layout` |
| `MethodTable`/`MethodEntry`/`ITab`/`GlobalRoot`/`RtFuncInfo` | `C5` §11.2–11.4 |
| `STACK-MAP v1` 行语法 + 位序（bit `i` ↔ `fp-frame_size+8i`）| `C5` §8.4 |
| `hchan` 96 B / `Sudog` 48 B 的字段序 | `C11` §7 + `chan_contract::t_sch_01`（**文档驱动**）|
| `FRAME-MAP v1`（M7 冻结，M9 未改）| `C5` §8 |
| `SSA-LIVE v1`（M6 冻结，M9 只放宽 `ptrs` 的含义）| `M6-design.md` §3.7 |

**不冻结**（明确留了口子）：贪心式 `frame` 分配（`T-NAT-48` 只要求**确定性**）、
`SsaCode` 的集合（`FUNC-VALUE` 已在 Phase 4c 下线）。

---

## 9. 接手指南

1. **先跑**：`run-native.sh`（69）→ `cargo test --workspace` → `mutation-m9.sh`。
   三样都绿才算「当前是好的」。
2. **改 IR 之前**读 `C4` §10：新增 `Instr` 大概率**不需要**新不变式，但要补
   `typing.rs` 的类型校验（`V3` 的实现）。
3. **改帧之前**读 `C5` §7 + `frame.rs` 的三条纪律（偏移一律负、按 `ValueId` 升序、
   零大小值有偏移无空间）；改动后 `native_frame_map` 会立刻红（它拿 `objdump` 对账）。
4. **改根协议之前**读 `C5` §8.4 + `stackmap.rs`：两份产物（文本 / `.rodata`）必须
   逐字节一致，位序写死。**别只改一侧**。
5. **加语料的姿势**：先看它落在哪个 Phase 的面；`run-native.sh` 的 `bad` 必须为 0，
   且新文件要进 `run-selected.txt`（分类器会数分母）。
6. **写判据的姿势**：先问「实现错的时候这条会不会红」。M9 有三条判据没通过这个
   自问（`G-M9-04..06`），它们的绿是**假的绿**。

---

## 10. 变更与修订索引

M9 期间的技术修订（`R-9-*`）全部记在 `.superpowers/sdd/M9-plan/progress.md` 的
对应 Phase 段（该目录不入库），与契约相关的六条同步进了 `C4`/`C5`/`C9`/`C11`
的变更记录。逐条索引见 `M9-manifest.json` 的 `measurement_deviations` 与
`open_gaps`。
