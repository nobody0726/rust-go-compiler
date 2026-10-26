# 项目长期记忆 —— rust_go_compiler (`rgoc`)

> 只保「每轮都要知道」的硬事实。细节去 `AGENTS.md`（工程入口，任务先读）、`docs/`、
> 当前阶段台账（`.superpowers/sdd/M{5,6}-plan/progress.md`）、`docs/milestones/M*-manifest.json`。

## 概况
- Rust 重写 Go 编译器 `rgoc`；origin = github.com/nobody0726/rust-go-compiler（main）；`go_source_code/` 只读不入库；首发 Linux/arm64（Docker）→ aarch64 ELF；规格基准 go1.27.1
- git 身份 `nobody0726` / `102368194+nobody0726@users.noreply.github.com`；见到 `wangf <yhome@…local>` 即丢失，`git config --global` 重设
- commit 纪律（2026-10-15 授权）：阶段收口即 commit + push（按交付块分 commit）；push 被拦走 `gh api`；`.superpowers/` 已 gitignore；`tools/ast-oracle` vendor 入库

## 当前状态（2026-10-25）
- **M7/M8 已收口**；**M9 进行中**：Phase 0/1/2/3/3b/4a/4b 已收口；**5a（defer+panic+recover）代码通但未收口**。
  下一阶段 = **5c 收尾（值接收者解引用拷贝 + invalid 类型 + nil 打印）→ 4c（方法值 → method5）**
- **M9 语料现态**：native **ok=65 / skip=1 / bad=3**（`method` / `nilptr2` / `print`）；
  `cargo test --workspace` **2150 passed / 9 failed**（全是分母/终止符/defer+recover 判据，
  **故意不重冻结**）；三个 bad 的精确病因见当日日志（值接收者解引用拷贝 / invalid 类型 / nil 打印格式）
  `rgoc-meta`（新 crate）+ desc/方法表/ITab 发射 + `rt.c` chunk 堆（mmap，`RGOC_HEAP_MAX` 缺省 256 MiB）
- ⚠️ **panic 用「返回码传播」不 unwind**（`.framemap` 不在 ELF 里 ⇒ `longjmp` 会漏扫根）：
  `rt_panic_*` 只置状态，每个调用/断言/越界点之后 `bl rt_panicking; cbnz x0, <本帧出口>`；
  出口 = 跑 defer + `ret`。**跑 defer 期间必须挂起 panic**（被 defer 调用的函数是正常执行的）
- ⚠️ **`check_panic` 用 `cbnz` 直接跳**，别造「跳过」标签（同函数多处检查会重复定义符号）
- ⚠️ **SSA 拒绝面三次重冻结 46/23 → 50/19 → 56/13 → 57/12**（`R-9-8`/`R-9-12`/`R-9-14`）。
  剩下 12 条全是 **FUNC-VALUE 8 + DEFER 4** ⇒ Phase 4 是 **+8**、Phase 5 是 **+4**（计划写 +7/+3）。
  按码分类的清单只告诉你「第一条拒绝是什么」，不告诉你「这个文件只剩这一条」
- ⚠️ **码会整条下线**：`MAP`/`IFACE-DYN`/`RANGE-FUNC` 都已没有生产者，`SsaCode::ALL` 8 → 7。
  下线要在 manifest 记 `retired_codes`（code/retired_by/revision/why），并把判据反转成
  「能建图 + 该码不出现」—— **不留永远不产出的码**
- ⚠️ **求值器与后端必须同口径**（`R-9-13`）：方法接收者在 IR 里**总是地址**（`Field` 只认
  `Value::Ptr`）。`ssa_exec` 的接口派发曾按值下降 ⇒ `panic: 地址不是指针`（native 却对）
- ⚠️ **`Instr::Index` 的 base 必须是地址**（`R-9-9`）：右值聚合（调用结果 / map 值 / 复合字面量）
  当底座会**把数组内容当地址** ⇒ 段错误。`for k := range a` 不塌只因为 key-only 不发 `Index`。
  同理 `Field`/`Index` 的 dst 类型必须是 `*成员类型`（`R-9-6`，沿用底座类型会算错偏移）
- ⚠️ **manifest 是现状、报告是历史**（`R-9-8`）：M6 `corpus_lists` 计数必须等于磁盘，离开原值
  就要写 `re_frozen`；历史里程碑的自报数字对着**快照**算（`native-input-m7.txt` / `native-rejected-m7.txt`）
- ⚠️ **每次 `in-container.sh` 是新容器**：上一步 build 的 `/tmp/x` 下一步就没了（像莫名 rc=127）。
  产物落仓库内目录
