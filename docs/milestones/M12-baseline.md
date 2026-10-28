# M12 基线测绘（Task 0）

> 目的：回答「今天到哪了」与「每个失败归因到哪一类」。`M12-plan.md` 的任务顺序
> 由这张表决定（`D-M12-10`：「没有这张表不排后面的任务顺序」）。
>
> 复现：
> ```bash
> RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work && \
>   bash tools/run-oracle/m12-classify.sh && \
>   bash tools/run-oracle/m12-baseline.sh'
> ```
> 两个脚本都在 `tools/run-oracle/`，读数全部**算**出来（不手抄）。

---

## 1. 语料划分（七份，和为 266）

| 清单 | 个数 | 门禁 | 说明 |
|---|---|---|---|
| `typeparam-run.txt` | **71** | `G12-2` 三层一致 | 无 stdlib import 的 `// run` |
| `typeparam-compile.txt` | **40** | `G12-1` 编译通过 | 无 stdlib import 的 `// compile` |
| `typeparam-errorcheck.txt` | **5** | `G12-3` 诊断码对齐 | 无 stdlib import、无 `-m` |
| `typeparam-errorcheck-m.txt` | 2 | 不做（`U-M12-08`） | 带 `-m`：期望错是**内联报告** |
| `typeparam-rejected.txt` | **76** | `G12-3` 干净拒绝 | 带 stdlib import（任何族） |
| `typeparam-dir.txt` | 67 | 不做（`U-M12-07`） | `// rundir`/`// compiledir` 桩 |
| `typeparam-none.txt` | 5 | 不做 | 没有任何指令 |
| **合计** | **266** | | `T-M12-01` 断言自洽 |

**门禁面 = 116 个文件**（71 + 40 + 5）。

### 1.1 `// rundir` / `// compiledir` 是什么（`F13`）

那 67 个 `.go` 全是 `package ignored`（1 个 `package p`）的**桩**，真正的测试在
同名 `.dir/` 目录里（69 个目录、每个 2–4 个文件）。目录里的测试用**相对包
import**（`import "./a"` 35 处、`import "./b"` 4 处）—— 那是「包解析」，
roadmap 明确划在 M12 之外。所以：**桩不算测试**，也不能进分母。

### 1.2 「无 stdlib import」怎么判（三条踩过的坑）

判据是「**import 集合 ⊆ {unsafe}**」，用能识别注释的小状态机取路径：

1. **块注释里的 import 不算**：`issue50417.go` / `issue50417b.go` 的
   `import "fmt"` 在 `/* … */` 里（官方为留档注释掉了），而这两个**今天就能跑通**。
   用 `grep '^import'` 会把它们判成「需要 stdlib」。
2. **`unsafe` 是支持的伪 import**（M4 的 `U-M4-06`）：`issue53390.go` 只有
   `import "unsafe"`，属门禁面。全语料 5 个文件 import `unsafe`，4 个有指令。
3. **指令要匹配 `^// run( |$)`**：6 个文件带尾随参数（`// run -gcflags="-l"`、
   `// compile -p=p`、`// errorcheck -0 -m`），精确匹配会漏。

---

## 2. 今天到哪了

| family | total | check 过 | build 过 | run 过 |
|---|---|---|---|---|
| run | 71 | 22 | **3** | **3** |
| compile | 40 | 25 | **6** | — |
| errorcheck | 5 | 2（= 该报错却没报） | — | — |
| errorcheck-m | 2 | 1 | 0 | — |
| rejected | 76 | 0（全部干净拒绝 ✓） | — | — |
| dir | 67 | 67 | 67 | — |
| none | 5 | 3 | 3 | — |

**门禁面的今天读数：`run` 3/71、`compile` 6/40、`errorcheck` 1/5（语义对）。**

* 今天 `run` 通过的 3 个：`issue47708.go`、`issue50417.go`、`issue50417b.go`
  （后两个的 `import "fmt"` 在块注释里，是**无泛型**的普通程序）。
* 今天 `compile` 通过的 6 个：`issue46461`、`issue47929`、`issue47966`、
  `issue49611`、`issue49875`、`issue50259`。
* 拒绝面 **76/76 干净拒绝**（退出码 1），且 76/76 的诊断码集合**含**
  `TYP-UNSUPPORTED-IMPORT-FORM`。

---

## 3. 逐条归因

### 3.1 check 面：64 个失败

| 首码 | 个数 | 成因 |
|---|---|---|
| `CHK-INVALID-INDEX` | **47** | **显式实例化 `F[int](x)` 被当成索引**：门禁族里 121 处 `cannot index value of type func`（47 个文件），另有 6 处 `cannot index value of type type parameter`（真·索引 `x[i]`，`x` 是 `[]T`） |
| `CHK-INVALID-ASSIGN` | 11 | 实例化后的可赋值：`cannot use value of type func as func value in argument to F`（把 `F[int]` 当函数值传）/ `[]int as []type parameter` |
| `CHK-INVALID-OPERATION` | 2 | `absdiff.go`、`issue48424.go`：core type 上的 `-`/`<` |
| `CHK-INVALID-BUILTIN-ARG` | 2 | `append.go`、`builtins.go`：`append`/`len` 在类型参数上 |
| `TYP-INVALID-BLANK` | 1 | `issue47710.go` |
| `CHK-INVALID-RANGE-EXPR` | 1 | `issue48453.go` |

**一个根因吃掉 47/64**：`index_expr` 不认「对泛型函数做显式实例化」。这是
M12a 的第一优先级。

### 3.2 build 面：38 个失败（check 已过）

| 形态 | 个数 | 成因 |
|---|---|---|
| `missing_type` | 12 | `HIR-MISSING-TYPE`：`拿不到表达式 X 的静态类型（mode=invalid operand）`——check 静默产出 `invalid` 操作数，HIR 建不起来 |
| `selector` | 11 | `CHK-INVALID-SELECTOR`：实例的字段/方法选择器在 **load 面**没实现（check 面过了） |
| `other` | 7 | **6 个是 `build` 内置的 check 又报错**（`TYP-INVALID-CONVERSION` 1、`CHK-INVALID-OPERATION` 5）而 `check` 子命令 rc=0；**1 个是真正的 clang 链接失败**（`issue52124.go`） |
| `layout` | 6 | `帧布局失败：v0 的类型 t32 没有运行期布局`：SSA 拿到了带类型参数的常量 |
| `verify` | 2 | `VerifyError: 常量 Int(7) 不可能有类型 TypeId(32)`：同上 |

