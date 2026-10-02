# Go 语言编译器功能全集（Rust 重写版）

> **基准规范**：The Go Language Specification, version go1.27 (May 26, 2026)
> **AST 基准**：`src/cmd/compile/internal/syntax/nodes.go`（本仓库 go_source_code）
> **方法论**：Closure Coverage —— Spec 语义原子 × AST 节点对账 × Runtime 抽象内核 三维交叉验证
> **文档性质**：Rust 重写编译器的完整需求集（RTM，Requirements Traceability Matrix）  
> **文档索引**：[`README.md`](./README.md)　|　**下游**：[`02-test-inventory.md`](./02-test-inventory.md)、[`03-roadmap.md`](./03-roadmap.md)

---

## 0. 追踪体系

### 0.1 三维交叉验证矩阵

```
[维度 1: Go Spec 语义原子]  ──►  [RTM 功能矩阵]  ◄──  [维度 2: AST 节点对账]
                                       │
                                       ▼
                              [维度 3: Runtime 五大内核]
                              (调度/GC/内存/复合类型/展开)
```

- **维度 1** 保证 *语义无遗漏*：Spec 每一个 h3/h4 章节至少映射到一个功能点 ID。
- **维度 2** 保证 *语法无遗漏*：`nodes.go` 每一个具体节点结构体必须在 Rust 前端 `enum` 中有对应变体；AST 覆盖率 = 100% 时，不存在本编译器无法解析的合法 Go 代码。
- **维度 3** 保证 *运行时无遗漏*：脱离官方 runtime 黑盒，将运行时需求拆为 5 个与编译器协同的子系统。

### 0.2 功能 ID 编码规则

| 前缀 | 领域 | 前缀 | 领域 |
|---|---|---|---|
| LEX-nn | 词法要素 | BIF-nn | 内建函数 |
| CON-nn | 常量与折叠 | PKG-nn | 包与初始化 |
| TYP-nn | 类型系统 | SYS-nn | unsafe/系统层 |
| SCP-nn | 块与作用域 | AST-nn | AST 对账条目 |
| EXP-nn | 表达式 | RT-{MEM,SCH,CMP,POLY,EXC}-nn | Runtime 子系统 |
| STM-nn | 语句 | PIPE-nn | 编译器流水线 |

### 0.3 覆盖率验收标准

1. Spec 章节 → 功能点映射率 = **100%**（含 Appendix 语言版本条目）
2. AST 节点 → Rust 变体对账率 = **100%**（46 个节点结构体全量核对）
3. 每个 BIF/内建 → 编译器魔法 + Runtime 入口 双向登记
4. 预声明标识符（21 类型 + true/false/iota + nil + 18 函数）全部落入 universe block 实现项

### 0.4 引用基准与路径约定

本仓库根目录记为 `$R = /Users/wangfeng/workspace/rust_go_compiler/go_source_code`。文档中所有引用位置均相对 `$R`，行号基于**本仓库当前快照**（go1.27 源码树），格式及含义如下：

| 引用前缀 | 展开路径 | 说明 |
|---|---|---|
| `spec:Lxx-yy` | `$R/doc/go_spec.html` | Spec 章节定义行区间（HTML 源文件行号，`<h2>/<h3>/<h4 id=...>` 起始行） |
| `nodes:Lxx-yy` | `$R/src/cmd/compile/internal/syntax/nodes.go` | AST 节点结构体定义行区间 |
| `runtime/xxx.go:NNN` | `$R/src/runtime/xxx.go` | Runtime 参考实现（本编译器需等价重写，非直接复用） |
| `abi/type.go:NNN` | `$R/src/internal/abi/type.go` | 运行时类型元数据布局 |
| `gc/xxx/` | `$R/src/cmd/compile/internal/xxx/` | 官方编译器对应阶段的参考目录 |
| `link/ld/` | `$R/src/cmd/link/internal/ld/` | 官方链接器参考目录 |

> **说明**：Spec 与 AST 的引用是**规范性（normative）**依据——Rust 实现须 100% 对齐；Runtime/编译器的源码引用是**参考性（reference）**依据——仅作语义校准，Rust 实现可自由设计内部结构。

---

## 1. 维度 1：Go Spec 语义原子全集

> 引用列格式 `spec:Lstart-end`，指向 `$R/doc/go_spec.html`。章节行区间按 `<h2>/<h3>/<h4>` 锚点切分。

### 1.1 词法要素（LEX）

| ID | 功能 | Spec 章节 | **Spec 位置** | 涉及 AST | 语义要点 |
|---|---|---|---|---|---|
| LEX-01 | UTF-8 源码表示 | Source code representation | `spec:L79-103` | File | 非规范化 UTF-8；可禁止 NUL；可忽略首部 BOM (U+FEFF) |
| LEX-02 | Unicode 字符分类 | Characters / Letters and digits | `spec:L105-135` | — | 字母 = Lu/Ll/Lt/Lm/Lo；数字 = Nd；`_` 视为小写字母 |
| LEX-03 | 行注释 / 块注释 | Comments | `spec:L139-161` | Comment | `//` 至行尾；`/*…*/`；无换行的块注释=空格，否则=换行；注释不可嵌套 |
| LEX-04 | Token 四分类与最长匹配 | Tokens | `spec:L163-177` | 全部 | 标识符/关键字/操作符标点/字面量；空白（U+0020/09/0D/0A）仅分隔 |
| LEX-05 | 25 个保留关键字 | Keywords | `spec:L255-266` | parser | break…var；不可作标识符 |
| LEX-06 | 47 个操作符与标点（含 `~`） | Operators and punctuation | `spec:L268-282` | Operation 等 | `+ - * / % & \| ^ << >> &^` 及复合赋值、`&& \|\| ! < > <= >= == !=`、`<- ++ -- := ... . , ; : ( ) [ ] { }`、`~`(1.18) |
| LEX-07 | 分号自动插入 | Semicolons | `spec:L179-230` | parser | 行尾 token 为标识符/字面量/`break continue fallthrough return`/`++ -- ) ] }` 时补 `;`；`)`/`}` 前可省略 |
| LEX-08 | 标识符词法 | Identifiers | `spec:L233-252` | Name | `letter { letter \| unicode_digit }` |
| LEX-09 | 整数字面量（4 进制 + 分隔符） | Integer literals | `spec:L284-333` | BasicLit | `0b/0B`、`0/0o/0O`、`0x/0X`、`0`；`_` 分隔符规则（前缀后/数字间） |
| LEX-10 | 浮点字面量（十进制 + 十六进制） | Floating-point literals | `spec:L336-411` | BasicLit | 十进制 `e/E` 指数；十六进制须带 `p/P` 二进制指数（IEEE 754-2008 §5.12.3） |
| LEX-11 | 虚数字面量 | Imaginary literals | `spec:L414-450` | BasicLit | int/float 后缀 `i`；兼容：纯十进制前导 0 按十进制（`0123i == 123i`） |
| LEX-12 | Rune 字面量与转义 | Rune literals | `spec:L453-551` | BasicLit | `\a \b \f \n \r \t \v \\ \'`；`\x HH`、`\u HHHH`、`\U HHHHHHHH`、`\ooo`(0-255)；`\u \U` 非法值报错 |
| LEX-13 | 字符串字面量（解释型/原始） | String literals | `spec:L553-631` | BasicLit | 双引号解释型（支持全部转义，不可跨行）；反引号原始型（可跨行、`\r` 被丢弃、无转义） |

