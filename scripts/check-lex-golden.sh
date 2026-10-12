#!/usr/bin/env bash
# 腿乙门禁 —— SM-M1-LEX-GOLDEN（M1-tests.md §7.2 / §8；M1-design.md §8.2）
#
# 判据（脚本本身就是测试，无第三方测试框架）：
#   --record  用 adapter（token-oracle）重生成全部 golden 到 rgoc/tests/smoke/lex/
#   默认      用 rgoc-driver lex 重算，与 golden 逐行 diff
#   判据      所有条目 diff 为空且守卫全过 -> exit 0；否则打印差异 -> exit 1
#
# 用法：
#   scripts/check-lex-golden.sh            校验（T-L21 八条门禁命令之一）
#   scripts/check-lex-golden.sh --record   重新录制（词法行为变更后才跑）
#
# ── 设计 ──────────────────────────────────────────────────────────────
# ① **归一化只有一处**：driver 输出第 2 行 `# source: rgoc` 换成 golden 登记的
#    来源标记（oracle / hand-written）。第 1 行 `# golden:` 两边传同一路径
#    字符串 ⇒ 逐字节相同（R40 口径）。除此之外**不做任何容错** —— driver 的
#    头格式若漂移，diff 立刻红（这就是 brief Step 3.5 的格式守卫的校验侧）。
# ② **格式守卫（录制侧）**：--record 时对每条**同时**跑 adapter 与 driver，
#    归一后必须逐字节一致才落盘 —— 否则腿乙就是在比两种格式，录出来的
#    golden 永远对不上 driver。
# ③ **反盲区守卫**（本仓同形教训「漏了不会报错」，第四次）：
#    A 每条 golden 第 2 行必须是本 kind 登记的来源标记；
#    B test/syntax/ 的每个 .go 必须以 mode none 登记（语料不许静默缩水）；
#    C fixtures/ 的每个 .txt 必须以 mode none 登记（char_corpus/char_golden
#      两个字符级数据文件除外 —— 它们走 §5.2.1 的另一条通道）；
#    D 不许有孤儿 golden（清单里没有、目录里却存在）；
#    E golden/清单路径不许被 .gitignore 静默排除（否则本地全绿、clone 缺文件）。
# ④ 构建与比对**全部在容器内**（04 §7）；宿主侧只做路径计算与转派。
#    实测 `bash <脚本>` 形态下退出码如实传播（exit 7 -> 7，false -> 1），
#    只有 `bash -lc '…'` 内联手写形态才有 §7.3 纪律 1 的压码问题。

