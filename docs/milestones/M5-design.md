# M5 设计：HIR 与顺序解释器（顺序语义基线）

> **阶段 ID**：`M5`　|　**状态**：**待确认**（2026-10-22 起草）　|　**起草**：2026-10-22
> **文档索引**：[`../README.md`](../README.md)　|　**上游**：[`../03-roadmap.md`](../03-roadmap.md) §M5（**权威口径**）、[`M4-report.md`](./M4-report.md) §4/§12、[`../contracts/C3-hir.md`](../contracts/C3-hir.md)（spike 级，本阶段升正式）　|　**下游**：`M5-tests.md`（冻结判据）→ `M5-plan.md` + `M5-manifest.json`
>
> **本文件的权威口径**：`03-roadmap.md` §M5（含 §M5a–§M5f 六个独立验收子阶段）。`AGENTS.md` 的 M5 一句话
> （「HIR 与顺序解释器」）是它的**摘要**，不是全量。
>
> **本文档未确认前不冻结**；确认后走 §12 变更记录。
> 本轮头脑风暴的输入 = roadmap §M5 原文 + M4 三件套 + `M4-report.md` + **§2.3 的实测事实基线**
> （全部为 2026-10-22 在 `rgoc:dev`（go1.27.1 linux/arm64）容器与本地语料上实测，逐条带出处）。
>
> **四条已确认的方向**（用户 2026-10-22 头脑风暴裁定，见 `D-M5-1/2/3`）：
> ① 执行模型 = **显式帧栈**；② 类型来源 = **复用检查器会话**；③ M4 接受面缺口 = **M5 开工前置任务组**；
> ④ 本轮交付 = **三件套一次出齐**。

---

## 0. 本文档在流程中的位置

沿用 M1–M4 的三件套流程（`03-roadmap.md` §6.2）：

```text
M5-design.md（本文）──用户确认──► M5-tests.md（冻结「怎么算通过」）► M5-plan.md + M5-manifest.json（任务与机器可读事实）► 执行 ► 门禁
```

* 本文只回答 **做什么、边界在哪、为什么**；不写测试 ID 全表（`M5-tests.md`），不写任务顺序（`M5-plan.md`）。
* 本文的每个决策要么进 §2.1 决策表，要么进 §2.4 取舍；每条事实要么带**实测**，要么带**源码出处**，不接受推测句。
* **交付线切换**：M0–M4 是交付线 A（前端，`03` §0.1）。**M5 是交付线 B 的起点**（`rgoc run`）。
  这意味着 M5 的验收面从「诊断与判定」变成「**可观察行为**」：输出字节、退出码、初始化顺序。
  前端那套「位类对齐」判据在这里**不适用**，必须换一套（§4）。

---

## 1. 目标与非目标

### 1.0 推导：为什么 M5 是「顺序解释」而不是「能跑就行」

交付线 B（`03` §0.1）只有一句话：**`rgoc run`；基础值、函数、闭包、接口动态语义和初始化的定义子集**。
拆开看，M5 必须同时解决四件互不替代的事：

1. **HIR**：把 AST 变成**可执行**的中间表示。C3 契约的 §2 列了 M0 留下的四项待验证（多返回值、可寻址性、
   闭包捕获、panic/recover），它们至今**一项未验证**（`C3-hir.md` §3 的差距表 1–4 全部指向 M5）。
2. **值模型**：Go 的赋值语义不是一种，是**四种**（数组/结构体拷贝、slice 共享、map 引用、string 不可变）。
   这四种在宿主里长得完全不同，任何一种搞错都会让「看起来能跑」的程序在别处错（`F16`）。
3. **初始化执行**：M4 只算图和顺序（`D-M4-7`），并且 `M4-report.md` §2.4 明确写了
   「静态图正确不能替代运行正确」。M5 必须真的按 **import → 包变量 → `init` → `main`** 跑。
4. **异常语义**：`panic/recover/defer` **不等同于 Rust `Result` 传播**（`C3` §2.4 的对照表）。
   defer 的参数在**登记时**求值、LIFO、recover 只在 defer 的直接调用里有效、命名返回值可被 recover 改写
   —— 这四条每一条都能单独写错，且都能在「普通函数调用都对」的前提下错。

**所以 M5 的产物不是「一个解释器」，而是「一套被证明过的顺序语义基线」**（roadmap 的「交付 B」）。
「能跑 hello」只是它的第一行证据，不是它本身。

### 1.1 M5 的输入与输出

| 类 | 内容 |
|---|---|
| 输入 ① | M4 的七条正式 crate（`lex → ast → parse → const → types → load → check`）+ `rgoc-driver`；C1/C2/C7/C8 契约 |
| 输入 ② | **`rgoc-check::Session` 的逐节点入口**（`F7`）—— 本阶段把它从 `pub(crate)` 开成 `pub`，成为**类型的唯一来源** |
| 输入 ③ | `rgoc-hir`（**SPIKE-ONLY**，4 文件 1087 行，`F8`）+ `C3` 契约的 §2 四项待验证 |
| 输入 ④ | `go_source_code/test` + `test/ken` 语料（只读）；容器内 `go1.27.1`（`go run` 作 oracle） |
| 输出 ① | **正式 `rgoc-hir`**（整体重写，替代 spike）：可执行 HIR + 中性 dump |
| 输出 ② | **新 crate `rgoc-interp`**：值模型、显式帧栈、求值机、内建、异常 |
| 输出 ③ | `rgoc-driver run <files…>` 子命令（输出通道 / 退出码 / `--dump-hir`） |
| 输出 ④ | **`rgoc-check` 的接受面收口**（`D-M5-3`：P0 任务组，把 69/80 打到 79/80） |
| 输出 ⑤ | `tools/run-oracle`（官方 testdir 语义复刻：合并流 + `.out`/空）+ `run-collide.sh --run` 形态 + 语料两清单 |
| 输出 ⑥ | **契约 `C3` 升正式** + **新契约 `C9`（值表示 / 输出字节 / 退出码）**；`M5-tests.md` / `M5-plan.md` / `M5-manifest.json` |
| 输出 ⑦ | **交付 B 报告 `M5-report.md`**（顺序语义基线：覆盖矩阵 + 三条分别声明 + 缺口） |

### 1.2 M5 范围（以 `03-roadmap.md` §M5 为权威，逐句展开）

roadmap §M5 原文五条（下划线为原文短语）：

| # | roadmap §M5 句子 | M5 落点（本设计） |
|---|---|---|
| 1 | 「**HIR lowering、环境/调用帧、按值/按引用捕获、可寻址性、多返回值**」 | `rgoc-hir`：`Place`（可寻址位置）显式建模；`Func` 的帧布局（形参/命名返回值/局部/临时）在 lowering 期一次定死；闭包捕获**按变量**（`Cell` 共享），不是按值；多返回值是**语言级 tuple**（`Vec<Value>`），不是结构体 |
| 2 | 「**基础值、数组/struct 拷贝、指针、slice 共享底层存储、不可变 string、Go map 语义适配**」 | `rgoc-interp` 值模型（§3.3）：数组/结构体 = `Rc<Vec<Cell>>` + **赋值即深拷贝**；slice = `{arr, off, len, cap}` 共享 `Rc`；string = `Rc<str>` 不可变；map = `Rc<RefCell<MapObj>>`，键按 Go 相等性（NaN≠NaN、结构体按值、接口按 (类型,值)） |
| 3 | 「**内建、接口动态值/方法调用/断言/type switch、typed nil、method value 和闭包**」 | 内建全集（§3.5 表）；接口 = `{typ: Option<TypeId>, data: Value}`，**typed nil ≠ nil interface**（`T-EX-*` 有正反 fixture）；方法值 = 绑定接收者的闭包；type switch 按动态类型逐 case |
| 4 | 「**解释执行包变量/init/main；defer/panic/recover：直接调用限制、命名返回值、嵌套 panic 必须逐行为测试**」 | 初始化执行（§3.6）：`import → 包变量（InitOrder）→ init（文件序）→ main`；异常（§3.7）：defer 参数**登记时求值**、LIFO、`recover` 仅在 defer 的直接调用帧有效、命名返回值可被改写、嵌套 panic 逐行为测试 |
| 5 | 「**泛型解释子集可单态化；loopvar 捕获语义直接测试。range over func（PIPE-16b）可在 M8d 补齐，未支持时显式拒绝**」 | 泛型：**按 `(origin, args)` 单态化**（消费 M4 的 `instantiate`），未支持的形态**显式拒绝**（稳定码）；loopvar：`go1.22` 语义（每轮新建变量），**直接测试**；range over func **显式拒绝**（`U-M5-*`），不假装支持 |

**roadmap §M5 的「不做」与门禁原文**：

* 「**不做**：goroutine/channel/select、Go GC、多级分配器、完整标准库；宿主管理内存但仍监控运行预算。」
  ⇒ §1.3 表逐项登记；**「监控运行预算」是硬要求**（§3.8：帧深/求值深/步数三道上限，超限**响亮失败**而不是 OOM 或挂死）。
