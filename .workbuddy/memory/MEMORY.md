# 项目长期记忆 —— rust_go_compiler

## 概况

- 目标：用 **Rust 重写 Go 编译器**，代号 **`rgoc`**；Git 已发布 <https://github.com/nobody0726/rust-go-compiler>（public / `main` / remote `origin`）
- 语料：`go_source_code/`（185 MB / 15,618 文件，`VERSION`=`go1.27.1`，2026-08-28），**只读、不入库**，由根目录 `corpus-manifest.sha256` 锁定（校验 `shasum -a 256 -c corpus-manifest.sha256`；`VERSION` 文件本身不足以证明一致）
- 首发平台：**Linux / arm64（Docker）→ `aarch64-unknown-linux-gnu` / ELF**，不维护 macOS 第二套环境
- **git 身份已配好**（2026-10-07）：`user.name=nobody0726`、
  `user.email=102368194+nobody0726@users.noreply.github.com`（**noreply 格式才能被 GitHub 关联**，
  `gh api .../commits/main --jq .author.login` 实测返回 `nobody0726` 而非 null）。
  ⚠️ 若又看到 `wangf <yhome@yhomedeMac-mini.local>`，是身份丢了，用 `git config --global` 重设
  （`gh config set git_user_name/email` 在 gh 2.102 **已废弃**，写了 git 也读不到）。
- **`git push` 曾常被拦**（`github.com` CONNECT 间歇 502），**但 2026-10-07 两次push（普通 + force）均成功**，
  暂未复现。真被拦时走 `gh api`（已登录，scope `repo`+`workflow`）——
  ⚠️ 原记录提到的技能 `github-push-via-api` **在本机 `~/.workbuddy/skills/` 下并不存在**，
  需要时按「gh api 建 blob → commit → update ref」自己走，别指望那条技能
- **工程入口是根目录 `AGENTS.md`**，任何任务先读它。本文件只保「每轮都要知道」的硬事实

## 状态：M0 **Phase 0/1/2 全部完成**（T01–T39 ✅，**E1/E2/E3/E4/E5/E10 六条门禁全过**），**Phase 3 待开工**

- 阶段文档 `docs/milestones/`：`M0-design.md`（已确认，D-M0-1~15）、`M0-tests.md`（**已冻结** T29）、`M0-plan.md`（T01–T55）、`M0-benchmarks.md`、`M0-manifest.json`（environment/gate/benchmarks）
- **2026-10-07 换机复验**：工作区从 GitHub 重新拉取后本机无 Docker，
  **从 Phase 0 起重建了全套环境**（镜像 / 命名卷 / VS Code Server / CodeLLDB），并修掉两个真bug。
  六条门禁全部**重新实测通过**（E1 构建、E3 153 条、E4 20/20、E5 断点命中），
  提交 `2d990d0`，自检 **125 条断言**。详见 `.workbuddy/memory/2026-10-07.md`。
- **E5 已于 2026-10-02 首次实测通过，并于 2026-10-07 复验通过**（宿主 VSCode 1.139.1；
  容器内 LLDB 日志 `Installing platform package` 与 `PROXY` 各 0 次为佐证，
  登记在 `gate.E5.reverified`）；**E3 已于 2026-10-04 通过**（T36 六类自测 19 条正反例齐备）
- 镜像 `rgoc:dev` = `sha256:46a572aa…2093`（3.0 GB / 14 层；engine 29.8.2、内核 7.0.14-linuxkit）——
  ⚠️ 2026-10-07 重建，**image id 与旧记录不同属正常**（位级不可复现，钉子是 `base.index_digest` + `src.*_sha256`）
- 三个 spike 放独立 crate `rgoc-spikes`（D-M0-14，可整块删）；`double_sum` **不删**（D-M0-15，E5 复验锚点）
- **E4 已于 2026-10-05 通过**（T38：20/20、5.8 s、峰值 15 MiB；修了 `compare.rs` 的 `\n` 转义）
- **T39 已完成**（Phase 2 收口）：登记漂移已修（`gate.E3.total_tests` 114→153、`phase_plan.done` 补 T37/T38）+ 新增 §5d/§5e **19 条断言**把登记与 E4 报告**对撞**
- **下一步**：**Phase 3**（T40 建 `rgoc-hir` 标 `SPIKE-ONLY` → T41 建 `rgoc-spikes` → T42/43/44 三个 spike S1/S2/S3 → T45 E6 → T46/47 E7）。**D-M0-2 阻塞已解除**
- 五个 Phase：0 容器底座 → 1 VSCode 调试环境（门禁=实测断点命中）→ 2 Rust 骨架+harness → 3 三 spike → 4 契约+报告
- 关键洞察：Go oracle 是**硬约束**（必须精确 `go1.27.1`），Rust 是软约束 → 用 `golang:1.27.1-bookworm` + `rustup`

