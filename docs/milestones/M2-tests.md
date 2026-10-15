# M2 测试先行清单（M2-tests.md）

> **阶段 ID**：`M2`　|　**状态**：**已冻结并验收**（A1–A8 全过，2026-10-15）　|　**日期**：2026-10-14
> **文档索引**：[`../README.md`](../README.md)　|　**上游**：[`M2-design.md`](./M2-design.md)（决策 D-M2-1~7、事实基线 F1–F19）、[`../03-roadmap.md`](../03-roadmap.md) §M2、[`../01-feature-set.md`](../01-feature-set.md) §1.2（AST-01..49）、§1.4（PIPE-03/04/05/25）
> **下游**：[`M2-plan.md`](./M2-plan.md)（RED-GREEN 任务分解）
>
> **本文档的效力**（[`../03-roadmap.md`](../03-roadmap.md) §6.2）：M2 开工前三件套之二。**必需测试 ID 100% 通过**是进入 M3 的条件；
> 白名单须**开工前冻结**；跳过项**不得记为 pass**。
>
> **ID 命名**：`T-A-<nn>` = M2 AST/parser（A = AST，与 M0 的 `T-S*`、M1 的 `T-L-*` 区分）。
> **切面写在列里，不写进 ID**（沿用 [`M1-tests.md`](./M1-tests.md) §0 的约定）。

---

## 0. 冻结声明

| 项 | 冻结内容 | 冻结时点 | 变更规则 |
|---|---|---|---|
| **F1** | §2 的 **AST-01..49 逐项测试 ID** | ✅ 已验收（A1，49/49 有测试） | **不得有项无测试**；新增须单独记录 |
| **F2** | §3 的**分母**（必需 ID 总数） | ✅ **67 个 T-A ID**（66 pass + 1 deferred〔T-A-97，open_gaps 登记〕） | **不得因跑不动而缩小**（`03` §3.3） |
| **F3** | §4 的负例清单与**双向断言**要求 | ✅ 已验收（A2，25 文件双向全过） | 同上 |
| **F4** | §6 的 unsupported 清单 | ✅ 已确认（U12 的 parse 半已解除；无新增 unsupported） | 同上 |
| **F5** | §7 的超时与资源上限 | ✅ 已确认（深度上限 1000 已落地为 `MAX_NEST_DEPTH`，`T-A-96`） | 实测后可有记录地调整 |
| **F6** | §5 的 oracle 口径（adapter / 中立 dump 格式 / 来源标注） | ✅ 已验收（A4；口径冻结进 `C6-ast-dump.md`） | 同上 |

---

## 1. 判据的三种来源

每个测试 ID 必须标明判据来源，**这决定了它能否被自动化**：

| 记号 | 含义 | 说明 |
|---|---|---|
| **`O`** | **oracle 对照** —— 期望值由 go1.27.1 的 ast-oracle adapter 生成 | 最权威；需要容器 |
| **`S`** | **自证** —— 不变式断言，不需要 oracle | 测 oracle 测不到的性质（单调、一致性、深度上限） |
| **`H`** | **手写** —— 期望值由人写，因为**官方没有这个行为** | **只用于实现限制**（递归深度上限），必须标注 |

> **`H` 是危险的记号**：见到 `H` 就要问「为什么官方没有」。M2 目前**只有一条**（`T-A-96`，递归深度上限）。

---

## 2. AST-01..49 逐项覆盖（F1）

> **口径修正**（源自 [`M2-design.md`](./M2-design.md) §3.1/§3.4，写 plan 时同步）：
> 1. **类型节点落在 `Expr` 族**：`AST-24..32` 的映射列写 `Type::Array` 等是**语法类别标注**，实现为 `Expr::ArrayType` 等，**不新建 `TypeExpr`**。
> 2. **官方无 `BadStmt`/`BadDecl`/`BadType`**：语句/声明错误对齐官方返回 `None`；`AST-09 BadExpr` 同时承载表达式与类型错误。
> 3. **`SimpleStmt` 独立成 enum**（`if`/`for`/`switch` 的 init 位专用）。

