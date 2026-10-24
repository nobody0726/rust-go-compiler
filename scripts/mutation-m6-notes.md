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
