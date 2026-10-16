# M3 设计文档：常量、类型核心与最小包模型

> **阶段 ID**：`M3`　|　**状态**：**已确认**（2026-10-16）　|　**日期**：2026-10-16
> **文档索引**：[`../README.md`](../README.md)　|　**上游**：[`../03-roadmap.md`](../03-roadmap.md) §M3、[`../01-feature-set.md`](../01-feature-set.md)（`CON-01..08` / `TYP-01..18,22` / `SCP-01..10` / `PIPE-06..09`）、[`M2-design.md`](./M2-design.md) §13、[`M2-manifest.json`](./M2-manifest.json) 的 `open_gaps`
> **下游**：`M3-tests.md`（测试先行清单）→ `M3-plan.md`（任务级计划）→ `M3-manifest.json`（机器可读事实）
>
> **本文档的性质**：Superpowers 流程中「头脑风暴」阶段的产出 —— **只冻结设计决策与边界，不写任务粒度、不写实现代码**。任务级拆分是 `writing-plans` 的职责。
>
> **读者提示**：不懂 `oracle` / `golden` / `spike` 这些词，先读 [`M0-design.md`](./M0-design.md) §1.0。本文档假设读者已读过 [`M0-report.md`](./M0-report.md) §7「接手指南」与 [`M2-design.md`](./M2-design.md) §1。
>
> **§2 的决策记录是五字段**（判据 / 候选 / 结论 / 否决理由 / **失效条件**）。「失效条件」用来判断决定是否还成立 —— 只看结论无法判断结论对不对。
>
> **标「我方决定」的条目**（`D-M3-8..12`）是**设计者代为选定**的「怎么做」类取舍，**不是**用户已同意的事实；用户可在签字时逐条推翻。

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
| `docs/milestones/M3-design.md` | ✅ 本文件 | 设计决策与边界 |
| `docs/milestones/M3-tests.md` | ⬜ **待产出** | 必需测试 ID、比较器、验证命令、暂不支持项、**分母** |
| `docs/milestones/M3-plan.md` | ⬜ **待产出** | 2–5 分钟粒度实施任务 |
| 机器可读 manifest | ⬜ **待产出** | `M3-manifest.json`：`gate` + `test_ids` + `unsupported` + `budget` + `open_gaps` |

**本文档未确认前不进入下一步。** 本轮头脑风暴确认 **七条用户决策**（`D-M3-1..7`）与 **五条我方决定**（`D-M3-8..12`），并**显式登记两处与 roadmap 字面的偏离**（`D-M3-2` 泛型深度、`D-M3-7` 版本门控归属）。

---

## 1. 目标与非目标

### 1.0 推导：为什么 M3 是「从树到类型」

M2 交付的是**语法树**：它知道「这是一次调用、那是一个标识符」，但**不知道两个标识符是不是同一个东西，也不知道两个类型是不是同一个类型**。任何编译器在能回答「这段代码是什么意思」之前，必须先能回答**「这两个类型是不是同一个类型」**——这是类型检查、常量折叠、解释执行、单态化、ABI 生成**共同的底座**。

M3 就是把这块底座立起来：**给定一个包（多文件），建立它的作用域与类型表示，并给出两个判定——类型同一性、常量可表示性。**

### 1.1 M3 的输入与输出

| | 内容 |
|---|---|
| **输入** | `rgoc-parse` 产出的 `AstFile` 序列（一个包的多文件）+ 可选的 `-lang` 版本；M2 已解析的 `AstFile.go_version`（来自 `//go:build`） |
| **输出** | ① 类型表示（`Type` 枚举，覆盖官方全部形态）② 包作用域 / universe / 文件作用域 ③ 判定：`Identical(x,y)`、`Representable(c,T)` ④ 诊断（位置 + `DiagCode`）⑤ 中性 **类型 dump**（供 oracle 对撞，C7） |
| **不做** | 表达式/语句检查、可赋值性、方法集、接口实现判定、约束满足、实例化、推断（全在 M4） |

### 1.2 明确不做（M3 边界）

> 边界由 `D-M3-2`（表示做宽 / 判定做窄）与 `D-M3-5`（包模型中等）共同确定。

| 不做 | 归属 | 说明 |
|---|---|---|
| `TYP-13` 接口实现判定、`TYP-18` 可赋值性 | **M4** | 二者都需要「表达式/语句」消费方才成立；M3 无消费者 ⇒ **登记为对 roadmap 字面（`TYP-01..18`）的偏离**，见 `D-M3-2` |
| `TYP-19` 方法集 | **M4** | 实现判定的前置；M3 只表示「具名类型的祖先/嵌入关系」，不算方法集 |
| `TYP-23` 约束满足、`TYP-24` 实例化、`TYP-25` 推断、`TYP-26` 统一 | **M4** | 见 `D-M3-2`；M3 只**表示**类型参数/并集/`~T`，**不判定**它们 |
| 官方二进制 export data、模块下载、`go.mod`/模块图 | **M4+** | `PIPE-06` 只做**源码** import（`D-M3-5`） |
| 表达式/语句检查、内建签名校验、`main` 约束 | **M4** | roadmap §M4 |
| **构建约束筛选**（按 `//go:build` 选文件）、`_test.go`/cgo/汇编 | **M4+** | `D-M3-14`：M3 由调用方给**显式文件列表** |
| **需要类型信息才能判**的 `const` 初始化式 | **M4** | `D-M3-19`：M3 的常量求值器只吃**常量子集**，其余出稳定诊断 |
| 诊断消息与官方**逐字**一致 | **不承诺** | 沿用 C1：只对齐**位置与类别**（`M1-design.md` `D-M1-9`） |
| `//go:` 未知指令的裁定 | **noder 等价阶段**（后续） | 见 `D-M3-6` |
| 相对路径拼接 + `filepath.Clean`、`Pos::Ord` 改用 rel 坐标 | **M4/M5** | M2 `open_gaps` 结转，M3 不碰（需 SourceMap 目录信息） |

### 1.3 M3 退出条件（逐条可判定）

| # | 退出条件 | 判据 |
|---|---|---|
| 1 | **类型表示覆盖官方 `Type` 接口的全部形态** | `Type` 枚举对官方 `Type` 实现类的**穷尽性断言**（新增/缺失变体即红）；含 `Named`/`Alias`/`Basic`/`Pointer`/`Slice`/`Array`/`Map`/`Chan`/`Struct`/`Signature`/`Tuple`/`Interface`/`TypeParam`/`Union` |
| 2 | **类型同一性判对** | 自有 fixture 里每条「相等 / 不等」都判对，**且**与 `go/types` 对撞逐条一致；覆盖别名 / 具名 / 底层 / 实例化具名 / 函数（变参+方向）/ 结构体（字段名+Tag）/ 通道方向。**约束接口（含 `~T`/并集）的同一性对撞不在 M3 集内**（见 §5.2 与 `D-M3-13`） |
| 3 | **常量不静默截断** | 长字面量（含 `1<<100` 级）求值精确；超范围**报错**而非截断；可表示性判定与 oracle 一致 |
| 4 | **版本门控机制落地并生效** | 机制（`allowVersion` 等价）存在，且 M3 真有的门控点生效；输入来自 `-lang` 或 `//go:build`（`D-M3-7`） |
| 5 | **包模型（中等档）** | 单包多文件跨文件引用可解析；源码 import 可加载；**import cycle 有诊断**；导出符号摘要可产出（`D-M3-5`） |
| 6 | **拒绝侧双向断言** | M3 显式拒绝的输入**确实**报出稳定诊断码（正向）；未拒绝的输入**不**报错（反向）——防「只测一边」 |
| 7 | **四条 cargo 门禁 + 既有自检不回归** | `fmt --check` / `check --workspace --all-targets` / `clippy --all-targets -D warnings` / `test --workspace --no-fail-fast` 全绿；M0（231 断言）/ M1 / M2 自检全过 |
| 8 | **契约影响落定** | C7（类型 dump）新建冻结；C1 如有扩展同步；`unsupported` 如实登记 |