**`other` 那 6 个是一条独立的真错**：`check` 与 `build` 的口径不一致（`F14`）。
`check` 报 0 个诊断，`build` 用同一份代码报出诊断 —— 不是「泛型不支持」，是
**同一条检查在两处的输入不同**。

⚠️ **`F14` 的 7 个是窄口径，真正的账是 29 个**（`T-GEN-08a` 落地后实测）。
窄口径按 `build_kind = other` 数；宽口径问的是「`check` 过而 `build` 报出**任何**
检查面诊断码」—— 那么 `missing_type` 12 + `selector` 11 + `other` 6 = **29**。
`layout` 6 与 `verify` 2 **不在其中**：它们不带 `[...]` 码（是 SSA/isel 的下游
失败），属于另一个面。

宽口径才是 `M12a` 该收敛的那个数 —— 窄口径会让那 23 个文件在「check 绿 / build
红」的分裂里躺着而没人看见。`t_gen_08a` 用的就是宽口径。

### 3.3 errorcheck：5 个的逐条对照

| 文件 | 官方期望 | 今天 | 判定 |
|---|---|---|---|
| `tparam1.go` | `T redeclared` | `TYP-DUPLICATE-DECL` | ✅ 语义对 |
| `issue48711.go` | `instantiation cycle` | `CHK-INVALID-INDEX` | ✗ 码不对（该 `TYP-INVALID-INSTANCE-CYCLE`） |
| `issue50317.go` | `interface method must have no type parameters` | `SYN-UNEXPECTED-TOKEN` | ✗ 码不对（解析面先拒了） |
| `issue51232.go` | `not enough type arguments for type Fn: have 1, want 2` | **无诊断** | ✗ 漏检（该 `TYP-WRONG-TYPE-ARG-COUNT`） |
| `issue51233.go` | 同上 | **无诊断** | ✗ 漏检 |

⇒ **今天 1/5（语义对）**。注意 `TYP-WRONG-TYPE-ARG-COUNT`、`TYP-INVALID-INSTANCE-CYCLE`
**两个码都已经在 `DiagCode` 里**（M4 建的），不需要新增。

⚠️ **两个读数的口径不同，不要混**：装置（`run-native-typeparam.sh --only errorcheck`）
今天报 **2/5** —— 因为它比的是「首码 ∈ 登记的集合」，而 `issue50317.go` 的登记值就是
`SYN-UNEXPECTED-TOKEN`（我们解析面先拒了，官方在检查面拒）。「语义对」只算 1 个，
是因为阶段不同不算对齐。阶段差异登记在 `M12-report.md` 的偏差表里。

### 3.4 不做面的今天读数

* `dir` 67 个桩全部 check/build 通过（它们是 `package ignored`，无内容）。
* `none` 5 个：3 个通过、2 个 check 失败（`issue47896`、`issue48604`）—— 不在任何门禁里，只登记。
* `errorcheck-m` 2 个：`pragma.go`、`issue54497.go`，期望错是 `can inline …`
  （gc 的内联报告），我们没有 `-m`，不做。

---

## 4. 四类归因汇总

| 类 | 个数 | 具体 |
|---|---|---|
| **检查面** | 64 | 显式实例化 47 / 可赋值 11 / core type 2 / 内建 2 / 边角 2 |
| **单态化（load 面）** | 38 | 选择器 11 / `missing_type` 12 / 布局 6 / verify 2 / 其它 7 |
| **反射** | 0 | 69 个门禁文件里 `reflect` 出现 **0** 次（`F7`）—— b 线不阻塞 a 线 |
| **范围外** | 76 + 67 + 2 + 5 | stdlib import 76 / dir 桩 67 / `-m` 2 / 无指令 5 |

---

## 5. 对设计文档的修正（六处）

| # | 设计原文 | 实测 | 性质 |
|---|---|---|---|
| 1 | `F1`：`run` 141 / `compile` 41 / `errorcheck` 6 / 无指令 78 | **`run` 142 / `compile` 44 / `errorcheck` 8 / `rundir` 49 / `compiledir` 18 / 无指令 5** | 指令是**五族**，且 6 个带尾随参数被漏数 |
| 2 | `F2`：门禁 69+37+5 = 111；拒绝 77 | **门禁 71+40+5 = 116；拒绝 76；另有 dir 67** | 块注释 import 2 个 + `unsafe` 伪 import + 尾随参数 |
| 3 | §4.2：五份清单 | **七份**（增 `dir` / `errorcheck-m`） | `dir` 是桩、`-m` 是内联诊断 |
| 4 | §6：新增 `CHK-INSTANTIATION-DEPTH` | **不新增**：`TYP-INVALID-INSTANCE-CYCLE` 与 `TYP-WRONG-TYPE-ARG-COUNT` 都已存在 | 少一个码要维护 |
| 5 | `G12-3`：「拒绝面 + 码稳定」 | 首码只有 **22/76** 是 `TYP-UNSUPPORTED-IMPORT-FORM`；但 **76/76 的码集合含它** | 判据改成「**码集合包含**」而不是「首码」 |
| 6 | — | 新增 `F13`（dir 桩与相对包 import）、`F14`（check/build 口径不一致 7 个） | 新事实 |

**新增两条白名单**：`U-M12-07`（`.dir` 目录测试：相对包 import）、
`U-M12-08`（`errorcheck -m`：内联诊断）。

---

## 6. 对计划的影响

1. **顺序**：检查面（64）→ 单态化（38）。检查面不动，单态化连实例都造不出来
   （`F[int](x)` 在 check 就被判成索引）。
2. **第一优先**：`index_expr` 认显式实例化 —— 一个改动解锁 47 个文件。
3. **独立真错**：`F14`（check/build 口径不一致 6 个 + 1 个 clang 链接失败）
   与泛型无关，但挡着 `compile` 门禁，要单独一条任务。
4. **b 线不阻塞**：门禁 116 个里 `reflect` 0 次，反射可以等 a 线收口再开。

---

## 7. Phase 1 收口后的账（Task 12，2026-10-28）

基线文件：`.superpowers/m12-baseline-t12/baseline.tsv`（复现命令同 §0）。

### 7.1 读数

| family | total | check 过 | Task 0 | build 过 | run 过 |
|---|---|---|---|---|---|
| run | 71 | **62** | 22 | 3 | 3 |
| compile | 40 | **37** | 25 | 6 | — |
| errorcheck | 5 | 2（= 漏报）| 2 | — | — |
| rejected | 76 | 0 = 全部干净拒绝 ✓ | 0 | — | — |

