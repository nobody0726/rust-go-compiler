# M4 报告：静态检查闭环（交付 A）

> **阶段 ID**：`M4`　|　**状态**：**收口（部分兑现）**　|　**收口日**：2026-10-22
> **文档索引**：[`../README.md`](../README.md)　|　**上游**：[`M4-design.md`](./M4-design.md)（已确认）／[`M4-tests.md`](./M4-tests.md)（已冻结，含 `R-4-1..7`、`R-9-1`）／[`M4-plan.md`](./M4-plan.md)（Task 1–44）
> **机器可读事实**：[`M4-manifest.json`](./M4-manifest.json)（由 `rgoc-driver/tests/manifest_m4.rs` 逐项对账）
> **契约**：[`../contracts/C8-check-diags.md`](../contracts/C8-check-diags.md)　|　**反向验证**：[`../../scripts/mutation-m4-notes.md`](../../scripts/mutation-m4-notes.md)

---

## 0. 一句话结论

**判定五件套与泛型闭环已兑现，检查器闭环未兑现。**

`rgoc` 现在能对单个 `.go` 文件做出与 `go/types` **同口径的静态判定**（可赋值/方法集/接口实现/约束满足/类型集同一性）与**同类别的诊断渲染**（`C8`，26 类），并且这套判定经**双腿对撞**（marker 判定 + 剥列位类多重集）与**八条变异反向验证**证伪过。
但入选语料 75 个文件里**只有 17 个两侧一致**（其中 8 个是「双侧零诊断」的弱绿），**真实内容对齐的只有 9 个**。检查器还没闭环 —— 这不是「差一点」，是**整块规则面没铺完**（§4）。

| 门禁 | 名称 | 实测 | 一句话 |
|---|---|---|---|
| `CH-1` | 判定五件套全绿 | **pass** | `T-CK-11..16`、`T-CK-81..85` 全绿；`U-M3-05` 拒面解除且留合成反例 |
| `CH-2` | 检查器接受/拒绝闭环 | **fail** | 入选 75 实测 **17 绿 / 58 红**；排除 16 行理由齐 |
| `CH-3` | 版本门控全绿 | **partial** | 特性表 31 项 + 自有 fixture 全绿；语料 `go1_8/12/13/16/19/25` 六文件仍 0 行 |
| `CH-4` | 泛型闭环 | **pass** | 实例化/推断/统一 + 实例环；`A3` 26 行根因已定位 |
| `CH-5` | 包级静态规则 | **pass** | init 序/环、main/init、import 三形态与可见性、`G4` 四组 fixture 齐 |
| `CH-6` | M3 白名单翻转同步 | **pass** | `R-32-2`/`R-6-1` 入档、`resolved_by` 追加、`manifest_m3` 2/2 绿 |
| `CH-7` | 四条 cargo + 全量回归 | **pass** | 四条 exit 0 无 warning；**1541 passed / 0 failed**（136 suite）；五条自检不回退 |
| `CH-8` | oracle 与对撞装置全绿 | **fail** | 五形态中 dump(14)/cmp(7)/corpus(31) 绿；`--check` 与 `--check --list` 红 |
| `CH-9` | 反向验证 + 交付 A | **pass** | `B1..B8` 8/8 实证变红并复原；`A2` 按 `D-M4-14` **显式降级**（见 §8） |

> **为什么 `CH-2`/`CH-8` 不强行写成 pass**：把 58 个红文件「移出」入选清单（`D-M4-3` 的失效条件要求「系统性差异 + 根因属官方实现细节」）就等于把 `CH-2` 的定义抽空 —— 58/75 移出后剩下的 17 个不再构成「精选接受/拒绝用例」。**不放宽判据换绿**是本阶段最硬的一条纪律。

---

## 1. 前端子集矩阵（功能 ID × 四维）

四维 = **语法**（`rgoc-parse`，M2）/ **静态判定**（`rgoc-types` 判定核）/ **静态检查**（`rgoc-check` 诊断面）/ **执行**（M5+，**本阶段一律不承诺**）。

图例：`●` 兑现　`◐` 部分　`○` 不承诺　`—` 不适用

| # | 功能（roadmap §M4 句子） | 语法 | 静态判定 | 静态检查 | 执行 | 证据 / 保留 |
|---|---|---|---|---|---|---|
| 1 | 表达式/语句检查 | ● | ● | ◐ | ○ | `T-CK-31..38` 自有 fixture 全绿；语料 17/75（§4） |
| 2 | 方法集/字段提升 | ● | ● | ● | ○ | `T-CK-14/51..54`；`cmp methodset` 七 kind 之一 |
| 3 | 接口实现/断言静态约束 | ● | ● | ● | ○ | `T-CK-12/13/38`；`cmp implements`/`satisfies` |
| 4 | 内建签名 | ● | ● | ● | ○ | `T-CK-41..43`；`unsafe` 白名单另见 §2.3 |
| 5 | 泛型实例化/推断 | ● | ● | ● | ○ | `T-CK-21..25`、`T-CK-84/85`；`cmp instance` |
| 6 | 版本门控 | ◐ | ● | ◐ | ○ | 特性表 31 项 + 门控位置全绿；**语法层门控未落**（§5） |
| 7 | `main` 约束 | ● | — | ● | ○ | `T-CK-73` |
| 8 | 初始化依赖/循环 | ● | — | ● | ○ | `T-CK-71/72`；**图 + 顺序 + 环诊断 ≠ 运行正确**（§2.4） |
| 9 | 导出与多包可见性 | ● | ● | ● | ○ | `T-CK-74/75`；**只做源码 import**，标准库 import 不承诺（`U-M3-07`） |

