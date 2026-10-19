# M4 测试先行清单：静态检查闭环

> **阶段 ID**：`M4`　|　**状态**：**已冻结**（2026-10-18）　|　**冻结触发**：`M4-design.md` 已确认（用户 2026-10-18「确认，继续」）
> **文档索引**：[`../README.md`](../README.md)　|　**上游**：[`M4-design.md`](./M4-design.md)（本清单只兑现它，不改它）　|　**下游**：`M4-plan.md`（任务级计划）→ `M4-manifest.json`
>
> **本清单的性质**：`AGENTS.md` §4.3 硬约束 —— **开工前必须产出**。它**冻结**的是「怎么算通过」：
> 覆盖矩阵、测试 ID、类别枚举、oracle/对撞口径、语料三清单、`unsupported` 白名单、门禁判据。
> **不写实现顺序**（那是 `M4-plan.md`）。
>
> **冻结规则**：本文件一经冻结，**任何**增删测试 ID、改判据口径、放宽白名单/语料清单，都必须**走修订**
> （本文件 §12 修订表 + manifest 同步），不得静默漂移。

---

## 0. 冻结声明

| 项 | 值 |
|---|---|
| 分母判据 | **测试 ID 数**：本文件共 **63** 个 `T-CK-*`（甲 37 / 乙 5 / 丙 8 / 丁 5 / 反 8） |
| 冻结日 | 2026-10-18 |
| 白名单 | §7，**开工前冻结**；跳过项一律**不得**记为 pass |
| 门禁 | §8，`CH-1..CH-9`（设计 `§1.4` 的定稿版） |
| 语料三清单 | §6：`check-selected.txt`（**75**）+ `check-allowlist.txt`（**16**）+ `check-fixedbugs-waveA.txt`（**50**）—— 同日冻结 |
| 判据来源 | §1，三种：`O` / `S` / `H` |
| 类别枚举 | §4，**v1 冻结（26 类）**；两侧映射纪律与 `other` 守卫同节 |
| 渲染判据结构 | §5（字节格式由 `C8` 契约在首个任务冻结，但**不得**弱于本节） |

**修订表**（追加式，不改历史行）：

| 修订 | 日期 | 内容 | 触发 |
|---|---|---|---|
| `R-6-1` | 2026-10-18 | 设计 `§10 #5` 的未决项落地：官方语义复扫下 **7/8** 个含语法错误文件为 EQ ⇒ **纳入入选**（仅 `expr3.go` 排除）；随之引入 `parse` 类别（§4）与解析面行对齐的新风险（§6 末） | 选料实测（不改变任何 `D-M4-*`） |
| `R-3-1` | 2026-10-18 | `T-CK-62` 出处引用勘误：`F2`⑦ ⇒ **`F11`**（文件版本语义）；判据不变 | 设计复审（m3） |
| `R-5-1` | 2026-10-18 | §5.1 判据收束：腿①**操作口径 = verdict-only**（官方全量消耗语义不采用）；过滤清单补 **Cheap-trick 抑制**（`F2`⑧）；§3.2 `T-CK-84/85` 增输入语法指针（`M3-tests.md` §5.2 `R-32-2`） | 设计复审（B2/M1/M5 对齐） |
| `R-8-1` | 2026-10-18 | §8 `CH-8` 形态计数勘误：四 ⇒ **五**（与 §9.2 对齐；`--check --list` 为独立形态）；判据不变 | 设计复审（M4 对齐） |
| `R-4-1` | 2026-10-19 | §4.2 表「`Other` 不出现在投影表」按**「（除修订）」**口修订：rgoc 侧 `Other` 值域 = 恰 **4 个无官方语义码**（`ChkUnimplemented` 骨架占位 + `ExceedsFileLimit`/`SynNestingTooDeep`/`TypNestingTooDeep` 实现限制）；由 `T-CK-16`（`rgoc-check/tests/category.rs`）精确钉值域（第 5 码入 `Other` 即红）；其余 **63** 码全落真实类别；入选语料出现 `other` 即红（`T-CK-94`）不变 | `T-CK-16` 落地实测（边界码无真实类别可投影；硬塞真实类别 = 「答错」） |
| `R-4-2` | 2026-10-19 | Task 20 检查会话真身落地：骨架占位码 `ChkUnimplemented` 按五件套纪律（无触发者不许进枚举）移除 ⇒ 码表 **72 → 71**（`rgoc-lex`/`rgoc-parse`/`rgoc-check` 计数断言同步）；`R-4-1` 的 `Other` 值域由 4 收窄为 **3**（仅实现限制码；`T-CK-16` 快照 `("other", 3)` + 边界数组三项）；`CHK-` 前缀暂离 `rgoc-lex` 白名单（反向断言「白名单前缀必须有代号在用」照纪律执行） | Task 20 落地实测（红 `72 vs 71` → 绿；driver `check_diag_dump_与契约want_逐字节一致` 上线） |
| `R-4-3` | 2026-10-19 | Task 21 表达式运算符面落地：新增 `CHK-INVALID-OPERATION`（类别 `op-invalid`）⇒ 码表 **71 → 72**（`rgoc-lex`/`rgoc-parse`/`rgoc-check` 三处计数断言同步）；`CHK-` 前缀**回到** `rgoc-lex` 白名单（`R-4-2` 的「暂离」条款失效）；其余拒绝复用 M3 码（`TYP-{DIV-BY-ZERO,INVALID-SHIFT-COUNT,INVALID-SHIFT-OPERAND,INVALID-CONVERSION,NUMERIC-OVERFLOW}`），按 oracle `--check` 对 53 行逐字节对账 | `T-CK-32` 落地实测（红：缺码 + 签名；绿：53 行 + 接受向量零诊断） |
| `R-4-4` | 2026-10-19 | Task 23 索引/切片面落地（前半）：新增 `CHK-INVALID-INDEX`（类别 `index`；官方 `NonIndexableOperand` 与 `InvalidIndex` 三条 —— `must be integer` / `must not be negative` / `out of bounds [0:n]` —— 的合并码）与 `CHK-INVALID-SLICE`（类别 `slice`；官方 `NonSliceableOperand`（`cannot slice`）/ `InvalidSliceExpr`（`3-index slice of string`）/ `SwappedSliceIndices` 的合并码）⇒ 码表 **72 → 74**（三处计数断言同步；`index`/`slice` 各类首码）；索引下标的无类型常量转换复用 `convert_untyped`（`a[1.5]`/`a[1e100]` ⇒ `truncated to int`、`sl["x"]` ⇒ `cannot convert ... to type int` —— `convert` 类），类型化常量超 `int` 范围（`a[u64]`，`1<<63`）⇒ `overflows int` 落 **`overflow`**（c8Rules 的 index 规则不匹配、通用 overflow 规则接管 —— 探针实证）；map 键面 `cannot use ... in map index`（`assign` 类）归 Task 25 的赋值面 | `T-CK-34`（前半）落地实测（红：0/19；绿：19 行 + 接受向量零诊断） |
| `R-4-5` | 2026-10-19 | Task 24 选择器面落地（后半）：新增 `CHK-INVALID-SELECTOR`（类别 `selector`；官方 `MissingFieldOrMethod`（`... undefined (type ...)` 句式）/ `AmbiguousSelector` / `InvalidMethodExpr` / `NonFieldSelector`（`operand for field selector`）四条消息的合并码）⇒ 码表 **74 → 75**（三处计数断言同步；`selector` 类首码）；查找 = 官方 `lookupFieldOrMethod` 逐深度 BFS（`seen` 遮蔽 / 同级碰撞 / 具名指针类型特例），方法值去接收者、方法表达式接收者作首参；`*T` 的 typexpr 臂接通（`(*T).M` 可达）；`pkg.Name` 与泛型接收者面不报 | `T-CK-34`（后半）落地实测（红：0/7；绿：7 行 + 接受向量零诊断） |
| `R-4-6` | 2026-10-19 | Task 25 语句面落地：新增 **11** 个检查码 ⇒ 码表 **75 → 86**（三处计数断言同步）—— `CHK-INVALID-ASSIGN`（`assign`；`cannot use ... as ... value` / `assignment mismatch` / `cannot assign to` / 实参个数与逐参的合并码）、`CHK-WRONG-RESULT-COUNT`（`ret`）、`CHK-MISSING-RETURN`（`terminate`，报在体右花括号）、`CHK-NO-NEW-VAR`（`decl`，soft）、`CHK-SINGLE-VALUE-CONTEXT`（`comma-ok`）、`CHK-UNUSED-EXPR`（`unused`）、`CHK-NOT-A-TYPE`（`not-type`）、`CHK-INVALID-COND`/`CHK-INVALID-CASE`/`CHK-INVALID-GO-DEFER`/`CHK-INVALID-SELECT-CASE`（`stmt`）；逐元素赋值的报点在**右值**（官方 `assignment` 的 `x.pos`） | `T-CK-35` 落地实测（红：2/35 —— 只剩装载面两条 `SYN-BRANCH-FALLTHROUGH`；绿：35 行 + 接受向量零诊断） |

