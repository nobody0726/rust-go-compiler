# 项目长期记忆 —— rust_go_compiler (`rgoc`)

> 只保「每轮都要知道」的硬事实。细节去 `AGENTS.md`（工程入口，任务先读）、`docs/`、
> `.superpowers/sdd/M4-plan/progress.md`（当前台账）、`docs/milestones/M{3,4}-manifest.json`。

## 概况
- Rust 重写 Go 编译器 `rgoc`；origin = github.com/nobody0726/rust-go-compiler（main）；`go_source_code/` 只读不入库；首发 Linux/arm64（Docker）→ aarch64 ELF；规格基准 go1.27.1
- git 身份 `nobody0726` / `102368194+nobody0726@users.noreply.github.com`；见到 `wangf <yhome@…local>` 即丢失，`git config --global` 重设
- commit 纪律（2026-10-15 授权）：阶段收口即 commit + push（按交付块分 commit）；push 被拦走 `gh api`；`.superpowers/` 已 gitignore；`tools/ast-oracle` vendor 入库

## 当前状态（2026-10-20）
- **M0–M3 全部完成**（E1–E10 / L1–L7 / A1–A8 / TY-1–8，三份 manifest 填实；`manifest_m3.rs` 做文档对账）
- **M4 执行中：Task 1–26 / 44 已落地**（提交链见台账 §提交链，已到 `45ffd62`）。Phase 0 装置（1–5）/
  Phase 1 判定五件套（6–11）/ Phase 2 泛型三件 + 版本门控（12–19）/ Phase 3 检查器主体（20–26：会话+operand+render、
  `expr.rs` 运算符/转换/索引切片/选择器、`stmt.rs` 语句面 + `env.rs` 检查面作用域栈 + `range` 形态矩阵）全部完成；
  **下一步 Task 27 复合字面量（`T-CK-37`）**
- M4 的完成口径 = 用户裁定 `docs/03-roadmap.md` §M4 九大项全量（不是 `AGENTS.md` §7.1 的三样子集）；
  设计 `M4-design.md`（D-M4-1..15 / F1–F18）、测试 `M4-tests.md`（63 个 `T-CK-*`，26 类枚举）、计划 `M4-plan.md` 均已冻结
- `M4-manifest.json` 是骨架：`gate` CH-1..9 与全部 `verdict` **仍 pending**，Task 42 才填实（不要误读成失败）
- 里程碑收口的四要素：代码 + 台账 + manifest 对账测试 + 报告；把 `U-M3-xx` 翻成「做」时**必须同步改 manifest 与 `M3-tests.md`**，否则 `manifest_m3.rs` 红（设计意图）

## 结构与改动前必读
- **crate 顺序**：`rgoc-lex` → `rgoc-ast` → `rgoc-parse` → `rgoc-const` → `rgoc-types` → `rgoc-load` → `rgoc-check` → `rgoc-driver`（`rgoc-hir` / `rgoc-spikes` 仍 SPIKE-ONLY）
- **体阶段唯一入口** `rgoc-load/src/scope.rs::resolve_bodies`（`load_sources` 第 ⑦ 步，`collect_decls` 之后），
  内部 `Checker::for_bodies` 要求包级状态机（`items`/`states`/`stack`）全空；`undefined` 唯一报点是
  `Checker::report_undefined`，按**书写位置**去重（前端把 `x, y int` 克隆两份 Field）