- **M0–M3 完成**；**M4 部分兑现**（`CH-1..9` = 6 pass / 1 partial / 2 fail）；**M5 部分兑现**（`EX-1..9` = 5 pass / 4 partial）
- **M6 已收口（2026-10-24，Task 1–53）**：九条门禁 9/9 pass；语料 **N=46 / M=23**；
  `--via=hir` == `--via=ssa` 46/46；`--opt=all` == `none` 46/46；`SSA-LIVE v1` 可被只读文本的桩消费；
  **176 suite / 1859 passed**；66 条变异全红。下一阶段 = **M7（native 代码生成）**
  - 语料 **N=46 / M=23 已冻结**（`R-6-1`）：23 行三栏落 `tools/run-oracle/ssa-rejected.txt`
    （IFACE-DYN 9 / FUNC-VALUE 7 / MAP 5 / DEFER 2）；`ssa_slice.rs` 两向断言
  - **三方差分绿**：`run-collide.sh --ssa` ⇒ 69 = 绿 46 / 跳过 23 / 红 0；
    `run --via={hir,ssa}` 两条通路在 46 个上逐字节一致（`ssa_diff.rs` + `ssa_corpus_run.rs`）
  - 白名单 **9 条**（`R-31-1` 补 FUNC-VALUE / RANGE-STR；`U-M6-05/06/07` 够不着 ⇒ 守卫测试 `R-31-2`）
  - `ssa` 子命令已接真管线（dump→stdout / 拒绝与 verify→stderr / 退出码 0·1·2）
- **M6 五件套**：`M6-design.md`（`D-M6-1..16` / `F1..F14` / 门禁 `SSA-1..9`）、`M6-tests.md`（64 个 `T-SSA-*` + 44 条修订）、
  `M6-plan.md`（Task 1–53 + 插入 32b/33b）、`M6-manifest.json`（已填实，`phase_summary.done`）、`M6-report.md`（交付 B）
- ⚠️ **`T-SSA-41`（构建预算 1e5）未实现**（`G-M6-01`）：设计稿有判据、计划无 Task。M7 补
- ⚠️ **文档驱动 golden 是唯一能防契约漂的写法**（`R-51-1`）：抄进测试的期望值只能证明
  「实现与那份副本一致」—— 改成从 `C10` 读之后一次抓出三处文档错
- 五条 M6 铁决策：SSA 输入=**HIR**、**单包**、phi=**块参数**、多返回值=**多结果指令**（tuple 不进 IR）、memory **按根分区**（Global/取址 Slot/Heap）；
  代码落 `rgoc-ssa`（纯数据）+ `rgoc-interp::ssa_exec`（复用 `Value`）；passes = fold/dce/copy + **块内 CSE**
- **里程碑收口的四要素**：代码 + 台账 + manifest 对账测试 + 报告。少一样就只是「跑得起来」，不是收口
- ✅ **`Program::init_order` 已进 SSA**（Task 32b / `R-32-1`：合成 `init` 函数，`Module::init`）
- ⚠️ **debug 构建吞吐 ~9.5M 步/秒** ⇒ 默认预算 1e8 要 10.4 s；任何「默认预算跑死循环」
  的判据都会踩 10 s 线（`R-36-1`：墙钟判据用显式 1e7）
- ⚠️ **指针根归属是构造性记录**（`Fb::ptr_root`，只认 `AddrOf`/`Field`/`Index`/指针转换）：
  合并/形参/调用结果一律「根未知」⇒ 写操作**扇出到所有根**（`store_to_unknown`），不猜堆根
- ⚠️ **`&&`/`||` 是控制流不是 `Bin`**（`Task 33b`）：`join_into` 的 `lead` 参数承载「表达式值」参数；
  `const_bool` 的类型必须由调用者给（`&&` 的结果可能是 `untyped bool`，`x<3 && y`）
- ⚠️ **`ssa_exec` 的运行期内存模型**：按**真实地址**访存（`Value::Ptr` 里的 `Cell`），
  `root` 只用于静态定序；`RootId` 是函数内下标，跨帧指针靠真实 `Cell`。
  块参数交接必须「先全算边实参、再全写」（`a,b = b,a`）
- ⚠️ **IR 的三处运行期信息**：`Module::global_types` / `SsaFunc::slot_types`（零值建存储）+
  `Instr::Call::dots`（`append(s,xs...)` 的展开 vs 单个）。`Call` 只压帧，`Return` 先全读再弹帧再写
- **语料闭环判据**：`rgoc-driver/tests/ssa_corpus_run.rs` —— 46 个 `N` 侧文件与官方 `.out`
  逐字节比（1.3 s）。这是「SSA 能不能跑真程序」的唯一强证据，别只看手写 fixture
