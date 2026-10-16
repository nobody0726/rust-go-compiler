# C7 契约：中性类型 dump 格式

> ## 本契约把「类型 dump 对撞」的比对面**冻结成文**
>
> M3 的验收核心（`T-TY-91`）是 rgoc 的类型表与官方 `go/types` 的类型表
> **逐字节 diff** —— 双方各自把「同一份 fixture 里的类型」渲染成一份**中性 dump**，
> diff 为空即类型表示同构。本契约规定那份 dump 的**格式、编号顺序与规范化规则**。
> 实现两侧：
>
> - Rust：`rgoc-types/src/dump.rs`（Task 16）
> - Go（oracle）：`tools/types-oracle/main.go`（Task 31）
>
> **为什么「中性」**：**不** dump 任何一侧的私有细节（位置、`Named` 的方法集缓存、
> 内部指针、名字表），两侧才能有共同的比对面。类型**名字**一律用**表内序号**代替
> （`M3-tests.md` §5.2），因为两侧的名字表与 `types2` 的 `*TypeName` 身份并不一一对应。

**状态**：**已冻结**（M3 / `Task 1`）　|　**契约级别**：**完整**（格式面）　|　**实现落点**：`rgoc-types/dump.rs` + `tools/types-oracle/main.go`　|　**消费者**：M3（对撞门禁 `TY-1`、`T-TY-91`）、M4+（类型检查调试输出）

---

## 1. 范围

| 本契约**规定** | 本契约**不规定**（留给谁） |
|---|---|
| dump 的行格式、标签集、每个标签的字段语法（§2、§4） | 类型的**内存表示**（`rgoc-types` 各 struct 的字段布局归代码与单测） |
| **编号顺序**与去重规则（§3） | 同一性判定（归 `rgoc-types/identical.rs`，见 `D-M3-1`） |
| 位置/名字/空值的规范化（§2.3） | 诊断的渲染（归 C1 §2.2/§3.2） |
| 与 oracle 的对撞方式（§5） | 常数**值**的十进制文本（归 C7 之外：`rgoc-const` 的 `Const` 渲染，§4.3 只规定接口） |

---

## 2. 七条格式规则（`M3-tests.md` §5.2，冻结）

**行式、UTF-8、LF、无 BOM、文件末尾恰一个换行。**

| # | 规则 | 内容 |
|---|---|---|
| 1 | 注释 / 空行 | 以 `#` 开头的行是注释；空行忽略（**两侧都忽略**，以便 golden 里写说明） |
| 2 | 每行一条 | `<序号>\t<类型 dump>`；`<序号>` 是**声明顺序下标**（从 `0` 起，见 §3）；**恰一个** TAB 分隔 |
| 3 | 缩进 | 嵌套项用 **2 空格**；每个嵌套项**独占一行** |
| 4 | **无位置** | 位置一律**不进** dump（另走诊断通道） |
| 5 | **无名字** | 绝不打印标识符；`named`/`alias` 用 **`#<序号>`** 引用（§2.3） |
| 6 | 字段序 | 按官方结构体的**字段声明序**（§4 逐标签给出） |
| 7 | 空值 | 无该项写 **`-`**；布尔写 `true`/`false`（小写） |

### 2.1 标签集（15 个，与 §3.1 的 `Type` 变体一一对应）

`basic` `named` `alias` `pointer` `slice` `array` `map` `chan` `struct` `signature` `tuple` `interface` `typeparam` `union` `invalid`

- 标签是**行内第一个 token**；其后是**空格分隔**的字段（`0x20`，不是 TAB）。
- **变体数即标签数**：`T-TY-11` 的穷尽性断言同时冻结「15」这个数字；新增变体必须改本表（§6）。

### 2.2 引用语法

类型引用写作 **`<引用>`**，其值由**被引用条目的标签**决定（两侧都先编号、再渲染，故可判定）：

```
<引用> := "#" <序号>     若被引用条目是 named 或 alias
        | <序号>         其余标签
```

**为什么给 `named`/`alias` 加 `#`**：它是 `M3-tests.md` §5.2「`named`/`alias` 用 `#<序号>` 而非名字」的字面实现 ——
`#` 让「这是一个**定义**」在文本里可见，从而 `named` 的**同一性**（两个同名不同定义的
`Named` 必须编号不同）在 diff 中就是一条 `#` 行号的差异。

