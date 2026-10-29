#!/bin/bash
# scripts/mutation-m12.sh —— M12 的 **c 线**变异探针（`M-M12-07..09`）。
#
# 用法（容器内）：bash scripts/mutation-m12.sh [--strict]
#
# 缺省就是 strict：三种「锚点不命中」在这里都是**真错**（代码已经写了，
# 锚点不命中就说明要打的那段被改过），没有 PENDING 这一档。
# `--strict` 现在只是个占位（判据已经写死），留着是为了兼容已写进文档的命令行。
# 环境变量：`M12_PROBE_TIMEOUT`（秒，缺省 900）。
#
# # 为什么脚本只覆盖 07..09
#
# `M-M12-01..03` 是 Task 25（a 线）跑的、`M-M12-04..06` 是 Task 35（b 线）跑的，
# 两批的记录在 `M12-report.md` §5 与 §10.1。把它们的锚点搬进脚本要把六段代码
# 重新对一遍（a 线的 mono / 实例名 / core_type 三个锚点跨两个 crate），
# 收益是「可复跑」、代价是「可能写出一个锚点已漂的假探针」—— 那是**变异绿**
# 的另一种成因（探针打错靶），比没有脚本更糟。等谁来动那几段代码时再补。
#
# # 纪律（M9–M11 踩过的，全部照抄）
#
# 1. **同一调用内完成变异与恢复**（容器 `/tmp` 调用即焚）。
# 2. **恢复之后必须 `touch`**：`mv` 从备份恢复会保留备份的 mtime，而变异期间
#    编出的 `.rlib` 更新 ⇒ cargo 认为源码没变、继续用变异版。
# 3. **改 Rust 源码的探针必须 `cargo clean -p`**（绑定挂载 mtime 只到整秒）。
# 4. **锚点也要核**：替换不命中就是 MISS（M9 踩过：签名写错、替换没命中、
#    测试照绿）。
# 5. `rt.c` 是**每次 build 都由 clang 现编**的，不走 cargo 缓存 ⇒ 改它不必
#    `cargo clean`，但仍然要 `touch`（有些路径按 mtime 判要不要重编）。
# 6. **不许并发跑**：两条探针共用一个 `crates/rgoc-driver/target/obj-map/`，
#    一边执行一边被另一边改写同一份可执行文件 ⇒ 进程原地打转（实测 99% CPU
#    烧了 28 分钟，既不红也不绿）。脚本用 `target/.mutation-m12.lock` 互斥。
# 7. **每条探针要能超时**：判据「红」必须是**断言失败**，卡死既不是红也不是绿，
#    还会把下一轮拖进「上一轮没跑完」。超时单独记 `HANG` 一栏，`M12_PROBE_TIMEOUT`
#    可调（缺省 900 秒）。
#
# # 三条
#
# | 变异 | 期望红的判据 |
# |---|---|
# | `M-M12-07` 内联跨 panic 边界（去掉「有 `Defer` 就不内联」）| `t_opt_01_带_defer_的被调方不内联` |
# | `M-M12-08` 逃逸分析把逃逸对象判成不逃逸（`Store` 进全局不再标）| `t_opt_02_存进全局的形参_槽进_arena` |
# | `M-M12-09` SwissTable 把 `DELETED` 也当终止符（探测少一步）| `t_map_11_十万键的语义不随探测结构变` |
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/rgoc" || { echo "进不去 $ROOT/rgoc" >&2; exit 2; }

RT="../runtime/native/aarch64-unknown-linux-gnu/rt.c"

red=0
green=0
miss=0
hang=0
err=0
report=""

# ⚠️ **互斥锁**（M12 Task 41 第二次跑踩的）：两个 `in-container.sh` 同时跑这个
# 脚本时，两边都往同一个 `crates/rgoc-driver/target/obj-map/` 写同名可执行文件，
# 一边执行一边被另一边改写 ⇒ 进程原地打转（实测 99% CPU 烧掉 28 分钟，既不红
# 也不绿）。`cargo` 自己有 target 锁，但**测试跑起来的产物**没有。用 `mkdir`
# 抢一个原子锁（容器 `/tmp` 是调用即焚的，锁必须落在 bind mount 上）。
LOCK="target/.mutation-m12.lock"
if ! mkdir "$LOCK" 2>/dev/null; then
  echo "已有另一个 mutation-m12.sh 在跑（锁：$ROOT/rgoc/target/.mutation-m12.lock）。" >&2
  echo "并发跑会让两条探针抢同一个可执行文件 —— 先确认没有在跑，再 rmdir 这个目录。" >&2
  exit 4
fi

