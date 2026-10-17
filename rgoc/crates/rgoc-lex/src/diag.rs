//! 诊断 —— `C1` 契约的另一半（诊断表示与排序）。
//!
//! 来源：`docs/contracts/C1-source-map.md` §3 / §4。
//!
//! # 为什么 `DiagCode` 是契约的一部分而不是注释
//!
//! `code` 看着只是个标识，但它**参与排序**（`C1` §4 的第 4 个排序键）。
//! 所以它必须：
//!
//! - **稳定** —— 改了代号就改变诊断顺序 ⇒ 会改变与 golden 的比对结果 ⇒ 回归。
//! - **可排序** —— 统一 `大写-前缀-小写词`（`LEX-...`），无大小写混排陷阱。
//! - **可分组** —— 前缀标识来源子系统（`LEX-`），「某一类问题」能被一次性筛出来。
//!
//! 反面做法是把 `message` 当排序键 —— 文案一改就重排全部诊断。
//!
//! # 消息文本不复刻官方
//!
//! `03` §3.4 规定：诊断先对齐**拒绝/接受、位置和预定义类别**，**文本另列指标**。
//! 所以 golden 只存 `行:列 + DiagCode`（`M1-design.md` D-M1-9），
//! `message` 是给人看的，不参与任何比对。
//!
//! # 这张码表是**枚举**出来的，不是想出来的
//!
//! 第一版只有 19 个代号，是**凭印象**从官方 scanner 的报错消息里挑的 —— 结果漏了 4 个
//! （`LEX-BAD-DIGIT` / `LEX-BAD-RADIX-POINT` / `LEX-STRING-NEWLINE` / `LEX-ESCAPE-BAD-DIGIT`）。
//! 漏掉时**不会报错**：那 4 条消息出现时行为未定义 —— 与 `AGENTS.md` 记的
//! 「写死的键列表会变成盲区」同形。
//!
//! 现在是**枚举** `scanner.go` / `source.go` 里全部 `errorf`/`errorAtf`/`error` 调用点
//! （26 处 ⇒ 去重 25 条消息模板）得到 22 个官方代号，再加 1 个本仓自己的限制，共 **23**（M1 冻结）。
//! M2 增补 3 个 `SYN-` 前缀语法代号（见 [`DiagCode`] 末尾），**当前共 26**。
//!
//! 两侧（Go 的 `tools/token-oracle/diagmap.go` 与这里）必须同步 ——
//! 由 `lex_代号表与_go_侧映射表逐字一致` 跨语言对撞，而不是靠人记得同步。
//!
//! ⚠️ 但那只对撞 **`LEX-` 子表**：token oracle 的映射表是词法专用的，`SYN-` 代号在那张表里
//! 永远不可能被产出（它的自检 ② 要求每个代号都有官方消息能产出）。`SYN-` 的半边由
//! `tools/ast-oracle` 的镜像表覆盖（T-AP16）。前缀与镜像表的对应关系见
//! `diag_code_的_as_str_前缀落在阶段白名单内`。

use crate::pos::Pos;