---

## 1. 判据的三种来源

沿用 M1/M3 的记号，全库一致：

| 记号 | 含义 | 用途 |
|---|---|---|
| **`O`** | **oracle 对撞**：`tools/types-oracle`（`go/parser` + `go/types`）产出，或官方语料标记（marker）按 **verdict-only** 口径判定（`^ ERRORx? ` 存在性；§5.1） | 抓「自己写的期望也错」 |
| **`S`** | **spec 手写期望**：照 `doc/go_spec.html` / 官方源码手写 | 锚规范文本；抓 oracle 也错的形态 |
| **`H`** | **结构性 / harness 不变式**：穷尽性、确定性、幂等、预算、两清单守卫 | 抓「形态漂移」与「不可重放」 |

> 每条 ID **至少**有一种来源；`O` 与 `S` 同时存在时，**两者都要过**（不是二选一）。
> M4 的「双腿判据」（§5）中：腿①（marker 判定）与腿②（实跑位类对齐）**都归 `O`**。

---

## 2. 覆盖矩阵

### 2.1 roadmap §M4 九大项 ↔ `T-CK-*`

| # | roadmap §M4 句子 | 覆盖它的测试 ID |
|---|---|---|
| 1 | 「表达式/语句检查」 | `T-CK-31..38` |
| 2 | 「方法集/字段提升」 | `T-CK-51..54`、`T-CK-14` |
| 3 | 「接口实现/断言静态约束」 | `T-CK-12`、`T-CK-13`、`T-CK-38`、`T-CK-53/54` |
| 4 | 「内建签名」 | `T-CK-41..44` |
| 5 | 「泛型实例化/推断」 | `T-CK-21..25`、`T-CK-84/85` |
| 6 | 「版本门控」 | `T-CK-61..64` |
| 7 | 「main 约束」 | `T-CK-73` |
| 8 | 「初始化依赖/循环」 | `T-CK-71`、`T-CK-72` |
| 9 | 「导出与多包可见性」 | `T-CK-74`、`T-CK-75` |

### 2.2 roadmap 门禁 G1–G5 ↔ 本清单

| 门禁 | 兑现处 |
|---|---|
| G1（check/spec/examples 精选全绿） | §6 清单 + `T-CK-91..95`（`CH-2`） |
| G2（对齐错误位置/类别） | §4 类别 + §5 渲染 + `T-CK-91/92` |
| G3（额外错误可检测） | `T-CK-94`（`other` 守卫）+ `T-CK-B4/B5`（变异） |
| G4（四组点名 fixture） | `.`/`_`/别名 import = `T-CK-75`；init cycle = `T-CK-72`；泛型推断 = `T-CK-22..24`；方法集/字段提升 = `T-CK-51/52`（各有 ≥1 ID） |
| G5（fixedbugs 逐批） | `T-CK-A1..A3` + wave A 清单（§6.3）；全集不足额入首轮门禁 |

### 2.3 M3 挂起项与 9 缺测试项

| 项 | 承接 |
|---|---|
| `TYP-13/18/19/23..26`（`U-M3-01..04`） | `T-CK-11..15`、`T-CK-21..25`（`CH-1`/`CH-4`） |
| `U-M3-05`（约束接口同一性，**必须解除**） | `T-CK-15` + `CH-6` 的 `T-CK-E5` |
| `U-M3-08`（需类型信息的 `const` 初始化式） | `T-CK-43`（由 `rgoc check` 面解除；`T-TY-83` 行为不改，加注记） |
| 9 缺测试项中 M4 段 | `T-CK-76`：`TYP-26`、`EXP-16`、`EXP-22`（静态）、`PKG-04`、`PKG-06` 的**自有 fixture**（`SCP-06` 已在 M3；`RT-*` 三项留 Runtime 阶段） |

**未覆盖检查**：§2.1–2.3 每行都有 ID；无孤儿功能点（本句是 `manifest_m4.rs` 的对账对象之一）。

---

## 3. 四腿测试 ID 全表

### 3.1 腿甲：crate 内单测（`S`/`H`，37 个）