- **声明进作用域唯一入口** `scope.rs::declare_name`：先声明者胜 + 重复报 `TypDuplicateDecl`（包级 "in this package" / 其余 "in this block"）；包级 declare 与 import 名都走它；作用域层级逐条照官方 `openScope`（`range` 的层在 range 子句**之后**才开）
- **作用域是竞技场**：`rgoc-types::scope::{ScopeId, ScopeTable, ScopeRef}`；`declare_universe(tt,&mut st)->ScopeId`；旧 `Scope<'a>`/`Scope::root()` 已不存在
- **多文件必须共享一张 `FileTable`**：`Lexer::new_in` / `parse_file_in`；用 `Lexer::new`/`parse_file` ⇒ 每文件都 `FileId(0)`，诊断排序第一键失效
- 装载入口：`load_from_explicit_files(tt, &[PathBuf], lang)` / `load_sources(tt, &[(name,bytes)], lang)` / `load_with_imports(tt, entry, roots, lang)`；`Package` 不含类型表
- **源码 import 四条口径**（`rgoc-load/src/importer.rs`）：① 只在 `roots` 下按 import 路径找同名目录、只挑 `.go`（排除 `_test.go` 与 `_`/`.` 开头）⇒ 其余 `TYP-UNSUPPORTED-IMPORT-FORM`；目录空 ⇒ `TYP-BROKEN-IMPORT`；环 ⇒ `TYP-IMPORT-CYCLE` ② 环检测/缓存键是 **canonicalize 后的目录**（否则认不出「入口包在环上」）；环上每包一条，每包只装一次 ③ 失败 import 每条 spec 都报，成功包诊断不重复 ④ 会话共用一张 `FileTable`，类型必须同一张 `TypeTable`（改这四处要一起看）
- M4 检查器：类别投影 `rgoc-check/src/diag.rs`（26 类，穷尽 match）；`expr.rs` 已覆盖运算符/转换/索引切片/选择器；`lookup.rs` = 官方 `lookupFieldOrMethod` 逐深度 BFS；`selection.rs` 方法值去接收者/方法表达式接收者作首参
- **`range` 面（Task 26）**：`stmt.rs::range_clause` 四段（求值 → `range_key_val` → 迭代变量个数 → 声明/赋值）；`RangeKV` **三态**（`Ok`/`Gated(Feat)`/`Bad`）—— 官方把 `requires go1.N` 拼进同一消息，而 C8 适配表 `^requires go1\.` 最高优先 ⇒ 门控落 `version` 类、拒绝落 `range` 类，**绝不能压成一个 `Option`**
- ⚠️ **报点用 `Expr::start_pos()` 而非 `Expr::pos()`**：本仓 `Operation.pos` 存的是**运算符**位（`1 << 63` 会指到 `<<`）；range 操作数统一走 `version::pos_of_range_operand`
- ⚠️ `stmt.rs::default_type` 只对「基本类型且未类型化」给默认型，其余必须返回**原类型**（早期用 `?` 吞掉非基本类型 ⇒ 方法表达式 `m1 := T.M` 登记成类型未知、`for range m1` 静默漏报）

## 环境硬事实
- 构建/测试全在容器：`RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && …'`（cwd 必须 `/work/rgoc`；探不到 daemon 即报错不降级）
- 四条门禁：`fmt --check` / `check --workspace --all-targets` / `clippy --all-targets -D warnings` / `test --workspace --no-fail-fast`
- target 卷属主 root：`docker run --rm -v <vol>:/x alpine chown -R 501:20 /x`（uid **501**）；`cargo clean` EBUSY ⇒ `find rgoc/target -mindepth 1 -delete`
- ⚠️ **产物陈旧陷阱（M3 两次踩）**：绑定挂载上 cargo 的 mtime 判据不可靠，`touch`/`os.utime` + 看 `Compiling` **都不够**；
  症状 = 每个变异报同一组失败 / 复原后仍红。**每次变异前 `cargo clean -p <crate>`**；结果可疑就全量重建。详见 `scripts/mutation-m3-notes.md` §0/§0.1
- ⚠️ 容器 `/tmp` 调用即焚：变异与恢复必须同一调用内完成；变异 runner 走 in-container.sh（宿主 subprocess 静默空输出）；变异必须可编译
- ⚠️ **别拿 `| tail` 的尾巴当「全绿」证据**：断言数/通过数逐行看到；`--no-fail-fast` 且遍历**所有** failures 区块
- ⚠️ /Volumes/MacBookData mtime 整秒 + 容器缓存：写盘后容器内 md5sum 轮询一致再跑；查代码一律 Grep/Read
- zsh 吃 `${!arr[@]}`；容器 `go build` 要 `-buildvcs=false`；脚本首行 `set -euo pipefail`；ast-oracle `go run .` 在 `tools/ast-oracle/` 内跑

