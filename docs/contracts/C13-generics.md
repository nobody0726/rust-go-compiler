# C13 契约：泛型实例化与反射元数据

> **状态**：**已定稿**（2026-10-29，M12/Task 43）。草案建于 2026-10-28（M12/Task 1），
> 执行期七次折回正文（见 §7 变更记录）；M12 收口时 `G12-1..13` 13/13。
>
> **范围**：泛型在 native 通路的**实例化口径**（命名 / 类型身份 / 检查面规则 /
> 诊断码）与**反射元数据**的形状。它约束 `rgoc-load`、`rgoc-check`、`rgoc-meta`、
> `rgoc-arm64` 与 `rt.c`；**不**约束解释器（解释器走 HIR，而 HIR 以下看不到泛型）。
>
> **与其它契约的关系**：
> - `C5` §10 给 `TypeDesc` 的 **40 字节**布局 —— 本契约**只改 `pad` 一个字段的语义**
>   （§5.1），不增删字段、不改任何偏移。
> - `C7` 给类型 dump —— 本契约的实例在 dump 里以 `Origin[Arg1,Arg2]` 出现（§1.3）。
> - `C8` 给诊断码五件套 —— 本契约**不新增码**（§4）。
> - `C12` §4 给根集合 —— 反射写面不需要写屏障的前提写进 `C12`（`D-M12-7`），本契约只引用。

---

## 1. 实例与命名

### 1.1 什么是实例

**实例** = 一个泛型定义（函数 / 具名类型）与一组**具体**类型实参的组合。

* 实例的**身份**是 `(origin, 实参组)`；实参组按 `identical`（`C7` 的同一性）比较，
  不是按类型 id 比较 —— `[]int` 与 `[]int` 在两个不同的 `TypeId` 上仍算同一实例。
* 同一 `(origin, 实参组)` 在整个编译单元里**有且只有一个**实例（memo）。
* 实参必须是**具体**类型：`F[T]` 的实例化实参不允许再是类型参数（除了嵌套实例化
  的中间态，见 §1.4）。

### 1.2 实例名

```text
实例名 = Origin 的名字 + "·" + 转义(实参1) + "·" + 转义(实参2) …
```

* 分隔符是 **U+00B7（`·`，MIDDLE DOT）**。它不是合法的 Go 标识符字符，因此
  **不可能与用户符号冲突** —— 这是选它的唯一理由（不用 `$`：汇编器与 ELF 都允许，
  但 `$` 在部分 shell/正则里要转义，会把判据写脏）。
* `Origin` 用**源码里的名字**（方法用 `Type·Method`，不含包名 —— 单文件编译单元里
  包名恒定，加了只会让名字变长）。
* **转义规则**（必须有，否则 `[]int` 这类实参没法进汇编符号）：
  保留 `[A-Za-z0-9]` 与 `.`（具名类型的包限定）；其余每个字节写成 `_` + 两位小写
  十六进制。例：`Sum·[]int` → `Sum·_5b_5dint`；`Box·*main.T` → `Box·_2amain.T`。
* **确定性**：同一程序两次编译（清 `target` 后）产出的符号名**逐字节相同**。
  实参的渲染必须走类型表的**规范化**渲染（`C7` 的 dump 口径），不许依赖 `HashMap`
  的迭代顺序。

### 1.3 实例在 dump 里怎么出现

* `types` dump：实例是**普通具名类型**，名字就是 §1.2 的实例名。
* `hir` dump：实例是**普通函数**，名字同上。
* **不出现**「类型参数」这种类型：`--dump=hir` 与 `--dump=types` 里
  `type parameter` 出现 0 次（`T-GEN-05`）。

### 1.4 实例化闭包

* 从**非泛型根**（`main` / `init` / 反射用到的类型）出发，按需实例化，**递归到不动点**。
* 深度上限用 `rgoc-types::instantiate::MAX_INSTANTIATION_DEPTH`（M4 已有，= 50）；
  超限报 `TYP-INVALID-INSTANCE-CYCLE`。
* memo 命中**不增加深度**（自引用的同参链必须收敛）。

---

## 2. 类型身份

| 项 | 规则 |
|---|---|
| 每个实例 | 有自己的 `typedesc`（`C5` §10 的 40 字节结构）|
| `Box[int]` vs `Box[string]` | **不同**类型；`typedesc` 指针不同 |
| 接口断言 / 类型 switch | 比较**`typedesc` 指针**（不是 `hash`：`hash` 只是加速，碰撞不改变语义）|
| 反射的 `String()` | `main.Box[int]`（包名 + 原名 + `[` + 实参 + `]`，实参用 Go 语法）|
| 反射的 `Name()` | `Box[int]`（不带包名）|
| 方法集 | 实例的方法集 = 原方法集按实参替换后的签名；接收者类型是实例 |