### 1.2 常量与常量折叠（CON）

| ID | 功能 | Spec 章节 | **Spec 位置** | 语义要点 |
|---|---|---|---|---|
| CON-01 | 无类型常量体系 | Constants | `spec:L633-739` | 布尔/整数/rune/浮点/复数/字符串 6 类无类型常量；任意精度 |
| CON-02 | 常量精度要求 | Constants | `spec:L633-739` | 实现须支持 ≥256 位整数、≥256 位尾数+≥16 位有符号指数（二进制）的浮点常量运算 |
| CON-03 | 隐式类型转换规则 | Constants / Properties | `spec:L633-739`, `spec:L1774-1776` | 无类型常量与 typed 操作数混合时隐式转换；单独使用时取默认类型（int/rune/float64/complex128/string/bool） |
| CON-04 | 常量表示性 | Representability | `spec:L2031-2091` | 常量须能被目标类型无精度损失表示（舍入：向偶数舍入） |
| CON-05 | 常量表达式折叠 | Constant expressions | `spec:L5757-5877` | 编译期求值：算术/比较/逻辑/移位/转换全集作用于常量 |
| CON-06 | iota 枚举 | Iota | `spec:L2400-2451` | ConstDecl 组内从 0 递增；表达式省略时重复上一表达式 |
| CON-07 | 常量声明 | Constant declarations | `spec:L2331-2398` | `const NameList [Type] = Values`；重复表达式语义；`const ( … )` 组 |
| CON-08 | 移位计数语义 | Operators (1.13) | `spec:L4814-4885`, `spec:L8721-8742` | 常量移位计数视为无符号；非负左值检查 |

### 1.3 类型系统（TYP）

| ID | 功能 | Spec 章节 | **Spec 位置** | 涉及 AST | 语义要点 |
|---|---|---|---|---|---|
| TYP-01 | 类型命名/未命名分类 | Types | `spec:L799-833` | TypeDecl | 命名类型（预声明/定义）vs 复合/未命名类型 |
| TYP-02 | 布尔类型 | Boolean types | `spec:L835-842` | — | 预声明 bool |
| TYP-03 | 数值类型全集 | Numeric types | `spec:L844-898` | — | 8 有符号 + 8 无符号（byte=uint8, rune=int32）+ uintptr + float32/64 + complex64/128 |
| TYP-04 | 字符串类型 | String types | `spec:L900-922` | — | 不可变字节序列；长度内置 |
| TYP-05 | 数组类型 | Array types | `spec:L924-980` | ArrayType | 长度为非负*常量表达式*（可由 `[...]` 推断）；长度属类型 |
| TYP-06 | 切片类型 | Slice types | `spec:L982-1050` | SliceType | 底层数组视图；nil 切片 |
| TYP-07 | 结构体类型 | Struct types | `spec:L1052-1193` | StructType | 字段名唯一性（含嵌入提升）、Tag、空字段 `_`、嵌入字段 |
| TYP-08 | 指针类型 | Pointer types | `spec:L1195-1211` | Operation(*) | 无算术；nil 指针 |
| TYP-09 | 函数类型 | Function types | `spec:L1213-1258` | FuncType | 参数/结果；可变参数 `...T`；nil 函数 |
| TYP-10 | 基本接口（方法集接口） | Basic interfaces | `spec:L1260-1287`, `spec:L1288-1385` | InterfaceType | 方法签名列表；`interface{}` 空方法集；嵌入接口方法并集 |
| TYP-11 | 通用接口（约束接口） | General interfaces | `spec:L1434-1606` | InterfaceType | 类型元素：项集（term = T \| ~T）、union `A\|B`、约束字面量仅用于类型参数（1.18） |
| TYP-12 | 嵌入接口 | Embedded interfaces | `spec:L1386-1433` | InterfaceType | 嵌入元素为接口类型名时方法集并集（1.14 允许重复嵌入） |
| TYP-13 | 接口实现判定 | Implementing an interface | `spec:L1607-1627` | typecheck | 方法集 ⊇ 接口方法集（签名精确匹配） |
| TYP-14 | 映射类型 | Map types | `spec:L1628-1687` | MapType | 键类型须可比较；nil map |
| TYP-15 | 通道类型 | Channel types | `spec:L1688-1772` | ChanType | 双向/`<-chan`/`chan<-` |
| TYP-16 | 底层类型 | Underlying types | `spec:L1827-1861` | typecheck | 递归求值至复合类型或基础类型 |
| TYP-17 | 类型同一性 | Type identity | `spec:L1862-1962` | typecheck | 复合类型逐元素同一；命名类型看声明；函数看参数结果（含变参/方向）；结构体看字段+Tag |
| TYP-18 | 可赋值性 | Assignability | `spec:L1963-2030` | typecheck | 同一/底层相同且至少一方未命名/接口实现/nil/无类型常量可表示/通道方向兼容 |
| TYP-19 | 方法集 | Method sets | `spec:L2093-2129` | typecheck | T 值接收者；*T 值+指针接收者；嵌入字段提升规则 |
| TYP-20 | 类型定义 | Type definitions | `spec:L2511-2632` | TypeDecl | `type N D` 创建新命名类型；方法不可跨包定义 |
| TYP-21 | 类型别名 | Alias declarations | `spec:L2465-2510`, `spec:L8834-8840` | TypeDecl | `type A = D`（1.9）；泛型别名（1.24） |
| TYP-22 | 类型参数声明 | Type parameter declarations | `spec:L2634-2702` | TypeDecl.TParamList | `[T any]`；约束即接口；每个函数/类型独立作用域 |
| TYP-23 | 约束满足性 | Satisfying a type constraint | `spec:L2765-2810` | typecheck | 类型参数须实现约束：项集成员（T、~T 底层匹配）、union 成员 |
| TYP-24 | 类型实例化 | Instantiations | `spec:L4384-4484` | IndexExpr | 泛型函数/类型以类型实参实例化；须检查约束满足 |
| TYP-25 | 类型推断 | Type inference | `spec:L4485-4692` | typecheck | 1.18 函数实参推断；1.21 接口方法/赋值推断；1.27 全赋值上下文函数类型推断 |
| TYP-26 | 类型统一算法 | Type unification | `spec:L4693-4813`, `spec:L8859-8869+` | typecheck | exact / loose 匹配模式；assignability 统一 `≡A`；结构/元素递归规则 |

### 1.4 代码块与作用域（SCP）

| ID | 功能 | Spec 章节 | **Spec 位置** | 语义要点 |
|---|---|---|---|---|
| SCP-01 | 块与作用域嵌套 | Blocks | `spec:L2130-2169` | universe ⊃ package ⊃ file ⊃ function ⊃ block；显式块 `{}` 与隐式块（if/for/switch/case/select 子句） |
| SCP-02 | 声明的作用域规则 | Declarations and scope | `spec:L2170-2252` | import=文件；函数/类型/变量/常量=包；标签=函数；块内声明自声明点至块尾 |
| SCP-03 | 标签作用域 | Label scopes | `spec:L2253-2267` | 声明于函数体；与变量/类型/常量/函数名空间独立 |
| SCP-04 | 空白标识符 | Blank identifier | `spec:L2268-2277` | `_` 无绑定，可多次声明 |
| SCP-05 | 预声明标识符 | Predeclared identifiers | `spec:L2278-2303` | universe block 注入 21 类型/true/false/iota/nil/18 函数；可被遮蔽 |
| SCP-06 | 导出标识符 | Exported identifiers | `spec:L2304-2320` | 包块/字段名/方法名 首字符 Unicode Lu |
| SCP-07 | 标识符唯一性 | Uniqueness of identifiers | `spec:L2321-2330` | 同作用域重声明错误；不同包的非导出标识符不同 |
| SCP-08 | 变量遮蔽 | Declarations and scope | `spec:L2170-2252` | 内层块声明同名列；`:=` 至少一个新变量语义 |
| SCP-09 | for 迭代变量独立作用域 | For statements (1.22) | `spec:L6606-6617`, `spec:L8815-8826` | 每次迭代新建迭代变量（语义变更） |
| SCP-10 | 控制语句 init 作用域 | If/Switch/For | `spec:L6327-6361`, `spec:L6606-6617` | Init 语句作用域延伸至整个语句 |

