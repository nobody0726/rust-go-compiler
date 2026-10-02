# 文档索引

> Rust 重写 Go 编译器的文档地图。**项目代号**：`rgoc`　|　**语料版本**：`go1.27.1`（本地快照）
> **首发平台**：Linux / arm64（Lima VM）→ `aarch64-unknown-linux-gnu` / ELF

---

## 1. 文档地图

| 序号 | 文档 | 层级 | 角色（一句话） | 状态 |
|---|---|---|---|---|
| 01 | [`01-feature-set.md`](./01-feature-set.md) | 规格层 | **要建什么** —— Go 编译器功能全集 / RTM，246 个可追踪功能点 | 已建立 |
| 02 | [`02-test-inventory.md`](./02-test-inventory.md) | 规格层 | **如何验证** —— 功能点 → 官方测试用例映射 | 已建立 |
| 03 | [`03-roadmap.md`](./03-roadmap.md) | 计划层 | **按什么顺序建** —— M0–M12 迭代计划（v2） | 已修订 |
| 04 | [`04-development-environment.md`](./04-development-environment.md) | 环境层 | **在哪建** —— Lima Linux VM 开发环境方案 | 方案稿 |

**阅读顺序**：01 → 02 → 03 → 04。先看「要建什么」，再看「怎么验证」，然后看「分几步建」，最后看「在什么环境里建」。

---

## 2. 依赖关系

```text
01-feature-set.md      ─┐
（功能 ID 全集）          ├──► 03-roadmap.md ──► docs/milestones/<阶段ID>-tests.md
02-test-inventory.md   ─┘        （迭代计划）        （M0 起逐阶段产出，见 03 §6.2）
（功能 ID → 测试线索）

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
- §0.2 记录了相对既有文档的 **6 处已核实事实修正**（如 `syntax/testdata/` 实为 31 个文件而非 29 个）。
- §14 统计：约 **236/246** 功能点有专属测试；**9 项**无专属测试，须在 `rgoc` 自有测试中补齐（清单见 03 §5）。

### 03 `03-roadmap.md` —— 计划层
- 四条交付线 A（前端）/ B（顺序解释）/ C（native MVP）/ D（能力扩展）；**首个 native 交付是 C，不以 D 完成为前置**。
- 里程碑 **M0–M12**，每个里程碑与独立验收子阶段均有各自的测试门禁与退出条件。
- 完成度按 **Frontend / Interpreter / Native / Runtime-metadata** 四维分别跟踪，不再以「246 项全勾选」为总退出条件。
- §6.2 约定：每个阶段开工前须产出 `docs/milestones/<阶段ID>-tests.md`。

### 04 `04-development-environment.md` —— 环境层
- 统一在 **Lima 管理的 arm64 Linux VM** 内完成开发、测试与首发 native 验证；不维护 macOS 第二套首发环境。
- §9 列出需在 M0 拍板的 7 项待决事项；§7 为风险与切断条件表。

---

## 4. 命名规范

- 顶层文档命名格式：`NN-<kebab-topic>.md`
  - `NN` 为两位序号，体现**层级与阅读顺序**（01/02 规格 → 03 计划 → 04 环境）。
  - 主题部分小写、连字符分词，便于跨平台文件系统与 URL 引用。
- `README.md` 为本索引，固定名称，不参与编号。
- 子目录文档不参与顶层编号，按各自约定命名。

---

## 5. 规划中的目录（尚不存在）

以下目录由 `03-roadmap.md` 规划，**M0 阶段才会创建**，当前不代表已有产出：

| 目录 | 来源 | 用途 |
|---|---|---|
| `docs/contracts/` | 03 §2 | 跨阶段接口与版本化契约 |
| `docs/milestones/` | 03 §6.2 | 每阶段 `<阶段ID>-tests.md` 测试先行清单与机器可读 manifest |

---

## 6. 文档状态说明

- 本仓库**当前不是 Git 仓库**。语料快照的一致性由**排序后的源码路径 / 文件 SHA-256 清单**锁定（见 03 §0.2），`VERSION` 文件本身不足以证明内容一致。
- 04 为**方案稿**：Lima、Linux VM、Rust/Go 工具链、环境 manifest 与任何编译器实现均**尚未**因该文档而搭建或执行。
- 03 的计划文本修订**不代表**环境安装、spike 或编译器实现已经完成（见 03 §9 说明）。
