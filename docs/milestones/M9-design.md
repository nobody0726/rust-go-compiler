# M9 设计：native 对象模型、接口与 metadata

> **状态**：🟡 **草案**（2026-10-25）—— 决策 `D-M9-1..16` / 事实基线 `F1..F16` / 门禁 `MEM-1..SCH-1`（十条）
> **权威范围**：`docs/03-roadmap.md` §M9（本文件只做**展开与决策**，不改范围）
> **上游**：`M7-report.md`（native MVP 的边界与十九条真错）、`M6-design.md`（SSA 形状）、
> `M8-report.md`（并发语义基线）、`C4-ssa.md` / `C5-abi.md` / `C9-values-and-output.md` / `C11-concurrency.md`
> **下游**：`M9-tests.md`（判据）、`M9-plan.md`（任务分解）、`M9-manifest.json`（机器可读事实）、
> **M10（精确 STW GC）直接依赖本文件的 `D-M9-1/2/13`**
>
> **四条已签字**（§2.4）：门禁 = 语料 **69** / 实现深度 = 行为对 + 接口留后门 /
> 推进 = 语料驱动按特性纵向 / panic = 返回码逐层传播。

---

## 0. 本文档在流程中的位置

```text
03-roadmap §M9（做什么）→ 本文档（怎么定、边界在哪）→ M9-tests.md（怎么算通过）→ M9-plan.md（按什么顺序做）
```

三件套齐备才开工（`03` §6.2）。M9 与前八个里程碑的**根本差别**：

* M0–M8 都是「新增能力 + 冻结语义」；M9 是**第一次给对象造出运行期表示**。
* M9 的产出**大部分不是给当前用**，而是给 M10（GC）与 M11（native 并发）**留下可消费的
  协议**。所以本文件里「格式」比「算法」重要：分配器可以重写，`TypeDesc` 的字段集不能。
* M9 的算法**刻意简化**（`D-M9-16`）：map 不做渐进搬迁、defer 不做 open-coded、
  itab 不做运行期哈希缓存。这三处都是「行为对但结构不同」，M11/M12 换实现时调用点不用改。

---

## 1. 目标与非目标

### 1.1 输入与输出

| 项 | 内容 |
|---|---|
| **输入** | M7 的 native 通路（`rgoc build` → `.s` → clang → ELF）；M6 的 SSA（14 `Instr` + 5 `Term`）；M5 的解释器（值语义的**参考实现**，69/69 逐字节） |
| **输出** | native 能跑**复合类型 + 接口 + 闭包 + 异常**的 ELF；一张给 M10 消费的**根协议**（`TypeDesc` + `STACK-MAP`）；`M9d` 的 channel 契约（不跑） |
| **成功判据** | 语料 **69/69**（`D-M9-15`）；十条门禁全 `pass`；`M5/M6/M7/M8` 零回归 |

### 1.2 范围（`03-roadmap.md` §M9 逐句展开）

| roadmap 原句 | 本文件的落点 |
|---|---|
| M9a typed allocation、bump/arena、零值、统一分配入口、布局/对齐/指针 metadata、资源统计 | `D-M9-1/2/3/4` + §3.2–3.4；门禁 `MEM-1/2` |
| M9b string/slice/map、闭包/方法值、iface/eface、接口调用/断言/type switch | `D-M9-7/8/9/10` + §3.6–3.9；门禁 `OBJ-1..4` |
| M9c1 native defer/panic/recover 和运行时检查 | `D-M9-11/12` + §3.10；门禁 `EXC-1` |
| M9c2 全局 roots、调用/回边安全点、stack maps、寄存器 spill/save roots、宿主桥接暂停策略 | `D-M9-13` + §3.11；门禁 `ROOT-1`。**「寄存器 spill」在本实现里不存在**，理由见 §3.11 |
| M9d channel 对象/等待队列和调度契约 | `D-M9-14` + §3.12；门禁 `SCH-1` |
| 保守正确性策略：不优化逃逸，可能逃逸的对象放 managed heap | `D-M9-4`：**值语义聚合留栈**，其余一律进 arena（见 §3.4 的三条边界） |
| 「不把『仍无 GC』当作 RT-MEM-01/02 多级分配器完成」 | `RT-MEM-01/02` 记 ✗；`D-M9-3` 明确不做 size-class |

### 1.3 明确不做（M9 边界）

| 不做 | 归谁 | 后果（使用者会看到什么） |
|---|---|---|
| **GC 本体**（标记、清扫、写屏障） | M10 | arena 只增不减；长程序最终撞 `heap limit` 退 2 |
| **size-class 多级分配器**（`RT-MEM-01/02`） | M10 | 只有 bump 分配；碎片不管 |
| **寄存器分配与 spill**（`PIPE-20`） | M11 | 每值一个栈槽（沿用 M7） |
| **native 的 `go` / `select` 执行** | M11 | `M9d` 只交结构与契约 |
| **抢占与安全点打断**（`RT-SCH-07`） | M11 | 单 goroutine，没有「被打断」这回事 |
| **反射**（`RT-POLY-06`：字段名/Tag/方法名） | M12 | `TypeDesc` 里没有名字；`reflect` 相关内建继续拒 |
| **完整 SwissTable 与渐进搬迁**（`RT-CMP-03`） | M11 或永不 | map 一次 rehash；大量插入时卡顿可测 |
| **open-coded defer**（`RT-EXC-01`） | M11 或永不 | defer 走堆链；每帧一条 `rt_deferrun` |
| **traceback / pclntab**（`RT-EXC-05`） | M12 | panic 只有一行 `panic: <值>`，不打栈（沿 `U-M8-08`） |
| **泛型运行期表示**（`RT-POLY-05`） | M12 | 继续全量单态化（M5 的窄面） |

### 1.4 退出条件（门禁十条）

| 门禁 | 一句话判据 | 主证据 |
|---|---|---|
| `MEM-1` | typed 分配 / 零值 / 对齐 / 超限退 2 | 分配 fixture + `heap limit` fixture |
| `MEM-2` | 逐类型 `size` + `ptrmask` 与解释器布局**对撞** | `desc_layout` 对撞（69 个语料的类型闭包） |
| `OBJ-1` | slice 全套（append 扩容 / 越界 / nil） | `obj_slice` + 语料里的 7 个 |
| `OBJ-2` | map 全套（迭代中 delete / 键相等 / comma-ok） | `obj_map` + 语料里的 5 个 |
| `OBJ-3` | iface（装箱 / 断言 / type switch / 方法调用） | `obj_iface` + 语料里的 9 个 |
| `OBJ-4` | funcval（闭包 / 方法值 / 捕获写回） | `obj_funcval` + 语料里的 7 个 |
| `EXC-1` | defer 逆序 / panic 逐层 / recover 仅 defer 内有效 / 除零越界 nil | `exc_native` + 语料里的 2 个 |
| `ROOT-1` | `STACK-MAP` 与 `live` 一致 + 全局 roots + 回边有 safepoint | `stackmap` 对账（产物 vs `.rodata`） |
| `CORP-1` | 语料 **69 全绿**（39 零回归 + 逐个解锁，差的记 `M'` 并写理由） | `run-native.sh` |
| `SCH-1` | `hchan` 布局 + 三操作真值表单测 | `chan_contract`（Rust 单测） |

---

## 2. 已确认决策记录

### 2.1 决策一览

