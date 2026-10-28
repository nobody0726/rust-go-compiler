# M12 设计 · 泛型 native / 有限反射 / 可选优化

> 契约：[`C13-generics.md`](./../contracts/C13-generics.md)（本文档定稿时同步新建）/
> [`C5`](../contracts/C5-object-model.md) §10 类型元数据 / [`C12`](../contracts/C12-heap-and-gc.md) 堆与根 /
> [`C8`](../contracts/C8-check-diagnostics.md) 诊断码 / [`C9`](../contracts/C9-values-and-output.md) 输出与退出码。
> 台账：`M12-plan.md`（Task 0 起）/ 判据：`M12-tests.md` / 清单：`M12-manifest.json`。

## 0. 本文档在流程中的位置

```text
03-roadmap.md §M12（三条扩展线）
  → 本文档（M12-design.md）：决策 + 事实基线 + 门禁 + 白名单
    → M12-tests.md：判据清单（T-GEN / T-REF / T-OPT / T-M12）
      → M12-plan.md：2–5 分钟粒度的实施任务
```

本文档**只描述做什么与为什么**；任务拆分在 `M12-plan.md`，判据正文在 `M12-tests.md`。

---

## 1. 目标与非目标

### 1.1 输入与输出

| 项 | 输入 | 输出 |
|---|---|---|
| a 泛型 | Go 源码（含类型参数）+ 官方 `test/typeparam` 的自包含子集 | aarch64 ELF，**每个实例一份代码**（单态化） |
| b 反射 | 白名单内的 `reflect` 调用 | 同一份 ELF 里的只读元数据 + 动态调用桥 + 写面 |
| c 优化 | 已通过 a/b 的程序 | 更小/更快的同一语义（`opt_equiv` 判等） |

### 1.2 范围（`03-roadmap.md` §M12 逐句展开，含本次实测修正）

roadmap 把 M12 写成三条**互相独立**的扩展线，本文档照此分解为三个子阶段：

| 子阶段 | roadmap 原句 | 本次落地形态 |
|---|---|---|
| **M12a 泛型 native** | 「先 native 全量单态化，覆盖泛型闭包、method value、interface/类型 switch 和必要的类型身份」 | 单态化在 **load 层**完成（§3.2）；门禁 = **116 个无 stdlib import 的文件**（71 `run` + 40 `compile` + 5 `errorcheck`）|
| **M12b 有限反射** | 「明确包/API 白名单；先 size/alignment、kind、字段偏移/Tag、方法表和类型身份，再定义最小调用接口」 | 白名单 = 读 + `Call` + 写面（§3.5–3.7）；`reflect` 用**源码桩**引入 |
| **M12c 优化** | 「shape/dictionary 共享、内联（PIPE-12）、精细逃逸分析（PIPE-13）、SwissTable、SSA 高级优化」 | 内联 / 精细逃逸 / SwissTable / **字典替换**（§3.8）；每段独立、独立读数 |

**实测修正（相对 roadmap 的原始假设）**：

1. roadmap 假定「泛型小切片可在 M9 后独立交付」—— 实测**不成立**：今天 141 个 `// run` 里 check 只过 22、build 只过 3、run 只过 3（`F3`）。泛型不是「收尾补丁」，而是一条独立长线。
2. roadmap 未提 stdlib —— 实测带 stdlib import 的有 **76 个**（`// run` 71 / `// compile` 4 / `// errorcheck` 1；`fmt` 占绝大多数）。本阶段**不碰 stdlib**（`U-M12-01`），语料门禁因此收缩到**无 stdlib import 的子集**（§4.2）。
   ⚠️ 口径（**Task 0 三次翻车换来的**，见 `M12-baseline.md` §1.2）：判据是「**import 集合 ⊆ {unsafe}**」，且要**注释感知** —— ① `issue50417.go`/`issue50417b.go` 的 `import "fmt"` 在块注释里；② `unsafe` 是 M4 支持的伪 import；③ 指令带尾随参数（`// run -gcflags="-l"`）要用 `^// run( |$)` 匹配。用 `grep -lE '^import|"'` 会把带字符串字面量的文件全误排除（第一版把 69 数成了 37）。

### 1.3 明确不做（M12 边界）

