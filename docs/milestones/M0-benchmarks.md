# M0 基准测试记录

> 由 T17–T19 产出，汇总进 `M0-manifest.json` 的 `benchmarks` 节（E10）。
> 全部数据为 **2026-10-02 / macOS 27.2 / arm64 / Docker 29.6.2 / aarch64 原生** 环境实测。
> 复现命令见 `M0-plan.md` T17 / T18。

## 1. 镜像

| 指标 | 实测值 | 说明 |
|---|---|---|
| 首次构建（含拉取基础镜像） | **1m14s** | T03，日志含 `[auth] library/golang:pull token`，各层耗时合计 67.8s + 调度开销 |
| 无缓存全量重建 | **59.05s / 61.32s / 68.44s / 84.28s** | 四次 `--no-cache`；84.28s 那次新增了 apt 包（sudo） |
| 二次构建（全缓存命中） | **3.91s / 3.55s / 3.03s** | 仅重新打 tag 与校验层 |
| 镜像体积 | **2.91GB → 2.92GB** | `docker images`；加 sudo 后 +10MB |
| 磁盘占用增量 | **2.909GB** | `docker system df -v` 的 UNIQUE SIZE（基础镜像未被单独 tag，故全部计入） |

## 2. 容器冷启动

| 指标 | #1 | #2 | #3 | 中位数 |
|---|---|---|---|---|
| `docker run --rm rgoc:dev true` | 0.16 s | 0.16 s | 0.15 s | **0.16 s** |
| `docker run --rm rgoc:dev rustc --version` | 0.17 s | 0.21 s | 0.17 s | **0.17 s** |

> 冷启动成本 ≈160ms，相对容器内实际工作量可忽略；**不需要**为提速做常驻容器。

## 3. 编译基准（最小工程）

| 指标 | 实测值 |
|---|---|
| `cargo fmt --check` | < 0.1 s |
| `cargo check --workspace --all-targets`（增量） | 0.07–0.08 s |
| `cargo clippy --workspace --all-targets -D warnings`（增量） | 0.07 s |
| `cargo test --workspace` | 0.00 s（1 passed） |

> 这是 1 crate / 1 函数的最小工程。**真实工作量基准要等 Phase 2** 引入 lexer/parser 之后才有意义；
> 本节只用于确立「工具链可用 + 门禁可跑」这一事实。

## 4. `target/` 放哪：bind mount vs 命名卷（D-M0-9 的决策依据）

测法：造一个 **120 模块 / 1082 行**的 crate，源码放在 bind mount 内（两种布局相同），
只有 `target/` 的落点不同。每次先清空 `target/`，再 `cargo build`，取 3 次。

> **布局要点**：crate 必须建在 bind mount 之内。若建在容器内 `/tmp`（overlayfs），
> 两种布局都测不到宿主文件系统 —— 这恰恰是 D-M0-9 要问的问题。

| 布局 | #1 | #2 | #3 | 中位数 | `target/` |
|---|---|---|---|---|---|
| **A** `target/` 在 bind mount（宿主 macOS FS） | 0.336 s | 0.359 s | 0.287 s | **0.336 s** | 6.2 MB / 141 文件 |
| **B** `target/` 在命名卷 | 0.150 s | 0.137 s | 0.133 s | **0.137 s** | 6.2 MB / 141 文件 |

**结论：B 比 A 快约 2.3×**，且两者产物完全一致（同体积、同文件数）。

### B 的两个代价（均实测，均已给出对策）

| 代价 | 原始报错 | 对策 |
|---|---|---|
| 新建空卷属主是 `root:root`，`dev` 写不进去 | `error: failed to create directory '/work/rgoc/target/debug'` / `Caused by: Permission denied (os error 13)` / `exit=101` | `in-container.sh` 做 bootstrap `chown 501:20`；devcontainer 走 `postCreateCommand` |
| `cargo clean` 无法删除卷挂载点 | `error: failed to remove directory '/work/rgoc/target'` / `Caused by: Device or resource busy (os error 16)` / `exit=101` | 清空改用 `find rgoc/target -mindepth 1 -delete`（两种布局通用，故不影响可比性） |

**决策：改用命名卷**（`rgoc-target` → `/work/rgoc/target`），理由：

1. `cargo build` 是开发循环中最高频的操作，2.3× 的差距会持续复利；
2. 两个代价都是一次性/可脚本化的，已固化进 `scripts/in-container.sh` 与 `.devcontainer/devcontainer.json`；
3. 编辑器与门禁共用同一卷名，满足 D-M0-10「同一环境」的要求。

## 5. 可复现性：镜像**不可**位级复现（E1 的实质结论）

同输入、同 Dockerfile、连续两次 `docker build --no-cache`：

| 对比项 | 构建 #1 | 构建 #2 | 相同？ |
|---|---|---|---|
| 镜像 ID | `sha256:42feb4bc…` | `sha256:853e9b77…` | **否** |
| 层数 | 14 | 14 | 是 |
| 层 digest 逐个比对 | — | 第 **8–13** 层（共 6 层）digest 不同 | **否** |
| 耗时 | 61.32 s | 68.44 s | — |

**结论与 E1 的判定方式调整**：

- `image_id` **不能**作为钉子，也**不得**写进门禁断言 —— 它只是「事故现场与本文件是否对得上」的事后比对依据；
- 真正的钉子是 **`base.index_digest`**（上游不可变 digest）与 **`src.*_sha256`**（本地源码指纹）；
- 因此 E1 的判定改为：`docker build` 可从零重放成功 + `docker/image.lock` 无占位符 + 基础镜像按 digest 解析成功。
  **不**要求 image id 稳定。

> 顺带记录一个坑：`docker build` 命中全部缓存时，**也会**让 `rgoc:dev` 这个 tag 指向一个新的 image id
> （实测 `502c3be0…` → `7b6c688b…`，两者 `.Created` 时间戳相同）。这说明 tag 指向会随构建漂移，
> 进一步支持「不要把 image id 当门禁」的结论。

## 6. 环境陷阱清单（本节是给后续阶段的避坑提示）

