# M6 设计：SSA、verifier 与精简优化

> **状态**：**已确认**（2026-10-23；决策 `D-M6-1..16` / 事实基线 `F1..F14` / 门禁 `SSA-1..9`）
> **权威范围**：`docs/03-roadmap.md` §M6（本文件只做**展开与决策**，不改范围）
> **上游**：`M5-design.md`（HIR 与解释器）、`M5-report.md` §8（接手指南）、`C4-ssa.md`（spike 级，本文档把它升正式）
> **下游**：`M6-tests.md`（判据）、`M6-plan.md`（任务分解）、`M6-manifest.json`（机器可读事实）

---

## 0. 本文档在流程中的位置

```text
03-roadmap §M6（做什么）→ 本文档（怎么定、边界在哪）→ M6-tests.md（怎么算通过）→ M6-plan.md（按什么顺序做）
```

三件套齐备才开工（`03` §6.2）。本文档的每一处主张都要求**可判死**：决策带五字段、事实带出处、门禁带拒绝面。

---

## 1. 目标与非目标

### 1.1 输入与输出

| | 内容 |
|---|---|
| **输入 ①** | M5 的正式 crate：`rgoc-lex → ast → parse → const → types → load → check → hir → interp → driver` |
| **输入 ②** | `rgoc-hir` 的**可执行 HIR**（54 个变体：`ExprKind` 21 / `Stmt` 14 / `Place` 5 / `Callee` 5 / `BuiltinKind` 17；`F1`） |
| **输入 ③** | `rgoc-interp` 的**值模型**（`C9`：13 个 `Value` 变体）与 `ops`（`apply_binary` / `apply_unary` / `normalize`） |
| **输入 ④** | `C4` 契约的**五项需求登记**（memory / tuple / 调用边界 / phi / verifier）—— 本文档逐条作答 |
| **输出 ⑤** | 新 crate **`rgoc-ssa`**（IR + builder + CFG/支配 + verifier + passes + dump，**纯数据不执行**） |
| **输出 ⑥** | `rgoc-interp::ssa_exec`（诊断求值器，复用 `Value`/`Cell`/`ops`/`print`） |
| **输出 ⑦** | `rgoc-driver` 的 `ssa [--dump\|--verify]` 与 `run --via={hir,ssa}` |
| **输出 ⑧** | **`C4` 升正式** + **新建 `C10-ssa-dump.md`**；`M6-{tests,plan}.md` / `M6-manifest.json` / `M6-report.md` |
| **输出 ⑨** | 装置：`run-collide.sh --ssa`、`scripts/mutation-m6-notes.md`、语料分类器（69 → N 能建图 / M 拒绝） |

### 1.2 范围（`03-roadmap.md` §M6 逐句展开）

| # | roadmap 句子 | 本文档的落点 |
|---|---|---|
| 1 | 「HIR→SSA 的 Block/Value/Phi」 | §3.2 IR 形状 + §3.3 构建算法；**phi 用块参数**（`D-M6-3`） |
| 2 | 「多返回值 tuple」 | §3.3.3 多结果指令 + 结果直连；**tuple 不进 IR**（`D-M6-4`） |
| 3 | 「memory/effect」 | §3.5 按根分区（`D-M6-5`）；`Call.effect` 声明写根集（`D-M6-9`） |
| 4 | 「调用和异常传播表示」 | `Instr::Call { dsts, callee, args, effect }`；`panic` 走 `Term::Panic`，`recover`/`defer` 显式拒绝（§5） |
| 5 | 「支配分析先于依赖它的变换」 | §3.4 支配树是 verifier `V2` 的前提，passes 在 `SSA-5` 里要求「pass 后 verifier 绿」 |
| 6 | 「先常量折叠/死代码/复制传播，再证明必要的 CSE」 | §3.6：三个 pass + **块内纯值 CSE**（`D-M6-8`） |
| 7 | 「SSA 求值器是诊断设施，共用定义好的值与 runtime 操作，不发展第三套完整 runtime」 | §3.8：`ssa_exec` 在 `rgoc-interp` 内，**值模型只有一个 crate**（`D-M6-7`） |
| 8 | 「定义 safepoint live set、指针/派生指针处理和 lowering→frame map 协议」 | §3.7：值级活跃性 + **`SSA-LIVE v1`** 版本化协议（`D-M6-10`，M6e 只到协议级） |
| 9 | 「完整 M5 功能可以分批降级，暂不支持闭包/异常等必须显式报告」 | §5 白名单 7 条，每条一个码 + 一个 fixture |
| 10 | 「不要求优化 ≥8」 | 门禁 `SSA-5` 只要求**语义保持**，不设 pass 数量下限 |

### 1.3 明确不做（M6 边界）

| 不做 | 为什么 | 归谁 |
|---|---|---|
| 寄存器分配 / 机器码 | 那是 M7 的 `PIPE-19` | M7 |
| 逃逸分析 | 依赖 M9 的对象模型；M6 只**声明**派生指针策略 | M9 |
| 全局 CSE / LICM / 循环优化 | roadmap 明确「复杂优化延期」 | M7+ |
| 多包 SSA | M5 的 `Program` 是单包（`F4`），多包属链接面 | M6 之外 |
| 并发相关（`chan`/`go`/`select`） | 解释器都没有，SSA 更没有 | M8 |
| `asmcheck` 作为语义证明 | roadmap：`asmcheck` 不算 SSA 语义证明 | — |

