#!/bin/bash
# scripts/mutation-m10.sh —— M10 的变异探针（`M10-plan.md` Task 17 / 30 / 36 / 41）。
#
# 用法（容器内）：bash scripts/mutation-m10.sh
#
# # 为什么变异必须实证
#
# 「测试全绿」有两种可能：**代码对**，或者**判据恒真**。M10 这一轮已经抓到
# 四条假绿，全部是「判据没牙」而不是「代码对」：
#
#   * `R-10-4`：`Instr::Make` 被 DCE 删 ⇒ 分配根本没发生
#   * `R-10-11`：函数级 effect 摘要说 `new`/`make` 纯 ⇒ 整个 `churn()` 调用被删
#   * `R-10-12`：bump 优先 ⇒ free list 形同虚设 ⇒ 回收的块从没被复用
#   * `t_root_12` 的 churn 大小写错 ⇒ 8 KiB 装不进 1040 字节的块
#
# 四条的共同点：**变异跑绿了，而绿的原因是「被测代码没执行」**。所以脚本必须
# 打印 RED/GREEN/MISS，GREEN 一律算失手。
#
# # 纪律
#
# 1. **同一调用内完成变异与恢复**（容器 `/tmp` 调用即焚）。
# 2. **恢复之后必须 `touch`**：`mv` 从 `.m10bak` 恢复会保留备份的 mtime（早于
#    变异），而变异期间编出的 `.rlib` 更新 ⇒ cargo 认为源码没变、继续用变异版。
# 3. **`rt.c` 不进 cargo 的依赖图**（每次 `build` 由 clang 重编）⇒ 不需要
#    `cargo clean`；改 Rust 源码的探针才需要（本脚本暂时都是 `rt.c`）。
# 4. **锚点也要核**：替换不命中就是 MISS（M9 踩过：签名写错、替换没命中、
#    测试照绿）。
#
# # 这里 9 条
#
# | 变异 | 期望红的判据 |
# |---|---|
# | `M-M10-01` 块大小恒 32 | `t_gc_04` |
# | `M-M10-02` 不置起始位 | `t_heap_05` |
# | `M-M10-03` sweep 把所有块当活 | `t_gc_04` |
# | `M-M10-04` 不写标记位 | `t_root_12` |
# | `M-M10-05` 去掉堆判定（`chunk_of`）| `t_gc_12` |
# | `M-M10-06` `resolve_block` 不退化 | `t_heap_05` |
# | `M-M10-07` 复用的块不清零 | `t_heap_01` |
# | `M-M10-08` 不走 free list | `t_heap_04` |
# | `M-M10-09` `mark_roots` 不扫全局 | `t_root_12` |
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/rgoc" || { echo "进不去 $ROOT/rgoc" >&2; exit 2; }

RT="../runtime/native/aarch64-unknown-linux-gnu/rt.c"

pass=0
fail=0
report=""

# 用法：probe <名字> <文件> <旧串> <新串> <cargo test 参数…>
probe() {
  local name="$1" file="$2" old="$3" new="$4"
  shift 4
  cp "$file" "$file.m10bak"
  if ! MF="$file" MO="$old" MN="$new" python3 -c '
import os, pathlib, sys
p = pathlib.Path(os.environ["MF"])
s = p.read_text(encoding="utf-8")
old = os.environ["MO"]
new = os.environ["MN"]
if old not in s:
    sys.exit(3)
p.write_text(s.replace(old, new, 1), encoding="utf-8")
'; then
    mv "$file.m10bak" "$file"
    touch "$file"
    report="$report
  MISS  $name  <<< 锚点没命中（改的那段代码已经变了）"
    fail=$((fail + 1))
    return
  fi

  local out
  out="$(cargo test "$@" 2>&1 | grep -E '^test result' | tail -1)"
  mv "$file.m10bak" "$file"
  touch "$file"
  if printf '%s' "$out" | grep -q 'FAILED'; then
    report="$report
  RED   $name  [$out]"
    pass=$((pass + 1))
  else
    report="$report
  GREEN $name  [$out]  <<< 判据没牙"
    fail=$((fail + 1))
  fi
}