> **退出条件的形式要求**（沿用 M2 §8.1 的「四条腿」）：每条条件都要有 **crate 内单测 + golden + oracle 对撞 + 拒绝侧断言** 四类证据中的对应项，不接受「端到端看起来对」。

---

## 2. 已确认决策记录

### 2.1 决策一览

| ID | 决策 | 结论 | 来源 | 日期 |
|---|---|---|---|---|
| D-M3-1 | 里程碑主角与「立住了」的定义 | **类型核心为主**（常量 + 类型同一性），包模型为管道 | 用户 | 2026-10-16 |
| D-M3-2 | 泛型范围 | **表示做宽、判定做窄**（C）；偏离 roadmap 字面 | 用户 | 2026-10-16 |
| D-M3-3 | 验收判据 | **自有 fixture + `go/types` 对撞 oracle** | 用户 | 2026-10-16 |
| D-M3-4 | 常量精度策略 | **不引入实现限制**（整数任意精度 + 浮点精确有理数） | 用户 | 2026-10-16 |
| D-M3-5 | 包模型范围 | **中等**（路线图字面：源码 import + cycle + 摘要） | 用户 | 2026-10-16 |
| D-M3-6 | `//go:` 未知指令裁决 | 前端**收下不报错**；裁定归 noder 等价阶段 | 用户 | 2026-10-16 |
| D-M3-7 | 版本门控归属 | **类型检查层**（不在 parser）；`T-A-97` 重定到 sema | 用户 | 2026-10-16 |
| D-M3-8 | crate 划分 | `rgoc-const` / `rgoc-types` / `rgoc-load` 三 crate | **我方决定** | 2026-10-16 |
| D-M3-9 | 类型表示的形态 | **Rust enum + struct 直译官方**（穷尽性检查是杠杆） | **我方决定** | 2026-10-16 |
| D-M3-10 | oracle 形态 | `tools/types-oracle`：import 标准库 `go/types`，输出中性 dump | **我方决定** | 2026-10-16 |
| D-M3-11 | 「显式拒绝」的形态 | 稳定 `DiagCode` + 登记 `unsupported`；内部不可达仍 `todo!` | **我方决定** | 2026-10-16 |
| D-M3-12 | 大数实现来源 | **自研**整数 bigint + 有理数（不引第三方；不复用 SPIKE） | **我方决定** | 2026-10-16 |

### 2.2 五字段记录

#### D-M3-1　里程碑主角与「立住了」的定义

| 字段 | 内容 |
|---|---|
| **判据** | 用户要能指着**一件具体的事**说「这个阶段的类型核心真的立住了」；路线图字面含「常量 / 类型核心 / 最小包模型」三块 |
| **候选** | ① 三块等权推进 ② **类型核心为主、包模型为管道** ③ 包模型为主 |
| **结论** | **②**。可指着说的是「**一组自有 fixture：每条类型相等/不等的判断、每条常量可表示/溢出的判断，都与 spec（及 oracle）对得上**」——而不是「check 跑起来了」 |
| **否决理由** | ① 验收焦点散：三块各半成品也能「都跑起来」，正是本仓反复警惕的假绿；③ 与目的冲突——类型 identity 是下游的共同底座，loader 不是 |
| **失效条件** | 若 M4 开工时发现**缺 loader 导致类型核心无法跨包验证** ⇒ 重估优先级 |

#### D-M3-2　泛型范围：表示做宽、判定做窄

| 字段 | 内容 |
|---|---|
| **判据** | 路线图 M3 名单含「类型项/类型集/约束/统一的直接测试」，但同段注明「**不宣称整个泛型推断已完成**」；且 `F5`：interface 自 1.18 起**定义即类型集** |
| **候选** | ① 泛型完全不进 M3 ② 类型集/约束**最小判定**也进 M3 ③ **表示进、判定只做 identity + 常量** |
| **结论** | **③**。`Type` 表示**一次到位**（`TypeParam` / `Union` / `~T` / 实例化具名类型都进 enum）；M3 只实现 `Identical` 与 `Representable` 两个判定；**类型集 / 约束判定留 M4**，M3 撞上需要它的输入走 `D-M3-11`（显式拒绝 + 登记）。<br>**⚠️ 这是对 roadmap 字面（`TYP-01..18`）的偏离**：M3 不做 `TYP-13`（接口实现）、`TYP-18`（可赋值性），理由是二者在 M3 **没有消费者**（M3 不检查表达式/语句） |
| **否决理由** | ① M4 要**同时**迁移「表示」与「判定」，且 M3「类型底座」的声明会缩水成「**普通**类型底座」；② 类型集代数（并集归一 / `~T` 与 `T` 重叠 / `comparable` 特例）是**「不报错但会错」的重灾区**，且必须配 oracle 才能验收 ⇒ 成本前移且风险最高 |
| **失效条件** | 若 M3 的 fixture 中「跨文件引用」**必然**牵出约束/类型集判定（例如嵌入泛型接口的具名类型） ⇒ 需把最小判定提前到 M3 |

#### D-M3-3　验收判据：自有 fixture + `go/types` 对撞 oracle

| 字段 | 内容 |
|---|---|
| **判据** | 本仓的成立前提是「没有 oracle 就没有『与 spec 一致』的主张」（M2 先例：AST 靠 vendor `syntax` 逐字节对撞）；且必须能抓「不报错但会错」 |
| **候选** | ① 纯自有 fixture（照 spec 手写期望值）② 纯 oracle ③ **自有 fixture + `go/types` 对撞** |
| **结论** | **③**。两条腿缺一不可：自有 fixture 锚 spec 文本，oracle 抓「自己写的期望也错」；对撞面 = 类型 dump（C7）+ identity 判定 + 可表示性判定 |
| **否决理由** | ① 「与 spec 一致」只剩人工审阅；② oracle 也是**实现**，两边同错会被判绿（且 oracle 只覆盖它能表达的形态） |
| **失效条件** | 若 `go/types` 无法在**无 importer** 的情况下检查单包 fixture（必须真实 `go/importer`） ⇒ 退回「自有 fixture + spec 摘录」，并在 `open_gaps` 登记该缺口 |

#### D-M3-4　常量精度策略：不引入实现限制

