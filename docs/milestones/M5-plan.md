# M5 实施计划：HIR 与顺序解释器

> **状态**：**已执行（Task 1–52；51 ✅ / 1 🟡 / 0 ❌）**　|　**测试清单**：[`M5-tests.md`](./M5-tests.md)（判据面已冻结）　|　**机器可读事实**：[`M5-manifest.json`](./M5-manifest.json)（已填实）　|　**交付 B**：[`M5-report.md`](./M5-report.md)（顺序语义基线·候选）
>
> **任务编号约定**：本计划用 **「Task N」（N=1..52）** 标识任务，**不与测试 ID `T-EX-*` 混用编号空间**。
> 执行期进度、偏离与 Ruling 记在 `.superpowers/sdd/M5-plan/progress.md`（不入库）。
>
> ⚠️ **本文件的 `- [ ]` 勾选框没有被维护过**（执行期 46 个任务直接写代码、没有逐条勾选）。
> 真实状态以 `progress.md` 的逐任务表与 `M5-report.md` §5/§6 为准 —— **不要**把这里的空框读成「没做」。
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `rgoc` 能把一份 Go 源码**跑起来** —— `rgoc-driver run` 输出与官方 `go run` 逐字节一致（合并流）、退出码一致 —— 覆盖 roadmap §M5 的顺序语义子集：HIR lowering、显式帧栈、四类赋值语义、内建、闭包/接口动态语义、defer/panic/recover、初始化执行。

**Architecture:** 新 `rgoc-hir`（正式，替代 spike；spike 迁 `rgoc-hir-spike` 保住 `SM-M0-NATIVE-HELLO`）+ 新 `rgoc-interp`（值模型 + 显式帧栈 + 显式求值栈）。类型信息**不另建表**：`rgoc-check::Session` 的 7 个逐节点入口开成 `pub`，lowering 持同一会话边 lower 边取 `Operand`（`D-M5-2`）。`rgoc-driver run` 先 check 后 lower 再执行，诊断非空即拒跑。验收四腿：自有 fixture（S）/ 官方语料对撞（O）/ 装置与 CLI（H）/ P0 前置（S+O），外加 8 条变异反向验证。

**Tech Stack:** Rust 1.98.1 / edition 2024 / resolver 3；workspace 在 `rgoc/`；全部命令经 `scripts/in-container.sh`（宿主不构建）。oracle 侧用容器内 `go1.27.1` 的 `go run`。

**Spec:** [`docs/milestones/M5-design.md`](./M5-design.md)（设计）＋ [`docs/milestones/M5-tests.md`](./M5-tests.md)（判据，已冻结）。**执行者必须同时读这两份** —— 本计划只决定「顺序与接口」，判据以 tests 清单为准。

## Global Constraints

- **只在容器内构建/测试**：一切 `cargo`/`go` 命令经 `scripts/in-container.sh <cmd>`；cargo 必须 `bash -lc 'cd rgoc && …'`（`R-9-1`）。
- **不默认新增第三方依赖**：Rust 侧只用 std + workspace 既有依赖。M5 **不需要**任何新 crate 依赖（`Rc`/`RefCell` 都在 std）。
- **`go_source_code/` 只读**：跑完语料批次后 `corpus-manifest.sha256` 必须校验通过（`T-EX-56`）。
- **`unsafe_code = "forbid"`**（workspace lint）：解释器**不许**用 `unsafe`；panic 控制流**不许**用 `catch_unwind`（`D-M5-1` §3.7）。
- **显式帧纪律**（`D-M5-1`）：`step_*` 不得递归；中间态必须在 `FrameState` / `eval` 栈 / `values` 栈里。
- **类型来源纪律**（`D-M5-2`）：lowering **不得**自行推导类型；拿不到类型 ⇒ 先修检查器（P0），不许在 lowering 里补规则。
- **诊断纪律**：用户可见的未实现 ⇒ 稳定码 `HIR-UNSUPPORTED-*` + `unsupported` 登记；内部不可达才 `todo!`。
- **测试失败 → 先确认失败来自缺失行为**（不是环境/harness），再做最小实现；测试之前写的代码一律删除。
- **产物陈旧陷阱**：变异前后 `cargo clean -p <被变异 crate>`；可疑时 `find rgoc/target -mindepth 1 -delete` 全量重建（约 4 min）。见 `scripts/mutation-m3-notes.md` §0。
- **资源上限**：帧深 1000 / 求值栈深 10000 / 步数 1e8 / 单程序墙钟 10 s。
- 每个任务结束都**提交**；提交信息引用 Task 与 `T-EX-*`。

## Review Focus

以下是 `M5-design.md` 隐含、但**没有哪个任务的测试天然覆盖**的五类失效模式，最可能的排前面。每条都已把测试钉到拥有该代码的任务上：

1. **数组/结构体的「拷贝」被写成 `Rc` 共享**：这是最隐蔽的一类 —— `hello` 通过、`for` 通过、`slice` 通过，只有 `a := arr; a[0]=1` 那一下才露。表示层一错，后面所有复合值语义全错（Task 29，`T-EX-11/12`）。
2. **defer 的参数被「执行时」求值**：写起来比「登记时」自然（少一个临时 slot），但 `defer f(x); x=2` 立刻错，而且**只在有副作用的 fixture 上暴露**（Task 42，`T-EX-34`）。
3. **`recover` 的「直接调用限制」被放宽**：`defer recover()` 看起来应该能工作，实际无效（返回值被丢）。放宽了会让 `T-EX-35` 绿得莫名其妙（Task 43，`T-EX-35`）。
4. **帧栈退化成 Rust 递归**：任何一处 `step` 里写了 `self.eval_expr(e)` 递归，`D-M5-1` 就名存实亡，而 90% 的 fixture 仍会绿 —— 只有 1000 层递归那条会红（Task 21/22，`T-EX-17/67`）。
5. **合并流被拆成两条**：一旦 `print` 走 stderr、`println` 走 stdout（或反过来），47 个 `print/println` 语料文件的**期望全落空**，而「只比 stdout」的实现会让它们「输出为空」误判为通过（Task 25，`T-EX-50/68`）。

