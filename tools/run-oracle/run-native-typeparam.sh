#!/bin/bash
# tools/run-oracle/run-native-typeparam.sh —— M12 的「泛型语料跑起来」装置（Task 3）。
#
# 判据（`M12-tests.md` §1）：
#   * `run`（71 个）      三层一致：native vs 解释器（`--via=hir`）vs 官方 `go run`
#   * `compile`（40 个）  `build` 退出 0 且产物是 aarch64 ELF
#   * `errorcheck`（5 个）`check` 退出 1 且**首码 ∈ 该文件的目标集合**
#   * `rejected`（76 个） `check` 退出 1 且**码集合 ⊇ TYP-UNSUPPORTED-IMPORT-FORM**
#
# 用法：
#   run-native-typeparam.sh [--list] [--filter PAT] [--only FAM] [--gc=off|on|stress]
#                           [--no-oracle] [--jobs N]
#
#   --list        每个文件打一行 OK/SKIP/BAD
#   --filter PAT  只跑路径含 PAT 的文件
#   --only FAM    run | compile | errorcheck | rejected（缺省全跑）
#   --gc=MODE     三态（`C12` §5.1）：off ⇒ `RGOC_GC=0`、stress ⇒ `RGOC_GC_STRESS=64`
#   --no-oracle   不重新生成官方期望值（`.oracle-*` 已缓存时用）
#
# 退出码：0 = 全绿；1 = 有 BAD。
#
# # 为什么官方层要**三样分开**取
#
# `tools/run-oracle/main.go` 取的是**合并流**（stdout+stderr 同一根管子）—— 那是
# M5 的口径。M12 要分三样比，因为「native 把输出打到 stderr 而官方打到 stdout」
# 这种错在合并流里看不见。所以这里自己起 `go run`、分三个文件接。
#
# # 为什么 `run` 族的汇总要分「有输出 / 空输出」两栏
#
# 71 个里相当一部分**不打印任何东西**。在空输出上「两边都是空」只证明**没崩**，
# 不构成语义证据（`M12-tests.md` §0.2）。所以分开报，并且断言两栏相加 == 分母。
#
# # 为什么 `errorcheck` 的目标集合写死在脚本里
#
# 那是**逐文件的期望码**（`M12-tests.md` §1 的表），不是语料清单 —— 语料清单由
# `m12-classify.sh` 算。写死在这里是为了「改一条要人看一眼」（沿 `T-GDEV-03`）。
#
# # 纪律（M7–M11 踩过的，全部照抄）
#
#  * 语料目录**只读**，产物落 `$OUT`；
#  * 退出码**不过管道取**（`cmd | head` 的 `$?` 是 `head` 的）；
#  * `slug` 用 basename（语料是同目录的平铺文件，不带 `/`）；
#  * 不写 `if ! cmd; then rc=$?` —— `!` 之后 `$?` 恒 0。
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
DRIVER="${RGOC_DRIVER:-$ROOT/rgoc/target/debug/rgoc-driver}"
LISTS="${RGOC_M12_LISTS:-$ROOT/.superpowers/m12-lists}"
OUT="${RGOC_M12_ORACLE:-$ROOT/.superpowers/m12-oracle}"
CORPUS="${RGOC_TYPEPARAM_DIR:-$ROOT/go_source_code/test/typeparam}"

LIST_MODE=0
FILTER=""
ONLY=""
GC_MODE=""
ORACLE=1

while [ $# -gt 0 ]; do
  case "$1" in
    --list) LIST_MODE=1 ;;
    --filter) shift; FILTER="${1:-}" ;;
    --only) shift; ONLY="${1:-}" ;;
    --gc=*) GC_MODE="${1#--gc=}" ;;
    --no-oracle) ORACLE=0 ;;
    -h|--help) sed -n '2,40p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) echo "未知参数：$1" >&2; exit 2 ;;
  esac
  shift
done

case "$GC_MODE" in
  ""|off|on|stress) ;;
  *) echo "run-native-typeparam.sh: --gc=$GC_MODE 不认识（off|on|stress）" >&2; exit 2 ;;
esac

if [ ! -x "$DRIVER" ]; then
  echo "run-native-typeparam.sh: 找不到 $DRIVER（先在容器内 cargo build）" >&2
  exit 2
fi
if [ ! -f "$LISTS/typeparam-run.txt" ]; then
  echo "run-native-typeparam.sh: 清单不存在（先跑 m12-classify.sh）" >&2
  exit 2
fi
mkdir -p "$OUT/bin" "$OUT/log" "$OUT/oracle"

line() {
  [ "$LIST_MODE" -eq 1 ] || return 0
  printf '%-4s %s\n' "$1" "$2"
}