# ── M-M10-01：块大小恒 32 ──────────────────────────────────────────────
# 块大小算错 ⇒ sweep 走到块中间、把中间当块首 ⇒ 整条链错位。
probe "M-M10-01 块大小恒 32" "$RT" \
  '    return NK_BLK(nk);' \
  '    return 32;' \
  -p rgoc-driver --test gc_reclaim t_gc_04

# ── M-M10-02：不置起始位 ──────────────────────────────────────────────
# 起始位图是「内部指针回块首」的唯一依据。不置位 ⇒ 连块首都解析不到。
probe "M-M10-02 不置起始位" "$RT" \
  'static void bm_set(Chunk *c, u64 off) { c->meta_bm[off >> 6] |= (u8)(1u << ((off >> 3) & 7)); }' \
  'static void bm_set(Chunk *c, u64 off) { (void)c; (void)off; }' \
  -p rgoc-driver --test gc_heap t_heap_05

# ── M-M10-03：sweep 把所有块当活 ──────────────────────────────────────
probe "M-M10-03 sweep 不回收" "$RT" \
  '            } else if (h & HDR_MARK) {' \
  '            } else if (1) {' \
  -p rgoc-driver --test gc_reclaim t_gc_04

# ── M-M10-04：不写标记位 ──────────────────────────────────────────────
# 标记位不置 ⇒ 所有块都「没标记」⇒ 全被回收（含全局根指向的那个）。
probe "M-M10-04 不写标记位" "$RT" \
  '    *(u64 *)blk = h | HDR_MARK;' \
  '    *(u64 *)blk = h;' \
  -p rgoc-driver --test gc_reclaim t_root_12

# ── M-M10-05：去掉堆判定 ──────────────────────────────────────────────
# `mark_ptr` 的第一步是 `chunk_of`。去掉它 ⇒ 栈地址 / rodata 地址都被当块首
# 去读块头 ⇒ 读到垃圾 ⇒ 崩或误标。
probe "M-M10-05 去掉堆判定" "$RT" \
  '    Chunk *c = chunk_of(p);
    if (c == 0) {
        return;
    }' \
  '    Chunk *c = heap;
    if (c == 0) {
        return;
    }' \
  -p rgoc-driver --test gc_reclaim t_gc_12

# ── M-M10-06：resolve_block 不退化 ────────────────────────────────────
# 本页没起点时必须往前找。漏了退化 ⇒ 跨页的内部指针解析不到块首。
probe "M-M10-06 resolve_block 不退化" "$RT" \
  '    for (u64 q = page; q > 0; q--) {' \
  '    for (u64 q = 0; q > 0; q--) {' \
  -p rgoc-driver --test gc_heap t_heap_05

# ── M-M10-07：复用的块不清零 ──────────────────────────────────────────
probe "M-M10-07 复用的块不清零" "$RT" \
  '    rt_memset(p + BLK_HDR, 0, blk - BLK_HDR);' \
  '    /* 变异：不清零 */' \
  -p rgoc-driver --test gc_heap t_heap_01

# ── M-M10-08：不走 free list ──────────────────────────────────────────
probe "M-M10-08 不走 free list" "$RT" \
  '    for (Chunk *c = heap; c != 0 && p == 0; c = c->next) {
        p = free_take(c, &blk);
    }' \
  '    /* 变异：跳过 free list */' \
  -p rgoc-driver --test gc_reclaim t_heap_04

# ── M-M10-09：mark_roots 不扫全局 ─────────────────────────────────────
probe "M-M10-09 mark_roots 不扫全局" "$RT" \
  '    for (u64 i = 0; i < rt_nglobals; i++) {' \
  '    for (u64 i = 0; i < 0; i++) {' \
  -p rgoc-driver --test gc_reclaim t_root_12

echo "=== mutation-m10: RED $pass / 失手 $fail"
printf '%s\n' "$report"
[ "$fail" -eq 0 ]
