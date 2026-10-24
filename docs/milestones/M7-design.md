# M7 设计：单平台 native MVP（arm64 代码生成）

> **状态**：✅ **已确认（2026-10-24）** —— 决策 `D-M7-1..16` / 事实基线 `F1..F18` / 门禁 `NAT-1..9`
> **权威范围**：`docs/03-roadmap.md` §M7（本文件只做**展开与决策**，不改范围）
> **上游**：`M0-report.md` §P3（native spike 六项实测）、`M6-design.md` §3（IR 形状）、
> `C4-ssa.md` / `C10-ssa-dump.md` / `C9-values-and-output.md`、`SSA-LIVE v1`
> **下游**：`M7-tests.md`（判据）、`M7-plan.md`（任务分解）、`M7-manifest.json`（机器可读事实）
>
> **三条已签字**（§2.4）：runtime 用 freestanding C / 支持面只拒 `[]T`（预估 40 进 / 6 拒）/ 浮点打印做完整最短往返。

---

## 0. 本文档在流程中的位置

```text
03-roadmap §M7（做什么）→ 本文档（怎么定、边界在哪）→ M7-tests.md（怎么算通过）→ M7-plan.md（按什么顺序做）
```

三件套齐备才开工（`03` §6.2）。M7 与前六个里程碑的**根本差别**：前六个都在 Rust 进程里，
判据可以靠「两个 Rust 实现互相比」。M7 的产物是**另一个进程里的机器码**，判据必须靠
**字节**（stdout/stderr/退出码/ELF 头）—— 这是本文档每一处判据都要求「可执行文件真的跑」的原因。

---

## 1. 目标与非目标

### 1.1 输入与输出

| | 内容 |
|---|---|
| **输入 ①** | M6 的 **SSA**（`rgoc_ssa::Module`，14 个 `Instr` + 5 个 `Term`，`F11`），且**已经过 pass**（`--opt=all` 口径） |
| **输入 ②** | `SsaFunc::value_types` / `slot_types`（`F14`）—— 帧布局的唯一依据 |
| **输入 ③** | `SSA-LIVE v1` 文本（`F15`）—— safepoint 的活值表 |
| **输入 ④** | M0 native spike 的**实测结论**（`F1`/`F2`/`F3`）：链接命令、`-nostartfiles`、`-Wl,-s`、零 libc |
| **输入 ⑤** | `C9` §4.3/§4.4/§5 的**输出字节与退出码表**（`F12`/`F13`）—— native 的 print 不许另立口径 |
| **输出 ⑥** | 新 crate **`rgoc-arm64`**：SSA → **文本 arm64 汇编**（纯函数，不碰磁盘、不碰 `clang`） |
| **输出 ⑦** | `runtime/native/aarch64-unknown-linux-gnu/rt.c`（freestanding C）+ 编译驱动（`clang -c`） |
| **输出 ⑧** | `rgoc-driver build` 子命令：装载 → 检查 → HIR → SSA → opt → 汇编 → `clang` → ELF |
| **输出 ⑨** | **`C5-abi.md` 升正式**（原文是 spike 级，明确「M7 做 MVP」）：rgoc-AAPCS64 子集 + 帧布局 + `FRAME-MAP v1` |
| **输出 ⑩** | 装置：`run-collide.sh --native`（第四列）、`scripts/mutation-m7-notes.md`、native 语料分类器 |

### 1.2 范围（`03-roadmap.md` §M7 逐句展开）

| # | roadmap 句子 | 本文档的落点 |
|---|---|---|
| 1 | 「arm64 指令选择」 | §3.4：14 个 `Instr` + 5 个 `Term` **逐个**给选择规则，没有通配分支 |
| 2 | 「最小寄存器分配」 | §3.3：**每个值一个栈槽**，寄存器只作临时与传参（`D-M7-3`） |
| 3 | 「调用/栈对齐」 | §3.2：rgoc-AAPCS64 子集 + `sp` 恒 16 字节对齐（`D-M7-5`） |
| 4 | 「静态/全局数据」 | §3.4：`Root::Global` 走 `adrp/add`；字符串常量进 `.rodata` 的**独立标签** |
| 5 | 「startup」 | §3.7：`_start` 按 `Program::init_order` 依次调 `init`，最后调 `main`，`exit_group(0)` |
| 6 | 「汇编输出与 `clang` 系统链接」 | §3.1：`.s` → `clang -nostartfiles -Wl,-s`（`F2` 的三条实测约束原样沿用） |
| 7 | 「固定栈、无 GC」 | §3.8：静态 arena + bump；**不 free、不移动**（`D-M7-7`） |
| 8 | 「显式越界/除零/nil 检查」 | §3.9：三个检查点，失败 ⇒ `panic: <值>\n` + 退出码 2（`C9` §5） |
| 9 | 「信号→panic 延期，不依赖 Rust unwind 跨 Go frame」 | §1.3：native 侧**没有** unwind；runtime 用 C 不用 Rust（`D-M7-4`） |
| 10 | 「接口/闭包等先为 M9 定义 ABI」 | §3.2：约定里**预留** `x8`（间接结果）与「聚合按指针传」，M9 的 iface 直接挂上 |
| 11 | 「在 MVP 指针 fixture 上验证 frame layout、live set/stack map 和保存区」 | §3.10：`FRAME-MAP v1`（样例级，`D-M7-12`） |
| 12 | 「`rgoc build` 生成并运行 hello、调用/栈对齐/分支/init fixture」 | §4.1：fixture 分母 + 三方差分 |

### 1.3 明确不做（M7 边界）

| 不做 | 为什么 | 归谁 |
|---|---|---|
| 寄存器分配（图着色/线性扫描） | MVP 的判据是**正确**，不是快；一个值一个栈槽可让帧布局从 `value_types` 直接算出 | M9 之后 |
| 自研 assembler / linker / ELF writer | `03` §0.2 已定：委托系统 `clang`；「链接正确 ≠ 自研 PIPE-23/27 完成」 | 不立项 |
| DWARF / 源码级调试 | `04` §112 明确延期 | M7 之外 |
| slice（`[]T` + `make` + `append` + `cap`） | 要对象模型与逃逸分析；roadmap 把 string/slice/map 归 **M9b** | M9b |
| 接口动态派发 / 闭包 / `defer` / `map` / `chan` | M6 已经在 SSA 侧拒（`U-M6-01..09`），native 连面都见不到 | M6 已拒 / M9 |
| GC 与精确栈扫描 | roadmap：M7「无 GC」；扫描器在 M10 | M10 |
| 第二平台（x86_64） | `03` §0.2：单平台首发，改要改决策与门禁 | 另立项 |
| 优化（窥孔/指令合并） | `G-M6-04` 同源；先把「对」做出来 | M7+ |

