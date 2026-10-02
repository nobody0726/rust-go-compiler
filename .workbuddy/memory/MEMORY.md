# 项目长期记忆 —— rust_go_compiler

## 项目概况

- 目标：用 **Rust 重写 Go 编译器**，项目代号 **`rgoc`**
- 语料快照：`go_source_code/`（`VERSION` = `go1.27.1`，2026-08-28）；规格基准 `doc/go_spec.html`
- 首发平台：**Linux / arm64（Docker 容器提供）→ `aarch64-unknown-linux-gnu` / ELF**；不维护 macOS 第二套首发环境
- **Git 仓库已发布**：<https://github.com/nobody0726/rust-go-compiler>（public，分支 `main`，remote = `origin`）
- **语料不入库**：`go_source_code/`（185 MB / 15,618 文件）由根目录 `corpus-manifest.sha256` 锁定；校验用 `shasum -a 256 -c corpus-manifest.sha256`
- **`git push` 常被拦**：`github.com` 的 CONNECT 间歇性 502，而 `api.github.com` 正常 → 改用技能 `github-push-via-api` 走 Git Data API（脚本含 `--root` 模式处理空仓引导提交场景）
- 语料快照一致性以 SHA-256 清单为准，`VERSION` 文件本身不足以证明内容一致

## 当前状态：M0 三件套已就绪，尚未开工

- **入口**：`docs/milestones/` 下的 M0 三件套 —— `M0-design.md`（**已确认**，D-M0-1~12）、`M0-tests.md`（测试清单，待冻结）、`M0-plan.md`（实施计划 T01–T28，待执行）
- **决策全部已定**：D-M0-1~6（用户拍板）—— Docker 替代 Lima／严格环境优先／单仓单根／语料不入库只写重建步骤／基础镜像 `golang:1.27.1-bookworm`／调试只覆盖 **Rust 代码级**；D-M0-7~12（2026-10-02 接受）—— index digest 钉镜像／rustup + `rust-toolchain.toml` 钉版／cargo home 用命名卷而 `target/` 待基准／开发容器长驻 + 门禁一次性 `docker run` 同 digest／最小 `rgoc-hir` 标 `SPIKE-ONLY`／环境 manifest 载体为 `M0-manifest.json`
- **待办**：`M0-manifest.json`（`environment` 节由 T15 产出）；`M0-tests.md` 的冻结；**Phase 2–4 的计划**要等 Phase 1 门禁（E5 实测断点）通过后再拆（理由见 `M0-plan.md` §0.1）
- **五个 Phase**：0 容器与工具链底座 → 1 VSCode 调试环境（**门禁 = 实测断点命中**，不是「能开窗口」）→ 2 Rust 骨架 + harness → 3 三个 spike（解释/SSA/native）→ 4 契约 + 报告
- **关键洞察**：Go oracle 是**硬约束**（必须精确 `go1.27.1`；宿主 `go1.24.5` 不可作基线），Rust 版本是**软约束** → 用 `golang:1.27.1-bookworm` 基础镜像满足 Go，用 `rustup` 满足 Rust
- **环境实测（2026-10-02 只读侦察）**：Docker 29.6.2／内核 `6.12.76-linuxkit`／`aarch64`／10 CPU、7.75 GiB；镜像 index digest `sha256:69a7b978…9195`，arm64 digest `sha256:1668bbf8…fae1`；**宿主无 rustc/cargo/rustup**；VSCode 1.139.1 **缺** `rust-lang.rust-analyzer`、`vadimcn.vscode-lldb`（由 devcontainer 在**容器内**装，宿主不用装）；Rust stable 参考值 `1.98.1`

## 文档体系（docs/）

**工程入口**：工作区根目录 **`AGENTS.md`** —— 任何任务开始前先读它。含：30 秒速览、目录地图、文档清单与状态、工作纪律（Superpowers TDD / 四维完成度 / 硬约束）、变更同步清单、文档自检命令、下一步。本文件（MEMORY.md）保留精简摘要以保证自动注入生效。

