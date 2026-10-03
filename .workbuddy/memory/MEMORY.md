# 项目长期记忆 —— rust_go_compiler

## 项目概况

- 目标：用 **Rust 重写 Go 编译器**，项目代号 **`rgoc`**
- 语料快照：`go_source_code/`（`VERSION` = `go1.27.1`，2026-08-28）；规格基准 `doc/go_spec.html`
- 首发平台：**Linux / arm64（Docker 容器提供）→ `aarch64-unknown-linux-gnu` / ELF**；不维护 macOS 第二套首发环境
- **Git 仓库已发布**：<https://github.com/nobody0726/rust-go-compiler>（public，分支 `main`，remote = `origin`）
- **语料不入库**：`go_source_code/`（185 MB / 15,618 文件）由根目录 `corpus-manifest.sha256` 锁定；校验用 `shasum -a 256 -c corpus-manifest.sha256`
- **`git push` 常被拦**：`github.com` 的 CONNECT 间歇性 502，而 `api.github.com` 正常 → 改用技能 `github-push-via-api` 走 Git Data API（脚本含 `--root` 模式处理空仓引导提交场景）
- 语料快照一致性以 SHA-256 清单为准，`VERSION` 文件本身不足以证明内容一致

## 当前状态：M0 Phase 0 与 Phase 1 均已完成（E1/E2/E10/E5 全过）；Phase 2–4 计划已拆完（T29–T55），待开工

- **入口**：`docs/milestones/` 下的 M0 四件套 + manifest —— `M0-design.md`（**已确认**，D-M0-1~12）、`M0-tests.md`（待冻结）、`M0-plan.md`（**Phase 0 已完成**）、`M0-benchmarks.md`（E10 证据）、`M0-manifest.json`（environment/gate/benchmarks 已填实）
- **Phase 0 ✅**：T01–T19 全部完成，门禁 **E1 / E2 / E10 通过**（证据在 `M0-manifest.json` 的 `gate`）
- **Phase 1 ✅**：T20–T28 全部完成。T20–T22 四条统一退出检查全过（调试目标改形状后已**无需任何 `#[allow]`**）；T23–T26 的 devcontainer / launch.json 就绪；**T27/T28 已由用户于 2026-10-02 人工实测通过 —— E5 是 M0 最后一项门禁**，登记在 `M0-manifest.json` 的 `gate.E5`（`status=pass` / `confirmed_at` / `confirmed_by` / `evidence`）。**三项环境前提均已实测**：VS Code Server 1.140.0 在持久卷 `/vscode`（两处 `test -d` 均 exit=0）、CodeLLDB 平台包 `platform.ok` 存在（lldb 22.1.8-codelldb）、`launch.json` 的 `cargo.cwd` 与 `filter.name` 两处静默陷阱已修。
- **环境已就绪**：镜像 `rgoc:dev` = `sha256:21f55802b6275509bf3c91d8b8f55fdc890048287d671dc94afd7ce500ba553b`（2.92GB，14 层）
- **计划已拆完（2026-10-02）**：`M0-plan.md` 覆盖 **Phase 0–4、T01–T55**（Phase 2 §4 / Phase 3 §5 / Phase 4 §6；原 §4/§5/§6 顺延为 §7/§8/§9）。拆解时定了 **D-M0-13**（crate 严格按当期需要：Phase 2 只建 harness/driver/xtask，Phase 3 才建 rgoc-hir + rgoc-spikes）、**D-M0-14**（三个 spike 放独立 crate `rgoc-spikes`，可整块删除）、**D-M0-15**（`double_sum` **不删** —— 它是 E5 的人工复验锚点，改为 T-H-01 的 fixture）
- **T29 ✅（2026-10-02）**：`M0-tests.md` **已冻结**（F1 20 样本 / F2 U1–U14 / F3 超时上限 / **F4 M0 分母 = 279**）。
  冻结时查出三处问题：① 分母实算 279（run 147 / errorcheck 120 / compile 12，含 5 个平台过滤项；排除 77 已逐条归 U）；② unsupported 清单**漏了 U13**（`skip` 指令 5 个，上游 `t.Skip`）与 **U14**（`linkmain.go`）；③ **分派顺序此前记漏** → §1.3 新增 **R1b**：官方是「R1 解析 action(:502-515) → **平台过滤(:522)** → switch(:541)」，**顺序颠倒会让 harness 在 `linkmain.go` 上误报 T-H-03**。T33 的 Rust 枚举器算出的分母**必须 == 279**。
