# M12 报告 · a 线（泛型）+ b 线（有限反射）+ c 线（优化 pass）

> 契约：`C8`（诊断码，不新增）/ `C9`（§4.1 合并流，Ruling 24-1）/ `C12`（三态口径不变）/
> `C13`（实例化与反射元数据，Task 43 定稿）。
> 台账：`M12-plan.md`（进度台账 `.superpowers/sdd/M12-plan/progress.md`，不入库）。
> 清单：`M12-manifest.json`（数字都在那儿，由 `manifest_m12.rs` 对账）。
> 本文件按线分章：a 线 §1–§9（Task 27）、b 线 §10（Task 36）、c 线 §11（Task 42）。

---

## 0. 收口总读数（2026-10-29，全部实测）

| 面 | 读数 | 在哪一章 |
|---|---|---|
| 门禁 | **`G12-1..13` 13/13 pass** | manifest `gate` |
| cargo | **251 suite / 2355 passed / 0 failed / 2 ignored**；fmt / check / clippy `-D warnings` 全绿 | §1 / §11 |
| 泛型门禁面 | **116 = run 71 + compile 40 + errorcheck 5**，拒绝面 **76**；装置实测 `ok=183 skip=9 bad=0`（skip 全是白名单） | §1 |
| 69 单线程语料 | 三态（off / on / stress）各 **69 ok / 0 skip / 0 bad** | §1 / §10.1 |
| 7 并发语料 | 三态各 **7/0/0** + `M'` **12/12** | §10.1 |
| 变异 | a **3/3** + b **3/3** + c **3/3** 全红（c 线一次跑完：`RED=3 GREEN=0 MISS=0 HANG=0 ERROR=0`） | §5 / §10.1 / §11.7 |
| c 线收益 | `.s` **−22.0%**（1220189 → 951616）；`bl rt_alloc_escaped` **45 → 41**；十万键 **1.109 组/次** | §11.1–§11.3 |
| 切断 | 字典替换**不做**（收益上界 1408 B / 22 个副本 ≈ **13 B/程序**） | §11.5 |
| 未关缺口 | `G-M12-01..05` 五条；白名单 `U-M12-01..12` 十二条 | `GAP-01.md` M12 节 |

> 一句话：**泛型从检查面打通到 native、反射给到「能读能写能调」的有限面、
> 优化只留三条量得出来的**。所有「没做」的东西都在白名单里逐条登记，所有
> 「做了」的东西都有一个能失败的判据。

---

## 1. 读数（a 线，全部实测，可复跑）

| 项 | 读数 | 复跑命令 |
|---|---|---|
| 四条门禁 | fmt / check / clippy 全绿；**251 suite / 2355 passed / 0 failed / 2 ignored**（收口态，Task 43） | `cargo test --workspace --no-fail-fast` |
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

## 6. 不承诺（`U-M12-01..12`，细节见 manifest）

一句话版：**部分实例化**（01）/ **reflect.MakeFunc**（02）/ **可变参数
Call**（03）/ PGO·DWARF·coverage（04）/ **泛型原体独立检查**（05）/
并发 GC（06）/ `.dir` 目录测试（07）/ `errorcheck -m`（08）/
**native 接口断言·itab 输出级老缺口**（09，8 条语料白名单冻住，ec8fd74
实测同错非回归）/ **局部泛型类型实例化**（10，本体 invalid 占位）/
b 线的两条：**字段只读层**（11）/ **合成 `*T` desc 同身份**（12，见 §10.4）。

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
* **c 线（opt）**：`G12-10..13` 与 `M-M12-07..09`（opt 族的变异）在后续 Task，
  manifest / report 届时补节。（b 线已收口：`G12-7..9` 全 pass，见 §10。）

---

## 10. b 线（有限反射，`G12-7..9`）

