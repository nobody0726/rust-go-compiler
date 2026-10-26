# C10 契约：SSA 中立 dump 格式

> ## 本契约把「SSA 长什么样」冻结成**可逐字节比对**的文本
>
> M6 的第一条门禁（`SSA-1`）要求 dump **确定性**（同一 IR 三次逐字节相同），
> 第三条（`SSA-3`）要求构建结果**可读可查**。两者都需要一份双方都认的文本格式 ——
> 否则「dump 一样」只能靠人工看一眼。
>
> **实现落点**：`rgoc/crates/rgoc-ssa/src/dump.rs`（唯一实现）。
> **消费者**：M6 的 `T-SSA-13/14/46`、`--dump` 子命令、M7 的调试链路。
>
> **与 `C7` 的关系**：`C7` 是**类型**的中立 dump（`identical` 的对撞面）；
> 本契约是**函数体**的中立 dump（IR 形状的对撞面）。两者都输出类型时不共用语法 ——
> 本契约只写 `t<id>` 引用 + 末尾映射，避免把 `C7` 的 15 个标签复制一份（复制就会漂）。

**状态**：**定稿**（M6 / Task 51：示例已由 `T-SSA-46` 的**文档驱动 golden** 逐字节对账）
**契约级别**：正式（六条规则冻结；示例与实现互为判据 —— 谁改了另一个必须跟着改）
**冻结范围**：行语法、编号顺序、禁止项。**不冻结**：dump 的用途（调试/对撞）、`--dump` 之外的通道

---

## 1. 范围

| 本契约**规定** | 本契约**不规定**（留给谁） |
|---|---|
| 头三行的字段与顺序（§2 规则 1） | IR 的**内存表示**（`rgoc-ssa/src/ir.rs`；字段名见 `M6-design.md` §3.2） |
| 块段与指令行的语法（规则 2–4） | 值的**语义**（求值归 `ssa_exec`；类型判定归 `typing.rs`） |
| 编号顺序与排序键（规则 5） | 支配树 / 活跃性怎么算（归 `dom.rs` / `live.rs`） |
| 禁止项（规则 6：地址 / 时间 / 哈希序） | 类型的**完整**渲染（归 `C7`；本契约只给 `t<id>` 与末尾一行映射） |

---

## 2. 六条格式规则（冻结）

1. **头三行固定**（顺序不可换、字段不可省）：
   ```text
   # SSA v1
   # stage: <ssa|hir>
   # func <name> blocks=<n> values=<n>
   ```
   `stage` 取 `ssa`（dump 的是 SSA）或 `hir`（预留：dump 前一步的 HIR 摘要）。
   `#` 开头的行**不参与比对**（沿 `C7` 的同一口径：`#` 行是给人读的）。
2. **块段**：每块一段，**按 `BlockId` 升序**：
   ```text
   block b<k> params=[v<i>,...] preds=[b<i>,...] succs=[b<i>,...]
   ```
   列表为空写 `[]`（**必须出现**，不许省略字段）；`preds`/`succs` 由 CFG **现算**，不存进 IR。
   **入口块的 `params` 就是函数形参**（`entry` 无前驱，故它们不参与 `V7` 的边实参计数）。
3. **指令行**：两块段内缩进两空格，**块内按程序顺序**：
   ```text
   v<k> = <op> <operands> : t<id>
   ```
   `<op>` 取 `Instr` 变体名的 **kebab-case**（`const` / `copy` / `bin` / `un` / `convert` /
   `addr-of` / `field` / `index` / `slice` / `composite` / `make` / `load` / `store` / `call` /
   M9 起追加的 `funcaddr` / `closure` / `capture` / `defer` / `assert` / `nil-check`）。
   **没有结果的指令不写 `v<k> = `**（`defer v3`、`nil-check v3`）—— 与 `store` 不同的是
   它们连令牌都不产出，写个空箭头会让「这一行定义了什么」变得含混。
   操作数按 IR 字段顺序用单空格分隔；`dsts` 多于一个时写 `v1,v2 = call ...`。
   **可选操作数缺省写 `_`**（`slice` 的 `lo/hi/max`、`make` 的 `cap`）——不是省略、不是空串，
   这样「第几个操作数是什么」只靠位置就能数，不必看上下文。
   **`load`/`store` 的第一个操作数是 mem 令牌**（`r<k> <mem> <addr> ...`，`R-16-1`）。
   **`call` 的调用点带 `...` 时在实参表后写 ` ...`**（`append(s, xs...)`，`R-35-1`）：
   `...` 是**语义**（展开成一串元素 vs 追加一个值），不是排版 —— 不写它，
   `append(s, xs)` 与 `append(s, xs...)` 的 dump 完全一样，而两者的行为不同。