/// 词法诊断代号。
///
/// **变体的声明顺序 == `as_str()` 的字典序**。这条由
/// `diag_code_的声明顺序与_as_str_字典序一致` 钉住 —— 不一致时，
/// 「同位置按 code 排序」在 Rust 侧（按 `Ord` = 声明顺序）与 golden 侧
/// （按字符串）会给出**不同的顺序**，而那种不一致只在两条诊断同位置时才暴露。
///
/// 加新代号时：**插入到它字典序该在的位置**，然后让那条断言告诉你有没有放对。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum DiagCode {
    /// 不认识的字符
    BadChar,
    /// 进制字面量里出现了不属于该进制的数位（如 `0b12` 里的 `2`）
    BadDigit,
    /// 指数缺数字 / 进制与尾数不匹配
    BadExponent,
    /// 标识符里混入非法字符
    BadIdentChar,
    /// 进制字面量里出现小数点（如 `0b1.0`）—— 只有十进制与十六进制允许
    BadRadixPoint,
    /// `_` 没有分隔连续的数码
    BadSep,
    /// 文件中部出现 BOM
    BomMidFile,
    /// 块注释未终止
    CommentUnterminated,
    /// 转义序列里的数位非法（如 `\xZZ`）
    EscapeBadDigit,
    /// 八进制转义值 > 255
    EscapeOctalOver,
    /// 转义值超范围或落在代理区
    EscapeRange,
    /// 未知转义
    EscapeUnknown,
    /// 超出单文件字节上限（**官方无对应行为**，是本仓的实现限制）
    ExceedsFileLimit,
    /// 标识符以数字开头
    IdLeadingDigit,
    /// NUL 字符
    InvalidNul,
    /// 非法的 UTF-8 编码
    InvalidUtf8,
    /// 进制前缀之后没有数字
    LitNoDigits,
    /// rune 字面量为空
    RuneEmpty,
    /// rune 字面量多于一个字符
    RuneMulti,
    /// rune 字面量里出现换行
    RuneNewline,
    /// rune 字面量未终止
    RuneUnterminated,
    /// 解释型字符串里出现裸换行
    StringNewline,
    /// 字符串未终止
    StringUnterminated,

    // ── M2 语法分析（`SYN-` 前缀）──
    //
    // ⚠️ **声明顺序必须与 `as_str()` 的字典序一致** —— `Ord` 是**声明顺序**，
    // 而 `C1` §4.2 / golden 按**字符串**排。两者不一致时「同位置多条诊断按
    // code 排」在 Rust 侧与 golden 侧会给出不同顺序，而那种不一致**只在两条
    // 诊断碰巧同位置时暴露**（极难定位）。
    // `diag_code_的声明顺序与_as_str_字典序一致` 那条单测是它的守卫 ——
    // 加变体时若忘了按字典序插进去，它立刻红。
    //
    // 本段 11 个变体 = 早先的 3 个（`SynExpectedSemicolon` /
    // `SynInvalidLineDirective` / `SynUnexpectedToken`，`T-AP06`/`T-AP15` 落的）
    // + `T-AP16` 新增的 8 个（官方 `branches.go` 的 12 处 `errf` 收敛成的 8 类）。
    /// `break` 不在循环 / `switch` / `select` 里，或标签指向的不是这三者
    ///
    /// 官方 `branches.go:218`（`break is not in a loop, switch, or select`）与
    /// `:261`（`invalid break label %s`）。
    SynBranchBreak,
    /// `continue` 不在循环里，或标签指向的不是 `for`
    ///
    /// 官方 `branches.go:224` 与 `:274`（`invalid continue label %s`）。
    SynBranchContinue,
    /// `fallthrough` 位置不对（在 `switch` 之外 / 类型 `switch` 里 / 最后一个 `case`）
    ///
    /// 官方 `branches.go:227` / `:230` / `:235` —— **三条消息共用一个代号**：
    /// 它们是**同一个检查**（`fallthrough` 是否合法）的三种失败文案，
    /// 走**同一次** `errf`（`msg` 是变量）。
    SynBranchFallthrough,
    /// 期望 `;`
    SynExpectedSemicolon,
    /// `goto` 跳进了块内部（官方 `branches.go:32`）
    SynGotoIntoBlock,
    /// `goto` **跳过**了变量声明（官方 `branches.go:194`）
    SynGotoJumpsOverDecl,
    /// `//line` 指令格式非法
    SynInvalidLineDirective,
    /// 标签在同一块里**重复**定义（官方 `branches.go:78`）
    SynLabelDuplicate,
    /// `goto`/`break`/`continue` 的标签**未定义**
    /// （官方 `branches.go:34` / `264` / `277`）
    SynLabelUndefined,
    /// 标签定义后**从未使用**（官方 `branches.go:42`，spec 要求）
    SynLabelUnused,
    /// 递归嵌套超过本仓上限（**`H` 判据：官方无此行为**，`T-A-96`）
    ///
    /// 官方编译器靠 Go 运行时的可增长栈；我们落在固定的宿主栈上，
    /// 必须自设上限防栈溢出。上限 1000（`parser.rs` 的 `MAX_NEST_DEPTH`），
    /// 每文件只报一次。
    SynNestingTooDeep,
    /// 意外的词元
    SynUnexpectedToken,

    // ── M3 类型检查（`TYP-` 前缀）──
    //
    // 同 `SYN-` 段：**声明顺序必须与 `as_str()` 的字典序一致**。
    //
    // 这两条是 M3/Task 7 落下的**移位**代号，刻意**贴合官方 `go/types` 的
    // 检查粒度**（`go/types/expr.go:630-700`）：
    //
    // - 官方的 `InvalidShiftCount` 用**同一个** `errorf` 覆盖三种失败
    //   （负计数 / 非 uint 计数 / 计数超 1074），所以这里也只给**一个**代号；
    //   拆细会让我们在「按代号比对」时与 oracle 的类别对不上。
    // - 同理 `InvalidShiftOperand` 一条。
    //
    // ⚠️ 计划与 `M3-tests.md` 冻结稿都写「**左值**非负检查」，**实测是错的**
    // （oracle：`-1 << 2 == -4` 合法）。见 `M3-tests.md` 修订 R1 与
    // `.superpowers/sdd/M3-plan/progress.md` 的 Task 7 Ruling。
    /// 包名标识符是**空标识符** `_` —— 官方 `BlankPkgName`（消息 `invalid package name _`）
    ///
    /// 位置取 **`package` 名字标识符**处（官方 `go/types/check.go:299`
    /// 的 `check.error(file.Name, …)`）—— 这正是它**区别于** `TypMismatchedPkgName`
    /// 的地方（那条取 `package` 关键字处）。
    /// 触发面与官方 `initFiles` 的 switch 同形：**在 `pkg.name` 还是空串时**遇到
    /// `package _` 才报（此后再遇到 `package _` 落进 `MismatchedPkgName`）。
    TypBlankPkgName,
    /// 被依赖包**装不上**（源码目录里没有可用的 `.go` 文件）—— 官方 `BrokenImport`
    /// （消息 `could not import %s (%s)`，`go/types/resolver.go:305`）
    ///
    /// 官方把**一切** import 失败都归到这条（`go/types` 只负责把 importer 的错误
    /// 包起来，真正的探测在 importer 一侧）——它的注释里举的例子正是
    /// `import "amissingpackage"`。
    ///
    /// ⚠️ 计划稿点名的是 `ImportNotFound`，官方**没有**这个代号（全表只有
    /// `BadImportPath` / `BrokenImport` / `ImportCRenamed` / `UnusedImport` /
    /// `UndeclaredImportedName`）⇒ 按「代号照官方」改用实名（同 `TypDuplicateDecl` 那次）。
    /// 这里只承载**「目录在、但没有 `.go`」**这一种（触发者 `T-TY-85` 的反向用例）；
    /// 「roots 里根本没有这个路径」落 [`Self::TypUnsupportedImportForm`]。
    TypBrokenImport,
    /// 常量**除数为零**（`/` 与 `%`，整数与浮点都算）—— 官方 `DivByZero`
    ///
    /// 实测官方四条都报它：`1/0`、`1%0`、`1.0/0`、`1/0.0`（消息是
    /// `invalid operation: division by zero`，其中 `invalid operation: `
    /// 是通用前缀，类别归 `DivByZero`）。
    TypDivByZero,
    /// 同一个作用域里**重复声明** —— 官方 `DuplicateDecl`
    ///
    /// 官方 `go/types/decl.go:22-30`（`check.declare`）在**任何**作用域上都用同一条码，
    /// 消息固定为 `%s redeclared in this block` —— **包括包级**（实测：同文件与跨文件的
    /// 包级重复都写 "block"，`go/types` 并不区分）。⚠️ `M3-plan.md` Task 25 写的
    /// 「包级 `RedeclaredInThisPackage`」在官方**不存在**（`internal/types/errors/codes.go`
    /// 全表与 `x/tools/.../errorcode.go` 都没有这个名字，也不存在 `UndefinedIdent`）⇒
    /// 按「代号照官方」的纪律合并成一条，块级/包级只差**消息措辞**（消息本来就自有渲染）。
    /// 留存差异：官方同时输出一条 `other declaration of %s` 的**附加行**，我方只报一条（`open_gaps`）。
    TypDuplicateDecl,
    /// **标签**重复声明 —— 官方 `DuplicateLabel`（消息 `label %s already declared`）
    ///
    /// 位置取**后声明**的那个标签（官方 `labels.go:137`：`err.addf(lbl, …)`，
    /// 而 `lbl` 是新建的、即后一个）。官方是 `soft` 错（不抑制后续诊断）。
    /// 标签**不在值作用域里** —— 它有自己的空间，且**每个函数体一个**
    /// （官方 `labels.go:17` 的 `NewScope(…, "label")`，只在 `funcBody` 里调一次）
    /// ⇒ 跨函数不可见、`goto` 不能跨函数。
    /// 留存差异：官方同时输出一条 `other declaration of %s` 附加行（同
    /// `TypDuplicateDecl`，`open_gaps`）。
    TypDuplicateLabel,
    /// 使用了**比该文件生效版本新**的语言特性 —— 官方 `UnsupportedFeature`
    /// （消息 `"%s requires %s or later"`，`go/types/errors.go:253`）
    ///
    /// 「生效版本」按**文件**算（`-lang` 或 `//go:build go1.N` 抬高后的 `max(v, go1.21)`），
    /// 见 [`crate::version`]。
    TypFeatureRequiresGoVersion,
    /// 文件的 `//go:build go1.N` 比当前工具链**新** —— 官方 `TooNew`
    ///
    /// 位置取该文件的 `package` **名字**（官方 `check.go:346` 的原话是
    /// 「Use position of 'package [p]'」，为与 types/types2 对齐）。
    TypFileRequiresNewerGo,
    /// **import 环**（`a → b → a`）—— **官方没有对应代号**
    ///
    /// 官方 `go/types` **不做**源码 import：它把这件事交给 `Config.Importer`，
    /// 而环检测在 importer 一侧且只报纯文本（`go/internal/srcimporter/srcimporter.go:85`
    /// 的 `import cycle through package %q`）。所以这条是**我方命名**，消息照官方原话。
    ///
    /// 口径（`M3-design.md` §2 的 D-M3-5 注）：**环上每个包一条**，位置取
    /// 「把这个包拉进环的那条 `import`」；入口包不是被 import 进来的 ⇒ 借用
    /// **闭合边**（发现环的那条 `import`）的位置。触发者 `T-TY-63`。
    TypImportCycle,
    /// 空标识符 `_` 被当**值或类型**用 —— 官方 `InvalidBlank`（消息 `cannot use _ as value or type`）
    ///
    /// 官方唯一触发点：`go/types/typexpr.go:28`（`check.error(e, InvalidBlank, …)`）。
    /// 位置取那个 `_`。
    /// ⚠️ **不是**「未定义」：`_` 从不绑定（官方 `check.declare` 第一句就返回），
    /// 所以任何「查作用域查不到」的通用路径都会把它误报成 `undefined: _`。
    /// 赋值 / 短变量声明 / `range` 的**左值位**是官方唯一容许 `_` 的地方
    /// （官方在 `assignVar` / `shortVarDecl` 里**先**特判 `_`），所以这个判据
    /// 不能只看「出现在表达式里」，要看**出现在哪**。
    TypInvalidBlank,
    /// `const` 的初始化式**不是常量**（在 M3 的常量子集口径下）—— 官方 `InvalidConstInit`
    ///
    /// 官方触发点在 `assignments.go:130`（`"%s is not constant"`），即
    /// 「求值结果没有值」的场合。⚠️ M3 把它的语义**撑宽**（`D-M3-19`）：
    /// 需要类型信息或源码外常量的初始化式（`pkg.C`、`len(s)`、`f()`）
    /// 一律走这条码 —— 这是 M3 的**稳定拒绝码**（`T-TY-83`）。
    TypInvalidConstInit,
    /// 常量**没有值**（`Const::Unknown`）却要参与转换 —— 官方 `InvalidConstVal`
    ///
    /// 与 `TypInvalidConversion` 的区别是**要害**：这条说的是「本来就没有值」
    /// （畸形字面量、除零、折叠失败），那条说的是「有值但转不过去」。
    /// 混成一个代号会把「上游出了内部不一致」伪装成「用户写错了」。
    TypInvalidConstVal,
    /// 常量转换不合法 —— 官方 `InvalidConversion`
    ///
    /// 覆盖官方的三种文案：`cannot convert … to type T` /
    /// `… truncated to T`（转换语境下官方实测也给这条，见
    /// `go/types/conversions.go:52-93`）/ `constant … overflows T` 的转换形态。
    TypInvalidConversion,
    /// **声明之间**的循环（`const A = B; const B = A`）—— 官方 `InvalidDeclCycle`
    ///
    /// 官方消息形状：`invalid cycle in declaration of %s`（`go/types/decl.go:286-290`）。
    /// ⚠️ 与 `TypInvalidRecursiveType` **分工**：后者是**类型图**的环
    /// （`valid_type`，Task 22，匿名构造之间），这条是**声明**的环（名字之间）。
    /// 两条判据不同：`type A *A` 是合法的（它连 `TypInvalidRecursiveType` 都不报），
    /// 而 `const A = A` 在这里报。
    TypInvalidDeclCycle,
    /// **初始化**之间的循环（`const A = B; const B = A`）—— 官方 `InvalidInitCycle`
    ///
    /// 官方消息形状：`initialization cycle for %s`（`go/types/initorder.go:178`），
    /// 位置取环里**源码序最前**的那个对象。与 `TypInvalidDeclCycle` 的分工照官方：
    /// 「环里**全是值**」⇒ 这条（官方由 initorder 报）；「环里只有别名、没有定义型」
    /// ⇒ `TypInvalidDeclCycle`（官方由 `decl.go` 的 `cycle()` 报）。
    TypInvalidInitCycle,
    /// 类型图里存在**非法的递归**（`type A [1]A`）—— 官方 `InvalidRecursiveType`
    ///
    /// 判据是「内存里装得下」：环只要**经过**指针/切片/映射/通道/函数就算合法
    /// （`type A *A` 是合法的），只有纯由「按值包含」的类型
    /// （数组元素 / 结构体字段 / 并集项 / 接口项集 / 具名类型的本体）串成的环才非法。
    /// 对齐 `go/types/validtype.go`（`F9`）。
    TypInvalidRecursiveType,
    /// 移位计数非法（负 / 非 uint / 超上界）—— 官方 `InvalidShiftCount`
    TypInvalidShiftCount,
    /// 移位左操作数不是整数 —— 官方 `InvalidShiftOperand`
    TypInvalidShiftOperand,
    /// 前向 `goto` 跳进了**嵌套块内部**的标签 —— 官方 `JumpIntoBlock`
    /// （消息 `goto %s jumps into block`）
    ///
    /// 位置取 `goto` 的**标签标识符**处（官方 `labels.go:40` 的 `check.errorf(jmp.Label, …)`）。
    /// 与 `TypUndeclaredLabel` 的分工靠**整函数体的标签全集**：收尾时前向跳转仍未匹配，
    /// 若全集里有同名标签 ⇒ 本条（跳进了块）；没有 ⇒ 那条（根本没声明）。
    /// 官方同时把那个标签标成「已使用」（免得再报一条 `UnusedLabel`）。
    TypJumpIntoBlock,
    /// 同包多文件里出现了**不一致的包名** —— 官方 `MismatchedPkgName`
    ///
    /// 官方 `go/types/check.go:307`（`check.errorf(atPos(file.Package), MismatchedPkgName, …)`）：
    /// 位置取 **`package` 关键字**处（不是包名标识符处 —— 那是 `BlankPkgName` 的位置），
    /// 消息 `package %s; expected package %s`。
    /// 包名以**第一个**文件为准（官方 `initFiles` 的 `case "": pkg.name = name`），
    /// 不一致的文件被**整个忽略**（不进 `pkg.files`）—— 对齐官方那句 `// ignore this file`。
    TypMismatchedPkgName,
    /// `break`/`continue` 的标签**不是**它外层那个可跳转语句 —— 官方 `MisplacedLabel`
    /// （消息 `invalid break label %s` / `invalid continue label %s`）
    ///
    /// 位置取 `break`/`continue` 的**标签标识符**处（官方 `labels.go:193`/`208`
    /// 的 `check.errorf(s.Label, …)`）。
    /// 两个触发面（官方 `enclosingTarget` 返回 `nil` 的两种情况都算）：
    /// ① 名字在本函数体里根本没有；② 有，但它标的不是 `for`/`range`/`switch`/`select`
    /// （`continue` 只认 `for`/`range`）。
    /// ⚠️ 与 `TypUndeclaredLabel` 分开是官方的选择：`break L` 名字不存在时官方**不**说
    /// 「未声明」，而说「break 的标签位置不对」。
    /// 官方此处 `return`（**不**把标签标成已使用）⇒ 一个只被非法 `break` 引用的标签
    /// 还会再报一条 `UnusedLabel`（实测 `s3.go` 两条）。
    TypMisplacedLabel,
    /// 类型**嵌套**超过本仓上限（**`H` 判据：官方无此行为**）
    ///
    /// 与 `SynNestingTooDeep` 是同一个理由：官方编译器靠 Go 运行时的可增长栈，
    /// 我们落在固定的宿主栈上，必须自设上限防栈溢出。上限 1000
    /// （`rgoc-types` 的 `MAX_TYPE_NEST_DEPTH`），每次判定只报一次。
    ///
    /// ⚠️ 它**不是**「类型写法非法」：1001 层的 `[1][1]…[1]int` 在 Go 里是合法类型，
    /// 报出来是**我方能力边界**。所以它**不能**复用 `TypInvalidRecursiveType`
    /// （那会说用户写错了）——两者必须分开，有测试钉住。
    TypNestingTooDeep,
    /// 常量**放不进**目标类型 —— 官方 `NumericOverflow`
    ///
    /// 与 `TypTruncatedFloat` 的分界是**有没有丢信息**：
    /// `int8(200)` 是「值太大」（这条），`int(1.5)` 是「值不是整数」（那条）。
    /// 官方 `representation()` 也是这么分的（`go/types/const.go` 尾部）。
    TypNumericOverflow,
    /// 包级 `-lang` 比当前工具链**新** —— 官方 `TooNew`
    ///
    /// 位置取 **`files[0]`**（官方 `check.go:315-317`：`check.errorf(files[0], TooNew, …)`，
    /// 而 `ast.File.Pos()` 是 `package` **关键字**的位置）。
    TypPackageRequiresNewerGo,
    /// 短变量声明的**同一个名字在左侧出现两次** —— 官方 `RepeatedDecl`
    /// （消息 `%s repeated on left side of :=`，`go/types/assignments.go:552`）
    ///
    /// 与 `TypDuplicateDecl` 的分工是官方的分工：这条只看**一条 `:=` 的左值列表内**
    /// （`x, y, y := …`），与作用域里本来有什么无关。
    TypRepeatedDecl,
    /// 常量向目标类型量化时被截断（`1.5` 赋给 `int`）—— 官方 `TruncatedFloat`
    ///
    /// ⚠️ 这条**不在转换语境**生效（那里是 `TypInvalidConversion`），
    /// 它服务**赋值/声明**语境：`const x int = 1.5` ⇒ 「1.5 truncated to integer」。
    TypTruncatedFloat,
    /// `goto` 的标签在整个函数体里**找不到** —— 官方 `UndeclaredLabel`
    /// （消息 `label %s not declared`）
    ///
    /// 位置取 `goto` 的**标签标识符**处（官方 `labels.go:38`：`check.errorf(jmp.Label, …)`）。
    /// 判据是**收尾**时的：前向跳转在整个体里转完仍未匹配 ⇒ 要么没声明（本条）、
    /// 要么跳进了嵌套块（`TypJumpIntoBlock`）。
    /// ⚠️ 官方消息是 `label %s not declared`（**not declared**），不是 `undefined: %s` ——
    /// 尽管它在同一族里，字面不同就是不同诊断。
    TypUndeclaredLabel,
    /// 引用了**没有声明**的名字 —— 官方 `UndeclaredName`（消息 `undefined: %s`）
    ///
    /// 官方两个来源：**类型位置** `go/types/typexpr.go:30`（`check.errorf(e, UndeclaredName, "undefined: %s", e.Name)`）、
    /// **值位置**由解析器收尾统一报（同一个代号）。另有一条 `UndeclaredImportedName`
    /// 专管 `pkg.Sel`（消息 `undefined: pkg.Sel`）—— 那条**归 Task 27**（源码 import）。
    /// ⚠️ `M3-plan.md` Task 25 写的 `UndefinedIdent` 在官方**不存在** ⇒ 用官方名。
    /// 位置取**该标识符**处；每次**出现**都报（实测 `y = 1; _ = y` 报两条）。
    TypUndeclaredName,
    /// 请求了**约束接口的同一性**（含项集 `~T`/并集，或 `comparable`）——
    /// **官方没有对应代号**：`go/types` 有完整类型集，判得了；M3 只表示不判定
    /// （`D-M3-13`，`U-M3-05`）
    ///
    /// 它**刻意偏离**上面几条「与官方代号一一对应」的纪律：那几条是「官方也报，
    /// 我们按同一类别报」；这条是「官方**不**报（它判得出来），我们明确说
    /// 「M3 不判」」。混用官方 `Code` 会把「我方能力边界」伪装成「用户写错了」。
    ///
    /// 为什么不静默返回 `false`：**「不判」与「不等」是两件事**。静默 `false`
    /// 会在上游造出一个「类型不匹配」的假诊断，把一条能力边界变成一条假报错
    /// （`M3-tests.md` §4：`T-TY-81` 要的是**稳定拒绝码**）。
    ///
    /// 触发者：`T-TY-81`（`identical` 的约束接口路径）。
    /// `T-TY-82`（`TYP-13/18/19/23..26` 的判定请求）在 M3 的**唯一入口**是
    /// `//rgoc:cmp` 的 kind 名 ⇒ 由驱动层用 `TYP-CMP-KIND-UNSUPPORTED` 拒绝
    /// （`rgoc-driver/src/cmp_cmd.rs` 的 `KIND_UNSUPPORTED`）—— 那条是「**请求**层」
    /// 的边界，与本码（**语言层**边界）不同层。本码留给 M4：等真检查器开始问
    /// 「约束接口同一性」时复用。参见 `M3-tests.md` §4 的 `T-TY-82` 行。
    TypUnsupportedConstraintIdentity,
    /// 用了 **M3 不支持的 import 形式** —— **官方没有对应代号**
    ///
    /// 触发面（`D-M3-5`）：M3 的 import 只认「在调用方给的 roots 下能找到**源码目录**」
    /// 这一种形式 ⇒ 二进制 export data（标准库那种只有归档的包）、模块路径
    /// （`example.com/x`，要 `go.mod`/下载）、相对路径全部落到这条。
    ///
    /// 为什么**不**复用 [`Self::TypBrokenImport`]：那条的语义是「我们要装的包装不上」，
    /// 而这里是「**我们不支持这种 import**」——`D-M3-11` 要的是**稳定拒绝码**，
    /// 混进官方的「用户写错了」族会把能力边界伪装成用户的错（与
    /// [`Self::TypUnsupportedConstraintIdentity`] 同一个理由）。
    /// ⚠️ 计划稿点名 `UnsupportedImportForm`，官方无此名 ⇒ 我方命名（`D-M3-11` 允许）。
    TypUnsupportedImportForm,
    /// 标签**声明了却没被用** —— 官方 `UnusedLabel`（消息 `label %s declared and not used`）
    ///
    /// 位置取**标签声明**处（官方 `labels.go:51` 的 `check.softErrorf(lbl, …)`）。
    /// 官方是 `soft` 错。判据是「整个函数体的分支语句里没有任何一处引用它」——
    /// 包括**被非法 `break`/`continue` 引用也不算**（官方在那两处 `return`，
    /// 见 `TypMisplacedLabel`）。
    /// 真实语料里不会假阳：`gc` 对同一条也是**硬错**（「label L defined and not used」），
    /// 所以能编译过的代码必然没有未用标签。
    TypUnusedLabel,
    /// 声明里**名字与初始化式的个数对不上** —— 官方 `WrongAssignCount`
    ///
    /// 官方 `go/types/resolver.go:55-90` 的 `arityMatch`：`const a, b = 1`
    /// ⇒ `missing init expr for b`（位置取**那个名字**）；`const a = 1, 2`
    /// ⇒ `extra init expr 2`（位置取**多出来的那个表达式**）。
    /// ⚠️ `var` 一侧（同一代号）M3 **不报** —— 它要求求值 var 的初始化式，
    /// 而 M3 不求值（`open_gaps`）；本任务只在 **`const`** 的「名字 ↔ 值」配对处
    /// 报它（那里本来就要配对，不报就会**静默**丢名字）。
    TypWrongAssignCount,
}

