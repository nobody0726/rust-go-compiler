# M7 实施计划：单平台 native MVP（arm64 代码生成）

> **状态**：**待执行**（Task 1–65）　|　**判据**：[`M7-tests.md`](./M7-tests.md)（已冻结，64 个 `T-NAT-*`）　|　**设计**：[`M7-design.md`](./M7-design.md)（`D-M7-1..16`）　|　**事实**：[`M7-manifest.json`](./M7-manifest.json)（Task 59 填实）
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> 执行期进度、偏离与 Ruling 记在 `.superpowers/sdd/M7-plan/progress.md`（不入库）。

**Goal:** 把 M6 交的 SSA 变成 **aarch64 机器码**，跑出真正的 ELF：帧布局可核对、调用与块参数交接正确、runtime 输出字节与 `C9` 一致、三方差分在 native 切片上逐字节全绿。

**Architecture:** 新 `rgoc-arm64`（纯数据：`abi` / `frame` / `isel` / `emit` / `diag`）+ `runtime/native/aarch64-unknown-linux-gnu/rt.c`（freestanding C）+ `rgoc-driver build`。汇编以**文本 `.s`** 产出，交容器内 `clang -nostartfiles -Wl,-s` 汇编并链接（`D-M7-2`，M0 spike 的三条实测约束原样沿用）。验收六腿：甲帧与 ABI、乙指令选择、丙 runtime 与输出、丁三方差分、戊装置与可复现，外加 8 条变异。

**Tech Stack:** Rust 1.98.1 / edition 2024 / resolver 3；workspace 在 `rgoc/`；容器内 `clang 14.0.6`（`aarch64-unknown-linux-gnu`）；全部命令经 `scripts/in-container.sh`（宿主不构建）；不引第三方依赖。

**Spec:** [`docs/milestones/M7-design.md`](./M7-design.md) + [`docs/milestones/M7-tests.md`](./M7-tests.md)。**执行者必须同时读这两份** —— 本计划只决定「顺序与接口」，判据以 tests 清单为准。

## Global Constraints

- **只在容器内构建/测试**：一切 `cargo`/`clang`/`go` 经 `scripts/in-container.sh`；cargo 必须 `bash -lc 'cd /work/rgoc && …'`。
- **不引第三方依赖**：槽分配、并行拷贝、`ftoa` 一律自己写（`D-M7-4`/`D-M7-15` 的邻居）。
- **`rgoc-arm64` 是纯函数**：`emit` 不许碰磁盘、不许读环境变量、不许看时间（`T-NAT-48` 的前提）。
- **`unsafe_code = "forbid"`**：生成器侧不写 `unsafe`；`rt.c` 是 C，不受此限，但**不许**引 libc。
- **`go_source_code/` 只读**：跑完批次后 `corpus-manifest.sha256` 必须校验通过（`T-NAT-56`）。
- **切片必须实测**（`D-M7-8`）：`N'` 与 `M'` 成对报，`N'+M'==46` 两向断言，**不许凑数**。
- **判据先行**：每个 Task 先写会红的 fixture；测试之前写的实现一律删除。
- **`Instr`/`Term` 的 match 不许有通配分支**：加一个变体就编译失败，逼你决定它的位置（`M7-design.md` §9 铁律）。
- **产物陈旧陷阱**：变异前后 `cargo clean -p <被变异 crate>`（`scripts/mutation-m3-notes.md` §0）。
- **每个 Task 收口即 commit**（信息里带 Task 号与 `T-NAT-*`）。
- **资源上限**：arena 1 MiB / 单文件编译 30 s / 单文件运行 10 s。

## Review Focus

以下是 `M7-design.md` 隐含、但没有任何单条任务天然覆盖的五类失效模式，最可能的排前面。每条都把测试钉到拥有该代码的任务上：

1. **块参数被顺序搬**（`a, b = b, a` 静默错）：与 M6 的 `D-M6-3` 是**同一个坑**，只是换到机器码上；90% fixture 仍绿（Task 12/16，`T-NAT-06/07` + 变异 `T-NAT-57`）。
2. **槽宽/对齐算错**：相邻槽互相覆盖，症状是「偶尔错」，只在特定类型组合出现（Task 8/9/17，`T-NAT-01/04`）。
3. **`sp` 不对齐 16**：只在调 runtime 时崩，纯计算 fixture 全绿（Task 13/15，`T-NAT-05`）。
4. **`ftoa` 做成「差不多最短」**：语料 42/46 空输出，抓不到；只在用户程序里爆（Task 43，`T-NAT-34` 的六个冻结点）。
5. **三方差分退化成「都没崩」**：42 个空输出文件上三方同时绿毫无信息量（Task 55/56，`T-NAT-42/43` 强制分栏）。

---

## 执行总览

| Phase | Task | 内容 |
|---|---|---|
| 0 | 1–7 | 装置与契约：`C5` 升正式、crate 骨架、emitter、`--native` 装置、分类器、错误码 |
| 1 | 8–17 | 帧与 ABI：槽分类 → 布局 → 零值 → `FRAME-MAP` → 并行拷贝 → 序言/调用点 |
| 2 | 18–39 | 指令选择：14 个 `Instr` + 5 个 `Term` 逐个「先写会红的 fixture 再实现」 |
| 3 | 40–48 | runtime：`rt.c`（write/exit/itoa/ftoa/complex/panic/arena/memcpy） |
| 4 | 49–53 | startup 与链接：`_start`、init 顺序、`build` 接线、可复现、hello 端到端 |
| 5 | 54–60 | 集成：三方差分、切片实测冻结、分栏报告、不支持面、manifest、变异 |
| 6 | 61–65 | 收口：四条 cargo、M6 零回归、报告、文档回写、全量回归 |

---

## Phase 0：装置与契约（Task 1–7）

### Task 1: `C5` 升正式（调用约定 + 帧布局 + `FRAME-MAP v1`）

> **开工前修正（`R-1-1`）**：设计稿原写「新建 `C11-native-abi.md`」，但 `docs/contracts/C5-abi.md`
> **已经存在**且标题就是「ABI / frame layout / 对象布局 / runtime symbol bridge」，文件头写明
> 「归属：**M7** 做 MVP、M9 完整化」。新建 C11 等于把同一件事写两遍，还会让 C5 永远停在 spike 级。
> 正确动作是**升正式**。

**Files:**
- Modify: `docs/contracts/C5-abi.md`（升正式；原文六项记录与可复现证据**保留**）
- Modify: `docs/README.md`（契约清单把 C5 从「spike 级」改成「已升正式」）

**Interfaces:**
- Produces: `C5` 的四张表（寄存器分类 / 聚合按指针 / 帧布局 / `FRAME-MAP v1` 行语法），供 Task 8–11 逐条对齐

- [ ] 改头部：`契约级别：spike 级` → **正式（MVP 级）**；写明「M0 的六项记录保留为历史证据，M7 的 §2.1'–§4 是新增正文」
- [ ] 写 §2.1′ 寄存器约定：`x0..x7` 整数/指针、`v0..v7` 浮点（**独立计数**）、`complex` 占 2 个 `v`、聚合**按指针**、多返回值按结果顺序依次占两个序列、聚合返回值走 `x8`、`x19..x29`/`v8..v15` 生成代码**一律不用**
- [ ] 写 §3 帧布局：`saved fp` +0 / `saved lr` +8 / 值槽区 / 取址槽区 / 并行拷贝临时区 / outgoing 区；`sp` 恒 16 对齐；槽宽与对齐规则
- [ ] 写 §4 `FRAME-MAP v1`：`# FRAME-MAP v1` / `# func <name> frame=<n> slots=<n>` / `# slot <v> off=<n> size=<n> type=<t>` / `# safepoint b<k>:<idx> offs=[..] ptrs=[..]`
- [ ] 写 §5 三条禁止项：不含地址 / 不含哈希序 / 不含时间；聚合只报起始偏移与大小
- [ ] 反向自洽：§4 的每个字段名都在 §4 的示例里出现（文档自洽测试）
- [ ] Commit：`docs(m7): C5 ABI 契约升正式（Task 1，R-1-1）`

