# GAP-01：功能 ID → 四维归属（M1/M2/M3/M4/M5/M6 进度）

> **范围**：`docs/01-feature-set.md` 共 210 个功能 ID。M1 只判定 **LEX-01..13**，M2 追加判定 **AST-01..49 + PIPE-03/04/05/25**（53 项），M4 追加判定 **TYP-19..26 / EXP / STM / BIF / PKG / SYS 的静态面**（**范围级**归属 + 例外项，见文末 M4 节），M5 追加判定 **PIPE-10/14/15/16/16c/24 + `RT-*` 的执行面**（见文末 M5 节），M6 追加判定 **PIPE-17/18/21/26 + GC 元数据协议**（见文末 M6 节），不填满 210 个（`M0-report.md` §7.2 明确警告）。
> **维度定义**：`docs/03-roadmap.md` §0.3 —— Frontend / Interpreter / Native / Runtime-metadata。
> **来源**：`docs/milestones/M1-tests.md` §2 与 `M2-tests.md` §2/§4 的逐项覆盖映射。

## LEX-01..13 四维归属

| 功能 ID | 描述 | Frontend | Interpreter | Native | Runtime-metadata | 测试 ID |
|---|---|---|---|---|---|---|
| LEX-01 | UTF-8 源码表示（BOM / NUL / 非法 UTF-8） | ✅ | — | — | — | T-L-13, T-L-14, T-L-15, T-L-16 |
| LEX-02 | Unicode 字母/数字分类 | ✅ | — | — | — | T-L-11, T-L-12, T-L-19, T-L-24, T-L-25 |
| LEX-03 | 行/块注释 | ✅ | — | — | — | T-L-45, T-L-46, T-L-52, T-L-70 |
| LEX-04 | Token 四分类与最长匹配 | ✅ | — | — | — | T-L-20..T-L-29 |
| LEX-05 | 25 个保留关键字 | ✅ | — | — | — | T-L-22, T-L-30 |
| LEX-06 | 48 条操作符与标点 | ✅ | — | — | — | T-L-28, T-L-31 |
| LEX-07 | 分号自动插入 | ✅ | — | — | — | T-L-40..T-L-48 |
| LEX-08 | 标识符词法 | ✅ | — | — | — | T-L-20, T-L-21, T-L-23, T-L-24 |
| LEX-09 | 整数字面量 | ✅ | — | — | — | T-L-33, T-L-38 |
| LEX-10 | 浮点字面量 | ✅ | — | — | — | T-L-34, T-L-38 |
| LEX-11 | 虚数字面量 | ✅ | — | — | — | T-L-35 |
| LEX-12 | Rune 字面量与转义 | ✅ | — | — | — | T-L-36, T-L-53 |
| LEX-13 | 字符串字面量 | ✅ | — | — | — | T-L-37, T-L-53 |

## 判定理由

- **LEX-01..13 全部归属 Frontend**：它们只决定「字节流如何被切成带位置的词元流」，不产生运行时语义、不生成机器码、不维护 runtime 元数据。
- 字面量的**值**（如 `0x1.8p-2` 的数值）属于 M3 常量体系，不在 M1 判定；因此 LEX-09/10/11 不进入 Interpreter/Native。
- `print`/`println` 内建的行为（stdout/stderr）属于 M5/M7，与 LEX-13 字符串的扫描边界无关。

## AST-01..49 四维归属（M2 / `T-AP21` 判定）

**判定口径**：AST 节点是**纯语法**产物 —— parser 产出它（Frontend），消费它的是
后续里程碑（Interpreter/Native/Runtime-metadata 全为 `—`）。
每个 ID 的 Frontend 证据 = **oracle 对撞**（31 个官方 testdata，`t_collide_*`，
覆盖全部节点形状）+ 该节点的专项单测（`test_nodes.rs`/`test_expr.rs`/
`test_stmt.rs`/`test_type.rs`/`test_decl.rs`，`T-A-01..26` 逐项映射见
`M2-tests.md` §2）。