impl DiagCode {
    /// 全部代号，**按声明顺序**（即字典序）。
    ///
    /// 供遍历式断言使用 —— 让「漏了一个代号」这件事有地方可查，
    /// 而不是靠人工维护一份可能过期的清单。
    pub const ALL: [DiagCode; 66] = [
        DiagCode::BadChar,
        DiagCode::BadDigit,
        DiagCode::BadExponent,
        DiagCode::BadIdentChar,
        DiagCode::BadRadixPoint,
        DiagCode::BadSep,
        DiagCode::BomMidFile,
        DiagCode::CommentUnterminated,
        DiagCode::EscapeBadDigit,
        DiagCode::EscapeOctalOver,
        DiagCode::EscapeRange,
        DiagCode::EscapeUnknown,
        DiagCode::ExceedsFileLimit,
        DiagCode::IdLeadingDigit,
        DiagCode::InvalidNul,
        DiagCode::InvalidUtf8,
        DiagCode::LitNoDigits,
        DiagCode::RuneEmpty,
        DiagCode::RuneMulti,
        DiagCode::RuneNewline,
        DiagCode::RuneUnterminated,
        DiagCode::StringNewline,
        DiagCode::StringUnterminated,
        DiagCode::SynBranchBreak,
        DiagCode::SynBranchContinue,
        DiagCode::SynBranchFallthrough,
        DiagCode::SynExpectedSemicolon,
        DiagCode::SynGotoIntoBlock,
        DiagCode::SynGotoJumpsOverDecl,
        DiagCode::SynInvalidLineDirective,
        DiagCode::SynLabelDuplicate,
        DiagCode::SynLabelUndefined,
        DiagCode::SynLabelUnused,
        DiagCode::SynNestingTooDeep,
        DiagCode::SynUnexpectedToken,
        DiagCode::TypBlankPkgName,
        DiagCode::TypBrokenImport,
        DiagCode::TypDivByZero,
        DiagCode::TypDuplicateDecl,
        DiagCode::TypDuplicateLabel,
        DiagCode::TypFeatureRequiresGoVersion,
        DiagCode::TypFileRequiresNewerGo,
        DiagCode::TypImportCycle,
        DiagCode::TypInvalidBlank,
        DiagCode::TypInvalidConstInit,
        DiagCode::TypInvalidConstVal,
        DiagCode::TypInvalidConversion,
        DiagCode::TypInvalidDeclCycle,
        DiagCode::TypInvalidInitCycle,
        DiagCode::TypInvalidRecursiveType,
        DiagCode::TypInvalidShiftCount,
        DiagCode::TypInvalidShiftOperand,
        DiagCode::TypJumpIntoBlock,
        DiagCode::TypMismatchedPkgName,
        DiagCode::TypMisplacedLabel,
        DiagCode::TypNestingTooDeep,
        DiagCode::TypNumericOverflow,
        DiagCode::TypPackageRequiresNewerGo,
        DiagCode::TypRepeatedDecl,
        DiagCode::TypTruncatedFloat,
        DiagCode::TypUndeclaredLabel,
        DiagCode::TypUndeclaredName,
        DiagCode::TypUnsupportedConstraintIdentity,
        DiagCode::TypUnsupportedImportForm,
        DiagCode::TypUnusedLabel,
        DiagCode::TypWrongAssignCount,
    ];