- **T30 ✅（2026-10-02）**：容器内 `go1.27.1` 复核 20 个样本，**19/20 一致**；唯一差异是**文档错**（T-C-20 `mainsig.go` 的 ERROR 期望实为 **5 条**：L9/L10×2/L12/L13，文档漏记第 9 行）→ **改文档不改样本**。复核中补了两条 harness 命门规则：**R2b** 官方 `runcmd`（`:642-647`）把 **stdout+stderr 合并**再比 `.out`；**R6** 三层命令形态是 **`go tool compile`**（`:188`/`:787-790`）而**不是 `go build`**，诊断路径要先 `replacePrefix` 成短名（`:2059-2072`，**含续行**）。原始记录见 `M0-benchmarks.md` §11
- **T31 ✅（2026-10-02）**：Test IR 骨架落地 —— `rgoc/crates/rgoc-harness/src/ir.rs`（**16 个必录字段**、`Verdict` **八种不得合并**、`Limits::for_layer` 是冻结预算的**唯一入口**）+ `tests/test_ir.rs`（**11 条验收测试**，同时是 **C2 契约的可执行副本**）。`double_sum` 按 D-M0-15 保留（E5 复验锚点 + T-H-01 进程内正例）。**Rust 侧的坑**：中文标识符可用但**不能夹大写 ASCII**（`512MiB` 触发 `non_snake_case`，在 `clippy -D warnings` 下直接变错误打爆门禁）；正则批量改代码后必须**立刻编译**
- **T32 ✅（2026-10-02）**：`src/instruction.rs` + `tests/test_instruction.rs`（**14 条**）。**R1b 的顺序契约写进了函数签名**：`dispatch(ins, platform_ok)` —— `platform_ok` 必填，官方平台过滤（:522）先于 switch（:541），类型层面让颠倒不可能发生。`Dispatch` 三个出口：`Proceed(Mode)` / `TargetFiltered` / `SkippedByDesign`（`// skip`）。构建约束判定照 `go/build/constraint/expr.go`（**HasPrefix 判未经 trim 的整行**，缩进的 `//go:build` 不算约束）
- **T33 ✅（2026-10-02）**：`src/corpus.rs` + `tests/test_corpus.rs`（**15 条**）。**★ 交叉校验通过：枚举真实语料算出的分母 == 279**（T29 冻结值）—— total 356 / executed 274 / target_filtered 5 / excluded 77，U7 31 · U2 14 · U6 11 · U5 9 · U1 5 · U13 5 · U3 1 · U14 1。`shouldTest` 逐条对齐 `:380-467`：**ToolTags 只对 `goexperiment.*` 前缀查**（否则 `arm64.v8.0` 错判为真）；tag 集合是 **go1.27.1 实测值**（ReleaseTags go1.1…go1.27；`goexperiment.simd` 不在默认集合 → 这正是 `simd_inline.go` 被过滤的原因）。**U7 只收 v0 集内**的用例；**平台过滤与 U 归类正交**（分母口径与过滤无关）
- **T34 ✅（2026-10-03）**：`src/oracle.rs` + `tests/test_oracle.rs`（**12 条**，真调容器内 go1.27.1）。**`run` 层走官方 fast path**（compile → link → 直跑 exe，`:1069-1077`）：父子关系直，超时 `child.kill()` 即杀掉被测程序，**不需要 unsafe 的 `kill(-pgid)`** —— 而 workspace 是 `unsafe_code = "forbid"`，**约束把设计推向了更好的选择**。
  ⚠️ 三个踩过的坑：`go version` 的平台是**斜杠连写的一个 token**（`linux/arm64`）；`if start >= deadline` **恒为 false**（要用 `Instant::now() >= deadline`）；oracle 的临时文件必须**实例级唯一**（固定路径在并行下互相覆盖，症状是「单跑必过、并行挂」）
