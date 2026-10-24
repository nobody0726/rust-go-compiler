# C9 契约：值表示 / 输出字节 / 退出码

> **契约级别：正式**（M5 冻结候选；`M9` 的 native 对象模型**不承诺**与本契约同构）
> **消费者**：M5 顺序解释器（`rgoc-interp`）、`rgoc-hir` 的 lowering、`rgoc-driver run`；
> M6+ 的解释器扩展与差分测试装置
> **上游**：`M5-design.md` §3.3（值模型）/ §3.4（帧栈）/ §4（输出与退出码）/ §4.5（三件不承诺）
> **判据**：`M5-tests.md` §4（§4.1 比较口径 / §4.2 字节格式 / §4.3 退出码 / §4.4 不承诺）

> ### ⚠️ 本文件的产出时间与普通契约不同
>
> 本文件按 `M5-plan.md` Task 9 应在 M5 的**第一个**任务产出，实际在**收口期**（2026-10-23）
> 补写 —— 原始执行漏了这一份（`M5-report.md` §5「偏离登记」第 1 条）。
>
> 补写方式是**从实现逐条核对**，不是照设计稿抄：
> 每一条都指到 `rgoc-interp` 的具体函数或 `rgoc-driver/tests/*.rs` 的具体测试。
> 因此本文件的强度比「先写契约后写代码」弱一档 —— 它描述的是**已经发生的事**，
> 而不是**事先约定的边界**。凡是「实现与设计不一致」的地方，本文件记实现，
> 并在该行注明差异（目前 0 处）。

---

## 1. 范围

| 在范围内 | 不在范围内 |
|---|---|
| `Value` 的 13 个变体与它们承载的 Go 语义 | native 内存布局（M9） |
| 四类赋值语义（拷贝 / 共享 / 不可变 / 引用） | GC、并发、`chan` 的运行时行为（`U-M5-01`） |
| `print` / `println` 的字节格式与合并流 | `fmt` 包（M5 不支持 import 标准库） |
| `rgoc-driver run` 的退出码与三条通道 | 官方 panic traceback（`U-M5-03`） |
| 三道上限（帧 / 求值栈 / 步数）的失败形态 | 真地址数值（`U-M5-02`） |

---

## 2. 值表示（`rgoc-interp/src/value.rs`）

```rust
pub type Cell = Rc<RefCell<Value>>;          // 一切可寻址位置的载体

pub enum Value {
    Bool(bool),
    Int(i128),                                // 宽度由静态类型决定，不进值
    Float(f64),                               // float32 每次运算后按 f32 舍入
    Complex(f64, f64),
    Str(Rc<str>),                             // 不可变；赋值只共享句柄
    Array(Rc<Vec<Cell>>),                     // 值语义：赋值 = deep_copy
    Struct(Rc<Vec<Cell>>),                    // 同上；字段下标由 lowering 定死
    Slice(SliceVal),                          // { arr: Rc<Vec<Cell>>, off, len, cap }
    Map(Rc<RefCell<MapObj>>),                 // 引用语义
    Ptr(Cell),
    Func(Rc<Closure>),                        // { func: FuncId, captured: Vec<Cell>, recv: Option<Value> }
    Iface { typ: Option<TypeId>, data: Box<Value> },   // typ == None ⇒ nil 接口
    Nil(NilKind),                             // Ptr / Map / Slice / Func / Chan / Iface
}
```

### 2.1 不变量

