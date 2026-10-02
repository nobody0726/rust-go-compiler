# M0 实施计划（M0-plan.md）

> **阶段 ID**：`M0`　|　**状态**：待执行　|　**日期**：2026-10-02
> **文档索引**：[`../README.md`](../README.md)　|　**上游**：[`M0-design.md`](./M0-design.md)（设计）、[`M0-tests.md`](./M0-tests.md)（验收）
> **方法论**：Superpowers `writing-plans` —— 每个任务 **2–5 分钟**可完成，含精确文件路径、完整可粘贴内容、可判定的验证方式。

---

## 0. 本计划的覆盖范围

### 0.1 本计划只拆 **Phase 0 与 Phase 1**（环境两阶段）

**这不是遗漏，是刻意为之**，理由有三条：

| # | 理由 |
|---|---|
| 1 | **writing-plans 要求「完整可粘贴的代码、无歧义」**。Phase 2 的 harness 代码结构依赖 Phase 1 实际建成的 workspace 骨架与真实工具链行为（`clang` 版本、rustup 行为、`cargo` 缓存布局）—— 现在硬写出来就是编造，会被执行者照抄成错。 |
| 2 | **D-M0-2「严格环境优先」** —— Phase 0/1 是环境的全部；**环境门禁未过，Phase 2+ 不得开工**。提前拆出无法执行的任务没有价值。 |
| 3 | `../03-roadmap.md` §8.2 —— **不提前拆完**；Superpowers 亦规定「计划变了就回头改计划」，而环境实测结果必然影响后续计划。 |

**Phase 2–4 的计划在 Phase 1 门禁（E5）通过后，基于实测到的环境事实再拆。**

### 0.2 一处 Phase 边界的调整（需注意）

`M0-design.md` §6.2 把「VSCode 调试环境」列为 Phase 1，其门禁 E5 要求在**某段 Rust 代码**上实测断点命中。但按原划分，Rust 工程要到 Phase 2 才建 —— 那就没有调试目标。

**因此本计划把「最小 Rust 工程骨架」提前到 Phase 1**（T20–T22）：

- 只建 `rgoc/` workspace + **一个** `rgoc-harness` crate + **一个**测试；
- 它是调试目标，也是 Phase 2 的起点；
- Phase 2 在此基础上扩展成完整 harness（Test IR / 指令解析 / 比较器）。

这符合 `M0-design.md` §6.3「只创建当期需要的模块」的原则 —— 当期（Phase 1）确实需要它。

### 0.3 任务编号与门禁对应

| 任务范围 | 对应 Phase | 对应门禁 |
|---|---|---|
| T01–T07 | Phase 0 · 镜像构建 | **E1** 镜像 digest 可重放 |
| T08–T11 | Phase 0 · 工具链验证 | **E2** 环境值入 manifest |
| T12–T16 | Phase 0 · 入口与 manifest | **E2** |
| T17–T19 | Phase 0 · 基准测试 | **E10** |
| T20–T22 | Phase 1 · 最小 Rust 工程 | — |
| T23–T27 | Phase 1 · devcontainer | — |
| T28 | Phase 1 · **实测断点命中** | **E5** |

---

## 1. 执行前提

开始前确认（任一条不满足即停止）：

```sh
# 1) Docker daemon 在运行
docker info --format '{{.ServerVersion}} {{.Architecture}}'
#    期望：29.6.2 aarch64

# 2) 当前目录是仓库根
pwd && ls AGENTS.md corpus-manifest.sha256
#    期望：/Users/wangfeng/workspace/rust_go_compiler

# 3) 语料完整（只读校验）
shasum -a 256 -c corpus-manifest.sha256 | grep -v ': OK$'
#    期望：无输出

# 4) 工作区干净（便于出问题时回退）
git status --short
```

> **注意**：本轮之前的文档改动尚未提交。**建议先提交一次**再开始 Phase 0，使每个任务失败时都能干净回退。

---

## 2. Phase 0 —— 容器与工具链底座

### 任务 T01：写 `.dockerignore`，把无关内容挡在构建上下文外

- **文件路径**：`.dockerignore`（新建，仓库根）

- **要做的**：写入以下内容（逐字粘贴）

```gitignore
# 构建镜像只需要 Dockerfile 与 rust-toolchain.toml，其余全部排除。
# 目的：缩小构建上下文、加快构建、避免把语料与凭据送进镜像层。

.git
.gitignore
.workbuddy
go_source_code
corpus-manifest.sha256
docs
AGENTS.md
rgoc/target
**/target
**/.DS_Store
**/._*
```

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
wc -l .dockerignore          # 期望：15 行左右
```

---

### 任务 T02：写 `rust-toolchain.toml`（Rust 版本的唯一来源）

- **文件路径**：`rust-toolchain.toml`（新建，仓库根）

- **要做的**：写入以下内容。这是 **D-M0-8** 的落地 —— Dockerfile 不再重复写版本号，`rustup` 读本文件自动安装/切换。

```toml
# Rust 工具链钉版（决策 D-M0-8，见 docs/milestones/M0-design.md §3.3）。
#
# 本文件是 Rust 版本的【唯一来源】：Dockerfile 通过 `rustup show` 触发安装，
# 本地开发与门禁容器都读取同一个版本，不允许在别处硬编码版本号。
#
# 版本选择依据：Go oracle 是硬约束（必须精确 go1.27.1），Rust 是软约束，
# 取当时 stable 即可。1.98.1 发布于 2026-09-03。

[toolchain]
channel = "1.98.1"
profile = "minimal"
components = ["rustfmt", "clippy"]
```

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
python3 -c "import tomllib,pathlib; print(tomllib.loads(pathlib.Path('rust-toolchain.toml').read_text())['toolchain'])"
# 期望：{'channel': '1.98.1', 'profile': 'minimal', 'components': ['rustfmt', 'clippy']}
```

---

### 任务 T03：创建 `docker/Dockerfile` 骨架（钉 digest + 架构断言）

- **文件路径**：`docker/Dockerfile`（新建；同时创建 `docker/` 目录）

- **要做的**：写入以下内容

