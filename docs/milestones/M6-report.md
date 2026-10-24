# M6 报告（交付 B）：SSA、verifier 与精简优化

> **状态**：**已收口** —— 九条门禁 `SSA-1..9` 全部 `pass`；六条 `open_gaps` 均为 M7–M9 的已知边界。
> 本报告里每个数字都能由 §9 的命令复现；凡不能复现的，写在这里就是错的。
> **阶段**：M6（`rgoc ssa --dump/--verify` + `rgoc run --via=ssa` 跑起来）　|　**产出**：2026-10-24
> **上游**：[`M6-design.md`](./M6-design.md)（`D-M6-1..16` / `F1–F14`）/ [`M6-tests.md`](./M6-tests.md)（64 个 `T-SSA-*` + 修订表）/ [`M6-plan.md`](./M6-plan.md)（Task 1–53）/ [`M6-manifest.json`](./M6-manifest.json)（机器可读事实）
> **契约**：[`C4`](../contracts/C4-ssa.md)（升正式）、[`C10`](../contracts/C10-ssa-dump.md)（定稿）、[`C9`](../contracts/C9-values-and-output.md) §7.5

---

## 1. 一句话结论

**HIR 能建成 SSA 图、图能被九条不变式检查、图能被求值，而且结果与 HIR 解释器和 `go run` 逐字节一致。**
入选 69 个官方语料里 **46 个能建图**（另 23 个带白名单码显式拒绝），46 个的 SSA 输出与官方 `.out`
**逐字节一致**，且 `--via=hir` 与 `--via=ssa` 在 46 个上**逐字节一致**。
四个优化 pass（fold/dce/copy/cse）开着与关着输出一致；值级活跃性与 `SSA-LIVE v1` 协议可被一个
**只读文本**的消费方桩解析（46 文件 / 140 函数 / 678 safepoint）。
**66 条变异探针全部实证变红**。**门禁 9/9 `pass`**；**Task 1–53 全部执行完毕**。

一句话的代价：**M6 只支持顺序语义**。闭包、`defer`、接口动态派发、`map`、`chan`、泛型、函数值、
`range` over string/ func —— 9 条全部**显式拒绝**（稳定码 + 退出码 1），不是静默不支持。

---

## 2. 覆盖矩阵（roadmap §M6 的句子 × 四腿）

`S` = 自有 fixture；`O` = 官方语料对撞；`H` = 装置与 CLI；`反` = 变异探针。

| # | roadmap §M6 的句子 | S | O | H | 反 | 结论 |
|---|---|---|---|---|---|---|
| 1 | HIR→SSA 的 Block/Value/Phi | `T-SSA-15..19`、`28` | 46/46 | `ssa --dump`（`C10`） | `T-SSA-52`（块参数顺序赋值） | ✅ 兑现（phi = **块参数**，`D-M6-3`） |
| 2 | 多返回值 tuple | `T-SSA-20/21/22` | 46/46 | — | `T-SSA-50`（只取第一个结果） | ✅ 兑现（**多结果指令**，tuple 不进 IR，`D-M6-4`） |
| 3 | memory/effect | `T-SSA-25`、`36` | 46/46 | `dump` 带 mem 令牌 | `T-SSA-49`（`Store` 不出新 mem） | ✅ 兑现（**按根分区**，`D-M6-5`） |
| 4 | 调用和异常传播表示 | `T-SSA-23`、`29`、`30` | 46/46 | `Effect` 摘要 | `M41`（摘要不认引用型槽） | ✅ 兑现（`panic` 走 `Term::Panic`；`defer`/`recover` 显式拒） |
| 5 | 支配分析先于依赖它的变换 | `T-SSA-03`（32 条反例） | — | `dom.rs` 手写 CHK | `T-SSA-48`（`V2` 短路） | ✅ 兑现 |
| 6 | 先折叠/死代码/复制传播，再证明必要的 CSE | `T-SSA-60..63` | 46/46（`--opt=all` vs `none`） | `--opt=all\|none\|…` | `M49`–`M55`（7 条） | ✅ 兑现（**块内** CSE，`D-M6-8`） |
| 7 | SSA 求值器共用定义好的值与 runtime 操作 | `T-SSA-32`、`34..37` | 46/46 | `run --via=ssa` | `M33`–`M43`（11 条） | ✅ 兑现（`D-M6-7`：值模型只有一个 crate） |
| 8 | 定义 safepoint live set / 派生指针 / 协议 | `T-SSA-55..59` | 140 函数 / 678 safepoint | `SSA-LIVE v1` | `M56`–`M67`（11 条） | ✅ 兑现（**只到协议级**，`D-M6-10`） |
| 9 | 完整 M5 功能可以分批降级，暂不支持必须显式报告 | `T-SSA-33`（12 条） | 23 行拒绝全带码 | 退出码 1 + 稳定码 | `M7`–`M9`、`M14` | ✅ 兑现（9 条白名单，`R-31-1`） |
| 10 | 不要求「优化 ≥8」 | `T-SSA-64` | — | — | — | ✅ 只要求语义保持（4 个 pass，不凑数） |