### 1.4 退出条件（门禁 `SSA-1..9`）

| 门禁 | 名字 | 判定 |
|---|---|---|
| `SSA-1` | IR 形状与 dump | 指令/终止符族穷尽；`C10` 冻结；dump 逐字节确定 |
| `SSA-2` | verifier 有牙 | 九条不变式各一**手写 IR** 反例，两向（反例全拒 / 合法图全过） |
| `SSA-3` | HIR→SSA 构建 | 控制流/多值/调用/复合字面量/slice/指针各一组 fixture；取址即建根 |
| `SSA-4` | 三方差分 | 冻结切片上 `HIR 解释` = `SSA 求值` = `go run` |
| `SSA-5` | pass 语义保持 | 每 pass 独立开关；全开与逐个关两路一致；每 pass 后 verifier 绿 |
| `SSA-6` | memory 模型 | 同根保序 / 异根不串 / 堆根全序 / 全局根跨函数可见 |
| `SSA-7` | 不支持面 | 每条 `SSA-UNSUPPORTED-*` 一个 fixture（退出码 1 + 稳定码 + 不 abort） |
| `SSA-8` | live set 与协议 | 值级活跃性正确（含回边）；`SSA-LIVE v1` 自描述；消费方桩测试能读 |
| `SSA-9` | 回归与收口 | 四条 cargo + 五条自检 + M5 切片零回归 + `C4` 升正式 + `GAP-01` 补 + 报告/manifest |

---

## 2. 已确认决策记录

### 2.1 决策一览

| # | 决策 | 一句话 |
|---|---|---|
| `D-M6-1` | SSA 输入 = **HIR**，不从 AST 重建 | 复用 M5 已显式化的求值顺序与 slot 语义 |
| `D-M6-2` | 仍是**单包** | 与 M5 的 `Program` 同构，多包属链接面 |
| `D-M6-3` | phi = **块参数**（`Block.params` + 前驱给参） | C4 预留的位置；verifier 可结构性检查 |
| `D-M6-4` | 多返回值 = **多结果指令**，tuple 不进 IR | 值保持单值化；优化与求值器不必懂 tuple |
| `D-M6-5` | memory **按根分区**（globals / 取址 slot / 堆） | 根是构造成分，不需别名推理就能验证 |
| `D-M6-6` | 指针是**普通值**，根标注在 `Load/Store` | 指针类型不承诺「无别名」 |
| `D-M6-7` | `rgoc-ssa`（纯数据）+ `rgoc-interp::ssa_exec`（执行） | 值模型只有一个 crate，第三套 runtime 在依赖图上不可能 |
| `D-M6-8` | passes = 常量折叠 / DCE / 复制传播 + **块内纯值 CSE** | roadmap 的三样 + 「证明必要的 CSE」 |
| `D-M6-9` | 调用的 effect 显式声明（写根集 / may_panic / may_call_back） | 没有 effect 的调用会让 memory 优化失效 |
| `D-M6-10` | M6e = 值级活跃性 + 协议，**不做逃逸分析** | 逃逸分析要 M9 的对象模型 |
| `D-M6-11` | verifier 反例必须是**手写 IR** | 反例若由构建器产出，构建器的 bug 会把非法图挡在门外 |
| `D-M6-12` | 切片 = **实测分类**（`N` 能建图 + `M` 拒绝 = 69，两向断言） | 不先定支持面再挑文件；数字不预测、只如实报 |
| `D-M6-13` | 新建 **`C10-ssa-dump.md`**，不复用 `C7` | `C7` 是类型 dump；SSA dump 的排序键与标签都不同 |
| `D-M6-14` | 用户可见码走**crate-local 枚举**（沿 `rgoc-hir::RejectKind` 先例），verifier 错误本地结构化；两者都**不进** `DiagCode` | `C8` 的 26 类是**检查器**词汇且 `other` 的值域被 `T-CK-16` 钉死为 3 个实现限制码，塞进去等于让冻结契约漂 |
| `D-M6-15` | 不引第三方依赖（支配/图算法手写） | 沿 `AGENTS.md` §4.3；`std` 足够 |
| `D-M6-16` | 每个 Task 收口即 commit；判据先于实现 | 沿 M4/M5 的教训（M5 曾 46 个任务不提交） |

### 2.2 五字段（只对**非显然**的七条展开）

#### `D-M6-1` SSA 输入 = HIR

| 字段 | 内容 |
|---|---|
| **背景** | 有两条路：从 HIR 建图，或从 AST 直接建 SSA（像官方 `gc/ssa` 从 noder 的 IR 建） |
| **选项** | ① 从 HIR；② 从 AST 重建一套 lowering |
| **决定** | ① |
| **理由** | M5 的 HIR 已经把**求值顺序**显式化了（`Assign.rhs` 全求值再写、`defer` 实参已是临时槽读、复合字面量 keyed 元素带下标）。从 AST 重建等于把 M5 的 2891 行 lowering 再写一遍，且两套 lowering 的求值顺序一旦不一致，`SSA-4` 的差分就会变成「在比两个不同的语言」 |
| **失效条件** | 若 HIR 缺少 SSA 必需的信息（例如某个副作用没有节点），**先补 HIR**，不许在 SSA 侧猜 |

