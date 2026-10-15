# 项目长期记忆 —— rust_go_compiler (`rgoc`)

> **只保「每轮都要知道」的硬事实**；细节一律去 `AGENTS.md`（工程入口，任何任务先读它）、
> `docs/`、`.superpowers/sdd/*/progress.md`（Ruling / 残留表都在那儿）。

## 概况
- 用 **Rust 重写 Go 编译器**，代号 `rgoc`；remote `origin` = github.com/nobody0726/rust-go-compiler
- 语料 `go_source_code/`（185 MB / 15,618 文件，`VERSION`=go1.27.1）**只读不入库**，由 `corpus-manifest.sha256` 锁定
- 首发平台 **Linux/arm64（Docker）→ `aarch64-unknown-linux-gnu` / ELF**，不维护 macOS 第二套
- git 身份 `nobody0726` / `102368194+nobody0726@users.noreply.github.com`；见到 `wangf <yhome@…local>`
  即身份丢失，用 `git config --global` 重设（`gh config set git_user_*` 已废弃）
- **提交只在用户明确要求时执行**；push 被拦时走已登录 `gh api` 建 blob→commit→update ref

## 状态
- **M0 全部完成**；**M1 完成**；**M2（AST/parser）进行中**：native/inline `executing-plans`，
  ledger = `.superpowers/sdd/M2-plan/progress.md`（**改代码前先读它**）
- M2 已完成：T-AP01..T-AP21 全部（含 PA-1）—— 八条门禁 A1–A8 全过；
  测试 **976 条全绿**；oracle 对撞 31/31、双向 ERROR 全过、标准库 6670 文件
  零错误、深度上限 1000；C1 §2.3 / C6 新建 / GAP-01 +53 项 / M2-manifest.json
- **下一个 = M3**（符号与类型检查入口）；遗留：`-lang` 门控（T-A-97 deferred）、
  `//go:` 未知指令待用户裁决（详见 M2-manifest.json open_gaps）
- **commit 纪律变更（2026-10-15 用户授权起）**：阶段收口即可 commit + push，
  不再等逐次开口；身份 `nobody0726`，push 被拦走 `gh api` 建链

## 环境硬事实（每轮都踩得到）
- **所有构建/测试在容器内**：`RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && …'`
  （工作目录必须是 `/work/rgoc`，不是 `/work`；探不到 daemon 即报错，**不降级**）
- `rgoc/target/` 在命名卷 `rgoc-target`：新建空卷属主 root:root → 先 `chown 501:20`；
  `cargo clean` 会 EBUSY(16) → 用 `find rgoc/target -mindepth 1 -delete`；
  卷属主错 ⇒ `docker run --rm -v <vol>:/x alpine chown -R 501:20 /x`（**uid 是 501 不是 1000**）
- 四条门禁：`cargo fmt --check` / `cargo check --workspace --all-targets` /
  `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace --no-fail-fast`
- ⚠️ **`/Volumes/MacBookData` mtime 只到整秒 + 容器挂载有内容缓存** ⇒ 写盘后必须在容器里
  **`md5sum` 轮询到一致**才准跑测试；同一根因让宿主 `grep` 读陈旧缓存 ⇒
  **查代码一律用 Grep/Read 专用工具，不用 Bash grep**
- ⚠️ **光核 md5 还不够：cargo 会不重建、直接复用旧产物**。⇒ 跑测试前先在**同一容器调用里**
  `touch` 源文件（**被改的那个文件本身**，不是 lib.rs），且输出出现 `Compiling <crate>`
  （`check`/`clippy` 打印的是 **`Checking`**）。新形状（T-AP18）：变异后不 touch 被改文件
  ⇒ M2–M4 全报 M1 的同一组失败、**复原后还红** = 陈旧二进制污染判定。
  另：变异必须**可编译**（`push("{")` 不编译=白测）、方向要过脑（禁 A 臂后全落 B 臂=行为等价）
- ⚠️ **容器里的 `/tmp` 每次调用都是新的**（不是持久卷）⇒ gate 日志**必须在同一次调用里读**；
  T-AP19 新形状：变异 runner 把**备份文件**放 `/tmp`、下次调用恢复 ⇒ `cp` 静默失败、
  变异态留真文件 ⇒ 变异与恢复必须同一次调用内完成（或宿主侧还原）