| 字段 | 内容 |
|---|---|
| **判据** | spec 允许实现限制（整数 ≥256 位、浮点尾数 ≥256 位 + ≥16 位指数）但**要求超限报错、不得静默截断**；路线图要求「不为『至少 256 位』静默截断」；`CON-02` |
| **候选** | ① **真任意精度**（整数 bigint + 浮点精确有理数）② 有界 ≥256 位 + 超限显式报错 |
| **结论** | **①**。整数任意精度；浮点用**精确有理数**（这正是 spec 的语义模型：常量运算全程精确，只在**转换到具体类型**时按「向偶数舍入」舍入，`CON-04`） |
| **否决理由** | ② 要额外对齐「官方那个限制值到底是多少」⇒ 形成一份新的、要对撞、要复刻的契约；**且有界并不省掉浮点那块工作**——判 `float64(1)/3` 是否可表示，必须精确有理数算完再舍入，256 位的界省不了这一步，只多一层边界 |
| **失效条件** | 若任意精度在语料级 fixture 上**耗时或内存超预算**（`03` §6.1） ⇒ 引入有界 + 显式错误，并登记为实现限制 |

#### D-M3-5　包模型范围：中等档

| 字段 | 内容 |
|---|---|
| **判据** | 类型核心要**跨文件引用**（`D-M3-1` 与退出条件 5）；路线图 M3 字面含「package identity / 显式源码依赖映射 / 导出符号摘要 / import cycle 诊断」 |
| **候选** | ① 最小（单包多文件、**无** import）② **中等**（+ 源码 import / cycle 诊断 / 导出摘要）③ 完整（+ 稳定摘要格式） |
| **结论** | **②**。单包多文件 + 按显式路径的**源码** import；import cycle 出诊断；产出导出符号摘要。**不**读官方二进制 export data、**不**下载模块 |
| **否决理由** | ① M3 的跨文件引用与退出条件 5 都要求它，省下的会在 M4 第一周加倍还回来；③ M3 用不上版本化 package cache，是过度设计 |
| **失效条件** | 若「依赖路径映射」这套最小约定与 M4 的 `go.mod`/模块语义**冲突** ⇒ 收回到最小档并登记 |

> **依赖路径映射的约定**（本轮先定最小口径，`writing-plans` 细化）：M3 的 import 只按**显式路径**在**调用方给定的根目录集合**内查找同名目录中的 `.go` 文件；不做 `GOPATH`/模块解析、不做 vendor 语义。cycle 诊断为**环上每个包一条**（对齐官方 `types2/cycles.go` 的口径由 `writer` 对撞确认）。

#### D-M3-6　`//go:` 未知指令裁决：前端收下不报错

| 字段 | 内容 |
|---|---|
| **判据** | `F2`：官方 `syntax` 层对 `go:` 指令**只认 `//go:build`**（取版本）并把 pragma 交给回调，**未知指令不报错**；「misplaced compiler directive」由 **`noder`** 报出（syntax **之后**的阶段）。本仓纪律：与官方一致、以 oracle 为准 |
| **候选** | ① 前端报错 ② **前端收下不报错** ③ 前端收下 + 在新阶段裁定 |
| **结论** | **②**（③ 是 ② 的后续动作：裁定归 **noder 等价阶段**，**不进 M3**）。M2 的临时口径「按忽略实现」由此**转正为裁决** |
| **否决理由** | ① 与官方 syntax 分叉，且 ast-oracle 对撞语料里就有 `//go:` 指令 ⇒ 一对撞即红；且 roadmap §M2 的「拒绝未知关键指令」本就不是 syntax 层的职责 |
| **失效条件** | 若用户要求在**前端**提前拒绝未知指令 ⇒ 需在 noder 等价阶段实现并单独登记偏离（不在 M3 前端） |

#### D-M3-7　版本门控归属：类型检查层

| 字段 | 内容 |
|---|---|
| **判据** | `F1`：`syntax.ParseFile(filename, errh, pragh, mode)` **没有** lang 参数；`F3`：语言版本判定全在类型检查器（`Config.GoVersion` + 每文件版本 + `check.allowVersion(...)` 逐点门控） |
| **候选** | ① 在 parser 加 lang 参数 ② **在类型检查层做** ③ 继续 deferred |
| **结论** | **②**。M3 落**门控机制**（`allowVersion` 等价 + 版本比较 + 来源：`-lang` 与 `AstFile.go_version`）；在 M3 真有的门控点生效。**M2 的 `T-A-97` 从 parser 层重定到 sema 层**——它不再是 deferred 漏洞，而是「归属本就不同」 |
| **否决理由** | ① 官方没有这一层，做了就是**自造层**且与 oracle 不可对撞；③ 门控点天然属于类型检查器，等于把 M3 该做的推给 M4 |
| **失效条件** | 若发现官方在 `syntax` 层确有**版本相关**的报错（本轮 grep 未发现） ⇒ 需在 parser 层补 |

#### D-M3-8　crate 划分（我方决定）

| 字段 | 内容 |
|---|---|
| **判据** | 需**可单测**、依赖方向单向、不把「常量失败」与「类型失败」混在一个单测切面里（`D-M1-7` 的教训：混装会让「出错不知在哪一层」） |
| **候选** | ① 单 crate `rgoc-types` 全装 ② **三 crate**：`rgoc-const`（常量 + 大数）+ `rgoc-types`（表示 + identity）+ `rgoc-load`（包模型）③ 拆到更细（universe/scope/oracle 各一） |
| **结论** | **②**。依赖方向：`rgoc-const → rgoc-types → rgoc-load → rgoc-driver`（均单向，无环） |
| **否决理由** | ① 三块失败模式不同、单测切面不同，混装违背 `D-M1-7`；③ M3 量级（约 3–5 千行）用不上，且 crate 边界一多，跨 crate 借用会变紧 |
| **失效条件** | 若 `rgoc-types` 与 `rgoc-const` 出现**循环依赖**（类型需要常量做数组长度，常量又需要类型做 typed 常量） ⇒ 抽出 `rgoc-base` 承载 `Pos`/基础枚举，或合并 |

> **注**：数组长度是常量（`TYP-05`），所以 `rgoc-types → rgoc-const` 是**必需**方向；`rgoc-const` 只依赖 `rgoc-lex` 的 `Pos`（诊断用），不依赖 `rgoc-types` ⇒ 无环。

#### D-M3-9　类型表示的形态（我方决定）

| 字段 | 内容 |
|---|---|
| **判据** | M2 的 **PA-1 纪律**：字段名 / 类型 / **可空性**都要与官方逐字对齐（M2 期间同形缺口出现 4 次）；且类型层是「不报错但会错」的重灾区 |
| **候选** | ① **Rust enum + struct 直译官方** ② 扁平 tag + union ③ trait 家族（object-safe `Type` trait + 各实现） |
| **结论** | **①**。`enum Type { ... }` + 各 struct（`Named`/`Alias`/`Basic`/`Pointer`/…）；**穷尽性检查**是 Rust 相对 Go 的最大杠杆 |
| **否决理由** | ② 丢掉 `match` 的穷尽性检查（把官方「漏一个变体」的错误重新变成可能）；③ trait object 家族对 M3 过度设计，且**难以与官方逐字对账**（PA-1 要求的字段级对齐会散进 trait 方法） |
| **失效条件** | 若 enum 变体数大到触发 `clippy::large_enum_variant` 且 `Box` 化**破坏逐字对齐** ⇒ 局部 `Box` + 带理由的 `#[allow]`（沿用 T-AP15 的处置） |

#### D-M3-10　oracle 形态（我方决定）