#### `D-M6-3` phi 用块参数

| 字段 | 内容 |
|---|---|
| **背景** | 汇合点的「同一逻辑变量两个定义」需要 phi；两种形态：块参数（前驱边传参）或 phi 指令（块内并列 `phi [v1, B1], [v2, B2]`） |
| **选项** | ① 块参数；② phi 指令；③ 不在 IR 里表达（靠 slot） |
| **决定** | ① |
| **理由** | ① C4 的 `Block::params` 就是留的这个位（「M6 接手时能看到 phi 该挂在哪里，而不是重新发明」）；② verifier 的判据变成「每条前驱给的实参个数 == 目标块 params 个数」——**结构性可查**，不必在指令流里反查 phi 的配对是否与 CFG 一致；③ 靠 slot 就等于没有 SSA |
| **失效条件** | 块参数要求**并行赋值**语义（见 §3.3.2）。如果实现里做成顺序赋值，`a, b = b, a` 会静默错 —— 这是本决策唯一的真陷阱，必须有专门 fixture |

#### `D-M6-4` 多结果指令

| 字段 | 内容 |
|---|---|
| **背景** | Go 的 `f()` 产出 n 个值，`a, b := f()` / `return f()` / `g(f())` 三处都要 n |
| **选项** | ① 多结果指令 `Call { dsts: Vec<ValueId> }`；② bundle/tuple 类型；③ 写回 n 个 slot |
| **决定** | ① |
| **理由** | 值仍是**单值化**的 ⇒ passes 与寄存器分配（M7）不需要懂 tuple；`return f()` / `g(f())` 在构建期做**结果直连**（把 f 的 `dsts` 直接当 g 的 args），不需要「先打包再拆包」的指令对；③ 会让「调用」变成隐式多处定义，与 `V1` 直接冲突 |
| **失效条件** | 若某天需要把多返回值当**一等值**传递（Go 不支持，`C4` §2.2 也这么说），才需要 bundle；现在不需要 |

#### `D-M6-5` memory 按根分区

| 字段 | 内容 |
|---|---|
| **背景** | 可变状态只有三种来源：包级变量、被取过地址的 slot、堆（map/slice/闭包捕获经指针写） |
| **选项** | ① 单 mem token 全序；② 按根分区；③ 不建 memory |
| **决定** | ② |
| **理由** | 根是**构造成分**（谁取过址就是一根），不需要别名推理就能过 verifier；同根保序给出「写后读必须读到新值」这条可查的判据（`SSA-6`）；异根不互相约束，为 M6e 的 live set 与 M7 的重排留出空间。① 可验证但所有访存全序，等于放弃 M6d 的一半价值；③ 让「先写后读」只能靠约定 |
| **失效条件** | 若 M6e 要按**对象**分区（field-level），需要真正的别名分析 —— 那时才升版本，不在本期偷做 |

#### `D-M6-7` crate 落点

| 字段 | 内容 |
|---|---|
| **背景** | 求值器放哪：`rgoc-ssa` 内 / 新 crate / `rgoc-interp` 内 |
| **选项** | ① `rgoc-ssa` 纯数据 + `rgoc-interp::ssa_exec`；② 新 `rgoc-ssa-eval`；③ 求值器进 `rgoc-ssa` |
| **决定** | ① |
| **理由** | roadmap 要求「不发展第三套完整 runtime」。**把值模型锁在 `rgoc-interp` 一个 crate 里**，这条要求就从「自觉」变成「依赖图上不可能」。② 多一个 crate，而 `ssa_exec` 必然要调 `ops`，两 crate 会互相缠；③ 让 IR 库背上「必须能跑」的包袱，verifier/passes 的测试被迫连执行环境一起备齐 |
| **失效条件** | 若 `ssa_exec` 需要修改 `Value` 的形状（例如引入未经 C9 修订的变体），**先改 C9**，不许在求值器里造平行值模型 |

#### `D-M6-10` M6e 只到协议级

| 字段 | 内容 |
|---|---|
| **背景** | roadmap §M6e 要求「指针活跃集、safepoint、派生指针策略 + 输出 codegen 可消费的版本化协议」 |
| **选项** | ① 只做协议 + 值级活跃性；② 连派生指针与逃逸分析一起做 |
| **决定** | ① |
| **理由** | 派生指针（interior pointer）的活跃性依赖「对象边界」概念，而对象模型在 M9 才定。M6 能做且**必须**做的是：值级活跃区间（含回边）、safepoint 位置的确定性输出、派生指针**策略声明**（保守：任一相关根活跃即整根活跃）。这样 M7 拿到的是「可消费的协议 + 保守但正确的策略」，而不是「等 M9 的空白」 |
| **失效条件** | 若 M7 的寄存器分配证明「保守策略不可用」（例如帧过大），在 M7 前瞻里升级为对象级活跃性 —— 但那时要有 M9 的对象模型支撑 |

