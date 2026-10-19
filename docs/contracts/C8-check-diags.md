# C8 契约：检查诊断对撞格式

> ## 本契约把「检查诊断对撞」的比对面**冻结成文**
>
> M4 的验收核心（`CH-2` / `T-CK-91..95`）是 rgoc 检查器与官方 `go/types` 对**同一份
> fixture** 的诊断**位类对齐** —— 两侧各自把诊断投影到**同一类别枚举**并渲染成一份
> 中性清单，按（行号, 类别）多重集比较。本契约规定那份清单的**字节格式、类别枚举、
> 两侧映射纪律、过滤规则与比较算法**。语义判据（腿① verdict / 腿② 对齐）归
> `M4-tests.md` §5；本契约只冻结**字节面与类目面**，两者一起才构成完整判据。
>
> 实现两侧：
>
> - Rust：`rgoc-check/src/render.rs`（`--diag-dump`）+ `rgoc-driver check`（Task 2/20）
> - Go（oracle）：`tools/types-oracle`（`--check`）（Task 3）
>
> **为什么「中性」**：**不** dump 任何一侧的原始消息文本（那是消息复刻工程，
> `U-M3-10`/`D-M4-3` 明确不做），只 dump「位置 + 类别」。位置面里**列号**先以
> 提示层处理（`U-M4-07`），行号与类别进硬判据 —— 见 §6。

**状态**：**已冻结**（M4 / `Task 1`）　|　**契约级别**：**完整**（格式面）　|　**实现落点**：`rgoc-check::render` + `tools/types-oracle --check`　|　**消费者**：M4（`CH-2`、`T-CK-91..95`）、`run-collide.sh --check`

---

## 1. 范围

| 本契约**规定** | 本契约**不规定**（留给谁） |
|---|---|
| 渲染的**字节格式**：头行、数据行、排序、重复、确定性（§2） | 诊断的**语义面**：腿① verdict、过滤后的错误集合怎么来（归 `M4-tests.md` §5.1 与 §4/§5 的实现任务） |
| **类别枚举 v1**（26 类，字典序）与 `other` 纪律（§3） | 每条具体诊断**消息文本**（不做消息复刻；两侧各自适配） |
| 两侧**映射纪律**（rgoc `DiagCode` 穷尽投影 / oracle regexp 适配表）（§4） | 各 `DiagCode` 的**触发条件**（归 `rgoc-check` 各模块与 `M4-tests.md` §4 的触发者纪律） |
| **过滤规则**（`F2` 摘录：旗标、解析、`": \t"`、Cheap-trick、importer、注入开关）（§5） | 语料**入选/排除**清单内容（归 `check-selected.txt` / `check-allowlist.txt`；判据在 `M4-tests.md` §6） |
| **比较算法**（剥列多重集、提示层、升格条款）（§6） | 退出码的**语义实现**（§7 只冻结数值约定） |
| 最小对撞目标（§8）与对撞断言载体（§9） | fixedbugs 分批（`M4-tests.md` §6.3）与多文件包（`U-M4-11`） |

---

## 2. 渲染字节格式（`M4-tests.md` §5.2，冻结）

**行式、UTF-8、LF、无 BOM、文件末尾恰一个换行。** 逐文件单跑（一次一个文件；
跨文件不归并 —— 多文件包 `U-M4-11`）。

```text
# source: <rgoc|oracle>
# file: <相对路径>
<行号>:<列>\t<类别>
…
```

