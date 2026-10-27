#!/bin/bash
# scripts/mutation-m11.sh —— M11 的变异探针（`M11-tests.md` §9 的 15 条）。
#
# 用法（容器内）：bash scripts/mutation-m11.sh [--strict]
#
#   --strict   锚点不命中算 MISS（失败）。Phase 0 用缺省（PENDING），
#              Phase 1 起每个 Task 收口都用 `--strict`。
#
# # 为什么 Phase 0 的锚点是 PENDING 而不是 MISS
#
# `M-M11-01`（`rt_switch` 不存 `d8..d15`）与 `M-M11-07`（`send` 的 `closed`
# 检查挪后）要改的代码**还不存在** —— `rt_switch` 在 Task 9、`hchan` 的四操作
# 在 Task 17–19。所以 Phase 0 的期望是「脚本骨架能跑、能如实报锚点缺失」，
# 不是「变异变红」。
#
# ⚠️ 但 **PENDING 不是通过**。Phase 1 收口（Task 15）起必须 `--strict` ——
# 那时 `rt_switch` 已经存在，锚点不命中就说明「改的那段代码变了」，是真错。
#
# # 为什么变异必须实证（沿 M10 的口径）
#
# 「测试全绿」有两种可能：**代码对**，或者**判据恒真**。M10 抓到 4 条假绿，
# 全部是「判据没牙」；M9 抓到 3 条。M11 的 15 条变异里至少 4 条（`M-M11-05`
# 参数深拷贝 / `M-M11-09` select 求值 / `M-M11-13` 只扫当前 G / `M-M11-15`
# Sudog 回 arena）是「实现对了但判据没测到」的高危形态。
#
# # 纪律（M9/M10 踩过的，全部照抄）
#
# 1. **同一调用内完成变异与恢复**（容器 `/tmp` 调用即焚）。
# 2. **恢复之后必须 `touch`**：`mv` 从备份恢复会保留备份的 mtime，而变异期间
#    编出的 `.rlib` 更新 ⇒ cargo 认为源码没变、继续用变异版。
# 3. **改 Rust 源码的探针必须 `cargo clean -p`**（绑定挂载 mtime 只到整秒）。
# 4. **锚点也要核**：替换不命中就是 MISS（M9 踩过：签名写错、替换没命中、
#    测试照绿）。
#
# # 这里 15 条（Phase 0 只有前两条，其余随各自 Task 加进来）
#
# | 变异 | 期望红的判据 | 加进来的 Task |
# |---|---|---|
# | `M-M11-01` `rt_switch` 不保存 `d8..d15` | `T-STK-01` | 9 |
# | `M-M11-02` 保存区 160 → 144（漏一对）| `T-STK-01` | 9 |
# | `M-M11-03` 序言不发栈检查 | `T-STK-03` | 10 |
# | `M-M11-04` 栈检查对 `off_stack` 帧也发 | `T-STK-05` | 10 |
# | `M-M11-05` `go` 只拷指针不拷值 | `T-GO-04` | 12 |
# | `M-M11-06` 就绪队列改成队头插入 | `T-GO-03` | 12 |
# | `M-M11-07` `send` 的 `closed` 检查挪到「buf 未满」之后 | `T-CHAN-03` | 19 |
# | `M-M11-08` `close` 只唤醒 `recvq` 不唤醒 `sendq` | `T-CHAN-04` | 19 |
# | `M-M11-09` `select` 阶段①只求值第一个 case | `T-SEL-01` | 22 |
# | `M-M11-10` `select` 不撤销（只删 winner 的 sudog）| `T-SEL-04` | 24 |
# | `M-M11-11` 死锁判定漏「有就绪 G」| `T-EXIT-05` | 26 |
# | `M-M11-12` `main` 返回后等 goroutine | `T-EXIT-02` | 27 |
# | `M-M11-13` `mark_roots` 只扫当前 G 的链 | `T-GROOT-01` | 33 |
# | `M-M11-14` `rt_gc_force_off` 空实现 | `T-GROOT-04` | 31 |
# | `M-M11-15` `Sudog` 改回 arena | `T-CHAN-08` | 18 |
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/rgoc" || { echo "进不去 $ROOT/rgoc" >&2; exit 2; }