**A. 判定五件套（`T-CK-11..16`）**

| ID | 来源 | 内容 | 期望 |
|---|---|---|---|
| `T-CK-11` | `S`+`O` | 可赋值性：identical→可表示→底层同一（至少一侧非具名）→implements→chan/nil 后继规则的**规则序**；含 cause 面不承诺 | fixture 逐条命中；`cmp assignable` 与 oracle 一致 |
| `T-CK-12` | `S`+`O` | 接口实现（`implements`，非约束模式）：空接口恒真；V 方法须全含；指向接口的指针规则 | 同上（`cmp implements`） |
| `T-CK-13` | `S`+`O` | 约束满足（`satisfies`，`constraint=true`）：空类型集一方恒满足；`~T`/并集/`comparable` 面 | 同上（`cmp satisfies`） |
| `T-CK-14` | `S`+`O` | 方法集：值/指针接收者、嵌入提升、`*T` 规则；与 `selection` 的一致性 | 同上（`cmp methodset`） |
| `T-CK-15` | `S` | **类型集版 interface 同一性（解除 `U-M3-05`）**：匿名约束接口按（comparable 标志、项集、方法集）判定；**反向合成反例**（异项集/异 comparable ⇒ 不同一） | 两侧都作答、无拒绝；合成反例各一条断言 |
| `T-CK-16` | `H` | 判定五件套的**投影穷尽性**：每件套的入口与 `DiagCode`↔类别投影一齐穷尽（编译期 `match` 无通配） | 计数断言 + 缺一即红 |

**B. 泛型三件 + 统一（`T-CK-21..25`）**

| ID | 来源 | 内容 | 期望 |
|---|---|---|---|
| `T-CK-21` | `S`+`O` | 显式实例化：`F[T1,…]`/`T[int]` 的替换、实例身份（同一实参 ⇒ 同一实例） | fixture + `cmp instance` 一致 |
| `T-CK-22` | `S` | 推断（1.18 实参模式）：实参对形参、嵌套泛型类型 | 命中；失败形态给稳定码 |
| `T-CK-23` | `S` | 推断（1.21 赋值/接口方法语境） | 命中 |
| `T-CK-24` | `S` | 推断（1.27 赋值上下文函数类型）+ **统一三模式**（exact/loose/assign） | 命中（`infer`/`unify` **不设** cmp 形式，`D-M4-15`） |
| `T-CK-25` | `S` | 实例环与已知边界：环 ⇒ 稳定拒绝；超出支持面形态 ⇒ 显式拒绝 + 登记（`U-M4-09`） | 拒面对（不静默接受） |

**C. 表达式与语句检查（`T-CK-31..38`）**

| ID | 来源 | 内容 | 期望 |
|---|---|---|---|
| `T-CK-31` | `S` | 操作数模式（值/变量/类型/无值）与表达式类型计算 | 命中 |
| `T-CK-32` | `S` | 运算符静态规则：算术/比较/逻辑/位运算、字符串、符号与类型组合、**移位计数** | 命中（拒绝用「码+位置」） |
| `T-CK-33` | `S` | 转换与可转换性（含常量边界与 `~T` 面） | 命中 |
| `T-CK-34` | `S` | 索引/切片（3-index、字符串/rune 面）/选择（字段/方法值/方法表达式） | 命中 |
| `T-CK-35` | `S` | 语句静态规则：赋值性、`if/for/switch/select` 合法性、`fallthrough` 规则、`return` 规则、**终止语句**、`go/defer` 约束 | 命中 |
| `T-CK-36` | `S` | `range` 静态形态：数组/切片/字符串/map/chan/整型（1.22）/func（1.23）；迭代变量类型与个数 | 命中 |
| `T-CK-37` | `S` | 复合字面量与**嵌套 key**（1.27 门控见 `T-CK-61`） | 命中 |
| `T-CK-38` | `S` | 类型断言静态约束：`v.(T)` 的目标接口性、P 方法冲突（同时含 `T` 与 `*T` 方法）等 | 命中 |

**D. 内建与 `unsafe`（`T-CK-41..44`）**

| ID | 来源 | 内容 | 期望 |
|---|---|---|---|
| `T-CK-41` | `S` | 18 个预声明内建的**签名**（`append`…`clear/min/max`） | 命中 |
| `T-CK-42` | `S` | 内建**静态约束**：`append` 元素可赋值、`close`/`delete`/`copy` 合法性、`make` 类型面、`new` 形态（1.26 面见 `T-CK-61`） | 命中 |
| `T-CK-43` | `S` | 常量折叠接线：`len`/`cap`/`Sizeof`/`Alignof`/`Offsetof` 的常量结果（**`U-M3-08` 解除点**） | 命中（`const x = len("abc")` 合法） |
| `T-CK-44` | `S` | `unsafe` 白名单：`Sizeof/Alignof/Offsetof` + `Pointer` 类型名；**白名单外显式拒绝**（`U-M4-06`）；sizes=`StdSizes{8,8}` | 白名单对 + 拒绝面各一条 |

**E. 方法集/提升/实现语境（`T-CK-51..54`）**

| ID | 来源 | 内容 | 期望 |
|---|---|---|---|
| `T-CK-51` | `S` | 方法集构成（继承 M3 表示册）+ 指针接收者规则 | 命中 |
| `T-CK-52` | `S` | 字段/方法**提升**：深度最小者胜、同级歧义=错、被遮蔽形态 | 命中 |
| `T-CK-53` | `S` | 实现检查的**语境**：赋值、实参、返回、`var` 声明、复合字面量字段 | 命中 |
| `T-CK-54` | `S` | 空接口/空类型集**边界**规则（恒真面与惊异面各一条） | 命中 |

**F. 版本门控（`T-CK-61..64`）**

| ID | 来源 | 内容 | 期望 |
|---|---|---|---|
| `T-CK-61` | `S`+`O` | `verifyVersionf/allowVersion` **全表**（设计 `F1`）的**表驱动 fixture**：每特性低/高两档 | 低档拒（`version` 类）+ 高档接 |
| `T-CK-62` | `S`+`O` | 版本**同源**：`// -lang=` 首行 与 `//go:build` 文件版本的取 `max(file, go1.21)` 语义（`F11` / `check.go:319-349`；文件版本**取代**包版本） | 与 oracle 一致（语料 `go1_19_20` 一族正是此面） |
| `T-CK-63` | `O` | 语料 18 个 `-lang` 文件 + `go1_xx_19`：接受/拒绝判定（§6.2 表） | 双侧判定一致 |
| `T-CK-64` | `S` | 门控**位置与类别**：错误落点（range 的 soft 面等）投影为 `version` 类 | 命中 |

