#!/bin/bash
# tools/run-oracle/run-native.sh —— M7 的「native 跑起来」对撞装置。
#
# 判据（`M7-tests.md` `T-NAT-40..46`）：
#   * 能构建的文件：产物跑完**退出码 0**，且 stdout 与官方 `.exp` **逐字节**相同；
#   * 被 `rgoc-arm64` 静态拒绝的文件（退出码 1）：归 `M'`，只数分母，**不算通过**；
#   * 汇总行必须**分两栏**（有输出 / 空输出）—— 见下。
#
# 用法：
#   run-native.sh [--list] [--filter PAT] <清单文件> [语料根]
#
#   --list        每个文件打一行 OK/SKIP/BAD，并汇总（默认也打）
#   --filter PAT  只跑路径含 PAT 的行
#
# 退出码：0 = 全绿（每个文件要么 OK 要么 SKIP）；1 = 有 BAD。
#
# # 为什么必须分两栏
#
# `N` 侧 46 个文件里绝大多数**输出为空**（官方 `.exp` 是 0 字节）。在空输出上
# 「两边都是空」不构成证据 —— 它只证明「没崩」。所以汇总行分开报：
#
#   有输出 a/A + 空输出 b/B
#
# 只写「N'/N' 绿」会把这两件事混成一句，而其中一半是空话。
#
# # 纪律
#
#  * 语料目录**只读**；产物落 `$OUT`（默认 `.superpowers/m7-native`）。
#  * 期望值**不手写**：一律读 `$CACHE/<slug>.exp`（`run-collide.sh` / 官方 `go run` 生成）。
#  * 退出码 1（静态拒绝）与退出码 2（内部失败）**必须分开**：把前者算成坏，
#    等于把「M7 不支持切片」记成「编译器自己炸了」。
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
DRIVER="$ROOT/rgoc/target/debug/rgoc-driver"
CACHE="${RGOC_ORACLE_CACHE:-$ROOT/.superpowers/m5-oracle-cache}"
OUT="${RGOC_NATIVE_OUT:-$ROOT/.superpowers/m7-native}"
LIST=""
CORPUS="$ROOT"
FILTER=""

while [ $# -gt 0 ]; do
  case "$1" in
    --list) ;;
    --filter) shift; FILTER="${1:-}" ;;
    -h|--help) sed -n '2,32p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) if [ -z "$LIST" ]; then LIST="$1"; else CORPUS="$1"; fi ;;
  esac
  shift
done

# 缺省清单 = M6 冻结的 69 个入选语料（M9/Task 10）。
#
# 原来必须显式给 `native-input.txt`（46 个）。M9 的门禁是「native 覆盖 69」，
# 所以缺省就该是 69 的那份 —— 否则「跑一遍看看」看到的永远是 46 的分母。
if [ -z "$LIST" ]; then
  LIST="$ROOT/tools/run-oracle/run-selected.txt"
  echo "run-native.sh: 未给清单，缺省用 run-selected.txt（69 个入选语料）" >&2
fi
if [ ! -x "$DRIVER" ]; then
  echo "run-native.sh: 找不到 $DRIVER（先在容器内 cargo build）" >&2
  exit 2
fi
mkdir -p "$OUT"

ok=0
skip=0
bad=0
out_ok=0
out_all=0
empty_ok=0
empty_all=0
badlist=""
skiplist=""

# 逐文件：构建（退出码 1 = 静态拒绝）→ 跑 → 比 `.exp`
while IFS= read -r rel; do
  [ -n "$rel" ] || continue
  case "$rel" in \#*) continue ;; esac
  if [ -n "$FILTER" ]; then
    case "$rel" in *"$FILTER"*) ;; *) continue ;; esac
  fi

  # ⚠️ 产物名必须用 **slug**（路径里的 `/` 换 `_`），不能用 basename：
  # `test/convert.go` 与 `test/ken/convert.go` 的 basename 都是 `convert`，
  # 用 basename 会让后一个的诊断/产物覆盖前一个 —— 于是清单里的「拒绝原因」
  # 是**另一个文件**的。实测踩过：`convert` 报了 `CG-UNSUPPORTED-SLICE`，
  # 那是 `ken/convert.go` 的原因，`test/convert.go` 其实死在 `reflect` 导入上。
  slug="$(printf '%s' "$rel" | tr '/' '_')"
  exp="$CACHE/$slug.exp"
  exe="$OUT/$slug"
  log="$OUT/$slug.build"

  # ⚠️ 不要写 `if ! cmd; then rc=$?` —— `!` 之后 `$?` 是**取反后**的状态（恒 0），
  # 于是「退出码 1 = 静态拒绝」会被算成 build0，整张 `M'` 清单都成了「坏」。
  "$DRIVER" build "$ROOT/$rel" -o "$exe" >"$log" 2>&1
  rc=$?
  if [ "$rc" -ne 0 ]; then
    if [ "$rc" -eq 1 ]; then
      skip=$((skip + 1))
      skiplist="$skiplist $slug($(head -1 "$log" | tr -d '\n'))"
    else
      bad=$((bad + 1))
      badlist="$badlist $slug:build$rc"
    fi
    continue
  fi

  timeout 20 "$exe" >"$OUT/$slug.out" 2>"$OUT/$slug.err"
  rc=$?
  if [ "$rc" -ne 0 ]; then
    bad=$((bad + 1))
    badlist="$badlist $slug:rc$rc"
    continue
  fi

  if [ ! -f "$exp" ]; then
    bad=$((bad + 1))
    badlist="$badlist $slug:noexp"
    continue
  fi

  if cmp -s "$OUT/$slug.out" "$exp"; then
    ok=$((ok + 1))
    if [ -s "$exp" ]; then
      out_ok=$((out_ok + 1))
      out_all=$((out_all + 1))
    else
      empty_ok=$((empty_ok + 1))
      empty_all=$((empty_all + 1))
    fi
  else
    bad=$((bad + 1))
    badlist="$badlist $slug:DIFF"
  fi
done < "$LIST"

total=$((ok + skip + bad))
echo "=== native: 有输出 $out_ok/$out_all + 空输出 $empty_ok/$empty_all（合计 ok=$ok skip=$skip bad=$bad / $total）"
[ -n "$skiplist" ] && echo "SKIP (M'):$skiplist"
[ -n "$badlist" ] && echo "BAD:$badlist"
[ "$bad" -eq 0 ]