| 功能 ID | 描述 | Frontend | Interpreter | Native | Runtime-metadata | 测试 ID |
|---|---|---|---|---|---|---|
| AST-01 | `AstFile`（编译单元） | ✅ | — | — | — | T-A-01, T-A-92/93 |
| AST-02 | `Group`（分组声明） | ✅ | — | — | — | T-A-92/93（`const (…)` 组在对撞语料内） |
| AST-03 | `Comment`（4 种位置 + Pragma 通道） | ✅ | — | — | — | T-A-05, T-A-92/93 |
| AST-04 | `ImportDecl` | ✅ | — | — | — | T-A-92/93 |
| AST-05 | `ConstDecl` | ✅ | — | — | — | T-A-92/93 |
| AST-06 | `TypeDecl{Alias, TParamList}` | ✅ | — | — | — | T-A-92/93 |
| AST-07 | `VarDecl` | ✅ | — | — | — | T-A-92/93 |
| AST-08 | `FuncDecl{Recv, TParamList, Type, Body}` | ✅ | — | — | — | T-A-07, T-A-92/93 |
| AST-09 | `BadExpr`（错误恢复占位） | ✅ | — | — | — | T-A-92/93（恢复路径入对撞语料） |
| AST-10 | `Name` | ✅ | — | — | — | T-A-92/93 |
| AST-11 | `BasicLit{Kind}` | ✅ | — | — | — | T-A-08, T-A-92/93 |
| AST-12 | `CompositeLit{NKeys}` | ✅ | — | — | — | T-A-13, T-A-92/93 |
| AST-13 | `KeyValueExpr` | ✅ | — | — | — | T-A-92/93 |
| AST-14 | `FuncLit` | ✅ | — | — | — | T-A-14, T-A-92/93 |
| AST-15 | `ParenExpr` | ✅ | — | — | — | T-A-92/93 |
| AST-16 | `SelectorExpr` | ✅ | — | — | — | T-A-10, T-A-92/93 |
| AST-17 | `IndexExpr` | ✅ | — | — | — | T-A-11, T-A-92/93 |
| AST-18 | `SliceExpr{Index[3], Full}` | ✅ | — | — | — | T-A-92/93 |
| AST-19 | `AssertExpr` | ✅ | — | — | — | T-A-12, T-A-92/93 |
| AST-20 | `TypeSwitchGuard{Lhs}` | ✅ | — | — | — | T-A-92/93 |
| AST-21 | `Operation{Op, X, Y}` | ✅ | — | — | — | T-A-15..17, T-A-92/93 |
| AST-22 | `CallExpr{HasDots}` | ✅ | — | — | — | T-A-18, T-A-92/93 |
| AST-23 | `ListExpr`（多值载体） | ✅ | — | — | — | T-A-19, T-A-92/93 |
| AST-24 | `ArrayType` | ✅ | — | — | — | T-A-20, T-A-92/93 |
| AST-25 | `SliceType` | ✅ | — | — | — | T-A-21, T-A-92/93 |
| AST-26 | `DotsType`（...T） | ✅ | — | — | — | T-A-92/93 |
| AST-27 | `StructType` | ✅ | — | — | — | T-A-22, T-A-92/93 |
| AST-28 | `Field`（统一载体） | ✅ | — | — | — | T-A-23, T-A-92/93 |
| AST-29 | `InterfaceType` | ✅ | — | — | — | T-A-24, T-A-92/93 |
| AST-30 | `FuncType` | ✅ | — | — | — | T-A-25, T-A-92/93 |
| AST-31 | `MapType` | ✅ | — | — | — | T-A-26, T-A-92/93 |
| AST-32 | `ChanType{Dir}` | ✅ | — | — | — | T-A-92/93 |
| AST-33 | `EmptyStmt` | ✅ | — | — | — | T-A-92/93 |
| AST-34 | `LabeledStmt` | ✅ | — | — | — | T-A-27, T-A-92/93 |
| AST-35 | `BlockStmt` | ✅ | — | — | — | T-A-92/93 |
| AST-36 | `ExprStmt` | ✅ | — | — | — | T-A-92/93 |
| AST-37 | `SendStmt` | ✅ | — | — | — | T-A-92/93 |
| AST-38 | `DeclStmt` | ✅ | — | — | — | T-A-92/93 |
| AST-39 | `AssignStmt{Op, Lhs, Rhs}` | ✅ | — | — | — | T-A-28..30, T-A-92/93 |
| AST-40 | `BranchStmt{Tok, Label, Target}` | ✅⚠️ | — | — | — | T-A-80..82（branches）；**Target 刻意不建模**（回指针，`Rc` 破坏深比较；由 M3 的 `checkBranches` 消费）—— 已在 M2 ledger 登记为设计裁定 |
| AST-41 | `CallStmt{Tok, Call, DeferAt}` | ✅⚠️ | — | — | — | T-A-92/93；`DeferAt` 官方 1.27 实验字段，M2 未建模（ledger 残留表） |
| AST-42 | `ReturnStmt` | ✅ | — | — | — | T-A-92/93 |
| AST-43 | `IfStmt{Init, Cond, Then, Else}` | ✅ | — | — | — | T-A-31, T-A-92/93 |
| AST-44 | `ForStmt` | ✅ | — | — | — | T-A-32, T-A-92/93 |
| AST-45 | `SwitchStmt` | ✅ | — | — | — | T-A-33, T-A-92/93 |
| AST-46 | `SelectStmt` | ✅ | — | — | — | T-A-92/93 |
| AST-47 | `RangeClause{Lhs, Def, X}` | ✅ | — | — | — | T-A-34, T-A-92/93 |
| AST-48 | `CaseClause` | ✅ | — | — | — | T-A-92/93 |
| AST-49 | `CommClause` | ✅ | — | — | — | T-A-92/93 |

## PIPE-03/04/05/25 四维归属（M2 / `T-AP21` 判定）

| 功能 ID | 描述 | Frontend | Interpreter | Native | Runtime-metadata | 测试 ID |
|---|---|---|---|---|---|---|
| PIPE-03 | 解析器（parser + 错误恢复 + 二义消解） | ✅ | — | — | — | T-A-92/93（31 文件对撞）、T-A-95（双向 ERROR）、T-A-94（6670 文件冒烟）、T-A-96/98/99 |
| PIPE-04 | 语言版本门控（`-lang` / `//go:build`） | ⚠️ 部分 | — | — | — | `//go:build` 取 `goVersion` 已落（T-A-02, T-AP15）；**`-lang` 拒绝特性未做**（`T-A-97`，`D-M2-6` 残留：`parse_file` 无 `-lang` 入口）→ M3+ |
| PIPE-05 | Pragma/编译指令 | ⚠️ 部分 | — | — | — | `//go:build`/`//line` 已落（T-A-04, T-AP15）；`//go:linkname`/`nosplit`/`noescape` 等**未知指令的接受策略待裁决**（M2 manifest `open_gaps`）→ M3+ |
| PIPE-25 | 诊断系统（位置化错误 + 恢复继续编译） | ✅（M1/M2 部分） | — | — | ⚠️ | C1 排序/格式（T-L-*）+ parser 诊断流（T-A-95 位置匹配）；**多行提示/types2 错误归 M4** |