| 陷阱 | 表现 | 正确做法 |
|---|---|---|
| 宿主 macOS 的 `/usr/bin/time` 不支持 GNU 的 `-f` | `illegal option -- f`，**命令整条不执行** | 用 `-p`，或用 bash 内建 `time` + `TIMEFORMAT` |
| 容器内**没有** `/usr/bin/time` | `No such file or directory`，被测命令根本没跑 | 一律用 bash 内建 `time`（截图见 §4 的命令） |
| 验证命令里 `cmd \| tail -N` 会吞掉失败 | 管道退出码取自 `tail`，clippy 失败仍返回 0 | 脚本首行加 `set -euo pipefail` |
| `cargo new <dir>` 目录名为 Rust 关键字 | `invalid package name \`crate\`：it is a Rust keyword` | 换名字（本例用 `probe`） |
| 登录 shell 的 PATH 被 `/etc/profile` 重置 | `bash -lc 'cargo --version'` → command not found | 镜像层 5 写 `/etc/profile.d/50-rgoc-toolchains.sh` |
| 新建命名卷属主是 root | `Permission denied (os error 13)` | 首次使用前 `chown 501:20` 卷根 |
| **宿主下发的 `http.proxy` 在容器内是死地址** | CodeLLDB 报 `Failed to establish a socket connection to proxies: PROXY 127.0.0.1:7897` —— 扩展**本体**装成功了，但它去 GitHub 下**平台包**时被卡住（详见 §7） | 用 `scripts/install-codelldb.sh` 绕开 VSCode 网络栈离线安装。**注意**：在远端 Machine settings 里写 `"http.proxy": ""` 实测**无效**（§7 第 6 条），别指望它 |
| **同一句报错可能来自不同层级** | 同样一句 `Failed to establish a socket connection to proxies: …`：既可能是扩展**平台包**（§7），也可能是 **VS Code Server 本体**（§9）—— 两者修法完全不同 | **先看日志里的 `Path:`**：若指向宿主 `/var/folders/…`（宿主临时目录），那就是 server 这一层 → `scripts/install-vscode-server.sh`。别看到眼熟的报错就按上一次的办法修 |
| **升级宿主 VSCode 会让窗口再也连不上容器** | commit 变了而持久卷 `/vscode` 里没有新 server → `Installing VS Code Server for commit …` → 被那个**已停服**的代理挡住（VSCode **不回退直连**）→ 报 `Retrying to download VS Code Server` / `TypeError: Failed to fetch`（详见 §9） | 跑 `scripts/install-vscode-server.sh --commit <宿主新 commit>` 装进持久卷 `/vscode`，此后两处判据直接命中、**不再发起任何下载**，也不用改宿主配置。**装完必须 Reopen in Container** |
| **调试器用的不是镜像里的 `lldb`** | 镜像内的系统 `lldb` 是 `14.0.6`；CodeLLDB 平台包**自带** `lldb 22.1.8-codelldb`，适配器只用自带那一份 | 排查调试问题时用 `<扩展目录>/lldb/bin/lldb`，不要用 `which lldb` 的结果 |
| **镜像内 `lldb` CLI 缺 python 模块** | `lldb --version` 直接抛 `ModuleNotFoundError: No module named 'lldb.embedded_interpreter'` | 层 2 加 `/usr/local/bin/lldb` 包装脚本注入 `PYTHONPATH`（`python3-lldb-14` 本就已安装，模块在 `/usr/lib/llvm-14/lib/python3.11/dist-packages/`；真因是 `/usr/lib/python3/dist-packages/lldb` 是相对符号链接，python 把它解析成 namespace package） |
| **CodeLLDB 的 cargo 工作目录取自 `cargo.cwd`，顶层 `cwd` 对它无效** | F5 报 `Cargo command did not complete successfully.`；LLDB 输出通道里是 `Cargo exited with code 101`，真因是 `could not find Cargo.toml in /work`（回退到了 workspaceFolder） | 在 `cargo` 对象里写 `"cwd": "${workspaceFolder}/rgoc"`。**顶层 `cwd` 照写也没用** —— 它只影响调试目标。详见 §10 |
| **CodeLLDB 的 `filter.name` 要比 target name（下划线），不是包名（连字符）** | 报 `Cargo has produced no matching compilation artifacts.` —— 包名 `rgoc-harness` 与 cargo 报的 `rgoc_harness` 不相等，0 匹配 | filter 写 `"name": "rgoc_harness"`（crate 未写 `[lib] name` 时 = 包名把 `-` 换成 `_`）。详见 §10 |
| **用 shell 复现 `--config=target.'cfg(all())'.runner=[…]` 会得到假报错** | `TOML parse error … invalid unquoted key` —— shell 剥掉了单引号；CodeLLDB 是 `spawn` 无 shell，参数里引号是保留的 | 复现时用 argv 列表（Python `subprocess` / 无 shell），别用 shell 命令串 |

## 7. 案例：CodeLLDB 在容器内「无法下载」

### 现象

Dev Containers 里扩展安装**成功**（`vadimcn.vscode-lldb` v1.12.3，日志有 `Extension installed successfully`），但调试器不可用，输出面板 `LLDB` 报：

```
Installing platform package from https://github.com/vadimcn/codelldb/releases/download/v1.12.3/codelldb-linux-arm64.vsix
Error: Error: Failed to establish a socket connection to proxies: PROXY 127.0.0.1:7897
```

> CodeLLDB 的**扩展本体**（来自 marketplace）与**平台包**（含真正的 lldb 二进制，来自 GitHub releases）
> 是两次独立下载。前者走 marketplace，后者走 `github.com`。**只看「扩展装上了」会误判成已就绪。**

补充一条更细的判据：扩展的 `ensurePlatformPackage()` 以 `<扩展目录>/platform.ok` 作为「平台包已安装」的
短路条件。出问题时扩展目录里**只有** `extension.js` / `package.json` / `syntaxes` / `images`，
**没有** `lldb/`、`adapter/`、`bin/`、`lang_support/`，也没有 `platform.ok`；
同时 `extensions.json` 中该扩展的 `metadata.targetPlatform` 是**缺失**的
（对比同一次安装的 `rust-lang.rust-analyzer` 明确写着 `linux-arm64`）。

### 因果链（逐段实测确认）

| # | 环节 | 证据 |
|---|---|---|
| 1 | 宿主 VSCode 设了代理 | `~/Library/Application Support/Code/User/settings.json` 里 `"http.proxy": "http://127.0.0.1:7897"`；`nc -z 127.0.0.1 7897` 有服务 |
| 2 | **不是**环境变量注入 | 容器内 `env \| grep -i proxy` 为空；`docker inspect` 的 `Config.Env` 里也无 proxy |
| 3 | 容器内 `127.0.0.1:7897` 无服务 | 那是容器自己的 loopback，代理在宿主侧 |
| 4 | 容器**直连**外网完全可用 | 平台包 `curl --noproxy '*'` → **HTTP 200 / 54,544,514 B / 13.3 s / 4.1 MB/s**（另一次 29.6 s / 1.8 MB/s，网络波动） |
| 5 | **真正来源：宿主主动下发** | `~/.vscode-server/data/logs/<会话>/ahp/ahp-*.jsonl` 抓到：`"action":{"type":"root/configChanged","config":{"http.proxy":"http://127.0.0.1:7897",…}}` |
| 6 | **远端 Machine settings 覆盖无效** | 把 `"http.proxy": ""` 与 `"http.proxySupport": "off"` 写进容器内 `Machine/settings.json`（文件 mtime **03:31:58**），之后新开的扩展宿主会话（**03:40:12**）**仍报同一错误** |

