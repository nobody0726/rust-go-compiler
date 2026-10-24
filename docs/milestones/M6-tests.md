# M6 测试清单：SSA、verifier 与精简优化

> **状态**：**判据面已冻结**（2026-10-23）　|　**设计**：[`M6-design.md`](./M6-design.md)（`D-M6-1..16`）　|　**计划**：[`M6-plan.md`](./M6-plan.md)　|　**事实**：[`M6-manifest.json`](./M6-manifest.json)
>
> **本文档定「怎么算通过」**，不定「怎么做」。凡是本文档与 `M6-design.md` 冲突的，以**本文档**为准（判据优先），并按 §9 的修订流程改设计。

| 项 | 值 |
|---|---|
| 阶段 | M6（SSA / verifier / 精简优化 / live set 协议） |
| 判据来源 | `S`（自有 fixture / 手写 IR）、`O`（官方语料三方差分）、`H`（装置与 CLI） |
| 输出判据 | 合并流**逐字节** + 退出码；唯一容差 `\r\n → \n`（沿 `M5-tests.md` §4.1） |
| 语义基准 | **HIR 解释器**（M5 已验收）+ 官方 `go run`，两者都必须与 SSA 求值一致 |
| 冻结日 | 2026-10-23（判据面）；**语料切片待 Task 3 实测后追加冻结**（`R-6-1` 同款做法） |
| 门禁 | §7，`SSA-1..9` |
| 白名单 | §5，**9 条**（7 + `R-31-1` 补的 2 条），开工前冻结；跳过项一律**不得**记为 pass |
| 变异 | §3.5，**8 条**，每条必须实证变红并复原 |

---

## 1. 判据来源（三种，含名与实）

| 代号 | 含义 | 载体 |
|---|---|---|
| `S` | 自有 fixture：Go 源（走完整流水线）或**手写 IR**（直接喂 verifier） | `rgoc-ssa/tests/*.rs`、`rgoc-driver/tests/ssa_*.rs` |
| `O` | 官方语料：`rgoc run --via=hir` = `rgoc run --via=ssa` = `go run` | `tools/run-oracle/run-collide.sh --ssa` |
| `H` | 装置与 CLI：dump 通道、`--verify`、资源上限、manifest 对账 | `rgoc-driver/tests/*.rs`、脚本 |

> **`S` 的两种形态必须分清**：**手写 IR** 用来测 verifier（反例/正例），**Go 源**用来测构建器与求值器。
> 用 Go 源测 verifier 是无效的 —— 构建器产不出非法图，等于永远绿（`D-M6-11`）。

---

## 2. 覆盖映射

### 2.1 `03-roadmap.md` §M6 十句 ↔ 测试 ID

| # | roadmap 句子 | 覆盖它的 ID |
|---|---|---|
| 1 | Block/Value/Phi | `T-SSA-01/07/08/15/16/17` |
| 2 | 多返回值 tuple | `T-SSA-20/21/22/50` |
| 3 | memory / effect | `T-SSA-06/10/18/23/25/26/27/28/49` |
| 4 | 调用与异常传播表示 | `T-SSA-23/30/31`、`T-SSA-38`（`defer`/`recover` 拒绝） |
| 5 | 支配分析先于变换 | `T-SSA-03`（V2）、`T-SSA-51` |
| 6 | 三 pass + 证明必要的 CSE | `T-SSA-60..64`、`T-SSA-51` |
| 7 | 求值器共用值与 runtime 操作 | `T-SSA-32..37`、`T-SSA-52` |
| 8 | live set / safepoint / 派生指针 | `T-SSA-53..56` |
| 9 | 不支持项显式报告 | `T-SSA-33`（每条码一个 fixture）、`T-SSA-38`（CLI 通道）、§5 白名单 |
| 10 | 不要求优化 ≥8 | §7 `SSA-5`（只要求语义保持） |

### 2.2 六子阶段 ↔ 门禁 ↔ 测试 ID

| 子阶段 | 门禁 | 测试 ID |
|---|---|---|
| M6a IR 与 verifier | `SSA-1` / `SSA-2` | `T-SSA-01..14` |
| M6b HIR→SSA 构建 | `SSA-3` | `T-SSA-15..30` |
| M6c 诊断求值与差分 | `SSA-4` | `T-SSA-31..37` |
| M6d 必要优化 | `SSA-5` | `T-SSA-60..64` |
| M6e live set 与协议 | `SSA-8` | `T-SSA-51..54` |
| 横切（不支持面 / 装置 / 回归 / 变异） | `SSA-6/7/9` | `T-SSA-33/38..46`、`T-SSA-47..54` |

---

## 3. 测试 ID 全表

