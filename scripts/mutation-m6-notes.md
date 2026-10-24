# M6 变异探针记录

> 沿 `mutation-m3-notes.md` / `-m4-` / `-m5-` 的格式：**每条变异必须实证变红并复原**。
> 变红的判据是「**指定**测试出现在 failures 里」，不是「随便哪条红了」。
>
> 环境纪律（`M0-benchmarks.md` §6）：变异前 `cargo clean -p <crate>`；
> 变异脚本用「进程内保存原文 → 改写 → 跑 → 写回」，**不用 `git checkout`**
> （工作区里常有未提交改动，checkout 会连真改动一起抹掉）。

## Task 17–29（摘要）

明细在台账 `.superpowers/sdd/M6-plan/progress.md`（`.superpowers/` 不入库，属过程材料）。
累计：Task 22 四条 / 24 六条 / 23 五条 / 25 五条 / 26–29 五条，全部实证变红。
其中一条**第一版断言抓不到**（变参 dots 也打包：1 个 slice 包成 1 个 slice 仍是 1 个实参），
改成「数 `Composite` 个数」才变红 —— 记录在案，提醒「断言必须能反向失败」。

## Task 30：指针、取址与写穿（6 条，全红）

脚本 `.superpowers/sdd/M6-plan/mut30.py`（同上：进程内复原）。

| 变异 | 改动 | 期望红在 | 实测 |
|---|---|---|---|
| `M1` 写穿不用根记录 | `write_place` 的 `Some(root) => store_root(...)` 改成扇出 | `t_ssa_25_写穿之后读同一个槽走同一条_mem_链` | ✅ 红 |
| `M2` 字段地址不传根 | `place_addr_root(Field)` 返回 `None`（丢根） | `t_ssa_25_写穿结构体字段` | ✅ 红 |
| `M3` 数组下标底座求成值 | `index_base` 的数组槽分支短路 ⇒ 底座变成 `Load` 出的数组**值** | `t_ssa_25_数组元素写落在数组槽根上` | ✅ 红 |
| `M4` 转换丢根 | `coerce_to` 不把 `ptr_hint` 传给 `Convert` 的结果 | `t_ssa_25_写穿结构体字段` | ✅ 红 |
| `M5` 读指针一律堆根 | `expr(Deref)` 不看 `ptr_hint` | `t_ssa_25_写穿之后读同一个槽走同一条_mem_链` | ✅ 红 |
| `M6` 未知根只写堆 | `store_to_unknown` 只发一条堆根 Store | `t_ssa_25_根未知的指针写扇出到所有根` | ✅ 红 |

### 这一批抓到的两个设计缺口（都是先有变异、后有判据）

1. **`Field`/`Index` 的结果也是地址，也必须带根**。初版只有 `AddrOf` 记根，
   于是 `p := &s.n` 一过 `Convert`（`*named` → `*int`）就「失忆」，
   写穿退化成扇出（实测 dump 里两条 Store）。修法：`note_root` 三处都记 +
   `coerce_to` 传根（指针转换不动地址）。
2. **「一条 Store」不是判据**：槽初始化本来就有一条 Store（`a := 1` 落进根），
   数总数会把正确实现判红。判据改成「**全部** Store 都落在同一个根」+「读用的是
   **最后一条**的 mem」—— 前者排除扇出，后者才是 `T-SSA-25` 要的「同一条链」。

## Task 31：拒绝面（3 条，全红）

脚本 `.superpowers/sdd/M6-plan/mut31.py`。

| 变异 | 改动 | 期望红在 | 实测 |
|---|---|---|---|
| `M7` defer 分支静默放行 | `unsup_stmt` 的 `Stmt::Defer` 返回 `None` | `t_ssa_33_defer` | ✅ 红 |
| `M8` map 类型不判 | `face_kind` 的 `Type::Map` 返回 `None` | `t_ssa_33_map` | ✅ 红 |
| `M9` `ALL` 少一个码 | `SsaCode::ALL` 里换重复项 | `码总数与字典序自洽` | ✅ 红 |

## Task 31b：语料实测抓到的 5 个构建错（全红）

