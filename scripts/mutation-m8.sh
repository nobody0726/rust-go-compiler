#!/bin/bash
# scripts/mutation-m8.sh —— M8 的变异探针（`M8-plan.md` Task 59）。
#
# 用法（容器内）：bash scripts/mutation-m8.sh
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
# 4. **恢复源码之后必须再 clean 一次**（见脚本末尾）：否则 `.rlib` 还是变异版，
#    下一次全量测试会红一批与变异无关的用例（`mutation-m3-notes.md` §0 实测踩过）。
#
# # 这里只有 4 条探针
#
# 另外 4 条（合成函数形参退回 v0 / `=` 形态不写回左值 / 死锁判定的 main_done 优先 /
# `next_token` 从 0 起）已在执行期逐条实测变红，记录在 `scripts/mutation-m8-notes.md`。
# 这里保留的是**回归代价低**的那 4 条，方便每次收口重跑。
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
  cp "$file" "$file.m8bak"
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
    mv "$file.m8bak" "$file"
    report="$report
  MISS  $name"
    fail=$((fail + 1))
    return
  fi

  cargo clean -p "$crate" >/dev/null 2>&1
  local out
  out="$(cargo test "$@" 2>&1 | grep -E '^test result' | tail -1)"
  mv "$file.m8bak" "$file"
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

# ① range-over-func 的 `continue` 不翻译 ⇒ 迭代在第一轮就结束（解释器的
#    `Branch{Continue}` 找不到循环会把 body 栈弹空 ⇒ 提前 return false）。
printf '%s' 'if kind == BranchKind::Continue && self.rf_translate_continue() {' > "$TMP/a_old"
printf '%s' 'if false && kind == BranchKind::Continue && self.rf_translate_continue() {' > "$TMP/a_new"
probe "M-M8-01 range-func 的 continue 不翻译" \
  crates/rgoc-hir/src/lower.rs "$TMP/a_old" "$TMP/a_new" \
  rgoc-hir -p rgoc-driver --test con_rangefunc

# ② select 子句体不走 `push_clause_seq` ⇒ 无标签 break 穿到外层 for 上
#    （官方 33，我们算出 1）。这是 M8 执行期抓到的一个**既有 bug**。
printf '%s' '            self.push_clause_seq(sh, fi, &c.body);' > "$TMP/b_old"
printf '%s' '            self.push_seq(sh, fi, &c.body);' > "$TMP/b_new"
probe "M-M8-02 select 子句体不拦 break" \
  crates/rgoc-interp/src/interp.rs "$TMP/b_old" "$TMP/b_new" \
  rgoc-interp -p rgoc-driver --test con_select

# ③ `try_send` 的 closed 检查挪到 buf 之后 ⇒ `close(c); c <- v`（cap>0 未满）
#    会静默把值塞进缓冲（官方 `chansend:224` 是锁后第一件事就查 closed）。
cat > "$TMP/c_old" <<'EOF'
        // ① 已关闭：**第一件事**（见模块头）。
        if self.closed {
            return SendOutcome::Closed;
        }
        // ② 有阻塞接收方：直接进它的 slot。
EOF
cat > "$TMP/c_new" <<'EOF'
        // ② 有阻塞接收方：直接进它的 slot。
EOF
# ⚠️ 目标必须是 `--test chan_obj`（集成测试），不是 `--lib`：
# `chan_obj.rs` 在 `crates/rgoc-interp/tests/` 下，`--lib` 只跑 `src/` 里的
# 单测 —— 第一版写成 `--lib`，判据一条都没跑到，探针「假绿」。
probe "M-M8-03 send 的 closed 检查整段删掉" \
  crates/rgoc-interp/src/chan.rs "$TMP/c_old" "$TMP/c_new" \
  rgoc-interp --test chan_obj

# ④ 死锁判定去掉 ⇒ nil 通道的 recv/send 变成「静静什么都不做」而不是退出 2。
printf '%s' '        if !self.main_done() && self.all_blocked() && self.gs.iter().any(Option::is_some) {
            self.halted = Some(Halt::Deadlock);
        }' > "$TMP/d_old"
printf '%s' '        if false {
            self.halted = Some(Halt::Deadlock);
        }' > "$TMP/d_new"
probe "M-M8-04 死锁判定不落地" \
  crates/rgoc-interp/src/interp.rs "$TMP/d_old" "$TMP/d_new" \
  rgoc-interp -p rgoc-driver --test con_exit

rm -rf "$TMP"

# ⚠️ 恢复源码之后**必须**让产物失效（同 mutation-m7.sh 的理由）。
cargo clean -p rgoc-hir >/dev/null 2>&1
cargo clean -p rgoc-interp >/dev/null 2>&1

echo ""
echo "==== M8 变异汇总：红 $pass / 失手 $fail ===="
printf '%s\n' "$report"
[ "$fail" -eq 0 ]