* 「**门禁**：无标准库/无并发的 hello、基础语言和 `ken` 筛选子集，自有接口/闭包/init/defer fixture；milestone 全绿。
  计划样本目标 **60–100** 个，最终冻结 ID 数以 M4 支持能力为准，**数量不足不以复制用例凑数**。」
  ⇒ 实测候选 **80**（`F1`），落在 60–100 区间内；入选数以 P0 收口后的实测为准（§4.4），**不补数**。
* 「**交付 B**：顺序语义基线；此时只是**冻结候选**，正式冻结等 M8e；后续增加已声明子集时保持测试，不假装永久不改。」
  ⇒ `M5-report.md` 的措辞必须写「**候选**」，不得写「冻结」。

**六个独立验收子阶段（roadmap §M5 表）→ 本设计的门禁映射**：

| 子阶段 | roadmap 退出条件 | 本设计门禁 |
|---|---|---|
| M5a：HIR 与最小求值 | Source→HIR→输出贯通；常量/算术/分支 fixture 全绿，未支持 HIR 节点显式拒绝 | `EX-1`、`EX-7` |
| M5b：函数、帧与初始化 | 函数/递归/多返回值及依赖包 init→main fixture 全绿；调用和初始化顺序可检查 | `EX-2` |
| M5c：复合值与内建 | 数组/struct 拷贝、slice 别名、string/map 和内建的正反 fixture 全绿 | `EX-3` |
| M5d：闭包与接口 | 捕获生命周期、loopvar、method value、typed nil、断言/type switch fixture 全绿 | `EX-4` |
| M5e：defer/panic/recover | 延迟参数求值、LIFO、命名返回值、合法 recover 与嵌套 panic fixture 全绿 | `EX-5` |
| M5f：顺序基线集成 | 冻结顺序语料 ID 全绿；不支持项、资源/超时结果和三方差异均可分类 | `EX-6`、`EX-8`、`EX-9` |

### 1.3 明确不做（M5 边界）

| 项 | 去向 | 依据 |
|---|---|---|
| goroutine / channel / select / `go` 语句 / `range` over channel | M8a–M8c | roadmap §M5「不做」 |
| Go GC、多级分配器、堆统计与 pacing | M9/M10 | roadmap §0.1 交付线 D |
| 完整标准库（`fmt` / `os` / `strings` / `reflect` …） | 首发不做；按明确包清单逐步加入 | roadmap §0.2 |
| `unsafe.Pointer` 转换 / `uintptr` 往返 / 内存访问 | M6+（`U-M4-06` 承接） | roadmap §0.2 |
| range over func（`PIPE-16b`） | M8d（未支持时**显式拒绝**） | roadmap §M5 第 5 条 |
| 未捕获 panic 的 **goroutine traceback 格式** | 不承诺（我们只承诺 `panic: <值>` 前缀与退出码） | 实现细节，`F18` |
| map 迭代顺序与官方**一致** | 不承诺（我们固定插入序；官方是随机序） | `U-M5-*`（§4.5） |
| 指针/地址的**数值** | 不承诺（地址不可复现，`F15` 实测 `0x2fcee60e0e90`） | `U-M5-*` |
| 求值顺序（`EXP-27`）的 codegen 面 | M6+ | roadmap §5 归属表 |
| `-race` / cgo / 汇编 / 插件 / wasm | 首发非目标 | roadmap §0.2 |

### 1.4 M5 退出条件（逐条可判定；门禁编号 `EX-*`，`M5-tests.md` 定稿）

| # | 退出条件 | 判据 |
|---|---|---|
| EX-1 | **HIR 与 lowering 闭环** | 自有 fixture 覆盖 HIR 每个节点与每个 `Place` 形态；**未支持节点显式拒绝**（稳定码，不是 `todo!`）；lowering 期诊断流**必须为空**（两遍一致性守卫，`D-M5-2`）；`hir --dump` 两次运行逐字节相同 |
| EX-2 | **函数、帧与初始化** | 递归/多返回值/变参/命名返回值 fixture 全绿；单包 `init` 序与 `InitOrder` 逐行一致；**双包 fixture**（源码 import）`import→var→init→main` 顺序可检查 |
| EX-3 | **复合值与内建** | 数组/结构体**拷贝**、slice **别名**、string **不可变**、map **键相等**四组各有正反 fixture；内建全集逐项有 fixture |
| EX-4 | **闭包与接口** | 捕获生命周期、loopvar（1.22）、method value、typed nil、断言、type switch 各有一组 fixture |
| EX-5 | **defer/panic/recover** | 参数登记时求值、LIFO、命名返回值改写、recover 直接调用限制、嵌套 panic 各有一组 fixture |
| EX-6 | **顺序语料闭环** | 入选清单**全绿**（逐字节：合并流 vs `.out`/空 + 退出码 0）；排除清单每行有理由；候选 = 入选 ∪ 排除（两向断言） |
| EX-7 | **显式帧与资源上限** | 帧栈/求值栈/步数三道上限各自**响亮失败**（稳定退出码 + 诊断，不 panic 宿主、不 OOM）；1000 层递归 fixture 通过（证明语义不依赖 Rust 调用栈） |
| EX-8 | **四条 cargo + 既有自检** | `fmt --check` / `check` / `clippy -D warnings` / `test --workspace`；M0/M1/M2 五条自检脚本不回退；**M4 的 63 条 `T-CK-*` 全绿不回退** |
| EX-9 | **反向验证 + 交付 B** | 变异探针 ≥ 8 条（覆盖值模型、帧、异常、判据装置）；`scripts/mutation-m5-notes.md`；`M5-report.md`（顺序语义基线，措辞为「候选」） |

---

## 2. 已确认决策记录

### 2.1 决策一览

| ID | 决策 | 结论 | 来源 | 日期 |
|---|---|---|---|---|
| D-M5-1 | **执行模型** | **显式帧栈**：调用/返回/defer/panic 全走 `Vec<Frame>`；**表达式求值也用显式任务栈**，不许用 Rust 递归承载语义 | 用户 | 2026-10-22 |
| D-M5-2 | **类型来源** | `rgoc-check::Session` 的逐节点入口开成 `pub`，lowering **持同一会话**取 `Operand`；**不建 Info 表、不编号 AST、不重推类型** | 用户 | 2026-10-22 |
| D-M5-3 | **M4 接受面缺口归属** | M5 **P0 前置任务组**：修 4 族规则把 69/80 打到 **79/80**（`reflect` 那 1 个按范围外登记），再进 M5a | 用户 | 2026-10-22 |
| D-M5-4 | **HIR 落点** | `rgoc-hir` **正式重写**；spike 版迁到新 crate **`rgoc-hir-spike`**（保住 `SM-M0-NATIVE-HELLO`） | **我方决定** | 2026-10-22 |
| D-M5-5 | **值模型** | 四类赋值语义各用不同宿主表示（§3.3）；变量一律 `Cell = Rc<RefCell<Value>>`，指针 = `Ptr(Cell)` | **我方决定** | 2026-10-22 |
| D-M5-6 | **输出面** | `print/println` 语义按 `F15` 实测表钉死；输出**合并流**（stdout+stderr 同序） | **我方决定** | 2026-10-22 |
| D-M5-7 | **语料判据** | 复刻官方 testdir `run` 语义（`F4`）：合并流 vs `.out`，无 `.out` 要求空；`runoutput` **排除** | **我方决定** | 2026-10-22 |
| D-M5-8 | **语料清单** | 候选 **80**（`F1`）→ 入选/排除两清单，逐行理由；**不补数**、不移出凑绿 | **我方决定** | 2026-10-22 |
| D-M5-9 | **异常语义** | defer 参数登记时求值 / LIFO / recover 直接调用限制 / 命名返回值改写 / 嵌套 panic 逐行为；未捕获 panic = `panic: <值>` + 退出码 2 | **我方决定** | 2026-10-22 |
| D-M5-10 | **初始化执行** | 单包：`InitOrder` → `init`（文件序）→ `main`；双包：自有 fixture 验证 `import→var→init→main` | **我方决定** | 2026-10-22 |
| D-M5-11 | **泛型** | 按 `(origin,args)` **单态化**（消费 M4 `instantiate`）；超出面**显式拒绝**；`range` over func 显式拒绝 | **我方决定** | 2026-10-22 |
| D-M5-12 | **资源上限** | 帧深 1000 / 求值栈深 10000 / 步数预算 1e8（`M5-tests.md` 冻结）；超限 = **稳定退出码 + 诊断**，不是宿主 panic | **我方决定** | 2026-10-22 |
| D-M5-13 | **契约** | `C3` **升正式**（HIR 形状）；**新建 `C9`**（值表示 / 输出字节 / 退出码）；`C1/C2/C7/C8` **预期不动** | **我方决定** | 2026-10-22 |
| D-M5-14 | **报告** | `M5-report.md` = 顺序语义基线（**候选**，正式冻结等 M8e）+ 三条分别声明（泛型/loopvar/range-over-func） | **我方决定** | 2026-10-22 |
| D-M5-15 | **反向验证** | 沿用 M3/M4（`cargo clean -p`、锚点先 `rg` 定位）；新增变异点：值模型拷贝语义、帧栈、defer 求值时机、判据装置本身 | **我方决定** | 2026-10-22 |

