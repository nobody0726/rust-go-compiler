# AGENTS.md —— rust_go_compiler 工程入口

> **本文件的作用**：任何 agent / 协作者在本工作区开始任务前，读这一份即可掌握 —— 工程是什么、文档在哪、各自什么状态、遵守什么纪律、下一步做什么。
> **遇到细节一律跳转 `docs/`，本文件不复制细节。**
>
> **最后同步**：2026-10-07　|　**同步触发条件**：见 §5

---

## 0. 30 秒速览

| 项 | 值 |
|---|---|
| 目标 | 用 **Rust 重写 Go 编译器** |
| 项目代号 | **`rgoc`** |
| 语料快照 | `go_source_code/`，`VERSION` = **`go1.27.1`**（2026-08-28T16:20:06Z） |
| 规格基准 | `go_source_code/doc/go_spec.html`（The Go Language Specification, version go1.27, May 26, 2026） |
| AST 基准 | `go_source_code/src/cmd/compile/internal/syntax/nodes.go` |
| 首发平台 | **Linux / arm64**（Docker 容器提供）→ `aarch64-unknown-linux-gnu` / ELF |
| 当前阶段 | **M0 全部完成**（T01–T55 ✅，**E1–E10 十条门禁全过**）→ **Phase 4 已收口**（T48–T55：五份契约初稿 + `M0-report.md` + manifest 完整化 + E9 六条退出检查） |
| 仓库 | **Git**，remote `origin` → <https://github.com/nobody0726/rust-go-compiler>（public，分支 `main`） |

**一句话状态**：**M0 全部完成，门禁 E1–E10 十条全过**。容器镜像 `rgoc:dev`、Go oracle 1.27.1、Rust 1.98.1、`.devcontainer/` 与 `scripts/` 均已落地并实测通过；`rgoc/` 下是 harness（六个模块）+ driver CLI + xtask + **两个 SPIKE-ONLY crate**（`rgoc-hir` / `rgoc-spikes`），**260 条测试全绿**、自检 **228 条断言**全过；**五份契约**（`docs/contracts/`）+ **交付报告 `M0-report.md`** 就位。**编译器实现尚未开始** —— M0 证明的是解释 / SSA / native 三条路线可行 + oracle 流水线可信，**M1（词法与位置）现已具备开工条件**。

---

## 1. 目录地图