### 3.1 甲腿：IR 与 verifier（`S`，14 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-SSA-01` | 指令/终止符**穷尽** | 每个 `Instr` / `Term` 变体至少被一条 fixture 产出；穷尽性由 `match` 的 `_ => unreachable` 之外的枚举表断言（新增变体必须让测试红） |
| `T-SSA-02` | `V1` 单一定义 | 手写 IR：同一 `ValueId` 两条 `dst` ⇒ 拒，`invariant == V1` |
| `T-SSA-03` | `V2` 支配 | 手写 IR：一支定义、汇合点直接使用 ⇒ 拒，`V2` |
| `T-SSA-04` | `V3` 类型一致 | 手写 IR：`i64 + string`、`Convert` 目标与 `value_types` 不符 ⇒ 拒，`V3` |
| `T-SSA-05` | `V4` CFG 互逆 | 手写 IR：`A→B` 有后继但 `B` 无前驱；entry 有前驱 ⇒ 拒，`V4` |
| `T-SSA-06` | `V5` 调用结果数 | 手写 IR：签名 2 结果、`dsts` 1 个 ⇒ 拒，`V5` |
| `T-SSA-07` | `V6` mem 支配与同块序 | 手写 IR 两条：① `Store` 的 mem 来自不支配它的路径 ⇒ 拒；② 同块同根两次访存跳过前一个 `Store` 的 `dst` ⇒ 拒 |
| `T-SSA-08` | `V7` 块参数计数 | 手写 IR：目标 2 个 `params`、某前驱给 1 个 ⇒ 拒，`V7` |
| `T-SSA-09` | `V8` 可达性 | 手写 IR：非 entry 块无前驱且未标 `dead` ⇒ 拒，`V8` |
| `T-SSA-10` | `V9` 取址即建根 | 手写 IR：`AddrOf(root)` 但 `slot_root` 缺该 slot ⇒ 拒，`V9` |
| `T-SSA-11` | **合法图全过** | 每种指令族各一张手写合法图（约 10 张）⇒ 全部 `Ok`；**反向证明 verifier 不是「一律拒」** |
| `T-SSA-12` | 拒绝可定位 | 每个反例的 `VerifyError` 必须带 `invariant` + `block` + `idx`，且三者与注入位置一致（不是「拒了但说不清哪条」） |
| `T-SSA-13` | dump 确定性 | 同一 IR 三次 dump **逐字节相同**；不含地址/哈希序 |
| `T-SSA-14` | `C10` 格式符合 | 头行字段齐（stage/version/func/blocks/values）、块与值按 ID 升序、行语法符合 `C10-ssa-dump.md` |

### 3.2 乙腿：HIR→SSA 构建（`S`，16 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-SSA-15` | 直线函数 | 无控制流函数：块数 1、`Term::Return`；`--dump` 与期望结构一致 |
| `T-SSA-16` | `if`/`else` 汇合 | 汇合块有 `params`，两条前驱各给同数量实参；`V7` 通过 |
| `T-SSA-17` | `for` 三形态 | `for{}` / `for cond{}` / `for init;cond;post{}` 各一；**回边**存在且块参数在回边上被重新喂值 |
| `T-SSA-18` | `switch` | 有 tag / 无 tag / `fallthrough` 各一；`fallthrough` 不产生新块参数丢失 |
| `T-SSA-19` | `goto` 与 labeled | 前向 `goto` / 反向 `goto` / labeled `break`/`continue` 各一 |
| `T-SSA-20` | 多返回值接收 | `a, b := f()` ⇒ `Call{dsts:[v1,v2]}` + 两个使用点 |
| `T-SSA-21` | `return f()` 直连 | 无中间值：`Return` 的操作数**就是** `f` 的 `dsts`（结构断言，不是看输出） |
| `T-SSA-22` | `g(f())` 实参直连 | `g` 的 `args` 里包含 `f` 的每个 `dsts`；顺序按签名 |
| `T-SSA-23` | 调用与 effect | 写全局的调用：`effect.writes` 含该 `Root::Global`；只读函数：`writes` 空且 `may_panic=false` |
| `T-SSA-24` | 复合字面量 | 数组/结构体/slice 字面量各一；`Composite` 的 `items` 顺序 == 源码顺序（keyed 元素按 lowering 定的下标） |
| `T-SSA-25` | 指针与写穿 | `p := &a; *p = 2` ⇒ 存在 `Root::Slot(a)` + `Store`；后续读 `a` 走 `Load` 且用同一个 mem 链 |
| `T-SSA-26` | slot 提升正例 | **没取过址**的局部队列：`--dump` 里**没有** `Load/Store`、也没有对应根（纯值化） |
| `T-SSA-27` | slot 提升反例 | 取过址的 slot **必须**在 `slot_root` 里；且该 slot 的读**必须**走 `Load`（不许被折叠成值） |
| `T-SSA-28` | 并行拷贝 | `a, b = b, a` ⇒ 交换正确；且 IR 里两条赋值**不得**共用同一个中间值（结构断言） |
| `T-SSA-29` | 变参调用 | `f(xs...)` / `f(1,2,3)` 到 `...int` 两种；`args` 个数与 `variadic` 标记正确 |
| `T-SSA-30` | 静态方法调用 | `CalleeRef::Method { recv, func }`；接收者按值/指针语义各一 |

### 3.3 丙腿：三方差分（`O`，7 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-SSA-31` | 切片分类两向 | 从 69 行出发：能建图 `N` + 拒绝 `M`，`N+M==69`（两向，无死行无重复）；`M` 每行的码都在 §5 |
| `T-SSA-32` | 三方差分 | `--via=hir` = `--via=ssa` = `go run`（合并流逐字节 + 退出码），在 `N` 上全绿 |
| `T-SSA-33` | 拒绝面每行有 fixture | `M` 每行都有一个能触发该码的最小 Go fixture（`S` 腿）+ 语料行本身（`O` 腿） |
| `T-SSA-34` | 纯函数/输出 | `helloworld.go` / `print.go` / `printbig.go` 三类输出形态在 SSA 下字节一致 |
| `T-SSA-35` | 循环与数组 | `ken/` 的循环/数组/slice 子集（按 `N` 的实际成员） |
| `T-SSA-36` | 指针写穿语料 | 含 `&x` + `*p =` 的文件在 SSA 下与 HIR 一致（memory 模型的真实检验） |
| `T-SSA-37` | 复合字面量语料 | 含结构体/数组/slice 字面量的文件一致 |