### workspace 三个 crate（判定逻辑**只有一处**：`runner::run_layer`）

| crate | 角色 | 测试 |
|---|---|---|
| `crates/rgoc-harness` | 六个模块的测试基础设施（见下表） | 114 条 |
| `crates/rgoc-driver` | **统一 CLI**（T37）：`harness list/run/report`；手写解析**不引 clap**；**不预留**未实现子命令 | 23 条 |
| `xtask` | 构建期工具（T37）：语料枚举（279）/ 报告生成 / manifest environment 生成 | 12 条 |

**共 153 条全绿，门禁四条全过 + 自检 123 条断言。** 两条纪律：① driver 与 xtask 都只**调** `run_layer`，
不自己判语义（两份判定 ⇒ E4 变成「两份报告说过了」）；② 报告渲染也只有一处
（`rgoc_driver::report::render_text`），xtask 初稿里从 JSON 反推文本的 `human_text` 已删。

### 已落地模块（`rgoc/crates/rgoc-harness/`，六个模块 + 六套测试，114 条）

| 模块 | 内容 | 测试 |
|---|---|---|
| `ir.rs` | `TestCase` 16 必录字段、`Verdict` 八种不得合并、`Limits::for_layer` 是冻结预算唯一入口 | 11 条 |
| `instruction.rs` | 16 个指令、`dispatch(ins, platform_ok)` 把 R1b 顺序写进签名 | 14 条 |
| `corpus.rs` | 平台过滤 `should_test` + `enumerate` + U 归类；**枚举实算分母 == 279**（与 T29 冻结值交叉校验通过） | 15 条 |
| `oracle.rs` | 版本守门 + R6 `go tool compile` + 超时回收（父子关系直，`child.kill()` 即可，`unsafe_code="forbid"` 不需 `kill(-pgid)`） | 12 条 |
| `compare.rs` | R2/R3/R4 + `errorCheck` 匹配；正则只实现子集，不认识就报 `UnsupportedRegex` | 26 条 |
| `runner.rs` | 一条用例端到端（R1→R1b→switch→oracle→比对）+ `LayerReport`；被过滤的用例**不执行**（`duration` 恒 0） | 19 条 |

- ⚠️ **未决决策点（T38 已消解，但更大问题还在）**：`initloop.go`（T-C-13）的跨行期望是
  `a refers to b\n.*b refers to c\n.*c refers to a`。根因是 `compare.rs` 把 `\` 后字符**一律字面量**，
  `\n` 变成字母 `n`；Go 的 `regexp/syntax` 把 `\n \t \r` 当 **Perl 类转义 ⇒ 真控制符**。
  已修（控制字符 vs 元字符**分两类**，`\.` 仍字面否则就是**放宽判定**）。
  ⚠️ 但 **279 全量里多数 errorcheck 仍会命中 `UNSUPPORTED-REGEX`**（`{n,m}` `[...]` `+` `?` `^` `$`）
  ⇒ 判 `harness-failure`。要「全量逐条判语义」须先决定扩展子集还是引 regex crate（T55 前）。
- ✅ **T36 已完成**（E3）：六类自测正反例齐备；「harness 能力不足」（`UNSUPPORTED-REGEX:`）与「真的不匹配」**已分开判** —— 前者 `harness-failure`、后者 `compiler-failure`
- ⚠️ **`CaseSpec` 必须带磁盘路径**（不只源码）：真实语料在**只读**的 `GOROOT/test`，执行器只读不写工作目录
- ⚠️ **fixture 坑**：`//go:build X` 之后**还得有指令行**，否则 R1 跳过后 action 变成 `"package"`（未知指令）；而**被过滤的用例会短路**，这个错只在「约束满足」那条反例里才暴露
- harness 命门规则（`M0-tests.md` §1.3）：R1 指令行非第 1 行；R1b `:522` 平台过滤先于 `:541` switch；R2b `runcmd` 合并 stdout+stderr 再比 `.out`；R6 是 `go tool compile` 非 `go build`，诊断路径先 `replacePrefix`（`:2059-2072`，含续行）

## Phase 0/1 实测硬事实（别再踩）

