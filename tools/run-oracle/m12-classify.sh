#!/bin/bash
# tools/run-oracle/m12-classify.sh —— M12 的语料分类装置（Task 0）。
#
# 判据（`M12-tests.md` `T-M12-01`）：`test/typeparam` 的 **266** 个 `.go` 被
# **七份清单**恰好划分，七份之和 == 266，且每份都由本脚本**算**出来（不手抄）。
#
# 用法：
#   m12-classify.sh [--out DIR] [--check] [--imports FILE]
#
#   --out DIR     清单落 DIR（缺省 `$ROOT/.superpowers/m12-lists`）
#   --check       只校验划分自洽（七份之和 == 266 且互不相交），不写文件
#   --imports F   只打印 F 的 import 路径（一行一个），用于人工核对
#
# 退出码：0 = 划分自洽；1 = 不自洽（会指名道姓说哪条不成立）。
#
# # 七份清单与它们各自的门禁
#
# | 清单 | 含义 | 门禁 |
# |---|---|---|
# | `typeparam-run.txt` | 无 stdlib import 的 `// run` | `G12-2` 三层一致 |
# | `typeparam-compile.txt` | 无 stdlib import 的 `// compile` | `G12-1` 编译通过 |
# | `typeparam-errorcheck.txt` | 无 stdlib import、无 `-m` 的 `// errorcheck` | `G12-3` 诊断码对齐 |
# | `typeparam-errorcheck-m.txt` | 带 `-m` 的 `// errorcheck` | 不做（`U-M12-08`：内联诊断）|
# | `typeparam-rejected.txt` | 带 stdlib import 的（任何族） | `G12-3` 干净拒绝 |
# | `typeparam-dir.txt` | `// rundir` / `// compiledir` 桩 | 不做（`U-M12-07`：相对包 import）|
# | `typeparam-none.txt` | 没有任何指令 | 不做 |
#
# # 三条「别用 grep 判 import」的理由（都是本脚本第一版翻车的地方）
#
# 1. **块注释里的 import 不算**：`issue50417.go` / `issue50417b.go` 的
#    `import "fmt"` 在 `/* … */` 里（Go 官方为了留档把整段注释掉了）。用
#    `grep '^import'` 会把这两个**今天就能跑通**的文件判成「需要 stdlib」。
# 2. **`unsafe` 是支持的伪 import**（M4 的 `U-M4-06` 白名单）：`issue53390.go`
#    只有 `import "unsafe"`，它属于门禁面，不属于拒绝面。
# 3. **`^import|"` 这种谓词更糟**：会把带**字符串字面量**的文件全误排除
#    （`M12-design.md` §1.2 记过：69 数成 37）。
#
# 所以这里用一个**能识别块注释与行注释**的小状态机取 import 路径，判据是
# 「import 集合 ⊆ {unsafe}」。
#
# # 为什么指令要匹配 `^// run( |$)` 而不是 `^// run$`
#
# 有 6 个文件带尾随参数：`// run -gcflags="-l"`、`// compile -p=p`、
# `// errorcheck -0 -m` 等。用精确匹配会把这 6 个漏成「无指令」——
# `M12-design.md` 的 `F1` 第一版就是这么错的（141/41/6/78 而不是 142/44/8/5）。
#
# # 纪律
#
#  * 语料目录**只读**；输出只落 `--out`。
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
CORPUS="${RGOC_TYPEPARAM_DIR:-$ROOT/go_source_code/test/typeparam}"
OUT="${RGOC_M12_LISTS:-$ROOT/.superpowers/m12-lists}"
CHECK_ONLY=0
IMPORTS_OF=""

while [ $# -gt 0 ]; do
    case "$1" in
        --out) OUT="$2"; shift 2 ;;
        --check) CHECK_ONLY=1; shift ;;
        --imports) IMPORTS_OF="$2"; shift 2 ;;
        *) echo "m12-classify: 未知参数 $1" >&2; exit 2 ;;
    esac
done