- **T35 ✅（2026-10-03）**：`src/compare.rs` + `tests/test_compare.rs`（**26 条**）。R2/R3/R4 + `errorCheck` 匹配算法逐条照搬官方。**正则只实现子集**（字面量 / `|` / `.` / `.*` / 转义），**不认识就报错**（`UnsupportedRegex`）—— 语料 5435 条 ERROR 模式用到全套正则，静默不匹配会把它们误判成「编译器有 bug」。**E4 的 20 个样本全在子集内**（T30 实测）；但 279 全量里多数 errorcheck 会命中 `UNSUPPORTED-REGEX` ⇒ 判 `harness-failure`，**「全量逐条判语义」需先决定扩展子集还是引入 regex crate**（T55 前的决策点）
- **下一步（Phase 2）**：① **T36 六类自测（E3）** —— 要能区分「harness 能力不足（`UNSUPPORTED-REGEX:` 前缀）」与「真的不匹配」（`03` §3.3 要求「白名单开工前冻结」，这是硬前置）→ ② T30 oracle 侧复核 20 样本期望值 → ③ T31–T35 harness 核心 → ④ T36 六类自测（E3）→ ⑤ T38 跑 20 样本（E4）
- **五个 Phase**：0 容器与工具链底座 → 1 VSCode 调试环境（**门禁 = 实测断点命中**，不是「能开窗口」）→ 2 Rust 骨架 + harness → 3 三个 spike（解释/SSA/native）→ 4 契约 + 报告
- **关键洞察**：Go oracle 是**硬约束**（必须精确 `go1.27.1`；宿主 `go1.24.5` 不可作基线），Rust 版本是**软约束** → 用 `golang:1.27.1-bookworm` 基础镜像满足 Go，用 `rustup` 满足 Rust
- **环境实测（2026-10-02 只读侦察，宿主 VSCode 于同日升到 1.140.0）**：Docker 29.6.2／内核 `6.12.76-linuxkit`／`aarch64`／10 CPU、7.75 GiB；镜像 index digest `sha256:69a7b978…9195`，arm64 digest `sha256:1668bbf8…fae1`；**宿主无 rustc/cargo/rustup**；宿主 VSCode **1.140.0 / commit `07f806f9…`**（侦察时为 1.139.1 / `04c0d99f…`），扩展 `rust-lang.rust-analyzer` 与 `vadimcn.vscode-lldb` 由 devcontainer 在**容器内**装；Rust stable 参考值 `1.98.1`

## Phase 0/1 实测得出的硬事实（踩过的坑，别再踩）

