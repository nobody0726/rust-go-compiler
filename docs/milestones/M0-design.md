# M0 设计文档：工具链、harness 与三个架构 spike

> **阶段 ID**：`M0`　|　**状态**：**已确认**（2026-10-02，含 D-M0-1 ~ D-M0-12 全部决策）　|　**日期**：2026-10-02
> **文档索引**：[`../README.md`](../README.md)　|　**上游**：[`../03-roadmap.md`](../03-roadmap.md) §4 M0、[`../04-development-environment.md`](../04-development-environment.md) §8
> **下游**：[`M0-tests.md`](./M0-tests.md)（测试先行清单，**已产出**）→ writing-plans（2–5 分钟任务级计划）→ 子代理执行
>
> **本文档的性质**：这是 Superpowers 流程中「头脑风暴」阶段的产出 —— **只冻结设计决策与边界，不写任务粒度、不写实现代码**。任务级拆分是 `writing-plans` 的职责。设计未确认前不进入下一步。

---

## 0. 本文档在流程中的位置

```text
头脑风暴（本文档）
   │  产出：用户签字确认的设计文档
   ▼
writing-plans
   │  产出：2–5 分钟粒度的任务计划（文件路径 / 可粘贴代码 / 验证命令）
   ▼
子代理驱动开发 + TDD（RED → GREEN → REFACTOR）
```

同时满足 [`../03-roadmap.md`](../03-roadmap.md) §6.2 的开工前置要求：

| 前置产出 | 本文档是否满足 | 说明 |
|---|---|---|
| `docs/milestones/M0-design.md` | ✅ 本文件 | 设计决策与边界（**已确认**） |
| `docs/milestones/M0-tests.md` | ✅ **已产出** | 必需测试 ID、比较器、验证命令、暂不支持项（见 [`M0-tests.md`](./M0-tests.md)） |
| `docs/milestones/M0-plan.md` | ✅ **已产出（Phase 0–1）** | 2–5 分钟粒度实施任务（见 [`M0-plan.md`](./M0-plan.md)） |
| 机器可读 manifest | ⏳ 部分 | `M0-manifest.json` 的 `environment` 节由 T15 产出；其余节在 Phase 2–4 填充 |

---

## 1. 目标与非目标

### 1.1 M0 要证明的三个命题

M0 **不承诺语言实现**，只证明三条技术路线在其最小形态下走得通。每条命题都必须**可证伪**：

| # | 命题 | 可证伪的判定方式 |
|---|---|---|
| P1 | 固定 HIR → 宿主求值这条**解释器路线**可行 | 一个固定 HIR 程序求值产出 `3`，stdout 精确等于 `3\n`，退出码 `0` |
| P2 | 固定 HIR → **Block/Value SSA** 表示可行，且能求值 | SSA 构造 + 求值结果与 P1 一致；memory / tuple / 调用边界需求被**显式记录** |
| P3 | 自研 arm64 汇编 → 系统 `clang` → **可运行 ELF** 这条 native 路线可行 | `file` 判定为 `ELF 64-bit LSB ... ARM aarch64`；运行输出 `hello\n`；退出码 `0` |

**P3 是 M0 风险最高的一项**（见 `../03-roadmap.md` §7「native/object/ABI 范围爆炸」），因此它在 M0 就有门禁，不能推迟到 M7。

### 1.2 明确不做（M0 砍项）

| 不做 | 理由 |
|---|---|
| 标准库实现 | M0 只做内建与固定 fixture |
| 完整官方 test runner | M0 只做 v0：`run` / `compile` / 纯前端 `errorcheck` |
| 自研 assembler / ELF writer / linker | 首发委托容器内系统 `clang`（`../03-roadmap.md` §0.2） |
| 真实 Go 源码 parser | spike 输入使用**固定 HIR**，不做 lexer/parser |
| 完整 CI | 只保留「同 digest 可接入」的能力，不实施 |
| 任何 M1–M12 的功能实现 | 见 `../03-roadmap.md` §8.2 |
| 第二个 target（x86_64） | 见 §3.3 决策 7 |

### 1.3 M0 退出条件（逐条可判定）

直接继承 `../03-roadmap.md` §4「M0」与 `../04-development-environment.md` §8，**全部为必需项，无「大致完成」**：

