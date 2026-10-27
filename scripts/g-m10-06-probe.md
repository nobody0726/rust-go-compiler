# `G-M10-06` 调查记录（M11 / Task 0）

> 判据 `T-INV-01..05`（`M11-tests.md` §0.4）。
> 结论：**「真 live-in 位图会让四个语料坏掉」的根因不在位图，在
> `rt_defer_push` 的「分配未发布」窗口。** 修掉那个窗口之后，真 live-in 也能跑绿。
> 日期：2026-10-27。

---

## 0. 一句话结论

`rt_defer_push` 用 `rt_arena_alloc` 分配 `DeferRec`，而 `rt_arena_alloc` 走
`gc_alloc` —— **`gc_alloc` 的 stress 钩子在分配之前跑 GC**。那一刻新记录还没挂上
`g_defer_top`，`mark_defer` 看不到它；而 `Defer` 不是 safepoint，`sp_idx` 还停在
`MakeClosure` 的位点。于是闭包块的**唯一**根就是那张位图。

M10 的位图是 live-out 超集，恰好含 `MakeClosure` 的 dst，所以一直没露。
去掉 dst（真 live-in）⇒ 闭包被回收 ⇒ 地址被下一个闭包复用 ⇒ `rt_deferrun`
调到**别人的**闭包 ⇒ panic，且 `recover()` 救不回来。

---

## 1. `T-INV-01` 复现

改动：`live.rs` 的 safepoint 集合加 `for d in ins.dsts() { set.remove(&d) }`（真 live-in）。
跑法：`RGOC_GC_STRESS=64`，四个语料。

| 语料 | 基线（超集） | 真 live-in | 失败形态 |
|---|---|---|---|
| `defernil.go` | rc=0，无输出 | **rc=2** | stderr 一行 `panic: runtime error: invalid memory address or nil pointer dereference`（72 B）；main 的 defer 闭包**没执行** |
| `nilptr2.go` | rc=0 | **rc=139** | 段错误，stdout/stderr 都空 |
| `method5.go` | rc=0 | **rc=139** | 同上 |
| `print.go` | rc=0，175 B | **rc=139** | stdout 只有 83 B（`(0x0,0x0)` 等前 4 行，**这些是对的** —— 它打的就是 nil 接口/nil map/nil slice）|

⚠️ 四个都坏，与 `M10-manifest.json::deferred_gaps[0].what` 的描述一致。

### 1.1 分类：病灶只在一个指令上

把改动收窄成「**只对 `MakeClosure` 去掉 dst**」，四个语料**全部照旧坏**。
⇒ 病灶是 `MakeClosure`，与 `Convert` / `Bin` 无关。

### 1.2 位图差集（`.stackmap` 对账）

`defernil` 的 `main`（frame=128，3 个位点）：

| 位点 | 基线 `ptrs` | 真 live-in `ptrs` | 差 |
|---|---|---|---|
| `b0:1`（`composite` 切片字面量）| `0009…` | `0009…` | 无 |
| `b0:4`（`closure`）| `6000…` = bit 5/6 → `fp-88`(v4) + `fp-80`(v3) | `4000…` = bit 6 → 只剩 `fp-80` | **少了 v4（闭包）** |
| `b0:6`（`call func#1`）| 全 0 | 全 0 | 无 |

⇒ 位图的差异**只有闭包的 dst**，与 `1.1` 一致。

### 1.3 决定性观察：GC 统计**完全相同**

| | `collections` | `allocs` | `frees` | `live` | `bad_ptr` |
|---|---|---|---|---|---|
| 基线 | 8 | 8 | 4 | 192 | 0 |
| 真 live-in | 8 | 8 | 4 | 192 | 0 |

⚠️ **统计一样，程序一个绿一个坏。** 这条排除了「GC 收错了活对象」这个直觉解释
（至少排除了「用总量能看出来」的那种），也是后面去 trace 的直接原因。

---

## 2. `T-INV-02` 最小化

`defernil.go`（33 行）已是最小，再切成 `probe1.go`（加打印，20 行）：

```go
func main() {
    defer func() { println("A: in main defer"); err := recover(); … }()
    f()
}
func f() { var nilf func(); defer nilf(); x = 1 }
```

真 live-in 下的输出：

```text
B: before f
D: f returning
panic: runtime error: invalid memory address or nil pointer dereference
```

**`A: in main defer` 没打印** ⇒ main 的 defer 闭包**根本没被调用**（不是「调了但
recover 失败」）。而 `GC=0` 与 `GC=on`（非 stress）下同一份产物输出完整、rc=0
⇒ **只有 stress 态坏**。

> ⚠️ 顺手排除一个假线索：`probe2.go`（`c := func(){…}` + churn）在真 live-in 下
> `live=0` 却仍打印两次 `C: closure ran`。看 SSA 才发现**它没有捕获，被优化成
> `FuncAddr`，根本没有 `MakeClosure`** —— 探针打错靶。它证明的是「块被回收后
> 内容可能还没被覆盖，所以『还能跑』是假绿」，不能当机制证据。

---

## 3. `T-INV-03` 三条假设的判定

