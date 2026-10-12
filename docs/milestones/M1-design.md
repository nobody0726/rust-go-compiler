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
| D-M1-11 | 诊断代号表的导出方式 | **枚举全部报错点** + 冻结条目数 + 跨语言对撞断言（19 → 23） | 2026-10-11 |
| D-M1-12 | 词元载荷的载体 | **字节**（`Vec<u8>`）而非 `String` —— `String` 装不下非法 UTF-8 | 2026-10-11 |

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
| **否决理由** | ① **实测不一致**：字母 Rust 多 **1,749** 码点（全为 Mn），数字 Rust 多 **1,154** 码点（全为 No），合计 **2,903** 个码点行为不同 ⇒ **诊断层面**不一致（`x²` 两侧都是**一个**标识符，但 Go 会报一条 `LEX-BAD-IDENT-CHAR`，近似谓词**静默通过**；起始位置还会给出不同代号，见 F29），**静默错误**；② 授权成本 + 需评估 Unicode 版本漂移；另 `unicode-ident`（Cargo.lock 已有，syn 的传递依赖）**也不适用** —— 它实现 UAX#31 的 XID，与「广义类别」规则不同 |
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

#### D-M1-11　诊断代号表的导出方式

> **本决策同样是执行期的缺陷修补**（`T-L04`）：设计时那张表是**凭印象手写**的。

| 字段 | 内容 |
|---|---|
| **判据** | 官方 scanner 报的每条诊断都要有代号；缺一条时**行为未定义**，且不会报错 |
| **候选** | ① 保持手写清单 ② 只列"常见"的若干条 ③ **枚举全部报错点 + 冻结条目数 + 跨语言对撞断言** |
| **结论** | **③**。`DiagCode` 由 **19 → 23**；条目数冻结为 25；Rust 码表与 Go 映射表由断言对撞 |
| **否决理由** | ① 实测已漏 4 条（`LEX-BAD-DIGIT` / `LEX-BAD-RADIX-POINT` / `LEX-STRING-NEWLINE` / `LEX-ESCAPE-BAD-DIGIT`）且**无任何报错**；② "常见"没有判据，只是把漏项藏得更深 |
| **失效条件** | 若官方把消息模板改成**不可静态枚举**（比如由数据驱动生成）⇒ 枚举法失效，需改为"运行时落盘再比对" |

**落地**：`tools/token-oracle/diagmap.go`（映射表 + `--selfcheck`）、
`rgoc-lex/src/diag.rs`（23 个变体）、跨语言对撞断言在 `tests/test_diag.rs`。

#### D-M1-12　词元载荷的载体

> **执行期缺陷修补**（`T-L04`）：`T-L03` 已按 `String` 实现，实测后必须改。

| 字段 | 内容 |
|---|---|
| **判据** | 词元载荷要能**原样**承载官方产出的原文，否则 golden 无法逐字节对齐 |
| **候选** | ① `String`（原设计） ② `String` + 有损转换 ③ **`Vec<u8>`** ④ `Lexeme` newtype 包 `Vec<u8>` |
| **结论** | **③**。`Ident(Vec<u8>)`、`Literal.text: Vec<u8>` |
| **否决理由** | ① **表达不了实测行为**：`x\xff y` ⇒ `lit="x\xff"`，`0xFF` 不是合法 UTF-8，`String` 在类型上就装不下；② `from_utf8_lossy` 把非法字节换成 U+FFFD ⇒ **静默丢信息**，golden 会因"少一个字节"而红且找不到原因；④ 只是给 `Vec<u8>` 换个名字，而 M2 真正需要的那一步（转 `&str`）本身就只有一行 `str::from_utf8` |
| **失效条件** | 若 M2 表明它从不需要原始字节（只吃 `&str`）⇒ 可在 M2 收窄；但那时改动面已在 M2 |