#### `D-M6-12` 切片实测分类

| 字段 | 内容 |
|---|---|
| **背景** | M5 入选 69 个语料文件（`F7`）。M6 要声明「SSA 支持切片」，有两种定法 |
| **选项** | ① 先定支持面，再挑文件；② 先实现，再按拒绝面实测分类 |
| **决定** | ② |
| **理由** | ① 会诱导「按支持面挑好看的文件」，与 M5 §6.3 的纪律（数量不足如实报、不凑数）冲突；② 得到三个可验的数（能建图 `N`、显式拒绝 `M`、`N+M=69`），且拒绝的**每一条都有码**。代价是 `N` 不可预测 —— 这正是诚实的价格 |
| **失效条件** | 若 `N` 小到无法支撑 `SSA-4`（例如 < 30），如实报告并把「拒绝面里哪些是可做的」列入 `open_gaps`，**不许**把拒绝项算成通过 |

### 2.3 事实基线（`F1..F14`，均为源码指认或实测）

| # | 事实 | 出处 |
|---|---|---|
| `F1` | HIR 共 54 个变体：`ExprKind` 21 / `Stmt` 14 / `Place` 5 / `Callee` 5 / `BuiltinKind` 17 | `rgoc-hir/src/types.rs`（2026-10-23 计数） |
| `F2` | 闭包在 lowering 期已**提升为独立 `Func`**，捕获记在 `Func.captured: Vec<Capture{outer_slot, slot}>` | `lower.rs` 的 `lower_funclit` |
| `F3` | 每个 `Func` 带 `nslots` + `slot_types: Vec<TypeId>` | `types.rs` 的 `Func` |
| `F4` | `Program` 是**单包**：`funcs` / `globals` / `init_order` / `inits` / `main` | `types.rs` 的 `Program` |
| `F5` | 值模型 13 个变体（`C9` 已冻结为候选） | `rgoc-interp/src/value.rs` |
| `F6` | `ops` 已提供 `apply_binary` / `apply_unary` / `normalize`（按宽度掩码） | `rgoc-interp/src/ops.rs`（265 行） |
| `F7` | M5 入选 69 / 排除 11 / 候选 80，对撞 69/69 绿 | `M5-manifest.json` 的 `corpus_lists`（实测） |
| `F8` | workspace 现状：150 suite / 1608 passed / 0 failed / 1 ignored | 2026-10-23 实测 |
| `F9` | `C4` 仍是 spike 级，五项需求登记（memory/tuple/call/phi/verifier） | `docs/contracts/C4-ssa.md` §2 |
| `F10` | `run-oracle` 已有 `--run/--list/--three-way` 三形态，期望值由 `go run` 生成 | `tools/run-oracle/run-collide.sh` |
| `F11` | M0 的 SSA spike 441 行，**直线程序**：1 块 / 0 succ / 0 param | `rgoc-spikes/src/ssa.rs` |
| `F12` | dump 契约先例（`C6`/`C7`）：`#` 头行 + 数字编号顺序 + 逐行语法 | `C6-ast-dump.md` / `C7-type-dump.md` |
| `F13` | 资源上限先例：帧 1000 / 求值栈 10000 / 步数 1e8 | `C9` §6、`M5-manifest.json` |
| `F14` | 诊断码现 **103** 个；`C8` 的 26 类已冻结，且 `other` 的值域被 `T-CK-16` 钉死为 3 个实现限制码 ⇒ **M6 不往里加码** | `rgoc-lex/src/diag.rs` + `rgoc-check/tests/category.rs`（2026-10-23 核对） |

---

## 3. 架构与数据模型

### 3.1 流水线与 crate 变化

```text
AST ──M5──► HIR ──M6(新)──► SSA ──► verifier ──► passes ──► ssa_exec（诊断求值）
             │                                                    │
             └──────────► rgoc-interp（HIR 解释器，语义基准）◄────┘
                              三方差分：HIR / SSA / 官方 go run
```

| crate | 变化 |
|---|---|
| `rgoc-ssa`（**新建**） | `ir`（类型与指令）/ `builder`（HIR→SSA）/ `cfg`（前驱后继、可达）/ `dom`（支配树）/ `verify`（九条不变式）/ `opt`（三 pass + CSE）/ `dump`（`C10`）/ `live`（活跃性 + 协议） |
| `rgoc-interp` | 新增 `ssa_exec` 模块；`lib.rs` 导出 `ssa_exec::run_ssa` |
| `rgoc-driver` | 新增 `ssa [--dump\|--verify]`；`run --via={hir,ssa}`（默认 `hir`，保持 M5 行为） |
| `rgoc-hir` | **不改**（若发现缺信息，先补 HIR 并登记，`D-M6-1` 的失效条件） |

### 3.2 IR 形状

