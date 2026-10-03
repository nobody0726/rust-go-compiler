# M0 实施计划（M0-plan.md）

> **阶段 ID**：`M0`　|　**状态**：待执行　|　**日期**：2026-10-02
> **文档索引**：[`../README.md`](../README.md)　|　**上游**：[`M0-design.md`](./M0-design.md)（设计）、[`M0-tests.md`](./M0-tests.md)（验收）
> **方法论**：Superpowers `writing-plans` —— 每个任务 **2–5 分钟**可完成，含精确文件路径、完整可粘贴内容、可判定的验证方式。

---

## 0. 本计划的覆盖范围

### 0.1 Phase 0–1 是「先拆」，Phase 2–4 是「后拆」（现已全部拆完）

**这不是遗漏，是刻意为之**，理由有三条：

| # | 理由 |
|---|---|
| 1 | **writing-plans 要求「完整可粘贴的代码、无歧义」**。Phase 2 的 harness 代码结构依赖 Phase 1 实际建成的 workspace 骨架与真实工具链行为（`clang` 版本、rustup 行为、`cargo` 缓存布局）—— 现在硬写出来就是编造，会被执行者照抄成错。 |
| 2 | **D-M0-2「严格环境优先」** —— Phase 0/1 是环境的全部；**环境门禁未过，Phase 2+ 不得开工**。提前拆出无法执行的任务没有价值。 |
| 3 | `../03-roadmap.md` §8.2 —— **不提前拆完**；Superpowers 亦规定「计划变了就回头改计划」，而环境实测结果必然影响后续计划。 |

> **已拆（2026-10-02）**：E5 通过后，本文档已补齐 **Phase 2（§4，T29–T39）、Phase 3（§5，T40–T47）、Phase 4（§6，T48–T55）**。下面三条理由是当初**为什么先不拆**，现在它们都已被满足或已用实测结果替代：
>
> ① Phase 2 的 harness 结构依赖 Phase 1 实际建成的 workspace 骨架 —— 现在骨架已存在（T20–T22 实测），不再是编造；
> ② D-M0-2「严格环境优先」—— 环境门禁 E1/E2/E5/E10 已全部通过，**Phase 2 具备开工条件**；
> ③ `03` §8.2「不提前拆完」—— 已到该拆的时点，且 Phase 3/4 已按实测环境事实校准（`clang` 14 / aarch64 ELF 实测见 `M0-benchmarks.md` §1）。
>
> **拆解时另有两处文档缺陷被查出并修正**（Superpowers「计划变了就改计划」）：
> 一是 `M0-design.md` §6.3 的 crate 表与 `03` §2「只建当期需要的」冲突（→ **D-M0-13**）；
> 二是「Phase 2 删除 `double_sum`」与「E5 是人工门禁」冲突（→ **D-M0-15**，改为让它当 T-H-01 的 fixture）。

> **状态更新（2026-10-02）**：Phase 0 与 Phase 1 均已完成，四项门禁 **E1/E2/E10/E5 全部通过**
> （E5 由用户在 VSCode 中按 F5 实测确认，登记在 `M0-manifest.json` 的 `gate.E5`）。
> **拆 Phase 2–4 计划的前置条件已满足**，现在即可动手。

### 0.2 一处 Phase 边界的调整（需注意）

`M0-design.md` §6.2 把「VSCode 调试环境」列为 Phase 1，其门禁 E5 要求在**某段 Rust 代码**上实测断点命中。但按原划分，Rust 工程要到 Phase 2 才建 —— 那就没有调试目标。

**因此本计划把「最小 Rust 工程骨架」提前到 Phase 1**（T20–T22）：

- 只建 `rgoc/` workspace + **一个** `rgoc-harness` crate + **一个**测试；
- 它是调试目标，也是 Phase 2 的起点；
- Phase 2 在此基础上扩展成完整 harness（Test IR / 指令解析 / 比较器）。

这符合 `M0-design.md` §6.3「只创建当期需要的模块」的原则 —— 当期（Phase 1）确实需要它。

### 0.3 任务编号与门禁对应

| 任务范围 | 对应 Phase | 对应门禁 | 状态 |
|---|---|---|---|
| T01–T07 | Phase 0 · 镜像构建 | **E1** 镜像 digest 可重放 | ✅ |
| T08–T11 | Phase 0 · 工具链验证 | **E2** 环境值入 manifest | ✅ |
| T12–T16 | Phase 0 · 入口与 manifest | **E2** | ✅ |
| T17–T19 | Phase 0 · 基准测试 | **E10** | ✅ |
| T20–T22 | Phase 1 · 最小 Rust 工程 | — | ✅ |
| T23–T27 | Phase 1 · devcontainer | — | ✅ |
| T28 | Phase 1 · **实测断点命中** | **E5** | ✅ 2026-10-02 |
| T29–T39 | Phase 2 · Rust 工程骨架与 harness 自验 | **E3** + **E4** | ⏳ 进行中（**T29–T34 ✅ 2026-10-03**） |
| T40–T47 | Phase 3 · 三个架构 spike（解释 / SSA / native） | **E6** + **E7** | ⏳ 待开工 |
| T48–T55 | Phase 4 · 契约初稿与交付报告 | **E8** + **E9** | ⏳ 待开工 |

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

> **实际落地的脚本与本处片段有 3 处不同**（`scripts/in-container.sh` 为准，见 §9 变更记录）：
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
# 子串匹配会把它误判成空缺 —— 这是本计划 v1 的缺陷（见 §9 变更记录）。
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

> **布局要点（本计划 v1 的缺陷，见 §9 变更记录）**：crate 必须建在**bind mount 之内**，
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

> **`set -euo pipefail` 不可省**（本计划 v1 的缺陷，见 §9 变更记录）：
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

> **实际落地的文件比本处片段多两项**（`.devcontainer/devcontainer.json` 为准，见 §9 变更记录）：
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

| 检查项 | 期望 | 实测（2026-10-02，用户人工） |
|---|---|---|
| 断点命中（未被跳过） | 黄条停在 `let sum = a + b;`（第 27 行） | ☑ **通过** |
| 「变量」面板非空 | 可见 `a = 1`、`b = 2` | ☑ **通过** |
| 「调用栈」面板非空 | ≥2 帧（`double_sum` ← `double_sum_works`） | ☑ **通过** |
| 单步后可观察值变化 | 停止行移到第 28 行，且 `sum == 3` | ☑ **通过** |

**四项全部 ☑ 才算 E5 通过**，并把结果记录进 `M0-manifest.json` 的 `gate.E5`。

> ✅ **E5 已完成**（2026-10-02）：用户在 VSCode dev container 中按 F5 运行
> 「调试当前测试 (CodeLLDB)」，上表四项逐项确认通过。结果已登记进
> `M0-manifest.json` 的 `gate.E5`（含 `confirmed_at` / `confirmed_by` / `evidence`，
> 原始正文见 `git log` 中本日期的提交）。自检也随之把
> `gate: E1/E2/E10=pass, E5=pending` 改为 `gate: E1/E2/E10/E5 全部 pass`，
> 并新增一条「`gate.E5` 登记了确认人与确认时间」的断言。

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

## 4. Phase 2 —— Rust 工程骨架与 harness 自验（门禁 E3 + E4）

> **本 Phase 的 crate 边界**（决策 **D-M0-13**，2026-10-02 拍板）：**只建当期需要的**——
> `rgoc-harness`（扩写）、`rgoc-driver`（骨架）、`xtask`。
> 三个 spike 所需的 `rgoc-hir` / `rgoc-spikes` **留到 Phase 3 建**。
> 依据是 `03` §2 的原文：「M0 只创建当期需要的部分……其余 crate **不建空壳**，按里程碑依赖引入」。
> （`M0-design.md` §6.3 的 crate 表原写法与此冲突，已按本决策修正并加注。）
>
> **纪律变化**：从 T29 起进入 TDD 区间（`03` §3.5）。**凡是行为改动一律 RED → GREEN → REFACTOR**；
> 纯验证任务（无文件改动）不适用，先确认失败来自缺失行为而非环境/harness。

