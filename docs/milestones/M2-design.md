# M2 设计文档：AST 与 parser

> **阶段 ID**：`M2`　|　**状态**：**已确认**（2026-10-15 收口）　|　**日期**：2026-10-14
> **文档索引**：[`../README.md`](../README.md)　|　**上游**：[`../03-roadmap.md`](../03-roadmap.md) §M2、[`../01-feature-set.md`](../01-feature-set.md)（`AST-01..49` / `PIPE-03/04/05/25`）、[`M1-design.md`](./M1-design.md) §13.3、[`M0-report.md`](./M0-report.md) §7
> **下游**：`M2-tests.md`（测试先行清单）→ `M2-plan.md`（任务级计划）→ `M2-manifest.json`（机器可读事实）
>
> **本文档的性质**：Superpowers 流程中「头脑风暴」阶段的产出 —— **只冻结设计决策与边界，不写任务粒度、不写实现代码**。任务级拆分是 `writing-plans` 的职责。
>
> **读者提示**：不懂 `oracle` / `golden` / `spike` 这些词，先读 [`M0-design.md`](./M0-design.md) §1.0。本文档假设读者已读过 [`M0-report.md`](./M0-report.md) §7「接手指南」与 [`M1-design.md`](./M1-design.md) §1。
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
| `docs/milestones/M2-design.md` | ✅ 本文件 | 设计决策与边界 |
| `docs/milestones/M2-tests.md` | ⬜ **待产出** | 必需测试 ID、比较器、验证命令、暂不支持项 |
| `docs/milestones/M2-plan.md` | ⬜ **待产出** | 2–5 分钟粒度实施任务 |
| 机器可读 manifest | ⬜ **待产出** | `M2-manifest.json`：`gate` + `test_ids` + `unsupported` + `budget` |

**本文档未确认前不进入下一步。** 本轮头脑风暴已确认 **七条决策**（`D-M2-1..7`，见 §2），并**显式登记一处与 roadmap 砍项的偏离**（§2.2 的 `D-M2-2`）。

---

## 1. 目标与非目标

### 1.0 推导：为什么 M2 是「线性 → 树」

**推导起点**：M1 结束时我们有了 `rgoc-lex` —— 它把字节流精确切成**带位置的词元流**（151 条 golden 全绿）。但词元流是**平铺**的：`for i := 0; i < 10; i++ { ... }` 被切成 14 个词元排成一排，它们之间**只有先后关系，没有归属关系**。

而 Go 源码的语义结构天然是**嵌套**的：`for` 语句包含一个块，块包含 `if` 语句，`if` 又包含块……

> **词元流是线性的，语言结构是树形的。**

这个矛盾是**语言结构本身**决定的，不是实现选择 —— 只要 Go 有嵌套语法，就绕不过这一步。于是 M2 被逼出来：

> **把线性序列重建为树形结构，同时保住位置、扛住错误、把 M1 推迟的指令/位置效果接住。**

**一句话**：M2 只产生**结构 + 位置**，不产生任何语义判断（类型、常量值、作用域、包语义都是 M3+ 的事）。roadmap §M2 门禁原文钉死了这条边界：**「parser 测试不等于 `run/compile` 全集已支持」**。

### 1.1 M2 的输入与输出

**输入**：M1 交付的三样东西（[`M1-design.md`](./M1-design.md) §13.3）。

| 输入 | 内容 | 来源 |
|---|---|---|
| **词元流** | 每个词元带：类别（`TokenKind`）+ 载荷（`Vec<u8>` 原文）+ `Pos` | `rgoc-lex::Lexer::next()` |
| **注释/指令文本**（`messages`） | `//line`、`//go:` 的**原始字节文本**（M1 只做前缀匹配，不解析内容） | `rgoc-lex` 的 `directives` 模式 |
| **`Pos` / `FileTable`** | C1 契约的位置体系（`Pos { file: FileId, line, col }`） | `rgoc-lex::pos` |

- 输入中**没有**：AST、类型、常量值、作用域、包/import 语义（全在 M3+）。
- 词元流是**流**不是数组：parser 每次问 lexer 要一个（pull 模型）。

**输出**：交 M3/M4。

| 输出 | 内容 | 消费者 |
|---|---|---|
| **AST（树）** | 节点 = 语法构造（`AST-01..49`）；边 = 包含关系；每节点带起点 `Pos` | M3 类型、M4 检查、M5 HIR |
| **诊断流** | 每条带 `Pos` + `DiagCode` + 消息（沿用 C1 排序规则） | M4、golden 对比 |
| **重映射后位置 + 指令记号** | `//line` 生效后的 `Pos`；`//go:` 指令解析成的 `Pragma`（挂在 AST 节点上，**效果不在此阶段生效**） | M3/M4、M7+ |

两个性质：

1. **AST 是"所有物树不是 arena"**：节点间用 `Box` 持有，与官方 Go 的指针树同形（§3.5）。
2. **诊断是旁路**：一个节点可以既正常产出、又附带诊断（表达式错误恢复产出 `BadExpr` 占位，同一处再附一条诊断）。

### 1.2 明确不做（M2 边界）

**每一项都有归属，不是"漏了"**：

| 不做 | 归属 | 依据 |
|---|---|---|
| 类型检查（`a+b` 是否同类型） | **M3** | AST 节点不带类型信息 |
| 常量求值（`1+2` 折叠成 `3`） | **M3** | AST 里仍是 `BinaryExpr(+, Lit(1), Lit(2))` |
| 作用域解析 / 遮蔽 | **M3/M4** | parser 不判"变量名在哪可见" |
| 包 / import **语义**（真去加载包） | **M3** | M2 只验 import 路径是**合法字符串语法**，不用 importer |
| `//go:` 指令的**效果**（`nosplit`/`noescape`/`linkname`） | **M7+** | `03` §M2：「实现效果列延期，不标完成」 |
| `//go:build` 的**求值**（选不选这个文件） | M3/M4 包模型 | M2 只解析约束表达式文本、取 `goVersion`，不求值 |
| AST → 源码的 **printer**（幂等打印 / gofmt 类） | 独立工具 | 非 M2 必需（`D-M2-7`） |
| 代码生成 | **M6/M7** | M2 产出的是树，不是机器码 |

