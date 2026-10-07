# C2 契约（初稿）：Test IR / 构建条件 / 比较器 / 结果分类

> **契约级别：完整初稿** —— M0 五份契约里**唯一**「完整」级的一份。
> **消费者：所有阶段**。M1 起的每一层测试都要按本契约登记用例。
>
> **反推来源**：`rgoc/crates/rgoc-harness/src/ir.rs`、`instruction.rs`、`compare.rs`、
> `runner.rs`（Phase 2，T31–T36，均已过门禁 E3）。
> 本文件是**从已通过门禁的代码反推**的，不是设计意图的复述 —— 凡代码与文档冲突，**以代码为准**。

**状态**：初稿（Phase 4 / T49）　|　**门禁证据**：E3（T36，19 条自测）、E4（T38，20/20）

---

## 1. Test IR：`TestCase` 的 15 个必录字段

**字段是「必录」不是「建议填」**：缺字段必须让 `TestCase::validate` 报错，
**不得静默填默认值** —— 静默默认值会让「漏填」伪装成「通过」。

| # | 字段 | 类型 | 作用 / 约束 |
|---|---|---|---|
| 1 | `id` | `CaseId` | 用例 ID（`T-C-01` / `T-H-01` / `T-S1-01`）。包一层新类型，避免被当自由字符串传 |
| 2 | `rel_path` | `PathBuf` | **相对 `GOROOT/test` 的路径** |
| 3 | `input_files` | `Vec<PathBuf>` | 输入文件集合（单文件用例就是它自己） |
| 4 | `mode` | `Mode` | 模式：v0 支持集 3 种 + **已冻结的不支持模式**（见 §2.2） |
| 5 | `instruction_args` | `Vec<String>` | 指令参数（`// run` 之后的部分；无参为空） |
| 6 | `build_tags` | `Vec<String>` | build tags，**平台过滤用**（规则 R1b） |
| 7 | `target` | `Target` | 目标平台 + oracle 版本（`goos` / `goarch` / `go_version`） |
| 8 | `imports` | `Vec<String>` | import 需求（`errorcheck` 层用它算 `-importcfg`） |
| 9 | `feature_deps` | `Vec<String>` | 功能依赖，挂到 `01-feature-set.md` 的**功能 ID** 上 |
| 10 | `comparator` | `Comparator` | 比较器（见 §4） |
| 11 | `expected` | `Expected` | 期望结果（见 §4.2） |
| 12 | `limits` | `Limits` | 超时与资源上限（见 §5） |
| 13 | `seed` | `u64` | 随机种子。**固定语料下恒为 0**；字段保留是为了将来引入随机化用例 |
| 14 | `milestone` | `String` | 归属阶段（`M0` / `M1` / …） |
| 15 | `unsupported` | `Option<Unsupported>` | 排除原因。**不在 v0 支持集时必填** |

> ⚠️ **15，不是 16**。曾有文档写「16 个必录字段」—— 那是**错的**，
> 代码里就是 15 个（`ir.rs` 的 `pub struct TestCase`）。已订正 `AGENTS.md`。

### 1.1 `CaseSpec` 必须带**磁盘路径**（不只是源码）

真实语料在**只读**的 `GOROOT/test`，执行器**只读不写工作目录**。
所以 `input_files` 记的是**磁盘路径**；只记源码内容会让用例无法真正执行。

### 1.2 `Target.go_version` 是**硬约束**

必须精确 `go1.27.1`（决策 D-M0-1：Go oracle 是硬约束，Rust 是软约束）。
版本不符时 oracle **必须被拒绝**，判 `reference-toolchain-failure`
（`M0-tests.md` T-H-06）。

---

## 2. 构建条件判定

### 2.1 两条规则，顺序不可交换

| 规则 | 内容 | 依据 |
|---|---|---|
| **R1** | 指令行**不是**文件第 1 行 —— 第 1 行是 `//` 指令**注释** | `testdir_test.go:522` 附近 |
| **R1b** | **平台过滤先于** `switch` 分派 | `testdir_test.go:522` → `:541` |

**R1b 的顺序为什么重要**：官方在 `switch` **之前**先做平台过滤。
`linkmain.go` 首行是 `//go:build ignore`，其 action 解析结果**不是** 16 个指令之一 ——
但它**先被平台过滤掉并 `t.Skip`**，所以**不会**触发 `unknown pattern` 硬失败。
这对应 unsupported 清单的 **U14（`target-filtered`）**。

> ⚠️ 反过来若把 `switch` 提到过滤之前，`linkmain.go` 会让整层直接失败。
> **`dispatch(ins, platform_ok)` 把这个顺序写进了函数签名** ——
> 让「顺序写错」变成编译期可见，而不是靠注释提醒。

