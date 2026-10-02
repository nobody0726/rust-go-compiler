# AGENTS.md —— rust_go_compiler 工程入口

> **本文件的作用**：任何 agent / 协作者在本工作区开始任务前，读这一份即可掌握 —— 工程是什么、文档在哪、各自什么状态、遵守什么纪律、下一步做什么。
> **遇到细节一律跳转 `docs/`，本文件不复制细节。**
>
> **最后同步**：2026-10-02　|　**同步触发条件**：见 §5

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
| 当前阶段 | **M0 前置已就绪，尚未开工** —— 设计与测试清单齐备，仍只有文档，无任何代码 |
| 仓库 | **Git**，remote `origin` → <https://github.com/nobody0726/rust-go-compiler>（public，分支 `main`） |

**一句话状态**：文档体系（4 篇正文 + 1 索引 + M0 设计与测试清单）已建立并互链，**已发布到 GitHub**；M0 **开工前置已就绪**，但**编译器工程、开发环境、工具链、全部里程碑均尚未开始**。

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
│   ├── contracts/                    ←   跨阶段版本化契约（**空**，M0 Phase 4 产出）
│   └── milestones/                   ←   阶段文档：<ID>-{design,tests,plan}.md + manifest
│       ├── M0-design.md              ←     M0 设计（**已确认**）
│       ├── M0-tests.md               ←     M0 测试先行清单（待冻结）
│       └── M0-plan.md                ←     M0 实施计划 Phase 0–1（待执行）
├── go_source_code/                   ← Go 1.27.1 官方语料，**只读**，**不入库**（185 MB）
├── .workbuddy/
│   ├── memory/
│   │   ├── MEMORY.md                 ←   项目长期事实（★每次请求自动注入）
│   │   └── YYYY-MM-DD.md             ←   按日工作日志（append-only，不改写）
│   └── backup/                       ←   文档快照（不入库）
```

> ★ **关键机制**：`.workbuddy/memory/MEMORY.md` 会被**自动注入每一次请求**。因此「必须每轮都知道的工程级事实」同时沉淀在本文件与 `MEMORY.md` 中 —— 本文件面向人与跨工具阅读，`MEMORY.md` 负责保证自动化生效。

**尚不存在**（全部由 M0 创建，结构见 `03` §2）：`rust-toolchain.toml`、`.dockerignore`、`docker/`、`scripts/`、`.devcontainer/`、`.vscode/`、`rgoc/`、`tests/`、`xtask/`。
**已创建但为空/待填充**：`docs/contracts/`（空）；`docs/milestones/` 已有 M0 三件套，缺 `M0-manifest.json`。

---

## 2. 文档体系

### 2.1 清单

| # | 文档 | 层级 | 回答什么问题 | 状态 | 规模 |
|---|---|---|---|---|---|
| — | [`docs/README.md`](./docs/README.md) | 索引 | 文档地图是什么 | 已建立 | ≈9 KB |
| 01 | [`docs/01-feature-set.md`](./docs/01-feature-set.md) | 规格 | **要建什么** —— 功能全集 / RTM | 已建立 | ≈46 KB |
| 02 | [`docs/02-test-inventory.md`](./docs/02-test-inventory.md) | 规格 | **如何验证** —— 功能点 → 官方测试用例 | 已建立（9 处修正） | ≈61 KB |
| 03 | [`docs/03-roadmap.md`](./docs/03-roadmap.md) | 计划 | **按什么顺序建** —— M0–M12 迭代计划 | 已修订（v2） | ≈52 KB |
| 04 | [`docs/04-development-environment.md`](./docs/04-development-environment.md) | 环境 | **在哪建** —— Docker 容器方案 | **方案稿（未执行）** | ≈13 KB |
| — | [`docs/milestones/M0-design.md`](./docs/milestones/M0-design.md) | 设计 | **怎么建 M0** —— 决策 D-M0-1~12 / 环境基线 / Phase 0–4 / 门禁 E1–E10 | **已确认** | ≈30 KB |
| — | [`docs/milestones/M0-tests.md`](./docs/milestones/M0-tests.md) | 测试 | **怎么验 M0** —— T-H/T-C/T-S 测试 ID、20 样本、unsupported、超时预算 | 待冻结 | ≈22 KB |
| — | [`docs/milestones/M0-plan.md`](./docs/milestones/M0-plan.md) | 计划 | **怎么干 M0** —— Phase 0–1 的 T01–T28 任务（路径 / 可粘贴内容 / 验证） | 待执行 | ≈40 KB |

**阅读顺序**：01 → 02 → 03 → 04。

**阶段文档**（`docs/milestones/`）——每阶段开工前须产出 **三件套**（`03` §6.2）：

| 件 | 回答什么 | M0 状态 |
|---|---|---|
| `<ID>-design.md` | **做什么、边界在哪** | ✅ 已确认 |
| `<ID>-tests.md` | **怎么算通过** | ⏳ 待冻结 |
| `<ID>-plan.md` + `manifest.json` | **按什么顺序动手** + 环境锁定值 | ⏳ 计划已有（Phase 0–1）；manifest 缺 |

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
| 编译器工程 | `rgoc/` **尚不存在**，无任何 Rust 代码 |
| 开发环境 | 容器镜像、Rust / Go 工具链、`.devcontainer` **均未搭建**（04 明示为方案稿；Docker Desktop 已实测可启动） |
| M0 | **未开工**。**三件套已就绪**：`M0-design.md`（已确认）/ `M0-tests.md`（待冻结）/ `M0-plan.md`（待执行），缺 `M0-manifest.json`。下一步：执行 `M0-plan.md` 的 Phase 0（T01–T19） |
| 9 项测试缺口 | TYP-26、SCP-06、EXP-16、EXP-22、PKG-04、PKG-06、RT-SCH-02、RT-POLY-03、RT-POLY-05 —— 须在 rgoc 自有测试补齐 |
| 阶段目录 | `docs/contracts/` **已创建但为空**；`docs/milestones/` 含 M0 设计与测试清单 |
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

改完文档后跑一遍（期望：① 无 `BROKEN` 行　② 输出 `clean`　③ 两个目录路径都列出）：

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
```

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