```text
rust_go_compiler/                     ← 工作区根（Git 仓库，remote: origin）
├── AGENTS.md                         ← 本文件：工程入口
├── .gitignore                        ← 排除语料 / backup / 系统产物
├── corpus-manifest.sha256            ← 语料锁定清单（15,618 条，见 §3.1）
├── docs/                             ← 文档体系（详见 §2）
│   ├── README.md                     ←   文档索引
│   ├── 01-feature-set.md             ←   规格：要建什么
│   ├── 02-test-inventory.md          ←   规格：如何验证
│   ├── 03-roadmap.md                 ←   计划：按什么顺序建
│   ├── 04-development-environment.md ←   环境：在哪建
│   ├── contracts/                    ←   跨阶段版本化契约（**5 份已产出**：C1 留位 / C2 完整初稿 / C3–C5 spike 级）
│   └── milestones/                   ←   阶段文档：<ID>-{design,tests,plan}.md + manifest
│       ├── M0-design.md              ←     M0 设计（**已确认**）
│       ├── M0-tests.md               ←     M0 测试先行清单（**已冻结** T29；§5.1 有修订 R1）
│       ├── M0-plan.md                ←     M0 实施计划（**T01–T55 全部完成**）
│       ├── M0-benchmarks.md          ←     M0 实测基准（§1–§12；E10 证据；D-M0-9 决策依据）
│       ├── M0-report.md              ←     **交接文档**（命题↔测试 ID / 门禁证据 / 差距 / 接手指南）
│       └── M0-manifest.json          ←     机器可读事实（**E1–E10 十条** + test_ids + unsupported + budget）
├── go_source_code/                   ← Go 1.27.1 官方语料，**只读**，**不入库**（185 MB）
├── rust-toolchain.toml               ← Rust 版本唯一来源（1.98.1）
├── .dockerignore                     ← 构建上下文收敛（镜像只需要 Dockerfile + toolchain 文件）
├── docker/                           ← 镜像定义
│   ├── Dockerfile                    ←     唯一镜像定义（6 层，含架构断言与登录 shell 修正）
│   └── image.lock                    ←     镜像锁定信息（E1；含「image id 不可复现」的说明）
├── scripts/                          ← 入口脚本（5 个，全部是「以后还用得到」的）
│   ├── in-container.sh               ←     统一容器入口（daemon 探测 + 卷 bootstrap + 参数透传）
│   ├── check-m0-consistency.py       ←     M0 一致性自检（228 条断言，退出码即结论）
│   ├── install-codelldb.sh           ←     CodeLLDB【平台包】离线安装（绕开宿主下发的死代理）
│   ├── install-vscode-server.sh      ←     VS Code Server 离线安装进持久卷 /vscode（宿主升级 VSCode 后用）
│   └── debug-smoke-test.sh           ←     无头调试链路冒烟测试（E5 的下层证据；第 2 节 A/B/C + 9 项断言）
├── .devcontainer/devcontainer.json   ← VSCode 调试环境（与门禁同镜像，D-M0-10）
├── .vscode/launch.json               ← CodeLLDB 调试配置（2 个：当前测试 / 全部测试）
├── rgoc/                             ← Rust workspace
│   ├── Cargo.toml                    ←     resolver 3 / edition 2024 / 全局 lint
│   ├── xtask/                       ←     构建期工具（语料枚举 / 报告生成 / manifest 生成，T37）
│   │   ├── src/corpus.rs           ←       全量枚举转发（分母 279）+ JSON
│   │   ├── src/report.rs           ←       报告生成（**复用 driver 的渲染器**，不自造）
│   │   ├── src/manifest.rs         ←       environment 节可重放生成（实测 + 手填常量分区）
│   │   └── tests/test_xtask.rs     ←       12 条（含「生成值无占位符」与 T16 同判据）
│   └── crates/
│       ├── rgoc-harness/            ←     测试基础设施（Phase 2 起是 harness 主体）
│       │   ├── src/ir.rs            ←       Test IR / 八种判定分类 / 冻结预算（T31）
│       │   ├── src/instruction.rs  ←       指令行解析 R1 + 分派顺序 R1b（T32）
│       │   ├── src/corpus.rs       ←       平台过滤 shouldTest + 语料枚举 + U 归类（T33）
│       │   ├── src/oracle.rs        ←       版本守门 + R6 命令形态 + 超时回收 + RSS（T34）
│       │   ├── tests/test_corpus.rs ←      15 条，含「分母 == 279」交叉校验
│       │   ├── tests/test_oracle.rs ←      12 条，真调容器内 go1.27.1（T34）
│       │   ├── src/compare.rs      ←       比较器 R2/R3/R4 + 正则子集匹配器（T35）
│       │   ├── tests/test_compare.rs ←     26 条（T35）
│       │   ├── src/runner.rs       ←       一条用例端到端 + 层级报告（T36 / **E3**）
│       │   ├── tests/harness_self_test.rs ← 19 条六类自测，正反例齐备（T36 / **E3**）
│       │   ├── tests/test_instruction.rs ←  14 条验收测试（含顺序契约与 linkmain.go fixture）
│       │   ├── src/lib.rs           ←       挂载 `pub mod ir` + `double_sum`（E5 复验锚点）
│       │   └── tests/test_ir.rs     ←       C2 契约的可执行副本（T31）
│       └── rgoc-driver/             ←     统一 CLI 入口（T37）
│           ├── src/cli.rs           ←       手写参数解析（**不引 clap**；不预留未实现子命令）
│           ├── src/samples.rs       ←       20 样本冻结表（与 M0-tests §4 逐条对账）
│           ├── src/report.rs        ←       分子/分母/八类分布 + JSON（分母不过滤变小）
│           └── tests/test_driver.rs ←       23 条（含「子命令必须报错」「分母纪律」「峰值 RSS」）
│       ├── rgoc-hir/                ←     **SPIKE-ONLY** 最小 HIR（T40，**M5 整体替换**）
│       │   ├── src/lib.rs           ←       crate 级 `SPIKE-ONLY` 标注（D-M0-11）+ C1 留位说明
│       │   ├── src/hir.rs           ←       `Const`（Big/Bool/Str）+ `Stmt::Print{newline,stream,args}` + `Stream`
│       │   ├── src/value.rs         ←       `BigInt`（base 2^32 任意精度）+ `Val`；**收敛到 i64 溢出即报错**
│       │   └── src/diag.rs          ←       `Pos` / `Diag` / `DiagBag`（C1 只留位，**无源码位置跟踪**）
│       └── rgoc-spikes/             ←     **SPIKE-ONLY** 三个 spike 的隔离载体（T41，可整块删）
│           ├── src/fixtures/mod.rs   ←       共享固定 HIR（S1/S2 **必须共用** `sum_expr()` 才能交叉验证）
│           ├── src/interp.rs         ←       S1 求值器（**判定逻辑在 lib，bin 只做 I/O**）
│           ├── src/ssa.rs            ←       S2 Block/Value 图（后序遍历 ⇒ 使用先于定义）
│           ├── src/ssa_needs.rs      ←       T-S2-02 需求清单（memory/tuple/调用边界/phi/verifier）
│           ├── src/native.rs         ←       S3 codegen + `clang -nostartfiles -Wl,-s` 链接
│           ├── src/native_records.rs ←       T-S3-04 六项记录（每项含「**没记录什么**」栏）
│           ├── src/bin/{s1_interp,s2_ssa,s3_native}.rs ← 三个进程边界（**均拒绝任何参数**）
│           └── tests/               ←       26+18+16+3 条（库验收 / 进程边界 / e2e / E6 输入守卫）
├── tests/corpus/                  ←     T38 的 E4 基线报告（可重放）
│   ├── T-C-report.md              ←       人读版 + 全量 279 分母一节
│   ├── T-C-report.json            ←       机器读（分母/分子/八类/逐例 RSS）
│   ├── build-report.py            ←       补全量一节（分母≠20 或 success≠true 拒绝产出）
│   └── register-gate.py           ←       登记 gate.E4（先读报告交叉核验再写 manifest）
├── .workbuddy/
│   ├── memory/
│   │   ├── MEMORY.md                 ←   项目长期事实（★每次请求自动注入）
│   │   └── YYYY-MM-DD.md             ←   按日工作日志（append-only，不改写）
│   └── backup/                       ←   快照（不入库）：文档历史 + 已移除脚本（`scripts-removed-*/`）
```

