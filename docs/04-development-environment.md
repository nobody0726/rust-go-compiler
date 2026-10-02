# 开发环境方案：Lima Linux VM

> 状态：方案稿，尚未搭建、安装或修改宿主机配置。本文记录方案、约束、风险和待决策项；不代表环境已经满足 M0 门禁。  
> **文档索引**：[`README.md`](./README.md)　|　**上游**：[`03-roadmap.md`](./03-roadmap.md)

## 1. 目标与结论

项目统一在 Lima 管理的 Linux VM 内完成开发、测试和首发 native 验证：

1. Rust 编译器本体、harness、测试和普通开发工具运行在固定的 arm64 Linux guest。
2. 首发 native 目标固定为 `aarch64-unknown-linux-gnu` / ELF，使用 VM 内的 Linux `clang`、系统库和 linker 做真实验证。

因此不再维护 macOS/Darwin 第二套首发环境。宿主机只负责运行 Lima；编译器的工作目录、工具链、harness、native 输出和验收命令均在 VM 内执行。

## 2. 建议的职责边界

| 工作 | Lima Linux VM | 宿主机 |
|---|---:|---:|
| Rust 编译器构建、格式化、静态检查 | 主环境 | 可复核 |
| parser、type checker、HIR、解释器单元测试 | 主环境 | 可复核 |
| SSA verifier、Test IR、harness 自测 | 主环境 | 可复核 |
| `aarch64-unknown-linux-gnu` 汇编/对象/链接 | 唯一首发门禁 | 仅运行 Lima |
| Linux SDK、ELF、系统库 | 唯一真实来源 | 不作为工具链来源 |
| Go `1.27.1` reference oracle | 以可验证安装为准 | 以可验证安装为准 |
| 全量/长时测试 | 夜间或资源允许时 | 夜间或发布前复核 |

代码、manifest、测试结果和工具链清单应由仓库保存；不要把 VM 磁盘状态当作项目状态。每次报告记录 Lima 实例、配置摘要、镜像摘要、guest kernel、工具路径、版本和必要 hash。

## 3. Lima VM 候选基线

### 3.1 初始形态

第一阶段建议使用 Lima 管理一个单独的 arm64 Linux VM，优先选择：

- arm64 Linux guest，与宿主的 arm64 架构保持一致，避免第一阶段引入 x86 模拟开销；
- 有明确版本或 digest 的发行版镜像；
- 非 root 日常开发用户；
- 宿主目录只挂载项目工作区或专用工作区，不默认挂载整个 home；
- 使用 Lima 的 SSH/端口转发能力，不把开发服务暴露到局域网；
- VM 的 CPU、内存、磁盘和共享目录策略写入版本化配置，并在 M0 根据可用机器资源确定。

这只是候选基线，不在本阶段假定某个发行版、镜像 URL 或 Lima 配置已经验证。环境文件应在试装后补充镜像摘要、Lima 版本、guest kernel、资源值和已知限制。

### 3.2 VM 内工具分层

建议分三层管理：

1. **系统包**：编译器、链接器、`pkg-config`、调试工具、证书和基础构建工具。
2. **版本锁定工具链**：Rust toolchain、脚本运行时以及项目要求的固定工具版本。
3. **项目工具**：`cargo` workspace、harness、测试语料和报告工具，全部由仓库入口统一调用。

系统包安装方式和版本快照必须可重放。Lima 配置与 VM 内初始化步骤必须明确写出 `rustc`、`cargo`、Go oracle 和 `clang` 的实际路径、版本和来源；不能依赖宿主机 PATH 的偶然继承。

Go reference oracle 有特殊约束：仓库语料是 `go1.27.1`，当前环境里的其他 Go 版本不能直接作为基线。M0 需要确认匹配的 Go 工具链获取方式、`go version`、路径和校验值；若匹配版本只能从源码构建，构建过程和产物 hash 也必须记录。

## 4. 宿主机与 VM 的交互

### 4.1 推荐工作流

```text
编辑器/终端
  └─ 进入 Lima VM：cargo、harness、解释器、SSA、ELF native smoke/milestone
```

统一入口应通过 Lima 进入 VM，命令输出声明 Lima 实例、Linux target 和实际工具路径。不要让命令根据宿主机 PATH 静默选择不同 oracle 或 linker。

建议优先让测试 harness 在目标环境执行，而不是让 VM 通过共享目录直接修改宿主构建产物。共享目录主要用于源代码、manifest、日志和报告；编译缓存可留在 VM 或宿主各自的本地磁盘，以减少文件系统同步性能和权限问题。

### 4.2 native 结果的来源标记

每个 native 结果至少记录：

- target triple：`aarch64-unknown-linux-gnu` 或明确的辅助 target；
- object format：ELF；
- `clang`、Linux sysroot、libc 和 linker 路径；
- 生成器 commit/工作区状态（若仓库尚无 Git，则记录源码清单）；
- 输入 fixture、seed、stdout、stderr、退出码和资源预算。

VM 内生成的 ELF 结果就是首发 native 结果；其他 target 的结果必须标记为 `target-filtered`，不能进入首发分母。

## 5. 为什么使用 Lima Linux VM

Lima 把 Linux guest 的发行版、CPU/内存/磁盘、挂载策略和启动方式集中管理，使开发者在同一个 Linux 环境里完成编译器、harness 和 native 验证。首发目标本身就是 Linux/ELF，因此不会再维护一个 macOS/Darwin 交叉验证路径。