4. **终止符行**：块的最后一行，前缀 `term`：
   ```text
   term goto b<k> (v<i>,...)
   term if v<k> -> b<i> (v..) | b<j> (v..)
   term return (v<i>,...)
   term panic (v<i>)
   term unreachable
   ```
   边上的实参 `(...)` **必须出现**（空也写 `()`）—— 它是 `V7` 的比对面。
5. **编号顺序**：`ValueId` / `BlockId` / `RootId` 一律按**创建顺序**编号，dump 按编号升序输出；
   类型的 `t<id>` 在末尾统一映射：
   ```text
   # type t0 = bool
   # type t1 = int
   ```
   映射行按 `t` 编号升序；每种类型只出现一次。
6. **禁止项**（出现即违约）：
   - **地址**（`0x...`、`Rc::as_ptr` 之类）—— 不可复现；
   - **时间/路径/环境**（`SystemTime`、绝对路径、用户名）；
   - **哈希序**（`HashMap` 的迭代顺序）—— 一切容器用 `BTreeMap`/`Vec`；
   - **未定义值**：`term unreachable` **不带操作数**（它不是「有个洞」，而是「流到此为止」）；
     不可达的块根本不出现在 dump 里（builder 在构建期丢弃）。

---

## 3. 完整示例（三段 + 汇合块参数）

Go 源（等价）：`func f() int { if true { return 1 }; return 0 }`（`v0` 是常量条件的简化前形态）

```text
# SSA v1
# stage: ssa
# func f blocks=3 values=5
block b0 params=[] preds=[] succs=[b1,b2]
  v0 = const true : t0
  v1 = const 0 : t1
  term if v0 -> b1 () | b2 (v1)
block b1 params=[] preds=[b0] succs=[b2]
  v2 = const 1 : t1
  term goto b2 (v2)
block b2 params=[v3] preds=[b0,b1] succs=[]
  v4 = bin add v3 v3 : t1
  term return (v4)
# type t0 = bool
# type t1 = int
```


### 3.2 第二个示例（`panic` 路径 + 根映射）

Go 源（等价）：`func g(p *int) int { if p == nil { panic("nil") }; return *p }`

```text
# SSA v1
# stage: ssa
# func g blocks=3 values=5
block b0 params=[v0] preds=[] succs=[b2,b1]
  v1 = const 0 : t2
  v2 = bin eq v0 v1 : t0
  v3 = const 0 : t2
  term if v2 -> b2 () | b1 ()
block b1 params=[] preds=[b0] succs=[]
  v4 = load r0 v3 v0 : t1
  term return (v4)
block b2 params=[] preds=[b0] succs=[]
  term panic (v1)
# root r0 = heap
# type t0 = bool
# type t1 = int
# type t2 = *int
```

这个例子钉住另外四件事：⑤ `term panic` 是**正常终止符**之一（`recover`/`defer` 才会被拒绝，`panic` 不会）；
⑥ 访存指令带**根**与 **mem 令牌**（`load r0 v3 v0`：根 / mem / 地址）—— 根是显式字段，
   不是从地址反推的；mem 是 `V6` 的比对面，`v3` 在这里只当「根 R0 的初始 mem」
   （所以它的类型是**根的地址类型** `*int` = `t2`，不是 `int`）；
⑦ **`nil` 写成 `const 0 : t<指针>`** —— `ConstVal` 没有 `Nil` 变体，零值由
   `typ` 决定（`R-20-1`）。这是「值 + 静态类型」模型的直接后果：`0` 是 `int` 的零值
   还是 `*int` 的零值，只看类型；
⑧ `succs` 保持**终止符里的顺序**（`if v2 -> b2 () | b1 ()` ⇒ `succs=[b2,b1]`），
   不按块号重排 —— 重排会让「then 走哪边」在 dump 里看不出来。
