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

## 当前状态（2026-10-27）
- **M0–M3 完成；M4/M5 部分兑现；M6–M10 全部收口**
- **M10（精确 STW GC）已收口**：九条门禁 `GC-1..GC-9` 9/9；契约 `C12` 定稿。
  读数 **235 suite / 2234 passed / 0 failed / 2 ignored**；语料
  **off / on / stress 各 69/0/0**（`on` 态真的收垃圾）；变异 **20/20 全红**；
  长跑 100 MB ⇒ `live=1040B` / `heap=17MB` / `chunks=1` / `rss=2772KB`。
  账在 `docs/milestones/M10-manifest.json`（`manifest_m10.rs` 对账，含
  「长跑读数必须能复现」），复盘在 `M10-report.md`
- **M11（native 并发）已收口**（2026-10-27，只做 a/b/c）：七条门禁 `GO-1..GO-7` 全绿。
  读数 **242 suite / 2278 passed / 0 failed / 2 ignored**；69 单个语料三态各 69/0/0；
  7 个并发语料三态各 7/0/0（`M'` 12/12）；三层差分 7/7；变异 **16/16 全红**；
  **溢出深度实测 N=452**（64 KiB 栈）。账在 `M11-manifest.json`（`manifest_m11.rs`
  对账 + 三态复现），复盘在 `M11-report.md`。M11d/e/f 明确不做。
  ⚠️ 白名单 **10 条**（计划 8 条 + 实测窄化 `U-M11-09` select 的 comma-ok /
  `U-M11-10` go 的聚合浮点实参）—— 范围外**不要顺手做**。
- M9 终态：native 69/0/0；`--run` 69 绿；`--via=ssa` 69 绿（拒绝面为空）
- M6 收口：语料 N=46 / M=23 冻结；66 变异全红

## M11 硬约束（改并发/切换前必读）
- **`g_frames` 是「**当前 G** 的链头」**，不是全局唯一链：`rt_schedule` 两条赋值
  （`prev->frames = g_frames` / `g_frames = next->frames`）。序言/尾声**不改**
  ⇒ M10 的 14 条栈根判据原样有效
- ⚠️ **扫根要遍历所有 gdesc（三种状态都扫）**，不只当前 G：挂起 G 的栈上有活值。
  另外 `go` 的**实参区**在新 G 栈顶、**不在帧记录里** ⇒ 自成一类根
  （`gdesc.argbytes` + 按字保守标 + trampoline 读完就清零）。漏它 = 闭包块被收
  ⇒ trampoline 调 `[fv+8]` 撞 **nil 检查 panic**（不是「读到垃圾」！）
- ⚠️ **`gc_pause != 0` 时自动触发与 stress 钩子都跳过**：只在 pause 里分配的程序
  （纯 select 循环）**一次 GC 都不跑** ⇒ 判据会恒真（`live=0` 永远 ≤ 阈值）
- ⚠️ **序言栈检查要在界内报错**：`stack_lo` 之下留一页（`G_STACK_SLACK`）+ 冷路径
  `mov sp, x9`。否则 panic 路径自己的栈帧落在映射外 ⇒ **SIGSEGV 而不是退出 2**
  （`main` 没事只因为它的 `stack_lo` 比真栈底高 4 MiB）
- ⚠️ **不能在自己栈上 `munmap` 自己**：退出的 G 进僵尸**链表**，由下一个调
  `rt_schedule` 的 G 收尸；`gdesc` 槽回 `g_free` 并整体清零（`frames`/`wait` 是悬空指针）
- `Hchan` 是 `K_OBJECT`（`chan_desc`：字 2 = buf、字 7..10 = recvq/sendq）；
  `WaitSet` 是 `K_RAW` ⇒ 里面的 `Hchan*` 要在 `mark_all_g_stacks` 里逐个标
- **`mark_frames(head)` 收参数**（每个 G 一条链）；只扫 `g_frames` 就是变异 `M-M11-13`

## M10 硬约束（改 GC 前必读）
- **`C12` §4 是地基**：十类根（M11c 增第 9「其它 G 的栈」/第 10「go 实参区」）/ 帧记录 `{prev, fi, sp_idx}` 三个字 / 位序
  `bit i ↔ fp - frame_size + 8i` / `sp_off` 是**字下标**不是位偏移
- ⚠️ **`is_safepoint` 是穷尽匹配**（不留 `_ => false`）：新增 `Instr` 变体会
  编译失败 —— 那是故意的。漏一条 = 那个分配点没有位点 = 扫到错的字。
  **M10 里漏了四次**（`Make` / `MakeClosure`+`Convert`+`Bin` / `Composite`）
- ⚠️ **起始位图的语义是「这里有一个块头」，不区分活块与空闲块**：
  `free_push` 要 `bm_set`、sweep 里已空闲的块要 `bm_clear`。漏了 ⇒ 假块首 ⇒
  拿旧 desc 扫垃圾 ⇒ 段错误（`ken/simpswitch.go`）
- ⚠️ **`gc_alloc` 在 stress 下分配前跑 GC** ⇒「已分配但还没发布」的块看不见。
  两条出路：`gc_pin`（中间还要再分配一次，如 `map_new`）/
  `gc_pause`（一次分配 + 立即发布，如 `rt_alloc_escaped`）
- ⚠️ **位图是 live-out 的保守超集，不是精确 live-in**（`R-10-21`）：这是**策略
  选择**（多标=泄漏一轮 / 漏标=活对象被回收，代价不对等），不是「live-in 不可行」。
  原先「真 live-in 会让四个语料全坏」的根因已查清 = `rt_defer_push` 的缺口，见下。
- ⚠️ **`rt_defer_push` 的分配必须 `gc_pause`**（`R-10-24`，`C12` §6.1）：
  `rt_arena_alloc` 走 `gc_alloc`，stress 钩子在**分配之前**跑 GC，而此刻新
  `DeferRec` 还没挂 `g_defer_top`、`Defer` 又不是 safepoint（`sp_idx` 停在
  `MakeClosure` 位点）⇒ 闭包块的唯一根是那张位图。守卫 `t_gc_12` 断言顺序
  `gc_pause++` < 分配 < `g_defer_top = r;` < `gc_pause--`（只查「出现」不够）。
  踩过：去掉它 + 真 live-in ⇒ 闭包被回收 ⇒ 地址被下一个闭包复用 ⇒
  `rt_deferrun` 调到别人的闭包 ⇒ panic 且 `recover()` 救不回来。
  代价是「扫到已死指针」是常态 ⇒ `bad_ptr` 只计真错
- ⚠️ **`bad_ptr` 的口径**：只计「解析不到块首」与「块头非法」。别把
  「指向已回收的块」加回去 —— 那会让探针恒非 0、然后没人看
- ⚠️ **逃逸槽必须进托管堆**（`rt_alloc_escaped`）：M9 放 arena ⇒ 闭包捕获
  `&槽` 时槽里的指针没有根（最小复现打印 204 而不是 42）
- ⚠️ **`MapObj.count` 必须留在 `+0`**：`len(m)` 是 `ldr [m]`（没调
  `rt_map_len`）；`slots` 在 `+40`，gcdata 用 bit5
- ⚠️ **自动触发看 `gc_since` 不看 `heap_committed`**（后者一跳 16 MiB）：
  `next = max(gc_threshold, gc_live_bytes*2)`
- **`arena` 保留面 6 条**，`gc_arena.rs::t_gc_11` 按源码片段正反向查；
  新增 `rt_arena_alloc` 调用点必须同时改表

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
