# M4 实施计划：静态检查闭环

> **状态**：**待执行**（Task 1–44 待办；产出 2026-10-18）　|　**测试清单**：[`M4-tests.md`](./M4-tests.md)（已冻结）　|　**机器可读事实**：[`M4-manifest.json`](./M4-manifest.json)（Task 42 填实）
>
> **任务编号约定**：本计划用 **「Task N」（N=1..44）** 标识任务，**不与测试 ID `T-CK-*` 混用编号空间**。
> 执行期进度、偏离与 Ruling 记在 `.superpowers/sdd/M4-plan/progress.md`（不入库）。
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `rgoc` 能对一组 `.go` 文件给出**与 `go/types` 一致的静态检查结论** —— 判定五件套（可赋值/方法集/接口实现/约束满足/类型集同一性）、泛型实例化与推断、表达式与语句检查、内建与 `unsafe` 白名单、版本门控、包级规则（init 依赖/环、main/init、导出与 import 可见性）——并以「官方语料双腿对撞 + 自有 fixture」证明。

**Architecture:** `rgoc-types` 扩出判定核与泛型三件；新 crate `rgoc-check` 做检查器（操作数/表达式/语句/内建/init/包级），诊断码族 `CHK-` 与类别投影（`C8`）；`rgoc-driver` 加 `check` 子命令与 `types --cmp` 新判定式；`tools/types-oracle --check` 复刻官方 harness 语义产出同格式渲染；`run-collide.sh --check` 驱动逐文件两侧对撞。验收四腿：crate 单测（S）/ cmp 对撞（O）/ 语料对撞（O）/ 驱动端到端（S+H），外加变异反向验证（H）。

**Tech Stack:** Rust 1.98.1 / edition 2024 / resolver 3；workspace 在 `rgoc/`；全部命令经 `scripts/in-container.sh`（宿主不构建）。oracle 是 Go 程序，用容器内 go1.27.1（`GOPROXY=off` + `-buildvcs=false`）。

**Spec:** [`docs/milestones/M4-design.md`](./M4-design.md)（设计，已确认）＋ [`docs/milestones/M4-tests.md`](./M4-tests.md)（测试先行清单，已冻结）。**执行者必须同时读这两份** —— 本计划只决定「顺序与接口」，判据以 tests 清单为准。

## Global Constraints

- **只在容器内构建/测试**：一切 `cargo`/`go` 命令经 `scripts/in-container.sh <cmd>`。
- **不默认新增第三方依赖**：Rust 侧只用 std + workspace 既有依赖；oracle 只用 Go 标准库。
- **`go_source_code/` 只读**，不得修改、不得回写；语料读取失败**响亮**失败（`RGOC_CORPUS_SRC_DIR` 仅可覆盖根，不许静默跳过）。
- **诊断纪律**：用户可见的未实现 ⇒ 稳定 `DiagCode` + `unsupported` 登记；内部不可达才 `todo!`（`D-M3-11` 沿承）。
- **`DiagCode` 五件套**：枚举按字典序落位 + `ALL` 数组 + `as_str` + **两个总数测试** + 至少一条触发测试；**没有触发者的码不许进枚举**。
- **类别投影全量穷尽**：`rgoc-check::diag` 的 `category_of` 编译期 `match` 无通配；`other` 不出现在投影表（除修订）。
- **`C8` 字节格式**（Task 1 冻结）：`# source` / `# file` / `<行号>:<列>\t<类别>`；排序（行号,列,类别）；重复保留；列**不进硬判据**（提示层）。
- **对撞纪律**：两侧 stderr 必须空；重复运行确定；**不许**把「一侧没产出」当「一致」；「两侧都空」仅当腿①判 accept。
- **测试失败 → 先确认失败来自缺失行为**（不是环境/harness），再做最小实现；测试之前写的代码一律删除。
- **语法错误文件参与**（`R-6-1`）：解析错误进错误清单；`parse` 类别；恢复策略与官方对齐（`AllErrors`）。
- **产物陈旧陷阱**：变异前后 `cargo clean -p <被变异 crate>`；可疑时 `find rgoc/target -mindepth 1 -delete` 全量重建（约 4 min）。见 `scripts/mutation-m3-notes.md` §0。
- **资源上限**：类型/表达式递归深度 **1000**；import 深度 **100**；每文件检查超时 **10 s**；文件大小 64 MiB。
- 每个任务结束都**提交**；提交信息引用 Task 与 `T-CK-*`。

## Review Focus

以下是 `M4-design.md` 隐含、但**没有哪个任务的测试天然覆盖**的五类失效模式，最可能的排前面。每条都已把测试钉到拥有该代码的任务上：

1. **实例化的同一性记忆**：同一 `(origin, args)` 必须得到**同一实例 id**（`(origin,args)` memo）；漏记忆会让 dump/identity 随机漂 —— 这正是 M3 语料 15 例 DIFF 的根因区（Task 12/15，`T-CK-21/85`）。
2. **可赋值性的规则序**：先 `identical` 再「底层一致且至少一侧非具名」再 interface/chan/nil —— 顺序错在 `type T = int` 邻域翻车（Task 6，`T-CK-11`）。
3. **次级错误过滤（`": \t"`）两侧都要**：rgoc **内部保留原始链**（人读），渲染前过滤；少滤 ⇒ 多行、误滤 ⇒ 丢行（Task 20 render、Task 33 环、`T-CK-91`）。
4. **`-lang` 与 `//go:build` 的取源**：生效版本 = `max(fileVersion, go1.21)`，且 `-lang` 缺省时不门控；语料 `go1_19_20` 一族就是探针（Task 18，`T-CK-62/63`）。
5. **oracle importer 只认 `unsafe`**：其余 import **响亮失败**；把「无法解析」静默当作「无错误」是假绿之源 —— `map0.go` 的 could-not-import 是钉子（Task 3，`T-CK-91`）。

---

## 执行总览

| Phase | Task | 内容 |
|---|---|---|
| 0 | 1–5 | 契约与装置：`C8` 契约、`rgoc-check` 骨架 + `rgoc check` 占位、oracle `--check`、`run-collide.sh --check`、`check_corpus.rs` |
| 1 | 6–11 | 判定五件套（assignable / methodset / implements+satisfies / comparable+convertible / identical 升级）+ 类别投影骨架 |
| 2 | 12–19 | 泛型三件（instantiate / unify / infer / 环）+ cmp 新 kind + 版本门控（核心 / 同源 / soft） |
| 3 | 20–36 | `rgoc-check` 主体：会话/操作数、表达式、语句、内建、`unsafe`、方法集接线、init 序/环、main/init、imports/可见性、9 缺 fixture |
| 4 | 37–44 | 语料收口、wave A、M3 翻转、变异、manifest、全量回归、报告 |

---

## Phase 0：契约与装置（Task 1–5）

### Task 1: `C8` 检查诊断契约（冻结）

