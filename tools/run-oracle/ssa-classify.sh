#!/bin/bash
# tools/run-oracle/ssa-classify.sh —— M6 的语料分类器（`T-SSA-31` 的数据源）。
#
# 用法：
#   ssa-classify.sh [清单文件]        # 缺省 = tools/run-oracle/run-selected.txt
#
# 输出（stdout，三栏，TAB 分隔）：
#   <相对路径>\t<N|M>\t<码>
#
#   N = 能建图且 verifier 通过（`ssa --verify` 退出码 0）
#   M = 被拒绝；第三栏是 stderr 里的第一个 `SSA-*` 码
#
# 纪律：
#  * **只读**语料（不改 `go_source_code/`）；
#  * 拒绝一定要**带码**：拒了但说不出为什么，与「跳过」没有区别（`M6-tests.md` §4.6）；
#  * `N + M == 清单行数` 由 `ssa_slice.rs`（Task 39）两向断言，本脚本只负责产出。
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
DRIVER="${RGOC_DRIVER:-$ROOT/rgoc/target/debug/rgoc-driver}"
LIST="${1:-$ROOT/tools/run-oracle/run-selected.txt}"

if [ ! -x "$DRIVER" ]; then
  echo "ssa-classify.sh: 找不到 $DRIVER（先在容器内 cargo build）" >&2
  exit 2
fi
if [ ! -f "$LIST" ]; then
  echo "ssa-classify.sh: 找不到清单 $LIST" >&2
  exit 2
fi

while IFS= read -r rel; do
  case "$rel" in ''|'#'*) continue ;; esac
  rel="${rel%%[[:space:]]*}"
  src="$ROOT/$rel"
  [ -f "$src" ] || src="$rel"

  # stderr 与 stdout 分开收：`ssa --verify` 的 dump 走 stdout、错误走 stderr。
  err="$("$DRIVER" ssa --verify "$src" 2>&1 >/dev/null)"
  code=$?
  if [ "$code" -eq 0 ]; then
    printf '%s\tN\t-\n' "$rel"
  else
    # 取第一个 `SSA-*` 码；没有码就如实写退出码（不留空栏 —— 空栏无法与「漏填」区分）。
    first="$(printf '%s' "$err" | grep -oE 'SSA-[A-Z][A-Z-]*' | head -1)"
    printf '%s\tM\t%s\n' "$rel" "${first:-ssa-exit-$code}"
  fi
done < "$LIST"