## M2 判定理由（补 M1 的三条之后）

- **AST-01..49 全部归属 Frontend**：它们只决定「词元流如何被组织成树」，
  不含求值、代码生成或 runtime 语义。字段级语义（如 `Op` 的具体含义）
  由对撞对照官方逐字段钉住（PA-1 的教训：数据模型验收必须对照官方源码逐字段 diff）。
- **AST-40 的 `Target` 与 AST-41 的 `DeferAt` 是仅有的两个「节点在、字段缺」**：
  前者是设计裁定（回指不进 `PartialEq` 的深比较），后者是官方实验字段
  （M2 ledger 残留表登记）。两者都不影响 dump 对撞（C6 不 dump 它们）。
- **PIPE-04/05 标「部分」是如实登记**：M2 只落了**读取侧**
  （`//go:build` → `goVersion`、`//line` → 基准链）；**拒绝侧**
  （按版本拒语法）与完整 pragma 集合归后续里程碑。

## M4 归属落点（TYP-19..26 / EXP / STM / BIF / PKG / SYS 静态面）

> **粒度声明（必读）**：M4 的验收面是按**测试 ID**（63 个 `T-CK-*`）组织的，不是按功能 ID 逐项组织的
> （`M4-tests.md` §2.1 只把 roadmap 的**九句话**映射到 ID 组）。所以本节给的是**范围级**归属 +
> **已知例外项**，**不是** 90 个功能 ID 的逐 ID 表。
> 逐 ID 展开留到 M5 前的收口专项（与 `G-M4-37` 同批）—— 在那里它才有意义：现在逐 ID 打勾会把
> 「诊断通路已通但规则没铺完」写成「已实现」，那是假账。

**判据来源**：`M4-tests.md` §2.1（九大项 ↔ `T-CK-*`）、§7（`unsupported` 18 条）、
`M4-manifest.json` 的 `gate`/`test_ids`、`M4-report.md` §1（前端子集矩阵）与 §4（58 个红的五桶）。

| 功能 ID 范围 | 描述 | Frontend | Interpreter | Native | Runtime-metadata | 测试 ID / 保留 |
|---|---|---|---|---|---|---|
| TYP-19..26 | 类型判定（可赋值/方法集/接口实现/约束满足/类型集同一性/泛型实例化与推断/`comparable`/`convertible`） | ✅ | — | — | — | `T-CK-11..16`、`T-CK-21..25`、`T-CK-81..85`；**`TYP-26` 由 `T-CK-76` 补 fixture**；`U-M3-05` 已解除 |
| EXP-01..26 | 表达式静态面（运算符/转换/索引切片/选择器/断言/复合字面量/`range`） | ◐ | — | — | — | `T-CK-32..38`；**◐ 而非 ✅**：自有 fixture 全绿，但语料 58 红里 `expr0/1/2`、`conversions`、`assignability`、`comparisons` 等仍不一致 |
| EXP-27 | 求值顺序（codegen 面） | — | ○ | ○ | — | **不承诺**（`U-M4-02`）；去向 M6+ |
| STM-01..21 | 语句静态面（赋值/返回/分支/`for`/`switch`/`select`/`go`/`defer`/标签/`goto`） | ◐ | — | — | — | `T-CK-35/36`；**◐**：`stmt0/1`、`labels`、`gotos` 仍红（标签与 `goto` 面缺口） |
| BIF-01..15 | 内建函数**签名与静态约束** | ✅ | ○ | ○ | — | `T-CK-41..43`；**只有静态面**：内建的**运行时行为**归 M5（`U-M4-01`） |
| PKG-01..04、07、08 | 包身份/初始化序/零值完备性/导出可见性 | ✅ | — | — | — | `T-CK-71..75`、`T-CK-76`（`PKG-04` 零值矩阵） |
| PKG-05、06 | 初始化依赖环 / `init` 约束 | ✅ | — | — | — | `T-CK-72`、`T-CK-76`（`PKG-06`）；**只算图与环，不执行** |
| SYS-01..04 | 静态可判定的系统面（布局/尺寸/`unsafe` 白名单） | ✅ | — | — | — | `T-CK-44`；白名单 = `Sizeof`/`Alignof`/`Offsetof` + `unsafe.Pointer` 类型名 |
| SYS-05 | 运行时错误集 | — | ○ | ○ | ○ | **不承诺**（`U-M4-03`）；去向 M8+ |
| PIPE-04（续） | 语言版本门控的**特性**面 | ◐ | — | — | — | `T-CK-61..64`：特性表 31 项 ✅；**语法/词法层未接** ⇒ `go1_8/12/13/16/19/25` 六文件仍 0 行（`G-M4-39`） |
| PIPE-25（续） | 诊断系统（位置化错误 + 类别） | ✅ | — | — | — | `C8` 契约冻结（26 类）+ `T-CK-91/92/94/95`；**消息文本不承诺逐字**（`U-M3-10`） |