**范围界定**：NUL / BOM（U+FEFF）/ 私有区字符（U+E000）**都是合法 UTF-8**，实测能进 `String` ——
所以这条**只为非法字节序列**服务。

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
| F21 | 官方对 `=` / `:=` / `<-` **不设 `op`** —— `op` 的有效域不含 `_Assign`/`_Define`/`_Arrow`（`scanner.go:42`） | 源码 + adapter 实测 |
| F22 | **标识符的原文可含非法 UTF-8**：`x\xff y` ⇒ `name lit="x\xff"` | oracle 实测 |
| F23 | **`bad=false` 与「原文含非法字节」可以同时成立**：`1\xff2` ⇒ `kind=0 bad=false` —— `bad` 只反映字面量**语法** | oracle 实测 |
| F24 | 官方 scanner 的诊断消息模板去重后共 **25** 条（26 处错误调用点：scanner 23 / source 3） | 枚举全部 `errorf`/`errorAtf`/`error` |
| F25 | 伪终端 ONLCR 会把 stdout 的 `\n` 翻成 `\r\n`（终端里**看不出**）⇒ 冻结 golden 必须 `RGOC_TTY=0` | 实测：20 行输出 20 个 CR vs 0 个 |
| F26 | **官方 `nextch` 的 NUL 检查只写在 ASCII 快路径里**，而懒填充让**首字节**走不到那条路 ⇒ 文件首字节的 NUL 不被报出；它作为普通字符送出，随后由 `next()` 报 `LEX-BAD-CHAR` | 字符级对撞（1177 例，31 例偏离） |
| F27 | **字符层与 oracle 的密集对撞结果：1177 例中 1146 例逐字节一致，31 例偏离**（全部是"首字节为 `0x00`"，同一根因） | `tests/test_char_golden.rs` |
| F28 | **容器内 `set -e` 生效时，显式 `exit N` 一律返回 1**（`exit 0`/`exit 3`/`exit 7` 实测都得 1）；无 `set -e` 时正常传播。且 `cmd && echo OK` 形态**不中止脚本** ⇒ 早期失败被后续成功掩盖（`false && echo OK; true` → 退出码 0） | `bash -lc` 逐形态实测（R12） |
| F29 | **`x²` 这类字符在 Go 里是「吸收 + 报错」，不是「切成两个词元」** —— 官方 `atIdentChar` 对「不是字母、也不是 Nd」的非 ASCII 字符走 `case s.ch >= utf8.RuneSelf:` 分支：**先 `errorf` 再 `return true`** ⇒ 该字符**被吸收进标识符**，同时产出一条 `LEX-BAD-IDENT-CHAR`。Rust std 的近似谓词（`is_numeric`/`is_alphabetic`）把它当成数字/字母 ⇒ **静默通过**。**差异在诊断层面，不在词元边界** | oracle 逐例实测（`x²` / `²abc` / `x\u{0345}` / `xµ` / `x１`） |
| F30 | **`..` 后面紧跟 `\n` 时官方会把行号多加一次**，且**后续所有位置都跟着偏移** —— `rewind()` 内部调的是**完整**的 `nextch()`，而它会按**前一个字符**（`..` 之后的那个）判换行。实测：`..\n` ⇒ `.`@1:1、`.`@**2:2**、EOF@3:1；`x\n..\ny\n` ⇒ `.`@2:1、`.`@**3:2**、`y`@**4:1**。⇒ 这条规则**可以精确写下来**（不像 F26 那种依赖缓冲填充时机的产物），所以**照抄** | oracle 逐例实测（T-L09） |
| F31 | **`1..2` 里根本没有 `..`** —— `1.` 抢走第一个点、`.2` 抢走第二个 ⇒ `literal "1."` + `literal ".2"`（两个 Float）。⇒ `..` 只有在**前面不是数字**时才可能出现 | oracle 实测（顺带纠正 `M1-tests.md` 的 `T-L-26`） |
| F32 | **指数分支里 `digsep` 是 `=` 不是 `|=`**（官方 `digsep = s.digits(10, nil) | digsep&2`） —— 尾数的"有数字"位被**丢弃**、只保留"有 `_`"位。⇒ `1.5e_` / `1e_` / `1_5e_` 报的是「指数缺数字」而**不**报 BAD-SEP（`ok` 已假，`invalidSep` 那步被跳过）；`1e1_0` / `0x1p1_0` / `1_0e1_0` **合法**。写成 `|=` 两个方向都错（少报 EXPONENT、多报 SEP） | oracle 逐例实测（T-L10 / RF3） |
| F33 | **`invalidSep` 把进制前缀算作一个"数码"**（⇒ `0x_1f` / `0X_1F` 合法）；**前导 `0` 也算**（⇒ `0_1` 合法；`0_` 报在 `_` 上，`0x1_` 同理） | oracle 逐例实测（T-L10） |
| F34 | **进制里非法数字的偏移是"相对词元起点"**（官方 `col - s.col`）：`089` 报 **1:2**（起点 1 + 偏移 1）、`0b12` 报 **1:4**（偏移 3）；把 `089` 推到第 4 列（`x:=089`）报 **1:5**。且只记**第一个**越界数字（`089` 报 `8` 不报 `9`） | oracle 逐例实测（T-L10） |
| F35 | **`0x1g` ⇒ literal `0x1` + ident `g`，零诊断** —— `digits(16)` 没有"越界数字"概念（十六进制里不存在越界），非十六进制字符直接停下 | oracle 实测（T-L10） |
| F36 | **简单转义的白名单与引号种类绑定** —— 官方 `escape(quote)` 的第一个 `case` 就是 `quote` 本身 ⇒ 在 **rune** 里 `\'` 合法而 `\"` 是 `LEX-ESCAPE-UNKNOWN`（字符串里正好相反：`\"` 合法、`\'` 非法） | oracle 实测（T-L11） |
| F37 | **八进制转义是定长 3 位**，且 `8`/`9` 不在 `case '0'..'7'` 里 ⇒ `'\0'` / `'\07'` 报 `LEX-ESCAPE-BAD-DIGIT`（报在**闭引号**上），而 `'\8'` 报的是 `LEX-ESCAPE-UNKNOWN`、`'\18'` 才是 BAD-DIGIT | oracle 逐例实测（T-L11） |
| F38 | **"字符数不对"的两种错误报在不同位置**：`''` 的 `LEX-RUNE-EMPTY` 报在**闭引号**（`errorf`），`'ab'` 的 `LEX-RUNE-MULTI` 报在**开引号**（`errorAtf(0)`）；且**转义算一个字符**（`'\n\n'` / `'\\n'` ⇒ MULTI） | oracle 逐例实测（T-L11） |
| F39 | **原始字符串里的 `\r` 留在 M1 的文本里** —— 官方 `rawString()` 的注释原文：「We leave CRs in the string since they are part of the literal (even though they are not part of the literal value)」。⇒ 「`\r` 被丢弃」（`01` 的 LEX-13 / `spec:L553-631`）说的是**值**，那是 M3 的 unquote 的事；**M1 的载荷是逐字节原文**，一个字节都不动。解释型里的裸 `\r` 同理保留 | oracle 实测（T-L12；顺带纠正 `M1-plan.md` T-L12 简报里「`\r` 被丢弃（不进入载荷）」的说法） |
| F40 | **`stdString` 的换行/EOF 分支没有 `if ok` 守卫，而 `rune()` 有** —— ⇒ `"\q` + 换行报**两条**诊断（`LEX-ESCAPE-UNKNOWN` + `LEX-STRING-NEWLINE`），`'\q` + 换行只报一条。**「顺手统一」成同一个形状会少一条诊断** | oracle 逐例实测（T-L12） |
| F41 | **注释相关的三处位置全部取「注释起点」**：① 上报位置（p01：`// c` 报在 1:8，不是行尾）；② 未终止诊断（p05：`comment not terminated` @1:3 = 注释起点）；③ 跨行块注释补出的 `Semi`（p24：@1:2 = 注释起点 —— 官方 `case '*'` 比较跨行时读的 `s.line/s.col` 是 token 起点、全程未被更新） | oracle 实测（T-L13，探针 p01/p05/p24） |
| F42 | **注释文本保留原始字节**：`\r` 在文本里（p10/p44；官方 TestComments 的 `"// want this one\r"` 同款）、非法 UTF-8 也在（p14：`// a\xffb`）⇒ `Message.text` 必须是 `Vec<u8>`，`String` 装不下 | oracle 实测（T-L13，探针 p10/p14/p44） |
| F43 | **`directives` 模式不做行首检查** —— 行中间的 `//line a.go:1` / `/*line a.go:1 */` 照样上报（p08/p22/p23）。「`//line` 必须在行首」是 **parser** `updateBase()` 的规则（F11），**scanner 层没有这条** | oracle 实测（T-L13） |
| F44 | **指令前缀匹配的两种形态不对称**：行注释认 `go:` **与** `line `；块注释**只认 `line `**（`/*go:build */` 不上报，p45）。且 `line ` **带尾随空格**（`//line` / `//linea foo` 都不算，p09 矩阵）、`go:` 带冒号（`//gofx` 不算、`//go:foo` 算） | oracle 实测（T-L13，探针 p09/p35–p42/p45） |
| F45 | **未终止的跨行块注释 + `nlsemi` ⇒ 仍产 `Semi`** —— 判定在 `fullComment()` **返回之后**执行、与闭合成败无关（p06：`x /* c\n` ⇒ `Semi`@1:3 **外加** `LEX-COMMENT-UNTERMINATED`@1:3；未终止时**不上报**半截文本） | oracle 实测（T-L13，探针 p06/p30） |
| F46 | **官方 `token.String()` 有两处「不细分」+ 一个特例**（渲染器必须照抄，否则 golden 全错）：`_IncOp` → `"opop"`（`++`/`--` **同名**）、`_Operator` → `"op"`（所有二元/一元运算符同名）、`_AssignOp` → `"op="`；而 `_Star` → `"*"` 是**唯一**带专门拼写的运算符族（官方 `case '*'` 特意 `don't goto assignop`）。细分拼写在 **`op=` 字段**里（`+`/`-`/`==`/`&^`…） | 官方 vendored `token_string.go`/`operator_string.go` 直读 + golden 印证（T-L15，n12/n13） |
| F47 | **同位置双诊断的排序输出与「发生时序」相反**（`T-L-54` 的最强端到端实例）：`0x\`+换行 ⇒ 两条诊断同在 **1:3** —— errh 回调顺序（发生时序）是「hexadecimal literal has no digits」先、`invalid character '\'` 后；而按 C1 §4（`line→col→code` 字符串）排序后，golden 里 **`LEX-BAD-CHAR` 在前**（`B`<`L`）。⇒ **排序键不是可选项**：按发生顺序渲染会产出与 golden 不同的字节 | oracle 实测（T-L16；候选由**片段笛卡尔扫描**产出 —— 157464 个输入 / 10236 个同位置命中里取最简） |
| F48 | **`"\` + 换行 ⇒ 两条同位置诊断**：`ESCAPE-UNKNOWN` 与 `STRING-NEWLINE` 同在 1:3（反斜杠后紧跟换行时，两处报错落在同一个「当前字符」上）—— F40 的双诊断除 `"\q\n`（1:3/1:4）外还有**同位**形态。作为 `T-L-54` 的对照实例（其字典序与时序恰好相同，`E`<`S`） | oracle 实测（T-L16，探针 `fixtures/recovery/t54_same.txt`） |

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
    Ident(Vec<u8>),                                          // _Name   ★ D-M1-12
    Literal { text: Vec<u8>, lit_kind: LitKind, bad: bool }, // _Literal ★ D-M1-12
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

