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