**图例**：`✅` 兑现　`◐` 部分（通路通、规则未铺完）　`○` 不承诺（有去向）　`—` 不适用

**M4 节的三条判定理由**：

1. **为什么 `EXP`/`STM` 只给 `◐`**：`C8` 渲染、类别投影、双腿装置全绿 ⇒ 通路是通的；但入选 75 里
   `EXP`/`STM` 相关文件仍有 20+ 个不一致（`M4-report.md` §4.2 的「过量 21 / 不足 19」主要落在这两族）。
   标 `✅` 就是把「装置绿」冒充「规则全」。
2. **为什么 `BIF` 只有 Frontend**：内建的**签名与静态约束**（参数个数/类型、常量折叠）属前端；
   内建的**行为**（`append` 的扩容、`copy` 的重叠、`panic` 的传播）是 M5 的执行语义 —— 这与
   `M4-report.md` §2 的三条分别声明同一条纪律。
3. **为什么 `SYS-05` 与 `EXP-27` 直接 `○`**：它们在 `M4-tests.md` §7.2 的 `unsupported` 里各有编号
   （`U-M4-03`、`U-M4-02`），并且**有正向断言**证明它们被显式拒绝/走文档约定路径 —— 白名单项
   **不得**记为 pass（§7 纪律），所以这里也只能是 `○`。

## M5 归属落点（`PIPE-10/14/15/16/16c/24` + `RT-*`，`M5-plan.md` Task 52）

**判定口径**：M5 的产物是**解释器**，所以「降级 / 转换 / 内建」这些在 native 世界里
属于中端的活，在 M5 一律落 **Interpreter** 维度 —— 它们真的被执行了（不是「有形状没实现」）。
Frontend 列只在「语义判定发生在前端」时打 ✅；Native / Runtime-metadata 列基本是 `—`，
因为 M5 不生成机器码、不维护运行时元数据。

| 功能 ID | 描述 | Frontend | Interpreter | Native | Runtime-metadata | 测试 ID |
|---|---|---|---|---|---|---|
| PIPE-10 | IR 树构建（AST→IR） | ✅ | ✅ | — | — | `T-EX-01..04`（45 变体穷尽 / dump 确定性 / Place 五形态）；`rgoc-hir/src/lower.rs` |
| PIPE-14 | 闭包转换（捕获分析 → 按值/按引用） | ◐ | ✅ | — | — | `T-EX-25/26`：捕获的是**变量**（`Closure.captured: Vec<Cell>`），不是 context struct ⇒ Native 的 context struct 仍归 M7 |
| PIPE-15 | 方法与接口降级 | — | ✅ | — | — | `T-EX-28/29/30/31`：方法值（`recv` 绑定）/ 方法表达式（首参 = 接收者）/ `Callee::Iface` 动态派发 / 断言 / type switch；**itab 未做**（Native `—`） |
| PIPE-16 | 内建函数降级 | — | ✅ | — | — | `T-EX-23`：`BuiltinKind` 全集在解释器里直接实现（**不**生成 runtime 调用） |
| PIPE-16c | 循环变量语义（1.22） | — | ✅ | — | — | `T-EX-27`：每轮**新建**变量（新 `Cell`）⇒ `012`；`PIPE-16b`（range over func）**显式拒绝**（`U-M5-06`） |
| PIPE-24 | 包初始化与启动运行时 | — | ◐ | — | — | `T-EX-39`：单包 `InitOrder` + `init` 序 + `main` ✅；**双包顺序未兑现**（`G-M5-02`）⇒ `◐`；`rt0`/调度器初始化归 M9（`—`） |
| RT-CMP-01 | Slice 头结构 | — | ✅ | — | — | `C9` 的 `SliceVal { arr, off, len, cap }` + `T-EX-13`（别名 / 增长脱钩 / 三索引 / 重叠 copy） |
| RT-CMP-02 | String 头结构 | — | ◐ | — | — | `C9` 的 `Value::Str(Rc<str>)` 是**不可变 UTF-8**，不是 `{ptr,len}` 字节串 ⇒ 非 UTF-8 字面量不保真（`U-M5-14`） |
| RT-CMP-03/04 | Map 桶算法 / 迭代随机化 | — | ◐ | — | — | `T-EX-14`：`MapObj` 是 `Vec` + **插入序**；**刻意不追** RT-CMP-04 的随机化（`U-M5-04`），键相等用 `go_eq` |
| RT-CMP-07 | 数组/结构体表示（值语义 / 零大小统一 0 地址） | — | ✅ | — | — | `T-EX-11/12`（`deep_copy` 正反）+ `C9` `I6`（零尺寸聚合的指针身份） |
| RT-MEM-01..08 | 分配器 / GC / 栈管理 | — | — | — | — | **M5 不涉及**：生命周期归宿主 `Rc`，没有 GC、没有栈增长 |
| RT-SCH-01..05 | `g`/`m`/`p` / 上下文切换 / `go` 语句 | — | — | — | — | **M5 不涉及**：`go` 语句在 lowering 就 `HIR-UNSUPPORTED-GO` 显式拒绝（`U-M5-01`） |
| RT-POLY-01.. | `_type` 元数据 / itab | — | — | — | — | **M5 不涉及**：动态派发用 `TypeId` + 方法表查表，不需要 `_type` 元数据 |
| （RT-EXC） | 异常 runtime | — | ✅ | — | — | **`01-feature-set.md` 没有定义 `RT-EXC-*` 编号**（只在第 35 行的命名约定里出现过）。异常语义的 ID 落点是 `STM-20`（defer）+ `BIF-12/13`（panic/recover），执行面在 `interp.rs` 的 `panic_val` + `unwinding` 状态机（`T-EX-34..38`） |