### 3.4 丁腿：装置、CLI 与资源（`H`，8 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-SSA-38` | `ssa --dump` / `--verify` 通道 | dump 只进 stdout；verifier 错误只进 stderr；两者同时给时**先 dump 后 verify** 且都输出 |
| `T-SSA-39` | `run` 默认行为不变 | 不给 `--via` 时仍走 HIR（M5 的 69/69 不受影响）；给非法 `--via` ⇒ 退出码 2 + 稳定消息 |
| `T-SSA-40` | 步数上限 | 死循环 fixture：`--max-steps` 触发 ⇒ 退出码 2 + stderr 原因；宿主不挂死（墙钟 < 10 s） |
| `T-SSA-41` | 构建预算 | 超 1e5 指令的函数 ⇒ 构建期拒绝（退出码 1 + 稳定码），不 OOM |
| `T-SSA-42` | 四条 cargo + 五条自检 | `fmt` / `check` / `clippy` / `test` 全 exit 0；m0（232）/ m1 / ast-golden / ast-testdata / ast-stdlib 全过 |
| `T-SSA-43` | M5 切片零回归 | `cargo test --workspace` 全绿且总数 **≥ 1608**（不许为了绿而删测试）；M5 的 69/69 对撞重跑仍绿 |
| `T-SSA-44` | manifest 对账 | `manifest_m6.rs`：`gate` / `test_ids` / `unsupported` / `open_gaps` 逐项对账；三类谎（标 pass 却自认未兑现 / 非 pass 无缺口 / 未全绿自称 done）必须被拦 |
| `T-SSA-45` | 语料只读 | 跑完整批后 `go_source_code/` 无文件被写（`corpus-manifest.sha256`） |
| `T-SSA-46` | dump 契约自洽 | `C10-ssa-dump.md` 的每个字段都在 dump 里出现；文档里的语法行与真实 dump 逐行对得上（人工对账 + 一条 golden） |

### 3.5 反腿：变异探针（8 条）

| ID | 变异 | 期望红在 |
|---|---|---|
| `T-SSA-47` | 删掉 `V7` 的块参数计数检查 | `T-SSA-08` |
| `T-SSA-48` | 支配检查短路（`V2` 恒过） | `T-SSA-03` |
| `T-SSA-49` | `Store` 不出新 mem（同根不串） | `T-SSA-07`、`T-SSA-36` |
| `T-SSA-50` | 调用只取第一个结果 | `T-SSA-20/21/22` |
| `T-SSA-51` | CSE 去掉「块内」限制（跨块也消） | `T-SSA-45` 的 pass A/B 或 `T-SSA-03` |
| `T-SSA-52` | 求值器块参数**顺序**赋值（破坏并行拷贝） | `T-SSA-28` |
| `T-SSA-53` | 分类器把拒绝的文件算进「能建图」 | `T-SSA-31` |
| `T-SSA-54` | dump 用 `HashMap` 迭代序 | `T-SSA-13` |

> 每条必须**实证变红并复原**，记录进 `scripts/mutation-m6-notes.md`（沿 M3/M4/M5 格式）。

### 3.6 戊腿：M6e 协议（`S`+`H`，5 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-SSA-55` | 值级活跃性 | 直线/分支/循环（含回边）各一：`live_in/live_out` 与手算一致；循环体内变量在回边上活跃 |
| `T-SSA-56` | `SSA-LIVE v1` 字段自洽 | 头行版本/函数/块数/值数；每行 `safepoint` 的 `live` 里每个值都有区间；`ptrs` 里的值类型都是指针 |
| `T-SSA-57` | 消费方桩 | 一个只读协议的桩解析全部函数并断言可构造「每个 safepoint 的存活指针集合」，不许访问 IR 内部结构 |
| `T-SSA-58` | 派生指针保守策略 | 派生指针存在时，其根被标活跃；协议里 `ptrs` 含根（保守但正确） |
| `T-SSA-59` | 协议确定性 | 同一函数两次输出逐字节相同（排序键 = 块号 + 指令下标） |

### 3.7 己腿：优化 passes（`S`，5 条）

| ID | 名称 | 判据要点 |
|---|---|---|
| `T-SSA-60` | `fold` 语义保持 | 常量折叠/分支折叠各一：折叠前后 `--via=ssa` 输出一致；折叠溢出走 `C9` 的可表示性（不静默截断）；pass 后 verifier 绿 |
| `T-SSA-61` | `dce` 语义保持 | 删除无使用且无副作用的值；**`Store`/有 effect 的 `Call` 不删**（一条「删了会错的」反例） |
| `T-SSA-62` | `copy` 语义保持 | `Copy` 链合并；`a, b = b, a` 在多轮 copy 后仍正确；块参数实参的 `Copy` 消解不破坏并行语义 |
| `T-SSA-63` | `cse` 块内语义保持 | 同块内重复纯表达式合一；**跨块的相同表达式不得合**（反例：两支各算一次，CSE 后仍各自一份） |
| `T-SSA-64` | A/B 与 verifier | 同一 fixture 跑「`--opt=all` / `--opt=none` / 逐个关」三路输出一致；每个 pass 单独跑完后 verifier 必须 `Ok` |

**合计 64 个 `T-SSA-*`**（甲 14 / 乙 16 / 丙 7 / 丁 9 / 反 8 / 戊 5 / 己 5）。

---

## 4. 判据细节

### 4.1 三方差分（唯一语义判据）

```text
rgoc run --via=hir  ==  rgoc run --via=ssa  ==  go run
合并流逐字节；退出码相同；唯一容差 \r\n → \n
```

**为什么是三方而不是两方**：HIR 与 SSA 都是我们写的，两边一起错（例如都把 `defer` 参数求值时机搞错）时两方比对会**同时绿**。官方 `go run` 是唯一独立基准。

### 4.2 切片（`T-SSA-31`）

```text
候选（M5 入选）= 69
N = 能建图且 verifier 通过的个数
M = 拒绝的个数         （每行带 §5 的码）
断言：N + M == 69，且 M 的码集 ⊆ 白名单
```

**反向守卫**：把 `M` 里任一行挪进 `N` 的清单 ⇒ `T-SSA-31` 必红（证明分类器不是「只数数」）。

### 4.3 手写 IR 反例的写法（`D-M6-11`）

- 每个反例 = 一段**直接构造** `SsaFunc` 的代码 + 期望 `VerifyError { invariant, block, idx }`。
- 反例**不得**经由 builder 生成；builder 的 bug 若让非法图无法出现，verifier 的测试就成了同义反复。
- 反例与正例**同文件**（`tests/verify_negative.rs` / `tests/verify_positive.rs`），便于对照。