| # | 决策 | 一句话 |
|---|---|---|
| `D-M9-1` | 对象头 | 所有堆对象带 **8 字节 `*TypeDesc`**，data 从 +8 开始 |
| `D-M9-2` | `TypeDesc` 字段集 | `{size, ptrdata, kind, flags, hash, gcdata, elem, methods}` = **40 B**（4+4+4+1+1+2pad+8×3），静态表 |
| `D-M9-3` | 分配器 | bump arena，16 MiB 起、**可增长**（mmap chunk 链），上限 256 MiB；**不做 size-class** |
| `D-M9-4` | 逃逸策略 | 值语义聚合留栈；取址且逃出 → arena；**新对象一律 arena**；不做逃逸优化 |
| `D-M9-5` | IR 扩展面 | **净增 3 个 `Instr`**（`FuncAddr` / `MakeClosure` / `Defer`），`Term` 不变 |
| `D-M9-6` | rt 调用约定 | `rt_*` 的**值结果一律 sret**（第 0 参是结果槽），标量走寄存器 |
| `D-M9-7` | slice | `{ptr,len,cap}` 3 字；`rt_makeslice` / `rt_growslice` |
| `D-M9-8` | map | `*MapObj` 单字，开放寻址 + 线性探测；`rt_hash` / `rt_equal` 按 `TypeDesc` 递归 |
| `D-M9-9` | iface | `{tab,data}` / `{type,data}` 各 2 字；itab **静态生成 + 运行期兜底** |
| `D-M9-10` | funcval | `{fn, captures...}` 堆上变长；**funcval 指针作隐式第 0 实参** |
| `D-M9-11` | defer | **全局链 + `frame_fp` 归属**；正常返回时 `rt_deferrun(cur_fp)` |
| `D-M9-12` | panic | **返回码逐层传播**（每调用点后注入检查）；**不用** unwinding / frame map |
| `D-M9-13` | 根协议 | safepoint 加**回边**；stack map 按 `gcdata` 到**字**；**没有 spill**（M7 无寄存器分配） |
| `D-M9-14` | channel 契约 | `hchan` 对齐官方字段序，只交布局 + Rust 单测（M11 才执行） |
| `D-M9-15` | 语料门禁 | **69**：39 零回归 + 逐个解锁；撞到别的墙的记 `M'` 并写理由 |
| `D-M9-16` | 实现深度 | **行为对 + 接口留后门**：map 无渐进搬迁 / defer 无 open-coded / itab 无哈希缓存 |

### 2.2 五字段（只对**非显然**的六条展开）

#### `D-M9-1` / `D-M9-2` 对象头与 `TypeDesc` 的字段集

| 字段 | 内容 |
|---|---|
| **背景** | M10 的扫描器拿到一个对象地址后，必须能回答两件事：这个对象多大、哪些字是指针。回答不了就只能保守扫描（把每个字当指针），而保守扫描会把整数当指针 → 假保留 + 可能的非法地址 |
| **选项** | ① 8 B `*TypeDesc` 头，desc 里带 `gcdata` 位图；② 4 B kind-id 头 + 一张全局 `kind → desc` 表；③ 无头，分配点静态记账（`rt_alloc(desc,…)` 记下区间），扫描时按地址查区间树 |
| **决定** | ① |
| **理由** | ② 的 4 字节省不了多少（对齐后仍是 8），却多一次间接查表；③ 的区间树要维护「按地址排序的分配记录」，每次分配 O(log n)、每次扫描前 O(n) 建表，且**对象搬迁（M10 若做压缩）就全废**。① 一次 `load` 拿 desc，且 `desc` 指针本身就是「对象身份」—— 接口断言 / `==` / map 的键 hash 都要它 |
| **失效条件** | 若 `TypeDesc` 的静态表让 `.rodata` 超过 256 KiB（69 个语料的类型闭包应当远小于），说明字段集塞了不该塞的东西（比如名字/方法表内联）——那时把 `methods` 改成二级指针（已经是）并检查有没有别的内联数组 |

| 字段 | 内容 |
|---|---|
| **背景** | 字段集是**放射性**的：M10 按它扫，M12 按它做反射，接口按它判等 |
| **选项** | ① 只放 M10 要的（size/ptrdata/kind/hash/gcdata/elem + methods 指针）；② 放全（含 `name`/`fields`/`methods[]` 内联）；③ 按 kind 分 24 张表（`ArrayType`/`ChanType`/…，官方就是这么干的） |
| **决定** | ① |
| **理由** | ② 让每个 desc 从 40 B 涨到几百 B，`.rodata` 爆炸而 M9 用不到。③ 是官方的做法，但它的驱动是**反射**（`reflect.TypeOf` 要按 kind 转型），M9 不做反射。① 的 `elem` 字段承担了「递推」：`*T` / `[]T` / `[n]T` / `map[K]V` 的 ptrmask 都能靠 `elem` 递归算出来，不需要 per-kind 表 |
| **失效条件** | M12 做反射时必然要扩字段集 —— 那是**追加**（`TypeDesc` 尾部加 `name`/`fields`），不是重排；若发现某字段的语义要改（如 `hash` 从 u32 变 u64），说明 M9 定错了，必须走契约修订 |

#### `D-M9-5` IR 扩展面 = 净增 3 个变体

| 字段 | 内容 |
|---|---|
| **背景** | map / iface / funcval / defer 都要「做点什么」。差别在于：做的事**是否是 IR 层可验证的语义** |
| **选项** | ① 只给 funcval 与 defer 加变体，map/iface 全走 `Call` 到 `rt_*`；② 全部变体化（`MapGet`/`MapSet`/`IfaceBox`/`IfaceAssert`…，Instr 从 14 到 24）；③ 一个也不加 |
| **决定** | ① |
| **理由** | map 的语义**全在 runtime**（哈希、探测、扩容），IR 层没有可优化也没有可验证的东西 —— 加变体只是把 `Call(Builtin(MapGet))` 换个写法，却让 C4 与 C10 的冻结面翻倍。iface 的装箱/断言同理。而 **funcval 的两步（取地址、装闭包）不是调用**（`FuncAddr` 没有 callee，`MakeClosure` 分配内存但不调用户代码），**defer 是控制流敏感的**（return 时要跑）—— 这两类落成 `Call` 就让 verifier 看不见「谁在什么时候跑」。③ 的后果具体是：defer 变成一个「隐形的运行期状态」，而它的顺序错乱只会在运行时表现为输出顺序不对 |
| **失效条件** | 若 map 的实现需要 IR 层的类型区分（例如 `rt_map_set` 的实参顺序依赖 key/value 的槽宽），那时加一个 `MapSet` 变体；**但必须是追加**，不许重排既有变体 |

#### `D-M9-9` itab：静态生成 + 运行期兜底

| 字段 | 内容 |
|---|---|
| **背景** | 方法调用要 `itab.slot[i]`。`(I, T)` 对在**赋值点**是已知的（`var i I = t`），但**断言到接口** `x.(J)` 的 `(J, T)` 对枚举不出来（`T` 是 `x` 的静态类型的所有可能实现） |
| **选项** | ① 全静态（每个 `(I,T)` 对一张 `.rodata` 表）；② 全运行期（`rt_itab` 现算 + 哈希缓存，官方）；③ **静态优先 + 运行期兜底**：赋值点用静态表，`x.(J)` 走 `rt_itab` |
| **决定** | ③ |
| **理由** | ① 在 `x.(J)` 上**根本不可行**（枚举不出来）。② 是官方做法，但要一张 `inter×concrete` 的哈希缓存表 + `getitab` 的锁 —— 而我们没有并发，缓存的收益只有「同一个断言跑多次」，M9 的语料里最多几百次。③ 让常见路径（赋值 + 调用）零运行期开销，罕见路径（断言到接口）走一次 `rt_itab`，两者共用同一张 `MethodTable` |
| **失效条件** | 若 `rt_itab` 在语料上被调用的次数超过 10⁵（可以加计数器测），说明静态表覆盖得太少 —— 那时把「断言点能枚举的 `(J,T)` 对」也预生成 |

#### `D-M9-11` defer：全局链 + `frame_fp` 归属