**判定理由**：

1. **为什么 PIPE-14/15/16 全落 Interpreter 而不是 Frontend**：这三项在 native 世界里是
   「把高层构造翻译成低层构造」；M5 的翻译目标是**值模型 + 显式帧**，翻译结果直接被解释器执行。
   写成 Frontend 会让人以为「只是 AST 变换」——而它们的语义（捕获按变量、方法值绑定接收者、
   `append` 扩容阈值）是**运行时行为**，只有执行才能证明。
2. **为什么 PIPE-24 是 `◐` 而不是 ✅**：单包 init 序已兑现且有 fixture，但
   `import _ "side"` 不跑 side 的 `init` —— 这是**真缺口**（`G-M5-02`），不能因为「单包绿了」就记 ✅。
3. **为什么 `RT-*` 大面积 `—`**：M5 的运行时就是宿主（Rust 的 `Rc`/`Vec`）。
   把 `RT-MEM-*` 记成「解释器已覆盖」是**把宿主的能力算成自己的** —— 那是 M4 报告里
   「弱绿不算绿」的同一条纪律。
4. **`RT-EXC` 不存在**：`01-feature-set.md` 第 35 行只是**命名约定**（`RT-{MEM,SCH,CMP,POLY,EXC}-nn`），
   实际只定义了 MEM/SCH/CMP/POLY 四族。这里如实写「无此 ID」，而不是造一个编号来填表。

## M6 归属落点（`PIPE-17/18/21/26` + GC 元数据协议，`M6-plan.md` Task 52）

**判定口径**：M6 的产物是**第二条执行通路**（HIR → SSA → 求值）与它的可验证性。
`PIPE-17/18` 落 **Frontend**（图在编译期建、在编译期优化），`PIPE-21` 只落
**Runtime-metadata 的协议面**（值级活跃性 + safepoint live set，**不是**栈帧布局），
`PIPE-26` 落 **Frontend**（dump 是开发期设施，不产生运行期语义）。

| 功能 ID | 描述 | Frontend | Interpreter | Native | Runtime-metadata | 测试 ID |
|---|---|---|---|---|---|---|
| PIPE-17 | SSA 构建（块/值/Phi） | ✅ | ◐ | — | — | `T-SSA-15..30`（`ssa_build.rs` **76** 条）；phi = **块参数**（`D-M6-3`）；**Interpreter ◐** 是因为 SSA 能被求值（`ssa_exec`），但那是诊断设施不是交付路径 |
| PIPE-18 | SSA 优化 passes（折叠/DCE/复制传播/CSE） | ✅ | — | — | — | `T-SSA-60..64`（`opt_*` **32** 条 + `opt_equiv` 46 个 `all==none`）；**窥孔（peephole）未做** ⇒ 与 M7 的指令选择一起（`G-M6-04` 同源） |
| PIPE-21 | 栈帧布局与活跃性 | ◐ | — | — | ◐ | **只做活跃性，不做帧布局**：`T-SSA-55..59`（`live.rs` 6 条 + `live_consumer` 46 文件/140 函数/678 safepoint）；`SSA-LIVE v1` 是**协议**，栈图/变量偏移/逃逸位图归 M7（`PIPE-21` 的 Native 列仍 `—`） |
| PIPE-26 | 阶段调试 dump | ✅ | — | — | — | `T-SSA-13/14/46`（`C10` 定稿 + 文档驱动 golden）；`rgoc ssa --dump` |
| （GC 元数据） | GC roots / live set / stack map / safe point | — | — | — | ◐ | `01-feature-set.md` 没有给这一族独立编号（只在 `RT-MEM-06` 的描述里提到「GC 栈扫描联动」）。**M6 只交协议**：safepoint 定义（`Call` + `Make`）、根闭包保守策略、`ptrs` 只含 `*T`（`G-M6-02/03`）；真正的栈图归 M7 `PIPE-21`，扫描器归 M10 |

**判定理由**：

1. **为什么 `PIPE-17` 的 Interpreter 是 `◐` 而不是 ✅**：SSA 求值器是**诊断设施**
   （roadmap 原文：「共用定义好的值与 runtime 操作，不发展第三套完整 runtime」）。
   把「能被求值」记成「解释器已覆盖 SSA」会让人以为 M7 可以走解释器 —— 而 M7 必须生成机器码。
2. **为什么 `PIPE-21` 两列都是 `◐`**：活跃性真的算了（值级、含回边、有 678 个 safepoint 的实测），
   但**帧布局没做**（没有帧大小、没有变量偏移、没有逃逸位图）。半件事记 `◐`，不记 ✅。
3. **为什么 `PIPE-18` 不记窥孔**：`peephole` 是**机器相关**的（要在指令选择之后才有意义），
   M6 的 IR 还是中立的 —— 现在写等于给一个不存在的后端做优化。
