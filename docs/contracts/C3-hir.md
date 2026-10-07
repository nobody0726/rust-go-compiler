# C3 契约（初稿）：HIR —— 多返回值 / 可寻址性 / 闭包 / 异常传播

> **契约级别：spike 级** —— 从 S1/S2 验证出的形状反推，**不是**完整设计。
>
> ## ⚠️ 本文件里「已验证」与「待验证」是**分节列开的**，不得混读
>
> M0 的三个 spike **只跑了 `println(1 + 2)` 这一个固定 fixture**。
> 凡本文件标「**待 M5 验证**」的，都是**从 Go 规范与 `03` 路线图推导的形状**，
> **不是** M0 的结论。把推测写成结论，正是本契约明令禁止的（`M0-plan.md` T50）。
>
> **反推来源**：`rgoc/crates/rgoc-hir/`（**SPIKE-ONLY，M5 整体替换**）、
> `rgoc/crates/rgoc-spikes/src/{interp,ssa,fixtures}.rs`。
> **M5 替换 `rgoc-hir` 时，本契约随代码一起迁移。**

**状态**：初稿（Phase 4 / T50）　|　**门禁证据**：E6 / E7（T45 / T46）
**消费者**：M5（HIR 与顺序解释器）　|　**SPIKE-ONLY 解除条件**：M5 正式 HIR 落地

---

## 1. 已验证的部分（`SPIKE-ONLY`，M5 会改写）

### 1.1 值表示：常量必须以任意精度存在

**验证来源**：`T-S1-03`（`1<<100` 与 `int64` 极值）

```rust
pub enum Const {
    Big(BigInt),   // 无类型常量：大整数高精度，【被赋给具体类型时】才收敛
    Bool(bool),
    Str(String),
}
```

**机制来源（实测证据）**：go1.27.1 对 `println(1<<100)` 报

```text
cannot use big (untyped int constant 1267650600228229401496703205376)
           as int value in argument to built-in println (overflows)
```

即 **Go 也是「先任意精度、后按类型收敛」**。所以：

| 规则 | 说明 |
|---|---|
| 常量在 HIR 里是 `BigInt`（base 2^32 尾数），**不是 `i64`** | 若一开始就用 `i64`，折叠那一步会**静默丢位**，而 `1 + 2` 仍正确 ⇒ 测试照样绿 |
| 收敛**只**发生在 `BigInt::to_int64()` 一处 | 预算由**一个函数**说了算 |
| 收敛失败返回 `None`，**不静默截断、不饱和、不回绕** | 对应 Go 的 `overflows` |

**为什么 `i128` 不够**（`rgoc-hir` 的设计决策）：`i128` 只能证明「比 i64 宽」，
证明不了「按类型收敛的时机对」；且 M3 的大整数策略要求**预算由一处说了算**。

### 1.2 输出流：必须类型层面区分

**验证来源**：`T-S1-01`（stderr）+ `T-S3-03`（stdout）

```rust
pub enum Stream { Stderr, Stdout }
pub struct Stmt { Print { newline: bool, stream: Stream, args: Vec<Val> } }
```

**实测**：Go 内建 `print` 家族写 **stderr**（`od -c` 逐字节，stdout 长度 0）。
而真实 Go 程序输出 hello 走 **stdout**。

**为什么做成枚举而不是注释**：S1 与 S3 的 fixture 都是「打印一个值」，
若只靠注释区分，早晚有人把它们「统一」掉 —— 而那会**同时打破**
`T-S1-01`（stdout 必须精确为空）与 `T-S3-03`（stdout 必须精确 `hello\n`）中的**一个**。
有测试「两个 fixture 的 stream 必须不同」钉住。

### 1.3 内建调用边界：参数在**进入内建之前**已求值

**验证来源**：`T-S1-01` / `T-S1-02` / `T-S2-01`

`Stmt::Print` 的 `args` 是 `Vec<Val>`（**已求值的值**），不是 `Vec<Expr>`。
边界画在 HIR 层，解释器只负责「按 print 家族语义渲染」。

**推论（待 M5 验证）**：真实 HIR 里**用户函数**的调用不能用这种方式 ——
参数必须在**被调方作用域**里求值（Go 的求值顺序有规定）。
内建的「参数已求值」是**内建特有的简化**，不是通用调用形态。

### 1.4 SSA 侧的形状：唯一定义点 + 使用先于定义

**验证来源**：`T-S2-01`（图里真有 `bin` 指令，求值结果与 S1 逐字节相同）

```rust
pub struct ValueId(usize);              // 新类型 ⇒ 「唯一定义点」由类型系统保证
pub enum Instr { Const(Val), Bin { op, lhs: ValueId, rhs: ValueId } }
pub struct Block { name, instrs, succs, params }
```

- **后序遍历**（先 lower 两侧再建本条）⇒ 「使用先于定义」成为**结构性事实**，
  而不是约定。写成先序就会出现「使用先于定义」。
- `Block::params`（phi 的容身处）与 `Block::succs` **恒空**（直线程序），
  但**字段留着** —— 让 M6 接手时能看到「phi 该挂在哪里」，而不是重新发明。

---

## 2. 待 M5 验证的部分（**本节全部是推导，不是 M0 的结论**）

### 2.1 多返回值（tuple）

**M0 状态**：**完全未涉及**。fixture 里没有任何多返回值。

**已识别的需求**（`S2-NEED-TUPLE`）：Go 的多返回值不是「结构体」而是**语言级 tuple** ——
可被 `a, b := f()` 整体赋值、可被直接传参、还能出现在 range 与赋值左侧。

**待验证的问题**（M5b）：

1. SSA 里每个返回值是**各自一个值**，还是引入 bundle 类型？
   前者会让「调用边界」与「赋值边界」都变复杂。