`http.proxy` 是 **application 作用域**设置：在 remote 窗口里由**客户端**求值并下发给服务端
（第 5 条就是它的载体），因此远端自己的 Machine settings **排不上队**。第 6 条是这条推断的直接反证。

> ⚠️ 本文件早期版本曾写「该层写入 Machine settings，优先级高于 User settings，故能覆盖宿主的继承值」——
> 那是**未经验证的推断，已被实测推翻**，2026-10-02 更正。`.devcontainer/devcontainer.json` 里保留这两行
> （对别的 VSCode 版本或环境可能仍有效，且无副作用），但**不要再把它当作修法**。

### 修法（可靠）：离线安装平台包，彻底绕开 VSCode 的网络栈

`scripts/install-codelldb.sh` 的五步：

1. **判据短路**：`<扩展目录>/platform.ok` 存在且 `lldb/bin/lldb` 可执行 → 直接退出 0（幂等，零网络请求）；
2. `curl -fL --noproxy '*' …` 直连 GitHub 下载平台包 —— `--noproxy '*'` 让任何代理设置**完全失效**；
3. 校验 size + sha256（v1.12.3 / linux-arm64 = `54,544,514 B` / `0887f67d…18101`），避免把半截文件装进去；
4. 用 VSCode 自带的 **`code-server`** CLI 从本地 vsix 安装：
   `~/.vscode-server/bin/<commit>/bin/code-server --install-extension <vsix> --force`；
5. 断言 `lldb --version` 输出**含 `codelldb`** —— 查输出而非退出码：损坏的 lldb 会打印 traceback
   但仍以 0 退出（这一点在镜像内系统 lldb 上真实踩过）。

安装路径完整实测（先把 `lldb/ adapter/ bin/ lang_support/ platform.ok` 删掉模拟现场）：

```
==> CodeLLDB 平台包安装器 (codelldb-linux-arm64.vsix, v1.12.3)
扩展本体：…/vadimcn.vscode-lldb-1.12.3
平台包缺失，继续安装。
code-server：…/bin/code-server
下载 https://github.com/vadimcn/codelldb/releases/download/v1.12.3/codelldb-linux-arm64.vsix
  HTTP=200  大小=54544514B  用时=29.618376s  速度=1841576B/s
指纹校验通过（54544514 B）
Installing extensions...
Extension 'codelldb-linux-arm64.vsix' was successfully installed.
OK：lldb version 22.1.8-codelldb
```

幂等路径（已装好时）：`已安装（platform.ok 存在且 lldb 可执行），无需操作。` → exit 0，零网络请求。

**为什么不用 `~/.vscode-server/bin/<commit>/bin/remote-cli/code`**：它要求 `VSCODE_IPC_HOOK_CLI`，
只能在 VSCode 集成终端里用；从 `docker exec` 调用直接返回
`Command is only available in WSL or inside a Visual Studio Code terminal`。
`code-server` 是独立入口，不需要活动窗口。

**为什么不把平台包烤进镜像**：容器的直连网络实测可用且不慢（54 MB / 13–30 s），
烤进去会让镜像 +54 MB 并引入构建期网络依赖；脚本已足够可靠，且容器重建后重跑即可。

### 同一个死代理还会打坏别的扩展

这不是 CodeLLDB 的问题。同一会话日志里 GitHub 与 Copilot 扩展也在报：

```
[GitHubBranchProtectionProvider] Failed to update repository branch protection: connect ECONNREFUSED 127.0.0.1:7897
[GitHub Copilot Chat] FetcherService: node-http failed with error: Failed to establish a socket connection to proxies: PROXY 127.0.0.1:7897
```

凡是在容器里走 VSCode 网络栈的请求都受影响。**本仓不依赖它们**（调试走 CodeLLDB，
Go/Rust 工具链与 git 走 `scripts/in-container.sh` 里的 curl / git），故不做额外处理。

### 若将来必须在走代理的网络下工作

- 让容器访问宿主代理：把 `"http.proxy"` 设为 `"http://host.docker.internal:7897"`
  （`host.docker.internal` 是 Docker Desktop 提供的宿主别名）。
  **但按第 6 条，写在远端未必生效** —— 遇到问题优先回到「离线安装」这条路。
- 宿主侧用 `http.noProxy` 放行 `github.com` 会让客户端下发的配置带上例外 —— 这条**未在本仓实测**，
  且会让宿主自己访问 GitHub 时也绕过代理（若宿主本就靠代理访问 GitHub，会反过来打断宿主），**不建议**。

### 对容器重建的适用性

`~/.vscode-server` **不在**任何命名卷里，容器重建后会被清空、扩展由 Dev Containers 重新安装，
平台包会再次缺失。**此时重跑一次脚本即可**：

```sh
docker exec <容器名> bash /work/scripts/install-codelldb.sh
# 然后在 VSCode 中执行 “Developer: Reload Window”
```

判据始终是同一条：`<扩展目录>/platform.ok` 存在。脚本据此幂等，重复执行无副作用。

## 8. 发现：rustc 不为「尾位置直接返回的 `let` 绑定」生成 DWARF 变量条目

背景：T28 的第 4 项检查要求「断点停在 `let sum = a + b;`，F10 单步后看到 `sum == 3`」。
调试目标原本写成 `let sum = a + b; sum`（尾表达式直接返回 `sum`）。实测**看不到 `sum`**，
且原因不在编辑器的调试器，而在编译产物本身。

### 证据一：DWARF 里根本没有 `sum`

对原形状的 `rgoc_harness::add` 导出 DWARF：

```
<2078> DW_TAG_formal_parameter  DW_AT_location: DW_OP_breg31 (sp): 8    a
<2086> DW_TAG_formal_parameter  DW_AT_location: DW_OP_breg31 (sp): 16   b
<2094> Abbrev Number: 0        ← 子节点到此结束：没有 lexical_block，没有 variable
```

`frame variable` 结果一致：只有 `a = 1`、`b = 2`；`frame variable sum` 报
`error: use of undeclared identifier 'sum'`（单步前、单步后都是如此）。
**这不是 CodeLLDB 或 lldb 的缺陷 —— 调试信息里没有这个变量，任何调试器都拿不到。**