> ⚠️ **`//go:` 指令"解析"与"生效"是两件事**。M2 **解析并挂载**（把 `//go:nosplit` 变成挂在 `FuncDecl` 上的 `Pragma`），但**不生效**。写成「M2 支持 nosplit」是错的。

### 1.3 M2 退出条件（逐条可判定）

| # | 条件 | 判据 |
|---|---|---|
| 1 | **`AST-01..49` 各至少一条直接测试**，全绿 | `M2-tests.md` 冻结 ID 100% 通过 |
| 2 | **语法 testdata 全绿**（正例无错 + 负例 `// ERROR` 双向匹配） | 见 §8 / 门禁 `A2` |
| 3 | **节点 `Pos` 规则全绿**（`@` 标记片段） | 见 §3.3 / 门禁 `A3` |
| 4 | **oracle 对撞全绿**（规范化 dump 逐行 diff 为空） | 见 §9 / 门禁 `A4` |
| 5 | **标准库冒烟**：`go_source_code/src` 全部 `.go` 解析**零意外错误** | 见 §8 / 门禁 `A5` |
| 6 | 四条 cargo 命令全过 | fmt / check / clippy `-D warnings` / test |
| 7 | **C1 契约延续**（parser 是第二个消费者）+ **新增 C6（AST dump 格式）** | 含变更记录 |
| 8 | **GAP-01 的 `AST-*` / `PIPE-03/04/05/25` 四维归属已定** | 续 `GAP-01.md` |

> **条件 5 是 M2 特有的**：官方对 parser 最强的自测是 `TestStdLib`（解析整个 `GOROOT/src`）。M2 照搬 —— 能解析 185 MB 真实 Go 代码且零意外错误，是"语法覆盖"最强的证据。

---

## 2. 已确认决策记录

### 2.1 决策一览

| # | 决策 | 结论 | 确认日期 |
|---|---|---|---|
| `D-M2-1` | AST 验收判据 | **oracle 对撞**（`tools/ast-oracle` + 中立 dump 逐行 diff） | 2026-10-14 |
| `D-M2-2` | 错误恢复深度 | **完整对齐官方**（⚠️ **较 roadmap 砍项上调**，见下） | 2026-10-14 |
| `D-M2-3` | 验收语料范围 | **syntax/testdata 31 + 标准库全量** | 2026-10-14 |
| `D-M2-4` | crate 划分 | **`rgoc-ast` + `rgoc-parse`** 两个新 crate | 2026-10-14 |
| `D-M2-5` | 规范化 dump 形式 | **中立缩进语法**（双方各写 dumper 输出同格式） | 2026-10-14 |
| `D-M2-6` | 版本门控（PIPE-04）深度 | **语法层门控**（只判"该语法在 lang 下是否合法"） | 2026-10-14 |
| `D-M2-7` | 交付产物范围 | driver 新增 **`rgoc ast`** 子命令；**不做 printer** | 2026-10-14 |

### 2.2 五字段记录

#### D-M2-1　AST 验收判据

| 字段 | 内容 |
|---|---|
| **判据** | `03` §M2 门禁要求「syntax testdata 和自有 AST/位置/错误恢复切片全绿」；仓库纪律要求"判定逻辑只有一处"（`AGENTS.md` §1）；M1 已建立 oracle golden 模式（`D-M1-2`/`D-M1-3`） |
| **候选** | ① **oracle 对撞**（vendor 官方 syntax + 双方输出同格式 dump 逐行 diff）② 手写结构化 golden ③ 冒烟 + 结构锚点 |
| **结论** | **①**。新增 `tools/ast-oracle`（vendor 官方 `syntax`），rgoc 与 oracle 各写一个 dumper 输出**同一套中立格式**，逐行 diff 必须为空 |
| **否决理由** | ② 覆盖度取决于人工挑用例，抓不住安静的结构性错误（M1 已证明 golden 必须 oracle 生成，见 `D-M1-3`）；③ 只能证明"不崩"，无法证明"树形一致" |
| **失效条件** | 若官方 `syntax` 包不再自包含（`go build` 编译不到）⇒ 必须换基准。当前 1.27.1 的 `syntax` 依赖 `go/build/constraint`（标准库），自包含 |

#### D-M2-2　错误恢复深度　⚠️ **本决策上调了 roadmap 砍项**

| 字段 | 内容 |
|---|---|
| **判据** | 用户在设计会话中明确要求"完整对齐官方"；官方 `advance()`/`stopset` 是可复刻的有限机制（`parser.go:326-375`） |
| **候选** | ① 最小同步（首错即停 / 仅跳过当前语句）② 语句/声明边界同步 ③ **完整对齐官方 `advance` + `stopset`** |
| **结论** | **③**。复刻 `advance(followlist...)`、`stopset`（**14** 个语句起始关键字）、`followset` 计算，以及 `list`/`stmtList` 层的恢复点 |
| **否决理由** | ① 多错用例只能报第一个错，负例覆盖弱，M4 对齐 errorcheck 会吃力；② 与官方 oracle 行为不一致，对撞时错误位置会分叉 |
| **失效条件** | 若完整对齐的实测工作量超出 M2 预算（2–4 人周 / 6 周）⇒ 按 [`03`](../03-roadmap.md) §6.1 的预算/日历**双触发**规则**先延期后降级**，并把降级写进 manifest 偏离项，**不静默放宽** |

> ⚠️ **与 roadmap 的偏离（显式登记）**：[`03-roadmap.md`](../03-roadmap.md) §M2 原文的砍项写的是「**砍广泛恢复策略/AST 展示**」。本决策把恢复策略从"砍"上调为"完整对齐官方"，**与砍项直接冲突**。
>
> **处理**：不在本阶段回头改 roadmap（roadmap 是 M0 时期的计划预估，冻结改动需单独决策），而是在 `M2-design.md`（本节）与 `M2-manifest.json` 的 `open_gaps`/偏离项里**如实登记**，让将来对照计划时能一眼看出差异。若后续希望**严格守 roadmap**，只需把本决策回退到候选②，其余设计不受影响。

#### D-M2-3　验收语料范围

