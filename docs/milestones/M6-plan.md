# M6 实施计划：SSA、verifier 与精简优化

> **状态**：**待执行**（Task 1–53）　|　**判据**：[`M6-tests.md`](./M6-tests.md)（已冻结，64 个 `T-SSA-*`）　|　**设计**：[`M6-design.md`](./M6-design.md)（`D-M6-1..16`）　|　**事实**：[`M6-manifest.json`](./M6-manifest.json)（Task 50 填实）
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> 执行期进度、偏离与 Ruling 记在 `.superpowers/sdd/M6-plan/progress.md`（不入库）。

**Goal:** 把 M5 的可执行 HIR 变成 **SSA**，并让 SSA 这条路上有牙（verifier 九条不变式）、有证据（`HIR 解释` = `SSA 求值` = `go run`）、有克制的优化（三 pass + 块内 CSE）、有可交付的活跃性协议（`SSA-LIVE v1`）。

**Architecture:** 新 `rgoc-ssa`（纯数据：IR / builder / cfg / dom / verify / opt / dump / live）+ `rgoc-interp::ssa_exec`（诊断求值器，复用 `Value`/`Cell`/`ops`/`print`，**值模型只有一个 crate**）。`rgoc-driver` 加 `ssa [--dump|--verify]` 与 `run --via={hir,ssa}`（默认仍 `hir`）。验收四腿：手写 IR 反例/正例（甲）、构建（乙）、三方差分（丙）、装置（丁），外加 8 条变异与 5 条活跃性协议测试。

**Tech Stack:** Rust 1.98.1 / edition 2024 / resolver 3；workspace 在 `rgoc/`；全部命令经 `scripts/in-container.sh`（宿主不构建）；不引第三方依赖（支配/图算法手写）。

**Spec:** [`docs/milestones/M6-design.md`](./M6-design.md) + [`docs/milestones/M6-tests.md`](./M6-tests.md)。**执行者必须同时读这两份** —— 本计划只决定「顺序与接口」，判据以 tests 清单为准。

## Global Constraints

- **只在容器内构建/测试**：一切 `cargo`/`go` 经 `scripts/in-container.sh`；cargo 必须 `bash -lc 'cd /work/rgoc && …'`（`R-9-1` 形态）。
- **不引第三方依赖**：支配树、CFG、活跃性一律手写（`D-M6-15`）。
- **`unsafe_code = "forbid"`**：SSA 求值器沿用 M5 的显式帧栈纪律，**不许**用 Rust 递归跑块（`D-M6-1` 的邻居，`D-M5-1` 同款）。
- **`go_source_code/` 只读**：跑完批次后 `corpus-manifest.sha256` 必须校验通过（`T-SSA-45`）。
- **非法 IR 反例必须手写**（`D-M6-11`）：反例不得经 builder 生成。
- **切片必须实测**（`D-M6-12`）：`N` 与 `M` 成对报，`N+M==69` 两向断言，**不许凑数**。
- **判据先行**：每个 Task 先写会红的测试；测试之前写的实现一律删除。
- **产物陈旧陷阱**：变异前后 `cargo clean -p <被变异 crate>`（`scripts/mutation-m3-notes.md` §0）。
- **每个 Task 收口即 commit**（信息里带 Task 号与 `T-SSA-*`）。
- **资源上限**：步数 1e8 / 构建指令 1e5 per 函数 / 单程序墙钟 10 s。

## Review Focus

以下是 `M6-design.md` 隐含、但没有任何单条任务天然覆盖的五类失效模式，最可能的排前面。每条都把测试钉到拥有该代码的任务上：

1. **块参数的实参被顺序赋值**（`a, b = b, a` 静默错）：块参数方案唯一的真陷阱；90% fixture 仍绿，只有交换才露（Task 22/34，`T-SSA-28` + 变异 `T-SSA-52`）。
2. **取址分析漏判导致可变状态被折叠成值**：`p := &a; *p = 2` 之后读 `a` 仍是旧值；`hello`/`for`/`slice` 全绿（Task 18/19/30，`T-SSA-27`）。
3. **支配树写错让 verifier 误拒合法图**：症状像「构建器坏了」，实际是 `idom` 错（Task 9/11，`T-SSA-11` 的正例集是唯一能抓它的东西）。
4. **`Effect` 摘要漏写根**：`dce` 随后删掉有副作用的调用（Task 27/42，`T-SSA-61` 的「删了会错」反例）。
5. **三方差分退化成两方差分**：HIR 与 SSA 一起错时双方同绿（Task 38/39，`T-SSA-32` 必须带 `go run` 第三列）。

---

## 执行总览

| Phase | Task | 内容 |
|---|---|---|
| 0 | 1–6 | 装置与契约：`C10` 骨架、crate 骨架、手写 IR 工具、分类器、`--ssa` 装置 |
| 1 | 7–16 | IR 与 verifier：类型 → CFG → 支配 → 九条不变式 → dump |
| 2 | 17–31 | HIR→SSA：取址分析 → slot 提升 → 控制流/块参数 → 多值 → 调用 → 复合 → 拒绝面 |
| 3 | 32–39 | `ssa_exec` + `--via=ssa` + 三方差分 + 切片冻结 |
| 4 | 40–44 | pass 框架 + fold / dce / copy / cse |
| 5 | 45–48 | 活跃性 + `SSA-LIVE v1` + 消费方桩 |
| 6 | 49–53 | 变异 8 条、manifest、契约定稿、报告、全量回归 |

---

## Phase 0：装置与契约（Task 1–6）

### Task 1: `C10` 契约骨架与 dump 期望形态

**Files:**
- Create: `docs/contracts/C10-ssa-dump.md`
- Modify: `docs/README.md`（契约清单登记 C10）

**Interfaces:**
- Produces: `C10` 的六条格式规则（头行字段 / 块序 / 值序 / 指令行语法 / 编号前缀 / 禁止项），供 Task 16 的 dump 实现逐条对齐

- [ ] 写 `C10`：头三行（`# SSA v1` / `# stage: <hir|ssa>` / `# func <name> blocks=<n> values=<n>`）、块段语法（`block <id> params=[v..] preds=[..] succs=[..]`）、指令行（`<v> = <op> <operands> : <type>`）、终止符行、**禁止项**（地址/时间/哈希序）
- [ ] 在 `docs/README.md` 的契约表加 C10 行
- [ ] 写一条**反向**检查：C10 里每个字段名都要在 §3 的示例里出现（文档自洽）
- [ ] Commit：`docs(m6): C10 SSA dump 契约骨架（Task 1）`