门禁族 116 个里 **check 通过 99**（`62 + 37`，不含 errorcheck 那 2 个「该报没报」）。
`M12-plan.md` 标题写的「64 → 0」**没有达成**，剩 12 个（下表）；它们不是
「Phase 1 漏做」，是**各自的独立缺口**，逐一归了去向。

### 7.2 剩余 12 个 check 失败与去向

| 文件 | 族 | 码 | 卡在哪 | 归 |
|---|---|---|---|---|
| `append.go` | run | `CHK-INVALID-OPERATION` | 类型参数上的 `!=` 要 `Recv` 的具体类型 —— 得先**推断变参调用的实参**（`_Append([]Recv{recv}, recv)`）| Task 20 |
| `dictionaryCapture-noinline.go` | run | `CHK-INVALID-ASSIGN` + `CHK-INVALID-SELECTOR` | 类型参数上的字段选择（`x.a`，core type 是 struct）| Task 15 |
| `issue48030.go` | run | `TYP-WRONG-TYPE-ARG-COUNT` | **部分类型实参**：`Seq2[int]` 只给 1 个，官方按推断补第 2 个 | 未排期（新特性）|
| `issue48137.go` | run | `TYP-UNDECLARED-NAME` | 函数体内的**局部类型**作类型实参（`type Bar func() Bar; Foo[Bar]()`）—— `T-GEN-08b` 的两份实现 | 未排期（`T-GEN-08b`）|
| `issue48645b.go` | run | `CHK-INVALID-ASSIGN` | 函数实参可赋值性（`func(T) bool` 里含类型参数替换）| Task 15/16 |
| `issue50264.go` | run | `CHK-INVALID-ASSIGN` | 同上 | Task 15/16 |
| `issue54225.go` | run | `CHK-NOT-A-TYPE` | **嵌套实例作类型实参**（`Two[Node[V]]()`）—— Task 12 探针新发现 | 未排期（新缺口）|
| `typeswitch3.go` | run | `TYP-UNDECLARED-NAME` | `switch x := i.(type) { case T: … }` 的**隐式变量** `x` 没声明 | 未排期（type switch 面）|
| `typeswitch4.go` | run | `TYP-UNDECLARED-NAME` | 同上 | 同上 |
| `builtins.go` | compile | `CHK-INVALID-BUILTIN-ARG` | `g3[T M4[rune, V], V any]` 的**第一个 term 的 map key 没替换成 `rune`**（泛型接口实例的 tset 未重算）| Task 9 的 open，`rgoc-types` |
| `issue51909.go` | compile | `CHK-INVALID-ASSIGN` | 函数可赋值性 | Task 15/16 |
| `typelist.go` | compile | `CHK-WRONG-RESULT-COUNT` + `TYP-WRONG-TYPE-ARG-COUNT` | 结果类型解析失败 ⇒ 结果表空 ⇒ 误报 return 个数 | 未排期 |

`errorcheck` 族 5 个里 2 个漏报（该报错的没报），归 **Task 22**。

### 7.3 什么叫「Phase 1 完成」

Phase 1（Task 5–11）的**承诺**是「把 check 面的**系统性**缺口清掉」，不是
「116 个全绿」：

* Task 6 的显式实例化：47 个文件；
* Task 7 的实例化后可赋值；
* Task 8 的 core type 运算（`allBasic` 不是 core type）；
* Task 9 的类型参数进 `Env` + 四个谓词（`underIs` / `commonUnder` / `sliceElem` / `allBasic`）；
* Task 10 的两个边角（接收者形参表的 `_`、`range` 的 `commonUnder`）；
* Task 11 的口径统一（lowering 复查期的上报不再入流）；
* Task 12 的接收者类型参数进 `Env`。

剩下的 12 个都是**单点**：每个只值 1–2 个文件，且大半要等 Phase 2
（单态化 / 实例选择器）才能判。把它们挡在 Phase 1 门口只会让 Phase 2
的实例都造不出来。

---

## 8. Phase 2 第一块砖后的账（Task 15，2026-10-28）

基线文件：`.superpowers/m12-baseline-t15/baseline.tsv`（装置同 §0，只换
`--out` 目录）。

### 8.1 读数

| family | total | check 过 | build 过（t14 → t15） | run 过（t14 → t15） |
|---|---|---|---|---|
| `run` | 71 | 62 | 3 → **28** | 3 → **28** |
| `compile` | 40 | 37 | 6 → **19** | — |
| `errorcheck` | 5 | 2 | 0 → 2 | — |
| `rejected` | 76 | 0（全部干净拒绝 ✓） | — | — |

装置（`run-native-typeparam.sh`）：**`ok 87 → 117`、`bad 105 → 75`**
（分母 192 = 116 门禁 + 76 拒绝）。分族：`run 3 → 20`、`compile 6 → 19`、
`errorcheck 2/5` 不变、`rejected 76/76` 不变。

⚠️ **装置 `run 20` 与基线表 `run 28` 不矛盾，差的 8 个是三层口径**：

| 形态 | 个数 | 文件 | 成因 |
|---|---|---|---|
| native 对、解释器崩/判错 | 5 | `devirtualize1` `devirtualize2` `equal` `issue51522a` `issue51522b` | 类型断言到接口后调方法（nil 解引用）、类型参数与接口判等 / `switch` |
| **native 错**、解释器对 | 1 | `typeswitch5` | `f[float64](myfloat(9))` 我们打 `fooer 0`，官方打 `fooer 9` |
| 流向口径（`D-M5-7`） | 2 | `shape1` `typeswitch7` | 我方 `println` 写 stdout、官方写 stderr；**内容逐字节相同** |

`typeswitch5` 是本轮**新发现的 native 真错**（不是「能力未及」），单独登记为
`F16`，归 Task 16 之后的 native 语义轮。

### 8.2 `build` 失败形态的迁移（`check` 已过之后）

| kind | t14 | t15 | Δ |
|---|---|---|---|
| `missing_type` | 69 | **41** | −28 |
| `layout` | 7 | **4** | −3 |
| `other` | 15 | **7** | −8 |
| `verify` | 3 | **0** | −3 |
| `panic` | 0 | **1** | +1 |
| **合计** | **94** | **53** | **−41** |