### 2.2 五字段记录（对每个**非显然**决策展开）

#### D-M5-1　执行模型：显式帧栈（含表达式求值）

| 字段 | 内容 |
|---|---|
| **判据** | `03-roadmap.md` §M8a：「**可暂停/恢复设计须在 M5/M6 契约留位置，不能让递归 Rust 调用栈成为唯一状态**」；§7 风险表「解释器难以暂停恢复：高；channel 阻塞依赖 Rust 递归栈」 |
| **候选** | ① Rust 递归调用（`eval_call` 直接递归）② 显式帧栈 + 递归表达式求值 ③ **显式帧栈 + 显式求值栈**（全程状态机） |
| **结论** | **③**。`Interp { frames: Vec<Frame>, eval: Vec<EvalTask>, values: Vec<Value>, panic: Option<PanicState> }`——**所有语义状态都是可枚举数据**；Rust 调用栈只承载「主循环的一步」，不承载 Go 的调用深度。理由：① 挂起点不止在语句级（`x := <-ch` 是**表达式内**的挂起），若表达式求值递归，M8 仍要重写求值器；② 递归深度上限从「Rust 栈深度（不可控、平台相关）」变成「我们自己的计数（可测、可报错）」；③ M6 的 SSA 求值器可复用同一套 `Value` + 操作语义 |
| **否决理由** | ① 直接违反 §M8a，且 M8 要整体重写；② 是「半留位」——看起来留了，`<-ch` 一来就撞墙；而且「递归深度超限」在 ② 里无法响亮失败（先爆 Rust 栈） |
| **失效条件** | 若实测显示 ③ 的性能使 milestone 批次超 30 min 预算（`F20` 的当前基线是 70 s 级，余量很大）⇒ 可**局部**改回递归，但必须保留「1000 层递归 fixture 通过」这条判据（`EX-7`），且 M8 前补齐 |

#### D-M5-2　类型来源：复用检查器会话

| 字段 | 内容 |
|---|---|
| **判据** | 实测（`F6`）：`CheckedPackage` **只有 `diags`**（`rgoc-check/src/check.rs:185-209`），不暴露任何类型表。而 lowering 的每一步都要类型：`a + b` 的结果类型、`x.f` 的字段偏移、`v.(T)` 的目标类型、`f()` 的返回 tuple 形状 |
| **候选** | ① 新增 `Info` 表 + AST 节点编号 pass（照 `go/types`）② **复用 `Session` 的逐节点入口** ③ lowering 自己重推类型 |
| **结论** | **②**。`Session::expr(file, e) -> Operand` 已存在（`expr.rs:175`），`expr_hint`（:184）、`type_expr`（:953）、`assignment`（:563）、`convert_untyped`（:2242）同理——**它们本来就返回带类型的 `Operand`**。M5 只做两件事：把这些入口从 `pub(crate)` 开成 `pub`；让 lowering 持 `&mut Session` 逐节点调用。**零 Info 表、零节点编号、类型规则只有一套** |
| **否决理由** | ① 要改 `rgoc-ast` 全线构造器与 `rgoc-parse`（M2 已冻结、31 条 golden 逐字节对撞），回归面极大；③ 等于第二套类型规则 —— roadmap §7「shared lowering 共同错误」风险直接翻倍 |
| **配套纪律（关键）** | **两遍结构**：`rgoc run` 先 `check_package` 出诊断（**有诊断即拒跑**），再 `lower`（lowering 内部新建**静默会话**只取类型）。**守卫**：lowering 期间静默会话的诊断流**必须为空**；非空 ⇒ 内部错（说明两遍不一致）。这条守卫让「复用会话」不是「绕过检查」，而是「同一规则的第二次消费」 |
| **失效条件** | 若某个 lowering 分支**拿不到**它需要的类型（例如 M4 的检查器根本没给某个形态算类型，如 `make(map[T]V)` 的底层类型，`F3-D3`）⇒ 该形态**先修检查器**（归 P0 任务组），**不许**在 lowering 里补一套推导 |

#### D-M5-3　M4 接受面缺口：M5 开工前置任务组

| 字段 | 内容 |
|---|---|
| **判据** | 实测（`F2`/`F3`）：80 个候选里 `rgoc check` **接受 69、拒绝 11**；11 个的根因只有 5 族，其中**4 族是真 bug**（第 5 族是 `import "reflect"`，范围外）。而 A 族（多值调用）与 B 族（调用语句结果丢弃）**正是 M5 的核心语义面** —— 不修它们，lowering 连 `b, x := f()` 和 `f()` 都过不去 |
| **候选** | ① M5 开工前置任务组 ② 先做 M4 收口专项（`G-M4-37` 五桶全量）③ run 路径绕过 check |
| **结论** | **①**。P0 任务组只修**接受面**（把合法程序误判为非法的 4 族），**不碰** `G-M4-37` 的拒绝面（58 个红文件里那些「该报没报」的）。目标 **79/80**（`reflect` 那个按范围外登记，`U-M5-*`）。理由：这 4 族在 `M4-report.md` §12.2 的性价比排序里正好是**第 1、2 位**（赋值/转换矩阵、类型参数核心类型），本来就是下一批该做的 |
| **否决理由** | ② M4 是 6–12 人周的活（roadmap §6.1），M5 会被无限期推后，且 M4 的拒绝面与 M5 的执行面**没有依赖关系**；③ 等于放弃「静态面与执行面同源」，M6 SSA 会立刻吃到同一批错误 |
| **失效条件** | 若某族修起来牵出**成片**规则（例如 D3 的 `make(map[T]V)` 实际是类型参数核心类型的整块缺失）⇒ 该文件**移入排除清单并写理由**，**不**为了让数字好看去改判据；`EX-6` 的判据是「入选全绿」，不是「80 全绿」 |

#### D-M5-4　HIR 落点：正式重写 + spike 改名

| 字段 | 内容 |
|---|---|
| **判据** | `rgoc-hir/src/lib.rs` 头写明「**SPIKE-ONLY，M5 整体替换**」（`F8`）；但**唯一消费者** `rgoc-spikes` 承载 `SM-M0-NATIVE-HELLO`（M1 起就是 smoke 回归项，`03` §M1 smoke 表），**不能删** |
| **候选** | ① 直接重写 `rgoc-hir`（会打断 spikes，smoke 红）② 新 crate `rgoc-hir2`（命名烂）③ **spike 改名 `rgoc-hir-spike`，`rgoc-hir` 腾出来做正式 HIR** |
| **结论** | **③**。`git mv crates/rgoc-hir crates/rgoc-hir-spike` + 改 `rgoc-spikes/Cargo.toml` 一行 + 4 个文件的 `rgoc_hir::` 引用；然后新建 `rgoc-hir`。验证：`cargo test -p rgoc-spikes` 全绿 + `SM-M0-NATIVE-HELLO` 仍通过 |
| **否决理由** | ① 打破 M1 起的 smoke 回归（`EX-8` 会红）；② 名字会永久留疤，且 `C3` 契约指向的是 `rgoc-hir` |
| **失效条件** | 无。这是一次机械迁移，判据是「既有测试一条不改、全绿」 |

#### D-M5-5　值模型：四类赋值语义各用不同表示

| 字段 | 内容 |
|---|---|
| **判据** | `03` §M5 第 2 条逐项点名：数组/struct **拷贝**、slice **共享底层**、string **不可变**、map **语义适配**。这四种在宿主里**必须长得不一样**，否则必有一项错 |
| **候选** | ① 全部用 `Rc` + 写时复制 ② 全部深拷贝 ③ **按 Go 语义分别表示**（§3.3） |
| **结论** | **③**：数组/结构体 = `Rc<Vec<Cell>>` 且**赋值即深拷贝**（新建 `Rc`，不是 COW）；slice = `{arr: Rc<Vec<Cell>>, off, len, cap}`（赋值只复制三元组）；string = `Rc<str>`；map = `Rc<RefCell<MapObj>>`；指针 = `Ptr(Cell)`；变量 = `Cell` |
| **否决理由** | ① COW 在「取地址 + 修改」下会出可观察差异（`&a[0]` 后再赋值 `b := a`），是经典陷阱；② 全部深拷贝会把 slice 的**别名语义**做没（`s2 := s1; s2[0]=1` 必须影响 `s1`） |
| **失效条件** | 若某组 fixture 显示表示选错（例如 `Cell` 树让 `&s[i]` 与 `s[i]` 不一致）⇒ 改表示，**不改判据** |

#### D-M5-7　语料判据：复刻官方 testdir 的 `run` 语义

