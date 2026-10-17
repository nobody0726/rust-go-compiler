# 项目长期记忆 —— rust_go_compiler (`rgoc`)

> 只保「每轮都要知道」的硬事实；细节去 `AGENTS.md`（工程入口，任何任务先读）、`docs/`、
> `.superpowers/sdd/*/progress.md`、`docs/milestones/M3-manifest.json`（open_gaps / unsupported）。

## 概况
- Rust 重写 Go 编译器 `rgoc`；origin = github.com/nobody0726/rust-go-compiler（main）；语料 `go_source_code/` 只读不入库；首发 Linux/arm64（Docker）→ aarch64 ELF
- git 身份 `nobody0726` / `102368194+nobody0726@users.noreply.github.com`；见到 `wangf <yhome@…local>` 即丢失，`git config --global` 重设
- **commit 纪律（2026-10-15 授权）**：阶段收口即可 commit + push（按交付块分 commit）；push 被拦走 `gh api` 建链；`.superpowers/` gitignore；`tools/ast-oracle` vendor 入库

## 状态
- **M0–M3 四个里程碑全部完成**：M1 = T-L01..21（**L1–L7**，151 条 golden）；M2 = T-AP01..21（**A1–A8**，oracle 对撞 31/31、stdlib 6670 文件零错、深度上限 1000）；M3 = T-TY-01..38（**TY-1–TY-8**，对撞 dump 8 / cmp 2 / 语料 31 三条腿逐字节一致、7 条变异全红、`M3-manifest.json` 已填实并由 `rgoc-driver/tests/manifest_m3.rs` 与冻结文档对账）。workspace **1282 passed / 0 failed**
- **M4（泛型实例化 / 类型集版 interface 同一性 / 可赋值性）尚未开工**：开工前读 `M3-manifest.json` 的 `open_gaps` + `unsupported`（`U-M3-05` 是 M4 **必须**解除的那条）与 `M3-tests.md` §6。若把某条 `U-M3-xx` 翻成「做」，**必须同步改 manifest 与 `M3-tests.md`**，否则 `manifest_m3.rs` 会红（设计意图）
- **M3 的执行台账**（历史用）：`.superpowers/sdd/M3-plan/progress.md`（**改代码前先读它**），门禁唯一入口 =
  `.superpowers/sdd/M3-plan/gates.sh`（容器内 `bash /work/.superpowers/sdd/M3-plan/gates.sh`）。
  三 crate = `rgoc-const` / `rgoc-types` / `rgoc-load` + `tools/types-oracle`。
- **M3 体阶段的两条结构事实（Task 25 落地，改动前必读）**：
  ① **体阶段唯一入口** `rgoc-load/src/scope.rs::resolve_bodies`（`load_sources` 末尾第 ⑦ 步，
  **在 `collect_decls` 之后**），内部用 `Checker::for_bodies` 造 `Checker` —— 包级状态机
  （`items`/`states`/`stack`）**必须全空**；`Checker::undefined_reported: BTreeSet<Pos>` +
  `report_undefined` 是 `undefined` 的**唯一报点**（按**书写位置**去重：前端把 `x, y int`
  克隆成两份 `Field`，不去重会报两条而官方只报一条）。
  ② **声明进作用域只有一个入口**：`scope.rs::declare_name(scopes, scope, kind, name, pos, typ, bag)`
  —— 先声明者胜 + 重复报 `TypDuplicateDecl`（措辞按作用域层：包级 “in this package”、
  其余 “in this block”）；包级 `declare` 与 **import 名**都走它。作用域层级逐条照官方
  `openScope`（`if`/`for`/**`range`**/`switch`/**每个 case**/`select` 子句各一层；
  `range` 的作用域在 **range 子句之后**才开）。