`panic` 那 1 个是 `dottype.go`：SSA 侧的 `todo!()`（`build.rs:4106`），与泛型无关，
只是 `run` 走到它才暴露。

### 8.3 剩下 41 个 `missing_type` 按「首条报文的表达式种类」

| 种类 | 文件数 | 典型 |
|---|---|---|
| `Call` | 13 | 多值调用 / 内建 / 方法值 |
| `CompositeLit` | 8 | 实例化具名类型作字面量类型（`Box[int]{v: 7}`）|
| `Unary` | 7 | `-x` / `^x` 落在类型参数上 |
| `TypeAssert` | 4 | `x.(T)` |
| `Selector` | 2 | `x.f`（core type 是 struct）|
| `Binary` | 2 | `a - b` / `a < b` |
| `List` | 1 | `smoketest.go` 的返回表 |
| `FuncLit` | 1 | |
| 别种报文 | 3 | type switch 的 case、复合字面量类型非 struct/array/slice/map、`.Bat` 选择器 |

`layout` 那 4 个（`issue45547` `issue51832` `issue51840` `issue55101`）是**一个根因**：
`typeexpr.rs` 把 `X[A]` 退化成 `X`（`T-TY-29`），于是签名里出现
「带类型参数的具名类型」（`Set[T]` `Cache[T]` `List[T]` `G[T]`）⇒ 没有运行期布局。
它们和 `CompositeLit` 那 8 个是同一件事的两面。

### 8.4 「20 个 run 绿」里一条语义证据都没有 —— 这是真的，不是缺陷

装置报 `有输出 0 + 空输出 20`。查过：`absdiff.go` 的 `main` 体被上游整段注释掉
（`issue45639`），`issue50417b.go` 是 `func main() {}`；全 71 个 run 语料里**只有
12 个**文本上出现 `print`/`println`。这批语料本来就是「能编译且不崩」型 ——
靠 `panic` 报错，不靠打印。

⇒ `G12-2` 的语义证据目前**只能由我们自己的用例给**（`t_gen_15c`：三次 `println`，
断言 stdout `7\n3\n9\n`）。别把「20 个绿」当成「20 个对」。

⚠️ 由此暴露一条**口径阻塞**（`F17`）：我方 `print`/`println` 按 `D-M5-7` 写
**stdout**，官方写 **stderr** ⇒ **任何会打印的 run 语料都过不了装置的 L2**
（`shape1`/`typeswitch7` 实测）。M12 收口前必须在两条里裁一条：
① 改我方流向对齐 Go（`C9` 随之改，全量测试里数以千计的 stdout 断言要跟着翻）；
② 装置改按**合并流**比（`M5` 的做法），牺牲「流向分裂」的可见性。


---

## 9. Task 16 的实验读数（**已回滚，不采纳**，2026-10-28）

留档原因：它精确指出了「`T-TY-29` 一旦推翻，哪些账立刻能收」，所以下次动手
可以直接拿它当靶子 —— 但这一版**不能采纳**（引入了一个真回归，见 §9.3）。

基线文件：`.superpowers/m12-baseline-t16/baseline.tsv`（装置同 §0）。

### 9.1 做了什么

`rgoc-load::typeexpr::resolve_type` 与 `rgoc-check::expr::type_expr` 各加一条
`Expr::Index` 臂，调 `rgoc_types::instantiate` —— 即 `X[A]` **不再退化成** `X`
（推翻 `T-TY-29`）。

### 9.2 读数

| 项 | t15 | 实验版 t16 |
|---|---|---|
| build 失败合计 | 53 | **37** |
| `missing_type` | 41 | **29** |
| `layout` | 4 | 5 |
| `other` | 7 | 10 |

**10 个文件 build 翻绿**：`issue51765` `issue54135` `issue46591` `issue47877`
`issue48344` `issue48617` `issue48838` `issue49049` `interfacearg`（run 族），
外加 `builtins.go` 的 check 也翻绿。**3 个文件推进到链接期**：
`issue48049` `issue52026` `issue48198`（缺 `__desc_0265b777`，以及
`fn4294967295` —— `FuncId::MAX` 没落实例的哨兵值，新形状，归 Task 18）。

新写的两条判据在这版上**一次就绿**（`T-GEN-16` 身份、`T-GEN-16b`
`Box[int]{v:7}` 跑出 `7\nhi\n1 z\n`），说明「臂本身」没写错。

### 9.3 为什么不能采纳

`func Get[T any](b Box[T]) T` 的**任何**调用都变成 `CHK-INVALID-ASSIGN`：

```
DBG load  Box args=[TypeId(36)] -> TypeId(37)   # 形参 `Box[T]`
DBG check Box args=[TypeId(6)]  -> TypeId(45)   # 字面量 `Box[int]`
DBG infer tparams=[TypeParamId(1)] par=[37] args=[45] -> Err(TypCannotInferTypeArgs)
```

形参实例里的 `T` 是 `TypeId(36)`、调用点的是 `TypeId(6)` —— **同一个声明的类型
参数被声明了两次**。推断 `nify(T36, T6)` 时 `T36` 不在句柄表里 ⇒ 走
`unify.rs:578` 的 `common_under` 分支（拿约束 `any` 的本体去对 `int`）⇒ 假 ⇒
`TypCannotInferTypeArgs` ⇒ 形参留 `Box[T]` ⇒ 报不可赋值。

⇒ **顺序应当是「先统一类型参数身份，再开实例化」**。先开实例化 = 每个泛型调用的
可赋值性全崩（实测），`t_gen_07` 腿③ 直接红。

---

## 10. Task 16 采纳版读数（2026-10-28）

基线文件：`.superpowers/m12-baseline-t16b/baseline.tsv`（装置同 §0）。
**§9 那一版已作废**（回滚），这一节是最终采纳的实现。

### 10.1 做了什么

两件事，缺一不可：

1. **`X[A]` 不再是 `X`**（推翻 `T-TY-29`）：`rgoc-load::typeexpr::resolve_type` 与
   `rgoc-check::expr::type_expr` 各加一条 `Expr::Index` 臂，按 `(origin, 实参组)`
   调 `rgoc_types::instantiate`（memo 复用）。非泛型的 `X[A]` 不在这里报错 ——
   交给调用点的「数量错」。