* **stdlib import**（`fmt` / `strings` / `strconv` / `sort` / `math` / `errors` / `sync` / `runtime`）—— **76 个**带 stdlib import 的文件只做**拒绝面**判据。
* **`reflect.MakeFunc`**（`U-M12-02`）：它是 `Call` 的镜像，需要反向 trampoline，语料里没有。
* **可变参数 `Call`**（`Value.CallSlice` 与 `...any` 实参，`U-M12-03`）。
* **并发 GC / 写屏障**（沿用 `U-M11-08`）：M12 仍是 STW + 单线程，`reflect.Value.Set` 因此不需要屏障（§3.7 写清前提）。
* **PGO / DWARF / coverage**（`U-M12-04`，roadmap 已划延期）。
* **`unsafe` 白名单外**（沿用 M4 的 `U-M4-06`）。
* **泛型原体的独立检查**（`U-M12-05`）：只检查**可达实例**，未被调用的泛型函数体里的类型错误不报（`D-M12-3` 的副作用）。
* **`.dir` 目录测试**（`U-M12-07`）：`// rundir`/`// compiledir` 的 67 个 `.go` 全是 `package ignored` 桩，真测试在同名目录里、用**相对包 import**（`import "./a"`）—— 属「包解析」，roadmap 已划延期。
* **`errorcheck -m`**（`U-M12-08`）：2 个文件的期望错是 gc 的**内联报告**（`can inline …`），我们没有 `-m`。

### 1.4 退出条件（门禁十三条）

| ID | 门禁 | 判据族 |
|---|---|---|
| `G12-1` | 泛型静态面：40 个无 stdlib import 的 `// compile` 全过 | `T-GEN-02` |
| `G12-2` | 泛型运行面：71 个无 stdlib import 的 `// run` 三层一致 | `T-GEN-01` |
| `G12-3` | 拒绝面：**76** 个干净拒绝且诊断码集合 ⊇ `TYP-UNSUPPORTED-IMPORT-FORM`；5 个 `errorcheck` 的首码 ∈ 登记集合 | `T-GEN-03` |
| `G12-4` | 单态化完备性：`.s` 无 `type parameter` 残留 + 实例名确定 | `T-GEN-04/05` |
| `G12-5` | 泛型 × 并发/GC：语料里的 `chan`/`map` 用例三态 | `T-GEN-06` |
| `G12-6` | 不回归：69 单线程 + 7 并发三态 + 全量 cargo | `T-GEN-07` |
| `G12-7` | 反射元数据：kind/size/align/field/tag 与官方一致 | `T-REF-01/02` |
| `G12-8` | 反射调用：`Call` 的 ABI 分档全覆盖 | `T-REF-03` |
| `G12-9` | 反射写面：`Set*`/`Make*`/`New` 生效且 GC 正确 | `T-REF-04/06` |
| `G12-10` | 内联：`opt_equiv` + 三态不变 + 体积读数 | `T-OPT-01` |
| `G12-11` | 精细逃逸：`allocs` 下降 + 三态不变 | `T-OPT-02` |
| `G12-12` | SwissTable：语义不变 + probe 读数 | `T-OPT-03` |
| `G12-13` | 字典替换：116 个门禁语料重跑 + 体积读数（**可切断**） | `T-OPT-04` |

`G12-1..6` 属 M12a，`7..9` 属 M12b，`10..13` 属 M12c。**三组互不阻塞**：a 收口即可发布，b/c 各自独立。

---

## 2. 决策记录

### 2.1 决策一览

