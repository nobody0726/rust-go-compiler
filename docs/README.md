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
| M0 | [`milestones/M0-tests.md`](./milestones/M0-tests.md) | **怎么验 M0** —— 测试 ID（T-H/T-C/T-S1~S3）、20 个官方样本清单、unsupported 清单 U1–U14、超时预算 | **已冻结**（T29，2026-10-02；§5.1 另有**修订 R1**） |
| M0 | [`milestones/M0-plan.md`](./milestones/M0-plan.md) | **怎么干 M0** —— **Phase 0–4 的 T01–T55**（文件路径 / 可粘贴内容 / 验证命令） | ✅ **全部完成**（T01–T55，2026-10-07） |
| M0 | [`milestones/M0-benchmarks.md`](./milestones/M0-benchmarks.md) | **实测依据** —— 镜像时间/体积、冷启动、编译、`target/` 挂载布局对比、可复现性结论、环境陷阱清单、调试环境四则案例（CodeLLDB 平台包 / DWARF 变量条目 / VS Code Server / cargo 启动配置） | **已产出**（E10 证据） |
| M0 | [`milestones/M0-manifest.json`](./milestones/M0-manifest.json) | **机器可读事实** —— `environment`（E2）+ `gate`（**E1–E10 十条**）+ `test_ids`（37 条）+ `unsupported`（U1–U14）+ `budget` + `test_contract` + `phase_plan` + `benchmarks` | ✅ **已填实**（十条门禁全部 `pass`） |
| M0 | [`milestones/M0-report.md`](./milestones/M0-report.md) | **交接文档** —— 三个命题各由哪个测试 ID 证明、门禁证据出处、spike 留下的差距、未验证范围（U1–U14）、**接手指南** | ✅ **已产出**（T53） |
| M0 | [`contracts/C1`–`C5`](./contracts/) | **五份契约** —— C1 留位 / **C2 完整初稿** / C3–C5 spike 级 | ✅ **已产出**（T48–T52 / E8） |

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
| `docs/contracts/` | 03 §2、§1.3 | 跨阶段接口与版本化契约 | ✅ **5 份初稿已产出**（C1 留位 / **C2 完整初稿** / C3–C5 spike 级，T48–T52 / E8） |
| `docs/milestones/` | 03 §6.2 | 每阶段 `<阶段ID>-{design,tests,plan}.md` 与机器可读 manifest | ✅ **M0 四件套全部就位**：`M0-design.md`（已确认）、`M0-tests.md`（**已冻结** T29，§5.1 另有修订 R1）、`M0-plan.md`（**T01–T55 全部完成**）、`M0-benchmarks.md`（§1–§12）、`M0-report.md`（交接文档）、`M0-manifest.json`（**E1–E10 十条** + `test_ids` + `unsupported` + `budget` 已填实） |

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
- `milestones/M0-tests.md` —— **测试先行清单，已于 2026-10-02 冻结**（T29）。§4 的 20 个官方样本、§6 的 unsupported 清单（U1–U14，冻结时补了 `skip` 与 `linkmain.go` 两类）、§7 的超时上限均已锁定；**§6.1 记录了 M0 分母 = 279**（`run` 147 / `errorcheck` 120 / `compile` 12，含 5 个平台过滤项）—— T33 的 Rust 枚举器必须算出同一个数。其 §1.1 记录了一处**事实修正**：官方驱动器是 `src/cmd/internal/testdir/testdir_test.go`（**非** `test/run.go`），指令集为 **16 个**（非 7 个）—— 应回写至 `02-test-inventory.md`。
- `milestones/M0-plan.md` —— **Phase 0–1 的实施计划**（T01–T28）。两点需注意：① T20–T22 把「最小 Rust 工程骨架」提前到 Phase 1，作为 E5 断点调试的目标（理由见该文档 §0.2）；② **E5 的唯一判定方式是 T28 的实测断点命中**，不是「能打开容器窗口」—— 该门禁已于 2026-10-02 由用户按 F5 实测确认，登记在 `M0-manifest.json` 的 `gate.E5`。③ **Phase 2–4 的计划已于 2026-10-02 拆完**（T29–T55），拆解时定了两条决策：**D-M0-13** crate 严格按当期需要建、**D-M0-14** 三个 spike 放独立 crate `rgoc-spikes`；并修正了 D-M0-15（`double_sum` 不删，它是 E5 的人工复验锚点）。
- `milestones/M0-benchmarks.md` —— **实测基准与决策依据**。§4 是 D-M0-9（`target/` 放 bind mount 还是命名卷）的判定数据（命名卷快 2.3×）；§5 记录「镜像不可位级复现」这一结论，并据此调整了 E1 的判定方式；§6 是环境陷阱清单（计时器、`pipefail`、Rust 关键字、宿主下发的死代理等 **14 项**）；**§7 是 CodeLLDB「无法下载」的完整因果链**（6 条证据，含「远端设置覆盖无效」的反证）与修法 `scripts/install-codelldb.sh`；**§8 是 rustc 不为「尾位置直接返回的 `let` 绑定」生成 DWARF 变量条目**这一发现（三变体对照实验），它决定了 T28 调试目标的形状；**§9 是 VS Code Server「无法下载」** —— 与 §7 **报错文字相同、层级不同**（前者在宿主侧、连接容器之前），给出按 `Path:` 辨异的判据、持久卷 `/vscode` 的两处 `test -d` 布局、实测数据（204 MB / 9.2–10.5 MB/s / sha256 两次逐字节一致）与修法 `scripts/install-vscode-server.sh`；**§10 是 F5 报「Cargo command did not complete successfully.」** —— CodeLLDB 的 cargo 工作目录取自 **`cargo.cwd`**（**不读顶层 `cwd`**）且 `filter.name` 要比 **target name**（下划线，非包名），两处缺陷都在首次 F5 才暴露，含三次复现与一条假线索（用 shell 复现会因为剥掉单引号得到**假的** TOML 报错）；同节末尾给出这条链路的**三层回归**（静态断言 / 冒烟测试第 2 节按 `launch.json` 原样复刻 / 变异测试注入后必须报 ✗）。
- **`M0` 环境已就绪，Phase 0 已完成**：镜像 `rgoc:dev`（`sha256:21f55802…`，2.92GB）、Go oracle `go1.27.1 linux/arm64`、Rust `1.98.1`、CodeLLDB 1.12.3（自带 lldb 22.1.8-codelldb）、`.devcontainer/` 与 `scripts/` 均已落地；E1/E2/E10 三项门禁通过，四条统一退出检查（`fmt`/`check`/`clippy`/`test`）全过。
- **M0 的四项门禁（E1/E2/E10/E5）已全部通过**。最后一项 **E5（实测断点命中）** 于 2026-10-02 由用户在 VSCode dev container 中按 F5 实测确认（T28 检查表四项逐项通过：断点命中未被跳过 / 变量面板 `a=1,b=2` / 调用栈 ≥2 帧 / F10 后停在第 28 行且 `sum==3`），登记在 `milestones/M0-manifest.json` 的 `gate.E5`（含 `confirmed_at` / `confirmed_by`）。**下层证据与之互相印证**：`scripts/debug-smoke-test.sh` 第 2 节 **A/B/C 三项 + 9 项断言全部通过**（第 2 节按 `.vscode/launch.json` 原样复刻 CodeLLDB 的 cargo 步骤：cwd 来自 `cargo.cwd`、退出码 0、`filter` 恰好选中 1 个产物；随后是断点解析 / 命中 / 调用栈 / 形参 / 单步 / 停止行 / 中间值 `sum = 3` / 测试结束）。**至此 Phase 0 与 Phase 1 完成**；Phase 2 开工前尚需**冻结 `M0-tests.md`** 并拆 Phase 2–4 的计划。**任何编译器实现仍未开始** —— `rgoc/` 下只有用于验证调试链路的最小 `rgoc-harness`（1 个函数 + 1 个测试），Phase 2 引入真实功能时删除。
- **编辑器链路的三个环境前提已就位**（2026-10-02，实测）：① 宿主 VSCode 1.140.0（commit `07f806f9…`）的 **VS Code Server** 已装入持久卷 `/vscode`（601 MB，Dev Containers 的两处 `test -d` 均 exit=0，**不会再发起任何下载**，见 §9）；② CodeLLDB **平台包**已装好（`platform.ok` 存在，`lldb 22.1.8-codelldb` 可用，见 §7）；③ `.vscode/launch.json` 的两处缺陷已修（`cargo.cwd` 与 `filter.name`，见 §10）。注意前两者都会**各自复发**：宿主升级 VSCode → 重跑 `install-vscode-server.sh`；容器重建 → 重跑 `install-codelldb.sh`。
- 03 的计划文本修订**不代表**环境安装、spike 或编译器实现已经完成（见 03 §9 说明）。
