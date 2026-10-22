# M5 测试先行清单：HIR 与顺序解释器

> **阶段 ID**：`M5`　|　**状态**：**已冻结（判据面）**（2026-10-22）　|　**冻结触发**：`M5-design.md` 已确认
> **文档索引**：[`../README.md`](../README.md)　|　**上游**：[`M5-design.md`](./M5-design.md)（本清单只兑现它，不改它）　|　**下游**：`M5-plan.md`（任务级计划）→ `M5-manifest.json`
>
> **本清单的性质**：`AGENTS.md` §4.3 硬约束 —— **开工前必须产出**。它**冻结**的是「怎么算通过」：
> 覆盖矩阵、测试 ID、输出判据、语料清单、`unsupported` 白名单、门禁判据。
> **不写实现顺序**（那是 `M5-plan.md`）。
>
> **冻结规则**：本文件一经冻结，**任何**增删测试 ID、改判据口径、放宽白名单/语料清单，都必须**走修订**
> （本文件 §12 修订表 + manifest 同步），不得静默漂移。
>
> **两处「分两次冻结」的例外**（本阶段特有，见 `R-6-1`）：
> ① **入选清单**在 P0 收口后**追加冻结**（候选 80 与判据现在冻结）；
> ② **`print/println` 的浮点边界**在 RED 期用 `go run` 批量对撞后**补钉**（`F10` 已钉住的点不动）。

---

## 0. 冻结声明

| 项 | 值 |
|---|---|
| 分母判据 | **测试 ID 数**：本文件共 **70** 个 `T-EX-*`（甲 40 / 乙 8 / 丙 8 / 丁 6 / 反 8） |
| 冻结日 | 2026-10-22（判据面）/ 入选清单待 P0 后追加（`R-6-1`） |
| 白名单 | §7，**开工前冻结**；跳过项一律**不得**记为 pass |
| 门禁 | §8，`EX-1..EX-9`（设计 §1.4 的定稿版） |
| 语料清单 | §6：`run-candidates.txt`（**80**，现冻结）+ `run-excluded.txt`（**6** 起步）+ `run-selected.txt`（P0 后追加冻结） |
| 判据来源 | §1，三种：`O` / `S` / `H` |
| 输出判据 | §4，**合并流逐字节** + 退出码；唯一容差 = `\r\n`→`\n` |
| 不承诺三件 | §4.4：地址数值 / map 迭代序 / panic traceback |

**修订表**（追加式，不改历史行）：

| 修订 | 日期 | 内容 | 触发 |
|---|---|---|---|
| `R-6-1` | 2026-10-22 | §6 语料清单**分两次冻结**：候选 80 与排除 6 现在冻结；入选清单在 P0（`T-EX-57..61`）收口后追加冻结，逐行理由入 `run-excluded.txt`。理由：入选与否取决于 P0 的 4 族规则修复结果，**先冻结会逼着我们在「修完之前」猜** | 设计 `D-M5-3` |
| `R-4-1` | 2026-10-22 | §4.2 浮点格式：`F10` 已实测的 7 个点（`3.5` / `1e+100` / `1.0`⇒`1` / `99` / `255` / `-1` / `18446744073709551615`）**冻结**；其余边界（`NaN`/`Inf`/`-0`/`float32` 舍入）在 RED 期用 `go run` 对撞后补钉，补钉不改已冻结点 | `F10` 只覆盖常见形态 |

---

## 1. 判据的三种来源

沿用 M1–M4 的记号，全库一致：

| 记号 | 含义 | 用途 |
|---|---|---|
| **`O`** | **oracle 对撞**：官方 `go run`（容器内 `go1.27.1`）产出的合并流，或官方语料 `<同名>.out` 的冻结期望 | 抓「自己写的期望也错」 |
| **`S`** | **spec 手写期望**：照 `doc/go_spec.html` / 官方源码手写 | 锚规范文本；抓 oracle 也错的形态 |
| **`H`** | **结构性 / harness 不变式**：确定性、幂等、预算、两清单守卫、只读性 | 抓「形态漂移」与「不可重放」 |

> 每条 ID **至少**有一种来源；`O` 与 `S` 同时存在时，**两者都要过**（不是二选一）。

---

## 2. 覆盖矩阵

### 2.1 roadmap §M5 五条 ↔ `T-EX-*`

