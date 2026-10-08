# Rust 版 Go 编译器迭代计划（ROADMAP v2）

> **输入规格**：[`01-feature-set.md`](./01-feature-set.md)（功能 ID）+ [`02-test-inventory.md`](./02-test-inventory.md)（测试线索）。两者是追踪输入，不等于已经实现或可以直接复用的测试。  
> **方法论**：Superpowers TDD，行为切片先测试，阶段交付端到端验证。  
> **项目代号**：`rgoc`。本次仅修订计划，尚未创建编译器工程，也未完成任何里程碑。  
> **文档索引**：[`README.md`](./README.md)　|　**上游**：[`01-feature-set.md`](./01-feature-set.md)、[`02-test-inventory.md`](./02-test-inventory.md)　|　**下游**：[`04-development-environment.md`](./04-development-environment.md)

---

## 0. 目标与兼容性边界

### 0.1 四条交付线

| 发布线 | 阶段 | 可交付能力 | 不要求同步完成 |
|---|---|---|---|
| A：前端 | M0–M4 | `rgoc lex/parse/check`；固定版本、单包/多文件/显式依赖的静态检查 | 解释器、机器码、runtime |
| B：顺序解释 | M5 | `rgoc run`；基础值、函数、闭包、接口动态语义和初始化的定义子集 | 并发、GC、完整标准库 |
| C：native MVP | M6–M7 | 单平台 `rgoc build`；最小代码生成、系统链接、与解释器差分 | 完整 B 子集、Go internal ABI、并发 runtime |
| D：能力扩展 | M8–M12 | 解释器并发、native 对象模型/GC/调度、泛型和有限反射 | 所有扩展一次性完成 |

**首个 native 交付是 C，不以 D 完成作为前置条件。** D 的子阶段可单独发布；STW GC、并发 GC、第二平台与完整反射有不同验收边界。

### 0.2 固定兼容性矩阵

| 维度 | 本修订版固定选择 | 说明 |
|---|---|---|
| 语言/语料版本 | 本地 `go_source_code/VERSION` 标记的 `go1.27.1` | 固定输入快照，不宣称它是当前最新官方版本 |
| 参考工具链 | 容器内与语料匹配的 `go1.27.1`（基础镜像 `golang:1.27.1` 锁版），记录二进制路径和校验值 | 宿主机 Go（当前 `go1.24.5`）不作为基线；不能用不匹配版本做 oracle |
| 首发宿主 | Linux / arm64（由 **Docker 容器**提供） | 开发、测试和首发 native 验证统一在固定容器内完成 |
| 首发目标 | `aarch64-unknown-linux-gnu`，ELF | 本修订版采用单平台首发；更改须更新决策、manifest 和门禁 |
| 最低系统版本/SDK | M0 锁定基础镜像 **digest**、Docker Desktop 版本、容器内核和 libc 基线 | 不依赖宿主 macOS SDK；CI 使用同 digest 的等同容器 |
| 汇编/对象/链接 | 自研 SSA→arm64 汇编；容器内系统 `clang` 产出 ELF 对象并链接 | 首版不自研 assembler、ELF writer 或 linker |
| 调用约定 | 首发平台 ABI + 项目私有调用约定，明确 runtime 桥接 | 不读写官方 Go `.a`，不承诺 Go internal ABI 兼容 |
| 包加载 | 源码加载：同包多文件 + 显式依赖目录映射 | 模块下载、完整 module resolver、官方 export data 格式不在首发 |
| 标准库 | 起步仅预声明内建；按明确包清单逐步加入源码支持 | 无支持的 `fmt/sync/runtime/reflect` 导入必须报 unsupported，不用静默桩骗过测试 |
| `unsafe` | 静态类型/布局检查分阶段支持 | 未实现的内存操作必须显式拒绝；native 另验收 |
| 泛型 | M3/M4 静态语义，M5 定义子集解释，native 先单态化 | shape/dictionary 独立优化，不阻塞首个 native 程序 |
| runtime | 宿主解释器与 native 实现分离 | 允许系统 libc、线程及虚拟内存 API；不承诺 freestanding |
| 第二平台 | 候选 `x86_64-unknown-linux-gnu` / ELF | C 稳定后另定资源与测试，不与首发并行承诺 |

**首发非目标**：完整标准库、cgo、race detector、wasm、plugin、用户 Go 汇编兼容、动态链接/插件能力、完整 DWARF/coverage/PGO、Go ABI 兼容、完整反射、性能追平官方 Go、并发 GC和异步抢占。系统链接默认使用容器内系统库，不承诺全静态可执行文件。

版本或平台调整须作为独立决策更新语料 manifest 和阶段测试集，不能为了通过某个用例临时换版本。本地快照无 Git 元数据时，采用排序后的源码路径/文件 SHA-256 清单锁定；`VERSION` 文件本身不足以证明内容一致。

**锁定状态**：Go 版本、Linux/arm64 宿主、首发架构和对象格式已在计划中确定。基础镜像 digest、Docker Desktop 版本与容器内核、匹配 Go 工具链的安装与校验、源码 hash 清单和 libc 基线的实测锁定仍是 M0 交付；本次文档修改不代表这些环境任务已经执行。

### 0.3 完成度与功能 ID

不再以“246 项全勾选”作为总退出条件。保持原功能 ID，分别跟踪四个维度：

- **Frontend**：词法、语法、常量、类型、作用域、包和静态诊断。
- **Interpreter**：顺序/并发执行、初始化、闭包、接口和异常语义。
- **Native**：lowering、SSA、调用约定、代码输出、启动和系统链接。
- **Runtime/metadata**：对象模型、栈、调度、GC、类型信息和有限反射。

每个 ID 的记录包含适用维度、当前阶段、直接测试、状态（未做/子集/已验收/委托系统/延期）和缺口。同一功能的静态、解释、native 实现不能互相替代完成证据。官方内部算法测试通过不等于 Go 语言兼容；使用系统工具也不等于自研功能已完成。

四类指标分别报告“已验收的功能 ID/该维度冻结的目标功能 ID”、子集/委托/延期数量，以及直接测试覆盖率。一个 ID 可适用于多个维度，四类分母不要求相加等于 246；目标分母调整须保留前后报告，不能通过删除未实现项抬高完成度。M0 建立逐 ID 的适用维度清单，§5 是归属规则，不是已经完成的逐项盘点。

---

## 1. 决策与依赖关系

### 1.1 修订后的决策记录

| # | 决策 | 选择 | 对原计划的调整 |
|---|---|---|---|
| D1 | 双轨 | HIR 解释器做语义参考，native 早期验证 | 原 M8 的后端可行性验证前移至 M0/M7 |
| D2 | runtime | 宿主解释器 → 极小 native runtime → STW → 调度/并发 GC | 不在一个阶段承诺完整 GMP、GC、抢占和定时器 |
| D3 | 代码基线 | 新实现，参考官方设计和既有仓库 | spike 不视为生产实现完成 |
| D4 | 泛型 | 类型集/统一地基先设计，按行为实现 | 不以整套 types2 架构照搬替代需求分析 |
| D5 | 平台 | 首发 arm64/Linux（Docker 容器），系统工具链 | 去掉早期双架构与自研链接器承诺 |
| D6 | 测试 | 只读语料 + Test IR + 分层执行 | 数量、兼容性和通过率由 manifest 决定 |
| D7 | 范围控制 | 先正确性，内部算法和性能优化另排期 | SwissTable、shape 共享、内联不阻塞首发 |

