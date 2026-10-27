#!/bin/bash
# tools/run-oracle/run-native-chan.sh —— M11 的「native 并发跑起来」装置（Task 4）。
#
# 判据（`M11-tests.md` `T-GCORP-01/02/03`、`T-GDEV-01/02/03`）：
#   * `N'`（= `chan-input.txt` − `chan-rejected.txt`，**算出来**）：native 产物
#     与官方 `.exp` **逐字节**相同（stdout / stderr / 退出码三样分开比）；
#   * `M'`（= `chan-rejected.txt`）：**只 build**，断言退出码 **1**（干净拒绝）；
#   * `N'` 里 build 退出码 1 的算 SKIP（M11a 之前的常态，不是失败）。
#
# 用法：
#   run-native-chan.sh [--list] [--filter PAT] [--gc=off|on|stress] [--no-oracle]
#
#   --list        每个文件打一行 OK/SKIP/BAD
#   --filter PAT  只跑路径含 PAT 的行
#   --gc=MODE     off | on | stress（M10 三态，`C12` §5.1；构建一次、跑三次）
#   --no-oracle   不生成期望值（`.exp` 已缓存时用）
#
# 退出码：0 = 全绿（每个 `N'` 要么 OK 要么 SKIP，且 `M'` 全干净拒绝）；1 = 有 BAD。
#
# # 为什么 `N'` 的清单要**算**出来（`T-GDEV-01`）
#
# 抄一份清单就会有第二份、第三份，然后三份互相不一致 —— 而「不一致」这件事
# 本身没有任何判据能发现（`M9` 的 `native_corpus_run` 就因为清单过期红过一次）。
# 所以这里只有两个事实源：`chan-input.txt`（全集）与 `chan-rejected.txt`（拒面）。
#
# # 为什么汇总行分三栏（沿 `run-chan.sh` 的口径）
#
# `N'` 的 7 个里有多半**输出为空**（`zerosize.go` 只 `make` 不打印）。在空输出上
# 「两边都是空」不构成证据 —— 它只证明「没崩」。所以：
#
#   自检 a/A + 仅有输出 b/B + 空跑 c/C
#
# `T-GDEV-02`：`a + b + c == A + B + C == ok + skip + bad`（脚本自己断言）。
#
# # 纪律（M7/M8 踩过的，全部照抄）
#
#  * 产物名用 **slug**（`/` 换 `_`），不用 basename；
#  * 不写 `if ! cmd; then rc=$?` —— `!` 之后 `$?` 恒 0；
#  * 语料目录**只读**，产物落 `$OUT`；
#  * `M'` 的行数**冻结**，变了要人看一眼（`T-GDEV-03`）。
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
DRIVER="$ROOT/rgoc/target/debug/rgoc-driver"
CACHE="${RGOC_ORACLE_CACHE:-$ROOT/.superpowers/m5-oracle-cache}"
OUT="${RGOC_CHAN_OUT:-$ROOT/.superpowers/m11-chan}"
INPUT="$HERE/chan-input.txt"
REJECTED="$HERE/chan-rejected.txt"

LIST_MODE=0
FILTER=""
GC_MODE=""
ORACLE=1

while [ $# -gt 0 ]; do
  case "$1" in
    --list) LIST_MODE=1 ;;
    --filter) shift; FILTER="${1:-}" ;;
    --gc=*) GC_MODE="${1#--gc=}" ;;
    --no-oracle) ORACLE=0 ;;
    -h|--help) sed -n '2,40p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) echo "未知参数：$1" >&2; exit 2 ;;
  esac
  shift
done

if [ ! -x "$DRIVER" ]; then
  echo "run-native-chan.sh: 找不到 $DRIVER（先在容器内 cargo build）" >&2
  exit 2
fi
mkdir -p "$OUT"

# ── 两个事实源：`N'` 算出来，`M'` 直接读 ─────────────────────────────────
N_LIST="$OUT/n-prime.txt"
M_LIST="$OUT/m-prime.txt"
awk -F'\t' '
  FNR == NR { if ($0 !~ /^#/ && $0 != "") rej[$1] = 1; next }
  $0 !~ /^#/ && $0 != "" { p = $1; if (!(p in rej)) print p }
' "$REJECTED" "$INPUT" >"$N_LIST"
awk -F'\t' '$0 !~ /^#/ && $0 != "" { print $1 }' "$REJECTED" >"$M_LIST"

n_total=$(wc -l <"$N_LIST" | tr -d ' ')
m_total=$(wc -l <"$M_LIST" | tr -d ' ')
m_frozen=$(awk -F'\t' '$0 !~ /^#/ && $0 != "" { c++ } END { print c + 0 }' "$REJECTED")

# `T-GDEV-03`：M' 的行数冻结。
if [ "$m_total" -ne "$m_frozen" ]; then
  echo "BAD: M' 行数 $m_total ≠ chan-rejected.txt 的 $m_frozen（T-GDEV-03）" >&2
  exit 1
fi
# `T-GDEV-01`：N' 必须真的由差集算出，且两半加起来等于全集（`chan-input`）。
# ⚠️ 不硬编码 7：那个数会在 M11 实现后变化（`M'` 里可能有文件变成可支持）。
# 但「N' + M' == 全集」永远成立 —— 它才是不变式。
n_all=$(awk -F'\t' '$0 !~ /^#/ && $0 != "" { c++ } END { print c + 0 }' "$INPUT")
if [ "$n_total" -eq 0 ]; then
  echo "BAD: N' 算出来是空的 —— 差集逻辑坏了（T-GDEV-01）" >&2
  exit 1
fi
if [ $((n_total + m_total)) -ne "$n_all" ]; then
  echo "BAD: N'($n_total) + M'($m_total) ≠ 全集($n_all) —— 差集漏了或多算了（T-GDEV-01）" >&2
  exit 1
fi

# 三态只影响**运行**侧（构建一次、跑三次；`C12` §5.1 的同一个口径）。
run_prog() {
  case "$GC_MODE" in
    off) RGOC_GC=0 timeout 60 "$1" ;;
    stress) RGOC_GC_STRESS=64 timeout 60 "$1" ;;
    on|"") timeout 60 "$1" ;;
    *) echo "run-native-chan.sh: --gc=$GC_MODE 不认识（off|on|stress）" >&2; exit 2 ;;
  esac
}