**`Vec<u8>` 为什么不是 `String`（D-M1-12）**：官方实测（2026-10-11，本仓 oracle）

```text
输入 (hex)            输出
78 FF 20 79           name lit="x\xff"      ← 标识符的原文里就有非法字节
31 FF 32              literal lit="1\xff2"
```

`0xFF` 不是合法 UTF-8，而 Rust 的 `String` **在类型上**就装不下它。
若用 `String`，要么编译不过，要么被迫 `from_utf8_lossy`（把非法字节换成 U+FFFD）——
**后者是静默丢信息**：golden 比对会因为"少了一个字节"而红，且红得找不到原因。

**注意范围**：NUL / BOM（U+FEFF）/ 私有区字符（U+E000）**都是合法 UTF-8**，实测能正常进 `String`。
所以这条改动**只为非法字节序列**服务 —— 不要因为"要字节"就顺手把别的东西也改成字节。

**`bad` 不是"原文可信"的判据**：实测 `1\xff2` 是 `kind=0 bad=false` ——
非法 UTF-8 由字符层（`nextch`）报出，不经数字语法，所以 `bad` 不变真。

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
| `b` / `r` / `rewind()` | **保留** | `segment()` 与标点层的回退（`..` / `.`+数字的交接）都要它 |

> ⚠️ **去掉 `fill`/`sentinel` 有一个可观测的后果**（F26 / §12 R11）：
> 官方把 NUL 的检查**只写在 ASCII 快路径里**，而懒填充让**首次** `nextch` 时
> `buf[0]` 还是 sentinel ⇒ **文件首字节的 NUL 不被报出**（由 `next()` 兜底报 `LEX-BAD-CHAR`）。
> 我们因为一次性读入，**总是**报 `LEX-INVALID-NUL`。**两侧都报错**，差别只在代号与机制。
>
> 受影响的偏移是「0 与每个 refill 边界」，取决于 `nextSize` 的翻倍时机、是否有活动段
> （`s.b` 会让内容下移）、以及 reader 每次返回多少字节 —— **不是一条能写下来的语义规则**。
> 所以按 D-M1-7「照抄语义、不照抄实现」判为**实现产物**，不照抄；偏离用
> **谓词 + 形状**双重断言登记在 `test_char_golden.rs` 里（§8.2.1）。

**关键不变量**：

| # | 不变量 |
|---|---|
| I1 | `ch` 是**尚未归属**的字符；`r - chw` 是它的字节下标 |
| I2 | `segment()` 返回 `buf[b..r-chw]`，**天然排除 `ch`** |
| I3 | `col` 在每次 `nextch()` 时 `+= chw`；`ch == '\n'` 时 `line += 1, col = 0` |
| I4 | **`col` 的单位是字节** ⇒ 一个 UTF-8 字符横跨多个列号（**F9 已坐实**） |
| I5 | `rewind()` **只服务标点层**（两个用途：`..` 的回退、`.`+数字的交接），且活动的段内**不得含换行**（官方 `source.go:99` 的前置条件）。⚠️ 与官方一致地：`rewind` 内部那次重读会按**前一个字符**判换行 —— 见 **F30** |

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

### 5.3 局部算法

| 算法 | 核心 | 易错点 |
|---|---|---|
| **ident** | ASCII 快路径 → Unicode 一般路径 → **切完再查关键字** | 谓词必须用 `unicode_tables`，**不能用 Rust std**（D-M1-6） |
| **number** | 前缀定 base → `digits()` 返回 **digsep 位集** → 指数 → 后缀 `i` → **事后** `invalidSep()` | 十六进制浮点必须带 `p` 指数；`_` 的非法位置是**事后**定位；⚠️ **指数分支的 `digsep` 是 `=` 不是 `|=`**（F32）；**前缀/前导零都算一个数码**（F33）；非法数字的偏移**相对词元起点**（F34）；两条入口（数字起头 / `.`+数字交接）都以"游标在字面量第一个字符"为前提 |
| **escape** | `(n, base, max)` 表驱动：简单转义 / `\ooo`(3,8,255) / `\x`(2,16,255) / `\u`(4,16,MaxRune) / `\U`(8,16,MaxRune) | 代理区、`\ooo > 255`、未知转义各有独立诊断；⚠️ **简单转义白名单含 `quote` 本身**（F36）；**数值转义是定长**（`'\0'` 报错，F37）；三种错误的报位不同：`\` 后那个字符 / 闭引号 / 循环之后（F38） |
| **string** | 解释型：`"` 收尾 / `\` 走 [`escape`] / 换行与 EOF 收尾；原始型：**不转义、可跨行、一个字节都不改** | ⚠️ **`stdString` 的换行/EOF 分支没有 `if ok` 守卫**（与 `rune` 不同 —— 会**多**报一条，F40）；原始串的 `\r` **留在文本里**（F39 —— 「丢弃」是值层面的事） |
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
| `Directives` | 仅匹配指令前缀的注释（见 F44） | `messages` |

**「位置约束」属于 M2，scanner 层没有**：「`//line` 必须位于行首」与「多行注释里的
`/*line*/` 无效」都是 parser `updateBase()` 的行为（F11）；**scanner 的 `directives`
模式只做前缀匹配、不看列号** —— 行中间的 `//line a.go:1` 照样上报（p08/p22/p23，F43）。
M1 照抄 scanner 这一层，「行首」检查留给 M2。

**两个实测保真点**：
- 上报文本是**原始字节切片**（`\r` 与非法 UTF-8 都留着 ⇒ `Message.text: Vec<u8>`，F42）；
- 前缀匹配不对称：行注释认 `go:` **与** `line `、块注释**只认** `line `（F44）。

上报原因决定 `MessageKind`（`Comments` 通道 ⇒ `Comment`；匹配前缀 ⇒ `Directive`）——
不引入「文本分类」这类官方没有的复合判定。注释与诊断**两个独立收集器**（D-M1-8）。

**M1 只交出文本，不做任何解析**（D-M1-1）。

---

## 6. 字符分类（`unicode_tables.rs`）

