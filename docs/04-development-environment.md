# 开发环境方案：Docker 容器（Linux / arm64）

> 状态：方案稿。本机 Docker Desktop 已验证可启动、架构与内核已实测记录（见 §3.1）；但**镜像、工具链、环境 manifest、`.devcontainer` 均尚未搭建**，不代表 M0 门禁已满足。  
> **文档索引**：[`README.md`](./README.md)　|　**上游**：[`03-roadmap.md`](./03-roadmap.md)

---

## 1. 目标与结论

项目统一在 Docker 容器内完成开发、测试和首发 native 验证：

1. Rust 编译器本体、harness、测试和普通开发工具运行在固定的 arm64 Linux 容器。
2. 首发 native 目标固定为 `aarch64-unknown-linux-gnu` / ELF，使用容器内的 Linux `clang`、系统库和 linker 做真实验证。

因此不再维护 macOS/Darwin 第二套首发环境。宿主机只负责运行 Docker Desktop；编译器的工作目录、工具链、harness、native 输出和验收命令均在容器内执行。

---

## 2. 建议的职责边界

| 工作 | Docker 容器 | 宿主机 |
|---|---:|---:|
| Rust 编译器构建、格式化、静态检查 | 主环境 | 可复核 |
| parser、type checker、HIR、解释器单元测试 | 主环境 | 可复核 |
| SSA verifier、Test IR、harness 自测 | 主环境 | 可复核 |
| `aarch64-unknown-linux-gnu` 汇编/对象/链接 | 唯一首发门禁 | 仅运行 Docker Desktop |
| Linux SDK、ELF、系统库 | 唯一真实来源 | 不作为工具链来源 |
| Go `1.27.1` reference oracle | 以容器内可验证安装为准 | 以容器内可验证安装为准 |
| 全量/长时测试 | 夜间或资源允许时 | 夜间或发布前复核 |

代码、manifest、测试结果和工具链清单应由仓库保存；不要把容器磁盘状态当作项目状态。每次报告记录镜像 digest、Docker Desktop 版本、容器内核、工具路径、版本和必要 hash。

---

## 3. 容器基线

### 3.1 实测记录（2026-10-02）

以下为**当前观测值**，用于证明路线可行；**不是**已锁定的基线。M0 需把它们连同镜像 digest 一并写入环境 manifest。

| 项 | 实测值 | 备注 |
|---|---|---|
| 宿主 | macOS 27.2 / arm64 | 与容器架构一致，无 x86 模拟 |
| Docker 引擎 | 29.6.2（Docker Desktop） | daemon 需先启动；`open -a Docker` 后约 5 秒就绪 |
| 引擎架构 | `aarch64` | 原生 arm64 |
| **容器内核** | **`6.12.76-linuxkit`** | **由 Docker Desktop 控制，不能单独钉版本**（见 §5 代价） |
| 当前资源 | 10 CPU / 7.75 GiB | M0 按实测调整并记录 |
| 候选基础镜像 | `golang:1.27.1-bookworm` | 已在 Docker Hub 确认存在；与语料 oracle 版本完全一致 |

### 3.2 初始形态

第一阶段建议从下述形态起步：

- 基础镜像 `golang:1.27.1-bookworm`，**必须按 digest 引用**（tag 会漂移）；
- arm64 Linux 容器，与宿主架构一致，避免第一阶段引入 x86 模拟开销；
- 容器内使用**非 root 日常开发用户**，uid/gid 与宿主对齐，避免挂载文件属主混乱；
- 只挂载项目工作区，**不默认挂载整个 home**；
- 不把开发服务暴露到局域网；
- CPU、内存、磁盘和挂载策略写入版本化配置（Dockerfile / devcontainer / compose），并在 M0 根据实测资源确定。

这只是候选基线，不在本阶段假定某个最终 digest 或 `.devcontainer` 配置已经验证。环境文件应在试装后补充镜像 digest、Docker Desktop 版本、容器内核、资源值和已知限制。

### 3.3 容器内工具分层

建议分三层管理：

1. **系统包**：`clang`/`llvm`、linker、`pkg-config`、调试工具（`lldb`/`gdb`）、证书和基础构建工具。
2. **版本锁定工具链**：Rust toolchain（`rustup` + `rust-toolchain.toml` 钉版本）、脚本运行时以及项目要求的固定工具版本。
3. **项目工具**：`cargo` workspace、harness、测试语料和报告工具，全部由仓库入口统一调用。