### 1.4 退出条件（门禁 `NAT-1..9`）

| 门禁 | 名字 | 判定 |
|---|---|---|
| `NAT-1` | 产物与工具链 | `file` **双条件**（`ELF 64-bit LSB` + `ARM aarch64`，沿 `T-S3-02`）；同一份 SSA 两次生成 `.s` 逐字节相同；同一份 `.s` 两次链接 ELF 逐字节相同 |
| `NAT-2` | 指令选择完备 | 14 个 `Instr` + 5 个 `Term` **每个**至少一条 fixture；写不出的必须落在 §5 的码上，不得 `todo!()` |
| `NAT-3` | 调用与帧 | 递归 / 多返回值 / 8 个以上实参 / 栈对齐 / **块参数并行拷贝的交换环** 各一条 fixture |
| `NAT-4` | 三方差分 | native 切片 `N'` 上 `rgoc build` 产物 == `run --via=hir` == `go run`（合并流逐字节 + 退出码） |
| `NAT-5` | 输出字节 | `C9` §4.4 的**六个冻结点**在 native 上逐字节复现（含 `1e-05` / `0.33333334`） |
| `NAT-6` | 基础对象 | 数组/struct 的**值语义**（拷贝后互不影响）、指针写回、全局读写、`new` 的 arena 分配与清零 |
| `NAT-7` | 帧布局与 stack map | `FRAME-MAP v1` 的每个偏移与 `objdump` 反汇编里实际用的偏移一致；safepoint 活值全在表内 |
| `NAT-8` | 不支持面 | 每条 `CG-UNSUPPORTED-*` 一个 fixture（退出码 1 + 稳定码 + 不 abort） |
| `NAT-9` | 回归与收口 | 四条 cargo + 五条自检 + M6 的 46/23 零回归 + `C5` 冻结 + `GAP-01` 补 + 报告/manifest |

---

## 2. 已确认决策记录

### 2.1 决策一览

| # | 决策 | 一句话 |
|---|---|---|
| `D-M7-1` | 输入 = **优化后的 SSA** | 不从 HIR 直接生成；三方差分要求 native 与 `--via=ssa` 吃同一份 IR |
| `D-M7-2` | 产物 = **文本 `.s` → `clang`** | 不自研 assembler/linker/ELF writer |
| `D-M7-3` | **每个值一个栈槽**，不做寄存器分配 | 正确性优先；帧布局可从 `value_types` 直接算 |
| `D-M7-4` | runtime 用 **freestanding C** | 不用 Rust（unwind 跨 frame）、不用纯汇编（`ftoa` 写不出来） |
| `D-M7-5` | 自定 **rgoc-AAPCS64 子集**，写成 `C5` | 内部调用与 runtime 调用**同一套**，少一条路径 |
| `D-M7-6` | 块参数交接 = **并行拷贝**（带环检测） | 与 `D-M6-3` 同源；顺序搬会让 `a,b=b,a` 静默错 |
| `D-M7-7` | 分配 = **静态 arena + bump**，不 free | 「无 GC」的最小可运行形态；`new`/复合字面量都走它 |
| `D-M7-8` | native 支持面 = **实测再切**（沿 `R-6-1`） | 不先定支持面再挑文件；预估 40 进 / 6 拒，**实测替换** |
| `D-M7-9` | **string 纳入**（`(ptr,len)` + 拼接 + 索引 + 子串） | 27/46 个文件要打印字符串，绕不开；`[]T` 才是 M9b |
| `D-M7-10` | 浮点打印做**完整 `%g` 最短往返** | 判据已经冻结在 `C9` §4.4，不需要重新发明；半吊子实现是地雷 |
| `D-M7-11` | 检查失败 = `panic: <值>\n` + **退出码 2** | 与 `C9` §5 同口径；native 不许有第二套退出码 |
| `D-M7-12` | `FRAME-MAP v1` 只到**样例级** | roadmap 明确「尚未完整覆盖的对象类型留 M9」 |
| `D-M7-13` | 不支持面走 **crate-local `CgCode`** | 沿 `SsaCode` 先例；`DiagCode` 保持 103 |
| `D-M7-14` | 可复现 = `.s` 逐字节 + ELF 逐字节 | `-Wl,-s` 是**必需**（`F2`），不是为了让测试通过 |
| `D-M7-15` | 判据 = **三方差分**（native / hir / go run） | 不再带 `--via=ssa` 那条腿（M6 已证 46/46 一致） |
| `D-M7-16` | 每个 Task 收口即 commit；判据先于实现 | 沿 M4/M5/M6 |

### 2.2 五字段（只对**非显然**的八条展开）

#### `D-M7-3` 每个值一个栈槽

| 字段 | 内容 |
|---|---|
| **背景** | 「最小寄存器分配」有两种读法：真的做分配（线性扫描），或干脆不分配 |
| **选项** | ① 线性扫描 + spill；② 每个值一个栈槽，`x0..x7`/`v0..v7` 只作临时；③ 值全在寄存器，溢出时再说 |
| **决定** | ② |
| **理由** | ① 要活跃区间、冲突图、spill 代价 —— 而 `SSA-LIVE v1` 给的是**值级**区间，够算栈槽但不够算「谁该进寄存器」；② 帧大小 = Σ 槽宽，**纯函数**，可以用 `NAT-7` 逐偏移核对；③ 是「先错后调」的写法，寄存器不够时行为不确定 |
| **失效条件** | 若某个 fixture 的帧大到超出栈（例如 8 MB），说明单函数值数失控 —— 那时先查 `nvalues`，**不是**先加分配器 |

#### `D-M7-4` runtime 用 freestanding C