### 证据二：对照实验（同一 crate、同一 `cargo test` profile）

| 函数 | 代码形状 | DWARF 子节点 |
|---|---|---|
| `v_tail` | `let sum = a + b; sum` | 只有 `a`、`b` —— **无 `sum`** |
| `v_double` | `let sum = a + b; sum * 2` | `a`、`b` + **`DW_TAG_lexical_block` → `DW_TAG_variable sum`** |
| `v_reuse` | `let sum = a + b; let doubled = sum * 2; doubled` | 同上 —— **有 `sum`** |

**规则**：`let` 绑定在尾位置被直接作为返回值时，rustc 把它与返回值合并，不生成变量条目；
只要该绑定在 `let` 之后被**第二次读取**，就会生成完整的 `lexical_block` + `variable`。
（环境：rustc 1.98.1 / `test` profile = unoptimized + debuginfo / aarch64。`#[inline(never)]` 与是否内联无关。）

### 后果与修正

T28 的第 4 项检查在原形状下**不可实现** —— 属计划缺陷，与本文件 §5（E1 的判定方式）、
`M0-plan.md` T16 / T18 / T21 / T22 同类。调试目标已改为：

```rust
pub fn double_sum(a: i64, b: i64) -> i64 {
    let sum = a + b;
    sum * 2
}
```

- `sum` 被 `sum * 2` 第二次读取 → 进入 DWARF → 单步后可读；
- 尾表达式是 `sum * 2` 而非 `sum`，因此**不再触发 `clippy::let_and_return`** ——
  原先为通过 T22 门禁而加的 `#[allow(clippy::let_and_return)]` 已**删除**（`clippy --all-targets -- -D warnings` exit=0）；
- 断点行号由 `lib.rs:21` 变为 `lib.rs:27`；`scripts/debug-smoke-test.sh` 从源码**推导**该行号（不硬编码）。

修正后的实测（`scripts/debug-smoke-test.sh`，9 项断言，exit 0）：

```
断点：lib.rs:27   单步后应在：lib.rs:28
  ✓ ① 断点按 file:line 解析到 double_sum   rgoc_harness::double_sum + 20 at lib.rs:27
  ✓ ② 断点命中（未被跳过）                 stop reason = breakpoint 1.1
  ✓ ③ 调用栈含 double_sum 与测试函数        rgoc_harness::tests::double_sum_works
  ✓ ④ 形参 a = 1
  ✓ ⑤ 形参 b = 2
  ✓ ⑥ 单步生效（step over）
  ✓ ⑦ 单步后停在 lib.rs:28
  ✓ ⑧ 中间值 sum = 3 可读                  ← 原形状下这一项拿不到
  ✓ ⑨ 测试跑到结束                         test result: ok. 1 passed; 0 failed
```

> **本脚本与 E5 的分工**：E5 的**验收**是 VSCode 里人工按 F5 的实测（T27/T28），
> 验证的是**编辑器链路**（扩展、launch.json、DAP 接线）—— 该门禁已于 **2026-10-02 通过**。
> 本脚本验证的是**它下面那一层**（调试信息、断点解析、`ptrace`、平台包二进制、clippy 门禁）。
> 两者分开，出问题时能直接定位是环境层还是编辑器层；
> **这也是环境复发时的定位手段**：先跑本脚本 —— 它红了就是环境层，它绿而按 F5 仍断不住才是编辑器层。

### 附带确认的环境事实

- 容器的能力配置属实（`docker inspect`）：`CapAdd=["CAP_SYS_PTRACE"]`、`SecurityOpt=["seccomp=unconfined"]`、
  `Privileged=false` —— 与 `devcontainer.json` 的 `runArgs` 一致。
  注意：容器内以非 root 用户 `dev` 运行时 `/proc/self/status` 的 `CapEff` **恒为 0**，
  不能据此判断容器能力，必须从宿主侧 `docker inspect` 看。
- 调试器实际使用的是平台包**自带**的 `lldb 22.1.8-codelldb`，不是镜像里的系统 `lldb 14.0.6`。
  直接调 lldb CLI 时会看到 `warning: This version of LLDB has no plugin for the language "rust"` ——
  扩展的适配器会先 `command script import <扩展目录>/lang_support/rust.py` 注册 Rust 类型格式化器，
  所以 VSCode 里不会有这个警告。手工复现无头会话时若要看 Rust 类型美化，需要自己 import。

### 一个易踩的坑：注释里也写着那段代码

`debug-smoke-test.sh` 第一版用 `grep -n 'let sum = a + b;'` 推导断点行，结果命中了**注释行**
（源码里为说明「为什么不是这个形状」而写了这串字符），推出 `lib.rs:13` →
断点变成 `no locations (pending)`，无头会话静默跑完：9 项里 8 项失败，但进程退出码是 0。
现改用 `grep -nE '^[[:space:]]*let sum = a \+ b;[[:space:]]*$'` 锚定「行首只有空白的代码行」。

## 9. 案例：VS Code Server 在容器外「无法下载」（与 §7 报错文字相同、层级不同）

### 先做层级辨异，再动手

§7 与本节**报错文字完全一样**：

```
Failed to establish a socket connection to proxies: PROXY 127.0.0.1:7897
```

但它们是**三个不同层级的下载**中不同的两层，修法也不同。**唯一可靠的判据是日志里的 `Path:`**：

| 层级 | 谁来下载 | 日志特征 | 修法 |
|---|---|---|---|
| ① **VS Code Server** | Dev Containers，在**宿主**侧、连接容器**之前** | `Installing VS Code Server for commit <sha>` + **`Path: /var/folders/…`**（macOS 宿主临时目录） | `scripts/install-vscode-server.sh` |
| ② 扩展**本体** | VSCode，走 marketplace | 扩展面板显示装不上 | 一般不受影响（不走 github） |
| ③ 扩展**平台包** | 扩展自己，走 GitHub releases | `<扩展目录>` 里没有 `platform.ok` | `scripts/install-codelldb.sh`（§7） |

> 本轮实际发生的**不是** CodeLLDB 复发。看到相同报错时，**先看 `Path:` 在不在宿主临时目录**，
> 否则会在错误的层里反复折腾。

### 现象

宿主 VSCode 升级到 **1.140.0** 后，`Reopen in Container` 卡在：

