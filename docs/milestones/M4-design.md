# M4 设计：静态检查闭环（表达式/语句检查 + 类型判定主体 + 泛型 + 包级静态规则）

> **阶段 ID**：`M4`　|　**状态**：**已确认**（用户 2026-10-18：「确认，继续」）　|　**起草**：2026-10-18
> **文档索引**：[`../README.md`](../README.md)　|　**上游**：[`../03-roadmap.md`](../03-roadmap.md) §M4（**权威口径**）、[`M3-design.md`](./M3-design.md)、[`M3-manifest.json`](./M3-manifest.json)　|　**下游**：`M4-tests.md`（冻结判据）→ `M4-plan.md` + `M4-manifest.json`
>
> **本文件的权威口径**：`03-roadmap.md` §M4。用户 2026-10-18 裁定：**以 `03-roadmap.md` 为完成要求**——
> `AGENTS.md` §7.1 的三样（泛型实例化 / 类型集版 interface 同一性 / 可赋值性）是 roadmap §M4 的**最小子集**，
> 不是 M4 的全量；本设计按 roadmap 全量展开（见 §1.2），并把「与窄口径的关系」写成 `D-M4-1`。
>
> **本文档已确认（2026-10-18），冻结；变更走 §12 变更记录。** 本轮头脑风暴的输入 = roadmap §M4 原文 + M3 三件套 + 本设计 §2.3 的
> 实测事实基线（全部为 2026-10-18 在 `rgoc:dev` 容器 / 本地语料上实测或源码指认，见每条出处）。

---

## 0. 本文档在流程中的位置

沿用 M1–M3 的三件套流程（`03-roadmap.md` §6.2）：

```text
M4-design.md（本文）──用户确认──► M4-tests.md（冻结「怎么算通过」）► M4-plan.md + M4-manifest.json（任务与机器可读事实）► 执行 ► 门禁
```

* 本文只回答 **做什么、边界在哪、为什么**；不写测试 ID 全表（`M4-tests.md`），不写任务顺序（`M4-plan.md`）。
* 本文的每个决策要么进 §2.1 决策表，要么进 §2.4 取舍；每条事实要么带**实测**，要么带**源码出处**，不接受推测句。

---

## 1. 目标与非目标

### 1.0 推导：为什么 M4 是「静态检查闭环」

交付线 A（`03-roadmap.md` §0.1）是 **M0–M4：`rgoc lex / parse / check`** —— M4 是 A 线的收官。此时：

* **已有**（M0–M3）：词法（`rgoc-lex`）、语法树与解析（`rgoc-ast`/`rgoc-parse`）、常量精度（`rgoc-const`）、类型表示与同一性（`rgoc-types`）、声明装载与作用域（`rgoc-load`）、`C7` 中性 dump、`tools/types-oracle` 对撞装置。
* **缺**：**消费方** —— 表达式与语句的检查。没有它，M3 显式挂起的欠账在数学上无法解除：
  * `TYP-13` 接口实现、`TYP-18` 可赋值性：都要求「值/类型在赋值、调用、返回等语境里被消费」才成立（M3 `D-M3-2` 的划界）；
  * `TYP-19` 方法集、`TYP-23..26`（约束满足/实例化/推断/统一）：随检查器与泛型一起来；
  * `U-M3-05`（约束接口同一性）：**M4 必须解除**（`M3-manifest.json`）；`U-M3-08`（需类型信息的 `const` 初始化式）同理。
* **M4 之后**，Go 的「静态面」闭环：一份**受支持子集**内的 Go 源码，从字节进来，接受/拒绝判断与错误位置、类别都能与官方 `go/types` 对上。**执行语义（解释）与机器码不是 M4 的承诺**（那是 M5 / M6+）。

### 1.1 M4 的输入与输出

| 类 | 内容 |
|---|---|
| 输入 | M3 七条正式 crate（`lex → ast → parse → const → types → load → driver`）；C7 契约；`tools/types-oracle`（dump/cmp/语料三形态）；`go_source_code` 语料（只读）；`src/internal/types/testdata/{check,spec,examples,fixedbugs}` 三目录 + fixedbugs 分批 |
| 输出 ① | **`rgoc-check` 新 crate**：检查器主体（操作数/表达式/语句/内建/选择器/版本特性门/初始化顺序/main/unsafe 白名单） |
| 输出 ② | **判定五件套**进 `rgoc-types`：可赋值性、方法集、接口实现（+约束满足）、类型集同一性（解除 `U-M3-05`）、可比较性与转换（静态面） |
| 输出 ③ | **泛型三件**：统一（`unify`）、推断（`infer`）、实例化（`instantiate`）——`rgoc-types` |
| 输出 ④ | `rgoc-driver check` 子命令（人读诊断 + `--diag-dump` 对撞渲染）；`types --cmp` 新判定式；`rgoc types` 的 M3 契约**保持不变** |
| 输出 ⑤ | `tools/types-oracle --check` 模式（含 lang 推导、`unsafe` importer、类别适配表）；`run-collide.sh --check` 形态；语料两清单（入选/排除） |
| 输出 ⑥ | **新契约 `C8`（check 对撞渲染与诊断类别）**；`M4-tests.md` / `M4-plan.md` / `M4-manifest.json`；**交付 A 报告 `M4-report.md`**（前端子集矩阵） |
| 输出 ⑦ | 对 M3 文档的**修订入档**：`M3-tests.md` §6 修订表、`M3-manifest.json` 对应项、受翻转影响的 M3 测试改写（见 `D-M4-11`） |

### 1.2 M4 范围（以 `03-roadmap.md` §M4 为权威，逐句展开）

roadmap §M4 原文列举（下划线为原文短语）：

| # | roadmap §M4 句子 | M4 落点（本设计） |
|---|---|---|
| 1 | 「**表达式/语句检查**」 | `rgoc-check`：操作数模式（常量/变量/类型/无值）、表达式类型计算、语句静态规则（赋值性、控制流合法性、终止语句、switch/select 静态约束、go/defer 静态约束、return 规则、fallthrough 规则） |
| 2 | 「**方法集/字段提升**」 | `rgoc-types::methodset` + `rgoc-check::lookup/selection`（深度最小者胜、同级歧义=错、方法表达式/方法值形态） |
| 3 | 「**接口实现/断言静态约束**」 | `rgoc-types` implements/约束满足（`check.implements` 语义面）+ 类型断言（`v.(T)`）的静态合法性（非接口必须先到接口、不能同时含 P 方法等） |
| 4 | 「**内建签名**」 | `rgoc-check::builtins`：18 个预声明 + `unsafe` 白名单成员的签名与静态约束（含常量折叠：`len/cap/Sizeof/Alignof/Offsetof` 等） |
| 5 | 「**泛型实例化/推断**」 | 显式实例化（`F[T1,…]`/`T[int]`）、函数实参推断（1.18）、赋值/接口方法语境推断（1.21）、赋值上下文函数类型推断（1.27）；统一算法（exact/loose/assign 模式） |
| 6 | 「**版本门控**」 | 检查器层 `verifyVersionf/allowVersion` 全表（§2.3 `F1`）；`// -lang=` 首行指令 + `//go:build` 文件版本，与其他输入同源 |
| 7 | 「**main 约束**」 | `func main`/`func init` 的签名与引用约束（按 `go/types` 实测面；compiler 独有面在报告里声明外置） |
| 8 | 「**初始化依赖/循环**」 | 包级变量/常量初始化的**依赖图与顺序**（`InitOrder` 对撞）+ **环诊断**；**不执行** |
| 9 | 「**导出与多包可见性**」 | `pkg.X` 的导出性判定与跨包对象比对（修正 M3 `open_gaps`「跨包引用解析不了」「`UndeclaredImportedName` 不做」）+ `.`/`_`/别名 import 三个形态 |

**roadmap §M4 门禁原文 → 本设计的兑现方式**：