⚠️ **`typedesc` 的 `hash` 允许碰撞**：`C5` §10 已经这么定，本契约不改。任何「用 hash
判等」的写法都是 bug。

---

## 3. 检查面规则

### 3.1 显式实例化 `F[int](x)` 的判定顺序

`index_expr` 必须按这个顺序判，**顺序反了会把真索引判成实例化**：

1. base 表达式解析出的对象是**泛型函数 / 泛型具名类型 / 泛型方法**吗？不是 ⇒ 走索引。
2. 是 ⇒ 把 `e.index` 当**类型实参列表**解析（逗号分隔的多个表达式）。
   有任何一个解析不出类型 ⇒ 报 `CHK-NOT-A-TYPE`（`1 is not a type`）。
3. 数量与形参不符 ⇒ `TYP-WRONG-TYPE-ARG-COUNT`。
4. 约束不满足 ⇒ `TYP-INVALID-TYPE-ARG`。
5. 都满足 ⇒ 产出**实例化后的操作数**（函数值 / 类型表达式），并登记实例请求。

⚠️ 「base 是泛型」判据不能用「名字首字母大写」或「有没有类型参数」猜 —— 必须查
符号表里那个 `Object` 的类型参数列表非空。

### 3.2 可赋值

* 形参含类型参数时，**先按当前实例的替换表把形参换成具体类型**，再走既有
  `assignable`。
* `[]int` 可赋给 `[]T`（`T = int`）；`func(int)` 可赋给 `func(T)`。
* 替换表只在**实例的检查上下文**里生效；非泛型代码里 `Type::TypeParam` 仍是
  「不判」（`D-M3-2` 的原口径）。

### 3.3 core type 运算

* `Type::TypeParam` 参与运算时，取其**类型集**的 core type（`rgoc-types::typeset`）：
  所有项共享同一个底层类型 ⇒ 用那个；不共享 ⇒ `CHK-INVALID-OPERATION`。
* `~int | ~float64` 上**没有**共同的 core type ⇒ `+` 要拒（`absdiff.go` 的第 3 行）。

### 3.4 两份实现同口径

类型表达式 → `TypeId` 有**两份**实现：`rgoc-load::typeexpr::resolve_type` 与
`rgoc-check::expr::type_expr`。任何一处新增分支（类型实参解析、实例选择器、
替换表），**两处都要有**，且 `T-GEN-08b` 正反向查。

---

## 4. 诊断码

**不新增码。** 四个已有码的分工：

| 码 | 触发 |
|---|---|
| `TYP-WRONG-TYPE-ARG-COUNT` | 实参个数 ≠ 形参个数（含「一个都没给且推断不出」之外的显式给错）|
| `TYP-INVALID-TYPE-ARG` | 实参不满足约束 |
| `TYP-CANNOT-INFER-TYPE-ARGS` | 推断不出缺失的实参 |
| `TYP-INVALID-INSTANCE-CYCLE` | 实例化链无界增长（深度超 `MAX_INSTANTIATION_DEPTH`）|

**报点**：报在**实例化现场**（`F[int](x)` 的 `[` 位置），不是泛型声明处；消息里带
类型实参的 Go 语法渲染。理由：用户写的是调用点，声明处可能只有一个 `T`。

---

## 5. 反射元数据

### 5.1 `TypeDesc` 的 `pad` 改语义

```text
TypeDesc（40 字节，C5 §10 冻结 —— 本契约一个偏移都不改）
  +0  size       u32
  +4  ptrdata    u32
  +8  hash       u32
 +12  kind       u8
 +13  flags      u8
 +14  pad        u16   ← 本契约：语义改为 meta_idx（0 = 没有反射元数据）
 +16  gcdata     *u8
 +24  elem       *TypeDesc
 +32  methods    *MethodTable
```

* `meta_idx == 0` ⇒ 该类型没有 `ReflectMeta`（反射拿到最小信息：只有 size/kind）。
* 上限 65535 个类型（语料远够）。
* **不新增字段、不移动任何偏移** ⇒ M9 的 `t_desc_*` 判据一条都不用改。

### 5.2 `ReflectMeta`（只读，`.rodata`）

