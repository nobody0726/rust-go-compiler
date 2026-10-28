# M12 实施计划 · 泛型 native / 有限反射 / 可选优化

> **For agentic workers:** 按 Task 顺序执行，每个 Step 勾一个。阶段收口即 commit + push。
> 步骤用 `- [ ]` 跟踪。

**Goal:** 让 native 后端跑通 Go 泛型（116 个官方语料的门禁面），再做有限反射与可选优化。

**Architecture:** 单态化在 `rgoc-load` + `rgoc-check` 的「实例化闭包」里完成 —— 实例以
mangled 名的**普通具体 decl** 进 HIR，SSA/isel/GC 看不到任何类型参数（`D-M12-2`）。
反射走「源码桩包 + `TypeDesc.pad` 改 `meta_idx` 旁挂 `ReflectMeta`」（`D-M12-5`）。
优化三段独立、字典替换排最后且第一个可切断（`D-M12-4`）。

**Tech Stack:** Rust workspace（`lex → ast → parse → const → types → load → check → hir →
interp → ssa → arm64 → driver`）+ aarch64 freestanding runtime（`rt.c`）+ 容器内
`go1.27.1` 当第三层 oracle。

**Spec:** [`M12-design.md`](./M12-design.md)（决策 `D-M12-1..10` / 基线 `F1..F14`）/
判据 [`M12-tests.md`](./M12-tests.md) / 实测 [`M12-baseline.md`](./M12-baseline.md)。

## Global Constraints

- **门禁面 = 116 个文件**：`typeparam-run.txt` 71 + `typeparam-compile.txt` 40 +
  `typeparam-errorcheck.txt` 5。**拒绝面 = 76**（码集合 ⊇ `TYP-UNSUPPORTED-IMPORT-FORM`）。
- **不做**：stdlib import（`U-M12-01`）/ `reflect.MakeFunc`（`U-M12-02`）/ 可变参数 `Call`
  （`U-M12-03`）/ PGO·DWARF·coverage（`U-M12-04`）/ 泛型原体独立检查（`U-M12-05`）/
  并发 GC·写屏障（`U-M12-06`）/ `.dir` 目录测试（`U-M12-07`）/ `errorcheck -m`
  （`U-M12-08`）。
- **`TypeDesc` 仍是 40 字节**、每个偏移不变（`C5` §10）；M9 的 `t_desc_*` 判据**不许改**。
- **HIR 以下看不到泛型**：`.s` 与 `--dump=hir` 里 `type parameter` 出现 0 次。
- **实例名确定性**：同一程序两次编译产物逐字节相同；名字含 `·`（`Sum·int`）。
- 四条 cargo 门禁每阶段收口必跑：`fmt --check` / `check --workspace --all-targets` /
  `clippy --workspace --all-targets -- -D warnings` / `test --workspace --no-fail-fast`。
- 全部命令在容器内跑：`RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && …'`
  （语料在 `/work/go_source_code/`，**只读**）。
- 读数**只准脚本算**，不准手抄；`.superpowers/` 已 gitignore（台账不入库）。

## Review Focus

按「最可能咬到人」排序，每条都要有判据钉在对应 Task 上：

1. **递归实例化**（`F[G[F[int]]]`、自引用约束）—— 应当收敛或报
   `TYP-INVALID-INSTANCE-CYCLE`，不能栈溢出/死循环。→ Task 14 的 `t_gen_14`。
2. **泛型函数作为值**（`F[int]` 只当实参传、不调用）—— 应当产出函数值（指针 + 实例），
   不能当成「索引表达式」或「未定义」。→ Task 6 的 `t_gen_06b`。
3. **零大小类型实参**（`struct{}`、`[0]byte`）—— 实例化、typedesc、`Call` 都不能把
   `size == 0` 当错误。→ Task 13 的 `t_gen_13b` + Task 33 的 `Call` 零大小档。
4. **实例装进 `any` 再取回**（`Box[int]` → `any` → 断言回 `Box[int]`）—— 类型身份必须
   能区分 `Box[int]` 与 `Box[string]`。→ Task 16 的 `t_gen_16b` + `T-REF-05`。
5. **`reflect.Call` 的浮点/混合/多返回** —— 最容易把参数塞错寄存器。→ Task 33 的 9 档。

---

## Phase 0 · 契约与装置

### Task 0: 基线测绘 —— ✅ 已完成（2026-10-28）

> **读数**：`M12-baseline.md` + 两个装置脚本。门禁面 **116**（71+40+5）、拒绝面 **76**、
> 今天 `run` **3/71** / `compile` **6/40** / `errorcheck` **1/5** / 拒绝面 **76/76**。
> 归因：检查面 64（`F[int](x)` 被当索引占 47）、单态化 38（选择器 11 / `missing_type` 12 /
> 布局 6 / verify 2 / 其它 7）、反射 0、范围外 150。
>
> 六处对设计的修正已写进 `M12-baseline.md` §5（指令是五族不是三类、门禁 111→116、
> 清单五份→七份、`TYP-INVALID-INSTANCE-CYCLE` 已存在、拒绝面判据改成「码集合包含」、
> 新增 `F13/F14` 与 `U-M12-07/08`）。