| 字段 | 内容 |
|---|---|
| **判据** | 官方对 parser 最强的自测是 `TestStdLib`（解析 `GOROOT/src` + `misc`，`parser_test.go:41-90`）；`03` §M2 要求 testdata 全绿 |
| **候选** | ① 仅 syntax/testdata（31 文件）② **testdata + 标准库全量** ③ testdata + 标准库 + `test/` 目录 errorcheck 负例 |
| **结论** | **②**。testdata 31 文件（正例 + `// ERROR` 负例）作主判据；`go_source_code/src` 全部 `.go` 作**冒烟**（零意外错误） |
| **否决理由** | ① 缺少"真实大规模 Go 代码"证据；③ `test/` 目录涉及 `UNSUPPORTED-REGEX` 缺口（GAP-02，归 M4/M5 决定），提前纳入会把 M4 的问题拉进 M2 |
| **失效条件** | 若标准库冒烟暴露大量**非 parser 层**的错误（如需要 importer 才能继续）⇒ 冒烟范围缩到"纯语法可判定的文件子集"，并登记被排除的文件 |

#### D-M2-4　crate 划分

| 字段 | 内容 |
|---|---|
| **判据** | AST 将被 M3/M4/M5 广泛依赖；M1 已是单 crate `rgoc-lex`；`03` §2 的「只创建当期需要的模块」原则 |
| **候选** | ① **`rgoc-ast`（纯类型）+ `rgoc-parse`（parser）** ② 单个 `rgoc-parse`（含 AST + parser）③ 都不建，parser 放 driver |
| **结论** | **①**。依赖方向 `rgoc-lex → rgoc-ast → rgoc-parse →（M3）` |
| **否决理由** | ② M3+ 只要 AST 也会把整个 parser 拉进依赖图；③ driver 是"工具型 crate 不含编译器逻辑"（`rgoc/Cargo.toml` 头注释），放 parser 违反分层 |
| **失效条件** | 若 M2 实现中发现 AST 与 parser 高度耦合、拆开反而需要大量 `pub(crate)` 穿透 ⇒ 可合并，但合并要经独立决策并登记 |

#### D-M2-5　规范化 dump 形式

| 字段 | 内容 |
|---|---|
| **判据** | 对撞要逐行 diff，Go 侧与 Rust 侧的 AST 组织不同，必须有**中立**中介格式；官方 `Fdump` 明说"格式不保证"且依赖 reflect（`dumper.go:17-19`） |
| **候选** | ① **中立缩进语法**（双方各写 dumper 输出同格式）② JSON 结构化 ③ 对齐官方 `Fdump` |
| **结论** | **①**。约定一套**中立节点名 + 字段名**，输出缩进文本（如 `ForStmt` / `Init` / `Cond` / `Body`），双方逐行 diff |
| **否决理由** | ② 键序/空白/数字格式需归一化，差异定位不如逐行直观；③ 官方 `Fdump` 靠 reflect 打印 Go 内部结构（含 `*Name @ pos` 特例），Rust 侧**无法复现** ⇒ 不可行 |
| **失效条件** | 若中立格式在实现中难以覆盖"两边结构不同"的节点（如 Go 的 `ListExpr` 无对应物）⇒ 扩展中立格式加入"规范化规则"（如"列表节点统一 n 元扁平化"），并把规则写进 C6 契约 |

#### D-M2-6　版本门控（PIPE-04）深度

| 字段 | 内容 |
|---|---|
| **判据** | `PIPE-04` 基准是 `gc/base/flag.go`(`-lang`) + `gc/syntax/syntax.go`；官方 `File.GoVersion` 承载 `//go:build` 取的版本（`nodes.go:45`）；M2 无类型检查 ⇒ 门控只能在语法层 |
| **候选** | ① **语法层门控** ② 只解析不门控 ③ 语法门控 + 逐条 Spec Appendix 特性表 |
| **结论** | **①**。parser 按 `goVersion` 决定是否接受新语法（泛型语法、`range over func` 语法等），**只判语法合法性，不碰类型语义** |
| **否决理由** | ② 会与官方 parser 的拒绝行为不一致，对撞必然失败；③ 要维护一张与 Spec Appendix 同步的大表，工作量显著且易腐坏 |
| **失效条件** | 若对撞发现官方还有 **非语法层**的门控（如依赖类型信息的 `-lang` 检查）⇒ 该部分归 M4，M2 只覆盖语法层并登记缺口 |

#### D-M2-7　交付产物范围

| 字段 | 内容 |
|---|---|
| **判据** | `03` §M2 列了 "AST dump"；M1 已建立 `rgoc-driver lex` 模式（`cli.rs` 的 `Command::Lex`） |
| **候选** | ① **`rgoc ast` 子命令**，不做 printer ② 只做库，不加子命令 ③ `ast` 子命令 + 移植官方 printer |
| **结论** | **①**。driver 新增 `rgoc ast <file>`（dump AST 给人看），对齐 `rgoc lex` 模式 |
| **否决理由** | ② 无法手工 inspect AST，调试只靠测试；③ 官方 `printer.go` 22 KB，是独立能力（AST→源码），超出 M2 必需范围 |
| **失效条件** | 若后续要做 gofmt 类工具 ⇒ printer 是独立里程碑，不在 M2 补 |

### 2.3 事实基线（核对自官方源码，行号可复查）

> **性质说明**：以下为**读 `go_source_code/src/cmd/compile/internal/syntax/` 源码核对**的事实（M0/M1 的"实测"指**运行**验证；本阶段的**运行级实测**留 `M2-tests.md` 阶段用 oracle adapter 完成）。行号以 `go1.27.1` 为准。

