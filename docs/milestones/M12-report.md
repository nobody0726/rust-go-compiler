# M12 报告 · a 线（泛型：检查面 → 单态化 → native）

> 契约：`C8`（诊断码，不新增）/ `C9`（§4.1 合并流，Ruling 24-1）/ `C12`（三态口径不变）。
> 台账：`M12-plan.md`（进度台账 `.superpowers/sdd/M12-plan/progress.md`，不入库）。
> 清单：`M12-manifest.json`（数字都在那儿，由 `manifest_m12.rs` 对账）。
> 本文件先立 a 线（Task 27）；b 线（反射）/ c 线（opt）收口后各自补节。

---

## 1. 读数（a 线，全部实测，可复跑）

| 项 | 读数 | 复跑命令 |
|---|---|---|
| 四条门禁 | fmt / check / clippy 全绿；**247 suite / 2327 passed / 0 failed / 2 ignored** | `cargo test --workspace --no-fail-fast` |
| 门禁面 | **116 = run 71 + compile 40 + errorcheck 5**；拒绝面 **76** | `bash tools/run-oracle/m12-classify.sh` |
| run 族（三层） | t_gen_01：**68/71**（白名单 3 冻住）；装置全族 **63/71**（skip 8 全白名单，bad=0） | `cargo test -p rgoc-driver --test m12_checkface t_gen_01`；`bash tools/run-oracle/run-native-typeparam.sh --only run` |
| compile 族 | **39/40**（skip 1 = typelist，U-M12-01） | `bash tools/run-oracle/run-native-typeparam.sh --only compile` |
| errorcheck 族 | **5/5** 首码与官方 ERROR 行一致（Task 22） | `bash tools/run-oracle/run-native-typeparam.sh --only errorcheck` |
| rejected 族 | **76/76** 干净拒绝，码集合 ⊇ `TYP-UNSUPPORTED-IMPORT-FORM` | `bash tools/run-oracle/run-native-typeparam.sh --only rejected` |
| HIR 无残留 | lower 成功的 **108** 个门禁文件，`type parameter` / `invalid` 出现 0 次（+ 阴性对照） | `t_gen_05_无残留` |
| 实例名确定性 | 两次 build 的 `.s` 逐字节相同；`Sum·int`/`Sum·string`/`Sum_int` 在 dump-hir 并存 | `t_gen_04_实例名确定性` |
| 泛型 × 并发 | 4 语料 × off/on/stress **逐字节相同**（gen_* 断言 `collections>0`） | `t_gen_11_泛型并发三态` |
| 变异 | a 线 **3/3 全红**（M-M12-01..03）+ Task 21 的 4/4（m21） | `.superpowers/m25-mutate.py` / `.superpowers/m21-mutate.py` |
| M11 不回归 | 69 单线程三态 + 7 并发三态（`M'` 12/12×3） | `run-native.sh` / `run-native-chan.sh` × 3 态 |

---

## 2. 交付（a 线）

* **检查面**：类型参数与约束的完整判路 —— 类型集四态投影（`IfaceTSet`）、
  逐 term 谓词（`common_slice_elem` / `common_map_key` / 内建六件套）、
  `under_or_core`（单档 core_type）、实例化闭包（不动点 + 深度环检测 +
  **诊断回滚**：实例体复查不产重复诊断）。
* **单态化**：`·` 结构性命名（实参间 U+00B7、复合内部 `.`，不可冒充）、
  memo 按 `identical`、深度封顶（issue48711 的环按**嵌套深度**判，不是条数
  —— 只按条数封顶时容器 OOM exit 137）、只实例化**可达**实例（`D-M12-3`）。
* **发码口**：`is_abstract` 必须递归（`type_mentions_tparam`）—— named 包着
  类型参数的实例（issue54225）不下沉；实例体过同一条 SSA → arm64 管线，
  没有泛型专用指令。
* **装置**：`run-native-typeparam.sh`（四族三层 + L1 逐流 / L2 **合并流**）、
  `m12-classify.sh`（七份清单**算出来**）、变异装置 m21/m25（apply 前查
  未提交改动 + 原文副本恢复）。

---

## 3. 关键决策的实测复核

* **Ruling 24-1（L2 合并流）**：官方 `println` builtin 进 stderr（Go 规范），
  我们的契约（`C9` §4.1）定「println + panic 合并流写 stdout」—— 逐流比
  必然假红（`native.out` 有字节、`oracle.out` 空、字节全在 `oracle.err`）。
  官方 `testdir_test.go` 的判据形态同样是**合并**比。L1（native vs 解释器）
  保持逐流 —— 两套实现同口径，混流反而看不见「打错管子」的错。
* **Ruling 24-2（本体占位）**：局部泛型类型 `type V U[int]` 的本体静默不填
  会让 `V(0)` panic（`named_underlying` expect）。先填 `invalid` 占位止血，
  缺口登记 U-M12-10 —— 「不许 panic」优先于「不许假错」。