```text
ReflectMeta（72 字节）
  +0   name       *u8     （不含包名，`Box[int]`）
  +8   pkgpath    *u8
  +16  fields     *FieldMeta    （+0 起，连续）
  +24  nfields    u32
  +28  nmethods   u32
  +32  methods    *MethodMeta
  +40  params     *ParamMeta    （`Call` 用的 ABI 类）
  +48  nparams    u32
  +52  nresults   u32
  +56  results    *ParamMeta
  +64  tramp      *u8     （§5.5 的 ABI 桥入口；非签名类型恒 0）

FieldMeta（32 字节）
  +0  name *u8 / +8 tag *u8 / +16 typ *TypeDesc / +24 offset u32 / +28 flags u8（anonymous/exported）

MethodMeta（24 字节）
  +0  name *u8 / +8 sig *TypeDesc（签名）/ +16 fn *u8（实例化后的入口）

ParamMeta（16 字节）
  +0  typ *TypeDesc / +8 abi_class u8（0=GPR 1=FPR 2=栈 3=零大小）/ +9 pad
```

`abi_class` 由 **M7 的 `abi_classify` 在编译期算好**写进元数据；**运行时不算**
（`D-M12-5` 的推论：反射不该重新实现一遍 ABI 分类）。

⚠️ **`+64 tramp` 是 Task 33 追加的**：块从 64 → 72 字节。所有消费方都按
**字段偏移**读（Go 侧 `reflectMeta` 镜像、C 侧 `struct ReflectMeta`），
没有一处按步长遍历 —— 表是**指针表**（`rt_reflect_metas`），所以加尾巴
不影响既有读法。`C` 侧的 `_Static_assert(sizeof == 72)` 是这条的判据。

### 5.3 写面与屏障

`Value.Set*` 只做整块拷贝（`CopyBytes` / 直接 `str`）。**不需要写屏障**的前提是
「STW + 单线程」—— 这条写进 `C12`（`D-M12-7`）。上并发 GC 时这条会**静默失效**，
必须先改 `C12`。

「能不能写」由 `Value.writable` 一个布尔决定（官方是 `flagAddr` + `flagRO` 两位）。
只有三条路能造出可写的 `Value`：`New(t).Elem()`、`Index(i)`、以及 `Elem()` 链的
延续；`ValueOf` / `MakeXxx` / `Call` 的结果一律不可写（它们没有地址 —— 与官方一致）。
判定的**顺序与文本**逐字对齐官方 `flag.mustBeAssignableSlow`：先零值
（`call of <method> on zero Value`）、再不可寻址（`<method> using unaddressable value`）。


### 5.4 `TypeDesc` 扩展区（M12/Task 31）

`elem` 槽（+24）只装 `*T` / `[]T` / `[n]T` 的元素（§5.1 的既有语义；map 的
K/V 与 chan 的 elem 走 `isel` 的独立引用，desc 里是 null —— 那是 `C5` §10
的原样）。反射要的 `Key()` / `Elem()` / `Len()` / `ReflectMeta` 指针没有槽可放，
而 40 字节本体冻结 —— 所以**扩展区跟在 desc 块尾部**：

```text
desc 块 = 40 字节本体（§5.1）之后，按 kind 追加扩展 quad（按 kind 判读）：
  Map   → +40 key *TypeDesc, +48 elem *TypeDesc, [+56 meta]（named 时）
  Chan  → +40 elem *TypeDesc, [+48 meta]（named 时）
  Array → +40 len u64, +48 elem *TypeDesc（匿名）… 或 +48 meta（named 时；
          named array 的 elem 走本体 +24？—— 不，Named 的 +24 恒 null，
          见下「规则」），[+56 meta]
  Named | Struct
        → meta *ReflectMeta（§5.2 的块符号）；named 聚合另有 elem/key len
          排在 meta 之前（见「规则」）
  其余  → 无扩展（named 基本类型只有 meta 一个 quad）
```

**规则**：扩展区镜像 **underlying 的形状**；`meta` 指针（`meta_idx != 0` 的类型）
固定**收尾**。`Named` 的本体 `+24 elem` 恒为 null（`C5` §10 的 `elem_of` 只认
表面形状，不改）—— 所以 named 聚合的 elem/key 全在扩展区：

| 表面类型 | 扩展区 |
|---|---|
| map（匿名/named） | key, elem, [meta] |
| chan | elem, [meta] |
| array | len, elem, [meta]（named）/ len（匿名 —— elem 在本体 +24）|
| named ptr / named slice | elem, meta |
| struct（匿名/named）| meta |
| named 基本类型 / named func / named iface | meta |
| 匿名 ptr/slice/func/iface/标量 | 无扩展 |