> 契约：`C13` §5（§5.1 元数据载体 / §5.2 `ReflectMeta` / §5.3 写面与屏障 /
> §5.4 扩展区 / §5.5 `Call` 的 ABI 桥 / §5.6 写面的分配入口）+ `C12` §4.7。
> 面是**刻意收窄**的：导出符号集合由 `t_ref_01` **相等**冻结，多一个少一个都红。

### 10.1 读数（全部实测，可复跑）

| 项 | 读数 | 复跑命令 |
|---|---|---|
| 四条门禁 | fmt / clippy 全绿；**248 suite / 2335 passed / 0 failed / 2 ignored** | `cargo test --workspace --no-fail-fast` |
| 导出面 | **12** 个顶层符号（6 类型 + 6 函数）+ `Value` 的 17 个方法；`MakeFunc` 断言不在 | `--test m12_reflect t_ref_01` |
| 元数据 | 20 个类型逐字段（kind/size/align/name/pkgpath + struct 字段 + 容器 elem/key/len）与官方 `go run` 逐行 | `t_ref_02` |
| 方法表与身份 | `Box[int]` 的 `String()` / `NumMethod()` / `Method(0).Name` / `Implements·AssignableTo·ConvertibleTo` | `t_ref_05` |
| `Call` | 九档 ABI 逐行（0 参 / 3 整数 / 2 浮点 / 混合 / **10 参走栈** / 16B 聚合按值 / 多返回 / 零大小 / nil 指针 + `Kind()=19`） | `t_ref_03` |
| 写面 | 十三条逐行（`New`/`Elem`/`Index`/`Set*`/`Make*`/`CanSet` 真假）+ 两条 panic 文本逐字、退出 2 | `t_ref_04_写面逐条` / `t_ref_04_panic文本对齐官方` |
| 反射 × GC | stress **collections=2446 / allocs=2450 / frees=2413 / live=1872 / `bad_ptr=0`**；off 与 on 各 **0 次收** ⇒ 只有 stress 能证伪 | `t_ref_06_写面在_stress_下保根` |
| 变异 | b 线 **3/3 全红**（M-M12-04..06） | §10.3 |

### 10.2 交付

* **数据面**：`TypeDesc` 的 `+14` 对齐填充改语义为 `meta_idx`（40 字节本体一个
  不动，M9 的 `t_desc_*` 判据零改动）；`ReflectMeta` 72 字节（`+64 tramp`）；
  6 个反射专用运行期入口（`rt_reflect_meta` / `rt_reflect_metas` 表 /
  `rt_reflect_call` / `rt_reflect_alloc`）。desc 扩展区按 kind 判读
  （map: key+elem / chan: elem / array: len+elem / named: meta 收尾）。
* **读面**：`TypeOf`（`any` 的第一字就是 `*TypeDesc`，零运行时计算）+
  `rtype` 的 Kind/Size/Align/Name/PkgPath/String/NumField/Field/FieldByName/
  Len/Key/Elem/NumMethod/Method/Implements/AssignableTo/ConvertibleTo +
  `ValueOf` 与 `Value` 的读访问器。
* **调用面**：`Value.Call` 三层 —— Go 侧摆缓冲 → `rt_reflect_call`（一行转发）
  → `__reflecttramp_<sig_hash>`（发射侧按签名生成的直线代码：装寄存器 /
  `blr` / 结果整体落帧再分发）。**打包不在 C 里做**：寄存器落位在编译期就定了
  （`D-M12-5`），通用打包要把寄存器号变成数据再绕回 `blr`。
* **写面**：`Value.writable` 一个布尔（官方 `flagAddr` + 非只读）+
  `Value.indirect`（官方 `flagIndir`）；`New`/`MakeSlice`/`MakeMap`/`MakeChan`
  全部走 `rt_reflect_alloc(op, …)`。
* **GC**：写面**不发屏障**，前提写成 `C12` §4.7（`D-M12-7`）的三条 ——
  GC 只在安全点、写操作不分配、目标槽已发布。

### 10.3 两条真错（都是「判据没写过这种程序」才漏的）