* **Ruling 25-1（实例名比 dump-hir）**：`.s` 里 arm64 后端把函数重编号成
  fn0/fn1 —— T-GEN-04 的「名字含 `·`」在 `.s` 上**验不了**，改在 dump-hir
  验；`.s` 层的确定性由「两次 build 逐字节相同」覆盖。
* **Ruling 25-2（M-M12-03 改靶）**：计划原靶 T-GEN-01 打不红 —— run 语料
  全是官方跑得通的程序，「多档约束取核心类型」是官方**必拒**形状，天然
  不在其中。补 T-GEN-19（内联约束三段对照）为有效靶。

---

## 4. 真错清单（按「为什么先前没抓到」分类）

### 4.1 「两份实现口径分叉」——接口混合比较恒假（Task 21）

`var x int; var i interface{}; x == i` 在解释器里恒 false：

1. **lower 层没装箱**：检查器只判定类型合法、不动树；HIR 里裸
   `slot(x) == slot(i)`，`go_eq` 没有 `(Int, Iface)` 臂 ⇒ 恒 false。
   装箱必须在 lower 做 —— 运行时没有操作数的静态类型。
2. **`go_eq` 的动态类型用裸 `TypeId` 相等**：`TypeTable::add_basic`
   **故意不去重**（设计如此）⇒ 同一个 `int` 有两个条目 ⇒ 相等被骗过。
   修法：`go_eq_tt` 先 `identical` 归一再比。

native 一直绿是 SSA 层自己补了拆箱 convert —— **求值器与后端必须同口径**
这条铁律的第 N 次应验。抓到它靠 t_gen_01 把 71 条语料逐条跑 interp+native。

### 4.2 「判据没人报」——errorcheck 假绿（Task 22）

`issue51232/51233`（`Fn[RCT]` 少实参）rc=0 假绿：装载面 `unwrap_or(base)`
把数量错**吞掉**（注释指望检查器报），而检查器的 `type_expr` 看不到
声明位置。修在装载面（Index 臂数量校验 + 报码 + 按书写位置去重）。
教训：**「错误恢复退回默认值」必须留一条「这里报过错」的痕迹**，否则
两层面互相指望就都没报。

### 4.3 「判据没写过这种程序」——局部泛型类型 panic（Task 24）

`type U[_ any] int; type V U[int]; x := V(0)`（函数体内）⇒ driver panic：
check 面局部类型声明不挂 `type_params` ⇒ `U[int]` 被 type_expr Index 臂判
「非泛型 named + 实参」⇒ None ⇒ V 的本体静默不填 ⇒ 转换读本体 panic。
二分定位：nested12 → nested17 → nested18 → 最小复现 tmp_n20（3 行）。
二分全程用**最小复现文件**而非语料原文 —— 语料 90 行，复现 3 行。

### 4.4 「契约与官方口径漂移」——L2 假红（Task 24）

逐流比官方必红（见 §3 Ruling 24-1）。症状像「oracle 缓存坏了」：重生成
后同样 —— 因为**根本不是缓存问题**。判据口径错的时候，症状会指向任何
地方，唯独不指向判据。

### 4.5 「变异绿的正确用途」——M-M12-03 打错靶（Task 25）

core_type 不查类型集的变异在 T-GEN-01 上**不红**。两种成因必须先分辨：
① 判据缺形态；② **探针打错靶**。这里是 ②：run 语料全是官方跑得通的
程序，「多档约束取核心类型」是官方必拒形状，天然不在其中。处置：补
T-GEN-19（三段对照：多档不同必拒 / 单档过 / 多档同底层过）—— 变异的
「绿」逼出了一条原本不存在的判据，这正是变异该干的活。

### 4.6 老坑重放（每条都咬过一次以上）

* **checkout 旧 commit 编译会覆盖 target/debug**：errorcheck 5/5 → 3/5
  的「读数抖动」是它，不是回归。重编即愈。
* **变异恢复不能 `git checkout`**：未提交改动会被一起丢（M10 Phase 0
  踩过）。m21/m25 装置因此强制「apply 前查锚点文件干净 + 原文副本恢复」。
* **`cargo clean -p <crate>` 不能省**：绑定挂载上 mtime 整秒粒度，
  变异后不 clean 可能跑旧 rlib（M-M12-03 首测行为没变就是这个原因，
  当时 `2>/dev/null` 还把编译报错吞了 —— 静默重定向会把「编译失败跑旧
  二进制」伪装成「变异没效果」）。
* **判据共享目录里的标签必须全局唯一**：`probe()` 写
  `target/m12-checkface-src/`，首版 t_gen_12 与既有
  `t_gen_12_接收者的类型参数` 撞 `gen12a/gen12b` 标签 —— 并行跑互踩，
  表现为「单跑绿、全量偶尔红」的抖动。