**Files:**
- Create: `docs/contracts/C8-check-diags.md`
- Create: `docs/contracts/testdata/check-diags-min.want`
- Modify: `docs/README.md`（契约清单登记：C8）

**Interfaces:**
- Consumes: `M4-tests.md` §4（类别 26 类）/ §5（双腿判据与渲染骨架）
- Produces: **字节格式**（对 Task 3/4/20 两侧共同约束）：头行 `# source: <rgoc|oracle>`、`# file: <相对路径>`（两侧）；`# markers: <N>`（oracle 侧独有，腿① 数据源）；渲染行 `<行号>:<列>\t<类别>`；排序（行号,列,类别）；重复行保留；列不进硬判据（提示层）；过滤规则（`": \t"` 次级剔除、解析错误参与、`AllErrors`）；确定性（重复运行逐字节同）

- [x] **Step 1: 写契约**，把 `M4-tests.md` §4/§5 的规则**原样**抽成独立契约（类别表 26 项、`other` 纪律、两侧映射机制、列-advisory 判据、退出码 0/1/2）
- [x] **Step 2: 写最小 fixture** `check-diags-min.want`（两行头 + 三条渲染行：同文件多错、同行同类别重复、含列字段），作为两侧实现的第一对撞目标
- [x] **Step 3: 登记** `docs/README.md` 契约清单（C8；「M4/Task 1 新建，已冻结」）；如既有自检断言契约**数量**，同步该断言
- [x] **Step 4: 校验** —— `python3 scripts/check-m0-consistency.py` / `check-m1-consistency.py` 期望 exit 0；文档链接自检无 BROKEN
- [x] **Step 5: 提交**

```bash
git add docs/contracts/C8-check-diags.md docs/contracts/testdata/check-diags-min.want docs/README.md
git commit -m "docs(m4): 新建并冻结 C8 检查诊断契约（T-CK-91..95 的对撞面）"
```

### Task 2: `rgoc-check` 骨架 + `rgoc check` 子命令占位

**Files:**
- Create: `rgoc/crates/rgoc-check/{Cargo.toml,src/lib.rs,src/diag.rs}`
- Modify: `rgoc/Cargo.toml`（members + workspace 依赖表）
- Modify: `rgoc/crates/rgoc-driver/src/cli.rs`、`src/main.rs`；Create `rgoc/crates/rgoc-driver/src/check_cmd.rs`
- Modify: `rgoc/crates/rgoc-lex/src/diag.rs`（新码 `CHK-UNIMPLEMENTED`，五件套全配）
- Test: `rgoc/crates/rgoc-driver/tests/test_driver.rs`

**Interfaces:**
- Produces: `rgoc_check::check_package(...) -> Result<CheckedPackage, DiagBag>`（占位：恒返回稳定诊断 `CHK-UNIMPLEMENTED`）；CLI `rgoc check [--diag-dump] [--lang go1.N] [--test-hooks] <files…>`（四个旗标**全部可解析**，行为=稳定失败退出码 1/2、**非 panic**）
- Consumes: `rgoc-load::load_with_imports`、`rgoc-lex::{DiagBag, DiagCode}`

- [x] **Step 1: 写失败测试**（`test_driver.rs`）：① `rgoc check` 在未实现时**退出码非 0**、stderr 含稳定文案（断言 `CHK-UNIMPLEMENTED` 字符串在）；② 非法旗标 ⇒ 用法错退出码 2；③ `--diag-dump --lang go1.22 --test-hooks` 组合可解析（不报用法错）
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh cargo test -p rgoc-driver check_ -v`；期望 FAIL（`check` 子命令不存在）
- [x] **Step 3: 实现骨架** —— crate 空壳（`pub fn check_package` 返回 `CHK-UNIMPLEMENTED`）；`DiagCode` 加 `ChkUnimplemented`（字典序落位 + `ALL` + `as_str` + 两个计数测试）；`cli.rs` 加分支与旗标解析
- [x] **Step 4: 全量不回归** —— `scripts/in-container.sh cargo test -p rgoc-driver --no-fail-fast`
- [x] **Step 5: 提交**

```bash
git add rgoc/Cargo.toml rgoc/crates/rgoc-check rgoc/crates/rgoc-driver rgoc/crates/rgoc-lex
git commit -m "feat(m4): rgoc-check 骨架 + rgoc check 子命令占位（TG-CHK-UNIMPLEMENTED）"
```

### Task 3: `tools/types-oracle --check`（官方语义复刻）

**Files:**
- Create: `tools/types-oracle/check.go`、`check_test.go`
- Modify: `tools/types-oracle/main.go`（模式分派加 `--check <files…>`）

**Interfaces:**
- Consumes: `docs/contracts/C8-check-diags.md`（Task 1）；官方 `go/types/check_test.go` 的 `parseFiles/parseFlags/commentMap/shouldTest` 语义（M4-design `F2`）
- Produces: `types-oracle --check [--test-hooks] <files…>` ⇒ stdout = C8 渲染；退出码 0/1/2。语义清单：① `parseFlags` 复刻**只读第一个文件（`srcs[0]`）首行**（`-lang` 提取；非空 `-goexperiment` 与 `-fakeImportC` 都要**响亮失败**，`U-M4-04/05`）；② …③ `parser.AllErrors|parser.SkipObjectResolution`，解析错误**参与**；④ importer **只认 `unsafe`**，其余失败（错误文本含 `could not import <path>`；包装细节以本侧输出为准）；⑤ 错误回调剔除 `": \t"` 次级；⑥ `shouldTest` 构建约束（`ReleaseTags`+GOOS/GOARCH）；⑦ marker 判定（前缀 `^ ERRORx? `；位置=前一 token）输出 `# markers: N`；⑧ `--test-hooks` ⇒ `types.DefPredeclaredTestFuncs()`（**仅**供 `check/` 前缀文件使用 —— 官方 `TestCheck` 注入而 `TestSpec/TestExamples` 不注入）；⑨ **Cheap-trick 抑制**由 `go/types` 内部生效（`errors.go:111-123`）——本侧免费继承，但 Step 1 要加**合成探针**锁行为

- [x] **Step 1: 写失败测试**（`check_test.go`，先 RED）：`parseFlags` 向量（lang 提取 / `-fakeImportC` 响亮失败 / goexperiment 响亮失败 / 无指令 ⇒ 不门控）；marker 计数向量（前缀正则、`ERRORx`、前一 token 位置、行首注释 ⇒ 0,0）；渲染排序与重复；**Cheap-trick 合成探针**（首错 + 随后含 `invalid operand`/`invalid type` 的错误 ⇒ 后者被抑制）
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh bash -lc 'cd tools/types-oracle && go test ./... -run Check'`；期望 FAIL（check.go 不存在）
- [x] **Step 3: 实现** —— 把 `.superpowers/sdd/M4-design/probe/m4probe.go`（已实测通过）的语义**移植**进 `check.go`（单文件/多文件同包；目录输入取顶层 `.go`）；适配表骨架（消息 → 类别，起步覆盖 F1/语料常见族；`other` 命中时**打印告警行到 stderr**，正式守卫在 Task 4）
- [x] **Step 4: 手动冒烟** —— 对 `check/literals.go`（`--test-hooks`）期望 0 错、`# markers: 0`；对 `check/go1_12.go` 期望 14 错、`# markers: 14`；对 `spec/conversions.go` 期望非空且无 `other`
- [x] **Step 5: 提交**

