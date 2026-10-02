# Go 编译器功能点测试用例清单

> **基准**：[`01-feature-set.md`](./01-feature-set.md) 中的 246 个功能点
> **检索范围**：`go_source_code/` 全树（go1.27.1）
> **检索方式**：逐功能点 Grep/Read 实名核对，仅登记实际存在的文件与测试函数
> **说明**：所有路径相对 `go_source_code/`；`spec`/`nodes` 引用含义见 [`01-feature-set.md`](./01-feature-set.md) §0.4  
> **文档索引**：[`README.md`](./README.md)　|　**上游**：[`01-feature-set.md`](./01-feature-set.md)　|　**下游**：[`03-roadmap.md`](./03-roadmap.md)

---

## 0. 测试资源与驱动方式

### 0.1 五类测试资源

| 类型 | 位置 | 形态 | 驱动方式 |
|---|---|---|---|
| A. 语言测试套件 | `test/`（顶层 ~600 个 .go + 子目录） | 独立可编译程序，**不是** `_test.go` | `test/run.go` 按文件首行指令自动发现 |
| B. 编译器单测 | `src/cmd/compile/internal/*/*_test.go` | 标准 Go 单测 | `go test` |
| C. 类型检查黄金语料 | `src/internal/types/testdata/{check,spec,examples,fixedbugs}/` | 带 `// ERROR` 注释的 .go 语料 | `go/types` 与 `types2` 的 `TestCheck/TestSpec/TestExamples/TestFixedbugs/TestLocal` |
| D. Runtime 单测 | `src/runtime/*_test.go` + `src/runtime/testdata/` | 标准单测 + 子进程程序 | `go test`；testdata 程序由 `TestMain` 分派 |
| E. 编译器内部导出 | `src/*/export_test.go` | 暴露内部符号供测试 | 被上述单测引用 |

**A 类文件的驱动指令**（首行注释）：`// run`、`// errorcheck`（含 `-0 -m` 等标志）、`// compile`、`// runoutput`、`// rundir`、`// errorcheckdir`、`// asmcheck`、`// build`。故其"测试函数"是文件内的 `main()` 或具名辅助函数，而非 `TestXxx`。

### 0.2 与既有文档不一致之处（已核实修正）

| 既有表述 | 实际 |
|---|---|
| `syntax/testdata/` 29 个文件 | **31 个**（多出 `typeset.go`、`issue70974.go` 等） |
| `test/escape*.go` 约 40 个 | **33 个** |
| `ssa/check_test.go`、`ssa/opt_test.go`、`ssa/html_test.go` | **均不存在**（dump 相关在 `ir/html_test.go`、`ir/dump_test.go`） |
| `escape/`、`walk/`、`coverage/`、`pkginit/`、`objw/`、`staticdata/`、`arm64/` 有测试 | 这些目录下**无任何 `*_test.go`** |
| `src/runtime/internal/` | **不存在**；SwissTable 测试在 `src/internal/runtime/maps/` |
| `sizeclasses_test.go`、`mheap_test.go`、`mcache_test.go`、`mcentral_test.go`、`cgo_test.go` | **均不存在** |

---

## 1. 词法要素 LEX（13/13 已覆盖）

| ID | 功能 | 测试文件 | 覆盖的测试函数 / 用例 |
|---|---|---|---|
| LEX-01 | UTF-8 源码表示（含 BOM） | `test/bom.go`；`src/go/scanner/scanner_test.go`；`test/crlf.go` | `// runoutput`；`TestScanErrors`（BOM 错误表）、`TestUTF16`、`TestScannerEnd`(BOM 子用例)、`TestSemicolons`（首 BOM 忽略）；`// runoutput`（\r/\r\n） |
| LEX-02 | Unicode 字母/数字分类 | `src/cmd/compile/internal/syntax/scanner_test.go`；`src/go/scanner/scanner_test.go`；`test/utf.go` | `TestTokens`（`sampleTokens` 含 `a۰۱۸`/`foo६४`/`bar９８７６`/`ŝ`）；`TestScan`（同）；`// run` |
| LEX-03 | 行/块注释 | `src/cmd/compile/internal/syntax/scanner_test.go`；`src/go/ast/commentmap_test.go` | `TestComments`（`//`、`/* */`、`/**/`、未终止）；`TestCommentMap`、`TestFilter`、`TestCommentText`、`TestIsDirective` |
| LEX-04 | Token 分类 | `src/cmd/compile/internal/syntax/scanner_test.go`；`src/go/token/token_test.go` | `TestSmoke`、`TestTokens`、`TestEmbeddedTokens`（分类 + precedence）；`TestIsIdentifier` |
| LEX-05 | 关键字 | `src/cmd/compile/internal/syntax/scanner_test.go`；`src/go/scanner/scanner_test.go` | `TestTokens`（25 个关键字逐个断言）；`TestScan` |
| LEX-06 | 操作符与标点（含 `~`） | 同上两处；`src/internal/types/testdata/check/unions.go` | `TestTokens`（含 `{_Operator,"~",Tilde,0}`）；`TestScan`（`token.TILDE`）；`TestCheck` |
| LEX-07 | 分号自动插入 | `src/go/scanner/scanner_test.go`；`test/syntax/semi1.go`…`semi7.go`；`test/eof.go`、`eof1.go` | `TestSemicolons`（`semicolonTests` 表）、`TestScanReuseSemiInNewlineComment`；7 个 `// errorcheck`；`// compile` |
| LEX-08 | 标识符词法 | `src/cmd/compile/internal/syntax/scanner_test.go`；`src/go/token/token_test.go` | `TestTokens`（name samples）；`TestIsIdentifier` |
| LEX-09 | 整数字面量 | `src/go/scanner/scanner_test.go`；`src/cmd/compile/internal/syntax/scanner_test.go`；`test/int_lit.go`；`src/internal/types/testdata/check/literals.go` | `TestNumbers`（binaries/octals/decimals/hexadecimals/separators 全表；含 `0b__1000` 错误）；`// run`；`TestCheck`（`0_123`/`0o`/`0b`/`0X_`） |
| LEX-10 | 浮点字面量（含 p 指数） | 同上两处 scanner_test；`test/float_lit.go`、`float_lit2.go`、`float_lit3.go` | `TestNumbers`（decimal/hex floats）；`// run` ×2；`// errorcheck` |
| LEX-11 | 虚数字面量 | scanner_test ×2；`test/cmplx.go`；`test/ken/cplx0.go`…`cplx5.go` | `TestNumbers`（IMAG/ImagLit）；`// run` |
| LEX-12 | Rune 字面量与转义 | scanner_test ×2；`test/char_lit.go`、`char_lit1.go`；`test/rune.go` | `TestTokens`（`'\000'`/`'\xFF'`/`'\uff16'`/`'\U0000ff16'`）、`TestScanErrors`；`// run`/`// errorcheck`/`// compile` |
| LEX-13 | 字符串字面量 | scanner_test ×2；`test/string_lit.go` | `TestScan`（反引号多行/`\r`）、`TestScanErrors`（未终止/NUL/非法 UTF-8）；`// run` |

---

## 2. 常量 CON（8/8 已覆盖）

| ID | 功能 | 测试文件 | 覆盖的测试函数 / 用例 |
|---|---|---|---|
| CON-01 | 无类型常量体系 | `test/const.go`；`src/go/constant/value_test.go`；`src/internal/types/testdata/check/const0.go`、`constdecl.go` | `// run`（`chuge=1<<100`）；`TestNumbers`、`TestOps`、`TestString`、`TestMakeFloat64`；`TestCheck` |
| CON-02 | 常量精度（≥256 位） | `src/go/types/check_test.go`、`src/cmd/compile/internal/types2/check_test.go`；`src/go/constant/value_test.go` | `TestLongConstants`（9999 位整数字面量）、`TestBitLen`、`TestFractions` |
| CON-03 | 隐式转换 / 默认类型 | `test/convlit.go`、`convlit1.go`；`test/named.go`、`named1.go`；`test/named.go` | `// errorcheck`；`// run`（`isArray/isBool/...` 检查默认类型） |
| CON-04 | 常量表示性 | `test/const1.go`、`const2.go`、`const5.go`、`const6.go`、`shift1.go`、`float_lit2/3.go`；`types2/check_test.go` | `// errorcheck`；`TestIndexRepresentability`、`TestIssue47243_TypedRHS` |
| CON-05 | 常量表达式折叠 | `src/cmd/compile/internal/test/constFold_test.go`（~130 个函数）；`src/go/types/eval_test.go`；`test/const3.go`、`prove_constant_folding.go` | `TestConstFolduint64add/sub/div/mul/mod`、`TestConstFoldint64*`、`TestConstFoldCompare*`、`TestConstFold*{lsh,rsh}`；`TestEvalArith`、`TestEvalBasic`、`TestCheckExpr` |
| CON-06 | iota 枚举 | `test/iota.go`；`test/const3.go`、`const8.go`；`src/internal/types/testdata/check/constdecl.go` | `// run`；`TestCheck`（`// iota` 组）、`TestFixedbugs` |
| CON-07 | 常量声明 | `test/const.go`、`const1/3/4/7/8.go`；`src/internal/types/testdata/check/constdecl.go` | `// run` / `// errorcheck`；`TestCheck`（`// constant declarations`） |
| CON-08 | 移位计数语义 | `test/shift1.go`、`shift2.go`、`shift3.go`；`src/cmd/compile/internal/test/shift_test.go`；`check/shifts.go` | `// errorcheck`/`// compile`/`// run`；`TestShiftOfZero`、`TestShiftByZero`、`TestShiftLargeCombine`、`TestShiftOverflow`、`TestNegShifts` |

---

## 3. 类型系统 TYP（25/26 已覆盖）