| 字段 | 内容 |
|---|---|
| **判据** | 实测（`F4`）：官方 `testdir_test.go:645-646` 把 **stdout 与 stderr 写进同一个 buffer**；`:1172-1193` 把合并结果与 `<同名>.out` **逐字节**比较，**无 `.out` 则要求输出为空**；`case "run"` 在 `:1054`，`runoutput` 在 `:1092` |
| **候选** | ① 只比 stdout（简单，但 `print/println` 全写 stderr ⇒ 27+20 个文件的期望全落空）② 分别比 stdout 与 stderr（顺序信息丢失）③ **合并流逐字节** |
| **结论** | **③**。合并流**保序**（我们自己的 buffer 按写入顺序 append，天然保序）；退出码必须 0（官方 `runcmd` 返回 err ⇒ 测试失败）。**`runoutput`（5 个，`F5`）排除**：它要求「跑第一段生成第二段源码再编译」，那是动态编译，M5 不做 |
| **否决理由** | ① 会让 47 个用 `print/println` 的文件全部「输出为空」误判；② 丢掉交错顺序（`print("a"); println(1)` 的语义就在顺序里） |
| **失效条件** | 若某文件输出含**不可复现内容**（地址 `F15` 实测 `0x2fcee60e0e90`、map 随机序）⇒ 该文件进排除清单（理由：不确定性），**不许**做「模糊匹配」 |

### 2.3 事实基线（全部为 2026-10-22 实测或源码指认）

> 记号：**【测】**= 本日在 `rgoc:dev`（go1.27.1 linux/arm64）实测；**【源】**= 源码指认（文件:行）。
> 设计期探针在 `.superpowers/m5-probe/`（不入库，可重放）。

| # | 事实 | 出处 |
|---|---|---|
| F1 | **语料分母**：`go_source_code/test/` 顶层 + `test/ken/` 共 **396** 个 `.go`；带 `// run` 或 `// runoutput` 指令的 **205**；其中**无 import** 的 **96**；再排除含 `go`/`chan`/`select`/`sync.`/`time.` 的 ⇒ **候选 80**（顶层 50 + `ken/` 30） | 【测】清单见 `M5-tests.md` §6 |
| F2 | **`rgoc check` 接受率：69 / 80**（11 被拒） | 【测】`.superpowers/m5-check.txt` |
| F3 | **11 个拒绝的 5 族根因**：<br>**A 多值调用**（2 文件）：`method5.go:103/121`「2 variables but f returns 1 value」、`simassign.go:75`「not enough arguments in call to swap」<br>**B 调用语句结果丢弃**（3 文件，7 行）：`divmod.go:126/127/143`、`escape3.go:16/17/18`、`ken/rob1.go:64` —— 全是 `f()` 当语句，被误报 `CHK-UNUSED-EXPR`<br>**C `&T{...}` 取址**（2 文件，3 行）：`compos.go:16`、`ddd.go:189/222`「cannot take address of T」<br>**D 三小族**（3 文件，13 行）：`ken/shift.go:82/83` 移位计数为 `uint` 被误判类型不匹配；`convert4.go:73/76` slice→数组指针转换（1.17）被当作赋值；`bigmap.go:43..87`（9 行）`make(map[T]V)` 的底层同一<br>**E 范围外**（1 文件）：`reflectmethod2.go` `import "reflect"` | 【测】逐文件 `rgoc-driver check` 输出（`F3` 的行号即报点） |
| F4 | **官方 testdir `run` 判据**：stdout 与 stderr **同一 buffer**（`testdir_test.go:645-646`）；`checkExpectedOutput`（`:1172-1193`）= 与 `.out` 逐字节，**无 `.out` 要求空**，`\r\n` 归一为 `\n`；`case "run"`（`:1054`）、`case "runoutput"`（`:1092`）；退出码非 0 ⇒ 直接失败 | 【源】`go_source_code/src/cmd/internal/testdir/testdir_test.go` |
| F5 | **`runoutput` 共 5 个**：`index0.go`、`rotate0.go`、`rotate1.go`、`rotate2.go`、`rotate3.go` —— 语义是「跑第一段把输出写成第二段源码再编译运行」，**M5 不做** | 【测】+【源】`:1092` |
| F6 | **`CheckedPackage` 只暴露 `diags`**（`rgoc-check/src/check.rs:185-209`），无任何类型表；`check_package`（`check.rs`）四段流程 = `constfold` → `main_check` → `expr::check_package_decls` → `stmt::check_package_bodies` → `initorder::analyze` 的环 | 【源】 |
| F7 | **`Session` 的逐节点入口已存在但为 `pub(crate)`**：`expr`（`expr.rs:175`）、`expr_hint`（:184）、`conversion`（:389）、`assignment`（:563）、`valid_index`（:898）、`type_expr`（:953）、`convert_untyped`（:2242）；`Session`/`CheckOptions` 已 `pub use`（`lib.rs`） | 【源】 |
| F8 | **`rgoc-hir` 是 SPIKE-ONLY**（4 文件 1087 行：`hir.rs`/`value.rs`/`diag.rs`/`lib.rs`）；**唯一消费者**是 `rgoc-spikes`（`Cargo.toml:13`）；`SM-M0-NATIVE-HELLO` 依赖 `rgoc-spikes::fixtures::s3_hello()` | 【源】+【测】`cargo metadata` |
| F9 | **CLI 真身 = `rgoc-driver`**（`tools/types-oracle/run-collide.sh:270`、`:218`）；文档里的 `rgoc check` 是简称。M5 的 `run` 是它的新子命令 | 【源】 |
| F10 | **`print/println` 输出格式**（`od -c` 级实测）：`println(1,-2,true,"s",3.5,1e100,1.0)` ⇒ `1 -2 true s 3.5 1e+100 1`（浮点 = `%g`/精度 -1；`1.0` 打成 `1`）；`print("a",1,"b")` ⇒ `a1b`（无分隔）；`println()` ⇒ 空行；nil 指针 ⇒ `0x0`；`&x` ⇒ `0x2fcee60e0e90`（**不可复现**）；nil 接口 ⇒ `(0x0,0x0)`；nil slice ⇒ `[0/0]0x0`；nil map ⇒ `0x0`；rune ⇒ 十进制整数（`'c'` ⇒ `99`）；`uint64` 全值；`int8(-1)` ⇒ `-1` | 【测】`.superpowers/m5-probe/p.go` |
| F11 | **`print/println` 的参数类型受限**：`println(S{1})` 官方报 `illegal types for operand: println`（结构体/数组**不允许**）；**`rgoc check` 不报**（诊断 0，exit 0）—— 这是 M4 的**拒绝面**缺口，不阻塞 M5（`U-M5-*` 登记） | 【测】`.superpowers/m5-probe/q.go` |
| F12 | **80 个候选全部是单文件单包、零 import**（所以 M5 的包面只需「单包」，多包用自有 fixture） | 【测】`F1` 的筛选口径 |
| F13 | **候选的构造分布**：含 `panic` 字符串 64 / 含 `defer` 9 / 含 `recover` 7 / 含 `interface` 14 / 含 `range` 8 / 含 map 字面量 5（`panic` 多在失败分支，不执行） | 【测】关键字统计 |
| F14 | **打印面分布**：27 个用 `println`、20 个用 `print` —— **都写 stderr**，而判据是合并流 ⇒ 顺序必须保真 | 【测】 |
| F15 | **`rgoc-hir` 的值表示已证「常量必须任意精度」**（`1<<100` 收敛点在 `BigInt::to_int64()` 一处）；`Const::Big/Bool/Str` 三变体；本阶段保留该结论，但**值表示整体重写**（spike 的 `Val` 只服务三个 fixture） | 【源】`C3` §1.1 |
| F16 | **`initorder::analyze(pkg)` 已产出 `order` 与 `cycles`**（M4 的 `T-CK-71/72` 对撞过）—— M5 直接消费，不重算 | 【源】`rgoc-check/src/initorder.rs` |
| F17 | **`rgoc-load::load_with_imports` 支持源码 import**（`-I` 根），`Package` 带作用域树与对象表；**标准库 import 一律拒绝**（`unsafe` 例外） | 【源】`rgoc-load/src/package.rs`、`M4-report.md` §6 |
| F18 | **未捕获 panic 的官方行为**：退出码 2，stderr 打 `panic: <值>` + `goroutine 1 [running]:` traceback。**我们只承诺前者**（traceback 是实现细节） | 【测】+ 约定 |
| F19 | **当前基线**：workspace **1541 测试 / 136 suite 全绿**；M0 自检 231 条、M1 自检、三条 AST 门禁脚本均通过；四条 cargo 无 warning（`M4-report.md` §10） | 【测】M4 收口 |
| F20 | **`rgoc-ast` 只有 `count_nodes`（`walk.rs`），无通用 visitor** —— M5 的 lowering 与求值各自写遍历，不共享（刻意：一个产出 HIR，一个消费 HIR） | 【源】`rgoc-ast/src/walk.rs` |

### 2.4 关键取舍（不逐条进五字段，直接采纳）

