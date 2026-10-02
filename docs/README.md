# 文档索引

> Rust 重写 Go 编译器的文档地图。**项目代号**：`rgoc`　|　**语料版本**：`go1.27.1`（本地快照）
> **首发平台**：Linux / arm64（Docker 容器）→ `aarch64-unknown-linux-gnu` / ELF

---

## 1. 文档地图

| 序号 | 文档 | 层级 | 角色（一句话） | 状态 |
|---|---|---|---|---|
| 01 | [`01-feature-set.md`](./01-feature-set.md) | 规格层 | **要建什么** —— Go 编译器功能全集 / RTM，246 个可追踪功能点 | 已建立 |
| 02 | [`02-test-inventory.md`](./02-test-inventory.md) | 规格层 | **如何验证** —— 功能点 → 官方测试用例映射 | 已建立 |
| 03 | [`03-roadmap.md`](./03-roadmap.md) | 计划层 | **按什么顺序建** —— M0–M12 迭代计划（v2） | 已修订 |
| 04 | [`04-development-environment.md`](./04-development-environment.md) | 环境层 | **在哪建** —— Docker 容器开发环境方案 | 方案稿 |

**阅读顺序**：01 → 02 → 03 → 04。先看「要建什么」，再看「怎么验证」，然后看「分几步建」，最后看「在什么环境里建」。

**阶段文档**（`docs/milestones/`，每阶段开工前产出，见 §5）：

| 阶段 | 文档 | 角色 | 状态 |
|---|---|---|---|
| M0 | [`milestones/M0-design.md`](./milestones/M0-design.md) | **怎么建 M0** —— 设计决策 D-M0-1~12、环境基线、Phase 0–4、门禁 E1–E10 | **已确认** |
| M0 | [`milestones/M0-tests.md`](./milestones/M0-tests.md) | **怎么验 M0** —— 测试 ID（T-H/T-C/T-S1~S3）、20 个官方样本清单、unsupported 清单、超时预算 | 待冻结 |
| M0 | [`milestones/M0-plan.md`](./milestones/M0-plan.md) | **怎么干 M0** —— Phase 0–1 的 T01–T28 任务（文件路径 / 可粘贴内容 / 验证命令） | 待执行 |

**三件套的分工**：`design` 定**做什么与边界** → `tests` 定**怎么算通过** → `plan` 定**按什么顺序、以什么粒度动手**。三者齐备才开工（03 §6.2）。

---

## 2. 依赖关系

```text
01-feature-set.md      ─┐
（功能 ID 全集）          ├──► 03-roadmap.md ──► docs/milestones/<阶段ID>-design.md
02-test-inventory.md   ─┘        （迭代计划）      ├─► <阶段ID>-tests.md
（功能 ID → 测试线索）                            ├─► <阶段ID>-plan.md
                                                 └─► <阶段ID>-manifest.json
                                        （M0 起逐阶段产出，见 03 §6.2）

04-development-environment.md ──► 落实 03-roadmap.md §0.2 的平台与工具链约束
```

- `03-roadmap.md` §0 明确声明 **01 与 02 是其输入规格**：01 提供功能 ID，02 提供测试线索。两者是**追踪输入**，不等于已实现或可直接复用。
- `02-test-inventory.md` 的 `spec` / `nodes` 引用含义定义在 `01-feature-set.md` §0.4。
- `04-development-environment.md` §10 是对 `03-roadmap.md` 平台约束的落实与细化。

---

## 3. 各文档要点

### 01 `01-feature-set.md` —— 规格层
- 方法论：**Closure Coverage**（Spec 语义原子 × AST 节点对账 × Runtime 抽象内核，三维交叉验证）。
- 结构：§0 追踪体系（ID 编码规则、覆盖率标准、**§0.4 引用路径约定**）、§1 Spec 语义原子、§2 AST 对账、§3 Runtime 五内核、§4 PIPE 流水线、§5 语言版本矩阵、§6 验收清单。
- 引用前缀定义（详见 §0.4）：`spec:Lxx-yy` → `doc/go_spec.html`（**规范性**）；`nodes:Lxx-yy` → `syntax/nodes.go`（**规范性**）；`runtime/*.go:NNN`、`abi/type.go:NNN`、`gc/*/`、`link/ld/`（**参考性**）。

### 02 `02-test-inventory.md` —— 规格层
- 把 01 的 246 个功能点逐一映射到 `go_source_code/` 下真实存在的测试文件与测试函数。
- §0.2 记录了相对既有文档的 **9 处已核实事实修正**（如 `syntax/testdata/` 实为 31 个文件而非 29 个；官方驱动器是 `src/cmd/internal/testdir/testdir_test.go` 而非 `test/run.go`；`test/` 顶层实为 356 个 .go 而非 ~600；驱动指令实为 16 个而非 8 个）。
- §14 统计：约 **236/246** 功能点有专属测试；**9 项**无专属测试，须在 `rgoc` 自有测试中补齐（清单见 03 §5）。