1. **跨包具体方法调用根本不进 HIR**（Task 33 的前置）。`find_method_path` 只查
   **本包**的方法表（`adopt` 时 `clear()`），于是 `.Call`/`.Int`/`.Get` 这类
   「导入类型 `reflect.Value` 的具名方法」在 lowering 期一律「找不到字段或方法」。
   修：`Cross.methods`，键 =**接收者 `TypeId`**（按类型名会串味：`lib.T` 与 `T`
   是两个类型）。
2. **指针类的两种表示没分开**（Task 35 抓到）。`ValueOf(&x)` 的 `ptr` 是
   **指针值**（`box_iface` 的 `is_ptr` 特例不装箱），而 `New(*int).Elem()` 的
   `ptr` 是**装着指针的槽**。少了官方的 `flagIndir` 这一位，
   `reflect.New(reflect.TypeOf((*int)(nil)))` 链上会打出一个裸地址
   （实测 `ptr-root 281472996999248`）。修：`Value.indirect` + `pointee()`
   归一，`Elem()`/`Set`/`Call` 的指针实参都走它。

⚠️ 另有一条**判据自身**的坑：`t_ref_06` 的 churn 第一版只造 32B 的垃圾，而
被测的 `[]int{cap:2}` 底层数组是 16B —— 块丢了根也不会被复用，读回原值
**假绿**（M-M12-05 首跑就是绿的）。改成 8/16/32B 三种尺寸、写进去的值带
`+1000` 之后才露馅。

### 10.4 不做面

| ID | 不做 | 判据 |
|---|---|---|
| `U-M12-02` | `reflect.MakeFunc`（需要反向 trampoline）| `t_ref_01` 断言它**不在**导出面 |
| `U-M12-03` | 可变参数 `Call`（`CallSlice` / `...any`）| `t_ref_03` 的分档里没有它 |
| `U-M12-11` | 结构体字段的只读层（`flagRO`）与 `Value.Field(i)` 的可写视图 | 字段读面不产出可寻址的 `Value` |
| `U-M12-12` | `New` 合成的 `*T` desc 与程序里的真 desc 同身份 | `ptrDesc` 现场合成，`TypeOf(&x) == New(TypeOf(x)).Type()` 为假 |

解释器通路：`rt_reflect_call` / `rt_reflect_alloc` 都**只有 native 后端有**
（两处通路都报 `G-M12-05`）—— 反射的整体定位是 native 特性，不静默降级。

## 11. c 线（优化 pass，`G12-10..13`）

> 本章在 Task 42（c 线收口）定稿：四条门禁逐条读数（§11.1–§11.5）+ 变异三条
> （§11.7）+ 探针纪律（§11.6 / §11.7.2）。结论一句话：**三条留下，一条量完切断**。

### 11.1 内联（Task 37，`T-OPT-01`）

| 项 | 读数 | 复跑 |
|---|---|---|
| 69 语料 `.s` 行数 | `none=1220189` / 四 pass（无内联）`=1202917` / 全开 `=951616`（比 none **−22.0%**，比四 pass **−20.9%**） | `--opt none\|fold,dce,copy,cse\|all --emit asm` |
| fixture（**两**调用点） | `191 → 164` 行、函数 `2 → 1`、`bl fn` `3 → 1` | `m12_opt.rs::t_opt_01_内联之后调用点消失且体积下降` |
| 同一 fixture（**八**调用点） | `317 → 338` 行（**涨**） | 同上测试的注释记录 |

「八调用点反而涨」是这一节的要点：一处内联的体比一条 `bl` 贵，收益全在
**被调方被收尸**那一份，而它只算一次。所以判据不能写成「内联一定让体积降」，
只能写成「两调用点这一形态降」——判据钉住的是形态，不是口号。

### 11.2 精细逃逸（Task 38，`T-OPT-02`）

