#!/bin/bash
# tools/run-oracle/run-collide.sh —— M5 的「跑起来」对撞装置。
#
# 判据（`M5-tests.md` §4.1）：合并流逐字节相同 + 退出码相同。
# 期望值由 `tools/run-oracle/main.go`（官方 `go run`）生成，不手写。
#
# 用法：
#   run-collide.sh --run [--list] [--three-way] [--filter PAT] <清单文件> [语料根]
#
#   --run         逐文件比对（默认动作）
#   --list        每个文件打一行 OK/FAIL/DIFF，并汇总
#   --three-way   额外比对 <文件>.out（存在时）；三方一致才算绿（Task 48）
#   --ssa         加一列 `run --via=ssa`：hir / ssa / go run 三方（M6 Task 5 骨架，Task 38 接实）
#   --filter PAT  只跑路径含 PAT 的行（开发期用）
#   --jobs N      并发数（默认 4）
#
# 退出码：0 = 全绿；1 = 有红。
#
# 纪律：
#  * 语料目录**只读** —— 期望值一律落在 $CACHE，不回写 `go_source_code/`。
#  * 不做多重集 / 剥列 / 正则 / 包含匹配（`M5-tests.md` §4.1）。
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
DRIVER="$ROOT/rgoc/target/debug/rgoc-driver"
CACHE="${RGOC_ORACLE_CACHE:-$ROOT/.superpowers/m5-oracle-cache}"
LIST=""
CORPUS="$ROOT"
MODE_LIST=0
THREE=0
SSA=0
FILTER=""
JOBS=4

while [ $# -gt 0 ]; do
  case "$1" in
    --run) ;;
    --list) MODE_LIST=1 ;;
    --three-way) THREE=1 ;;
    --ssa) SSA=1 ;;
    --filter) shift; FILTER="${1:-}" ;;
    --jobs) shift; JOBS="${1:-4}" ;;
    -h|--help) sed -n '2,25p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) if [ -z "$LIST" ]; then LIST="$1"; else CORPUS="$1"; fi ;;
  esac
  shift
done

if [ -z "$LIST" ]; then
  echo "run-collide.sh: 缺少清单文件" >&2
  exit 2
fi
if [ ! -x "$DRIVER" ]; then
  echo "run-collide.sh: 找不到 $DRIVER（先在容器内 cargo build）" >&2
  exit 2
fi
mkdir -p "$CACHE"

# `--ssa` 需要 `run --via=ssa`（M6/Task 37）。没有就**明确拒绝**：
# 静默退回两方比对 = 报了一个没做的检查（M5 的 `T-EX-43/70` 就是这么被骗过两次的）。
if [ "$SSA" = 1 ]; then
  probe="$ROOT/go_source_code/test/helloworld.go"
  msg="$("$DRIVER" run --via=ssa "$probe" 2>&1 >/dev/null || true)"
  if printf '%s' "$msg" | grep -q '未知选项'; then
    echo "run-collide.sh: --ssa 需要 \`run --via=ssa\`（M6/Task 37），当前驱动不支持；拒绝静默退回两方比对。" >&2
    exit 2
  fi
fi