| # | 退出条件 | 判定证据 |
|---|---|---|
| E1 | 基础镜像按 **digest** 锁定，含钉版 Rust 与 `clang` | Dockerfile 中 `FROM ...@sha256:...`；`docker build` 可重放 |
| E2 | 环境实测值全部记录进 manifest | `M0-manifest.json` 的 `environment` 节无空缺字段 |
| E3 | harness 自验：成功 / 失败 / 未知指令 / 平台过滤 / 超时 / 不匹配版本**六类**各有测试 | 六类测试**全绿**（`../03-roadmap.md` §4 门禁） |
| E4 | 官方语料基线 **≥ 20 个**按模式分层的确定性样本 **100% 通过** | harness 报告；样本清单冻结在 `M0-tests.md` |
| E5 | VSCode 调试环境**实测断点命中** | 在 harness 测试中下断点 → 运行 → 确认停下，且变量与调用栈可见 |
| E6 | 三个 spike 的**输入、结果、环境**全部可复现 | 三份 spike 决策记录 + 重跑一致 |
| E7 | native `hello` 可运行，并被登记为 M1 smoke 回归项 | 运行输出 + `file` 输出 + M1 smoke 清单含该项 |
| E8 | 契约初稿产出 | `docs/contracts/` 下 5 份（见 §7） |
| E9 | 统一退出检查（`../03-roadmap.md` §6.3 六条） | `cargo fmt --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` 全过 |
| E10 | 绑定挂载 vs 容器卷的构建/测试基准已完成 | 基准数据进 manifest 的 `benchmarks` 节 |

> **预算**（`../03-roadmap.md` §6.1）：M0 首轮 2–3 维护者人周，最长日历周期 6 周。超限时的切断顺序见 §9.2。

---

## 2. 已确认决策记录

本轮讨论中由用户逐项确认的 6 项决策，是后续所有设计的约束：

| ID | 决策项 | 选择 | 对既有文档的影响 |
|---|---|---|---|
| **D-M0-1** | 环境底座 | **Docker 容器**（替代 Lima VM） | `04` 整篇重写；`03` §0.2 / §1.1 D5 / §2 修订；`AGENTS.md`、`README.md` 同步 —— **均已完成** |
| **D-M0-2** | 推进顺序 | **严格环境优先** —— 环境完全就绪（E1/E2/E5）后才开始 harness 与 spike | 落实 `../03-roadmap.md` §8.3 |
| **D-M0-3** | 代码位置 | **单仓单根** —— 仓库根即 `rust_go_compiler/`，Rust workspace 在 `rgoc/` | `../03-roadmap.md` §2 骨架树已按此重写 |
| **D-M0-4** | 语料处置 | `go_source_code/` **不入库**，由 `corpus-manifest.sha256` 锁定；新环境**只在文档写明重建步骤** | `.gitignore` + `corpus-manifest.sha256` 已就位；重建步骤见 §5.2 |
| **D-M0-5** | 基础镜像 | **`golang:1.27.1-bookworm`** | 与语料 oracle 版本天然一致（§3.2） |
| **D-M0-6** | 调试环境覆盖层 | **Rust 代码级**（rust-analyzer + CodeLLDB），不做 native ELF 源码级调试 | `../04-development-environment.md` §4.2 已写明本阶段不做项 |

### 2.1 本文档新增的设计决策（**已确认** 2026-10-02）

| ID | 决策项 | 选择 | 理由 |
|---|---|---|---|
| **D-M0-7** | 镜像 digest 钉法 | Dockerfile 用 **index digest**；manifest 另记 **arm64 平台 digest** + 上游 revision | index digest 不可变且可读性好；平台 digest 用于校验实际拉取内容（§3.3） |
| **D-M0-8** | Rust 工具链来源 | **`rustup` + `rust-toolchain.toml`** 钉版，**不依赖**镜像内置 | Go oracle 必须精确匹配（硬约束），Rust 版本相对宽松；用 rust-toolchain.toml 换取可复现与易升级 |
| **D-M0-9** | 依赖缓存位置 | `cargo` registry/git → **命名卷**；`target/` → **先 bind mount，Phase 0 基准后决定** | macOS 上 bind mount 随机写慢；`target/` 需实测（§3.5） |
| **D-M0-10** | 容器生命周期 | **开发容器长期驻留** + **门禁用一次性 `docker run`**，两者同 digest | 调试需要长驻进程；门禁需要干净环境保证可复现 |
| **D-M0-11** | 是否引入 `rgoc-hir` crate | **引入，但只含三个 spike 共享的最小 HIR**，标注 `SPIKE-ONLY` | 三个 spike 需要同一份 HIR 形状，否则「固定 HIR」会漂移成三份；M5 用正式 HIR 替换（§6.5） |
| **D-M0-12** | 环境 manifest 载体 | **`docs/milestones/M0-manifest.json`** 单一机器可读文件，`environment` 节即 `04` §8 要求的「环境 manifest」 | 避免多份文件漂移；便于脚本校验 |
| **D-M0-13** | M0 的 crate 落地节奏 | **严格按当期需要**：Phase 2 只建 `rgoc-harness` / `rgoc-driver` / `xtask`；Phase 3 才建 `rgoc-hir` 与 spike 相关 crate。**其余 crate 不建空壳** | 本节原表把 8 个 crate 都列成「M0 建」，与 `../03-roadmap.md` §2 的原文「M0 只创建当期需要的部分……其余 crate **不建空壳**，按里程碑依赖引入」**冲突**。以 `03` §2 为准（它是路线图，规范性更强）。2026-10-02 拍板 |
| **D-M0-14** | 三个 spike 的代码隔离形态 | 放在**独立 crate `rgoc-spikes`**（三个 bin + 共享固定 HIR fixture），依赖 `SPIKE-ONLY` 的 `rgoc-hir` | §6.4 要求「spike 代码**隔离保留**，后续用测试驱动的正式实现替换，**不盲目演进临时代码**」。隔离成一个 crate 后可**整块删除/归档**，不会与正式实现纠缠在同一模块里。2026-10-02 拍板 |
| **D-M0-15** | `rgoc-harness` 的 `double_sum` 调试目标 | **保留**，并转为 **T-H-01（成功类）的 fixture** —— 不按原计划在 Phase 2 删除 | 原计划（`M0-plan.md` §8 纪律 + 本文 §6.3 口径）写「Phase 2 引入真实功能后删除」。但 **E5 是人工门禁**，删掉锚点就无法复验（换机器 / 重装 / 升级 VSCode 后要重验）。改为让它承担 T-H-01 的**进程内正例**，从「遗留物」变成「有存在价值的自测输入」。2026-10-02 修正 |

