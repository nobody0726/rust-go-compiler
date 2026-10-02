# 项目长期记忆 —— rust_go_compiler

## 项目概况

- 目标：用 **Rust 重写 Go 编译器**，项目代号 **`rgoc`**
- 语料快照：`go_source_code/`（`VERSION` = `go1.27.1`，2026-08-28）；规格基准 `doc/go_spec.html`
- 首发平台：**Linux / arm64（Lima VM 提供）→ `aarch64-unknown-linux-gnu` / ELF**；不维护 macOS 第二套首发环境
- 工作区**不是 Git 仓库**：语料一致性靠排序后的源码路径 / 文件 SHA-256 清单锁定，`VERSION` 文件本身不足以证明内容一致

## 文档体系（docs/）

**工程入口**：工作区根目录 **`AGENTS.md`** —— 任何任务开始前先读它。含：30 秒速览、目录地图、文档清单与状态、工作纪律（Superpowers TDD / 四维完成度 / 硬约束）、变更同步清单、文档自检命令、下一步。本文件（MEMORY.md）保留精简摘要以保证自动注入生效。

**命名规范**：顶层文档 `NN-<kebab-topic>.md`，NN 体现层级与阅读顺序；`README.md` 为索引，固定名不编号。

| 序号 | 文档 | 层级 | 角色 |
|---|---|---|---|
| — | `README.md` | 索引 | 文档地图、依赖关系、命名规范 |
| 01 | `01-feature-set.md` | 规格 | **要建什么** —— 功能全集 / RTM，246 个功能点（Closure Coverage 三维交叉验证） |
| 02 | `02-test-inventory.md` | 规格 | **如何验证** —— 功能点 → 官方测试用例映射 |
| 03 | `03-roadmap.md` | 计划 | **按什么顺序建** —— M0–M12 迭代计划 v2 |
| 04 | `04-development-environment.md` | 环境 | **在哪建** —— Lima Linux VM 方案 |

**依赖方向**：01 + 02 → 03（03 §0 声明二者为其「输入规格」）→ `docs/milestones/<阶段ID>-tests.md`；04 落实 03 §0.2 的平台约束。

**规划中但尚不存在的目录**：`docs/contracts/`（03 §2）、`docs/milestones/`（03 §6.2）—— **M0 才创建**。

**引用约定**（定义在 `01-feature-set.md` §0.4）：`spec:Lxx-yy` → `doc/go_spec.html`（规范性）；`nodes:Lxx-yy` → `syntax/nodes.go`（规范性）；`runtime/*.go:NNN`、`abi/type.go:NNN`、`gc/*/`、`link/ld/`（参考性）。

## 关键方法论约定

- **Superpowers TDD**：行为切片一律 RED → GREEN → REFACTOR；先确认失败来自缺失行为而非环境/harness，再最小实现
- 完成度按 **四维**（Frontend / Interpreter / Native / Runtime-metadata）分别跟踪，**不以「246 项全勾选」作为总退出条件**
- 每个阶段及独立验收子阶段开工前须产出 `docs/milestones/<阶段ID>-tests.md` + 机器可读 manifest
- 提交仅在明确要求时执行；提交信息引用功能 ID / 验证证据
- 只读官方语料，**不改动** `go_source_code/`

## 重要事实

- `go_source_code/test/` 与 `src/internal/types/testdata/` 是**语言级黑盒测试**，只依赖编译器对外行为，可直接作为 Rust 重写实现的验收测试集
- `test/` 下不是 `_test.go`，而是由 `test/run.go` 按首行指令（`// run`、`// errorcheck`、`// compile`、`// runoutput`、`// rundir`、`// errorcheckdir`、`// asmcheck`）驱动的独立程序
- 02-test-inventory §0.2 记录了 6 处对既有文档的事实修正（如 `syntax/testdata/` 实为 31 个文件），**以该节为准**
