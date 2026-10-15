# GAP-01：功能 ID → 四维归属（M1/M2 进度）

> **范围**：`docs/01-feature-set.md` 共 210 个功能 ID。M1 只判定 **LEX-01..13**，M2 追加判定 **AST-01..49 + PIPE-03/04/05/25**（53 项），不填满 210 个（`M0-report.md` §7.2 明确警告）。
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

## 未判定部分

其余 144 个功能 ID（CON/TYP/SCP/EXP/STM/BIF/PKG/SYS 及其余 PIPE）待 M3 起按阶段推进，不在本文件填写。