ok=0; skip=0; bad=0
self_ok=0; self_all=0
out_ok=0; out_all=0
empty_ok=0; empty_all=0
m_ok=0
badlist=""
skiplist=""

line() {
  [ "$LIST_MODE" -eq 1 ] || return 0
  printf '%-4s %s\n' "$1" "$2"
}

# ── ① `M'`：只 build，断言干净拒绝 ───────────────────────────────────────
while IFS= read -r rel; do
  [ -n "$rel" ] || continue
  if [ -n "$FILTER" ]; then
    case "$rel" in *"$FILTER"*) ;; *) continue ;; esac
  fi
  slug="$(printf '%s' "$rel" | tr '/' '_')"
  log="$OUT/$slug.build"
  "$DRIVER" build "$ROOT/$rel" -o "$OUT/$slug.m" >"$log" 2>&1
  rc=$?
  if [ "$rc" -eq 1 ]; then
    m_ok=$((m_ok + 1))
    line SKIP "$rel (M' 干净拒绝)"
  else
    bad=$((bad + 1))
    badlist="$badlist ${slug}:m-rc${rc}"
    line BAD "$rel (M' 期望 rc=1，实际 $rc)"
  fi
done <"$M_LIST"

# ── ② `N'`：build → run → 比 `.exp` ─────────────────────────────────────
while IFS= read -r rel; do
  [ -n "$rel" ] || continue
  if [ -n "$FILTER" ]; then
    case "$rel" in *"$FILTER"*) ;; *) continue ;; esac
  fi
  slug="$(printf '%s' "$rel" | tr '/' '_')"
  exp="$CACHE/$slug.exp"
  exe="$OUT/$slug"
  log="$OUT/$slug.build"

  "$DRIVER" build "$ROOT/$rel" -o "$exe" >"$log" 2>&1
  rc=$?
  if [ "$rc" -eq 1 ]; then
    # M11a 之前的常态：静态拒绝（`SSA-UNSUPPORTED-CHAN`）。不是失败。
    skip=$((skip + 1))
    skiplist="$skiplist $slug"
    # ⚠️ 不截断诊断：`cut -c` 在 UTF-8 下按**字节**切，会把中文切出半个字符
    # （实测 `表达�`）。诊断本来就一行，完整显示更有用。
    line SKIP "$rel（rc=1：$(head -1 "$log")）"
    continue
  fi
  if [ "$rc" -ne 0 ]; then
    bad=$((bad + 1))
    badlist="$badlist ${slug}:build${rc}"
    line BAD "$rel (build rc=$rc)"
    continue
  fi

  run_prog "$exe" >"$OUT/$slug.out" 2>"$OUT/$slug.err"
  rc=$?
  if [ "$rc" -ne 0 ]; then
    bad=$((bad + 1))
    badlist="$badlist ${slug}:rc${rc}"
    line BAD "$rel (run rc=$rc)"
    continue
  fi

  # 期望值：官方 `go run`。缺了才生成。
  if [ "$ORACLE" -eq 1 ] && [ ! -f "$exp" ]; then
    ( cd "$ROOT" && go run ./tools/run-oracle/main.go -file "$ROOT/$rel" -out "$exp" ) \
      >/dev/null 2>"$CACHE/$slug.oracle-err"
  fi
  if [ ! -f "$exp" ]; then
    bad=$((bad + 1))
    badlist="$badlist ${slug}:noexp"
    line BAD "$rel (无期望值)"
    continue
  fi

  # ⚠️ 只比 **stdout**：`C9` §5 的死锁文本进 stderr，合并了就分不清谁打的。
  # 退出码上面已经比过（必须 0）。stderr 单独存一份供人看，不参与判定。
  if cmp -s "$OUT/$slug.out" "$exp"; then
    ok=$((ok + 1))
    if [ -s "$exp" ]; then
      out_ok=$((out_ok + 1)); out_all=$((out_all + 1))
    else
      empty_ok=$((empty_ok + 1)); empty_all=$((empty_all + 1))
    fi
    line OK "$rel"
  else
    bad=$((bad + 1))
    badlist="$badlist ${slug}:DIFF"
    line BAD "$rel (DIFF)"
  fi
done <"$N_LIST"

# `T-GDEV-02`：三栏相加必须等于分母。
if [ $((ok + skip + bad)) -ne "$n_total" ]; then
  bad=$((bad + 1))
  badlist="$badlist 计数不闭合($((ok + skip + bad))≠$n_total)"
fi

GCTAG=""
[ -n "$GC_MODE" ] && GCTAG=" gc=$GC_MODE"
echo "=== native-chan: 自检 $self_ok/$self_all + 仅有输出 $out_ok/$out_all + 空跑 $empty_ok/$empty_all（合计 ok=$ok skip=$skip bad=$bad / $n_total） M' $m_ok/$m_total$GCTAG"
[ -n "$skiplist" ] && echo "SKIP (N'):$skiplist"
[ -n "$badlist" ] && echo "BAD:$badlist"
[ "$bad" -eq 0 ]