**G. 包级规则（`T-CK-71..76`）**

| ID | 来源 | 内容 | 期望 |
|---|---|---|---|
| `T-CK-71` | `S`+`O` | init 依赖图与 `InitOrder`（依赖排序后的 `[]Initializer`） | fixture + oracle 对撞逐条一致 |
| `T-CK-72` | `S` | init **环**诊断（次级链按 `": \t"` 过滤后） | 命中（环上每个变量各一条） |
| `T-CK-73` | `S` | `main`/`init` 约束：签名与引用约束（按 `go/types` 实测面；compiler 独有面在报告声明外置） | 命中 |
| `T-CK-74` | `S` | 导出与多包可见性：`pkg.X` 先导出性（Unicode 大写）再**跨包对象比对**；`UndeclaredImportedName` 细分码 | 命中 |
| `T-CK-75` | `S` | import 三形态：`.`/`_`/别名 的名字进入规则（G4 点名组） | 命中 |
| `T-CK-76` | `S` | 9 缺测试项 M4 段：`TYP-26`、`EXP-16`、`EXP-22`（静态）、`PKG-04`、`PKG-06` 的自有 fixture | 各 ≥1 条，全过 |

### 3.2 腿乙：`--cmp` 新判定式对撞（`O`，5 个）

| ID | 来源 | 内容 | 期望 |
|---|---|---|---|
| `T-CK-81` | `O` | `assignable <V> => <T>` 的 fixture 行 | 与 oracle 逐条一致（`true/false/reject`） |
| `T-CK-82` | `O` | `implements <V> => <I>` | 同上 |
| `T-CK-83` | `O` | `satisfies <V> => <C>` | 同上 |
| `T-CK-84` | `O` | `methodset <T>`（一元；v1=排序后方法**名字**序列，`-` 为空）——输入语法扩展走 `M3-tests.md` §5.2 修订 `R-32-2` | 同上 |
| `T-CK-85` | `O` | `instance <F> => <args>`（实参列表；成功=C7 单根 dump；失败=`reject:<category>`）——语法/输出形态同 `R-32-2` 与 §3.2 | 同上 |

> 语法沿 `//rgoc:cmp <kind> <A> => <B>`（`R-32-1`）；`infer`/`unify` 不设 cmp；未知 kind 仍拒
> （`T-TY-82` 改写点，`CH-6`）。排除集机制沿 M3（`# reject` 行两侧同剔、打印条数）。

### 3.3 腿丙：语料与装置（`O`，8 个）

| ID | 来源 | 内容 | 期望 |
|---|---|---|---|
| `T-CK-91` | `O` | **入选 75 双腿对撞**：`run-collide.sh --check <testdata 根>` 全绿（腿① marker 判定 + 腿② 位类对齐 + stderr 空） | 全绿（`CH-2`） |
| `T-CK-92` | `O`+`H` | 对撞**确定性**：两侧各自重复运行 sha256 一致；`--diag-dump` 幂等 | 一致 |
| `T-CK-93` | `H` | **两清单守卫**（`check_corpus.rs`）：候选 == 入选 ∪ 排除（两向、无死行、无重复）；理由非空且含指针（`D-M4-`/`U-M3-`/`T-CK-`/`CH-`） | 断言在、且**反向验证过** |
| `T-CK-94` | `O` | **`other` 守卫**：入选语料上 oracle 侧每条消息命中非 `other`；rgoc 侧类别投影无 `Other`（除修订）；适配表变更须全语料重跑 | 全命中；反向见 `B3` |
| `T-CK-95` | `H` | **清单↔官方判定互洽**：每个入选文件「腿① verdict（markers 有/无）」与「腿② 渲染空/非空」一致（accept ⟺ 空渲染且无 reject 行） | 一致 |
| `T-CK-A1` | `H` | wave A 清单冻结（`check-fixedbugs-waveA.txt`：50 行、4 组、全 EQ、零 import）与存在性断言 | 断言在 |
| `T-CK-A2` | `O` | wave A 双腿对撞（同 `--check --list` 形态）；**预算不足时按降级序先砍，砍掉须登记**（`M4-report.md`） | 全绿，或「显式砍 + 登记」 |
| `T-CK-A3` | `O`+`S` | **M3 15 例 DIFF / 10 例拒绝的根因复核**：实例化落地后逐例给出「已可提升 / 仍属 C7 渲染面 / 登记去向」结论（记录入 `M4-report.md`） | 逐例有结论；不要求 M4 内提升（M3 清单不动） |

### 3.4 腿丁：驱动与端到端（`S`/`H`，5 个）

| ID | 来源 | 内容 | 期望 |
|---|---|---|---|
| `T-CK-E1` | `S` | `rgoc check` 端到端：退出码约定（0=无诊断 / 1=有 / 2=用法或致命）、诊断流（位置+码）、`--diag-dump`（C8 渲染）、`--lang`、`--test-hooks` 注入开关；`-goexperiment` 指令的**响亮失败**守卫（`U-M4-04`） | 命中 |
| `T-CK-E2` | `S`+`H` | `rgoc types` 回归：既有 `--dump`/`--cmp`（旧 kind）行为不变（M3 口径保持）；新 kind 由 `rgoc types --cmp` 同门 | 不回归 |
| `T-CK-E3` | `H` | `manifest_m4.rs` 对账：`M4-manifest.json` 的 `gate`/`test_ids`/`unsupported` 与**冻结文档**逐项对账（照 `manifest_m3.rs` 范式，含合成反例） | 全绿 |
| `T-CK-E4` | `H` | 既有自检不回退：`scripts/check-m0-consistency.py`、`check-m1-consistency.py`、M2 三脚本（`check-ast-golden.sh`/`check-ast-testdata.sh`/`check-ast-stdlib.sh`） | 全过、exit 0 |
| `T-CK-E5` | `S` | **M3 翻转同步**（`CH-6`）：`M3-tests.md` §6 修订入档；`M3-manifest.json` 追加式更新（`resolved_by: M4`）；`T-TY-81`（解除断言）与 `T-TY-82`（新 kind 作答）**定向改写**；`T-TY-83` 行为不改加注记；`cargo test -p rgoc-driver --test manifest_m3` 全绿 | 全绿 |

### 3.5 腿反：变异反向验证（`H`，8 个）