在 `rt.c` 加临时 trace（`RGOC_G06_TRACE=1`，打 `tag:hex` 到 fd 2），
记录 `push-fp/push-fv`、`run-fp/run-top`、`mark-fv`、`gc-begin/gc-marked`。

真 live-in 的 trace（节选）：

```text
push-fp:fffff10ab8f0          ← main 登记 defer
push-fv:ffff80000070          ← 闭包 A
gc-begin:3
gc-marked:3                   ← ⚠️ 3 号 GC 里**没有** mark-fv
B: before f
gc-begin:4
mark-fv:ffff80000070          ← 4 号才看得到（记录已挂链）
…
push-fp:fffff10ab850          ← f 登记 defer
push-fv:ffff80000070          ← ⚠️ f 的闭包拿到**同一个地址**（A 已被回收复用）
D: f returning
run-fp:fffff10ab850
run-top:fffff10ab850          ← f 的 defer 匹配
run-fp:fffff10ab8f0
run-top:fffff10ab8f0          ← main 的 defer 匹配，但 r->fv 指向的块已是 f 的闭包
gc-begin:8
gc-marked:8                   ← 链已空
panic: runtime error: invalid memory address or nil pointer dereference
```

| 假设 | 判定 | 证据 |
|---|---|---|
| **A** 被调方内部二次 GC | ✅ **证实**（形态比原假设更精确）| `push` 的 trace 在 `gc-begin:3` **之前**、而 3 号 GC 没有 `mark-fv` ⇒ GC 发生在 `rt_defer_push` **内部**，此时记录未挂链。`rt_arena_alloc` 的实现是 `return gc_alloc(size, K_ARENA, 0, size)`，确认走 `gc_alloc` 的 stress 钩子 |
| **B** `sp_idx` 配对偏差 | ❌ 证伪 | trace 里 `sp_idx` 从未参与；`.stackmap` 与 `.s` 的位点序列对得上（§1.2 的差集只有闭包 dst 一处）。若 B 成立，差集里会看到**别的**位点漂移 |
| **C** 槽复用（dst 槽旧值是活对象）| ❌ 证伪（但「地址复用」是真的，属于回收的**后果**）| dst 槽 `fp-88` 在 `MakeClosure` 之前是零页（`main` 的帧由 `_start` 首次使用，`init` 只用到 `-64`）。真正被复用的是**堆地址**：闭包 A 被回收后，f 的闭包拿到同一块 `ffff80000070` |

**原假设 A 的措辞需要修正**：不是「被调方内部再触发一次 GC」，而是
**「登记动作自己的分配触发了 GC，而登记尚未完成」** —— 同一个 `gc_pause` 缺口，
但位置在**调用方**，不在被调方。

---

## 4. 修法与验证

`rt_defer_push` 加 `gc_pause`（`R-10-24`）：

```c
gc_pause++;
DeferRec *r = rt_arena_alloc(sizeof(DeferRec), 8);
r->next = g_defer_top; r->fp = fp; r->fv = fv; g_defer_top = r;
gc_pause--;
```

理由与 `rt_alloc_escaped`（`R-10-23`）同源：**一次分配 + 立即发布**，中间没有别的
分配点 ⇒ 这段时间里不产生垃圾 ⇒ 不收也安全。

| 组合 | `defernil` | `nilptr2` | `method5` | `print` |
|---|---|---|---|---|
| 超集（基线）| rc=0 | rc=0 | rc=0 | rc=0 / 175 B |
| 真 live-in，**无**修法 | rc=2 | rc=139 | rc=139 | rc=139 / 83 B |
| 真 live-in，**有**修法 | rc=0 | rc=0 | rc=0 | rc=0 / 175 B |
| 超集，**有**修法 | rc=0 | rc=0 | rc=0 | rc=0 / 175 B |

修法后的 trace：`push-fv:ffff9a000070` → `gc-begin:3` → **`mark-fv:ffff9a000070`**
（3 号就看到了），f 的闭包拿到**不同**地址 `ffff9a0000e0`。

---

## 5. 结论怎么落地

1. **`rt_defer_push` 的 `gc_pause` 采纳**（`R-10-24`）。它是一个**独立于位图选择**
   的缺口：即使保留超集，M11 的多 G 扫根也会让「地址复用」跨 G 发生。
2. **位图仍然保留 live-out 超集**（`C12` §4.4 不变）。理由从「机制没查清」
   换成「保守优先 + 代价不对等」；「真 live-in 会坏」这一条从
   **位图的属性**降级成**一个已修的 `gc_pause` 缺口**。
3. `G-M10-06` 从 `deferred_gaps` 移到 `closed_gaps`。
4. 新增回归判据（`T-INV-06`）：`defer` + 闭包 + stress，断言两个闭包拿到**不同**
   地址，且 main 的 defer 确实执行（`probe1` 的形状）。

## 6. 对 M11 的影响

- `M11-design.md` §2.5 的三条候选机制表：A 的措辞要按 §3 修正。
- `M11-plan.md` Task 0 的硬门禁**已解除**，Phase 0 可以开工。
- `M11-tests.md` `T-GROOT-06`（多 G 超集扫描不坏）保留 —— 它现在是**回归面**，
  不是「未查清的未知」。