# 注释感知的 import 提取器：一行一个路径（去引号），保持出现顺序。
# 支持 `import "p"` / `import ( "p" ... )` / `别名 "p"` / `_ "p"` / `. "p"`。
imports_awk() {
    awk '
    BEGIN { inblock = 0; inimp = 0 }
    {
        line = $0; out = ""; i = 1; n = length(line)
        while (i <= n) {
            c = substr(line, i, 1)
            if (inblock) {
                if (c == "*" && substr(line, i + 1, 1) == "/") { inblock = 0; i += 2 }
                else i++
            } else if (c == "/" && substr(line, i + 1, 1) == "*") {
                inblock = 1; i += 2
            } else if (c == "/" && substr(line, i + 1, 1) == "/") {
                break
            } else {
                out = out c; i++
            }
        }
        if (inimp) {
            if (out ~ /^[ \t]*\)/) { inimp = 0; next }
            if (match(out, /"[^"]+"/)) print substr(out, RSTART + 1, RLENGTH - 2)
            next
        }
        if (out ~ /^import[ \t]+\(/) { inimp = 1; next }
        if (out ~ /^import[ \t]/) {
            if (match(out, /"[^"]+"/)) print substr(out, RSTART + 1, RLENGTH - 2)
        }
    }' "$1"
}

if [ -n "$IMPORTS_OF" ]; then
    imports_awk "$IMPORTS_OF"
    exit 0
fi

if [ ! -d "$CORPUS" ]; then
    echo "m12-classify: 语料目录不存在：$CORPUS" >&2
    exit 2
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

: >"$TMP/run"
: >"$TMP/compile"
: >"$TMP/errorcheck"
: >"$TMP/errorcheck-m"
: >"$TMP/rejected"
: >"$TMP/dir"
: >"$TMP/none"

total=0
for f in "$CORPUS"/*.go; do
    b="$(basename "$f")"
    total=$((total + 1))

    # ① 族：先判 dir 族，再判 run/compile（`^// run( |$)` 不会匹配 `// rundir`）
    fam=""
    if grep -qE '^// rundir( |$)' "$f"; then
        fam=dir
    elif grep -qE '^// compiledir( |$)' "$f"; then
        fam=dir
    elif grep -qE '^// run( |$)' "$f"; then
        fam=run
    elif grep -qE '^// compile( |$)' "$f"; then
        fam=compile
    elif grep -qE '^// errorcheck( |$)' "$f"; then
        fam=errorcheck
    else
        fam=none
    fi

    # ② `-m` 是「内联诊断」族：期望错是 gc 的内联报告，不是语言错误（`U-M12-08`）
    if [ "$fam" = "errorcheck" ] && grep -qE '^// errorcheck .*-m' "$f"; then
        echo "$b" >>"$TMP/errorcheck-m"
        continue
    fi

    # ③ import 面：只认真的 import 声明（忽略注释），`unsafe` 不算 stdlib
    if [ "$fam" = "dir" ] || [ "$fam" = "none" ]; then
        echo "$b" >>"$TMP/$fam"
        continue
    fi
    stdlib=""
    while IFS= read -r p; do
        [ -n "$p" ] || continue
        [ "$p" = "unsafe" ] && continue
        stdlib="$stdlib $p"
    done <<EOF
$(imports_awk "$f")
EOF
    if [ -n "$stdlib" ]; then
        echo "$b" >>"$TMP/rejected"
    else
        echo "$b" >>"$TMP/$fam"
    fi
done

n_run=$(wc -l <"$TMP/run" | tr -d ' ')
n_compile=$(wc -l <"$TMP/compile" | tr -d ' ')
n_errorcheck=$(wc -l <"$TMP/errorcheck" | tr -d ' ')
n_err_m=$(wc -l <"$TMP/errorcheck-m" | tr -d ' ')
n_rejected=$(wc -l <"$TMP/rejected" | tr -d ' ')
n_dir=$(wc -l <"$TMP/dir" | tr -d ' ')
n_none=$(wc -l <"$TMP/none" | tr -d ' ')
sum=$((n_run + n_compile + n_errorcheck + n_err_m + n_rejected + n_dir + n_none))

echo "typeparam 语料：$CORPUS"
echo "  run           $n_run   （门禁 G12-2，三层一致）"
echo "  compile       $n_compile   （门禁 G12-1，编译通过）"
echo "  errorcheck    $n_errorcheck    （门禁 G12-3，诊断码对齐）"
echo "  errorcheck-m  $n_err_m    （不做：U-M12-08，内联诊断）"
echo "  rejected      $n_rejected   （门禁 G12-3，干净拒绝；带 stdlib import）"
echo "  dir           $n_dir   （不做：U-M12-07，相对包 import）"
echo "  none          $n_none     （不做：没有指令）"
echo "  ──────────────────"
echo "  sum           $sum / total $total"

rc=0
if [ "$sum" -ne "$total" ]; then
    echo "✗ 七份之和 $sum != 文件总数 $total（有文件被漏掉或重复归类）" >&2
    rc=1
fi
if [ "$total" -ne 266 ]; then
    echo "✗ 语料总数 $total != 266（语料变了 ⇒ F1 要重测，不是脚本坏了）" >&2
    rc=1
fi

dup=$(cat "$TMP"/run "$TMP"/compile "$TMP"/errorcheck "$TMP"/errorcheck-m \
    "$TMP"/rejected "$TMP"/dir "$TMP"/none | sort | uniq -d | wc -l | tr -d ' ')
if [ "$dup" -ne 0 ]; then
    echo "✗ 有 $dup 个文件同时进了两份清单" >&2
    rc=1
fi

if [ "$CHECK_ONLY" -eq 0 ]; then
    mkdir -p "$OUT"
    for k in run compile errorcheck errorcheck-m rejected dir none; do
        cp "$TMP/$k" "$OUT/typeparam-$k.txt"
    done
    echo "清单已写入 $OUT"
fi

if [ "$rc" -eq 0 ]; then
    echo "✓ 划分自洽"
fi
exit "$rc"