**这张表怎么读**：`静态判定 ●` 表示「判定函数本身正确且被自有 fixture + `--cmp` 对撞证明」；`静态检查 ◐` 表示「诊断通路已通（`C8` 渲染、类别投影、双腿装置全绿），但规则覆盖不全，语料上仍有 58 个文件不一致」。

---

## 2. 三条分别声明（`D-M4-12` / `D-M4-13`）

> 这一节是交付 A 的**核心免责条款**：**不因语法可解析而标记运行语义完成**。
> 三条各按「静态面做到哪 / 执行面到哪 / 谁接」分开写，不许合并成一句「已支持」。

### 2.1 range over func（Go 1.23）

* **静态面：纳入。** `T-CK-36` 的形态矩阵（16 行）覆盖 `range` 的迭代变量个数、键值类型、`range` 表达式形态（数组/切片/map/字符串/整数/函数/通道）与 `InvalidRangeExpr` / `InvalidIterVar` 两类拒绝；`T-CK-64` 钉住 `1.23` 门控两档（`6:12` / `8:12` 两侧一致）。
* **执行面：不承诺（M8）。** 迭代顺序、`yield` 的调用时机、`break`/`return` 的收尾语义、`range` 随机起点**一概未实现**，也没有测试假装实现。
* **当前实测保留**：语料 `spec/range.go`（oracle 18 / rgoc 19）、`spec/range_int.go`（29 / 27）仍在 58 个红里 —— 静态面是**部分**兑现，不是「已绿」。

### 2.2 loopvar（Go 1.22）

* **没有静态错误可报**：`loopvar` 是**语义开关**（每轮新建迭代变量 vs 复用），不是版本拒绝项。所以本阶段**没有为它新增任何 `DiagCode`**，这是对的，不是漏了。
* **静态面只到「作用域 + 可赋值」层**：迭代变量在体内的作用域、`&v` 的可取性、类型可赋值性由 `T-CK-35/36` 覆盖。
* **语义不承诺（M5/M7）**：`go.mod` 的 `go 1.22` 如何切换语义、闭包捕获到的是哪一轮变量 —— **都不在 M4**。
* **纪律**：本阶段**不**因为 `go1_22_21.go` 在入选集里就对 `loopvar` 打勾 —— 那个文件测的是**语法/门控**，不是语义。

### 2.3 `unsafe`

* **静态白名单（`D-M4-8`）**：`Sizeof` / `Alignof` / `Offsetof` 的**静态求值与布局验证** + `unsafe.Pointer` 作为**类型名**的表示。
* **不承诺（M6+，`U-M4-06`）**：`unsafe.Pointer` 的**转换**、`uintptr` 往返、任何**内存访问**、`unsafe.Add/Slice/SliceData/String/StringData`。
* **正向证据**：`T-CK-44`（`rgoc-check/tests/unsafe_pkg.rs`）—— 白名单外的成员被**显式拒绝**并给稳定码，不是静默放行。
* **语料面**：3 个用 `unsafe` 的文件（`typeinstcycles`、`spec/conversions`、`examples/methods`）在入选集内；`typeinstcycles` 双侧零诊断（弱绿），另两个在 58 红里。

### 2.4 初始化边界（roadmap §M4 原文的硬要求）

`T-CK-71` 只证明**图与顺序**（`InitOrder` 与官方逐行一致，含函数体传递闭包与函数字面量体）、`T-CK-72` 只证明**环诊断**（14 行 `cycle` 与官方同位置）。
**M5 必须真的按 import → 包变量 → `init` → `main` 执行；M7 必须生成这个顺序的 startup。** 本报告明确：**静态图正确不能替代运行正确**。

---

## 3. 判定与泛型（`CH-1` / `CH-4`）—— 本阶段最扎实的部分

