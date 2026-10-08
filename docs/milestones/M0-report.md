# M0 交付报告

> **读者**：下一个接手 rgoc 的人（无论是人还是 agent）。
> **本报告的目标**：让你在**不重跑任何东西**的前提下，判断「哪些结论可靠、哪些不可靠、下一步该做什么」。
>
> **一句话结论**：M0 的三个技术路线**都已证明可行**，M0 的**语言实现工作量为零**
> （三个 spike 跑的是同一个固定 fixture）。M0 真正的产出是
> **可信的 oracle 流水线** + **五份契约** + **一份被实测推翻过的错误记载**。

**生成日期**：2026-10-07　|　**对应计划**：`M0-plan.md` T53
**证据基准**：`M0-manifest.json`（八条门禁全过）+ `M0-benchmarks.md`（§1–§12）

---

## 1. 三个命题各由哪个测试 ID 证明

`M0-design.md` §1.1 定义了 M0 要证明的三条路线。逐条对照：

| 命题 | 内容 | **证明它的测试 ID** | 实测结果 |
|---|---|---|---|
| **P1** | 固定 HIR → 宿主求值（解释器路线）可行 | **`T-S1-01`**（主）、`T-S1-02`（可重复）、`T-S1-03`（值表示边界） | stderr 精确 `3\n`、stdout 精确为空、退出码 0 |
| **P2** | 固定 HIR → Block/Value SSA 可表示且能求值 | **`T-S2-01`**（交叉验证）、`T-S2-02`（需求清单）、`T-S2-03`（与 M6 差距） | 图含真 `bin` 指令（`v2 = v0 + v1`），求值结果与 P1 **逐字节相同** |
| **P3** | 自研 arm64 汇编 → `clang` → 可运行 ELF（native 路线）可行 | **`T-S3-01`**（产出 ELF）、`T-S3-02`（`file` 判定）、`T-S3-03`（运行）、`T-S3-04`（六项记录）、`T-S3-05`（可复现） | `ELF 64-bit LSB pie executable, ARM aarch64`；stdout 精确 `hello\n`；退出码 0 |

> **P3 是 M0 风险最高的一项**（`03` §7「native/object/ABI 范围爆炸」），
> 因此它在 M0 就有门禁（E6/E7），没有推迟到 M7。

### 1.1 ⚠️ P1 的判定方式被**实测推翻并订正**（必读）

`M0-design.md` §1.1 原文写的是：

> 「一个固定 HIR 程序求值产出 `3`，**stdout 精确等于 `3\n`**，退出码 `0`」

**这一条是错的。** 实测（容器内 go1.27.1，`od -c` 逐字节）：

```text
$ go run p.go >out.txt 2>err.txt   # main 里只有 println(1 + 2)
exit=0
--- stdout(od) --- 0000000            ← 长度 0，一个字节都没有
--- stderr(od) --- 0000000   3  \n    ← "3\n" 在这里
```

Go 的内建 `println` 按语言规范写**标准错误**。订正后的判定：
**stderr 精确 `3\n` 且 stdout 精确为空**（两条流都要断言 ——
只断言 stderr 会漏掉「实现顺手也往 stdout 写了一份」）。

订正存档在 `M0-tests.md` §5.1 的**修订 R1**；配套的设计改动见 §3.2。

---

## 2. 门禁证据出处

