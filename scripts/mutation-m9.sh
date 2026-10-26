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
# # 这里只有 4 条探针
#
# 另外 6 条（05 ITab 填序 / 06 闭包只填一个捕获 / 07 `check_panic` 不查 /
# 09 `ptrs` 退回值粒度 / 10 回边不产 safepoint / 以及 08 的**另一种**改法）
# 已在执行期逐条实测变红，记录在 `scripts/mutation-m9-notes.md`。
# 这里保留的是**回归代价低**的 4 条（08 是例外：它要重编 69 个程序的 rt.o，
# 约 30 s —— 但它是唯一能证明「defer 逆序」这条判据有牙的探针，值得留着）。
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

rm -rf "$TMP"

# ⚠️ 恢复源码之后**必须**让产物失效（同 mutation-m7.sh / -m8.sh 的理由）。
cargo clean -p rgoc-meta >/dev/null 2>&1
cargo clean -p rgoc-types >/dev/null 2>&1
cargo clean -p rgoc-interp >/dev/null 2>&1
cargo clean -p rgoc-driver >/dev/null 2>&1

echo ""
echo "==== M9 变异汇总：红 $pass / 失手 $fail ===="
printf '%s\n' "$report"
[ "$fail" -eq 0 ]