| 组 | 节点 | 覆盖它的测试 ID | 判据 |
|---|---|---|---|
| **A. 文件与声明** | `AST-01` File | `T-A-00` `T-A-20` | O |
| | `AST-02` Group | `T-A-05` `T-A-21` `T-A-22` | O |
| | `AST-03` Comment | `T-A-05` `T-A-26` | O |
| | `AST-04` ImportDecl | `T-A-01` `T-A-21` | O |
| | `AST-05` ConstDecl | `T-A-01` `T-A-22` | O |
| | `AST-06` TypeDecl | `T-A-01` `T-A-23` | O |
| | `AST-07` VarDecl | `T-A-01` `T-A-24` | O |
| | `AST-08` FuncDecl | `T-A-01` `T-A-25` | O |
| **B. 表达式** | `AST-09` BadExpr | `T-A-02` `T-A-38` `T-A-63` | O |
| | `AST-10` Name | `T-A-02` `T-A-30` | O |
| | `AST-11` BasicLit | `T-A-02` `T-A-30` | O |
| | `AST-12` CompositeLit | `T-A-02` `T-A-33` | O |
| | `AST-13` KeyValueExpr | `T-A-02` `T-A-33` | O |
| | `AST-14` FuncLit | `T-A-02` `T-A-36` | O |
| | `AST-15` ParenExpr | `T-A-02` `T-A-37` | O |
| | `AST-16` SelectorExpr | `T-A-02` `T-A-34` | O |
| | `AST-17` IndexExpr | `T-A-02` `T-A-34` | O |
| | `AST-18` SliceExpr | `T-A-02` `T-A-34` | O |
| | `AST-19` AssertExpr | `T-A-02` `T-A-35` | O |
| | `AST-20` TypeSwitchGuard | `T-A-02` `T-A-35` | O |
| | `AST-21` Operation | `T-A-02` `T-A-31` | O |
| | `AST-22` CallExpr | `T-A-02` `T-A-32` | O |
| | `AST-23` ListExpr | `T-A-02` `T-A-38` | O |
| **C. 类型**（落在 `Expr` 族） | `AST-24` ArrayType | `T-A-02` `T-A-40` | O |
| | `AST-25` SliceType | `T-A-02` `T-A-40` | O |
| | `AST-26` DotsType | `T-A-02` `T-A-41` | O |
| | `AST-27` StructType | `T-A-02` `T-A-42` | O |
| | `AST-28` Field | `T-A-05` `T-A-42` | O |
| | `AST-29` InterfaceType | `T-A-02` `T-A-43` | O |
| | `AST-30` FuncType | `T-A-02` `T-A-44` | O |
| | `AST-31` MapType | `T-A-02` `T-A-45` | O |
| | `AST-32` ChanType | `T-A-02` `T-A-45` | O |
| **D. 语句** | `AST-33` EmptyStmt | `T-A-03` `T-A-50` | O |
| | `AST-34` LabeledStmt | `T-A-03` `T-A-50` | O |
| | `AST-35` BlockStmt | `T-A-03` `T-A-50` | O |
| | `AST-36` ExprStmt | `T-A-03` `T-A-51` | O |
| | `AST-37` SendStmt | `T-A-03` `T-A-51` | O |
| | `AST-38` DeclStmt | `T-A-03` `T-A-52` | O |
| | `AST-39` AssignStmt | `T-A-03` `T-A-04` `T-A-52` | O |
| | `AST-40` BranchStmt | `T-A-03` `T-A-53` `T-A-80` | O |
| | `AST-41` CallStmt | `T-A-03` `T-A-54` | O |
| | `AST-42` ReturnStmt | `T-A-03` `T-A-55` | O |
| | `AST-43` IfStmt | `T-A-03` `T-A-56` | O |
| | `AST-44` ForStmt | `T-A-03` `T-A-57` | O |
| | `AST-45` SwitchStmt | `T-A-03` `T-A-58` | O |
| | `AST-46` SelectStmt | `T-A-03` `T-A-59` | O |
| | `AST-47` RangeClause | `T-A-04` `T-A-59` | O |
| | `AST-48` CaseClause | `T-A-05` `T-A-58` `T-A-59` | O |
| | `AST-49` CommClause | `T-A-05` `T-A-59` | O |

> **退出条件 1**：上表每行至少一个 ID，全绿。**49 项无遗漏**。

---

## 3. 十一个切面的测试 ID（F2）

### 3.1 切面 ① AST 数据模型与遍历（`T-A-0x`）—— 判据全 `O`

> 测 `rgoc-ast` crate：类型能构造、能遍历、能 dump。**不含 parser**。

