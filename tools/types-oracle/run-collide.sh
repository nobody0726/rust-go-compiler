#!/usr/bin/env bash
# 腿乙门禁（oracle 对撞）—— T-TY-91（C7 dump）、T-TY-92/93（identity / representable）
#                     与 T-TY-95（确定性）。（M3-tests.md §7.2 / §5.1/§5.2）
#
# 判据：对 fixture 目录里每个 .go，**oracle 侧**（tools/types-oracle，标准库
# go/types）与 **rgoc 侧**（`rgoc-driver types …`）各出一份规范文本，两边**逐字节
# 一致**（`#` 开头的注释行与空行按 C7 §2 规则 1 忽略）。
#
#   --dump（缺省）：C7 中性类型 dump；fixture = 目录里所有 *.go
#   --cmp         ：`//rgoc:cmp` 判定行；fixture = 目录里 cmp-*.go
#
#   全过 -> exit 0
#   有差异 / 任一侧没产出 / 任一侧有 stderr / 重复运行不确定 -> 打印现场 -> exit 1
#   用法错 -> exit 2
#
# 用法：
#   tools/types-oracle/run-collide.sh <fixture-dir> [--cmp] [--lang go1.N]
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
# ④ **两侧的 stderr 都要空**（`T-TY-91` 配套）：脚本从前的写法是 `2>/dev/null`，
#    那会把「fixture 有类型错」「工具报内部错」一律吞掉 —— 症状是 diff 报
#    「不一致」，而真因在没人看的 stderr 里。现在 stderr 进临时文件并在
#    **任何一侧非空**时报失败（fixture 必须合法，`M3-tests.md` §5.1）。
# ⑤ **确定性**（`T-TY-95`）：同一个 fixture **重复跑两次**，两侧各自的 sha256
#    必须一致。map 迭代序、类型编号缓存之类的非确定性一旦出现，逐字节对撞会
#    变成间歇性失败 —— 那种红比没有红更糟。oracle 与 rgoc **两侧都查**
#    （计划只点了 oracle 侧；rgoc 是我们自己新写的，非确定性风险只高不低）。
# ⑥ **`--cmp` 的排除集**（`D-M3-13` / `U-M3-05`）：rgoc 侧对「两侧都是匿名
#    接口、且至少一侧含项集或 comparable」的 identity 请求**显式拒绝**
#    （`result = reject`）。这类条目按设计**不参与**逐对比较 —— 脚本把它们
#    从**两侧**都剔掉再 diff，并把剔掉的条数与行号**打印出来**（不是静默）。
#    「排除集 == 声明的集合」这条更强的断言在 `collide_cmp.rs` 里（靠 fixture
#    里的 `// rgoc:expect-reject` 标记），脚本只保证「不拿它们当失败」。

set -uo pipefail

ORACLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$ORACLE_DIR/../.." && pwd)"

usage() {
    cat >&2 <<'EOF'
用法：tools/types-oracle/run-collide.sh <fixture-dir> [--cmp] [--lang go1.N]

  <fixture-dir>  含 .go fixture 的目录（每个文件单独对撞）
  --cmp          对撞 `//rgoc:cmp` 判定行（缺省 = 对撞 C7 dump）
  --lang go1.N   显式版本门控，原样透传给两侧（缺省 = 各自按文件读）
EOF
}

FIXDIR=""
MODE="dump"
LANG_ARG=()
while [ $# -gt 0 ]; do
    case "$1" in
        --cmp) MODE="cmp"; shift ;;
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

# ⚠️ 两侧的**选项名不一样**：oracle 侧 dump 是缺省（不认 `--dump`），rgoc 侧要显式
# `types --dump`（不给 `--dump` 时只打印摘要 —— 症状是「rgoc 侧没有产出 dump」，
# 而它其实产出了别的东西）。所以两个数组分开。
if [ "$MODE" = cmp ]; then
    ORACLE_ARGS=(--cmp)
    RGOC_ARGS=(--cmp)
    GLOB="cmp-*.go"