```bash
git add tools/types-oracle
git commit -m "feat(m4): types-oracle --check（官方 harness 语义复刻 + C8 渲染）"
```

### Task 4: `run-collide.sh --check [--list]` + 首跑（RED 证据）

**Files:**
- Modify: `tools/types-oracle/run-collide.sh`

**Interfaces:**
- Consumes: Task 2（`rgoc check --diag-dump`）、Task 3（`--check`）、`check-selected.txt`
- Produces: `run-collide.sh --check [--list <清单>] <testdata 根>` —— 逐文件：oracle 渲染（`check/` 前缀加 `--test-hooks`）vs `rgoc check --diag-dump`；硬判据=剥列后（行号,类别）多重集；列差异**计数并提示**；两侧 stderr 空；重复运行确定；`other` 出现在入选渲染中 ⇒ 红；清单行不在语料 ⇒ exit 2；首个不一致 ⇒ 打印现场 exit 1

- [x] **Step 1: 写脚本**（照 M3 `--corpus` 形态：不自己重算筛法；清单为唯一输入；temp 文件逐文件清理）
- [x] **Step 2: 首次运行（预期响亮红）** —— `scripts/in-container.sh bash tools/types-oracle/run-collide.sh --check go_source_code/src/internal/types/testdata`；期望输出「rgoc 侧未实现（CHK-UNIMPLEMENTED）/ 首个不一致」并 exit 1（**保存输出节选，作为 RED 证据入** `.superpowers/sdd/M4-plan/progress.md`）
- [x] **Step 3: 非入选守卫** —— `--list` 指向一个含不存在路径的临时清单 ⇒ exit 2
- [x] **Step 4: 提交**

```bash
git add tools/types-oracle/run-collide.sh
git commit -m "feat(m4): run-collide.sh --check [--list]（双腿对撞形态）"
```

### Task 5: `check_corpus.rs`（两清单守卫）+ B7 反向验证

**Files:**
- Create: `rgoc/crates/rgoc-driver/tests/check_corpus.rs`
- Modify: `rgoc/crates/rgoc-driver/Cargo.toml`（如测试需要 dev-dep，实则无需）

**Interfaces:**
- Consumes: `check-selected.txt` / `check-allowlist.txt` / `check-fixedbugs-waveA.txt`；语料根解析沿 `collide_corpus.rs`（`RGOC_CORPUS_SRC_DIR` 可覆盖；读不到**响亮失败**）
- Produces: Rust 侧断言（`T-CK-93` / `T-CK-A1`）：① 候选（三目录顶层 `.go`）机械重算 == 入选 ∪ 排除（两向、无重复、无死行）；② 每行存在；③ 排除理由非空且含指针（`D-M4-`/`U-M3-`/`T-CK-`/`CH-`）；④ wave A 50 行、4 组注释、全部存在且**不**与三目录候选重复；⑤ 四个目录包（`check/decls2/` 等）存在但不在候选

- [x] **Step 1: 写测试**（先 RED：故意断言候选==91 与两向相等；若初始即绿，用「临时改一行清单」制造 RED 再复原）
- [x] **Step 2: 运行** —— `scripts/in-container.sh cargo test -p rgoc-driver --test check_corpus -v`
- [x] **Step 3: 反向验证（`B7` 第一半）** —— 从 `check-selected.txt` 移走一行 ⇒ 期望该测试**变红**（打印两向差集）；复原。记录命令与输出摘录入 progress
- [x] **Step 4: 提交**

```bash
git add rgoc/crates/rgoc-driver/tests/check_corpus.rs
git commit -m "test(m4): check_corpus 两清单守卫（T-CK-93/A1）"
```

---

## Phase 1：判定五件套（Task 6–11）

### Task 6: `assignable.rs`（可赋值性）

**Files:**
- Create: `rgoc/crates/rgoc-types/src/assignable.rs`
- Modify: `rgoc/crates/rgoc-types/src/lib.rs`（模块与导出）
- Test: `rgoc/crates/rgoc-types/tests/assignable.rs`

**Interfaces:**
- Produces: `pub fn assignable(t: &TypeTable, v: TypeId, tgt: TypeId) -> Result<bool, DiagCode>`（`Ok(false)` = 明确不可赋值；`Err` = 显式拒绝面，沿 `identical` 习惯）
- 规则序（`go/types` `operand.go::assignableTo`）：identical → 类型参数面（1.18 规则：`T` 与 `~T` 项存在）→ 底层同一且**至少一侧非具名**→ `implements(tgt)` → chan 方向（V/V 双向、具名通道对裸通道单向）→ `nil` 面
- Consumes: `identical` / `underlying` / `implements`（Task 8 前先留 `todo!` 路径**不可达**，或先本地窄实现 —— 二选一由 Step 3 决定并记 Ruling）