---

## 执行总览

| Phase | Task | 内容 |
|---|---|---|
| 0 | 1–8 | **P0 前置收口**：候选清单固化 + A/B/C/D 四族规则修复 + 79/80 门禁 |
| 1 | 9–19 | 契约与 HIR：`C9` 契约、spike 迁移、`rgoc-hir` 骨架、`Session` 开面、lowering（decls/stmt/expr）、dump |
| 2 | 20–28 | 求值内核：`rgoc-interp` 骨架、值、求值栈、帧、算术、控制流、打印、初始化、`run` 子命令、预算 |
| 3 | 29–35 | 复合值与内建：数组/结构体、slice、string、map、指针、内建全集、转换 |
| 4 | 36–41 | 闭包与接口：函数值/捕获、loopvar、接口/typed nil、动态调用/方法值、断言/type switch、泛型单态化 |
| 5 | 42–45 | 异常：defer、panic/recover、命名返回值、未捕获 panic |
| 6 | 46–52 | 集成收口：语料清单、`run-oracle`、三方差分、变异、回归、报告、manifest |

---

## Phase 0：P0 前置收口（Task 1–8）

> **为什么 M5 从「修检查器」开始**：实测 80 个候选里 `rgoc check` 只接受 **69**（`M5-design.md` `F2`），
> 而拒绝的 4 族里 **A（多值调用）与 B（调用语句结果丢弃）正是 M5 的核心语义面** ——
> 不修它们，lowering 连 `b, x := f()` 与 `f()` 都过不去（`D-M5-3`）。

### Task 1: 固化候选清单与现状基线

**Files:**
- Create: `tools/run-oracle/run-candidates.txt`（80 行）
- Create: `tools/run-oracle/run-excluded.txt`（6 行 + 逐行理由）
- Create: `tools/run-oracle/survey.sh`（可重放的筛选脚本，`T-EX-41` 的「口径可重放」）
- Create: `.superpowers/sdd/M5-plan/baseline.txt`（不入库：69/80 的逐文件结论）

**Interfaces:**
- Consumes: `M5-tests.md` §6.1（80 个文件名）、§6.2（6 个排除）
- Produces: 后续所有任务的**分母**；`T-EX-41/45/47` 的数据源

**Steps:**
- [ ] 按 `M5-tests.md` §6.1 的口径写 `survey.sh`（`// run`/`// runoutput` + 零 import + 无并发），跑出 80 行，与文档逐行对账
- [ ] 写 `run-excluded.txt`：6 行（5 个 `runoutput` + `reflectmethod2.go`），每行 `文件<TAB>理由<TAB>指针`
- [ ] 跑 `rgoc-driver check` 逐个记录，存 `baseline.txt`（69 接受 / 11 拒绝，拒绝行带诊断首行）

**Verify:** `scripts/in-container.sh bash -lc 'cd /work && bash tools/run-oracle/survey.sh | wc -l'` ⇒ `80`；`wc -l tools/run-oracle/run-excluded.txt` ⇒ `6`

---

### Task 2: A 族 —— 多值调用（`T-EX-57`）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/expr.rs`（`assignment` / 调用结果元数）
- Modify: `rgoc/crates/rgoc-check/src/stmt.rs`（短声明与赋值左侧的元数匹配）
- Create: `rgoc/crates/rgoc-check/tests/multi_value.rs`

**Interfaces:**
- Consumes: `Operand` 的 `commaok/commaerr` 模式（已存在）
- Produces: 调用结果元数 `n`；`b, x := f()`、`a, b = f()`、`g(f())` 三种语境都能正确展开

**Steps:**
- [ ] RED：`method5.go:103` 形态（`b, x := f(1000, 99)`，f 返回 `(byte,int)`）⇒ 断言零诊断；当前报 `assignment mismatch: 2 variables but f returns 1 value`
- [ ] 定位：调用结果的元数在 `expr.rs` 的 `Call` 分支被折叠成 1（tuple 未被识别）
- [ ] GREEN：按 `Signature.results` 的个数展开；`g(f())` 作为实参时按官方规则（仅当 f 是唯一实参）
- [ ] 补 `simassign.go:75` 的 `swap(swap(a,b))` 形态

**Verify:** `scripts/in-container.sh bash -lc 'cd rgoc && cargo test -p rgoc-check --test multi_value'`；`… rgoc-driver check go_source_code/test/method5.go` ⇒ exit 0

---

### Task 3: B 族 —— 调用语句的结果丢弃（`T-EX-58`）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/stmt.rs`（表达式语句的合法性判定）
- Create: `rgoc/crates/rgoc-check/tests/expr_stmt.rs`

**Interfaces:**
- Consumes: `Operand::mode`（`novalue` / `value` / `commaok`）
- Produces: 「表达式语句」白名单 = **调用**（含内建调用）/ 接收 / `x++`/`x--`；其余报 `CHK-UNUSED-EXPR`

**Steps:**
- [ ] RED：`divmod.go:126` 的 `divzerouint(x, y)`（有返回值，当语句）⇒ 当前报 `value of type uint is not used`
- [ ] RED 反向：`x + 1` 当语句**仍须**报 `CHK-UNUSED-EXPR`（不许把规则删掉）
- [ ] GREEN：判 `Expr::Call` 直接放行；`escape3.go` / `ken/rob1.go` 一并转绿