| ID | 用例 | 断言要点 | 判据 |
|---|---|---|---|
| `T-A-00` | `AstFile` 顶层 | package 名 + decls 数 + eof Pos + goVersion + pragma 均可构造与读取 | O |
| `T-A-01` | `Decl` 五变体 | Import/Const/Type/Var/Func 各构造一个，match 分派正确 | O |
| `T-A-02` | `Expr` 全变体 | Name/BasicLit/Binary/Unary/Call/Bad/CompositeLit/KeyValue/FuncLit/Paren/Selector/Index/Slice/TypeAssert/TypeSwitchGuard/List + **类型变体**（ArrayType/SliceType/DotsType/StructType/InterfaceType/FuncType/MapType/ChanType）各构造一个 | O |
| `T-A-03` | `Stmt` 全变体 | Empty/Labeled/Block/Expr/Send/Decl/Assign/Branch/Call/Return/If/For/Switch/Select 各构造一个 | O |
| `T-A-04` | `SimpleStmt` enum | Empty/Expr/Send/Assign/RangeClause 各构造一个；验证它与 `Stmt` 分开（`if`/`for`/`switch` init 位专用） | O |
| `T-A-05` | 辅助 struct | Field(name=None→匿名)/Group/CaseClause(cases=None→default)/CommClause(comm=None→default)/Comment(四种 CommentKind)/Pragma/ChanDir(SendOnly=1/RecvOnly=2) 各构造一个 | O |
| `T-A-06` | 遍历器 | 深度优先走完整棵 AST（3 层以上嵌套），节点数与手数一致 | S |

### 3.2 切面 ② 节点 Pos 规则（`T-A-1x`）—— 判据全 `O` —— 门禁 `A3`

> **这是 M2 最容易出"差 1"的地方**（[`M2-design.md`](./M2-design.md) §3.3）。
> 判据：用官方 `@` 标记片段（`F10`），逐条断言节点 pos。

| ID | 用例 | 断言要点 |
|---|---|---|
| `T-A-10` | 声明 Pos | `import @"math"`、`const (@x)` → pos 在首个 token；**与是否分组无关** |
| `T-A-11` | 表达式 Pos | `@x`（Name）、`@0`（BasicLit）、`a@+b`（Binary 取 `+`）—— 取**最左标识其种类的** token |
| `T-A-12` | 终端节点 Pos | Name/BasicLit = token 本身位置 |
| `T-A-13` | 非终端节点 Pos | IndexExpr 取 `[`、IfStmt 取 `if`、ForStmt 取 `for` —— 取**唯一关联 token** |
| `T-A-14` | 复合声明 Pos | 分组 `const (@a) (@b)` → 各声明的 pos 在组内首个 token，**不在 `(`** |

### 3.3 切面 ③ 声明族解析（`T-A-2x`）—— 覆盖 A 组 `AST-01..08` —— 判据全 `O`

| ID | 用例 | 断言要点 |
|---|---|---|
| `T-A-20` | package 子句 + `//go:build` | `AST-01`：pkg_name 正确；goVersion 取自 `//go:build go1.22`（仅 `top` 时，`F7`） |
| `T-A-21` | ImportDecl | `AST-04`：单行 / 分组 / 别名 / 点 import；路径是合法字符串语法（**不加载包**）；`AST-02` Group 共享 |
| `T-A-22` | ConstDecl | `AST-05`：含 iota / 分组共享 iota / 无类型 / 无表达式 |
| `T-A-23` | TypeDecl | `AST-06`：含泛型 tparams / alias（`=`）/ 非别名 |
| `T-A-24` | VarDecl | `AST-07`：单 / 多 / 分组 / 含初始化 / 无初始化 |
| `T-A-25` | FuncDecl | `AST-08`：含 recv（方法）/ 泛型 tparams / body=nil（外部声明）/ 无 recv |
| `T-A-26` | Comment 挂载 | `AST-03`：四种 CommentKind（Above/Below/Left/Right，`F17`）；`//go:` 经 Pragma 通道（`F8`） |

### 3.4 切面 ④ 表达式族解析（`T-A-3x`）—— 覆盖 B 组 `AST-09..23` —— 判据全 `O`