⑨ `term unreachable` 不带操作数（见下）。

`term unreachable` 的片段（块可达、流到此为止；**不是**死块）：

```text
block b3 params=[] preds=[b0] succs=[]
  v4 = copy v0 : t1
  term unreachable
```

**死块则根本不出现**：builder 在构建期丢弃不可达块，所以 `V8` 只需查「非 entry 块有前驱」，
不需要一个 `dead` 标记（有标记就等于允许两种表示同一件事）。

**这个例子同时钉住四件事**：① 汇合块 `b2` 的 `params=[v3]` 与两条前驱的实参个数一致（`V7`）；
② `b2` 里用 `v3` 而不是 `v2`（块参数才是汇合点的定义）；③ 空实参写 `()`（`b1` 的出边）；
④ `preds`/`succs` 与边互逆（`b1` 的 `preds=[b0]`、`b0` 的 `succs` 含 `b1`）。

---

## 4. 三条硬要求（`T-SSA-13/14/46` 的判据）

| # | 要求 | 怎么验 |
|---|---|---|
| 1 | **确定性**：同一 IR 三次 dump 逐字节相同 | 连续三次 `dump_module`，`assert_eq!` 三份字符串 |
| 2 | **自描述**：不查 IR 也能读懂（块参数/边实参/类型映射齐） | 解析器只依赖本契约的语法即可还原结构（`T-SSA-14` 的解析回环） |
| 3 | **文档与实现一致**：§3 的示例能被实现产出（字段名、`op` 拼写、顺序） | `T-SSA-46`：示例里的每一行都能在真实 dump 里找到同形（Task 51 定点对账） |

---

## 5. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-23 | **骨架创建**（M6/Task 1）：六条规则 + 完整示例 + 三条硬要求 | `M6-design.md` §3.2 / §4.3；`C6`/`C7` 的先例（`#` 头行 + 编号升序 + 禁止不可复现项） |
| 2026-10-24 | **定稿**（M6/Task 51）：§3.2 的示例按**实现回填**三处 —— ① `succs=[b1,b2]` → `[b2,b1]`（`succs` 保终止符顺序，示例自己写反了）；② `v1 = const nil : t2` → `const 0 : t2`（`ConstVal` 没有 `Nil`）；③ `v3 = const 0 : t1` → `: t2`（mem 令牌的类型是根的地址类型）。`T-SSA-46` 的 golden 改成**从本文档读**，三处一次全被抓出 | 文档驱动 golden（`dump.rs::c10_block`）；`R-51-1` |
| 2026-10-25 | **M8 不改**：并发的六个 HIR 节点在 SSA 侧仍走拒绝面（`SSA-UNSUPPORTED-CHAN` / `-FUNC-VALUE`），dump 的行语法、编号顺序、禁止项一字未改。`SsaCode::RangeFunc` 的删除不影响本契约（它是**拒绝码**，不是 dump 语法） | `M8-tests.md` §10 的判据边界；`C4` §9 |
| 2026-10-23 | **`R-16-1`**：规则 3 补两处 —— ① `load`/`store` 的第一个操作数是 **mem 令牌**（`V6` 的比对面；不写它 dump 里就看不见 mem 链）；② 可选操作数缺省写 `_`。§3.2 示例随之加一个 mem 种子值（`values` 4→5）。另明确 **`Effect` 不进 dump**（它是 pass 的输入，不是指令操作数） | `M6-plan.md` Task 16 实现期发现骨架漏了 mem 字段；`T-SSA-46` 的 golden 对账 |
| 2026-10-26 | **M9 补登记**（规则 3）：操作数表加上 M9 已经产出的 6 个 op（`funcaddr` / `closure` / `capture` / `defer` / `assert` / `nil-check`），并写明**无结果的指令不写 `v<k> = `**。原因：M9 的 6 个 op 是逐个追加进实现的，规则 3 的操作数表一直停在 M6 的 14 个 —— 表比实现少，`nil-check` 这种「新形状」（没有 dst）就没有契约可依 | `M9/Phase 5c`：`&*p` 的 nil 检查在 IR 里无处可写（`R-9-21`）；`iri_shape` / `dump` 两侧的穷尽 `match` 是编译期判据 |