```rust
pub struct Module { pub funcs: Vec<SsaFunc>, pub globals: Vec<GlobalId>, pub main: Option<FuncId> }

pub struct SsaFunc {
    pub src: FuncId,                       // 回指 HIR（诊断定位用）
    pub params: Vec<ValueId>,
    pub blocks: Vec<Block>,
    pub entry: BlockId,
    pub roots: Vec<Root>,                  // 按 RootId 索引
    pub slot_root: BTreeMap<SlotId, RootId>,   // 只有**取过址**的 slot 才在这里
    pub value_types: Vec<TypeId>,          // ValueId → 静态类型（长度 == 值个数）
}

pub enum Root { Global(GlobalId), Slot(SlotId), Heap }

pub struct Block {
    pub params: Vec<ValueId>,              // 块参数 = phi（D-M6-3）
    pub instrs: Vec<Instr>,
    pub term: Term,
}

pub enum Instr {
    // ── 纯值 ──
    Const { dst: ValueId, c: ConstVal, typ: TypeId },
    Copy  { dst: ValueId, src: ValueId },
    Bin   { dst: ValueId, op: BinOp, lhs: ValueId, rhs: ValueId, typ: TypeId },
    Un    { dst: ValueId, op: UnOp, x: ValueId, typ: TypeId },
    Convert { dst: ValueId, x: ValueId, typ: TypeId },
    AddrOf  { dst: ValueId, root: RootId },                       // &x / &g：根的起点
    Field   { dst: ValueId, base: ValueId, index: usize },         // 字段偏移
    Index   { dst: ValueId, base: ValueId, i: ValueId },           // 数组 / slice 元素
    Slice   { dst: ValueId, base: ValueId, lo: Option<ValueId>, hi: Option<ValueId>, max: Option<ValueId> },
    Composite { dst: ValueId, typ: TypeId, items: Vec<ValueId> },
    Make    { dst: ValueId, typ: TypeId, len: Option<ValueId>, cap: Option<ValueId> },
    // ── 访存（mem 是值，串行化靠它）──
    Load  { dst: ValueId, mem: ValueId, root: RootId, addr: ValueId, typ: TypeId },
    Store { dst: ValueId /* 新 mem */, mem: ValueId, root: RootId, addr: ValueId, val: ValueId },
    // ── 调用 ──
    Call  { dsts: Vec<ValueId>, callee: CalleeRef, args: Vec<ValueId>, effect: Effect },
}

pub enum CalleeRef { Func(FuncId), Method { recv: ValueId, func: FuncId }, Builtin(BuiltinKind), Indirect(ValueId) }

/// 边上传参：**块参数的实参挂在边上**（`V7` 要查的就是这个 `Vec` 的长度）。
pub enum Term {
    Goto { to: BlockId, args: Vec<ValueId> },
    If { cond: ValueId, then_: (BlockId, Vec<ValueId>), else_: (BlockId, Vec<ValueId>) },
    Return(Vec<ValueId>),
    Panic(ValueId),
    /// 块**可达**但流到此为止（如 `panic` 之后的块尾）；**无操作数**，也不是「死块」。
    Unreachable,
}
```

> **三条被这个形状固定下来的事**：
> ① 结果不是 `SsaFunc` 的字段，而是 `Term::Return(Vec<ValueId>)` 的操作数 —— 返回值就是值，不需要额外槽；
> ② 访存的 `root` 是**显式字段**（不是从 `addr` 反推），否则 verifier 要重跑一遍别名分析才能查 `V6`；
> ③ 每条前驱给的实参个数是 `Term` 上的 `Vec` 长度，`V7` 才有东西可查（而不是去数「谁看起来像 phi」）。
>
> **包级初始化不在 SSA 里重排**：`Program.init_order` / `inits`（`F4`）保持 HIR 决定的顺序，
> SSA 只处理「函数体」；初始化序列由 driver 依次调用（与 M5 相同）。

### 3.3 构建算法（`D-M6-1/3/4` 的落地）

#### 3.3.1 slot 提升与取址分析

M5 的 HIR 是 slot 化的（`F3`）。构建分三步：

1. **取址扫描**：遍历函数体，收集所有出现在 `ExprKind::AddrOf` 下的 `Place::Slot`/`Place::Global`。**取过址的 slot → 一根**（`Root::Slot`），其读写走 `Load/Store`；**没取过址的 slot → 直接提升成值**（`mem2reg`）。
2. **定义-使用**：对可提升的 slot，按「赋值点定义新值 + 使用点引用当前值」展开；汇合点由块参数合并（`D-M6-3`）。
3. **未初始化读**：HIR 保证 `slot_types` 齐（`F3`），零值由 `Const` 或 `Composite` 起始值提供；**不许**把「未定义」当 `Undef` 静默放过（Go 的零值语义）。

> ⚠️ 漏判取址是本设计最隐蔽的错误源：它让可变状态被折叠成 SSA 值，`hello`/`for`/`slice` 都绿，只有 `p := &a; *p = 2` 那一下才错。判据：`V9`（每个 `AddrOf` 的根必须在 `slot_root` 里）。

#### 3.3.2 控制流与块参数（并行拷贝）