| 字段 | 内容 |
|---|---|
| **背景** | defer 记录必须能被两件事找到：① 本函数**正常返回**时（要跑自己注册的那些）；② panic 展开时（要按帧逆序跑） |
| **选项** | ① 每帧一个 `defer_head` 槽（官方 `g._defer` 链按帧串）；② **一条全局链**，每条记录存注册时的 `fp`；③ open-coded（≤8 个用位掩码 + 栈数组，多的进堆） |
| **决定** | ② |
| **理由** | ① 要求「帧里预留 8 B + 每个函数都要初始化它」，而且 panic 展开时仍然要知道「当前 fp 对应哪个 defer_head 偏移」—— 那需要帧描述进 `.rodata`（`F7`：`.framemap` 是并列产物，运行期读不到）。② 把「找 defer」变成「扫全局链并比较 `fp`」，**零帧元数据**。③ 是官方做法，工作量翻倍（三级方案），行为完全相同 |
| **代价** | 全局链的每条记录多 8 B（存 `fp`）；`rt_deferrun(cur_fp)` 是 O(链长) |
| **失效条件** | 若某个函数注册的 defer 数超过 10⁴（可以测），O(链长) 的 `deferrun` 会退化 —— 那时给链加「按 fp 分桶」 |

#### `D-M9-12` panic = 返回码逐层传播

| 字段 | 内容 |
|---|---|
| **背景** | native 是 freestanding、没有 Rust/系统 unwind；`.framemap` 是并列产物**不在 ELF 里**（`F7`），所以「按 PC 查帧描述」在运行期走不通。而 panic 必须**逐层跑 defer** 才能退出 |
| **选项** | ① **返回码传播**：`rt_panic` 设状态后返回，每个调用点后注入 `if panicking goto epilogue`；② 帧回溯：per-frame 的 defer 链 + 帧描述编进 `.rodata` + recover 靠 continuation（freestanding 无 `setjmp`）；③ 自写 `setjmp/longjmp` 直接跳到最外层 |
| **决定** | ① |
| **理由** | ③ **丢 defer**（longjmp 跳过 epilogue）⇒ 语义错。② 是官方同构，但要新建「帧描述进 `.rodata`」+「PC → 函数」的查找（pclntab），且 recover 需要 continuation 机制 —— 工作量约 ① 的两倍。① 的代价是每个调用点多一条检查 + 一个分支（代码体积 ~+10%），换来**零帧元数据、零 unwinding 机制**，且 defer 顺序天然正确（每层跑自己的） |
| **代价** | ① 的检查必须**真的每个调用点都有**：漏一处 = panic 被吞，症状是「程序继续跑下去」（极隐蔽）。所以 verifier 要加一条：**每个 `Term` 可达的 `Call` 之后必须有一条 `panicking` 检查**（新不变式 `V10`） |
| **失效条件** | 若代码体积实测增长超过 25%（用 `.text` 字节数测），说明检查太重 —— 那时改成「只在会分配/会调用户函数的调用点后检查」 |

#### `D-M9-13` 根协议

| 字段 | 内容 |
|---|---|
| **背景** | M10 的 STW GC 要答两个问题：停在哪（safepoint）、每个停点上哪些字是根（stack map） |
| **选项** | ① 精确：safepoint = 调用 + 分配 + **回边**，stack map 按 `gcdata` 精确到字，全局 roots 静态表；② 保守：栈上每个字都当指针；③ 精确但只到「值」粒度（沿用 M7 的 `live.ptrs`：值是 `*T` 才算根） |
| **决定** | ① |
| **理由** | ② 会把整数当指针（假保留 + 可能解引用非法地址），且 **M10 的「可检查」门禁无法满足**（保守扫描枚举不出根集）。③ 会**漏根**：`var s []int` 的 `ptr` 字是根，但 `s` 的类型是 slice 不是 `*T` —— 漏根的后果是 M10 把活对象回收掉，症状是「随机崩」 |
| **红利** | **不需要「寄存器 spill/save roots」**：M7 不做寄存器分配、**每个 `ValueId` 恒有一个栈槽**（`F4`），寄存器里装的只是那个槽的**副本**；而 safepoint 的活值集恰好是 `ValueId` 集（`F5`），所以扫槽就覆盖全部根。调用实参也一样：进寄存器的那些仍然在各自的槽里，进栈的那些落在 `Frame::outgoing`（也在帧内）。roadmap 写「寄存器 spill/save roots」是按官方有寄存器分配写的，本实现不适用 |
| **失效条件** | M11 引入寄存器分配时，本条的「红利」失效 —— 那时必须补 spill 区，且 safepoint 上的活寄存器要先存到 spill 槽。**这是 M9 冻结给 M11 的一条已知接口变更点** |

### 2.3 事实基线（`F1..F16`，全部实测或源码指认）

| # | 事实 | 出处 |
|---|---|---|
| `F1` | SSA IR 现有 **14 个 `Instr` + 5 个 `Term`**（`Goto/If/Return/Panic/Unreachable`） | `rgoc-ssa/src/ir.rs:336` / `pub enum Term` |
| `F2` | `CalleeRef::{Func, Method, Builtin, Indirect}` **已有 `Indirect(ValueId)`** | `ir.rs:318` |
| `F3` | `Layout { size, align }` **没有 ptrmask**；`layout()` 对泛型 / `Tuple` / `[...]T` 返回 `None` | `rgoc-types/src/layout.rs:51,71` |
| `F4` | `Frame { values: BTreeMap<ValueId, Slot>, addr_slots, heap: BTreeMap<SlotId, HeapSlot>, tmp_base, outgoing, tmp_slots, frame_size }` —— **每值一个栈槽，无寄存器分配** | `rgoc-arm64/src/frame.rs:65` |
| `F5` | `live::Safepoint { block, idx, live, ptrs }`；`is_safepoint` 只认 `Call` 与 `Make`（**没有回边**） | `rgoc-ssa/src/live.rs:105,302` |
| `F6` | `Safepoint::ptrs` 的判据是 `is_ptr_ty`（值的类型是 `*T` 才算），**不含** slice/string 的 `ptr` 字 | `live.rs:281` |
| `F7` | `FRAME-MAP v1` 是**并列产物** `<out>.framemap`（不在 ELF 里）；原因是 `#` 在 `.s` 里是立即数前缀 | `rgoc-arm64/src/module.rs:107`；M7 报告 §2 |
| `F8` | `rt.c` **815 行 / 24 个符号**，freestanding 零 libc，`svc #0` 直调 | `runtime/native/aarch64-unknown-linux-gnu/rt.c` |
| `F9` | arena 是**静态 8 MiB**；`turing.go`（几千次字符串拼接）曾把 1 MiB 耗尽 | `M7-manifest.json` `budget.arena_bytes` |
| `F10` | 语料现状：**46 = 39 绿 + 7 拒（全 `CG-UNSUPPORTED-SLICE`）**；另有 **23 = IFACE-DYN 9 / FUNC-VALUE 7 / MAP 5 / DEFER 2** 在 SSA 层建不出图 | `native-rejected.txt` / `ssa-rejected.txt` |
| `F11` | `SsaCode::ALL` 现 **8 条**（M8 下线 `RANGE-FUNC`）；`RejectKind::ALL` 现 **5 条** | `rgoc-ssa/src/diag.rs:49` |
| `F12` | 解释器在 **69/69** 上逐字节（M5 冻结的值语义基准） | `run-collide.sh --run` |
| `F13` | `string` 已按 **16 B 两字**标量处理（`slot_of` 的 `round_up_8(size).max(8)`） | `rgoc-arm64/src/abi.rs:77` |
| `F14` | `Instr::Make` 的语义三分：`len=cap=None` 且 `typ` 为指针 ⇒ `new`；否则 `make` | `live.rs` 的 `is_safepoint` 注释 |
| `F15` | `FRAME-MAP` 与 `objdump -d` **双向对账**（`native_frame_map`）—— 这是 M9 的 `STACK-MAP` 对账的先例 | `rgoc-driver/tests/native_frame_map.rs` |
| `F16` | `dom.rs` 已有支配关系（`V2` 的消费者），回边判定不需要新算法 | `rgoc-ssa/src/dom.rs` |

