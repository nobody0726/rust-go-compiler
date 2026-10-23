# M5 报告（交付 B）：顺序语义基线

> **状态**：**候选** —— 顺序语义基线**候选**，正式冻结等 M8e（`D-M5-14`）。
> 本报告里的每个数字都能由 §9 的命令复现；凡不能复现的，写在这里就是错的。
> **阶段**：M5（`rgoc-driver run` 跑起来）　|　**产出**：2026-10-23（收口期补写）
> **上游**：[`M5-design.md`](./M5-design.md)（D-M5-1..15 / F1–F20）/ [`M5-tests.md`](./M5-tests.md)（70 个 `T-EX-*`）/ [`M5-plan.md`](./M5-plan.md)（Task 1–52）/ [`M5-manifest.json`](./M5-manifest.json)（机器可读事实）
> **契约**：[`C9`](../contracts/C9-values-and-output.md)（值表示 / 输出字节 / 退出码）、[`C3`](../contracts/C3-hir.md)

---

## 1. 一句话结论

**`rgoc` 能把顺序子集的 Go 程序跑起来，输出与 `go run` 逐字节一致**：入选 69 个官方语料
**69/69 全绿**（合并流 + 退出码），自有 fixture 40 条全绿，8 条变异探针全部实证变红。
**门禁 5/9 `pass`、4 条 `partial`** —— 四条 partial 的缺口是**内容缺口**（常量溢出诊断、
双包 init、泛型实例化、M4 的 CH-2 遗留），不是收口漏洞，逐条登记在 §6。

---

## 2. 覆盖矩阵（roadmap §M5 五条 × 四腿）

`S` = 自有 fixture（`rgoc-driver/tests/run_semantics.rs` 等）；`O` = 官方语料对撞（`tools/run-oracle`）；
`H` = 装置与 CLI；`反` = 变异探针。

| # | roadmap §M5 的句子 | S | O | H | 反 | 结论 |
|---|---|---|---|---|---|---|
| 1 | HIR lowering、环境/调用帧、按值/按引用捕获、可寻址性、多返回值 | `T-EX-01..05`、`17..19`、`25` | 69/69 兜底 | `--dump-hir` 确定性（`T-EX-04`） | `T-EX-67`（帧栈退化） | ✅ 兑现 |
| 2 | 基础值、数组/struct 拷贝、指针、slice 共享底层、不可变 string、map 语义适配 | `T-EX-07..16`、`23` | 69/69 | — | `T-EX-63/64`（拷贝/共享反写） | ✅ 兑现 |
| 3 | 内建、接口动态值/方法调用/断言/type switch、typed nil、method value 和闭包 | `T-EX-23`、`25..31` | 69/69 | — | — | ✅ 兑现 |
| 4 | 解释执行包变量/init/main；defer/panic/recover（直接调用限制、命名返回值、嵌套） | `T-EX-34..39` | 69/69 | 退出码表（`T-EX-49`） | `T-EX-65/66`（defer 时机、recover 放宽） | ⚠️ **单包兑现**；双包 init（`T-EX-40`）未兑现 ⇒ `G-M5-02` |
| 5 | 泛型解释子集可单态化；loopvar 捕获语义直接测试；range over func 显式拒绝 | `T-EX-27`、`32/33` | — | — | — | ⚠️ **部分**：loopvar ✅ / range-over-func 拒绝 ✅ / 泛型只到 `any` + 推断（`G-M5-03/04`） |

**四腿分母**：甲 40 / 乙 8 / 丙 8 / 丁 6 / 反 8 = 70 个 `T-EX-*`（`M5-tests.md` §3）。

---

## 3. 三条分别声明（`D-M5-14` 要求逐条表态，不许含糊）

### 3.1 泛型

| 面 | 现状 | 证据 |
|---|---|---|
| **能用** | `any` 约束 + 类型推断：`Id(1)` / `Id("s")` 正确单态化并执行 | `T-EX-32` 的正向半边 |
| **不能用** | 显式实例化 `F[int](1)`、泛型类型字面量 `Box[int]{v:7}`、`Sum([]int{...})` —— **在 check 面被拒**（`CHK-INVALID-INDEX` / `CHK-INVALID-SELECTOR` / `CHK-INVALID-ASSIGN`） | `G-M5-03` |
| **拒绝码** | 用的是**通用 `CHK-*` 码**，不是专属 `HIR-UNSUPPORTED-GENERIC` | `G-M5-04`（码在 `rgoc-hir/src/diag.rs` 里，但走不到） |

**判断**：拒绝是**确定的、响亮的、不静默算错**（三条 fixture 退出码 1 + stdout 空），
判据想防的事都防住了；但「单态化到哪」这句话的答案是
**「执行面就位，检查面只放进来 `any` + 推断这一种形态」**。根因与 M4 的 `A3` 同源：
`dump` 根集不产实例（`M4-report.md` §4.3）。