| ID | 内容（"怎么造红"） | 目标守卫 |
|---|---|---|
| `T-CK-B1` | 把判定五件套任一条做反（如 `implements` 忽略一条方法） | `T-CK-12/13`、`CH-1` |
| `T-CK-B2` | rgoc 类别投影改一码（如 `shift`→`op-invalid`） | `T-CK-91` 渲染差异 |
| `T-CK-B3` | oracle 适配表删一条 regexp 信条 ⇒ `other` 出现 | `T-CK-94` 守卫 |
| `T-CK-B4` | 往 rgoc 注入一条**多余**诊断 ⇒ 多重集不等 | G3「额外错误可检测」 |
| `T-CK-B5` | 打乱一侧渲染一行（或改一行行号） | 比对器本身 |
| `T-CK-B6` | 版本门控反转（低档放行） | `T-CK-61/63` |
| `T-CK-B7` | 清单守卫：从 `check-selected.txt` 移走一行 / 往 allowlist 塞一行 | `T-CK-93` 两向相等 |
| `T-CK-B8` | 交换 `InitOrder` 两行输出 | `T-CK-71` |

> 纪律：每条都要有**实证记录**（命令 + 变红输出摘要）进 `scripts/mutation-m4-notes.md`；
> 沿 M3 的产物陈旧陷阱（`cargo clean -p`；可疑时全量重建），见 `scripts/mutation-m3-notes.md` §0。

---

## 4. 类别枚举（**冻结 v1**）与映射纪律

### 4.1 枚举（26 类，字典序）

```text
assign  builtin  comma-ok  convert  cycle  decl  implements  import  index
not-type  op-invalid  other  overflow  parse  range  ret  selector  shift
slice  stmt  terminate  type-arg  undef  unused  unsafe  version
```

* 相对设计 `§3.6` 草案的**唯一增补**：**`parse`** —— 解析（词法/语法）面诊断的统一类别。
  依据：`R-6-1`（7 个含语法错误文件入入选；官方侧 `scanner.ErrorList` 参与错误清单）。
  两侧的解析错误都投影为 `parse`（消息文本不进判据）。
* `other` 是**逃逸口**，不是类别：只允许在**排除清单**的文件上出现；**入选语料上出现即红**（`T-CK-94`）。

### 4.2 两侧映射纪律

| 侧 | 机制 | 守卫 |
|---|---|---|
| rgoc | 新 `DiagCode` 族（字符串前缀 `CHK-`；既有 `TYP-` 族沿用；Rust 枚举 `Chk*`）+ **全量穷尽投影**到类别 | 编译期 `match` 无 `_` 通配 + 计数断言（`T-CK-16`）；`Other` 不出现在投影表（除修订） |
| oracle | 消息 regexp **适配表** → 类别 | `T-CK-94`：入选语料每条消息非 `other`；适配表变更须全语料重跑 |

新码纪律沿 `M3-tests.md` §4「五件套」：枚举按字典序落位 + `ALL` 数组 + `as_str` + **两个总数测试**
+ 至少一条触发它的测试 ID。**没有触发者的码不许进枚举。**

---

## 5. 双腿判据与渲染（判据结构冻结）

### 5.1 腿①：marker 判定（操作口径 = verdict-only；坐标取 `commentMap`，`F2`⑦）

* marker 提取：注释文本（去 `//`、`/*`、`*/`）匹配 `^ ERRORx? `（前缀正则）；位置=**前一 token 的行列**（`commentMap` 语义）。
* 判定：`markers>0 ⇒ reject`（rgoc 侧诊断数 >0）；`markers==0 ⇒ accept`（诊断数==0）。
* 过滤/抑制：错误回调剔除含 `": \t"` 的次级消息；**Cheap-trick 抑制照 `go/types`（`errors.go:111-123`）两侧同规则**；解析错误参与；首行 `// -lang=` 生效；`shouldTest` 构建约束过滤。
* 本腿**只判 verdict**（官方全量消耗语义——每条实错必须匹配 + 每条标记必须消耗 + `colDelta=0`——**不采用**；`U-M3-10`）。

### 5.2 腿②：位类对齐渲染（`C8` 骨架；字节格式契约冻结于首个任务）

```text
# source: <rgoc|oracle>
# file: <相对路径>
<行号>:<列>\t<类别>
…
```

* 排序 = 按（行号, 列, 类别）稳定排序；**重复行保留**（多重集）。
* **列号进渲染、不进硬判据**（`U-M4-07`，设计 `§10 #1`）：硬判据 = 剥掉列之后的
  （行号, 类别）多重集逐项相等；列差异收集为**提示**打印（不影响 pass/fail）。
  RED 期实测两侧列稳定 ⇒ **修订升格**为硬判据（渲染格式已是最终态，升格只改判据）。
* 每文件**必须**至少输出头两行（即使零错误）——否则「两侧都空」会是假绿。
* 判据：两侧去除 `#` 行、剥列后**逐项一致**；两侧 stderr 皆空；两侧各自重复运行 sha256 一致。
* 「两侧都空」**仅当**腿①判 accept 时才算过（`T-CK-95` 互洽）。

### 5.3 单文件通过定义

```text
pass(file) = 腿①verdict ∧ 腿②逐字节一致 ∧ stderr==空 ∧ 重复运行确定
```

---

## 6. 语料三清单（冻结）

### 6.1 口径

* **候选**（check 面）= `check/`、`spec/`、`examples/` 三目录**顶层** `.go` = **91**；
  目录包（`decls2/`、`importdecl0/`、`importdecl1/`、`issue25008/`）不在候选（wave 2，`U-M4-11`）。
* **入选** = 官方语义复扫 EQ ＋ 特例 ⇒ `tools/types-oracle/check-selected.txt`（**75**）：64 reject（标记 **1376**）+ 11 accept。
* **排除** = `check-allowlist.txt`（**16**，逐行理由 + 指针）：14 计数不等（含 const1/decls1/lookup1/lookup2 四个 errs<markers）+ chans（`import "runtime"`，`U-M3-07`）+ importC（`-fakeImportC`，`U-M4-05`）。
* **两向互补**由 `check_corpus.rs` 断言（`T-CK-93`）。
* wave A（fixedbugs）= `check-fixedbugs-waveA.txt`（**50**：G1 25 / G2 8 / G3 13 / G4 4；全 EQ、零 import、标记 132）。

### 6.2 语言特例与版本门控文件（入选中）

* **`check/literals.go`** 特例：依赖 `assert` 注入（`F13`）。两侧**都开**测试内建
  （oracle `--check` 注入 / rgoc `--test-hooks`）；不注入会看到 68 条 `undefined: assert`。