| # | 门禁短语 | 兑现方式 |
|---|---|---|
| G1 | 「check/spec/examples **按支持矩阵精选**的接受与拒绝用例全绿」 | 三目录**全量机械扫描**（§2.3 `F10` 已预扫）→ 入选/排除两清单（逐行理由）→ 入选文件走「双腿判据」（§7.2） |
| G2 | 「纯前端 errorcheck **对齐错误位置/类别**」 | 对撞渲染 = `<行号>\t<类别>`（§4.1 / `C8`）；两侧各自把诊断投影到**同一类别枚举**（§3.6） |
| G3 | 「**额外错误可检测**」 | 判据是**多重集相等**（不是包含）：多一条即红；类别适配表的 `other` 缺省在入选语料上出现即红（§7.2 守卫） |
| G4 | 「`.`/`_`/别名 import、init cycle、泛型推断**有自有 fixture**」 | 四组自有 fixture 进 `M4-tests.md` §3 表（每组≥1 个 `T-CK-*` ID） |
| G5 | 「fixedbugs **按依赖逐批引入**，不一次性门禁全集」 | fixedbugs 分 wave（起步 wave A：与「被解除项」直接相关的 issue 文件，逐批扩），**不进**首轮门禁分母 |

**roadmap §M4 其余原文**：

* 「**初始化边界**：此处只算图和顺序；M5 必须执行 import→包变量→init→main；M7 必须生成该顺序的 startup，不能以静态图正确替代运行正确。」
  ⇒ 本设计 §3.5 只承诺「图 + 顺序 + 环诊断」；`M4-report.md` 明写此边界（不许把静态图当运行正确）。
* 「**交付 A**：可发布前端子集报告。range over func、loopvar 和 `unsafe` 的静态/执行范围分别声明，不因语法可解析而标记运行语义完成。」
  ⇒ `D-M4-13`：`M4-report.md` = 前端子集矩阵（按功能 ID 四维记状态）+ 三条**分别声明**：
  * **range over func（1.23）**：静态面纳入（语法形态、迭代变量类型、约束）——**执行面不承诺**（M8）（见 `D-M4-12`）。
  * **loopvar（1.22）**：**无静态错误**（不是版本拒绝项；语义开关）——静态面只到「作用域/可赋值」层，**语义不承诺**（M5/M7）。
  * **`unsafe`**：静态白名单 = `Sizeof/Alignof/Offsetof`（+ `unsafe.Pointer` 类型名表示）——转换/`uintptr` 往返/内存访问**均不承诺**（M6+ 另列条件）。
* 「**砍项**：fixedbugs 扩展及未冻结语义；保留支持子集的接受/拒绝和初始化门禁。」
  ⇒ 预算吃紧时的**降级序**（§6.3）：先砍 fixedbugs 后续 wave，再砍 check 语料里的高难文件（→排除清单+理由），**不砍**：入选集的接受/拒绝 + 位类对齐、版本门控、init 顺序/环、自有三组合 fixture。

### 1.3 明确不做（M4 边界）

以下项**不进 M4 任何验收面**（在报告 / `unsupported` 里如实登记去向）：

| 项 | 去向 | 依据 |
|---|---|---|
| 执行语义（求值、短路、range 随机起点、goroutine、defer LIFO 等一切的**运行时**面） | M5 | roadmap §0.1 交付线 B |
| `EXP-27` 求值顺序（codegen 面） | M6+（codegen） | roadmap §5 归属表 |
| native / ABI / GC / 并发 | M6+ | roadmap §0.1 |
| 构建约束**筛选**、`_test.go`/cgo/汇编（`U-M3-06`） | M4 之后 | M3-manifest（`M4+`） |
| 官方二进制 export data、模块下载、`go.mod`（`U-M3-07`） | M4 之后 | M3-manifest（`M4+`）；M4 只做**源码 import** 的可见性面 |
| 标准库 import（`import "fmt"` 等） | M4 之后（**`unsafe` 例外**：见 `D-M4-8`） | `U-M3-07` 沿革 |
| `//go:` 未知指令的**裁定**（`U-M3-09`） | noder 等价阶段 | M3-manifest |
| 诊断消息**逐字**一致（`U-M3-10`） | 不承诺 | M1 `D-M1-9` 沿用 |
| `unsafe.Pointer` 转换、`uintptr` 往返、内存访问、`unsafe.Add/Slice/SliceData/String/StringData` | M6+ | roadmap §0.2「分阶段支持」 |
| loopvar 的运行时语义、range-over-func 的执行 | M5/M8 | 交付 A 的分别声明（§1.2） |
| 反射、并发、`SYS-05` 运行时错误集 | M8+ | roadmap §5 |

### 1.4 M4 退出条件（逐条可判定；门禁编号暂名，`M4-tests.md` 定稿）

| # | 退出条件 | 判据 |
|---|---|---|
| CH-1 | **判定五件套全绿**：可赋值性 / 方法集 / 接口实现 / 约束满足 / 类型集同一性 | 自有 fixture（`S`）+ `types --cmp` 新判定式与 oracle 逐条一致（`O`）；`U-M3-05` 的拒面**被解除**且反向探针（合成反例）存在 |
| CH-2 | **检查器接受/拒绝闭环**：check/spec/examples 入选文件**全绿** | 双腿判据（marker 判定 + 位类对齐），零容忍（多/少/错类均红）；排除清单每行有理由 |
| CH-3 | **版本门控全绿** | `verifyVersionf/allowVersion` 全表自有 fixture + `go1_*.go` 语料文件；`// -lang=` 与 `//go:build` 两侧同源 |
| CH-4 | **泛型闭环** | 显式实例化 + 三类推断 + 统一模式的自有 fixture；`check/{typeinst0,typeinst1,funcinference,typeinference}.go` 与 `spec/*` 相应文件在入选集；语料 15 例 DIFF 的根因（`U-M3-04`）复核 |
| CH-5 | **包级静态规则** | init 依赖序（`InitOrder` 对撞）+ init 环 + main/init 约束 + 导出可见性 + `.`/`_`/别名 import：各有趣 fixture（`S`）+ oracle 对撞（`O`） |
| CH-6 | **M3 白名单翻转同步完成** | `M3-tests.md` §6 修订入档；`M3-manifest.json` 对应项更新；受影响 M3 测试改写后 workspace 全绿；`manifest_m3.rs` 对账仍绿（含新修订语义） |
| CH-7 | **四条 cargo 命令 + 全量回归** | `fmt --check` / `check` / `clippy -D warnings` / `test --workspace --no-fail-fast`；M0/M1/M2 门禁脚本不回退（`check-m0-consistency.py` 等） |
| CH-8 | **oracle 与对撞装置全绿** | `run-collide.sh`：dump / `--cmp` / `--corpus` / `--check` 四形态；oracle 侧 `go test ./...` + `gofmt` 干净；两侧 stderr 空、重复运行确定 |
| CH-9 | **反向验证 + 交付 A** | 变异探针 ≥ 8 条（覆盖新判据面）；`scripts/mutation-m4-notes.md`；`M4-report.md`（前端子集矩阵 + 三条分别声明） |

---

## 2. 已确认决策记录

### 2.1 决策一览