### 2.2 指令集共 **16 个**

未知指令**直接 `t.Fatalf`**（官方行为），M0 不做「宽容跳过」。

**v0 支持集只有 3 种**：`run` / `compile` / `errorcheck`。
其余 13 种**建模进 `Mode` 枚举**而不是忽略 —— 因为 harness 遇到它们必须能
**显式分类**为 `expected-unsupported`（`03` §3.3）。

⚠️ **`errorcheck` 的一个陷阱**：裸写 `// errorcheck` 也会被自动加上
`-d=ssa/check/on`（官方 `testdir_test.go:797-811`），因此它的期望
**不能当纯语言语义验收**。

### 2.3 分母口径（`03` §3.3）

- **平台过滤项留在分母内**，不计入分子。
- 分母 = 顶层 `test/*.go` 中「action 属于 v0 支持集且 action 不含排除参数（U7）」的文件集
- **冻结于 T29（2026-10-02）= 279**（`run` 147 + `errorcheck` 120 + `compile` 12），
  其中平台过滤 5 个 → 实际执行 274
- **后续不得因跑不动而缩小分母**（`M0-plan.md` T38 实测登记：全量 356 − 排除 77 = 279）

---

## 3. 判定分类：八种 `Verdict`，**不得合并**

```text
Pass                     唯一计入分子
CompilerFailure          被测件【编译】失败
RuntimeFailure           编译过了但【运行】出错
HarnessFailure           harness 自己坏了（指令解析 / oracle 调用 / 比较器崩）
TargetFiltered           被平台过滤（不计入分子、仍计入分母）
Timeout                  超时（子进程必须已被【真正回收】）
ResourceFailure          超出资源上限（峰值 RSS 等）
ReferenceToolchainFailure oracle 工具链版本不符
```

### 3.1 为什么八种不得合并（这是本契约最重要的一条）

**`HarnessFailure` 与 `CompilerFailure` 合并，就等于把基建失败算成被测件的语义失败。**
那样 E3/E4 的分母分子全部失去意义 —— 一个坏掉的 harness 会显示成「rgoc 编译不过」。

### 3.2 另一条不得混淆的分类：能力不足 ≠ 不匹配

比较器的正则子集**不认识**某个模式时，产出的不是「不匹配」，而是
`UNSUPPORTED-REGEX:` 前缀的 `HarnessFailure`：

| 情况 | 判定 |
|---|---|
| 模式用了子集外的语法（`{n,m}` `[...]` `+` `?` `^` `$`） | `HarnessFailure`（**能力不足**） |
| 模式在子集内，但**真的没匹配上** | `CompilerFailure`（**语义不符**） |

**这条分离是 T36 建立的**，它让「全量 279 跑一遍」这个动作有意义：
未覆盖的模式会**如实显示为能力不足**，而不是伪装成「rgoc 不合格」。

---

## 4. 比较器规格

### 4.1 三种比较器

| 变体 | 用于 | 规格 |
|---|---|---|
| `MergedStreamStrictEq` | `run` | 子进程 **stdout 与 stderr 合并流**与 `.out` **严格相等** |
| `ExitCodeOnly` | `compile` | 编译成功即通过（退出码 0 且无诊断），**不比对任何输出** |
| `ErrorRegexPerDiag` | `errorcheck` | 逐条诊断比对（切分 R3 / 期望 R4） |

### 4.2 两条容易漏的规则

**R2b（合并流）**：官方 `runcmd` 把 stdout 与 stderr 写进**同一个 buffer**
再交给 `checkExpectedOutput`（`testdir_test.go:642-647`）。
`helloworld.go` / `printbig.go` 用内建 `print`（写 **stderr**）——
**只捕 stdout 会让它们「输出为空」而误判失败**。

**R2（缺 `.out` 即期望为空）**：不是「任意输出都通过」。
`Expected::has_out_file` 只决定**报错措辞**（有 `.out` = 内容不符 / 无 `.out` = 本应为空却非空），
**不改变比较逻辑**。

> ⚠️ `03` §3.4 的纪律：**`compile` 模式不得默认把输出当 stdout**。
> 所以 `ExitCodeOnly` 完全不碰输出。

### 4.3 `errorcheck` 的两条规则

| 规则 | 内容 |
|---|---|
| **R3** | 诊断切分：一条编译输出切成多条诊断 |
| **R4** | ERROR 期望匹配：先对齐**拒绝/接受**，再对齐**位置**（`文件:行号` 前缀） |

`ExpectedDiag.pattern` 是**未锚定**的模式（官方用 `regexp.MatchString`，
**不加** `^` / `$`）。期望的位置是**绝对行号**（`LINE±n` 已在解析阶段折算）。

