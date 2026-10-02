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
| 当前阶段 | **M0 · Phase 0 已完成**（E1/E2/E10 通过）；Phase 1 环境就绪、四条门禁全过，**仅剩 E5（实测断点命中）待人工操作** |
| 仓库 | **Git**，remote `origin` → <https://github.com/nobody0726/rust-go-compiler>（public，分支 `main`） |

**一句话状态**：文档体系（4 篇正文 + 1 索引 + M0 四件套）已建立并互链，**已发布到 GitHub**；**容器镜像 `rgoc:dev`、Go oracle 1.27.1、Rust 1.98.1、`.devcontainer/` 与 `scripts/` 均已落地并实测通过**；`rgoc/` 下只有用于验证调试链路的最小 `rgoc-harness`（1 函数 + 1 测试），**编译器实现尚未开始**。

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
│       ├── M0-plan.md                ←     M0 实施计划 Phase 0–1（**Phase 0 已完成**）
│       ├── M0-benchmarks.md          ←     M0 实测基准（E10 证据；D-M0-9 决策依据）
│       └── M0-manifest.json          ←     机器可读事实（environment + gate + benchmarks）
├── go_source_code/                   ← Go 1.27.1 官方语料，**只读**，**不入库**（185 MB）
├── rust-toolchain.toml               ← Rust 版本唯一来源（1.98.1）
├── .dockerignore                     ← 构建上下文收敛（镜像只需要 Dockerfile + toolchain 文件）
├── docker/                           ← 镜像定义
│   ├── Dockerfile                    ←     唯一镜像定义（6 层，含架构断言与登录 shell 修正）
│   └── image.lock                    ←     镜像锁定信息（E1；含「image id 不可复现」的说明）
├── scripts/                          ← 入口脚本（5 个，全部是「以后还用得到」的）
│   ├── in-container.sh               ←     统一容器入口（daemon 探测 + 卷 bootstrap + 参数透传）
│   ├── check-m0-consistency.py       ←     M0 一致性自检（52 条断言，退出码即结论）
│   ├── install-codelldb.sh           ←     CodeLLDB【平台包】离线安装（绕开宿主下发的死代理）
│   ├── install-vscode-server.sh      ←     VS Code Server 离线安装进持久卷 /vscode（宿主升级 VSCode 后用）
│   └── debug-smoke-test.sh           ←     无头调试链路冒烟测试（E5 的下层证据；第 2 节 A/B/C + 9 项断言）
├── .devcontainer/devcontainer.json   ← VSCode 调试环境（与门禁同镜像，D-M0-10）
├── .vscode/launch.json               ← CodeLLDB 调试配置（2 个：当前测试 / 全部测试）
├── rgoc/                             ← Rust workspace
│   ├── Cargo.toml                    ←     resolver 3 / edition 2024 / 全局 lint
│   └── crates/rgoc-harness/          ←     测试基础设施（**Phase 1 只有调试目标**）
├── .workbuddy/
│   ├── memory/
│   │   ├── MEMORY.md                 ←   项目长期事实（★每次请求自动注入）
│   │   └── YYYY-MM-DD.md             ←   按日工作日志（append-only，不改写）
│   └── backup/                       ←   快照（不入库）：文档历史 + 已移除脚本（`scripts-removed-*/`）
```

> ★ **关键机制**：`.workbuddy/memory/MEMORY.md` 会被**自动注入每一次请求**。因此「必须每轮都知道的工程级事实」同时沉淀在本文件与 `MEMORY.md` 中 —— 本文件面向人与跨工具阅读，`MEMORY.md` 负责保证自动化生效。

**尚不存在**（由 M0 后续阶段创建，结构见 `03` §2）：`tests/`、`xtask/`，以及 `rgoc/crates/` 下除 `rgoc-harness` 之外的全部 crate（lexer / parser / sema / hir / mir / ssa / codegen …）。
**已创建但为空/待填充**：`docs/contracts/`（空，M0 Phase 4 产出 5 份初稿）。

---

## 2. 文档体系

### 2.1 清单

| # | 文档 | 层级 | 回答什么问题 | 状态 | 规模 |
|---|---|---|---|---|---|
| — | [`docs/README.md`](./docs/README.md) | 索引 | 文档地图是什么 | 已建立 | ≈12 KB |
| 01 | [`docs/01-feature-set.md`](./docs/01-feature-set.md) | 规格 | **要建什么** —— 功能全集 / RTM | 已建立 | ≈45 KB |
| 02 | [`docs/02-test-inventory.md`](./docs/02-test-inventory.md) | 规格 | **如何验证** —— 功能点 → 官方测试用例 | 已建立（9 处修正） | ≈60 KB |
| 03 | [`docs/03-roadmap.md`](./docs/03-roadmap.md) | 计划 | **按什么顺序建** —— M0–M12 迭代计划 | 已修订（v2） | ≈51 KB |
| 04 | [`docs/04-development-environment.md`](./docs/04-development-environment.md) | 环境 | **在哪建** —— Docker 容器方案 | **已落地**（Phase 0 实测通过） | ≈13 KB |
| — | [`docs/milestones/M0-design.md`](./docs/milestones/M0-design.md) | 设计 | **怎么建 M0** —— 决策 D-M0-1~12 / 环境基线 / Phase 0–4 / 门禁 E1–E10 | **已确认** | ≈30 KB |
| — | [`docs/milestones/M0-tests.md`](./docs/milestones/M0-tests.md) | 测试 | **怎么验 M0** —— T-H/T-C/T-S 测试 ID、20 样本、unsupported、超时预算 | 待冻结 | ≈21 KB |
| — | [`docs/milestones/M0-plan.md`](./docs/milestones/M0-plan.md) | 计划 | **怎么干 M0** —— Phase 0–1 的 T01–T28 任务（路径 / 可粘贴内容 / 验证） | **Phase 0 已完成** | ≈66 KB |
| — | [`docs/milestones/M0-benchmarks.md`](./docs/milestones/M0-benchmarks.md) | 实测 | **凭什么是这样** —— 时间/体积/冷启动/挂载布局/可复现性/环境陷阱 + **四则调试环境案例**（§7 平台包 / §8 DWARF / §9 Server / §10 cargo 启动配置） | **已产出** | ≈37 KB |

**阅读顺序**：01 → 02 → 03 → 04。

**阶段文档**（`docs/milestones/`）——每阶段开工前须产出 **三件套**（`03` §6.2）：

| 件 | 回答什么 | M0 状态 |
|---|---|---|
| `<ID>-design.md` | **做什么、边界在哪** | ✅ 已确认 |
| `<ID>-tests.md` | **怎么算通过** | ⏳ 待冻结 |
| `<ID>-plan.md` + `manifest.json` | **按什么顺序动手** + 环境锁定值 | ⏳ 计划已有（Phase 0–1）；manifest 已建立（`environment` + `gate`） |

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
| 编译器工程 | `rgoc/` 下**只有 `rgoc-harness`**（1 个函数 + 1 个测试），作用是 Phase 1 的调试目标；**编译器实现尚未开始** |
| 开发环境 | **已就绪**：镜像 `rgoc:dev`（`sha256:21f55802…`，2.92GB）、Go oracle `go1.27.1 linux/arm64`、Rust `1.98.1`、clang 14、CodeLLDB 1.12.3（自带 lldb 22.1.8-codelldb）、`.devcontainer/` 与 `scripts/` 全部落地并实测通过 |
| M0 | **Phase 0 已完成**（T01–T19；E1/E2/E10 通过，证据见 `M0-manifest.json` 的 `gate` 与 `M0-benchmarks.md`）。**Phase 1 只剩 E5**：T23–T26 的文件与断言已就绪，T27（Reopen in Container）/ T28（**实测断点命中**）需人工在 VSCode 中完成 |
| 已知环境约束 | ① 所有构建与测试**必须**在容器内（`scripts/in-container.sh`）；② `rgoc/target/` 在命名卷 `rgoc-target`，故 `cargo clean` 会 `EBUSY` —— 清空用 `find rgoc/target -mindepth 1 -delete`；③ 镜像 **不可位级复现**，钉子只有 `base.index_digest` + `src.*_sha256`，**image id 不得写进门禁**；④ **宿主 VSCode 的 `http.proxy` 会被下推进容器**（经 AHP `root/configChanged`），容器内 `127.0.0.1` 指向自己 → 一切走 VSCode 网络栈的下载都会失败。**远端 Machine settings 覆盖不了它**；CodeLLDB 平台包用 `scripts/install-codelldb.sh` 离线装（见 `M0-benchmarks.md` §7）；⑤ **每次升级宿主 VSCode 都可能让窗口连不上容器** —— commit 变了而持久卷 `/vscode` 里没有新 server，Dev Containers 便去宿主侧下载（`Path: /var/folders/…`）再被死代理挡住。**报错文字与 ④ 一模一样但层级不同**，按 `Path:` 辨异，修法是 `scripts/install-vscode-server.sh`（见 §9）；⑥ **容器重建后 CodeLLDB 平台包必丢**（`~/.vscode-server/extensions/` 不在任何卷里），重跑 `install-codelldb.sh`；⑦ **CodeLLDB 的 cargo 启动配置有两个静默陷阱**（`launch.json` 里写错不报错，只在按 F5 时以 `Cargo command did not complete successfully.` 出现）：`cargo` 的工作目录取自 **`cargo.cwd`（不读顶层 `cwd`）**，漏写就回退到 `/work`（无 `Cargo.toml`）→ cargo 退出 **101**；`filter.name` 比对的是 **cargo 的 target name（下划线）而非包名（连字符）**，写错会 0 匹配。真错在 **OUTPUT → LLDB** 通道的 `Cargo exited with code N`，**不在 VSCode 弹出的那个提示里**；且**不能用 shell 复现**那条命令（CodeLLDB 是无 `shell: true` 的 `spawn`，shell 会剥掉 `target.'cfg(all())'` 的单引号 → 假的 TOML 报错）。见 `M0-benchmarks.md` §10，回归由 `debug-smoke-test.sh` 第 2 节守住（该节按 `launch.json` 原样复刻 CodeLLDB 的 cargo 步骤） |
| 9 项测试缺口 | TYP-26、SCP-06、EXP-16、EXP-22、PKG-04、PKG-06、RT-SCH-02、RT-POLY-03、RT-POLY-05 —— 须在 rgoc 自有测试补齐 |
| 阶段目录 | `docs/contracts/` **已创建但为空**；`docs/milestones/` 含 M0 四件套 + `M0-manifest.json` |
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

**Phase 0 已完成；Phase 1 只剩 E5 这一项人工操作。**

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
1. **完成 E5**（人工，唯一阻塞项）：在 VSCode 中打开本仓库 → `Dev Containers: Reopen in Container`
   → 在 `rgoc/crates/rgoc-harness/src/lib.rs` 的 `double_sum()` 里 `let sum = a + b;`（第 27 行）下断点
   → 按 F5 跑 **「调试当前测试 (CodeLLDB)」** → 逐项核对 `M0-plan.md` T28 的四项检查表。
   **注意**：第 4 项（单步后 `sum == 3`）依赖调试目标的形状 —— 尾位置直接返回的 `let` 绑定
   不会进 DWARF，换目标前先读 `M0-benchmarks.md` §8。
   **如果 F5 又弹出 `Cargo command did not complete successfully.`**：那不是环境层的问题，
   而是 CodeLLDB 的 **cargo 启动配置**（`cargo.cwd` / `filter.name`，两者都是**静默**陷阱，
   VSCode 的提示里不会指向它们）。真错在 **OUTPUT → LLDB** 通道里的 `Cargo exited with code N`，
   不在弹出的那个提示里 —— 两个坑、复现方法与修法见 `M0-benchmarks.md` §10。
   **别用 shell 去复现那条 cargo 命令**：CodeLLDB 是不经 shell 的 `spawn`，shell 会剥掉
   `target.'cfg(all())'` 上的单引号，给你一个**假的** TOML 报错。
2. 把 E5 的四项结果填入 `docs/milestones/M0-manifest.json` 的 `gate.E5`
3. **冻结 `M0-tests.md`** —— §4 的 20 个官方样本与 §6 的 unsupported 清单是 Phase 2 开工前的白名单
4. 基于**实测到的环境事实**拆 **Phase 2–4** 的计划（`M0-plan.md` §0.1 已说明为何此时才拆）

**M0 的 5 个 Phase**（详见 `M0-design.md` §6）：

| Phase | 内容 | 门禁 | 计划状态 |
|---|---|---|---|
| 0 | 容器与工具链底座 | E1 镜像 digest 可重放、E2 环境值入 manifest、E10 基准 | ✅ **已完成**（T01–T19；E1/E2/E10 通过） |
| 1 | VSCode 调试环境 | **E5 实测断点命中** | ⏳ T20–T22 ✅（四条门禁全过）/ T23–T26 ✅ / **T27–T28 待人工** |
| 2 | Rust 工程骨架 + harness | E3 六类自测全绿、E4 20 样本 100% | ⏳ 待拆 |
| 3 | 三个 spike（解释 / SSA / native） | E6 可复现、E7 native `hello` | ⏳ 待拆 |
| 4 | 契约初稿 + 报告 | E8/E9/E10 | ⏳ 待拆 |

**原则**：早期可行性验证、单平台首发、独立可退出的能力切片优先；完整 Go toolchain / runtime 是后续扩展，不是首发承诺。