| ID | 用例 | 断言要点 |
|---|---|---|
| `T-A-30` | Name / BasicLit | `AST-10/11`：标识符 + 五种 LitKind（Int/Float/Imag/Rune/String）—— **值不算**，M3 折叠 |
| `T-A-31` | Operation | `AST-21`：一元 / 二元 / **优先级爬升**（`a+b*c` 的结合形状与 oracle 一致）；一元 Y=nil |
| `T-A-32` | CallExpr | `AST-22`：普通调用 / 泛型实例化 / 类型转换（语法同形）/ `...` 变参 |
| `T-A-33` | CompositeLit + KeyValue | `AST-12/13`：含 `xnest` 二义消解（§5.6）；`if x == T{}{}` 类用例对撞 |
| `T-A-34` | Selector / Index / Slice | `AST-16/17/18`：`x.y` / `x[i]` / `x[i:j]` / `x[i:j:k]`；Index 的二义（索引 or 泛型实例化）由 M3 消解 |
| `T-A-35` | TypeAssert / TypeSwitchGuard | `AST-19/20`：`x.(T)` / `x.(type)` / `v := x.(type)` |
| `T-A-36` | FuncLit | `AST-14`：匿名函数字面量 |
| `T-A-37` | ParenExpr | `AST-15`：`(x)` 求值分组 |
| `T-A-38` | ListExpr / BadExpr | `AST-23/09`：多返回值列表载体；错误恢复占位 `Expr::Bad(BadExpr)`（类型错误也复用它） |

### 3.5 切面 ⑤ 类型族解析（`T-A-4x`）—— 覆盖 C 组 `AST-24..32`（落在 `Expr` 族）—— 判据全 `O`

> ⚠️ **类型与表达式同族**（[`M2-design.md`](./M2-design.md) §3.1）：parser 阶段无法区分。这些节点都是 `Expr` 的变体，不是独立 `TypeExpr`。

| ID | 用例 | 断言要点 |
|---|---|---|
| `T-A-40` | ArrayType / SliceType | `AST-24/25`：`[n]T` / `[...]T`（Len=nil→...）/ `[]T` |
| `T-A-41` | DotsType | `AST-26`：`...T`（变参） |
| `T-A-42` | StructType / Field | `AST-27/28`：含嵌入字段 / tag / 匿名字段（name=None） |
| `T-A-43` | InterfaceType | `AST-29`：含方法集 / 嵌入接口 / `any` 别名 / 类型约束（`~T` / `T | U`） |
| `T-A-44` | FuncType | `AST-30`：含泛型 tparams / 参数 / 结果（命名 / 未命名） |
| `T-A-45` | MapType / ChanType | `AST-31/32`：`map[K]V` / `chan T` / `chan<- T` / `<-chan T`（ChanDir：SendOnly=1/RecvOnly=2，`F16`） |

### 3.6 切面 ⑥ 语句族解析（`T-A-5x`）—— 覆盖 D 组 `AST-33..49` —— 判据全 `O`

| ID | 用例 | 断言要点 |
|---|---|---|
| `T-A-50` | Empty / Labeled / Block | `AST-33/34/35`：`;` / `Label:` / `{ ... }` |
| `T-A-51` | ExprStmt / SendStmt | `AST-36/37`：`f()` / `ch <- v` |
| `T-A-52` | DeclStmt / AssignStmt | `AST-38/39`：局部声明 / `=` / `:=` / `++` / `--` / `+=`（Op=0&Rhs=nil→IncDec） |
| `T-A-53` | BranchStmt | `AST-40`：break / continue / goto / fallthrough；Target 由 CheckBranches 计算 |
| `T-A-54` | CallStmt | `AST-41`：`go f()` / `defer f()` |
| `T-A-55` | ReturnStmt | `AST-42`：零值 / 单值 / 多值 / 裸 return |
| `T-A-56` | IfStmt | `AST-43`：含 init / short / else / else-if 链 |
| `T-A-57` | ForStmt | `AST-44`：三段式 / 条件-only / 无限 / range（含 **range over func**，go1.27 语法） |
| `T-A-58` | SwitchStmt / TypeSwitchStmt | `AST-45`：含 init / tag / case / default / type switch（`x.(type)`） |
| `T-A-59` | SelectStmt + RangeClause/CaseClause/CommClause | `AST-46/47/48/49`：select 的 case/comm/default；CaseClause(cases=None→default)；CommClause(comm=None→default) |

### 3.7 切面 ⑦ 错误恢复（`T-A-6x`）—— 判据全 `O`

> **本组的每条都必须断言两件事**：①诊断（位置 + `DiagCode` + 消息子串）②**其后的解析仍然正确**（剩余 AST 与 oracle 一致）。
> 只断言"报错了"是假通过（同 [`M1-tests.md`](./M1-tests.md) §3.5）。