### 2.3 字符串值（Tag / 方法名 / 字段名）的转义

- 字段名、方法名、结构体 Tag **是源码里的字符串**，按 **Rust `{:?}` 语义**转义并加双引号
  （与 C6 §2.4 同一套 `rustDebugQuote`，两侧同形）：
  `\0`→`\0`、`\n`→`\n`、`\t`→`\t`、`\\`→`\\`、`"`→`\"`、其余 `<0x20` 用 `\u{XX}`。
- **匿名字段**（嵌入）没有声明名 ⇒ 名字位写 `-`（**不是**官方 `Var.Name()` 返回的类型名 —— 那会引入名字）。

---

## 3. 编号顺序（**本契约的第二个冻结面**）

`M3-tests.md` §5.2 只规定了「`<序号>` 是声明顺序下标」，**没规定遍历方式**。
两侧若不按同一顺序编号，dump 必然对不上，故本节把它定死。

**根遍历**：按 fixture 的**顶层声明源码序**逐条走；每条声明内部：

| 声明形态 | 根 |
|---|---|
| `type D <T>` | **`D` 本身**（**定义**，见下） |
| `var` / `const` | 声明里写出的**类型表达式**（`var x []int` 的 `[]int`；初始化式里的类型**不在**范围内） |
| `func` / 方法 | 该函数的**签名**（记在 `Defs[名字]` 上，**不是**在类型表达式表里） |

**`number(T)` 的两条规则**：

1. **去重**：同一个类型条目**只编号一次**，取**首次出现**的号；再次遇到写同一个号。
   - 去重按**条目身份**：**定义型**（`named`/`alias`）按定义身份；**基本型**按种类
     （事实上每个 `BasicKind` 只有一个实例）。
   - ⚠️ **结构型（pointer/slice/array/map/chan/struct/signature/tuple/interface/union）不去重** ——
     `[]byte` 在源码里出现两次就**占两个号**。这不是疏漏：两侧都不做结构规范化
     （`go/types` 不提供），要求「同构即同号」会把一个语义判定偷偷塞进 dump 层。
2. **后序**：**结构型先子后父**；**定义型（`named`/`alias`）先占号、再下钻其本体**
   （`Named` 取 underlying / `Alias` 取 RHS）。
   占号在前是**递归类型终止**的唯一手段（`type N struct{ p *N }`），也**顺带**解决了
   「引用一个未在本 fixture 里声明的定义」——如 universe 的 `error`（`Named`，本体是
   `interface{ Error() string }`）——它的本体同样会被拉进来编号。

**逐构造的子节点顺序**（两侧必须一致）：

| 构造 | 先编号的子节点（按此序） | 最后编号 |
|---|---|---|
| `pointer` / `slice` / `chan` | elem | 自身 |
| `array` | elem（**长度不是类型条目**，只在行里以十进制文本出现） | 自身 |
| `map` | key、value | 自身 |
| `struct` | 各字段类型（**字段声明序**） | 自身 |
| `tuple` | 各元素类型（**参数声明序**，含无名参数） | 自身 |
| `signature` | recv（若有）、params 的**各元素类型**、params 元组、results 的**各元素类型**、results 元组 | 自身 |
| `interface` | 各方法的签名、项集各 term 的类型、项集（`union`） | 自身 |
| `union` | 各 term 的类型 | 自身 |
| `typeparam` | 约束 | 自身 |
| `named` / `alias` | （见规则 2：本体在**自身之后**） | 自身（**先**） |

**循环安全**：结构型不去重且可能自引用，但自引用只能经由 `named` 发生，
而 `named` 先占号且去重 ⇒ dump 必然有限。

---

## 4. 逐标签行语法

`<ref>` 按 §2.2。缩进按 §2.3 的 2 空格（嵌套层级 `2 * depth`）。

### 4.1 基础与复合

| 标签 | 行 | 嵌套项（depth+1，2 空格） |
|---|---|---|
| `basic` | `basic <basic名>` | 无 |
| `invalid` | `invalid` | 无 |
| `pointer` | `pointer <ref(elem)>` | 无 |
| `slice` | `slice <ref(elem)>` | 无 |
| `array` | `array <len文本> <ref(elem)>` | 无 |
| `map` | `map <ref(key)> <ref(value)>` | 无 |
| `chan` | `chan <dir> <ref(elem)>`，`dir ∈ sendrecv\|sendonly\|recvonly` | 无 |