| # | roadmap §M5 句子 | 覆盖它的测试 ID |
|---|---|---|
| 1 | 「HIR lowering、环境/调用帧、按值/按引用捕获、可寻址性、多返回值」 | `T-EX-01..05`、`T-EX-17..19`、`T-EX-25` |
| 2 | 「基础值、数组/struct 拷贝、指针、slice 共享底层、不可变 string、Go map 语义适配」 | `T-EX-07..16`、`T-EX-23` |
| 3 | 「内建、接口动态值/方法调用/断言/type switch、typed nil、method value 和闭包」 | `T-EX-23`、`T-EX-25..31` |
| 4 | 「解释执行包变量/init/main；defer/panic/recover：直接调用限制、命名返回值、嵌套 panic」 | `T-EX-34..40` |
| 5 | 「泛型解释子集可单态化；loopvar 捕获语义直接测试；range over func 未支持时显式拒绝」 | `T-EX-27`、`T-EX-32/33` |

### 2.2 六个子阶段 ↔ 门禁 ↔ 测试 ID

| 子阶段 | 门禁 | 测试 ID |
|---|---|---|
| M5a HIR 与最小求值 | `EX-1` | `T-EX-01..06`、`T-EX-49..52` |
| M5b 函数、帧与初始化 | `EX-2` | `T-EX-17..21`、`T-EX-39/40` |
| M5c 复合值与内建 | `EX-3` | `T-EX-10..16`、`T-EX-22..24` |
| M5d 闭包与接口 | `EX-4` | `T-EX-25..33` |
| M5e defer/panic/recover | `EX-5` | `T-EX-34..38` |
| M5f 顺序基线集成 | `EX-6` | `T-EX-41..48` |
| 横切（前置/回归/反向） | `EX-7/8/9` | `T-EX-57..62`、`T-EX-53..56`、`T-EX-63..70` |

---

## 3. 测试 ID 全表

### 3.1 甲腿：自有 fixture（`S`，40 条）