- [x] **Step 1**: `tools/run-oracle/m12-classify.sh`（七份清单，注释感知的 import 提取）。
- [x] **Step 2**: `tools/run-oracle/m12-baseline.sh`（逐文件 TSV + 归因汇总）。
- [x] **Step 3**: `docs/milestones/M12-baseline.md`（读数 + 归因 + 六处修正）。
- [x] **Step 4: Commit** `docs(m12): 基线测绘（Task 0）`

### Task 1: `C13-generics.md` 契约骨架

**Files:**
- Create: `docs/contracts/C13-generics.md`

**Interfaces:**
- Produces: 契约编号 `C13`；`§1 实例与命名`、`§2 类型身份`、`§3 检查面规则`、
  `§4 诊断码`、`§5 反射元数据`、`§6 不做面`。

- [ ] **Step 1**: 写 `§1`：实例命名规则 `Origin·Arg1·Arg2`（`·` = U+00B7，不是合法 Go
  标识符）；同一 `(origin, 实参组)` 只有一个实例（memo 口径按 `identical`）。
- [ ] **Step 2**: 写 `§2`：每个实例有自己的 `typedesc`；`Box[int]` ≠ `Box[string]`；
  接口断言的比较口径是 typedesc 指针相等。
- [ ] **Step 3**: 写 `§3`：显式实例化 `F[int](x)` 的判定顺序（先试类型实参，再退化成索引）；
  `[]T` 的可赋值按实例化后判；core type 运算的判据。
- [ ] **Step 4**: 写 `§4`：只用已有码 `TYP-WRONG-TYPE-ARG-COUNT` / `TYP-INVALID-TYPE-ARG` /
  `TYP-CANNOT-INFER-TYPE-ARGS` / `TYP-INVALID-INSTANCE-CYCLE`；**不新增码**。
- [ ] **Step 5**: 写 `§5/§6`：`ReflectMeta` 的字段表 + 八条不做面（`U-M12-01..08`）。
- [ ] **Step 6: Commit** `docs(m12): C13 契约骨架（Task 1）`

### Task 2: `T-M12-01` 七份清单自洽

**Files:**
- Create: `rgoc/crates/rgoc-driver/tests/m12_corpus.rs`
- Test: 同上（`t_m12_01_七份清单自洽`）

**Interfaces:**
- Consumes: `tools/run-oracle/m12-classify.sh --check`（退出 0 = 自洽）
- Produces: `t_m12_01_七份清单自洽`；后续 Task 复用 `m12_lists()` 读清单。

- [ ] **Step 1**: 写测试：调用脚本 `--check`，断言退出 0；再**自己**算一遍七份之和 ==
  `ls *.go | wc -l` == 266（不信任脚本的自报）。
- [ ] **Step 2**: 跑测试 —— 期望 **PASS**（脚本已存在）。再手工把一份清单删掉一行，
  期望 **FAIL** 且报「七份之和 ≠ 266」（证明判据有牙）。
- [ ] **Step 3: Commit** `test(m12): 七份清单自洽（Task 2）`

### Task 3: 三层差分装置 `run-native-typeparam.sh`

**Files:**
- Create: `tools/run-oracle/run-native-typeparam.sh`
- Test: `rgoc/crates/rgoc-driver/tests/m12_corpus.rs`（`t_m12_02_装置打的是真靶`）

**Interfaces:**
- Produces: `run-native-typeparam.sh [--list] [--filter PAT] [--gc=off|on|stress]`；
  汇总行 `ok/a+b/A+B`（有输出/空输出两栏）；退出码 0 = 全绿。
- Consumes: `typeparam-run.txt` / `typeparam-compile.txt` / `typeparam-errorcheck.txt` /
  `typeparam-rejected.txt`。

- [ ] **Step 1**: 抄 `run-native-chan.sh` 的骨架（slug 命名、产物落 `.superpowers/m12-oracle/`、
  语料只读、不用 `!` 取 rc）。
- [ ] **Step 2**: `run` 族：`build --emit=elf` → 跑产物 → 与解释器 `--via=hir`、官方
  `go run` 三样分开比（stdout / stderr / 退出码）。
- [ ] **Step 3**: `compile` 族：只断言 `build` 退出 0 + `readelf -h` 的 `Machine == AArch64`。
- [ ] **Step 4**: `errorcheck` 族：断言退出 1 + 首码 ∈ `M12-tests.md` §1 的目标集合。
  `rejected` 族：断言退出 1 + 码集合含 `TYP-UNSUPPORTED-IMPORT-FORM`。
- [ ] **Step 5**: `t_m12_02`：拿 `absdiff.go`（已知红）必须报 BAD、`issue47708.go`（已知绿）
  必须报 OK —— **只查「装置能跑」等于没查**。
- [ ] **Step 6**: 跑 `--gc=off` 记下读数，应复现 `M12-baseline.md` §2（run 3/71、
  compile 6/40、errorcheck 1/5、rejected 76/76）。
- [ ] **Step 7: Commit** `test(m12): 三层差分装置（Task 3）`

### Task 4: Phase 0 收口

- [ ] **Step 1**: 四条门禁 + 既有三套语料（69 单线程 / 7 并发）零回归。
- [ ] **Step 2: Commit + push** `chore(m12): Phase 0 收口（Task 4）`

---

## Phase 1 · 检查面（64 个 check 失败 → 0）