2. 裸函数（`func() (int, error)`）与命名返回值在 HIR 里是否同一形态？
3. 「零值补齐」规则：返回 fewer 值时，剩余位置填什么？

**M0 的输入**：`ssa_needs.rs` 的 `S2-NEED-TUPLE` 条目（含触发形状 `func two() (int, error)`）。

### 2.2 可寻址性（addressability）

**M0 状态**：**完全未涉及**。S1/S2 的值都是不可变的。

**待验证的问题**（M5b）：

1. HIR 里怎么表达「可取地址」？是否每个表达式节点带一个 `addressable: bool`？
2. `&x` 取的是**变量的存储**还是**值的拷贝**？（`a := 1; p := &a; *p = 2` 后 `a` 是多少）
3. slice 元素、map 值、struct 字段各自的可寻址性规则是否一致？
4. **逃逸**与可寻址性的关系：一个「不可寻址」的表达式如何变成可寻址？

**M0 的输入**：`ssa_needs.rs` 的 `S2-NEED-MEMORY`（memory 模型是本问题在 SSA 侧的投影）。

### 2.3 闭包与捕获

**M0 状态**：**完全未涉及**。

**待验证的问题**（M5d）：

1. 捕获的是**变量**（by reference，Go 语义）还是**值**（by value）？
   → 必须是**变量**，因为 `for` 循环里的闭包捕获的是同一个变量（loopvar 语义）。
2. 捕获变量的**生命周期**归谁管？宿主管理（`03` §M5「宿主管理内存」）还是 runtime？
3. 逃逸的闭包（返回出去的闭包）如何保证变量还活着？

**M0 的输入**：`ssa_needs.rs` 的 `S2-NEED-CALL`（调用边界）与 `S2-NEED-MEMORY`。

### 2.4 异常传播：panic / recover / defer

**M0 状态**：**完全未涉及**，且**明确不涉及**。

**关键提醒**：Go 的 panic/recover **不等同于普通 Rust `Result` 传播**：

| 维度 | Go panic | Rust `Result` |
|---|---|---|
| 能否被调用方忽略 | 能（不 recover 就继续） | 不能（`?` 必须处理） |
| 能否跨函数边界 | 能（沿调用栈向上） | 不能 |
| 能否在 defer 里 recover | 能 | 不适用 |
| 命名返回值的作用 | recover 后可改写它们 | 不适用 |

**待验证的问题**（M5e）：

1. `defer` 的**参数何时求值**？（登记时 vs 执行时 —— Go 是登记时）
2. defer 的执行顺序（LIFO）
3. 嵌套 panic 的行为
4. 命名返回值 + recover 的交互

**M0 的输入**：`ssa_needs.rs` 的 `S2-NEED-VERIFIER`（panic 路径让图不再有静态控制流边）。

### 2.5 unwind 边界

**已验证（S3）**：生成的汇编**没有 CFI 指令**，产物也没有 `.eh_frame` ⇒ **不可 unwind**。
这意味着「panic 沿栈向上传播」在 M0 的后端形态下**无法工作**。
M7 若要支持 unwind，必须**新增** CFI 行号表生成 —— 那是**新增能力**，不是「本来就支持」。

**待验证**：M5 的 panic/recover 若要跨函数边界，与 M7 的 unwind 能力**如何对齐**？
（两者是同一套机制还是两套？`03` §M5 的 defer/panic 与 §M7 的 native 需要对齐。）

---

## 3. M0 留下的差距清单（汇总）

| # | 差距 | 归属 | 阻塞了什么 |
|---|---|---|---|
| 1 | 多返回值 | M5b | 任何返回 tuple 的函数 |
| 2 | 可寻址性 / 逃逸 | M5b | 指针、slice、map |
| 3 | 闭包捕获 | M5d | 任何闭包 |
| 4 | panic/recover/defer | M5e | 任何错误处理路径 |
| 5 | unwind（CFI） | M7 | panic 跨函数传播 |
| 6 | **memory SSA** | M6 | 任何可变状态（`S2-NEED-MEMORY`） |
| 7 | **phi / 支配关系** | M6 | 任何控制流（`S2-NEED-PHI`） |
| 8 | **图 verifier** | M6 | 「图是对的」这一前提（`S2-NEED-VERIFIER`） |
| 9 | 类型标注（当前图里整数与字符串运算**长得一样**） | M6 | 静态错误发现 |

> **第 9 条值得单独强调**：S2 的 `eval_bin` 遇到非整数实参返回 `Err`（**运行时**检查），
> 但图**本身没有类型标注** ⇒「两个 i32 相加」与「一个 i32 加一个 string」在图里同形。
> **M6 的 verifier 第一件要补的就是这个。**

---

## 4. 本契约的验证方式

| 断言 | 载体 |
|---|---|
| 常量任意精度 + 收敛点 | `rgoc-hir/src/value.rs` 单测（`移位_1_左移_100_位_装得下且不可收敛到_i64` 等） |
| 收敛溢出即 `None` | 同上（`i64_极值往返无损`、`常数收敛溢出时给出_none`） |
| 两 fixture 的 stream 必须不同 | `rgoc-spikes/src/fixtures/mod.rs` |
| 后序遍历 ⇒ 使用先于定义 | `rgoc-spikes/tests/s2_ssa_test.rs`（`定义顺序保证_使用先于定义`） |
| 图里真有 `bin` 指令 | 同上（`图里真的有_bin_指令_而不是纯_const`）—— 反向验证 S2 没退化成「搬运求值结果」 |

---

## 5. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-07 | **初稿创建**（T50）。已验证项（§1）与待验证项（§2）**分节列开** | Phase 3 的 S1/S2 + 门禁 E6 |