2. **替换要能进实例实参**：`rgoc-types::subst::subst_rec` 的 `Type::Named` 分支
   原先把实例当**原子**（`reinst == None` 直接返回），于是 `T → int` 换不进
   `Box[T]` 的实参里。官方 `subst.go:318-344` 明确要求：`t.Origin()` 非空 ⇒
   逐实参替换、改了就 `check.instance(...)` 重建。停步口径照做，但**不报错**
   （调用方没有错误通道）—— 重建失败退回原条目。

### 10.2 读数

| 项 | t15 | t16b |
|---|---|---|
| build 失败合计 | 52 | **46** |
| `missing_type` | 41 | **31** |
| `layout` | 4 | 4 |
| `other` | 7 | 10 |
| `panic` | 0 | 1 |
| `run` 族 build_ok | 28/71 | **37/71** |
| `compile` 族 build_ok | 19/40 | **22/40** |
| `run` 族真跑通 | 30/71 | **35/71** |

**12 个文件直接变好**：`builtins`（check 也过）、`issue51355`、`issue51765`、
`interfacearg`、`issue46591`、`issue47877`、`issue48344`、`issue48617`、`issue48838`、
`issue49049`、`issue49421`、`issue54135`。

**7 个是「推进」而不是退步**（严重度表把它们记成变坏，逐个看报文后确认）：
7 个的 `check` 全是 **1 → 0**，即检查面首次通过，暴露的是**下游**缺口：

| 文件 | 现在卡在 |
|---|---|
| `issue48049` / `issue52026` / `issue48198` | 链接期缺 `__desc_*` / `__itab_*`（Task 18） |
| `issue51909` | `HIR-UNSUPPORTED-GENERIC`：`Operation` 没落到任何实例 |
| `issue48645b` | `HIR-MISSING-TYPE`：`Call` 的静态类型（内建 `print` 族的调用） |
| `issue50264` / `issue54225` | lowering 期选择器找不到方法（实例方法的接收者类型） |

`other` 从 7 涨到 10、`panic` 出现的 1 个（`dottype.go`）都在这一簇里 —— **不是**
语义回退。

### 10.3 上一节（§9）的根因判断是错的，这里订正

§9 说「同一个声明的类型参数被声明了两次（`TypeId(36)` vs `TypeId(6)`）」。
本次在 `declare_tparams` 上打点，实测：

```
DBG declare_tparams parent=ScopeId(1) n=1 names=[Some("T")]
DBG   tp=TypeParamId(0) ty=TypeId(33) name=Some("T")   # type Box[T any] 的 T
DBG declare_tparams parent=ScopeId(1) n=1 names=[Some("T")]
DBG   tp=TypeParamId(1) ty=TypeId(36) name=Some("T")   # func Get[T any] 的 T
```

两次声明对应**两个不同的声明**（`Box` 与 `Get`），没有重复。`TypeId(6)` 是
`int`，不是另一个 `T` —— §9 的读数里 `Box args=[TypeId(6)]` 那一行是**字面量**
`Box[int]` 的实参，被误读成了类型参数。

真实断点：`infer_from_call` 先用 `rename_map` 把 `T1` 换成新条目 `T2`，再对形参
做 `substitute` —— 而 `substitute` 进不到 `Box[T1]` 的实参里 ⇒ 形参仍是
`Box[T1]` ⇒ `is_parameterized(par, [T2])` 为假 ⇒ ① 段的统一**整位跳过** ⇒
`T` 无绑定 ⇒ `TypCannotInferTypeArgs`。所以修的是 `subst.rs`，不是 `declare_tparams`。

---

## 11. Task 17 读数（2026-10-28）

基线文件：`.superpowers/m12-baseline-t17b/baseline.tsv`。

### 11.1 做了什么（四条，全是「上游漏收敛」而不是「布局算不出来」）

| # | 症状 | 根因 | 修法 |
|---|---|---|---|
| 1 | `issue55101` / `issue51832` / `issue51840`：`帧布局失败：v0 的类型 tXX` | `lower_func` 把 `result_types` 写进 `prog.funcs[fid]` 的时机在**函数体下沉之后**，而 `return` 在体内读它 ⇒ 永远空表 ⇒ `untyped nil` 没被收敛 | 体下沉**之前**先写一次 `result_types` |
| 2 | `issue45547`：`帧布局失败：v2 的类型 t48`（元组进值槽） | 包级 `var _, _ = f()` 的两个 `_` 都被登记成全局，`globals_by_name` 里后者覆盖前者 ⇒ 组里只剩 1 格 ⇒ 多值调用按单值求值 | `InitGroup::globals` 改 `Vec<Option<GlobalId>>`（`None` = 空白标识符占位），`_` 不进全局表 |
| 3 | `issue46461` 回归（Task 16 修好 `X[A]` 之后新暴露）：`M` 的结果类型是 `Invalid` | ① 声明 `type T[U interface{ M() T[U] }]` 解析约束时 `T` 的 `type_params` 还没挂 ⇒ `T[U]` 被当非泛型；② `implements` 完全不认类型参数（方法集空 ⇒ 恒假） | 拆 `declare_tparams` 成 `declare_tparam_names` + `resolve_tparam_constraints`，类型声明在两段之间挂形参表；`has_all_methods` 补「类型参数 ⇒ 用约束接口的方法集」 |
| 4 | 同上 | memo 登记在**约束校验之后** ⇒ 自引用约束无界重入 | 照官方 `context.instance` 顺序：先 `add_named_instance` + `record_instance`，再校验；失败时 `remove_instance` 回滚 |

另外 `instantiate` 加了一档宽限：约束**提到自己的实例**（`mentions_origin`）时跳过该位校验 ——
装载期的方法集此刻还没挂满（方法挂载在签名解析之后），判不了；真不满足的约束由检查面
（方法集完整时）照报。

### 11.2 读数

| 项 | t16b | t17b |
|---|---|---|
| build 失败合计 | 46 | **40** |
| `missing_type` | 31 | **31** |
| `layout` | 4 | **0** |
| `other` | 10 | 8 |
| `panic` | 1 | 1 |
| `compile` build_ok | 22/40 | **28/40** |
| `run` build_ok | 37/71 | 37/71 |

逐文件对比：**只有一处变化** —— `issue46461` 从 `layout` 变 `ok`（它的 `check` 也过）。
其余 173 个文件的判定逐字节不变。全量 **246 suite / 2310 passed / 0 failed / 2 ignored**。

> `layout` 这一栏到此清零，`verify` 早在 Task 16 就清零（t16b 的形态表里没有它）。
> 计划里写的「`VerifyError` 2 个」是被 Task 16 的实例化顺带解决的。