- **所有构建测试在容器内**：`scripts/in-container.sh <命令>`（先探 daemon，失败即报错**不降级**）
- **镜像不可位级复现**（14 层中 6 层 digest 变）→ **image id 不得写进门禁**；钉子用 `base.index_digest` + `src.*_sha256`
- `rgoc/target/` 在命名卷 `rgoc-target`（2.3×）。① 新建空卷属主 `root:root`，须先 `chown 501:20`（故镜像装了 `sudo`）；② **`cargo clean` 会 `EBUSY(16)`/exit 101** → 用 `find rgoc/target -mindepth 1 -delete`；③ ⚠️ **cargo 的两个 registry/git 卷同样会属主错，且 `in-container.sh` 的 bootstrap 只在「新建空卷」时 chown —— 已存在但属主错的卷不会被接管**，症状是 `Permission denied (os error 13)`。修法 `docker run --rm -v <vol>:/x alpine chown -R 501:20 /x`，**uid 是 501 不是 1000**（dev 用户复用宿主 uid，按 1000 改仍失败）
- 登录 shell 的 PATH 被 `/etc/profile` 重置 → 镜像层 5 写 `/etc/profile.d/50-rgoc-toolchains.sh`
- 门禁四条：`cargo fmt --check` / `cargo check --workspace --all-targets` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace`
- 验证命令别用 `cmd | tail -N` 串 `&&`（退出码取自 `tail`，失败被吞）→ 脚本首行 `set -euo pipefail`
- 计时器：宿主 `/usr/bin/time` 无 GNU `-f`（用 `-p`），容器内**没有** `/usr/bin/time`（用 bash 内建 `time` + `TIMEFORMAT`）
- `cargo new <dir>` 目录名不能叫 `crate`（Rust 关键字）
- **Rust 侧命名坑**：中文标识符可用但**不能夹大写 ASCII**（`512MiB`、`..._ID_...` 触发 `non_snake_case`，在 `-D warnings` 下直接打爆门禁 —— T37 复发一次，改成「编号」）；正则批量改代码后**立刻编译**
- ⚠️ **「静默失败」六连坑（T37 踩的全是这类，`check-m0-consistency.py` 的空缺检查抓不到 —— 因为值都非空）**：
  ① `rustc`/`cargo` **不带参数**把完整 help 打到 stdout 且**退出码 0**（必须强制带版本参数 + 只取首行）；
  ② `cargo test` 子进程的 PATH ≠ 交互 shell，裸 `ldd`/`uname` 找不到（**一律绝对路径**）；
  ③ 裸 `uname` 只输出 `Linux` 不含版本，**必须 `-r`**；
  ④ glibc 版本取 `ldd --version` **行尾裸版本**（`2.36`）而非括号里的发行版修订（`2.36-9+deb12u14`）；
  ⑤ `docker/image.lock` 形态是 `前缀.键<空格>= 值`（**不是 `key: value`**），按冒号解析会「取不到就跳过」⇒ **两层静默叠加**；
  ⑥ `local.layer_count` 值是 `14（其中容器层 7 个：…）`，`parse::<u64>()` 失败后回退成字符串。
  **共同形状：失败时字段要么空、要么塞进了别的东西，而 JSON 仍合法 —— 只能靠「断言具体值」而不是「断言非空」抓出来**

### VSCode / 调试链路（三个独立复发点）

1. **宿主 `http.proxy` 被下推进容器**（经 AHP `root/configChanged`）→ 容器内下载全废；远端 Machine settings 写 `http.proxy: ""` **实测无效**。修法：`scripts/install-codelldb.sh` 离线绕过
2. **VS Code Server 在持久卷 `/vscode`**：`~/.vscode-server/bin/<commit>` 只是符号链接，实体在卷内；Dev Containers **两处 `test -d` 都缺才下载** → **宿主每次升级 VSCode 都复发**，跑 `scripts/install-vscode-server.sh --commit <宿主 commit>`，装完**必须 Reopen in Container**
3. **容器重建后 CodeLLDB 平台包必丢**（`~/.vscode-server/extensions/` 不在任何卷里）→ 重跑 `install-codelldb.sh`

- ⚠️ 同一句 `PROXY 127.0.0.1:7897` 可能来自**两个层级**：① 扩展平台包（容器内）② VS Code Server 本体（宿主侧）。**唯一可靠判据是日志里的 `Path:`** —— 指向宿主 `/var/folders/…` 就是 ②
- CodeLLDB **两段式安装**：扩展本体 ≠ 平台包（含真 lldb 22.1.8-codelldb）。判据 `<扩展目录>/platform.ok`
- ⚠️ **`launch.json` 两个静默陷阱**（写错不报错，按 F5 才以 `Cargo command did not complete successfully.` 收场）：① 工作目录取自 **`cargo.cwd`**，**不读顶层 `cwd`**（漏写→回退 `/work` 无 `Cargo.toml`→cargo 退 101）；② `filter.name` 比的是 **target name（下划线）**不是包名（连字符）。**真错在 OUTPUT→LLDB 通道的 `Cargo exited with code N`**
- ⚠️ **别用 shell 复现 CodeLLDB 的 cargo 命令**：它是无 `shell:true` 的 `spawn`，shell 会剥掉 `target.'cfg(all())'` 的单引号 → 得到**假的** TOML 报错。必须用 argv 列表复现
- `rustc` **不为「尾位置直接返回的 `let` 绑定」生成 DWARF 变量条目** → 调试目标必须让中间值被第二次读取（`double_sum` 写成 `sum * 2`）

### 自检脚本的四个陷阱

- `check-m0-consistency.py`（**123 条**断言；§5d 登记↔报告对撞 / §5e phase_plan↔产物对撞）：用 `TOTAL` 计数器，最后一条断言把自己数进去，**必须是文件最后一条**；改断言数只需同步 `AGENTS.md` §1 一处
- ⚠️ **断言必须锚定真实代码，注释里的同名串会让它恒真**（本仓踩过两次：`grep` 断点行命中注释行；`install-*.sh` 头部注释里就写着 `platform.ok`/`--noproxy`）→ 第 5/5b 节一律作用于 `code_only()`（剔整行注释）
- ⚠️ **断言里写死的键列表会变成盲区**（T38 踩到）：门禁双向校验写死 `("E1","E2","E10","E5")`，
  于是新加的 `gate.E3`/`gate.E4` **从未被校验过**。**加门禁时必须同步这个列表** ——
  「漏了」不会报错，只会让断言恒真
- ⚠️ **断言的关键词必须锚定真实字段，不能凭印象写**（T37/T38/T39 **连续三轮同形**）：
  T39 查「放宽判定」，那句话却在 `discipline` 节而不在 `fix_required_to_pass` 节里 ⇒ 恒红。
  改成分字段查才过。**写完断言要反向验证它真能失败**（T39 做了 7 次变异）
- ⚠️ **门禁状态要两处同步**（manifest + `M0-plan.md` 门禁汇总表），现有 4 条断言做双向校验
- ⚠️ **大段插入已有文档要以 `git show HEAD:<file>` 为基线重建**，插入后逐条 `grep -cF` 验收
- 两个变异测试脚本已按用户要求移除（快照在 `.workbuddy/backup/scripts-removed-20261002-1504/`）→ 改断言后须**人工反向验证**。`env-probe.sh` 同样已移除，脚本体在 `M0-plan.md` T14
- 环境陷阱完整清单（**14 项**）在 `M0-benchmarks.md` §6；四则调试案例在 §7（平台包）/§8（DWARF）/§9（Server）/§10（cargo 启动配置）；期望值复核在 §11
- 非 root 下 `/proc/self/status` 的 `CapEff` 恒为 0 → 查容器能力用宿主 `docker inspect … HostConfig.CapAdd`

## Excalidraw 架构图（2026-10-04）

- 产出 `.workbuddy/diagrams/rgoc-harness-four-layer-architecture.{excalidraw,png}`，生成器 `gen_arch.py`（**改图改它，别手编 JSON**），本地渲染 `render_local.py` + `render_template.html`
- ⚠️ **skill 自带的 `render_excalidraw.py` 已失效**：其模板走 `esm.sh/@excalidraw/excalidraw?bundle`，其子依赖 `@braintree/sanitize-url@6.0.2/…/constants.mjs` 现 **404** → `__moduleReady` 永不就绪。改用 `cdn.jsdelivr.net/npm/@excalidraw/excalidraw@0.18.1/+esm`
- ⚠️ **text 元素漏写 `text` 字段（只写 `originalText`）→ 全部文字静默消失**，`renderExcalidraw` 仍返回 `{ok:true}`。**「渲染成功」≠「文字在」，必须 Read PNG 回看**
- ⚠️ 同层多行 y **不能按字符串 `\n` 数推算**（Excalidraw 按渲染宽度自动换行）→ 用固定行位
- `playwright install chromium` 收尾报 `[safe-delete] broker denied __dirlock` 可忽略，浏览器已装好

## 方法论与文档

- **Superpowers TDD**：RED → GREEN → REFACTOR；先确认失败来自缺失行为而非环境/harness
- 完成度按**四维**跟踪（Frontend / Interpreter / Native / Runtime-metadata），不以「246 项全勾选」为总退出条件
- 每阶段开工前须齐备 `docs/milestones/<ID>-{design,tests}.md` + manifest（`03` §6.2）
- 文档体系：`README.md`(索引) / `01-feature-set`(246 功能点) / `02-test-inventory` / `03-roadmap`(M0–M12) / `04-development-environment`；引用前缀 `spec:Lxx-yy`、`nodes:Lxx-yy`（规范性）定义在 `01` §0.4
- `02-test-inventory` §0.2 的 **9 处**事实修正以该节为准
- **官方测试驱动器是 `src/cmd/internal/testdir/testdir_test.go`（2,072 行）**，不是 `test/run.go`（不存在）
- 指令集共 **16 个**，未知指令直接 `t.Fatalf`；`errorcheck` 裸写也会被自动加 `-d=ssa/check/on`，故其期望不能当纯语言语义验收
- 提交仅在明确要求时执行；只读官方语料，不改 `go_source_code/`