- ⚠️ **pass 的三条硬约束**：① 删指令后必须 `compact_values`（`V1` 要求**每个**值有定义点）
  且同步 `f.params`；② `Effect.prints` 是独立维度（print 在内存效果上是纯的，删了少输出）；
  ③ 摘要的存储归属要看**引用型**槽（指针/切片/map），`s[i]=v` 写的是堆
- **pass 的唯一强判据是 `opt_equiv.rs`**（46 个 N 上 `--opt=all` == `--opt=none` 逐字节）；
  单测只证明「变换发生了」，不证明「没改语义」

## 结构与改动前必读
- **crate 顺序**：`rgoc-lex` → `rgoc-ast` → `rgoc-parse` → `rgoc-const` → `rgoc-types` → `rgoc-load` → `rgoc-check` → `rgoc-hir` → `rgoc-interp` → `rgoc-driver`（`rgoc-hir-spike` / `rgoc-spikes` 仍 SPIKE-ONLY，可整块删）
- **体阶段唯一入口** `rgoc-load/src/scope.rs::resolve_bodies`；`undefined` 唯一报点 `Checker::report_undefined`，按**书写位置**去重
- **声明进作用域唯一入口** `scope.rs::declare_name`（先声明者胜 + 重复报 `TypDuplicateDecl`）；作用域层级逐条照官方 `openScope`（`range` 的层在 range 子句**之后**才开）
- **作用域是竞技场**：`rgoc-types::scope::{ScopeId, ScopeTable, ScopeRef}`；旧 `Scope<'a>` 已不存在
- **多文件必须共享一张 `FileTable`**（`Lexer::new_in` / `parse_file_in`）；类型必须同一张 `TypeTable`
- 装载入口：`load_from_explicit_files` / `load_sources` / `load_with_imports`；**源码 import 四条口径**（`rgoc-load/src/importer.rs`：同名目录只挑 `.go` / canonicalize 做环检测 / 失败 import 每条 spec 都报 / 共用 FileTable+TypeTable）
- HIR（M5）：`Func { params, results: Vec<SlotId>, nslots, slot_types, captured: Vec<Capture>, labels }`；`Expr` 是 `{ kind, typ }`
- 解释器（M5）：`interp.rs` 显式帧栈 + 显式求值栈，`step_*` **不递归**；闭包已提升为独立 `Func`（`Capture{outer_slot, slot}` 共享同一 `Cell`）
- ⚠️ **报点用 `Expr::start_pos()` 而非 `Expr::pos()`**（`Operation.pos` 存的是**运算符**位）；range 操作数走 `version::pos_of_range_operand`
- ⚠️ 标签**两层各报一次**（`SYN-LABEL-*` "defined" + `TYP-*` "declared"）；断言时先滤 `TYP-*`
- ⚠️ `_` 当值/类型是 `TypInvalidBlank`；豁免只在**左值位**（`=`/`:=`/`range`），复合赋值与 `_++` 照报
- ⚠️ 包名诊断位置差一个词：`TypBlankPkgName` 取**包名标识符**，`TypMismatchedPkgName` 取 `package` 关键字
- ⚠️ `stmt.rs::default_type` 只对「基本类型且未类型化」给默认型，其余必须返回**原类型**
- parser（M2）：`prec` 与 `form` 正交（判二元必须 `form == Plain`）；`IfStmt.cond` 是 `Option`；深度守卫挂 `expr`/`stmt_or_nil`/`type_or_nil` 三条边，超限恰好 1 条 `SYN-NESTING-TOO-DEEP`

## 环境硬事实
- 构建/测试全在容器：`RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && …'`（cwd 必须 `/work/rgoc`；探不到 daemon 即报错不降级）
- 四条门禁：`fmt --check` / `check --workspace --all-targets` / `clippy --all-targets -D warnings` / `test --workspace --no-fail-fast`
- target 卷属主 root：`docker run --rm -v <vol>:/x alpine chown -R 501:20 /x`（uid **501**）；`cargo clean` EBUSY ⇒ `find rgoc/target -mindepth 1 -delete`
- ⚠️ **产物陈旧陷阱**：绑定挂载上 cargo 的 mtime 判据不可靠，`touch` + 看 `Compiling` **都不够**。**变异前 `cargo clean -p <crate>`**；可疑就全量重建（`mutation-m3-notes.md` §0）
- ⚠️ 容器 `/tmp` 调用即焚：变异与恢复必须同一调用内完成；变异 runner 走 in-container.sh（宿主 subprocess 静默空输出）；变异必须可编译
- ⚠️ **别拿 `| tail` 的尾巴当「全绿」证据**：`--no-fail-fast` 且遍历**所有** failures 区块；汇总计数要自己算（`awk` 按 `passed;`/`failed;`/`ignored;` 取数）
- ⚠️ /Volumes/MacBookData mtime 整秒 + 容器缓存：写盘后容器内 md5sum 轮询一致再跑；查代码一律 Grep/Read
- zsh 吃 `${!arr[@]}`；容器 `go build` 要 `-buildvcs=false`；脚本首行 `set -euo pipefail`；oracle `go run .` 在其目录内跑