| ID | 用例 | 断言要点 |
|---|---|---|
| `T-A-61` | `advance` + stopset + followset | `F2/F3`：fnest>0 时 stopset（**14** 个语句起始关键字，勘误见 `M2-design.md` §14）并入 followset |
| `T-A-62` | `errh=nil` 首错即停 | `F4`：遇第一个错 `panic(p.first)`，由外层 `recover`（`F19`）转成 first error 返回 |
| `T-A-63` | `errh≠nil` 尽量继续 | `F4`：报错后产出 `BadExpr` 占位（表达式层）/ `None`（语句/声明层），**后续 AST 与 oracle 一致** |
| `T-A-64` | EOF 抑制连环错 | `F5`：EOF 且已有错时不再报 |
| `T-A-65` | 三个恢复点 | §5.3：顶层声明之间 / 语句列表 / 括号内列表 |
| `T-A-66` | 错误消息子串匹配 | `F11`：`rx.MatchString(e.Msg)` 是**子串匹配**（非全等）；消息前缀 `syntax error: ` |

### 3.8 切面 ⑧ 指令与位置（`T-A-7x`）—— 判据全 `O`

| ID | 用例 | 断言要点 |
|---|---|---|
| `T-A-70` | `//line` 重映射 | `F6`：`//line filename:line[:col]` → 后续 Pos 指向新基准；由 parser 的 `updateBase()` 处理 |
| `T-A-71` | `//go:build` 取 goVersion | `F7`：**仅 `top`**（package 子句之前）且 `constraint.Parse` 成功时写入 `p.goVersion` |
| `T-A-72` | `//go:` PragmaHandler | `F8`：经回调，`text` 已去 `//` 前缀；解析并挂载为 `Pragma`，**不生效** |
| `T-A-73` | 已知指令 + 拒绝未知 | §5.5：已知清单见 §4.2；未知关键指令按 [`03`](../03-roadmap.md) §M2「拒绝未知关键指令」处理（⚠️ 与官方"忽略未知"行为的冲突见 §9 待裁决） |

### 3.9 切面 ⑨ branches 检查（`T-A-8x`）—— 判据全 `O`

> **必须实现**（[`M2-design.md`](./M2-design.md) §5.7）：testdata 的 `// ERROR` 测试用 `CheckBranches` 模式（`F13`）。

| ID | 用例 | 断言要点 |
|---|---|---|
| `T-A-80` | break/continue/fallthrough 位置 | `F18`：错位的 break/continue/fallthrough 报诊断 |
| `T-A-81` | 标签 | 非法 / 未用 / 重复 / 缺失的标签 |
| `T-A-82` | goto | goto 跳过变量声明 / 跳进块 |

### 3.10 切面 ⑩ oracle 对撞与 dump（`T-A-9x`）—— 门禁 `A4` —— 判据全 `O`

| ID | 用例 | 断言要点 |
|---|---|---|
| `T-A-90` | 中立 dump 格式（C6） | `D-M2-5`：rgoc dumper 与 oracle dumper 输出**同格式**；规则见 §5.2 |
| `T-A-91` | ast-oracle adapter | §5.1：vendor 官方 syntax，编译成独立二进制，容器内运行 |
| `T-A-92` | testdata 31 文件对撞 | `F15`：双方 dump 逐行 diff **为空** |
| `T-A-93` | 规范化规则 | §5.3 / `R1`：列表扁平化 / Operation 统一 / 两边结构不同的节点有规则覆盖 |

### 3.11 切面 ⑪ 标准库冒烟与不变式（`T-A-9x` 续）—— 门禁 `A5` —— 判据 `O`/`S`/`H`

| ID | 用例 | 断言要点 | 判据 |
|---|---|---|---|
| `T-A-94` | 标准库全量冒烟 | `go_source_code/src` 全部 `.go` 解析**零意外错误**（对应官方 `TestStdLib`） | O |
| `T-A-95` | `// ERROR` 双向匹配 | `F11/F12`：声明的都报出 + 报出的都声明；行注释校验**行号**，块注释校验**行+列** | O |
| `T-A-96` | 递归深度上限 | 深嵌套（>1000 层）报诊断 `SYN-NESTING-TOO-DEEP`，**不 panic / 不栈溢出** | **H** |
| `T-A-97` | 版本门控 | `D-M2-6`：`-lang go1.18` 拒绝泛型语法 / `-lang go1.22` 拒绝 range over func；**只判语法合法性** | O |
| `T-A-98` | 三次一致性 | 同输入跑 3 次，AST + 诊断流**逐字节相同** | S |
| `T-A-99` | Eof 唯一性 | parser 结束后 AST 的 eof 字段有且仅有一个 Pos | S |