**四腿分母**：甲 14 / 乙 16 / 丙 7 / 丁 9 / 反 8 / 戊 5 / 己 5 = **64 个 `T-SSA-*`**（`M6-tests.md` §3）。

---

## 3. 三条分别声明（不许含糊）

### 3.1 SSA 支持面（能建图的是什么）

| 项 | 结论 |
|---|---|
| **语料** | 69 个入选文件里 **46 个能建图**（`N=46`），**23 个被拒**（`M=23`），`N+M=69` 两向断言（`T-SSA-31`） |
| **拒绝码分布** | `IFACE-DYN` 9 / `FUNC-VALUE` 7 / `MAP` 5 / `DEFER` 2 —— 每行三栏落 `tools/run-oracle/ssa-rejected.txt` |
| **拒绝的形状** | 稳定码 `SSA-UNSUPPORTED-*` + 退出码 **1**（静态面）；**不是** panic，也不是静默跳过 |
| **能建图的范围** | 直线 / `if` / `for` 三形态 / `switch` / `goto` 与标签 / 多返回值 / 直连 / 变参 / 方法调用 / 复合字面量 / 切片三索引 / `make`·`new` / 指针写穿 / 全局与包初始化 |
| **明确不做** | 闭包（捕获跨帧写）、`defer`/`recover`、接口动态派发与 type switch、`map` 读写、`chan`/`go`/`select`、泛型实例化、函数值、`range` over string / func |

**为什么「能建图」不等于「支持」**：46 个建图之后还要过 verifier（九条不变式）与三方差分。
三条都过才叫支持 —— 这也是为什么 `M` 侧的 23 个文件**不**记成 pass（`R-31-2` 的 3 条守卫同理）。

### 3.2 优化边界（做什么、不做什么）

| pass | 做什么 | **不做**什么（每条都有理由） |
|---|---|---|
| `fold` | `Bin`/`Un`/`Convert` 常量折叠 + `If` 常量条件改 `Goto`（连带清死块） | 整数按**宽度回绕**折（Go 语义）；**除零 / 负移位 / 浮点→整数 / 复数不折** —— 折了就是替实现做决定（`R-41-1`） |
| `dce` | 删无使用且无副作用的值 + 清死块 | `Store` / 有效果的 `Call` / 间接调用**永不删**；`print` 单列为 `prints` 维度（`R-42-1`） |
| `copy` | `Copy` 链解析到终点后整条消掉 | 不破坏并行语义（先建全表、再一次性替换） |
| `cse` | **块内**纯值 `(op, 操作数, 类型)` 合一 | **不跨块**（那要支配树 + 可用表达式，属 M7）；**不利用交换律**（浮点位级不等价，`R-44-1`） |

**开关**：`--opt=all|none|fold,dce,copy,cse`（默认 `all`）。**判据不是「变快了」，是「没变语义」**：
`opt_equiv` 在 46 个 `N` 上比 `--opt=all` 与 `--opt=none` 的**输出逐字节**；每个 pass 之后 `opt::run`
自己跑一遍 verifier（`SSA-5`）。**没有性能数字** —— M6 不承诺优化收益，只承诺语义保持。

### 3.3 live set 与协议（`M6e` 只到协议级）

| 项 | 结论 |
|---|---|
| **算什么** | 值级 `live_in/live_out`（含回边，迭代到不动点）+ 值区间 `[def, last_use]` + safepoint 存活集 |
| **safepoint 的定义** | **能调出去或能分配**的指令：`Call`（任何被调方）+ `Make`（`make`/`new`）。`Load`/`Store` 不是 |
| **派生指针** | 保守：`&s.n` 活 ⇒ 根 `&s` 也标活（**根级**闭包，不做对象级） |
| **协议** | `SSA-LIVE v1`：`# safepoint b<i> <idx> live=[…] ptrs=[…]` + `# interval v<k> def=… last=…`，排序键 = 块号 + 指令下标，逐字节确定 |
| **可消费性** | `live_consumer.rs` 的解析器**只吃 `&str`**（签名里没有 `SsaFunc`/`Cfg`/`TypeTable`）；46 文件 / 140 函数 / 678 safepoint / 48 个有存活指针 |
| **不做什么** | 不做逃逸分析、不做对象级根集、不做栈图/寄存器映射（那是 M7 `PIPE-17`）、`ptrs` 只含 `*T`（切片/map 的底层存储属 M9） |