4. **为什么 GC 元数据不给编号**：`01-feature-set.md` 没有这一族的 ID。**如实写「无此编号」**，
   而不是造一个来填表（沿 M5 节对 `RT-EXC` 的同一处理）。

## 未判定部分

其余功能 ID 待后续里程碑按阶段推进，不在本文件填写。**M6 之后的余额**：`PIPE-19/20/22/23`（机器相关 lowering / 指令选择与寄存器分配 / 对象文件与元数据 / 链接器，归 M7）、`PIPE-21` 的**帧布局与栈图**部分（M7）、GC 元数据的**扫描器**（M10）、`PIPE-16b`（range over func）与 M6 的 9 条白名单（闭包/defer/接口动态/map/chan/泛型/函数值/range-str，M7–M9）。**M5 遗留的余额**：TYP/EXP/STM/BIF/PKG/SYS 六族的**逐 ID** 展开（`G-M4-37` 收口专项）、`CON-01..08`（M3 常量体系已实现，逐 ID 归属待补）、`SCP-01..10`（M3 作用域/对象，逐 ID 归属待补）、`RT-MEM/SCH/POLY` 三族（M9 的对象模型与运行时，M5 只登记「不涉及」）、`PIPE-16b`（range over func，M5 显式拒绝，`U-M5-06`）。


## M7 节：native 代码生成（2026-10-25）

> **口径**：PIPE 族的这四个 ID 是 M7 的全部范围。表格只写「做到什么程度」，
> 判据在 `M7-manifest.json` 的 `gate`（`NAT-1..9`）与 `M7-tests.md` 的 `T-NAT-*`。

| ID | 能力 | M7 后状态 | 说明 |
|---|---|---|---|
| `PIPE-19` | 机器相关 lowering | ✅ | 14 个 `Instr` + 5 个 `Term` 逐变体有指令选择（`isel.rs`），`match` **无通配**；`Slice` 走拒绝面（`CG-UNSUPPORTED-SLICE`）。`Instr::Copy` / `Term::Unreachable` **没有生产者**（`R-13-7`），所以只有 17 个变体能被测 |
| `PIPE-20` | 指令选择与寄存器分配 | ◐ | **指令选择做了**（`isel`）；**寄存器分配没做** —— 每个值固定一个栈槽（`D-M7-3`），`x0..x7`/`v0..v7` 只作调用参数与指令临时。分配质量归 M9 |
| `PIPE-21` | 帧布局与栈图 | ✅ | `frame::layout`（值槽/取址槽/临时区/outgoing 区/saved 对）+ `FRAME-MAP v1` 真产物（`<out>.framemap`）+ 与 **objdump 反汇编**双向对账（`native_frame_map`）。**残留**：safepoint 的活值覆盖只在手搓 IR 上验过，未在语料上逐点验 |
| `PIPE-22` | 对象文件与元数据 | ◐ | 产物是**文本 `.s`**，交给 `clang` 汇编+链接（`D-M7-1`）；自己不做 ELF 写出、不做 `.o` 重定位。`.s` 与 ELF 的**逐字节可复现**已断言 |
| `PIPE-23` | 链接器 | ◐ | 不自研链接器：`clang -nostartfiles -Wl,-s` + 我们自己的 `_start`（`D-M7-1`）。零 libc 运行时（`svc #0`） |
| `PIPE-21`（GC 元数据扫描器） | 扫描器 | ✗ | **不做**，归 M10。M7 只交「栈图上哪些偏移是活指针」的表 |

**三条口径说明**

1. **`PIPE-20` 与 `PIPE-23` 记 `◐` 而不是 ✅**：roadmap 的 ID 覆盖的是「整件事」，
   而 M7 有意只做了前半（选择 + 交给现成工具链）。记 ✅ 会让人以为 M7 有分配器、
   有链接器 —— 那时 `M9` 的路线就得重排。
2. **`PIPE-22` 记 `◐`**：`D-M7-1` 明确「不自研 assembler/linker」。产物是文本汇编，
   所以「对象文件」这件事实际上是 clang 替我们做的。
3. **`PIPE-21` 记 ✅ 但有残留**：帧布局与帧图都在（含 objdump 对账），
   而「safepoint 活值在语料上逐点验」没做 —— 这一条写在 `M7-manifest.json`
   的 `NAT-7` 的 `gap` 字段里，不在这里重复。

**M7 之后的余额**：`PIPE-20` 的寄存器分配与 spill（M9）、`PIPE-22/23` 的自研后端（不在路线图上，
`D-M7-1` 明确放弃）、GC 元数据的**扫描器**（M10）、聚合**按值形参**（`G-M7-08`，M6 的表示问题）、
`U-M7-01` 的切片全集（M9b）。

---

## M8 节：并发解释执行（2026-10-25）

> **口径**：`RT-SCH` 族的 8 个 ID 是 M8 的全部范围。表格只写「做到什么程度」，
> 判据在 `M8-manifest.json` 的 `gate`（`CON-1..9`）与 `M8-tests.md` 的 `T-CON-*`。
> **M8 没有新 crate** —— 并发落在既有 `rgoc-hir` / `rgoc-interp` / `rgoc-check`。