| 字段 | 内容 |
|---|---|
| **判据** | M2 用 vendor 官方 `syntax` 做逐字节对撞；类型这层有标准库 `go/types`（官方类型检查器本体）可直接用 |
| **候选** | ① vendor `cmd/compile/internal/types2` ② **`tools/types-oracle` import 标准库 `go/types`，输出中性 dump** ③ 手写期望值 |
| **结论** | **②**。`tools/types-oracle` 对同一份 fixture 包运行 `go/types`，输出**中性类型 dump**（C7 契约）+ identity/可表示性判定，与 rgoc 的 dump 对撞 |
| **否决理由** | ① `types2` 依赖 `internal/syntax` 等大量 internal 包，vendor 面**远大于** M2 的 16 文件；③ 与 `D-M3-3` 的 ① 相同（纯手写期望值） |
| **失效条件** | 若 `go/types` 与 `types2`（编译器实际所用）在判据上出现**分歧** ⇒ 改 vendor `types2`，或对分歧逐条登记 |

#### D-M3-11　「显式拒绝」的形态（我方决定）

| 字段 | 内容 |
|---|---|
| **判据** | M2 纪律：**不用 `None` 冒充未实现**（用 `todo!`）；同时「与官方一致」不能被**静默**破坏——否则「立住了」不可信 |
| **候选** | ① `panic!`/`todo!` ② 静默忽略 ③ **稳定 `DiagCode` + 登记 `unsupported`** |
| **结论** | **③**。对**真实输入**（用户给的 `.go` 文件）能力不足时，出**稳定诊断码**并在 `M3-manifest.json` 的 `unsupported` 登记；**内部不可达分支**仍用 `todo!`（编译期即可定位） |
| **否决理由** | ① 对真实输入 panic 不可接受（M2 已确立「用户可见的未实现必须是诊断」）；② 会让「不一致」隐身，直接摧毁退出条件 2/3 的可信度 |
| **失效条件** | 若确认某类输入**官方也拒绝**（并非仅我方未实现） ⇒ 归入**正常诊断**，**不**进 `unsupported` |

#### D-M3-12　大数实现来源（我方决定）

| 字段 | 内容 |
|---|---|
| **判据** | 本仓红线：「**不默认新增第三方依赖**」；M0 的 `rgoc-hir` spike 虽有 base-2^32 `BigInt`，但**被刻意收敛到 i64 溢出即报错**，且标 **SPIKE-ONLY**（M5 整体替换） |
| **候选** | ① 引入 `num-bigint` / `num-rational` ② **自研**整数 bigint + 有理数 ③ 复用/升级 `rgoc-hir` spike 的 `BigInt` |
| **结论** | **②**。可**借鉴** spike 的结构（进制、归一）作参考，但正式 crate **不依赖** SPIKE-ONLY 产物 |
| **否决理由** | ① 需单独授权，且把依赖面扩大到一个精度敏感的位置；③ 让正式 crate 依赖「M5 会被整体替换」的产物，违反 M0 的隔离约定 |
| **失效条件** | 若自研有理数的**舍入**反复与 spec/oracle 对不齐（向偶数舍入的边界） ⇒ 重新讨论引入 `num-bigint` 的授权 |

### 2.3 事实基线（全部为实测，不是推断）

> 行号基于本仓库当前语料快照（go1.27.1）。**语料一变，全部行号失效**（`AGENTS.md` §5）。

| # | 事实 | 出处 | 对本设计的约束 |
|---|---|---|---|
| F1 | `syntax.ParseFile(filename, errh, pragh, mode)` **无 lang 参数**；语言版本不在 parser | `syntax/syntax.go:84` | `D-M3-7`：门控放类型检查层 |
| F2 | `syntax` 层对 `go:` 只认 `//go:build`（取版本）并转交 `pragh`；**未知指令不报错**；「misplaced compiler directive」由 `noder` 报出 | `syntax/parser.go:77-87`；`noder/noder.go:208,244` | `D-M3-6`：前端收下不报错 |
| F3 | 版本门控在类型检查器：`goVersion` 字符串形如 `go1.%d`（无发布号）；`allowVersion`/`verifyVersionf`；门控点如 `comparable requires go1.20` | `types2/version.go:38-70`；`instantiate.go:313`；`conversions.go:203,216`；`call.go:89,394,560`；`literals.go:19` | `D-M3-7`：机制 + M3 真有的门控点 |
| F4 | 类型同一性入口 `Identical(x, y Type)`；核心实现是按类型形态 `switch` | `types2/api_predicates.go:85`；`predicates.go:261` | 退出条件 2 的判据面 |
| F5 | **interface 即类型集**：`Interface.typeSet()` → `_TypeSet{terms, methods, comparable}`；`IsAll` / `IsMethodSet` / `IsComparable` | `types2/interface.go:16,28`；`typeset.go:26,33,36,39,42`；`termlist.go:14`；`typeterm.go:13` | `D-M3-2`：表示含类型集；判定窄 |
| F6 | 别名：`Alias` 可带 `TypeParams`/`TypeArgs`/`Rhs`（泛型别名，1.24）；`Unalias` 归一 | `types2/alias.go:24,44,61,72,76,83` | 表示含 `Alias`；identity 需 `Unalias` |
| F7 | 常量可表示性：`representableConst(x, check, typ, rounded)`（`check` 可为 `nil`）；typed 常量不满足时报 `constant result is not representable` | `types2/const.go:76`、`const.go:249`、`const.go:27` | `D-M3-4`；退出条件 3 的诊断码 |
| F8 | `comparable` 是 universe 里的**特殊** interface（标记为 comparable），不是普通类型名 | `types2/universe.go:110-112,253` | M3 表示须能表达「预声明的特殊约束」 |
| F9 | 类型图循环检测：`validType0`；别名循环另走 objDecl 循环检测 | `types2/validtype.go:10-20` | §7 资源上限：类型递归深度 + 循环检测 |
| F10 | 版本表常量：`go1_9/13/14/17/18/20/21/22/23/26/27` | `types2/version.go:38-50` | `D-M3-7` 的门控点清单（M3 只用得到其中少数） |
| F11 | 类型检查入口 `checkFiles(files)`；`Config.GoVersion`；每文件版本并存 | `types2/check.go:422`、`check.go:312-326`；`api.go:113-118` | `D-M3-5`/`D-M3-7` 的输入形态 |
| F12 | `rgoc` 侧现状：`rgoc-lex`（`Pos`/`DiagCode` 35 个/`FileId`）、`rgoc-ast`（节点 + `dump.rs`）、`rgoc-parse`（`AstFile.go_version` 已填） | `rgoc/crates/`；`M1-design.md` §3.1；`M2-design.md` §3 | 复用面：位置/诊断/节点；**不重复实现** |
| F13 | **语言版本是「每文件」的**：`versions[file.Pos().FileBase()] = v`；有 `//go:build` 版本时 `v = max(fileVersion, go1_21)`，否则回落 `Config.GoVersion`；「太新」有两处诊断：包级 `package requires newer Go version`、文件级 `file requires newer Go version` | `types2/check.go:312-348` | `D-M3-7`/`D-M3-15`：版本**按文件**存储与判定，我 §5.4 的初稿写浅了 |
| F14 | `representableConst(x, check, typ *Basic, rounded)`：**只吃 `*Basic`**，`check` 可为 `nil`，舍入值 `rounded` 是**出参**；int/uint 判定还需 `Sizes`（`conf.sizeof`） | `types2/const.go:76-99` | `D-M3-16`：`Representable` 可只依赖 `BasicId` + `Sizes` ⇒ 放在 `rgoc-const`，**不产生环** |

### 2.4 关键设计取舍（不由五字段承载，直接采纳）