> **D-M0-7 ~ D-M0-12 已于 2026-10-02 由用户全部接受，无条件项。**
>
> **D-M0-13 / D-M0-14 于 2026-10-02 由用户拍板**（拆 Phase 2–4 计划时暴露出本文 §6.3 的 crate 表与 `03` §2 冲突）。
> **D-M0-15 是修正而非新增决策** —— 它推翻了「Phase 2 删除调试目标」这条会**破坏 E5 可复验性**的安排。

---

## 3. 环境基线（Docker）

### 3.1 已实测事实（2026-10-02 侦察）

以下全部为**本机实测值**，非推测：

| 项 | 实测值 | 来源 |
|---|---|---|
| Docker 引擎 | **29.6.2**（Docker Desktop） | `docker info` |
| 引擎架构 | **`aarch64`** | `docker info` |
| 容器内核 | **`6.12.76-linuxkit`** | `docker info` |
| 可用资源 | **10 CPU / 8,321,515,520 B（≈7.75 GiB）** | `docker info` |
| 本机已有镜像 | `ubuntu:24.04`（257 MB）、`veloco-dev:*`（1.19 GB，他项目） | `docker images` |
| 基础镜像 index digest | `sha256:69a7b9788769bec032d238959b61854e9ae87f57be9029ec04e9885fabf99195` | `docker buildx imagetools inspect` |
| 基础镜像 **linux/arm64** digest | `sha256:1668bbf856490c9bd0b77204ae1cdaebe273dc459b5eeb3a31a4ddc78ca6fae1` | `docker manifest inspect` |
| 上游构建 revision | `c4664da1bd8d0cbc975460744d90c031b409c215`（`docker-library/golang` `1.27/bookworm`） | 镜像 OCI 注解 |
| 镜像构建时间 | `2026-09-19T02:16:26Z` | 镜像 OCI 注解 |
| **宿主 Rust 工具链** | **不存在**（`rustc` / `cargo` / `rustup` 均 not found） | `which` |
| VSCode | **1.139.1**（arm64，commit `04c0d99f`） | `code --version` |
| VSCode 扩展数 | **13 个** | `code --list-extensions` |
| 其中 Dev Containers | ✅ `ms-vscode-remote.remote-containers` | 已装 |
| 其中 **rust-analyzer** | ❌ **未装** —— `rust-lang.rust-analyzer` | 待补（§4） |
| 其中 **CodeLLDB** | ❌ **未装** —— `vadimcn.vscode-lldb` | 待补（§4） |
| Rust 当前 stable | **1.98.1**（2026-09-03 发布；edition 2024） | 官方发布记录 |
| 语料许可文件 | `LICENSE`（1,453 B）、`PATENTS`（1,303 B）**均在** | `go_source_code/` |

### 3.2 为什么是 `golang:1.27.1-bookworm`

两个版本约束的**松紧程度不同**，这决定了镜像与工具链的分工：

| 工具 | 约束强度 | 理由 |
|---|---|---|
| **Go oracle** | **硬约束 —— 必须精确 `go1.27.1`** | 语料快照是 `go1.27.1`；宿主 `go1.24.5` 版本不符，**不可作基线**（`../03-roadmap.md` §0.2、`04` §3.4）。版本不匹配时差分结论无效 |
| **Rust 工具链** | **软约束 —— 取当时 stable 即可** | 只影响编译器本体的实现语言，不影响 oracle 正确性 |

因此：**用基础镜像满足硬约束（Go），用 `rustup` 满足软约束（Rust）**。这直接推出 D-M0-8。

若基础镜像选反（例如用 `rust:1.98.1-bookworm` 再自己装 Go），则要自行保证 Go 的精确版本与校验 —— 把硬约束交给手工步骤，风险更高。

### 3.3 七项待决事项的设计结论

`../04-development-environment.md` §9 列出 7 项「M0 拍板」事项，本文档逐项给出设计结论：