| # | 规则 | 内容 |
|---|---|---|
| 1 | 头行 | **前两行固定**：`# source: `（取值 `rgoc` / `oracle`，各写各的）+ `# file: <相对路径>`（该文件的路径；两侧各自如实给出，供人读）。**即使零诊断也必须输出这两行** —— 否则「两侧都空」会是假绿（`M4-tests.md` §5.2）。 |
| 2 | oracle 附加头 | oracle 侧在 `--check` 输出中**额外**输出 `# markers: <N>`（`N` = `commentMap` 语义数出的 `/* ERRORx? */` 标记数；**腿① 的机器读数据源**，`M4-tests.md` §5.1）。rgoc 侧不输出。 |
| 3 | `#` 行纪律 | 所有以 `#` 开头的行（含头行）是**注释**：两侧都忽略、**不进任何比较**。空行同样忽略。头行/`# markers` 的差异因此不影响判据（它们只服务人读与腿①）。 |
| 4 | 数据行 | `<行号>:<列>\t<类别>`。行号/列号 **1-based**（列 = 该侧词法位置的列；两侧各按自身位置表示，列先只进提示层，§6）。`<类别>` ∈ §3 枚举（**不得**输出枚举外 token）。**消息文本不进渲染**。 |
| 5 | 排序 | 按（行号, 列, 类别）**稳定排序**（同一文件内）。 |
| 6 | 重复 | **重复行保留**（多重集语义，不去重）—— 同一（行, 列, 类别）出现两次就写两行；不同列但同（行, 类别）也各写各行。 |
| 7 | 确定性 | 同一输入重复运行**逐字节相同**（`T-CK-92`）。 |
| 8 | 零诊断 | 只输出头行（rgoc：2 行；oracle：≥2 行，含 `# markers` 时 3 行）。 |

> **装置前提**：**逐文件单跑**。`# file:` 行供人读；跨文件不归并。多文件同包（目录包）
> 不在首轮对撞面（`U-M4-11`，wave 2）。

---

## 3. 类别枚举 v1（26 类，**字典序**，`M4-tests.md` §4 冻结）

```text
assign  builtin  comma-ok  convert  cycle  decl  implements  import  index
not-type  op-invalid  other  overflow  parse  range  ret  selector  shift
slice  stmt  terminate  type-arg  undef  unused  unsafe  version
```

* 枚举是**封闭集合**：渲染行的类别 token 必须落在其中。
* **`other` 是逃逸口，不是类别**：只允许出现在**排除清单**的文件上；
  **入选语料上出现即红**（`T-CK-94` 守卫）。rgoc 侧投影表**不含 `Other`**
  （除修订）；oracle 侧适配表未命中 ⇒ `other` ⇒ 该文件在入选语料上即红。
* **`parse`**：解析（词法/语法）面诊断的统一类别；两侧的解析错误都投影为 `parse`
  （消息文本不进判据）。依据 `R-6-1`（7 个含语法错误文件入入选）。
* 新码纪律（沿 `M3-tests.md` §4「五件套」）：枚举按字典序落位 + `ALL` 数组 +
  `as_str` + **两个总数测试** + **至少一条触发它的测试 ID**。**没有触发者的码不许进枚举。**

---

## 4. 两侧映射纪律（`M4-tests.md` §4.2）

| 侧 | 机制 | 守卫 |
|---|---|---|
| rgoc | 新 `DiagCode` 族（前缀 `CHK-`；既有 `TYP-` 族沿用；Rust 枚举 `Chk*`）+ **全量穷尽投影**到类别（**全量函数**：每条诊断必落到枚举内 token） | 编译期 `match` 无 `_` 通配 + 计数断言（`T-CK-16`）；`Other` 不出现在投影表 |
| oracle | 消息 regexp **适配表** → 类别 | `T-CK-94`：入选语料每条消息非 `other`；**适配表变更须全语料重跑** |

两侧各自独立实现「诊断 → 类别」；**类别 token 是唯一的公共词汇表**。

---

## 5. 过滤规则（`F2` 摘录，语义面在 `M4-tests.md` §5.1）

两侧**同规则**（rgoc 实现与 oracle 免费继承的都是官方口径）：

| # | 规则 | 锚点 |
|---|---|---|
| 1 | **旗标只读第一个文件的首行**：`// -lang=` → `Config.GoVersion`；`-goexperiment` 非空 ⇒ **响亮失败**；`-fakeImportC` ⇒ **响亮失败**（对应文件由清单排除，`U-M4-05`） | `go/types/check_test.go:181-187`（`F2`①） |
| 2 | 解析：`parser.AllErrors\|parser.SkipObjectResolution`；**parse 错误参与**（投影 `parse`） | `check_test.go:143`（`F2`②） |
| 3 | 注入开关（装置级）：`types.DefPredeclaredTestFuncs()`（`assert`/`trace`），对应 `rgoc check --test-hooks` | `F13` |
| 4 | importer：**只认 `unsafe`**（返回 `types.Unsafe`）；其余 import ⇒ 该文件进排除清单（`U-M3-07`） | 设计 §4.1 #4 |
| 5 | 次级消息过滤：错误回调**剔除**渲染字符串含 `": \t"` 的消息 | `check_test.go:169`（`F2`④） |
| 6 | **Cheap-trick 抑制**：由 `go/types` 内部生效（非首错位置 + 已有首错 + 指定前缀），oracle「免费继承」；rgoc 按同规则实现；**合成探针**证明行为在（Task 3 Step 1） | `go/types/errors.go:111-123`（`F2`⑧） |
| 7 | stderr 必须**空**；两侧各自重复运行 sha256 一致 | `M4-tests.md` §5.2 |