## 纪律
- **TDD** RED→GREEN→REFACTOR；先确认失败来自缺行为而非环境；断言必须反向验证真能失败（已抓 6+ 恒真断言）
- 断言锚定**真实字段**、作用于 `code_only()`；不用 `None` 冒充未实现（用 `todo!`）；写不出判据就删
- 抄 Go「遍历+原地压缩」循环要保留 range 快照（显式 clone），否则死循环 OOM；抄官方三档判定函数**兜底档**回源逐字核对
- 生成代码的数据用脚本算别眼数；建模验收对照官方**逐字段 diff**（可空性/零值/类型）
- 完成度按**四维**跟踪；只读 `go_source_code/`
- clippy 炸点：`collapsible_if` → let-chain；`large_enum_variant` → 先 NonZero 实测；`doc_lazy_continuation` → 改措辞

## 关键设计硬事实（细节在 ledger / 契约）
- **DiagCode 五件套**：变体按字典序声明 + ALL + `as_str` + 两个总数测试。现 **103** 码（LEX 23 + SYN 12 + TYP 42 + CHK 26；M4 收口实测）；代号**一律照官方实名**核 `src/internal/types/errors/codes.go` 与 `x/tools/.../errorcode.go`（计划稿点名的 `UndefinedIdent`/`RedeclaredInThisBlock`/`ImportNotFound` 官方**不存在**）。M4 每加一码要让 lex/parse/check 三处计数断言 + `category.rs` 快照同步 + `C8`/`M4-tests` 的 `R-4-N` 修订行
- **`is_exported` 用生成的 `is_upper`（类别 `Lu`）**，绝不用 Rust `char::is_uppercase`（derived `Uppercase` 比 Go `unicode.IsUpper` 多 120 码点，5 段）⇒ `Ⅰx` 会误判导出。`unicode_tables.rs` 是生成物（`tools/token-oracle/gen_unicode`），永不手改
- **标签**：独立一趟（`rgoc-load/src/labels.rs`），`all` 是**体**级集合、每个函数体各跑一次（含函数字面量）⇒ 跨函数不可见；标签不进值作用域；`goto` 可前向 ⇒ 前向跳转表 + 收尾判 `UndeclaredLabel`/`JumpIntoBlock`。改 `check_labels` 调用点**两处都要改**
- ⚠️ 标签**两层各报一次**：语法器 `checkBranches`（`SYN-LABEL-*` "defined"）+ 类型检查器（`TYP-*` "declared"）；断言时先滤 `TYP-*`
- ⚠️ `_` 当值/类型是 `TypInvalidBlank`，不是 `undefined: _`；豁免只在**左值位**（纯 `=`/`:=`/`range`），复合赋值与 `_++` 照报
- ⚠️ 包名诊断位置差一个词：`TypBlankPkgName` 取**包名标识符**，`TypMismatchedPkgName` 取 `package` 关键字；`BlankPkgName` 只在 `pkg.name` 还空时触发
- ⚠️ 变异 GREEN 先怀疑**用例集**（M3 两次：只测 `if` 体抓不到块不开层；只有前向 `goto`）
- parser（M2）要点：`prec` 与 `form` 正交（判二元必须 `form == Plain`，`prec` 不进 followset）；节点位置取 `Expr::pos()`；`IfStmt.cond` 是 `Option`；深度守卫挂 `expr`/`stmt_or_nil`/`type_or_nil` 三条边，超限恰好 1 条 `SYN-NESTING-TOO-DEEP`
- M3 遗留缺口（`M4 open_gaps` 承继）：import 名取路径最后一段 / 跨包 `lib.T` 解析（M4 两阶段）/ import 路径不转义解码 / `UndeclaredImportedName` 未做 / `//go:` 未知指令裁定归 noder 阶段