| # | 不变量 | 为什么（错了会怎样） | 判据 |
|---|---|---|---|
| `I1` | `Int` 只存 `i128`，**宽度由静态类型给**；每次运算后按宽度掩码（有符号做符号扩展） | 槽里存宽度会让 `int8(127)+1` 与 `int` 的加法走两条路 | `T-EX-07`（回绕）、`T-EX-16`（截断/符号扩展） |
| `I2` | 数组 / 结构体的赋值走 `Value::deep_copy`（新建 `Rc` + 新 `Cell`，递归） | 用 `Rc::clone` 共享 ⇒ `a := arr; a[0]=1` 会改到 `arr`（**最隐蔽的一类**） | `T-EX-11/12`；变异 `T-EX-63` |
| `I3` | `SliceVal` 是四元组 `{arr, off, len, cap}`，`arr` 与切片共享 | 只存 `Vec` ⇒ 失去别名；`append` 超 cap 的脱钩也就无从判 | `T-EX-13`；变异 `T-EX-64` |
| `I4` | `Str` 是 `Rc<str>`（**合法 UTF-8**） | 换字节串表示会牵动 29 处调用点 ⇒ 记为 `U-M5-14`（非 UTF-8 字面量不保真） | `T-EX-10` |
| `I5` | `MapObj` 用 `Vec<(Value, Value)>` 存**插入序**，键相等由 `go_eq` 判 | 用 `HashMap` 就得给 `f64` 造不自反的 `Hash`（`NaN != NaN`） | `T-EX-14`（键相等矩阵 + 插入序） |
| `I6` | `Ptr(Cell)` 的相等 = `Rc::ptr_eq`，**唯一例外**是零尺寸聚合（都指向同一身份） | 零尺寸对象不并 ⇒ `zerosize.go` 的 `p != q` 走错分支 | `T-EX-15`；`go_eq` 的 `is_empty_aggregate` |
| `I7` | `Iface{typ: None}` 是 nil 接口，`Nil(kind)` 是**有类型的 nil**；两者 `is_nil()` 都为真 | 用同一个表示 ⇒ `var i I` 与 `var p *T` 的 `== nil` 混为一谈 | `T-EX-29` |

### 2.2 零值（`zero_of`）

| 类型 | 零值 |
|---|---|
| `bool` / 数值 | `false` / `0` / `0.0` / `(0,0)` |
| `string` | `""` |
| `*T` / `map` / `[]T` / `func` / `chan` | `Nil(对应 kind)` —— **不是**空容器（`var z []int; z == nil` 为 `true`） |
| `interface` | `Iface{typ: None, data: Nil(Iface)}` |
| 数组 / 结构体 | 逐元素递归 `zero_of`（元素各占一个 `Cell`） |

### 2.3 键相等与值相等（`go_eq`）

数值按值、字符串按字节、指针按身份、数组/结构体按字段递归、接口按 `(动态类型, 值)`、
`NaN != NaN`（刻意，`f64` 的 `==` 已如此）、函数按 `Rc` 身份。
**没有实现**：`chan` 的相等（`U-M5-01`）。

---

## 3. 四类赋值语义

| # | 语义 | 实现 | 正向判据 | 反向判据（必须**不**发生） |
|---|---|---|---|---|
| 1 | 数组 / 结构体 **拷贝** | `deep_copy` | `a := arr; a[0]=1` ⇒ `arr[0]` 原值 | `arr[0]` 变 |
| 2 | slice **共享底层** | 四元组复制句柄 | `s2 := s1; s2[0]=1` ⇒ `s1[0]==1` | `s1[0]` 不变 |
| 3 | string **不可变** | `Rc<str>` 只共享 | `s2 := s1; s2 += "x"` ⇒ `s1` 不变 | `s1` 变 |
| 4 | map **引用** | `Rc<RefCell<MapObj>>` 共享 | `m2 := m1; m2[k]=v` ⇒ `m1[k]==v` | 两侧不共享 |

**补充两条**（设计 §3.3 未单列，实现里必须对）：

* 接口**装箱**时复制的是值：`var i I = arr; arr[0]=1` ⇒ `i` 里仍是原值（`T-EX-12` 的接口行）。
* `append` 超 `cap` 后**不再共享**底层（`T-EX-13` 的第 2 组）。

---

## 4. 输出字节格式

### 4.1 合并流

`print` / `println` 的字节与未捕获 panic 的 `panic: <值>` **写进同一个 buffer**，
顺序 = 写入顺序（`F4`、`D-M5-6`）。`rgoc-driver` 把它整块写到 **stdout**；
静态诊断与资源超限原因写 **stderr**（两条通道不混）。

**唯一容差**：`\r\n → \n`（官方 `testdir_test.go:1184` 同款）。其余**逐字节**比较。

### 4.2 `print` / `println` 规则（`interp.rs` 的 `Print|Println` 分支）

| 规则 | 字节 |
|---|---|
| `print` 多实参 | **无分隔**直接拼（`print(1,2)` ⇒ `12`） |
| `println` 多实参 | **单空格**分隔、末尾一个 `\n`、末尾无空格 |
| `println()` | 仅 `\n` |
| 无隐式刷新/缓冲 | 所有字节进同一个 `String`，结束时一次性写出 |