### Task 5: `T-GEN-08a` 先红（check 与 build 口径不一致）

**Files:**
- Test: `rgoc/crates/rgoc-driver/tests/m12_checkface.rs`（`t_gen_08a_口径一致`）

**Interfaces:**
- Produces: `check_verdict(path) -> (i32, Vec<&str>)` 与 `build_verdict(path) -> (i32, Vec<&str>)`
  两个辅助函数（后续 Task 复用）。

- [ ] **Step 1**: 写测试：遍历 116 个门禁文件，断言「`check` 的结论」与「`build` 的前置
  check 结论」相同（要么都过、要么都报错）。今天是 **7 个不一致**（`F14`）。
- [ ] **Step 2**: 跑 —— 期望 **FAIL**，失败信息里列出那 7 个文件名。
- [ ] **Step 3: Commit** `test(m12): check/build 口径一致（Task 5，先红）`

### Task 6: 显式实例化 `F[int](x)`（47 个文件）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/expr.rs:691`（`index_expr`）
- Modify: `rgoc/crates/rgoc-check/src/expr.rs`（`call_expr` 的显式类型实参路径）
- Test: `rgoc/crates/rgoc-driver/tests/m12_checkface.rs`（`t_gen_06_显式实例化`、`t_gen_06b_实例当值传`）

**Interfaces:**
- Consumes: `rgoc_types::infer::infer_for_call(tt, callee_tparams, callee_targs, callee_params,
  args, generic_args)`、`rgoc_types::instantiate::instantiate(tt, origin, args)`
- Produces: `index_expr` 在「base 是泛型函数/泛型类型 + index 是类型表达式」时返回
  实例化后的 `Operand`；实例请求进 `TypeTable` 的 pending 队列（Task 14 消费）。

- [ ] **Step 1**: 写测试 `t_gen_06`：6 个探针（`Id[int](7)`、`Box[int]{v:3}`、`F[T]` 作为
  实参传、`[]T` 上的 `len`、`map[K]V` 的显式实例化、嵌套 `F[G[int]]`）—— 断言 `check` 退出 0。
- [ ] **Step 2**: 跑 —— 期望 **FAIL**（今天全是 `CHK-INVALID-INDEX`）。
- [ ] **Step 3**: 改 `index_expr`：先判「base 是不是泛型 decl」，是则把 `e.index` 当**类型
  实参**解析（复用 `expr::type_expr`），走 `instantiate`/`infer_for_call`；**不是**才落回
  索引的既有分支。
- [ ] **Step 4**: `t_gen_06b`：`F[int]` **只当值传不调用**（`var g func(int) int = F[int]`）——
  断言 `check` 退出 0（这是 Review Focus 第 2 条）。
- [ ] **Step 5**: 跑 `m12-baseline.sh --only run --only compile`：`CHK-INVALID-INDEX` 从 47
  降到 ≤ 6（剩下 6 处是真索引 `x[i]` 且 `x` 是 `[]T`，归 Task 7）。
- [ ] **Step 6: Commit** `feat(check): 显式实例化识别（Task 6）`

### Task 7: 实例化后的可赋值（11 个文件）

**Files:**
- Modify: `rgoc/crates/rgoc-types/src/assignable.rs`（`TypeParam` 分支）
- Modify: `rgoc/crates/rgoc-check/src/expr.rs`（实参检查处传实例化后的形参类型）
- Test: `m12_checkface.rs`（`t_gen_07_可赋值`）

- [ ] **Step 1**: 写测试：`[]int` 传给 `[]T`、`func(int)` 传给 `func(T)`、`Box[int]` 传给
  `Box[T]` 三种形态 —— 断言 `check` 退出 0。
- [ ] **Step 2**: 跑 —— 期望 **FAIL**（`CHK-INVALID-ASSIGN`，11 个文件里 31 处）。
- [ ] **Step 3**: 在「实参 vs 形参」比较前把形参按当前实例的 `SubstMap` 替换
  （`rgoc_types::subst::substitute`），再走既有 `assignable`。
- [ ] **Step 4**: 跑基线：`CHK-INVALID-ASSIGN` 从 11 降到 0；`x[i]` 那 6 处真索引
  （`cannot index value of type type parameter`）也要过 —— 按 core type 取 `[]T` 的元素类型。
- [ ] **Step 5: Commit** `feat(check): 实例化后的可赋值（Task 7）`

### Task 8: core type 运算（2 个文件）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/expr.rs`（二元运算的类型判定）
- Test: `m12_checkface.rs`（`t_gen_08_core_type`）

- [ ] **Step 1**: 写测试：`[T ~int|~float64]` 上的 `+`、`[T ~int]` 上的 `<`、混合类型集上的
  `+`（应拒）—— 前两个退出 0、第三个退出 1 且码为 `CHK-INVALID-OPERATION`。
- [ ] **Step 2**: 跑 —— 期望 **FAIL**（`absdiff.go` / `issue48424.go` 今天报
  `CHK-INVALID-OPERATION`）。
- [ ] **Step 3**: 按 `Type::TypeParam` 的**类型集**取 core type（`rgoc_types::typeset`），
  再判运算符；类型集里没有共同的 core type 时仍然拒。
- [ ] **Step 4**: 跑基线：`CHK-INVALID-OPERATION` 从 2 降到 0；`M-M12-03`（不查类型集）
  的靶子在这里（Task 25 用）。