**问题**（F17/F18 实测）：LEX-02 要的是广义类别（字母 = Lu/Ll/Lt/Lm/Lo；数字 = Nd），
而 Rust std 只能给 Alphabetic 属性与 `N*`，**两者都比 Go 宽**：

| 谓词 | Go | Rust std | 差 |
|---|---|---|---|
| 字母 | 145,672 | 147,421 | **+1,749**（全 Mn，如 U+0345） |
| 数字 | 770 | 1,924 | **+1,154**（全 No，如 U+00B2 `²`） |

**后果**（**F29 实测更正**，见下）：**不在词元边界上，边界通常相同** —— 官方
`atIdentChar` 对「不是字母、也不是 Nd」的非 ASCII 字符是**报错但仍然吸收**
（`case s.ch >= utf8.RuneSelf: s.errorf(…);` 之后 `return true`），所以 `x²` 在 Go 里是
**一个** `name` **外加一条** `LEX-BAD-IDENT-CHAR`；用 std 的近似谓词时 `²` 被当成数字
⇒ **不报错** ⇒ **少一条诊断**。起始位置（`²abc`）还会给出**不同的代号**。
**少一条诊断比「词元切错」更难发现** —— 静默错误，没有运行期提示。

| 输入 | Go 的词元 | Go 的诊断 | std 近似谓词 |
|---|---|---|---|
| `x²`（U+00B2，No） | `name "x\xc2\xb2"` | `LEX-BAD-IDENT-CHAR` @1:2 | `is_numeric`=true ⇒ **静默通过** |
| `²abc` | `name "\xc2\xb2abc"` | `LEX-BAD-IDENT-CHAR` @1:1 | 报成 `LEX-ID-LEADING-DIGIT`（**代号不同**） |
| `x\u{0345}`（Mn） | `name "x\xcc\x85"` | `LEX-BAD-IDENT-CHAR` @1:2 | `is_alphabetic`=true ⇒ **静默通过** |
| `xµ`（U+00B5，Ll） | `name "x\xc2\xb5"` | **无** | 一致（对照组） |
| `x１`（U+FF11，Nd） | `name "x\xef\xbc\x91"` | **无** | 一致（对照组） |

**解法（D-M1-6）**：由 oracle 生成两张区间表。

| 表 | 区间数 | 生成方式 |
|---|---|---|
| `LETTER_RANGES` | 684 | 遍历 0x0..0x10FFFF，取 `unicode.IsLetter(r)`（跳过代理区）并为连续段 |
| `DIGIT_RANGES` | 72 | 同上，`unicode.IsDigit(r)` |

- 查找用二分（区间有序、互不重叠 —— 这是查找正确性的**前提**，不是洁癖）
- 文件头**必须**注明：来源（go1.27.1 的 `unicode` 包）、Unicode 版本 **17.0.0**（F19）、
  生成命令、**勿手改**

**尺寸的口径要说清**（两种数都有人用，混着说会对不上）：

| 口径 | 值 |
|---|---|
| 区间**数据**（756 × 8 字节） | ≈ **6.0 KB** |
| 生成的 `.rs` **源文件** | **849 行 / 21 KB**（一条区间一行 —— 好处是 git diff 能定位到具体区间） |

### 6.1 `_`（U+005F）走访问函数，**不进表**

Go 规范的 `letter = unicode_letter | "_"` 把 `_` 算作字母（官方 `isLetter()` 也是
`ASCII 字母 || '_'`），但 **Unicode 没把它归入 `L*`**。

所以：

```rust
pub fn is_letter(c: char) -> bool { c == '_' || lookup(LETTER_RANGES, c) }
```

**这样表就严格等于 `unicode.IsLetter` 的区间 —— 一条不多一条不少。**
若把 `_` 并进表里，表的条数（684）就与 oracle 对不上了，
"重跑生成器应逐字节相同"这条验收也就失去了意义（表里混进了人的判断）。

### 6.2 访问函数也是生成的

`unicode_tables.rs` 是**整文件生成物**（表 + `is_letter`/`is_digit`/`lookup`）。
要改就得改 `gen_unicode/main.go` —— 单一事实来源，且生成物对 `rustfmt` **稳定**
（实测：生成后 `cargo fmt --check` 通过；再生成一次哈希不变）。

### 6.3 验收判据（三层，逐层抓不同的东西）

| # | 判据 | 抓什么 |
|---|---|---|
| 1 | **重跑生成器 → 逐字节相同** | 手工改过生成物（实测锚点：sha256 `ecf563fd…52c25`（因 F29 订正改过生成器注释，此锚点随之更新）） |
| 2 | **11 条 Rust 断言**（`tests/test_unicode_tables.rs`） | 表内容错：覆盖、拒真、有序性、区间数、码点数、代理区、`_` |
| 3 | **7 道变异全部变红**（人工注入） | 断言本身是不是恒真（见下） |

第 2 层的计数类断言是刻意的**两道**：区间数（684/72）与**码点数**（145,672/770）。
只钉区间数抓不到"边界写错"（把 `hi` 写成 `lo` 时区间数不变）。

**第 3 层的实测记录**（7/7 与预期一致）：

| 变异 | 变红的断言 |
|---|---|
| 删掉 DIGIT 的 `(0x06F0,0x06F9)` | 区间数 / 码点数 / 数字覆盖 |
| 字母 `0x0041-0x005A` 缩成 `0x0041-0x0041` | 码点数 / 字母覆盖 |
| 往 LETTER 塞 `(0x005F,0x005F)` | 区间数 / 码点数 / **`_` 不进表** |
| 交换前两个区间（破坏升序） | 有序且互不重叠 |
| `lookup` 比较器方向写反 | 字母覆盖 / 数字覆盖 |
| 往 DIGIT 塞代理区区间 | 代理区 / 区间数 / 有序性 / 码点数 |
| 删掉 LETTER 最后一个区间（模拟截断） | 区间数 / 码点数 |

> **⚠️ 反向验证脚本本身也要有证据链**：本任务第一次跑反验时出现过
> 「**哈希是新版、失败清单却是上一版的**」，导致连续两次得出错误结论。
> 修法是让每次变异同时输出三样东西并逐项校验：
> ① 容器内的**指纹**（变异特有的那一行）②宿主哈希 ③容器哈希（且必须不等于基线哈希）。
> 另外**基线必须来自"重新生成"**，不能取磁盘上当时的文件 ——
> 脚本中途崩溃过一次，导致"基线"自己就带着上一次的变异，后续结论全部作废。
> 这与本仓「断言必须反向验证」是同一条纪律，只是被用在了验证装置自己身上。

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

**23 个代号**，按 `as_str()` 字典序（== `diag.rs` 的声明顺序，那条不变量有断言钉住）。

**导出方式：枚举，不是回忆。** 把 `scanner.go` / `source.go` 里**全部**
`errorf` / `errorAtf` / `error` 调用点列出来（**26 处**，scanner 23 / source 3），
去重后得 **25 条消息模板**，逐条给代号。