| ID | 功能 | 测试文件 | 覆盖的测试函数 / 用例 |
|---|---|---|---|
| TYP-01 | 类型命名/未命名分类 | `test/named.go`、`named1.go`；`src/go/types/typestring_test.go` | `// run`（Array/Bool/Chan/Float/Int/Map/Slice/String）；`TestTypeString`、`TestQualifiedTypeString` |
| TYP-02 | 布尔类型 | `test/codegen/bool.go`、`test/ken/simpbool.go`；`check/expr1.go`、`expr2.go` | `// run`；`TestCheck`（binary expressions / comparisons） |
| TYP-03 | 数值类型全集 | `src/internal/types/testdata/check/decls1.go`；`test/intcvt.go`、`cmplx.go`、`divmod.go`、`convert.go`…`convert5.go` | `TestCheck`；`// run` |
| TYP-04 | 字符串类型 | `test/string_lit.go`、`stringrange.go`、`gcstring.go`、`strcopy.go`；`test/codegen/strings.go`、`test/ken/string.go` | `// run` |
| TYP-05 | 数组类型 | `test/ken/array.go`、`simparray.go`、`simassign.go`；`test/alg.go`、`bigalg.go`；`src/cmd/compile/internal/test/testdata/array_test.go` | `// run`；`testSliceLenCap`、`testSliceGetElement`、`testSlicePanic1/2` |
| TYP-06 | 切片类型 | `test/slice3.go`、`slice3err.go`、`slicecap.go`、`makeslice.go`；`check/slices.go`；`test/codegen/slices.go` | `// runoutput`/`// errorcheck`/`// run`；`TestCheck` |
| TYP-07 | 结构体（字段/标签/嵌入） | `src/internal/types/testdata/spec/structLits.go`、`check/decls3.go`；`test/struct0.go`；`src/reflect/all_test.go` | `TestSpec`/`TestCheck`；`TestAnonymousFields`、`TestBigStruct`、`TestTagGet`、`TestStructArg` |
| TYP-08 | 指针类型 | `test/nilptr.go`…`nilptr5.go`；`test/indirect.go`、`indirect1.go`、`parentype.go` | `// run`/`// errorcheck`/`// compile` |
| TYP-09 | 函数类型/变参 | `test/ddd.go`、`ddd1.go`、`ddd2.go`+`ddd2.dir/`；`test/func.go`…`func8.go`；`src/go/types/builtins_test.go` | `// run`/`// errorcheck`/`// rundir`；`TestBuiltinSignatures`（`...T` 签名） |
| TYP-10 | 基本接口 | `test/interface/`（26 项）；`src/go/types/typeset_test.go` | `// run` 等；`TestTypeSetString`、`TestInvalidTypeSet` |
| TYP-11 | 通用接口/约束（union、`~T`） | `src/go/types/termlist_test.go`、`typeterm_test.go`、`typeset_test.go`；`examples/constraints.go`、`typesets.go`；`check/unions.go` | `TestTermlist{All,String,Norm,Union,Intersect,Equal,Includes,SupersetOf,SubsetOf}`；`TestTerm{String,Equal,Union,Intersection,Includes,SubsetOf,Disjoint}` |
| TYP-12 | 嵌入接口 | `test/interface/embed.go`、`embed1.go`+`.dir/`、`embed2.go`、`embed3.go`+`.dir/`；`src/go/types/object_test.go` | `// run`/`// rundir`/`// errorcheck`；`TestEmbeddedMethod` |
| TYP-13 | 接口实现判定 | `src/go/types/api_test.go`、`methodset_test.go`；`test/interface/explicit.go`、`fail.go`、`pointer.go` | `TestImplements`、`TestMissingMethodAlternative`、`TestNewMethodSet`、`TestIssue60634`；`// errorcheck` |
| TYP-14 | map 类型 | `test/map.go`、`map1.go`、`makemap.go`、`mapclear.go`、`bigmap.go`、`maplinear.go`；`check/map0.go`、`map1.go`；`test/codegen/maps.go` | `// run`/`// errorcheck`；`TestCheck` |
| TYP-15 | chan 类型 | `test/chancap.go`、`makechan.go`、`chanlinear.go`、`closedchan.go`；`test/chan/`（19 项）；`check/chans.go` | `// run`/`// errorcheck`；`TestCheck` |
| TYP-16 | 底层类型 | `src/go/types/named_test.go`、`typestring_test.go`；`check/decls4.go`、`spec/assignability.go` | `TestFiniteTypeExpansion`、`TestMethodOrdering`、`TestTypeString` |
| TYP-17 | 类型同一性 | `src/go/types/api_test.go`、`hash_test.go` | `TestIdentical`、`TestIdentical_issue15173`、`TestIdenticalUnions`、`TestIssue61737`、`TestHasher` |
| TYP-18 | 可赋值性 | `src/go/types/api_test.go`；`spec/assignability.go`、`spec/conversions.go`；`test/assign.go`、`assign1.go`、`cannotassign.go` | `TestAssignableTo`、`TestConvertibleTo`；`TestSpec`；`// errorcheck` |
| TYP-19 | 方法集 | `src/go/types/methodset_test.go`、`example_test.go`；`check/methodsets.go`、`spec/methods.go`；`test/method.go`…`method7.go` | `TestNewMethodSet`、`TestNewMethodSet_RecursiveGeneric`、`ExampleMethodSet`；`TestCheck` |
| TYP-20 | 类型定义 | `check/decls0.go`、`decls5.go`、`spec/receivers.go`；`test/named.go`、`test/typecheck.go`；`check/cycles0.go`…`cycles6.go` | `TestCheck`（含 `invalid recursive type`）；`// run` |
| TYP-21 | 类型别名 | `src/go/types/object_test.go`、`api_test.go`、`alias_test.go`；`spec/typeAliases1.8.go`、`1.22.go`、`1.23.go`；`test/alias.go`、`alias1.go`、`alias2.go`、`alias3.go` | `TestIsAlias`、`TestNewAlias_Issue65455`、`TestUnaliasTooSoonInCycle`、`TestIssue74181`；`TestSpec` |
| TYP-22 | 类型参数声明 | `test/typeparam/tparam1.go`、`smoketest.go`；`check/typeparams.go`；`examples/functions.go`、`types.go`、`methods.go`；`syntax/testdata/tparams.go` | `// errorcheck`（`T redeclared`）；`TestCheck`/`TestExamples`；`TestSyntaxErrors` |
| TYP-23 | 约束满足性 | `src/go/types/typeset_test.go`、`api_test.go`；`examples/constraints.go`、`spec/comparable.go`、`check/typeparams.go` | `TestTypeSetString`、`TestInvalidTypeSet`、`TestImplements`；`TestExamples`/`TestSpec` |
| TYP-24 | 类型实例化 | `src/go/types/instantiate_test.go`、`api_test.go`；`check/typeinst0.go`、`typeinst1.go`、`typeinstcycles.go`；`test/typeparam/smoketest.go` 等 | `TestInstantiateEquality`、`TestInstantiateNonEquality`、`TestMethodInstantiation`、`TestInstanceIdentity`、`TestInstanceInfo` |
| TYP-25 | 类型推断 | `check/typeinference.go`、`funcinference.go`、`examples/inference.go`、`inference2.go`；`src/go/types/api_test.go` | `TestCheck`、`TestExamples`；`TestTypesInfo`、`TestGenericMethodInfo` |
| TYP-26 | 类型统一算法 | — | ⚠️ **未找到专用测试**（无 `unify_test.go`）。仅由 TYP-25 的语料间接覆盖，建议新增 |

---

## 4. 作用域 SCP（9/10 已覆盖）

| ID | 功能 | 测试文件 | 覆盖的测试函数 / 用例 |
|---|---|---|---|
| SCP-01 | 块与作用域嵌套 | `src/go/types/scope2_test.go`；`api_test.go`；`resolver_test.go` | `TestScopeLookupParent`（`/*name=kind:line*/` 驱动逐位置解析）；`TestScopesInfo`、`TestDefsInfo`、`TestUsesInfo`；`TestResolveIdents` |
| SCP-02 | 声明作用域规则 | `check/decls0.go`、`decls1.go`、`decls2/`、`vardecl.go`、`init0.go`…`init2.go` | `TestCheck` |
| SCP-03 | 标签作用域 | `test/label.go`、`label1.go`、`ken/label.go`、`goto.go`；`check/labels.go`、`doubled_labels.go`、`gotos.go` | `// errorcheck`；`TestCheck` |
| SCP-04 | 空白标识符 `_` | `test/blank.go`、`blank1.go`；`check/blank.go`、`decls1.go`、`stmt0.go` | `// run`（`Test behavior of the blank identifier`）/`// errorcheck`；`TestCheck` |
| SCP-05 | 预声明标识符 | `test/rename.go`、`rename1.go`、`undef.go`；`src/go/types/builtins_test.go` | `// run`（预声明名可被重声明）/`// errorcheck`；`TestBuiltinSignatures`（遍历 `Universe.Names()`） |
| SCP-06 | 导出标识符 | `test/interface/private.go` + `private.dir/{prog.go,private1.go}` | ⚠️ 仅 `// errorcheckdir`（跨包未导出方法不可见）。**无直接断言 `Exported()` 的单元测试**，建议新增 |
| SCP-07 | 标识符唯一性/重声明 | `check/decls0.go`、`decls2/decls2b.go`、`stmt0.go`、`main0.go`、`typeparams.go`；`test/declbad.go`、`decl.go` | `TestCheck`（`no new variables`）；`// errorcheck`/`// run` |
| SCP-08 | 变量遮蔽 | `test/fixedbugs/issue22822.go`、`issue24547.go`、`issue4326.go`；`check/stmt0.go`；`examples/methods.go` | `// errorcheck`/`// run`/`// compiledir`；`TestCheck` |
| SCP-09 | for 迭代变量独立作用域 | `src/cmd/compile/internal/loopvar/loopvar_test.go` + `testdata/`；`test/closure*.go`；`test/range.go`…`range4.go` | `TestLoopVarGo1_21`（`-lang=go1.21 -d=loopvar={-1,0,1,2}`）；testdata：`for_esc_*.go`、`range_esc_*.go`、`opt-121/122.go` |
| SCP-10 | 控制语句 init 作用域 | `check/stmt0.go`；`src/go/parser/parser_test.go`；`test/syntax/if.go`、`else.go`、`typesw.go`、`initvar.go`；`test/typeswitch*.go` | `TestCheck`（`Test correct scope setup`）；`TestColonEqualsScope`、`TestVarScope`；`// errorcheck` |