### 2.4 四条签字（2026-10-25 已确认）

| # | 问题 | 决定 |
|---|---|---|
| `Q1` | 一次做完还是先切一段 | **一次到底**：design + tests + plan 全覆盖 M9a–M9d（风险：c1/c2/d 的判据建立在还没写的代码上，由 `M9-plan.md` 的 Phase 边界「按实测重写」条款兜住） |
| `Q2` | 语料门禁 | **69**（39 零回归 + 23 逐个解锁；撞其他墙的记 `M'` 并写理由） |
| `Q3` | 实现深度 | **行为对 + 接口留后门**（`D-M9-16`） |
| `Q4` | 推进顺序 | **语料驱动、按特性纵向**：每个 Phase 交一个数字 |

---

## 3. 架构与数据模型

### 3.1 crate 与模块变化

| crate | 变化 | 说明 |
|---|---|---|
| `rgoc-types` | `layout` 扩 `ptr_mask()`：一个类型的 ptrmask 位图（递归，靠 `elem`） | `Layout` 本身**不改**（仍是 size/align）；ptrmask 是独立函数，避免每次布局都算位图 |
| `rgoc-ssa` | `ir.rs` 加 3 个 `Instr`；`live.rs` 加回边 safepoint + `ptrs` 改按 gcdata；`verify.rs` 加 `V10`（panic 检查）；新增 pass `panic_check.rs`（注入检查块）；`build.rs` 拆掉 5 处 `SSA-UNSUPPORTED-*` | 建图面是 M9b 的一半工作量 |
| `rgoc-arm64` | `abi.rs` 加「funcval 隐式第 0 实参」；`isel.rs` 加 3 个新变体的选择 + slice/map 内建的调用序列；`module.rs` 加 `TypeDesc` 与 `STACK-MAP` 的 `.rodata` 发射 | 最大的单点 |
| `rgoc-driver` | `build` 子命令多产物：`.stackmap`（与 `.framemap` 并列）；`run-native.sh` 扩语料 | 装置 |
| **`rgoc-meta`（新 crate）** | `TypeDesc` 的 Rust 侧表示 + ptrmask 计算 + 与方法表的生成 | 让「元数据怎么算」有一处唯一实现，isel 与 dump 都读它 |
| `runtime/native/.../rt.c` | 815 行 → 估 **~2200 行**（分配器 / map / hash / equal / itab / defer / panic / 检查桩） | **仍是 freestanding、零 libc** |

> 为什么新开 `rgoc-meta` 而不是塞进 `rgoc-arm64`：`TypeDesc` 是**给 M10 消费的协议**，
> 而 M10 的扫描器不依赖 aarch64（它读的是内存里的表）。放进 `rgoc-arm64` 等于把协议
> 绑在指令集上。

### 3.2 对象头与 `TypeDesc`（`D-M9-1/2`）

```text
堆对象的统一形状（8 字节对齐）：
  [ +0  ] desc: *TypeDesc     ← 所有堆对象都有
  [ +8  ] data...             ← 零值已按 desc.needs_zero 清过
```

```rust
// rgoc-meta
pub struct TypeDesc {
    pub size: u32,      // data 部分字节数（不含头）
    pub ptrdata: u32,   // [0, ptrdata) 前缀里可能有指针（gcdata 覆盖这段）
    pub hash: u32,      // 类型 hash（接口 == / map 键）
    pub kind: u8,       // 与 reflect.Kind 对齐的子集（24 种里取 16 种）
    pub flags: u8,      // bit0 comparable / bit1 has_pointers / bit2 needs_zero
    pub gcdata: *const u8,   // ptrmask，每 8 字节 1 位；ptrdata==0 ⇒ 空
    pub elem: *const TypeDesc,   // *T / []T / [n]T 的元素；其余为 null
    pub methods: *const MethodTable,  // 运行期 itab 用；无方法为 null
}
```
**40 B**（`size_of`/`offset_of` 断言钉住 —— 它是 `C5` §10 冻结的内存布局，不是 Rust 结构体的自然大小）。
**不发名字、不发字段表** —— 那是 M12。

`MethodTable` = `{ count: u32, pad: u32, entries: [MethodEntry; count] }`，
`MethodEntry` = `{ name_hash: u32, sig_hash: u32, fn: *const u8 }`。
名字的**哈希**够用来做 itab 匹配（`rt_itab` 靠它）；真名字留 M12。

**零值**：`flags.needs_zero` 为真时 `rt_alloc` 负责清零。哪些类型需要清零 ——
含指针或含浮点的（`0.0` 的位模式不是全零）；全整数/布尔的聚合不需要。
这条**由 `rgoc-meta` 算**，不是手写清单。

### 3.3 分配器（`D-M9-3`）

```text
rt_alloc(desc, need_zero) -> *u8
  size = desc.size + 8
  若当前 chunk 剩余 < size：rt_grow(size)     // mmap 新 chunk，挂到链上
  p = bump; bump += align_up(size, 8)
  *p = desc; 若 need_zero { memset(p+8, 0, desc.size) }
  return p
```

| 参数 | 值 | 理由 |
|---|---|---|
| 初始 chunk | 16 MiB | M7 的 8 MiB 在 `turing.go` 上勉强够；复合类型一上来用量翻倍 |
| 增长粒度 | `max(16 MiB, 请求大小向上取整)` | 大对象要能一次装下 |
| 上限 | 256 MiB（`RGOC_HEAP_MAX` 可覆盖） | 门禁要「unbounded arena 可触发明确错误」 |
| 超限 | 退出码 **2** + `runtime error: out of memory (heap limit N bytes)` | 与 `C9` §6 的资源超限同形 |
| 不做 | size-class / 空闲链表 / 复用 | M10 的 GC 一到就要重写（`RT-MEM-01/02` 记 ✗） |

**资源统计**：`rt_heap_used()` / `rt_heap_limit()` 两个符号（`RT-MEM-08` 的 ◐ 兑现：
只报数字，不做 `HeapAlloc/HeapObjects` 的细分）。

### 3.4 布局、ptrmask 与逃逸（`D-M9-4`）

`ptr_mask(tt, t) -> Option<Vec<bool>>`：每 8 字节一格，`true` ⇒ 这个字是指针。

| 类型 | ptrmask | 说明 |
|---|---|---|
| `*T` / `unsafe.Pointer` / `map` / `chan` / `funcval` | `[true]` | 单字指针 |
| `string` | `[true, false]` | 字 0 是数据指针，字 1 是 len |
| `slice` | `[true, false, false]` | 字 0 是数据指针 |
| `iface` / `eface` | `[true, true]` | 字 0 是元数据指针（`ITab`/`TypeDesc`，**不在 Go 堆上**，但扫描时要跳进去）；字 1 是 data |
| 结构体 | 逐字段拼接（按字段偏移） | |
| 数组 | `elem` 的位图重复 `n` 次 | |
| 标量（int/float/bool） | `[false]` | |

> `iface` 的字 0 是元数据指针：M10 扫到它时**不能**把它当一个 Go 对象去追（它不是堆对象），
> 而要**经它找到字 1 的 desc**。这条写进 `C11`/M10 的扫描器契约。本里程碑只产出位图，
> 位图里「字 0 = true」表示「这里有一个需要特殊处理的指针」。