> **分母 = 67 个 T-A ID**（`T-A-00`..`T-A-99`，跳过未使用的编号段：`07-09`/`15-19`/`27-29`/`39`/`46-49`/`60`/`67-69`/`74-79`/`83-89`，共 33 个空位）。

---

## 4. 负例清单（F3）

### 4.1 testdata 负例（主力）

| 来源 | 文件数 | `// ERROR` 条数 | 双向断言 |
|---|---|---|---|
| `syntax/testdata/` 25 个含 `// ERROR` 的文件 | 25 | ~122（执行期枚举冻结） | ①声明的都报出 ②报出的都声明 |

**验证方式**：`scripts/check-ast-testdata.sh`，对每个文件：
1. rgoc parser 解析（`CheckBranches` 模式，`F13`）
2. 收集诊断流
3. 正向：每条 `// ERROR rx` 的位置（行号 or 行+列，`F12`）至少有一条诊断匹配，且消息含 `rx` 子串（`F11`）
4. 反向：每条报出的诊断至少被一条 `// ERROR` 声明覆盖（**无多余错误**）

> **子串匹配的宽松点**（`F11`）：`rx.MatchString(e.Msg)` 是**包含匹配**，有些用 `.*` 通配。
> 所以严格度是"**位置必须准 + 消息含指定子串**"，不是逐字全等。
> ⚠️ **M2 必须复刻官方消息文本**（与 M1 的 `D-M1-9`「不复刻消息」不同，见 [`M2-design.md`](./M2-design.md) §5.4）。

### 4.2 自有负例

| ID | 输入 | 期望诊断 | 期望的"之后还能解析" |
|---|---|---|---|
| N1 | `func f() { break }`（break 不在循环里） | `SYN-BREAK-NOT-IN-LOOP` | 后续声明继续 |
| N2 | `goto L` 但无 `L:` 标签 | `SYN-LABEL-NOT-DEFINED` | 同上 |
| N3 | `func f() { L: x := 1; goto L }` 但 L 未用 | `SYN-LABEL-UNUSED`（非 fatal） | 同上 |
| N4 | 递归深度 >1000 层 | `SYN-NESTING-TOO-DEEP` | **不 panic**，返回部分 AST |
| N5 | `//line bad` 格式错 | `SYN-INVALID-LINE-DIRECTIVE` | 后续用默认基准继续 |
| N6 | `//go:unknown` 未知关键指令 | ⚠️ 待裁决（见 §9） | 同上 |

> **N1–N3/N5 的期望值由 oracle 生成**；**N4 是 `H`**（官方无此行为，靠栈增长）。

### 4.3 `//go:` 已知指令清单（冻结）

M2 **解析并挂载**（不生效）以下指令为 `Pragma`：

| 指令 | 出处 | 备注 |
|---|---|---|
| `//go:build` | Spec | 特殊：取 `goVersion`（仅 `top`，`F7`） |
| `//go:linkname` | runtime | |
| `//go:nosplit` | runtime | 效果 M7+ |
| `//go:noescape` | runtime | 效果 M7+ |
| `//go:noinline` | gc | |
| `//go:inline` | gc | |
| `//go:systemstack` | runtime | |
| `//go:nowritebarrier` | gc | |
| `//go:nowritebarrierrec` | gc | |
| `//go:yeswritebarrierrec` | gc | |
| `//go:uintptrescapes` | runtime | |

**未知关键指令的处理**（⚠️ 待裁决，见 §9）：
- [`03-roadmap.md`](../03-roadmap.md) §M2 原文要求「**拒绝未知关键指令**」
- 官方实际行为：**忽略**未知 `//go:` 指令（不报错）
- 两种理解的冲突：若"拒绝"=报诊断 → oracle 对撞会多一条诊断（假红）；若"拒绝"=不挂载（忽略）→ 与官方一致
- **临时处置**：先按"忽略（不挂载）"实现以通过对撞，把"是否报诊断"作为 §9 待裁决项

---

## 5. oracle 口径（F6）

### 5.1 adapter

