# M2 AST 与 parser · 实施计划

> **状态**：**已完成**（T-AP01..21 全落地，2026-10-15）　|　**测试清单**：`M2-tests.md`（已冻结验收）　|　**机器可读事实**：[`M2-manifest.json`](./M2-manifest.json)
>
> 2026-10-16 补记：全部 Step 复选框补勾（此前只翻了头部状态行，任务级 Step 漏勾）。T-AP01..16 的交付落 `d279ae4`，T-AP17/18 落 `19a88f2`，T-AP19 落 `a7c1111`，T-AP20 落 `fb93ef4`，T-AP21 落 `37ef6e5`；唯一 deferred = T-A-97 版本门控（见 manifest open_gaps）。
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 `rgoc-ast` + `rgoc-parse` —— 把 M1 吐出的线性词元流重建为 AST 树，并接住 M1 推迟的位置重映射、错误恢复、指令解析；覆盖 AST-01..49。

**Architecture:** 两个新 crate 分层：`rgoc-ast`（纯类型定义 + 中立 dump 器，不含 parser 逻辑）+ `rgoc-parse`（手写递归下降 parser）。parser 骨架对齐官方 `cmd/compile/internal/syntax/parser.go`，单前瞻 `tok`、状态 `top`/`fnest`/`xnest`、错误恢复 `advance`+`stopset`。验收靠 oracle 对撞（vendor 官方 syntax，双方输出中立 dump 逐行 diff）。

**Tech Stack:** Rust（edition 2024，`unsafe_code = "forbid"`）、cargo workspace、容器内构建（`scripts/in-container.sh`）、Go 1.27.1 作 oracle（vendor `cmd/compile/internal/syntax`）。

**Spec:** [`M2-design.md`](./M2-design.md)（决策 D-M2-1~7、事实基线 F1–F19）
**测试清单:** [`M2-tests.md`](./M2-tests.md)（67 个 T-A ID、四条腿判据）

---

## Global Constraints

以下每条都是**全局要求**，每个任务隐含包含。**G1–G15 沿用 [`M1-plan.md`](./M1-plan.md) 不复述**（容器/门禁/unsafe/位置/命名/mtime/门禁命令形态/反验脚本六要求），仅列 M2 新增与修订：

| # | 约束 | 出处 |
|---|---|---|
| **G5M** | **依赖方向 `rgoc-lex → rgoc-ast → rgoc-parse`**：`rgoc-ast` 只依赖 `rgoc-lex`（取 `Pos`/`FileId`）；`rgoc-parse` 依赖 `rgoc-ast` + `rgoc-lex`；**反向依赖**（driver → ast/parse）允许 | `M2-design.md` §4 / `D-M2-4` |
| **G16** | **`rgoc-ast` 不含任何 parser 逻辑** —— 它是纯数据 + 遍历 + dump。任何"怎么解析"的代码都在 `rgoc-parse` | `M2-design.md` §4 |
| **G17** | **`rgoc-parse` 不重复定义 AST 类型** —— 只 `use rgoc_ast::*` | `M2-design.md` §4 |
| **G18** | **类型节点落在 `Expr` 族** —— 不新建 `TypeExpr` 枚举（对齐官方"类型 = 表达式"）；`AST-24..32` 的映射列 `Type::Array` 是语法标注，实现为 `Expr::ArrayType` 等 | `M2-design.md` §3.1/§3.4 |
| **G19** | **官方无 `BadStmt`/`BadDecl`** —— `Expr` 用 `Expr::Bad(BadExpr)`；`Stmt`/`Decl` 错误返回 `Option`（`None`） | `M2-design.md` §3.1 |
| **G20** | **错误消息必须复刻官方** —— testdata 的 `// ERROR` 匹配消息子串（`F11`），与 M1 的 `D-M1-9`「不复刻消息」**不同** | `M2-design.md` §5.4 |
| **G21** | **递归深度上限 1000 层** —— 官方靠栈增长，Rust 需显式上限，超限报 `SYN-NESTING-TOO-DEEP` 不 panic | `M2-tests.md` §7.4 |
| **G22** | **中立 dump 是对撞的唯一中介** —— rgoc dumper 与 oracle dumper 输出同格式（C6），不得用官方 `Fdump`（靠 reflect，`F14`） | `M2-design.md` §8.2 / `D-M2-5` |

> **cargo 命令形态**、**门禁命令形态**、**反验脚本六要求**、**mtime 陷阱**全部沿用 `M1-plan.md` G11/G13/G14/G15，不再复述。

---

## Review Focus

以下五类是**设计里蕴含、但没有任务的测试直接覆盖**的输入 ——
它们最可能在真实使用中咬人。**每一行都必须在下面指定的任务里加一条测试钉住。**