### 03 `03-roadmap.md` —— 计划层
- 四条交付线 A（前端）/ B（顺序解释）/ C（native MVP）/ D（能力扩展）；**首个 native 交付是 C，不以 D 完成为前置**。
- 里程碑 **M0–M12**，每个里程碑与独立验收子阶段均有各自的测试门禁与退出条件。
- 完成度按 **Frontend / Interpreter / Native / Runtime-metadata** 四维分别跟踪，不再以「246 项全勾选」为总退出条件。
- §6.2 约定：每个阶段开工前须产出 `docs/milestones/<阶段ID>-design.md`（设计）、`<阶段ID>-tests.md`（测试先行清单）与机器可读 manifest；设计确认后用 `<阶段ID>-plan.md` 承载 2–5 分钟粒度的实施任务。

### 04 `04-development-environment.md` —— 环境层
- 统一在 **Docker 管理的 arm64 Linux 容器**内完成开发、测试与首发 native 验证；不维护 macOS 第二套首发环境。
- §9 列出需在 M0 拍板的 7 项待决事项；§7 为风险与切断条件表。

---

## 4. 命名规范

- 顶层文档命名格式：`NN-<kebab-topic>.md`
  - `NN` 为两位序号，体现**层级与阅读顺序**（01/02 规格 → 03 计划 → 04 环境）。
  - 主题部分小写、连字符分词，便于跨平台文件系统与 URL 引用。
- `README.md` 为本索引，固定名称，不参与编号。
- 子目录文档不参与顶层编号，按各自约定命名。

---

## 5. 阶段文档目录（M0 起启用）

`docs/contracts/` 与 `docs/milestones/` 由 `03-roadmap.md` 规划，**M0 已创建**：

| 目录 | 来源 | 用途 | 当前内容 |
|---|---|---|---|
| `docs/contracts/` | 03 §2、§1.3 | 跨阶段接口与版本化契约 | **空** —— 待 M0 Phase 4 产出 5 份初稿（见 `milestones/M0-design.md` §7） |
| `docs/milestones/` | 03 §6.2 | 每阶段 `<阶段ID>-{design,tests,plan}.md` 与机器可读 manifest | `M0-design.md`（已确认）、`M0-tests.md`（待冻结）、`M0-plan.md`（待执行）；`M0-manifest.json` 由 T15 产出 |

**每阶段开工前置**（03 §6.2）——三样齐备才能进入该阶段：

1. `<阶段ID>-design.md` —— 设计决策与边界（**做什么**）
2. `<阶段ID>-tests.md` —— 必需测试 ID、比较器、验证命令、暂不支持项（**怎么算通过**）
3. `<阶段ID>-plan.md` + 机器可读 manifest —— 2–5 分钟粒度任务（**按什么顺序动手**）+ 环境锁定值

> 这些产出**不代表**任何编译期实现已完成。

---

## 6. 文档状态说明

- 本仓库是 **Git 仓库**，remote `origin` → <https://github.com/nobody0726/rust-go-compiler>（public，分支 `main`）。语料快照的一致性由**排序后的源码路径 / 文件 SHA-256 清单**（`corpus-manifest.sha256`）锁定（见 03 §0.2），`VERSION` 文件本身不足以证明内容一致；`go_source_code/` **不入库**，获取与校验步骤见 03 §4 M0 与该清单。
- 04 为**方案稿**：容器镜像、工具链、环境 manifest、`.devcontainer` 与任何编译器实现均**尚未**因该文档而搭建或执行（Docker Desktop 已实测可启动，见 04 §3.1）。
- `milestones/M0-design.md` —— **已确认**（D-M0-1 ~ D-M0-12 全部决策，2026-10-02）。其 §3.1 的环境实测值（Docker 29.6.2、内核 `6.12.76-linuxkit`、`aarch64`、10 CPU / 7.75 GiB、基础镜像 digest、宿主机无 Rust 工具链、VSCode 缺 `rust-analyzer` / CodeLLDB）来自**只读侦察**，用于让设计基于真实数据。
- `milestones/M0-tests.md` —— **测试先行清单**。§4 的 20 个官方样本与 §6 的 unsupported 清单**须在开工前冻结**。其 §1.1 记录了一处**事实修正**：官方驱动器是 `src/cmd/internal/testdir/testdir_test.go`（**非** `test/run.go`），指令集为 **16 个**（非 7 个）—— 应回写至 `02-test-inventory.md`。
- `milestones/M0-plan.md` —— **Phase 0–1 的实施计划**（T01–T28），**待执行**。两点需注意：① T20–T22 把「最小 Rust 工程骨架」提前到 Phase 1，作为 E5 断点调试的目标（理由见该文档 §0.2）；② **E5 的唯一判定方式是 T28 的实测断点命中**，不是「能打开容器窗口」。Phase 2–4 的计划待 Phase 1 门禁通过后再拆。
- **`M0` 整体尚未开工**：设计确认、测试清单与实施计划产出**都不代表**环境已就绪 —— 容器镜像、工具链、`.devcontainer` 与任何编译器实现均**未搭建**。
- 03 的计划文本修订**不代表**环境安装、spike 或编译器实现已经完成（见 03 §9 说明）。
