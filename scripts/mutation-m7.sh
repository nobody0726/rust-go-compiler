#!/bin/bash
# scripts/mutation-m7.sh —— M7 的变异探针（`M7-plan.md` Task 60）。
#
# 用法（容器内）：bash scripts/mutation-m7.sh
#
# # 为什么变异必须实证
#
# 「测试全绿」有两种可能：**代码对**，或者**判据恒真**。区分它们的唯一办法是
# 把代码改坏一处，看判据会不会红。本仓已经抓到过 6+ 条恒真断言（`M3`/`M4` 的账）。
#
# # 纪律（M6 的血）
#
# 1. **变异前 `cargo clean -p <crate>`**：绑定挂载上 cargo 的 mtime 判据不可靠，
#    不 clean 会跑到旧产物上，得到「绿色」的假结论。
# 2. **同一调用内完成变异与恢复**：容器 `/tmp` 调用即焚，跨调用恢复不了。
# 3. 每条都必须看到 `test result: FAILED` 才算「变异有效」；全绿 = 判据没牙。
#
# # 这里只有 4 条探针
#
# 另外 4 条（并行拷贝顺序化 / `max_outgoing` 恒 0 / 移位不钳制 / 帧图 off 减 8）
# 已在执行期逐条实测变红，记录在 `scripts/mutation-m7-notes.md`。这里保留的是
# **回归代价低**的那 4 条，方便每次收口重跑。
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/rgoc" || { echo "进不去 $ROOT/rgoc" >&2; exit 2; }

pass=0
fail=0
report=""

# 用法：probe <名字> <文件> <旧串文件> <新串文件> <crate> <测试命令…>
# 用「文件传串」而不是把串写进 shell：带引号/换行的 Rust 代码在 shell 里转义
# 一次错一次（M6 踩过）。
probe() {
  local name="$1" file="$2" oldf="$3" newf="$4" crate="$5"
  shift 5
  cp "$file" "$file.m7bak"
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
    mv "$file.m7bak" "$file"
    report="$report
  MISS  $name"
    fail=$((fail + 1))
    return
  fi

  cargo clean -p "$crate" >/dev/null 2>&1
  local out
  out="$(cargo test "$@" 2>&1 | grep -E '^test result' | tail -1)"
  mv "$file.m7bak" "$file"
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

# ① `sub sp` 少减 16 ⇒ sp 比最低槽高，被调方的 `stp x29,x30,[sp,#-16]!` 盖住它。
printf '%s' 'self.frame_size.saturating_sub(16)' > "$TMP/a_old"
printf '%s' 'self.frame_size.saturating_sub(32)' > "$TMP/a_new"
probe "M-M7-01 sub_sp_imm 少 16（帧太小）" \
  crates/rgoc-arm64/src/frame.rs "$TMP/a_old" "$TMP/a_new" \
  rgoc-arm64 --test frame_layout

# ② 整数寄存器阈值 8→6（AAPCS64 是 x0..x7）。调用方与被调方同表 ⇒ 行为仍自洽，
#    所以只有**直接断表**的用例抓得住 —— 这正是 `place_args_*` 存在的理由。
printf '%s' 'if xi + need <= 8 {' > "$TMP/b_old"
printf '%s' 'if xi + need <= 6 {' > "$TMP/b_new"
probe "M-M7-02 place_args 整数阈值 8→6" \
  crates/rgoc-arm64/src/abi.rs "$TMP/b_old" "$TMP/b_new" \
  rgoc-arm64 --test abi_classify

# ③ 形参只存一个字：`string` 丢 len。
cat > "$TMP/c_old" <<'EOF'
                    st_x(asm, &format!("x{reg}"), s.off);
                    if n > 1 {
                        st_x(asm, &format!("x{}", reg + 1), s.off + 8);
                    }
EOF
cat > "$TMP/c_new" <<'EOF'
                    st_x(asm, &format!("x{reg}"), s.off);
EOF
probe "M-M7-03 bind_params 丢了 string 的第二个字" \
  crates/rgoc-arm64/src/isel.rs "$TMP/c_old" "$TMP/c_new" \
  rgoc-driver --test native_args

# ④ 字符串比较退回指针比较（静默错：内容相同但地址不同）。
printf '%s' '    if is_string_ty(cx.tt, cx.ty(lhs)) {' > "$TMP/d_old"
printf '%s' '    if false && is_string_ty(cx.tt, cx.ty(lhs)) {' > "$TMP/d_new"
probe "M-M7-04 字符串比较退回指针比较" \
  crates/rgoc-arm64/src/isel.rs "$TMP/d_old" "$TMP/d_new" \
  rgoc-driver --test native_corpus_run

rm -rf "$TMP"

# ⚠️ 恢复源码之后**必须**让产物失效。
#
# 绑定挂载上 cargo 的 mtime 判据不可靠（`mutation-m3-notes.md` §0）：源码已经恢复，
# 但 `.rlib` 还是变异版 —— 下一次 `cargo test` 跑在「源码对、库错」的混合状态上，
# 表现为一堆**与变异无关**的用例变红。实测踩过：跑完 4 条探针后全量测试红了 2 条，
# 而 `git status` 显示工作区干净。
cargo clean -p rgoc-arm64 >/dev/null 2>&1

echo ""
echo "==== M7 变异汇总：红 $pass / 失手 $fail ===="
printf '%s\n' "$report"
[ "$fail" -eq 0 ]
