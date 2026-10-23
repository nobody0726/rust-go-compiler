# C3 契约：HIR —— 多返回值 / 可寻址性 / 闭包 / 异常传播

> **契约级别：正式**（M5 冻结候选；`M9` 的 native 对象模型**不承诺**与本契约同构）
>
> ## 本文件的两段历史
>
> | 段 | 内容 | 强度 |
> |---|---|---|
> | §1 | M0 的 S1/S2 实测形状（`println(1 + 2)` 一个 fixture） | **已验证**（证据仍在 `rgoc-hir-spike`） |
> | §2 | M0 留下的**四项待验证** → **M5 已给出答案** | **已定**（2026-10-23，逐条指到实现与判据） |
>
> **M5 做了什么**：`D-M5-4` 把 spike HIR 整体改名 `rgoc-hir-spike`（保住
> `SM-M0-NATIVE-HELLO`），把 `rgoc-hir` 这个名字让给正式实现。于是本契约的 §1
> 仍然成立、但**代码位置变了**；§2 的四项从「推导」升级成「实现 + 测试」。
>
> **反推来源（现状）**：`rgoc/crates/rgoc-hir/`（正式：program/stmt/expr/place/lower/dump/diag）、
> `rgoc/crates/rgoc-hir-spike/`（M0 spike 原件）、`rgoc/crates/rgoc-interp/`（值模型与执行机）。

**状态**：**正式（M5 冻结候选）**（原初稿 Phase 4 / T50；2026-10-23 升正式）
**门禁证据**：E6 / E7（M0）；`EX-1`（HIR 与 lowering 闭环）、`EX-3`（值语义）、`EX-5`（异常）
**消费者**：M5 顺序解释器、M6+ 的后端

---

## 1. M0 已验证的部分（证据迁到 `rgoc-hir-spike`）

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

**推论（M5 已确认，但结论反了一半）**：真实 HIR 里**用户函数**的调用确实不能用这种方式 ——
`Expr::Call { args: Vec<Expr> }` 存的是**未求值的表达式**，由解释器在**调用方帧**里求值
（Go 的求值顺序规定在调用方），求值结果再落进被调方帧的形参槽。

内建的「参数已求值」在正式 HIR 里也**没有保留**：`print` / `println` 同样走
`Call { args: Vec<Expr> }`（`Callee::Builtin`），所以 §1.3 的 `Stmt::Print` 只是
**spike 的简化形态**，不是正式 HIR 的形状。真正的边界是「求值在调用方、绑定在被调方」。

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

## 2. M0 留下的四项「待验证」→ **M5 的答案**（2026-10-23 升为已定）

> 每条给三样：**已定形态**（代码里的形状）、**为什么这么定**（一句话）、**判据**（能变红的测试）。
> 「M0 的输入」保留原样，方便对照当时的推导。

### 2.1 多返回值（tuple）

**M0 状态**：**完全未涉及**（`S2-NEED-TUPLE`）。

**已定形态**：

```rust
// rgoc-hir/src/types.rs
Func { results: Vec<SlotId>, named_results: bool, .. }
Expr::Call { callee, args, variadic, spread: usize }   // spread = 唯一实参是多值调用时展开出的个数
Stmt::Assign { lhs: Vec<Place>, rhs: Vec<Expr>, .. }   // 先全求值再逐个写
```

**三个子问题的答案**：

| 子问题 | 答案 |
|---|---|
| ① SSA 里每个返回值各自一个值，还是 bundle？ | **各自一个值**。HIR 的 `Expr` 是单值的，没有 tuple 类型；多值只以**个数**（`spread`）和**求值栈上的多个槽**存在。理由：引入 bundle 会让「调用边界」与「赋值边界」都要处理一种新的可寻址性 |
| ② 裸函数与命名返回值是否同一形态？ | **同一形态**。差别只有 `named_results: bool` 与结果 slot 是否绑名字；裸 `return` 读的是同一批 slot |
| ③ 「零值补齐」填什么？ | 按签名逐个补零值：lowering 在 `return` 处按 `result_types_of_current()` 把缺的位补齐（`lower.rs` 的 `Return` 分支） |

**判据**：`T-EX-18`（`a, b := f()` / `return f()` 直传 / 单值语境取第一个）、`T-EX-19`（命名返回值与裸 `return`）、
`T-EX-57`（`f(g())` 的实参展开，`simassign.go` 的 `swap(swap(a,b))`）。

### 2.2 可寻址性（addressability）

**M0 状态**：**完全未涉及**（`S2-NEED-MEMORY`）。

**已定形态**：

```rust
// rgoc-hir/src/types.rs —— 「C3 §2.2 的答案」就写在这行注释上
pub enum Place {
    Slot(SlotId),                                   // 局部 / 形参 / 命名返回值
    Global(GlobalId),                               // 包级变量
    Field { base: Box<Place>, index: usize },       // 结构体字段
    Index { base: Box<Expr>, index: Box<Expr> },    // 数组 / slice 元素
    Deref(Box<Expr>),                               // *p
}
```