RT="../runtime/native/aarch64-unknown-linux-gnu/rt.c"

STRICT=0
[ "${1:-}" = "--strict" ] && STRICT=1

red=0
green=0
pending=0
miss=0
report=""

# 用法：_probe_body <名字> <文件> <旧串> <新串> <锚点缺失时算什么> <cargo test 参数…>
#
# 「锚点缺失时算什么」：`strict` ⇒ MISS（失败）；`pending` ⇒ PENDING（未实现）。
_probe_body() {
  local name="$1" file="$2" old="$3" new="$4" on_missing="$5"
  shift 5
  local bak="$file.m11bak"
  cp "$file" "$bak"
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
    mv "$bak" "$file"
    touch "$file"
    if [ "$on_missing" = "pending" ]; then
      report="$report
  PENDING $name  <<< 锚点还不存在（代码未实现，Phase 0 预期）"
      pending=$((pending + 1))
    else
      report="$report
  MISS    $name  <<< 锚点没命中（改的那段代码已经变了）"
      miss=$((miss + 1))
    fi
    return
  fi

  local out
  out="$(cargo test "$@" 2>&1 | grep -E '^test result' | tail -1)"
  mv "$bak" "$file"
  touch "$file"
  if printf '%s' "$out" | grep -q 'FAILED'; then
    report="$report
  RED     $name  [$out]"
    red=$((red + 1))
  else
    report="$report
  GREEN   $name  [$out]  <<< 判据没牙"
    green=$((green + 1))
  fi
}

# Phase 0 用：锚点缺失 ⇒ PENDING（未实现）。
probe_pending() {
  local name="$1" file="$2" old="$3" new="$4"
  shift 4
  _probe_body "$name" "$file" "$old" "$new" pending "$@"
}

# Phase 1 起用：锚点缺失 ⇒ MISS（真错）。
probe() {
  local name="$1" file="$2" old="$3" new="$4"
  shift 4
  _probe_body "$name" "$file" "$old" "$new" strict "$@"
}

# 改 Rust 源码的探针：恢复后必须 `cargo clean -p`。
probe_rs() {
  local name="$1" file="$2" old="$3" new="$4" crate="$5"
  shift 5
  probe "$name" "$file" "$old" "$new" "$@"
  cargo clean -p "$crate" >/dev/null 2>&1
}

if [ "$STRICT" -eq 1 ]; then
  PROBE=probe
  PROBE_RS=probe_rs
  PROBE_P=probe_pending   # 还没实现的（M11b 的 hchan）仍走 pending
  MODE="strict（锚点缺失算 MISS；M11b 的两条除外）"
else
  PROBE=probe_pending
  PROBE_RS=probe_pending
  PROBE_P=probe_pending
  MODE="phase0（锚点缺失算 PENDING）"
fi

# ── M-M11-01：`rt_switch` 不保存 `d8..d15` ────────────────────────────
# 浮点寄存器不保存 ⇒ 跨切换的浮点值串味。`T-STK-01` 用**可区分值**填满
# `x19..x28` 与 `d8..d15`（0/1 看不出「没保存」和「保存对了」的区别）。
$PROBE "M-M11-01 rt_switch 不存 d8..d15" "$RT" \
  'stp  d8,  d9,  [sp, #96]' \
  '/* 变异 M-M11-01：不保存 d8/d9 */' \
  -p rgoc-driver --test go_stack t_stk_01

# ── M-M11-02：恢复时 `add sp` 少 16（漏一对）──────────────────────────
# ⚠️ 第一次写的是「只改 `sub sp, #160` → `#144`」，**跑绿了** —— 因为栈上
# 有余量，保存区短 16 字节没造成可观测的影响（`stp d14,d15,[sp,#144]` 写到
# 了 `sub` 范围之外，但那是调用方的栈，还没被用到）。
# 改成 `add sp, sp, #160` → `#144`：恢复后 sp 少 16 ⇒ 每个调用返回都错位 ⇒ 必崩。
$PROBE "M-M11-02 恢复时 add sp 少 16" "$RT" \
  'add  sp, sp, #160' \
  'add  sp, sp, #144' \
  -p rgoc-driver --test go_stack t_stk_01