| ID | 事实 | 出处 |
|---|---|---|
| `F1` | parser 状态核心是 `top` / `fnest`（函数嵌套层，决定 stopset 生效） / `xnest`（表达式嵌套层，complit 二义用） | `parser.go:32-35` |
| `F2` | 错误恢复靠 `advance(followlist...)`：循环 `next()` 直到 token 属于 followset 或 EOF | `parser.go:345-375` |
| `F3` | `stopset` = **14** 个语句起始关键字（break/const/continue/defer/fallthrough/for/go/goto/if/return/select/switch/type/var），**仅当 `fnest>0` 时并入 followset** | `parser.go:326-339` |
| `F4` | `errorAt`：**`errh==nil` 时 `panic(p.first)`**（遇第一个错即停）；`errh≠nil` 时回调继续 | `parser.go:239-249` |
| `F5` | `syntaxErrorAt`：EOF 且已有错时**抑制后续**（避免无意义连环错）；消息前缀 `syntax error: ` | `parser.go:252-298` |
| `F6` | `//line` 指令由 **parser** 的 `updateBase()` 处理（scanner 只把文本交给 parser 的回调） | `parser.go:61-75`、`124-175` |
| `F7` | `//go:build` **只在 `p.top`（package 子句之前）**且用 `constraint.Parse` 成功时，才写入 `p.goVersion` | `parser.go:79-83` |
| `F8` | `//go:` 指令经 `pragh(pos, blank, text, current) Pragma` 回调；`text` 已去掉 `//` 前缀 | `syntax.go:41-53`、`parser.go:84-86` |
| `F9` | 节点 `Pos` 规则：**终端节点**（Name/BasicLit）= 该 token 位置；**非终端节点** = 最左标识其种类的 token 位置 | `nodes.go:12-24`、`nodes_test.go:22-24,63-65` |
| `F10` | 官方用 `@` 标记片段测 `Pos` 规则（decls/exprs/stmts/types 四组） | `nodes_test.go:13-19` |
| `F11` | `// ERROR rx` 注释声明期望错误；`rx.MatchString(e.Msg)` ⇒ **子串匹配**（非全等） | `error_test.go:12-27`、`150-158` |
| `F12` | 行注释 `// ERROR`：只校验**行号**；块注释 `/* ERROR */`：校验**位置（行+列）** | `error_test.go:17-23`、`141-148` |
| `F13` | 官方错误测试入口 = `ParseFile(filename, errh, nil, CheckBranches)` | `error_test.go:164` |
| `F14` | `Fdump` 依赖 `reflect`，注释明说"no specific output format is guaranteed" | `dumper.go:17-19` |
| `F15` | `testdata/` 共 **31 个文件**；**25 个含 `// ERROR`（约 122 条）**，**6 个纯正例**（chans/interface/map/map2/linalg/slices） | 本次枚举 |
| `F16` | `ChanDir`：`SendOnly=1`、`RecvOnly=2`（`0` 表示无方向） | `nodes.go:340-346` |
| `F17` | `CommentKind`：`Above=0`/`Below`/`Left`/`Right` | `nodes.go:492-505` |
| `F18` | `checkBranches` 独立于 parser，在 `funcBody` 后可选调用（`Mode::CheckBranches`） | `branches.go:16-45`、`syntax.go:17-19` |
| `F19` | `Parse` 外层有 `recover`：把内部 `panic(Error)` 转成 `first error` 返回 | `syntax.go:66-81` |

### 2.4 关键设计取舍（不由五字段承载，直接采纳）

以下三条由 `feature-set.md` 与官方基准**共同钉死**，不再单列决策：

1. **AST 用和类型（enum）**：官方是「接口 + 嵌 `node` 的结构体」，Rust 对应 `enum`。`01-feature-set.md` 的 AST 表已逐条给出映射（`AstFile` / `Decl::Import` / `Expr::Bad` / `Type::Array`…），直接采用。
2. **节点持有点用 `Box`（所有权树）**：不引 arena。M2 追求**正确性对齐**，不是性能；`Box` 与官方指针树同形，便于对撞。
3. **辅助类型用 `struct`**：`Field` / `Group` / `CaseClause` / `CommClause` / `Comment` 按官方原样做结构体。

---

## 3. 数据模型（AST）

### 3.1 顶层与三类和类型

官方顶层 `File`（`nodes.go:40-47`）→ Rust：

```rust
pub struct AstFile {
    pub pos: Pos,                 // File 节点位置（文件起点）
    pub pragma: Pragma,           // 文件级 //go: 指令
    pub pkg_name: Name,           // package 名
    pub decls: Vec<Decl>,         // 顶层声明
    pub eof: Pos,                 // EOF 位置
    pub go_version: Option<String>, // 来自 //go:build（F7）
}
```

三类**和类型**（对应官方 `Decl`/`Expr`/`Stmt` 三个接口族 —— **类型没有独立接口族**，见下）：

```rust
pub enum Decl { Import(ImportDecl), Const(ConstDecl), Type(TypeDecl), Var(VarDecl), Func(FuncDecl) }
pub enum Expr { Name(Name), BasicLit(BasicLit), Binary { op: Op, x: Box<Expr>, y: Box<Expr> }, Call(CallExpr), Bad(BadExpr), /* …类型与表达式变体见 §6… */ }
pub enum Stmt { Empty, Block(BlockStmt), If(IfStmt), For(ForStmt), /* …见 §6… */ }
```

四点约定：

- **类型与表达式同族**（**对齐官方**）：官方 `type_()` / `typeOrNil()` **返回 `Expr`**（`parser.go:1322`），与 `expr()` 同族 —— 因为 parser 阶段**无法区分**类型与表达式（`T` 既可能是类型名也可能是变量名；`x.(T)` 的 `T` 要到 M3 才定）。所以 Rust 侧 **不设独立的 `TypeExpr` 枚举**：`AST-24..32` 的类型节点（Array/Slice/Struct/Map/Chan/Interface/Func/Dots）都是 **`Expr` 的变体**（如 `Expr::ArrayType`）。
- **错误占位只在表达式层**：官方**只有** `BadExpr`（`nodes.go:162`）；`stmtOrNil` 遇无法识别的语句**返回 `nil`**（`parser.go:2709`），`funcDeclOrNil` 同理。所以 Rust 侧：`Expr` 用显式变体 `Expr::Bad(BadExpr)`（类型错误也复用它，因为类型属 `Expr` 族）；**`Stmt`/`Decl` 不设 `Bad` 变体**，错误处对齐官方返回 `None`。
- **`Stmt`/`Decl` 的错误用 `Option`**：`stmtOrNil` / `funcDeclOrNil` 对应 Rust 的 `Option<Stmt>` / `Option<Decl>`，`None` = "此处无成功解析的节点（含错误）"。
- **错误位置由诊断流承载**：AST 返回 `None` **不丢位置** —— 错误位置在 `Diag.pos`（§5.4）。AST 只承载"成功解析出的结构"。

> ⚠️ **与 `01-feature-set.md` 映射列的口径差异（登记）**：该表 `AST-24..32` 的映射列写的是 `Type::Array` 等。这是**语法类别的标注**；M2 实现按官方口径统一落在 **`Expr` 族**（`Expr::ArrayType`），**不新建 `TypeExpr` 枚举**。理由：parser 阶段类型与表达式不可分（官方同族）、对撞要求结构一致。此差异在 `M2-tests.md` 与 `M2-manifest.json` 登记。