| ID | 决策 | 结论 | 来源 | 日期 |
|---|---|---|---|---|
| D-M4-1 | 完成要求口径 | **以 `03-roadmap.md` §M4 全量为准**；`AGENTS.md` §7.1 三样是最小子集 | 用户 | 2026-10-18 |
| D-M4-2 | 检查器与判定的落点 | 新 crate `rgoc-check`（检查器）+ `rgoc-types` 扩展（判定五件套 + 统一/推断/实例化）；driver 增 `check`；`types` 契约冻结 | **我方决定** | 2026-10-18 |
| D-M4-3 | 「位/类对齐 + 额外错误可检测」的判据形态 | **双腿**：① marker 判定（官方语义直译）；② 实际诊断位类对齐（两侧实跑，行号多重集 + 类别多重集**相等**） | **我方决定** | 2026-10-18 |
| D-M4-4 | 诊断类别体系 | 自定**有限类别枚举** + 两侧映射（rgoc 全量穷尽；oracle 适配表 + 未命中响亮失败）；不承诺消息逐字 | **我方决定** | 2026-10-18 |
| D-M4-5 | 泛型深度 | 照 `go/types` 算法面实现（unify/infer/instantiate）；验收面 = 语料精选 + 自有 fixture；超出面显式拒绝 + 登记 | **我方决定** | 2026-10-18 |
| D-M4-6 | 版本门控归属与面 | 检查器层实现全表；语料 `go1_*.go` 纳入门禁；`-lang` 首行 + `//go:build` 同源 | **我方决定** | 2026-10-18 |
| D-M4-7 | 初始化边界 | 只算**图与顺序** + 环诊断（`InitOrder` 对撞）；执行归 M5 | **我方决定** | 2026-10-18 |
| D-M4-8 | `unsafe`/SYS 子集 | 静态白名单 = `Sizeof/Alignof/Offsetof` + `unsafe.Pointer` 类型名；sizes 表 = arm64 `StdSizes{8,8}`；其余显式拒绝 | **我方决定** | 2026-10-18 |
| D-M4-9 | 契约 | 新建 `C8`（check 对撞渲染与类别）；`C7` **预期不修订**（如实测需要走修订表 + 重刷 `.want`，列为风险） | **我方决定** | 2026-10-18 |
| D-M4-10 | 语料纪律 | 三目录全量机械扫描 → 入选/排除两清单（逐行理由）；fixedbugs 分 wave；候选=全体、入选+排除=候选 | **我方决定** | 2026-10-18 |
| D-M4-11 | M3 翻转的同步义务 | `U-M3-01/02/03/04/05/08` 翻转时：修订 `M3-tests.md` §6、更新 `M3-manifest.json`、改写受影响 M3 测试；`rgoc types` 的 M3 口径**保持**（翻转发生在 `check`/`cmp` 面） | **我方决定** | 2026-10-18 |
| D-M4-12 | 报告与声明 | `M4-report.md`：前端子集矩阵 + range/loopvar/unsafe 三条**分别声明** | **我方决定** | 2026-10-18 |
| D-M4-13 | 反向验证纪律 | 沿用 M3（产物陈旧陷阱、`cargo clean -p`）；新增变异点：类别适配表未命中守卫、额外错误检出、比对器本身 | **我方决定** | 2026-10-18 |
| D-M4-14 | 夹具 ↔ 测试 ID 机制化 | 从 M4 新夹具起：夹具带 `// T-CK-nn` 注释 + 一条「每个 `O` 标记 ID 指得到夹具」断言；M3 旧夹具**不动**（避免重刷 `.want`），旧 ID 用一张映射表断言覆盖 | **我方决定** | 2026-10-18 |
| D-M4-15 | 判定式扩展面 | `types --cmp` 增 `assignable` / `implements` / `satisfies` / `methodset` / `instance`；`infer`/`unify` **不设** cmp 形式（走自有 fixture） | **我方决定** | 2026-10-18 |

### 2.2 五字段记录（对每个**非显然**决策展开）

#### D-M4-1　完成要求：roadmap §M4 全量

| 字段 | 内容 |
|---|---|
| **判据** | 用户原话（2026-10-18）：「以 `docs/03-roadmap.md` 为完成要求」。roadmap §M4 的句子包含九大项（§1.2 表）与五条门禁；`AGENTS.md` §7.1 只列了其中三样 |
| **候选** | ① 按 `AGENTS.md` §7.1 窄口径（泛型/类型集同一性/可赋值性）② 按 roadmap §M4 全量 ③ 折中（泛型为主 + 部分静态面） |
| **结论** | **②**。M4 = 「静态检查闭环」：表达式/语句检查、方法集/字段提升、接口实现/断言、内建签名、泛型（实例化/推断/统一）、版本门控、main 约束、初始化依赖/循环、导出与多包可见性；门禁按 §1.2 G1–G5 兑现 |
| **否决理由** | ① 会把 A 线收官缩水成「类型判定散件」：`rgoc check` 不存在时，「静态检查闭环」无从谈起；且 M3 的 `U-M3-01..04/08` 是**声明过要 M4 解除的**，窄口径无法全部覆盖 |
| **失效条件** | 无（用户裁定）。若执行中实测发现某一项**在 M4 预算内不可达**（尤其 fixedbugs 与高难语料文件），按 §1.2 砍项降级序**显式**砍，并把砍掉的面写进 `M4-report.md` 与 `unsupported`——**不静默缩水** |

#### D-M4-3　双腿判据的形态

| 字段 | 内容 |
|---|---|
| **判据** | roadmap §M4 门禁 G2/G3：「对齐错误位置/类别」「额外错误可检测」。官方 `check_test.go` 的判据是「每条实际错误必须匹配某条 `// ERROR`（子串/正则）+ 每条标记必须被消耗 + 列号 colDelta=0」；而本仓**不承诺消息逐字**（`U-M3-10`）⇒ 直接照搬 marker 匹配不可行（且 marker 的 regexp 指的是**官方消息文本**） |
| **候选** | ① 照官方 marker 匹配（要求消息复刻——推翻 `U-M3-10`，否决）② 只比「接受/拒绝」二值（太弱：G2/G3 落空）③ **双腿**：marker 判 verdict + 实跑-实跑比位类 |
| **结论** | **③**。腿①：marker 判（有标记⇒必须拒、无标记⇒必须接；官方语义直译，包含「注释在行尾/块注释定位到下一词元」等规则——M2 已实现过一次，`rgoc-parse/tests/test_oracle.rs` 的 `errorcheck()`）。腿②：**oracle 实跑 vs rgoc 实跑**，两侧把诊断渲染为 `<行号>\t<类别>` 排序行，**逐字节一致**（渲染规则进 `C8`）；两侧都复刻官方的**过滤规则**：`: \t` 开头的次级错误过滤（`F2`）、`assert/trace` 注入（`F13`）、首行 `-lang`（`F2`）、parse 错误参与（`F2`） |
| **否决理由** | ① 与 `U-M3-10` 冲突且把 M4 变成消息复刻工程；② G3「额外错误可检测」变成无判定；③ 的代价：需要类别适配表（`D-M4-4`）与「抑制/级联差异」的选择纪律（见 §7.2 风险） |
| **失效条件** | 若腿②在入选文件上出现**系统性**列/行差异而根因是「官方实现细节」而非我们缺行为（例如官方对某形态的抑制我们做不到）⇒ 该文件进**排除清单**（逐行写理由），并把差异类型登记进 `M4-report.md`；**不许**放宽容差换绿色 |

#### D-M4-4　诊断类别体系（也是 `C8` 的核心）

| 字段 | 内容 |
|---|---|
| **判据** | G2 要求「对齐**类别**」，但官方诊断**没有类别编码**（`errors.go` 有 Code 但仅部分错误带、且是内部枚举）；`03-roadmap.md` §3.4 原文：「官方诊断无统一类别编码时通过明确的适配表/自有语义 fixture 判定，**未知类别不强行归类**」 |
| **候选** | ① 用官方内部 Code（不可得/不稳）② 消息 regexp 适配表 → 类别 ③ 自定类别枚举 + 两侧映射（rgoc 由 `DiagCode` 全量投影；oracle 由适配表投影） |
| **结论** | **③**。类别是**两边共用的有限枚举**（v1 草案见 §3.6，`M4-tests.md` 冻结）；两侧映射都**必须全量**：rgoc 侧=编译期穷尽 match + 计数断言；oracle 侧=适配表 + 「选中语料上每条消息都命中非 `other` 类别」的守卫测试（未命中⇒**响亮失败**）。`other` 在入选语料上出现 ⇒ 红 |
| **否决理由** | ① 不是稳定公共 API；② 消息 regexp 面太宽（`builtins0.go` 单文件 287 条标记），维护即复刻 |
| **失效条件** | 若某类诊断**本质上无法**投影（例如同一消息涵盖两种语义）⇒ 拆类别或拆文件进排除清单；`other` 只允许在**排除清单**的文件里出现 |

#### D-M4-5　泛型深度

| 字段 | 内容 |
|---|---|
| **判据** | roadmap §M4 明列「泛型实例化/推断」；`TYP-26` 是 9 个缺测试项之一（自有 fixture 义务）；M3 的 15 例语料 DIFF 根因单一（实例化） |
| **候选** | ① 只做显式实例化（不做推断）② 照 `go/types` 全算法（unify 三模式 + infer 1.18/1.21/1.27）③ 全算法 + 不设边界 |
| **结论** | **②**，并**显式保留超出面**：算法以 `go/types`（`unify.go`/`infer.go`/`instantiate.go`/`subst.go`）为基准实现；验收面 = 入选语料 + 自有 fixture；任何未支持的形态（如某种类型参数/别名组合）⇒ 稳定 `DiagCode` 显式拒绝 + 登记，不静默接受也不静默漏报 |
| **否决理由** | ① 推断是 M4 门禁 G4 的点名项（「泛型推断有自有 fixture」）、也是语料 `typeinference.go`/`inference*.go` 的主轴；③ 「全算法 + 不设边界」= 无退出条件 |
| **失效条件** | 若推断实现后的对撞显示**算法正确但 oracle 侧行为依赖版本/实验开关**（如 `GOEXPERIMENT`）⇒ 以「两侧同开关」为口径并登记到 `C8` |