- **所有构建与测试必须在容器内**：`scripts/in-container.sh <命令>`（先探 daemon，失败即报错**不降级**）
- **镜像不可位级复现**：同输入两次 `docker build --no-cache` 产出的 image id 与 14 层中的 **6 层** digest 均不同。→ **image id 不得写进门禁**；真正的钉子是 `base.index_digest` + `src.*_sha256`（连「命中全部缓存的重建」也会让 tag 指向新 id）
- **`rgoc/target/` 在命名卷 `rgoc-target`**（D-M0-9，T18 实测 **2.3×**：0.137s vs 0.336s）→ 两个代价：① 新建空卷属主是 `root:root`，必须先 `chown 501:20`（`in-container.sh` 有 bootstrap；devcontainer 走 `postCreateCommand`，这也是镜像里装 `sudo` 的**唯一**原因）；② **`cargo clean` 会 `EBUSY(16)` / exit 101** → 清空改用 `find rgoc/target -mindepth 1 -delete`
- **登录 shell 的 PATH 会被 `/etc/profile` 无条件重置** → `bash -lc 'cargo --version'` 报 command not found（VSCode 集成终端默认就是登录 shell）。镜像层 5 写 `/etc/profile.d/50-rgoc-toolchains.sh` 解决，层 6 是它的回归断言
- **验证命令里别用 `cmd | tail -N` 串 `&&`**：管道退出码取自 `tail`，失败会被静默吞掉。脚本首行加 `set -euo pipefail`
- **计时器**：宿主 macOS 的 `/usr/bin/time` 不支持 GNU `-f`（报错且**命令整条不执行**）；容器内**没有** `/usr/bin/time`（Debian 的 `time` 包未装）。宿主用 `-p`，容器用 bash 内建 `time` + `TIMEFORMAT`
- **`cargo new <dir>` 的目录名不能叫 `crate`**（Rust 关键字，直接拒绝）
- **门禁四条命令**（`03` §6.3）：`cargo fmt --check` / `cargo check --workspace --all-targets` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace`
- **宿主 VSCode 的 `http.proxy` 会被下推进容器**（经 AHP `root/configChanged`，证据在 `~/.vscode-server/data/logs/*/ahp/ahp-*.jsonl`）→ 容器内 `127.0.0.1` 指向自己，一切走 VSCode 网络栈的下载全废。**远端 Machine settings 里写 `http.proxy: ""` 覆盖不了它**（实测：写完后新会话仍报同一错）。修法是离线绕过：`scripts/install-codelldb.sh`（`curl --noproxy '*'` + `code-server --install-extension`）
- ⚠️ **同一句 `Failed to establish a socket connection to proxies: PROXY 127.0.0.1:7897` 可能来自两个不同层级，修法不同** —— ① **扩展平台包**（在容器内、走 GitHub releases，判据 `<扩展目录>/platform.ok`）→ `install-codelldb.sh`；② **VS Code Server 本体**（在**宿主**侧、连容器**之前**、走 `update.code.visualstudio.com`）→ `install-vscode-server.sh`。**唯一可靠判据是日志里的 `Path:`** —— 指向宿主 `/var/folders/…` 就是 ②。别看到眼熟的报错就按上次的办法修（证据在 `M0-benchmarks.md` §7 / §9）
- **VS Code Server 装在持久命名卷 `/vscode`**：`~/.vscode-server/bin/<commit>` 只是符号链接，实体在 `/vscode/vscode-server/bin/linux-<arch>/<commit>`；Dev Containers **两处都缺才下载**。→ **宿主每次升级 VSCode（commit 变）都会复发**，跑 `install-vscode-server.sh --commit <宿主 commit>` 装进卷内即永久命中（601 MB，实测 204 MB / 9.2–10.5 MB/s）。**装完必须 Reopen in Container**。该脚本断言 `product.json` 的 commit（**语义**）而非 tarball 字节哈希 —— 上游会重打包
- **容器重建后 CodeLLDB 平台包必丢**（`~/.vscode-server/extensions/` **不在任何卷里**）→ 重跑 `install-codelldb.sh`。这与上一条是**两个独立复发点**
- **CodeLLDB 是两段式安装**：扩展**本体**（marketplace）≠ **平台包**（GitHub releases，含真正的 lldb）。判据是 `<扩展目录>/platform.ok`。调试器用的是平台包**自带**的 `lldb 22.1.8-codelldb`，**不是**镜像里的 `lldb 14.0.6`
- **rustc 不为「尾位置直接返回的 `let` 绑定」生成 DWARF 变量条目** → 该中间值对**任何**调试器都不可见（`let sum = a + b; sum` 看不到 `sum`；`sum * 2` 才看得到）。写调试目标时必须让中间值被第二次读取。证据在 `M0-benchmarks.md` §8
- ⚠️ **CodeLLDB 的 cargo 启动配置有两个静默陷阱**（`launch.json` 里写错不报错，只在按 F5 时以 `Cargo command did not complete successfully.` 收场）：① 工作目录取自 **`cargo.cwd`**（源码 `getCargoCwd(e){return e ?? this.workspaceFolder?.uri?.fsPath}`，调用处传 `e.cargo.cwd`），**不读顶层 `cwd`** —— 漏写就回退到 `/work`（无 `Cargo.toml`）→ cargo 退出 **101**；② `filter.name` 比对的是 **cargo 的 target name（下划线）**，不是包名（连字符）—— 本包包名 `rgoc-harness`、target name `rgoc_harness`，用包名会 0 匹配。**真错在 OUTPUT → LLDB 通道的 `Cargo exited with code N`，不在 VSCode 弹出的提示里**。`${workspaceFolder}` 可用在 `cargo.cwd`（解析发生在变量替换之后的 hook 内）。证据与三层回归见 `M0-benchmarks.md` §10
- ⚠️ **别用 shell 复现 CodeLLDB 的 cargo 命令**：它是无 `shell: true` 的 `spawn`，shell 会剥掉 `target.'cfg(all())'` 上的单引号 → 得到一个**假的** TOML 报错（`invalid unquoted key`）。必须用 argv 列表（Python `subprocess.run(argv, …)`）复现 —— 本仓曾据此误判过根因
- ⚠️ **`AGENTS.md` 里记录的断言数由自检脚本自己对账**（`check-m0-consistency.py` §6）：用 `TOTAL` 计数器，最后一条断言把自己也数进去，所以它**必须是文件的最后一条**。改断言后只需同步 `AGENTS.md` §1 一处，忘了同步自检会直接报 ✗（本轮已这样抓到过两次）
- **非 root 用户下 `/proc/self/status` 的 `CapEff` 恒为 0**，不能用它判断容器能力 → 从宿主 `docker inspect … HostConfig.CapAdd/SecurityOpt`
- ⚠️ **断言必须锚定真实代码，注释里的同名串会把它变成恒真断言** —— 本仓踩过两次：① 推导断点行的 `grep -n 'let sum = a + b;'` 命中**注释行**；② `install-*.sh` 的**头部注释**里也写着 `--noproxy '*'` / `platform.ok` / `ln -sfn` / `[0-9a-f]{40}`，用原文做子串匹配会被注释满足。→ `check-m0-consistency.py` 第 5/5b 节一律作用于 `code_only()`（剔整行注释）
- 环境陷阱的完整清单在 `docs/milestones/M0-benchmarks.md` §6（**14 项**）；两个可判定自检入口：`scripts/check-m0-consistency.py`（**95 条**断言，含 §6 的「`AGENTS.md` 记录的断言数 == 实际数」防腐断言）、`scripts/debug-smoke-test.sh`（第 2 节按 `launch.json` 复刻 CodeLLDB 的 cargo 步骤 A/B/C + 9 条 lldb 断言）
- ⚠️ **原先的两个变异测试脚本已于 2026-10-02 按用户要求移除**（`mutation-test-m0-consistency.py` 33 个变异、`mutation-test-debug-smoke.sh` 4 个变异），以精简交付物。快照在 `.workbuddy/backup/scripts-removed-20261002-1504/`（该目录被 git 忽略），拷回 `scripts/` 即可重跑。**代价**：现在没有自动手段能证明断言「真的会失败」—— 改动 `check-m0-consistency.py` 的断言后必须**人工反向验证**（把目标改成注释形态或删掉，确认报 ✗）。它们曾真实抓到 3 处缺陷（2 条恒真断言 + 夹具缺文件导致断言被整段跳过）
- `scripts/env-probe.sh`（E2 环境事实数据源）同样已移除，但**脚本体完整保存在 `M0-plan.md` T14**，需要复核 E2 时按文重建即可 —— 它的输出已固化进 `M0-manifest.json`

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
| — | `milestones/M0-plan.md` | 计划 | **怎么干 M0** —— T01–T28（Phase 0 已完成） |
| — | `milestones/M0-benchmarks.md` | 实测 | **凭什么是这样** —— 基准数据、D-M0-9 决策依据、可复现性结论、环境陷阱清单（11 项）；环境陷阱清单（14 项）；**§7 CodeLLDB 平台包死代理案 / §8 rustc 尾位置 `let` 不进 DWARF / §9 VS Code Server 死代理案（含层级辨异表）/ §10 F5 报 `Cargo command did not complete successfully.`（`cargo.cwd` + `filter.name` 两个静默陷阱，含三层回归）** |
| — | `milestones/M0-manifest.json` | 事实 | 机器可读 —— `environment` / `gate` / `benchmarks` |

**阶段文档**（`docs/milestones/`，每阶段开工前须三样齐备，03 §6.2）：`<ID>-design.md` + `<ID>-tests.md` + 机器可读 manifest。

**依赖方向**：01 + 02 → 03（03 §0 声明二者为其「输入规格」）→ `docs/milestones/<阶段ID>-{design,tests}.md`；04 落实 03 §0.2 的平台约束。

**已创建**：`docs/contracts/`（**空**，M0 Phase 4 才产出 5 份契约初稿）、`docs/milestones/`（M0 四件套 + `M0-manifest.json`）。

**引用约定**（定义在 `01-feature-set.md` §0.4）：`spec:Lxx-yy` → `doc/go_spec.html`（规范性）；`nodes:Lxx-yy` → `syntax/nodes.go`（规范性）；`runtime/*.go:NNN`、`abi/type.go:NNN`、`gc/*/`、`link/ld/`（参考性）。

## 关键方法论约定

- **Superpowers TDD**：行为切片一律 RED → GREEN → REFACTOR；先确认失败来自缺失行为而非环境/harness，再最小实现
- 完成度按 **四维**（Frontend / Interpreter / Native / Runtime-metadata）分别跟踪，**不以「246 项全勾选」作为总退出条件**
- 每个阶段及独立验收子阶段开工前须产出 `docs/milestones/<阶段ID>-tests.md` + 机器可读 manifest
- 提交仅在明确要求时执行；提交信息引用功能 ID / 验证证据
- 只读官方语料，**不改动** `go_source_code/`
- ⚠️ **大段插入已有文档时以 `git show HEAD:<file>` 为基线重建**，不要在磁盘现状上叠加 —— 本轮发生过一次整段回退（已落盘的 6 处编辑丢失，而 HEAD 是对的），成因未能确定。插入后必须**逐条 `grep -cF` 验收**关键内容，不能只看脚本报成功
- ⚠️ **门禁状态要在两处同步：manifest（机器可读事实）与 M0-plan 的门禁汇总表（人读入口）**。自检原本只校验前者，漂移不会被发现（人反而会以文档为准）。现有 4 条断言做双向校验 + 任务编号连续性

## 重要事实

- `go_source_code/test/` 与 `src/internal/types/testdata/` 是**语言级黑盒测试**，只依赖编译器对外行为，可直接作为 Rust 重写实现的验收测试集
- **官方测试驱动器是 `src/cmd/internal/testdir/testdir_test.go`**（2,072 行），**不是 `test/run.go`（该文件不存在）** —— 此为 2026-10-02 实测修正，`02-test-inventory.md` 尚待回写
- **指令集共 16 个**（非此前记录的 7 个）：`compile`/`compiledir`/`build`/`builddir`/`buildrundir`/`run`/`buildrun`/`runoutput`/`rundir`/`runindir`/`asmcheck`/`errorcheck`/`errorcheckdir`/`errorcheckoutput`/`errorcheckandrundir`/`skip`；**未知指令直接 `t.Fatalf` 硬失败**
- 三条 harness 必需精确实现的规则（带源码行号，详见 `M0-tests.md` §1.3）：① 指令行是「首个非空且非构建约束行」，**不能假定在第 1 行**；② **`.out` 缺失 ⇒ 期望输出为「空」**（不是「任意输出都可通过」）；③ `errorcheck` 即使裸写也会被自动加 `-d=ssa/check/on`，故其期望**不能当纯语言语义验收**
- 02-test-inventory §0.2 记录了 **9 处**对既有文档的事实修正（如 `syntax/testdata/` 实为 31 个文件），**以该节为准**