**四个子问题的答案**：

| 子问题 | 答案 |
|---|---|
| ① 怎么表达「可取地址」？ | **不用 `addressable: bool`**，而是把「可取地址的表达式」单独建成 `Place`。类型系统保证：只有 `Place` 能被取址/赋值，`Expr` 不行 |
| ② `&x` 取存储还是拷贝？ | 取**变量的存储**（`Cell = Rc<RefCell<Value>>` 共享）。`a := 1; p := &a; *p = 2` ⇒ `a == 2` |
| ③ slice 元素 / map 值 / 字段的规则一致吗？ | **不一致，且这是刻意的**：slice 元素可寻址（`Index`），结构体字段可寻址（`Field`），**map 元素不可寻址** —— 静态面就拒（`T-EX-05` 的反向面） |
| ④ 逃逸与可寻址性的关系？ | M5 **没有逃逸分析**：生命周期归宿主（`Rc`），「不可寻址的表达式」不会变成可寻址的。唯一例外是复合字面量取址（`&T{...}`，`C` 族修复，`T-EX-59`） |

**判据**：`T-EX-05`（五形态各一条 + map 元素取址被拒）、`T-EX-15`（`*p = v` 写穿、nil 解引用 panic）、
`T-EX-59`（`&T{...}` 合法、`&f()` 仍拒）。

### 2.3 闭包与捕获

**M0 状态**：**完全未涉及**（`S2-NEED-CALL`）。

**已定形态**：

```rust
// rgoc-interp/src/value.rs
pub struct Closure { func: FuncId, captured: Vec<Cell>, recv: Option<Value> }
```

**三个子问题的答案**：

| 子问题 | 答案 |
|---|---|
| ① 捕获变量还是值？ | **变量**（`Vec<Cell>`，与外层共享同一个 `RefCell`）。所以闭包内的改动外部可见，两个闭包也能共享同一变量 |
| ② 生命周期归谁管？ | **宿主**（`Rc` 引用计数）。没有 GC，也没有 runtime；「变量活着」= 还有 `Cell` 引用 |
| ③ 逃逸的闭包怎么保证变量还活着？ | `Rc` 的引用计数本身保证 —— 闭包被返回时 `captured` 里的 `Cell` 跟着走，帧弹出不影响 |

**补充（1.22 loopvar）**：`for` 循环的每轮迭代**新建**变量（新 `Cell`），
所以 `fs = append(fs, func(){ print(i) })` 打印 `012` 而不是 `333`。
这是**语义要求**，不是实现细节 —— `T-EX-27` 钉住。

**判据**：`T-EX-25`（按变量捕获）、`T-EX-26`（闭包活过帧）、`T-EX-27`（loopvar）、`T-EX-28`（方法值/方法表达式）。

### 2.4 异常传播：panic / recover / defer

**M0 状态**：**完全未涉及**，且**明确不涉及**。

**已定形态**：

```rust
// rgoc-interp/src/interp.rs
Interp { frames: Vec<Frame>, panic_val: Option<Value>, .. }
Frame  { slots: Vec<Cell>, state: FrameState, defers: Vec<Pending>, .. }
```

**四个子问题的答案**：

| 子问题 | 答案 |
|---|---|
| ① `defer` 的参数何时求值？ | **登记时**（`Pending::Defer` 保存**已求值的值**）。所以 `defer f(x); x = 2` ⇒ `f` 收到旧值 |
| ② defer 的执行顺序 | **LIFO**（`defers: Vec<Pending>` 尾部弹出） |
| ③ 嵌套 panic | **覆盖**：`panic_val` 是单槽，展开中再 panic 就替换它，继续展开 |
| ④ 命名返回值 + recover | **可以改写**：命名结果在帧的 `slots` 里，defer 里的闭包通过 `Cell` 改它们；未命名结果没有对应的可写槽 |

**与 Rust `Result` 的区别没变**（§2 原来的四行表格仍然成立）：panic 可被忽略、可跨帧、
可在 defer 里 recover、可改写命名结果 —— 所以 HIR 里**没有** `Result` 形状的返回，
异常是**执行机的状态**（`panic_val` + 展开标记），不是值。

**判据**：`T-EX-34`（时机 + LIFO）、`T-EX-35`（recover 只在直接 defer 里有效）、
`T-EX-36`（命名结果改写）、`T-EX-37`（嵌套覆盖）、`T-EX-38`（未捕获 ⇒ `panic: <值>` + 退出码 2）。
反向证据：变异 `T-EX-65`（defer 改执行时求值）、`T-EX-66`（recover 放宽）都实证变红。

### 2.5 unwind 边界

**已验证（S3）**：M0 生成的汇编**没有 CFI 指令**，产物也没有 `.eh_frame` ⇒ **不可 unwind**。