- ⚠️ **变异 runner 的「复原」必须覆盖本组 edits 的每一个文件** —— 用循环残留的 `path` 只复原最后一个
  ⇒ 其余**永远留在变异态** ⇒ 从那一条起每次都「编译不过」，症状**极具误导性**。
  脚本模板 `/tmp/rgoc-mut/runner_tap14.py`（最新）
- ⚠️ zsh 会吃掉 `${!want[@]}` 这类数组展开；容器同步 md5 循环用两个显式变量写
- `cargo new <dir>` 目录名不能叫 `crate`；中文标识符**不能夹大写 ASCII**（触发 `non_snake_case`）
- 计时器：宿主 `/usr/bin/time` 用 `-p`；容器内无它（用 bash 内建 `time`+`TIMEFORMAT`）
- 验证命令别 `cmd | tail -N` 串 `&&`（退出码取自 tail）→ 脚本首行 `set -euo pipefail`
- ⚠️ **容器内 `go build` 要 `-buildvcs=false`**（容器 git 对 /work 报 dubious-ownership 128；
  `go run` 不盖 VCS 戳不受影响）。Go 探针/golden 一律 `RGOC_TTY=0` 抓
- ast-oracle：`go run .` 在 `tools/ast-oracle/` 目录内跑（自带 go.mod）；
  dump 逐字节契约 = `rgoc-ast::dump.rs`（不是 M2-tests §5.2 的草案示例）

## 纪律（每条都花了代价）
- **TDD**：RED→GREEN→REFACTOR；先确认失败来自缺行为而非环境/harness
- ⚠️ **断言必须反向验证真能失败**；本仓已抓 6+ 次「恒真断言」。共同形状：**「漏了」不会报错，
  只让断言恒真**。两条**骗人方式不同**的形态：① **fail-fast 截断** ⇒ 必须 `--no-fail-fast`
  **且遍历所有 `failures:` 区块**（用 `re.finditer`，只读第一段会丢掉新跑出来的红）；
  ② **panic-based 断言用错原因**（`fnest: usize` 的**下溢 panic**也会满足「有没有炸」）
  ⇒ 必须 `downcast` 核对 payload。
  ③ **「反向验证」用例的**前置条件**与被测行为相关** ⇒ 变异后前置不成立、整段被跳过
  （T-AP16 的 `_Name` 裸拼写断言：前置写 `starts_with("unexpected name ")`、变异后是**逗号**分隔）。
  ⇒ 前置条件必须与被测行为**独立**；**写不出判据时删掉它并写下「为什么写不出」，别硬凑恒真断言**
- ⚠️ **抄 Go 的「遍历 + 原地压缩」循环时，必须保留「range 遍历副本」这个隐含快照**：
  `for _, v := range s { if 匹配 { /* 不动 i */ } else { s[i] = v; i++ } }; s = s[:i]`
  直译成 `while i < s.len()` ⇒ **匹配时不递增 `i` 且 `len()` 不变 ⇒ 死循环**。
  症状是 **OOM 被 SIGKILL**（不是报错），且**只在该路径的用例上出现**（T-AP16：8 条含 `goto` 的
  用例全挂，看着像「`goto` 一律坏」）⇒ 显式 `let snapshot = v.clone();`
- ⚠️ **变异 runner 的命令**必须走 `scripts/in-container.sh` —— `/work/rgoc` 只在容器里存在，
  宿主上 `subprocess.run(shell=True)` **静默返回空输出** ⇒ 下游看到「没有 `Compiling` 行」
  报「cargo 复用旧产物」，真因是「命令压根没跑」。**「命令失败」与「命令成功」同形**。
  另：`touch lib.rs` 后 cargo 仍可能复用 ⇒ 必须先 `cargo build --workspace --tests`
- ⚠️ **凡是从「生成的代码」里读数据，用脚本算，别用眼睛数** —— 我手数官方 `token_string.go`
  的 47 项索引表得到「46 token 对 48 索引」⇒ **整张表全错**。用 `re` 抽出来按官方公式逐项算才对上
