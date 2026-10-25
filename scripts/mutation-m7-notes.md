# M7 变异探针记录

> `M7-plan.md` Task 60。**8 条**探针，全部实证变红并复原。
> 其中 4 条（`M-M7-05..08`）在执行期逐条手工跑过，4 条（`M-M7-01..04`）固化成
> `scripts/mutation-m7.sh`，方便每次收口重跑。

## 纪律（M6 的血）

1. **变异前 `cargo clean -p <crate>`**：绑定挂载上 cargo 的 mtime 判据不可靠，
   不 clean 会跑到旧产物上 —— 得到「绿色」的假结论（`mutation-m3-notes.md` §0）。
2. **同一调用内完成变异与恢复**：容器 `/tmp` 调用即焚。
3. 看到 `test result: FAILED` 才算有效。**全绿 = 判据没牙**，要改判据而不是改代码。
4. 变异必须**可编译**。编译不过的变异证明不了任何事（它测的是编译器）。

## 八条

| # | 变异 | 期望红的判据 | 结果 |
|---|---|---|---|
| `M-M7-01` | `sub_sp_imm` 从 `frame_size - 16` 改成 `- 32`（帧太小） | `frame_layout`（`sub_sp_imm()` 的绝对值断言） | **红** 8 passed / 3 failed |
| `M-M7-02` | `place_args` 的整数阈值 `8` → `6`（AAPCS64 是 `x0..x7`） | `abi_classify::place_args_*`（**直接断表**） | **红** 13 passed / 3 failed |
| `M-M7-03` | `bind_params` 的 `Int` 只存第一个字（`string` 丢 `len`） | `native_args`（`str_args`） | **红** 4 passed / 1 failed |
| `M-M7-04` | 字符串比较退回指针比较（`is_string_ty` 分支禁用） | `native_corpus_run` | **红** 0 passed / 1 failed |
| `M-M7-05` | 并行拷贝改成顺序搬（`parallel_copy::schedule` 直通） | `parallel_copy`（模拟执行判据） | **红** 6 / 10 |
| `M-M7-06` | `max_outgoing` 恒返回 `0`（不给 outgoing 区） | `native_args::twelve_args` | **红** 4 passed / 1 failed |
| `M-M7-07` | 移位不钳制到操作数位宽（去掉 `cmp/csel`） | `native_corpus_run`（`ken/shift.go`） | **红** 0 passed / 1 failed |
| `M-M7-08` | 帧图的 `off=` 减 8（表与代码错位） | `native_frame_map`（「负偏移不在帧图的槽偏移里」） | **红** 1 passed / 1 failed |

## 三条「变异没红」的教训（都是判据的问题，不是代码的问题）

1. **`copy()` → 单字搬** 不变红。原因不是判据弱，是那条分支**到不了**：
   `Instr::Copy` 在 46 个语料的 SSA 里出现 **0 次**（`R-13-7`）。
   把变异改到 `store` 的聚合分支上，`native_artifacts` 的两条值语义立刻红。
   → **变异要先确认目标代码真被执行**，否则「绿」与「红」都不说明问题。
2. **`place_args` 阈值 8→6** 在 `native_args` 上是**绿的**：调用方与被调方用同一张表，
   所以两边一起错、行为仍自洽。只有**直接断表**的用例才抓得住 ——
   这正是 `place_args_*` 那 5 条存在的理由。凡是「两方共用一张表」的设计，
   端到端测试都证明不了表的正确性（同 `R-13-3` 的教训）。
3. **`ten_args` 是软判据**：`max_outgoing` 恒 0 时它仍然绿（帧底恰好有 16 B 对齐余量）。
   硬判据是 `twelve_args`（4 个栈字 = 32 B）。测试里已写明这一点。

## 一条新踩的坑：恢复源码之后必须让产物失效

跑完 4 条探针（脚本已把源码 `mv` 回去、`git status` 干净）之后，全量测试红了 2 条 ——
`native_args::str_args` 与 `native_corpus_run` 里的 `decl` / `func8`。
红的样子正是「字符串丢 len」那个老症状。

原因不是源码没恢复，是 **`.rlib` 还是变异版**：绑定挂载上 cargo 的 mtime 判据不可靠
（`mutation-m3-notes.md` §0 的同一条），于是 `cargo test` 跑在「源码对、库错」的混合状态上。

→ 脚本末尾加了一句 `cargo clean -p rgoc-arm64`。**这条比任何一条探针都重要**：
   它解释了「为什么变异之后的全量回归不能直接信」。