```
Installing VS Code Server for commit 07f806f999227108933c2e30515b26eecc1fda74
URL:  https://update.code.visualstudio.com/commit:07f806f9…/server-linux-arm64/stable
Path: /var/folders/w2/…/serverCache/07f806f9…/vscode-server-linux-arm64.tar.gz
Failed to establish a socket connection to proxies: PROXY 127.0.0.1:7897
Retrying to download VS Code Server.
TypeError: Failed to fetch
```

`Path:` 指向**宿主的** `serverCache` —— 这一条就把它和 §7 区分开了：**下载根本没进容器**。

### 因果链（逐条实测）

| # | 环节 | 证据 |
|---|---|---|
| 1 | 宿主 VSCode 的 commit **变了** | `product.json`：`1.140.0` / `07f806f999227108933c2e30515b26eecc1fda74`；容器里原来只有 `04c0d99f…`（对应 1.139.1）与 `110a328e…` |
| 2 | 该 commit 的 server 在**两处判据**里都不存在 → 触发下载 | `test -d ~/.vscode-server/bin/07f806f9…` 与 `test -d /vscode/vscode-server/bin/linux-arm64/07f806f9…` 均非 0 |
| 3 | 宿主那个代理**已经不在跑了** | 三重确认：`nc -z 127.0.0.1 7897` → exit 1；`curl -x http://127.0.0.1:7897` → `Couldn't connect to server`；`lsof -iTCP:7897 -sTCP:LISTEN` → **空** |
| 4 | 宿主 User settings 里 `http.proxy` **仍指着它** | 该设置的来源与下发机制见 §7 第 5 条。**VSCode 不会对死代理回退直连** —— 这就是失败的直接原因 |
| 5 | 绕过代理后**直连完全可用** | `curl --noproxy '*' https://update.code.visualstudio.com/api/releases/stable` → **HTTP 200 / 0.39 s** |
| 6 | 容器直连下载大文件也很快 | 见下方实测 |

### 持久卷布局：为什么装进 `/vscode` 就能永久生效

Dev Containers 在连接前按顺序 `test -d` **两处**，**都缺才去下载**：

```
①  ~/.vscode-server/bin/<commit>                        （符号链接 → 指向 ②）
②  /vscode/vscode-server/bin/linux-<arch>/<commit>      （实体，在持久命名卷 vscode 里）
```

实测（容器 `cb319b99f648`）：

```
$ ls -1 /vscode/vscode-server/bin/linux-arm64/
04c0d99f4fb0d8afe6ce4f0c58e31e183ac3e4b1     ← 1.139.1
07f806f999227108933c2e30515b26eecc1fda74     ← 1.140.0（本轮装入）
110a328ea54b42367b803ec53ee0bf52ef26b419

$ ls -la ~/.vscode-server/bin/
04c0d99f… -> /vscode/vscode-server/bin/linux-arm64/04c0d99f…
07f806f9… -> /vscode/vscode-server/bin/linux-arm64/07f806f9…
```

**关键**：`/vscode` 是 Dev Containers 自动创建的**持久命名卷**，不随容器重建丢失 —— 所以把它塞对位置，
之后每次开容器都直接命中，**不再发起任何下载**；而且**全程不改宿主配置**（不需要让用户去动那个死代理）。

### 实测数据

同一个 tarball 测了两次（首次安装 20.4 s，复核 23.2 s）：

```
HTTP=200  大小=214058758B  用时=23.178163s  速度=9235363B/s     ← 204 MB / 9.2 MB/s
HTTP=200  大小=214058758B  用时=20.368187s  速度=10509465B/s    ← 204 MB / 10.5 MB/s
sha256: 91cf7a8598d18594c2ddff13d70f27f024d79f2a8c23d2d4ab4d6a4344c072c6   ← 两次逐字节一致
tarball 顶层目录：vscode-server-linux-arm64/
```

装入后（解压 + `sudo cp -a` 到卷内 + 补符号链接）：

```
$ du -sh /vscode/vscode-server/bin/linux-arm64/07f806f9…
601M
$ <该目录>/bin/code-server --version
1.140.0
07f806f999227108933c2e30515b26eecc1fda74
arm64
$ <该目录>/node --version
v24.21.0
$ <该目录>/out/server-main.js          ← 断言该文件存在
```

两个 `test -d` 均 **exit=0** → Dev Containers 的下载被跳过。

### 修法：`scripts/install-vscode-server.sh`

```sh
# 1) 取宿主当前 VSCode 的 commit（权威且一行）
python3 -c "import json;print(json.load(open('/Applications/Visual Studio Code.app/Contents/Resources/app/product.json'))['commit'])"

# 2) 装进持久卷（幂等；已装则短路，零网络请求）
docker exec <容器名> bash /work/scripts/install-vscode-server.sh --commit <sha>
```

与 `install-codelldb.sh` 同构，要点：

1. **`--noproxy '*'`**：让 curl 完全无视任何代理设置 —— 不依赖「宿主那把代理是死是活」；
2. **写进 `/vscode` 持久卷**（`sudo cp -a`，卷属主是 root，靠镜像里的免密 sudo），再补 `~/.vscode-server/bin/<commit>` 符号链接；
3. **幂等短路**：`<DEST>/bin/code-server` 可执行即直接 exit 0；
4. **断言校验的是语义而非字节**：读 `<DEST>/product.json` 的 `commit` 是否等于请求值 ——
   tarball 字节哈希**不写进脚本**，因为上游可能重打包；而 commit 不匹配才是会让客户端拒绝复用的错误。
   （本节记录的 sha256 只作为「事故现场与本文件是否对得上」的事后比对依据，与 §5 对 image id 的处置一致。）
5. `--commit` 必须是 **40 位小写 hex**，否则 exit 2；无参数同样 exit 2 并给出取法。

> 实测幂等路径输出 `已安装，无需操作。` → exit 0。

### 复发条件（重要）

| 触发 | 会怎样 | 怎么办 |
|---|---|---|
| **升级宿主 VSCode** | commit 变了 → 卷里没有新 server → 又去宿主下载 → **再次被死代理挡住** | 重跑 `install-vscode-server.sh --commit <新commit>`，然后 **Reopen in Container** |
| **容器重建** | `~/.vscode-server/extensions/` **不在任何卷里** → CodeLLDB 平台包丢失（§7） | 重跑 `install-codelldb.sh` |

**这两件事是独立的，会各自复发**；`install-vscode-server.sh` 幂等且不发多余请求，所以在「容器连不上」
这种信息最少的场景下，**先无脑跑它一次**是成本最低的排查起手式。

### 为什么仍然不把 server 烤进镜像