- ⚠️ **「能编译 + 断言全绿」对数据模型不构成正确性证据** ⇒ 建模类任务验收**必须含
  「对照官方源码逐字段 diff」，且 diff 对象要含**可空性 / 零值可达性 / 字段的类型**。
  4 次同形：PA-1 字段少（16 处）/ `Op::Tilde` 变体少 / `IfStmt.cond`（恢复路径上保持零值）/
  `CallStmt.call`（字段类型比官方窄）⇒ **只有走完官方每条出口、逐字段核类型才发现**；
  写窄了的症状：**照常编译、照常不报错，只是那条路径上无路可走**
- ⚠️ **抄官方的「三档判定」函数时，兜底档必须回源逐字核对** —— 前两档各有专属注释容易抄对，
  兜底档最容易被「按前两档的直觉」补出来（T-AP15 连中两枪：`RelCol` 兜底是 `pos.Col()`
  不是 `b.col`；`updateBase` 的 `col==0` 检查对象是 `col` 不是 `n2`）
- ⚠️ **`clippy -D warnings` 常见炸点**：`collapsible_if`（照抄官方嵌套 `if` 时必然遇到）→
  用 **let-chain** 直译官方的 `&&` 合取，**别**拆成两个 `match` 臂（会改变同形度）；
  `doc_lazy_continuation`（文档里以 `+`/`-` **开头**的行被认成列表项，后续行必须缩进）→
  **改措辞**，**别**加 `#[allow]`；`large_enum_variant`（字段变大顶破阈值）→ **先试 `NonZero`**
  空位优化并**实测**，不够再带理由地 `allow`（别直接 `Box` 字段：多一层间接 + 偏离建模）；
  `enum_variant_names` → 改枚举名（保留变体名与官方指令名的对应关系）
- ⚠️ 断言关键词必须锚定**真实字段**（T37/38/39/47 四轮同形）；注释里的同名串会让断言恒真 → 作用于 `code_only()`
- ⚠️ 不能用 `None` 冒充「未实现」：官方 `None` 有确切语义 → 骨架未实现臂用 `todo!`
- ⚠️ **绑在「功能未实现」上的测试，落地那天必然失败** ⇒ 实现它时**必须同时改写成别的判据**
  （T-AP07 的 `fnest` A/B 单测从「panic」改成「**语句条数**」，且 **A/B 双向**都要断言）
- ⚠️ **RED 的价值在「断言写错的那些」之外** ⇒ 别因「失败多是笔误」就跳过 RED
- 完成度按**四维**跟踪（Frontend/Interpreter/Native/Runtime-metadata），不以「246 项全勾选」为退出条件
- 只读官方语料，不改 `go_source_code/`

## M2 关键硬事实
- **oracle 对撞验收**（D-M2-1）：vendor 官方 `cmd/compile/internal/syntax`，双方输出「中立 dump」逐行 diff
- 官方测试驱动器 = `src/cmd/internal/testdir/testdir_test.go`（**不是** `test/run.go`）；指令集 16 个；
  `errorcheck` 裸写会自动加 `-d=ssa/check/on`（其期望不能当纯语言语义验收）
- **类型与表达式同族**：`type_()` 返回 `Expr`（官方亦然），`AST-24..32` 是 `Expr` 变体，
  **不新建 `TypeExpr`**；官方**无 `BadStmt/BadDecl/BadType`** → 错误恢复返回 `None`
- `Follow` 表达官方 followlist 的**词元类别**；⚠️ **`prec` 是词法层知识，绝不能抄进 parser
  followset**（抄第二份优先级表 ⇒ 抄错那份永远不命中，症状是恢复点提前停住**且不报错**）
- ⚠️ **节点位置只能取 `Expr::pos()`，不能取 `self.tok.pos`**（`a.b` 的位置是 `a`；`f(x)` 在 **`(`**——
  `nodes_test.go:158` 的 `f@()` 是权威判据，`ExprStmt.pos = lhs.Pos()`）
