# M12 判据 · 泛型 native / 有限反射 / 可选优化

> 设计：[`M12-design.md`](./M12-design.md) / 基线：[`M12-baseline.md`](./M12-baseline.md) /
> 台账：[`M12-plan.md`](./M12-plan.md)。
>
> 编号：`T-GEN-*`（泛型）/ `T-REF-*`（反射）/ `T-OPT-*`（优化）/ `T-M12-*`（装置）。
> **每条判据都必须能红**（§9 的变异是它的反向验证）。

---

## 0. 语料（所有判据的地基）

### 0.1 七份清单（`T-M12-01` 保证自洽）

| 清单 | 个数 | 谁用 |
|---|---|---|
| `typeparam-run.txt` | 71 | `T-GEN-01`（三层一致）|
| `typeparam-compile.txt` | 40 | `T-GEN-02`（编译通过）|
| `typeparam-errorcheck.txt` | 5 | `T-GEN-03`（诊断码对齐）|
| `typeparam-rejected.txt` | 76 | `T-GEN-03`（干净拒绝）|
| `typeparam-errorcheck-m.txt` | 2 | 不做（`U-M12-08`）|
| `typeparam-dir.txt` | 67 | 不做（`U-M12-07`）|
| `typeparam-none.txt` | 5 | 不做 |
| **合计** | **266** | |

**门禁面 = 116**（71 + 40 + 5）。

### 0.2 为什么汇总要分「有输出 / 空输出」两栏

71 个 `// run` 里有相当一部分**不打印任何东西**（`func main() {}`）。在空输出上
「两边都是空」只证明**没崩**，不构成语义证据。所以每份读数必须写成
`有输出 a/A + 空输出 b/B`，且 `a + b == A + B == ok + bad`
（沿 `M11` 的 `T-GDEV-02`：脚本自己断言这个等式）。

### 0.3 三层差分的口径

| 层 | 从哪来 | 覆盖 | 主/副 |
|---|---|---|---|
| **L1** native vs **解释器** | `--via=hir` | 全部 116 | **主判据**（`C11` 冻结语义）|
| **L2** native vs **官方** | `go run`（go1.27.1）| 71 个 `run` | 副判据（证明解释器没偏离 Go）|
| **L3** 解释器 vs 官方 | 同上 | 71 个 `run` | 副判据 |

三样**分开**比：stdout / stderr / 退出码。stderr 只在两边都非空时比（`C8` 管
渲染，判据不测措辞）。

---

## 1. `G12-1/2/3` 语料三门

| ID | 判据 | 断言 | 今天 |
|---|---|---|---|
| `T-GEN-01` | **71 个 `run` 三层一致** | 每个文件：L1 三样相同；L2/L3 三样相同。汇总分两栏（§0.2）| **3/71** |
| `T-GEN-02` | **40 个 `compile` 编译通过** | `build --emit=elf` 退出 0；产物是 aarch64 ELF（`readelf -h` 的 `Machine == AArch64`）| **6/40** |
| `T-GEN-03a` | **76 个拒绝面干净拒绝** | `check` 退出 1 **且诊断码集合 ⊇ {`TYP-UNSUPPORTED-IMPORT-FORM`}** | **76/76** |
| `T-GEN-03b` | **5 个 `errorcheck` 诊断码对齐** | 退出 1 **且首码 ∈ 该文件的目标集合**（下表）| **1/5** |

`T-GEN-03b` 的目标集合（**逐文件登记，改了要人看一眼**）：

| 文件 | 目标集合 | 理由 |
|---|---|---|
| `tparam1.go` | `{TYP-DUPLICATE-DECL}` | `T redeclared` |
| `issue48711.go` | `{TYP-INVALID-INSTANCE-CYCLE}` | `instantiation cycle`（码已存在）|
| `issue51232.go` | `{TYP-WRONG-TYPE-ARG-COUNT}` | `not enough type arguments` |
| `issue51233.go` | `{TYP-WRONG-TYPE-ARG-COUNT}` | 同上 |
| `issue50317.go` | `{SYN-UNEXPECTED-TOKEN}` | 官方在检查面报，我们**解析面**就拒了 ⇒ 阶段不同，登记进 `M12-report.md` 的偏差表 |

⚠️ **不写「stderr 非空」这种弱判据**：`M11` 的 `T-GCORP-02` 已经踩过
「断码变成测渲染格式」。

---

## 2. `G12-4` 单态化完备性

| ID | 判据 | 断言 |
|---|---|---|
| `T-GEN-04` | **实例名确定性** | 同一程序**两次**编译（清 `target` 后）产出的 `.s` **逐字节相同**；且实例名含 `·`（`Sum·int`），不与用户符号冲突（用一个真叫 `Sum_int` 的对照组）|
| `T-GEN-05` | **`.s` 与 HIR dump 无类型参数残留** | `grep -c 'type parameter'` 在 `.s` 与 `--dump=hir` 里都是 **0**；且 `--dump=types` 里不出现未被实例化的 `TypeParam` 作为函数形参类型 |