    /// 机器可读代号文本。**它是契约的一部分**，改动等于契约变更。
    pub fn as_str(self) -> &'static str {
        match self {
            DiagCode::BadChar => "LEX-BAD-CHAR",
            DiagCode::BadDigit => "LEX-BAD-DIGIT",
            DiagCode::BadExponent => "LEX-BAD-EXPONENT",
            DiagCode::BadIdentChar => "LEX-BAD-IDENT-CHAR",
            DiagCode::BadRadixPoint => "LEX-BAD-RADIX-POINT",
            DiagCode::BadSep => "LEX-BAD-SEP",
            DiagCode::BomMidFile => "LEX-BOM-MIDFILE",
            DiagCode::CommentUnterminated => "LEX-COMMENT-UNTERMINATED",
            DiagCode::EscapeBadDigit => "LEX-ESCAPE-BAD-DIGIT",
            DiagCode::EscapeOctalOver => "LEX-ESCAPE-OCTAL-OVER",
            DiagCode::EscapeRange => "LEX-ESCAPE-RANGE",
            DiagCode::EscapeUnknown => "LEX-ESCAPE-UNKNOWN",
            DiagCode::ExceedsFileLimit => "LEX-EXCEEDS-FILE-LIMIT",
            DiagCode::IdLeadingDigit => "LEX-ID-LEADING-DIGIT",
            DiagCode::InvalidNul => "LEX-INVALID-NUL",
            DiagCode::InvalidUtf8 => "LEX-INVALID-UTF8",
            DiagCode::LitNoDigits => "LEX-LIT-NO-DIGITS",
            DiagCode::RuneEmpty => "LEX-RUNE-EMPTY",
            DiagCode::RuneMulti => "LEX-RUNE-MULTI",
            DiagCode::RuneNewline => "LEX-RUNE-NEWLINE",
            DiagCode::RuneUnterminated => "LEX-RUNE-UNTERMINATED",
            DiagCode::StringNewline => "LEX-STRING-NEWLINE",
            DiagCode::StringUnterminated => "LEX-STRING-UNTERMINATED",
            DiagCode::SynBranchBreak => "SYN-BRANCH-BREAK",
            DiagCode::SynBranchContinue => "SYN-BRANCH-CONTINUE",
            DiagCode::SynBranchFallthrough => "SYN-BRANCH-FALLTHROUGH",
            DiagCode::SynExpectedSemicolon => "SYN-EXPECTED-SEMICOLON",
            DiagCode::SynGotoIntoBlock => "SYN-GOTO-INTO-BLOCK",
            DiagCode::SynGotoJumpsOverDecl => "SYN-GOTO-JUMPS-OVER-DECL",
            DiagCode::SynInvalidLineDirective => "SYN-INVALID-LINE-DIRECTIVE",
            DiagCode::SynLabelDuplicate => "SYN-LABEL-DUPLICATE",
            DiagCode::SynLabelUndefined => "SYN-LABEL-UNDEFINED",
            DiagCode::SynLabelUnused => "SYN-LABEL-UNUSED",
            DiagCode::SynNestingTooDeep => "SYN-NESTING-TOO-DEEP",
            DiagCode::SynUnexpectedToken => "SYN-UNEXPECTED-TOKEN",
            DiagCode::TypBlankPkgName => "TYP-BLANK-PKG-NAME",
            DiagCode::TypBrokenImport => "TYP-BROKEN-IMPORT",
            DiagCode::TypDivByZero => "TYP-DIV-BY-ZERO",
            DiagCode::TypDuplicateDecl => "TYP-DUPLICATE-DECL",
            DiagCode::TypDuplicateLabel => "TYP-DUPLICATE-LABEL",
            DiagCode::TypFeatureRequiresGoVersion => "TYP-FEATURE-REQUIRES-GO-VERSION",
            DiagCode::TypFileRequiresNewerGo => "TYP-FILE-REQUIRES-NEWER-GO",
            DiagCode::TypImportCycle => "TYP-IMPORT-CYCLE",
            DiagCode::TypInvalidBlank => "TYP-INVALID-BLANK",
            DiagCode::TypInvalidConstInit => "TYP-INVALID-CONST-INIT",
            DiagCode::TypInvalidConstVal => "TYP-INVALID-CONST-VAL",
            DiagCode::TypInvalidConversion => "TYP-INVALID-CONVERSION",
            DiagCode::TypInvalidDeclCycle => "TYP-INVALID-DECL-CYCLE",
            DiagCode::TypInvalidInitCycle => "TYP-INVALID-INIT-CYCLE",
            DiagCode::TypInvalidRecursiveType => "TYP-INVALID-RECURSIVE-TYPE",
            DiagCode::TypInvalidShiftCount => "TYP-INVALID-SHIFT-COUNT",
            DiagCode::TypInvalidShiftOperand => "TYP-INVALID-SHIFT-OPERAND",
            DiagCode::TypJumpIntoBlock => "TYP-JUMP-INTO-BLOCK",
            DiagCode::TypMismatchedPkgName => "TYP-MISMATCHED-PKG-NAME",
            DiagCode::TypMisplacedLabel => "TYP-MISPLACED-LABEL",
            DiagCode::TypNestingTooDeep => "TYP-NESTING-TOO-DEEP",
            DiagCode::TypNumericOverflow => "TYP-NUMERIC-OVERFLOW",
            DiagCode::TypPackageRequiresNewerGo => "TYP-PACKAGE-REQUIRES-NEWER-GO",
            DiagCode::TypRepeatedDecl => "TYP-REPEATED-DECL",
            DiagCode::TypTruncatedFloat => "TYP-TRUNCATED-FLOAT",
            DiagCode::TypUndeclaredLabel => "TYP-UNDECLARED-LABEL",
            DiagCode::TypUndeclaredName => "TYP-UNDECLARED-NAME",
            DiagCode::TypUnsupportedConstraintIdentity => "TYP-UNSUPPORTED-CONSTRAINT-IDENTITY",
            DiagCode::TypUnsupportedImportForm => "TYP-UNSUPPORTED-IMPORT-FORM",
            DiagCode::TypUnusedLabel => "TYP-UNUSED-LABEL",
            DiagCode::TypWrongAssignCount => "TYP-WRONG-ASSIGN-COUNT",
        }
    }
}

