#!/usr/bin/env bash
# 腿乙门禁（testdata 正/负例）—— T-A-95（M2-tests.md §4.2；M2-design.md §8.1）
#
# 判据（脚本本身就是测试）：
#   ① 31 个 testdata 副本与官方语料逐字节一致（同 check-ast-golden.sh 守卫①）；
#   ② 双向 ERROR 匹配的**覆盖面守卫**：
#      A 带声明错误的文件数 == 25（6 个正例零错 —— 语料不许静默缩水）；
#      B test_oracle.rs 里 errcheck 与 collide 两组测试各 31 条（两组不许失衡）；
#   ③ cargo test -p rgoc-parse --test test_oracle -- --test-threads=1（errcheck 组）
#
# 用法：
#   scripts/check-ast-testdata.sh          校验（M2 门禁 A2 的 testdata 部分）
#
# ── 设计 ──────────────────────────────────────────────────────────────
# 双向匹配本体是官方 error_test.go 的直译，住在 test_oracle.rs 的
# `errorcheck()`（F11 子串 / F12 行注释=行号、块注释=下一个词元的行+列）；
# 本脚本只做「门禁编排 + 反盲区」，不重复实现匹配逻辑。

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
TEST_RS="rgoc/crates/rgoc-parse/tests/test_oracle.rs"

fail=0
note() { printf '  %s\n' "$*"; }
die()  { printf '  !! %s\n' "$*"; fail=1; }

# ── 守卫 ①：语料副本逐字节一致 ────────────────────────────────────────
echo "== 守卫 ①：语料副本逐字节守卫 =="
n=0
for src in "$CORPUS"/*.go; do
    b=$(basename "$src")
    [ -f "$AST_DIR/$b" ] || { die "缺副本：$AST_DIR/$b"; continue; }
    cmp -s "$src" "$AST_DIR/$b" || die "副本与语料不一致：$b"
    n=$((n+1))
done
[ "$n" -eq 31 ] || die "语料文件数应为 31，实得 $n"

# ── 守卫 ②：双向匹配的覆盖面 ──────────────────────────────────────────
echo "== 守卫 ②：覆盖面守卫 =="
# A 官方语料里带 // ERROR 声明的文件数（errorcheck 的分母不许缩水）
# ⚠️ ERROR 注释是**行内**的（前有代码），不能锚定行首。
nerr=0
for src in "$CORPUS"/*.go; do
    if grep -qE '(// ERROR |/\* ERROR )' "$src"; then
        nerr=$((nerr+1))
    fi
done
note "带 ERROR 声明的语料文件数：$nerr"
[ "$nerr" -eq 25 ] || die "带 ERROR 声明的文件应为 25，实得 $nerr"

# B test_oracle.rs 两组测试各 31 条
ncoll=$(grep -c '    t_collide_' "$TEST_RS" || true)
nerrc=$(grep -c '    t_errcheck_' "$TEST_RS" || true)
note "test_oracle.rs：collide $ncoll 条 / errcheck $nerrc 条"
[ "$ncoll" -eq 31 ] || die "collide 测试应为 31 条，实得 $ncoll"
[ "$nerrc" -eq 31 ] || die "errcheck 测试应为 31 条，实得 $nerrc"

# ── 守卫 ③：跑 errcheck（单线程：失败信息按文件聚拢，易读）────────────
echo "== 守卫 ③：cargo 双向 ERROR 匹配（T-A-95）=="
touch rgoc/crates/rgoc-parse/tests/test_oracle.rs
# cargo 必须在 workspace 根（/work 没有 Cargo.toml）；先捕获再过滤，
# 退出码取 cargo 的，不取 sed 的。
out="$( cd rgoc && cargo test -p rgoc-parse --test test_oracle -- --test-threads=1 2>&1 )"
rc=$?
printf '%s\n' "$out" | sed -n '/test result:/p;/^test t_errcheck .* FAILED/p'
[ "$rc" -eq 0 ] || die "cargo test exit $rc"

if [ "$fail" -ne 0 ]; then
    echo "check-ast-testdata: FAIL"
    exit 1
fi
echo "check-ast-testdata: OK"