**逃逸的三条边界**（`D-M9-4`，替代 roadmap 的「全部放 managed heap」这句笼统话）：

1. **值语义聚合留栈**：`[n]T` / `struct` 的局部变量、形参、返回值都在栈槽（M7 已如此）。
   它们是**值**，不是对象 —— 放堆会破坏值语义（`a := b` 要深拷贝）。
2. **取址且逃出 → arena**：`&x` 且 `x` 的地址逃出本函数（return / 传参 / 存全局 / 存堆）
   ⇒ `x` 的存储在 arena（`Frame::heap` + `R-13-5` 已有）。判定沿用 M6 的 `ptr_root`。
3. **新对象一律 arena**：`make` / `new` / 复合字面量取址 / 接口装箱 / 闭包分配。
   **不做逃逸优化**（roadmap 明写）：即使「这个 `[]int` 没逃出去」也进 arena。

### 3.5 IR 扩展（`D-M9-5`）

```rust
// rgoc-ssa/src/ir.rs 追加（不改既有 14 个）
FuncAddr { dst: ValueId, func: FuncId },                      // 函数地址常量（1 字）
MakeClosure { dst: ValueId, func: FuncId, captures: Vec<ValueId> },  // 分配 funcval
Defer { call: Box<Instr> },                                   // 必须是 Call（verifier 查）
```

**verifier 的新不变式**：

| # | 不变式 | 抓什么 |
|---|---|---|
| `V10` | 每个「`Term` 可达的 `Call`」之后，同一块内必须有一条 `panicking` 检查（`D-M9-12`） | 漏注入 ⇒ panic 被吞 |
| `V11` | `Defer.call` 必须是 `Instr::Call`（不许嵌套 defer / 不许 defer 一个非调用） | defer 的展开靠统一形状 |
| `V12` | `MakeClosure.func` 的 `captured` 数量 == `captures.len()`（且类型逐个 identical） | 捕获错位（M7 的 `string` 丢 len 同类错） |

**C4 / C10 怎么改**（`D-M9-5` 的连带）：

* `C4` 加 §10「M9 的三个新变体」：签名 + verifier 不变式 + 为什么这三个是变体而 map 不是。
* `C10` 的 dump 语法**追加**三行 `op` 拼写（`funcaddr` / `makeclosure` / `defer`），
  既有 golden **一字不动**。这是 `R-51-1` 的教训：C10 的示例是文档驱动 golden 的源头，
  改一个字就要重跑一遍全量对账。

**执行期的偏差（Phase 3 起，收口时补记）**：上面三行是**计划**，实际追加了六条 ——
`TypeAssert`（Phase 3）、`CapturePtr`（Phase 4b）、`NilCheck`（Phase 5c）也在其中，
且 `Defer` 的实现是 `Defer { fv: ValueId }`（延迟调用的实参在**登记时**就打包进一个
合成的包装函数，`synth_defer_wrapper`），不是 `Defer { call: Box<Instr> }`。

| 变体 | 加进来的阶段 | 为什么不能复用既有 |
|---|---|---|
| `TypeAssert` | Phase 3 | comma-ok 形态有**两个**结果，`Convert` 只有一个；失败要么 panic 要么写 `ok` |
| `CapturePtr` | Phase 4b | 捕获是**按引用**的（`R-9-15`）：闭包要先从 funcval 里取出指向外层变量的指针 |
| `NilCheck` | Phase 5c（`R-9-21`） | `&*p` / `&p.f` **不读内存**却在 nil 上 panic，IR 里连指令都没有 ⇒ 这个「可能 panic 的点」无处可写 |

`C10` 的 §3 操作数表也按实现补登了这六条（见其变更记录 2026-10-26 行）。

### 3.6 slice（`D-M9-7`）

| 动作 | 落法 |
|---|---|
| `make([]T, n[, c])` | `Instr::Make` 已有 ⇒ isel 侧落 `rt_makeslice(elem_desc, n, c)` → 结果槽写 3 字 |
| `s[lo:hi]` / `s[lo:hi:max]` | `Instr::Slice` 已有 ⇒ isel 侧算 `ptr + lo*sizeof(T)`、`len = hi-lo`、`cap = max-lo`，**带越界检查** |
| `s[i]` / `s[i] = v` | `Instr::Index` 已有 ⇒ 已有 isel（M7 对数组实现过），扩到 slice 头取 `ptr` |
| `append(s, xs...)` | `rt_growslice(old, elem_desc, n) -> slice`（必要时新分配 + memcpy） |
| `len(s)` / `cap(s)` | isel 直接读字 1 / 字 2（M7 对 `string` 已这么做） |
| nil slice | `{0,0,0}`；`s == nil` 走「三字全零」比较 |

**越界检查**：`0 <= i < len` ⇒ 否则 `rt_panic_bounds(i, len)`（panic 走 `D-M9-12` 的返回码）。
判据来自 `C9` §5 的 `panic: runtime error: index out of range [i] with length n`（与解释器同文本）。

### 3.7 map（`D-M9-8`）

```text
MapObj（堆上，单字指针指向它）:
  [0]  count: u32      [4]  cap: u32        // cap 恒为 2^k
  [8]  key_size: u32   [12] val_size: u32
  [16] kdesc: *TypeDesc  [24] vdesc: *TypeDesc
  [32] slots: *u8        // cap 个 {state:u8, key[key_size], val[val_size]} 紧密排布
```

`state` ∈ `{empty, live, tomb}`。`hash = rt_hash(kdesc, key) & (cap-1)`，线性探测。

| 操作 | 落法 |
|---|---|
| `m[k]` / `m[k] = v` / `delete(m,k)` | `rt_map_get` / `rt_map_set` / `rt_map_del`（sret 写结果槽，返回 `bool`） |
| `len(m)` | 读 `count` |
| `v, ok := m[k]` | `rt_map_get` 的 `bool` 返回值 |
| `for k, v := range m` | `rt_map_iter(&it)`：迭代器 `{m, idx, epoch}`，**跳过 `tomb` 与 `empty`** |
| 负载因子 | 0.75 触发 rehash（一次全量；**不做渐进搬迁**） |

**键相等**：`rt_equal(kdesc, a, b) -> bool` 按 `kind` 递归（标量按宽度比较、`string` 比内容、
`iface` 比 `(tab,data)` 两者、结构体逐字段）。**map 的键必须是 comparable** —— 这条
检查器已经管（`C8`），M9 不重复。

**迭代中删除**（M8 在解释器里踩过一次）：规范要求「未到达而被删的条目不产出」。
我们的迭代器**按 `state` 判**（`live` 才产出），所以天然满足；「迭代中新增」可能被产出、
也可能不被（取决于是否落在已扫过的区间）—— 规范允许两者。

### 3.8 iface / itab（`D-M9-9`）

```text
iface  = { tab: *ITab, data: *u8 }    // 有方法的接口
eface  = { type: *TypeDesc, data: *u8 }  // interface{} / any
ITab   = { iface: *TypeDesc, concrete: *TypeDesc, fn: [n] *const u8 }  // .rodata 或运行期构造
```

| 动作 | 落法 |
|---|---|
| 装箱 `var i I = t` | `data = rt_box(desc, &t)`；`tab = <静态 ITab 符号>` |
| 装箱到 `any` | `data = rt_box(desc, &v)`；`type = desc`（标量也 box，与官方一致） |
| `i.M(args)` | `fn = itab.fn[k]`；调用 = `Indirect`，实参 = `[data, args...]` |
| `x.(T)`（具体类型） | iface：比 `tab.concrete == desc(T)`；eface：比 `type == desc(T)` |
| `x.(J)`（接口） | `rt_itab(desc(J), concrete) -> *ITab`（运行期构造 + **不缓存**，`D-M9-16`） |
| `switch x.(type)` | 逐个 case 的 `x.(T)`；`default` 是失败分支 |