---

## 5. 表达式 EXP（25/27 已覆盖）

| ID | 功能 | 测试文件 | 覆盖的测试函数 / 用例 |
|---|---|---|---|
| EXP-01 | 操作数 | `test/literal.go`、`literal2.go`、`const.go`；`check/literals.go`；scanner_test | `main()`/`assert()`/`equal()`；`TestNumbers`、`TestTokens` |
| EXP-02 | 限定标识符 pkg.Name | `test/import.go`、`import2.dir/`、`import4.dir/`；`check/importdecl0/`；`test/typeparam/importtest.go` | `main()`；`TestUsesInfo`、`TestPkgNameOf` |
| EXP-03 | 复合字面量 | `test/complit.go`、`complit1.go`、`complit2.go`；`test/syntax/composite.go`；`check/expr3.go`（`struct_literals/array_literals/slice_literals/map_literals`）、`compliterals.go`；`test/typeparam/struct.go` | `main()`/`itor()`/`teq()`；`TestCompositeLitTypes` |
| EXP-04 | 结构体字面量嵌套字段 key (1.27) | `src/internal/types/testdata/spec/structLits.go` | 顶层 var 块（含 `B{b:0,x:0,...}` 与 `cannot specify promoted field ... and enclosing embedded field` 用例） |
| EXP-05 | 函数字面量/闭包 | `test/closure.go`、`closure1.go`、`closure2.go`、`closure4.go`、`closure6.go`、`closure7.go`、`closure3.dir/`、`closure5.dir/`；`test/func.go`、`func5.go`；`test/escape_closure.go` | `main()`/`accum()`/`newfunc()`/`gocall()`；escape 检查函数 |
| EXP-06 | 主表达式 | `src/cmd/compile/internal/syntax/parser_test.go`、`nodes_test.go`；`test/syntax/topexpr.go` | `TestParse`、`TestVerify`、`TestStdLib`、`TestPos` |
| EXP-07 | 选择器与字段提升 | `test/method.go`；`test/interface/embed*.go`；`check/lookup1.go`、`lookup2.go`；`src/go/types/api_test.go`、`methodset_test.go`；`test/typeparam/genembed*.go` | `promotion()`/`main()`；`TestSelection`、`TestLookupFieldOrMethod*`、`TestNewMethodSet` |
| EXP-08 | 方法表达式 T.M | `test/method7.go`、`genmeth1.go`、`genmeth2.go`；`check/expr3.go`（`method_expressions()`） | `mExp()`/`nExp()`/`main()`；`TestGenericMethodInfo` |
| EXP-09 | 方法值 x.M | `test/method.go`、`method5.go`、`genmeth1.go`；`test/typeparam/boundmethod.go` | `val()`/`CheckI()`/`CheckF()`/`mVal()` |
| EXP-10 | 索引表达式 | `test/index.go`、`index0.go`、`index1.go`、`index2.go`；`test/typeparam/index.go`、`index2.go`；`check/expr3.go`（`indexes()`）；`test/checkbce.go`、`loopbce.go` | `testExpr()`/`forall()`/`Index[T]()`；`TestIndexRepresentability` |
| EXP-11 | 切片表达式 | `test/slice3.go`、`slice3err.go`、`slicecap.go`；`check/expr3.go`（`indexes()`） | `checkSlice()`/`checkString()`/`checkBytes()`/`notOK()` |
| EXP-12 | 类型断言 x.(T) | `test/interface/assertinline.go`、`fail.go`、`returntype.go`、`noeq.go`；`test/typeparam/dottype.go`；`check/expr3.go`（`type_asserts()`） | `assertptr/assertfunc/assertstruct/assertbig/assertslice/assertInter`（+`*2`/`*2ok`）；`f()` |
| EXP-13 | 调用 | `check/expr3.go`（`_calls()`）；`test/func.go`、`func5.go`、`method5.go` | `f1()`–`f9()`、`gocall()`、`call()` |
| EXP-14 | 变参传递 f(s...) | `test/ddd.go`、`ddd1.go`、`ddd2.dir/`；`test/syntax/ddd.go`；`check/expr3.go` | `sum()`/`sumA()`/`sumC()`/`intersum()`/`ln()`；`main()` |
| EXP-15 | 泛型实例化 | `test/typeparam/`（363 文件）；`src/go/types/instantiate_test.go`、`api_test.go` | `TestInstantiateEquality`、`TestInstantiateNonEquality`、`TestInstanceIdentity`、`TestInstanceInfo`；代表文件：`geninline.go`、`subdict.go`、`dictionaryCapture.go`、`smoketest.go`、`shape1.go` |
| EXP-16 | 运算符优先级 | — | ⚠️ **未找到专用测试**。间接：`printer_test.go:TestVerify`（Fprint 往返）、`test/fixedbugs/bug448.dir/`（接收运算符优先级），建议新增 |
| EXP-17 | 算术运算符 | `check/expr1.go`、`expr0.go`；`test/codegen/arithmetic.go`；`test/64bit.go`、`divmod.go`、`divide.go`、`zerodivide.go`；`test/ken/divconst.go`、`modconst.go` | 一元/二元运算组；`AddLargeConst`、`Mul_2`、`Pow2Muls`；`divzerouint*()` |
| EXP-18 | 整数溢出回绕 | `test/intcvt.go`、`convert.go`、`convert2.go`、`convert4.go`；`test/const2.go`、`shift1.go`；`check/conversions0.go` | `chki8/16/32/64()`、`chku8/16/32/64()`；`wantPanic()` |
| EXP-19 | 浮点 IEEE754 | `test/float_lit.go`、`float_lit2.go`、`floatcmp.go`、`cmplxdivide.go`；`test/codegen/floats.go` | `pow10()`/`close()`/`fromBits()`；`FusedAdd32()`、`Mul2()` |
| EXP-20 | 字符串连接 | `test/strcopy.go`、`string_lit.go`；`test/codegen/strings.go`；`test/typeparam/stringer.go` | `main()`；`CountRunes()`、`HasPrefix3/5/6/7()` |
| EXP-21 | 比较运算符（可比较性） | `test/cmp.go`、`cmp6.go`、`floatcmp.go`；`test/codegen/comparisons.go`；`test/interface/noeq.go`；`test/alg.go`、`bigalg.go`；`spec/comparable.go`、`comparisons.go` | `p1()`–`p4()`、`shouldPanic()`；`CompareString1-3`、`CompareArray1-6`、`CompareStruct1-4`；`arraycmptest()` |
| EXP-22 | 逻辑运算符短路 | — | ⚠️ **未找到专用测试**（`test/codegen/shortcircuit.go` 实为类型断言单跳转）。间接：`test/ken/simpbool.go`、`test/codegen/bool.go`（`phiAnd/phiOr`），建议新增 |
| EXP-23 | 地址运算符 & / * | `check/expr0.go`、`expr2.go`（`pointers()`、`structs()`）；`test/indirect.go`、`indirect1.go`；`test/ken/ptrvar.go`、`ptrfun.go` | `// errorcheck`；`crash()`/`nocrash()` |
| EXP-24 | 接收运算符 <- | `test/chan/select.go`、`nonblock.go`；`test/closedchan.go`；`check/expr0.go`、`expr2.go`（`channels()`） | `GetValue()`/`Send()`/`i32receiver()`；`XChan.Recv()`、`Recv2()`、`Nbrecv()` |
| EXP-25 | 转换 | `test/convert.go`…`convert5.go`、`convlit.go`、`intcvt.go`；`spec/conversions.go`；`src/go/types/api_test.go` | `typeof()`/`f()`/`g()`/`wantPanic()`；`TestConvertibleTo`、`TestAssignableTo` |
| EXP-26 | 常量表达式 | `test/const.go`、`const1.go`、`const3.go`…`const8.go`、`iota.go`；`test/codegen/constants.go`；`types2/check_test.go` | `ints()`/`floats()`/`truncate()`；`TestLongConstants`；`shifted16BitConstants()` |
| EXP-27 | 求值顺序 | `test/reorder.go`、`reorder2.go`、`simassign.go`、`initexp.go` | `p1()`–`p11()`、`check()`、`testit()`/`swap()`；`a1()`–`a8()`、`b1()`–`b8()` |

---

## 6. 语句 STM（21/21 已覆盖）