### 11.3 变异验证：3 / 3 全红，其中 C 差点被误判成「冗余」

三处改动逐一变异（`instantiate.rs` / `lower.rs` / `types.rs`），方法如下：

| # | 变异 | 变红的判据 |
|---|---|---|
| A | `lower_func` 把 `result_types` 的写入挪回体下沉之后 | `t_gen_17`（`issue55101` 等 build 退出 2）+ `t_gen_17b` |
| B | `InitGroup::globals` 改回 `Vec<GlobalId>`（`_` 重新进全局表） | `t_gen_17`（`issue45547`） |
| C | `instantiate_at` 的 `add_named_instance` **与** `record_instance` 一起后移到约束校验之后 | `t_gen_17`（`issue46461`：`TypInvalidInstanceCycle` ⇒ 退出 2） |

**C 第一次跑是绿的，结论是错的。** 当时按「判据打不到的路径不留」把 memo 提前**回退**了，
留了段注释说「④″ 宽限已经把自引用约束整个跳过，memo 先后没有可观测差别」。
回退后立刻实测：`issue46461` 变成 `TypInvalidInstanceCycle` +
`帧布局失败：函数 M：v1 的类型 t32 没有运行期布局` —— 说明 memo 提前**不是**冗余。

两个原因叠在一起，才让它看起来像冗余：

1. **变异做的不彻底**：第一版只挪了 `record_instance`，`add_named_instance` 留在原处。
   memo 查询读的是 record 表，但半成品实例的行为已经被 `add_named_instance` 那一步
   改变了 —— 真实路径未完全破坏，变异强度不够。
2. **判据缺形态**（`§11` 开头那条纪律的①档）：`t_gen_17` 当时只列了四个「根因直接
   命中」的语料（`issue55101` / `issue51832` / `issue51840` / `issue45547`），
   **没有 `issue46461`**。而 §11.1 第 4 条的 memo 顺序恰好只由 `issue46461` 这一种
   形态（`type T[U interface{ M() T[U] }]`）打到。全量测试里也没有别的断言编它。

处置：把 `issue46461` 补进 `t_gen_17` 的语料表（第五个），重跑变异 C（两个调用一起后移）
⇒ 变红，报错文本与人工实测一致。教训写进判据的文档注释里：
**变异绿先怀疑判据缺形态，别急着判「路径冗余」。**

顺带清掉两处变异残留：`rgoc-types/src/implements.rs.bak`（带 `DBG impl` 打点的副本，
被误提交进 `291f428`）与 `typeexpr.rs` 里 `DBG load inst` 那一段 `eprintln!`（同一提交）。


## 12. Task 18 读数（2026-10-28）

基线文件：`.superpowers/m12-baseline-t18c/baseline.tsv`（`tools/run-oracle/m12-baseline.sh`）。

### 12.1 做了什么（五条）

前三条是「链接期缺符号」，后两条是同一批语料在收口时暴露的上游漏收敛。

| # | 症状 | 根因 | 修法 |
|---|---|---|---|
| 1 | `issue48049` / `issue52026`：`undefined reference to fn4294967295` | 泛型**方法**的实例没有任何调用点（动态分派由 ssa 按方法名解析，itab 的槽位没有源码位置）⇒ 方法实例从没被请求 ⇒ `resolve_method` 给哨兵 `FuncId(u32::MAX)` | `Mono::declare_method` 记 `(基类型, 方法)` 的 origin；检查面 `request_typed_methods` 为每个具名实例补请求；`lower` 的 `find_method_path` 按实例名查表 |
| 2 | `issue52124`：缺 `__desc_*` | itab 两侧的类型描述子不在 `type_closure` 的根里（`42` 是 `untyped int`，被根过滤掉） | 根收集后把 `it.iface` / `it.concrete` 也压栈 |
| 3 | `issue50264` / `issue51909`：`帧布局失败：函数 Some2·tp5` | 包级那一遍检查泛型体会用**未绑定**的 `T` 请求实例（`Some2·tp5`、`Operation·struct.Body.tp2`），它没有运行期布局 | 判定放在**发码口**（`instance::is_abstract`），见 §12.4 |
| 4 | `issue51909`：`帧布局失败：函数 func@27:11：v4 的类型 t31`（`t31 = UntypedNil`） | `lower_funclit` 也在**体下沉之后**才写 `result_types`（与 §11.1 第 1 条同一个根因的第二种形态），`return nil` 读到空表 | 体下沉之前先写一次 |
| 5 | `issue54535`：`HIR-UNSUPPORTED-GENERIC`（`f[int]` 找不到实例） | `func_keys` 按**裸名**登记，方法 `func (n *node[T]) f` 与包级 `func f[T any]()` 撞名，`or_insert` 让包级 `f` 拿到方法的键 ⇒ 请求被译成方法实例 `node·int.f` | `func_keys` 只收包级函数（`recv.is_none()`） |

第 5 条的症状值得记一笔：`inst_sites` 里好好记着 `f·int`，`instances` 里却只有
`node·int.f` —— 报错文本（「不在 M5 面」）完全指不到真正的原因，是把它说的「实例」
打出来才看见键错了。

### 12.2 读数

| 项 | t17b | t18c |
|---|---|---|
| build 成功合计 | 138 | **154** |
| `run` 族 build_ok | 37/71 | **46/71** |
| `run` 族跑绿 | 35 | **44** |
| `compile` 族 build_ok | 28/40 | **35/40** |
| `missing_type` | 31 | **20** |
| `other` | 8 | **3** |
| `layout` | 0 | 0 |
| `verify` | 0 | 0 |
| `panic` | 1 | 1 |

逐文件对比：**16 个转 `ok`**（`run` 族 9 个：`issue47514b` / `issue47775b` /
`issue47925` / `issue48047` / `issue48049` / `issue49659b` / `issue50264` /
`issue52026` / `issue54535` —— 全部跑绿；`compile` 族 7 个：`issue47797` /
`issue49432` / `issue49538` / `issue51909` / `issue52124` / `issue52241` /
`issue53406`），另有 `issue48253` 少一条 `CHK-INVALID-SELECTOR`（`check` 仍红，
但它与泛型无关，是 `TYP-UNSUPPORTED-IMPORT-FORM` 那一档的连带）。全量
**246 suite / 2312 passed / 0 failed / 2 ignored**。

> `missing_type` 还剩 20 个，归 Phase 3 的账（`M12-plan.md` Task 19 起）。