### 3.2 辅助结构（struct）

| Rust 结构 | 对应官方 | 关键字段 |
|---|---|---|
| `Name { value: String, pos: Pos }` | `Name`（`nodes.go:167-170`） | 标识符 |
| `BasicLit { value: String, kind: LitKind, bad: bool, pos }` | `BasicLit`（`nodes.go:173-178`） | 字面量原文（**值不算**，M3 才折叠） |
| `Field { name: Option<Name>, type_: Expr, pos }` | `Field`（`nodes.go:299-303`） | 字段/参数/结果/接口方法/嵌入元素统一载体 |
| `Group` | `Group`（`nodes.go:139-141`） | 分组声明共享 iota 上下文 |
| `CaseClause { cases: Option<Expr>, body: Vec<Stmt>, colon: Pos }` | `CaseClause`（`nodes.go:462-467`） | `cases==None` → default |
| `CommClause { comm: Option<SimpleStmt>, body, colon }` | `CommClause`（`nodes.go:469-474`） | `comm==None` → default |
| `Comment { kind: CommentKind, text: String, next: Option<Box<Comment>> }` | `Comment`（`nodes.go:501-505`） | M1 已交文本；M2 挂到节点（若需要） |
| `Pragma` | `Pragma any`（`syntax.go:36-39`） | 指令记号，由 `PragmaHandler` 决定内容 |

**`SimpleStmt`**：官方是 `Stmt` 的子接口（`nodes.go:357-360`），用于 `if`/`for`/`switch` 的 init 位。Rust 侧用 **`enum SimpleStmt`**（`Empty`/`Expr`/`Send`/`Assign`/`RangeClause`…），与 `Stmt` 分开。

### 3.3 位置规则（**必须逐条对齐官方，`F9`**）

| 节点类别 | `pos` 取值 | 例子（官方 `nodes_test.go`） |
|---|---|---|
| **声明** `*Decl` | 该声明的**第一个 token**，**与是否分组无关** | `import @"math"`、`const (@x)` → pos 在 `x` |
| **表达式** `*Expr` | **最左的、标识其种类的** token | `@x`（Name）、`@0`（BasicLit）、`a@+b`（Binary 取 `+`） |
| **终端节点**（`Name`/`BasicLit`） | 该 token 本身位置 | `@x` |
| **非终端节点** | 唯一关联 token，通常最左 | `IndexExpr` 取 `[`，`IfStmt` 取 `if` |

> **这是 M2 最容易出"差 1"的地方**：位置算错不会报错，表现为 golden/对撞 diff 里某一列差 1，极难定位（`C1-source-map.md` §2.1）。`M2-tests.md` 必须为四类各给 `@` 标记用例（门禁 `A3`）。

### 3.4 与 `01-feature-set.md` 的映射

`AST-01..49` 的 Rust 映射列已在 `01-feature-set.md` 逐条给出（`AstFile` / `Decl::Import` / `Expr::Bad` / `Field` / `Comment` …）。**本设计不重复该表**，只登记三处口径修正（写 `M2-tests.md` 时同步）：

1. **类型节点落在 `Expr` 族**（§3.1）：`AST-24..32` 的映射列 `Type::Array` 等是**语法类别标注**，实现为 `Expr::ArrayType` 等，**不新建 `TypeExpr`**（对齐官方"类型 = 表达式"）。
2. **官方无 `BadStmt`/`BadDecl`/`BadType`**（§3.1）：语句/声明错误对齐官方返回 `None`；`AST-09 BadExpr` 同时承载表达式与类型错误。
3. **`SimpleStmt` 独立成 enum**（§3.2）：官方是 `Stmt` 子接口，Rust 侧独立枚举（`if`/`for`/`switch` 的 init 位专用）。

---

## 4. 模块结构与依赖方向

```text
rgoc-lex ──► rgoc-ast ──► rgoc-parse ──► (M3 类型 / M4 检查 / M5 HIR)
   (M1)         (新)          (新)
                 │
           中立 dump 器 ──► 规范化 dump ◄── tools/ast-oracle（Go）
```

| crate | 职责 | 依赖 | 测试 |
|---|---|---|---|
| **`rgoc-ast`** | AST 类型定义（`AST-01..49`）+ 中立 dump 器 | `rgoc-lex`（只取 `Pos`/`FileId`） | AST 构造/遍历/dump 单测 |
| **`rgoc-parse`** | parser（手写递归下降）+ 错误恢复 + pragma + branches | `rgoc-ast` + `rgoc-lex` | parser 验收 + 对撞 + testdata |

**两个 crate 的边界纪律**：

1. **`rgoc-ast` 不含任何 parser 逻辑** —— 它是纯数据 + 遍历 + dump。任何"怎么解析"的代码都在 `rgoc-parse`。
2. **`rgoc-parse` 不重复定义 AST 类型** —— 只 `use rgoc_ast::*`。
3. **判定逻辑只有一处**（`AGENTS.md` §1）：AST 的结构判定（"这是不是一个 `for` 语句"）只在 `rgoc-ast` 定义，driver 与测试都只**调**不自己判。

**driver 集成**：`rgoc-driver` 新增 `Command::Ast`（对齐 `Command::Lex`，`cli.rs` 手写解析，不引 clap）。

---

## 5. 数据流与算法

### 5.1 parser 状态（对齐 `F1`）

| 状态 | 作用 | 官方 |
|---|---|---|
| `tok` | **唯一前瞻**词元 + 其载荷 | `scanner.tok` |
| `top: bool` | 是否在 package 子句之前（`//go:build` 只在此时取 `goVersion`） | `p.top` |
| `fnest: int` | 函数嵌套层，**决定 stopset 是否并入 followset** | `p.fnest` |
| `xnest: int` | 表达式嵌套层，**complit 二义消解**用（§5.6） | `p.xnest` |
| `pragma: Pragma` | 当前待挂载的指令 | `p.pragma` |
| `errcnt / first` | 错误计数与首个错误 | `p.errcnt` / `p.first` |
| `base: PosBase` | 当前位置基准（`//line` 重映射改它） | `p.base` |