### 1.2 依赖图

```text
M0 环境/语料锁定 + 解释/HIR、SSA、native 三个 spike
│
M1 词法 → M2 AST → M3 类型核心/最小包模型 → M4 静态检查闭环
                                                    │
                                                 M5 顺序解释
                                                    │
                                ┌───────────────────┴─────────────────┐
                              M6 SSA                              M8 解释器并发
                                │                                      │
                              M7 native MVP                           │
                                └───────────┬──────────────────────────┘
                                          M9 native 对象模型/分配/metadata
                                            │
                                          M10 精确 STW GC
                                            │
                                          M11 native 栈/并发/调度/定时器
                                            │
                                          M12 泛型扩展/有限反射/优化
```

M6/M7 与 M8 只在写入范围和契约独立时并行。M9a 的分配/metadata 可在 M7 后推进；M9 的 channel 设计和 M11 的 native 并发验收必须等 M8 语义基线。并发 GC、异步抢占、第二平台等从各自前置阶段分出独立可选任务，不绑定 M12 的正确性交付。

**原计划与修订版编号对照**：此前“拆分 M5、M6、M8、M9”的建议指原计划编号。后端验证前移后，编号已经变化，按下表追踪对应工作。

| 原里程碑 | 修订后的独立阶段 | 调整结果 |
|---|---|---|
| 原 M5：HIR/解释器/简版 runtime | M5a–M5f；native 分配器移至 M9a | 顺序解释分步验收，移除并发与 native runtime 前置 |
| 原 M6：并发/异常/runtime 定型 | M8a–M8e；native 布局与 channel 契约移至 M9 | 并发语义、异常和冻结分别验收 |
| 原 M8：机器码/runtime.a | M0 native spike、M7a–M7d、M9、M11a–M11c | 拆开可行性、代码生成、对象模型、栈与调度 |
| 原 M9：GC/完整 runtime | M10a–M10c、M11d–M11f | STW、定时器、多线程和并发 GC 分别验收 |
| 修订版 M6：SSA | M6a–M6e | 构建、求值、优化和 GC metadata 各有门禁 |

所有子阶段共享 §3/§6 的验证要求，并须拥有自己的必需测试 ID、交付报告和退出条件。后续子阶段不能通过“整个 M<n> 总体通过”替代当前子阶段的验证。

### 1.3 必须版本化的跨阶段契约

| 契约 | 首次定义 / 验证 | 后续消费者 |
|---|---|---|
| SourceMap、file/line/column、诊断排序 | M0/M1；M2 错误恢复验证 | checker、harness、debug dump |
| Test IR、构建条件、比较器和结果格式 | M0 | 所有阶段 |
| 包身份、源码 importer、导出符号摘要、初始化顺序 | M3/M4 | M5 执行、M7 startup、M9 runtime |
| HIR 多返回值、可寻址性、闭包/方法值、异常传播 | M0 spike；M5 正式实现 | SSA、解释器、native |
| SSA tuple/memory、Phi、支配关系、调用边界 | M0 spike；M6 verifier | codegen、liveness |
| ABI、frame layout、对象布局、runtime symbol bridge | M0 spike；M7 MVP；M9 完整化 | 分配、接口、GC、栈切换 |
| typedesc、GC roots、live set、stack map、safe point | M6 设计；M7 样例 metadata；M9 全支持子集验证 | M10 扫描、M11 栈/调度 |

GC 链条明确为：类型布局 → typed allocation → SSA 指针活跃集 → 调用点/回边安全点 → frame/stack maps → 全局/堆/栈/保存寄存器 roots → 精确扫描。**不能先生成任意栈帧，M10 才补根来源。**

---

## 2. 工程骨架与实现边界

**仓库结构（单仓单根）**：文档与代码同处一个仓库；`docs/contracts/` 与 `docs/milestones/` 位于顶层 `docs/` 下、与 `01`–`04` 同级，`rgoc/` 只承载 Rust 工程。M0 只创建当期需要的模块，后续按依赖引入，不一次性建立全部空 crate。

```text
rust_go_compiler/                           # 仓库根
├── .devcontainer/                         # VSCode Dev Containers 配置（引用 docker/Dockerfile）
├── .dockerignore                          # 构建上下文裁剪
├── .vscode/                               # 调试启动配置（CodeLLDB）
├── AGENTS.md                              # 工程入口
├── corpus-manifest.sha256                 # 语料锁定清单
├── rust-toolchain.toml                    # Rust 版本唯一来源（D-M0-8）
├── docker/
│   ├── Dockerfile                         # 唯一镜像定义（开发容器与门禁容器共用，D-M0-10）
│   └── image.lock                         # 基础镜像 digest 与构建元数据
├── scripts/                               # 容器入口（in-container.sh）/ 离线安装（install-*）/ 自检与冒烟测试
├── docs/
│   ├── README.md, 01-…, 02-…, 03-…, 04-…
│   ├── contracts/                         # 跨阶段接口与版本化契约
│   └── milestones/                        # <阶段ID>-design.md、-tests.md、-plan.md、-manifest.json
└── rgoc/                                  # Rust 工程（cargo workspace）
    ├── Cargo.toml
    ├── crates/
    │   ├── rgoc-lex/          # token / SourceMap
    │   ├── rgoc-ast/          # AST / parser
    │   ├── rgoc-const/        # arbitrary-precision constants
    │   ├── rgoc-types/        # scopes / types / inference
    │   ├── rgoc-loader/       # source packages / imports / init graph
    │   ├── rgoc-hir/          # shared semantic lowering
    │   ├── rgoc-interp/       # host execution
    │   ├── rgoc-ssa/          # IR / verifier / diagnostic evaluator
    │   ├── rgoc-codegen/      # arm64 assembly / ABI / metadata
    │   ├── rgoc-runtime/      # native runtime and host boundary
    │   ├── rgoc-harness/      # Test IR / oracle / comparators
    │   └── rgoc-driver/       # lex / parse / check / run / build
    ├── tests/{smoke,milestone,corpus}/
    ├── runtime/native/aarch64-unknown-linux-gnu/  # startup / assembly bridge
    └── xtask/                 # corpus / reports / environment manifest
```

**M0 只创建当期需要的部分**：`docker/`、`scripts/`、`rust-toolchain.toml`、`.devcontainer/`、`.vscode/`，以及 `rgoc/` 中的 `rgoc-harness`（+ 三个 spike 各自所需的最小 crate）。其余 crate **不建空壳**，按里程碑依赖引入。

不设首发 `rgoc-linker`；使用容器内的 `clang` 输出 ELF 并链接项目 runtime。生产代码不把官方 Go `.s` 直接喂给系统汇编器。runtime 的 Rust/C/汇编桥接方式在 M0 spike 中验证，包括 unwind 边界、符号、栈对齐和工具链适配。