### 3.2 loopvar（Go 1.22 语义）

**已兑现**：`for i := 0; i < 3; i++ { fs = append(fs, func(){ print(i) }) }` ⇒ `012`（每轮新建变量）。
`T-EX-27` 是端到端 fixture（不是查 AST），语料侧 `loopvar*.go` 在候选内且已入选/排除按 §6.3 口径处理。

### 3.3 range over func

**显式拒绝**：`range` 的操作数是函数类型时，lowering 报 `HIR-UNSUPPORTED-RANGE-FUNC`（退出码 1），
不是静默跳过、不是 `todo!()`。白名单项 `U-M5-06`，正向证据 `T-EX-02` 的第五条；
`test/range4.go` 因此在排除清单（`R-6-2`，§5 第 3 条）。

---

## 4. 语料实测（`EX-6` 的核心）

| 项 | 数字 | 复现命令 |
|---|---|---|
| 候选 | **80**（顶层 48 + `ken/` 32） | `wc -l tools/run-oracle/run-candidates.txt` |
| 入选 | **69**（顶层 37 + `ken/` 32） | `run-collide.sh --run --list run-selected.txt` |
| 排除 | **11**（冻结 6 + `R-6-2` 追加 5） | `wc -l tools/run-oracle/run-excluded.txt` |
| 两向断言 | 80 = 69 ∪ 11，无死行、无重复 | `cargo test -p rgoc-driver --test run_corpus` |
| 对撞 | **69 个文件：绿 69 / 红 0**（合并流逐字节 + 退出码） | §9 第 5 条 |
| 三方差分 | 5 个有 `.out` 的文件：`rgoc` / `.out` / `go run` 三方一致 | §9 第 6 条 |
| `ken/` 专项 | **32/32 绿** | §9 第 5 条（`run-ken.txt`） |

**关于「没有 `.out` 的 64 个文件」**：官方期望是**空输出**，`go run` 与 `rgoc` 都是空 ——
这 64 个不是「跳过」，是「期望为空且实测为空」。`T-EX-70` 的变异（把「无 `.out`」当「不比」）
正是靠 `compared == 69` 这个分母才变红的（`R-3-1`）。

---

## 5. 偏离登记（**与冻结口径不一致的每一处都在这里**）

| # | 偏离 | 性质 | 处置 | 认账处 |
|---|---|---|---|---|
| 1 | **`C9` 契约（Task 9）与 `M5-report.md`（Task 51）在收口期才产出** | 顺序偏离 | 两份都按「从实现逐条核对」补写，不照设计稿抄；`C9` 的强度因此比「先契约后代码」弱一档，已在该文件头部声明 | 本报告 §5、`C9` 头部、`M5-manifest.json` 的 `docs` |
| 2 | **排除数 6 → 11**（`R-6-2`） | 口径变更 | 5 行追加排除（`divmod` / `convert4` / `method7` / `literal` / `range4`），逐行四栏进 `run-excluded.txt`，判断依据进 `M5-tests.md` §6.4 | `G-M5-06` |
| 3 | **白名单追加 `U-M5-13/14`**（`R-7-1`） | 白名单扩容 | 两条各对应第 2 条里的一行排除；没有它们，那两行排除就只剩「不做了」 | `M5-tests.md` §7.1 |
| 4 | **反腿「期望红在」有两处预测未兑现**（`R-3-1`） | 预测错 | ① `T-EX-68` 实测红在 `T-EX-06/24/50`（`T-EX-42` 不比字节）；② `T-EX-70` **首轮没红**，补 `compared == 69` 分母后才红 —— 这是本轮唯一一处**测试改动** | `scripts/mutation-m5-notes.md` |
| 5 | **`manifest` 的 `measured` 曾比实测少 2 条测试、1 个 suite**（写 1606/149，实测 1608/150） | 记录滞后 | **已对齐**：收口期复跑后把 `budget.measured` 与 `phase_summary.workspace_tests` 两处同时改成 1608 / 150（判据口径不变，只改数字） | `M5-manifest.json` 的 `budget.measured` |
| 6 | **Task 52 的三项文档回写未做**：`C3` 升正式（仍 7 处「待验证」）、`GAP-01` 的 `PIPE-*`/`RT-*` 落点、`03-roadmap` 状态节 | 未完成 | 已登记为 `G-M5-07..09`（见 §6）；`C3` 的升正式需要先补「四项待验证」的答案，不是改个标题 | `M5-manifest.json` 的 `open_gaps` |
| 7 | **`cargo fmt --all --check` 在收口时实际是红的**（`manifest_m5.rs` 从未格式化，8 处 diff） | 门禁自报与实际不符 | 收口期跑四条 cargo 时才发现；已 `cargo fmt --all` 修好并复跑（`fmt_check=0`、clippy 无 warning、测试仍 1608 绿）。**这条说明「manifest 里的 exit 0」不能代替自己跑一遍** | 本次提交（`fix(fmt)`） |