| 字段 | 内容 |
|---|---|
| **背景** | `print` 要整数转十进制、浮点转最短往返、字符串拼接、panic 文本。这些放哪？ |
| **选项** | ① 纯 arm64 汇编（M0 spike 的路子）；② freestanding C（`clang -ffreestanding -nostdlib -c`）；③ Rust `no_std` staticlib |
| **决定** | ② |
| **理由** | ① `ftoa` 的最短往返在汇编里写不出来（要 128 位中间精度 + 大整数回退），硬写等于把 M7 变成 M7+M9；③ Rust 的 `no_std` 仍会带 `panic_handler`/`core::fmt` 的边界，roadmap 明确「**不依赖 Rust unwind 跨 Go frame**」；② C 只依赖 clang 本身（`F3` 已装），`-ffreestanding` 下没有 libc 依赖，且**我们仍然是自研**（`PIPE-23/27` 委托的是 assembler/linker，不是 runtime） |
| **失效条件** | 若 `-ffreestanding` 下 clang 仍对某些构造发出 `memcpy`/`memset` 调用，**必须在 `rt.c` 里自己实现这两个**（§3.6），不许为此引入 libc |

#### `D-M7-5` 自定 ABI

| 字段 | 内容 |
|---|---|
| **背景** | 官方 Go 用 internal ABI（寄存器 + 栈偏移由编译器私有约定）；我们要不要跟？ |
| **选项** | ① 复刻 Go internal ABI；② 复刻 C 的 AAPCS64 全量（含 HFA 规则）；③ 自定子集，与 AAPCS64 兼容 |
| **决定** | ③ |
| **理由** | ① roadmap 明确「不用 Go internal ABI 作门槛」，且它没有稳定文档；② HFA/小聚合的规则我们**不需要**（`print` 的实参面不含复合值，`C9` §4.3），照抄只会引入没测过的分支；③ 子集 = 整数/指针走 `x0..x7`、浮点/复数走 `v0..v7`、**聚合一律按指针传**（调用方在自己的帧里放副本）、多返回值按同一顺序回填、`x8` 留给「调用方提供返回缓冲」。这样 runtime 的 C 函数可以直接被调（C 侧看到的正是 AAPCS64 的标量部分） |
| **失效条件** | 若 M9 的 iface 需要「按值传 16 字节聚合」，会与「聚合按指针」冲突 —— 那时**改 `C5` 并升版本**，不许在代码里悄悄特例 |

#### `D-M7-6` 块参数并行拷贝

| 字段 | 内容 |
|---|---|
| **背景** | `Term::Goto { to, args }` 要给目标块参数赋值。`a, b = b, a` 的边上是 `args=[b,a]`、`params=[a,b]` |
| **选项** | ① 逐条 `ldr/str` 搬；② 先全读进临时寄存器/临时槽，再全写；③ 检测交换环 + 只用 1 个临时 |
| **决定** | ②（③ 作为它的实现细节） |
| **理由** | ① 是 M6 记下的**同一个坑**（`D-M6-3` 的失效条件）：`a,b=b,a` 会静默错，而 90% 的 fixture 仍然绿；② 正确性与参数个数无关，代价是临时区；③ 只是省几条指令，**不作为正确性依据** |
| **失效条件** | 若临时区大小与帧布局耦合出「临时区盖住值槽」的错，症状是**偶发**——所以 `NAT-3` 必须有一条「交换环 + 多个值同时活跃」的 fixture，且用 `objdump` 核对偏移 |

#### `D-M7-7` 静态 arena + bump

| 字段 | 内容 |
|---|---|
| **背景** | `new(T)`（8 个文件，`F9`）、字符串拼接、复合字面量逃逸都需要分配 |
| **选项** | ① 不做分配 ⇒ 这些全拒；② 静态 arena + bump（不 free）；③ 调宿主 `malloc` |
| **决定** | ② |
| **理由** | ① 会把 `new` 的 8 个文件全砍掉，其中包含 `newexpr.go`/`gc1.go` 这类基础样例；③ 引入 libc 依赖，与 M0 spike 的「零 libc」结论（`F1`）冲突，也让「我们到底依赖了什么」变模糊；② 一个 `.bss` 块 + 一个指针 + 清零，**十行**，且正好是 roadmap「固定栈、无 GC」的字面形态 |
| **失效条件** | arena 耗尽必须**显式 panic**（`out of arena`），不许静默返回未初始化内存；`M9a` 会用 typed allocation 替换它，替换时 `C5` 的分配入口签名不变 |

#### `D-M7-8` 支持面实测切片

| 字段 | 内容 |
|---|---|
| **背景** | M6 交了 46 个能建图的文件（`F4`）。M7 要声明「native 支持切片」，怎么定？ |
| **选项** | ① 先定支持面，再挑文件；② 先实现，再按拒绝面实测分类 |
| **决定** | ②（沿 `D-M6-12`） |
| **理由** | 同 M6：① 会诱导「挑好看的文件」。② 得到三个可验的数（`N'` 能生成 / `M'` 显式拒绝 / `N'+M'=46`），且 `M'` 每一条都有码。代价是 `N'` 不可预测 |
| **失效条件** | 若 `N'` 小到撑不起 `NAT-4`（例如 < 30），如实报告并把拒项列 `open_gaps`，**不许**把拒项算成通过 |

#### `D-M7-10` 浮点最短往返

| 字段 | 内容 |
|---|---|
| **背景** | `C9` §4.3 规定浮点按 `strconv.FormatFloat(f,'g',-1,bits)`；解释器借 Rust 的 `{:e}`（`print.rs` §`format_float`）拿到最短往返。native 没有 Rust std |
| **选项** | ① 完整实现最短往返（Grisu/Ryu 类）；② 只实现「整值与短小数」，其余显式拒绝；③ 用固定 17 位有效数字再裁尾零 |
| **决定** | ① |
| **理由** | ② 会在 `0.1+0.2` 这类值上给出不同字节，而**语料里 42/46 是空输出**（`F5`），根本抓不到 —— 等于埋一个只在用户程序里爆的雷；③ 不是最短往返（`0.1` 会打成 `0.10000000000000001`），`C9` §4.4 的 `0.0001`/`1e-05` 直接红；① 只写一次，判据是**已经冻结的六个字节点**（`F12`），不需要新设计 |
| **失效条件** | 若实现出的最短往返在某个值上与解释器不一致，**以 `C9` §4.4 为准**并回写 `C9`；不许为了过测试改 `C9` |