- [x] **Step 1: 写失败测试**（`T-CK-11` 底座）：每族 ≥2 正例 + ≥2 反例（`spec/assignability.go`、`examples/*` 取型；至少含 `type T = int` 别名、具名/非具名底层、chan 方向、`nil`）
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh cargo test -p rgoc-types --test assignable`
- [x] **Step 3: 实现**（严格按规则序；对暂不可判形态返回 `Err(DiagCode::Typ…)` **新码/既有码**，不许答错）
- [x] **Step 4: 全量不回归** —— `scripts/in-container.sh cargo test -p rgoc-types --no-fail-fast`
- [x] **Step 5: 提交**

### Task 7: `methodset.rs`（方法集）

**Files:**
- Create: `rgoc/crates/rgoc-types/src/methodset.rs`；Modify `lib.rs`
- Test: `rgoc/crates/rgoc-types/tests/methodset.rs`

**Interfaces:**
- Produces: `pub struct MSetEntry { pub name: String, pub sig: SignatureId }`；`pub fn methodset(t: &TypeTable, typ: TypeId) -> Result<Vec<MSetEntry>, DiagCode>`（按名字排序；嵌入提升取**深度最小者**；同级歧义名**排除**；`*T` 规则照 `go/types/methodset.go`）
- Consumes: `TypeTable` 的 `NamedDef`/`StructDef`/`InterfaceDef` 表示（M3）+ `unalias`

- [x] **Step 1: 写失败测试**（`T-CK-14` 底座）：值/指针接收者、嵌入 1 层与 2 层提升、歧义排除、接口方法集、`*T` vs `T`
- [x] **Step 2: 运行确认失败** —— `cargo test -p rgoc-types --test methodset`
- [x] **Step 3: 实现**
- [x] **Step 4: 全量不回归**
- [x] **Step 5: 提交**

### Task 8: `implements.rs`（接口实现 + 约束满足）

**Files:**
- Create: `rgoc/crates/rgoc-types/src/implements.rs`；Modify `lib.rs`
- Test: `rgoc/crates/rgoc-types/tests/implements.rs`

**Interfaces:**
- Produces: `pub fn implements(t: &TypeTable, v: TypeId, iface: TypeId, constraint: bool) -> Result<bool, DiagCode>`
  - `constraint=false`：非约束接口实现（方法集包含；空接口恒真；`*I` 面照官方）
  - `constraint=true`：类型集包含（`V ∈ C`：空集恒满足；`~T` 项、并集、`comparable` 标志面）；供 `satisfies` 与实例化校验共用
- Consumes: `methodset`（Task 7）、`identical`（Task 10 升级后）

- [x] **Step 1: 写失败测试**（`T-CK-12/13` 底座）：方法缺失/多余、空接口、`~int`、并集、`comparable`、空类型集边界
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现**（`constraint` 双态一核两径，共享方法集比较）
- [x] **Step 4: 全量不回归**
- [x] **Step 5: 提交**

### Task 9: `comparable.rs` + `convertible.rs`（可比较 / 可转换静态面）

**Files:**
- Create: `rgoc/crates/rgoc-types/src/comparable.rs`、`src/convertible.rs`；Modify `lib.rs`
- Test: `rgoc/crates/rgoc-types/tests/comparable.rs`、`tests/convertible.rs`

**Interfaces:**
- Produces: `pub fn comparable(t: &TypeTable, typ: TypeId) -> Result<bool, DiagCode>`；`pub fn convertible(t: &TypeTable, v: TypeId, tgt: TypeId) -> Result<bool, DiagCode>`（静态类型对；**常量**特例面留给检查器 Task 22）
- 规则序照 `go/types/predicates.go::comparable` 与 `conversions.go`

- [x] **Step 1: 写失败测试**（`T-CK-32/33` 底座）：struct/array/接口/类型参数 comparable 面；`[]byte↔string`、数值互转、通道方向、指针↔uintptr 拒绝面
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现**
- [x] **Step 4: 全量不回归**
- [x] **Step 5: 提交**

### Task 10: `identical` 升级 —— 类型集版 interface 同一性（**解除 `U-M3-05`**）

**Files:**
- Modify: `rgoc/crates/rgoc-types/src/identical.rs`（删拒点、实现 terms 归一 + comparable 标志 + 方法集比较；`TypeParam`/`Union` 独立形态照 `go/types/predicates.go` 语义）
- Modify: `rgoc/crates/rgoc-lex/src/diag.rs`（**移除** `TypUnsupportedConstraintIdentity` 变体 + 计数测试同步；如 manifest_m3 相关断言受影响，一并最小同步 —— 正式文档修订在 Task 40）
- Modify: `tools/types-oracle/testdata/cmp-basic.go`（3 条 `// rgoc:expect-reject U-M3-05` 行改写）+ `cmp-basic.want` + `rgoc/crates/rgoc-driver/tests/collide_cmp.rs`（排除集硬编码断言同步改写）——`D-M4-14` 的 `U-M3-05` 牵连条目
- Modify: `rgoc/crates/rgoc-types/tests/identical.rs`（原拒面用例改写为**判定断言** + 合成反例）
- Test: `rgoc/crates/rgoc-driver/tests/types_e2e.rs`（`T-TY-81` 定向改写：原「拒」断言 ⇒ 「两侧都作答」）

**Interfaces:**
- Produces: `identical` 对「两侧都是匿名约束接口（至少一侧含项集或 `comparable`）」给出判定；具名约束接口沿具名分支
- **同步义务（`D-M4-11` 第一刀）**：代码与测试**原子**改（含 `D-M4-14` 牵连条目）；`M3-tests.md` 修订行与 `M3-manifest.json` 的 `resolved_by` 落档在 Task 40（此处只登记在 progress）

- [x] **Step 1: 改失败测试为先**：把原「期望 Err」用例改「期望 Ok(true/false)」（含 `interface{ ~int | ~string }` 置换序相等、`comparable` 差异不等 2 条合成反例）
- [x] **Step 2: 运行确认失败**（红在实现仍早退 Err）
- [x] **Step 3: 实现** + 移除死码 + 全量不回归（`cargo test --workspace --no-fail-fast`；`manifest_m3.rs` 若红⇒当场最小修）
- [x] **Step 4: B 类反向抽验**：手动把 terms 比较改成恒 true ⇒ 期望合成反例变红；复原
- [x] **Step 5: 提交**

### Task 11: 类别投影骨架 `rgoc-check::diag`（`T-CK-16` 起步）

**Files:**
- Create/Modify: `rgoc/crates/rgoc-check/src/diag.rs`（`pub enum Category`（26 类）+ `pub fn category_of(code: DiagCode) -> Category` **穷尽 match 无通配**）
- Test: `rgoc/crates/rgoc-check/tests/category.rs`（枚举成员断言 26 + `other` 不在映射值域 + 抽样码映射表）

**Interfaces:**
- Consumes: `rgoc-lex::DiagCode` 全枚举（含 M3 `Typ*` 全部 + M4 `Chk*` 生长中）
- Produces: 投影函数；`Category::Other` 存在但**无码映射到它**（编译期不允许；由测试断言 + 后续 `T-CK-94` 语料守卫）

- [x] **Step 1: 写失败测试**（26 类枚举、抽样 12 个码的期望类别、`ALL_CATEGORIES` 计数）
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现**（对每个既有 `TYP-` 码定类；新增 `CHK-` 码在各自任务落位时**必须**同步此 match —— 编译不过即提醒）
- [x] **Step 4: 提交**

---

## Phase 2：泛型三件 + 版本门控（Task 12–19）

### Task 12: `instantiate.rs` + `subst.rs`（实例化 + 替换）

**Files:**
- Create: `rgoc/crates/rgoc-types/src/instantiate.rs`、`src/subst.rs`；Modify `lib.rs`、`src/typ.rs`/`src/table.rs`（`Type::Named` 携带**实例化实参**；具体形态在此任务落定，若触发 `C7` 渲染变化走 `D-M4-9` 修订路径并记 progress）
- Test: `rgoc/crates/rgoc-types/tests/instantiate.rs`

**Interfaces:**
- Produces: `pub fn instantiate(t: &mut TypeTable, origin: NamedId, args: &[TypeId]) -> Result<TypeId, DiagCode>`（实参数量校验 + `implements/satisfies` 校验；**按 (origin, args) memo**，同一实例化 ⇒ 同一 id）；`pub fn substitute(...)`（类型参数替换）
- Consumes: Task 8/10