宿主机仍可能影响 VM 性能、文件共享和网络，但这些属于 Lima 运行条件，不属于编译器的第二套目标环境。任何宿主相关差异都应通过版本化 Lima 配置、资源记录和测试报告处理。

## 6. 资源与性能建议

资源值暂不拍脑袋固定，在 M0 通过 Lima smoke 测量后锁定。需要测量：

- VM 启动时间、磁盘占用、可用磁盘余量；
- CPU/内存配置下的 `cargo check`、单元测试和 smoke wall time；
- 共享目录与 VM 本地磁盘上的构建/测试差异；
- harness 峰值 RSS、长时测试资源消耗；
- VM 内 ELF native smoke 的编译和运行耗时。

与 [`03-roadmap.md`](./03-roadmap.md) 的初始目标保持一致：smoke 不超过 5 分钟，milestone 不超过 30 分钟，full 单次不超过 2 小时；真实测量后只做有记录的调整，不能用提高超时来掩盖环境问题。

## 7. 主要风险与控制措施

| 风险 | 影响 | 控制措施 | 触发的决策 |
|---|---|---|---|
| VM 与宿主目标混淆 | 宿主工具链绕过 VM，结果不可复现 | 结果强制带 Lima/target/format；首发门禁只在 VM 执行 | M0/M7 |
| Lima 共享目录性能或权限问题 | 编译变慢、文件模式/时间戳异常 | 源码与缓存分离做基准；必要时在 VM 本地 checkout/缓存 | M0 |
| VM 镜像漂移 | 结果不可复现 | 镜像版本/digest、Lima 版本、guest kernel 写入 manifest | M0 |
| arm64 guest 工具缺失 | 安装困难或被迫模拟 x86 | 先验证原生 arm64 包；无法满足时评估 Rosetta/仿真成本，不默认接受 | M0 |
| Go oracle 版本不匹配 | 差分结论无效 | 固定 `go1.27.1`，路径/版本/hash 不匹配即拒绝作为基线 | M0 |
| 宿主机与 VM 结果不一致 | 结果难以复现 | 统一 Lima 配置、命令入口、报告 schema 和 VM 内 smoke 回归 | 全周期 |
| 共享目录泄露或暴露服务 | 源码/凭据风险 | 最小挂载、SSH/本地端口、禁止把 secret 放项目目录 | 全周期 |
| VM 资源不足 | 测试超时、OOM、错误归因 | 记录 CPU/RAM/RSS；资源失败单独分类，不改判为语义失败 | 全周期 |
| 第二 target 过早分叉 | 同时维护两套 native ABI/runtime | 首发只支持 `aarch64-unknown-linux-gnu`；其他 target 单独立项 | M6/M7 |

## 8. 分阶段采用策略

### M0：只做 Lima 可行性核验

- 安装/启动 Lima 和 Linux guest 的实际可行性核验；
- 记录 Lima、镜像、guest kernel、Linux 架构、Rust、Go oracle、`clang`、libc/target 信息；
- 在 VM 内运行 harness/解释器/SSA 的最小验证；
- 在 Lima VM 内完成 ELF native `hello` spike；
- 对共享目录与本地磁盘做一次构建/测试基准；
- 产出环境 manifest、资源测量、阻塞项和选择结论。

M0 不搭建完整 CI、不下载全部依赖、不开始 M1–M12 的实现，也不把“Lima 能启动”当作编译器路线可行。

### M1 以后：Lima 内 smoke

- 每个行为切片和 native 相关切片均在 Lima VM 内测试；
- 报告记录 Lima 实例和 Linux target；
- 只有通过固定环境和目标过滤规则的结果才能进入对应分母。

## 9. 待讨论并需要在 M0 决定的事项

以下事项现在不强行拍板：

1. Linux guest 发行版及其镜像固定方式；
2. Lima VM 的 CPU、内存、磁盘和共享目录模式；
3. Rust toolchain 是使用仓库锁定文件、系统安装还是两者组合；
4. Go `1.27.1` oracle 使用匹配预构建包还是源码可重复构建；
5. Lima VM 是人工使用、专用实例还是后续 CI runner；
6. 是否需要第二个 Linux target，以及它是否值得承担双 target 维护成本；
7. 是否将 VM 配置版本化为 Lima template，还是先以版本化配置和手册验证，避免过早承诺自动化安装。

默认建议是：先固定一个原生 arm64 Linux guest 和最小 VM 资源，先完成 M0 测量；所有首发 native 验证都在该 VM 内完成。只有测量结果显示 Lima 共享目录严重拖慢迭代时，再把 checkout/cache 分离到 VM 本地，不提前复杂化工作流。

## 10. 与路线图的关系

本讨论稿落实并细化了 [`03-roadmap.md`](./03-roadmap.md) 的以下约束：

- Go 语料版本为 `go1.27.1`；
- 首发宿主为 Linux/arm64（由 Lima VM 提供），目标为 `aarch64-unknown-linux-gnu` / ELF；
- M0 必须完成 Lima 配置/镜像锁定、工具链锁定、harness 和三个 spike；
- native `hello` 在 Lima VM 内于 M0 验证，M1 进入 smoke 回归；
- Lima VM 是唯一开发和首发验证环境，宿主机不提供替代工具链。

**当前状态**：本文只完成方案记录。Lima、Linux VM、Rust/Go 工具链、环境 manifest、CI 和任何编译器实现均未因本文而搭建或执行。