| DiagCode | 触发 | 官方对应消息（格式化模板） |
|---|---|---|
| `LEX-BAD-CHAR` | 不认识的字符 | `invalid character %#U` |
| `LEX-BAD-DIGIT` | 进制字面量里出现不属于该进制的数位 | `invalid digit %q in %s literal` |
| `LEX-BAD-EXPONENT` | 指数缺数字 / 进制与尾数不匹配 | `exponent has no digits`；`%q exponent requires … mantissa`；`hexadecimal mantissa requires a 'p' exponent` |
| `LEX-BAD-IDENT-CHAR` | 标识符里混入非法字符 | `invalid character %#U in identifier` |
| `LEX-BAD-RADIX-POINT` | 进制字面量里出现小数点（只有十进制/十六进制允许） | `invalid radix point in %s literal` |
| `LEX-BAD-SEP` | `_` 没有分隔连续数码 | `'_' must separate successive digits` |
| `LEX-BOM-MIDFILE` | 文件中部出现 BOM | `invalid BOM in the middle of the file` |
| `LEX-COMMENT-UNTERMINATED` | 块注释未终止 | `comment not terminated` |
| `LEX-ESCAPE-BAD-DIGIT` | 转义序列里的数位非法 | `invalid character %q in %s escape` |
| `LEX-ESCAPE-OCTAL-OVER` | 八进制转义 > 255 | `octal escape value %d > 255` |
| `LEX-ESCAPE-RANGE` | 转义值超范围 / 落在代理区 | `escape is invalid Unicode code point %#U` |
| `LEX-ESCAPE-UNKNOWN` | 未知转义 | `unknown escape` |
| `LEX-EXCEEDS-FILE-LIMIT` | 超单文件上限 | **官方无对应**（本仓实现限制，D-M1-4） |
| `LEX-ID-LEADING-DIGIT` | 标识符以数字开头 | `identifier cannot begin with digit %#U` |
| `LEX-INVALID-NUL` | NUL 字符 | `invalid NUL character` |
| `LEX-INVALID-UTF8` | 非法 UTF-8 序列 | `invalid UTF-8 encoding` |
| `LEX-LIT-NO-DIGITS` | `0x`/`0b`/`0o` 之后没有数字 | `%s literal has no digits` |
| `LEX-RUNE-EMPTY` | rune 字面量为空 | `empty rune literal or unescaped '` |
| `LEX-RUNE-MULTI` | rune 字面量多字符 | `more than one character in rune literal` |
| `LEX-RUNE-NEWLINE` | rune 字面量含换行 | `newline in rune literal` |
| `LEX-RUNE-UNTERMINATED` | rune 字面量未终止 | `rune literal not terminated` |
| `LEX-STRING-NEWLINE` | 解释型字符串里出现裸换行 | `newline in string` |
| `LEX-STRING-UNTERMINATED` | 字符串未终止 | `string not terminated` |

> **`LEX-EXCEEDS-FILE-LIMIT` 那一行是刻意的**：它必须明确标注「官方无对应」——
> 这是**我们自己的限制**，不得伪装成对齐官方。

**这张表由两道机制守着，不靠人记得同步：**

| 机制 | 位置 | 抓什么 |
|---|---|---|
| Go 侧 `--selfcheck` | `tools/token-oracle/diagmap.go` | 映射指向不存在的代号 / **永远不会被产出的死代号** / 条目数（冻结 25）/ 码表无重复 |
| **跨语言对撞断言** | `rgoc-lex/tests/test_diag.rs` | Rust `DiagCode::ALL` 与 Go `allCodes` **逐字不一致** |
| 运行时 | adapter | 遇到映射表之外的消息**立刻 `exit 3`**，不许猜、不许跳过 |

> **为什么要有第二道**：第一版这张表只有 **19** 个代号，是**凭印象**挑的，漏了 4 个
> （`LEX-BAD-DIGIT` / `LEX-BAD-RADIX-POINT` / `LEX-STRING-NEWLINE` / `LEX-ESCAPE-BAD-DIGIT`）。
> 漏掉时**不会报错** —— 那 4 条消息出现时行为未定义。这与 `AGENTS.md` 记的
> 「**写死的键列表会变成盲区**」（T38/T39/T47 三次同形）是同一个形状。
> 只靠"两侧各维护一份清单"，漂移是必然的 ⇒ 用断言把它们钉在一起。

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

> **落地记录（T-L17）**：诊断与 `Eof` 都报在**文件起点 1:1**（超限判定只看大小、
> 不看内容 —— 起点是唯一有意义的坐标）。错误面**两分**：**超限 ⇒ `Ok`**（词法层
> 的拒绝）；**io 失败（不存在 / 不可读）⇒ `Err`**（不可映射成超限诊断，否则
> 「读不到」与「太大」不可区分）。golden 是**手写**的
> （`rgoc-lex/tests/golden/t63_exceeds.golden`，`# source: hand-written`，
> 全仓唯一 `H` 类）。「检查在读入之前」的判别测试：用**超限 + 不可读**的文件把
> 「先 metadata 再 read」逼出来 —— 先 read 会得 io 错误（且换成 10 GB 就是 OOM）。

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

对齐：位置占 7 列、词元名占 9 列（左对齐），即 `"  %-7s%-9slit=%s"`。

**词元行可带两组附加字段，都**只在该类别下有效**时才出现：**

| 附加字段 | 何时出现 | 内容 |
|---|---|---|
| `kind=` `bad=` | 仅 `_Literal` | 官方数值（0=Int…4=String）+ `true`/`false` |
| `op=` `prec=` | 仅 `_Operator` / `_AssignOp` / `_IncOp` / `_Star` | 官方 `Operator.String()` + 优先级数值 |

> **`=` / `:=` / `<-` 不追加 `op`/`prec`** —— 实测 **F21**：官方对这三者**不设 `op`**
> （有效域不含 `_Assign`/`_Define`/`_Arrow`）。这正是纪律 1 在运算符上的体现。

**`--mode` 开启时多一节**（LEX-03 用）：

```text
== messages ==
  <pos>  <escaped-text>
```

**`lit` / 注释文本的转义规则**（两侧必须一字不差地实现）：

| 字节 | 输出 |
|---|---|
| `\` (0x5C) | `\\` |
| `"` (0x22) | `\"` |
| 换行 / 回车 / 制表 | `\n` `\r` `\t` |
| 可打印 ASCII（0x20–0x7E） | 原样 |
| **其余一切**（0x00–0x1F、0x7F、所有 ≥0x80 的字节） | `\xNN`（两位小写十六进制，**逐字节**） |

> **为什么不用 Go 的 `%q` / Rust 的 `{:?}`**：两者对控制字符的写法不同（`\x00` vs `\u{0}`），
> 一旦不一致，golden 就成了"只有 Go 能通过"的东西。而且 Rust 的 `String`
> **根本装不了非法 UTF-8**（F22）—— 见 D-M1-12。

**四条规范化纪律**（全部为实测踩出来的）：

| # | 纪律 | 踩坑经历 |
|---|---|---|
| 1 | **只录有效域内的字段** —— `lit` 只对 `_Name`/`_Literal`/`_Semi` 记录 | 第一版无条件录 `lit` ⇒ `(` 的 `lit` 显示为上一个词元的 `main`，**把实现细节写进契约** |
| 2 | **诊断与词元分两条流、各自有序**（C1 §4 的 `line→col→code`） | 第一版按事件顺序输出 ⇒ 诊断 `E 4:8` 出现在词元 `T 4:7` **之前**，golden 编码了内部时序 |
| 3 | **只存 `Pos` + `DiagCode`，不存消息文本** | D-M1-9 |
| 4 | **确定性** —— 同一输入多次运行产出的字节完全相同 | 已实测：3 次 `sha256` 一致 |