> ★ **关键机制**：`.workbuddy/memory/MEMORY.md` 会被**自动注入每一次请求**。因此「必须每轮都知道的工程级事实」同时沉淀在本文件与 `MEMORY.md` 中 —— 本文件面向人与跨工具阅读，`MEMORY.md` 负责保证自动化生效。

**尚不存在**（由 M0 后续阶段创建，结构见 `03` §2）：`rgoc/crates/` 下的编译器正式 crate（lexer / parser / sema / mir / codegen …）—— `rgoc-hir` 虽已存在但**是 SPIKE-ONLY**，M5 会被正式 HIR 整体替换。

---

## 2. 文档体系

### 2.1 清单

| # | 文档 | 层级 | 回答什么问题 | 状态 | 规模 |
|---|---|---|---|---|---|
| — | [`docs/README.md`](./docs/README.md) | 索引 | 文档地图是什么 | 已建立 | ≈13 KB |
| 01 | [`docs/01-feature-set.md`](./docs/01-feature-set.md) | 规格 | **要建什么** —— 功能全集 / RTM | 已建立 | ≈45 KB |
| 02 | [`docs/02-test-inventory.md`](./docs/02-test-inventory.md) | 规格 | **如何验证** —— 功能点 → 官方测试用例 | 已建立（9 处修正） | ≈60 KB |
| 03 | [`docs/03-roadmap.md`](./docs/03-roadmap.md) | 计划 | **按什么顺序建** —— M0–M12 迭代计划 | 已修订（v2） | ≈51 KB |
| 04 | [`docs/04-development-environment.md`](./docs/04-development-environment.md) | 环境 | **在哪建** —— Docker 容器方案 | **已落地**（Phase 0 实测通过） | ≈13 KB |
| — | [`docs/milestones/M0-design.md`](./docs/milestones/M0-design.md) | 设计 | **怎么建 M0** —— 决策 D-M0-1~15 / 环境基线 / Phase 0–4 / 门禁 E1–E10 | **已确认** | ≈32 KB |
| — | [`docs/milestones/M0-tests.md`](./docs/milestones/M0-tests.md) | 测试 | **怎么验 M0** —— T-H/T-C/T-S 测试 ID、20 样本、unsupported（U1–U14）、超时预算、**M0 分母 279** | **已冻结**（2026-10-02，T29） | ≈32 KB |
| — | [`docs/milestones/M0-plan.md`](./docs/milestones/M0-plan.md) | 计划 | **怎么干 M0** —— **Phase 0–4 的 T01–T55**（路径 / 可粘贴内容 / 验证） | ✅ **全部完成**（T01–T55，2026-10-07） | ≈100 KB |
| — | [`docs/milestones/M0-benchmarks.md`](./docs/milestones/M0-benchmarks.md) | 实测 | **凭什么是这样** —— 时间/体积/冷启动/挂载布局/可复现性/环境陷阱 + **四则调试环境案例**（§7 平台包 / §8 DWARF / §9 Server / §10 cargo 启动配置）+ §11 期望值复核 + **§12 Phase 3 可复现性** | **已产出** | ≈52 KB |
| — | [`docs/milestones/M0-report.md`](./docs/milestones/M0-report.md) | **交接** | **给下一个接手的人** —— 三个命题各由哪个测试 ID 证明 / 门禁证据出处 / spike 留下的差距 / 未验证范围 / **接手指南** | ✅ **已产出**（T53） | ≈13 KB |
| — | [`docs/contracts/`](./docs/contracts/) | 契约 | **五份契约** —— [C1](./docs/contracts/C1-source-map.md) 留位（位置+诊断排序）/ [C2](./docs/contracts/C2-test-ir.md) **完整初稿**（Test IR）/ [C3](./docs/contracts/C3-hir.md) HIR / [C4](./docs/contracts/C4-ssa.md) SSA / [C5](./docs/contracts/C5-abi.md) ABI | ✅ **已产出**（T48–T52 / E8） | ≈35 KB |

**阅读顺序**：01 → 02 → 03 → 04。

**阶段文档**（`docs/milestones/`）——每阶段开工前须产出 **三件套**（`03` §6.2）：

| 件 | 回答什么 | M0 状态 |
|---|---|---|
| `<ID>-design.md` | **做什么、边界在哪** | ✅ 已确认 |
| `<ID>-tests.md` | **怎么算通过** | ✅ **已冻结**（T29，2026-10-02；§5.1 另有**修订 R1**） |
| `<ID>-plan.md` + `manifest.json` | **按什么顺序动手** + 环境锁定值 | ✅ **计划全部完成**（T01–T55）；manifest **已填实**（`environment` + `gate` **十条** + `test_ids` + `unsupported` + `budget`） |

### 2.2 依赖方向

```text
01-feature-set ───┐
（246 个功能 ID）   ├──► 03-roadmap ──► docs/milestones/<阶段ID>-design.md
02-test-inventory ┘    （迭代计划）      docs/milestones/<阶段ID>-tests.md
（测试线索）                             （每阶段开工前必产，03 §6.2）

04-development-environment ──► 落实 03 §0.2 的平台 / 工具链约束
```