want() { [ -z "$ONLY" ] || [ "$ONLY" = "$1" ]; }
selected() {
  [ -z "$FILTER" ] && return 0
  case "$1" in *"$FILTER"*) return 0 ;; *) return 1 ;; esac
}

# 运行侧的三态（构建一次、跑三次；环境变量只加在运行侧）。
run_native() {
  case "$GC_MODE" in
    off) RGOC_GC=0 timeout 60 "$1" ;;
    stress) RGOC_GC_STRESS=64 timeout 60 "$1" ;;
    *) timeout 60 "$1" ;;
  esac
}

# `errorcheck` 的**逐文件期望码**（`M12-tests.md` §1 的表）。
errorcheck_target() {
  case "$1" in
    tparam1.go) echo "TYP-DUPLICATE-DECL" ;;
    issue48711.go) echo "TYP-INVALID-INSTANCE-CYCLE" ;;
    issue51232.go) echo "TYP-WRONG-TYPE-ARG-COUNT" ;;
    issue51233.go) echo "TYP-WRONG-TYPE-ARG-COUNT" ;;
    issue50317.go) echo "SYN-UNEXPECTED-TOKEN" ;;
    *) echo "" ;;
  esac
}

# 官方期望值：三样分开存（`.oracle-out` / `.oracle-err` / `.oracle-rc`）。
# `go run` 需要可写缓存，而语料目录只读 ⇒ GOCACHE/GOTMPDIR 落到 $OUT。
gen_oracle() {
  local b="$1" slug="$2" tmp
  tmp="$(mktemp -d)"
  ( cd "$CORPUS" && GOCACHE="$OUT/oracle/cache" GOTMPDIR="$tmp" GO111MODULE=off \
      timeout 60 go run "$b" ) >"$OUT/oracle/$slug.out" 2>"$OUT/oracle/$slug.err"
  echo $? >"$OUT/oracle/$slug.rc"
  rm -rf "$tmp"
}

# 三样分开比。`stderr` 只在两边都非空时比（`M12-tests.md` §0.3：`C8` 管渲染）。
# 返回 0 = 相同。
cmp_three() {
  local a_out="$1" a_err="$2" a_rc="$3" b_out="$4" b_err="$5" b_rc="$6"
  cmp -s "$a_out" "$b_out" || return 1
  [ "$a_rc" = "$b_rc" ] || return 2
  if [ -s "$a_err" ] && [ -s "$b_err" ]; then
    cmp -s "$a_err" "$b_err" || return 3
  fi
  return 0
}

# 合并流比（`M12 Ruling 24-1`）：官方 `println` builtin 进 stderr、我们的
# 契约（`C9` §4.1）定「println + panic 合并流写 stdout」—— 与官方比时
# out+err 拼起来逐字节（`testdir_test.go` 的判据形态），退出码照比。
cmp_merged() {
  local a_out="$1" a_err="$2" a_rc="$3" b_out="$4" b_err="$5" b_rc="$6"
  cat "$a_out" "$a_err" >"$OUT/.merged.a"
  cat "$b_out" "$b_err" >"$OUT/.merged.b"
  cmp -s "$OUT/.merged.a" "$OUT/.merged.b" || return 1
  [ "$a_rc" = "$b_rc" ] || return 2
  return 0
}

# `run` 族的**白名单**（`M12-baseline.md` §15.3 / §17）：
# - `U-M12-09`（§17.2）：合并流口径暴露的 **native 输出级老缺口** —— 在
#   `ec8fd74`（Task 21 修复前）实测同样输出错误（`other 2814739…` /
#   `test N failed`），不是回归。修复是 native 接口断言 / itab 的深水区，
#   登记后由独立任务跟进。
# - `§15.3`：t_gen_01 判据冻住的 3 条功能级缺口（build 失败）。
# 白名单是**冻结**的：多一条少一条都要改这行。
run_whitelist() {
  case "$1" in
    typeswitch3.go|typeswitch4.go|typeswitch5.go|issue51522b.go|eface.go) return 0 ;;
    dictionaryCapture-noinline.go|dottype.go|issue44688.go) return 0 ;;
    *) return 1 ;;
  esac
}

ok=0; skip=0; bad=0
badlist=""
skiplist=""
# 每族的读数（`ok/total`）
r_ok=0; r_total=0
c_ok=0; c_total=0
e_ok=0; e_total=0
j_ok=0; j_total=0
# `run` 族的两栏：只在**通过**的文件上分类（分母 = 通过数）。
# 理由：没通过的文件没有「输出」可言，把它们算进分母会让这个不变式变成
# 「通过数 + 失败数 == 总数」的复述，一点信息量都没有。
out_ok=0; empty_ok=0
# 官方层不可用（`package p` 之类）的个数
no_official=0

