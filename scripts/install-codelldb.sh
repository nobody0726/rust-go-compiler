#!/usr/bin/env bash
# =============================================================================
# 在 dev container 内安装 CodeLLDB 的【平台包】（含真正的 lldb 二进制）
#
# ── 为什么需要这个脚本 ───────────────────────────────────────────────────────
# CodeLLDB 拆成两部分：
#   ① 扩展本体（来自 marketplace）—— Dev Containers 会自动装好；
#   ② 平台包 codelldb-<platform>.vsix（来自 GitHub releases）—— 由扩展在首次
#      激活时自己下载，判据是 <扩展目录>/platform.ok 是否存在。
#
# ② 走的是 VSCode 扩展宿主的网络栈，会被【宿主下发进容器的 http.proxy】劫持：
#   宿主 VSCode 通过 AHP `root/configChanged` 主动把
#       http.proxy = http://127.0.0.1:7897
#   推给容器，而容器里的 127.0.0.1 是**容器自己的 loopback**（那里没有代理在听），
#   于是下载必然失败，报：
#       Error: Failed to establish a socket connection to proxies:
#              PROXY 127.0.0.1:7897
#
# ⚠️ 在 devcontainer 的 remote Machine settings 里写 "http.proxy": "" **不足以**
#    覆盖它 —— 实测：写完（文件 mtime 03:31:58）之后新开的会话（03:40:12）仍然
#    报同一错误。因为该值由**客户端**下发，优先级高于远端设置。
#    完整因果链与证据见 docs/milestones/M0-benchmarks.md §7。
#
# ── 本脚本的做法 ─────────────────────────────────────────────────────────────
# 完全绕开 VSCode 的网络栈：
#   1. 用 curl --noproxy '*' 直连 GitHub 下载（容器的直连网络实测 54 MB / 13 s /
#      4.1 MB/s，完全够用）；
#   2. 再用 VSCode 自带的 code-server CLI 从**本地 vsix** 安装。
# 全程不需要改动宿主的任何配置，容器重建后重跑一次即可。
#
# ── 用法 ─────────────────────────────────────────────────────────────────────
#   容器内：bash /work/scripts/install-codelldb.sh
#   宿主  ：docker exec <容器名> bash /work/scripts/install-codelldb.sh
#
# 幂等：已安装且可用时立即返回 0，不产生任何网络请求。
# =============================================================================
set -euo pipefail

VERSION="${CODELLDB_VERSION:-1.12.3}"
# M0 的首发平台固定为 Linux / arm64（见 docs/04-development-environment.md）
PLATFORM="linux-arm64"
PKG="codelldb-${PLATFORM}.vsix"
URL="https://github.com/vadimcn/codelldb/releases/download/v${VERSION}/${PKG}"

# v1.12.3 / linux-arm64 的实测指纹 —— 下载后校验，避免把半截文件装进去。
# 换版本时这两个值必须一起更新（脚本会因此失败，是故意的）。
EXPECTED_SIZE=54544514
EXPECTED_SHA256=0887f67d440554617894266f80706b700907c36b95e6e49d23b95a0e05318101

EXT_ROOT="${HOME}/.vscode-server/extensions"

echo "==> CodeLLDB 平台包安装器 (${PKG}, v${VERSION})"

# ---------------------------------------------------------------------------
# 定位已安装的扩展本体目录
# ---------------------------------------------------------------------------
find_ext_dir() {
    local d
    for d in "${EXT_ROOT}"/vadimcn.vscode-lldb-*; do
        if [ -d "$d" ]; then
            printf '%s\n' "$d"
            return 0
        fi
    done
    return 1
}