| ID | 功能 | 测试文件 | 覆盖的测试函数 / 用例 |
|---|---|---|---|
| STM-01 | 终止语句判定 | `test/return.go`；`check/stmt1.go`；`test/fixedbugs/issue49003.go` | 大量 `func _() int {}`（`missing return`）；含 fallthrough/panic/无限 for 判定 |
| STM-02 | 空语句 | `test/fixedbugs/bug136.go`；`syntax/testdata/fallthrough.go` | `main()`（`L: ;`）；`_()` |
| STM-03 | 标签语句 | `test/label.go`、`label1.go`；`check/labels.go`、`gotos.go`；`test/goto.go` | `f0()`–`f6()` |
| STM-04 | 表达式语句 | `check/stmt0.go`（`expression_statements(ch chan int)`） | `TestCheck` |
| STM-05 | 发送语句 ch<-v | `test/chan/sendstmt.go`、`nonblock.go`；`check/stmt0.go`（`sends()`） | `chanchan()`/`sendprec()`/`i32sender()` |
| STM-06 | IncDec | `check/stmt0.go`（`incdecs()`）；`test/for.go` | `TestCheck`；`main()` |
| STM-07 | 赋值语句（含元组） | `test/assign.go`、`assign1.go`、`simassign.go`、`initcomma.go`、`cannotassign.go`；`check/stmt0.go`（`assignments0/1/2`） | `main()`/`testit()`/`swap()`；`issue6487()` |
| STM-08 | 短变量声明 | `test/decl.go`、`declbad.go`、`varinit.go`；`check/stmt0.go`（`shortVarDecls1()`） | `// errorcheck`（`redeclared`/`no new`） |
| STM-09 | if 语句 | `test/if.go`；`test/syntax/if.go`、`else.go`；`check/stmt0.go`（`issue11667/11687`） | `// errorcheck` |
| STM-10 | 表达式 switch | `test/switch.go`、`switch2.go`…`switch7.go`；`check/stmt0.go`（`switches0/1/2`）；`test/codegen/switch.go` | `f0()`–`f8()`；含 fallthrough/default 位置用例 |
| STM-11 | 类型 switch | `test/typeswitch.go`、`typeswitch1.go`、`typeswitch2.go`、`typeswitch2b.go`、`typeswitch3.go`；`test/typeparam/typeswitch1.go`…`typeswitch7.go`；`check/stmt0.go`（`typeswitches/typeswitch0-3`） | `whatis()`/`assert()`/`noninterface()` |
| STM-12 | for 三形态 | `test/for.go`、`ken/for.go`；`check/stmt0.go`（`fors1()`）；`test/loopbce.go` | `main()` |
| STM-13 | for range | `test/range.go`、`range2.go`、`range3.go`（over int）、`range4.go`（over func）、`rangegen.go`、`stringrange.go`；`test/typeparam/maps.go`；`spec/range.go`、`range_int.go`；`check/stmt0.go`（`rangeloops1/2`） | `testchan()`/`testslice()`/`testmap()`/`testint1-5()`/`testfunc0-9()`/`gen()`；`issue65133/64471/66561/67027` |
| STM-14 | go 语句 | `test/chan/goroutines.go`；`check/stmt0.go`（`gos()`）；`test/func5.go` | `gocall()`/`caller()` |
| STM-15 | select 语句 | `test/chan/select.go`、`select2.go`…`select8.go`、`doubleselect.go`、`nonblock.go`、`zerosize.go`；`check/stmt0.go`（`selects()`）；`test/codegen/select.go` | `testPanic()`/`testBlock()`/`checkorder()`/`sender()`/`recver()` |
| STM-16 | return 语句 | `test/return.go`、`retjmp.go`+`retjmp.dir/`；`check/stmt0.go`（`returns0-3`） | 大量 `func _() int {...}` |
| STM-17 | break/continue | `check/stmt0.go`（`breaks()`、`continues()`）；`test/label.go`、`label1.go`；`test/chan/select5.go`；`test/fixedbugs/bug136.go` | `f1()`/`f2()`；`checkorder()` |
| STM-18 | goto 语句 | `test/goto.go`；`check/gotos.go`；`test/label1.go`；`test/escape_goto.go` | 大量 `func _()`（跳入块/跳过声明等） |
| STM-19 | fallthrough | `syntax/testdata/fallthrough.go`；`test/switch.go`、`switch4.go`、`convinline.go`、`return.go` | `_()`（`fallthrough statement out of place`、`cannot fallthrough final case`、`cannot fallthrough in type switch`） |
| STM-20 | defer 语句 | `test/defer.go`、`defererrcheck.go`、`deferfin.go`、`defernil.go`、`deferprint.go`（+`.out`）；`check/stmt0.go`（`defers()`）；`test/typeparam/recoverimp.go`；`test/abi/defer_recover_results.go` | `test1()`/`test1helper()`/`test2()`/`addDotDotDot()`；`f1()`–`f9()` |
| STM-21 | 块内声明语句 | `test/decl.go`；`check/decls0.go`、`decls1.go`、`decls2/`、`decls3.go`、`decls4.go`、`decls5.go`、`vardecl.go`；`test/syntax/initvar.go` | `main()`；`TestCheck` |

---

## 7. 内建函数 BIF（15/15 已覆盖）

| ID | 函数 | 测试文件 | 覆盖的测试函数 / 用例 |
|---|---|---|---|
| BIF-01 | `append` | `test/append.go`、`append1.go`；`check/builtins0.go`（`append1/2/3`）、`builtins1.go`；`test/codegen/append.go`、`append_freegc.go`；`test/typeparam/append.go` | `verify()`/`verifyStruct()`/`verifyInterface()`；`_Append()` |
| BIF-02 | `copy` | `test/copy.go`、`copy1.go`；`check/builtins0.go`（`copy1/2`）；`test/codegen/copy.go` | `doAllSlices()`/`verify8/16/32/64()`/`bad8/16/32/64()` |
| BIF-03 | `clear` (1.21) | `test/clear.go`、`mapclear.go`；`check/builtins0.go`（`clear1`）；`test/fixedbugs/issue70189/78410/59411/61127/77435.go` | `checkClearSlice()`/`checkClearMap()`/`checkcleared()`/`checkloopvars()` |
| BIF-04 | `close` | `test/closedchan.go`、`chan/nonblock.go`、`chan/select3.go`；`check/builtins0.go`（`close1/2`）；`test/typeparam/chans.go` | `XChan.Close()`/`shouldPanic()` |
| BIF-05 | `complex` | `test/cmplx.go`；`check/builtins0.go`（`complex1/2`）；`test/ken/cplx0.go`…`cplx5.go` | `F1()`/`F3()`/`main()` |
| BIF-06 | `real`/`imag` | `check/builtins0.go`（`real1/2`、`imag1/2`）；`test/cmplx.go`、`ken/cplx*.go` | `TestCheck` |
| BIF-07 | `delete` | `check/builtins0.go`（`delete1/2`）；`test/map.go`、`mapclear.go` | `testbasic()`/`testfloat()`/`testnan()` |
| BIF-08 | `len`/`cap` | `check/builtins0.go`（`len1/2/3`、`cap1/2/3`）；`test/chancap.go`、`slicecap.go` | `shouldPanic()`/`checkString()`/`checkBytes()`/`checkInts()`/`notOK()` |
| BIF-09 | `make` | `test/makemap.go`、`makechan.go`、`makeslice.go`；`check/builtins0.go`（`make1/2`）；`test/typeparam/builtins.go` | `testMakeInts()`/`testMakeBytes()`/`testMakeInAppendInts()`/`shouldPanic()` |
| BIF-10 | `max`/`min` (1.21) | `check/builtins0.go`（`max1/2`、`min1/2`）；`test/fixedbugs/issue60582/60982/60990/60991/79274.go`；`test/typeparam/min.go`、`issue48424.go` | `min[T Ordered]()`；`f(x,b)` |
| BIF-11 | `new` | `test/makenew.go`、`newexpr.go`；`check/builtins0.go`（`new1/2`）；`test/newinline.go` | `add1()`/`add2()`/`switchType()`（含大量 `can inline` 检查） |
| BIF-12 | `panic` | `check/builtins0.go`（`panic1/2`）；`test/recover.go`、`recover1.go`、`recover2.go`；`test/nil.go`；`test/interface/fail.go`；`test/convert4.go` | `shouldPanic()`/`shouldBlock()`；`test1()`–`test16()` |
| BIF-13 | `recover` | `test/recover.go`…`recover5.go`；`check/builtins0.go`（`recover1/2`）；`test/typeparam/recoverimp.go`；`test/abi/defer_recover_results.go` | `mustRecover()`/`mustNotRecover()`/`doubleRecover()`/`try()`/`varargs()` |
| BIF-14 | `print`/`println` | `test/print.go`、`printbig.go`（+`.out`）、`goprint.go`（+`.out`）、`deferprint.go`（+`.out`）；`check/builtins0.go`（`print1/2`、`println1/2`） | `main()`（对照 `.out` 期望输出） |
| BIF-15 | 内建引导机制 | `src/builtin/builtin.go`；`src/go/types/builtins_test.go`；`src/go/types/universe.go`（`DefPredeclaredTestFuncs`） | `TestBuiltinSignatures`（`var builtinCalls` 表，遍历 `Universe`/`Unsafe` 确保无遗漏） |

---

## 8. 包与初始化 PKG（7/8 已覆盖）

| ID | 功能 | 测试文件 | 覆盖的测试函数 / 用例 |
|---|---|---|---|
| PKG-01 | 包结构与源文件组织 | `src/go/types/api_test.go`（`TestFiles`）；`test/*.dir/`；`test/helloworld.go` | `TestFiles`、`TestCheck`；`// run` |
| PKG-02 | package 子句 | `check/main0.go`、`main1.go`、`decls0.go`；`test/syntax/topexpr.go` | `TestCheck`（`package main` 要求） |
| PKG-03 | import 声明 | `test/import.go`、`import1.go`、`import5.go`、`import6.go`、`import2.dir/`、`import4.dir/`；`test/syntax/import.go`；`check/importdecl0/`、`importdecl1/` | `f()`/`main()`；`// errorcheck`；`TestImplicitsInfo`、`TestPkgNameOf` |
| PKG-04 | 零值 | `test/nil.go`、`varinit.go`、`zerosize.go`；`test/interface/noeq.go` | ⚠️ **无同名专用测试**。间接：`arraytest()`/`chantest()`/`maptest()`/`slicetest()`（nil 零值行为）、`main()` |
| PKG-05 | 包初始化顺序 | `src/go/types/api_test.go`（`TestInitOrderInfo`、`TestMultiFileInitOrder`）；`test/init.go`、`init1.go`、`initialize.go`；`check/init0.go`…`init2.go` | `f1()`–`f6()`、`f7()`/`f8()`/`m8()`/`f10()`/`f12()`/`f15()` |
| PKG-06 | 初始化依赖循环检测 | `test/initloop.go` | `// errorcheck`（`a refers to b\n.*b refers to c\n.*c refers to a\|initialization loop`）。⚠️ types2 侧无对应语料，建议补充 |
| PKG-07 | 程序初始化与 main | `check/main0.go`、`main1.go`；`test/helloworld.go`、`linkmain.go`、`init.go`、`init1.go`、`noinit.go` | `init()`/`main()`/`gopherize()`；`// run`（对照 `.out`） |
| PKG-08 | 导出与可见性 | `test/interface/private.dir/`；`check/lookup2.go`；`src/go/types/resolver_test.go`、`object_test.go` | `// errorcheckdir`；`TestResolveIdents`；`TestIsAlias`、`TestEmbeddedMethod` |