#### D-M4-8　`unsafe`/SYS 子集

| 字段 | 内容 |
|---|---|
| **判据** | roadmap §0.2「`unsafe` 初始白名单仅规划 `Sizeof/Alignof/Offsetof` 的静态求值/布局验证；`unsafe.Pointer` 转换、`uintptr` 往返及内存访问**须另列支持条件**」；§M4 交付 A 要求「`unsafe` 的静态/执行范围**分别声明**」 |
| **候选** | ① 完全不碰 unsafe（连 `Sizeof` 也不做）② 静态白名单三件 + `Pointer` 类型名 + sizes 表 ③ 把 `unsafe` 全部静态规则都做 |
| **结论** | **②**。`unsafe.Sizeof/Alignof/Offsetof` 是 `go/types` 的**内建**（`F8`，常量，`uintptr` 类型）；sizes 表 = `StdSizes{WordSize:8, MaxAlign:8}`（arm64，`F9`）。`unsafe` 作为**特殊包**：`rgoc-check` 直接建模（不走源码 import）；其余成员（`Add/Slice/...`）与 `Pointer` 转换**显式拒绝**（稳定诊断） |
| **否决理由** | ① 放弃一个**官方内建**面（对应语料 `sizeof_test.go` 一族），且 `PKG-04` 零值/布局面也连坐；③ 把 M6 的运行时承诺提前进来（`uintptr` 往返的执行正确性无法在 M4 证明） |
| **失效条件** | 若入选语料的某文件**必然**需要 `Add/Slice` 静态面才能通过 ⇒ 要么扩白名单（走修订、更新报告），要么该文件进排除清单 |

#### D-M4-11　M3 翻转的同步义务（「翻转发生在哪一面」）

| 字段 | 内容 |
|---|---|
| **判据** | `AGENTS.md` §7.1：「M4 若把某条 `U-M3-xx` 翻成『做』，**必须同步改 manifest 与 `M3-tests.md`**，否则那条测试会红（这是设计意图）」；`rgoc-driver/tests/manifest_m3.rs` 的集合相等断言仍在 |
| **候选** | ① 就地改 M3 文档与测试不留痕 ② 只更新行为、文档不动（必红） ③ **修订入档 + 定向改写** |
| **结论** | **③**。原则：**M3 文档写的是 M3 时刻的事实**——不重写历史，用**修订表/追加注记**：<br>• `U-M3-01..04`：`types --cmp` 新判定式**开始作答** ⇒ `T-TY-82` 改写为「新 kind 作答 + 未知 kind 仍拒」；`M3-tests.md` §6 各行加修订指针。<br>• `U-M3-05`：`identical` 升级 ⇒ `T-TY-81` 的拒面改写为**解除断言**（两侧匿名约束接口按类型集判对 + 反向合成反例）；`M3-manifest.json` 的 `unsupported` 项更新为「已由 M4 解除（2026-10-XX）」+ `open_gaps` 对应行关闭。<br>• `U-M3-08`：解除发生在 **`rgoc check`**（检查器接线后 `const x = len("abc")` 合法）；**`rgoc types` 的 M3 口径保持**（装载入口语义不变）⇒ `T-TY-83` **不改行为**，但 §6 注记「M4 由 `check` 面解除」。<br>• `open_gaps`：「跨包引用解析不了」「`UndeclaredImportedName`」「`-I` 根指错与不支持 import 同码」「语料 15 例 DIFF」「语料 10 例两侧拒绝」「`-lang` 特性面」逐条对应 M4 任务或登记去向 |
| **否决理由** | ① 制造两份历史、对账脚本失明；② 违反仓库纪律（红测试不许绕过） |
| **失效条件** | 无。改完必须 `cargo test -p rgoc-driver --test manifest_m3` 全绿 + workspace 全绿（CH-6） |

### 2.3 事实基线（全部为 2026-10-18 实测或源码指认）

> 记号：**【测】**= 本日在 `rgoc:dev`（go1.27.1 linux/arm64）实测；**【源】**= 语料源码指认（文件:行）。
> 所有【测】数据可由 `.superpowers/sdd/M4-design/probe/`（本次设计期探针，不入库）重放。