`T-GEN-05` 的**反例对照**：拿 `issue47708.go`（今天能跑）当阴性对照，断言它的
`.s` 里也没有该串 —— 否则「0 次出现」可能只是 grep 打错靶。

---

## 3. `G12-5` 泛型 × 并发 / GC

| ID | 判据 | 断言 |
|---|---|---|
| `T-GEN-06` | **泛型程序在三态下输出相同** | 门禁里 `chan` 的 2 个 + 新增 2 个 fixture（泛型 + `go` / 泛型 + `chan`）：`off`/`on`/`stress` 三态 stdout + 退出码逐字节相同；`stress` 态断言 `collections > 0`（否则判据恒真，`M11` 的 `t_chan_08` 踩过）|

---

## 4. `G12-6` 不回归

| ID | 判据 | 断言 |
|---|---|---|
| `T-GEN-07` | **既有面零回归** | 69 单线程语料三态各 69/0/0；7 并发语料三态各 7/0/0；`cargo test --workspace` 的 `passed` 不减、`failed == 0` |

⚠️ `passed` 不减要在**同一个 commit 序列内**比：新增判据会让总数上升，所以判据
写成「本阶段开始时的快照（243 suite / 2281 passed）加上本阶段新增的条数」。

---

## 5. `T-GEN-08` 两份实现同口径

| ID | 判据 | 断言 |
|---|---|---|
| `T-GEN-08a` | **同一份源码 `check` 与 `build` 结论相同** | 116 个门禁文件：`check` rc 与 `build` 的**前置 check** 结论一致（要么都过、要么都报错）。今天 **7 个不一致**（`F14`）|
| `T-GEN-08b` | **类型表达式解析两份实现对同一 fixture 同判** | 一个含「实例的字段选择器 + 类型参数」的 fixture：`check` 与 `build` 都通过或都拒绝；另外用源码片段**正反向**查两处（`rgoc-load::typeexpr::resolve_type` 与 `rgoc-check::expr::type_expr`）都处理了 `TypeParam` 替换 |

---

## 6. `G12-7/8/9` 有限反射

| ID | 判据 | 断言 |
|---|---|---|
| `T-REF-01` | **白名单对账** | `stdlib/reflect/reflect.go` 的导出 `func`/`type` 集合 == 测试表里登记的集合（脚本算，两边都从源码读）；`MakeFunc` **不在**集合里（`U-M12-02`）|
| `T-REF-02` | **元数据逐字段** | 对 20 个类型（基本 / 指针 / 切片 / 数组 / map / chan / struct 含匿名字段 / 接口 / 泛型实例）：`Size`/`Align`/`Kind`/`NumField`/`Field(i)` 的 `{Name, Offset, Tag, Anonymous, Type}` 与官方 `go run` 输出**逐字段**相同 |
| `T-REF-03` | **`Call` 的 ABI 分档** | 9 档：0 参数 / 1..8 整型 / 1..8 浮点 / 整浮混合 / 超 8 个（走栈）/ 聚合体按值 / 多返回值 / 零大小参数 / `nil` 参数。每档一个 fixture，输出与解释器一致 |
| `T-REF-04` | **写面生效** | `Set*`/`New`/`MakeSlice`/`MakeMap`/`MakeChan` 各一条；`CanSet` 为假时 panic 文本对齐官方；`Set` 之后被写对象的值可读回 |
| `T-REF-05` | **实例的类型身份** | `TypeOf(Box[int]{}).String() == "main.Box[int]"`、`!= TypeOf(Box[string]{}).String()`；`Implements`/`AssignableTo`/`ConvertibleTo` 在实例与接口之间与官方同判 |
| `T-REF-06` | **反射 × GC** | `reflect.New` 出来的对象在 `stress` 下存活；`Set` 写进去的指针是**根**（写后 churn，再读回仍有效）；断言 `collections > 0` |

---

## 7. `G12-10/11/12/13` 优化

| ID | 判据 | 断言 |
|---|---|---|
| `T-OPT-01` | **内联** | `opt_equiv.rs` 全绿（语义等价）；69 + 116 三态输出不变；`.s` 总行数下降 ≥ 10%（读数进 manifest）|
| `T-OPT-02` | **精细逃逸** | `allocs` 读数下降（同一 fixture 前后对比，读数进 manifest）；三态输出不变；**不逃逸的 `&x` 留在栈上**要有正例（`.s` 里该槽不再 `bl rt_alloc_escaped`）|
| `T-OPT-03` | **SwissTable** | `t_map_*` 全绿；大 map（10 万键）的 probe 读数下降；三态输出不变 |
| `T-OPT-04` | **字典替换**（可切断）| 116 个门禁语料**重跑一遍**全绿；代码体积读数（`.text` 字节数）下降；⚠️ 它是**重写**，不是优化 |