**Verify:** `… rgoc-driver check go_source_code/test/divmod.go` ⇒ exit 0；`… go_source_code/test/escape3.go` ⇒ exit 0；反向 fixture 仍 exit 1

---

### Task 4: C 族 —— 复合字面量取址（`T-EX-59`）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/expr.rs`（`&` 的操作数可寻址性判定）
- Create: `rgoc/crates/rgoc-check/tests/addr_composite.rs`

**Interfaces:**
- Consumes: `Operand::mode`（`value` vs `variable`）
- Produces: `&T{...}` 合法；`&f()` 仍拒

**Steps:**
- [ ] RED：`compos.go:16` 的 `return &T{1}` ⇒ 当前报 `cannot take address of T`
- [ ] GREEN：复合字面量在取址语境下**物化**（视为可寻址临时）；`ddd.go:189/222` 一并转绿
- [ ] 反向：`&f()`（函数调用结果）仍报

**Verify:** `… check go_source_code/test/compos.go` ⇒ 0；`… check go_source_code/test/ddd.go` ⇒ 0

---

### Task 5: D 族 —— 移位计数 / slice→数组指针 / `make(map[T]V)`（`T-EX-60`）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/expr.rs`（移位运算的类型规则）
- Modify: `rgoc/crates/rgoc-types/src/convertible.rs`（slice→数组指针，Go 1.17）
- Modify: `rgoc/crates/rgoc-check/src/expr.rs`（`make` 的类型参数与底层同一）
- Create: `rgoc/crates/rgoc-check/tests/shift_convert_make.rs`

**Interfaces:**
- Consumes: `convertible` 判定核（M4 已绿）
- Produces: 三小族各自的修复；若某族牵出**成片**规则（如类型参数核心类型）⇒ **停下并登记**（`D-M5-3` 失效条件），该文件进排除

**Steps:**
- [ ] RED：`ken/shift.go:82` 的 `i <<= c`（`c` 为 `uint`）⇒ 当前报 mismatched types；官方只要求计数为整数（`F3-D`）
- [ ] RED：`convert4.go:73` 的 `(*Int4)(slice)` ⇒ 当前当作赋值拒绝
- [ ] RED：`bigmap.go:43` 的 `make(map[T]V)`（`T`/`V` 为本地具名类型）⇒ 当前报底层类型不匹配
- [ ] GREEN：逐族最小修复；**每族单独提交**（便于回滚）
- [ ] 若某族超出预算 ⇒ 该文件写进 `run-excluded.txt` + 理由，**不改判据**

**Verify:** `… check go_source_code/test/ken/shift.go`、`convert4.go`、`bigmap.go` 三条各自 exit 0

---

### Task 6: P0 门禁 —— 79/80 与 M4 不回退（`T-EX-61/62`）

**Files:**
- Create: `rgoc/crates/rgoc-driver/tests/accept_rate_m5.rs`
- Create: `.superpowers/sdd/M5-plan/accept.txt`（不入库）

**Interfaces:**
- Consumes: `run-candidates.txt` / `run-excluded.txt`
- Produces: 「候选 80 = 接受 79 ∪ 例外 1」的两向断言；M4 的 63 条 `T-CK-*` 快照

**Steps:**
- [ ] 写 `accept_rate_m5.rs`：逐文件跑 `check_package`，断言「拒绝集合 == {`reflectmethod2.go`}」
- [ ] 跑 `cargo test -p rgoc-check -p rgoc-driver`：M4 的 63 条必须全绿
- [ ] 跑 `run-collide.sh` 五形态：退出码与红/绿分布与 M4 收口快照一致

**Verify:** `… cargo test -p rgoc-driver --test accept_rate_m5`；`… cargo test --workspace` 无新增失败

---

### Task 7: P0 变异（`T-EX-63` 的邻居，不计入门禁 8 条）

**Files:**
- Modify: `scripts/mutation-m5-notes.md`（新建）

**Steps:**
- [ ] 对 Task 2–5 的四处修复各做一次「改回原状」的变异，确认对应 `T-EX-57..60` 变红
- [ ] 复原并记录（格式沿 `mutation-m4-notes.md`）

**Verify:** 四处变异各自变红、复原后全绿

---

### Task 8: Phase 0 收口

**Files:**
- Modify: `.superpowers/sdd/M5-plan/progress.md`

**Steps:**
- [ ] 四条 cargo 全绿（`T-EX-53` 的第一次记录）
- [ ] 五条既有自检全过（`T-EX-54` 的第一次记录）
- [ ] 记录 P0 的实际产出（修了几族、是否触发 `D-M5-3` 失效条件）

**Verify:** `scripts/in-container.sh bash -lc 'cd rgoc && cargo test --workspace --no-fail-fast'` ⇒ 0 failed

---

## Phase 1：契约与 HIR（Task 9–19）

### Task 9: `C9` 契约（值表示 / 输出字节 / 退出码）

**Files:**
- Create: `docs/contracts/C9-values-and-output.md`
- Modify: `docs/README.md`（契约清单登记 C9）

**Interfaces:**
- Consumes: `M5-design.md` §3.3 / §3.5 / §4.2；`M5-tests.md` §4
- Produces: 值表示（`Value`/`Cell`/`MapObj`）、四类赋值语义、`print/println` 字节表、合并流、退出码表、三件不承诺

**Verify:** `docs/contracts/C9-values-and-output.md` 的「字节格式」节与 `M5-tests.md` §4.2 逐行一致（人工对账 + 一条 doc 测试）

---

### Task 10: spike HIR 迁移到 `rgoc-hir-spike`（`D-M5-4`）