- ⚠️ **`prec` 与 `form` 正交**，判「是不是二元运算符」必须写 `form == Plain`：`+=` 是 `(Assign,Add)`、
  `++` 是 `(IncDec,Add)` ⇒ **`prec` 与 `+` 一样是 4** ⇒ 只比 `prec` 会把 `x += 1` 读成 `Binary`。
  症状**故意骗人**：报错落在 `++` **后面**那个词元上。**`prec` 是 `u8`，装不下 `form`**
- `p.op == X` 的替身 = `Parser::op_is(Operator)`（只比 `(Plain,op)`）；`p.tok == _X`（`_X` 带载荷）
  的场合用 `at_follow(&Follow)`（**判类别**）；⚠️ `Follow::Semi` 必须认**三种来源**
- 官方的临时 `nil` 用**短命中间形态**表达（`PartialField` / `ArrayOrTArgs`），**不用 `Expr::Bad` 当哨兵**
  （会被 `Type != nil` 算成「有类型」）
- T-AP09：`extractName` 的 `[` 二义 = **先解析成表达式、再倒着拆**；`force` = 「`[` 后面以 `,` 结尾」。
  `funcDeclOrNil` 三坑：① recv 的 `0` 与 `default` **既报错又取第一个**；② 认不出名字造 `"_"` 假名 +
  空 `FuncType` 继续；③ 体可选（`body == None` = 前向声明，**不报错**）。
  `func` 是**唯一不走 `appendGroup`** 的顶层声明
- T-AP13：**`header` 是全仓唯一把 `xnest` 置负的地方**（RF1），**三条出口都要恢复 `outer`** ——
  漏一条 ⇒ 之后整个文件不再允许复合字面量，**症状是树形悄悄变浅（不报错）**。
  ⚠️ **RF1 端到端输入必须分两行**（`if x == T{}\n{ f() }`）：同一行时官方 `stmtList` 的恢复会冲到
  后面那个 `}` 把块语句吞掉 ⇒ 两读法都只剩 1 条、用例失效。⚠️ **`IfStmt.cond` 是 `Option<Expr>`**
  （官方 `header` 的具名返回值在 `if x y {}` 路径上**从不赋值**）
- T-AP14：`stmtOrNil` 按官方拆成 `stmt_or_nil` / `stmt_or_nil_rest` 两个方法（`_Var/_Const/_Type` 在
  `clearPragma()` **之前**）⇒ 界线**在类型上可见**。三条分支臂的标签**三种差别**：
  `fallthrough` **不读**、`break/continue` **有名字才读**、`goto` **无条件** `name()`（缺标签时
  **造假名 `_`**）。`call_stmt` 的 `keep_parens` **只为**那条 `expression in %s must not be parenthesized`
  （走 **`errorAt`：无 `syntax error: ` 前缀、不受 EOF 抑制**；位置是**括号**不是 `go`；`%s` 随 `go`/`defer` 变）。
  `return_stmt` 的条件是**两个**（`!= _Semi` **且** `!= _Rbrace`）。⚠️ 官方 `syntaxErrorAt` 按**消息前缀**
  分**三**支，**我们只实现 `default` 支** ⇒ **只有 `default` 支的消息能逐字断言**
- T-AP15：`Pos` 加了 `base: Option<BaseId>`（官方同构；用户 2026-10-15 拍板改 M1 冻结产物），
  `BaseId` 用 **`NonZeroU32`**（空位优化 ⇒ `Option<BaseId>` 只 4 字节），
  指针链 → **表 + 下标**（`PosBases`）⇒ `Pos` 仍 `Copy`/`Ord`/`Hash`。⚠️ **file base 的 `line`/`col`
  必须是 `1`/`1`**（官方 `linebase`/`colbase`），否则全仓 `rel_line` 都是 0。
  `base == None` ⇒ `rel_*` 退回绝对 ⇒ M1 行为逐条不变。`Parser::next()` 的顺序是
  「取词元 → **处理指令消息** → 按当前 base 重算 `pos`」，**反了会让 `//line` 后第一个词元用旧基准**。
  ⚠️ `go_version = take()` 与 `top = false` 的**先后不可区分**（lexer 懒扫描 ⇒ 两个顺序同值）——
  早先注释写「必须在之前，反了任何文件都取不到」是**错的**，已被变异证伪并改成如实描述。