**Verify:** `grep -c '^| ' docs/contracts/C10-ssa-dump.md` ⇒ ≥ 6（六条规则成表）；`grep -n 'C10' docs/README.md` 有命中

---

### Task 2: `rgoc-ssa` crate 骨架 + `ssa` 子命令空壳

**Files:**
- Create: `rgoc/crates/rgoc-ssa/Cargo.toml`、`src/lib.rs`、`src/ir.rs`
- Modify: `rgoc/Cargo.toml`（members）、`rgoc/crates/rgoc-driver/Cargo.toml`（依赖）、`rgoc/crates/rgoc-driver/src/cli.rs`、`src/lib.rs`、`src/main.rs` 之外的新模块 `src/ssa_cmd.rs`

**Interfaces:**
- Produces: `rgoc_ssa::ir::{ValueId, BlockId, RootId, Module, SsaFunc, Block, Instr, Term, Root, Effect, CalleeRef}`；`rgoc_ssa::dump::dump_module(&Module) -> String`；CLI `rgoc-driver ssa [--dump|--verify] <file>...`
- Consumes: 无（骨架）

- [ ] 建 crate，`lib.rs` 声明 `pub mod ir; pub mod dump;`（其余模块后续任务加）
- [ ] `ir.rs` 先放最小 `ValueId(pub u32)` / `BlockId(pub u32)` / `RootId(pub u32)` 与空 `Module`/`SsaFunc`
- [ ] driver 加 `Command::Ssa(SsaCommand)`、`ssa_cmd::run_ssa(&cmd) -> Result<SsaOutcome, String>`，`SsaOutcome { stdout, stderr, exit }`（照 `run_cmd` 的三通道形状）
- [ ] 测试 `T-SSA-38` 的骨架版：`--dump` 只写 stdout、`--verify` 错误只写 stderr
- [ ] Commit：`feat(ssa): crate 骨架与 ssa 子命令（Task 2）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_cli`（新建，1 条）；`… cargo check --workspace`

---

### Task 3: 手写 IR 工具（`testsupport`）

**Files:**
- Create: `rgoc/crates/rgoc-ssa/src/testsupport.rs`（`#[doc(hidden)]`，`cfg(any(test, feature = "testsupport"))`）
- Test: `rgoc/crates/rgoc-ssa/tests/verify_negative.rs`（占位，Task 10 起填）

**Interfaces:**
- Produces: `FnBuilder`（`block()` / `params(&[..])` / `instr(..)` / `term(..)` / `finish()`），**故意不做类型检查**；`fn single_block(dsts: &[ValueId]) -> SsaFunc`
- Consumes: `ir::*`

- [ ] 实现 `FnBuilder`：只负责「按我给的字面内容拼出 `SsaFunc`」，**不检查**任何不变式（否则反例造不出来）
- [ ] 写一条自测：用 `FnBuilder` 造一个**故意非法**的图（同 `ValueId` 两次 `dst`），断言构造成功（证明工具本身不拦）
- [ ] Commit：`test(ssa): 手写 IR 构造工具（Task 3）`

**Verify:** `… cargo test -p rgoc-ssa --lib testsupport_`（≥1 条）；该工具不出现在 `pub use` 的默认面里

---

### Task 4: 语料分类器（`ssa-rejected.txt` 三栏）

**Files:**
- Create: `tools/run-oracle/ssa-classify.sh`（读 `run-selected.txt`，逐文件跑 `rgoc-driver ssa --verify`，产出 `/tmp/ssa-classify.txt`）
- Create: `tools/run-oracle/ssa-rejected.txt`（**本期只建骨架**：头部注释 + 三栏格式，内容 Task 39 填）
- Create: `.superpowers/sdd/M6-plan/classify-notes.txt`（不入库：实测过程）

**Interfaces:**
- Produces: 分类器命令形态 `bash tools/run-oracle/ssa-classify.sh` ⇒ 输出 `<文件>\t<N|M>\t<码>`；供 Task 39 冻结 `N`/`M`
- Consumes: Task 2 的 `ssa` 子命令

- [ ] 脚本骨架：逐行读清单、跳过 `#`、调 `rgoc-driver ssa --verify`、按退出码与 stderr 首个 `SSA-*` 码分类
- [ ] 空跑一次（此时全拒），断言脚本能产出 69 行且每行三栏
- [ ] Commit：`feat(ssa): 语料分类器骨架（Task 4）`

**Verify:** `… bash tools/run-oracle/ssa-classify.sh | wc -l` ⇒ `69`；`awk -F'\t' 'NF!=3' ` 无输出

---

### Task 5: `run-collide.sh --ssa` 骨架

**Files:**
- Modify: `tools/run-oracle/run-collide.sh`（加 `--ssa` 分支）
- Modify: `tools/run-oracle/main.go`（如需要，加 `--via` 直通；否则只改脚本）

**Interfaces:**
- Consumes: `rgoc-driver run --via={hir,ssa}`
- Produces: `run-collide.sh --run --ssa [--list] <清单>` = 三方比对（hir / ssa / go run），退出码 0 = 全绿

- [ ] 加 `--ssa` 分支：先跑 `--via=hir`、再跑 `--via=ssa`、再取 `go run` 期望，三者逐字节比
- [ ] 缺 `--via` 支持时**显式报错**（不许静默退回两方）
- [ ] Commit：`feat(run-oracle): --ssa 三方差分骨架（Task 5）`

**Verify:** `… bash tools/run-oracle/run-collide.sh --run --ssa --list tools/run-oracle/run-selected.txt` 在 Task 37 之前**必须红**（`--via=ssa` 未实现）—— 这正是「骨架不是空转」的证据

---

### Task 6: Phase 0 收口

**Files:**
- Modify: `.superpowers/sdd/M6-plan/progress.md`（新建）

**Steps:**
- [ ] 四条 cargo 全绿；五条既有自检全过（记进台账）
- [ ] 记录 Phase 0 的实际产出与偏离（若有）
- [ ] Commit：`chore(m6): Phase 0 收口（Task 6）`

**Verify:** `… cargo test --workspace --no-fail-fast` ⇒ 0 failed；`python3 scripts/check-m0-consistency.py` exit 0

---

## Phase 1：IR 与 verifier（Task 7–16）

### Task 7: IR 类型落地（指令/终止符/根/effect）

**Files:**
- Modify: `rgoc/crates/rgoc-ssa/src/ir.rs`
- Test: `rgoc/crates/rgoc-ssa/tests/ir_shape.rs`