消费者必须**先判 kind 再读扩展**（`ext` 字段只在对应 kind 的分支里触碰）；
`meta_idx != 0` ⇒ 是 named ⇒ elem/key 在扩展区；`== 0` ⇒ 匿名 ⇒ elem 在本体
`+24`（struct 的 meta 恒存在，`meta_idx` 判定对 struct 不适用）。

* **本体消费方零改动**：`rt` 侧按 40 字节读 `TypeDesc`，扩展 quad 看不见；
  `elem_of` 的 Map/Chan 分支**不改**（+24 照旧 null）。
* **M9 的 `t_desc_*` 判据零改动**：它们只读 header + 三根 `.quad`
  （gcdata/elem/methods），不检查块尾后面还有什么。
* `Array.len` 与 `size / elem.size` 恒相等（数组无内部填充）—— 两条来源
  互相校验，发射侧用 `layout::array_len_of`（**长度怎么读的唯一口径**）。

### 5.5 `Call` 的 ABI 桥（M12/Task 33）

`Value.Call` 要调一个**运行期才知道**的函数指针，而「指针 → 函数值」的转换
Go 源码表达不了 ⇒ 需要一段编译器生成的桩 + 一个 C 入口：

```text
void __reflecttramp_<sig_hash>(void *funcval, const void *args, void *results)   ← 发射侧生成
void rt_reflect_call(void *funcval, void *tramp, void *args, void *results)      ← rt.c
void rt_reflect_call(fn, tramp, args, results)                                   ← Go 侧内建
```

**契约**（发射侧 `emit_reflect_trampoline` 与 `reflect.go::Value.Call` 逐条对齐）：

1. `funcval` 是**闭包块**的地址（`{code, ctx…}`）—— 与 `CalleeRef::Indirect`
   的 `ldr x16, [x0]` 同口径；它同时就是 `x0` 里的隐式 ctx（`ctx_abi` 的
   约定：**凡是当过值的函数**都带 ctx 形参）⇒ 真形参从 **`x1`** 起分配，
   落位是 `place_args([ctx, 形参…])` 的**下标 1 起**。
2. `args` / `results` 是**缓冲**：第 i 个形参在 `off_i = Σ max(8, 槽宽)`。
   两侧同一个公式（发射侧 `buf_slot` / Go 侧 `slotBytes`）—— 一旦分叉，
   参数整体错位。
3. 结果寄存器 `x0..x7` / `d0..d7` **先整体落帧**再分发：聚合结果 > 16 字节
   时被调方返回的是**堆地址**，分发要 `rt_memcpy`（clobber `x0/x1/x2`）。
4. 结果的寄存器计数**逐类独立**（与 `isel::take_results` 同一张表）。
5. **指针类实参不拷内容**：`box_iface` 对 `Type::Pointer` 有 `is_ptr` 特例
   （不装箱，`any` 的 data 字就是指针本身）⇒ `Value.ptr` 指**目标对象**而不是
   「装着指针的槽」。照 `size` 拷会拷成被指向的内容（实测把 `5` 当地址
   解引用 ⇒ SIGSEGV）。结果侧同口径：指针类结果直接取那一个字。

**为什么不在 C 里做通用打包**：寄存器落位在编译期就定了（`D-M12-5`），
通用打包要把「寄存器号」变成数据、再绕回 `blr` —— 同一件事做两遍。

**溢出到栈**：trampoline 的栈参数写在 `[sp, #off]`（自己的帧底），保存区
在 `stackbytes` 之上 —— 被调方从 `fp+16` 读到的正是这块。

**`rt_reflect_call` 是内建**（`BuiltinKind::ReflectCall`），名字进 universe
（`BUILTIN_NAMES`），`check` 钉住「四个实参都是 8 字节」—— 它们直接进
`x0..x3`。⚠️ 它是**运行期钩子、不是语言特性**：`reflect` 之外的代码用它
没有意义，解释器通路直接报 `G-M12-05`。

### 5.6 写面的分配入口（M12/Task 34）

`reflect.New` / `MakeSlice` / `MakeMap` / `MakeChan` 都要「**按运行期拿到的
desc** 分配」，而 `new` / `make` 的 lowering 只服务**静态类型** —— Go 源码写不出
这件事。四个运行期入口合成一个符号，用 `op` 分派：