**Files:**
- Move: `rgoc/crates/rgoc-hir` → `rgoc/crates/rgoc-hir-spike`（`git mv`）
- Modify: `rgoc/Cargo.toml`（members）
- Modify: `rgoc/crates/rgoc-spikes/Cargo.toml`（依赖路径）
- Modify: `rgoc/crates/rgoc-spikes/src/{interp,ssa,native}.rs`（`rgoc_hir::` → `rgoc_hir_spike::`）
- Modify: `docs/contracts/C3-hir.md`（「反推来源」追加指针）

**Steps:**
- [ ] `git mv` + 改包名（`Cargo.toml` 的 `name = "rgoc-hir-spike"`）
- [ ] 全局替换 `rgoc_hir` → `rgoc_hir_spike`（只在 `rgoc-spikes` 内）
- [ ] 跑 `cargo test -p rgoc-spikes`：**一条测试都不许改**，必须全绿
- [ ] 跑 `SM-M0-NATIVE-HELLO` 的 smoke（`rgoc-spikes` 的 s3 测试）

**Verify:** `… cargo test -p rgoc-spikes` ⇒ 全绿；`… cargo test -p rgoc-spikes --test s3_native_e2e` ⇒ 通过

---

### Task 11: 新 `rgoc-hir` 骨架与 HIR 类型

**Files:**
- Create: `rgoc/crates/rgoc-hir/Cargo.toml`、`src/lib.rs`、`src/program.rs`、`src/stmt.rs`、`src/expr.rs`、`src/place.rs`、`src/diag.rs`
- Modify: `rgoc/Cargo.toml`（members 加回 `rgoc-hir`）

**Interfaces:**
- Consumes: `M5-design.md` §3.2 的完整定义
- Produces: `Program`/`Func`/`Stmt`/`Expr`/`Place`/`Capture`；`SlotId`/`FuncId`/`GlobalId` 新类型

**Steps:**
- [ ] 按 §3.2 **逐字**落地类型定义（先只有类型，`lower` 留 `todo!()` 骨架）
- [ ] `#![deny(missing_docs)]` + 每个变体一句注释（说明它对应哪个 Go 构造）
- [ ] `T-EX-01` 的穷尽性断言先写成 `ALL` 常量 + 计数测试（沿 `DiagCode` 五件套范式）

**Verify:** `… cargo check -p rgoc-hir`；`… cargo test -p rgoc-hir`

---

### Task 12: HIR 中性 dump

**Files:**
- Create: `rgoc/crates/rgoc-hir/src/dump.rs`
- Create: `rgoc/crates/rgoc-hir/tests/dump.rs`

**Interfaces:**
- Produces: 确定性文本 dump（`# func <name>` / 缩进块 / 节点按遍历序编号）；供 `--dump-hir` 与 `T-EX-04`

**Steps:**
- [ ] 格式定死：每行 `<depth>\t<kind>\t<detail>`；头行 `# program` / `# funcs: N`
- [ ] 两次运行逐字节相同（`T-EX-04`）

**Verify:** `… cargo test -p rgoc-hir --test dump`

---

### Task 13: `Session` 开面（`D-M5-2`）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/expr.rs`（7 个 `pub(crate) fn` → `pub fn`）
- Modify: `rgoc/crates/rgoc-check/src/check.rs`（`Session::new` 若未 pub 则开）
- Modify: `rgoc/crates/rgoc-check/src/lib.rs`（re-export）

**Interfaces:**
- Produces: `Session::{expr, expr_hint, conversion, assignment, valid_index, type_expr, convert_untyped}` 全部 `pub`；`Operand` 的 `mode`/`typ`/`val` 可读

**Steps:**
- [ ] 逐个开面，**只改可见性**（不改语义）
- [ ] 加一条「签名冻结」测试：用 `fn assert_sig` 钉住 7 个入口的参数/返回类型（防将来漂移）

**Verify:** `… cargo test --workspace` 全绿（开面不许引入任何行为变化）

---

### Task 14: lowering 骨架（decls / 全局 / 函数表）

**Files:**
- Create: `rgoc/crates/rgoc-hir/src/lower.rs`

**Interfaces:**
- Consumes: `rgoc_load::Package`、`&mut rgoc_check::Session`、`rgoc_check::initorder::analyze`
- Produces: `lower(pkg, session) -> Result<Program, Vec<HirReject>>`；全局变量表 + `init_order` + 函数表

**Steps:**
- [ ] 遍历 `Package` 的 decls：`func` → `Func`（帧槽分配：形参/命名返回值/局部/临时）
- [ ] 全局 var → `Global`；`init_order` 直接取 `initorder::analyze(pkg).order`（`F16`）
- [ ] `main` 定位；`init` 按文件序
- [ ] 守卫：`session` 的诊断流必须为空，非空 ⇒ `HirReject::InconsistentCheck`（`T-EX-03`）

**Verify:** `… cargo test -p rgoc-hir --test lower_decls`

---

### Task 15: lowering 语句

**Files:**
- Modify: `rgoc/crates/rgoc-hir/src/lower.rs`

**Interfaces:**
- Produces: `Stmt` 全变体；`go`/`select`/`chan` ⇒ `HirReject::Unsupported(kind)`

**Steps:**
- [ ] `Assign`/`ShortDecl`/`Expr`/`If`/`For`/`Range`/`Switch`/`TypeSwitch`/`Return`/`Branch`/`Labeled`/`Block`/`Defer`
- [ ] **求值顺序显式化**：赋值先算完 rhs 再逐个写 lhs（临时 slot）
- [ ] `defer` 的实参在 lowering 期展开为临时 slot（`D-M5-9` 的「登记时求值」在 HIR 层就定死）
- [ ] `go` / `chan` / `select` / `range` over func ⇒ `HirReject::Unsupported`（`T-EX-02`）