| 面 | 实现落点 | 证明方式 |
|---|---|---|
| 可赋值性 | `rgoc-types::assignable` | `T-CK-11`：规则序（identical → 可表示 → 底层同一且至少一侧非具名 → implements → chan/nil）逐条命中；`cmp assignable` 与 oracle 一致 |
| 方法集 | `rgoc-types::methodset` + loader 挂载 | `T-CK-14`：值/指针接收者、嵌入提升、`*T` 规则；`cmp methodset` 名字序列 |
| 接口实现 | `implements`（非约束模式） | `T-CK-12/13`：空接口恒真、V 方法须全含、指向接口的指针规则 |
| 约束满足 | `satisfies`（`constraint=true`） | 同上；`~T` / 并集 / `comparable` 面 |
| 类型集同一性 | `identical` 升级 | **`U-M3-05` 拒面被解除**：按（comparable 标志、项集、方法集）判定，且**反向合成反例**存在（异项集/异 comparable ⇒ 不同一） |
| 实例化 | `instantiate` + `subst` + memo | `T-CK-21`：同一 `(origin,args)` 得到**同一实例 id**（`Review Focus` #1 的记忆性要求）；`cmp instance` 单根 dump |
| 推断 | `infer` 三入口 + `renameTParams` | `T-CK-22/23`：1.18 实参模式、嵌套泛型类型、赋值语境；失败形态给稳定码 |
| 实例环 | 重实例化 + memo 回滚 + 深度守门 | `T-CK-25`：环被**拒绝**而非静默接受（`U-M4-09` 的正向断言） |

**纪律记录**：`U-M3-05` 的解除是 M3 白名单里唯一一条「必须解除」项，它在 `M3-tests.md` 有独立的修订行（`R-6-1`）与 `M3-manifest.json` 的 `resolved_by: M4`，`manifest_m3.rs` 2/2 绿 —— 三条链都闭合才算兑现。

---

## 4. 检查器闭环实测（`CH-2`）—— **未兑现**

### 4.1 入选 75 的实测分布

`run-collide.sh --check` 只看首个不一致，收口需要全局视图，故用逐文件侦察（`.superpowers/sdd/M4-plan/t37-survey.sh`，判据与 `C8` §6 相同：剥列后 `(行号, 类别)` 多重集相等 + 两侧 stderr 空）。

```
OK = 17 / DIFF = 58      （2026-10-22 实测，清单 tools/types-oracle/check-selected.txt）
```

**17 个绿再分强弱**（`.superpowers/sdd/M4-plan/t44-okstrength.sh` 逐文件数两侧行数）：

| 强度 | 数 | 文件 | 含义 |
|---|---|---|---|
| **强绿**（非零且逐行一致） | **9** | `check/blank.go`(1)、`conversions1.go`(47)、`decls5.go`(3)、`go1_22_21.go`(1)、`init0.go`(20)、`init1.go`(7)、`init2.go`(26)、`main0.go`(4)、`main1.go`(1) | 真的查对了 |
| **弱绿**（双侧零诊断） | **8** | `check/compliterals.go`、`go1_19_20.go`、`go1_20_19.go`、`go1_21_19.go`、`go1_21_22.go`、`go1_xx_19.go`、`literals.go`、`typeinstcycles.go` | 两侧都「什么都没说」，一致但**不构成能力证据** |

> **为什么要把这 8 个单独拎出来**：`17/75` 听起来比 `9/75` 好一倍。把弱绿算进能力面就是自欺 —— 它们只证明「rgoc 在这些文件上不乱报」，不证明「rgoc 查得出该查的」。报告里两个数都给。

### 4.2 58 个红的**形状**（按差异方向分，精确计数）

| 形状 | 数 | 含义 | 文件（示例，括号内 `oracle/rgoc` 行数） |
|---|---|---|---|
| **假阴性**（`oracle>0, rgoc=0`） | **15** | 该报的**一条都没报** | `go1_8/12/13/16/19/25`(1/0…4/0)、`typeAliases1.8/1.22/1.23`、`cycles3`(1/0)、`cycles6`(13/0)、`typeinference`(12/0)、`unions`(3/0)、`operations`(3/0)、`receivers`(1/0) |
| **假阳性**（`oracle=0, rgoc>0`） | **3** | 凭空多报 | `cycles1`(0/12)、`linalg`(0/2)、`map1`(0/4) |
| **过量**（两侧皆有，rgoc 行数更多） | **21** | 规则过粗 / 报点重复 | `typeparams`(67/105)、`gotos`(40/68)、`labels`(33/64)、`conversions`(24/59)、`inference2`(14/36)、`functions`(15/34)、`methods`(7/9)… |
| **不足**（两侧皆有，rgoc 行数更少） | **19** | 规则面缺 | `shifts`(171/42)、`stmt0`(175/156)、`issues0`(84/51)、`vardecl`(54/12)、`assignability`(35/11)、`decls0`(43/16)、`expr0`(51/34)… |

> `过量 21` 与 `不足 19` 同时存在，说明**不是「整体多报」或「整体漏报」**，而是规则面成片缺失且报点口径不一致 —— 这与 §4.3 的主题分桶吻合。

### 4.3 主题分桶（**非互斥**，一个文件可能横跨两桶）