| # | 决策 | 一句话理由 |
|---|---|---|
| `D-M12-1` | 三条线**独立门禁、顺序 a→b→c**、可各自发布或切断 | 三条线合计 12–24 人周；绑在一起验收就是烂尾 |
| `D-M12-2` | **单态化在 `rgoc-load` 层完成**，HIR 以下看不到泛型 | 后端零改动；从根上绕开 M4 的 `A3`（dump 根集不产实例） |
| `D-M12-3` | 实例化闭包从**非泛型根**出发（`main`/`init`/反射用到的类型） | 不做全程序全实例（266 个语料里很多泛型函数根本没人调） |
| `D-M12-4` | 泛型 ABI：**先单态化，c 线换字典**；字典是重写、排最后、第一个可切断 | 正确性基线先立住，优化不回头推翻它 |
| `D-M12-5` | `reflect` 用**源码桩**；`TypeDesc` 40 字节不动，`pad` 改 `meta_idx` 旁挂 `ReflectMeta` | 白名单由包内容决定（不用第二份清单）；不碰 `C5` §10 冻结布局 |
| `D-M12-6` | 反射白名单 = **读 + `Call` + 写面**；`MakeFunc` 排除 | 全写面是本次拍板的上限；`MakeFunc` 是另一个量级 |
| `D-M12-7` | 写面**不需要写屏障**（STW + 单线程），但要写进 `C12` 当**前提** | 将来上并发 GC 时这条会静默失效，必须留字据 |
| `D-M12-8` | 语料门禁 = **71 `run` + 40 `compile` + 5 `errorcheck` = 116 个无 stdlib import 的文件**；76 个转拒绝面；67 个 `.dir` 桩与 5 个无指令不纳入 | 拒绝理由不对的判据就是假绿 |
| `D-M12-9` | 官方层用容器内的 `go run`（go1.27.1） | 三层差分的第三层（`F11`） |
| `D-M12-10` | **Task 0 = 基线测绘**（116 个逐条归因），没有它不排计划 | 四档怎么切必须是算出来的，不是拍的 |

### 2.2 只对**非显然**的五条展开

#### `D-M12-2` 为什么把单态化放在 load 层

今天 native 的失败形态是 `帧布局失败：函数 F：v0 的类型 t32 没有运行期布局` 与 `VerifyError: 常量 Int(7) 不可能有类型 TypeId(32)`（`F5`）—— 两者都说明 **SSA 拿到的是「带类型参数的常量」**。也就是说：泛型信息一路漏到了后端。

两条路：

| 路线 | 做法 | 代价 |
|---|---|---|
| **A（采纳）** | 在 `scope::resolve_bodies` **之前**插一遍「实例化闭包」，实例以 mangled 名（`Sum·int`）作为**普通 `FuncDecl`/`TypeDecl`** 进符号表，泛型原体不进 HIR | 后端（SSA/isel/GC/typedesc）**零改动**；代价是代码体积 |
| B | 在 SSA 层做实例化 | 后端要认识「类型参数」，`is_safepoint`/帧布局/typedesc 全要改；且 M4 的 `A3` 仍然卡在前面 |

A 的额外好处：**`A3` 被绕开**。`A3` 的根因是「dump 的根集不产实例」，而 A 路线根本不依赖 dump —— 实例是 load 层显式造出来的。

#### `D-M12-3` 实例化闭包为什么从「非泛型根」出发

全程序实例化（把每个泛型 decl 的所有可能实例都造出来）需要枚举类型实参的组合，而 Go 的类型实参可以是任意类型 —— 不可判定。所以只能**按需**：从 `main`/`init` 出发，遇到 `F[int]` 就造 `F·int`，递归到不动点。

⚠️ 副作用：**没有被任何人调用的泛型函数不会产生实例**，因此「泛型函数体里的类型错误」不会被发现。这与官方行为不同（官方检查泛型原体）。**取舍**：M12 只做「可达实例」的检查，泛型原体的独立检查登记为 `U-M12-05`（见 §5）。语料里若出现「不可达但应报错」的用例，逐条登记。

#### `D-M12-5` 为什么旁挂而不是扩 `TypeDesc`

`TypeDesc` 是 **40 字节**、字段偏移由 `C5` §10 冻结，两侧各有 `_Static_assert` / `offset_of` 判据（M9 建的）。反射要 `Name/PkgPath/Field(i)/Method(i)/NumIn/NumOut/Key()` —— 直接扩会让每个类型都变胖，且要改一整套布局判据。

旁挂方案：`TypeDesc.pad`（现在空着的 `u16`）改语义为 `meta_idx`，`reflect` 侧按索引查只读表 `ReflectMeta[]`。**40 字节、每个偏移都不变**，M9 的判据一条都不用改。上限 65535 个类型，语料远够。

#### `D-M12-6` 为什么排除 `MakeFunc`