---

## 4. 语料实测（`SSA-4` 的核心）

```text
候选（M5 入选）= 69
  ├─ N = 46  能建图 → verifier 过 → SSA 求值
  └─ M = 23  构建期拒绝（每行带白名单码 + 原因）
断言：N + M == 69（两向）；M 的码集 ⊆ SsaCode::ALL（唯一来源）
```

| 比对 | 结果 |
|---|---|
| SSA 输出 vs 官方 `.out` | **46/46 逐字节一致**（`ssa_corpus_run.rs`，1.3 s） |
| `--via=hir` vs `--via=ssa` | **46/46 逐字节一致**，23 个按码跳过（`ssa_diff.rs` + `run-collide.sh --ssa`） |
| 三方（+ `go run`） | 5 个有 `.out` 的文件三方一致；其余 64 个按「期望为空」只做两方 |
| `--opt=all` vs `--opt=none` | **46/46 逐字节一致**（`opt_equiv.rs`） |

**语料实测抓到的真错**（不是设计推断出来的，是跑出来的）：

| # | 错 | 抓到它的东西 |
|---|---|---|
| 1 | `read_place` 拿**地址类型**（`*[30000]uint8`）当 `Load.typ` ⇒ `a[pc] += 1` 的加法变「指针 + 整数」 | `turing.go`（`V3` 当场拒） |
| 2 | 二元运算的操作数**没有物化隐式转换** ⇒ `int + untyped int` 一片 `V3` | 多处 |
| 3 | `range` 的上界/自增用 `int`，而槽可能是具名类型 | `range3.go` |
| 4 | `switch` 判定链用一条 `bin lor` 串 case 表达式 ⇒ 副作用多做一次 | `ken/simpswitch.go` |
| 5 | 前向 `goto` 的目标被当死代码跳过 ⇒ 留下空块 | `ken/label.go` |
| 6 | `for i = range [5]int{}` 结束后 `i == 5`（应 4） | `for.go` |
| 7 | `&&` 的出口常量用缓存 `bool`，而结果可能是 `untyped bool` | 5 个文件 |
| 8 | `Effect` 缺 `prints` ⇒ `dce` 删掉 `println`（少一半输出） | `ken/cplx0.go` |
| 9 | 摘要的存储归属不认**切片**槽 ⇒ `setpd(a)` 被删 | `ken/array.go` |

> 这九条里 **6 条**是「语料第一次真跑」才暴露的。手写 fixture 全绿而语料全红是**完全可能**的
> —— 这是 M4 的教训（弱绿只证明「不乱报」），M6 用同一套装置把它又抓了一遍。

---

## 5. 偏离登记（与冻结口径不一致的每一处）

完整表在 `M6-tests.md` §9（**44 条** `R-*`）。这里列**改变了契约形状**的十条：

| 修订 | 偏离 | 为什么（证据） |
|---|---|---|
| `R-12-2` | `V2` **不查 `mem` 操作数**，`mem` 的支配性归 `V6` | `V2` 先跑会把 `V6` 的一条判据挡成**永远不可达**（永远绿的断言） |
| `R-22-1` | 类型判等一律走 `identical`，**不许比 `TypeId` 下标** | `add_basic` 不去重：同一个 `int` 在表里有多条（`1`/`2` 拿到 `t45`/`t46`） |
| `R-28-1` | **聚合类型（结构体/数组）的槽不提升** | IR 没有「从结构体值取第 k 个字段」的指令；留在内存后 `s.f` 就是 `Load(Field(AddrOf(root)))` |
| `R-31-1` | 白名单 **7 → 9**（补函数值 / `range` over string） | 这两条原先落 `todo!()` = **panic**，而 panic 不是「拒绝」 |
| `R-31-2` | `U-M6-05/06/07` 的正向证据改成**守卫测试** | 三条码在本前端够不着（HIR/检查器先拒），硬造 fixture 等于假证据 |
| `R-35-4` | 运行期**按真实地址**访存，计划稿的「按 `root` 索引 mem」**作废** | `RootId` 是函数内下标；`&a` 穿过帧边界必须指回调用方的 `Cell` |
| `R-40-2` | 删指令的 pass 收尾必须**值压实**（且同步 `f.params`） | `V1` 遍历全部 `value_types`，没有定义点就红；不同步 `params` 会让形参与块参数指向不同的值 |
| `R-42-1` | `Effect`/`Summary` 增加 **`prints`** 维度 | `print` 在 writes/panic/callback 上都是纯的，但删了**少输出** |
| `R-46-1` | `SSA-LIVE v1` 语法定稿：方括号列表 / `b<k>:-1` 入口位点 | 设计稿的 `live=<v1,v2,...>` 是占位写法，落地必须二选一 |
| `R-51-1` | `T-SSA-46` 的 golden 改成**从文档读** | 抄进测试的期望值只能证明「实现与那份副本一致」；改完一次抓出 `C10` 的三处文档错 |

