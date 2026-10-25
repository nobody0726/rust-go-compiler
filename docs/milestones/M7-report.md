# M7 报告 —— native 代码生成（arm64 MVP）

> **状态**：✅ **已收口**（2026-10-25）—— 九条门禁 **9/9 pass**（其中一条在收口期才补齐）
> **上游**：`M7-design.md`（`D-M7-1..16` / `F1..F18`）/ `M7-tests.md`（64 个 `T-NAT-*`）/ `M7-plan.md`（Task 1–65）
> **机器可读事实**：`M7-manifest.json`（由 `manifest_m7.rs` 对账，9 种合成反例）
> **下游**：M8（泛型）/ M9（寄存器分配与对象模型）/ M10（GC 与扫描器）

---

## 1. 一句话结论

**`rgoc build hello.go` 能生成 aarch64 ELF 并跑对。** 入选语料里能编的 **39 个全部与官方
`go run` 的输出逐字节一致**，7 个因切片面被静态拒绝（`M'`），**0 个坏**。

真正值得记的不是这个数字，而是：**39 个全绿之后仍然有 19 个真错**从语料里躲了过去
（其中「帧太小」让 32 个文件照样绿）。见 §5。

---

## 2. 读数（本报告写就时实测）

| 项 | 值 |
|---|---|
| workspace 测试 | **1974 passed / 0 failed / 1 ignored**（195 suite） |
| 四条 cargo | `fmt --check` 0 / `check --workspace --all-targets` 0 / `clippy --all-targets -D warnings` 0 / `test --workspace --no-fail-fast` 0 failed |
| 语料（native） | `有输出 3/3 + 空输出 36/36（ok=39 skip=7 bad=0 / 46）` |
| 语料（对照官方 `.exp`） | **39/39 逐字节相同** |
| `M'` | 7 个，全部 `CG-UNSUPPORTED-SLICE`（`native-rejected.txt`） |
| M6 零回归 | `run-collide --run` 69 绿 / `--ssa` 46 绿 23 跳过 0 红 |
| 变异 | **8 条全红**（`scripts/mutation-m7-notes.md`） |
| `DiagCode` | 仍 **103**（M7 的码走 crate-local `CgCode`，3 个变体） |
| 自检 | 5 条全过（m0 232 条 / m1 / lex-golden 151 / ast-golden / ast-testdata / ast-stdlib） |

---

## 3. 交付物

| 类别 | 内容 |
|---|---|
| 新 crate | `rgoc-arm64`（`abi` / `frame` / `isel` / `module` / `emit` / `parallel_copy` / `diag`） |
| 扩 crate | `rgoc-types`（`layout`：size/align/field_offsets + `array_len_of`）、`rgoc-driver`（`build` 子命令）、`rgoc-ssa`（修 `live` 的漏根缺陷） |
| runtime | `runtime/native/aarch64-unknown-linux-gnu/rt.c`（freestanding，零 libc，24 个符号） |
| 契约 | `C5-abi.md` **升正式**（§6 调用约定 / §7 帧布局 / §8 `FRAME-MAP v1` / §9 确定性） |
| 装置 | `run-native.sh` + `native-input.txt` + `native-rejected.txt` + `mutation-m7.sh` + `mutation-m7-notes.md` |
| 测试 | `native_smoke` / `native_float` / `native_cli` / `native_args` / `native_artifacts` / `native_isel_cover` / `native_frame_map` / `native_corpus_run` / `native_slice` / `native_unsupported` / `manifest_m7` |

---

## 4. 九条门禁

| 门禁 | 判据 | verdict | 证据（测试文件名，不列 ID —— `R-59-1`） |
|---|---|---|---|
| `NAT-1` | 产物与工具链 | **pass** | `emit_deterministic`（7）/ `native_artifacts::t_nat_48`（同一份源码两次构建：`.s` 与 ELF **都**逐字节相同） |
| `NAT-2` | 指令选择完备 | **pass** | `native_isel_cover`（13 组覆盖 17 个可产生的变体）/ `match` 无通配 / `Slice` 走拒绝面 |
| `NAT-3` | 调用与帧 | **pass** | `parallel_copy`（10）/ `native_args`（5）/ `native_corpus_run`（39 含递归与多返回值） |
| `NAT-4` | 三方差分 | **pass** | `run-native.sh`：`有输出 3/3 + 空输出 36/36（ok=39 skip=7 bad=0）`；期望值来自官方 `go run` |
| `NAT-5` | 输出字节 | **pass** | `native_float`：`C9` §4.4 六个冻结点 + 与解释器对撞 20 个刁钻值 |
| `NAT-6` | 基础对象 | **pass** | `native_artifacts`（结构体 16 B / 数组 24 B 拷贝后互不影响，变异实测变红）/ `zerosize` 在 `N'` |
| `NAT-7` | 帧布局与 stack map | **pass** | `frame_layout`（11）/ `frame_map`（6）/ `frame_zero`（5）/ `native_frame_map`（**与 objdump 双向对账**） |
| `NAT-8` | 不支持面 | **pass** | `native_unsupported`（4）：码 + 退出码 1/2 + 守卫 |
| `NAT-9` | 回归与收口 | **pass** | 本节 §2 的四条 cargo + 五条自检 + M6 零回归 + `C5` 折回 + GAP-01 补 + 本报告 + manifest |