脚本 `.superpowers/sdd/M6-plan/mut31b.py`。这一批不是「设计推断的变异」，
而是**先跑 69 行语料见到真错**（6 个 panic + 12 个 `V3`），修完再用变异锁住。

| 变异 | 改动 | 期望红在 | 实测 |
|---|---|---|---|
| `M10` 复合赋值读地址类型 | `read_place` 回退到 `values[addr]` | `t_ssa_25_复合赋值读数组元素要按值类型建_load` | ✅ 红 |
| `M11` 运算操作数不对齐 | `Binary` 的算术分支不 `coerce_to` | `t_ssa_15_未定型常量参与运算要就地转成目标类型` | ✅ 红 |
| `M12` `range` 上界用 `int` | `coerce_to(key_ty, xs)` → `int_ty` | `t_ssa_17_range_具名下标的类型跟槽走` | ✅ 红 |
| `M13` 间接调用只给一个结果 | `func_result_types` → `vec![e.typ]` | `t_ssa_22_间接调用的结果个数取自函数类型` | ✅ 红 |
| `M14` 预扫描不扫被调方 | `Callee::Value(_)` 只扫实参 | `t_ssa_33_立即调用的函数字面量` | ✅ 红 |

### 语料实测本身抓到的六个真错（清单）

1. `read_place` 拿**地址值类型**（`*[30000]uint8`）当 `Load.typ` ⇒ `a[pc] += 1` 的 `bin add` 被 `V3` 拒（`turing.go`）。
2. 二元运算的操作数**没有物化隐式转换**（`int` 与 `untyped int`）⇒ (A) 多处 `V3`。
3. `range` 上界/自增用 `int`，而下标槽可能是具名类型 ⇒ `lt MyInt int`（`range3.go`）。
4. `range` 上界是 `untyped int` 常量 ⇒ `lt int untyped-int`（`range3.go` / `turing.go`）。
5. 间接调用的 `dsts` 只给一个 ⇒ `b, x := f(1000, 99)` panic（`method5.go`）。
6. 预扫描漏扫 `Callee::Value` 的**被调方** ⇒ `func(){}()` 落进 `todo!()`（5 个文件 panic）。

另修一条 `typing` 判据：常量 `Float`/`Complex` 不认**未类型化**的 `untyped float/complex`
（`float_lit.go` / `ken/cplx*.go` 一片假拒绝），并让 `basic()` 走 `underlying`（具名数值类型）。

## Task 32b：合成包初始化函数（4 条，全红）

脚本 `.superpowers/sdd/M6-plan/mut32b.py`。

| 变异 | 改动 | 期望红在 | 实测 |
|---|---|---|---|
| `M15` 不合成 init | `build()` 不 push 合成函数 | `t_ssa_36_包级初始化式建出_init_函数` | ✅ 红 |
| `M16` 不写初始化式 | `build_init` 里 `vs` 恒空 | `t_ssa_36_包级初始化式建出_init_函数` | ✅ 红 |
| `M17` 不扫初始化式 | `unsupported_init` 恒 `None` | `t_ssa_36_初始化式里的不支持面要带码拒绝` | ✅ 红 |
| `M18` 不调用文件的 `init` | 循环里 `continue` | `t_ssa_36_文件的_init_被合成_init_调用` | ✅ 红 |

## Task 32：`ssa_exec` 骨架（3 条，全红）

脚本 `.superpowers/sdd/M6-plan/mut32.py`。

| 变异 | 改动 | 期望红在 | 实测 |
|---|---|---|---|
| `M19` 不查步数上限 | `steps >= max_steps` → `if false` | `t_ssa_32_步数上限要能停住` | ✅ 红 |
| `M20` 缺 `main` 假装成功 | `Err(Halt::NoMain)` → `Ok(())` | `t_ssa_32_没有_main_要报错而不是假装成功` | ✅ 红 |
| `M21` `Return` 当不可达 | `Term::Return` → `Halt::Unreachable` | `t_ssa_32_空函数立即返回` | ✅ 红 |

## Task 33b：`&&` / `||` 短路（5 条，全红）

脚本 `.superpowers/sdd/M6-plan/mut33b.py`。