# ⚠️ 中断兜底（M12 Task 41 首次跑踩的）：一条 `cargo test` 可能跑几分钟，撞上
# 容器/调用方的 SIGTERM（exit 137）就死在半路 —— 那时变异版**留在工作树里**，
# 下一次跑第一条探针会因为「锚点已经被自己改掉」而报 MISS。`M-M12-09` 就这么
# 假 MISS 过一次。所以把「当前正在变异的目标」记在全局，任何退出路径都还原。
CUR_FILE=""
CUR_BAK=""
_restore() {
  if [ -n "$CUR_FILE" ] && [ -f "$CUR_BAK" ]; then
    mv "$CUR_BAK" "$CUR_FILE" || true
    touch "$CUR_FILE" || true
  fi
  CUR_FILE=""
  CUR_BAK=""
}
_cleanup() {
  _restore
  rmdir "$LOCK" 2>/dev/null || true
}
trap '_cleanup' EXIT INT TERM

# ⚠️ **单条探针要带超时**（同上）：判据「红」必须是**断言失败**，不能是卡死。
# 卡死既不红也不绿，还会把下一轮拖进「上一轮没跑完」的状态。超时单独记一栏。
PROBE_TIMEOUT="${M12_PROBE_TIMEOUT:-900}"

# 用法：_probe <名字> <文件> <旧串> <新串> <恢复后要不要 cargo clean -p 的 crate 或 "-"> <cargo test 参数…>
#
# 第二处（可选）变异：调用前设 `MUT_EXTRA`，格式 `旧串<换行>---<换行>新串`。
# 只在同一个文件上、同一次 python 里落；写两处的时候必须在注释里说清**为什么
# 第二处不是逻辑改动**（`M-M12-09` 就是这种：第二处只是防变异自己死循环的兜底）。
_probe() {
  local name="$1" file="$2" old="$3" new="$4" clean_crate="$5"
  shift 5
  local bak="$file.m12bak"
  cp "$file" "$bak"
  CUR_FILE="$file"
  CUR_BAK="$bak"
  if ! MF="$file" MO="$old" MN="$new" python3 -c '
import os, pathlib, sys

p = pathlib.Path(os.environ["MF"])
s = p.read_text(encoding="utf-8")
pairs = [(os.environ["MO"], os.environ["MN"])]
extra = os.environ.get("MUT_EXTRA", "")
if extra:
    o2, n2 = extra.split("\n---\n")
    pairs.append((o2, n2))
for old, new in pairs:
    if old not in s:
        sys.exit(3)
    s = s.replace(old, new, 1)
p.write_text(s, encoding="utf-8")
'; then
    _restore
    report="$report
  MISS    $name  <<< 锚点没命中（要打的那段代码已经变了）"
    miss=$((miss + 1))
    return
  fi

  local log="target/.mutation-m12.out.log"
  local rc=0
  timeout "$PROBE_TIMEOUT" cargo test "$@" >"$log" 2>&1 || rc=$?
  local out
  out="$(grep -E '^test result' "$log" | tail -1)"
  _restore
  touch "$file"
  [ "$clean_crate" != "-" ] && cargo clean -p "$clean_crate" >/dev/null 2>&1
  if [ "$rc" -eq 124 ]; then
    report="$report
  HANG    $name  <<< 超时 ${PROBE_TIMEOUT}s（卡死既不是红也不是绿）"
    hang=$((hang + 1))
  elif printf '%s' "$out" | grep -q 'FAILED'; then
    report="$report
  RED     $name  [$out]"
    red=$((red + 1))
  elif [ "$rc" -eq 0 ]; then
    report="$report
  GREEN   $name  [$out]  <<< 判据没牙"
    green=$((green + 1))
  else
    report="$report
  ERROR   $name  [rc=$rc] <<< 变异版编译不过 / 测试没跑起来，这既不是红也不是绿"
    err=$((err + 1))
  fi
}

# ── M-M12-07：内联跨 panic 边界 ────────────────────────────────────────
# 去掉「有 `Defer` 就不内联」这一条。为什么它就是「跨 panic 边界」：`defer`
# 登记在**当前帧**的链上，而内联会把被调方的体搬进调用方 —— 于是被调方的
# defer 跑到调用方的帧上，执行时机（以及 panic 展开时跑谁）都变了。
# `t_opt_01_带_defer_的被调方不内联` 断言的就是「这种被调方不许被搬」，
# 变异之后它会红。
_probe "M-M12-07 内联跨 panic 边界（有 Defer 也内联）" \
  "crates/rgoc-ssa/src/opt/inline.rs" \
  'Instr::Defer { .. } | Instr::Go { .. } | Instr::CapturePtr { .. } => return false,' \
  'Instr::Go { .. } | Instr::CapturePtr { .. } => return false,' \
  rgoc-ssa \
  -p rgoc-ssa --test opt_inline t_opt_01_带_defer_的被调方不内联