| ID | 名称 | 判据要点 | 来源 |
|---|---|---|---|
| `T-EX-01` | HIR 节点覆盖 | `Expr` 每个变体、`Stmt` 每个变体至少被一个 fixture 产出；用 `hir --dump` 的节点计数断言（穷尽性） | `H`+`S` |
| `T-EX-02` | 未支持构造显式拒绝 | `go` / `chan` / `select` / `range` over func / `close` 各一条：**退出码 1 + 稳定码 `HIR-UNSUPPORTED-*`**，且**不**是 `todo!()`（进程不 abort） | `S` |
| `T-EX-03` | lowering 诊断一致性守卫 | lowering 用的静默会话诊断流**必须为空**；人为造一个「check 通过但 lower 报诊断」的形态 ⇒ 内部错（`D-M5-2`） | `H` |
| `T-EX-04` | `hir --dump` 确定性 | 同一输入两次运行**逐字节相同**；`#` 头行格式固定；stdout 只有 dump、stderr 只有诊断 | `H` |
| `T-EX-05` | `Place` 全形态 | slot / global / field / index / deref 五形态各一条取址 fixture；**map 元素取址在静态面被拒**（M4 面，M5 复核） | `S` |
| `T-EX-06` | 常量收敛与任意精度 | `1<<100` 赋给 `int` ⇒ 静态 overflow（M4 面）；`int64` 极值往返无损；常量在 HIR 里**保留任意精度**直到收敛点 | `S` |
| `T-EX-07` | 整数算术 | 各宽度回绕（`int8(127)+1` ⇒ `-128`）、有符号右移、移位计数、除零 ⇒ panic、`%` 符号规则 | `S`+`O` |
| `T-EX-08` | 浮点与复数 | `float32` 每次运算后按 f32 舍入；`%g` 格式；复数 `complex/real/imag` | `S`+`O` |
| `T-EX-09` | 比较与短路 | `&&`/`||` 短路（副作用计数）；无类型常量比较；接口比较 | `S` |
| `T-EX-10` | 字符串 | 不可变（`s[0]` 只读）、索引得 byte、切片、`range` 得 rune、`+` 拼接、`len` 按字节 | `S`+`O` |
| `T-EX-11` | 数组拷贝（正反） | `a := arr; a[0]=1` ⇒ `arr[0]` 不变；`&arr[0]` 之后再拷贝仍不串；数组字面量/`[...]` | `S` |
| `T-EX-12` | 结构体拷贝与嵌入 | 同上 + 字段提升选择器 + 匿名嵌入的拷贝语义 | `S` |
| `T-EX-13` | slice 别名与增长 | `s2 := s1; s2[0]=1` ⇒ `s1[0]==1`；`append` 超 cap 后**不再共享**；三索引切片；`copy` 重叠 | `S` |
| `T-EX-14` | map 语义 | 引用语义（`m2:=m1` 共享）；键相等（`NaN != NaN`、结构体按值、接口按 (类型,值)、指针按身份）；`delete`；双值 `v, ok`；插入序迭代 | `S` |
| `T-EX-15` | 指针 | 取址/解引用/指针到字段/指针到数组元素/指针到 slice 元素；`*p = v` 的写穿；nil 解引用 ⇒ panic | `S` |
| `T-EX-16` | 类型转换矩阵 | 数值互转（含截断/符号扩展）、`string`↔`[]byte`/`[]rune`、`rune`/`byte`、接口→具体（断言不是转换） | `S`+`O` |
| `T-EX-17` | 函数、递归、变参 | 递归 1000 层（**证明不依赖 Rust 栈**，`EX-7`）；变参 `...`；空函数 | `S` |
| `T-EX-18` | 多返回值 | `a, b := f()`、`a, b = f()`、`g(f())` 嵌套实参、`return f()` 直传、单值语境取第一个 | `S` |
| `T-EX-19` | 命名返回值与裸 return | 命名结果被 `return` 前赋值、裸 `return` 返回当前值 | `S` |
| `T-EX-20` | 调用语句丢弃结果 | `f()` 当语句（有返回值）⇒ 合法且结果丢弃（**B 族修复的正向 fixture**） | `S` |
| `T-EX-21` | 控制流 | if/for（三种形态）/switch（有 tag/无 tag/`fallthrough`）/goto/labeled break/continue | `S` |
| `T-EX-22` | range 四形态 | array / slice / string / map / int（1.22）；迭代变量个数；`_` | `S`+`O` |
| `T-EX-23` | 内建全集 | `len`/`cap`/`make`/`new`/`append`/`copy`/`delete`/`clear`/`min`/`max`/`complex`/`real`/`imag` 各一条 | `S` |
| `T-EX-24` | `print/println` 格式表 | `F10` 已冻结的 7 个点 + 多参数分隔 + 空行 + nil 指针/接口/slice/map + `rune` 十进制 | `O`+`S` |
| `T-EX-25` | 闭包捕获（按变量） | 捕获后外部改动可见；两个闭包共享同一变量 | `S` |
| `T-EX-26` | 闭包生命周期 | 返回出去的闭包，其捕获变量仍活着（宿主管理内存，`03` §M5） | `S` |
| `T-EX-27` | loopvar（1.22） | `for i := 0; i < 3; i++ { fs = append(fs, func(){print(i)}) }` ⇒ `012`（每轮新建） | `S`+`O` |
| `T-EX-28` | 方法值/方法表达式 | `x.M` 绑定接收者；`T.M` 首参是接收者；接口方法值 | `S` |
| `T-EX-29` | 接口与 typed nil | 动态值/动态类型；`var p *T; var i I = p` ⇒ `i != nil`；`var i I` ⇒ `i == nil` | `S` |
| `T-EX-30` | 类型断言 | 单值（失败 panic）/双值（失败 ok=false）/接口到接口 | `S` |
| `T-EX-31` | type switch | 各 case 命中、`nil` case、`default`、绑定变量的类型 | `S` |
| `T-EX-32` | 泛型单态化 | 显式实例化 `F[int]`、推断、泛型类型 `Box[T]`；同一 `(origin,args)` 复用实例 | `S` |
| `T-EX-33` | 泛型超出面拒绝 | 未支持的形态（如泛型方法值/嵌套实例化超出面）⇒ 稳定拒绝码 | `S` |
| `T-EX-34` | defer 时机与 LIFO | `defer f(x); x=2` ⇒ `f` 收到旧值；三个 defer 逆序 | `S` |
| `T-EX-35` | recover 直接调用限制 | `defer func(){ recover() }()` 有效；`defer recover()` 无效；在普通函数里调 `recover` 无效 | `S` |
| `T-EX-36` | 命名返回值 + recover | recover 后改写命名结果；未命名结果不可改写 | `S` |
| `T-EX-37` | 嵌套 panic | 展开中再 panic ⇒ 覆盖原值继续展开；三层 | `S` |
| `T-EX-38` | 未捕获 panic | 合并流含 `panic: <值>` 前缀、退出码 **2**；**不**比 traceback（`U-M5-03`） | `S` |
| `T-EX-39` | 单包初始化执行 | 全局变量按 `InitOrder` 求值（每个初始化式 `print` 自己的名字，输出序 == `InitOrder`）；`init` 按文件序；最后 `main` | `S` |
| `T-EX-40` | 双包初始化顺序 | `import→被导入包 var→被导入包 init→本包 var→本包 init→main`；用两个小包 fixture | `S` |

