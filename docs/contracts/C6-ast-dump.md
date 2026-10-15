# C6 契约：AST 中立 dump 格式

> ## 本契约把「oracle 对撞」的比对面**冻结成文**
>
> M2 的验收核心（`T-A-92/93`）是 rgoc AST 与官方 `cmd/compile/internal/syntax`
> 的 AST 的**逐字节 diff** —— 双方各自渲染成一份「中立 dump」，diff 为空即树同构。
> 本契约规定那份 dump 的**格式与规范化规则**。实现两侧：
>
> - Rust：`rgoc-ast/src/dump.rs`（被 976 条测试冻结）
> - Go（oracle）：`tools/ast-oracle/dump.go`（T-AP17，与 dump.rs 逐行同形）
>
> **为什么「中立」**：节点名取自官方 `nodes.go` 的类型名（`ForStmt`/`BinaryExpr`…），
> 但**不 dump 任何一侧的私有细节**（位置、内部缓存、回指针）——
> 两边才能有共同的比对面。

**状态**：**已冻结**（M2 / `T-AP21` 升级）　|　**契约级别**：**完整**　|　**实现落点**：`rgoc-ast/dump.rs` + `tools/ast-oracle/dump.go`　|　**消费者**：M2（对撞门禁 A4）、M3（类型检查的调试输出）、M4+（诊断上下文）

---

## 1. 范围

| 本契约**规定** | 本契约**不规定**（留给谁） |
|---|---|
| dump 的行格式（节点行 / 字段行 / 缩进） | AST 的**内存表示**（`rgoc-ast` 各 struct 的字段布局归代码与对撞测试） |
| 五条格式规则（§2） | 字面量**值的语义**（折叠/常量求值归 M3） |
| 三条规范化规则（§3） | 诊断的渲染（归 C1 §2.2/§3.2） |
| 字符串值的转义写法（§2.4，Rust `{:?}` 语义） | 位置信息的表达（**刻意不 dump**，§2 规则 4） |

---

## 2. 五条格式规则（M2-tests §5.2，冻结）

1. **节点名用中立名**：`File` / `ForStmt` / `BinaryExpr` / `ArrayType` …
   —— 即官方 `nodes.go` 的导出类型名，不加包名、不加侧别前缀。
2. **列表显式标 `[n entries]`**：`Decls: [2 entries]`、`Args: [0 entries]`。
   空列表**必须出现**且标 `[0 entries]` —— 「没有这个字段」与「空列表」是两回事。
3. **可选字段用 `-` 占位**：`Name: -`、`GoVersion: -`、`Recv: -`。
   `Option::None` 与「字段存在但值为零」在 dump 上**必须可区分**。
4. **位置默认不 dump**：dump 是**树形**的比对面，不含行列。
   位置语义由 C1 §2.3 承担，其验证走 errcheck 位置匹配（`T-A-95`）与 @ 腿。
5. **字面量只 dump 原文**：`BasicLit(int) "1"`、`Name "x"`。
   不做值折叠、不做进制规范化 —— 那是 M3 折叠器的产出，混进来会让
   「解析正确」与「求值正确」两个维度互相污染。

### 2.1 行语法

```text
File
  PkgName: Name "p"
  GoVersion: -
  Decls: [1 entries]
    FuncDecl
      Recv: -
      Name: Name "F"
      ...
```

- 缩进：**两个空格一层**（不用 tab —— 与 Go 源码的 tab 惯例刻意不同，
  避免「复制粘贴进 .go 文件」时混排）。
- 字段行：`标签: 值`；子节点行：`标签:` 单独一行、子节点缩进一层。
- 文件头两行固定为 `File` 与 `PkgName`；`GoVersion` 缺省（无 `//go:build`）为 `-`。

### 2.2 布尔与枚举

- 布尔直书：`Alias: true` / `HasDots: false`。
- 枚举用**官方名**：`Op:` 用 `Add`/`Mul`/`Def`…（`tokens.go` 的 `Operator`）；
  `LitKind` 用 `Int`/`String`/`Rune`…

### 2.3 键名与官方字段的对应