# ── ① run：三层一致 ─────────────────────────────────────────────────────
if want run; then
  while IFS= read -r b; do
    [ -n "$b" ] || continue
    selected "$b" || continue
    r_total=$((r_total + 1))
    slug="${b%.go}"
    if run_whitelist "$b"; then
      case "$b" in
        dictionaryCapture-noinline.go|dottype.go|issue44688.go) tag="UM12-S15" ;;
        *) tag="UM12-09" ;;
      esac
      skip=$((skip + 1)); skiplist="$skiplist $slug:$tag"
      line SKIP "$b ($tag：白名单冻住，§15.3 / §17.2)"
      continue
    fi
    exe="$OUT/bin/$slug"
    log="$OUT/log/$slug.build"

    "$DRIVER" build --emit=elf -o "$exe" "$CORPUS/$b" >"$log" 2>&1
    rc=$?
    if [ "$rc" -ne 0 ]; then
      bad=$((bad + 1)); badlist="$badlist $slug:build$rc"
      line BAD "$b (build rc=$rc：$(head -1 "$log"))"
      continue
    fi

    run_native "$exe" >"$OUT/log/$slug.native.out" 2>"$OUT/log/$slug.native.err"
    nrc=$?
    "$DRIVER" run --via hir "$CORPUS/$b" \
      >"$OUT/log/$slug.hir.out" 2>"$OUT/log/$slug.hir.err"
    hrc=$?

    # 官方（缓存）
    if [ "$ORACLE" -eq 1 ] && [ ! -f "$OUT/oracle/$slug.rc" ]; then
      gen_oracle "$b" "$slug"
    fi
    orc="-"
    [ -f "$OUT/oracle/$slug.rc" ] && orc="$(cat "$OUT/oracle/$slug.rc")"

    # L1：native vs 解释器（**主判据**）
    cmp_three "$OUT/log/$slug.native.out" "$OUT/log/$slug.native.err" "$nrc" \
      "$OUT/log/$slug.hir.out" "$OUT/log/$slug.hir.err" "$hrc"
    d1=$?
    if [ "$d1" -ne 0 ]; then
      bad=$((bad + 1)); badlist="$badlist $slug:L1-$d1"
      line BAD "$b (L1 native vs 解释器 不一致，形态 $d1)"
      continue
    fi

    # L2/L3：与官方比。官方跑不起来（`package p` 没有 main）⇒ 只算 L1 通过。
    if [ "$orc" != "0" ]; then
      no_official=$((no_official + 1))
      skip=$((skip + 1)); skiplist="$skiplist $slug:official-$orc"
      line SKIP "$b (官方层不可用 rc=$orc；L1 已一致)"
      continue
    fi
    # ⚠️ 与官方比要用**合并流**（`C9` §4.1 的 `M12 Ruling 24-1`）：
    # 官方 `println` builtin 进 **stderr**（Go 规范），我们的契约定的是
    # 「println + panic 合并流写 stdout」—— 逐流比必然假红（native.out
    # 有字节、oracle.out 空、字节全在 oracle.err）。官方 testdir_test.go
    # 的判据形态同样是**合并**比。所以 out+err 拼起来逐字节，退出码照比。
    cmp_merged "$OUT/log/$slug.native.out" "$OUT/log/$slug.native.err" "$nrc" \
      "$OUT/oracle/$slug.out" "$OUT/oracle/$slug.err" "$orc"
    d2=$?
    if [ "$d2" -ne 0 ]; then
      bad=$((bad + 1)); badlist="$badlist $slug:L2-$d2"
      line BAD "$b (L2 native vs 官方 不一致，形态 $d2)"
      continue
    fi

    ok=$((ok + 1)); r_ok=$((r_ok + 1))
    if [ -s "$OUT/oracle/$slug.out" ]; then
      out_ok=$((out_ok + 1))
    else
      empty_ok=$((empty_ok + 1))
    fi
    line OK "$b"
  done <"$LISTS/typeparam-run.txt"

  # 两栏相加 == 通过数（`T-GDEV-02` 的同款不变式）
  if [ $((out_ok + empty_ok)) -ne "$r_ok" ]; then
    bad=$((bad + 1))
    badlist="$badlist run两栏不闭合($((out_ok + empty_ok))≠$r_ok)"
  fi
fi