| ID | 能力 | M8 后状态 | 说明 |
|---|---|---|---|
| `RT-SCH-01` | 协程控制块 Task | ✅ | `rgoc-interp` 的 `G`（`frames` / `eval` / `values` / `targets` / `pending` / `phase` / `blocked` / `waiters` / `select_regs`）+ `Gid`。**不是** OS 线程上的 g，是解释器里的显式帧栈持有者 |
| `RT-SCH-02` | 有栈上下文切换 | ✗ | **不做**。M:1 单线程 + 协作式让出，没有「保存/恢复被调用者保存寄存器」这回事 —— 切换点就是 Rust 函数返回。这是**有意**的：有栈切换会让「同 seed 同结果」不可实现（`D-M8-13`） |
| `RT-SCH-03` | GMP 模型 | ✗ | **不做**。一个 M 都没有：全局就绪队列是 `VecDeque`，没有 P、没有 work-stealing、没有 `schedt`。归 M9d（若路线图仍要） |
| `RT-SCH-04` | `go` 语句实现 | ✅ | `f` 与**全部实参**在**当前** G 求值，求值完把帧搬进新 G（`STM-14`）。判据 `T-CON-02`：`go f(g())` ⇒ `g` 的打印在前 |
| `RT-SCH-05` | 阻塞点调度 | ✅ | `ChanObj` 的 `recvq`/`sendq` + `Waiter{g, token, slot, done}`；四个操作（`try_recv`/`try_send`/`close`/`len`/`cap`）+ 登记/撤销。判据 `T-CON-11..24` |
| `RT-SCH-06` | 生命周期与调度循环 | ◐ | 调度循环（`drive`：FIFO 取、跑到 Blocked/Exited、`main` 一回收就**整个进程结束**）与退出语义都在；**不等其它 goroutine** 是有记录的窄化（官方会等） |
| `RT-SCH-07` | 抢占与安全点 | ✗ | **不做**（`U-M8-01`）。守卫判据 `T-CON-07`：goroutine 里 `for {}` ⇒ 预算烧完退出 2，**不**切走 |
| `RT-SCH-08` | 定时器子系统 | ✗ | **不做**（`U-M8-05`）。无 stdlib ⇒ `time.Sleep` / select 超时在 import 期就失败 |

**三条口径说明**

1. **`RT-SCH-02/03` 记 ✗ 而不是 ◐**：它们不是「做了一半」，是**整件事都不做** ——
   而这是 M8 的第一条硬规则（M:1 不抢占）的直接后果。记 ◐ 会让人以为有一条
   通往「部分有栈切换」的路。
2. **`RT-SCH-06` 记 ◐**：调度循环本身完整（含死锁判定与退出码三分），差的是
   「main 返回时等不等其它 goroutine」这一个语义点 —— 我们不等，官方等。
3. **`RT-SCH-01` 记 ✅ 但形状与官方不同**：官方的 `g` 有真栈与汇编切换点；
   我们的 `G` 是显式帧栈的持有者。同名不同物，读官方 runtime 时不要对号入座。

**M8 之后的余额**：`RT-SCH-02/03`（有栈切换与 GMP，M9d 若仍要）、
`RT-SCH-07`（抢占）、`RT-SCH-08`（定时器）、`U-M8-03` 的 range-over-func
`return`/`goto`、`U-M8-08` 的 goroutine panic 栈。

---

## M9 节：native 对象模型、接口与 metadata（2026-10-26）

> **口径**：`RT-MEM` / `RT-CMP` / `RT-POLY` / `RT-EXC` 四族里属于「对象模型与
> metadata」的 ID 是 M9 的范围（`M9-design.md` §7 的对照表）。这里只记**实现后的**
> 状态，与设计稿的不同之处逐条标出。
> 判据在 `M9-manifest.json` 的 `gate`（`MEM-1..SCH-1`）与 `M9-tests.md` 的各族 `T-*`。

