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
#
# 环境变量：
#   RGOC_IMAGE   覆盖镜像名（默认 rgoc:dev）
#   RGOC_TTY     设为 0 时禁用 -t（非交互场景，如 CI）
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

# ── 3. 组装 docker 参数 ──────────────────────────────────────────────────────
# 注意：数组必须始终保持【非空】—— 在 `set -u` 下展开一个空数组会报
# "TTY_ARGS[@]: unbound variable"。这里以 --rm 作为恒存元素。
DOCKER_FLAGS=("--rm")
if [ "${RGOC_TTY:-1}" = "1" ] && [ -t 0 ]; then
    DOCKER_FLAGS+=("-it")
fi

# ── 4. 三个命名卷的 bootstrap（D-M0-9）────────────────────────────────────────
# rgoc/target/ 放在【命名卷】而非 bind mount。依据是 T18 的实测：
#   120 模块 crate 冷构建，bind mount 0.287–0.359s vs 命名卷 0.133–0.150s（≈2.3×）。
#
# 代价与对策（都实测过）：
#   a) 新建的空卷属主是 root:root，dev 用户写入报 Permission denied
#      → 对策：下面这次 chown（只改卷根，cargo 自己在下面建目录）
#   b) `cargo clean` 会因卷挂载点无法 rmdir 而报 EBUSY(16)、exit 101
#      → 对策：清空用 `find target -mindepth 1 -delete`
#
# ⚠️ 2026-10-07 修复：原先只 bootstrap 了 TARGET_VOLUME 一个卷，漏了
#   CARGO_REGISTRY_VOLUME 与 CARGO_GIT_VOLUME —— 症状是第一条 cargo 命令就
#   `Permission denied (os error 13)`，而报错指向「源码读取」而非「卷属主」，
#   很容易误判为文件权限问题。三个卷都需要同一处理。
#
# ⚠️ 关于「chown 明明 exit 0，下一个容器又变回 0:0」：
#   实测 Docker Desktop 4.94 / engine 29.8.2 下，**卷根属主不会跨容器保留**
#   （同一容器内 chown 后 su dev 写入 OK，退出后下一个容器 stat 又是 0:0）。
#   但这不影响正确性 —— 因为脚本每次调用都会重新 stat 并按需 chown，
#   而真正使用这些卷的就是紧随其后的第 5 步那个容器。
#   ⇒ 判据必须落在「实际写入」，不是「stat 保持 501」。
VOLUMES=("rgoc-target" "rgoc-cargo-registry" "rgoc-cargo-git")
for VOLUME in "${VOLUMES[@]}"; do
    docker volume create "${VOLUME}" >/dev/null
    # 只在属主不是 dev 时才 chown，避免每次调用都起一个 root 容器
    # 注意 docker 的参数顺序：所有选项必须在【镜像名之前】，镜像名之后是命令与参数。
    if [ "$(docker run --rm --user root --entrypoint stat \
                -v "${VOLUME}:/mnt" "${IMAGE}" -c %u /mnt 2>/dev/null)" != "501" ]; then
        docker run --rm --user root --entrypoint chown \
            -v "${VOLUME}:/mnt" "${IMAGE}" 501:20 /mnt
    fi
done

# ── 5. 执行 ──────────────────────────────────────────────────────────────────
# $CARGO_HOME/registry 与 /git 也挂命名卷：macOS 上 bind mount 随机写慢。
# 卷名与上面bootstrap 的列表一致—— 少挂一个就会出现「Permission denied」。
exec docker run "${DOCKER_FLAGS[@]}" \
    -v "${REPO_ROOT}:/work" \
    -w /work \
    -v rgoc-cargo-registry:/home/dev/.cargo/registry \
    -v rgoc-cargo-git:/home/dev/.cargo/git \
    -v rgoc-target:/work/rgoc/target \
    "${IMAGE}" "$@"