**M0 三件套（design / tests / plan）已就绪，可以开工。**

按序执行：

1. **提交当前文档改动**（建议先做）—— 使 Phase 0 每个任务失败时都能干净回退
2. **冻结 `M0-tests.md`** —— §4 的 20 个官方样本与 §6 的 unsupported 清单是开工前的白名单
3. **执行 `M0-plan.md` 的 Phase 0**（T01–T19）：镜像构建 → 工具链验证 → 入口脚本 → 环境 manifest → 基准测试
4. **执行 Phase 1**（T20–T28）：最小 workspace → `.devcontainer` → **T28 实测断点命中（E5）**
5. Phase 1 门禁通过后，**基于实测环境事实再拆 Phase 2–4 的计划**

**M0 的 5 个 Phase**（详见 `M0-design.md` §6）：

| Phase | 内容 | 门禁 | 计划状态 |
|---|---|---|---|
| 0 | 容器与工具链底座 | E1 镜像 digest 可重放、E2 环境值入 manifest、E10 基准 | ✅ T01–T19 |
| 1 | VSCode 调试环境 | **E5 实测断点命中** | ✅ T20–T28 |
| 2 | Rust 工程骨架 + harness | E3 六类自测全绿、E4 20 样本 100% | ⏳ 待拆 |
| 3 | 三个 spike（解释 / SSA / native） | E6 可复现、E7 native `hello` | ⏳ 待拆 |
| 4 | 契约初稿 + 报告 | E8/E9/E10 | ⏳ 待拆 |

**原则**：早期可行性验证、单平台首发、独立可退出的能力切片优先；完整 Go toolchain / runtime 是后续扩展，不是首发承诺。