**Verify:** `grep -c '^| ' docs/contracts/C5-abi.md` ⇒ ≥ 20；`grep -n 'C5' docs/README.md` 有命中且不再写「spike 级」

---

### Task 2: `rgoc-arm64` crate 骨架 + `build` 子命令空壳

**Files:**
- Create: `rgoc/crates/rgoc-arm64/Cargo.toml`、`src/lib.rs`、`src/diag.rs`
- Modify: `rgoc/Cargo.toml`（members）、`rgoc/crates/rgoc-driver/Cargo.toml`、`src/cli.rs`、`src/lib.rs`、新模块 `src/build_cmd.rs`

**Interfaces:**
- Produces: `rgoc_arm64::diag::CgCode`（`ALL` + `as_str`，沿 `SsaCode` 形状）；CLI `rgoc-driver build [-o PATH] [--emit=asm|elf] [--keep-asm] <file>...`
- Consumes: 无（骨架）

- [x] 建 crate；`lib.rs` **只声明 `pub mod diag;`** —— 其余模块由各自 Task 建立
      （**不留空占位文件**：一个只有 `//!` 的模块看起来像「已实现」，实际什么都没有）
- [x] `CgCode` **三个**变体：`Slice` / `IndirectCall` / `MethodValue`
      （**偏离计划**：原写四个，`OutOfArena` 是**运行期**事件 —— 出口是 `panic: out of arena`
      + 退出码 2，不是生成期诊断。把运行期事件编进 `CgCode::ALL` 会让码表里混进一个
      编译期永不出现的成员，正好违反「拒绝必须有码」的反面）
- [x] 写 `CgCode::ALL` 与计数测试（`tests/diag.rs` 4 条：条数/字典序/前缀唯一/不撞 TYP- 与 SSA-）
- [x] driver 加 `Command::Build(BuildCommand)` + `Emit` + `-o` / `--emit` / `--keep-asm` / `--opt`；
      `build_cmd::run_build` 返回 `Err("尚未实现")`（→ 退出码 2）
- [x] 测试 `T-NAT-47` 的骨架版：`tests/native_cli.rs` 7 条（含「`--emit=llvm` 不许静默回落」）
- [x] **连带修**：`tests/test_driver.rs::未实现的子命令必须报错` 把 `build` 从「将来会有」清单
      移出（T37 的清单随实现推进而收缩，否则它自己变成谎话）
- [x] Commit：`feat(arm64): crate 骨架与 build 子命令（Task 2）`

**Verify:** `… cargo test -p rgoc-arm64 --test diag`（4 条）；`… cargo test -p rgoc-driver --test native_cli`（7 条）；`… cargo clippy -p rgoc-arm64 -p rgoc-driver --all-targets -- -D warnings`（0）

---

### Task 3: 文本汇编 emitter 骨架（确定性）

**Files:**
- Create: `rgoc/crates/rgoc-arm64/src/emit.rs`
- Test: `rgoc/crates/rgoc-arm64/tests/emit_deterministic.rs`

**Interfaces:**
- Produces: `Asm { text, rodata, data }`、`global/label/ins/directive/comment`、`fresh_label() -> u32`、
  `rodata_bytes(&[u8]) -> String`、`rodata_f64(u64) -> String`、`data_bytes(&[u8]) -> String`、`render() -> String`
- Consumes: 无

- [x] 实现 `Asm`：三段（`.text` / `.rodata` / `.data`），`render` 按**固定顺序**拼接
      ⇒ 「先写代码后写常量」与反过来写得到同一份产物
- [x] 标签按**调用顺序**递增（`.Lstr<N>` / `.Lf64_<N>` / `fresh_label()`），**不按内容去重**
- [x] `rodata_f64` 收**位模式**（`.quad 0x…`）而不是 `.double`：后者要过汇编器的十进制解析，
      `-0.0`/`NaN`/次正规数都不是「写出来就一样」
- [x] 空串也写一个 0 字节（Go 允许 `len==0` 时取地址，标签后面不能直接跟下一个标签）
- [x] 测试 7 条：两次渲染相同 / 标签按序递增 / **相同内容不去重**（这条是「不许按内容做标签名」
      的可执行版本）/ 空串占位 / 段序无关 / 产物无地址与时间 / 两段共用计数器
- [x] Commit：`feat(arm64): 确定性文本汇编 emitter（Task 3）`

**Verify:** `… cargo test -p rgoc-arm64 --test emit_deterministic`（7 条全绿）；`… cargo clippy -p rgoc-arm64 --all-targets -- -D warnings`（0）

---

### Task 4: fixture 装置（Go 源 → 产物 → 比对）

**Files:**
- Create: `rgoc/crates/rgoc-driver/tests/support/mod.rs`、`tests/support/native_harness.rs`
- Create: `rgoc/crates/rgoc-driver/fixtures/native/{hello,ret42,empty}.go`
- Create: `rgoc/crates/rgoc-driver/tests/native_smoke.rs`

**Interfaces:**
- Produces: `native_harness::{build_and_run(name, src) -> Result<Outcome, String>, fixture_root()}`；
  `Outcome { stdout, stderr, exit, asm }`（内部走 `build_cmd::run_build`，**不重实现流水线**）
- Consumes: Task 2 的 `build`；`build_cmd::BuildOutcome` 新增 `artifact: Option<PathBuf>`

- [x] 装置把 fixture 写到 `target/native-fixtures/<name>/`（**不写 `go_source_code/`**），编译、运行、收集三通道
- [x] `Outcome` 带 `asm`（`--keep-asm` 时读回 `<output>.s`），供后续任务做结构断言
- [x] **偏离计划**：三条 fixture 测试用 `#[ignore = "build 无产物（Task 50 接线后去掉）"]`，
      而不是「先全红」。理由：全红的测试留在树里 40 多个 Task，等于让 `cargo test` 长期是红的
      —— 那时没人分得清「本来红」和「刚坏」。另留一条**绿**的 `骨架期_build_未实现时装置必须报错`
      钉住当前诚实行为（装置不许把「没产物」当成「跑出空输出」）
- [x] 三个 fixture：`hello`（13 字节字节面）/ `ret42`（乘法，故意不用常量 —— 常量会被 `fold` 折掉）/ `empty`（空输出 + 退出码 0）
- [x] Commit：`test(arm64): native fixture 装置（Task 4）`

**Verify:** `… cargo test -p rgoc-driver --test native_smoke` ⇒ **1 passed / 3 ignored**；
Task 50 接线后去掉 ignore，三条必须真绿。

---

### Task 5: `run-collide.sh --native` 空壳 + native 语料分类器

**Files:**
- Modify: `tools/run-oracle/run-collide.sh`（加 `--native` 第四列）
- Create: `tools/run-oracle/native-classify.sh`

**Interfaces:**
- Produces: `--native` 列（`build` 产物 vs `--via=hir` vs `go run`）；分类器产出 `tools/run-oracle/native-rejected.txt`（两栏：路径 + 码）
- Consumes: Task 2 的 `build`

