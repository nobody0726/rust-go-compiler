# M1 设计文档：词法与位置

> **阶段 ID**：`M1`　|　**状态**：**初稿（待评审）**　|　**日期**：2026-10-11
> **文档索引**：[`../README.md`](../README.md)　|　**上游**：[`../03-roadmap.md`](../03-roadmap.md) §M1、[`../01-feature-set.md`](../01-feature-set.md) §1.1（LEX-01..13）、[`../contracts/C1-source-map.md`](../contracts/C1-source-map.md)、[`M0-report.md`](./M0-report.md) §7
> **下游**：`M1-tests.md`（测试先行清单，待产出）→ `M1-plan.md`（任务级计划，待产出）→ 执行
>
> **本文档的性质**：Superpowers 流程中「头脑风暴」阶段的产出 —— **只冻结设计决策与边界，不写任务粒度、不写实现代码**。任务级拆分是 `writing-plans` 的职责。
>
> **读者提示**：不懂 `oracle` / `golden` / `spike` 这些词，先读 [`M0-design.md`](./M0-design.md) §1.0 —— 那里有白话定义。本文档假设读者已读过 `M0-report.md` §7「接手指南」。
>
> **§2 的决策记录是五字段**（判据 / 候选 / 结论 / 否决理由 / **失效条件**）。「失效条件」用来判断决定是否还成立 —— 只看结论无法判断结论对不对。

---

## 0. 本文档在流程中的位置

```text
头脑风暴（本文档）
   │  产出：用户签字确认的设计文档
   ▼
writing-plans
   │  产出：2–5 分钟粒度的任务计划（文件路径 / 可粘贴代码 / 验证命令）
   ▼
执行 + TDD（RED → GREEN → REFACTOR）
```

对应 [`../03-roadmap.md`](../03-roadmap.md) §6.2 的开工前置要求：

| 前置产出 | 本文档是否满足 | 说明 |
|---|---|---|
| `docs/milestones/M1-design.md` | ✅ 本文件 | 设计决策与边界 |
| `docs/milestones/M1-tests.md` | ⬜ **待产出** | 必需测试 ID、比较器、验证命令、暂不支持项 |
| `docs/milestones/M1-plan.md` | ⬜ **待产出** | 2–5 分钟粒度实施任务 |
| 机器可读 manifest | ⬜ **待产出** | `M1-manifest.json`：`gate` + `test_ids` + `unsupported` + `budget` |

**本文档未确认前不进入下一步。**

---

## 1. 目标与非目标

### 1.0 推导：为什么 M1 是「切词 + 记位」

**推导起点**：M0 结束时我们有了可运行的容器、可信的 harness、三条已验证的路线 —— 但
**从未读过一行 Go 源码**（这是 `M0-report.md` §5.1 的 `U12`，也是最容易被误读的一条）。

从源码到可执行文件这条路上，第一个问题不是「怎么生成代码」，而是：

> **文本里的「一个东西」在哪里开始、在哪里结束？**

`fori:=0` 是四个东西还是一个？这个问题绕不过去：**没有它，后面所有阶段拿到的都是一坨字符**。
而这个问题的答案不能靠"看着像"——它由语言规范定义（`spec:L163-177` Tokens），
且必须**逐字节可验证**。

于是第一步被逼出来：**把字节流切成词元流**。

**但还有第二根支柱，且它常被当成副产品**：

> **每个东西在原文的哪一行、哪一列？**

这不是"顺便记一下"。因为：
- M2 的语法错误要报位置（`parser` 的 `errorAt`）
- M4 的诊断要与 oracle 的 `文件:行号` 前缀对齐（`M0-tests.md` §1.3 的 R3/R4）
- C1 契约的**唯一目的**就是让位置在跨阶段之间唯一可比

**位置算错了，上层所有诊断都会漂移，而漂移的表现为「差 1」—— 这种错极难定位**
（`C1-source-map.md` §2.1 原文）。

**所以 M1 = 切词（把字节流变成词元流） + 记位（每个词元知道自己从哪来），两者并重。**

### 1.1 M1 的输入与输出

**输入**：一个 `.go` 文件的字节序列 + 一个文件标识（路径）。

- 来源：`go_source_code/` 只读语料（185 MB / 15,618 文件）
- **不来自 M0 的任何产物**（M0 的 `U12` 已明确）
- 输入中**没有**：AST、类型、包、import

**输出**：**两条流**

| 输出 | 内容 | 消费者 |
|---|---|---|
| **词元流** | 每个词元带：类别 + 载荷（原文/子类/运算符…） + `Pos` | M2 parser |
| **诊断流** | 每条带：`Pos` + `DiagCode` + 消息 | M4 静态检查、golden 对比 |

两个性质：

1. **它是"流"不是"数组"**：`Lexer::next()` 每调用一次吐一个（pull 模型）
2. **诊断是旁路**：一个词元可以既正常产出、又附带一条诊断（实测：`0x` → `Literal kind=Int bad=true` + 一条诊断）

### 1.2 明确不做（M1 边界）

**每一项都有归属，不是"漏了"**：

| 不做 | 归属 | 依据 |
|---|---|---|
| AST / parser | **M2** | 词元 ≠ 语法树 |
| 字面量的**值** | **M3** 常量 | M1 只给原文；`"hello"` 的载荷是带引号的那串字符 |
| `//line` 的**位置重映射** | **M2** parser 的 `updateBase()` | **实测**：官方 scanner 只交出注释文本，改基准是 parser 干的 |
| `//go:build` 的**求值** | M3/M4 包模型 | M1 只把文本交出去 |
| `//go:` 指令的**语义与效果** | M2 解析 / M7+ 生效 | `03` §M2：「实现效果列延期，不标完成」 |
| 预声明标识符的语义 | M3/M4 | **实测**：`true`/`nil`/`println`/`int` 全是 `_Name` |
| SourceMap 的 offset↔line/col **换算服务** | M2/M4 | **D-M1-5 只做 `FileTable`，不做换算** |
| 文档注释的保留 | 无（不需要） | **实测**：`cmd/compile` 连文档注释都看不见 |

### 1.3 M1 退出条件（逐条可判定）

| # | 条件 | 判据 |
|---|---|---|
| 1 | **LEX-01..13 各至少一条直接测试**，全绿 | `M1-tests.md` 冻结 ID 100% 通过 |
| 2 | **词法 golden 全绿**（切片级 + 语料级两条腿） | `SM-M1-LEX-GOLDEN` |
| 3 | **负例全绿**：非法 UTF-8 / NUL / 文件中部 BOM / 未终止注释 / 未终止字符串 / 超限 | 每条负例**同时**断言诊断与后续词元 |
| 4 | **`SM-M0-NATIVE-HELLO` 回归通过** | 输入**不经 lexer**（固定 HIR/SSA），不得改写 |
| 5 | 四条 cargo 命令全过 | fmt / check / clippy `-D warnings` / test |
| 6 | **位置差异可定位** | 失败报告能指出「哪个文件、哪一行、哪个词元」 |
| 7 | **C1 从「留位」升为「完整」** | 含字段变更与变更记录 |
| 8 | **GAP-01 的 LEX 13 项四维归属已定** | `M0-report.md` §7.2 点名 |

