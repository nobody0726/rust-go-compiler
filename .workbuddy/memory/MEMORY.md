# 项目长期记忆 —— rust_go_compiler (`rgoc`)

> 只保「每轮都要知道」的硬事实。细节去 `AGENTS.md`（入口，任务先读）、`docs/`、
> `.superpowers/sdd/M*-plan/progress.md`、`docs/milestones/M*-manifest.json` / `M*-report.md`。

## 概况
- Rust 重写 Go 编译器 `rgoc`；origin = github.com/nobody0726/rust-go-compiler（main）；
  `go_source_code/` 只读不入库；首发 Linux/arm64（Docker）→ aarch64 ELF；基准 go1.27.1
- git 身份 `nobody0726` / `102368194+nobody0726@users.noreply.github.com`（见 `wangf <yhome@…local>` 即丢失）
- ⚠️ **`git push` 报 `CONNECT tunnel failed, response 502`** 是**代理**（`http://127.0.0.1:56060`）
  坏了，不是网络断了：`env -u HTTP_PROXY -u HTTPS_PROXY -u http_proxy -u https_proxy git push`
  **直连**就通（`gh api` 也通）。别急着改用 gh api 手工搬对象
- commit 纪律：阶段收口即 commit + push（按交付块分）；`.superpowers/` 已 gitignore（台账不入库）

## 当前状态（2026-10-26）
- **M0–M3 完成；M4/M5 部分兑现；M6–M9 全部收口**（M9 尾巴已清：`open_gaps` 0 / `closed_gaps` 4）
- **M10（精确 STW GC）进行中**：三件套已出并拍板（块头 16 B / 逐位点 `sp_idx` / map 进托管堆）。
  **Phase 0 收口**（Task 1–5）+ **Phase 1 前半**（Task 6–9：chunk 对齐 / 位图 / 16 字节块头 /
  三种托管分配入口）。读数 **229 suite / 2205 passed / 0 failed / 2 ignored**；语料 69/0/0；
  三态各 69/0/0。下一步 = Task 10–13（free list / 标记 / 清扫 / `rt_gc` + 全局根）
- M9 终态读数：native 69/0/0；`--run` 69 绿；`--via=ssa` 69 绿（**拒绝面为空**）；2184 passed
- M6 收口：语料 N=46 / M=23 冻结；66 变异全红

## M10 硬约束（先记住再动手）
- 托管堆与 arena 分离（`rt_arena_alloc` 只留不含堆指针的结构）
- 块头 **16 B** `{desc|flags, n|kind}`；数组/闭包要元素个数
- 根走**帧记录链 + 逐位点**：每帧 24 B `{prev, fi, sp_idx}`，链头 `g_frames`。**函数级并集位图 = 保守扫描**
- **没有寄存器根**（每值一槽的推论）；**不移动**（内部指针无 forwarding）
- 内部指针回块首：chunk 16 MiB 对齐（`p >> 24`）+ 起始位图（1 位/8 B）+ 页摘要（1 位/4 KiB）
- ⚠️ **`rt_alloc` 返回 data（块首 + 8）**（`R-10-1`）：返回块首会让字段 0 压对象头、`need_zero` 漏首字
- 根协议源头：`Safepoint.ptr_words` 来自 `rgoc_types::layout::ptr_mask`（与 `TypeDesc.gcdata` 同函数）；
  回边 safepoint 用 `dom::DomTree` 判「目标支配本块」
- `STACK-MAP v1`：`sp b<block>:<idx> off=<字下标> live=<n> ptrs=<hex>`；bit i ↔ `fp - frame_size + 8i`
- **统计行语法冻结**（`C12` §5.2，12 字段）：`gc: collections= allocs= frees= live= heap= chunks= rss= kinds=<5 个逗号> bad_sp= bad_ptr= bad_desc= mode=<on|off|stress>`。
  全字段**总是出现**（`R-10-2`）；`mode` 是 `R-10-3` 追加的**装置判据**
- ⚠️ **`R-10-4`：DCE 不许删 `Instr::Make`**（`new`/`make` 有分配副作用）。
  修之前 `gc1.go` 的 1e5 次 `new([100]byte)` 在 `--opt=all` 下被全删（`heap=0`）
  ⇒ **stress 态退化成空跑**。CSE 早就正确排除了 `Make`，只有 DCE 漏了
- ⚠️ **`mapslots` 块自带 `{kdesc,vdesc}` 头**（`R-10-1`）：扫它不必回头找 `MapObj`
- ⚠️ **`gcdata`/`ptrdata` 一致性**：`ptrdata > 0 ⇒ gcdata 非 null`。已知违反点
  `g_panic_str_desc`（Task 24 要改）
- ⚠️ **`CHUNK_HDR`(64) ≠ `BLK_HDR`(16)**：`bump` 起点是 **64**。写成 16 会让第一个块
  覆盖自己的 chunk 头（`meta_bm` 变 0）⇒ 第一次分配正常、**第二次**段错误。
  `_Static_assert(sizeof(Chunk) == CHUNK_HDR)` 钉住
- ⚠️ **arena 分配也带 16 字节块头**（kind = `K_ARENA` = 5，永不回收/不扫）：
  不给头的话 sweep 按位图走会把它当空闲塞进 free list，复用就覆盖 itab（`R-10-6`）
- ⚠️ **`array` 的 `n` 用 cap 不是 len**（`R-10-7`）：`s = s[:cap]` 之后的元素可写指针
- ⚠️ **字符串字面量拼接会被编译期折叠**（`g := "a"+"b"` 不调 `rt_strcat`）——
  要测 raw 分配必须放进函数（形参折不了）