> **⚠️ 纪律 5（不是格式，是采集方式）：冻结 golden 时必须 `RGOC_TTY=0`。**
> `scripts/in-container.sh` 在 stdin 是 tty 时加 `-it`，伪终端 ONLCR 把 `\n` 翻成 `\r\n` ——
> **终端里肉眼看不出**，但 golden 会每行带 CR。实测 F25：默认 20 行含 20 个 CR，设了为 0 个。

**来源必须标注**（golden 两种 + 对撞输出一种）：

| 来源 | 覆盖 | 标记 |
|---|---|---|
| oracle 生成 | 绝大多数 | `# source: oracle` |
| **手写** | **只有实现限制类**（`LEX-EXCEEDS-FILE-LIMIT`，官方无此行为） | `# source: hand-written` |
| **driver 重算**（腿乙对撞时现算，**不落盘**） | `lex` 子命令输出（`T-L18`） | `# source: rgoc`（对撞时归一这一行；`# golden:` 行在传同一路径字符串时逐字节相同） |

> 不标注的后果：将来有人会以为"超限报错"也是 oracle 说的。

### 8.2.1 字符级 golden（腿甲的第二个判据，`T-L07` 立）

词元级 golden 在**词元边界**上取样；而位置是**每个字节**推出来的，
所以 `source.nextch` 这一层需要**更密**的判据。

| 项 | 值 |
|---|---|
| 语料 | `tools/token-oracle/fixtures/char_corpus.txt` —— **1177 个字节串**（全部单字节 256 + 30 个"结构相关字节"的两两组合 900 + 27 个有名字的多字节用例；去重保序） |
| golden | `tools/token-oracle/fixtures/char_golden.txt`（3279 行 / 62 KB），由 `go run . --chars-golden` 生成 |
| 比对 | `rgoc-lex/tests/test_char_golden.rs`，**逐事件** |
| 生成器 | `tools/token-oracle/charcorpus.go`（语料）/ `chargolden.go`（渲染） |

**格式（冻结）—— 顺序即发生顺序，与词元级相反、不需要排序：**

```text
## <hex>                             ← 用例（输入字节串的 hex；空串为 -）
D <line>:<col>      <CODE>          ← 诊断
C <line>:<col>      U+<HEX> w=<n>   ← 读到一个字符；EOF 不进事件流
```

**为什么这里可以不排序**：官方 `nextch` 的 `redo:` 标签在位置推进**之前**，
所以"违规字节先报诊断、再让出下一个字符"是**语义**（每个被跳过的字节各占一列），
不是内部时序 —— 与词元级恰好相反（那里"诊断何时触发"是实现细节，两条流必须分开排序）。

**为什么 `w` 必须进 golden**：它是 byte-column 语义的直接证据。
注意**合成**的 `U+FFFD` 从不进入事件流（官方 `goto redo` 会重读），
事件流里出现的 `U+FFFD` 一定来自源码（`w=3`）—— 这是设计期我写错、被实测纠正的一处。

**一处已登记的偏离**（§12 R11）：输入**首字节是 `0x00`**。判据写成**谓词**而不是
写死的 hex 列表（本仓三次踩过「写死的键列表变成盲区」，T38/T39/T47），
且**同时断言偏离的形状**：

- 谓词：`bs.first() == Some(&0x00)`，并断言它覆盖的用例数**远小于**一致数（防判据写松成垃圾桶）
- 形状：我们比 golden **多一条** `D 1:1 LEX-INVALID-NUL`、**少一条** `C 1:1 U+0000 w=1`，
  **其后必须逐字节相同**

### 8.3 六个切面 → 测试组

| 组 | 切面 | 典型用例 | 判据 |
|---|---|---|---|
| `T-L-1x` | ① 解码与位置 | **主力：1177 例字节级密集对撞**（§8.2.1）；代表用例：中文/emoji 列号、BOM 三处、NUL、CRLF、非法 UTF-8、私有区字符 | oracle 对照（**逐事件**） |
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
├── README.md                 来源、版本、校验值、格式、再生成步骤
├── go.mod                    module tokenoracle（仅标准库）
├── main.go                ★  命令行入口 + 按 §8.2 格式化
├── diagmap.go             ★  官方消息 → DiagCode 映射表 + `--selfcheck`
├── fixtures/
│   ├── hello.txt             正向样本
│   └── bad_utf8.txt          负向样本（含裸 0xFF）
└── gosyntax/
    ├── LICENSE               BSD-3-Clause 正文（随语料根，27 行）
    ├── scanner.go            881 行 ┐
    ├── source.go             218 行 │ vendor 的官方五文件
    ├── tokens.go             159 行 │ **原样复制、零改动**
    ├── token_string.go        70 行 │ （共 1,374 行）
    ├── operator_string.go     46 行 ┘
    └── export.go            ★  同包 shim：唯一能做转发的地方