- `03-roadmap.md` §0 明确声明 **01 与 02 是其「输入规格」**。
- 二者是**追踪输入**，**不等于已实现**、**也不等于可直接复用**。
- `02-test-inventory.md` 中 `spec:` / `nodes:` 引用的含义定义在 `01-feature-set.md` **§0.4**。

### 2.3 引用约定（定义在 01 §0.4，全库通用）

| 前缀 | 展开为 | 性质 |
|---|---|---|
| `spec:Lxx-yy` | `go_source_code/doc/go_spec.html` 行区间 | **规范性**（Rust 实现须 100% 对齐） |
| `nodes:Lxx-yy` | `.../internal/syntax/nodes.go` 行区间 | **规范性** |
| `runtime/*.go:NNN`、`abi/type.go:NNN` | 官方 runtime / ABI 源码 | 参考性（仅语义校准） |
| `gc/*/`、`link/ld/` | 官方编译 / 链接阶段目录 | 参考性 |

> 所有行号基于**本仓库当前快照**；语料版本一变，**全库行号即失效**（见 §5）。

---

## 3. 状态快照

### 3.1 已完成

- 文档体系 4 篇正文 + 索引，内部相对链接 **24/24 全部有效**（2026-10-02 校验）
- 仓库已发布：<https://github.com/nobody0726/rust-go-compiler>（public，`main`）
- **语料锁定**：`corpus-manifest.sha256`（**15,618** 条，`shasum -a 256 -c` 全量校验 **0 失败**）
- **功能全集**：246 个功能点 = Spec 语义原子 **133** + AST 对账 **49** + Runtime **36** + PIPE **28**
- **测试映射**：约 **236/246** 有专属官方测试；**9 项**无
- **迭代计划 v2**：**M0–M12**；四条交付线 —— A 前端 / B 顺序解释 / C native MVP / D 能力扩展
  - 首个 native 交付是 **C**，**不以 D 完成为前置**
  - 两个端到端锚点：M0 native `hello` spike（固定输入）、M7 正式 native MVP

### 3.2 未完成 / 需注意

| 项 | 说明 |
|---|---|
| 编译器工程 | `rgoc/` 下**只有 harness + 两个工具 crate**（`rgoc-harness` / `rgoc-driver` / `xtask`），harness 已不只是调试目标：T31 落下了 Test IR 骨架（16 个必录字段 + 八种判定分类 + 冻结预算的唯一入口 `Limits::for_layer`），T37 加了 CLI 与构建期工具，T38 出了 E4 基线（20/20），T39 复核登记并把登记变成可执行断言。**编译器实现仍未开始** —— 还没有 lexer / parser / HIR（那是 Phase 3 的 `rgoc-hir` 与三个 spike） |
| 开发环境 | **已就绪**：镜像 `rgoc:dev`（`sha256:21f55802…`，2.92GB）、Go oracle `go1.27.1 linux/arm64`、Rust `1.98.1`、clang 14、CodeLLDB 1.12.3（自带 lldb 22.1.8-codelldb）、`.devcontainer/` 与 `scripts/` 全部落地并实测通过 |
| M0 | **Phase 0 与 Phase 1 均已完成**（T01–T28）。四项门禁 **E1/E2/E10/E5 全部通过**，证据见 `M0-manifest.json` 的 `gate` 与 `M0-benchmarks.md`；E5（实测断点命中）由用户在 VSCode 中按 F5 于 2026-10-02 确认，登记在 `gate.E5`（含 `confirmed_at` / `confirmed_by`）。**Phase 2 已完成（T29–T39，2026-10-05 收口）**：清单已冻结（T29）、期望值已复核（T30）、harness 五件套已落地（T31–T35）、E3 六类自测已过（T36）、driver CLI + xtask 已落地（T37）、**E4 基线 20/20 通过（T38，5.8 s / 峰值 15 MiB）**、T39 复核登记（把登记变成 **19 条可执行断言**，含 7 次反向验证）。**Phase 3 已完成（T40–T47，2026-10-07）**：两个 SPIKE-ONLY crate 落地、三个 spike 跑通、**E6/E7 通过**；**Phase 4 已完成（T48–T55）**：五份契约 + `M0-report.md` + manifest 完整化 + **E9 六条退出检查**。**M0 五个 Phase 全部完成，E1–E10 十条门禁全过；下一步 M1（词法与位置）**—— 开工前读 `M0-report.md` §7 |
| 已知环境约束 | ① 所有构建与测试**必须**在容器内（`scripts/in-container.sh`）；② `rgoc/target/` 在命名卷 `rgoc-target`，故 `cargo clean` 会 `EBUSY` —— 清空用 `find rgoc/target -mindepth 1 -delete`；③ 镜像 **不可位级复现**，钉子只有 `base.index_digest` + `src.*_sha256`，**image id 不得写进门禁**；④ **宿主 VSCode 的 `http.proxy` 会被下推进容器**（经 AHP `root/configChanged`），容器内 `127.0.0.1` 指向自己 → 一切走 VSCode 网络栈的下载都会失败。**远端 Machine settings 覆盖不了它**；CodeLLDB 平台包用 `scripts/install-codelldb.sh` 离线装（见 `M0-benchmarks.md` §7）；⑤ **每次升级宿主 VSCode 都可能让窗口连不上容器** —— commit 变了而持久卷 `/vscode` 里没有新 server，Dev Containers 便去宿主侧下载（`Path: /var/folders/…`）再被死代理挡住。**报错文字与 ④ 一模一样但层级不同**，按 `Path:` 辨异，修法是 `scripts/install-vscode-server.sh`（见 §9）；⑥ **容器重建后 CodeLLDB 平台包必丢**（`~/.vscode-server/extensions/` 不在任何卷里），重跑 `install-codelldb.sh`；⑦ **CodeLLDB 的 cargo 启动配置有两个静默陷阱**（`launch.json` 里写错不报错，只在按 F5 时以 `Cargo command did not complete successfully.` 出现）：`cargo` 的工作目录取自 **`cargo.cwd`（不读顶层 `cwd`）**，漏写就回退到 `/work`（无 `Cargo.toml`）→ cargo 退出 **101**；`filter.name` 比对的是 **cargo 的 target name（下划线）而非包名（连字符）**，写错会 0 匹配。真错在 **OUTPUT → LLDB** 通道的 `Cargo exited with code N`，**不在 VSCode 弹出的那个提示里**；且**不能用 shell 复现**那条命令（CodeLLDB 是无 `shell: true` 的 `spawn`，shell 会剥掉 `target.'cfg(all())'` 的单引号 → 假的 TOML 报错）。见 `M0-benchmarks.md` §10，回归由 `debug-smoke-test.sh` 第 2 节守住（该节按 `launch.json` 原样复刻 CodeLLDB 的 cargo 步骤） |
| 9 项测试缺口 | TYP-26、SCP-06、EXP-16、EXP-22、PKG-04、PKG-06、RT-SCH-02、RT-POLY-03、RT-POLY-05 —— 须在 rgoc 自有测试补齐 |
| 阶段目录 | `docs/contracts/` **5 份契约已产出**（C1 留位 / C2 完整初稿 / C3–C5 spike 级）；`docs/milestones/` 含 M0 四件套 + `M0-report.md` + `M0-manifest.json` |
| 事实修正 | 02 **§0.2** 记录了 **9 处**对既有表述的修正（如 `syntax/testdata/` 实为 **31** 个文件；驱动器是 `testdir_test.go` 而非 `test/run.go`）——**以该节为准** |

