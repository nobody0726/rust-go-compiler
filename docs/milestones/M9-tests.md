# M9 测试清单：native 对象模型、接口与 metadata

> **状态**：🟡 **判据面冻结中**（2026-10-25）　|　**设计**：[`M9-design.md`](./M9-design.md)（`D-M9-1..16`）　|　**计划**：[`M9-plan.md`](./M9-plan.md）
>
> **本文档定「怎么算通过」**，不定「怎么做」。凡是本文档与 `M9-design.md` 冲突的，
> 以**本文档**为准（判据优先），并按 §9 的修订流程改设计。

| 项 | 值 |
|---|---|
| 阶段 | M9（对象模型 / 分配器 / 元数据 / slice / map / iface / funcval / 异常 / 根协议 / channel 契约） |
| 判据来源 | `S`（自有 fixture，Go 源走完整流水线）、`O`（官方语料：native 与 `go run` 逐字节）、`D`（**差分**：native 与解释器逐字节）、`H`（装置 / 元数据对账 / 单测） |
| 输出判据 | 合并流**逐字节** + 退出码；唯一容差 `\r\n → \n`（沿 `M5-tests.md` §4.1） |
| 语义基准 | 官方 `go run`（第一基准）+ 解释器（**第二基准**，见 §1 的 `D` 腿） |
| 门禁 | §7，`MEM-1` / `MEM-2` / `OBJ-1..4` / `EXC-1` / `ROOT-1` / `CORP-1` / `SCH-1`（**十条**） |
| 白名单 | §5，**10 条**，开工前冻结；跳过项一律**不得**记为 pass |
| 变异 | §6，**10 条**，每条必须实证变红并复原 |
| 语料 | §3，**69**（39 零回归 + 23 逐个解锁），`N' + M' == 69` 恒成立 |

---

## 1. 判据来源（四种，含名与实）

| 代号 | 含义 | 载体 |
|---|---|---|
| `S` | 自有 fixture：Go 源走完整流水线（`rgoc build` → ELF → 跑） | `rgoc-driver/tests/obj_*.rs`、`build_*.rs` |
| `O` | 官方语料：native 的合并流与退出码 = `go run` 的 | `tools/run-oracle/run-native.sh` |
| `D` | **差分**：native 的合并流 = 解释器（`rgoc run`）的 | `rgoc-driver/tests/native_vs_interp.rs` |
| `H` | 装置 / 元数据 / 单测：`STACK-MAP` 对账、`TypeDesc` 对账、`hchan` 的 `size_of`、manifest | `rgoc-driver/tests/stackmap.rs`、`rgoc-meta/tests/*.rs`、`rgoc-driver/tests/chan_contract.rs` |

### 1.1 为什么 M9 必须有 `D` 腿（这条是新增的）

M7 只有 `O`（native vs 官方）。出错时只能看到「和官方不一样」，无法判断是哪一侧错。
M9 要**从头实现一整套对象表示**（slice 头 / map 桶 / 接口对 / 闭包对象 / 异常展开），
每一样都有「两边都对但值不同」的失败模式（例如 slice 的 `cap` 算错、接口的 `data`
该 box 还是该内联）。

解释器在 69 个语料上已与官方逐字节一致（`M9-design.md` `F12`），所以：

* `D` 腿红而 `O` 腿绿 ⇒ **native 错**（定位到侧）
* `O` 腿红而 `D` 腿绿 ⇒ 不可能（解释器已对过官方）⇒ 说明 `O` 腿的期望值取错了
* 两腿都红 ⇒ 很可能是**新写的 fixture** 本身就是错的

**纪律**：`D` 腿只对「解释器也支持」的程序有意义。native 支持而解释器不支持的
（例如 M9d 的 `hchan` 布局）只能走 `H`。

---

## 2. 判据分组

### 2.1 `MEM-1` 分配与布局（`S` + `H`，8 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-MEM-01` | 分配返回对齐地址 | 连分 100 个 `desc.size` ∈ {1,3,8,9,17} 的对象 ⇒ 每个地址 `% 8 == 0` |
| `T-MEM-02` | 对象头是 desc | 分配后读 `[p-8]`（或按 `C5` 的偏移）== 传入的 `*TypeDesc` |
| `T-MEM-03` | 零值 | `make([]int, 3)` 的 3 个元素全 0；`new(S)`（S 含 `*int` 与 `float64`）全零（含 `0.0` 的位模式）；`new(S2)`（S2 只含 int）可不清零但**值正确** |
| `T-MEM-04` | `need_zero` 由 `rgoc-meta` 算 | 纯整数聚合的 `flags.needs_zero == false`；含指针或浮点的 `== true`（直接断言 `TypeDesc.flags`） |
| `T-MEM-05` | 分配递增 | 同一 desc 连分 N 次 ⇒ 地址单调递增，间隔 == `align_up(size, 8)` |
| `T-MEM-06` | chunk 增长 | 分到超过初始 16 MiB ⇒ 不崩，地址继续可用（`rt_heap_used()` 随之增长） |
| `T-MEM-07` | 堆上限触发 | `RGOC_HEAP_MAX=1MiB` + 循环分 100 MiB ⇒ 退出 **2** + stderr `runtime error: out of memory (heap limit 1048576 bytes)` |
| `T-MEM-08` | 布局不可算必须拒 | 泛型 / `[...]T` 的类型出现在 `make` 的参数里 ⇒ **编译期**拒（`CG-UNSUPPORTED-*`），不许静默给尺寸 |

### 2.2 `MEM-2` 元数据对账（`H`，6 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-META-01` | `TypeDesc` 的尺寸与偏移 | `size_of::<TypeDesc>() == 40`；`offset_of` 逐字段 == `C5` §10 冻结的值 |
| `T-META-02` | 逐类型 `size` 对撞 | 69 个语料的**类型闭包**上，`TypeDesc.size` == `rgoc_types::layout().size`（逐类型，报告差异的第一个） |
| `T-META-03` | 逐类型 ptrmask 对撞 | 同上闭包，`gcdata` 解出的位图 == `ptr_mask()` 的结果 |
| `T-META-04` | ptrmask 的五个特例 | `string` = `[1,0]`；`slice` = `[1,0,0]`；`iface` = `[1,1]`；`*T` = `[1]`；`struct{a int; b *T}` 按字段偏移 `[0,1]` |
| `T-META-05` | 类型 hash 稳定且区分 | 同一类型两次算 == 同一个值；`int` vs `int64` vs `[]int` vs `[]int64` 四者互不相同；结构体按字段序（`struct{a,b int}` ≠ `struct{b,a int}`） |
| `T-META-06` | `elem` 递推 | `[][]int` 的 `elem` == `[]int` 的 desc；`[3]int` 的 `elem` == `int`；`map[string]int` 的 `elem` 是 value 的 desc（与 `key` 分开，见 `T-MAP-01`） |

### 2.3 `OBJ-1` slice（`S` + `O` + `D`，8 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-SLICE-01` | `make` 三形态 | `make([]int,3)` 的 `len=3 cap=3`；`make([]int,1,5)` 的 `len=1 cap=5`；`make([]int,0)` 的 `len=cap=0` 且**非 nil**（`s == nil` 为 false） |
| `T-SLICE-02` | nil slice | `var s []int` ⇒ `len=0 cap=0`、`s == nil` 真、`range` 零次、`append` 可用 |
| `T-SLICE-03` | 切片表达式 | `s[1:3]` 的 `ptr` 前进 1 个元素、`len=2`、`cap=cap-1`；`s[1:3:4]` 的 `cap=3` |
| `T-SLICE-04` | 下标读写 | `s[i] = v` 后 `s[i]` 读回；**别名**：`t := s[0:2]; t[0]=9` ⇒ `s[0]==9`（同一底层数组） |
| `T-SLICE-05` | `append` 不扩容 | `cap` 够时 `ptr` 不变、`len` +1 |
| `T-SLICE-06` | `append` 扩容 | `cap` 不够时新分配、`ptr` 变、旧 slice 不受影响（拷出 `old := s` 再比） |
| `T-SLICE-07` | 越界 panic | `s[3]`（len=3）⇒ 退出 2 + `panic: runtime error: index out of range [3] with length 3` |
| `T-SLICE-08` | 切片越界 | `s[2:5]`（len=3）⇒ 退出 2 + 与解释器**同文本**（`D` 腿比） |

### 2.4 `OBJ-2` map（`S` + `O` + `D`，9 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-MAP-01` | `make` 与零值 | `make(map[string]int)` 可读写；`var m map[string]int`（nil）读得零值、写 panic |
| `T-MAP-02` | 读写基本 | 插入 10 个键、逐个读；未插入的键得零值 |
| `T-MAP-03` | comma-ok | `v, ok := m[k]`：有键 ⇒ `(v,true)`；无键 ⇒ `(零值,false)` |
| `T-MAP-04` | `delete` | 删后读得零值 + `ok=false`；`len` 减 1；删不存在的键不 panic |
| `T-MAP-05` | rehash | 插到超过 `0.75*cap` ⇒ `len` 正确、全部旧键仍可读（一次全量 rehash） |
| `T-MAP-06` | 键相等：string | `"ab"` 与另一个 `"ab"`（不同底层数组）算同一个键 |
| `T-MAP-07` | 键相等：struct | `struct{a,b int}` 作键，字段序不同算不同的键；`struct{p *int}` 作键按指针比 |
| `T-MAP-08` | 迭代中删除不产出 | 迭代中 `delete` 一个**尚未访问**的键 ⇒ 它不产出；已访问过的不影响（`D` 腿 + 计数） |
| `T-MAP-09` | 迭代顺序不承诺 | 同一 map 两次 `range` 的顺序**允许不同**（判据：只断言「键集合相同、次数相同」，**不断言顺序**） |

### 2.5 `OBJ-3` iface（`S` + `O` + `D`，10 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-IFACE-01` | 装箱到 `any` | `var x any = 42` ⇒ `x.(int) == 42`；`var y any = "s"` ⇒ `y.(string) == "s"` |
| `T-IFACE-02` | 装箱到有方法的接口 | `var i I = T{}` ⇒ `i.M()` 调到 `T.M`（静态 ITab 的 `fn[slot]`） |
| `T-IFACE-03` | 方法调用的 receiver | 指针接收者方法：`var i I = &T{}` ⇒ `i.M()` 改的是那个 `T`（同一对象） |
| `T-IFACE-04` | **typed nil** | `var p *T; var i I = p` ⇒ `i != nil`（真），但 `i.M()` 里 `p == nil`。**这条是 roadmap 门禁点名的** |
| `T-IFACE-05` | 断言到具体类型 | `x.(int)` 成功；`x.(string)` ⇒ `ok=false` 且值零值 |
| `T-IFACE-06` | 断言到接口 | `var a any = T{}; j := a.(J)` ⇒ 走 `rt_itab` 且 `j.M()` 正确（**运行期兜底路径**） |
| `T-IFACE-07` | type switch | 三分支 + default，命中数与打印都对；`case nil` 单独一支 |
| `T-IFACE-08` | 接口比较 | `var a, b any = 1, 1` ⇒ `a == b`；`a = 1, b = "1"` ⇒ `false`；含不可比较类型时**编译期**拒（`C8`） |
| `T-IFACE-09` | 接口 hash | 两个内容相同的 `any` 作 map 的键 ⇒ 同一个键（靠 `TypeDesc.hash` + `rt_equal` 的 iface 分支） |
| `T-IFACE-10` | 空接口是 2 字 | `size_of` 断言：`any` / `I` 都是 16 B，`data` 在 +8 |

### 2.6 `OBJ-4` funcval（`S` + `O` + `D`，8 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-FUNC-01` | 函数值 | `f := add; f(1,2) == 3`（`FuncAddr` + `Indirect`） |
| `T-FUNC-02` | 闭包捕获只读 | `f := func() int { return x }`；改 `x` 后 `f()` 看到新值（捕获是**引用**） |
| `T-FUNC-03` | 闭包写捕获 | `x++` 在闭包里 ⇒ 外层 `x` 变（`D` 腿 + 3 次调用后 `x==3`） |
| `T-FUNC-04` | 方法值 | `f := t.M`（值接收者）⇒ `f()` 用**捕获时的副本**；`g := p.M`（指针接收者）⇒ 用同一对象 |
| `T-FUNC-05` | 多个捕获 | 捕获 4 个（含 `string` 与 `*T`）⇒ 每个都读对（防「只搬了第一个」） |
| `T-FUNC-06` | 闭包作参数/返回值 | `apply(f, x)` 与 `mkAdder(n) func(int) int` 都对 |
| `T-FUNC-07` | 嵌套闭包 | 两层闭包，内层捕获外层的捕获 ⇒ 值正确 |
| `T-FUNC-08` | funcval 是指针 | `size_of(funcval) == 8`；两个不同的闭包 `!=`（比指针） |

### 2.7 `EXC-1` 异常与检查（`S` + `O` + `D`，10 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-EXC-01` | defer 逆序 | 三个 `defer` ⇒ 输出倒序（`3,2,1`） |
| `T-EXC-02` | defer 与命名返回值 | `func f() (r int) { defer func(){ r++ }(); return 1 }` ⇒ 返回 2 |
| `T-EXC-03` | defer 在 panic 中执行 | `defer print("d"); panic(1)` ⇒ 输出 `d` 然后 `panic: 1`、退出 2 |
| `T-EXC-04` | **逐层展开** | `f`→`g`→`h` 各 defer 一次，`h` 里 panic ⇒ 输出顺序 `h,g,f` 三个 defer 标记，再 `panic`。**少一层 = `V10` 漏注入**（`RM-1` 的牙） |
| `T-EXC-05` | recover 有效 | `defer func(){ recover() }()` + panic ⇒ 退出 **0**，panic 之后的语句不跑，函数正常返回 |
| `T-EXC-06` | recover 只在 defer 内有效 | 非 defer 里调 `recover()` ⇒ 返回 nil，panic 继续 |
| `T-EXC-07` | recover 只对同帧有效 | 子函数里 recover 父函数的 panic ⇒ 无效（panic 继续） |
| `T-EXC-08` | 除零 | `1/0`（运行期变量）⇒ `panic: runtime error: integer divide by zero`、退出 2 |
| `T-EXC-09` | nil 解引用 | `var p *T; p.x` ⇒ `panic: runtime error: invalid memory address or nil pointer dereference` |
| `T-EXC-10` | nil map 写 | `var m map[int]int; m[1]=2` ⇒ `panic: assignment to entry in nil map` |

### 2.8 `ROOT-1` 根协议（`H`，8 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-ROOT-01` | `STACK-MAP` 与 `.rodata` 一致 | `<out>.stackmap` 与 ELF 里的 `RtFuncInfo` 表**逐字节**对账（`native_frame_map` 的同款） |
| `T-ROOT-02` | 每个函数都有帧记录 | 69 个语料里每个被编译的函数都能在 `STACK-MAP` 里找到（按函数数对账） |
| `T-ROOT-03` | safepoint 数 ≥ 调用点数 | 逐函数：`n_sp >= 该函数 IR 里的 Call 数`（漏了 safepoint 就红） |
| `T-ROOT-04` | 回边有 safepoint | 一个「循环 100 次、循环体里无调用无分配」的函数 ⇒ `n_sp >= 1` 且其中之一在回边块上 |
| `T-ROOT-05` | 位图粒度到「字」 | `var s []int` 只有一个 `ptr` 字 ⇒ 位图里恰有 1 位；对照 `string`（1 位）、`iface`（2 位） |
| `T-ROOT-06` | **slice 的 ptr 字是唯一根** | fixture：`s := make([]int, 3)` 之后不再有别的引用，`s` 活在 safepoint 上 ⇒ 位图里 `s` 的 ptr 字被标（`RM-2` 的牙：按「值是指针类型」判就漏了） |
| `T-ROOT-07` | 全局 roots 表 | `rt_globals` / `rt_nglobals` 的条目数 == 包级变量数；每条 `(desc, addr)` 指对 |
| `T-ROOT-08` | 帧内区间约定 | 位图 bit `i` ↔ `fp - frame_size + 8*i`；`outgoing` 区内的指针被标、saved `fp/lr` 不被标 |

### 2.9 `CORP-1` 语料（`O` + `D`，4 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-CORP-01` | 69 全绿 | `run-native.sh` ⇒ `ok=69 skip=0 bad=0 / 69`；每个文件的 stdout 与官方 `.exp` 逐字节 |
| `T-CORP-02` | 三栏自洽 | `N' + M' == 69`（两向断言，`T-CON-54` 同款）；每条 `M'` 有码 + 理由 |
| `T-CORP-03` | 39 零回归 | M7 收口时的 39 个文件**逐个**仍绿且逐字节相同（不是只看总数） |
| `T-CORP-04` | `D` 腿全绿 | 69 个文件的 `native` 与 `rgoc run` 输出逐字节一致 |

### 2.10 `SCH-1` channel 契约（`H`，4 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-SCH-01` | `hchan` 布局 | `size_of::<Hchan>()` 与逐字段 `offset_of` == `C11` §8 冻结的值（与官方 `runtime/chan.go:34` 的字段序对齐） |
| `T-SCH-02` | 三操作真值表 | Rust 单测：`try_recv` / `try_send` / `close` 在（空/半/满/已关）×（有无等待者）上的结果 == `M8` 的 `chan_obj` 同一张表 |
| `T-SCH-03` | 等待队列是双向链 | `park_send` / `park_recv` / `revoke` 后 `head/tail` 不变式（空 ⇔ 两者皆 null；非空 ⇔ 两端互指） |
| `T-SCH-04` | 不跑 Go 代码 | `T-SCH-01..03` 全部在 Rust 单测里；`M9` 的 native 侧**没有** `go`/`select` 的执行路径（守卫：源码扫描不到 `rt_go` / `rt_select` 符号） |

---

## 3. 语料三栏（`D-M9-15`）

| 阶段 | `N'` | `M'` | 备注 |
|---|---|---|---|
| M8 收口（状态） | 39 | 7 | 7 个全 `CG-UNSUPPORTED-SLICE`（在 46 里） |
| Phase 0（M9a） | 39 | 7 | 不涨 |
| Phase 1（slice） | **46** | 0 | 7 个切片文件全绿 |
| Phase 2（map） | **51** | 0 | +5（`bigmap` / `indirect` / `ken/complit` / `ken/range` / `print`） |
| Phase 3（iface） | **60** | 0 | +9 |
| Phase 4（funcval） | **67** | 0 | +7 |
| Phase 5（defer/panic） | **69** | 0 | +2 |
| Phase 6（根协议） | 69 | 0 | 不涨 |
| Phase 7（channel） | 69 | 0 | 不涨 |

**装置变更**：`run-native.sh` 的输入从 `native-input.txt`（46）换成 **`run-selected.txt`（69）**。
`native-rejected.txt` 的 7 行在 Phase 1 收口时清零；之后若出现新 `M'`，逐行追加
（三栏：路径 / 码 / 理由）。

> **不许**为了「数字好看」而改分母：`N' + M' == 69` 是断言（`T-CORP-02`），不是口号。

---

## 4. 判据口径

| 项 | 口径 |
|---|---|
| 合并流 | `print` / `println` / `panic: <值>` 进**同一个** buffer，顺序 = 写入顺序（`C9` §4.1） |
| 容差 | 只有 `\r\n → \n`；其余逐字节 |
| 退出码 | `0` 跑完 / `1` 没跑（静态拒绝）/ `2` 跑了没跑完（panic、堆超限） |
| 运行期文本 | 四条（除零/越界/nil/nilmap）+ `out of memory`，**必须与解释器逐字节一致**（`D` 腿的前提） |
| map 迭代顺序 | **不断言**（规范无序）；只断言「键集合 + 次数 + 已删不产出」 |
| `.rodata` 对账 | `STACK-MAP` / `TypeDesc` 的产物与 ELF 内的表**逐字节**（`T-ROOT-01` / `T-META-02`） |
| 单测 vs 端到端 | 元数据与 `hchan` 走 Rust 单测（`H`）；**行为**一律走端到端（`S`/`O`/`D`） |

---

## 5. 白名单（10 条，`U-M9-01..10`）

见 `M9-design.md` §5。**纪律**：白名单项**不得**记为 pass；`U-M9-10`
（interior pointer）是**语义级拒绝**，必须有守卫测试（`T-ROOT-06` 的邻居：一条
`unsafe.Pointer` 算术后不被当根的 fixture，断言「位图里没有它」）。

---

## 6. 变异（10 条，每条必须实证变红）

| # | 变异 | 命令 | 必须变红的判据 |
|---|---|---|---|
| `M-M9-01` | `rt_alloc` 不清零（`need_zero` 恒 false） | `mutation-m9.sh` | `T-MEM-03` |
| `M-M9-02` | `ptr_mask` 对 `string` 只标 1 位 | 同上 | `T-META-04` |
| `M-M9-03` | `append` 扩容时不拷旧元素 | 同上 | `T-SLICE-06` |
| `M-M9-04` | `rt_map_del` 不置 `tomb`（置 `empty`） | 同上 | `T-MAP-08`（tomb 丢了会让探测链断） |
| `M-M9-05` | 静态 ITab 的 `fn[]` 按字典序而非声明序 | notes | `T-IFACE-02`（方法调到错的那个） |
| `M-M9-06` | `MakeClosure` 只填第一个捕获 | notes | `T-FUNC-05` |
| `M-M9-07` | `panic_check` pass 跳过最后一个块 | notes | `T-EXC-04`（最外层没退） |
| `M-M9-08` | `rt_deferrun` 用正序而非逆序 | 同上 | `T-EXC-01` |
| `M-M9-09` | `ptrs` 退回「值是指针类型」粒度 | notes | `T-ROOT-06`（`RM-2` 的牙） |
| `M-M9-10` | 回边不产 safepoint | notes | `T-ROOT-04` |

`scripts/mutation-m9.sh` 放**回归代价低**的 4 条（01/02/03/08），其余记
`scripts/mutation-m9-notes.md`（沿 M7/M8 的做法）。

---

## 7. 门禁 ↔ 测试 ID

| 门禁 | 判定 | 覆盖它的 ID |
|---|---|---|
| `MEM-1` | typed 分配 / 零值 / 对齐 / 超限退 2 | `T-MEM-01..08` |
| `MEM-2` | 元数据逐类型对撞 | `T-META-01..06` |
| `OBJ-1` | slice 全套 | `T-SLICE-01..08` |
| `OBJ-2` | map 全套 | `T-MAP-01..09` |
| `OBJ-3` | iface 全套 | `T-IFACE-01..10` |
| `OBJ-4` | funcval 全套 | `T-FUNC-01..08` |
| `EXC-1` | 异常与四条检查 | `T-EXC-01..10` |
| `ROOT-1` | 根协议 | `T-ROOT-01..08` |
| `CORP-1` | 语料 69 | `T-CORP-01..04` |
| `SCH-1` | channel 契约 | `T-SCH-01..04` |

---

## 8. 不变式与自检

### 8.1 IR 的三条新不变式（`C4` §10）

| # | 不变式 | 抓什么 | 牙 |
|---|---|---|---|
| `V10` | 每个 `Term` 可达的 `Call` 之后，同一块内必须有 `panicking` 检查 | 漏注入 ⇒ panic 被吞 | `M-M9-07` 必须变红 |
| `V11` | `Defer.call` 必须是 `Instr::Call` | defer 形状不一 | 变异（可选） |
| `V12` | `MakeClosure` 的捕获数 == 被调方的 `captured` 数且类型逐个 identical | 捕获错位 | `T-FUNC-05` |

**这四条（含 `T-EXC-04`）本身要做变异验证** —— 不变式恒真就等于没有不变式。

### 8.2 三条对账（`M9-design.md` §4.3）

`TypeDesc` ↔ `layout`；`STACK-MAP` ↔ `.rodata`；map 迭代的**性质**（不是逐字节）。
前两条是 `H` 腿的逐字节，第三条只断言性质 —— 把差别写清楚，否则后来者会以为第三条没做对。

---

## 9. 修订流程

**流程**：执行期发现判据写错/写漏 ⇒ 在 §9.2 追加一行 `R-9-y`（日期 / 改了什么 / 触发证据），
同步改 `M9-design.md` 或 `M9-plan.md`，**不许**只改代码不改文档。

### 9.2 修订

| 修订 | 日期 | 内容 | 触发 |
|---|---|---|---|
| — | — | （开工后按 `R-9-y` 追加） | — |

---

## 10. 与 M5 / M6 / M7 / M8 的判据边界

| 面 | M9 的口径 |
|---|---|
| M5 的值语义 | **`D` 腿的基准**：69 个语料上解释器与官方逐字节一致，所以 native 与解释器比 = 与官方比 |
| M5 的 `C9` | 四条运行期 panic 文本沿用 `C9` §4.5；M9 只**追加** `out of memory` 一条 |
| M6 的 SSA | **零回归**：`--via=ssa` 46 绿 23 跳过 0 红；M9 拆掉的 5 个 `SSA-UNSUPPORTED-*` 是**新建图能力**而不是改既有图 |
| M7 的 native | **零回归**：39 个文件逐个仍绿（`T-CORP-03`）；`.framemap` 产物不变，**新加** `.stackmap` |
| M8 的并发 | **零回归**：`run-chan.sh` 7 ok 12 skip 0 bad 不变；`M9d` 只交契约，不改解释器 |
| M8 的 `C11` | `M9d` 的 `hchan` **参照不同构**（单线程 vs 多核）—— 写进 `C11` §8 的抬头 |
| `--via=ssa` 在 chan 上 | 仍拒绝（`SSA-UNSUPPORTED-CHAN` 不变）；M9 不碰并发 |