`Value.Call` 是「Go 代码调反射」，`MakeFunc` 是「反射调 Go 代码」——后者要一个**反向 trampoline**：把运行时打包的参数按目标函数的 ABI 摆好、再跳进去。它和 `Call` 共用 ABI 分类，但需要独立的入口桩与生命周期管理（闭包块 + 捕获）。语料里 `MakeFunc` 出现 0 次（`F7` 的 69 个门禁文件里 `reflect` 0 次）。**收益为零、风险翻倍**，明确排除。

#### `D-M12-8` 为什么 `errorcheck` 只保留 5 个、且要求**诊断码对齐**

`// errorcheck` 的语义是「这段代码应当报**某个特定**错误」。6 个里 **5 个无 `import`**（`F2`），它们的期望错是语言层面的，属于我们能负责的面 ⇒ 纳入。

**判据口径**：不比文本（`C8` 管渲染；`M11` 的 `T-GCORP-02` 已经踩过「断码变成测渲染格式」的坑），而是断言「**第一条诊断码 ∈ Task 0 逐条登记的集合**」——集合写在 `M12-tests.md` 里，改了要人看一眼（沿 `T-GDEV-03` 的「冻结行数」纪律）。这样「我们报了另一个错」会被抓到，「我们把消息改了措辞」不会误伤。

剩下 **1 个带 `import`** 的：我们会先报 `TYP-UNSUPPORTED-IMPORT-FORM` —— **拒绝理由不对**，把它当「拒绝面通过」就是假绿。这一个如实登记在 `U-M12-01` 里。

### 2.3 事实基线（`F1..F12`，全部实测）

| # | 事实 | 来源 |
|---|---|---|
| `F1` | `test/typeparam` 共 **266** 个文件，指令是**五族**：`// run` 142 / `// rundir` 49 / `// compile` 44 / `// compiledir` 18 / `// errorcheck` 8 / 无指令 5（六族互斥，和为 266）| `grep -lE '^// <族>( |$)'` |
| `F2` | 无 stdlib import 的：`// run` **71** / 142、`// compile` **40** / 44、`// errorcheck` **5** / 8；带 stdlib import 的合计 **76**；`// rundir`+`// compiledir` 的 **67** 个全是 `package ignored` 桩 | `m12-classify.sh`（注释感知的 import 提取）|
| `F3` | **今天**的读数：门禁 116 个 ⇒ `run` **3** / 71、`compile` **6** / 40、`errorcheck` **1** / 5（语义对）；拒绝面 **76/76** 干净拒绝 | `m12-baseline.sh` |
| `F4` | check 失败诊断码分布（门禁族 64 个）：`CHK-INVALID-INDEX` 47（其中 **121 处** `cannot index value of type func` = 显式实例化被当索引）、`CHK-INVALID-ASSIGN` 11、`CHK-INVALID-OPERATION` 2、`CHK-INVALID-BUILTIN-ARG` 2、`TYP-INVALID-BLANK` 1、`CHK-INVALID-RANGE-EXPR` 1 | 同上 |
| `F5` | check 过但 build 失败 38 个：`HIR-MISSING-TYPE` 12、`CHK-INVALID-SELECTOR` 11、`帧布局失败` 6、`VerifyError` 2、其它 7（6 个是 `build` 内置 check 又报错 + 1 个真 clang 链接失败 `issue52124`）| 同上 |
| `F6` | 六种形态探测：`Id[T any](x T)` + `Id(7)` = check ✅ / 解释器 ✅ / native ❌ verifier；`Id[int](7)` = `CHK-INVALID-INDEX`；`Box[T]{v:3}` + `b.v` = check ✅ 但 `run`/`build` 报 `CHK-INVALID-SELECTOR`；`[T comparable]` = check ✅ / 解释器 ✅ / native ❌；`[T Num]` + `+` = `CHK-INVALID-OPERATION`；`[]T` 形参 = `CHK-INVALID-ASSIGN` | 六份探针源码 |
| `F7` | 71 个门禁 `// run` 的特性分布：`any` 56+、`[T ` 55+、`interface` 54+、`func(` 18、`~` 8、`comparable` 8、`map[` 6、`chan` 2、`recover` 1、**`unsafe` 4（伪 import）、`reflect` 0** | `grep -c` 逐特性 |
| `F8` | `import "reflect"` ⇒ `TYP-UNSUPPORTED-IMPORT-FORM`（"no source directory under the import roots"）；`-I` 只存在于 `types` 子命令 | 实测 + `cli.rs:785` |
| `F9` | `TypeDesc` 40 字节（`C5` §10），`pad: u16` 现在空着 | `rt.c` / `rgoc-meta::desc` |
| `F10` | M7 已有 `abi_classify`（参数 ABI 分类）；`opt_equiv.rs` 是 pass 的唯一强判据 | M7/M10 交付 |
| `F11` | 容器内 `go version go1.27.1 linux/arm64` | 实测 |
| `F12` | 现有全量：**243 suite / 2281 passed / 0 failed / 2 ignored**；69 单线程三态各 69/0/0；7 并发三态各 7/0/0 | M11 收口读数 |
| `F13` | `// rundir`/`// compiledir` 的 67 个文件是 `package ignored` 桩；同名 `.dir/` 目录 69 个、用相对包 import（`import "./a"` 35 处）| Task 0 实测 |
| `F14` | **`check` 与 `build` 口径不一致 7 个**：6 个 `check` rc=0 而 `build` 的内置 check 报错（`TYP-INVALID-CONVERSION` 1 / `CHK-INVALID-OPERATION` 5），1 个是真 clang 链接失败 | Task 0 实测 |

