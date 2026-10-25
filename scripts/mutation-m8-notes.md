# M8 变异记录（`M8-plan.md` Task 59）

`scripts/mutation-m8.sh` 放**回归代价低**的 4 条，收口时重跑；其余在执行期逐条
实测，记录在下面。纪律同 `mutation-m3-notes.md` §0：**变异前 `cargo clean -p`**、
**同一调用内完成变异与恢复**、每条都必须看到 `test result: FAILED`。

汇总：**8 条红 / 0 条失手**（另有 17 条执行期附加变异，其中 2 条绿，结论见 §3）。

---

## §1 脚本里的 4 条（`bash scripts/mutation-m8.sh` ⇒ 红 4 / 失手 0）

| # | 变异 | 文件 | 判据 | 结果 |
|---|---|---|---|---|
| M-M8-01 | range-func 的 `continue` 不翻译成 `return true` | `rgoc-hir/src/lower.rs` | `con_rangefunc` 2 条红（`t_con_45` / `t_con_49d`） | 红 |
| M-M8-02 | `select` 子句体不走 `push_clause_seq`（不拦无标签 break） | `rgoc-interp/src/interp.rs` | `con_select` 1 条红（`t_con_60`） | 红 |
| M-M8-03 | `try_send` 的 `closed` 检查整段删掉 | `rgoc-interp/src/chan.rs` | `chan_obj` 2 条红（`closed_send_必须报_closed…` / 另一条） | 红 |
| M-M8-04 | 死锁判定不落地（`if false`） | `rgoc-interp/src/interp.rs` | `con_exit` 2 条红（nil recv / nil send） | 红 |

### M-M8-03 踩的坑：探针目标是 `--lib` ⇒ **假绿**

第一版把测试目标写成 `rgoc-interp --lib`，而判据在 `crates/rgoc-interp/tests/chan_obj.rs`
（集成测试）里 —— `--lib` 只跑 `src/` 下的单测，于是一条判据都没跑到，探针报 GREEN。
改 `--test chan_obj` 后立刻红 2 条。

**教训**：变异探针的「测试命令」和判据所在的目标必须逐个对齐；`--lib` / `--test <name>` /
`-p <crate>` 三者不是一回事。这一条比变异本身更值得记。

---

## §2 执行期实测的另外 4 条

| # | 变异 | 文件 | 判据 | 结果 |
|---|---|---|---|---|
| M-M8-05 | 合成函数形参名退回 `v0`（不用 `:=` 左值的名字） | `rgoc-hir/src/lower.rs` | `con_rangefunc` 11 条红（body 解析不到迭代变量） | 红 |
| M-M8-06 | `=` 形态不写回左值（`if false` 包住 prelude） | `rgoc-hir/src/lower.rs` | `con_rangefunc` 1 条红（`t_con_49`：`after 2` 变成 `after 0`） | 红 |
| M-M8-07 | `next_token` 从 0 起（与「普通 waiter 无 token」撞车） | `rgoc-interp/src/interp.rs` | `con_select` 死锁（`revoke(0)` 把阻塞的普通发送方一起删掉） | 红 |
| M-M8-08 | 合成函数体尾不补 `return true` | `rgoc-hir/src/lower.rs` | `con_rangefunc` 11 条红（`yield` 恒返回零值 `false` ⇒ 只跑一轮） | 红 |

M-M8-07 的形态值得单独说：它不是「某条断言红」而是**死锁** —— `revoke(0)` 把
token=0 的普通 waiter 一起删掉，阻塞的发送方再也没人唤醒。这类「变异表现为挂住」
的判据在脚本里不好自动判（要靠 timeout），所以留在 notes 里。

---

## §3 执行期附加变异（17 条）与 2 条绿的结论

Task 45–48 与 select break 修复期间另跑了 17 条附加变异。**15 条红**，2 条绿，
两条都有结论（不藏）：

### 绿①：`break` 不翻译（`if false &&` 包住翻译支）

**现象**：删掉 `break → return false` 的翻译，全部 15 条判据仍然绿。

**原因**：两条路**重合**。不翻译时 `Stmt::Branch{Break}` 落在合成函数里，
`branch()` 的 Break 臂找不到可 break 的构造就把 body 栈弹空 ⇒ 下一步
`next_stmt` 走 `do_return(空)` ⇒ 结果槽保持零值 `false` —— 恰好等于翻译目标。

**处置**：**保留显式翻译**。理由：① 不依赖「解释器的 break 落空行为」这个巧合；
② HIR 自描述 —— dump 里看到的是 `return false` 而不是一个指向虚无的 `Branch break`；
③ 规范原文就是「yield returns false」。同时在 `con_rangefunc` 里用
`t_con_44`（break ⇒ 迭代器看到 false ⇒ `iter-stop`）钉住**译成错的值**会红
（实测：译成 `true` ⇒ 2 条红）。

### 绿②：`breakable` 不看标签（`*breakable || …`）

**现象**：让 select 子句体也拦**带标签**的 break，判据全绿。

**原因**：**本前端不可达**。具名块要算「已使用」只能靠 `goto L`（`break L` 打块标签
在官方 go 里是编译错误 `invalid break label`），而块在循环内就不是函数体顶层块
⇒ 被 goto 窄面拒（`HIR-UNSUPPORTED-GOTO`）；块在顶层则无标签 break 一定先被内层
for/switch 拦住。官方 go 接受 `L: { break; if false { goto L } }`（输出 1），
我们拒 —— 这是 goto 窄面（`U-M5`）的既有代价，不是新缺口。

**处置**：不编造一条到不了的判据。结论记在这里。

---

## §4 复现

```bash
# 容器内
bash scripts/mutation-m8.sh          # ⇒ 红 4 / 失手 0
```

§2/§3 的条目在执行期用同款 `probe` 手法逐条跑过（改 → `cargo clean -p` → 跑 → 恢复 →
再 clean），文本与期望输出都记在 `.superpowers/sdd/M8-plan/progress.md` 的对应 Task 下。