---

## 4. 工作纪律

### 4.1 开发流程（Superpowers）

1. **规格未确认，不动手** —— 先对齐再执行
2. 行为切片一律 **RED → GREEN → REFACTOR**：**先确认失败来自缺失行为**（而非环境 / harness），再做最小实现
3. **没有失败的测试，就不算实现**；测试之前写的代码一律删除
4. **提交仅在明确要求时执行**；提交信息引用功能 ID / 验证证据

### 4.2 完成度按四维分别跟踪

**Frontend / Interpreter / Native / Runtime-metadata**。

**不以「246 项全勾选」作为总退出条件**；同一功能的静态、解释、native 实现**互不替代**完成证据。官方内部算法测试通过 ≠ Go 语言兼容；使用系统工具 ≠ 自研功能已完成。

### 4.3 硬约束（红线）

- **只读官方语料**：`go_source_code/` 不得修改、不得回写；该目录**不入库**，由 `corpus-manifest.sha256` 锁定
- **`git push` 常被拦**：`github.com` 的 CONNECT 间歇性返回 502。不通时改用技能 `github-push-via-api` 走 Git Data API —— 见 §6.2
- **不默认新增第三方依赖**：需单独明确请求与授权
- 每个阶段及独立验收子阶段**开工前**必须先产出 `docs/milestones/<阶段ID>-tests.md` + 机器可读 manifest
- 门禁：必需用例 **100% 通过**，无基建 / 不稳定失败；白名单须**开工前冻结**，跳过项**不得**记为 pass
- 测试预算：**smoke ≤ 5 min**，**milestone ≤ 30 min**，**full 单次 ≤ 2 h**（实测后只做有记录的调整）

---

## 5. 变更同步清单

> 改下面任何一处，**必须**连带更新右列，否则文档体系会失去一致性。

| 若发生 | 必须同步 |
|---|---|
| 新增 / 删除 / 改名功能点 | `01` §0.3 计数 + §1–4 表格 → `02` 对应映射 → `03` §4 追踪表、§5 归属表 |
| 重命名 / 移动任何文档 | `docs/README.md` 文档地图 → **本文件 §1 §2** → 各文档头部「文档索引 / 上游 / 下游」行 → 全库 grep 旧名 |
| 里程碑增删或编号变化 | `03` §1.2 依赖图与编号对照表、§4 详细计划、§5 归属表、§6.1 预算表 → **本文件 §3** |
| 语料版本变更（`VERSION`） | `01` §0.4 行号基准（**全库行号失效**）→ `02` 全部引用 → `03` §0.2 兼容性矩阵 → 重新生成 `corpus-manifest.sha256` → **本文件 §0 §3** |
| 平台 / 目标 triple 变更 | `03` §0.2 → `04` 全文 → **本文件 §0** |
| 新增阶段文档（design / tests / manifest） | `docs/README.md` §1 §5 → **本文件 §1 §2.1 §3** |
| 某阶段完成 / 门禁达成 | 该阶段 `docs/milestones/<ID>-tests.md` → `03` 对应里程碑 → **本文件 §3** |
| 完成任何实质工作 | 追加 `.workbuddy/memory/YYYY-MM-DD.md`；长期约定写入 `MEMORY.md` |

