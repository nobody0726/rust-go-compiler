#!/usr/bin/env python3
"""scripts/shrink_driver.py —— `shrink-m8.sh` 的驱动（M8-plan Task 60，T-CON-61）。

职责只有一件：把「函数体里的语句」逐条（从后往前）删掉，删完仍失败就保留。

判据是**退出码相同**（不是「有输出」也不是「stderr 非空」）—— 这样不会把
panic（2）缩成编译不过（1）。

边界与理由见 `shrink-m8.sh` 的文档头；这里只实现「语句检测」与「贪心删除循环」。
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path


def candidates(lines: list[str]) -> list[tuple[int, int]]:
    """返回可删的语句区间 `(lo, hi)`（1-based，含端点），按出现顺序。

    规则：
    * 只在函数体里找（`func` 签名那行往后，到配对的花括号为止）。
    * 一条语句 = 一行；若该行（去空白后）以 `{` 结尾，则延伸到配对的 `}`。
    * `}` / `{` / 空行 / 纯注释行 / `case …:` / `default:` / 标签行不可删
      （删了会改变语法形状，不再是「同一个程序失败」）。
    """
    out: list[tuple[int, int]] = []
    i = 0
    n = len(lines)
    in_func = False
    depth = 0  # 相对函数体第一层的花括号深度
    while i < n:
        raw = lines[i]
        s = raw.strip()
        if not in_func:
            # 函数签名行：`func name(...) ... {` 或 `func (r T) M(...) {`
            if s.startswith("func ") and s.endswith("{"):
                in_func = True
                depth = 0
            i += 1
            continue
        # 在函数体里：先让本行的花括号影响深度
        opens = s.count("{")
        closes = s.count("}")
        if s == "}" and depth == 0:
            # 函数体结束
            in_func = False
            i += 1
            continue
        if (
            not s
            or s == "{"
            or s.startswith("//")
            or s.startswith("case ")
            or s == "default:"
            or s.endswith(":")  # 标签（`out:` / `L:`）—— 形状敏感，不删
        ):
            depth += opens - closes
            i += 1
            continue
        lo = i + 1  # 1-based
        if s.endswith("{"):
            # 延伸到配对的 `}`
            d = 0
            j = i
            while j < n:
                d += lines[j].count("{") - lines[j].count("}")
                if d <= 0:
                    break
                j += 1
            hi = j + 1
        else:
            hi = lo
        out.append((lo, hi))
        depth += opens - closes
        i += 1
    return out


def run(cmd: list[str], path: Path) -> int:
    args = [a.replace("@FILE@", str(path)) for a in cmd]
    try:
        p = subprocess.run(args, capture_output=True, text=True, timeout=60)
    except subprocess.TimeoutExpired:
        # 超时也算一种失败（死锁的另一种面貌）；用 124 与官方约定对齐。
        return 124
    return p.returncode


def default_bin() -> str:
    """默认跑 `rgoc-driver run`：优先用仓库里刚构建的那个（容器里它不在 PATH 上）。"""
    for p in (Path("target/debug/rgoc-driver"), Path("target/release/rgoc-driver")):
        if p.is_file():
            return str(p)
    return "rgoc-driver"


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    src = Path(sys.argv[1])
    rest = sys.argv[2:]
    if rest and rest[0] == "--":
        rest = rest[1:]
    cmd = rest or [default_bin(), "run", "@FILE@"]
    if not src.is_file():
        print(f"找不到 {src}", file=sys.stderr)
        return 2

    tmp = Path(tempfile.mkdtemp(prefix="shrink-m8-"))
    cur = tmp / "cur.go"
    cur.write_text(src.read_text(encoding="utf-8"), encoding="utf-8")

    base = run(cmd, cur)
    if base == 0:
        print("原始 fixture 是**成功**的（退出码 0）—— 没什么可缩。", file=sys.stderr)
        return 2
    print(f"== 原始失败：退出码 {base}")
    print(f"== 命令：{' '.join(cmd)}")

    total = 0
    for rnd in range(1, 17):
        lines = cur.read_text(encoding="utf-8").splitlines()
        deleted_this_round = 0
        for lo, hi in reversed(candidates(lines)):
            trial = lines[: lo - 1] + lines[hi:]
            cur.write_text("\n".join(trial) + "\n", encoding="utf-8")
            rc = run(cmd, cur)
            if rc == base:
                lines = trial
                deleted_this_round += 1
                total += 1
                print(f"  轮{rnd} 删 {lo}-{hi} ⇒ 仍失败（{rc}），保留")
            else:
                cur.write_text("\n".join(lines) + "\n", encoding="utf-8")
                print(f"  轮{rnd} 删 {lo}-{hi} ⇒ 退出码变 {rc}，还原")
        if deleted_this_round == 0:
            break

    out = src.with_suffix(src.suffix + ".min.go")
    out.write_text(cur.read_text(encoding="utf-8"), encoding="utf-8")
    left = len([l for l in cur.read_text(encoding="utf-8").splitlines() if l.strip()])
    print("")
    print(f"==== 缩减完成：删了 {total} 条语句，剩 {left} 个非空行")
    print(f"==== 最小复现：{out}")
    print("")
    print(cur.read_text(encoding="utf-8"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