| # | 输入 / 条件 | 合理的人会期待什么 | 钉在哪个任务 |
|---|---|---|---|
| RF1 | `if x == T{} {}`（complit 二义） | `xnest` 正确消解：header 里禁用 complit ⇒ `T{}` 不被当复合字面量 | **T-AP11** |
| RF2 | `a + b * c`（优先级爬升） | 结合形状与 oracle 一致（`*(b,c)` 在 `+(a,*)` 下，不是左结合的 `+(*(a,b),c)`） | **T-AP10** |
| RF3 | `//line foo.go:10` 后的节点 Pos | 指向 `foo.go:10` 而非原文件；重映射后诊断也指向新基准 | **T-AP15** |
| RF4 | 错误恢复后的**剩余 AST** | 不仅"报了错"，报错**之后**的 AST 仍与 oracle 一致（双断言） | **T-AP06** |
| RF5 | `ListExpr` 在对撞中的规范化 | 列表扁平化后两边输出一致，不因 Rust 用 `Vec` vs Go 用 `ListExpr` 假红 | **T-AP04** |

---

## 阶段 A：rgoc-ast 基础设施

### Task T-AP01: 创建 `rgoc-ast` crate + 辅助类型

**Files:**
- Create: `rgoc/crates/rgoc-ast/Cargo.toml`、`rgoc/crates/rgoc-ast/src/lib.rs`、`rgoc/crates/rgoc-ast/src/aux.rs`
- Modify: `rgoc/Cargo.toml`（members 加 `crates/rgoc-ast`）
- Test: `rgoc/crates/rgoc-ast/tests/test_aux.rs`

**Interfaces:**
- Consumes: `rgoc-lex::pos::{Pos, FileId}`、`rgoc-lex::token::TokenKind`
- Produces: `Field`、`Group`、`Comment`、`CommentKind`、`Pragma`、`ChanDir`、`LitKind`、`Op`、`BranchTok`

- [x] **Step 1: 写失败测试** —— 构造 `Field{name:None, type_:Expr::Bad(...), pos}`、`ChanDir::SendOnly`（=1）、`CommentKind::Above`（=0）、`Group{..}` 各一个，断言字段可读。`test_aux.rs` 引用 `T-A-05`。
- [x] **Step 2: 跑确认失败** —— `cargo test -p rgoc-ast`（crate 不存在）
- [x] **Step 3: 实现 `aux.rs`** —— 按设计文档 §3.2 的辅助结构表。`Op`/`LitKind`/`BranchTok` 可从 `rgoc-lex` re-export 或在本 crate 重定义（M2 决定：parser 侧需要的运算符优先级等在本 crate 定义，不依赖 lexer 的 `OpForm`）。
- [x] **Step 4: 跑确认通过** —— `scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo test -p rgoc-ast'`
- [x] **Step 5: Commit** —— `feat(ast): rgoc-ast crate skeleton + auxiliary types`

### Task T-AP02: `Decl` enum + `AstFile` 顶层 + `SimpleStmt` enum

**Files:**
- Create: `rgoc/crates/rgoc-ast/src/decl.rs`、`rgoc/crates/rgoc-ast/src/stmt.rs`
- Test: `rgoc/crates/rgoc-ast/tests/test_decl.rs`

**Interfaces:**
- Consumes: T-AP01 的辅助类型
- Produces: `AstFile`、`Decl`（五变体）、`ImportDecl`/`ConstDecl`/`TypeDecl`/`VarDecl`/`FuncDecl`、`SimpleStmt`（五变体）

- [x] **Step 1: 写失败测试** —— 构造 `AstFile{pos, pragma, pkg_name, decls, eof, go_version}`；`Decl` 五变体各构造一个（`T-A-00`/`T-A-01`/`T-A-04`）。`SimpleStmt` 五变体各构造一个。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现 `decl.rs`** —— 按 `M2-design.md` §3.1。`FuncDecl` 含 `recv: Option<Field>`/`tparams: Vec<Field>`/`type_: Expr`/`body: Option<BlockStmt>`。`SimpleStmt` 独立 enum（`Empty`/`Expr`/`Send`/`Assign`/`RangeClause`）。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(ast): Decl enum + AstFile + SimpleStmt`

### Task T-AP03: `Expr` enum（全变体含类型族）+ `Stmt` enum

**Files:**
- Create: `rgoc/crates/rgoc-ast/src/expr.rs`、`rgoc/crates/rgoc-ast/src/stmt.rs`（续）
- Test: `rgoc/crates/rgoc-ast/tests/test_expr.rs`、`tests/test_stmt.rs`

**Interfaces:**
- Consumes: T-AP01/T-AP02
- Produces: `Expr`（全部变体，含类型族 `ArrayType`/`SliceType`/`DotsType`/`StructType`/`InterfaceType`/`FuncType`/`MapType`/`ChanType`）、`Stmt`（全变体）

- [x] **Step 1: 写失败测试** —— `Expr` 全变体各构造一个（`T-A-02`，含类型族）；`Stmt` 全变体各构造一个（`T-A-03`）。验证 `Expr::Bad(BadExpr)` 存在（G19）；验证**无** `BadStmt`/`BadDecl` 变体（编译期保证）。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现 `expr.rs`** —— 按 `01-feature-set.md` AST-09..32 的映射 + G18（类型族落在 `Expr`）。`Operation` 拆成 `Expr::Unary`/`Expr::Binary`（AST-21）。`Stmt` 按 AST-33..49。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(ast): Expr enum (all variants incl. type family) + Stmt enum`

