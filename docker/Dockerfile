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

# ── 层 2：native 与调试能力 ───────────────────────────────────────────────────
# clang 是 native spike（P3）的唯一驱动；lldb 是容器内的调试备选。
# 层 3 的 rustup 需要 curl 与 ca-certificates；
# perl 提供 shasum —— 语料清单校验（T11）要用。
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
        perl \
        sudo \
    ; \
    rm -rf /var/lib/apt/lists/*; \
    # ── lldb CLI 的包装脚本（实测缺陷修复）──────────────────────────────────
    # 直接跑 /usr/bin/lldb 会抛：
    #   ModuleNotFoundError: No module named 'lldb.embedded_interpreter'
    # 原因不是缺包（python3-lldb-14 已装，模块确实在
    # /usr/lib/llvm-14/lib/python3.11/dist-packages/lldb/），而是 Debian 的
    # /usr/lib/python3/dist-packages/lldb 是个相对符号链接，python 把它解析成了
    # namespace package（`import lldb` 成功但 __file__ 为 None），内嵌解释器因此
    # 找不到子模块。显式给出 PYTHONPATH 后实测正常：`lldb version 14.0.6`。
    #
    # 用包装脚本而【不是】全局 PYTHONPATH：后者会作用于容器内所有 python 进程的
    # 模块搜索路径，有遮蔽标准库/三方包的风险。包装脚本只影响 lldb 这一个命令。
    # 说明：lldb 是 04 §4.2 列的「容器内调试备选」；主路径是 VSCode 的 CodeLLDB
    # （它自带 lldb 二进制，不走系统 lldb）。
    # python3.11 写死是可接受的：基础镜像已按 index digest 钉死，版本不会漂移。
    printf '%s\n' \
        '#!/bin/sh' \
        'PYTHONPATH=/usr/lib/llvm-14/lib/python3.11/dist-packages${PYTHONPATH:+:$PYTHONPATH} \' \
        '    exec /usr/lib/llvm-14/bin/lldb "$@"' \
        > /usr/local/bin/lldb; \
    chmod 0755 /usr/local/bin/lldb; \
    # 回归断言：必须断言【输出】，不能只靠退出码 —— 损坏状态下 lldb 会把
    # python traceback 打到 stderr 却仍以 0 退出，只查 $? 会漏判。
    lldb --version | tee /dev/stderr | grep -q '^lldb version' \
        || { echo "ERROR: lldb 包装脚本未生效（期望输出以 'lldb version' 开头）"; exit 1; }

# ── 层 3：非 root 日常开发用户 ────────────────────────────────────────────────
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
    id dev; \
    # sudo 的用途很具体（不是「为了方便」）：devcontainer 的 postCreateCommand
    # 需以 root 修正命名卷的属主 —— D-M0-9 把 rgoc/target/ 放到命名卷，而新建的
    # 空卷默认属于 root，dev 用户写不进去（实测 Permission denied）。
    # 没有 sudo，VSCode 路径上这一步无解。sudoers 必须在 useradd 之后写，
    # 否则 visudo 会因未知用户而校验失败。
    printf 'dev ALL=(ALL) NOPASSWD:ALL\n' > /etc/sudoers.d/90-dev; \
    chmod 0440 /etc/sudoers.d/90-dev; \
    visudo -c -f /etc/sudoers.d/90-dev; \
    sudo -u dev sudo -n true; \
    echo "sudo: dev 免密可用"

# ── 层 4：Rust 工具链（版本来源 = 仓库根的 rust-toolchain.toml）───────────────
# 顺序说明：rustup 必须装在 dev 用户下，因此本层排在「非 root 用户」之后。
#
# 版本号【不】在此重复写：从 rust-toolchain.toml 解析 channel（D-M0-8）。
# 【不使用】rustup 的 auto-installation —— 该行为已被 upstream 标记为
# deprecated（scripts relying on this behavior may stop working in the future），
# 因此这里显式 `rustup toolchain install`，并用 -n 断言解析成功。
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
    channel="$(sed -nE 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' /tmp/tb/rust-toolchain.toml)"; \
    [ -n "${channel}" ] || { echo "ERROR: 无法从 rust-toolchain.toml 解析 channel"; exit 1; }; \
    echo "installing rust toolchain: ${channel}"; \
    rustup toolchain install "${channel}" --profile minimal --no-self-update; \
    rustup default "${channel}"; \
    # components 在此显式列出（03 §6.3 的统一退出检查需要 rustfmt 与 clippy）。
    # 【同步约束】若改动 rust-toolchain.toml 的 components 字段，本行需一并更新。
    rustup component add rustfmt clippy; \
    rustc --version; cargo --version; \
    rm -rf /tmp/tb

# ── 层 5：让【登录 shell】也能找到工具链 ──────────────────────────────────────
# 为什么必须有这一层（实测缺陷，见 docker/image.lock 更新记录）：
#   上面的 ENV PATH 只对 `docker exec` / `sh -c` 这类**非登录**shell 生效。
#   登录 shell 会执行 /etc/profile，把 PATH 重置为
#     /usr/local/bin:/usr/bin:/bin:/usr/local/games:/usr/games
#   结果 `bash -lc 'cargo --version'` 报 command not found（go 同样）。
#   而 VSCode 集成终端默认以登录 shell 启动 —— 不修这条，用户在编辑器里
#   敲 cargo 就是 command not found，CodeLLDB 的 cargo 集成也会失败。
#
# 修法：只写 /etc/profile.d/50-rgoc-toolchains.sh 一处。
#   实测确认 Debian bookworm 的 /etc/profile 是【先重置 PATH、后 source
#   /etc/profile.d/*.sh】，因此这一处足以生效；不要再往 ~/.profile 重复写，
#   否则 PATH 前缀会被叠加两次（曾经这样写过，实测 PATH 出现双份 cargo bin）。
#   基础镜像已按 index digest 钉死，这个顺序不会再变。
USER root
RUN set -eux; \
    printf '%s\n' 'export PATH=/home/dev/.cargo/bin:/go/bin:/usr/local/go/bin:$PATH' \
        > /etc/profile.d/50-rgoc-toolchains.sh; \
    chmod 0644 /etc/profile.d/50-rgoc-toolchains.sh; \
    echo "--- /etc/profile.d/50-rgoc-toolchains.sh ---"; \
    cat /etc/profile.d/50-rgoc-toolchains.sh
USER dev

# 断言：登录 shell 也能解析到 cargo / go。
# 这条断言就是上面那个缺陷的【回归测试】—— 删掉层 5 时它会立刻失败，
# 而不是等到用户在 VSCode 里敲 cargo 时才发现。
RUN set -eux; \
    bash -lc 'command -v cargo && command -v go'; \
    bash -lc 'cargo --version && rustc --version && go version'
