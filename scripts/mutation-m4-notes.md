# M4 变异反向验证记录（`T-CK-B1..B8`，Task 41）

> 人读记录，纪律同 `mutation-m3-notes.md`：每条 = **施加点 + 命令 + 变红摘要 + 复原确认**。
> 全部在容器内跑（`scripts/in-container.sh`）。施加方式：临时改一处 → `cargo clean -p <crate>`（或 `go build`）→
> 跑**目标门禁** → `git checkout -- <file>` 复原 → 再跑一次确认回到绿。

## 汇总

| 变异 | 施加点 | 目标 | 结果 |
|---|---|---|---|
| `B1` 判定做反 | `rgoc-types/src/implements.rs`：函数首行 `return Ok(false)` | `cargo test -p rgoc-types --test implements` | **红 9/9**（`0 passed; 9 failed`） |
| `B2` 类别投影 | `rgoc-check/src/diag.rs`：`ChkInvalidAssign => Category::Decl` | `cargo test -p rgoc-check --test category` | **红**（`5 passed; 1 failed`，`每类计数快照` + 抽样向量） |
| `B3` 适配表信条 | `tools/types-oracle/check.go`：删 `{"op-invalid", ^invalid operation: }` | `types-oracle --check` + `go test ./...` | **红**：4 行输出变 `other`（正是 `T-CK-94` 的守卫条件）；`TestClassifyCheckMessage` FAIL |
| `B4` 多余诊断 | `rgoc-check/src/check.rs`：`check_package` 末尾 `report_hard` 一条 | `cargo test -p rgoc-check --test gaps_fixtures` | **红 3/6**（零诊断断言三条全炸） |
| `B5` 比对器 | `tools/types-oracle/run-collide.sh`：`strip()` 改 `cat` | `run-collide.sh tools/types-oracle/testdata` | **红**（14 fixture，diff 命中 `# source:` 行） |
| `B6` 版本门控反转 | `rgoc-types/src/version.rs`：`Versions::allow` 恒 `true` | `cargo test -p rgoc-check --test version_table` | **红**（`3 passed; 4 failed`） |
| `B7` 清单守卫 | `check-selected.txt`：删 `check/blank.go` 一行 | `cargo test -p rgoc-driver --test check_corpus` | **红**（`check_清单_两向集合相等且理由具体`） |
| `B8` init 顺序 | `rgoc-check/src/initorder.rs`：`compute` 交换前两条 | `cargo test -p rgoc-check --test initorder` | **红**（`1 passed; 5 failed`，含 `t_ck_71_顺序与_oracle_逐行一致`） |

**8/8 全部变红**（`M4-tests.md` §10 要求 ≥8 条）。

## 逐条现场

### B1 判定做反

```
$ cargo test -p rgoc-types --test implements --no-fail-fast
test result: FAILED. 0 passed; 9 failed; 0 ignored; 0 measured; 0 filtered out
```

判据来源：`implements` 是 `U-M3-01` 的落地 API；恒 `false` 让 9 条向量（含 `T-CK-12/13`
的正反两侧）全炸 ⇒ 单测**确实**钉住了判定结果，不是「跑了但没断言」。

### B2 类别投影

```
$ cargo test -p rgoc-check --test category --no-fail-fast
test result: FAILED. 5 passed; 1 failed
```

`assign` 与 `decl` 的每类计数快照差 1 ⇒ 投影表与快照互锁。**注意**：这条变异**不会**
让 `--check` 语料变红（语料本身仍红），所以 `T-CK-16` 的单测是唯一能把「投影被改」
与「语料差异」区分开的地方 —— 这正是它在 §10 里的位置。

### B3 适配表信条（`other` 守卫）

```
$ /tmp/b3o --check --test-hooks probe/t36/exp22.go | tail -4
warning: uncategorized check message -> other: invalid operation: division by zero
14:13	other
...
$ go test ./...   # tools/types-oracle
--- FAIL: TestClassifyCheckMessage
    check_test.go:213: classifyCheckMessage("invalid operation: division by zero") = "other", want "op-invalid"
```

`other` 是 `T-CK-94` 的**唯一**红条件（`run-collide.sh` 逐行判）。适配表少一条 ⇒
该族消息全部逃逸 ⇒ 语料对撞会拿「两边都是 other」当一致 —— 这就是必须留守卫的原因。

### B4 多余诊断

```
$ cargo test -p rgoc-check --test gaps_fixtures --no-fail-fast
test result: FAILED. 3 passed; 3 failed
    t_ck_76_exp22_逻辑运算符静态面 / t_ck_76_pkg04_零值矩阵 / t_ck_76_pkg06_初始化依赖循环
```

多一条诊断 ⇒ 三条「零诊断」断言炸；说明接受向量不是摆设。

### B5 比对器

```
$ run-collide.sh tools/types-oracle/testdata
     +# source: rgoc
run-collide: FAIL（14 个 fixture，模式 dump）
```

`strip()` 失效 ⇒ `# source: oracle` / `# source: rgoc` 进入比对面 ⇒ 全红。
这条同时证明「两侧都空 ⇒ 一致」的假绿路径被 `#` 过滤规则挡住。

### B6 版本门控反转

```
$ cargo test -p rgoc-check --test version_table --no-fail-fast
test result: FAILED. 3 passed; 4 failed
```

`allow` 恒真 ⇒ 低档特性全放行 ⇒ 全表 fixture 的 `version` 行全丢。

### B7 清单守卫

```
$ cargo test -p rgoc-driver --test check_corpus --no-fail-fast
test result: FAILED. 3 passed; 1 failed
    check_清单_两向集合相等且理由具体
```

删一行入选 ⇒ 「候选 == 入选 ∪ 排除」不再成立（该行既不在入选也不在排除）。

### B8 init 顺序

```
$ cargo test -p rgoc-check --test initorder --no-fail-fast
test result: FAILED. 1 passed; 5 failed
```

交换前两条 ⇒ `t_ck_71_顺序与_oracle_逐行一致` 与跨文件/函数字面量向量全炸。

## 复原确认

每次 `git checkout -- <file>` 后 `git status --porcelain` 只剩本轮**文档**改动；
复原后重跑 `run-collide.sh tools/types-oracle/testdata` ⇒
`OK（14 个 fixture，模式 dump；逐字节一致 + stderr 空 + 重复运行确定）`。

> **不承诺**：本记录只证明「8 个关键判据各自可被单点破坏」；它**不**证明检查器的
> 全部语义正确 —— 后者由 `M4-report.md` 的差距清单与 `CH-2` 的实测结论承担。