| 门禁 | 判定 | 证据在哪 |
|---|---|---|
| **E1** | `docker build` 从零重放；`image.lock` 无占位符 | `docker/image.lock`；四次 `--no-cache` 构建。**image id 不作为门禁**（实测不可复现，见 `M0-benchmarks.md` §5） |
| **E2** | 环境 manifest 无空缺字段 | `M0-manifest.json` 的 `environment` 节；T16 校验脚本输出 `none` |
| **E3** | harness 六类自测全绿 | `rgoc/crates/rgoc-harness/tests/harness_self_test.rs`（19 条，正反例齐备） |
| **E4** | 官方语料 20/100% 通过 | `rgoc/tests/corpus/T-C-report.{md,json}`；实测 5.8 s / 峰值 15 MiB |
| **E5** | VSCode **实测断点命中** | 用户在 dev container 按 F5 实测（登记在 `gate.E5`，含确认人与时间）+ 下层印证 `scripts/debug-smoke-test.sh`（A/B/C + 9 项断言） |
| **E6** | 三 spike 输入/结果/环境全部可复现 | **`M0-benchmarks.md` §12**（含完整实测数据与不可复现性的定位过程） |
| **E7** | native `hello` 可运行**且已登记** | 运行 + `file` 输出 + `03-roadmap.md` 的 M1 smoke 清单（`SM-M0-NATIVE-HELLO`，注明来源 `T-S3-03`） |
| **E8** | 五份契约初稿 | `docs/contracts/`（C1 留位 / C2 完整初稿 / C3–C5 spike 级） |
| **E9** | `03` §6.3 六条统一退出检查 | 见本报告 §6 |
| **E10** | 基准数据进 manifest | `M0-benchmarks.md` §1–§4（Phase 0）+ **§12（Phase 3 可复现性）** |

**workspace 当前状态**：**260 条测试全绿**，自检 **147 条断言全过**，
门禁四条（`fmt --check` / `check` / `clippy -D warnings` / `test`）实测通过。

---

## 3. spike 验证出的形状

### 3.1 值表示：常量必须以任意精度存在（`T-S1-03`）

机制来源是 Go 自己的一条报错：

```text
cannot use big (untyped int constant 1267650600228229401496703205376)
           as int value in argument to built-in println (overflows)
```

**Go 也是「先任意精度、后按类型收敛」**。所以 M0 的 HIR 里常量是 `BigInt`
（base 2^32 尾数），收敛**只**发生在 `to_int64()` 一处，失败返回 `None`。

> **若一开始就用 `i64`**：折叠那一步会**静默丢位**，而 `1 + 2` 仍正确 ⇒ **测试照样绿**。
> 这就是 T-S1-03 存在的理由。

### 3.2 输出流必须类型层面区分（`T-S1-01` vs `T-S3-03`）

S1 走 stderr（Go 内建 `println`），S3 走 stdout（真实程序输出 hello）。
做成 `Stream` 枚举而非注释 —— 否则将来有人「统一」两者，
会**同时打破** `T-S1-01` 与 `T-S3-03` 中的一个。有测试钉住「两个 fixture 的 stream 必须不同」。

### 3.3 SSA：唯一定义点 + 使用先于定义是结构性事实（`T-S2-01`）

`ValueId` 是不可变新类型；`Builder::expr` 用**后序遍历** ⇒
「引用的 ID 必然小于自己的 ID」成为结构性事实而非约定。
块表用 `BTreeMap`（顺序必须确定，否则 E6 会随机失败）。

### 3.4 codegen 是纯函数（`T-S3-05`）

三次生成同一份 HIR ⇒ 汇编**逐字节相同**（sha256 `098cac69…`）。

---

## 4. spike 留下的差距

### 4.1 `T-S2-02`：SSA 需求清单（memory / tuple / 调用边界 + phi + verifier）

**每条都带触发形状**（具体到能写成一个测试）—— 只写「需要 memory 模型」是空话。

| ID | 需求 | 触发形状 | 为什么图不够 | 归属 |
|---|---|---|---|---|
| `S2-NEED-MEMORY` | memory / 别名 | `a := 1; p := &a; *p = 2` | 图里值**不可变且唯一定义**；改写语义与 SSA 直接冲突 | M6 |
| `S2-NEED-TUPLE` | 多返回值 | `func two() (int, error)` | Go 的多返回值是**语言级 tuple**（可整体赋值/传参），不是结构体 | M5b |
| `S2-NEED-CALL` | 调用边界 | `println(1+2)` | M0 把实参存成已求值的 `Val` ⇒ **图里看不到调用**，控制流边无从画起 | M6 / M5 |
| `S2-NEED-PHI` | phi / 支配 | `if c { return 1 }; return 2` | 汇合处同一变量有两个定义点 | M6 |
| `S2-NEED-VERIFIER` | 图不变式检查 | 多块图（M0 **触发不了**） | SSA 正确性依赖一组不变式，必须**被检查**而非被信任 | M6 |

