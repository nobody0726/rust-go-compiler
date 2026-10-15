#!/usr/bin/env bash
# 标准库冒烟门禁 —— T-A-94（M2-tests.md §4.3；官方 parser_test.go 的 TestStdLib/walkDirs）
#
# 判据（脚本本身就是测试）：
#   默认  ① 分母守卫：语料 `src/` 下（跳过任何名为 `testdata` / `test` 的目录，
#            官方 walkDirs 同款规则）的 .go 文件数 > 6000（go1.27.1 实测 6670）；
#         ② cargo test -p rgoc-parse --test test_smoke（含 T-A-94 全量 +
#            T-A-96 深度上限 + T-A-98 确定性 + T-A-99 Eof）。
#   全过 -> exit 0；否则 exit 1
#
# 「已知 unsupported 项」目前为空：6670 个文件首轮即零错误（T-AP20 侦察轮实测），
# 出现新失败时应先修 bug 而不是加白名单。
#
# ── 设计 ──────────────────────────────────────────────────────────────
# 构建与运行**全部在容器内**（04 §7）；宿主侧只做路径计算与转派
# （与 check-ast-golden.sh 同形，bash 3.2 兼容）。

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

fail=0
note() { printf '  %s\n' "$*"; }
die()  { printf '  !! %s\n' "$*"; fail=1; }

# ── 守卫 ①：分母（官方 walkDirs 口径的 .go 文件数）─────────────────────
echo "== 守卫 ①：语料分母（walkDirs 口径）=="
n="$(find go_source_code/src -name '*.go' ! -path '*/testdata/*' ! -path '*/test/*' | wc -l | tr -d ' ')"
note "src/ 下 .go（跳过 testdata/test）：$n"
[ "$n" -gt 6000 ] || die "分母异常：$n ≤ 6000（语料缺失或 walkDirs 口径变了）"

# ── 守卫 ②：cargo 冒烟（T-A-94/96/98/99）──────────────────────────────
echo "== 守卫 ②：cargo test_smoke（T-A-94/96/98/99）=="
touch rgoc/crates/rgoc-parse/tests/test_smoke.rs
out="$( cd rgoc && cargo test -p rgoc-parse --test test_smoke --no-fail-fast 2>&1 )"
rc=$?
printf '%s\n' "$out" | sed -n '/test result:/p;/^test .* FAILED/p'
[ "$rc" -eq 0 ] || die "cargo test exit $rc"

if [ "$fail" -ne 0 ]; then
    echo "check-ast-stdlib: FAIL"
    exit 1
fi
echo "check-ast-stdlib: OK"