**包/API 边界**：内部 runtime 调用 libc 不等于支持用户 `import "C"`，cgo 仍明确拒绝。`unsafe` 初始白名单仅规划 `Sizeof/Alignof/Offsetof` 的静态求值/布局验证；`unsafe.Pointer` 转换、`uintptr` 往返及内存访问须另列支持条件，并在 native 根协议验证前拒绝执行。`reflect` 首发不提供包实现；M12 的有限反射必须先冻结包/API 白名单，未列入的方法和动态调用显式拒绝，不能以内部 typedesc 已存在作为 `reflect` 支持证据。

解释器 Rust `HashMap` 只能是底层存储，外层仍需实现 Go key 的可比较性、浮点/NaN、interface 动态类型、更新/删除时迭代等行为；不要求复刻 SwissTable 内部布局。解释器对象 ID/引用和 native 指针/头结构分离，不把宿主内存管理当作 Go GC 完成证据。

不默认引入新第三方依赖。任意精度、编码等需求在阶段计划中列明“自行实现/既有工具/新增依赖”权衡；新增依赖须单独明确请求与授权，不能借本路线图自动添加。

---

## 3. 测试、统计与 TDD 协议

### 3.1 M0 建立语料 manifest

输入是 `go_source_code/test/`、`src/internal/types/testdata/` 和 syntax 等目录。**路线图不固化目录数量为验收分母。** M0 分别统计文件数、独立用例数、目录用例、版本/平台过滤数、模式和依赖；辅助 `.out`、`.dir` 文件不是独立测试。

Test IR 至少记录：用例 ID、相对路径、输入文件集合、模式、指令参数、build tags、目标/版本、import/标准库需求、功能依赖、比较器、期望退出码/输出/诊断、超时、资源上限、seed、归属阶段、unsupported 原因。

- 指令解析支持前导注释/构建条件，不假定文件首行总是指令；目录测试、生成后再编译测试须单独驱动。
- `// errorcheck -m/-d`、`asmcheck` 等编译器内部期望不能当纯语言语义验收。
- 官方 `*_test.go` 常依赖内部 IR/runtime；只提取可适配的行为或数据，不以文件数估计移植成本。
- [`02-test-inventory.md`](./02-test-inventory.md) 是定位线索；“已有测试”不等于“可直接复用”。本计划的适配规则优先，不改动官方语料。

### 3.2 三层执行

| 层次 | 频率 | 内容 |
|---|---|---|
| smoke | 每个行为切片/CI | 已支持模块单测、10–30 个确定性小程序、M0 native fixture；正式阶段到达后加入 SSA verifier |
| milestone | 阶段门禁 | 冻结 ID 列表、能力矩阵、预算和预期结果的阶段语料 |
| full | 夜间/发布前 | 全量分类扫描、适用语料、长时并发/GC；新平台立项后才加入多平台 |

暂不存在的模块不算 smoke 失败，但不能把跳过项标为通过。

### 3.3 结果与分母不可操纵

状态为：`pass`、`expected-unsupported`、`target-filtered`、`compiler-failure`、`runtime-failure`、`harness-failure`、`reference-toolchain-failure`、`timeout/resource-failure`、`nondeterministic-failure`。

- 语义通过率 = `pass / 固定阶段中声明支持的用例数`；基建失败、超时仍在分母，不因跑不起来而剔除。
- 同时报“已支持用例/目标适用用例”“unsupported/过滤项”和四维功能完成度，不只报通过率。
- 白名单须在开工前冻结，含原因、负责人、移除条件；新增跳过必须单独记录变更，保留原分母报告。
- 必需门禁用例 **100% 通过**、无基建失败/不稳定失败；探索集的百分比仅是进展指标，不决定阶段退出。

### 3.4 比较与可重复性

- **deterministic mode**：harness 提供显式的确定性运行模式，固定 seed、单 harness worker、环境变量/locale、语料枚举顺序和资源预算，并记录完整运行配置。M0 只纳入无外部时序依赖的 fixture；M8 的解释器控制调度/map/select 随机源，时间相关 fixture 使用 fake clock；native 调度回放须在 M11 的对应能力验收后才能声明支持。真实时钟、系统线程或外部 I/O 尚不可控的用例标记为不适用，转性质/压力测试，不静默忽略差异。固定配置不保证真实耗时一致，也不控制官方 runtime；不能用排序 map 输出改变被测语义。并发压力另跑多 seed 集，不能把确定性回放当作并发正确性证明。
- 分别捕获 stdout、stderr、退出码；不得默认把 `print/println` 当 stdout。具体期望由匹配工具链和 fixture 确认。
- 没有 `.out` 的 `// run` 用例按官方模式约定判定，不猜测缺失文件就是任意输出可通过。
- 诊断先对齐拒绝/接受、位置和预定义类别，文本/regex 另列指标；禁止“任意错误即通过”，检测未预期额外错误。官方诊断无统一类别编码时通过明确的适配表/自有语义 fixture 判定，未知类别不强行归类。
- map 顺序/select 调度不应与参考进程逐字 diff；优先使用程序内语义断言/集合或性质比较器。固定 seed 只保证 rgoc 回放，不控制官方 runtime，也不能替代多 seed 压力/公平性检查。
- panic 比较消息/语义和退出状态；地址、栈格式等实现细节不纳入首发精确一致性。
- 子进程隔离、墙钟超时和资源限制；容器内可硬限制的指标使用 cgroup/ulimit 等机制，并记录容器与宿主资源边界。
- 失败保存源文件、flags、版本、seed、轨迹、阶段 dump；先人工缩减并固化回归，自动 reducer 在有复现 fixture 后作为独立工具任务。

**差分最小化协议**：先在相同配置下连续复现 3 次；不稳定失败先归入 `nondeterministic-failure` 并保留轨迹，不直接运行普通 reducer。稳定语义差异的缩减候选必须保持 oracle 可接受、仍在已声明支持子集内、失败类别/比较器和目标差异不变；崩溃/诊断差异按其原判据处理，不接受变成无关语法错误或 unsupported 的候选。按文件/声明/语句/表达式逐层人工缩减，保存原始与最小样本、判定命令及每次结果；自动化沿用同一判据。每个失败首轮缩减上限 2 小时，未缩完保留当前样本与阻塞说明，已确认的语义失败仍阻塞所属门禁。

### 3.5 TDD 与提交

行为切片执行 RED → GREEN → REFACTOR：先确认失败来自缺失行为而非环境/harness，再最小实现。阶段开始前生成测试先行清单，记录来源、适配方式、依赖、比较器和验证命令。

不机械按功能 ID 或 2–5 分钟拆任务，也不强制每次红绿一个 commit。按一个可回滚的语言行为、IR 不变量、runtime contract 或端到端切片组织；提交仅在明确要求提交时执行，使用仓库 Lore 协议并引用功能 ID/验证证据。

---

## 4. 里程碑详细计划

### M0：工具链、harness 与三个架构 spike

**目标**：先证明管道/边界可行，不承诺完整语言实现。预计资源窗口见 §6。

