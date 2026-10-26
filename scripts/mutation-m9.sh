#!/bin/bash
# scripts/mutation-m9.sh —— M9 的变异探针（`M9-plan.md` Task 70）。
#
# 用法（容器内）：bash scripts/mutation-m9.sh
#
# # 为什么变异必须实证
#
# 「测试全绿」有两种可能：**代码对**，或者**判据恒真**。区分它们的唯一办法是
# 把代码改坏一处，看判据会不会红。本仓已经抓到过 6+ 条恒真断言（`M3`/`M4` 的账），
# M9 这一轮又抓到两条（见 `mutation-m9-notes.md` §3）。
#
# # 纪律（M6 的血）
#
# 1. **变异前 `cargo clean -p <crate>`**：绑定挂载上 cargo 的 mtime 判据不可靠，
#    不 clean 会跑到旧产物上，得到「绿色」的假结论。
# 2. **同一调用内完成变异与恢复**：容器 `/tmp` 调用即焚，跨调用恢复不了。
# 3. 每条都必须看到 `test result: FAILED` 才算「变异有效」；全绿 = 判据没牙。
# 4. **锚点也要核**：M9 实测踩过一次 —— 按 `void rt_init(void *sp)` 改 `rt.c`，
#    而实际签名是 `void rt_init(u64 sp)`，替换没命中、测试照绿。所以 `probe`
#    里「找不到就 MISS」，MISS 也算失手。
# 5. **恢复源码之后必须再 clean 一次**（见脚本末尾）：否则 `.rlib` 还是变异版，
#    下一次全量测试会红一批与变异无关的用例。
#
# # 这里 8 条探针
#
# 另外两条（05 ITab 填序 / 09 `ptrs` 退回值粒度 / 10 回边不产 safepoint）
# 的实测记录在 `scripts/mutation-m9-notes.md`：05 要跑 69 个语料（~25 s，
# 回归代价高），09/10 的判据在 `rgoc-ssa` 的单测里，没必要每次全跑。
#
# **收口补的四条**（04 / 06 / 07 / 11）原来都是「笔记里绿的那三条」——
# 它们绿是因为**判据缺了形态**，不是代码对。补上判据之后它们才红，于是
# 进脚本当回归守卫（详见 `mutation-m9-notes.md` §三）。
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/rgoc" || { echo "进不去 $ROOT/rgoc" >&2; exit 2; }

pass=0
fail=0
report=""

# 用法：probe <名字> <文件> <旧串文件> <新串文件> <crate|-> [cargo test 参数…]
# 用「文件传串」而不是把串写进 shell：带引号/换行的 Rust/C 代码在 shell 里
# 转义一次错一次（M6 踩过）。`crate` 传 `-` ⇒ 不 clean（`rt.c` 不进 cargo 的
# 依赖图，每次 build 都会重编）。
probe() {
  local name="$1" file="$2" oldf="$3" newf="$4" crate="$5"
  shift 5
  cp "$file" "$file.m9bak"
  if ! python3 - "$file" "$oldf" "$newf" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
s = p.read_text(encoding="utf-8")
old = pathlib.Path(sys.argv[2]).read_text(encoding="utf-8")
new = pathlib.Path(sys.argv[3]).read_text(encoding="utf-8")
if old not in s:
    sys.exit(3)
p.write_text(s.replace(old, new, 1), encoding="utf-8")
PY
  then
    mv "$file.m9bak" "$file"
    touch "$file"
    report="$report
  MISS  $name  <<< 锚点没命中（改的那段代码已经变了）"
    fail=$((fail + 1))
    return
  fi

  if [ "$crate" != "-" ]; then
    cargo clean -p "$crate" >/dev/null 2>&1
  fi
  local out
  out="$(cargo test "$@" 2>&1 | grep -E '^test result' | tail -1)"
  mv "$file.m9bak" "$file"
  # ⚠️ 恢复之后**必须** `touch`：`mv` 保留了 `.m9bak` 的 mtime（创建于变异**之前**），
  # 而变异期间产生的 `.rlib` 的 mtime 比它新 ⇒ cargo 认为源码没变、继续用
  # **变异版**的 rlib。M9 收口实测踩到：探针全红，紧接着的全量测试在
  # `unsafe_ptr` 上报 `V3` —— 那是变异版的 `rgoc-check` 还在产物里。
  touch "$file"
  if printf '%s' "$out" | grep -q 'FAILED'; then
    report="$report
  RED   $name  [$out]"
    pass=$((pass + 1))
  else
    report="$report
  GREEN $name  [$out]  <<< 判据没牙"
    fail=$((fail + 1))
  fi
}

TMP="$(mktemp -d)"

