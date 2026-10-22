#!/bin/sh
# M5 候选语料筛选（`T-EX-41` 的「口径可重放」入口）。
#
# 用法（容器内，仓库根）：
#   sh tools/run-oracle/survey.sh              # 打印候选清单
#   sh tools/run-oracle/survey.sh --stats      # 分母漏斗
#   sh tools/run-oracle/survey.sh --diff tools/run-oracle/run-candidates.txt
#
# 逻辑全在 survey.py（本文件只是一层不依赖 bash 的包装：容器里 `sh` 一定有，
# `bash -lc` 由调用方给）。
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../.." && pwd)
cd "$root"
exec python3 "$here/survey.py" "$@"
