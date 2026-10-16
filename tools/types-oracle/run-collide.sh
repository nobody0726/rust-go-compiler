#!/usr/bin/env bash
# 腿乙门禁（oracle 对撞）—— T-TY-91（C7 契约；M3-tests.md §7.2 / §5.1）
#
# 判据：对 fixture 目录里每个 .go，**oracle 侧**（tools/types-oracle，
# 标准库 go/types）与 **rgoc 侧**（`rgoc-driver types --dump`）各出一份
# 中性类型 dump，两边**逐字节一致**（C6/C7 的注释行与空行按 C7 §2 规则 1 忽略）。
#
#   全过 -> exit 0
#   有差异 / 任一侧没产出 -> 打印首个不一致行 -> exit 1
#   用法错 -> exit 2
#
# 用法：
#   tools/types-oracle/run-collide.sh <fixture-dir> [--lang go1.N]
#
# ⚠️ 本脚本**在容器内跑**（宿主侧用 scripts/in-container.sh 包一层）：
#   scripts/in-container.sh bash tools/types-oracle/run-collide.sh tools/types-oracle/testdata
#   —— 它要同时驱动 `go` 与 `cargo`，宿主上两个都没有（04 §7：一律容器内）。
#
# ── 设计 ──────────────────────────────────────────────────────────────
# ① **必须两侧都非空**才比对。「两边都空」是最阴的一种通过 —— 它意味着
#    两边都没跑起来，而 diff 会高呼「一致」。
# ② 注释/空行两侧都忽略（C7 §2 规则 1）。oracle 写 `# source: oracle`，
#    rgoc 写 `# source: rgoc` —— 来源如实标注，正是**刻意保留**的那一处不同。
# ③ `--lang` 原样透传给两侧（`D-M3-15`：版本门控按文件；这里是显式覆盖）。

set -uo pipefail

ORACLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$ORACLE_DIR/../.." && pwd)"

usage() {
    cat >&2 <<'EOF'
用法：tools/types-oracle/run-collide.sh <fixture-dir> [--lang go1.N]

  <fixture-dir>  含 .go fixture 的目录（每个文件单独对撞）
  --lang go1.N   显式版本门控，原样透传给两侧（缺省 = 各自按文件读）
EOF
}

FIXDIR=""
LANG_ARG=()
while [ $# -gt 0 ]; do
    case "$1" in
        --lang)
            [ $# -ge 2 ] || { echo "  --lang 缺值" >&2; usage; exit 2; }
            LANG_ARG=(--lang "$2"); shift 2 ;;
        -h|--help) usage; exit 2 ;;
        --*) echo "  未知选项：$1" >&2; usage; exit 2 ;;
        *)
            if [ -n "$FIXDIR" ]; then
                echo "  只接受一个 fixture 目录（多余的：$1）" >&2; usage; exit 2
            fi
            FIXDIR="$1"; shift ;;
    esac
done
if [ -z "$FIXDIR" ]; then echo "  缺少 fixture 目录" >&2; usage; exit 2; fi
if [ ! -d "$FIXDIR" ]; then echo "  fixture 目录不存在：$FIXDIR" >&2; exit 2; fi
# 绝对化：下面两侧都要 `cd` 到别处（oracle 进 tools/、rgoc 进 rgoc/），
# 相对路径会在 cd 之后指错地方 —— 症状是「oracle 侧没有产出」，很难猜。
FIXDIR="$(cd "$FIXDIR" && pwd)"

# C7 §2 规则 1：以 `#` 开头的行是注释、空行忽略 —— 两侧都丢。
strip() { grep -v -e '^#' -e '^[[:space:]]*$' || true; }

oracle_dump() {
    ( cd "$ORACLE_DIR" && go run . "$1" "${LANG_ARG[@]+"${LANG_ARG[@]}"}" ) 2>/dev/null | strip
}
rgoc_dump() {
    ( cd "$ROOT/rgoc" && cargo run -q -p rgoc-driver -- types --dump "$1" \
        "${LANG_ARG[@]+"${LANG_ARG[@]}"}" ) 2>/dev/null | strip
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

n=0
fail=0
for f in "$FIXDIR"/*.go; do
    [ -f "$f" ] || continue
    b="$(basename "$f")"
    n=$((n+1))

    oracle_dump "$f" > "$tmp/o.txt"
    rgoc_dump   "$f" > "$tmp/r.txt"

    # ① 两侧都必须非空（见设计注 ①）
    if [ ! -s "$tmp/o.txt" ]; then
        printf '  !! %s：oracle 侧没有产出 dump\n' "$b"; fail=1; continue
    fi
    if [ ! -s "$tmp/r.txt" ]; then
        printf '  !! %s：**rgoc 侧**没有产出 dump' "$b"
        printf '（`rgoc types --dump` 尚未实现 —— M3-plan.md Task 32 才接线）\n'
        fail=1; continue
    fi

    # ② 逐字节比对，首个不一致行即判据
    if ! diff -u "$tmp/o.txt" "$tmp/r.txt" > "$tmp/d.txt"; then
        printf '  !! %s：两侧 dump 不一致，首个差异：\n' "$b"
        sed -n '1,12p' "$tmp/d.txt" | sed 's/^/     /'
        fail=1; continue
    fi
    printf '  ✓ %s\n' "$b"
done

if [ "$n" -eq 0 ]; then
    echo "  fixture 目录里没有 .go：$FIXDIR" >&2
    exit 2
fi

if [ "$fail" -ne 0 ]; then
    echo "run-collide: FAIL（$n 个 fixture）"
    exit 1
fi
echo "run-collide: OK（$n 个 fixture，两侧逐字节一致）"