- `basic名`：**25 个**，与 `go/types` 的 `BasicKind` **一一对应**（去掉 `Invalid` —— 它走 `invalid` 标签）；
  拼写为小写无空格形：

  ```text
  bool int int8 int16 int32 int64 uint uint8 uint16 uint32 uint64 uintptr
  float32 float64 complex64 complex128 string unsafe_pointer
  untyped_bool untyped_int untyped_rune untyped_float untyped_complex untyped_string untyped_nil
  ```

  ⚠️ **实测订正（M3/Task 3）**：本契约初稿把 `byte` / `rune` / `error` 当成独立 token，**是错的**。
  `go/types` 里 `Byte = Uint8`、`Rune = Int32` 是**同一个 `*Basic`**（同一指针 ⇒ 同一 Kind），
  所以 `byte` dump 成 `basic uint8`、`rune` dump 成 `basic int32`；而 `error` 是
  **`Named`**（其 underlying 是 `interface{ Error() string }`），走 `named` 标签。
  25 + 1（`invalid` 标签）= 26 个 `BasicKind`，恰好覆盖全集。
  `T-TY-12` 的 H 侧计数器对账的是 **25**。
- `len文本`：数组长度的**十进制**文本；长度未定（`[...]T`）写 **`-`**。
  （`go/types` 直接给 `int64`，未定为 `-1` ⇒ 写 `-`；非整数长是非法源码，不由本契约兜底。）

### 4.2 定义与聚合

| 标签 | 行 | 嵌套项 |
|---|---|---|
| `named` | `named <ref(underlying)>` | 无 |
| `alias` | `alias <ref(rhs)>` | 无 |
| `tuple` | `tuple <n>` | `n` 行：`<name或多-> <ref(typ)>`（按**参数声明序**，**不排序**） |
| `signature` | `signature <ref(recv) 或 -> <ref(params)> <ref(results)> <variadic>` | 无（`params`/`results` 指向 `tuple` 条目） |
| `struct` | `struct <n>` | `n` 行：`<name> <ref(typ)> <tag> <embedded>`（按**字段声明序**） |
| `interface` | `interface <m> <t>` | `m` 行 `method <name> <ref(sig)>`（按**方法名升序**，字节序）＋ 若 `t=1` 一行 `term <ref(union)>` |
| `typeparam` | `typeparam <idx> <ref(constraint)>` | 无（`idx` 是该类型参数在**自身列表**里的下标，从 0 起） |
| `union` | `union <n>` | `n` 行：`term <tilde\|exact> <ref(typ)>`（按**声明序**） |

- `recv` 为 `-` 表示**非方法**（普通函数 / 接口方法）。
- `variadic`：`true`/`false`。
- `tuple` 的 `<name或多->`：无名参数写 `-`。
- `interface` 的 `t` = **有没有项集**（实测：oracle 判据是 `unionOf(x) != nil`，
  即该 interface 的类型集里有没有 term 项 —— **不是** `IsMethodSet()`）。
  `t=1` 时**恰一行** `term`；`t=0` 时**无** `term` 行。
- `interface` 的 `comparable` 标记（`F5`/`F8`）**不出现在 dump 里**：预声明的
  `comparable` 与 `interface{}` 都是 `interface 0 0`。它只在表示里（「约束接口的
  同一性」要判它，见 `D-M3-13`）；而 `comparable` 不能作变量类型 ⇒ dump 的根
  永远到不了它（附带结论：**写对撞样本时不要把它放进变量类型位置**，见 §5.1）。
- **约束接口的同一性不进对撞**（`D-M3-13`）：`interface` 的 `term` 行**只在 dump 里出现**，
  用于 `T-TY-17` 的**形状**对撞；`Identical` 判定遇到它按 `U-M3-05` 显式拒绝。
- ⚠️ **预声明名也可能是 `alias`**（实测，M3/Task 3）：`go/types` 里 `any` 是
  **`Alias`**（`type any = interface{}`）。所以 `G[T any]` 的约束位置上会出现
  `alias` 条目，而**不是** `named`。`byte`/`rune` 则连 `alias` 都不是（见 §4.1）。