```dockerfile
# syntax=docker/dockerfile:1

# ─────────────────────────────────────────────────────────────────────────────
# rgoc 的【唯一】镜像定义。开发容器与门禁容器共用同一个镜像（决策 D-M0-10）。
# 设计依据：docs/milestones/M0-design.md §3.4
#
# 基础镜像按 index digest 钉死（决策 D-M0-7）。tag 仅为可读性注释，不参与解析。
#   tag      golang:1.27.1-bookworm
#   index    sha256:69a7b9788769bec032d238959b61854e9ae87f57be9029ec04e9885fabf99195
#   arm64    sha256:1668bbf856490c9bd0b77204ae1cdaebe273dc459b5eeb3a31a4ddc78ca6fae1
#   上游     docker-library/golang 1.27/bookworm @ c4664da1bd8d0cbc975460744d90c031b409c215
#   构建于   2026-09-19T02:16:26Z
#
# 为什么是 golang 镜像而不是 rust 镜像：
#   Go oracle 必须精确匹配语料版本 go1.27.1 —— 这是【硬约束】；
#   Rust 版本是【软约束】。把硬约束交给镜像，把软约束交给 rustup。
# ─────────────────────────────────────────────────────────────────────────────
FROM golang:1.27.1-bookworm@sha256:69a7b9788769bec032d238959b61854e9ae87f57be9029ec04e9885fabf99195

# 架构断言：首发平台固定 arm64（03 §0.2）。不是 arm64 就立刻失败，
# 而不是在后续步骤里以奇怪的方式出错。
RUN set -eux; \
    arch="$(dpkg --print-architecture)"; \
    echo "detected arch: ${arch}"; \
    [ "${arch}" = "arm64" ] || { echo "ERROR: 需要 arm64，实际为 ${arch}"; exit 1; }
```

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
docker build -f docker/Dockerfile -t rgoc:dev .
# 期望：构建成功，日志中出现 "detected arch: arm64"
# 首次构建需拉取基础镜像，耗时较长
```

> **疑似风险**：若此处拉取失败，说明网络受限。记录错误原文，不要改为 `golang:latest` 绕过 —— 版本不匹配会让 E2 直接失败。

---

### 任务 T04：加系统包层（`clang` 等 native 与调试能力）

- **文件路径**：`docker/Dockerfile`（追加到文件末尾）

- **要做的**：追加以下内容

```dockerfile
# ── 层 2：native 与调试能力 ───────────────────────────────────────────────────
# clang 是 native spike（P3）的唯一驱动；lldb 是容器内调试备选。
# 层 3 的 rustup 需要 curl 与 ca-certificates。
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        clang \
        llvm \
        lld \
        lldb \
        make \
        pkg-config \
        file \
        curl \
        ca-certificates \
    ; \
    rm -rf /var/lib/apt/lists/*
```

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
docker build -f docker/Dockerfile -t rgoc:dev .
docker run --rm rgoc:dev clang --version | head -2
# 期望：输出 clang 版本行（Debian bookworm 上通常为 clang 14.x）
# 【执行时记录】把实际版本号写入 M0-manifest.json
```

---

### 任务 T05：加非 root 开发用户层（uid/gid 与宿主对齐）

- **文件路径**：`docker/Dockerfile`（追加）

- **要做的**：追加以下内容

```dockerfile
# ── 层 4：非 root 日常开发用户 ────────────────────────────────────────────────
# uid/gid 与宿主对齐，避免 bind mount 的文件属主错乱（04 §3.2）。
# macOS 宿主：uid=501、gid=20。GID 20 在 Debian 上已被 dialout 占用，
# 因此先探测：已存在就复用，不存在才新建。
ARG USER_UID=501
ARG USER_GID=20

RUN set -eux; \
    if getent group "${USER_GID}" >/dev/null 2>&1; then \
        echo "gid ${USER_GID} already exists: $(getent group "${USER_GID}" | cut -d: -f1)"; \
    else \
        groupadd -g "${USER_GID}" dev; \
    fi; \
    useradd -m -u "${USER_UID}" -g "${USER_GID}" -s /bin/bash dev; \
    id dev
```

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
docker build -f docker/Dockerfile -t rgoc:dev .
docker run --rm rgoc:dev id dev
# 期望：uid=501(dev) gid=20(...) groups=20(...)
```

---

### 任务 T06：加 rustup 与钉版工具链层

- **文件路径**：`docker/Dockerfile`（追加）

- **要做的**：追加以下内容。**关键**：版本号不在 Dockerfile 里重复，通过 `rustup show` 读取已 COPY 进来的 `rust-toolchain.toml` 触发安装。

```dockerfile
# ── 层 3：Rust 工具链（版本来源 = 仓库根的 rust-toolchain.toml）────────────────
USER dev
ENV RUSTUP_HOME=/home/dev/.rustup \
    CARGO_HOME=/home/dev/.cargo \
    PATH=/home/dev/.cargo/bin:$PATH

# 只 COPY 版本钉版文件本身，避免让整个源码树进入这一层（缓存友好）。
COPY --chown=dev:dev rust-toolchain.toml /tmp/tb/rust-toolchain.toml

RUN set -eux; \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh; \
    sh /tmp/rustup-init.sh -y --no-modify-path --default-toolchain none --profile minimal; \
    rm -f /tmp/rustup-init.sh; \
    cd /tmp/tb && rustup show; \
    rustc --version; cargo --version; \
    rm -rf /tmp/tb
```

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
docker build -f docker/Dockerfile -t rgoc:dev .
docker run --rm rgoc:dev rustc --version
# 期望：rustc 1.98.1
docker run --rm rgoc:dev cargo --version
# 期望：cargo 1.98.1
```

> **疑似风险**：rustup 需访问 `sh.rustup.rs` 与 `static.rust-lang.org`。若被拦，改用镜像源或预下载 `rustup-init`；**不要**降级为「用镜像内置 Rust」（那会违反 D-M0-8）。

---

### 任务 T07：固化镜像并记录构建元数据（E1）

- **文件路径**：`docker/image.lock`（新建）

- **要做的**：执行构建并记录结果，写入以下模板（值填实际测得的）

```text
# rgoc 镜像锁定信息（E1）
# 由 T07 生成；每次重建镜像都必须更新本文件并说明原因。

base.tag              = golang:1.27.1-bookworm
base.index_digest     = sha256:69a7b9788769bec032d238959b61854e9ae87f57be9029ec04e9885fabf99195
base.arm64_digest     = sha256:1668bbf856490c9bd0b77204ae1cdaebe273dc459b5eeb3a31a4ddc78ca6fae1
base.upstream_revision = c4664da1bd8d0cbc975460744d90c031b409c215

local.image_tag       = rgoc:dev
local.image_digest    = <docker images --digests 测得的 sha256:...>
build.date            = <构建日期>

docker.version        = <docker version --format '{{.Server.Version}}'>
docker.engine_arch    = aarch64
container.kernel      = <docker run --rm rgoc:dev uname -r 测得的>
```

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
docker images --digests rgoc:dev
docker run --rm rgoc:dev uname -r
# 把两个输出中对应的值填进 docker/image.lock，确认没有 <...> 占位符残留
grep -c '<' docker/image.lock    # 期望：0
```

---

### 任务 T08：验证容器内 Go oracle 版本（E2 硬门禁）

- **文件路径**：无文件改动（验证任务）

- **要做的**：这是 **E2 的核心**。版本不符即**拒绝作为基线**，不得继续。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
docker run --rm rgoc:dev sh -c 'go version; which go; go env GOROOT'
# 期望：
#   go version go1.27.1 linux/arm64
#   /usr/local/go/bin/go
#   /usr/local/go
sha256sum $(docker run --rm rgoc:dev which go 2>/dev/null) 2>/dev/null || \
  docker run --rm rgoc:dev sh -c 'sha256sum /usr/local/go/bin/go'
# 期望：得到 Go 二进制的 SHA-256，记录进 M0-manifest.json
```

> **若输出不是 `go1.27.1`：停止，不要继续。** 记录实际版本，回到 T03 检查 digest 是否被改动。

---

### 任务 T09：验证容器内 `clang` 能产出 aarch64 ELF（P3 的前置）

- **文件路径**：无文件改动（验证任务）

- **要做的**：证明 P3 路线在工具链层面成立 —— 这是 `M0-design.md` §9.1 里风险最高的一项的**最早探测点**。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
docker run --rm rgoc:dev sh -c '
  set -eux
  printf "int main(void){return 0;}\n" > /tmp/t.c
  clang -o /tmp/t /tmp/t.c
  file /tmp/t
  /tmp/t
  echo "exit=$?"
'
# 期望：
#   file 输出含 "ELF 64-bit LSB" 且含 "ARM aarch64"
#   程序运行后 echo exit=0
```

- **记录**：`clang` 版本、`ld`(linker) 实际路径 —— 写入 `M0-manifest.json` 的 `toolchain.clang` 与 `toolchain.linker`。

---

### 任务 T10：验证容器内 Rust 工具链完整可用

- **文件路径**：无文件改动（验证任务）

- **要做的**：不只验版本，还要验**能编译并运行**（版本对但工具链残缺的情况是存在的）。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
docker run --rm rgoc:dev sh -c '
  set -eux
  cd /tmp && cargo new probe --bin >/dev/null 2>&1
  cd /tmp/probe
  cargo build --quiet
  ./target/debug/probe
  cargo fmt --check && echo "fmt ok"
  cargo clippy --quiet -- -D warnings && echo "clippy ok"
'
# 期望：Hello, world! / fmt ok / clippy ok
```

> 这一步同时验证了 `components = ["rustfmt", "clippy"]` 真的装上了（`03` §6.3 的统一退出检查依赖它们）。

---

### 任务 T11：验证容器内语料可只读挂载并校验

- **文件路径**：无文件改动（验证任务）

- **要做的**：证明「宿主语料 → 容器内只读」这条链路可用，且清单校验在容器内同样通过。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
docker run --rm \
  -v "$PWD/go_source_code:/go_source_code:ro" \
  -v "$PWD/corpus-manifest.sha256:/corpus-manifest.sha256:ro" \
  -w / \
  rgoc:dev sh -c '
    set -eux
    shasum -a 256 -c /corpus-manifest.sha256 2>/dev/null | grep -v ": OK$" || echo "MANIFEST CLEAN"
    test ! -w /go_source_code && echo "READONLY OK"
  '
# 期望：MANIFEST CLEAN 与 READONLY OK
```

> **注意**：`shasum` 在 Debian 上由 `perl` 提供。若报 command not found，在 T04 的包列表里加 `perl`。

---

### 任务 T12：写统一入口脚本 `scripts/in-container.sh`

- **文件路径**：`scripts/in-container.sh`（新建；同时创建 `scripts/` 目录）

- **要做的**：写入以下内容，然后 `chmod +x`

```bash
#!/usr/bin/env bash
# rgoc 统一容器入口（04 §4.1 / §7）。
#
# 职责：
#   1. 先探测 Docker daemon —— 失败时【明确报错】，绝不静默降级到宿主工具链
#   2. 声明镜像、target、工作目录，使输出可追溯
#   3. 把后续参数原样传给容器内命令
#
# 用法：
#   scripts/in-container.sh <命令> [参数...]
#   scripts/in-container.sh cargo test --workspace
#   scripts/in-container.sh bash          # 交互式 shell
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${RGOC_IMAGE:-rgoc:dev}"

# ── 1. daemon 探测：失败即退出，不降级 ────────────────────────────────────────
if ! docker info >/dev/null 2>&1; then
    cat >&2 <<'EOF'
ERROR: 无法连接 Docker daemon。

本项目的所有构建、测试与 native 验证【必须】在容器内进行，
不会回退到宿主工具链（04 §7）。请先启动 Docker Desktop：

    open -a Docker

等待约 5 秒后重试。
EOF
    exit 1
fi

# ── 2. 声明本次执行的来源，保证结果可追溯 ────────────────────────────────────
{
    echo "──────────────────────────────────────────────────"
    echo "rgoc container run"
    echo "  image   : ${IMAGE}"
    echo "  target  : aarch64-unknown-linux-gnu / ELF"
    echo "  repo    : ${REPO_ROOT}"
    echo "──────────────────────────────────────────────────"
} >&2

# ── 3. 执行 ──────────────────────────────────────────────────────────────────
exec docker run --rm -it \
    -v "${REPO_ROOT}:/work" \
    -w /work \
    -v rgoc-cargo-registry:/home/dev/.cargo/registry \
    -v rgoc-cargo-git:/home/dev/.cargo/git \
    "${IMAGE}" "$@"
```

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
chmod +x scripts/in-container.sh
scripts/in-container.sh rustc --version
# 期望：先看到 "rgoc container run" 头部，然后 rustc 1.98.1
```

> **实际落地的脚本与本处片段有 3 处不同**（`scripts/in-container.sh` 为准，见 §6 变更记录）：
>
> | 差异 | 原因 |
> |---|---|
> | `--rm -it` → `DOCKER_FLAGS=("--rm")`，仅在 TTY 存在时追加 `-it` | 原写法在非 TTY（CI / 后台）下 `[ -t 0 ]` 为假，`"${TTY_ARGS[@]}"` 空数组在 `set -u` 下直接崩：`TTY_ARGS[@]: unbound variable` |
> | 新增 `rgoc-target` 命名卷挂到 `/work/rgoc/target` | T18 实测命名卷快 2.3×（D-M0-9） |
> | 新增 `rgoc-target` 的 bootstrap `chown 501:20` | 新建空卷属主是 root，dev 写入报 `Permission denied` |

---

### 任务 T13：验证 daemon 未运行时的报错行为

- **文件路径**：无文件改动（验证任务）

- **要做的**：这是 `04` §7 风险表「Docker Desktop 未运行 → 易被误判为代码问题」的直接验证。

- **验证**（**不要真的关掉 Docker**，用伪造环境变量即可）：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
DOCKER_HOST=tcp://127.0.0.1:1 scripts/in-container.sh echo hi; echo "exit=$?"
# 期望：
#   打印 "ERROR: 无法连接 Docker daemon." 及提示
#   exit=1（非 0）
#   并且【没有】打印 "hi"
```

---

### 任务 T14：写环境探测脚本 `scripts/env-probe.sh`

- **文件路径**：`scripts/env-probe.sh`（新建）

> **注（2026-10-02，交付前精简）**：该脚本已完成使命（输出已固化进
> `M0-manifest.json` 的 `environment` 节，即 E2 的证据），并按用户要求移除以精简交付物，
> **当前 `scripts/` 下没有这个文件**。脚本体就保存在下面 —— 需要重新采集环境事实
> （例如换了基础镜像、要复核 E2）时，按下文重建后按「验证」跑即可。
> 快照另存于 `.workbuddy/backup/scripts-removed-20261002-1504/`（不入库）。

- **要做的**：写入以下内容，`chmod +x`。它把容器内的环境事实输出为 JSON，供 T15 组装进 manifest。

```bash
#!/usr/bin/env bash
# 在容器内运行，输出环境事实的 JSON 片段（E2）。
# 用法：scripts/in-container.sh bash scripts/env-probe.sh
set -euo pipefail

j() { printf '"%s": %s' "$1" "$2"; }

printf '{\n'
j "arch"           "\"$(dpkg --print-architecture)\"";   printf ',\n'
j "kernel"         "\"$(uname -r)\"";                     printf ',\n'
j "os_release"     "\"$(. /etc/os-release; echo "$PRETTY_NAME")\""; printf ',\n'
j "glibc"          "\"$(ldd --version | head -1 | awk '{print $NF}')\""; printf ',\n'
j "go_version"     "\"$(go version)\"";                   printf ',\n'
j "go_path"        "\"$(which go)\"";                     printf ',\n'
j "go_sha256"      "\"$(sha256sum "$(which go)" | cut -d' ' -f1)\""; printf ',\n'
j "rustc_version"  "\"$(rustc --version)\"";              printf ',\n'
j "cargo_version"  "\"$(cargo --version)\"";              printf ',\n'
j "rustup_show"    "\"$(rustup show active-toolchain 2>/dev/null || echo unknown)\""; printf ',\n'
j "clang_version"  "\"$(clang --version | head -1)\"";    printf ',\n'
j "linker"         "\"$(which ld)\"";                     printf ',\n'
j "llvm_version"   "\"$(llvm-config --version 2>/dev/null || echo n/a)\""; printf ',\n'
j "nproc"          "$(nproc)";                            printf ',\n'
j "mem_total_kb"   "$(awk '/MemTotal/{print $2}' /proc/meminfo)"; printf '\n'
printf '}\n'
```

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
chmod +x scripts/env-probe.sh
scripts/in-container.sh bash scripts/env-probe.sh | python3 -m json.tool
# 期望：合法 JSON，字段齐全，无 "unknown"/"n/a" 之外的占位
```

---

### 任务 T15：组装 `docs/milestones/M0-manifest.json` 的 `environment` 节（E2）

- **文件路径**：`docs/milestones/M0-manifest.json`（新建）

- **要做的**：把 T14 的输出填进下面的骨架，`<...>` 全部替换为实测值

```json
{
  "stage": "M0",
  "generated_at": "<ISO8601>",
  "environment": {
    "host": {
      "os": "macOS 27.2",
      "arch": "arm64",
      "docker_desktop_version": "<docker version 测得>"
    },
    "image": {
      "tag": "rgoc:dev",
      "digest": "<T07 测得>",
      "base_tag": "golang:1.27.1-bookworm",
      "base_index_digest": "sha256:69a7b9788769bec032d238959b61854e9ae87f57be9029ec04e9885fabf99195",
      "base_arm64_digest": "sha256:1668bbf856490c9bd0b77204ae1cdaebe273dc459b5eeb3a31a4ddc78ca6fae1"
    },
    "container": {
      "arch": "arm64",
      "kernel": "<uname -r 测得，当前为 6.12.76-linuxkit>",
      "os_release": "<>",
      "glibc": "<>",
      "nproc": 0,
      "mem_total_kb": 0
    },
    "toolchain": {
      "go": { "version": "go version go1.27.1 linux/arm64", "path": "/usr/local/go/bin/go", "sha256": "<>" },
      "rust": { "rustc": "rustc 1.98.1", "cargo": "cargo 1.98.1", "pinned_by": "rust-toolchain.toml" },
      "clang": { "version": "<>", "linker": "<>" }
    },
    "target": {
      "triple": "aarch64-unknown-linux-gnu",
      "object_format": "ELF"
    }
  },
  "gate": {},
  "test_ids": [],
  "unsupported": [],
  "budget": {},
  "benchmarks": {}
}
```

> `gate` / `test_ids` / `unsupported` / `budget` / `benchmarks` 在 Phase 2–4 填充（`M0-tests.md` §9）。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
python3 -m json.tool docs/milestones/M0-manifest.json >/dev/null && echo "JSON OK"
grep -n '<' docs/milestones/M0-manifest.json || echo "无占位符残留"
```

---

### 任务 T16：校验 manifest 无空缺字段（E2 门禁）

- **文件路径**：无文件改动（验证任务）

- **要做的**：E2 要求「环境实测值全部记录进 manifest，无空缺」。写一个最小校验断言。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
python3 - <<'PY'
import json, sys
m = json.load(open('docs/milestones/M0-manifest.json'))
env = m['environment']

# 占位符只认【整值等于】占位词，不做子串匹配。
# 理由：target.triple 的合法字面量 "aarch64-unknown-linux-gnu" 本身含 "unknown"，
# 子串匹配会把它误判成空缺 —— 这是本计划 v1 的缺陷（见 §6 变更记录）。
PLACEHOLDERS = {'unknown', 'n/a', 'na', 'todo', 'tbd', 'fixme'}

def walk(o, path=''):
    if isinstance(o, dict):
        for k, v in o.items(): yield from walk(v, f'{path}.{k}')
    else:
        yield path, o

def is_bad(v):
    if v in ('', None, 0): return True
    if isinstance(v, str):
        if '<' in v: return True
        if v.strip().lower() in PLACEHOLDERS: return True
    return False

bad = [(p, v) for p, v in walk(env) if is_bad(v)]
print('EMPTY/PLACEHOLDER FIELDS:', bad if bad else 'none')
sys.exit(1 if bad else 0)
PY
# 期望：EMPTY/PLACEHOLDER FIELDS: none，退出码 0
```

> **反向校验**（确认这个检查真的能抓到空缺，不是恒真断言）：临时把 `environment.container.glibc` 改成
> `"unknown"` 或 `"<TBD>"`，脚本应打印该字段并退出码 1；改回后恢复 `none` + 0。

---

### 任务 T17：基准测试 A —— 镜像构建时间与体积（E10）

- **文件路径**：`docs/milestones/M0-benchmarks.md`（新建）

- **要做的**：写入以下表格的**实测数据**（`04` §6 要求）

```markdown
# M0 基准测试记录

> 由 T17–T19 产出，汇总进 `M0-manifest.json` 的 `benchmarks` 节（E10）。

## 1. 镜像

| 指标 | 实测值 |
|---|---|
| 首次构建（含拉取基础镜像） | 1m14s |
| 无缓存全量重建 | 59.05s / 61.32s / 68.44s / 84.28s |
| 二次构建（全缓存命中） | 3.03–3.91s |
| 镜像体积（含全部层） | 2.91GB → 2.92GB |
| 磁盘占用增量 | 2.909GB |

## 2. 容器冷启动

| 指标 | 实测值 |
|---|---|
| `docker run --rm rgoc:dev true` | 0.15–0.16 s |
| `docker run --rm rgoc:dev rustc --version` | 0.17–0.21 s |
```

> **实测结果已写入 `M0-benchmarks.md` §1–§2**（那里是唯一权威副本，含每条数据的来源与口径说明）。
> 本处只保留结论，避免两处数字各自漂移。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
docker images rgoc:dev --format '{{.Size}}'
time (docker build -f docker/Dockerfile -t rgoc:dev . | tail -1)
time (docker run --rm rgoc:dev true)
```

> **计时器注意（实测踩过两次）**：宿主 macOS 的 `/usr/bin/time` 不支持 GNU 的 `-f`
> （报 `illegal option -- f`，**命令整条不执行**）；容器内则**根本没有** `/usr/bin/time`
> （Debian 的 `time` 包未安装）。一律用 `/usr/bin/time -p`（宿主）或 bash 内建
> `time` + `TIMEFORMAT`（容器）。详见 `M0-benchmarks.md` §6。

---

### 任务 T18：基准测试 B —— bind mount vs 命名卷（D-M0-9 的待决点）

- **文件路径**：`docs/milestones/M0-benchmarks.md`（追加）

- **要做的**：这是 `M0-design.md` §3.3 第 4 项「`target/` 放哪」的**决策依据**。分别测两种布局的 `cargo build` 耗时。

```markdown
## 3. bind mount vs 命名卷（决定 target/ 的最终位置，D-M0-9）

同一个 **120 模块 / 1082 行**的 crate，源码都在 bind mount 内，只有 `target/` 落点不同；
每次清空 `target/` 后 `cargo build`，各跑 3 次。

| 布局 | 第 1 次（冷） | 第 2 次 | 第 3 次 | 中位数 | 结论 |
|---|---|---|---|---|---|
| `target/` 在 bind mount | 0.336 s | 0.359 s | 0.287 s | 0.336 s | 慢 2.3× |
| `target/` 在命名卷 | 0.150 s | 0.137 s | 0.133 s | 0.137 s | **采用** |

**决策**：**改用命名卷**（`rgoc-target` → `/work/rgoc/target`），理由：

1. `cargo build` 是开发循环最高频操作，2.3× 会持续复利；
2. 两个代价都可一次性/脚本化消除 —— ① 新卷属主是 root，需 `chown 501:20`
   （`in-container.sh` bootstrap + devcontainer `postCreateCommand`）；
   ② `cargo clean` 在卷挂载点报 `EBUSY(16)`、exit 101，清空改用
   `find rgoc/target -mindepth 1 -delete`；
3. 编辑器与门禁共用同一卷名，满足 D-M0-10「同一环境」。

> 注意：清空手段从 `cargo clean` 改为 `find … -delete` 是**两种布局统一**的，
> 因此不影响上表的可比性。
```

- **验证**：

> **布局要点（本计划 v1 的缺陷，见 §6 变更记录）**：crate 必须建在**bind mount 之内**，
> 两种布局只有 `target/` 的落点不同。若把 crate 建在容器内 `/tmp`，那是 overlayfs，
> 两种布局都测不到宿主文件系统 —— 而这恰恰是 D-M0-9 要问的问题。

```sh
cd /Users/wangfeng/workspace/rust_go_compiler

# ── 0. 造一个「有体量」的 crate：120 个模块，让 target/ 的写入量可比 ──────────
# 注意两个坑：① 目录名不能叫 crate（Rust 关键字，cargo new 会拒绝）
#             ② 容器内没有 /usr/bin/time，用 bash 内建 time + TIMEFORMAT
rm -rf bench-tmp && mkdir -p bench-tmp
docker run --rm -v "$PWD:/work" -w /work rgoc:dev bash -c '
  set -e
  cargo new bench-tmp/probe --bin -q
  cd bench-tmp/probe/src
  N=120
  { seq 0 $((N-1)) | sed "s/^/mod m/;s/$/;/"; printf "fn main() {\n"; seq 0 $((N-1)) | sed "s/^/    m/;s/$/::f();/"; printf "}\n"; } > main.rs
  for i in $(seq 0 $((N-1))); do
    { printf "pub fn f() -> u64 { g() }\n"; printf "fn g() -> u64 { %d }\n" "$i"; seq 1 5 | sed "s/^/\/\/ padding line /"; } > "m$i.rs"
  done
'

# ── 布局 A：target/ 落在 bind mount（宿主 macOS 文件系统） ──────────────────
docker run --rm -v "$PWD:/work" -w /work rgoc:dev bash -c '
  set -e; cd /work/bench-tmp/probe
  TIMEFORMAT="  A 冷构建 #%R s"
  for i in 1 2 3; do find target -mindepth 1 -delete 2>/dev/null || true; time cargo build -q; done
'

# ── 布局 B：target/ 落在命名卷（新卷属主是 root，必须先 chown）─────────────
docker volume create rgoc-bench-target >/dev/null
docker run --rm --user root --entrypoint chown -v rgoc-bench-target:/mnt \
  rgoc:dev 501:20 /mnt
docker run --rm -v "$PWD:/work" -v rgoc-bench-target:/work/bench-tmp/probe/target \
  -w /work rgoc:dev bash -c '
  set -e; cd /work/bench-tmp/probe
  TIMEFORMAT="  B 冷构建 #%R s"
  for i in 1 2 3; do find target -mindepth 1 -delete 2>/dev/null || true; time cargo build -q; done
'

# ── 清理 ───────────────────────────────────────────────────────────────────
# `rm -rf` 在卷挂载点会 EBUSY，用 find 逐个删（与上面清空 target/ 同理）
find bench-tmp -type f -delete 2>/dev/null; find bench-tmp -depth -type d -empty -delete 2>/dev/null
docker volume rm rgoc-bench-target >/dev/null
```

---

### 任务 T19：把 Phase 0 结果登记进 manifest 并跑一次门禁复核

- **文件路径**：`docs/milestones/M0-manifest.json`（更新 `benchmarks` 节）

- **要做的**：把 T17/T18 的数据填入 `benchmarks`，然后逐条核对 E1/E2/E10。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
python3 -m json.tool docs/milestones/M0-manifest.json >/dev/null && echo "JSON OK"
python3 - <<'PY'
import json
m = json.load(open('docs/milestones/M0-manifest.json'))
print('E1 image digest :', m['environment']['image']['digest'] or 'MISSING')
print('E2 env fields   :', len(m['environment']))
print('E10 benchmarks  :', list(m.get('benchmarks', {}).keys()) or 'MISSING')
PY
```

**Phase 0 门禁检查表**（全部 ✅ 才能进入 Phase 1）：

| 门禁 | 判定 | 证据 |
|---|---|---|
| E1 | `docker build` 可从零重放，digest 记录在案 | `docker/image.lock` + manifest |
| E2 | 环境值无空缺、无占位符 | T16 的校验脚本输出 `none` |
| E10 | 构建时间/体积/冷启动/挂载基准齐备 | `M0-benchmarks.md` + manifest |

---

## 3. Phase 1 —— VSCode 调试环境

### 任务 T20：创建 `rgoc/` workspace 骨架

- **文件路径**：`rgoc/Cargo.toml`（新建）

- **要做的**：写入以下内容。**只建当期需要的 crate**（`M0-design.md` §6.3）——Phase 1 只需要 harness 作为调试目标。

```toml
# rgoc —— Rust 版 Go 编译器（项目代号 rgoc）
#
# 本 workspace 采用「只创建当期需要的模块」原则（03 §2）：
# M0 Phase 1 只建 rgoc-harness；其余 crate 按里程碑依赖逐个引入，
# 【不】一次性建立全部空 crate。

[workspace]
resolver = "3"
members = ["crates/rgoc-harness"]

[workspace.package]
version = "0.0.0"
edition = "2024"
rust-version = "1.98"
license = "BSD-3-Clause"

[workspace.lints.rust]
unsafe_code = "forbid"
missing_debug_implementations = "warn"

[workspace.lints.clippy]
all = "warn"
```

> `edition = "2024"` 与 `rust-version = "1.98"` 对应 Rust 1.98.1（2024 edition 自 1.85 起稳定）。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh bash -lc 'cd rgoc && cargo metadata --no-deps --format-version 1 | python3 -m json.tool | head -20'
# 期望：合法 JSON；此时会因缺少 crate 而报错 members 不存在 —— 先做 T21 再验证
```

---

### 任务 T21：创建 `rgoc-harness` crate（调试目标）

- **文件路径**：`rgoc/crates/rgoc-harness/Cargo.toml`、`rgoc/crates/rgoc-harness/src/lib.rs`（新建）

- **要做的**：

`Cargo.toml`：

```toml
[package]
name = "rgoc-harness"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[lints]
workspace = true

[dependencies]
```

`src/lib.rs`：

```rust
//! rgoc 的测试基础设施：Test IR、官方语料驱动、结果比较器。
//!
//! **当前状态**：Phase 1 骨架 —— 只有一个用于验证调试链路的最小函数。
//! 完整实现（Test IR 字段、指令解析、`.out` 比较器）在 Phase 2 按
//! `docs/milestones/M0-tests.md` §1.3 的规则逐条 RED→GREEN 实现。

/// 把两个数相加，再把结果翻倍。
///
/// 这是 Phase 1 的**调试目标**：T28 会在这个函数内下断点，
/// 验证 VSCode + CodeLLDB 的调试链路真的可用（E5）。
/// Phase 2 引入真实功能后可删除。
//
// 【为什么不是 `let sum = a + b; sum`】
//   实测（证据见 docs/milestones/M0-benchmarks.md §8）：
//   rustc 1.98.1 对「直接在尾位置返回的 `let` 绑定」**不生成 DWARF 变量条目** ——
//   `let sum = a + b; sum` 反汇编出的 DWARF 里，`add` 的子节点只有形参 a、b，
//   没有 `DW_TAG_lexical_block` / `DW_TAG_variable sum`，于是 **任何**调试器
//   （lldb CLI、CodeLLDB、gdb）都看不到 `sum`，「单步后观察中间值」这项检查
//   根本无法完成。只要 `sum` 在 `let` 之后被【第二次读取】（这里是被 `sum * 2`
//   读取），rustc 就会为它生成完整条目，调试器即可读到。
//   因此本函数的形状是 E5 可验证性的前提，不是随手写的示例代码。
//
// 【关于 clippy】
//   尾表达式是 `sum * 2` 而非 `sum`，所以不会触发 `clippy::let_and_return`，
//   不需要任何 allow。T22 的 clippy 门禁在无 allow 状态下通过。
pub fn double_sum(a: i64, b: i64) -> i64 {
    let sum = a + b;
    sum * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_sum_works() {
        assert_eq!(double_sum(1, 2), 6);
    }
}
```

> **与 v1 计划的实现差异（2026-10-02 实测修正）**：v1 写的是 `pub fn add(...) -> i64 { let sum = a + b; sum }`
> 并配 `#[allow(clippy::let_and_return)]`。实测该形状下 `sum` **不在 DWARF 里**，
> T28 第 4 项检查无法完成 —— 属计划缺陷。现改为「中间值被第二次读取」的形状，
> allow 一并删除。完整证据（含三变体对照实验）见 `M0-benchmarks.md` §8。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh bash -lc 'cd rgoc && cargo test --workspace 2>&1 | tail -12'
# 期望：test result: ok. 1 passed
```

---

### 任务 T22：确认 Phase 1 的 workspace 门禁命令全过

- **文件路径**：无文件改动（验证任务）

- **要做的**：`03` §6.3 要求的四条统一退出检查，在最小工程上先跑通一次。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh bash -lc '
  set -euo pipefail
  cd rgoc
  cargo fmt --check
  cargo check --workspace --all-targets
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
'
echo "T22 exit=$?"
# 期望：四条全部成功（fmt 无 diff、check/clippy 无警告、test 通过），T22 exit=0
```

> **`set -euo pipefail` 不可省**（本计划 v1 的缺陷，见 §6 变更记录）：
> v1 写的是 `cargo clippy ... | tail -2` 串在 `&&` 链里。管道退出码取自**最后一个命令**
> （`tail`），因此 clippy 失败时整条链仍返回 0，后续步骤照跑，失败被静默吞掉 ——
> 实测正是如此：v1 的命令「跑完了」，但 clippy 其实报错退出。
> 加了 `pipefail` 后，管道中任一命令失败都会向上传播。

> **本任务同时是调试目标形状的回归测试**（原为 `#[allow(clippy::let_and_return)]` 的回归测试，
> 2026-10-02 随调试目标改形状一并更新）：T21 的 `double_sum()` 一旦被改回
> `let sum = a + b; sum`（尾位置直接返回），本任务第 3 条会因 `clippy::let_and_return` 立刻失败 ——
> 那道 lint 的存在恰好是「中间值不进 DWARF」这个坑的信号。详见 `M0-benchmarks.md` §8。

---

### 任务 T23：创建 `.devcontainer/devcontainer.json`（基础形态）

- **文件路径**：`.devcontainer/devcontainer.json`（新建；同时创建 `.devcontainer/` 目录）

- **要做的**：写入以下内容（T24/T25 会再加两项）

```jsonc
// rgoc 的 VSCode 调试环境（M0-design.md §4.2）
//
// 关键点：本文件引用的镜像是 docker/Dockerfile —— 与门禁容器【同一个】镜像，
// 保证「在编辑器里跑通的」与「在门禁里跑的」是同一环境。
{
  "name": "rgoc (aarch64 / Linux)",
  "build": {
    "dockerfile": "../docker/Dockerfile",
    "context": ".."
  },
  "workspaceFolder": "/work",
  "remoteUser": "dev",
  "mounts": [
    "source=${localWorkspaceFolder},target=/work,type=bind,consistency=cached",
    "source=rgoc-cargo-registry,target=/home/dev/.cargo/registry,type=volume",
    "source=rgoc-cargo-git,target=/home/dev/.cargo/git,type=volume"
  ],
  "customizations": {
    "vscode": {
      "extensions": [],
      "settings": {
        "rust-analyzer.cargo.features": "all",
        "terminal.integrated.defaultProfile.linux": "bash"
      }
    }
  }
}
```

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
python3 - <<'PY'
import json, pathlib, re
s = pathlib.Path('.devcontainer/devcontainer.json').read_text()
# 去掉 // 行注释后解析（jsonc）
s = re.sub(r'^\s*//.*$', '', s, flags=re.M)
json.loads(s)
print('devcontainer.json 语法 OK')
PY
```

> **实际落地的文件比本处片段多两项**（`.devcontainer/devcontainer.json` 为准，见 §6 变更记录）：
>
> 1. `mounts` 增加 `source=rgoc-target,target=/work/rgoc/target,type=volume` —— 与
>    `scripts/in-container.sh` 用**同一个卷名**，保证编辑器与门禁同环境（D-M0-10）；
> 2. `postCreateCommand: "sudo chown 501:20 /work/rgoc/target"` —— 新建空卷属主是 root，
>    没有这一步用户第一次 `cargo build` 就报 `Permission denied`。这也是镜像里必须装
>    `sudo` 的唯一原因。

---

### 任务 T24：给 devcontainer 加 `ptrace` 权限（E5 的前提）

- **文件路径**：`.devcontainer/devcontainer.json`（修改）

- **要做的**：在 `build` 与 `workspaceFolder` 之间插入 `runArgs`。**没有这两项，CodeLLDB 必定无法附加，断点必然不生效。**

```jsonc
  // 调试器需要 ptrace 与关闭 seccomp 限制（04 §4.2）。
  // 缺这两项时，断点表现为「被静默跳过」，极易被误判为调试器配置错误。
  "runArgs": [
    "--cap-add=SYS_PTRACE",
    "--security-opt", "seccomp=unconfined"
  ],