# ── M-M12-08：逃逸分析把逃逸对象判成不逃逸 ────────────────────────────
# 把「写进全局 / 堆算逃逸」那一条的标记清空 ⇒ `g(p){ G = p }` 的形参被判成
# 不逃逸 ⇒ 调用方的槽留在帧里，而地址已经存进全局：下一次调用复用同一块内存，
# 全局那个指针就指向别人的变量。`t_opt_02_存进全局的形参_槽进_arena` 直接红。
#
# ⚠️ 只改 `escape.rs` 的 `summary`，不改 `frame.rs::escaping_slots` 的同名分支：
# 后者管的是「**本函数**里 `G = &x`」，前者管的是「**被调方**存了它」。
# 这条变异打的是跨函数那一票，正是 `T-OPT-02` 新增的那一半。
_probe "M-M12-08 逃逸分析判错（Store 进全局不再标）" \
  "crates/rgoc-arm64/src/escape.rs" \
  '                } if !matches!(f.roots[root.index()], Root::Slot(_)) => {
                    mark(addr, &mut esc);
                    mark(val, &mut esc);
                }' \
  '                } if !matches!(f.roots[root.index()], Root::Slot(_)) => {
                    /* 变异 M-M12-08：写进全局 / 堆不再算逃逸 */
                }' \
  rgoc-arm64 \
  -p rgoc-arm64 --test escape_summary t_opt_02_存进全局的形参_槽进_arena

# ── M-M12-09：SwissTable 把 DELETED 也当终止符（探测序列少一步）──────────
# 把 `if (emp != 0)` 改成 `if (emp != 0 || del != 0)`：探测在**第一个墓碑所在的
# 组**就收手，于是「键在墓碑之后」的那一段全被判成不存在，`rt_map_set` 就把
# 同一个键又插一遍（份数涨、`len` 涨、`ok` 假阳性）。
#
# ⚠️ 为什么用十万键那条判据：`cap = 8` 时一组（16 个控制字节）覆盖整张表，
# 任何终止判定都命中，删一半再查也照样绿（`t_map_10` 就是这么绿过来的）。
# 十万键 ⇒ `cap = 131072`、8192 组，探测链才真的跨组、跨墓碑。
#
# ⚠️ **第二处编辑不是逻辑改动，是防死循环的兜底**。只改第一处的话这条变异
# **不红，而是卡死**（实测 `M12_PROBE_TIMEOUT` 900s 打满）。成因已用判别实验
# 钉死（`.probe/m9-hang2.sh`）：`first_del` 的记录写在 `emp` 判定**之后**，所以
# 变异后能走到 `return (i + low_bit_index(emp)) & mask;` 而此刻 `emp == 0`，
# `low_bit_index` 里的 `while ((x & 1u) == 0)` 死转。加上 `if (x == 0) return 0;`
# 之后**不再卡死**，语义错误就露出来了（`.probe/m9-hang.sh` 同形程序的读数）：
#
# | n | 只改第一处 | 再加兜底 |
# |---|---|---|
# | 8 / 16 / 64 / 1000 | 跑完 | 跑完且正确 |
# | 200 | 卡在查询相位 | `100 19900 0` |
# | 20000 | 卡在删除相位 | `10016 199990000 7`（应 `10000 … 0`）|
# | 100000 | 卡在删除相位 | `50822 4999950000 818`（应 `50000 … 0`）|
#
# 兜底本身对**正确**代码是零改动（`low_bit_index` 正常路径永远拿到非零掩码），
# 所以变异的行为差异仍然只来自第一处。
MUT_EXTRA=$'static u64 low_bit_index(u32 x) {\n    u64 i = 0;\n---\nstatic u64 low_bit_index(u32 x) {\n    if (x == 0) return 0;\n    u64 i = 0;'
export MUT_EXTRA
_probe "M-M12-09 SwissTable 把 DELETED 当终止符（探测少一步）" \
  "$RT" \
  '        if (emp != 0) {' \
  '        if (emp != 0 || del != 0) {' \
  - \
  -p rgoc-driver --test obj_map t_map_11_十万键的语义不随探测结构变
unset MUT_EXTRA

echo "M12 c 线变异（三条）"
echo "----------------------------------------"
printf '%s\n' "$report"
echo "----------------------------------------"
echo "RED=$red  GREEN=$green  MISS=$miss  HANG=$hang  ERROR=$err"
[ "$red" -eq 3 ] && [ "$green" -eq 0 ] && [ "$miss" -eq 0 ] && [ "$hang" -eq 0 ] && [ "$err" -eq 0 ]