# ── M-M9-01：`needs_zero` 恒 false ⇒ `rt_alloc` 不清零 ────────────────
#
# 判据 `T-MEM-03`（`mem_alloc::含指针的类型要清零`）：`new(S)`（S 含 `*int`
# 与 `float64`）必须全零，含 `0.0` 的**位模式**。不清零的话 `float64` 的
# 垃圾位会让「值正确」当场红 —— 而零值恰好是 0 的整数永远是绿的，所以
# 那条判据必须带浮点（这是它自己的设计）。
printf '%s' 'pub fn needs_zero(tt: &TypeTable, t: TypeId) -> bool {
    has_pointers(tt, t) || has_float(tt, t)
}' > "$TMP/a_old"
printf '%s' 'pub fn needs_zero(tt: &TypeTable, t: TypeId) -> bool {
    let _ = (tt, t);
    false
}' > "$TMP/a_new"
probe "M-M9-01 needs_zero 恒 false（不清零）" \
  crates/rgoc-meta/src/desc.rs "$TMP/a_old" "$TMP/a_new" \
  rgoc-meta -p rgoc-driver --test mem_alloc

# ── M-M9-02：`ptr_mask` 对 `string` 只留 1 个字 ──────────────────────
#
# 判据 `T-META-04`（`ptr_mask::五个特例的位图与_design_一致`）：`string` 是
# `[1,0]` —— **两个字**、只有字 0 是根。截成 1 个字的话「len 那个字」根本不
# 存在，扫描器会把 `string` 当 `*byte`（`len` 被当地址追）。
printf '%s' '        Type::Basic(b) => {
            if matches!(
                b,
                BasicId::String | BasicId::UntypedString | BasicId::UnsafePointer
            ) {
                out[0] = true;
            }
        }' > "$TMP/b_old"
printf '%s' '        Type::Basic(b) => {
            if matches!(
                b,
                BasicId::String | BasicId::UntypedString | BasicId::UnsafePointer
            ) {
                out[0] = true;
                if matches!(b, BasicId::String) {
                    out.truncate(1);
                }
            }
        }' > "$TMP/b_new"
probe "M-M9-02 ptr_mask 对 string 只留 1 字" \
  crates/rgoc-types/src/layout.rs "$TMP/b_old" "$TMP/b_new" \
  rgoc-types -p rgoc-types --test ptr_mask

# ── M-M9-03：`append` 扩容时不拷旧元素 ───────────────────────────────
#
# 判据 `T-SLICE-06` 的**求值器半边**（`ssa_run::t_slice_06_append_扩容要拷旧元素`）：
# 扩容时新数组必须**先拷 `len` 个旧元素**再追加。不拷的话 `append` 之后前 `len`
# 个变成零值 —— 而 `len`/`cap` 全对，只有内容错，所以判据必须比内容。
#
# ⚠️ **别拿 `obj_slice` 当这条的判据**：它走 `Emit::Elf`（native 通路），
# `append_value` 根本不在那条路上 —— 第一版探针就是 `--test obj_slice`，
# 结果变异的居然是「绿」。而原生侧的同一条语义由 `rt.c` 的 append 实现，
# 是**另一份代码**。这条探针要打的是共同那一份。
printf '%s' '        if i < len {
            cells.push(cell(arr[off + i].borrow().deep_copy()));
        } else if i - len < extra.len() {' > "$TMP/c_old"
printf '%s' '        if i < len && false {
            cells.push(cell(arr[off + i].borrow().deep_copy()));
        } else if i - len < extra.len() {' > "$TMP/c_new"
probe "M-M9-03 append 扩容不拷旧元素" \
  crates/rgoc-interp/src/interp.rs "$TMP/c_old" "$TMP/c_new" \
  rgoc-interp -p rgoc-driver --test ssa_run

# ── M-M9-08：`rt_defer_push` 改成 FIFO ⇒ defer 按**正序**跑 ───────────
#
# 判据 `T-EXC-01`：三个 `defer` ⇒ 输出倒序（`3,2,1`）。`rt_deferrun` 从链头
# 弹（LIFO），所以登记要插在**链头**；改成插链尾就变成正序（`1,2,3`）。
#
# 这条的判据是 `native_corpus_run`（native 与解释器逐字节对撞）：语料里有
# 带 defer 的文件，正序会让 native 与解释器的输出分叉。
printf '%s' 'void rt_defer_push(void *fp, void *fv) {
    DeferRec *r = rt_arena_alloc(sizeof(DeferRec), 8);
    r->next = g_defer_top;
    r->fp = fp;
    r->fv = fv;
    g_defer_top = r;
}' > "$TMP/d_old"
printf '%s' 'void rt_defer_push(void *fp, void *fv) {
    DeferRec *r = rt_arena_alloc(sizeof(DeferRec), 8);
    r->next = 0;
    r->fp = fp;
    r->fv = fv;
    if (g_defer_top == 0) {
        g_defer_top = r;
    } else {
        DeferRec *p = g_defer_top;
        while (p->next != 0) {
            p = p->next;
        }
        p->next = r;
    }
}' > "$TMP/d_new"
probe "M-M9-08 rt_defer_push 改 FIFO（defer 正序）" \
  ../runtime/native/aarch64-unknown-linux-gnu/rt.c "$TMP/d_old" "$TMP/d_new" \
  - -p rgoc-driver --test native_corpus_run