| 项 | 摘要生效 | 反事实（`EscInfo::empty()`） | 复跑 |
|---|---|---|---|
| 69 语料 `bl rt_alloc_escaped` | **41** 条（有分配的文件 9 个） | 45 条（10 个） | `.probe/esc-count.sh` |
| 哨兵 `a.go`（`g` 只读 `*p`） | **0** 条；三态 `allocs=0 heap=0` | 1 条；三态 `allocs=1 heap=17039872` | 见下 |
| 哨兵 `b.go`（`G = p` 存全局） | 1 条；三态 `allocs=1 heap=17039872` | 同左 | 见下 |

两个哨兵是**同一台编译器换程序**，不是同一程序换编译器：

```go
func g(p *int, n int) int { s := 0; for i := 0; i < n; i++ { s += *p; … }; return s }
// a.go: main 里 g(&x, 100000)        ⇒ 形参不逃逸
// b.go: g 里有 `G = p`（G 是全局）   ⇒ 形参逃逸
```

`a.go` 的形状是 `T-OPT-02` 的全部意义：`&x` 的槽**留在帧里**，`.s` 里一条
`rt_alloc_escaped` 都没有，三态 `allocs=0 / heap=0`，输出恒 `300000`。
反事实那一列才是判据的牙：把摘要换成 `EscInfo::empty()`（= Task 38 之前的
保守档），同一个 `a.go` 立刻退回 `allocs=1 / heap=17039872` —— 说明摘要
**真的在改代码**，而不是「算得很准但没人用」。

三态语料仍 `69/0/0`（off / on / stress 各 69 绿、0 skip、0 bad）。

**交付**：`escape.rs`（模块级逐形参**单调不动点**，轮数上界 `Σ(params+1)`，
递归/互递归不需要特判）+ `frame.rs::escaping_slots(f, esc)` 按摘要逐形参判
+ `layout_with_esc`；`module.rs` 三个要用帧的地方（编译 / 帧图 / 栈图）
**共用同一份** `esc`。

**顺手补的两条漏判**（方向是**保守**，会让一部分程序多分配）：
`CalleeRef::Indirect`（函数值调用，被调方运行期才定）与 `Instr::Go`
（`go f(&x)` 拷的是指针值，指向的仍是调用方帧）。这两条以前**一个字都没算**，
意味着闭包/`go` 之后的 `&x` 靠的是「碰巧没被复用」而不是判据。
净效果仍是 `45 → 41`：补漏抬上去的，比摘要降下来的少。

### 11.3 SwissTable 探测（Task 39，`T-OPT-03`）

| 项 | 读数 | 复跑 |
|---|---|---|
| 十万键探测趟数 | **1.109 组/次**（`lookups=350000 groups=388281`），H2 筛过之后只比了 0.606 次键/次 | `obj_map.rs::t_map_12_十万键的探测读数` |
| 反事实（组步进 1 槽 = 线性探测） | **2.17 组/次** ⇒ 判据红 | 把 `i = (i + MAP_GROUP)` 改成 `i = (i + 1)`，语义不变 |
| 十万键语义 | `len=50000 sum=4999950000 miss=0`（填、全查、删一半、再查） | `t_map_11` |
| 缺省不打统计 | 不设 `RGOC_MAP_STATS` 时 stderr 一个字节都没有 | `t_map_13` |
| 三态语料 | `69/0/0`（off / on / stress） | `run-native.sh --gc=…` |

**改了什么**：槽块从「每槽自带 8 字节状态字」换成「一整块控制字节 + H2 组探测」。

```text
旧: [+0 kdesc][+8 vdesc][+16 槽…]      槽 = 8(state) + align8(k) + align8(v)
新: [+0 kdesc][+8 vdesc][+16 ctrl(cap+16)][槽…]   槽 = align8(k) + align8(v)
    ctrl[i] = 0x80 EMPTY / 0xFE DELETED / 其余 = H2（hash 高 7 位）
```

三件事值得写下来：