| # | 事实 | 出处 |
|---|---|---|
| F1 | **版本门控全表**（检查器层，都在 `go/types`）：别名声明=1.9（`decl.go:524`）｜有符号移位计数=1.13（`expr.go:664`）+ 数字字面量=1.13（`literals.go:23`）｜`comparable` 嵌入语义=1.14（`typeset.go:243`）｜`unsafe.Add/Slice`=1.17（`builtins.go:737/869`）+ slice→array 指针转换=1.17（`conversions.go:219`）｜**泛型全家=1.18**：类型参数（`decl.go:561`）、函数/类型实例化（`call.go:44/301`、`signature.go:307`、`typexpr.go:277`）、导入约束（`decl.go:508`）、类型集元素/嵌入（`typeset.go:276/285/299`）、`any/comparable` 预声明（`typexpr.go:34`）｜slice→array 转换=1.20（`conversions.go:206`）+ `unsafe.SliceData/String/StringData`=1.20（`builtins.go:891/908/928`）｜`clear/min/max`=1.21（`builtins.go:234/587`）+ 隐式实例化函数作实参=1.21（`call.go:617`）｜**range over int=1.22**（`range.go:227`）｜**range over func=1.23**（`range.go:242`）+ 泛型别名=1.23（`decl.go:521`）｜`new(expr)`=1.26（`builtins.go:672`）｜嵌套字段 key=1.27（`literals.go:194`）+ 泛型方法=1.27（`resolver.go:448`）；同特性内层站点另有 `call.go:90/396/560`、`instantiate.go:316`、`resolver.go:427`、`range.go:69` | 【源】grep `verifyVersionf`/`allowVersion` 全站点 |
| F2 | **官方 harness 语义**：① 首行指令 `// -lang=go1.19` 由 `parseFlags`（`check_test.go:108` 起，首行是 `//` 注释且以 `-` 起）解析；② `// ERROR "..."`=**子串**匹配、`// ERRORx "..."`=**正则**（`check_test.go:237-257`）；③ 每条实错必须被消耗、每条标记必须被消耗（多报=红，少报=红），列号 `colDelta=0`（`:282`）；④ `": \t"` 的**次级/上下文错误被过滤**（`:169`）；⑤ parse 错误**参与**匹配；⑥ `shouldTest` 按构建标签（GOOS/GOARCH/release tags）跳文件；⑦ 匹配坐标 = **行**（行注释）+ 下一个词元的列（块注释） | 【源】`go/types/check_test.go` |
| F3 | **无错误上限**：`go/types` 不截断错误；错误经 `Config.Error` 回调，`types.Error{Pos, Msg, Soft}`；**Soft 有实义**：range 门控报的是 soft 错误（`cannot range over 3: requires go1.22 or later`，`s1`） | 【源】grep 无 `too many errors`；【测】B/C/D 探针 |
| F4 | **init 顺序与环**：`Info.InitOrder` = 依赖排序后的 `[]*Initializer{Lhs vars, Rhs expr}`（实测 `init [b] <- 2; init [a] <- b + 1; init [c] <- a`）；环 = `initialization cycle for a` + 次级链 `a refers to b`/`b refers to c`/`c refers to a`（**次级被 `": \t"` 规则过滤**，与 F2④ 同源） | 【测】E/F 探针；【源】`initorder.go` |
| F5 | **接口同一性 = 类型集比较**：`comparable` 标志相等 + `terms.equal` + 方法集（按 Id 排序、顺序无关）；防环靠 `ifacePair` 栈；两侧先 `Unalias`。M3 已落方法集版；**terms/comparable 面 = M4 解除 `U-M3-05` 的工作量** | 【源】`predicates.go:393-450` |
| F6 | **可赋值性规则序**（`operand.go:328+`）：identical →（untyped）可表示 →（底层同一且至少一侧非具名、且非类型参数）→ 接口 implements（含指向接口的指针、类型参数待定分支）→ 通道方向/nil 等后续规则；失败带 `cause` 串。**implements**（`instantiate.go:243+`）：空接口恒真；**空类型集的一方恒满足**；V 有方法须全含；`constraint` 模式换动词 `satisfy` | 【源】`operand.go`、`instantiate.go` |
| F7 | **方法集**：基准 = `go/types/methodset.go`（T 值接收者；`*T` 含指针接收者；嵌入提升按**最小深度**、同名同深=冲突不进集合）。字段提升同族规则在 `lookup.go`（选择器） | 【源】文件清单；细节留 RED 阶段逐条实测 |
| F8 | **`unsafe` = 内建**（`builtins.go`）：`Sizeof/Alignof/Offsetof` 返回**常量**（实测 `unsafe.Sizeof(int64(0))` ⇒ 值 `8`、类型 `uintptr`）；`std` `go/types` 下 `unsafe` 走 **Importer**（默认缺 importer 时 `could not import unsafe`），`types.Unsafe` 可直接返回 | 【测】H 探针；【源】`universe.go:165+` |
| F9 | **sizes 表**：`StdSizes{WordSize: 8, MaxAlign: 8}`（arm64）；`Alignof/Offsetsof/Sizeof` 规则在 `sizes.go`（结构体字段对齐/尾部填充） | 【源】`sizes.go:48+`；【测】sizeof=8 |
| F10 | **三目录预扫（本设计实测）**：`check/` 顶层单文件包 **70** 个（另有目录包如 `decls2/` 未计）：accept+0 标记 **10**、accept-but-markers **5**、parse-fail **5**、reject+markers **48**（其中 errs==markers 的「计数 EQ」**21**）、reject-no-markers **2**；`spec/` **13** 个：reject 10（9 EQ）、accept-but-markers **2**、parse-fail **1**；`examples/` **8** 个：reject 6（4 EQ）、parse-fail **2**。关键观察：**大量文件 errs 与 markers 精确相等**（`expr0.go` 51=51、`assignability.go` 35=35、`typeparams.go` 69=69、`typeinference.go` 12=12、`unions.go` 3=3 等）⇒ 位类对齐**在大量文件上可达**。异常两例已解：`chans.go` 需 import `runtime`（stdlib 依赖 ⇒ 排除理由）；`literals.go` 依赖 `assert` 内建（见 F13） | 【测】sweep 探针（输出存 `.superpowers/sdd/M4-design/probe/sweep-20261018.txt`，可重放） |
| F11 | **文件版本读取**：`go/parser` 自动把 `//go:build go1.22` 填进 `ast.File.GoVersion`（实测 `"go1.22"`）；`go/types` 侧：`pkgVersion=conf.GoVersion`（高于工具链 ⇒ `TooNew` 报错），per-file `v = max(fileVersion, go1.21)`；`check.version` **逐文件切换** | 【测】G 探针；【源】`check.go:319-349`、`check.go:41` |
| F12 | **缺省 `GoVersion=""`**：实测默认运行下泛型/range 门控**不**触发版本错（即被解释为当前工具链版本或等价语义）；精确缺省规则在 RED 阶段钉死（不得推断） | 【测】B/C/D 对照（`--lang` 显式时才触发门控） |
| F13 | **`assert`/`trace` 是测试内建**（不是预声明）：`DefPredeclaredTestFuncs()` 注入（`universe.go:222-240`）；官方 harness 在测试二进制里注入 ⇒ `check/literals.go`（68 个 `assert` 引用）官方**通过**；不注入时 68 个 `undefined: assert`（本设计亲历）**⇒ 对撞装置必须注入，且 rgoc 侧要有等价的开/关** | 【源】+【测】sweep + 官方 `go test go/types -run TestCheck/literals` **PASS**（容器内实测） |
| F14 | **语料目录结构**：官方 std `go/types` 测试**直接读** `../../internal/types/testdata/{check,spec,examples,fixedbugs}`（`go/types/check_test.go:414-420`）；目录包（`decls2/` 等）按「多文件单包」跑；`fixedbugs/` **274** 个文件 | 【源】+【测】 |
| F15 | **oracle 版本一致**：容器 `go version` = `go1.27.1 linux/arm64`，与快照 `VERSION` 一致 ⇒ `go/types`（标准库）即官方实现基准，无需 vendor（沿 `D-M3-10`） | 【测】 |
| F16 | **`rgoc-types` 现状**：`identical.rs:189-196` 的 `TypUnsupportedConstraintIdentity` 拒绝点 = `U-M3-05` 的物理位置（M4 要改）；`Type` 枚举 15 变体（`basic/named/alias/pointer/slice/array/map/chan/struct/signature/tuple/interface/typeparam/union/invalid`，`C7` 标签集一一对应） | 【源】`rgoc-types/src/identical.rs`、`M3-tests.md §5.2` |
| F17 | **cmp 装置现状**：`//rgoc:cmp <kind> <A> => <B>`（带空格操作数）或空白二分形态（`R-32-1`）；`kind` 集合**封闭**，未知 kind ⇒ 拒绝；输出 `<行号>\t<kind>\t<result>`，`result ∈ true/false/reject`；两边读**同一份** fixture | 【源】`tools/types-oracle/testdata/cmp-basic.go`、`M3-tests.md §5.2` |
| F18 | **对撞脚本现状**：`run-collide.sh` 三形态（dump=fixture 目录 / `--cmp` / `--corpus` 清单）；纪律=两侧非空 + stderr 空 + 重复运行确定 + `#` 注释行不进对撞；`corpus-selected.txt` 为**唯一**清单（脚本不重算筛） | 【源】`tools/types-oracle/run-collide.sh` |

### 2.4 关键取舍（不逐条进五字段，直接采纳）

1. **检查器本体新开 crate（`rgoc-check`）而不是塞进 `rgoc-load`**：`load` 已冻结「装载/声明/作用域」的口径（M3 的 23 条测试与驱动契约），检查器是**新的语义层**；依赖方向 `load → check → driver` 单向。判定五件套仍留 `rgoc-types`（纯类型代数，无 AST 依赖），把「谁消费」与「判什么」分开。
2. **不追求消息逐字**（沿 `U-M3-10`），类别枚举 + 位类对齐代替；代价=适配表维护，收益=可长期维持的判据。
3. **语料从「计数 EQ 文件」起步**：先啃 errs==markers 的 34 个文件（21+9+4），暴露机制问题后再扩到高难文件（`builtins0` 365 条等）；排除清单**逐行理由**，理由分类见 §7.2。
4. **parser 不重写**：M2 的 `rgoc-parse` 逐字节对撞过官方 syntax（31/31）——检查器**只消费**它，不再对解析面动刀；含语法错误的 8 个语料文件（F10）走「排除清单（解析面另有 M2 渠道）」或后续 wave，见 §7.2。
5. **`types` 子命令与 `check` 子命令分工冻结**（`D-M4-11`）：避免「一个命令两种时代语义」；`types` 只扩 cmp 判定式与（如需要的）dump 面。
6. **`assert`/`trace` 开关**：`rgoc-check` 提供 `--test-hooks`（暂名）开关注入这两个内建，**默认关**（生产语义），对撞装置**开**（对齐官方 harness）——不是「支持 assert」，而是「装置对齐」（`F13`）。

---

## 3. 架构与数据模型

### 3.1 流水线与 crate 变化

```text
输入：同目录多文件 .go
  ▲
  │ M3 现有
rgoc-load::load_with_imports ──► Package { 作用域、对象表、常量值、导出摘要、诊断 }
  │
  │ M4 新增（rgoc-check）
  ▼
rgoc-check::check_package ──► CheckedPackage { 表达式类型表、选择 info、InitOrder、
                                              初始化环诊断、main/init 约束诊断、诊断流 }
  │
  ├─ driver：rgoc check [--diag-dump] [--lang go1.N] [--test-hooks] <files…>
  └─ 对撞：renders → `<文件>\t<行号>\t<类别>`（C8）
```