### 4.4 正则子集：认识的就匹配，不认识的**明确报错**

`compare.rs` 只实现了一个正则**子集**。遇到子集外的语法 → 报 `UnsupportedRegex`，
**不猜、不放宽**。

**⚠️ 已知缺口（T38 遗留）**：`initloop.go`（T-C-13）的跨行期望
`a refers to b\n.*b refers to c\n.*c refers to a` 曾因此判 `compiler-failure`。

**已修的部分**：`\` 后的字符**不能一律当字面量**。
Go 的 `regexp/syntax` 把 `\n \t \r` 当 **Perl 类转义 ⇒ 真控制符**。
现已**分两类**：控制字符（`\n` → 真换行）vs 元字符（`\.` → 仍字面）。
**若把 `\.` 也当「任意字符」，就是放宽判定** —— 那会让假通过出现。

**仍未解决的**（M0 明确登记为缺口，见 `M0-report.md` §未验证）：
279 全量里多数 errorcheck 仍会命中 `UNSUPPORTED-REGEX`。
**M4/M5 之前要决定**：扩展子集，还是引入 regex crate。
**不得为了让分母变绿而放宽判定**（`M0-plan.md` T38 的判定纪律第 3 条）。

---

## 5. 超时与资源上限：**整个 M0 只有这一个定义处**

`Limits::for_layer(layer)` 是**冻结预算的唯一入口**（`ir.rs`）。
散落在各处就会出现两套预算，而门禁只看一套。

| 层 | 单用例 wall | 整层 wall | 备注 |
|---|---|---|---|
| `HarnessSelfTest`（T-H-*） | 30 s | 180 s | 含一个人为的超时用例 |
| `Corpus`（T-C-*） | 60 s | 300 s | 对齐 `03` §6.2 smoke ≤ 5 min |
| `SpikeFast`（T-S1/S2） | 30 s | 60 s | 极小程序 |
| `SpikeNative`（T-S3） | 120 s | 300 s | 含 clang 编译与链接 |
| `Overall`（M0 全门禁） | 900 s | 900 s | 远低于 milestone ≤ 30 min |

**单用例峰值 RSS ≤ 512 MiB**（五层同值）。
单用例可放宽（如 `T-C-03` 的 `gc1.go`），但**必须显式调用
`Limits::with_rss_override`**，不能就地改全局。

**T38 实测基线**（E4，20/20）：整层 **5.8 s**（预算 300 s）、
最慢单项 **0.031 s**（预算 60 s）、峰值 **15 MiB**（预算 512 MiB）。

---

## 6. 判定逻辑只有一处

**`rgoc_harness::runner::run_layer` 是唯一的端到端判定入口。**

`rgoc-driver` 与 `xtask` 都**只调它**，不自己判语义。
理由：两个工具各写一份判定，E4 就变成「两份报告说过了」而不是「harness 判过了」。

同理，**报告渲染也只有一处**：`rgoc_driver::report::render_text`。
`xtask` 初稿里曾写了个从 JSON 反推文本的 `human_text`，**已删**
（两份渲染器迟早漂）。

---

## 7. 超时回收：父子关系直，不需要进程组

`oracle.rs` 的超时实现用 `child.kill()` 即可，**不需要** `kill(-pgid)`。
理由：Rust 的 `std::process::Command` 在 Unix 上**默认不建进程组**，
子进程与父进程在同一组，`kill(-pgid)` 会连自己一起杀。
加上 workspace 的 `unsafe_code = "forbid"」，也用不了 `libc` 的裸调用。

---

## 8. 本契约的验证方式

| 断言 | 载体 |
|---|---|
| 缺必录字段必须报错 | `test_ir.rs`（C2 契约的可执行副本，T31） |
| `Verdict` 八种不得合并 | `test_ir.rs` + `harness_self_test.rs` 六类自测（T36 / E3） |
| 平台过滤先于 switch（R1b） | `test_instruction.rs` 顺序契约 + `linkmain.go` fixture |
| 合并流（R2b） | `test_compare.rs` |
| 正则子集边界 | `test_compare.rs`（含跨行守卫 + 变异测试） |
| 分母纪律 | `test_driver.rs`（`denominator` 不因过滤变小） |
| 20/20 基线 | `rgoc/tests/corpus/T-C-report.{md,json}`（E4） |

---

## 9. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-07 | **初稿创建**（T49）。从 `ir.rs` / `instruction.rs` / `compare.rs` / `runner.rs` 反推。**订正「16 个必录字段」为 15** | Phase 2（T31–T36）+ 门禁 E3 / E4 |