**只有 `tok` 一个前瞻词元** —— Go 的 `parser.go` 不需要 LL(2)（`gotAssign` 是特殊的 `:=` 处理）。这一点必须保持：多前瞻会让错误恢复与官方对不上。

### 5.2 分派骨架（对齐官方函数结构）

```text
fileOrNil                    顶层：package → { ImportDecl | TopLevelDecl } ";" 
  └─ appendGroup(importDecl | constDecl | typeDecl | varDecl) / funcDeclOrNil
       ├─ typeDecl   → type_()        （类型语法）
       ├─ funcDeclOrNil → funcBody   → stmtList
       └─ varDecl/constDecl → exprList

stmtOrNil                    语句分派（按首 token）
  ├─ _Name       → exprList → labeledStmtOrNil | simpleStmt
  ├─ _Var/_Const/_Type → declStmt
  ├─ _For/_Switch/_Select/_If → forStmt/switchStmt/selectStmt/ifStmt
  ├─ _Go/_Defer  → callStmt
  └─ _Return/_Break/_Continue/_Goto/_Fallthrough/_Semi → 直接构造

expr()  → binaryExpr(x, prec) → unaryExpr → operand → pexpr
type_() → typeOrNil / typeInstance / funcType / arrayType / sliceType /
          structType / interfaceType / chanElem / mapType
```

**表达式层的优先级**：`binaryExpr(x, prec)` 用**优先级爬升**（不走独立函数树），这是官方做法，必须复刻（否则 `a+b*c` 的结合形状可能不同）。

### 5.3 错误恢复（对齐 `F2`/`F3`/`F4`）

**核心是一个函数**：

```text
advance(followlist...):
    followset = {EOF}                       # 不跳过 EOF
    if followlist 非空:
        if fnest > 0: followset |= stopset  # 函数体内才用语句起始关键字
        followset |= followlist
    while tok ∉ followset:
        next()
        if followlist 为空: break           # 空 followlist = 消费一个 token 保证前进
```

**三个恢复点**（对齐官方）：

| 位置 | 恢复目标 | 官方 |
|---|---|---|
| 顶层声明之间 | `advance(_Import, _Const, _Type, _Var, _Func)` | `parser.go:469,479` |
| 语句列表 | `advance(_Semi, _Rbrace, _Case, _Default)` | `parser.go:2728` |
| 括号内列表 | `advance(_Rparen, _Rbrack, _Rbrace)` | `parser.go:518` |

**`errh` 两档**（`F4`）：M2 取「**尽量继续**」档（`errh≠nil`）。`errh==nil` 的"首错即停"档**仍要实现**（官方保留两种），但它不是 M2 的主判据。

**抑制连环错**（`F5`）：EOF 且已有错时，`syntaxError` 不再报 —— 复刻，否则文件尾会喷一串无意义错误。

### 5.4 诊断（对齐 C1）

| 层面 | Rust 侧 | 说明 |
|---|---|---|
| 错误载体 | `Diag { pos: Pos, code: DiagCode, msg: String }` | **沿用 M1 的 `rgoc-lex::diag`**（C1 契约） |
| 收集 | `DiagBag`（沿用 M1） | 排序规则由 C1 规定 |
| 消息 | 复刻官方消息**子串**（`syntax error: unexpected X, expected Y` 形态） | 因对撞要求消息匹配（`F11`），**M2 必须复刻消息文本**（与 M1 的 `D-M1-9`「不复刻消息」不同 —— 见下方理由） |

> ⚠️ **与 M1 的口径差异（重要）**：M1 的 `D-M1-9` 定的是「诊断判据 = 位置 + `DiagCode`，**不复刻官方消息文本**」，因为词法 golden 是 oracle 生成的 token 流，不含官方消息。**M2 不同**：testdata 的 `// ERROR` 直接匹配**官方消息**（`F11`/`F13`），所以 parser 的**错误消息必须复刻官方文本**。这是阶段性质决定的，不是自相矛盾，`M2-tests.md` 要写明。

### 5.5 pragma 与 `//line`（对齐 `F6`/`F7`/`F8`）

**`//line` 位置重映射**（M1 明确推迟给 M2）：

```text
//line filename:line[:col]     →  后续 token 报告的 (file,line,col) 以此为准
```
- 由 `updateBase(pos, tline, tcol, text)` 解析（`F6`），改的是 `parser.base`。
- **判据**：重映射后，后续节点与诊断的 `Pos` 都指向**新基准**。`M2-tests.md` 要专门给 `//line` 用例。

**`//go:` 指令解析**（只认不生效）：
- 经 `PragmaHandler` 回调（`F8`），`text` 已去 `//` 前缀。
- `//go:build` 额外走 `constraint.Parse` 取 `goVersion`（仅 `top` 时，`F7`）。
- **已知指令清单**：M2 解析并挂载 `//go:build` / `//go:linkname` / `//go:nosplit` / `//go:noescape` 等（`PIPE-05` 列举）为 `Pragma`；**未知关键指令**按 `03` §M2「拒绝未知关键指令」处理（清单与拒绝规则在 `M2-tests.md` 冻结）。

### 5.6 表达式二义消解（`xnest`，对齐 `F1`）

`p.xnest` 解决 Go 最著名的语法二义：

```go
x.y(z)        // 是"调用"还是"类型转换"？解析时无法确定
```

- **在 if/for/switch 的 header 里**（`{}` 可能开始块）：`xnest = -1`（**禁用** complit）。
- **在括号/表达式内**：`xnest >= 0`（允许 complit）。
- 复刻官方 `pexpr` 的 `xnest` 增减逻辑（`parser.go:1104-1259`），否则 `if x == T{} {}` 类用例会对撞失败。

### 5.7 标签检查（`branches.go`，对齐 `F18`）

`checkBranches(body, errh)` 独立于 parser，在 `funcBody` 后**可选**调用（`Mode::CheckBranches`，`F18`）。它检查：

- 错位的 break/continue/fallthrough
- 非法/未用/重复/缺失的标签
- goto 跳过变量声明或跳进块

**M2 必须实现**，因为 `// ERROR` 测试用 `CheckBranches` 模式（`F13`），且 testdata 里 `fallthrough.go` 全是标签/分支错误。

---

## 6. AST 节点覆盖清单（`AST-01..49`）

