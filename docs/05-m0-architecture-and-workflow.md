# M0 架构设计与 crates 时序工作流

> **文档索引**：[`README.md`](./README.md)　|　**上游**：[`03-roadmap.md`](./03-roadmap.md)、[`04-development-environment.md`](./04-development-environment.md)、[`milestones/M0-design.md`](./milestones/M0-design.md)　|　**下游**：[`milestones/M0-plan.md`](./milestones/M0-plan.md)

---

## 1. M0 的设计理念：为什么不能直接写编译器？

重写 Go 编译器（目标：纯 Rust 实现，输出 Linux/arm64 ELF 机器码）是一项横跨前端语法分析、类型系统、中间表示（IR）、SSA 优化、寄存器分配、机器码生成和运行时支持（GC / Goroutine）的系统级工程（规划共 M0–M12 共 13 个里程碑）。

如果一上来就直接从 Lexer / Parser 写起，必然陷入以下致命陷阱：
1. **测试与判卷无基准**：Go 官方测试语料有 15,000+ 文件，带有独特的执行指令（`// run`, `// errorcheck`）和复杂的判定规则。如果没有一套经官方 Oracle（Go 1.27.1）验证过 100% 正确的“自动化判卷机”，后续编译器报错时，根本分不清是**代码写错了**、**测试判卷机判错了**、还是**运行环境不兼容**。
2. **环境与调试深坑**：macOS 宿主到 Linux/arm64 容器的跨环境交叉调试极其脆弱，若不在一开始把环境镜像、Docker 命名卷性能、VSCode 单步断点调试钉死，后续排查复杂 SSA / 代码生成 bug 时会付出惨重代价。
3. **架构盲目冒险**：SSA 怎么设计？解释器如何表示值？Native aarch64 怎么调 Clang 链接出合法 ELF？如果不先做最小概念探针（Spikes）探出边界，直接大规模铺开写代码，一旦架构假设错误就会全盘推倒重来。

因此，**M0（Milestone 0）的核心使命是：打好一切工程地基，不写正式编译器业务代码**。

---

## 2. M0 各阶段（Phase 0 ~ Phase 4）的作用与演进图谱

M0 严格按照 **“底座环境 → 调试链路 → 测试基准 → 架构探针 → 跨阶段契约”** 的逻辑层层推进：

```text
[Phase 0] 容器化底座 ──► [Phase 1] IDE 单步调试 ──► [Phase 2] Harness 判卷机 ──► [Phase 3] 架构 Spikes ──► [Phase 4] 沉淀契约与交接
 (可复现/高性能环境)        (按 F5 断点命中)           (官方 20 样本全绿基线)       (解释/SSA/Native探针)      (固化 C1~C5 规格初稿)
```

| 阶段 | 核心任务 | 解决什么问题 / 角色 | 产出与门禁 |
|---|---|---|---|
| **Phase 0**<br>容器底座与环境基准 | T01–T14 | 解决**“在我的机器上能跑”**与构建性能问题。<br>固化环境镜像，解决命名卷缓存与工具链版本。 | 镜像 `rgoc:dev`（Go 1.27.1 + Rust 1.98.1 + Clang 14）；命名卷 `rgoc-target` 提速 2.3x；门禁 **E1、E2、E10**。 |
| **Phase 1**<br>VSCode 调试链路 | T15–T28 | 确保后续开发复杂编译器算法时，**能用 IDE 设断点、单步跟踪和查看内存变量**。<br>攻克代理下发、Server 缓存、Cargo 启动配置等陷阱。 | CodeLLDB 调试通道打通；离线安装脚本；门禁 **E5**（F5 断点实测命中 `double_sum`，变量 `sum == 3`）。 |
| **Phase 2**<br>测试基础设施 Harness | T29–T39 | 打造工程全生命周期的**“判卷机”**，并用官方 Oracle 跑通基线，证明判卷机 100% 靠谱。<br>落地 Test IR、16 指令解析、R2/R3/R4 比对、CLI。 | `rgoc-harness`、`rgoc-driver`、`xtask`；门禁 **E3**（六类自测全绿）、**E4**（官方 20 样本 20/20 5.8s 全过，分母 279 冻结）。 |
| **Phase 3**<br>三个架构 Spike | T40–T47 | **低成本探路**。在**不写 Lexer/Parser** 前提下，用写死的固定 HIR 提前穿刺后续三种核心编译器形态，摸清架构风险。 | `rgoc-spikes`（S1 解释器、S2 SSA 图、S3 Native ELF）；门禁 **E6**（重跑一致）、**E7**（native `hello` 登记为 M1 冒烟项）。 |
| **Phase 4**<br>跨阶段契约与交付报告 | T48–T55 | 把探针与测试框架的经验固化为**不可篡改的版本化契约（Specs）**，交付交底报告，收口 M0。 | C1~C5 契约初稿；`M0-report.md`；全量 Manifest；门禁 **E8、E9、E10 收口**。 |