### 4.3 数组长度的文本来源

`len文本` 取 `rgoc-const::Const` 的**十进制渲染**（整数）；本契约只规定**形态**：
十进制、无前导 `0`（`0` 本身除外）、无 `+`、无下划线。渲染函数归 `rgoc-const`（Task 5）。

---

## 5. 与 oracle 的对撞（本契约的验证方式）

| 断言 | 载体 |
|---|---|
| rgoc dump == oracle dump（fixture 逐字节） | `tools/types-oracle/run-collide.sh` + `rgoc-driver/tests/collide_dump.rs`（`T-TY-91`） |
| oracle 自身确定性（同输入 3 次 sha256 一致） | `T-TY-95` |
| rgoc 侧 dump 确定性/幂等（同输入两次 sha256 一致） | `rgoc-types/tests/dump_c7.rs` 的 `t_ty_19` |
| 最小对撞目标 | `docs/contracts/testdata/type-dump-min.want`（本契约 §6 的输入见下） |
| 扩大对撞 | `tools/types-oracle/corpus-allowlist.txt`（`T-TY-94`，按 M3 支持面筛选） |

**`type-dump-min` 的输入**（Task 3 放在 `tools/types-oracle/testdata/type-dump-min.go`）：

```go
package p

var x int
var y *int
```

**期望 dump**（= `docs/contracts/testdata/type-dump-min.want`）：

```text
0	basic int
1	pointer 0
```

理由（照 §3 逐条走）：顶层声明 1 是 `var x int` ⇒ 后序 → `int` 得 `0`；
顶层声明 2 是 `var y *int` ⇒ 后序先 `int`（**去重**，仍是 `0`）再 `*int` 得 `1`。

### 5.1 对撞样本必须是**合法程序**（实测约束，Task 15）

`number()` 的后序走访在**非法程序**上会遇到真正的环：

```go
var h interface{ comparable; M() }   // 非法：comparable 与方法的组合
```

`go/types` 仍会造出一个 term 指回该接口自身的**类型集**，走访于是无限下钻
（实测以 `fatal error: stack overflow` 收场，1 GB 栈上限）。oracle 现在会
**响亮 panic** 并给出这条文案，不再崩掉整个进程（`fix(m3-oracle): 环守卫`）。

* 合法程序里匿名类型**不可能**自引用（自引用必须经 `Named`/`Alias`，而这两类
  「先占号再下钻」，环在 `idx` 那一行就被挡掉了）⇒ 该分支只命中坏样本，
  **不改变任何合法样本的编号**（回归证据：`fn.go` 的 dump 逐字节不变）。
* 因此：**对撞 fixture 一律写合法 Go**。要写「错误恢复」类样本时，只允许
  「类型本身仍可表示」的错误（如未定义标识符 ⇒ `invalid`），不允许把
  `comparable` / 含约束的 interface 放进变量类型的位置。

---

## 6. 变更记录

| 日期 | 变更 | 触发 |
|---|---|---|
| 2026-10-16 | 新建并冻结（`Task 1`）；把 `M3-tests.md` §5.2 的 8 条规则抽成独立契约，并**补出** §3 编号顺序（原冻结清单未规定，两侧不可能自洽） | M3 开工 |
| 2026-10-16 | **实测订正（`Task 3`，oracle 落地）**：§4.1 删掉 `byte` / `rune` / `error` 三个 token（`Byte=Uint8`、`Rune=Int32` 同一 `*Basic`；`error` 是 `Named`），basic 全集 **26 → 25**；§3 补「定义型先占号 + 顺带编号本体」「结构型不去重」「逐构造子节点顺序表」三条；§4.2 补「预声明名可能是 `alias`（`any`）」 | `tools/types-oracle` 首次对撞 |
| 2026-10-17 | 新增 §5.1「对撞样本必须是**合法程序**」（环守卫的实测结论：`interface{comparable; M()}` 让走访成环）；§4.2 补记 `interface <m> <t>` 的 `t = 有无项集`（实测 `unionOf(x) != nil`，**不是** `IsMethodSet()`） | `Task 15` 探针 + oracle 环守卫 |