---

## 9. 系统层 SYS（5/5 已覆盖）

| ID | 功能 | 测试文件 | 覆盖的测试函数 / 用例 |
|---|---|---|---|
| SYS-01 | unsafe.Pointer | `test/unsafebuiltins.go`、`nilptr.go`…`nilptr5.go`、`escape_unsafe.go`、`uintptrkeepalive.go`、`uintptrescapes*.go`；`src/internal/unsafeheader/unsafeheader_test.go`；`src/go/types/builtins_test.go` | `main()`；`TestTypeMatchesReflectType`、`TestWriteThroughHeader`；`TestBuiltinSignatures` |
| SYS-02 | unsafe.Add/Slice/SliceData/String/StringData | `test/unsafebuiltins.go`（`unsafe.Add`/`Slice` nil/负长/溢出 panic）、`unsafe_slice_data.go`、`unsafe_string.go`、`unsafe_string_data.go`；`src/go/types/builtins_test.go` | `main()`/`assert()`/`mustPanic()`；`TestBuiltinSignatures`（Add/Slice/SliceData/String/StringData 条目） |
| SYS-03 | Alignof/Offsetof/Sizeof | `test/sizeof.go`、`align.go`；`src/go/types/sizeof_test.go` | `main()`（嵌入字段 Offsetof issue 4909、`testDeep()`）；`TestSizeof` |
| SYS-04 | 大小与对齐保证 | `src/go/types/sizes_test.go`；`src/cmd/compile/internal/test/align_test.go`；`src/reflect/all_test.go`；`test/sizeof.go`、`align.go`、`zerosize.go` | `TestMultipleSizeUse`、`TestAlignofNaclSlice`、`TestAtomicAlign`、`TestGCSizes`；`TestAlignEqual`；`TestAlignment` |
| SYS-05 | Errors 与运行时 panic | `src/errors/errors_test.go`、`wrap_test.go`、`join_test.go`；`test/recover.go`…`recover5.go`；`src/runtime/panic_test.go`、`panicnil_test.go` | `TestNewEqual`、`TestIs`、`TestAs`、`TestJoin`；`TestPanicWithDirectlyPrintableCustomTypes`、`TestPanicNil` |

---

## 10. AST 节点对账（49 项，语法层全覆盖）

> **关键事实**：`src/cmd/compile/internal/syntax/parser_test.go` **不引用 testdata**；真正遍历 `testdata/` 的是 `error_test.go:TestSyntaxErrors`（`os.ReadDir(testdata)` 覆盖全部 31 个文件）。节点位置由 `nodes_test.go:TestPos` 的子用例集（`decls/exprs/types/fields/stmts/ranges/guards/cases/comms`）逐一验证。

### 10.1 syntax 包测试函数总表

| 文件 | 测试函数（实名确认） |
|---|---|
| `src/cmd/compile/internal/syntax/scanner_test.go` | `TestSmoke`、`TestTokens`、`TestScanner`、`TestEmbeddedTokens`、`TestComments`、`TestNumbers`、`TestScanErrors`、`TestDirectives`、`TestIssue21938`、`TestIssue33961` |
| `src/cmd/compile/internal/syntax/parser_test.go` | `TestParse`、`TestVerify`、`TestStdLib`、`TestIssue17697`、`TestParseFile`、`TestLineDirectives`、`TestLineDirectivesWithDir`、`TestUnpackListExprAllocs` |
| `src/cmd/compile/internal/syntax/nodes_test.go` | `TestPos` |
| `src/cmd/compile/internal/syntax/dumper_test.go` | `TestDump` |
| `src/cmd/compile/internal/syntax/printer_test.go` | `TestPrint`、`TestPrintError`、`TestPrintString`、`TestShortString` |
| `src/cmd/compile/internal/syntax/error_test.go` | `TestSyntaxErrors`（遍历 testdata 全部 31 文件） |
| `src/cmd/compile/internal/syntax/issues_test.go` | `TestIssue67866` |
| `src/cmd/compile/internal/syntax/testing_test.go` | `TestCommentMap` |

### 10.2 节点 → 测试映射

| ID | 节点 | 专项测试文件 | 测试函数 / 子用例 |
|---|---|---|---|
| AST-01 | File | `syntax/parser_test.go` | `TestParse`、`TestStdLib` |
| AST-02 | Group | 同上（声明组解析） | `TestParse` |
| AST-03 | Comment | `syntax/scanner_test.go`、`testing_test.go` | `TestComments`、`TestCommentMap` |
| AST-04 | ImportDecl | `nodes_test.go`；`testdata/sample.go`；`test/syntax/import.go` | `TestPos/decls`；`TestSyntaxErrors` |
| AST-05 | ConstDecl | `nodes_test.go`；`test/syntax/initvar.go` | `TestPos/decls`、`TestPos/stmts` |
| AST-06 | TypeDecl | `nodes_test.go`；`test/syntax/semi6.go` | `TestPos/decls` |
| AST-07 | VarDecl | `nodes_test.go`；`test/syntax/vareq.go`、`vareq1.go` | `TestPos/decls` |
| AST-08 | FuncDecl | `nodes_test.go`；`testdata/issue56022.go` | `TestPos/decls`；`TestSyntaxErrors` |
| AST-09 | BadExpr | — | 由各 ERROR 用例间接覆盖 |
| AST-10 | Name | `nodes_test.go` | `TestPos/exprs` |
| AST-11 | BasicLit | `nodes_test.go` | `TestPos/exprs`（数字/复数/rune/字符串/反引号） |
| AST-12 | CompositeLit | `nodes_test.go`；`test/syntax/composite.go` | `TestPos/exprs`；`TestSyntaxErrors` |
| AST-13 | KeyValueExpr | `nodes_test.go`；`syntax/issues_test.go` | `TestPos/exprs`；`TestIssue67866` |
| AST-14 | FuncLit | `nodes_test.go`；`testdata/slices.go`、`smoketest.go` | `TestPos/exprs` |
| AST-15 | ParenExpr | `nodes_test.go` | `TestPos/exprs` |
| AST-16 | SelectorExpr | `nodes_test.go` | `TestPos/exprs` |
| AST-17 | IndexExpr | `nodes_test.go`；`testdata/issue47704.go` | `TestPos/exprs`；`TestSyntaxErrors` |
| AST-18 | SliceExpr | `nodes_test.go` | `TestPos/exprs`（`a[:]`…`a[i:j:k]`） |
| AST-19 | AssertExpr | `nodes_test.go` | `TestPos/exprs` |
| AST-20 | TypeSwitchGuard | `nodes_test.go`；`test/syntax/typesw.go` | `TestPos/guards` |
| AST-21 | Operation | `nodes_test.go` | `TestPos/exprs`、`TestPos/types` |
| AST-22 | CallExpr | `nodes_test.go`；`test/syntax/ddd.go` | `TestPos/exprs` |
| AST-23 | ListExpr | `nodes_test.go` | `TestPos/exprs`（多值 const/var 声明） |
| AST-24 | ArrayType | `nodes_test.go`；`testdata/issue49482.go` | `TestPos/types` |
| AST-25 | SliceType | `nodes_test.go` | `TestPos/types` |
| AST-26 | DotsType | `nodes_test.go`；`test/syntax/ddd.go`；`testdata/issue43674.go` | `TestPos/types` |
| AST-27 | StructType | `nodes_test.go` | `TestPos/types` |
| AST-28 | Field | `nodes_test.go` | `TestPos/fields` |
| AST-29 | InterfaceType | `testdata/interface.go`、`typeset.go`；`nodes_test.go`；`testdata/issue52391.go` | `TestSyntaxErrors`；`TestPos/types` |
| AST-30 | FuncType | `nodes_test.go`；`testdata/issue48382.go` | `TestPos/types` |
| AST-31 | MapType | `testdata/map.go`、`map2.go`；`nodes_test.go` | `TestSyntaxErrors`；`TestPos/types` |
| AST-32 | ChanType | `testdata/chans.go`；`test/syntax/chan.go`、`chan1.go`；`nodes_test.go` | `TestSyntaxErrors`；`TestPos/types` |
| AST-33 | EmptyStmt | `nodes_test.go` | `TestPos/stmts` |
| AST-34 | LabeledStmt | `nodes_test.go` | `TestPos/stmts` |
| AST-35 | BlockStmt | `nodes_test.go` | `TestPos/stmts` |
| AST-36 | ExprStmt | `nodes_test.go`；`test/syntax/topexpr.go` | `TestPos/stmts` |
| AST-37 | SendStmt | `nodes_test.go`；`test/syntax/chan1.go` | `TestPos/stmts` |
| AST-38 | DeclStmt | `nodes_test.go`；`test/syntax/initvar.go` | `TestPos/stmts` |
| AST-39 | AssignStmt | `nodes_test.go`；`test/syntax/topexpr.go` | `TestPos/stmts` |
| AST-40 | BranchStmt | `nodes_test.go`；`testdata/fallthrough.go`、`issue70974.go` | `TestPos/stmts`；`TestSyntaxErrors` |
| AST-41 | CallStmt | `nodes_test.go` | `TestPos/stmts`（`@defer f()`/`@go f()`） |
| AST-42 | ReturnStmt | `nodes_test.go` | `TestPos/stmts` |
| AST-43 | IfStmt | `nodes_test.go`；`test/syntax/if.go`、`else.go`、`semi1.go`、`semi7.go` | `TestPos/stmts` |
| AST-44 | ForStmt | `nodes_test.go`；`test/syntax/semi3.go`、`semi4.go` | `TestPos/stmts` |
| AST-45 | SwitchStmt | `nodes_test.go`；`test/syntax/semi2.go` | `TestPos/stmts` |
| AST-46 | SelectStmt | `nodes_test.go` | `TestPos/stmts` |
| AST-47 | RangeClause | `nodes_test.go` | `TestPos/ranges` |
| AST-48 | CaseClause | `nodes_test.go` | `TestPos/cases` |
| AST-49 | CommClause | `nodes_test.go` | `TestPos/comms` |