---

## 5. 十九条真错（这一节是 M7 最有价值的部分）

全部**先跑出真红/真错**再修。按「为什么语料抓不到」分组 —— 这才是可复用的经验。

### 5.1 语料抓不到的（**39 全绿时仍然错**）

| 错 | 为什么 39 个绿了也没抓到 |
|---|---|
| **帧太小**（`R-13-4`）：`sub sp` 只减到 `frame_size - 16`，`sp` 比最低槽高 16 B，被调方的 `stp x29,x30,[sp,#-16]!` 正好盖住最后一次写入的槽 | 只在「函数里有调用」**且**「最低槽在调用后还要读」时现形。46 个里 32 个两者不同时满足 |
| **9 个以上实参**（`R-13-6`）：第 9 个起写 `[sp, #k]`，那是**本方帧内部** | 语料里几乎没有 ≥9 实参的函数 |
| **移位不钳制**（Go 规定 `n ≥ 位宽 ⇒ 0`，硬件只取低 6 位） | 只有一个文件（`ken/shift.go`）有 `<< 1025` 这种计数，而它**输出为空** —— 错的时候也只是「多打了 3 行」 |
| **聚合按值形参**（`G-M7-08`，**上游 M6 的表示问题**，未修） | 语料里没有一个文件按值传结构体 |

### 5.2 静默错（不崩、不报，只是结果不对）

字符串比较/拼接是指针运算 · `len/cap/min/max/real/imag` 是空实现（`dst` 永不写）·
形参绑定漏第二字（`string` 丢 `len`）· 窄类型符号扩展用 `sxtw` 而非自身宽度 ·
比较按 `dst`（`bool`）分类导致浮点比较变成位模式比大小 · 整数值的浮点常量被当整数装槽 ·
聚合 `==` 没实现 · 边上并行拷贝只搬一个字 · 字符串下标按 8 字节步长 ·
`panic(浮点)` 打位模式 · 方法接收者被丢 · 调用点按**实参**类型选寄存器（`R-13-3`）·
`complex64` 当 1 个 `v` 传（`R-13-2`）· `extra_slots` 没进 `slot_types`（`&CompositeLit` 无帧存储）·
地址逃逸（`R-13-5`）· `rt_zerobase` 两处定义 · `CgCode` 退出码 2 应为 1 · 切片面按指令漏判

### 5.3 装置自己在说谎（这一类最危险）

1. `run-native.sh` 用 `basename` 做产物名 ⇒ `test/ken/convert.go` 的诊断覆盖同名文件 ——
   清单里的「拒绝原因」是**另一个文件**的。
2. `if ! cmd; then rc=$?` —— `!` 之后 `$?` 恒 0，于是所有静态拒绝被记成「坏」。
3. `objdump -d` 的行是「地址 / 机器码 / 助记符 / 操作数」，按「取前两段」会把**机器码**
   当助记符 ⇒ 帧图对账一个偏移都解析不出来（看起来像码错了）。

---

## 6. 执行期修订（10 条契约级）