- [x] **Step 1: 写失败测试**（`T-CK-21` 底座）：等价实参不同写法 ⇒ 同 id；不同实参 ⇒ 不同 id；数量错 ⇒ 稳定拒绝；约束不满足 ⇒ 稳定拒绝
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现**；跑 `cargo test -p rgoc-types --no-fail-fast`；再跑 `--dump` 对撞（`collide_dump.rs`）确认**不回归**
- [x] **Step 4: 提交**

### Task 13: `unify.rs`（统一，三模式）

**Files:**
- Create: `rgoc/crates/rgoc-types/src/unify.rs`；Test: `tests/unify.rs`

**Interfaces:**
- Produces: `pub enum UnifyMode { Exact, Loose, Assign }`；`pub fn unify(t, mode, x: TypeId, y: TypeId, infer: &mut Vec<(TypeParamId, TypeId)>) -> Result<bool, DiagCode>`（照 `go/types/unify.go` 结构递归：类型参数绑定、项集交集、通道方向、签名、接口）
- Consumes: Task 8/10/12

- [x] **Step 1: 写失败测试**（`T-CK-24` 底座）：exact/loose/assign 三模式差异向量；嵌套泛型；`~T` 项集交集
- [x] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 14: `infer.rs`（推断三入口）

**Files:**
- Create: `rgoc/crates/rgoc-types/src/infer.rs`；Test: `tests/infer.rs`

**Interfaces:**
- Produces: ① `infer_from_args`（1.18 实参模式）；② 隐式实例化于**实参位置**与**赋值/接口方法语境**（1.21）；③ 赋值上下文**函数类型**推断（1.27）；失败 ⇒ `Err`/`Ok(None)` + 稳定码
- Consumes: Task 13

- [x] **Step 1: 写失败测试**（`T-CK-22/23/24` 底座）：三入口各 ≥4 向量 + 失败形态（不能推断 ⇒ 稳定码）
- [x] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 15: cmp 新判定式（两侧）—— `T-CK-81..85`

**Files:**
- Modify: `tools/types-oracle/cmp.go`、`cmp_test.go`（kind 分派：`assignable`/`implements`/`satisfies`/`methodset`/`instance`；未知 kind 仍拒）
- Modify: `rgoc/crates/rgoc-driver/src/cmp_cmd.rs`、`types_cmd.rs`；fixture：`tools/types-oracle/testdata/cmp-*.go` 增行
- 语法/输出扩展（一元 `methodset`、`instance` 实参列表、非 `true/false/reject` 结果形态）：同批备好 `M3-tests.md` §5.2 修订 `R-32-2` 条文（登记在 progress；落档在 Task 40）
- Test: `rgoc/crates/rgoc-driver/tests/collide_cmp.rs`（自动覆盖新 fixture）+ 重写 `T-TY-82`（新 kind 作答、未知 kind 拒 —— **`D-M4-11` 第二刀**）

**Interfaces:**
- Consumes: Task 6/7/8/12（判定实现）
- Produces: 五种 kind 的**双侧一致**输出（`true/false/reject`；`methodset`=名字序列；`instance`=C7 单根 dump / `reject:<category>`）

- [x] **Step 1: fixture 先行**：写 `cmp-*` 行（每 kind ≥6 行，含 reject 行）
- [x] **Step 2: RED** —— `scripts/in-container.sh bash tools/types-oracle/run-collide.sh --cmp tools/types-oracle/testdata`；期望未知 kind / 缺实现红
- [x] **Step 3: 实现两侧**（oracle 侧最简：直接用 `go/types` 对应 API；driver 侧接线 Task 6–12 的判定）
- [x] **Step 4: 绿 + 全量不回归**（含 `collide_cmp.rs`）
- [x] **Step 5: 提交**

### Task 16: 实例环与拒绝面（`T-CK-25`）

**Files:**
- Create/Modify: `rgoc/crates/rgoc-types/src/instantiate.rs`（环检测）；Test: `tests/instantiate_cycles.rs`

**Interfaces:**
- Produces: 实例化过程的**环检测**（`F1[*]` 类自引用链）+ 已知边界形态的**显式拒绝**（`U-M4-09` 登记）；语料 `typeinstcycles.go` / `fixedbugs` G1 族将有对照

- [x] **Step 1: 写失败测试**（环 ⇒ 稳定拒绝非 panic；样例取 `check/typeinstcycles.go` 邻域）
- [x] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 17: 版本门控核心（`verify_version` 表）

**Files:**
- Create/Modify: `rgoc/crates/rgoc-check/src/version.rs`（表驱动：特性 → `go1.N` 阈值 + 报错文案形态）+ 必要时 `rgoc-types/src/version.rs` 扩 API
- Test: `rgoc/crates/rgoc-check/tests/version_table.rs`

**Interfaces:**
- Consumes: `rgoc-types::version`（M3 既有 `Versions::require` / `pkg_versions`）
- Produces: `verify_version(cx, at: Pos, feature: Feat, ...)` 全表（`F1`：数值字面量/别名词法/移位/切片→数组指针/`~T` 等）；表驱动 fixture 每特性低/高两档（`T-CK-61` 起步）

- [x] **Step 1: 写失败测试**（≥12 特性 × 低/高两档；期望类别恒 `version`）
- [x] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 18: `-lang` / `//go:build` 同源进入（`T-CK-62/63` 接线）

**Files:**
- Modify: `rgoc/crates/rgoc-driver/src/check_cmd.rs`（首行 `// -lang=` 解析：**harness 语义**，与 oracle `parseFlags` 同规则；四字符指令外的内容忽略）
- Modify: `rgoc/crates/rgoc-check/src/check.rs`（per-file 生效版本 = `max(fileVersion, go1.21)`，沿 M3 `pkg_versions`；`-lang` 缺省 ⇒ 不门控）
- Test: `rgoc/crates/rgoc-check/tests/version_source.rs` + 语料冒烟

**Interfaces:**
- Consumes: M2 `go_version_from_build`、M3 `Versions`
- Produces: 语料 `go1_*` 文件的判定在 `rgoc check` 侧**可重放**（本 Task 只接线；全绿收口在 Task 37）