# ── M-M11-03：序言不发栈检查 ──────────────────────────────────────────
# 删掉 `cmp sp, x9` ⇒ `t_stk_04` 数不到检查（`cmps == 0`）。
# 这条改的是 **Rust 源码** ⇒ 必须 `cargo clean -p`（绑定挂载 mtime 只到整秒）。
$PROBE_RS "M-M11-03 序言不发栈检查" "crates/rgoc-arm64/src/isel.rs" \
  '        asm.ins("cmp sp, x9");' \
  '        // 变异：不发栈检查' \
  rgoc-arm64 \
  -p rgoc-driver --test go_stack t_stk_04

# ── M-M11-04：检查条件写反（`b.hs` → `b.lo`）──────────────────────────
# 正常程序（`sp >= stack_lo`）会**误报溢出** ⇒ `t_stk_01` 直接 rc=2。
# 等价于「检查写错」—— 比「对 off_stack 也发」更容易做，且抓的是同一类错。
$PROBE_RS "M-M11-04 检查条件写反" "crates/rgoc-arm64/src/isel.rs" \
  '        asm.ins(&format!("b.hs .Lso_{}", cx.fi_index));' \
  '        asm.ins(&format!("b.lo .Lso_{}", cx.fi_index));' \
  rgoc-arm64 \
  -p rgoc-driver --test go_stack t_stk_01

# ── M-M11-07：`send` 的 `closed` 检查挪到「buf 未满」之后 ──────────────
# `C11` §3.2 的 ⚠️：`closed` 检查必须**排第一**。挪到后面 ⇒ 往已关闭的有缓冲
# 通道发送会**静默塞进缓冲**，而不是 `panic: send on closed channel`。
# ⚠️ 变异写成「条件收窄」而不是「整块搬走」：`qcount == dataqsiz` 让**未满的
# 有缓冲通道**跳过 closed 检查 —— 语义上等价于「检查挪到 buf 满之后」，
# 但只改一行，锚点不会因为缩进变化而失配。
#
# ⚠️ 锚点**必须带上下两行**：`rt_send` 改成循环后它的缩进是 8 格，而
# `rt_close` 里也有一模一样的 4 格 `if (c->closed != 0) {` —— 只写那一行会
# 误伤 `rt_close`（变异「关闭不查重复」而不是「发送不查关闭」）。
$PROBE "M-M11-07 send 的 closed 检查挪后" "$RT" \
  '    if (c->closed != 0) {
        rt_panic_send_closed();' \
  '    if (c->closed != 0 && c->qcount == c->dataqsiz) {
        rt_panic_send_closed();' \
  -p rgoc-driver --test go_chan t_chan_06

# ── M-M11-08：`close` 只唤醒 `recvq` 不唤醒 `sendq` ────────────────────
# 挂在 `sendq` 上的 G 永远醒不过来 ⇒ 死锁（或者主 G 返回后悄悄丢掉它）。
# `t_chan_06` 的 ① 不覆盖这条（它是「已关闭后再发」而不是「阻塞中被关闭」），
# 所以这里打 `t_chan_07`（阻塞的发送方被 close 唤醒后 panic）。
$PROBE "M-M11-08 close 不唤醒 sendq" "$RT" \
  '    while ((s = waitq_pop(&c->sendq_head, &c->sendq_tail)) != 0) {
        rt_wake_retry(s);
    }' \
  '    /* 变异 M-M11-08：不唤醒 sendq */' \
  -p rgoc-driver --test go_chan t_chan_07

# ── M-M11-16：`rt_recv` 的「已关闭且空」返回 1 而不是 0 ──────────────────
# `ok` 恒为真 ⇒ `range c` 在 close 之后**永不退出**（挂死）；`v, ok := <-c`
# 也恒为 true。打 `t_chan_05` 而不是 `t_chan_09`：后者会**挂死**，而 cargo
# test 没有超时兜底（`run-native-chan.sh` 才有）—— 用输出断言就能抓到的
# 形态，不要用「靠超时」的形态。
$PROBE "M-M11-16 关闭且空时 ok 恒为真" "$RT" \
  '    if (c->closed != 0) {
        if (es > 0) {
            rt_memset(out, 0, es);
        }
        return 2;
    }' \
  '    if (c->closed != 0) {
        if (es > 0) {
            rt_memset(out, 0, es);
        }
        return 1;
    }' \
  -p rgoc-driver --test go_chan t_chan_05