- [ ] **Step 5: Commit** `feat(check): core type 运算（Task 8）`

### Task 9: 类型参数上的内建（2 个文件）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/expr.rs`（`append` / `len` / `cap` / `make` 的分支）
- Test: `m12_checkface.rs`（`t_gen_09_内建`）

- [ ] **Step 1**: 写测试：`append(xs, x)`（`xs []T`）、`len(xs)`、`cap(m)`（`m map[K]V`）——
  断言退出 0。
- [ ] **Step 2**: 跑 —— 期望 **FAIL**（`append.go` / `builtins.go`：`CHK-INVALID-BUILTIN-ARG`）。
- [ ] **Step 3**: 内建参数先按 core type 归一（`[]T` → `[]elem`），再走既有判据。
- [ ] **Step 4**: 跑基线：`CHK-INVALID-BUILTIN-ARG` 从 2 降到 0。
- [ ] **Step 5: Commit** `feat(check): 类型参数上的内建（Task 9）`

### Task 10: 边角两个（`TYP-INVALID-BLANK` / `CHK-INVALID-RANGE-EXPR`）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/decl.rs`（类型参数声明里的 `_`）
- Modify: `rgoc/crates/rgoc-check/src/stmt.rs`（`range` 的表达式类型）
- Test: `m12_checkface.rs`（`t_gen_10_边角`）

- [ ] **Step 1**: 看 `issue47710.go` 与 `issue48453.go` 的实际期望（`// ERROR` 行），把两个
  目标码写进测试。
- [ ] **Step 2**: 跑 —— 期望 **FAIL**。
- [ ] **Step 3**: 按目标码修（`_` 作为类型参数名是合法的；`range` 允许类型参数）。
- [ ] **Step 4**: 跑基线：这两个码在门禁面清零。
- [ ] **Step 5: Commit** `feat(check): 类型参数的两个边角（Task 10）`

### Task 11: `T-GEN-08a` 转绿（口径统一）

**Files:**
- Modify: `rgoc/crates/rgoc-driver/src/build_cmd.rs:80-90`（`load_with_imports` 后的 check 入口）
- Test: `m12_checkface.rs`（`t_gen_08a_口径一致`）

- [ ] **Step 1**: 定位那 7 个不一致的**输入差异**（`check` 子命令与 `build` 用的
  `CheckOptions` / `lang` / import roots 是否不同）。先把差异写成注释留在 `build_cmd.rs`。
- [ ] **Step 2**: 让 `build` 与 `check` 走**同一个** `check_package_with` 调用（差异只能来自
  `Emit` 之后的阶段）。`issue52124.go` 的 clang 链接失败单独修（Task 18）。
- [ ] **Step 3**: 跑 `t_gen_08a` —— 期望 **PASS**（6 个口径不一致消失）。
- [ ] **Step 4**: 跑既有 69 语料三态，确认零回归（口径统一可能让某些文件从「过」变「拒」，
  若发生，逐条记录原因）。
- [ ] **Step 5: Commit** `fix(driver): check 与 build 同口径（Task 11）`

### Task 12: Phase 1 收口

- [ ] **Step 1**: 跑基线：门禁 116 个里 `check` 失败应从 64 降到 **0**（build 面还有 38 个）。
- [ ] **Step 2**: 四条门禁 + 69/7 语料零回归。
- [ ] **Step 3: Commit + push** `chore(m12): Phase 1 收口（Task 12）`

---

## Phase 2 · 单态化（38 个 build 失败 → 0）

### Task 13: `rgoc-load::mono` 骨架 + 实例命名

**Files:**
- Create: `rgoc/crates/rgoc-load/src/mono.rs`
- Modify: `rgoc/crates/rgoc-load/src/lib.rs`（`pub mod mono;`）
- Test: `rgoc/crates/rgoc-load/tests/mono.rs`（`t_gen_04_实例名`）

**Interfaces:**
- Produces:
  ```rust
  /// 泛型函数的身份：`rgoc-load` 里 `FuncDecl` 的下标（**不是** `TypeId` ——
  /// 函数没有类型条目，只有签名）。
  pub type FuncKey = usize;
  /// 一个待实例化的请求（同一 (origin, args) 只入队一次）。
  pub struct InstRequest { pub origin: FuncKey, pub args: Vec<TypeId>, pub name: String }
  /// 实例名：`Sum·int`、`Box·string`（`·` = U+00B7）。
  pub fn mangled(tt: &TypeTable, origin_name: &str, args: &[TypeId]) -> String;
  /// 请求入队（memo 命中则返回 false）。
  pub fn request(&mut self, tt: &mut TypeTable, origin: FuncKey, args: &[TypeId]) -> bool;
  pub fn take(&mut self) -> Option<InstRequest>;
  ```
- Consumes: `TypeTable::named_def` / `lookup_instance`（`rgoc-types/src/table.rs:424`）

- [ ] **Step 1**: 写测试 `t_gen_04_实例名`：`mangled` 对 `(Sum, [int])` 产出 `Sum·int`、
  对 `(Box, [string])` 产出 `Box·string`；两个不同实参**不同名**；名字里没有 `_`/`$`
  （避免与用户符号或汇编器冲突）。