### 4.4 dump 判据（`T-SSA-13/14/46`）

`C10` 的三条硬要求：① `#` 头行（stage/version/func/blocks/values）；② 块按 `BlockId`、值按 `ValueId`、指令按 **块内顺序**；③ 不含地址、不含哈希序、不含时间。同一 IR 三次相同即通过。

### 4.5 资源上限

| 项 | 值 | 超限形态 |
|---|---|---|
| 步数 | 1e8（与 HIR 解释器同口径） | 退出码 2 + stderr 原因 |
| 构建指令数 | 1e5 / 函数 | 退出码 1 + 稳定码 |
| 墙钟 | 10 s / 程序（装置约束） | 装置判超时，不算 pass |

### 4.6 白名单纪律

白名单项**不得**记为 pass；每条都有「Go fixture + 码 + 退出码 1」三件套（`T-SSA-33`）。
「没做」与「拒绝」必须能区分：拒绝是**有码的拒绝**，不是静默跳过。

---

## 5. 白名单（`unsupported`，开工前冻结；`R-31-1` 后 9 条）

| ID | 项 | 为何不做 | 正向证据 | 计划阶段 |
|---|---|---|---|---|
| `U-M6-01` | 闭包（捕获跨帧写） | `Func.captured` 非空 ⇒ 帧槽与外层 `Cell` 别名，SSA 的「值唯一定义」不成立 | `T-SSA-33` 的闭包 fixture（`SSA-UNSUPPORTED-CLOSURE`）；语料 `closure*.go` 在 `M` 里 | M8（goroutine/闭包帧） |
| `U-M6-02` | `defer` / `recover` | 延迟调用的链是**运行期**结构，不是 SSA 值 | `T-SSA-33`（`SSA-UNSUPPORTED-DEFER`）；`T-EX-34..37` 的 fixture | M8 |
| `U-M6-03` | 接口动态派发 / type switch / 类型断言 | 目标函数在运行期才定，SSA 的 `CalleeRef` 只能间接调用（不做） | `T-SSA-33`（`SSA-UNSUPPORTED-IFACE-DYN`） | M8 |
| `U-M6-04` | `map` 读写 | 键相等是宿主语义（`C9` 的 `go_eq`），memory 根无法覆盖 | `T-SSA-33`（`SSA-UNSUPPORTED-MAP`） | M9（对象模型） |
| `U-M6-05` | `chan` / `go` / `select` | 并发语义整体不做（M8） | **守卫测试**：`HIR-UNSUPPORTED-CHAN` 先拒，`SSA-UNSUPPORTED-CHAN` 不出现 | M8 |
| `U-M6-06` | `range` over func | HIR 层已拒（`U-M5-06`），SSA 见不到 | **守卫测试**：拒绝来自 HIR，`-RANGE-FUNC` 不出现 | M8d |
| `U-M6-07` | 泛型实例化后的形态 | 检查器没放进来（`G-M5-03`），M6 无输入 | **守卫测试**：检查器先拒，`-GENERIC` 不出现 | 检查器面 |
| `U-M6-08` | 函数值（函数字面量当值 / 具名函数当值 / 方法值） | IR 里没有「把函数变成值」的指令 | `T-SSA-33` 的 `funcvalue` fixture（`SSA-UNSUPPORTED-FUNC-VALUE`） | M8（闭包帧） |
| `U-M6-09` | `range` over string | 要按 rune 解码，指令集里没有对应操作 | `T-SSA-33` 的 `range-str` fixture（`SSA-UNSUPPORTED-RANGE-STR`） | M7（字符串 rune 面） |

> **`U-M6-05/06/07` 在本前端「够不着」**（`R-31-2`）：上游先拒，SSA 层拿不到输入。
> 它们的码保留（契约稳定），正向证据改为**守卫测试**：断言该码**不出现**、拒绝来自上游。
> 上游一旦放行，守卫即红 —— 那时才回来实现 SSA 侧的码。

> **`U-M6-01..03` 是 M6 支持面最大的三块**：闭包/defer/接口动态派发恰好是「顺序语义」里最常用的三样。
> 因此 `N` 可能较小 —— 届时**如实报告**并把「哪些可提前做」列入 `open_gaps`（`D-M6-12` 的失效条件）。

---

## 6. 语料清单

| 清单 | 内容 |
|---|---|
| 候选 | M5 的 `run-selected.txt`（**69** 行，已冻结） |
| 能建图 `N` | **46**（冻结，`R-6-1`） |
| 拒绝 `M` | **23**（冻结）；每行三栏落 `tools/run-oracle/ssa-rejected.txt` |

**`M` 侧的码分布**（23 行，全部落在 §5 白名单里）：

| 码 | 行数 | 典型形态 |
|---|---|---|
| `SSA-UNSUPPORTED-IFACE-DYN` | 9 | 接口方法调用 / type switch / 类型断言（`method*.go` / `ken/inter*.go` / `alias1.go`） |
| `SSA-UNSUPPORTED-FUNC-VALUE` | 7 | 函数字面量当值、具名函数当值（`closure1/2.go` / `ken/litfun.go` / `ddd.go`） |
| `SSA-UNSUPPORTED-MAP` | 5 | `map` 类型出现在表达式里（`bigmap.go` / `print.go` / `ken/complit.go`） |
| `SSA-UNSUPPORTED-DEFER` | 2 | `defer`（`closure4.go` / `defernil.go`） |

**两向自洽**（`rgoc/crates/rgoc-driver/tests/ssa_slice.rs`）：

1. `N + M == 69`（两个数都写死，任一侧漂了都红）；
2. 拒绝清单的**实测集合 == 清单集合**（逐文件重跑构建器，路径与码都比）——
   清单一旦与代码脱节，「46/23」就只是历史陈述；