1. **`rgoc-interp` 新 crate，不塞进 `rgoc-hir`**：HIR 是**数据**（可 dump、可对撞），解释器是**机器**（有状态、有预算）。依赖方向 `check → hir → interp → driver` 单向。M6 的 SSA 求值器将来只依赖 `hir` + 一套 `Value` 语义，不需要 `interp`。
2. **`rgoc run` 前**必须**先过 check**（有诊断即拒跑，退出码 1）：否则「跑一个静态非法的程序」会给出无意义的行为证据。这条也是 `D-M5-2` 两遍结构的前提。
3. **不追求与官方 map 迭代序一致**：官方是**故意随机**的，追不上也不该追。我们固定插入序，并在 `M5-report.md` 里明确「依赖 map 迭代序的程序不在支持面」。
4. **不追求地址数值**：`F10` 实测地址每次不同。凡是输出里含指针数值的语料，一律进排除清单（理由：不确定性），不做模糊匹配。
5. **泛型只做单态化，不做字典/shape**：`RT-POLY-05` 的字典化在 M12（roadmap §5），M5 单态化足够跑通子集。
6. **`unsafe` 在执行面继续拒绝**：M4 的白名单是静态求值（`Sizeof/Alignof/Offsetof`），这些在 M5 里**本来就是常量**（检查器已折叠），执行面无需新能力。
7. **loopvar 用 `go1.22` 语义**：每轮迭代新建变量。这是 `03` §M5 第 5 条点名的「直接测试」项，且它**没有静态错误**（`M4-report.md` §2.2），只能靠执行面证明。

---

## 3. 架构与数据模型

### 3.1 流水线与 crate 变化

```text
输入：单包多文件 .go（M5 语料 = 单文件单包）
  ▲
  │ M4 现有
rgoc-load::load_with_imports ──► Package { 作用域、对象表、常量值、导出摘要 }
  │
  │ M4 现有（第一遍：出诊断）
rgoc-check::check_package ──► CheckedPackage { diags } ──► 有诊断 ⇒ 拒跑（exit 1）
  │
  │ M5 新增（第二遍：同一 Session，静默会话取类型）
rgoc-hir::lower(&mut Session, &Package) ──► Program { funcs, globals, init_order, main }
  │                                          （lowering 期诊断必须为空 —— 守卫）
  │
  │ M5 新增
rgoc-interp::run(&Program, opts) ──► RunOutcome { merged_output, exit_code, budget_used }
  │
  ├─ driver：rgoc-driver run [--dump-hir] <files…>
  └─ 对撞：merged_output 与 `<同名>.out`（或空）逐字节 + 退出码
```

| crate | 变化 | 内容 |
|---|---|---|
| `rgoc-hir-spike`（**新**） | 建（迁移） | 现 `rgoc-hir` 的 4 个文件原样迁入；`rgoc-spikes` 改依赖；`SM-M0-NATIVE-HELLO` 不动 |
| `rgoc-hir`（**重写**） | 整体替换 | `program.rs`（Program/Func/Global/InitOrder 消费）、`stmt.rs`、`expr.rs`、`place.rs`（可寻址位置）、`lower.rs`（lowering 主体）、`dump.rs`（中性 dump）、`diag.rs`（稳定拒绝码） |
| `rgoc-interp`（**新**） | 建 | `value.rs`（Value/Cell/MapObj）、`eval.rs`（求值任务栈）、`frame.rs`（帧栈与主循环）、`call.rs`（调用/返回/变参/多值）、`builtin.rs`、`ops.rs`（算术/比较/宽度）、`print.rs`（输出语义）、`init.rs`（包初始化）、`exc.rs`（defer/panic/recover）、`budget.rs`（三道上限） |
| `rgoc-check` | **开面 + P0 修** | `Session` 的 7 个入口 `pub(crate) → pub`；P0 的 4 族规则修复（§2.1 `D-M5-3`） |
| `rgoc-driver` | 扩 | `run` 子命令（`--dump-hir` / `--max-steps`）；`manifest_m5.rs` 对账测试（沿 `manifest_m4.rs` 范式） |
| `tools/run-oracle`（**新**） | 建 | 官方 testdir `run` 语义复刻（合并流 + `.out`/空）；`run-collide.sh --run [--list]` 形态；两清单 |
| `docs/` | 新建 | `contracts/C9-values-and-output.md`；`M5-{tests,plan}.md`、`M5-manifest.json`、`M5-report.md`；`C3` 升正式 |

### 3.2 HIR 形状（正式，替代 spike）

**设计原则**：HIR 是**可执行**的，不是 AST 的换皮。每条语句/表达式的**求值顺序**在 HIR 里是显式的（不许留「到解释时再决定」的余地），否则 §3.4 的显式求值栈无法工作。

```rust
pub struct Program {
    pub funcs: Vec<Func>,            // 索引即 FuncId
    pub globals: Vec<Global>,        // 包级变量（含零值初始化式）
    pub init_order: Vec<GlobalId>,   // 来自 rgoc-check::initorder::analyze
    pub inits: Vec<FuncId>,          // 各文件的 init 函数（按文件序）
    pub main: Option<FuncId>,
}

pub struct Func {
    pub name: String,
    pub params: Vec<SlotId>,         // 形参槽
    pub results: Vec<SlotId>,        // 命名/匿名返回值槽
    pub nlocals: usize,              // 帧槽总数（形参 + 返回值 + 局部 + 临时）
    pub body: Block,
    pub captured: Vec<Capture>,      // 闭包捕获（按变量，`Capture { outer_slot, kind }`）
}

pub enum Stmt {
    Assign { lhs: Vec<Place>, op: Option<BinOp>, rhs: Vec<Expr> },  // 求值顺序：rhs 先、逐个赋
    ShortDecl { slots: Vec<SlotId>, rhs: Vec<Expr> },
    Expr(Expr),                      // 调用语句（结果丢弃）——**与 M4 的 B 族修复对应**
    If { init: Option<Box<Stmt>>, cond: Expr, then: Block, els: Option<Block> },
    For { init: Option<Box<Stmt>>, cond: Option<Expr>, post: Option<Box<Stmt>>, body: Block },
    Range { key: Option<Place>, value: Option<Place>, x: Expr, body: Block, kind: RangeKind },
    Switch { init: Option<Box<Stmt>>, tag: Option<Expr>, cases: Vec<Case> },
    TypeSwitch { /* 绑定变量 + 每 case 的类型列表 */ },
    Return(Vec<Expr>),
    Branch { kind: BranchKind, label: Option<String> },   // break/continue/goto/fallthrough
    Labeled { label: String, stmt: Box<Stmt> },
    Block(Block),
    Defer { call: CallSite },        // **参数已在 lowering 期展开为 slot**（登记时求值）
    Go,                              // **不产生**：`go` 语句在 M5 显式拒绝（`U-M5-*`）
}

pub enum Expr {
    Const { v: ConstVal, typ: TypeId },
    Slot(SlotId),                    // 读槽（值语义：数组/结构体读出来就拷贝）
    Global(GlobalId),
    Field { base: Box<Expr>, index: usize },
    Index { base: Box<Expr>, index: Box<Expr> },
    Slice { base: Box<Expr>, lo: Option<Box<Expr>>, hi: Option<Box<Expr>>, max: Option<Box<Expr>> },
    Deref(Box<Expr>),
    Addr(Place),                     // `&x`：**唯一**产生指针的构造
    Unary { op: UnOp, x: Box<Expr> },
    Binary { op: BinOp, x: Box<Expr>, y: Box<Expr>, typ: TypeId },
    Call { callee: Callee, args: Vec<Expr>, variadic: bool },
    Convert { to: TypeId, x: Box<Expr> },
    Composite { typ: TypeId, elems: Vec<CompositeElem> },
    FuncLit(FuncId),                 // 函数字面量在 lowering 期已变成独立 Func
    TypeAssert { x: Box<Expr>, to: TypeId, comma_ok: bool },
    Make { kind: MakeKind, typ: TypeId, args: Vec<Expr> },
    Builtin { kind: BuiltinKind, args: Vec<Expr> },
}

pub enum Place {                      // 可寻址位置（C3 §2.2 的答案）
    Slot(SlotId),                    // 局部/形参/命名返回值
    Global(GlobalId),
    Field { base: Box<Place>, index: usize },
    Index { base: Box<Expr>, index: Box<Expr> },   // 数组/slice 元素（**map 元素不可寻址**）
    Deref(Box<Expr>),
}
```

**三条与 C3 待验证项的直接对应**（本设计对 `C3` §2 的四个问题的回答）：

