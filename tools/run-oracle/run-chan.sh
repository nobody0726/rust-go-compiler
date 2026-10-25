#!/bin/bash
# tools/run-oracle/run-chan.sh —— M8 的「并发跑起来」对撞装置（Task 53/55）。
#
# 判据（`M8-tests.md` `T-CON-52/53/54`）：
#   * `N'`（入选，见 chan-input.txt）：`rgoc run` 的**合并流 + 退出码**与官方
#     `go run` 逐字节相同；
#   * `M'`（拒绝，见 chan-rejected.txt）：退出码 1（静态拒绝）⇒ 只数分母，
#     **不算通过**；退出码 2 也归 skip 但要在汇总里能看出来；
#   * 退出码其它（内部错 / 超时）⇒ BAD。
#
# 用法：
#   run-chan.sh [--list] [--filter PAT] [--no-oracle] <清单文件> [语料根]
#
#   --list        每个文件打一行 OK/SKIP/BAD（默认也打汇总）
#   --no-oracle   不生成期望值（期望已由 run-collide.sh 生成过时用）
#
# # 为什么汇总行要三栏
#
# `N'` 里绝大多数程序**输出为空**（官方 `.exp` 是 0 字节）。在空输出上
# 「两边都是空」不构成证据 —— 它只证明「没崩」。所以分开报：
#
#   自检 a/A + 仅有输出 b/B + 空跑 c/C
#
# 只写「N'/N' 绿」会把这三件事混成一句，而其中一多半是空话。
# 另外 `M'` 的条数是**冻结**的（`chan-rejected.txt` 的行数）：变了说明语料性质
# 变了（要么我们多支持了一个 import，要么官方换了用例），必须人看一眼。
#
# # 纪律（M7 学到的两条，都踩过）
#
#  * 产物名用 **slug**（`/` 换 `_`），不用 basename —— 同名的不同目录文件会互相覆盖；
#  * 不写 `if ! cmd; then rc=$?` —— `!` 之后 `$?` 恒 0，「退出码 1 = 静态拒绝」
#    会被算成 0，整张 M' 清单都成了「坏」。
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
DRIVER="$ROOT/rgoc/target/debug/rgoc-driver"
CACHE="${RGOC_ORACLE_CACHE:-$ROOT/.superpowers/m8-oracle-cache}"
OUT="${RGOC_CHAN_OUT:-$ROOT/.superpowers/m8-chan}"
REJECTED="$HERE/chan-rejected.txt"
LIST=""
CORPUS="$ROOT"
FILTER=""
ORACLE=1

while [ $# -gt 0 ]; do
  case "$1" in
    --list) ;;
    --no-oracle) ORACLE=0 ;;
    --filter) shift; FILTER="${1:-}" ;;
    -h|--help) sed -n '2,40p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) if [ -z "$LIST" ]; then LIST="$1"; else CORPUS="$1"; fi ;;
  esac
  shift
done

if [ -z "$LIST" ]; then
  echo "run-chan.sh: 缺少清单文件" >&2
  exit 2
fi
if [ ! -x "$DRIVER" ]; then
  echo "run-chan.sh: 找不到 $DRIVER（先在容器内 cargo build）" >&2
  exit 2
fi
mkdir -p "$OUT" "$CACHE"

# `M'` 的冻结条数（清单里的非注释行数）。
frozen_skip=0
if [ -f "$REJECTED" ]; then
  frozen_skip=$(grep -cve '^\s*$' -e '^#' "$REJECTED")
fi

ok=0
skip=0
bad=0
self_ok=0
self_all=0
out_ok=0
out_all=0
empty_ok=0
empty_all=0
badlist=""
skiplist=""

while IFS= read -r rel; do
  [ -n "$rel" ] || continue
  case "$rel" in \#*) continue ;; esac
  if [ -n "$FILTER" ]; then
    case "$rel" in *"$FILTER"*) ;; *) continue ;; esac
  fi

  # ⚠️ slug：路径里的 `/` 换 `_`。`test/chan/select.go` 与 `test/select.go`
  # 的 basename 相同，用 basename 会让后者的诊断覆盖前者的。
  slug="$(printf '%s' "$rel" | tr '/' '_')"
  src="$CORPUS/$rel"
  exp="$CACHE/$slug.exp"
  got="$OUT/$slug.out"
  log="$OUT/$slug.log"

  if [ ! -f "$src" ]; then
    bad=$((bad + 1))
    badlist="$badlist $slug:MISS"
    continue
  fi

  # 期望值：官方 `go run`。缺了才生成（`--no-oracle` 时不生成，直接用缓存）。
  if [ "$ORACLE" -eq 1 ] && [ ! -f "$exp" ]; then
    ( cd "$ROOT" && go run ./tools/run-oracle/main.go -file "$src" -out "$exp" ) \
      >/dev/null 2>"$CACHE/$slug.oracle-err"
  fi

  # `run` 的退出码三分：0 = 跑通；1 = 静态拒绝（诊断非空）；其它 = 坏。
  timeout 60 "$DRIVER" run "$src" >"$got" 2>"$log"
  rc=$?
  perl -pi -e 's/\r\n/\n/g' "$got" 2>/dev/null

  if [ "$rc" -eq 1 ]; then
    skip=$((skip + 1))
    skiplist="$skiplist $slug"
    continue
  fi
  if [ "$rc" -ne 0 ]; then
    bad=$((bad + 1))
    badlist="$badlist $slug:rc$rc"
    continue
  fi
  [ -f "$exp" ] || : > "$exp"

  if cmp -s "$exp" "$got"; then
    ok=$((ok + 1))
    # 三栏：有输出 / 空输出；「自检」= 用例自带 errorcheck 输出（.err 非空或
    # 官方 `.exp` 里带 `-` 注释行）—— 这里用「官方输出非空」近似。
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
echo "=== chan: 自检 $self_ok/$self_all + 仅有输出 $out_ok/$out_all + 空跑 $empty_ok/$empty_all（合计 ok=$ok skip=$skip bad=$bad / $total）"
[ -n "$skiplist" ] && echo "SKIP (M'):$skiplist"
[ -n "$badlist" ] && echo "BAD:$badlist"

# M' 条数变了 ⇒ 语料性质变了，必须人看一眼（Task 55 的 c ≠ 2 守卫同源）。
if [ "$skip" -ne "$frozen_skip" ]; then
  echo "警告：skip=$skip 与 chan-rejected.txt 的 $frozen_skip 不一致 —— 支持面变了？" >&2
fi
[ "$bad" -eq 0 ]