---

## 6. 缺口清单（`open_gaps`，逐条带「卡住谁」）

| ID | 缺口 | 卡住谁 | 去向 |
|---|---|---|---|
| `G-M5-01` | 常量溢出诊断缺失（`var y int8 = 200` 被接受并打印 `200`） | `T-EX-06` 的静态半边 | M4 的 `CH-2` 面（`M5-tests.md` 原文就标「M4 面」）；M5 只登记不修 |
| `G-M5-02` | 双包初始化顺序未兑现（`import _ "side"` 不跑 `side` 的 `init`） | `T-EX-40` / `EX-2` | 需要 HIR 的多包表示 + 初始化图 ⇒ **M6 的链接面** |
| `G-M5-03` | 泛型只到「`any` 约束 + 推断」 | `T-EX-32` / `EX-4` | 根因同 M4 的 `A3`（dump 根集不产实例）⇒ **M5 之后的检查器面** |
| `G-M5-04` | 泛型超面用通用 `CHK-*` 码拒绝，非专属 `HIR-UNSUPPORTED-GENERIC` | `T-EX-33` / `EX-4` | 要走到专属码，得先让 check 过（即 `G-M5-03`） |
| `G-M5-05` | M4 的 `CH-2` 收口时即未兑现，M5 未回退也未修 | `T-EX-62` / `EX-8` | M5 判据是「不回退」（1608 passed 证明没坏），那批红仍在 |
| `G-M5-06` | `R-6-2` 的 5 行追加排除 = 与冻结口径（排除 6）的偏离 | `T-EX-47` / `EX-6` | 已在本报告 §5 第 2 条认账 |
| `G-M5-07` | `C3-hir.md` 未升正式（§2 仍 7 处「待验证」） | Task 52 的文档收口 | 需先补四项答案（多返回值 / 可寻址性 / 闭包 / 异常传播的**已定形态**），`M5-design.md` §3.2 有素材 |
| `G-M5-08` | `GAP-01.md` 未补 `PIPE-10/14/15/16/16c/24` 与 `RT-*` 落点 | Task 52 的文档收口 | 纯登记工作，无技术风险 |
| `G-M5-09` | `03-roadmap.md` 的 M5 状态节未回写 | Task 52 的文档收口 | 与 `AGENTS.md` 一起改（约定：**只在阶段完成时**改；M5 是 `partial`，应写「已执行（部分兑现）」） |
| `G-M5-10` | Task 7 的 P0 变异（`T-EX-57..60` 各一次「改回原状」）没有留档 | `T-EX-57..60` 的反向证据 | 不影响 `EX-1..9` 的任何结论（P0 变异不计入门禁 8 条），但这四条修复目前**只有正向证据**；补做步骤见 `progress.md` §5 第 1 条 |

> **`G-M5-07..09` 是收口期补写的三份文档之外剩下的 Task 52 项**（`C3` 升正式 / `GAP-01` 落点 / 状态回写）。
> 它们**不影响**任何一条测试或门禁的结论，但会让「M5 的文档收口」看起来比实际完整 —— 所以单独登记。
> 本报告与 `C9` 的补写**不**抵扣这三条：写新文档 ≠ 回写旧文档。
>
> **`G-M5-10` 是「计划里有、执行时漏了」的那类缺口**：它不是文档问题，是**证据缺口**。
> 补做成本约 10 分钟（4 次变异 + 4 次复原），但需要严格走 `cargo clean -p` 的产物纪律。

---

## 7. 门禁（`EX-1..9`）

| 门禁 | 状态 | 证据（可复现） |
|---|---|---|
| `EX-1` HIR 与 lowering 闭环 | ⚠️ `partial` | `T-EX-01..05` 全绿（45 变体穷尽 / dump 确定性 / Place 五形态 / `HIR-UNSUPPORTED-*` 五条）；`T-EX-06` 只兑现「收敛无损」半边 ⇒ `G-M5-01` |
| `EX-2` 函数、帧与初始化 | ⚠️ `partial` | `T-EX-17..21`、`39` 全绿（`down(998)` 通过 ⇒ 语义不压 Rust 栈）；`T-EX-40` 未兑现 ⇒ `G-M5-02` |
| `EX-3` 复合值与内建 | ✅ `pass` | `T-EX-10..16`、`23/24` 全绿 + 语料 69/69 兜底 |
| `EX-4` 闭包与接口 | ⚠️ `partial` | `T-EX-25..31` 全绿（含 1.22 loopvar、typed nil、断言双值、type switch 四支）；`32/33` ⇒ `G-M5-03/04` |
| `EX-5` defer/panic/recover | ✅ `pass` | `T-EX-34..38` 全绿（注册时求值 + LIFO、recover 直接调用限制、命名结果改写、嵌套覆盖、未捕获 `panic:` + 退出码 2） |
| `EX-6` 顺序语料闭环 | ✅ `pass` | §4 全表；两向断言在 `run_corpus.rs` |
| `EX-7` 显式帧与资源上限 | ✅ `pass` | `T-EX-52`（三道上限各一条超限 fixture：退出码 2 + 稳定消息 + 宿主不 abort）+ `T-EX-17`（1000 层通过） |
| `EX-8` 四条 cargo + 既有自检 | ⚠️ `partial` | 四条 cargo 全 exit 0（1608 passed / 0 failed）—— **`fmt` 在收口时曾红，已修（§5 第 7 条）**；五条自检全过（m0 232 断言 / m1 / ast-golden / ast-testdata / ast-stdlib）；`T-EX-62` 只兑现「无回退」⇒ `G-M5-05` |
| `EX-9` 反向验证 + 交付 B | ✅ `pass` | `scripts/mutation-m5-notes.md`（8/8 实证变红并复原）+ 本报告 |