### 3.2 乙腿：语料对撞（`O`，8 条）

| ID | 名称 | 判据要点 | 来源 |
|---|---|---|---|
| `T-EX-41` | 候选清单守卫 | `run-candidates.txt` **恰 80 行**；`入选 ∪ 排除 == 候选`（**两向**，无死行、无重复）；候选的生成口径可重放（`F1` 的筛选脚本） | `H` |
| `T-EX-42` | 入选清单逐文件对撞 | 每个入选文件：合并流 vs `<同名>.out`（或空）**逐字节** + 退出码 0；失败**不跳过**、不重试掩盖 | `O` |
| `T-EX-43` | `go run` 三方差分 | 入选集上 `rgoc` / `.out` / `go run` 三方一致；`.out` 与 `go run` 不一致 ⇒ 该文件进排除（理由：期望已脱节） | `O` |
| `T-EX-44` | `ken` 子集专项 | `ken/` 的入选文件单独一组跑（它们是最老的语义测试，覆盖 for/range/接口/指针/移位） | `O` |
| `T-EX-45` | `runoutput` 排除理由 | 5 个 `runoutput`（`index0`/`rotate0..3`）全部在排除清单，理由为「动态编译两段程序，M5 不做」 | `H` |
| `T-EX-46` | 不确定性守卫 | 排除清单里「地址输出 / map 迭代序」类文件**有理由且可复核**；另有一个**探针**：把含地址输出的 fixture 塞进入选集 ⇒ 必红（证明判据真的逐字节） | `H` |
| `T-EX-47` | 排除清单逐行理由 | 每行带理由 + 指针（`F3` 的行号 / 设计章节）；无空理由、无「TODO」 | `H` |
| `T-EX-48` | 重复运行确定性 | 同一批语料连跑两次，合并流 sha256 相同；跨两次容器调用相同 | `H` |

### 3.3 丙腿：装置与 CLI（`H`，8 条）

| ID | 名称 | 判据要点 | 来源 |
|---|---|---|---|
| `T-EX-49` | `run` 退出码表 | 0 = 正常；1 = 静态诊断 / HIR 拒绝；2 = 参数/io/内部/预算/未捕获 panic。六个形态各一条 | `H` |
| `T-EX-50` | 合并流保真 | `print("a"); println(1); print("b")` ⇒ `a1\nb`（**交错顺序**不可丢）；stdout 与 stderr 写入同一 buffer | `H` |
| `T-EX-51` | `--dump-hir` 通道分离 | dump 只进 stdout、诊断只进 stderr；`--dump-hir` 与 `run` 同时存在时**先 dump 后跑**（顺序固定） | `H` |
| `T-EX-52` | 三道上限各自响亮失败 | 帧深 1000 / 求值栈 10000 / 步数 1e8 各造一个超限 fixture ⇒ 退出码 2 + 稳定消息；**宿主不 abort、不 OOM、不挂死**（墙钟 < 10 s） | `H` |
| `T-EX-53` | 四条 cargo | `fmt --check` / `check --workspace --all-targets` / `clippy -D warnings` / `test --workspace --no-fail-fast`：四条 exit 0、无 warning | `H` |
| `T-EX-54` | 五条既有自检不回退 | `check-m0-consistency.py`（231 条）/ `check-m1-consistency.py` / `check-ast-golden.sh` / `check-ast-testdata.sh` / `check-ast-stdlib.sh` | `H` |
| `T-EX-55` | manifest 对账 | `manifest_m5.rs`：`gate` / `test_ids` / `unsupported` / `open_gaps` 逐项对账；三类谎（标 pass 却自认未兑现 / 非 pass 无缺口登记 / 门禁未全绿却自称 done）必须被拦 | `H` |
| `T-EX-56` | 语料只读 | 跑完整个语料批次后 `go_source_code/` 无任何文件被写（`corpus-manifest.sha256` 校验通过） | `H` |

### 3.4 丁腿：P0 前置收口（`S`+`O`，6 条）

