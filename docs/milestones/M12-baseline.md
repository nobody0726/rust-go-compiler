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