| 桶 | 代表文件 | 缺什么 | 去向 |
|---|---|---|---|
| **语法/词法级版本门控** | `go1_8/12/13/16/19/25`、`typeAliases1.8/1.22/1.23` | `rgoc-lex` 无版本门控（下划线数字、`0b`/`0o`、十六进制浮点）；别名/泛型的**语法级** `requires go1.N` | `G-M4-39` |
| **泛型判定面** | `typeparams`、`typeinst0/1`、`typeinference`、`unions`、`constraints`、`inference`、`inference2`、`typesets`、`types` | 类型集运算、推断细节、**实例化后的方法集** | `G-M4-37` |
| **赋值/转换全矩阵** | `assignability`、`conversions`、`comparisons`、`comparable*`、`structLits`、`vardecl`、`expr0/1/2`、`stmt0/1`、`issues0`、`decls0/4`、`errors`、`slices`、`range_int` | 各形态的 `assignment`/`convertibleTo` 分支与报点 | `G-M4-37` |
| **递归接口同一性** | `cycles0..6` | 递归接口的 `identical`（`cycles1` 是**假阳性 12 条**） | `G-M4-37` |
| **类型参数核心类型** | `linalg`、`methodsets`、`shifts`、`labels`、`gotos`、`doubled_labels`、`issue70974`、`functions`、`spec/methods`、`spec/range` | `coreType` 上的运算符/移位规则、标签面 | `G-M4-37` |

### 4.4 排除清单 16 行

`tools/types-oracle/check-allowlist.txt` 16 行，逐行带理由与指针（`D-M4-3` / `U-M3-07` / `U-M4-05`）：14 个计数不等（含 `const1`/`decls1`/`lookup1`/`lookup2` 四个 `errs<markers`）+ `chans`（`import "runtime"`）+ `importC`（`-fakeImportC`）。`check_corpus.rs` 断言「候选 91 == 入选 75 ∪ 排除 16」（两向、无死行、无重复、理由含指针）。

---

## 5. 版本门控（`CH-3`）—— **partial**

**已兑现**：`F1` 特性表 **31 项**（`T-CK-61`）；`-lang` 与 `//go:build` **同源进入**（`T-CK-62/63`，语义 = `max(fileVersion, go1.21)`，`-lang` 缺省不门控）；门控位置与 soft 面（`T-CK-64`）；`range` 两档 `6:12`/`8:12` 两侧一致。

**未兑现**：语料 18+1 个版本文件里 **6 个仍 0 行** —— `go1_8`、`go1_12`、`go1_13`、`go1_16`、`go1_19`、`go1_25`。根因是**门控的接入层不对**：官方在语法/词法层就拒绝（`0b1010` 在 1.13 前、下划线数字在 1.13 前、十六进制浮点在 1.13 前、`any` 在 1.18 前……），而 `rgoc-check` 只在**语句/表达式**面查门控，这些文件在**词法层就该报**，走到检查器时已经解析成功。

> 这条最容易被误读成「版本门控没做」。准确说法：**门控表与门控语义做完了，缺的是「让门控在更早的层生效」**。`G-M4-39` 登记了去向（M5 前收口专项），与 `G-M4-37` 同批。

---

## 6. 包级规则（`CH-5`）—— 兑现

| 面 | 测试 | 关键语义 |
|---|---|---|
| 初始化依赖与顺序 | `T-CK-71`（`initorder.rs`，6 条） | `InitOrder` 逐行一致；**函数字面量体计入依赖**、**被调用函数体做传递闭包**、**裸函数体引用不计入**（三条都照官方实测抄） |
| 初始化环 | `T-CK-72`（`init_cycles.rs`，6 条） | 环上每个 var 一条 `cycle`；`T3.m`/`T5.m` 走**方法集解析**补边（`init2.go` 14 行钉死） |
| `main`/`init` | `T-CK-73`（`main_init.rs`，8 条） | `main` 无参无返回、`init` 不可引用/不可作值、重复 `main`（`Ruling 34-1/34-2`） |
| import 三形态与可见性 | `T-CK-74/75`（`imports.rs`，13 条） | `.` 合并进文件作用域 / `_` 只触发装载 / 别名覆盖默认名；`pkg.X` 先查导出性再跨包对象比对（`Ruling 35-1..3`） |
| 9 缺测试项 M4 段 | `T-CK-76`（`gaps_fixtures.rs`，6 条） | `TYP-26`、`EXP-16`（AST 形状）、`EXP-22`、`PKG-04` 零值矩阵、`PKG-06` init 环（`Ruling 36-1`） |

`G4` 四组点名 fixture 齐：`.`/`_`/别名 import（`T-CK-75`）、init cycle（`T-CK-72`）、泛型推断（`T-CK-22..24`）、方法集/字段提升（`T-CK-51/52`）。

---

## 7. M3 欠账复核（`A3`，`T-CK-A3`）—— **0 例可提升**