**Verify:** `… cargo test -p rgoc-hir --test lower_stmt`（含 `T-EX-02` 的 5 条拒绝）

---

### Task 16: lowering 表达式

**Files:**
- Modify: `rgoc/crates/rgoc-hir/src/lower.rs`

**Interfaces:**
- Consumes: `session.expr(file, e)` / `expr_hint` / `type_expr` / `conversion`（Task 13）
- Produces: `Expr` 全变体；`Place` 的五形态（`T-EX-05`）

**Steps:**
- [ ] 常量（`session` 给的 `Operand.val`，**保留任意精度**直到收敛点）、名字、二元/一元、调用、索引、切片、选择器、复合字面量、函数字面量、断言
- [ ] `Addr` 只从 `Place` 产生；`&T{}` ⇒ 物化临时 slot + `Addr(Slot)`（Task 4 的语义答案）
- [ ] **不许**自己推类型：每个节点都从 `Operand.typ` 取；取不到 ⇒ `HirReject::MissingType`（响亮失败，`D-M5-2` 失效条件）

**Verify:** `… cargo test -p rgoc-hir --test lower_expr`

---

### Task 17: 复合字面量与函数字面量的物化

**Files:**
- Modify: `rgoc/crates/rgoc-hir/src/lower.rs`

**Interfaces:**
- Produces: `Composite` 的 elem 顺序（keyed/unkeyed）；`FuncLit` 的独立 `Func` + `Capture` 列表

**Steps:**
- [ ] keyed 复合字面量按**字段下标**展开（lowering 期定死，求值期不查名）
- [ ] 函数字面量：捕获分析（自由变量 → `Capture { outer_slot }`），**按变量**不是按值
- [ ] 嵌套函数字面量的捕获链

**Verify:** `… cargo test -p rgoc-hir --test lower_composite`

---

### Task 18: `rgoc-driver hir --dump`

**Files:**
- Modify: `rgoc/crates/rgoc-driver/src/cli.rs`（`HirCommand`）
- Create: `rgoc/crates/rgoc-driver/src/hir_cmd.rs`
- Create: `rgoc/crates/rgoc-driver/tests/test_hir_cmd.rs`

**Interfaces:**
- Produces: `rgoc-driver hir [--dump] <files…>`；dump 走 stdout、诊断走 stderr（`T-EX-51` 的一半）

**Verify:** `… cargo test -p rgoc-driver --test test_hir_cmd`；两次运行 sha256 相同

---

### Task 19: Phase 1 收口

**Steps:**
- [ ] 四条 cargo + 五条自检
- [ ] `T-EX-01..06` 全绿（`EX-1` 达成）
- [ ] 记录进度

**Verify:** `… cargo test -p rgoc-hir -p rgoc-driver` ⇒ 全绿

---

## Phase 2：求值内核（Task 20–28）

### Task 20: `rgoc-interp` 骨架与值

**Files:**
- Create: `rgoc/crates/rgoc-interp/Cargo.toml`、`src/lib.rs`、`src/value.rs`
- Modify: `rgoc/Cargo.toml`（members）

**Interfaces:**
- Consumes: `C9`（Task 9）
- Produces: `Value`/`Cell`/`MapObj`/`NilKind`；`deep_copy()`；`type_of()`

**Steps:**
- [ ] 按 `M5-design.md` §3.3 逐字落地
- [ ] `deep_copy` 的递归深度受预算约束（防深结构爆栈 —— 这里允许递归，因为它不承载 Go 调用语义）

**Verify:** `… cargo test -p rgoc-interp --test value`

---

### Task 21: 求值栈与任务机（`D-M5-1` 的核心）

**Files:**
- Create: `rgoc/crates/rgoc-interp/src/eval.rs`

**Interfaces:**
- Produces: `EvalTask` 枚举 + `step_eval(&mut Interp)`；**不递归**

**Steps:**
- [ ] `EvalTask` 覆盖 `Expr` 全变体（`Expr(e)` / `Bin(op)` / `Call(argc)` / `Index` / `Slice` / `Assert` / `Composite(n)` …）
- [ ] `step_eval` 一次只做一件事：取任务 → 推子任务 / 压值
- [ ] 深度计数（`T-EX-52` 的求值栈上限）
- [ ] **反向**：把 `step_eval` 写成递归 `eval_expr` ⇒ `T-EX-67` 必红

**Verify:** `… cargo test -p rgoc-interp --test eval`

---

### Task 22: 帧栈与调用/返回

**Files:**
- Create: `rgoc/crates/rgoc-interp/src/frame.rs`、`src/call.rs`

**Interfaces:**
- Produces: `Frame`/`FrameState`/`Interp`；主循环 `run_program`；调用 = push、返回 = pop

**Steps:**
- [ ] `FrameState` 六态（`Enter`/`Stmt`/`Eval`/`CallReturn`/`RunningDefers`/`Done`）
- [ ] 形参绑定（值传递 = `deep_copy`）、命名返回值槽、多返回值写回
- [ ] 变参 `...`：末参打包成 slice
- [ ] 1000 层递归 fixture（`T-EX-17`）：**不爆 Rust 栈**

**Verify:** `… cargo test -p rgoc-interp --test frame`；`… --test recursion_1000`

---

### Task 23: 算术与比较（宽度/回绕/除零）

**Files:**
- Create: `rgoc/crates/rgoc-interp/src/ops.rs`

**Interfaces:**
- Produces: `bin_op(op, x, y, typ) -> Result<Value, RuntimeError>`；按宽度掩码