> **条件 6 是 M1 特有的**：`03` §M1 门禁原文是「**token 与位置差异可定位**」。
> 这条不是"能报错"就行 —— 它要求 golden 的 diff 能落到具体行。

---

## 2. 已确认决策记录

### 2.1 决策一览

| # | 决策 | 结论 | 确认日期 |
|---|---|---|---|
| D-M1-1 | 注释与指令的支持范围 | 对齐官方两种 mode | 2026-10-11 |
| D-M1-2 | 词法 golden 的落位 | 两条腿：crate 内 + driver 门禁 | 2026-10-11 |
| D-M1-3 | oracle adapter 的基准 | 编译器版 `syntax`，vendor 五文件 | 2026-10-11 |
| D-M1-4 | 资源上限策略 | 定硬上限并登记为**实现限制** | 2026-10-11 |
| D-M1-5 | `Pos` 的文件标识 | `FileId` + `FileTable`（修订 C1） | 2026-10-11 |
| D-M1-6 | Unicode 分类表来源 | **由 oracle 生成**，不引第三方依赖 | 2026-10-11 |
| D-M1-7 | 内部结构 | 三层拆分（SourceReader / SemicolonPolicy / Lexer） | 2026-10-11 |
| D-M1-8 | 注释与诊断的取用方式 | 两个独立收集器 + `take`，不用回调 | 2026-10-11 |
| D-M1-9 | 诊断的判据口径 | 位置 + `DiagCode`，**不复刻官方消息文本** | 2026-10-11 |
| D-M1-10 | 运算符词元的建模 | `Op` 增加 `form` 字段（**防止丢信息**） | 2026-10-11 |

### 2.2 五字段记录

#### D-M1-1　注释与指令的支持范围

| 字段 | 内容 |
|---|---|
| **判据** | LEX-03 要能被**直接测试**（`03` §M1：「LEX 各项直接测试」）；且 M2 需要 `//go:build` 文本 |
| **候选** | ① 静默跳过（最小 API）② **对齐官方两种 mode** ③ 两种 mode + 同时落地 SourceMap |
| **结论** | **②**。`Lexer` 带 mode：默认静默跳过；`comments` 模式上报全部注释；`directives` 模式上报 `//go:` 与 `//line` |
| **否决理由** | ① 的 LEX-03 只能靠"注释被正确跳过"**间接**验收，无法逐条对齐官方 `TestComments`/`TestDirectives`；③ 把 SourceMap 拉进 M1，阶段范围失控（且 M1 是单文件，换算服务无消费者） |
| **失效条件** | 若官方 `TestComments` 的表结构与我们的通道模型不兼容（写 `M1-tests.md` 时核对）⇒ 回到①并如实登记 LEX-03 的验收降级 |

**边界（不要误读）**：M1 **只识别并交出文本**，**不解析**指令内容，**不做**位置重映射。
实测依据：`cmd/compile` 的 `newParser` 用 `directives` 模式，`//line` 由 parser 的
`updateBase()` 处理（`syntax/parser.go:76`）。

#### D-M1-2　词法 golden 的落位

| 字段 | 内容 |
|---|---|
| **判据** | 「判定逻辑只有一处」的仓库纪律（`AGENTS.md` §1）+ `03` §3.2 的三层执行定义 + 尽量不动已冻结契约 |
| **候选** | ① 全部进 harness（新增 `Mode::Lex`，动 C2）② 只放 crate 内 ③ **两条腿** |
| **结论** | **③**，但**第二条腿挂在 driver 而非 harness**：腿甲 = `rgoc-lex/tests/*.rs`（六切面细粒度）；腿乙 = `rgoc-driver lex <file>`（`03` §2 已规划）+ 门禁脚本跑 golden diff |
| **否决理由** | ① 需修订 C2（M0 的「唯一完整级契约」），而实测 `03` 把 `SM-M1-LEX-GOLDEN` 归在 **smoke 清单**而非 milestone ⇒ 无须动 C2；② 判定实现出现两处 |
| **失效条件** | 若后续要求「lex 用例计入 279 分母」⇒ 那时才新增 `Mode::Lex`（届时是独立决策，不是本决策的反悔） |

#### D-M1-3　oracle adapter 的基准

| 字段 | 内容 |
|---|---|
| **判据** | `01` PIPE-02 把词法基准指向 `gc/syntax/scanner.go`；`03` §M1 明令不得用 `go tool compile -E` 当 token oracle |
| **候选** | ① **编译器版（vendor 五文件）** ② 公开版 `go/scanner`（零 vendoring）③ 两者都接做交叉校验 |
| **结论** | **①**。vendor `scanner.go` / `source.go` / `tokens.go` / `token_string.go` / `operator_string.go` 共 **1,374 行** |
| **否决理由** | ② 的粒度与 PIPE-02 不同（数字/字符/字符串是独立类别、无 `LitKind`/`op`/`prec`），golden 形状对不上官方编译器测试，M2 接不上；③ 两套维护 |
| **失效条件** | 若官方这五个文件不再自包含（`go tool compile <五文件>` 不再可编译）⇒ 必须换方案。当前版本头注释明确声明自包含 |

#### D-M1-4　资源上限策略

| 字段 | 内容 |
|---|---|
| **判据** | `03` §M1 门禁要求「非法 UTF-8 / **超长输入**等负例全绿」；但**实测官方 `syntax` scanner 无任何 token 长度上限常量** |
| **候选** | ① **定硬上限并登记为实现限制** ② 不设上限、只用既有 `Limits` 兜底 ③ 上限可配置、默认关闭 |
| **结论** | **①**。见 §7.3：**只设一个上限 = 单文件 64 MiB** |
| **否决理由** | ② 让门禁里那条"负例"失去错误路径可测，且单个巨型词元可把内存吃满；③ 默认路径仍无界，且多一套配置面与测试 |
| **失效条件** | 若将来要处理**生成代码**（远超 64 MiB）⇒ 上限需调；但调整必须走 manifest 变更并保留原报告 |

#### D-M1-5　`Pos` 的文件标识