| C3 §2 问题 | 本设计的答案 |
|---|---|
| 多返回值是「各自一个值」还是 bundle？ | **语言级 tuple**：`Call` 的结果在求值栈上压 `n` 个值；`Return(Vec<Expr>)`；`Assign` 的 `rhs` 可以是单元素 `Call` 展开成多个。**不是结构体**（`f().x` 非法） |
| 可寻址性怎么表达？ | **`Place` 与 `Expr` 分开**：只有 `Place` 能取址（`Addr(Place)`）。`&T{...}` 是 `Addr` 的特例 —— lowering 期把复合字面量**物化**成一个临时 slot（`D-M5-3` C 族的语义答案） |
| 闭包捕获按值还是按变量？ | **按变量**：`Capture { outer_slot }` 指向 `Cell`，闭包与外部共享同一个 `Cell`。loopvar 语义靠「每轮新建 `Cell`」实现 |
| defer 参数何时求值？ | **登记时**：lowering 期把 defer 的实参**预先求值到临时 slot**，`Defer { call }` 只持有 slot 列表 |

**「未支持节点显式拒绝」的形态**（`EX-1` 的判据）：HIR 的 `Stmt::Go` 存在但 **lowering 永不产出**它；`lower` 遇到 `go`/`chan`/`select`/`range` over func 时返回稳定拒绝码（`HIR-UNSUPPORTED-*`），**不是** `todo!()`、**不是**静默跳过。反向验证：把某个拒绝分支改成 `todo!()` ⇒ 对撞必红（进程 abort ≠ 退出码 1）。

### 3.3 值模型（`C9` 的核心）

```rust
pub type Cell = Rc<RefCell<Value>>;      // 一切可寻址位置的载体

pub enum Value {
    Bool(bool),
    Int(i128),                  // 宽度由 HIR 节点的静态类型决定；存取时按宽度规范化
    Float(f64),                 // float32 每次运算后按 f32 舍入
    Complex(f64, f64),
    Str(Rc<str>),               // 不可变；赋值共享
    Array(Rc<Vec<Cell>>),       // 值语义：赋值 = 深拷贝（新建 Rc + 新 Cell）
    Struct(Rc<Vec<Cell>>),      // 同上；字段下标由 lowering 定死
    Slice { arr: Rc<Vec<Cell>>, off: usize, len: usize, cap: usize },
    Map(Rc<RefCell<MapObj>>),   // 引用类型；赋值共享
    Ptr(Cell),
    Func(Rc<Closure>),          // 捕获的 Cell 列表 + FuncId
    Iface { typ: Option<TypeId>, data: Box<Value> },  // None = nil 接口
    Nil(NilKind),               // 有类型 nil：ptr/map/slice/func/chan
}
```

**四条语义的落点（逐条可测）**：

| 语义 | 表示 | 判据（fixture） |
|---|---|---|
| 数组/结构体**拷贝** | `Rc<Vec<Cell>>` + `deep_copy()` | `a := arr; a[0]=1; arr[0]` 仍为原值；`&arr[0]` 之后再拷贝仍不串 |
| slice **共享底层** | `{arr: Rc, off, len, cap}` | `s2 := s1; s2[0]=1` ⇒ `s1[0]==1`；但 `s2 = append(s2, x)` 后**可能**不再共享（cap 决定） |
| string **不可变** | `Rc<str>` | `s[0]` 只读；`s[0]='x'` 在静态面就被拒（M4）；`[]byte(s)` 是拷贝 |
| map **引用** | `Rc<RefCell<MapObj>>` | `m2 := m1; m2[k]=v` ⇒ `m1[k]==v`；`delete` 影响两侧 |

**`MapObj` 的键**：Go 的 `==` 语义（`NaN != NaN`、结构体按字段、接口按 `(动态类型, 值)`、指针按身份）。宿主 `HashMap` 只是存储，**外层必须自己实现键相等**（`03` §2 原文）。迭代顺序固定为**插入序**（`D-M5-8` 取舍 3）。

**整数宽度**：`Int(i128)` + 节点静态类型给的宽度。运算后按宽度掩码（有符号做符号扩展）。`uint64` 最大值 `18446744073709551615` 在 `i128` 里装得下（`F10` 实测打印正确）。**除零** = panic（Go 语义），不是宿主 panic。

### 3.4 帧栈与执行机（`D-M5-1` 的落地）

```rust
pub struct Interp {
    frames: Vec<Frame>,       // 帧栈 —— 唯一状态之一
    eval: Vec<EvalTask>,      // 表达式求值栈 —— 唯一状态之二
    values: Vec<Value>,       // 求值结果栈
    panic: Option<PanicState>,
    budget: Budget,           // 步数 / 深度
}

pub struct Frame {
    func: FuncId,
    slots: Vec<Cell>,         // 帧槽（形参/返回值/局部/临时）
    state: FrameState,        // **显式 PC**
    defers: Vec<Deferred>,    // 本帧的 defer 栈（LIFO）
}

pub enum FrameState {
    Enter,
    Stmt { block: BlockId, idx: usize },      // 下一条语句
    Eval { /* 等表达式求值完 */ },
    CallReturn { /* 等被调方返回 */ },
    RunningDefers { idx: usize, reason: DeferReason },
    Done,
}
```

**主循环**（伪码）：

```text
loop {
    if let Some(p) = panic.take() { unwind(p)?; continue; }   // 展开：跑 defers → 弹帧
    match top().state {
        Done => { pop_frame_and_push_result(); }
        Stmt{..} => step_stmt(),
        Eval{..} => step_eval(),        // 从 eval 栈取一个任务，推子任务/压值
        RunningDefers{..} => step_defer(),
        Enter => enter_frame(),
    }
}
```

**「帧是唯一状态」的三条硬约束**（`EX-7` 的判据来源）：

1. `step_*` 函数**不得**递归调用自身或互相递归（除了一层的任务分派）。
2. 任何「等子表达式求值」的地方，中间态必须在 `FrameState` 或 `eval` 栈里，**不在 Rust 局部变量里**。
3. 帧深与求值栈深都有计数上限，超限 ⇒ `RunOutcome { exit_code: 2, diag: "frame depth exceeded" }`。

**为什么这样就能接上 M8**：M8a 的挂起 = 在 `FrameState` 加一个 `Blocked(WaitId)` 分支，主循环见到就换 goroutine；`Interp` 的字段全是可枚举数据 ⇒ 「保存/恢复一个 goroutine」= 保存 `(frames, eval, values, panic)` 四个 Vec。**如果当初用 Rust 递归，这四个 Vec 里只会有最外层一帧，其余都在 Rust 栈上，救不回来。**

### 3.5 内建与输出（`F10` 的表就是判据）

| 内建 | M5 语义 | 备注 |
|---|---|---|
| `print` / `println` | 见 `F10` 格式表；写**合并流**（`D-M5-6`） | `println` 空格分隔 + 换行；`print` 无分隔；浮点 `%g`（精度 -1）；`rune` 打十进制 |
| `len` / `cap` | string/array/slice/map/chan（chan 不出） | 常量折叠已在 M4；这里只做运行时 |
| `append` / `copy` | slice 增长（cap 翻倍策略**不承诺**，只承诺语义） | `append` 返回新 slice，可能共享底层 |
| `make` / `new` | slice/map/chan（chan 不出）/ 指针 | `new(T)` = 分配零值 `Cell` |
| `delete` / `clear` | map / slice/map | |
| `min` / `max` | 1.21 | 常量面在 M4 |
| `complex` / `real` / `imag` | 复数 | |
| `panic` / `recover` | §3.7 | |
| `close` | **拒绝**（chan 不出） | 稳定码 |

### 3.6 初始化执行（`D-M5-10`）

```text
单包（M5 语料全部如此）：
  ① 全局变量按 init_order（来自 rgoc-check::initorder::analyze，F16）逐个求值
  ② 每个文件的 init() 按文件序执行
  ③ main()
双包（自有 fixture）：
  import 依赖图（rgoc-load 已建）→ 深度优先：被导入包先跑完 ①②，再跑本包
```

**「顺序可检查」的判据**：`InitOrder` 是**静态**结论（M4 已对撞），执行面要证明的是「**执行序与 InitOrder 一致**」—— 用一个 fixture：每个全局变量的初始化式里 `print` 自己的名字，输出序列必须等于 `InitOrder` 的序列。这条把静态图与运行序**接在同一条证据链**上，正是 `M4-report.md` §2.4 要求的「不许把静态图当运行正确」的兑现方式。

### 3.7 defer / panic / recover（`D-M5-9`）

| 规则 | 语义 | 判据 |
|---|---|---|
| 参数求值时机 | **登记时**（`defer f(x)` 的 `x` 在 defer 语句执行时求值） | `defer f(x); x=2` ⇒ `f` 收到旧值 |
| 执行顺序 | **LIFO**，按帧 | 三个 defer 的打印序 |
| `recover` 有效条件 | 必须**直接**在 defer 的调用链里（`defer func(){ recover() }()`）；`defer recover()` 无效（返回值被丢） | 两种写法各一个 fixture |
| 命名返回值 | `recover` 后可改写；未命名返回值不可 | `func f() (r int) { defer func(){ if p:=recover(); p!=nil { r=1 } }(); panic(0) }` |
| 嵌套 panic | 展开过程中再 panic ⇒ 覆盖原 panic，继续展开 | 三层嵌套 |
| 未捕获 | `panic: <值>\n` 到合并流 + 退出码 **2** | 自有 fixture（**不**比 traceback，`F18`） |
| `panic(nil)` | Go 1.21+ ⇒ `*runtime.PanicNilError`；我们**简化**为 `panic: nil`，登记 `U-M5-*` | 显式登记，不假装一致 |