**Steps:**
- [ ] 整数各宽度回绕、有符号右移（算术移位）、移位计数取模（Go 语义：`x << 64` 在 `uint64` 上是 0）
- [ ] 除零/模零 ⇒ `RuntimeError::DivideByZero`（转 panic，`T-EX-07`）
- [ ] 浮点：`float32` 每次运算后按 f32 舍入；`NaN`/`±Inf`
- [ ] 比较：数值、字符串、指针身份、接口（动态类型 + 值）

**Verify:** `… cargo test -p rgoc-interp --test ops`

---

### Task 24: 控制流

**Files:**
- Modify: `rgoc/crates/rgoc-interp/src/frame.rs`

**Steps:**
- [ ] if/for/switch（含 `fallthrough`）/goto/labeled break/continue
- [ ] `range` 四形态（array/slice/string/map/int），**每轮新建迭代变量**（为 loopvar 铺路，Task 37）

**Verify:** `… cargo test -p rgoc-interp --test control_flow`

---

### Task 25: 输出面（`print/println`）

**Files:**
- Create: `rgoc/crates/rgoc-interp/src/print.rs`

**Interfaces:**
- Consumes: `M5-tests.md` §4.2 的格式表
- Produces: `OutBuf`（**单一合并 buffer**）；`format_value(v, typ) -> String`

**Steps:**
- [ ] 按 `F10` 表逐行实现（空格分隔 / 无分隔 / `%g` 精度 -1 / `0x0` / `(0x0,0x0)` / `[0/0]0x0` / rune 十进制 / 宽度）
- [ ] **一个** buffer（`T-EX-50`）：不许分 stdout/stderr 两条
- [ ] 浮点边界（`NaN`/`Inf`/`-0`）在 RED 期用 `go run` 对撞后补钉（`R-4-1`）

**Verify:** `… cargo test -p rgoc-interp --test print`（`T-EX-24`）

---

### Task 26: 初始化执行

**Files:**
- Create: `rgoc/crates/rgoc-interp/src/init.rs`

**Interfaces:**
- Consumes: `Program.init_order` / `Program.inits` / `Program.main`
- Produces: `run_program` 的启动序

**Steps:**
- [ ] 单包：全局按 `init_order` → `init`（文件序）→ `main`（`T-EX-39`）
- [ ] 双包：`rgoc-load` 的依赖图深度优先（`T-EX-40`）
- [ ] 顺序 fixture：每个初始化式 `print` 自己的名字，输出序 == `InitOrder`

**Verify:** `… cargo test -p rgoc-interp --test init_order`

---

### Task 27: `rgoc-driver run`

**Files:**
- Modify: `rgoc/crates/rgoc-driver/src/cli.rs`（`RunCommand`）
- Create: `rgoc/crates/rgoc-driver/src/run_cmd.rs`
- Create: `rgoc/crates/rgoc-driver/tests/test_run_cmd.rs`

**Interfaces:**
- Produces: `rgoc-driver run [--dump-hir] [--max-steps N] <files…>`；退出码 0/1/2（`T-EX-49`）

**Steps:**
- [ ] 流程：读文件 → `version_source` → `load_with_imports` → `check_package`（有诊断 ⇒ exit 1）→ `lower` → `run_program`
- [ ] `--dump-hir` 先 dump 再跑（`T-EX-51`）
- [ ] 合并流写 stdout；诊断/运行时错误写 stderr

**Verify:** `… rgoc-driver run go_source_code/test/helloworld.go` ⇒ 输出 `hello, world\n`、exit 0

---

### Task 28: 预算与三道上限

**Files:**
- Create: `rgoc/crates/rgoc-interp/src/budget.rs`

**Steps:**
- [ ] 帧深 1000 / 求值栈 10000 / 步数 1e8，各自独立计数
- [ ] 超限 ⇒ `RunOutcome { exit: 2, msg }`；**宿主不 abort**（`T-EX-52`）
- [ ] 墙钟由 driver 侧计时（10 s），超时归 `timeout` 分类

**Verify:** `… cargo test -p rgoc-interp --test budget`（三个超限 fixture 各自 exit 2）

---

## Phase 3：复合值与内建（Task 29–35）

### Task 29: 数组与结构体（拷贝语义，`T-EX-11/12`）

**Files:**
- Modify: `rgoc/crates/rgoc-interp/src/value.rs`、`src/eval.rs`

**Steps:**
- [ ] `Array`/`Struct` 的赋值 = `deep_copy()`（新建 `Rc` + 新 `Cell`）
- [ ] 字段选择器按 lowering 定的下标（不查名）
- [ ] **反向**：改成共享 `Rc` ⇒ `T-EX-63` 必红

**Verify:** `… cargo test -p rgoc-interp --test aggregates`

---

### Task 30: slice（别名/append/copy/三索引，`T-EX-13`）

**Files:**
- Modify: `rgoc/crates/rgoc-interp/src/{value,eval,builtin}.rs`

**Steps:**
- [ ] `{arr, off, len, cap}`；赋值只复制三元组
- [ ] `append`：cap 够则原地、否则新建（**扩容策略不承诺**，只承诺语义）
- [ ] `copy` 处理重叠；三索引切片限制 `len ≤ cap`

**Verify:** `… cargo test -p rgoc-interp --test slice`

---

### Task 31: string（不可变/索引/切片/range，`T-EX-10`）

**Steps:**
- [ ] `Rc<str>`；`s[i]` 得 byte；`s[a:b]` 按字节；`range` 按 rune（含 UTF-8 解码失败 ⇒ `U+FFFD`，与 Go 一致）
- [ ] `+` 拼接产生新 `Rc`

**Verify:** `… cargo test -p rgoc-interp --test string`