# ── M-M9-04：`rt_map_del` 置 `empty` 而不是 `tomb` ⇒ 探测链断 ─────────
#
# 判据 `T-MAP-10`（`obj_map::t_map_10_删除不切断探测链`）。
#
# ⚠️ 这条判据**必须构造哈希碰撞**才有牙：`rt_hash` 是 FNV-1a（整数按 8 字节
# 小端），初表 `cap = 8`；`0` 与 `8` 的桶都是 5 ⇒ 先插 0 占 5 号、再插 8 落 6 号，
# 删 0 之后查 8 会经过 5 号槽。用「填 40 个 → 删偶数 → 再插 20 个」那种自然
# 形态实测**是绿的**（40 个键摊得太开，没一条链穿过被删的槽）。
printf '%s' '    slot[0] = MAP_TOMB;
' > "$TMP/e_old"
printf '%s' '    slot[0] = MAP_EMPTY;
' > "$TMP/e_new"
probe "M-M9-04 rt_map_del 置 empty（探测链断）" \
  ../runtime/native/aarch64-unknown-linux-gnu/rt.c "$TMP/e_old" "$TMP/e_new" \
  - -p rgoc-driver --test obj_map t_map_10

# ── M-M9-06：`MakeClosure` 只填第一个捕获 ─────────────────────────────
#
# 判据 `T-FUNC-05` 的求值器半边（`ssa_run::t_func_05_四个捕获都要打开`）。
# 捕获 1 号以后缺失 ⇒ 读 `CapturePtr` 当场报「捕获下标越界」。
printf '%s' '                for c in captures {
' > "$TMP/f_old"
printf '%s' '                for c in captures.iter().take(1) {
' > "$TMP/f_new"
probe "M-M9-06 闭包只填第一个捕获" \
  crates/rgoc-interp/src/ssa_exec.rs "$TMP/f_old" "$TMP/f_new" \
  rgoc-interp -p rgoc-driver --test ssa_run t_func_05

# ── M-M9-07：`check_panic` 不跳本帧出口 ───────────────────────────────
#
# 判据 `T-EXC-04`（`native_isel_cover::t_exc_04_逐层_defer_与_panic_之后调用方不再往下跑`）。
#
# ⚠️ **别拿语料当这条的探针**：69 个语料里的 panic 之后调用方本来就走出口，
# 去掉 `cbnz` 输出不变 ⇒ 绿。要的是「panic 之后还有可观察输出」的 fixture，
# 而且**三层**都要有（只有一层的话抓不到「中间层忘了查」）。
printf '%s' '    asm.ins("bl rt_panicking");
    asm.ins(&format!("cbnz x0, {}", panic_label(cx)));
' > "$TMP/g_old"
printf '%s' '    asm.ins("bl rt_panicking");
' > "$TMP/g_new"
probe "M-M9-07 check_panic 不跳出口" \
  crates/rgoc-arm64/src/isel.rs "$TMP/g_old" "$TMP/g_new" \
  rgoc-arm64 -p rgoc-driver --test native_isel_cover t_exc_04

# ── M-M9-11：`type_expr` 不认 `unsafe.Pointer`（M9 收口补的 `R-9-41`）──
#
# 判据 `G-M9-01` 的守门（`unsafe_ptr` 两条）。删掉那个 `Expr::Selector` 臂之后
# 签名里的 `unsafe.Pointer` 重新变 `invalid`，`bin Ne` 当场被 `V3` 拒。
printf '%s' '            Expr::Selector(s)
                if matches!(&*s.x, Expr::Name(n) if n.value == crate::unsafe_pkg::PKG_NAME)
                    && s.sel.value == "Pointer" =>
            {
                Some(self.s.tt.add_basic(BasicId::UnsafePointer))
            }
' > "$TMP/h_old"
printf '%s' '' > "$TMP/h_new"
probe "M-M9-11 type_expr 不认 unsafe.Pointer" \
  crates/rgoc-check/src/expr.rs "$TMP/h_old" "$TMP/h_new" \
  rgoc-check -p rgoc-driver --test unsafe_ptr

rm -rf "$TMP"

# ⚠️ 恢复源码之后**必须**让产物失效（同 mutation-m7.sh / -m8.sh 的理由）。
cargo clean -p rgoc-meta >/dev/null 2>&1
cargo clean -p rgoc-types >/dev/null 2>&1
cargo clean -p rgoc-interp >/dev/null 2>&1
cargo clean -p rgoc-check >/dev/null 2>&1
cargo clean -p rgoc-arm64 >/dev/null 2>&1
cargo clean -p rgoc-driver >/dev/null 2>&1

echo ""
echo "==== M9 变异汇总：红 $pass / 失手 $fail ===="
printf '%s\n' "$report"
[ "$fail" -eq 0 ]