> ⚠️ `F1/F2/F3/F7` 是 **Task 0 实测修正后**的值（设计第一版写的 141/41/6/78 与
> 111 是把「精确匹配 `^// run$`」当成了族划分）。修正记录见 `M12-baseline.md` §5。

### 2.4 已拍板的五件事（2026-10-28）

1. **三条线全做**（泛型 / 反射 / 优化），但**独立门禁、可各自切断**。
2. **泛型语料的「全过」= 116 个无 stdlib import 的文件**（71 `run` + 40 `compile` + 5 `errorcheck`）；76 个转拒绝面；67 个 `.dir` 桩与 5 个无指令不纳入。
3. **反射 = 全写面**（读 + `Call` + `Set*`/`Make*`/`New`），`MakeFunc` 排除。
4. **泛型 ABI = 先单态化、c 线换字典**（字典是重写，排最后、第一个可切断）。
5. **不碰 stdlib**（`fmt` 等 8 个包不做），`errorcheck` 族去掉。

---

## 3. 架构与数据模型

### 3.1 crate 与模块变化

| crate | 变化 |
|---|---|
| `rgoc-check` | 补四处缺口（§3.4）：显式实例化 / `[]T` 可赋值 / core type 运算 / 实例的字段与方法 |
| `rgoc-load` | **新增 `mono.rs`**：实例化闭包（§3.2）；`scope.rs` 与 `check` 的类型表达式解析**同口径**（这是已知的两份实现坑） |
| `rgoc-hir` | 不变（只见到具体类型） |
| `rgoc-ssa` / `rgoc-arm64` | 不变（a 线）；c 线改内联/逃逸/字典 |
| `rgoc-meta` | `TypeDesc.pad` → `meta_idx`；新增 `ReflectMeta` 的发射 |
| `runtime/.../rt.c` | 新增 `rt_reflect_*`（元数据查询 / `Call` 桥 / 写面） |
| 新目录 `stdlib/reflect/` | `reflect` 源码桩（`U-M12-01` 之外唯一的包） |

### 3.2 单态化：实例化闭包（`rgoc-load::mono`）

```text
输入：已声明、未解析体的包（decls-only）
  ↓ ① 根集 = 非泛型函数的体（main/init）+ 反射用到的类型
  ↓ ② 扫描体里的「泛型引用」：F[int](…)、Box[string]{…}、类型实参推断出的实例
  ↓ ③ 对每个新实例：把类型参数替换成实参，生成 mangled 名的普通 decl
  ↓ ④ 新实例的体可能又引用新的泛型 ⇒ 回到 ②（不动点）
输出：一张「实例 decl 列表」，与用户 decl 一起进 resolve_bodies
```

**实例命名**：`Sum·int`、`Box·string·Get`（`·` 不是合法 Go 标识符，不会与用户符号冲突）。名字**必须确定性**（同一程序两次编译产物逐字节相同，`T-GEN-05`）。

**类型替换**：`TypeId` 级的替换表 `{T → int, U → string}`，走已有的 `identical`/`assignable` 判约束满足（M4 已交付）。