### 10.3 syntax/testdata 31 文件覆盖矩阵

| 文件 | 覆盖特性 |
|---|---|
| `chans.go` | 泛型 + **ChanType** + select/send/recv + 泛型方法 |
| `fallthrough.go` | **BranchStmt(fallthrough)** 位置合法性 |
| `interface.go` | **InterfaceType** 约束元素（`~int`、`int\|string`、嵌入） |
| `typeset.go` | 纯类型集、`~t`、`t\|t`、数组长度 vs 类型参数歧义 |
| `linalg.go` | 泛型约束接口 + 泛型方法 + 实例化 |
| `map.go` / `map2.go` | 泛型 map（`Map[K,V]`） |
| `slices.go` | 泛型 `Map/Reduce/Filter` + 类型推断 |
| `smoketest.go` | 泛型全景（TParamList/实例化/嵌入/类型集/泛型方法） |
| `tparams.go` | **TParamList** 消歧（`missing type constraint`/`missing type parameter name`） |
| `sample.go` | 错误注释 harness 示例 |
| `issue20789.go` | 无换行 EOF 容错 |
| `issue23385.go` | `=` 误用为 `==` 的错误恢复 |
| `issue23434.go` | 缺失类型后的同步 |
| `issue31092.go` | const/type/var 中 `:=` 错误恢复 |
| `issue43527.go` | 泛型参数列表缺约束/名字 |
| `issue43674.go` | `...` 缺类型（DotsType） |
| `issue46558.go` | switch case 中意外 `{` |
| `issue47704.go` | **IndexExpr** `m[]`/`m[x,]`/`m[x a b c]` |
| `issue48382.go` | 函数类型/接口方法不得有类型参数 |
| `issue49205.go` | 结构缺分号/逗号错误消息 |
| `issue49482.go` | **ArrayType/TypeDecl** 消歧（`*T,`/`[P(T)]`） |
| `issue52391.go` | interface 中括号/`~`/`\|` 位置错误 |
| `issue56022.go` | func 缺名字/接收者 |
| `issue60599.go` | if 条件中赋值错误 |
| `issue63835.go` | 接收者泛型 `[]byte` |
| `issue65790.go` | 语句末尾多余 name |
| `issue68589.go` | `var _ (type T)` |
| `issue69506.go` | 参数列表缺类型/名字 |
| `issue70957.go` | `goto` 后缺标签名 |
| `issue70974.go` | **BranchStmt(break)** label 校验 |

---

## 11. Runtime 五大子系统

### 11.1 内存与 GC（RT-MEM）

| ID | 功能 | 测试文件 | 覆盖函数 |
|---|---|---|---|
| RT-MEM-01 | 多级分配器 | `src/runtime/malloc_test.go`、`mpagealloc_test.go`、`mpagecache_test.go`、`mpallocbits_test.go`、`mranges_test.go`、`mgcscavenge_test.go` | `TestTinyAlloc`、`TestTinyAllocIssue37262`、`TestPageAllocGrow/Alloc/Exhaust/Free`、`TestPageCacheAlloc/Flush`、`TestPallocBitsSummarize*`、`TestAddrRangesAdd/FindSucc`、`TestScavenger` |
| RT-MEM-02 | 小/大对象路径 | `src/runtime/malloc_test.go`、`malloc_bench_generated_test.go`、`metrics_test.go` | `TestTinyAlloc`、`BenchmarkMalloc8/16/32`、`BenchmarkMallocLargeStruct`、`BenchmarkMallocgc`、`TestMetricHeapUnusedLargeObjectOverflow` |
| RT-MEM-03 | 栈内存管理 | `src/runtime/stack_test.go`、`proc_test.go`、`debug_test.go` | `TestStackGrowth`、`TestStackCache`、`TestStackPanic`、`TestFramePointerAdjust`、`TestPreemptSplitBig`、`TestDebugCallGrowStack` |
| RT-MEM-04 | GC STW Mark-Sweep | `src/runtime/gc_test.go`、`proc_test.go`、`testdata/testprog/stw_trace.go` | `TestGcSys`、`TestGcRescan`、`TestGcLastTime`、`TestUserForcedGC`、`TestStopTheWorldDeadlock`、`TestTraceSTW`、`TraceGCSTW()` |
| RT-MEM-05 | 并发标记与写屏障 | `src/runtime/gc_test.go`、`mgcpacer_test.go`、`mgclimit_test.go` | `BenchmarkWriteBarrier`、`BenchmarkBulkWriteBarrier`、`BenchmarkScanStackNoLocals`、`TestGcPacer`、`TestIdleMarkWorkerCount`、`TestGCCPULimiter` |
| RT-MEM-06 | 堆位图/指针元数据 | `src/runtime/gcinfo_test.go`、`gc_test.go`、`malloc_test.go` | `TestGCInfo`、`TestHugeGCInfo`、`TestGCTestPointerClass`、`TestGCTestIsReachable`、`TestScanAllocIssue77573` |
| RT-MEM-07 | mallocgc/newobject | `src/runtime/malloc_test.go` | `TestMkmalloc`、`TestScanAllocIssue77573`；（`export_test.go` 导出 `MallocGC`、`PersistentAlloc`） |
| RT-MEM-08 | 内存统计与 GOMEMLIMIT | `src/runtime/gc_test.go`、`malloc_test.go`、`metrics_test.go`、`testdata/testprog/gc.go` | `TestMemoryLimit`、`TestMemoryLimitNoGCPercent`、`TestReadMemStats`、`TestMemStats`、`TestReadMetricsConsistency`、`GCMemoryLimit()` |

### 11.2 调度与上下文（RT-SCH）

| ID | 功能 | 测试文件 | 覆盖函数 |
|---|---|---|---|
| RT-SCH-01 | 协程控制块 g | `src/runtime/proc_test.go`、`metrics_test.go`、`crash_test.go` | `TestGetgThreadSwitch`、`TestNumGoroutine`、`TestLockOSThread*`、`TestMainGoroutineID` |
| RT-SCH-02 | 汇编上下文切换 | — | ⚠️ **未找到专用测试**（`gogo`/`mcall` 无单测，仅被调度/panic/traceback 间接覆盖） |
| RT-SCH-03 | GMP 模型 | `src/runtime/proc_test.go`、`testdata/testprog/gomaxprocs.go` | `TestSchedLocalQueue`、`TestSchedLocalQueueSteal`、`TestStealOrder`、`TestGoroutineParallelism`、`TestBigGOMAXPROCS`、`TestPingPongHog`；`PrintGOMAXPROCS()` |
| RT-SCH-04 | newproc | `src/runtime/proc_test.go`、`runtime_test.go` | `TestNumGoroutine`、`BenchmarkCreateGoroutines*`、`TestGoroutineProfileTrivial` |
| RT-SCH-05 | gopark/goready | `src/runtime/chan_test.go`、`sema_test.go`、`semasleep_test.go`、`crash_test.go` | `TestShrinkStackDuringBlockedSend`、`TestNoShrinkStackWhileParking`、`TestSemaHandoff`、`TestSpuriousWakeupsNeverHangSemasleep`、`TestSimpleDeadlock` |
| RT-SCH-06 | mstart/调度循环 | `src/runtime/proc_test.go`、`runtime_test.go`、`testdata/testprog/lockosthread.go` | `TestMexitSTW`、`BenchmarkProcYield`、`BenchmarkOSYield`；`LockOSThreadMain()` |
| RT-SCH-07 | 抢占与安全点 | `src/runtime/proc_test.go`、`unsafepoint_test.go`、`debug_test.go`、`testdata/testprog/preempt.go` | `TestPreemption`、`TestPreemptionGC`、`TestAsyncPreempt`、`TestPreemptionAfterSyscall`、`TestUnsafePoint`、`TestDebugCallUnsafePoint` |
| RT-SCH-08 | 定时器子系统 | `src/runtime/time_test.go`、`proc_test.go`；`src/time/sleep_test.go`、`tick_test.go` | `TestFakeTime`、`TestTimerFairness`、`TestSleep`、`TestAfterFunc`、`TestTickerStress`、`TestAdjustTimers`、`TestMultiWakeupTicker` |

### 11.3 复合类型（RT-CMP）