- [ ] **Step 2**: 跑 —— 期望 **FAIL**（模块不存在）。
- [ ] **Step 3**: 实现 `mono.rs`（只做命名 + 队列，不做代码生成）。
- [ ] **Step 4**: `t_gen_13b`：零大小类型实参（`F[struct{}]`）的 mangled 名**唯一**且可读
  （Review Focus 第 3 条）。
- [ ] **Step 5: Commit** `feat(load): mono 骨架与实例命名（Task 13）`

### Task 14: 实例化闭包（不动点）

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/check.rs:442`（`check_package_with` 在 `f` 之前跑循环）
- Modify: `rgoc/crates/rgoc-check/src/check.rs`（`Session::check_instance`）
- Modify: `rgoc/crates/rgoc-hir/src/lib.rs`（`lower` 消费 `CheckedPackage.instances`）
- Test: `m12_checkface.rs`（`t_gen_14_不动点`、`t_gen_14b_递归实例`）

**Interfaces:**
- Consumes: `mono::{request, take, mangled}`
- Produces: `CheckedPackage.instances: Vec<InstanceBody>`（`{name, origin, subst, hir}`）；
  `rgoc_hir::lower` 把这些也下沉成 `Func`。

- [ ] **Step 1**: 写测试 `t_gen_14_不动点`：`main` 调 `A[int]`，`A` 的体里调 `B[string]`，
  `B` 的体里调 `A[int]` —— 断言只产 **2 个**实例（`A·int` / `B·string`），且不无限循环。
- [ ] **Step 2**: 跑 —— 期望 **FAIL**（今天一个实例都不产，`F5` 的 `t32 没有运行期布局`）。
- [ ] **Step 3**: 在 `check_package_with` 里加循环：`while let Some(req) = session.take_inst()`
  → `check_instance(req)`（用 `SubstMap` 把类型参数换成实参后走既有 decl 检查）。
- [ ] **Step 4**: `t_gen_14b_递归实例`：`F[G[F[int]]]` 与自引用约束 —— 断言要么收敛、
  要么报 `TYP-INVALID-INSTANCE-CYCLE`（Review Focus 第 1 条）。
- [ ] **Step 5**: 跑基线：`build` 失败从 38 降，`layout` / `verify` / `missing_type` 三类应显著减少。
- [ ] **Step 6: Commit** `feat(check): 实例化闭包到不动点（Task 14）`

### Task 15: 实例的字段与方法选择器（11 个）

**Files:**
- Modify: `rgoc/crates/rgoc-load/src/typeexpr.rs`（`resolve_type` 的实例分支）
- Modify: `rgoc/crates/rgoc-check/src/expr.rs`（`selector_expr` 的实例分支）
- Test: `m12_checkface.rs`（`t_gen_15_选择器`、`t_gen_08b_两份实现同口径`）

- [ ] **Step 1**: 写测试：`Box[int]{v:3}.v`、`b.Get()`、嵌套实例 `Pair[int,string].First` ——
  断言 `check` 与 `build` **都**退出 0。
- [ ] **Step 2**: 跑 —— 期望 **FAIL**（11 个文件报 `CHK-INVALID-SELECTOR`，`check` 却过）。
- [ ] **Step 3**: 让 load 的 `resolve_type` 与 check 的 `type_expr` 对 `Type::TypeParam` 与
  实例走**同一套**判定（这是记忆里那个「两份实现」的坑）。
- [ ] **Step 4**: `t_gen_08b`：用源码片段**正反向**查两处都处理了 `TypeParam`（改一处不
  改另一处要红）。
- [ ] **Step 5**: 跑基线：`selector` 从 11 降到 0。
- [ ] **Step 6: Commit** `feat: 实例的字段与方法选择器（Task 15）`

### Task 16: `HIR-MISSING-TYPE`（12 个）+ 类型身份

**Files:**
- Modify: `rgoc/crates/rgoc-check/src/expr.rs`（`invalid` 操作数的产出点）
- Modify: `rgoc/crates/rgoc-hir/src/lower.rs`（实例的静态类型）
- Test: `m12_checkface.rs`（`t_gen_16_missing_type`、`t_gen_16b_类型身份`）

- [ ] **Step 1**: 写测试：12 个文件逐个 `build` 退出 0。
- [ ] **Step 2**: 跑 —— 期望 **FAIL**（`拿不到表达式 X 的静态类型（mode=invalid operand）`）。
- [ ] **Step 3**: 根因是 check 静默产出 `invalid` 操作数（`D-M5-2` 的失效条件）。让「操作数
  为 invalid」在**没有诊断**时变成硬错（否则 HIR 拿到 invalid 只能猜）。
- [ ] **Step 4**: `t_gen_16b`：`Box[int]` 与 `Box[string]` 的 typedesc **不同**
  （`--dump=types` 里两个不同的 `TypeId`），且 `Box[int]` 装进 `any` 再断言回来成功
  （Review Focus 第 4 条）。
- [ ] **Step 5**: 跑基线：`missing_type` 从 12 降到 0。
- [ ] **Step 6: Commit** `feat: 实例的静态类型与类型身份（Task 16）`

### Task 17: 帧布局 6 个 + `VerifyError` 2 个

**Files:**
- Modify: `rgoc/crates/rgoc-ssa/src/layout.rs`（或 `isel` 的帧布局入口）
- Modify: `rgoc/crates/rgoc-ssa/src/verify.rs`（常量类型的判定）
- Test: `m12_checkface.rs`（`t_gen_17_布局与验证`）

- [ ] **Step 1**: 写测试：8 个文件逐个 `build` 退出 0。
- [ ] **Step 2**: 跑 —— 期望 **FAIL**（`帧布局失败：v0 的类型 t32 没有运行期布局` /
  `VerifyError: 常量 Int(7) 不可能有类型 TypeId(32)`）。
- [ ] **Step 3**: 两处的共同根因是「SSA 拿到带类型参数的类型/常量」。单态化之后如果仍出现，
  说明**某条路径没走实例**（记录具体路径，可能是 `Composite` 或方法值）。
- [ ] **Step 4**: 跑基线：`layout` 与 `verify` 清零。
- [ ] **Step 5: Commit** `fix: 单态化后的帧布局与验证（Task 17）`

### Task 18: `issue52124.go` 的 clang 链接失败

**Files:**
- Modify: 视根因（`rgoc/crates/rgoc-arm64/src/emit.rs` 或 `rt.c`）
- Test: `m12_checkface.rs`（`t_gen_18_链接`）

- [ ] **Step 1**: 看 `issue52124.go` 的链接错误原文（`clang 链接失败：` 之后的符号名），
  写测试断言该文件 `build` 退出 0。
- [ ] **Step 2**: 跑 —— 期望 **FAIL**。
- [ ] **Step 3**: 按符号名定位（缺符号 / 重复符号 / 未定义的外部引用），修。
- [ ] **Step 4**: 跑基线：门禁 116 个的 `build` 失败降到 0。
- [ ] **Step 5: Commit** `fix: issue52124 的链接失败（Task 18）`

### Task 19: `T-GEN-05` 无类型参数残留

**Files:**
- Test: `m12_checkface.rs`（`t_gen_05_无残留`）

- [ ] **Step 1**: 写测试：116 个门禁文件逐个 `--emit=asm` + `--dump=hir`，断言
  `grep -c 'type parameter'` == 0；**阴性对照**用 `issue47708.go`（今天就能跑）。
- [ ] **Step 2**: 跑 —— 期望 **PASS**（若红，说明还有路径没走实例化）。
- [ ] **Step 3: Commit** `test(m12): 无类型参数残留（Task 19）`

---

## Phase 3 · a 线收口

### Task 20: `T-GEN-02` 40 个 `compile`

- [ ] **Step 1**: 跑 compile 族，把失败清单落 `.superpowers/m12a/compile-fails.txt`
  （`--list` 的 BAD 行），逐条修到 **40/40**。
- [ ] **Step 2**: 每个修掉的失败在 commit message 里写一句根因。
- [ ] **Step 3: Commit** `feat(m12a): compile 门禁 40/40（Task 20）`

### Task 21: `T-GEN-01` 71 个 `run` 三层一致

- [ ] **Step 1**: 跑 run 族，失败清单落 `.superpowers/m12a/run-fails.txt`，记 `ok/a+b` 读数。
- [ ] **Step 2**: 逐条修到 **71/71**；L1（解释器）为主判据，L2/L3 只证明解释器没偏离。
- [ ] **Step 3**: 空输出的文件单独记一栏（`M12-tests.md` §0.2）。
- [ ] **Step 4: Commit** `feat(m12a): run 门禁 71/71 三层一致（Task 21）`

### Task 22: `T-GEN-03b` 5 个 `errorcheck`

- [ ] **Step 1**: `tparam1` 已绿；`issue48711` 要落 `TYP-INVALID-INSTANCE-CYCLE`、
  `issue51232/51233` 要落 `TYP-WRONG-TYPE-ARG-COUNT`、`issue50317` 登记为阶段不同。
- [ ] **Step 2**: 每修一个，跑一次 `run-native-typeparam.sh --filter <file>`。
- [ ] **Step 3: Commit** `feat(m12a): errorcheck 诊断码对齐 5/5（Task 22）`

### Task 23: `T-GEN-06` 泛型 × 并发三态

- [ ] **Step 1**: 门禁里 `chan` 的 2 个 + 新写 2 个 fixture（泛型 + `go` / 泛型 + `chan`）。
- [ ] **Step 2**: 三态（off/on/stress）stdout + 退出码逐字节相同；`stress` 断言
  `collections > 0`（否则判据恒真）。
- [ ] **Step 3: Commit** `test(m12a): 泛型 × 并发三态（Task 23）`

### Task 24: `T-GEN-07` 不回归

- [ ] **Step 1**: 69 单线程三态各 69/0/0；7 并发三态各 7/7/0（含 `M'` 12/12）。
- [ ] **Step 2**: **拒绝面 76/76 仍成立**（`T-GEN-03a`）—— 单态化可能让某些带 import 的
  文件在 import 报错**之前**先过检查，那样拒绝理由就变了。