## 纪律
- **TDD** RED→GREEN→REFACTOR；先确认失败来自缺行为而非环境；断言必须反向验证真能失败（已抓 6+ 恒真断言）
- 断言锚定**真实字段**；不用 `None` 冒充未实现（用 `todo!`）；写不出判据就删
- 抄 Go「遍历+原地压缩」循环要保留 range 快照（显式 clone），否则死循环 OOM；抄官方判定函数**兜底档**回源逐字核对
- 生成代码的数据用脚本算别眼数；建模验收对照官方**逐字段 diff**
- 完成度按**四维**跟踪；只读 `go_source_code/`
- clippy 炸点：`collapsible_if` → let-chain；`large_enum_variant` → 先 NonZero 实测；`doc_lazy_continuation` → 改措辞

## 关键设计硬事实
- **`DiagCode` 五件套**：变体按字典序 + `ALL` + `as_str` + 计数测试；现 **103** 码。**`C8` 的 26 类冻结，`other` 值域被 `T-CK-16` 钉死为 3 个实现限制码** ⇒ M6 的 `SSA-UNSUPPORTED-*` **不进 `DiagCode`**（走 crate-local 枚举，沿 `rgoc-hir::RejectKind` 先例）
- **`is_exported` 用生成的 `is_upper`（类别 `Lu`）**，绝不用 Rust `char::is_uppercase`（多 120 码点，`Ⅰx` 会误判导出）；`unicode_tables.rs` 是生成物，永不手改
- **标签**独立一趟（`rgoc-load/src/labels.rs`），`all` 是**体**级；改 `check_labels` 调用点**两处都要改**
- ⚠️ 变异 GREEN 先怀疑**用例集**（M3 两次踩：只测 `if` 体 / 只有前向 `goto`）
- M3 遗留（M4 承继）：`lib.T` 跨包解析 / import 路径不转义解码 / `UndeclaredImportedName` / `//go:` 未知指令归 noder

## 历史收口要点（细节在各自 report，别在记忆里复述）
- **M4**（`M4-report.md`）：`CH-2` **fail** = 该拒的没拒（入选 75 只 17 绿，真实内容对齐 **9**）；**弱绿只证明「不乱报」，不证明「查得出」**；写对账断言时会被自己的断言抓到「空话证据」（`positive_evidence` 必须可追）；**合成反例是唯一能证明断言有牙的东西**；文档里的命令也可能是错的（`R-9-1`）
- **M5**（`M5-report.md`）：150 suite / **1608 passed**；语料 **69/69** 逐字节；Task 1–52 = **51 ✅ / 1 🟡**（泛型边界）；`open_gaps` 只剩 `G-M5-01..06`；
  ⚠️ **`manifest.measured` 是自报不是证据** —— 收口实跑才发现 `cargo fmt` 本来是红的（`cargo test` 全绿**不覆盖** fmt）。收口必须自己跑四条
- **M5 的四条硬事实**：`divmod.go` 是**超限**不是慢（`--max-steps 1e9` 跑 98 s 仍不够）；`convert4.go`/`literal.go` 是 `C9` 的**已知边界**（`Ptr` 不表达子区间 / `Rc<str>` 不是字节串）；块参数/并行拷贝与「取址分析漏判」是 M6 最隐蔽的两类错

## 文档索引
- `AGENTS.md`（入口）/ `docs/README.md`（索引）/ `docs/03-roadmap.md`（M0–M12）/ `docs/01-feature-set.md`
- 契约 **C1–C9**（C8 检查诊断 / C9 值与输出）；**C10（SSA dump）M6/Task 1 新建**
- manifest `M0–M5` 已填实 + `M6` 骨架；`M0-benchmarks.md` §6 环境陷阱；`GAP-01.md` 四维归属（已补 M4/M5 节，M6 待补）
- 台账：`M1..M4-plan/progress.md`（历史，M4 含侦察脚本 `t37-survey.sh` / `t44-okstrength.sh`）+ `M5-plan/progress.md` + `M6-plan/progress.md`（执行期新建）