| ID | 名称 | 判据要点 | 来源 |
|---|---|---|---|
| `T-EX-57` | A 族：多值调用 | `b, x := f()`、`a, b = swap(1,2)`、`swap(swap(a,b))`、`g(f())`：全部**接受**（exit 0）；`method5.go` / `simassign.go` 转绿 | `S`+`O` |
| `T-EX-58` | B 族：调用语句结果丢弃 | `f()` 当语句（有返回值）**不再**报 `UNUSED-EXPR`；`divmod.go` / `escape3.go` / `ken/rob1.go` 转绿；**反向**：`x + 1` 当语句仍报 | `S`+`O` |
| `T-EX-59` | C 族：`&T{...}` | 复合字面量取址合法（`compos.go` / `ddd.go` 转绿）；**反向**：`&f()` 仍拒 | `S`+`O` |
| `T-EX-60` | D 族：移位/转换/`make` | `i <<= c`（`c` 为 `uint`）合法；`(*Int4)(slice)`（1.17）合法；`make(map[T]V)` 底层同一（`ken/shift.go` / `convert4.go` / `bigmap.go` 转绿） | `S`+`O` |
| `T-EX-61` | 接受率 79/80 | 80 个候选里**恰好 79 个** `rgoc check` 接受；唯一例外 `reflectmethod2.go`（`import "reflect"`，`U-M5-01`） | `O` |
| `T-EX-62` | M4 判据不回退 | `T-CK-01..63` 全绿；`run-collide.sh` 五形态的**退出码与红/绿分布不变**（M4 收口时的实测快照为基线） | `H` |

### 3.5 反腿：变异探针（8 条）

| ID | 变异 | 期望红在 |
|---|---|---|
| `T-EX-63` | 数组赋值改成共享 `Rc`（不做深拷贝） | `T-EX-11/12` |
| `T-EX-64` | slice 赋值改成深拷贝 | `T-EX-13` |
| `T-EX-65` | defer 参数改成**执行时**求值 | `T-EX-34` |
| `T-EX-66` | `recover` 改成「任何 defer 里都有效」 | `T-EX-35` |
| `T-EX-67` | 帧栈改成 Rust 递归（`D-M5-1` 的反向） | `T-EX-17`（1000 层）/ `T-EX-52` |
| `T-EX-68` | 合并流改成只比 stdout | `T-EX-24/42/50` |
| `T-EX-69` | 未支持构造改成静默跳过 | `T-EX-02` |
| `T-EX-70` | `run-oracle` 把「无 `.out`」当成「不比」 | `T-EX-42`（空输出类文件） |

> **每条变异必须实证变红并复原**，记录进 `scripts/mutation-m5-notes.md`（沿 `mutation-m3/m4-notes.md` 格式）。

---

## 4. 判据细节

### 4.1 输出比较（唯一判据）

```text
rgoc 合并流  ==  <同名>.out 的内容        （无 .out ⇒ 期望为空）
退出码       ==  0
唯一容差     :  \r\n → \n                （官方 testdir_test.go:1184 同款）
```

* **逐字节**：不做多重集、不做剥列、不做正则、不做「包含」。
* **合并流**：`print`/`println`（写 stderr）与未来可能的 stdout 写入进**同一个 buffer**，顺序 = 写入顺序（`F4`、`F14`）。
* 失败时保存：源文件、flags、合并流、期望、退出码（沿 roadmap §3.4）。

### 4.2 `print/println` 字节格式（`F10` 实测，已冻结的 7 点）

| 输入 | 输出 | 说明 |
|---|---|---|
| `println(1, -2, true, "s", 3.5, 1e100, 1.0)` | `1 -2 true s 3.5 1e+100 1\n` | 空格分隔；浮点 `%g`（精度 -1）；`1.0` ⇒ `1` |
| `print("a", 1, "b")` | `a1b` | 无分隔 |
| `println()` | `\n` | 空行 |
| `var p *int; println(p)` | `0x0\n` | nil 指针 |
| `var i I; println(i)` | `(0x0,0x0)\n` | nil 接口 |
| `var sl []int; println(sl)` | `[0/0]0x0\n` | nil slice |
| `println('c')` | `99\n` | rune 打十进制 |
| `println(uint64(18446744073709551615))` | `18446744073709551615\n` | 全宽 |
| `println(byte(255), int8(-1))` | `255 -1\n` | 宽度按静态类型 |

> **未冻结、RED 期补钉**（`R-4-1`）：`NaN` / `+Inf` / `-0` / `float32` 舍入 / 大 `rune`（> 0x10FFFF 非法）/ 字符串里的 `%`。

### 4.3 退出码表（`T-EX-49`）