---

## 3. `rgoc/crates` 目录下代码的 UML 时序图

当前 [`rgoc/crates/`](file:///Users/wangfeng/workspace/rust_go_compiler/rgoc/crates/) 下包含两个核心 crate：
* **`rgoc-driver`**：统一 CLI 入口（手写参数解析，负责样本加载与报告渲染，**零判定逻辑**）。
* **`rgoc-harness`**：核心判卷引擎（指令解析、平台过滤、Oracle 调用、比对器、执行器）。

以下时序图完整展示了在终端执行 `cargo run -p rgoc-driver -- harness run --all` 运行官方语料基线时的完整调用时序与协作机制：

```mermaid
sequenceDiagram
    autonumber
    actor User as 用户 / CLI
    participant Driver as rgoc_driver::cli
    participant Samples as rgoc_driver::samples
    participant Runner as rgoc_harness::runner
    participant Instruction as rgoc_harness::instruction
    participant Corpus as rgoc_harness::corpus
    participant Oracle as rgoc_harness::oracle
    participant Compare as rgoc_harness::compare
    participant Sys as 系统进程 / Go Oracle

    User->>Driver: harness run --all
    Driver->>Samples: 加载 20 个样本源码与 .out 期望
    Samples-->>Driver: 返回 Vec<CaseSpec>

    Driver->>Runner: run_layer(&specs, &oracle_cfg, &corpus_cfg)
    
    %% Oracle 版本守门
    activate Runner
    Runner->>Oracle: Oracle::new(oracle_cfg)
    Oracle->>Sys: 执行 go version
    Sys-->>Oracle: "go version go1.27.1 linux/arm64"
    Note over Oracle: 守门: 版本不符则整层拒绝 (T-H-06)
    Oracle-->>Runner: Ok(Oracle 实例)

    %% 逐用例执行循环
    loop 遍历每个 CaseSpec
        Runner->>Runner: run_case(spec, corpus_cfg, oracle)
        activate Runner

        %% 1. 解析指令
        Runner->>Instruction: parse_action(spec.src)
        Instruction-->>Runner: Ok(ins) 提取 action (如 "run", "errorcheck")

        %% 2. 平台过滤 (R1b 顺序契约)
        Runner->>Corpus: should_test(header, corpus_cfg)
        Note over Runner,Corpus: R1b 核心契约: 平台过滤必须先于指令分派<br/>避免 linkmain.go 等文件误报未知指令
        alt 构建约束不满足 (如 Windows 专有用例)
            Corpus-->>Runner: false
            Runner-->>Runner: Verdict = TargetFiltered (耗时置0，计入分母)
        else 构建约束满足
            Corpus-->>Runner: true
            
            %% 3. 指令分派
            Runner->>Instruction: dispatch(&ins, platform_ok=true)
            Instruction-->>Runner: Ok(Dispatch::Proceed(Mode))

            %% 4. 执行 Oracle
            Runner->>Oracle: run_mode_path(mode, spec.path)
            activate Oracle
            alt Mode::Run (走 Fast Path: 编译+链接+直跑)
                Oracle->>Sys: go tool compile -> go tool link -> 执行二进制
                Sys-->>Oracle: 合并输出 (stdout + stderr) & RSS & exit_code
            else Mode::ErrorCheck (错误检查模式)
                Oracle->>Sys: go tool compile -d=ssa/check/on
                Sys-->>Oracle: 捕获编译器诊断输出
            end
            Oracle-->>Runner: OracleOutput (包含合并输出、PID、峰值 RSS)
            deactivate Oracle

            %% 5. 结果判决与比对
            Runner->>Runner: judge(mode, spec, out, oracle)
            activate Runner
            alt 超过超时或 RSS 上限
                Runner-->>Runner: Verdict = Timeout / ResourceFailure
            else Mode::Run
                Runner->>Compare: check_expected_output(merged, expected_out)
                Compare-->>Runner: Ok(()) 或 Err(不匹配原因)
                Note over Runner: 成功 -> Pass; 失败 -> RuntimeFailure
            else Mode::ErrorCheck
                Runner->>Compare: wanted_errors(spec.src)
                Compare-->>Runner: 提取期望的 // ERROR 注释
                Runner->>Compare: replace_prefix(merged, 短文件名)
                Runner->>Compare: error_check(diag, want)
                alt 正则语法超出当前子集
                    Compare-->>Runner: Err("UNSUPPORTED-REGEX: ...")
                    Note over Runner: 判 HarnessFailure (能力不足，非编译器 bug)
                else 诊断完全匹配
                    Compare-->>Runner: Ok(()) -> Verdict = Pass
                else 诊断缺失或多余
                    Compare-->>Runner: Err(...) -> Verdict = CompilerFailure
                end
            end
            deactivate Runner
        end
        deactivate Runner
        Runner-->>Runner: 记录 CaseOutcome
    end

    Runner-->>Driver: 返回 LayerReport (包含逐用例判定、分母/分子)
    deactivate Runner

    Driver->>Driver: summarize(&rep) 聚合统计指标
    Driver->>Driver: render_text(&summary) 渲染终端表格
    Driver-->>User: 打印表格 (20/20 全部 Pass，耗时 5.8s) & 退出码 0
```

---

## 4. 关键架构设计与工程红线

1. **R1b 顺序契约（步骤 5–8）**：
   * 真实 Go 语料（如 `linkmain.go`）首行带版权声明，如果不做平台过滤直接解析指令，会报“未知指令硬失败”；官方真实逻辑是**先做平台过滤后 `t.Skip`**。`dispatch` 函数把此顺序编码在接口签名中，从根本上防止了时序颠倒。
2. **Fast Path 避免 `unsafe`（步骤 12）**：
   * `Mode::Run` 没有直接调用 `go run`，而是走 `compile -> link -> 直跑二进制`。直接运行可执行文件使父子进程关系单一，超时只需 `child.kill()`，无需依赖系统级进程组强杀（需要 `unsafe`），完美满足项目全局 `#![forbid(unsafe_code)]` 的硬约束。
3. **两层失败的严格区分（步骤 17–19）**：
   * 当遇到复杂正则表达式时，比较器返回 `UNSUPPORTED-REGEX`，会被明确判定为 **`HarnessFailure`**（测试框架能力不足），而绝对不会误判为 **`CompilerFailure`**（被测程序有 bug），杜绝测试框架掩盖真实缺陷或误报。
4. **统一分母纪律**：
   * 被平台过滤跳过的用例（`TargetFiltered`）依然保留在分母中，只有真跑通过的才计入分子，坚决杜绝“为了好看偷偷缩小分母”的伪通过行为。
5. **Spike 隔离与随时可删原则（决策 D-M0-14）**：
   * Phase 3 的三个 Spike 集中在独立 crate `rgoc-spikes`，输入全部写死。未来里程碑（M5/M6/M7）由测试驱动写出正式实现时，该 crate 可直接整体删除或归档，杜绝临时代码侵入正式编译器架构。
