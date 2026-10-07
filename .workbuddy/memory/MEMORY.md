# 项目长期记忆 —— rust_go_compiler

> 压缩版（2026-10-07 整理，原 16 KB 超出注入上限被截断）。**只保「每轮都要知道」的硬事实**；
> 细节去 `AGENTS.md`（工程入口，任何任务先读它）与 `docs/`。

## 概况

- 目标：用 **Rust 重写 Go 编译器**，代号 **`rgoc`**；Git remote `origin` →
  <https://github.com/nobody0726/rust-go-compiler>（public / `main`）
- 语料：`go_source_code/`（185 MB / 15,618 文件，`VERSION`=`go1.27.1`），**只读、不入库**，
  由根目录 `corpus-manifest.sha256` 锁定（`VERSION` 文件本身不足以证明一致）
- 首发平台：**Linux / arm64（Docker）→ `aarch64-unknown-linux-gnu` / ELF**，不维护 macOS 第二套环境
- git 身份：`nobody0726` / `102368194+nobody0726@users.noreply.github.com`
  （**noreply 格式才能被 GitHub 关联**）。若看到 `wangf <yhome@yhomedeMac-mini.local>` 就是身份丢了，
  用 `git config --global` 重设（`gh config set git_user_name/email` 在 gh 2.102 **已废弃**）
- `git push` 曾间歇被拦（`github.com` CONNECT 502）。真被拦时走已登录的 `gh api`
  自建 blob→commit→update ref。提交仅在用户明确要求时执行

## 状态：M0 Phase 0/1/2/3 全部完成（T01–T47 ✅，E1–E7 + E10 **八条门禁全过**），**Phase 4 待开工**

- 阶段文档 `docs/milestones/`：`M0-design.md`（已确认，D-M0-1~15）、`M0-tests.md`（**已冻结** T29，
  §5.1 有**修订 R1**）、`M0-plan.md`（T01–T55）、`M0-benchmarks.md`（**§12 = E6 实测**）、
  `M0-manifest.json`
- 2026-10-07：换机复验（从 Phase 0 重建环境）+ **Phase 3 全部完成**，同日两轮。
  详见 `.workbuddy/memory/2026-10-07.md`
- 镜像 `rgoc:dev` 3.0 GB / 14 层；⚠️ **image id 不可复现，不得写进门禁**，钉子用
  `base.index_digest` + `src.*_sha256`
- 五个 Phase：0 容器底座 → 1 VSCode 调试环境 → 2 Rust 骨架+harness → 3 三 spike → 4 契约+报告
- 关键洞察：Go oracle 是**硬约束**（必须精确 `go1.27.1`），Rust 是软约束 → `golang:1.27.1-bookworm` + `rustup`
- 三个 spike 放独立 crate `rgoc-spikes`（D-M0-14，可整块删）；`rgoc-hir` 标 `SPIKE-ONLY`（D-M0-11，M5 替换）；
  `double_sum` **不删**（D-M0-15，E5 复验锚点）

### ⚠️ Phase 3 实测推翻的三处「文档/直觉」（别踩回去）

1. **Go 内建 `println` 写 stderr，不是 stdout**（go1.27.1 实测，`od -c`，stdout 长度 0）。
   `T-S1-01` 原期望「stdout 精确 `3\n`」是错的，已订正（`M0-tests.md` §5.1 修订 R1）。
   **别改用 `fmt.Println`** —— 那要 import 解析 + 包符号表 + 接口派发，spike 会高一整个量级。
2. **`print` 不加分隔符，只有 `println` 加**（`print("a","b",1,2)` → `ab12`）。规范只列内建名、
   **没规定分隔符** ⇒ 这类行为只能问 oracle。
3. **链接裸汇编必须两个开关**：`-nostartfiles`（否则 `Scrt1.o` 已定义 `_start`，链接失败）
   + `-Wl,-s`（否则 `.strtab` 残留 clang 随机中间名 `hello-d9450b.o`，**产物不可复现**）。
   判据：BuildID 三次相同（内容哈希）⇒ 代码本身可复现，差异只在符号表。
   **`-static` 故意不加**（会掩盖「到底依赖了什么」）。

### workspace 五个 crate（判定逻辑**只有一处**：`runner::run_layer`）

| crate | 角色 | 测试 |
|---|---|---|
| `crates/rgoc-harness` | `ir`(11) `instruction`(14) `corpus`(15) `oracle`(12) `compare`(26) `runner`(19) | 114 条 |
| `crates/rgoc-driver` | 统一 CLI（`harness list/run/report`）；手写解析**不引 clap**；**不预留**未实现子命令 | 23 条 |
| `xtask` | 语料枚举（分母 279）/ 报告生成 / manifest environment 生成 | 12 条 |
| `crates/rgoc-hir` | **SPIKE-ONLY**：`BigInt`（base 2^32）+ `Val` + `Stmt::Print{newline,stream,args}` + `Stream` + C1 留位 | 24 条 |
| `crates/rgoc-spikes` | **SPIKE-ONLY**：`fixtures`（S1/S2 **必须共用** `sum_expr()` 才能交叉验证）/ `interp` / `ssa` / `ssa_needs` / `native` / `native_records` + 3 bin | 83 条 |