**Interfaces:**
- Produces: 完整 `Instr`（**14** 变体）/ `Term`（5）/ `Root`（3）/ `Effect` / `CalleeRef`（4）/ `Block` / `SsaFunc`（`value_types`），字段与 `M6-design.md` §3.2 逐字一致
- Consumes: `rgoc_hir::{FuncId, GlobalId, SlotId}`、`rgoc_types::TypeId`、`rgoc_const::Const`

- [ ] 按设计抄形状（字段名不许改：`dsts` / `params` / `slot_root` / `value_types` / `writes_all`）
- [ ] `T-SSA-01`：一张**变体穷尽表**（`Instr` 14 / `Term` 5 / `Root` 3 / `CalleeRef` 4），用 `match` + `_ => unreachable!()` 之外的方式断言 —— 新增变体必须让测试红（例如 `const N_INSTR: usize = 12` 与实际计数比对）
- [ ] Commit：`feat(ssa): IR 类型定稿（Task 7）`

**Verify:** `… cargo test -p rgoc-ssa --test ir_shape`；改一个变体名后测试必红（手工验一次并记台账）

---

### Task 8: CFG 与可达性

**Files:**
- Create: `rgoc/crates/rgoc-ssa/src/cfg.rs`
- Test: `rgoc/crates/rgoc-ssa/tests/cfg.rs`

**Interfaces:**
- Produces: `Cfg::build(&SsaFunc) -> Cfg`、`Cfg::{preds, succs, reachable, is_dead}`；边上实参 `Cfg::edge_args(from, to) -> &[ValueId]`
- Consumes: `ir::*`

- [ ] `Cfg::build`：遍历每个 `Term` 收集边与实参；`preds` 反填
- [ ] 可达性：从 `entry` BFS；不可达标 `dead`
- [ ] 测试：`if/else` 与 `for` 的两张图，断言 `preds/succs` 对称、`reachable` 集合正确
- [ ] Commit：`feat(ssa): CFG 与可达性（Task 8）`

**Verify:** `… cargo test -p rgoc-ssa --test cfg`

---

### Task 9: 支配树（迭代到不动点）

**Files:**
- Create: `rgoc/crates/rgoc-ssa/src/dom.rs`
- Test: `rgoc/crates/rgoc-ssa/tests/dom.rs`

**Interfaces:**
- Produces: `DomTree::build(&SsaFunc, &Cfg) -> DomTree`、`DomTree::dominates(a: BlockId, b: BlockId) -> bool`、`DomTree::idom(b) -> Option<BlockId>`
- Consumes: `cfg::{Cfg}`

- [ ] 实现 Cooper-Harvey-Kennedy 迭代（`idom` 初值 `entry`，迭代到不变）
- [ ] 测试：菱形图（`entry → a,b → join`）断言 `dominates` 全组合；**含交叉边**的图（`a→join`、`b→a`）断言 `idom`
- [ ] 反向测试：把 `idom` 实现改成「RPO 前驱取第一个」，菱形图必须红（记录后复原）
- [ ] Commit：`feat(ssa): 支配树（Task 9）`

**Verify:** `… cargo test -p rgoc-ssa --test dom`；反向测试记录进台账后复原并全绿

---

### Task 10: verifier 骨架 + `V1` / `V4` / `V8`

**Files:**
- Create: `rgoc/crates/rgoc-ssa/src/verify.rs`
- Test: `rgoc/crates/rgoc-ssa/tests/verify_negative.rs`、`verify_positive.rs`

**Interfaces:**
- Produces: `VerifyError { invariant: &'static str, block: BlockId, idx: usize, detail: String }`；`verify(f: &SsaFunc) -> Result<(), VerifyError>`；`INVARIANTS: [&str; 9] = ["V1",..,"V9"]`
- Consumes: `ir`、`cfg`、`dom`

- [ ] 骨架：按 `V1→V9` 顺序跑，第一个失败即返回（顺序稳定，便于断言）
- [ ] `V1`（每值恰好定义一次）：数 `dsts` 与 `params`；`T-SSA-02`
- [ ] `V4`（前驱后继互逆、entry 无前驱）：`T-SSA-05`
- [ ] `V8`（可达性 / `dead`）：`T-SSA-09`
- [ ] 三条反例 + 一条正例
- [ ] Commit：`feat(ssa): verifier 骨架与 V1/V4/V8（Task 10）`

**Verify:** `… cargo test -p rgoc-ssa --test verify_negative --test verify_positive`

---

### Task 11: `V2` 支配（含块参数例外）

**Files:**
- Modify: `src/verify.rs`

**Interfaces:**
- Consumes: `DomTree::dominates`

- [ ] 实现：每个值的使用点必须被其定义点支配；**块参数**视为「在块入口定义」，由前驱边提供（不要求支配前驱）
- [ ] `T-SSA-03` 反例：一支定义、汇合点直接用
- [ ] 正例：带块参数的菱形图必须过（这条是 Task 9 支配树写错时的**唯一报警器**）
- [ ] Commit：`feat(ssa): V2 支配检查（Task 11）`

**Verify:** `… cargo test -p rgoc-ssa --test verify_negative V2`；`--test verify_positive`

---

### Task 12: `V3` 类型一致（`bin` 规则表只许一份）

**Files:**
- Modify: `src/verify.rs`
- Create: `rgoc/crates/rgoc-ssa/src/typing.rs`（`BinOp → 允许的操作数类型` 表）
- Test: `tests/verify_negative.rs`（`V3` 反例）

**Interfaces:**
- Produces: `typing::check_instr(&SsaFunc, &Instr, &TypeTable) -> Result<(), String>`；判等用 `rgoc_types::identical`
- Consumes: `rgoc_types::{TypeTable, identical}`

- [ ] 把 `BinOp` 的规则表放 `typing.rs`，**并在文件头声明「这是唯一一份」**（若与 `rgoc-check` 冲突，改 `rgoc-check` 引用它）
- [ ] `T-SSA-04` 两条反例（`i64 + string`、`Convert` 目标与 `value_types` 不符）
- [ ] Commit：`feat(ssa): V3 类型一致（Task 12）`

**Verify:** `… cargo test -p rgoc-ssa --test verify_negative V3`；`grep -c 'fn allowed_operands' rgoc/crates/rgoc-ssa/src/typing.rs` ⇒ 1

---

### Task 13: `V5` + `V7`（调用结果数、块参数计数）

**Files:**
- Modify: `src/verify.rs`

**Interfaces:**
- Consumes: `Signature`（从 `TypeTable` 取被调函数签名）