#### `D-M7-12` `FRAME-MAP v1` 只到样例级

| 字段 | 内容 |
|---|---|
| **背景** | roadmap 要求 M7「验证 frame layout、live set/stack map 和保存区」，但对象模型在 M9 |
| **选项** | ① 只出帧布局，不出 stack map；② 出 `FRAME-MAP v1`（帧布局 + safepoint 活值偏移）；③ 连 GC 根位图一起出 |
| **决定** | ② |
| **理由** | ① 等于没交 roadmap 要的「扩展接口」；③ 需要对象边界与派生指针策略，属 M9c2；② 能把 `SSA-LIVE v1` 的**值**翻译成**偏移**，并让 `NAT-7` 用 `objdump` 核对 —— 这是「协议真的能落地」的唯一证据 |
| **失效条件** | 若某个 safepoint 的活值包含聚合（数组/struct），M7 只报它的**起始偏移与大小**，不承诺内部指针 —— 明确写进 `C5` |

### 2.3 事实基线（`F1..F18`，均为源码指认或实测）

| # | 事实 | 出处 |
|---|---|---|
| `F1` | M0 的 native spike **仍然全绿**：`s3_native_e2e` 10 条 + `s3_native_test` 16 条；生成器 427 行，从固定 HIR 生成 `.s`（不是手写汇编） | 2026-10-24 实测（`cargo test -p rgoc-spikes`） |
| `F2` | 链接命令实测 = `clang -nostartfiles -Wl,-s -o <elf> <asm.s>`；`-nostartfiles` **必需**（否则 `Scrt1.o` 的 `_start` 重复定义）；`-Wl,-s` **必需**（否则 `.strtab` 里 clang 的随机 `STT_FILE` 名让字节不可复现） | `native.rs::link_elf` 的文档（含三次复现证据） |
| `F3` | 容器工具链：`Debian clang 14.0.6`，target `aarch64-unknown-linux-gnu`；GNU `as`/`ld` 2.40；`file` / `readelf` / `objdump` / `llvm-objdump` 可用 | 2026-10-24 容器内实测 |
| `F4` | SSA 语料 **N=46 / M=23**（`R-6-1` 冻结）；`--via=hir` == `--via=ssa` 46/46；`--opt=all` == `none` 46/46 | `M6-manifest.json` |
| `F5` | **46 个里 42 个输出为空**（`.exp` 字节数 0）；只有 4 个有输出：`helloworld` 13 B、`printbig` 41 B、`ken/cplx0` 28 B、`ken/string` 49 B | 2026-10-24 实测（`.superpowers/m5-oracle-cache/*.exp`） |
| `F6` | **46 个里没有任何一个打印浮点**；`ken/cplx0` 打印 `(5+6i)`，其分量走 `format_float` | 同上 |
| `F7` | 文本扫描（**预估，非实测**）：`[]T` 波及 **6**、字符串拼接 4（其中 `string.go` 与 slice 重叠）、`complex` 3；排除 `[]T` 后剩 **40** | 2026-10-24 扫描（正则），**必须由 `NAT-4` 的分类器实测替换** |
| `F8` | 46 个共 **131 个 `func`**（平均 2.8，最多 12：`func.go`/`ken/array.go`/`ken/robfunc.go`） | 2026-10-24 计数 |
| `F9` | `new(` 出现在 **8** 个文件；`make(` 只在 2 个（都在 `[]T` 集合内）；`append`/`copy`/`delete` **0** 个 | 2026-10-24 扫描 |
| `F10` | `panic(` 出现在 **34** 个文件（都是测试自检，正常路径不触发） | 2026-10-24 扫描 |
| `F11` | SSA IR = **14 个 `Instr`**（`Const`/`Copy`/`Bin`/`Un`/`Convert`/`AddrOf`/`Field`/`Index`/`Slice`/`Composite`/`Make`/`Load`/`Store`/`Call`）+ **5 个 `Term`**（`Goto`/`If`/`Return`/`Panic`/`Unreachable`）；`BuiltinKind` **17** | `rgoc-ssa/src/ir.rs`、`rgoc-hir/src/types.rs` |
| `F12` | `C9` §4.4 已冻结**六个**输出字节点（`1e+06` / `0.0001` / `1e-05` / `1e-07` / `0.33333334` / `(-1-2i)`） | `C9-values-and-output.md` §4.4 |
| `F13` | `C9` §5 退出码：正常 `0` / 静态诊断 `1` / panic 与资源超限 `2`；`1` 时 stdout 不许有半截输出 | `C9` §5 |
| `F14` | `SsaFunc` 带 `value_types`（每值静态类型）与 `slot_types`（**取过址**的槽→类型，进帧按类型建零值） | `rgoc-ssa/src/ir.rs` §`SsaFunc` |
| `F15` | `SSA-LIVE v1` 已冻结：`live=[...] ptrs=[...]`、入口位点 `b<k>:-1`、safepoint = `Call` + `Make` | `M6-design.md` §3.7 |
| `F16` | workspace 现状：**176 suite / 1859 passed / 0 failed / 1 ignored**；`DiagCode` **103** | M6 收口实测 |
| `F17` | `rgoc-driver` 的 CLI **手写**，现有 7 个子命令（`harness`/`lex`/`ast`/`types`/`check`/`run`/`ssa`），**没有 `build`** | `rgoc-driver/src/cli.rs` |
| `F18` | 解释器的 `print` 格式表**逐条来自 go1.27.1 实测**（`print(1,2)`⇒`12`、`println("a","b")`⇒`a b\n`、`'A'`⇒`65`） | `rgoc-interp/src/print.rs` 模块文档 |

### 2.4 三条签字（2026-10-24 已确认）

> 这三条会改变 `M7-plan.md` 的形状，**先签字再拆任务**。