### 4.2 `T-S2-03`：与 M6 完整 verifier 的差距

| 不变式 | 状态 | 说明 |
|---|---|---|
| 唯一定义点 | ✅ 具备 | 类型系统保证 |
| 使用先于定义 | ✅ 具备（**直线内**） | ⚠️ 多块时**不再成立** |
| 控制流图 | ❌ | 单块，`succs` 恒空 ⇒ **没有「汇合」概念** |
| phi 正确性 | ❌ | 无 phi 指令，`Block::params` 只是**容身处** |
| **支配关系** | ❌ | **单块图里支配关系是平凡的（全可达）⇒ 从未被验证过** |
| 类型一致性 | ❌ | 图**无类型标注** ⇒「i32+i32」与「i32+string」**同形** |
| 副作用 / 内存 | ❌ | 指令全是纯函数，无 load/store |
| 调用指令 | ❌ | 图里**没有 call 指令** |

> **「不具备」与「从未被验证过」必须分开读。**
> 单块图里很多性质是**平凡成立**的，那不构成「已支持」的证据。

### 4.3 `T-S3-04`：native 六项记录

| 项 | 结论 | **没记录什么**（缺口） |
|---|---|---|
| 调用约定 | 裸 `_start`，不经 C 运行时；`write`=64 / `exit_group`=94，号在 `x8`，`svc #0` | 只验证了**无参无返回**的系统调用；用户函数的参数传递/栈帧/callee-saved 全未验证 |
| 栈对齐 | 本 fixture **完全不动栈**，`sp` 天然 16 字节对齐 | ⚠️ **「不需要动栈」≠「栈对齐已验证」**：`bl` 前对齐、leaf 不存 `fp`/`lr`、非 leaf 必须存 —— 全未验证 |
| 输出流 | `write(2)`，`x0`=fd（1=stdout / 2=stderr）；PIE 下用 `adrp`+`:lo12:` | **未处理短写（部分写）返回值** —— 代码里有 `TODO(S3-04)` 标记 |
| 退出码 | `exit_group(94)`，`x0`=状态；正常 0 | 未验证非零路径；**Go 的 panic 退出码是 2**，何时用哪个未定 |
| runtime 桥接 | **零 libc 符号**（`svc #0` 直达内核）；`readelf -d` 仍显示 `NEEDED libc.so.6` 但未引用 | ⚠️ **「不引用」≠「不需要」**；`-Wl,-s` 意味着**不可符号级调试** |
| unwind 边界 | **不可 unwind**：无 `.cfi_*`、无 `.eh_frame` | 这是**固有限制不是待办**；将来加 unwind 是**新增能力** |

### 4.4 两个必需链接开关（都不是可选项）

| 开关 | 不加会怎样（实测） |
|---|---|
| `-nostartfiles` | **链接失败**：`multiple definition of _start; Scrt1.o: first defined here` |
| `-Wl,-s` | 链接成功但**产物不可复现**：`.strtab` 残留 clang 随机中间名 `hello-d9450b.o`（差在第 66200 字节） |

**完整定位过程**（BuildID 三次相同 ⇒ 内容可复现 ⇒ `readelf -p .strtab` 打出根因）
见 `M0-benchmarks.md` §12.4。

---

## 5. 明确未验证 / 超出范围

### 5.1 unsupported 清单（U1–U14，冻结于 T29）

**M0 的分母口径**：顶层 `test/*.go` 中「action 属于 v0 支持集（`run`/`compile`/`errorcheck`）
且 action 不含排除参数（U7）」的文件集 = **279**（`run` 147 + `errorcheck` 120 + `compile` 12）。
其中平台过滤 5 个 → 实际执行 274。**后续不得因跑不动而缩小分母。**

