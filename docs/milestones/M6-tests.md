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
| 白名单 | §5，**7 条**，开工前冻结；跳过项一律**不得**记为 pass |
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

## 5. 白名单（`unsupported`，开工前冻结，7 条）

| ID | 项 | 为何不做 | 正向证据 | 计划阶段 |
|---|---|---|---|---|
| `U-M6-01` | 闭包（捕获跨帧写） | `Func.captured` 非空 ⇒ 帧槽与外层 `Cell` 别名，SSA 的「值唯一定义」不成立 | `T-SSA-33` 的闭包 fixture（`SSA-UNSUPPORTED-CLOSURE`）；语料 `closure*.go` 在 `M` 里 | M8（goroutine/闭包帧） |
| `U-M6-02` | `defer` / `recover` | 延迟调用的链是**运行期**结构，不是 SSA 值 | `T-SSA-33`（`SSA-UNSUPPORTED-DEFER`）；`T-EX-34..37` 的 fixture | M8 |
| `U-M6-03` | 接口动态派发 / type switch | 目标函数在运行期才定，SSA 的 `CalleeRef` 只能间接调用（不做） | `T-SSA-33`（`SSA-UNSUPPORTED-IFACE-DYN`） | M8 |
| `U-M6-04` | `map` 读写 | 键相等是宿主语义（`C9` 的 `go_eq`），memory 根无法覆盖 | `T-SSA-33`（`SSA-UNSUPPORTED-MAP`） | M9（对象模型） |
| `U-M6-05` | `chan` / `go` / `select` | 并发语义整体不做（M8） | `T-SSA-33`（`SSA-UNSUPPORTED-CHAN`） | M8 |
| `U-M6-06` | `range` over func | HIR 层已拒（`U-M5-06`），SSA 见不到 | `T-SSA-33`（`SSA-UNSUPPORTED-RANGE-FUNC`） | M8d |
| `U-M6-07` | 泛型实例化后的形态 | 检查器没放进来（`G-M5-03`），M6 无输入 | `T-SSA-33`（`SSA-UNSUPPORTED-GENERIC`） | 检查器面 |

> **`U-M6-01..03` 是 M6 支持面最大的三块**：闭包/defer/接口动态派发恰好是「顺序语义」里最常用的三样。
> 因此 `N` 可能较小 —— 届时**如实报告**并把「哪些可提前做」列入 `open_gaps`（`D-M6-12` 的失效条件）。

---

## 6. 语料清单

| 清单 | 内容 |
|---|---|
| 候选 | M5 的 `run-selected.txt`（**69** 行，已冻结） |
| 能建图 `N` | **待 Task 3 实测后冻结**（`R-6-1` 同款：先做分类器，拿到真实数字再冻） |
| 拒绝 `M` | 同上；每行带 §5 的码，落 `tools/run-oracle/ssa-rejected.txt`（三栏：文件/码/原因） |

> **纪律**：`N` 低于 30 时如实报，不复制用例凑数（沿 `M5-tests.md` §6.3）。

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
| `SSA-7` | 不支持面 | 7 条各一 fixture + 码 + 退出码 1 | `T-SSA-33/38` |
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
| （待） | — | 本文档冻结后，任何判据改动都要走修订行（`R-6-x`），并同步 `M6-design.md` 与 manifest | — |

**修订格式**：`| R-x-y | 日期 | 改了什么 | 为什么（证据） |`。改判据**不许**静默改 —— `manifest_m6.rs` 会对账 `test_ids` 集合。