- [ ] `V5`：`Call.dsts.len() == sig.results.len()`；内建按 `BuiltinKind` 的固定元数
- [ ] `V7`：每条边的 `args.len() == target.params.len()`
- [ ] 反例 `T-SSA-06`/`T-SSA-08` 各一
- [ ] Commit：`feat(ssa): V5/V7（Task 13）`

**Verify:** `… cargo test -p rgoc-ssa --test verify_negative V5`、`V7`

---

### Task 14: `V6` + `V9`（mem 与根）

**Files:**
- Modify: `src/verify.rs`

- [ ] `V6`（`T-SSA-07`）：① 每个 `Load/Store` 的 `mem` 操作数支配该指令；② 同块同根两次访存，后者的 `mem` 必须等于前者（`Store.dst`），不得跳过
- [ ] `V9`（`T-SSA-10`）：每个 `AddrOf.root` 必须在 `slot_root` 里（**取址即建根**）
- [ ] 三条反例（跳过 store / mem 不支配 / 根缺失）
- [ ] Commit：`feat(ssa): V6/V9（Task 14）`

**Verify:** `… cargo test -p rgoc-ssa --test verify_negative V6`、`V9`

---

### Task 15: 正例集与拒绝可定位

**Files:**
- Test: `tests/verify_positive.rs`（10 张合法图）、`tests/verify_negative.rs`（补 `invariant/block/idx` 断言）

**Interfaces:**
- Produces: `T-SSA-11` 的 10 张图（每指令族一张）、`T-SSA-12` 的三元组断言

- [ ] 10 张合法图：Const/Bin/Convert/Call/Load+Store/Composite/AddrOf+Field/Index/Slice/块参数
- [ ] 每个反例断言 `err.invariant`、`err.block`、`err.idx` 与注入位置一致
- [ ] Commit：`test(ssa): 合法图正例集与拒绝定位（Task 15）`

**Verify:** `… cargo test -p rgoc-ssa --test verify_positive`（10/10）；`--test verify_negative`（9/9 且三元组匹配）

---

### Task 16: dump 实现与确定性

**Files:**
- Modify: `src/dump.rs`
- Test: `tests/dump.rs`

**Interfaces:**
- Produces: `dump_module(&Module) -> String`（严格照 `C10`）
- Consumes: `ir`

- [ ] 按 `C10` 输出头行 / 块段 / 指令行 / 终止符行；块与值按 ID 升序
- [ ] `T-SSA-13`：三次 dump 逐字节相同（用 `BTreeMap` 而非 `HashMap` 存中间结构）
- [ ] `T-SSA-14`：字段齐 + 排序正确（解析回结构化后对账）
- [ ] Commit：`feat(ssa): dump 与确定性（Task 16）`

**Verify:** `… cargo test -p rgoc-ssa --test dump`

---

## Phase 2：HIR→SSA 构建（Task 17–31）

### Task 17: 构建骨架（函数/入口/返回）

**Files:**
- Create: `rgoc/crates/rgoc-ssa/src/build.rs`
- Test: `rgoc/crates/rgoc-driver/tests/ssa_build.rs`

**Interfaces:**
- Produces: `build(program: &rgoc_hir::Program, tt: &TypeTable) -> BuildOutcome { module: Module, rejected: Vec<Reject> }`；`Reject { func: FuncId, code: SsaCode, pos: Option<rgoc_lex::Pos> }`
- Consumes: `rgoc_hir::Program`

- [ ] 每个 `Func` 建一个 `SsaFunc`：`params` 由 `Func.params` 建值、入口块、`Term::Return` 用零值/参数占位（Task 20 起填内容）
- [ ] 测试：空函数 `func main() {}` ⇒ 1 块、0 指令、`Return([])`
- [ ] Commit：`feat(ssa): 构建骨架（Task 17）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build`（≥1 条）

---

### Task 18: 取址分析与 `slot_root`

**Files:**
- Modify: `src/build.rs`
- Test: `ssa_build.rs`（`T-SSA-27`）

**Interfaces:**
- Produces: `fn scan_addressed(body: &rgoc_hir::Block) -> BTreeSet<SlotId>`
- Consumes: `rgoc_hir::{Place, ExprKind::AddrOf}`

- [ ] 遍历函数体收集 `AddrOf(Place::Slot)` 的 slot 集合（含嵌套在 `Field/Index` 下的）
- [ ] 每个被取址 slot 建 `Root::Slot(id)` 并登记 `slot_root`
- [ ] `T-SSA-27`：`p := &a; *p = 2` 的图里 `slot_root` 含 `a`，且读 `a` 走 `Load`
- [ ] Commit：`feat(ssa): 取址分析（Task 18）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build addressed`

---

### Task 19: slot 提升（未取址的纯值化）

**Files:**
- Modify: `src/build.rs`

- [ ] 未被取址的 slot：赋值点定义新值，使用点引用当前值（**维护「当前值」映射**）
- [ ] `T-SSA-26`：无取址函数里**没有** `Load/Store`，也没有 `Root::Slot`
- [ ] 反向守卫：把同一函数加一行 `_ = &a` ⇒ 现在**必须**出现根与 `Load/Store`（同一测试里两段）
- [ ] Commit：`feat(ssa): slot 提升（Task 19）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build promote`

---

### Task 20: 直线表达式（常量/算术/转换/拷贝）

**Files:**
- Modify: `src/build.rs`

**Interfaces:**
- Consumes: `rgoc_hir::ExprKind`（Const/Binary/Unary/Convert/Slot/Global/Nil）

- [ ] `Const` ⇒ `Instr::Const`（保留 `ConstVal` 的任意精度）；`Binary` ⇒ `Bin`；`Unary` ⇒ `Un`；`Convert` ⇒ `Convert`；读未取址 slot ⇒ 当前值（必要时 `Copy`）
- [ ] 测试：`a := 1 + 2*3` 的 dump 与期望结构一致
- [ ] Commit：`feat(ssa): 直线表达式（Task 20）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build linear`

---

### Task 21: 赋值与短声明（含多左值）

**Files:**
- Modify: `src/build.rs`

- [ ] `Stmt::Assign` / `ShortDecl`：右值全求值 → 逐个写（未取址 ⇒ 更新当前值；取址 ⇒ `Store`）
- [ ] 测试：`a, b := 1, 2; a = b` 的当前值映射正确
- [ ] Commit：`feat(ssa): 赋值与短声明（Task 21）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build assign`

---

### Task 22: `if/else` 与块参数（含并行拷贝）