* **版本门控文件 18+1**（`-lang` 指令 18；`go1_xx_19` 只有 `//go:build`）：
  `check/`：`decls0`(1.17)、`go1_8`、`go1_12`、`go1_13`、`go1_16`、`go1_19`、`go1_19_20`、`go1_20_19`、`go1_21_19`、`go1_21_22`、`go1_22_21`、`go1_25`、`issues0`(1.17)；
  `spec/`：`comparable1.19`、`typeAliases1.8`、`typeAliases1.22`、`typeAliases1.23`；
  `examples/`：`inference`(1.20)。判定预期以冻结日复扫为准（accept/reject 各文件已钉）。
* **import 特例 4 个**：3 个 `unsafe`（`typeinstcycles`、`spec/conversions`、`examples/methods`，白名单内）+
  1 个 `map0`（`import "chans"` 的 could-not-import 负例，两侧同判、统一渲染）。

### 6.3 修订纪律（执行期）

* **移出**（入选→排除）：仅按 `D-M4-3` 失效条件（系统性差异且根因属官方实现细节/未支持面），
  逐行理由 + 本文件修订表 + `M4-report.md`；**不许**放宽判据换绿。
* **移入**（排除→入选）：须修订 + 出处说明（新 wave）。
* **R-6-1 的风险登记**：7 个含语法错误文件中，解析错误的**行对齐**依赖 `go/parser` 与
  rgoc-parse（M2 的 `internal/syntax` 口径）在恢复策略上的一致；实测不一致 ⇒ 按「移出」纪律处理
  （逐例理由），不引入容差。

---

## 7. `unsupported` 白名单（**开工前冻结**）

### 7.1 从 M3 承接、仍有效

| # | 项 | 去向 |
|---|---|---|
| `U-M3-06` | 构建约束**筛选**、`_test.go`/cgo/汇编 | M4 之后 |
| `U-M3-07` | 官方二进制 export data、模块、`go.mod`、标准库 import（`unsafe` 例外） | M4 之后 |
| `U-M3-09` | `//go:` 未知指令的**裁定** | noder 等价阶段 |
| `U-M3-10` | 诊断消息**逐字**一致 | 不承诺 |
| `U-M3-11/12/13` | 非法 UTF-8 dump 分叉 / `Pos::Ord` 绝对坐标 / 相对路径拼接 | 暴露时回填 |

### 7.2 M4 新增

| # | 项 | 依据/去向 |
|---|---|---|
| `U-M4-01` | 执行语义（求值、短路、`defer` LIFO、goroutine、range 随机起点；含 loopvar 与 range-over-func 的**执行**面） | M5/M8（`D-M4-12` 分别声明） |
| `U-M4-02` | `EXP-27` 求值顺序（codegen 面） | M6+ |
| `U-M4-03` | 反射、并发、`SYS-05` 运行时错误集 | M8+ |
| `U-M4-04` | `-goexperiment=` 指令面 | 未建模：装置遇非空 goexperiment ⇒ **响亮失败** |
| `U-M4-05` | `-fakeImportC` / cgo `import "C"` | wave 2（`importC.go` 排除理由） |
| `U-M4-06` | `unsafe` 白名单外成员（`Add/Slice/…`、`Pointer` 转换、`uintptr` 往返、内存访问） | M6+（`D-M4-8`） |
| `U-M4-07` | **列号**进硬判据 | 未决（`§5.2`：渲染携带、判据剥列；升格=修订） |
| `U-M4-08` | `methodset` 的**签名**渲染面 | v1 名字序列（§3.2 `T-CK-84`） |
| `U-M4-09` | 泛型算法**超出面**的显式拒绝形态 | 具体形态 RED 期钉死（`D-M4-5`） |
| `U-M4-10` | fixedbugs **全集**（wave A 之外） | 逐批引入（G5） |
| `U-M4-11` | check 目录包（4 个多文件包） | wave 2 |

> **纪律**：白名单项**不得**记为 pass；每项要有**正向断言**证明它确实被显式拒绝/走文档约定路径
> （`U-M4-04` 的守卫进 `T-CK-E1`；`U-M4-06` 进 `T-CK-44`；`U-M4-07/08` 由 `T-CK-91/84` 的**反向**说明承担）。

### 7.3 M3 翻转表（`D-M4-11`）

| 项 | 翻转子面 | 同步动作 |
|---|---|---|
| `U-M3-01..04` | `types --cmp` 新 kind / 判定五件套 | `T-TY-82` 改写（新 kind 作答 + 未知 kind 仍拒）；`M3-tests.md` §6 加修订指针；`M3-manifest.json` 追加 `resolved_by` |
| `U-M3-05` | `identical`（约束接口） | `T-TY-81` 改写为**解除断言**；合成反例；manifest 更新 |
| `U-M3-08` | **`rgoc check` 面** | `T-TY-83` **行为不改**（`rgoc types` 口径保持）+ 注记 |
| —— | 总闸 | `cargo test -p rgoc-driver --test manifest_m3` 全绿 + workspace 全绿（`T-CK-E5`） |

---

## 8. 门禁定稿（`CH-1..CH-9`）

| # | 门禁 | 由哪些 ID 判定 | 通过定义 |
|---|---|---|---|
| `CH-1` | 判定五件套全绿 | `T-CK-11..16`、`T-CK-81..85` | 全绿；`U-M3-05` 拒面**被解除**且反向合成反例存在 |
| `CH-2` | 检查器接受/拒绝闭环 | `T-CK-31..38`、`T-CK-51..54`、`T-CK-91..95` | 入选 75 全绿（双腿）；排除清单每行有理由；零容忍（多/少/错类均红） |
| `CH-3` | 版本门控全绿 | `T-CK-61..64` | 全表 fixture + 语料 18+1 文件；`-lang` 与 `//go:build` 同源 |
| `CH-4` | 泛型闭环 | `T-CK-21..25`、`T-CK-85`、`T-CK-A3` | 自有 fixture（推断三形态）+ 语料 `typeinst*`/`typeinference`/`inference*` 在入选；15 例 DIFF 根因复核记录在 |
| `CH-5` | 包级静态规则 | `T-CK-71..76` | 全绿；四组点名 fixture（G4）齐 |
| `CH-6` | M3 白名单翻转同步 | `T-CK-E5` | 修订入档 + manifest 更新 + `T-TY-81/82` 改写 + `manifest_m3.rs` 全绿 |
| `CH-7` | 四条 cargo + 全量回归 | `T-CK-E4` + §9.1 命令 | 四条命令 exit 0、无 warning；M0/M1/M2 自检不回退 |
| `CH-8` | oracle 与对撞装置全绿 | `T-CK-91/92`、§9.2 五形态 | `run-collide.sh` 五形态全绿；oracle 侧 `gofmt`/`go vet` 干净；两侧 stderr 空、重复运行确定 |
| `CH-9` | 反向验证 + 交付 A | `T-CK-B1..B8`、`T-CK-A1/A2` | 变异 ≥8 条有实证；`mutation-m4-notes.md` 在；`M4-report.md`（前端子集矩阵 + 三条分别声明）在 |