| crate | 变化 | 内容 |
|---|---|---|
| `rgoc-types` | **扩** | 新模块：`assignable.rs`（可赋值性）、`methodset.rs`（方法集）、`implements.rs`（接口实现+约束满足）、`comparable.rs`（可比较性）、`convertible.rs`（转换静态面）、`unify.rs`、`infer.rs`、`instantiate.rs`、`subst.rs`；`identical.rs` 升级（类型集同一性、实例化具名、类型参数、对 `interface{}` 的 terms/comparable 面） |
| `rgoc-check`(**新**) | 建 | `check.rs`（主驱动/顺序/延迟检查）、`operand.rs`（操作数模式）、`expr.rs`、`stmt.rs`、`builtins.rs`、`lookup.rs`+`selection.rs`（选择器/字段提升/方法表达式与方法值）、`initorder.rs`、`main.rs`（main/init 约束）、`version.rs`（特性门控）、`unsafe_pkg.rs`（白名单 + sizes）、`imports.rs`（点/空/别名 import 的可见性缝合）、`diag.rs`（诊断码 + 类别投影）、`render.rs`（`--diag-dump` 渲染，与 C8 对齐） |
| `rgoc-load` | 小扩 | `pkg.X` 的**导出性**判定与跨包对象比对接线；import 名规则补齐（`ImportName` 完整规则）；`-I` 根指错 vs 不支持形式的**诊断拆分**（`open_gaps` 项） |
| `rgoc-driver` | 扩 | `check` 子命令；`types --cmp` 新判定式；`manifest_m4.rs` 对账测试（沿 `manifest_m3.rs` 范式） |
| `tools/types-oracle` | 扩 | `--check` 模式 + `--cmp` 新 kind + 适配表 + `run-collide.sh --check` |
| `docs/` | 新建 | `contracts/C8-check-diags.md`；`milestones/M4-{tests,plan}.md`、`M4-manifest.json`、`M4-report.md` |

### 3.2 判定五件套（`rgoc-types` 扩展）

| 件 | 对应 | 基准（`go/types`） | 输出形态 |
|---|---|---|---|
| 可赋值性 | `TYP-18` | `operand.go::assignableTo`（F6） | `fn assignable(V, T) -> Result`（沿 `identical` 的 `Result` 习惯：`Ok(bool)` vs 显式拒绝） |
| 方法集 | `TYP-19` | `methodset.go`（F7） | 有序方法列表（名 + 签名 id），供 implements/选择器消费 |
| 接口实现/约束满足 | `TYP-13`、`TYP-23` | `instantiate.go::implements`（F6） | `fn implements(V, T, constraint: bool) -> Result`；cause 串（渲染人读诊断用） |
| 类型集同一性 | `U-M3-05` | `predicates.go` 接口分支（F5） | `identical` 升级：terms 归一比较 + comparable 标志 + 方法集；**删除 `TypUnsupportedConstraintIdentity` 拒点** |
| 可比较/转换 | `EXP-21/25` 静态面 | `predicates.go::comparable`、`conversions.go` | `fn comparable(T)`；`fn convertible(V, T)`（含常量特例面） |

> **「显式拒绝」的形态沿用 `D-M3-11`**：稳定 `DiagCode` + 登记 `unsupported`；内部不可达仍 `todo!`。

### 3.3 泛型三件

* **统一（`unify.rs`）**：照 `go/types/unify.go` 的**三种模式**（exact / loose / assign）与结构递归规则；类型参数、类型集（terms）、接口、通道方向、签名比较等目标面。
* **推断（`infer.rs`）**：入口按 `go/types/infer.go`：函数实参推断（1.18）、隐式实例化在**实参位置**（1.21）、赋值/接口方法语境（1.21）与**赋值上下文完整推断**（1.27）。推断失败 ⇒ 诊断（含「cannot infer T」类）。
* **实例化（`instantiate.rs` + `subst.rs`）**：`Instantiate(origin, args)`——实参数量/约束满足校验（消费 `implements`/`satisfies`）；结果类型**按 (origin, args) 记忆**（同一实例化不重复造 id，dump/identity 才有依托——M3 的 15 例 DIFF 根因直指这里）。
* 表示层的迁移：M3 的 `Type::Named` 需扩展携带**实例化实参**（具体形态在 `M4-plan.md` 的表示任务里落，若触发 `C7` 渲染变化则走 `D-M4-9` 的修订路径）。

### 3.4 检查器（`rgoc-check`）

* **操作数模式**（`operand.rs`）：`invalid / novalue / builtin / type / variable(常量/非常量) / value(非常量)`——与 `go/types/operand.go` 对齐；这是「值层类型接线」（M3 `open_gaps`：`-uint32(5)` 漏报的根因）的落点。
* **表达式**（`expr.rs`）：一元/二元/比较/移位/转换/调用（含变参、多值、内建）/索引（含 map 双值、泛型实参二义消解）/切片（含 3-index）/选择器与字段提升/方法值与方法表达式/类型断言（含 `v, ok`）/复合字面量（含 1.27 嵌套字段 key）/函数字面量/括号/接收。
* **语句**（`stmt.rs`）：赋值与复合赋值（操作数对数、可赋值性）、短变量声明（至少一新、同块重声明规则）、if/for/switch/type-switch/select 的静态规则、range 静态面（含 1.22 int、1.23 func 的**形态**）、go/defer 静态约束、return（含命名结果）、break/continue/goto/fallthrough（标签面延用 M3）、终止语句判定（`STM-01`）、块内声明。
* **内建**（`builtins.rs`）：18 个预声明 + `assert/trace`（开关注入，F13）+ `unsafe` 白名单三件；签名、参数校验、常量折叠（`len/cap/Sizeof/Alignof/Offsetof`、`min/max` 常量面）与版本门控挂点（F1）。
* **版本门控**（`version.rs`）：F1 全表；`// -lang=`（首行指令，harness 语义）与 `//go:build`（文件版本，F11）**同源进入**；`TooNew`（包版本高于工具链）沿用。
* **初始化**（`initorder.rs`）：包级 var/const 的依赖收集（表达式引用）+ 拓扑序（`InitOrder` 对撞）+ 环诊断（含次级链，之后按 F2④ 过滤渲染——**内部保留原始链**供人读）。
* **main/init 约束**（`main.rs`）：`func init` 无参无结果、有体（soft 规则照官方）；`func main` 在 package main 的签名/引用约束（精确面 RED 阶段实测钉死）；包名/导入自身限制（`PKG-02`）。
* **unsafe/sizes**（`unsafe_pkg.rs`）：F8/F9。

### 3.5 包级与可见性（`rgoc-load` 小扩 + `rgoc-check::imports`）

* `.`/`_`/别名 import 三形态的**名字进入规则**（点导入合并进文件作用域；空导入只触发装载；别名覆盖默认名）。
* `pkg.X`：先导出性（大写开头，`SCP-06` 已落判定函数）再**跨包对象比对**（类型/常量值一致）；`UndeclaredImportedName` 细分码。
* `PKG-05/06`：见 3.4；`PKG-04` 零值 = 类型完备性谓词（自有 fixture 面）；`PKG-08` 导出可见性 = 上述 + 报告矩阵条目。

### 3.6 诊断模型与类别（`C8` 的核心之一）

* **rgoc 侧**：新 `DiagCode` 一族（前缀 `TYP-`/`CHK-`，命名在 `M4-tests.md` 冻结）；每个码到**类别**的投影是**全量穷尽**（编译期 match + 计数断言）。
* **类别枚举 v1 草案**（`M4-tests.md` 冻结；`other` 只允许出现在排除文件）：

  `undef`｜`not-type`｜`op-invalid`｜`overflow`｜`shift`｜`convert`｜`assign`｜`index`｜`slice`｜`selector`｜`implements`｜`ret`｜`terminate`｜`decl`｜`unused`｜`import`｜`cycle`｜`version`｜`range`｜`builtin`｜`type-arg`｜`stmt`｜`comma-ok`｜`unsafe`｜`other`