## 跨阶段硬事实（会反复踩）
- ⚠️ **「变异绿」两种成因**：① 判据缺形态；② **探针打错靶**（用例集不走那段代码）。先分辨再改
- ⚠️ **「记一笔 open_gap」不等于收口**；⚠️ **`manifest.measured` 是自报不是证据**（fmt 红过而 test 全绿）
- ⚠️ **变异恢复源码后必须 `touch`**：`mv` 保留旧 mtime ⇒ cargo 复用变异版 rlib（单跑绿、全量红）
- ⚠️ **变异恢复不能用 `git checkout`**：未提交的改动会被一起丢掉（Phase 0 踩过一次，
  Task 3 的 `rt.c` 三处编辑全丢）。要么先提交、要么用「原文副本」（M9 的 `.m9bak`）
- ⚠️ **变异前 `cargo clean -p <crate>`**；绑定挂载上 mtime 判据不可靠
- ⚠️ **每次 `in-container.sh` 是新容器**：上一步 `/tmp/x` 下一步就没了（像莫名 rc=127）
- ⚠️ **别拿 `| tail` 当全绿**：`--no-fail-fast` 要遍历所有 failures 区块，计数自己 `awk` 算
- ⚠️ **panic 用返回码传播不 unwind**（`.framemap` 不在 ELF ⇒ `longjmp` 漏扫根）；
  `check_panic` 用 `cbnz` 直接跳（别造跳过标签，多处检查会重定义符号）
- ⚠️ **求值器与后端必须同口径**：方法接收者在 IR 里总是地址（`Field` 只认 `Value::Ptr`）；
  `Instr::Index`/`Field` 的 base 必须是地址、dst 类型必须是 `*成员类型`
- ⚠️ **「类型表达式 → `TypeId`」两份实现**：`rgoc-load::typeexpr::resolve_type` + `rgoc-check::expr::type_expr`，改一处要改两处
- ⚠️ **pass 三约束**：删指令后 `compact_values` + 同步 `f.params`；`Effect.prints` 独立维度；
  `pass` 唯一强判据是 `opt_equiv.rs`（单测只证明变换发生）
- ⚠️ **文档驱动 golden 防契约漂**（`R-51-1`）：从契约文件读期望值，别抄副本
- **里程碑收口四要素**：代码 + 台账 + manifest 对账测试 + 报告

## 结构入口
- crate 序：`lex → ast → parse → const → types → load → check → hir → interp → driver`（`*-spike` 可整块删）
- 体阶段唯一入口 `rgoc-load/src/scope.rs::resolve_bodies`；声明入口 `scope.rs::declare_name`
- 作用域是竞技场 `rgoc-types::scope::{ScopeId, ScopeTable}`；多文件必须共享 `FileTable` / `TypeTable`
- HIR `Func{params, results, nslots, slot_types, captured, labels}`；解释器显式帧栈，`step_*` 不递归
- ⚠️ 报点用 `Expr::start_pos()`（`Operation.pos` 是运算符位）；标签两层各报一次（断言先滤 `TYP-*`）
- parser：`prec` 与 `form` 正交（判二元须 `form == Plain`）；深度守卫超限恰好 1 条 `SYN-NESTING-TOO-DEEP`
- `DiagCode` 五件套（变体字典序 + `ALL` + `as_str` + 计数测试）；`C8` 的 26 类冻结；
  M6 的 `SSA-UNSUPPORTED-*` 走 crate-local 枚举不进 `DiagCode`
- `is_exported` 用生成的 `is_upper`（类别 Lu），绝不用 Rust `char::is_uppercase`；`unicode_tables.rs` 永不手改

## 环境硬事实
- 全在容器跑：`RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && …'`（cwd 必须 `/work/rgoc`）
- 四条门禁：`fmt --check` / `check --workspace --all-targets` / `clippy --all-targets -D warnings` / `test --workspace --no-fail-fast`
- target 卷属主 root：`docker run --rm -v <vol>:/x alpine chown -R 501:20 /x`；
  `cargo clean` EBUSY ⇒ `find rgoc/target -mindepth 1 -delete`
- ⚠️ /Volumes/MacBookData mtime 整秒 + 容器缓存：写盘后容器内 md5sum 轮询一致再跑；查代码用 Grep/Read
- zsh 吃 `${!arr[@]}`；容器 `go build` 要 `-buildvcs=false`；脚本首行 `set -euo pipefail`；oracle `go run .` 在其目录内跑

## 纪律
- **TDD** RED→GREEN→REFACTOR；先确认失败来自缺行为而非环境；断言必须反向验证真能失败
- 断言锚定**真实字段**；不用 `None` 冒充未实现（用 `todo!`）；写不出判据就删
- 抄 Go「遍历+原地压缩」保留 range 快照（显式 clone）；抄官方判定函数**兜底档**回源逐字核对
- 生成数据用脚本算别眼数；建模验收对照官方**逐字段 diff**；完成度按**四维**跟踪；只读 `go_source_code/`
- clippy 炸点：`collapsible_if` → let-chain；`large_enum_variant` → 先 NonZero 实测；`doc_lazy_continuation` → 改措辞

## 文档索引
- `AGENTS.md`（入口）/ `docs/README.md` / `docs/03-roadmap.md`（M0–M12）/ `docs/01-feature-set.md`
- 契约 **C1–C9**（C8 检查诊断 / C9 值与输出）；**C10**（SSA dump，M6 建）；**C11/C12** 见 M9/M10
- manifest `M0–M9` 已填实；`GAP-01.md` 四维归属；`M*-plan/progress.md` 为各期台账