**对象**：`tools/types-oracle/corpus-allowlist.txt` 的 **26 行**（M3 的 15 例 DIFF + 10 例拒绝 + 1 例计数不等）。
**判据**：实例化落地后，逐例给结论 ∈ {已可 dump 对撞 ⇒ 建议 M3 修订提升 / 仍属 C7 渲染面 / 登记去向}。**不要求本阶段修改 M3 清单**。

### 7.1 26 行按根因分四组

| 组 | 数 | 行号 | 结论 |
|---|---|---|---|
| **A. 双侧都拒绝**（类型图成环，`C7` §5.1） | 4 | 1 `cycles1`、8 `issue43056`、12 `issue48695`、15 `issue50426` | 两侧都 exit 2 ⇒ **无可对撞内容**。M4 未改变这一点（环判定是 C7 的地盘） |
| **B. oracle 拒绝 / rgoc 不成环** | 6 | 2 `issue25838`、10 `issue46461`、11 `issue47887`、19 `issue52529`、22 `issue60460`、23 `issue60556` | 官方在**实例环**上报错，rgoc 的图不成环（`U-M3-04` 邻域）⇒ **无对照**。M4 有了实例化，但 rgoc 的**环判定**仍按基类型走 |
| **C. 双侧根集皆空** | 1 | 3 `issue39711`（`type _ interface{…}` —— 空标识符不是声明） | 只能证明「两侧都空」，**不构成 dump 能力证据** |
| **D. 实例作根/底层/字段/参数** | 15 | 4,5,6,7,9,13,14,16,17,18,20,21,24,25,26 | **根因已从「没有实例化」变成「dump 根集不产实例」** —— 见 §7.2 |

### 7.2 根因实证（D 组）

拿 `fixedbugs/issue65854.go`（`var _ T[int]`，根是实例）现场对撞：

```text
=== ORACLE ===            === RGOC ===
0	alias 1               0	alias 1
1	basic int             1	basic int
2	named 3               2	named 3
3	pointer #0            3	pointer #0
4	named 3          ← 实例条目缺失
```

oracle 出 **5** 条，第 4 条是 `T[int]` 的独立 `named`；rgoc 只出 **4** 条。
**实例化本身是对的**（`cmp instance` 七 kind 全绿、`T-CK-21` 的记忆性断言绿）—— 缺的是 `rgoc types --dump` 的**根集构造**：它按「包级对象 + 具名类型」取根，实例不在根集里，于是「实例化好了但没人 dump 它」。

> **这是 M4 对 A3 的真正贡献**：不是「把 26 例修绿」（它们不在 M4 的验收面内），而是把根因**从判定面挪到了渲染面**。下一个接手的人不需要再猜。

### 7.3 顺带发现（M3 遗留，非 M4 引入）

`rgoc types --dump` 的输出头里 **`# source: rgoc` 出现两次**：

```text
# source: rgoc
# files: …/issue65854.go
# package: p
# source: rgoc      ← 重复
0	alias 1
```

成因：`types_cmd.rs` 自己写了一行头，而 `rgoc_types::dump` 的正文**自带**同一行（`cmp_cmd.rs` 是 `strip_prefix` 掉它的，`types_cmd.rs` 没剥）。
`C7` 规定 `#` 行是注释、对撞两侧都忽略，所以**任何门禁都抓不到它**。已登记 `G-M4-41`（去向：M5 收口顺手修，`types_cmd.rs` 一处 `strip_prefix`）。**不在 M4 内修**：`rgoc types` 的 M3 口径按 `T-TY-83` / `T-CK-E2` 明确「行为不改」，里程碑收口期不动它。

---

## 8. wave A 处置（`T-CK-A2`，`CH-9` 的**先砍**项）

### 8.1 为什么砍

`D-M4-14` 的降级序第一刀就是「fixedbugs 后续 wave 的引入；wave A 本身的裁减」。入选 75 的 58 个红已经把检查器的主要缺口暴露完，wave A 的 30 个红**与它们是同一批未实现面**（同一个检查器、同一批规则），不构成独立信息。所以按纪律**显式降级并登记**，不是静默跳过。

### 8.2 实测

```
wave A: OK = 20 / DIFF = 30      （check-fixedbugs-waveA.txt，50 行）
```

**20 个绿同样分强弱**：

| 强度 | 数 | 文件 |
|---|---|---|
| **强绿**（非零且一致） | **3** | `issue64704`(1)、`issue79832`(1)、`issue50816`(2) |
| **弱绿**（双侧零诊断） | **17** | `issue25838`、`39711`、`39755`、`39982`、`43056`、`45550`、`46461`、`47887`、`50259`、`50276`、`50729`、`50729b`、`50779`、`52529`、`57192`、`60556`、`65854` |

> **必须说清楚**：这 17 个里有一大批正是 §7 的 A3 名单 —— 它们在 **dump 面**因「根集不产实例」不可对撞，在 **check 面**又因为「两侧都没报错」而弱绿。**同一个文件在两个面上都「绿」，但两个绿都不是能力证据。** 这是本报告最容易被数字骗过去的地方。