| # | 待决事项 | 设计结论 | 状态 |
|---|---|---|---|
| 1 | 镜像 digest 钉法与更新策略 | **Dockerfile 写 index digest**；manifest 记 arm64 平台 digest + 上游 revision + 构建时间。**更新 = 独立决策**：换 digest 须重跑全部 M0 门禁并保留前后报告 | 已定（D-M0-7） |
| 2 | CPU / 内存 / 磁盘配额 | **不设容器级上限**（`--cpus`/`--memory` 会让 `cargo build` 变慢并污染基准）。改为**记录** Docker Desktop VM 级总量（当前 10 CPU / 7.75 GiB），并在资源不足失败时按 `runtime-failure` 分类 | 已定，实测值待复测 |
| 3 | Rust 工具链来源 | **`rustup` + `rust-toolchain.toml`**，钉 `1.98.1`（M0 执行时以当时 stable 为准并写入 manifest） | 已定（D-M0-8） |
| 4 | 依赖缓存位置 | `cargo` registry + git → **命名卷** `rgoc-cargo-home`；`target/` → **Phase 0 基准测试后决定**（bind mount 快则保留，慢则移卷） | 部分待实测（D-M0-9） |
| 5 | 容器生命周期 | **双形态同镜像**：开发用长期驻留（devcontainer）；门禁用一次性 `docker run` | 已定（D-M0-10） |
| 6 | 是否需要 CI 同 digest | **M0 不实施 CI**。只保证：统一入口脚本不依赖宿主 PATH，使后续接 CI 成本为零 | 已定，不做 |
| 7 | 是否需要第二 Linux target | **M0 明确不做**，且 M0 不引入任何 x86 专用依赖 | 已定，不做 |

### 3.4 Dockerfile 分层设计

```text
.golang:1.27.1-bookworm@sha256:<index>        ← 层 1：钉死的基础镜像（含 Go oracle 1.27.1）
  └─ 系统包：clang / llvm / lld / make / pkg-config / lldb / gdb   ← 层 2：native 与调试能力
       └─ rustup + 钉版工具链（来自 rust-toolchain.toml）           ← 层 3：编译器本体语言
            └─ 非 root dev 用户（uid/gid 与宿主对齐）               ← 层 4：挂载属主正确
```

要点：

- **每一层都必须在 Dockerfile 里显式写出工具的实际路径、版本与来源**，不依赖宿主 PATH 的偶然继承（`../04-development-environment.md` §3.3）。
- `clang` 是 **P3 的唯一驱动**，必须在 Phase 0 就装好并记录版本。
- 层 4 的 uid/gid 对齐是 `../04-development-environment.md` §7「绑定挂载性能或权限问题」的直接控制措施。

### 3.5 挂载与缓存的候选方案

| 路径 | 候选策略 | 待 Phase 0 基准回答 |
|---|---|---|
| 源码 `rust_go_compiler/` | **bind mount**（唯一） | 编译速度是否可接受 |
| `$CARGO_HOME/registry`、`$CARGO_HOME/git` | **命名卷** | — |
| `rgoc/target/` | bind mount **或** 命名卷 | 二者 `cargo build` / `cargo test` wall time 差异 |
| `go_source_code/` | **bind mount 只读** | — |

**基准判据**：`../04-development-environment.md` §6 要求实测镜像构建时间、镜像体积、`cargo check`/单测/smoke wall time、harness 峰值 RSS、native smoke 编译运行耗时、容器冷启动时间。这些数据进 `M0-manifest.json` 的 `benchmarks` 节（E10）。

### 3.6 统一入口约定

- 所有容器内命令通过**版本化脚本**进入，脚本输出声明：镜像 digest、Linux target、工具实际路径与版本。
- 命令**不得**根据宿主 PATH 静默选择不同 oracle 或 linker（`../04-development-environment.md` §4.1）。
- 入口脚本在 daemon 未运行时**明确报错**，不静默降级（`../04-development-environment.md` §7）。

---

## 4. VSCode 调试环境设计

### 4.1 现状与缺口（实测）

| 组件 | 现状 | 需要的动作 |
|---|---|---|
| VSCode | ✅ 1.139.1 arm64 | 无 |
| Dev Containers | ✅ `ms-vscode-remote.remote-containers` | 无 |
| **rust-analyzer** | ❌ **未装** | 写入 `.devcontainer/devcontainer.json` 的 `customizations.vscode.extensions` |
| **CodeLLDB** | ❌ **未装** | 同上 |

**关键点：宿主不需要手动安装这两个扩展。** Dev Containers 会在**容器内**自动安装 `customizations.vscode.extensions` 列出的扩展。宿主只要求 Dev Containers 本身已装（已满足）。这避免把宿主环境变成第二个需要维护的工具链来源。

### 4.2 `.devcontainer/` 组成