| 字段 | 内容 |
|---|---|
| **判据** | C1 的 `Pos` 是从 `SPIKE-ONLY` 的 `rgoc-hir` **反推**的**留位**；M1 是第一个真正的消费者，此刻定形状成本最低 |
| **候选** | ① 保持 `file: String` ② **`FileId(u32)` + `FileTable`** ③ `file: Arc<str>` |
| **结论** | **②**。`FileTable` 只登记 名字↔Id，**不做 offset↔line/col 换算**（那不是它） |
| **否决理由** | ① 每个 `Pos` 自带一份文件名，clone/比较有成本，且"同一文件"靠字符串相等；③ 仍未给跨文件场景留下标识位，且没有解决"比较是字符串比较" |
| **失效条件** | 若 M2 表明"单文件内根本不需要 Id"（例如 `Pos` 从不跨文件比较）⇒ 可退化回①，但那时改动面已扩大 |

#### D-M1-6　Unicode 分类表的来源

| 字段 | 内容 |
|---|---|
| **判据** | LEX-02 要的是「字母 = Lu/Ll/Lt/Lm/Lo；数字 = Nd」（`spec:L105-135`）；`03` §2 规定**不默认引入新第三方依赖** |
| **候选** | ① 用 Rust std 的 `char::is_alphabetic`/`is_numeric` ② 引第三方 crate ③ **由 oracle 生成区间表** |
| **结论** | **③**。由 go1.27.1 的 `unicode` 包导出两张区间表（字母 **684** 区间 / 数字 **72** 区间，共 **756** 区间 ≈ 5.9 KB） |
| **否决理由** | ① **实测不一致**：字母 Rust 多 **1,749** 码点（全为 Mn），数字 Rust 多 **1,154** 码点（全为 No），合计 **2,903** 个码点行为不同 ⇒ `x²` 会从一个标识符变成一个非法字符，**静默错误**；② 授权成本 + 需评估 Unicode 版本漂移；另 `unicode-ident`（Cargo.lock 已有，syn 的传递依赖）**也不适用** —— 它实现 UAX#31 的 XID，与「广义类别」规则不同 |
| **失效条件** | 若核心语料中**从不出现**非 ASCII 标识符 ⇒ 差异不会暴露，可降级；但 `scanner_test.go` 的 `sampleTokens` **明确含** `a۰۱۸`/`foo६४`/`bar９８７６`/`ŝ` ⇒ 不成立 |

#### D-M1-7　内部结构

| 字段 | 内容 |
|---|---|
| **判据** | 六个测试切面（见 §8.3）必须能**各自独立测试**；且分派顺序要与 oracle 行为对齐 |
| **候选** | ① **三层拆分** ② 忠实移植（单块 `next()`）③ 三层 + 零拷贝区间表示 |
| **结论** | **①**。`SourceReader` / `SemicolonPolicy` / `Lexer` 三层；**分派与判定顺序照官方 `next()` 写** |
| **否决理由** | ② 不可单测（只能端到端），出错不知在哪一层；且"扁平结构体 + 注释说明有效域"与 D-M1-5 的 enum 方向自相矛盾；③ 生命周期参数 `'src` 会传染到 M2/M4，而 M1 的量级（279 个中等文件 + 94 MB 语料）根本不需要 |
| **失效条件** | 若语料级 golden 的耗时超出预算（`03` §6.1：smoke ≤5 分钟）⇒ 才考虑零拷贝 |

**「照抄语义、不照抄实现」的具体含义**（本节的方向性取舍）：

| 照抄 | 不照抄 |
|---|---|
| 分派顺序、判定条件、边界行为 | sentinel 哨兵字节 |
| 词元载荷的语义（有效域） | 流式 `io.Reader` 增量填充与 `fill()`/`nextSize()` |
| 错误的**位置与类别** | 错误的**消息文本**（见 D-M1-9） |

#### D-M1-8　注释与诊断的取用方式

| 字段 | 内容 |
|---|---|
| **判据** | 官方把注释与错误塞进**同一个回调** `errh`，靠"文本以 `/` 开头"区分（`syntax/parser.go:52`）。**注释不是诊断**，混在一起会让 C1 的排序规则被无意义地施加到注释上 |
| **候选** | ① 照官方用单一回调 ② **两个独立收集器 + `take`** ③ 回调 + 泛型 Sink |
| **结论** | **②**。`Lexer` 持有 `diags: DiagBag` 与 `messages: Vec<Message>`，经 `take_diags()` / `take_messages()` 取走 |
| **否决理由** | ① 在 Rust 里回调 + `&mut self` 借用冲突，且语义混装；③ 泛型 Sink 对 M1 是过度设计 |
| **失效条件** | 若 `rgoc-driver lex` 需要流式输出（边扫边打印）而非收集后再取 ⇒ 才需要回调/Sink |

#### D-M1-9　诊断的判据口径

| 字段 | 内容 |
|---|---|
| **判据** | `03` §3.4：「诊断**先对齐拒绝/接受、位置和预定义类别**，文本/regex **另列指标**」 |
| **候选** | ① 逐字复刻官方消息文本 ② **只对齐位置 + `DiagCode`** ③ 正则匹配官方文本 |
| **结论** | **②**。golden 只存 `行:列 + DiagCode`；adapter 侧维护一张「官方消息 → `DiagCode`」映射表（§7.2） |
| **否决理由** | ① 把官方文案当成契约，官方改一个字我们就红，而那种红**没有语义价值**；③ 正则本身就是 `GAP-02` 那个未决问题（M0 交接 §7.1 第 3 条）的同一个坑 |
| **失效条件** | 若 M4 的 errorcheck 对齐**必须**靠文本才能区分两类错误 ⇒ `DiagCode` 需要细分 |

#### D-M1-10　运算符词元的建模（`form` 字段）

> **本决策是 `writing-plans` 阶段发现的缺陷修补**，不是设计期的遗漏 ——
> 写任务分解时才发现 §3.2 的原型**丢信息**。

| 字段 | 内容 |
|---|---|
| **判据** | 官方用**不同的 `tok` 值**区分 `+`（`_Operator`）与 `+=`（`_AssignOp`）；M2 靠这个区分「二元表达式」与「赋值语句」 |
| **候选** | ① 保持 `Op { op, prec }` ② 照官方拆成 `_Operator`/`_AssignOp`/`_IncOp`/`_Assign`/`_Define`/`_Arrow`/`_Star` 七个变体 ③ **`Op` 加一个 `form` 字段** |
| **结论** | **③**。`Op { form: OpForm, op: Operator, prec: u8 }`，`OpForm = Plain \| Assign \| IncDec` |
| **否决理由** | ① **有损**：`+` 与 `+=` 都成 `op=Add, prec=5`，M2 无法区分 —— 这是**静默的信息丢失**；② 七个变体把"运算符语义"和"书写形式"两个正交维度压进一个枚举，M2 每处都要重新拼装 |
| **失效条件** | 若 M2 表明它只按 `form` 三分即可（不需要 `op` 的具体值）⇒ 可再合并；但那时改动面已在 M2 |

**无损性核对**（`form × op` 的组合是单射）：

