#!/bin/bash
# tools/run-oracle/m12-baseline.sh —— M12 的基线测绘装置（Task 0）。
#
# 输出一张**逐文件**的表（TSV）与一段汇总，用来回答「今天到哪了」和
# 「每个失败归因到哪一类」。`M12-plan.md` 的任务顺序由这张表决定（`D-M12-10`）。
#
# 用法：
#   m12-baseline.sh [--out DIR] [--only run|compile|errorcheck|errorcheck-m|rejected|dir|none]
#
# 输出列（TSV，第一行是表头）：
#   family  file  check_rc  check_codes  build_rc  build_kind  run_rc
#
#   check_codes  该文件**所有**诊断码去重后用 `,` 连（空 = 没有诊断）
#   build_kind   check 过之后 build 失败时，失败形态的分类（见下）
#   run_rc       只有 `run` 族且 build 成功才有值，否则 `-`
#
# `build_kind` 的取值（从 stderr 首行归类，**只分类不比对文本**）：
#   ok            build 成功
#   layout        `帧布局失败：…没有运行期布局`
#   verify        `VerifyError`
#   selector      `CHK-INVALID-SELECTOR`
#   missing_type  `HIR-MISSING-TYPE`
#   other         其它（原文进 build_msg 列，便于人工看）
#
# # 为什么 build 的退出码不能过管道拿
#
# `cmd | head -5; echo $?` 拿到的是 `head` 的退出码 —— 恒 0。本脚本先把 stderr
# 落到文件、再读文件，`$?` 取自 `cmd` 本身。（`M11` 踩过「`| tail` 当全绿」。）
#
# # 为什么不用 `set -e`
#
# 每个文件都**预期**可能失败；`set -e` 会在第一个失败处退出，把「基线」变成
# 「第一个失败」。
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
DRIVER="${RGOC_DRIVER:-$ROOT/rgoc/target/debug/rgoc-driver}"
LISTS="${RGOC_M12_LISTS:-$ROOT/.superpowers/m12-lists}"
OUT="${RGOC_M12_BASE:-$ROOT/.superpowers/m12-baseline}"
ONLY=""

while [ $# -gt 0 ]; do
    case "$1" in
        --out) OUT="$2"; shift 2 ;;
        --only) ONLY="$2"; shift 2 ;;
        *) echo "m12-baseline: 未知参数 $1" >&2; exit 2 ;;
    esac
done

if [ ! -x "$DRIVER" ]; then
    echo "m12-baseline: 找不到 driver：$DRIVER（先 cargo build）" >&2
    exit 2
fi
if [ ! -f "$LISTS/typeparam-run.txt" ]; then
    echo "m12-baseline: 清单不存在（先跑 m12-classify.sh）" >&2
    exit 2
fi

CORPUS="${RGOC_TYPEPARAM_DIR:-$ROOT/go_source_code/test/typeparam}"
mkdir -p "$OUT/bin" "$OUT/log"
TSV="$OUT/baseline.tsv"
: >"$TSV"
printf 'family\tfile\tcheck_rc\tcheck_codes\tbuild_rc\tbuild_kind\trun_rc\n' >>"$TSV"

# 归类 build 的失败形态（只看 stderr 首行，不比对完整文本）
build_kind() {
    local log="$1"
    if grep -q '帧布局失败' "$log" 2>/dev/null; then
        echo layout
    elif grep -q 'VerifyError' "$log" 2>/dev/null; then
        echo verify
    elif grep -q 'CHK-INVALID-SELECTOR' "$log" 2>/dev/null; then
        echo selector
    elif grep -q 'HIR-MISSING-TYPE' "$log" 2>/dev/null; then
        echo missing_type
    elif grep -q 'panic' "$log" 2>/dev/null; then
        echo panic
    else
        echo other
    fi
}

want() { [ -z "$ONLY" ] || [ "$ONLY" = "$1" ]; }

for fam in run compile errorcheck errorcheck-m rejected dir none; do
    want "$fam" || continue
    list="$LISTS/typeparam-$fam.txt"
    [ -f "$list" ] || continue
    while IFS= read -r b; do
        [ -n "$b" ] || continue
        f="$CORPUS/$b"
        ck="$OUT/log/$fam.$b.chk"
        "$DRIVER" check "$f" >"$ck.out" 2>"$ck.err"
        crc=$?
        codes="$(grep -ohE '\[[A-Z][A-Z0-9-]+\]' "$ck.out" "$ck.err" 2>/dev/null |
            tr -d '[]' | sort -u | paste -sd, -)"

        brc="-"
        bkind="-"
        rrc="-"
        if [ "$crc" -eq 0 ]; then
            bl="$OUT/log/$fam.$b.bld"
            bin="$OUT/bin/$fam.$b.bin"
            "$DRIVER" build --emit=elf -o "$bin" "$f" >"$bl.out" 2>"$bl.err"
            brc=$?
            if [ "$brc" -eq 0 ]; then
                bkind=ok
                # 只有 run 族才真的跑（compile 族「跑起来」不是它的判据）
                if [ "$fam" = "run" ]; then
                    timeout 10 "$bin" >"$OUT/log/$fam.$b.run.out" 2>"$OUT/log/$fam.$b.run.err"
                    rrc=$?
                fi
            else
                bkind="$(build_kind "$bl.err")"
            fi
        fi
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$fam" "$b" "$crc" "$codes" "$brc" "$bkind" "$rrc" >>"$TSV"
    done <"$list"
done

# ── 汇总 ────────────────────────────────────────────────────────────────
echo "基线表：$TSV"
echo
awk -F'\t' '
NR == 1 { next }
{
    fam = $1
    tot[fam]++
    if ($3 == 0) cok[fam]++
    if ($5 == 0) bok[fam]++
    if ($7 == 0) rok[fam]++
    if ($3 != 0) {
        n = split($4, cs, ",")
        for (i = 1; i <= n; i++) if (cs[i] != "") code[cs[i]]++
    }
    if ($6 != "-" && $6 != "ok") kind[$6]++
}
END {
    printf "%-11s %5s %7s %7s %7s\n", "family", "total", "check", "build", "run"
    split("run compile errorcheck errorcheck-m rejected dir none", order, " ")
    for (i = 1; i <= 7; i++) {
        f = order[i]
        if (f in tot) printf "%-11s %5d %7d %7d %7d\n", f, tot[f], cok[f], bok[f], rok[f]
    }
    printf "\ncheck 失败诊断码分布（按出现次数）：\n"
    for (c in code) printf "  %-28s %d\n", c, code[c]
    printf "\nbuild 失败形态（check 过之后）：\n"
    for (k in kind) printf "  %-14s %d\n", k, kind[k]
}' "$TSV"