- **M3 包模型的三条结构事实（Task 23 落地，改动前必读）**：
  ① 作用域是**竞技场**（`rgoc-types::scope::{ScopeId, ScopeTable, ScopeRef}`；节点 `Scope` 只做本层，
  沿链 `lookup`/`lookup_level`/`depth` 在 `ScopeTable`/`ScopeRef`）；`declare_universe(tt, &mut st) -> ScopeId`，
  `universe() -> (ScopeTable, ScopeId, TypeTable)` ⇒ 旧 `Scope<'a>`/`Scope::root()` 已不存在（Ruling 23-1）。
  ② **多文件必须共享一张 `FileTable`**：`Lexer::new_in(&mut FileTable, …)` / `parse_file_in(&mut FileTable, …)`；
  `Lexer::new`/`parse_file` 自带新表 ⇒ 每文件都是 `FileId(0)`（诊断排序第一键失效，Ruling 23-4）。
  ③ 装载入口 `load_from_explicit_files(tt, &[PathBuf], lang) -> (Package, DiagBag)`（**多一个 `tt` 形参**，Ruling 23-2）
  与 `load_sources(tt, &[(name, bytes)], lang)`；`Package { name, files, file_table, asts, scopes, scope, skipped }`，
  类型表**不**在 `Package` 里（universe 要声明进 dump 那张表）；顶层声明名先**占位**（`typ/val = None`），求值归 Task 24。
- **平行历史已合流**（2026-10-17）：`origin/main`(43843d6) merge 进本地 main。
  两侧对 `M1-plan`/`M2-plan` 的补勾**逐字节相同** ⇒ 零冲突上代码；只有
  `.workbuddy/memory/*` 两处冲突，一律取**本地侧**（M3 记录更靠后）。
  **不得 force push**（远端 43843d6 是已推送的历史，必须留在祖先里）。
  `2026-10-16.md` 记了这段分叉的来龙去脉
- 遗留：`//go:` 未知指令的**裁定**归 noder 等价阶段（M3 按「收下不报错」实现，`U-M3-09`）；`-lang` 按文件版本门控已由 M3 的 Task 21 落地（**特性面**仍未做，M2 `open_gaps` 承继）
- ⚠️ **产物陈旧陷阱（M3 两次踩，务必记）**：绑定挂载上 cargo 的 mtime 判据不可靠 ⇒ 变异/复原后可能拿到**旧 rlib**（一面是假绿、一面是假红：复原后 `rgoc-parse` 里还编着 M84 变异 ⇒ `test_directive.rs` 8 条假红）。变异前 `cargo clean -p <crate>`；收尾后若结果可疑，`find rgoc/target -mindepth 1 -delete` 全量重建再跑门禁（`cargo clean` 会 EBUSY）。详见 `scripts/mutation-m3-notes.md` §0 / §0.1

## 环境硬事实
- 构建/测试全在容器：`RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && …'`
  （cwd 必须 `/work/rgoc`；探不到 daemon 即报错**不降级**）
- 四条门禁：fmt --check / check --workspace --all-targets / clippy --all-targets -D warnings /
  test --workspace --no-fail-fast
- target 卷属主 root → `docker run --rm -v <vol>:/x alpine chown -R 501:20 /x`（**uid 501**）；
  `cargo clean` EBUSY → `find rgoc/target -mindepth 1 -delete`