**共 260 条全绿，自检 147 条断言。** 三条纪律：
① driver 与 xtask 都只**调** `run_layer` 不自己判语义（两份判定 ⇒ E4 变成「两份报告说过了」）；
② 报告渲染也只有一处（`rgoc_driver::report::render_text`）；
③ **spike 的判定逻辑全在 lib，bin 只做 I/O 边界**（否则 S1/S2 无法交叉验证）。

- harness 命门规则见 `M0-tests.md` §1.3（R1 指令行非第 1 行；R1b `:522` 平台过滤先于 `:541` switch；
  R2b `runcmd` 合并两流；R6 是 `go tool compile` 非 `go build`；诊断路径先 `replacePrefix`）。
  两条易忘：`CaseSpec` **必须带磁盘路径**（语料在只读 `GOROOT/test`）；`//go:build X` 之后
  **还得有指令行**，否则 R1 跳过后 action 变 `"package"`。
- ✅ T36 已把「harness 能力不足」（`UNSUPPORTED-REGEX:`）与「真的不匹配」**分开判**。
  ⚠️ **未决**（T55 前要决定）：279 全量里多数 errorcheck 仍会命中 `UNSUPPORTED-REGEX`
  （`{n,m}` `[...]` `+` `?` `^` `$`）⇒ 判 `harness-failure`。扩展子集还是引 regex crate？

## 环境硬事实（别再踩）

- **所有构建测试在容器内**：`scripts/in-container.sh <命令>`（先探 daemon，失败即报错**不降级**）
- `rgoc/target/` 在命名卷 `rgoc-target`（2.3×）。① 新建空卷属主 `root:root`，须先 `chown 501:20`
  （故镜像装了 `sudo`）；② **`cargo clean` 会 `EBUSY(16)`/exit 101** → 用
  `find rgoc/target -mindepth 1 -delete`；③ ⚠️ **cargo 的 registry/git 卷同样会属主错，且
  `in-container.sh` 的 bootstrap 只在「新建空卷」时 chown** → 症状 `Permission denied (os error 13)`，
  修法 `docker run --rm -v <vol>:/x alpine chown -R 501:20 /x`，**uid 是 501 不是 1000**
- 门禁四条：`cargo fmt --check` / `cargo check --workspace --all-targets` /
  `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace`
- 验证命令别用 `cmd | tail -N` 串 `&&`（退出码取自 `tail`，失败被吞）→ 脚本首行 `set -euo pipefail`
- 计时器：宿主 `/usr/bin/time` 无 GNU `-f`（用 `-p`），容器内**没有** `/usr/bin/time`
  （用 bash 内建 `time` + `TIMEFORMAT`）
- 登录 shell 的 PATH 被 `/etc/profile` 重置 → 镜像层 5 写 `/etc/profile.d/50-rgoc-toolchains.sh`
- `cargo new <dir>` 目录名不能叫 `crate`（Rust 关键字）
- **Rust 侧命名坑**：中文标识符可用但**不能夹大写 ASCII**（`512MiB`、`..._ID_...` 触发
  `non_snake_case`，`-D warnings` 下直接打爆门禁 —— T37 复发一次，改成「编号」）；
  正则批量改代码后**立刻编译**
- ⚠️ **「静默失败」六连坑**（T37 全踩；详见 `M0-plan.md` T37 完成记录）：最反直觉的两个是
  ① `rustc`/`cargo` **不带参数**会把完整 help 打到 stdout 且**退出码 0**；
  ② `docker/image.lock` 形态是 `前缀.键<空格>= 值`（**不是 `key: value`**），
  按冒号解析会「取不到就跳过」⇒ 与下游的「取不到就跳过」**两层静默叠加**。
  **共同形状：失败时字段要么空、要么塞进了别的东西，而 JSON 仍合法 ——
  只能靠「断言具体值」而不是「断言非空」抓出来**

### VSCode / 调试链路（三个独立复发点，E5 复验必看）

1. **宿主 `http.proxy` 被下推进容器**（经 AHP `root/configChanged`）→ 容器内下载全废；
   远端 Machine settings 写 `http.proxy: ""` **实测无效**。修法：`scripts/install-codelldb.sh` 离线绕过
2. **VS Code Server 在持久卷 `/vscode`**：宿主每次升级 VSCode 都复发 → 跑
   `scripts/install-vscode-server.sh --commit <宿主 commit>`，装完**必须 Reopen in Container**
3. **容器重建后 CodeLLDB 平台包必丢** → 重跑 `install-codelldb.sh`。
   两段式安装：扩展本体 ≠ 平台包（含真 lldb），判据 `<扩展目录>/platform.ok`