- [ ] `--native` 在缺 `rgoc-driver build` 时**明确拒绝**（沿 `--ssa` 的同款纪律：不静默降级）
- [ ] 分类器从 `ssa-rejected.txt` 的补集（46 行）出发，逐个 `build`，落两栏
- [ ] 分类器**只产出**清单，不改语料；输出路径可配（`--out`）
- [ ] Commit：`feat(m7): --native 对撞列与语料分类器骨架（Task 5）`

**Verify:** `bash tools/run-oracle/run-collide.sh --native --list tools/run-oracle/run-selected.txt` ⇒ 明确报「build 未实现」并退出 2

---

### Task 6: 错误通道（`CgCode` → stderr + 退出码）

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/diag.rs`、`rgoc/crates/rgoc-driver/src/build_cmd.rs`
- Test: `rgoc/crates/rgoc-driver/tests/native_diag.rs`

**Interfaces:**
- Produces: `build_cmd` 的三通道（`stdout` / `stderr` / `exit`），`CgCode` 渲染成 `CG-UNSUPPORTED-*` 单行
- Consumes: Task 2 的 `CgCode`

- [ ] `CgCode` 的 `Display` 输出稳定文本（码 + 函数名 + 值号），**不含**地址
- [ ] 不支持项 ⇒ 退出码 **1**（`C9` §5 的「没跑」口径），**不许** abort
- [ ] 测试 `T-NAT-52` 的骨架版：造一个必拒的输入，断言 `exit == 1` 且 stderr 含码
- [ ] Commit：`feat(arm64): 不支持面错误通道（Task 6）`

**Verify:** `… cargo test -p rgoc-driver --test native_diag`（≥2 条）

---

### Task 7: 变异笔记脚本骨架

**Files:**
- Create: `scripts/mutation-m7-notes.md`

**Interfaces:**
- Produces: 变异记录的固定表头（`# | 变异 | 期望红在 | 实测红在 | 复原确认`）

- [ ] 建表，先登记 `T-NAT-57..64` 八行（实测列留空）
- [ ] 写「§0 产物陈旧陷阱」一节，引用 `mutation-m3-notes.md` §0 与 `AGENTS.md` §4.3
- [ ] Commit：`docs(m7): 变异笔记骨架（Task 7）`

**Verify:** `grep -c '^| ' scripts/mutation-m7-notes.md` ⇒ ≥ 10

---

## Phase 1：帧与 ABI（Task 8–17）

### Task 8: `abi::classify`（值 → 槽宽 / 对齐 / 传参类别）

**Files:**
- Create: `rgoc/crates/rgoc-arm64/src/abi.rs`
- Test: `rgoc/crates/rgoc-arm64/tests/abi_classify.rs`

**Interfaces:**
- Produces: `rgoc_types::layout::{layout, field_offset, field_offsets, Layout}`（**新建，`R-8-1`**）；
  `rgoc_arm64::abi::{slot_of, arg_kind, reg_count, SlotClass, Slot, ArgKind}`
- Consumes: `rgoc_types::{TypeTable, TypeId}`、`rgoc_const::Sizes`

> **`R-8-1`（开工即修正）**：设计稿 §7.3 写「偏移一律来自 `rgoc-types` 的布局」，
> 但 M7 开工时 **`rgoc-types` 里没有任何布局代码** —— 解释器用的是字段**下标**
> （`Value::Struct(Rc<Vec<Cell>>)`），从来不需要字节偏移。所以第一步是**把布局建起来**，
> 落点选 `rgoc-types::layout`（不是 `rgoc-arm64`）：M9/M10 也要用，且「唯一来源」才守得住。

- [x] `rgoc-types::layout`：`layout()` / `field_offsets()` / `field_offset()`，覆盖
      Basic/Pointer/Slice/Array/Struct/Interface/Map/Chan/Signature/Named/Alias；
      `Tuple`/`TypeParam`/`Union`/`Invalid`/`[...]T` ⇒ `None`；别名环有 64 层深度守卫
- [x] **每个数字都是实测的**（`go run` + `unsafe.Sizeof/Alignof/Offsetof`，15 条测试）：
      `complex64` 的 align 是 **4** 不是 8 / `[0]int` 是 **0 字节但 align 8** /
      `struct{a int64; b struct{}}` 是 **16**（末尾零大小 +1）/ `struct{b struct{}}` 是 **0**（不 +1）
- [x] `abi::slot_of`：标量一律 8；`complex64` 8 / `complex128` 16；聚合向上取 8 的倍数；
      零大小 ⇒ 0；命名类型按本体分类（`type MyFloat float64` 必须走浮点寄存器）
- [x] `arg_kind` / `reg_count`：`string` 传参占 **2 个 `x`**；`complex64` **1 个 `v`**、
      `complex128` **2 个 `v`**（**`R-8-2`**：C5 §6.1 原写「complex 占 2 个 v」不够精确，已改）
- [x] Commit：`feat(arm64): 对象布局与值分类（Task 8，T-NAT-01/02，R-8-1/R-8-2）`

**Verify:** `… cargo test -p rgoc-types --test layout`（15 条）；`… cargo test -p rgoc-arm64 --test abi_classify`（11 条）

---

### Task 9: `frame::layout`（槽分配 + 帧大小 + 16 对齐）

**Files:**
- Create: `rgoc/crates/rgoc-arm64/src/frame.rs`
- Test: `rgoc/crates/rgoc-arm64/tests/frame_layout.rs`

**Interfaces:**
- Produces: `Frame { slots: BTreeMap<ValueId, Slot { off: i32, size: u32 }>, slot_area: BTreeMap<SlotId, Slot>, tmp_base: i32, frame_size: u32 }`、`layout(f: &SsaFunc, tt: &TypeTable) -> Frame`
- Consumes: Task 8 的 `classify` / `slot_size`；`SsaFunc::{value_types, slot_types, params}`

- [ ] 先写会红的测试：两个相邻槽写入不互相覆盖（用 `[3]int8` 与 `int64` 相邻）；`frame_size % 16 == 0`
- [ ] 值槽按 `ValueId` 升序分配（确定性），偏移从 `-16` 起（`saved fp/lr` 占 +0/+8，槽区在 `fp` 之下）
- [ ] `tmp_base` = 槽区之后的并行拷贝临时区，大小 = 最大边实参个数 × 8
- [ ] `frame_size` = `saved(16)` + 槽区 + 临时区，向上取 16 的倍数
- [ ] Commit：`feat(arm64): 帧布局（Task 9，T-NAT-01/04）`

**Verify:** `… cargo test -p rgoc-arm64 --test frame_layout`（≥4 条）

---

### Task 10: 零值（`slot_types` 进帧清零）

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/frame.rs`、`src/emit.rs`
- Test: `rgoc/crates/rgoc-arm64/tests/frame_zero.rs`

**Interfaces:**
- Produces: `Frame::zero_init(&self, asm: &mut Asm)` —— 按 `slot_types` 的槽逐个 `str xzr` / `memset`
- Consumes: Task 9 的 `Frame`

- [ ] 先写会红的测试：`var a [3]int` 的帧里三个字都是 0（反汇编里能看到清零指令，且不在每个元素上重复清）
- [ ] 实现：8 字节槽用 `str xzr`；聚合按 8 字节步长循环 `str xzr`
- [ ] 零大小槽**不生成**任何指令（`T-NAT-02`）
- [ ] Commit：`feat(arm64): 帧槽零值初始化（Task 10，T-NAT-03）`

**Verify:** `… cargo test -p rgoc-arm64 --test frame_zero`（≥3 条）

---

### Task 11: `FRAME-MAP v1` 渲染

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/frame.rs`
- Test: `rgoc/crates/rgoc-arm64/tests/frame_map.rs`