**「照抄语义、不照抄实现」在类型层的具体含义**（沿用 `M1-design.md` §2.2 的方向性取舍）：

| 照抄 | 不照抄 |
|---|---|
| 判定**顺序**、判定**条件**、边界行为（哪个先判、短路在哪） | `types2` 的指针身份 / `seen map[Type]bool` 缓存机制（改用显式 visited 集合或索引） |
| 类型形态的**字段名 / 字段类型 / 可空性**（PA-1） | Go 的 `Type` 接口方法集到 Rust 的机械映射（用 enum `match` 代替接口 |
| **错误的位置与类别** | 错误的消息文本（`D-M1-9` 沿用） |
| 版本门控的**门控点与版本号** | `go/version` 包的字符串比较实现 |

**判定 / 表示的分工表**（`D-M3-2` 的可执行摘要）：

| 功能 ID | M3 做什么 | M3 不做什么 |
|---|---|---|
| `CON-01` 无类型常量体系 | ✅ 表示 + 默认类型 | — |
| `CON-02` 常量精度 | ✅ 任意精度实现 | — |
| `CON-03` 隐式转换规则 | ✅ 常量侧的隐式转换（无类型→typed） | 表达式混合操作（M4） |
| `CON-04` 可表示性 | ✅ `Representable` 判定 + 溢出诊断 | — |
| `CON-05` 常量表达式折叠 | ✅ 算术/比较/逻辑/移位/转换（作用于**常量**） | 运行期折叠（M5/M6） |
| `CON-06` iota | ✅ 组内递增 | — |
| `CON-07` 常量声明 | ✅ 表示 + 重复表达式 | 作用域外的语义（M4） |
| `CON-08` 移位计数 | ✅ 无符号语义 + 非负检查 | — |
| `TYP-01..12,14..17,20..22` | ✅ **表示** + `TYP-16/17` **判定** | （见下） |
| `TYP-11` 通用接口 | ✅ **表示**类型集/项集/`~T` | ❌ 类型集**判定**（M4） |
| `TYP-13` 接口实现 | ❌ | 无消费者（M4） |
| `TYP-18` 可赋值性 | ❌ | 无消费者（M4） |
| `TYP-19` 方法集 | ❌ | 实现判定前置（M4） |
| `TYP-22` 类型参数声明 | ✅ **表示** + 作用域绑定 | ❌ 约束满足（M4） |
| `TYP-23..26` 约束/实例化/推断/统一 | ❌ | M4 |
| `SCP-01..10` | ✅ 全量（含 `SCP-06` 自有测试——9 缺口之一） | — |
| `PIPE-06` 导入解析 | ✅ **源码** import + cycle | ❌ 二进制 export data / 模块下载 |
| `PIPE-07/08/09` | ✅ 类型检查器骨架 / 常量折叠引擎 / 作用域解析 | ❌ 表达式语句检查（M4） |


### 2.5 头脑风暴评审后的补充条目（**我方决定**）

> 本节由设计文档的**独立 builder check** 触发：评审指出的「会猜错就影响计划」之处，能由设计者裁定的在此落定。
> 只列**结论**与**失效条件**（判据见正文引用处）。其中 `D-M3-14`（import 面）与 `D-M3-19`（常量表达式求值面）是最可能被推翻的两条，已单独提请用户确认 —— **两条均确认保持**（2026-10-16）。

| ID | 结论 | 失效条件 |
|---|---|---|
| **D-M3-13** | **约束接口的同一性明确排除**：M3 的 `Interface` 同一性是**方法集版**，对含 `~T`/并集的接口**不给出判定**（不是「给个错的」）。对撞 fixture **不含**此类成对比较；排除项进 `unsupported`。这是 `D-M3-2`（用户已选「判定做窄」）的直接后果，**不是新决策** | 若 M4 前的任何验收需要它 ⇒ 提前到 M3 实现类型集版同一性 |
| **D-M3-14** | **loader 的文件选择＝显式文件列表**：M3 **不评估构建约束**（不读 `//go:build` 做文件筛选）、不处理 `_test.go` / cgo / 汇编。由调用方给「这个包由哪些文件构成」。**冒烟语料**改为：自建无 import 的多文件包 + 语料里**真正不 import 任何东西**的单包（§8.3 的「标准库单包」口径**撤回**——标准库包普遍 import `internal/*`） | 若 M4 的包模型需要构建约束筛选 ⇒ 在 M4 实现并登记 |
| **D-M3-15** | **版本按文件存储**（对齐 `F13`）：`SourceMap`/检查器持有 `file base → version` 映射；`allow_version` 取**该文件**的版本；两处「太新」诊断（包级/文件级）按官方分别出。版本来源优先级：`-lang` > `//go:build` 文件版本（`max(v, go1.21)`） > 回落 |
| **D-M3-16** | **判定 API 的落位**（解环）：`Representable(c: &Const, b: BasicId, sizes: &Sizes) -> Result<Rounded, DiagCode>` 放在 **`rgoc-const`**（对齐 `F14`：官方只吃 `*Basic`）；**typed 常量**（`Const` + `TypeId`）与「取 `Underlying` 的 `Basic`」这一步放在 **`rgoc-types`**。⇒ `rgoc-const` 不依赖 `rgoc-types`，`D-M3-8` 的环风险**消除** |
| **D-M3-17** | **对撞成对的编址**：identity / 可表示性的**成对样本不由 oracle 侧生成**，而是**写在 fixture 源码里**（机器可读指令注释，形如 `//rgoc:cmp identical <expr-a> <expr-b>`、`//rgoc:cmp representable <const-expr> <type>`）。两侧读**同一个 fixture** ⇒ 编址天然一致，无需协商编号方案 | 若表达式无法在注释里唯一引用（例如需要跨文件） ⇒ 改为「按声明序下标」并在 C7 里冻结 |
| **D-M3-18** | **「导出符号摘要」的最小形态**：M3 只提供 `rgoc-load` 的**内存 API**（`ExportSummary`：导出对象名 → 简化签名/类型 dump 行）供 M4 消费；**不**冻结文本格式、**不**做磁盘缓存（`D-M3-5` 已排除「完整档」）。若 M4 需要版本化格式，届时按 C 系列契约流程新建 | 若 M4 的包模型需要二进制/版本化摘要 ⇒ 在 M4 立契约 |
| **D-M3-19** | **`const` 初始化式的能力边界**：M3 **带一个受限的常量表达式求值器**（`CON-05` 的常量子集：字面量、标识符引用到常量、算术/比较/逻辑/移位/转换），并对**不是常量表达式**的初始化式出**稳定诊断**（`D-M3-11` 的路径）。**不**做类型检查意义上的完整表达式检查（那是 M4） | 若受限求值器在语料上频繁撞到「需要类型信息才能判」的表达式 ⇒ 与用户重估 M3/M4 边界（见 §1.2） |

---

## 3. 数据模型（类型表示）

### 3.1 `Type` 枚举（"表示做宽"的落点）

对齐官方 `Type` 接口的实现类（`types2/*.go`），Rust 侧为 **enum + struct**（`D-M3-9`）：