1. 启动固定 Docker 容器：按 digest 钉基础镜像（`golang:1.27.1-bookworm`）并构建含钉版工具链的镜像；环境 manifest 记录 `rustc/cargo`、Go oracle、`clang`、Docker Desktop 版本、容器内核、libc 和资源配置。
2. 锁定源码快照/许可、版本和参考工具链，并把语料**获取与校验步骤**写入文档。可用匹配预构建包或可重复构建方案；宿主机工具链不替代容器内的首发基线。
3. 构建 Test IR/manifest 与小型 harness。枚举所有指令；v0 优先执行 run/compile/纯前端 errorcheck，其他模式显式分类，不在 M0 重写完整官方 runner。
4. **解释 spike**：最小 parser 或明确标识的固定 AST → HIR → `println(1 + 2)`；支持范围只限这个 fixture。
5. **SSA spike**：固定 HIR → Block/Value → 求值；识别 memory/tuple/调用需求，复杂验证留 M6。
6. **native spike**：固定 HIR/SSA 函数 → arm64 汇编 → `clang` → 可运行程序；验证调用、栈对齐、输出流、退出码、最小 runtime 桥接。至少包含一个输出 `hello` 的固定 fixture。

**测试门禁**：harness 成功/失败/未知指令/平台过滤/超时/不匹配版本均有测试；官方基线至少 20 个按模式分层的确定性样本 100% 通过；三个 spike 的输入、结果和环境全部可复现。native `hello` 在 M0 通过后纳入 M1 smoke 回归，M1 只能验证回归，不得推迟首次后端验证。

**交付**：版本锁定清单、harness 自验报告、三份 spike 决策记录、契约初稿。spike 可隔离保留，后续用测试驱动的正式实现替换，不盲目演进临时代码。

**砍项**：不做标准库、全量 runner、自研链接器。若 native spike 被环境或桥接阻塞，先记录原因并调整方案；未证明关键路线可行前不展开后端大规模实现。

### M1：词法与位置（LEX、PIPE-02/25 前置）

UTF-8/Unicode、注释、token 最长匹配、分号、数字/rune/string、错误位置与资源上限。参考 syntax scanner 和自有边界 fixture；必要时使用匹配 Go 的专用 scanner adapter 获取 token oracle，不使用 `go tool compile -E` 假定为 token 输出。

**门禁**：LEX 各项直接测试、词法 golden、非法 UTF-8/超长输入等负例全绿；token 与位置差异可定位。不需要整个程序通过类型检查。

#### M1 smoke 清单（含 M0 带入的 native fixture）

| smoke ID | 内容 | 来源 | M1 的判定 |
|---|---|---|---|
| `SM-M0-NATIVE-HELLO` | 固定 HIR/SSA → arm64 汇编 → `clang -nostartfiles -Wl,-s` → ELF；运行 stdout 精确 `hello\n`、退出码 `0` | **M0 的 `T-S3-03`**（T44，2026-10-07） | **回归**：必须继续通过。**不得**因为 M1 引入真实 lexer 就跳过或改写它 |
| `SM-M1-LEX-GOLDEN` | 词法 golden（token 序列 + 位置） | M1 新增 | 本阶段门禁 |

> **这条 smoke 的边界（不要误读）**：M0 的 `hello` 是**固定 HIR/SSA** 验证 ——
> 它证明「后端路线可行」，**不代表 M1 已能从 Go 源码生成机器码**。
> M1 引入 lexer 后，`SM-M0-NATIVE-HELLO` 仍是**同一个固定 fixture**（`rgoc-spikes` 的
> `fixtures::s3_hello()`），它的输入**不经过 lexer**。等 M2 有了 parser，
> 才谈得上「源码 → 机器码」。把它提前当成端到端通过，是 M0 就前移后端验证（`03` §4 第 6 条）
> 最容易被误用的地方。

### M2：AST、parser 与版本/指令前置（AST、PIPE-03/04/05/25）

表达式/类型/声明/语句、位置保真、错误恢复、注释/构建条件、AST dump。先支持解析与拒绝未知关键指令；`linkname/nosplit/noescape` 等实现效果列延期，不标完成。

**门禁**：syntax testdata 和自有 AST/位置/错误恢复切片全绿；parser 测试不等于 `run/compile` 全集已支持。测试导入路径合法语法，但不依赖完整 importer。

### M3：常量、类型核心、最小包模型（CON、TYP-01..18/22、SCP、PIPE-06/07/08/09）

1. 类型表示/identity、作用域、universe、常量精度/舍入/表示性。
2. 单包多文件、package identity、显式源码依赖映射、导出符号摘要和 import cycle 诊断；区分文件作用域与包作用域。
3. 类型项/类型集/约束/统一的直接测试，版本门控；不会宣称整个泛型推断已经完成。

**门禁**：无依赖类型核心切片与 loader fixture 各自全绿，至少覆盖常量长字面量、声明遮蔽、同包跨文件引用。需要复杂 import/初始化的 check 语料留 M4；不要求 check 全集或诊断文本 100% 一致。

**砍项**：只读源码 loader，不读取官方二进制 export data、不下载模块。大整数策略单独决定，不为“至少 256 位”静默截断超过预算的常量；超限用例须按参考规则/明确实现限制处理。

### M4：静态检查闭环（TYP-19..26、EXP/STM/BIF/PKG 静态部分、SYS 子集）

表达式/语句检查、方法集/字段提升、接口实现/断言静态约束、内建签名、泛型实例化/推断、版本门控、main 约束、初始化依赖/循环、导出与多包可见性。

**门禁**：check/spec/examples 按支持矩阵精选的接受与拒绝用例全绿；纯前端 errorcheck 对齐错误位置/类别，额外错误可检测；`.`/`_`/别名 import、init cycle、泛型推断有自有 fixture。fixedbugs 按依赖逐批引入，不一次性门禁全集。

**初始化边界**：此处只算图和顺序；M5 必须执行 import→包变量→init→main；M7 必须生成该顺序的 startup，不能以静态图正确替代运行正确。

**交付 A**：可发布前端子集报告。range over func、loopvar 和 `unsafe` 的静态/执行范围分别声明，不因语法可解析而标记运行语义完成。

### M5：HIR 与顺序解释器（PIPE-10/14/15/16/24，RT-CMP/POLY/EXC 子集）

1. HIR lowering、环境/调用帧、按值/按引用捕获、可寻址性、多返回值。
2. 基础值、数组/struct 拷贝、指针、slice 共享底层存储、不可变 string、Go map 语义适配。
3. 内建、接口动态值/方法调用/断言/type switch、typed nil、method value 和闭包。
4. 解释执行包变量/init/main；defer/panic/recover：直接调用限制、命名返回值、嵌套 panic 必须逐行为测试，不等同普通 Rust `Result` 传播。
5. 泛型解释子集可单态化；loopvar 捕获语义直接测试。range over func（PIPE-16b）可在 M8d 补齐，未支持时显式拒绝。

**不做**：goroutine/channel/select、Go GC、多级分配器、完整标准库；宿主管理内存但仍监控运行预算。

**门禁**：无标准库/无并发的 hello、基础语言和 `ken` 筛选子集，自有接口/闭包/init/defer fixture；milestone 全绿。计划样本目标 60–100 个，最终冻结 ID 数以 M4 支持能力为准，数量不足不以复制用例凑数。