**本设计不复述 49 个节点的字段**（`01-feature-set.md` 已有逐条表 + 官方行号）。这里只给**测试分组**，供 `M2-tests.md` 直接展开：

| 组 | 节点 | 官方来源 |
|---|---|---|
| **A. 文件与声明** | `AST-01..08`（File/Group/Comment/Import/Const/Type/Var/Func） | `nodes.go:40-141` |
| **B. 表达式** | `AST-09..23`（Bad/Name/BasicLit/CompositeLit/KeyValue/FuncLit/Paren/Selector/Index/Slice/Assert/TypeSwitchGuard/Operation/Call/List） | `nodes.go:153-268` |
| **C. 类型** | `AST-24..32`（Array/Slice/Dots/Struct/Field/Interface/Func/Map/Chan） | `nodes.go:270-330` |
| **D. 语句** | `AST-33..49`（Empty/Labeled/Block/Expr/Send/Decl/Assign/Branch/Call/Return/If/For/Switch/Select/RangeClause/CaseClause/CommClause） | `nodes.go:351-474` |

**每个节点至少一条直接测试**（退出条件 1）；每个节点至少在其所属的**对撞语料**里出现一次。

---

## 7. 错误处理与资源上限

### 7.1 恢复原则

**报错但继续，产出畸形节点**（沿用 M1 的 §7.1 精神）：

- 表达式错 → `Expr::Bad(BadExpr)`（官方同形）
- 语句错 → 对齐官方 `stmtOrNil`：返回 `None`，在语句边界停下（**官方无 `BadStmt`**）
- 声明错 → 对齐官方 `funcDeclOrNil`：返回 `None`，`advance` 到下一个声明继续

### 7.2 诊断代号表（`DiagCode`）

**续 M1 的 `rgoc-lex::diag::DiagCode`**（C1 契约，M1 已有 23 个代号）。M2 新增 parser 相关代号（如 `UnexpectedToken` / `ExpectedSemicolon` / `InvalidLineDirective` …），总数在 `M2-tests.md` 冻结。

> ⚠️ **M1 的教训（D-M1-11）**：代号表"凭印象手写会漏条且无报错"。M2 的做法：**从官方 `syntaxError` 的调用点枚举**全部报错点，冻结条目数，并加**跨语言对撞断言**（同 M1 的 `--selfcheck`）。

### 7.3 资源上限

沿用 M1 的**单文件 64 MiB** 上限（`D-M1-4`）。parser 不新增上限，但需**深度保护**：表达式/语句嵌套过深会导致栈溢出 —— 官方靠 `go` 的栈增长，Rust 侧需要**显式递归深度上限**，超限报诊断而非崩溃。上限值在 `M2-tests.md` 定。

---

## 8. 测试策略

### 8.1 四条腿

| 腿 | 判据 | 对应官方机制 | 门禁 |
|---|---|---|---|
| **甲：节点 Pos 规则** | `@` 标记片段，逐条断言节点 pos | `nodes_test.go`（`F10`） | `A3` |
| **乙：testdata 正/负例** | 31 文件：6 正例零错；25 负例 `// ERROR` **双向**匹配（声明的都报出、报出的都被声明） | `error_test.go`（`F11`/`F12`） | `A2` |
| **丙：oracle 对撞** | 规范化 dump 逐行 diff 为空（testdata 31 + 自有切片） | 新增（`tools/ast-oracle`） | `A4` |
| **丁：标准库冒烟** | `go_source_code/src` 全部 `.go` 解析**零意外错误** | `TestStdLib`（`F15` 语料） | `A5` |

> **甲的"双向"很重要**：负例不仅要"报出的错都在声明里"，还要"声明的错都报出来了" —— 只做前者会漏掉"该报没报"（M1 的 `D-M1-2` 已有同形纪律）。

### 8.2 dump 规范（C6，`D-M2-5`）

**中立格式草案**（细节进 C6 契约）：

```text
File
  PkgName: Name "p"
  Decls: [3 entries]
    FuncDecl
      Name: Name "f"
      Type: FuncType
        Params: []
    ...
```

规则：节点名用**中立名**（两边共有）；列表显式标 `[n entries]`；可选字段用 `-` 占位（如 `Recv: -`）；位置默认**不 dump**（另用 `@` 腿测），需要时加 `@pos` 开关。

### 8.3 对撞的等价性（风险点）

Rust 与 Go 的 AST **组织不同**（如 Go 的 `ListExpr`、`Operation` 在 Rust 侧可能拆成多个变体）。**C6 必须定义"规范化规则"**（如"列表节点统一扁平化"、"一元/二元 operation 统一"），否则对撞会假红。**这是 M2 最大的实现风险**（§12 `R1`）。

---

## 9. oracle adapter（`tools/ast-oracle`）

**沿用 M1 的 adapter 模式**（`tools/token-oracle`，`D-M1-3`）：

```text
tools/ast-oracle/
├── README.md             来源、版本、校验值、格式、再生成步骤
├── go.mod                module astoracle（仅标准库 + go/build/constraint）
├── main.go            ★  命令行入口：读源码 → Parse → 输出中立 dump
├── dump.go            ★  官方 AST → 中立格式的遍历器
└── gosyntax/             vendor 官方 syntax 包
    ├── LICENSE           BSD-3-Clause（随语料根）
    ├── nodes.go          原样复制 ┐
    ├── parser.go         原样复制 │
    ├── scanner.go        原样复制 │ vendor 的官方文件（**零改动**）
    ├── branches.go / positions.go / source.go / tokens.go / … │
    ├── dumper.go / syntax.go                    ┘
    └── export.go      ★  同包 shim：唯一能做转发的地方
```

**三个关键点**（都来自 M1 的实测教训）：

1. **官方 `syntax` 包内部标识符未导出** ⇒ 必须在同 package 加纯转发 `export.go`（M1 `§9.1` 的坑，M2 同形）。
2. **样本文件不能叫 `.go` 且与 adapter 同目录**（`main redeclared`，M1 `§9.3`）。
3. **`dump.go` 是手写的**：不能直接用官方 `Fdump`（`F14` 靠 reflect），必须自己遍历官方 AST 输出**中立格式**。

> **adapter 是"仅标准库依赖的 Go 程序"**：它编译成独立二进制，在容器内运行，与 rgoc 的 Rust dumper 输出同格式，再由脚本 diff。