**还有两条「诚实的例外」**（不假装有覆盖）：

1. `R-35-6`：`Load`/`Store` 两处 `deep_copy` **互为冗余** —— 单独去掉任一处全绿，两处一起去掉才红。
   判据因此写成端到端的值语义（`b := a` 之后改 `b` 不影响 `a`）。
2. `R-45-2`：`live_out` 公式里的 `− params(s)` 在当前 use/def 约定下**永远不起作用**，没有变异能杀掉它。

---

## 6. 缺口清单（`open_gaps`，逐条带「卡住谁」）

| ID | 缺口 | 卡住谁 | 归属 |
|---|---|---|---|
| `G-M6-01` | **构建预算未实现**（超 1e5 指令的函数应被构建期拒绝，`T-SSA-41`） | 极端输入的健壮性；语料最大函数远小于 1e5 所以不暴露 | M7（与寄存器分配的规模上限一起做） |
| `G-M6-02` | 派生指针只做**根级**闭包，不做对象级（`&s.n` 活 ⇒ 整块 `s` 活） | 内存回收精度（会多留） | M9 |
| `G-M6-03` | `ptrs` 只含 `*T`；切片 / `map` 的底层存储不在 live 协议里 | 真正的 GC 根表 | M9 |
| `G-M6-04` | CSE 只在块内；无跨块/循环优化（LICM） | 性能（不影响正确性） | M7 |
| `G-M6-05` | 9 条白名单全部拒（闭包/defer/接口动态/map/chan/泛型/range-func/函数值/range-str） | 功能面（不是收口漏洞：拒绝是**显式**的） | M7–M9 |
| `G-M6-06` | `ssa_exec` 对 `CalleeRef::Indirect` 运行期报错 | 函数值调用（M8 的 goroutine/闭包需要它） | M8 |

**已关闭的缺口**（收口时确认）：`G-M6-07`（`init_order` 没进 SSA ⇒ Task 32b）、`G-M6-08`（`&&`/`||` 无短路 ⇒ Task 33b）。

---

## 7. 门禁（`SSA-1..9`）

| 门禁 | 名字 | 结论 | 证据 |
|---|---|---|---|
| `SSA-1` | IR 形状与 dump | ✅ `pass` | `T-SSA-01/13/14/46`；`dump.rs` 8 条（含两个**文档驱动** golden） |
| `SSA-2` | verifier 有牙 | ✅ `pass` | `verify_negative` **32** 条反例（带 `block`/`idx`）+ `verify_positive` **16** 条正例 |
| `SSA-3` | HIR→SSA 构建 | ✅ `pass` | `ssa_build.rs` **76** 条 |
| `SSA-4` | 三方差分 | ✅ `pass` | 46 绿 / 23 跳过 / 0 红；46 个与 `.out` 逐字节 |
| `SSA-5` | pass 语义保持 | ✅ `pass` | `opt_equiv` 46 个 `all==none`；`opt_*` 共 **32** 条；每 pass 后 verify |
| `SSA-6` | memory 模型 | ✅ `pass` | `V6` 反例 + 写穿同根 + 全局跨函数 + 切片共享 + 数组元素 |
| `SSA-7` | 不支持面 | ✅ `pass` | 9 码：6 条直接 fixture + 3 条守卫；`ssa_reject` **12** 条；`DiagCode` 仍 **103** |
| `SSA-8` | 活跃性与协议 | ✅ `pass` | `live.rs` 6 条 + `live_consumer`（46/140/678/48） |
| `SSA-9` | 回归与收口 | ✅ `pass` | 四条 cargo 全 0；**176 suite / 1859 passed / 0 failed / 1 ignored**（≥ M5 的 1608）；M5 语料 69/69；`corpus-manifest.sha256` 15618/15618 |

---

## 8. 接手指南（给 M7 的人）