**30 个红的形状**：假阳性 8（`issue40038/41176/48695/50426/59338b/60460/66751/77905`，rgoc 报了 oracle 没报的）、过量 10、假阴性 4（`56351/59639/49541/49579`）、同条数但类别/行位不同 4（`39976/53692/58671/59740`）、不足 4（`51229/59956/60377/60562`）。

### 8.3 去向

30 个文件**全部**转入 wave B（`U-M4-10`），与入选 75 的 58 个红**同批处理**（`G-M4-37`）。清单本身无瑕疵（`T-CK-A1` 仍 `pass`：50 行 / 四组 25-8-13-4 / 全 EQ / 零 import / 标记 132）。

---

## 9. 反向验证（`T-CK-B1..B8`）

八条变异**全部实证变红并复原**（细节见 [`scripts/mutation-m4-notes.md`](../../scripts/mutation-m4-notes.md)）：

| 变异 | 施加点 | 红在 | 结果 |
|---|---|---|---|
| `B1` 判定做反 | `rgoc-types` 判定核心（`implements` 忽略一条方法） | `T-CK-12/13` + `--cmp` | 9/9 fail |
| `B2` 类别投影改一码 | `DiagCode`→类别 投影表 | `--check` 渲染行级差异 | 红 |
| `B3` 适配表删一条信条 | oracle 适配表 | `other` 出现 ⇒ `T-CK-94` | 红 |
| `B4` 注入多余诊断 | rgoc 诊断流 | 多重集不等（G3） | 3/6 fail |
| `B5` 打乱渲染一行 | 比对器输入 | 装置自身 diff | dump 腿红 |
| `B6` 版本门控反转 | 低档放行 | `T-CK-61/63` | 4 fail |
| `B7` 清单守卫 | 移一行 / 塞一行 | `T-CK-93` 两向相等 | 红 |
| `B8` 交换 `InitOrder` 两行 | `T-CK-71` 输出 | 顺序断言 | 5 fail |

**复原确认**：八条全部复原后 dump 腿回到 `OK(14)`、workspace 回到 1539 绿（当时点），无残留。
**两条踩坑记录**（已写进 notes，供下阶段复用）：容器内 `git checkout` 撞 `dubious ownership`（宿主 uid ≠ `/work` 属主）；`B3` 的锚点字符串凭印象写会 miss，必须先用 `rg` 定位再施加。

---

## 10. 回归与预算（`CH-7`）

### 10.1 四条 cargo（`R-9-1` 生效形态）

| 命令 | exit | 墙钟 |
|---|---|---|
| `cargo fmt --all --check` | 0 | 1.67 s |
| `cargo check --workspace --all-targets` | 0 | 2.57 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | 1.68 s |
| `cargo test --workspace --no-fail-fast` | 0 | **70.14 s** |

**`cargo test`：1541 passed / 0 failed / 0 ignored，136 个 suite 全 `ok`（含 doctest）。**

> **`R-9-1`（本次新增修订）**：`M4-tests.md` §9.1 原写的裸形态 `scripts/in-container.sh cargo …` **跑不通** —— 容器 `-w /work`（仓库根），workspace 在 `rgoc/`，`cargo` 在 `/work` 找不到 `Cargo.toml`（`fmt` exit 1、`check`/`clippy` exit 101）。生效形态是 `bash -lc 'cd rgoc && cargo …'`。**判据一字未变**（exit 0、无 warning），改的只是调用形态。这条坑 M3 期就存在，只是没写进文档。

### 10.2 五条既有自检（不回退）

| 脚本 | 结果 | 墙钟 |
|---|---|---|
| `check-m0-consistency.py` | 全部通过（231 条断言） | 0.70 s |
| `check-m1-consistency.py` | 全部通过 | 0.13 s |
| `check-ast-golden.sh` | OK（31 golden / 62 条 cargo 断言） | 1.69 s |
| `check-ast-testdata.sh` | OK（25 带 ERROR 语料 / 62 条） | 1.66 s |
| `check-ast-stdlib.sh` | OK（6670 文件分母 / 5 条 smoke） | 11.70 s |

### 10.3 对撞装置五形态（`CH-8`）

| 形态 | 命令 | exit | 结果 |
|---|---|---|---|
| ① dump | `run-collide.sh tools/types-oracle/testdata` | **0** | OK（14 fixture） |
| ② cmp | `… --cmp tools/types-oracle/testdata` | **0** | OK（7 fixture，七 kind） |
| ③ corpus | `… --corpus go_source_code/src/internal/types/testdata` | **0** | OK（31 fixture） |
| ④ check | `… --check …/testdata` | **1** | **FAIL**（首个不一致 `check/cycles0.go`） |
| ⑤ check --list | `… --check --list check-fixedbugs-waveA.txt …` | **1** | **FAIL**（首个不一致 `fixedbugs/issue40038.go`） |