```text
.devcontainer/
├── devcontainer.json       # 引用 Dockerfile、扩展清单、挂载、postCreateCommand
└── Dockerfile              # 或复用仓库根的 docker/Dockerfile（同一 digest）
```

`devcontainer.json` 必须包含：

| 配置项 | 值 | 理由 |
|---|---|---|
| `build.dockerfile` + `build.args` | 引用钉 digest 的 Dockerfile | 与门禁容器同源 |
| `customizations.vscode.extensions` | `rust-lang.rust-analyzer`、`vadimcn.vscode-lldb` | 容器内自动安装 |
| `runArgs` | `--cap-add=SYS_PTRACE`、`--security-opt seccomp=unconfined` | **否则调试器无法 `ptrace`，断点必然不生效**（`../04-development-environment.md` §4.2） |
| `remoteUser` | 非 root dev 用户 | 与 Dockerfile 层 4 一致 |
| `mounts` | 源码 bind mount + cargo 命名卷 | 见 §3.5 |

### 4.3 验收判据：实测断点命中

> **判据是「在某个 harness 测试里下断点 → 运行 → 确认真的停下，且变量与调用栈可见」，而不是「能打开容器窗口」。**（`../04-development-environment.md` §4.2）

可执行的三步验证：

1. 在 `rgoc-harness` 的某个测试函数内下断点；
2. 用 **CodeLLDB** 启动该测试；
3. 确认：命中（未跳过）、栈帧列表非空、至少一个局部变量可求值。

**本阶段不做**（`../04-development-environment.md` §4.2）：native ELF 的源码级调试（M0 不生成 DWARF，等 M7）、汇编/反汇编视图。

---

## 5. 语料重建（不入库前提下）

### 5.1 为什么不入库

- 语料约 **185 MB**，包含完整 Go 源码树，入库会使仓库体积与 clone 成本失控；
- 语料是**只读依赖**（红线段规：不得修改、不得回写），并非项目的可编辑资产；
- 一致性由 **`corpus-manifest.sha256`** 锁定：**15,618 条**排序后的 `路径 → SHA-256`，`shasum -a 256 -c` 全量校验 **0 失败**。

### 5.2 两个必须区分的场景

| 场景 | 需要的动作 | 频率 |
|---|---|---|
| **本机现状** | `go_source_code/` **已存在且已用 manifest 校验通过** → **无需重建**，只需在 M0 复核校验仍为 0 失败 | 一次性 |
| **他人 / 新机器 / 未来 CI 从零开始** | 按下方步骤下载官方 `go1.27.1` 源码 → 展开为 `go_source_code/` → 用 manifest 校验 | 按需 |

**重建步骤（写入文档，不写脚本自动化 —— D-M0-4）**：

```sh
# 1) 取得官方 go1.27.1 源码快照（tarball 或 git 检出 go1.27.1 tag 均可）
# 2) 展开/复制为工作区根的 go_source_code/
# 3) 用仓库内清单逐文件校验
cd /Users/wangfeng/workspace/rust_go_compiler
shasum -a 256 -c corpus-manifest.sha256 | grep -v ': OK$'   # 期望：无输出
```

> **注意**：清单由 `find … -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256` 生成，**已排除 `.DS_Store` 与 `._*`**。重建时若引入这些文件，会出现 `FAILED` 行（属于噪声而非语料不符）。

### 5.3 合规

- 语料**不入库**，因此仓库中**不复制** Go 的 `LICENSE` / `PATENTS`；合规要求落在**获取步骤说明**上（`../03-roadmap.md` §7「许可/语料再分发」）。
- 若未来在自有测试或文档中**引用 Go 源码片段**，须保留其许可与来源声明。
- 语料目录**始终只读**，任何情况下不得回写、不得修改（`AGENTS.md` §4.3 红线）。

---

## 6. 工作分解：Phase 0–4

> **顺序原则（D-M0-2）**：Phase 0 → 1 是**环境**，Phase 2 → 4 是**内容**。环境门禁未过，**不得**开始 Phase 2。

### 6.1 Phase 0 —— 容器与工具链底座

| 项 | 内容 |
|---|---|
| **目标** | 让「钉死的环境」真实存在并可重放 |
| **产出** | `Dockerfile`（钉 digest）、`rust-toolchain.toml`、容器入口脚本、`M0-manifest.json` 的 `environment` 节 |
| **门禁** | ① `docker build` 可从零重放；② 容器内 `go version` = `go1.27.1` 且路径可记录；③ 容器内 `rustc --version` = 钉版；④ 容器内 `clang --version` 可记录；⑤ 四者的路径/版本/校验值与镜像 digest 一并写入 manifest（**E1、E2**） |
| **阻塞信号** | `golang:1.27.1-bookworm` 拉取失败 / `clang` 或 `lld` 在 arm64 上装不上 / rustup 无法访问分发源 |

### 6.2 Phase 1 —— 调试环境