---

## 5. 判据的「牙」（变异 7 条全红）

| 变异 | 内容 | 靶 | 结果 |
|---|---|---|---|
| m21-E | 混合比较装箱整条短路 | t_gen_01 | RED |
| m21-F | `identical` 归一恒不命中 | t_gen_01 | RED |
| m21-G | `is_abstract` 退回只看布局 | t_gen_01 | RED |
| m21-H | interp 方法表 key 还原整个 mangled 名 | t_gen_01 | RED |
| M-M12-01 | 实例化不动点只收敛一轮（嵌套请求抽不干） | t_gen_01 | RED |
| M-M12-02 | mangled 实例名丢实参后缀 | t_gen_04 | RED |
| M-M12-03 | core_type 不查「underlying 全同」 | t_gen_19（原靶 T-GEN-01 打不红，见 §4.5） | RED |

装置纪律（m25，§14.5/§14.6 之后固化）：apply 前锚点文件必须干净
（git status 拒绝）、锚点字符串必须**唯一**（count==1 校验）、恢复用
原文副本（`.m25bak`）、变异前 `cargo clean -p`。

---

## 6. 不承诺（`U-M12-01..10`，细节见 manifest）

一句话版：**部分实例化**（01）/ **reflect.MakeFunc**（02）/ **可变参数
Call**（03）/ PGO·DWARF·coverage（04）/ **泛型原体独立检查**（05）/
并发 GC（06）/ `.dir` 目录测试（07）/ `errorcheck -m`（08）/
**native 接口断言·itab 输出级老缺口**（09，8 条语料白名单冻住，ec8fd74
实测同错非回归）/ **局部泛型类型实例化**（10，本体 invalid 占位）。

---

## 7. 与 M5–M11 的边界

* `C12` 的十类根、`is_safepoint` 穷尽匹配、stress 间隔口径**一个字没动**
  —— 泛型实例走同一条分配管线，`t_gc_11` 的 arena 保留面 6 条原样。
* `C8` 的 26 类诊断码**不新增**：`TYP-INVALID-INSTANCE-CYCLE` /
  `TYP-WRONG-TYPE-ARG-COUNT` 都已存在（baseline §5 校正 4）。
* M11 的并发门禁（`GO-1..7`）与三态读数原样复跑不回归（Task 24）。
* `TypeDesc` 仍是 40 字节，`t_desc_*` 判据未改。

---

## 8. 接手指南（b/c 线）

```bash
# 1) 全量：四条门禁（容器内）
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work/rgoc && \
  cargo fmt --all --check && cargo check --workspace --all-targets && \
  cargo clippy --workspace --all-targets -- -D warnings && \
  cargo test --workspace --no-fail-fast'

# 2) typeparam 四族（容器内；--no-oracle 用缓存跳过官方层）
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work && \
  bash tools/run-oracle/run-native-typeparam.sh --list'

# 3) M11 语料不回归
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work && \
  for m in off on stress; do bash tools/run-oracle/run-native.sh --gc=$m; done'

# 4) 变异（m25：逐条 apply / test / restore）
RGOC_TTY=0 scripts/in-container.sh bash -lc 'cd /work && \
  python3 .superpowers/m25-mutate.py apply I && \
  cd /work/rgoc && cargo clean -p rgoc-check && \
  cargo test -q -p rgoc-driver --test m12_checkface t_gen_01'
```

**改泛型路径的四条铁律**：

1. **两份 typeexpr 实现必须同改**：`rgoc-load/src/typeexpr.rs::resolve_type`
   与 `rgoc-check/src/expr.rs::type_expr` —— 改一处不改另一处，声明面与
   使用面的判定就分叉（Task 22 的教训）。
2. **「错误恢复退回默认」必须留报错痕迹**：`unwrap_or(base)` 一类的
   恢复路径要么自己报码、要么断言上游已报，否则两层面互相指望（§4.2）。
3. **实例名只在 dump-hir 可见**：`.s` 里是 fn0/fn1 —— 别在 `.s` 上找
   `·`（Ruling 25-1）。
4. **变异「绿」先分辨成因再动手**：判据缺形态 vs 探针打错靶（§4.5），
   处置完全不同。

---

## 9. 遗留

见 `M12-manifest.json` 的 `open_gaps`（3 条）。最值得先看：

* **具名约束拿不到核心类型**（`G-M12-01`）：`core_type` 的 unalias 不解
  Named ⇒ `type C interface{ ~*int }` 约束上的 `*p` 一律拒。修法一行
  （先 underlying 再判 Interface），但要先证明单档具名约束语料全绿。
* **b 线（反射）/ c 线（opt）**：`G12-7..13` 与 `M-M12-04..08`（reflect/
  opt 族的变异）在后续 Task，manifest / report 届时补节。