### Task T-AP04: 中立 dump 器（C6 格式 + 规范化规则）

**Files:**
- Create: `rgoc/crates/rgoc-ast/src/dump.rs`
- Test: `rgoc/crates/rgoc-ast/tests/test_dump.rs`

**Interfaces:**
- Consumes: T-AP01..T-AP03 的全部类型
- Produces: `fn dump(ast: &AstFile) -> String`（中立缩进格式）

- [x] **Step 1: 写失败测试** —— 手构造一个小 AST（`package p; func f(){}`），dump 后断言输出含 `File`/`PkgName`/`FuncDecl`/`Name "f"` 等行（`T-A-90`）。**RF5 钉点**：构造含 `ListExpr` 的 AST，断言 dump 输出扁平化（`[a, b, c]` → 三行 `Name`，不包裹 `ListExpr`）。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现 `dump.rs`** —— 按 `M2-tests.md` §5.2 的五条格式规则 + §5.3 的规范化规则。深度优先遍历，缩进 2 空格。`Operation` 统一输出（一元 Y 打 `null`）。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(ast): neutral dump formatter (C6)`

---

## 阶段 B：rgoc-parse 骨架

### Task T-AP05: 创建 `rgoc-parse` crate + Parser 状态 + `fileOrNil`

**Files:**
- Create: `rgoc/crates/rgoc-parse/Cargo.toml`、`rgoc/crates/rgoc-parse/src/lib.rs`、`rgoc/crates/rgoc-parse/src/parser.rs`
- Modify: `rgoc/Cargo.toml`（members 加 `crates/rgoc-parse`）
- Test: `rgoc/crates/rgoc-parse/tests/test_file.rs`

**Interfaces:**
- Consumes: `rgoc-ast::*`、`rgoc-lex::{Lexer, TokenKind, Pos}`
- Produces: `Parser`、`fn parse_file(src: &[u8], errh: Option<ErrHandler>) -> Result<AstFile, DiagBag>`

- [x] **Step 1: 写失败测试** —— 解析 `package main\n`，断言 `AstFile.pkg_name == "main"`、`decls` 为空（`T-A-20`）。解析 `package p; import "math"`，断言有一条 `Decl::Import`。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现 `parser.rs`** —— Parser 状态按 §5.1（`tok`/`top`/`fnest`/`xnest`/`pragma`/`base`）。`fileOrNil` 骨架：package 子句 → 顶层声明循环（`appendGroup`/`funcDeclOrNil`）。词元接入：每次 `next()` 调 `Lexer::next()`。`updateBase` 留位（T-AP15 实现）。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(parse): rgoc-parse crate + Parser state + fileOrNil`

### Task T-AP06: `advance` + `stopset` + errh 双档 + 三个恢复点

**Files:**
- Create: `rgoc/crates/rgoc-parse/src/recovery.rs`
- Test: `rgoc/crates/rgoc-parse/tests/test_recovery.rs`

**Interfaces:**
- Consumes: T-AP05 的 `Parser`
- Produces: `fn advance(&mut self, followlist: &[TokenKind])`、`stopset` 常量、`fn error_at(&mut self, pos, code, msg)`