| 拼写 | 建模 | 是否唯一 |
|---|---|---|
| `+` | `(Plain, Add)` | ✅ |
| `+=` | `(Assign, Add)` | ✅ |
| `++` | `(IncDec, Add)` | ✅ |
| `*` | `(Plain, Mul)` | ✅（`*=` 是 `(Assign, Mul)`）—— 因此**不需要**单独照搬官方的 `_Star` |
| `=` | `(Plain, Assign)` | ✅ |
| `:=` | `(Plain, Define)` | ✅ |
| `<-` | `(Plain, Arrow)` | ✅ |
| `==` | `(Plain, Eql)` | ✅（与 `=` 的 `op` 不同） |

### 2.3 事实基线（全部为实测，不是推断）

| # | 事实 | 实测方式 |
|---|---|---|
| F1 | 词元类别是**闭集**：`tokenCount = 47`（含 0 号占位）⇒ **46 个真实类别** | 官方枚举直接读 |
| F2 | 关键字 **25** 个；非关键字类别 **21** 个 | 官方枚举直接读 |
| F3 | spec 的运算符与标点表**实测 48 条**（9+9+9+9+9+3） | 解析 `go_spec.html` 的 `<pre class="grammar">` |
| F4 | 官方 scanner **无 token 长度上限常量** | 全树 grep `maxScanTokenSize` 无命中 |
| F5 | `lit` 只在 `_Name`/`_Literal`/`_Semi` 有效；其余类别是**上一个词元的残留值** | 逐字段转储实测 |
| F6 | `LitKind`：`0=Int 1=Float 2=Imag 3=Rune 4=String` | 逐字段转储实测 |
| F7 | 预声明标识符（`true`/`nil`/`iota`/`println`/`int`/`any`…）**全是 `_Name`** | 逐项实测 |
| F8 | BOM 仅允许文件首；文件中出现时报错**但仍并入标识符** | 实测 `lit="x\ufeffy"` |
| F9 | 非法 UTF-8 / NUL **留在词元原文里**（`lit="1\xff2"`、`lit="x\x00y"`） | 实测 |
| F10 | `cmd/compile` **看不见普通注释与文档注释**（`newParser` 用 `directives` 模式） | 源码 + 实测 |
| F11 | `//line` 的**位置重映射由 parser 做**，不在 scanner | `syntax/parser.go:76` `updateBase()` |
| F12 | 编译器认识的 `//go:` 指令 **15 条**（另有 `go:embed`/`go:linkname` 分别处理） | `noder/lex.go` 逐条 `case` |
| F13 | 编译器 AST 里的 `Comment`/`CommentKind` 是**死代码**（`printer.go`/`dumper.go` 相关代码全被注释掉，无任何构造点） | 全树 grep |
| F14 | 语料 **11,480** 个 `.go` / **94.2 MB** / 最大 **3.77 MB** / 均值 8.4 KB | `find` + `stat` |
| F15 | `syntax/scanner_test.go` 的 `sampleTokens` 表 **133 条** | 解析源码 |
| F16 | 官方 `TestTokens` **只对 `_Name`/`_Literal` 断言 `lit`**，其余类别不碰 | `scanner_test.go` |
| F17 | 字母分类：Go **145,672** vs Rust `is_alphabetic` **147,421**（多 1,749，全 Mn） | 两端各枚举 0x110000 码点后做集合差 |
| F18 | 数字分类：Go **770** vs Rust `is_numeric` **1,924**（多 1,154，全 No） | 同上 |
| F19 | 官方 Unicode 表版本 = **17.0.0** | `unicode.Version` |
| F20 | 48 条运算符/标点拼写**全部**能切成单一词元（`++`/`--`/`)`/`]`/`}` 额外带出一个自动分号，属正常） | 批量实测 |

---

## 3. 数据模型

### 3.1 位置（`pos.rs`）—— C1 契约的正式实现

```rust
/// 文件标识。不暴露内部整数。D-M1-5
pub struct FileId(u32);

/// 只登记 名字 ↔ Id。**不做** offset↔(line,col) 换算（那是 SourceMap，归 M2/M4）。
pub struct FileTable { /* names: Vec<String> */ }
impl FileTable {
    pub fn intern(&mut self, name: &str) -> FileId;
    pub fn name(&self, id: FileId) -> &str;
}

/// 位置三元组。**1 起**；`col` 是**字节列**（C1 §2.1 两条硬规定）。
pub struct Pos { pub file: FileId, pub line: u32, pub col: u32 }
```

**显示格式**（C1 §2.2，必须与 oracle 的 `文件:行号` 前缀兼容）：

```text
Pos  →  {file}:{line}:{col}          例：test/helloworld.go:3:7
```

### 3.2 词元（`token.rs`）

```rust
pub struct Token { pub pos: Pos, pub kind: TokenKind }

pub enum TokenKind {
    Ident(String),                                          // _Name
    Literal { text: String, lit_kind: LitKind, bad: bool }, // _Literal
    Keyword(Keyword),                                       // 25 个，独立枚举
    Op { form: OpForm, op: Operator, prec: u8 },            // ★ D-M1-10
    Delim(Delim),                                           // ( ) [ ] { } , : . ...
    Semi(SemiOrigin),                                       // 分号，来源可辨
    Eof,
}

/// 运算符的【书写形式】—— 与 `Operator`（语义）正交。D-M1-10
pub enum OpForm { Plain, Assign, IncDec }   // +、+=、++

pub enum LitKind { Int, Float, Imag, Rune, String }
pub enum SemiOrigin { Explicit, Newline, Eof }
```

**`OpForm` 为什么必须有**：官方用**不同的 `tok`** 区分 `+`/`+=`/`++`；
若把它们都压成 `op=Add`，**M2 就无法区分二元表达式与赋值语句**。详见 D-M1-10 的无损性核对表。

**设计要点：`lit` 的有效域从注释变成了类型。**

Go 的扁平结构体里，`lit` 只在 `_Name`/`_Literal`/`_Semi` 有效，其余类别残留上一个词元的值（**F5 实测**）。
Rust 的和类型让"这个字段此刻是否存在"成为**编译期事实** —— 读不到就是编译错误，那类静默失败从根上消失。

**与官方字段的对应关系**：

| 官方字段 | 我们的位置 | 备注 |
|---|---|---|
| `tok` | `TokenKind` 的**判别式** | 一个 `tok` 值拆成多个分支（如 `_Operator` → `Op`） |
| `lit` | **各分支的载荷** | 见上 |
| `kind` | `Literal.lit_kind` | 只在 `Literal` 分支存在 |
| `op` / `prec` | `Op.op` / `Op.prec` | 只在 `Op` 分支存在 |
| —（官方的 `tok` 区分） | **`Op.form`** | ★ D-M1-10：官方靠不同 `tok` 值区分，我们用 `form` 字段 |
| `bad` | `Literal.bad` | 只在 `Literal` 分支存在 |
| `line`/`col` | `Pos` | 独立值对象 |
| `blank` | **不实现** | 官方只用于 `//go:build` 独占一行的判定（`parser.go:85`），M1 不需要 |
| `nlsemi` | `SemicolonPolicy` 的内部状态 | **不是词元的字段** |