| # | 问题 | 选项 | **决定** | 理由 |
|---|---|---|---|---|
| `Q1` | runtime 用什么写 | ① 纯 arm64 汇编；② freestanding C；③ Rust `no_std` | **② freestanding C** | `ftoa` 在汇编里写不出来；Rust 带 unwind 边界，与 roadmap「不依赖 Rust unwind 跨 Go frame」冲突 |
| `Q2` | native 支持面 | ① 只拒 `[]T`（预估 **40 进 / 6 拒**）；② 再拒字符串拼接（36）；③ 再拒 complex（33） | **① 只拒 `[]T`** | ① 能保住**全部 4 个有输出的语料文件**（含唯一打印复数的 `cplx0` 与打印字符串的 `string.go`）—— 42/46 是空输出（`F5`），有输出的那几个才是判据的牙齿 |
| `Q3` | 浮点打印 | ① 完整最短往返；② 只做整值/短小数，其余拒绝；③ 固定 17 位再裁尾零 | **① 完整最短往返** | 判据已冻结（`F12`）；②③ 在空输出的语料上抓不到，只会在用户程序里爆 |

> `Q2` 的 40/6 是**文本扫描的预估**（`F7`），不是判据。`M7-tests.md` §6 把它写成
> 「待实测冻结」的占位，冻结值由 `T-NAT-*` 的分类器实测产出（沿 `R-6-1` 的做法）。

---

## 3. 架构与数据模型

### 3.1 流水线与 crate 变化

```text
AST ─► HIR ─► SSA ─► opt ─┬─► ssa_exec（M6 诊断求值）
                          └─► rgoc-arm64 ──► prog.s ─┐
                                                     ├─► clang -nostartfiles -Wl,-s ─► ELF
                          runtime/native/rt.c ───────┘        │
                                                              └─► 三方差分：ELF / hir / go run
```

| crate / 目录 | 变化 |
|---|---|
| `rgoc-arm64`（**新建**） | `abi`（`C5` 的寄存器与分类规则）/ `frame`（帧布局 + `FRAME-MAP v1`）/ `isel`（指令选择）/ `emit`（文本汇编）/ `diag`（`CgCode`） |
| `rgoc-driver` | 新增 `build` 子命令；`run` 加 `--via=native`（可选，调 `build` 再执行） |
| `runtime/native/aarch64-unknown-linux-gnu/` | `rt.c`（freestanding）+ `rt.h`；`README.md` 记六项桥接事实 |
| `rgoc-ssa` / `rgoc-interp` / `rgoc-hir` | **不改**（若发现 IR 缺信息，先补 SSA 并登记，`D-M7-1` 的失效条件） |

> **为什么 `rgoc-arm64` 不并进 `rgoc-ssa`**：SSA 的 verifier 与 passes 是**平台无关**的，一旦
> 把 `arm64` 塞进去，`C4`/`C10` 的契约就被一个具体 ISA 绑住。M9 若上第二平台，加 crate 即可。

### 3.2 调用约定（rgoc-AAPCS64 子集，`C5` 的正文）

| 项 | 约定 |
|---|---|
| 整数 / 指针 / bool 实参 | `x0..x7`，超出部分走**调用方帧**的 outgoing 区（`sp+0` 起，16 字节对齐） |
| 浮点实参 | `v0..v7`（**独立计数**，与 `x` 不共用序号） |
| complex 实参 | 占 **2 个 `v`**（`re` 低、`im` 高） |
| 聚合实参（数组/struct） | **一律按指针**：调用方在自己的帧里放副本，传地址（不跟 AAPCS64 的 by-value 小聚合规则） |
| 整数返回值 | `x0..x7` |
| 浮点返回值 | `v0..v7` |
| 多返回值 | 按结果顺序**依次**占用 `x`/`v` 两个序列 |
| 聚合返回值 | 调用方在**自己的帧**里留缓冲，地址进 `x8`（间接结果寄存器，M9 的 iface 直接用这个位） |
| 被调方保存 | `x19..x29` / `v8..v15` —— **生成代码一律不用**，所以序言里没有 save/restore；runtime 是 C 编译的，它用到的部分由 clang 自己按 AAPCS64 保存（两边不冲突，因为「不用」比「保存」更强） |
| `sp` | **恒 16 字节对齐**（进入函数时、每次 `bl` 前、每次 `svc` 前） |
| 栈方向 | 向下增长；序言 `sub sp, sp, #frame`，尾声 `add sp, sp, #frame` |

> **「不用 callee-saved」是刻意的**：`D-M7-3` 让所有值都在栈上，寄存器只是**指令内部的临时**，
> 活不过一条指令。于是「保存区」在 M7 只有 `fp`/`lr` 两项 —— 这正是 `NAT-7` 能逐偏移核对的前提。

### 3.3 帧布局

```text
高地址
  ┌──────────────────────────┐
  │ 调用方的 outgoing 实参区   │  ← 由调用方在自己的帧尾预留（16 对齐）
  ├──────────────────────────┤ ← fp (x29)
  │ saved fp                 │  +0
  │ saved lr                 │  +8
  ├──────────────────────────┤
  │ 值槽区（value_types 顺序） │  -8, -16, ...（按类型宽度与对齐）
  ├──────────────────────────┤
  │ 取址槽区（slot_types）     │  进帧时按类型清零（`F14`）
  ├──────────────────────────┤
  │ 块参数并行拷贝临时区        │  最大 = 单边实参个数 × 8
  └──────────────────────────┘ ← sp
低地址
```

- 槽宽：`bool`/整数/指针/浮点 = **8**；`complex64` = 8、`complex128` = 16；数组/struct = `size_of(typ)` 向上取 8 的倍数。
- **对齐**：一律 8 字节对齐（`align.go`/`zerosize.go` 在语料里，`NAT-6` 有专门 fixture）。
- **零值**：`slot_types` 里的槽进帧时清零；可提升的槽不建存储（M6 已提升成值）。

### 3.4 指令选择表（14 个 `Instr` + 5 个 `Term`，`NAT-2` 的分母）