- server 与**宿主 VSCode 的 commit 强绑定**，烤进镜像会在宿主升级后**立刻过期**，反而制造假的安全感；
- 它是**每个 commit 一份**的 601 MB 目录，塞进镜像只服务一次；
- 持久卷的语义正好匹配它的生命周期（跟宿主走、不跟容器走）。

### 一条与 §7 不同的结论：这次不需要碰宿主配置

§7 的修法是「绕开 VSCode 网络栈装平台包」，**根因（宿主死代理）仍在**。
本节同样绕开，但**副作用更小**：server 装进持久卷后，**连 Dev Containers 自己都不再走网络**，
所以那个死代理对本工作流的这一环**彻底失效**。仍然只有用户能改宿主那把 `http.proxy`（本仓不做、也不建议代改），
但**已经不影响本仓的开发与调试**。

### 排查命令备忘

```sh
# 宿主：VSCode 的 commit
python3 -c "import json;print(json.load(open('/Applications/Visual Studio Code.app/Contents/Resources/app/product.json'))['commit'])"
# 宿主：代理是否真的活着（三重确认，任一为否就别信它）
nc -z 127.0.0.1 7897 ; curl -x http://127.0.0.1:7897 --max-time 5 https://example.com ; lsof -iTCP:7897 -sTCP:LISTEN
# 宿主：绕过代理的直连是否可用
curl -sS --noproxy '*' -o /dev/null -w '%{http_code}\n' https://update.code.visualstudio.com/api/releases/stable
# 容器：两处判据
docker exec <容器名> test -d /vscode/vscode-server/bin/linux-arm64/<commit> && echo OK
# 宿主：Dev Containers 日志里上一次请求的 commit
grep -rhoE 'Installing VS Code Server for commit [0-9a-f]{40}' \
  "$HOME/Library/Application Support/Code/logs"/*/window*/exthost*/ 2>/dev/null | tail -1
```

## 10. 案例：F5 报「Cargo command did not complete successfully.」

### 现象

容器**已经能进**（§9 修好之后），但按 F5 调试时报：

```
Cargo command did not complete successfully.
```
并提示打开 `launch.json`。

**真正的错误不在弹窗里**，而在 CodeLLDB 自己的输出通道：

```
$ cat ~/.vscode-server/data/logs/<会话>/exthost1/output_logging_*/2-LLDB.log
Initial debug configuration: { ... cwd: '${workspaceFolder}/rgoc', ... }
Running: cargo test --no-run --package=rgoc-harness --lib --message-format=json \
         --color=always --config=target.'cfg(all())'.runner=["…/codelldb-launch"]
Cargo exited with code 101
```

> 文件名序号会递增（`1-LLDB.log` / `2-LLDB.log` / …），别只看第一个。
> CodeLLDB 把 cargo 的 **stdout 当 JSON 解析**，stderr 并不落进这个日志 ——
> 所以「真正的错误文本」得自己复现才能看到（见下）。

### 定位过程：三次复现，逐层排除

| # | 假设 | 做法 | 结果 |
|---|---|---|---|
| 1 | PATH 里没有 cargo | `docker exec … bash -c 'command -v cargo'` | **排除** —— `/home/dev/.cargo/bin/cargo` 到处都在；扩展宿主（`/proc/<extHostPid>/environ`）的 PATH 也含它 |
| 2 | 那条命令行本身有问题（`--config=target.'cfg(all())'.runner=[…]` 的 TOML） | 用 shell 复现 | 得 `TOML parse error … invalid unquoted key` / exit 101，**但这是假线索** —— shell 会剥掉单引号 |
| 3 | 同上，但**无 shell**（CodeLLDB 是 `spawn(cargo, args, {…})`，没有 `shell: true`） | 用 Python `subprocess` 传 argv 列表，并注入**扩展宿主进程的真实 env** | **exit 0** —— 命令行、env 都没问题 |

三次都排除后，剩下的只有 **cwd**。实测：

```
cwd = /work/rgoc → exit 0
cwd = /work      → exit 101   error: could not find `Cargo.toml` in `/work` or any parent directory
```

与日志的 `Cargo exited with code 101` **逐字对上**。

### 根因：CodeLLDB 的工作目录取自 `cargo.cwd`，**不读顶层的 `cwd`**

```
getCargoCwd(e) { return e ?? this.workspaceFolder?.uri?.fsPath }   // 无 path.resolve
…
this.runCargoAndGetArtifacts(l, d, r.cwd, t)                       // r === e.cargo
…
c.spawn(cargo, args, { stdio:["ignore","pipe","pipe"], cwd, env })
```

`launch.json` 把 `cwd` 写在**顶层**，而 `cargo` 里没有 —— 于是回退成
`workspaceFolder` = **`/work`**，而 `/work` 下没有 `Cargo.toml`（cargo 工程根是 `/work/rgoc`）。

> **为什么容易误判**：顶层 `cwd: "${workspaceFolder}/rgoc"` 看起来完全正确、
> 也是 VSCode 文档里的常规写法；而日志里也照原样打印了它。
> 但它**只影响调试目标，不影响 cargo 那一步**。

### 第二个坑：`filter.name` 要比 **target name**，不是包名

把 cwd 修好后**立刻会撞上**这一个 —— 所以在同一次里一起修掉了（否则又要一轮返工）：

```
产物入列： { fileName: e.executable, name: e.target.name, kind: e.target.kind[0] }
再过滤：   e.filter(e => !(t.name != null && e.name != t.name || t.kind != null && e.kind != t.kind))
```

本包包名是 `rgoc-harness`（连字符），而 **cargo 的 `target.name` 是 `rgoc_harness`（下划线）**。
实测（在真实 cargo 产物上复刻上述筛选逻辑）：

```
Raw artifacts:
   {'fileName': '/work/rgoc/target/debug/deps/rgoc_harness-62ebc75611d6c22a',
    'name': 'rgoc_harness', 'kind': 'lib'}

filter = {'name': 'rgoc-harness', 'kind': 'lib'}  → ✗ 0 个匹配
        → "Cargo has produced no matching compilation artifacts."
filter = {'name': 'rgoc_harness', 'kind': 'lib'}  → OK
```

规则：crate 未写 `[lib] name` 时，target name = **包名把 `-` 换成 `_`**。

### 修法（`.vscode/launch.json`，两个配置都要）

```jsonc
"cargo": {
  "args": ["test", "--no-run", "--package=rgoc-harness", "--lib"],
  "filter": { "name": "rgoc_harness", "kind": "lib" },   // ← target name（下划线）
  "cwd": "${workspaceFolder}/rgoc"                        // ← 必需；顶层 cwd 无效
},
"cwd": "${workspaceFolder}/rgoc"                          // 只影响调试目标，保留
```