**Files:**
- Modify: `src/build.rs`

**Interfaces:**
- Produces: 汇合块的 `params` 与边上 `args`（`Term::If` 的二元组）

- [ ] 两支各建块，汇合块参数 = 两支都改过的值；边上带实参
- [ ] **并行拷贝**：`a, b = b, a` 的两个实参在边上必须各自独立（不许先写 `a` 再读 `b`）
- [ ] `T-SSA-16` + `T-SSA-28`
- [ ] Commit：`feat(ssa): if/else 与块参数（Task 22）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build if_join`、`parallel_copy`

---

### Task 23: `for` 三形态（含回边）

**Files:**
- Modify: `src/build.rs`

- [ ] `for{}` / `for cond{}` / `for init;cond;post{}`；回边把块参数重新喂值
- [ ] `T-SSA-17`：三种形态各一条；断言回边存在（`preds(header)` 含 latch）
- [ ] Commit：`feat(ssa): for 三形态（Task 23）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build for_`

---

### Task 24: `switch`（含 `fallthrough`）

**Files:**
- Modify: `src/build.rs`

- [ ] 有 tag / 无 tag（等价 if 链）各一；`fallthrough` 连边到下一 case
- [ ] `T-SSA-18`
- [ ] Commit：`feat(ssa): switch（Task 24）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build switch_`

---

### Task 25: `goto` 与 labeled 跳转

**Files:**
- Modify: `src/build.rs`

- [ ] 前向 `goto`（先建空块占位，回填）、反向 `goto`、labeled `break`/`continue`
- [ ] `T-SSA-19`
- [ ] Commit：`feat(ssa): goto 与 labeled（Task 25）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build goto_`

---

### Task 26: 多返回值与结果直连

**Files:**
- Modify: `src/build.rs`

- [ ] `Call{dsts}` 按签名结果数建值；`a,b := f()` 逐个接住
- [ ] `return f()` ⇒ `Return(f.dsts)`（**结构断言：无中间值**）
- [ ] `g(f())` ⇒ `g.args` 里含 `f.dsts`（按 `spread` 展平）
- [ ] `T-SSA-20` / `T-SSA-21` / `T-SSA-22`
- [ ] Commit：`feat(ssa): 多返回值直连（Task 26）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build multi_value`

---

### Task 27: 调用与 effect 摘要

**Files:**
- Modify: `src/build.rs`
- Create: `src/effect.rs`

**Interfaces:**
- Produces: `effect::summarize(&Program) -> Vec<Effect>`（`writes` / `writes_all` / `may_panic` / `may_call_back`，递归到不动点）

- [ ] 摘要：函数内直接写的根（`Root::Global` / `Heap`）+ 被调函数的摘要；递归或未知 ⇒ `writes_all = true`
- [ ] 测试：`func f(){ g = 1 }` 的摘要含该 global；只读函数 `writes` 空且 `may_panic=false`
- [ ] `T-SSA-23`
- [ ] Commit：`feat(ssa): 调用与 effect（Task 27）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build effect`

---

### Task 28: 变参与方法调用

**Files:**
- Modify: `src/build.rs`

- [ ] `f(xs...)` / `f(1,2,3)` 到 `...int`；`Call.variadic` 标记
- [ ] `CalleeRef::Method { recv, func }`（值接收者与指针接收者各一）
- [ ] `T-SSA-29` / `T-SSA-30`
- [ ] Commit：`feat(ssa): 变参与方法调用（Task 28）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build variadic method_call`

---

### Task 29: 复合字面量

**Files:**
- Modify: `src/build.rs`

- [ ] 数组 / 结构体 / slice 字面量 ⇒ `Composite { typ, items }`；items 顺序 == 源码顺序（keyed 按 lowering 定死的下标）
- [ ] `T-SSA-24`
- [ ] Commit：`feat(ssa): 复合字面量（Task 29）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build composite`

---

### Task 30: 指针、取址与写穿

**Files:**
- Modify: `src/build.rs`

- [ ] `&x` ⇒ `AddrOf(root)`；`*p = v` ⇒ `Store`（带 root）；`p.f` ⇒ `Field`；`a[i]` ⇒ `Index`
- [ ] `T-SSA-25`：写穿后读同一根要用新的 mem
- [ ] Commit：`feat(ssa): 指针与写穿（Task 30）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build pointer`

---

### Task 31: 构建期拒绝面（crate-local `SsaCode`，9 个码）

> **执行期修订 `R-31-1`/`R-31-2`**：白名单 7 → 9（函数值 / `range` over string 原先没码）；
> `U-M6-05/06/07` 在本前端够不着 ⇒ 证据改成守卫测试。判据面同步见 `M6-tests.md` §5。

**Files:**
- Create: `rgoc/crates/rgoc-ssa/src/diag.rs`（`SsaCode` 7 变体 + `ALL` + `code() -> &'static str`，**沿 `rgoc-hir::RejectKind` 的形状**）
- Modify: `rgoc/crates/rgoc-ssa/src/build.rs`（拒绝分支）
- Test: `rgoc/crates/rgoc-driver/tests/ssa_reject.rs`

**Interfaces:**
- Produces: `SsaCode`（`Closure` / `Defer` / `IfaceDyn` / `Map` / `Chan` / `RangeFunc` / `Generic`）+ `Reject { func, code: SsaCode, pos }`；driver 渲染成 stderr 一行
- **不动** `rgoc-lex::DiagCode`（`D-M6-14`：`C8` 的 26 类冻结、`other` 值域被 `T-CK-16` 钉死）

- [x] `diag.rs`：九个变体按字典序 + `ALL` + `code()`（`-CHAN` / `-CLOSURE` / `-DEFER` / `-FUNC-VALUE` / `-GENERIC` / `-IFACE-DYN` / `-MAP` / `-RANGE-FUNC` / `-RANGE-STR`）
- [x] 拒绝分支（一个**预扫描** pass，`unsupported()`）：闭包（`!captured.is_empty()`）/ `Stmt::Defer` / `BuiltinKind::Recover` / `Callee::Iface` + `Stmt::TypeSwitch` + `TypeAssert` / `map`（按类型，任意出现）/ `chan`（按类型 + `close` + `<-ch`）/ `FuncLit` + `Bound` / `RangeKind::Str`；`range` over func 与泛型由上游先拒（守卫测试）
- [x] `T-SSA-33` 的 `S` 腿：每种一个最小 fixture，断言码 + 退出码 1（6 条直接 fixture + 3 条守卫）
- [x] 反向守卫：把任一拒绝分支改成静默通过 ⇒ 测试必红（防「拒了但没人知道」；3 条变异全红）
- [ ] Commit：`feat(ssa): 不支持面 7 码与拒绝分支（Task 31）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_reject`（11 条）；`… cargo test -p rgoc-check --test category`（`DiagCode` 仍是 103，**必须没动**）

