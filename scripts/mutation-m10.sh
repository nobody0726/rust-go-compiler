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
# # 这里 19 条
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
# | `M-M10-10` `Composite` 不是 safepoint | `t_heap_07` |
# | `M-M10-11` `mapslots` 不扫 | `t_map_11` |
# | `M-M10-12` `free_push` 不置起点位 | `t_heap_08` |
# | `M-M10-13` `mapslots` 不滤 `TOMB` | `t_map_12` |
# | `M-M10-14` `mark_defer` 不扫 | `t_root_13` |
# | `M-M10-15` 逃逸槽退回 arena | `t_root_14` |
# | `M-M10-16` 帧链不摘 | `t_root_11` |
# | `M-M10-17` `sp_idx` 恒 0 | `t_root_10` |
# | `M-M10-18` 整帧保守扫描 | `t_prec_01` |
# | `M-M10-19` 帧位序反 | `t_root_09` |
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

# 用法：probe_rs <名字> <文件> <旧串> <新串> <crate> <cargo test 参数…>
#
# 与 `probe` 的区别：改的是 **Rust 源码** ⇒ 恢复之后必须 `cargo clean -p`。
# 绑定挂载上 mtime 只到整秒，`touch` 之后可能仍与 rlib 同秒 ⇒ cargo 认为
# 「源码没变」、继续用变异版的 rlib（M9 踩过：单跑绿、全量红）。
probe_rs() {
  local name="$1" file="$2" old="$3" new="$4" crate="$5"
  shift 5
  probe "$name" "$file" "$old" "$new" "$@"
  cargo clean -p "$crate" >/dev/null 2>&1
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

# ── M-M10-10：`Composite` 不是 safepoint ──────────────────────────────
# 切片字面量要 `rt_makeslice` 建底层数组。不算位点 ⇒ 构造过程中前一个数组
# 被回收、后一个复用同一块 ⇒ 三个 slice 头指向同一个数组。
probe_rs "M-M10-10 Composite 不是 safepoint" "crates/rgoc-ssa/src/live.rs" \
  'Instr::Composite { typ, .. } => is_slice(tt, *typ),' \
  'Instr::Composite { .. } => false,' \
  rgoc-ssa \
  -p rgoc-driver --test gc_heap t_heap_07

# ── M-M10-11：`mapslots` 不扫 ─────────────────────────────────────────
# map 的值里有指针（`map[int][]int`）⇒ 槽块不扫 ⇒ 那些数组被回收。
probe "M-M10-11 mapslots 不扫" "$RT" \
  '    case K_MAPSLOTS: {' \
  '    case K_MAPSLOTS: { break;' \
  -p rgoc-driver --test gc_heap t_map_11

# ── M-M10-12：`free_push` 不置起点位 ──────────────────────────────────
# 起始位图的语义是「这里有一个块头」，不区分死活。空闲区不置位 ⇒
# `resolve_block` 往前找到残留旧位 ⇒ 把空闲区里的旧字节当块头扫 ⇒ 段错误。
probe "M-M10-12 free_push 不置起点位" "$RT" \
  '    bm_set(c, off);
    sum_set(c, off >> 12);
}' \
  '}' \
  -p rgoc-driver --test gc_heap t_heap_08

# ── M-M10-13：`mapslots` 不滤 TOMB ────────────────────────────────────
# 槽块的 `n` 是 cap，`delete` 只改状态不清值 ⇒ 不过滤就把死对象钉住。
probe "M-M10-13 mapslots 不滤 TOMB" "$RT" \
  '            if (slot[0] != MAP_LIVE) {
                continue;
            }
            mark_words(slot + 8, kd);' \
  '            mark_words(slot + 8, kd);' \
  -p rgoc-driver --test gc_heap t_map_12

# ── M-M10-14：`mark_defer` 不扫 ───────────────────────────────────────
probe "M-M10-14 mark_defer 不扫" "$RT" \
  '        mark_ptr(r->fv);' \
  '        (void)r;' \
  -p rgoc-driver --test gc_stack t_root_13