**先读三份契约**：`C4`（SSA 的形状与九条不变式）、`C10`（dump 格式，含 mem 令牌与 `...`）、`C9` §7.5（值模型共用）。

**四条命令**（容器内）：

```bash
rgoc ssa --dump   file.go     # 看 IR（先 --opt=none 看构建器原样）
rgoc ssa --verify file.go     # 九条不变式；不合法 ⇒ 退出码 1
rgoc run --via=ssa file.go    # 跑 SSA（与 --via=hir 必须逐字节一致）
rgoc run --via=ssa --opt=none file.go   # 关掉优化，定位「是 pass 还是构建器」
```

**四条硬约束**（踩过，别再踩）：

1. **删指令之后必须值压实**（`opt::compact_values`），且要同步 `SsaFunc::params`（它是入口块 params 的副本）。
2. **`Effect` 的五个维度**：`writes` / `writes_all` / `may_panic` / `may_call_back` / **`prints`** —— 少一个维度
   就会删掉「看起来没用」的指令。
3. **摘要的存储归属要看引用型槽**（指针 / 切片 / map）：`s[i] = v` 写的是底层数组。
4. **`&&`/`||` 是控制流不是 `Bin`**；`switch` 的判定链也必须**逐表达式短路**（两处都是「两个操作数都已求值」的坑）。

**三个「改之前先想」的点**：

- 往 `Instr` 加变体 ⇒ 同时改 `Instr::dsts`/`uses`、`dump::render_instr`、`typing::check_instr`、`ssa_exec::step_instr`、
  `opt` 的四个 pass —— 漏一处会**静默算错**（不是编译错）。
- 往 `Module`/`SsaFunc` 加字段 ⇒ 想清楚「这是 IR 形状还是运行期信息」。运行期信息（`global_types`/`slot_types`/`dots`）
  进 IR 是因为求值器**不能回头问 HIR**（`D-M6-2` 的单包前提之外还有「IR 要自足」）。
- 改 `live` 的语义 ⇒ 先看 `T-SSA-55` 的三条手算对照；`live_in/live_out` 必须是**精确值级**，
  保守闭包只允许加在 safepoint 的 `live` 上。

---

## 9. 复现命令（全部容器内，`cd /work/rgoc`）

```bash
# ① 四条门禁
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --workspace --no-fail-fast        # 期望 176 suite / 1859 passed / 0 failed / 1 ignored

# ② 语料三方（在 /work 下）
bash tools/run-oracle/run-collide.sh --run --list --ssa --three-way tools/run-oracle/run-selected.txt
# 期望：69 个文件：绿 46 / 跳过 23 / 红 0

# ③ 语料只读（15,618 条）
shasum -a 256 -c corpus-manifest.sha256 | grep -vc OK   # 期望 0

# ④ 关键单条
cargo test -p rgoc-driver --test ssa_slice      # N=46 / M=23 两向
cargo test -p rgoc-driver --test ssa_corpus_run # 46 个与官方 .out 逐字节
cargo test -p rgoc-driver --test ssa_diff       # hir == ssa
cargo test -p rgoc-driver --test opt_equiv      # all == none
cargo test -p rgoc-driver --test live_consumer  # 协议可消费
cargo test -p rgoc-driver --test manifest_m6    # manifest 对账（四类谎）

# ⑤ 变异（66 条，脚本在 .superpowers/sdd/M6-plan/，不入库）
#    每条：cargo clean -p <crate> → 施加 → 跑指定测试 → 必须红 → 复原
```

---

## 10. 本报告与 `M5-report.md` 的关系

M5 交付的是**顺序语义基线**（解释器 + 69/69 语料）；M6 交付的是**同一语义的第二条执行通路**
（HIR → SSA → 求值）与它的**可验证性**（九条不变式 + 优化 pass 的语义保持 + live 协议）。

两者不重叠也不互相替代：

| 问题 | 谁回答 |
|---|---|
| 「Go 语义对不对」 | `M5-report.md`（解释器 + `go run` 三方） |
| 「这张 SSA 图合法吗」 | M6 的 `V1..V9`（`ssa --verify`） |
| 「SSA 与解释器算得一样吗」 | M6 的 `SSA-4`（46 个上逐字节） |
| 「优化改了语义吗」 | M6 的 `opt_equiv`（`all` vs `none`） |
| 「codegen 能拿到什么」 | M6 的 `SSA-LIVE v1`（**协议级**，不是栈图） |

**M6 不承诺**：native 代码生成、寄存器分配、逃逸分析、GC、并发 —— 那是 M7–M9。