| 变异 | 改动 | 期望红在 | 实测 |
|---|---|---|---|
| `M22` 短路退回一条 `bin` | `expr` 的 `LAnd/LOr` 分支建 `Bin` | `t_ssa_33b_and_建成控制流而不是一条_bin` | ✅ 红 |
| `M23` 两支不对调 | `(t, f)` 恒为 `(rhs_b, short_b)` | `t_ssa_33b_or_的短路出口是_true_且两支对调` | ✅ 红 |
| `M24` 出口值反了 | `const_bool(!is_and)` → `const_bool(is_and)` | `t_ssa_33b_and_建成控制流而不是一条_bin` | ✅ 红 |
| `M25` 汇合处不带上 mem | 右支 `BranchOut.mem` 清空 | `t_ssa_33b_右支里的访存要在汇合处合上` | ✅ 红 |
| `M26` 出口常量用缓存 `bool` | `e.typ` → `self.bool_ty()` | `t_ssa_33b_短路出口要跟结果类型而不是缓存_bool` | ✅ 红 |

### 语料实测抓到的第七个真错

`&&` 的结果类型可能是 **`untyped bool`**（`x < 3 && y` 的左操作数是比较表达式，
Go 规范说比较产生 untyped boolean）。短路支的常量若用缓存的 `bool`，
汇合块两条边就是两种类型 ⇒ `V3` 拒 **5 个语料文件**（`float_lit.go` / `simassign.go` /
`ken/array.go` / `ken/divmod.go` / `ken/string.go`）。修法：`const_bool(b, t)` 由调用者给类型。

## Task 33：纯值求值（3 条，全红）

脚本 `.superpowers/sdd/M6-plan/mut33.py`（端到端测试在 `rgoc-driver/tests/ssa_run.rs`）。

| 变异 | 改动 | 期望红在 | 实测 |
|---|---|---|---|
| `M27` 比较恒 `false` | `ops::compare` → `false` | `t_ssa_33_一元运算` | ✅ 红 |
| `M28` 转换的源类型传错 | `convert(tt, v, typ, typ)` | `t_ssa_33_转换与浮点` | ✅ 红 |
| `M29` `println` 分隔符错 | 空格 → 逗号 | `t_ssa_33_短路的值` | ✅ 红 |

### 顺手把三处 M5 代码抽成可复用

`apply_unary` / `const_value` / `nil_of` / `convert` / `slice_value` 提到 `pub(crate)`，
其中 `convert` 与 `slice_value` 原本是 `Interp` 的方法（只用到 `self.tt`）——
改成自由函数后两套求值器共用**同一份**语义（`D-M6-7` 的要求）。

## Task 34：控制流与块参数（3 条，全红）

脚本 `.superpowers/sdd/M6-plan/mut34.py`。

| 变异 | 改动 | 期望红在 | 实测 |
|---|---|---|---|
| `M30` 边算边写（破坏并行） | `jump` 改成「逐个读→逐个写」 | `t_ssa_34_并行赋值交换` | ✅ 红 |
| `M31` `if` 恒走 then | `cond` → `true` | `t_ssa_34_if_else_分支` | ✅ 红 |
| `M32` `Panic` 不终止 | `Term::Panic` → 弹帧 | `t_ssa_34_panic_走合并流且退出码_2` | ✅ 红 |

### 顺手补的构建期缺口

`panic(v)` 原先建成 `Call Builtin(Panic)`（一条普通调用），于是「panic 之后的语句」
仍被建进同一个块 —— 图上看不出这里断了，可达性分析也跟着错。
现在 `Stmt::Expr` 特判：`panic(v)` ⇒ `Term::Panic(v)`（`Term::Panic` 本来就是为它准备的）。
运行期另留一条兜底（漏网的 `Builtin(Panic)` 仍按 Go 语义终止），双保险。

## Task 35：访存、调用与内建（11 条，全红）

脚本 `.superpowers/sdd/M6-plan/mut35.py`。

