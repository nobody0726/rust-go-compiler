#!/usr/bin/env bash
# =============================================================================
# 在 dev container 内【离线安装指定 commit 的 VS Code Server】
#
# ── 为什么需要这个脚本 ───────────────────────────────────────────────────────
# Dev Containers 在连接容器前先确认 server 已就位，按顺序检查两处：
#     ~/.vscode-server/bin/<commit>                          （实体或指向下面的符号链接）
#     /vscode/vscode-server/bin/linux-arm64/<commit>         （持久命名卷 vscode 里）
# 两处都没有时，它会在【宿主】上下载 server tarball（约 204 MB）—— 这一步走的是
# **宿主 VSCode 的网络栈**。若宿主 User settings 里的 `http.proxy` 指向一个
# **已经停掉的代理**（本机实测：`http://127.0.0.1:7897`，Clash 退出后端口无人监听），
# 下载必然失败，且 VSCode **不会回退直连**：
#     Failed to establish a socket connection to proxies: PROXY 127.0.0.1:7897
#     Retrying to download VS Code Server.
#     TypeError: Failed to fetch
# 于是窗口永远连不上容器。
#
# ⚠️ 每次升级宿主 VSCode，commit 都会变 → 这个坑会复现。
#    实测：1.139.1（04c0d99f…）→ 1.140.0（07f806f9…）时容器里没有新 commit 的 server。
#
# ── 本脚本的做法 ─────────────────────────────────────────────────────────────
# 容器**直连**外网可用且很快（实测该 tarball 204 MB / 20.4 s / 10.5 MB/s），
# 所以用 `curl --noproxy '*'` 把官方 tarball 拉进容器，解压到**持久卷**的对应位置，
# 再补上 `~/.vscode-server/bin/<commit>` 符号链接。此后 Dev Containers 的两个
# `test -d` 直接命中，**不再发起任何下载**。全程不改宿主配置。
#
# ── 用法 ─────────────────────────────────────────────────────────────────────
#   容器内：bash /work/scripts/install-vscode-server.sh --commit <sha>
#   宿主  ：docker exec <容器名> bash /work/scripts/install-vscode-server.sh --commit <sha>
#
#   commit 从哪来（宿主侧，权威且一行）：
#     python3 -c "import json;print(json.load(open('/Applications/Visual Studio Code.app/Contents/Resources/app/product.json'))['commit'])"
#
#   或者从 Dev Containers 的失败日志里抓（它自己会记这行）：
#     grep -rhoE 'Installing VS Code Server for commit [0-9a-f]{40}' \
#       "$HOME/Library/Application Support/Code/logs"/*/window*/exthost*/ 2>/dev/null | tail -1
#
# 幂等：目标目录已存在且 `bin/code-server` 可执行 → 立即返回 0，不发任何请求。
# =============================================================================
set -euo pipefail

COMMIT=""
while [ $# -gt 0 ]; do
    case "$1" in
        --commit) COMMIT="${2:-}"; shift 2 ;;
        --commit=*) COMMIT="${1#*=}"; shift ;;
        -h|--help)
            sed -n '2,40p' "$0" | sed 's/^# \?//'
            exit 0
            ;;
        *) echo "错误：未知参数 $1（用 --help 看用法）" >&2; exit 2 ;;
    esac
done
COMMIT="${COMMIT:-${VSCODE_COMMIT:-}}"

if [ -z "${COMMIT}" ]; then
    echo "错误：必须给出 --commit <sha>（或设 VSCODE_COMMIT 环境变量）。" >&2
    echo "      取法见本脚本头部注释；最省事的是在宿主读 VSCode 的 product.json。" >&2
    exit 2
fi
if ! printf '%s' "${COMMIT}" | grep -qE '^[0-9a-f]{40}$'; then
    echo "错误：--commit 必须是 40 位小写十六进制，收到 '${COMMIT}'。" >&2
    exit 2
fi