键名**逐字段**对齐官方 `nodes.go`（如 `AssignStmt` 的 `Lhs/Rhs/Op`、
`CaseClause` 的 `Cases/Body`）。两侧（Rust/Go）由 T-AP17 的逐字节对撞钉住，
**键名漂移就是对撞红**，不存在「人记得同步」。

### 2.4 字符串值的转义（`rustDebugQuote`）

字符串值（`Name` 的名字、`BasicLit` 的原文）在 dump 里带**引号与转义**，
转义语义 = **Rust `{:?}` 的常用分支**（`\"` `\\` `\n` `\r` `\t` `\0`、
`<0x20>`/`<0x7f>` → `\u{hex}`）—— **不是** Go `%q`（那会把非可打印字符
按 Go 规则转义，与 Rust 侧不一致）。

> **已知缺口**（登记在 M2 ledger）：Rust `from_utf8_lossy` 把非法 UTF-8
> 字面量转成 `U+FFFD`，Go 侧保留原始字节 ⇒ 含非法 UTF-8 的输入两侧 dump
> 会分叉。合法 Go 源码极少触发；对撞 31 文件 + 标准库 6670 文件均未覆盖到。

---

## 3. 三条规范化规则（M2-tests §5.3，冻结）

规范化的目的：把「两侧同构但表示不同」的形状**归一到 dump 层一致**，
只对撞「真正的树差异」。

1. **`Operation` 统一输出**：一元与二元共用节点名 `Operation`
   （官方 `Operation{X, Op, Y}` 一体），一元时 `Y: null`、二元时 `Y:` 子节点。
   不拆成 `UnaryExpr`/`BinaryExpr` 两个名字 —— 那会让「一元减号与二元减号」
   在两侧因建模差异而假红。
2. **`ListExpr` 扁平化**：官方 parser 在多值上下文（`a, b := f()` 的 lhs）
   用 `ListExpr` 包装；单元素时**剥离包装直接输出元素**（两侧同规则）。
3. **`AssignStmt` 的 `IncDec` 显式标注**：`x++` 官方是
   `AssignStmt{Op: Add, Rhs: nil}`（T-AP17 实证）—— dump 时 `Op` 照实输出
   `Add`，`Rhs: null` 即隐含「这是 `++`/`--`」。**不**发明 `IncDec` 键。

> ⚠️ `M2-tests.md` §5.2 的**示例**是 C6 草案期的旧稿
> （`Expr: CallExpr` 带冒号、`GoVersion: "go1.22"` 带引号）——
> **以本契约与 `dump.rs` 为准**（`X: CallExpr` 无冒号、无引号）。
> M2-tests 的示例修订随本文档冻结一并生效。

---

## 4. 与 oracle 的对撞（本契约的验证方式）

| 断言 | 载体 |
|---|---|
| rgoc dump == oracle golden（31 文件逐字节） | `rgoc-parse/tests/test_oracle.rs` 的 `t_collide_*`（31 条，`T-A-92/93`） |
| oracle 自身确定性（同输入 3 次 sha256 一致） | `tools/ast-oracle/README.md` §5 验收记录 |
| rgoc 侧确定性 | `test_smoke.rs` 的 `t_a_98` |
| oracle 产出合法（首行 `File`） | `scripts/check-ast-golden.sh` 守卫 ② |
| driver 正文 == `dump()` 直出 | `rgoc-driver/tests/test_ast_cmd.rs` 的 `run_ast_正文与_rgoc_ast_dump_一致` |
| 门禁入口 | `scripts/check-ast-golden.sh`（`--record` 重录 golden） |

---

## 5. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-15 | **初稿创建并冻结**（M2 / `T-AP21`）。五条格式规则 + 三条规范化 + 转义语义（`rustDebugQuote`）+ 已知缺口（非法 UTF-8）如实登记；M2-tests §5.2 旧稿示例的分歧在此裁定（以 dump.rs 为准） | `rgoc-ast/dump.rs`（976 条测试冻结）+ `tools/ast-oracle/dump.go`（对撞 31/31 全绿）+ `M2-plan.md` `T-AP21` Step 2 |