`${workspaceFolder}` **可以**用在 `cargo.cwd` 里：cargo 的解析发生在
`resolveDebugConfigurationWithSubstitutedVariables`（**变量已替换**之后），
证据是扩展源码里 `resolveCargoConfig` 的唯一调用点就在该 hook 内。

### 为什么不能靠「打开的文件」自动定位

CodeLLDB 有 `discoverDebugConfigurations`（按 `Cargo.toml` 生成配置）的能力，
但那是**命令行 `--config` / 自动生成**那条路；手写 `launch.json` 时它只认 `cargo.cwd`。
本仓选**手写**（配置可见、可回归），代价就是必须自己写对这两个字段 ——
现由 `scripts/check-m0-consistency.py` 第 1 节守着（4 条断言）。

### 回归：这条链路怎么被自动化守住

两个坑都属于**静默**陷阱 —— 写错了 `launch.json` 依然合法、编辑器不报错，只在按 F5
的那一刻变成一句没指向性的提示。所以不能靠「读一遍文件觉得对」，要让它**真的跑一次**。
为此把 CodeLLDB 的那一步**按 `.vscode/launch.json` 原样复刻**进冒烟测试，静态与动态两条线互补：

| 层 | 脚本 | 管什么 | 数量 |
|---|---|---|---|
| 静态 | `check-m0-consistency.py` 第 1 节 | 字段在不在、`filter.name` 是否等于从 `--package=` 解析出的 target name、`cargo.cwd` 下是否真有 `Cargo.toml` | 4 条断言 |
| 静态 | `check-m0-consistency.py` 第 5c 节 | 第 2 节**确实是在复刻**（读真文件 / 同序替换变量 / 不经 shell / 复刻 filter 筛选） | 6 条断言 |
| 动态 | `debug-smoke-test.sh` 第 2 节 | 按 `launch.json` 跑一次真 cargo：A) cwd 来自 `cargo.cwd`　B) 退出码 0　C) `filter` 恰好选中 1 个产物 | 3 项 + 9 项 lldb 断言 |

第 2 节的复刻有三处**必须照抄**，每一处都对应一次真实踩坑：

1. **读 `launch.json`，不抄参数** —— 抄一份就永远测不到漂移。它做与 CodeLLDB 同样的
   `${workspaceFolder}` 替换（顺序也一致：cargo 解析发生在变量替换**之后**）。
2. **不经 shell 调用 cargo**（argv 列表 / `subprocess.run(argv, …)`）—— 本案例第一次
   定位时就是被这一条骗了：shell 会剥掉 `target.'cfg(all())'` 的单引号，给出一个
   **假的** TOML 报错，而退出码恰好也是 101，看起来「完美复现」。
3. **逐行复刻产物筛选**（`e.target.name` 入列、再按 `filter.name` / `filter.kind` 过滤）——
   否则「0 匹配」这个失败模式不会被复现，而它正是第二个坑的全部内容。

**这些静态断言本身是没有被反向校验的**（2026-10-02 起）：原先有一个变异测试脚本
（`scripts/mutation-test-debug-smoke.sh`，往**真实** `launch.json` 注入上面两个坑、
要求第 2 节按预期原因报 ✗、跑完靠 `trap` + sha256 还原；它已验证 4/4 变异被抓住），
但按用户要求随另外几个脚本一起移除以精简交付物，快照在
`.workbuddy/backup/scripts-removed-20261002-1504/`（不入库），拷回 `scripts/` 即可重跑。
**改动上面这些断言时，反向验证要人工做一遍**（把目标改成注释形态或删掉，确认断言报 ✗）。

### 排查命令备忘

```sh
# 1) 看 CodeLLDB 实际执行了什么、退出码多少（序号会递增，取最新的那个）
ls -1t ~/.vscode-server/data/logs/*/exthost1/output_logging_*/[0-9]*-LLDB.log | head -1
# 2) 复现那条命令（【不要】用 shell —— shell 会剥掉 target.'cfg(all())' 的单引号，
#    制造一个假的 TOML 报错）
docker exec -i -u dev -w /work/rgoc <容器名> python3 - <<'PY'
import json, subprocess
runner = "/home/dev/.vscode-server/extensions/vadimcn.vscode-lldb-1.12.3/bin/codelldb-launch"
cfg = "--config=target.'cfg(all())'.runner=[%s]" % json.dumps(runner)
cmd = ["cargo","test","--no-run","--package=rgoc-harness","--lib",
       "--message-format=json","--color=always",cfg]
p = subprocess.run(cmd, capture_output=True, text=True)
print("exit:", p.returncode); print(p.stderr[-3000:])
PY
# 3) 只想知道「cwd 对不对」时，最快的一刀：
docker exec -u dev -w /work/rgoc <容器名> cargo metadata --no-deps --format-version=1 >/dev/null && echo "cwd OK"
```

---

## 11. T30 复核：20 个官方样本的期望值（oracle 侧，`go1.27.1` linux/arm64）

> **为什么有这一节**：`M0-tests.md` §4.2 写明「期望值由**宿主 `go1.24.5`** 实测（2026-10-02），
> M0 须在容器内用 **`go1.27.1`** 复核后锁定；两者不一致时**以容器内为准**并记录差异」。
> 本节就是那次复核的原始记录（T30，2026-10-02）。

**复核方法**（一次性探针，逐条复刻官方 `testdir_test.go`）：

| 复刻对象 | 出处 | 作用 |
|---|---|---|
| `runcmd` —— **stdout 与 stderr 合并** | `:642-647` | 决定 `run` 层比的是哪个流 |
| `splitOutput`（R3） | `:1195-1213` | 诊断切分（Tab 续行拼接、跳过 `go tool` / `#` / `<autogenerated>`） |
| `wantedErrors`（R4） | `:1440-1499` | 解析 `// ERROR "…"`（多引号 / `LINE±n` / `////` 禁用） |
| `errorCheck` 匹配算法 | `:1249-1304` | 按 `<short>:<line>` 取诊断 → 砍位置前缀 → 未锚定正则 |
| `replacePrefix` | `:1236-1241` + `:2059-2072` | 诊断里的**全路径**换成**短名**（含续行） |
| 三条命令形态（R6） | `:188` / `:787-790` / `:1069-1085` | `go tool compile` 而**不是** `go build` |
| R5：自动加 `-d=ssa/check/on` | `:613-625` | errorcheck 层 |

**复核结果**：

oracle = go version go1.27.1 linux/arm64
语料    = /work/go_source_code/test