```text
enum Type {
    Basic(BasicId),                 // 预声明基础类型；含 Untyped*（无类型常量）
    Named(NamedId),                 // 具名类型（含实例化：origin + targs）
    Alias(AliasId),                 // 别名（可带 tparams/targs/Rhs，1.24 泛型别名）
    Pointer(TypeId),
    Slice(TypeId),
    Array { len: ConstId, elem: TypeId },   // 长度是常量（TYP-05）⇒ 依赖 rgoc-const
    Map { key: TypeId, elem: TypeId },
    Chan { dir: ChanDir, elem: TypeId },
    Struct(StructId),               // 字段名/类型/Tag/嵌入
    Signature(SignatureId),         // 参数/结果元组 + 变参标记
    Tuple(TupleId),
    Interface(InterfaceId),         // 方法集 + （M3 表示的）类型集
    TypeParam(TypeParamId),         // TYP-22：表示，不判定
    Union(UnionId),                 // TYP-11：项集（term = T | ~T）
    Invalid,                        // 对齐官方 Typ[Invalid]
}
```

> **注**：`Id` 一律是**表内下标**（`TypeId`/`NamedId`/…），不是指针或 `Rc` —— 理由与 `M1-design.md` `D-M1-5` 同源（`Type` 要能 `Copy`、要能 `Ord`/`Hash`；指针方案会把生命周期传染到 M4/M5）。

### 3.2 类型集（M3 只表示）

```text
TypeSet { terms: Option<TermList>, methods: Vec<Method>, comparable: bool }
Term    { tilde: bool, typ: TypeId }     // ~T 与 T
```

`Option<TermList>` 对应官方 `termlist` 的「nil = 全部」（`F5` 的 `isAll`）。**M3 只构造、只 dump，不做「并集归一 / 重叠判定 / 满足性」**（M4）。

### 3.3 常量（`rgoc-const`）

```text
Const {
    Int   (BigInt),            // 任意精度；rune 亦走此路（对齐 iota 的整数性）
    Float (BigRat),            // 精确有理数（D-M3-4）
    Complex { re: BigRat, im: BigRat },
    Str   (Vec<u8>),           // 字节（沿用 M1 D-M1-12：String 装不下非法 UTF-8）
    Bool  (bool),
    Unknown,                   // 求值失败/未定（对齐 const.Unknown，非 panic）
}
```

**不变式**：`Const` 永远**精确**；「是否可表示」永远是**导出判定**——`Representable(&Const, BasicId, &Sizes) -> Result<Rounded, DiagCode>`（`D-M3-16`）。**不**把舍入结果写回 `Const`：舍入值像官方那样作**出参/返回值**单独给出（`F14`）。

### 3.4 作用域与对象（`rgoc-load` / `rgoc-types`）

```text
Scope { parent: Option<ScopeId>, elems: map[Name]ObjectId, kind: ScopeKind }
Object { kind, name: String, typ: Option<TypeId>, pos: Pos, ... }   // 变量/常量/类型名/函数/包名/标签
universe  ⊃  package  ⊃  file  ⊃  function  ⊃  block     (SCP-01)
```

`Object` 的 `kind` 与字段对齐官方 `types2/object.go`（PA-1：字段名/类型/可空性逐字对齐）。

---

## 4. 模块结构与依赖方向

```text
rgoc-lex ──► rgoc-ast ──► rgoc-parse          （M1/M2 已落地）
     │            │            │
     │            │            └────────────┐
     ▼            ▼                         ▼
rgoc-const ──► rgoc-types ──► rgoc-load ──► rgoc-driver
（常量+大数）   （表示+identity） （包模型）    （新增 `rgoc types` 子命令）
```

| crate | 职责 | 依赖 | 新增？ |
|---|---|---|---|
| `rgoc-const` | 大数（`BigInt`/`BigRat`）+ `Const` + 折叠（`CON-05`）+ `Representable`（`CON-04`，只吃 `BasicId`+`Sizes`，`D-M3-16`） | `rgoc-lex`（`Pos`） | ✅ 新 |
| `rgoc-types` | `Type` 表示 / universe / `Identical` / 版本门控机制（按文件，`D-M3-15`） | `rgoc-lex`、`rgoc-const`、`rgoc-ast` | ✅ 新 |
| `rgoc-load` | 单包多文件 + 源码 import + cycle 诊断 + 导出摘要（内存 API，`D-M3-18`） | `rgoc-parse`、`rgoc-types` | ✅ 新 |
| `rgoc-driver` | 新增 `rgoc types <files…>`：出中性 dump / 诊断（供门禁与手工验证） | 全部 | 改 |

> **不做**：不把类型检查塞进 `rgoc-parse`（M2 的 parser 只产树，不产类型）；不新建 SPIKE-ONLY crate（那是 M0 的机制）。

---

## 5. 数据流与算法

### 5.1 包加载（`rgoc-load`）

```text
[文件列表] ──parse──► [AstFile] ──声明收集──► [包作用域 + 文件作用域]
                                        │
                                        ├─ import 解析（源码路径映射，D-M3-5）
                                        ├─ import cycle 检测 ──► 诊断
                                        └─ 导出符号摘要
```

顺序**照官方**：先建 universe → 再声明收集（declaration collection）→ 再类型求值（type resolution，可能触发具名类型的延迟求值）。

**文件选择（`D-M3-14`）**：M3 **不评估构建约束**——不读 `//go:build` 筛选文件、不处理 `_test.go` / cgo / 汇编；由**调用方给显式文件列表**（`rgoc types a.go b.go c.go`）。「导出符号摘要」= `ExportSummary` 内存 API（`D-M3-18`），不落盘。

### 5.2 类型同一性（`Identical`）

按官方 `predicates.go` 的**分派顺序**逐形态判定：

| 形态 | 判据（对齐 `TYP-17`） |
|---|---|
| `Basic` | 同一 `BasicId` |
| `Named` | 同一 `NamedId`（**含** origin + 类型实参逐一同一；M3 无实例化 ⇒ 退化为同一 Id） |
| `Alias` | `Unalias` 后判定（`F6`） |
| `Pointer/Slice` | 元素同一 |
| `Array` | 长度**常量相等** **且** 元素同一 |
| `Map` | 键、值都同一 |
| `Chan` | 方向一致 **且** 元素同一 |
| `Struct` | 字段数相同；逐字段名、类型、**Tag** 同一（嵌入标记一致） |
| `Signature` | 参数/结果元组同一；**变参标记**一致 |
| `Interface` | **M3 暂按方法集**：方法数相同、逐个方法名 + 签名同一（**⚠️ 类型集版同一性 M4 到**，见 `D-M3-2`） |
| `TypeParam` | 同一 `TypeParamId`（判定的**边界**：不比较约束） |
| `Union` | 项集逐一同一（顺序无关） |

> **⚠️ 已知局限（`D-M3-13`，必须写清以免误读为「无害」）**：M3 的 `Interface` 同一性是**方法集版**，而 `go/types` 的 `Identical` 用的是**真类型集**。于是 `interface{~int}`、`interface{~string}`、`interface{}` 在 rgoc 侧会「都相等」、在 oracle 侧「都不同」——**这是静默错答，不是无害**。
> 处置（`D-M3-2` 的直接后果，不是新决策）：**M3 对含 `~T`/并集的接口不给出同一性判定**（显式拒绝，`D-M3-11`），对撞 fixture **不含**此类成对比较，排除项进 `unsupported`。M4 必须实现类型集版同一性并解除该排除。

### 5.3 常量可表示性（`Representable`）

对齐官方 `const.go:76 representableConst(x, check, typ *Basic, rounded)`（`F7`/`F14`）的**判定顺序**，签名见 `D-M3-16`：

