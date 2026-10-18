#!/usr/bin/env bash
# 腿乙门禁（oracle 对撞）—— T-TY-91（C7 dump）、T-TY-92/93（identity / representable）
#                     与 T-TY-95（确定性）。（M3-tests.md §7.2 / §5.1/§5.2）
#
# 判据：对 fixture 目录里每个 .go，**oracle 侧**（tools/types-oracle，标准库
# go/types）与 **rgoc 侧**（`rgoc-driver types …`）各出一份规范文本，两边**逐字节
# 一致**（`#` 开头的注释行与空行按 C7 §2 规则 1 忽略）。
#
#   --dump（缺省）：C7 中性类型 dump；fixture = 目录里所有 *.go（**响亮跳过
#                   `check-*` 族** —— 那些夹具按契约故意含类型错误，不是 dump 夹具）
#   --cmp         ：`//rgoc:cmp` 判定行；fixture = 目录里 cmp-*.go
#   --corpus <dir>：**官方语料**模式（`T-TY-94`）。`<dir>` = `types/testdata` 的根；
#                   文件清单来自 `corpus-selected.txt`（**唯一**清单 —— 脚本**不**自己
#                   重算机械筛，免得口径在 bash 与 `collide_corpus.rs` 两边漂移）。
#                   逐文件跑 dump 对撞（两侧非空 + stderr 空 + 重复运行确定，与上面同）。
#
#   --check <dir> ：**静态检查面**（`T-CK-91/92`；C8 契约 `docs/contracts/C8-check-diags.md`，
#                   比较算法 = C8 §6）。`<dir>` = `types/testdata` 的根；清单缺省取
#                   `check-selected.txt`（`--list <清单>` 可换，如 check-fixedbugs-waveA.txt）。
#                   逐文件跑两侧 `--check` 渲染：
#                     腿① marker 判定：oracle 头 `# markers: N` >0 ⇒ rgoc 诊断行数 >0；
#                                     N==0 ⇒ rgoc 诊断行数 ==0（`T-CK-95` 互洽：
#                                     「两侧都空」只在 N==0 时才算过）。
#                     腿② 剥列 (行号,类别) 多重集逐项一致（C8 §6-5）；列差异仅**提示**
#                                     （C8 §6-6；升格走修订条款，见 C8 §6）。
#                   `check/` 前缀文件两侧都加 `--test-hooks`（`F13`）；
#                   两侧 stderr 必须空 + 两侧各自重复运行 sha256 一致；
#                   渲染出现类别 `other` ⇒ 红（`T-CK-94`；适配表未命中）。
#                   首个不一致 ⇒ 打印现场 exit 1；清单行不在语料 ⇒ exit 2。
#
#   全过 -> exit 0
#   有差异 / 任一侧没产出 / 任一侧有 stderr / 重复运行不确定 -> 打印现场 -> exit 1
#   用法错（含清单里的路径不在语料里）-> exit 2
#
# 用法：
#   tools/types-oracle/run-collide.sh <fixture-dir> [--cmp] [--lang go1.N]
#   tools/types-oracle/run-collide.sh --corpus <testdata-dir> [--lang go1.N]
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
      tools/types-oracle/run-collide.sh --corpus <testdata-dir> [--lang go1.N]
      tools/types-oracle/run-collide.sh --check [--list <清单>] <testdata-dir>

  <fixture-dir>  含 .go fixture 的目录（每个文件单独对撞）
  --corpus <dir> 官方语料模式：<dir> = types/testdata 根，文件清单取
                 corpus-selected.txt（与 --cmp/--check 互斥）
  --cmp          对撞 `//rgoc:cmp` 判定行（缺省 = 对撞 C7 dump）
  --check        静态检查面（C8 §6 比较算法；与 --cmp/--corpus/--lang 互斥）
  --list <清单>  仅 --check：替换缺省清单（check-selected.txt），
                 如 check-fixedbugs-waveA.txt
  --lang go1.N   显式版本门控，原样透传给两侧（缺省 = 各自按文件读）
EOF
}