# ── M-M11-09：`select` 阶段②**逆序**试探（违反源码顺序）────────────────
# `C11` §4 / `D-M8-3`：多就绪时取**源码顺序第一个**。逆序 ⇒ 取最后一个。
# `t_sel_03` 的两个通道都预置了值，所以它必然红（输出 2 而不是 1）。
$PROBE "M-M11-09 select 阶段②逆序试探" "$RT" \
  '        for (u64 i = 0; i < w->n; i++) {
            SelCase *k = &w->cases[i];
            if (k->c == 0) {
                continue; /* nil 通道永远不就绪 */
            }' \
  '        for (u64 i = w->n - 1; i < w->n; i--) {
            SelCase *k = &w->cases[i];
            if (k->c == 0) {
                continue; /* nil 通道永远不就绪 */
            }' \
  -p rgoc-driver --test go_select t_sel_03

# ⚠️ `M-M11-10`（`select` 不撤销其余登记）**没有落地**：变异之后
# `t_sel_04` 的形态是「主 G 被重复入队」⇒ **挂死**而不是干净失败，而
# `cargo test` 没有超时兜底（`run-native-chan.sh` 才有）。按「能用输出断言
# 抓的形态，不要用靠超时的形态」这条口径，宁可不加。
# 撤销本身由 `t_sel_04` 正面覆盖（它查 `a` 上的第二次发送是否真的交付）。

# ── M-M11-06：就绪队列改成**队头插入**（LIFO）─────────────────────────
# `C11` §2.1：就绪队列 FIFO。队头插入 ⇒ 后创建的 G 先跑。
# `t_go_01` 的调度自测建 A、B 两个 G（A 先入队），FIFO ⇒ `A\nB\n`；
# LIFO ⇒ `B\nA\n`。
$PROBE "M-M11-06 就绪队列队头插入" "$RT" \
  '    if (g_ready_tail == 0) {
        g_ready_head = g;
        g_ready_tail = g;
    } else {
        g_ready_tail->qnext = g;
        g_ready_tail = g;
    }' \
  '    g->qnext = g_ready_head;
    g_ready_head = g;
    if (g_ready_tail == 0) {
        g_ready_tail = g;
    }' \
  -p rgoc-driver --test go_sched t_go_01

# ── M-M11-11：死锁判定**无条件**触发（漏「有就绪 G」这一条）────────────
# `rt_schedule` 只在队列空时才报死锁。改成无条件 ⇒ 第一次换 G 就退出 2。
# `t_go_01`（调度骨架）立刻红。
$PROBE "M-M11-11 死锁判定无条件触发" "$RT" \
  '    GDesc *next = rt_dequeue();
    if (next == 0) {
        rt_fatal_deadlock();
    }' \
  '    GDesc *next = rt_dequeue();
    rt_fatal_deadlock();' \
  -p rgoc-driver --test go_sched t_go_01

# ── M-M11-12：`go` 之后**立刻切走**（等价于「main 返回后还在跑别的 G」）──
# `C11` §2.1 的「不抢占」：`rt_go` 只入队，不切走当前 G。
# 加了 `rt_schedule()` ⇒ `go f()` 立刻跑 f。`t_exit_02` 的 fixture 是
# 「`go func(){println("X")}()` + main 直接返回」，正常输出空；变异后打印 X。
$PROBE "M-M11-12 go 之后立刻切走" "$RT" \
  '    /* 入队**队尾**，不切走当前 G（`C11` §2.1 的「不抢占」）。 */
    rt_enqueue(g);
}' \
  '    /* 变异 M-M11-12：入队后立刻切走 */
    rt_enqueue(g);
    rt_schedule();
}' \
  -p rgoc-driver --test go_exit t_exit_02

echo "=== mutation-m11: RED $red / GREEN $green / PENDING $pending / MISS $miss（$MODE）"
[ -n "$report" ] && echo "$report"

# 判定：GREEN 与 MISS 一律算失手；PENDING 只在 phase0 模式允许。
if [ "$green" -ne 0 ] || [ "$miss" -ne 0 ]; then
  exit 1
fi
if [ "$STRICT" -eq 1 ] && [ "$pending" -ne 0 ]; then
  exit 1
fi
exit 0