### 4.3 单个操作数的文本（`print::format_operand`）

| 输入（静态类型） | 字节 | 依据 |
|---|---|---|
| `bool` | `true` / `false` | — |
| 整数（各宽度） | 十进制，**宽度按静态类型**（`byte(255)` ⇒ `255`，`int8(-1)` ⇒ `-1`） | `T-EX-24` |
| `rune` / `byte` | 十进制（`'A'` ⇒ `65`）—— **不**打字符 | `F10` |
| 浮点 | `strconv.FormatFloat(f,'g',-1,bits)` 的等价物；`float32` 按 f32 最短往返 | `print::format_float` |
| 复数 | `(re+imi)` / `(re-imi)` | `T-EX-24` |
| `string` | 原样字节（不做 `%` 解释） | — |
| `*T` / `func` / `map` | `0xc000000000`（**占位**，见 §7） | `PTR_TEXT` |
| `[]T`（非 nil） | `[len/cap]0xc000000000` | — |
| nil 指针 / nil map / nil func | `0x0` | `F10` |
| nil 接口 | `(0x0,0x0)` | `F10` |
| nil slice | `[0/0]0x0` | `F10` |
| 数组 / 结构体 | **静态面就被拒**（`illegal types for operand`），运行期不可达 | `print.rs` 模块文档第 1 条 |

### 4.4 冻结的字节点（`T-EX-24` 逐字节断言）

```text
print(1, 2)                    -> 12
println("a", "b")              -> a b\n
println(1000000.0)             -> 1e+06\n
println(0.0001)                -> 0.0001\n
println(0.00001)               -> 1e-05\n
println(1e-7)                  -> 1e-07\n
println(float32(1)/3)          -> 0.33333334\n
println(complex(-1, -2))       -> (-1-2i)\n
println(*int(nil))             -> 0x0\n
println('A') / println(byte(65)) -> 65\n / 65\n
```

浮点的 `%g` 判据：`exp < -4 || exp >= 6` ⇒ 指数形式（`shortest ⇒ eprec = 6`），
指数**至少两位**（`1e-07` 不是 `1e-7`）。

### 4.5 `panic: <值>` 行（未捕获 panic）

合并流末尾追加 `panic: <值>\n`，退出码 **2**。`<值>` 的文本见 `print::panic_text`：
字符串原样、整数十进制、`nil` ⇒ `nil`、接口取动态值。
**不比 traceback**（`U-M5-03`）。

---

## 5. 退出码（`rgoc-driver run`）

| 情形 | 码 | 判据 |
|---|---|---|
| 跑完且 `main` 正常返回 | `0` | `T-EX-49` |
| 静态诊断（`C8` 行）非空 ⇒ **不执行** | `1` | `T-EX-49`；`run_cmd.rs` §④ |
| `HIR-UNSUPPORTED-*` / `HIR-MISSING-TYPE` 等 HIR 拒绝 | `1` | `T-EX-02` |
| 未捕获 panic | `2` | `T-EX-38` |
| 资源超限（帧 / 求值栈 / 步数） | `2` | `T-EX-52` |
| 参数错 / io 错 / 内部错（`run_run` 返回 `Err`） | `2` | `main_entry` |

**1 与 2 的分界**：`1` = **没跑**（静态面就判死），`2` = **跑了但没跑完**。
所以退出码 `1` 时 stdout 里绝不允许出现半截程序输出（`EX-2`「诊断优先」的判据）。

**诊断去重的唯一口径**（`run` 独有）：`check_package_with` 把会话同时交给 lowering
（`D-M5-2`），同一个表达式被反复查询 ⇒ 同一条诊断被重复推入。`run_cmd` 按
`(位置, 码, 消息)` 去重后才打印。**只去重完全相同的三元组**，同码不同消息必须各自成行。

---

## 6. 资源上限

| 上限 | 默认 | 超限形态 |
|---|---|---|
| 帧深 | `1_000` | 退出码 `2` + stderr 稳定消息；宿主不 abort |
| 求值栈深 | `10_000` | 同上 |
| 步数 | `100_000_000`（`--max-steps` 可调） | 同上 |
| 单程序墙钟 | 10 s（装置侧约束，不是代码里的常量） | `T-EX-52` 的判据 |

**这三道是 `D-M5-1` 的证据面**：帧深上限恰好 1000，而 `T-EX-17` 的 `down(998)` 必须通过 ——
两条合起来证明「递归语义不压 Rust 栈」。