### 12.3 变异验证：5 / 5 全红

装置 `.superpowers/m18-mutate.py`（`apply A..E` / `restore`），每次变异前
`cargo clean -p <crate>`，恢复走 `git checkout --`（代码先提交了，见 `cd33cd5`）。

| # | 变异 | 变红的判据 |
|---|---|---|
| A | `func_keys` 把方法也收进去（去掉 `recv.is_none()`） | `t_gen_18_链接`（`issue54535`） |
| B | `lower_funclit` 的 `result_types` 退回体下沉之后 | `t_gen_17`（`issue51909`） |
| C | `lower_funclit` 的结果类型退回 `type_expr_of` | `t_gen_17`（`issue48191`） |
| D | `request_typed_methods` 恒 `false`（不补方法实例） | `t_gen_18_链接`（`issue48049` / `issue52026`） |
| E | `is_abstract` 恒 `false`（不过滤发码） | `t_gen_17`（`issue50264`） |

**C 第一次是绿的**，与 §11.3 同一档教训（判据缺形态）：`t_gen_17` 当时列的
`issue51909` 的结果类型是接口 `Response`，`type_expr_of` 与 `type_in_body` 两档
都能解，打不到这条路径。真正能打到的是 `issue48191`（函数字面量结果类型是 `G3`
这类类型参数），而它当时不在任何判据里。补进 `t_gen_17` 的语料表后 C 变红。
同一次还补了 `issue50264`（`is_abstract` 的牙口）。

### 12.4 一处改对了又改回来的地方：判定该放请求口还是发码口

第 3 条最初是**挡在请求口**的：`Session::request_inst` 里「实参有任一无运行期
布局的类型 ⇒ 不请求」。它让 `issue50264` / `issue51909` 立刻转好，看起来干净，
但两条判据当场塌：

* `t_gen_14b_递归实例`：自引用链 `Loop[T] → Loop·tp0` 被拒，受理表只剩 1 条
  （判据要 2 条 —— 链必须**收敛**，不是被挡）；
* `t_gen_14d_实例化环有码`：`issue48711.go` 的环**只**在类型参数实参上长出来
  （每层 `[]T` 长一级是唯一能让深度长起来的东西），挡住它
  `TYP-INVALID-INSTANCE-CYCLE` 就再也报不出来，只剩一条
  `TYP-WRONG-TYPE-ARG-COUNT` 的假症状。

判据是「造不造函数」，不是「认不认这条请求」——两者的分界线在发码口。拒绝一个
合法请求的代价比少发一个「谁也不调」的函数大得多（泛型原型本身不发码，所以没有
活代码会引用这些实例）。这条写进了 `Session::request_inst` 的注释里。

## 13. Task 19：`T-GEN-05` 无类型参数残留（2026-10-28）

判据 `t_gen_05_无残留`（`m12_checkface.rs`）。

### 13.1 判据换了载体：不是 `grep 'type parameter'`

计划里写的是「逐个 `--emit=asm` + `--dump=hir`，断言 `grep -c 'type parameter' == 0`」。
实测两处都数不出这个词：

| 载体 | 今天的内容 | 数得到 `type parameter` 吗 |
|---|---|---|
| `run --dump-hir`（HIR dump） | `# func 0 main` / `0 Func params=0 …` / `7 Expr const Int(7)` —— **只有结构，没有类型名** | ❌ 恒 0 |
| `types --dump`（`C7` 中性 dump） | `0 named 12` / `3 invalid` / `4 tuple 1` —— 类型**种类**用小写单词写 | ❌ 恒 0（`TypeParam` 不在 `C7` 的词表里） |

两条都恒 0 ⇒ 那样的判据是**恒真**的（`§11.3` 的同一条纪律）。
改成查**真实字段**：走库 API 拿 `Program` + `TypeTable`，逐函数查
`param_types` / `result_types` / `slot_types` 与全局类型。

三处口径分开写：

* `param_types` / `result_types` / 全局类型：`TypeParam` 与 `Invalid` **一个都不许有**；
* `slot_types`：**允许 `Invalid`**（`types.rs` 的约定就是「`Invalid` ⇒ 无类型临时槽」），
  但**不许** `TypeParam`。

> `TypeParam` 要**先看原类型**再看 `underlying`：`underlying` 会把类型参数解到约束
> 里的接口上，于是 `Type::TypeParam` 那一档永远匹配不到 —— 阴性对照第一次就是这么
> 恒真的（塞了 `TypeParam` 却报空）。

### 13.2 读数

| 项 | 值 |
|---|---|
| 查了的门禁文件 | **84**（`run` 47 + `compile` 35 + `errorcheck` 2） |
| 残留 | **0** |
| 跳过 | 42（`run` 24 + `compile` 5 + `errorcheck` 3，全是还没编过的 —— Phase 3 的账） |

门槛 `MIN_CHECKED = 84` 是断言的一部分：某个文件从 ok 变 reject 时这条判据会红，
否则它会悄悄退化成「一个都没查」。

### 13.3 判据有牙的三份证据

1. **内建阴性对照**（每次跑都做）：往一份真 HIR 的 `result_types` 里塞 `invalid` /
   塞一个 `TypeParam`，断言 `type_residue` 立刻非空。没有它，「残留为空」可能只是
   判据恒真。
2. **真实残留**：拿 §12.3 的变异 E（`is_abstract` 恒 `false`，让「泛型原型自己」
   也发码）跑这条判据，立刻红，且**位置精确到形参**：
   ```
   issue50264.go: func#3 Some2·tp5 形参#0: 类型参数
   issue50264.go: func#3 Some2·tp5 槽#0: 类型参数
   ```
   比 `帧布局失败：函数 Some2·tp5：v0 的类型 t52` 有用得多 —— 后者要读到
   「帧布局」才知道是类型没替换，前者直接说「哪个函数第几个形参还是 T」。
3. 门槛断言（§13.2）。

### 13.4 顺带发现的一处假绿（归 Task 22）

`errorcheck` 族的 `issue51232` / `issue51233` 现在 `check` **退出 0** —— 它们本该
报 `TYP-WRONG-TYPE-ARG-COUNT`（`M12-plan.md` Task 22 的账）。因为 check 干净，
`t_gen_05` 把它们当成「能 lower 成功的文件」计入了 84。这不是本 Task 引入的问题，
但**必须记账**：Task 22 修好之后这两个文件会从 84 里掉出去，`MIN_CHECKED` 要跟着
改成 82。