> 腿① 另吃两条：首行 `// -lang=`（`F2`①）与 `//go:build` 的 `shouldTest` 构建约束过滤
> （`F18`）—— 它们决定「该文件进不进对撞分母」，见 `M4-tests.md` §5.1/§6。

---

## 6. 比较算法（判据冻结；列 = 提示层）

对一对（rgoc 输出, oracle 输出）：

```text
1) stderr 两侧皆空                                  否则 FAIL
2) 两侧原始输出各含 ≥2 个 `#` 头行（`# source`/`# file`）   否则 FAIL（防「两侧都空」假绿）
3) 忽略 `#` 行与空行
4) 每条数据行：首个 `:` 左侧 = L；其余部分首个 `\t` 右侧 = K   （L 行号, K 类别）
5) 硬判据：{(L, K)} 多重集相等 —— 实现 = 按 (L, K) 稳定排序后逐项比较
6) 提示层：{(L, C, K)} 三元组差异按 (L, K) 分组计数打印    不影响 pass/fail
```

* **硬判据 = 剥列之后的（行号, 类别）多重集逐项相等**（`U-M4-07`；设计 §10 #1）。
  列差异只作**提示**输出（两侧列表示若不同源，允许只在提示层体现）。
* **「两侧都空」仅当腿①判 accept 时才算过**（`T-CK-95` 互洽）：reject 文件
  两侧必须非空 —— 由「腿①：markers>0 ⇒ rgoc 诊断数>0」+「step 5 相等」传递闭合。
* **升格条款**：RED 期实测两侧列稳定 ⇒ 走修订把列**升格**进硬判据
  （渲染格式已是最终态，升格只改判据、不改格式）。**不许**在未走修订的情况下
  把列当硬判据用。

---

## 7. 退出码约定（`T-CK-E1`；oracle 同）

| 码 | 含义 |
|---|---|
| `0` | 无诊断 |
| `1` | 有诊断（渲染照常输出） |
| `2` | 用法错误或致命错误（不进行对撞） |

对撞装置必须**容忍 `1`**（reject 文件是常态）、**在 `2` 上失败**。

---

## 8. 最小对撞目标：`check-diags-min`

**输入**（Task 3/4 放在 `tools/types-oracle/testdata/check-diags-min.go`）：

```go
package p

var _ = x
var _ = y; var _ = y
```

**期望渲染**（= `docs/contracts/testdata/check-diags-min.want`）：

```text
# source: oracle
# file: testdata/check-diags-min.go
3:9	undef
4:9	undef
4:20	undef
```

**理由（逐条对着 §2–§6 走）**：

* 官方实跑（容器内 go1.27.1，本契约冻结时探针复刻 §5 口径：`AllErrors|SkipObjectResolution`、
  `": \t"` 过滤、默认 importer）：**恰 3 条** —— `3:9 undefined: x`、`4:9 undefined: y`、`4:20 undefined: y`；
  无解析错误、stderr 空、两次运行同序。
* 类目投影：`undefined: …` ⇒ `undef`（oracle 适配表一条信条 / rgoc `CHK-` 对应码）。
* 排序（§2.5）：`(3,9,undef) < (4,9,undef) < (4,20,undef)`。
* 该 fixture 一次演示三件事：**同文件多错**（3 条）、**同行同类别重复**（`4:9` 与 `4:20`
  都是 `undef`，**两行都保留**，§2.6）、**列字段进渲染**（`9` / `20`）。
* 该文件**不在**任何语料清单里；它是两侧实现的**第一对撞目标**（单元/对撞测试用），
  不参与 `CH-2` 的语料门禁分母。

**`.want` 机制**（沿 C7）：`.want` = **冻结的 oracle golden**；

| 侧 | 断言 |
|---|---|
| oracle（`tools/types-oracle` 的测试） | oracle **现跑** == `.want`（忽略 `#` 行） |
| rgoc（`rgoc-driver` 的测试） | rgoc **现跑** == 同一个 `.want`（忽略 `#` 行） |