**静态 ITab 的生成**：`rgoc-meta` 在编译期为每个**赋值点出现的 `(I, T)` 对**生成一个符号
`__itab_I_T`，`fn[]` 按 **`I` 的方法声明序**填（不是字典序 —— 字典序是接口类型自身的
方法表规约，`C7` §4）。`rt_itab` 用 `MethodTable` 的 `name_hash` 查同签名的方法。

### 3.9 funcval（`D-M9-10`）

```text
funcval = { fn: *code, captures... }      // 堆上变长（desc 的 size 按捕获数定）
```

* `Instr::FuncAddr` ⇒ `adrp/add` 取函数地址（1 字）。
* `Instr::MakeClosure` ⇒ `rt_alloc(closure_desc, need_zero=1)` + 填 `fn` + 逐个填捕获。
* **调用约定（C5 要加一节）**：funcval 指针作**隐式第 0 个实参**进 `x0`；被调方的
  「捕获形参」不占形参寄存器，而是从 `[x0 + 8 + 8*i]` 读。
  * 为什么不让捕获占形参寄存器：捕获个数是变长的，占寄存器会让 `abi::place_args`
    的整数/浮点分流表多一维；而「x0 是 ctx」是 AAPCS64 里 `rd`（间接结果）位置的自然延伸。
* 方法值 `x.M` = `MakeClosure{func: <方法体>, captures: [recv]}` —— 方法体的形参 0
  本来就是 receiver（`F2` 的 `CalleeRef::Method` 已如此），正好对齐。

### 3.10 异常：defer + panic（`D-M9-11/12`）

**defer 记录**（堆上，单链）：

```text
DeferRec = { next: *DeferRec, fp: *u8, fn: *code, args... }
```

| 时机 | 动作 |
|---|---|
| `Instr::Defer` | 实参照常在**当前帧**求值 → `rt_defer_push(fn, args…, fp)` |
| 正常返回（epilogue） | `rt_deferrun(cur_fp)`：从链顶弹出所有 `fp == cur_fp` 的，逆序调 |
| panic 中返回（`D-M9-12`） | 同上（epilogue 是同一条路径）→ 所以 defer 顺序**天然正确** |
| `recover()` | 只在 defer 的**直接函数体**内有效：`rt_recover()` 查「正在跑 defer」+「本帧在 panicking」⇒ 清状态并返回值 |

**panic 传播**：

```text
rt_panic(desc, val):  current_panic = {desc,val}; panicking = 1
每个调用点后:          if rt_panicking() { goto epilogue }     ← SSA pass 注入
epilogue:             rt_deferrun(cur_fp); ret                ← 逐层退
main 的 epilogue:      if rt_panicking() { 打印 "panic: <值>"; exit(2) }
```

**为什么状态是全局的而不是每帧一个**：M9 只有 main 一个 goroutine（M11 才有并发）；
M11 引入多 G 时必须把它挪进 G 描述符 —— 这是**已知的接口变更点**，记在 §8 风险表。

**运行时检查**（`RT-EXC-04` 的 ◐）：除零 → `rt_panic_div0`；越界 → `rt_panic_bounds`；
nil 解引用 → `rt_panic_nil`；nil map 写 → `rt_panic_nilmap`。四条文本与解释器一致（`C9` §4.5）。

### 3.11 根协议（`D-M9-13`）

**safepoint 集合** = `Call` ∪ `Make` ∪ **回边**。回边的判据用已有的支配关系（`F16`）：
边 `b → h` 是回边 ⟺ `h` 支配 `b`。

**stack map 的粒度**：对每个 safepoint，对每个**活值**，查它的 `TypeDesc.gcdata`，
把「是根的字」在帧内槽偏移上标位。产出：

```text
STACK-MAP v1（并列产物 <out>.stackmap，同时编进 .rodata）
# func <id> name=<sym> frame=<bytes> safepoints=<n>
sp <idx> off=<frame_off> live=<n> ptrs=<bitmap-hex>
```

**位序（必须写死，否则两侧各按各的理解）**：位图覆盖**整个帧**区间
`[fp - frame_size, fp)`，bit `i` ↔ 地址 `fp - frame_size + 8*i`（bit 0 = 帧内最低地址）。
**包含** `outgoing` 区（栈实参里有指向堆的指针）与 `tmp` 区（并行拷贝的临时）；
不含 saved `fp`/`lr` 那 16 字节（它们在 `fp` 之上，不是根）。

**`.rodata` 里的形态**（M10 消费）：

```text
RtFuncInfo = { frame_size: u32, n_sp: u32, sp_off: *u32, sp_bits: *u64 }
```
`sp_off` / `sp_bits` 按 safepoint 索引平行数组。M9 的交付是**表 + 协议**；
按 PC 反查函数（pclntab）归 M10 —— M9 只保证「给定函数索引就能扫它的帧」。

**全局 roots**：包级变量 → 静态表 `GlobalRoot { desc: *TypeDesc, addr: *u8 }`，
`rt_globals` / `rt_nglobals` 两个符号暴露给 M10。

**「可检查」怎么落实**（`ROOT-1` 的牙）：`.stackmap` 产物与 `.rodata` 内的表**逐字节对账**
（沿 `F15` 的 `native_frame_map` 先例）—— 任何一侧漏了，那条测试就红。

### 3.12 channel 契约（`D-M9-14`）

只交结构与协议，**不写执行**（M11）：

```text
hchan = { qcount: u32, dataqsiz: u32, buf: *u8, elemsize: u32,
          closed: u32, elemtype: *TypeDesc,
          sendq: waitq, recvq: waitq, lock: u32 }
waitq = { head: *sudog, tail: *sudog }
sudog = { g: *gdesc, elem: *u8, next: *sudog, prev: *sudog, ... }
```

| 字段 | M11 是否需要 | 备注 |
|---|---|---|
| `qcount` / `dataqsiz` / `buf` / `elemsize` | 需要 | 环形缓冲（M11 按 `buf+off*elemsize` 取） |
| `elemtype` | 需要（M9 新增） | 解释器的 `ChanObj` 靠 Rust 类型记 `elem`；native 只能靠 desc |
| `closed` | 需要 | |
| `sendq` / `recvq` / `sudog` | 需要 | `sudog` 的字段集 M11 定（要挂 goroutine） |
| `lock` | M11 定 | 单核时是 0；多核时是自旋锁 |

判据是 **Rust 单测**（`chan_contract`）：布局的 `size_of` / `offset_of` 断言 + 三操作
（`try_recv` / `try_send` / `close`）在「纯数据结构」层面的真值表 —— 与 `M8` 的
`chan_obj` 同形，但**不跑 Go 代码**。

---

## 4. 判据与装置

### 4.1 语料三栏（`D-M9-15`）

「入选语料」= M6 冻结的 **69** 个（`run-selected.txt`）。M9 的门禁是**全部 69 个都进 `N'`**。

| Phase | 动作 | `N'`（绿） | `M'`（拒） | 说明 |
|---|---|---|---|---|
| 现状 | — | **39** | 7 | 7 个全 `CG-UNSUPPORTED-SLICE` |
| M9a | 分配器 + `TypeDesc` + ptrmask | 39 | 7 | **不涨数字**（骨架） |
| slice | `rt_makeslice`/`growslice` + isel | **46** | 0 | 7 个切片文件全绿 |
| map | `rt_map_*` + 迭代器 | **51** | 0 | `bigmap` / `indirect` / `ken/complit` / `ken/range` / `print` |
| iface | `rt_box` / `rt_itab` / 静态 ITab | **60** | 0 | `alias1` / `ken/embed` / `interbasic` / `interfun` / `intervar` / `rob1` / `method` / `method3` / `method5` |
| funcval | `FuncAddr` / `MakeClosure` + 隐式 ctx | **67** | 0 | `closure1/2` / `ddd` / `func6` / `ken/litfun` / `ken/ptrfun` / `nilptr2` |
| defer + panic | `Defer` + 返回码传播 | **69** | 0 | `closure4` / `defernil` |
| 根协议 | 回边 safepoint + stack map | 69 | 0 | 不涨数字，但 `ROOT-1` 绿 |
| channel 契约 | `hchan` + 单测 | 69 | 0 | 不涨数字，`SCH-1` 绿 |