| ID | 能力 | M9 后状态 | 说明 |
|---|---|---|---|
| `RT-MEM-01` | 多级分配器 | ✗ | `U-M9-02`：只有 bump arena（16 MiB 起、mmap chunk 链、上限 256 MiB） |
| `RT-MEM-02` | 小/大对象路径 | ✗ | 同上；没有 size-class |
| `RT-MEM-03` | 栈内存管理 | ✗ | 归 M11（native 的 goroutine 栈）。M9 只做「超大帧走 mmap」（`R-9-22`） |
| `RT-MEM-04/05` | GC（STW / 并发） | ✗ | 归 M10。M9 交的是**根协议**（`STACK-MAP` + `RtFuncInfo` + `rt_globals`） |
| `RT-MEM-06` | 堆位图与指针元数据 | ✅ | `TypeDesc.gcdata` + `ptr_mask`（同一函数供 `desc` 与 `live` 用，`R-9-31`）|
| `RT-MEM-07` | 统一分配入口 | ✅ | `rt_alloc(desc, need_zero)`；对象头 8 字节（`D-M9-1`）|
| `RT-MEM-08` | 内存统计 | ◐ | 只报 `used`/`limit`（`RGOC_HEAP_MAX`），不做 `HeapAlloc`/`HeapObjects` 细分 |
| `RT-CMP-01` | Slice 头 | ✅ | `{ptr,len,cap}`；`growslice` 的**拷贝**是 `M-M9-03` 的靶子 |
| `RT-CMP-02` | String 头 | ✅ | M7 已有；M9 补 `string(rune)` 与拼接边界、以及**引用型打印**（`R-9-23`）|
| `RT-CMP-03` | Map 桶算法 | ◐ | 开放寻址 + 线性探测 + `tomb`；**无渐进搬迁**（`U-M9-08`）。判据缺口见 `G-M9-04`（`M-M9-04` 是绿的）|
| `RT-CMP-04` | Map 迭代随机化 | ✅ | 迭代器按槽状态判（「已删不产出」）；顺序**不承诺**（连解释器都固定插入序）|
| `RT-CMP-05` | Channel 结构 | ◐ | 只交 `hchan` 布局（`C11` §7，96 B）与状态机影子；**执行归 M11**（`U-M9-07` 的守卫钉住「代码里没有 `rt_go`/`rt_select`」）|
| `RT-CMP-06` | Channel 语义全套 | ✗ | 归 M11。解释器侧的那一套在 M8（`C11` §3）|
| `RT-CMP-07` | 数组/结构体表示 | ✅ | 值语义留栈 + `zerobase`；**零大小值**的读写判据修正见 `R-9-30` |
| `RT-POLY-01` | `_type` 元数据 | ✅ | `TypeDesc` 40 B（`C5` §11.1）|
| `RT-POLY-02` | 接口值布局 | ✅ | `{tab|type, data}` 两字；`ptr_mask` 两个字都标根 |
| `RT-POLY-03` | itab 动态分派 | ◐ | 静态 ITab（`(iface, concrete)` 对）+ `rt_itab` 兜底；**无哈希缓存** |
| `RT-POLY-04` | 断言 / type switch | ✅ | `x.(T)` 比 desc；`x.(J)` 走 `rt_itab`。**nil 接口**的两个坑（段错误 / 静默成功）在 `R-9-30` 修掉 |
| `RT-POLY-05` | 泛型分派 | ✗ | 继续全量单态化（`U-M9-05`）；`HIR-UNSUPPORTED-GENERIC` + 退 1 |
| `RT-POLY-06` | 反射元数据 | ✗ | 归 M12（`U-M9-04`）：`TypeDesc` 只有 `name_hash`，没有名字 |
| `RT-POLY-07` | method value 闭包化 | ✅ | **实现与设计稿不同**：`MakeClosure{fn: 合成包装函数, captures: [接收者盒子]}`，盒子用 `new(T)+Store`（`box_new`）而不是 1 元素 slice —— 后者对 16 字节接口元素只落半个值（`R-9-30`）|
| `RT-EXC-01` | defer 链表 | ◐ | 全局链（`rt_defer_push` 插链头 / `rt_deferrun` 从链头弹）；**非** open-coded（`U-M9-09`）|
| `RT-EXC-02` | panic 级联展开 | ✅ | **返回码逐层传播**（`D-M9-12`）：`rt_panic_*` 只置状态，每个调用/断言/越界点后 `bl rt_panicking; cbnz`；体积代价实测 **0.49%**（`M9-report.md` §1.1②）|
| `RT-EXC-03` | recover 拦截器 | ✅ | 仅 defer 直接函数体内有效；跑 defer 期间 panic **挂起** |
| `RT-EXC-04` | 运行时致命错误 | ◐ | 四条检查（除零/越界/nil/nilmap）+ **`out of memory`**（`C9` §6.1，`R-9-39`）；不做栈溢出与并发 map 写。判据缺口 `G-M9-06` |
| `RT-EXC-05` | 栈展开与 traceback | ✗ | 归 M12（`U-M9-06`）：panic 只有一行，不打栈 |
| `RT-EXC-06` | 编译器协同 | ✅ | **实现与设计稿不同**：没有独立的 `panic_check` pass —— 检查点由 `isel` 在每个安全点内联（`check_panic`），因为「哪些指令之后要查」是 ABI 知识，不是图的性质 |
| `PIPE-13` | 逃逸分析 | ✗ | **不做优化**（roadmap 明写）；只有「取址且逃出 ⇒ arena」的保守判定（`R-13-5` / `escaping_slots`）|

**四条口径说明**

1. **`RT-MEM-01/02` 记 ✗**：不是「做了一半」，是**整件事不做** —— 只有 bump arena。
   把它记 ◐ 会让人以为有分层分配的路。
2. **`RT-CMP-05` 记 ◐ 而不是 ✅**：M9 只交**布局与状态机影子**（`Hchan` + `ChanState`），
   一个字节的可执行代码都没有；`t_sch_04` 就是钉这一点的守卫。
3. **两条「实现与设计稿不同」**（`RT-POLY-07` / `RT-EXC-06`）：设计稿写的是形状，
   落地时形状变了 —— 原因分别写在括号里，改的是**怎么做到**，不是**做到什么**。
4. **三个判据缺口**（`G-M9-04/05/06`）都是「变异改坏之后判据不红」：它们比
   「没实现」更值得记，因为「绿的」看起来像有据。

**M9 之后的余额**：`RT-MEM-01/02`（分层分配，可能永不）、`RT-MEM-03/04/05`（栈与 GC，M10/M11）、
`RT-CMP-03` 的渐进搬迁、`RT-CMP-05/06` 的 native 执行（M11）、`RT-POLY-05/06`（泛型与反射，M12）、
`RT-EXC-05`（traceback，M12）、`PIPE-13`（逃逸优化）。