**`status` = `partial`，5 pass / 4 partial。** 与 `M5-manifest.json` 的 `phase_summary.gates_pass = 5` 一致。

---

## 8. 接手指南（给 M6 的人）

1. **先读三处**：本报告 §5（偏离）、§6（缺口）、`M5-manifest.json` 的 `open_gaps`。
2. **跑一遍再动手**：§9 的九条命令，全部在容器内（`R-9-1` 形态：`cd /work/rgoc`）。
3. **改动前必知的三条硬约束**：
   - `step_*` 不许递归（`D-M5-1`）—— 任何一处 `self.eval_expr(e)` 递归都会让 1000 层那条红，而 90% 的 fixture 仍绿；
   - lowering 不许自己推类型（`D-M5-2`）—— 拿不到类型先修检查器；
   - 数组/结构体的赋值必须 `deep_copy` —— 写成 `Rc` 共享只有 `a := arr; a[0]=1` 那一下会露（`T-EX-63` 的变异就是这条）。
4. **`divmod.go` 为什么排除**：不是慢，是**超限**（`--max-steps 1e9` 跑 98 s 仍不够；官方注释自述 ≈4e6 组用例）。提高上限等于取消上限（`T-EX-52` 会红）。
5. **`convert4.go` / `literal.go` 为什么排除**：值模型表达不了（`Ptr` 不能指子区间 / `Rc<str>` 不是字节串）。这两条是 `C9` 的**已知边界**，不是 bug。
6. **M6 之前最值钱的一步**：把 `G-M5-03`（泛型实例化）与 M4 的 `A3` 一起看 —— 它们是同一个根因（dump 根集不产实例），修一处两边都动。

---

## 9. 复现命令（全部容器内）

```bash
# ① 四条 cargo（`EX-8`）
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo fmt --all --check'
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo check --workspace --all-targets'
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo clippy --workspace --all-targets -- -D warnings'
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo test --workspace --no-fail-fast'

# ② 五条既有自检（`EX-8`）
python3 scripts/check-m0-consistency.py          # 232 条断言
python3 scripts/check-m1-consistency.py
bash scripts/check-ast-golden.sh
bash scripts/check-ast-testdata.sh
bash scripts/check-ast-stdlib.sh

# ③ 语料对撞（`EX-6`）
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work && bash tools/run-oracle/run-collide.sh --run --list tools/run-oracle/run-selected.txt'
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work && bash tools/run-oracle/run-collide.sh --run --list tools/run-oracle/run-ken.txt'

# ④ 三方差分（`T-EX-43`）
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work && bash tools/run-oracle/run-collide.sh --run --three-way tools/run-oracle/run-selected.txt'

# ⑤ 单文件冒烟
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && ./target/debug/rgoc-driver run ../go_source_code/test/helloworld.go'
```

**2026-10-23 收口期实测**：① 四条全 exit 0（150 suite / **1608 passed** / 0 failed / 1 ignored）；
② 五条全过；③ 69/69 与 32/32 全绿；④ 5 个 `.out` 三方一致；⑤ `hello, world` / exit 0。

---

## 10. 本报告与 `M4-report.md` 的关系

M4 的报告是「**前端子集矩阵**」（检查器能查出多少），本报告是「**顺序语义基线**」（跑起来对不对）。
两者的 `partial` 不是同一件事：

* M4 的 `CH-2` `fail` = **该拒的没拒**（检查器漏报）；
* M5 的四条 `partial` = **该做全的没做全**（溢出诊断、双包 init、泛型实例化、`C3` 回写）。

**不互相抵扣**：M5 没有回收 M4 的缺口（`M5-tests.md` §11.6），M4 的 `CH-2` 仍是 `fail`。