**交付 B**：顺序语义基线；此时只是冻结候选，正式冻结等 M8e；后续增加已声明子集时保持测试，不假装永久不改。

**独立验收子阶段**：下列依赖引用的是已验收交付，不要求未引用子阶段提前完成。

| 子阶段 | 前置 | 交付与独立退出条件 |
|---|---|---|
| M5a：HIR 与最小求值 | M4 | Source→HIR→输出贯通；常量/算术/分支 fixture 全绿，未支持 HIR 节点显式拒绝 |
| M5b：函数、帧与初始化 | M5a | 函数/递归/多返回值及依赖包 init→main fixture 全绿；调用和初始化顺序可检查 |
| M5c：复合值与内建 | M5b | 数组/struct 拷贝、slice 别名、string/map 和内建的正反 fixture 全绿 |
| M5d：闭包与接口 | M5c | 捕获生命周期、loopvar、method value、typed nil、断言/type switch fixture 全绿 |
| M5e：defer/panic/recover | M5d | 延迟参数求值、LIFO、命名返回值、合法 recover 与嵌套 panic fixture 全绿 |
| M5f：顺序基线集成 | M5a–M5e | 冻结顺序语料 ID 全绿；不支持项、资源/超时结果和三方差异均可分类 |

### M6：SSA、verifier 与精简优化（PIPE-17/18/21/26）

HIR→SSA 的 Block/Value/Phi、多返回值 tuple、memory/effect、调用和异常传播表示；支配分析先于依赖它的变换。先常量折叠/死代码/复制传播，再证明必要的 CSE；LICM 和复杂优化延期。

SSA 求值器是诊断设施，共用定义好的值与 runtime 操作，不发展第三套完整 runtime。定义 safepoint live set、指针/派生指针处理和 lowering→frame map 协议。

**门禁**：当前声明 SSA 支持的 M5 切片在 HIR/SSA 求值下零回归，每次 pass 后 verifier 全绿；完整 M5 功能可以分批降级，暂不支持闭包/异常等必须显式报告，不要求“优化 ≥8”。`asmcheck` 不算 SSA 语义证明。

| 子阶段 | 前置 | 交付与独立退出条件 |
|---|---|---|
| M6a：IR 与 verifier | M5f | block/value/type/tuple/memory 定义与非法 IR fixture；verifier 拒绝全部必需反例 |
| M6b：HIR→SSA 构建 | M6a | 分支/循环/Phi/调用/多返回值生成 fixture 全绿；支配/类型/effect 检查通过 |
| M6c：诊断求值与差分 | M6b | 冻结的 SSA 支持切片在 HIR/SSA/官方 oracle 下结果一致；dump 可定位失败 |
| M6d：必要优化 | M6c | 每个 pass 开关前后语义一致、verifier 全绿；超出当期需求的 pass 延期 |
| M6e：live set 与 metadata 协议 | M6c | 指针活跃集、safepoint、派生指针策略的定向 fixture 全绿；输出 codegen 可消费的版本化协议 |

M6d 和 M6e 可独立推进；M7 可行性不依赖优化数量，正式代码生成须依赖 M6b/c/e 已验收的契约。

### M7：单平台 native MVP（PIPE-19..24 子集、SYS-04）

arm64 指令选择、最小寄存器分配、调用/栈对齐、静态/全局数据、startup、汇编输出与 `clang` 系统链接。固定栈、无 GC，采用显式越界/除零/nil 检查；信号→panic 延期，不依赖 Rust unwind 跨 Go frame。

**范围**：内建输出、整数/基础浮点、函数、多返回值、分支/循环、基础数组/struct/初始化。接口/闭包等先为 M9 定义 ABI，并用定向 fixture 验证可扩展性；不承诺 native 已覆盖 B 全集。

**metadata**：在 MVP 指针 fixture 上验证 frame layout、live set/stack map 和保存区，尚未完整覆盖的对象类型留 M9。没有 GC 不意味着可以丢掉这个扩展接口。

**门禁**：`rgoc build` 生成并运行 hello、调用/栈对齐/分支/init fixture；native/解释器/匹配官方 oracle 对确定性结果一致。官方 ABI 测试按语义和标准库依赖筛选，官方寄存器约定/assembly regex 不直接作为首发门槛。

**交付 C**：首个 native 可用子集。链接正确不等于自研 PIPE-23/27 已完成；对象输出可由系统工具委托。

| 子阶段 | 前置 | 交付与独立退出条件 |
|---|---|---|
| M7a：输出与系统链接 | M0 native spike、M6b/c/e | 正式 SSA→汇编→ELF→运行；hello、退出码/输出流 fixture 全绿 |
| M7b：调用与控制流 | M7a | 寄存器/栈帧/多返回值、分支/循环/初始化 fixture 全绿；ABI 桥接可检查 |
| M7c：基础对象与 metadata | M7b | 基础数组/struct/指针 fixture 全绿；样例 stack map/live set 与实际 frame 匹配 |
| M7d：native MVP 集成 | M7a–M7c | 冻结 MVP 语料三方差分全绿，资源/unsupported 报告完整，形成发布 C |

### M8：解释器并发、异常补齐与冻结

- **M8a**：显式 continuation/解释帧、M:1 队列、goroutine 创建/退出、无缓冲 channel；可暂停/恢复设计须在 M5/M6 契约留位置，不能让递归 Rust 调用栈成为唯一状态。
- **M8b**：缓冲队列、nil/满/空 channel、close、唤醒/发送 panic、range。
- **M8c**：select ready/default/阻塞、重复 channel、取消其他候选等待者、公平性多 seed 测试。
- **M8d**：defer/panic/recover 边界、range over func、map 迭代更新/删除；泛型 run 按已支持语义选取，不要求字典/内联测试全过。
- **M8e**：同 seed 可回放、性质测试多 seed 无已知错误，差分失败可缩减，形成冻结候选报告。

**门禁**：channel/select/异常行为 fixture 与 `test/chan` 的适用子集全绿；导入未支持 stdlib 的测试不混入必需分母；阻塞/死锁/进程 main 结束行为可区分。

冻结后只修已声明语义与差分阻塞 bug；需要新增行为必须显式扩展基线，不承诺所有 native 新能力都同步加入解释器。

| 子阶段 | 前置 | 独立退出条件 |
|---|---|---|
| M8a | M5f | 显式帧暂停/恢复、go 参数求值、生命周期和无缓冲 handoff fixture 全绿 |
| M8b | M8a | nil/满/空、缓冲 FIFO、close/广播/发送 panic/range fixture 全绿 |
| M8c | M8b | select ready/default/阻塞、重复 channel、等待取消 fixture 全绿；多 seed 测试有明确预算 |
| M8d | M8c | 异常、rangefunc、map 修改/迭代各有独立必需 ID 集；每项结果均可分类 |
| M8e | M8a–M8d | 全部冻结解释语料全绿、同 seed 可回放、无已知多 seed 失败；发布冻结报告 |

### M9：native 对象模型、接口与 metadata（RT-CMP/POLY/MEM 子集）