```

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
python3 - <<'PY'
import json, pathlib, re
s = pathlib.Path('.devcontainer/devcontainer.json').read_text()
s = re.sub(r'^\s*//.*$', '', s, flags=re.M)
d = json.loads(s)
assert '--cap-add=SYS_PTRACE' in d['runArgs'], d.get('runArgs')
assert 'seccomp=unconfined' in d['runArgs']
print('runArgs OK:', d['runArgs'])
PY
```

---

### 任务 T25：给 devcontainer 加扩展清单（rust-analyzer + CodeLLDB）

- **文件路径**：`.devcontainer/devcontainer.json`（修改）

- **要做的**：把 `customizations.vscode.extensions` 从 `[]` 改为：

```jsonc
      // 宿主【不需要】手动安装这两个扩展 —— Dev Containers 会在容器内自动安装。
      // 宿主只要求已装 ms-vscode-remote.remote-containers（已确认具备）。
      "extensions": [
        "rust-lang.rust-analyzer",
        "vadimcn.vscode-lldb"
      ],
```

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
python3 - <<'PY'
import json, pathlib, re
s = pathlib.Path('.devcontainer/devcontainer.json').read_text()
s = re.sub(r'^\s*//.*$', '', s, flags=re.M)
d = json.loads(s)
ext = d['customizations']['vscode']['extensions']
assert 'rust-lang.rust-analyzer' in ext and 'vadimcn.vscode-lldb' in ext, ext
print('extensions OK:', ext)
PY
```

---

### 任务 T26：写 `.vscode/launch.json` 调试配置

- **文件路径**：`.vscode/launch.json`（新建；同时创建 `.vscode/` 目录）

- **要做的**：写入以下内容

```jsonc
{
  "version": "0.2.0",
  "configurations": [
    {
      // 调试单个测试：在测试函数里下断点后选本配置并按 F5。
      "type": "lldb",
      "request": "launch",
      "name": "调试当前测试 (CodeLLDB)",
      "cargo": {
        "args": [
          "test",
          "--no-run",
          "--package=rgoc-harness",
          "--lib"
        ],
        // ⚠️ 用 target name（下划线），不是包名（连字符）—— 见下方注释
        "filter": { "name": "rgoc_harness", "kind": "lib" },
        // ⚠️ 必需：cargo 的工作目录。CodeLLDB **不读顶层的 cwd** —— 见下方注释
        "cwd": "${workspaceFolder}/rgoc"
      },
      "args": [],
      "cwd": "${workspaceFolder}/rgoc"
    },
    {
      // 调试整个 workspace 的测试
      "type": "lldb",
      "request": "launch",
      "name": "调试全部测试 (CodeLLDB)",
      "cargo": {
        "args": ["test", "--no-run", "--workspace"],
        "cwd": "${workspaceFolder}/rgoc"
      },
      "cwd": "${workspaceFolder}/rgoc"
    }
  ]
}
```

> **⚠️ 实测踩坑：`cargo` 那两个字段写错都会在 F5 时报错**（2026-10-02，进容器后真的遇到）
>
> 本任务初版的模板有两处缺陷，都是**首次 F5 才暴露**的：
>
> | 缺陷 | 现象 | 为什么 |
> |---|---|---|
> | `cargo` 里没有 `cwd` | F5 弹 **`Cargo command did not complete successfully.`**，LLDB 输出通道里是 `Cargo exited with code 101` | CodeLLDB 的工作目录取自 **`cargo.cwd`**（源码 `getCargoCwd(e){return e ?? this.workspaceFolder?.uri?.fsPath}`，调用处传 `e.cargo.cwd`），**不读顶层 `cwd`** → 回退到 workspaceFolder `/work` → 那儿没有 `Cargo.toml` |
> | `filter.name` 写成包名 `rgoc-harness` | 修好前一条后会报 **`Cargo has produced no matching compilation artifacts.`** | 产物按 **`target.name`** 入列再过滤；本包 target name 是 `rgoc_harness`（下划线） |
>
> `${workspaceFolder}` 可以放心用在 `cargo.cwd` 里 —— cargo 的解析发生在
> `resolveDebugConfigurationWithSubstitutedVariables`（**变量已替换**之后）。
>
> 完整定位过程、三次复现与排障命令见 `M0-benchmarks.md` §10；
> 这两点现由 `scripts/check-m0-consistency.py` 第 1 节守着（4 条断言）。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
# 形状校验已并入自检脚本（比只校验「type 都是 lldb」强得多）：
#   · 每个 cargo 配置都有 cargo.cwd，且其指向的目录下真的有 Cargo.toml
#   · filter.name 等于 crate 的 target name（下划线），而不是包名
python3 scripts/check-m0-consistency.py | sed -n '3,20p'
# 期望：launch.json 相关的 5 条断言全为 ✓
```