* **oracle 侧适配表**：消息文本 → 类别（regexp 表）；守卫测试：对**入选清单**的每个文件跑 oracle，断言没有一条消息落到 `other`；改动适配表要跑全部语料。
* **渲染（C8 格式草案）**：

  ```text
  # source: <rgoc|oracle>
  # file: <相对路径>
  <行号>\t<类别>
  ```

  排序 = 按（行号, 类别）稳定排序；列号**渲染**但默认**不进硬判据**（§10 未决 #1；若实测列号稳定则升格）。

### 3.7 资源上限

* 沿用 M3：递归/导入深度、文件大小上限；新增：**每文件检查超时**（建议 10s）与**每批次总超时**（smoke ≤ 5 min / milestone ≤ 30 min，沿 `AGENTS.md` §4.3）。
* `--diag-dump` 的确定性：两次运行逐字节相同（沿 `run-collide.sh` 纪律）。

---

## 4. oracle 扩展与对撞装置

### 4.1 `types-oracle --check`

* 输入：一组同包文件（或目录）；输出：`C8` 渲染（§3.6）。
* 语义对齐官方 harness（F2）：
  1. 首行 `// -lang=` 解析（`parseFlags` 语义）→ `Config.GoVersion`；
  2. `parser.AllErrors|parser.SkipObjectResolution`（与官方一致），**parse 错误参与**；
  3. `types.DefPredeclaredTestFuncs()`（F13）——**注入开关 = 装置级**；
  4. importer：**只认 `unsafe`**（返回 `types.Unsafe`；其余响亮失败）——标准库依赖文件由清单排除；
  5. 过滤 `": \t"` 次级错误（F2④）后渲染；
  6. stderr 必须空、重复运行确定（沿 M3 纪律）。
* 退出码：0=无错；1=有（渲染照常）；2=用法/致命（沿现有约定）。

### 4.2 `types --cmp` 新判定式（`D-M4-15`）

* 语法沿用 `//rgoc:cmp <kind> <A> => <B>`（`R-32-1`）；新增 kind：
  * `assignable <V> => <T>`（V/T 类型表达式）→ `true/false/reject`；
  * `implements <V> => <I>`（接口方法集面）→ `true/false/reject`；
  * `satisfies <V> => <C>`（约束面，`constraint=true`）→ `true/false/reject`；
  * `methodset <T>` → 单行：排序后的方法**名字**序列（`-` 为空）；签名的渲染面若需要，走 `C8` 修订（§10 未决 #2）；
  * `instance <F> => <arg1, arg2, …>` → 成功：该实例化类型的 **C7 单根 dump**（缩进块）；失败：`reject:<category>`。
* `infer`/`unify` 不设 cmp；`identical`/`representable` 既有语义**不变**；未知 kind 仍拒（`T-TY-82` 改写点，`D-M4-11`）。

### 4.3 `run-collide.sh` 新形态

* `--check <testdata 根>`：读 `check-selected.txt`（唯一清单；不重算机械筛——沿 `F18` 纪律），逐文件跑两侧 `--check` 渲染并逐字节比对；附带 `check-allowlist.txt`（排除，含**逐行理由**；由一条测试断言「候选 = 入选 ∪ 排除」）。
* `--cmp` 形态自动涵盖新 kind（清单不变，fixture 增）。

### 4.4 fixedbugs 分批

* wave A 起步：与**被解除项**直接相关的 issue 文件（实例化/同一性/可赋值/接口实现/init 环/版本门控，按文件名与 `issueNNNNN` 内容人工挑一批，清单进 `M4-tests.md`）；
* 后续 wave 按依赖逐批加入；**首轮门禁不含 fixedbugs 全集**（G5）。

---

## 5. 覆盖清单（roadmap §M4 ↔ 功能 ID 面）

### 5.1 进 M4 的 ID（含静态面）

| 组 | 进 M4 的项 | 例外/声明 |
|---|---|---|
| `TYP` | `TYP-13`、`TYP-18`、`TYP-19`、`TYP-23..26`（M3 挂起项 + 本阶段的判定五件套/泛型） | `TYP-20/21/22` 已在 M3 表示层（M4 消费）；`TYP-26` 顺带补齐 9 缺测试项之一 |
| `EXP` | `EXP-01..15`、`EXP-17`、`EXP-20`、`EXP-21`、`EXP-23..26` 的**静态面**；`EXP-16` 自有 fixture（9 缺测试项之一） | `EXP-18/19`（回绕/浮点）只做**常量折叠**面与类型规则；`EXP-22` 短路=运行时，静态面=可类型化+常量折叠；`EXP-24` 静态面=chan 方向/类型；`EXP-27` **全出** |
| `STM` | `STM-01..21` 的**静态面**（含 range 形态 1.22/1.23） | 执行语义项全出（阻塞/随机/goroutine/LIFO……）：`STM-05/13/14/15/20` 等只做静态规则 |
| `BIF` | `BIF-01..15` 全体：签名 + 静态约束 + 常量折叠 | 运行时行为（扩容、分配、展开）进 Runtime 面（M5+） |
| `PKG` | `PKG-01..08` 静态面：init 图/序/环、main/init 约束、导出可见性、`.`/`_`/别名 import | `PKG-05` 的执行、`PKG-07` 的 runtime 入口（argv/envp）出 |
| `SYS` | `SYS-03`、`SYS-04`（+ `unsafe.Pointer` 类型名表示） | `SYS-01/02/05` 出（`D-M4-8`） |
| `PIPE` | `PIPE-04` 特性面（版本门控全表）、`PIPE-24` 的 **init 拓扑静态面**（startup 运行时出）、`PIPE-06` 的**源码 import 面**（导出数据面出，`U-M3-07`）、`PIPE-11` 的**静态面**（infer/unify；字典化出，`RT-POLY-05`）、`PIPE-25` 诊断系统静态面（类别/位置；多行提示出） | `PIPE-07` 的完整实现即 M4 主体；`PIPE-08` 常量引擎已在 M3 落地（M4 补 `len/cap/Sizeof` 等折叠） |
| 9 缺测试项 | `TYP-26`、`EXP-16`、`EXP-22`（静态）、`PKG-04`、`PKG-06` 在 M4 补自有 fixture（`SCP-06` 已在 M3） | `RT-SCH-02`/`RT-POLY-03`/`RT-POLY-05` 仍留（Runtime 阶段）——`RT-POLY-05` 的**静态面**（泛型分派形状）随本阶段 M4 复核 |

### 5.2 roadmap §M4 的 9 大项 ↔ 本设计章节

（已在 §1.2 表逐句落点，此处不再重复；`M4-tests.md` §3 会把每个 ID 变成 `T-CK-*` 测试项。）

---

## 6. 错误处理与资源上限

### 6.1 诊断

* 稳定 `DiagCode` + 位置；消息**人读向**（不承诺与官方逐字，`U-M3-10`）。
* 「接受面」与「拒绝面」都要有**正反**断言（沿 M2/M3 的双向纪律）：拒绝用「码 + 位置」，接受用「无诊断 + 退出码」。

### 6.2 资源上限（承接 + 新增）

| 项 | 值 | 说明 |
|---|---|---|
| 递归深度 | 1000（沿 M2/M3） | 类型图/表达式遍历同口径 |
| 导入深度 | 100（沿 M3） | |
| 每文件检查超时 | 10 s（建议值，`M4-tests.md` 冻结） | 语料单文件 |
| 批次总预算 | smoke ≤ 5 min / milestone ≤ 30 min / full ≤ 2 h（沿 `AGENTS.md` §4.3） | `--check` 语料批次与门禁脚本 |
| 文件大小 | 64 MiB（沿 M1 `D-M1-4`） | |

### 6.3 预算与降级序

* roadmap §6 给 M4 的量级：**6–12 人周 / 12 周日历**。本仓执行形态（AI 代理 + 容器）按任务数折算，不进 manifest 的「人周」字段——`M4-manifest.json` 记**测试数与门禁**，不记人周。
* **降级序**（roadmap §M4「砍项」的执行化）：① 砍 fixedbugs 后续 wave；② 砍入选清单里高难文件（→排除清单+理由）；③ 保证不砍：入选集接受/拒绝 + 位类对齐、版本门控、init 顺序/环、四组自有 fixture（G4）、判定五件套、`U-M3-05` 解除。

---

## 7. 测试策略

### 7.1 四条腿 + 反向验证（沿 M3 §8 结构）

