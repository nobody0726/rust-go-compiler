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

| 声明形态 | 遍历 |
|---|---|
| `type D <T>` | **先**给 `D` 本身编一个 `named`/`alias` 条目的号（**先占位**），**再**后序遍历 `<T>` |
| `var` / `const` / `func`（含方法） | 对声明里的**类型表达式**做后序遍历（先子后父） |

**后序遍历**：对类型表达式 `*E` / `[]E` / `[N]E` / `map[K]V` / `chan E` / `struct{...}` /
`func(...) (...)` / `interface{...}` / `T1 | T2` —— **先**走子表达式，**再**给本节点编号。

**去重**：**同一个类型条目只编号一次**，取**首次出现**的号；再次遇到写同一个号。

**为什么 `type` 声明先占位**：递归类型（`type N struct{ p *N }`）必须能终止，
且官方 checker 也是**先声明后解析 RHS**。这一条同时保证 `named` 的自我引用
（`p *N` 里的 `N`）指向**同一个** `#号`。

**循环安全**：结构型节点**不去重**（每次展开），但结构型的无限展开只能经由 `named`
发生，而 `named` 去重 ⇒ dump 必然有限。

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

- `basic名`：`BasicId` 的**小写无空格拼写**，全集 **26** 个（`T-TY-12` 的 H 侧计数器与之对账）：

  ```
  bool byte complex64 complex128 error float32 float64 int int8 int16 int32 int64
  uint uint8 uint16 uint32 uint64 uintptr rune string unsafe_pointer
  untyped_bool untyped_int untyped_rune untyped_float untyped_complex untyped_string untyped_nil
  invalid
  ```

  （`byte`/`rune` 是 `uint8`/`int32` 的**别名拼写**，官方 `types2` 里是独立 `Basic` 实例，
  故**保留为独立 token**；`unsafe_pointer` 同理对应 `unsafe.Pointer`。）
- `len文本`：数组长度的**十进制**文本；长度不是常量或为 `-1`（`[...]T` 未定）时写 **`-`**。
  非整数长（非法源码）写 **`?`**（配合 `invalid` 条目）。

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
- `interface` 的 `t=1` 时**恰一行** `term`；`t=0` 时**无** `term` 行。
- **约束接口的同一性不进对撞**（`D-M3-13`）：`interface` 的 `term` 行**只在 dump 里出现**，
  用于 `T-TY-17` 的**形状**对撞；`Identical` 判定遇到它按 `U-M3-05` 显式拒绝。

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

---

## 6. 变更记录

| 日期 | 变更 | 触发 |
|---|---|---|
| 2026-10-16 | 新建并冻结（`Task 1`）；把 `M3-tests.md` §5.2 的 8 条规则抽成独立契约，并**补出** §3 编号顺序（原冻结清单未规定，两侧不可能自洽） | M3 开工 |