---

### 任务 T27：在 VSCode 中 Reopen in Container 并跑通测试

- **文件路径**：无文件改动（**人工操作任务**）

- **要做的**（由用户在 VSCode 中执行）：

0. **（可选前置）** 若第 2 步的 Reopen 报 `Installing VS Code Server for commit …` 或
   `Failed to establish a socket connection to proxies`，**先别急着按 CodeLLDB 的办法修** ——
   那是**另一个层级**，见下方第二个踩坑注记与 `M0-benchmarks.md` §9。最低成本的起手式：
   `docker exec <容器名> bash /work/scripts/install-vscode-server.sh --commit <宿主 commit>`；
1. 打开 `/Users/wangfeng/workspace/rust_go_compiler`；
2. `Cmd+Shift+P` → **Dev Containers: Reopen in Container**；
3. 等待容器构建与扩展安装完成（底部状态栏显示容器名）；
4. 打开集成终端，确认提示符来自 Linux 容器：

```sh
cd /work/rgoc && cargo test --workspace
```

- **验证**：

```sh
# 在容器内的 VSCode 集成终端中执行
uname -a                      # 期望：Linux ... aarch64
rustc --version               # 期望：rustc 1.98.1
cd /work/rgoc && cargo test --workspace
# 期望：test result: ok. 1 passed
# 并且 VSCode 左侧「扩展」面板中 rust-analyzer 与 CodeLLDB 已处于已安装状态
```