**实例的类型身份**：每个实例生成自己的 `typedesc`（含 `meta_idx`），因此 `Box[int]` 与 `Box[string]` 在接口断言/反射里是**不同类型**（`T-REF-05`）。

### 3.3 与 M4 的 `A3` 的关系

`A3` 的症状是「dump 的根集不产实例」。本设计**不修 `A3`**，而是**绕开**：实例由 `mono` 显式生成，`dump` 的根集不再承担「产实例」的职责。`A3` 仍留在 M4 的 `open_gaps` 里（它影响 `types --cmp` 的对账面，不影响 native）。

### 3.4 检查面四处缺口（a 线的前置）

| 缺口 | 今天 | 改成 |
|---|---|---|
| 显式实例化 `F[int](x)` | `CHK-INVALID-INDEX`（把 func 当可索引） | 识别为实例化，走 `mono` |
| `[]T` 形参 | `[]int` 不能赋给 `[]T`（`CHK-INVALID-ASSIGN`） | 按**实例化后**的类型判可赋值 |
| core type 运算 | `~int` 上的 `+` ⇒ `CHK-INVALID-OPERATION` | 按 `TypeParam` 的 core type 判 |
| 实例的字段/方法 | `check` 过、`run`/`build` 报 `CHK-INVALID-SELECTOR`（load 面没实现） | **load 面与 check 面同口径**（同一个 `resolve_type`） |

⚠️ 第四条是记忆里那个「类型表达式 → `TypeId` 两份实现」的坑（`rgoc-load::typeexpr::resolve_type` + `rgoc-check::expr::type_expr`）。改一处必须改两处，**判据要两边都查**（`T-GEN-08`）。

### 3.5 反射：包形态与元数据旁挂

* `stdlib/reflect/reflect.go` 是**普通 Go 源码包**（用 `unsafe` + 内建 `rt_reflect_*`）。白名单 = 这个包的**导出符号**，`T-REF-01` 用脚本对账（包里的 `func`/`type` 与测试表一一对应）。
* 三个子命令（`check`/`run`/`build`）加 `-I`（现在只有 `types` 有，`F8`），这样 `stdlib/` 能进 import root。
* `ReflectMeta`（只读，`.rodata`）：`name` / `pkgpath` / `fields[]`（`{name, offset, tag, typ, anonymous}`）/ `methods[]`（`{name, sig, fn}`）/ `params[]`（`Call` 用的 ABI 类）。

### 3.6 反射：`Call` 的 ABI 桥

`rt_reflect_call(fn, meta, args, nargs, results)`：

1. 按 `meta->params[i].abi_class` 把每个实参放进 GPR / FPR / 栈槽（分类结果由 M7 的 `abi_classify` 预先算好写进元数据，**运行时不算**）；
2. 一小段 `__asm__` 摆好 `x0..x7` / `d0..d7` / 栈，`blr`；
3. 按返回值的 ABI 类收集结果。

**分档判据**（`T-REF-03`）：0 参数 / 1..8 整型 / 1..8 浮点 / 混合 / 超过 8 个（走栈）/ 聚合体按值 / 多返回值 / 零大小参数。

### 3.7 反射：写面与 STW 前提

* `Value.Set` / `SetInt` / …：`rt_memcpy(dst, src, size)`；`CanSet` = 可寻址且非只读。
* `New` / `MakeSlice` / `MakeMap` / `MakeChan`：走现有 `rt_alloc*`，遵守 `gc_pause` 约定（`C12` §1.3）。
* **不需要写屏障的前提**：M12 仍是 STW + 单线程，GC 只在分配点同步跑，`Set` 写指针不可能与标记并发。这条**必须写进 `C12`**（`D-M12-7`），否则将来上并发 GC 会静默出错。

### 3.8 优化四段

| 段 | 落点 | 判据 |
|---|---|---|
| 内联 | `rgoc-ssa` 新增 inline pass | `opt_equiv` + 69/67 三态不变 + `.s` 行数下降 |
| 精细逃逸 | `rgoc-ssa` 的逃逸分析（现在保守全堆分配） | `allocs` 下降 + 三态输出不变 |
| SwissTable | `rt.c` 的 `MapObj` 槽块 | `t_map_*` 全绿 + 大 map probe 读数 |
| 字典替换 | a 线的代码生成（重写） | 116 个门禁语料重跑 + 代码体积读数 |