## M4 收口（2026-10-22）—— **部分兑现**，别当完成读

- **门禁**：`CH-1/4/5/6/7/9` `pass`；**`CH-2` `fail`（入选 75 仅 17 绿）**、**`CH-8` `fail`（`--check` 形态红）**、`CH-3` `partial`（语料 6 文件 0 行）。`phase_summary.status` 保持 `partial`。
- **数字要成对记**：入选 75 = **17 绿**，其中**真实内容对齐只有 9**（另 8 个是「双侧零诊断」弱绿）；wave A 50 = **20 绿**，真实对齐**只有 3**。**报数只报 17/20 就是自欺** —— 弱绿只证明「不乱报」，不证明「查得出」。
- **58 个红的形状**（精确）：假阴性 15 / 假阳性 3 / 过量 21 / 不足 19。过量与不足**同时存在** ⇒ 不是整体偏多或偏少，是**规则成片缺失 + 报点口径不一**。
- **`A3` 结论是「0 例可提升」，但那是进步**：根因从「没有实例化」挪到了「**dump 根集不产实例**」（`var _ T[int]` 现场：oracle 5 条 / rgoc 4 条，差的那条是实例 `named`）。下一个接手的人不用再猜。
- **纪律教训（本次新增，可复用）**：
  1. **对账断言不能照抄上一个阶段**。M3 收口时门禁全绿，`manifest_m3.rs` 可以断言 `status == "pass"`；M4 有 `fail`/`partial`，照抄只剩「把 manifest 写成假的」或「把断言删了」两条路。正确解法是换成**可证伪的一致性**：`pass` 而证据自认「未兑现」⇒ 红；非 `pass` 必须在 `open_gaps` 点名；`done` 当且仅当全 `pass`。
  2. **写断言时会被自己的断言抓到「空话证据」**：`positive_evidence` 写「同上」「白名单项不得记为 pass」这类没有落点的话，一律补成可追指针（`docs/…md §x` / `T-CK-xx`）。这一步别省 —— 它抓出的都是真的空话。
  3. **不可达断言**：初版「pass 门禁名下的非 pass 测试项必须被 open_gaps 点名」与另一条**逻辑等价**，永远不触发；合成反例的「竟然通过了」当场抓住。合成反例不是形式，是唯一能证明断言有牙的东西。
  4. **文档里的命令也可能是错的**：`M4-tests.md` §9.1 的裸形态 `scripts/in-container.sh cargo …` 从来跑不通（容器 `-w /work`，workspace 在 `rgoc/`）⇒ 走 `R-9-1` 修订，**判据不变、只改调用形态**。跑门禁前先确认「失败是缺行为还是调用形态错」。
- **新增遗留**：`G-M4-37`（58 红收口）、`G-M4-38`（wave A 30 红）、`G-M4-39`（语法/词法级门控跨 crate）、`G-M4-40`（`--check` 形态红）、`G-M4-41`（`rgoc types --dump` 头里 `# source: rgoc` **重复一次**，M3 遗留；`C7` 规定 `#` 行被忽略 ⇒ 任何门禁都抓不到）。
- **M5 的下手顺序**（按性价比）：赋值/转换矩阵 → 类型参数核心类型 → 递归接口同一性（`cycles1` 有 12 条**假阳性**会污染别处）→ 泛型判定面 → 语法级门控（跨 crate，风险最高）。

## M5 执行（2026-10-23）—— 代码基本完成，**交付物与 commit 有缺口**