| 情形 | 码 |
|---|---|
| 正常结束（含 `main` 返回） | 0 |
| 静态诊断（check 面） | 1 |
| HIR 未支持构造（`HIR-UNSUPPORTED-*`） | 1 |
| 参数错 / io 错 / 内部错 / 预算超限 / 未捕获 panic | 2 |

### 4.4 三件不承诺（`U-M5-02/03/04`，每条带正向证据）

| 项 | 不承诺什么 | 正向证据 |
|---|---|---|
| 地址数值 | `&x` 打印出的十六进制值 | `T-EX-46`：含地址输出的 fixture 被**显式排除**并留理由；探针塞进入选集必红 |
| map 迭代序 | 与官方随机序一致 | `T-EX-14`：`range` over map 只断言**集合相等**，不断言顺序；`T-EX-46` 有对应守卫 |
| panic traceback | `goroutine 1 [running]:` 及其后所有行 | `T-EX-38`：只断言 `panic: <值>` 前缀 + 退出码 2 |

---

## 5. 值语义 fixture 矩阵（甲腿的核心，`EX-3` 的判据）

| # | 语义 | 正向 fixture | 反向 fixture（必须**不**发生） |
|---|---|---|---|
| 1 | 数组拷贝 | `a := arr; a[0]=1; print(arr[0])` ⇒ 原值 | `a[0]=1` 影响 `arr` |
| 2 | 结构体拷贝 | 同上（字段） | 同上 |
| 3 | 数组取址后再拷贝 | `p := &arr[0]; b := arr; b[0]=2` ⇒ `*p` 仍原值 | `*p` 被 `b` 影响 |
| 4 | slice 别名 | `s2 := s1; s2[0]=1` ⇒ `s1[0]==1` | `s1[0]` 不变 |
| 5 | slice 增长后脱钩 | `s2 = append(s2, ...)` 超 cap ⇒ 改 `s2` 不影响 `s1` | 仍共享 |
| 6 | string 不可变 | `s2 := s1; s2 += "x"` ⇒ `s1` 不变 | `s1` 变 |
| 7 | map 引用 | `m2 := m1; m2[k]=v` ⇒ `m1[k]==v` | 不共享 |
| 8 | 指针写穿 | `*p = 9` ⇒ 原变量 9 | 只改副本 |
| 9 | 接口装箱拷贝 | `var i I = arr; arr[0]=1` ⇒ `i` 里仍是原值 | 接口看到新值 |
| 10 | 闭包按变量捕获 | 闭包内改，外部可见 | 捕获副本 |

---

## 6. 语料清单

### 6.1 候选（`run-candidates.txt`，**80**，现冻结）

**生成口径**（可重放，`T-EX-41` 断言）：`go_source_code/test/*.go` ∪ `test/ken/*.go` → 首 80 行内出现 `// run` 或 `// runoutput` → **零 import** → 不含 `go ` 语句 / `chan` / `select {` / `sync.` / `time.`。

**顶层 48**：

```text
alias1.go  align.go  bigmap.go  closure1.go  closure2.go  closure4.go
compos.go  const8.go  convert4.go  ddd.go  decl.go  defernil.go
divmod.go  escape3.go  float_lit.go  for.go  func.go  func6.go
func7.go  func8.go  gc1.go  helloworld.go  if.go  index0.go
indirect.go  initcomma.go  intcvt.go  iota.go  literal.go  method.go
method3.go  method5.go  method7.go  newexpr.go  nilptr2.go  print.go
printbig.go  range3.go  range4.go  reflectmethod2.go  rotate0.go  rotate1.go
rotate2.go  rotate3.go  simassign.go  turing.go  varinit.go  zerosize.go
```

**`ken/` 32**：

```text
array.go  complit.go  convert.go  cplx0.go  cplx1.go  cplx2.go  divmod.go
embed.go  for.go  interbasic.go  interfun.go  intervar.go  label.go  litfun.go
mfunc.go  ptrfun.go  ptrvar.go  range.go  rob1.go  robfor.go  robfunc.go
shift.go  simparray.go  simpbool.go  simpconv.go  simpfun.go  simpswitch.go
simpvar.go  slicearray.go  sliceslice.go  string.go  strvar.go
```

### 6.2 排除（`run-excluded.txt`，**6** 起步，现冻结）

| 文件 | 理由 | 指针 |
|---|---|---|
| `index0.go` | `runoutput`：跑第一段把输出写成第二段源码再编译运行 —— 动态编译，M5 不做 | `F5`、设计 §1.3 |
| `rotate0.go` | 同上 | `F5` |
| `rotate1.go` | 同上 | `F5` |
| `rotate2.go` | 同上 | `F5` |
| `rotate3.go` | 同上 | `F5` |
| `reflectmethod2.go` | `import "reflect"` —— 标准库 import 不在 M5 支持面（`U-M5-01`） | `F3-E`、`F17` |