FIXDIR=""
CORPUS=""
MODE="dump"
LANG_ARG=()
CHECK_LIST=""
while [ $# -gt 0 ]; do
    case "$1" in
        --cmp)
            [ "$MODE" != check ] || { echo "  --cmp 与 --check 互斥" >&2; usage; exit 2; }
            MODE="cmp"; shift ;;
        --check)
            [ "$MODE" != cmp ] || { echo "  --check 与 --cmp 互斥" >&2; usage; exit 2; }
            [ -z "$CORPUS" ] || { echo "  --check 与 --corpus 互斥" >&2; usage; exit 2; }
            MODE="check"; shift ;;
        --list)
            [ $# -ge 2 ] || { echo "  --list 缺值" >&2; usage; exit 2; }
            CHECK_LIST="$2"; shift 2 ;;
        --corpus)
            [ "$MODE" != check ] || { echo "  --corpus 与 --check 互斥" >&2; usage; exit 2; }
            [ $# -ge 2 ] || { echo "  --corpus 缺值" >&2; usage; exit 2; }
            CORPUS="$2"; shift 2 ;;
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
# `--corpus` 与 `--cmp` 互斥：语料里没有 `//rgoc:cmp` 指令行，混着用会「什么都不查」
# 而报成功 —— 静默失效正是本仓最防的那种红。
if [ -n "$CORPUS" ] && [ "$MODE" = cmp ]; then
    echo "  --corpus 与 --cmp 互斥（语料里没有 //rgoc:cmp 指令行）" >&2; usage; exit 2
fi
# `--list` 只对 `--check` 有意义 —— 静默忽略会让人以为换了清单（同「静默失效」纪律）。
if [ -n "$CHECK_LIST" ] && [ "$MODE" != check ]; then
    echo "  --list 只在 --check 模式下有效" >&2; usage; exit 2
fi
# `--check` 的语言版本只来自文件首行指令（C8 §5 规则 1；oracle 侧 --check 同样拒绝 --lang）。
if [ "$MODE" = check ] && [ "${#LANG_ARG[@]}" -gt 0 ]; then
    echo "  --check 与 --lang 互斥（语言版本只来自文件首行）" >&2; usage; exit 2
fi
if [ -n "$CORPUS" ]; then
    [ -z "$FIXDIR" ] || { echo "  --corpus 与 <fixture-dir> 互斥" >&2; usage; exit 2; }
    [ -d "$CORPUS" ] || { echo "  语料目录不存在：$CORPUS" >&2; exit 2; }
    FIXDIR="$(cd "$CORPUS" && pwd)"
elif [ -z "$FIXDIR" ]; then
    if [ "$MODE" = check ]; then
        echo "  --check 需要 <testdata 根> 位置参数（如 go_source_code/src/internal/types/testdata）" >&2
        usage; exit 2
    fi
    echo "  缺少 fixture 目录（或 --corpus <dir>）" >&2; usage; exit 2
elif [ ! -d "$FIXDIR" ]; then
    echo "  fixture 目录不存在：$FIXDIR" >&2; exit 2
fi
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

# ── 文件清单 ──────────────────────────────────────────────────────────
# fixture 模式：目录里 `$GLOB` 匹配的文件。
# 清单模式（`--corpus` / `--check`）：清单文件是**唯一**输入（`corpus-selected.txt` /
# `check-selected.txt`）。脚本不重算机械筛 —— 两份口径一旦漂移，就会出现
# 「门禁少查了几个文件却报成功」这种静默失效；清单本身由 `collide_corpus.rs`
# （M3）与 `check_corpus.rs`（M4）断言「恰好等于候选 − 排除」。
FILES=()
NAMES=()
if [ -n "$CORPUS" ] || [ "$MODE" = check ]; then
    if [ "$MODE" = check ]; then
        LIST="${CHECK_LIST:-$ORACLE_DIR/check-selected.txt}"
    else
        LIST="$ORACLE_DIR/corpus-selected.txt"
    fi
    [ -f "$LIST" ] || { echo "  清单不存在：$LIST" >&2; exit 2; }
    while IFS= read -r rel; do
        case "$rel" in ''|'#'*) continue ;; esac
        if [ ! -f "$FIXDIR/$rel" ]; then
            echo "  清单里的路径不在语料里：$rel" >&2
            echo "  （清单：$LIST；语料根：$FIXDIR）" >&2
            exit 2
        fi
        FILES+=("$FIXDIR/$rel")
        NAMES+=("$rel")
    done < "$LIST"