/// 一条诊断。
///
/// `message` **不参与排序、不参与比对** —— 只给人看（见模块文档）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diag {
    /// 出错位置
    pub pos: Pos,
    /// 机器可读代号（参与排序）
    pub code: DiagCode,
    /// 人类可读描述（不参与排序）
    pub message: String,
}

/// 诊断集合。
///
/// 取用**只有一条路**：[`DiagBag::into_sorted`]，且它**消费 `self`**。
///
/// 之所以消费而不是返回 `&[Diag]`：若返回引用，调用方可能拿它去渲染而**忘记先排序** ——
/// 那就绕过了这套类型存在的唯一理由（`C1` §4.3）。消费 `self` 让排序成为
/// **类型上不可绕过**的一步。
#[derive(Default, Debug)]
pub struct DiagBag {
    items: Vec<Diag>,
}

impl DiagBag {
    /// 建一个空集合。
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一条诊断（无序）。
    pub fn push(&mut self, d: Diag) {
        self.items.push(d);
    }

    /// 已收条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 消费并返回**排好序**的诊断。
    ///
    /// 排序键（`C1` §4，按优先级）：`pos.file` → `pos.line` → `pos.col` → `code`。
    /// 前三个由 [`Pos`] 的 `Ord` 提供；第 4 个在这里补上。
    ///
    /// 第 4 个键不是可有可无的洁癖：**同一位置的多条诊断**若顺序取决于插入/哈希顺序，
    /// 输出就不确定，而 `E6` 要求重复执行结果一致 —— 同位置两条诊断的顺序
    /// 正是最容易泄漏随机性的地方。
    pub fn into_sorted(self) -> Vec<Diag> {
        let mut v = self.items;
        v.sort_by(|a, b| a.pos.cmp(&b.pos).then_with(|| a.code.cmp(&b.code)));
        v
    }
}