两条都成立 ⇒ 「rgoc == oracle」成立（传递性），而 `.want` 是**冻结在库里**的。

---

## 9. 与 oracle 的对撞（本契约的验证方式）

| 断言 | 载体 |
|---|---|
| 入选 75 双腿对撞全绿（腿① verdict + 腿② 位类对齐 + stderr 空） | `tools/types-oracle/run-collide.sh --check` + `T-CK-91`（`CH-2`） |
| 对撞确定性（两侧重复运行 sha256 一致；`--diag-dump` 幂等） | `T-CK-92` |
| 两清单守卫（候选 == 入选 ∪ 排除；理由非空且带指针） | `check_corpus.rs`（`T-CK-93`） |
| `other` 守卫（入选语料每条消息命中非 `other`） | `T-CK-94` |
| 清单↔官方判定互洽（verdict 与渲染空/非空一致） | `T-CK-95` |
| 最小对撞目标 | `docs/contracts/testdata/check-diags-min.want`（§8） |
| fixedbugs wave（同一渲染格式） | `check-fixedbugs-waveA.txt`（50 行） |

---

## 10. 变更记录

| 日期 | 变更 |
|---|---|
| 2026-10-18 | **新建并冻结**（M4 / `Task 1`）：渲染字节格式、类别枚举 v1（26 类，含 `parse`）、两侧映射纪律、过滤规则（`F2` 摘录）、比较算法（剥列多重集 + 升格条款）、退出码、最小对撞目标 `check-diags-min`（官方实跑 3 条，探针复刻 §5 口径）。 |
| 2026-10-19 | **修订（`R-4-1`，Task 11）**：§4 表「`Other` 不出现在投影表」按**「（除修订）」**口 —— rgoc 侧 `Other` 值域 = 恰 **4 个无官方语义码**（`ChkUnimplemented` / `ExceedsFileLimit` / `SynNestingTooDeep` / `TypNestingTooDeep`），由 `T-CK-16`（`rgoc/crates/rgoc-check/tests/category.rs`）钉死值域（第 5 码入 `Other` 即红）；其余码全落真实类别。`T-CK-94`（入选语料 `other` 即红）与 26 类枚举不变。 |
| 2026-10-19 | **修订（`R-4-2`，Task 20）**：检查会话真身落地 ⇒ 骨架占位码 `ChkUnimplemented` 按五件套纪律（**没有触发者的码不许进枚举**）从 `DiagCode` 移除：码表 **72 → 71**（`rgoc-lex`/`rgoc-parse`/`rgoc-check` 三处计数断言同步）；`R-4-1` 的 `Other` 值域由 4 收窄为 **3 个实现限制码**（`ExceedsFileLimit` / `SynNestingTooDeep` / `TypNestingTooDeep`），`T-CK-16` 定案快照同步 `("other", 3)`、边界数组三项；`CHK-` 前缀在首个检查面代号落地前暂离 `rgoc-lex` 白名单（该测试的反向断言「白名单前缀必须有代号在用」照纪律执行，属测试**第 4 次**抓到真实不一致）。 |
| 2026-10-19 | **修订（`R-4-3`，Task 21）**：首个检查面代号落地 —— 新增 `CHK-INVALID-OPERATION`（类别 `op-invalid`；承载一元 / 二元 / 比较 / `*` / `&` / `<-` / `nil` 面的「类型不匹配 · 运算符未定义」判据），码表 **71 → 72**（`rgoc-lex`/`rgoc-parse`/`rgoc-check` 三处计数断言同步）；`CHK-` 前缀**回到** `rgoc-lex` 白名单（`R-4-2` 的「暂离」条款自然失效）；其余拒绝面复用 M3 码（`TYP-{DIV-BY-ZERO,INVALID-SHIFT-COUNT,INVALID-SHIFT-OPERAND,INVALID-CONVERSION,NUMERIC-OVERFLOW}`）。对撞证据：`T-CK-32` 的 53 行拒绝向量（码 + 位置 + 渲染类别）与 oracle 逐字节一致 + 接受向量零诊断。 |
| 2026-10-19 | **修订（`R-4-4`，Task 23）**：索引 / 切片表达式面落地 —— 新增 `CHK-INVALID-INDEX`（类别 `index`；官方 `NonIndexableOperand` 与 `InvalidIndex` 三条 `must be integer` / `must not be negative` / `out of bounds [0:n]` 的**合并码**）与 `CHK-INVALID-SLICE`（类别 `slice`；官方 `NonSliceableOperand`（`cannot slice`）/ `InvalidSliceExpr`（`3-index slice of string`）/ `SwappedSliceIndices` 的**合并码**），码表 **72 → 74**（三处计数断言同步；`index` / `slice` 各类**首码**）。索引下标的无类型常量转换复用 Task 21 的 `convert_untyped`（`a[1.5]` / `a[1e100]` ⇒ `truncated to int`、`sl["x"]` ⇒ `cannot convert ... to type int` —— 类别 `convert`）；类型化常量超 `int` 范围（`a[u64]`，`u64 = 1<<63`）⇒ `overflows int` 落 **`overflow`**（c8Rules 的 index 规则不匹配、通用 overflow 规则接管 —— 探针实证）；map 键面 `cannot use ... in map index`（`assign` 类）归 Task 25 的赋值面。对撞证据：`T-CK-34`（Task 23 前半）19 行拒绝向量（码 + 位置 + 渲染类别）与 oracle 逐字节一致 + 接受向量零诊断。 |
| 2026-10-19 | **修订（`R-4-5`，Task 24）**：选择器面落地 —— 新增 `CHK-INVALID-SELECTOR`（类别 `selector`；官方 `MissingFieldOrMethod`（`... undefined (type ...)` 句式）/ `AmbiguousSelector` / `InvalidMethodExpr` / `NonFieldSelector`（`operand for field selector`）的**合并码**），码表 **74 → 75**（三处计数断言同步；`selector` 类**首码**）。查找按官方 `lookup.go::lookupFieldOrMethod` 窄面逐条对齐（逐深度 BFS / `seen` 遮蔽 / 同级碰撞 / 具名指针类型特例 / 指针接收者不可寻址回退）；方法值签名**去接收者**、方法表达式**接收者作首参**（`selection.rs`）；`*T` 的 typexpr 臂接通（`(*T).M` 可达；官方 `*ast.StarExpr` 的 typexpr 分支）；`pkg.Name`（`PkgName` 操作数不可建）与泛型接收者面 ⇒ 不报。对撞证据：`T-CK-34`（Task 24 后半）7 行拒绝向量（码 + 位置 + 渲染类别）与 oracle 逐字节一致 + 接受向量零诊断。 |
| 2026-10-19 | **修订（`R-4-6`，Task 25）**：语句面落地 —— 新增 **11** 个检查码 ⇒ 码表 **75 → 86**（三处计数断言同步）：`CHK-INVALID-ASSIGN`（`assign`；`cannot use ... as ... value` / `assignment mismatch` / `cannot assign to` / 实参个数与逐参面的合并码）、`CHK-WRONG-RESULT-COUNT`（`ret`）、`CHK-MISSING-RETURN`（`terminate`；报在体右花括号）、`CHK-NO-NEW-VAR`（`decl`；soft）、`CHK-SINGLE-VALUE-CONTEXT`（`comma-ok`）、`CHK-UNUSED-EXPR`（`unused`）、`CHK-NOT-A-TYPE`（`not-type`）、`CHK-INVALID-COND` / `CHK-INVALID-CASE` / `CHK-INVALID-GO-DEFER` / `CHK-INVALID-SELECT-CASE`（皆 `stmt`）。归属口径：**可赋值性/个数**一律落 `assign`（官方 `InvalidAssign` 一族），**语句形态错**合到 `stmt`。对撞证据：`T-CK-35` 的 35 行拒绝向量（码 + 位置 + 渲染类别）与 oracle 逐字节一致 + 接受向量零诊断。 |