```

> **★ 三个文件是手写的**，其余是官方原文。设计初稿以为"`adapter.go` 是唯一手写部分"，
> 实测不成立 —— 官方 scanner 的**全部**标识符都是未导出的（`scanner`/`next`/`tok`/`lit`…），
> 外部包在语言层面访问不到，而"官方文件零改动"是硬纪律 ⇒ **必须**在同一个 package 里
> 加一层纯转发 shim（`export.go` 不含任何词法逻辑）。映射表单独成文件是为了安放 `--selfcheck`。

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
| `rgoc-lex/src/unicode_tables.rs` | **756 区间**（数据 ≈6.0 KB；源文件 849 行 / 21 KB） | 由 `gen_unicode` 生成，Unicode 17.0.0；**整文件生成物**（含访问函数） |

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
| R5 | `char::is_alphabetic` 之类的**近似谓词**被顺手用进字符分类 | M1 收尾时全仓 grep **精确模式** `\.is_(alphabetic|numeric|alphanumeric)\(`，**逐个命中都要给出理由**。⚠️ 判据不是「零命中」—— 我第一版就是这么写的，**是错的**：① 精确模式仍会命中 `test_unicode_tables.rs` 的**反证**断言（`assert!(c.is_alphabetic())`，它们让"拒绝 std 的那批"这条断言不致恒真，**必须保留**）；② 会命中 `lexer.rs` 里**描述这个模式自身**的文档文字；③ 会命中 `rgoc-harness/src/corpus.rs`（与词法无关的路径处理）。⇒ 把它写成"确认零使用"会诱使人**删掉证据**去达标 —— 那是本仓「不许靠放宽判据达标」的变体。**正确判据：命中清单冻结 + 每条写明归属**；出现**新的**命中才是红灯 |
| R6 | 位置漂移（"差 1"类错误） | `T-L-1x` 必须含**中文与 emoji** 样本 —— 否则字节列写错也看不出来 |
| R7 | 超预算 | 按 `03` §6.1 的预算（smoke ≤5 分钟）；**不得**通过砍 LEX 项或放宽判据来达标 |
| R8 | **码表三处漂移** —— 设计 §7.2 / Go `allCodes` / Rust `DiagCode::ALL`。目前只有 Rust↔Go 有断言（`tests/test_diag.rs`），**设计文档与代码之间没有自动检查** | 由 `T-L21` 的文档同步检查补上；在此之前，改码表必须**同时**改三处并跑 `cargo test -p rgoc-lex --test test_diag` |
| R9 | **词元载荷是字节，容易被顺手 `String` 化** —— M2 起每个消费方都会想要 `&str` | 取 `&str` 必须显式 `str::from_utf8`（**不得** `from_utf8_lossy` —— 那会静默丢字节）；关键字查询天然安全（全 ASCII） |
| R10 | **容器里可能跑的是上一版二进制** —— 宿主写文件后容器仍看到缓存的 mtime ⇒ cargo 判定「无需重建」。⚠️ 看不出来：内容对、哈希对、指纹也对 | 在容器里 `touch` 源文件后再跑（计划 **G13**）；反验脚本已按此写 |
| R11 | **首字节 NUL 与 oracle 不同**（F26/F27）—— 官方漏报（懒填充的产物），我们报 `LEX-INVALID-NUL`。**两侧都报错**，差别只在代号（我们）/ `LEX-BAD-CHAR`（官方） | 判为**实现产物**，按 D-M1-7 不照抄；偏离在 `test_char_golden.rs` 里用谓词 + 形状双重断言登记。**再评估触发条件**：若 `T-L19` 的全文件 golden 在某个语料上撞到它（或 M4 的 errorcheck 对齐要求逐代号一致）⇒ 重新裁决是否照抄 |
| R12 | **门禁的退出码不可信**（F28）—— ① 容器内 `set -e` + 显式 `exit N` ⇒ 一律 1；② `cmd && echo OK` 不中止 ⇒ 早期失败被后续成功掩盖 |
| R13 | **反验/取证脚本的路径有两套**：宿主是仓库根相对路径（`rgoc/crates/...`），容器内是 workspace 根（`cd /work/rgoc` 后 `crates/...` 或绝对 `/work/rgoc/...`）。拼错时 `touch` / `grep` / `sha256sum` **同时落空** ⇒ 证据链整段失效，而 `cargo test` 的失败清单**仍然看起来对**（测试真的会红） | T-L10 实测：11 道变异全被证据链误判 ❌"指纹未命中" —— **装置自己把问题标出来了**（这正是证据链存在的理由）；修正路径后 11/11 ✅。见计划 **G11 的补充条款** | 门禁命令按计划 **G14** 的规范形态写（`fail` 累积 + 末条 `[ $fail -eq 0 ]`，**不写 `exit`**）。⚠️ M0 与 M1 前几个任务的"门禁全过"是靠**读打印输出**判的，没有受影响，但退出码这个信号一直不可用 |
| R14 | **反验的判红判据 `rc != 0` 太弱** —— **编译失败**也是 `rc=101`，但它没验到任何断言；更糟的是"变异文本把语法搞坏"会**伪装成红**（T-L11 实测：`// MUTV2` 插在 `if … {` 的 `{` 前 ⇒ 行注释吃掉花括号 ⇒ 编译器报 `unexpected closing delimiter`） | 判红加一条：**必须有 ≥1 条 `test … FAILED`**（证明测试二进制真的跑起来了）；变异指纹改用**块注释** `/* MUTVn */`。见计划 **G15** |
| R15 | **反验脚本的「多义锚点」** —— 同一形状的代码在文件里出现多次（std 与 raw 的尾部、两个 `None` 分支…）时，只改「第一个」会**打错目标**；而 `nth` 语义若写成「`nth=1` 兼表**必须唯一**」，注入器会**误报「注入失败」**（T-L12 实测） | 锚点不唯一时**显式给出现序号**；`nth=None`（必须唯一）与 `nth=k`（取第 k 次）语义分开。见计划 **G15 ④** |
| R16 | **R10 的二次实测：反验脚本的「mtime 错位」** —— 处置里「在容器里 `touch`（**或让容器自己写**）」的括注**不够**：target 在命名卷、源码走 bind mount（Docker Desktop / VirtioFS）时容器侧 mtime 只有**秒级**精度（实测 `stat` 见 `.000000000`）。连续两道变异若落进同一秒，cargo 判定「缓存新鲜」⇒ **跑的是上一道变异的旧二进制**。T-L13 反验第一轮 V3–V11 **全部**误报成同一失败者，而每道都"判红"了 —— 看起来 11/11 ✅。**这种错位看不出来**：内容对、sha256 对、指纹对，只有失败清单是上一道的 | 脚本每次写入后**显式设定一个递增的唯一 mtime**（`os.utime` / `touch -d`；脚本在容器内运行、与 cargo 读同一视图），并断言每道变异的 cargo 输出**含 `Compiling`**（缺席即作废重跑）。见计划 **G13 订正**与 **G15 ⑤** |
| R36 | **反验观测被 cargo 的 `fail-fast` 截断**（R10/R16 的同族第三例）—— `cargo test` 默认一个 test target 失败后**不再运行后续 target**；变异打红了先序 target（`test_char_golden` 等）后，**期望失败者所在的 target 根本没跑** ⇒ 失败清单里没有期望名 ⇒ 判据统计**悄悄少计**（T-L16 第一轮 11/12「未达标」，不是变异失效，是**观测被截断**）。同一轮的另一处错：**期望名锚错了路径**（期望的诊断走 `report`（当前字符位置），实际走 `report_at`（事后校验段的偏移语义）） | 反验一律 **`cargo test --no-fail-fast`**；期望失败者必须**指名到具体测试**，且**对着调用点核**（`LIT-NO-DIGITS`/`BAD-EXPONENT` 走 `report`；`BAD-SEP`/`BAD-DIGIT` 走 `report_at`）。见计划 **T-L16 Step 3** |

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
| 2026-10-11 | **新增 D-M1-11（诊断代号表导出方式）**：§7.2 由 19 条扩到 **23** 条。原表凭印象手写，漏 4 条且无报错。同步改了 §7.2、§8.3 的引用 | `T-L04` 执行期发现（见 §12 的 R8） |
| 2026-10-11 | **新增 D-M1-12（词元载荷载体）**：`Ident(String)` / `Literal.text: String` → **`Vec<u8>`**。实测标识符原文可含非法 UTF-8 | `T-L04` 执行期发现（见 §12 的 R9） |
| 2026-10-11 | §8.2 补全：`op=`/`prec=` 的生效条件、`== messages ==` 一节、**字节级转义规则**、采集必须 `RGOC_TTY=0`（F25） | `T-L04` 冻结格式时定下 |
| 2026-10-11 | §9.1 组成树修正：手写部分由 1 个文件改为 **3 个**（`main.go` / `diagmap.go` / `gosyntax/export.go`），并随附 LICENSE；实测坑由 1 条增至 5 条 | `T-L04` 实测 |
| 2026-10-11 | **新增 §8.2.1 字符级 golden**（`T-L07`）：语料 1177 例 / golden 3279 行 / 逐事件对撞；§8.3 切面 ① 的判据由"手写若干条"升为"字节级密集对撞 + 少量可读代表" | 切面 ① 的判据原文就是「位置必须与 oracle **逐字节**相等」——手写取样支撑不了这句话 |
| 2026-10-11 | §5.1 补明"去掉 `fill`/`sentinel` 的可观测后果"；§2.3 追加 **F26–F28**；§12 追加 **R11**（首字节 NUL 偏离）/ **R12**（门禁退出码） | `T-L07` 对撞与门禁自检实测 |
| 2026-10-11 | §6 补全：`_` 不进表的口径、尺寸的两种口径、**验收判据三层**与 7 道变异的实测记录；§10.3 产物尺寸更新 | `T-L05` 执行期 |
| 2026-10-11 | **订正一条写错的断言**：原写「`x²` 在 Go 里是 `x` + 非法字符」，实测**证伪** —— 官方是「**吸收 + 报 `LEX-BAD-IDENT-CHAR`**」，差异在**诊断层面**而非词元边界。同步改 §2.1 的 D-M1-6 否决理由、§6 后果段、生成器注释与模板（⇒ 生成物 sha256 锚点更新为 `ecf563fd…52c25`）、`test_unicode_tables.rs` 两处注释；新增 **F29** | `T-L08` 开工前实测（F29） |
| 2026-10-11 | §5.1 的 I5 / 简化表：`rewind` 的用途由「只服务 `..`」改为「只服务**标点层**（`..` + `.`+数字交接）」，并补明它内部的换行判定；§2.3 新增 **F30**（`..`+`\n` 的行号偏移）与 **F31**（`1..2` 里没有 `..`） | `T-L09` 实测（F30 是**只有 oracle 才看得出来**的行为） |
| 2026-10-11 | **R5 的判据改掉了**：原写「全仓 grep `is_alphabetic` 确认零使用」—— 这条**达不到**：精确模式仍会命中 `test_unicode_tables.rs` 的**反证**断言（必须保留）、`lexer.rs` 里描述该模式自身的文档、以及 `rgoc-harness` 的无关代码。写成"零使用"会诱使人**删掉证据**去达标。改为「**命中清单冻结 + 每条写明归属**，出现新命中才是红灯」 | `T-L08` 落地时实测（R5） |
| 2026-10-11 | §12 新增 **R10**（容器可能跑旧二进制）；`advance`/`rewind` 的返回类型由 `Option<Diag>` 改 **`Vec<Diag>`**（P1 的「目前不会」被实测推翻） | `T-L06` 执行期（R9） |
| 2026-10-12 | §2.3 追加 **F32–F35**（数字字面量四条实测：指数 `digsep` 用 `=` / 前缀与前导零算数码 / 非法数字偏移相对起点 / `0x1g` 零诊断）；§5.3 的 `number` 行补上三处照抄点与两条入口约定；§12 追加 **R13**（反验脚本的宿主/容器路径有两套） | `T-L10` 执行期（13 条测试全部先经 oracle 取期望值；11 道变异反向验证） |
| 2026-10-12 | §2.3 追加 **F36–F38**（转义与 rune 三条实测：白名单与引号绑定 / 八进制定长 3 位 / 两种"字符数"错误的报位不同）；§5.3 的 `escape` 行补三处；§12 追加 **R14**（反验判红判据太弱） | `T-L11` 执行期（12 条测试先经 oracle 取证；12 道变异反向验证） |
| 2026-10-12 | §2.3 追加 **F39–F40**（原始串 `\r` 留在文本里 / `stdString` 无 `if ok` 守卫）；§5.3 去掉标题里的计数并新增 **string** 行；§12 追加 **R15**（反验脚本的多义锚点） | `T-L12` 执行期（12 条测试先经 oracle 取证；9 道变异反向验证） |
| 2026-10-12 | §2.3 追加 **F41–F45**（注释五条实测：三处位置都取注释起点 / 文本保留原始字节 / `directives` 无行首检查 / 前缀匹配不对称 / 未终止跨行块注释仍产 `Semi`）；§5.5 订正「位置约束」的**跨层混淆**（那是 parser `updateBase` 的、scanner 只做前缀匹配）；§12 追加 **R16**（反验 mtime 错位的二次实测：秒级精度 ⇒ 旧二进制错位，及修法） | `T-L13` 执行期（44 个 oracle 探针先取证；12 条测试；11 道变异反向验证——第一轮因 R16 的错位作废重跑） |
| 2026-10-12 | §5.4 的规则表落地为 `semicolon.rs`：官方六处 `nlsemi` 设置点（含关键字 bitmask）逐条枚举；`Keyword`/`Delim`/`OpForm` 的**成员全覆盖**断言 + 基数断言（21 = 25−4）。`T-L-49` 判据保持 `S`（纯规则表，无新 oracle 事实） | `T-L14` 执行期（3 条测试一次全绿；10 道变异反向验证——含「false 成员同时列进 true 分支」的可编译变异技巧） |
| 2026-10-12 | §2.3 追加 **F46**（官方拼写的两处「不细分」+ `_Star` 特例）；§5.2 骨架落地为 `next()`（含 `skip_whitespace` 与 `default` 分支）；**渲染器提前落地**于 `rgoc-lex::render`（R30——它是对撞格式的唯一一处实现）；`next` 保留官方名 + `allow`（R31，与 R3 的差异见该处论证）；反验判红判据记录**第三形态**（R32：进程崩溃/SIGKILL） | `T-L15` 执行期（24 个词元级 golden 由 oracle 直出；15 条测试；13 道变异全红——第一轮 2 道未红后修正） |
| 2026-10-12 | §2.3 追加 **F47–F48**（同位置双诊断的两个实例：`0x\` 的**排序输出与发生时序相反** / `"\`+换行 的同位序不变）；错误恢复 N1–N11 的行为经探路对撞确认**已由 T-L08–L15 全覆盖**（14 探针逐字节一致）——本轮交付为**双向断言固化**（15 条）+ `push_diag` 单入口（R34）；R33 记「验收任务无自然 RED，测试有效性由反验兑现」的裁定 | `T-L16` 执行期（14 个 oracle 探针先取证；15 条测试；12 道变异反向验证——第一轮因 cargo fail-fast 截断观测后修正） |
| 2026-10-12 | §7.3 补**落地记录**（诊断与 `Eof` 报在**文件起点 1:1**；错误面两分：超限 `Ok` / io 失败 `Err`；`t63_exceeds.golden` 手写登记）；R37 记「**用常量造文件**的自测盲区」—— 引用 `MAX_FILE_BYTES` 造文件的边界测试会**随常量走**（常量被改小时它恒绿），须另设「钉绝对值」断言 | `T-L17` 执行期（7 条测试；8 道变异反向验证——含「超限 + 不可读」文件逼出检查时机） |
| 2026-10-12 | §8.2 补**第三来源**（`# source: rgoc`：driver 重算、**不落盘**）与**头两行口径**（`# golden: <原样路径>` + `# source: rgoc`；对撞归一 `# source:` 行）；§5.2 的「`--mode` 开启时多一节」落地为 `render::golden_body_with_messages`（**此前 `== messages ==` 无处渲染**，R40 ②）；R39 记「注释宣称的守卫不存在」（`cli.rs` 的「用法可解析」断言从未落地 ⇒ 以解析样例组取代） | `T-L18` 执行期（driver 8 条 + `test_render` 5 条；11 道变异反向验证） |