| ID | 不支持项 | 实际样本数 | 归属 |
|---|---|---|---|
| **U1** | `rundir` / `runindir` | 5 | 后续 |
| **U2** | `runoutput` | 14 | 后续 |
| **U3** | `errorcheckdir` | 1 | 后续 |
| **U4** | `asmcheck` | — | **M7+** |
| **U5** | `build` / `builddir` / `buildrun` / `compiledir` 等 | 9 | 后续 |
| **U6** | `errorcheckoutput` / `errorcheckandrundir` / `errorcheckwithauto` | 11 | 后续 |
| **U7** | 带 `-gcflags` / `-d` / `-goexperiment` 的用例 | 31 | 不定 |
| **U8** | `test/fixedbugs/` 全目录 | — | 后续 |
| **U9** | `test/ken/`、`test/chan/`、`test/typeparam/` 等子目录 | — | 后续 |
| **U10** | 需要 `unsafe` / `reflect` / `cgo` / 汇编内联 | — | M9+/M12 |
| **U11** | `src/internal/types/testdata/` | — | M4 |
| **U12** | **真实 Go 源码的 lex/parse** | — | **M1/M2** |
| **U13** | `skip` 指令（**上游设计即跳过**，官方直接 `t.Skip`） | 5 | 后续 |
| **U14** | action 不是 16 指令之一但被平台过滤（`linkmain.go`） | 1 | 后续 |

> **U12 是最容易被误读的一条**：三个 spike 用的是**固定 HIR**，
> **M0 从未解析过一行 Go 源码**。M1（lexer）与 M2（parser）才是那条路。

### 5.2 已知未解决的实现缺口

> 全部条目在 `M0-manifest.json` 的 `open_gaps` 节有结构化登记（`GAP-01` … `GAP-04`）。

| 缺口 | 影响 | 归属 |
|---|---|---|
| **GAP-01 逐 ID 的适用维度清单未建** | `03` §0.3 要求「M0 建立逐 ID 的适用维度清单」，但 M0 只登记了 37 个**测试 ID**，**不是** 210 个**功能 ID** 的四维归属 ⇒ **四维完成度指标目前无法计算** | M1（LEX-* 13 个是第一批） |
| **GAP-02 正则子集覆盖不足** | 279 全量里多数 errorcheck 命中 `UNSUPPORTED-REGEX`（`{n,m}` `[...]` `+` `?` `^` `$`）⇒ 判 `harness-failure` | **M4/M5 之前要决定**：扩展子集还是引 regex crate |
| **GAP-03** `TestCase` **无位置字段** | 失败报告只能定位到**文件级**，不能到行 | M4（诊断）/ M2（行号） |
| **GAP-04 CI 未实施** | **按计划未做**（M0 砍项）；`scripts/in-container.sh` 即「同 digest 可接入」的入口 | 按需 |

> ⚠️ **不得为了让分母变绿而放宽判定**（`M0-plan.md` T38 判定纪律第 3 条）。
> GAP-02 这个缺口是**如实登记**的，不是遗漏。

### 5.3 关于 GAP-01 的说明（为什么不硬凑一份清单）

`01-feature-set.md` 共有 **210 个功能 ID**（AST 49 / TYP 33 / EXP 31 / STM 31 /
PIPE 28 / BIF 15 / LEX 13 / SCP 11 / PKG 10 / CON 8 / SYS 5）。
判定「每个 ID 适用哪些维度」（一个 ID 可适用多维，如 `CON-08` 同时属 Frontend 与 Interpreter）
是**实质工作量**，而且：

- M0 的三个 spike **都不依赖它**（它们跑的是固定 HIR，不解析任何 Go 源码）；
- **硬凑一份「看起来完整」的清单比不做更坏** —— 未判定的格子会被后来人当成「已判定」。

所以 M0 选择**如实登记缺口**，而不是交一份filled-in-的表。
**M1 是它的自然起点**：M1 要实现 `LEX-*` 那 13 个，届时它们的维度归属是确定的。

---

## 6. E9：`03` §6.3 的六条统一退出检查