`if`/`for`（三种形态）/`switch`/`goto`/labeled `break`/`continue` 都转成 CFG；汇合点的活跃 slot 变成块参数。**关键语义**：块参数实参是**并行赋值**——`a, b = b, a` 在边上交接时，两个实参都要用**赋值前**的值。实现上由求值器一次性算出全部实参再写入目标块的 `params`，构建器不得逐条搬。

#### 3.3.3 多返回值与结果直连

- `a, b := f()` ⇒ `Call { dsts: [v1, v2] }` + 两次 `Copy`/直接使用。
- `return f()` ⇒ `Call { dsts }` 后 `Return(dsts)`，不产生中间值。
- `g(f())` ⇒ `Call f` 的 `dsts` 直接作为 `Call g` 的 `args`（`spread` 在 HIR 已有，`F1`）。

#### 3.3.4 调用与 effect

```rust
/// `writes` 只可能含 `Root::Global` / `Root::Heap` —— slot 根是帧内的，跨函数不可见。
pub struct Effect { pub writes: Vec<RootId>, pub writes_all: bool, pub may_panic: bool, pub may_call_back: bool }
```

- 写根集：构建期按被调函数的「写根摘要」计算（递归到不动点，遇到递归/未知 ⇒ 全集 `writes_all = true`）。
- `may_panic`：函数体含 `panic`/除零/越界/断言 则真；调用的 `may_panic` 影响 `panic` 后的可达性（`V8`）。

### 3.4 verifier 九条不变式（`SSA-2` 的分母）

| # | 不变式 | 判据 |
|---|---|---|
| `V1` | 每值恰好定义一次 | 遍历所有 `dst` 计数 == 1 |
| `V2` | 定义支配使用 | 用支配树判 `def.dom.use`；块参数例外（由前驱边提供） |
| `V3` | 类型一致 | `value_types` 与指令语义一致：判等用 `rgoc_types::identical`（已 `pub`）；`Bin` 的操作数规则用**一张表**（`BinOp → 允许的操作数类型`），该表从 `rgoc-check` 提取成 `pub` 或在 `rgoc-types` 重建 —— 两处只能有一份，否则 verifier 会「自己发明一套类型规则」 |
| `V4` | 前驱后继互逆、entry 无前驱 | 双向对账 |
| `V5` | 调用结果数 == 签名结果数 | `dsts.len() == sig.results.len()` |
| `V6` | mem 值被支配 + 同根同块不乱序 | ① 每个 `Load/Store` 的 `mem` 操作数**支配**该指令（`V2` 的推论，但单独查，因为 mem 是隐式链）；② 同块内同根的两个访存，后者的 `mem` 必须等于前者（`Store` 的 `dst`），不得跳过 |
| `V7` | 块参数个数 == 每条前驱给参个数 | 边上的 `args.len() == target.params.len()` |
| `V8` | 可达性 | 非 entry 块**必须至少有一条前驱**；**builder 不产出不可达块**（真死块在构建期丢弃，不靠标记） |
| `V9` | 取址即建根 | 每个 `AddrOf` 的 `root` 在 `slot_root` 里 |

**反例必须手写 IR**（`D-M6-11`）：每次拒绝要带**不变式名 + 块号 + 指令下标**，这样「拒了但不知道拒了哪条」不会发生。

### 3.5 memory 模型（`D-M6-5/6/9`）

- **根**：`Root::Global(GlobalId)` / `Root::Slot(SlotId)`（仅取址）/ `Root::Heap`（一个，代表全部堆对象）。
- **序**：同根上 `Store` 产出新 `mem`，后续同根 `Load/Store` 必须用它 ⇒ 「写后读读到新值」是**结构性**的。
- **异根**：互不约束（允许 M7 重排）。
- **调用**：`Call.effect.writes` 声明写哪些根；收缩规则：`writes_all` ⇒ 所有根。
- **不做别名推理**：两个 `AddrOf` 指向同一 slot 时，它们**同一个根**（slot 身份），所以不需要推理。

### 3.6 passes（`D-M6-8`）

| pass | 内容 | 边界 |
|---|---|---|
| `fold` | 常量折叠（复用 `rgoc-const` 的任意精度）、`Bin`/`Un`/`Convert` 折叠、`If` 常量条件 ⇒ 边重定向 | 折叠溢出必须走 `C9` 的可表示性规则，不许静默截断 |
| `dce` | 删除无使用且**无副作用**的值；`Store`/`Call` 不删（除非 effect 全空） | 死块（不可达）单独一步，标 `dead` |
| `copy` | `Copy` 链合并；块参数与实参的 `Copy` 消解 | 并行拷贝语义不得被破坏（`a,b=b,a` 的 fixture 必过） |
| `cse` | **块内纯值**公共子表达式消除 | 不做跨块（那要支配树 + 可用表达式，属 M7） |

**开关**：`--opt=all|none|fold,dce,copy,cse`；`SSA-5` 要求「全开 / 逐个关」两路结果一致，且每次 pass 后跑 verifier。

### 3.7 活跃性与 `SSA-LIVE v1`（`D-M6-10`）

- **值级**：按块算 `live_in/live_out`（含回边，迭代到不动点），再按值算区间 `[def, last_use]`。
- **指针**：保守策略 —— 一个指针值活跃 ⇒ 其根活跃（不做对象级）。
- **协议**（`SSA-LIVE v1`）：逐函数一段确定性文本：