| SSA | arm64 | 备注 |
|---|---|---|
| `Const`（整数） | `movz`/`movk` 序列 | 64 位常量最多 4 条 |
| `Const`（浮点/复数） | `.rodata` 里的 `.quad`/`.double` + `ldr` | 不拼立即数，避免精度争议 |
| `Const`（字符串） | `.rodata` 的**独立标签** + `(ptr,len)` | 标签按 `ValueId` 命名，逐字节可复现 |
| `Copy` | `ldr`/`str`（聚合走 `memcpy`） | |
| `Bin`（整数） | `add/sub/mul/sdiv/udiv/lsl/lsr/asr/and/orr/eor` + `cmp`+`cset` | `sdiv` 前插**除零检查** |
| `Bin`（浮点） | `fadd/fsub/fmul/fdiv` + `fcmp`+`cset` | |
| `Bin`（复数） | 展开成 4 个浮点运算（除法按 Smith 形式） | 与 `ops.rs` 逐式对齐 |
| `Un` | `neg`/`mvn`/`fneg` | |
| `Convert` | `sxtb/sxth/sxtw`/`and`/`scvtf`/`ucvtf`/`fcvtzs`/`fcvt` | 宽度按目标 `TypeId` 定 |
| `AddrOf` | `add x, sp, #off`（Slot）/ `adrp`+`add :lo12:`（Global） | PIE 安全形式，沿 `T-S3-01` |
| `Field` | `add x, base, #off` | 偏移来自 `rgoc-types` 的布局（**唯一来源**，不许自算） |
| `Index` | 乘元素宽 + `add` | 数组越界检查在 `Load/Store` 侧 |
| `Slice` | **拒**（`CG-UNSUPPORTED-SLICE`） | M9b |
| `Composite` | 逐元素 `str`（栈上）/ `memcpy`（聚合） | 零值先清零 |
| `Make` | **拒**（`CG-UNSUPPORTED-SLICE`） | `make` 只有 slice/map/chan 三种 |
| `Load` | `ldr`/`ldrb`/`ldrh`/`ldrsb`...（按 `typ`）；聚合 `memcpy` | 指针为 0 ⇒ **nil 检查** |
| `Store` | `str` 同族；聚合 `memcpy` | 同上 |
| `Call` | 装参 → `bl` → 取结果 | 间接调用在 M7 不可达（M6 已拒函数值） |
| `Term::Goto` | 并行拷贝 + `b` | §3.5 |
| `Term::If` | `cbnz` + 两条边的并行拷贝 | 边实参拷贝**必须在跳转前**完成 |
| `Term::Return` | 装结果 → `ldp` 恢复 `fp/lr` → `ret` | |
| `Term::Panic` | 调 `rt_panic_*`（**不返回**） | |
| `Term::Unreachable` | `brk #1` | 真到了就是 codegen 的错 |

### 3.5 块参数并行拷贝（`D-M7-6`）

算法（在**边**上执行，不在目标块里）：

1. 收集 `(目标槽, 源槽)` 对；源与目标都已经是**栈偏移**（`D-M7-3` 让这一步是纯算术）。
2. 先处理「源不是任何目标」的对（直接 `ldr`/`str`）。
3. 剩下的构成若干**环**；每个环用临时区里的一个槽打断：`t = a; a = b; b = t`。
4. 环检测用「目标 → 源」的单射图，规模 ≤ 边实参个数。

> ⚠️ 这是本设计**最隐蔽**的错误源（与 M6 的风险 1 同源）：漏了环检测时，`a,b=b,a` 得到 `(b,b)`，
> 而所有「不交换」的 fixture 仍然全绿。`NAT-3` 必须有一条**交换环 + 三个值轮换**的 fixture。

### 3.6 runtime（`rt.c`，freestanding）

| 函数 | 作用 | 判据 |
|---|---|---|
| `rt_write(fd, ptr, len)` | `svc #0`（`write` = 64） | `NAT-5` |
| `rt_exit(code)` | `svc #0`（`exit_group` = 94） | 退出码 |
| `rt_print_i64` / `rt_print_u64` | 十进制（含 `-`） | `printbig` 的 41 字节 |
| `rt_print_f64(f64, is32)` | **最短往返 + `%g` 判定**（`exp < -4 \|\| exp >= 6` 走指数，指数至少两位） | `C9` §4.4 六点 |
| `rt_print_complex(re, im)` | `(re+imi)` | `cplx0` 的 28 字节 |
| `rt_print_str(ptr, len)` | 原样字节 | `string.go` 的 49 字节 |
| `rt_print_bool` | `true`/`false` | |
| `rt_panic_i64` / `rt_panic_str` / `rt_panic_nil` | 写 `panic: <值>\n` 到 **fd 2**，`exit(2)` | `C9` §4.5 |
| `rt_div_zero` / `rt_nil_deref` / `rt_bounds` | 三个检查点的失败出口 | `NAT-8` |
| `memcpy` / `memset` | **必须自己实现**（`-ffreestanding` 下 clang 仍可能发这两个调用） | 链接时 `undefined reference` 即判据 |
| `rt_arena_alloc(size, align)` | bump + 清零；耗尽 ⇒ `panic: out of arena` | `NAT-6` |

编译：`clang -ffreestanding -nostdlib -O2 -c rt.c -o rt.o`，与 `prog.s` 一起链。
**不链 libc**：链接后 `readelf -d` 若出现非 `NEEDED` 的 libc 符号即红（沿 M0 spike 的 `F1` 口径）。

### 3.7 startup 与初始化顺序

```asm
_start:
  bl rt_init_arena        // 清 arena 指针
  bl init$0               // 按 Program::init_order
  bl init$1
  ...
  bl main
  mov x0, #0
  bl rt_exit              // exit_group(0)
```

- **顺序不许重排**：`Program::init_order` 是 HIR 决定的（M5），SSA 只做 `Module::init` 合成（`R-32-1`），
  codegen 只按序 `bl`。`NAT-3` 有一条「包变量依赖顺序」的 fixture。
- `main` 的返回：Go 的 `main` 无返回值 ⇒ 恒 `exit_group(0)`；若 `main` 走 `Term::Panic` ⇒ 退出码 2。

### 3.8 分配（`D-M7-7`）

```text
.bss: rt_arena[1 MiB]  /  rt_arena_used (u64)
rt_arena_alloc(size, align):
    p = align_up(rt_arena_used, align)
    if p + size > 1 MiB: rt_panic_str("out of arena")
    memset(rt_arena + p, 0, size)
    rt_arena_used = p + size
    return rt_arena + p
```