> **本任务的产出判据只是「容器能开、测试能跑」** —— 断点是否命中是 T28 的判据。

> **实测踩坑：CodeLLDB「无法下载」**（2026-10-02，首次 Reopen 时真的遇到）
>
> 现象：扩展**本体**安装成功（`Extension installed successfully`），但 LLDB 输出面板报
> `Failed to establish a socket connection to proxies: PROXY 127.0.0.1:7897`，
> 于是**平台包**（含真正的 lldb 二进制，从 GitHub releases 单独下载）没装上，调试器不可用。
> 判据：`<扩展目录>/platform.ok` 与 `<扩展目录>/lldb/bin/lldb` 是否存在。
>
> 根因（已坐实）：宿主 VSCode 通过 AHP 的 `root/configChanged` **主动把**
> `http.proxy = http://127.0.0.1:7897` **下发进容器**
> （证据：`~/.vscode-server/data/logs/<会话>/ahp/ahp-*.jsonl`），
> 而容器内的 `127.0.0.1` 指向容器自己的 loopback。
>
> ⚠️ **在 `.devcontainer/devcontainer.json` 的 `customizations.vscode.settings` 里写
> `"http.proxy": ""` 实测【无效】** —— 该值由客户端下发，优先级高于远端 Machine settings。
> 实测：写好后（文件 mtime 03:31:58）新开的会话（03:40:12）仍报同一错误。
> 这两行予以保留（对别的环境可能有效、无副作用），但**它不是修法**。
>
> **修法**：跑 `scripts/install-codelldb.sh` —— 用 `curl --noproxy '*'` 直连下载平台包，
> 再用 VSCode 自带的 `code-server` CLI 从本地 vsix 安装，完全绕开 VSCode 的网络栈。
> 幂等（`platform.ok` 存在即短路），容器重建后重跑一次即可：
>
> ```sh
> docker exec <容器名> bash /work/scripts/install-codelldb.sh
> # 然后在 VSCode 中执行 “Developer: Reload Window”
> ```
>
> 完整因果链（6 条证据）、修法与实测输出见 `M0-benchmarks.md` §7。
>
> **排障命令**（在容器内）：
> `cat ~/.vscode-server/data/logs/*/exthost*/output_logging_*/[12]-LLDB.log`
> （注意文件名号会递增，别只看 `1-LLDB.log`）

