# M3 实施计划：常量、类型核心与最小包模型

> **状态**：**已完成**（T-TY-01..T-TY-38 全落地，2026-10-17）　|　**测试清单**：`M3-tests.md`（已冻结验收）　|　**机器可读事实**：[`M3-manifest.json`](./M3-manifest.json)
>
> 全部 38 个任务的 Step 复选框已补勾（Task 1–35 的提交序列与 Ruling 见 `.superpowers/sdd/M3-plan/progress.md`；Task 36 落 `65ba170`、Task 37 落 `59405f1`、Task 38 落收口提交）。**无 deferred Step**；八条门禁 `TY-1`..`TY-8` 全部 `pass`，workspace **1282 条测试全绿**（含 Task 38 的 manifest 对账测试）。
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `rgoc` 能对一组 `.go` 文件建立作用域与类型表示，并对「两个类型是否同一类型」和「常量是否可表示」给出与 Go 规范及官方 `go/types` 一致的判定。

**Architecture:** 三个新 crate 单向依赖 —— `rgoc-const`（任意精度大数 + 常量折叠 + 可表示性）→ `rgoc-types`（`Type` 表示 + universe + `Identical` + 按文件的版本门控）→ `rgoc-load`（单包多文件 + 源码 import + 导出摘要）。验收靠「自有 golden + `tools/types-oracle`（标准库 `go/types`）逐字节对撞」两条腿。

**Tech Stack:** Rust 1.98.1 / edition 2024 / resolver 3；workspace 在 `rgoc/`；全部命令经 `scripts/in-container.sh`（宿主不构建）。oracle 是 Go 程序，用容器内的 go1.27.1。

**Spec:** [`docs/milestones/M3-design.md`](./M3-design.md)（设计，已确认）＋ [`docs/milestones/M3-tests.md`](./M3-tests.md)（测试先行清单，已冻结）。**执行者必须同时读这两份** —— 本计划只决定「顺序与接口」，判据以 tests 清单为准。

## Global Constraints

- **只在容器内构建/测试**：一切 `cargo`/`go` 命令经 `scripts/in-container.sh <cmd>`；宿主不装工具链。
- **不默认新增第三方依赖**（`M3-design.md` `D-M3-12`）：大数与有理数**自研**；oracle 只用 Go 标准库。
- **`go_source_code/` 只读**，不得修改、不得回写。
- **`Const` 永远精确**：舍入结果作返回值，不写回 `Const`（`D-M3-16`）。
- **`rgoc-const` 不依赖 `rgoc-types`**：`Representable` 只吃 `BasicId` + `Sizes`（`D-M3-16`）。
- **用户可见的未实现 ⇒ 稳定 `DiagCode` + `unsupported` 登记**；内部不可达才 `todo!`（`D-M3-11`）。
- **`DiagCode` 五件套**：枚举按**字典序**落位 + `ALL` 数组 + `as_str` + **两个总数测试**（`rgoc-lex` 既有纪律）。
- **类型表示一次到位**：`Type` 含 `TypeParam`/`Union`/`~T`/实例化具名类型；**判定只做 `Identical` + `Representable`**（`D-M3-2`）。
- **版本按文件**存储；来源优先级 `-lang` > 文件 `//go:build`（`max(v, go1.21)`）> 回落（`D-M3-15`）。
- **资源上限**：类型递归深度 **1000**；import 深度 **100**；常量**不设**人为上限。
- 每个任务结束都**提交**；提交信息引用任务 ID 与测试 ID。

## Review Focus

以下是 `M3-design.md` 隐含、但**没有哪个任务的测试天然覆盖**的五类输入/失效模式，最可能的排前面。每条都已把测试钉到拥有该代码的任务上：

1. **含 `~T` 或并集的接口**（`interface{ ~int }`）：合理期望是「与另一个约束接口类型相等」被**判定**出来。M3 的答案是**显式拒绝**而非给个错的（`T-TY-81`，Task 19）。
2. **超长常量字面量**（`1<<1000`）：合理期望是精确值，不是被悄悄截断成某个近似值（`T-TY-31`，Task 4/10）。
3. **浮点常量恰好落在舍入中点**（如 `float32(1<<24 + 1)`）：合理期望是**向偶数**舍入，不是向零或四舍五入（`T-TY-44`，Task 4/10）。
4. **带 `//go:build` 排除标记的文件被显式列进包**：合理期望是「M3 不评估构建约束 ⇒ 照样装载」，不是静默丢弃（`T-TY-65`，Task 28）。
5. **未知 `//go:` 指令**（如 `//go:totally-made-up`）：合理期望是前端**不**报错（与官方 syntax 一致）（`T-TY-84`，Task 36）。

---

## Phase 0：契约与骨架

### Task 1: C7 类型 dump 契约

**Files:**
- Create: `docs/contracts/C7-type-dump.md`
- Create: `docs/contracts/testdata/type-dump-min.want`
- Modify: `docs/README.md`（契约清单登记）

**Interfaces:**
- Consumes: `docs/milestones/M3-tests.md` §5.2（已冻结的格式规则）
- Produces: 契约文件；格式口径（行式 / 2 空格缩进 / `named` 用 `#序号` / 无位置 / `-` 表空）供 Task 16、Task 31 两侧共同实现

- [x] **Step 1: 写契约文件**，把 `M3-tests.md` §5.2 的规则表**原样**搬成独立契约，补：标签全集（15 个）、`<序号>\t<dump>` 的行格式、注释/空行规则、UTF-8/LF/无 BOM/末尾单换行
- [x] **Step 2: 写最小 fixture** `type-dump-min.want`（一条 `basic` + 一条 `pointer`），作为两侧实现的第一对撞目标
- [x] **Step 3: 登记** `docs/README.md` 的契约清单（C7；标注「M3/Task 1 新建，已冻结」）
- [x] **Step 4: 校验文档自检** —— 运行 `python3 scripts/check-m0-consistency.py`；期望 exit 0（若它断言契约**数量**，同步更新该断言与 `AGENTS.md` §2.1）
- [x] **Step 5: 提交**

```bash
git add docs/contracts/C7-type-dump.md docs/contracts/testdata/type-dump-min.want docs/README.md
git commit -m "docs(m3): 新建并冻结 C7 类型 dump 契约（T-TY-11..19 的对撞面）"
```

### Task 2: 三个 crate 骨架 + `rgoc types` 子命令

**Files:**
- Create: `rgoc/crates/rgoc-const/{Cargo.toml,src/lib.rs}`
- Create: `rgoc/crates/rgoc-types/{Cargo.toml,src/lib.rs}`
- Create: `rgoc/crates/rgoc-load/{Cargo.toml,src/lib.rs}`
- Modify: `rgoc/Cargo.toml`（`members` 登记三个 crate；workspace 依赖表）
- Modify: `rgoc/crates/rgoc-driver/src/cli.rs`、`src/main.rs`
- Test: `rgoc/crates/rgoc-driver/tests/test_driver.rs`

**Interfaces:**
- Produces: `rgoc_types::{Type, TypeTable, BasicId, Sizes}`（空壳）、`rgoc_const::{Const, BigInt, BigRat}`（空壳）、`rgoc_load::load_package(files: &[PathBuf], lang: Option<&str>) -> Result<Package, DiagBag>`（空壳）；子命令 `rgoc types <files…>`（未实现时**报错非 panic**）
- Consumes: `rgoc-lex`（`Pos`/`DiagBag`/`DiagCode`）、`rgoc-parse`（`AstFile`）