- [x] **Step 1: 写失败测试**：向量 `go1_19_20`（max⇒1.21 不门控）/ `go1_22_21`（1.22 门控 range）/ `go1_xx_19`（只有 build 版本）/ `decls0`（1.17）
- [x] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 19: 门控位置与 soft 面（`T-CK-64`）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/version.rs`、`src/diag.rs`（soft 错误内部标记：**保留**在原始流，渲染层照 `C8` 判据输出）
- Test: `rgoc/crates/rgoc-check/tests/version_pos.rs`

**Interfaces:**
- Produces: 门控错误的**行/列来源** = 触发点（range 的 `range` 关键字、new 的实参……）；soft 面与官方一致（不因 soft 提前丢）
- Consumes: Task 17/18

- [x] **Step 1: 写失败测试**：`go1_25.go` 的 4 条 new() 门控行号向量；`go1_22_21` 的 1 条 range 门控
- [x] **Step 2..5: RED → 实现 → 不回归 → 提交**

## Phase 3：`rgoc-check` 主体（Task 20–36）

### Task 20: 检查会话骨架 + `operand.rs` + `render.rs`（C8 对齐）

**Files:**
- Modify/Create: `rgoc/crates/rgoc-check/src/lib.rs`、`check.rs`（会话/顺序/延迟检查）、`operand.rs`（`invalid / novalue / builtin / typexpr / constant_ / variable / mapindex / value（含 nilvalue）/ commaok / commaerr`，照 `operand.go:24-37`）、`render.rs`（`--diag-dump` 字节格式=C8）
- Modify: `rgoc/crates/rgoc-driver/src/check_cmd.rs`（打通 `--diag-dump` 输出）
- Test: `rgoc/crates/rgoc-check/tests/session.rs`、`tests/render.rs`

**Interfaces:**
- Consumes: `rgoc-load::Package`（作用域/对象/常量值）、Task 6–19
- Produces: `check_package` 真身（返回 `CheckedPackage { 诊断流（含原始次级链）、渲染入口 }`）；`render.rs` 输出**与 `check-diags-min.want` 逐字节一致**；诊断**上报层**实现 Cheap-trick 抑制同规则（`F2`⑧）
- **值层接线的落点**：M3 `open_gaps`「值层没有类型接线」（`-uint32(5)` 漏报根因）在本任务落

- [x] **Step 1: 写失败测试**：`render.rs` 对 min fixture 逐字节；`operand` 模式向量 ≥12（常量/变量/类型/nil/内建名）
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现**（诊断流**内部保留**含 `": \t"` 的原始条目；渲染层过滤——见 Review Focus #3）
- [x] **Step 4: 冒烟** —— `rgoc check --diag-dump` 对 `check/go1_12.go` 输出 vs oracle 手工对比
- [x] **Step 5: 提交**

### Task 21: `expr.rs` 运算符与表达式（`T-CK-32`）

**Files:**
- Create: `rgoc/crates/rgoc-check/src/expr.rs`（一元/二元/比较/逻辑/位运算/移位）；Test: `tests/expr_ops.rs`

**Interfaces:**
- Consumes: Task 9（`comparable/convertible`）、Task 20
- Produces: 运算符静态规则诊断（`op-invalid` / `shift` / `overflow` 类）；**移位计数**与常量面

- [x] **Step 1: 写失败测试**：每运算符族 ≥3 接受 + ≥3 拒绝（样本取自 `check/expr*.go` 与 `spec/*`）
- [x] **Step 2..5: RED → 实现 → 全量不回归 → 提交**

### Task 22: 转换接线（`T-CK-33`）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/expr.rs`（转换表达式 + 常量特例面）；Test: `tests/conversions.rs`

**Interfaces:**
- Consumes: Task 9 `convertible` + `rgoc-const` 可表示性（M3）
- Produces: 转换诊断（`convert` 类；常量边界）

- [x] **Step 1: 写失败测试**（`spec/conversions.go` 邻域样本 ≥10）：数值/字符串/指针/常量边界
- [x] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 23: 索引与切片（`T-CK-34` 前半）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/expr.rs`（索引：数组/切片/字符串/map 双值/泛型实参二义消解；切片：3-index）；Test: `tests/index_slice.rs`

- [x] **Step 1: 写失败测试**：map 双值、字符串 rune 面、3-index 约束、负常量索引
- [x] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 24: 选择器 / 字段提升 / 方法值（`T-CK-34` 后半、`T-CK-51/52` 接线）

**Files:**
- Create: `rgoc/crates/rgoc-check/src/lookup.rs`、`src/selection.rs`；Test: `tests/selection.rs`

**Interfaces:**
- Consumes: Task 7 `methodset` + M3 `Scope/ScopeRef`
- Produces: 选择器诊断（`selector` 类）；深度最小者胜 / 同级歧义错 / 方法值与方法表达式形态

- [x] **Step 1: 写失败测试**（G4 点名组之一）：提升 2 层、歧义、方法值 `x.M`、方法表达式 `T.M`、指针自动解引用
- [x] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 25: `stmt.rs` 语句静态规则（`T-CK-35`）

**Files:**
- Create: `rgoc/crates/rgoc-check/src/stmt.rs`；Test: `tests/stmt.rs`

**Interfaces:**
- Produces: 赋值/复合赋值、短变量声明、if/for/switch/type-switch/select 合法性、go/defer 约束、return 规则、fallthrough 规则、**终止语句**（`terminate` 类）、break/continue/goto 标签面（沿 M3）

- [x] **Step 1: 写失败测试**：每族 ≥2 接受 + ≥2 拒绝（样本 `check/stmt0/1.go`、`check/gotos.go`、`check/labels.go`）
- [x] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 26: `range` 静态形态（`T-CK-36`）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/stmt.rs`（range：数组/切片/字符串/map/chan/整型 1.22/func 1.23）；Test: `tests/range.rs`

**Interfaces:**
- Consumes: Task 17/19（门控挂点）
- Produces: range 诊断（`range`/`version` 类）；迭代变量型与个数

- [ ] **Step 1: 写失败测试**（`spec/range.go`、`spec/range_int.go`、`go1_22_21.go` 邻域）
- [ ] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 27: 复合字面量（`T-CK-37`）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/expr.rs`（复合字面量 + 嵌套 key 1.27 门控）；Test: `tests/complit.rs`

- [ ] **Step 1: 写失败测试**（`check/compliterals.go` 邻域；嵌套 key 正反两档）
- [ ] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 28: 类型断言静态约束（`T-CK-38`）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/expr.rs`（`v.(T)`、`v, ok`）；Test: `tests/assert.rs`

- [ ] **Step 1: 写失败测试**：非接口操作数、P 方法冲突（同时含 `T(*T)` 方法面）、`ok` 形态
- [ ] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 29: `builtins.rs`（内建签名与静态约束）（`T-CK-41/42`）

**Files:**
- Create: `rgoc/crates/rgoc-check/src/builtins.rs`；Test: `tests/builtins.rs`

**Interfaces:**
- Produces: 18 个预声明内建的签名与静态约束（`append` 元素可赋值、`close/delete/copy`、`make/new` 形态含 1.26 门控挂点、`clear/min/max`）；`assert/trace` 仅 `--test-hooks` 下注入（`F13`）

- [ ] **Step 1: 写失败测试**（`check/builtins1.go` 邻域精选 ≥12 向量）
- [ ] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 30: 常量折叠接线（`T-CK-43`，**解除 `U-M3-08`**）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/builtins.rs`（`len/cap/Sizeof/Alignof/Offsetof/min/max` 常量结果）；必要时 `rgoc-load/src/decls.rs` 注解
- Test: `tests/const_fold.rs`

**Interfaces:**
- Consumes: M3 `rgoc-const`（任意精度）
- Produces: `const x = len("abc")` 一类**在 `rgoc check` 面合法**；`rgoc types`（load 面）行为**不改**（`T-TY-83` 加注记在 Task 40）

- [ ] **Step 1: 写失败测试**：`len/cap/Sizeof` 常量与非常量两档、`U-M3-08` 解除断言
- [ ] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 31: `unsafe_pkg.rs`（白名单 + sizes）（`T-CK-44`）

**Files:**
- Create: `rgoc/crates/rgoc-check/src/unsafe_pkg.rs`；Test: `tests/unsafe_pkg.rs`

**Interfaces:**
- Produces: `unsafe.Sizeof/Alignof/Offsetof` + `unsafe.Pointer` 类型的白名单行为（`StdSizes{8,8}`）；**白名单外成员显式拒绝**（`unsafe` 类码；`U-M4-06` 守卫）

- [ ] **Step 1: 写失败测试**：`check/typeinstcycles.go` 的 `unsafe.Sizeof(F1[int])` 接受；`unsafe.Add` 拒绝
- [ ] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 32: `initorder.rs`（初始化依赖图与顺序）（`T-CK-71`）

**Files:**
- Create: `rgoc/crates/rgoc-check/src/initorder.rs`；Test: `tests/initorder.rs`

**Interfaces:**
- Produces: 包级 var/const 依赖收集（表达式引用）+ 拓扑序 ⇒ `InitOrder`（与 oracle `types.Info.InitOrder` 对撞：本任务同时把 `--check` oracle 侧/渲染的 init 面对齐纳入 `run-collide --check` 之外的单列 fixture —— 自有 fixture + oracle 对撞脚本复用 `--check`）

- [ ] **Step 1: 写失败测试**（`check/init0/1/2.go` 邻域）：依赖链、跨文件、函数体引用不计入
- [ ] **Step 2..5: RED → 实现 → 对撞 → 提交**

### Task 33: 初始化环诊断（`T-CK-72`）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/initorder.rs`（环 + **原始次级链**）；Test: `tests/init_cycles.rs`

**Interfaces:**
- Produces: 环上每个变量各一条主诊断 + 次级链（渲染层照 `": \t"` 过滤）

- [ ] **Step 1: 写失败测试**（`check/init*`、`cycle` 族样本；环主诊断行号向量）
- [ ] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 34: `main.rs`（main/init 约束）（`T-CK-73`）

**Files:**
- Create: `rgoc/crates/rgoc-check/src/main.rs`；Test: `tests/main_init.rs`

**Interfaces:**
- Produces: `func init` 无参无结果有体（soft 照官方）；`func main` 在 `package main` 的签名/引用约束（**精确面以 RED 阶段实测钉死**，记 Ruling）；包名/导入自身限制接线

- [ ] **Step 1: 写失败测试**（`check/main0/1.go` 邻域精选）
- [ ] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 35: `imports.rs`（三形态 import + 导出可见性）（`T-CK-74/75`）

**Files:**
- Create: `rgoc/crates/rgoc-check/src/imports.rs`；Modify: `rgoc/crates/rgoc-load/src/importer.rs`、`src/export_summary.rs`（跨包对象比对、`ImportName` 规则补齐）
- Test: `tests/imports.rs`、`rgoc/crates/rgoc-load/tests/*`（增量）

**Interfaces:**
- Produces: `.`/`_`/别名 import 名字进入规则（**G4 点名组**）；`pkg.X` 导出性（Unicode 大写）+ 跨包对象比对；`UndeclaredImportedName` 细分码；`-I` 根指错与不支持形式的诊断拆分

- [ ] **Step 1: 写失败测试**（三形态 + 未导出 + 同名冲突 + 循环 import 面）
- [ ] **Step 2..5: RED → 实现 → 不回归 → 提交**

### Task 36: 9 缺测试项 M4 段 fixture（`T-CK-76`）

**Files:**
- Create: `rgoc/crates/rgoc-check/tests/gaps_fixtures.rs`、`rgoc/crates/rgoc-load/tests/*`（增量）
- 触及：`TYP-26`（泛型实例化——本阶段判定面）、`EXP-16`/`EXP-22`（静态面）、`PKG-04`（零值/类型完备性谓词）、`PKG-06`

**Interfaces:**
- Produces: 5 项各 ≥1 条自有断言；`RT-*` 三项仍留（`M4-tests.md` §2.3）

- [ ] **Step 1: 写失败测试**
- [ ] **Step 2..5: RED → 实现 → 不回归 → 提交**

---

## Phase 4：收口（Task 37–44）

### Task 37: 语料 75 全绿收口（`T-CK-91/92/94/95`、`T-CK-63` 收口）

**Files:**
- 迭代修改：`rgoc-check` / `rgoc-types` / oracle 适配表（`check.go`）；清单如动走修订纪律
- Test: `tools/types-oracle/run-collide.sh --check`（门禁本体）

- [ ] **Step 1: 全量跑** —— `scripts/in-container.sh bash tools/types-oracle/run-collide.sh --check go_source_code/src/internal/types/testdata`，按首个不一致逐文件修（每轮记录 progress）
- [ ] **Step 2: 收束纪律** —— 系统性差异且根因属官方实现细节/未支持面 ⇒ 按 `D-M4-3`「移出」：逐行理由 + `M4-tests.md` §12 修订行；**不许**放宽判据换绿（`R-6-1` 的 parse 行对齐风险优先排查）
- [ ] **Step 3: 确定性**（`T-CK-92`）：两侧各自重复运行 sha256 一致；`other` 守卫（`T-CK-94`）零命中
- [ ] **Step 4: 全绿存档** —— 把最终 `--check` 全绿输出节选（含计数）记入 progress；`T-CK-63` 的 18+1 文件判定逐条勾核
- [ ] **Step 5: 提交**

### Task 38: fixedbugs wave A 对撞（`T-CK-A1/A2`）

**Files:**
- Test: `tools/types-oracle/run-collide.sh --check --list tools/types-oracle/check-fixedbugs-waveA.txt …`

- [ ] **Step 1: 全量跑** —— 50 文件同 Task 37 纪律
- [ ] **Step 2: 预算不足 ⇒ 显式降级**：wave A 是**先砍**项；砍必须写 `M4-report.md`（哪些文件、为什么、去向 wave B）——不许静默
- [ ] **Step 3: 提交**

### Task 39: `A3` —— M3 15 例 DIFF / 10 例拒绝的根因复核

**Files:**
- Create/Modify: `docs/milestones/M4-report.md` 的「M3 欠账复核」章（Task 44 汇总前先落此内容）

- [ ] **Step 1: 逐例复核**（`tools/types-oracle/corpus-allowlist.txt` 的 26 行）：实例化落地后每例给结论 ∈ {已可 dump 对撞 ⇒ 建议 M3 修订提升；仍属 C7 渲染/命名面；登记去向}；**不要求本阶段修改 M3 清单**
- [ ] **Step 2: 记录**入 `M4-report.md` 草稿（表格：文件 / 原理由 / 新结论 / 依据命令）
- [ ] **Step 3: 提交**

### Task 40: `T-CK-E5` —— M3 翻转同步落档（`CH-6`）

**Files:**
- Modify: `docs/milestones/M3-tests.md`（增修订行：§6/§4 的 `U-M3-01..05`/`08` 翻转指针与 `T-TY-81/82/83` 改写说明；**§5.2 增 `R-32-2`**（cmp 语法扩展，Task 15 备好条文））
- Modify: `docs/milestones/M3-manifest.json`（`unsupported` 条目追加 `"resolved_by": "M4"`；`open_gaps` 注记）
- Modify: `rgoc/crates/rgoc-driver/tests/manifest_m3.rs`（如对账断言需要容纳新字段/新措辞——**只增不减**）

- [ ] **Step 1: 写修订行**（追加式；不改历史行）
- [ ] **Step 2: 更新 manifest** 与受影响对账测试
- [ ] **Step 3: 全绿** —— `scripts/in-container.sh cargo test -p rgoc-driver --test manifest_m3 --no-fail-fast` + `cargo test --workspace --no-fail-fast`
- [ ] **Step 4: 提交**

### Task 41: 变异反向验证（`T-CK-B1..B8`）+ `mutation-m4-notes.md`

**Files:**
- Create: `scripts/mutation-m4-notes.md`

- [ ] **Step 1: 逐条执行**（`M4-tests.md` §10 表）：B1 判定做反 / B2 类别投影 / B3 适配表信条 / B4 多余诊断 / B5 比对器 / B6 版本门控反转 / B7 清单守卫 / B8 init 顺序交换
- [ ] **Step 2: 纪律**：每次变异前 `cargo clean -p <crate>`；收尾复原后如结果可疑 ⇒ 全量重建再跑门禁把守；每条记录「命令 + 变红输出摘要 + 复原确认」
- [ ] **Step 3: 提交**

### Task 42: `manifest_m4.rs` + `M4-manifest.json` 填实（`T-CK-E3`、`T-CK-16` 完整化）

**Files:**
- Create: `rgoc/crates/rgoc-driver/tests/manifest_m4.rs`（沿 `manifest_m3.rs` 范式：与冻结文档**逐项对账** + ≥10 条**合成反例**）
- Create: `docs/milestones/M4-manifest.json`（**骨架已于 2026-10-18 产出**；本任务填实：`stage` / `generated_at` / `scope` / `crates` / `gate`(CH-1..9) / `coverage` / `test_ids`(63 个 `T-CK-*`) / `unsupported`（`U-M4-01..11` + M3 承接 7 项）/ `m3_sync` / `corpus_lists` / `budget`（含 `effort_calendar` 填实）/ `open_gaps` / `phase_summary`）

- [ ] **Step 1: 写对账测试（RED：文档与占位 manifest 不一致）**
- [ ] **Step 2: 填 manifest 至全绿**；`category_of` 穷尽性最终复核（`T-CK-16`：**每个** DiagCode 有映射、`other` 零映射）
- [ ] **Step 3: 提交**

### Task 43: `CH-7` 全量回归（四条 cargo + 既有自检，`T-CK-E4`）

- [ ] **Step 1: 四条命令**

```sh
scripts/in-container.sh cargo fmt --all --check
scripts/in-container.sh cargo check --workspace --all-targets
scripts/in-container.sh cargo clippy --workspace --all-targets -- -D warnings
scripts/in-container.sh cargo test --workspace --no-fail-fast
```

- [ ] **Step 2: 既有自检**（不回退）

```sh
python3 scripts/check-m0-consistency.py
python3 scripts/check-m1-consistency.py
bash scripts/check-ast-golden.sh && bash scripts/check-ast-testdata.sh && bash scripts/check-ast-stdlib.sh
```

- [ ] **Step 3: 记录**四条命令与门禁的**墙钟耗时**（对照 §预算），记入 progress
- [ ] **Step 4: 提交**

### Task 44: `M4-report.md` + 状态回写 + 收口提交

**Files:**
- Create: `docs/milestones/M4-report.md`（前端子集矩阵 + **三条分别声明**（静态/判定/对撞）+ `A3` 复核表 + wave A 处置 + 差距清单 + 接手指南）
- Modify: `AGENTS.md`（§0 状态一句话、§3.2 表、§7 下一步）、`docs/03-roadmap.md`（M4 状态）、`docs/README.md`（里程碑索引）、`docs/milestones/GAP-01.md`（M4 归属落点）
- Modify: `.workbuddy/memory/MEMORY.md`、追加当日日志

- [ ] **Step 1: 写报告**（含 `CH-1..9` 证据指向；每条「不承诺」项明示）
- [ ] **Step 2: 状态回写**（上述五处 + 本文件状态行 ⇒ 已完成）
- [ ] **Step 3: 文档自检**（§6.1 五项）全过
- [ ] **Step 4: 收口提交 + 推送**（`git push origin main`；被拦 ⇒ `python3 scripts/push-via-api.py`）

---

## 预算与降级序（`D-M4-14` 沿承）

| 层级 | 预算 | 出处 |
|---|---|---|
| 单文件检查 | ≤ **10 s** | `M4-tests.md` §9.4 |
| smoke（`--check` 抽样子集） | ≤ **5 min** | `AGENTS.md` §4.3 |
| milestone（`--check` 全量 75） | ≤ **30 min** | 同上 |
| full（四条 cargo + 四形态对撞） | ≤ **2 h** | 同上 |

**降级序**（预算不足时从上往下砍，砍掉必须登记）：

1. **先砍**：fixedbugs 后续 wave 的引入（wave A 之外一律不动）；wave A 本身的裁减（`T-CK-A2` 的显式降级路径）；
2. **次砍**：入选语料中的**高难文件**（按 `D-M4-3`「移出」纪律：逐行理由 + 修订 + 报告）；
3. **不砍**：入选集接受/拒绝 + 位类对齐、版本门控、init 顺序/环、四组点名 fixture（G4）、判定五件套与 cmp 新 kind、`U-M3-05` 解除、变异 ≥8 条。

## 门禁 ↔ Task 对照

| 门禁 | 主要由哪些 Task 兑现 |
|---|---|
| `CH-1` 判定五件套 | 6–11、12、15 |
| `CH-2` 检查器闭环 | 20–36、37 |
| `CH-3` 版本门控 | 17–19、26、37 |
| `CH-4` 泛型闭环 | 12–16、37–39 |
| `CH-5` 包级规则 | 32–36 |
| `CH-6` M3 翻转同步 | 10、15、40 |
| `CH-7` 四命令 + 回归 | 43 |
| `CH-8` oracle/装置 | 3、4、15、37 |
| `CH-9` 反向验证 + 交付 | 38、41、44 |

> **完成定义**：`M4-tests.md` §8 的 `CH-1..CH-9` 全部 `pass`（含各自「通过定义」），且 `M4-manifest.json` 的 `gate` 与此一致（Task 42 对账）。