| 项 | 约定 |
|---|---|
| 基准 | **编译器版** `cmd/compile/internal/syntax/`（同 M1 的 `D-M1-3`） |
| 位置 | `tools/ast-oracle/`（含 `README.md` 记来源/版本/校验值/再生成步骤） |
| 入口 | `main.go`（读源码 → `Parse` → 遍历 AST 输出中立 dump）+ `dump.go`（手写遍历器）+ `gosyntax/export.go`（同包 shim，**官方标识符未导出**，[`M2-design.md`](./M2-design.md) §9） |
| 运行 | 必须在容器内（`scripts/in-container.sh`） |
| 依赖 | 仅标准库 + `go/build/constraint`（`D-M2-1` 失效条件已验） |
| **禁止** | 直接用官方 `Fdump`（靠 reflect，格式不保证，`F14`，Rust 无法复现） |

### 5.2 中立 dump 格式（C6 草案，`D-M2-5`）

> **这是 oracle 对撞的枢纽**（[`M2-design.md`](./M2-design.md) §8.2）。双方（rgoc dumper + oracle dumper）输出**同一格式**，逐行 diff。

```text
File
  PkgName: Name "p"
  GoVersion: "go1.22"
  Decls: [3 entries]
    FuncDecl
      Recv: -
      Name: Name "f"
      TParams: [0 entries]
      Type: FuncType
        Params: [0 entries]
        Results: [0 entries]
      Body: BlockStmt
        Stmts: [1 entries]
          ExprStmt
            Expr: CallExpr
              Fun: Name "println"
              Args: [1 entries]
                Name "i"
    ...
```

**五条格式规则**：

1. **节点名用中立名**（两边共有，如 `ForStmt` / `BinaryExpr` / `ArrayType`）
2. **列表显式标 `[n entries]`**（空列表 = `[0 entries]`）
3. **可选字段用 `-` 占位**（如 `Recv: -`）
4. **位置默认不 dump**（另用 `@` 腿测，`T-A-1x`）；需要时加 `--with-pos` 开关
5. **字面量只 dump 原文**（值不算，M3 折叠）

### 5.3 规范化规则（C6 必须定义，`R1`）

Rust 与 Go 的 AST **组织不同**，C6 必须定义"规范化规则"否则对撞假红：

| 差异点 | 规范化规则 |
|---|---|
| Go `Operation{Op,X,Y}` → Rust `Expr::Unary` / `Expr::Binary` | **统一输出 `Operation`**，一元时 Y 打印 `null` |
| Go `ListExpr` 无直接对应物（Rust 用 `Vec<Expr>`） | **列表节点统一扁平化**：`[a, b, c]` 输出三行 `Name` 而非一个 `ListExpr` 包裹 |
| Go `AssignStmt{Op,Lhs,Rhs}` → Rust `Stmt::Assign{kind}` | **统一输出 `AssignStmt`**，`Op=0 & Rhs=nil` 时标 `IncDec` |
| Go 接口嵌入 vs Rust 嵌入 | 按 Go 原样输出 `Field`（name=None → `Embedded`） |

> **这是 M2 最大的实现风险**（[`M2-design.md`](./M2-design.md) §12 `R1`）。先小语料（smoketest.go）验证再扩全量。

### 5.4 来源标注

| 来源 | 覆盖 | 标记 |
|---|---|---|
| oracle 生成 | 除 `T-A-96`（深度上限）外全部 | `# source: oracle` |
| **手写** | **仅 `T-A-96`**（官方无此行为） | `# source: hand-written` |
| **driver 重算**（门禁级对撞时现算） | `ast` 子命令输出，不落盘 | `# source: rgoc` |

---

## 6. unsupported 清单（F4）

| ID | 不支持项 | 归属 | 本阶段处置 |
|---|---|---|---|
| **U12** | **真实 Go 源码的 parse** | M2 | ✅ **M2 解除**（parse） |
| U11 | `src/internal/types/testdata/` | M4 | 不跑 |
| U8 | `test/fixedbugs/` 全目录 | 后续 | 不跑 |
| U1–U7、U9、U10、U13、U14 | 见 [`M0-tests.md`](./M0-tests.md) §6 | 后续 | 不跑 |

> **M2 不引入新的 unsupported 项**（除上表的外来项）。`//go:` 指令的**效果**不算 unsupported（属"明确不做"，[`M2-design.md`](./M2-design.md) §1.2）。

---

## 7. 验证命令、超时与资源上限（F5）

### 7.1 腿甲（crate 内）