### 6.3 入选（`run-selected.txt`，**P0 后追加冻结**，`R-6-1`）

**入选判据**（逐条全满足）：

1. 在候选内；
2. `rgoc check` **接受**（P0 收口后）；
3. 合并流**可复现**（不含地址数值、不依赖 map 迭代序）；
4. 只用 M5 支持面内的构造（无 chan/go/select/rangefunc/标准库）。

**预期量级**：74 个待定里，扣除「P0 修不完的规则」「不可复现输出」两类，预期 **≈70**。

**纪律**：入选数低于 60 时**如实报**（roadmap §M5：「数量不足不以复制用例凑数」），能力面靠甲腿 40 条自有 fixture 补足。

---

## 7. 白名单（`unsupported`，开工前冻结）

| ID | 项 | 为何不承诺 | 正向证据 | 计划阶段 |
|---|---|---|---|---|
| `U-M5-01` | 标准库 import（含 `reflect`） | roadmap §0.2：按明确包清单逐步加入 | `T-EX-61`：`reflectmethod2.go` 是唯一例外，且在排除清单 | 后续 wave |
| `U-M5-02` | 地址数值 | 不可复现（`F10` 实测） | `T-EX-46` | 不承诺 |
| `U-M5-03` | 未捕获 panic 的 traceback | 实现细节（`F18`） | `T-EX-38` | 不承诺 |
| `U-M5-04` | map 迭代序与官方一致 | 官方**故意随机** | `T-EX-14`（集合相等断言） | 不承诺 |
| `U-M5-05` | goroutine / channel / select / `go` | roadmap §M5「不做」 | `T-EX-02`：显式拒绝 | M8a–M8c |
| `U-M5-06` | `range` over func（`PIPE-16b`） | roadmap §M5 第 5 条 | `T-EX-02`：显式拒绝 | M8d |
| `U-M5-07` | `unsafe` 的执行面 | M4 白名单只到静态求值 | M4 的 `U-M4-06` 承接；执行面无触发者 | M6+ |
| `U-M5-08` | `panic(nil)` 的 Go 1.21+ 语义 | 我们简化为 `panic: nil` | `T-EX-38` 的一个 case | 不承诺 |
| `U-M5-09` | 求值顺序（`EXP-27`）的 codegen 面 | roadmap §5 归属 | 解释器顺序由 `T-EX-09/18` 覆盖 | M6 |
| `U-M5-10` | 泛型字典/shape 共享（`RT-POLY-05`） | 只做单态化 | `T-EX-32/33` | M12 |
| `U-M5-11` | `print/println` 的**非法操作数**拒绝（M4 缺口，`F11`） | 拒绝面缺口，不阻塞执行 | `F11` 实测；登记为 M4 backlog | M4 收口 |
| `U-M5-12` | `close`（chan 内建） | 依赖 `U-M5-05` | `T-EX-02`：显式拒绝 | M8b |

> **白名单纪律**（沿 `M3-tests.md` §6）：跳过项**不得**记为 pass；每项有**正向断言**；`manifest_m5.rs` 对账。

---

## 8. 门禁（`EX-1..EX-9`，定稿）

| 门禁 | 名称 | 判据 | 测试 ID |
|---|---|---|---|
| `EX-1` | HIR 与 lowering 闭环 | 节点穷尽 + 未支持显式拒绝 + 诊断一致性守卫 + dump 确定性 | `T-EX-01..06` |
| `EX-2` | 函数、帧与初始化 | 递归 1000 层 / 多值 / 变参 / 命名返回 / 单包 init 序 / 双包 init 序 | `T-EX-17..21`、`39/40` |
| `EX-3` | 复合值与内建 | §5 的 10 组正反 + 内建全集 + `print` 格式表 | `T-EX-10..16`、`23/24` |
| `EX-4` | 闭包与接口 | 捕获 / 生命周期 / loopvar / 方法值 / typed nil / 断言 / type switch / 泛型 | `T-EX-25..33` |
| `EX-5` | defer/panic/recover | 时机 / LIFO / 直接调用限制 / 命名返回值 / 嵌套 / 未捕获 | `T-EX-34..38` |
| `EX-6` | 顺序语料闭环 | 两清单守卫 + 入选全绿 + 三方差分 + ken 专项 + 确定性 | `T-EX-41..48` |
| `EX-7` | 显式帧与资源上限 | 三道上限响亮失败 + 1000 层递归通过（语义不依赖 Rust 栈） | `T-EX-52`、`17` |
| `EX-8` | 四条 cargo + 既有自检 | 四条 exit 0 + 五条自检 + **M4 的 63 条不回退** | `T-EX-53/54/62` |
| `EX-9` | 反向验证 + 交付 B | 8 条变异实证变红并复原 + `M5-report.md`（措辞为「候选」） | `T-EX-63..70` |