---

## 7. 三件不承诺（每条带正向证据）

| # | 不承诺 | 实现现状 | 正向证据 |
|---|---|---|---|
| `U-M5-02` | 地址数值 | 一律打占位 `0xc000000000` | 含地址输出的 fixture 被排除并留理由；探针塞进入选集必红（`T-EX-46`） |
| `U-M5-04` | map 迭代序与官方随机序一致 | 固定**插入序** | `T-EX-14` 只断言集合相等，不断言顺序 |
| `U-M5-03` | panic traceback | 只打 `panic: <值>` | `T-EX-38` 只断言前缀 + 退出码 2 |

> 占位地址是**刻意的**：真地址在官方实现里也不可复现（ASLR），
> 假装一致会让「逐字节」这个判据变成谎话。

---

## 7.5 SSA 求值器共用同一 `Value` 与同一合并流（M6/Task 51 补）

`rgoc-interp/src/ssa_exec.rs`（M6 的 SSA 求值器）**不是**第三套运行时（`D-M6-7`）：

| 项 | 共用方式 | 证据 |
|---|---|---|
| 值表示 | 直接用 `value::Value` / `Cell`；SSA 的 `ValueId` 只是**帧内下标**，不是值模型 | `ssa_exec.rs` 的 `SFrame.values: Vec<Option<Value>>` |
| 零值 | 直接调 `zero_of(tt, typ)`（`Fb::mem_of` 的 `const 0 : *T` 也靠它定语义） | `t_ssa_35_数组与结构体字段` |
| 键相等 | 与解释器同一个 `go_eq`（`map` 键）；M6 的 `map` 本身被白名单拒，这条只在 `clear`/`copy` 的路径上间接用到 | `ssa_reject` 的 `SSA-UNSUPPORTED-MAP` |
| 算术 | 共用 `ops::bin_arith` / `ops::compare` / `interp::apply_unary` / `convert` / `slice_value` / `append_value`（后三个从 `Interp` 的方法提成 `pub(crate)` 自由函数） | `R-33-1` |
| 输出 | **同一个**合并流：`print`/`println` 写同一个 `String`，`panic` 走同一个 `panic: <值>` 渲染 | `t_ssa_34_panic_走合并流且退出码_2` |
| 退出码 | 与 §5 同口径：正常 0 / 未捕获 `panic` 2 / 预算耗尽 2 | `t_ssa_40_*` |

**两条**差异（都是 M6 的实现限制，不是语义差异）：

1. `panic` 在 SSA 里是**终止符**（`Term::Panic`），不是一次调用 —— 这样「panic 之后的语句」
   在图上就是死代码。渲染仍走 §4.5 的同一个函数。
2. `CalleeRef::Indirect`（函数值调用）在运行期报错（`G-M6-06`）；函数值本身已被白名单拒绝，
   所以端到端语料碰不到。

> 这条判据的强度来自**三方差分**（`SSA-4`）：同一个程序 `--via=hir` 与 `--via=ssa`
> 的合并流必须逐字节一致。两套求值器若在「值怎么变成字节」上分了叉，那一条会红。

---

## 8. 变更记录

| 修订 | 日期 | 内容 | 触发 |
|---|---|---|---|
| `C9-1` | 2026-10-23 | 首次产出（收口期补写，从实现逐条核对） | Task 9 原漏产出 |
| `C9-2` | 2026-10-23 | §4.3 明确「地址一律占位」；§4.4 补 `float32` 与指数两位的判据 | `print.rs` 实现与 `T-EX-24` 对齐 |
| `C9-3` | 2026-10-23 | §5 补「`run` 独有的诊断去重」口径 | `run_cmd.rs` §④ 的实测（`F[int](1)` 推 8 次） |
| `C9-4` | 2026-10-24 | 补 §7.5「SSA 求值器共用同一 `Value` 与同一合并流」（M6/Task 51） | `D-M6-7` 的兑现：值模型只有一个 crate；`opt_equiv`/`ssa_diff` 是它的判据 |

**待办**：`U-M5-14`（非 UTF-8 字符串）兑现时要改 `I4` 与 §4.3 的 `string` 行 —— 届时升 `C9` 的
次版本，并把 `literal.go` 从排除清单挪回（`M5-tests.md` §6.4）。