1. 由 `rgoc-types` 侧取目标类型的 `Underlying` 的 `Basic`，把 `BasicId` 交给 `rgoc-const`（保持单向依赖）。
2. 按 `Const` 形态与 `Basic` 的分类做区间/精度判定（整数：范围；浮点：**精确有理数**算完后按**向偶数舍入**判是否无损；复数：实虚部各自判定）；`Sizes` 参与 int/uint 判定（`F14`）。
3. 失败 ⇒ 出**稳定诊断码**（对齐 `constant result is not representable` 的**类别**，不复刻文本），**不**改 `Const`；成功 ⇒ 返回舍入值（对齐官方的出参语义）。

### 5.4 版本门控

对齐官方 `check.go:312-348`（`F13`），**版本是「每文件」的**：

```text
版本来源（优先级）：显式 -lang（整包）  >  该文件 //go:build 的版本  >  未指定（放行）
  文件有版本时：v = max(fileVersion, go1.21)        （官方 check.go:336 的兼容规则）
  文件无版本时：v = Config.GoVersion（= -lang）
存储：file base ──► version   （对齐 check.go:347）
判定：allow_version(want) := v 无效 || v cmp(want) >= 0     （F3 / F10）
```

**两处「太新」诊断**（类别分开，位置按官方）：包级 `package requires newer Go version`（`check.go:314`，位置取 `files[0]`）、文件级 `file requires newer Go version`（`check.go:344`，位置取该文件的 `package` 名，官方注释注明是为 types/types2 一致性）。

**M3 的门控点范围**：M3 按官方**照做**两处 `TooNew` 诊断；M3 真正会碰到的语言特性门控点（如「类型参数需要 go1.18」）由 `writing-plans` 逐点列举，与 `D-M3-2` 的「显式拒绝」走同一条路径。**不做**的是 M4 才有的门控点集合（可赋值性/可比较性/转换相关的版本规则）。

### 5.5 中性类型 dump（C7，对撞面）

对齐 C6（AST dump）的做法：**行式、规范化、无位置**（位置另走诊断通道）。每行一个类型，形态为「分类前缀 + 规范化字段」，`Named`/`Alias` 用**表内序号**而非名字（避免两侧名字表的差异污染对撞）。

**成对样本的编址**：见 `D-M3-17` 与 §9——fixture 源码里带 `//rgoc:cmp` 指令注释，两侧读同一份 fixture，无需协商编号。

---

## 6. 覆盖清单（功能 ID → M3 归属）

| 组 | ID | M3 | 说明 |
|---|---|---|---|
| 常量 | `CON-01..08` | ✅ | 全量；`CON-04/05/08` 是判定重点 |
| 类型 | `TYP-01..12,14..17,20..22` | ✅ 表示（+`16/17` 判定） | `TYP-11` 只表示类型集 |
| 类型 | `TYP-13,18,19` | ❌ → **M4** | 无消费者；**偏离**（`D-M3-2`） |
| 类型 | `TYP-23..26` | ❌ → **M4** | `D-M3-2` |
| 作用域 | `SCP-01..10` | ✅ | **`SCP-06`（导出标识符）须补自有测试**（9 缺口之一） |
| 管道 | `PIPE-06` | ✅ **源码**档 | 二进制 export data / 模块下载 → M4+；**不评估构建约束**（`D-M3-14`） |
| 管道 | `PIPE-07,08,09` | ✅ | 类型检查器骨架 / 常量折叠 / 作用域解析 |

---

## 7. 错误处理与资源上限

### 7.1 诊断

- 沿用 C1：**位置 + `DiagCode`**，不复刻官方消息文本（`D-M1-9`）。
- `DiagCode` 扩展遵守 **五件套**纪律（`M2` 硬事实）：枚举声明**按字典序落位** + `ALL` 数组 + `as_str` + **两个总数测试**（防止「声明序 = 字典序」被破坏）。
- M3 新增的类别（暂名，`writing-plans` 定稿）：不可表示常量、类型图循环、import cycle、未定义标识符、重复声明、版本不允许的特性、**M3 显式拒绝**（`D-M3-11`）。

### 7.2 资源上限（全部登记为实现限制）

| 上限 | 值 | 理由 |
|---|---|---|
| 类型图递归深度 | 与 parser 一致（`MAX_NEST_DEPTH = 1000`，M2） | 防深嵌套类型栈溢出；超限**恰好 1 条**诊断 |
| 类型图循环 | 检测并报（`F9`） | `type A A` / `type A = [10]A` 类 |
| import 深度 / cycle | cycle 报诊断；深度设硬上限 | 防相互 import 死循环 |
| 常量大小 | **不设人为上限**（`D-M3-4`） | 但受 §7.3 的预算约束 |

### 7.3 预算

沿用 `03` §6.1：**smoke ≤ 5 min**、**milestone ≤ 30 min**、full ≤ 2 h。M3 的类型对撞语料规模由 `M3-tests.md` 冻结。

---

## 8. 测试策略

### 8.1 四条腿（沿用 M2 §8.1）

| 腿 | 内容 | 抓什么 |
|---|---|---|
| ① **crate 内单测** | `Identical` / `Representable` / 作用域解析的逐条判定 | 定位到具体函数 |
| ② **golden（自有 fixture）** | 照 spec 手写期望值（类型 dump + identity 判定 + 可表示性） | 锚 spec 文本；抓 oracle 也错的形态 |
| ③ **oracle 对撞** | `tools/types-oracle`（`go/types`）与 rgoc 的中性 dump 逐字节对撞 | 抓「自己写的期望也错」 |
| ④ **拒绝侧双向断言** | 正向：该拒的**确实**报稳定码；反向：不该拒的**不**报 | 防「只测一边」的假绿（M2 已抓 6+ 恒真断言） |

### 8.2 反向验证纪律（沿用并强化）

每条断言都必须**反向验证真能失败**：变异 → 逐字节 md5 复原 → 核 `Compiling`（`check`/`clippy` 是 `Checking`）。
**「照不出」的探针如实登记，不硬凑**（硬凑只会给「全绿」假象）。

### 8.3 语料

| 来源 | 用途 |
|---|---|
| 自建 fixture（`.go` + `.want` dump + `//rgoc:cmp` 指令） | 腿 ②③ 的主语料（`D-M3-17`） |
| `types2` / `go/types` 的 `testdata/` 精选 | 腿 ③ 的**扩大**语料（按 M3 支持面筛选；约束接口同一性等排除项进白名单） |
| 语料中**真正不 import 任何东西**的单包 | 冒烟：能装载、不崩、诊断数可解释 |

> **⚠️ 撤回**：初稿写的「标准库单包（无跨包依赖者）」口径**不成立**——标准库包普遍 import `internal/*`。按 `D-M3-14` 改为「自建无 import 的多文件包 + 语料里真正零 import 的单包」。

---

## 9. oracle adapter（`tools/types-oracle`）

对齐 M2 的 `tools/ast-oracle`（`D-M3-10`）：