# ═══ 宿主侧：计算路径并转派进容器 ═══════════════════════════════════════
# 注意：本段必须在 macOS 自带的 bash 3.2 下也能跑（不用 mapfile / 关联数组）。
set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SELF="${BASH_SOURCE[0]}"
case "$SELF" in /*) SELF="${SELF#"$ROOT"/}" ;; esac

if [ "${1:-}" != "--inner" ]; then
    cd "$ROOT"
    RGOC_TTY=0 exec scripts/in-container.sh bash "$SELF" --inner "$@"
fi
shift   # 去掉 --inner；以下均在容器内执行（cwd=/work）

# ═══ 容器内侧 ══════════════════════════════════════════════════════════
set -uo pipefail

SMOKE_DIR="rgoc/tests/smoke/lex"
MANIFEST="$SMOKE_DIR/manifest.txt"
DRIVER="rgoc/target/debug/rgoc-driver"
ORACLE_DIR="tools/token-oracle"
OVERLIMIT_PATH="/tmp/rgoc-smoke-overlimit.go"
OVERLIMIT_BYTES=$((64 * 1024 * 1024 + 1))   # MAX_FILE_BYTES + 1（T-L17 钉的值）

ACTION="check"
case "${1:-}" in
    "")        ACTION="check" ;;
    --record)  ACTION="record" ;;
    *)         echo "用法：scripts/check-lex-golden.sh [--record]" >&2
               echo "  （无参数 = 校验；--record = 用 adapter 重录 golden）" >&2
               exit 2 ;;
esac

GATE_TMP="$(mktemp -d)"
trap 'rm -f "$OVERLIMIT_PATH"; rm -rf "$GATE_TMP"' EXIT

# ── 工具函数 ──────────────────────────────────────────────────────────

# 本 kind 登记的来源标记（守卫 A 的依据）
expected_tag() {
    case "$1" in
        corpus|fixture) echo "oracle" ;;
        hand-sparse)    echo "hand-written" ;;
        *)              echo "??" ;;
    esac
}

# golden 落点（规则与 manifest 头注释一致；mode 为 none 时不加后缀）。
# ⚠️ corpus 的镜像路径**必须去掉 `go_source_code/` 前缀** —— .gitignore 的
#    `go_source_code/` 规则无前导斜杠，匹配任意层级的同名目录；若原样镜像，
#    57 个语料 golden 会被**静默排除出 git**（本地门禁全绿、fresh clone 缺文件）。
#    守卫 E（git check-ignore）钉住这一点。
golden_path() {
    local kind="$1" input="$2" mode="$3" base
    case "$kind" in
        corpus)      base="corpus/${input#go_source_code/}" ;;
        fixture)     base="fixtures/${input#tools/token-oracle/fixtures/}" ;;
        hand-sparse) base="hand/overlimit" ;;
        *)           echo "!!bad-kind/$kind" ;;
    esac
    if [ "$mode" = "none" ]; then
        echo "$SMOKE_DIR/$base.golden"
    else
        echo "$SMOKE_DIR/$base.$mode.golden"
    fi
}

# driver：路径原样回显（第 1 行因此与 golden 逐字节相同）
run_driver() {
    local input="$1" mode="$2"
    if [ "$mode" = "none" ]; then
        "$DRIVER" lex "$input"
    else
        "$DRIVER" lex "$input" --mode "$mode"
    fi
}

# adapter：cwd 必须是 oracle 目录（/work 不是 Go module）；录完把第 1 行
# 归一成仓库相对路径（§5.2 冻结格式要求 `# golden: <语料相对路径>`）。
# ⚠️ 若 oracle 第 1 行格式变了，sed 不匹配 ⇒ golden 保留原始行 ⇒ 校验侧变红。
# ⚠️ mode 必须如实传给 adapter —— 首轮录制就因漏传被格式守卫当场抓住
#    （oracle 跑默认 mode 无 messages 节，driver 跑 comments/directives 有）。
run_oracle() {   # $1=input $2=mode → stdout
    local input="$1" mode="$2" abs="$PWD/$1" tmp="$GATE_TMP/oracle_raw"
    if [ "$mode" = "none" ]; then
        ( cd "$ORACLE_DIR" && go run . "$abs" ) >"$tmp" || return $?
    else
        ( cd "$ORACLE_DIR" && go run . --mode "$mode" "$abs" ) >"$tmp" || return $?
    fi
    sed "1s|^# golden: .*|# golden: $input|" "$tmp"
}

# 唯一一处归一化：driver 的来源标记换成 golden 登记的标记
normalize_source() {
    sed "s|^# source: rgoc\$|# source: $1|"
}

create_sparse() {
    rm -f "$OVERLIMIT_PATH"
    truncate -s "$OVERLIMIT_BYTES" "$OVERLIMIT_PATH"
}

show_diff() {   # $1=标题 $2=golden $3=实际
    echo "    --- diff（$1）---"
    diff -u "$2" "$3" | head -20 | sed 's/^/    /' || true
}

# ── 0. 构建 driver（门禁自己保证二进制最新）──────────────────────────
if ! ( cd rgoc && cargo build -q -p rgoc-driver ); then
    echo "FAIL: rgoc-driver 构建失败，门禁中止" >&2
    exit 1
fi
[ -x "$DRIVER" ] || { echo "FAIL: 找不到 $DRIVER" >&2; exit 1; }

# ── 1. 清单 ───────────────────────────────────────────────────────────
[ -f "$MANIFEST" ] || {
    echo "FAIL: 清单缺失：$MANIFEST（应在容器内 /work 下运行）" >&2
    exit 1
}

# ── 1.5 守卫 E 的前提：git 必须可用（否则「防静默忽略」无法执行，门禁中止）──
# safe.directory：/work 的属主与容器内用户不同，git 会以 dubious ownership 拒绝。
if ! command -v git >/dev/null 2>&1 ||
   ! git -c safe.directory=/work -C /work rev-parse --git-dir >/dev/null 2>&1; then
    echo "FAIL 守卫 E: git 不可用（或 /work 不是 git 仓库）—— 防忽略守卫无法执行" >&2
    exit 1
fi
if git -c safe.directory=/work -C /work check-ignore -q "$MANIFEST"; then
    echo "FAIL 守卫 E: 清单文件被 .gitignore 忽略：$MANIFEST" >&2
    exit 1
fi

# ── 2. 逐条处理 ───────────────────────────────────────────────────────
fail=0
total=0 ok=0 rec=0 skip=0 missing=0
c_corpus=0 c_fixture=0 c_hand=0
corpus_files=0 fixture_files=0
declare -a EXPECTED=()

while read -r kind input mode rest; do
    case "$kind" in \#*|"") continue ;; esac
    if [ -n "${rest:-}" ] || [ -z "$input" ] || [ -z "${mode:-}" ]; then
        echo "FAIL 清单行格式错误（应三列）：$kind ${input:-} ${mode:-} ${rest:-}" >&2
        fail=1
        continue
    fi
    case "$mode" in none|comments|directives) ;; *)
        echo "FAIL 清单行 mode 非法（$mode）：$kind $input" >&2
        fail=1
        continue
    ;; esac

    total=$((total + 1))
    label="$input [$mode]"
    g="$(golden_path "$kind" "$input" "$mode")"
    tag="$(expected_tag "$kind")"
    EXPECTED+=("$g")

    # 守卫 E：golden 路径不许被 .gitignore 静默排除（本地有、入库没有）
    if git -c safe.directory=/work -C /work check-ignore -q "$g"; then
        echo "  FAIL $label: golden 路径被 .gitignore 忽略（守卫 E，会静默不入库）：$g"
        fail=1
        continue
    fi

    case "$kind" in
        corpus)  c_corpus=$((c_corpus + 1))
                 [ "$mode" = none ] && corpus_files=$((corpus_files + 1)) ;;
        fixture) c_fixture=$((c_fixture + 1))
                 [ "$mode" = none ] && fixture_files=$((fixture_files + 1)) ;;
        hand-sparse) c_hand=$((c_hand + 1)) ;;
    esac

    if [ "$ACTION" = "record" ]; then
        # ── 录制：adapter 与 driver 必须先达成一致（格式守卫）──
        if [ "$kind" = "hand-sparse" ]; then
            echo "  skip $label —— 手写条目不重录（期望由 T-L17 裁定，本脚本只校验）"
            skip=$((skip + 1))
            continue
        fi
        want="$GATE_TMP/want" got="$GATE_TMP/got" d="$GATE_TMP/d"
        rc=0
        run_oracle "$input" "$mode" >"$want" || rc=$?
        if [ "$rc" -ne 0 ]; then
            echo "  FAIL $label: adapter 退出码 $rc（不落盘）"
            fail=1
            continue
        fi
        if [ "$(sed -n '2p' "$want")" != "# source: oracle" ]; then
            echo "  FAIL $label: adapter 第 2 行不是 '# source: oracle'（格式变了？不落盘）"
            fail=1
            continue
        fi
        rc=0
        run_driver "$input" "$mode" | normalize_source "$tag" >"$got" || rc=$?
        if [ "$rc" -ne 0 ]; then
            echo "  FAIL $label: driver 退出码 $rc（不落盘）"
            fail=1
            continue
        fi
        if diff -u "$want" "$got" >"$d" 2>&1; then
            mkdir -p "$(dirname "$g")"
            cp "$want" "$g"
            echo "  rec  $label -> $g"
            rec=$((rec + 1))
        else
            echo "  FAIL $label: adapter 与 driver 输出不一致（格式守卫，不落盘）"
            show_diff "$label" "$want" "$got"
            fail=1
        fi
        continue
    fi

    # ── 校验 ──
    if [ ! -f "$g" ]; then
        if [ "$missing" -lt 3 ]; then
            echo "  FAIL $label: golden 缺失（$g）"
        fi
        missing=$((missing + 1))
        fail=1
        continue
    fi
    gtag="$(sed -n '2s/^# source: //p' "$g")"
    if [ "$gtag" != "$tag" ]; then
        echo "  FAIL $label: golden 第 2 行来源标记是 '$gtag'，应为 '$tag'（守卫 A）"
        fail=1
        continue
    fi
    if [ "$kind" = "hand-sparse" ]; then
        create_sparse
    fi
    got="$GATE_TMP/got" d="$GATE_TMP/d"
    rc=0
    run_driver "$input" "$mode" | normalize_source "$tag" >"$got" || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "  FAIL $label: driver 退出码 $rc"
        show_diff "$label" "$g" "$got"
        fail=1
        continue
    fi
    if diff -u "$g" "$got" >"$d" 2>&1; then
        echo "  ok   $label"
        ok=$((ok + 1))
    else
        echo "  FAIL $label"
        show_diff "$label" "$g" "$got"
        fail=1
    fi
done < "$MANIFEST"

if [ "$total" -eq 0 ]; then
    echo "FAIL: 清单为空（$MANIFEST 没有任何条目）" >&2
    exit 1
fi
if [ "$missing" -gt 3 ]; then
    echo "  ……其余 $((missing - 3)) 条同样缺 golden"
fi
if [ "$missing" -eq "$total" ]; then
    echo "  （golden 尚未生成 —— 先跑：scripts/check-lex-golden.sh --record）"
fi

# ── 3. 守卫 B/C：语料与 fixture 不许静默缩水 ─────────────────────────
if [ ! -d go_source_code/test/syntax ]; then
    echo "FAIL 守卫: go_source_code/test/syntax 不存在（语料未就位？）"
    fail=1
else
    for f in go_source_code/test/syntax/*.go; do
        if ! awk -v p="corpus $f none" '$0 == p { hit = 1 } END { exit !hit }' "$MANIFEST"; then
            echo "FAIL 守卫 B: 语料 $f 未以 mode none 登记（新增语料必须进清单）"
            fail=1
        fi
    done
fi

if [ ! -d tools/token-oracle/fixtures ]; then
    echo "FAIL 守卫: tools/token-oracle/fixtures 不存在"
    fail=1
else
    while IFS= read -r f; do
        if ! awk -v p="fixture $f none" '$0 == p { hit = 1 } END { exit !hit }' "$MANIFEST"; then
            echo "FAIL 守卫 C: fixture $f 未以 mode none 登记（新增探针必须进清单）"
            fail=1
        fi
    done < <(find tools/token-oracle/fixtures -name '*.txt' \
                  ! -name char_corpus.txt ! -name char_golden.txt | sort)
fi

# ── 4. 守卫 D：不许有孤儿 golden ──────────────────────────────────────
# （record 与 check 同一判据：清单是唯一事实来源，目录里有清单外文件即红）
printf '%s\n' "${EXPECTED[@]}" | sort >"$GATE_TMP/expected"
find "$SMOKE_DIR" -name '*.golden' -type f | sort >"$GATE_TMP/actual"
orphans="$(comm -13 "$GATE_TMP/expected" "$GATE_TMP/actual")"
if [ -n "$orphans" ]; then
    echo "FAIL 守卫 D: 孤儿 golden（清单里没有这些文件 —— 删了清单行？）："
    echo "$orphans" | sed 's/^/    /'
    fail=1
fi

# ── 5. 汇总 ───────────────────────────────────────────────────────────
echo "──────────────────────────────────────────"
if [ "$ACTION" = "record" ]; then
    echo "录制：$rec 条落盘 / $skip 条跳过 / 共 $total 条"
else
    echo "校验：$ok/$total 条一致"
fi
echo "条目构成：corpus $c_corpus（$corpus_files 文件 × 3 modes）/ fixture $c_fixture（$fixture_files 文件 + mode 变体）/ hand $c_hand"
if [ "$fail" -eq 0 ]; then
    echo "守卫 A–E 全过；SM-M1-LEX-GOLDEN 通过"
else
    echo "有失败项（见上）—— SM-M1-LEX-GOLDEN 未通过"
fi
[ "$fail" -eq 0 ]