---

### Task 32: map（键相等/插入序/delete/双值，`T-EX-14`）

**Files:**
- Modify: `rgoc/crates/rgoc-interp/src/value.rs`、`src/builtin.rs`

**Steps:**
- [ ] `MapKey`：`NaN != NaN`、结构体按字段、数组按元素、接口按 `(动态类型, 值)`、指针按身份
- [ ] 迭代 = **插入序**（`U-M5-04`）
- [ ] `delete` / `clear` / 双值 `v, ok` / nil map 读不 panic、写 panic

**Verify:** `… cargo test -p rgoc-interp --test map`

---

### Task 33: 指针与可寻址性（`T-EX-15`）

**Steps:**
- [ ] `Ptr(Cell)`；`*p` 读/写；nil 解引用 ⇒ panic
- [ ] `&s[i]` / `&arr[i]` / `&st.f` 都指向**同一个 Cell**（与直接访问一致）
- [ ] `&T{}`（Task 17 物化的临时 slot）

**Verify:** `… cargo test -p rgoc-interp --test pointer`

---

### Task 34: 内建全集（`T-EX-23`）

**Files:**
- Modify: `rgoc/crates/rgoc-interp/src/builtin.rs`

**Steps:**
- [ ] `len`/`cap`/`make`/`new`/`append`/`copy`/`delete`/`clear`/`min`/`max`/`complex`/`real`/`imag`/`print`/`println`/`panic`/`recover`
- [ ] `close` ⇒ `Unsupported`（`U-M5-12`）

**Verify:** `… cargo test -p rgoc-interp --test builtins`

---

### Task 35: 类型转换（`T-EX-16`）

**Steps:**
- [ ] 数值互转（截断/符号扩展/浮点↔整数）
- [ ] `string`↔`[]byte`/`[]rune`（拷贝，不是别名）
- [ ] `rune`/`byte`；常量收敛（复用检查器已算好的常量值）

**Verify:** `… cargo test -p rgoc-interp --test convert`

---

## Phase 4：闭包与接口（Task 36–41）

### Task 36: 函数值与闭包捕获（`T-EX-25/26`）

**Steps:**
- [ ] `Value::Func(Rc<Closure>)`：`FuncId` + 捕获的 `Cell` 列表
- [ ] **按变量**捕获（共享 `Cell`）；返回出去的闭包其 `Cell` 由 `Rc` 保活

**Verify:** `… cargo test -p rgoc-interp --test closure`

---

### Task 37: loopvar（1.22，`T-EX-27`）

**Steps:**
- [ ] `for` 每轮**新建**迭代变量 `Cell`（1.22 语义）
- [ ] fixture：三个闭包分别打印 0/1/2

**Verify:** `… cargo test -p rgoc-interp --test loopvar`

---

### Task 38: 接口与 typed nil（`T-EX-29`）

**Steps:**
- [ ] `Iface { typ: Option<TypeId>, data }`；`var i I` ⇒ `typ=None`（nil）
- [ ] `var p *T; var i I = p` ⇒ `typ=Some(*T)`、`data=Ptr(null)` ⇒ `i != nil`
- [ ] 接口相等：先比动态类型再比值；`NaN` 在接口里同样不等

**Verify:** `… cargo test -p rgoc-interp --test iface`

---

### Task 39: 动态方法调用 / 方法值 / 方法表达式（`T-EX-28`）

**Steps:**
- [ ] 接口调用：按动态类型的 `FuncId` 查表（M4 的 `methodset` 已给方法集）
- [ ] 方法值 `x.M` = 绑定接收者的闭包；方法表达式 `T.M` = 首参是接收者
- [ ] 指针接收者与值接收者的自动取址/解引用

**Verify:** `… cargo test -p rgoc-interp --test method_value`

---

### Task 40: 类型断言与 type switch（`T-EX-30/31`）

**Steps:**
- [ ] 单值断言失败 ⇒ panic；双值 ⇒ `ok=false`
- [ ] 接口→接口断言（动态类型是否实现目标）
- [ ] type switch：`nil` case、多类型 case、绑定变量的类型

**Verify:** `… cargo test -p rgoc-interp --test assert`

---

### Task 41: 泛型单态化（`T-EX-32/33`）

**Steps:**
- [ ] 消费 M4 的 `instantiate`：同一 `(origin,args)` 复用同一实例
- [ ] 泛型函数/泛型类型各一组 fixture
- [ ] 超出面 ⇒ `HirReject::Unsupported`（`U-M5-10`）

**Verify:** `… cargo test -p rgoc-interp --test generic`

---

## Phase 5：异常（Task 42–45）

### Task 42: defer（时机 + LIFO，`T-EX-34`）

**Files:**
- Create: `rgoc/crates/rgoc-interp/src/exc.rs`

**Steps:**
- [ ] `Deferred { slots: Vec<Value>, call: CallSite }` —— 参数**已求值**（lowering 期展开，Task 15）
- [ ] LIFO；函数正常返回时先跑 defer 再写回返回值
- [ ] **反向**：改成执行时求值 ⇒ `T-EX-65` 必红

**Verify:** `… cargo test -p rgoc-interp --test defer`

---

### Task 43: panic / recover（`T-EX-35`）

**Steps:**
- [ ] `unwind`：沿帧栈弹，每帧跑 defer
- [ ] `recover()` 仅在**正在跑的 defer 的直接调用帧**里有效（用一个 `recovering: Option<FrameId>` 标记）
- [ ] `defer recover()`（返回值被丢）⇒ 无效（`T-EX-35` 的反向）

**Verify:** `… cargo test -p rgoc-interp --test recover`

---

### Task 44: 命名返回值 + recover（`T-EX-36`）