---

## Phase 3：求值器与三方差分（Task 32–39）

### Task 32b: 合成包初始化函数（**执行期插入**，`R-32-1`）

> Task 31 跑语料时发现：`Program::init_order`（`var g = f()` 的右侧）**只有解释器跑**，
> 而 `build()` 从头到尾没读它 ⇒ SSA 模块里的全局永远是零值。三方差分（`T-SSA-32`）
> 必然抓到，所以必须在 Phase 3 开头补上。

**Files:** Modify: `rgoc/crates/rgoc-ssa/src/build.rs`、`src/ir.rs`（`Module::init`）

- [ ] `build()` 把 `init_order` 的每组 `(globals, rhs)` 建成一个**合成 `SsaFunc`**
      （`name = "init"`，写全局根，`Term::Return`），按声明序排；`inits`（各文件 `init()`）
      接在后面，`main` 之前执行
- [ ] `Module::init: Option<usize>`（`funcs` 下标）；`dump` 如实列出
- [ ] 判据：`var g = 7; func main(){ println(g) }` 的 dump 里 init 函数写 `r_g`，
      且 `main` 的 `Load` 用 init 产出的 mem（`T-SSA-36` 的同类）
- [ ] Commit：`feat(ssa): 包级初始化合成函数（Task 32b）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build init`

---

### Task 32: `ssa_exec` 骨架（显式帧 + 按块执行）

**Files:**
- Create: `rgoc/crates/rgoc-interp/src/ssa_exec.rs`
- Modify: `rgoc/crates/rgoc-interp/src/lib.rs`（`pub mod ssa_exec;`）

**Interfaces:**
- Produces: `ssa_exec::run_ssa(&Module, &TypeTable, Options) -> Outcome`（复用 `interp::Outcome`）
- Consumes: `rgoc_ssa::{Module, Instr, Term}`、`interp::{Value, Cell, Options, Outcome}`

- [ ] 帧：`struct SFrame { func: usize, block: BlockId, idx: usize, values: Vec<Value>, mems: Vec<Value> }`；块参数写入 `values`
- [ ] 主循环：取当前块的下一条指令执行，`Term` 决定下一块（**不递归**）
- [ ] 空函数可跑（`main` 立即返回，exit 0）
- [ ] Commit：`feat(interp): ssa_exec 骨架（Task 32）`

**Verify:** `… cargo test -p rgoc-interp --test ssa_exec`（新建，1 条空函数）

---

### Task 33b: `&&` / `||` 的短路进 IR（**执行期插入**，`R-33-1` / `G-M6-08`）

> Task 33 开工前核对 `ops` 的接口时发现：**`&&` / `||` 在 SSA 里没有短路**。
> `build.rs` 把 `Binary{LAnd}` 建成一条 `Bin` 指令，两个操作数都求值 ——
> 而 M5 的解释器有 `Task::Short` 专门做短路。副作用（调用、panic）会被多执行一次，
> 结果值却仍然正确 ⇒ **静默错**，三方差分才抓得到。

**Files:** Modify: `rgoc/crates/rgoc-ssa/src/build.rs`、`src/build.rs` 的 `join_into`

- [ ] `expr(Binary{LAnd|LOr})` 建成三块：`lhs` → `if` → `rhs` / 短路出口 → 汇合（块参数 bool）
- [ ] 汇合处 `val` / `mem` 两条路径都要合（沿 `stmt_if` 的 `join_into`）
- [ ] 判据：`f() && g()` 里 `g` 在 `f()` 为假时**不被调用**（手搓 IR 数调用次数）
- [ ] Commit：`feat(ssa): &&/|| 短路（Task 33b）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_build short`

---

### Task 33: 纯值与运算（复用 `ops`）

**Files:**
- Modify: `ssa_exec.rs`；可能 `ops.rs`（把 `Interp::convert` 的纯函数部分抽出来）

- [x] ~~`Const/Copy/Bin/Un/Convert/Composite/Field/Index/Slice/Make/AddrOf` 全实现~~
      **执行期修订 `R-33-1`**：`Field/Index/AddrOf` 依赖访存，并入 **Task 35**；
      Task 33 只做**不碰内存**的值计算：`Const/Copy/Bin/Un/Convert/Composite/Make/Slice`
      + `print`/`println`（没有输出通道，Task 33 就没有判据）
- [ ] 算术走 `ops::bin_arith` / `ops::compare`（计划稿写的 `apply_binary`/`apply_unary`
      在 `ops` 里**不存在** —— 见 `R-33-1`）
- [ ] 测试：`1+2*3`、`int8(127)+1` 回绕、`string` 拼接各一
- [ ] Commit：`feat(interp): 纯值求值（Task 33）`

**Verify:** `… cargo test -p rgoc-interp --test ssa_exec values`

---

### Task 34: 控制流与块参数（并行赋值）

**Files:**
- Modify: `ssa_exec.rs`

- [ ] `Goto/If`：先**一次性**算出全部边实参，再写入目标块 `params`（并行）
- [ ] `Return/Panic/Unreachable`：`Panic` 走 M5 的 `panic: <值>` + 退出码 2
- [ ] `T-SSA-28` 的求值面：`a, b = b, a` 输出正确
- [ ] Commit：`feat(interp): 控制流与块参数（Task 34）`

**Verify:** `… cargo test -p rgoc-interp --test ssa_exec control_flow`

---

### Task 35: 访存、调用与内建（执行期修订：`R-35-4` 作废「按 root 索引 mem」）

**Files:**
- Modify: `rgoc-ssa/src/{ir,build,dump}.rs`、`rgoc-interp/src/{ssa_exec,interp}.rs`

- [x] `CalleeRef::Func/Method/Indirect`：`Func`/`Method` **只压帧**（`ret_to = dsts`），控制权回主循环 ——
      不能「压帧 + 驱动到底」（那是宿主栈递归，`max_frames` 挡不住）；`Indirect` 是 M6 面外，运行期报错
- [x] `Builtin`：`print/println/len/cap/append/copy/min/max/complex/real/imag/new/clear`，
      复用 M5 的 `print::format_operand` 与 `interp` 的纯函数（`append_value` 抽成两套求值器共用）