- [x] **Step 1: 写失败测试** —— `tests/test_driver.rs::types_subcommand_rejects_unknown_when_unimplemented`：断言 `rgoc types` 在未实现时**退出码非 0 且 stderr 有稳定文案**（不是 panic、不是 0）
- [x] **Step 2: 运行确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-driver types_subcommand -v`
Expected: FAIL（`types` 子命令不存在 / 编译期未登记）

- [x] **Step 3: 实现骨架** —— 三个 `Cargo.toml`（`edition.workspace = true`、`resolver` 继承）；三个 `lib.rs` 只放空类型与 `// M3` 注释；`cli.rs` 增加 `types` 分支，调用 `rgoc_load::load_package`，当前直接返回「未实现」诊断（`D-M3-11` 的路径）
- [x] **Step 4: 运行新测试 + 全量** —— `scripts/in-container.sh cargo test -p rgoc-driver --no-fail-fast`；期望 PASS 且既有 23 条不回归
- [x] **Step 5: 提交**

```bash
git add rgoc/Cargo.toml rgoc/crates/rgoc-const rgoc/crates/rgoc-types rgoc/crates/rgoc-load rgoc/crates/rgoc-driver
git commit -m "feat(m3): 三个新 crate 骨架 + rgoc types 子命令占位"
```

### Task 3: `tools/types-oracle` 骨架与对撞入口

**Files:**
- Create: `tools/types-oracle/{go.mod,main.go}`
- Create: `tools/types-oracle/run-collide.sh`
- Create: `tools/types-oracle/testdata/`（先放 `type-dump-min` 的输入侧）

**Interfaces:**
- Consumes: `docs/contracts/C7-type-dump.md`（Task 1）
- Produces: `run-collide.sh <fixture-dir> [--lang go1.N]` —— 退出码 0 表示两侧 dump 逐字节一致；非 0 打印首个不一致行

- [x] **Step 1: 写失败测试** —— `run-collide.sh` 对 `type-dump-min` 期望**失败**（rgoc 侧还没实现 dump）并打印一侧输出；脚本自身 exit code 语义正确
- [x] **Step 2: 运行确认失败**

Run: `scripts/in-container.sh bash tools/types-oracle/run-collide.sh tools/types-oracle/testdata`
Expected: 非 0；输出指出 rgoc 侧尚未产出 dump

- [x] **Step 3: 实现骨架** —— `main.go` 用 `go/parser`（`parser.ParseComments`）读入文件，`go/types.Config{GoVersion: ...}.Check` 检查，按 C7 §5.2 打印 dump；`run-collide.sh` 依次跑 oracle 与 `rgoc types --dump`，`diff` 两份输出
- [x] **Step 4: 验证** —— 手动跑泛型 fixture，确认 `Config.GoVersion` 生效；确认 `go build -buildvcs=false` 在容器内可过
- [x] **Step 5: 提交**

```bash
git add tools/types-oracle
git commit -m "feat(m3): tools/types-oracle 骨架（go/parser + go/types）+ run-collide.sh"
```

---

## Phase 1：常量与大数（`rgoc-const`）

### Task 4: `BigInt` / `BigRat` 与向偶数舍入

**Files:**
- Create: `rgoc/crates/rgoc-const/src/bigint.rs`、`src/bigrat.rs`
- Test: 两文件的 `#[cfg(test)] mod tests`

**Interfaces:**
- Produces（`rgoc_const`）：
  - `pub struct BigInt`（base 2^32 limbs + 符号；`from_dec/from_hex/from_oct/from_bin`、`add/sub/mul/div_mod/shl/shr/cmp/is_neg/abs`、`to_i64()->Option<i64>`）
  - `pub struct BigRat`（`num: BigInt, den: BigInt` 恒规范化，`den>0`；`add/sub/mul/div/cmp`；`round_to_nearest_even(&self, prec: u32, emin: i32, emax: i32) -> Option<RoundResult>`）
  - `pub enum RoundResult { Exact, Rounded, Overflow }`
- Consumes: 无外部依赖（自研，`D-M3-12`）

- [x] **Step 1: 写失败测试**（`T-TY-31` / `T-TY-33` 的底座）
  - `BigInt::from_dec("1267650600228229401496703205376") == BigInt::from(1) << 100`（`2^100`）
  - `(BigInt::from(1) << 1000).to_string().len() == 302`
  - `BigRat`：`0.1 != 1/10` 的**十进制**近似断言（`BigRat::from_dec("0.1") == BigRat::new(1,10)`）
  - 舍入：`round_to_nearest_even` 对 `float32`（prec 24, emin -126, emax 127）在 `2^24+1` 上必须**进位到偶数**（`16777216`，不是 `16777218`）
- [x] **Step 2: 运行确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-const bigint bigrat -- -v`
Expected: FAIL（类型未定义）

- [x] **Step 3: 实现** —— `BigInt` 用 `Vec<u32>` 小端 limbs + 符号位；乘除用小学算法（不做 Karatsuba，YAGNI）；`BigRat` 每次运算后 `gcd` 约分；舍入实现 IEEE-754 round-half-to-even（**先算精确有理数**，再判是否落在界内、再定舍入方向）
- [x] **Step 4: 运行确认通过**

Run: `scripts/in-container.sh cargo test -p rgoc-const --no-fail-fast`
Expected: PASS；`clippy` 无 warning

- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-const/src/bigint.rs rgoc/crates/rgoc-const/src/bigrat.rs
git commit -m "feat(m3): 自研 BigInt/BigRat + 向偶数舍入（T-TY-31/33/44 底座）"
```

### Task 5: `Const` 表示与字面量构造

**Files:**
- Create: `rgoc/crates/rgoc-const/src/value.rs`
- Modify: `rgoc/crates/rgoc-const/src/lib.rs`
- Test: `src/value.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Produces（`rgoc_const`）：
  - `pub enum Const { Int(BigInt), Float(BigRat), Complex{re:BigRat,im:BigRat}, Str(Vec<u8>), Bool(bool), Unknown }`
  - `pub fn const_from_literal(lit: &rgoc_ast::BasicLit) -> Const` —— 十进制/`0x`/`0o`/`0b`/下划线/rune/转义/原始字符串
- Consumes: `rgoc_ast::BasicLit`（`rgoc-parse` 已产）

- [x] **Step 1: 写失败测试**（`T-TY-32`、`T-TY-3a`）
  - `1_000_000` / `0x_FF` / `0o17` / `0b1010` 都精确
  - `'x'` → `Const::Int(120)`；`'\u00e9'` → `233`
  - `` `a\nb` ``（原始字符串）→ 字节含**反斜杠与 n 两个字**，不是换行
  - `"a" + "b"`（拼接断言归 Task 6，这里先断言字面量字节）
- [x] **Step 2: 运行确认失败**

Run: `scripts/in-container.sh cargo test -p rgoc-const const_from_literal -v`
Expected: FAIL（`Const` 未定义）

- [x] **Step 3: 实现** —— 进制与下划线规则照 spec `spec:L633-739`；`Str` 装 `Vec<u8>`（沿用 `D-M1-12`，非法 UTF-8 字节不得被 `from_utf8_lossy` 吞掉）
- [x] **Step 4: 运行确认通过** —— `scripts/in-container.sh cargo test -p rgoc-const --no-fail-fast`
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-const/src/value.rs rgoc/crates/rgoc-const/src/lib.rs
git commit -m "feat(m3): Const 表示 + 字面量构造（T-TY-32/3a）"
```

### Task 6: 常量折叠：算术 / 比较 / 逻辑