- [ ] **Step 3**: `cargo test --workspace` 的 `failed == 0`，`passed` ≥ 阶段开始快照 + 新增条数。
- [ ] **Step 4: Commit**（并入 Task 25）

### Task 25: 变异 `M-M12-01..03`

**Files:**
- Create: `scripts/mutation-m12.sh`（抄 `mutation-m11.sh` 的 `PROBE`/`PROBE_RS` 骨架）

- [ ] **Step 1**: `M-M12-01`（只产一层实例）→ `T-GEN-01` 红。
- [ ] **Step 2**: `M-M12-02`（实例名不带实参）→ `T-GEN-04` 红。
- [ ] **Step 3**: `M-M12-03`（core type 不查类型集）→ `T-GEN-01` 红。
- [ ] **Step 4**: `--strict` 跑，记 `RED/GREEN/PENDING/MISS`。
- [ ] **Step 5: Commit** `test(m12a): 变异三条（Task 25）`

### Task 26: `M12-manifest.json` + 对账

- [ ] **Step 1**: 门禁 `G12-1..6` 逐条证据 + `measured`（116/71/40/5/76 + 三态读数）。
- [ ] **Step 2**: `rgoc/crates/rgoc-driver/tests/manifest_m12.rs`：结构自洽 + **三态必须能
  从一次真实跑复现** + 合成反例逐个指名道姓报错。