## 14. Task 20：`T-GEN-02` compile 族 39/40（2026-10-28）

基线文件：`.superpowers/m12-baseline-t20/baseline.tsv`。

### 14.1 五条修复（每条有语料对账）

| # | 语料 | 症状 | 根因 | 修法 |
|---|---|---|---|---|
| 1 | `issue53762` / `issue49516` | `Value[int](1)` / `(*Q[int]).Push` 报 `HIR-UNSUPPORTED-GENERIC` | check 面只认「**函数**实例化」（`generic_funcs` 有账），**类型**实例化 `T[P]` 落进索引分派 ⇒ invalid | `explicit_instantiation` 旁新增 `type_instantiation`：`x` 是 `TypeExpr` 且指向带类型参数的 named 原型 ⇒ `rgoc_types::instantiate` 造实例，返回 `TypeExpr` operand |
| 2 | `issue47878` | `m[k]()` / `(<-ch)()` / `x.(T)()` 的调用报 `HIR-MISSING-TYPE`（mode=invalid） | check 的 `call` 只接 `Value | Variable`，map index / comma-ok 操作数落 `_ => invalid()` —— 但它们的 `typ` 就是签名类型 | match 扩 `MapIndex | CommaOk` |
| 3 | `issue47878` | `槽 s0 没有根（函数 main）` / `既不提升也没有根` | 指针接收者方法调用的 recv 是 `Addr(Place::Slot(s))`，SSA 的取址扫描（`scan_expr`）与**确定赋值分析**（`Def::expr`）都不扫 `Callee::Method.recv` | 两处都补扫；`Iface.recv` 保守同扫。⚠️ 单测 `var src Src1[int]; src.Next()` 能过只是因为 `_ = src` 把槽推进了 `unsafe_slots` —— **假绿**，`Def` 的缺口一直都在 |
| 4 | `smoketest.go` | `var _ = f2[int, string, struct{}]` 报「没有落到任何实例」 | lowering 的 `Index` 臂**先求下标**：多实参的 index 是 `Expr::List`，不是表达式，恒 invalid ⇒ 断链 | 先按位置查 `inst_target`（实例账），`a[i]` 没账自然落回索引路径 |
| 5 | `typelist.go` | `func _[T any](m T)` 报 `not enough type arguments`；`~int` 约束上 `*p` / `ch <- 0` / `<-ch` 三条误报 | ① `generic_funcs` 按名字记账，`func _[T …]` 与 `func _ interface{…}` 撞裸名；② 类型参数上的操作符判定只看 `underlying(T)`（= 约束接口） | ① `Package::blank_func_tparams` 按**声明位置**记账；② 新增 `rgoc_types::core_type`（项表非空且 underlying 全同 ⇒ 那个 underlying），`deref` / `recv` / `send` 改走 `under_or_core` |

> 第 5 条的 `core_type` 有个与 `t_gen_05` 同源的坑：**必须先看原类型**是不是
> `TypeParam` 再走 `underlying` —— 后者把类型参数解到约束接口上，那一档
> 永远匹配不到（第一次就这么写的，三条诊断纹丝不动）。

### 14.2 读数

| 项 | t18c | t20 |
|---|---|---|
| build 成功合计 | 154 | **168** |
| `run` 族 build_ok | 46/71 | **55/71** |
| `run` 族跑绿 | 44 | **50** |
| `compile` 族 build_ok | 35/40 | **39/40** |
| 全量测试 | 2312 | **2314** passed / 0 failed |

逐文件 14 个转 `ok`：compile 族 4 个（`issue47878` / `issue49516` /
`issue53762` / `smoketest`），run 族 10 个（`eface` / `issue47925b/c/d` /
`issue48598` / `issue48602` / `issue48645b` / `issue50109b` / `issue53477` /
`issue48604`）—— 后一批是类型实例化与复杂被调者的**连带收益**（泛型类型的
方法值调用、类型断言调用都不是 typelist 专属形态）。

### 14.3 白名单：U-M12-01（部分实例化）

`typelist.go` 剩两条诊断（92 / 112 行的 `f := f2[byte]` /
`f := f4[int]`）：**部分实例化** —— 给部分类型实参，剩余的从调用实参推断。
官方里「部分实例化的函数值」是一等公民（类型 = 仍带类型参数的签名）；
我们的类型系统没有这个维度，支持它需要贯穿三层：

* check：`Operand` 得能表示「部分泛型函数值」（新 mode 或 typ 携带 tparams）；
* HIR：`f := f2[byte]` 的槽里存什么？（实例要等调用点的推断才定）
* SSA：帧布局拿到含 `TypeParam` 的签名怎么办？

这是一条独立的里程碑级工作，不是 Task 20 的局部补丁。判据
`t_gen_02_compile_39of40` 把白名单**冻住**（`WHITELIST = ["typelist"]` +
断言条数 == 1），防止悄悄变大。

### 14.4 变异验证：4 / 4 全红

装置 `.superpowers/m20-mutate.py`（apply A..D / restore），每次变异前
`cargo clean -p`，判据都是 `t_gen_02_compile_39of40`（compile 全族遍历，
四条修复的语料都在里面）。

| # | 变异 | 变红 |
|---|---|---|
| A | `type_instantiation` 整条短路（`if false &&`） | `issue53762` / `issue49516` |
| B | `call` 的 mode 收窄回 `Value | Variable` | `issue47878` |
| C | `Def::expr` 不再读方法接收者 | `issue47878`（建图 panic） |
| D | lowering 的 `Index` 先求下标（实例账断链） | `smoketest` |

### 14.5 ⚠️ 事故记录：变异恢复冲掉未提交改动（同一条坑第三次）

变异 B 的锚点没匹配（0 次）⇒ 脚本 `exit 2`，但**恢复已经跑过** ——
`.superpowers/m20-mutate.py` 的 `restore` 用 `git checkout --`，把
**未提交的 Task 20 全部改动**（expr.rs / build.rs / lower.rs）冲掉了。
`§11.3` 记过这条纪律（「要么先提交、要么用原文副本」），这次是执行层没
落实：变异装置的 FILES 列表里是**尚未提交**的文件。处置：按会话记录
逐段重写（三处都有完整 diff 上下文），**从本轮起纪律改为「写完即 add」**
—— staging 区就是恢复操作的保护区。装置本身也该在 apply 前检查
`git status --short <file>` 非空时拒绝运行。