| 变异 | 改动 | 期望红在 | 实测 |
|---|---|---|---|
| `M33` 根存储不建零值 | `frame_roots` 的 Slot 分支恒 `Nil` | `t_ssa_35_数组与结构体字段` | ✅ 红 |
| `M34` 两处拷贝都去掉 | `Load` 与 `Store` 的 `deep_copy` 都改浅 | `t_ssa_35_数组赋值是拷贝` | ✅ 红 |
| `M35` 取字段不导航 | `field_cell` 返回底座自身 | `t_ssa_35_方法调用_指针接收者写回` | ✅ 红 |
| `M36` 返回不交接结果 | `Return` 的 `ret_to` 循环换成空迭代 | `t_ssa_35_函数调用与递归` | ✅ 红 |
| `M37` 方法不补接收者 | `Method` 的实参表不加 `recv` | `t_ssa_35_方法调用_指针接收者写回` | ✅ 红 |
| `M38` `append` 忽略 `dots` | `extra` 恒按「单个值」取 | `t_ssa_35_append_展开` | ✅ 红 |
| `M39` nil 解引用不报错 | `as_ptr` 的 `Nil` 返回一个临时单元 | `t_ssa_35_nil_指针解引用` | ✅ 红 |
| `M40` switch 判定不短路 | 非末个 case 表达式的 else 落到 case 体 | `t_ssa_35_switch_的_case_表达式逐个短路` | ✅ 红 |
| `M41` range 计数器用用户槽 | `idx_slot` 退回用户的下标槽 | `t_ssa_35_range_结束后下标是最后一个` | ✅ 红 |
| `M42` 不预扫描 goto 目标 | `goto_targets` 恒空 | `ssa_corpus_run`（`ken/label.go`） | ✅ 红 |
| `M43` 指针底座取成变量地址 | 删掉 `expr_addr_root` 的指针早分流 | `t_ssa_35_通过指针读字段` | ✅ 红 |

### `M34` 为什么必须「两处一起去掉」

`Load` 与 `Store` 各有一处 `deep_copy`，**互为冗余**：单独去掉任一处，另一处
仍然把拷贝做了（第一次跑变异时 `M34` 单点去掉是**绿的**，于是改成两处一起去）。
根因是「元素/字段写」会把槽标成根（`partial`），于是赋值必然走 `Store`、
读必然走 `Load` —— 两条路都过拷贝。判据因此写成端到端的值语义
（`b := a` 之后改 `b` 不影响 `a`），而不是「某一行必须拷贝」（`R-35-6`）。

## Task 36/37：预算与 `--via`（3 条，全红）

脚本 `.superpowers/sdd/M6-plan/mut36_37.py`。

| 变异 | 改动 | 期望红在 | 实测 |
|---|---|---|---|
| `M44` 预算不给诊断 | `Halt::Budget` 的 `diag` 置 `None` | `t_ssa_40_死循环按预算停下并给出原因` | ✅ 红 |
| `M45` `--via` 解析恒 `hir` | `Via::parse("ssa")` 返回 `Hir` | `t_ssa_39_解析_run_的_via` | ✅ 红 |
| `M46` 拒绝面当成功 | `if false && !rejected.is_empty()` | `t_ssa_39_via_ssa_的拒绝面退出码是_1` | ✅ 红 |

### 帧深守卫为什么没有变异

把 `push_frame` 的 `frames.len() >= max_frames` 去掉，测试不是「变红」而是**吃光内存**
（`main` 调自己会一直压帧，预算 1e8 步也拦不住 1e8 个帧）。一条会 OOM 的变异
没法当判据，所以帧深守卫用**直接测试**钉（`t_ssa_40_无限递归按帧深上限停下`：
退出码 2 + 诊断里有 1000）。这条偏差记在这里，不假装有变异覆盖。

## Task 38/39：三方差分与切片冻结（2 条，全红）

脚本 `.superpowers/sdd/M6-plan/mut38_39.py`。这两条是**数据变异**（改
`tools/run-oracle/ssa-rejected.txt` 一行），不重编译 —— 打的就是「清单与实测脱节」。

| 变异 | 改动 | 期望红在 | 实测 |
|---|---|---|---|
| `M47` 删掉一行拒绝记录 | 清单少 `closure4.go` | `ssa_slice::t_ssa_31_切片冻结值两向自洽` + `…清单与实测一致` | ✅ 红 |
| `M48` 把码改错 | `closure4.go` 的 `-DEFER` 改成 `-MAP` | 同上（码不一致） | ✅ 红 |