# 逐文件跑一遍，结果写到 $CACHE/<slug>.{exp,got,meta}
one() {
  local rel="$1"
  local slug; slug="$(printf '%s' "$rel" | tr '/' '_')"
  local src="$CORPUS/$rel"
  [ -f "$src" ] || src="$ROOT/$rel"
  if [ ! -f "$src" ]; then
    printf 'MISS\t%s\t源文件不存在\n' "$rel" > "$CACHE/$slug.meta"
    return
  fi
  # 期望值：官方 go run
  ( cd "$ROOT" && go run ./tools/run-oracle/main.go -file "$src" -out "$CACHE/$slug.exp" ) \
    >/dev/null 2>"$CACHE/$slug.oracle-err"
  local ecode=$?
  # 实测值：rgoc
  ( cd "$ROOT" && "$DRIVER" run "$src" ) > "$CACHE/$slug.got" 2>"$CACHE/$slug.err"
  local gcode=$?
  # 唯一容差：\r\n → \n
  perl -pi -e 's/\r\n/\n/g' "$CACHE/$slug.got" 2>/dev/null
  [ -f "$CACHE/$slug.exp" ] || : > "$CACHE/$slug.exp"

  local verdict="OK" why="" three_n=0
  if ! cmp -s "$CACHE/$slug.exp" "$CACHE/$slug.got"; then
    verdict="DIFF"; why="合并流不同"
  elif [ "$ecode" != "$gcode" ]; then
    verdict="CODE"; why="退出码 go=$ecode rgoc=$gcode"
  fi
  if [ "$SSA" = 1 ]; then
    ( cd "$ROOT" && "$DRIVER" run --via=ssa "$src" ) > "$CACHE/$slug.ssa" 2>"$CACHE/$slug.ssaerr"
    local scode=$?
    perl -pi -e 's/\r\n/\n/g' "$CACHE/$slug.ssa" 2>/dev/null
    # M 侧（构建期拒绝）：**白名单码 + 退出码 1** ⇒ SKIP，不算红。
    #
    # 判据必须是「stderr 里有 `SSA-UNSUPPORTED-*`」，不能只看退出码 1 ——
    # 退出码 1 也可能是别的静态错（那必须照红）。否则「拒绝」就成了万能挡箭牌：
    # 任何 SSA 侧的红都能被一句话解释掉，装置等于没有。
    if [ "$scode" = 1 ] && grep -q 'SSA-UNSUPPORTED-' "$CACHE/$slug.ssaerr" 2>/dev/null; then
      local code
      code="$(grep -o 'SSA-UNSUPPORTED-[A-Z-]*' "$CACHE/$slug.ssaerr" | head -1)"
      verdict="SKIP"; why="$code"
    elif [ "$verdict" = "OK" ] && ! cmp -s "$CACHE/$slug.ssa" "$CACHE/$slug.got"; then
      verdict="DIFF"; why="HIR 与 SSA 合并流不同"
    elif [ "$verdict" = "OK" ] && [ "$scode" != "$gcode" ]; then
      verdict="CODE"; why="退出码 hir=$gcode ssa=$scode"
    fi
  fi
  if [ "$THREE" = 1 ]; then
    # ⚠️ 命名规则是**把 `.go` 换成 `.out`**，不是「在 `.go` 后面再接 `.out`」。
    # 依据：官方 `testdir_test.go:1176`（`filename[:len-3] + ".out"`）。
    # 2026-10-23 修复：原先写成 `"$src.out"` ⇒ 拼出 `helloworld.go.out`（永远不存在）
    # ⇒ 三方比对的 `if` 从不成立、静默退化成两方比对。**这正是 `T-EX-43/70` 要防的
    # 形态**：装置坏了却报绿。修复后入选集里有 3 个文件真正走到这一支。
    local dotout="${src%.go}.out"
    if [ -f "$dotout" ]; then
      three_n=1
      perl -pe 's/\r\n/\n/g' "$dotout" > "$CACHE/$slug.dotout"
      if ! cmp -s "$CACHE/$slug.dotout" "$CACHE/$slug.exp"; then
        verdict="DRIFT"; why=".out 与 go run 脱节"
      fi
    fi
  fi
  # 第 4 列 = 这一行**真的**做了三方比对（有 `.out`）。汇总时数出来，
  # 免得「一个 .out 都没有」时 `--three-way` 仍然报「绿」—— 空集合上的全称命题。
  printf '%s\t%s\t%s\t%s\n' "$verdict" "$rel" "$why" "$three_n" > "$CACHE/$slug.meta"
}

export -f one
export ROOT DRIVER CACHE THREE CORPUS SSA

# 串行更稳（go run 会抢 GOCACHE 锁）；候选只有几十个，慢不到哪去。
n=0; bad=0; skip=0; three_seen=0
while IFS= read -r rel; do
  case "$rel" in ''|'#'*) continue ;; esac
  rel="${rel%%[[:space:]]*}"
  [ -n "$FILTER" ] && case "$rel" in *"$FILTER"*) ;; *) continue ;; esac
  one "$rel"
  slug="$(printf '%s' "$rel" | tr '/' '_')"
  verdict="$(cut -f1 "$CACHE/$slug.meta")"
  why="$(cut -f3 "$CACHE/$slug.meta")"
  [ "$(cut -f4 "$CACHE/$slug.meta")" = "1" ] && three_seen=$((three_seen+1))
  n=$((n+1))
  case "$verdict" in
    OK) [ "$MODE_LIST" = 1 ] && printf 'OK   %s\n' "$rel" ;;
    SKIP) skip=$((skip+1)); printf 'SKIP %s   %s\n' "$rel" "$why" ;;
    *)  bad=$((bad+1)); printf '%-5s %s   %s\n' "$verdict" "$rel" "$why" ;;
  esac
done < "$LIST"

printf -- '---- %d 个文件：绿 %d / 跳过 %d / 红 %d%s（cache=%s）\n' \
  "$n" "$((n-bad-skip))" "$skip" "$bad" \
  "$([ "$SSA" = 1 ] && echo '（hir vs ssa vs go run）' || true)" "$CACHE"
if [ "$THREE" = 1 ]; then
  # `--three-way` 的分母**不是** $n，而是「真的有 `.out`」的 $three_seen。
  # 一个都没有时报 0，不报「绿」—— 空集合上「全部一致」是恒真句，不是证据。
  printf -- '---- 三方比对：%d 个文件有 .out（其余 %d 个按「期望为空」只做两方）\n' \
      "$three_seen" "$((n-three_seen))"
  if [ "$three_seen" = 0 ]; then
    echo "run-collide.sh: --three-way 但没有任何 .out 可比 —— 这条不算兑现" >&2
    exit 1
  fi
fi
[ "$bad" = 0 ] || exit 1