**Interfaces:**
- Produces: `render_frame_map(f: &SsaFunc, fr: &Frame, live: &Liveness, tt: &TypeTable) -> String`
- Consumes: Task 9 的 `Frame`；`rgoc_ssa::live::{Liveness, analyze}`

- [ ] 先写会红的测试：格式符合 `C5` §4（头两行 + `slot` 行按 `ValueId` 升序 + `safepoint` 行按块号/下标升序）
- [ ] `frame=` 等于 `frame_size`；`slot` 行的 `off`/`size` 来自 `Frame`
- [ ] `safepoint` 的 `offs` 覆盖 `SSA-LIVE v1` 在该位点的**全部**活值（少一个即红）
- [ ] 两次渲染逐字节相同
- [ ] Commit：`feat(arm64): FRAME-MAP v1（Task 11，T-NAT-51）`

**Verify:** `… cargo test -p rgoc-arm64 --test frame_map`（≥4 条）

---

### Task 12: 并行拷贝算法（含环检测）

**Files:**
- Create: `rgoc/crates/rgoc-arm64/src/parallel_copy.rs`
- Test: `rgoc/crates/rgoc-arm64/tests/parallel_copy.rs`

**Interfaces:**
- Produces: `schedule(pairs: &[(Slot, Slot)], tmp: Slot) -> Vec<Move>`，`Move { Load(Slot), Store(Slot) }` 序列；保证**读都在写之前**（或经 `tmp` 打断环）
- Consumes: Task 9 的 `Slot`

- [ ] 先写会红的测试：`[(a←b), (b←a)]` 必须产生 3 条 move（含 tmp）；`[(a←b),(b←c),(c←a)]` 环长 3
- [ ] 无环情形不得多写（`[(a←b),(c←d)]` ⇒ 2 条）
- [ ] 环检测：建「目标 → 源」单射图，沿链走，遇到已访问即环
- [ ] 反向守卫：把 `tmp` 去掉（直接顺序搬）⇒ 交换测试必红
- [ ] Commit：`feat(arm64): 并行拷贝与环检测（Task 12，T-NAT-06/07/08）`

**Verify:** `… cargo test -p rgoc-arm64 --test parallel_copy`（≥6 条）

---