### 三方差分的实测（脚本腿）

```
bash tools/run-oracle/run-collide.sh --run --list --ssa --three-way tools/run-oracle/run-selected.txt
---- 69 个文件：绿 46 / 跳过 23 / 红 0（hir vs ssa vs go run）
---- 三方比对：5 个文件有 .out（其余 64 个按「期望为空」只做两方）
```

`SKIP` 的判据是「stderr 里有 `SSA-UNSUPPORTED-*`」**而不是**「退出码 1」：
后者会把任何静态错都说成「它被拒了」，装置等于没有。

## Task 40–44：四个 pass 与它们依赖的两条不变量（7 条，全红）

脚本 `.superpowers/sdd/M6-plan/mut40_44.py`。

| 变异 | 改动 | 期望红在 | 实测 |
|---|---|---|---|
| `M49` `copy` 不解析链 | 去掉 `while map.get` 的链式解析 | `opt_copy::62_链式_copy_一路消到终点` | ✅ 红 |
| `M50` `dce` 删 `Store` | `Store` 的 `keep` 返回 `false` | `opt_dce::61_没用的_store_必须留着` | ✅ 红 |
| `M51` `fold` 除零也折 | `Div` 的 `b == 0` 返回 `Some(0)` | `opt_fold::60_除零不折` | ✅ 红 |
| `M52` 压实不同步 `params` | 删掉 `f.params = …` 那一行 | `ssa_cli::39_两条通路在正常程序上输出一致` | ✅ 红 |
| `M53` `dce` 把 `prints` 当纯 | `pure_effect` 不看 `prints` | `opt_equiv::64_全开与全关在入选集上输出一致` | ✅ 红 |
| `M54` 摘要不认引用型槽 | `owner_of_expr` 的 `Slot` 分支恒 `Local` | 同上（`ken/array.go`） | ✅ 红 |
| `M55` `cse` 跨块也合 | `seen` 提到块循环外 | `opt_cse::63_跨块不合` | ✅ 红 |

### `M54` 第一次打偏了

初版把锚点放在 `owner_of_place` 的 `Place::Slot` 分支 —— 变异**全绿**。原因：
`s[i] = v` 的归属走的是 `owner_of_place(Index)` → `owner_of_expr(base)`，真正
要看类型的那个函数是 `owner_of_expr`。改成打后者才红。

这条偏差说明一件事：**「我改了某个判断」不等于「变异打到了那个判断」**。
变异全绿时先怀疑锚点，再怀疑用例。

## Task 45/46/48：活跃性与 `SSA-LIVE v1`（11 条，全红）

脚本 `.superpowers/sdd/M6-plan/mut45_48.py`。

| 变异 | 改动 | 期望红在 | 实测 |
|---|---|---|---|
| `M56` 块参数不算已定义 | `defined.insert(*p)` 去掉 | `t_ssa_55_直线_…`（`live_in[b1]` 会多出块参数） | ✅ 红 |
| `M57` `live_in` 不减 `def` | `inn.extend(out)` 不过滤 `def_b` | `t_ssa_55_直线_…`（`live_in[b0]` 会多出 v0/v1） | ✅ 红 |
| `M58` 不迭代到不动点 | 单轮 + 正序（两处一起改） | `t_ssa_55_循环_回边上变量活跃` | ✅ 红 |
| `M65` `live_out` 不含边实参 | 删掉 `out.extend(cfg.edge_args…)` | `t_ssa_55_直线_…` | ✅ 红 |
| `M67` safepoint 判定恒假 | `Instr::Call { .. } => false` | `t_ssa_56_协议字段自洽`（一条 safepoint 都没有） | ✅ 红 |
| `M59` safepoint 不并入自身操作数 | 删掉 `set.extend(ins.uses())` | `t_ssa_56_协议字段自洽`（实参 v0 不在 live 里） | ✅ 红 |
| `M60` `ptrs` 不排 mem 令牌 | 去掉 `!mem_values.contains(v)` | `t_ssa_56_协议字段自洽`（ptrs 变 `[1,2]`） | ✅ 红 |
| `M61` 不做根闭包 | 删掉闭包两行 | `t_ssa_58_派生指针活跃则根也活跃` | ✅ 红 |
| `M62` 区间 `last` 不更新 | `touch` 体置空 | `t_ssa_58_…`（v1 的 last 停在定义点） | ✅ 红 |
| `M63` safepoint 不按块序 | 块循环加 `.rev()` | `t_ssa_59_两次输出逐字节相同` | ✅ 红 |
| `M64` 头行 `values=` 写块数 | 两个字段对调 | `t_ssa_56_协议字段自洽` | ✅ 红 |