系统包安装方式和版本快照必须可重放 —— **Dockerfile 是唯一来源**。Dockerfile 与容器内初始化步骤必须明确写出 `rustc`、`cargo`、Go oracle 和 `clang` 的实际路径、版本和来源；不能依赖宿主机 PATH 的偶然继承。

### 3.4 Go oracle 的锁定

Go reference oracle 有特殊约束：仓库语料是 `go1.27.1`，而**宿主机当前的 Go（`go1.24.5` @ `/usr/local/go/bin/go`）不匹配，不能直接作为基线**。

基础镜像 `golang:1.27.1-bookworm` 使 oracle 与语料版本天然一致。M0 仍需确认并记录：

- 镜像 **digest**（不可只靠 tag）；
- 容器内 `go version` 与 `which go` 的实际输出；
- Go 二进制的校验值。

**退路**：若该镜像在将来不可得或需要重建，使用官方 `go1.27.1.linux-arm64.tar.gz`（已确认可从 `go.dev/dl` 下载），并按 SHA-256 记录产物 hash。

---

## 4. 宿主与容器的交互

### 4.1 推荐工作流

```text
编辑器 / 终端（宿主）
  └─ 进入容器：cargo、harness、解释器、SSA、ELF native smoke/milestone
```

统一入口应通过版本化脚本进入容器，命令输出声明镜像 digest、Linux target 和实际工具路径。不要让命令根据宿主机 PATH 静默选择不同 oracle 或 linker。

建议优先让测试 harness 在容器内执行，而不是让容器通过挂载目录直接修改宿主构建产物。挂载目录主要用于源代码、manifest、日志和报告；编译缓存可留在容器卷或宿主各自的本地磁盘，以减少文件系统同步性能和权限问题。

### 4.2 VSCode 调试环境

宿主已具备：VSCode **1.139.1**（arm64）+ Dev Containers 扩展（`ms-vscode-remote.remote-containers` 0.469.0）。

M0 交付目标：

- 从 `.devcontainer/devcontainer.json` 一键 **Reopen in Container**；
- **Rust 代码级调试**：`rust-analyzer` 语言服务 + **CodeLLDB** 调试器，能在 harness 与三个 spike 的 Rust 代码里下断点、单步、查看变量与调用栈；
- 容器需 `--cap-add=SYS_PTRACE` 与 `security-opt seccomp=unconfined`，否则调试器无法 `ptrace`。

**验收判据是实测断点命中** —— 在某个 harness 测试里下断点、运行、确认真的停下 —— 而不是「能打开容器窗口」。

本阶段不做：native ELF 的源码级调试（M0 尚不生成 DWARF 调试信息，要等 M7）、汇编/反汇编视图（可作为 M0 之后的增强项）。

### 4.3 native 结果的来源标记

每个 native 结果至少记录：

- target triple：`aarch64-unknown-linux-gnu`；
- object format：ELF；
- `clang`、sysroot、libc 和 linker 路径；
- **容器镜像 digest + Docker Desktop 版本 + 容器内核版本**；
- 生成器 commit / 工作区状态（若仓库无 Git，则记录源码清单）；
- 输入 fixture、seed、stdout、stderr、退出码和资源预算。

容器内生成的 ELF 结果就是首发 native 结果；其他 target 的结果必须标记为 `target-filtered`，不能进入首发分母。

---

## 5. 为什么使用 Docker（以及放弃 Lima 的代价）

Docker 容器提供与首发目标同构的 Linux/arm64 用户态：同样的 `clang`、glibc/sysroot 与 ELF 链接路径。相对先前评估的 Lima VM 方案，Docker 的主要好处是：

- **本机已具备** —— Docker Desktop 29.6.2 已安装，无需额外安装虚拟化工具与独立的 VM 镜像；
- 引擎架构 `aarch64`，与宿主一致，**无 x86 模拟开销**；
- 镜像层天然可版本化、可复用，`docker build` 可重放，启动远快于完整 VM。

**必须如实记录的代价**（这是相对 Lima 的真实让步）：