---

## 10. 契约与产物影响

| 契约 | 影响 | 动作 |
|---|---|---|
| **C1**（位置/诊断） | parser 是**第二个消费者**；`//line` 重映射在此阶段落地 | **延续**，补"parser 侧位置语义"一节 + 变更记录 |
| **C2**（Test IR） | 不变（M2 用例不走 harness） | 不动 |
| **C6**（AST dump 格式） | **新增** —— 对撞的中介格式，Rust↔Go 的跨语言接口 | **新产出** |

**新增产物**：

| 产物 | 路径 |
|---|---|
| `rgoc-ast` crate | `rgoc/crates/rgoc-ast/` |
| `rgoc-parse` crate | `rgoc/crates/rgoc-parse/` |
| ast-oracle | `tools/ast-oracle/` |
| AST 对撞语料 | `rgoc/tests/ast/`（golden + dump） |
| C6 契约 | `docs/contracts/C6-ast-dump.md` |
| `rgoc ast` 子命令 | `rgoc/crates/rgoc-driver/src/ast_cmd.rs` |

---

## 11. M2 门禁汇总

| # | 门禁 | 判据 | 证据落点 |
|---|---|---|---|
| `A1` | `AST-01..49` 各至少一条直接测试 | `rgoc-ast` / `rgoc-parse` 测试全绿 | `M2-manifest.json` `coverage` |
| `A2` | testdata 31 全绿（正例零错 + 负例双向） | `scripts/check-ast-testdata.sh` | 对撞报告 |
| `A3` | 节点 `Pos` 规则全绿（`@` 标记） | `rgoc-parse/tests/test_pos.rs` | 单测 |
| `A4` | oracle 对撞全绿（dump 逐行 diff 为空） | `scripts/check-ast-golden.sh` | 对撞报告 |
| `A5` | 标准库冒烟零意外错误 | `go_source_code/src` 全量解析 | 冒烟报告 |
| `A6` | 四条 cargo 命令全过 | fmt / check / clippy / test | gate 日志 |
| `A7` | C1 延续 + C6 新增 | 契约头 `level` + 变更记录 | 契约文件 |
| `A8` | GAP-01 的 AST/PIPE 四维归属已定 | 续 `GAP-01.md` | `GAP-01.md` |

> **门禁编号用 `A`**（AST），与 M0 的 `E*`、M1 的 `L*` 区分。具体条目在 `M2-manifest.json` 的 `gate` 节冻结。

---

## 12. 风险与未决

| ID | 风险 | 说明 | 缓解 |
|---|---|---|---|
| `R1` | **对撞等价性**（§8.3） | Rust 与 Go AST 组织不同，中立格式可能无法一一对应 ⇒ 假红 | C6 定义规范化规则；先小语料验证再扩 |
| `R2` | **错误消息复刻**（§5.4） | `// ERROR` 匹配官方消息子串 ⇒ 消息必须对齐，工作量大 | 先对齐 `syntaxError` 的消息模板（有限几类） |
| `R3` | **完整恢复的预算**（`D-M2-2`） | 完整对齐官方 `advance` 可能超 M2 预算 | 按 §12 预算触发：先延期后降级，**不静默放宽** |
| `R4` | **标准库冒烟的边界**（`D-M2-3`） | 部分文件可能需要 importer 才能判定 | 冒烟范围可按文件子集收缩，被排除项登记 |
| `R5` | **递归深度**（§7.3） | 深嵌套可能栈溢出 | 显式深度上限 + 诊断 |
| `R6` | **`xnest` 二义**（§5.6） | complit 二义消解复杂，易对撞失败 | 专门测试组 + 对齐 `pexpr` |

**未决项**（留 `M2-tests.md` / 执行期）：

- `//go:` **已知指令清单**与"拒绝未知关键指令"的具体规则（§5.5）。
- **规范化规则**的具体条目（§8.3，C6 初稿时定）。
- `DiagCode` 的 M2 新增条目与冻结总数（§7.2）。

---

## 13. 与后续文档的接口

### 13.1 交给 `writing-plans` 的输入

| 输入 | 出处 |
|---|---|
| 七条决策 + 五字段论证 | §2.2 |
| 数据模型（可直接落成类型定义） | §3 |
| 模块结构与依赖方向 | §4 |
| parser 算法骨架 | §5 |
| AST 覆盖清单与测试分组 | §6 |
| 四条测试腿 | §8 |
| adapter 组成 | §9 |
| 门禁八条 | §11 |

### 13.2 `M2-tests.md` 必须覆盖

- `AST-01..49` 的**逐项**测试 ID（不得有项无测试）
- 四条腿的具体用例清单与**期望值来源**（oracle 生成 / 手写）
- 负例清单及其**双向断言**（诊断 + 节点位置）
- `//go:` 已知指令清单与拒绝规则
- `DiagCode` 的 M2 新增条目与冻结总数
- 标准库冒烟的**文件范围**
- 分母：M2 的冻结测试 ID 总数（**开工前冻结**）

### 13.3 与后续阶段的衔接

| 交给 | 什么 |
|---|---|
| **M3** | AST（完整树）+ 字面量原文（值由 M3 算）+ `goVersion`（版本门控传递给类型检查） |
| **M4** | parser 诊断流（`DiagCode` + 位置，与 errorcheck 对齐的基础） |
| **M5** | AST → HIR lowering 的输入 |
| **M7+** | `//go:` 指令的 `Pragma`（效果生效） |

---

## 14. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-14 | **初稿创建**。七条决策（`D-M2-1..7`）经逐项确认；含 `D-M2-2` 与 roadmap 砍项的**偏离登记**；含 19 条源码核对事实基线（`F1..F19`） | 本仓设计会话（Superpowers `brainstorming`） |
| 2026-10-14 | **勘误 `F3`**：`stopset` 的关键字数 **13 → 14**（逐条核对 `parser.go:326-339`：Break / Const / Continue / Defer / Fallthrough / For / Go / Goto / If / Return / Select / Switch / Type / Var）。原「13」是**标数错误**，括号里列的 14 个关键字本身没错 —— 但「13」若被抄进断言就会变成恒真盲区。同一错误同步修正 `M2-tests.md` `T-A-61` 与 `M2-plan.md` `T-AP06` | 执行 `T-AP06` 时回源核对 |