| ID | 功能 | 测试文件 | 覆盖函数 |
|---|---|---|---|
| RT-CMP-01 | Slice 头/扩容 | `src/runtime/slice_test.go`、`runtime_test.go`、`memmove_test.go`；`test/makeslice.go`、`append.go`、`slice3.go`、`slicecap.go` | `TestSideEffectOrder`、`TestAppendOverlap`、`TestAppendGeneric`、`TestAppendSliceGrowth`、`TestMemmove`、`TestMemclr`、`BenchmarkGrowSlice`、`BenchmarkAppend*` |
| RT-CMP-02 | String 头/字符串操作 | `src/runtime/string_test.go`、`runtime_test.go` | `TestStringW`、`TestLargeStringConcat`、`TestStringOnStack`、`TestIntString`、`TestString2Slice`、`TestRangeStringCast`、`TestEqString`、`BenchmarkCompareString*` |
| RT-CMP-03 | Map 桶算法/SwissTable | `src/runtime/map_test.go`、`map_benchmark_test.go`；`src/internal/runtime/maps/map_test.go`、`fuzz_test.go` | `TestHmapSize`、`TestBigItems`、`TestGrowWithNaN`、`TestNegativeZero`、`TestEmptyKeyAndValue`、`TestMapPut`、`TestSmallMapGrow`、`TestTableClear`、`TestMapIndirect`、`BenchmarkHashStringSpeed`、`BenchmarkMapAccessHit/Miss` |
| RT-CMP-04 | Map 迭代随机化 | `src/runtime/map_test.go`；`src/internal/runtime/maps/map_test.go` | `TestMapIterOrder`、`TestMapSparseIterOrder`、`TestMapIterDuplicate`、`TestIterGrowWithGC`、`TestTableIteration`、`TestTableIterationGrowDelete` |
| RT-CMP-05 | Channel 结构 hchan | `src/runtime/chan_test.go`、`chanbarrier_test.go` | `TestChan`、`TestChanSendBarrier`、`TestChanSendSelectBarrier`、`BenchmarkMakeChan`、`BenchmarkChanContended` |
| RT-CMP-06 | Channel 语义 | `src/runtime/chan_test.go`；`test/chan/`（19 项）；`test/makechan.go`、`closedchan.go` | `TestNonblockRecvRace`、`TestSelectFairness`、`TestSelectStress`、`TestMultiConsumer`、`TestPseudoRandomSend`、`BenchmarkChanProdCons*`、`BenchmarkReceiveDataFromClosedChan` |
| RT-CMP-07 | 零大小类型 | `src/runtime/map_test.go`、`iface_test.go`、`mfinal_test.go`、`mcleanup_test.go`、`pinner_test.go` | `TestGroupSizeZero`、`TestMapHugeZero`、`TestZeroConvT2x`、`TestFinalizerZeroSizedStruct`、`TestCleanupZeroSizedStruct`、`TestPinnerPinZerosizeObj` |

### 11.4 多态与反射（RT-POLY）

| ID | 功能 | 测试文件 | 覆盖函数 |
|---|---|---|---|
| RT-POLY-01 | `_type` 元数据 | `src/runtime/sizeof_test.go`、`typelinksrace_test.go`、`align_test.go`、`abi_test.go`；`src/reflect/type_test.go` | `TestSizeof`、`TestTypelinksRace`、`TestAtomicAlignment`、`TestFinalizerRegisterABI`、`TestTypeFor`、`TestIsRegularMemory` |
| RT-POLY-02 | 接口值布局 iface/eface | `src/runtime/iface_test.go`、`runtime_test.go`、`sizeof_test.go` | `TestCmpIfaceConcreteAlloc`、`TestNonEscapingConvT2E`、`TestNonEscapingConvT2I`、`TestZeroConvT2x`、`BenchmarkConvT2E*`、`BenchmarkConvT2I*`、`BenchmarkIfaceCmp100` |
| RT-POLY-03 | itab/getitab | — | ⚠️ **未找到专用测试**（无 `itab_test.go`）。间接：`src/reflect/set_test.go` 的 `TestImplements`、`TestAssignableTo`；`runtime/iface_test.go` 的 `BenchmarkConvT2I*` |
| RT-POLY-04 | 类型断言代码生成 | `src/runtime/iface_test.go`；`src/reflect/all_test.go`；`test/interface/assertinline.go` | `BenchmarkAssertE2T/E2I/I2T/I2I/E2E`（及 `*2`/`*2Blank` 变体）；`TestTypeAssert*`、`TestTypeAssertPanic` |
| RT-POLY-05 | 泛型分派 | `src/runtime/gc_test.go`；`test/typeparam/` | `TestMyGenericFunc`；⚠️ 无字面 `GCShape` 关键字的测试 |
| RT-POLY-06 | 反射元数据 | `src/reflect/all_test.go`、`type_test.go`、`set_test.go`；`src/runtime/symtab_test.go` | `TestTypeOf`、`TestMethod`、`TestMethodValue`、`TestMethodPkgPath`、`TestConvert`、`TestImplements`、`TestCaller`、`TestLineNumber` |
| RT-POLY-07 | method value 闭包化 | `src/reflect/all_test.go`；`src/runtime/closure_test.go`、`proc_test.go`；`test/typeparam/boundmethod.go` | `TestMethodValue`、`TestMethodByNameUnExportedFirst`、`BenchmarkCallClosure*`、`BenchmarkClosureCall` |

### 11.5 异常与展开（RT-EXC）

| ID | 功能 | 测试文件 | 覆盖函数 |
|---|---|---|---|
| RT-EXC-01 | defer 链表管理 | `src/runtime/defer_test.go`、`stack_test.go`、`mfinal_test.go`；`test/defer*.go` | `TestOpenAndNonOpenDefers`、`TestNonOpenAndOpenDefers`、`TestConditionalDefers`、`TestDisappearingDefer`、`TestDeferWithRepeatedRepanics`、`TestIssue37688/43920/43921/43941`、`TestDeferPtrs*`、`TestDeferHeapAndStack`、`TestDeferKeepAlive` |
| RT-EXC-02 | panic 级联展开 | `src/runtime/panic_test.go`、`panicnil_test.go`、`crash_test.go`、`testdata/testprog/crash.go` | `TestPanicRecoverSpeed`、`TestRecursivePanic`…`5`、`TestRepanickedPanic`、`TestPanicWhilePanicking`、`TestDoublePanicWithSameValue`、`TestPanicLoop`、`TestPanicInlined` |
| RT-EXC-03 | recover 拦截器 | `src/runtime/panic_test.go`、`defer_test.go`、`runtime_test.go`、`crash_test.go`、`stack_test.go` | `TestRecoverMatching`、`TestRecoveredPanicAfterGoexit`、`TestGoexitInPanic`、`TestPanicUseStack`、`TestPanicFar`、`BenchmarkPanicRecover` |
| RT-EXC-04 | 运行时致命错误 | `src/runtime/crash_test.go`、`security_test.go`、`testdata/testprog/{abort,crash,segv,deadlock}.go` | `TestAbort`、`TestCrashHandler`、`TestRuntimePanic`、`TestG0StackOverflow`、`TestThreadExhaustion`、`TestSUID` |
| RT-EXC-05 | 栈展开与 traceback | `src/runtime/traceback_test.go`、`traceback_system_test.go`、`symtab_test.go`、`start_line_test.go`、`crash_test.go` | `TestTracebackInlined`、`TestTracebackElision`、`TestTracebackArgs`、`TestTracebackGeneric`、`TestTracebackParentChildGoroutines`、`TestStartLine`、`TestPanicTraceback`、`TestBadTraceback` |
| RT-EXC-06 | 编译器协同 | `src/runtime/defer_test.go`、`runtime_test.go`；`test/defer*.go` | `TestNonSSAableArgs`、`TestDeferForFuncWithNoExit`、`BenchmarkDefer`、`BenchmarkDefer10`、`BenchmarkDeferMany` |

---

## 12. 编译器流水线 PIPE（28/28 有测试资源）