```text
# SSA-LIVE v1
# func <name> blocks=<n> values=<n>
# safepoint <block> <instr_idx> live=<v1,v2,...> ptrs=<v3,...>
# interval v<k> def=<block:idx> last=<block:idx>
```

消费方桩测试（`SSA-8`）：解析这段文本、断言字段自洽（`live` 里的值都有区间、`ptrs` 都是指针类型），证明「codegen 可消费」。

### 3.8 求值器（`ssa_exec`）

- 位置：`rgoc-interp/src/ssa_exec.rs`，复用 `Value`/`Cell`/`ops`/`print`（`D-M6-7`）。
- 结构：**显式帧栈 + 按块执行**（不进 Rust 递归），`Vec<Value>` 存值，`Vec<Frame>` 存块现场；栈上 `mem` 值按根索引。
- 资源：步数上限 1e8（`F13` 口径），超限退出码 2。
- 输出：与 HIR 解释器**同一套合并流**（`print`/`println` 写同一 buffer，`C9` §4）。

---

## 4. 判据与装置

### 4.1 三方差分（`SSA-4`）

```text
rgoc run --via=hir  ==  rgoc run --via=ssa  ==  go run（合并流逐字节 + 退出码）
唯一容差：\r\n → \n
```

装置：`run-collide.sh --ssa`（在 M5 的 `--run` 上加一列）。失败时保存：源文件、两侧 dump、两侧输出、期望。

### 4.2 切片计算（`D-M6-12`）

从 `run-selected.txt` 的 69 行出发，逐个 `rgoc ssa --verify`：能建图且 verifier 过 ⇒ 计入 `N`；否则落 `M` 并记**拒绝码**。
断言：`N + M == 69`（两向），且 `M` 里每一行的码都在 §5 白名单中。**反向守卫**：把任一 `M` 里的文件硬塞进 `N` 清单 ⇒ 必红。

### 4.3 `C10` dump 格式（`D-M6-13`）

沿 `C6`/`C7` 的先例：`#` 头行（stage/version/func/blocks/values）、块按 `BlockId` 升序、指令逐行、值按 `ValueId` 升序、`#` 行之外不接受注释。**逐字节确定**（`SSA-1`），dump 不得含地址或哈希序。

### 4.4 资源与预算

| 项 | 值 |
|---|---|
| 步数上限 | 1e8（SSA 求值器与 HIR 解释器同口径） |
| 帧/块深 | 沿用 1000 / 10000（`F13`） |
| 单语料文件墙钟 | 10 s（装置侧约束） |
| 构建预算 | 单个函数 1e5 条指令上限，超限显式拒绝（防病态输入） |

---

## 5. 不支持面（白名单，7 条）

| ID | 拒绝什么 | 判据（构建期） | 码 | 正向证据 |
|---|---|---|---|---|
| `U-M6-01` | 闭包（捕获跨帧写） | `!func.captured.is_empty()`（`F2`） | `SSA-UNSUPPORTED-CLOSURE` | `T-EX-26` 的 counter；语料 `closure*.go` |
| `U-M6-02` | `defer` / `recover` | `Stmt::Defer` / `BuiltinKind::Recover` 出现 | `SSA-UNSUPPORTED-DEFER` | `T-EX-34..37` |
| `U-M6-03` | 接口动态派发 / type switch | `Callee::Iface` / `Stmt::TypeSwitch` | `SSA-UNSUPPORTED-IFACE-DYN` | `T-EX-29/30/31` |
| `U-M6-04` | `map` 读写 | `Type::Map` 上的 `Load/Store`/内建 | `SSA-UNSUPPORTED-MAP` | `T-EX-14` |
| `U-M6-05` | `chan` / `go` / `select` | `Type::Chan` / `Stmt` 内 `go`/`select` | `SSA-UNSUPPORTED-CHAN` | `U-M5-01` |
| `U-M6-06` | `range` over func | HIR 层已拒（`U-M5-06`） | `SSA-UNSUPPORTED-RANGE-FUNC` | `range4.go` |
| `U-M6-07` | 泛型实例化后的形态 | 与 `G-M5-03` 同根（检查器没放进来） | `SSA-UNSUPPORTED-GENERIC` | `T-EX-32/33` |

**纪律**：白名单项**不得**记为 pass；每条都有「一个 fixture + 一个码 + 一个退出码」三件套。

---

## 6. 错误处理

| 类 | 出口 | 形态 |
|---|---|---|
| 用户面（不支持 / 构建预算） | `stderr` + 退出码 1 | `rgoc-ssa/src/diag.rs` 的 `SsaCode`（7 个 `SSA-UNSUPPORTED-*`），**crate-local**，沿 `rgoc-hir::RejectKind` 的先例；`DiagCode` 保持 103 不动（`D-M6-14`） |
| 开发者面（verifier 报错） | `Err(VerifyError)` | 不进 `DiagCode`：`{ invariant: "V1".."V9", block: BlockId, idx: usize, detail }` |
| 内部不可达 | `todo!()` + 注释指向门禁 | 不许用 `None` 冒充未实现 |