### 3.3 配套枚举

| 类型 | 内容 | 来源 |
|---|---|---|
| `Keyword` | **25** 个 | `spec:L255-266` |
| `Operator` | 运算符**语义**（`Add`/`Shr`/`Assign`/`Define`/`Arrow`…）+ `prec`；**书写形式**由 `OpForm` 承载（D-M1-10） | `spec:L268-282` |
| `Delim` | `( ) [ ] { } , : . ...`（`;` 归 `Semi`） | 同上 |
| `Mode` | `Comments` / `Directives` 两个标志位 | D-M1-1 |
| `DiagCode` | §7.2 那张表 | D-M1-9 |
| `Message` | 注释或指令的文本 + 位置 + 类型 | D-M1-1 / D-M1-8 |

---

## 4. 模块结构与依赖方向

```text
rgoc/crates/rgoc-lex/
├── src/
│   ├── lib.rs             公开 API + crate 级文档（含 SPIKE 关系说明）
│   ├── pos.rs             Pos / FileId / FileTable          ← C1 契约
│   ├── token.rs           TokenKind / LitKind / Keyword / Operator / Delim / SemiOrigin
│   ├── source.rs          SourceReader：字节缓冲 + UTF-8 解码 + 位置推进 + 原文切片
│   ├── semicolon.rs       SemicolonPolicy：nlsemi 状态机（LEX-07 单独可见）
│   ├── unicode_tables.rs  ★ 生成物：字母 684 区间 / 数字 72 区间（D-M1-6）
│   ├── diag.rs            Diag / DiagBag / DiagCode         ← C1 契约
│   └── lexer.rs           Lexer：分派 + 各类词元扫描
└── tests/
    ├── golden/*.tokens    切片级 golden（腿甲）
    └── *.rs               六切面测试
```

**依赖方向（硬约束）**：`lexer → {source, semicolon, token, pos, diag, unicode_tables}`，**单向**。

> **`rgoc-lex` 不得依赖任何其他 `rgoc-*` crate** —— 尤其不得引用 AST/HIR 类型。
> 依据：`M0-design.md` 的限界上下文边界；破坏它会让 M5 替换 `rgoc-hir` 时被反向拖下水。
>
> **反向依赖是允许且必需的**：`rgoc-driver → rgoc-lex`（腿乙的 `lex` 子命令要用它）。
> 单向的意思是「词法层不知道上层存在」，不是「谁也不许用词法层」。

**关于 `Pos`/`Diag`/`DiagBag` 的落位**：C1 契约目前只存在于 `rgoc-hir`（`SPIKE-ONLY`，M5 整体替换）。
M1 把它们的**正式实现**落在 `rgoc-lex`，`rgoc-hir` 那份保持原样直到 M5 删除。

- 依据：`03` §2 对 `rgoc-lex` 的定位是 "token / **SourceMap**"
- 收益：M2/M4/M5 向下依赖最底层 crate，**不会出现"为拿一个类型去依赖 SPIKE-ONLY crate"的反向依赖**

---

## 5. 数据流与算法

### 5.1 `SourceReader` —— 状态与不变量

```rust
pub struct SourceReader {
    buf: Vec<u8>,     // 整个文件（M1 的输入是文件，不是管道）
    b: Option<usize>, // 当前段起点（None = 无活动段）
    r: usize,         // 下一个待解码字节的下标
    ch: Option<char>, // ★ 当前【待判定】字符；None = EOF
    chw: u8,          // ch 的字节宽度（1-4）—— 字节列的直接来源
    line: u32,        // 内部 0 起；对外 +1
    col: u32,         // 内部 0 起；对外 +1，单位是【字节】
}
```

**相对官方的三处简化**（D-M1-7「不照抄实现」的具体落地）：

| 官方机制 | 我们的处理 | 理由 |
|---|---|---|
| `io.Reader` 增量填充 + `e` 索引 | **去掉**（一次性读入） | 省掉 `fill()`/`nextSize()` 与对应测试 |
| `sentinel` 哨兵字节 | **去掉** | 它是"ASCII 快路径少一次比较"的微优化；**先正确，后优化** |
| `b` / `r` / `rewind()` | **保留** | `segment()` 与 `..` 的回退都要它 |

**关键不变量**：

| # | 不变量 |
|---|---|
| I1 | `ch` 是**尚未归属**的字符；`r - chw` 是它的字节下标 |
| I2 | `segment()` 返回 `buf[b..r-chw]`，**天然排除 `ch`** |
| I3 | `col` 在每次 `nextch()` 时 `+= chw`；`ch == '\n'` 时 `line += 1, col = 0` |
| I4 | **`col` 的单位是字节** ⇒ 一个 UTF-8 字符横跨多个列号（**F9 已坐实**） |
| I5 | `rewind()` **只服务 `..`**，且活动的段内**不得含换行**（官方 `source.go:99` 的前置条件） |

### 5.2 `next()` 的分派骨架

```text
next():
  ① nlsemi_old = semi.take()        // 取走上一次的值并复位
  ② loop:                            // ← redo 回边（注释 / 非法字符走这里）
       ③ skip_whitespace(nlsemi_old) // '\n' 仅在 !nlsemi_old 时被吞 ★LEX-07 第一处机关
       ④ pos = mark_token_start()
       ⑤ if at_eof: nlsemi_old ? Semi(Eof) : Eof     // ★EOF 也补分号
       ⑥ 按首字符分派（顺序照官方）：
            letter 或 (非 ASCII 且是标识符字符) → ident()
            '\n'      → Semi(Newline)                  // 只有 nlsemi_old 时才会走到
            '0'..'9'  → number(false)
            '"'       → std_string()
            '`'       → raw_string()
            '\''      → rune()
            '/' 且是注释 → 上报 + continue（回到 ②）
            其他      → punct() 或报"非法字符"后 continue
  ⑦ semi.set_after(&kind)  // 按类别决定下一个换行是否译为分号