3. 每行三栏、原因非空、码 ∈ `SsaCode::ALL`。

> **纪律**：`N` 低于 30 时如实报，不复制用例凑数（沿 `M5-tests.md` §6.3）。本次 `N=46`。

---

## 7. 门禁（`SSA-1..9`，定稿）

| 门禁 | 名字 | 判据 | 测试 ID |
|---|---|---|---|
| `SSA-1` | IR 形状与 dump | 变体穷尽 + dump 确定 + `C10` 冻结 | `T-SSA-01/13/14/46` |
| `SSA-2` | verifier 有牙 | 反例全拒（9/9）+ 合法图全过（10/10）+ 可定位 | `T-SSA-02..12` |
| `SSA-3` | HIR→SSA 构建 | 控制流/多值/调用/复合/指针/取址各一 | `T-SSA-15..30` |
| `SSA-4` | 三方差分 | `N` 上 `HIR = SSA = go run` | `T-SSA-31/32/34..37` |
| `SSA-5` | pass 语义保持 | 每 pass 开关两路一致 + 每 pass 后 verifier 绿 | `T-SSA-60..64` + `T-SSA-51` |
| `SSA-6` | memory 模型 | 同根保序 / 异根不串 / 堆根全序 / 全局跨函数 | `T-SSA-07/25/36/49` |
| `SSA-7` | 不支持面 | 9 条各一码：7 条有直接 fixture（码 + 退出码 1），3 条够不着的改成守卫测试（`R-31-2`） | `T-SSA-33/38` |
| `SSA-8` | 活跃性与协议 | 值级活跃正确（含回边）+ 协议自描述 + 消费方桩 | `T-SSA-55..59` |
| `SSA-9` | 回归与收口 | 四条 cargo + 五条自检 + M5 零回归 + 文档 | `T-SSA-42..45` |

---

## 8. 判定口径的三条纪律（沿 M4/M5 的教训）

1. **不报「大部分绿」**：`N` 与 `M` 成对报，`N+M=69` 两向断言。
2. **不把「弱绿」记成 pass**：verifier 的合法图正例必须**真的被检查过**（`T-SSA-11` 反向证明它不是一律拒）。
3. **变异红在哪就记哪**：`§3.5` 的「期望红在」是**预测**，实测不同就改预测并留痕。

---

## 9. 修订机制

