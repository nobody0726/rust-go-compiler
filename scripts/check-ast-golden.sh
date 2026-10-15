#!/usr/bin/env bash
# 腿丙门禁（oracle 对撞）—— T-A-92/T-A-93（M2-tests.md §5.2/§5.3；M2-design.md §8.2）
#
# 判据（脚本本身就是测试）：
#   --record  用 ast-oracle 重生成 31 份 golden dump（+ 诊断清单）到 rgoc/tests/ast/
#   默认      ① 语料副本逐字节守卫（31 个 testdata 与官方语料 cmp 全过）；
#             ② golden 齐性守卫（无孤儿、无缺失、非空、以 File 起头）；
#             ③ cargo test -p rgoc-parse --test test_oracle（rgoc dump vs golden 逐行 diff）
#   判据      全过 -> exit 0；否则打印差异 -> exit 1
#
# 用法：
#   scripts/check-ast-golden.sh            校验（M2 门禁 A4）
#   scripts/check-ast-golden.sh --record   重新录制（官方行为升级/格式变更后才跑）
#
# ── 设计 ──────────────────────────────────────────────────────────────
# ① golden 由 oracle 生成、**不得手改**（M2-tests.md §5.4：`# source: oracle`）。
# ② testdata 副本（rgoc/tests/ast/*.go）与语料逐字节一致由 cmp 守卫 ——
#    对撞的输入漂移会让 golden 与测试用例「各改各的」。
# ③ 构建与比对**全部在容器内**（04 §7）；宿主侧只做路径计算与转派
#    （与 check-lex-golden.sh 同形，bash 3.2 兼容）。

# ═══ 宿主侧：计算路径并转派进容器 ═══════════════════════════════════════
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

CORPUS="go_source_code/src/cmd/compile/internal/syntax/testdata"
AST_DIR="rgoc/tests/ast"
GOLDEN_DIR="$AST_DIR/golden"
DIAGS_DIR="$AST_DIR/diags"
ORACLE_DIR="tools/ast-oracle"

ACTION="check"
case "${1:-}" in
    "")       ACTION="check" ;;
    --record) ACTION="record" ;;
    *)        echo "用法：scripts/check-ast-golden.sh [--record]" >&2
              echo "  （无参数 = 校验；--record = 用 ast-oracle 重录 golden）" >&2
              exit 2 ;;
esac

fail=0
note() { printf '  %s\n' "$*"; }
die()  { printf '  !! %s\n' "$*"; fail=1; }

# ── 守卫 ①：testdata 副本与语料逐字节一致，不许缺、不许漂移 ────────────
echo "== 守卫 ①：语料副本逐字节守卫 =="
n=0
for src in "$CORPUS"/*.go; do
    b=$(basename "$src")
    [ -f "$AST_DIR/$b" ] || { die "缺副本：$AST_DIR/$b"; continue; }
    cmp -s "$src" "$AST_DIR/$b" || die "副本与语料不一致：$b"
    n=$((n+1))
done
note "语料文件数：$n"
[ "$n" -eq 31 ] || die "语料文件数应为 31，实得 $n（语料升级要改本脚本与 test_oracle.rs）"

# 反盲区：不许有孤儿副本（目录里有、语料里没有）
for f in "$AST_DIR"/*.go; do
    b=$(basename "$f")
    [ -f "$CORPUS/$b" ] || die "孤儿副本：$b（语料里没有）"
done

if [ "$ACTION" = "record" ]; then
    echo "== --record：ast-oracle 重录 golden =="
    mkdir -p "$GOLDEN_DIR" "$DIAGS_DIR"
    for src in "$CORPUS"/*.go; do
        b=$(basename "$src")
        ( cd "$ORACLE_DIR" && go run . "/work/$src" \
            > "/work/$GOLDEN_DIR/$b.dump" 2> "/work/$DIAGS_DIR/$b.err" )
        rc=$?
        if [ $rc -ne 0 ] && [ $rc -ne 1 ]; then
            die "oracle exit $rc（>1 是致命错）：$b"
        fi
        head -1 "$GOLDEN_DIR/$b.dump" | grep -q '^File$' || die "golden 首行不是 File：$b"
    done
    note "重录完成（exit 0 = 无语法错 / 1 = 有语法错但 dump 照常）"
fi

# ── 守卫 ②：golden 齐性（无孤儿、无缺失、非空）────────────────────────
echo "== 守卫 ②：golden 齐性守卫 =="
ngold=0
for f in "$GOLDEN_DIR"/*.dump; do
    [ -f "$f" ] || { die "没有任何 golden：$GOLDEN_DIR/"; break; }
    b=$(basename "$f" .dump)
    [ -f "$AST_DIR/$b" ] || die "孤儿 golden：$b.dump（无对应副本）"
    [ -s "$f" ] || die "空 golden：$b.dump"
    head -1 "$f" | grep -q '^File$' || die "golden 首行不是 File：$b.dump"
    ngold=$((ngold+1))
done
note "golden 数：$ngold"
[ "$ngold" -eq 31 ] || die "golden 应为 31 份，实得 $ngold"

# ── 守卫 ③：rgoc 侧重算（cargo 对撞）─────────────────────────────────
echo "== 守卫 ③：cargo 对撞（T-A-92/93）=="
touch rgoc/crates/rgoc-parse/tests/test_oracle.rs
# cargo 必须在 workspace 根（/work 没有 Cargo.toml）；先捕获再过滤，
# 退出码取 cargo 的，不取 sed 的。
out="$( cd rgoc && cargo test -p rgoc-parse --test test_oracle --no-fail-fast 2>&1 )"
rc=$?
printf '%s\n' "$out" | sed -n '/test result:/p;/^test .* FAILED/p'
[ "$rc" -eq 0 ] || die "cargo test exit $rc"

if [ "$fail" -ne 0 ]; then
    echo "check-ast-golden: FAIL"
    exit 1
fi
echo "check-ast-golden: OK"