| ID | 样本 | 模式 | 文档写的期望 | go1.27.1 实测 | 判定 | 耗时 |
|---|---|---|---|---|---|---|
| T-C-01 | `helloworld.go` | run | stdout+stderr = "hello, world\n"（来自 helloworld.out.out） | stdout = "hello, world\n", exit = 0 | ✅ 一致 | 0.0s |
| T-C-02 | `closure1.go` | run | stdout = ""（无 .out → R2：期望为空） | stdout = "", exit = 0 | ✅ 一致 | 0.0s |
| T-C-03 | `gc1.go` | run | stdout = ""（无 .out → R2：期望为空） | stdout = "", exit = 0 | ✅ 一致 | 0.0s |
| T-C-04 | `printbig.go` | run | stdout+stderr = "-9223372036854775808\n9223372036854775807\n"（来自 printbig.out.out） | stdout = "-9223372036854775808\n9223372036854775807\n", exit = 0 | ✅ 一致 | 0.0s |
| T-C-05 | `closure4.go` | run | stdout = ""（无 .out → R2：期望为空） | stdout = "", exit = 0 | ✅ 一致 | 0.0s |
| T-C-06 | `func6.go` | run | stdout = ""（无 .out → R2：期望为空） | stdout = "", exit = 0 | ✅ 一致 | 0.0s |
| T-C-07 | `compos.go` | run | stdout = ""（无 .out → R2：期望为空） | stdout = "", exit = 0 | ✅ 一致 | 0.0s |
| T-C-08 | `method3.go` | run | stdout = ""（无 .out → R2：期望为空） | stdout = "", exit = 0 | ✅ 一致 | 0.0s |
| T-C-09 | `eof.go` | compile | 编译成功（exit 0、无诊断） | exit = 0, 诊断 0 条 | ✅ 一致 | 0.0s |
| T-C-10 | `empty.go` | compile | 编译成功（exit 0、无诊断） | exit = 0, 诊断 0 条 | ✅ 一致 | 0.0s |
| T-C-11 | `parentype.go` | compile | 编译成功（exit 0、无诊断） | exit = 0, 诊断 0 条 | ✅ 一致 | 0.0s |
| T-C-12 | `rune.go` | compile | 编译成功（exit 0、无诊断） | exit = 0, 诊断 0 条 | ✅ 一致 | 0.0s |
| T-C-13 | `initloop.go` | errorcheck | ERROR 期望 1 条全部匹配 | exit = 2, 诊断 1 条 | ✅ 一致 | 0.0s |
| T-C-14 | `recover5.go` | errorcheck | ERROR 期望 2 条全部匹配 | exit = 2, 诊断 2 条 | ✅ 一致 | 0.0s |
| T-C-15 | `varerr.go` | errorcheck | ERROR 期望 2 条全部匹配 | exit = 2, 诊断 2 条 | ✅ 一致 | 0.0s |
| T-C-16 | `convlit1.go` | errorcheck | ERROR 期望 3 条全部匹配 | exit = 2, 诊断 3 条 | ✅ 一致 | 0.0s |
| T-C-17 | `init.go` | errorcheck | ERROR 期望 3 条全部匹配 | exit = 2, 诊断 3 条 | ✅ 一致 | 0.0s |
| T-C-18 | `switch4.go` | errorcheck | ERROR 期望 1 条全部匹配 | exit = 2, 诊断 1 条 | ✅ 一致 | 0.0s |
| T-C-19 | `typecheck.go` | errorcheck | ERROR 期望 4 条全部匹配 | exit = 2, 诊断 5 条 | ✅ 一致 | 0.0s |
| T-C-20 | `mainsig.go` | errorcheck | ERROR 期望 4 条全部匹配 | exit = 2, 诊断 5 条 | ❌ 不一致 | 0.0s |
| | | | | | ↳ 文档写 ERROR 条数=4，源码里解析出 5 条 | |

合计：19/20 一致
总耗时：0.2s（冻结预算：整层 T-C ≤ 5 min，单项 ≤ 60 s）

**结论：20 个样本中 19 个与文档一致；1 个是【文档错】，已改文档（不是改样本）。**

### 11.1 唯一的差异：T-C-20 `mainsig.go` 的 ERROR 条数

| 项 | 值 |
|---|---|
| 文档原写 | 4 条 |
| 源码实测 | **5 条**（L9 一条、L10 同行两条、L12 一条、L13 一条） |
| 处置 | **改文档**：`M0-tests.md` §4.4 表与期望原文块都已补上第 9 行，条数改为 5 |

漏记的是第 9 行 `func main(int) {}` 那条 `// ERROR`。**样本本身没问题** ——
5 条期望全部匹配上、诊断无剩余，按官方算法就是 5 条。
这正是 T30 存在的意义：**期望值错了，harness 的比较器就会被喂一个错的 oracle，20/20 通过也没有意义。**

### 11.2 复核过程中查出、并已回写进 `M0-tests.md` 的两处规则缺失

1. **R2b：比较的是 `stdout` + `stderr` 的合并流**（`:642-647`）。
   `helloworld.go` / `printbig.go` 用内建 `print`（写 **stderr**），
   `.out` 记的正是它 —— **只捕 stdout 会让这两个样本「输出为空」而误判失败**。
   探针第一次跑就是这样：8 个 `run` 样本里 2 个红，原因是 stdout 为空。
2. **R6：三层各自的命令形态**（`run` / `compile` / `errorcheck`）。
   最关键的一条：**`errorcheck` 用 `go tool compile`，不是 `go build`** ——
   所以 `-d=ssa/check/on`（R5）才是合法参数，`go build -d=…` 直接报
   `flag provided but not defined: -d`。探针第一次用 `go build`，8 个 errorcheck 样本全红。
   另需注意 `-C` 关掉首行列号、诊断路径要先 `replacePrefix` 成短名（**含续行**）。

### 11.3 与宿主 `go1.24.5` 的差异

**未发现差异**：20 个样本的期望在 `go1.24.5`（文档原值）与 `go1.27.1`（本次实测）之间**完全相同**。
所以 §4.2 那句「以容器内为准」在本轮**没有触发任何改写**（除了 11.1 那条本来就写错的条数）。

### 11.4 耗时（对照冻结预算）

| 项 | 冻结预算 | 实测 |
|---|---|---|
| T-C 单项 | ≤ 60 s | 全部 < 0.1 s |
| T-C 整层 | ≤ 5 min | **0.2 s**（20 个样本） |

> 注：这是「只做类型检查 / 编译」的耗时。真实 harness 还要跑 `link`（`run` 层的 fast path）
> 与被测程序的**运行**，T38 才是完整基线。