---

## 6. 文档自检与推送

### 6.1 文档自检

改完文档后跑一遍（期望：① 无 `BROKEN` 行　② 输出 `clean`　③ 两个目录路径都列出　④ 自检全部通过、exit 0）：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler

# 1) 内部相对链接是否都指向存在的文件（含子目录，期望无 BROKEN 行）
for f in $(find docs -name '*.md'); do
  d=$(dirname "$f")
  grep -o '](\.\.\?/[^)]*)' "$f" 2>/dev/null | sed 's/](//; s/)$//' | while read -r t; do
    [ -e "$d/$t" ] || echo "BROKEN: $f -> $t"
  done
done

# 2) 是否残留旧文件名（期望输出 clean）
grep -rn --include='*.md' -E "COMPILER_FEATURE_SET|TEST_CASE_INVENTORY|DEVELOPMENT_ENVIRONMENT|ROADMAP\.md" docs/ || echo clean

# 3) 阶段目录是否已创建（期望列出两个路径）
ls -d docs/contracts docs/milestones 2>/dev/null || echo "阶段目录缺失"

# 4) 环境一致性自检（期望：全部通过，exit 0）
python3 scripts/check-m0-consistency.py
```

> **注意**：`check-m0-consistency.py` 的断言是**静态判据**。**一条恒真的断言比没有断言更糟** ——
> 它给出「已检查」的错觉。本仓已两次踩到「断言被注释满足」的坑（断点行推导的 `grep`
> 命中注释行；`install-*.sh` 的头部注释里也写着 `--noproxy '*'` / `platform.ok` /
> `ln -sfn` / `[0-9a-f]{40}`）。所以第 5 节的断言一律作用于 `code_only()`（剔除整行注释），
> 且**改动任何断言后都要人工做一次反向验证**：把目标改成注释形态或直接删掉，
> 确认断言会报 ✗ —— 不报 ✗ 就是恒真断言，必须重写。
>
> **为什么是「人工」**：原先有两个变异测试脚本（`mutation-test-m0-consistency.py` 33 个变异、
> `mutation-test-debug-smoke.sh` 4 个变异）把这步自动化了，并且**确实抓到过 3 处真实缺陷**
> （2 条恒真断言 + 夹具缺文件导致断言被整段跳过）。它们已于 2026-10-02 按用户要求移除
> 以精简交付物，快照留在 `.workbuddy/backup/scripts-removed-20261002-1504/`（不入库），
> 需要时可拷回 `scripts/` 直接跑。**代价要记着：现在没有任何自动手段能证明断言会失败。**
>
> **第 4 条自身还带一条防腐断言**（§6）：`AGENTS.md` 里记录的断言数必须等于实际断言数。
> 数字漂了比没有数字更糟 —— 它会让人以为「覆盖了 N 条」而实际不是。

新增文档的约定：顶层命名 `NN-<kebab-topic>.md`（`NN` 体现层级与阅读顺序；`README.md` 固定不编号）；新增后登记进 `docs/README.md` → 更新本文件 §1 §2 → 补头部「文档索引 / 上游 / 下游」行。

### 6.2 推送到 GitHub

`github.com` 的 CONNECT 在本机常被会话代理拦截（`git push` 报 `CONNECT tunnel failed, response 502`），而 `api.github.com` 正常。

```sh
# 1) 先直接重试 —— 该故障是间歇性的
git push origin main

# 2) 仍不通则走 Git Data API（脚本自带 SHA 校验，保证远端与本地完全一致）
python3 ~/.workbuddy/skills/github-push-via-api/push_via_api.py \
    --repo nobody0726/rust-go-compiler --cwd . --gh /opt/homebrew/bin/gh
```

**空仓库的坑**：GitHub 的 Git Data API 在**完全空的仓库**上会拒绝 `POST /git/blobs`（409 `Git Repository is empty`）。此时先用 Contents API 造一个引导提交使仓库非空，再用 `--root` 把 ref 强制指向本地根提交：

```sh
gh api -X PUT repos/nobody0726/rust-go-compiler/contents/.gitkeep \
   -f message="chore: bootstrap" -f content="eA=="
python3 ~/.workbuddy/skills/github-push-via-api/push_via_api.py \
    --repo nobody0726/rust-go-compiler --cwd . --root --gh /opt/homebrew/bin/gh