装置本身无恙：两侧 stderr 空、重复运行 sha256 一致、oracle 侧 `gofmt -l` 与 `go vet ./...` 干净、语料缺失/清单行不在语料一律 exit 2（响亮失败）。**红的只有内容对齐，不是装置。**

### 10.4 预算对照（`M4-tests.md` §9.4）

| 层级 | 上限 | 实测 |
|---|---|---|
| 单文件检查 | ≤ 10 s | 秒级（`--check` 全量 75 文件 + 50 文件侦察合计 < 10 s） |
| smoke | ≤ 5 min | 约 1.5 min（热缓存） |
| milestone（75 全量） | ≤ 30 min | 同上 |
| full（四条 cargo + 五形态） | ≤ 2 h | **约 1.5 min**（热缓存；冷构建另计 ≈ 4 min） |

> 实测值全部**远低于**上限，说明本阶段的瓶颈**不是算力，是规则面没铺完** —— 预算从来没有成为砍项的理由，`D-M4-14` 的降级是**范围**决定，不是**资源**决定。这一点在 §8 里已经如实写明。

---

## 11. 差距清单与去向

| ID | 差距 | 卡住谁 | 去向 |
|---|---|---|---|
| `G-M4-37` | 入选 75 仅 17 绿（真实内容对齐 9） | `CH-2` | M5 前收口专项：按 §4.3 五桶逐桶补规则 |
| `G-M4-38` | wave A 仅 20/50（真实对齐 3） | `T-CK-A2` / `CH-9` | wave B（与 `G-M4-37` 同批） |
| `G-M4-39` | 版本门控在语法/词法层未落（6 文件 0 行） | `CH-3` | 与 `G-M4-37` 同批；需在 `rgoc-lex` 接门控 |
| `G-M4-40` | `run-collide.sh --check` 形态仍红 | `CH-8` | 根因同 `G-M4-37`；装置侧无需改 |
| `G-M4-41` | `rgoc types --dump` 头里 `# source: rgoc` 重复（M3 遗留） | 无门禁（`#` 行被忽略） | M5 收口顺手修（`types_cmd.rs` 一处 `strip_prefix`） |
| `G-1..G-9` | 开工前冻结的九项（列号升格、methodset 签名面、泛型超出面拒绝形态、`R-6-1` 行对齐、值层类型接线、`A3`、fixedbugs 全集、目录包、Cheap-trick 抑制） | 见 manifest | 见 `M4-manifest.json` 的 `open_gaps` |

`U-M4-01..11` + M3 承接 7 项 = **18 条白名单**，每条带「正向证据」（证明它确实被**显式拒绝**或走**文档约定路径**，而不是静默缺失）—— 由 `manifest_m4.rs` 对账，空话写不进去（见 §12.1）。

---

## 12. 接手指南

### 12.1 先看什么

1. **`M4-manifest.json`** —— 机器可读事实。它的 `gate` / `test_ids` / `unsupported` / `open_gaps` 由 `rgoc-driver/tests/manifest_m4.rs` **逐项对账**（35 条合成反例，每条必须指名报错）。**想改状态先跑这条测试**：它会拦住三类谎 —— 标 `pass` 却在证据里自认「未兑现」、非 `pass` 却没有缺口登记、`phase_summary` 自称 `done` 而门禁没全绿。
2. **`M4-tests.md`** —— 冻结的「怎么算通过」。改任何判据口径都要走修订表（现有 `R-4-1..7`、`R-9-1`）。
3. **本报告 §4.3 的五桶** —— 下一阶段的工作分解就是它。

### 12.2 从哪里下手（按性价比排序）

| 序 | 做什么 | 为什么先做这个 | 验证 |
|---|---|---|---|
| 1 | **赋值/转换全矩阵**（18 文件，含 `vardecl`(54/12)、`assignability`(35/11)） | 红得最集中、规则最机械、`assignable` 判定核已绿，只差**分支接线与报点** | `t37-survey.sh` 单文件 diff 收敛 |
| 2 | **类型参数核心类型**（`shifts`(171/42)、`methodsets`(12/28)、`linalg`） | `coreType` 已有底座，缺的是运算符/移位在类型参数上的规则 | 同上；`T-CK-32` 扩用例 |
| 3 | **递归接口同一性**（`cycles0..6`，含 `cycles1` 的 12 条假阳性） | 假阳性会**污染**其他文件的判定（错一次传一片） | `T-CK-15` 扩合成反例 |
| 4 | **泛型判定面**（`typeparams`(67/105)、`typeinst1`、`inference*`） | 量大但依赖 1/2 的成果，放后面 | `cmp instance` + `--check` |
| 5 | **语法/词法级门控**（6 文件） | 跨 crate（`rgoc-lex`），要动 M2 的地盘，风险最高 | `T-CK-61..64` + `go1_*` 语料 |
| 6 | `G-M4-41`（重复头行） | 一行 `strip_prefix`，顺手 | `rgoc types --dump \| head -5` |