- **形态**（已定，取代初稿的「待定」）：Go 程序，用 **`go/parser`** 产 `ast.File`（**带注释**，以便读 fixture 里的 `//rgoc:cmp` 指令），再喂 **`go/types.Config.Check`**。**不** vendor `internal/*`。
- **成对样本的编址**（`D-M3-17`）：**不由 oracle 生成**。fixture 源码里写机器可读注释，例如
  ```go
  //rgoc:cmp identical  A  B
  //rgoc:cmp representable  1<<100  int8
  ```
  两侧读**同一个 fixture** ⇒ 编址天然一致，无需协商编号方案（失效时退回「按声明序下标」并在 C7 冻结）。
- **输出**：① 中性类型 dump（C7）② identity 判定（成对类型）③ 可表示性判定（常量 × 目标类型）。
- **确定性**：同一输入多次运行输出 sha256 一致（沿用 ast-oracle 的验收判据）。
- **与 rgoc 的差异面**：位置、诊断文本一律**不进对撞**（只对撞 dump 与判定）；**约束接口的同一性不进对撞**（`D-M3-13`）。
- **构建**：容器内 `go build` 需 `-buildvcs=false`（M2 已踩，进 MEMORY）。

---

## 10. 契约与产物影响

| 契约 | 影响 |
|---|---|
| **C7（新建）** | **类型 dump 契约**：行式、规范化、无位置；`Named`/`Alias` 用表内序号。冻结后，oracle 与 rgoc 两侧按它对齐（对齐 C6 的角色） |
| C1 | 若 M3 新增位置语义（类型位置取 `Expr::pos()` 还是 `StartPos`——M2 已踩过右偏一列）⇒ 同步 C1 |
| C2（Test IR） | M3 的新判定（identity/可表示性）需新增**判定分类**⇒ 同步 C2 |
| C3/C4/C5 | 无影响（HIR/SSA/ABI 在 M5+） |

`M3-manifest.json` 的 `unsupported` 须如实登记 `D-M3-2`（不做 `TYP-13/18/19/23..26`）、`D-M3-5`（不做二进制 export data）、`D-M3-13`（约束接口同一性）、`D-M3-14`（不做构建约束筛选）以及 `D-M3-11` 的所有显式拒绝项。

---

## 11. M3 门禁汇总（暂名 `TY-1..TY-8`，命名待 `M3-tests.md` 定稿）

| # | 门禁 | 判据 |
|---|---|---|
| TY-1 | 类型表示穷尽性 | `Type` 变体 ↔ 官方实现类一一对应，有穷尽性断言 |
| TY-2 | 同一性正确 | 自有 fixture + oracle 对撞全绿（覆盖别名/具名/底层/函数/结构体/通道）；**对撞集不含约束接口的成对比较**（`D-M3-13`） |
| TY-3 | 常量精度 | 长字面量精确；不静默截断；可表示性逐条与 oracle 一致 |
| TY-4 | 版本门控 | 机制落地 + M3 门控点生效（正反例） |
| TY-5 | 包模型 | 多文件跨文件引用 + 源码 import + cycle 诊断 + 导出摘要 |
| TY-6 | 拒绝侧双向 | 显式拒绝项**确实**报稳定码；未拒绝项**不**报 |
| TY-7 | 四条 cargo 门禁 | fmt / check / clippy `-D warnings` / test `--no-fail-fast` |
| TY-8 | 既有自检不回归 | M0（231）/ M1 / M2 自检全过 |

---

## 12. 风险与未决

| # | 风险 / 未决 | 影响 | 处置 |
|---|---|---|---|
| R1 | **`go/types` 与 `types2` 判据分歧** | 对撞可能「对不上但两边都对」 | `D-M3-10` 失效条件：逐条登记或改 vendor `types2` |
| R2 | **interface 同一性在 M3 不完整**（按方法集） | M4 必须升级，否则约束接口判等错 | 已登记 `D-M3-2`；M4 设计文档**必须**接手 |
| R3 | **`go/types` 需 importer** | 单包 fixture 可能必须真实 importer | `D-M3-3` 失效条件：退回自有 fixture + spec 摘录并登记 |
| R4 | **自研有理数的舍入边界**（向偶数舍入） | 可表示性在边界上判错且不报错 | `D-M3-12` 失效条件：讨论引入 `num-bigint` 授权 |
| R5 | **偏离 roadmap 字面**（`TYP-13/18` 移出 M3） | 需在 M4 补，且 M3 的「类型底座」声明须写明边界 | 已在 §1.2 / `D-M3-2` 显式登记 |
| R6 | 结转缺口：非法 UTF-8 dump 分叉 / `Pos::Ord` 绝对坐标 / 消息逐字对撞 | M3 若触碰则暴露 | M2 `open_gaps` 结转，M3 评估是否处理（**默认不处理**） |
| R7 | **常量求值器的能力边界**（`D-M3-19`） | 若语料频繁撞到「需类型信息才能判」的 `const` 初始化式，M3 的拒绝面会大到不可接受 | `D-M3-19` 失效条件：与用户重估 M3/M4 边界（这是**最可能被推翻**的一条，已提请确认） |
| R8 | **对撞集排除约束接口**（`D-M3-13`） | M3 的同一性对撞比 oracle 窄；若不写明会被误读为「全覆盖」 | 已在 `unsupported` 登记，且 TY-2 判据显式收窄 |
| R9 | **loader 不评估构建约束**（`D-M3-14`） | 真实多平台包可能装载出与 `go build` 不同的文件集 | `D-M3-14` 失效条件：M4 实现并登记；M3 冒烟只用零 import 包 |

---

## 13. 与后续文档的接口

### 13.1 交给 `writing-plans` 的输入

- **交付物**：三个新 crate（`rgoc-const`/`rgoc-types`/`rgoc-load`）+ `rgoc-driver types` + `tools/types-oracle` + **C7**。
- **决策**：§2 十九条（`D-M3-1..12` 头脑风暴产出 + `D-M3-13..19` 评审后补充）。
- **边界**：§1.2 的「不做」表与 §2.4 的判定/表示分工表。
- **验收**：§11 的 `TY-1..TY-8`。

### 13.2 `M3-tests.md` 必须覆盖

- 类型 dump（C7）的**冻结 golden**；identity 判定表；可表示性判定表；
- 四条腿的证据映射；**分母**（语料规模）；
- `unsupported` 白名单（= §1.2 的「不做」表）；
- **拒绝侧的双向断言**清单。

### 13.3 与后续阶段的衔接

| 下游 | M3 必须留下的东西 |
|---|---|
| **M4（静态检查闭环）** | 类型表示（含类型集）**不再迁移**；M4 只加判定：`TYP-13/18/19/23..26`、**类型集版 interface 同一性**（R2） |
| M5（HIR/解释器） | `Const` 的精确值、`Type` 的可判等表示 |
| M9（typedesc/itab） | 具名类型/接口的方法集（M4 补全后） |

---

## 14. 变更记录

| 日期 | 变更 | 作者 |
|---|---|---|
| 2026-10-16 | 初稿：头脑风暴产出（`D-M3-1..7` 用户决策 + `D-M3-8..12` 我方决定） | brainstorm |
| 2026-10-16 | 独立 builder check 后修订：新增 §2.5（`D-M3-13..19`）；补 `F13`（版本按文件）/`F14`（`representableConst` 签名）；修正 §5.2「无害」的误述、§5.4 版本门控、§5.3 判定签名、§8.3 冒烟口径（撤回）、§9 oracle 形态与编址 | brainstorm |
| 2026-10-16 | **用户确认**：`D-M3-14` / `D-M3-19` 保持裁法；状态置 **已确认**，进入 `writing-plans` | 用户 |