```text
void rt_reflect_alloc(i64 op, void **out, const TypeDesc *a, const TypeDesc *b, i64 c)

op = 0  new        a = desc                 ⇒ rt_alloc(a, 1)        （零值对象）
op = 1  元素数组    a = elem, c = 元素个数    ⇒ rt_alloc_array(a, c)  （底层数组）
op = 2  map        a = key, b = elem         ⇒ rt_map_make(a, b)
op = 3  chan       a = elem, c = 容量         ⇒ rt_makechan(&ch, c, a)
```

**契约**：

1. **结果写进 `out`，不返回**。`uintptr → unsafe.Pointer` 这个方向在本仓今天的
   代码生成上有缺口（§5.5 第 5 条的邻居问题）⇒ 让 C 写进一个指针槽，Go 侧
   只做 `*T → unsafe.Pointer`。
2. `op` 全是 Go 侧的常量（`allocNew` / `allocArray` / `allocMap` / `allocChan`），
   两侧必须同序；未知 `op` 给 `null`（上层炸在解引用上，好过静默造错对象）。
3. **分配后立刻发布**：调用方拿到指针就写进 `Value.ptr`，中途不再分配 ⇒
   没有「已分配但还没发布」的窗口（`R-10-17` 那一类）。
4. `op = 1` 的三个短路与 `rt_makeslice` **同序**：`c <= 0` ⇒ 不分配；元素零大小
   ⇒ `rt_zerobase`（`&s[0]` 才能取地址）；其余走 `rt_alloc_array`。
5. `MakeSlice` 的**切片头自己也要在托管堆上**：`Value` 只存裸指针，头落栈上
   返回即悬空。按切片类型自己的 desc `rt_alloc` 24 字节 —— 那块对象的 gcdata
   恰好覆盖第 0 字（data 指针），所以底层数组有根。
6. `Map` / `Chan` 的 `Value.ptr` 指向**装着那个指针的槽**（与 `ValueOf(map)`
   同口径：`box_iface` 只对 `Type::Pointer` 走 `is_ptr` 特例）⇒ 造容器时多分配
   一块 8 字节的槽。`Len()` / `Cap()` 因此要先解一层再读 `MapObj.count`（+0）/
   `Hchan.qcount`（+0）/ `Hchan.dataqsiz`（+8）。
7. **`New` 的 `*T` desc 是现场合成的**（`ptrDesc`）：编译器只为源码里出现过的
   类型发 desc，`reflect.New(TypeOf(x))` 里的 `*T` 完全可能一次都没出现过。
   合成块的 `size = 8` / `kind = Pointer` / `elem = T` / `hash = T.hash ^ 0x9e3779b9`。
   ⚠️ 它与程序里真用到的那块 `*T` desc **不是同一个对象** ⇒ 见 `U-M12-10`。

`rt_reflect_alloc` 同样是内建（`BuiltinKind::ReflectAlloc`），名字进 universe，
`check` 钉住「五个实参都是 8 字节」，`isel` 直接进 `x0..x4`。


---

## 6. 不做面

| ID | 不做 | 登记处 |
|---|---|---|
| `U-M12-01` | stdlib import（`fmt`/`strings`/`strconv`/`sort`/`math`/`errors`/`sync`/`runtime`）| 76 个文件 ⇒ 拒绝面 |
| `U-M12-02` | `reflect.MakeFunc` | `T-REF-01` 的白名单里没有它 |
| `U-M12-03` | 可变参数 `Call`（`CallSlice`、`...any` 实参）| `T-REF-03` 的分档里没有它 |
| `U-M12-04` | PGO / DWARF / coverage | roadmap 已划延期 |
| `U-M12-05` | 泛型**原体**的独立检查（不可达实例的类型错误不报）| `D-M12-3` 的副作用 |
| `U-M12-06` | 并发 GC / 写屏障 | 沿用 `U-M11-08` |
| `U-M12-07` | `.dir` 目录测试（相对包 import）| 67 个桩 + 69 个目录 |
| `U-M12-08` | `errorcheck -m`（内联诊断）| 2 个文件 |
| `U-M12-09` | native 接口断言 / itab 的输出级老缺口（typeswitch3/4/5 …）| 合并流口径暴露，白名单冻在装置里（非回归）|
| `U-M12-10` | 局部泛型类型的实例化（函数体内 `type V U[int]`）| 局部声明不挂 `type_params`（Ruling 24-2）|
| `U-M12-11` | 结构体字段的**只读层**（官方 `flagRO`）与 `Value.Field(i)` 的可写视图 | §5.3：写面只从 `New` / `Index` / `Elem` 造可写值；字段读面今天只读 |
| `U-M12-12` | `New` 合成的 `*T` desc 与程序里的真 desc **同身份** | §5.6 第 7 条：要发射面给每个带 meta 的类型多发一块 `*T` desc + 一张反查表 |