| 修订 | 日期 | 内容 | 触发 |
|---|---|---|---|
| `R-12-1` | 2026-10-23 | `verify` 的签名是 `verify(&SsaFunc, &TypeTable)`，**不是**设计稿的 `verify(&SsaFunc)` | `V3` 要判 `i64 + string`，必须能按 `TypeId` 解析真实类型；类型表是**包级**的，塞进 `SsaFunc` 荒唐（每个函数背一张表）。`M6-design.md` §3.5 同步改 |
| `R-12-2` | 2026-10-23 | `V2` **不查 `mem` 操作数**，`mem` 的支配性归 `V6` | 实测：`V2` 先跑会把 `V6` 的 ① 挡成不可达（一条永远绿的断言）。分工改为「`V2` 判普通值、`V6` 判 mem 链」 |
| `R-13-1` | 2026-10-23 | `V5` 的被调方签名存在 `SsaFunc::call_sigs`（新增 `ir::Sig { params, results, variadic }`），不是模块级符号表 | 让 `SsaFunc` 自足（dump 确定性 / 单函数测试 / 后续 passes 都不必拖模块上下文）；代价是被调方签名在多个调用者里各记一份 |
| `R-28-1` | 2026-10-24 | **聚合类型（结构体 / 数组）的槽不提升**（一律进内存） | `Instr::Field` 只在**地址**上取字段，IR 里没有「从结构体值里取出第 k 个字段」的指令。留在内存后 `s.f` 就是 `Load(Field(AddrOf(root_s), k))`，不需要新指令。代价：结构体/数组局部变量全走内存 |
| `R-28-2` | 2026-10-24 | `range` 的下标可以用**构建器私有槽**（`fresh_slot`：`u32::MAX-1` 往下，类型记在 `extra_slots`，直接进 `promoted`） | `for range xs` 在 HIR 里没有 key 位，但下标必须跨块携带 ⇒ 需要一个槽当载体。私有槽只活在本函数内，永远不是根 |
| `R-28-3` | 2026-10-24 | **`range` 并入 Task 28**（计划原先没有这一项）；支持 `Array`/`Slice`/`Int`，`Str`/`Map` 显式拒绝 | 没有 `range` 连变参的语义都验不了（`func sum(xs ...int)` 的基准就是 range 遍历）。`Str` 要按 rune 解码、当前指令集没有对应操作；`Map` 的键相等是宿主语义（`SSA-UNSUPPORTED-MAP`） |
| `R-28-4` | 2026-10-24 | **修 `rgoc-hir` 的变参实参收敛**：末尾那段实参收敛到**元素**类型，不是整个 slice 类型 | `sum(1, 2, 3)` 里 `1` 被标成 `[]int`（`arg_types[0]` 是 slice 形参类型，逐位收敛把它整体转了）。解释器不看静态类型所以 M5 从未暴露；SSA 的 `V3` 当场拒绝。这是 M5 留在 HIR 里的真错 |
| `R-23-1` | 2026-10-24 | slot 提升的判据改为「**读前必已写**」，不再看「函数末尾的 `must` 集合」 | 循环体的推进不传出循环（可能 0 次）⇒ 循环体内声明并使用的槽永远不在末尾 `must` 里，于是既不提升也没有根、构建器 panic。判据只看「每一次读是否在此前写过」，循环体推进传不传出只影响**读点判定**（那正是要保守的方向） |
| `R-24-1` | 2026-10-24 | 合流点（`if`/`switch`/标签块）**值与 mem 都要合**；抽成 `join_into`，`if` 与 `switch` 共用 | `stmt_if` 初版只合了 `val`，`mem` 留在「最后建的那条分支」上 ⇒ 一条分支里 `Store` 过、另一条没有时，合流后 mem 变成没 store 那条的（把 Store 从 mem 链上摘掉）。实测 `if c { *p = 1 }; return *p` 被 `V6` 抓（合流块里 `Load` 的 mem 不支配） |
| `R-25-1` | 2026-10-24 | **含 `goto` 的函数一律不提升**（所有槽进内存）；标签块按「全部根」定参数布局 | `goto` 是非结构化跳转：前向 `goto` 时目标块还不存在、反向 `goto` 时目标块参数已定死 ⇒ 提升后的回边带不进新值，且类型/支配全对、`verifier` 全绿。全走内存后值住在帧里，跳转不需要携带值语义（`ssa_exec` 顺序执行，帧是共享的） |
| `R-22-1` | 2026-10-24 | 类型判等一律走 `rgoc_types::identical`，**不许比 `TypeId` 下标** | `TypeTable::add_basic` **不去重**：同一个 `int` 在表里有多条（实测 `1` 与 `2` 拿到 `t45`/`t46`，都打印成 `int`）。比下标会把同一类型判成不同类型 —— 构建器插假转换、`V3` 拒合法边 |
| `R-22-2` | 2026-10-24 | 构建器在**写入点**物化隐式转换（`coerce_to`：槽类型 / 签名结果类型） | HIR 的常量带 `untyped` 类型，Go 的隐式转换在 lowering 里没落成节点。物化后得到不变式「槽的当前值类型恒等于槽的静态类型」 |
| `R-19-1` | 2026-10-24 | Task 19（slot 提升）**执行顺序挪到 Task 21 之后**（编号不变） | `T-SSA-26` 的判据是「无取址函数里没有 `Load/Store`」—— 那要看指令，而指令要到 Task 20/21 才建得出来。计划把分析（18/19）排在构建（20/21）之前，是可执行性问题 |
| `R-20-1` | 2026-10-24 | `V3` 放宽：`ConstVal::Int(0)` 在**任意**类型上合法（表示「该类型的零值」） | SSA 没有 `nil` 常量，`ConstVal` 属于 M5 契约（不为 SSA 加第 5 个变体）；有类型的 `nil`、结构体零值、mem 令牌种子都用 `Int(0)` |
| `R-20-2` | 2026-10-24 | `build(program, &mut TypeTable)`（原计划 `&TypeTable`） | `AddrOf` 的 `dst` 要是 `*T`，指针类型只能由 `add_pointer` 新建；拿不可变的表只能在 IR 里编假类型 |
| `R-21-1` | 2026-10-24 | 根表有**三条**来源：取址 ∪ 部分写 ∪ **读前未赋值** | 三条根因不同、结论相同（不能提升）：`&x`、`s.f = 1`（SSA 没有改字段指令）、`var a int; return a`（零值只能由帧初始化给） |
| `R-17-1` | 2026-10-23 | `Reject` 没有 `pos` 字段，改为 `{ func, code, detail }` | HIR 的 `Expr`/`Stmt` **不带位置**（位置只在 lowering 期的 AST 上有）⇒ `pos` 会永远 `None`；留一个永远空的字段等于骗读的人 |
| `R-18-1` | 2026-10-23 | `scan_addressed(&TypeTable, &Block)` 比计划多一个类型表参数 | `&arr[i]`（数组）元素在数组槽的存储里 ⇒ 数组槽成根；`&s[i]`（slice）元素在底层数组（堆根）⇒ 不建槽根。两条结论相反，只能按**类型**分叉 |
| `R-16-1` | 2026-10-23 | `dump` 的 `load`/`store` 行**带 mem 令牌**；可选操作数缺省写 `_`；`Effect` 不进 dump | 骨架期 `C10` 漏了 mem 字段，而 mem 是 `V6` 的比对面（不写它 dump 里看不见 mem 链）。见 `C10` §5 同号行 |
| `R-31-1` | 2026-10-24 | 白名单 **7 → 9**：新增 `U-M6-08`（函数值，`SSA-UNSUPPORTED-FUNC-VALUE`）与 `U-M6-09`（`range` over string，`SSA-UNSUPPORTED-RANGE-STR`） | 实测发现两条都没有码：① `f := func(){}` / `f := g`（具名函数当值）在 IR 里无法表示（没有「函数引用」值），原先落 `todo!()` = **panic**；② `range` over string 在计划里写了「显式拒绝」却没有码。纪律：拒绝必须**有码**（§4.6），panic 不算拒绝 |
| `R-31-2` | 2026-10-24 | `U-M6-05/06/07` 的正向证据改为**守卫测试**（断言码**不出现**，拒绝来自上游），而不是「一个直接 fixture」 | 实测：`make(chan int)` 在 lowering 就拒（`HIR-UNSUPPORTED-CHAN`）；`range` over func 无 `RangeKind` 变体（`U-M5-06`）；泛型被检查器挡住（`G-M5-03`）。三条码在**本前端够不着** —— 硬造 fixture 只能靠改前端，那是假证据 |
| `R-33-1` | 2026-10-24 | Task 33 只做**不碰内存**的值计算（`Const/Copy/Bin/Un/Convert/Composite/Make/Slice` + `print`/`println`）；`Field/Index/AddrOf` 并入 Task 35。`ops` 的实际接口是 `bin_arith` / `compare`（**没有**计划稿写的 `apply_binary`/`apply_unary`） | ① 地址类指令要 `Cell`，与 `Load/Store` 是同一套运行期设施，拆开做只会写两遍；② 没有 `print`/`println` 就没有**观察通道**，Task 33 的判据无从落地；③ 计划稿的接口名是设计期猜的，实际 `ops` 只导出了 `bin_arith`/`compare`/`normalize` |
| `R-33-2` | 2026-10-24 | **`&&` / `||` 在 SSA 里没有短路**（`G-M6-08`）：`build.rs` 把 `Binary{LAnd}` 建成一条 `Bin`，两侧都求值 | M5 的解释器有 `Task::Short` 做短路；SSA 侧漏了。结果值仍然正确、副作用（调用/panic）多执行一次 ⇒ **静默错**，只有三方差分能抓。插入 **Task 33b** 修 |
| `R-35-1` | 2026-10-24 | `Instr::Call` 增加 `dots: bool`；`C10` 的 `call` 行在实参表后写 ` ...` | `append(s, xs)` 与 `append(s, xs...)` 的**实参个数一样**，差别只在「展开 vs 单个」。不记这一位，IR 表达不出语义，求值器只能猜（猜错 = 静默算错）。实测：`c := append(a, b...)` 原先只追加一个元素（`len(c)==3`） |
| `R-35-2` | 2026-10-24 | 「指针底座」三处修正：① `expr_addr_root` 对**指针类型**的表达式先分流（值本身就是地址）；② `place_addr_root(Field)` 的指针底座用 `read_place`；③ `scan_place_root`/`scan_value_root` **不把**指针底座标成根 | 实测 `func (c *Counter) Add()` 的 `c.n += d` 建成了「指针的指针取字段」（`addr-of r_c` 再 `field`），运行期报「取字段的底座不是结构体」。③ 是**提升**面的修正（不标根 ⇒ 指针形参留在值里）；① 才是正确性面 |
| `R-35-3` | 2026-10-24 | `switch` 判定链改成**逐表达式短路**（每个 case 表达式一个 `if` 块），不再用 `bin lor` 串 | Go 规范：case 表达式自左向右求值，第一个相等的生效。`case f(2), f(3):` 用 `bin lor` 意味着两个操作数**都已求值** ⇒ `f(3)` 被多调一次（与 `R-33-2` 同一个坑）。实测 `ken/simpswitch.go` 因此运行期报「逻辑运算出现在 Bin 里」 |
| `R-35-4` | 2026-10-24 | 运行期**按真实地址访存**（`Value::Ptr` 里的 `Cell`），计划稿写的「按 `root` 索引 mem（`Vec<Value>` by `RootId`）」**作废** | `RootId` 是**函数内**下标：`&a` 传给 `inc(p *int)` 之后，被调方必须指回调用方的槽 —— 只有真实 `Cell` 能做到。`root` 只用于静态定序（`D-M6-5`） |
| `R-35-5` | 2026-10-24 | `range` 的计数器改用**私有槽**，用户的下标槽只在**体首**被赋值 | 实测 `for i = range [5]int{} {}` 结束后 `i == 5`，而 Go 规范要求 `4`（最后一次迭代的下标）。拿用户槽当计数器时，出口送出的是「越过末尾」的那个值（`for.go` 的 `assertequal(i, 4, ...)`） |
| `R-35-6` | 2026-10-24 | 值语义的**两处** `deep_copy`（`Load` 与 `Store`）登记为**互为冗余**：判据只钉「至少一处生效」 | 实测（变异 `M34`）：单独去掉任一处，全部用例仍绿（另一处把拷贝做了）；**两处一起去掉**才红。原因是「元素/字段写」会把槽标成根（`partial`），赋值必然走 `Store` 而读必然走 `Load`。所以判据写成端到端的「`b := a` 之后改 `b` 不影响 `a`」，而不是「某一行必须拷贝」 |
| `R-35-7` | 2026-10-24 | `stmt_labeled` 的「死代码跳过」加**全函数 goto 目标预扫描**（`goto_targets`） | `goto X; ...; L: ...; X: goto L` 里 `L` 写下时还没有入边，之后才被指到。跳过它留下空块（`unreachable`），运行期走到就报「执行到了 Unreachable 块」—— 实测 `ken/label.go` |
| `R-36-1` | 2026-10-24 | `T-SSA-40` 的「墙钟 < 10 s」用**显式预算 1e7** 判；默认 1e8 只用常量断言钉住 | 实测 debug 构建吞吐约 **9.5M 步/秒**（1e7 步 ≈ 1.05 s）⇒ 默认 1e8 要 **10.4 s**，拿它断言等于把机器速度写进测试（release 下又必然过）。判据仍是「死循环必须停下且给出原因」，只是分母换成一条不会随机器翻车的值 |
| `R-37-1` | 2026-10-24 | `run --via=ssa` 在 `build` 与 `run_ssa` 之间**强制**跑 verifier；并修 `struct_field_ty`/`elem_ty_of` 不认指针底座 | ① 求值器假定 `V1..V9` 成立，图不合法时行为未定义（可能碰巧对）⇒ 「要么合法、要么响亮报错」必须是硬边界；② `via` 的第一条端到端用例（`p.x += d`，`p *P`）当场抓到 `V3` 失败：`struct_field_ty(*P, 0)` 返回 `None` ⇒ 字段地址类型退化成 `TypeId(0)`（`bool`），Load 的 `typ` 跟着错。加 `pointee_of` 并让两个推导函数穿透指针 |
| `R-6-1` | 2026-10-24 | §6 的 `N`/`M` 冻结为 **46 / 23**；`M` 侧落 `tools/run-oracle/ssa-rejected.txt`（23 行三栏） | Task 39 实测（分类器 + `ssa_slice.rs` 两向断言）。此前 §6 只是「待 Task 3 实测后冻结」的占位；冻结值 = 2026-10-24 的语料（go1.27.1）+ 本仓构建器 |
| `R-40-1` | 2026-10-24 | `--opt` 默认 `all`（`ssa` 与 `run` 都有）；且**先验后优化**：`opt.any()` 时先跑一遍 verifier，坏图直接报（1）不进 pass | pass 假定 `V1..V9` 成立。在坏图上做变换，出来的错会被报成「pass 的 bug」，而真正的错在构建器 —— `T-SSA-33` 的泛型守卫就是这么暴露的（图本来就 `V3` 不合法，pass 之后变成「内部错 2」） |
| `R-40-2` | 2026-10-24 | 删指令的 pass 收尾必须**值压实**（`compact_values`），且压实要同步 `SsaFunc::params` | `V1` 遍历的是**全部** `value_types`：任何下标没有定义点就报「v_k 从未被定义」⇒ 删了指令不压实，下一轮 verify 立刻红。而 `SsaFunc::params` 是入口块 params 的**副本**（`C10` 规则 2），只改块不改它，求值器绑形参与函数体读块参数就指向不同的值（实测：`p.add(4)` 的结果少加一次） |
| `R-41-1` | 2026-10-24 | `fold` 的边界：整数按**宽度回绕**折；除零/模零、负移位、浮点→整数、复数**一律不折** | 折错一个值 = 程序在没有任何报错的情况下算错。Go 对整数溢出/转换有**明确定义**（回绕/截断）⇒ 可以折；除零是运行期 panic、浮点→整数是「实现定义」⇒ 折了就是替实现做决定 |
| `R-42-1` | 2026-10-24 | `Effect` / `Summary` 增加 **`prints`** 维度（`print`/`println` 置真，经调用图传递） | `print` 在 `writes`/`may_panic`/`may_call_back` 三个维度上都是「纯」的，可它**会往 stdout 写字节** —— 少了这一位，`dce` 把没人用的 `println` 删掉：图合法、verifier 全绿、程序少输出（实测 `cplx0.go` 只剩一半输出） |
| `R-42-2` | 2026-10-24 | 摘要的「存储归属」看**引用型**槽（指针 / 切片 / map），不只看指针；`summarize` 因此要收 `&TypeTable` | `s[i] = v`（`s []int`）改的是**底层数组**。判成「本地」⇒ 摘要说「这个函数什么都不写」⇒ `dce` 把 `setpd(a)` 删掉（实测 `ken/array.go`：`t` 一直是 0） |
| `R-43-1` | 2026-10-24 | `CalleeRef::Method { recv }` 是**使用点**（`for_each_use` / 值压实都要走到） | 漏掉它的后果是双重的：`dce` 以为接收者的地址值没人用而删掉它；压实又把 `recv` 留在旧编号上（实测：`p.add(4)` 的接收者变成一个整数常量，运行期报「地址不是指针」） |
| `R-44-1` | 2026-10-24 | 块内 CSE 的键：`Bin` 的 `typ` 冗余（`V3` 要求结果类型 == 操作数类型），`Convert` 的 `typ` 是语义的一部分；**不**利用交换律 | 浮点加法不满足位级交换等价（NaN 载荷 / `-0.0`），为省一条指令引入这种不确定性不划算 |
| `R-45-1` | 2026-10-24 | `Instr::dsts()` / `Instr::uses()` / `Term::uses()` 进 **IR**（`ir.rs`），不在 `opt`/`live` 里各写一份 | 「一条指令定义谁、读谁」是 IR 的**形状**。抄两份的话，新增一个指令变体必然只改一处，而漏的那一处会安静地算错（`opt` 的 `CalleeRef::Method.recv` 就是这么漏的，`R-43-1`） |
| `R-45-2` | 2026-10-24 | `live_out` 公式里的 `− params(s)` 登记为**冗余**（无判据覆盖） | 块参数算作「块入口已定义」⇒ `live_in[s]` 里永远没有它，所以这一项删掉也不变。真正有判据的是「边实参进 `live_out`」（`M65`）与「块参数不进 `live_in`」（`M56`）。不假装这一项有覆盖 |
| `R-46-1` | 2026-10-24 | `SSA-LIVE v1` 语法定稿：列表带方括号（`live=[v1,v2]` / `ptrs=[]`），空集写 `[]` 不省略字段，块入口定义点写 `b<k>:-1` | 设计稿 §3.7 的 `live=<v1,v2,...>` 是占位写法，落到实现必须二选一。选方括号是为了与 `C10` 的 `params=[]` 同口径（同一个仓里两套列表语法只会让人猜）；`-1` 是「比任何指令下标都小」的哨兵，比给 `Pos` 加一个 `enum` 更省且排序天然正确 |
| `R-48-1` | 2026-10-24 | safepoint 的定义 = **能调出去或能分配**（`Call` 任何被调方 + `Make`）；`ptrs` 只收 `*T` 并**排除 mem 令牌**；根闭包只作用于 safepoint 的 `live`，`live_in/live_out` 保持精确值级 | ① safepoint 的意义是「别的代码可能跑起来、GC 可能发生」，`Load`/`Store` 不是；② mem 令牌的静态类型就是 `*T`（`Fb::mem_of` 用 `const 0 : *T` 起头）但不是对象地址，不排掉 `ptrs` 里会混进常量 0；③ 若把闭包写进 `live_in/live_out`，「与手算一致」这条判据就没法写了（手算的是精确值级） |

| `R-49-1` | 2026-10-24 | `T-SSA-52` 的判据从端到端 `ssa_run::并行赋值交换` **改指向手搓 IR** `ssa_exec::回边上对调两个块参数` | 实测：端到端 `a, b = b, a` 不经块参数 —— HIR 靠**改名**解决（dump：`goto b4 (v4,v3,v5)`，交的是 latch 自己的参数）。所以那条测试证明的是「改名方案正确」，**不覆盖** `jump` 的并行语义。手搓的回边 `goto b1(p1, p0, pc+1)` 才真正把两个块参数对调，顺序赋值得 `2 2` 而不是 `2 1`。同时更正 Task 34 的 `M30` 记录（今天无法复现） |

**修订格式**：`| R-x-y | 日期 | 改了什么 | 为什么（证据） |`。改判据**不许**静默改 —— `manifest_m6.rs` 会对账 `test_ids` 集合。