**M5 的答案**：**解释器不用 unwind**。`unsafe_code = "forbid"`、禁止 `catch_unwind`
（`D-M5-1` §3.7），panic 是**显式帧栈上的状态机**（`panic_val` + `unwinding` 标记 + 逐个跑 defer 再弹帧）。
所以「panic 跨函数边界」在 M5 是**天然成立**的，不依赖 `.eh_frame`。

**与 M7 的关系**：**共享语义，不共享实现**。两边必须一致的是三样 ——
① panic 的值；② defer 链的顺序与时机；③ 命名结果槽的可写性。
M7 若要支持跨帧 panic，必须**新增** CFI 行号表（那是新增能力，不是「本来就支持」）；
它复用本节的语义，但不复用 M5 的 `FrameState`。

---

## 3. M0 留下的差距清单（**M5 收口后的状态**）

| # | 差距 | 归属 | 状态 |
|---|---|---|---|
| 1 | 多返回值 | M5b | ✅ **已关闭**（§2.1；`T-EX-18/19/57`） |
| 2 | 可寻址性 / 逃逸 | M5b | ✅ **已关闭**（§2.2；`Place` 五形态；**逃逸分析不做**，归宿主 `Rc`） |
| 3 | 闭包捕获 | M5d | ✅ **已关闭**（§2.3；`T-EX-25/26/27`） |
| 4 | panic/recover/defer | M5e | ✅ **已关闭**（§2.4；`T-EX-34..38`） |
| 5 | unwind（CFI） | M7 | ⬜ 仍开放（M5 用显式帧栈绕过；§2.5） |
| 6 | **memory SSA** | M6 | ⬜ 仍开放 |
| 7 | **phi / 支配关系** | M6 | ⬜ 仍开放 |
| 8 | **图 verifier** | M6 | ⬜ 仍开放 |
| 9 | 类型标注（当前图里整数与字符串运算**长得一样**） | M6 | ⬜ 仍开放 |

> **第 9 条值得单独强调**：S2 的 `eval_bin` 遇到非整数实参返回 `Err`（**运行时**检查），
> 但图**本身没有类型标注** ⇒「两个 i32 相加」与「一个 i32 加一个 string」在图里同形。
> **M6 的 verifier 第一件要补的就是这个。**
>
> **正式 `rgoc-hir` 的做法**：类型**不进 HIR**，而是每个 `Expr` 带 `typ: TypeId`
> 指回 `TypeTable`（`D-M5-2`：lowering 不自己推类型，边 lower 边向检查器会话要 `Operand`）。
> 所以第 9 条在**解释器这条路上已经绕开**，但它仍是 M6 图 verifier 的活。

---

## 4. 本契约的验证方式

| 断言 | 载体 |
|---|---|
| 常量任意精度 + 收敛点 | `rgoc-hir-spike/src/value.rs` 单测（`移位_1_左移_100_位_装得下且不可收敛到_i64` 等） |
| 收敛溢出即 `None` | 同上（`i64_极值往返无损`、`常数收敛溢出时给出_none`） |
| 两 fixture 的 stream 必须不同 | `rgoc-spikes/src/fixtures/mod.rs` |
| 后序遍历 ⇒ 使用先于定义 | `rgoc-spikes/tests/s2_ssa_test.rs`（`定义顺序保证_使用先于定义`） |
| 图里真有 `bin` 指令 | 同上（`图里真的有_bin_指令_而不是纯_const`）—— 反向验证 S2 没退化成「搬运求值结果」 |
| **§2.1 多返回值** | `rgoc-driver/tests/run_semantics.rs` 的 `T-EX-18/19`；`rgoc-check/tests/multi_value.rs` 的 `T-EX-57` |
| **§2.2 可寻址性** | `rgoc-driver/tests/run_semantics.rs` 的 `T-EX-05/15`；`rgoc-check/tests/addr_composite.rs` 的 `T-EX-59` |
| **§2.3 闭包** | `rgoc-driver/tests/run_semantics.rs` 的 `T-EX-25/26/27/28` |
| **§2.4 异常** | 同上 `T-EX-34..38`；反向证据 `scripts/mutation-m5-notes.md` 的 `T-EX-65/66` |
| **§2.5 unwind** | `rgoc-interp/src/interp.rs` 的 `panic_val` + `unwinding` 状态机；`T-EX-37/38` |

---

## 5. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-07 | **初稿创建**（T50）。已验证项（§1）与待验证项（§2）**分节列开** | Phase 3 的 S1/S2 + 门禁 E6 |
| 2026-10-23 | **升正式**：§2 的四项从「推导」改成「已定」；§1 的证据位置改指 `rgoc-hir-spike`；§3 差距清单前四条标为已关闭、第 9 条补「正式 HIR 用 `typ: TypeId` 绕开」；§4 增 6 行 M5 判据 | `M5-plan.md` Task 52；`M5-report.md` §5 第 6 条（`G-M5-07`） |