- **代码侧实测（本人复跑，不是读文档）**：`cargo test --workspace` **150 suite / 1608 passed / 0 failed / 1 ignored**；
  四条 cargo 全 exit 0；五条自检（m0 232 / m1 / ast-golden / ast-testdata / ast-stdlib）全过；
  `run-collide.sh --run --list run-selected.txt` = **69/69 绿**；`rgoc-driver run test/helloworld.go` 打印 `hello, world` exit 0。
  新代码：`rgoc-hir`（lower 2891 行）+ `rgoc-interp`（interp 3531 行）+ driver `run_cmd` + 4 个 M5 集成测试（约 3500 行）。
- **门禁**：manifest 自报 `EX-3/5/6/7/9 pass`、`EX-1/2/4/8 partial` ⇒ `status = partial`，`open_gaps = G-M5-01..10`（诚实）
- **2026-10-23 收口补做（本人）**：① 把 Task 7–52 的 12.4k 行未提交产出切成 **5 个 commit**（`8a2df91` hir+interp / `5e9ac28` driver run / `b01c146` 四腿测试 / `9361dc7` run-oracle / `51496a4` manifest+变异记录）；
  ② 补写 **`docs/contracts/C9-values-and-output.md`**（从实现逐条核对，7 条不变量 + 字节表 + 退出码 + 资源上限 + 三件不承诺）与 **`docs/milestones/M5-report.md`**（交付 B：覆盖矩阵 / 三条分别声明 / 偏离登记 / 缺口 / 接手指南）；
  ③ 重建 **`.superpowers/sdd/M5-plan/progress.md`**（逐任务 51✅/1🟡/0❌ + 8 条 Ruling + 接手须知）；④ README/plan 状态回写、manifest 数字对齐 1608/150、新增 `G-M5-07..10`。
- **2026-10-23 二次收口（本人）**：`G-M5-07..10` **全部关闭**，记在 manifest 的 `closed_gaps`：
  `G-M5-10` 补做 **P0 四条变异**（`T-EX-57..60` 各改回原状 → 实证变红 → 复原，4/4；`mutation-m5-notes.md` 新增一节）；
  `G-M5-07` `C3` **升正式**（§2 四项改成「已定」：`spread`/`Place` 五形态/`Closure.captured`/登记时求值）；
  `G-M5-08` `GAP-01` 新增 **M5 节**（`PIPE-10/14/15/16/16c/24` + `RT-CMP-01/02/03/04/07` + `RT-MEM/SCH/POLY` 的「不涉及」）；
  `G-M5-09` `03-roadmap` 与 `AGENTS.md` 状态回写（crate 7→10、测试 1282→1608、`rgoc-hir-spike`）。
  ⇒ **Task 1–52 = 51 ✅ / 1 🟡（Task 41 泛型边界，内容缺口）/ 0 ❌**；`open_gaps` 只剩 `G-M5-01..06`。
- `M5-plan.md` 的 149 个 `- [ ]` **从未维护**，已在头部声明「真实状态看 progress.md / M5-report.md」，别把空框读成没做。
- ⚠️ **`manifest.measured` 是自报，不是证据**：收口期实跑才发现 `cargo fmt --all --check` 本来是**红的**（`manifest_m5.rs` 从未格式化，8 处 diff），而 manifest 写着「fmt exit 0」。**`cargo test` 全绿不覆盖 fmt**。已修（`f203b61`）并记进 `M5-report.md` §5 第 7 条。收口必须自己跑四条。

## 文档索引
- `AGENTS.md`（入口）/ `docs/README.md`（索引）/ `docs/03-roadmap.md`（M0–M12）/ `docs/01-feature-set.md`
- 契约 C1–C8（C8 = 检查诊断，M4）；manifest `M0–M4`；`M0-benchmarks.md` §6 环境陷阱；`GAP-01.md` 四维归属
- 台账：`M1-plan/` `M2-plan/` `M3-plan/progress.md`（历史）+ `gates.sh`；`M4-plan/progress.md`（M4 已收口，含 Task 37/38 侦察脚本 `t37-survey.sh` / `t44-okstrength.sh` —— **复用它们做逐文件 diff**）；**下一阶段** M5 开工前先读 `docs/milestones/M4-report.md` §4.3 + §12