else
    # ⚠️ `$GLOB` **不能加引号** —— 加了就没有通配展开，`[ -f ]` 全 false，
    # 症状是「fixture 目录里没有 …」而目录里明明有（刚踩过）。
    #
    # ⚠️ dump 模式的 `*.go` 要**响亮跳过 `check-*` 族**：check 夹具按 `C8` §8
    # 契约故意含类型错误（dump 会是半个 `invalid`，对撞没有意义 ——
    # `M3-tests.md` §5.1 同款理由）。静默不选是另一头的错：新族混进来时会
    # 「门禁少查了文件却报成功」。所以这里**打印跳过行**。
    for f in "$FIXDIR"/$GLOB; do
        [ -f "$f" ] || continue
        case "$MODE:$(basename "$f")" in
            dump:check-*)
                printf '  - %s（check 面夹具，dump 腿不适用，跳过）\n' "$(basename "$f")"
                continue ;;
        esac
        FILES+=("$f")
        NAMES+=("$(basename "$f")")
    done
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

# ══ --check：C8 对撞（`T-CK-91/92`）════════════════════════════════════
# 与上面 dump 路径的逐字节对撞**分开**：判据 = C8 §6 —— 剥列 (行号,类别) 多重集
# （列差异只提示）＋ 腿① marker 判定 ＋ 两侧 stderr 空 ＋ 重复运行确定。
# 两侧输出形态目前不同（oracle 已是 C8；rgoc 侧 Task 20 前是临时 C1 形态），
# 所以解析必须**严格**：不合 C8 行格式的行计入 bad —— 不许静默丢行。
# （「解析器把两侧都解析成空集合，然后高呼一致」是本仓反复防的假绿。）
if [ "$MODE" = check ]; then
    TAB="$(printf '\t')"

    # 两侧都**先构建一次、直接调二进制**：不用 `go run` 的原因很具体 ——
    # 程序退出码非 0 时 `go run` **自己**往 stderr 加一行 `exit status N`
    # （而 `--check` 的正常退出码就是 1），会直接污染 C8 §6-1 的 stderr 判据；
    # 顺带把 75 文件的进程启动开销压到最低。
    ORACLE_BIN="$tmp/types-oracle"
    RGOC_BIN="$ROOT/rgoc/target/debug/rgoc-driver"
    ( cd "$ORACLE_DIR" && go build -buildvcs=false -o "$ORACLE_BIN" . ) || {
        echo "  oracle 侧构建失败（go build；容器内跑）" >&2; exit 2
    }
    ( cd "$ROOT/rgoc" && cargo build -q -p rgoc-driver ) || {
        echo "  rgoc 侧构建失败（cargo build -p rgoc-driver）" >&2; exit 2
    }
    [ -x "$RGOC_BIN" ] || { echo "  rgoc 二进制不存在：$RGOC_BIN" >&2; exit 2; }

    # 数据行 → `L\tC\tK`；不合 C8 行格式的行数写进 $3（**不**静默消失）。
    parse_c8_rows() { # $1=文本文件 $2=rows 输出 $3=bad 计数输出
        awk -v badfile="$3" '
            {
                i = index($0, ":")
                if (i == 0) { bad++; next }
                L = substr($0, 1, i-1)
                rest = substr($0, i+1)
                j = index(rest, "\t")
                if (j == 0) { bad++; next }
                C = substr(rest, 1, j-1)
                K = substr(rest, j+1)
                if (L !~ /^[0-9]+$/ || C !~ /^[0-9]+$/ || K !~ /^[a-z][a-z-]*$/) { bad++; next }
                printf "%s\t%s\t%s\n", L, C, K
            }
            END { print bad+0 > badfile }
        ' "$1" > "$2"
    }
    oracle_check_run() { # $1=file $2=stdout $3=stderr $4=退出码文件 ; $5=hooks (y/n)
        local hooks=()
        [ "$5" = y ] && hooks=(--test-hooks)
        "$ORACLE_BIN" --check "${hooks[@]+"${hooks[@]}"}" "$1" >"$2" 2>"$3"
        echo $? > "$4"
    }
    rgoc_check_run() { # 同上；rgoc 侧旗标= `check --diag-dump [--test-hooks] <file>`
        local hooks=()
        [ "$5" = y ] && hooks=(--test-hooks)
        "$RGOC_BIN" check --diag-dump "${hooks[@]+"${hooks[@]}"}" "$1" >"$2" 2>"$3"
        echo $? > "$4"
    }

    if [ "${#FILES[@]}" -eq 0 ]; then
        echo "  检查清单是空的：$LIST" >&2
        exit 2
    fi

    # 现场打印 + **首个不一致即退出**（exit 1），不接着跑后面的文件 ——
    # RED 期第一个文件就能把「哪一侧、差在哪」说清，不必等 75 个跑完。
    site() {
        printf '     —— 首个不一致现场（%s）——\n' "$b"
        printf '     oracle 退出码=%s；rgoc 退出码=%s；hooks=%s\n' "$ocode" "$rcode" "$hooks"
        if [ -s "$tmp/o.raw" ]; then
            printf '     oracle stdout 前 6 行：\n'
            sed -n '1,6p' "$tmp/o.raw" | sed 's/^/       /'
        fi
        if [ -s "$tmp/r.raw" ]; then
            printf '     rgoc stdout 前 6 行：\n'
            sed -n '1,6p' "$tmp/r.raw" | sed 's/^/       /'
        fi
        if [ -s "$tmp/o.err" ]; then
            printf '     oracle stderr 前 4 行：\n'
            sed -n '1,4p' "$tmp/o.err" | sed 's/^/       /'
        fi
        if [ -s "$tmp/r.err" ]; then
            printf '     rgoc stderr 前 4 行：\n'
            sed -n '1,4p' "$tmp/r.err" | sed 's/^/       /'
        fi
        # 骨架期（Task 20 前）rgoc 侧恒报 CHK-UNIMPLEMENTED —— 显式点名，
        # 免得下一个人把「尚未实现」误读成「检查器真跑出了不一致」。
        if grep -q 'CHK-UNIMPLEMENTED' "$tmp/r.raw" "$tmp/r.err" 2>/dev/null; then
            printf '     ⇒ **rgoc 侧未实现（CHK-UNIMPLEMENTED）**：Task 20 前属预期（RED 期）。\n'
        fi
    }
    fail_first() { # $1=理由
        printf '  !! %s：%s\n' "$b" "$1"
        site
        printf 'run-collide: FAIL —— 首个不一致（%s；模式 check；清单 %s）\n' "$b" "$LIST"
        exit 1
    }

    ci=0
    while [ "$ci" -lt "${#FILES[@]}" ]; do
        f="${FILES[$ci]}"
        b="${NAMES[$ci]}"
        ci=$((ci+1))

        hooks=n
        case "$b" in check/*) hooks=y ;; esac   # F13：check/ 面注入 assert/trace 预声明

        oracle_check_run "$f" "$tmp/o.raw" "$tmp/o.err" "$tmp/o.code" "$hooks"
        rgoc_check_run   "$f" "$tmp/r.raw" "$tmp/r.err" "$tmp/r.code" "$hooks"
        ocode="$(cat "$tmp/o.code")"
        rcode="$(cat "$tmp/r.code")"

        # 退出码：0/1 都合法（1=有诊断）；2 = 用法/致命 ⇒ 装置级失败，不进行对撞
        [ "$ocode" = 2 ] && fail_first "oracle 侧退出码 2（用法/致命，不进行对撞）"
        [ "$rcode" = 2 ] && fail_first "rgoc 侧退出码 2（用法/致命，不进行对撞）"

        # C8 §6-1：两侧 stderr 皆空
        [ -s "$tmp/o.err" ] && fail_first "oracle 侧有 stderr（C8 §6-1 要求空）"
        [ -s "$tmp/r.err" ] && fail_first "rgoc 侧有 stderr（C8 §6-1 要求空；诊断只许进 --diag-dump 渲染）"

        # C8 §6-2：两侧原始输出各含 ≥2 个 `#` 头行，且都有 `# source:`
        # （防「两侧都空」假绿：一条数据行都没有时，必须能证明两侧都真的跑过）
        oh="$(grep -c '^#' "$tmp/o.raw" || true)"
        rh="$(grep -c '^#' "$tmp/r.raw" || true)"
        [ "${oh:-0}" -ge 2 ] || fail_first "oracle 侧头行不足（${oh:-0} < 2）"
        [ "${rh:-0}" -ge 2 ] || fail_first "rgoc 侧头行不足（${rh:-0} < 2）"
        grep -q '^# source: ' "$tmp/o.raw" || fail_first "oracle 侧缺 \`# source:\` 头行"
        grep -q '^# source: ' "$tmp/r.raw" || fail_first "rgoc 侧缺 \`# source:\` 头行"

        # C8 §6-3/4：解析数据行（不合格式者计入 bad，**不**静默丢）
        strip < "$tmp/o.raw" > "$tmp/o.txt"
        strip < "$tmp/r.raw" > "$tmp/r.txt"
        parse_c8_rows "$tmp/o.txt" "$tmp/o.rows" "$tmp/o.bad"
        parse_c8_rows "$tmp/r.txt" "$tmp/r.rows" "$tmp/r.bad"
        obad="$(cat "$tmp/o.bad")"
        rbad="$(cat "$tmp/r.bad")"
        [ "$obad" = 0 ] || fail_first "oracle 侧有 $obad 行不合 C8 数据行格式"
        [ "$rbad" = 0 ] || fail_first "rgoc 侧有 $rbad 行不合 C8 数据行格式（骨架期临时形态属预期，见现场）"

        # 腿①：marker 判定（`T-CK-95`：「两侧都空」只在 N==0 时才算过）
        M="$(sed -n 's/^# markers: \([0-9][0-9]*\)$/\1/p' "$tmp/o.raw" | head -n1)"
        [ -n "$M" ] || fail_first "oracle 侧缺 \`# markers: N\` 头行（腿①判据不可用）"
        orows="$(wc -l < "$tmp/o.rows" | tr -d ' ')"
        rrows="$(wc -l < "$tmp/r.rows" | tr -d ' ')"
        if [ "$M" -gt 0 ] && [ "$rrows" -eq 0 ]; then
            fail_first "腿①违反：markers=$M>0（reject）但 rgoc 侧诊断行数 0"
        fi
        if [ "$M" -eq 0 ] && [ "$rrows" -ne 0 ]; then
            fail_first "腿①违反：markers=0（accept）但 rgoc 侧诊断行数 $rrows>0"
        fi
        if [ "$M" -eq 0 ] && [ "$orows" -ne 0 ]; then
            fail_first "oracle 侧自不一致：markers=0 但渲染有 $orows 条诊断"
        fi

        # `other` 不许出现在入选渲染（`T-CK-94`：适配表未命中 ⇒ 红）
        oother="$(awk -F'\t' '$3=="other"{n++} END{print n+0}' "$tmp/o.rows")"
        rother="$(awk -F'\t' '$3=="other"{n++} END{print n+0}' "$tmp/r.rows")"
        if [ "$oother" -ne 0 ] || [ "$rother" -ne 0 ]; then
            printf '     other 行（oracle=%s，rgoc=%s）前 5 条：\n' "$oother" "$rother"
            awk -F'\t' '$3=="other"' "$tmp/o.rows" | sed -n '1,5p' | sed 's/^/       o /'
            awk -F'\t' '$3=="other"' "$tmp/r.rows" | sed -n '1,5p' | sed 's/^/       r /'
            fail_first "渲染出现类别 other（适配表未命中，T-CK-94）"
        fi

        # C8 §6-5 硬判据：剥列 (行号,类别) 多重集（稳定排序后逐项比较）
        awk -F'\t' '{print $1 "\t" $3}' "$tmp/o.rows" | LC_ALL=C sort -s -t "$TAB" -k1,1n -k2,2 > "$tmp/o.lk"
        awk -F'\t' '{print $1 "\t" $3}' "$tmp/r.rows" | LC_ALL=C sort -s -t "$TAB" -k1,1n -k2,2 > "$tmp/r.lk"
        if ! cmp -s "$tmp/o.lk" "$tmp/r.lk"; then
            printf '     (行号,类别) 差异（< oracle / > rgoc；前 16 行）：\n'
            diff -u "$tmp/o.lk" "$tmp/r.lk" | sed -n '1,18p' | sed 's/^/       /'
            fail_first "(行号,类别) 多重集不等（C8 §6-5 硬判据）"
        fi

        # 确定性：两侧再各跑一次，raw sha256 与退出码必须一致（沿 T-TY-95）
        oracle_check_run "$f" "$tmp/o2.raw" "$tmp/o2.err" "$tmp/o2.code" "$hooks"
        rgoc_check_run   "$f" "$tmp/r2.raw" "$tmp/r2.err" "$tmp/r2.code" "$hooks"
        if [ "$(sha "$tmp/o.raw")" != "$(sha "$tmp/o2.raw")" ] \
            || [ "$(cat "$tmp/o2.code")" != "$ocode" ]; then
            printf '     oracle 侧重复运行不确定（diff 前 12 行）：\n'
            diff -u "$tmp/o.raw" "$tmp/o2.raw" | sed -n '1,14p' | sed 's/^/       /'
            fail_first "oracle 侧重复运行不一致（非确定性）"
        fi
        if [ "$(sha "$tmp/r.raw")" != "$(sha "$tmp/r2.raw")" ] \
            || [ "$(cat "$tmp/r2.code")" != "$rcode" ]; then
            printf '     rgoc 侧重复运行不确定（diff 前 12 行）：\n'
            diff -u "$tmp/r.raw" "$tmp/r2.raw" | sed -n '1,14p' | sed 's/^/       /'
            fail_first "rgoc 侧重复运行不一致（非确定性）"
        fi

        # C8 §6-6 提示层：列差异（不影响 pass/fail）
        LC_ALL=C sort -s -t "$TAB" -k1,1n -k3,3 -k2,2n "$tmp/o.rows" > "$tmp/o.full"
        LC_ALL=C sort -s -t "$TAB" -k1,1n -k3,3 -k2,2n "$tmp/r.rows" > "$tmp/r.full"
        coldiff="$(diff "$tmp/o.full" "$tmp/r.full" | grep -c '^[<>]' || true)"
        printf '  ✓ %s（oracle %s 行 / rgoc %s 行；markers=%s；列差异 %s（提示层，diff ±行合计））\n' \
            "$b" "$orows" "$rrows" "$M" "${coldiff:-0}"
    done

    echo "run-collide: OK（${#FILES[@]} 个文件，模式 check，语料=$FIXDIR，清单=$LIST；腿① + 腿②剥列多重集 + stderr 空 + 重复运行确定）"
    exit 0
fi

# ⚠️ 迭代**数组**（不是 glob）：语料模式的清单是文件，见上面「文件清单」一节。
i=0
while [ "$i" -lt "${#FILES[@]}" ]; do
    f="${FILES[$i]}"
    b="${NAMES[$i]}"
    i=$((i+1))
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
    if [ -n "$CORPUS" ]; then
        echo "  入选清单是空的（$ORACLE_DIR/corpus-selected.txt）" >&2
    elif [ "$MODE" = dump ]; then
        echo "  fixture 目录里没有可对撞的 dump fixture（$GLOB，不含 check-* 族）：$FIXDIR" >&2
    else
        echo "  fixture 目录里没有 $GLOB 匹配的文件：$FIXDIR" >&2
    fi
    exit 2
fi

SRC="fixture=$FIXDIR"
[ -n "$CORPUS" ] && SRC="语料=$FIXDIR"
if [ "$fail" -ne 0 ]; then
    echo "run-collide: FAIL（$n 个 fixture，模式 $MODE，$SRC）"
    exit 1
fi
echo "run-collide: OK（$n 个 fixture，模式 $MODE，$SRC；逐字节一致 + stderr 空 + 重复运行确定）"