| # | 检查项 | 判定 | 证据 |
|---|---|---|---|
| 1 | 当前阶段必需测试全绿，**无未分类 / 基建 / 不稳定失败** | ✅ | 260 条测试 0 失败；八类 `Verdict` **不合并**，`HarnessFailure` 与 `CompilerFailure` 严格分开 |
| 2 | 四条 cargo 命令全过 | ✅ | `fmt --check` / `check --workspace --all-targets` / `clippy --workspace --all-targets -- -D warnings` / `test --workspace` 实测通过 |
| 3 | 环境、报告、复现材料与契约版本齐全，保留 unsupported 清单 | ✅ | `environment` 无空缺；五份契约；`M0-benchmarks.md` §1–§12；U1–U14 全在案 |
| 4 | 新功能都归属当前冻结测试集或已登记风险 | ✅ | 每条测试有 `T-H`/`T-C`/`T-S*` ID；两个 SPIKE-ONLY crate 有 `milestone` 归属与 M5 替换条件 |
| 5 | ABI / 布局 / 基线 / 必需用例的决策、迁移与回归影响有记录 | ✅ | 契约修订记录表（C1–C5 各有 §变更记录）；`M0-plan.md` 门禁汇总表；T-S1-01 订正存档 |
| 6 | **不可行时先缩减范围，不把 recoverable failure 当完成** | ✅ | `UNSUPPORTED-REGEX` 如实登记为 `harness-failure`（**未**放宽判定）；T-S3-04 六项各带「没记录什么」栏 |

---

## 7. 接手指南：按这个顺序读

| 顺序 | 文档 | 为什么先读它 |
|---|---|---|
| 1 | **`AGENTS.md`** | 工程入口：当前阶段、目录地图、纪律、下一步 |
| 2 | 本报告 §3–§5 | **先知道哪些结论可靠、哪些是缺口**，再读细节 |
| 3 | `docs/03-roadmap.md` §4 | M0 三个命题的原文与 M1–M12 的位置 |
| 4 | `docs/contracts/C2-test-ir.md` | **唯一「完整」级契约** —— 所有阶段都要按它登记用例 |
| 5 | `docs/milestones/M0-tests.md` | 测试冻结口径（分母 279 / unsupported U1–U14 / 命门规则 §1.3） |
| 6 | `docs/contracts/C1` / `C3` / `C4` / `C5` | 按你负责的阶段挑：M1/M2 看 C1，M5 看 C3，M6 看 C4，M7 看 C5 |

### 7.1 开始 M1 之前必须知道的三件事

1. **M1 的 native smoke 必须包含 `SM-M0-NATIVE-HELLO`**（`T-S3-03` 的延续）。
   它是**固定 HIR/SSA** 验证，**输入不经过 lexer** ——
   别把它误读成「M1 已能从 Go 源码生成机器码」。
2. **`M0-tests.md` §5.1 有修订 R1**：`T-S1-01` 的流是 **stderr** 不是 stdout。
   若你照原文写测试，会得到一个「永远红」的断言。
3. **正则子集的缺口会在 M1 之后暴露**：一旦开始跑全量 errorcheck，
   `UNSUPPORTED-REGEX` 会大量出现。**先决定扩展还是引 crate**，否则会被迫放宽判定。

### 7.2 M1 会遇到的一个「已知的没做」

**GAP-01：`03` §0.3 要求的「逐 ID 适用维度清单」M0 没有建**（`01` 有 210 个功能 ID）。
后果是**四维完成度指标目前算不出来**。M1 实现 `LEX-*` 那 13 个时，
可以顺带把它们的维度归属定下来 —— 但**不要试图一次填完 210 个**，
未判定的格子被当成「已判定」比空着更危险。

另三条缺口（GAP-02 正则子集 / GAP-03 无位置字段 / GAP-04 CI 未实施）
见 §5.2 与 manifest 的 `open_gaps` 节。

### 7.3 环境复验（换机器时）

**E5 是人工门禁**（断点命中），换机器/重装/升级 VSCode 后**无法自动复验**。
需要复验时按 `M0-plan.md` T28 的操作与排障路径走；
下层印证用 `scripts/debug-smoke-test.sh`。
**三个已知的复发点**（代理下推进容器、VS Code Server 升级、CodeLLDB 平台包丢失）
记在 `AGENTS.md` §「VSCode / 调试链路」。

---

## 8. 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-07 | **初稿创建**（T53）。含 §1.1「P1 判定被实测推翻」的如实记录 | Phase 0–3 全部完成 + 门禁 E1–E7 / E10 |