| 项 | 内容 |
|---|---|
| **目标** | 「人能高效地在这个环境里写代码」 |
| **产出** | `.devcontainer/devcontainer.json`（+ Dockerfile 引用）、扩展清单、调试启动配置 |
| **门禁** | **E5：实测断点命中**（§4.3 三步验证全过） |
| **阻塞信号** | `ptrace` 被拒 / CodeLLDB 无法附加 → 优先检查 `runArgs` 的 `SYS_PTRACE` 与 `seccomp=unconfined` |
| **依赖** | Phase 0 的镜像 digest |

### 6.3 Phase 2 —— Rust 工程骨架与 harness 自验

| 项 | 内容 |
|---|---|
| **目标** | 建立最小工程骨架 + 可自证的 harness |
| **产出** | `rgoc/` workspace（`Cargo.toml` + **仅当期需要的 crate**）、`rgoc-harness`（Test IR / 指令解析 / oracle / 比较器 / 结果格式）、`tests/smoke/`、`xtask/`、**`M0-tests.md`** |
| **门禁** | **E3**（六类自测全绿）+ **E4**（≥20 个官方样本 100% 通过） |
| **阻塞信号** | 指令解析发现不可预期的前导格式 / 官方样本的期望输出无法在无 `.out` 时确定 |

**crate 创建范围**（`../03-roadmap.md` §2：**只创建当期需要的模块，不一次性建立全部空 crate**）。
**决策 D-M0-13（2026-10-02）** 明确按 Phase 落地 —— 下表的「建」列即该 crate 的**创建阶段**：

| crate | 创建于 | 理由 |
|---|---|---|
| `rgoc-harness` | ✅ Phase 1 建（调试目标）→ Phase 2 扩写 | E3/E4 的直接载体；Phase 1 已建，此处只是扩展 |
| `rgoc-driver` | ✅ Phase 2 | 统一 CLI 入口的骨架，M0 只需能调度 harness 与三个 spike |
| `xtask` | ✅ Phase 2 | 语料枚举 / 报告 / 环境 manifest 生成 |
| `rgoc-hir` | ⚠️ Phase 3，**最小**（D-M0-11） | 三个 spike 共享的固定 HIR；标 `SPIKE-ONLY`，M5 替换 |
| `rgoc-spikes` | ✅ Phase 3（D-M0-14） | 三个 spike 的**隔离载体**：三个 bin + 共享 fixture，可整块删除 |
| `rgoc-interp` / `rgoc-ssa` / `rgoc-codegen` / `rgoc-runtime` | ❌ **M0 不建** | spike 在 `rgoc-spikes` 内实现；正式 crate 按里程碑依赖引入（M5/M6/M7） |
| `rgoc-lex`、`rgoc-ast`、`rgoc-const`、`rgoc-types`、`rgoc-loader` | ❌ **M0 不建** | M1–M4 才需要（`../03-roadmap.md` §2） |

> ⚠️ **本表原写法**（8 个 crate 全在 M0 建）与 `../03-roadmap.md` §2 的「M0 只创建当期需要的部分……
> 其余 crate **不建空壳**」**冲突**。已按 D-M0-13 以 `03` §2 为准修正。
> spike 的隔离形态另见 **D-M0-14**：放独立 crate `rgoc-spikes`，而不是散进各正式 crate。

**harness 的 v0 范围**（`../03-roadmap.md` §4 M0 第 3 条）：

- 模式支持：`run` / `compile` / 纯前端 `errorcheck`；
- 其他模式（`errorcheckdir`、`rundir`、`runoutput`、`asmcheck` 等）**显式分类为 unsupported**，不静默跳过；
- **不重写**完整官方 runner；
- Test IR 必须记录 `../03-roadmap.md` §3.1 列出的全部字段（用例 ID、相对路径、输入文件集合、模式、指令参数、build tags、目标/版本、import 需求、功能依赖、比较器、期望退出码/输出/诊断、超时、资源上限、seed、归属阶段、unsupported 原因）。

### 6.4 Phase 3 —— 三个架构 spike

| Spike | 输入 | 输出 | 门禁 | 记录什么 |
|---|---|---|---|---|
| **S1 解释** | 固定 HIR（`println(1+2)`） | 宿主求值 → stdout | stdout **精确** = `3\n`，退出码 `0` | 值表示、内建调用边界、输出流走法 |
| **S2 SSA** | 固定 HIR | Block / Value 图 + 求值 | 求值结果与 S1 一致 | **memory / tuple / 调用边界需求**被显式记录；退化点与 M6 的差距 |
| **S3 native** | 固定 HIR/SSA 函数 | arm64 汇编 → `clang` → ELF | `file` = `ELF 64-bit LSB … ARM aarch64`；运行输出 `hello\n`；退出码 `0` | 调用约定、**栈对齐**、输出流、退出码、最小 runtime 桥接、unwind 边界 |