---

## 4. 判据与装置

### 4.1 三层差分（沿 M11 的口径）

| 层 | 从哪来 | 覆盖 |
|---|---|---|
| **L1** native vs **解释器** | `--via=hir` | 全部 116 个门禁语料（**主判据**） |
| **L2** native vs **官方** | `go run`（go1.27.1，`F11`） | 69 个 `// run` |
| **L3** 解释器 vs 官方 | 同上 | 69 个 `// run` |

L1 仍是主判据：`C11` 已把 rgoc 的语义冻结，官方只用来证明「解释器那一侧没偏离 Go」。

### 4.2 语料与清单（全部**算出来**，不手抄）

| 清单 | 个数 | 用途 |
|---|---|---|
| `typeparam-run.txt` | 71 | 无 stdlib import 的 `// run`（三层一致）|
| `typeparam-compile.txt` | 40 | 无 stdlib import 的 `// compile`（编译通过）|
| `typeparam-errorcheck.txt` | 5 | 无 stdlib import、无 `-m` 的 `// errorcheck`（诊断码对齐）|
| `typeparam-errorcheck-m.txt` | 2 | 带 `-m`（`U-M12-08`，不做）|
| `typeparam-rejected.txt` | 76 | 带 stdlib import（干净拒绝 + 码集合稳定）|
| `typeparam-dir.txt` | 67 | `.dir` 桩（`U-M12-07`，不做）|
| `typeparam-none.txt` | 5 | 无指令（不做）|

`T-M12-01` 断言：**七份**之和 = 266、两两不相交，且每份都由 `tools/run-oracle/m12-classify.sh` 从语料算出（不手抄）。

### 4.3 判据表（正文在 `M12-tests.md`）

| 族 | 覆盖 |
|---|---|
| `T-GEN-01..08` | 三层一致（69）/ 编译通过（37）/ 拒绝面（77）+ 诊断码对齐（5）/ 实例名确定 / `.s` 无类型参数残留 / 泛型×并发三态 / 不回归 / 两份实现同口径 |
| `T-REF-01..06` | 白名单对账 / 元数据逐字段 / `Call` ABI 分档 / 写面生效 / 泛型实例的类型身份 / 反射×GC |
| `T-OPT-01..04` | 内联 / 精细逃逸 / SwissTable / 字典替换 |
| `T-M12-01..03` | 清单划分 / 三层装置自检 / 分档读数可复现 |

### 4.4 变异（每线三条，进 `scripts/mutation-m12.sh`）

| # | 变异 | 期望红 |
|---|---|---|
| `M-M12-01` | `mono` 只产一层实例（不递归到不动点） | `T-GEN-01` |
| `M-M12-02` | 实例名不带类型实参（`Sum·int` 与 `Sum·string` 撞名） | `T-GEN-04` |
| `M-M12-03` | core type 运算不查类型集 | `T-GEN-01` |
| `M-M12-04` | `ReflectMeta` 的字段偏移错 8 字节 | `T-REF-02` |
| `M-M12-05` | `Call` 的浮点参数塞进 GPR | `T-REF-03` |
| `M-M12-06` | `Set` 不检查 `CanSet` | `T-REF-04` |
| `M-M12-07` | 内联跨 panic 边界 | `T-OPT-01` |
| `M-M12-08` | 逃逸分析把逃逸对象判成不逃逸 | `T-OPT-02` |
| `M-M12-09` | SwissTable 探测序列少一步 | `T-OPT-03` |

### 4.5 预算与资源

* 每条线 **4–8 人周**、最长 **12 周**，独立计窗（不逐子阶段重复领取）。
* **超限切断顺序**：SwissTable → 内联/逃逸的预算目标（保留正确性）→ 字典替换（退回单态化）。
* **不可放宽**：`G12-1/2/3`（116 个门禁语料 + 76 拒绝面）、反射白名单每个符号有判据、四条 cargo 门禁。

---

## 5. 不支持面（白名单）