**Steps:**
- [ ] 命名返回值是帧槽（`Cell`），defer 里可写
- [ ] `func f() (r int)` + recover 改写 ⇒ 调用方看到改写值

**Verify:** `… cargo test -p rgoc-interp --test named_result`

---

### Task 45: 嵌套 panic 与未捕获（`T-EX-37/38`）

**Steps:**
- [ ] 展开中再 panic ⇒ 覆盖原值，继续展开
- [ ] 未捕获 ⇒ `panic: <值>\n` 进合并流 + exit 2；**不**打 traceback（`U-M5-03`）
- [ ] `panic(nil)` ⇒ `panic: nil`（`U-M5-08`）

**Verify:** `… cargo test -p rgoc-interp --test panic_nested`

---

## Phase 6：集成收口（Task 46–52）

### Task 46: 语料清单追加冻结（`R-6-1`）

**Files:**
- Create: `tools/run-oracle/run-selected.txt`
- Modify: `tools/run-oracle/run-excluded.txt`（补齐 P0 后仍不合格的行）
- Create: `tools/run-oracle/run-ken.txt`

**Steps:**
- [ ] 按 `M5-tests.md` §6.3 的四条判据逐个过；逐行写排除理由
- [ ] 断言「入选 ∪ 排除 == 候选」（`T-EX-41`）
- [ ] 记录入选数（若 < 60，如实记，`M5-tests.md` §6.3）

**Verify:** `… cargo test -p rgoc-driver --test run_corpus`（两向断言）

---

### Task 47: `tools/run-oracle` 与 `run-collide.sh --run`

**Files:**
- Create: `tools/run-oracle/main.go`、`run-collide.sh` 的 `--run` 分支
- Modify: `tools/types-oracle/run-collide.sh`（或新建 `tools/run-oracle/run-collide.sh`，二者共用纪律）

**Steps:**
- [ ] oracle：`go run` + 合并流 + `.out`/空 + `\r\n` 归一（`F4`）
- [ ] `--run [--list]`：逐文件两侧比对合并流 + 退出码（`T-EX-42`）
- [ ] 无 `.out` 且输出非空 ⇒ 红（`T-EX-70` 的守卫）

**Verify:** `… bash tools/run-oracle/run-collide.sh --run --list tools/run-oracle/run-selected.txt go_source_code/test` ⇒ exit 0

---

### Task 48: 三方差分（`T-EX-43`）

**Steps:**
- [ ] 入选集上跑 `rgoc` / `.out` / `go run`，三方一致才算绿
- [ ] `.out` 与 `go run` 不一致 ⇒ 该文件进排除（理由：期望已脱节）

**Verify:** `… bash tools/run-oracle/run-collide.sh --run --three-way` ⇒ 报告 0 不一致

---

### Task 49: 变异探针 8 条（`T-EX-63..70`）

**Files:**
- Create: `scripts/mutation-m5-notes.md`

**Steps:**
- [ ] 逐条施加 → 确认变红 → 复原（`cargo clean -p` 每次）
- [ ] 记录施加点、红在哪个测试、复原后状态

**Verify:** 8 条全部实证变红并复原；workspace 回到全绿

---

### Task 50: `M5-manifest.json` 填实 + `manifest_m5.rs`

**Files:**
- Modify: `docs/milestones/M5-manifest.json`（骨架 → 填实）
- Create: `rgoc/crates/rgoc-driver/tests/manifest_m5.rs`

**Steps:**
- [ ] 按 `M5-tests.md` §10 的映射逐字段填
- [ ] 写对账测试：三类谎必须被拦（`T-EX-55`）

**Verify:** `… cargo test -p rgoc-driver --test manifest_m5`

---

### Task 51: `M5-report.md`（交付 B）

**Files:**
- Create: `docs/milestones/M5-report.md`

**Steps:**
- [ ] 覆盖矩阵（roadmap §M5 五条 × 四腿）
- [ ] 三条分别声明：泛型（单态化到哪）、loopvar（1.22 语义）、range-over-func（显式拒绝）
- [ ] 语料实测（入选/排除/绿）、缺口清单、接手指南
- [ ] **措辞**：顺序语义基线是「**候选**」，正式冻结等 M8e

**Verify:** 报告里的每个数字都能由 §9 的命令复现

---

### Task 52: 全量回归与文档收口

**Steps:**
- [ ] 四条 cargo + 五条既有自检 + `run-collide.sh` 全部形态
- [ ] `AGENTS.md` / `03-roadmap.md` 的状态节（**只在 M5 完成时**改）
- [ ] `GAP-01.md` 补 `PIPE-10/14/15/16/16c/24` 与 `RT-*` 落点
- [ ] `C3` 升正式（§2 的「待验证」改为「已定」+ 指向 `M5-design.md` §3.2）

**Verify:** 四条 cargo exit 0；五条自检全过；`EX-1..EX-9` 全绿

---

## 附：任务 ↔ 门禁 ↔ 测试 ID 速查

| Task | 门禁 | 测试 ID |
|---|---|---|
| 1–8 | （P0 前置） | `T-EX-57..62` |
| 9 | `EX-1`（契约） | — |
| 10–18 | `EX-1` | `T-EX-01..06` |
| 20–28 | `EX-1/7` | `T-EX-49..52` |
| 29–35 | `EX-3` | `T-EX-10..16`、`23/24` |
| 36–41 | `EX-4` | `T-EX-25..33` |
| 42–45 | `EX-5` | `T-EX-34..38` |
| 46–48 | `EX-6` | `T-EX-41..48` |
| 49 | `EX-9` | `T-EX-63..70` |
| 50–52 | `EX-8/9` | `T-EX-53..56` |
