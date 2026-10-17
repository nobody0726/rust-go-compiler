# M3 反向验证记录（变异测试）

> **这一份是什么**：M3（符号与类型检查入口）测试的**反向验证** —— 把实现里的一处
> 改坏，看对应的那条测试**是否真的会红**。测了不红 = 那条断言是**恒真**的
> （`AGENTS.md` §6.1 的老坑：一条恒真的断言比没有断言更糟，它给出「已检查」的错觉）。
>
> **执行方式**：人工挑变异点、**容器内**逐条跑（`scripts/in-container.sh`），
> 脚本不留库（`M0` 的先例：变异脚本已移除，快照在 `.workbuddy/backup/`）。
> 本文是**人读的产物**：每条写清「变异 → 期望 → 实测 → 复原证据」。
>
> **日期**：2026-10-17（M3/Task 37）。
>
> **前置基线**：变异开始前，`M3-tests.md` §7 的四条门禁 + 腿乙三条**全绿**
> （`workspace 1280 passed / 0 failed`、`gate_fail=0`；腿乙 dump 8 / cmp 2 / 语料 31）。

## 0. 一条**必须先说**的执行陷阱（本次真的踩到了）

**变异前必须 `cargo clean -p <被变异的 crate>`。**

第一次跑（7 条探针，没有 `cargo clean`）里 **`T-TY-19` 与 `T-TY-65` 两条「照不出来」**
—— 变异明明落地了（文件 md5 变了），测试却绿。根因不是断言没牙，而是 **cargo 用了
上一次编出来的 rlib**：这个工作区在**绑定挂载**上，cargo 的 mtime 判据不可靠。

脚上加了 `cargo clean -p <crate>` 之后，同样 7 条**全部变红**（见下表）。
⇒ 结论照实登记：**那两条「照不出」是执行方式的问题，不是覆盖缺口**；而这条陷阱本身
值得写进纪律 —— 它会让**任何**变异测试（包括 `M0` 那批）静默失效。

### 0.1 反过来也踩了一次（Task 38 收口时实测）

同一个根因的**另一面**：7 条探针**全部复原**（工作区 `git diff` 干净、md5 逐字节相同）之后，
Task 38 第一次跑全量门禁是 **`passed=1274 failed=8`**，红的 8 条全在
`rgoc-parse/tests/test_directive.rs`，报的还是 `M84` 那句
`"syntax error: unknown //go: directive"` —— 也就是说 **`rgoc-parse` 的 rlib 里还编着变异版**，
cargo 认为它新鲜，没重编。

修法只有一条：**全量重建**

```sh
find /work/rgoc/target -mindepth 1 -delete   # cargo clean 会 EBUSY（target 在命名卷上）
```

重建后同一条命令 **`passed=1282 failed=0`、`gate_fail=0`**（1280 + Task 38 新加的 2 条对账测试）。

**纪律**：变异收尾之后，**别相信紧接着的那次门禁结果** —— 要么 `cargo clean -p` 每个被变异的 crate，
要么直接 `find rgoc/target -mindepth 1 -delete` 全量重建（≈4 min）。
假绿与假红**都是**这个陷阱的表现，而**假红**比假绿更危险：它会让人去「修」一个根本没坏的东西。

## 1. 探针总表

| # | 覆盖 ID | 变异（改哪儿 → 改成什么） | 期望 | 实测 |
|---|---|---|---|---|
| `M11a` | `T-TY-11` 穷尽性 | `rgoc-types/src/typ.rs`：**删掉** `Type::Union(UnionId)` 变体 | 红 | ✅ **编译期**红：`error[E0599]: no variant ... named 'Union'`，`could not compile rgoc-types (lib) due to 6 previous errors` |
| `M11b` | `T-TY-11` 计数断言 | 同文件：测试里 `labels` 数组**删一项** `"union"` | 红 | ✅ `变体数与标签数都是_15` FAILED：`left: 14 / right: 15` |
| `M19` | `T-TY-19` dump 幂等 | `rgoc-types/src/dump.rs`：`try_dump_types` 的输出**掺进「本进程第几次 dump」** | 红 | ✅ `dump_根序按声明序` FAILED：两次 dump 差 `0\tinvalid` / `1\tinvalid` |
| `M47` | `T-TY-47` 溢出 | `rgoc-const/src/representable.rs`：整数目标的**范围检查失效**（`&& false`） | 红 | ✅ `t_ty_47_溢出不截断` FAILED：少了一条 `TypNumericOverflow`（`left` 2 条 vs `right` 3 条） |
| `M65` | `T-TY-65` 不筛文件 | `rgoc-load/src/package.rs`：`read_sources` 里**错误地评估**构建约束（含 `//go:build ignore` 就丢文件） | 红 | ✅ `t_ty_65_显式列表里的文件一个都不筛` FAILED：`file_count` `left: 3 / right: 4` |
| `M81` | `T-TY-81` 拒绝改错答 | `rgoc-driver/src/cmp_cmd.rs`：`identical` 的 `Err(code)` 分支**不再写 `reject`**，改写 `false` | 红 | ✅ `ty81_约束接口同一性_正向reject_反向不拒` FAILED：输出成了 `3 identical false` |
| `M84` | `T-TY-84` 未知指令 | `rgoc-parse/src/directive.rs`：未知 `//go:` 指令**改成报错** | 红 | ✅ `ty84_未知go指令不报错_而版本门控确实读build行` FAILED：多了 `[SYN-UNEXPECTED-TOKEN]`（2 条 vs 0 条） |