- **容器内核由 Docker Desktop 控制**（当前 `6.12.76-linuxkit`），**不能像独立 VM 那样单独钉内核版本**；Docker Desktop 升级可能改变内核。
- 容器通常没有 systemd/init，属于「进程级」环境而非完整系统。
- 因此镜像漂移的控制方式从「VM 镜像 + guest kernel 双重锁定」变为「**镜像 digest 锁定 + 记录 Docker Desktop 版本与内核版本**」；内核变化作为**已知风险**登记（§7），出现时必须重跑门禁而不是默认接受。
- 若后续确实需要严格钉内核，Lima 仍可作为备选底座；切换成本主要落在本文件与 [`03-roadmap.md`](./03-roadmap.md) §0.2。

宿主机仍可能影响容器性能、文件共享和网络，但这些属于 Docker Desktop 的运行条件，不属于编译器的第二套目标环境。任何宿主相关差异都应通过版本化配置、资源记录和测试报告处理。

---

## 6. 资源与性能建议

资源值暂不拍脑袋固定，在 M0 通过实测锁定。需要测量：

- 镜像构建时间、镜像体积、磁盘占用、可用磁盘余量；
- 当前 CPU/内存配置下的 `cargo check`、单元测试和 smoke wall time；
- 绑定挂载（bind mount）与容器卷上的构建/测试差异；
- harness 峰值 RSS、长时测试资源消耗；
- 容器内 ELF native smoke 的编译和运行耗时；
- **容器冷启动时间**（相对 VM 的优势项，需实测确认）。

与 ROADMAP 的初始目标保持一致：smoke 不超过 5 分钟，milestone 不超过 30 分钟，full 单次不超过 2 小时；真实测量后只做有记录的调整，不能用提高超时来掩盖环境问题。

---

## 7. 主要风险与控制措施

| 风险 | 影响 | 控制措施 | 触发的决策 |
|---|---|---|---|
| 容器与宿主目标混淆 | 宿主工具链绕过容器，结果不可复现 | 结果强制带镜像 digest/target/format；首发门禁只在容器执行 | M0/M7 |
| **容器内核随 Docker Desktop 漂移** | 结果不可复现（相对 VM 的已知代价） | 记录 Docker Desktop 版本 + 内核版本；内核变化视为环境变更，须重跑门禁 | 全周期 |
| Docker Desktop 未运行 | 全部命令失败，易被误判为代码问题 | 统一入口先探测 daemon，失败时明确报错而非静默降级 | 全周期 |
| 绑定挂载性能或权限问题 | 编译变慢、文件属主/时间戳异常 | uid/gid 对齐；挂载与卷做基准；必要时把缓存移入容器卷 | M0 |
| Go oracle 版本不匹配 | 差分结论无效 | 固定 `go1.27.1`，`go version`/路径/校验值不符即拒绝作基线 | M0 |
| arm64 工具缺失 | 安装困难或被迫模拟 x86 | 先验证原生 arm64 包；无法满足时评估仿真成本，不默认接受 | M0 |
| 宿主机与容器结果不一致 | 结果难以复现 | 统一 Dockerfile、命令入口、报告 schema 和容器内 smoke 回归 | 全周期 |
| 挂载泄露或凭据暴露 | 源码/凭据风险 | 最小挂载、禁止把 secret 放项目目录 | 全周期 |
| 容器资源不足 | 测试超时、OOM、错误归因 | 记录 CPU/RAM/RSS；资源失败单独分类，不改判为语义失败 | 全周期 |
| 第二 target 过早分叉 | 同时维护两套 native ABI/runtime | 首发只支持 `aarch64-unknown-linux-gnu`；其他 target 单独立项 | M6/M7 |

---

## 8. 分阶段采用策略

### M0：环境就绪与可行性核验

- 按 digest 钉基础镜像，构建带 `clang` 与钉版 Rust 的镜像，记录 digest；
- 记录 Docker Desktop 版本、容器内核、Linux 架构、Rust、Go oracle、`clang`、libc/target 信息；
- 在容器内运行 harness/解释器/SSA 的最小验证；
- 在容器内完成 ELF native `hello` spike；
- 建立 `.devcontainer` 并**实测断点命中**，交付可用的 Rust 调试环境；
- 对绑定挂载与容器卷做一次构建/测试基准；
- 产出环境 manifest、资源测量、阻塞项和选择结论。