1. **`ctrl[i] < 0x80` 这一个比较同时排掉 EMPTY 与 DELETED** —— 扫描器、迭代器、
   探测共用它。这不是巧合，是「H2 只有 7 位」的直接结果；改常量必须三处一起改
   （已写进 `C12` §2.2）。
2. **控制字节不能靠 `gc_alloc` 清零**：`0x00` 是合法 H2，于是新表会被当成「每个槽
   都有键」、又永远遇不到 EMPTY ⇒ 死循环。必须显式填 `0x80`。
3. **负载因子要连墓碑一起算**（`(count + tomb + 1) * 8 > cap * 7`）。只数 `count`
   的话，反复插删的循环永远不扩容，而 DELETED 只增不减 ⇒ `EMPTY` 被耗尽 ⇒
   探测的终止性（`map_probe` 不变式 4）不成立。

组匹配写成**逐字节 SWAR**（`eq_bytes`）而不是 NEON：`rt.c` 是 `-ffreestanding`
的纯 C，`arm_neon.h` 的可用性不是这里能假定的。收益的大头本来就来自「一次读 16 个
控制字节」，比较指令数是次要的 —— 换 NEON 只改 `group_masks` 一处。

> `eq_bytes` 为什么不用「减 1 再与」那套 `haszero`：减法会**跨字节借位**，
> `…00 01` 会在第 1 个字节上报假阳性。判「有没有零字节」无妨，判「**哪些**」就错。
> 改成「拆低 7 位 + 单独看最高位、加 `0x7F` 不进位」之后是精确的逐字节判定。

### 11.4 切断顺序走了一遍，只走到最后一条

`M12-design.md` §4.5 定的顺序是 **SwissTable → 内联/逃逸的预算目标（保正确性）→
字典替换（退回单态化）**。收口时按这个顺序回溯了一遍，**前三条都没到要切的地步**：

| 垫付项 | 预算口径 | 实际 | 是否切 |
|---|---|---|---|
| SwissTable | 「超限就切，保住语义」 | 十万键 **1.109 组/次**（线性反事实 2.17），语义 `50000 4999950000 0` | 不切 |
| 内联 | 「体积目标达不成就只留正确性」 | 69 语料 `.s` **−22.0%**；但八调用点的 fixture 是**涨**的（317→338） | 不切（把「涨」写进 §11.1）|
| 精细逃逸 | 同上 | `bl rt_alloc_escaped` **45→41**，三态 allocs 不升 | 不切 |
| 字典替换 | 「量完超预算就退回单态化」 | 收益上界 **13 B/程序**（1408B/22 个副本，摊 107 个程序）| **切** |

**唯一被切掉的不是因为它贵，是因为它不值**：13 字节/程序的收益，换来每个类型
参数使用都变成「从字典加载 + 一次间接」。这一条是「量完再决定」的正面样本 ——
如果按计划「第一个可切断」就直接不做，会漏掉「上界到底多少」这个数。

### 11.5 字典替换：量过了，**切断**（Task 40，`T-OPT-04`）

计划把这一条标成「第一个可切断」，`M12-design.md` §4.5 的切断顺序也把它放在最后
（「退回单态化」）。**这次不是没时间做，是量完之后不该做。**

先量「单态化到底多花了多少字节」，也就是字典替换的**上界**：字典传递能省的，只有
「同一个泛型来源、不同实例化类型、但生成的指令**完全一样**」的那些副本。

| 面 | `.text` 近似 | 实例函数 | 其中**完全相同的副本** |
|---|---|---|---|
| 116 门禁语料（107 个有产物） | 14084 B / 3521 条指令 | 4052 B / 92 个（**28.8%**） | **1408 B / 22 个（10.0%）** |
| 69 语料（native 主门禁） | 17428 B / 4357 条指令 | **0 B / 0 个** | 0 B / 0 个 |

两条读数合起来就是结论：