# ── M-M10-15：逃逸槽退回 arena ────────────────────────────────────────
# M9 就是这么写的：闭包捕获 `&槽`，槽在 arena ⇒ 槽里的指针没有根。
probe "M-M10-15 逃逸槽退回 arena" "$RT" \
  '    gc_pause++;
    void *p = gc_alloc(desc->size, K_OBJECT, desc, 0);
    gc_pause--;
    return p;' \
  '    return rt_arena_alloc(desc->size, 8);' \
  -p rgoc-driver --test gc_stack t_root_14

# ── M-M10-16：帧链不摘 ────────────────────────────────────────────────
# `epilogue` 是唯一出口。漏摘 ⇒ 链指向已返回的帧 ⇒ 那块栈被下一帧覆盖 ⇒
# 下一次 GC 拿垃圾当 `fi` / `sp_idx` 去扫。
probe_rs "M-M10-16 帧链不摘" "crates/rgoc-arm64/src/isel.rs" \
  '        asm.ins(&format!("ldr x10, [x29, #{}]", crate::frame::REC_PREV));
        asm.ins("str x10, [x9]");' \
  '        asm.ins("nop");' \
  rgoc-arm64 \
  -p rgoc-driver --test gc_stack t_root_11

# ── M-M10-17：`sp_idx` 恒 0 ───────────────────────────────────────────
# 写一次就不动 ⇒ GC 用 0 号位点的位图扫整帧 ⇒ 后面才活的指针全漏标。
probe_rs "M-M10-17 sp_idx 恒 0" "crates/rgoc-arm64/src/isel.rs" \
  '    let Some(spi) = cx.sp_of.get(&(u32::try_from(bi).unwrap_or(u32::MAX), idx)) else {
        return;
    };' \
  '    if true {
        return;
    }
    let Some(spi) = cx.sp_of.get(&(u32::try_from(bi).unwrap_or(u32::MAX), idx)) else {
        return;
    };' \
  rgoc-arm64 \
  -p rgoc-driver --test gc_stack t_root_10

# ── M-M10-18：整帧保守扫描 ────────────────────────────────────────────
# 不看位图、把整帧所有字都当根 ⇒ 像地址的整数被当根 ⇒ 死对象钉住不回收。
probe "M-M10-18 整帧保守扫描" "$RT" \
  '            const u64 *bits = fi->sp_bits + fi->sp_off[idx];
            u64 words = ((u64)fi->frame_size / 8 + 63) / 64;
            u8 *base = fp - fi->frame_size;
            for (u64 w = 0; w < words; w++) {
                u64 word = bits[w];
                while (word != 0) {
                    u64 b = (u64)__builtin_ctzll(word);
                    word &= word - 1;
                    u64 bit = w * 64 + b;
                    if (bit * 8 >= (u64)fi->frame_size) {
                        continue;
                    }
                    void *q;
                    rt_memcpy(&q, base + bit * 8, 8);
                    mark_ptr(q);
                }
            }' \
  '            u8 *base = fp - fi->frame_size;
            for (u64 bit = 0; bit * 8 < (u64)fi->frame_size; bit++) {
                void *q;
                rt_memcpy(&q, base + bit * 8, 8);
                mark_ptr(q);
            }' \
  -p rgoc-driver --test gc_precise t_prec_01

# ── M-M10-19：帧位序反 ────────────────────────────────────────────────
# 位序是 bit i ↔ `fp - frame_size + 8i`（`C5` §8.4 冻结）。基点写错 ⇒
# 扫到的是帧外的字。
probe "M-M10-19 帧位序反" "$RT" \
  '            u8 *base = fp - fi->frame_size;' \
  '            u8 *base = fp - 8;' \
  -p rgoc-driver --test gc_stack t_root_09

echo "=== mutation-m10: RED $pass / 失手 $fail"
printf '%s\n' "$report"
[ "$fail" -eq 0 ]