### Task 13: 序言与尾声

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/isel.rs`（新建）
- Test: `rgoc/crates/rgoc-driver/tests/native_prologue.rs`

**Interfaces:**
- Produces: `isel::prologue(&Frame, &mut Asm)` / `isel::epilogue(&Frame, &mut Asm)`
- Consumes: Task 9/10

- [ ] 序言：`stp x29, x30, [sp, #-16]!` → `mov x29, sp` → `sub sp, sp, #(frame_size-16)` → 零值初始化
- [ ] 尾声：`add sp, sp, #(frame_size-16)` → `ldp x29, x30, [sp], #16` → `ret`
- [ ] 断言 `(frame_size - 16) % 16 == 0`（`T-NAT-05` 的结构前提）
- [ ] Commit：`feat(arm64): 序言与尾声（Task 13，T-NAT-05）`

**Verify:** `… cargo test -p rgoc-driver --test native_prologue`（≥3 条，断言反汇编里的立即数）

---

### Task 14: 形参与返回值装配

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/isel.rs`
- Test: `rgoc/crates/rgoc-arm64/tests/abi_params.rs`

**Interfaces:**
- Produces: `isel::bind_params(&Frame, &SsaFunc, &mut Asm)`（入口块 `params` ← `x0..x7`/`v0..v7`）；`isel::load_results(&[ValueId], &Frame, &mut Asm)`
- Consumes: Task 8 的 `arg_reg_kind`

- [ ] 先写会红的测试：`f(a int, b float64, c int)` ⇒ `a→x0`、`b→v0`、`c→x1`（**两个序列独立计数**）
- [ ] 多返回值 `(int, float64, int)` ⇒ `x0`/`v0`/`x1`
- [ ] 聚合返回值：`x8` 是调用方缓冲地址，`str` 进去
- [ ] Commit：`feat(arm64): 形参与返回值装配（Task 14，T-NAT-10）`

**Verify:** `… cargo test -p rgoc-arm64 --test abi_params`（≥5 条）

---

### Task 15: 调用点（含 outgoing 区与对齐）

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/isel.rs`
- Test: `rgoc/crates/rgoc-driver/tests/native_call.rs`

**Interfaces:**
- Produces: `isel::emit_call(&Call, &Frame, &mut Asm)`
- Consumes: Task 14

- [ ] 前 8 个整数实参进 `x0..x7`，第 9 个起写 `[sp, #k*8]`；调用前 `sp` 已 16 对齐
- [ ] 结果按 `dsts` 顺序从 `x0..x7`/`v0..v7` 取回，写进各自槽
- [ ] 测试 `T-NAT-11`：9 个实参的函数，被调方读到的顺序正确
- [ ] 测试 `T-NAT-05`：奇数个 8 字节实参的调用不崩（对齐判据）
- [ ] Commit：`feat(arm64): 调用点与 outgoing 区（Task 15，T-NAT-05/11）`

**Verify:** `… cargo test -p rgoc-driver --test native_call`（≥4 条）

---

### Task 16: 帧与 ABI 的 fixture 收口（甲腿）

**Files:**
- Create: `rgoc/crates/rgoc-driver/fixtures/native/{swap2,rotate3,noclobber,aggrparam,mixret}.go`
- Test: `rgoc/crates/rgoc-driver/tests/native_frame.rs`

**Interfaces:**
- Consumes: Task 9–15 全部

- [ ] `swap2`：`a, b = b, a` 后打印两值（**带真输出**）
- [ ] `rotate3`：`a, b, c = b, c, a`
- [ ] `noclobber`：`[3]int8` 与 `int64` 相邻，写一个读另一个
- [ ] `aggrparam`：`func f(s S)` 改字段不影响调用方
- [ ] `mixret`：`(int, float64, int)` 三个可区分的值
- [ ] Commit：`test(arm64): 甲腿 11 条 fixture 收口（Task 16，T-NAT-01..11）`

**Verify:** `… cargo test -p rgoc-driver --test native_frame`（≥11 条）

---

### Task 17: 帧与 `objdump` 对账工具

**Files:**
- Create: `rgoc/crates/rgoc-driver/tests/support/disasm.rs`

**Interfaces:**
- Produces: `disasm(elf: &Path) -> String`（走容器内 `objdump -d`）；`sub_sp_imm(&str) -> Option<u32>`
- Consumes: Task 11 的 `FRAME-MAP` 文本

- [ ] 解析 `sub sp, sp, #N` 的 N，与 `FRAME-MAP` 的 `frame=` 比对（`T-NAT-04`）
- [ ] 解析 `add x?, sp, #off` 的 off，与 `slot` 行的 `off` 比对（`T-NAT-24/51`）
- [ ] 工具自身先测一遍：喂一段**手写**反汇编文本，断言解析正确（防止「解析器恒返回 None 也算过」）
- [ ] Commit：`test(arm64): objdump 对账工具（Task 17，T-NAT-04/51）`

**Verify:** `… cargo test -p rgoc-driver --test native_frame_map`（≥3 条）

---

## Phase 2：指令选择（Task 18–39）

> **本 Phase 的统一纪律**：每个 `Instr` 变体一条「先红后绿」的 fixture，且 `isel` 的 `match` **不许通配**。

### Task 18: `isel` 框架（穷尽 match）

**Files:**
- Modify: `rgoc/crates/rgoc-arm64/src/isel.rs`
- Test: `rgoc/crates/rgoc-arm64/tests/isel_exhaustive.rs`

**Interfaces:**
- Produces: `isel::func(f: &SsaFunc, fr: &Frame, tt: &TypeTable, asm: &mut Asm) -> Result<(), CgCode>`
- Consumes: Phase 1 全部

- [ ] 写 `match instr { ... }` 的 14 个分支骨架，每个分支先 `todo!()`（编译通过、运行 panic）
- [ ] 测试 `T-NAT-02` 的形态：遍历 `Instr` 的**枚举清单**，断言每个变体都有非 `todo!()` 的分支（用 `std::panic::catch_unwind` 或一个 `visited` 集合）
- [ ] `Slice`/`Make` 分支直接返回 `Err(CgCode::Slice)`（不是 `todo!()`）
- [ ] Commit：`feat(arm64): isel 框架与穷尽分支（Task 18，T-NAT-02）`

**Verify:** `… cargo test -p rgoc-arm64 --test isel_exhaustive`（≥2 条）

---

### Task 19: `Const` 整数

**Files:** Modify `isel.rs`；Test `rgoc/crates/rgoc-driver/fixtures/native/const_int.go`
- [ ] fixture：`print(0, -1, 1<<20, 1<<40, -1<<40)`（带真输出）
- [ ] 实现：`movz`/`movk` 序列（≤4 条）；负值走补码
- [ ] 边界：`i64::MIN`/`i64::MAX`（`printbig.go` 的两个数）
- [ ] Commit：`feat(arm64): Const 整数（Task 19，T-NAT-12）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_const`（≥3 条）

---

### Task 20: `Const` 浮点与复数

**Files:** Modify `isel.rs`、`emit.rs`；Test `fixtures/native/const_float.go`
- [ ] fixture：`print(0.1, 1e300, 1.0/0.0, 0.0/0.0)`（`+Inf`/`NaN` 的文本）
- [ ] 实现：`.rodata` 的 `.double`/`.quad` + `ldr d0, [x?, #off]`；**位模式**精确（`-0.0` 用 `0x8000000000000000`）
- [ ] 测试断言的是**输出字节**，不是「近似相等」
- [ ] Commit：`feat(arm64): Const 浮点与复数（Task 20，T-NAT-13）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_const`（≥5 条）

---

### Task 21: `Const` 字符串（独立标签）

**Files:** Modify `isel.rs`、`emit.rs`；Test `fixtures/native/const_str.go`
- [ ] fixture：`print("")`, `print("a\x00b")`, `print("中文")`
- [ ] 实现：每个字符串值一个 `.Lstr<N>`（**不合并相同内容**，避免标签复用带来的顺序依赖）
- [ ] `(ptr,len)` 两半都进槽；len 按**字节**
- [ ] Commit：`feat(arm64): Const 字符串与 .rodata 标签（Task 21，T-NAT-14）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_const`（≥7 条）

---

### Task 22: `Copy`

**Files:** Modify `isel.rs`；Test `fixtures/native/copy.go`
- [ ] 标量：`ldr`/`str`；聚合：`bl memcpy`
- [ ] fixture：`b := a`（数组）后改 `b` 不影响 `a`
- [ ] Commit：`feat(arm64): Copy（Task 22，T-NAT-15）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_mem`（≥2 条）

---

### Task 23: `Bin` 整数算术

**Files:** Modify `isel.rs`；Test `fixtures/native/bin_int.go`
- [ ] `add`/`sub`/`mul`；按**宽度回绕**（`int8(127)+1 == -128`）
- [ ] fixture 覆盖 `int8`/`int16`/`int32`/`int64` 各一组溢出
- [ ] Commit：`feat(arm64): Bin 整数算术（Task 23，T-NAT-16）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_bin`（≥4 条）

---

### Task 24: `Bin` 除法与取模（含除零检查）

**Files:** Modify `isel.rs`；Test `fixtures/native/bin_div.go` + `fixtures/native/div_zero.go`
- [ ] `sdiv`/`udiv`；`%` 用 `msub` 求余
- [ ] **除零检查**：`cbnz` 跳过、为零则 `bl rt_panic_div_zero`（不返回）
- [ ] `div_zero.go` 断言：退出码 2 + 合并流末尾 `panic: runtime error: integer divide by zero`（`C9` §4.5 的文本规则）
- [ ] 反向守卫：去掉检查 ⇒ `div_zero` 必红（`T-NAT-61` 的预演）
- [ ] Commit：`feat(arm64): Bin 除法与除零检查（Task 24，T-NAT-17）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_bin`（≥7 条）

---

### Task 25: `Bin` 移位与位运算

**Files:** Modify `isel.rs`；Test `fixtures/native/bin_shift.go`
- [ ] `lsl`/`lsr`/`asr`/`and`/`orr`/`eor`
- [ ] 负移位与 ≥ 宽度的移位按 Go 语义（与 `ops` 一致；必要时先 `and` 掩码）
- [ ] Commit：`feat(arm64): Bin 移位与位运算（Task 25，T-NAT-18）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_bin`（≥10 条）

---

### Task 26: `Bin` 比较

**Files:** Modify `isel.rs`；Test `fixtures/native/bin_cmp.go`
- [ ] 有符号（`lt/le/gt/ge`）/ 无符号（`lo/ls/hi/hs`）/ 浮点（`fcmp`）三类；`cmp` + `cset`
- [ ] `NaN` 的六个运算符（除 `!=` 全 `false`）
- [ ] Commit：`feat(arm64): Bin 比较（Task 26，T-NAT-19）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_bin`（≥14 条）

---

### Task 27: `Bin` 浮点

**Files:** Modify `isel.rs`；Test `fixtures/native/bin_float.go`
- [ ] `fadd`/`fsub`/`fmul`/`fdiv`；`float32` 每次运算后 `fcvt s,s`（按 `f32` 舍入）
- [ ] fixture：`float32(1)/3` ⇒ `0.33333334`（`C9` §4.4 的冻结点之一）
- [ ] Commit：`feat(arm64): Bin 浮点（Task 27，T-NAT-20）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_bin`（≥18 条）

---

### Task 28: `Bin` 复数

**Files:** Modify `isel.rs`；Test `fixtures/native/bin_complex.go`
- [ ] `+ - *` 直接展开；`/` 按 Smith 形式（与 `ops` 逐式对齐）
- [ ] fixture：`cplx0.go` 的 `(5+6i)` 四行
- [ ] Commit：`feat(arm64): Bin 复数（Task 28，T-NAT-21）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_bin`（≥22 条）

---

### Task 29: `Un`

**Files:** Modify `isel.rs`；Test `fixtures/native/un.go`
- [ ] `neg`/`mvn`/`fneg`；`-(-0.0)` 位模式仍是 `-0.0`
- [ ] Commit：`feat(arm64): Un（Task 29，T-NAT-22）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_un`（≥3 条）

---

### Task 30: `Convert`

**Files:** Modify `isel.rs`；Test `fixtures/native/convert.go`
- [ ] 宽度截断 `int64→int8`（`sxtb`）/ 符号扩展 `int8→int64` / `scvtf`/`fcvtzs` / `fcvt`（f32↔f64）
- [ ] `float→int` 的截断方向与 `C9` 一致（向零）
- [ ] Commit：`feat(arm64): Convert（Task 30，T-NAT-23）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_conv`（≥8 条）

---

### Task 31: `AddrOf` 帧槽

**Files:** Modify `isel.rs`；Test `fixtures/native/addr_slot.go`
- [ ] `add x, sp, #off`（off 来自 `Frame`）
- [ ] fixture：`p := &a; *p = 2; print(a)` ⇒ `2`（带真输出）
- [ ] Commit：`feat(arm64): AddrOf 帧槽（Task 31，T-NAT-24）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_addr`（≥2 条）

---

### Task 32: `AddrOf` 全局（`adrp`/`add`）

**Files:** Modify `isel.rs`、`emit.rs`；Test `fixtures/native/addr_global.go`
- [ ] `adrp x, sym` + `add x, x, :lo12:sym`（PIE 安全形式，沿 `T-S3-01`）
- [ ] fixture：两个不同全局的地址不相等
- [ ] Commit：`feat(arm64): AddrOf 全局（Task 32，T-NAT-25）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_addr`（≥4 条）

---

### Task 33: `Field`

**Files:** Modify `isel.rs`；Test `fixtures/native/field.go`
- [ ] 偏移来自 `rgoc-types` 的布局（**唯一来源**）；嵌套 `s.a.b` 偏移叠加
- [ ] fixture：`struct{a int8; b int64}` 的 `b` 偏移 = 8
- [ ] Commit：`feat(arm64): Field（Task 33，T-NAT-26）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_addr`（≥6 条）

---

### Task 34: `Index`

**Files:** Modify `isel.rs`；Test `fixtures/native/index.go`
- [ ] 元素地址 = 基址 + `i * elem_size`；`[3]int8` 与 `[3]int64` 各一
- [ ] Commit：`feat(arm64): Index（Task 34，T-NAT-27）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_addr`（≥8 条）

---

### Task 35: `Load` / `Store`（含 nil 检查）

**Files:** Modify `isel.rs`；Test `fixtures/native/load_store.go` + `fixtures/native/nil_deref.go`
- [ ] 1/2/4/8 字节（`ldrb/ldrh/ldr/str` 同族）+ 聚合 `memcpy`
- [ ] nil 检查：`cbz` 跳过、为零则 `bl rt_panic_nil`
- [ ] `nil_deref.go`：退出码 2 + `panic: runtime error: invalid memory address or nil pointer dereference`
- [ ] Commit：`feat(arm64): Load/Store 与 nil 检查（Task 35，T-NAT-28）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_mem`（≥8 条）

---

### Task 36: `Composite`

**Files:** Modify `isel.rs`；Test `fixtures/native/composite.go`
- [ ] 数组/struct 字面量：先清零再逐元素 `str`；keyed 元素按下标落位
- [ ] fixture：`[5]int{0: 1, 3: 4}` ⇒ 其余为 0
- [ ] Commit：`feat(arm64): Composite（Task 36，T-NAT-29）`

**Verify:** `… cargo test -p rgoc-driver --test native_isel_mem`（≥10 条）

---

### Task 37: `Slice` / `Make` 拒绝

**Files:** Modify `isel.rs`；Test `rgoc/crates/rgoc-driver/tests/native_reject.rs`
- [ ] 两个变体都返回 `Err(CgCode::Slice)`
- [ ] fixture：`s := []int{1,2,3}` ⇒ 退出码 1 + `CG-UNSUPPORTED-SLICE`，**不 abort**
- [ ] Commit：`feat(arm64): Slice/Make 显式拒绝（Task 37，T-NAT-52）`

**Verify:** `… cargo test -p rgoc-driver --test native_reject`（≥2 条）

---

### Task 38: `Term` 五族

**Files:** Modify `isel.rs`；Test `fixtures/native/term_{goto,if,ret,panic,unreachable}.go`
- [ ] `Goto`（并行拷贝 + `b`）、`If`（`cbnz` + 两条边各自的拷贝）、`Return`、`Panic`（不返回）、`Unreachable`（`brk #1`）
- [ ] `if` 的边实参拷贝**必须在跳转前**完成
- [ ] Commit：`feat(arm64): Term 五族（Task 38，T-NAT-30）`

**Verify:** `… cargo test -p rgoc-driver --test native_term`（≥5 条）

---

### Task 39: 指令选择完备性收口（乙腿）

**Files:** Test `rgoc/crates/rgoc-arm64/tests/isel_coverage.rs`
- [ ] 断言 14 个 `Instr` + 5 个 `Term` 每个都有**非 `todo!()`** 的分支，且至少一条 fixture 走到
- [ ] 反向守卫：给任一变体加回 `todo!()` ⇒ 该测试必红
- [ ] Commit：`test(arm64): 乙腿 19 条收口（Task 39，T-NAT-12..30）`

**Verify:** `… cargo test -p rgoc-arm64 --test isel_coverage`（≥2 条）

---

## Phase 3：runtime（Task 40–48）

### Task 40: `rt.c` 骨架 + 编译与链接

**Files:**
- Create: `runtime/native/aarch64-unknown-linux-gnu/rt.c`、`rt.h`
- Modify: `rgoc/crates/rgoc-driver/src/build_cmd.rs`（调 `clang -ffreestanding -nostdlib -c`）
- Test: `rgoc/crates/rgoc-driver/tests/native_rt_link.rs`

**Interfaces:**
- Produces: `rt.o`（与 `prog.s` 一起链）；`memcpy` / `memset` 自实现
- Consumes: Task 2 的 `build`

- [ ] `rt.c` 只用 `-ffreestanding` 下可用的东西（无 `#include <stdio.h>` 等）
- [ ] 实现 `memcpy` / `memset`（clang 对聚合拷贝可能发这两个调用）
- [ ] `build_cmd` 先编 `rt.o` 再链；`rt.o` 缓存到 `target/native-rt/`
- [ ] 测试 `T-NAT-39`：链接产物无未定义符号（`readelf -s` 里没有 `UND` 的 `memcpy`）
- [ ] Commit：`feat(rt): rt.c 骨架与链接（Task 40，T-NAT-39）`

**Verify:** `… cargo test -p rgoc-driver --test native_rt_link`（≥2 条）

---

### Task 41: `rt_write` / `rt_exit`

**Files:** Modify `rt.c`；Test `fixtures/native/write_raw.go`
- [ ] `svc #0`：`write`(64) / `exit_group`(94)
- [ ] fixture：`print("x")` 走 `rt_write(1, ...)`
- [ ] Commit：`feat(rt): write/exit（Task 41，T-NAT-31）`

**Verify:** `… cargo test -p rgoc-driver --test native_rt_io`（≥2 条）

---

### Task 42: `rt_print_i64` / `rt_print_u64`

**Files:** Modify `rt.c`；Test `fixtures/native/print_int.go`
- [ ] 十进制含负号；`i64::MIN` 不溢出（先转 `u64` 再取负）
- [ ] fixture：`print(0, -1, -9223372036854775808)`（`printbig.go` 的形态）
- [ ] Commit：`feat(rt): itoa（Task 42，T-NAT-31）`

**Verify:** `… cargo test -p rgoc-driver --test native_rt_print`（≥4 条）

---

### Task 43: `rt_print_f64`（最短往返 + `%g`）

**Files:** Modify `rt.c`；Test `fixtures/native/print_float.go`
- [ ] 最短往返（Grisu/Ryu 类）；`exp < -4 || exp >= 6` 走指数；指数**至少两位**
- [ ] `NaN` / `+Inf` / `-Inf`
- [ ] fixture 就是 `C9` §4.4 的六个点（`1e+06` / `0.0001` / `1e-05` / `1e-07` / `0.33333334`）
- [ ] 性质测试：随机 200 个 `f64`，native 与 `format_float` 逐字节比（这条是 `T-NAT-34` 的加强版）
- [ ] 反向守卫：改成「固定 17 位再裁尾零」⇒ `0.0001` 必红（`T-NAT-60` 的预演）
- [ ] Commit：`feat(rt): ftoa 最短往返（Task 43，T-NAT-34）`

**Verify:** `… cargo test -p rgoc-driver --test native_rt_print`（≥10 条）

---

### Task 44: `rt_print_complex`

**Files:** Modify `rt.c`；Test `fixtures/native/print_complex.go`
- [ ] `(re+imi)` / `(re-imi)`；虚部取绝对值后格式化
- [ ] fixture：`complex(-1,-2)` ⇒ `(-1-2i)`；`complex(5,6)` ⇒ `(5+6i)`
- [ ] Commit：`feat(rt): 复数打印（Task 44，T-NAT-35）`

**Verify:** `… cargo test -p rgoc-driver --test native_rt_print`（≥12 条）

---

### Task 45: `print` / `println` 分隔规则（生成器侧）

**Files:** Modify `rgoc/crates/rgoc-arm64/src/isel.rs`；Test `fixtures/native/print_sep.go`
- [ ] `print`：无分隔；`println`：空格分隔 + 末尾 `\n`；零实参 `println()` ⇒ `\n`
- [ ] 分隔符由生成器插 `rt_write` 调用（runtime 不认识「第几个实参」）
- [ ] Commit：`feat(arm64): print/println 分隔规则（Task 45，T-NAT-36）`

**Verify:** `… cargo test -p rgoc-driver --test native_rt_print`（≥16 条）

---

### Task 46: panic 通道

**Files:** Modify `rt.c`、`isel.rs`；Test `fixtures/native/panic_{str,int,nil,div,nilptr}.go`
- [ ] `rt_panic_str` / `rt_panic_i64` / `rt_panic_nil` / `rt_panic_div_zero` / `rt_panic_nilptr`
- [ ] 统一出口：写 `panic: <值>\n` 到 **fd 2**，`exit(2)`
- [ ] 五个 fixture 各断言「退出码 2 + 合并流末尾那一行」（不比 traceback）
- [ ] Commit：`feat(rt): panic 通道与三个检查点（Task 46，T-NAT-37）`

**Verify:** `… cargo test -p rgoc-driver --test native_panic`（≥5 条）

---

### Task 47: arena

**Files:** Modify `rt.c`、`isel.rs`；Test `fixtures/native/arena.go` + `arena_oom.go`
- [ ] `.bss` 的 1 MiB 块 + bump 指针；分配前 `memset` 清零
- [ ] `new(T)` 与字符串拼接都走 `rt_arena_alloc`
- [ ] `arena_oom.go`：循环 `new` 直到耗尽 ⇒ `panic: out of arena` + 退出码 2
- [ ] Commit：`feat(rt): 静态 arena（Task 47，T-NAT-38）`

**Verify:** `… cargo test -p rgoc-driver --test native_arena`（≥4 条）

---

### Task 48: 丙腿收口（runtime 与输出）

**Files:** Test `rgoc/crates/rgoc-driver/tests/native_rt.rs`
- [ ] 把 `T-NAT-31..39` 的九条各钉一条测试（缺哪条一眼看得出）
- [ ] 断言 `readelf -d` 里没有来自 libc 的符号引用（`NEEDED` 允许存在）
- [ ] Commit：`test(rt): 丙腿 9 条收口（Task 48，T-NAT-31..39）`

**Verify:** `… cargo test -p rgoc-driver --test native_rt`（≥9 条）

---

## Phase 4：startup 与链接（Task 49–53）

### Task 49: `_start` 与 init 顺序

**Files:** Modify `rgoc/crates/rgoc-arm64/src/emit.rs`、`isel.rs`；Test `fixtures/native/init_order.go`
- [ ] `_start`：`bl rt_init_arena` → 按 `Program::init_order` 逐个 `bl init$k` → `bl main` → `mov x0,#0` → `bl rt_exit`
- [ ] 结构断言：`bl` 序列与 `init_order` 逐位一致（不是看输出）
- [ ] 语义断言：包变量依赖顺序（`var a = b + 1; var b = 2`）结果正确
- [ ] Commit：`feat(arm64): _start 与 init 顺序（Task 49，T-NAT-44）`

**Verify:** `… cargo test -p rgoc-driver --test native_startup`（≥3 条）

---

### Task 50: `build` 子命令接线

**Files:** Modify `rgoc/crates/rgoc-driver/src/build_cmd.rs`、`cli.rs`
- [ ] 默认输出：与源文件同目录的 `<stem>`（无扩展名），`-o` 覆盖
- [ ] `--emit=asm` 只出 `.s`；`--keep-asm` 保留中间件
- [ ] 通道：诊断 → stderr + 1；工具链失败 → stderr + 2；成功 → 静默 + 0
- [ ] Commit：`feat(driver): build 子命令接线（Task 50，T-NAT-47）`

**Verify:** `… cargo test -p rgoc-driver --test native_cli`（≥6 条）

---

### Task 51: 可复现（`.s` 与 ELF）

**Files:** Test `rgoc/crates/rgoc-driver/tests/native_repro.rs`
- [ ] 同一输入两次 `--emit=asm` 逐字节相同
- [ ] 同一份 `.s` 两次链接 ELF 逐字节相同（`-Wl,-s`）
- [ ] 反向守卫：去掉 `-Wl,-s` ⇒ ELF 比较必红（证明旗标是必需的，`T-NAT-49`）
- [ ] Commit：`test(arm64): 可复现两向（Task 51，T-NAT-48/49）`

**Verify:** `… cargo test -p rgoc-driver --test native_repro`（≥4 条）

---

### Task 52: `file` 判据 + 工具链失败不吞

**Files:** Test `rgoc/crates/rgoc-driver/tests/native_artifact.rs`
- [ ] `file` 输出必须**同时**含 `ELF 64-bit LSB` 与 `ARM aarch64`
- [ ] 人为给非法 `.s`：`clang` 的 stderr 原样带出、退出码 2（`T-NAT-53`）
- [ ] Commit：`test(arm64): 产物判据与失败通道（Task 52，T-NAT-50/53）`

**Verify:** `… cargo test -p rgoc-driver --test native_artifact`（≥4 条）

---

### Task 53: hello 端到端

**Files:** Test `rgoc/crates/rgoc-driver/tests/native_hello.rs`
- [ ] `hello.go` ⇒ stdout 精确 `hello, world\n`、stderr 空、退出码 0（对齐语料的 13 字节）
- [ ] 这是 `NAT-1`/`NAT-5` 的第一个端到端证据
- [ ] Commit：`test(arm64): hello 端到端（Task 53，NAT-1）`

**Verify:** `… cargo test -p rgoc-driver --test native_hello`（≥2 条）

---

## Phase 5：集成与切片（Task 54–60）

### Task 54: 三方差分装置接线

**Files:** Modify `tools/run-oracle/run-collide.sh`（`--native` 接实）
- [ ] `--native` 列：`build` 产物 vs `--via=hir` vs `go run`
- [ ] 失败时保存：源文件、`.s`、两侧输出、期望、`objdump` 片段
- [ ] Commit：`feat(m7): --native 三方差分接线（Task 54，T-NAT-41）`

**Verify:** `bash tools/run-oracle/run-collide.sh --native --list tools/run-oracle/run-selected.txt`（此时期望部分红）

---

### Task 55: 切片实测与冻结（`N'`/`M'`）

**Files:** Modify `tools/run-oracle/native-classify.sh`；Create `tools/run-oracle/native-rejected.txt`；Test `rgoc/crates/rgoc-driver/tests/native_slice.rs`
- [ ] 分类器跑 46 个，落两栏（路径 + 码）
- [ ] 冻结值写进 `native-rejected.txt` 头部注释与 `native_slice.rs` 的常量
- [ ] 两向断言：`N'+M'==46`；拒绝集合 == 实测集合；码 ∈ `CgCode::ALL`
- [ ] **4 个有输出的文件必须在 `N'` 里**（`T-NAT-42` 的硬约束），否则写明原因并登记
- [ ] Commit：`test(m7): native 切片冻结（Task 55，T-NAT-40，R-6-1 同款）`

**Verify:** `… cargo test -p rgoc-driver --test native_slice`（≥4 条）

---

### Task 56: 有输出 / 空输出分栏报告

**Files:** Modify `run-collide.sh`（汇总行分两栏）；Test 并入 `native_slice.rs`
- [ ] 汇总行必须写「有输出 a/4 + 空输出 b/42」，**不许**只写「N'/N' 绿」
- [ ] 空输出栏的结论文案固定为「跑完 / 退出码 0 / 两流皆空」
- [ ] Commit：`feat(m7): 分栏汇总（Task 56，T-NAT-42/43）`

**Verify:** 汇总行含 `有输出` 与 `空输出` 两个词

---

### Task 57: 指针 / 全局 / 复合语料

**Files:** Test `rgoc/crates/rgoc-driver/tests/native_corpus_groups.rs`
- [ ] 按 `M7-tests.md` §2.1 的分组，把 `N'` 里的文件按「指针写穿 / 全局 / 复合字面量」分类，逐组报结论
- [ ] 每组的绿必须**点名文件**，不许只报组名
- [ ] Commit：`test(m7): 语料分组结论（Task 57，T-NAT-45/46）`

**Verify:** `… cargo test -p rgoc-driver --test native_corpus_groups`（≥3 条）

---

### Task 58: 不支持面 fixture（含两条守卫）

**Files:** Test `rgoc/crates/rgoc-driver/tests/native_unsupported.rs`
- [ ] `U-M7-01`：slice fixture ⇒ 码 + 退出码 1
- [ ] `U-M7-04`：arena 耗尽 ⇒ `panic: out of arena` + 退出码 2
- [ ] `U-M7-02/03`：**守卫测试** —— 断言这两个码**不出现**、拒绝来自 M6（`SSA-UNSUPPORTED-FUNC-VALUE` / `IFACE-DYN`）
- [ ] Commit：`test(m7): 白名单 4 条（Task 58，T-NAT-52）`

**Verify:** `… cargo test -p rgoc-driver --test native_unsupported`（≥4 条）

---

### Task 59: `M7-manifest.json` 填实 + 对账

**Files:** Create `docs/milestones/M7-manifest.json`；Test `rgoc/crates/rgoc-driver/tests/manifest_m7.rs`
- [ ] 填 `gate`（`NAT-1..9` 的实际 verdict）/ `test_ids`（64 条）/ `unsupported`（4 条）/ `corpus_lists`（`N'`/`M'`）/ `budget`（含 `measured`）/ `open_gaps`
- [ ] 对账四类谎（沿 `R-50-1`）：标 pass 却自认未兑现 / 非 pass 无缺口 / 未全绿自称 done / done 但报告不存在
- [ ] 每条门禁的 verdict 必须能指到 `test_ids` 里的至少一条
- [ ] Commit：`test(m7): manifest 填实与对账（Task 59，NAT-9）`

**Verify:** `… cargo test -p rgoc-driver --test manifest_m7`（≥10 条）

---

### Task 60: 变异 8 条

**Files:** Modify `scripts/mutation-m7-notes.md`
- [ ] 逐条实施 `T-NAT-57..64`：**先 `cargo clean -p`**，跑，记实测红点，复原，再跑确认绿
- [ ] 每条记录「期望红在 / 实测红在 / 复原确认」三列
- [ ] 与预测不符的，改 `M7-tests.md` §3.6 的预测并留痕（沿 §8 纪律 3）
- [ ] Commit：`test(m7): 8 条变异全红（Task 60，T-NAT-57..64）`

**Verify:** `grep -c '| 红 |' scripts/mutation-m7-notes.md` ⇒ 8；`… cargo test --workspace` 复原后全绿

---

## Phase 6：收口（Task 61–65）

### Task 61: 四条 cargo + 五条自检

- [ ] `fmt --check` / `check --workspace --all-targets` / `clippy --all-targets -D warnings` / `test --workspace --no-fail-fast` 全 exit 0
- [ ] 五条自检：m0（232）/ m1 / lex-golden（151）/ ast-golden / ast-testdata
- [ ] **汇总计数自己算**（`awk` 按 `passed;`/`failed;`/`ignored;`），不拿 `| tail` 的尾巴当证据
- [ ] Commit：`chore(m7): 四条 cargo + 五条自检（Task 61，T-NAT-54）`

**Verify:** 四条命令 exit 0；汇总 `0 failed`

---

### Task 62: M6 零回归

- [ ] `run-collide.sh --ssa` 仍 46 绿 / 23 跳过 / 0 红
- [ ] `run --via={hir,ssa}` 仍 46/46 一致
- [ ] workspace 总数 **≥ 1859**
- [ ] Commit：`test(m7): M6 零回归（Task 62，T-NAT-55）`

**Verify:** 三条命令的计数与 M6 收口值逐项相同

---

### Task 63: `M7-report.md`（交付 C）

**Files:** Create `docs/milestones/M7-report.md`
- [ ] 覆盖矩阵（roadmap §M7 十二句 × 四维）
- [ ] **有输出 4 / 空输出 42 分栏**的实测结论（这是本阶段最容易被误读的数）
- [ ] `N'`/`M'` 实测与预估（40/6）的差异及原因
- [ ] 偏离登记（`R-*`）+ 缺口 `G-M7-*` + 接手指南
- [ ] Commit：`docs(m7): 交付 C 报告（Task 63）`

**Verify:** 报告含 `有输出` / `空输出` / `N'` / `open_gaps` 四节

---

### Task 64: 文档回写

- [ ] `C5` 校订（把执行期 `R-*` 修订折回正文）；`C9` 补 native 消费者一节；`C4`/`C10` 不改（写明理由）
- [ ] `GAP-01.md` 补 `PIPE-19/20/21/22/24` 的 M7 落点
- [ ] `03-roadmap.md` §M7 状态块；`AGENTS.md` §0 一句话状态；`docs/README.md` 四行
- [ ] Commit：`docs(m7): 契约升正式与文档回写（Task 64）`

**Verify:** `python3 scripts/check-m1-consistency.py` 全过；README 的 M7 行不再是 🟡

---

### Task 65: 全量回归与 manifest 翻 `done`

- [ ] 重跑 Task 61/62 的全部命令
- [ ] `M7-manifest.json` 的 `phase_summary` 翻 `done`（**只在全绿后**）
- [ ] `… cargo test -p rgoc-driver --test manifest_m7` 全绿
- [ ] Commit：`chore(m7): 全量回归 + manifest 翻 done（Task 65，M7 收口）`

**Verify:** 四条 cargo exit 0；`manifest_m7` 全绿；`git status` 干净

---

## 附：任务与门禁的对应

| 门禁 | 主要 Task |
|---|---|
| `NAT-1` | 51 / 52 / 53 |
| `NAT-2` | 18–39 |
| `NAT-3` | 9 / 12 / 13 / 14 / 15 / 16 |
| `NAT-4` | 54 / 55 / 56 / 57 |
| `NAT-5` | 41–46 / 48 |
| `NAT-6` | 10 / 22 / 31–36 / 47 |
| `NAT-7` | 9 / 11 / 17 |
| `NAT-8` | 37 / 58 |
| `NAT-9` | 59 / 60 / 61 / 62 / 63 / 64 / 65 |