### `M58` 为什么必须两处一起改

只去掉不动点（`while changed` → 单轮）而保留**逆序**遍历，我的循环 fixture
恰好一轮就收敛（b3 → b2 → b1 → b0 正好是信息流方向）—— 变异是**绿的**。
加上「改成正序」才红。这条偏差记下来：**fixture 的块序本身在帮逆序遍历收敛**，
将来换 fixture 要重新确认这一点。

### 一条诚实的例外：`− params(s)` 打不掉

`live_out` 公式里的 `− params(s)` 在当前 use/def 约定下**永远不起作用**
（块参数算「入口已定义」⇒ `live_in[s]` 里没有它）。所以没有变异能杀掉它，
已在 `R-45-2` 里登记为冗余，不假装有覆盖。

## Task 49：`T-SSA-47..54` 八条（反腿清单，8/8 全红）

脚本 `.superpowers/sdd/M6-plan/mut49.py`。这八条是 `M6-tests.md` §3.5 点名的探针；
其中四条在各自 Task 里已经打过（`T-SSA-51`≈`M55`、`T-SSA-52`≈`M30`、`T-SSA-49`/`50` 有近似版），
这里按清单**重打一遍**，因为收口要能回答「§3.5 那八条都真的红过」。

| 探针 | 改动 | 红在 |
|---|---|---|
| `T-SSA-47` | `V7` 的块参数计数检查 → `if false` | `verify_negative::t_ssa_08_v7_边实参比块参数少` |
| `T-SSA-48` | `check_v2` 开头 `return Ok(())` | `verify_negative::t_ssa_03_v2_*`（三条全红） |
| `T-SSA-49` | `store_root` 不前进 `self.mem` | `ssa_build::写穿之后读同一个槽走同一条_mem_链` |
| `T-SSA-50` | `Return` 只交接第一个结果 | `ssa_run::多返回值与结果直连` |
| `T-SSA-51` | `cse` 的 `seen` 提到块外 | `opt_cse::跨块不合` |
| `T-SSA-52` | `jump` 改成「读一个写一个」 | `ssa_exec::回边上对调两个块参数` |
| `T-SSA-53` | 分类器把被拒文件也算进「能建图」 | `ssa_slice::清单与实测一致` |
| `T-SSA-54` | dump 的类型行改按 `HashMap` 迭代序 | `dump::同一输入三次_dump_逐字节相同` |

### ⚠️ 一条旧记录被推翻（`M30`）

`T-SSA-52` 第一次打时，我按 `M30` 的老判据指向 `ssa_run::并行赋值交换` —— **绿的**。
查下去发现：端到端的 `a, b = b, a` **根本不经块参数**，HIR 靠「改名」解决，
dump 里那条边交的是 latch 自己的参数（`goto b4 (v4,v3,v5)`）：

```text
b2: term goto b4 (v4,v3,v5)      ← 交的是 b4 自己的参数，不是 b1 的
```

所以 `M30`（Task 34）当时「红在 `并行赋值交换`」这一条**今天无法复现**（锚点也已随
Task 35 的重构失效，脚本报 `SETUP-FAIL`）。两种可能：当时的代码路径不同，或当时
记录写错了。**不猜**，按今天的实测更正：

- `jump` 的并行语义在**本前端**只有手搓 IR 打得到（回边把两个块参数对调）；
- 已补 `ssa_exec::t_ssa_52_回边上对调两个块参数`（三轮交换 ⇒ `2 1`；顺序赋值会得 `2 2`）；
- 端到端的 `并行赋值交换` 保留（它证明的是「改名方案正确」），但**不再声称**它覆盖 `jump`。