```

---

## 7. 下一步

**Phase 0 与 Phase 1 均已完成 —— M0 的四项门禁（E1 / E2 / E10 / E5）全部通过。**

**E5 的完成记录**：2026-10-02 由用户在 VSCode dev container 中按 F5 实测确认，T28 检查表四项逐项通过（断点命中未被跳过 / 变量面板 `a=1,b=2` / 调用栈 ≥2 帧 / F10 后停在第 28 行且 `sum==3`），登记在 `M0-manifest.json` 的 `gate.E5`。

按序执行：

0. **先确认「连得上容器」**（宿主机执行）：Dev Containers 报 `Installing VS Code Server for commit …`
   就是这一层。先把宿主 VSCode 的 commit 取出来，再装进容器：

   ```sh
   VSCODE_PRODUCT="/Applications/Visual Studio Code.app/Contents/Resources/app/product.json"
   COMMIT=$(python3 -c "import json;print(json.load(open('$VSCODE_PRODUCT'))['commit'])")
   docker exec <容器名> bash /work/scripts/install-vscode-server.sh --commit "$COMMIT"
   ```

   装进持久卷 `/vscode` 后两处判据直接命中，**不再发起任何下载**。幂等、零多余请求，
   所以在信息最少时它是成本最低的起手式（`M0-benchmarks.md` §9）。
   ⚠️ **升级宿主 VSCode 后必做**，且装完要 Reopen in Container。
0b. **再确认容器环境层没坏**（宿主机执行，约 10 秒）：
   `docker exec <容器名> bash /work/scripts/debug-smoke-test.sh` → 期望 **A/B/C 三项 + 9 项断言通过、exit 0**。
   其中第 2 节的 A/B/C 是按 `.vscode/launch.json` **原样**跑一遍 cargo（cwd 来自 `cargo.cwd`、
   `filter` 按 target name 筛选），即 E5 那条链路在下层的一次完整彩排 —— 它红了就不必去按 F5。
   如果 CodeLLDB 报 `Failed to establish a socket connection to proxies: PROXY 127.0.0.1:7897`，
   说明**平台包**没装上，先跑 `docker exec <容器名> bash /work/scripts/install-codelldb.sh`
   再 `Developer: Reload Window`。**别去改宿主 settings，也别指望 devcontainer 里那两行
   `http.proxy` 配置** —— 该值由宿主经 AHP 下发，远端覆盖不了（实测，见 `M0-benchmarks.md` §7）。
   **注意与第 0 步的区别**：同一句报错，`Path:` 在宿主 `/var/folders/…` 就是 server 层（第 0 步），
   不在宿主就是平台包层（本步）—— 见 §9 的层级辨异表。
   （第 0/0b 两步在 Phase 1 之后依然保留：它们是**任何一次环境异常时成本最低的起手式**，
   与「有没有 E5 要做」无关。）
1. ~~冻结 `M0-tests.md`~~ ✅ **已完成**（T29，2026-10-02）：20 样本 + U1–U14 + 超时上限已冻结，**M0 分母 = 279**
   ✅ T30 —— oracle 侧复核 20 个样本期望值（19/20 一致，1 处文档错已改）
   ✅ T31 —— `rgoc-harness` 的 Test IR 骨架（**15 个必录字段** / 八种判定分类 / 冻结预算唯一入口）
   ✅ T32 —— 指令行解析（R1）+ 分派顺序（R1b），**顺序写进了函数签名**：`dispatch(ins, platform_ok)`
   ✅ T33 —— 平台过滤 `shouldTest` + 语料枚举 + U 归类，**枚举分母 == 279 与冻结口径对上**
   ✅ T34 —— oracle 调用 + 版本守门（T-H-06）；`run` 层走官方 fast path，超时不留孤儿且不需要 unsafe
   ✅ T35 —— 比较器（R2 输出期望 / R3 诊断切分 / R4 ERROR 期望）+ **正则子集：认识的就匹配，不认识的明确报错**
   ✅ T36 —— 六类自测全绿，**E3 门禁通过**（19 条，正反例齐备）
   ✅ T37 —— `rgoc-driver` CLI 骨架（`harness list/run/report`，**不预留**未实现子命令）+ `xtask`（语料枚举 / 报告生成 / manifest 生成）
   ✅ T38 —— **E4 门禁通过：20/20**，5.8 s / 峰值 15 MiB；修了跨行正则的 `\n` 转义（`compare.rs`）
   ✅ T39 —— Phase 2 门禁复核与登记（`phase_plan.phase2` 标 done；新增 5d/5e 两节共 19 条断言，
   把「登记的证据」与**可重放产物**（E4 报告）对撞；7 次变异测试确认断言真能失败）
   ✅ T40 —— `rgoc-hir`（**SPIKE-ONLY**，M5 替换）：`BigInt` 任意精度 + `Stream` 输出流 + C1 留位
   ✅ T41 —— `rgoc-spikes` 骨架（三个 bin 各留 RED 占位，退出码 70）+ **E6「输入写死」的可执行守卫**
   ✅ T42 —— **S1 解释 spike**（`T-S1-01/02/03`）：stderr 精确 `3\n`、stdout 精确空、`1<<100` 拒绝收敛
   ✅ T43 —— **S2 SSA spike**（`T-S2-01/02/03`）：图里真有 `bin` 指令，结果与 S1 **逐字节相同**
   ✅ T44 —— **S3 native spike**（`T-S3-01…04`）：HIR → arm64 汇编 → ELF → 运行输出 `hello`
   ✅ T45 —— **E6 门禁通过**：三 spike 各 3 次，输入/结果/环境全部一致（`M0-benchmarks.md` §12）
   ✅ T46 —— **E7 门禁通过**：`SM-M0-NATIVE-HELLO` 登记进 `03` §M1 smoke 清单
   ✅ T47 —— Phase 3 门禁复核与登记（新增 §5f 共 22 条断言；**门禁键改为从 manifest 动态枚举**）
   ✅ T48 —— C1 契约初稿（**留位**：位置表示与诊断排序已定，但**无源码位置跟踪** —— 那是 M1/M2）
   ✅ T49 —— C2 契约初稿（**完整初稿**，M0 唯一「完整」级契约，消费者是所有阶段）
   ✅ T50/T51/T52 —— C3（HIR）/ C4（SSA）/ C5（ABI）契约初稿（**spike 级**，已验证与待验证分节列开）
   ✅ T53 —— `M0-report.md` 交付报告（面向接手人：三个命题的证明者、门禁证据、差距清单、接手指南）
   ✅ T54 —— manifest 三节完整化（37 个测试 ID / U1–U14 / 预算三节）
   ✅ T55 —— **E9** 六条统一退出检查 + M0 全门禁复核（**E1–E10 十条全过**）
   👉 **下一个**：**M1 · 词法与位置**（`03` §M1）—— 读 **`M0-report.md` §7「接手指南」**，
   那里列了必读顺序与**开始 M1 之前必须知道的三件事**
2. 基于**实测到的环境事实**拆 **Phase 2–4** 的计划（`M0-plan.md` §0.1 已说明为何此时才拆）
3. 开工前跑一遍 §6.1 的自检，并把执行状态回写 `M0-manifest.json` / `M0-plan.md`

> ⚠️ **Phase 3 的两处「实测推翻文档/直觉」（别再踩回去）**：
>
> 1. **Go 内建 `println` 写 stderr，不是 stdout**（go1.27.1 实测，`od -c` 逐字节）。
>    `M0-tests.md` §5.1 的 `T-S1-01` 原写「stdout 精确 `3\n`」是**错的**，已订正
>    （修订 R1 存档在该节）。配套：`rgoc-hir` 的 `Stmt::Print` 增加 `Stream` 字段，
>    S1=`Stderr` / S3=`Stdout`，**类型层面**禁止混淆两个 fixture。
> 2. **`print` 不加分隔符，只有 `println` 加**（实测 `print("a","b",1,2)` → `ab12`）。
>    Go 规范只把两者列为内建名、**没规定分隔符**，这类行为只能问 oracle。
> 3. **链接裸汇编必须两个开关**：`-nostartfiles`（否则 `Scrt1.o` 已定义 `_start`，链接失败）
>    + `-Wl,-s`（否则 `.strtab` 残留 clang 随机中间名 `hello-d9450b.o`，**产物不可复现**）。

**E5 已完成的记录（2026-10-02）**：由用户在 VSCode dev container 中按 F5 实测确认，
T28 检查表四项逐项通过，登记在 `M0-manifest.json` 的 `gate.E5`。
下面是当时的操作与排障路径 —— **环境复发时（换机器 / 重装 / 升级 VSCode）要复验时照它走**：

- 在 VSCode 中打开本仓库 → `Dev Containers: Reopen in Container`
  → 在 `rgoc/crates/rgoc-harness/src/lib.rs` 的 `double_sum()` 里 `let sum = a + b;`（第 27 行）下断点
  → 按 F5 跑 **「调试当前测试 (CodeLLDB)」** → 逐项核对 `M0-plan.md` T28 的四项检查表。
- **注意**：第 4 项（单步后 `sum == 3`）依赖调试目标的形状 —— 尾位置直接返回的 `let` 绑定
  不会进 DWARF，换目标前先读 `M0-benchmarks.md` §8。
- **如果 F5 弹出 `Cargo command did not complete successfully.`**：那不是环境层的问题，
  而是 CodeLLDB 的 **cargo 启动配置**（`cargo.cwd` / `filter.name`，两者都是**静默**陷阱，
  VSCode 的提示里不会指向它们）。真错在 **OUTPUT → LLDB** 通道里的 `Cargo exited with code N`，
  不在弹出的那个提示里 —— 两个坑、复现方法与修法见 `M0-benchmarks.md` §10。
  **别用 shell 去复现那条 cargo 命令**：CodeLLDB 是不经 shell 的 `spawn`，shell 会剥掉
  `target.'cfg(all())'` 上的单引号，给你一个**假的** TOML 报错。

**M0 的 5 个 Phase**（详见 `M0-design.md` §6）：

| Phase | 内容 | 门禁 | 计划状态 |
|---|---|---|---|
| 0 | 容器与工具链底座 | E1 镜像 digest 可重放、E2 环境值入 manifest、E10 基准 | ✅ **已完成**（T01–T19；E1/E2/E10 通过） |
| 1 | VSCode 调试环境 | **E5 实测断点命中** | ✅ **已完成**（T20–T28；四条统一退出检查全过，E5 于 2026-10-02 人工实测确认） |
| 2 | Rust 工程骨架 + harness | E3 六类自测全绿、E4 20 样本 100% | ✅ **已完成**（T29–T39；E3 2026-10-04、E4 2026-10-05 20/20） |
| 3 | 三个 spike（解释 / SSA / native） | E6 可复现、E7 native `hello` | ✅ **已完成**（T40–T47；E6/E7 均 2026-10-07） |
| 4 | 契约初稿 + 报告 | E8 / E9 / E10 收口 | ✅ **已完成**（T48–T55；E8 五份契约、E9 六条退出检查，2026-10-07） |

> **M0 五个 Phase 全部完成**（T01–T55，2026-10-07），**E1–E10 十条门禁全过**。
> **M1（词法与位置）已具备开工条件** —— 开工前先读 [`M0-report.md`](./docs/milestones/M0-report.md) §7，
> 那里列了必读顺序与**必须知道的三件事**。

**原则**：早期可行性验证、单平台首发、独立可退出的能力切片优先；完整 Go toolchain / runtime 是后续扩展，不是首发承诺。