**实现**：`unwind` 是主循环里的一个状态（不是 Rust 的 `catch_unwind`）—— 用宿主 panic 做控制流会**同时**违反 `D-M5-1` 与 workspace 的 `unsafe_code = "forbid"` 邻近纪律（`catch_unwind` 需要 `UnwindSafe` 且语义错位）。

### 3.8 资源上限（`D-M5-12`，`EX-7` 的判据）

| 项 | 值 | 超限行为 |
|---|---|---|
| 帧深 | 1000（沿 M2/M3 的递归深度口径） | 退出码 2 + `frame depth exceeded` |
| 求值栈深 | 10000 | 同上 |
| 步数预算 | 1e8（单程序） | 同上 + `step budget exceeded` |
| 单程序墙钟 | 10 s（`M5-tests.md` 冻结） | `timeout` 分类，**不**记 pass |

**为什么要三道上限而不是一道**：`F19` 的实测显示预算从来没成为瓶颈（M4 全量 1.5 min），所以上限的作用**不是省资源**，而是**把「无限循环/无限递归」变成可分类的结果**（roadmap §3.3 的 `timeout/resource-failure` 状态）。

---

## 4. 对撞装置与判据

### 4.1 `rgoc-driver run`

```text
rgoc-driver run [--dump-hir] [--max-steps N] <files…>
```

| 通道 | 内容 |
|---|---|
| 合并流（stdout） | 程序的 `print/println` 输出 —— **与官方同构**：我们把 stdout 与 stderr 写进**同一个 buffer**（`D-M5-7`） |
| 诊断（stderr） | 静态诊断（有 ⇒ 不跑，exit 1）/ 运行时错误（exit 2） |
| 退出码 | 0 = 正常；1 = 静态诊断；2 = 参数/io/内部/资源超限/未捕获 panic |

> **`--dump-hir` 走 stdout**（与 `--diag-dump` 同纪律：dump 是给对撞看的，诊断走 stderr）。

### 4.2 `tools/run-oracle`：官方 testdir 语义复刻

* 输入：一个 `.go` 文件；输出：**合并流字节** + 退出码（与 `rgoc run` 同构）。
* 语义（`F4`）：
  1. 用容器内 `go run`（`go1.27.1`）跑；stdout/stderr 写**同一 buffer**；
  2. 期望 = `<同名>.out` 的内容；**无 `.out` ⇒ 期望为空**；
  3. `\r\n` 归一为 `\n`（官方 `:1184`）；
  4. 退出码非 0 ⇒ 该用例**不适用**（官方判失败，我们也不该把它算 pass）。
* **三方差分**（`EX-6` 的加强）：`rgoc` 输出 vs `.out` vs `go run` 输出。三方一致才算「无争议绿」；`.out` 与 `go run` 不一致 ⇒ 该文件进排除（说明语料期望已与当前工具链脱节）。

### 4.3 `run-collide.sh --run` 新形态

* `--run <testdata 根>`：读 `run-selected.txt`（唯一清单），逐文件跑两侧并**逐字节**比对合并流 + 退出码。
* `--run --list <清单>`：指定清单（供 ken 子集/单文件复现）。
* 纪律沿用：两侧非空、重复运行确定、`#` 注释不进对撞。

### 4.4 语料清单（候选 80 → 入选/排除）

| 类 | 数 | 判据 |
|---|---|---|
| 候选 | **80** | `// run` + 零 import + 无并发（`F1`） |
| 入选 | 待 P0 收口后实测（目标 **≈70**） | 候选 ∧ `rgoc check` 接受 ∧ 不含不可复现输出 ∧ 不含 M5 不支持的构造 |
| 排除 | 80 − 入选 | 逐行理由，五类：`P0 未修完的规则`｜`import "reflect"（范围外）`｜`runoutput（动态编译）`｜`不可复现输出（地址/map 序）`｜`M5 未支持构造（chan/go/select/rangefunc）` |

**纪律**：入选与排除的并集必须**恰好等于候选**（两向断言）；**不许**为了让数字好看移出文件（`D-M5-8`）。

### 4.5 明确不承诺的三件事（写进 `unsupported`，每条带正向证据）

1. **地址数值**：`F10` 实测 `0x2fcee60e0e90` 每次不同 ⇒ 不承诺。正向证据：含地址输出的 fixture 被**显式排除**（清单行 + 理由）。
2. **map 迭代序**：官方随机，我们插入序 ⇒ 不承诺一致。正向证据：`range` over map 的 fixture 只断言「集合相等」，不断言顺序（`T-EX-*` 有对应断言）。
3. **未捕获 panic 的 traceback**：只承诺 `panic: <值>` 前缀 + 退出码 2。正向证据：fixture 断言**前缀**与退出码，并断言「不含 `goroutine` 字样」是**允许差异**（不是失败）。

---

## 5. 覆盖清单（roadmap §M5 ↔ 功能 ID 面）

| 组 | 进 M5 的项 | 例外/声明 |
|---|---|---|
| `PIPE` | `PIPE-10`（HIR lowering）、`PIPE-14`（闭包）、`PIPE-15`（接口动态）、`PIPE-16`（多返回值/求值顺序的**执行**面）、`PIPE-24`（初始化**执行**）、`PIPE-01`（`run` 子命令） | `PIPE-16b`（range over func）**显式拒绝**（M8d）；`PIPE-16c`（loopvar）**做**（直接测试）；`PIPE-11` 只做单态化，字典化出（M12） |
| `EXP` | `EXP-01..27` 的**执行面**（求值、短路、回绕、浮点、比较、转换、调用、索引、切片、选择器、断言、复合字面量、函数字面量） | `EXP-27`（求值顺序）只到「解释器顺序」；codegen 面归 M6 |
| `STM` | `STM-01..21` 的执行面（赋值、短声明、if/for/switch/type-switch/range、break/continue/goto/fallthrough、return、defer、go（拒绝）） | `STM-05`（select）拒绝；`STM-13/14/15`（并发相关）拒绝 |
| `BIF` | `BIF-01..15` 的运行时面（§3.5 表） | `close` 拒绝（chan 不出） |
| `PKG` | `PKG-05`（init 执行）、`PKG-07`（`main` 入口） | `PKG-07` 的 argv/envp 面**不做**（无 `os`） |
| `RT-CMP` | `RT-CMP-01..07`（比较/哈希/相等语义，map 键 + 接口比较） | 不要求 SwissTable/扩容策略照搬（`03` §2） |
| `RT-POLY` | `RT-POLY-01..04/06/07` 的**动态面**（接口值、itab 等价物、断言、method value） | `RT-POLY-05`（字典化）出（M12）；`RT-POLY-*` 的反射面出（M12） |
| `RT-EXC` | `RT-EXC-01..06` 的**解释器语义面**（defer/panic/recover/嵌套/退出） | traceback 格式与信号转换出（M9/M11） |
| `RT-MEM` | **一律不计**（宿主管理内存，`03` §5 明写「宿主管理不计实现」） | 只保留预算监控（§3.8） |
| `RT-SCH` | **一律不做**（M8） | — |

---

## 6. 错误处理与资源上限

### 6.1 三类诊断，三个出口

| 类 | 出口 | 退出码 |
|---|---|---|
| **静态诊断**（check 面） | stderr（`{file}:{line}:{col}: <msg> [<CODE>]`） | 1（**不跑**） |
| **HIR 拒绝**（未支持构造） | stderr + 稳定码 `HIR-UNSUPPORTED-*` | 1（**不跑**） |
| **运行时错误**（除零/nil 解引用/越界/panic/预算） | 合并流（`panic: ...`）+ stderr | 2 |

**为什么 HIR 拒绝是 1 不是 2**：它是**静态**结论（这份源码不在支持面内），与「程序跑起来出错了」是两件事。混在一起会让 `EX-6` 的统计失去区分能力。

### 6.2 资源上限

见 §3.8。补充：语料批次预算沿用 `M4` 的实测口径（单文件 ≤10 s，milestone ≤30 min，full ≤2 h），M5 的实际值在 `M5-tests.md` §9 冻结。

### 6.3 预算与降级序

* roadmap §6.1 给 M5 的量级：**6–12 人周（全部子阶段合计）/ 12 周日历**。本仓执行形态（AI 代理 + 容器）按任务数折算，**不换算人周**；`M5-manifest.json` 记测试数与门禁 + `budget.effort_calendar`。
* **降级序**（roadmap §M5「砍项」的执行化）：① 砍 `ken/` 子集的一半（保留顶层 50）；② 砍泛型解释子集（→ 显式拒绝 + 登记）；③ 砍双包 init fixture（保留单包）；**不砍**：HIR 拒绝面、显式帧（`D-M5-1`）、四类赋值语义、defer/panic/recover 五条规则、四条 cargo 与既有自检。
* **止损切片**（可先发布）：`EX-1`（HIR）+ `EX-2`（函数/帧/初始化）+ `EX-3`（复合值）三块 —— 它们覆盖了「顺序语义基线」的骨架；`EX-4/5`（闭包/接口、异常）是增量。