**装置变更**：`run-native.sh` 的输入从 `native-input.txt`（46 行）换成
**`run-selected.txt`（69 行）**；`native-rejected.txt` 的 7 行在 slice 落地后清零，
之后如果出现新的 `M'`，逐行写进同一份清单（三栏：路径 / 码 / 理由）。

**「撞到别的墙」的处理**（`D-M9-15` 的兜底）：某个文件在解锁了它当前的拒绝原因之后
又撞到 M9 之外的墙（如 `unsafe` 的 interior pointer、反射），则**记 `M'` 并写清是哪一面**。
`N' + M' == 69` **恒成立**（这是 `T-CON-54` 同款的两向断言），所以分母不会漂。

### 4.2 差分口径

| 腿 | 内容 | 与 M7 的差别 |
|---|---|---|
| ① native vs 官方 `.exp` | 逐字节 | 不变（`native_corpus_run`） |
| ② native vs 解释器 | 逐字节 | **新增**：解释器在 69 上已与官方逐字节一致（`F12`），所以这条能**定位**错误在哪一侧 |
| ③ `--via=hir` vs `--via=ssa` | 逐字节 | 不变（`ssa_diff`），M9 只影响 ssa 侧 |

**为什么加第 ② 腿**：M7 只有 ①，出错时只能看到「native 与官方不同」，再去手工判断是
native 错还是官方（不可能）。② 让「三方一致」成为可能，且在 M9 这种「新实现一整套对象表示」
的场景里最值钱 —— 解释器的值模型是同一套语义的另一份实现。

### 4.3 元数据自检（三条对账）

| 对账 | 两侧 | 抓什么 |
|---|---|---|
| `MEM-2` | `TypeDesc.size/ptrmask`（native 侧）vs `layout`/`ptr_mask`（Rust 侧） | 两条路算出不同的布局 —— 这是 M7 「形参绑定漏第二字」同类错的温床 |
| `ROOT-1` | `<out>.stackmap`（并列产物）vs `.rodata` 内的 `RtFuncInfo` 表 | 一侧漏了 safepoint / 漏了位 |
| `OBJ-2` | map 的迭代顺序：native（`state` 判）vs 解释器（快照 key 列表） | **不要求相同**（规范无序），但要求「已删的不产出」两边都成立 |

> 前两条是**逐字节**的；第三条是**性质**的（无序 ⇒ 不能逐字节）。把它们的差别写清楚，
> 否则后来者会以为第三条「没做对」。

### 4.4 预算与资源

| 项 | 默认 | 超限 |
|---|---|---|
| 堆上限 | 256 MiB（`RGOC_HEAP_MAX`） | 退出 2 + `runtime error: out of memory (heap limit N bytes)` |
| defer 记录数 | 无硬限（受堆上限约束） | 同堆超限 |
| 单程序墙钟 | 20 s（装置侧，`run-native.sh` 的 `timeout`） | 记为 bad |
| `.rodata` 体积 | 期望 < 256 KiB（`TypeDesc` 静态表） | 超了说明字段集塞多了（`D-M9-1/2` 的失效条件） |

---

## 5. 不支持面（白名单，10 条）

| id | 不承诺 | 形态 | 后果 |
|---|---|---|---|
| `U-M9-01` | **GC / 内存回收** | 不存在（arena 只增） | 长程序撞 `heap limit` 退 2 |
| `U-M9-02` | size-class 多级分配器（`RT-MEM-01/02`） | 不存在 | 只有 bump；碎片不管 |
| `U-M9-03` | 寄存器分配与 spill（`PIPE-20`） | 不存在（沿用 M7 每值一槽） | 代码慢 2–3×（无性能门禁，测不出） |
| `U-M9-04` | 反射（名字 / 字段表 / Tag，`RT-POLY-06`） | `TypeDesc` 无名字 | `reflect` 内建继续拒 |
| `U-M9-05` | 泛型运行期表示（`RT-POLY-05`） | 不存在（全量单态化） | 泛型仍走 M5 窄面 |
| `U-M9-06` | traceback / pclntab（`RT-EXC-05`） | 不存在 | panic 只有一行，不打栈（沿 `U-M8-08`） |
| `U-M9-07` | native 的 `go` / `select` 执行 | 代码里没有 | `M9d` 只交契约；执行归 M11 |
| `U-M9-08` | map 的渐进搬迁（`RT-CMP-03` 的一半） | 不存在 | 一次 rehash 会卡顿（可测） |
| `U-M9-09` | open-coded defer（`RT-EXC-01` 的一半） | 不存在（全局链） | 每次 defer 一次堆分配 |
| `U-M9-10` | **interior pointer 的 metadata** | **明确拒绝** | `unsafe.Pointer` 做算术后得到的指针**对 GC 不可见** → M10 会漏扫。这是 roadmap 要求「明确支持或拒绝」的那一条，我们选**拒绝**，并在 `C5` 写清 |

> `U-M9-10` 是**语义级**的拒绝，不是「还没做」：`*(*int)(unsafe.Pointer(uintptr(p)+8))`
> 这样的代码在 M9/M10 下会得到「GC 看不见这个指针」的行为。若将来要支持，必须先定
> interior pointer 的 metadata 方案（官方用 `span` + `findObject`）。

---

## 6. 错误处理

| 情形 | 行为 | 判据 |
|---|---|---|
| 堆超限 | 退出 **2** + `runtime error: out of memory (heap limit N bytes)` | `MEM-1` |
| 除零 | `panic: runtime error: integer divide by zero` | `EXC-1` |
| 越界 | `panic: runtime error: index out of range [i] with length n` | `EXC-1` |
| nil 解引用 | `panic: runtime error: invalid memory address or nil pointer dereference` | `EXC-1` |
| nil map 写 | `panic: assignment to entry in nil map` | `EXC-1` |
| 未捕获 panic | 退出 **2** + `panic: <值>`（一行，不打栈） | `EXC-1` |
| 布局不可算（泛型 / `Tuple` / `[...]T`） | **编译期**拒（`CG-UNSUPPORTED-*` 已有），不许静默给个尺寸 | `MEM-2` |

**所有运行期错误文本必须与解释器逐字节一致** —— 这是 `C9` §5 / §4.5 的要求，
也是第 ② 条差分腿能成立的前提。

---

## 7. 覆盖清单（roadmap §M9 ↔ 功能 ID）