- T-AP16：**`want(f, expected: &str)` → `want(f)`** —— 早先 30 个调用点各自手写期望词元的名字，
  **大部分与官方 `tokstring` 不符**（`expected ;` 应是 `expected semicolon or newline`；
  `expected chan` 应是 `expected keyword chan`；裸 `for` 应是 `keyword for`）⇒ 现在由
  `tokstring(&Follow)` **一处派生**。⚠️ 症状只是消息里几个字符不同，**要到 T-AP18 对撞才暴露**。
- T-AP16：`syntaxErrorAt` 的三支判据是**消息的前缀**（不是 `DiagCode`）：
  `""` → 拼 `<tok>` 不加中缀 / `in `·`at `·`after ` → 前置**一个空格** / `expected ` → 前置 `", "` /
  **其余 → 原样透传、且不拼 `<tok>`**。`<tok>` 六档：`_Name`→`name <字面>`、`_Semi`→**`p.lit`**
  （`newline`/`semicolon`/`EOF`，**都不是 `;`**）、`_Literal`→`literal <字面>`、
  `_Operator`/`_IncOp`→**裸 `op.String()`**、`_AssignOp`→`op.String()+"="`。
  ⚠️ **`++` 的 `op` 是 `Add`** ⇒ 消息里是**一个** `+`（官方 `operator_string.go` 无 `IncDec` 变体）。
- T-AP16：`checkBranches` 的门控是「**本函数**无语法错」（官方 `errcnt == p.errcnt`，两个
  `errcnt` 分别在 `blockStmt` 前后取）—— 写成「整个文件干净」会让一个坏 import 让**全文件**
  分支检查消失。官方 12 处 `errf` 收敛成 **8 个 `SYN-` 代号**（`DiagCode::ALL` 26 → **34**）。
  ⚠️ **`BranchStmt.Target` 刻意不建模**：它是**回指**（`Rc` 会破坏 `PartialEq` 的深比较）、
  **不参与** dump、只被 M3 类型检查读。
- 已知缺口（细节见 ledger 残留表）：`lit_text()` 的有损 UTF-8 转换（T-AP17/18）；
  `//line` 的 `filepath.Clean` + 相对路径拼接（M3 才有 SourceMap）；`constraint.Parse` 完整表达式
  （只取首个 `goX.Y`）；`Pos` 的 `Ord` 仍按**绝对**坐标（官方按 `Rel*`）；`parse_file` 无 `-lang` 参数

## 文档索引
- `docs/`：`README` / `01-feature-set`(246) / `02-test-inventory` / `03-roadmap`(M0–M12) /
  `04-development-environment`；`spec:Lxx-yy`/`nodes:Lxx-yy` 引用前缀定义在 `01` §0.4
- M0：`M0-{design,tests,plan,benchmarks}.md` + `M0-manifest.json`；环境陷阱 14 项在 `M0-benchmarks.md` §6
- **VSCode/调试三个复发点（E5 复验必看）**：① 宿主 `http.proxy` 被下推进容器 → 跑
  `scripts/install-codelldb.sh`；② VS Code Server 在持久卷 `/vscode`，宿主升级即复发 →
  `scripts/install-vscode-server.sh --commit <c>` + Reopen；③ 容器重建后 CodeLLDB 平台包必丢 →
  重跑 `install-codelldb.sh`。`launch.json` 两坑：工作目录取自 `cargo.cwd`（不读顶层 `cwd`）；
  `filter.name` 比 **target name（下划线）**
- 自检脚本 `check-m0-consistency.py`（**231 条**断言）+ `check-m1-consistency.py`：**在宿主用
  python3 跑**（M0 脚本自己会调 docker）；末尾 TOTAL 计数断言**必须是文件最后一条**；
  断言须作用于 `code_only()` 且**锚定真实字段**；改断言数同步 `AGENTS.md` §1；门禁状态**两处同步**
  （manifest + `M0-plan.md` 汇总表）