- **M9a（依赖 M7）**：typed allocation、bump/arena、零值、统一分配入口、布局/对齐/指针 metadata、资源统计。
- **M9b**：string/slice/map、闭包/方法值、iface/eface、接口调用/断言/type switch；基础 itab/动态类型标识在这里完成，不能全部拖到 M12。
- **M9c**：native defer/panic/recover 和运行时检查；全局 roots、调用/回边安全点、stack maps、寄存器 spill/save roots、宿主桥接暂停策略。
- **M9d（依赖 M8）**：channel 对象/等待队列和调度契约；此处设计与独立测试，native go/select 执行门禁放 M11。

**保守正确性策略**：PIPE-13 逃逸分析先不优化，所有可能逃逸对象（包括闭包捕获、接口装箱、返回的局部变量地址）放 managed heap；不把悬空栈指针留给 GC。derived/unsafe/interior pointers 必须在 metadata 方案中明确支持或拒绝。

**门禁**：已支持顺序 native 语料零回归；逐类型布局、typed nil/接口 hash、别名与拷贝语义全绿；根 metadata 在强制 safepoint fixture 上可检查；unbounded arena 程序能触发明确资源错误。不把“仍无 GC”当作 RT-MEM-01/02 多级分配器完成。

| 子阶段 | 前置 | 独立退出条件 |
|---|---|---|
| M9a | M7d | typed allocation、清零/布局/资源计数 fixture 全绿；预算耗尽明确失败 |
| M9b | M9a、M5c/d | string/slice/map、闭包/接口/断言逐行为 native/解释差分全绿 |
| M9c1：异常与检查 | M9b、M5e | native defer/panic/recover、除零/越界/nil fixture 全绿，明确跨桥接传播规则 |
| M9c2：根与安全点协议 | M9c1、M6e、M7c | 全局/栈/寄存器/临时值/宿主持有引用 fixture 全绿；M10 可检查全部已支持类型的根 |
| M9d：channel/调度契约 | M9a、M8a–M8c | channel 布局/等待引用/取消协议单测全绿；不声称 native go/select 已运行 |

M9c1/c2 分别验收，合起来对应 M9c；M10 只依赖已验收的 M9a/b/c1/c2。M9d 可独立推进，但必须在 M11 的 native 并发执行前完成。

### M10：精确 STW GC（RT-MEM-04/06/07/08 子集）

1. **M10a**：独立小堆 + 已登记 roots 的手工触发 mark/sweep，验证扫描算法；仅单测设施，不能据此启用整程序 GC。
2. **M10b**：接入全局/静态、栈、保存寄存器、闭包/接口等全部支持类型 roots；分配点/调用/回边 safepoint 和暂停协议。
3. **M10c**：整程序回收、统计、压力/长跑、多次 collection、碎片与零初始化回归。

栈图不能漏寄存器或仅扫描 heap；分配过程中的临时对象、runtime 持有引用和宿主桥接区同样需要 root/liveness 协议。Go GC 与 Rust 宿主分配器明确分工，不能用宿主 drop 代替 managed heap 回收。

**门禁**：可达对象不误收、不可达对象可收、栈/寄存器/全局 roots fixture 全绿；长跑 warm-up 后 live bytes/已分配页趋于有界，记录 RSS 而不要求 RSS 每次下降。完整 GOMEMLIMIT/GC pacing、不存在支撑 API 的官方 runtime 测试延期。

**交付**：单线程/明确暂停条件下可用 STW 基线。无写屏障前不允许并发标记；记录后续并发 GC 前置，不以“有演进 issue”冒充功能完成。

### M11：native 栈、调度、channel/select 与定时器

- **M11a**：首发平台有栈上下文切换、寄存器/SP/PC/栈对齐 fixture；先受限固定大小 goroutine 栈，溢出明确报错，不承诺复制增长。
- **M11b**：M:1 协作调度、go 参数求值/复制、channel/select 等待与取消、main 结束；将 M8 的语义用例转入 native 门禁。
- **M11c**：停所有任务的 STW、调度栈/root 管理；栈增长先选择可证明的方案。复制式栈需重定位 stack maps、内部栈指针、defer/closure/保存寄存器等；在明确拒绝暂不支持布局前不可上线。
- **M11d**：定时器和系统时间接口（fake clock + wall clock 分开）；不能用固定 seed 掩盖时间差异。
- **M11e（可选）**：多线程/work-stealing 或 GMP；Go 内存模型、channel happens-before、原子/锁、阻塞 syscall/线程退出、STW rendezvous 必须新增测试。
- **M11f（可选，单独资源门禁）**：write barrier → 并发标记/清扫；同步抢占先行，异步信号抢占再单独立项。

**GC 启用门禁**：M11a/b 的多任务运行强制禁用整程序 GC，退回有预算上限的 arena/不回收分配路径；不得沿用 M10 的单栈自动回收。只有 M11c 验证所有运行/挂起任务的栈、保存寄存器、调度器和 channel/select 等待队列持有引用均可停顿并扫描后，才重新启用 GC。先用多任务 root 扫描定向 fixture 证明完整性，再开启分配压力/调度/回收组合测试；关闭 GC 的测试结果不作为多任务 GC 通过证据。

**门禁**：每个子阶段明确模型、支持范围和压力预算；不把 M:1 通过当多线程正确，不以“看起来公平”替代多 seed/调度不变量验证。

### M12：泛型 native、有限反射与可选优化

**正确性线**：依赖 M4 的实例化/推断和 M9 类型/接口模型；先 native 全量单态化，覆盖泛型闭包、method value、interface/类型 switch 和必要的类型身份。早期泛型小切片可在 M9 后独立交付，无需等待可选多线程/并发 GC。

**有限反射线**：明确包/API 白名单；先 size/alignment、kind、字段偏移/Tag、方法表和类型身份，再定义最小调用接口。`reflect/all_test.go` 只抽选可适配行为，完整 reflect 不做整体门禁。

**优化线（各自独立）**：shape/dictionary 共享、内联（PIPE-12）、精细逃逸分析（PIPE-13）、SwissTable、SSA 高级优化；先正确性回归，再代码体积/编译时长/执行性能预算，不把优化设计绑定语言支持。

**门禁**：精选 interface/typeparam 自检查 run 与自有元数据 fixture 全绿；所有扩展有范围报告；无“246 项全部完成”的替代证据。

---

## 5. 功能归属与延期登记