> **第二次实测踩坑：这次是 VS Code Server 本身下不来**（2026-10-02，宿主 VSCode 升到 1.140.0 后复发）
>
> ⚠️ **先做层级辨异再动手** —— 报错文字与上面那条**一模一样**（都是
> `Failed to establish a socket connection to proxies: PROXY 127.0.0.1:7897`），
> 但这次卡住的是**三层下载里最上面那一层**，修法完全不同。判据只有一个：
> **看日志里的 `Path:`**。
>
> | 卡在哪一层 | 日志特征 | 修法 |
> |---|---|---|
> | ① **VS Code Server** | `Installing VS Code Server for commit <sha>` + `Path: /var/folders/…`（**宿主**临时目录） | `scripts/install-vscode-server.sh --commit <sha>` |
> | ② 扩展**本体**（marketplace） | 扩展面板显示装不上 | 一般不受影响（走 marketplace，非 github） |
> | ③ 扩展**平台包**（GitHub releases） | `<扩展目录>` 里没有 `platform.ok` | `scripts/install-codelldb.sh` |
>
> 根因：宿主 VSCode 升级 → **commit 变了**（1.139.1 `04c0d99f…` → 1.140.0 `07f806f9…`），
> 而持久卷 `/vscode` 里只有旧 commit 的 server，于是 Dev Containers 去宿主侧下载 204 MB 的
> server tarball，又被那个**已经停掉的**死代理挡住。实测该代理端口确已无监听
> （`nc` 拒连 / `curl -x` 拒连 / `lsof -iTCP:7897 -sTCP:LISTEN` 为空），
> 而容器**直连** `update.code.visualstudio.com` 是通的（HTTP 200）。
>
> **修法**（同样绕开 VSCode 网络栈，同样幂等）：
>
> ```sh
> # 1) 取宿主当前 VSCode 的 commit
> python3 -c "import json;print(json.load(open('/Applications/Visual Studio Code.app/Contents/Resources/app/product.json'))['commit'])"
> # 2) 容器直连下载 tarball 装进持久卷 /vscode（会补 ~/.vscode-server/bin/<commit> 符号链接）
> docker exec <容器名> bash /work/scripts/install-vscode-server.sh --commit <sha>
> ```
>
> 完整因果链、持久卷布局与实测数据见 `M0-benchmarks.md` §9。
> **复发条件**：每次升级宿主 VSCode 都要重跑一次。**修完必须重新 Reopen in Container 才生效。**

---

### 任务 T28：**E5 —— 实测断点命中**

- **文件路径**：无文件改动（**人工操作任务**）

- **前置 0：先确认容器连得上**（`Reopen in Container` 若报 `Installing VS Code Server…` 就是这一层）

```sh
# 在宿主（macOS）执行
COMMIT=$(python3 -c "import json;print(json.load(open('/Applications/Visual Studio Code.app/Contents/Resources/app/product.json'))['commit'])")
echo "宿主 VSCode commit = $COMMIT"
# 检查 Dev Containers 的第一处判据（符号链接，与实际架构无关；第二处是 /vscode 卷内的实体目录）
docker exec <容器名> test -d "/home/dev/.vscode-server/bin/$COMMIT" \
  && echo "✓ server 就位" || echo "✗ 需先跑 install-vscode-server.sh --commit $COMMIT"
# 期望：✓ server 就位。若 ✗ → 见 T27 的第二个踩坑注记（M0-benchmarks.md §9）
```