- [x] **Step 1: 写失败测试** —— ① `errh=nil`：语法错 → panic 被 `recover` 转成 first error（`T-A-62`）。② `errh≠nil`：`package p; func f() { ; }` 正常；`package p; !!!` 报错后 `advance` 到 EOF（`T-A-63`）。**RF4 钉点**：`func f() { x =; y := 1 }` —— 报错后 `y := 1` 仍被正确解析（双断言：诊断 + 后续 AST）。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现 `recovery.rs`** —— 按 §5.3：`advance(followlist)` 循环 `next()` 直到 token ∈ followset 或 EOF；`fnest>0` 时 stopset 并入。`stopset` = **14** 个语句起始关键字（`F3`，勘误见 `M2-design.md` §14）。三个恢复点（顶层/语句列表/括号内）。`errorAt`：`errh==nil` panic，`errh≠nil` 回调。EOF 抑制连环错（`F5`）。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(parse): advance + stopset + errh dual-mode + recovery points`

### Task T-AP07: `stmtOrNil` 骨架 + `stmtList`

**Files:**
- Create: `rgoc/crates/rgoc-parse/src/stmt.rs`
- Test: `rgoc/crates/rgoc-parse/tests/test_stmt_list.rs`

**Interfaces:**
- Consumes: T-AP05/T-AP06
- Produces: `fn stmt_or_nil(&mut self) -> Option<Stmt>`、`fn stmt_list(&mut self) -> Vec<Stmt>`

- [x] **Step 1: 写失败测试** —— 解析 `{ x := 1; y := 2 }`，断言 `BlockStmt.stmts.len() == 2`（`T-A-50`）。解析 `{ }` 断言空块。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现 `stmt.rs`** —— `stmtOrNil` 按首 token 分派（§5.2）。本任务只做骨架：`_Name`→`simpleStmt`、`_{`→`block`、其余留 `todo!()` 在后续任务填。`stmtList` 循环到 `_Rbrace`/`_Case`/`_Default`。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(parse): stmtOrNil dispatch + stmtList`

---

## 阶段 C：声明族解析

### Task T-AP08: `appendGroup` + ImportDecl + ConstDecl(iota) + VarDecl

**Files:**
- Create: `rgoc/crates/rgoc-parse/src/decl.rs`
- Test: `rgoc/crates/rgoc-parse/tests/test_decl.rs`

**Interfaces:**
- Consumes: T-AP05 的 `Parser`
- Produces: `fn import_decl(&mut self) -> Decl`、`fn const_decl(&mut self, group) -> Decl`、`fn var_decl(&mut self, group) -> Decl`、`fn append_group(&mut self, f) -> Vec<Decl>`

- [x] **Step 1: 写失败测试** —— ① 单行 import（`T-A-21`）。② 分组 import（`import ( "a"; "b" )`）。③ 别名/点 import。④ const 含 iota（`T-A-22`）：`const ( a = iota; b )` 断言 b 的值表达式含 iota。⑤ var 单/多（`T-A-24`）。验证路径只验**合法字符串语法**，不加载包（G5M 边界）。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现 `decl.rs`** —— `appendGroup` 按官方处理 `(`/`)` 分组（`Group` 共享 iota）。import 路径验合法字符串。const 的 iota 是一个特殊标记（值不算，M3 折叠）。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(parse): importDecl + constDecl(iota) + varDecl + appendGroup`

### Task T-AP09: TypeDecl(泛型) + FuncDecl(recv/funcBody)

**Files:**
- Modify: `rgoc/crates/rgoc-parse/src/decl.rs`（续）
- Test: `rgoc/crates/rgoc-parse/tests/test_type_func_decl.rs`

**Interfaces:**
- Consumes: T-AP08 + T-AP07（`stmtList` 给 funcBody）
- Produces: `fn type_decl(&mut self, group) -> Decl`、`fn func_decl_or_nil(&mut self) -> Option<Decl>`、`fn func_body(&mut self) -> Option<BlockStmt>`

- [x] **Step 1: 写失败测试** —— ① type alias（`type A = B`，`T-A-23`）。② 泛型 type（`type List[T any] struct{...}`）。③ func 无 recv（`T-A-25`）。④ 方法（`func (r T) M()`）。⑤ 泛型 func。⑥ body=nil（外部声明 `func C.free()`）。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现** —— `typeDecl`：解析 tparams（`[` FieldList `]`）、alias 标记（`=`）。`funcDeclOrNil`：可选 recv（`(` Field `)`）、name、tparams、funcType、funcBody。`funcBody`：`{` stmtList `}`，nil 时返回 `None`。`fnest` 在进入 funcBody 时 +1。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(parse): typeDecl(generics) + funcDecl(recv/body)`

---

## 阶段 D：表达式与类型族解析

### Task T-AP10: operand + pexpr + binaryExpr 优先级爬升 + unaryExpr

**Files:**
- Create: `rgoc/crates/rgoc-parse/src/expr.rs`
- Test: `rgoc/crates/rgoc-parse/tests/test_expr.rs`

**Interfaces:**
- Consumes: T-AP05 的 `Parser`
- Produces: `fn expr(&mut self) -> Expr`、`fn binary_expr(&mut self, x, prec) -> Expr`、`fn unary_expr(&mut self) -> Expr`、`fn operand(&mut self) -> Expr`、`fn pexpr(&mut self, x) -> Expr`

- [x] **Step 1: 写失败测试** —— ① Name/BasicLit（`T-A-30`）。② **RF2 钉点**：`a + b * c` 断言 Binary 的嵌套形状（`*` 在内、`+` 在外）。③ 一元（`-x`/`!x`/`*p`/`&x`/`^x`，`T-A-31`）。④ Paren（`(x)`，`T-A-37`）。⑤ Selector/Index/Slice（`T-A-34`）。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现 `expr.rs`** —— `operand`：Name/BasicLit/`(`expr`)`/funcLit/complit/typeLit（后三类后续任务补）。`pexpr`：后缀链（`.x`/`[i]`/`[i:j]`/`.(T)`/`(args)`）。`binaryExpr` 用**优先级爬升**（不走独立函数树，对齐官方）。`unaryExpr`：一元运算符前缀。`xnest` 在 `pexpr` 入口增减（留 complit 二义给 T-AP11）。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(parse): operand + pexpr + binaryExpr(prec climbing) + unaryExpr`

### Task T-AP11: CallExpr + CompositeLit + xnest 二义 + FuncLit + KeyValue + ListExpr + BadExpr

**Files:**
- Modify: `rgoc/crates/rgoc-parse/src/expr.rs`（续）
- Test: `rgoc/crates/rgoc-parse/tests/test_expr_advanced.rs`

**Interfaces:**
- Consumes: T-AP10
- Produces: `fn call_expr(&mut self, fun) -> Expr`、`fn complit(&mut self, type_) -> Expr`、`fn func_lit(&mut self) -> Expr`

- [x] **Step 1: 写失败测试** —— ① CallExpr 普通调用 + `...` 变参（`T-A-32`）。② **RF1 钉点**：`if x == T{} {}` —— header 里 `xnest=-1` 禁用 complit ⇒ `T{}` 不被当复合字面量。③ CompositeLit（`[]int{1,2}`，`T-A-33`）。④ KeyValue（`map{K: V}`）。⑤ FuncLit（`func(){}`，`T-A-36`）。⑥ ListExpr（`T-A-38`）。⑦ BadExpr（错误恢复产出，`T-A-38`）。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现** —— `callExpr`：`(` argList `)` / `(` argList `...` `)`。`complit`：`type_ { elemList }`。**xnest 二义消解**（§5.6）：header 里 `xnest=-1` 禁 complit；括号内 `xnest>=0` 允许。`funcLit`：`funcType block`。`BadExpr`：`errorAt` 后返回 `Expr::Bad(BadExpr{pos})`。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(parse): callExpr + complit(xnest) + funcLit + keyValue + listExpr + badExpr`

### Task T-AP12: 类型族（Array/Slice/Dots/Struct/Interface/Func/Map/Chan）

**Files:**
- Create: `rgoc/crates/rgoc-parse/src/type_.rs`
- Test: `rgoc/crates/rgoc-parse/tests/test_type.rs`

**Interfaces:**
- Consumes: T-AP10（`operand`/`pexpr` 给类型名）
- Produces: `fn type_(&mut self) -> Expr`、`fn type_or_nil(&mut self) -> Option<Expr>`

- [x] **Step 1: 写失败测试** —— ① `[n]T` / `[...]T` / `[]T`（`T-A-40`）。② `...T`（`T-A-41`）。③ struct 含嵌入/tag/匿名（`T-A-42`）。④ interface 含方法/嵌入/约束（`~T` / `T | U`，`T-A-43`）。⑤ funcType 含 tparams（`T-A-44`）。⑥ `map[K]V`（`T-A-45`）。⑦ `chan T` / `chan<- T` / `<-chan T`（ChanDir，`T-A-45`）。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现 `type_.rs`** —— **关键**：`type_()` 返回 `Expr`（G18，类型与表达式同族）。按官方 `type_()` 的分派：`[`→array/slice、`...`→dots、`struct`→structType、`interface`→interfaceType、`func`→funcType、`map`→mapType、`chan`/`<-`→chanType、其余→`pexpr`（类型名/泛型实例化）。interface 的类型约束（`~`/`|`）在 1.18+ 语法。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(parse): type family (array/slice/dots/struct/interface/func/map/chan)`

---

## 阶段 E：语句族解析

### Task T-AP13: simpleStmt + 控制流（If/For/Switch/Select + Range/Case/Comm）

**Files:**
- Modify: `rgoc/crates/rgoc-parse/src/stmt.rs`（续）
- Test: `rgoc/crates/rgoc-parse/tests/test_stmt_ctrl.rs`

**Interfaces:**
- Consumes: T-AP07（`stmtOrNil` 骨架）+ T-AP10/T-AP11（`expr`）
- Produces: `fn simple_stmt(&mut self) -> SimpleStmt`、`fn if_stmt(&mut self) -> Stmt`、`fn for_stmt(&mut self) -> Stmt`、`fn switch_stmt(&mut self) -> Stmt`、`fn select_stmt(&mut self) -> Stmt`

- [x] **Step 1: 写失败测试** —— ① AssignStmt `=`/`:=`/`+=`/`++`（`T-A-52`）。② SendStmt（`T-A-51`）。③ IfStmt 含 init/else-if 链（`T-A-56`）。④ ForStmt 三段式/range/无限/**range over func**（`T-A-57`）。⑤ SwitchStmt + TypeSwitch（`T-A-58`）。⑥ SelectStmt + CommClause（`T-A-59`）。⑦ CaseClause（`T-A-59`）。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现** —— `simpleStmt`：`exprList` → 判 `=`/`:=`/`++`/`--`/`+=`/`<-`（send）。`ifStmt`：`if [init;] cond block [else if/block]`。`forStmt`：`for [init;] [cond;] [post] block` / `for [init] range x block`（含 range over func）。`switchStmt`：`switch [init;] [tag] { caseList }`。`selectStmt`：`select { commList }`。CaseClause/CommClause 的 default 用 `None`。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(parse): simpleStmt + control flow (if/for/switch/select + range/case/comm)`

### Task T-AP14: 其余语句（Labeled/Block/Branch/Call/Return/Empty/Decl）

**Files:**
- Modify: `rgoc/crates/rgoc-parse/src/stmt.rs`（续）
- Test: `rgoc/crates/rgoc-parse/tests/test_stmt_rest.rs`

**Interfaces:**
- Consumes: T-AP13
- Produces: 补全 `stmtOrNil` 的所有分支

- [x] **Step 1: 写失败测试** —— ① Empty（`;`，`T-A-50`）。② Labeled（`L:`，`T-A-50`）。③ Block（`{...}`，`T-A-50`）。④ BranchStmt break/continue/goto/fallthrough（`T-A-53`）。⑤ CallStmt go/defer（`T-A-54`）。⑥ ReturnStmt 零/多/裸（`T-A-55`）。⑦ DeclStmt（`T-A-52`）。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现** —— 补全 `stmtOrNil` 分派：`_Break`/`_Continue`/`_Goto`/`_Fallthrough`→`BranchStmt`；`_Go`/`_Defer`→`CallStmt`；`_Return`→`ReturnStmt`；`_Lbrace`→`BlockStmt`；`_Semi`→`EmptyStmt`；`_Var`/`_Const`/`_Type`→`DeclStmt`。Labeled：`Name :` → 递归 `stmtOrNil`。`_Name` 后判 `:`（labeled）vs `:=`/`=`/`<-`/`++`（simpleStmt）。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(parse): remaining statements (labeled/block/branch/call/return/empty/decl)`

---

## 阶段 F：指令 + branches + 诊断

### Task T-AP15: `//line` 重映射 + `//go:` pragma + goVersion + 版本门控

**Files:**
- Create: `rgoc/crates/rgoc-parse/src/directive.rs`
- Test: `rgoc/crates/rgoc-parse/tests/test_directive.rs`

**Interfaces:**
- Consumes: T-AP05（`base` 字段）+ M1 的 `directives` 模式输出
- Produces: `fn update_base(&mut self, pos, line, col, text)`、`fn pragma_handler(&mut self, pos, blank, text) -> Pragma`

- [x] **Step 1: 写失败测试** —— ① **RF3 钑点**：`//line foo.go:10` 后的节点 Pos 指向 `foo.go:10`（`T-A-70`）。② `//go:build go1.22` 取 goVersion（仅 `top`，`T-A-71`）。③ `//go:nosplit` 挂到 FuncDecl 的 pragma（`T-A-72`）。④ 版本门控（`T-A-97`）：`-lang go1.18` 拒绝泛型语法。**④ 实际未做（deferred）**：`parse_file` 无 `-lang` 入口，见 `M2-manifest.json` open_gaps —— 本步其余三条全部落地（27 条测试）。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现 `directive.rs`** —— `updateBase`：解析 `//line filename:line[:col]`，改 `parser.base`（§5.5）。`pragmaHandler`：`//go:` 去前缀，已知清单（`M2-tests.md` §4.3）解析挂载；未知按"忽略"临时处置（§9 待裁决）。`//go:build` 走 `constraint.Parse` 取 goVersion（仅 `top`，`F7`）。版本门控：**deferred（T-A-97）**——`parse_file` 尚无 `-lang` 注入入口，待接口改动时落地；其余 `updateBase` / `pragmaHandler` / `//go:build` 取 goVersion 全部落地。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(parse): //line remap + //go: pragma + goVersion + lang gate`

### Task T-AP16: branches 检查 + 错误消息复刻 + DiagCode 冻结

**Files:**
- Create: `rgoc/crates/rgoc-parse/src/branches.rs`、`rgoc/crates/rgoc-parse/src/diag.rs`
- Test: `rgoc/crates/rgoc-parse/tests/test_branches.rs`、`tests/test_diag.rs`

**Interfaces:**
- Consumes: T-AP14（完整的 Stmt 树）
- Produces: `fn check_branches(body: &BlockStmt, errh)`、M2 新增 `DiagCode`（`SYN-` 前缀）

- [x] **Step 1: 写失败测试** —— ① break 不在循环里（`T-A-80`/N1）。② 标签未定义（`T-A-81`/N2）。③ goto 跳过变量（`T-A-82`）。④ 错误消息子串匹配（`T-A-66`）：`syntax error: unexpected X, expected Y` 形态。⑤ DiagCode 冻结总数（从 `syntaxError` 调用点枚举）。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现** —— `branches.rs`：复刻 `branches.go`（`F18`），遍历 BlockStmt 检查 break/continue/fallthrough/goto 标签。`diag.rs`：续 M1 的 `rgoc-lex::diag::DiagCode`，新增 `SYN-` 前缀代号。**消息复刻**（G20）：从 `parser.go` 的 `syntaxErrorAt` 调用点枚举消息模板，冻结条目数（设计文档 §7.2 的教训：代号表"凭印象手写会漏且无报错"）。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(parse): checkBranches + error message replication + DiagCode(SYN-) freeze`

---

## 阶段 G：oracle adapter + 对撞

### Task T-AP17: vendor `tools/ast-oracle` + adapter 编译运行 + `dump.go`

**Files:**
- Create: `tools/ast-oracle/{README.md, go.mod, main.go, dump.go, gosyntax/}`（vendor 官方 syntax）
- Test: 手动运行 `go run . <file>` 验证输出

**Interfaces:**
- Consumes: `go_source_code/src/cmd/compile/internal/syntax/`（vendor）
- Produces: ast-oracle 二进制，输入 `.go` 文件输出中立 dump

- [x] **Step 1: 写 README** —— 记来源（`go1.27.1`）、版本、校验值、再生成步骤（同 M1 的 `tools/token-oracle/README.md` 模式）。
- [x] **Step 2: vendor `gosyntax/`** —— 原样复制官方 `syntax/` **16 个非测试** `.go`（**零改动**，`M2-design.md` §9；`*_test.go`/`testdata` 不 vendor，理由见 README §2.2）。⚠️ **未加 `export.go`**：ast-oracle 走的全是导出面（`ParseFile`/全部节点类型/`LitKind`/`Operator`），实测 `go build` 不需要任何转发 —— 与 M1 scanner「全部未导出」不同形，不预留空 shim（README §2.1 + ledger）。
- [x] **Step 3: 实现 `main.go` + `dump.go`** —— `main.go`：读源码 → `ParseFile(errh, nil, CheckBranches)` → 遍历 AST。`dump.go`：**手写遍历器**输出中立格式（不用 `Fdump`，`F14`）。格式与 `rgoc-ast::dump` **逐字节一致**（`rustDebugQuote` 模拟 Rust `{:?}`，不用 `%q`）。
- [x] **Step 4: 编译运行验证** —— 容器内 `go run . /work/go_source_code/src/fmt/print.go`：exit 0、dump 9490 行、`File`/`PkgName`/`Decls: [77 entries]` 起头、3 次 sha256 一致（`T-A-91`）；另验探针文件全节点形状 + 错误路径（exit 1 + stderr 诊断 + dump 照常）+ `CheckBranches`（`goto L jumps over declaration`）。
- [x] **Step 5: Commit** —— `feat(tools): ast-oracle adapter (vendor syntax + neutral dump)`（2026-10-16 补记：落 `19a88f2`，commit 纪律已改为阶段收口即提交）

### Task T-AP18: 对撞回路（testdata 31 + 规范化规则 + 双向匹配）

**Files:**
- Create: `scripts/check-ast-golden.sh`、`scripts/check-ast-testdata.sh`、`rgoc/crates/rgoc-parse/tests/test_oracle.rs`
- Test: `test_oracle.rs`（`include_str!` 嵌入 testdata）

**Interfaces:**
- Consumes: T-AP04（rgoc dumper）+ T-AP17（oracle dumper）
- Produces: 对撞报告

- [x] **Step 1: 写失败测试** —— `test_oracle.rs` 62 条（collide 31 + errcheck 31，官方 `error_test.go` 直译：F11 子串 / F12 行注释=行号、块注释=下一词元行+列）。**RF5 钉点**：ListExpr 扁平化经 `exprListFrom` 两侧同规则。
- [x] **Step 2: 跑确认失败** —— RED 实测 **4/62 红**（tparams/typeset 的 errcheck 列差一列 + 2 文件 bad-pattern）；**collide 31/31 首跑即全绿**（899 条前置测试的收敛度）。修 ① `Expr::start_pos`（官方 `StartPos` 语义，`parser.go:2077` 唯一使用点）② `go_rx_to_rust`（Go 裸 `{` 是字面量 / Rust regex 报错）。修后 62/62 绿。
- [x] **Step 3: 实现 `check-ast-golden.sh`** —— ①语料副本逐字节守卫 ②golden 齐性守卫（无孤儿/非空/首行 File）③cargo 对撞。**`check-ast-testdata.sh`**：覆盖面守卫（ERROR 文件数 25 / 两组测试各 31）+ 双向匹配（T-A-95）。
- [x] **Step 4: 扩到 testdata 31 全量** —— 31 文件 collide 全绿 + 25 文件 errcheck 双向全绿；**反向验证 4 变异全命中**（M1 start_pos→pos → tparams/typeset 红；M2 dump 行改名 → 11 collide 红；M3 行注释回退失效 → 6 errcheck 红；M4 裸花括号不转义 → issue46558/56022 红）。四条门禁全过（**963 条**，899+62+2）；M0（231）/M1 一致性自检全过。
- [x] **Step 5: Commit** —— `feat(parse): oracle collision harness (testdata 31 + bidirectional ERROR match)`（2026-10-16 补记：落 `19a88f2`）

---

## 阶段 H：driver + 冒烟 + 门禁

### Task T-AP19: `rgoc ast` 子命令

**Files:**
- Create: `rgoc/crates/rgoc-driver/src/ast_cmd.rs`
- Modify: `rgoc/crates/rgoc-driver/src/cli.rs`（加 `Command::Ast`）、`rgoc/crates/rgoc-driver/Cargo.toml`（依赖 `rgoc-parse`）
- Test: `rgoc/crates/rgoc-driver/tests/test_ast_cmd.rs`

**Interfaces:**
- Consumes: T-AP05（`parse_file`）+ T-AP04（`dump`）
- Produces: `rgoc ast <file>` 命令（输出中立 dump 到 stdout）

- [x] **Step 1: 写失败测试** —— `rgoc-driver -- ast <fixture>` 输出含 `File`/`PkgName` 行；`-- ast --help` 列出用法。对齐 `rgoc lex` 模式（`cli.rs` 手写解析，不引 clap）。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现 `ast_cmd.rs`** —— 读文件 → `parse_file` → `dump` → stdout。加 `# source: rgoc` 头（对齐 `M1-tests.md` §5.3 的来源标注）。`cli.rs`：`Command::Ast(PathBuf)`，手写解析。
- [x] **Step 4: 跑确认通过**
- [x] **Step 5: Commit** —— `feat(driver): rgoc ast subcommand`（2026-10-16 补记：落 `a7c1111`）

### Task T-AP20: 标准库冒烟 + 递归深度上限

**Files:**
- Create: `scripts/check-ast-stdlib.sh`、`rgoc/crates/rgoc-parse/tests/test_smoke.rs`
- Modify: `rgoc/crates/rgoc-parse/src/parser.rs`（深度计数）

**Interfaces:**
- Consumes: T-AP18（parser 已完整）
- Produces: 冒烟报告 + 深度保护

- [x] **Step 1: 写失败测试** —— ① **`T-A-94`**：解析 `go_source_code/src` 全部 `.go`，断言**零意外错误**（允许已知 unsupported 项）。② **`T-A-96`**（**H**）：构造 >1000 层嵌套表达式，断言报 `SYN-NESTING-TOO-DEEP` 且**不 panic**。③ `T-A-98`：同输入 3 次结果一致。④ `T-A-99`：Eof 唯一。
- [x] **Step 2: 跑确认失败**
- [x] **Step 3: 实现** —— `parser.rs`：加 `depth: usize` 计数器，`expr`/`stmt_or_nil`/`type_` 入口 +1、出口 -1，超 1000 报诊断返回。`check-ast-stdlib.sh`：遍历 `go_source_code/src/**/*.go`，每文件 `rgoc-driver -- ast`，收集非零退出码（**只记意外错误**，unsupported 项排除）。
- [x] **Step 4: 跑确认通过**（标准库冒烟可能需要多轮修 bug）
- [x] **Step 5: Commit** —— `feat(parse): stdlib smoke + recursion depth limit + consistency`（2026-10-16 补记：落 `fb93ef4`）

### Task T-AP21: 阶段收口（C1 延续 + C6 新增 + GAP-01 + manifest + 文档同步）

**Files:**
- Modify: `docs/contracts/C1-source-map.md`（补 parser 侧位置语义）、Create: `docs/contracts/C6-ast-dump.md`、Modify: `docs/milestones/GAP-01.md`、Create: `docs/milestones/M2-manifest.json`、Modify: `AGENTS.md`、`docs/README.md`、`docs/milestones/M2-design.md`（状态→已确认）、`M2-tests.md`（状态→已冻结）、`M2-plan.md`（状态→已完成）

- [x] **Step 1: C1 延续** —— 在 `C1-source-map.md` 补"parser 侧位置语义"一节（`//line` 重映射、节点 Pos 规则）+ 变更记录。
- [x] **Step 2: C6 新增** —— 写 `C6-ast-dump.md`：中立 dump 格式（§5.2 五规则）+ 规范化规则（§5.3）+ 变更记录。`level: confirmed`。
- [x] **Step 3: GAP-01 续** —— 填 AST-01..49 + PIPE-03/04/05/25 的四维归属表（53 项）。
- [x] **Step 4: manifest** —— 写 `M2-manifest.json`：`gate`（A1-A8）+ `test_ids`（67 个）+ `unsupported` + `budget` + `open_gaps`（含 `D-M2-2` 偏离登记 + `//go:` 未知指令待裁决）。
- [x] **Step 5: 门禁复核** —— 四条 cargo 命令 + `check-ast-golden.sh` + `check-ast-testdata.sh` + `check-ast-stdlib.sh` 全过。同步 `AGENTS.md` / `docs/README.md` 索引。
- [x] **Step 6: Commit** —— `chore(m2): 阶段收口 —— C1/C6/GAP-01/manifest/文档同步`

---

## 自检记录

| 检查 | 结果 |
|---|---|
| **Spec coverage** | `M2-design.md` §1.2 的 7 条不做项都有归属；AST-01..49 在 `M2-tests.md` §2 逐项映射到测试 ID；D-M2-1~7 每条都有任务落地（D-M2-1→T-AP17/18、D-M2-2→T-AP06、D-M2-3→T-AP18/20、D-M2-4→T-AP01/05、D-M2-5→T-AP04/17、D-M2-6→T-AP15、D-M2-7→T-AP19） |
| **Step scan** | 每个 Step 只要求一件事；无「处理边界情况」这类空话 |
| **Type consistency** | `AstFile`/`Decl`/`Expr`/`Stmt`/`SimpleStmt` 的签名在 T-AP01..T-AP03 定义，后续任务只引用不重定义；`Parser` 状态在 T-AP05 定义，T-AP06..T-AP16 只用 |
| **Review Focus** | 五条 RF 各自钉在 T-AP11 / T-AP10 / T-AP15 / T-AP06 / T-AP04 |
| **Proportion** | 本计划 21 个任务；代码块以签名与测试断言为主，不含完整函数体 |