- [x] `Load/Store`：运行期**按真实地址**访存（`Value::Ptr` 的 `Cell`），`root` 只用于静态定序（`R-35-4`）
- [x] IR 补三项运行期必需的信息：`Module::global_types`、`SsaFunc::slot_types`、`Instr::Call::dots`（`R-35-1`）
- [x] `Term::Return` 把结果写进调用方的 `ret_to`（先全读、再弹帧、再写）
- [x] 端到端 `rgoc-driver/tests/ssa_run.rs`（Task 35 部分 18 条）+ 语料闭环 `ssa_corpus_run.rs`（46 个 `N` 全绿）
- [x] Commit：`feat(interp): SSA 访存、调用与内建（Task 35）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_run --test ssa_corpus_run`

---

### Task 36: 预算与退出码

**Files:**
- Modify: `ssa_exec.rs`

- [x] 步数计数（默认 1e8，与 M5 同口径）+ 超限 ⇒ `Outcome.exit = 2` + stderr 原因
- [x] 帧深上限（默认 1000）：无限递归 ⇒ 退出码 2 + 原因（不靠宿主栈溢出兜）
- [x] `T-SSA-40`：死循环 fixture 触发上限；**显式预算 1e7** 钉墙钟 < 10 s（`R-36-1`）
- [x] Commit：`feat(interp): 预算与退出码（Task 36）`

**Verify:** `… cargo test -p rgoc-interp --test ssa_exec budget`

---

### Task 37: `run --via={hir,ssa}`

**Files:**
- Modify: `rgoc/crates/rgoc-driver/src/cli.rs`、`run_cmd.rs`

**Interfaces:**
- Produces: `RunCommand { via: Via }`（默认 `Hir`）；`Via::Ssa` 时走 `build` → `verify` → `opt` → `run_ssa`

- [x] 加 `--via`；默认 `hir`（M5 行为不变）
- [x] 非法值 ⇒ `CliError::BadVia`（退出码 2）+ 稳定消息
- [x] `Via::Ssa` 时 `build` → **`verify`** → `run_ssa`（verifier 不是可选开关，`R-37-1`）
- [x] `T-SSA-39`：默认通路不变；两条通路输出逐字节一致；拒绝 ⇒ 1、panic ⇒ 2
- [x] Commit：`feat(driver): run --via（Task 37）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_cli via_`

---

### Task 38: 三方差分接入

**Files:**
- Modify: `tools/run-oracle/run-collide.sh`（Task 5 的骨架填实）
- Test: `rgoc/crates/rgoc-driver/tests/ssa_diff.rs`

- [ ] `--ssa` 形态跑通：hir / ssa / go run 三列
- [ ] `T-SSA-32` / `T-SSA-34` / `T-SSA-35` / `T-SSA-36` / `T-SSA-37`：在 `N`（Task 39 之前用「当前能建图的集合」）上全绿
- [ ] Commit：`feat(ssa): 三方差分（Task 38）`

**Verify:** `… bash tools/run-oracle/run-collide.sh --run --ssa --list tools/run-oracle/run-selected.txt` ⇒ 绿；`grep -c 'DIFF'` 无输出

---

### Task 39: 切片实测冻结

**Files:**
- Modify: `tools/run-oracle/ssa-rejected.txt`（填 11 行以内）
- Modify: `docs/milestones/M6-tests.md`（§6 填 `N`/`M` 实测值 + 修订行 `R-6-1`）
- Test: `rgoc/crates/rgoc-driver/tests/ssa_slice.rs`

- [ ] 跑分类器，得到 `N`/`M`；`M` 每行落三栏
- [ ] 两向断言：`N+M==69`、`M` 的码 ⊆ 白名单
- [ ] 反向守卫：把 `M` 一行挪进 `N` 清单 ⇒ 测试必红（写死在测试里）
- [ ] Commit：`test(ssa): 切片实测冻结 N/M（Task 39）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_slice`；`awk -F'\t' 'NF!=3' tools/run-oracle/ssa-rejected.txt` 无输出

---

## Phase 4：passes（Task 40–44）

### Task 40: pass 框架与开关

**Files:**
- Create: `rgoc/crates/rgoc-ssa/src/opt/mod.rs`
- Modify: `ssa_cmd.rs` / `run_cmd.rs`（`--opt=`）

**Interfaces:**
- Produces: `opt::Config { fold: bool, dce: bool, copy: bool, cse: bool }`；`opt::run(&mut Module, &TypeTable, Config)`；`Config::parse("all|none|fold,dce")`

- [ ] 框架：每个 pass 一个 `fn(&mut SsaFunc, &TypeTable)`，跑完**立刻 verify**（`SSA-5` 的「每 pass 后 verifier 绿」）
- [ ] `--opt=` 解析 + 默认 `all`
- [ ] `T-SSA-64` 骨架：`all` / `none` 两路输出一致
- [ ] Commit：`feat(ssa): pass 框架与开关（Task 40）`

**Verify:** `… cargo test -p rgoc-ssa --test opt_config`

---

### Task 41: `fold`

**Files:**
- Create: `src/opt/fold.rs`

- [ ] 常量折叠（`Bin/Un/Convert`）+ `If` 常量条件简化边；溢出走 `rgoc-const` 的可表示性（不静默截断）
- [ ] `T-SSA-60`
- [ ] Commit：`feat(ssa): fold（Task 41）`

**Verify:** `… cargo test -p rgoc-ssa --test opt_fold`

---

### Task 42: `dce`

**Files:**
- Create: `src/opt/dce.rs`

- [ ] 删无使用且无副作用的值；`Store`/有 effect 的 `Call` 不删
- [ ] `T-SSA-61`（含「删了会错」反例：effect 非空的调用必须留下）
- [ ] Commit：`feat(ssa): dce（Task 42）`

**Verify:** `… cargo test -p rgoc-ssa --test opt_dce`

---

### Task 43: `copy`

**Files:**
- Create: `src/opt/copy.rs`

- [ ] `Copy` 链合并；块参数实参里的 `Copy` 消解（**不得**破坏并行语义）
- [ ] `T-SSA-62`
- [ ] Commit：`feat(ssa): copy（Task 43）`

**Verify:** `… cargo test -p rgoc-ssa --test opt_copy`

---

### Task 44: `cse`（块内纯值）

**Files:**
- Create: `src/opt/cse.rs`

- [ ] 单块内相同 `(op, operands, typ)` 的纯指令合一；**跨块不合**
- [ ] `T-SSA-63`（正例 + 跨块反例）
- [ ] Commit：`feat(ssa): 块内 CSE（Task 44）`