> wave A（`T-CK-A2`）属 `CH-9` 的**先砍**项：预算不足时显式砍并登记，不算静默失败；
> 入选集接受/拒绝 + 位类对齐、版本门控、init 顺序/环、四组 fixture、判定五件套、`U-M3-05` 解除——**不砍**。

**冻结规则**（沿 M3）：① 门禁用例 100% 通过，无基建/不稳定失败；② 白名单只能开工**前**修订；
③ 跳过项不得记为 pass；④ 每条断言上线前做**反向验证**（照不出失败形态的探针**如实登记**，不硬凑）。

---

## 9. 验证命令、超时与资源上限

### 9.1 四条 cargo 命令（判据不变）

```sh
scripts/in-container.sh cargo fmt --all --check
scripts/in-container.sh cargo check --workspace --all-targets
scripts/in-container.sh cargo clippy --workspace --all-targets -- -D warnings
scripts/in-container.sh cargo test --workspace --no-fail-fast
```

### 9.2 对撞装置（五形态；`run-collide.sh`）

```sh
# ① 自有 fixture（dump / C7）—— M3 既有，不回归
scripts/in-container.sh bash tools/types-oracle/run-collide.sh tools/types-oracle/testdata
# ② 自有 fixture（//rgoc:cmp 判定行；含新 kind）—— T-CK-81..85
scripts/in-container.sh bash tools/types-oracle/run-collide.sh --cmp tools/types-oracle/testdata
# ③ 官方语料（M3 dump 面）—— 不回归
scripts/in-container.sh bash tools/types-oracle/run-collide.sh --corpus go_source_code/src/internal/types/testdata
# ④ check 面入选（75）—— T-CK-91/92/95（--check 为 M4 新增形态）
scripts/in-container.sh bash tools/types-oracle/run-collide.sh --check go_source_code/src/internal/types/testdata
# ⑤ check 面 wave A（50）—— T-CK-A2
scripts/in-container.sh bash tools/types-oracle/run-collide.sh --check --list tools/types-oracle/check-fixedbugs-waveA.txt go_source_code/src/internal/types/testdata
```

* 语料缺失 ⇒ **响亮失败**（目录不存在=退出码 2；读不到清单=退出码 2；清单行不在语料=退出码 2）。
* oracle 侧自检：`gofmt -l` 干净、`go vet ./...` 干净（构建带 `-buildvcs=false`）。

### 9.3 既有自检（不回退，`CH-7`）

```sh
python3 scripts/check-m0-consistency.py
python3 scripts/check-m1-consistency.py
bash scripts/check-ast-golden.sh          # M2
bash scripts/check-ast-testdata.sh        # M2
bash scripts/check-ast-stdlib.sh          # M2
```

### 9.4 资源上限（冻结）

| 上限 | 值 | 依据 |
|---|---|---|
| 类型/表达式递归深度 | **1000** | 沿 M2/M3 |
| 导入深度 | **100** | M3 |
| **每文件检查超时** | **10 s** | `M4-design.md` §6.2（本文件定稿） |
| 文件大小 | 64 MiB | M1 `D-M1-4` |
| smoke | **≤ 5 min** | `AGENTS.md` §4.3 |
| milestone | **≤ 30 min** | 同上 |
| full 单次 | **≤ 2 h** | 同上 |

---

## 10. 反向验证明细（与 `T-CK-B1..B8` 对应）

| 变异 | 施加点 | 期望红在 |
|---|---|---|
| `B1` 判定做反 | `rgoc-types` 判定核心 | `T-CK-12/13` 单测 + `run-collide --cmp` |
| `B2` 类别投影 | `DiagCode`→类别 投影表 | `T-CK-91` 渲染差异（行级） |
| `B3` 适配表信条 | oracle 适配表删一条 | `T-CK-94`（`other` 出现 ⇒ 红） |
| `B4` 多余诊断 | 往 rgoc 注入一条多余诊断 | 多重集不等（`T-CK-91`） |
| `B5` 比对器 | 打乱/改行 | 装置自身（diff 非空） |
| `B6` 版本门控 | 低档放行 | `T-CK-61/63` |
| `B7` 清单守卫 | 移一行/塞一行 | `T-CK-93` 两向相等 |
| `B8` init 顺序 | 交换两行输出 | `T-CK-71` |

> 每条变异执行前后按 `mutation-m3-notes.md` §0 清产物（`cargo clean -p <crate>`；可疑时全量重建）。
> 记录进 `scripts/mutation-m4-notes.md`（人读，不入库纪律同 M3）。

---

## 11. 待办（本清单之外，开工前或执行中收口）

| # | 待办 | 归属 |
|---|---|---|
| 1 | `C8` 契约（渲染字节格式 + 类别 + 过滤规则；从 §4/§5 抽出） | `M4-plan.md` 首个任务 |
| 2 | `tools/types-oracle --check` 形态 + `run-collide.sh --check [--list]` | `M4-plan.md` |
| 3 | `check_corpus.rs`（两向守卫 + 指针断言）与 `manifest_m4.rs`（对账） | `M4-plan.md` |
| 4 | `M4-manifest.json`：`gate`(CH-1..9) + `test_ids` + `unsupported`(U-M4-01..11 + M3 承接) + `budget` + `open_gaps` | `M4-plan.md` 末任务 |
| 5 | `scripts/mutation-m4-notes.md` | `CH-9` 收口 |
| 6 | `M4-report.md`（前端子集矩阵 + 三条分别声明 + `A3` 复核记录） | `CH-9` 收口 |
| 7 | M3 文档/测试翻转（`T-CK-E5`）与 `AGENTS.md`/`03` 状态回写 | 收口时 |
| 8 | `GAP-01.md` 补 M4 归属落点 | 收口时 |

---

## 12. 修订表