- [ ] **Step 3: Commit** `docs(m12): manifest + 对账（Task 26）`

### Task 27: a 线报告

- [ ] **Step 1**: `M12-report.md` 的 a 节：读数 / 真错清单（按「为什么先前没抓到」分类）/
  判据的牙 / 接手指南。
- [ ] **Step 2: Commit** `docs(m12): a 线报告（Task 27）`

---

## Phase 4 · b 线（有限反射）

### Task 28: 三个子命令加 `-I`

**Files:**
- Modify: `rgoc/crates/rgoc-driver/src/cli.rs:775-815`（`roots` 构造）
- Test: `rgoc/crates/rgoc-driver/tests/m12_reflect.rs`（`t_ref_00_import_root`）

- [ ] **Step 1**: 写测试：`-I <tmp>/root` 里放一个 `fmt/fmt.go` 桩，`check -I` 退出 0
  （今天只有 `types` 有 `-I`）。
- [ ] **Step 2**: 跑 —— 期望 **FAIL**（`unknown argument`）。
- [ ] **Step 3**: `check`/`run`/`build` 都接受 `-I`，透传到 `load_with_imports` 的 roots。
- [ ] **Step 4: Commit** `feat(driver): check/run/build 支持 -I（Task 28）`

### Task 29: `stdlib/reflect` 骨架 + `T-REF-01`

**Files:**
- Create: `stdlib/reflect/reflect.go`
- Test: `m12_reflect.rs`（`t_ref_01_白名单对账`）

- [ ] **Step 1**: 写最小包：`type Type interface{ Kind() Kind; Size() uintptr; … }`、
  `func TypeOf(any) Type`、`type Value struct{…}`、`func ValueOf(any) Value`。内部调
  `rt_reflect_*`（先 `todo` 未实现的）。
- [ ] **Step 2**: `t_ref_01`：从 `reflect.go` 读导出符号集合，与测试里登记的集合比；
  断言 `MakeFunc` **不在**里面。
- [ ] **Step 3**: 跑 —— 期望 **FAIL**（包还没有），实现后 **PASS**。
- [ ] **Step 4: Commit** `feat(reflect): 源码桩骨架 + 白名单对账（Task 29）`

### Task 30: `meta_idx` + `ReflectMeta` 发射

**Files:**
- Modify: `rgoc/crates/rgoc-meta/src/desc.rs`（`pad: u16` → `meta_idx: u16`）
- Modify: `rgoc/crates/rgoc-arm64/src/emit.rs`（发 `ReflectMeta[]` 到 `.rodata`）
- Modify: `runtime/native/aarch64-unknown-linux-gnu/rt.c`（`TypeDesc` 注释 + `rt_reflect_*` 读表）
- Test: `m12_reflect.rs`（`t_ref_02_元数据逐字段`）

**Interfaces:**
- Produces: `ReflectMeta { name: &str, pkgpath: &str, fields: &[FieldMeta], methods: &[MethodMeta] }`；
  `TypeDesc.pad` 的语义改为 `meta_idx`（0 = 没有元数据）。

- [ ] **Step 1**: 写测试：20 个类型（含 `Box[int]`）的 `Size/Align/Kind/NumField/Field(i)`
  与官方 `go run` 输出逐字段相同。
- [ ] **Step 2**: 跑 —— 期望 **FAIL**。
- [ ] **Step 3**: 改 `pad` 语义 + 发 `ReflectMeta`；**断言 `size_of::<TypeDesc>() == 40`**
  （M9 的判据不许改，这里只加一条新的守卫）。
- [ ] **Step 4: Commit** `feat(meta): ReflectMeta 旁挂（Task 30）`

### Task 31: 读面 API

- [ ] **Step 1**: 实现 `Kind/Size/Align/Name/PkgPath/Field/FieldByName/NumField/Tag/Len/Key/Elem`。
- [ ] **Step 2**: 跑 `t_ref_02` 到绿。
- [ ] **Step 3: Commit** `feat(reflect): 读面 API（Task 31）`

### Task 32: 方法表 + 类型身份（`T-REF-05`）

- [ ] **Step 1**: `NumMethod/Method/Implements/AssignableTo/ConvertibleTo`。
- [ ] **Step 2**: `t_ref_05`：`TypeOf(Box[int]{}).String() == "main.Box[int]"`、与
  `Box[string]` 不等、`Implements` 在实例与接口之间与官方同判。