| ID | 功能 | 测试文件 | 测试函数 / 用例 |
|---|---|---|---|
| PIPE-01 | 编译驱动与 CLI | `src/cmd/compile/script_test.go` + `testdata/script/`（11 项） | `TestMain`、`TestScript`（`script_test_basics.txt`、`closure_name.txt`、`dwarf5_gen_assembly_and_go.txt`、`embedbad.txt`、`issue70173/73947/75461/77033/80258.txt`） |
| PIPE-02 | 词法扫描器 | `syntax/scanner_test.go` | `TestSmoke`、`TestTokens`、`TestScanner`、`TestEmbeddedTokens`、`TestComments`、`TestNumbers`、`TestScanErrors`、`TestDirectives` |
| PIPE-03 | 解析器 | `syntax/parser_test.go`、`error_test.go` | `TestParse`、`TestVerify`、`TestStdLib`、`TestParseFile`、`TestLineDirectives`；`TestSyntaxErrors`（31 testdata） |
| PIPE-04 | 语言版本门控 (-lang) | `test/embedvers.go`、`newexpr.go`、`fixedbugs/issue11614/23609/26416/31747/34329/46525/49368/51531/55889/63489a/63489b/67141.go`、`bug195.go`；`types2/api_test.go` | 首行 `// <mode> -lang=go1.xx`；`TestFileVersions`、`TestTooNew`、`TestModuleVersion` |
| PIPE-05 | Pragma/编译指令 | `test/directive.go`、`directive2.go`、`linkname.go`、`linkname3.go`、`linknameasm.go`、`nosplit.go`、`nowritebarrier.go`；`noder/lex_test.go`；`src/go/build/constraint/expr_test.go` | `TestPragmaFields`、`TestPragcgo`；`TestParse`、`TestExprEval`、`TestParsePlusBuildExpr` |
| PIPE-06 | 导入解析 | `src/cmd/compile/internal/importer/gcimporter_test.go`；`test/import*.go` | `TestImportTestdata`、`TestVersionHandling`、`TestImportStdLib`、`TestImportedTypes`、`TestCorrectMethodPackage`、`TestGenMeth`、`TestIssue5815/13566/13898/15517/15920/20046/25301/25596/63285/69912` |
| PIPE-07 | 类型检查器 | `types2/*_test.go`（28 文件） | `TestCheck`（check 74 语料）、`TestSpec`、`TestExamples`、`TestFixedbugs`、`TestLocal`、`TestManual`；`TestValuesInfo`、`TestTypesInfo`、`TestScopesInfo`、`TestStdlib` |
| PIPE-08 | 常量折叠 | `types2/check_test.go`；`check/const0.go`、`const1.go`、`constdecl.go`；`test/const*.go` | `TestLongConstants`、`TestIndexRepresentability`、`TestIssue47243_TypedRHS` |
| PIPE-09 | 作用域解析 | `types2/lookup_test.go`、`resolver_test.go`、`api_test.go` | `BenchmarkLookupFieldOrMethod`、`TestResolveIdents`、`TestScopesInfo`、`TestLookupFieldOrMethod*` |
| PIPE-10 | IR 树构建 | `noder/lex_test.go`；`ir/dump_test.go`、`func_test.go`、`html_test.go`、`sizeof_test.go` | `TestPragmaFields`、`TestPragcgo`；`TestMatchPkgFn`、`TestSplitPkg`、`TestHTMLWriter`、`TestSizeof` |
| PIPE-11 | 泛型处理 | `types2/instantiate_test.go`、`mono_test.go`、`typeterm_test.go`、`termlist_test.go`、`typeset_test.go`；`test/typeparam/`（363） | `TestInstantiateEquality`、`TestMonoGood`、`TestMonoBad`、`TestTerm*`、`TestTermlist*`、`TestTypeSetString` |
| PIPE-12 | 内联 | `test/inline*.go`（9）；`inline/inlheur/*_test.go` | `TestSerDeser`、`TestFuncProperties`、`TestDumpCallSiteScoreDump`、`TestInlScoreAdjFlagParse`、`TestClassifyIntegerCompare/Float/AssortedShifts` |
| PIPE-13 | 逃逸分析 | `test/escape*.go`（33） | 均 `// errorcheck -0 -m` 泄漏诊断。⚠️ `escape/` 目录无单测 |
| PIPE-14 | 闭包转换 | `test/closure*.go`（8，含 `closure3.dir`、`closure5.dir`） | ⚠️ `walk/` 目录无单测 |
| PIPE-15 | 方法与接口降级 | `test/interface/`（26）；`test/method*.go`（8） | `assertinline.go` 首行 `// errorcheck -0 -d=typeassert` |
| PIPE-16 | 内建函数降级 | `test/append.go`、`append1.go`、`copy.go`、`copy1.go`、`clear.go`、`clearfat.go`；`types2/builtins_test.go` | `TestBuiltinSignatures`；⚠️ `walk/` 无单测 |
| PIPE-16b | range over func (1.23) | `src/cmd/compile/internal/rangefunc/rangefunc_test.go`；`test/range4.go`、`rangegen.go` | 测试函数 + `// run` 程序 |
| PIPE-16c | 循环变量语义 (1.22) | `src/cmd/compile/internal/loopvar/loopvar_test.go` + `testdata/` | `TestLoopVarGo1_21` |
| PIPE-17/18 | SSA 构建与优化 | `ssa/*_test.go`（36） | `TestSCCP*`、`TestDeadStore*`、`TestShortCircuit`、`TestSchedule`、`TestCSEAuxPartitionBug`、`TestBranchElimIf*`、`TestFuse*`、`TestLICM*`、`TestDominators*`、`TestMagic*`、`TestWriteBarrier*`、`TestCondRewrite`、`TestConstCache`、`TestGeneratedFilesUpToDate` |
| PIPE-19/20 | lowering 与寄存器分配 | `ssa/regalloc_test.go`；`amd64/versions_test.go`、`versions_simd_test.go`、`versions_nosimd_test.go`；`test/abi/`（56） | `TestSpillWithLoop`、`TestSpillMove1/2`、`TestClobbersArg0/1`、`TestLiveControlOps`、`TestPreload`、`TestGoAMD64v1`、`TestPopCnt`、`TestBLSI/BLSMSK/BLSR`、`TestFMA` |
| PIPE-21 | 栈帧与活跃性 | `liveness/intervals_test.go`；`test/live*.go`（5） | `TestIntervalOverlap`、`TestIntervalMerge`、`TestRandomIntervalsOverlap`、`TestBuilder` |
| PIPE-22 | 对象文件与元数据 | `reflectdata/alg_test.go`；`test/alg.go`、`bigalg.go` | `BenchmarkEqArrayOfStrings5/64/1024`、`BenchmarkEqArrayOfStructsEq/NotEq`、`BenchmarkEqStruct`；⚠️ `objw/`、`staticdata/` 无单测 |
| PIPE-23 | 链接器 | `src/cmd/link/internal/ld/*_test.go`（13） | `TestAddGotSym`、`TestWriteULebFixedLength`、`TestDeadcode`、`TestRuntimeTypesPresent`、`TestSizes`、`TestInlinedRoutineCallFileLine`、`TestDynSymShInfo`、`TestElfBindNow`、`TestUndefinedRelocErrors`、`TestLargeTextSectionSplitting` |
| PIPE-24 | 包初始化与启动 | `test/init.go`、`init1.go`、`initloop.go`、`initializerr.go`、`initialize.go` | `// errorcheck`/`// run`；⚠️ `pkginit/` 无单测 |
| PIPE-25 | 诊断系统 | `types2/errors_test.go`、`errorcalls_test.go`；`syntax/error_test.go` | `TestError`、`TestStripAnnotations`、`TestErrorCalls`、`TestSyntaxErrors` |
| PIPE-26 | 阶段调试 dump | `ir/html_test.go`、`ir/dump_test.go` | `TestHTMLWriter`、`TestMatchPkgFn`；⚠️ `ssa/html_test.go` 不存在 |
| PIPE-27 | 汇编器 | `src/cmd/asm/internal/asm/*_test.go`（5）、`lex/lex_test.go` | `TestLex`、`TestBadLex`、`TestExpr`、`TestOperandParser` 系列、`Test*EndToEnd`、`TestAMD64Encoder`、`TestLOONG64Encoder`、`TestRISCV64Validation`、`TestErroneous` |
| PIPE-28 | 覆盖/调试信息 | `dwarfgen/linenum_test.go`、`scope_test.go`；`test/dwarf/` | `TestIssue75249`、`TestScopeRanges`、`TestEmptyDwarfRanges`；⚠️ `coverage/` 无单测 |

---

## 13. 未找到专用测试的功能点（待补充清单）

| 功能 ID | 功能 | 现状 | 建议 |
|---|---|---|---|
| TYP-26 | 类型统一算法 | 无 `unify_test.go`，仅由 TYP-25 语料间接覆盖 | 新增 `unify_test.go` 直接断言 unification 结果 |
| SCP-06 | 导出标识符 | 无直接断言 `Exported()` 的单测 | 在 `go/types` 新增显式导出性测试 |
| EXP-16 | 运算符优先级 | 仅 `TestVerify`（Fprint 往返）与 `bug448.dir` 间接覆盖 | 新增优先级矩阵用例 |
| EXP-22 | 逻辑运算符短路 | `test/codegen/shortcircuit.go` 实为类型断言单跳转 | 新增 `&&`/`\|\|` 短路求值顺序用例 |
| PKG-04 | 零值 | 无同名专用测试，仅 `nil.go`/`varinit.go`/`zerosize.go` 间接覆盖 | 新增按类型的零值规则表测试 |
| PKG-06 | 初始化依赖循环检测 | 仅 `test/initloop.go` 一处；types2 侧无语料 | 补充 types2 语料与正向顺序用例 |
| RT-SCH-02 | 汇编上下文切换 | `gogo`/`mcall` 无双测 | 新增寄存器保存/恢复的定向测试 |
| RT-POLY-03 | itab/getitab | 无 `itab_test.go` | 新增 itab 缓存/并发构建测试 |
| RT-POLY-05 | 泛型分派字典 | 无字面 `GCShape` 关键字测试 | 新增字典传参与实例化共享的定向测试 |

**另有 7 个编译器目录无任何 `*_test.go`**（测试依赖 `test/` 目录的端到端用例）：
`gc/escape/`、`gc/walk/`、`gc/coverage/`、`gc/pkginit/`、`gc/objw/`、`gc/staticdata/`、`gc/arm64/`

---

## 14. 统计汇总

| 维度 | 功能点数 | 有专项测试 | 仅间接覆盖 | 未找到 |
|---|---|---|---|---|
| 词法 LEX | 13 | 13 | 0 | 0 |
| 常量 CON | 8 | 8 | 0 | 0 |
| 类型 TYP | 26 | 25 | 0 | 1 |
| 作用域 SCP | 10 | 9 | 0 | 1 |
| 表达式 EXP | 27 | 25 | 0 | 2 |
| 语句 STM | 21 | 21 | 0 | 0 |
| 内建 BIF | 15 | 15 | 0 | 0 |
| 包初始化 PKG | 8 | 7 | 0 | 1 |
| 系统层 SYS | 5 | 5 | 0 | 0 |
| AST 对账 | 49 | 49 | 0 | 0 |
| Runtime | 36 | 3 子系统全有 | 3 | 3 |
| 流水线 PIPE | 28 | 28 | 0 | 0 |
| **合计** | **246** | **~236** | **3** | **9** |

**测试资源总量**：
- `test/` 顶层语言测试程序 ≈ 600 个 + `test/fixedbugs/` 2096 个 + `test/typeparam/` 363 个 + `test/abi/` 56 个 + 其他子目录 ≈ 3200 个程序
- 编译器中端/后端 `*_test.go`：syntax 8 + types2 28 + ssa 36 + ir 4 + 其他 ≈ 90 个
- `src/runtime/*_test.go`：135 个
- `src/internal/types/testdata/`：check 74 + fixedbugs 274 + spec/examples 若干 ≈ 370 个语料

> **结论**：246 个功能点中约 236 个（96%）存在可直接复用的专项测试；9 个功能点缺少直接测试但均有间接覆盖路径。由于 `test/` 与 `src/internal/types/testdata/` 均为**语言级黑盒测试**（只依赖编译器对外行为），它们可直接作为 Rust 重写实现的验收测试集。
