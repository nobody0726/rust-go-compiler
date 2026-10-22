#!/bin/sh
# 逐个跑 `rgoc check`，输出 `<OK|REJ>\t<文件>\t<首条诊断>`。
#
# 用法（容器内，仓库根）：
#   sh tools/run-oracle/check-all.sh                      # 跑候选清单
#   sh tools/run-oracle/check-all.sh --list FILE          # 跑指定清单
#
# 用途：Task 1 的现状基线；Task 6 的 79/80 接受率门禁（`T-EX-61`）。
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../.." && pwd)
cd "$root"

list="$here/run-candidates.txt"
if [ "${1:-}" = "--list" ]; then
    list="$2"
fi

bin="${RGOC_BIN:-rgoc/target/debug/rgoc-driver}"
[ -x "$bin" ] || { echo "缺少 $bin（先 cargo build -p rgoc-driver）" >&2; exit 2; }

grep -v '^#' "$list" | grep -v '^[[:space:]]*$' | while IFS= read -r f; do
    if out=$("$bin" check "$f" 2>&1); then
        printf 'OK\t%s\t\n' "$f"
    else
        first=$(printf '%s\n' "$out" | grep -m1 -E '^[^ ]+:[0-9]+:[0-9]+:' || true)
        printf 'REJ\t%s\t%s\n' "$f" "$first"
    fi
done