---

## 9. 验证命令与预算

### 9.1 四条 cargo（`R-9-1` 生效形态，沿 M4）

```sh
scripts/in-container.sh bash -lc 'cd rgoc && cargo fmt --all --check'
scripts/in-container.sh bash -lc 'cd rgoc && cargo check --workspace --all-targets'
scripts/in-container.sh bash -lc 'cd rgoc && cargo clippy --workspace --all-targets -- -D warnings'
scripts/in-container.sh bash -lc 'cd rgoc && cargo test --workspace --no-fail-fast'
```

> ⚠️ **裸形态跑不通**（`M4-tests.md` `R-9-1`）：容器 `-w /work`，workspace 在 `rgoc/`。必须 `bash -lc 'cd rgoc && …'`。

### 9.2 对撞形态

```sh
# 单文件（开发期）
scripts/in-container.sh bash tools/run-oracle/run-collide.sh --run go_source_code/test/helloworld.go

# 入选清单（门禁）
scripts/in-container.sh bash tools/run-oracle/run-collide.sh --run --list tools/run-oracle/run-selected.txt go_source_code/test

# ken 专项
scripts/in-container.sh bash tools/run-oracle/run-collide.sh --run --list tools/run-oracle/run-ken.txt go_source_code/test

# HIR dump（对撞/调试）
scripts/in-container.sh bash -lc 'cd /work && rgoc/target/debug/rgoc-driver run --dump-hir go_source_code/test/helloworld.go'
```

### 9.3 预算

| 层级 | 上限 | 说明 |
|---|---|---|
| 单文件（rgoc + go run 两侧） | ≤ 10 s | 含 `go run` 的编译时间（冷缓存另计） |
| smoke | ≤ 5 min | 甲腿 40 条 fixture + 5 个语料样本 |
| milestone | ≤ 30 min | 入选清单全量（≈70）× 两侧 |
| full | ≤ 2 h | 四条 cargo + 全部对撞形态 + 五条自检 |

> M4 的实测（`M4-report.md` §10.4）显示这些上限有**一个数量级**的余量；M5 的瓶颈预期在 `go run` 的编译开销，不在解释执行。

---

## 10. 与 manifest 的接口

`M5-manifest.json` 必须能由本文件**机械推导**：

| manifest 字段 | 来源 |
|---|---|
| `test_ids` | §3 的 70 条（含 `leg` 腿标记） |
| `gate.EX-*` | §8 表 |
| `unsupported` | §7 表（12 条） |
| `corpus_lists` | §6（候选 80 / 排除 6 / 入选待冻结） |
| `coverage` | §2 两张矩阵 |
| `budget` | §9.3 |

`manifest_m5.rs` 的对账断言（`T-EX-55`）：三类谎必须被拦 —— ① 标 `pass` 却在证据里自认未兑现；② 非 `pass` 却没有 `open_gaps` 登记；③ `phase_summary` 自称 `done` 而门禁没全绿。

---

## 11. 纪律（不是建议）

1. **不许放宽判据换绿**：逐字节就是逐字节；`\r\n` 是唯一容差。
2. **不许静默移出**：候选 → 排除只能按 §6.3 的四条判据，逐行理由 + 修订 + 报告。
3. **不许把「能跑」当「语义对」**：`hello` 通过 ≠ 值模型对；甲腿的 10 组正反才是 `EX-3` 的判据。
4. **不许用 Rust 递归承载语义**（`D-M5-1`）：`T-EX-17` 的 1000 层是硬判据。
5. **不许跳过白名单项**：`unsupported` 每条有正向断言，`manifest_m5.rs` 会拦。
6. **不许改 M4 的判据来让 P0 好看**：P0 修的是**接受面**；M4 的 `CH-2` 判的是**拒绝面**，两者不冲突，**互不修改**。

---

## 12. 修订表

见 §0 的修订表（追加式）。当前两条：`R-6-1`（入选清单分两次冻结）、`R-4-1`（浮点边界补钉）。
