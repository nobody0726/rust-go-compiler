# 项目长期记忆 —— rust_go_compiler (`rgoc`)

> 只保「每轮都要知道」的硬事实；细节去 `AGENTS.md`（工程入口，任何任务先读）、`docs/`、
> `.superpowers/sdd/*/progress.md`、`docs/milestones/M2-manifest.json`（open_gaps）。

## 概况
- Rust 重写 Go 编译器 `rgoc`；origin = github.com/nobody0726/rust-go-compiler（main）；语料 `go_source_code/` 只读不入库；首发 Linux/arm64（Docker）→ aarch64 ELF
- git 身份 `nobody0726` / `102368194+nobody0726@users.noreply.github.com`；见到 `wangf <yhome@…local>` 即丢失，`git config --global` 重设
- **commit 纪律（2026-10-15 授权）**：阶段收口即可 commit + push（按交付块分 commit）；push 被拦走 `gh api` 建链；`.superpowers/` gitignore；`tools/ast-oracle` vendor 入库

## 状态
- **M0/M1/M2 全部完成**（M2 = T-AP01..21：A1–A8 全过、oracle 对撞 31/31、stdlib 6670 文件零错、深度上限 1000）
- **M3（符号与类型检查入口）进行中**：native/inline `executing-plans`，ledger =
  `.superpowers/sdd/M3-plan/progress.md`（**改代码前先读它**），门禁唯一入口 =
  `.superpowers/sdd/M3-plan/gates.sh`（容器内 `bash /work/.superpowers/sdd/M3-plan/gates.sh`）。
  已落地 `rgoc-const` / `rgoc-types` 两 crate + `tools/types-oracle`；**Task 1–18 完成**（C7 契约、
  常量层、类型表示/dump/同一性），下一批 = Task 21+（版本门控 / 包与作用域 / 装载 / 对撞接线 / manifest）
- ⚠️ **本地与远端是平行历史**：本地 main = M3 的 ~20 个提交（基于 0d267ea），远端
  origin/main = 43843d6。**别 force push**；收敛走 `git fetch origin` → `git rebase origin/main`
  （或 merge），评估后再推。`2026-10-16.md` 记了这段分叉的来龙去脉
- 遗留：`-lang` deferred（M3 的 Task 21 会落地按文件版本门控）、`//go:` 未知指令裁决（M3 收下不报错）

## 环境硬事实
- 构建/测试全在容器：`RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && …'`
  （cwd 必须 `/work/rgoc`；探不到 daemon 即报错**不降级**）
- 四条门禁：fmt --check / check --workspace --all-targets / clippy --all-targets -D warnings /
  test --workspace --no-fail-fast
- target 卷属主 root → `docker run --rm -v <vol>:/x alpine chown -R 501:20 /x`（**uid 501**）；
  `cargo clean` EBUSY → `find rgoc/target -mindepth 1 -delete`
- ⚠️ /Volumes/MacBookData mtime 整秒 + 容器缓存 ⇒ 写盘后容器内 md5sum 轮询一致才跑测试；查代码一律 Grep/Read 工具
- ⚠️ cargo 陈旧产物：同一次容器调用里 touch **被改文件本身** + 核验 `Compiling`（check/clippy 是 `Checking`）；「复原后还红」= 陈旧二进制污染
- ⚠️ 容器 `/tmp` 调用即焚；**变异与恢复必须同一调用内完成**；变异 runner 走 in-container.sh（宿主 subprocess 静默空输出）；复原覆盖本组每个文件；变异必须可编译
- zsh 吃 `${!arr[@]}`；容器 `go build` 要 `-buildvcs=false`；脚本首行 `set -euo pipefail`；ast-oracle `go run .` 在 `tools/ast-oracle/` 内跑，dump 契约 = `rgoc-ast::dump.rs`

## 纪律（案例细节在 ledger / commit message）
- **TDD**：RED→GREEN→REFACTOR；先确认失败来自缺行为而非环境
- ⚠️ 断言必须反向验证真能失败（已抓 6+ 恒真断言）：① `--no-fail-fast` 且遍历**所有** failures 区块；② panic 断言 downcast 核 payload；③ 前置条件须与被测行为**独立**，写不出判据就删
- ⚠️ 抄 Go「遍历+原地压缩」循环保留 range 快照（显式 clone），否则死循环 OOM；抄官方三档判定函数，**兜底档**回源逐字核对
- ⚠️ 生成代码的数据用脚本算别眼数；建模验收必须对照官方**逐字段 diff**（可空性/零值/类型）
- clippy 炸点：collapsible_if → let-chain；large_enum_variant → 先 NonZero 实测；doc_lazy_continuation → 改措辞
- 断言锚定**真实字段**、作用于 code_only()；不用 `None` 冒充未实现（用 `todo!`）；绑「未实现」的测试落地时必须改判据
- 完成度按**四维**跟踪；只读 `go_source_code/`

## M2 硬事实（M3 仍会用；细节在 ledger / commits d279ae4..37ef6e5）
- 类型与表达式同族：`type_()` 返回 `Expr`，不建 TypeExpr；官方无 BadStmt/BadDecl → 恢复返回 None
- ⚠️ `prec` 与 `form` 正交：判二元运算符必须 `form == Plain`；`prec` 绝不进 followset；节点位置取 `Expr::pos()` 不取 `self.tok.pos`（`f(x)` 在 `(`）；`header` 三条出口都恢复 outer
- `IfStmt.cond` 是 `Option<Expr>`；`Follow::Semi` 认三种来源；官方临时 nil 用短命中间形态，不用 `Expr::Bad` 当哨兵
- `want(f)` 消息由 `tokstring(&Follow)` 一处派生；`syntaxErrorAt` 只实现 default 支；`checkBranches` 门控 = **本函数**无语法错；`BranchStmt.Target` 刻意不建模（回指，**M3 读**）
- `Pos.base = Option<BaseId>`（NonZeroU32）；file base line/col 必须 **1/1**；base==None ⇒ rel_* 退回绝对
- **DiagCode 五件套**：变体声明序=字典序（derive(Ord) 按声明序不是 ALL 序）+ ALL + as_str + 两个总数测试；现 **43** 个（M1 的 23 `LEX-` + M2 的 12 `SYN-` + M3 的 8 `TYP-`）
- 深度守卫挂**三条递归边**：`expr` / `stmt_or_nil` / `type_or_nil`；超限**恰好 1 条** SYN-NESTING-TOO-DEEP

## 文档索引
- `docs/`：README / 01-feature-set(246) / 03-roadmap(M0–M12)；契约 C1–C6；M2-manifest.json；M0 环境陷阱 14 项在 M0-benchmarks.md §6；VSCode/调试复发点与自检脚本用法见 `AGENTS.md`