# ---------------------------------------------------------------------------
# 定位 VSCode server 的 code-server CLI
#   注意 1：不能用 .../bin/remote-cli/code —— 它要求 VSCODE_IPC_HOOK_CLI，
#   只能在 VSCode 集成终端里用，从 docker exec 调用会直接报
#   "Command is only available in WSL or inside a Visual Studio Code terminal"。
#   code-server 是独立入口，不需要活动窗口。
#   注意 2：`~/.vscode-server/bin/` 下会有【多个】版本目录 —— 每升级一次宿主
#   VSCode 就多一个（配合 scripts/install-vscode-server.sh 离线塞进来的也是）。
#   取最近修改的那个，它对应客户端当前在用的版本。扩展目录与 extensions.json
#   是所有 server 版本共用的，用哪个都装得上；取最新只为不依赖陈旧版本的行为。
# ---------------------------------------------------------------------------
find_code_server() {
    local c
    for c in $(ls -1dt "${HOME}"/.vscode-server/bin/*/bin/code-server 2>/dev/null); do
        if [ -x "$c" ]; then
            printf '%s\n' "$c"
            return 0
        fi
    done
    return 1
}

# ---------------------------------------------------------------------------
# 幂等检查
# ---------------------------------------------------------------------------
if EXT_DIR="$(find_ext_dir)"; then
    if [ -f "${EXT_DIR}/platform.ok" ] && [ -x "${EXT_DIR}/lldb/bin/lldb" ]; then
        echo "已安装（platform.ok 存在且 lldb 可执行），无需操作。"
        "${EXT_DIR}/lldb/bin/lldb" --version | head -1
        exit 0
    fi
    echo "扩展本体：${EXT_DIR}"
    echo "平台包缺失，继续安装。"
else
    echo "尚未安装 CodeLLDB 扩展本体（${EXT_ROOT} 下没有 vadimcn.vscode-lldb-*）。"
    echo "请先在 VSCode 中执行 Reopen in Container，等 Dev Containers 装好扩展后再重跑本脚本。"
    exit 0
fi

if ! CODE_SERVER="$(find_code_server)"; then
    echo "错误：找不到 code-server CLI（${HOME}/.vscode-server/bin/*/bin/code-server）。" >&2
    echo "请先在 VSCode 中连上本容器（Reopen in Container）再重跑。" >&2
    exit 1
fi
echo "code-server：${CODE_SERVER}"

# ---------------------------------------------------------------------------
# 下载（--noproxy '*' 强制直连，彻底避开宿主下发的死代理）
# ---------------------------------------------------------------------------
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT
VSIX="${TMP_DIR}/${PKG}"

echo "下载 ${URL}"
curl -fL --noproxy '*' --retry 3 --connect-timeout 15 -o "${VSIX}" "${URL}" \
    -w '  HTTP=%{http_code}  大小=%{size_download}B  用时=%{time_total}s  速度=%{speed_download}B/s\n'

# ---------------------------------------------------------------------------
# 指纹校验
# ---------------------------------------------------------------------------
ACTUAL_SIZE="$(stat -c %s "${VSIX}")"
ACTUAL_SHA256="$(sha256sum "${VSIX}" | cut -d' ' -f1)"
if [ "${ACTUAL_SIZE}" != "${EXPECTED_SIZE}" ] || [ "${ACTUAL_SHA256}" != "${EXPECTED_SHA256}" ]; then
    echo "错误：下载产物与预期指纹不符。" >&2
    echo "  预期  ${EXPECTED_SIZE} B  ${EXPECTED_SHA256}" >&2
    echo "  实际  ${ACTUAL_SIZE} B  ${ACTUAL_SHA256}" >&2
    exit 1
fi
echo "指纹校验通过（${ACTUAL_SIZE} B）"

# ---------------------------------------------------------------------------
# 安装
# ---------------------------------------------------------------------------
"${CODE_SERVER}" --install-extension "${VSIX}" --force

# ---------------------------------------------------------------------------
# 断言：必须真的能跑
# ---------------------------------------------------------------------------
EXT_DIR="$(find_ext_dir)" || { echo "错误：安装后仍找不到扩展目录。" >&2; exit 1; }
[ -f "${EXT_DIR}/platform.ok" ]    || { echo "错误：${EXT_DIR}/platform.ok 缺失。" >&2; exit 1; }
[ -x "${EXT_DIR}/lldb/bin/lldb" ]  || { echo "错误：${EXT_DIR}/lldb/bin/lldb 缺失或不可执行。" >&2; exit 1; }

# 注意断言的是【输出内容】而不是退出码：损坏的 lldb 会打印 traceback 但仍以 0 退出。
LLDB_VERSION="$("${EXT_DIR}/lldb/bin/lldb" --version | head -1)"
case "${LLDB_VERSION}" in
    *codelldb*)
        echo "OK：${LLDB_VERSION}"
        ;;
    *)
        echo "错误：lldb 版本串不含 'codelldb'，平台包可能没装上：${LLDB_VERSION}" >&2
        exit 1
        ;;
esac

echo
echo "下一步：在 VSCode 中执行 “Developer: Reload Window”，然后即可在"
echo "        rgoc/crates/rgoc-harness/src/lib.rs 下断点并按 F5 调试。"