**Files:**
- Create: `rgoc/crates/rgoc-const/src/fold.rs`
- Test: `src/fold.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Produces: `pub fn fold_binary(op: rgoc_ast::BinOp, x: &Const, y: &Const) -> Const`、`pub fn fold_unary(op: rgoc_ast::UnOp, x: &Const) -> Const`、`pub fn fold_compare(op: rgoc_ast::CmpOp, x: &Const, y: &Const) -> Const`
- Consumes: Task 4 的 `BigInt`/`BigRat`、Task 5 的 `Const`

- [x] **Step 1: 写失败测试**（`T-TY-35`、`T-TY-36`）
  - `7 / 2 == 3`（整数截断）；`-7 % 2 == -1`（`%` 取被除数符号）
  - `1.5 * 2 == 3`（有理数精确，不是 `3.0000000000000004`）
  - `1e400 * 1e400` 仍精确（**不**溢出为 `+Inf`）
  - 比较 `1 < 2.0`、逻辑 `true && false`
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh cargo test -p rgoc-const fold -v` → FAIL
- [x] **Step 3: 实现** —— 逐算子按 spec `spec:L5757-5877`；**除零**（整数 `%0`、`/0`）返回 `Const::Unknown` 并由求值器（Task 12）转诊断
- [x] **Step 4: 运行确认通过** —— `scripts/in-container.sh cargo test -p rgoc-const --no-fail-fast`
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-const/src/fold.rs
git commit -m "feat(m3): 常量折叠——算术/比较/逻辑（T-TY-35/36）"
```

### Task 7: 移位：无符号计数与非负检查

**Files:**
- Modify: `rgoc/crates/rgoc-const/src/fold.rs`
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Produces: `fold.rs` 中 `Shl`/`Shr` 分支；`pub fn shift_ok(x: &Const) -> Result<(), DiagCode>`

- [x] **Step 1: 写失败测试**（`T-TY-37`、`CON-08`）
  - `1 << 100` 精确（不是 0、不是 panic）
  - **常量移位计数视为无符号**：`1 << -1` 报错（不是右移）
  - 左值非负检查：`(-1) << 2` 报错
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh cargo test -p rgoc-const shift -v`
- [x] **Step 3: 实现** —— 照 `spec:L4814-4885` / `spec:L8721-8742`：计数走 `to_u64`，负值直接拒绝；左值负数在**非 typed** 语境拒绝
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-const/src/fold.rs
git commit -m "feat(m3): 常量移位语义 + 非负检查（T-TY-37，CON-08）"
```

### Task 8: 常量转换折叠与默认类型

**Files:**
- Create: `rgoc/crates/rgoc-const/src/convert.rs`
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Produces: `pub fn untyped_default_type(c: &Const) -> BasicId`；`pub fn convert_const(c: &Const, to: BasicId, sizes: &Sizes) -> Result<Const, DiagCode>`
- Consumes: Task 10 的 `Sizes`（本任务先接收 `&Sizes` 参数，`Sizes` 定义在 Task 14）

- [x] **Step 1: 写失败测试**（`T-TY-38`、`T-TY-3b`）
  - 默认类型：`Int→int`、`Float→float64`、`Complex→complex128`、`Str→string`、`Bool→bool`、rune 字面量 `'a'` → `Int` 默认 `rune`
  - 转换：`int→float64` 精确；`float64(1)/3` **不可精确表示于 float64** ⇒ `convert_const` 仍给精确有理数（可表示性判定归 Task 10）
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh cargo test -p rgoc-const convert -v`
- [x] **Step 3: 实现** —— 转换**不丢失精度**：目标是「把精确值表示为另一个精确值」，**不**在此处舍入（舍入只发生在 Task 10）
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-const/src/convert.rs
git commit -m "feat(m3): 常量转换 + 无类型默认类型（T-TY-38/3b，CON-03）"
```

### Task 9: iota 与常量声明组

**Files:**
- Create: `rgoc/crates/rgoc-const/src/iota.rs`
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Produces: `pub fn expand_const_group(decls: &[rgoc_ast::ConstDecl]) -> Vec<(usize, rgoc_ast::Expr)>`（返回 `(iota 值, 表达式)` 对，含「省略则重复上一表达式」的展开）
- Consumes: `rgoc_ast` 的 `ConstDecl`/`Expr`

- [x] **Step 1: 写失败测试**（`T-TY-39`、`CON-06/07`）
  - `const ( A = iota; B; C )` ⇒ `0,1,2`
  - `const ( A = 1 << iota; B; C )` ⇒ `1,2,4`
  - **空行/注释不重置 iota**；**新的 `const (` 组重置为 0**
  - `_` 也占一个 iota 槽位
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh cargo test -p rgoc-const iota -v`
- [x] **Step 3: 实现** —— 照 `spec:L2400-2451` / `spec:L2331-2398`；表达式省略时**复制上一行的表达式**（含 iota 引用，故必须在展开时求值）
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-const/src/iota.rs
git commit -m "feat(m3): iota 与常量声明组展开（T-TY-39，CON-06/07）"
```

### Task 10: `Representable` 与 `Sizes`

**Files:**
- Create: `rgoc/crates/rgoc-const/src/representable.rs`
- Create: `rgoc/crates/rgoc-const/src/sizes.rs`
- Test: `src/representable.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Produces（`rgoc_const`）：
  - `pub struct Sizes { pub int: u32, pub uint: u32, pub uintptr: u32 }` + `pub const HOST_ARM64: Sizes`（8/8/8）
  - `pub fn representable(c: &Const, to: BasicId, sizes: &Sizes) -> Result<RoundResult, DiagCode>`
- Consumes: Task 4 的舍入、Task 5 的 `Const`、Task 14 的 `BasicId`

- [x] **Step 1: 写失败测试**（`T-TY-41..46`、`T-TY-48`）
  - 边界：`int8` 收 `-128`/`127` 过、`128`/`-129` 不过；`uint64` 收 `2^64-1` 过、`2^64` 不过
  - `byte`/`rune` 等价于 `uint8`/`int32`
  - `float32(1<<24)` 过；`float32(1<<24+1)` **过但记 `Rounded`**（向偶数）；`float32(1<<128)` 不过
  - `1<<100` → `int8` ⇒ `Err(不可表示)`
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh cargo test -p rgoc-const representable -v`
- [x] **Step 3: 实现** —— 判定顺序照 `types2/const.go:76`（`F7`/`F14`）：先分类（int/uint/float/complex/string/bool），整型比范围，浮点用 `round_to_nearest_even` 判 `Exact`/`Rounded`/`Overflow`，复数实虚部分别判。**不改 `Const`**
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-const/src/representable.rs rgoc/crates/rgoc-const/src/sizes.rs
git commit -m "feat(m3): Representable + Sizes（T-TY-41..48，CON-04）"
```

### Task 11: 常量诊断码（五件套）

**Files:**
- Modify: `rgoc/crates/rgoc-lex/src/diag.rs`（或 `DiagCode` 所在文件，按现状）
- Test: 该文件的五件套测试（字典序 + `ALL` + `as_str` + 两个总数测试）

**Interfaces:**
- Produces: 新 `DiagCode` 变体：`ConstNotRepresentable`、`ConstOverflow`、`ShiftCountNegative`、`NotAConstantExpression`（命名与最终集合由本任务定，须进 `as_str`）
- Consumes: `rgoc-lex` 既有五件套结构

- [x] **Step 1: 写失败测试** —— 断言 `DiagCode::ALL` 包含新码；断言「枚举声明序 == 字典序」；断言总数测试的新期望值（两个：变体数、`as_str` 覆盖数）
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh cargo test -p rgoc-lex diag -v`
- [x] **Step 3: 实现** —— 新码**按字典序插入**声明位置（不是追加到末尾）；同步 `ALL`、`as_str`、两个总数
- [x] **Step 4: 运行确认通过** + 反向验证（删掉一个变体的 `as_str` 分支 ⇒ 总数测试必须红）
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-lex/src/diag.rs
git commit -m "feat(m3): 常量类 DiagCode（五件套：字典序 + ALL + as_str + 两个总数）"
```

### Task 12: 常量表达式求值器（常量子集）+ 非常量诊断

**Files:**
- Create: `rgoc/crates/rgoc-const/src/eval.rs`
- Test: `tests/eval_const.rs`

**Interfaces:**
- Produces: `pub enum EvalOutcome { Const(Const), NotAConstant }`；`pub fn eval_const_expr(e: &rgoc_ast::Expr, env: &ConstEnv, sizes: &Sizes) -> (EvalOutcome, DiagBag)`；`pub trait ConstEnv { fn lookup_const(&self, ident: &str) -> Option<&Const>; }`
- Consumes: Task 5/6/7/8 的 `Const` 与折叠算子；`rgoc_ast::Expr`

- [x] **Step 1: 写失败测试**（`T-TY-83` 正向 + 反向）
  - 支持：字面量、**引用到常量**的标识符、`+ - * / % << >> & | ^ &^`、比较、逻辑、转换、括号
  - 反向（**不**报错）：`const A = 1 + 2*3`、`const B = A << 4`
  - 正向（**报错** `NotAConstantExpression`）：`const C = f()`、`const D = x`（`x` 是变量）
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh cargo test -p rgoc-const --test eval_const -v` → FAIL
- [x] **Step 3: 实现** —— 只走**常量**路径：任何需要类型信息/运行期值的子表达式 ⇒ `NotAConstant` + 诊断；除零/负移位 ⇒ 对应 `DiagCode`
- [x] **Step 4: 运行确认通过** —— 同时确认**不误报**（反向用例全绿）
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-const/src/eval.rs rgoc/crates/rgoc-const/tests/eval_const.rs
git commit -m "feat(m3): 常量表达式求值器（常量子集）+ 非常量诊断（T-TY-83/84，D-M3-19）"
```

---

## Phase 2：类型表示与判定（`rgoc-types`）

### Task 13: `Type` 枚举与穷尽性

**Files:**
- Create: `rgoc/crates/rgoc-types/src/typ.rs`
- Test: `tests/type_exhaustive.rs`

**Interfaces:**
- Produces（`rgoc_types`）：
  - `pub enum Type { Basic(BasicId), Named(NamedId), Alias(AliasId), Pointer(TypeId), Slice(TypeId), Array{len: ConstId, elem: TypeId}, Map{key: TypeId, elem: TypeId}, Chan{dir: ChanDir, elem: TypeId}, Struct(StructId), Signature(SignatureId), Tuple(TupleId), Interface(InterfaceId), TypeParam(TypeParamId), Union(UnionId), Invalid }`
  - `pub enum ChanDir { SendRecv, SendOnly, RecvOnly }`
  - `pub const TYPE_VARIANT_COUNT: usize`
- Consumes: `rgoc_const::{Const, Sizes}`（`Array.len` 是常量）

- [x] **Step 1: 写失败测试**（`T-TY-11`）
  - `TYPE_VARIANT_COUNT == 15`
  - 一条 `match` 覆盖全部变体（穷尽性由编译器保证）；一条**显式计数**断言
  - 反向探针：临时删掉一个变体 ⇒ 编译失败（记录在案）
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh cargo test -p rgoc-types --test type_exhaustive -v`
- [x] **Step 3: 实现** —— 字段名/类型/可空性照 `types2/{array,chan,map,pointer,slice,struct,signature,tuple,interface,typeparam,union}.go`（**PA-1**）；`Id` 一律是无符号下标（`Copy`）
- [x] **Step 4: 运行确认通过** + `clippy`（若触发 `large_enum_variant`，按 `D-M3-9` 局部 `Box` + 带理由 `#[allow]`）
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-types/src/typ.rs rgoc/crates/rgoc-types/tests/type_exhaustive.rs
git commit -m "feat(m3): Type 枚举（15 变体）+ 穷尽性断言（T-TY-11）"
```

### Task 14: 类型表与 `BasicId`

**Files:**
- Create: `rgoc/crates/rgoc-types/src/table.rs`、`src/basic.rs`
- Test: `src/table.rs` 内

**Interfaces:**
- Produces: `pub struct TypeTable`（arena：`add_*` 返回各 `Id`；`type_of(TypeId)->&Type`）；`pub enum BasicId`（`Bool`,`Int`,`Int8..Int64`,`Uint..Uint64`,`Uintptr`,`Float32`,`Float64`,`Complex64`,`Complex128`,`String`,`UnsafePointer`,`UntypedBool`,`UntypedInt`,`UntypedRune`,`UntypedFloat`,`UntypedComplex`,`UntypedString`,`UntypedNil`,`Invalid`）+ `pub fn basic_underlying(b: BasicId) -> Option<BasicId>`
- Consumes: Task 10 的 `Sizes`；`rgoc-const` 的 `ConstId` 语义

- [x] **Step 1: 写失败测试**
  - `byte ≡ uint8`、`rune ≡ int32`（`basic_underlying` / 判等口径）
  - 预声明类型全集计数（与 `types2` 的 `Typ` 表对齐）
  - 表内 `Array.len` 取到的是 `ConstId`，且能回查 `Const`
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现** —— arena 用 `Vec<T>`；`Id` 用 `u32` newtype（`Copy`/`Ord`/`Hash`）
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-types/src/table.rs rgoc/crates/rgoc-types/src/basic.rs
git commit -m "feat(m3): TypeTable arena + BasicId 全集"
```

### Task 15: universe 作用域与预声明对象

**Files:**
- Create: `rgoc/crates/rgoc-types/src/universe.rs`
- Create: `rgoc/crates/rgoc-types/src/object.rs`
- Test: `tests/universe.rs`

**Interfaces:**
- Produces: `pub struct Object { kind: ObjKind, name: String, typ: Option<TypeId>, pos: Pos, .. }`；`pub enum ObjKind { Var, Const, TypeName, Func, Builtin, PkgName, Label, Nil }`；`pub fn universe() -> (Scope, TypeTable)`；`comparable` 以**特殊对象**登记（`F8`，不是普通类型名）
- Consumes: Task 13/14

- [x] **Step 1: 写失败测试**（`T-TY-51`）
  - universe 含：`bool`/`int`/…/`string`、`true`/`false`/`iota`/`nil`、内建函数名（`len`/`cap`/`make`/`new`/`append`/…）
  - `comparable` 存在但**不是**普通 `TypeName`（标记为特殊约束）
  - 与 oracle 的 universe dump 对撞（若 oracle 侧能枚举）
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现** —— 照 `types2/universe.go`；字段对齐 `types2/object.go`（PA-1）
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-types/src/universe.rs rgoc/crates/rgoc-types/src/object.rs
git commit -m "feat(m3): universe + 预声明对象（T-TY-51）"
```

### Task 16: C7 类型 dump 渲染器

**Files:**
- Create: `rgoc/crates/rgoc-types/src/dump.rs`
- Test: `tests/dump_c7.rs` + `testdata/*.want`

**Interfaces:**
- Produces: `pub fn dump_types(tt: &TypeTable, roots: &[TypeId]) -> String`（严格按 C7：行式 / 2 空格缩进 / `named` 用 `#序号` / 无位置 / `-` 表空）
- Consumes: Task 1 的 C7 契约；Task 13/14

- [x] **Step 1: 写失败测试**（`T-TY-12..18`、`T-TY-19`）
  - `basic` 全集、`pointer/slice/array/map/chan`、`struct`（Tag/嵌入）、`signature`（变参/多结果）、`named`/`alias`（`#序号`）、`interface`（方法集 + 项集）、`typeparam`/`union`/`~T`
  - 幂等：同表两次 `dump_types` 结果**逐字节相等**
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh cargo test -p rgoc-types --test dump_c7 -v`
- [x] **Step 3: 实现** —— 递归渲染，缩进 2 空格，**表内序号**按 `TypeTable` 插入序（两侧必须约定同一序：**声明顺序**，见 C7 §5.2）
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-types/src/dump.rs rgoc/crates/rgoc-types/tests/dump_c7.rs rgoc/crates/rgoc-types/testdata
git commit -m "feat(m3): C7 类型 dump 渲染器（T-TY-12..19）"
```

### Task 17: `Identical`：basic / named / alias / 复合

**Files:**
- Create: `rgoc/crates/rgoc-types/src/identical.rs`
- Test: `tests/identical.rs`

**Interfaces:**
- Produces: `pub fn identical(tt: &TypeTable, x: TypeId, y: TypeId) -> bool`；`pub fn unalias(tt: &TypeTable, t: TypeId) -> TypeId`
- Consumes: Task 13/14

- [x] **Step 1: 写失败测试**（`T-TY-21..25`）
  - `byte` ≡ `uint8`；`rune` ≡ `int32`
  - `type A int` 与 `int` **不等**；`A` 与 `A` 相等
  - `type A = B` 与 `B` 相等（`unalias`）
  - `*A` vs `*B`（按元素）；`[]int` vs `[]int` 等、vs `[1]int` **不等**
  - `[2]int` vs `[1+1]int` **相等**（长度**常量相等**）
  - `map`/`chan`（**方向**必须一致）
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh cargo test -p rgoc-types --test identical -v`
- [x] **Step 3: 实现** —— 分派顺序照 `types2/predicates.go:261`（`F4`）；`Array` 比对用 `rgoc_const` 的常量相等
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-types/src/identical.rs rgoc/crates/rgoc-types/tests/identical.rs
git commit -m "feat(m3): Identical——basic/named/alias/复合（T-TY-21..25）"
```

### Task 18: `Identical`：struct / signature / interface（方法集版）

**Files:**
- Modify: `rgoc/crates/rgoc-types/src/identical.rs`
- Test: `tests/identical.rs`（续）

**Interfaces:**
- Produces: 上述 `identical` 的 `Struct`/`Signature`/`Interface` 分支
- Consumes: Task 17

- [x] **Step 1: 写失败测试**（`T-TY-26..28`）
  - struct：字段**名**、**Tag**、**嵌入**、**顺序**都要一致才算等
  - signature：**变参标记**不同 ⇒ 不等；结果元组按位置比
  - interface：**只比方法集**（方法名 + 签名）；`interface{ M() }` vs `interface{ M(); N() }` 不等
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现** —— interface 分支**明确只走方法集**（类型集版留 M4，见 Task 19 的排除）
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-types/src/identical.rs rgoc/crates/rgoc-types/tests/identical.rs
git commit -m "feat(m3): Identical——struct/signature/interface 方法集版（T-TY-26..28）"
```

### Task 19: 约束接口的同一性 ⇒ 显式拒绝

**Files:**
- Modify: `rgoc/crates/rgoc-types/src/identical.rs`
- Modify: `rgoc/crates/rgoc-lex/src/diag.rs`（新码 `TypeSetIdentityUnsupported`）
- Test: `tests/identical_reject.rs`

**Interfaces:**
- Produces: `pub fn identical_or_reject(tt: &TypeTable, x: TypeId, y: TypeId, bag: &mut DiagBag) -> Option<bool>` —— 任一操作数是**含 `~T`/并集**的接口时返回 `None` 并挂诊断
- Consumes: Task 18、Task 11 的五件套

- [x] **Step 1: 写失败测试**（`T-TY-81` 双向）
  - 正向：`interface{~int}` vs `interface{~int}` ⇒ `None` + `TypeSetIdentityUnsupported`
  - 反向：`interface{ M() }` vs `interface{ M() }` ⇒ `Some(true)`，**无**诊断
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现** —— 检测接口的项集是否非「全部」（`F5` 的 `isAll`）；非全部即拒绝。**不是**给个错的答案
- [x] **Step 4: 运行确认通过** + 反向验证（把拒绝改成 `Some(true)` ⇒ 正向测试必须红）
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-types/src/identical.rs rgoc/crates/rgoc-lex/src/diag.rs rgoc/crates/rgoc-types/tests/identical_reject.rs
git commit -m "feat(m3): 约束接口同一性显式拒绝（T-TY-81，D-M3-13）"
```

### Task 20: 实例化具名类型的边界

**Files:**
- Modify: `rgoc/crates/rgoc-types/src/identical.rs`
- Test: `tests/identical_generic.rs`

**Interfaces:**
- Produces: `Named` 的 `origin: NamedId` + `targs: Vec<TypeId>`（表示层已备，判定层 M3 走「同一 Id」）

- [x] **Step 1: 写失败测试**（`T-TY-29`）
  - `type G[T any] struct{}`：`G[int]` 与 `G[int]` 相等；`G[int]` 与 `G[string]` **不等**（按 targs 逐一比）
  - 记录边界：M3 **不做**实例化（不产生新的 `NamedId`），故该测试只覆盖「表示层的 targs 相等」
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现** —— 按 targs 逐一 `identical`；**不**做实例化
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-types/src/identical.rs rgoc/crates/rgoc-types/tests/identical_generic.rs
git commit -m "feat(m3): 实例化具名类型的判定边界（T-TY-29）"
```

### Task 21: 版本门控：按文件 + 两处 TooNew

**Files:**
- Create: `rgoc/crates/rgoc-types/src/version.rs`
- Modify: `rgoc/crates/rgoc-lex/src/diag.rs`（`PackageRequiresNewerGo`、`FileRequiresNewerGo`、`FeatureRequiresGoVersion`）
- Test: `tests/version_gate.rs`

**Interfaces:**
- Produces: `pub struct Versions(HashMap<FileId, GoVersion>)`；`pub fn pkg_versions(lang: Option<&str>, files: &[&rgoc_ast::AstFile]) -> (Versions, DiagBag)`；`impl Versions { pub fn allow(&self, f: FileId, want: GoVersion) -> bool }`
- Consumes: `rgoc-ast::AstFile::go_version`（M2 已填）

- [x] **Step 1: 写失败测试**（`T-TY-71..75`）
  - `-lang go1.18` 允许类型参数；`-lang go1.17` 拒绝（`FeatureRequiresGoVersion`）
  - 文件 `//go:build go1.22` 且 `-lang` 缺省 ⇒ 该文件版本 = `max(go1.22, go1.21)` = `go1.22`
  - 文件版本 `go1.99` ⇒ `FileRequiresNewerGo`（位置取该文件 `package` 名）
  - 包级 `-lang go1.99` ⇒ `PackageRequiresNewerGo`（位置取 `files[0]`）
  - **按文件**：同包两文件不同版本，各自判定
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现** —— 照 `types2/version.go` 与 `check.go:312-348`（`F3`/`F13`）：`go1.%d` 解析、`cmp`、`allow`、`max(v, go1.21)`、两处 `TooNew`
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-types/src/version.rs rgoc/crates/rgoc-lex/src/diag.rs rgoc/crates/rgoc-types/tests/version_gate.rs
git commit -m "feat(m3): 版本门控——按文件 + 两处 TooNew（T-TY-71..75，D-M3-15）"
```

### Task 22: 类型图循环检测与递归深度

**Files:**
- Modify: `rgoc/crates/rgoc-types/src/table.rs`（或新建 `src/validtype.rs`）
- Modify: `rgoc/crates/rgoc-lex/src/diag.rs`（`InvalidRecursiveType`）
- Test: `tests/type_cycle.rs`

**Interfaces:**
- Produces: `pub fn valid_type(tt: &TypeTable, t: TypeId, bag: &mut DiagBag) -> bool`

- [x] **Step 1: 写失败测试**
  - `type A [1]A`（非法递归）⇒ 诊断
  - `type A *A`（合法，经指针）⇒ **不**诊断
  - 深嵌套 `[1][1][1]...` 恰 **1000** 层 ⇒ 恰 **1 条**诊断（超限）
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现** —— 照 `types2/validtype.go`（`F9`）：按名去重（`HashSet<NamedId>`）而非指针；深度计数与 parser 的 `MAX_NEST_DEPTH` 同值
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-types/src rgoc/crates/rgoc-lex/src/diag.rs rgoc/crates/rgoc-types/tests/type_cycle.rs
git commit -m "feat(m3): 类型图循环检测 + 递归深度 1000"
```

---

## Phase 3：包模型（`rgoc-load`）

### Task 23: 单包多文件 + 文件/包作用域

**Files:**
- Create: `rgoc/crates/rgoc-load/src/lib.rs`、`src/scope.rs`、`src/package.rs`
- Test: `tests/package_scope.rs`

**Interfaces:**
- Produces（`rgoc_load`）：
  - `pub struct Package { pub name: String, pub files: Vec<FileId>, pub scope: ScopeId }`
  - `pub enum ScopeKind { Universe, Package, File, Func, Block }`
  - `pub fn new_file_scope(parent: ScopeId) -> ScopeId`；`pub fn insert(scope: ScopeId, obj: ObjectId) -> Result<(), DiagCode>`
- Consumes: Task 15 的 universe；`rgoc-parse::AstFile`

- [x] **Step 1: 写失败测试**（`T-TY-52`、`T-TY-61`、`T-TY-66`）
  - 两个文件：`a.go` 声明 `type T int`，`b.go` 用 `T` ⇒ 可见（**包作用域**）
  - 同包多文件 `package` 名不一致 ⇒ 诊断
  - 文件作用域的 import 名**只在该文件**可见（不是包级）
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh cargo test -p rgoc-load --test package_scope -v`
- [x] **Step 3: 实现** —— 作用域链 `universe ⊃ package ⊃ file ⊃ func ⊃ block`（`SCP-01`）
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-load/src
git commit -m "feat(m3): 单包多文件 + 包/文件作用域（T-TY-52/61/66）"
```

### Task 24: 声明收集与类型求值顺序

**Files:**
- Modify: `rgoc/crates/rgoc-load/src/package.rs`
- Test: `tests/decl_collect.rs`

**Interfaces:**
- Produces: `pub fn collect_decls(pkg: &mut Package, tt: &mut TypeTable, bag: &mut DiagBag)`（先声明后求值；具名类型延迟求值）

- [x] **Step 1: 写失败测试**
  - 前向引用：`type A B` 在 `type B int` **之前** ⇒ 合法（先收集再求值）
  - 常量组依赖：`const A = B + 1; const B = 2` ⇒ `A == 3`
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现** —— 顺序**照官方**：建 universe → 收集声明（建 `Object` 占位）→ 求值类型/常量
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-load/src/package.rs rgoc/crates/rgoc-load/tests/decl_collect.rs
git commit -m "feat(m3): 声明收集 + 类型求值顺序"
```

### Task 25: 块作用域 / 遮蔽 / 未定义 / 重复声明

**Files:**
- Modify: `rgoc/crates/rgoc-load/src/scope.rs`
- Modify: `rgoc/crates/rgoc-lex/src/diag.rs`（`UndefinedIdent`、`RedeclaredInThisBlock`、`RedeclaredInThisPackage`）
- Test: `tests/scopes.rs`

**Interfaces:**
- Produces: `pub fn lookup(scope: ScopeId, name: &str) -> Option<ObjectId>`（沿 parent 链）；`pub fn declare(scope: ScopeId, obj: Object) -> Result<ObjectId, DiagCode>`

- [x] **Step 1: 写失败测试**（`T-TY-53..55`）
  - 遮蔽：内层 `var x` 遮外层 `x`；内层块结束后外层仍可见
  - 未定义 `y` ⇒ `UndefinedIdent`
  - 块内重复声明 `x` 两次 ⇒ `RedeclaredInThisBlock`
  - **包级**重复声明 ⇒ `RedeclaredInThisPackage`（**与块级不同的码**，`SCP` 规则）
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现** —— `SCP-02/03/04`；包级与块级走**不同**诊断码
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-load/src/scope.rs rgoc/crates/rgoc-lex/src/diag.rs rgoc/crates/rgoc-load/tests/scopes.rs
git commit -m "feat(m3): 作用域遮蔽/未定义/重复声明（T-TY-53..55，SCP）"
```

### Task 26: 导出标识符（Unicode）/ 标签 / 空标识符

**Files:**
- Create: `rgoc/crates/rgoc-load/src/exported.rs`
- Modify: `rgoc/crates/rgoc-load/src/scope.rs`
- Test: `tests/exported_label.rs`

**Interfaces:**
- Produces: `pub fn is_exported(name: &str) -> bool`（**Unicode 大写字母**判定，不是 `A..=Z`）；`pub fn declare_label(..)`；空标识符 `_` **不进**作用域

- [x] **Step 1: 写失败测试**（`T-TY-56/57/58`，含 **`SCP-06` 缺口**）
  - `IsExported("Foo")`、`!IsExported("foo")`
  - **Unicode**：`É`、`Ł`、`Ω`（大写）⇒ 导出；`é`、`ω` ⇒ 不导出
  - `_` 声明两次**不**冲突、**不**可引用
  - 标签：函数内 `goto L` 与 `L:` 绑定；跨函数不可见
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现** —— 用 `unicode` 的 `is_uppercase`（**不**自造表；沿用 M1 `D-M1-6` 的 Unicode 表来源纪律）
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-load/src/exported.rs rgoc/crates/rgoc-load/src/scope.rs rgoc/crates/rgoc-load/tests/exported_label.rs
git commit -m "feat(m3): 导出标识符（Unicode，补 SCP-06 缺口）+ 标签 + 空标识符（T-TY-56..58）"
```

### Task 27: 源码 import + 路径映射 + cycle 诊断

**Files:**
- Create: `rgoc/crates/rgoc-load/src/importer.rs`
- Modify: `rgoc/crates/rgoc-lex/src/diag.rs`（`ImportCycle`、`UnsupportedImportForm`、`ImportNotFound`）
- Test: `tests/imports.rs` + `testdata/multi-pkg/`

**Interfaces:**
- Produces: `pub struct ImportResolver { roots: Vec<PathBuf> }`；`impl ImportResolver { pub fn resolve(&self, path: &str) -> Option<Vec<PathBuf>> }`；`pub fn load_with_imports(entry: &[PathBuf], roots: &[PathBuf], lang: Option<&str>) -> (Package, DiagBag)`

- [x] **Step 1: 写失败测试**（`T-TY-62/63/85`）
  - 正向（**不**报错）：`import "lib"` 且 `roots` 下有 `lib/*.go` ⇒ 装载成功
  - 正向：**import cycle** `a→b→a` ⇒ `ImportCycle`（环上每个包一条）
  - 正向（**报错**）：`import "fmt"`（无源码、需二进制 export data）⇒ `UnsupportedImportForm`，**不是**「找不到」
  - 反向：路径存在但目录里没有 `.go` ⇒ `ImportNotFound`
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh cargo test -p rgoc-load --test imports -v`
- [x] **Step 3: 实现** —— 只在 `roots` 内按**显式路径**找同名目录（`D-M3-5`）；cycle 用「正在加载栈」检测；深度上限 100
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-load/src/importer.rs rgoc/crates/rgoc-lex/src/diag.rs rgoc/crates/rgoc-load/tests/imports.rs rgoc/crates/rgoc-load/testdata
git commit -m "feat(m3): 源码 import + 路径映射 + cycle 诊断（T-TY-62/63/85）"
```

### Task 28: 显式文件列表（不评估构建约束）

**Files:**
- Modify: `rgoc/crates/rgoc-load/src/package.rs`
- Test: `tests/file_selection.rs`

**Interfaces:**
- Produces: `pub fn load_from_explicit_files(files: &[PathBuf], lang: Option<&str>) -> (Package, DiagBag)`

- [x] **Step 1: 写失败测试**（`T-TY-65`）
  - `a.go` 带 `//go:build ignore`、`b.go` 带 `//go:build windows`，两者都列进包 ⇒ **都被装载**、**都不报错**
  - 断言：`package.files.len() == 2`（**没有**被构建约束筛掉）
  - 断言：`_test.go` 若被列出也照样进包（M3 不做测试文件语义）
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现** —— **不读** `//go:build` 做筛选；只把 `AstFile.go_version` 交给 Task 21 的版本门控（读版本 ≠ 筛文件）
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-load/src/package.rs rgoc/crates/rgoc-load/tests/file_selection.rs
git commit -m "feat(m3): 显式文件列表，不评估构建约束（T-TY-65，D-M3-14）"
```

### Task 29: `ExportSummary` 内存 API

**Files:**
- Create: `rgoc/crates/rgoc-load/src/export_summary.rs`
- Test: `tests/export_summary.rs`

**Interfaces:**
- Produces: `pub struct ExportSummary { entries: BTreeMap<String, ExportEntry> }`；`pub enum ExportEntry { Type(String), Const(String), Func(String), Var(String) }`；`pub fn export_summary(pkg: &Package, tt: &TypeTable) -> ExportSummary`（值用 **C7 dump 行**表示）

- [x] **Step 1: 写失败测试**（`T-TY-64`）
  - 只有导出名进摘要；非导出名人不在、但**类型 dump 里会出现**（作为字段）
  - 顺序**确定**（`BTreeMap`）；两次调用结果相同
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现** —— **不**冻文本格式、**不**落盘（`D-M3-18`）
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-load/src/export_summary.rs rgoc/crates/rgoc-load/tests/export_summary.rs
git commit -m "feat(m3): ExportSummary 内存 API（T-TY-64，D-M3-18）"
```

### Task 30: 冒烟：零 import 语料包

**Files:**
- Create: `rgoc/crates/rgoc-load/tests/smoke_corpus.rs`
- Test: 同上

**Interfaces:**
- Consumes: Task 28 的 `load_from_explicit_files`

- [x] **Step 1: 先找语料**：在 `go_source_code/src/` 下筛出**真正零 import** 的单包（例如某些 `internal/*` 叶子包），把路径列表**写死进测试**
- [x] **Step 2: 写失败测试**（`T-TY-96`）：逐包 `load_from_explicit_files`，断言**不 panic、不超时**，且诊断数为**可解释**（要么 0，要么全部属于白名单类别）
- [x] **Step 3: 运行确认失败/或确认门槛** —— `scripts/in-container.sh cargo test -p rgoc-load --test smoke_corpus -v`
- [x] **Step 4: 实现测试夹具**（如需在 `roots` 下建最小桩以解 import —— 注意：**只**用零 import 包，桩不必要则应删）
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-load/tests/smoke_corpus.rs
git commit -m "test(m3): 零 import 语料包冒烟（T-TY-96）"
```

---

## Phase 4：oracle 对撞

### Task 31: `types-oracle` 实现（C7 dump）

**Files:**
- Modify: `tools/types-oracle/main.go`
- Test: `tools/types-oracle/testdata/`（新增 fixture）

**Interfaces:**
- Produces: `types-oracle [-lang go1.N] <files…>` → stdout 为 C7 dump；`--cmp` 模式读 `//rgoc:cmp` 指令并输出判定
- Consumes: Task 1 的 C7；Task 3 的骨架

- [x] **Step 1: 写失败测试**（`T-TY-91` 的 oracle 侧）：对 `type-dump-min` 输出与 `type-dump-min.want` **逐字节一致**
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh bash -c 'cd tools/types-oracle && go test ./...'`
- [x] **Step 3: 实现** —— `go/parser`（`ParseComments`）→ `types.Config{GoVersion: lang}.Check`；`types.TypeString(t, types.RelativeTo(pkg))` 不够——**必须按 C7 自己渲染**（表内序号、2 空格缩进）
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add tools/types-oracle
git commit -m "feat(m3): types-oracle 实现 C7 dump（T-TY-91 oracle 侧）"
```

### Task 32: `//rgoc:cmp` 指令读取

**Files:**
- Modify: `tools/types-oracle/main.go`
- Modify: `rgoc/crates/rgoc-driver/src/cli.rs`（`--cmp` 模式）
- Test: `tools/types-oracle/testdata/cmp-basic.go` + 两侧测试

**Interfaces:**
- Produces: 两侧各自的 `//rgoc:cmp identical <A> <B>` / `//rgoc:cmp representable <C> <T>` 解析器，输出规范行 `<行号>\t<kind>\t<result>`（`result ∈ true|false|reject`）
- Consumes: Task 1 的编址约定（`D-M3-17`）

- [x] **Step 1: 写失败测试**（`T-TY-92/93` 的输入侧）
  - 注释里的 `<A>`/`<B>` 是**表达式**，两侧都按「同一文件里的标识符/类型表达式」解析
  - `reject` 只在 rgoc 侧出现（oracle 不会拒绝）；**断言契约**：rgoc 输出 `reject` 的条目，必须在 `M3-tests.md` §6 白名单里
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现** —— 两侧都用「按行读注释 + 表达式解析」；rgoc 侧复用 `rgoc-parse`
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add tools/types-oracle rgoc/crates/rgoc-driver
git commit -m "feat(m3): //rgoc:cmp 指令读取（T-TY-92/93 输入侧，D-M3-17）"
```

### Task 33: dump 对撞 + 确定性

**Files:**
- Modify: `tools/types-oracle/run-collide.sh`
- Create: `rgoc/crates/rgoc-driver/tests/collide_dump.rs`
- Test: 同上

**Interfaces:**
- Produces: 门禁脚本，rgoc 与 oracle 两侧 dump 逐字节 `diff`

- [x] **Step 1: 写失败测试**（`T-TY-91`、`T-TY-95`）
  - 对**全部**自建 dump fixture 逐字节一致
  - oracle 侧重复运行 **sha256 一致**
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh bash tools/types-oracle/run-collide.sh tools/types-oracle/testdata`
- [x] **Step 3: 实现** —— 先跑 oracle 落盘 `.want`，再跑 rgoc，`diff -u`；不一致时**打印首个差异行 + 两侧上下文**
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add tools/types-oracle/run-collide.sh rgoc/crates/rgoc-driver/tests/collide_dump.rs
git commit -m "test(m3): dump 对撞 + oracle 确定性（T-TY-91/95）"
```

### Task 34: identity / representable 对撞

**Files:**
- Modify: `tools/types-oracle/run-collide.sh`
- Create: `rgoc/crates/rgoc-driver/tests/collide_cmp.rs`
- Test: 同上

**Interfaces:**
- Consumes: Task 32 的 `--cmp`

- [x] **Step 1: 写失败测试**（`T-TY-92/93`）
  - identity 对撞：**排除**约束接口条目（`D-M3-13`）；断言排除集 == `M3-tests.md` §6 的 `U-M3-05`
  - representable 对撞：边界值（`int8` 边界、`float32` 中点）全部一致
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现** —— `run-collide.sh --cmp` 分支：两侧输出按行归一后 `diff`
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add tools/types-oracle/run-collide.sh rgoc/crates/rgoc-driver/tests/collide_cmp.rs
git commit -m "test(m3): identity/representable 对撞（T-TY-92/93）"
```

### Task 35: `testdata/` 精选扩大对撞

**Files:**
- Modify: `tools/types-oracle/run-collide.sh`（接受 `--corpus <dir>`）
- Create: `tools/types-oracle/corpus-allowlist.txt`
- Test: `rgoc/crates/rgoc-driver/tests/collide_corpus.rs`

**Interfaces:**
- Consumes: `docs/milestones/M3-tests.md` §6 白名单

- [x] **Step 1: 写失败测试**（`T-TY-94`）
  - 从 `go_source_code/src/cmd/compile/internal/types2/testdata/` 或 `go/types/testdata/` 精选**单包、无 import** 的文件
  - 筛掉的每一个都要**在 `corpus-allowlist.txt` 里有一行理由**；断言「被筛掉的文件的每一行理由**非空**」
- [x] **Step 2: 运行确认失败**
- [x] **Step 3: 实现** —— 白名单机制；对每个入选文件跑 dump + `--cmp` 对撞
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add tools/types-oracle/run-collide.sh tools/types-oracle/corpus-allowlist.txt rgoc/crates/rgoc-driver/tests/collide_corpus.rs
git commit -m "test(m3): testdata 精选扩大对撞 + 白名单理由（T-TY-94）"
```

---

## Phase 5：端到端与收口

### Task 36: `rgoc types` 端到端 + 拒绝侧双向全套

**Files:**
- Modify: `rgoc/crates/rgoc-driver/src/cli.rs`
- Create: `rgoc/crates/rgoc-driver/tests/types_e2e.rs`
- Test: 同上

**Interfaces:**
- Produces: `rgoc types [--lang go1.N] [--dump] [--cmp] [-I <root>]… <files…>`

- [x] **Step 1: 写失败测试**（`T-TY-81..85` 的**成对**全套）
  - `T-TY-82` 正向：请求 `TYP-13/18/19/23..26` 相关判定 ⇒ 稳定码；反向：相邻 M3 支持项**不**报错
  - `T-TY-84`：`//go:totally-made-up` ⇒ **前端不报错**；`//go:build` 版本**仍被读取**
  - 退出码语义：仅警告 ⇒ 0；有 error ⇒ 非 0
- [x] **Step 2: 运行确认失败** —— `scripts/in-container.sh cargo test -p rgoc-driver --test types_e2e -v`
- [x] **Step 3: 实现** —— 子命令串起 `rgoc-load` → `rgoc-types` → dump/诊断；`--dump` 走 C7
- [x] **Step 4: 运行确认通过**
- [x] **Step 5: 提交**

```bash
git add rgoc/crates/rgoc-driver
git commit -m "feat(m3): rgoc types 端到端 + 拒绝侧双向断言（T-TY-81..85）"
```

### Task 37: 反向验证（变异测试）

**Files:**
- Create: `scripts/mutation-m3-notes.md`（不写脚本，**记录人工变异**）

**Interfaces:**
- Consumes: `M3-tests.md` §8 冻结规则 4

- [x] **Step 1: 选探针** —— 至少覆盖：`T-TY-11`（删变体）、`T-TY-19`（dump 幂等）、`T-TY-47`（溢出改截断）、`T-TY-65`（构建约束筛选）、`T-TY-81`（拒绝改错答）、`T-TY-84`（未知指令改报错）
- [x] **Step 2: 逐个变异 + 跑对应测试** —— 每个都必须**红**；用 `md5` 逐字节复原后确认恢复绿
- [x] **Step 3: 记录** —— 每条写「变异 → 期望红 → 实测 → 复原证据」；**照不出失败的探针如实登记**（不得硬凑）
- [x] **Step 4: 提交**

```bash
git add scripts/mutation-m3-notes.md
git commit -m "test(m3): 反向验证记录（6 条探针，含照不出的如实登记）"
```

### Task 38: manifest 填实 + 全门禁复核

**Files:**
- Create: `docs/milestones/M3-manifest.json`
- Modify: `rgoc/xtask/src/manifest.rs`（若需重放生成）
- Modify: `docs/milestones/M3-plan.md`（状态回写）、`AGENTS.md`（§0/§2.1/§3）、`.workbuddy/memory/2026-10-16.md`

**Interfaces:**
- Produces: `M3-manifest.json` 含 `stage/generated_at/scope/crates/gate(TY-1..8)/coverage/test_ids/unsupported(U-M3-01..13)/budget/open_gaps/phase_summary`

- [x] **Step 1: 跑四条 cargo 门禁**（`M3-tests.md` §7.3）—— 四条都要 exit 0
- [x] **Step 2: 跑三条既有自检** —— `check-m0-consistency.py`（231）、`check-m1-consistency.py`、M2 自检；全过
- [x] **Step 3: 跑文档自检**（`AGENTS.md` §6.1 第 1–3 条）—— 无 `BROKEN`、无旧名、目录齐全
- [x] **Step 4: 写 manifest** —— `gate` 逐条给出证据出处（测试 ID / 命令 / 输出摘要）；`unsupported` 与 `M3-tests.md` §6 **逐条对齐**（写一条断言校验两者集合相等）
- [x] **Step 5: 同步文档 + 提交**

```bash
git add docs/milestones/M3-manifest.json docs/milestones/M3-plan.md AGENTS.md .workbuddy/memory/2026-10-16.md rgoc/xtask
git commit -m "chore(m3): manifest 填实（TY-1..TY-8）+ 全门禁复核 + 文档同步"
```

---

## 自检记录（计划写完后的对照）

| 检查 | 结论 |
|---|---|
| **规格覆盖** | `M3-tests.md` §2 覆盖矩阵的每个 ✅ 行都能指到一个任务；§6 的 13 条白名单都有任务落点（Task 19/27/28/36 等） |
| **步骤扫描** | 无「TBD」「加适当校验」类决定不了任何事的行；无整段函数体（只给签名与断言） |
| **类型一致** | `Representable(&Const, BasicId, &Sizes)`、`identical(tt, x, y)`、`unalias(tt, t)`、`load_from_explicit_files(files, lang)`、`dump_types(tt, roots)` 在各任务间**同名同签名** |
| **Review Focus** | 五条各已钉到任务：① Task 19 ② Task 4/10 ③ Task 4/10 ④ Task 28 ⑤ Task 36 |
| **比例** | 本计划 ≈ 设计文档（624 行）的 1.3 倍，与该仓 M1 先例（设计 ≈700 行 / 计划 1655 行）同量级；无代码转写 |