### 1.5 表达式（EXP）

| ID | 功能 | Spec 章节 | **Spec 位置** | 涉及 AST | 语义要点 |
|---|---|---|---|---|---|
| EXP-01 | 操作数 | Operands | `spec:L3111-3148` | Name/BasicLit/CompositeLit/FuncLit/ParenExpr | 字面量/标识符/括号/复合字面量/函数字面量 |
| EXP-02 | 限定标识符 | Qualified identifiers | `spec:L3149-3171` | SelectorExpr | `pkg.Name`；pkg 须为导入包名 |
| EXP-03 | 复合字面量 | Composite literals | `spec:L3172-3416` | CompositeLit | 数组/切片/映射/结构体；键值对与位置元素；`[...]`；嵌套可省略内层类型；`&T{…}`；越界/重复键编译错误 |
| EXP-04 | 结构体字面量嵌套字段 key | Composite literals (1.27) | `spec:L3172-3416`, `spec:L8842-8857` | CompositeLit/KeyValueExpr | key 可为字段选择器链（如 `a.b: …`），须是 struct 的合法字段 selector |
| EXP-05 | 函数字面量（闭包） | Function literals | `spec:L3417-3448` | FuncLit | 捕获变量语义（引用捕获）；defer/go 中参数即时求值 |
| EXP-06 | 主表达式 | Primary expressions | `spec:L3449-3486` | — | 一元/二元运算与选择器/索引/切片/调用组合表 |
| EXP-07 | 选择器与字段提升 | Selectors | `spec:L3487-3632` | SelectorExpr | 深度规则（最浅者胜、同级歧义错误）；方法选择 |
| EXP-08 | 方法表达式 | Method expressions | `spec:L3633-3753` | SelectorExpr | `T.M`/`(*T).M` → 函数值（首参为接收者） |
| EXP-09 | 方法值 | Method values | `spec:L3754-3871` | SelectorExpr | `x.M` → 绑定接收者的函数值（接收者求值时机） |
| EXP-10 | 索引表达式 | Index expressions | `spec:L3872-4003` | IndexExpr | a[i]：数组/切片/map/字符串/*array；泛型 `f[T]` 即实例化；map 双值 `v, ok`；越界 panic |
| EXP-11 | 切片表达式 | Slice expressions | `spec:L4004-4159` | SliceExpr | 简单 `a[low:high]`；完整 `a[low:high:max]`；省略边界；字符串切片；常量边界检查 |
| EXP-12 | 类型断言 | Type assertions | `spec:L4160-4227` | AssertExpr | `x.(T)`：非接口须先到接口；失败 panic；`v, ok` 形式 |
| EXP-13 | 调用 | Calls | `spec:L4228-4332` | CallExpr | 变参展开；多值返回传递；方法调用；无返回值调用语句 |
| EXP-14 | 变参传递 | Passing arguments to … | `spec:L4333-4383` | CallExpr(HasDots) | `f(s...)`；`...T` 参数类型实为 `[]T` |
| EXP-15 | 泛型实例化 | Instantiations | `spec:L4384-4484` | IndexExpr+ListExpr | `F[T1,T2]` 显式类型实参 |
| EXP-16 | 运算符优先级 | Operator precedence | `spec:L4886-4925` | parser | 5 级：`* / % << >> & &^` > `+ - \| ^` > `== != < <= > >=` > `&&` > `\|\|`；一元最高 |
| EXP-17 | 算术运算符 | Arithmetic operators | `spec:L4926-5058` | Operation | 整数（含 `&^` 清位）、浮点、复数、字符串 `+` |
| EXP-18 | 整数溢出回绕 | Integer overflow | `spec:L5059-5079` | Operation | 有符号/无符号回绕语义（不 panic） |
| EXP-19 | 浮点运算 | Floating-point operators | `spec:L5080-5118` | Operation | IEEE-754；溢出→±Inf；NaN != NaN |
| EXP-20 | 字符串连接 | String concatenation | `spec:L5119-5134` | Operation(+) | `+`；运算数须均为无类型或 string |
| EXP-21 | 比较运算符 | Comparison operators | `spec:L5135-5293` | Operation | 可比较性规则：布尔/数值/字符串/指针/通道/接口/数组/结构体；切片/map/函数仅可判 nil；接口比较（动态类型+值） |
| EXP-22 | 逻辑运算符 | Logical operators | `spec:L5294-5308` | Operation | `&& \|\| !` 短路求值 |
| EXP-23 | 地址运算符 | Address operators | `spec:L5309-5345` | Operation(&/*) | `&x`/`&*p`/`&T{}` 合法组合；`*p` 间接；nil 解引用 panic |
| EXP-24 | 接收运算符 | Receive operator | `spec:L5346-5393` | Operation(<-) | 阻塞接收；`v, ok`（关闭→零值,false）；nil chan 永久阻塞 |
| EXP-25 | 转换 | Conversions | `spec:L5394-5725`（含 `L5626`、`L5726`） | CallExpr | 数值间、整型↔string、[]byte↔string、slice↔array/[N]（1.17 指针/1.20 数组）、指针转换（含 unsafe） |
| EXP-26 | 常量表达式 | Constant expressions | `spec:L5757-5877` | typecheck | 编译期全折叠；非常量仅：转换、len/cap 等白名单 |
| EXP-27 | 求值顺序 | Order of evaluation | `spec:L5879-5949` | codegen | 函数调用、通信、赋值按词法序先行求值；操作数求值顺序保证 |

### 1.6 语句（STM）

| ID | 功能 | Spec 章节 | **Spec 位置** | 涉及 AST | 语义要点 |
|---|---|---|---|---|---|
| STM-01 | 终止语句判定 | Terminating statements | `spec:L5965-6045` | typecheck | return/goto/落尾 panic/无限 for/块尾终止/switch 全覆盖等递归规则 |
| STM-02 | 空语句 | Empty statements | `spec:L6046-6056` | EmptyStmt | `;` |
| STM-03 | 标签语句 | Labeled statements | `spec:L6057-6073` | LabeledStmt | `L: stmt`；goto/break/continue 目标 |
| STM-04 | 表达式语句 | Expression statements | `spec:L6074-6104` | ExprStmt | 仅可为调用/接收（`<-ch`）作为语句 |
| STM-05 | 发送语句 | Send statements | `spec:L6105-6140` | SendStmt | `ch <- v`；阻塞；nil chan 永久阻塞；closed chan panic |
| STM-06 | IncDec 语句 | IncDec statements | `spec:L6141-6165` | AssignStmt | `x++`/`x--` 非表达式 |
| STM-07 | 赋值语句 | Assignment statements | `spec:L6166-6326` | AssignStmt | `=`、`op=`（算术+移位）、元组赋值（多值返回/map 双值/断言/接收 comma-ok）；运算数对数匹配 |
| STM-08 | 短变量声明 | Short variable declarations | `spec:L2866-2918` | AssignStmt(:=) | 至少一个新变量；同块重声明规则（类型须同一） |
| STM-09 | if 语句 | If statements | `spec:L6327-6361` | IfStmt | Init 作用域；else-if 链 |
| STM-10 | 表达式 switch | Expression switches | `spec:L6384-6470` | SwitchStmt | Init；无 tag（true）；case 常量唯一性；fallthrough 仅末尾非默认；tag 须可比较 |
| STM-11 | 类型 switch | Type switches | `spec:L6471-6605` | SwitchStmt+TypeSwitchGuard | `x.(type)`；case 多类型+nil；每 case 隐式新变量绑定类型；tag 须接口类型 |
| STM-12 | for 三形态 | For statements | `spec:L6606-6714` | ForStmt | 单条件 / Init;Cond;Post / range |
| STM-13 | for range 语义 | For range | `spec:L6715-6961` | RangeClause | 数组/切片（索引+元素）、string（rune 索引+码点）、map（无序、随机起点）、chan（直到关闭）、int（1.22）、迭代器函数（1.23，seq/seq2 协议） |
| STM-14 | go 语句 | Go statements | `spec:L6962-6998` | CallStmt(Go) | 表达式求值在调用 goroutine，执行在新 goroutine |
| STM-15 | select 语句 | Select statements | `spec:L6999-7107` | SelectStmt | 就绪 case 均匀随机；default；nil case 永不就绪；空 select 永久阻塞 |
| STM-16 | return 语句 | Return statements | `spec:L7108-7205` | ReturnStmt | "裸 return"（命名结果参数）；多值展开；终止性 |
| STM-17 | break / continue | Break/Continue statements | `spec:L7206-7272` | BranchStmt | 带标签跳出/继续外层 for/switch/select |
| STM-18 | goto 语句 | Goto statements | `spec:L7273-7327` | BranchStmt(Goto) | 不得跳入块/跳过变量声明；标签须存在 |
| STM-19 | fallthrough | Fallthrough statements | `spec:L7328-7340` | BranchStmt(Fallthrough) | 仅表达式 switch 的非末 case 可用 |
| STM-20 | defer 语句 | Defer statements | `spec:L7341-7406` | CallStmt(Defer) | LIFO；参数与接收者即时求值；与 recover 协同 |
| STM-21 | 块内声明语句 | Declarations | `spec:L2170-2252` | DeclStmt | 块内 var/const/type；作用域规则 |

### 1.7 内建函数（BIF）— 预声明全集（18 个）

| ID | 函数 | Spec 章节 | **Spec 位置** | 编译器/Rust 实现要点 |
|---|---|---|---|---|
| BIF-01 | `append(s, …)` | Appending and copying slices | `spec:L7423-7514` | 可变参数特化 `append([]T, T...)`；扩容策略（Runtime）；`append([]byte, string...)` 特例 |
| BIF-02 | `copy(dst, src)` | 同上 | `spec:L7423-7514` | 元素级拷贝；切片↔字符串特例；返回拷贝数 |
| BIF-03 | `clear(m/s/t)` (1.21) | Clear | `spec:L7515-7547`, `spec:L8802-8813` | map→空、slice→零值、type param→各自操作 |
| BIF-04 | `close(ch)` | Close | `spec:L7548-7569` | 关闭通道；唤醒全部接收者（零值）；double-close panic；只发通道关闭 panic |
| BIF-05 | `complex(re, im)` | Manipulating complex numbers | `spec:L7570-7638` | 常量折叠；操作数须均为浮点同宽 |
| BIF-06 | `real(c)` / `imag(c)` | 同上 | `spec:L7570-7638` | 常量折叠；无类型复数→无类型浮点 |
| BIF-07 | `delete(m, k)` | Deletion of map elements | `spec:L7639-7662` | 键类型须可赋值给 map 键 |
| BIF-08 | `len(v)` / `cap(v)` | Length and capacity | `spec:L7663-7733` | 数组（常量折叠！）/切片/map/chan/字符串；数组指针；编译期常量白名单 |
| BIF-09 | `make(T, …)` | Making slices, maps and channels | `spec:L7734-7795` | 三种分配路径（Runtime 入口）；len≤cap 检查 |
| BIF-10 | `max/min(…)` (1.21) | Min and max | `spec:L7796-7860`, `spec:L8802-8813` | 同类数值/字符串；至少一参数 |
| BIF-11 | `new(T)` | Allocation | `spec:L7861-7903` | `*T` 零值分配（Runtime newobject） |
| BIF-12 | `panic(v)` | Handling panics | `spec:L7904-7979` | 启动展开；非 nil interface 参数处理 |
| BIF-13 | `recover()` | 同上 | `spec:L7904-7979` | 仅在 defer 函数直接调用有效；返回 nil/panic 值 |
| BIF-14 | `print` / `println` | Bootstrapping | `spec:L7980-8000` | 非保证格式；编译器内建输出 |
| BIF-15 | 内建引导机制 | Bootstrapping | `spec:L7980-8000` | print/println/实现定义行为；unsafe 手写实现注册 |

### 1.8 包与初始化（PKG）

| ID | 功能 | Spec 章节 | **Spec 位置** | 语义要点 |
|---|---|---|---|---|
| PKG-01 | 包结构与源文件组织 | Packages / Source file organization | `spec:L8002-8012`, `spec:L8013-8026` | 同包多文件共享包块 |
| PKG-02 | package 子句 | Package clause | `spec:L8027-8051` | 每文件首行；非 main 不得导入自身 |
| PKG-03 | import 声明 | Import declarations | `spec:L8052-8130` | Path 常量字符串；命名导入 `x "path"`；点导入 `. "path"`；空导入 `_`（副作用初始化） |
| PKG-04 | 零值 | The zero value | `spec:L8179-8228` | 每类型零值规则表（数值 0/false/nil/空串/复合递归） |
| PKG-05 | 包初始化顺序 | Package initialization | `spec:L8229-8394` | 依赖分析：导入序 → 常量 → 包级变量（按依赖序）→ init()（多 init 按文件序） |
| PKG-06 | 初始化依赖循环检测 | Package initialization | `spec:L8229-8394` | 变量/常量/init 引用循环 → 编译错误 |
| PKG-07 | 程序初始化与 main | Program init / execution | `spec:L8395-8424`, `spec:L8425-8445` | main.main(argv, envp) 签名；不可被调用；不 return（异常退出） |
| PKG-08 | 导出与可见性 | Exported identifiers | `spec:L2304-2320` | 跨包访问控制（编译器符号名 mangle：pkg.Sym） |

### 1.9 系统层（SYS）

| ID | 功能 | Spec 章节 | **Spec 位置** | 语义要点 |
|---|---|---|---|---|
| SYS-01 | unsafe.Pointer 语义 | Package unsafe | `spec:L8492-8649` | 任意指针 ↔ unsafe.Pointer；uintptr 仅数值 |
| SYS-02 | unsafe.Add / Slice / SliceData / String / StringData | Package unsafe (1.17/1.20) | `spec:L8492-8649`, `spec:L8752-8762`, `spec:L8786-8800` | 指针算术与切片/字符串头重构 |
| SYS-03 | Alignof / Offsetof / Sizeof | Package unsafe | `spec:L8492-8649` | 类型布局查询（编译期折叠） |
| SYS-04 | 保证的大小与对齐表 | Size and alignment guarantees | `spec:L8650-8685` | 数值类型宽度/对齐表；结构体字段对齐填充规则 |
| SYS-05 | Errors 与运行时 panic | Errors / Run-time panics | `spec:L8446-8467`, `spec:L8468-8489` | error 接口协议；预声明运行时错误集（越界/nil 解引用/类型断言失败/除零/…） |

---

## 2. 维度 2：AST 节点完备性对账表（nodes.go → Rust 前端）

**对账规则**：`syntax/nodes.go` 中每一个具体节点结构体必须映射为 Rust `enum` 变体或独立 `struct`。46 个节点（1 File + 5 Decl + 23 Expr + 14 Stmt + 3 子句）+ 辅助结构（Group/Comment/Field）全量登记。引用列格式 `nodes:Lstart-end`。

### 2.1 文件与编译单元

| ID | Go 节点 | **源码位置** | Rust 映射 | 说明 |
|---|---|---|---|---|
| AST-01 | `File{PkgName, DeclList, GoVersion, Pragma}` | `nodes:L40-51` | `struct AstFile` | 编译单元；GoVersion 承载 -lang 门控 |
| AST-02 | `Group` | `nodes:L139-141` | `struct DeclGroup` | 分组声明（const/var 组共享 iota 上下文） |
| AST-03 | `Comment{Kind, Text}` | `nodes:L501-505`（Kind 常量 `L492-499`） | `struct Comment` | 4 种位置（Above/Below/Left/Right）；//go: 指令经 Pragma 通道 |

> 基类：`Node` 接口 `nodes:L12-24`；`node` 内嵌结构 `nodes:L26-34`（所有节点共享 pos）；`decl` `nodes:L134-136`。

### 2.2 声明节点（5）

| ID | Go 节点 | **源码位置** | Rust 映射 | 对应功能 |
|---|---|---|---|---|
| AST-04 | `ImportDecl` | `nodes:L64-70` | `enum Decl::Import` | PKG-03 |
| AST-05 | `ConstDecl` | `nodes:L75-82` | `enum Decl::Const` | CON-06/07 |
| AST-06 | `TypeDecl{Alias, TParamList}` | `nodes:L85-93` | `enum Decl::Type` | TYP-20/21/22 |
| AST-07 | `VarDecl` | `nodes:L98-105` | `enum Decl::Var` | PKG-04 |
| AST-08 | `FuncDecl{Recv, TParamList, Type, Body}` | `nodes:L111-119` | `enum Decl::Func` | 含方法（Recv≠nil）与泛型方法（1.27）；Body=nil 为外部声明 |

### 2.3 表达式节点（23）

| ID | Go 节点 | **源码位置** | Rust 映射 | 对应功能 |
|---|---|---|---|---|
| AST-09 | `BadExpr` | `nodes:L162-164` | `Expr::Bad` | 错误恢复占位 |
| AST-10 | `Name` | `nodes:L167-170` | `Expr::Name` | LEX-08/EXP-02 |
| AST-11 | `BasicLit{Kind}` | `nodes:L173-178` | `Expr::BasicLit(LitKind)` | LEX-09..13（IntLit/FloatLit/ImagLit/RuneLit/StringLit） |
| AST-12 | `CompositeLit{NKeys}` | `nodes:L181-187` | `Expr::CompositeLit` | EXP-03/04 |
| AST-13 | `KeyValueExpr` | `nodes:L190-193` | `Expr::KeyValue` | EXP-03 |
| AST-14 | `FuncLit` | `nodes:L196-200` | `Expr::FuncLit` | EXP-05 |
| AST-15 | `ParenExpr` | `nodes:L203-206` | `Expr::Paren` | 求值分组 |
| AST-16 | `SelectorExpr` | `nodes:L209-213` | `Expr::Selector` | EXP-02/07/08/09 |
| AST-17 | `IndexExpr` | `nodes:L217-221` | `Expr::Index` | EXP-10/15（索引或泛型实例化，二义由类型检查消解） |
| AST-18 | `SliceExpr{Index[3], Full}` | `nodes:L224-233` | `Expr::Slice` | EXP-11 |
| AST-19 | `AssertExpr` | `nodes:L236-240` | `Expr::TypeAssert` | EXP-12 |
| AST-20 | `TypeSwitchGuard{Lhs}` | `nodes:L244-248` | `Expr::TypeSwitchGuard` | STM-11 |
| AST-21 | `Operation{Op, X, Y}` | `nodes:L250-254` | `Expr::Unary / Expr::Binary` | EXP-16..24（一元 Y=nil） |
| AST-22 | `CallExpr{HasDots}` | `nodes:L257-262` | `Expr::Call` | EXP-13/14/25（转换亦为调用语法） |
| AST-23 | `ListExpr` | `nodes:L265-268` | `Expr::List` | 多返回值/泛型类型实参列表载体 |
| AST-24 | `ArrayType{Len=nil→...}` | `nodes:L271-276` | `Type::Array` | TYP-05 |
| AST-25 | `SliceType` | `nodes:L279-282` | `Type::Slice` | TYP-06 |
| AST-26 | `DotsType` | `nodes:L285-288` | `Type::Variadic` | TYP-09（...T） |
| AST-27 | `StructType{FieldList, TagList}` | `nodes:L291-295` | `Type::Struct` | TYP-07 |
| AST-28 | `Field{Name=nil→匿名}` | `nodes:L299-303` | `struct Field` | 字段/参数/结果/接口方法/嵌入元素统一载体 |
| AST-29 | `InterfaceType` | `nodes:L306-309` | `Type::Interface` | TYP-10/11/12 |
| AST-30 | `FuncType{ParamList, ResultList}` | `nodes:L311-315` | `Type::Func` | TYP-09 |
| AST-31 | `MapType` | `nodes:L318-321` | `Type::Map` | TYP-14 |
| AST-32 | `ChanType{Dir}` | `nodes:L326-330`（ChanDir `L340-346`） | `Type::Chan(ChanDir)` | TYP-15（SendOnly/RecvOnly） |

> 基类：`Expr` 接口 `nodes:L154-158`；`expr` 内嵌 `nodes:L333-336`（含 typeAndValue 类型检查结果）。

### 2.4 语句节点（14）

| ID | Go 节点 | **源码位置** | Rust 映射 | 对应功能 |
|---|---|---|---|---|
| AST-33 | `EmptyStmt` | `nodes:L362-364` | `Stmt::Empty` | STM-02 |
| AST-34 | `LabeledStmt` | `nodes:L366-370` | `Stmt::Labeled` | STM-03 |
| AST-35 | `BlockStmt` | `nodes:L372-376` | `Stmt::Block` | SCP-01 |
| AST-36 | `ExprStmt` | `nodes:L378-381` | `Stmt::Expr` | STM-04 |
| AST-37 | `SendStmt` | `nodes:L383-386` | `Stmt::Send` | STM-05 |
| AST-38 | `DeclStmt` | `nodes:L388-391` | `Stmt::Decl` | STM-21 |
| AST-39 | `AssignStmt{Op, Lhs, Rhs}` | `nodes:L393-397` | `Stmt::Assign{kind}` | STM-06/07/08（Op=0&Rhs=nil → IncDec；`:=` 变体） |
| AST-40 | `BranchStmt{Tok, Label, Target}` | `nodes:L399-409` | `Stmt::Branch(BranchTok)` | STM-17/18（Target 由 parser 的 CheckBranches 计算，见 `syntax/branches.go`） |
| AST-41 | `CallStmt{Tok: Go\|Defer, Call, DeferAt}` | `nodes:L411-416` | `Stmt::Call` | STM-14/20 |
| AST-42 | `ReturnStmt` | `nodes:L418-421` | `Stmt::Return` | STM-16 |
| AST-43 | `IfStmt{Init, Cond, Then, Else}` | `nodes:L423-429` | `Stmt::If` | STM-09 |
| AST-44 | `ForStmt{Init, Cond, Post, Body}` | `nodes:L431-437` | `Stmt::For` | STM-12/13（Init 可含 RangeClause） |
| AST-45 | `SwitchStmt{Init, Tag, Body}` | `nodes:L439-445` | `Stmt::Switch` | STM-10/11 |
| AST-46 | `SelectStmt{Body}` | `nodes:L447-451` | `Stmt::Select` | STM-15 |

> 基类：`Stmt`/`SimpleStmt` 接口 `nodes:L352-360`；`stmt` `nodes:L477-479`；`simpleStmt` `nodes:L481-485`（区分简单语句，用于 if/for/switch 的 Init）。

### 2.5 辅助节点（3）

| ID | Go 节点 | **源码位置** | Rust 映射 | 对应功能 |
|---|---|---|---|---|
| AST-47 | `RangeClause{Lhs, Def, X}` | `nodes:L455-460` | `Stmt::RangeClause` | STM-13（`Def` 标记 `:=`） |
| AST-48 | `CaseClause{Cases, Body}` | `nodes:L462-467` | `struct CaseClause` | STM-10/11（Cases=nil → default） |
| AST-49 | `CommClause{Comm, Body}` | `nodes:L469-474` | `struct CommClause` | STM-15（Comm=nil → default） |

**对账结论**：46 个节点结构体 + 3 个辅助子句 + 基类（Node/node/expr/stmt/simpleStmt/decl）+ Group/Comment/Field 全部登记，AST 覆盖率 = **100%**。语法层面不存在无法解析的合法 Go 1.27 代码。

---

## 3. 维度 3：Runtime 五大抽象内核

> 引用列为官方参考实现位置（`$R/src/runtime/…` 或 `$R/src/internal/abi/…`）。Rust Runtime 需提供**等价语义**，不需复刻结构。

### 3.1 内存与 GC（RT-MEM）

| ID | 功能 | **参考实现** | 需求描述 |
|---|---|---|---|
| RT-MEM-01 | 多级分配器 | `runtime/malloc.go:405`(mallocinit)、`runtime/mheap.go:422`(mspan)、`runtime/mcache.go`、`runtime/mcentral.go`、`gc/`(SizeClassToSize) | 类 tcmalloc：线程本地缓存 → 中心空闲列表 → 页堆；Span/SizeClass 分级 |
| RT-MEM-02 | 小对象/大对象路径 | `runtime/mheap.go`、`runtime/mbitmap.go` | ≤32KB 走 SizeClass；>32KB 直接页分配 |
| RT-MEM-03 | 栈内存管理 | `runtime/stack.go` | 协程栈：初始小栈动态增长（拷贝式）；GC 时收缩 |
| RT-MEM-04 | GC：初版 STW Mark-Sweep | `runtime/mgc.go:733`(gcStart)、`runtime/mgcmark.go`、`runtime/mgcsweep.go` | 根扫描（全局/栈/寄存器）→ 三色标记 → 清扫；STW 安全点协议 |
| RT-MEM-05 | GC：并发标记演进 | `runtime/mgc.go`、`runtime/mwbbuf.go`(写屏障缓冲) | 混合写屏障；终止标记 STW；后台清扫 goroutine |
| RT-MEM-06 | 堆位图与指针元数据 | `runtime/mbitmap.go`、`gc/typebits/typebits.go` | allocation header 含类型指针 → _type；ptrmask 位图驱动扫描 |
| RT-MEM-07 | 统一分配入口 | `runtime/malloc.go:1067`(mallocgc)、`runtime/malloc.go:2140`(newobject) | new/make/compositeLit 逃逸 → `rt_alloc(typedesc, size, needzero)` |
| RT-MEM-08 | 内存统计与限制 | `runtime/mprof.go`、`runtime/mstats.go` | HeapAlloc/HeapObjects；软内存上限 |

### 3.2 调度与上下文（RT-SCH）

| ID | 功能 | **参考实现** | 需求描述 |
|---|---|---|---|
| RT-SCH-01 | 协程控制块 Task | `runtime/runtime2.go:471`(g) | goid、栈边界、状态（_Grunnable/_Grunning/_Gwaiting/_Gdead）、defer 链表头、panic 链 |
| RT-SCH-02 | 有栈上下文切换 | `runtime/asm_amd64.s`、`runtime/asm_arm64.s`(gogo/mcall) | 汇编实现保存/恢复被调用者保存寄存器+SP+PC（amd64/arm64 双后端） |
| RT-SCH-03 | GMP 模型 | `runtime/runtime2.go:616`(m)、`:774`(p)、`:932`(schedt)、`runtime/proc.go:4150`(schedule) | 全局运行队列 + P 本地队列 + work-stealing |
| RT-SCH-04 | go 语句实现 | `runtime/proc.go:5334`(newproc)、`runtime/runtime2.go:179`(funcval) | 拷贝参数到新栈、挂入运行队列 |
| RT-SCH-05 | 阻塞点调度 | `runtime/proc.go:457`(gopark)、`:493`(goready)、`runtime/runtime2.go:404`(sudog) | chan 阻塞、阻塞 syscall、Sleep → 让出 CPU；goroutine 挂起/唤醒 |
| RT-SCH-06 | 生命周期与调度循环 | `runtime/proc.go:1864`(mstart)、`runtime/proc.go:4150`(schedule) | runtime 主入口；exit 语义；main goroutine 结束即进程结束 |
| RT-SCH-07 | 抢占与安全点 | `runtime/preempt.go` | 长运行代码安全点检查（编译器在循环回边插桩，配合 RT-MEM-04） |
| RT-SCH-08 | 定时器子系统 | `runtime/time.go` | 四叉堆/时间轮；Sleep 语义；select 超时 |

### 3.3 复合类型支持结构（RT-CMP）

| ID | 功能 | **参考实现** | 需求描述 |
|---|---|---|---|
| RT-CMP-01 | Slice 头结构体 | `internal/unsafeheader/unsafeheader.go:22`(Slice)、`runtime/slice.go:102`(makeslice)、`:178`(growslice) | `{ptr, len, cap}`（24B amd64）；append 扩容阈值语义 |
| RT-CMP-02 | String 头结构体 | `internal/unsafeheader/unsafeheader.go:34`(String)、`runtime/string.go` | `{ptr, len}`；运行时字符串常量表；string(rune) 编码转换 |
| RT-CMP-03 | Map 桶算法 | `runtime/map.go:62`(makemap)、`internal/runtime/maps/map.go`、`internal/runtime/maps/group.go` | SwissTable/桶+溢桶：负载因子、扩容翻倍、渐进式搬迁 |
| RT-CMP-04 | Map 迭代随机化 | `internal/runtime/maps/map.go`(iter)、`runtime/map.go` | 迭代起点随机 → STM-13 无序语义 |
| RT-CMP-05 | Channel 结构 | `runtime/chan.go:34`(hchan)、`runtime/runtime2.go:404`(sudog) | 环形缓冲区 + 发送/接收等待队列（sudog 双向链表）+ 锁 |
| RT-CMP-06 | Channel 语义全套 | `runtime/chan.go:176`(chansend)、`:524`(chanrecv)、`:414`(closechan) | 直接发送到等待接收者；关闭广播唤醒；nil/满/空三态阻塞矩阵；select 随机探活 |
| RT-CMP-07 | 数组/结构体表示 | `runtime/malloc.go`(zerobase)、`runtime/mbitmap.go` | 值语义（拷贝）；零大小类型统一 0 地址 |

### 3.4 多态与反射元数据（RT-POLY）

| ID | 功能 | **参考实现** | 需求描述 |
|---|---|---|---|
| RT-POLY-01 | `_type` 元数据结构 | `abi/type.go:21`(Type=_type)、`:270`(ArrayType)、`:299`(ChanType)、`:472`(InterfaceType)、`:504`(SliceType)、`:520`(FuncType)、`:568`(PtrType)、`:583`(StructType) | size/ptrbytes/hash/tflag/kind/compare/equal/hash/GCData → 驱动 RT-MEM-06 扫描 |
| RT-POLY-02 | 接口值布局 | `runtime/runtime2.go:184`(iface)、`:189`(eface) | iface `{tab *itab, data}`（16B）；eface `{_type, data}`（即 any，16B） |
| RT-POLY-03 | itab 动态分派虚表 | `abi/iface.go:17`(ITab)、`runtime/iface.go:44`(getitab)、`gc/reflectdata/reflect.go` | inter↔concrete 哈希缓存；运行时生成/查找；方法表按字典序 |
| RT-POLY-04 | 类型断言/switch 代码生成 | `runtime/iface.go`、`gc/reflectdata/`、`gc/walk/` | iface 断言：itab 查找 + on-the-fly 构建；eface 断言：_type 比较 |
| RT-POLY-05 | 泛型分派方案 | `gc/reflectdata/`、`abi/type.go`(字典)、`gc/noder/` | GC shape 字典（相同指针布局共享实例化 + 字典传参）；初版可全量单态化（正确性优先） |
| RT-POLY-06 | 反射元数据支撑 | `runtime/type.go`、`abi/type.go:702`(NewName)、`abi/map.go:32`(MapType) | 方法表/字段偏移/Tag/类型名；kind 全枚举 |
| RT-POLY-07 | method value 闭包化 | `gc/walk/closure.go`、`runtime/runtime2.go:179`(funcval) | x.M 编译为 `{fn, recv}` 闭包对象 |

### 3.5 异常与展开（RT-EXC）

| ID | 功能 | **参考实现** | 需求描述 |
|---|---|---|---|
| RT-EXC-01 | defer 链表管理 | `runtime/runtime2.go:1154`(_defer)、`runtime/panic.go:353`(deferproc)、`gc/ssagen/` | open-coded defer（≤8 位掩码+指针数组）、栈上/堆上 defer 三级方案 |
| RT-EXC-02 | panic 级联展开 | `runtime/panic.go:809`(gopanic) | panic 结构 {_type, data, 嵌套链}；G 状态 _Gpanicking；逐帧回溯执行 defer |
| RT-EXC-03 | recover 拦截器 | `runtime/panic.go:1083`(gorecover) | 仅 defer 直接函数体内有效；捕获后恢复到 deferreturn 序言 |
| RT-EXC-04 | 运行时致命错误 | `runtime/panic.go`(fatal/throw) | fatal throw（不可 recover）：并发 map 写、栈耗尽、OOM；与 panic(v) 路径区分 |
| RT-EXC-05 | 栈展开与 traceback | `runtime/traceback.go`、`link/ld/`(pclntab)、`gc/dwarfgen/` | PC → 帧信息（需 RT-POLY-01 元数据 + pclntab）；goroutine 崩溃时打印 |
| RT-EXC-06 | 编译器协同 | `gc/ssagen/ssa.go`、`gc/walk/`、`gc/deadlocals/` | defer 插桩点（序言/return 前）；loop defer 堆分配判定（逃逸分析协同） |

---

## 4. 编译器流水线功能（PIPE）

> 引用列指向官方参考实现 `$R/src/cmd/compile/internal/…`（`gc/` 即 `cmd/compile/internal/`）或 `$R/src/cmd/link/…`。

### 4.1 前端

| ID | 功能 | **参考源码** | 说明 |
|---|---|---|---|
| PIPE-01 | 编译驱动与 CLI | `$R/src/cmd/compile/main.go`、`gc/base/flag.go`、`gc/base/base.go` | -p 包路径、-lang 版本门控、-o 输出、-I 导入路径 |
| PIPE-02 | 词法扫描器 | `gc/syntax/scanner.go`、`gc/syntax/source.go`、`gc/syntax/pos.go` | LEX-01..08 全量；分号插入；位置追踪 Pos(file,line,col) |
| PIPE-03 | 解析器 | `gc/syntax/parser.go`、`gc/syntax/nodes.go`、`gc/syntax/branches.go` | AST-01..49 全量；错误恢复（BadExpr/跳读）；类型 vs 表达式二义消解 |
| PIPE-04 | 语言版本门控 | `gc/base/flag.go`(-lang)、`gc/syntax/syntax.go` | go.mod -lang 与文件 //go:build；按 Spec Appendix 放行/拒绝特性 |
| PIPE-05 | Pragma/编译指令 | `gc/syntax/`(pragma 解析)、`gc/base/debug.go` | //go:build、//go:linkname、//go:nosplit、//go:noescape |
| PIPE-06 | 导入解析 | `gc/importer/`、`gc/types2/` | 编译包依赖：读取导出数据（类型/常量/内联体） |

### 4.2 类型检查与 IR（中端前端）

| ID | 功能 | **参考源码** | 说明 |
|---|---|---|---|
| PIPE-07 | 类型检查器 | `gc/types2/check.go`、`gc/types2/*.go` | TYP/SCP/EXP 全语义 |
| PIPE-08 | 常量折叠引擎 | `gc/types2/const.go` | ≥256 位精度（CON-02）；四则/移位/比较/转换全套 |
| PIPE-09 | 作用域解析与遮蔽 | `gc/types2/scope.go`、`gc/types2/resolver.go` | SCP-01..10 |
| PIPE-10 | IR 树构建 | `gc/noder/`、`gc/ir/` | AST→IR（Oxxx 节点）；位置保真 |
| PIPE-11 | 泛型处理 | `gc/types2/infer.go`、`gc/types2/unify.go`、`gc/noder/` | 类型参数 → 实例化/字典（RT-POLY-05） |
| PIPE-12 | 内联 | `gc/inline/inl.go`、`gc/inline/inlheur/` | 小函数体替换；内联成本模型 |
| PIPE-13 | 逃逸分析 | `gc/escape/escape.go`、`gc/escape/graph.go`、`gc/escape/solve.go` | 图算法：参数/返回值/defer 逃逸 → 堆/栈分配决策（联动 RT-MEM-07） |
| PIPE-14 | 闭包转换 | `gc/walk/closure.go` | FuncLit 捕获分析 → 按值/按引用捕获（context struct） |
| PIPE-15 | 方法与接口降级 | `gc/walk/`、`gc/devirtualize/` | 方法值/表达式 → 闭包/函数；itab 构造调用 |
| PIPE-16 | 内建函数降级 | `gc/walk/builtin.go`、`gc/walk/complit.go` | BIF-01..15 → runtime 调用/编译期折叠 |
| PIPE-16b | range over func 降级 (1.23) | `gc/rangefunc/rewrite.go` | 迭代器函数 → 循环（STM-13） |
| PIPE-16c | 循环变量语义 (1.22) | `gc/loopvar/` | 每迭代独立变量（SCP-09） |

### 4.3 后端

| ID | 功能 | **参考源码** | 说明 |
|---|---|---|---|
| PIPE-17 | SSA 构建 | `gc/ssa/`(block.go/cfg.go) | IR→SSA；块/值/Phi |
| PIPE-18 | SSA 优化 passes | `gc/ssa/opt.go`、`gc/ssa/*.go` | 常量折叠/死代码消除/复制传播/CSE/窥孔 |
| PIPE-19 | 机器相关 lowering | `gc/ssagen/ssa.go`、`gc/amd64/`、`gc/arm64/` | amd64 / arm64 双后端 |
| PIPE-20 | 指令选择与寄存器分配 | `gc/ssa/regalloc.go`、`gc/amd64/`、`gc/arm64/` | 线性扫描；Go ABI internal（寄存器传参，`gc/abi/`、`gc/ssagen/abi.go`） |
| PIPE-21 | 栈帧布局与活跃性 | `gc/ssagen/pgen.go`、`gc/liveness/plive.go`、`gc/liveness/intervals.go` | 帧大小/变量偏移/逃逸位图（GC 栈扫描联动 RT-MEM-06） |
| PIPE-22 | 对象文件与元数据输出 | `gc/objw/objw.go`、`gc/gc/obj.go`、`gc/gc/export.go`、`gc/staticdata/data.go`、`gc/rttype/rttype.go`、`gc/typebits/typebits.go`、`gc/dwarfgen/dwarf.go` | 符号表/重定位/pclntab/类型元数据（traceback 需要，RT-EXC-05） |
| PIPE-23 | 链接器 | `$R/src/cmd/link/internal/ld/main.go`、`ld/deadcode.go`、`link/loader/`、`link/sym/` | 符号解析/重定位/死代码消除/全局符号表；生成 main 入口 |
| PIPE-24 | 包初始化与启动运行时 | `gc/pkginit/init.go`、`runtime/rt0_*.s`、`runtime/proc.go:1864`(mstart) | init 拓扑（PKG-05）；汇编入口 → 内存分配器初始化 → 调度器初始化 → main.main |

### 4.4 错误诊断与工具性

| ID | 功能 | **参考源码** | 说明 |
|---|---|---|---|
| PIPE-25 | 诊断系统 | `gc/types2/errors.go`、`gc/base/` | 位置化错误/警告；错误恢复继续编译；多行提示 |
| PIPE-26 | 阶段调试 dump | `gc/base/debug.go`(-d=ssa)、`gc/ssa/html.go` | 编译各阶段 dump（开发期质量保障） |
| PIPE-27 | 汇编器支持 | `$R/src/cmd/asm/` | 对应 .s 文件（本仓库含 503 个 .s）；初版可仅链接预编译 runtime 汇编 |
| PIPE-28 | 覆盖/调试信息 | `gc/coverage/`、`gc/dwarfgen/dwarf.go`、`gc/logopt/` | -cover、DWARF、PGO 日志 |

---

## 5. 语言版本特性矩阵（Go 1.9 → 1.27）

> 引用列指向 `$R/doc/go_spec.html` 的 Appendix/Language versions 各版本小节。

| 版本 | 特性 | 功能 ID | **Spec 位置** |
|---|---|---|---|
| 1.9 | 类型别名 `type A = B` | TYP-21 | `spec:L8714-8720` |
| 1.13 | `0b/0o` 前缀、十六进制浮点、`_` 数字分隔、有符号移位计数 | LEX-09/10/11、CON-08 | `spec:L8721-8743` |
| 1.14 | 嵌入接口重复方法不再报错 | TYP-10 | `spec:L8744-8751` |
| 1.17 | slice→array 指针转换、unsafe.Add/Slice | EXP-25、SYS-02 | `spec:L8752-8763` |
| 1.18 | 泛型：`~` token、类型参数、通用接口、any/comparable | LEX-06、TYP-11/22/23/24/25、AST-06/08 | `spec:L8764-8785` |
| 1.20 | slice→array 转换、unsafe.SliceData/String/StringData、comparable 放宽 | EXP-25、SYS-02、TYP-23 | `spec:L8786-8801` |
| 1.21 | min/max/clear 内建、接口方法类型推断、泛型函数值推断 | BIF-03/10、TYP-25 | `spec:L8802-8814` |
| 1.22 | for 迭代变量每迭代独立、range over int | SCP-09、STM-13 | `spec:L8815-8826` |
| 1.23 | range over func 迭代器 | STM-13 | `spec:L8827-8833` |
| 1.24 | 泛型别名 | TYP-21、AST-06 | `spec:L8834-8841` |
| 1.27 | 赋值上下文函数类型推断、泛型方法声明、struct 字面量嵌套字段 key | TYP-25、AST-08、EXP-04 | `spec:L8842-8858` |

---

## 6. 覆盖率验收清单

- [ ] LEX 13 项：scanner 逐条测试（含 unicode_test），对照 `spec:L79-631`
- [ ] CON 8 项：常量折叠引擎 ≥256 位精度用例，对照 `spec:L633-739`、`spec:L5757-5877`
- [ ] TYP 26 项：类型同一性/可赋值性/方法集/泛型统一算法，对照 `spec:L799-2129`、`spec:L2465-2810`
- [ ] SCP 10 项：作用域/遮蔽/标签/goto 合法性，对照 `spec:L2130-2330`
- [ ] EXP 27 项：优先级/求值顺序/转换全集，对照 `spec:L3104-5949`
- [ ] STM 21 项：控制流/defer/recover 语义，对照 `spec:L5950-7406`
- [ ] BIF 15 项：18 个内建函数全实现，对照 `spec:L7407-8000`
- [ ] PKG 8 项：init 拓扑排序 + 循环检测，对照 `spec:L8002-8445`
- [ ] SYS 5 项：unsafe + 布局表，对照 `spec:L8446-8685`
- [ ] AST 49 项对账：Rust enum 变体逐一核对 `nodes:L12-505`
- [ ] RT 五大子系统：MEM 8 / SCH 8 / CMP 7 / POLY 7 / EXC 6，对照 `$R/src/runtime/` 与 `$R/src/internal/abi/`
- [ ] PIPE 28 项：从 CLI 到链接的完整流水线，对照 `$R/src/cmd/compile/internal/` 与 `$R/src/cmd/link/`
- [ ] 版本矩阵：1.9→1.27 每条特性有 -lang 门控测试，对照 `spec:L8688-8858`

**总量**：Spec 语义原子 133 项 + AST 对账 49 项 + Runtime 需求 36 项 + 流水线功能 28 项 ≈ **246 个可追踪功能点**；每项均附规范性（Spec/AST）或参考性（Runtime/编译器）**引用位置**，构成闭环覆盖矩阵。