- ⚠️ /Volumes/MacBookData mtime 整秒 + 容器缓存 ⇒ 写盘后容器内 md5sum 轮询一致才跑测试；查代码一律 Grep/Read 工具
- ⚠️ cargo 陈旧产物：同一次容器调用里 touch **被改文件本身** + 核验 `Compiling`（check/clippy 是 `Checking`）；「复原后还红」= 陈旧二进制污染
- ⚠️⚠️ **`os.utime` + `Compiling` 守卫不足**（2026-10-17 Task 23 实测）：写盘/还原的时间戳把 cargo 指纹搅乱后，后续 run 会**静默复用上一个变异的产物**（症状 = 每个变异都报同一组失败，且复原后仍红）。**唯一可信的做法是每次变异前 `cargo clean -p <crate>`**（连带强制依赖方重编）；被污染后要 `cargo clean -p` 那一组 crate 才能恢复
- ⚠️ 容器 `/tmp` 调用即焚；**变异与恢复必须同一调用内完成**；变异 runner 走 in-container.sh（宿主 subprocess 静默空输出）；复原覆盖本组每个文件；变异必须可编译
- ⚠️ **别拿 `| tail` 的尾巴当「全绿」证据**（Task 23 踩过）：断言数、通过数必须逐行看到；truncated 输出里没出现的测试等于没跑
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
- **DiagCode 五件套**：变体声明序=字典序（derive(Ord) 按声明序不是 ALL 序）+ ALL + as_str + 两个总数测试；现 **66** 个（M1 的 23 `LEX-` + M2 的 12 `SYN-` + M3 的 31 `TYP-`；Task 23 增 `TypMismatchedPkgName`；Task 24 补 4 条 = 官方 `InvalidConstInit` / `InvalidDeclCycle` / `InvalidInitCycle` / `WrongAssignCount`；Task 25 补 3 条 = 官方 `DuplicateDecl` / `UndeclaredName` / `RepeatedDecl` —— ⚠️ 计划稿点名的 `UndefinedIdent` / `RedeclaredInThisBlock` / `RedeclaredInThisPackage` 官方**不存在**；Task 26 补 7 条 = `BlankPkgName` / `DuplicateLabel` / `InvalidBlank` / `JumpIntoBlock` / `MisplacedLabel` / `UndeclaredLabel` / `UnusedLabel`；Task 27 补 3 条 = `TypBrokenImport`（**官方实名**，替代计划稿点名的 `ImportNotFound`）/ `TypImportCycle` / `TypUnsupportedImportForm`（后两条官方**无**对应物：官方 `go/types` 不做源码 import，环在 importer 侧只有纯文本）；**代号一律照官方实名核过两份表**：`src/internal/types/errors/codes.go` + `x/tools/.../typesinternal/errorcode.go`）
- 深度守卫挂**三条递归边**：`expr` / `stmt_or_nil` / `type_or_nil`；超限**恰好 1 条** SYN-NESTING-TOO-DEEP

## M3 硬事实（细节在 `.superpowers/sdd/M3-plan/progress.md` / commits）
- **标签是独立一趟**（`rgoc-load/src/labels.rs`，照官方 `go/types/labels.go`）：`all` 是**体**级标签集，**每个函数体各跑一次**（含函数字面量）⇒ 跨函数不可见；标签**不进值作用域**（与变量同名不冲突）；`goto` 可**前向**引用 ⇒ 前向跳转表 + 收尾判 `UndeclaredLabel` / `JumpIntoBlock`。改 `check_labels` 的调用点必须两处都改（漏一处整族静默）
- ⚠️ **标签在两层各报一次**：语法器 `checkBranches`（M2）报 `SYN-LABEL-*`（“defined/not defined”），类型检查器报 `TYP-*`（“declared/not declared”）；官方 `gc` 语法层报完就不类型检查，我们两层都跑 ⇒ 用例断言时先滤 `TYP-*`
- ⚠️ **`_` 当值/类型是 `TypInvalidBlank`，不是 `undefined: _`**；豁免只在**左值位**（纯 `=` / `:=` / `range`）——复合赋值 `_ += 1` 与 `_++` 官方走 `check.expr` ⇒ **照报**
- ⚠️ **两条包名诊断的位置差一个词**：`TypBlankPkgName` 取**包名标识符**处，`TypMismatchedPkgName` 取 **`package` 关键字**处；且 `BlankPkgName` 只在 `pkg.name` 还空时触发
- ⚠️ **`is_exported` 用生成的 `is_upper`（类别 `Lu`），绝不用 Rust std `char::is_uppercase`**：std 是 derived `Uppercase`（`Lu` ∪ `Other_Uppercase`），比 Go 的 `unicode.IsUpper` **多 120 个码点**（5 段：U+2160..216F / U+24B6..24CF / U+1F130..1F149 / U+1F150..1F169 / U+1F170..1F189）⇒ 会把 `Ⅰx` 误判成导出。`unicode_tables.rs` 是**生成物**（`tools/token-oracle/gen_unicode`），永不手改
- ⚠️ **变异 GREEN 先怀疑用例集，再怀疑变异**（两次都这样）：Task 25 的 M4（只测 `if` 体抓不到块不开层）、Task 26 的 M12（用例集里只有**前向** `goto`，没有**向后**跳）
- **源码 import（Task 27，`rgoc-load/src/importer.rs`）四条口径**：
  ① 入口 `load_with_imports(tt, entry, roots, lang) -> (Package, DiagBag)` —— **显式路径映射**：
     `ImportResolver` 只在 `roots` 里按 `import` 路径找**同名目录**（第一条命中的根生效）；
     目录里只挑 `.go`，排除 `_test.go` 与 `_`/`.` 开头（照官方 `go/build`）⇒ 标准库/模块路径/相对路径一律 `TYP-UNSUPPORTED-IMPORT-FORM`；
     目录在但没可用 `.go` ⇒ `TYP-BROKEN-IMPORT`（官方实名 `BrokenImport`）；环 ⇒ `TYP-IMPORT-CYCLE`（消息照 `srcimporter.go:85` 原话）
  ② **环检测/缓存的键是「解析后的目录」**（`fs::canonicalize`）而不是 import 路径 —— 否则「环上包就是入口包」这条形状认不出来；
     环「**环上每个包一条**」：位置取「把该包拉进环的那条 import」，入口包借用**闭合边**的位置；每个包只装一次（缓存）⇒ 环只报一次
  ③ **失败的 import 每条 spec 都报**（官方 `check.errorf(at, BrokenImport, …)` 用 spec 位置）；**成功**的包只装一次（诊断不重复）
  ④ 会话**共用一张 `FileTable`**（`load_sources_into`，末尾回填给每个包）⇒ 被依赖包里的诊断位置也能用入口包那张表渲染；`Package::imports: BTreeMap<String, Package>`，
     类型**必须**同一张 `TypeTable`（跨包同一性）；改这四处要一起看（`Scope`/`ScopeTable` 的 `Clone` 就是被 ②/④ 逼出来的）