## 2. 每条的关键现场（从原始日志里摘）

`M11a`（穷尽性 match 的价值：**删变体连编译都过不去**，比测试红更早一步）：

```
error[E0599]: no variant, associated function, or constant named `Union` found for enum `typ::Type`
  --> crates/rgoc-types/src/typ.rs:272:19
error: could not compile `rgoc-types` (lib) due to 6 previous errors
error: could not compile `rgoc-types` (lib test) due to 7 previous errors
```

`M11b`（**计数断言**那一半的价值：编译期抓不到「标签漏了」，这条断言才抓得到）：

```
thread 'typ::tests::变体数与标签数都是_15' panicked at crates/rgoc-types/src/typ.rs:235:9:
assertion `left == right` failed
  left: 14
 right: 15
```

`M19`（幂等断言的现场 —— 两次调用的输出只差那一行）：

```
thread 'dump_根序按声明序' panicked at crates/rgoc-load/tests/decl_collect.rs:412:5:
  left:  "# source: rgoc\n0\tnamed 1\n…\n4\tsignature - 2 3 false\n0\tinvalid\n"
 right: "# source: rgoc\n0\tnamed 1\n…\n4\tsignature - 2 3 false\n1\tinvalid\n"
```

`M47`（「不截断」这条断言抓的是**诊断条数与类别**，不是文本）：

```
thread 't_ty_47_溢出不截断' panicked at crates/rgoc-load/tests/decl_collect.rs:262:5:
  left: [TypNumericOverflow, TypTruncatedFloat]
 right: [TypNumericOverflow, TypNumericOverflow, TypTruncatedFloat]
```

`M65`：

```
thread 't_ty_65_显式列表里的文件一个都不筛' panicked at crates/rgoc-load/tests/file_selection.rs:50:5:
assertion `left == right` failed: 四个文件都进包（构建约束不筛、`_test.go` 也不筛）
  left: 3
 right: 4
```

`M81`（`reject` 与 `false` 的差别就是「不判」与「不等」的差别）：

```
3	identical	false          ← 变异后（本该是 reject + `# reject 3 TYP-UNSUPPORTED-CONSTRAINT-IDENTITY`）
```

`M84`：

```
stderr=…/fwd.go:1:1: syntax error: unknown //go: directive [SYN-UNEXPECTED-TOKEN]
       …/fwd.go:2:1: syntax error: unknown //go: directive [SYN-UNEXPECTED-TOKEN]
  left: 1
 right: 0
```

## 3. 复原证据

每条都是「`cp` 备份 → md5 逐字节比对」；复原先后的 md5 **相同**才继续下一条
（脚本里带断言，任何一条复不了就停在原地，绝不带着脏现场往下跑）：

| 文件 | 复原后 md5 |
|---|---|
| `rgoc/crates/rgoc-types/src/typ.rs` | `703c8920dfeaeb250ef86b5d947801ba` |
| `rgoc/crates/rgoc-types/src/dump.rs` | `26320acfd8800e789a4358059e62e410` |
| `rgoc/crates/rgoc-const/src/representable.rs` | `16b902764c1bf099dbf9977c12c957bc` |
| `rgoc/crates/rgoc-load/src/package.rs` | `6760dc9e4a6109de48ce95b87a7890d6` |
| `rgoc/crates/rgoc-driver/src/cmp_cmd.rs` | `f00fc22da7ec7ff8eecc39bc0059b7dd` |
| `rgoc/crates/rgoc-parse/src/directive.rs` | `1d0a9463e35e3d82d812b530cae0fab7` |

收尾守卫：`git diff --stat` **空**（变异不留痕）；随后 Task 38 再跑一次全门禁收口。

## 4. 覆盖面的诚实说明（**没**测的）

* 这里只钉了 **6 个 ID + 1 个额外半边（`T-TY-11` 的两条断言各测一次）**，
  不是「M3 的每一条断言都被反向验证过」。挑的是**最容易被写松**的那几类：
  穷尽性、幂等、边界（溢出）、「不做事」的反向断言（不筛文件 / 不报错）、
  边界码（拒绝 vs 错答）。
* **没**覆盖：对撞类（`T-TY-91..95`，它们本身就是「两侧现跑」⇒ 反向验证是
  「改一侧」——那由 `run-collide.sh` 的逐字节判据承担）、语料精选纪律
  （`T-TY-94`，已在 Task 35 用合成反例逐条踩过）、`T-TY-81..85` 里其余三条
  （`82/83/85` 在 `types_e2e.rs` 里已经成对写过正/反例）。
* 变异是**人工**挑的（`scripts/` 里没有自动变异器 —— `M0` 的两个变异脚本已按用户
  要求移除，快照在 `.workbuddy/backup/scripts-removed-20261002-1504/`）。
  **代价要记着**：现在没有任何自动手段能证明断言会失败，这份记录就是那件事的替补。