```sh
scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo test -p rgoc-ast'     # AST 类型与遍历
scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo test -p rgoc-parse'   # parser 与对撞
```

### 7.2 腿乙（门禁级）

```sh
scripts/in-container.sh cargo run -p rgoc-driver -- ast <file>   # 产出 AST dump
scripts/check-ast-testdata.sh                                     # testdata 31 文件双向匹配
scripts/check-ast-golden.sh                                       # oracle 对撞逐行 diff
```

### 7.3 四条门禁命令

沿用 [`M1-tests.md`](./M1-tests.md) §7.3 的 G14 规范形态（`fail` 累积 + 末条即判据），**不变**。

### 7.4 资源上限（冻结）

| 项 | 值 | 依据 |
|---|---|---|
| 单文件上限 | **64 MiB** | 沿用 M1（`D-M1-4`） |
| **递归深度上限** | **1000 层** | [`M2-design.md`](./M2-design.md) §7.3 / `R5`：官方靠栈增长，Rust 需显式上限 |
| 腿甲总耗时 | ≤ **120 s** | parser 比 lexer 重，翻倍 |
| 腿乙总耗时 | ≤ **600 s** | 标准库冒烟 185 MB |
| 单文件处理 | ≤ **10 s** | 留余量 |

> **超时的处置**：记 `Timeout` / `ResourceFailure`，**仍在分母里**（`03` §3.3）。

---

## 8. 门禁判定与冻结规则

| 门禁 | 由哪些 ID 判定 | 分母 | 通过条件 |
|---|---|---|---|
| **AST-01..49** | §2 表中全部 | **49 项** | **每项至少一条 ID**，且全部通过 |
| **节点 Pos 规则** | `T-A-10`..`T-A-14` | 5 | 100%，`@` 标记逐条 |
| **testdata 正/负例** | `T-A-95` + §4.1 | 31 文件 | 正例零错 + 负例双向 |
| **oracle 对撞** | `T-A-90`..`T-A-93` | testdata 31 + 自有切片 | diff 全空 |
| **标准库冒烟** | `T-A-94` | `go_source_code/src` 全量 | 零意外错误 |
| **错误恢复** | `T-A-61`..`T-A-66` | 6 | 100%，双断言（诊断 + 后续 AST） |
| **branches** | `T-A-80`..`T-A-82` | 3 | 100% |
| **指令与位置** | `T-A-70`..`T-A-73` | 4 | 100% |
| **`SM-M0-NATIVE-HELLO`** | `T-S3-03`（M0 的） | 1 | **继续通过**；不得改写 |
| **GAP-01** | AST/PIPE-03/04/05/25 四维归属 | 53 项 | 已填 |

**判定纪律**（沿用 [`M0-tests.md`](./M0-tests.md) §8 / [`M1-tests.md`](./M1-tests.md) §8）：

1. 必需 ID **100% 通过**、**无基建失败 / 不稳定失败**（`03` §3.3）
2. 白名单**开工前冻结**，任何新增跳过须**单独记录**并保留原分母报告
3. **不得**通过放宽比较器、把跳过记为 pass、或缩小分母来「达成」门禁
4. **断言必须反向验证** —— 写完一条断言要故意制造它能抓的 bug，确认会变红；
   不红就是恒真断言，必须重写（本仓 T37/T38/T39/T47 连续四轮同形的坑）

> **门禁编号用 `A`**（AST），与 M0 的 `E*`、M1 的 `L*` 区分（[`M2-design.md`](./M2-design.md) §11）。

---

## 9. 待办

| # | 事项 | 归属 | 阻塞性 |
|---|---|---|---|
| 1 | **冻结本文档**（F1–F6 六项） | 用户确认 | **阻塞 M2 开工** |
| 2 | **裁决 `//go:` 未知指令**：拒绝（报诊断）vs 忽略（不挂载） | 用户 | 不阻塞开工（先按忽略实现），但阻塞 `T-A-73` 的最终冻结 |
| 3 | **冻结 `DiagCode` 的 M2 新增条目**：从官方 `syntaxError` 调用点枚举，命名前缀 `SYN-`，总数在 plan 执行期冻结 | plan T-A 首任务 | 不阻塞 tests 冻结（分类已定） |
| 4 | 产出 `M2-plan.md` | 下一步（`writing-plans`） | tests 冻结后 |
| 5 | 填 §3 / §8 里的「⬜ 待确认」标记 | ✅ 已填（2026-10-15，T-AP21） | |