| 编号 | 内容 |
|---|---|
| `R-1-1` | 设计稿写「新建 `C11-native-abi.md`」，实际应**升 `C5` 为正式**（C5 早就存在且文件头写明归 M7） |
| `R-8-1` | `rgoc-types` **没有任何布局代码**（解释器用字段下标），要先建 `rgoc-types::layout` |
| `R-8-2` | ~~`complex64` 占 1 个 `v`~~ —— **被 `R-13-2` 推翻** |
| `R-11-1` | **M6 的真错**：`live.rs` 的 safepoint 反向扫描漏掉「只在终止符里被读」的值 ⇒ 帧图漏根 |
| `R-11-2` | `C5` §8 的 `type=` 改 `t<id>` |
| `R-13-1` | 无类型常量按 Go **默认类型**取布局与分类 |
| `R-13-2` | `complex64` 是 **HFA** ⇒ **2 个 `v`**（推翻 `R-8-2`） |
| `R-13-3` | 调用点的寄存器分类由**被调方的形参**决定；精度不同在边界 `fcvt` |
| `R-13-4` | 帧大小：`sub sp` 必须覆盖到**最低槽** |
| `R-13-5` | 逃逸的取址槽进 arena（保守判定：`return` / 传给非内建函数 / 写进全局或堆） |
| `R-13-6` | 落位表 `place_args` + `Frame::outgoing` + 栈实参从 `[x29, #16+off]` 读 |
| `R-13-7` | `Instr::Copy` 与 `Term::Unreachable` **没有生产者**（`build.rs` 从不构造） |
| `R-13-8` | `FRAME-MAP` 的 `# func` 行尾补 `sym=fn<id>` |
| `R-59-1` | manifest 不单列 64 条 `test_ids`：`evidence` 直接点名测试**文件名** |

---

## 7. 缺口

### 关闭（5 条）

`G-M7-01`（9 实参 outgoing 区）/ `G-M7-02`（FRAME-MAP vs objdump）/ `G-M7-03`（逐变体 fixture）/
`G-M7-04`（聚合值语义）/ `G-M7-05`（ELF 可复现）—— 证据在 `M7-manifest.json` 的 `closed_gaps`。

### 未关（2 条）

| id | 内容 | owner |
|---|---|---|
| `G-M7-06` | ~~报告未写~~ → **本文件** | — |
| `G-M7-08` | **聚合按值形参**在 SSA 层就没有正确落位：`build.rs` 把 `func f(s S)` 的形参类型记成 `*S`、home 槽存指针，但 `s.N` 的 place lowering 仍 `addr-of 槽` 再 `field` ⇒ 读出来是指针的位模式、写进去覆盖指针 | M6（表示）/ M9（对象模型） |

`G-M7-08` 是**上游**问题，M7 侧**没有**写「把错误行为钉成期望值」的用例 ——
`native_args.rs` 顶部写清了为什么不写。

---

## 8. 三条分别声明

**做了什么**：SSA → arm64 文本汇编 → clang → ELF；每值一个栈槽；零 libc runtime
（`ftoa` 是最短往返的 Dragon4 free-format）；静态 arena + `zerobase`；`FRAME-MAP v1` 真产物
并与 objdump 双向对账；A 的切片按**类型**统一拒；堆栈实参、`complex64` HFA、字符串比较/拼接、
`string(i)` 的 UTF-8、聚合 `==`、`panic(浮点)` 全部走 runtime。

**没做什么**（有意）：寄存器分配与 spill（`PIPE-20`，M9）；自研 assembler/linker（`D-M7-1` 放弃）；
切片全集（`U-M7-01`，M9b）；GC 与扫描器（M10）；聚合按值形参（上游 `G-M7-08`）；
`safepoint` 活值在语料上逐点验。

**不知道什么**（诚实）：换一个 clang 版本后 ELF 是否仍逐字节可复现（当前只在 14.0.6 验过）；
arena 只 bump 不回收，长循环程序在**真实**负载下会不会假性耗尽（只有 8 MiB 与一条 fixture）。

---

## 9. 接手指南

1. **先跑这四条**（都在容器内）：
   ```bash
   RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work && bash tools/run-oracle/run-native.sh tools/run-oracle/native-input.txt'
   RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo test -p rgoc-driver --test native_corpus_run'
   RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work && bash scripts/mutation-m7.sh'
   RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && cargo test --workspace --no-fail-fast'
   ```
2. **改 `isel` 之前读**：`bind_params` / `pass_args` 都走 `frame::callee_param_tys` + `abi::place_args`
   的**同一张表**。各写一遍就是 `string` 两字与 `complex64` 两 `s` 错位，而症状是
   「参数看起来对、比较全错」。
3. **加 `Instr`/`Term` 变体**：`match` 无通配 ⇒ 编译不过，逼你决定它的位置。
   加完记得在 `native_isel_cover.rs` 的表里加一行 —— 否则「17 个可产生」这个分母就旧了。
4. **改帧布局**：`native_frame_map` 会把帧图与 objdump 对上；它红的时候先看
   「负偏移 X 不在帧图的槽偏移里」—— 那是表与代码错位，不是码算错。