- 同一句 `PROXY 127.0.0.1:7897` 可能来自**两个层级**（① 扩展平台包 ② Server 本体），
  **唯一可靠判据是日志里的 `Path:`** —— 指向宿主 `/var/folders/…` 就是 ②
- ⚠️ **`launch.json` 两个静默陷阱**（写错不报错，按 F5 才以 `Cargo command did not complete
  successfully.` 收场）：① 工作目录取自 **`cargo.cwd`** 而**不读顶层 `cwd`**（漏写→回退 `/work`
  无 `Cargo.toml`→cargo 退 101）；② `filter.name` 比 **target name（下划线）**不是包名。
  真错在 OUTPUT→LLDB 通道的 `Cargo exited with code N`。
  ⚠️ **别用 shell 复现它的 cargo 命令**（无 `shell:true` 的 `spawn`，shell 会剥掉单引号→假 TOML 报错），
  必须用 argv 列表。
- `rustc` **不为「尾位置直接返回的 `let` 绑定」生成 DWARF 变量条目** → 调试目标必须让中间值被
  第二次读取（`double_sum` 写成 `sum * 2`）

## 自检脚本的四个陷阱

- `check-m0-consistency.py`（**147 条**断言；§5d 登记↔报告对撞 / §5e phase_plan↔产物 /
  **§5f E6/E7↔证据对撞**）：用 `TOTAL` 计数器，最后一条断言把自己数进去，**必须是文件最后一条**；
  改断言数只需同步 `AGENTS.md` §1 一处
- ⚠️ **断言必须锚定真实代码，注释里的同名串会让它恒真**（本仓踩过两次）→ 第 5/5b 节一律作用于
  `code_only()`（剔整行注释）
- ⚠️ **断言里写死的键列表会变成盲区**（**T38 → T39 → T47 第三次同形**）：曾写死
  `("E1","E2","E10","E5")`，导致 E3/E4/E6/E7 **从未被校验过**。
  **现改为从 manifest 的 `gate` 节动态枚举全部键** + 一条反向的「八条齐备」断言。
  共同形状：**「漏了」不会报错，只会让断言恒真**
- ⚠️ **断言关键词必须锚定真实字段，不能凭印象写**（T37/T38/T39/**T47** 连续四轮同形）：
  T47 又一次把测试写成查「不等于」而记录里是符号「≠」⇒ 恒红。
  **写完断言要反向验证它真能失败**
- ⚠️ **断言可能锚定「会过期的状态」**（T47 新踩）：原断言查 `phase3.blocked_by` 含「已全部通过」——
  那是**开工前**的判据，Phase 3 完成后恒红。修法不是删，而是**按当前阶段改写判据**，
  并把历史事实单独留在 `blocked_by_resolved` 里给下一阶段复用
- ⚠️ **门禁状态要两处同步**（manifest + `M0-plan.md` 门禁汇总表），现有断言做双向校验
- ⚠️ **大段插入已有文档要以 `git show HEAD:<file>` 为基线重建**，插入后逐条 `grep -cF` 验收
- 变异测试脚本已按用户要求移除（快照 `.workbuddy/backup/scripts-removed-20261002-1504/`）→ 改断言后须
  **人工反向验证**。`env-probe.sh` 同样已移除，脚本体在 `M0-plan.md` T14
- 环境陷阱清单（**14 项**）在 `M0-benchmarks.md` §6；四则调试案例 §7（平台包）/§8（DWARF）/§9（Server）/
  §10（cargo 启动配置）；期望值复核 §11；**Phase 3 可复现性 §12（含链接不可复现的完整定位）**
- 非 root 下 `/proc/self/status` 的 `CapEff` 恒为 0 → 查容器能力用宿主 `docker inspect … HostConfig.CapAdd`

## 方法论与文档

- **Superpowers TDD**：RED → GREEN → REFACTOR；先确认失败来自缺失行为而非环境/harness
- 完成度按**四维**跟踪（Frontend / Interpreter / Native / Runtime-metadata），
  不以「246 项全勾选」为总退出条件
- 文档体系：`README.md` / `01-feature-set`(246 功能点) / `02-test-inventory` / `03-roadmap`(M0–M12) /
  `04-development-environment`；引用前缀 `spec:Lxx-yy`、`nodes:Lxx-yy` 定义在 `01` §0.4；
  `02-test-inventory` §0.2 的 **9 处**事实修正以该节为准
- **官方测试驱动器是 `src/cmd/internal/testdir/testdir_test.go`（2,072 行）**，不是 `test/run.go`（不存在）；
  指令集共 **16 个**，未知指令直接 `t.Fatalf`；`errorcheck` 裸写也会被自动加 `-d=ssa/check/on`
  （故其期望不能当纯语言语义验收）
- 只读官方语料，不改 `go_source_code/`