- `new(T)` / 逃逸的复合字面量 / 字符串拼接都走它。
- **不 free、不移动** ⇒ 「无 GC」的字面含义；`M9a` 用 typed allocation 替换时入口签名不变。

### 3.9 print / panic 的字节面（`D-M7-11`）

`C9` §4 的规则**原样适用**，native 只实现、不发明：

| 规则 | native 落点 |
|---|---|
| `print` 无分隔、`println` 空格分隔 + `\n`、末尾无空格 | 生成器按实参个数插 `rt_write(" ")` / `rt_write("\n")` |
| 整数按静态类型宽度打十进制 | `rt_print_i64`，宽度已在 `Convert` 阶段截断 |
| `'A'` / `byte(65)` ⇒ `65` | 同上（rune/byte 是整数） |
| 浮点 `%g` 最短往返 | `rt_print_f64`（`D-M7-10`） |
| 指针 / 非 nil 接口 / 非 nil slice ⇒ `0xc000000000` | **M7 不可达**（`print` 的实参面在 40 个语料里没有指针；`[]T` 已拒） |
| 数组 / struct ⇒ 静态面就拒 | `C9` §4.3 已定；native 不实现递归格式化 |

> **一处必须核对的细节**：`C9` §4.3 说 `[]T` 打印成 `[len/cap]0x...`，而 `D-M7-8` 把 `[]T` 拒掉了 ——
> 于是 native 的 `print` 实参面**不含** slice。若将来放开 `[]T`，`rt_print_slice` 要补，**不是**改 `C9`。

### 3.10 `FRAME-MAP v1`（`D-M7-12`）

逐函数一段确定性文本（沿 `SSA-LIVE v1` 的行文）：

```text
# FRAME-MAP v1
# func main frame=64 slots=7
# slot v0 off=-8 size=8 type=int
# slot v3 off=-16 size=16 type=complex128
# safepoint b1:0 offs=[-8,-24] ptrs=[]
```

- `frame` 是 `sub sp, sp, #frame` 的**实际数值**；`NAT-7` 用 `objdump` 反汇编核对。
- `safepoint` 的位点沿用 `SSA-LIVE v1`（`b<k>:<idx>`，入口 `-1`）。
- `offs` 是该位点活值的**偏移列表**；`ptrs` 只含指针类型（M7 的语料里通常为空）。
- 聚合只报**起始偏移 + 大小**（`D-M7-12` 的失效条件）。

---

## 4. 判据与装置

### 4.1 三方差分（`NAT-4`）

```text
rgoc build <file> -o prog  &&  ./prog        ==  rgoc run --via=hir  ==  go run（合并流 + 退出码）
唯一容差：\r\n → \n
```

装置：`run-collide.sh --native`（在 M6 的 `--ssa` 之外加第四列）。失败时保存：源文件、`.s`、两侧输出、期望、`objdump` 片段。

### 4.2 切片计算（`D-M7-8`）

从 `run-selected.txt` 的 **46** 个出发，逐个 `rgoc build`：能生成且能跑完 ⇒ 计入 `N'`；否则落 `M'` 并记**拒绝码**。
断言：`N' + M' == 46`（两向），且 `M'` 每一行的码都在 §5 白名单里。**反向守卫**：把任一 `M'` 里的文件硬塞进 `N'` ⇒ 必红。

> `F7` 的正则扫描只是**预估**（40/6）。实测与预估不一致时，**以实测为准**并把差异写进 `M7-report.md`。

### 4.3 可复现（`NAT-1`）

| 对象 | 判据 |
|---|---|
| `.s` | 同一份 SSA 生成两次，逐字节相同（生成器**不得**用哈希序、不得写时间戳/路径） |
| ELF | 同一份 `.s` 链接两次，逐字节相同（靠 `-Wl,-s`，`F2`） |
| 反例守卫 | 故意让生成器输出一个 `HashMap` 迭代序 ⇒ `NAT-1` 必红 |

### 4.4 资源与预算

| 项 | 值 |
|---|---|
| arena | 1 MiB（`D-M7-7`），耗尽 ⇒ `panic: out of arena` + 退出码 2 |
| 单函数值槽 | 无硬上限，但 `frame > 64 KiB` 记 `open_gaps`（不是错，是可疑） |
| 单文件编译墙钟 | 30 s（装置侧约束） |
| 单文件运行墙钟 | 10 s（沿 M5 口径） |

---

## 5. 不支持面（白名单，`D-M7-13`）

| ID | 拒绝什么 | 判据（生成期） | 码 | 正向证据 |
|---|---|---|---|---|
| `U-M7-01` | `[]T` 全套（`Instr::Slice` / `Instr::Make`） | 遇到这两个 `Instr` 变体 | `CG-UNSUPPORTED-SLICE` | `ken/slicearray.go` 等 6 个 |
| `U-M7-02` | 间接调用（`CalleeRef::Indirect`） | 遇到该 `CalleeRef` 变体 | `CG-UNSUPPORTED-INDIRECT-CALL` | 守卫测试（M6 已拒函数值，够不着） |
| `U-M7-03` | 方法值 / 接口动态派发 | 遇到 `CalleeRef::Method` 的 `recv` 不是静态类型 | `CG-UNSUPPORTED-METHOD-VALUE` | 守卫测试（M6 已拒） |
| `U-M7-04` | arena 耗尽 | 运行期 | `panic: out of arena` + 退出码 2 | `NAT-6` 的 fixture |

**纪律**：白名单项**不得**记为 pass；每条都有「一个 fixture + 一个码 + 一个退出码」三件套。
`U-M7-02/03` 在本前端**没有入口**（M6 的 `U-M6-01..09` 先拒），它们的正向证据是**守卫测试**：
断言该码**不出现**、且拒绝来自 M6 —— M6 一旦放行，守卫即红。

---

## 6. 错误处理