```

### 5.3 五个局部算法

| 算法 | 核心 | 易错点 |
|---|---|---|
| **ident** | ASCII 快路径 → Unicode 一般路径 → **切完再查关键字** | 谓词必须用 `unicode_tables`，**不能用 Rust std**（D-M1-6） |
| **number** | 前缀定 base → `digits()` 返回 **digsep 位集** → 指数 → 后缀 `i` → **事后** `invalidSep()` | 十六进制浮点必须带 `p` 指数；`_` 的非法位置是**事后**定位 |
| **escape** | `(n, base, max)` 表驱动：简单转义 / `\ooo`(3,8,255) / `\x`(2,16,255) / `\u`(4,16,MaxRune) / `\U`(8,16,MaxRune) | 代理区、`\ooo > 255`、未知转义各有独立诊断 |
| **operator 最长匹配** | 显式前瞻 + `assignop` 汇合点 | `&^=`、`<<=`、`...` 最易漏；**唯一回退点是 `..`** |
| **comment** | 行注释**故意不吃 `\n`**；跨行块注释在 `nlsemi` 语境下等价于换行 | 这两条是分号规则的输入 |

### 5.4 `SemicolonPolicy`

```rust
pub struct SemicolonPolicy { nlsemi: bool }
impl SemicolonPolicy {
    pub fn take(&mut self) -> bool;              // 进入 next() 时取走并复位
    pub fn set_after(&mut self, k: &TokenKind);  // 产出词元后，按类别决定
}
```

**`set_after` 的完整规则表**（必须逐行可测）：

| 词元类别 | 置位 |
|---|---|
| `Ident` / `Literal` | ✅ |
| `Keyword` ∈ {`break`, `continue`, `fallthrough`, `return`} | ✅ |
| `Keyword` 其余 21 个 | ❌ |
| `Op { form: IncDec }`（即 `++` / `--`） | ✅ |
| `Op` 其余（二元运算符、`=`、`:=`、`<-`、`*`） | ❌ |
| `Delim` ∈ {`)`, `]`, `}`} | ✅ |
| `Delim` 其余（`(` `[` `{` `,` `:` `.` `...`）、`Semi`、`Eof` | ❌ |

> **为什么独立成对象**：LEX-07 是本阶段**唯一跨调用保留状态**的地方。埋在 `next()` 里就只能靠端到端 golden 反推。

### 5.5 注释与指令通道

| mode | 上报什么 | 落到哪个收集器 |
|---|---|---|
| 默认（无标志） | **什么都不上报** | — |
| `Comments` | 全部注释（`//` 与 `/* */`，含 `//go:`） | `messages` |
| `Directives` | 仅 `//go:` 与 `//line`（且需符合位置约束） | `messages` |

**位置约束（实测自官方 `parser.go:59`）**：
- `//line` 必须**位于行首**（`col == colbase`）或写作 `/*line*/`
- `/*line*/` 在多行注释里**无效**（由 `updateBase` 自行丢弃）

**M1 只交出文本，不做任何解析**（D-M1-1）。

---

## 6. 字符分类（`unicode_tables.rs`）

**问题**（F17/F18 实测）：LEX-02 要的是广义类别（字母 = Lu/Ll/Lt/Lm/Lo；数字 = Nd），
而 Rust std 只能给 Alphabetic 属性与 `N*`，**两者都比 Go 宽**：

| 谓词 | Go | Rust std | 差 |
|---|---|---|---|
| 字母 | 145,672 | 147,421 | **+1,749**（全 Mn，如 U+0345） |
| 数字 | 770 | 1,924 | **+1,154**（全 No，如 U+00B2 `²`） |

**后果**：`x²` 在 Go 里是 `x` + 非法字符，用 Rust std 会**整个切成一个标识符**。
**静默错误，没有运行期提示。**

**解法（D-M1-6）**：由 oracle 生成两张区间表。

| 表 | 区间数 | 生成方式 |
|---|---|---|
| `LETTER_RANGES` | 684 | 遍历 0x0..0x10FFFF，取 `unicode.IsLetter(r)`（跳过代理区）并为连续段 |
| `DIGIT_RANGES` | 72 | 同上，`unicode.IsDigit(r)` |

- 紧凑表示 `&[(u32, u32)]` ≈ **5.9 KB**
- 文件头**必须**注明：来源（go1.27.1 的 `unicode` 包）、Unicode 版本 **17.0.0**（F19）、生成方式、**勿手改**
- 查找用二分（区间有序）

**验收判据**：把生成器重跑一遍，产出的表**逐字节相同**。

---

## 7. 错误处理与资源上限

### 7.1 恢复原则：**报错但继续，产出畸形词元**

实测的官方行为（M1 照做）：

| 输入 | 官方行为（实测） |
|---|---|
| `1\xff2` | 报 `invalid UTF-8 encoding`，**仍产出** `Literal text="1\xff2"` |
| `x\0y` | 报错，**仍产出** `Ident "x\0y"` |
| 文件中 BOM | 报错，**仍产出**含 BOM 的标识符 |
| `0x` | 报错，**仍产出** `Literal kind=Int bad=true` |
| `'ab'` | 报错，**仍产出** `Literal lit_kind=Rune bad=true` |

**共同形状**：**从不"失败退出"，总是"给你一个可能畸形的词元 + 一条诊断"。**

### 7.2 诊断代号表（`DiagCode`）

| DiagCode | 触发 | 官方对应消息 |
|---|---|---|
| `LEX-INVALID-UTF8` | 非法 UTF-8 序列 | invalid UTF-8 encoding |
| `LEX-INVALID-NUL` | NUL 字符 | invalid NUL character |
| `LEX-BOM-MIDFILE` | 文件中部 BOM | invalid BOM in the middle of the file |
| `LEX-BAD-CHAR` | 不认识的字符 | invalid character %#U |
| `LEX-BAD-IDENT-CHAR` | 标识符里混入非法字符 | invalid character %#U in identifier |
| `LEX-ID-LEADING-DIGIT` | 标识符以数字开头 | identifier cannot begin with digit %#U |
| `LEX-COMMENT-UNTERMINATED` | 块注释未终止 | comment not terminated |
| `LEX-LIT-NO-DIGITS` | `0x`/`0b`/`0o` 无数字 | %s literal has no digits |
| `LEX-BAD-SEP` | `_` 位置非法 | '_' must separate successive digits |
| `LEX-BAD-EXPONENT` | 指数缺数字 / 进制不匹配 | exponent has no digits；%q exponent requires … mantissa |
| `LEX-RUNE-EMPTY` | rune 字面量为空 | empty rune literal or unescaped ' |
| `LEX-RUNE-MULTI` | rune 字面量多字符 | more than one character in rune literal |
| `LEX-RUNE-NEWLINE` | rune 字面量含换行 | newline in rune literal |
| `LEX-RUNE-UNTERMINATED` | rune 字面量未终止 | rune literal not terminated |
| `LEX-ESCAPE-UNKNOWN` | 未知转义 | unknown escape |
| `LEX-ESCAPE-RANGE` | 转义值超范围 / 代理区 | escape is invalid Unicode code point %#U |
| `LEX-ESCAPE-OCTAL-OVER` | 八进制转义 > 255 | octal escape value %d > 255 |
| `LEX-STRING-UNTERMINATED` | 字符串未终止 | string not terminated |
| `LEX-EXCEEDS-FILE-LIMIT` | 超文件上限 | **官方无对应**（实现限制，D-M1-4） |

> **最后一行的写法是刻意的**：它必须明确标注「官方无对应」。这是**我们自己的限制**，
> 不得伪装成对齐官方。

**消息文本**：不复刻（D-M1-9）。`DiagCode` 参与 C1 §4 的排序（第 4 排序键）。

### 7.3 资源上限：**只设一个**

| 项 | 值 | 依据 |
|---|---|---|
| **单文件上限** | **64 MiB** | 语料最大文件 3.77 MB（F14），即 **17 倍**余量 |

**为什么只设一个**：单个词元的长度**不可能超过文件大小**（词元是文件的连续子串）。
限住文件 ⇒ 词元自动有界 ⇒ 内存有界（最坏 ≈ 缓冲 64 MiB + 一个词元副本 64 MiB）。

**比"两个上限"好在哪**：不需要回答"单个词元超限后游标往哪走"这个没有标准答案的问题。
少一个上限 = 少一套恢复逻辑 = 少一组测试。

**检查时机**：**必须在读文件之前**用 `fs::metadata().len()` 判断。
否则 10 GB 的文件在 `fs::read` 那一步就已经 OOM —— **检查放在读完以后等于没检查**。

**超限行为**：产出一条 `LEX-EXCEEDS-FILE-LIMIT` 诊断，返回**只含 `Eof` 的词元流**。
**不 panic，不静默截断。**

---

## 8. 测试策略

### 8.1 两条腿（D-M1-2）

| 腿 | 代码位置 | golden 位置 | 跑什么 | 频率 | 判据 |
|---|---|---|---|---|---|
| **甲** | `rgoc-lex/tests/*.rs` | `rgoc-lex/tests/golden/` | 六切面的细粒度用例 | 每次改代码 | Rust `#[test]` 断言 |
| **乙** | `rgoc-driver` 的 `lex` 子命令 + 门禁脚本 | **`rgoc/tests/smoke/lex/`**（`03` §2 已规划 `tests/{smoke,milestone,corpus}/`） | 全文件 golden（词元流 + 诊断流） | 阶段门禁 / CI | **逐行 diff** |

**`SM-M1-LEX-GOLDEN`** 在 `03` 的「**M1 smoke 清单**」里 ⇒ 腿乙不需要动 C2 契约。

**两条腿的分工边界**（避免"两处都测"变成"两处都不深")：
**腿甲测规则的每一条**（枚举型，求覆盖）；**腿乙测真实文件的整体行为**（抽样型，求端到端）。
一条规则若只在腿乙里被间接覆盖，等于没被专门测过。

### 8.2 golden 格式（冻结）

```text
# golden: <语料相对路径>
# source: oracle | hand-written        ← ★ 必须标注来源
== tokens ==
  1:1    package  lit=-
  1:9    name     lit="main"
  1:13   ;        lit="newline"
  4:10   literal  lit="\"hello\"" kind=4 bad=false
  ...
== diags ==
  4:8    LEX-INVALID-UTF8
```

**四条规范化纪律**（全部为实测踩出来的）：

| # | 纪律 | 踩坑经历 |
|---|---|---|
| 1 | **只录有效域内的字段** —— `lit` 只对 `_Name`/`_Literal`/`_Semi` 记录 | 第一版无条件录 `lit` ⇒ `(` 的 `lit` 显示为上一个词元的 `main`，**把实现细节写进契约** |
| 2 | **诊断与词元分两条流、各自有序**（C1 §4 的 `line→col→code`） | 第一版按事件顺序输出 ⇒ 诊断 `E 4:8` 出现在词元 `T 4:7` **之前**，golden 编码了内部时序 |
| 3 | **只存 `Pos` + `DiagCode`，不存消息文本** | D-M1-9 |
| 4 | **确定性** —— 同一输入多次运行产出的字节完全相同 | 已实测：3 次 `sha256` 一致 |

**两种来源必须标注**：

| 来源 | 覆盖 | 标记 |
|---|---|---|
| oracle 生成 | 绝大多数 | `# source: oracle` |
| **手写** | **只有实现限制类**（`LEX-EXCEEDS-FILE-LIMIT`，官方无此行为） | `# source: hand-written` |

> 不标注的后果：将来有人会以为"超限报错"也是 oracle 说的。

### 8.3 六个切面 → 测试组

| 组 | 切面 | 典型用例 | 判据 |
|---|---|---|---|
| `T-L-1x` | ① 解码与位置 | 中文/emoji 列号、BOM 三处、NUL、CRLF、非法 UTF-8、私有区字符 | oracle 对照 |
| `T-L-2x` | ② 切分边界 | 能/不能续接成对、`for` vs `forx`、`1str`、`_x`、`..` 回退 | oracle 对照 |
| `T-L-3x` | ③ 词元载荷 | 25 关键字、48 运算符（F3/F20）、5 字面量子类、转义全表、进制全表 | oracle 对照（**只在有效域内断言**，F16） |
| `T-L-4x` | ④ 分号插入 | 三种来源、`)`/`}` 后补、`(`/`{` 后不补、行注释吃不吃换行、跨行块注释 | oracle 对照 + `set_after` 表逐行 |
| `T-L-5x` | ⑤ 错误恢复 | 每条负例**同时断言诊断与后续词元** | oracle 对照 |
| `T-L-6x` | ⑥ 流层不变式 | 位置单调、拼接等价、三次一致、`LEX-EXCEEDS-FILE-LIMIT` | **自证**（无 oracle） |

**ID 命名**：`T-L-01..NN` **顺序编号**；切面写在文档的列里而**不是 ID 里**。

> 依据：M0 的 `M0-tests.md` §2 用 `T-H-` / `T-C-` / `T-S1-` 前缀 + 编号；
> 而"写死的键列表会变成盲区"是本仓踩过三次的坑（`AGENTS.md`）。用例换组时 ID 不动。

**已知可用的 golden 来源（F15）**：`syntax/scanner_test.go` 的 `sampleTokens` 表 **133 条**
（`{tok, src, op, prec}` 四元组，`src` 即期望 `lit`），分布：`_Literal` 48 / `_Operator` 20 /
`_Name` 12 / `_AssignOp` 11，25 个关键字各 1。

---

## 9. oracle adapter（D-M1-3）

### 9.1 组成

```text
tools/token-oracle/
├── README.md                 来源、版本、校验值、再生成步骤
├── gosyntax/                 vendor 的官方五文件（BSD-3-Clause，保留许可头）
│   ├── scanner.go            881 行
│   ├── source.go             218 行
│   ├── tokens.go             159 行
│   ├── token_string.go        70 行
│   └── operator_string.go     46 行
└── adapter.go                唯一手写的部分：调 next() 并按 §8.2 格式化
```

### 9.2 为什么必须写这个程序

官方 `next()` **不返回值** —— 结果写在自己的字段上（`s.tok` / `s.lit` / `s.kind` / …）。
所以**没有任何现成命令能直接吐出词元流**：

- `go build` 不给词元流
- **`go tool compile -E` 给的是语法树 dump**（`03` §M1 明令不得当 token oracle）

### 9.3 一个实测的坑

**样本文件不能叫 `.go` 且与 adapter 同目录** —— 会 `main redeclared in this block`。
改名为 `.txt` 即可。

---

## 10. 契约与产物影响

### 10.1 C1：留位 → 完整

| 项 | 变化 |
|---|---|
| `Pos.file` | `String` → **`FileId`** |
| 新增 | `FileTable`、`DiagCode` |
| **不规定**（保持不变） | offset↔(line,col) 换算、诊断抑制/去重、跨文件映射 |
| 排序规则 | **不变**（`file → line → col → code`） |

**按 `AGENTS.md` §5 走契约变更记录。**

### 10.2 C2 / C3 / C4 / C5

| 契约 | 影响 |
|---|---|
| **C2**（Test IR） | **不动** —— 依据 §8.1，`SM-M1-LEX-GOLDEN` 属 smoke 层 |
| C3 / C4 / C5 | 无关（HIR / SSA / ABI，M5+） |

> **C2 不动是一条正面结论**：M0 那份「唯一完整级契约」在 M1 不需要修改。

### 10.3 新增产物

| 产物 | 规模 | 来源标注 |
|---|---|---|
| `tools/token-oracle/gosyntax/` | **1,374 行** Go（BSD-3-Clause） | 官方 `gc/syntax/` 五文件，go1.27.1；保留许可头 |
| `rgoc-lex/src/unicode_tables.rs` | **756 区间 ≈ 5.9 KB** | 由 oracle 生成，Unicode 17.0.0 |

两者**都不是手写的**，都必须写明「来源 + 生成方式 + 勿手改」。

---

## 11. M1 门禁汇总

| 门禁 | 由什么判定 | 通过条件 |
|---|---|---|
| **LEX-01..13** | `T-L-*` 各切面测试 | **100%** 通过 |
| **`SM-M1-LEX-GOLDEN`** | 腿乙的 golden diff | 全部文件 diff 为空 |
| **`SM-M0-NATIVE-HELLO`** | 回归 | 必须**继续通过**；输入**不经 lexer**，**不得改写** |
| **四条 cargo 命令** | `cargo fmt --check` / `check --workspace --all-targets` / `clippy … -D warnings` / `test --workspace` | 全过 |
| **C1 升为完整** | 契约变更记录 | 存在且一致 |
| **GAP-01 的 LEX 13 项** | 四维归属表 | 已填（**不试图填完 210 个**） |

**判定纪律**（沿用 `M0-tests.md` §8）：

1. 必需 ID **100% 通过**、无基建失败 / 不稳定失败
2. 白名单**开工前冻结**，新增跳过须**单独记录**并保留原报告
3. **不得**通过放宽比较器、把跳过记为 pass、或缩小分母来「达成」门禁
4. 断言必须**锚定真实值**并**反向验证**（写完要故意制造它能抓的 bug，看是否真变红）

---

## 12. 风险与未决

| # | 项 | 处置 |
|---|---|---|
| R1 | **LEX-06 的计数不一致** —— `01` 写「47 个」，spec 的 `<pre>` 实测 **48 条**（F3） | **提议按 48 修 `01`**（该节正文枚举**含 `;`**，而 `;` 另由 LEX-07 覆盖）。修订须同步 `02` 与 `AGENTS.md` §5 |
| R2 | **GAP-02 正则子集** —— M0 交接 §7.1 点名「会在 M1 之后暴露」 | **不阻塞 M1**（M1 的 golden 是词元级）。但**M1 结束前必须裁决**：扩展子集还是引 crate |
| R3 | 新增第三方依赖 | ✅ **不需要**（D-M1-6 用 oracle 生成表，绕开授权问题） |
| R4 | vendoring 的同步成本 | 五文件跟语料版本手动同步；`tools/token-oracle/README.md` 记录来源版本与校验值 |
| R5 | `char::is_alphabetic` 之类的**近似谓词**在别处复现 | M1 完成后全仓 grep `is_alphabetic`/`is_numeric`/`is_alphanumeric`，确认零使用 |
| R6 | 位置漂移（"差 1"类错误） | `T-L-1x` 必须含**中文与 emoji** 样本 —— 否则字节列写错也看不出来 |
| R7 | 超预算 | 按 `03` §6.1 的预算（smoke ≤5 分钟）；**不得**通过砍 LEX 项或放宽判据来达标 |

---

## 13. 与后续文档的接口

### 13.1 交给 `writing-plans` 的输入

| 输入 | 出处 |
|---|---|
| 九个决策 + 五字段论证 | §2 |
| 数据模型（可直接落成类型定义） | §3 |
| 模块结构与依赖方向 | §4 |
| 算法骨架与规则表 | §5 |
| 诊断代号表 | §7.2 |
| 六切面与 ID 命名 | §8.3 |
| adapter 组成 | §9 |

### 13.2 `M1-tests.md` 必须覆盖

- LEX-01..13 的**逐项**测试 ID（不得有项无测试）
- 六切面的具体用例清单与**期望值来源**（oracle 生成 / 手写）
- 负例清单及其**双向断言**（诊断 + 后续词元）
- `SM-M0-NATIVE-HELLO` 的回归方式
- 分母：M1 的冻结测试 ID 总数（**开工前冻结**）

### 13.3 与后续阶段的衔接

| 交给 | 什么 |
|---|---|
| **M2** | 词元流 + 注释/指令文本（`messages`）+ `Pos`/`FileTable`；`//line` 重映射由它做 |
| **M3** | 字面量的原文（`Literal.text`）—— 值由 M3 算 |
| **M4** | `DiagCode` 表 + `DiagBag` 的排序规则（与 errorcheck 对齐的基础） |
| **M5** | C1 契约随 `rgoc-hir` 的删除迁移到 `rgoc-lex`（本阶段已就位） |

---

## 14. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-11 | **初稿创建**。九个决策（D-M1-1..9）经逐项确认；含 20 条实测事实基线（F1..F20） | 本仓设计会话；D-M1-6 的 2,903 码点差异为实测量化 |
| 2026-10-11 | **新增 D-M1-10（`Op.form`）**。写任务计划时发现 §3.2 的原型**丢信息**（`+` 与 `+=` 同形）—— 这是缺陷修补，非设计期遗漏。同步改了 §3.2、§3.3、§5.4 | `writing-plans` 阶段的自行核对 |