---

## 7. 测试策略

### 7.1 四条腿 + 反向验证（沿 M3/M4 结构）

| 腿 | 内容 | 主判据 |
|---|---|---|
| 甲 | crate 内单测：**自有 fixture**（`S`，手写期望）—— HIR 节点覆盖、值语义四类、帧/递归/多值、内建、闭包/接口、defer/panic、init 序 | `S` |
| 乙 | **语料对撞**：入选清单逐文件（合并流逐字节 + 退出码）+ `go run` 三方差分 + `ken` 子集 | `O` |
| 丙 | **装置与 CLI**：`run` 退出码表、合并流保真、`--dump-hir` 确定性、预算/超时、四条 cargo、五条既有自检、manifest 对账、语料只读 | `H` |
| 丁 | **P0 前置**：4 族规则的修复（接受面）+ 79/80 接受率 + **M4 的 63 条 `T-CK-*` 不回退** | `S`+`O` |
| 反 | **变异探针 ≥8 条**（`D-M5-15`）：值模型拷贝、帧栈、defer 时机、判据装置本身 | 反证 |

### 7.2 判据形态（与 M4 的「位类对齐」完全不同）

M4 判的是「诊断的行与类别」；M5 判的是「**输出字节**」。所以：

* **逐字节**，不做多重集、不做剥列、不做容差。
* **合并流**（`D-M5-7`），顺序敏感。
* 退出码**必须**是 0（`// run` 的官方语义）。
* 唯一允许的差异：`\r\n` → `\n`（官方 `:1184` 也这么做）。

### 7.3 反向验证与遗漏纪律

* 沿 `scripts/mutation-m3-notes.md` §0/§0.1：变异前 `cargo clean -p`；可疑时全量重建；探针照不出就照实登记。
* **本阶段新增的变异点**（每条都要「造一个假绿场景它会红」的实证）：
  1. 把数组赋值改成共享 `Rc`（不做深拷贝）⇒ 拷贝 fixture 必红；
  2. 把 slice 赋值改成深拷贝 ⇒ 别名 fixture 必红；
  3. 把 defer 参数改成执行时求值 ⇒ 时机 fixture 必红；
  4. 把 `recover` 改成「任何 defer 里都有效」⇒ 直接调用限制 fixture 必红；
  5. 把帧栈改成递归（`D-M5-1` 的反向）⇒ 1000 层递归 fixture 必红（或宿主栈溢出 ⇒ 更红）；
  6. 把合并流改成只比 stdout ⇒ 47 个 `print/println` 文件必红；
  7. 把「未支持构造」改成静默跳过 ⇒ 拒绝面 fixture 必红；
  8. 在 `run-oracle` 里把 `.out` 缺失当成「不比」⇒ 空输出 fixture 必红。

---

## 8. 契约与产物影响

| 产物 | 动作 |
|---|---|
| `docs/contracts/C3-hir.md` | **升正式**：§2 的四项待验证全部有答案（§3.2 表）；新增 HIR 节点全表与 `Place` 定义；「SPIKE-ONLY 解除条件」标记为已满足 |
| `docs/contracts/C9-values-and-output.md` | **新建**（`M5-plan.md` 首个任务）：值表示、四类赋值语义、`print/println` 字节格式（`F10` 表）、合并流、退出码表、三件不承诺（§4.5） |
| `docs/contracts/C1/C2/C7/C8` | **预期不动**（C1 的位置表示在 lowering 里被消费，不改格式） |
| `rgoc-hir-spike` | 新建（迁移），`C3` 的「反推来源」段落追加指针 |
| `M4-report.md` / `M4-manifest.json` | **不改**（M5 不回收 M4 的缺口；P0 修的是**接受面**，M4 的 `CH-2` 判的是**拒绝面**，两者不冲突 —— 这条要在 `M5-report.md` 里写清楚） |
| `GAP-01.md` | M5 完成后补 `PIPE-10/14/15/16/16c/24` 与 `RT-*` 的落点（沿 M2/M3/M4 做法） |
| `AGENTS.md` / `03-roadmap.md` | 仅当 M5 **完成**时更新状态节（不与执行同日） |

---

## 9. 门禁汇总

见 §1.4 的 **EX-1..EX-9**（`M5-tests.md` §8 定稿）。命名取 M5 主题词 `EX`（execution），与 M3 的 `TY-*`、M4 的 `CH-*` 并列。

---

## 10. 风险与未决

| # | 事项 | 类型 | 处理 |
|---|---|---|---|
| 1 | **P0 四族修复的实际工作量** | 风险（最高） | A/B 族是 M5 的核心面（必须修）；C 族一行规则；D 族三小族可能牵出类型参数核心类型（`G-M4-37`）⇒ 若成片，相关文件进排除（`D-M5-3` 失效条件） |
| 2 | **`make(map[T]V)` 的底层同一** | 风险 | `bigmap.go` 一个文件 9 行；修不动就排除，不影响 `EX-6` |
| 3 | 显式求值栈的性能 | 未决 | 实测（`F19` 的基线余量很大）；若 milestone 超 30 min ⇒ 局部改递归，但保留 `EX-7` 的 1000 层判据 |
| 4 | 浮点格式（`%g` 精度 -1）的边界 | 未决 | `F10` 只钉了几个点；RED 期用 `go run` 批量对撞 `float_lit.go` 一族 |
| 5 | `panic(nil)` 的 Go 1.21+ 语义 | 已登记 | 简化 + `U-M5-*`，不假装一致 |
| 6 | map 迭代序 | 已定 | 插入序 + 不承诺一致（§4.5） |
| 7 | 地址数值 | 已定 | 排除含地址输出的文件（§4.5） |
| 8 | 语料入选数可能低于 60 | 风险 | roadmap 说「数量不足不以复制用例凑数」⇒ 不足就如实报，**用自有 fixture 补足能力面**（甲腿），不补语料 |
| 9 | `C3` 升正式时的形状漂移 | 工程 | `C3` 的 §1（已验证）**一字不改**，只把 §2 的「待验证」改成「已定」+ 指向 §3.2 |
| 10 | 与 M4 停点的工作树状态 | 工程 | 沿 `AGENTS.md` §6/§7（就地 main、门禁后提交） |

---

## 11. 与后续文档的接口

### 11.1 交给 `writing-plans`（`M5-plan.md`）的输入

1. §3 架构与 §7 测试策略 → 任务分解（七个 Phase：P0 前置收口 → P1 契约与 HIR → P2 求值内核 → P3 复合值与内建 → P4 闭包与接口 → P5 异常 → P6 集成收口）；
2. `D-M5-4` 的 spike 迁移任务（**必须第一个做**，否则 `SM-M0-NATIVE-HELLO` 会被打断）；
3. `D-M5-2` 的 `Session` 开面任务（7 个入口）；
4. `M5-report.md`（交付 B）的产出任务近收口处。

### 11.2 `M5-tests.md` 必须覆盖

* `T-EX-*` 测试 ID 全表（甲/乙/丙/丁/反 五腿）；
* 值语义四类的正反 fixture 清单；
* 语料两清单（**开工前冻结**；逐行理由）+ 候选=入选∪排除的两向断言；
* `print/println` 字节格式表（`F10`）+ 三件不承诺（§4.5）；
* 四条 cargo + `run-collide.sh --run` 形态 + 五条既有自检的**验证命令与预算**；
* `unsupported` 白名单（M4 承接仍有效的 + M5 新增）。

### 11.3 与后续阶段的衔接

* **M6（SSA）**：消费 `rgoc-hir` 的 `Program` 与 `rgoc-interp` 的 `Value` 语义（SSA 求值器与解释器必须同结果，`T-S2-01` 的范式）；
* **M7（native MVP）**：消费 HIR 的帧布局与初始化序，生成 startup；
* **M8（并发）**：消费 `D-M5-1` 的显式帧（挂起 = `FrameState::Blocked`）；
* **M9（对象模型）**：`Value` 的 native 对应物（`Ptr`/`Iface`/`Slice` 的布局契约在 M9 定，M5 的表示**不**承诺与 native 同构）。

---

## 12. 变更记录

| 日期 | 变更 |
|---|---|
| 2026-10-22 | 初稿：按 roadmap §M5 全量起草；含 `F1–F20` 实测事实基线（探针在 `.superpowers/m5-probe/`，不入库）；`D-M5-1..15` 决策；`EX-1..9` 门禁草案。四条方向由用户头脑风暴裁定（显式帧栈 / 复用会话 / P0 前置 / 三件套一次出齐） |
