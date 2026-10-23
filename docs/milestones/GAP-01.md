# GAP-01：功能 ID → 四维归属（M1/M2/M3/M4/M5 进度）

> **范围**：`docs/01-feature-set.md` 共 210 个功能 ID。M1 只判定 **LEX-01..13**，M2 追加判定 **AST-01..49 + PIPE-03/04/05/25**（53 项），M4 追加判定 **TYP-19..26 / EXP / STM / BIF / PKG / SYS 的静态面**（**范围级**归属 + 例外项，见文末 M4 节），M5 追加判定 **PIPE-10/14/15/16/16c/24 + `RT-*` 的执行面**（见文末 M5 节），不填满 210 个（`M0-report.md` §7.2 明确警告）。
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

## 未判定部分

其余功能 ID 待后续里程碑按阶段推进，不在本文件填写。**M5 之后的余额**：TYP/EXP/STM/BIF/PKG/SYS 六族的**逐 ID** 展开（`G-M4-37` 收口专项）、`CON-01..08`（M3 常量体系已实现，逐 ID 归属待补）、`SCP-01..10`（M3 作用域/对象，逐 ID 归属待补）、`RT-MEM/SCH/POLY` 三族（M9 的对象模型与运行时，M5 只登记「不涉及」）、`PIPE-16b`（range over func，M5 显式拒绝，`U-M5-06`）。