| 功能组/ID | 静态 / 解释 | native / runtime | 首发缺口或策略 |
|---|---|---|---|
| LEX/AST、PIPE-02/03 | M1/M2 | 共用前端 | golden/位置不等于运行覆盖 |
| CON/TYP/SCP、PIPE-07/08/09 | M3/M4 | M7/M9/M12 消费 | 全部语言细节按 manifest 声明 |
| EXP/STM/BIF/PKG/SYS | M4 静态；M5/M8 执行 | M7/M9/M11 | 并发、rangefunc、unsafe 分层 |
| PIPE-01/04/05/25/26 | M0–M4、M6 dump | M7/M9 CLI/诊断 | runtime 指令含义和完整调试工具延期 |
| PIPE-06 | M3/M4 源码 importer/符号摘要 | 后续版本化 package cache | 官方 export data/模块下载延期 |
| PIPE-10/14/15/16/16b/16c | M5 HIR/closure/interface；M8 rangefunc | M9 对象/降级 | loopvar 不得仅语法打勾 |
| PIPE-11、RT-POLY-05 | M3/M4 静态，M5 子集解释 | M12 单态化/优化 | shape/dictionary 延期 |
| PIPE-12/13 | 内联延期；M5 保证捕获生命周期 | M9 保守堆分配，M12 优化 | 不强制复制官方算法/诊断 |
| PIPE-17/18 | M6 | M7 消费 | 优化数量非完成指标 |
| PIPE-19/20/21/22 | M6 metadata 协议 | M7 后端样例，M9 完整支持子集 | 第二架构、官方 ABI/DWARF 延期 |
| PIPE-23/27 | 不自研 | M0/M7 委托系统工具 | 委托不计“自研完成” |
| PIPE-24 | M4 顺序分析、M5 执行 | M7 startup、M9/M11 runtime | 每层单独 fixture |
| PIPE-28 | 延期 | 延期 | coverage/DWARF/PGO 非首发 |
| RT-CMP-01..07 | M5/M8 宿主行为 | M9/M11 | 不要求 SwissTable/扩容策略照搬 |
| RT-MEM-01/02/03/07 | 宿主管理不计实现 | M9 arena/typed alloc、M11 栈 | 分级分配器/复制栈优化延期 |
| RT-MEM-04..06/08 | 无 Go GC | M10 STW/metadata/统计；M11f 并发 | 完整内存限制/pacing 延期 |
| RT-SCH-01..08 | M8 M:1 语义 | M11 分层模型 | GMP/异步抢占独立可选 |
| RT-POLY-01..04/06/07 | M4 静态、M5 动态接口/方法值 | M9 typedesc/itab、M12 有限反射 | 完整 reflect 不承诺 |
| RT-EXC-01..06 | M5/M8 语义 | M9 异常、M11 任务退出 | traceback 格式/信号转换分开 |

[`02-test-inventory.md`](./02-test-inventory.md) 所列缺少直接测试的项，在 rgoc 自有测试中补齐：TYP-26、SCP-06、EXP-16/22、PKG-04/06；上下文切换、itab、泛型分派定向测试随所属阶段加入，不回写官方源码。

---

## 6. 资源预算、门禁与变更控制

### 6.1 估算是假设，不是完成日期

暂按**一个全职主维护者，有 AI 辅助但维护者审阅/验证不省略**估算。下表是投入上限触发点而非难度已证明的工期；不将并行 AI 数量换算成人周，也不自动推导整体交付日期。

| 阶段 | 首轮预算窗口（维护者人周） | 最长日历周期 | 超限时优先切断 / 不可放宽的门禁 |
|---|---|---|---|
| M0 | 2–3 | 6 周 | 缩减 runner 模式；保留匹配 oracle 和三个关键 spike |
| M1 | 2–4 | 6 周 | 砍 token dump 美化；保留词法/位置正确性及 native smoke |
| M2 | 2–4 | 6 周 | 砍广泛恢复策略/AST 展示；保留当前声明语法及负例诊断 |
| M3 | 6–12 | 12 周 | 砍模块/二进制包兼容、扩大语料；保留类型/常量与最小源码 loader |
| M4 | 6–12 | 12 周 | 砍 fixedbugs 扩展及未冻结语义；保留支持子集的接受/拒绝和初始化门禁 |
| M5 | 6–12（全部子阶段合计） | 12 周 | 砍 corpus 扩展、复杂泛型/rangefunc；保留已声明顺序闭环 |
| M6 | 4–8（全部子阶段合计） | 12 周 | 砍高级/非必要优化；保留 verifier、必要 lowering/求值与 metadata 协议 |
| M7 | 4–8（全部子阶段合计） | 12 周 | 砍完整 B 覆盖、额外指令优化；保留 native MVP 与 ABI/metadata fixture |
| M8 | 6–12（全部子阶段合计） | 16 周 | 延期新增泛型/rangefunc；保留已冻结 channel/select/异常语义，禁止缩减公平性测试伪造通过 |
| M9 | 6–12（全部子阶段合计） | 16 周 | 砍布局/容器算法优化；可延期 channel 契约但阻塞 native 并发，不砍 M10 所需 roots |
| M10 | 6–12（全部子阶段合计） | 16 周 | 砍 pacing/内存限制 API/算法优化；根扫描不完整则保持 GC 禁用，不能验收整程序 GC |
| M11a–M11d | 各 4–8 | 各 12 周 | 依次限制栈模型、调度模型、栈增长和时间 API；多任务 roots 未过不得启用 GC，已验收子阶段可单独发布 |
| M11e–M11f | 各 6–10（可选） | 各 12 周 | 延期多线程/work-stealing、并发 GC；异步抢占另立项，不阻塞首发 |
| M12 | 每条扩展线 4–8 | 每条 12 周 | 延期 shape/高级优化和扩大反射 API；保留冻结的单态化/有限反射正确性门禁 |

M0 先测每类 fixture 的适配/开发成本，M2 和首次 native MVP 后重新估算。窗口上限是停止扩项/重新规划的触发点，不是完成承诺；按阶段开始至退出的实际日历时间计，不因环境阻塞自动停表。达到人周或日历周期任一上限，或连续两周无门禁进展时，必须提交原因/已验收切片/保留范围/延期项和新的测试 ID 清单；不能单纯放宽通过率或无限追加功能。M5–M10 的子阶段预算必须从父阶段总额内分配，不可逐子阶段重复领取；M11/M12 的独立扩展不自动全部启动。

### 6.2 验证预算

初始 CI 目标：smoke ≤5 分钟，milestone ≤30 分钟，full 单次 ≤2 小时；长时 GC/并发 soak 独立手动/夜间 job。M0 按机器实测调整并记录 wall time、峰值 RSS 和 runner 配置；预算不是漏测理由。

每阶段及独立验收子阶段开工前产出 `docs/milestones/<阶段ID>-design.md`（设计文档，适用时）、`<阶段ID>-tests.md`（测试先行清单）以及机器可读 manifest，列必需 ID、退出结果、比较器、工具链、投入/日历期限、测试资源上限和暂不支持项；设计经确认后用 `<阶段ID>-plan.md` 承载 **2–5 分钟粒度**的实施任务（文件路径 / 可粘贴内容 / 验证命令）。进入下一阶段以必需 ID 100% 通过为准。子阶段也沿用 §6.1 的无进展/预算触发规则，并写明超限优先延期项与不可放宽门禁。这些是未来交付要求，本次不把不存在文件标为已完成。

### 6.3 统一退出与需求变更

1. 当前阶段必需 tests/smoke 全绿，无未分类、基建或不稳定失败。
2. Rust 工程建立后执行 `cargo fmt --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`；支持目标集和相关 native verifier/contract checks 也必须过。
3. 环境、报告、复现材料和 contract 版本齐全；保留 unsupported/未测试清单。
4. 新功能必须归属当前冻结测试集或解除已记录风险，否则进入 backlog。
5. 更改 ABI/布局/基线/必需用例需记录决策、迁移、回归影响，不能由并行子任务擅自扩大共享契约。
6. 不可行时先缩减交付范围；若目标仍无法证明，明确重规划，不把 recoverable failure 当完成。