---

## 7. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-28 | 建（§1–§6）| M12/Task 1；`D-M12-2/5/8` |
| 2026-10-28 | §5.4 扩展区（Map/Chan/Array/Named 的反射读面载体）| M12/Task 31；40 字节冻结 + `elem_of` 语义不变的折中 |
| 2026-10-27 | Task 31 落地细则：① `FieldMeta.tag` 存**字面量的值**（不含反引号/引号定界符 —— `check::typeexpr` 原抄 `BasicLit.value` 带定界符，实测 `Tag` 打出 `` `json:"x"` ``，官方是 `json:"x"`；与 `rgoc_load::typeexpr::tag_text` 同口径）；② 匿名 struct 的 `PkgPath()` 返回空串（匿名也有 meta，但 `name` 空 ⇒ 包路径不外漏）；③ `kind` 升官方 27 档（见 `C5` §11.1 变更记录）；④ `type_hash` 按 `TypeId` 记忆化（自引用接口类型曾指数爆炸）+ 别名透明（别名臂直接返回 rhs hash） | M12/Task 31；t_ref_02 对官方 oracle 逐字段 |
| 2026-10-27 | Task 32 落地细则：① `ReflectMeta.methods` 填实（MethodMeta 24B：name / sig / fn）；具名接口方法来自 `InterfaceDef`（`fn=0`），具名具体类型来自 `MethodSet` —— ⚠️ `MethodSet.ty` 是**具名基类型**（`strip_ptr(recv)` 分组），按 `underlying` 找永远落空（实测 NumMethod=0）；值类型的 meta 只收**值接收者**方法（指针接收者的归 `*T`，官方方法集口径；指针性从 `named_def.methods` 的签名 `recv` 读）；② 新增 `__sigdesc_<hash>`（kind=Func 的 40 字节 desc，`size=8`，hash 就是 `sig_hash` 编码）—— `Implements` 运行期比它的 +8；③ `String()` 升级：具名 `pkgpath.name`、复合沿 elem 链（`*T` / `[]T` / `[N]T` / `map[K]V` / `chan T`） | M12/Task 32；t_ref_05 对官方 `go run` 逐行 |
| 2026-10-29 | §5.5 `Call` 的 ABI 桥 + §5.2 加 `+64 tramp`（ReflectMeta 64→72 字节）；`rt_reflect_call` 作为内建（`BuiltinKind::ReflectCall`，进 `BUILTIN_NAMES`，check 钉住「四个 8 字节实参」）。⚠️ 同批修了**跨包具体方法调用**的 lowering（`Cross.methods`，键 = 接收者 TypeId）—— 没有它 `reflect.Value.Call`/`.Int()` 这类「导入类型的具名方法」根本进不了 HIR | M12/Task 33；t_ref_03 对官方 `go run` 九档逐行 |
| 2026-10-29 | §5.6 写面的分配入口（`rt_reflect_alloc` 的 op 分派、结果写 `out`、切片头进托管堆、map/chan 的 box 口径、`*T` desc 现场合成）+ §5.3 补「`writable` 一个布尔 = `flagAddr` + 非只读」与 mustBeAssignable 的顺序；`BUILTIN_NAMES` 19→20、`UNIVERSE_SIZE` 45→46；新登记 `U-M12-09` / `U-M12-10` | M12/Task 34；t_ref_04 对官方 `go run` 十三条逐行 + 两条 panic 文本逐字 |
| 2026-10-29 | **定稿**（M12/Task 43）。三条与 c 线相关的确认：① §1 的**单态化**口径不变 —— 字典替换量完**切断**（收益上界 13 B/程序，`M12-report.md` §11.5），本契约的实例命名与类型身份条款因此不需要为字典留口子；② §2 的类型身份按 `TypeId`，**实例与原型不同条目**（`t_gen_16`），这与 c 线「GC-shape 归并」的将来路线不冲突（归并发生在发码层，不改变类型身份）；③ §5.5 的 `Call` 缓冲公式与实际 `rt_reflect_call` 一致，`C5` §10 的 40 字节 `TypeDesc` 未被本契约改动（只改 `pad` 语义）。 | M12/Task 43；`G12-1..13` 13/13、manifest 对账绿 |