- **前置 1：再跑环境层冒烟测试**（把「环境坏了」与「编辑器接线坏了」分开）

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
docker exec <容器名> bash /work/scripts/debug-smoke-test.sh
# 期望：第 2 节 A/B/C 三项 + 9 项断言全部 ✓，exit 0。任一 ✗ 就先修环境，不要急着怪 VSCode。
```

该脚本在容器内用平台包自带的 lldb 跑完整会话并逐项断言（断点解析 / 命中 / 调用栈 / 形参 /
单步 / 停止行 / 中间值 / 测试结束）。它是 E5 的**下层证据**；E5 的验收仍需下面的人工步骤。

其中**第 2 节是 E5 那条「编辑器链路」在下层的彩排**：它按 `.vscode/launch.json` **原样**跑一次
cargo（A：工作目录取自 `cargo.cwd`；B：退出码 0；C：`filter` 恰好选中 1 个产物），
复刻 CodeLLDB 的 `runCargoAndGetArtifacts` / `getProgramFromArtifacts`。
这三个坑不在这里暴露就必然在 F5 那一刻暴露 —— 它们都是**静默**的（配置写错也合法，
只在按 F5 时变成一句没指向性的提示），详见 `M0-benchmarks.md` §10。

第 2 节的「确实是在复刻」由 `check-m0-consistency.py` 第 5c 节静态守住（6 条断言：
读真文件 / 同序替换变量 / 不经 shell / 复刻 filter 筛选 / 逐项打印 A/B/C / 互引 §10）。

- **要做的**（人工）：

1. 在容器内的 VSCode 中打开 `rgoc/crates/rgoc-harness/src/lib.rs`；
2. 在 `double_sum()` 函数体第一行（**`let sum = a + b;`，当前为第 27 行**）
   **点击行号左侧设置断点**（出现红点）；
3. 打开 `launch.json` 对应的 **「调试当前测试 (CodeLLDB)」** 配置，按 **F5**；
   **若弹出 `Cargo command did not complete successfully.` 并提示打开 `launch.json`**：
   这不是环境层问题（前置 1 已经证伪），而是 CodeLLDB 的 **cargo 启动配置** ——
   两个静默陷阱是 `cargo.cwd` 漏写与 `filter.name` 写成包名，**真错在 OUTPUT → LLDB
   通道里的 `Cargo exited with code N`**，不在弹出的提示里。排查方法与修法见
   `M0-benchmarks.md` §10；**不要用 shell 复现那条命令**（会得到假的 TOML 报错）。
4. 确认：执行**停在断点处**（黄条高亮），左侧「变量」面板显示 `a = 1`、`b = 2`，
   「调用栈」面板显示 `rgoc_harness::double_sum` ← `tests::double_sum_works`；
5. 按 F10 单步一次，确认停止行移到 `sum * 2`（第 28 行）且 `sum` 显示为 `3`；
   按 F5 继续至结束。

- **验证**（**E5 的判定必须是实测，不是「能开窗口」**）：

| 检查项 | 期望 | 实测 |
|---|---|---|
| 断点命中（未被跳过） | 黄条停在 `let sum = a + b;`（第 27 行） | ☐ |
| 「变量」面板非空 | 可见 `a = 1`、`b = 2` | ☐ |
| 「调用栈」面板非空 | ≥2 帧（`double_sum` ← `double_sum_works`） | ☐ |
| 单步后可观察值变化 | 停止行移到第 28 行，且 `sum == 3` | ☐ |

**四项全部 ☑ 才算 E5 通过**，并把结果记录进 `M0-manifest.json` 的 `gate.E5`。

> 第 4 项依赖调试目标的具体形状 —— 见 T21 的说明与 `M0-benchmarks.md` §8：
> 若函数写成 `let sum = a + b; sum`（尾位置直接返回），rustc 不会为 `sum` 生成 DWARF 条目，
> 这一项**任何调试器都做不到**。改形状前请先读 §8。

- **若断点被静默跳过**：优先检查 T24 的 `runArgs` 是否真的生效。
  ⚠️ **不要**在容器里用 `/proc/self/status` 的 `CapEff` 判断 —— 以非 root 用户 `dev` 运行时它
  **恒为 0**，会得出错误结论。要在**宿主**上查容器配置：

```sh
# 在宿主（macOS）执行
docker inspect <容器名> --format 'CapAdd={{json .HostConfig.CapAdd}} SecurityOpt={{json .HostConfig.SecurityOpt}}'
# 期望：["CAP_SYS_PTRACE"]  ["seccomp=unconfined"]
```

---

## 4. 阶段门禁汇总

### Phase 0 出口

| 门禁 | 判定方式 | 状态 | 证据 |
|---|---|---|---|
| **E1** | `docker build` 从零重放成功；`docker/image.lock` 无占位符 | **✅** | 四次 `--no-cache` 全通过；`grep -c '<' docker/image.lock` = 0；base index digest 按 digest 解析成功。**image id 不作为门禁**（实测不可复现，见 `M0-benchmarks.md` §5） |
| **E2** | T16 校验脚本输出 `none`（无空缺/占位符） | **✅** | `EMPTY/PLACEHOLDER FIELDS: none`，exit=0；并做了反向校验（3 种注入均被抓到） |
| **E10** | `M0-benchmarks.md` 四组数据齐备 | **✅** | §1 镜像 / §2 冷启动 / §3 编译 / §4 挂载布局，全部为实测值 |

### Phase 1 出口

| 门禁 | 判定方式 | 状态 | 证据 |
|---|---|---|---|
| T22 | `fmt` / `check` / `clippy -D warnings` / `test` 四条全过 | **✅** | 修正验证命令后 `clippy --all-targets -- -D warnings` exit=0，日志中 warning/error 行数为 0。**注意**：调试目标改为 `double_sum` 后已**不再需要** `#[allow(clippy::let_and_return)]`（尾表达式是 `sum * 2`），该 allow 已删除 |
| **E5** | T28 的四项检查全部 ☑（**实测断点命中**） | ☐ **待人工** | 下层证据已齐：`scripts/debug-smoke-test.sh` 第 2 节 **A/B/C 三项 + 9 项断言通过、exit 0**（断点解析到 `double_sum + 20 at lib.rs:27`、`stop reason = breakpoint 1.1`、调用栈 3 帧、`a=1`/`b=2`、`step over` 停到 `lib.rs:28`、`sum = 3`、`1 passed; 0 failed`）；第 2 节的 A/B/C 即「编辑器链路」在下层的彩排（其复刻行为由自检第 5c 节 6 条断言守住）。T27/T28 的**编辑器链路**仍需用户在 VSCode 中按 F5 确认 |

**Phase 1 门禁通过后**：

1. 交付用户可用的 VSCode 调试环境（用户明确要求的那一项）；
2. 基于**实测到的环境事实**回头拆 **Phase 2–4** 的计划（本计划 §0.1 已说明理由）；
3. 更新 `M0-manifest.json` 与 `M0-plan.md` 的执行状态。

---

## 5. 执行纪律

继承 `../03-roadmap.md` §3.5 与 `AGENTS.md` §4：

| 纪律 | 说明 |
|---|---|
| **规格未确认不动手** | 本计划经确认后才开始 T01 |
| **RED → GREEN → REFACTOR** | 本计划中 T21 之后的每个行为改动都应先有失败测试；环境类任务（T01–T19）以「验证命令」代替测试 |
| **没有失败的测试就不算实现** | 环境任务至少要有**可判定的验证命令**，且必须真的跑过 |
| **不盲目演进临时代码** | T21 的 `double_sum()` 是明确的**调试目标**，Phase 2 引入真实功能后删除 |
| **提交仅在明确要求时执行** | 每个任务完成后**不自动提交**；提交信息须引用任务号与验证证据 |
| **失败不掩盖** | 任何验证失败都要记录原始报错与复现方式，不得通过放宽断言、换镜像 tag 等方式「绕过」 |

**遇到阻塞时的处理顺序**：

1. 记录原始报错（命令 + 完整输出）；
2. 判断属于哪类：环境 / 工具链 / 网络 / 计划本身有误；
3. 网络类 → 重试 3–4 次（`github.com` 类故障是间歇性的）；
4. 工具链类 → 回到对应任务修改，**不要绕过门禁**；
5. 计划有误 → **回头改计划**（Superpowers：计划变了就改计划，不要悄悄改）。

---

## 6. 变更记录