> **为什么 SSA 的码不进 `DiagCode`**（这条是写计划时核对出来的）：
> `DiagCode` 的每个码都必须被 `rgoc-check/tests/category.rs` 投影到 **`C8` 冻结的 26 类**之一，
> 而其中 `other` 的值域被 `T-CK-16` 钉死为「恰好 3 个实现限制码」（`ExceedsFileLimit` /
> `SynNestingTooDeep` / `TypNestingTooDeep`）。把 7 个 SSA 拒绝码硬塞进去，只有两条路：
> 让 `other` 的值域漂（破 `T-CK-16` 与 `C8`），或给它们编一个假类别（用类别答错）。
> **两条都不行。** 所以走 `rgoc-hir::RejectKind` 的同一条路：crate 自己一个 `SsaCode`，
> 码文本进 `SSA-*` 命名空间，`DiagCode` 保持 103。

---

## 7. 覆盖清单（roadmap §M6 ↔ 功能 ID）

| 功能 ID | 内容 | 归属 | 本文档落点 |
|---|---|---|---|
| `PIPE-17` | SSA 构建（块/值/Phi） | ✅ M6 | §3.2 / §3.3 |
| `PIPE-18` | SSA 优化 passes | ◐ M6（三 pass + 块内 CSE） | §3.6 |
| `PIPE-21` | 栈帧布局与活跃性 | ◐ M6（**只做活跃性与协议**，帧布局归 M7） | §3.7 |
| `PIPE-26` | 阶段调试 dump（`-d=ssa`） | ◐ M6（`C10` dump + `--verify`，不出 HTML） | §4.3 |
| `RT-MEM-06`（堆位图） | 依赖对象模型 | ❌ 不涉及 | §1.3 |

---

## 8. 风险登记（前三条是最可能的坑）

| # | 风险 | 为什么危险 | 兜底判据 |
|---|---|---|---|
| 1 | **块参数做成顺序赋值** | `a, b = b, a` 静默错，90% fixture 仍绿 | §3.3.2 的并行拷贝 fixture（必红式） |
| 2 | **取址分析漏判** | 可变状态被折叠成值 ⇒ 语义错但几乎全绿（与 M5 的 `Rc` 共享同类） | `V9` + `p := &a; *p = 2` 的 fixture |
| 3 | **支配树写错** | verifier 会**误拒合法图**，看着像构建器的错 | `V2` 的正例集（合法图必须过）+ 反例集，两向 |
| 4 | `C10` dump 用了哈希序 | 逐字节对撞随机失败，极难定位 | `SSA-1` 的三次一致性 |
| 5 | effect 摘要算错（漏写根） | `dce`/`cse` 会删掉有副作用的调用 | `SSA-6` 的跨函数写根 fixture |

---

## 9. 任务分解原则（`M6-plan.md` 的输入）

六个 Phase，估 **48–56 个 Task**：

| Phase | 内容 | 估 |
|---|---|---|
| 0 | 装置与契约：`C10` 骨架、手写 IR 小工具、dump 骨架、语料分类器（69 → N/M） | 6–8 |
| 1 | IR + verifier：九条不变式逐条「先写反例再实现」 | 10–12 |
| 2 | HIR→SSA：取址分析、slot 提升、控制流与块参数、多值直连、调用与 effect、复合字面量 | 14–16 |
| 3 | `ssa_exec` + 三方差分 + `--via=ssa` | 8–10 |
| 4 | 三 pass + 块内 CSE（每个 pass：实现 → A/B → verifier 绿） | 6–8 |
| 5 | 值级活跃性 + `SSA-LIVE v1` + 消费方桩 | 4–5 |
| 6 | 变异 8 条、回归、报告、manifest、文档回写 | 6–7 |

**铁律**：判据先行（先写会红的测试）；每 Task 收口即 commit；非法 IR 反例不得由构建器生成；pass 的测试必须同时有开关两路。

---

## 10. 契约与产物影响

| 产物 | 动作 |
|---|---|
| `docs/contracts/C4-ssa.md` | **升正式**：五项需求登记逐条给答案（本文档 §3.2–§3.5） |
| `docs/contracts/C10-ssa-dump.md` | **新建**：确定性 dump 格式（`#` 头 + 编号顺序 + 逐行语法） |
| `docs/contracts/C9-values-and-output.md` | **补一节**：SSA 求值器共用同一 `Value`/合并流（`D-M6-7` 的证据） |
| `docs/milestones/GAP-01.md` | 补 `PIPE-17/18/21/26` 的 M6 落点 |
| `docs/03-roadmap.md` / `AGENTS.md` | M6 状态节（**只在收口时**改） |
| `M6-{tests,plan}.md` / `M6-manifest.json` / `M6-report.md` | 三件套 + 事实 + 交付 B |

---

## 11. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-23 | **初稿确认**：`D-M6-1..16`、`F1..F14`、`SSA-1..9`、白名单 7 条、风险 5 条 | 与用户逐段确认（§1 架构 / §2 判据装置 / §3 边界与分解）；`C4` §2 的五项需求登记 |