| 类 | 出口 | 形态 |
|---|---|---|
| 用户面（不支持） | `stderr` + 退出码 1 | `rgoc-arm64/src/diag.rs` 的 `CgCode`（`CG-UNSUPPORTED-*`），**crate-local**，沿 `SsaCode` 先例；`DiagCode` 保持 103（`D-M7-13`） |
| 工具链面（`clang` 失败） | `stderr` + 退出码 2 | 原样带出 `clang` 的 stderr，**不吞**（`F2` 的教训：吞掉就无法定位 `-nostartfiles` 那类错） |
| 运行期（检查失败） | `stderr` + 退出码 2 | `panic: <值>\n`（`C9` §4.5） |
| 开发者面（生成器内部矛盾） | `todo!()` + 注释指向门禁 | 不许用「生成一条近似指令」冒充未实现 |

---

## 7. 覆盖清单（roadmap §M7 ↔ 功能 ID）

| 功能 ID | 内容 | 归属 | 本文档落点 |
|---|---|---|---|
| `PIPE-19` | 指令选择 | ✅ M7 | §3.4 |
| `PIPE-20` | 寄存器分配（最小） | ◐ M7（**不做分配**，值全落栈槽） | §3.3 |
| `PIPE-21` | 栈帧布局与活跃性 | ◐ M7（**帧布局**；活跃性 M6 已交） | §3.3 / §3.10 |
| `PIPE-22` | 目标代码生成 | ✅ M7 | §3.4 |
| `PIPE-23` | 汇编 / 对象输出 | ❌ 不自研（委托 `clang`） | §1.3 |
| `PIPE-24` | 包初始化顺序 | ✅ M7（`_start` 的 `bl` 序列） | §3.7 |
| `PIPE-27` | 链接 | ❌ 不自研（委托 `clang`） | §1.3 |
| `SYS-04` | 单平台 ELF 产物 | ✅ M7 | §4.3 |
| `RT-MEM-06`（堆位图） | 依赖对象模型 | ❌ 不涉及 | §1.3 |

---

## 8. 风险登记（前三条是最可能的坑）

| # | 风险 | 为什么危险 | 兜底判据 |
|---|---|---|---|
| 1 | **42/46 语料是空输出**（`F5`） | 「46 个全绿」可能只证明「没崩」，不证明「算对了」 | 自有 fixture **必须带真输出**；`NAT-4` 的分母要单独报「有输出/无输出」两类 |
| 2 | **块参数并行拷贝漏环检测** | `a,b=b,a` 静默错，其余 fixture 全绿 | §3.5 的交换环 + 三轮换 fixture |
| 3 | **槽宽/对齐算错** | 相邻槽被覆盖，症状是「偶尔错」，且只在特定类型组合出现 | `NAT-7` 逐偏移与 `objdump` 核对 |
| 4 | 浮点最短往返实现错 | 只有特定数值才红（`1e-05` 边界、`0.1+0.2`） | `C9` §4.4 六点 + 随机值对撞解释器（性质测试） |
| 5 | 栈对齐错（`sp` 非 16 倍数） | 只在调 runtime 时崩，纯计算测试全绿 | `NAT-3` 的「调 runtime 的 fixture」+ `objdump` 看 `sub sp` 的立即数 |

---

## 9. 任务分解原则（`M7-plan.md` 的输入）

七个 Phase，估 **55–65 个 Task**：

| Phase | 内容 | 估 |
|---|---|---|
| 0 | 装置与契约：`C5` 骨架、`run-collide --native` 空壳、语料分类器（46 → N'/M'） | 6–7 |
| 1 | 帧与 ABI：`frame` 模块（槽分配、零值、`FRAME-MAP v1`）、并行拷贝 | 8–10 |
| 2 | 指令选择：14 个 `Instr` 逐个「先写会红的 fixture 再实现」 | 14–16 |
| 3 | runtime：`rt.c`（write/exit/itoa/ftoa/complex/panic/arena/memcpy） | 8–10 |
| 4 | startup 与链接：`_start`、init 顺序、`clang` 驱动、可复现 | 5–6 |
| 5 | 集成：三方差分、`build` 子命令、错误码、切片实测冻结 | 7–9 |
| 6 | 变异、回归、报告、manifest、文档回写 | 7–8 |

**铁律**：判据先行（先写会红的 fixture）；每 Task 收口即 commit；`Instr` 变体**不得**用通配分支
（加一个变体就编译失败，逼你决定它的位置）；`.s` 的生成必须是纯函数（不碰磁盘、不看环境）。

---

## 10. 契约与产物影响

| 产物 | 动作 |
|---|---|
| `docs/contracts/C5-abi.md` | **升正式**（spike 级 → MVP 级）：补调用约定（寄存器分类 / 聚合按指针 / 多返回值）、帧布局、`FRAME-MAP v1`；原文六项记录与可复现性证据保留 |
| `docs/contracts/C9-values-and-output.md` | 补一节：native 的 print/panic 与解释器**同一口径**（本文件 §3.9 的证据） |
| `docs/contracts/C4-ssa.md` / `C10` | **不改**（native 只消费，不改 IR 形状） |
| `docs/milestones/GAP-01.md` | 补 `PIPE-19/20/21/22/24` 的 M7 落点 |
| `docs/03-roadmap.md` / `AGENTS.md` / `docs/README.md` | M7 状态节（**只在收口时**改） |
| `M7-{tests,plan}.md` / `M7-manifest.json` / `M7-report.md` | 三件套 + 事实 + 交付 C |

---

## 11. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-24 | **初稿**：`D-M7-1..16`、`F1..F18`、`NAT-1..9`、白名单 5 条、风险 5 条；`Q1..Q3` 待签字 | 容器内实测（`F1`/`F3`/`F5`–`F10`）+ `M0-report.md` §P3 + `C9` §4/§5 + `M6-design.md` §3 |
| 2026-10-24 | **确认**：`Q1..Q3` 全部按建议签字（runtime = freestanding C / 支持面只拒 `[]T` / 浮点做完整最短往返）；白名单 5 → 4 条（删掉「不存在的 `%v`」占位项） | 用户签字；占位项与「拒绝必须有码」的纪律冲突（§5 纪律） |
| 2026-10-24 | `R-1-1`：ABI 契约改为**把 `C5-abi.md` 升正式**，不新建 `C11`（§1.1 输出⑨ / §3.2 / §10 同步改） | 开工 Task 1 时发现 C5 已存在且文件头写明「归属：M7 做 MVP」；新建等于同一件事写两遍，并让 C5 永远停在 spike 级 |