else
    ORACLE_ARGS=()
    RGOC_ARGS=(--dump)
    GLOB="*.go"
fi

# C7 §2 规则 1：以 `#` 开头的行是注释、空行忽略 —— 两侧都丢。
strip() { grep -v -e '^#' -e '^[[:space:]]*$' || true; }

# 两侧的驱动：**raw 输出进 stdout**（`--cmp` 的 `# reject` 行只在 raw 里能看到），
# **stderr 进第二个参数给的文件**（见设计注 ④）。
oracle_run() {
    ( cd "$ORACLE_DIR" && go run . "${ORACLE_ARGS[@]+"${ORACLE_ARGS[@]}"}" "$1" \
        "${LANG_ARG[@]+"${LANG_ARG[@]}"}" ) 2>"$2"
}
rgoc_run() {
    ( cd "$ROOT/rgoc" && cargo run -q -p rgoc-driver -- types "${RGOC_ARGS[@]+"${RGOC_ARGS[@]}"}" "$1" \
        "${LANG_ARG[@]+"${LANG_ARG[@]}"}" ) 2>"$2"
}
sha() { sha256sum "$1" | cut -d' ' -f1; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

n=0
fail=0

show_stderr() {
    printf '     %s 的 stderr（前 10 行）：\n' "$1"
    sed -n '1,10p' "$2" | sed 's/^/       /'
}

# ⑦ 把 rgoc 标了 reject 的行从**两侧**剔掉（`--cmp` 专用）。剔掉的行号与条数
# 打印出来 —— 「静默跳过样本」是本仓反复踩过的坑。
apply_exclusions() {
    sed -n 's/^# reject \([0-9][0-9]*\) .*/\1/p' "$tmp/r.raw" | sort -n > "$tmp/rej.txt"
    if [ ! -s "$tmp/rej.txt" ]; then
        printf ''
        return 0
    fi
    local why
    why="$(sed -n 's/^# reject [0-9][0-9]* \(.*\)/\1/p' "$tmp/r.raw" | sort -u | tr '\n' ' ')"
    printf '     排除 %s 条（行 %s；%s）\n' \
        "$(wc -l < "$tmp/rej.txt" | tr -d ' ')" \
        "$(tr '\n' ',' < "$tmp/rej.txt" | sed 's/,$//')" \
        "${why% }"
    awk 'NR==FNR{rej[$1]=1;next} {split($0,a,"\t"); if (!(a[1] in rej)) print}' \
        "$tmp/rej.txt" "$tmp/o.txt" > "$tmp/o.excl"
    mv "$tmp/o.excl" "$tmp/o.txt"
    awk 'NR==FNR{rej[$1]=1;next} {split($0,a,"\t"); if (!(a[1] in rej)) print}' \
        "$tmp/rej.txt" "$tmp/r.txt" > "$tmp/r.excl"
    mv "$tmp/r.excl" "$tmp/r.txt"
}

# ⚠️ `$GLOB` **不能加引号** —— 加了就没有通配展开，`[ -f ]` 全 false，
# 症状是「fixture 目录里没有 …」而目录里明明有（刚踩过）。
for f in "$FIXDIR"/$GLOB; do
    [ -f "$f" ] || continue
    b="$(basename "$f")"
    n=$((n+1))

    oracle_run "$f" "$tmp/o.err" > "$tmp/o.raw"
    rgoc_run   "$f" "$tmp/r.err" > "$tmp/r.raw"
    strip < "$tmp/o.raw" > "$tmp/o.txt"
    strip < "$tmp/r.raw" > "$tmp/r.txt"

    # ① 两侧都必须非空（见设计注 ①）
    if [ ! -s "$tmp/o.txt" ]; then
        printf '  !! %s：oracle 侧没有产出规范文本\n' "$b"
        [ -s "$tmp/o.err" ] && show_stderr oracle "$tmp/o.err"
        fail=1; continue
    fi
    if [ ! -s "$tmp/r.txt" ]; then
        printf '  !! %s：**rgoc 侧**没有产出规范文本（真因常在 stderr，见下）\n' "$b"
        [ -s "$tmp/r.err" ] && show_stderr rgoc "$tmp/r.err"
        fail=1; continue
    fi

    # ④ stderr 必须空（fixture 合法 + 工具无内部错）
    if [ -s "$tmp/o.err" ]; then
        printf '  !! %s：oracle 侧有 stderr（fixture 必须**合法**，M3-tests.md §5.1）\n' "$b"
        show_stderr oracle "$tmp/o.err"; fail=1; continue
    fi
    if [ -s "$tmp/r.err" ]; then
        printf '  !! %s：rgoc 侧有 stderr（fixture 必须**合法**）\n' "$b"
        show_stderr rgoc "$tmp/r.err"; fail=1; continue
    fi

    # ⑥ `--cmp`：先剔排除集（见设计注 ⑥）
    if [ "$MODE" = cmp ]; then
        apply_exclusions
        if [ ! -s "$tmp/o.txt" ] || [ ! -s "$tmp/r.txt" ]; then
            printf '  !! %s：剔掉排除集之后有一侧为空（全是拒绝？）\n' "$b"
            fail=1; continue
        fi
    fi

    # ② 逐字节比对，首个不一致行即判据
    if ! diff -u "$tmp/o.txt" "$tmp/r.txt" > "$tmp/d.txt"; then
        printf '  !! %s：两侧不一致（%s），首个差异：\n' "$b" "$MODE"
        sed -n '1,12p' "$tmp/d.txt" | sed 's/^/     /'
        fail=1; continue
    fi

    # ⑤ 确定性：两侧各自再跑一次，sha256 必须一样（T-TY-95）
    oracle_run "$f" "$tmp/o2.err" > "$tmp/o2.raw"
    rgoc_run   "$f" "$tmp/r2.err" > "$tmp/r2.raw"
    strip < "$tmp/o2.raw" > "$tmp/o2.txt"
    strip < "$tmp/r2.raw" > "$tmp/r2.txt"
    if [ "$MODE" = cmp ] && [ -s "$tmp/rej.txt" ]; then
        # 剔同一批行号（排除集由 rgoc 自己的输出决定，两次都一样）
        # ⚠️ 必须在 rej.txt **非空**时才走 awk：`NR==FNR` 在第一个文件为空时会
        # 把第二个文件的第一行也当成「排除行」吃掉 ⇒ 假的确定性失败。
        awk 'NR==FNR{rej[$1]=1;next} {split($0,a,"\t"); if (!(a[1] in rej)) print}' \
            "$tmp/rej.txt" "$tmp/o2.txt" > "$tmp/o2.excl" && mv "$tmp/o2.excl" "$tmp/o2.txt"
        awk 'NR==FNR{rej[$1]=1;next} {split($0,a,"\t"); if (!(a[1] in rej)) print}' \
            "$tmp/rej.txt" "$tmp/r2.txt" > "$tmp/r2.excl" && mv "$tmp/r2.excl" "$tmp/r2.txt"
    fi
    for side in o r; do
        if [ "$(sha "$tmp/$side.txt")" != "$(sha "$tmp/${side}2.txt")" ]; then
            printf '  !! %s：%s 侧**重复运行不一致**（非确定性，T-TY-95）\n' \
                "$b" "$([ "$side" = o ] && echo oracle || echo rgoc)"
            diff -u "$tmp/$side.txt" "$tmp/${side}2.txt" | sed -n '1,12p' | sed 's/^/     /'
            fail=1; continue 2
        fi
    done

    printf '  ✓ %s\n' "$b"
done

if [ "$n" -eq 0 ]; then
    echo "  fixture 目录里没有 $GLOB 匹配的文件：$FIXDIR" >&2
    exit 2
fi

if [ "$fail" -ne 0 ]; then
    echo "run-collide: FAIL（$n 个 fixture，模式 $MODE）"
    exit 1
fi
echo "run-collide: OK（$n 个 fixture，模式 $MODE；逐字节一致 + stderr 空 + 重复运行确定）"
