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
def walk(o, path=''):
    if isinstance(o, dict):
        for k, v in o.items(): yield from walk(v, f'{path}.{k}')
    else:
        yield path, o
bad = [(p, v) for p, v in walk(env)
       if v in ('', None, 0) or (isinstance(v, str) and ('<' in v or 'unknown' in v))]
print('EMPTY/PLACEHOLDER FIELDS:', bad if bad else 'none')
sys.exit(1 if bad else 0)
PY
# 期望：EMPTY/PLACEHOLDER FIELDS: none，退出码 0
```

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
| 首次构建（含拉取基础镜像） | <分钟:秒> |
| 二次构建（全缓存命中） | <秒> |
| 镜像体积（含全部层） | <MB> |
| 磁盘占用增量 | <MB> |

## 2. 容器冷启动

| 指标 | 实测值 |
|---|---|
| `docker run --rm rgoc:dev true` | <毫秒> |
| `docker run --rm rgoc:dev rustc --version` | <毫秒> |
```

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
docker images rgoc:dev --format '{{.Size}}'
time (docker build -f docker/Dockerfile -t rgoc:dev . | tail -1)
time (docker run --rm rgoc:dev true)
```

---

### 任务 T18：基准测试 B —— bind mount vs 命名卷（D-M0-9 的待决点）

- **文件路径**：`docs/milestones/M0-benchmarks.md`（追加）

- **要做的**：这是 `M0-design.md` §3.3 第 4 项「`target/` 放哪」的**决策依据**。分别测两种布局的 `cargo build` 耗时。

```markdown
## 3. bind mount vs 命名卷（决定 target/ 的最终位置，D-M0-9）

创建同一个最小 crate，分别以两种方式挂载 `target/`，各跑 3 次 `cargo build`（先 `cargo clean`）。

| 布局 | 第 1 次（冷） | 第 2 次 | 第 3 次 | 结论 |
|---|---|---|---|---|
| `target/` 在 bind mount | <s> | <s> | <s> | |
| `target/` 在命名卷 | <s> | <s> | <s> | |

**决策**：<保留 bind mount / 改用命名卷>，理由：<>
```

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
# bind mount 布局
docker run --rm -v "$PWD:/work" -w /work -u 501:20 rgoc:dev sh -c '
  cd /tmp && cargo new bench --bin -q && cd bench
  time cargo build -q; cargo clean; time cargo build -q
'
# 命名卷布局
docker volume create rgoc-bench-target >/dev/null
docker run --rm -v "$PWD:/work" -v rgoc-bench-target:/tmp/bench/target -w /work -u 501:20 rgoc:dev sh -c '
  cd /tmp && cargo new bench --bin -q && cd bench
  time cargo build -q; cargo clean; time cargo build -q
'
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

/// 把两个数相加。
///
/// 这是 Phase 1 的**调试目标**：T28 会在这个函数内下断点，
/// 验证 VSCode + CodeLLDB 的调试链路真的可用（E5）。
/// Phase 2 引入真实功能后可删除。
pub fn add(a: i64, b: i64) -> i64 {
    let sum = a + b;
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_works() {
        assert_eq!(add(1, 2), 3);
    }
}
```

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
  cd rgoc
  cargo fmt --check
  cargo check --workspace --all-targets
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
'
# 期望：四条全部成功（fmt 无 diff、check/clippy 无警告、test 通过）
```

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
        "filter": { "name": "rgoc-harness", "kind": "lib" }
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
        "args": ["test", "--no-run", "--workspace"]
      },
      "cwd": "${workspaceFolder}/rgoc"
    }
  ]
}
```

- **验证**：

```sh
cd /Users/wangfeng/workspace/rust_go_compiler
python3 - <<'PY'
import json, pathlib, re
s = pathlib.Path('.vscode/launch.json').read_text()
s = re.sub(r'^\s*//.*$', '', s, flags=re.M)
d = json.loads(s)
assert all(c['type'] == 'lldb' for c in d['configurations'])
print('launch.json OK:', [c['name'] for c in d['configurations']])
PY
```

---

### 任务 T27：在 VSCode 中 Reopen in Container 并跑通测试

- **文件路径**：无文件改动（**人工操作任务**）

- **要做的**（由用户在 VSCode 中执行）：

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

---

### 任务 T28：**E5 —— 实测断点命中**

- **文件路径**：无文件改动（**人工操作任务**）

- **要做的**：

1. 在容器内的 VSCode 中打开 `rgoc/crates/rgoc-harness/src/lib.rs`；
2. 在 `add()` 函数体第一行（`let sum = a + b;`）**点击行号左侧设置断点**（出现红点）；
3. 打开 `launch.json` 对应的 **「调试当前测试 (CodeLLDB)」** 配置，按 **F5**；
4. 确认：执行**停在断点处**（黄条高亮），左侧「变量」面板显示 `a = 1`、`b = 2`，「调用栈」面板显示 `rgoc_harness::add` ← `tests::add_works`；
5. 按 F10 单步一次，确认 `sum` 变为 `3`；按 F5 继续至结束。

- **验证**（**E5 的判定必须是实测，不是「能开窗口」**）：

| 检查项 | 期望 | 实测 |
|---|---|---|
| 断点命中（未被跳过） | 黄条停在 `let sum = a + b;` | ☐ |
| 「变量」面板非空 | 可见 `a`、`b` | ☐ |
| 「调用栈」面板非空 | 至少 2 帧（`add` ← `add_works`） | ☐ |
| 单步后可观察值变化 | `sum == 3` | ☐ |

**四项全部 ☑ 才算 E5 通过**，并把结果记录进 `M0-manifest.json` 的 `gate.E5`。

- **若断点被静默跳过**：优先检查 T24 的 `runArgs` 是否真的生效 —— 在容器内的 VSCode 终端执行：

```sh
cat /proc/self/status | grep CapEff        # 期望含 cap_sys_ptrace 位
```

---

## 4. 阶段门禁汇总

### Phase 0 出口

| 门禁 | 判定方式 | 状态 |
|---|---|---|
| **E1** | `docker build` 从零重放成功；`docker/image.lock` 无占位符 | ☐ |
| **E2** | T16 校验脚本输出 `none`（无空缺/占位符） | ☐ |
| **E10** | `M0-benchmarks.md` 四组数据齐备 | ☐ |

### Phase 1 出口

| 门禁 | 判定方式 | 状态 |
|---|---|---|
| T22 | `fmt` / `check` / `clippy -D warnings` / `test` 四条全过 | ☐ |
| **E5** | T28 的四项检查全部 ☑（**实测断点命中**） | ☐ |

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
| **不盲目演进临时代码** | T21 的 `add()` 是明确的**调试目标**，Phase 2 引入真实功能后删除 |
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