- [ ] **Step 3: Commit** `feat(reflect): 方法表与类型身份（Task 32）`

### Task 33: `Call` 的 ABI 桥（`T-REF-03`）

**Files:**
- Modify: `runtime/native/aarch64-unknown-linux-gnu/rt.c`（`rt_reflect_call`）
- Modify: `rgoc/crates/rgoc-meta/src/desc.rs`（`params[]` 的 ABI 类）

- [ ] **Step 1**: 把 M7 的 `abi_classify` 结果写进 `ReflectMeta.params`（**运行时不算**）。
- [ ] **Step 2**: 实现 `rt_reflect_call(fn, meta, args, nargs, results)`：GPR/FPR/栈三路摆放
  + `blr` + 收集返回值。
- [ ] **Step 3**: `t_ref_03` 的 9 档（含零大小参数、多返回值、浮点混合）。
- [ ] **Step 4**: 跑 —— 逐档修到绿。
- [ ] **Step 5: Commit** `feat(reflect): Call 的 ABI 桥（Task 33）`

### Task 34: 写面（`T-REF-04`）

- [ ] **Step 1**: `Set/SetInt/SetString/SetBool/New/MakeSlice/MakeMap/MakeChan`。
- [ ] **Step 2**: `CanSet` 为假时 panic，文本对齐官方。
- [ ] **Step 3**: `t_ref_04` 每条 API 一个断言。
- [ ] **Step 4: Commit** `feat(reflect): 写面（Task 34）`

### Task 35: 反射 × GC（`T-REF-06`）+ 变异 `M-M12-04..06`

- [ ] **Step 1**: `t_ref_06`：`reflect.New` 的对象在 `stress` 下存活；`Set` 写进去的指针
  是根（写后 churn 再读回）；断言 `collections > 0`。
- [ ] **Step 2**: `C12` 补一段「写面不需要屏障的前提是 STW + 单线程」（`D-M12-7`）。
- [ ] **Step 3**: 变异三条跑红。
- [ ] **Step 4: Commit** `test(m12b): 反射 × GC + 变异（Task 35）`

### Task 36: b 线收口

- [ ] **Step 1**: `G12-7..9` 全绿；manifest / report 补 b 节。
- [ ] **Step 2: Commit + push** `chore(m12b): b 线收口（Task 36）`

---

## Phase 5 · c 线（优化，可切断）

### Task 37: 内联（`T-OPT-01`）

- [ ] **Step 1**: `rgoc-ssa` 新增 inline pass（单层 + 深度上限），`opt_equiv.rs` 当唯一强判据。
- [ ] **Step 2**: 三态输出不变；`.s` 行数下降读数进 manifest。
- [ ] **Step 3: Commit** `feat(ssa): 内联（Task 37）`

### Task 38: 精细逃逸（`T-OPT-02`）

- [ ] **Step 1**: 不逃逸的 `&x` 留栈（与 `C12` §4.5 的逃逸槽交互）。
- [ ] **Step 2**: `allocs` 下降 + 三态不变；`.s` 里该槽不再 `bl rt_alloc_escaped`。
- [ ] **Step 3: Commit** `feat(ssa): 精细逃逸（Task 38）`

### Task 39: SwissTable（`T-OPT-03`）

- [ ] **Step 1**: `MapObj` 槽块换 SwissTable 探测。
- [ ] **Step 2**: `t_map_*` 全绿 + 10 万键 probe 读数。
- [ ] **Step 3: Commit** `feat(rt): SwissTable（Task 39）`

### Task 40: 字典替换（`T-OPT-04`，**第一个可切断**）

- [ ] **Step 1**: 116 个门禁语料重跑（单态化的判据全部重来一遍）。
- [ ] **Step 2**: 体积读数（`.text` 字节数）下降。
- [ ] **Step 3**: 超预算就**退回单态化**，把结论写进 `M12-report.md`。
- [ ] **Step 4: Commit** `feat: 字典替换（Task 40）`

### Task 41: 变异 `M-M12-07..09`

- [ ] **Step 1**: 内联跨 panic 边界 / 逃逸判错 / SwissTable 少一步，三条各自跑红。
- [ ] **Step 2: Commit**（并入 Task 42）

### Task 42: c 线收口

- [ ] **Step 1**: `G12-10..13` 逐条读数；切断顺序按 `M12-design.md` §4.5。
- [ ] **Step 2: Commit + push** `chore(m12c): c 线收口（Task 42）`

---

## Phase 6 · M12 收口

### Task 43: 全量回归与文档

- [ ] **Step 1**: 四条门禁 + 三套语料（69 单线程 / 7 并发 / 116 泛型）+ 变异全红。
- [ ] **Step 2**: `M12-manifest.json` 定稿（13 条门禁 + 8 条白名单 + measured）。
- [ ] **Step 3**: `M12-report.md` 定稿；`GAP-01.md` 加 M12 节；`README` / `AGENTS.md` /
  `03-roadmap.md` 回写；`C13` 定稿。
- [ ] **Step 4**: 项目记忆 `MEMORY.md` 加「M12 硬约束」一节。
- [ ] **Step 5: Commit + push** `chore(m12): 收口（Task 43）`