**命名规范**：顶层文档 `NN-<kebab-topic>.md`，NN 体现层级与阅读顺序；`README.md` 为索引，固定名不编号。

| 序号 | 文档 | 层级 | 角色 |
|---|---|---|---|
| — | `README.md` | 索引 | 文档地图、依赖关系、命名规范 |
| 01 | `01-feature-set.md` | 规格 | **要建什么** —— 功能全集 / RTM，246 个功能点（Closure Coverage 三维交叉验证） |
| 02 | `02-test-inventory.md` | 规格 | **如何验证** —— 功能点 → 官方测试用例映射 |
| 03 | `03-roadmap.md` | 计划 | **按什么顺序建** —— M0–M12 迭代计划 v2 |
| 04 | `04-development-environment.md` | 环境 | **在哪建** —— Docker 容器方案（aarch64 原生，替代原 Lima VM 方案） |
| — | `milestones/M0-design.md` | 设计 | **怎么建 M0** —— 决策 D-M0-*、环境基线、Phase 0–4、门禁 E1–E10 |

**阶段文档**（`docs/milestones/`，每阶段开工前须三样齐备，03 §6.2）：`<ID>-design.md` + `<ID>-tests.md` + 机器可读 manifest。

**依赖方向**：01 + 02 → 03（03 §0 声明二者为其「输入规格」）→ `docs/milestones/<阶段ID>-{design,tests}.md`；04 落实 03 §0.2 的平台约束。

**已创建**：`docs/contracts/`（**空**，M0 Phase 4 才产出 5 份契约初稿）、`docs/milestones/`（仅 `M0-design.md` 设计稿）。

**引用约定**（定义在 `01-feature-set.md` §0.4）：`spec:Lxx-yy` → `doc/go_spec.html`（规范性）；`nodes:Lxx-yy` → `syntax/nodes.go`（规范性）；`runtime/*.go:NNN`、`abi/type.go:NNN`、`gc/*/`、`link/ld/`（参考性）。

## 关键方法论约定

- **Superpowers TDD**：行为切片一律 RED → GREEN → REFACTOR；先确认失败来自缺失行为而非环境/harness，再最小实现
- 完成度按 **四维**（Frontend / Interpreter / Native / Runtime-metadata）分别跟踪，**不以「246 项全勾选」作为总退出条件**
- 每个阶段及独立验收子阶段开工前须产出 `docs/milestones/<阶段ID>-tests.md` + 机器可读 manifest
- 提交仅在明确要求时执行；提交信息引用功能 ID / 验证证据
- 只读官方语料，**不改动** `go_source_code/`

## 重要事实

- `go_source_code/test/` 与 `src/internal/types/testdata/` 是**语言级黑盒测试**，只依赖编译器对外行为，可直接作为 Rust 重写实现的验收测试集
- **官方测试驱动器是 `src/cmd/internal/testdir/testdir_test.go`**（2,072 行），**不是 `test/run.go`（该文件不存在）** —— 此为 2026-10-02 实测修正，`02-test-inventory.md` 尚待回写
- **指令集共 16 个**（非此前记录的 7 个）：`compile`/`compiledir`/`build`/`builddir`/`buildrundir`/`run`/`buildrun`/`runoutput`/`rundir`/`runindir`/`asmcheck`/`errorcheck`/`errorcheckdir`/`errorcheckoutput`/`errorcheckandrundir`/`skip`；**未知指令直接 `t.Fatalf` 硬失败**
- 三条 harness 必需精确实现的规则（带源码行号，详见 `M0-tests.md` §1.3）：① 指令行是「首个非空且非构建约束行」，**不能假定在第 1 行**；② **`.out` 缺失 ⇒ 期望输出为「空」**（不是「任意输出都可通过」）；③ `errorcheck` 即使裸写也会被自动加 `-d=ssa/check/on`，故其期望**不能当纯语言语义验收**
- 02-test-inventory §0.2 记录了 **9 处**对既有文档的事实修正（如 `syntax/testdata/` 实为 31 个文件），**以该节为准**