| 日期 | 变更 | 原因 |
|---|---|---|
| 2026-10-02 | 初版，覆盖 Phase 0（T01–T19）与 Phase 1（T20–T28） | 见 §0.1 |
| 2026-10-02 | T16 的检查脚本：`'unknown' in v` 子串匹配改为「整值等于占位词」 | **旧写法会让 E2 永远无法通过** —— `target.triple` 的合法字面量 `aarch64-unknown-linux-gnu` 含 `unknown`，被误判为空缺。已用脚本实证（旧断言报出该字段，新断言判 `none`） |
| 2026-10-02 | T18 验证命令：crate 从容器内 `/tmp` 改为 **bind mount 之内** | 原写在 `/tmp`（overlayfs），两种布局都测不到宿主文件系统 —— 而这正是 D-M0-9 要问的问题 |
| 2026-10-02 | T18：crate 名 `crate` → `probe`；清空手段 `cargo clean` → `find … -delete`；`/usr/bin/time -f` → bash 内建 `time` | `crate` 是 Rust 关键字；`cargo clean` 在命名卷挂载点报 `EBUSY(16)`；容器内没有 `/usr/bin/time`（Debian 的 `time` 包未装） |
| 2026-10-02 | T19 后的 D-M0-9 决策：`target/` **改用命名卷**（`rgoc-target`） | T18 实测 2.3×（0.137s vs 0.336s）。相应改 `scripts/in-container.sh` 与 `.devcontainer/devcontainer.json` |
| 2026-10-02 | T21 的 `add()` 增加 `#[allow(clippy::let_and_return)]` | **计划自相矛盾**：原代码 `let sum = a + b; sum` 无法通过 T22 的 `clippy -D warnings`。中间的 `sum` 是 T28「单步后观察值变化」的必需观察点，故局部 allow 并写明理由，而非删掉绑定。**⚠️ 本条已被下方 2026-10-02 的「调试目标改形状」取代 —— 该 allow 已删除** |
| 2026-10-02 | T22 验证命令：加 `set -euo pipefail` | 原写 `cargo clippy … \| tail -2` 串在 `&&` 链里，管道退出码取自 `tail` —— clippy 失败仍返回 0，**失败被静默吞掉**（实测正是如此：命令「跑完了」但 clippy 已报错退出） |
| 2026-10-02 | Dockerfile 新增层 5（`/etc/profile.d/50-rgoc-toolchains.sh`）与层 6（回归断言） | 实测缺陷：登录 shell 执行 `/etc/profile` 会**无条件重置 PATH**，`bash -lc 'cargo --version'` → `command not found`。VSCode 集成终端默认是登录 shell，不修则用户敲 cargo 就报错 |
| 2026-10-02 | Dockerfile 新增 `sudo`（非 root 用户免密） | 命名卷属主修正需要 root；devcontainer 的 `postCreateCommand` 没有 sudo 无解 |
| 2026-10-02 | E1 的判定方式调整：不要求 image id 稳定 | 实测同输入两次 `--no-cache` 构建产出不同 image id 与 6/14 个不同层 digest。钉子改为 `base.index_digest` + `src.*_sha256` |
| 2026-10-02 | T17/T18/T23 的模板片段回填为实测值与本文件的实现差异说明 | 计划是活文档；实测结果已定稿在 `M0-benchmarks.md`，此处只留结论与出处，避免数字两处漂移 |
| 2026-10-02 | devcontainer 的 `customizations.vscode.settings` 增加 `"http.proxy": ""` + `"http.proxySupport": "off"` | **T27 实测踩到**：宿主 User settings 的 `http.proxy=http://127.0.0.1:7897` 被继承进容器，容器内该地址指向自己的 loopback → CodeLLDB 下平台包时报 `Failed to establish a socket connection to proxies`。**⚠️ 后来实测证明这两行【不生效】，见下方 2026-10-02 的更正条目** |
| 2026-10-02 | Dockerfile 层 2 增加 `/usr/local/bin/lldb` 包装脚本 + 输出断言 | **实测缺陷**：`lldb --version` 抛 `ModuleNotFoundError: No module named 'lldb.embedded_interpreter'`（python3-lldb-14 已装、模块也在，但 `/usr/lib/python3/dist-packages/lldb` 是相对符号链接，被解析成 namespace package）。用包装脚本注入 `PYTHONPATH` 而非全局环境变量，避免污染其它 python 进程。断言查**输出**而非退出码 —— 损坏态会打 traceback 但仍以 0 退出 |
| 2026-10-02 | **更正上一条的处置**：新增 `scripts/install-codelldb.sh`（离线安装平台包），并在文档中明确 `"http.proxy": ""` **无效** | **实测反证**：Machine settings 写入时间 03:31:58，之后新会话 03:40:12 **仍报同一错误**。真因是宿主通过 AHP `root/configChanged` **下发** `http.proxy` 给容器（证据在 `~/.vscode-server/data/logs/<会话>/ahp/ahp-*.jsonl`），远端设置排不上队。脚本用 `curl --noproxy '*'` + `code-server --install-extension` 完全绕开 VSCode 网络栈，实测通过。因果链 6 条证据见 `M0-benchmarks.md` §7 |
| 2026-10-02 | T21 的调试目标由 `add()` 改为 `double_sum()`（`let sum = a + b; sum * 2`），删除 `#[allow(clippy::let_and_return)]`；T28 的断点行改 21 → 27 | **T28 第 4 项检查原状不可实现（计划缺陷）**：rustc 1.98.1 不为「尾位置直接返回的 `let` 绑定」生成 DWARF 变量条目，`sum` 对**任何**调试器都不可见。三变体对照实验见 `M0-benchmarks.md` §8 |
| 2026-10-02 | 新增 `scripts/debug-smoke-test.sh`（无头调试链路冒烟测试，9 项断言） | 把 E5 的**下层**证据变成可重放的判定：断点解析 / 命中 / 调用栈 / 形参 / 单步 / 停止行 / 中间值 / 测试结束。断点行从源码推导（不硬编码），并锚定「行首纯代码行」——第一版宽松 `grep` 命中了**注释行**，推出错误的行号 |
| 2026-10-02 | T28 的「断点被静默跳过」排障命令：`cat /proc/self/status \| grep CapEff` → 宿主侧 `docker inspect … HostConfig.CapAdd/SecurityOpt` | **原命令会给出错误结论**：以非 root 用户 `dev` 运行时 `CapEff` **恒为 0**（实测），据此会误判成「容器没给 SYS_PTRACE」。实测容器配置为 `CapAdd=["CAP_SYS_PTRACE"]`、`SecurityOpt=["seccomp=unconfined"]`，本来就是对的 |
| 2026-10-02 | **新增 `scripts/install-vscode-server.sh`**（离线把指定 commit 的 VS Code Server 装进持久卷 `/vscode`）；T27 增加「VS Code Server 下不来」的踩坑注记；T28 增加**前置 0**（server 就位自检） | **同一句报错、不同层级**：宿主 VSCode 升到 1.140.0（`07f806f9…`）后，卷里只有 `04c0d99f…`（1.139.1）与 `110a328e…`，Dev Containers 于是去**宿主侧**下载 204 MB 的 server tarball，被那个**已停服**的代理挡住（实测 `nc`/`curl -x`/`lsof` 三重确认 `127.0.0.1:7897` 无监听，而容器直连 HTTP 200）。**这不是 §7 的 CodeLLDB 复发** —— 判据是日志里的 `Path:` 指向宿主 `/var/folders/…`。修法与实测（204 MB / 两种测速 / sha256 / 两处 `test -d` 均 0）见 `M0-benchmarks.md` §9 |
| 2026-10-02 | T27 的 `Reopen in Container` 步骤补一条**判据**：连不上窗口时先跑 server 自检，再谈别的 | 避免下次看到眼熟的代理报错就按 CodeLLDB 的办法修 —— 那会在错误的层里空转。`install-vscode-server.sh` 幂等且零多余请求，是信息最少时的最低成本起手式 |
| 2026-10-02 | **T26 的两处缺陷修正**：`cargo` 里补 `"cwd": "${workspaceFolder}/rgoc"`；`filter.name` 由包名 `rgoc-harness` 改为 target name `rgoc_harness`；T26 的验证命令改为跑自检脚本（原来只校验 `type` 全是 lldb，强度不够） | **首次 F5 实测暴露（计划缺陷）**：① `cargo.cwd` 缺失 → CodeLLDB 回退到 workspaceFolder `/work`，而 `/work` 下没有 `Cargo.toml` → `Cargo exited with code 101` → VSCode 报 `Cargo command did not complete successfully.`（顶层 `cwd` 写得再对也没用 —— CodeLLDB 不读它）；② `filter.name` 用包名 → 产物按 `target.name`（`rgoc_harness`，下划线）过滤 → 0 匹配 → `Cargo has produced no matching compilation artifacts.`。定位过程、三次复现与假线索（用 shell 复现会因剥引号得到假 TOML 报错）见 `M0-benchmarks.md` §10 |
| 2026-10-02 | `scripts/check-m0-consistency.py` 第 1 节增加 4 条 launch.json 断言；`mutation-test-m0-consistency.py` 增加 3 个变异用例与**对照组** | 把 §10 的两个坑变成可回归的判据。**顺带修掉变异测试自身的一个缺陷**：临时副本漏了 `rgoc/Cargo.toml`，导致相关断言被整段跳过、变异「打不响」被误读成「断言恒真」—— 现在先跑一次未变异的对照组，必须 exit 0 |
| 2026-10-02 | **`debug-smoke-test.sh` 第 2 节改写**：不再自己拼一套 cargo 参数，而是**按 `.vscode/launch.json` 原样复刻** CodeLLDB 的 `runCargoAndGetArtifacts` / `getProgramFromArtifacts`，逐项打印 A) cwd 来自 `cargo.cwd`、B) 退出码 0、C) `filter` 恰好选中 1 个产物 | §10 的两个坑只在「真的按 launch.json 跑一次 cargo」时才暴露。旧版第 2 节用通配符找二进制，**恰好绕过了 filter 那一步** —— 即修错了 `filter.name` 也能通过。复刻时有三处必须照抄：读 `launch.json` 而非抄参数、**不经 shell**（argv 列表）、逐行复刻产物筛选 |
| 2026-10-02 | **新增 `scripts/mutation-test-debug-smoke.sh`**（4 个变异 + 对照组，跑完自动还原）；`check-m0-consistency.py` 增加 §5c 共 15 条断言（**45 → 60**）；`mutation-test-m0-consistency.py` 增加 11 个 §5c 变异（**20 → 31**） | 新断言同样必须被证明**能失败**。这个变异测试直接改**真实** `launch.json`（因为第 2 节就是按它的真实路径读的），所以还原靠 `trap` + sha256 双保险，且判定标准是**失败原因特征**而非仅退出码 —— 否则脚本自身崩掉也会被算成「抓住」。它自己也进 `FILES` 夹具，否则 §5c 的断言在临时副本里会被整段跳过 |
| 2026-10-02 | `code_only()`（剔除**整行注释**后再做子串断言）推广到 `install-*.sh` 的既有断言 | 变异测试抓出**两条恒真断言**：`ln -sfn` 与 `[0-9a-f]{40}` 的匹配点被脚本**头部注释**满足 —— 把真实代码行改成注释形态也不会报错。本仓已第二次踩「注释满足断言」的坑（第一次是断点行推导的宽松 `grep`） |
| 2026-10-02 | **移除 3 个脚本**：`scripts/env-probe.sh`（T14，输出已固化进 manifest）、`scripts/mutation-test-m0-consistency.py`（33 个变异）、`scripts/mutation-test-debug-smoke.sh`（4 个变异）。`check-m0-consistency.py` 由 **63 条断言降为 52 条**（删掉守变异脚本的 9 条 §5c + 2 条 §6）；§6 保留「`AGENTS.md` 记录的断言数 == 实际数」这条防腐断言 | **用户要求「提交前清理掉以后可能不再需要的脚本」**。判据是**是否与自检/门禁耦合、以及是否会复发**：两个 `install-*.sh` 会随容器重建与宿主升级 VSCode 复发，`in-container.sh` 被 `image.lock` 指纹引用，`debug-smoke-test.sh` 是 E5 的下层证据 —— 都不能删。**代价要明确**：移掉两个变异脚本后，「断言是否恒真」不再有任何自动校验，改断言须人工反向验证（把目标改成注释形态/删掉，确认报 ✗）。快照留 `.workbuddy/backup/scripts-removed-20261002-1504/`（不入库），拷回即可重跑。**注意**：`env-probe.sh` 的脚本体仍完整保存在 T14，重新采集 E2 事实时按文重建即可 |