- 三者的**输入、结果、环境必须全部可复现**（E6）—— 重跑必须一致；
- S3 完成后，native `hello` **登记为 M1 smoke 回归项**（E7，`../03-roadmap.md` §4）；
- spike 代码放在**独立 crate `rgoc-spikes`**（D-M0-14），**隔离保留**，后续用测试驱动的正式实现替换，**不盲目演进临时代码**（`../03-roadmap.md` §4）；三个 spike 的**输入一律写死**，不接受命令行传入可变输入 —— 否则「可复现」（E6）无从判定。

### 6.5 Phase 4 —— 契约初稿与交付报告

| 项 | 内容 |
|---|---|
| **目标** | 把 spike 验证出的形状固化为可版本化契约，并交付报告 |
| **产出** | `docs/contracts/` 下 5 份初稿（§7）、`M0-report.md`、`M0-manifest.json`（完整） |
| **门禁** | **E8、E9、E10** + `../03-roadmap.md` §6.3 六条统一退出 |

---

## 7. 契约初稿清单

`../03-roadmap.md` §1.3 规定「首次定义 / 验证」落在 M0 的契约。M0 须产出：

| # | 契约 | M0 交付程度 | 后续消费者 |
|---|---|---|---|
| C1 | **SourceMap、file/line/column、诊断排序** | **留位**（M0 不做真实 lexer，只在 Test IR 中固定位置表示与排序规则） | M1/M2 checker、harness、debug dump |
| C2 | **Test IR、构建条件、比较器和结果格式** | **完整初稿** | 所有阶段 |
| C3 | **HIR 多返回值、可寻址性、闭包/方法值、异常传播** | **spike 级**（从 S1/S2 验证出的形状反推） | SSA、解释器、native |
| C4 | **SSA tuple/memory、Phi、支配关系、调用边界** | **spike 级**（从 S2 反推） | codegen、liveness |
| C5 | **ABI、frame layout、对象布局、runtime symbol bridge** | **spike 级**（从 S3 反推；M7 做 MVP，M9 完整化） | 分配、接口、GC、栈切换 |

> **C1 必须显式标注为「留位」**，不得因为文件存在而被误读为 M0 已完成源码位置跟踪。

---

## 8. M0 门禁汇总

| ID | 门禁 | 所属 Phase | 类型 |
|---|---|---|---|
| E1 | 镜像按 digest 锁定，可重放构建 | 0 | 环境 |
| E2 | 环境实测值全部入 manifest，无空缺 | 0 | 环境 |
| E5 | **调试断点实测命中** | 1 | 环境 |
| E3 | harness 六类自测全绿 | 2 | 测试 |
| E4 | ≥20 个官方样本 100% 通过 | 2 | 测试 |
| E6 | 三个 spike 输入/结果/环境可复现 | 3 | 测试 |
| E7 | native `hello` 可运行 + 登记 M1 smoke | 3 | 测试 |
| E8 | 5 份契约初稿产出 | 4 | 交付 |
| E9 | `../03-roadmap.md` §6.3 六条统一退出 | 4 | 统一 |
| E10 | bind mount vs 卷基准完成 | 0 或 4 | 环境 |

**门禁纪律**（`../03-roadmap.md` §3.3）：必需门禁 **100% 通过**，**无基建失败 / 不稳定失败**；跳过项**不得记为 pass**；白名单须**开工前冻结**。

---

## 9. 风险与砍项

### 9.1 M0 特定风险

| 风险 | 早期信号 | 控制措施 |
|---|---|---|
| **P3 native 被环境或桥接阻塞** | `clang` 在 arm64 上不可用；汇编无法链接；程序段错误 | **这是唯一可能在 M0 就无法证明的路线**。先记录原因并调整方案；未证明前**不展开后端大规模实现**（`../03-roadmap.md` §4） |
| Go oracle 版本不匹配 | 容器内 `go version` ≠ `go1.27.1` | `go version` / 路径 / 校验值不符即**拒绝作基线**（`04` §7） |
| 容器内核随 Docker Desktop 漂移 | Docker Desktop 升级 | 记录版本 + 内核；内核变化**视为环境变更，须重跑门禁**（`04` §5、§7） |
| harness 自身有错 | 假通过 / 遗漏目录与参数 | 六类正反自测；**未知指令必报**（`../03-roadmap.md` §7） |
| 绑定挂载导致编译过慢 | `cargo check` wall time 异常 | Phase 0 基准；必要时缓存与 `target/` 移入命名卷 |
| 调试器无法 `ptrace` | 断点不生效（表现为「跳过」） | `--cap-add=SYS_PTRACE` + `seccomp=unconfined`；E5 是硬门禁 |
| `rgoc-hir` 被误当正式实现 | 后续阶段直接在其上开发 | crate 级 `SPIKE-ONLY` 标注 + D-M0-11 明文说明 M5 替换 |

### 9.2 超预算时的切断顺序

`../03-roadmap.md` §6.1 对 M0 的砍项是：「**缩减 runner 模式；保留匹配 oracle 和三个关键 spike**」。据此确定切断顺序：