# ── 平台 ─────────────────────────────────────────────────────────────────────
case "$(uname -m)" in
    aarch64|arm64) VSCODE_ARCH="arm64" ;;
    x86_64|amd64)  VSCODE_ARCH="x64"   ;;
    *) echo "错误：不支持的架构 $(uname -m)" >&2; exit 1 ;;
esac
SERVER_DIR_NAME="vscode-server-linux-${VSCODE_ARCH}"
VOLUME_BIN="/vscode/vscode-server/bin/linux-${VSCODE_ARCH}"
DEST="${VOLUME_BIN}/${COMMIT}"
LINK="${HOME}/.vscode-server/bin/${COMMIT}"

echo "==> VS Code Server 离线安装器"
echo "   commit   : ${COMMIT}"
echo "   架构     : linux-${VSCODE_ARCH}"
echo "   安装位置 : ${DEST}"

# ── 幂等短路 ─────────────────────────────────────────────────────────────────
if [ -x "${DEST}/bin/code-server" ]; then
    echo "已安装，无需操作。"
    "${DEST}/bin/code-server" --version || true
else
    URL="https://update.code.visualstudio.com/commit:${COMMIT}/server-linux-${VSCODE_ARCH}/stable"
    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "${TMP_DIR}"' EXIT

    echo "   下载     : ${URL}"
    curl -fL --noproxy '*' --retry 3 --connect-timeout 15 \
        -o "${TMP_DIR}/server.tar.gz" "${URL}" \
        -w '   HTTP=%{http_code}  大小=%{size_download}B  用时=%{time_total}s  速度=%{speed_download}B/s\n'

    echo "   解压…"
    tar -xzf "${TMP_DIR}/server.tar.gz" -C "${TMP_DIR}"
    SRC="${TMP_DIR}/${SERVER_DIR_NAME}"
    if [ ! -d "${SRC}" ]; then
        echo "错误：tarball 里没有预期的顶层目录 ${SERVER_DIR_NAME}" >&2
        ls -1 "${TMP_DIR}" >&2
        exit 1
    fi

    # /vscode 卷属主是 root；dev 靠镜像里装的免密 sudo 写入
    sudo mkdir -p "${DEST}"
    sudo cp -a "${SRC}/." "${DEST}/"
    sudo chown -R root:root "${DEST}"
    rm -rf "${TMP_DIR}"
fi

# ── 符号链接 ─────────────────────────────────────────────────────────────────
mkdir -p "${HOME}/.vscode-server/bin"
ln -sfn "${DEST}" "${LINK}"

# ── 断言 ─────────────────────────────────────────────────────────────────────
# 关键：校验的是【语义】（product.json 里的 commit 是否等于请求的 commit），
# 而不是 tarball 的字节哈希 —— 上游可能重新打包，字节哈希会无辜失配；
# 而 commit 不匹配才是真正会让客户端拒绝复用的错误。
ACTUAL="$("${DEST}/node" -e "process.stdout.write(require('${DEST}/product.json').commit)" 2>/dev/null || true)"
if [ "${ACTUAL}" != "${COMMIT}" ]; then
    echo "错误：product.json 里的 commit 与请求不符（请求 ${COMMIT}，实际 ${ACTUAL:-空}）。" >&2
    exit 1
fi
[ -x "${DEST}/bin/code-server" ] || { echo "错误：${DEST}/bin/code-server 不可执行。" >&2; exit 1; }
[ -f "${DEST}/out/server-main.js" ] || { echo "错误：${DEST}/out/server-main.js 缺失。" >&2; exit 1; }

echo
echo "OK：$("${DEST}/bin/code-server" --version 2>&1)"
echo "    $("${DEST}/node" -e "const p=require('${DEST}/product.json');process.stdout.write(p.nameLong+' '+p.version)")"
echo "    体检：$(du -sh "${DEST}" | cut -f1)"
echo
echo "Dev Containers 的两处检查现在应当都通过："
test -d "${LINK}"  && echo "  ✓ ${LINK}"
test -d "${DEST}"  && echo "  ✓ ${DEST}"
echo
echo "下一步：在 VSCode 中重新执行 “Reopen in Container”（或重连窗口）。"