| 功能 ID | 名字 | M9 做到什么 |
|---|---|---|
| `RT-MEM-01` | 多级分配器 | ✗ **不做**（`U-M9-02`）：只有 bump arena |
| `RT-MEM-02` | 小/大对象路径 | ✗ 同上 |
| `RT-MEM-03` | 栈内存管理 | ✗ 归 M11（native 的 goroutine 栈） |
| `RT-MEM-04/05` | GC（STW / 并发） | ✗ 归 M10；M9 只交根协议 |
| `RT-MEM-06` | 堆位图与指针元数据 | ✅ `TypeDesc.gcdata` + `ptr_mask` |
| `RT-MEM-07` | 统一分配入口 | ✅ `rt_alloc(desc, need_zero)` |
| `RT-MEM-08` | 内存统计 | ◐ 只报 `used`/`limit`，不做 `HeapAlloc/HeapObjects` 细分 |
| `RT-CMP-01` | Slice 头 | ✅ `{ptr,len,cap}` + `makeslice`/`growslice` |
| `RT-CMP-02` | String 头 | ✅ M7 已有（`F13`），M9 补 `string(rune)` 与拼接的边界 |
| `RT-CMP-03` | Map 桶算法 | ◐ 开放寻址 + 线性探测；**无渐进搬迁**（`U-M9-08`） |
| `RT-CMP-04` | Map 迭代随机化 | ✅ 迭代器按 `state` 判（「已删不产出」）；顺序不承诺 |
| `RT-CMP-05` | Channel 结构 | ◐ 只交 `hchan` 布局（`D-M9-14`），执行归 M11 |
| `RT-CMP-06` | Channel 语义全套 | ✗ 归 M11 |
| `RT-CMP-07` | 数组/结构体表示 | ✅ 值语义留栈 + `zerobase`（M7 已有） |
| `RT-POLY-01` | `_type` 元数据 | ✅ `TypeDesc` 40 B（`D-M9-2`） |
| `RT-POLY-02` | 接口值布局 | ✅ `{tab|type, data}` 2 字 |
| `RT-POLY-03` | itab 动态分派 | ◐ 静态生成 + `rt_itab` 兜底；**无哈希缓存** |
| `RT-POLY-04` | 断言 / type switch | ✅ `x.(T)` 比 desc；`x.(J)` 走 `rt_itab` |
| `RT-POLY-05` | 泛型分派 | ✗ 继续全量单态化（`U-M9-05`） |
| `RT-POLY-06` | 反射元数据 | ✗ 归 M12（`U-M9-04`） |
| `RT-POLY-07` | method value 闭包化 | ✅ `MakeClosure{fn: 方法体, captures: [recv]}` |
| `RT-EXC-01` | defer 链表 | ◐ 全局链（**非** open-coded，`U-M9-09`） |
| `RT-EXC-02` | panic 级联展开 | ✅ 返回码逐层传播（`D-M9-12`） |
| `RT-EXC-03` | recover 拦截器 | ✅ 仅 defer 直接函数体内有效 |
| `RT-EXC-04` | 运行时致命错误 | ◐ 四条检查（除零/越界/nil/nilmap）；不做栈溢出与并发 map 写 |
| `RT-EXC-05` | 栈展开与 traceback | ✗ 归 M12（`U-M9-06`） |
| `RT-EXC-06` | 编译器协同 | ✅ `Instr::Defer` + `panic_check` pass |
| `PIPE-13` | 逃逸分析 | ✗ **不做优化**（roadmap 明写）；只有 `R-13-5` 的「取址且逃出」判定 |

---

## 8. 风险登记（前三条是最可能的坑）

| # | 风险 | 症状 | 对策 |
|---|---|---|---|
| `RM-1` | **`V10` 漏注入**（某个调用点后没有 `panicking` 检查） | panic 被吞，程序**继续跑**；输出错但没有任何报错 | verifier 硬检查（`V10`）+ 一条「三层调用各 defer 一次、最内层 panic」的 fixture（少一层检查就少一行输出） |
| `RM-2` | **根协议漏根**（`ptrs` 仍是「值是指针类型」粒度） | M9 全绿，M10 一上就随机崩（活对象被回收） | §4.3 的对账 + 一条**专门**的 fixture：`var s []int` 且 `s` 是唯一持有者（漏根就必崩）。这条 fixture 必须在 `ROOT-1` 里 |
| `RM-3` | **闭包捕获与取址槽的交互**（`MakeClosure` 的捕获值 vs `Frame::heap` 的取址槽） | 闭包写捕获，外层看不到（或反之）；`x++` 后闭包读到旧值 | `V12` + 「闭包改捕获、外层读」的 fixture；与 M7 的「`string` 丢 len」同类，靠**逐字段**对账而非整体看输出 |
| `RM-4` | 语料里有文件撞 M9 之外的墙 | `N'` 到不了 69 | 记 `M'` 并写清是哪一面（`D-M9-15` 的兜底）；`N'+M' == 69` 恒成立 |
| `RM-5` | `panic` 的全局状态在 M11 多 G 下失效 | M11 才暴露 | 已在 §3.10 标注为**已知接口变更点**；`C11` 里留一行 |
| `RM-6` | `.rodata` 的 `TypeDesc` 表随类型闭包膨胀 | 产物变大、链接变慢 | `D-M9-1/2` 的失效条件：超 256 KiB 就查字段集 |

---

## 9. 任务分解原则（`M9-plan.md` 的输入）

1. **Phase 0 = M9a（骨架）**，不涨数字但绕不过：`TypeDesc` + `ptr_mask` + `rt_alloc` +
   `rgoc-meta`。它是后面每一段的**前置**。
2. **每个特性 Phase 的五步固定节奏**：
   ① IR/verifier（加变体 + 不变式）→ ② `rgoc-meta`（desc 与 itab 生成）→
   ③ `isel`（指令选择 + 调用序列）→ ④ `rt.c`（运行期函数）→ ⑤ 语料（跑 `run-native.sh`，交数字）。
   五步的顺序不能颠倒：先 IR 才能验，先 meta 才有 desc，先 isel 才能链，先 rt 才能跑。
3. **每个 Phase 收口必须交一个数字**（`N'` / `M'`），没有数字的 Phase 不算收口。
4. **Phase 边界允许「按实测重写后半段」**：`Q1` 选了「一次到底」，代价是 c1/c2/d 的任务
   写在还没写的代码上。所以 `M9-plan.md` 必须声明：**Phase 5（defer/panic）之前的所有
   Phase 收口时，后续 Phase 的 Task 允许按实测重写**（沿 `M8-plan.md` 的做法，每条改动
   记 `R-9-*` 修订）。
5. **`M9d`（channel 契约）最后做**：它不依赖前四段，但放在最后可以让它的字段表**吸收**
   前面几段发现的真实需求（例如 `elemtype` 就是 M9a 才需要的）。

---

## 10. 契约与产物影响

| 契约 / 产物 | 变化 | 谁消费 |
|---|---|---|
| `C4-ssa.md` | 加 §10：3 个新 `Instr` + `V10/V11/V12` | `rgoc-ssa`、verifier |
| `C5-abi.md` | 加 §10：`TypeDesc` 的**内存布局**（不是 Rust 结构体）、funcval 的隐式第 0 实参、`STACK-MAP v1` 格式、`rt_*` 的 sret 约定 | `rgoc-arm64`、M10、M11 |
| `C10-ssa-dump.md` | 追加 3 行 `op` 拼写（既有 golden 不动） | dump 对撞 |
| `C11-concurrency.md` | 加 §8：`hchan` 契约（**参照不同构**） | M11 |
| `C9-values-and-output.md` | 补四条运行期 panic 文本（除零/越界/nil/nilmap）与解释器一致 | 差分第 ② 腿 |
| **新产物** `<out>.stackmap` | `STACK-MAP v1`，与 `.framemap` 并列；同时编进 `.rodata` | M10、`ROOT-1` 对账 |
| **新 crate** `rgoc-meta` | `TypeDesc` / `MethodTable` / `ptr_mask` 的唯一实现 | `rgoc-arm64`、dump、M10 |
| `rt.c` | 815 → ~2200 行；符号 24 → 预计 ~45 | 所有 native 产物 |

---

## 11. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-25 | **初稿**：`D-M9-1..16` / `F1..F16` / 门禁 `MEM-1..SCH-1`（十条）/ 覆盖清单 / 风险 6 条 | `03-roadmap.md` §M9 逐句展开；`M7-report.md`（边界与十九条真错）；`M8-report.md`（并发基线）；四条签字 `Q1..Q4` |
