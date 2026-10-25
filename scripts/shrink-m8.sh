#!/bin/bash
# scripts/shrink-m8.sh —— 并发 fixture 的**语句级**缩减（`M8-plan.md` Task 60，`T-CON-61`）。
#
# 用法：
#   bash scripts/shrink-m8.sh <fixture.go>                  # 默认命令：rgoc-driver run <file>
#   bash scripts/shrink-m8.sh <fixture.go> -- <命令…>       # 命令里的 `@FILE@` 会被替换成当前候选文件
#
# 例子（把一条死锁 fixture 缩到最小）：
#   bash scripts/shrink-m8.sh target/tmp/x.go -- rgoc-driver run @FILE@
#
# # 做法
#
# 从后往前逐条删**函数体里的语句**：删完仍失败（退出码与最初那次失败**同一个**）
# 就保留这次删除，否则还原。一整轮没有任何删除就停。结果写到 `<fixture>.min.go`，
# 每一步都打到 stdout。
#
# # 边界（文档写清，**不做**语法树级简化）
#
# * 只删函数体里的语句。`package` / `import` / `func` 签名一行都不动 ——
#   动了就不是「同一个程序失败」而是「另一个程序编译不过」。
# * 一条语句占多行时，按「到配对 `}` 为止」整块删（花括号配对在 Python 侧做）。
# * 不重命名、不内联、不改字面量、不合并声明。那些是 delta-debugging 的活，
#   本脚本只做「整条语句删/不删」这一维。
# * 「仍失败」的判据是**退出码相同**。这保证不会把「panic（2）」缩成
#   「编译不过（1）」—— 那是另一个 bug 的最小复现，不是这个。
#
# # 实测下限：4 个非空行（`T-CON-61` 的如实记录）
#
# 对一条 13 行、两个 goroutine 的死锁 fixture 跑：删 5 条 ⇒ 剩 7 个非空行，
# 且**两个 goroutine 都被删掉了**（死锁不需要它们 —— main 自己的 `<-a` 就够）。
# 语句级删除的结构性下限是 4 个非空行：
#
#     package main
#     func main() {
#         <一条语句>
#     }
#
# 想再降到 3 行得把字面量内联进去（`<-make(chan int)`），那是本脚本**故意不做**的
# 一维。所以「≤3 行」这个目标在语句级删除下不可达 —— 记在这里，不假装达标。
set -uo pipefail

if [ $# -lt 1 ]; then
  sed -n '2,28p' "$0" >&2
  exit 2
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/rgoc" || { echo "进不去 $ROOT/rgoc" >&2; exit 2; }

# 驱动在 Python 里：语句检测（花括号配对）与「删了还失败吗」的循环用 bash 写
# 会和 echo/计数打架（踩过：$(one_pass) 把日志一起吃进去）。
exec python3 "$ROOT/scripts/shrink_driver.py" "$@"