| 修订 | 日期 | 内容 | 触发 |
|---|---|---|---|
| `R-6-1` | 2026-10-18 | 设计 `§10 #5` 的未决项落地：官方语义复扫下 **7/8** 个含语法错误文件为 EQ ⇒ **纳入入选**（仅 `expr3.go` 排除）；随之引入 `parse` 类别（§4）与解析面行对齐的新风险（§6 末） | 选料实测（不改变任何 `D-M4-*`） |
| `R-3-1` | 2026-10-18 | `T-CK-62` 出处引用勘误：`F2`⑦ ⇒ **`F11`**（文件版本语义）；判据不变 | 设计复审（m3） |
| `R-5-1` | 2026-10-18 | §5.1 判据收束：腿①**操作口径 = verdict-only**（官方全量消耗语义不采用）；过滤清单补 **Cheap-trick 抑制**（`F2`⑧）；§3.2 `T-CK-84/85` 增输入语法指针（`M3-tests.md` §5.2 `R-32-2`） | 设计复审（B2/M1/M5 对齐） |
| `R-8-1` | 2026-10-18 | §8 `CH-8` 形态计数勘误：四 ⇒ **五**（与 §9.2 对齐；`--check --list` 为独立形态）；判据不变 | 设计复审（M4 对齐） |
| `R-4-1` | 2026-10-19 | §4.2 表「`Other` 不出现在投影表」按**「（除修订）」**口修订：rgoc 侧 `Other` 值域 = 恰 **4 个无官方语义码**（`ChkUnimplemented` 骨架占位 + `ExceedsFileLimit`/`SynNestingTooDeep`/`TypNestingTooDeep` 实现限制）；由 `T-CK-16`（`rgoc-check/tests/category.rs`）精确钉值域（第 5 码入 `Other` 即红）；其余 **63** 码全落真实类别；入选语料出现 `other` 即红（`T-CK-94`）不变 | `T-CK-16` 落地实测（边界码无真实类别可投影；硬塞真实类别 = 「答错」） |
| `R-4-2` | 2026-10-19 | Task 20 检查会话真身落地：骨架占位码 `ChkUnimplemented` 按五件套纪律（无触发者不许进枚举）移除 ⇒ 码表 **72 → 71**（`rgoc-lex`/`rgoc-parse`/`rgoc-check` 计数断言同步）；`R-4-1` 的 `Other` 值域由 4 收窄为 **3**（仅实现限制码；`T-CK-16` 快照 `("other", 3)` + 边界数组三项）；`CHK-` 前缀暂离 `rgoc-lex` 白名单（反向断言「白名单前缀必须有代号在用」照纪律执行） | Task 20 落地实测（红 `72 vs 71` → 绿；driver `check_diag_dump_与契约want_逐字节一致` 上线） |
| `R-4-3` | 2026-10-19 | Task 21 表达式运算符面落地：新增 `CHK-INVALID-OPERATION`（类别 `op-invalid`）⇒ 码表 **71 → 72**（`rgoc-lex`/`rgoc-parse`/`rgoc-check` 三处计数断言同步）；`CHK-` 前缀**回到** `rgoc-lex` 白名单（`R-4-2` 的「暂离」条款失效）；其余拒绝复用 M3 码（`TYP-{DIV-BY-ZERO,INVALID-SHIFT-COUNT,INVALID-SHIFT-OPERAND,INVALID-CONVERSION,NUMERIC-OVERFLOW}`），按 oracle `--check` 对 53 行逐字节对账 | `T-CK-32` 落地实测（红：缺码 + 签名；绿：53 行 + 接受向量零诊断） |
| `R-4-4` | 2026-10-19 | Task 23 索引/切片面落地（前半）：新增 `CHK-INVALID-INDEX`（类别 `index`；官方 `NonIndexableOperand` 与 `InvalidIndex` 三条 —— `must be integer` / `must not be negative` / `out of bounds [0:n]` —— 的合并码）与 `CHK-INVALID-SLICE`（类别 `slice`；官方 `NonSliceableOperand`（`cannot slice`）/ `InvalidSliceExpr`（`3-index slice of string`）/ `SwappedSliceIndices` 的合并码）⇒ 码表 **72 → 74**（三处计数断言同步；`index`/`slice` 各类首码）；索引下标的无类型常量转换复用 `convert_untyped`（`a[1.5]`/`a[1e100]` ⇒ `truncated to int`、`sl["x"]` ⇒ `cannot convert ... to type int` —— `convert` 类），类型化常量超 `int` 范围（`a[u64]`，`1<<63`）⇒ `overflows int` 落 **`overflow`**（c8Rules 的 index 规则不匹配、通用 overflow 规则接管 —— 探针实证）；map 键面 `cannot use ... in map index`（`assign` 类）归 Task 25 的赋值面 | `T-CK-34`（前半）落地实测（红：0/19；绿：19 行 + 接受向量零诊断） |
| `R-4-5` | 2026-10-19 | Task 24 选择器面落地（后半）：新增 `CHK-INVALID-SELECTOR`（类别 `selector`；官方 `MissingFieldOrMethod`（`... undefined (type ...)` 句式）/ `AmbiguousSelector` / `InvalidMethodExpr` / `NonFieldSelector`（`operand for field selector`）四条消息的合并码）⇒ 码表 **74 → 75**（三处计数断言同步；`selector` 类首码）；查找 = 官方 `lookupFieldOrMethod` 逐深度 BFS（`seen` 遮蔽 / 同级碰撞 / 具名指针类型特例），方法值去接收者、方法表达式接收者作首参；`*T` 的 typexpr 臂接通（`(*T).M` 可达）；`pkg.Name` 与泛型接收者面不报 | `T-CK-34`（后半）落地实测（红：0/7；绿：7 行 + 接受向量零诊断） |
| `R-4-6` | 2026-10-19 | Task 25 语句面落地：新增 **11** 个检查码 ⇒ 码表 **75 → 86**（三处计数断言同步）—— `CHK-INVALID-ASSIGN`（`assign`；`cannot use ... as ... value` / `assignment mismatch` / `cannot assign to` / 实参个数与逐参的合并码）、`CHK-WRONG-RESULT-COUNT`（`ret`）、`CHK-MISSING-RETURN`（`terminate`，报在体右花括号）、`CHK-NO-NEW-VAR`（`decl`，soft）、`CHK-SINGLE-VALUE-CONTEXT`（`comma-ok`）、`CHK-UNUSED-EXPR`（`unused`）、`CHK-NOT-A-TYPE`（`not-type`）、`CHK-INVALID-COND`/`CHK-INVALID-CASE`/`CHK-INVALID-GO-DEFER`/`CHK-INVALID-SELECT-CASE`（`stmt`）；逐元素赋值的报点在**右值**（官方 `assignment` 的 `x.pos`） | `T-CK-35` 落地实测（红：2/35 —— 只剩装载面两条 `SYN-BRANCH-FALLTHROUGH`；绿：35 行 + 接受向量零诊断） |