| ID | 不做 | 判据/登记 |
|---|---|---|
| `U-M12-01` | stdlib import（`fmt`/`strings`/`strconv`/`sort`/`math`/`errors`/`sync`/`runtime`） | **76** 个文件 ⇒ 拒绝面（`T-GEN-03`）|
| `U-M12-02` | `reflect.MakeFunc` | `T-REF-01` 的白名单里没有它 |
| `U-M12-03` | 可变参数 `Call`（`CallSlice`、`...any` 实参） | `T-REF-03` 的分档里没有它 |
| `U-M12-04` | PGO / DWARF / coverage | roadmap 已划延期 |
| `U-M12-05` | **泛型原体的独立检查**（不可达实例的类型错误不报） | `D-M12-3` 的副作用，如实登记 |
| `U-M12-06` | 并发 GC / 写屏障 | 沿用 `U-M11-08`；`D-M12-7` 的前提写进 `C12` |
| `U-M12-07` | `.dir` 目录测试（相对包 import）| 67 个桩 + 69 个目录，不在任何门禁里 |
| `U-M12-08` | `errorcheck -m`（内联诊断）| 2 个文件，不在门禁里 |

---

## 6. 错误处理

* **实例化失败**（约束不满足）：诊断码沿用 M4 的 `CHK-*`，**不新增码**；报点用实例化**现场**的位置（不是泛型声明处），并在消息里带类型实参。
* **递归实例化无底洞**（`F[F[F[...]]]`）：超限报 **`TYP-INVALID-INSTANCE-CYCLE`** —— 该码 **M4 已建**（`rgoc-lex/src/diag.rs`），深度上限用它自己的 `MAX_INSTANTIATION_DEPTH`。**不新增码**（Task 0 修正）。
* **`reflect` 误用**：`CanSet` 为假时 panic（文本对齐官方），走既有 `rt_panic_*`。
* **`Call` 的 ABI 不匹配**：编译期就拒绝（白名单外的形态不给过），不在运行时猜。

---

## 7. 覆盖清单（roadmap §M12 ↔ 功能 ID）

| roadmap 条目 | 功能 ID | 归属 |
|---|---|---|
| 泛型 native 单态化 | `RT-POLY-05` / `PIPE-11` / `TYP-24` / `EXP-15` | M12a |
| 泛型闭包 / method value | `RT-POLY-07` | M12a |
| interface / type switch / 类型身份 | `RT-POLY-01/02/04` | M12a（身份）+ M12b（反射） |
| 反射元数据 | `RT-POLY-06` | M12b |
| 内联 | `PIPE-12` | M12c |
| 精细逃逸 | `PIPE-13` | M12c |
| SwissTable | `RT-CMP-*`（map 实现） | M12c |
| shape/dictionary 共享 | `RT-POLY-05`（第二阶段） | M12c（可切断） |

---

## 8. 风险登记

| # | 风险 | 依据 | 应对 |
|---|---|---|---|
| 1 | **M4 检查面是成片缺口** | M4 报告：75 个入选里真实对齐只有 9 个；`typeparams` 67/105 行 diff | 档 1–2 的预算按「检查面为主」估；`Task 0` 的归因表决定档序 |
| 2 | `Call` 的 ABI 桥是唯一裸 ABI | 无现成实现 | 复用 `abi_classify`，分类预写进元数据；先做 0..8 参数的整数/浮点，聚合体最后 |
| 3 | 字典替换是**重写 a 线** | `D-M12-4` | 排最后 + 第一个可切断项 |
| 4 | 三条线全做超预算 | roadmap 上限 12–24 人周；按本次最高档实际可能 30+ | **分段独立发布**：a 收口即发布 |
| 5 | `mono` 与 `check` 的类型表达式解析再次漂移 | 已有前科（两份实现） | `T-GEN-08` 两边都查 |

---

## 9. 任务分解原则（`M12-plan.md` 的输入）

1. **Task 0 先做基线测绘**：116 个门禁语料 + 76 个拒绝面各跑一遍，逐条归因成「检查面 / 单态化 / 反射 / 范围外」四类，产出 `M12-baseline.md`。**没有这张表不排后面的任务顺序。**
2. 每条线内部按「先判据后实现」：先写会红的判据（今天 `T-GEN-01` 就该红），再实现。
3. 每个 Task 2–5 分钟粒度、带可粘贴内容与验证命令（沿 M10/M11 的格式）。
4. 阶段收口即 commit + push；每段收口跑四条门禁 + 两套语料三态 + 变异。