**Verify:** `… cargo test -p rgoc-ssa --test opt_cse`

---

## Phase 5：活跃性与协议（Task 45–48）

### Task 45: 值级活跃性

**Files:**
- Create: `rgoc/crates/rgoc-ssa/src/live.rs`
- Test: `tests/live.rs`

**Interfaces:**
- Produces: `analyze(&SsaFunc, &Cfg) -> Liveness { live_in: Vec<BTreeSet<ValueId>>, live_out: .., intervals: BTreeMap<ValueId, (Pos, Pos)> }`

- [ ] 迭代到不动点（含回边）；`live_out[b] = ∪ live_in[succ]`，`live_in = use ∪ (live_out − def)`
- [ ] `T-SSA-55`：直线 / 分支 / 循环（回边上变量活跃）各一
- [ ] Commit：`feat(ssa): 活跃性（Task 45）`

**Verify:** `… cargo test -p rgoc-ssa --test live`

---

### Task 46: `SSA-LIVE v1` 输出

**Files:**
- Modify: `src/live.rs`（`render`）

- [ ] 逐行格式照设计 §3.7；排序键 = 块号 + 指令下标
- [ ] `T-SSA-56` / `T-SSA-59`：字段自洽 + 两次逐字节相同
- [ ] Commit：`feat(ssa): SSA-LIVE v1（Task 46）`

**Verify:** `… cargo test -p rgoc-ssa --test live render`

---

### Task 47: 消费方桩

**Files:**
- Create: `rgoc/crates/rgoc-driver/tests/live_consumer.rs`

- [ ] 桩只解析文本（不碰 IR 内部），构造「每个 safepoint 的存活指针集合」并断言非空
- [ ] `T-SSA-57`
- [ ] Commit：`test(ssa): 协议消费方桩（Task 47）`

**Verify:** `… cargo test -p rgoc-driver --test live_consumer`

---

### Task 48: 派生指针保守策略

**Files:**
- Modify: `src/live.rs`

- [ ] 派生指针（`Field/Index` 出来的指针）活跃 ⇒ 其根活跃；协议 `ptrs` 含根
- [ ] `T-SSA-58`
- [ ] Commit：`feat(ssa): 派生指针保守策略（Task 48）`

**Verify:** `… cargo test -p rgoc-ssa --test live derived`

---

## Phase 6：收口（Task 49–53）

### Task 49: 变异 8 条

**Files:**
- Create: `scripts/mutation-m6-notes.md`

- [ ] 逐条施加 `T-SSA-47` / `T-SSA-48` / `T-SSA-49` / `T-SSA-50` / `T-SSA-51` / `T-SSA-52` / `T-SSA-53` / `T-SSA-54` 的变异（每次 `cargo clean -p` 对应 crate）→ 确认变红 → 复原
- [ ] 记录施加点、红在哪、复原状态
- [ ] Commit：`test(m6): 变异 8 条记录（Task 49）`

**Verify:** 8/8 实证变红；workspace 回到全绿

---

### Task 50: `M6-manifest.json` 填实 + `manifest_m6.rs`

**Files:**
- Modify: `docs/milestones/M6-manifest.json`
- Create: `rgoc/crates/rgoc-driver/tests/manifest_m6.rs`

- [ ] 按 `M6-tests.md` §7 的门禁映射逐字段填
- [ ] 对账测试：三类谎必须被拦（`T-SSA-44`）
- [ ] Commit：`test(m6): manifest 填实与对账（Task 50）`

**Verify:** `… cargo test -p rgoc-driver --test manifest_m6`

---

### Task 51: 契约定稿（`C4` 升正式 / `C10` 定稿 / `C9` 补节）

**Files:**
- Modify: `docs/contracts/C4-ssa.md`、`C10-ssa-dump.md`、`C9-values-and-output.md`

- [ ] `C4`：五项需求登记逐条给答案（指向 `M6-design.md` §3.2–§3.5）
- [ ] `C10`：定稿（去掉「骨架」字样，格式与实现逐行对账）
- [ ] `C9`：补「SSA 求值器共用同一 `Value` 与合并流」一节
- [ ] `T-SSA-46`：文档语法行与真实 dump 逐行对得上
- [ ] Commit：`docs(m6): 契约定稿（Task 51）`

**Verify:** `… cargo test -p rgoc-driver --test ssa_cli dump_contract`

---

### Task 52: 交付 B 报告与文档回写

**Files:**
- Create: `docs/milestones/M6-report.md`
- Modify: `docs/milestones/GAP-01.md`、`docs/03-roadmap.md`、`AGENTS.md`

- [ ] 报告：覆盖矩阵（roadmap 十句 × 四腿）、三条分别声明（SSA 支持面 / 优化边界 / live set 协议）、偏离登记、缺口、接手指南
- [ ] `GAP-01` 补 `PIPE-17/18/21/26`；`03-roadmap` 与 `AGENTS.md` 的 M6 状态节（**只在 M6 收口时**改）
- [ ] Commit：`docs(m6): 报告与状态回写（Task 52）`

**Verify:** 报告里每个数字都能由 §Verify 的命令复现

---

### Task 53: 全量回归

**Files:**
- Modify: `.superpowers/sdd/M6-plan/progress.md`

- [ ] 四条 cargo + 五条既有自检 + `run-collide` 五形态 + `--ssa` 形态
- [ ] `T-SSA-42` / `T-SSA-43` / `T-SSA-45`：全绿、测试数 ≥ 1608、语料 hash 通过
- [ ] Commit：`chore(m6): 全量回归（Task 53）`

**Verify:** 四条 cargo exit 0；`bash tools/run-oracle/run-collide.sh --run --ssa` 全绿；`shasum -a 256 -c corpus-manifest.sha256` 通过

---

## 附：Task ↔ 门禁 ↔ 测试 ID

| Task | 门禁 | 测试 ID |
|---|---|---|
| 1–6 | （装置） | `T-SSA-14/31/32` 的骨架 |
| 7–16 | `SSA-1` / `SSA-2` | `T-SSA-01..14` |
| 17–31 | `SSA-3` / `SSA-7` | `T-SSA-15..30`、`33` |
| 32–39 | `SSA-4` / `SSA-6` | `T-SSA-31..37`、`39/40` |
| 40–44 | `SSA-5` | `T-SSA-60..64` |
| 45–48 | `SSA-8` | `T-SSA-55..59` |
| 49–53 | `SSA-9` | `T-SSA-41..46`、`47..54` |