M0 不搭建完整 CI、不下载全部依赖、不开始 M1–M12 的实现，也不把「容器能跑起来」当作编译器路线可行。

### M1 以后：容器内 smoke

- 每个行为切片和 native 相关切片均在容器内测试；
- 报告记录镜像 digest 和 Linux target；
- 只有通过固定环境和目标过滤规则的结果才能进入对应分母。

---

## 9. 原本待决定的事项 —— **M0 已全部拍板**（2026-10-07 回填）

> ⚠️ **本节原为「待讨论并需要在 M0 决定的事项」，写于 M0 开工前。**
> M0 已完成（E1–E10 十条门禁全过），七项**全部有了实测结论**。
> 保留原文是为了让读者看到「当时的判断」，但**下面每一项都已是既定事实** ——
> 别再把它们当悬而未决的问题。

| # | 原本待定 | **M0 的决定** | 依据 |
|---|---|---|---|
| 1 | 基础镜像 digest 的钉法与更新策略 | ✅ **tag + digest 并存**：tag 仅作可读性注释，**解析只走 digest** | `docker/image.lock` 的 `base.tag` + `base.index_digest`；D-M0-7 |
| 2 | 容器的 CPU、内存与磁盘配额 | ✅ **未显式设限**，实测环境为 `nproc=10` / `mem≈7.75 GiB` / Debian 12 bookworm | manifest `environment.resources` |
| 3 | Rust toolchain 用 `rustup` + 钉版本，还是用镜像内置 | ✅ **`rustup` + `rust-toolchain.toml` 钉版（1.98.1）**，且该文件是版本的**唯一来源** | D-M0-8；`rust-toolchain.toml` |
| 4 | 依赖缓存放容器卷还是绑定挂载 | ✅ **三个命名卷**：`rgoc-target` / `rgoc-cargo-registry` / `rgoc-cargo-git` | D-M0-9；T18 实测 bind mount 慢 **2.3×**（`M0-benchmarks.md` §4） |
| 5 | 容器长期运行还是每次 `docker run` | ✅ **双形态同镜像**：开发用长期驻留 devcontainer，门禁用一次性 `docker run` | D-M0-10 |
| 6 | 是否需要 CI 使用同 digest 的等同容器 | ✅ **暂不建 CI**，但「同 digest 可接入」的能力已具备（`in-container.sh` 即该入口） | `03` §4 M0 砍项「不实施完整 CI」 |
| 7 | 是否需要第二个 Linux target | ✅ **不建**（首发固定 `aarch64-unknown-linux-gnu`）；双 target 维护成本不划算 | D-M0-3；`03` §0.2 |

**两个「定了但要记住后果」的**：

- **第 1 条的代价**：镜像**位级不可复现**（14 层里 6 层 digest 变）⇒
  **image id 不得写进门禁**，钉子只能用 `base.index_digest` + `src.*_sha256`。
  实测见 `M0-benchmarks.md` §5。
- **第 4 条的代价**：命名卷的属主问题（新建空卷为 `root:root`，须 `chown 501:20`；
  且 `in-container.sh` 的 bootstrap **只在新建空卷时**接管，已存在但属主错的卷不会被修）。
  这是换机器时最常踩的环境坑，处置见 `M0-benchmarks.md` §6。

---

## 10. 与路线图的关系

本方案落实并细化了 [`03-roadmap.md`](./03-roadmap.md) 的以下约束：

- Go 语料版本为 `go1.27.1`；
- 首发宿主为 Linux/arm64（由 Docker 容器提供），目标为 `aarch64-unknown-linux-gnu` / ELF；
- M0 必须完成镜像 digest 锁定、工具链锁定、harness 和三个 spike；
- native `hello` 在容器内于 M0 验证，M1 进入 smoke 回归；
- 容器是唯一开发和首发验证环境，宿主机不提供替代工具链。

**当前状态**：本机 Docker Desktop 已验证可启动，架构与内核已实测记录（§3.1）。但**容器镜像、工具链、环境 manifest、`.devcontainer`、CI 和任何编译器实现均未因本文而搭建或执行**。