- **Task 27 的四个已知缺口（Task 38 进 `open_gaps`）**：import 名取**路径最后一段**（不是被依赖包 `package` 子句的真名）/
  **跨包引用 `lib.T` 解析不了**（装载顺序是「先求值本包、再解析 import」⇒ M4 改两阶段）/ `import` 路径不做转义解码（只剥引号）/
  `UndeclaredImportedName`（选择子）M3 不做（无触发者、`M3-tests.md` 没要求）
- **manifest 与冻结文档的「对账」是一个可执行断言**（M3/Task 38 新建的**落点**，
  M1/M2 的 manifest 此前没有任何脚本引用它）：`rgoc/crates/rgoc-driver/tests/manifest_m3.rs`
  —— ① `test_ids` 集合 == `M3-tests.md` §3 表**第一列**（68）；② `unsupported` 集合 == §6（13）；
  ③ `gate` 键**恰好** `TY-1..8` 且全 `pass`、证据带指针；④ `budget` == §7.4 冻结值；
  ⑤ `coverage` 计数器自洽；⑥ §6 纪律：拒绝侧那几条必须有 `T-TY-8x` 正向指针。
  **五条例外**单列成判据表（`U-M3-06/10/11/12/13`：约定路径 / 不承诺逐字 / M2 承继未触发）——
  硬要 8x 会逼人写假测试。另有 **15 条合成反例**（13 改 manifest + 2 改**文档**）证明它真在读文档
- **对撞夹具不标注测试 ID**（收口审计：68 个 ID 里 17 个无字面引用 —— 12 个靠区间注释、
  5 个靠夹具内容）⇒ 「文档 §3 的 ID → 夹具」只能**手查**；`T-TY-3a`/`T-TY-34` 的夹具
  已补（`cmp-representable.go` 末尾 9 条 + `-update` bless 的 `.want`）。机制化见 `open_gaps` 第 14 条
- **语料口径（Task 35 普查，可重放 `probe/corpus-survey.sh`）**：**57 候选** = 入选 **31**
  （`tools/types-oracle/corpus-selected.txt`）/ DIFF **15**（根因同一 ⇒ `U-M3-04` 实例化）/
  两侧都拒绝 4 / oracle 单侧拒绝 6 / 两侧都空 1；排除集逐行理由在 `corpus-allowlist.txt`
  （`<路径>\t<理由>`），**两个清单互斥且合起来恰好覆盖候选**（Rust 侧断言，Ruling 35-3/4）

## 文档索引
- `docs/`：README / 01-feature-set(246) / 03-roadmap(M0–M12)；契约 C1–C7；`M0/M1/M2/M3-manifest.json`；M0 环境陷阱 14 项在 M0-benchmarks.md §6；VSCode/调试复发点与自检脚本用法见 `AGENTS.md`