| 顺序 | 可砍 | **不可砍** |
|---|---|---|
| 1 | 缩减 harness 支持的官方模式数量（保 `run`，退 `compile`/`errorcheck`） | — |
| 2 | 缩减 E4 样本数（但**不得低于 20**，因 §4 门禁写死） | — |
| 3 | 延后 E10 基准测试（记录为 known-gap） | — |
| 4 | 延后 C1/C3/C4/C5 中非 C2 的契约初稿 | — |
| ✗ | — | **Go oracle 版本匹配**（E2 核心） |
| ✗ | — | **三个关键 spike（P1/P2/P3）** |
| ✗ | — | **调试断点命中（E5）** —— 属环境门禁，D-M0-2 明确「环境未经就绪不得开始内容」 |

**禁止**：通过放宽通过率、把跳过项记为 pass、或在环境未就绪时推进 Phase 2+ 来「达成」M0。

---

## 10. 与后续文档的接口

### 10.1 交给 `writing-plans` 的输入 → 已产出 [`M0-plan.md`](./M0-plan.md)

writing-plans 把 §6 的 Phase 拆成 **2–5 分钟粒度**的任务，每个任务必须含：

| 字段 | 来源 |
|---|---|
| **文件路径**（精确到文件） | §3.4、§4.2、§6.3 的结构 + `../03-roadmap.md` §2 骨架树 |
| **要做的**（完整可粘贴代码） | 由各 Phase 的产出定义推导 |
| **验证**（具体命令或断言） | §1.3 的 E1–E10 + §8 门禁表 |

**当前进度**：[`M0-plan.md`](./M0-plan.md) 已覆盖 **Phase 0（T01–T19）与 Phase 1（T20–T28）**，两阶段**均已完成**，四项门禁 **E1/E2/E10/E5 全部通过**（E5 于 2026-10-02 由用户按 F5 实测确认，登记在 [`M0-manifest.json`](./M0-manifest.json) 的 `gate.E5`）。Phase 2–4 的计划**待 Phase 1 门禁（E5）通过后**基于实测环境事实再拆 —— 该前置条件现已满足，理由见该文档 §0.1。

### 10.2 `M0-tests.md` 必须覆盖

- E3 六类 harness 自测的**具体测试 ID** 与比较器；
- E4 的 **≥20 个官方样本清单**（路径 + 模式 + 期望），且**开工前冻结**；
- 三个 spike 各自的**必需测试 ID**（S1/S2/S3）；
- **unsupported 清单**与原因；
- 每个测试的**验证命令**、超时与资源上限。

### 10.3 与后续阶段的衔接

- M0 的 native `hello` → **M1 smoke 回归项**（`../03-roadmap.md` §4）；
- M0 的 `rgoc-hir`（SPIKE-ONLY）→ **M5 用正式 HIR 替换**；
- M0 的 C2（Test IR）→ **所有阶段**的地基，后续改动须走版本化契约流程；
- M0 的环境 manifest → 所有阶段报告中的环境字段来源。

---

## 11. 待办

### 11.1 已完成

- **D-M0-7 ~ D-M0-12**（§2.1 六项新增设计决策）—— 2026-10-02 用户全部接受。
- **`docs/milestones/M0-tests.md`** —— 已产出（§10.2 的全部内容）。

### 11.2 下一步

1. 用 **writing-plans** 把 Phase 0–4 拆成 **2–5 分钟**任务（§10.1：文件路径 / 可粘贴代码 / 验证命令）；
2. 按 §6.1 开始 **Phase 0** 实施（钉 digest 镜像 + Rust 工具链 + `clang` + 环境 manifest）；
3. Phase 1 完成后交付**用户本地 VSCode 可断点调试的环境**（E5）。

### 11.3 文档同步（已完成）

| 文档 | 已改什么 |
|---|---|
| `docs/README.md` | §1 增阶段文档表；§5 改写为「已启用」+ 每阶段三样前置；§6 增设计稿状态说明 |
| `AGENTS.md` | §1 目录地图补 `milestones/`、`contracts/`；§2.1 增 M0 行并修正 04 体量；§3 状态快照；§5 增同步行；§6.1 自检脚本扩展至子目录；§7 下一步重写为五 Phase 表 |
| `.workbuddy/memory/MEMORY.md` | 清除 Lima 残留；新增「当前状态」节（决策、Phase、环境实测值） |
| `.workbuddy/memory/2026-10-02.md` | 追加当日进展；显式更正早前的 Lima 表述 |

**已回写**（2026-10-02）：`02-test-inventory.md` §0.1 与 §0.2 已修正 —— 驱动器改为 `src/cmd/internal/testdir/testdir_test.go`；指令集改为 **16 个**；顶层 `.go` 数改为 **356**；新增「`.out` 缺失 ⇒ 期望输出为空」一条。§0.2 修正项由 6 处增至 **9 处**（`README.md`、`AGENTS.md`、`MEMORY.md` 的计数已同步）。