1. **69 语料里一个泛型实例都没有** —— 那一侧字典替换的收益恒为 0。
2. typeparam 侧的收益上界是 **1408 字节、摊到 107 个程序约 13 字节/程序**。而 92 个
   实例函数里只有 22 个是重复体：另外 70 个因为**类型不同、指令也就不同**，字典
   替换一样省不掉。代价侧却是实打实的：每个类型参数的使用都从「直接寻址」变成
   「从字典里加载 + 一次间接」，在平均 132 字节/程序的体量上，这些开销会把那 13
   字节吃干净还有多的。

**决策**：保留单态化，不做字典传递。若将来真的出现「实例爆炸」（同一泛型被几十个
同形状类型实例化），先做 Go 那边的做法 —— **按 GC shape 归并**（`[8]byte` 与
`*T` 共用一个 stencil），它只需要「同形状的类型共用一个实例」，不需要引入字典参数
与全部间接层。这条写在这里是为了不让下一个人从头再量一遍。

**顺带发现的一条缺陷**（不在 Task 40 范围内，未修，记 `G-M12-04`）：
`go_source_code/test/typeparam/dottype.go` 是合法的 `// run` 程序，`check` 零诊断、
`--via=hir` 跑得出 `3 / 0 / 0 true`，但 `build`（native）**panic**：

```text
thread 'main' panicked at crates/rgoc-ssa/src/build.rs:4130:
not yet implemented: 2 个结果的调用出现在单值位置（检查器应已拒绝）
```

形状是 `println(f2[int](i))` —— **多值调用当唯一实参**（Go 允许，两个结果展开成两个
实参）。这条不是「检查器该拒绝却没拒」（程序合法），而是 SSA 构建期没有这条展开。
现状是 panic 而不是干净拒绝，属于「不支持面必须干净拒绝」的纪律缺口；但它要新增
一个 `SsaCode` 并给 HIR 扫描加「位置」上下文，超出 Task 40 的面，按纪律不顺手做。

### 11.6 这一段踩的两个探针坑（都会安静地给出 0）

1. **产物名被 `with_extension` 吃掉一层**。`build --emit asm` 的 `.s` 路径是
   `out.with_extension("s")`，而我的探针把语料路径的 `/` 换成 `_` 却没换 `.`，
   于是 `test_convert.go` 的产物叫 `test_convert.s`，按 `test_convert.go.s`
   去找永远找不到 —— 69 个全记成「0 次分配」。教训：读数里必须有
   **「产物在不在」（`nos`）** 这一栏，否则「没产物」和「没分配」长得一模一样。
2. **`touch` + `cargo build` 不保证重建**（绑定挂载上 mtime 判据不可靠）。
   反事实那一轮其实没换二进制，两个相位都是 0，看起来「摘要毫无作用」。
   改 `cargo clean -p rgoc-arm64` 强制重建，并让探针自己带一个**哨兵程序**
   自检「这一轮真的换了编译器」—— 哨兵的读数不区分，就说明整轮无效。

### 11.7 变异三条（Task 41，`M-M12-07..09`）

三条都在 `scripts/mutation-m12.sh` 里，一次跑完、可复跑（`bash scripts/mutation-m12.sh`）：

```text
RED=3  GREEN=0  MISS=0  HANG=0  ERROR=0
```

| 变异 | 锚点（改成什么） | 期望红的判据 | 实测 |
|---|---|---|---|
| `M-M12-07` 内联跨 panic 边界 | `inline.rs` 去掉「有 `Defer` 就不内联」 | `t_opt_01_带_defer_的被调方不内联` | RED（`0 passed; 1 failed`，8 filtered）|
| `M-M12-08` 逃逸判错 | `escape.rs::summary` 的 `Store` 非 slot 根分支不再标逃逸 | `t_opt_02_存进全局的形参_槽进_arena` | RED（`0 passed; 1 failed`，5 filtered）|
| `M-M12-09` 探测少一步 | `map_probe` 的 `if (emp != 0)` → `if (emp != 0 \|\| del != 0)` | `t_map_11_十万键的语义不随探测结构变` | RED（`0 passed; 1 failed`，10 filtered）|