### 12.3 不许做的事（纪律，不是建议）

* **不许放宽判据换绿**：列号仍在**渲染**里、硬判据**剥列**（`U-M4-07`）；要升格走修订，不改格式。
* **不许静默移出**：入选 → 排除只能按 `D-M4-3` 的失效条件（系统性差异**且**根因属官方实现细节/未支持面），逐行理由 + 修订 + 本报告。
* **不许把弱绿当能力**：双侧零诊断的文件必须标出来（§4.1 / §8.2 就是范例）。
* **不许把静态图当运行正确**：`InitOrder` 绿 ≠ M5 的执行序对。
* **不许跳过白名单项**：`unsupported` 里的每一条都要有**正向断言**证明它被显式拒绝，`manifest_m4.rs` 会拦。

### 12.4 复现全部结论的命令

```sh
# 门禁（R-9-1 生效形态）
scripts/in-container.sh bash -lc 'cd rgoc && cargo fmt --all --check'
scripts/in-container.sh bash -lc 'cd rgoc && cargo check --workspace --all-targets'
scripts/in-container.sh bash -lc 'cd rgoc && cargo clippy --workspace --all-targets -- -D warnings'
scripts/in-container.sh bash -lc 'cd rgoc && cargo test --workspace --no-fail-fast'

# 对撞五形态
scripts/in-container.sh bash tools/types-oracle/run-collide.sh tools/types-oracle/testdata
scripts/in-container.sh bash tools/types-oracle/run-collide.sh tools/types-oracle/testdata --cmp
scripts/in-container.sh bash tools/types-oracle/run-collide.sh --corpus go_source_code/src/internal/types/testdata
scripts/in-container.sh bash tools/types-oracle/run-collide.sh --check go_source_code/src/internal/types/testdata
scripts/in-container.sh bash tools/types-oracle/run-collide.sh --check --list tools/types-oracle/check-fixedbugs-waveA.txt go_source_code/src/internal/types/testdata

# 逐文件侦察（§4/§8 的数字来源）
scripts/in-container.sh bash -lc 'cd /work && bash .superpowers/sdd/M4-plan/t37-survey.sh'
scripts/in-container.sh bash -lc 'cd /work && bash .superpowers/sdd/M4-plan/t44-okstrength.sh .superpowers/sdd/M4-plan/ok-selected.txt /tmp/x.out'

# 既有自检
python3 scripts/check-m0-consistency.py && python3 scripts/check-m1-consistency.py
bash scripts/check-ast-golden.sh && bash scripts/check-ast-testdata.sh && bash scripts/check-ast-stdlib.sh
```

> `.superpowers/sdd/M4-plan/` 下的侦察脚本与输出**不入库**（沿 M3 的 progress 台账纪律），但脚本本身随仓库留在工作区，供下阶段直接复用。

---

## 13. 不承诺清单（沿 `unsupported`，18 条）

**M4 新增 11 条**：执行语义（`U-M4-01`）、`EXP-27` 求值顺序（`U-M4-02`）、反射/并发/`SYS-05`（`U-M4-03`）、`-goexperiment=`（`U-M4-04`）、`-fakeImportC`/cgo（`U-M4-05`）、`unsafe` 白名单外（`U-M4-06`）、列号进硬判据（`U-M4-07`）、`methodset` 签名渲染面（`U-M4-08`）、泛型超出面的拒绝形态（`U-M4-09`）、fixedbugs 全集（`U-M4-10`）、check 目录包（`U-M4-11`）。
**M3 承接 7 条**：`U-M3-06`、`U-M3-07`、`U-M3-09`、`U-M3-10`、`U-M3-11/12/13`。

**已翻转（不再是白名单）**：`U-M3-01..05`（`types --cmp` 新 kind + 判定五件套）、`U-M3-08` 的**检查面半边**（`rgoc check` 能算 `const x = len("abc")`；`rgoc types` 口径不变，`T-TY-83` 只加注记）。

---

## 14. 结论

M4 兑现了它**能自证**的部分：判定五件套与泛型三件有 `--cmp` 逐条对撞、有合成反例、有八条变异反向验证；`C8` 渲染契约、类别投影穷尽性、双腿对撞装置、两清单守卫、manifest 对账全部绿。
它**没有**兑现检查器闭环：入选 75 里真实内容对齐的只有 **9** 个，wave A 里只有 **3** 个。缺口不是零散的，是**成片**的（赋值/转换矩阵、类型参数核心类型、递归接口同一性、泛型判定面、语法级门控）。

**这是一份如实报告，不是一份完成报告。** `CH-2` 与 `CH-8` 保持 `fail`，`CH-3` 保持 `partial`，`phase_summary.status` 保持 `partial` —— `manifest_m4.rs` 会拦住任何试图把它们改成 `done` 的改动。