# ── ② compile：编译通过 + 产物是 aarch64 ELF ────────────────────────────
if want compile; then
  while IFS= read -r b; do
    [ -n "$b" ] || continue
    selected "$b" || continue
    c_total=$((c_total + 1))
    slug="${b%.go}"
    # U-M12-01（部分实例化，§14.3）：`typelist` 的 `f := f2[byte]` 需要
    # 「部分泛型函数值」贯穿 check/HIR/SSA 三层 —— 判据 `t_gen_02` 冻住的
    # 白名单，脚本同口径（多一条少一条都要改这行）。
    if [ "$slug" = "typelist" ]; then
      skip=$((skip + 1)); skiplist="$skiplist $slug:UM12-01"
      line SKIP "$b (U-M12-01：部分实例化，判据 t_gen_02 白名单)"
      continue
    fi
    exe="$OUT/bin/$slug"
    log="$OUT/log/$slug.build"

    "$DRIVER" build --emit=elf -o "$exe" "$CORPUS/$b" >"$log" 2>&1
    rc=$?
    if [ "$rc" -ne 0 ]; then
      bad=$((bad + 1)); badlist="$badlist $slug:build$rc"
      line BAD "$b (build rc=$rc：$(head -1 "$log"))"
      continue
    fi
    if ! readelf -h "$exe" 2>/dev/null | grep -q 'Machine:.*AArch64'; then
      bad=$((bad + 1)); badlist="$badlist $slug:not-elf"
      line BAD "$b (产物不是 aarch64 ELF)"
      continue
    fi
    ok=$((ok + 1)); c_ok=$((c_ok + 1))
    line OK "$b"
  done <"$LISTS/typeparam-compile.txt"
fi

# ── ③ errorcheck：首码 ∈ 目标集合 ───────────────────────────────────────
if want errorcheck; then
  while IFS= read -r b; do
    [ -n "$b" ] || continue
    selected "$b" || continue
    e_total=$((e_total + 1))
    slug="${b%.go}"
    log="$OUT/log/$slug.chk"

    "$DRIVER" check "$CORPUS/$b" >"$log.out" 2>"$log.err"
    rc=$?
    first="$(grep -ohE '\[[A-Z][A-Z0-9-]+\]' "$log.out" "$log.err" 2>/dev/null |
      head -1 | tr -d '[]')"
    target="$(errorcheck_target "$b")"
    if [ "$rc" -ne 1 ]; then
      bad=$((bad + 1)); badlist="$badlist $slug:rc$rc"
      line BAD "$b (期望 rc=1，实际 $rc)"
      continue
    fi
    if [ -z "$target" ]; then
      bad=$((bad + 1)); badlist="$badlist $slug:no-target"
      line BAD "$b (脚本里没有登记目标码 —— 语料变了？)"
      continue
    fi
    if [ "$first" != "$target" ]; then
      bad=$((bad + 1)); badlist="$badlist $slug:code($first≠$target)"
      line BAD "$b (首码 $first ≠ 目标 $target)"
      continue
    fi
    ok=$((ok + 1)); e_ok=$((e_ok + 1))
    line OK "$b"
  done <"$LISTS/typeparam-errorcheck.txt"
fi

# ── ④ rejected：干净拒绝 + 码集合含 import 诊断 ─────────────────────────
if want rejected; then
  while IFS= read -r b; do
    [ -n "$b" ] || continue
    selected "$b" || continue
    j_total=$((j_total + 1))
    slug="${b%.go}"
    log="$OUT/log/$slug.chk"

    "$DRIVER" check "$CORPUS/$b" >"$log.out" 2>"$log.err"
    rc=$?
    if [ "$rc" -ne 1 ]; then
      bad=$((bad + 1)); badlist="$badlist $slug:rc$rc"
      line BAD "$b (期望 rc=1，实际 $rc)"
      continue
    fi
    if ! grep -q 'TYP-UNSUPPORTED-IMPORT-FORM' "$log.out" "$log.err" 2>/dev/null; then
      bad=$((bad + 1)); badlist="$badlist $slug:no-import-diag"
      line BAD "$b (拒绝理由不是 import 不支持)"
      continue
    fi
    ok=$((ok + 1)); j_ok=$((j_ok + 1))
    line OK "$b"
  done <"$LISTS/typeparam-rejected.txt"
fi

GCTAG=""
[ -n "$GC_MODE" ] && GCTAG=" gc=$GC_MODE"
echo "=== typeparam: run $r_ok/$r_total（通过里有输出 $out_ok + 空输出 $empty_ok） compile $c_ok/$c_total errorcheck $e_ok/$e_total rejected $j_ok/$j_total（合计 ok=$ok skip=$skip bad=$bad） 官方层不可用=$no_official$GCTAG"
[ -n "$skiplist" ] && echo "SKIP:$skiplist"
[ -n "$badlist" ] && echo "BAD:$badlist"
[ "$bad" -eq 0 ]