`M-M12-09` 的红是**语义红**，不是读数红 —— 同一段十万键程序在变异下打出
`50822 4999950000 818`，正确值 `50000 4999950000 0`：删除掉的键被重新插了一遍
（`len` 涨 822）、查不到的键被当成「不存在」（`miss=818`）。

#### 11.7.1 `M-M12-09` 的第一版锚点不是红，是卡死 —— 而且卡的不在探测里

第一版只改了一处（`emp != 0` → `emp != 0 || del != 0`），期望它红，结果是
**900 秒超时打满**。缩规模量了一遍（`.probe/m9-hang.sh`，同一段 BIG_MAP 的 n；
`.probe/` 在 `.gitignore` 里、不入库，读数已抄进下面两张表）：

| n | 只改这一处 |
|---|---|
| 8 / 16 / 64 / 1000 | 跑完（`4 28 0` … `500 499500 0`）|
| 200 | 卡在**查询**相位（最后一行 `C del 100`）|
| 20000 / 100000 | 卡在**删除**相位（最后一行 `B sum …`）|

非单调 ⇒ 不可能是「少判一组」这一步本身的逻辑后果。**判别实验**
（`.probe/m9-hang2.sh`：在变异之上再给 `low_bit_index` 加一句 `if (x == 0) return 0;`）
一次定死：

| n | 只改一处 | 再加兜底 |
|---|---|---|
| 8 / 16 / 64 / 1000 | 跑完 | 跑完且正确 |
| 200 | 卡死 | `100 19900 0` |
| 20000 | 卡死 | `10016 199990000 7`（应 `10000 … 0`）|
| 100000 | 卡死 | `50822 4999950000 818`（应 `50000 … 0`）|

成因：`map_probe` 里 `first_del` 的记录写在 `emp` 判定**之后**，所以变异后能走到

```c
*found = 0;
if (first_del != ~0UL) { return first_del; }
return (i + low_bit_index(emp)) & mask;   /* ← 此刻 emp == 0 */
```

而 `low_bit_index(0)` 的 `while ((x & 1u) == 0)` 是死转 —— 它只靠调用点保证非零。
**所以脚本里这条变异是两处编辑**：第一处是逻辑变异，第二处是「让变异别自己
死循环」的兜底（对正确代码零改动，正常路径永远拿到非零掩码）。这也记成
`G-M12-05`：`low_bit_index` 的非零前提没有断言，探测的终止条件一旦改错，症状是
死循环而不是断言失败 —— 而变异检验恰恰靠「断言失败」当证据。

#### 11.7.2 探针纪律（本轮新增三条，都是「安静地不给出红」）

1. **中断会把变异留在工作树里**。首跑被 SIGTERM 打断，`rt.c` 停在变异态，下一轮
   第一条探针报 **MISS**（锚点已经不存在了）。修法：把「当前变异目标」记进全局，
   `trap … EXIT INT TERM` 里 `mv` 还原 + `touch`。
2. **不许并发跑**。两个容器同时跑脚本时，两边往同一个
   `crates/rgoc-driver/target/obj-map/` 写同名可执行文件，一边执行一边被另一边
   改写 ⇒ 进程原地打转（实测 99% CPU 烧掉 28 分钟，既不红也不绿）。`cargo` 自己
   有 target 锁，但**测试跑起来的产物**没有。修法：`target/.mutation-m12.lock`
   用 `mkdir` 抢原子锁。
3. **卡死不是判据**。判据「红」必须是**断言失败**；卡死既不是红也不是绿，还会把
   下一轮拖进「上一轮没跑完」的状态。修法：每条探针 `timeout`，单独记 `HANG`
   一栏，且脚本要求 `RED=3 / GREEN=0 / MISS=0 / HANG=0 / ERROR=0` 五项同时成立。