### 任务 T29：冻结 `M0-tests.md`（白名单 —— 门禁纪律要求「开工前冻结」）

- **文件路径**：`docs/milestones/M0-tests.md`（改文档头状态与 §0 冻结表）

- **要做的**：
  1. 文档头 `状态：待冻结` → `已冻结（2026-10-02）`；
  2. §0 冻结表三行（F1 20 样本 / F2 unsupported 清单 / F3 超时与资源上限）标注冻结时点；
  3. **计算并记录 M0 分母**（§9 待办的最后一项，也是 §6 末尾那条要求）：
     分母 = 顶层 `test/` 中模式属于 v0 支持集（`run` / `compile` / `errorcheck`）且无排除参数的**文件集**；
     E3 分母 = 6（T-H-01..06），E4 分母 = 20（T-C-01..20）。写进 §8 的判定表。
  4. §9 待办分流：「期望值复核」→ T30；「manifest 机器可读版」→ T54。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
head -4 docs/milestones/M0-tests.md | grep -o "状态.*"
# 期望：状态：已冻结（2026-10-02）
awk '/^### 4\.[234]/{s=1} /^## 5/{s=0} s && /^\| \*\*T-C-/{n++} END{print "T-C 样本数 =", n}' docs/milestones/M0-tests.md
# 期望：T-C 样本数 = 20   （F1 变更规则：不得削减到 20 以下）
```

> **冻结的意义**：此后 M0 的通过与否**只由这些 ID 决定**，不因「总体感觉良好」而改变（§0 末句）。
> 要新增样本必须单独记录并说明理由；**任何情况下不得把分母改小**。

> ✅ **T29 已完成（2026-10-02）**。执行中**查出三处问题**，都已回写进 `M0-tests.md`：
>
> 1. **M0 分母 = 279**（`run` 147 / `errorcheck` 120 / `compile` 12），其中 **5 个**在 linux/arm64 下被平台过滤
>    （`inline_math_bits_rotate` / `nilptr_aix` / `simd_inline` / `wasmexport` / `wasmexport2`）——
>    仍留在分母（`03` §3.3）。排除 77 项已逐条归到 U1/U2/U3/U5/U6/U7/U13/U14。记为 **F4**；
> 2. **原 unsupported 清单漏了两类** → 补 **U13**（`skip` 指令，5 个，上游 `t.Skip` 设计即跳过）与
>    **U14**（`linkmain.go`：action 非法但被平台过滤抢先 Skip，1 个）；
> 3. **分派顺序此前记漏了** → §1.3 新增 **R1b**：官方在 `switch`（`:541`）**之前**先做平台过滤
>    （`:522`）。**漏掉这条，harness 会在 `linkmain.go` 上误报 T-H-03（未知指令硬失败）**。
>
> **给 T33 的硬要求**：Rust 枚举器算出的分母**必须等于 279**（含那 5 个平台过滤项）——
> 这是「文档口径」与「代码实现」之间的一道交叉校验。

### 任务 T30：在 oracle 侧复核 20 个样本的期望值

- **文件路径**：`docs/milestones/M0-benchmarks.md`（**追加 §11**，不改既有章节）

- **要做的**：在容器内用 `go1.27.1` 逐个跑 §4 的 20 个样本，把**实际结果**与文档写的期望比对：
  - `run` 层 8 个（§4.2）：`go run` 的 stdout 与 `.out` 比对。**重点复核缺 `.out` 的样本**——
    按规则 R2，缺 `.out` 意味着**期望输出为空**，不是「任意输出都通过」；
  - `compile` 层 4 个（§4.3）：只编译，诊断走 R3 切分后比对；
  - `errorcheck` 层 8 个（§4.4）：比对错误诊断，并记住 **R5** —— `errorcheck` 即使裸写也会被
    自动加 `-d=ssa/check/on`，所以它的期望**不能当纯语言语义验收**。

  任何不符，**先判断是文档错、还是样本不适用**，再改文档并记录；**不允许「改期望值让它过」**。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh bash -lc 'go version'
# 期望：go version go1.27.1 linux/arm64
# 复核结果表写入 M0-benchmarks.md §11：20 行，每行含 样本 ID / 实际输出摘要 / 与文档是否一致 / 处置
```

> 这一步是 **E4 的前置**：期望值错了，harness 的比较器就被喂了一个错的 oracle，
> 后面 20/20 通过也**没有意义**。宁可在这里发现，不要留到 E4 判定时。

> ✅ **T30 已完成（2026-10-02）**。容器内 `go1.27.1` 逐个复核，原始记录见
> `M0-benchmarks.md` **§11**（含 20 行复核表）。结果与三处产出：
>
> 1. **19/20 与文档一致**；唯一不符的 **T-C-20 `mainsig.go`** 是**文档错** ——
>    源码里 ERROR 期望实为 **5 条**（L9 / L10×2 / L12 / L13），文档漏记第 9 行那条。
>    **处置：改文档**（§4.4 表与期望原文块已补），**不改样本**。
> 2. **查出规则缺失 → 补 R2b**：官方 `runcmd`（`:642-647`）把 **stdout 与 stderr 合并**
>    再交给 `checkExpectedOutput`。`helloworld.go` / `printbig.go` 用内建 `print`
>    （写 stderr），**只捕 stdout 会让这两个样本误判失败**。
> 3. **查出规则缺失 → 补 R6**：三层各自的命令形态。**`errorcheck` 用 `go tool compile`
>    而不是 `go build`** —— 所以 R5 的 `-d=ssa/check/on` 才合法（`go build -d=…` 直接报
>    `flag provided but not defined: -d`）。另含 `-C` 关列号、诊断路径要先
>    `replacePrefix` 成短名（含续行）、`errorCheck` 的匹配语义。
>
> **给 T32 / T34 / T35 的直接输入**：R2b（合并流）、R6（三条命令形态 + importcfg 生成）、
> R3 / R4 / `errorCheck` / `replacePrefix` 的逐条出处都已写进 `M0-tests.md` §1.3。
> **顺带实测**：20 个样本整层 **0.2 s**（预算 ≤ 5 min），单项 < 0.1 s。

### 任务 T31：把 `rgoc-harness` 从「调试目标」扩成 Test IR 骨架

- **文件路径**：`rgoc/crates/rgoc-harness/src/lib.rs`（改）

- **要做的**：
  1. ⚠️ **不要删 `double_sum`** —— 这条**修正了既有计划**：`M0-design.md` §1.2 与本计划 §8 的纪律
     原写「Phase 2 引入真实功能后删除」。但 **E5 是人工门禁**，删掉锚点就无法复验。
     改为：让它成为 **T-H-01（成功类）的 fixture**，从「遗留调试目标」变成「有存在价值的自测输入」。
     已同步登记为 **D-M0-15**（见 `M0-design.md` §2.1）；
  2. 新增 Test IR 类型（**C2 契约的载体**），必录字段见 `M0-tests.md` / `03` §3.1：
     用例 ID、相对路径、输入文件集合、模式、指令参数、build tags、目标/版本、import 需求、
     功能依赖、比较器、期望退出码/输出/诊断、超时、资源上限、seed、归属阶段、unsupported 原因；
  3. 结果分类**至少八种且不得合并**：`pass` / `compiler-failure` / `runtime-failure` /
     `harness-failure` / `target-filtered` / `timeout` / `resource-failure` /
     `reference-toolchain-failure`
     —— 合并就等于把基建失败算成语义失败（`03` §3.3）；
  4. 超时与资源上限**只在 Test IR 里定义一次**，取值照 `M0-tests.md` §7.5，不在别处写死。