---

## 8. 装置自身（`T-M12-*`）

| ID | 判据 | 断言 |
|---|---|---|
| `T-M12-01` | **七份清单自洽** | `m12-classify.sh --check` 退出 0：七份之和 == 266、两两不相交、每份都从语料**算**出来（脚本里没有硬编码文件名）|
| `T-M12-02` | **装置打的是真靶** | 三层差分装置对一个**已知红**的文件必须报 BAD（用 `M12-baseline.md` 里今天的红样本，如 `absdiff.go`），对一个**已知绿**的必须报 OK（`issue47708.go`）。⚠️ 只查「装置能跑」等于没查 |
| `T-M12-03` | **读数可复现** | 同一档连跑两次，`ok/total` 读数相同；`m12-baseline.sh` 的汇总能复现 `M12-baseline.md` §2 的表（差值必须能解释）|

---

## 9. 变异（每条必须实证变红）

| # | 变异 | 期望红 |
|---|---|---|
| `M-M12-01` | `mono` 只产一层实例（不递归到不动点）| `T-GEN-01` |
| `M-M12-02` | 实例名不带类型实参（`Sum·int` 与 `Sum·string` 撞名）| `T-GEN-04` |
| `M-M12-03` | core type 运算不查类型集 | `T-GEN-01` |
| `M-M12-04` | `ReflectMeta` 的字段偏移错 8 字节 | `T-REF-02` |
| `M-M12-05` | `Call` 的浮点参数塞进 GPR | `T-REF-03` |
| `M-M12-06` | `Set` 不检查 `CanSet` | `T-REF-04` |
| `M-M12-07` | 内联跨 panic 边界 | `T-OPT-01` |
| `M-M12-08` | 逃逸分析把逃逸对象判成不逃逸 | `T-OPT-02` |
| `M-M12-09` | SwissTable 探测序列少一步 | `T-OPT-03` |

**纪律（`M11` 的三条教训，照抄）**：

1. 变异前 `cargo clean -p <crate>`；恢复后 `touch`（`mv` 保留旧 mtime ⇒ cargo
   复用变异版 rlib）。
2. 恢复**不能**用 `git checkout`（未提交的改动会一起丢）—— 用原文副本。
3. 变异绿了先分辨：**判据缺形态** / **探针打错靶** / **夹具没走到被变异的路径**。

---

## 10. 不变式（`M12-design.md` 的硬约束，逐条要有判据）

| # | 不变式 | 判据 |
|---|---|---|
| 1 | HIR 以下看不到泛型（`D-M12-2`）| `T-GEN-05` |
| 2 | 实例名确定性（同一程序两次编译相同）| `T-GEN-04` |
| 3 | 实例的类型身份独立（`Box[int]` ≠ `Box[string]`）| `T-REF-05` |
| 4 | `TypeDesc` 仍是 40 字节、偏移不变（`C5` §10）| M9 的 `t_desc_*` 原样通过（**不许改**）|
| 5 | 写面不需要写屏障的前提是 STW + 单线程 | `T-REF-06` + `C12` 新增的前提段落 |
| 6 | 三条线独立：a 收口不依赖 b/c | `T-GEN-01..07` 全绿时 b/c 可以一行没写 |

---

## 11. 与 M5–M11 的判据边界

| 判据面 | 谁管 |
|---|---|
| `TypeDesc` 的 40 字节布局与 `gcdata` | M9（`C5` §10）—— M12 只把 `pad` 改语义 |
| 根协议 / 帧记录 / `sp_idx` 位图 | M10（`C12` §4）—— M12 不改 |
| 并发语义与多 G 扫根 | M11（`C11` §2–§6）—— M12 只**复用** |
| 退出码与合并流 | M5（`C9` §5） |
| 诊断码字典序与 `ALL` | M6（`C8`）—— M12 新增码要走五件套 |
| pass 的语义等价 | M10 的 `opt_equiv.rs` —— M12 的 c 线**复用**它当唯一强判据 |

---

## 12. 修订流程

1. 新增判据 → 先在 `M12-design.md` 的决策或 `M12-baseline.md` 的归因里找到**理由**。
   找不到理由的判据删掉。
2. 新增「不做」→ 先进 §5 白名单（`U-M12-*`），再在 `C13` 正文留痕。
3. 判据红了先分三类：**实现错** / **判据没牙** / **装置打错靶**（`M11` 三次都是
   第二、三类）。
4. 语料读数（`ok/total`）**只准脚本算**，不准手抄进文档。