| 腿 | 内容 | 主判据 |
|---|---|---|
| 甲 | crate 内单测：自有 fixture（`S`，spec 手写期望）——四组点名 fixture（`.`/`_`/别名 import、init cycle、泛型推断、+ 方法集/字段提升）+ 判定五件套 + 版本门控 | `S` |
| 乙 | oracle 对撞：`--cmp` 新 kind、`--dump`（语料 15 例 DIFF 复核）、`--check`（位类对齐） | `O` |
| 丙 | 语料精选：check/spec/examples 入选清单全绿（双腿）；fixedbugs wave | `O`+`S` |
| 丁 | 端到端：`rgoc check` 退出码/诊断流、`--diag-dump` 确定性与两清单守卫、门禁脚本 | 驱动断言 |
| 反 | 变异探针 ≥ 8 条（`D-M4-13`），含：类别适配表未命中守卫、额外错误检出（往 rgoc 注入一条多余诊断⇒必红）、比对器（打乱一侧一行⇒必红） | 反证 |

### 7.2 双腿判据与选择纪律

* **腿①（marker 判定）**：直译官方语义（F2）——`ERROR` 子串 / `ERRORx` 正则 / 双向消耗 / `": \t"` 过滤 / `-lang` 首行。**注意**：本腿不需要消息复刻——**leg① 只判 verdict**（有标记⇒必须拒、无标记⇒必须接），消息匹配在官方语义里淘汰（`D-M4-3`）。
* **腿②（位类对齐）**：oracle 实跑 vs rgoc 实跑（§3.6 渲染）；**行号多重集 + 类别多重集相等**；stderr 空；两次运行确定。
* **排除清单理由分类**（逐行写）：`多文件目录包（wave 2）`｜`含语法错误（走解析面渠道/wave 2）`｜`依赖标准库 import`｜`依赖 assert/trace 之外的内建或实验面`｜`依赖未支持特性（unsafe 转换等）`｜`官方抑制行为差异（逐例说明）`。
* **抑制/级联差异**（本设计**已知的最大风险**）：官方对某些错误会抑制后续报错，逐文件对齐时若在某个形态上无法一致 ⇒ **该文件进排除**（不许放宽多重集口径）；随 wave 复核，能对齐的再收回来。

### 7.3 反向验证与遗漏纪律

* 沿 `scripts/mutation-m3-notes.md` §0/§0.1：变异前 `cargo clean -p`；可疑时全量重建；探针照不出就照实登记。
* **守卫的反向验证**：适配表守卫、额外错误检出、两清单相等断言——每条都要有「造一个假绿场景它会红」的实证（写入 `mutation-m4-notes.md`）。

### 7.4 白名单纪律与翻转

沿 `M3-tests.md` §6：白名单**不得**记为 pass；每项有正向断言；M4 翻转按 `D-M4-11` 走修订，`manifest_m3.rs` 与新的 `manifest_m4.rs` 双侧对账。

---

## 8. 契约与产物影响

| 产物 | 动作 |
|---|---|
| `docs/contracts/C8-check-diags.md` | **新建**（`M4-plan.md` 首个任务）：渲染格式（§3.6）+ 类别枚举 + 两侧映射纪律 + 过滤规则（F2 摘录） |
| `docs/contracts/C7-type-dump.md` | **预期不动**；若实例化/类型集渲染需要扩展 ⇒ 修订表 + 全 `.want` 重刷 + oracle `-update`（风险登记 §10） |
| `M3-tests.md` | 修订表追加（§6 各行）：`U-M3-01..05/08` 的解除指针；`T-TY-81/82` 的改写说明 |
| `M3-manifest.json` | `unsupported`/`open_gaps` 对应项更新（**追加式**：保留原值 + `resolved_by: M4` 指针） |
| `M0/M1/M2` 门禁脚本 | 不回退（CH-7） |
| `M4-report.md` | 交付 A：前端子集矩阵（功能 ID × 静态/执行）+ 三条分别声明 + 排除清单统计 + 未决项 |
| `GAP-01.md` | M4 完成后补 PIPE/EXP/STM/BIF/PKG/SYS 的归属落点（沿 M2/M3 做法） |
| `AGENTS.md` / `03-roadmap.md` | 仅当 M4 **完成**时更新状态节（不与実行同日） |

---

## 9. 门禁汇总（暂名，`M4-tests.md` 定稿）

见 §1.4 的 **CH-1..CH-9**。命名风格拟与 M3（`TY-1..8`）并列取 M4 主题词缩写（`CH` = check），定稿在 `M4-tests.md` §8。

---

## 10. 风险与未决

| # | 事项 | 类型 | 处理 |
|---|---|---|---|
| 1 | 列号是否进硬判据 | 未决 | 默认只判行；渲染含列；RED 期实测两侧列稳定性后再定（升格或登记） |
| 2 | `methodset` 渲染是否含签名 | 未决 | v1 只给名字序列；签名面若需要，走 `C8` 修订 |
| 3 | 抑制/级联差异面的大小 | 风险 | 计数值 EQ 的 34 个文件先验；差异文件进排除 + 逐例说明（§7.2） |
| 4 | fixedbugs wave 划分数量 | 未决 | wave A 起步；按「被解除项」相关性挑；数量与清单在 `M4-tests.md` 冻结 |
| 5 | 8 个含语法错误文件（F10）的处置 | 未决 | 首轮排除（理由入清单）；是否纳入后续 wave 取决于收益 |
| 6 | `C7` 是否需修订 | 风险 | 以实测为准（`D-M4-9`）；最坏情况=修订 + 重刷 `.want` |
| 7 | 推断实现的对撞深度（1.27 赋值上下文） | 风险 | 语料 `inference*.go` 为主轴；超界显式拒绝（`D-M4-5`） |
| 8 | `check/` 目录包（`decls2/` 等）的纳入时机 | 未决 | 首轮排除（多文件包形态确认后 wave 2） |
| 9 | 工具链版本漂移（`go1.27.1` 固定） | 环境 | 容器钉死；若官方改动行为（如 F1 表），跟改 + 登记 |
| 10 | `merge` 时与 M3 停点的工作树状态 | 工程 | 沿 `AGENTS.md` §6/§7 的流程（就地 main、门禁后提交） |

---

## 11. 与后续文档的接口

### 11.1 交给 `writing-plans`（`M4-plan.md`）的输入

1. 本设计的 §3 架构与 §7 测试策略 → 任务分解（沿用 M3 的 5 个 Phase 形态：契约 → 判定面 → 检查器 → 对撞接线 → 收口）；
2. `D-M4-11` 的 M3 同步任务（专项任务，不得遗漏）；
3. `D-M4-14` 的夹具-ID 机制化任务；
4. `M4-report.md`（交付 A）的产出任务近收口处。

### 11.2 `M4-tests.md` 必须覆盖

* `T-CK-*` 测试 ID 全表（甲/乙/丙/丁 四腿 + 反向验证）；
* 类别枚举冻结 + 两侧映射纪律 + `other` 守卫；
* 入选/排除两清单（**开工前冻结**；逐行理由）；
* fixedbugs wave A 清单；
* 四条 cargo 命令 + `run-collide.sh` 四形态 + 门禁脚本的**验证命令与预算**；
* `unsupported` 白名单（含从 M3 承接仍有效的项 + M4 新增项，如 `SYS-01/02`、实验开关面等）。

### 11.3 与后续阶段的衔接

* M5（解释器）：消费 `rgoc-check` 的类型表与 `InitOrder`；本设计的「不执行」边界即 M5 的开工点；
* M6/M7（native）：`unsafe`/`SYS-01/02` 的支持条件在此另列；
* M8+：range-over-func 执行、并发、反射。

---

## 12. 变更记录

| 日期 | 变更 |
|---|---|
| 2026-10-18 | 初稿：按用户裁定（`D-M4-1` roadmap 全量）起草；含 F1–F18 实测事实基线（探针在 `.superpowers/sdd/M4-design/probe/`，不入库）；D-M4-1..15 决策；CH-1..9 门禁草案 |
| 2026-10-18 | **用户确认**（「确认，继续」）：D-M4-1..15 生效，冻结进入 `M4-tests.md` 阶段；builder-check 复审意见如有 Blocker/Major 继续按本表入档 |