- **验证**（先 RED）：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
# 1) 先写 Test IR 字段齐全性与八种分类可枚举的测试 → 跑 → 应当【失败】（类型还不存在）
scripts/in-container.sh cargo test -p rgoc-harness
# 2) 最小实现 → 再跑 → 应当全绿
scripts/in-container.sh cargo test -p rgoc-harness
# 3) double_sum 仍存在且其测试通过（E5 复验锚点不能丢）
scripts/in-container.sh cargo test -p rgoc-harness double_sum
# 期望：test result: ok. 1 passed
```

### ✅ T31 完成记录（2026-10-02）

**RED → GREEN 走完一轮**，产物落在 `rgoc/crates/rgoc-harness/src/ir.rs`（**新增**）
与 `tests/test_ir.rs`（**新增**），`src/lib.rs` 只加了一行 `pub mod ir;`。

> **与计划的一处偏差**：计划写「文件路径：`src/lib.rs`」，实际把 Test IR 放进了
> **独立的 `src/ir.rs`**。理由：T32–T35 各自要建 `instruction.rs` / `corpus.rs` /
> `oracle.rs` / `compare.rs`，`ir.rs` 与它们同构；全塞进 `lib.rs` 会让文件迅速失控。

| 步骤 | 结果 |
|---|---|
| 1) RED | 写 11 条验收测试 → `E0432 unresolved import rgoc_harness::ir`（**只此一条**，失败确实来自类型不存在） |
| 2) GREEN | 实现 `ir.rs` → 11 集成 + 4 单元测试全绿 |
| 3) 锚点 | `cargo test -p rgoc-harness double_sum` 通过（D-M0-15：`double_sum` 保留） |
| 4) 门禁 | T22 四条全过：`fmt --check` / `check --workspace --all-targets` / `clippy -D warnings` / `test --workspace` |

**实现的取舍**：`validate()` **只校验 5 条**会让「结果不可信」的规则（空 ID、空路径、
比较器/模式错配、`errorcheck` 却期望成功、非 v0 模式缺 unsupported 说明）；
`imports` / `feature_deps` / `build_tags` 等字段目前**只记录不校验** ——
等 T33/T35 真正用到时再补，避免现在就写没人验证的分支。

**过程中三次踩坑**（都是「手改代码 > 用工具改代码」的代价）：
1. 测试里写了 `let mut 无 id = …` —— Rust 标识符**不能含空格**，混进了一个与 RED 无关的语法错误。
   **RED 必须纯净**：失败只能来自缺失行为，否则会把「夹具坏了」误当成「功能没实现」。
2. 批量给测试函数改 snake_case 时，正则多插了一个 `(`，把 `() {` 变成 `( {`、`Ok(())` 变成 `Ok()`。
   → 用正则批量改代码后**必须立刻编译一次**，不要攒着。
3. 三个测试函数名里含 `M0` / `T_H_01` / `512MiB` 等大写片段，触发 `non_snake_case` ——
   在 `clippy -D warnings` 下会**变成错误**并打爆 T22 门禁。中文函数名可以用，但**不能夹大写 ASCII**。

### 任务 T32：实现指令行解析（规则 R1）

- **文件路径**：`rgoc/crates/rgoc-harness/src/instruction.rs`（新建）、`lib.rs`

- **要做的**：按 `M0-tests.md` §1.3 的 **R1**（源码出处 `testdir_test.go:502-515`）实现：
  1. `.go` 文件不得以换行开头（`:497` 硬失败）；
  2. 从头逐行扫描，**跳过** `//go:build` 与 `// +build` 构建约束行；
  3. `action = TrimSpace(TrimPrefix(line, "//"))` ← 第一个「非空且非构建约束」的行；
  4. `action` 为空 → **硬失败**（`execution recipe not found`），不得静默跳过。

  推论要写成注释：**不能假定指令在第 1 行**，也不能假定它前面只有注释。

- **验证**（正反例齐备，`M0-tests.md` §3 的附加要求）：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh cargo test -p rgoc-harness instruction
# 期望：至少覆盖 —— 指令在首行 / 指令前有 //go:build 约束 / 指令前有空行与注释 /
#       未知指令硬失败 / 全是注释时硬失败
```

### ✅ T32 完成记录（2026-10-02）

RED → GREEN 走完一轮。产物：`rgoc/crates/rgoc-harness/src/instruction.rs`（新增）、
`tests/test_instruction.rs`（新增，**14 条**），`ir.rs` 补 `Mode::ALL`（T32 顺带加的，见下）。

| 步骤 | 结果 |
|---|---|
| 1) RED | 写 14 条验收测试 → `E0432/E0433 unresolved import rgoc_harness::instruction`（只此一条） |
| 2) GREEN | 实现 `instruction.rs` → 14 集成 + 7 单元全绿；`cargo test --workspace` 共 **32 条**全绿 |
| 3) 门禁 | T22 四条全过（`fmt` 需先跑 `cargo fmt`） |

**关键设计：把 R1b 的顺序契约写进函数签名**

```rust
pub fn dispatch(ins: &Instruction, platform_ok: bool) -> Result<Dispatch, DispatchError>
```

`platform_ok` 是**必填参数**而不是内部计算 —— 官方 `:522` 的平台过滤先于 `:541` 的
`switch`，若让调用方「先判指令再过滤」，这个顺序就可能被颠倒。
`Dispatch` 也不做成 `Mode`：官方在这条路上有**三个互不相同的出口**
（被平台过滤 / 上游设计即跳过 / 正常执行），把前两者塞进 `Mode` 会让
「这个用例到底跑没跑」变得不可读。

**实现中抓到的 4 个 bug**（都是测试逼出来的，注释里已记下）：

1. `strip_one_trailing_newline` 的 `then_some` **写反了** —— 「不含换行」时返回 `None`，
   于是**所有单行输入都被当成含换行**，构建约束一个也识别不出来（8 条测试同时红）。
2. 构建约束的长度比较**拿整行减前缀长度**算错了；比较基准必须是「前缀之后的那部分」。
3. **前缀判定用在了 trim 之后** —— 官方 `HasPrefix` 发生在 `TrimSpace` **之前**，
   所以缩进的 `  //go:build linux` **不是**约束。
4. `split_quoted` 里闭合引号后**漏了 `continue`** —— 闭合引号本身被塞进参数
   （得到 `"foo bar\""`）。根因是 Go 的 `case quote != 0` 排在 `case unicode.IsSpace` 之前，
   引号内的空格因此**不是**分隔符；Rust 的 `else if` 链很容易丢掉这个次序。

**另有 3 处是我的测试期望写错了**（实现是对的，已按官方语义改测试）：
- 源文件以换行开头会触发 `LeadingNewline`（`:497`），我的 fixture 撞上了它；
- 非注释首行 `package main` → 官方 `splitQuoted` 只取 `f[0]`，action 是 `"package"`；
- 「全是注释 → `NoRecipe`」我读错了规则：`action == ""` 指**空**注释行，
  非空的普通注释**就是**合法的 action（`linkmain.go` 即如此）。

**顺带加了 `Mode::ALL`**：「16 个指令名 + `skip` == 全部 17 个 Mode 变体」这条不变量
在类型层面查不出来（数组长度是编译期属性），有了 `ALL` 才能在**运行时**兜住
「加了变体却忘了加指令名」的漏项 —— 那种漏项会让真实语料被误判成未知指令。

**给 T33 的输入**：`parse_action` / `dispatch` / `is_go_build_line` / `is_plus_build_line`
都已就位；T33 只需实现**平台过滤本身**（`shouldTest` 的 build 约束求值 + GOOS/GOARCH），
然后把结论喂给 `dispatch(ins, platform_ok)`。**枚举出的分母必须等于 279**（T29 冻结值）。

### 任务 T33：语料枚举、样本选择与 unsupported 分类

- **文件路径**：`rgoc/crates/rgoc-harness/src/corpus.rs`（新建）

- **要做的**：
  1. 枚举 `go_source_code/test/` **顶层**（U9：子目录不在 M0 范围）与
     `src/internal/types/testdata/`（U11，M4 才纳入）—— 后者只统计不纳入；
  2. 按模式分派：`run` / `compile` / `errorcheck` 为 v0 支持集，其余 15 个指令按
     `M0-tests.md` §6 的 **U1–U12** 显式分类为 `expected-unsupported`，
     **每个不通过的用例都要能说清落在哪一条 U**；
  3. 排除参数用例（`-gcflags` / `-d` / `-goexperiment` / `-godebug`）→ U7；
  4. 实现平台过滤：build tag 不满足 → 判 `target-filtered`，**不计入分子、仍计入分母**
     （`03` §3.3；对应 T-H-04）。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh cargo test -p rgoc-harness corpus
# 期望：枚举结果确定（同样的语料 → 同样的文件集与计数，重复跑一致）
# 期望：M0 分母与 T29 冻结时记录的一致
```

### ✅ T33 完成记录（2026-10-02）

RED → GREEN 走完一轮。产物 `rgoc/crates/rgoc-harness/src/corpus.rs`（新增）、
`tests/test_corpus.rs`（新增，**15 条**），`ir.rs` 顺带补 `Mode` 的 `Ord` 与
`Comparator::for_mode`。

**★ 本任务的核心是那条交叉校验：枚举真实语料，算出的分母必须等于 279。**
T29 冻结的文档口径 vs T33 的代码实现，两者对上才算数。结果：

```text
total 356 · denominator 279（run 147 / compile 12 / errorcheck 120）
executed 274（分母内真正参与执行）· target_filtered 5 · excluded 77
U7 31 · U2 14 · U6 11 · U5 9 · U1 5 · U13 5 · U3 1 · U14 1
```

**实现中抓到的 4 个 bug**（都是测试逼出来的）：

1. **U7 只检查了 `action`，而开关在 `args` 里** —— `parse_action` 按官方 `splitQuoted`
   把 `// errorcheck -d=panic` 切成 `action="errorcheck"` + `args=["-d=panic"]`。
   结果 **31 个 U7 文件全漏掉，分母从 279 虚到 310**，另多出一个 11 项的「?」桶。
2. **旧式 `// +build` 的 `!` 取反没实现** —— `// +build !windows` 本该为真，却算成假。
3. **「被平台过滤 ⇒ 不进排除清单」这条捷径是错的**。平台过滤与 U 归类是**正交**的两件事：
   分母口径是「v0 集 ∧ 无排除参数」，与过滤无关。`checkbce.go` 被排除是因为它带 `-d=`
   （U7），不是因为它被过滤（它同时有 `//go:build amd64`）。修正后 11 个
   「既带排除参数又带构建约束」的文件各归其位（U7/U5/U6/U1）。
4. **U7 归因缺 `is_v0_supported` 守卫** —— 非 v0 模式若也带 `-gcflags`
   （如 `errorcheckwithauto`）会被抢到 U7，冻结口径是 **U7 只收 v0 集内**的用例
   （「模式本身不支持」比「带内部开关」更根本）。修正后 U6 回到 11、U7 回到 31。

顺带把「`// skip` 是特殊的」抽成 `instruction::is_skip()` —— 原先只有 `dispatch`
内联知道，语料归类又写了一遍，等于两处各写一次同一个特殊规则。

**测试里也有 2 处口径写错**（已改）：
- 「被平台过滤的 5 个」原先在**全部文件**里筛，把 11 个 U7 文件也算进去了 →
  改为只在**分母内**筛（`in_denominator(f) && f.target_filtered`）；
- 「未知 action 一律报硬错误」漏了 `// skip` → 官方对它 `t.Skip` 而非 `Fatalf`。

**平台过滤的实现**（`shouldTest`，`:380-467` 逐条对齐）：
- tag 判定：`ReleaseTags`（go1.1…go1.27）→ `goexperiment.*` 查 `ToolTags` →
  `cgo`（`-cgo` 关）→ `GOOS`/`gc` → `GOARCH` → `gcflags_noopt`（`GO_GCFLAGS` 空）→ `test_run`
- ⚠️ **`ToolTags` 只对 `goexperiment.` 前缀查** —— 写「任何名字都查」会让
  `arm64.v8.0` 被错判为真
- `//go:build` 表达式：`&&` / `||` / `!` / 括号 / tag（递归下降，`&&` 优先于 `||`）；
  多行之间 AND；`//go:build` 存在时**优先**于旧式 `// +build`
- 旧式 `// +build`：空格 = OR，逗号 = AND，`!` 取反，多行 AND
- **解析失败只让该行不参与**（官方 `constraint.Parse` 出错后 `continue`），
  不会把用例判死
- tag 集合是 `go1.27.1` 的**实测值**（容器内 `go list` / `go/build.Default`），
  不是猜的；换 oracle 版本必须重新采集

**给 T34 的输入**：`enumerate` 产出 `CorpusReport`（分母 / 八类分布 / 逐文件归类），
`should_test` 已就位。T34 只需实现 **oracle 调用 + 版本守门**（T-H-06）。

### 任务 T34：oracle 调用与版本守门

- **文件路径**：`rgoc/crates/rgoc-harness/src/oracle.rs`（新建）

- **要做的**：
  1. oracle 就是**容器内精确的 `go1.27.1`**（硬约束，D-M0-1）：调用前先校验 `go version`；
  2. **版本不符即拒绝**（T-H-06，`04` §7）：报 `reference-toolchain-failure`，
     **不得**降级用宿主 `go1.24.5` 悄悄跑；
  3. 每条用例带**超时与 RSS 上限**（`M0-tests.md` §7.5：T-C 单项 60 s / 整层 5 min；
     单用例峰值 RSS ≤ 512 MiB，`T-C-03` 放宽到 768 MiB）；
  4. 超时后**真正终止子进程**（T-H-05 要求验证进程被回收，不能只 `kill` 父进程）。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh cargo test -p rgoc-harness oracle
# 期望：① 正常调用返回结果 ② 伪造版本不符时判 reference-toolchain-failure
#       ③ 超时用例返回 timeout 且事后 `ps` 查不到残留子进程
```

### ✅ T34 完成记录（2026-10-03）

RED → GREEN 走完一轮。产物 `rgoc/crates/rgoc-harness/src/oracle.rs`（新增）、
`tests/test_oracle.rs`（新增，**12 条**），`ir.rs` 顺带补 `Limits::with_per_case_override`。

| 步骤 | 结果 |
|---|---|
| 1) RED | 12 条验收测试 → `E0432 unresolved import rgoc_harness::oracle` |
| 2) GREEN | 13 单元 + 12 集成全绿；`cargo test --workspace` 共 **65 条**全绿 |
| 3) 门禁 | T22 四条全过（`fmt --check` / `check` / `clippy -D warnings` / `test`） |

**约束反过来改善了设计：`run` 层走官方的 fast path**

原本打算用 `go run`（`:1079`），但 T-H-05 要求「超时后真正终止子进程」。`go run` 的子进程是
`go`，它再拉起被测程序 —— 只 kill `go` 会留下**孤儿进程**。要杀整组得用
`kill(-pgid, SIGKILL)`，那需要 `unsafe`，而本仓 workspace 是 **`unsafe_code = "forbid"`**。

于是改走官方**首选**的 fast path（`:1069-1077`）：`go tool compile` → `go tool link` → **直跑 exe**。
父子关系是直的，`child.kill()` 就等于杀掉被测程序本身 —— 既忠实于官方（无 flags 时官方也走这条），
又完全不需要 unsafe。**约束把设计推向了一个更好的选择。**

**实现中抓到的 5 个 bug**：

1. **`parse_go_version` 的格式假设错了** —— `go version go1.27.1 linux/arm64` 里
   平台是**斜杠连写的一个 token**，我按空格分隔去取第 2、3 段，于是拿到
   `"linux/arm64"` 和 `None`，**版本守门对所有输入都失败**。查了半天才发现
   （用 `rustc` 单独跑最小复现才确认 Rust 没问题，是我的假设错了）。
2. **`if start >= deadline` 恒为 false** —— `deadline = start + per_case`，而 `start`
   是个不再变化的快照，**永远小于** `start + per_case`。正确写法是 `Instant::now() >= deadline`。
   症状：30 秒的 sleep 跑满全程都没触发超时。是插桩打印 `deadline_in=0ns` 才看出来的。
3. **fast path 的 exe 步骤被当成 `go` 的参数** —— 得到
   `go /tmp/xxx.exe: unknown command`。根因是「每一步都是 `go` 的子命令」这个假设，
   于是引入 `Step { program, args }`：最后一步的 program 是 exe 本身。
4. **中间步骤的失败被吞掉** —— `run_mode` 最后硬编码 `exit_code: Some(0)`，
   于是 errorcheck 样本明明编译失败却报成功。改为「以最后一步的退出码为准」。
5. **并发下 importcfg 互相覆盖** —— 固定路径 `/tmp/m0-importcfg-<pid>`，
   10 个实例同时建就随机失败（单跑必过、并行挂）。改为**实例级唯一**的临时目录；
   顺带把 fast path 的 `pkg.a` / `exe` 也挪进那里 —— **它们绝不能写进 work_dir**，
   真实语料（`GOROOT/test`）是**只读挂载**的。

**测试里也有 2 处问题**：

- RSS fixture 只写不读 `b[i] = 1`，被 Go 当成 **dead store 优化掉**，页面从未被触碰，
  峰值只观测到 13 MiB；必须**读回一个值**，并停留 300 ms 让轮询采得到
  （轮询采样的固有限制已写进 `read_peak_rss` 的注释：短命进程可能采不到，
  要精确得用 `wait4` 的 rusage，那需要 unsafe）；
- 超时测试的计时器把 `Oracle::new` 也算进去了 —— 而 `go list -export std` 在高并发下
  实测能到 **45 s**（std 导出缓存）。400 ms 的用例预算与它无关，改为只量 `run_mode`。

**给 T35 的输入**：`Oracle::run_mode` 给出 `merged`（**stdout+stderr 合并流**）、
`exit_code`、`timed_out`、`resource_exceeded`、`peak_rss_bytes`。
T35 只需实现「拿这些去比 `.out` / 诊断」。

**顺带修掉自检的一个根本缺陷**：`code_only()` 原先只剥 `#` 开头的行（为 shell 写的），
对 Rust 的 `//` 注释**完全无效** —— 也就是说所有针对 Rust 文件的断言一直在
「含注释的文本」上匹配。已补上 `//`，并反向验证（把真代码改成注释形态 → 断言报 ✗）。

### 任务 T35：比较器与诊断切分（规则 R2 / R3 / R4）

- **文件路径**：`rgoc/crates/rgoc-harness/src/compare.rs`（新建）

- **要做的**（三条规则都带源码出处，**不能凭直觉实现**）：
  - **R2 输出期望**（`testdir_test.go:1169-1193`）：期望文件在 **goroot 的 test 目录**下查找
    （不是测试文件所在目录）；`.out` 存在则比对，**不存在则期望为空**；比较前把 `\r\n` 归一为 `\n`；
    **严格相等**；不匹配时区分两种措辞（有 `.out` = 内容不符 / 无 `.out` = 本应为空却非空）；
  - **R3 诊断切分**（`:1195-1213`）：`\t` 开头追加到上一行；以 `go tool` / `#` / `<autogenerated>`
    开头的行跳过；仅空白行跳过；其余非空行各自成条；
  - **R4 ERROR 注释**（`:1432-1499`）：支持 `// ERROR` 与 `// GC_ERROR`、
    `ERRORAUTO`、**一行多个引号模式**（`mainsig.go` 的 `// ERROR "..." "..."` 是两条独立期望）、
    `LINE` / `LINE+n` / `LINE-n` 替换为 `文件:行号`；含 `////` 的行跳过。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh cargo test -p rgoc-harness compare
# 期望：\r\n 归一 / 缺 .out 判空 / 严格相等不匹配 / 续行拼接 / 多引号双期望 /
#       LINE±n 替换 / \\\\ 禁用 全部有正反例
```

### 任务 T36：六类自测全绿（**E3**）

- **文件路径**：`rgoc/crates/rgoc-harness/tests/t_harness.rs`（新建）

- **要做的**：把 `M0-tests.md` §3 的六类逐条落为测试，**正反例齐备、不依赖网络**：

| ID | 类别 | 构造 | 期望 |
|---|---|---|---|
| `T-H-01` | 成功 | 一个合法 `// run` 用例 | `pass`；退出码 0；stdout 与 `.out` 一致 |
| `T-H-02` | 失败 | 与 `.out` 不符 / 非零退出码 | 判 `compiler-failure` 或 `runtime-failure`，**不是 harness 崩溃** |
| `T-H-03` | 未知指令 | 首行 `// notarealpattern` | 判 `harness-failure` 并报未知指令；**绝不静默跳过、不记 pass** |
| `T-H-04` | 平台过滤 | 首行 `//go:build windows` | 判 `target-filtered`，**不计入分子、仍计入分母** |
| `T-H-05` | 超时 | 超时 fixture | 判 `timeout` 且**子进程被真正回收** |
| `T-H-06` | 不匹配版本 | 伪造 `go version` | **拒绝**该 oracle，判 `reference-toolchain-failure` |

  另：`double_sum`（T31 保留）作为 `T-H-01` 的**进程内**补充正例，说明「成功」不只来自 Go 用例。

- **验证**（**E3 门禁**）：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh cargo test -p rgoc-harness --test t_harness
# 期望：T-H-01..06 全绿，且正反例齐备
scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo test -p rgoc-harness --test t_harness 2>&1 | tail -3'
# 期望：test result: ok. 6 passed（E3 达成）
```

### 任务 T37：`rgoc-driver` CLI 骨架与 `xtask`

- **文件路径**：`rgoc/crates/rgoc-driver/src/main.rs`（新建 crate）、`rgoc/xtask/src/main.rs`（新建 crate）、`rgoc/Cargo.toml`（改 members）

- **要做的**：
  1. `rgoc-driver`：统一 CLI 入口（M0 只需能调度 harness 与将来的三个 spike）——
     子命令先只留 `harness`（`run` / `list` / `report`），**不预留**未实现的子命令；
  2. `xtask`：三个职责 —— 语料枚举、报告生成、**环境 manifest 生成**
     （把 `M0-manifest.json` 的 `environment` 节从手填改为可重放生成）；
  3. 两个 crate 的目录名**不能叫 `crate`**（Rust 关键字，`cargo new` 会拒绝 —— Phase 0 已踩过）。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh cargo run -p rgoc-driver -- harness list
# 期望：列出冻结的 20 个 T-C 样本及其模式
scripts/in-container.sh cargo run -p rgoc-driver -- harness run --all
# 期望：跑完并给出结果统计（分子/分母/八类分布）
```

### 任务 T38：跑 20 个官方样本并出报告（**E4**）

- **文件路径**：`rgoc/tests/corpus/`（放本次报告）、`docs/milestones/M0-manifest.json`（登记）

- **要做的**：对 T-C-01..20 逐个执行，按 `M0-tests.md` §4.5 的比较器规格判定，
  产出**可重放**的报告（哪些通过、哪些按哪条 U 排除、分母是多少、耗时与峰值 RSS）。
  遵守 §8 的判定纪律：**不得**通过放宽比较器、把跳过记为 pass、或缩小分母来「达成」门禁。

- **验证**（**E4 门禁**）：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh cargo run -p rgoc-driver -- harness run --all
# 期望：T-C-01..20 全部 pass（20/20），整层耗时 ≤ 5 min，单项 ≤ 60 s
# 期望：报告中分母 = 20，且没有任何一条是靠「排除」消失的
```

> ⚠️ 若某个样本**确实跑不通**：先判定是 harness 的缺陷还是样本超出 M0 范围。
> 属于范围的**修 harness**，不属于的**登记为新的 U 条**并记在报告里 ——
> **不能沉默地把它从分母里拿掉**。

### 任务 T39：Phase 2 门禁复核与登记

- **文件路径**：`docs/milestones/M0-manifest.json`（改）、`AGENTS.md`（改）

- **要做的**：把 E3 / E4 的证据登记进 manifest（`gate.E3` / `gate.E4`），
  回写 `AGENTS.md` 的阶段状态；跑一遍 `AGENTS.md` §6.1 的自检。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo fmt --all -- --check && cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace'
# 期望：四条全过（`03` §6.3 第 2 条）
python3 scripts/check-m0-consistency.py    # 期望：全部通过，exit 0
```

---

## 5. Phase 3 —— 三个架构 spike（门禁 E6 + E7）

> **前置**：Phase 2 的 E3 / E4 已通过 —— **环境门禁未过不得开工**（D-M0-2）。
> **crate 边界**（D-M0-13）：本 Phase 才建 `rgoc-hir`（最小、标 `SPIKE-ONLY`）与 `rgoc-spikes`。
> **spike 的隔离形态**（决策 **D-M0-14**）：三个 spike 放在**独立 crate `rgoc-spikes`** 下，
> 三个 bin + 共享的固定 HIR fixture。理由是 `03` §4 的纪律「**不盲目演进临时代码**」——
> 隔离成一个 crate 后，后续用测试驱动的正式实现替换时**可以整块删掉**，不会与正式代码纠缠。

### 任务 T40：建 `rgoc-hir`（最小、标 `SPIKE-ONLY`）

- **文件路径**：`rgoc/crates/rgoc-hir/Cargo.toml`（新建）、`rgoc/crates/rgoc-hir/src/lib.rs`（新建）

- **要做的**：三个 spike 共享的**固定 HIR**，只覆盖 spike 所需子集
  （M0 **不做**真实 Go 源码的 lex/parse —— U12）。
  必须在 crate 头部与 `docs/contracts/` 两处标注 **`SPIKE-ONLY`，M5 替换**（D-M0-11）。
  C1 契约在此处**只留位**：位置表示与诊断排序规则固定下来，但**不得**被误读为「M0 已实现源码位置跟踪」。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh cargo test -p rgoc-hir
# 期望：crate 自身测试通过；且 `grep -rn "SPIKE-ONLY" rgoc/crates/rgoc-hir/` 有命中
```

### 任务 T41：建 `rgoc-spikes` crate 骨架

- **文件路径**：`rgoc/crates/rgoc-spikes/Cargo.toml`（新建）、`src/bin/s1_interp.rs`、`src/bin/s2_ssa.rs`、`src/bin/s3_native.rs`、`src/fixtures/mod.rs`

- **要做的**：三个 bin 各留一个**会失败的**占位（RED），共享 fixture 模块放固定的 HIR
  （`println(1+2)` 与输出 `hello` 的 native 函数）。三个 spike 的**输入必须写死**，
  不允许从命令行传入可变输入 —— 否则「可复现」无从判定（E6）。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh cargo build -p rgoc-spikes
# 期望：三个 bin 都能构建（此时它们还只会返回失败）
```

### 任务 T42：S1 —— 解释 spike（`T-S1-01` … `T-S1-03`）

- **文件路径**：`rgoc/crates/rgoc-spikes/src/bin/s1_interp.rs`

- **要做的**：宿主求值固定 HIR（`println(1 + 2)`）→ stdout。
  记录：**值表示、内建调用边界、输出流走法**。
  `T-S1-03` 要与 `T-C-04`（printbig）的语义对齐 —— 即值表示不能只在 64 位上凑巧成立。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh cargo run -p rgoc-spikes --bin s1_interp
# 期望：stdout 精确为 "3\n"（字节级比较，不是「包含 3」），退出码 0
```

### 任务 T43：S2 —— SSA spike（`T-S2-01` … `T-S2-03`）

- **文件路径**：`rgoc/crates/rgoc-spikes/src/bin/s2_ssa.rs`

- **要做的**：对**同一份**固定 HIR 构造 Block / Value 图并求值。
  **两条路线交叉验证是重点**：`T-S2-01` 的求值结果必须与 `T-S1-01` 一致。
  `T-S2-02` 必须**显式列出** memory / tuple / 调用边界的需求（`03` §4 第 5 条要求识别这三项）；
  `T-S2-03` 记录与 M6 完整 verifier 的差距（不在 M0 实现 verifier）。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh cargo run -p rgoc-spikes --bin s2_ssa
# 期望：输出 "3\n"，且与 S1 的输出逐字节相同；需求清单已打印并落盘
```

### 任务 T44：S3 —— native spike（`T-S3-01` … `T-S3-04`）

- **文件路径**：`rgoc/crates/rgoc-spikes/src/bin/s3_native.rs`、`rgoc/runtime/native/aarch64-unknown-linux-gnu/`（新建）

- **要做的**：固定 HIR/SSA 函数 → arm64 汇编 → 经容器内 `clang` 链接成 ELF。
  **六项都要有记录**（`T-S3-04`）：调用约定、**栈对齐**、输出流、退出码、最小 runtime 桥接、unwind 边界。
  不设首发 `rgoc-linker`（`03` §2）；生产代码不把官方 `.s` 直接喂给系统汇编器。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh cargo run -p rgoc-spikes --bin s3_native
# 期望：产出 ELF；file 输出含 "ELF 64-bit LSB" 且含 "ARM aarch64"
scripts/in-container.sh bash -lc '/work/rgoc/target/s3-hello; echo "exit=$?"'
# 期望：hello
#       exit=0
```

### 任务 T45：可复现性验证（**E6**）

- **文件路径**：`docs/milestones/M0-benchmarks.md`（追加 §12）

- **要做的**：三个 spike 各重复执行 **3 次**，比对三次的
  **输入、结果、环境**（Go 版本、Rust 版本、镜像 digest、clang 版本）是否完全一致。
  把实测数据写进 benchmarks（E10 的 benchmarks 节同理扩充）。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
for i in 1 2 3; do scripts/in-container.sh cargo run -p rgoc-spikes --bin s1_interp; done | sort -u | wc -l
# 期望：1（三次结果完全一致）
# 同法验证 s2_ssa 与 s3_native（T-S1-02 / T-S3-05）
```

### 任务 T46：native `hello` 登记为 M1 smoke 回归项（**E7**）

- **文件路径**：`docs/03-roadmap.md`（改 M1 小节）、`docs/milestones/M0-manifest.json`（改）

- **要做的**：把 S3 的 `hello` fixture 写进 M1 的 smoke 清单，注明来源是 M0 的 `T-S3-03`。
  E7 的判定是「**可运行 + 已登记**」两件事，缺一不可。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
grep -n "hello" docs/03-roadmap.md | head
# 期望：M1 小节里出现该 smoke 项，且注明来自 T-S3-03
```

### 任务 T47：Phase 3 门禁复核与登记

- **文件路径**：`docs/milestones/M0-manifest.json`（改）、`AGENTS.md`（改）

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
python3 scripts/check-m0-consistency.py
# 期望：gate.E6 / gate.E7 = pass，且全部断言通过
```

---

## 6. Phase 4 —— 契约初稿与交付报告（门禁 E8 + E9 + E10 收口）

> 本 Phase **不再产生新行为**，只做两件事：把 spike 验证出的形状**固化为可版本化契约**，
> 以及交付可复核的报告。D-M0-11 的 `SPIKE-ONLY` 标注在 M5 替换时解除。

### 任务 T48：C1 契约初稿（SourceMap / 位置 / 诊断排序）

- **文件路径**：`docs/contracts/C1-source-map.md`（新建）

- **要做的**：**留位**。M0 不做真实 lexer，只在 Test IR 中固定位置表示与诊断排序规则。
  **文件头必须显式写「留位」** —— 不得因为文件存在就被误读为「M0 已完成源码位置跟踪」（`M0-design.md` §7）。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
head -20 docs/contracts/C1-source-map.md | grep -n "留位"
# 期望：命中（文件头就写明）
```

### 任务 T49：C2 契约初稿（Test IR / 构建条件 / 比较器 / 结果格式）

- **文件路径**：`docs/contracts/C2-test-ir.md`（新建）

- **要做的**：**完整初稿**（消费者：所有阶段）。从 T31–T35 的实现**反推**成契约：
  Test IR 必录字段、构建条件判定、比较器规格、八种结果分类、超时与资源上限。
  这一份是 M0 唯一「完整」级契约，写好后它就是后续所有阶段的公共依赖。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
grep -c "" docs/contracts/C2-test-ir.md     # 期望：非空且成体系
grep -n "SPIKE-ONLY\|留位\|完整初稿" docs/contracts/C2-test-ir.md   # 期望：标注为「完整初稿」
```

### 任务 T50：C3 契约初稿（HIR 多返回值 / 可寻址性 / 闭包 / 异常传播）

- **文件路径**：`docs/contracts/C3-hir.md`（新建）

- **要做的**：**spike 级**，从 S1/S2 验证出的形状反推。必须写清哪些是**已验证**、
  哪些是**待 M5 验证**，不得把推测写成结论。

- **验证**：同 T48 的形式检查（文件存在 + 标注级别 + 列出未验证项）。

### 任务 T51：C4 契约初稿（SSA tuple/memory / Phi / 支配关系 / 调用边界）

- **文件路径**：`docs/contracts/C4-ssa.md`（新建）

- **要做的**：**spike 级**，从 S2 反推；消费方是 codegen 与 liveness（`M0-design.md` §7）。

### 任务 T52：C5 契约初稿（ABI / frame layout / 对象布局 / runtime symbol bridge）

- **文件路径**：`docs/contracts/C5-abi.md`（新建）

- **要做的**：**spike 级**，从 S3 反推（M7 做 MVP、M9 完整化）。
  必须包含 S3 记录的六项：调用约定、栈对齐、输出流、退出码、runtime 桥接、unwind 边界。

### 任务 T53：交付报告 `M0-report.md`

- **文件路径**：`docs/milestones/M0-report.md`（新建）

- **要做的**：面向「下一个接手的人」，必须包含：
  ① 三个命题各自被**哪个测试 ID** 证明（`M0-design.md` §1.1）；
  ② 每一项门禁的证据出处；
  ③ **spike 验证出的形状与留下的差距**（含 S2-02 的需求清单、S2-03 与 M6 的差距、S3-04 的六项）；
  ④ 明确**未验证/超出范围**的部分（U1–U12）；
  ⑤ 接手指南：下一阶段该先读哪几份文档。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
grep -n "T-S1-01\|T-S3-04\|U1\b\|U12" docs/milestones/M0-report.md
# 期望：三类引用都能查到（证明报告不是空话）
```

### 任务 T54：`M0-manifest.json` 完整化

- **文件路径**：`docs/milestones/M0-manifest.json`（改）

- **要做的**：填掉现在还是空对象的三个节：
  `test_ids`（T-H / T-C / T-S 全部 ID 与判定结果）、`unsupported`（U1–U12 及各自的实际样本数）、
  `budget`（实测耗时与 RSS 上限）。
  同时把 `benchmarks` 节从 Phase 0 的五节扩到含 Phase 3 的可复现性数据。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
python3 -c "import json;d=json.load(open('docs/milestones/M0-manifest.json'));print(len(d['test_ids']),len(d['unsupported']),sorted(d['budget']))"
# 期望：test_ids 非空（≥ 33）、unsupported 12 条、budget 三节齐
```

### 任务 T55：六条统一退出检查（**E9**）+ M0 全门禁复核

- **文件路径**：`AGENTS.md`（改状态）、`docs/milestones/M0-plan.md`（改门禁汇总）

- **要做的**：`03` §6.3 的**六条**逐条判定并记录证据：
  1. 当前阶段必需 smoke 全绿，无未分类 / 基建 / 不稳定失败；
  2. 四条 cargo 命令（`fmt --check` / `check --workspace --all-targets` /
     `clippy --workspace --all-targets -- -D warnings` / `test --workspace`）全过，
     目标集与 native verifier / contract checks 也过；
  3. 环境、报告、复现材料与契约版本齐全，保留 unsupported 清单；
  4. 新功能都归属当前冻结测试集或已登记风险；
  5. 更改 ABI / 布局 / 基线 / 必需用例的决策、迁移与回归影响有记录；
  6. 不可行时先缩减范围，**不把 recoverable failure 当完成**。

  然后复核 E1–E10 全部状态，回写 `AGENTS.md`。

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo fmt --all -- --check && cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace'
# 期望：四条全过
python3 scripts/check-m0-consistency.py
# 期望：全部通过；gate 里 E1–E10 全部 pass
python3 scripts/debug-smoke-test.sh 2>/dev/null || docker exec <容器名> bash /work/scripts/debug-smoke-test.sh
# 期望：A/B/C + 9 项全过（E5 的复验锚点仍在，见 T31）
```

**M0 全部门禁通过后**，Phase 2–4 才算结束；`03` §4 的 M1 才具备开工条件。

---

## 7. 门禁汇总（全部 Phase）

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
| **E5** | T28 的四项检查全部 ☑（**实测断点命中**） | **✅ 通过**（2026-10-02） | **实测**：用户在 VSCode dev container 中按 F5 运行「调试当前测试 (CodeLLDB)」，T28 四项逐项通过（断点命中未被跳过 / 变量面板 `a=1,b=2` / 调用栈 ≥2 帧 / F10 后停在第 28 行且 `sum==3`），登记在 `M0-manifest.json` 的 `gate.E5`。**下层印证**：`scripts/debug-smoke-test.sh` 第 2 节 **A/B/C 三项 + 9 项断言通过、exit 0**（断点解析到 `double_sum + 20 at lib.rs:27`、`stop reason = breakpoint 1.1`、调用栈 3 帧、`a=1`/`b=2`、`step over` 停到 `lib.rs:28`、`sum = 3`、`1 passed; 0 failed`）；第 2 节的 A/B/C 即「编辑器链路」在下层的彩排（其复刻行为由自检第 5c 节 6 条断言守住） |

**Phase 1 门禁已通过（2026-10-02）—— 三项收尾的实际状态**：

| # | 事项 | 状态 |
|---|---|---|
| 1 | 交付用户可用的 VSCode 调试环境（用户明确要求的那一项） | ✅ **已交付**：`.devcontainer/` + `.vscode/launch.json` + 两个离线安装脚本，用户在容器内按 F5 实测断开成功 |
| 2 | 基于**实测到的环境事实**回头拆 **Phase 2–4** 的计划 | ⏳ **待办**（前置条件已满足，见 §0.1 的状态更新） |
| 3 | 更新 `M0-manifest.json` 与 `M0-plan.md` 的执行状态 | ✅ **已完成**：`gate.E5 = pass`（含 `confirmed_at`/`confirmed_by`）、本节的检查表与门禁汇总表均已回写 |

**下一步（Phase 2 开工前）**：① 冻结 `M0-tests.md`；② 拆 Phase 2–4 计划；③ 跑一遍 `AGENTS.md` §6.1 的自检。

### Phase 2 出口（⏳ 待开工）

| 门禁 | 判定方式 | 由哪些任务 | 状态 |
|---|---|---|---|
| **E3** | harness 六类自测全绿（正反例齐备、不依赖网络） | T36（`T-H-01`~`T-H-06`） | ⏳ 待开工 |
| **E4** | ≥20 个官方样本 **100%** 通过（**分母不得缩小**） | T38（`T-C-01`~`T-C-20`） | ⏳ 待开工 |

> **硬前置**：T29（冻结 `M0-tests.md`）必须先做 —— `03` §3.3 的门禁纪律要求「白名单**开工前**冻结」，
> `M0-tests.md` §0 的 F1/F2/F3 三项都指向这一点。

### Phase 3 出口（⏳ 待开工）

| 门禁 | 判定方式 | 由哪些任务 | 状态 |
|---|---|---|---|
| **E6** | 三个 spike 的输入 / 结果 / 环境**全部可复现**（各重复 3 次一致） | T45（`T-S1-02` / `T-S3-05`） | ⏳ 待开工 |
| **E7** | native `hello` 可运行（`T-S3-03`）**且已登记**进 M1 smoke | T46 | ⏳ 待开工 |

> **前置**：Phase 2 的 E3 / E4 必须先过（D-M0-2：harness 门禁未过不得开工内容）。

### Phase 4 出口（⏳ 待开工）

| 门禁 | 判定方式 | 由哪些任务 | 状态 |
|---|---|---|---|
| **E8** | `docs/contracts/` 下 **5 份**契约初稿（C1 留位 / C2 完整 / C3–C5 spike 级） | T48–T52 | ⏳ 待开工 |
| **E9** | `03` §6.3 的**六条**统一退出检查 | T55 | ⏳ 待开工 |
| **E10** | 基准数据（含 Phase 3 的可复现性）进 manifest | T45 / T54 | ⏳ 待开工 |

> C1 **必须显式标注「留位」** —— 不得因为文件存在就被误读为「M0 已完成源码位置跟踪」。

---

## 8. 执行纪律

继承 `../03-roadmap.md` §3.5 与 `AGENTS.md` §4：

| 纪律 | 说明 |
|---|---|
| **规格未确认不动手** | 本计划经确认后才开始 T01 |
| **RED → GREEN → REFACTOR** | 本计划中 T21 之后的每个行为改动都应先有失败测试；环境类任务（T01–T19）以「验证命令」代替测试 |
| **没有失败的测试就不算实现** | 环境任务至少要有**可判定的验证命令**，且必须真的跑过 |
| **不盲目演进临时代码** | 三个 spike 一律放独立 crate `rgoc-spikes`（D-M0-14），可整块删除；**例外**：T21 的 `double_sum()` **不删** —— 它是 E5（人工门禁）的复验锚点，改为当 T-H-01 的 fixture（D-M0-15）|
| **提交仅在明确要求时执行** | 每个任务完成后**不自动提交**；提交信息须引用任务号与验证证据 |
| **失败不掩盖** | 任何验证失败都要记录原始报错与复现方式，不得通过放宽断言、换镜像 tag 等方式「绕过」 |

**遇到阻塞时的处理顺序**：

1. 记录原始报错（命令 + 完整输出）；
2. 判断属于哪类：环境 / 工具链 / 网络 / 计划本身有误；
3. 网络类 → 重试 3–4 次（`github.com` 类故障是间歇性的）；
4. 工具链类 → 回到对应任务修改，**不要绕过门禁**；
5. 计划有误 → **回头改计划**（Superpowers：计划变了就改计划，不要悄悄改）。

---

## 9. 变更记录

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
| 2026-10-02 | **E5 通过**：`M0-manifest.json` 的 `gate.E5` 由 `pending` 改为 `pass`，补 `confirmed_at` / `confirmed_by`；T28 检查表四项补实测列；门禁汇总表与任务总表回写状态；`check-m0-consistency.py` 的门禁断言由「E1/E2/E10=pass, E5=pending」改为「四条全部 pass」并新增「`gate.E5` 登记了确认人与确认时间」（**52 → 53 条断言**） | **用户在 VSCode dev container 中按 F5 实测确认 T28 四项通过**（断点命中未被跳过 / 变量面板 `a=1,b=2` / 调用栈 ≥2 帧 / F10 后停在第 28 行且 `sum==3`）。这是 M0 最后一项门禁 —— 至此 **E1/E2/E10/E5 全部通过**，Phase 0 与 Phase 1 完成。**门禁状态是硬事实，必须与实测同步**：改了 `gate` 就要同步自检里的断言与全部文档，否则自检会红（本轮 `AGENTS.md` 的断言数就又被 §6 抓到一次） |
| 2026-10-02 | **拆解 Phase 2–4 计划**：新增 §4 Phase 2（T29–T39）、§5 Phase 3（T40–T47）、§6 Phase 4（T48–T55）；原 §4/§5/§6 顺延为 §7/§8/§9（5 处「见 §6 变更记录」自引用同步修正）；§0.1 改为「现已全部拆完」并说明当初为什么先不拆；§0.3 任务总表补三行；§7 补 Phase 2/3/4 出口门禁表；执行纪律的 `double_sum` 条款按 D-M0-15 修正 | **E5 已通过，「等门禁再拆」的前置条件满足**。两处结构性问题由用户拍板：crate 落地节奏（D-M0-13，严格按当期需要）、spike 隔离形态（D-M0-14，独立 crate `rgoc-spikes`）。**同时查出一处既有计划缺陷**：「Phase 2 删除 `double_sum`」会破坏 E5 的人工可复验性 → 改为保留并当 T-H-01 的 fixture（D-M0-15） |
| 2026-10-02 | 自检新增 2 条断言（**53 → 55**）：「M0-plan 门禁汇总表的状态 == manifest 的 gate（双向）」与「M0-plan 任务总表里 T28 的状态 == gate.E5」 | **本轮发生一次真实文档漂移**：上一轮把 `gate.E5` 改成 `pass` 后，`M0-plan.md` 的门禁汇总表被回退成「☐ 待人工」，而当时自检只校验 manifest，**全绿放行** —— manifest 是机器可读事实、文档是人读入口，两者不一致时人会以文档为准。新断言按**双向**校验（pass↔✅、pending↔待人工），并已**人工反向验证**（把 E5 行改回「☐ 待人工」→ 报 `✗ E5=manifest.pass 但文档行不是 ✅`） |