---

## 7. 风险登记册

| 风险 | 影响 / 早期信号 | 责任阶段 | 应对与切断条件 |
|---|---|---|---|
| 工具链/SDK/版本漂移 | 极高；本机 oracle 与源码不匹配 | M0，所有阶段复检 | 快照 hash/二进制校验/SDK 锁定；不匹配拒绝作为基线 |
| harness 本身错误 | 高；假通过/遗漏目录与参数 | M0 | 正反自测、多模式样本、未知指令必报；模式按需适配 |
| 测试适配成本失控 | 高；大量内部单测无法移植 | 全周期 | 提取行为非内部实现；三层语料；报告适配耗时 |
| 分母/unsupported 漂移 | 高；高通过率但支持面变窄 | 全周期 | 冻结 ID/白名单，原新分母同时报，跳过不记 pass |
| 包加载/init 缺口 | 高；import/main/多文件返工 | M3–M5/M7 | 源码 loader、包身份、静态顺序/执行/startup 三层测试 |
| 解释器难以暂停恢复 | 高；channel 阻塞依赖 Rust 递归栈 | M5/M8 | continuation/显式帧契约先留位；复杂 stdlib 延期 |
| shared lowering 共同错误 | 高；两轨一致却都错 | M5–M12 | 官方 oracle 第三方对照 + 不同路径的自有断言 |
| native/object/ABI 范围爆炸 | 极高；迟迟无可执行文件 | M0/M7 | arm64 单平台、系统工具、固定最小 ABI；不用 Go internal ABI 作门槛 |
| Rust/native 桥接内存安全 | 极高；UB/unwind/栈损坏 | M0/M7/M9/M11 | 隔离 unsafe 接口，调用/对齐/寄存器 fixture、sanitizer/guard 可用时验证 |
| 根丢失/栈增长/GC 耦合 | 极高；临时/寄存器/捕获对象误收 | M6–M11 | liveness→stack map→完整 roots→STW；无完整 roots 不启用整程序 GC |
| 并发时序/内存模型 | 极高；固定 seed 过但多线程失败 | M8/M11 | seed 回放与多 seed、fake clock、happens-before/原子/压力；多线程单独门禁 |
| 内部算法仿制拖慢交付 | 高；SwissTable/SSA pass/字典先于正确性 | M5–M12 | HashMap 语义外层、必要 pass、全量单态化；优化可砍 |
| arena 无回收/资源耗尽 | 高；M9 长跑无界内存 | M9 | typed allocation 统计、预算明确失败；STW 独立阶段，不掩盖缺 GC |
| 单态化膨胀/编译预算 | 高；代码体积/实例化失控 | M5/M12 | 去重、递归/循环检测、明确限额与诊断；shape 优化不许换掉正确性门禁 |
| 许可/语料再分发 | 中高；CI 取不到快照/丢 license | M0 | 只读依赖、固定获取说明，保留 LICENSE/PATENTS；迁移片段保留合规来源 |
| 人力/CI 容量不足 | 极高；阶段连续超预算 | 全周期 | 人周/实测滚动估算；砍扩展，不降低证据要求 |

---

## 8. 下一步行动

1. 仅进入 **M0 的详细计划**：容器镜像 digest / 工具链锁定、源码锁定与语料校验步骤、harness 自验、三个 spike。
2. 为 M0 写精确路径/任务依赖/RED-GREEN 验证命令和预算；不提前拆完 M1–M12 的细任务。
3. 先查明 Rust 工具链和匹配 Go oracle 获取路径，再编写依赖它们的正式实现。
4. M0 达到门禁后推进 M1；未达标先修环境/设计，不能把 spike 成功的示例当成语言实现进度。
5. 每个阶段交付统一报告：四维完成度、固定分母通过率、失败/unsupported、性能/资源、修改契约、剩余风险、未测试项、下一阶段前置条件。

**原则**：早期可行性验证、单平台首发、独立可退出的能力切片优先；完整 Go toolchain/runtime 是后续扩展，不是首发承诺。

---

## 9. 首轮审阅建议落实对照

本表追踪的是**计划文本**的修订。**执行状态更新（2026-10-07）**：M0 已全部完成
（E1–E10 十条门禁全过），故下表第 1/3/5/6 行的「M0 执行 / M0 验证 / M0 建立」
**均已发生**；但**编译器实现（M1–M12）仍未开始** —— M0 证明的是三条路线可行，
不是语言实现进度（见 §6.3 第 4 条与本表第 2 行的「待执行」）。

| 原建议 | 本版落实位置 | 文档状态 / 后续执行 |
|---|---|---|
| 1. 固定 Go 版本、宿主和首发架构 | §0.2：Go 1.27.1、Linux/arm64（Docker）、ELF | ✅ 已确定并**已在 M0 执行**（E1/E2：镜像按 digest 钉死、工具链钉版、语料 hash 清单齐备） |
| 2. 完成度拆为四类 | §0.3：Frontend/Interpreter/Native/Runtime | 已定义各维度分母/状态/覆盖指标；逐 ID 清单和实际报告待执行 |
| 3. M0 增加三个架构 spike | M0：解释、SSA、native 验证 | ✅ **已实施**（T42/T43/T44；E6 可复现 + E7 native `hello` 于 2026-10-07 通过）。**注**：三个 spike 跑的是**同一个固定 fixture**（`println(1+2)` / `hello`），**不是语言实现** |
| 4. 拆分原 M5/M6/M8/M9 | §1.2 编号对照；M5–M11 子阶段 | 已拆分；M5/M6/M7/M8/M9 独立门禁见各表 |
| 5. 明确 runtime/ABI/linker/stdlib/cgo/reflect/unsafe 边界 | §0.2、§2、§5 功能延期 | 已修订包/API 边界；✅ 桥接方案**已在 M0 验证**（C5 契约：裸 `_start` + `svc #0`，**零 libc 符号**；两个必需链接开关 `-nostartfiles` / `-Wl,-s`） |
| 6. smoke/milestone/full 三层测试 | §3.1/3.2 | 已定义；✅ 语料 manifest **已在 M0 建立**（分母 **279** 冻结于 T29；native `hello` 已登记为 M1 smoke 项 `SM-M0-NATIVE-HELLO`） |
| 7. deterministic、失败分类、差分缩减 | §3.3/3.4、M8 | 已定义模式适用范围、复现/缩减判据及预算；实现待执行，自动 reducer 延后 |
| 8. 修改机械任务/提交粒度 | §3.5 | 已改为可验证行为切片与逻辑提交 |
| 9. 资源预算、周期与砍项 | §6.1 逐阶段预算/最长日历周期、§6.2 子阶段约束、§7 | 已列资源/周期双触发与砍项；实测后滚动校准，不自动延长 |
| 10. 提前 native hello 验证 | M0 native spike、M1 smoke；M7 正式 MVP | 已前移至 M0并在 M1 回归；固定输入验证与完整编译分开验收 |
